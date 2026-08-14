# Refine Projection Consolidation

## Purpose

Move single-host view projection logic from the web layer
(`inspectah-web/src/handlers.rs`) into the shared refine engine
(`inspectah-refine`). This creates a single projection contract that
future consumers (web UI, TUI, CLI) can share, eliminating the current pattern
where `handlers.rs` contains 1,500+ lines of domain logic that should
live in the library.

## Scope

### In scope (single-host only)

- Decision projection: builds classified decision data from session state
- Reference projection: builds immutable context data from original snapshot
- Web adapter: transforms domain projections to current wire format
- All 7 view-returning endpoints cut over atomically
- Contract snapshot tests as pre-cutover gate
- `resume_from()` cache rebuild for new projection caches

### Out of scope: fleet

Fleet projection (`fleet_handlers.rs`) is explicitly out of scope. Fleet
uses different aggregation paths (`fleet_view`, `fleet_diff`) and
different data shapes (prevalence scoring, consensus derivation).

**Dependency seam:** Fleet can reuse the shared classifiers (`classify_*`)
and reference-building pieces from `inspectah-refine`, but fleet
aggregation requires its own projection type -- it cannot simply
aggregate `DecisionProjection` instances. `DecisionProjection` is
single-host/view-oriented; fleet needs different grouping (by variant,
by prevalence zone). Fleet adoption is a separate spec.

### What moves to `inspectah-refine`

**Decision projection builders** -- functions that take `&RefineSession`
and produce classified decision data:

| Current function (handlers.rs) | New location (projection.rs) | Returns |
|---|---|---|
| `build_service_decisions` (L444) | `project_decisions()` body | part of `DecisionProjection` |
| `build_container_decisions` (L474) | `project_decisions()` body | part of `DecisionProjection` |
| `build_sysctl_decisions` (L507) | `project_decisions()` body | part of `DecisionProjection` |
| `build_tuned_decisions` (L523) | `project_decisions()` body | part of `DecisionProjection` |
| `build_repo_groups` (L386) | `project_decisions()` body | part of `DecisionProjection` |
| `build_sensitivity_summary` (L864) | uses `session.is_sensitive()` | `bool` field |
| version changes in `build_view_response` (L331) | `project_decisions()` body | part of `DecisionProjection` |
| user/group extraction in `build_view_response` (L360) | `project_decisions()` body | part of `DecisionProjection` |

**No new decision types.** The existing `Refined*` types in
`inspectah-refine/src/types.rs` ARE the decision types. See
[Design decision: reuse existing types](#design-decision-reuse-existing-refined-types).

**Reference data builders** -- functions that take
`&InspectionSnapshot` and return typed domain reference data:

| Current function (handlers.rs) | New function (projection.rs) |
|---|---|
| `normalize_for_reference` (L933, orchestrator) | `project_reference()` |
| `normalize_services` (L1056) | `project_ref_services()` |
| `normalize_version_changes` (L975) | `project_ref_version_changes()` |
| `normalize_containers` (L1324) | `project_ref_containers()` |
| `normalize_network` (L1435) | `project_ref_network()` |
| `normalize_storage` (L1568) | `project_ref_storage()` |
| `normalize_scheduled_tasks` (L1635) | `project_ref_scheduled_tasks()` |
| `normalize_non_rpm_software` (L1721) | `project_ref_non_rpm()` |
| `normalize_kernel_boot` (L1787) | `project_ref_kernel_boot()` |
| `normalize_selinux` (L1952) | `project_ref_selinux()` |

Each `normalize_*` function gets **decomposed**: domain extraction logic
moves to the corresponding `project_ref_*` function; presentation
formatting stays in a per-section web adapter function. See the
[decomposition table](#normalize_-decomposition-table) and
[web adapter design](#per-section-web-adapters).

### What stays in `inspectah-web`

- Per-section web adapter functions (presentation formatting)
- `ContextItem`, `ContextSubsection`, `ReferenceSection` wire types
- HTTP handler functions (get_view, get_sections, etc.)
- Router, CORS, middleware
- `format_evr_pair()`, `typed_service_subtitle()`, `searchable_text` assembly

### Already in `inspectah-refine` (no change)

- `classify_packages`, `classify_configs` -- produce `RefinedPackage`, `RefinedConfig`
- `classify_services` -- produces `Vec<RefinedServiceState>`, `Vec<RefinedDropIn>`
- `classify_containers` -- produces `Vec<RefinedQuadlet>`, `Vec<RefinedFlatpak>`
- `classify_sysctls` -- produces `Vec<RefinedSysctl>`
- `classify_tuned` -- produces `Vec<RefinedTunedSelection>`
- `RefinedView` -- holds packages, configs, containerfile preview, stats

---

## Design

### Design decision: reuse existing Refined\* types

Round 2 proposed new parallel `ServiceDecision`, `DropInDecision`,
`QuadletDecision`, etc. types. This was wrong -- the existing `Refined*`
types already carry exactly the right data:

| Existing type | Fields | Role |
|---|---|---|
| `RefinedServiceState` | `entry: ServiceStateChange`, `triage: TriageTag` | Classified service |
| `RefinedDropIn` | `entry: SystemdDropIn`, `triage: TriageTag` | Classified drop-in |
| `RefinedQuadlet` | `entry: QuadletUnit`, `triage: TriageTag` | Classified quadlet |
| `RefinedFlatpak` | `entry: FlatpakApp`, `triage: TriageTag` | Classified flatpak |
| `RefinedSysctl` | `entry: SysctlOverride`, `triage: TriageTag` | Classified sysctl |
| `RefinedTunedSelection` | `active_profile`, `custom_profiles`, `triage: TriageTag`, **`include: bool`** | Classified tuned |

Each `Refined*` type pairs the full domain entry struct with a
`TriageTag`. The entry structs carry all domain data (unit names, paths,
images, versions, include flags). The `TriageTag` carries `Triage` (with
`SingleHost`/`Fleet` variants), `TriageReason`, and `TriageAnnotation`s.

The existing `classify_*` functions in `classify.rs` already return
these types. The `build_*_decisions` functions in `handlers.rs` just
destructure them into DTO structs that drop most of the entry data.
The fix: skip the DTO layer, use `Refined*` types directly.

`RefinedView` holds `Vec<RefinedPackage>` and `Vec<RefinedConfig>`. The
new `DecisionProjection` holds `Vec<Refined*>` for the remaining section
types plus repo groups, version changes, and user/group decisions.

### Tuned include-state fix

**Problem:** The current `RefinedTunedSelection` lacks an `include`
field. The `build_tuned_decisions` function in `handlers.rs` (L523)
derives `include` from `snapshot_projected().kernel_boot.tuned_include`
and injects it into `TunedDecisionDto`. This means tuned include state
is only available via the web DTO -- it's lost when using `Refined*`
types directly.

**Fix:** Add `include: bool` to `RefinedTunedSelection`:

```rust
// inspectah-refine/src/types.rs  (CHANGE)
pub struct RefinedTunedSelection {
    pub active_profile: String,
    pub custom_profiles: Vec<String>,
    pub triage: TriageTag,
    pub include: bool,  // NEW -- derived from kernel_boot.tuned_include
}
```

**Derivation** (matches existing `build_tuned_decisions` at L525):

```rust
let tuned_include = snap.kernel_boot.as_ref().is_none_or(|kb| kb.tuned_include);
```

When `kernel_boot` is `None`, `tuned_include` defaults to `true`.
When present, it reads `KernelBootSection::tuned_include` (L72 in
`kernelboot.rs`), which the `SetInclude` refinement op toggles
(L1474 in `session.rs`).

**Why this matters:** Without this field, the web adapter would need
to reach back into the session to derive include state, breaking the
projection abstraction. Kit confirmed: hardcoding `include: true`
breaks the `SetInclude` round-trip on the frontend.

### Two projections with distinct lifecycles

```
InspectionSnapshot (original, immutable)
    |
    +--> ReferenceProjection (computed once, OnceLock)
    |      9 typed domain reference sections
    |
    +--> project_snapshot() (applies ops, produces projected snapshot)
           |
           +--> RefinedView (packages + configs, existing)
           |
           +--> DecisionProjection (services, containers, sysctls, tuned,
                  repos, version changes, users/groups, sensitivity)
```

**Reference projection:** Derived from the original (un-refined)
snapshot. Immutable for the session lifetime. Computed lazily on first
access via `OnceLock`. Contains the 9 reference data sections that
show "what's on this host."

**Decision projection:** Derived from the projected (refined) snapshot.
Recomputed on every mutation (apply/undo/redo). Contains classified
items with triage tags and include states.

### `DecisionProjection` type

```rust
// inspectah-refine/src/projection.rs  (NEW)

/// All classified decision data for single-host view rendering.
/// Recomputed on every mutation alongside `RefinedView`.
pub struct DecisionProjection {
    // Services
    pub service_states: Vec<RefinedServiceState>,
    pub service_dropins: Vec<RefinedDropIn>,

    // Containers
    pub quadlets: Vec<RefinedQuadlet>,
    pub flatpaks: Vec<RefinedFlatpak>,

    // Kernel/boot
    pub sysctls: Vec<RefinedSysctl>,
    pub tuned: Vec<RefinedTunedSelection>,  // now carries include: bool

    // Repos
    pub repo_groups: Vec<RepoGroup>,

    // Version changes
    pub version_changes: Vec<VersionChange>,  // core type, not DTO

    // Users & groups
    pub users_groups: Vec<UserGroupDecision>,

    // Sensitivity
    pub is_sensitive: bool,

    // Baseline
    pub baseline_summary: Option<BaselineSummary>,
}
```

The `RepoGroup` type replaces `RepoGroupInfo` in `inspectah-refine`:

```rust
// inspectah-refine/src/types.rs  (NEW, same fields as RepoGroupInfo)
pub struct RepoGroup {
    pub section_id: String,
    pub provenance: RepoProvenance,
    pub is_distro: bool,
    pub tier: RepoTier,
    pub package_count: usize,
    pub enabled: bool,
}
```

### `project_decisions()` compute order

```rust
// inspectah-refine/src/projection.rs

pub fn project_decisions(session: &RefineSession) -> DecisionProjection {
    // IMPORTANT: session.view() must be materialized BEFORE calling this.
    // Both cached_view and cached_decisions are invalidated on mutation;
    // recompute_view() computes view FIRST, then calls project_decisions().
    // This guarantees classify_* sees the same projected snapshot as view.

    let snap = session.snapshot_projected();

    let (service_states, service_dropins) = classify_services(&snap);
    let (quadlets, flatpaks) = classify_containers(&snap);
    let sysctls = classify_sysctls(&snap);

    // Tuned: derive include from projected kernel_boot
    let tuned_include = snap.kernel_boot.as_ref().is_none_or(|kb| kb.tuned_include);
    let tuned: Vec<RefinedTunedSelection> = classify_tuned(&snap)
        .into_iter()
        .map(|mut t| { t.include = tuned_include; t })
        .collect();

    let repo_groups = build_repo_groups(session);

    let version_changes: Vec<VersionChange> = snap
        .rpm
        .as_ref()
        .map(|rpm| rpm.version_changes.clone())
        .unwrap_or_default();

    let users_groups: Vec<UserGroupDecision> = snap
        .users_groups
        .map(|ug| ug.users)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();

    DecisionProjection {
        service_states,
        service_dropins,
        quadlets,
        flatpaks,
        sysctls,
        tuned,
        repo_groups,
        version_changes,
        users_groups,
        is_sensitive: session.is_sensitive(),
        baseline_summary: session.baseline_summary(),
    }
}
```

### `ReferenceProjection` type and domain reference types

```rust
// inspectah-refine/src/projection.rs

/// Immutable reference data derived from the original snapshot.
/// Computed once per session via OnceLock.
pub struct ReferenceProjection {
    // 6 typed sections
    pub services: RefServices,
    pub version_changes: RefVersionChanges,
    pub containers: RefContainers,
    pub kernel_boot: RefKernelBoot,
    pub network: RefNetwork,
    pub storage: RefStorage,

    // 3 generic sections
    pub scheduled_tasks: Vec<GenericRefItem>,
    pub non_rpm_software: Vec<GenericRefItem>,
    pub selinux: Vec<GenericRefItem>,
}
```

#### Typed section: services (`RefServices`)

The most complex section. 5 item categories + 3 subsection types,
matching the current `normalize_services` structure (L1056-L1322):

```rust
/// Domain reference data for the services section.
pub struct RefServices {
    /// Divergent services (state differs from preset).
    /// From state_changes, minus omitted units.
    pub divergent: Vec<RefServiceItem>,
    /// Preset-matched units that have drop-in overrides.
    /// Preset-matched without drop-ins are suppressed.
    pub preset_matched_with_dropins: Vec<RefServiceItem>,
    /// Enabled units with no preset rule.
    pub preset_unknown_enabled: Vec<RefServiceItem>,
    /// Disabled units with no preset rule.
    pub preset_unknown_disabled: Vec<RefServiceItem>,
    /// Standalone drop-ins (unit not in any of the above categories).
    pub standalone_dropins: Vec<RefDropInItem>,
    /// Omitted services (package proven absent via render_service_intent).
    pub omitted: Vec<RefOmittedService>,
    /// Service advisories (presence uncertain).
    pub advisories: Vec<RefServiceAdvisory>,
    /// Service warnings from the collector.
    pub warnings: Vec<RefServiceWarning>,
}

pub struct RefServiceItem {
    pub unit: String,
    pub current_state: ServiceUnitState,
    pub default_state: Option<PresetDefault>,
    /// Owning RPM package name (when known).
    pub owning_package: Option<String>,
    /// Drop-in contents folded into this unit (if any).
    pub dropin_contents: Vec<String>,
}

pub struct RefDropInItem {
    pub unit: String,
    pub content: String,
}

pub struct RefOmittedService {
    pub unit: String,
    pub owning_package: String,
}

pub struct RefServiceAdvisory {
    pub unit: String,
    pub owning_package: String,
    pub reasons: Vec<AdvisoryReason>,
}

pub struct RefServiceWarning {
    pub unit: String,
    pub message: String,
}
```

**Domain logic that moves here:** `render_service_intent()` call,
building the `matched_set`/`divergent_set`/`enabled_set`/`disabled_set`,
drop-in folding by unit (`dropin_by_unit` lookup), omission filtering,
and the 5-way categorization of service items.

#### Typed section: version\_changes (`RefVersionChanges`)

```rust
/// Domain reference data for the version changes section.
pub struct RefVersionChanges {
    /// Downgrades, sorted. Render before upgrades.
    pub downgrades: Vec<RefVersionChangeItem>,
    /// Upgrades, sorted. Render after downgrades.
    pub upgrades: Vec<RefVersionChangeItem>,
    /// Why the section is empty (when both vecs are empty).
    pub empty_reason: Option<EmptyReason>,
}

pub struct RefVersionChangeItem {
    pub name: String,
    pub arch: String,
    pub host_version: String,
    pub base_version: String,
    pub host_epoch: String,
    pub base_epoch: String,
    pub direction: VersionChangeDirection,  // enum, not string
}

/// Why a reference section has no items.
pub enum EmptyReason {
    /// rpm section exists but no baseline data.
    NoBaseline,
    /// Baseline exists but zero version drift detected.
    ZeroDrift,
    /// No rpm section in snapshot at all.
    DataUnavailable,
}
```

**Domain logic that moves here:** Three-state empty reason derivation,
partition into downgrades/upgrades, ordering. `format_evr_pair()` and
directional arrow prefix stay in the web adapter.

#### Typed section: containers (`RefContainers`)

```rust
/// Domain reference data for the containers section.
pub struct RefContainers {
    pub quadlets: Vec<RefQuadletItem>,
    pub compose_files: Vec<RefComposeItem>,
    pub running_containers: Vec<RefRunningContainerItem>,
    pub flatpaks: Vec<RefFlatpakRefItem>,
}

pub struct RefQuadletItem {
    pub name: String,
    pub image: String,
    pub path: String,
    pub content: String,
    pub ports: Vec<String>,
    pub volumes: Vec<String>,
}

pub struct RefComposeItem {
    pub path: String,
    pub services: Vec<ComposeService>,
    pub include: bool,
}

pub struct RefRunningContainerItem {
    pub id: String,
    pub name: String,
    pub image: String,
    pub status: String,
    pub env: Vec<String>,
    pub mounts: Vec<ContainerMount>,
    pub restart_policy: String,
}

/// Mount entry for a running container.
pub struct ContainerMount {
    pub mount_type: String,
    pub source: String,
    pub destination: String,
}

pub struct RefFlatpakRefItem {
    pub app_id: String,
    pub origin: String,
    pub branch: String,
    pub remote: String,
    pub remote_url: String,
}
```

**Domain logic that moves here:** Extraction of quadlet items
(`name`/`image`/`path`/`content`/`ports`/`volumes`), compose files
(`path`/`services`/`include`), running containers
(`id`/`name`/`image`/`status`/`env`/`mounts`/`restart_policy`), and
flatpak apps (`app_id`/`origin`/`branch`/`remote`/`remote_url`) from
`ContainerSection`.

#### Typed section: kernel\_boot (`RefKernelBoot`)

```rust
/// Domain reference data for the kernel/boot section.
pub struct RefKernelBoot {
    pub cmdline: Option<String>,
    pub grub_defaults: Option<String>,
    pub tuned_active: Option<String>,
    pub locale: Option<String>,
    pub timezone: Option<String>,
    pub sysctl_overrides: Vec<RefSysctlOverride>,
    pub non_default_modules: Vec<RefKernelModule>,
    pub modules_load_d: Vec<RefConfigSnippet>,
    pub modprobe_d: Vec<RefConfigSnippet>,
    pub dracut_conf: Vec<RefConfigSnippet>,
    pub custom_tuned_profiles: Vec<RefConfigSnippet>,
    pub alternatives: Vec<RefAlternativeEntry>,
}

pub struct RefSysctlOverride {
    pub key: String,
    pub runtime: String,
    pub default: String,
    pub source: String,
}

pub struct RefAlternativeEntry {
    pub name: String,
    pub path: String,
    pub status: String,
}

pub struct RefConfigSnippet {
    pub path: String,
    pub content: String,
}

pub struct RefKernelModule {
    pub name: String,
    pub size: String,
    pub used_by: String,
}
```

**Domain logic that moves here:** Extraction from `KernelBootSection`,
empty-string-to-None conversion, sysctl override extraction
(`key`/`runtime`/`default`/`source`), non-default kernel module
extraction (`name`/`size`/`used_by`), alternatives extraction
(`name`/`path`/`status`), config snippet extraction for modules-load.d,
modprobe.d, dracut.conf.d, and custom tuned profiles.

#### Typed section: network (`RefNetwork`)

```rust
/// Domain reference data for the network section.
pub struct RefNetwork {
    pub connections: Vec<RefNMConnection>,
    pub firewall_zones: Vec<RefFirewallZone>,
    pub firewall_direct_rules: Vec<RefFirewallDirectRule>,
    pub static_routes: Vec<RefStaticRoute>,
    pub ip_routes: Vec<String>,
    pub ip_rules: Vec<String>,
    pub resolv_provenance: String,
    pub hosts_additions: Vec<String>,
    pub proxy_env: Vec<RefProxyEnv>,
}

pub struct RefNMConnection {
    pub name: String,
    pub conn_type: String,
    pub method: String,
    pub path: String,
}

pub struct RefFirewallZone {
    pub name: String,
    pub path: String,
    pub content: String,
    pub services: Vec<String>,
    pub ports: Vec<String>,
    pub rich_rules: Vec<String>,
}

pub struct RefFirewallDirectRule {
    pub ipv: String,
    pub table: String,
    pub chain: String,
    pub priority: String,
    pub args: String,
}

pub struct RefStaticRoute {
    pub path: String,
    pub name: String,
}

pub struct RefProxyEnv {
    pub source: String,
    pub line: String,
}
```

**Domain logic that moves here:** Extraction from `NetworkSection`
subtypes: NM connections, firewall zones (with services/ports/rich\_rules),
firewall direct rules (`ipv`/`table`/`chain`/`priority`/`args`), static
route files (`path`/`name`), ip routes, ip rules, DNS resolver provenance
(`resolv_provenance`), hosts file additions (`hosts_additions`), and
proxy environment entries (`source`/`line`).

#### Typed section: storage (`RefStorage`)

```rust
/// Domain reference data for the storage section.
pub struct RefStorage {
    pub fstab_entries: Vec<RefFstabEntry>,
    pub mount_points: Vec<RefMountPoint>,
    pub lvm_volumes: Vec<RefLvmVolume>,
    pub var_directories: Vec<RefVarDirectory>,
    pub credential_refs: Vec<RefCredentialRef>,
}

pub struct RefCredentialRef {
    pub credential_path: String,
    pub mount_point: String,
    pub source: String,
}

pub struct RefFstabEntry {
    pub device: String,
    pub mount_point: String,
    pub fstype: String,
    pub options: String,
}

pub struct RefMountPoint {
    pub target: String,
    pub source: String,
    pub fstype: String,
    pub options: String,
}

pub struct RefLvmVolume {
    pub vg_name: String,
    pub lv_name: String,
    pub lv_size: String,
}

pub struct RefVarDirectory {
    pub path: String,
    pub size_estimate: String,
    pub recommendation: String,
}
```

**Domain logic that moves here:** Extraction from `StorageSection`
subtypes: fstab entries, mount points, LVM volumes, var directories
(with `size_estimate` display string and `recommendation`), and
credential refs (`credential_path`/`mount_point`/`source`).

#### Generic section type

For sections that are flat lists of uniform items (scheduled\_tasks,
non\_rpm\_software, selinux):

```rust
/// A generic reference item for sections that don't need typed subtypes.
/// Fields use domain-oriented names; the per-section web adapter maps
/// these to title/subtitle/detail/searchable_text for the wire format.
pub struct GenericRefItem {
    pub id: String,
    /// Primary identifier (cron expression, timer name, package name, SELinux label, etc.)
    pub key: String,
    /// Supporting context (command, version, state description, etc.)
    pub summary: Option<String>,
    /// Full detail body (script content, file content, etc.)
    pub content: Option<String>,
    /// Machine-readable terms for the web adapter's searchable_text assembly.
    pub tags: Vec<String>,
}
```

**Domain meaning of fields per generic section:**

| Section | `key` | `summary` | `content` |
|---|---|---|---|
| `scheduled_tasks` | Cron expression or timer name | Command or exec\_start | Script body (if present) |
| `non_rpm_software` | Package name or env-file path | Version/method/language | Package list or file content |
| `selinux` | Label, boolean name, module name, or file path | State or type description | File content (for CarryForwardFile items) |

**Promotion criterion:** A generic section should be promoted to a typed
section when a consumer needs to render its items differently based on
subtype, or when the section gains subsections. The current
`GenericRefItem` fields are sufficient for web wire parity; promotion
to typed structs may be needed when TUI or CLI detail views require
richer per-subtype rendering. File a follow-up issue when the need
arises.

**Current generic sections (3):**

| Section | Item subtypes | Why generic is fine |
|---|---|---|
| `scheduled_tasks` | CronJob, SystemdTimer, AtJob, GeneratedTimerUnit | Flat list, uniform rendering |
| `non_rpm_software` | NonRpmItem (pip/npm), ConfigFileEntry (env\_files) | Flat list, uniform rendering |
| `selinux` | mode, fips, port labels, boolean overrides, custom modules, fcontext rules, audit rules (CarryForwardFile), PAM configs (CarryForwardFile) | Flat list, uniform rendering |

### `SectionKind` alignment

The existing `SectionKind` enum in `types.rs` already has the right
variants:

```rust
// inspectah-refine/src/types.rs  (EXISTING -- no changes needed)
pub enum SectionKind {
    Package,
    Config,
    Repo,
    User,
    Service,
    Quadlet,
    Flatpak,
    Sysctl,
    Tuned,
    ComposeContext,
}
```

Drop-ins are counted under `Service` (no `DropIn` variant). Users are
`User` (not `UserGroup`). Stats counting extends to cover the new
decision sections using the existing `SectionStats` pattern.

### New module: `inspectah-refine/src/projection.rs`

```rust
// inspectah-refine/src/projection.rs

mod types;  // DecisionProjection, ReferenceProjection, Ref* types, GenericRefItem, EmptyReason

pub use types::*;

/// Build decision projection from session state.
/// PRECONDITION: session.view() is already materialized (cached_view is Some).
pub fn project_decisions(session: &RefineSession) -> DecisionProjection { ... }

/// Build reference projection from original snapshot.
/// Called once per session, cached via OnceLock.
pub fn project_reference(snap: &InspectionSnapshot) -> ReferenceProjection { ... }

// Per-section domain extractors (private)
fn project_ref_services(snap: &InspectionSnapshot) -> RefServices { ... }
fn project_ref_version_changes(snap: &InspectionSnapshot) -> RefVersionChanges { ... }
fn project_ref_containers(snap: &InspectionSnapshot) -> RefContainers { ... }
fn project_ref_kernel_boot(snap: &InspectionSnapshot) -> RefKernelBoot { ... }
fn project_ref_network(snap: &InspectionSnapshot) -> RefNetwork { ... }
fn project_ref_storage(snap: &InspectionSnapshot) -> RefStorage { ... }
fn project_ref_scheduled_tasks(snap: &InspectionSnapshot) -> Vec<GenericRefItem> { ... }  // key=timer/cron name, summary=command, content=script body
fn project_ref_non_rpm(snap: &InspectionSnapshot) -> Vec<GenericRefItem> { ... }  // key=package name, summary=version/method, content=file content
fn project_ref_selinux(snap: &InspectionSnapshot) -> Vec<GenericRefItem> { ... }  // key=label/name, summary=state, content=file body
```

### `RefineSession` integration

```rust
// inspectah-refine/src/session.rs  (changes to existing struct)

pub struct RefineSession {
    // ... existing fields ...
    cached_view: Option<RefinedView>,               // existing -- packages + configs
    cached_decisions: Option<DecisionProjection>,    // NEW -- decision sections
    cached_reference: OnceLock<ReferenceProjection>, // NEW -- immutable reference
    // ...
}

impl RefineSession {
    /// Returns the decision projection, recomputing if stale.
    pub fn decisions(&self) -> &DecisionProjection {
        self.cached_decisions
            .as_ref()
            .expect("decisions always computed after new() or mutation")
    }

    /// Returns the reference projection. Computed once, cached for session lifetime.
    pub fn reference(&self) -> &ReferenceProjection {
        self.cached_reference.get_or_init(|| {
            crate::projection::project_reference(&self.original)
        })
    }

    // Existing mutation methods (apply, undo, redo) already set
    // cached_view = None. They now ALSO set cached_decisions = None.
    // No other changes to mutation logic.
}
```

**Compute order in `recompute_view()`:** The existing `recompute_view()`
(L1656 in `session.rs`) computes `cached_view` by calling
`classify_packages` and `classify_configs`. After this change, it also
computes `cached_decisions` by calling `project_decisions(self)`. The
order is:

1. `project_snapshot()` -- apply ops to get projected snapshot
2. `classify_packages()` + `classify_configs()` -- build `RefinedView`
3. `self.cached_view = Some(view)` -- materialize view
4. `self.cached_decisions = Some(project_decisions(self))` -- build decisions

Step 4 must come after step 3 because `project_decisions()` calls the
same `classify_*` functions that operate on `snapshot_projected()`. Both
`cached_view` and `cached_decisions` are invalidated together on every
mutation.

The `cached_reference` field is `OnceLock` and lazily initialized on
first access -- no explicit rebuild needed.

**`resume_from()` cache rebuild.** The existing `resume_from()` (L413)
restores ops and cursor, sets `cached_view = None`, and calls
`self.recompute_view()`. After this change, `recompute_view()` also
computes `cached_decisions`. The `cached_reference` is `OnceLock` and
initializes on first access. No additional changes to `resume_from()`.

### normalize\_\* decomposition table

This table documents what moves to `inspectah-refine` vs what stays
in the web adapter for each `normalize_*` function:

| Function | Domain logic (moves to refine) | Presentation (stays in web) |
|---|---|---|
| `normalize_version_changes` | Partition into downgrades/upgrades, ordering, empty-reason derivation (no\_baseline vs zero\_drift vs data\_unavailable) | `format_evr_pair()`, directional arrow prefix, subtitle string formatting |
| `normalize_services` | `render_service_intent()` call, divergent/preset-matched/preset-unknown categorization, omission filtering, drop-in folding by unit | `typed_service_subtitle()`, searchable\_text assembly, subtitle string construction |
| `normalize_containers` | Quadlet (name/image/path/content/ports/volumes), compose (path/services/include), running container (id/name/image/status/env/mounts/restart\_policy), flatpak (app\_id/origin/branch/remote/remote\_url) extraction | subtitle, searchable\_text |
| `normalize_network` | NM connections (name/type/method/path), firewall zones (name/content/services/ports/rich\_rules), direct rules (ipv/table/chain/priority/args), static routes (path/name), ip routes/rules, resolv\_provenance, hosts\_additions, proxy env (source/line) | subtitle, searchable\_text |
| `normalize_storage` | fstab entries (device/mount\_point/fstype/options), mount points (target/source/fstype/options), LVM (vg/lv/size), var dirs (path/size\_estimate/recommendation), credential refs (credential\_path/mount\_point/source) | subtitle, searchable\_text |
| `normalize_scheduled_tasks` | Cron jobs (path/source), systemd timers (name/on\_calendar/description/exec\_start/source\_path), at jobs (file/user/command/working\_dir), generated timer units (name/cron\_expr/source\_path/command) | subtitle, searchable\_text |
| `normalize_non_rpm_software` | NonRpmItem (name/path/method/confidence/lang/version/packages), env\_files (path/kind/content) | subtitle, searchable\_text |
| `normalize_kernel_boot` | cmdline, grub\_defaults, tuned\_active, locale, timezone, sysctl overrides (key/runtime/default/source), kernel modules (name/size/used\_by), modules-load.d/modprobe.d/dracut.conf.d/tuned profiles (path/content), alternatives (name/path/status) | subtitle, searchable\_text |
| `normalize_selinux` | SELinux mode, FIPS mode, port labels (protocol/port/label\_type), boolean overrides (name/value\|state), custom modules, fcontext rules, audit rules (CarryForwardFile: path/content), PAM configs (CarryForwardFile: path/content) | subtitle, searchable\_text |

### Per-section web adapters

**Design decision (from consult round):** Write per-section adapter
functions, not one generic function. Each adapter takes the typed domain
reference output from `inspectah-refine` and produces the current
`ContextItem`/`ReferenceSection` wire shape.

This is the SAME code as the current `normalize_*` functions, relocated.
The domain extraction is stripped out (it's in `project_ref_*` now);
what remains is the presentation formatting that builds `ContextItem`
fields.

```rust
// inspectah-web/src/adapter.rs  (NEW)

use inspectah_refine::projection::*;

// --- Reference section adapters ---

pub fn web_services_section(data: &RefServices) -> ReferenceSection { ... }
pub fn web_version_changes_section(data: &RefVersionChanges) -> ReferenceSection { ... }
pub fn web_containers_section(data: &RefContainers) -> ReferenceSection { ... }
pub fn web_kernel_boot_section(data: &RefKernelBoot) -> ReferenceSection { ... }
pub fn web_network_section(data: &RefNetwork) -> ReferenceSection { ... }
pub fn web_storage_section(data: &RefStorage) -> ReferenceSection { ... }
pub fn web_generic_section(id: &str, display_name: &str, items: &[GenericRefItem]) -> ReferenceSection { ... }

/// Build all 9 reference sections in canonical order.
pub fn build_web_sections(ref_proj: &ReferenceProjection) -> Vec<ReferenceSection> {
    vec![
        web_services_section(&ref_proj.services),
        web_version_changes_section(&ref_proj.version_changes),
        web_containers_section(&ref_proj.containers),
        web_network_section(&ref_proj.network),
        web_storage_section(&ref_proj.storage),
        web_generic_section("scheduled_tasks", "Scheduled Tasks", &ref_proj.scheduled_tasks),
        web_generic_section("non_rpm_software", "Non-RPM Software", &ref_proj.non_rpm_software),
        web_kernel_boot_section(&ref_proj.kernel_boot),
        web_generic_section("selinux", "Security & Access Control", &ref_proj.selinux),
    ]
}
```

**Section ordering** (matches live `normalize_for_reference` at L933):

| Index | Section ID | Display Name |
|---|---|---|
| 0 | `services` | Services |
| 1 | `version_changes` | Version Changes |
| 2 | `containers` | Containers |
| 3 | `network` | Network |
| 4 | `storage` | Storage |
| 5 | `scheduled_tasks` | Scheduled Tasks |
| 6 | `non_rpm_software` | Non-RPM Software |
| 7 | `kernel_boot` | Kernel & Boot |
| 8 | `selinux` | Security & Access Control |

#### Detailed adapter example: `web_services_section`

This is the most complex adapter because services has 5 item categories
and 3 subsection types. Shown in full to demonstrate the pattern:

```rust
pub fn web_services_section(data: &RefServices) -> ReferenceSection {
    let mut items = Vec::new();
    let mut subsections = Vec::new();

    // 1. Divergent items (from state_changes, omitted units excluded)
    for svc in &data.divergent {
        let subtitle = typed_service_subtitle(svc.current_state, svc.default_state);
        let dropin_detail = if svc.dropin_contents.is_empty() {
            None
        } else {
            Some(svc.dropin_contents.join("\n---\n"))
        };
        let state_str = svc.current_state.to_string();
        // implied_action() is on ServiceStateChange, not ServiceUnitState.
        // The adapter derives action directly from current_state.
        let action_str = match svc.current_state {
            ServiceUnitState::Enabled => "enable",
            ServiceUnitState::Disabled => "disable",
            ServiceUnitState::Masked => "mask",
        };
        let default_str = svc.default_state
            .map(|d| d.to_string())
            .unwrap_or_else(|| "none".to_string());
        let mut search = format!("{} {} {} {}", svc.unit, state_str, default_str, action_str);
        if let Some(pkg) = &svc.owning_package {
            search.push(' ');
            search.push_str(pkg);
        }
        items.push(ContextItem {
            id: svc.unit.clone(),
            title: svc.unit.clone(),
            subtitle: Some(subtitle),
            detail: dropin_detail,
            searchable_text: search,
        });
    }

    // 2. Preset-matched with drop-in
    for svc in &data.preset_matched_with_dropins {
        let state = match svc.current_state {
            ServiceUnitState::Enabled => "enabled",
            _ => "disabled",
        };
        items.push(ContextItem {
            id: svc.unit.clone(),
            title: svc.unit.clone(),
            subtitle: Some(format!("{} (matches preset, has drop-in override)", state)),
            detail: Some(svc.dropin_contents.join("\n---\n")),
            searchable_text: format!("{} {} drop-in override", svc.unit, state),
        });
    }

    // 3. Preset-unknown enabled
    // Live code uses "enabled (no preset rule)" subtitle and
    // "{unit} enabled no preset rule" searchable_text.
    // Note: live code does NOT append owning_package to searchable_text
    // for preset-unknown services (unlike divergent services).
    for svc in &data.preset_unknown_enabled {
        let dropin_detail = if svc.dropin_contents.is_empty() {
            None
        } else {
            Some(svc.dropin_contents.join("\n---\n"))
        };
        items.push(ContextItem {
            id: svc.unit.clone(),
            title: svc.unit.clone(),
            subtitle: Some("enabled (no preset rule)".into()),
            detail: dropin_detail,
            searchable_text: format!("{} enabled no preset rule", svc.unit),
        });
    }

    // 4. Preset-unknown disabled
    // Live code uses "disabled (no preset rule)" subtitle and
    // "{unit} disabled no preset rule" searchable_text.
    // Note: live code does NOT append owning_package here either.
    for svc in &data.preset_unknown_disabled {
        items.push(ContextItem {
            id: svc.unit.clone(),
            title: svc.unit.clone(),
            subtitle: Some("disabled (no preset rule)".into()),
            detail: None,
            searchable_text: format!("{} disabled no preset rule", svc.unit),
        });
    }

    // 5. Standalone drop-ins
    for d in &data.standalone_dropins {
        items.push(ContextItem {
            id: format!("dropin-{}", d.unit),
            title: format!("{} (drop-in)", d.unit),
            subtitle: Some("drop-in override".into()),
            detail: Some(d.content.clone()),
            searchable_text: format!("{} drop-in", d.unit),
        });
    }

    // Subsections: omitted services
    if !data.omitted.is_empty() {
        let omission_items: Vec<ContextItem> = data.omitted.iter().map(|o| {
            ContextItem {
                id: format!("omitted-{}", o.unit),
                title: o.unit.clone(),
                subtitle: Some(format!("package '{}' not in target image", o.owning_package)),
                detail: None,
                searchable_text: format!("{} omitted {}", o.unit, o.owning_package),
            }
        }).collect();
        subsections.push(ContextSubsection {
            id: "omitted_services".to_string(),
            display_name: "Omitted Services".to_string(),
            items: omission_items,
        });
    }

    // Subsections: advisories
    // Live code maps AdvisoryReason variants inline (no .label() method):
    //   PackageExcluded => "package excluded"
    //   PackageUnreachable => "package unreachable"
    //   BaselineUnavailable => "baseline unavailable"
    if !data.advisories.is_empty() {
        let advisory_items: Vec<ContextItem> = data.advisories.iter().map(|a| {
            let reasons_str: Vec<&str> = a.reasons.iter().map(|r| match r {
                AdvisoryReason::PackageExcluded => "package excluded",
                AdvisoryReason::PackageUnreachable => "package unreachable",
                AdvisoryReason::BaselineUnavailable => "baseline unavailable",
            }).collect();
            ContextItem {
                id: format!("advisory-{}", a.unit),
                title: a.unit.clone(),
                subtitle: Some(format!("package '{}': {}", a.owning_package, reasons_str.join("; "))),
                detail: None,
                searchable_text: format!("{} advisory {} {}", a.unit, a.owning_package, reasons_str.join(" ")),
            }
        }).collect();
        subsections.push(ContextSubsection {
            id: "service_advisories".to_string(),
            display_name: "Service Advisories".to_string(),
            items: advisory_items,
        });
    }

    // Subsections: warnings
    if !data.warnings.is_empty() {
        let warning_items: Vec<ContextItem> = data.warnings.iter().map(|w| {
            ContextItem {
                id: format!("warning-{}", w.unit),
                title: w.unit.clone(),
                subtitle: Some(w.message.clone()),
                detail: None,
                searchable_text: format!("warning {}", w.message),
            }
        }).collect();
        subsections.push(ContextSubsection {
            id: "service_warnings".to_string(),
            display_name: "Service Warnings".to_string(),
            items: warning_items,
        });
    }

    ReferenceSection {
        id: "services".to_string(),
        display_name: "Services".to_string(),
        items,
        subsections,
        empty_reason: None,
    }
}
```

**Key insight:** This adapter is structurally identical to the current
`normalize_services` (L1056-L1322), but it reads from `RefServices`
instead of `InspectionSnapshot`. All the categorization logic
(divergent/matched/unknown/omitted) has moved to `project_ref_services`
in the refine crate. The adapter just formats.

#### Decision-side web adapter: `build_web_view`

```rust
// inspectah-web/src/adapter.rs

pub fn build_web_view(
    view: &RefinedView,
    decisions: &DecisionProjection,
) -> WebViewResponse {
    WebViewResponse {
        // RefinedView fields (flattened)
        packages: view.packages.clone(),
        config_files: view.config_files.clone(),
        containerfile_preview: view.containerfile_preview.clone(),
        stats: view.stats.clone(),
        generation: view.generation,

        // Decision fields (adapted from Refined* to DTO shape)
        repo_groups: decisions.repo_groups.iter().map(web_repo_group).collect(),
        baseline_summary: decisions.baseline_summary.clone(),
        version_changes: decisions.version_changes.iter().map(web_version_change).collect(),
        service_states: decisions.service_states.iter().map(web_service_decision).collect(),
        service_dropins: decisions.service_dropins.iter().map(web_dropin_decision).collect(),
        quadlets: decisions.quadlets.iter().map(web_quadlet_decision).collect(),
        flatpaks: decisions.flatpaks.iter().map(web_flatpak_decision).collect(),
        sysctls: decisions.sysctls.iter().map(web_sysctl_decision).collect(),
        tuned: decisions.tuned.iter().map(web_tuned_decision).collect(),
        users_groups_decisions: decisions.users_groups.clone(),
        session_is_sensitive: decisions.is_sensitive,
    }
}
```

Each `web_*_decision` function maps a `Refined*` type to the current DTO
wire shape. Example for services:

```rust
fn web_service_decision(s: &RefinedServiceState) -> ServiceDecisionDto {
    ServiceDecisionDto {
        unit: s.entry.unit.clone(),
        triage: s.triage.clone(),
        include: s.entry.include,
        owning_package: s.entry.owning_package.clone(),
    }
}
```

These are the same transformations as the current `build_service_decisions`
(L444), `build_container_decisions` (L474), etc. -- just relocated from
handlers.rs to adapter.rs.

### Full endpoint cutover

All 7 view-returning endpoints are cut over atomically in a single
commit. The endpoints, with actual route names from the router
(L0, `inspectah-web/src/main.rs`):

| Route | Method | Handler | Current source | After cutover |
|---|---|---|---|---|
| `/api/view` | GET | `get_view` | `build_view_response(&session)` | `build_web_view(session.view(), session.decisions())` |
| `/api/op` | POST | `apply_op` | `build_view_response(&session)` | `build_web_view(session.view(), session.decisions())` |
| `/api/undo` | POST | `undo` | `build_view_response(&session)` | `build_web_view(session.view(), session.decisions())` |
| `/api/redo` | POST | `redo` | `build_web_view(session.view(), session.decisions())` | `build_web_view(session.view(), session.decisions())` |
| `/api/tarball` | POST | `export_tarball` | `session.snapshot_projected()` (no view) | unchanged (reads projected directly) |
| `/api/user-strategy` | POST | `user_strategy` | `build_view_response(&session)` | `build_web_view(session.view(), session.decisions())` |
| `/api/user-password` | POST | `user_password` | `build_view_response(&session)` | `build_web_view(session.view(), session.decisions())` |
| `/api/snapshot/sections` | GET | `get_sections` | `normalize_for_reference(session.snapshot())` | `build_web_sections(session.reference())` |

Additional endpoints that do NOT return view data (no changes):

| Route | Method | Handler | Returns |
|---|---|---|---|
| `/api/health` | GET | `health` | Health/completeness JSON |
| `/api/ops` | GET | `get_ops` | `session.ops_history()` |
| `/api/changes` | GET | `get_changes` | `session.pending_changes()` |
| `/api/user-preview` | GET | `user_preview` | User preview JSON |
| `/api/viewed` | GET/POST | `get_viewed`/`mark_viewed` | Viewed tracking |

Fleet endpoints (out of scope, unchanged):

| Route | Method | Handler |
|---|---|---|
| `/api/fleet/view` | GET | `fleet_view` |
| `/api/fleet/diff` | POST | `fleet_diff` |

### Wire format changes

**No wire format changes for decisions.** The web adapter produces
the exact same JSON shape as the current `build_view_response`. The
`WebViewResponse` type has the same fields and serde attributes as
the current `ViewResponse`:

| Wire field | Type | Source |
|---|---|---|
| `packages` (flattened) | `Vec<RefinedPackage>` | `RefinedView` |
| `config_files` (flattened) | `Vec<RefinedConfig>` | `RefinedView` |
| `containerfile_preview` (flattened) | `String` | `RefinedView` |
| `stats` (flattened) | `RefineStats` | `RefinedView` |
| `generation` (flattened) | `u64` | `RefinedView` |
| `repo_groups` | `Vec<RepoGroupInfo>` | adapter maps `RepoGroup` |
| `baseline_summary` | `Option<BaselineSummary>` | passthrough |
| `version_changes` | `Vec<VersionChangeEntry>` | adapter maps `VersionChange` |
| `service_states` | `Vec<ServiceDecisionDto>` | adapter maps `RefinedServiceState` |
| `service_dropins` | `Vec<DropInDecisionDto>` | adapter maps `RefinedDropIn` |
| `quadlets` | `Vec<QuadletDecisionDto>` | adapter maps `RefinedQuadlet` |
| `flatpaks` | `Vec<FlatpakDecisionDto>` | adapter maps `RefinedFlatpak` |
| `sysctls` | `Vec<SysctlDecisionDto>` | adapter maps `RefinedSysctl` |
| `tuned` | `Vec<TunedDecisionDto>` | adapter maps `RefinedTunedSelection` |
| `users_groups_decisions` | `Vec<UserGroupDecision>` | passthrough |
| `session_is_sensitive` | `bool` | passthrough |

**No wire format changes for reference sections.** The adapters produce
the same `ReferenceSection { id, display_name, items, subsections,
empty_reason }` structure with the same `ContextItem { id, title,
subtitle, detail, searchable_text }` fields. GlobalSearch depends on
`searchable_text` format, so wire shape preservation matters.

**Frontend type updates (Kit's scope).** The frontend TypeScript types
mirror the wire format. After cutover, the wire shape is identical, so
frontend types don't change initially. If we later decide to expose
richer types to the frontend (e.g., `direction` as enum instead of
string), Kit handles those type updates in migration steps.

### Export safety

The `/api/tarball` endpoint (L597) has a sensitivity gate:

1. Parse `TarballRequest` body (requires `generation` field) -- `400` on malformed JSON
2. Lock session, check generation match -- `RefineError::StaleGeneration` on mismatch
3. Snapshot projected state and release lock
4. **Sensitivity check:** If `session.is_sensitive()` is true, require
   `x-ack-sensitive: true` header (primary, defined as `ACK_SENSITIVE_HEADER`
   in handlers.rs) or `x-acknowledge-sensitive` (legacy, defined as
   `LEGACY_ACK_SENSITIVE_HEADER`). The handler checks both via
   `.get(ACK_SENSITIVE_HEADER).or_else(|| .get(LEGACY_ACK_SENSITIVE_HEADER))`.
   The browser client (`client.ts`) sends `X-Acknowledge-Sensitive` (the
   legacy name). Both header names must be tested.
   Missing/false header returns `428 PRECONDITION_REQUIRED` with a
   sensitivity summary JSON body.
5. `spawn_blocking` for render + tar work -- `StatusCode::OK` with
   `application/gzip` body, `Content-Disposition: attachment`

**Error codes:**
- `400 Bad Request` -- malformed JSON body
- `409 Conflict` -- stale generation (via `RefineError::StaleGeneration`)
- `428 Precondition Required` -- sensitive data, no acknowledgment header
- `200 OK` -- tarball bytes, `application/gzip`

**Contract test coverage for export:**
- Happy path: generation matches, no sensitivity, returns 200 + gzip
- Stale generation: returns error with expected/actual generation
- Sensitive without ack header: returns 428 + summary JSON
- Sensitive with primary header (`x-ack-sensitive`): returns 200 + gzip
- Sensitive with legacy header (`x-acknowledge-sensitive`): returns 200 + gzip

This endpoint does NOT use `build_view_response` -- it reads
`session.snapshot_projected()` directly and passes it to
`render_refine_export()`. No changes needed for the projection
consolidation; documenting for completeness.

---

## Migration strategy

### Step 1: Add `include` to `RefinedTunedSelection`

Add `include: bool` field to `RefinedTunedSelection` in
`inspectah-refine/src/types.rs`. Update `classify_tuned()` to accept
the include value as a parameter (or derive it in the caller).

### Step 2: Add `RepoGroup` type

Add `RepoGroup` struct to `inspectah-refine/src/types.rs`.
Same fields as `RepoGroupInfo`, cleaner name.

### Step 3: Add projection module

Create `inspectah-refine/src/projection.rs` with:
- `DecisionProjection` struct
- `ReferenceProjection` struct
- All `Ref*` domain reference types (6 typed + 1 generic)
- `EmptyReason` enum
- `project_decisions()` function
- `project_reference()` function
- All `project_ref_*` private functions

Code motion from `handlers.rs`:
- `build_*` functions -> `project_decisions()` body
- Domain extraction from `normalize_*` functions -> `project_ref_*` functions

### Step 4: Wire `RefineSession`

Add `cached_decisions: Option<DecisionProjection>` and
`cached_reference: OnceLock<ReferenceProjection>` to `RefineSession`.
Add `decisions()` and `reference()` accessors.

Extend `recompute_view()` to also compute `cached_decisions` (after
`cached_view`). Both `cached_view` and `cached_decisions` cleared on
mutation.

### Step 5: Add web adapter

Create `inspectah-web/src/adapter.rs` with:
- `WebViewResponse` (same shape as current `ViewResponse`)
- `Web*Decision` DTO types (same as current `*DecisionDto`)
- Per-section reference adapters
- `build_web_view()` function
- `build_web_sections()` function

### Step 6: Contract snapshot gate

**Before cutover**, capture golden JSON output from the current
`build_view_response()` and `normalize_for_reference()` for a
representative snapshot. Run the new adapter against the same snapshot.
Assert byte-identical JSON output (modulo key ordering).

This is the pre-cutover gate. If the adapter produces different output,
the migration has a bug.

Test structure:

```rust
#[test]
fn contract_view_response_matches() {
    let session = RefineSession::new(rich_snapshot());
    let old = serde_json::to_value(build_view_response(&session)).unwrap();
    let new = serde_json::to_value(build_web_view(
        session.view(),
        session.decisions(),
    )).unwrap();
    assert_eq!(old, new, "web adapter must produce identical ViewResponse JSON");
}

#[test]
fn contract_sections_match() {
    let session = RefineSession::new(rich_snapshot());
    let old = serde_json::to_value(normalize_for_reference(session.snapshot())).unwrap();
    let new = serde_json::to_value(build_web_sections(session.reference())).unwrap();
    assert_eq!(old, new, "web adapter must produce identical sections JSON");
}
```

### Step 7: Atomic endpoint cutover

In a single commit:

1. Replace `build_view_response()` calls in all 6 view endpoints with
   `build_web_view(session.view(), session.decisions())`
2. Replace `normalize_for_reference()` in `get_sections` with
   `build_web_sections(session.reference())`
3. Update `AppState::sections_cache` type to
   `OnceLock<Vec<ReferenceSection>>` (if not already)
4. Remove old `build_*`, `normalize_*` functions, `*Dto` types,
   `ViewResponse`, `VersionChangeEntry`, `ReferenceSection`
   (the wire type moves to adapter.rs), `ContextSubsection`,
   `ContextItem` from `handlers.rs`

### Step 8: Frontend type updates (Kit)

Kit updates frontend TypeScript types to match any wire shape changes
(there should be none initially). Kit also verifies GlobalSearch
continues to work with the preserved `searchable_text` format.

---

## Testing strategy

### Unit tests (inspectah-refine)

- `project_decisions()` returns correct counts for each section
- `project_reference()` returns 6 typed sections + 3 generic sections
- `RefVersionChanges` correctly partitions downgrades/upgrades
- `RefServices` correctly categorizes divergent/matched/unknown/omitted
- `RefinedTunedSelection.include` derives from `kernel_boot.tuned_include`
- `EmptyReason` variants match the three-state logic
- Snapshot with missing sections produces empty typed/generic data

### Contract tests (inspectah-web)

- `contract_view_response_matches` -- adapter output == old output
- `contract_sections_match` -- adapter sections == old sections
- Run against `rich_snapshot()` (the existing test fixture)
- Run against `empty_snapshot()` (edge case)

### Mutation-sequence contract tests (inspectah-web)

The initial contract tests (above) verify that the adapter produces
correct output from a static snapshot. These tests verify that the
adapter produces correct output AFTER mutations -- the round-trip
through apply/undo/redo must preserve the wire shape.

```rust
#[test]
fn contract_mutation_round_trip() {
    // rich_snapshot() seeds httpd.service with include: false in state_changes.
    // We flip to include: true here so the op exercises a real state change —
    // setting include: false would be a no-op and stale cached_decisions
    // could slip through undetected.
    let snap = rich_snapshot();
    let mut session = RefineSession::new(snap);

    // Apply an op, verify the response contains the full updated view.
    // RefinementOp::SetInclude uses ItemId (not SectionKind + id string).
    // session.apply() takes &mut self.
    // Uses httpd.service because apply() validates against state_changes —
    // sshd.service is only in enabled_units (preset-unknown), not state_changes.
    let op = RefinementOp::SetInclude {
        item_id: ItemId::Service {
            unit: "httpd.service".into(),
        },
        include: true,  // flips from false (fixture default) to true
    };
    session.apply(op.clone()).unwrap();

    let old = serde_json::to_value(build_view_response(&session)).unwrap();
    let new = serde_json::to_value(build_web_view(
        session.view(),
        session.decisions(),
    )).unwrap();
    assert_eq!(old, new, "post-mutation adapter must match old path");

    // The decision projection must reflect the flip: httpd.service include == true.
    let httpd_decision = new["service_states"].as_array().unwrap()
        .iter().find(|s| s["unit"] == "httpd.service").unwrap();
    assert_eq!(httpd_decision["include"], true,
        "decision projection must show include: true after apply (cache invalidated)");

    // Undo: restores include to false (the fixture default)
    session.undo().unwrap();
    let old_undo = serde_json::to_value(build_view_response(&session)).unwrap();
    let new_undo = serde_json::to_value(build_web_view(
        session.view(),
        session.decisions(),
    )).unwrap();
    assert_eq!(old_undo, new_undo, "post-undo adapter must match old path");

    let httpd_undo = new_undo["service_states"].as_array().unwrap()
        .iter().find(|s| s["unit"] == "httpd.service").unwrap();
    assert_eq!(httpd_undo["include"], false,
        "undo must restore include: false (fixture default)");

    // Redo: flips back to include: true
    session.redo().unwrap();
    let old_redo = serde_json::to_value(build_view_response(&session)).unwrap();
    let new_redo = serde_json::to_value(build_web_view(
        session.view(),
        session.decisions(),
    )).unwrap();
    assert_eq!(old_redo, new_redo, "post-redo adapter must match old path");

    let httpd_redo = new_redo["service_states"].as_array().unwrap()
        .iter().find(|s| s["unit"] == "httpd.service").unwrap();
    assert_eq!(httpd_redo["include"], true,
        "redo must flip back to include: true");
}
```

**Why this matters:** The initial-state contract test could pass while
mutation handling diverges -- e.g., if `cached_decisions` fails to
invalidate on undo, the post-undo view would carry stale data that the
old path wouldn't. Testing the full apply/undo/redo cycle catches these
invalidation bugs.

### Transport-level export tests (inspectah-web)

The `/api/tarball` endpoint has a multi-step error contract (400/409/428/200)
that is currently tested implicitly. Add explicit HTTP-level tests for each
status code path:

```rust
#[tokio::test]
async fn tarball_malformed_body_returns_400() {
    let app = test_app_with_session();
    let resp = app.post("/api/tarball")
        .body("{not json")
        .header("content-type", "application/json")
        .send().await;
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn tarball_stale_generation_returns_409() {
    let app = test_app_with_session();
    let resp = app.post("/api/tarball")
        .json(&json!({"generation": 99999}))
        .send().await;
    assert_eq!(resp.status(), 409);
}

#[tokio::test]
async fn tarball_sensitive_no_ack_returns_428() {
    let app = test_app_with_sensitive_session();
    let gen = current_generation(&app).await;
    let resp = app.post("/api/tarball")
        .json(&json!({"generation": gen}))
        .send().await;
    assert_eq!(resp.status(), 428);
    // Verify the response body contains the sensitivity summary
    let body: serde_json::Value = resp.json().await;
    assert!(body.get("sensitivity_summary").is_some());
}

#[tokio::test]
async fn tarball_sensitive_with_ack_returns_200() {
    let app = test_app_with_sensitive_session();
    let gen = current_generation(&app).await;
    let resp = app.post("/api/tarball")
        .json(&json!({"generation": gen}))
        .header("x-ack-sensitive", "true")
        .send().await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["content-type"], "application/gzip");
}

#[tokio::test]
async fn tarball_sensitive_with_legacy_header_returns_200() {
    // The browser client (client.ts) sends "X-Acknowledge-Sensitive".
    // The handler accepts both "x-ack-sensitive" (primary) and
    // "x-acknowledge-sensitive" (legacy). Both must be tested.
    let app = test_app_with_sensitive_session();
    let gen = current_generation(&app).await;
    let resp = app.post("/api/tarball")
        .json(&json!({"generation": gen}))
        .header("x-acknowledge-sensitive", "true")
        .send().await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["content-type"], "application/gzip");
}

#[tokio::test]
async fn tarball_happy_path_returns_gzip() {
    let app = test_app_with_session();  // non-sensitive
    let gen = current_generation(&app).await;
    let resp = app.post("/api/tarball")
        .json(&json!({"generation": gen}))
        .send().await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["content-type"], "application/gzip");
    assert!(resp.headers().contains_key("content-disposition"));
}
```

**CORS note:** The CORS layer in `lib.rs` explicitly allows both `x-ack-sensitive` and `x-acknowledge-sensitive` header names; a follow-up test should verify that preflight (`OPTIONS`) requests accept both headers in `Access-Control-Allow-Headers`.

**Why this matters:** The export endpoint is the only write-action in
the web layer. Its error contract (stale generation, sensitivity gate)
must survive the projection consolidation unchanged. These tests verify
the HTTP-level behavior that integration tests would otherwise miss.

### Test fixture updates

After cutover, existing test fixtures (`rich_snapshot()`,
`empty_snapshot()`, and any section-specific fixtures in `api_test.rs`)
must be updated to reflect any wire shape changes introduced by the
adapter layer. Since this spec targets zero wire changes, fixture
updates should be minimal -- but the contract tests themselves serve as
the verification gate. If a fixture needs updating, the contract test
will fail and pinpoint the divergence.

### Existing test preservation

The existing tests in `api_test.rs` continue to work:

- `normalize_for_reference_section_count_and_ids` -- verifies 9 sections
  in the correct order (services, version\_changes, containers, network,
  storage, scheduled\_tasks, non\_rpm\_software, kernel\_boot, selinux)
- `normalize_for_reference_item_counts` -- verifies non-empty sections
- Section-specific tests (normalize\_non\_rpm\_empty\_section, etc.)

These tests temporarily test both old and new paths during migration.
After step 7, they test the new path exclusively.

---

## Team split

| Scope | Owner | What |
|---|---|---|
| Rust projection layer | Tang | Steps 1-4: types, projection module, session integration |
| Web adapter + cutover | Tang | Steps 5-7: adapter module, contract tests, endpoint cutover |
| Frontend type updates | Kit | Step 8: TypeScript types, GlobalSearch verification |

Both start after spec approval. Tang's work is sequential (steps 1-7).
Kit's work starts after step 7 lands (or in parallel if we're confident
the wire shape is preserved).

---

## Finding traceability

### Round 3 findings

| # | Finding | Severity | Resolution |
|---|---|---|---|
| B1 | Tuned `include` hardcoded to true | Blocker | Added `include: bool` to `RefinedTunedSelection`. Derived from `snapshot_projected().kernel_boot.tuned_include`. See [Tuned include-state fix](#tuned-include-state-fix). |
| B2 | `get_sections` wire contract not tight enough | Blocker | Per-section web adapters reproduce exact wire shape. See [Per-section web adapters](#per-section-web-adapters). Contract snapshot gate validates. |
| B3 | `RefItem::Container` incomplete, `Generic` too loose | Blocker | 6 fully typed sections with concrete Rust types. 3 generic sections with promotion criterion. See [ReferenceProjection type](#referenceprojection-type-and-domain-reference-types). |
| I4 | `project_decisions()` compute order | Important | Documented: `cached_view` materialized before `cached_decisions`. See [Compute order in recompute\_view()](#refinesession-integration). |
| I5 | Reference section ordering drifts | Important | Matched live ordering from `normalize_for_reference()` L933-L944. Documented in [Section ordering table](#per-section-web-adapters). |
| I6 | Export status code mismatch | Important | Documented actual codes from code: 400/409/428/200. See [Export safety](#export-safety). |
| I7 | Route names and `resume_from` cache | Important | Used actual route names from router. Documented `resume_from()` cache rebuild. See [Full endpoint cutover](#full-endpoint-cutover) and [resume\_from() cache rebuild](#refinesession-integration). |

### Round 4 findings

| # | Finding | Severity | Resolution |
|---|---|---|---|
| R4-B1 | Typed reference structs missing fields used by normalize\_\* | Blocker | Exhaustive field inventory conducted against live code. Added: `RefRunningContainerItem` (env, mounts, restart\_policy), `RefFlatpakRefItem` (remote\_url), `RefFirewallDirectRule` (table, args -- replaced incorrect `command`), `RefStaticRoute` (renamed to path, name -- was interface, content), `RefNetwork` (resolv\_provenance, hosts\_additions), `RefVarDirectory` (size\_estimate replaces size\_bytes, added recommendation), `RefStorage` (credential\_refs + `RefCredentialRef`), `RefKernelBoot` (sysctl\_overrides + `RefSysctlOverride`, alternatives + `RefAlternativeEntry`), `RefKernelModule` (size, used\_by -- replaced incorrect `path`), `RefServiceItem` (owning\_package). |
| R4-I1 | SELinux display name wrong in spec | Important | Fixed: section ordering table and `build_web_sections` now use `"Security & Access Control"` to match live `reference_section("selinux", "Security & Access Control", items)`. |
| R4-I2 | Services adapter searchable\_text drifts from live output | Important | Fixed: divergent service adapter example now uses `format!("{} {} {} {}", unit, state\_str, default\_str, action\_str)` with optional owning\_package, matching live `normalize_services` code. Preset-unknown categories also fixed. |
| R4-I3 | Mutation/export test coverage too thin | Important | Added mutation-sequence contract test (apply/undo/redo round-trip), transport-level `/api/tarball` tests for 400/409/428/200 paths, and test fixture update note. See [Mutation-sequence contract tests](#mutation-sequence-contract-tests-inspectah-web) and [Transport-level export tests](#transport-level-export-tests-inspectah-web). |
| R4-N1 | Generic section item subtypes incomplete | Note | Updated generic section table: scheduled\_tasks now lists GeneratedTimerUnit, non\_rpm\_software lists ConfigFileEntry (env\_files), selinux lists all 8 subtypes including CarryForwardFile variants. |
| R4-N2 | Decomposition table lacked field-level detail | Note | Expanded all 9 rows of the decomposition table to list every field each `normalize_*` function reads. |

### Round 6 findings

| # | Finding | Severity | Resolution |
|---|---|---|---|
| R6-B1 | Mutation test uses obsolete API shape (`section`/`id` fields, immutable session) | Blocker | Fixed: `RefinementOp::SetInclude` now uses `item_id: ItemId::Service { unit }` matching actual types.rs. Session declared `let mut`. |
| R6-I1 | `GenericRefItem` uses web-shaped field names (`title`/`subtitle`/`detail`/`search_terms`) | Important | Renamed to domain-oriented names: `key`/`summary`/`content`/`tags`. Added per-section table documenting domain meaning of each field. |
| R6-I2 | Services adapter uses wrong string literals (3 mismatches) | Important | Fixed: omission subtitle uses `"not in target image"` (not `"not installed"`); preset-unknown uses `"enabled (no preset rule)"` / `"disabled (no preset rule)"` with matching searchable\_text; advisory reasons use inline match (not `r.label()`). All strings verified against live handlers.rs. |
| R6-I3 | Export tests miss legacy header (`X-Acknowledge-Sensitive`) | Important | Added `tarball_sensitive_with_legacy_header_returns_200` test case. Updated export safety section to document both header constants and browser client usage. |
| R6-I4 | Fleet-reuse wording implies `DecisionProjection` aggregation works | Important | Narrowed: fleet can reuse shared classifiers but needs its own projection type. `DecisionProjection` is single-host/view-oriented; fleet needs by-variant/by-zone grouping. |

### Round 7 findings

| # | Finding | Severity | Resolution |
|---|---|---|---|
| R7-B1 | Mutation test uses `sshd.service` but `apply()` validates against `state_changes` only; `sshd.service` is in `enabled_units` (preset-unknown), not `state_changes` | Blocker | Changed to `httpd.service` which is the service in `state_changes` in the test fixture. Test is now executable against the current API. |
| R7-I1 | Adapter calls `svc.current_state.implied_action(svc.default_state)` but `implied_action()` is a method on `ServiceStateChange` (takes `&self`, no args), not on `ServiceUnitState` | Important | Replaced with direct match on `svc.current_state` to derive action string (`Enabled => "enable"`, etc.), matching the live code pattern. |
| R7-I2 | Preset-unknown adapter appends `owning_package` to `searchable_text` but live `normalize_services()` does NOT do this for preset-unknown services | Important | Removed `owning_package` append from both preset-unknown enabled (section 3) and preset-unknown disabled (section 4) searchable\_text construction. Only divergent services (section 1) append owning\_package, matching live code. |
| R7-I3 | Spec claims TUI "will consume" the projection, but TUI spec is being updated separately | Important | Softened to "future consumers (web UI, TUI, CLI)" language. Added note that `GenericRefItem` sections are sufficient for web wire parity but may need promotion to typed when TUI/CLI detail views require richer rendering. |
