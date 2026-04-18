# inspectah architect Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `inspectah architect` subcommand that loads multiple refined fleet tarballs, proposes a base+derived layer topology, and serves an interactive PatternFly 6 web UI with click-to-move package reassignment and Containerfile tree export.

**Architecture:** New `src/inspectah/architect/` package following the fleet/refine patterns. Loader discovers refined tarballs, analyzer computes 100% prevalence split, server exposes topology/move/export APIs via BaseHTTPRequestHandler, Jinja2+PatternFly 6 templates provide the interactive UI. Driftify gets multi-fleet fixture generation as a prerequisite.

**Tech Stack:** Python 3.11+, dataclasses, argparse, BaseHTTPRequestHandler, Jinja2, PatternFly 6 CSS (bundled), pytest. Driftify is stdlib-only Python.

**Spec:** `docs/specs/proposed/2026-03-29-architect-design.md`

**Project conventions:** `AGENTS.md` at workspace root. Conventional commits, `Assisted-by:` attribution, TDD, no AI slop. Two separate git repos: `inspectah/` and `driftify/`.

---

## File Map

### Driftify (separate repo: `driftify/`)

| Action | File | Responsibility |
|--------|------|----------------|
| Modify | `driftify/driftify.py` | Add multi-fleet profile sets and exclusive package support |
| Create | `driftify/tests/test_multi_fleet.py` | Tests for new fleet topology generation |

### Inspectah (repo: `inspectah/`)

| Action | File | Responsibility |
|--------|------|----------------|
| Create | `src/inspectah/architect/__init__.py` | Package marker |
| Create | `src/inspectah/architect/cli.py` | CLI subcommand registration |
| Create | `src/inspectah/architect/loader.py` | Load refined fleet tarballs |
| Create | `src/inspectah/architect/analyzer.py` | Cross-fleet analysis + LayerTopology model |
| Create | `src/inspectah/architect/server.py` | HTTP server with API routes |
| Create | `src/inspectah/architect/export.py` | Containerfile tree + build.sh generation |
| Create | `src/inspectah/templates/architect/architect.html.j2` | Main template |
| Create | `src/inspectah/templates/architect/_css.html.j2` | Custom CSS |
| Create | `src/inspectah/templates/architect/_js.html.j2` | Interactive JS |
| Modify | `src/inspectah/cli.py` | Register architect subcommand |
| Create | `tests/test_architect_loader.py` | Loader tests |
| Create | `tests/test_architect_analyzer.py` | Analyzer tests |
| Create | `tests/test_architect_server.py` | Server API tests |
| Create | `tests/test_architect_export.py` | Export tests |

---

### Task 1: Driftify — Multi-Fleet Fixture Support

**Context:** Driftify currently uses cumulative profiles (minimal → standard → kitchen-sink). For architect, we need to generate hosts across separate fleets where packages differ BETWEEN fleets but are identical WITHIN a fleet. This task adds a new mode to driftify that generates fleet-ready host sets.

**Files:**
- Modify: `driftify/driftify.py`
- Create: `driftify/tests/test_multi_fleet.py`

- [ ] **Step 1: Write tests for multi-fleet fixture generation**

Create `driftify/tests/test_multi_fleet.py`:

```python
"""Tests for multi-fleet topology fixture generation."""

import json
import pytest
from pathlib import Path

# Import will work after implementation
from driftify import FLEET_TOPOLOGIES, generate_fleet_topology


class TestFleetTopologies:
    def test_three_role_overlap_topology_exists(self):
        assert "three-role-overlap" in FLEET_TOPOLOGIES

    def test_hardware_split_topology_exists(self):
        assert "hardware-split" in FLEET_TOPOLOGIES

    def test_three_role_overlap_has_three_fleets(self):
        topo = FLEET_TOPOLOGIES["three-role-overlap"]
        assert len(topo["fleets"]) == 3

    def test_hardware_split_has_two_fleets(self):
        topo = FLEET_TOPOLOGIES["hardware-split"]
        assert len(topo["fleets"]) == 2

    def test_each_fleet_has_hosts(self):
        for name, topo in FLEET_TOPOLOGIES.items():
            for fleet in topo["fleets"]:
                assert len(fleet["hosts"]) >= 3, f"{name}/{fleet['name']} needs 3+ hosts"

    def test_fleets_have_shared_packages(self):
        topo = FLEET_TOPOLOGIES["three-role-overlap"]
        all_pkg_sets = [set(f["shared_packages"] + f["exclusive_packages"]) for f in topo["fleets"]]
        shared = all_pkg_sets[0]
        for s in all_pkg_sets[1:]:
            shared = shared & s
        # Should have significant overlap from shared_packages
        assert len(shared) > 10

    def test_fleets_have_exclusive_packages(self):
        topo = FLEET_TOPOLOGIES["three-role-overlap"]
        for fleet in topo["fleets"]:
            assert len(fleet["exclusive_packages"]) >= 3, f"{fleet['name']} needs exclusive pkgs"

    def test_exclusive_packages_dont_overlap(self):
        topo = FLEET_TOPOLOGIES["three-role-overlap"]
        exclusive_sets = [set(f["exclusive_packages"]) for f in topo["fleets"]]
        for i, s1 in enumerate(exclusive_sets):
            for j, s2 in enumerate(exclusive_sets):
                if i != j:
                    overlap = s1 & s2
                    assert not overlap, f"Fleets {i} and {j} share exclusive pkg: {overlap}"


class TestGenerateFleetTopology:
    def test_generates_output_directory(self, tmp_path):
        generate_fleet_topology("three-role-overlap", tmp_path)
        # Should create subdirectories per fleet
        fleet_dirs = [d for d in tmp_path.iterdir() if d.is_dir()]
        assert len(fleet_dirs) == 3

    def test_each_fleet_dir_has_host_snapshots(self, tmp_path):
        generate_fleet_topology("three-role-overlap", tmp_path)
        for fleet_dir in tmp_path.iterdir():
            if not fleet_dir.is_dir():
                continue
            json_files = list(fleet_dir.glob("*.json"))
            assert len(json_files) >= 3

    def test_hosts_within_fleet_share_packages(self, tmp_path):
        generate_fleet_topology("three-role-overlap", tmp_path)
        for fleet_dir in tmp_path.iterdir():
            if not fleet_dir.is_dir():
                continue
            snapshots = []
            for f in fleet_dir.glob("*.json"):
                snapshots.append(json.loads(f.read_text()))
            if len(snapshots) < 2:
                continue
            # All hosts in a fleet should have the same packages
            pkg_sets = []
            for snap in snapshots:
                pkgs = {p["name"] for p in snap.get("rpm", {}).get("packages_added", [])}
                pkg_sets.append(pkgs)
            for ps in pkg_sets[1:]:
                assert ps == pkg_sets[0], f"Hosts in {fleet_dir.name} have different packages"

    def test_hosts_have_different_hostnames(self, tmp_path):
        generate_fleet_topology("three-role-overlap", tmp_path)
        for fleet_dir in tmp_path.iterdir():
            if not fleet_dir.is_dir():
                continue
            hostnames = set()
            for f in fleet_dir.glob("*.json"):
                snap = json.loads(f.read_text())
                hostnames.add(snap["meta"]["hostname"])
            assert len(hostnames) >= 3

    def test_invalid_topology_name_raises(self):
        with pytest.raises(ValueError, match="Unknown topology"):
            generate_fleet_topology("nonexistent", Path("/tmp"))
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/driftify && python -m pytest tests/test_multi_fleet.py -v`
Expected: FAIL — `ImportError` (FLEET_TOPOLOGIES and generate_fleet_topology don't exist)

- [ ] **Step 3: Implement fleet topology data and generator**

Add to `driftify/driftify.py` — the topology definitions and generator function. Place near the top of the file after existing profile constants:

```python
# --- Multi-Fleet Topology Fixtures ---
# Used by inspectah architect to demonstrate cross-fleet layer decomposition.
# Each topology defines multiple fleets with shared + exclusive packages.
# Hosts within a fleet are identical (different hostnames only) — simulates
# the post-fleet+refine "cooking show" state.

_SHARED_BASE_PACKAGES = [
    "bash-5.1.8-9.el9.x86_64",
    "coreutils-8.32-35.el9.x86_64",
    "systemd-252-32.el9.x86_64",
    "openssl-3.0.7-27.el9.x86_64",
    "kernel-5.14.0-427.el9.x86_64",
    "glibc-2.34-83.el9.x86_64",
    "vim-enhanced-9.0.2153-1.el9.x86_64",
    "tmux-3.2a-5.el9.x86_64",
    "rsync-3.2.3-19.el9.x86_64",
    "lsof-4.96.4-1.el9.x86_64",
    "tcpdump-4.99.0-9.el9.x86_64",
    "net-tools-2.0-0.62.el9.x86_64",
    "curl-7.76.1-29.el9.x86_64",
    "wget-1.21.1-8.el9.x86_64",
    "tar-1.34-7.el9.x86_64",
    "gzip-1.12-1.el9.x86_64",
    "sudo-1.9.5p2-10.el9.x86_64",
    "openssh-server-8.7p1-38.el9.x86_64",
    "openssh-clients-8.7p1-38.el9.x86_64",
    "firewalld-1.3.4-1.el9.x86_64",
    "audit-3.1.2-2.el9.x86_64",
    "aide-0.16.15-13.el9.x86_64",
    "chrony-4.5-1.el9.x86_64",
    "cronie-1.5.7-10.el9.x86_64",
    "logrotate-3.21.0-1.el9.x86_64",
    "policycoreutils-3.6-2.1.el9.x86_64",
    "selinux-policy-targeted-38.1.35-2.el9.noarch",
    "dnf-4.18.0-3.el9.noarch",
    "rpm-4.16.1.3-29.el9.x86_64",
    "yum-utils-4.3.0-13.el9.noarch",
]

FLEET_TOPOLOGIES = {
    "three-role-overlap": {
        "description": "Three fleets (~85% shared base) with role-specific packages",
        "fleets": [
            {
                "name": "web-servers",
                "hosts": ["web-01", "web-02", "web-03"],
                "shared_packages": list(_SHARED_BASE_PACKAGES),
                "exclusive_packages": [
                    "httpd-2.4.57-5.el9.x86_64",
                    "mod_ssl-2.4.57-5.el9.x86_64",
                    "php-8.0.30-1.el9.x86_64",
                    "php-fpm-8.0.30-1.el9.x86_64",
                    "mod_security-2.9.7-1.el9.x86_64",
                ],
            },
            {
                "name": "db-servers",
                "hosts": ["db-01", "db-02", "db-03", "db-04"],
                "shared_packages": list(_SHARED_BASE_PACKAGES),
                "exclusive_packages": [
                    "postgresql-server-15.4-1.el9.x86_64",
                    "postgresql-contrib-15.4-1.el9.x86_64",
                    "pg_stat_statements-15.4-1.el9.x86_64",
                    "pgaudit-1.7.0-1.el9.x86_64",
                ],
            },
            {
                "name": "app-servers",
                "hosts": ["app-01", "app-02", "app-03"],
                "shared_packages": list(_SHARED_BASE_PACKAGES),
                "exclusive_packages": [
                    "python3.11-3.11.7-1.el9.x86_64",
                    "python3.11-pip-22.3.1-4.el9.noarch",
                    "redis-7.0.12-1.el9.x86_64",
                    "gunicorn-21.2.0-1.el9.noarch",
                ],
            },
        ],
    },
    "hardware-split": {
        "description": "Two fleets (~90% shared) with GPU hardware split",
        "fleets": [
            {
                "name": "standard-compute",
                "hosts": ["std-01", "std-02", "std-03"],
                "shared_packages": list(_SHARED_BASE_PACKAGES),
                "exclusive_packages": [],
            },
            {
                "name": "gpu-nodes",
                "hosts": ["gpu-01", "gpu-02", "gpu-03"],
                "shared_packages": list(_SHARED_BASE_PACKAGES),
                "exclusive_packages": [
                    "nvidia-driver-550.54.14-1.el9.x86_64",
                    "cuda-toolkit-12-3-12.3.2-1.x86_64",
                    "kmod-nvidia-550.54.14-1.el9.x86_64",
                    "nvidia-persistenced-550.54.14-1.el9.x86_64",
                ],
            },
        ],
    },
}


def generate_fleet_topology(topology_name: str, output_dir: Path) -> None:
    """Generate inspection snapshot JSON files for a multi-fleet topology.

    Creates one subdirectory per fleet, each containing N host snapshot JSON files.
    Hosts within a fleet have identical packages but different hostnames.
    """
    if topology_name not in FLEET_TOPOLOGIES:
        raise ValueError(f"Unknown topology: {topology_name!r}. Available: {list(FLEET_TOPOLOGIES)}")

    topo = FLEET_TOPOLOGIES[topology_name]
    for fleet in topo["fleets"]:
        fleet_dir = output_dir / fleet["name"]
        fleet_dir.mkdir(parents=True, exist_ok=True)

        all_packages = fleet["shared_packages"] + fleet["exclusive_packages"]

        for hostname in fleet["hosts"]:
            snapshot = {
                "schema_version": 6,
                "meta": {
                    "hostname": hostname,
                    "timestamp": "2026-03-29T00:00:00Z",
                    "profile": "fleet-fixture",
                },
                "os_release": {
                    "name": "Red Hat Enterprise Linux",
                    "version_id": "9.4",
                    "id": "rhel",
                },
                "rpm": {
                    "base_image": "registry.redhat.io/rhel9/rhel-bootc:9.4",
                    "packages_added": [
                        {
                            "name": pkg.rsplit("-", 2)[0] if pkg.count("-") >= 2 else pkg,
                            "nvra": pkg,
                            "source": "dnf",
                        }
                        for pkg in all_packages
                    ],
                    "packages_removed": [],
                },
                "config": {"files": []},
                "services": {"enabled": [], "disabled": []},
            }
            snap_path = fleet_dir / f"{hostname}.json"
            snap_path.write_text(json.dumps(snapshot, indent=2))
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/driftify && python -m pytest tests/test_multi_fleet.py -v`
Expected: All PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/driftify
git add driftify.py tests/test_multi_fleet.py
git commit -m "feat: Add multi-fleet topology fixture generation for architect

Adds FLEET_TOPOLOGIES data and generate_fleet_topology() function that
creates per-fleet directories of host snapshot JSONs. Hosts within a
fleet have identical packages but different hostnames, simulating
post-refine converged state. Two topologies: three-role-overlap (3 fleets,
~85% shared base) and hardware-split (2 fleets, GPU hardware axis).

Assisted-by: Claude Code (opus)"
```

---

### Task 2: Architect — Data Model + Analyzer

**Context:** Core decomposition logic. Takes loaded fleet data, builds cross-fleet package index, applies 100% prevalence heuristic. This is the analytical heart of architect.

**Files:**
- Create: `src/inspectah/architect/__init__.py`
- Create: `src/inspectah/architect/analyzer.py`
- Create: `tests/test_architect_analyzer.py`

- [ ] **Step 1: Create package marker**

Create `src/inspectah/architect/__init__.py`:

```python
"""inspectah architect — layer topology planner for multi-fleet decomposition."""
```

- [ ] **Step 2: Write failing tests for the analyzer**

Create `tests/test_architect_analyzer.py`:

```python
"""Tests for architect cross-fleet analyzer."""

import pytest

from inspectah.architect.analyzer import (
    FleetInput,
    Layer,
    LayerTopology,
    analyze_fleets,
)


def _make_fleet(name: str, packages: list[str], host_count: int = 3) -> FleetInput:
    return FleetInput(name=name, packages=packages, configs=[], host_count=host_count)


class TestAnalyzeFleets:
    def test_single_fleet_no_base_extraction(self):
        fleets = [_make_fleet("web", ["httpd", "openssl", "bash"])]
        topo = analyze_fleets(fleets)
        # Single fleet should produce no base layer
        assert len(topo.layers) == 1
        assert topo.layers[0].name == "web"
        assert topo.layers[0].parent is None

    def test_two_fleets_common_packages_go_to_base(self):
        fleets = [
            _make_fleet("web", ["httpd", "openssl", "bash"]),
            _make_fleet("db", ["postgresql", "openssl", "bash"]),
        ]
        topo = analyze_fleets(fleets)
        base = topo.get_layer("base")
        assert base is not None
        assert set(base.packages) == {"openssl", "bash"}
        assert base.parent is None

    def test_two_fleets_exclusive_packages_go_to_derived(self):
        fleets = [
            _make_fleet("web", ["httpd", "openssl", "bash"]),
            _make_fleet("db", ["postgresql", "openssl", "bash"]),
        ]
        topo = analyze_fleets(fleets)
        web = topo.get_layer("web")
        db = topo.get_layer("db")
        assert web is not None and "httpd" in web.packages
        assert db is not None and "postgresql" in db.packages
        assert web.parent == "base"
        assert db.parent == "base"

    def test_package_in_some_fleets_duplicated_to_each(self):
        fleets = [
            _make_fleet("web", ["httpd", "openssl", "bash", "curl"]),
            _make_fleet("db", ["postgresql", "openssl", "bash", "curl"]),
            _make_fleet("gpu", ["nvidia", "openssl", "bash"]),
        ]
        topo = analyze_fleets(fleets)
        base = topo.get_layer("base")
        # curl is in 2/3 fleets — NOT in base, duplicated to web and db
        assert "curl" not in base.packages
        assert "curl" in topo.get_layer("web").packages
        assert "curl" in topo.get_layer("db").packages

    def test_all_packages_shared_everything_in_base(self):
        fleets = [
            _make_fleet("web", ["openssl", "bash"]),
            _make_fleet("db", ["openssl", "bash"]),
        ]
        topo = analyze_fleets(fleets)
        base = topo.get_layer("base")
        assert set(base.packages) == {"openssl", "bash"}
        # Derived layers exist but are empty
        assert topo.get_layer("web").packages == []
        assert topo.get_layer("db").packages == []

    def test_no_overlap_empty_base(self):
        fleets = [
            _make_fleet("web", ["httpd"]),
            _make_fleet("db", ["postgresql"]),
        ]
        topo = analyze_fleets(fleets)
        base = topo.get_layer("base")
        assert base.packages == []

    def test_fan_out_computed(self):
        fleets = [
            _make_fleet("web", ["httpd", "bash"]),
            _make_fleet("db", ["postgresql", "bash"]),
            _make_fleet("gpu", ["nvidia", "bash"]),
        ]
        topo = analyze_fleets(fleets)
        base = topo.get_layer("base")
        assert base.fan_out == 3  # three derived layers

    def test_turbulence_computed_with_floor(self):
        fleets = [
            _make_fleet("web", ["httpd", "bash"]),
            _make_fleet("db", ["postgresql", "bash"]),
        ]
        topo = analyze_fleets(fleets)
        web = topo.get_layer("web")
        # 1 package, fan_out=0 → formula gives 0, but floor is 1.0 for non-base
        assert web.turbulence >= 1.0


class TestMovePackage:
    def test_move_between_derived_layers(self):
        fleets = [
            _make_fleet("web", ["httpd", "openssl", "bash"]),
            _make_fleet("db", ["postgresql", "openssl", "bash"]),
        ]
        topo = analyze_fleets(fleets)
        topo.move_package("httpd", "web", "db")
        assert "httpd" not in topo.get_layer("web").packages
        assert "httpd" in topo.get_layer("db").packages

    def test_move_from_base_broadcasts_to_all_derived(self):
        fleets = [
            _make_fleet("web", ["httpd", "openssl", "bash"]),
            _make_fleet("db", ["postgresql", "openssl", "bash"]),
        ]
        topo = analyze_fleets(fleets)
        topo.move_package("openssl", "base", "web")
        assert "openssl" not in topo.get_layer("base").packages
        # Broadcast: openssl should be in ALL derived layers
        assert "openssl" in topo.get_layer("web").packages
        assert "openssl" in topo.get_layer("db").packages

    def test_move_updates_turbulence(self):
        fleets = [
            _make_fleet("web", ["httpd", "openssl", "bash"]),
            _make_fleet("db", ["postgresql", "openssl", "bash"]),
        ]
        topo = analyze_fleets(fleets)
        old_turbulence = topo.get_layer("base").turbulence
        topo.move_package("openssl", "base", "web")
        new_turbulence = topo.get_layer("base").turbulence
        assert new_turbulence != old_turbulence  # should change since pkg count changed

    def test_move_nonexistent_package_raises(self):
        fleets = [
            _make_fleet("web", ["httpd", "bash"]),
            _make_fleet("db", ["postgresql", "bash"]),
        ]
        topo = analyze_fleets(fleets)
        with pytest.raises(ValueError, match="not found"):
            topo.move_package("nonexistent", "base", "web")


class TestTopologyJson:
    def test_to_json_roundtrip(self):
        fleets = [
            _make_fleet("web", ["httpd", "openssl", "bash"]),
            _make_fleet("db", ["postgresql", "openssl", "bash"]),
        ]
        topo = analyze_fleets(fleets)
        data = topo.to_dict()
        assert "layers" in data
        assert "fleets" in data
        assert len(data["layers"]) == 3  # base + 2 derived
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && python -m pytest tests/test_architect_analyzer.py -v`
Expected: FAIL — `ImportError`

- [ ] **Step 4: Implement the analyzer**

Create `src/inspectah/architect/analyzer.py`:

```python
"""Cross-fleet analyzer for layer topology decomposition."""

from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass, field


@dataclass
class FleetInput:
    """Simplified fleet data for analysis."""

    name: str
    packages: list[str]
    configs: list[str]
    host_count: int = 0


@dataclass
class FleetInfo:
    """Fleet metadata for the topology."""

    name: str
    host_count: int
    total_packages: int


@dataclass
class Layer:
    """A layer in the image topology."""

    name: str
    parent: str | None
    packages: list[str] = field(default_factory=list)
    configs: list[str] = field(default_factory=list)
    fleets: list[str] = field(default_factory=list)
    fan_out: int = 0
    turbulence: float = 0.0

    def _recalc_turbulence(self) -> None:
        raw = self.fan_out * (len(self.packages) / 50.0)
        if self.parent is not None:  # non-base layers get floor of 1.0
            self.turbulence = max(1.0, raw)
        else:
            self.turbulence = raw


@dataclass
class LayerTopology:
    """Complete layer topology with move and export support."""

    layers: list[Layer] = field(default_factory=list)
    fleets: list[FleetInfo] = field(default_factory=list)

    def get_layer(self, name: str) -> Layer | None:
        for layer in self.layers:
            if layer.name == name:
                return layer
        return None

    def move_package(self, package: str, from_layer: str, to_layer: str) -> None:
        """Move a package between layers.

        Standard move: remove from from_layer, add to to_layer.
        Special case — moving FROM base: package is broadcast to ALL
        derived layers (every fleet still needs it). The to_layer parameter
        identifies the user's chosen target; broadcast is a side effect.
        """
        src = self.get_layer(from_layer)
        dst = self.get_layer(to_layer)
        if src is None:
            raise ValueError(f"Layer {from_layer!r} not found")
        if dst is None:
            raise ValueError(f"Layer {to_layer!r} not found")
        if package not in src.packages:
            raise ValueError(f"Package {package!r} not found in layer {from_layer!r}")

        src.packages.remove(package)

        if from_layer == "base":
            # Broadcast to ALL derived layers
            for layer in self.layers:
                if layer.parent is not None and package not in layer.packages:
                    layer.packages.append(package)
        else:
            if package not in dst.packages:
                dst.packages.append(package)

        self._recalc_all()

    def _recalc_all(self) -> None:
        base = self.get_layer("base")
        if base is not None:
            base.fan_out = sum(1 for l in self.layers if l.parent == "base")
            base._recalc_turbulence()
        for layer in self.layers:
            if layer.parent is not None:
                layer.fan_out = sum(1 for l in self.layers if l.parent == layer.name)
                layer._recalc_turbulence()

    def to_dict(self) -> dict:
        return {
            "layers": [
                {
                    "name": l.name,
                    "parent": l.parent,
                    "packages": l.packages,
                    "configs": l.configs,
                    "fleets": l.fleets,
                    "fan_out": l.fan_out,
                    "turbulence": round(l.turbulence, 1),
                }
                for l in self.layers
            ],
            "fleets": [
                {"name": f.name, "host_count": f.host_count, "total_packages": f.total_packages}
                for f in self.fleets
            ],
        }


def analyze_fleets(fleets: list[FleetInput]) -> LayerTopology:
    """Analyze multiple fleets and produce a layer topology.

    Uses 100% cross-fleet prevalence heuristic: packages in ALL fleets → base.
    Remaining packages stay in their fleet's derived layer.
    Configs always stay with their original fleet (not decomposed).
    Single fleet → no base extraction (fleet becomes the only layer).
    """
    fleet_names = [f.name for f in fleets]
    fleet_infos = [
        FleetInfo(name=f.name, host_count=f.host_count, total_packages=len(f.packages))
        for f in fleets
    ]

    if len(fleets) == 1:
        f = fleets[0]
        layer = Layer(
            name=f.name,
            parent=None,
            packages=list(f.packages),
            configs=list(f.configs),
            fleets=[f.name],
        )
        layer._recalc_turbulence()
        return LayerTopology(layers=[layer], fleets=fleet_infos)

    # Build cross-fleet package index
    pkg_to_fleets: dict[str, set[str]] = defaultdict(set)
    for f in fleets:
        for pkg in f.packages:
            pkg_to_fleets[pkg].add(f.name)

    all_fleet_names = set(fleet_names)
    base_packages = sorted(pkg for pkg, f_set in pkg_to_fleets.items() if f_set == all_fleet_names)

    # Build layers
    base = Layer(name="base", parent=None, packages=base_packages, fleets=fleet_names)

    derived_layers = []
    for f in fleets:
        derived_packages = sorted(pkg for pkg in f.packages if pkg not in base_packages)
        derived = Layer(
            name=f.name,
            parent="base",
            packages=derived_packages,
            configs=list(f.configs),
            fleets=[f.name],
        )
        derived_layers.append(derived)

    layers = [base] + derived_layers
    topo = LayerTopology(layers=layers, fleets=fleet_infos)
    topo._recalc_all()
    return topo
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && python -m pytest tests/test_architect_analyzer.py -v`
Expected: All PASS

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add src/inspectah/architect/__init__.py src/inspectah/architect/analyzer.py tests/test_architect_analyzer.py
git commit -m "feat(architect): Add layer topology analyzer with cross-fleet decomposition

Implements FleetInput, Layer, LayerTopology dataclasses and analyze_fleets()
function. Uses 100% prevalence heuristic for base extraction. Packages in
some-but-not-all fleets are duplicated to each derived layer. Move from base
broadcasts to all derived layers. Single fleet skips base extraction.

Assisted-by: Claude Code (opus)"
```

---

### Task 3: Architect — Loader

**Context:** Reads refined fleet tarballs from a directory, extracts the InspectionSnapshot from each, and converts to FleetInput for the analyzer.

**Files:**
- Create: `src/inspectah/architect/loader.py`
- Create: `tests/test_architect_loader.py`

- [ ] **Step 1: Write failing tests**

Create `tests/test_architect_loader.py`:

```python
"""Tests for architect fleet tarball loader."""

import json
import tarfile
import pytest
from io import BytesIO
from pathlib import Path

from inspectah.architect.loader import load_refined_fleets
from inspectah.architect.analyzer import FleetInput


def _make_snapshot(hostname: str, packages: list[str], fleet_name: str = "test-fleet") -> dict:
    return {
        "schema_version": 6,
        "meta": {
            "hostname": hostname,
            "fleet": {
                "source_hosts": [hostname],
                "total_hosts": 3,
            },
        },
        "os_release": {"name": "RHEL", "version_id": "9.4", "id": "rhel"},
        "rpm": {
            "base_image": "registry.redhat.io/rhel9/rhel-bootc:9.4",
            "packages_added": [
                {"name": pkg, "nvra": f"{pkg}-1.0-1.el9.x86_64", "source": "dnf"}
                for pkg in packages
            ],
        },
        "config": {"files": [{"path": "/etc/test.conf", "content": "test"}]},
    }


def _write_tarball(directory: Path, name: str, snapshot: dict) -> Path:
    tarball_path = directory / f"{name}.tar.gz"
    snap_json = json.dumps(snapshot).encode()
    with tarfile.open(tarball_path, "w:gz") as tar:
        info = tarfile.TarInfo(name="inspection-snapshot.json")
        info.size = len(snap_json)
        tar.addfile(info, BytesIO(snap_json))
    return tarball_path


class TestLoadRefinedFleets:
    def test_loads_tarballs_from_directory(self, tmp_path):
        snap1 = _make_snapshot("web-fleet", ["httpd", "openssl"])
        snap2 = _make_snapshot("db-fleet", ["postgresql", "openssl"])
        _write_tarball(tmp_path, "web-fleet", snap1)
        _write_tarball(tmp_path, "db-fleet", snap2)

        fleets = load_refined_fleets(tmp_path)
        assert len(fleets) == 2
        assert all(isinstance(f, FleetInput) for f in fleets)

    def test_fleet_name_from_hostname(self, tmp_path):
        snap = _make_snapshot("web-servers", ["httpd"])
        _write_tarball(tmp_path, "web-servers", snap)

        fleets = load_refined_fleets(tmp_path)
        assert fleets[0].name == "web-servers"

    def test_packages_extracted(self, tmp_path):
        snap = _make_snapshot("web", ["httpd", "openssl", "bash"])
        _write_tarball(tmp_path, "web", snap)

        fleets = load_refined_fleets(tmp_path)
        assert set(fleets[0].packages) == {"httpd-1.0-1.el9.x86_64", "openssl-1.0-1.el9.x86_64", "bash-1.0-1.el9.x86_64"}

    def test_host_count_from_fleet_meta(self, tmp_path):
        snap = _make_snapshot("web", ["httpd"])
        snap["meta"]["fleet"]["total_hosts"] = 42
        _write_tarball(tmp_path, "web", snap)

        fleets = load_refined_fleets(tmp_path)
        assert fleets[0].host_count == 42

    def test_configs_extracted(self, tmp_path):
        snap = _make_snapshot("web", ["httpd"])
        _write_tarball(tmp_path, "web", snap)

        fleets = load_refined_fleets(tmp_path)
        assert len(fleets[0].configs) >= 1

    def test_empty_directory(self, tmp_path):
        fleets = load_refined_fleets(tmp_path)
        assert fleets == []

    def test_skips_non_tarball_files(self, tmp_path):
        (tmp_path / "readme.md").write_text("not a tarball")
        snap = _make_snapshot("web", ["httpd"])
        _write_tarball(tmp_path, "web", snap)

        fleets = load_refined_fleets(tmp_path)
        assert len(fleets) == 1
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && python -m pytest tests/test_architect_loader.py -v`
Expected: FAIL — `ImportError`

- [ ] **Step 3: Implement the loader**

Create `src/inspectah/architect/loader.py`:

```python
"""Load refined fleet tarballs for architect analysis."""

from __future__ import annotations

import json
import logging
import tarfile
from pathlib import Path

from inspectah.architect.analyzer import FleetInput

logger = logging.getLogger(__name__)


def load_refined_fleets(input_dir: Path) -> list[FleetInput]:
    """Load refined fleet tarballs from a directory.

    Each tarball should contain an inspection-snapshot.json with fleet metadata.
    Returns a list of FleetInput objects ready for the analyzer.
    """
    fleets: list[FleetInput] = []

    if not input_dir.exists():
        return fleets

    for path in sorted(input_dir.iterdir()):
        if not (path.suffix == ".gz" and path.name.endswith(".tar.gz")):
            continue

        try:
            snapshot = _extract_snapshot(path)
        except Exception as e:
            logger.warning("Skipping %s: %s", path.name, e)
            continue

        if snapshot is None:
            logger.warning("No inspection-snapshot.json found in %s", path.name)
            continue

        fleet_input = _snapshot_to_fleet_input(snapshot)
        fleets.append(fleet_input)

    return fleets


def _extract_snapshot(tarball_path: Path) -> dict | None:
    """Extract inspection-snapshot.json from a tarball."""
    with tarfile.open(tarball_path, "r:gz") as tar:
        for member in tar.getmembers():
            if member.name.endswith("inspection-snapshot.json"):
                f = tar.extractfile(member)
                if f is None:
                    continue
                return json.loads(f.read())
    return None


def _snapshot_to_fleet_input(snapshot: dict) -> FleetInput:
    """Convert an inspection snapshot dict to a FleetInput."""
    meta = snapshot.get("meta", {})
    hostname = meta.get("hostname", "unknown")
    fleet_meta = meta.get("fleet", {})
    host_count = fleet_meta.get("total_hosts", 1)

    # Extract package NVRAs
    rpm = snapshot.get("rpm", {})
    packages = [
        pkg.get("nvra", pkg.get("name", ""))
        for pkg in rpm.get("packages_added", [])
        if pkg.get("nvra") or pkg.get("name")
    ]

    # Extract config file paths
    config = snapshot.get("config", {})
    configs = [f.get("path", "") for f in config.get("files", []) if f.get("path")]

    return FleetInput(
        name=hostname,
        packages=packages,
        configs=configs,
        host_count=host_count,
    )
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && python -m pytest tests/test_architect_loader.py -v`
Expected: All PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add src/inspectah/architect/loader.py tests/test_architect_loader.py
git commit -m "feat(architect): Add refined fleet tarball loader

Discovers .tar.gz files in input directory, extracts inspection-snapshot.json,
converts to FleetInput with package NVRAs, config paths, and host count from
fleet metadata.

Assisted-by: Claude Code (opus)"
```

---

### Task 4: Architect — Export

**Context:** Generates a tarball containing Containerfiles + tree/ directories per layer, plus a build.sh with ordered build commands.

**Files:**
- Create: `src/inspectah/architect/export.py`
- Create: `tests/test_architect_export.py`

- [ ] **Step 1: Write failing tests**

Create `tests/test_architect_export.py`:

```python
"""Tests for architect Containerfile tree export."""

import io
import tarfile
import pytest

from inspectah.architect.analyzer import FleetInput, Layer, LayerTopology, analyze_fleets
from inspectah.architect.export import export_topology


def _make_topology() -> LayerTopology:
    fleets = [
        FleetInput(name="web", packages=["httpd", "openssl", "bash"], configs=["/etc/httpd/httpd.conf"]),
        FleetInput(name="db", packages=["postgresql", "openssl", "bash"], configs=["/etc/pg/pg.conf"]),
    ]
    return analyze_fleets(fleets)


class TestExportTopology:
    def test_returns_bytes(self):
        topo = _make_topology()
        data = export_topology(topo, base_image="registry.redhat.io/rhel9/rhel-bootc:9.4")
        assert isinstance(data, bytes)
        assert len(data) > 0

    def test_is_valid_tarball(self):
        topo = _make_topology()
        data = export_topology(topo, base_image="registry.redhat.io/rhel9/rhel-bootc:9.4")
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            names = tar.getnames()
            assert len(names) > 0

    def test_contains_base_containerfile(self):
        topo = _make_topology()
        data = export_topology(topo, base_image="registry.redhat.io/rhel9/rhel-bootc:9.4")
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            assert "base/Containerfile" in tar.getnames()

    def test_contains_derived_containerfiles(self):
        topo = _make_topology()
        data = export_topology(topo, base_image="registry.redhat.io/rhel9/rhel-bootc:9.4")
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            names = tar.getnames()
            assert "web/Containerfile" in names
            assert "db/Containerfile" in names

    def test_base_containerfile_has_from_upstream(self):
        topo = _make_topology()
        data = export_topology(topo, base_image="registry.redhat.io/rhel9/rhel-bootc:9.4")
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            content = tar.extractfile("base/Containerfile").read().decode()
            assert "FROM registry.redhat.io/rhel9/rhel-bootc:9.4" in content

    def test_derived_containerfile_has_from_base(self):
        topo = _make_topology()
        data = export_topology(topo, base_image="registry.redhat.io/rhel9/rhel-bootc:9.4")
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            content = tar.extractfile("web/Containerfile").read().decode()
            assert "FROM localhost/base:latest" in content

    def test_base_containerfile_has_dnf_install(self):
        topo = _make_topology()
        data = export_topology(topo, base_image="registry.redhat.io/rhel9/rhel-bootc:9.4")
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            content = tar.extractfile("base/Containerfile").read().decode()
            assert "dnf install -y" in content
            assert "openssl" in content
            assert "bash" in content

    def test_contains_build_sh(self):
        topo = _make_topology()
        data = export_topology(topo, base_image="registry.redhat.io/rhel9/rhel-bootc:9.4")
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            assert "build.sh" in tar.getnames()
            content = tar.extractfile("build.sh").read().decode()
            assert "localhost/base:latest" in content
            assert "localhost/web:latest" in content

    def test_build_sh_builds_base_first(self):
        topo = _make_topology()
        data = export_topology(topo, base_image="registry.redhat.io/rhel9/rhel-bootc:9.4")
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            content = tar.extractfile("build.sh").read().decode()
            base_pos = content.index("localhost/base:latest")
            web_pos = content.index("localhost/web:latest")
            assert base_pos < web_pos

    def test_empty_layer_no_dnf_line(self):
        fleets = [
            FleetInput(name="web", packages=["openssl"], configs=[]),
            FleetInput(name="db", packages=["openssl"], configs=[]),
        ]
        topo = analyze_fleets(fleets)
        # Both packages go to base, derived layers are empty
        data = export_topology(topo, base_image="registry.redhat.io/rhel9/rhel-bootc:9.4")
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            content = tar.extractfile("web/Containerfile").read().decode()
            assert "dnf install" not in content
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && python -m pytest tests/test_architect_export.py -v`
Expected: FAIL — `ImportError`

- [ ] **Step 3: Implement export**

Create `src/inspectah/architect/export.py`:

```python
"""Export layer topology as a Containerfile tree tarball."""

from __future__ import annotations

import io
import tarfile
from inspectah.architect.analyzer import LayerTopology


def export_topology(topo: LayerTopology, base_image: str) -> bytes:
    """Generate a .tar.gz containing Containerfile + tree/ per layer, plus build.sh."""
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz") as tar:
        for layer in topo.layers:
            containerfile = _render_containerfile(layer.name, layer.parent, layer.packages, base_image)
            _add_string_to_tar(tar, f"{layer.name}/Containerfile", containerfile)

        build_sh = _render_build_sh(topo, base_image)
        _add_string_to_tar(tar, "build.sh", build_sh)

    return buf.getvalue()


def _render_containerfile(
    layer_name: str,
    parent: str | None,
    packages: list[str],
    base_image: str,
) -> str:
    """Render a Containerfile for a single layer."""
    lines = []

    if parent is None:
        lines.append(f"FROM {base_image}")
    else:
        lines.append(f"FROM localhost/{parent}:latest")

    lines.append("")

    if packages:
        pkg_list = " \\\n    ".join(sorted(packages))
        lines.append(f"RUN dnf install -y \\\n    {pkg_list} \\\n    && dnf clean all")
        lines.append("")

    return "\n".join(lines)


def _render_build_sh(topo: LayerTopology, base_image: str) -> str:
    """Render build.sh with ordered build commands."""
    lines = [
        "#!/bin/bash",
        "# Build base first, then derived images",
        "set -euo pipefail",
        "",
    ]

    # Base first
    base = topo.get_layer("base")
    if base is not None:
        lines.append(f"podman build -t localhost/base:latest base/")

    # Then derived in order
    for layer in topo.layers:
        if layer.parent is not None:
            lines.append(f"podman build -t localhost/{layer.name}:latest {layer.name}/")

    lines.append("")
    return "\n".join(lines)


def _add_string_to_tar(tar: tarfile.TarFile, name: str, content: str) -> None:
    """Add a string as a file to a tarball."""
    data = content.encode("utf-8")
    info = tarfile.TarInfo(name=name)
    info.size = len(data)
    tar.addfile(info, io.BytesIO(data))
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && python -m pytest tests/test_architect_export.py -v`
Expected: All PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add src/inspectah/architect/export.py tests/test_architect_export.py
git commit -m "feat(architect): Add Containerfile tree export as tarball

Generates architect-export.tar.gz with Containerfile per layer (base FROM
upstream, derived FROM localhost/base:latest) plus build.sh with ordered
podman build commands. Empty layers get no dnf install line.

Assisted-by: Claude Code (opus)"
```

---

### Task 5: Architect — Server + CLI Registration

**Context:** HTTP server serving the API routes and the template-rendered UI. CLI subcommand registration. This wires everything together.

**Files:**
- Create: `src/inspectah/architect/server.py`
- Create: `src/inspectah/architect/cli.py`
- Modify: `src/inspectah/cli.py`
- Create: `tests/test_architect_server.py`

- [ ] **Step 1: Write failing tests for server API**

Create `tests/test_architect_server.py`:

```python
"""Tests for architect HTTP server API."""

import json
import threading
import time
import urllib.request
import urllib.error
import pytest
from pathlib import Path

from inspectah.architect.analyzer import FleetInput, analyze_fleets
from inspectah.architect.server import create_handler, start_server


def _make_topology():
    fleets = [
        FleetInput(name="web", packages=["httpd", "openssl", "bash"], configs=["/etc/httpd/httpd.conf"]),
        FleetInput(name="db", packages=["postgresql", "openssl", "bash"], configs=["/etc/pg/pg.conf"]),
    ]
    return analyze_fleets(fleets)


@pytest.fixture()
def server_url(tmp_path):
    """Start architect server on a free port, yield URL, stop after test."""
    topo = _make_topology()
    port, httpd = start_server(
        topo,
        base_image="registry.redhat.io/rhel9/rhel-bootc:9.4",
        template_dir=Path(__file__).resolve().parent.parent / "src" / "inspectah" / "templates",
        patternfly_css="/* test */",
        bind="127.0.0.1",
        port=0,  # let OS pick a free port
        open_browser=False,
    )
    thread = threading.Thread(target=httpd.serve_forever, daemon=True)
    thread.start()
    yield f"http://127.0.0.1:{port}"
    httpd.shutdown()


class TestHealthEndpoint:
    def test_health_returns_ok(self, server_url):
        resp = urllib.request.urlopen(f"{server_url}/api/health")
        assert resp.status == 200
        data = json.loads(resp.read())
        assert data["status"] == "ok"


class TestTopologyEndpoint:
    def test_returns_topology(self, server_url):
        resp = urllib.request.urlopen(f"{server_url}/api/topology")
        assert resp.status == 200
        data = json.loads(resp.read())
        assert "layers" in data
        assert "fleets" in data
        assert len(data["layers"]) == 3  # base + web + db

    def test_layers_have_expected_fields(self, server_url):
        resp = urllib.request.urlopen(f"{server_url}/api/topology")
        data = json.loads(resp.read())
        layer = data["layers"][0]
        assert "name" in layer
        assert "packages" in layer
        assert "fan_out" in layer
        assert "turbulence" in layer


class TestMoveEndpoint:
    def test_move_package_between_layers(self, server_url):
        body = json.dumps({"package": "httpd", "from": "web", "to": "db"}).encode()
        req = urllib.request.Request(
            f"{server_url}/api/move",
            data=body,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        resp = urllib.request.urlopen(req)
        assert resp.status == 200
        data = json.loads(resp.read())
        # Response should include updated topology
        assert "layers" in data
        db_layer = next(l for l in data["layers"] if l["name"] == "db")
        assert "httpd" in db_layer["packages"]

    def test_move_nonexistent_returns_400(self, server_url):
        body = json.dumps({"package": "fake", "from": "web", "to": "db"}).encode()
        req = urllib.request.Request(
            f"{server_url}/api/move",
            data=body,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        with pytest.raises(urllib.error.HTTPError) as exc_info:
            urllib.request.urlopen(req)
        assert exc_info.value.code == 400


class TestExportEndpoint:
    def test_export_returns_tarball(self, server_url):
        resp = urllib.request.urlopen(f"{server_url}/api/export")
        assert resp.status == 200
        assert resp.headers["Content-Type"] == "application/gzip"
        data = resp.read()
        assert len(data) > 0


class TestIndexEndpoint:
    def test_index_returns_html(self, server_url):
        resp = urllib.request.urlopen(f"{server_url}/")
        assert resp.status == 200
        content = resp.read().decode()
        assert "inspectah Architect" in content
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && python -m pytest tests/test_architect_server.py -v`
Expected: FAIL — `ImportError`

- [ ] **Step 3: Implement the server**

Create `src/inspectah/architect/server.py`:

```python
"""HTTP server for inspectah architect interactive UI."""

from __future__ import annotations

import json
import logging
import socket
import webbrowser
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from typing import TYPE_CHECKING

import jinja2

from inspectah.architect.analyzer import LayerTopology
from inspectah.architect.export import export_topology

if TYPE_CHECKING:
    pass

logger = logging.getLogger(__name__)

_DEFAULT_PORT = 8643


def _find_free_port(start: int = _DEFAULT_PORT, max_attempts: int = 20) -> int:
    """Return first free TCP port at or above *start*."""
    for port in range(start, start + max_attempts):
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            try:
                s.bind(("127.0.0.1", port))
                return port
            except OSError:
                continue
    raise RuntimeError(f"No free port found in range {start}-{start + max_attempts}")


def create_handler(
    topology: LayerTopology,
    base_image: str,
    rendered_html: str,
) -> type[BaseHTTPRequestHandler]:
    """Create a request handler class with the topology bound."""

    class _Handler(BaseHTTPRequestHandler):
        _topology = topology
        _base_image = base_image
        _html = rendered_html

        def log_message(self, format, *args):
            logger.debug(format, *args)

        def do_GET(self) -> None:
            path = self.path.split("?")[0]
            if path in ("/", "/index.html"):
                self._send(200, self._html.encode(), "text/html; charset=utf-8")
            elif path == "/api/health":
                self._send_json(200, {"status": "ok"})
            elif path == "/api/topology":
                self._send_json(200, self._topology.to_dict())
            elif path == "/api/export":
                data = export_topology(self._topology, self._base_image)
                self._send(200, data, "application/gzip", {
                    "Content-Disposition": 'attachment; filename="architect-export.tar.gz"',
                })
            else:
                self._send(404, b"Not found", "text/plain")

        def do_POST(self) -> None:
            path = self.path.split("?")[0]
            if path == "/api/move":
                length = int(self.headers.get("Content-Length", 0))
                body = json.loads(self.rfile.read(length))
                try:
                    self._topology.move_package(
                        body["package"], body["from"], body["to"],
                    )
                    self._send_json(200, self._topology.to_dict())
                except (ValueError, KeyError) as e:
                    self._send_json(400, {"error": str(e)})
            else:
                self._send(404, b"Not found", "text/plain")

        def _send(self, code: int, body: bytes, content_type: str, extra_headers: dict | None = None) -> None:
            self.send_response(code)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(body)))
            for k, v in (extra_headers or {}).items():
                self.send_header(k, v)
            self.end_headers()
            self.wfile.write(body)

        def _send_json(self, code: int, data: dict) -> None:
            body = json.dumps(data).encode()
            self._send(code, body, "application/json")

    return _Handler


def start_server(
    topology: LayerTopology,
    base_image: str,
    template_dir: Path,
    patternfly_css: str,
    bind: str = "127.0.0.1",
    port: int = 0,
    open_browser: bool = True,
) -> tuple[int, HTTPServer]:
    """Create and return (port, server) without starting serve_forever."""
    # Render template
    env = jinja2.Environment(
        loader=jinja2.FileSystemLoader(str(template_dir)),
        autoescape=False,
    )
    template = env.get_template("architect/architect.html.j2")
    rendered_html = template.render(
        topology_json=json.dumps(topology.to_dict()),
        patternfly_css=patternfly_css,
    )

    handler_class = create_handler(topology, base_image, rendered_html)

    if port == 0:
        # Let OS pick
        httpd = HTTPServer((bind, 0), handler_class)
        actual_port = httpd.server_address[1]
    else:
        actual_port = _find_free_port(port)
        httpd = HTTPServer((bind, actual_port), handler_class)

    url = f"http://{bind}:{actual_port}"
    logger.info("Serving architect UI at %s", url)

    if open_browser:
        try:
            webbrowser.open(url)
        except Exception:
            pass  # best-effort

    return actual_port, httpd
```

- [ ] **Step 4: Create minimal template for tests**

Create `src/inspectah/templates/architect/architect.html.j2`:

```jinja2
<!DOCTYPE html>
<html lang="en" class="pf-v6-theme-dark">
<head>
  <meta charset="UTF-8"/>
  <title>inspectah Architect</title>
  <style>{{ patternfly_css }}</style>
  {% include "architect/_css.html.j2" %}
</head>
<body>
  <div class="pf-v6-c-page">
    <header class="pf-v6-c-masthead">
      <div class="pf-v6-c-masthead__main">
        <span class="pf-v6-c-masthead__brand">inspectah Architect</span>
      </div>
      <div class="pf-v6-c-masthead__content">
        <button id="theme-toggle" class="pf-v6-c-button pf-m-plain">&#9728;</button>
      </div>
    </header>
    <div class="pf-v6-c-page__body" id="app">
      <!-- JS will render the interactive UI -->
      <noscript>JavaScript is required for the architect UI.</noscript>
    </div>
  </div>
  <script>
    window.__TOPOLOGY__ = {{ topology_json }};
  </script>
  {% include "architect/_js.html.j2" %}
</body>
</html>
```

Create `src/inspectah/templates/architect/_css.html.j2`:

```jinja2
<style>
/* Architect custom styles — will be expanded in Task 6 */
</style>
```

Create `src/inspectah/templates/architect/_js.html.j2`:

```jinja2
<script>
(function(){
  // Theme toggle
  var t = localStorage.getItem('inspectah-architect-theme');
  if (t === 'light') document.documentElement.classList.remove('pf-v6-theme-dark');
  var btn = document.getElementById('theme-toggle');
  if (btn) btn.addEventListener('click', function() {
    var html = document.documentElement;
    html.classList.toggle('pf-v6-theme-dark');
    localStorage.setItem('inspectah-architect-theme',
      html.classList.contains('pf-v6-theme-dark') ? 'dark' : 'light');
  });
})();

// Full interactive UI will be added in Task 6
</script>
```

- [ ] **Step 5: Implement CLI registration**

Create `src/inspectah/architect/cli.py`:

```python
"""CLI registration for inspectah architect subcommand."""

from __future__ import annotations

import argparse
import logging
import sys
from pathlib import Path

logger = logging.getLogger(__name__)


def add_architect_args(parser: argparse.ArgumentParser) -> None:
    """Register architect-specific CLI arguments."""
    parser.add_argument(
        "input_dir",
        type=Path,
        metavar="INPUT_DIR",
        help="Directory containing refined fleet tarballs (.tar.gz)",
    )
    parser.add_argument(
        "--port",
        type=int,
        default=8643,
        help="Port for the architect web UI (default: 8643)",
    )
    parser.add_argument(
        "--no-browser",
        action="store_true",
        help="Don't open browser automatically",
    )
    parser.add_argument(
        "--bind",
        default="127.0.0.1",
        help="Address to bind (default: 127.0.0.1)",
    )


def run_architect(args: argparse.Namespace) -> int:
    """Entry point for the architect subcommand."""
    from inspectah.architect.loader import load_refined_fleets
    from inspectah.architect.analyzer import analyze_fleets
    from inspectah.architect.server import start_server

    input_dir = args.input_dir
    if not input_dir.exists():
        print(f"Error: directory {input_dir} does not exist", file=sys.stderr)
        return 1

    fleets = load_refined_fleets(input_dir)
    if not fleets:
        print(f"Error: no refined fleet tarballs found in {input_dir}", file=sys.stderr)
        return 1

    if len(fleets) < 2:
        print(
            f"Error: architect requires at least 2 fleets, found {len(fleets)}. "
            "Load multiple refined fleet tarballs to decompose into layers.",
            file=sys.stderr,
        )
        return 1

    print(f"Loaded {len(fleets)} fleets: {', '.join(f.name for f in fleets)}")

    topology = analyze_fleets(fleets)
    base = topology.get_layer("base")
    print(f"Proposed topology: {len(base.packages)} base packages, "
          f"{len(topology.layers) - 1} derived layers")

    # Load PatternFly CSS
    template_dir = Path(__file__).resolve().parent.parent / "templates"
    pf_path = template_dir / "patternfly.css"
    patternfly_css = pf_path.read_text() if pf_path.exists() else ""

    # Determine base image from first fleet's snapshot (if available)
    base_image = "registry.redhat.io/rhel9/rhel-bootc:9.4"

    port, httpd = start_server(
        topology,
        base_image=base_image,
        template_dir=template_dir,
        patternfly_css=patternfly_css,
        bind=args.bind,
        port=args.port,
        open_browser=not args.no_browser,
    )

    print(f"Serving architect UI at http://{args.bind}:{port}")
    print("Press Ctrl+C to stop")

    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        print("\nStopping architect server")
        httpd.shutdown()

    return 0
```

- [ ] **Step 6: Register architect in main CLI**

Read `src/inspectah/cli.py` and find the `SUBCOMMANDS` tuple and the subparser setup section. Add `"architect"` to `SUBCOMMANDS` and register the subparser. The exact edit depends on the current code structure — find where `fleet` is registered and add `architect` following the same pattern:

```python
# In cli.py, add to SUBCOMMANDS:
SUBCOMMANDS = ("inspect", "fleet", "refine", "architect")

# In the subparser setup section, add:
from inspectah.architect.cli import add_architect_args, run_architect

architect_parser = subparsers.add_parser("architect", help="Plan layer decomposition from refined fleets")
add_architect_args(architect_parser)
architect_parser.set_defaults(func=run_architect)
```

- [ ] **Step 7: Run server tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && python -m pytest tests/test_architect_server.py -v`
Expected: All PASS

- [ ] **Step 8: Run full test suite for regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && python -m pytest -v`
Expected: All tests pass (existing + new)

- [ ] **Step 9: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add src/inspectah/architect/cli.py src/inspectah/architect/server.py \
        src/inspectah/templates/architect/ src/inspectah/cli.py \
        tests/test_architect_server.py
git commit -m "feat(architect): Add HTTP server, CLI registration, and minimal templates

Server exposes /api/topology, /api/move, /api/export, /api/health on port
8643. CLI registers 'architect' subcommand with input_dir, --port,
--no-browser, --bind flags. Templates are minimal scaffolds — full UI
in next task.

Assisted-by: Claude Code (opus)"
```

---

### Task 6: Architect — Interactive Frontend

**Context:** The PatternFly 6 web UI with sidebar (fleets), center (layer tree), drawer (packages), click-to-move interaction, and export button. This is the biggest single task.

**Files:**
- Modify: `src/inspectah/templates/architect/architect.html.j2`
- Modify: `src/inspectah/templates/architect/_css.html.j2`
- Modify: `src/inspectah/templates/architect/_js.html.j2`

- [ ] **Step 1: Implement the full HTML template**

Replace `src/inspectah/templates/architect/architect.html.j2` with the complete PatternFly 6 page layout. This is a large template — follow the exact structure from the refine `report.html.j2` for PatternFly class patterns, theme toggle, and masthead. The key sections:

- Masthead with "inspectah Architect" branding and theme toggle
- PF6 page body with sidebar, main content, and drawer
- Sidebar: fleet list rendered from `window.__TOPOLOGY__.fleets`
- Center: layer tree rendered from `window.__TOPOLOGY__.layers`
- Drawer: package list for selected layer with "Move to →" dropdown buttons
- Bottom toolbar: summary counts + Export button

The template receives `{{ topology_json }}` and `{{ patternfly_css }}` from the server.

**Implementation note:** Since this is a large HTML file, write it in full. Follow refine's exact PF6 class usage (`pf-v6-c-page`, `pf-v6-c-page__sidebar`, `pf-v6-c-page__main`, `pf-v6-c-drawer`, `pf-v6-c-masthead`, etc.). Use the approved layout mockup from the brainstorm as the reference:

- Sidebar: fleet cards with colored left borders, host count, package count
- Center: base layer card (red left border) with fan-out and turbulence badges, derived layers indented below
- Drawer: selected layer's packages as a list, each with "Move to →" dropdown
- Bottom: sticky toolbar with counts and red Export button

- [ ] **Step 2: Implement the CSS**

Replace `src/inspectah/templates/architect/_css.html.j2` with custom styles. Reference refine's `_css.html.j2` for patterns. Key styles needed:

- Fleet cards in sidebar with colored left borders
- Layer tree cards with left border accents (red for base, fleet colors for derived)
- Metric badges (fan-out amber, turbulence blue)
- Package list styling with hover state
- "Move to →" dropdown button styling
- Sticky bottom toolbar
- Responsive layout adjustments

- [ ] **Step 3: Implement the JavaScript**

Replace `src/inspectah/templates/architect/_js.html.j2` with the interactive JS. Key functions:

```javascript
// State
let topology = window.__TOPOLOGY__;
let selectedLayer = 'base';

// Rendering
function renderSidebar() { /* render fleet list from topology.fleets */ }
function renderTree() { /* render layer tree from topology.layers */ }
function renderDrawer() { /* render packages for selectedLayer */ }
function renderToolbar() { /* render summary counts */ }
function render() { renderSidebar(); renderTree(); renderDrawer(); renderToolbar(); }

// Layer selection
function selectLayer(name) { selectedLayer = name; render(); }

// Move package
async function movePackage(pkg, from, to) {
    const resp = await fetch('/api/move', {
        method: 'POST',
        headers: {'Content-Type': 'application/json'},
        body: JSON.stringify({package: pkg, from: from, to: to}),
    });
    topology = await resp.json();
    render();
}

// Impact tooltip (deterministic)
function calcImpact(pkg, targetLayer) {
    const target = topology.layers.find(l => l.name === targetLayer);
    const newPkgCount = target.packages.length + 1;
    const newTurbulence = Math.max(1.0, target.fan_out * (newPkgCount / 50.0));
    return `Moving ${pkg} to ${targetLayer} affects ${target.fan_out} downstream images. Turbulence: ${target.turbulence.toFixed(1)} → ${newTurbulence.toFixed(1)}`;
}

// Export
async function exportTopology() {
    const resp = await fetch('/api/export');
    const blob = await resp.blob();
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = 'architect-export.tar.gz';
    a.click();
    URL.revokeObjectURL(url);
}

// Theme toggle (same pattern as refine)
// Init
render();
```

- [ ] **Step 4: Verify the server serves the updated templates**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && python -m pytest tests/test_architect_server.py::TestIndexEndpoint -v`
Expected: PASS (template renders without Jinja2 errors)

- [ ] **Step 5: Manual verification**

Prepare test data and launch the server manually to verify the UI. This creates mock refined fleet tarballs directly (bypassing the full driftify → inspect → fleet → refine pipeline):

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah

# Generate mock refined fleet tarballs
python -c "
import json, tarfile, io
from pathlib import Path

out = Path('/tmp/architect-test')
out.mkdir(exist_ok=True)

shared = [f'shared-pkg-{i}-1.0-1.el9.x86_64' for i in range(25)]
fleets = {
    'web-servers': shared + ['httpd-2.4.57-5.el9.x86_64', 'mod_ssl-2.4.57-5.el9.x86_64', 'php-8.0.30-1.el9.x86_64'],
    'db-servers': shared + ['postgresql-server-15.4-1.el9.x86_64', 'pgaudit-1.7.0-1.el9.x86_64'],
    'app-servers': shared + ['python3.11-3.11.7-1.el9.x86_64', 'gunicorn-21.2.0-1.el9.noarch', 'redis-7.0.12-1.el9.x86_64'],
}
for name, pkgs in fleets.items():
    snap = {
        'schema_version': 6,
        'meta': {'hostname': name, 'fleet': {'source_hosts': [f'{name}-01', f'{name}-02', f'{name}-03'], 'total_hosts': 3}},
        'os_release': {'name': 'RHEL', 'version_id': '9.4', 'id': 'rhel'},
        'rpm': {'base_image': 'registry.redhat.io/rhel9/rhel-bootc:9.4', 'packages_added': [{'name': p.rsplit('-', 2)[0], 'nvra': p, 'source': 'dnf'} for p in pkgs]},
        'config': {'files': []},
    }
    data = json.dumps(snap).encode()
    with tarfile.open(out / f'{name}.tar.gz', 'w:gz') as tar:
        info = tarfile.TarInfo(name='inspection-snapshot.json')
        info.size = len(data)
        tar.addfile(info, io.BytesIO(data))
print(f'Created {len(fleets)} mock fleet tarballs in {out}')
"

# Launch architect
python -m inspectah architect /tmp/architect-test --no-browser
# Open http://localhost:8643 manually and verify:
# - Three fleets show in sidebar
# - Base layer has ~25 shared packages
# - Derived layers have 2-3 exclusive packages each
# - Click-to-move works
# - Export downloads a tarball
```

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add src/inspectah/templates/architect/
git commit -m "feat(architect): Add interactive PatternFly 6 web UI

Three-column layout: fleet sidebar, layer topology tree, package drawer.
Click-to-select layers, click-to-move packages with 'Move to' dropdown.
Deterministic impact tooltips. Dark/light theme toggle. Export button
downloads Containerfile tarball.

Assisted-by: Claude Code (opus)"
```

---

### Task 7: Integration — End-to-End Verification

**Context:** Integration test covering the architect-internal pipeline: load refined tarballs → analyze → move → export. Uses mock refined tarballs (not the full driftify → inspect → fleet → refine upstream pipeline — that's a separate manual test).

**Files:**
- Create: `tests/test_architect_integration.py`

- [ ] **Step 1: Write integration test**

Create `tests/test_architect_integration.py`:

```python
"""Integration test for architect: fixture → load → analyze → export."""

import io
import json
import tarfile
import pytest
from pathlib import Path

from inspectah.architect.loader import load_refined_fleets
from inspectah.architect.analyzer import analyze_fleets
from inspectah.architect.export import export_topology


@pytest.fixture()
def three_fleet_dir(tmp_path):
    """Create mock refined fleet tarballs for three fleets."""
    shared = [f"shared-pkg-{i}-1.0-1.el9.x86_64" for i in range(20)]

    fleet_data = {
        "web-servers": shared + ["httpd-2.4-1.el9.x86_64", "mod_ssl-2.4-1.el9.x86_64"],
        "db-servers": shared + ["postgresql-15-1.el9.x86_64", "pgaudit-1.7-1.el9.x86_64"],
        "app-servers": shared + ["python3-3.11-1.el9.x86_64", "gunicorn-21-1.el9.x86_64"],
    }

    for fleet_name, packages in fleet_data.items():
        snapshot = {
            "schema_version": 6,
            "meta": {
                "hostname": fleet_name,
                "fleet": {"source_hosts": [f"{fleet_name}-01"], "total_hosts": 3},
            },
            "os_release": {"name": "RHEL", "version_id": "9.4", "id": "rhel"},
            "rpm": {
                "base_image": "registry.redhat.io/rhel9/rhel-bootc:9.4",
                "packages_added": [
                    {"name": p.split("-")[0], "nvra": p, "source": "dnf"}
                    for p in packages
                ],
            },
            "config": {"files": []},
        }
        snap_json = json.dumps(snapshot).encode()
        tarball_path = tmp_path / f"{fleet_name}.tar.gz"
        with tarfile.open(tarball_path, "w:gz") as tar:
            info = tarfile.TarInfo(name="inspection-snapshot.json")
            info.size = len(snap_json)
            tar.addfile(info, io.BytesIO(snap_json))

    return tmp_path


class TestEndToEnd:
    def test_load_analyze_export(self, three_fleet_dir):
        # Load
        fleets = load_refined_fleets(three_fleet_dir)
        assert len(fleets) == 3

        # Analyze
        topo = analyze_fleets(fleets)
        base = topo.get_layer("base")
        assert base is not None
        assert len(base.packages) == 20  # shared packages
        assert base.fan_out == 3

        # Each derived layer has 2 exclusive packages
        for name in ("web-servers", "db-servers", "app-servers"):
            layer = topo.get_layer(name)
            assert layer is not None
            assert len(layer.packages) == 2
            assert layer.parent == "base"

        # Move a package from base to web
        topo.move_package(base.packages[0], "base", "web-servers")
        assert len(base.packages) == 19
        # Broadcast: all derived layers get the package
        for name in ("web-servers", "db-servers", "app-servers"):
            layer = topo.get_layer(name)
            assert len(layer.packages) == 3  # 2 original + 1 moved

        # Export
        data = export_topology(topo, base_image="registry.redhat.io/rhel9/rhel-bootc:9.4")
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            names = tar.getnames()
            assert "base/Containerfile" in names
            assert "web-servers/Containerfile" in names
            assert "db-servers/Containerfile" in names
            assert "app-servers/Containerfile" in names
            assert "build.sh" in names

            # Verify base Containerfile
            base_cf = tar.extractfile("base/Containerfile").read().decode()
            assert "FROM registry.redhat.io/rhel9/rhel-bootc:9.4" in base_cf
            assert "dnf install" in base_cf

            # Verify derived references base
            web_cf = tar.extractfile("web-servers/Containerfile").read().decode()
            assert "FROM localhost/base:latest" in web_cf

            # Verify build.sh order
            build = tar.extractfile("build.sh").read().decode()
            assert build.index("localhost/base:latest") < build.index("localhost/web-servers:latest")
```

- [ ] **Step 2: Run integration test**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && python -m pytest tests/test_architect_integration.py -v`
Expected: All PASS

- [ ] **Step 3: Run full test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && python -m pytest -v`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add tests/test_architect_integration.py
git commit -m "test(architect): Add end-to-end integration test

Verifies load → analyze → move → export pipeline with 3-fleet fixture.
Checks base extraction, derived layer contents, broadcast on move from
base, and exported Containerfile structure.

Assisted-by: Claude Code (opus)"
```

---

## Summary

7 tasks across 2 repos:

| # | Task | Repo | Days | Risk |
|---|------|------|------|------|
| 1 | Driftify multi-fleet fixtures | driftify | 0.5 | Low |
| 2 | Analyzer + data model | inspectah | 1 | Low |
| 3 | Loader | inspectah | 0.5 | Low |
| 4 | Export | inspectah | 0.5 | Low |
| 5 | Server + CLI | inspectah | 1.5 | Medium |
| 6 | Interactive frontend | inspectah | 3-4 | **High** |
| 7 | Integration test | inspectah | 0.5 | Low |

**Total: ~8 days.** Task 6 (frontend) is the riskiest and largest — if time gets tight, the click-to-move interaction can be simplified to a basic dropdown without tooltips.

Tasks 1-5 can be done sequentially in ~4 days. Task 6 is the frontend sprint. Task 7 is fast verification.
