# inspectah architect: Layer Topology Planner — Design Spec

**Date:** 2026-03-29
**Status:** Approved
**Deadline:** April 8, 2026 (demo at internal Red Hat meeting)
**Reviewed by:** Mark Russell

## Overview

A new `inspectah architect` subcommand that takes multiple refined fleet outputs and helps enterprise architects decompose them into a layered bootc image hierarchy: a base image plus derived role/hardware-specific images. Ships with an interactive web UI for exploring and adjusting the proposed topology.

## Pipeline Position

```
inspect → fleet → refine → architect (NEW)
```

Architect consumes refined fleet outputs. It does not replace or subsume refine.

| Tool      | Direction  | Question answered              |
|-----------|------------|--------------------------------|
| inspect   | Collect    | What's on this host?           |
| fleet     | Aggregate  | What do these hosts share?     |
| refine    | Curate     | Which variants win?            |
| architect | Decompose  | How should the layers split?   |

## Decisions Log

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Architecture | New package in inspectah, not standalone app | Follows existing patterns, fastest path, extract later |
| Server | Raw BaseHTTPRequestHandler, port 8643 | Matches refine pattern |
| UI framework | PatternFly 6 bundled + Jinja2 templates | Matches refine, Red Hat look |
| Layout | Sidebar + tree + drawer (3-column) | Matches refine pattern, approved via mockup |
| Initial split algorithm | 100% cross-fleet prevalence → base | Simple, explainable, human corrects |
| Interaction model | Click-to-select + "Move to" button | De-risks timeline vs drag-and-drop; drag-and-drop deferred to standalone app |
| Blast radius | Mocked with plausible shifting numbers | Real advisory feed integration deferred |
| Config/directive assignment | Stays with original fleet | Directive-follows-package deferred to v2 |
| Configs in UI | Read-only (visible but not movable) | Simplifies interaction for demo |
| Single-fleet behavior | No base extraction, show message | Edge case — architect needs 2+ fleets to decompose |
| JS approach | Inline via _js.html.j2 partials | Matches refine pattern, no static/ directory needed |
| Export format | Tarball with Containerfile + tree/ per layer | Complete buildable artifacts |
| Fleet crossover (GPU+role) | Deferred | Keep fleets as single roles for demo |
| Packaging | Deferred | Demo runs via containerized wrapper |

## Data Flow

```
inspectah architect ./refined-fleets/
    ↓ loads refined fleet tarballs from directory
    ↓ cross-fleet commonality analysis
    ↓ prevalence-based base/derived split (100% → base)
    ↓ launches web UI on localhost:8643
    ↓ user explores, moves packages between layers (click-to-move)
    ↓ export button → Containerfile tree as tarball
```

### CLI

```
inspectah architect <input_dir>
```

- `input_dir`: directory containing refined fleet tarballs (`.tar.gz`)
- Launches HTTP server on port 8643
- Prints URL to stdout: `Serving architect UI at http://localhost:8643`
- Opens browser via `webbrowser.open()` when running natively (best-effort, no error if it fails)
- When running inside the container, browser opening is skipped — the user opens the URL manually (port 8643 is exposed by the wrapper)
- Ctrl+C to stop

## Backend Architecture

New package: `src/inspectah/architect/`

### `__init__.py`
Package marker.

### `cli.py`
Registers the `architect` subcommand with argparse. Takes `input_dir` argument. Calls `run_architect()`.

Follows existing pattern in `src/inspectah/fleet/cli.py` for subcommand registration.

### `loader.py`
Reads refined fleet tarballs from the input directory. Each tarball contains a refined `inspection-snapshot.json` with fleet metadata and prevalence data.

Reuses patterns from `src/inspectah/fleet/loader.py` (tarball discovery, JSON extraction, schema validation).

Returns: list of fleet objects, each with identity (name, host count) and their refined packages/configs.

### `analyzer.py`
Core decomposition logic.

**Input:** List of loaded fleets from loader.

**Scope constraint:** v1 uses a flat topology — one base layer plus one derived layer per fleet. No intermediate/composite layers for packages shared by some-but-not-all fleets. Subset-shared intermediate layers are deferred to v2.

**Algorithm:**
1. Build cross-fleet package index: for each package, which fleets contain it?
2. Apply 100% prevalence heuristic: packages in ALL fleets → `base` layer
3. Remaining packages → each fleet's derived layer. If a package appears in 2 of 3 fleets, it is duplicated into both derived layers (not factored into a shared intermediate layer)

**Config files are NOT decomposed.** Configs and service directives stay with their original fleet — always in derived layers, never promoted to base. This avoids the directive-ownership problem (v2) and keeps the v1 algorithm packages-only. Configs are displayed read-only in the UI for context but cannot be moved.

**Output:** `LayerTopology` data structure:

```python
@dataclass
class Layer:
    name: str            # "base", "web-servers", "db-servers", etc.
    parent: str | None   # None for base, "base" for derived
    packages: list[str]  # package NVRAs
    configs: list[str]   # config file paths
    fleets: list[str]    # which fleets this layer serves
    # Mocked metrics (plausible numbers, not real)
    fan_out: int         # number of derived layers below this one (real)
    turbulence: float    # semi-made-up: fan_out * (package_count / 50.0), floor 1.0 for non-base

@dataclass
class LayerTopology:
    layers: list[Layer]
    fleets: list[FleetInfo]  # loaded fleet metadata

    def move_package(self, package: str, from_layer: str, to_layer: str) -> None:
        """Move a package between layers. Recalculates metrics.

        Standard move: remove from `from_layer`, add to `to_layer`.

        Special case — moving FROM base: since base packages were
        100% prevalent, every fleet needs them. Remove from base,
        add to `to_layer`. All OTHER derived layers also get the
        package automatically (they still need it). The `to_layer`
        parameter is still meaningful — it identifies which layer
        the user chose, and the broadcast to other derived layers
        is a side effect the UI should communicate.
        """

    def export(self) -> bytes:
        """Generate tarball with Containerfile + tree/ per layer."""
```

**Semi-made-up blast radius:** `fan_out` is real (count of derived layers that inherit from this layer). `turbulence` is `max(1.0, fan_out * (package_count / 50.0))` — shifts naturally when packages move between layers since package_count changes. Floor of 1.0 for non-base layers prevents awkwardly small numbers. Simple formula, looks responsive, no real advisory data needed.

### `server.py`
Local HTTP server on port 8643. Raw `BaseHTTPRequestHandler` matching refine's pattern.

**Routes:**

| Method | Path | Description |
|--------|------|-------------|
| GET | `/` | Serve the HTML UI (rendered from Jinja2 template) |
| GET | `/api/topology` | Current topology state as JSON |
| POST | `/api/move` | Move a package between layers. Body: `{"package": "...", "from": "...", "to": "..."}`. When moving from base, package is broadcast to all derived layers (see move_package docstring). Response includes updated topology so UI can reflect the broadcast. |
| GET | `/api/export` | Generate and return tarball |
| GET | `/api/health` | Health check |

All JS/CSS is inlined via Jinja2 template partials (matching refine's pattern). No separate static file serving needed.

### Templates

`src/inspectah/templates/architect/` — Jinja2 templates following refine's pattern:

- `architect.html.j2` — main page template
- `architect/_css.html.j2` — custom CSS overrides
- `architect/_js.html.j2` — interactive JS (click-to-move, API calls, metric updates)

All JS/CSS inlined via partials — no separate static directory. Matches refine's approach. Confirm Jinja2 loader search path includes the `architect/` subdirectory.

Uses bundled PatternFly 6 CSS from `src/inspectah/templates/patternfly.css` (already in project).

## Frontend Design

### Theme
Dark/light toggle matching refine: `pf-v6-theme-dark` class on `<html>`, persisted to `localStorage` key `inspectah-architect-theme`.

### Layout (PatternFly 6 page layout)

```
┌─────────────────────────────────────────────────────────┐
│  Masthead: inspectah Architect              [☀️ theme]     │
├──────────────┬──────────────────────┬───────────────────┤
│  Sidebar     │  Layer Topology      │  Package Drawer   │
│  (fleets)    │  (tree view)         │  (selected layer) │
│              │                      │                   │
│  web-servers │  ┌─ base ──────────┐ │  base · 152 pkgs  │
│  42 hosts    │  │ 152 pkgs        │ │                   │
│  186 pkgs    │  │ fan-out: 3      │ │  📦 bash-5.1.8    │
│              │  │ turbulence: 4.2 │ │  📦 openssl-3.0.7 │
│  db-servers  │  └─────────────────┘ │  📦 systemd-252   │
│  28 hosts    │    ├─ web-servers     │  📦 kernel-5.14   │
│  201 pkgs    │    ├─ db-servers      │  ...              │
│              │    └─ gpu-nodes       │                   │
│  gpu-nodes   │                      │  [Move to → ▼]    │
│  12 hosts    │                      │                   │
│  224 pkgs    │                      │                   │
├──────────────┴──────────────────────┴───────────────────┤
│  3 fleets · 82 hosts · 4 layers        [Export ▼]       │
└─────────────────────────────────────────────────────────┘
```

### Sidebar (Left)
- Lists loaded fleets with color-coded left border
- Shows host count and package count per fleet
- Click to highlight that fleet's packages in the drawer

### Layer Topology (Center)
- Tree of layers: base at top, derived indented below
- Each layer card shows: name, package count, fleet(s) served
- Mocked metrics: fan-out badge, turbulence score badge
- Click a layer to select it → drawer shows its packages
- Metrics shift when packages are moved (plausible, not real)

### Package Drawer (Right)
- Shows packages for the selected layer
- Config files shown below packages (read-only, not movable — labeled as such)
- Each package row has a "Move to →" button that shows available target layers
- Click to select package, click target layer → `POST /api/move`, UI updates, metrics recalculate
- Impact tooltip on the move button: deterministic formula based on target layer's fan_out and package count (e.g., "Moving {pkg} to {target} affects {target.fan_out} downstream images. Layer turbulence: {target.turbulence:.1f} → {new_turbulence:.1f}"). Uses the same stable turbulence formula — no randomness, same input always shows same tooltip.

### Bottom Toolbar (Sticky)
- Fleet/host/layer summary counts
- **Export Containerfiles** button (red, prominent) → triggers download of tarball

## Export Format

Export button generates a `.tar.gz`:

```
architect-export.tar.gz
├── base/
│   ├── Containerfile
│   └── tree/
│       ├── etc/...
│       └── ...
├── web-servers/
│   ├── Containerfile
│   └── tree/
│       └── ...
├── db-servers/
│   ├── Containerfile
│   └── tree/
│       └── ...
└── gpu-nodes/
    ├── Containerfile
    └── tree/
        └── ...
```

**Containerfile generation:**
- Base: `FROM registry.redhat.io/rhel9/rhel-bootc:9.4` + `RUN dnf install -y <packages>` + `COPY tree/ /`
- Derived: `FROM localhost/base:latest` + `RUN dnf install -y <packages>` + `COPY tree/ /`

Export also includes a `build.sh` at the root with ordered build commands:
```bash
#!/bin/bash
# Build base first, then derived images
podman build -t localhost/base:latest base/
podman build -t localhost/web-servers:latest web-servers/
podman build -t localhost/db-servers:latest db-servers/
podman build -t localhost/gpu-nodes:latest gpu-nodes/
```

Reuses existing Containerfile rendering patterns from `src/inspectah/renderers/`.

**Config/directive assignment (v1):** Config files and service directives stay with their original fleet. No directive-follows-package logic. Known limitation — user hand-edits edge cases. Flagged for v2.

## Driftify Fixtures

### Prerequisite: Multi-Fleet Topology Generation

Driftify needs to generate two fixture scenarios with controlled inter-fleet variance. Current cumulative profile model needs support for exclusive/subtractive packages between fleets.

**Key requirement:** 3-4 hosts per fleet, differing only by hostname. Within each fleet, packages are identical — simulating the "cooking show" scenario where fleet+refine already converged the hosts. The interesting variance is between fleets.

### Fixture 1: "three-role-overlap"

| Fleet | Hosts | Unique packages | Shared with all |
|-------|-------|-----------------|-----------------|
| web-servers | web-01, web-02, web-03 | httpd, mod_ssl, php | ~85% shared base |
| db-servers | db-01, db-02, db-03, db-04 | postgresql-server, pg_stat, pgaudit | ~85% shared base |
| app-servers | app-01, app-02, app-03 | python3, gunicorn, redis | ~85% shared base |

Demonstrates clean base extraction — architect proposes a large base layer and three small derived layers.

### Fixture 2: "hardware-split"

| Fleet | Hosts | Unique packages | Shared with all |
|-------|-------|-----------------|-----------------|
| standard-compute | std-01, std-02, std-03 | (baseline server packages) | ~90% shared base |
| gpu-nodes | gpu-01, gpu-02, gpu-03 | nvidia-driver, cuda-toolkit, kmod-nvidia | ~90% shared base |

Demonstrates hardware tier — architect proposes base + two derived layers where the hardware difference is the split.

## Container Integration

The demo runs via the containerized inspectah wrapper. Port 8643 must be exposed in the container for the architect web UI. No packaging changes needed — deferred.

## Deferred (v2+)

- **Directive-follows-package:** When a package moves between layers, its associated service enables, config files, and Containerfile directives should follow. The inspection snapshot has the data — implementation deferred.
- **Real blast radius scoring:** Advisory feed integration (Red Hat OVAL, CentOS Stream advisories) for actual change frequency data.
- **Cross-cutting axes:** GPU nodes that are also web/db servers — multi-dimensional topology with 3+ levels.
- **Subset-shared intermediate layers:** Packages shared by some-but-not-all fleets could factor into shared intermediate layers instead of being duplicated. Requires multi-level topology support.
- **Config decomposition:** Promote common configs to base, with directive-follows-package logic.
- **Drag-and-drop interaction:** Replace click-to-move with HTML5 drag-and-drop in the standalone web app.
- **Standalone web app extraction:** Full interactive UI with drag-and-drop layer builder as a separate application.
- **Collaboration/sharing:** Multi-user review of topology decisions.
- **Packaging:** Homebrew/COPR distribution of architect subcommand.

## Testing

- **Unit tests for analyzer:** Given known fleet data, verify correct base/derived split. Test edge cases: single fleet (no base extraction), all packages shared (everything in base), no overlap (empty base).
- **Unit tests for loader:** Verify tarball discovery and parsing from directory.
- **Integration test:** Load fixture tarballs → run analysis → verify topology structure → export → verify Containerfile contents.
- **Server tests:** API endpoints return expected shapes, move endpoint updates state correctly.
- **Driftify tests:** Verify new fixtures produce the expected inter-fleet variance.

Follow existing test patterns in `tests/` — pytest, data-driven where appropriate.
