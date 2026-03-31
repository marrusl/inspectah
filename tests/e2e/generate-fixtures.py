#!/usr/bin/env python3
"""Generate E2E test fixture tarballs from Pydantic models.

Builds InspectionSnapshot objects, renders them through the full pipeline
(run_all()), and outputs tarballs with schema-version caching so they only
regenerate when the schema changes.

Usage:
    uv run python tests/e2e/generate-fixtures.py          # skip if up to date
    uv run python tests/e2e/generate-fixtures.py --force   # always regenerate
"""

from __future__ import annotations

import argparse
import shutil
import sys
import tarfile
import tempfile
from pathlib import Path

# ---------------------------------------------------------------------------
# Ensure the yoinkc package is importable when run from the repo root.
# ---------------------------------------------------------------------------
REPO_ROOT = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(REPO_ROOT / "src"))

from yoinkc.renderers import run_all  # noqa: E402
from yoinkc.schema import (  # noqa: E402
    SCHEMA_VERSION,
    ConfigCategory,
    ConfigFileEntry,
    ConfigFileKind,
    ConfigSection,
    FleetMeta,
    FleetPrevalence,
    InspectionSnapshot,
    OsRelease,
    PackageEntry,
    PackageState,
    RepoFile,
    RpmSection,
    ServiceSection,
    ServiceStateChange,
)

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------
FIXTURES_DIR = Path(__file__).resolve().parent / "fixtures"
ARCHITECT_DIR = FIXTURES_DIR / "architect-topology"
SCHEMA_VERSION_FILE = FIXTURES_DIR / ".schema-version"


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _fleet(count: int, total: int, hosts: list[str]) -> FleetPrevalence:
    return FleetPrevalence(count=count, total=total, hosts=hosts)


ALL_HOSTS = ["web-01", "web-02", "web-03"]


def _pkg(
    name: str,
    version: str = "1.0",
    release: str = "1.el9",
    arch: str = "x86_64",
    state: PackageState = PackageState.ADDED,
    include: bool = True,
    source_repo: str = "",
    fleet: FleetPrevalence | None = None,
) -> PackageEntry:
    return PackageEntry(
        name=name,
        version=version,
        release=release,
        arch=arch,
        state=state,
        include=include,
        source_repo=source_repo,
        fleet=fleet,
    )


def _svc(
    unit: str,
    current_state: str = "enabled",
    default_state: str = "disabled",
    action: str = "enable",
    include: bool = True,
    fleet: FleetPrevalence | None = None,
) -> ServiceStateChange:
    return ServiceStateChange(
        unit=unit,
        current_state=current_state,
        default_state=default_state,
        action=action,
        include=include,
        fleet=fleet,
    )


def _cfg(
    path: str,
    kind: ConfigFileKind = ConfigFileKind.UNOWNED,
    category: ConfigCategory = ConfigCategory.OTHER,
    content: str = "",
    include: bool = True,
    fleet: FleetPrevalence | None = None,
) -> ConfigFileEntry:
    return ConfigFileEntry(
        path=path,
        kind=kind,
        category=category,
        content=content,
        include=include,
        fleet=fleet,
    )


def _render_to_tarball(snapshot: InspectionSnapshot, tarball_path: Path) -> None:
    """Render a snapshot through run_all() into a temp dir, then tar it up."""
    with tempfile.TemporaryDirectory() as tmpdir:
        output_dir = Path(tmpdir) / "output"
        run_all(snapshot, output_dir)
        # Write the inspection-snapshot.json (required by refine server)
        snapshot_path = output_dir / "inspection-snapshot.json"
        snapshot_path.write_text(snapshot.model_dump_json(indent=2))
        tarball_path.parent.mkdir(parents=True, exist_ok=True)
        with tarfile.open(tarball_path, "w:gz") as tar:
            for item in sorted(output_dir.iterdir()):
                tar.add(item, arcname=item.name)


# ---------------------------------------------------------------------------
# Fixture: Fleet (3-host)
# ---------------------------------------------------------------------------

def build_fleet_snapshot() -> InspectionSnapshot:
    """Fleet of 3 web servers with variant ties, mixed prevalence, triage mix."""
    os_rel = OsRelease(
        name="Red Hat Enterprise Linux",
        version_id="9.4",
        version="9.4 (Plow)",
        id="rhel",
        id_like="fedora",
        pretty_name="Red Hat Enterprise Linux 9.4 (Plow)",
    )

    # --- RPM packages at varying prevalence ---
    packages_added = [
        # 100% prevalence (all 3 hosts)
        _pkg("httpd", "2.4.57", "5.el9", source_repo="rhel-9-appstream",
             fleet=_fleet(3, 3, ALL_HOSTS)),
        _pkg("mod_ssl", "2.4.57", "5.el9", source_repo="rhel-9-appstream",
             fleet=_fleet(3, 3, ALL_HOSTS)),
        _pkg("php", "8.1.27", "1.el9", source_repo="rhel-9-appstream",
             fleet=_fleet(3, 3, ALL_HOSTS)),
        # 66% prevalence (2 of 3 hosts)
        _pkg("redis", "7.0.12", "1.el9", source_repo="epel-9",
             fleet=_fleet(2, 3, ["web-01", "web-02"])),
        _pkg("memcached", "1.6.22", "1.el9", source_repo="epel-9",
             fleet=_fleet(2, 3, ["web-02", "web-03"])),
        # 33% prevalence (1 of 3 hosts)
        _pkg("nginx", "1.24.0", "2.el9", source_repo="epel-9",
             fleet=_fleet(1, 3, ["web-03"])),
        _pkg("certbot", "2.6.0", "1.el9", source_repo="epel-9",
             fleet=_fleet(1, 3, ["web-01"])),
        # Excluded package (triage: user toggled off)
        _pkg("debug-tools", "1.0.0", "1.el9", include=False,
             fleet=_fleet(3, 3, ALL_HOSTS)),
    ]

    repo_files = [
        RepoFile(
            path="etc/yum.repos.d/epel.repo",
            content="[epel]\nname=EPEL 9\nbaseurl=https://dl.fedoraproject.org/pub/epel/9/$basearch\nenabled=1\ngpgcheck=1\n",
            is_default_repo=False,
            fleet=_fleet(3, 3, ALL_HOSTS),
        ),
    ]

    rpm = RpmSection(
        packages_added=packages_added,
        repo_files=repo_files,
        base_image="quay.io/centos-bootc/centos-bootc:stream9",
    )

    # --- Config files with variant ties ---
    config = ConfigSection(files=[
        # 2-way tie: /etc/app.conf appears on 2 hosts with same content
        _cfg("/etc/app.conf",
             kind=ConfigFileKind.UNOWNED,
             content="# App config variant A\nworkers=4\nlog_level=info\n",
             fleet=_fleet(2, 3, ["web-01", "web-02"])),
        _cfg("/etc/app.conf",
             kind=ConfigFileKind.UNOWNED,
             content="# App config variant B\nworkers=8\nlog_level=debug\n",
             fleet=_fleet(1, 3, ["web-03"])),
        # 3-way tie: /etc/httpd/conf/httpd.conf all different
        _cfg("/etc/httpd/conf/httpd.conf",
             kind=ConfigFileKind.RPM_OWNED_MODIFIED,
             category=ConfigCategory.OTHER,
             content="# httpd.conf variant 1\nServerName web-01.example.com\nMaxClients 256\n",
             fleet=_fleet(1, 3, ["web-01"])),
        _cfg("/etc/httpd/conf/httpd.conf",
             kind=ConfigFileKind.RPM_OWNED_MODIFIED,
             category=ConfigCategory.OTHER,
             content="# httpd.conf variant 2\nServerName web-02.example.com\nMaxClients 512\n",
             fleet=_fleet(1, 3, ["web-02"])),
        _cfg("/etc/httpd/conf/httpd.conf",
             kind=ConfigFileKind.RPM_OWNED_MODIFIED,
             category=ConfigCategory.OTHER,
             content="# httpd.conf variant 3\nServerName web-03.example.com\nMaxClients 128\n",
             fleet=_fleet(1, 3, ["web-03"])),
        # Clear winner: /etc/nginx/nginx.conf same on all hosts
        _cfg("/etc/nginx/nginx.conf",
             kind=ConfigFileKind.RPM_OWNED_MODIFIED,
             content="# nginx.conf\nworker_processes auto;\nevents { worker_connections 1024; }\n",
             fleet=_fleet(3, 3, ALL_HOSTS)),
        # Excluded config (triage: user toggled off)
        _cfg("/etc/motd",
             kind=ConfigFileKind.UNOWNED,
             content="Welcome to web server\n",
             include=False,
             fleet=_fleet(3, 3, ALL_HOSTS)),
    ])

    # --- Services with variants ---
    services = ServiceSection(state_changes=[
        _svc("httpd.service", "enabled", "disabled", "enable",
             fleet=_fleet(3, 3, ALL_HOSTS)),
        _svc("redis.service", "enabled", "disabled", "enable",
             fleet=_fleet(2, 3, ["web-01", "web-02"])),
        _svc("firewalld.service", "disabled", "enabled", "disable",
             fleet=_fleet(3, 3, ALL_HOSTS)),
        # Excluded service
        _svc("debug-helper.service", "enabled", "disabled", "enable",
             include=False,
             fleet=_fleet(1, 3, ["web-01"])),
    ])

    # --- Redacted secret ---
    redactions = [
        {
            "path": "/etc/app-secrets.conf",
            "reason": "Contains API key pattern",
            "pattern": "API_KEY=sk-.*",
        },
    ]

    return InspectionSnapshot(
        schema_version=SCHEMA_VERSION,
        meta={
            "hostname": "fleet-merged",
            "timestamp": "2026-03-30T12:00:00Z",
            "profile": "web-servers",
            "fleet": FleetMeta(
                source_hosts=ALL_HOSTS,
                total_hosts=3,
                min_prevalence=33,
            ).model_dump(),
        },
        os_release=os_rel,
        rpm=rpm,
        config=config,
        services=services,
        redactions=redactions,
    )


# ---------------------------------------------------------------------------
# Fixture: Single-host
# ---------------------------------------------------------------------------

def build_single_host_snapshot() -> InspectionSnapshot:
    """Minimal single-host snapshot with no fleet data."""
    os_rel = OsRelease(
        name="CentOS Stream",
        version_id="9",
        version="9",
        id="centos",
        id_like="rhel fedora",
        pretty_name="CentOS Stream 9",
    )

    rpm = RpmSection(
        packages_added=[
            _pkg("vim-enhanced", "9.0.2081", "1.el9"),
            _pkg("tmux", "3.3a", "3.el9"),
            _pkg("git", "2.43.0", "1.el9"),
        ],
        base_image="quay.io/centos-bootc/centos-bootc:stream9",
    )

    config = ConfigSection(files=[
        _cfg("/etc/vimrc.local",
             kind=ConfigFileKind.UNOWNED,
             content="set number\nset tabstop=4\nsyntax on\n"),
        _cfg("/etc/tmux.conf",
             kind=ConfigFileKind.UNOWNED,
             content="set -g mouse on\nset -g history-limit 50000\n"),
    ])

    services = ServiceSection(state_changes=[
        _svc("sshd.service", "enabled", "enabled", "unchanged"),
        _svc("chronyd.service", "enabled", "disabled", "enable"),
    ])

    return InspectionSnapshot(
        schema_version=SCHEMA_VERSION,
        meta={
            "hostname": "dev-workstation",
            "timestamp": "2026-03-30T12:00:00Z",
            "profile": "single-host",
        },
        os_release=os_rel,
        rpm=rpm,
        config=config,
        services=services,
    )


# ---------------------------------------------------------------------------
# Fixture: Architect topology (3 fleet tarballs)
# ---------------------------------------------------------------------------

# Shared packages (~10) across all three fleets
SHARED_PACKAGES = [
    "bash-completion", "bind-utils", "curl", "jq", "lsof",
    "net-tools", "rsync", "strace", "tcpdump", "unzip",
]


def _architect_fleet(
    fleet_name: str,
    hosts: list[str],
    unique_packages: list[str],
) -> InspectionSnapshot:
    """Build an architect fleet snapshot with shared + unique packages."""
    all_pkgs: list[PackageEntry] = []
    host_count = len(hosts)
    fp = _fleet(host_count, host_count, hosts)

    # Shared packages (present across all fleets)
    for pkg_name in SHARED_PACKAGES:
        all_pkgs.append(_pkg(pkg_name, "1.0", "1.el9", fleet=fp))

    # Unique packages
    for pkg_name in unique_packages:
        all_pkgs.append(_pkg(pkg_name, "1.0", "1.el9", fleet=fp))

    os_rel = OsRelease(
        name="Red Hat Enterprise Linux",
        version_id="9.4",
        version="9.4 (Plow)",
        id="rhel",
        id_like="fedora",
        pretty_name="Red Hat Enterprise Linux 9.4 (Plow)",
    )

    return InspectionSnapshot(
        schema_version=SCHEMA_VERSION,
        meta={
            "hostname": f"{fleet_name}-merged",
            "timestamp": "2026-03-30T12:00:00Z",
            "profile": fleet_name,
            "fleet": FleetMeta(
                source_hosts=hosts,
                total_hosts=host_count,
                min_prevalence=50,
            ).model_dump(),
        },
        os_release=os_rel,
        rpm=RpmSection(
            packages_added=all_pkgs,
            base_image="quay.io/centos-bootc/centos-bootc:stream9",
        ),
        config=ConfigSection(files=[
            _cfg(f"/etc/{fleet_name}/app.conf",
                 content=f"# {fleet_name} config\n",
                 fleet=fp),
        ]),
        services=ServiceSection(state_changes=[
            _svc(f"{fleet_name}.service", fleet=fp),
        ]),
    )


def build_architect_snapshots() -> dict[str, InspectionSnapshot]:
    """Three fleet tarballs with overlapping + unique packages."""
    return {
        "web-servers": _architect_fleet(
            "web-servers",
            ["web-01", "web-02", "web-03"],
            ["httpd", "mod_ssl", "php", "php-fpm", "mod_security"],
        ),
        "db-servers": _architect_fleet(
            "db-servers",
            ["db-01", "db-02"],
            ["postgresql-server", "postgresql-contrib", "pgaudit",
             "pg_stat_monitor", "barman"],
        ),
        "app-servers": _architect_fleet(
            "app-servers",
            ["app-01", "app-02", "app-03"],
            ["java-17-openjdk", "tomcat", "maven", "redis", "haproxy"],
        ),
    }


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def fixtures_are_current() -> bool:
    """Check if .schema-version matches SCHEMA_VERSION."""
    if not SCHEMA_VERSION_FILE.exists():
        return False
    stored = SCHEMA_VERSION_FILE.read_text().strip()
    return stored == str(SCHEMA_VERSION)


def write_schema_version() -> None:
    """Write SCHEMA_VERSION to the cache file."""
    SCHEMA_VERSION_FILE.write_text(str(SCHEMA_VERSION) + "\n")


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate E2E test fixtures")
    parser.add_argument(
        "--force", action="store_true",
        help="Regenerate fixtures even if schema version hasn't changed",
    )
    args = parser.parse_args()

    if not args.force and fixtures_are_current():
        print("Fixtures up to date, skipping.")
        return

    print(f"Generating fixtures for schema version {SCHEMA_VERSION}...")

    # Ensure directories exist
    FIXTURES_DIR.mkdir(parents=True, exist_ok=True)
    ARCHITECT_DIR.mkdir(parents=True, exist_ok=True)

    # Fleet (3-host)
    print("  Fleet (3-host)...")
    fleet_snapshot = build_fleet_snapshot()
    _render_to_tarball(fleet_snapshot, FIXTURES_DIR / "fleet-3host.tar.gz")

    # Single-host
    print("  Single-host...")
    single_snapshot = build_single_host_snapshot()
    _render_to_tarball(single_snapshot, FIXTURES_DIR / "single-host.tar.gz")

    # Architect topology (3 fleets)
    architect_snapshots = build_architect_snapshots()
    for fleet_name, snapshot in architect_snapshots.items():
        print(f"  Architect: {fleet_name}...")
        _render_to_tarball(snapshot, ARCHITECT_DIR / f"{fleet_name}.tar.gz")

    # Write schema version cache
    write_schema_version()

    print("Done. Fixtures written to:")
    print(f"  {FIXTURES_DIR / 'fleet-3host.tar.gz'}")
    print(f"  {FIXTURES_DIR / 'single-host.tar.gz'}")
    for fleet_name in architect_snapshots:
        print(f"  {ARCHITECT_DIR / f'{fleet_name}.tar.gz'}")
    print(f"  {SCHEMA_VERSION_FILE}")


if __name__ == "__main__":
    main()
