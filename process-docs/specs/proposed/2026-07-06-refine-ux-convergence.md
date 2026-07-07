# Refine & TUI UX Convergence

## Status
Proposed

## Problem

The extended findings branch added advisory findings, section grouping, /var dual treatment, full-shadow detection, network inventory, and other new data types. The data model, collection, HTML report, audit report, and Containerfile rendering all use the new taxonomy. However, the refine web UI and TUI still carry the older Review/Reference taxonomy and miss several rendering paths for the new data.

The review panel (Thorn, Collins, Fern, Tang) across R1–R4 of the extended findings work consistently flagged these as must-fix items. They are architectural changes to the interactive surfaces, not quick fixes, so Mark approved deferring them into their own spec cycle.

## Design Decisions

All decisions were validated through consults with Ember (product), Fern (UX/interaction design), Tang (Rust implementation), and Collins (bootc domain correctness).

## 1. Sidebar Navigation Overhaul

### Current State

The sidebar uses a hardcoded "Review" / "Reference" two-group model:
- **Review** (8 sections): packages, configs, users_groups, services, containers, language_packages, unmanaged_files, system_tuning
- **Reference** (8 sections): version_changes, compose, network, storage, scheduled_tasks, non_rpm_software, kernel_boot, selinux

`MainContent.tsx` dispatches via flat `if (activeSection === ...)` branches with no group awareness.

### Design

Replace the Review/Reference split with 8 collapsible domain groups using PatternFly `NavExpandable`.

**Single source of truth:** `SectionGroup` in `crates/pipeline/src/section_group.rs` is the canonical source for group membership, labels, and slugs. All downstream consumers (web API metadata, sidebar rendering, batch-toggle routing, keyboard shortcuts) derive from this enum — no separate mapping.

**Complete section-to-group mapping.** `SectionGroup::for_section()` must cover every section ID that appears in the web sidebar. The current mapping plus required additions:

| Section ID | Group | Notes |
|-----------|-------|-------|
| `rpm` / `packages` | Packages | existing |
| `config` / `configs` | SystemConfig | existing |
| `kernel_boot` | SystemConfig | existing |
| `selinux` | SystemConfig | existing |
| `services` | Services | existing |
| `scheduled_tasks` | Services | existing |
| `containers` / `compose` | Services | existing (`compose` is a frontend alias for `containers`) |
| `users_groups` | Identity | existing |
| `network` | Network | existing |
| `storage` | Storage | existing |
| `non_rpm_software` | Software | existing |
| `unmanaged_files` | Software | existing |
| `language_packages` | Software | **new** — currently missing from `for_section()` |
| `secrets` | Secrets | existing |
| `subscription` | Secrets | existing |

**Retired section IDs:** `system_tuning` and `version_changes` are web-only aggregation views that do not map to snapshot sections. In the new model:
- `system_tuning` is retired — its contents (kernel_boot + selinux items) become separate sections under System Configuration
- `version_changes` is retired — it folds into the Packages section as a collapsible "Version Changes" panel within the packages content view (not a separate sidebar entry)

`MainContent.tsx` routes for these retired IDs are removed. Any saved session state referencing them is ignored (treated as if the section doesn't exist).

**Batch-toggle routing:** Route parameters use `SectionGroup::slug()` values, replacing the current ad-hoc route names (`packages`, `configs`, `services`, `network`). Unit tests verify: (1) every section ID in the table above resolves to a group, (2) slug values are unique, (3) batch-toggle routes match slug values exactly, (4) no route exists for reference-only groups.

**Groups and their sections:**

| Group | Sections | Type |
|-------|----------|------|
| Packages | packages | triage |
| System Configuration | configs, kernel_boot, selinux | configs=triage; kernel_boot, selinux=reference |
| Services & Scheduling | services, containers, scheduled_tasks, compose | services, containers=triage; scheduled_tasks, compose=reference |
| Users & Identity | users_groups | triage |
| Network | network | reference |
| Storage | storage | reference (with advisory items) |
| Software & Files | language_packages, unmanaged_files, non_rpm_software | language_packages, unmanaged_files=triage; non_rpm_software=reference |
| Secrets & Subscription | secrets, subscription | reference |

**Singleton groups** (Packages, Users & Identity, Network, Storage) render as a single clickable entry with no expand arrow. **Multi-section groups** expand to show child section entries.

**Badge differentiation:**
- Triage sections: blue `Badge` (PatternFly default variant) with item count
- Reference sections: grey `Badge` (`isRead` variant) with no count

The absence of a count on reference sections is itself a signal — triage sections visually pop without adding noise to context sections. Individual items within each section carry their own `FindingKind` (Actionable/Advisory/Inventory) semantics, so the interactivity distinction is per-item, not per-section.

**Bulk-exclude state visibility:** When all actionable items in a triage section are excluded, the badge text changes to "0" (textual state, not color-only). Dimming is secondary styling. Screen reader announcement via `aria-live` when state changes (e.g., "Packages: 0 decisions remaining").

**Collapsed/expanded state:** Persisted to the refine session so it survives page reloads. Storage: sidebar expansion state is a `Record<string, boolean>` on the session object, keyed by group slug. Default: all groups expanded on first load.

**Keyboard and focus contract by group type:**

| Group type | Number-key jump | Enter/Space on heading | Click on heading | Batch menu | aria-current |
|------------|----------------|----------------------|-----------------|------------|-------------|
| Singleton triage (Packages, Users & Identity) | Focus the entry, load section, set `aria-current="page"` | Load section | Load section | Show menu | On the NavItem |
| Multi-section triage (System Config, Services, Software) | Expand if collapsed, focus first triage child, load it, set `aria-current="page"` on child | Toggle expand/collapse only (does NOT load a section) | Toggle expand/collapse only | Show menu | On the active child NavItem, not the heading |
| Singleton reference (Network, Storage) | Focus the entry, load section, set `aria-current="page"` | Load section | Load section | No menu | On the NavItem |
| Multi-section reference (Secrets & Subscription) | Expand if collapsed, focus first child, load it, set `aria-current="page"` on child | Toggle expand/collapse only | Toggle expand/collapse only | No menu | On the active child NavItem |

**Key distinctions:** Multi-section group headings are expand/collapse controls only — they never load a section themselves. Singleton groups have no heading separate from their single section entry.

**`aria-current` rule (single authoritative rule):** `aria-current="page"` is always on the active section's NavItem, even when its parent group is collapsed and the NavItem is not visible. Collapsing a group hides the NavItem visually but does not change which section is active — the content area continues showing it. Group headings never receive `aria-current`. When a screen reader user navigates to a collapsed group heading, the heading announces as a collapsed group, not as the current page.

**Focus restoration:** When a group is collapsed while one of its children is the active section, keyboard focus moves to the group heading (because the child NavItem is no longer visible). `aria-current="page"` stays on the hidden child NavItem — it does not move to the heading. The content area continues showing the active section until the user navigates to a different section.

**Heading row focus order (Tab order):** Group label (focusable, handles expand/collapse) → kebab action menu button (if present, triage groups only) → no further tab stops on the heading row. The expand chevron is decorative (part of the group label's click target), not a separate focusable element. Arrow Down from the heading moves to the first child section entry if expanded.

### Affected files

- `crates/web/ui/src/components/Sidebar.tsx` — replace `BASE_REVIEW_SECTIONS` / `REFERENCE_SECTIONS` constants and `NavGroup` structure with 8 `NavExpandable` groups
- `crates/web/ui/src/components/MainContent.tsx` — remove flat section dispatch; route through group-aware rendering (content area behavior unchanged — one section per view)
- `crates/web/ui/src/hooks/useKeyboard.ts` — update number-key shortcuts to target groups
- `crates/web/ui/src/api/types.ts` — add section-group metadata to API response if needed
- `crates/web/src/adapter.rs` — tag sections with their group in the API response

## 2. Group-Level Batch Toggle

### Design

Group-level "Include all / Exclude all" via an action menu on group headings. **Only rendered on groups with actionable descendants.** Reference-only groups (Network, Storage, Secrets & Subscription) do not show the menu, keyboard shortcuts, or backend routes.

**Interaction pattern:**
- Kebab/ellipsis icon button on group heading row, right-aligned before the expand chevron
- Opens a two-item action menu: "Include all" / "Exclude all"
- No per-section toggles in the sidebar — per-section batch controls already exist in the content area
- `aria-live` announcement on toggle completion (e.g., "12 items included in Services & Scheduling")
- Partial-success case (some items locked/skipped): announcement includes count of affected items ("8 of 12 items included — 4 locked")

**Keyboard shortcuts:** Ctrl+Shift+A (include all in focused group) / Ctrl+Shift+X (exclude all in focused group) when a group heading has focus. Shortcuts are no-ops on reference-only groups (no handler registered).

**Safety gate:** When the user selects "Exclude all" on the Packages group, show a confirmation dialog warning that this produces an image missing critical runtime dependencies. Other triage groups do not require confirmation.

### Backend

`batch_toggle_group()` in `crates/web/src/handlers.rs` currently supports packages, configs, services, and network (no-op). Expand to cover groups with actionable items: Packages, System Configuration, Services & Scheduling, Users & Identity, Software & Files. Reference-only groups (Network, Storage, Secrets & Subscription) are not registered as routes. Route parameters use `SectionGroup::slug()` values, replacing the current ad-hoc route names.

### Frontend

- `crates/web/ui/src/api/client.ts` — add `batchToggleGroup(groupSlug: string, include: boolean)` method
- Sidebar group heading wires to the client method via the kebab action menu

## 3. Full-Shadow Service Rendering in Refine Web

### Current State

The collector (`crates/collect/src/inspectors/services.rs` step 5c) correctly discovers full-shadow services — where `/etc/systemd/system/foo.service` completely replaces the vendor unit. `ServiceSection.tsx` renders `shadow_rationale` helper text for full-shadow services that also have state divergence (they have a parent row). Orphan full-shadow services (no state divergence, e.g., sshd running normally with a local override) have no parent row and are invisible in refine web.

### Design

**Mutation contract:** Orphan full-shadow services become first-class refine decisions only when their durable host state is already representable under the existing `ServiceStateChange.current_state: ServiceUnitState` contract. This cycle does **not** change that contract.

**Synthesis location:** `crates/refine/src/classify.rs`, in the classification pass that runs before the session is constructed. After the collector delivers the snapshot, classify iterates `services.drop_ins` looking for full-shadow entries whose `unit` has no corresponding entry in `services.state_changes`. For each orphan whose durable state is already known from the existing service inventory, classify inserts a synthetic `ServiceStateChange` into `state_changes` with:
- `unit`: the shadow file's unit name (e.g., `sshd.service`)
- `shadow_type: Some(ShadowType::FullShadow)`
- `shadow_rationale: Some("base image updates to this unit will be silently ignored")`
- `include: true` (default — shadows are actionable findings)
- `current_state`: copied from the durable host state already known in the snapshot under the existing contract

**Unavailable-state boundary:** If an orphan full-shadow unit appears without durable host state that can be represented by the current `ServiceStateChange.current_state: ServiceUnitState` contract, classify does **not** synthesize a `ServiceStateChange` for it in this cycle. Supporting unknown-state orphan full-shadows would require an explicit contract change to `ServiceStateChange.current_state`, which is out of scope for this spec revision.

**Why classify, not orchestrate:** The synthesis must happen before `RefineSession::new()` sees the snapshot, so the session validates the synthetic entry as a normal `ServiceStateChange`. `classify.rs` already runs at this boundary.

**Session contract:** Once the synthetic entry exists in `state_changes`, the entire existing contract applies without modification:
- `ItemId::Service { unit }` — validates successfully because the unit now exists in `state_changes`
- `SetInclude` / `SetExclude` — mutates the entry's `include` field
- **Autosave:** synthetic entries are serialized as part of the session JSON, same as real entries
- **Reload:** on session reload, synthetic entries load from the session file. If the underlying snapshot is re-scanned and the shadow file is gone, the synthetic entry has no matching drop-in — classify does not re-synthesize it, and the stale session entry is pruned during session/snapshot reconciliation (existing behavior)
- **Export:** the Containerfile renderer sees the entry like any other included service. If included, the shadow file is copied into the image. If excluded, it is omitted.

No new `ItemId` variant required. The adapter renders these entries with shadow-specific visual treatment but does not synthesize the decision itself.

**Visual treatment (ALL full-shadow services, not just orphans):**

- **Warning amber border-left** using `--pf-v5-global--warning-color--100`, consistent with the existing triage-level border-left pattern in ServiceSection
- **Compact "Shadow override" `Label` badge** on the row
- **`shadow_rationale` helper text** underneath: "base image updates to this unit will be silently ignored" — rendered as italic, muted, borderless (matching existing pattern). Helper text element has a stable `id` attribute for programmatic association.
- **Toggle behavior** identical to any other actionable service — include/exclude

**Border-left conflict resolution:** When a shadow service also has a triage-level border (e.g., high-attention due to state divergence), the triage-level border wins the left edge. The shadow gets a subtle amber background tint on the row instead. Single signal per visual channel.

**Badge ordering:** Left-to-right on the row: triage severity badge → "Shadow override" badge → locked badge. Consistent stacking order prevents visual noise.

**Accessibility contract for the toggle control:**
- The checkbox/toggle `aria-label` is the unit name (e.g., "sshd.service")
- `aria-describedby` on the checkbox references the helper text element's `id` — screen readers announce the shadow rationale when the toggle is focused
- When both shadow and locked descriptions apply, `aria-describedby` references both IDs (space-separated) — shadow description first, locked explanation second
- The row-level `aria-description` from the initial design is removed; the interactive control owns the descriptive association

**Shadow count in summary:** The Services section header shows a breakdown when shadows are present: "12 services (3 shadow overrides)". Sidebar badge includes shadow services in its count — they are services.

### Affected files

- `crates/refine/src/classify.rs` — synthesize `ServiceStateChange` entries for orphan full-shadow services during classification pass
- `crates/refine/src/projection/types.rs` — add `shadow_type: Option<String>` and `shadow_rationale: Option<String>` to `RefServiceItem`
- `crates/web/src/adapter.rs` — attach shadow fields to all full-shadow services (no synthesis needed — the pipeline provides real entries)
- `crates/web/ui/src/components/ServiceSection.tsx` — warning border-left for full-shadow rows, "Shadow override" badge, aria-describedby on toggle, border conflict resolution
- `crates/web/src/web_types.rs` — shadow fields already exist in web DTOs (added in extended findings work)

## 4. TUI Inventory-Row Modeling

**Note:** The TUI is scheduled for a broader redesign. This section covers only the correctness fix for the inventory toggle path — not rendering polish.

### Current State

Network inventory items (connections, firewall zones, static routes, IP routes/rules, resolv, hosts, proxy) are built with `RawItem::new()` in `single_host.rs` instead of using the `ListItem::inventory()` constructor. This means they can accidentally enter a toggle path. `app.rs` guards `is_advisory` but not inventory rows. `session.rs` accepts inventory `ItemId` variants but treats them as unhandled during projection, creating a meaningless toggle cycle.

### Design

- `single_host.rs` network section: switch from `RawItem::new()` to `ListItem::inventory()` for all network inventory rows
- `app.rs`: add `item.is_inventory` to the toggle guard alongside `item.is_advisory`
- `session.rs`: reject inventory `ItemId` variants at the validation layer with an error — if an inventory toggle reaches the session, it's a bug in the caller, and it should surface immediately
- ifcfg deprecation advisory stays as `RawItem::advisory()` — it IS an advisory (user guidance about deprecated configuration), not inventory

This is a small correctness fix using existing infrastructure. `ListItem::inventory()` already produces the correct visual treatment. No TUI rendering changes.

### Affected files

- `crates/tui/src/screen/single_host.rs` — network section item construction
- `crates/tui/src/app.rs` — toggle guard
- `crates/refine/src/session.rs` — validation rejection for inventory ItemId variants

## 5. /var Ownership Detection and tmpfiles.d Output

### Current State

`VarDirectory` in `crates/core/src/types/storage.rs` has `path`, `size_estimate`, `recommendation`, and `backing` fields but no `owner`/`group`. The Containerfile renderer emits `RUN mkdir -p {path}` for unbacked /var directories but cannot produce ownership-correct provisioning.

### Design

inspectah should be opinionated about image-mode best practices. In bootc, the image's /var contents seed the host on **first deployment only** — subsequent upgrades preserve the existing /var and ignore the image's copy. `RUN mkdir -p && chown` works for first boot but fails if the directory is deleted or permissions drift. A `tmpfiles.d` entry runs every boot, handles recovery, and is the documented image-mode best practice.

**Primary output: tmpfiles.d under `/usr/lib/tmpfiles.d`**

For unbacked /var directories, emit a tmpfiles.d configuration file. The file goes under `/usr/lib/tmpfiles.d/` (vendor-managed image defaults), NOT `/etc/tmpfiles.d/` (admin override layer). In bootc/ostree, `/etc` is machine-local state with 3-way-merge semantics. Vendor-shipped image content belongs under `/usr`. Administrators who need to override inspectah's defaults can create `/etc/tmpfiles.d/inspectah-var.conf` which takes precedence per systemd's tmpfiles.d lookup order.

```
# /usr/lib/tmpfiles.d/inspectah-var.conf
# postgres data directory (source uid=26, gid=26)
d /var/lib/pgsql/data 0750 postgres postgres -
# application directory (root-owned, default)
d /var/lib/custom-app 0755 root root -
```

Note: tmpfiles.d syntax does not support inline comments. The `#` character is only treated as a comment when it is the first non-whitespace character on a line. All annotations go on separate comment lines above the entry.

**Renderer artifact contract:** A new function `render_tmpfiles_conf()` in `crates/pipeline/src/render/containerfile.rs` generates the tmpfiles.d conf file content from the list of unbacked `VarDirectory` entries that have ownership and mode data. The generated file is staged as `config/usr/lib/tmpfiles.d/inspectah-var.conf` in the output bundle, following the existing "materialize files first, derive COPY lines from the staged tree" model used by all other config artifacts. `render_all()` in `crates/pipeline/src/render/mod.rs` calls `render_tmpfiles_conf()` and writes the result to the staged path before deriving COPY directives. If no unbacked directories have ownership/mode data, the file is not generated and no COPY directive appears.

The Containerfile emits:

```dockerfile
# /var directories provisioned via tmpfiles.d (runs every boot, handles recovery).
# Override: create /etc/tmpfiles.d/inspectah-var.conf on the target host.
# Alternative: RUN mkdir -p /var/lib/pgsql/data && chown postgres:postgres /var/lib/pgsql/data
COPY config/usr/lib/tmpfiles.d/inspectah-var.conf /usr/lib/tmpfiles.d/inspectah-var.conf
```

**Ownership policy: names as primary, numeric fallback for unresolvable accounts.**

For each unbacked /var directory, the renderer resolves ownership at render time using information available in the snapshot and the Containerfile being built. The rule determines whether a named account is **guaranteed to exist in the target image** before emitting it:

1. **Root-owned (UID 0):** Emit `root root`. Always safe.
2. **Account present in user materialization output:** Emit name. The Containerfile's `useradd`/`groupadd` directives (from `crates/pipeline/src/render/users.rs`) create this account before tmpfiles.d runs. Guaranteed to exist.
3. **Account owned by an RPM that is included in the Containerfile:** Emit name. The RPM's scriptlets create the account during `dnf install`. Guaranteed to exist if the package installs successfully. Add a comment: `# created by package: postgresql-server`.
4. **Account name available but not covered by cases 1–3:** **Fall back to numeric UID:GID.** The account is not guaranteed to exist in the target image — it may be a packaged account from a package that isn't being replicated, or a stale account from a removed package. Add a comment: `# account 'postgres' not guaranteed in target — using numeric`.
5. **Name unavailable (stat returned numeric only):** Emit numeric UID:GID. Add a comment noting the unresolved name.

Comment lines above each entry show the source UID:GID for cross-reference:

```
# postgres data directory (source uid=26, gid=26)
d /var/lib/pgsql/data 0750 postgres postgres -
# custom app directory (source uid=1001, gid=1001)
d /var/lib/myapp 0700 appuser appgroup -
# unknown account (source uid=5432, gid=5432)
d /var/lib/orphaned 0750 5432 5432 -
```

**Permission bits:** `VarDirectory` captures the directory mode via `stat -c '%04a'` (zero-padded 4-digit octal, e.g., `0750`, `2770`, `1777`). This captures standard permissions and special bits (setuid, setgid, sticky). The mode is required for correct tmpfiles.d output — a silent fallback to `0755` would be wrong for directories with restricted permissions or special bits.

**Collection:** `discover_var_directories()` captures per directory in a single `stat` call:
- `stat -c '%04a %u %g %U %G'` — mode (4-digit octal), numeric UID, numeric GID, owner name, group name
- Parsed into VarDirectory fields. If `stat` fails (permissions error), the directory is recorded with all ownership/mode fields as `None` and a degradation warning is emitted.

**Schema: no version bump.** All new fields on `VarDirectory` are `Option<T>` with `#[serde(default)]`. Older v21 snapshots deserialize cleanly with these fields as `None`. The renderer handles `None` gracefully: directories without ownership/mode data fall back to `RUN mkdir -p` (the current behavior) instead of tmpfiles.d entries. No re-scan or re-aggregate required.

### Affected files

- `crates/core/src/types/storage.rs` — add `mode: Option<String>`, `owner_name: Option<String>`, `group_name: Option<String>`, `owner_uid: Option<u32>`, `owner_gid: Option<u32>` to `VarDirectory`
- `crates/collect/src/inspectors/storage.rs` — capture mode and ownership in `discover_var_directories()` via single `stat` call
- `crates/pipeline/src/render/containerfile.rs` — new `render_tmpfiles_conf()` function; emit staged file at `config/usr/lib/tmpfiles.d/inspectah-var.conf`; COPY directive from staged tree; fallback to RUN mkdir when ownership/mode data is `None`
- `crates/pipeline/src/render/mod.rs` — call `render_tmpfiles_conf()` in `render_all()`, write to staged path, add to artifact set
- All `VarDirectory` constructor sites (aggregate merge, fleet merge, degraded path, test constructors)
- HTML report template, TUI, refine web storage view — show ownership and mode info
- `crates/pipeline/src/render/readme.rs` — document tmpfiles.d artifact in generated README

## 6. ifcfg Deprecation Note in Refine Web

### Current State

`IFCFG_DEPRECATION_NOTE` is defined in `crates/core/src/types/network.rs` and consumed in the HTML report and audit renderer. The TUI renders it as an advisory item (added in extended findings work). There are zero references to ifcfg or the deprecation note in `crates/web/`.

### Design

Wire `has_ifcfg` detection and the deprecation note through the web adapter to the React network view.

**Banner content:** PatternFly `Alert` (variant: info, inline) at the top of the Network section when `has_ifcfg` is true. The banner text uses the existing `IFCFG_DEPRECATION_NOTE` constant from `crates/core/src/types/network.rs` — the same text rendered in the HTML report and audit trail. This keeps all surfaces in sync from a single source of truth.

The note's tone matches inspectah's current migration guidance: network configuration is environment-sensitive and should be planned separately for the target environment, not prescribed as a one-size-fits-all transformation.

### Affected files

- `crates/web/src/adapter.rs` — pass `has_ifcfg` and deprecation note text to network section DTO
- `crates/web/src/web_types.rs` — add ifcfg fields to network section response
- Network view component in `crates/web/ui/src/components/` — render `Alert` banner

## Implementation Approach

Single spec, single implementation plan with SDD task dependencies. Estimated 15–18 tasks.

**Parallelism:** Tasks 4 (TUI inventory), 5 (/var ownership collection), and the Rust-side data model work for 3 (full-shadow projection) and 6 (ifcfg adapter) can all run in parallel with the sidebar React work (1). They touch different crates with no compile-time conflicts.

**Sequencing constraint:** Task 2 (batch-toggle client) is blocked by Task 1 (sidebar overhaul) — the toggle controls need a place to live. Tasks 3 and 4 both modify `ListItem` in the TUI crate — add a `blocked_by` between them to avoid conflicts.

**Owner routing:**
- Kit: sidebar overhaul (1), batch-toggle frontend (2), full-shadow React rendering (3 frontend), ifcfg React banner (6 frontend)
- Tang: full-shadow projection + adapter (3 backend), TUI inventory (4), /var ownership (5), ifcfg adapter (6 backend), batch-toggle backend expansion (2 backend)
- Reviews: Thorn (code quality), Fern (UX/interaction), Collins (domain correctness)

## Out of Scope

- **Compose replication** — translating compose files into bootc image build artifacts. The compose topology (multi-container, external volumes/networks, variable interpolation) doesn't map to a single-image build. Future roadmap item.
- **Refine web aggregate view** — aggregate-specific rendering of the new finding types. The single-host refine view is the focus of this spec.

## Schema Impact

**No schema version bump.** All new `VarDirectory` fields are `Option<T>` with `#[serde(default)]`. Older v21 snapshots deserialize cleanly with these fields as `None`. The renderer falls back to current behavior (RUN mkdir) when ownership/mode data is absent. No re-scan or re-aggregate required.

## Testing Strategy

- **Sidebar:** Playwright tests for group expansion/collapse, badge rendering (blue/grey, "0" cleared state), keyboard navigation per group type (see §1 interaction table), batch-toggle interaction, focus restoration after collapse
- **Section-group contract:** Unit tests verifying: (1) `SectionGroup::for_section()` resolves every section ID in the §1 mapping table, (2) slug values are unique, (3) batch-toggle routes match slug values, (4) no route exists for reference-only groups, (5) retired IDs (`system_tuning`, `version_changes`) do not appear in the mapping
- **Full-shadow mutation round-trip:** Rust unit test proving classify.rs synthesizes `ServiceStateChange` for orphan full-shadow drop-ins, and that the synthetic entry survives: session construction → `ItemId::Service` validation → `SetInclude(false)` toggle → autosave → session reload → export (Containerfile omits the shadow file when excluded). Also test re-scan reconciliation: if the shadow file disappears, the stale session entry is pruned.
- **Full-shadow rendering:** React component tests for warning border, "Shadow override" badge, `aria-describedby` on toggle control, combined shadow+locked description
- **TUI inventory:** Rust unit tests verifying `ListItem::inventory()` construction, toggle guard rejection in app.rs, and session-level rejection of inventory `ItemId` variants (not just UI-level guard)
- **/var ownership:** Rust unit tests for: (1) `stat` output parsing (mode, ownership — both name and numeric), (2) `render_tmpfiles_conf()` output format (separate-line comments, no inline comments, correct field ordering), (3) staged artifact path is `config/usr/lib/tmpfiles.d/inspectah-var.conf`, (4) non-default modes (0700, 2770, 1777 sticky), (5) ownership resolution: packaged account (name used), non-system account (name used), unresolvable account (numeric fallback), root-owned (root root), (6) graceful fallback to RUN mkdir when ownership/mode is `None`, (7) stat failure produces degradation warning with `None` fields, (8) file not generated when no unbacked dirs have ownership data
- **ifcfg:** Rust unit test for adapter wiring, React component test for Alert banner, verify banner text matches `IFCFG_DEPRECATION_NOTE` constant (single source of truth)
- **Batch toggle:** Rust handler tests for triage groups only (verify reference-only groups have no registered routes), TypeScript client tests, Playwright test for packages confirmation dialog, partial-success announcement test
- **Batch toggle zero-actionable:** Handler/session test verifying groups with zero actionable items return appropriate response (not silently succeed)
