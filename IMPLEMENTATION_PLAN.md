# inspectah Codebase Analysis & Implementation Plan

## Executive Summary

**inspectah** is a Python 3.11+ CLI tool that inspects running RHEL/CentOS/Fedora hosts and generates bootc container image artifacts for atomic OS migration. The codebase is well-structured with clear separation of concerns: inspectors (data collection), schema (strong typing), renderers (output generation), and a pipeline orchestrator.

---

## 1. Project Structure

### Root Directory Layout
```
/Users/mrussell/Work/bootc-migration/inspectah/
├── src/inspectah/                    # Main package
│   ├── __main__.py               # CLI entry point
│   ├── cli.py                    # argparse configuration
│   ├── pipeline.py               # Orchestrator
│   ├── schema.py                 # Pydantic data models
│   ├── inspectors/               # Data collectors
│   │   ├── __init__.py
│   │   ├── rpm.py               # RPM/packages inspector
│   │   ├── config.py            # Config files inspector
│   │   ├── service.py           # Systemd services
│   │   ├── network.py           # Network config
│   │   ├── storage.py           # Disk/filesystem
│   │   ├── scheduled_tasks.py   # Cron/timers
│   │   ├── container.py         # Container workloads
│   │   ├── non_rpm_software.py  # Non-packaged software
│   │   ├── kernel_boot.py       # Kernel params
│   │   ├── selinux.py           # SELinux config
│   │   └── users_groups.py      # Users/groups
│   ├── renderers/                # Output generators
│   │   ├── __init__.py          # run_all() orchestrates all
│   │   ├── containerfile/       # Dockerfile generation
│   │   ├── audit_report.py      # Markdown audit report
│   │   ├── html_report.py       # Interactive HTML dashboard
│   │   ├── readme.py            # Build instructions
│   │   ├── kickstart.py         # Anaconda kickstart
│   │   ├── secrets_review.py    # Redacted secrets
│   │   └── merge_notes.py       # Fleet merge notes
│   ├── fleet/                    # Multi-host aggregation
│   ├── refine/                   # Interactive browser editor
│   ├── architect/                # Layer decomposition planner
│   ├── templates/                # Jinja2 templates
│   ├── validate.py              # `podman build` validation
│   ├── preflight.py             # Runtime checks (podman, root, etc)
│   ├── baseline.py              # Resolve base image packages
│   ├── subscription.py          # RHEL cert bundling
│   ├── redact.py                # Sensitive data masking
│   ├── heuristic.py             # Smart classification
│   ├── packaging.py             # Tarball handling
│   └── system_type.py           # OS detection
├── tests/                        # Test suite
│   ├── conftest.py              # Pytest fixtures
│   ├── fixtures/                # Mock /etc directories
│   ├── e2e/                     # Playwright browser tests
│   └── test_*.py                # ~20 test modules
├── docs/                         # User documentation
├── pyproject.toml               # Build config
├── inspectah-build                 # Shell wrapper for building images
└── run-inspectah.sh                # Container invocation wrapper
```

### Go Module Info
**Not a Go project** — this is Python. Uses `setuptools` via `pyproject.toml`.

---

## 2. CLI Architecture

### Entry Point: `src/inspectah/__main__.py`
- **Function**: `main(argv: Optional[list] = None, cwd: Optional[Path] = None) -> int`
- **Structure**: 
  - Calls `parse_args(argv)` from `cli.py`
  - Matches on `args.command` (inspect, fleet, refine, architect)
  - Delegates to handler: `_run_inspect()`, `_run_fleet()`, `run_refine()`, `run_architect()`
  - Returns 0 on success, 1 on error

### Command Registration: `src/inspectah/cli.py`
- **Parser**: Standard `argparse.ArgumentParser`
- **Subcommands**: tuple `("inspect", "fleet", "refine", "architect")`
- **Backwards Compat**: Bare flags without a subcommand prepend "inspect" automatically
  - `inspectah --from-snapshot f` → `inspectah inspect --from-snapshot f`
- **Pattern**: 
  ```python
  parser = argparse.ArgumentParser(prog="inspectah")
  subparsers = parser.add_subparsers(dest="command")
  # Each subcommand adds its own args
  ```

### Key Flags (inspect subcommand)
| Flag | Type | Purpose |
|------|------|---------|
| `--host-root` | Path | Root for chroot (for containerized inspection) |
| `--output-file` / `-o` | Path | Single tarball output |
| `--output-dir` | Path | Unpacked directory output |
| `--from-snapshot` | Path | Skip inspection; load snapshot and render only |
| `--inspect-only` | Flag | Run inspectors; save snapshot; skip renderers |
| `--target-version` | str | RHEL version for base image (e.g., "9.6") |
| `--target-image` | str | Full base image reference (overrides --target-version) |
| `--baseline-packages` | Path | Custom baseline (air-gapped mode) |
| `--no-baseline` | Flag | Allow running without baseline (generates all packages) |
| `--user-strategy` | str | How to create users: sysusers/kickstart/useradd/auto |
| `--config-diffs` | Flag | Generate line-by-line diffs for modified configs |
| `--deep-binary-scan` | Flag | Scan non-RPM software (slow) |
| `--query-podman` | Flag | Connect to podman socket to enumerate containers |
| `--skip-preflight` | Flag | Skip privilege/podman/registry checks |
| `--validate` | Flag | Run `podman build` to verify Containerfile |
| `--push-to-github` | str | GitHub token to auto-push image |

---

## 3. Existing Analysis/Checking Infrastructure

### Preflight Checks: `src/inspectah/preflight.py`
Runs before inspection (unless `--skip-preflight`):
- `check_podman()` — Verify podman is installed
- `check_root()` — Verify running as root
- `check_registry_login()` — Verify logged in to registries (for RHEL images)
- `check_container_privileges()` — Verify `--pid=host`, `--privileged`, SELinux label=disable
- `requires_registry_login()` — Detect if RHEL images need auth

### Triage System: `src/inspectah/renderers/_triage.py`
Classifies packages/configs/users for priority/attention:
- **Classes**:
  - `NoteMajority` — Config used by multiple hosts
  - `NoteUnchanged` — Host uses baseline value
  - `NoteConflicting` — Fleet hosts disagree on value
  - `NoteOrphan` — From removed package
  - `NoteWarning` — Potential migration issue
  - `NoteError` — Blocker (e.g., incompatible syscall limit)
- **Methods**:
  - `triage_packages()` — Mark attention-needed RPMs
  - `triage_configs()` — Mark attention-needed files
  - `triage_users()` — Mark attention-needed users

### Validation: `src/inspectah/validate.py`
- `run_validate(output_dir)` — Runs `podman build` on generated Containerfile
- Catches missing packages, bad syntax, registry errors

### Heuristic Classification: `src/inspectah/heuristic.py`
- `find_heuristic_candidates()` — Identify non-RPM software by path patterns
- `apply_noise_control()` — Filter out noise (build artifacts, cache, VCS)
- Skips: `.git`, `node_modules`, `__pycache__`, build trees, IDE metadata

---

## 4. Testing Patterns

### Test Organization
- **Framework**: pytest 7.0+
- **Config**: `pyproject.toml` → `testpaths = ["tests"]`, `pythonpath = ["src"]`
- **Coverage**: pytest-cov enabled
- **Tests**: ~20 modules in `/tests/test_*.py`

### Key Test Modules
| File | Purpose |
|------|---------|
| `test_preflight.py` | Preflight check validation |
| `test_triage.py` | Triage classification logic |
| `test_containerfile_output.py` | Containerfile generation |
| `test_audit_report_output.py` | Audit report rendering |
| `test_plan_containerfile.py` | Image build planning |
| `test_plan_services.py` | Systemd service handling |
| `test_fleet_merge.py` | Multi-host aggregation |
| `test_heuristic.py` | Non-RPM software detection |
| `test_ostree_rpm.py` | ostree-specific RPM handling |
| `test_redact.py` | Secrets masking |

### Fixtures: `tests/conftest.py`
- **Patterns**:
  - `tests/fixtures/host_etc/` — Mock filesystem hierarchy for testing
  - Contains: `/etc/`, `/opt/`, `/usr/`, `/proc/`, `/sys/` mock trees
  - Includes: sample configs, venvs, cron jobs, systemd units, container images

### Testing Best Practices
- Fixtures are git-tracked mock `/etc` trees with realistic data
- Tests snapshot JSON and compare structure (not exact strings)
- E2E tests use Playwright for browser interaction (Refine dashboard)
- Integration tests verify full pipeline: inspect → render → validate

---

## 5. Key Types & Interfaces

### Schema (Pydantic v2): `src/inspectah/schema.py`
All data flows through `InspectionSnapshot` — single source of truth.

**Core Classes**:
```python
# Enums for state tracking
class PackageState(str, Enum):
    ADDED = "added"                    # Not in base image
    BASE_IMAGE_ONLY = "base_image_only"  # Not on host
    MODIFIED = "modified"              # Version differs

class VersionChangeDirection(str, Enum):
    UPGRADE = "upgrade"     # Base image has newer
    DOWNGRADE = "downgrade" # Base image has older

# Strongly typed entries
class PackageEntry(BaseModel):
    name: str
    epoch: str = "0"
    version: str
    release: str
    arch: str
    state: PackageState = PackageState.ADDED
    include: bool = True                # User can deselect
    source_repo: str = ""
    fleet: Optional[FleetPrevalence] = None  # For multi-host

class EnabledModuleStream(BaseModel):
    module_name: str
    stream: str
    profiles: List[str] = []
    include: bool = True
    baseline_match: bool = False
    fleet: Optional[FleetPrevalence] = None

class VersionLockEntry(BaseModel):
    """Pin from /etc/dnf/plugins/versionlock.list"""
    raw_pattern: str
    name: str
    epoch: int = 0
    version: str
    release: str
    arch: str
    include: bool = True

class RpmVaEntry(BaseModel):
    """From `rpm -Va`: modified RPM-owned file"""
    path: str
    flags: str  # e.g., "S.5....T."
    package: Optional[str] = None

class OstreePackageOverride(BaseModel):
    """rpm-ostree override (layered, replaced, removed)"""
    name: str
    from_nevra: str = ""
    to_nevra: str = ""

class RepoFile(BaseModel):
    """Repo definition (.repo file)"""
    path: str
    content: str = ""
    is_default_repo: bool = True
    include: bool = True
    fleet: Optional[FleetPrevalence] = None

class RpmSection(BaseModel):
    """Output of RPM inspector"""
    packages_added: List[PackageEntry]
    base_image_only: List[PackageEntry]
    rpm_va: List[RpmVaEntry]
    repo_files: List[RepoFile]
    gpg_keys: List[RepoFile]
    dnf_history_removed: List[str]
    module_streams: List[EnabledModuleStream]
    version_locks: List[VersionLockEntry]
    multiarch_packages: List[str]
    duplicate_packages: List[str]
    ostree_overrides: List[OstreePackageOverride]
    ostree_removals: List[str]
    base_image: Optional[str]  # e.g., "quay.io/centos-bootc/centos-bootc:stream10"
    no_baseline: bool = False
    warnings: List[str] = Field(default_factory=list)
```

**Config Classes**:
```python
class ConfigFileKind(str, Enum):
    RPM_OWNED_MODIFIED = "rpm_owned_modified"
    UNOWNED = "unowned"
    ORPHANED = "orphaned"

class ConfigCategory(str, Enum):
    """Semantic category from path analysis"""
    TMPFILES = "tmpfiles"
    ENVIRONMENT = "environment"
    AUDIT = "audit"
    LIBRARY_PATH = "library_path"
    JOURNAL = "journal"
    LOGROTATE = "logrotate"
    AUTOMOUNT = "automount"
    SYSCTL = "sysctl"

class ConfigFileEntry(BaseModel):
    path: str
    kind: ConfigFileKind
    category: Optional[ConfigCategory] = None
    is_redacted: bool = False
    redaction_reason: Optional[str] = None
    conflict_summary: Optional[str] = None
    include: bool = True
    fleet: Optional[FleetPrevalence] = None
```

**Fleet Support**:
```python
class FleetPrevalence(BaseModel):
    """Which hosts have this item"""
    host_count: int
    total_hosts: int
    hosts: List[str] = Field(default_factory=list)  # Host IDs
```

**Top-level Snapshot**:
```python
class InspectionSnapshot(BaseModel):
    """Complete inspection result — contract between inspectors & renderers"""
    schema_version: int = SCHEMA_VERSION
    metadata: Metadata
    rpm: Optional[RpmSection] = None
    config: Optional[ConfigSection] = None
    service: Optional[ServiceSection] = None
    network: Optional[NetworkSection] = None
    storage: Optional[StorageSection] = None
    scheduled_tasks: Optional[ScheduledTasksSection] = None
    container: Optional[ContainerSection] = None
    non_rpm_software: Optional[NonRpmSoftwareSection] = None
    kernel_boot: Optional[KernelBootSection] = None
    selinux: Optional[SelinuxSection] = None
    users_groups: Optional[UsersGroupsSection] = None
    warnings: List[Warning] = Field(default_factory=list)
```

---

## 6. Build System

### Build Tool: setuptools + pyproject.toml
```toml
[build-system]
requires = ["setuptools>=61", "wheel"]
build-backend = "setuptools.build_meta"

[project]
name = "inspectah"
version = "0.5.1"
requires-python = ">=3.11"
dependencies = ["pydantic>=2.0", "jinja2>=3.1"]

[project.scripts]
inspectah = "inspectah.__main__:main"

[tool.setuptools.packages.find]
where = ["src"]

[tool.setuptools.package-data]
inspectah = ["templates/*.j2", "templates/*.css", "static/**/*"]
```

### Build Artifacts
- **Container Image**: Published to `ghcr.io/marrusl/inspectah:latest` (multi-arch: amd64 + arm64)
- **Image Definition**: `Containerfile` in repo root
- **Wrapper Script**: `run-inspectah.sh` — handles podman install, login, image pull
- **Build Helper**: `inspectah-build` — shell script for building bootc images from inspectah output
- **CI/CD**: `.github/workflows/`, `.copr/` (COPR package build)

---

## 7. Config/Options Pattern

### Flags are parsed in `cli.py`
- **argparse.Namespace** object passed to handlers
- **Handlers** in `__main__.py` (_run_inspect, _run_fleet, etc.)
- **Pipeline** (pipeline.py) receives parsed args + extracts needed values

### Key Option Handling Pattern
```python
# In cli.py: Define args
parser.add_argument("--target-version", type=str, metavar="VERSION", help="...")

# In __main__.py: Pass to handler
args = parse_args(argv)
match args.command:
    case "inspect":
        return _run_inspect(args)

# In _run_inspect: Extract and use
def _run_inspect(args):
    snapshot = run_pipeline(
        host_root=args.host_root,
        target_version=args.target_version,
        target_image=args.target_image,
        no_baseline=args.no_baseline,
        # ... more args
    )
```

### Environment Variables (Runtime Overrides)
| Variable | Effect |
|----------|--------|
| `INSPECTAH_IMAGE` | Override container image reference |
| `INSPECTAH_HOSTNAME` | Override reported hostname |
| `INSPECTAH_DEBUG` | Set to 1 for full tracebacks |

---

## 8. Error Handling Pattern

### Style: Explicit, Readable, Non-Intrusive
```python
# In preflight.py and __main__.py
try:
    check_podman()
except RuntimeError as e:
    print(f"Error: {e}", file=sys.stderr)
    return 1

# NotImplementedError for planned but unfinished features
except NotImplementedError as e:
    print(f"Error: {e}", file=sys.stderr)
    return 1

# Generic catch with debug support
except Exception as e:
    print(f"Error: {e}", file=sys.stderr)
    if os.environ.get("INSPECTAH_DEBUG"):
        traceback.print_exc()
    else:
        print("Set INSPECTAH_DEBUG=1 for the full traceback.", file=sys.stderr)
    return 1
```

### Inspector Error Handling: `_safe_run()`
```python
def _safe_run(name: str, fn: Callable[[], T], default: T, warnings: list) -> T:
    """Run an inspector; on PermissionError/OSError return default + warn."""
    try:
        return fn()
    except (PermissionError, OSError) as e:
        warnings.append(make_warning(name, f"Skipped: {e}"))
        return default
```

### Validation Errors
- Pydantic validates snapshot structure
- `model_validate_json()` raises `ValidationError` if snapshot corrupted
- User-facing: "Set INSPECTAH_DEBUG=1 for details"

---

## 9. RPM/System Analysis Code

### RPM Inspector: `src/inspectah/inspectors/rpm.py`
**Calls via nsenter (for containerized inspection)**:
- `rpm -qa` — All installed packages
- `rpm -Va` — Modified RPM-owned files
- `dnf module list --enabled` — Enabled module streams
- `cat /etc/dnf/plugins/versionlock.list` — Version pins
- `dnf history` — Removed packages
- `rpm -E %{_rpmfilesdir}` — GPG keys directory

**Returns**: `RpmSection` (Pydantic model)

### Package Detection Pattern
```python
def run_rpm(...) -> RpmSection:
    # 1. Query baseline (or skip if --no-baseline)
    baseline_pkgs = resolver.resolve(target_image) if target_image else None
    
    # 2. Parse rpm -qa on host
    host_pkgs = parse_rpm_qa(executor)
    
    # 3. Diff: mark as ADDED, MODIFIED, or BASE_IMAGE_ONLY
    for pkg in host_pkgs:
        if pkg not in baseline_pkgs:
            pkg.state = PackageState.ADDED
        elif pkg.version != baseline_pkgs[pkg.name].version:
            pkg.state = PackageState.MODIFIED
    
    # 4. Collect module streams, repos, GPG keys
    return RpmSection(packages_added=..., module_streams=..., ...)
```

### Baseline Resolution: `src/inspectah/baseline.py`
- Maps host version + distro to base bootc image
- Pulls image (cached), extracts package list, caches in `~/.cache/inspectah/`
- Handles registry auth, caching, fallback to defaults
- **Class**: `BaselineResolver`
  - `resolve(target_image: Optional[str]) -> Optional[List[PackageEntry]]`

### DNF/RPM Commands Used
| Command | Purpose |
|---------|---------|
| `rpm -qa --qf='...'` | All installed packages with metadata |
| `rpm -Va` | Verify modified files |
| `dnf module list --enabled` | Enabled module streams |
| `dnf repolist -v` | All enabled repos |
| `rpm -E %{_rpmfilesdir}` | GPG keys directory |
| `dnf history` | Removed packages from history |

---

## 10. Inspector Pattern

### Base Pattern (All Inspectors Follow This)
```python
# In inspectors/__init__.py or each module:
def run_XXXXX(
    host_root: Path,
    executor: CommandExecutor,
    warnings: list = None,
    **kwargs
) -> Optional[XxxxxSection]:
    """
    Inspect aspect of the system.
    
    Returns: Pydantic model for this aspect, or None if data unavailable.
    On error: return default/None and append to warnings list.
    """
    try:
        # 1. Collect data (subprocess calls via executor)
        # 2. Parse into strongly-typed models
        # 3. Cross-reference with baseline (if applicable)
        # 4. Return Pydantic model
        return XxxxxSection(...)
    except (PermissionError, OSError) as e:
        if warnings:
            warnings.append(make_warning("xxxxx", str(e)))
        return None
```

### Inspector Registration: `src/inspectah/pipeline.py`
```python
_TOTAL_STEPS = 11  # Hard-coded step count for progress

_section_banner("Packages", 1, _TOTAL_STEPS)
snapshot.rpm = _safe_run("rpm", _run_rpm_inspector, None, warnings)

_section_banner("Config", 2, _TOTAL_STEPS)
snapshot.config = _safe_run("config", lambda: run_config(...), None, warnings)

# ... etc for service, network, storage, scheduled_tasks, container, etc.
```

### Current Inspectors
| Inspector | File | Output Section |
|-----------|------|-----------------|
| RPM/Packages | `inspectors/rpm.py` | `RpmSection` |
| Config Files | `inspectors/config.py` | `ConfigSection` |
| Systemd Services | `inspectors/service.py` | `ServiceSection` |
| Network Config | `inspectors/network.py` | `NetworkSection` |
| Storage/Filesystems | `inspectors/storage.py` | `StorageSection` |
| Cron/Timers | `inspectors/scheduled_tasks.py` | `ScheduledTasksSection` |
| Container Workloads | `inspectors/container.py` | `ContainerSection` |
| Non-RPM Software | `inspectors/non_rpm_software.py` | `NonRpmSoftwareSection` |
| Kernel Params | `inspectors/kernel_boot.py` | `KernelBootSection` |
| SELinux Config | `inspectors/selinux.py` | `SelinuxSection` |
| Users & Groups | `inspectors/users_groups.py` | `UsersGroupsSection` |

---

## 11. Renderer Pattern

### Base Pattern (All Renderers Follow This)
```python
# In renderers/__init__.py:
def run_all(
    snapshot: InspectionSnapshot,
    output_dir: Path,
    refine_mode: bool = False,
    original_snapshot_path: Optional[Path] = None,
) -> None:
    """Run all renderers. Creates output_dir if needed."""
    output_dir.mkdir(parents=True, exist_ok=True)
    
    # Load Jinja2 templates
    templates_dir = Path(__file__).parent.parent / "templates"
    env = Environment(loader=FileSystemLoader(...), autoescape=True)
    
    # Run each renderer (they write to output_dir)
    render_containerfile(snapshot, env, output_dir)
    render_audit_report(snapshot, env, output_dir)
    render_html_report(snapshot, env, output_dir)
    render_readme(snapshot, env, output_dir)
    # ... etc.
```

### Renderer Signatures
```python
def render(snapshot: InspectionSnapshot, env: Environment, output_dir: Path) -> None:
    """Consume snapshot, use Jinja2 env, write to output_dir"""
    template = env.get_template("template_name.j2")
    output = template.render(snapshot=snapshot, ...)
    (output_dir / "filename").write_text(output)
```

### Current Renderers
| Renderer | File | Output |
|----------|------|--------|
| Containerfile | `renderers/containerfile/` | `Containerfile` |
| Config Tree | `renderers/containerfile/_config_tree.py` | `config/` dir |
| Audit Report | `renderers/audit_report.py` | `audit-report.md` |
| HTML Report | `renderers/html_report.py` | `report.html` |
| README | `renderers/readme.py` | `README.md` |
| Kickstart | `renderers/kickstart.py` | `kickstart-suggestion.ks` |
| Secrets Review | `renderers/secrets_review.py` | `secrets-review.md` |
| Merge Notes | `renderers/merge_notes.py` | `.merge-notes.json` |

---

## 12. Pipeline Flow

### High-Level Execution: `src/inspectah/pipeline.py`
```
main()
  ↓
parse_args() → args.command
  ↓
_run_inspect(args)
  ↓
run_pipeline(
    host_root, executor, baseline, target_image,
    warnings, refine_mode, ...
)
  ├─ Preflight (if not --skip-preflight)
  │   ├─ check_podman()
  │   ├─ check_root()
  │   ├─ check_registry_login()
  │   └─ check_container_privileges()
  │
  ├─ OS Detection
  │   ├─ Detect system_type (RHEL, CentOS, Fedora, ostree)
  │   └─ Map to base bootc image
  │
  ├─ Baseline Resolution (preflight)
  │   └─ Query base image package list (cached)
  │
  ├─ Run 11 Inspectors (with _safe_run wrapper)
  │   ├─ run_rpm() → snapshot.rpm
  │   ├─ run_config() → snapshot.config
  │   ├─ run_service() → snapshot.service
  │   ├─ run_network() → snapshot.network
  │   ├─ run_storage() → snapshot.storage
  │   ├─ run_scheduled_tasks() → snapshot.scheduled_tasks
  │   ├─ run_container() → snapshot.container
  │   ├─ run_non_rpm_software() → snapshot.non_rpm_software
  │   ├─ run_kernel_boot() → snapshot.kernel_boot
  │   ├─ run_selinux() → snapshot.selinux
  │   └─ run_users_groups() → snapshot.users_groups
  │
  ├─ Redaction (secrets masking)
  │   └─ redact_snapshot(snapshot)
  │
  ├─ Subscription Bundling (if not --no-subscription)
  │   └─ bundle_subscription_certs(snapshot, output_dir)
  │
  ├─ Snapshot Save
  │   └─ output_dir/inspection-snapshot.json
  │
  └─ (if not --inspect-only) Run Renderers
      └─ renderers.run_all(snapshot, output_dir)
          ├─ Containerfile
          ├─ audit-report.md
          ├─ report.html
          ├─ README.md
          ├─ etc.
          └─ config/ tree
```

### Return Value Handling
- Returns 0 on success
- Returns 1 on error (with stderr message)
- Catches NotImplementedError (planned features)
- Supports debug mode via `INSPECTAH_DEBUG=1`

---

## 13. Key Implementation Details for New Analyzers

### Adding a New Analyzer/Check

#### Step 1: Define Schema
Create entry in `src/inspectah/schema.py`:
```python
class YourAnalysisEntry(BaseModel):
    """Single item in your analysis"""
    name: str
    severity: Optional[str] = None
    include: bool = True
    fleet: Optional[FleetPrevalence] = None

class YourAnalysisSection(BaseModel):
    """Output of your analyzer"""
    items: List[YourAnalysisEntry] = Field(default_factory=list)
    warnings: List[str] = Field(default_factory=list)

# Add to InspectionSnapshot
class InspectionSnapshot(BaseModel):
    # ... existing fields ...
    your_analysis: Optional[YourAnalysisSection] = None
```

#### Step 2: Implement Inspector
Create `src/inspectah/inspectors/your_analysis.py`:
```python
from ..schema import YourAnalysisEntry, YourAnalysisSection

def run_your_analysis(
    host_root: Path,
    executor: CommandExecutor,
    warnings: list = None,
    **kwargs
) -> Optional[YourAnalysisSection]:
    """Inspect and analyze aspect of system."""
    try:
        items = []
        # Collect data via executor
        result = executor.run(["some", "command"])
        
        # Parse into YourAnalysisEntry objects
        for line in result.stdout.splitlines():
            items.append(YourAnalysisEntry(...))
        
        return YourAnalysisSection(items=items)
    except (PermissionError, OSError) as e:
        if warnings:
            warnings.append(make_warning("your_analysis", str(e)))
        return None
```

#### Step 3: Register in Pipeline
Edit `src/inspectah/pipeline.py`:
```python
_TOTAL_STEPS = 12  # Increment count

_section_banner("Your Analysis", 12, _TOTAL_STEPS)
snapshot.your_analysis = _safe_run(
    "your_analysis",
    lambda: run_your_analysis(
        host_root, executor, warnings=w, resolver=resolver
    ),
    None,
    w
)
```

#### Step 4: Create Renderer
Create `src/inspectah/renderers/your_analysis.py`:
```python
from ..schema import InspectionSnapshot

def render(snapshot: InspectionSnapshot, env, output_dir: Path) -> None:
    """Consume snapshot, write report."""
    if not snapshot.your_analysis:
        return
    
    template = env.get_template("your_analysis.j2")
    output = template.render(
        analysis=snapshot.your_analysis,
        items=snapshot.your_analysis.items,
    )
    (output_dir / "your_analysis_report.md").write_text(output)
```

#### Step 5: Add Template
Create `src/inspectah/templates/your_analysis.j2`:
```jinja2
# Your Analysis Report

{% for item in items %}
- {{ item.name }}
  - Severity: {{ item.severity }}
{% endfor %}
```

#### Step 6: Register Renderer
Edit `src/inspectah/renderers/__init__.py`:
```python
from .your_analysis import render as render_your_analysis

def run_all(...):
    # ... existing renderers ...
    render_your_analysis(snapshot, env, output_dir)
```

#### Step 7: Add Tests
Create `tests/test_your_analysis.py`:
```python
import pytest
from inspectah.inspectors.your_analysis import run_your_analysis
from inspectah.schema import YourAnalysisSection

def test_your_analysis_basic(tmp_path, executor_mock):
    result = run_your_analysis(tmp_path, executor_mock)
    assert isinstance(result, YourAnalysisSection)
    assert len(result.items) > 0

def test_your_analysis_error_handling(tmp_path, executor_mock):
    executor_mock.side_effect = PermissionError("denied")
    result = run_your_analysis(tmp_path, executor_mock, warnings=[])
    assert result is None
```

---

## 14. Development Workflow

### Project Setup
```bash
git clone https://github.com/marrusl/inspectah.git
cd inspectah
python -m venv .venv
source .venv/bin/activate
pip install -e ".[dev]"  # Install with dev dependencies
```

### Running Tests
```bash
pytest tests/                           # All tests
pytest tests/test_your_analysis.py     # Single test module
pytest -xvs tests/test_your_analysis.py::test_name  # Single test
pytest --cov=src/inspectah                # With coverage
```

### Running inspectah Locally
```bash
# Inspect the local host (requires root)
sudo python -m inspectah inspect --output-file test-output.tar.gz

# From snapshot (no root needed)
python -m inspectah inspect --from-snapshot test-output.tar.gz --output-dir test-refine

# In container
sudo ./run-inspectah.sh
```

### Key Files to Understand First
1. `src/inspectah/__main__.py` — Entry point and main flow
2. `src/inspectah/cli.py` — Command registration
3. `src/inspectah/schema.py` — Data model contract
4. `src/inspectah/pipeline.py` — Orchestration
5. `src/inspectah/inspectors/__init__.py` — Inspector registry
6. `src/inspectah/renderers/__init__.py` — Renderer registry

---

## 15. Important Notes for Implementation

### Critical Constraints
- **Python 3.11+** only (type hints, match statements)
- **Pydantic v2** (not v1) — use Field(), BaseModel
- **No Go code** — inspectah is pure Python
- **Container-first** — Always runs in podman with chroot
- **No mutating files** — Inspection-only, generates artifacts

### Running Commands: Use `CommandExecutor`
Never call subprocess directly. Use provided executor:
```python
result = executor.run(["rpm", "-qa"])
if result.returncode != 0:
    raise RuntimeError(f"rpm -qa failed: {result.stderr}")
```

### Baselines are Mandatory (by default)
- Without baseline, all packages included in Containerfile
- Use `--no-baseline` to opt in to this risky behavior
- Preflight fails fast if baseline can't be resolved (unless --skip-preflight)

### Warnings are Tracked
- Each inspector appends to `warnings: list`
- Warnings appear in snapshot + HTML report
- Use `make_warning(section, msg, level="warning")` to create

### Fleet Mode (Multi-Host)
- `inspectah fleet dir/ -p 80` merges N snapshots
- Tracks host prevalence for each item
- Outputs `fleet-snapshot.json` + merged reports

---

## Quick Reference: File Locations

| Purpose | Path |
|---------|------|
| Entry point | `src/inspectah/__main__.py` |
| CLI parser | `src/inspectah/cli.py` |
| Data schema | `src/inspectah/schema.py` |
| Orchestration | `src/inspectah/pipeline.py` |
| Inspectors | `src/inspectah/inspectors/*.py` |
| Renderers | `src/inspectah/renderers/*.py` |
| Preflight checks | `src/inspectah/preflight.py` |
| Baseline resolution | `src/inspectah/baseline.py` |
| Templates | `src/inspectah/templates/*.j2` |
| Tests | `tests/test_*.py` |
| Fixtures | `tests/fixtures/` |
| Build config | `pyproject.toml` |
| Container wrapper | `run-inspectah.sh` |
| Image builder | `inspectah-build` |

---

## Summary

**inspectah is a well-structured Python CLI tool** with clear separation between data collection (inspectors), strong typing (Pydantic schema), output generation (renderers), and orchestration (pipeline). To add a new analyzer:

1. Define Pydantic schema for your data
2. Write inspector function (returns schema object)
3. Register in pipeline
4. Create Jinja2 renderer
5. Add template
6. Write tests

The codebase follows consistent patterns: error handling via try/except and warnings lists, type safety via Pydantic, plugin-like inspector/renderer registry, and comprehensive testing with fixtures. A developer with zero context can follow this plan to extend inspectah with custom analyzers or new report types.
