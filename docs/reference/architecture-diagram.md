# inspectah Architecture Diagram

## High-Level Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                     inspectah CLI Entry Point                       │
│                  src/inspectah/__main__.py::main()                  │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
                      parse_args() [cli.py]
                             │
                    ┌────────┴────────┐
                    │   Subcommand?   │
                    └────────┬────────┘
        ┌───────────┬────────┼────────┬──────────┐
        │           │        │        │          │
    scan       fleet      refine  architect   (help)
        │           │        │        │
        └─────────────────────────────┘
              _run_XXXXX() handlers

┌─────────────────────────────────────────────────────────────────┐
│                     SCAN SUBCOMMAND FLOW                         │
│         (Default command; runs actual system analysis)           │
└─────────────────────────────────────────────────────────────────┘

_run_scan(args)
        │
        ▼
    run_pipeline()
        │
        ├─► Preflight Checks [preflight.py]
        │   ├─ check_podman()
        │   ├─ check_root()
        │   ├─ check_registry_login()
        │   └─ check_container_privileges()
        │
        ├─► OS Detection [system_type.py]
        │   ├─ Read /etc/os-release
        │   └─ Detect RHEL/CentOS/Fedora/ostree
        │
        ├─► Baseline Resolution [baseline.py]
        │   ├─ Map host version → bootc base image
        │   ├─ Pull image if needed
        │   └─ Extract + cache package list
        │
        ├─► Run 11 Inspectors (in sequence) [inspectors/*.py]
        │   │
        │   ├─► run_rpm()              → snapshot.rpm
        │   │   └─ rpm -qa, rpm -Va, dnf module list, repos, gpg keys
        │   │
        │   ├─► run_config()           → snapshot.config
        │   │   └─ Find modified /etc files, categorize
        │   │
        │   ├─► run_service()          → snapshot.service
        │   │   └─ systemctl list-unit-files, enabled services
        │   │
        │   ├─► run_network()          → snapshot.network
        │   │   └─ /etc/sysconfig/network*, nmcli, firewall-cmd
        │   │
        │   ├─► run_storage()          → snapshot.storage
        │   │   └─ lsblk, findmnt, crypttab, fstab
        │   │
        │   ├─► run_scheduled_tasks()  → snapshot.scheduled_tasks
        │   │   └─ crontab, /etc/cron.*, systemd timers
        │   │
        │   ├─► run_container()        → snapshot.container
        │   │   └─ podman/docker images, containers, quadlets
        │   │
        │   ├─► run_non_rpm_software() → snapshot.non_rpm_software
        │   │   └─ /opt, /srv, venvs, npm apps, standalone binaries
        │   │
        │   ├─► run_kernel_boot()      → snapshot.kernel_boot
        │   │   └─ GRUB, kernel params, modprobe
        │   │
        │   ├─► run_selinux()          → snapshot.selinux
        │   │   └─ getenforce, policy, contexts
        │   │
        │   └─► run_users_groups()     → snapshot.users_groups
        │       └─ passwd, group, sudoers, ssh keys
        │
        │   [Each wrapped in _safe_run() for error handling]
        │   [Warnings collected in snapshot.warnings]
        │
        ├─► Redaction [redact.py]
        │   └─ Mask secrets, passwords, SSH keys, tokens
        │
        ├─► Subscription Bundling (optional) [subscription.py]
        │   └─ Copy /etc/rhsm/ca.pem, etc. → output
        │
        ├─► Save Snapshot [packaging.py]
        │   └─ inspection-snapshot.json
        │
        └─► Run 8 Renderers (if not --inspect-only) [renderers/__init__.py]
            │
            ├─► render_containerfile()    → Containerfile
            │   └─ Multi-layer Dockerfile optimized for cache hits
            │
            ├─► write_redacted_dir()      → config/ tree
            │   └─ Copy /etc, /opt, /usr files to include in image
            │
            ├─► render_audit_report()     → audit-report.md
            │   └─ Detailed findings, version drift, storage plan
            │
            ├─► render_html_report()      → report.html
            │   └─ Interactive dashboard (Refine source)
            │
            ├─► render_readme()           → README.md
            │   └─ Build commands, FIXME checklist
            │
            ├─► render_kickstart()        → kickstart-suggestion.ks
            │   └─ Anaconda installer config (optional)
            │
            ├─► render_secrets_review()   → secrets-review.md
            │   └─ Redacted sensitive content
            │
            └─► render_merge_notes()      → .merge-notes.json
                └─ (Only in fleet mode)


┌─────────────────────────────────────────────────────────────────┐
│                  DATA FLOW THROUGH SCHEMA                        │
│        (Single source of truth: InspectionSnapshot)              │
└─────────────────────────────────────────────────────────────────┘

Input: Running host (or snapshot.json)
    │
    ▼
Inspectors [inspectors/*.py]
    ├─ run_rpm()        → RpmSection
    ├─ run_config()     → ConfigSection
    ├─ run_service()    → ServiceSection
    ├─ run_network()    → NetworkSection
    ├─ run_storage()    → StorageSection
    ├─ run_scheduled_tasks() → ScheduledTasksSection
    ├─ run_container()  → ContainerSection
    ├─ run_non_rpm_software() → NonRpmSoftwareSection
    ├─ run_kernel_boot()     → KernelBootSection
    ├─ run_selinux()    → SelinuxSection
    └─ run_users_groups()    → UsersGroupsSection
    │
    ▼
InspectionSnapshot [schema.py]
    {
      schema_version: int
      metadata: Metadata
      rpm: RpmSection
      config: ConfigSection
      ... (other sections)
      warnings: List[Warning]
    }
    │
    ├─► Redaction [redact.py]
    │   └─ Mask passwords, SSH keys, tokens
    │
    ├─► JSON Serialization
    │   └─ inspection-snapshot.json
    │
    └─► Renderers [renderers/*.py]
        ├─ render_containerfile(snapshot, ...)
        ├─ render_audit_report(snapshot, ...)
        ├─ render_html_report(snapshot, ...)
        ├─ render_readme(snapshot, ...)
        └─ ... (other renderers)


┌─────────────────────────────────────────────────────────────────┐
│                  INSPECTOR ANATOMY (Example)                     │
│                        rpm.py                                    │
└─────────────────────────────────────────────────────────────────┘

def run_rpm(
    host_root: Path,           # ← Path to inspected filesystem
    executor: CommandExecutor,  # ← Run commands via nsenter/chroot
    baseline_packages_file: Path = None,
    warnings: list = None,
    resolver: BaselineResolver = None,  # ← Get baseline packages
    target_version: str = None,
    target_image: str = None,
    **kwargs
) -> Optional[RpmSection]:
    """Query host packages, compare to baseline, return RpmSection."""
    
    try:
        # 1. Query baseline (if available)
        baseline_pkgs = resolver.resolve(target_image)
        
        # 2. Query host
        host_result = executor.run(["rpm", "-qa", "--qf=..."])
        host_pkgs = parse_rpm_qa(host_result.stdout)
        
        # 3. Diff packages
        packages_added = []
        for pkg in host_pkgs:
            if pkg.name not in baseline_pkgs:
                pkg.state = PackageState.ADDED
                packages_added.append(pkg)
            elif pkg.version != baseline_pkgs[pkg.name].version:
                pkg.state = PackageState.MODIFIED
        
        # 4. Query additional data
        repo_files = query_repos(executor)
        gpg_keys = query_gpg_keys(executor)
        module_streams = query_module_streams(executor)
        rpm_va = query_rpm_va(executor)
        
        # 5. Return strongly-typed section
        return RpmSection(
            packages_added=packages_added,
            repo_files=repo_files,
            gpg_keys=gpg_keys,
            module_streams=module_streams,
            rpm_va=rpm_va,
            base_image=target_image,
            warnings=[],
        )
    
    except (PermissionError, OSError) as e:
        if warnings:
            warnings.append(make_warning("rpm", str(e)))
        return None


┌─────────────────────────────────────────────────────────────────┐
│                  RENDERER ANATOMY (Example)                      │
│                    audit_report.py                               │
└─────────────────────────────────────────────────────────────────┘

def render(
    snapshot: InspectionSnapshot,  # ← Full inspection result
    env: Environment,               # ← Jinja2 environment
    output_dir: Path                # ← Where to write
) -> None:
    """Consume snapshot, render audit report markdown."""
    
    if not snapshot.rpm or not snapshot.rpm.packages_added:
        return  # Skip if no data
    
    # Load template
    template = env.get_template("audit_report.j2")
    
    # Run triage (classify for priority)
    triaged = triage_packages(snapshot.rpm.packages_added)
    
    # Render
    output = template.render(
        snapshot=snapshot,
        packages=triaged,
        warnings=snapshot.warnings,
    )
    
    # Write
    (output_dir / "audit-report.md").write_text(output)


┌─────────────────────────────────────────────────────────────────┐
│                    COMMAND EXECUTOR PATTERN                      │
│             (How to run commands from inspectors)                │
└─────────────────────────────────────────────────────────────────┘

# All inspectors receive an executor
executor: CommandExecutor

# Use it to run commands (always runs in nsenter/chroot context)
result = executor.run(["rpm", "-qa"])

# Result object has:
result.stdout   # str - command output
result.stderr   # str - error output
result.returncode  # int - exit code

# Check for errors
if result.returncode != 0:
    raise RuntimeError(f"Command failed: {result.stderr}")

# Parse output
for line in result.stdout.splitlines():
    # ... process


┌─────────────────────────────────────────────────────────────────┐
│              FLEET MODE (Multi-Host Aggregation)                 │
│                   inspectah fleet dir/                              │
└─────────────────────────────────────────────────────────────────┘

  Host1 Snapshot ──┐
                  ├─→ merge_snapshots() → FleetSnapshot
  Host2 Snapshot ──┤   (Track prevalence per item)
                  │
  Host3 Snapshot ──┘

FleetSnapshot includes:
  - Each item has fleet.host_count / fleet.total_hosts
  - Which hosts have the item (fleet.hosts list)
  - Used by renderers to mark "X of Y hosts have this"


┌─────────────────────────────────────────────────────────────────┐
│              REFINE MODE (Interactive Editor)                    │
│                 inspectah refine *.tar.gz                           │
└─────────────────────────────────────────────────────────────────┘

  Load snapshot
        │
        ▼
  Start web server (Flask/Bottle)
        │
        ├─ Serve HTML dashboard
        ├─ Load/save snapshot.json
        ├─ Toggle items on/off (include: true/false)
        ├─ Edit notes/comments
        └─ Real-time preview of Containerfile, reports
        │
        ▼
  User edits in browser
        │
        ▼
  Save modified snapshot
        │
        ▼
  Re-render all reports


┌─────────────────────────────────────────────────────────────────┐
│                 KEY TYPES & RELATIONSHIPS                        │
└─────────────────────────────────────────────────────────────────┘

InspectionSnapshot
├─ metadata: Metadata
│  ├─ hostname: str
│  ├─ created_at: datetime
│  └─ os_info: OsRelease
│
├─ rpm: RpmSection
│  ├─ packages_added: List[PackageEntry]
│  │  └─ PackageEntry
│  │     ├─ name, epoch, version, release, arch
│  │     ├─ state: PackageState (ADDED, MODIFIED, BASE_IMAGE_ONLY)
│  │     ├─ include: bool (user can toggle)
│  │     └─ fleet: FleetPrevalence (for multi-host)
│  │
│  ├─ module_streams: List[EnabledModuleStream]
│  ├─ version_locks: List[VersionLockEntry]
│  └─ repo_files: List[RepoFile]
│
├─ config: ConfigSection
│  ├─ files: List[ConfigFileEntry]
│  │  └─ ConfigFileEntry
│  │     ├─ path: str
│  │     ├─ kind: ConfigFileKind (RPM_OWNED_MODIFIED, UNOWNED, ORPHANED)
│  │     ├─ category: ConfigCategory (TMPFILES, ENVIRONMENT, SYSCTL, etc.)
│  │     ├─ is_redacted: bool
│  │     └─ include: bool
│  │
│  └─ rpm_owned_paths: Dict[str, str]  # path → package
│
├─ service: ServiceSection
│  └─ services: List[ServiceEntry]
│     ├─ name, state, enabled, type
│     └─ baseline_match: bool
│
├─ network: NetworkSection
├─ storage: StorageSection
├─ scheduled_tasks: ScheduledTasksSection
├─ container: ContainerSection
├─ non_rpm_software: NonRpmSoftwareSection
├─ kernel_boot: KernelBootSection
├─ selinux: SelinuxSection
├─ users_groups: UsersGroupsSection
│
└─ warnings: List[Warning]
   └─ Warning
      ├─ section: str
      ├─ message: str
      └─ level: str (warning, error)
