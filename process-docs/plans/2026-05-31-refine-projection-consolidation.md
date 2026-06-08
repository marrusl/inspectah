# Refine Projection Consolidation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move single-host view projection logic from `inspectah-web/src/handlers.rs` to `inspectah-refine`, creating shared `DecisionProjection` + `ReferenceProjection` types that any consumer (web, TUI, CLI) can use.

**Architecture:** Two projections with different lifecycles — `DecisionProjection` regenerates on mutation (cached alongside `RefinedView`), `ReferenceProjection` is immutable from the original snapshot (`OnceLock`). The existing `Refined*` types from `types.rs` ARE the decision types — no parallel type family. Per-section web adapters in `inspectah-web/src/adapter.rs` map domain data to current wire shapes.

**Tech Stack:** Rust (2024 edition), serde, insta (snapshot testing), inspectah-core types, inspectah-refine classify functions.

**Spec:** `docs/specs/proposed/2026-05-30-refine-projection-consolidation.md` — read this for full type definitions, decomposition tables, and traceability.

**Team split:** Tang (Tasks 1-13, Rust), Kit (Task 14, frontend TypeScript).

---

## File Map

### New files
| File | Responsibility |
|---|---|
| `inspectah-refine/src/projection/mod.rs` | Module root, re-exports |
| `inspectah-refine/src/projection/types.rs` | `DecisionProjection`, `ReferenceProjection`, all `Ref*` structs, `GenericRefItem`, `EmptyReason` |
| `inspectah-refine/src/projection/decisions.rs` | `project_decisions()` — builds `DecisionProjection` from session state |
| `inspectah-refine/src/projection/reference.rs` | `project_reference()` + per-section `project_ref_*()` extractors |
| `inspectah-web/src/adapter.rs` | Per-section web adapters, `build_web_view()`, `build_web_sections()` |
| `inspectah-web/src/web_types.rs` | Relocated DTO types (`ViewResponse`, `*DecisionDto`, `VersionChangeEntry`, `RepoGroupInfo`, `ReferenceSection`, `ContextItem`, `ContextSubsection`) |
| `inspectah-web/tests/contract_snapshots.rs` | Contract snapshot tests (pre-cutover gate) |

### Modified files
| File | Changes |
|---|---|
| `inspectah-refine/src/types.rs` | Add `include: bool` to `RefinedTunedSelection` |
| `inspectah-refine/src/classify.rs` | Pass `tuned_include` into `classify_tuned()` |
| `inspectah-refine/src/session.rs` | Add `cached_decisions`, `cached_reference`; modify `recompute_view()`, mutation methods, `resume_from()` |
| `inspectah-refine/src/lib.rs` | Add `pub mod projection;` |
| `inspectah-web/src/lib.rs` | Add `pub mod adapter;`, `pub mod web_types;` |
| `inspectah-web/src/handlers.rs` | Replace `build_view_response()` and `get_sections()` with adapter calls; remove dead code |

---

## Actual API Reference (grounded in live code)

This section documents the real function signatures and types the plan depends on.
Every task MUST use these — not approximations.

### classify.rs signatures (inspectah-refine/src/classify.rs)
```rust
pub fn classify_packages(snap: &InspectionSnapshot) -> Vec<RefinedPackage>
pub fn classify_configs(snap: &InspectionSnapshot) -> Vec<RefinedConfig>
pub fn classify_services(snap: &InspectionSnapshot) -> (Vec<RefinedServiceState>, Vec<RefinedDropIn>)
pub fn classify_containers(snap: &InspectionSnapshot) -> (Vec<RefinedQuadlet>, Vec<RefinedFlatpak>)
pub fn classify_sysctls(snap: &InspectionSnapshot) -> Vec<RefinedSysctl>
pub fn classify_tuned(snap: &InspectionSnapshot) -> Vec<RefinedTunedSelection>
```

There are NO functions named `classify_dropins`, `classify_quadlets`, or `classify_flatpaks`.
Services and containers each return tuples that the caller destructures.

### handlers.rs decision builders (inspectah-web/src/handlers.rs)
```rust
fn build_view_response(session: &RefineSession) -> ViewResponse           // private
pub(crate) fn build_repo_groups(session: &RefineSession) -> Vec<RepoGroupInfo>
fn build_service_decisions(session: &RefineSession) -> (Vec<ServiceDecisionDto>, Vec<DropInDecisionDto>)
fn build_container_decisions(session: &RefineSession) -> (Vec<QuadletDecisionDto>, Vec<FlatpakDecisionDto>)
fn build_sysctl_decisions(session: &RefineSession) -> Vec<SysctlDecisionDto>
fn build_tuned_decisions(session: &RefineSession) -> Vec<TunedDecisionDto>
fn build_sensitivity_summary(snap: &InspectionSnapshot) -> serde_json::Value
```

### handlers.rs reference builders (inspectah-web/src/handlers.rs)
```rust
pub fn normalize_for_reference(snap: &InspectionSnapshot) -> Vec<ReferenceSection>  // PUBLIC
fn normalize_services(snap: &InspectionSnapshot) -> ReferenceSection
fn normalize_version_changes(snap: &InspectionSnapshot) -> ReferenceSection
fn normalize_containers(snap: &InspectionSnapshot) -> ReferenceSection
fn normalize_network(snap: &InspectionSnapshot) -> ReferenceSection
fn normalize_storage(snap: &InspectionSnapshot) -> ReferenceSection
fn normalize_scheduled_tasks(snap: &InspectionSnapshot) -> ReferenceSection
fn normalize_non_rpm_software(snap: &InspectionSnapshot) -> ReferenceSection
fn normalize_kernel_boot(snap: &InspectionSnapshot) -> ReferenceSection
fn normalize_selinux(snap: &InspectionSnapshot) -> ReferenceSection
```

### Canonical section order (normalize_for_reference L933-944)
```rust
vec![
    normalize_services(snap),           // 1
    normalize_version_changes(snap),    // 2
    normalize_containers(snap),         // 3
    normalize_network(snap),            // 4
    normalize_storage(snap),            // 5
    normalize_scheduled_tasks(snap),    // 6
    normalize_non_rpm_software(snap),   // 7
    normalize_kernel_boot(snap),        // 8
    normalize_selinux(snap),            // 9
]
```

All 9 sections are returned unconditionally. Empty sections carry `empty_reason`.
The live test `sections_returns_nine_sections` asserts `sections.len() == 9`.

### RefineSession public API (inspectah-refine/src/session.rs)
```rust
pub fn new(snapshot: InspectionSnapshot) -> Self
pub fn new_with_tarball(snapshot: InspectionSnapshot, tarball: PathBuf) -> Self
pub fn resume_from(tarball: &Path) -> Result<Option<Self>, RefineError>
pub fn view(&self) -> &RefinedView
pub fn snapshot(&self) -> &InspectionSnapshot          // original, immutable
pub fn snapshot_projected(&self) -> InspectionSnapshot  // clone with ops applied
pub fn baseline_summary(&self) -> Option<BaselineSummary>
pub fn is_sensitive(&self) -> bool
pub fn viewed_ids(&self) -> &HashSet<String>
```

Private: `fn recompute_view(&mut self)` at L1656 — computes `cached_view`.

### Existing types (DO NOT redefine)
```rust
// inspectah_core::types::users
pub struct UserGroupDecision {
    pub name: String, pub uid: u64, pub gid: u64,
    pub shell: String, pub home: String, pub include: bool,
    pub classification: String,
    pub containerfile_strategy: UserContainerfileStrategy,
    pub password_choice: UserPasswordChoice,
    pub password_hash: Option<String>, pub has_sudo: Option<bool>,
    pub has_subuid: Option<bool>, pub ssh_key_count: Option<u64>,
    pub ssh_keys: Option<Vec<String>>,
    pub classification_rationale: Option<String>,
    pub supplementary_groups: Option<Vec<String>>,
    pub password_status: Option<String>,
}

// inspectah_refine::baseline_summary
pub struct BaselineSummary {
    pub image_ref: String, pub image_digest: String,
    pub strategy: String,
    pub baseline_count: usize, pub user_added_count: usize,
    pub review_count: usize,
}

// inspectah_refine::types
pub enum RepoProvenance { Verified, Incomplete, Unknown }  // serde: snake_case
pub enum RepoTier { Distro, OfficialOptional, ThirdParty, None }  // serde: snake_case

// inspectah-web/src/handlers.rs (current location — will move to web_types.rs)
pub struct RepoGroupInfo {
    pub section_id: String, pub provenance: RepoProvenance,
    pub is_distro: bool, pub tier: RepoTier,
    pub package_count: usize, pub enabled: bool,
}

pub struct ViewResponse {
    #[serde(flatten)] pub view: RefinedView,
    pub repo_groups: Vec<RepoGroupInfo>,
    pub baseline_summary: Option<BaselineSummary>,
    pub version_changes: Vec<VersionChangeEntry>,
    pub service_states: Vec<ServiceDecisionDto>,
    pub service_dropins: Vec<DropInDecisionDto>,
    pub quadlets: Vec<QuadletDecisionDto>,
    pub flatpaks: Vec<FlatpakDecisionDto>,
    pub sysctls: Vec<SysctlDecisionDto>,
    pub tuned: Vec<TunedDecisionDto>,
    pub users_groups_decisions: Vec<UserGroupDecision>,
    pub session_is_sensitive: bool,
}

pub struct ReferenceSection {
    pub id: String, pub display_name: String,
    pub items: Vec<ContextItem>,
    pub subsections: Vec<ContextSubsection>,
    pub empty_reason: Option<String>,
}

pub struct ContextItem {
    pub id: String, pub title: String,
    pub subtitle: Option<String>, pub detail: Option<String>,
    pub searchable_text: String,
}

pub struct ContextSubsection {
    pub id: String, pub display_name: String,
    pub items: Vec<ContextItem>,
}

pub struct AppState {
    pub session: Arc<Mutex<RefineSession>>,
    pub sections_cache: OnceLock<Vec<ReferenceSection>>,
}
```

### Test harness (inspectah-web/tests/api_test.rs)
```rust
fn test_state() -> Arc<AppState>       // minimal snapshot (1 pkg, 1 config)
fn rich_snapshot() -> InspectionSnapshot // full snapshot with all section types
fn rich_state() -> Arc<AppState>       // rich_snapshot wrapped in AppState
fn app(state: Arc<AppState>) -> axum::Router  // inspectah_web::router(state, "http://localhost:8642")
async fn get_json(app: &axum::Router, path: &str) -> (StatusCode, serde_json::Value)
async fn post_json(app: &axum::Router, path: &str, body: serde_json::Value) -> (StatusCode, serde_json::Value)
```

All test helpers are private to the test module. `rich_snapshot()` is NOT accessible
from other test files. Contract tests must either duplicate the fixture or use the
HTTP harness approach (construct `AppState`, build router, make requests).

---

## Task 1: Add `include` to `RefinedTunedSelection`

**Files:**
- Modify: `inspectah-refine/src/types.rs` (add field)
- Modify: `inspectah-refine/src/classify.rs` (pass include value)

- [ ] **Step 1: Add `include: bool` field to `RefinedTunedSelection`**

In `inspectah-refine/src/types.rs` L401, the struct currently is:

```rust
pub struct RefinedTunedSelection {
    pub active_profile: String,
    pub custom_profiles: Vec<String>,
    pub triage: TriageTag,
}
```

Add the field:

```rust
pub struct RefinedTunedSelection {
    pub active_profile: String,
    pub custom_profiles: Vec<String>,
    pub triage: TriageTag,
    pub include: bool,  // NEW — derived from kernel_boot.tuned_include
}
```

- [ ] **Step 2: Update `classify_tuned()` to set `include`**

In `inspectah-refine/src/classify.rs`, `classify_tuned()` (L491) takes `&InspectionSnapshot`.
Find where `RefinedTunedSelection { ... }` is constructed and add the include field.

The include value comes from `snap.kernel_boot.as_ref().map_or(true, |kb| kb.tuned_include)`.
If `kernel_boot` is `None`, default to `true` (tuned is included unless explicitly excluded).

```rust
pub fn classify_tuned(snap: &InspectionSnapshot) -> Vec<RefinedTunedSelection> {
    let tuned_include = snap
        .kernel_boot
        .as_ref()
        .map_or(true, |kb| kb.tuned_include);

    // ... existing classification logic ...
    // At every RefinedTunedSelection { ... } construction site, add:
    //   include: tuned_include,
}
```

Search for all construction sites:

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && grep -rn 'RefinedTunedSelection' inspectah-refine/src/
```

Add `include: true` (or `include: tuned_include` in classify_tuned) to every site.

- [ ] **Step 3: Run tests**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-refine
```

Expected: all existing tests pass after adding `include: true` to any test construction sites.

- [ ] **Step 4: Run clippy**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo clippy -p inspectah-refine -- -D warnings
```

Expected: zero warnings.

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-refine/src/types.rs inspectah-refine/src/classify.rs
git commit -m "feat(refine): add include field to RefinedTunedSelection

Derived from kernel_boot.tuned_include in classify_tuned().
Prerequisite for projection consolidation — the decision projection
needs tuned include state without reaching back into the session."
```

---

## Task 2: Add projection types module

**Files:**
- Create: `inspectah-refine/src/projection/mod.rs`
- Create: `inspectah-refine/src/projection/types.rs`
- Modify: `inspectah-refine/src/lib.rs`

This task defines ALL projection types. No logic — just structs, enums, derives.
Types REUSE existing definitions from `inspectah-core` and `inspectah-refine` — no redefinition.

- [ ] **Step 1: Create module directory**

```bash
mkdir -p /Users/mrussell/Work/bootc-migration/inspectah/inspectah-refine/src/projection
```

- [ ] **Step 2: Write `projection/types.rs`**

Create `inspectah-refine/src/projection/types.rs`. Critical rules:

1. `UserGroupDecision` is imported from `inspectah_core::types::users::UserGroupDecision` — NOT redefined
2. `BaselineSummary` is imported from `crate::baseline_summary::BaselineSummary` — NOT redefined
3. `RepoGroupInfo` uses `RepoProvenance` and `RepoTier` enums — NOT strings
4. `VersionChange` is imported from `inspectah_core::types::rpm::VersionChange` — NOT redefined

```rust
use serde::{Deserialize, Serialize};

use crate::baseline_summary::BaselineSummary;
use crate::types::{
    RepoProvenance, RepoTier,
    RefinedDropIn, RefinedFlatpak, RefinedQuadlet, RefinedServiceState,
    RefinedSysctl, RefinedTunedSelection,
};
use inspectah_core::types::rpm::VersionChange;
use inspectah_core::types::users::UserGroupDecision;

// ── Decision projection ──────────────────────────────────────────

/// All classified decision data for single-host view rendering.
/// Recomputed on every mutation alongside `RefinedView`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionProjection {
    pub service_states: Vec<RefinedServiceState>,
    pub service_dropins: Vec<RefinedDropIn>,
    pub quadlets: Vec<RefinedQuadlet>,
    pub flatpaks: Vec<RefinedFlatpak>,
    pub sysctls: Vec<RefinedSysctl>,
    pub tuned: Vec<RefinedTunedSelection>,
    pub repo_groups: Vec<RepoGroup>,
    pub version_changes: Vec<VersionChange>,  // core type, not DTO — web adapter maps to VersionChangeEntry
    pub users_groups: Vec<UserGroupDecision>,
    pub is_sensitive: bool,
    pub baseline_summary: Option<BaselineSummary>,
}

/// Repo group with provenance and tier for the view.
/// Same fields as the current `RepoGroupInfo` in handlers.rs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoGroup {
    pub section_id: String,
    pub provenance: RepoProvenance,
    pub is_distro: bool,
    pub tier: RepoTier,
    pub package_count: usize,
    pub enabled: bool,
}

// NOTE: VersionChangeEntry (with direction as String) lives only in
// inspectah-web/src/web_types.rs — the web adapter maps VersionChange
// (core type with VersionChangeDirection enum) to VersionChangeEntry
// when building the wire response. Do NOT define VersionChangeEntry here.

// ── Reference projection ─────────────────────────────────────────

/// Immutable reference data derived from the original snapshot.
/// Computed once per session via OnceLock.
#[derive(Debug, Clone)]
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

// ── Services ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct RefServices {
    pub divergent: Vec<RefServiceItem>,
    pub preset_matched_with_dropins: Vec<RefServiceItem>,
    pub preset_unknown_enabled: Vec<RefServiceItem>,
    pub preset_unknown_disabled: Vec<RefServiceItem>,
    pub standalone_dropins: Vec<RefDropInItem>,
    pub omitted: Vec<RefOmittedService>,
    pub advisories: Vec<RefServiceAdvisory>,
    pub warnings: Vec<RefServiceWarning>,
}

#[derive(Debug, Clone)]
pub struct RefServiceItem {
    pub unit: String,
    pub current_state: inspectah_core::types::services::ServiceUnitState,
    pub default_state: Option<inspectah_core::types::services::PresetDefault>,
    pub owning_package: Option<String>,
    pub dropin_contents: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RefDropInItem {
    pub unit: String,
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct RefOmittedService {
    pub unit: String,
    pub package: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct RefServiceAdvisory {
    pub unit: String,
    pub owning_package: String,
    pub reasons: Vec<inspectah_pipeline::render::service_intent::AdvisoryReason>,
}

#[derive(Debug, Clone)]
pub struct RefServiceWarning {
    pub unit: String,
    pub message: String,
}

// ── Version changes ──────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct RefVersionChanges {
    pub downgrades: Vec<RefVersionChangeItem>,
    pub upgrades: Vec<RefVersionChangeItem>,
    pub empty_reason: Option<EmptyReason>,
}

#[derive(Debug, Clone)]
pub struct RefVersionChangeItem {
    pub name: String,
    pub arch: String,
    pub host_version: String,
    pub base_version: String,
    pub host_epoch: String,
    pub base_epoch: String,
    pub direction: inspectah_core::types::rpm::VersionChangeDirection,
}

#[derive(Debug, Clone)]
pub enum EmptyReason {
    NoBaseline,
    ZeroDrift,
    DataUnavailable,
}

// ── Containers ───────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct RefContainers {
    pub quadlets: Vec<RefQuadletItem>,
    pub compose_files: Vec<RefComposeItem>,
    pub running_containers: Vec<RefRunningContainerItem>,
    pub flatpaks: Vec<RefFlatpakRefItem>,
}

#[derive(Debug, Clone)]
pub struct RefQuadletItem {
    pub name: String,
    pub image: String,
    pub path: String,
    pub content: String,
    pub ports: Vec<String>,
    pub volumes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RefComposeItem {
    pub path: String,
    pub services: Vec<inspectah_core::types::containers::ComposeService>,
    pub include: bool,
}

#[derive(Debug, Clone)]
pub struct RefRunningContainerItem {
    pub id: String,
    pub name: String,
    pub image: String,
    pub status: String,
    pub env: Vec<String>,
    pub mounts: Vec<ContainerMount>,
    pub restart_policy: String,
}

#[derive(Debug, Clone)]
pub struct ContainerMount {
    pub mount_type: String,
    pub source: String,
    pub destination: String,
}

#[derive(Debug, Clone)]
pub struct RefFlatpakRefItem {
    pub app_id: String,
    pub origin: String,
    pub branch: String,
    pub remote: String,
    pub remote_url: String,
}

// ── Kernel/boot ──────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
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

#[derive(Debug, Clone)]
pub struct RefSysctlOverride {
    pub key: String,
    pub runtime: String,
    pub default: String,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct RefKernelModule {
    pub name: String,
    pub size: String,
    pub used_by: String,
}

#[derive(Debug, Clone)]
pub struct RefConfigSnippet {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct RefAlternativeEntry {
    pub name: String,
    pub path: String,
    pub status: String,
}

// ── Network ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
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

#[derive(Debug, Clone)]
pub struct RefNMConnection {
    pub name: String,
    pub conn_type: String,
    pub method: String,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct RefFirewallZone {
    pub name: String,
    pub path: String,
    pub content: String,
    pub services: Vec<String>,
    pub ports: Vec<String>,
    pub rich_rules: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RefFirewallDirectRule {
    pub ipv: String,
    pub table: String,
    pub chain: String,
    pub priority: String,
    pub args: String,
}

#[derive(Debug, Clone)]
pub struct RefStaticRoute {
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct RefProxyEnv {
    pub source: String,
    pub line: String,
}

// ── Storage ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct RefStorage {
    pub fstab_entries: Vec<RefFstabEntry>,
    pub mount_points: Vec<RefMountPoint>,
    pub lvm_volumes: Vec<RefLvmVolume>,
    pub var_directories: Vec<RefVarDirectory>,
    pub credential_refs: Vec<RefCredentialRef>,
}

#[derive(Debug, Clone)]
pub struct RefFstabEntry {
    pub device: String,
    pub mount_point: String,
    pub fstype: String,
    pub options: String,
}

#[derive(Debug, Clone)]
pub struct RefMountPoint {
    pub target: String,
    pub source: String,
    pub fstype: String,
    pub options: String,
}

#[derive(Debug, Clone)]
pub struct RefLvmVolume {
    pub vg_name: String,
    pub lv_name: String,
    pub lv_size: String,
}

#[derive(Debug, Clone)]
pub struct RefVarDirectory {
    pub path: String,
    pub size_estimate: String,
    pub recommendation: String,
}

#[derive(Debug, Clone)]
pub struct RefCredentialRef {
    pub credential_path: String,
    pub mount_point: String,
    pub source: String,
}

// ── Generic ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GenericRefItem {
    pub id: String,
    pub key: String,
    pub summary: Option<String>,
    pub content: Option<String>,
    pub tags: Vec<String>,
}
```

- [ ] **Step 3: Write `projection/mod.rs`**

Create `inspectah-refine/src/projection/mod.rs`:

```rust
mod types;

pub use types::*;
```

- [ ] **Step 4: Add module to `lib.rs`**

In `inspectah-refine/src/lib.rs`, add after the existing `pub mod types;` line:

```rust
pub mod projection;
```

- [ ] **Step 5: Verify compilation**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo check -p inspectah-refine
```

Expected: compiles. If you get `dead_code` warnings for the new types, add `#[allow(dead_code)]` at the top of `projection/types.rs` temporarily — types will be consumed starting in Task 3.

- [ ] **Step 6: Run clippy and commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo clippy -p inspectah-refine -- -D warnings
git add inspectah-refine/src/projection/ inspectah-refine/src/lib.rs
git commit -m "feat(refine): add projection types module

DecisionProjection, ReferenceProjection, and all Ref* domain types.
Types only — no logic yet. Reuses existing UserGroupDecision,
BaselineSummary, RepoProvenance, and RepoTier from their home crates."
```

---

## Task 3: Implement `project_decisions()`

**Files:**
- Create: `inspectah-refine/src/projection/decisions.rs`
- Modify: `inspectah-refine/src/projection/mod.rs`

This task builds `DecisionProjection` from session state using the ACTUAL classify
function signatures documented in the API Reference above.

- [ ] **Step 1: Write the failing test**

Add to `inspectah-refine/src/projection/decisions.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::RefineSession;
    use inspectah_core::snapshot::InspectionSnapshot;

    fn test_snapshot() -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        use inspectah_core::types::services::*;
        snap.services = Some(ServiceSection {
            state_changes: vec![ServiceStateChange {
                unit: "httpd.service".into(),
                current_state: ServiceUnitState::Enabled,
                default_state: Some(PresetDefault::Disable),
                include: true,
                owning_package: Some("httpd".into()),
                fleet: None,
                attention_reason: None,
            }],
            enabled_units: vec![],
            disabled_units: vec![],
            drop_ins: vec![],
            preset_matched_units: vec![],
        });
        snap
    }

    #[test]
    fn project_decisions_includes_classified_services() {
        let session = RefineSession::new(test_snapshot());
        let decisions = project_decisions(&session);
        assert_eq!(decisions.service_states.len(), 1);
        assert_eq!(decisions.service_states[0].entry.unit, "httpd.service");
    }

    #[test]
    fn project_decisions_empty_snapshot() {
        let session = RefineSession::new(InspectionSnapshot::new());
        let decisions = project_decisions(&session);
        assert!(decisions.service_states.is_empty());
        assert!(decisions.quadlets.is_empty());
        assert!(decisions.flatpaks.is_empty());
        assert!(!decisions.is_sensitive);
    }
}
```

- [ ] **Step 2: Implement `project_decisions()`**

In `inspectah-refine/src/projection/decisions.rs`:

```rust
use crate::classify::{classify_services, classify_containers, classify_sysctls, classify_tuned};
use crate::projection::types::*;
use crate::session::RefineSession;
use inspectah_core::types::rpm::VersionChange;
use inspectah_core::types::users::UserGroupDecision;

/// Build decision projection from session state.
/// PRECONDITION: session.view() has been materialized (cached_view is Some).
pub fn project_decisions(session: &RefineSession) -> DecisionProjection {
    let snap = session.snapshot_projected();

    // Services — classify_services returns (Vec<RefinedServiceState>, Vec<RefinedDropIn>)
    let (service_states, service_dropins) = classify_services(&snap);

    // Containers — classify_containers returns (Vec<RefinedQuadlet>, Vec<RefinedFlatpak>)
    let (quadlets, flatpaks) = classify_containers(&snap);

    // Sysctls and tuned
    let sysctls = classify_sysctls(&snap);
    let tuned = classify_tuned(&snap);

    // Version changes — pass through core VersionChange directly.
    // The web adapter maps VersionChangeDirection enum to "upgrade"/"downgrade"
    // strings when building VersionChangeEntry for the wire.
    let version_changes: Vec<VersionChange> = snap
        .rpm
        .as_ref()
        .map(|rpm| rpm.version_changes.clone())
        .unwrap_or_default();

    // Users/groups — deserialized from serde_json::Value in projected snapshot
    let users_groups: Vec<UserGroupDecision> = snap
        .users_groups
        .as_ref()
        .map(|ug| {
            ug.users
                .iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect()
        })
        .unwrap_or_default();

    // Repo groups — delegate to session's existing build_repo_groups logic.
    // NOTE: build_repo_groups is currently pub(crate) in handlers.rs.
    // For now, replicate the logic here. It reads session.view().packages,
    // the repo_index, and pending changes. Read handlers.rs L386-440 for
    // the exact algorithm and port it.
    let repo_groups = build_repo_groups_from_session(session);

    // Sensitivity and baseline summary — use session's public API
    let is_sensitive = session.is_sensitive();
    let baseline_summary = session.baseline_summary();

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
        is_sensitive,
        baseline_summary,
    }
}

/// Port of handlers.rs build_repo_groups (L386-440).
/// Read that function in full and translate it. It uses:
/// - session.view().packages (the classified package list)
/// - the repo_index (available via session — may need a new accessor)
/// - pending_changes (available via session — may need a new accessor)
///
/// The output shape maps directly to RepoGroup which has the same fields
/// as RepoGroupInfo with enum-typed provenance and tier.
fn build_repo_groups_from_session(session: &RefineSession) -> Vec<RepoGroup> {
    // Read handlers.rs build_repo_groups() in full. It iterates over
    // session.view().packages to group by source_repo, then looks up
    // each repo in the RepoIndex to get provenance/tier/distro status.
    //
    // If session does not expose repo_index publicly, add a pub accessor:
    //   pub fn repo_index(&self) -> &RepoIndex { &self.repo_index }
    // This is the cleanest approach — it's a read-only accessor on
    // immutable data.
    //
    // Implementation must match the existing function exactly.
    Vec::new() // placeholder — implementer ports from handlers.rs
}
```

- [ ] **Step 3: Update `projection/mod.rs`**

```rust
mod decisions;
mod types;

pub use decisions::project_decisions;
pub use types::*;
```

- [ ] **Step 4: Run tests, clippy, commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-refine project_decisions -- --nocapture
cargo clippy -p inspectah-refine -- -D warnings
git add inspectah-refine/src/projection/
git commit -m "feat(refine): implement project_decisions()

Builds DecisionProjection from session state using existing classify_*
functions. Destructures tuple returns from classify_services() and
classify_containers(). Reuses Refined* types directly — no DTO layer."
```

---

## Task 4: Implement reference extractors — services + version changes

**Files:**
- Create: `inspectah-refine/src/projection/reference.rs`
- Modify: `inspectah-refine/src/projection/mod.rs`

These are the two most complex reference sections. Each extractor reads from
`InspectionSnapshot` and returns typed domain data.

- [ ] **Step 1: Write failing tests**

```rust
// inspectah-refine/src/projection/reference.rs

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::snapshot::InspectionSnapshot;

    #[test]
    fn project_ref_services_empty_snapshot() {
        let snap = InspectionSnapshot::new();
        let result = project_ref_services(&snap);
        assert!(result.divergent.is_empty());
        assert!(result.preset_matched_with_dropins.is_empty());
    }

    #[test]
    fn project_ref_services_divergent_service() {
        let mut snap = InspectionSnapshot::new();
        use inspectah_core::types::services::*;
        snap.services = Some(ServiceSection {
            state_changes: vec![ServiceStateChange {
                unit: "httpd.service".into(),
                current_state: ServiceUnitState::Enabled,
                default_state: Some(PresetDefault::Disable),
                include: true,
                owning_package: Some("httpd".into()),
                fleet: None,
                attention_reason: None,
            }],
            enabled_units: vec![],
            disabled_units: vec![],
            drop_ins: vec![],
            preset_matched_units: vec![],
        });
        let result = project_ref_services(&snap);
        assert_eq!(result.divergent.len(), 1);
        assert_eq!(result.divergent[0].unit, "httpd.service");
    }

    #[test]
    fn project_ref_version_changes_partitions_by_direction() {
        let mut snap = InspectionSnapshot::new();
        use inspectah_core::types::rpm::*;
        snap.rpm = Some(RpmSection {
            version_changes: vec![
                VersionChange {
                    name: "openssl".into(),
                    arch: "x86_64".into(),
                    host_version: "3.0.1".into(),
                    base_version: "3.1.0".into(),
                    host_epoch: String::new(),
                    base_epoch: String::new(),
                    direction: VersionChangeDirection::Downgrade,
                },
            ],
            ..Default::default()
        });
        let result = project_ref_version_changes(&snap);
        assert_eq!(result.downgrades.len(), 1);
        assert_eq!(result.upgrades.len(), 0);
        assert!(result.empty_reason.is_none());
    }

    #[test]
    fn project_ref_version_changes_data_unavailable() {
        let snap = InspectionSnapshot::new(); // no rpm section
        let result = project_ref_version_changes(&snap);
        assert!(matches!(result.empty_reason, Some(EmptyReason::DataUnavailable)));
    }
}
```

- [ ] **Step 2: Implement `project_ref_services()`**

Read `handlers.rs` `normalize_services` (L1056-L1322) in full. That function
categorizes services into subsections (divergent, preset-matched-with-dropins,
preset-unknown-enabled/disabled) and builds ContextItems. The reference extractor
does the same categorization but returns typed `RefServiceItem` structs instead
of ContextItems. The web adapter (Task 11) handles ContextItem formatting.

```rust
use crate::projection::types::*;
use inspectah_core::snapshot::InspectionSnapshot;

pub fn project_ref_services(snap: &InspectionSnapshot) -> RefServices {
    let services = match &snap.services {
        Some(s) => s,
        None => return RefServices::default(),
    };

    // Port the categorization logic from normalize_services (handlers.rs L1056-1322).
    // Use the SAME branching rules — the adapter layer must produce identical output.
    //
    // The categories map to normalize_services subsections:
    //   divergent → "Service State Divergence" subsection
    //   preset_matched_with_dropins → "Preset-Matched Services with Drop-In Overrides"
    //   preset_unknown_enabled → "Services With No Preset Rule (Enabled)"
    //   preset_unknown_disabled → "Services With No Preset Rule (Disabled)"
    //   standalone_dropins → "Standalone Drop-Ins" subsection
    //   omitted → not shown (but tracked)
    //   advisories → top-level items with advisory info
    //   warnings → from snap warnings related to services

    // Implementation: iterate state_changes, branch on default_state vs current_state,
    // collect drop-ins for each unit from services.drop_ins.

    RefServices::default() // placeholder — implementer ports from normalize_services
}
```

- [ ] **Step 3: Implement `project_ref_version_changes()`**

This is a cleaner extraction — version changes have a 3-way empty reason and
partition into downgrades/upgrades.

```rust
pub fn project_ref_version_changes(snap: &InspectionSnapshot) -> RefVersionChanges {
    let rpm = match &snap.rpm {
        Some(r) => r,
        None => {
            return RefVersionChanges {
                empty_reason: Some(EmptyReason::DataUnavailable),
                ..Default::default()
            };
        }
    };

    if rpm.version_changes.is_empty() {
        let reason = if snap.baseline.is_some() {
            EmptyReason::ZeroDrift
        } else {
            EmptyReason::NoBaseline
        };
        return RefVersionChanges {
            empty_reason: Some(reason),
            ..Default::default()
        };
    }

    let mut downgrades = Vec::new();
    let mut upgrades = Vec::new();

    for vc in &rpm.version_changes {
        let item = RefVersionChangeItem {
            name: vc.name.clone(),
            arch: vc.arch.clone(),
            host_version: vc.host_version.clone(),
            base_version: vc.base_version.clone(),
            host_epoch: vc.host_epoch.clone(),
            base_epoch: vc.base_epoch.clone(),
            direction: vc.direction.clone(),
        };
        match vc.direction {
            inspectah_core::types::rpm::VersionChangeDirection::Downgrade => downgrades.push(item),
            inspectah_core::types::rpm::VersionChangeDirection::Upgrade => upgrades.push(item),
        }
    }

    RefVersionChanges {
        downgrades,
        upgrades,
        empty_reason: None,
    }
}
```

- [ ] **Step 4: Update `projection/mod.rs`**

```rust
mod decisions;
mod reference;
mod types;

pub use decisions::project_decisions;
pub use reference::*;
pub use types::*;
```

- [ ] **Step 5: Run tests, clippy, commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-refine project_ref -- --nocapture
cargo clippy -p inspectah-refine -- -D warnings
git add inspectah-refine/src/projection/
git commit -m "feat(refine): implement service + version change reference extractors

project_ref_services() categorizes into divergent/matched/unknown/omitted.
project_ref_version_changes() partitions by direction with 3-way empty reason."
```

---

## Task 5: Implement reference extractors — containers + kernel/boot

**Files:**
- Modify: `inspectah-refine/src/projection/reference.rs`

Same pattern as Task 4. Read the corresponding `normalize_*` functions in
`handlers.rs` to understand the extraction logic.

- [ ] **Step 1: Write failing tests for `project_ref_containers()`**

Test that quadlets, compose files, running containers, and flatpaks are
extracted from `ContainerSection`. Use the same `InspectionSnapshot::new()` +
populate pattern.

- [ ] **Step 2: Implement `project_ref_containers()`**

Read `handlers.rs` `normalize_containers` (L1324-L1433). Extract quadlets
(name/image/path/content/ports/volumes from `ContainerSection.quadlet_units`),
compose files (from `ContainerSection.compose_files`), running containers
(from `ContainerSection.running_containers`), flatpaks (from
`ContainerSection.flatpak_apps`).

- [ ] **Step 3: Write failing tests for `project_ref_kernel_boot()`**

Test cmdline, sysctl_overrides, non_default_modules, alternatives extraction.

- [ ] **Step 4: Implement `project_ref_kernel_boot()`**

Read `handlers.rs` `normalize_kernel_boot` (L1787-L1950). Extract cmdline,
grub_defaults, tuned_active, locale, timezone, sysctl_overrides, kernel modules,
config snippets (modules-load.d, modprobe.d, dracut.conf.d, custom tuned),
alternatives. NOTE: cmdline truncation (80 chars) is a PRESENTATION concern —
do it in the web adapter, NOT here. Store the full cmdline.

- [ ] **Step 5: Run tests, clippy, commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-refine project_ref -- --nocapture
cargo clippy -p inspectah-refine -- -D warnings
git add inspectah-refine/src/projection/reference.rs
git commit -m "feat(refine): implement container + kernel/boot reference extractors"
```

---

## Task 6: Implement reference extractors — network + storage

**Files:**
- Modify: `inspectah-refine/src/projection/reference.rs`

- [ ] **Step 1: Write failing tests for `project_ref_network()`**

Test NM connections, firewall zones, direct rules, static routes, resolv_provenance,
hosts_additions, proxy_env.

- [ ] **Step 2: Implement `project_ref_network()`**

Read `handlers.rs` `normalize_network` (L1435-L1566). Extract all subtypes from
`NetworkSection`.

- [ ] **Step 3: Write failing tests for `project_ref_storage()`**

Test fstab entries, mount points, LVM volumes, var directories, credential refs.

- [ ] **Step 4: Implement `project_ref_storage()`**

Read `handlers.rs` `normalize_storage` (L1568-L1633). Extract all subtypes from
`StorageSection`.

- [ ] **Step 5: Run tests, clippy, commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-refine project_ref -- --nocapture
cargo clippy -p inspectah-refine -- -D warnings
git add inspectah-refine/src/projection/reference.rs
git commit -m "feat(refine): implement network + storage reference extractors"
```

---

## Task 7: Implement generic extractors + `project_reference()` orchestrator

**Files:**
- Modify: `inspectah-refine/src/projection/reference.rs`

- [ ] **Step 1: Write failing tests for generic sections**

Test that scheduled_tasks, non_rpm_software, and selinux produce `GenericRefItem`
vectors from their respective snapshot sections.

- [ ] **Step 2: Implement generic section extractors**

```rust
fn project_ref_scheduled_tasks(snap: &InspectionSnapshot) -> Vec<GenericRefItem> {
    // Read normalize_scheduled_tasks (handlers.rs L1635-L1719).
    // Source: snap.scheduled_tasks (ScheduledTaskSection)
    // Maps systemd timers and cron jobs to GenericRefItem.
    Vec::new() // implementer ports from normalize_scheduled_tasks
}

fn project_ref_non_rpm(snap: &InspectionSnapshot) -> Vec<GenericRefItem> {
    // Read normalize_non_rpm_software (handlers.rs L1721-L1785).
    // Source: snap.non_rpm_software (NonRpmSoftwareSection)
    // Maps pip packages, snap packages, etc. to GenericRefItem.
    Vec::new() // implementer ports from normalize_non_rpm_software
}

fn project_ref_selinux(snap: &InspectionSnapshot) -> Vec<GenericRefItem> {
    // Read normalize_selinux (handlers.rs L1952+).
    // Source: snap.selinux (SelinuxSection)
    // Maps SELinux booleans, modules, ports to GenericRefItem.
    Vec::new() // implementer ports from normalize_selinux
}
```

- [ ] **Step 3: Implement `project_reference()` orchestrator**

```rust
pub fn project_reference(snap: &InspectionSnapshot) -> ReferenceProjection {
    ReferenceProjection {
        services: project_ref_services(snap),
        version_changes: project_ref_version_changes(snap),
        containers: project_ref_containers(snap),
        kernel_boot: project_ref_kernel_boot(snap),
        network: project_ref_network(snap),
        storage: project_ref_storage(snap),
        scheduled_tasks: project_ref_scheduled_tasks(snap),
        non_rpm_software: project_ref_non_rpm(snap),
        selinux: project_ref_selinux(snap),
    }
}
```

- [ ] **Step 4: Write orchestrator test**

```rust
#[test]
fn project_reference_returns_all_sections() {
    let snap = InspectionSnapshot::new();
    let result = project_reference(&snap);
    // Structural test — all 9 fields accessible (even if empty)
    let _ = &result.services;
    let _ = &result.version_changes;
    let _ = &result.containers;
    let _ = &result.kernel_boot;
    let _ = &result.network;
    let _ = &result.storage;
    let _ = &result.scheduled_tasks;
    let _ = &result.non_rpm_software;
    let _ = &result.selinux;
}
```

- [ ] **Step 5: Export from mod.rs and run tests, clippy, commit**

Update `projection/mod.rs`:

```rust
mod decisions;
mod reference;
mod types;

pub use decisions::project_decisions;
pub use reference::project_reference;
pub use types::*;
```

Also re-export per-section functions for direct use in testing:

```rust
pub use reference::{
    project_ref_services, project_ref_version_changes,
    project_ref_containers, project_ref_kernel_boot,
    project_ref_network, project_ref_storage,
};
```

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-refine project_ref -- --nocapture
cargo clippy -p inspectah-refine -- -D warnings
git add inspectah-refine/src/projection/
git commit -m "feat(refine): implement generic extractors + project_reference() orchestrator

Completes the reference projection: 6 typed sections + 3 generic.
project_reference() builds the full immutable projection from snapshot."
```

---

## Task 8: Wire `RefineSession` with projection caches

**Files:**
- Modify: `inspectah-refine/src/session.rs`

- [ ] **Step 1: Write failing test**

Add to `session.rs` test module:

```rust
#[test]
fn session_exposes_decisions_and_reference() {
    let mut snap = InspectionSnapshot::new();
    snap.services = Some(ServiceSection {
        state_changes: vec![ServiceStateChange {
            unit: "httpd.service".into(),
            current_state: ServiceUnitState::Enabled,
            default_state: Some(PresetDefault::Disable),
            include: true,
            owning_package: Some("httpd".into()),
            fleet: None,
            attention_reason: None,
        }],
        ..Default::default()
    });
    let session = RefineSession::new(snap);
    let decisions = session.decisions();
    assert_eq!(decisions.service_states.len(), 1);
    let reference = session.reference();
    // services section accessible
    let _ = &reference.services;
}

#[test]
fn decisions_invalidated_on_mutation() {
    use crate::types::{RefinementOp, ItemId};
    let mut snap = InspectionSnapshot::new();
    // Seed httpd.service with include: false in state_changes — SetInclude
    // validates against state_changes, so the service must exist there.
    use inspectah_core::types::services::*;
    snap.services = Some(ServiceSection {
        state_changes: vec![ServiceStateChange {
            unit: "httpd.service".into(),
            current_state: ServiceUnitState::Enabled,
            default_state: Some(PresetDefault::Disable),
            include: false,  // starts excluded
            owning_package: Some("httpd".into()),
            fleet: None,
            attention_reason: None,
        }],
        ..Default::default()
    });

    // Seed RPM section with version_changes AND a package so we can
    // mutate the projected snapshot in a way that changes RPM state.
    // This makes the reference stability assertions non-vacuous:
    // without this data, version_changes checks would be 0==0.
    use inspectah_core::types::rpm::*;
    snap.rpm = Some(RpmSection {
        version_changes: vec![
            VersionChange {
                name: "openssl".into(),
                arch: "x86_64".into(),
                host_version: "3.0.9".into(),
                base_version: "3.1.0".into(),
                host_epoch: "1".into(),
                base_epoch: "1".into(),
                direction: VersionChangeDirection::Downgrade,
            },
            VersionChange {
                name: "curl".into(),
                arch: "x86_64".into(),
                host_version: "8.2.0".into(),
                base_version: "8.1.0".into(),
                host_epoch: "0".into(),
                base_epoch: "0".into(),
                direction: VersionChangeDirection::Upgrade,
            },
        ],
        packages_added: vec![PackageEntry {
            name: "custom-agent".into(),
            arch: "x86_64".into(),
            include: true,
            source_repo: "custom-repo".into(),
            ..Default::default()
        }],
        ..Default::default()
    });

    let mut session = RefineSession::new(snap);

    // BEFORE mutation: decision projection shows include: false for httpd
    let decisions_before = session.decisions();
    assert_eq!(decisions_before.service_states.len(), 1);
    assert!(!decisions_before.service_states[0].entry.include,
        "httpd should start excluded");

    // Reference projection — snapshot fields BEFORE mutation.
    // We cannot hold a &ReferenceProjection borrow across session.apply()
    // because apply() takes &mut self. Clone the data, then compare after.
    //
    // WHY these assertions prove immutability:
    // ReferenceProjection is backed by OnceLock — computed once from
    // self.original, then returned by reference on every subsequent call.
    // If someone accidentally changed reference() to rebuild from
    // snapshot_projected(), these assertions catch it:
    //
    // 1. version_changes: seeded with 1 downgrade + 1 upgrade. The
    //    reference reads from snap.rpm.version_changes. No current
    //    mutation alters version_changes directly, but the non-zero
    //    counts prove the reference actually computed real data (not
    //    vacuous 0==0). If a future mutation type DOES touch
    //    version_changes, this test catches the regression.
    //
    // 2. services.divergent: httpd is Enabled with preset Disable,
    //    so it's divergent. The service include-flip doesn't change
    //    the Enabled/Disable categorization — but the OnceLock
    //    guarantee means the SAME ReferenceProjection instance is
    //    returned, so field equality holds by identity, not accident.
    //
    // 3. Pointer identity check (below): the strongest proof. If
    //    reference() rebuilt on each call, the pointers would differ
    //    even if content happened to match.
    let ref_before = session.reference();
    let ref_divergent_before = ref_before.services.divergent.clone();
    let ref_advisories_before = ref_before.services.advisories.clone();
    let ref_version_downgrades_before = ref_before.version_changes.downgrades.clone();
    let ref_version_upgrades_before = ref_before.version_changes.upgrades.clone();
    // Pointer identity: if OnceLock works, same address before and after.
    let ref_ptr_before = ref_before as *const _;

    // Preconditions: reference has non-trivial data to compare against.
    assert_eq!(ref_version_downgrades_before.len(), 1,
        "precondition: fixture must seed exactly 1 downgrade");
    assert_eq!(ref_version_upgrades_before.len(), 1,
        "precondition: fixture must seed exactly 1 upgrade");
    assert!(!ref_divergent_before.is_empty(),
        "precondition: httpd (Enabled vs preset Disable) must appear in divergent");

    // Mutate: flip httpd.service to include: true (changes projected state)
    session.apply(RefinementOp::SetInclude {
        item_id: ItemId::Service {
            unit: "httpd.service".into(),
        },
        include: true,
    }).unwrap();

    // Also mutate a package — this changes the projected RPM section
    // (packages_added[0].include flips to false). If reference() were
    // accidentally rebuilt from snapshot_projected(), it would receive
    // a different RpmSection than self.original provides.
    session.apply(RefinementOp::SetInclude {
        item_id: ItemId::Package {
            name: "custom-agent".into(),
            arch: "x86_64".into(),
        },
        include: false,
    }).unwrap();

    // AFTER mutation: decision projection shows include: true (cache invalidated and rebuilt)
    let decisions_after = session.decisions();
    assert!(decisions_after.service_states[0].entry.include,
        "httpd should now be included after mutation");

    // Reference projection MUST be identical — same data, same instance.
    let ref_after = session.reference();

    // Pointer identity: OnceLock returns the same &ReferenceProjection.
    // This is the strongest possible proof of immutability — if reference()
    // recomputed after mutation, the pointer would differ.
    assert!(std::ptr::eq(ref_ptr_before, ref_after),
        "reference() must return the same OnceLock instance across mutations — \
         pointer changed, which means it was recomputed");

    // Field-level assertions as defense-in-depth (also catches any future
    // refactor that breaks OnceLock or switches to a clone-based cache).
    assert_eq!(ref_after.services.divergent.len(), ref_divergent_before.len(),
        "reference services.divergent must be stable across mutations");
    assert_eq!(ref_after.services.advisories.len(), ref_advisories_before.len(),
        "reference services.advisories must be stable across mutations");
    assert_eq!(ref_after.version_changes.downgrades.len(), ref_version_downgrades_before.len(),
        "reference version_changes.downgrades count must be stable across mutations");
    assert_eq!(ref_after.version_changes.upgrades.len(), ref_version_upgrades_before.len(),
        "reference version_changes.upgrades count must be stable across mutations");

    // Content-level checks: verify actual values, not just counts.
    for (i, item) in ref_after.services.divergent.iter().enumerate() {
        assert_eq!(item.unit, ref_divergent_before[i].unit,
            "reference divergent service unit names must be stable");
    }
    for (i, item) in ref_after.version_changes.downgrades.iter().enumerate() {
        assert_eq!(item.name, ref_version_downgrades_before[i].name,
            "reference downgrade package names must be stable");
        assert_eq!(item.host_version, ref_version_downgrades_before[i].host_version,
            "reference downgrade versions must be stable");
    }
    for (i, item) in ref_after.version_changes.upgrades.iter().enumerate() {
        assert_eq!(item.name, ref_version_upgrades_before[i].name,
            "reference upgrade package names must be stable");
        assert_eq!(item.host_version, ref_version_upgrades_before[i].host_version,
            "reference upgrade versions must be stable");
    }
}
```

- [ ] **Step 2: Add fields to `RefineSession`**

In `session.rs` L23, the struct currently is:

```rust
pub struct RefineSession {
    original: InspectionSnapshot,
    repo_index: RepoIndex,
    baseline_available: bool,
    refine_mode: RefineMode,
    ops: Vec<RefinementOp>,
    cursor: usize,
    cached_view: Option<RefinedView>,
    generation: u64,
    viewed: HashSet<String>,
    tarball_path: Option<PathBuf>,
    durability_degraded: bool,
}
```

Add two fields:

```rust
pub struct RefineSession {
    original: InspectionSnapshot,
    repo_index: RepoIndex,
    baseline_available: bool,
    refine_mode: RefineMode,
    ops: Vec<RefinementOp>,
    cursor: usize,
    cached_view: Option<RefinedView>,
    cached_decisions: Option<crate::projection::DecisionProjection>,    // NEW
    cached_reference: std::sync::OnceLock<crate::projection::ReferenceProjection>, // NEW
    generation: u64,
    viewed: HashSet<String>,
    tarball_path: Option<PathBuf>,
    durability_degraded: bool,
}
```

- [ ] **Step 3: Initialize in constructors**

In `RefineSession::new()` (L49), add to the struct initialization:

```rust
cached_decisions: None,
cached_reference: std::sync::OnceLock::new(),
```

Do the same in `new_with_tarball()` (L351) and in the reconstruction inside
`resume_from()` (L413).

- [ ] **Step 4: Modify `recompute_view()` to also compute decisions**

In `recompute_view()` (L1656), find the end where `self.cached_view = Some(RefinedView { ... })`.
AFTER that line, add:

```rust
self.cached_decisions = Some(crate::projection::project_decisions(self));
```

This MUST come after `cached_view` is set because `project_decisions()` calls
`session.view()` which reads `cached_view`.

- [ ] **Step 5: Add accessor methods**

```rust
/// Returns the current decision projection.
/// Computed alongside `view()` on construction and after every mutation.
pub fn decisions(&self) -> &crate::projection::DecisionProjection {
    self.cached_decisions
        .as_ref()
        .expect("decisions always computed after new() or mutation")
}

/// Returns the immutable reference projection.
/// Computed once from the original snapshot, cached for session lifetime.
pub fn reference(&self) -> &crate::projection::ReferenceProjection {
    self.cached_reference.get_or_init(|| {
        crate::projection::project_reference(&self.original)
    })
}
```

- [ ] **Step 6: Invalidate `cached_decisions` on mutations**

Search for every place `self.cached_view = None;` appears. There will be
lines in `apply()`, `undo()`, `redo()` and possibly others. Alongside each one,
add:

```rust
self.cached_decisions = None;
```

Search pattern:

```bash
grep -n 'cached_view = None' /Users/mrussell/Work/bootc-migration/inspectah/inspectah-refine/src/session.rs
```

Do NOT invalidate `cached_reference` — it is immutable for the session lifetime
(reads from `self.original`, which never changes).

- [ ] **Step 7: Handle `resume_from()`**

In `resume_from()` (L413), find where `RefineSession` is reconstructed.
Add `cached_decisions: None` and `cached_reference: OnceLock::new()` to that
construction. The subsequent `recompute_view()` call will populate
`cached_decisions`.

- [ ] **Step 8: Run tests, clippy, commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-refine -- --nocapture
cargo clippy -p inspectah-refine -- -D warnings
git add inspectah-refine/src/session.rs
git commit -m "feat(refine): wire RefineSession with projection caches

cached_decisions recomputed on mutation alongside cached_view.
cached_reference is OnceLock — computed once, immutable for session lifetime.
decisions() and reference() accessors for consumers."
```

---

## Task 9: Relocate web DTO types + capture contract snapshots (pre-cutover gate)

**Files:**
- Create: `inspectah-web/src/web_types.rs`
- Create: `inspectah-web/tests/contract_snapshots.rs`
- Modify: `inspectah-web/src/lib.rs`
- Modify: `inspectah-web/src/handlers.rs` (re-export from web_types)

This task solves the type lifecycle bug: the adapter (Task 10) needs `ViewResponse`,
all `*DecisionDto` structs, `ReferenceSection`, `ContextItem`, `ContextSubsection`,
`RepoGroupInfo`, and `VersionChangeEntry`. These currently live in `handlers.rs`.
Task 13 will remove dead code from `handlers.rs` — but the types must survive.

Moving them to `web_types.rs` BEFORE cutover ensures the adapter can import them,
and Task 13's dead code removal does not delete types still in use.

- [ ] **Step 1: Create `web_types.rs`**

Move (cut from `handlers.rs`, paste into `web_types.rs`) these type definitions:

```
ReferenceSection (L84-92)
ContextSubsection (L94-99)
ContextItem (L102-108)
reference_section() helper (L111-119)
RepoGroupInfo (L122-129)
ServiceDecisionDto (L132-139)
DropInDecisionDto (L142-148)
QuadletDecisionDto (L151-157)
FlatpakDecisionDto (L160-168)
SysctlDecisionDto (L175-182)
TunedDecisionDto (L186-191)
ViewResponse (L194-208)
VersionChangeEntry (L211-219)
```

Add appropriate imports at the top of `web_types.rs`. Keep derives and serde
attributes intact.

- [ ] **Step 2: Re-export from `handlers.rs`**

At the top of `handlers.rs`, replace the old type definitions with:

```rust
pub use crate::web_types::*;
```

This ensures all existing code that imports from `handlers` still works.

- [ ] **Step 3: Add module to `lib.rs`**

```rust
pub mod web_types;
```

- [ ] **Step 4: Verify compilation**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-web
```

Everything must pass — the re-export makes this a no-op refactor.

- [ ] **Step 5: Write contract snapshot tests**

Create `inspectah-web/tests/contract_snapshots.rs`. Since `build_view_response`
is private and `rich_snapshot()` is private to `api_test.rs`, use the HTTP
test harness approach:

```rust
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::services::*;
use inspectah_core::types::rpm::*;
// ... (same imports as api_test.rs for snapshot types)
use inspectah_refine::session::RefineSession;
use inspectah_web::handlers::AppState;
use std::sync::{Arc, Mutex, OnceLock};
use tower::ServiceExt;

/// Build a rich snapshot for contract testing.
/// This is a self-contained fixture — does NOT depend on api_test.rs.
fn contract_snapshot() -> InspectionSnapshot {
    // Build a snapshot that exercises every section.
    // Can be simpler than rich_snapshot() — just needs at least one item
    // in each section (services, rpm, containers, network, storage,
    // scheduled_tasks, non_rpm_software, kernel_boot, selinux).
    let mut snap = InspectionSnapshot::new();
    // ... populate each section ...
    snap
}

fn contract_state() -> Arc<AppState> {
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(contract_snapshot()))),
        sections_cache: OnceLock::new(),
    })
}

fn app(state: Arc<AppState>) -> axum::Router {
    inspectah_web::router(state, "http://localhost:8642")
}

async fn get_json(app: &axum::Router, path: &str) -> serde_json::Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn get_view_contract() {
    let app = app(contract_state());
    let json = get_json(&app, "/api/view").await;
    insta::assert_json_snapshot!("get_view_contract", json);
}

#[tokio::test]
async fn get_sections_contract() {
    let app = app(contract_state());
    let json = get_json(&app, "/api/snapshot/sections").await;
    insta::assert_json_snapshot!("get_sections_contract", json);
}
```

- [ ] **Step 6: Run tests and accept snapshots**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-web contract -- --nocapture
cargo insta review -p inspectah-web
```

First run creates new snapshots. Review and accept them — these are the golden baseline.

- [ ] **Step 7: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/src/web_types.rs inspectah-web/src/handlers.rs inspectah-web/src/lib.rs inspectah-web/tests/contract_snapshots.rs inspectah-web/tests/snapshots/
git commit -m "refactor(web): relocate DTO types to web_types.rs + capture contract snapshots

Types moved out of handlers.rs so the adapter can import them
independently. Contract snapshots freeze the current wire format
as a pre-cutover gate."
```

---

## Task 10: Build web adapter — decision projection

**Files:**
- Create: `inspectah-web/src/adapter.rs`
- Modify: `inspectah-web/src/lib.rs`

- [ ] **Step 1: Write failing test**

The test constructs the same `AppState` / router combo and compares the adapter
output against the old code path's HTTP response:

```rust
// inspectah-web/src/adapter.rs

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::snapshot::InspectionSnapshot;
    use inspectah_refine::session::RefineSession;

    fn test_snapshot() -> InspectionSnapshot {
        // Minimal but populated snapshot (same pattern as contract_snapshots.rs)
        let mut snap = InspectionSnapshot::new();
        // Add at least: rpm.packages_added, services.state_changes, config.files
        snap
    }

    #[test]
    fn build_web_view_produces_valid_json() {
        let session = RefineSession::new(test_snapshot());
        let result = build_web_view(&session);
        let json = serde_json::to_value(&result).unwrap();
        assert!(json.get("generation").is_some());
        assert!(json.get("repo_groups").is_some());
        assert!(json.get("service_states").is_some());
    }
}
```

- [ ] **Step 2: Implement `build_web_view()`**

The adapter takes `&RefineSession` and produces `ViewResponse` by reading from
`session.view()`, `session.decisions()`, and `session.baseline_summary()`.

```rust
use crate::web_types::*;
use inspectah_refine::session::RefineSession;

/// Build a ViewResponse from session state using the new projection path.
/// Produces identical JSON to the old build_view_response() in handlers.rs.
pub fn build_web_view(session: &RefineSession) -> ViewResponse {
    let view = session.view().clone();
    let decisions = session.decisions();

    // Map Refined* types to the existing DTO structs.
    // This is a 1:1 field copy — the Refined types have the same data,
    // just in a different struct shape.

    let service_states: Vec<ServiceDecisionDto> = decisions
        .service_states
        .iter()
        .map(|s| ServiceDecisionDto {
            unit: s.entry.unit.clone(),
            triage: s.triage.clone(),
            include: s.entry.include,
            owning_package: s.entry.owning_package.clone(),
        })
        .collect();

    let service_dropins: Vec<DropInDecisionDto> = decisions
        .service_dropins
        .iter()
        .map(|d| DropInDecisionDto {
            unit: d.entry.unit.clone(),
            path: d.entry.path.clone(),
            triage: d.triage.clone(),
            include: d.entry.include,
        })
        .collect();

    let quadlets: Vec<QuadletDecisionDto> = decisions
        .quadlets
        .iter()
        .map(|q| QuadletDecisionDto {
            path: q.entry.path.clone(),
            name: q.entry.name.clone(),
            image: q.entry.image.clone(),
            triage: q.triage.clone(),
            include: q.entry.include,
        })
        .collect();

    let flatpaks: Vec<FlatpakDecisionDto> = decisions
        .flatpaks
        .iter()
        .map(|f| FlatpakDecisionDto {
            app_id: f.entry.app_id.clone(),
            remote: f.entry.remote.clone(),
            branch: f.entry.branch.clone(),
            triage: f.triage.clone(),
            include: f.entry.include,
            lifecycle: "first_boot".to_string(),
        })
        .collect();

    let sysctls: Vec<SysctlDecisionDto> = decisions
        .sysctls
        .iter()
        .map(|s| SysctlDecisionDto {
            key: s.entry.key.clone(),
            runtime: s.entry.runtime.clone(),
            default: s.entry.default.clone(),
            source: s.entry.source.clone(),
            triage: s.triage.clone(),
            include: s.entry.include,
        })
        .collect();

    let tuned: Vec<TunedDecisionDto> = decisions
        .tuned
        .iter()
        .map(|t| TunedDecisionDto {
            active_profile: t.active_profile.clone(),
            custom_profiles: t.custom_profiles.clone(),
            triage: t.triage.clone(),
            include: t.include,
        })
        .collect();

    // Repo groups — map RepoGroup to RepoGroupInfo
    let repo_groups: Vec<RepoGroupInfo> = decisions
        .repo_groups
        .iter()
        .map(|rg| RepoGroupInfo {
            section_id: rg.section_id.clone(),
            provenance: rg.provenance.clone(),
            is_distro: rg.is_distro,
            tier: rg.tier.clone(),
            package_count: rg.package_count,
            enabled: rg.enabled,
        })
        .collect();

    // Version changes — map core VersionChange (typed enum) to wire VersionChangeEntry (string direction)
    let version_changes: Vec<VersionChangeEntry> = decisions
        .version_changes
        .iter()
        .map(|vc| {
            use inspectah_core::types::rpm::VersionChangeDirection;
            let dir = match vc.direction {
                VersionChangeDirection::Upgrade => "upgrade",
                VersionChangeDirection::Downgrade => "downgrade",
            };
            VersionChangeEntry {
                name: vc.name.clone(),
                arch: vc.arch.clone(),
                host_version: vc.host_version.clone(),
                base_version: vc.base_version.clone(),
                host_epoch: vc.host_epoch.clone(),
                base_epoch: vc.base_epoch.clone(),
                direction: dir.to_string(),
            }
        })
        .collect();

    ViewResponse {
        view,
        repo_groups,
        baseline_summary: decisions.baseline_summary.clone(),
        version_changes,
        service_states,
        service_dropins,
        quadlets,
        flatpaks,
        sysctls,
        tuned,
        users_groups_decisions: decisions.users_groups.clone(),
        session_is_sensitive: decisions.is_sensitive,
    }
}
```

**IMPORTANT:** Verify that DTO field names (e.g., `s.entry.unit`, `s.entry.include`)
match the actual `Refined*` struct field names from `types.rs`. Cross-reference
with the `build_*_decisions` functions in `handlers.rs` to ensure identical mapping.

- [ ] **Step 3: Add `pub mod adapter;` to `lib.rs`**

- [ ] **Step 4: Run tests, clippy, commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-web build_web_view -- --nocapture
cargo clippy -p inspectah-web -- -D warnings
git add inspectah-web/src/adapter.rs inspectah-web/src/lib.rs
git commit -m "feat(web): implement build_web_view() adapter

Maps DecisionProjection to ViewResponse wire shape. Produces identical
JSON to old build_view_response() path."
```

---

## Task 11: Build web adapter — reference sections

**Files:**
- Modify: `inspectah-web/src/adapter.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn build_web_sections_returns_nine_sections() {
    let snap = InspectionSnapshot::new();
    let session = RefineSession::new(snap);
    let sections = build_web_sections(session.reference());
    // Must return ALL 9 sections — same as normalize_for_reference
    assert_eq!(sections.len(), 9);
    let ids: Vec<&str> = sections.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec![
        "services", "version_changes", "containers",
        "network", "storage", "scheduled_tasks",
        "non_rpm_software", "kernel_boot", "selinux",
    ]);
}
```

- [ ] **Step 2: Implement per-section adapters**

For each section, port the PRESENTATION logic from the corresponding `normalize_*`
function. The domain extraction is already done by `project_ref_*()` in Tasks 4-7.
The adapter just maps typed domain data to `ContextItem` / `ReferenceSection` wire shape.

```rust
use crate::web_types::*;
use inspectah_refine::projection::*;

pub fn web_services_section(data: &RefServices) -> ReferenceSection { /* ... */ }
pub fn web_version_changes_section(data: &RefVersionChanges) -> ReferenceSection { /* ... */ }
pub fn web_containers_section(data: &RefContainers) -> ReferenceSection { /* ... */ }
pub fn web_kernel_boot_section(data: &RefKernelBoot) -> ReferenceSection { /* ... */ }
pub fn web_network_section(data: &RefNetwork) -> ReferenceSection { /* ... */ }
pub fn web_storage_section(data: &RefStorage) -> ReferenceSection { /* ... */ }
pub fn web_generic_section(id: &str, display_name: &str, items: &[GenericRefItem]) -> ReferenceSection { /* ... */ }
```

Port the presentation logic (subtitle formatting, searchable_text assembly) from
each `normalize_*` function in `handlers.rs`. The exact strings MUST match — the
contract snapshot gate will catch any drift.

- [ ] **Step 3: Implement `build_web_sections()` orchestrator**

```rust
/// Build all 9 reference sections in canonical order.
/// Order matches normalize_for_reference() L933-944.
/// Returns ALL 9 sections unconditionally — empty sections carry empty_reason.
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

**CRITICAL:** The section order above matches the ACTUAL `normalize_for_reference()`.
Do NOT reorder. Do NOT filter empty sections — the current code returns all 9 unconditionally,
and the test `sections_returns_nine_sections` asserts `sections.len() == 9`.

- [ ] **Step 4: Run tests, clippy, commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-web build_web_sections -- --nocapture
cargo clippy -p inspectah-web -- -D warnings
git add inspectah-web/src/adapter.rs
git commit -m "feat(web): implement per-section web adapters + build_web_sections()

9 per-section adapters map ReferenceProjection data to
ContextItem/ReferenceSection wire shapes. build_web_sections()
orchestrates in canonical section order. Returns all 9 unconditionally."
```

---

## Task 12: Atomic endpoint cutover

**Files:**
- Modify: `inspectah-web/src/handlers.rs`

- [ ] **Step 1: Switch `get_view` handler**

In `handlers.rs` L320, the current handler is:

```rust
pub async fn get_view(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let session = state.session.lock().unwrap();
    let response = build_view_response(&session);
    Json(serde_json::to_value(&response).unwrap())
}
```

Replace with:

```rust
pub async fn get_view(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let session = state.session.lock().unwrap();
    let response = crate::adapter::build_web_view(&session);
    Json(serde_json::to_value(&response).unwrap())
}
```

- [ ] **Step 2: Switch mutation handlers**

Find all places that call `build_view_response()` after a mutation. These are
in `apply_op`, `undo`, `redo`, `user_strategy`, `user_password`:

```bash
grep -n 'build_view_response' /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/src/handlers.rs
```

Replace each `build_view_response(&session)` with `crate::adapter::build_web_view(&session)`.

- [ ] **Step 3: Switch `get_sections` handler**

In `handlers.rs` L897, the current handler is:

```rust
pub async fn get_sections(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let sections = state.sections_cache.get_or_init(|| {
        let session = state.session.lock().unwrap();
        normalize_for_reference(session.snapshot())
    });
    Json(sections.clone())
}
```

Replace with:

```rust
pub async fn get_sections(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let sections = state.sections_cache.get_or_init(|| {
        let session = state.session.lock().unwrap();
        crate::adapter::build_web_sections(session.reference())
    });
    Json(sections.clone())
}
```

- [ ] **Step 4: Verify contract snapshots match**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-web contract -- --nocapture
```

Expected: ALL contract snapshot tests pass. If any fail, the adapter is producing
different JSON than the old code. Fix the adapter — do NOT update the snapshots.

- [ ] **Step 5: Run full test suite**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-web
```

Expected: ALL tests pass, including `sections_returns_nine_sections` (asserts 9 sections).

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/src/handlers.rs
git commit -m "refactor(web): atomic cutover to projection-based handlers

All endpoints now use adapter::build_web_view() and
adapter::build_web_sections() instead of build_view_response()
and normalize_for_reference(). Contract snapshot tests verify
byte-identical output."
```

---

## Task 13: Dead code removal

**Files:**
- Modify: `inspectah-web/src/handlers.rs`

- [ ] **Step 1: Identify dead functions**

After the cutover, these functions in `handlers.rs` are dead:

```
build_view_response()
build_service_decisions()
build_container_decisions()
build_sysctl_decisions()
build_tuned_decisions()
build_sensitivity_summary()
normalize_for_reference()
normalize_services()
normalize_version_changes()
normalize_containers()
normalize_network()
normalize_storage()
normalize_kernel_boot()
normalize_scheduled_tasks()
normalize_non_rpm_software()
normalize_selinux()
format_evr_pair()
reference_section()  (helper, now in web_types.rs)
```

- [ ] **Step 2: Verify callers before removing**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && grep -rn 'build_view_response\|normalize_for_reference\|build_service_decisions\|build_container_decisions\|build_sysctl_decisions\|build_tuned_decisions\|build_sensitivity_summary\|normalize_services\|normalize_version_changes\|normalize_containers\|normalize_network\|normalize_storage\|normalize_kernel_boot\|normalize_scheduled_tasks\|normalize_non_rpm_software\|normalize_selinux\|format_evr_pair' inspectah-web/src/ inspectah-web/tests/
```

Only remove functions with ZERO remaining callers. Pay special attention to:

- `normalize_for_reference` — also imported in `api_test.rs` (L28: `use inspectah_web::handlers::normalize_for_reference`). Update the test to use `crate::adapter::build_web_sections` instead, or remove the test import if it's no longer used.
- `build_repo_groups()` — keep this if `fleet_handlers.rs` still imports it.

- [ ] **Step 3: Remove dead functions and DTO structs from handlers.rs**

The DTO structs (`ServiceDecisionDto`, etc.) now live in `web_types.rs`, so
the copies in `handlers.rs` are already removed (Task 9 moved them). Any
remaining dead type definitions can go.

Clean up unused `use` imports at the top of `handlers.rs`.

- [ ] **Step 4: Run full workspace test suite**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test
```

Expected: ALL tests pass across entire workspace.

- [ ] **Step 5: Run clippy**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo clippy --workspace -- -D warnings
```

Expected: zero warnings.

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/src/handlers.rs inspectah-web/tests/
git commit -m "refactor(web): remove dead projection code from handlers.rs

Projection logic, normalize_* functions, and decision builder functions
superseded by inspectah-refine projection module + web adapter layer."
```

---

## Task 14: Frontend type updates (Kit)

**Files:**
- Verify: `inspectah-web/ui/src/api/types.ts`
- Verify: `inspectah-web/ui/src/components/GlobalSearch.tsx`

This task is owned by **Kit**.

- [ ] **Step 1: Verify current wire shape is unchanged**

After the backend cutover, start the dev server and verify the UI works:

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && cargo run -- refine --port 8642 <snapshot-tarball>
```

Open `http://localhost:8642` and verify:
- All 9 reference sections render
- All decision sections render (services, containers, sysctls, tuned, users/groups)
- Search works across all sections
- Triage toggles (include/exclude) work and the view updates
- Undo/redo works
- Export works

- [ ] **Step 2: Compare API responses against TypeScript types**

```bash
curl -s http://localhost:8642/api/view | jq keys
curl -s http://localhost:8642/api/snapshot/sections | jq '.[0] | keys'
```

Cross-reference with `types.ts`. The wire shape should be identical. If any
field names changed (they should not have — the adapter preserves the exact
DTO shape), update `types.ts` to match.

- [ ] **Step 3: Verify GlobalSearch**

Search for a known service, package, and config item. Verify results match the
pre-cutover behavior. The `searchable_text` format is preserved by the per-section
adapters.

- [ ] **Step 4: Commit if changes needed**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/
git commit -m "fix(ui): update TypeScript types for projection cutover"
```

If no changes needed, skip this commit.

---

## Self-Review Checklist

1. **Spec coverage:**
   - [x] Tuned include field (Task 1)
   - [x] Projection types module (Task 2)
   - [x] project_decisions() (Task 3)
   - [x] Reference extractors — all 9 sections (Tasks 4-7)
   - [x] RefineSession wiring (Task 8)
   - [x] Type relocation + contract snapshot gate (Task 9)
   - [x] Web adapter — decision view (Task 10)
   - [x] Web adapter — reference sections (Task 11)
   - [x] Atomic endpoint cutover (Task 12)
   - [x] Dead code removal (Task 13)
   - [x] Frontend type updates (Task 14)

2. **Critical bug fixes from v1:**
   - [x] Types reused, not redefined (UserGroupDecision from inspectah-core, BaselineSummary from inspectah-refine, RepoProvenance/RepoTier as enums not strings)
   - [x] Actual classify_* signatures used (tuple returns for services and containers)
   - [x] Correct section order (matches normalize_for_reference L933-944)
   - [x] All 9 sections returned unconditionally (no filtering)
   - [x] Type relocation handled explicitly (web_types.rs created in Task 9, before adapter needs types)
   - [x] Test strategy uses real harness (HTTP requests via tower::ServiceExt, insta snapshots)
   - [x] Private helpers accessed via HTTP harness, not pub(crate) hacks

3. **Type consistency:** All types reference the definitions in Task 2. DecisionProjection, ReferenceProjection, RefServices, etc. are consistent throughout. Types that already exist in other crates are imported, not redefined.
