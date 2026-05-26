# Section Promotion & Triage Model Redesign

**Date:** 2026-05-25
**Status:** Proposed
**Scope:** Triage model replacement (all sections) + Tier 1 actionable promotion (services, quadlets, flatpak provisioning, sysctls, tuned) + explicit deferral of compose rendering

---

## Problem

inspectah scans 12+ system dimensions but only gives users triage control
over four: packages, config files, repos, and users/groups. Everything else
is either silently baked into the Containerfile or shown as read-only
context. This creates two gaps:

1. **Silent inclusion.** The Containerfile renderer already acts on
   services, sysctls, tuned profiles, quadlet units, and more. Users
   cannot exclude items they do not want.

2. **Wrong triage lens.** The current attention system
   (NeedsReview / Informational / Routine) answers "how confident is the
   tool?" when users need "what do I do?" The levels are
   implementation-oriented, not action-oriented. "Informational" is a dead
   zone — it means "we don't know" which is not actionable.

## Solution

Three coupled changes ship together:

1. **Replace the triage model** with action-oriented buckets across all
   sections (existing and new).
2. **Promote Tier 1 actionable surfaces** that already have a truthful
   render/materialization contract: services, quadlets, flatpak
   provisioning, sysctls, and tuned.
3. **Add an explicit ownership/pruning contract** between promoted
   sections and the generic config carry-forward path so excluded promoted
   artifacts cannot leak back in through `COPY config/...`.

These are inseparable. The new triage model is the foundation the promoted
sections need, and the ownership contract is what keeps those promotions
honest. Shipping promotion into the old model or without config pruning
would create tech debt by design.

---

## Triage Model

### Single-Host: Baseline / Site / Investigate

All items default enabled. Grouping is informational — it helps users
orient, not gatekeep.

| Bucket | Meaning | User action | Default |
|--------|---------|-------------|---------|
| **Baseline** | Matches base image state. Stock content. | Skim and confirm. | Enabled |
| **Site** | User customization. Non-default state. | Review — this is what you're migrating. | Enabled |
| **Investigate** | Can't classify. Unknown origin or no baseline data. | Verify before deciding. | Enabled |

Classification is always against the **actual base image**, not RPM
metadata defaults. RHEL base images modify service states and tuned
profiles during build. The base image IS the authority.

For promoted renderable sections, **Baseline is confirm-only**. Baseline
rows stay visible and included so users can verify them, but they generate
no materialized files or Containerfile output unless another
bucket/variant-selection says the base image is no longer sufficient.

### Fleet: Investigate / Divergent / Partial / Universal

Order is action-heavy at top. Items in Investigate and Divergent require
decisions. Partial and Universal are confirmations.

| Bucket | Meaning | User action | Default |
|--------|---------|-------------|---------|
| **Investigate** | Unknown origin, no baseline consensus. | Verify. | Enabled |
| **Divergent** | Hosts disagree on value/state. Includes config/content variants. | Pick a strategy. | Enabled (majority value) |
| **Partial** | Present on some hosts, not all. Below threshold. | Opt-in. | Disabled |
| **Universal** | 100% host agreement. | Skim and confirm. | Enabled |

"Divergent" is intentionally neutral — config variants across hosts are
often intentional (prod vs staging, regional tuning), not errors.

### Classification priority: Partial gates Divergent

An item present on some hosts (not all) AND with different content across
those hosts is **Partial**, not Divergent. The primary triage decision is
"do I even want this?" — content divergence is irrelevant until the user
opts it in. Once toggled on, the variant affordance (SelectVariant /
EditVariant) activates so the user can resolve the content divergence.

Priority order:
1. `prevalence < total` → **Partial** (regardless of content divergence)
2. `prevalence == total && content diverges` → **Divergent**
3. `prevalence == total && content agrees` → **Universal**
4. Unknown origin overrides all → **Investigate**

The variant system is an item-level feature, not a bucket-level one. Any
item with content variants can access SelectVariant/EditVariant regardless
of which bucket it's in.

### Divergent review tracking

Fleet `Divergent` items default to the majority value so the preview can
stay concrete, but they are not considered reviewed until the operator
interacts with them.

Review state is tracked in the **session layer** as a
`HashSet<ItemId>` of confirmed items — not in the triage struct. Triage
is computed from data; review state is user interaction state. Mixing them
creates staleness bugs on re-scan.

An item enters the confirmed set when the operator:
- explicitly toggles it (include or exclude), or
- selects a variant via `SelectVariant`

No new `RefinementOp` is needed. The existing toggle and variant ops
implicitly confirm. The UI shows "N unconfirmed" in the Divergent status
bar chip. Progress treats only confirmed and excluded items as resolved.

### TriageReason and ValidationSignal

Every classified item carries one **typed primary reason** explaining why
it landed in a bucket, plus zero or more **typed triage annotations** that
add secondary warnings without changing the bucket.

Examples:

| Type | Examples |
|------|----------|
| Primary reasons | `PackageNoRepoSource`, `ConfigModified`, `ServiceNonDefaultState`, `QuadletPresentInBaseImage`, `FlatpakProvisionedOnFirstBoot`, `SysctlFileBackedOverride`, `TunedCustomProfile` |
| Triage annotations | `SensitivePath`, `FirstBootProvisioned`, `RequiresProjectedPackage { name: "tuned" }`, `RuntimeOnlyObservation` |

Reasons stay closed enums in Rust/serde. User-facing strings come from a
formatter layer. This keeps the wire contract typed while still letting the
UI say "Security-sensitive path — verify before including" instead of enum
names.

---

## Type System Changes

### Core types (inspectah-refine)

Replace `AttentionLevel` with explicit single-host and fleet bucket types.
They do not map 1:1 — `Baseline` has no fleet equivalent, `Divergent` has
no single-host equivalent. Keep that truth in the types rather than
smearing it across runtime checks.

```rust
enum TriageBucket {
    Baseline,
    Site,
    Investigate,
}

enum FleetBucket {
    Investigate,
    Divergent,
    Partial,
    Universal,
}

struct Prevalence {
    count: u32,
    total: u32,
}

struct FleetTriage {
    bucket: FleetBucket,
    prevalence: Prevalence,
}

enum Triage {
    SingleHost(TriageBucket),
    Fleet(FleetTriage),
}

enum TriageAnnotation {
    SensitivePath,
    FirstBootProvisioned,
    RequiresProjectedPackage { name: String },
    RuntimeOnlyObservation,
}

struct TriageTag {
    triage: Triage,
    primary_reason: TriageReason,
    annotations: Vec<TriageAnnotation>,
}
```

`SensitivePath` moves from "promote to another bucket" to a validation
signal layered on top of the primary classification. This preserves the
current overlay behavior without forcing a second fake bucket.

### RefinementOp collapse

Replace per-section Exclude/Include variants with a generic `SetInclude`:

```rust
enum RefinementOp {
    SetInclude { item_id: ItemId, include: bool },

    // Non-trivial payloads remain
    UserStrategy { username: String, strategy: UserContainerfileStrategy },
    UserPassword(UserPasswordOp),
    SelectVariant { item_id: ItemId, target: ContentHash },
    EditVariant { item_id: ItemId, content: String, based_on: Option<ContentHash> },
    DiscardVariant { item_id: ItemId, variant: ContentHash },
}
```

`SetInclude` is the transport op for independent or bundled decision
items. After replay, session validation still enforces section invariants:

- included tuned selection requires its bundled custom profile content
- included tuned selection requires the projected `tuned` package
- sysctl output is generated from selected keys, not by copying original
  source files wholesale

Divergent review tracking uses a session-layer `HashSet<ItemId>`, not a
`RefinementOp`. Any `SetInclude` or `SelectVariant` touching a Divergent
item implicitly adds it to the confirmed set.

Fix `ItemId::Package` from `{ name_arch: String }` to `{ name: String,
arch: String }` to eliminate the lossy concatenation round-trip.

### ItemId additions

```rust
enum ItemId {
    Package { name: String, arch: String },
    Config { path: String },
    Repo { section_id: String },
    User { username: String },

    // Promoted:
    Service { unit: String },
    ServiceDropIn { unit: String, dropin_path: String },
    Quadlet { path: String },
    Flatpak { app_id: String, remote: String, branch: String },
    Sysctl { key: String },
    TunedSelection { profile: String },

    // Existing context-only / select-only:
    Compose { path: String },
    Fstab { mount_point: String },
    NonRpm { name: String },
}
```

`Repo` keeps `section_id` as the canonical identity so old repo ops can be
rewritten without ambiguity. `Flatpak` uses `(app_id, remote, branch)` as
the identity key — the minimal tuple that uniquely identifies a flatpak
across hosts. `remote_url` is render metadata on the item struct, not part
of identity (two hosts can name the same remote differently or point the
same name at different URLs). `Compose` stays context-only/select-only in
this spec.

### ChangesSummary and RefineStats

Use ordered section summaries, not `HashMap`s. Empty sections and stable
section ordering are part of the UI contract.

```rust
enum SectionKind {
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

struct SectionStats {
    kind: SectionKind,
    total: usize,
    included: usize,
    excluded: usize,
}

struct SectionChangeSummary {
    kind: SectionKind,
    included: Vec<ItemId>,
    excluded: Vec<ItemId>,
}

struct ChangesSummary {
    sections: Vec<SectionChangeSummary>,
    variants_changed: usize,
    is_dirty: bool,
}

struct RefineStats {
    sections: Vec<SectionStats>,
}
```

### Frontend type alignment

The TypeScript union simplifies to match:

```typescript
type RefinementOp =
  | { op: "SetInclude"; target: { item_id: ItemId; include: boolean } }
  | { op: "UserStrategy"; target: { username: string; strategy: string } }
  | { op: "UserPassword"; target: UserPasswordOp }
  | { op: "SelectVariant"; target: { item_id: ItemId; target: string } }
  | { op: "EditVariant"; target: { item_id: ItemId; content: string; based_on: string | null } }
  | { op: "DiscardVariant"; target: { item_id: ItemId; variant: string } };
```

`buildToggleOp()` collapses to one pattern. Divergent review tracking is
client-side state — when a Divergent item receives a `SetInclude` or
`SelectVariant`, the frontend adds it to a confirmed set and updates the
status bar chip count.

Fleet DTOs also carry `prevalence: { count, total }` and
`review_state: "defaulted" | "confirmed"` only on the `Divergent` fleet
variant.

## Ownership and Invariants

Promotion only ships if each promoted surface has one renderer authority and
one pruning rule:

| Surface | Actionable item | Materialized artifact | Renderer authority | Pruning / invariant |
|---------|-----------------|-----------------------|--------------------|---------------------|
| Service state | `Service { unit }` | no file; `RUN systemctl ...` only for non-Baseline rows | Services renderer | independent from drop-in carry-forward; Baseline rows are no-output |
| Service drop-in | `ServiceDropIn { unit, dropin_path }` | `drop-ins/etc/systemd/system/...` | Services renderer | generic config section never copies these paths |
| Quadlet | `Quadlet { path }` | `quadlet/<unit>` for non-Baseline rows only | Containers renderer | generic config section never copies quadlet paths; Baseline rows are no-output |
| Flatpak | `Flatpak { app_id, remote, branch }` + `remote_url` as render metadata | `flatpak-install.json` + `flatpak-provision.service` | Containers renderer | always flagged `FirstBootProvisioned`; URL divergence detected by content comparison, resolvable via variant selection |
| Sysctl | `Sysctl { key }` | synthesized `sysctl/etc/sysctl.d/99-inspectah-migrated.conf` | Sysctl renderer | generic config section never copies original sysctl source files |
| Tuned | `TunedSelection { profile }` | `tuned/etc/tuned/<name>/` + activation lines | Tuned renderer | generic config section never copies bundled tuned paths; active custom profile bundles its files; included tuned requires projected `tuned` package |
| Compose | `Compose { path }` | none in this spec | reference only | deferred until render contract exists |

---

## Promoted Sections

### Implementation sequence

Per Ember's recommendation, ship together but implement sequentially:

1. **Services** — highest post-migration failure rate and the first place
   the ownership/pruning contract has to be proven.
2. **Quadlets + Flatpak provisioning** — same sidebar section, but separate
   lifecycle semantics.
3. **Sysctls + Tuned** — promotion plus kernel/tuned ownership pruning.

### 1. Services

**Items:** `ServiceStateChange` (unit name, state, include) and
`SystemdDropIn` (path, include).

**Classification (single-host):**
- **Baseline:** Service state matches base image. Package-installable
  services where the state is the base image's default (compared against
  actual base image, not RPM scriptlet defaults).
- **Site:** Non-default service state — explicitly enabled, disabled, or
  masked. Local drop-in overrides are always Site.
- **Investigate:** Service not from any installed RPM, no baseline
  available, or drop-in content cannot be tied back to a valid unit.

**Classification (fleet):**
- **Universal:** All hosts agree on service state or drop-in content.
- **Divergent:** Hosts disagree (e.g., `firewalld enabled 42/50,
  disabled 8/50`, or same drop-in path with different content).
- **Partial:** Service or drop-in present on only some hosts. Default
  excluded.
- **Investigate:** Unknown origin across fleet.

**Masked vs disabled:** Surface the distinction in the UI. A masked
service cannot be started even manually — this is a meaningful difference
from disabled.

**Drop-in cascade rule:** Drop-ins are independently toggleable but depend
on their parent service. Excluding a service auto-excludes its drop-ins.
A drop-in CAN be excluded without excluding the parent. A drop-in CANNOT
be included if its parent service is excluded. A drop-in without its
service is semantically meaningless in systemd.

**Containerfile output:**
- Included service-state rows emit `RUN systemctl enable/disable/mask
  <unit>` only when the row is not `Baseline`.
- Included drop-ins materialize under
  `drop-ins/etc/systemd/system/<unit>.d/` and are copied with a dedicated
  `COPY drop-ins/etc/systemd/system/ /etc/systemd/system/`.
- The generic config section MUST stop materializing these paths.

**Default include:** All enabled. Services were already being rendered
silently — promotion gives users control, not new content.

### 2. Container Workloads

#### Quadlets

**Items:** `QuadletUnit`.

**Classification (single-host):**
- **Baseline:** Quadlet already exists in the actual base image or vendor
  path and matches target-image content.
- **Site:** Local/admin quadlet workload under `/etc`.
- **Investigate:** Unusual origin, unreadable content, or no trustworthy
  baseline comparison.

**Classification (fleet):**
- **Universal:** Same workload, same content, all hosts.
- **Divergent:** Same container exists but content differs. Activates
  the variant system (VariantSelection: Only/Selected/Alternative).
- **Partial:** Workload only on some hosts. Default excluded.
- **Investigate:** Unusual origin.

**Containerfile output:** `COPY quadlet/ /etc/containers/systemd/` only for
non-`Baseline` rows. Baseline/vendor matches stay included for review but do
not get re-materialized into `/etc`.

**Default include:** All enabled.

#### Flatpak Apps

**Items:** `FlatpakApp`, keyed by `(app_id, remote, branch)`. `remote_url`
is render metadata on the item struct — it determines the generated
first-boot manifest content but is not part of the identity key.

**Classification (single-host):**
- **Site:** Installed app reconstructed into a first-boot provisioning
  manifest.
- **Investigate:** Missing remote/branch/origin data needed to reconstruct
  a deterministic manifest.

Flatpaks are not image-baked workload content. They always carry the
`FirstBootProvisioned` triage annotation.

**Classification (fleet):**
- **Universal:** Same `(app_id, remote, branch)` identity and same
  `remote_url` across all hosts.
- **Divergent:** Same app ID but different remote/branch/URL tuple across
  hosts.
- **Partial:** Present on some hosts, not all. Default excluded.
- **Investigate:** Incomplete provenance for manifest reconstruction.

**Containerfile output:** write `flatpak/flatpak-install.json`, copy
`flatpak-provision.service`, and `RUN systemctl enable
flatpak-provision.service`.

**Default include:** All enabled, but the row and preview always label this
as first-boot provisioning rather than baked image content.

#### Compose (deferred in this spec)

`ComposeFile` stays reference-only / variant-select-only in this spec. It
does NOT gain include/exclude because inspectah does not yet have a
truthful compose render/materialization contract. Existing variant display
and searchable text stay in place, but promotion waits for a follow-on
spec.

### 3. Sysctls

**Items:** file-backed `SysctlOverride` entries only.

**Actionable set:** entries with a non-empty `source` under
`/etc/sysctl.d/` or `/etc/sysctl.conf`. Runtime-only observations stay as
reference / investigate signals; they are not promoted into a renderer
contract.

**Classification (single-host):**
- **Baseline:** File-backed value matches the base image's effective value.
- **Site:** File-backed non-default override.
- **Investigate:** No baseline to compare, can't classify.

**Classification (fleet):**
- **Universal:** All hosts agree on value.
- **Divergent:** Hosts have different values for the same key. Activates
  variant display with human-readable values (not content hashes).
- **Partial:** Override only on some hosts.
- **Investigate:** No baseline consensus.

**Guardrail:** a small deny list can still block obviously runtime-only
keys, but the primary gate is "file-backed and renderable", not the deny
list itself.

**Containerfile output:** synthesize one merged file at
`sysctl/etc/sysctl.d/99-inspectah-migrated.conf` containing only the
included keys, then `COPY` that file into `/etc/sysctl.d/`. The generic
config section MUST stop copying original sysctl source files. This avoids
shared-source leakage where excluding one key would otherwise reintroduce
it via `COPY config/etc/...`.

**Default include:** All enabled. Sysctl overrides are explicit operator
tuning — always intentional.

### 4. Tuned Profiles

**Items:** one bundled `TunedSelection` representing the active profile. If
the active profile is custom, its backing `/etc/tuned/<name>/` files travel
as bundled payload for that one decision item rather than as independent
toggle rows.

**Classification (single-host):**
- **Baseline:** Active profile matches the base image's tuned state. The
  base image is the authority — a custom image with
  `throughput-performance` as default means that profile is Baseline, not
  Site.
- **Site:** Non-default active profile. Custom active profiles are Site.
- **Investigate:** Tuned active but package not installed, custom profile
  selected but bundled files are missing, or other unusual state.

**Classification (fleet):**
- **Universal:** All hosts same profile.
- **Divergent:** Hosts run different profiles, or same custom profile name
  with different bundled content.
- **Partial:** Tuned active on some hosts, not on others.
- **Investigate:** Unusual state.

**Stock profiles:** `balanced`, `throughput-performance`,
`latency-performance`, `virtual-guest`, `powersave`,
`network-latency`, `network-throughput`, `cpu-partitioning`,
`hpc-compute`, `oracle`, `mssql`, `sap-hana`, `desktop`, etc. These
ship with the `tuned` RPM. A static list is reasonable for display
purposes.

**Package prerequisite:** included tuned selection requires the projected
`tuned` package. If the package is absent from the projected image, surface
`RequiresProjectedPackage { name: "tuned" }`, keep the item unresolved, and
omit tuned output from the preview until the prerequisite is satisfied.

**Containerfile output:** file-write approach (validated in Rust renderer,
correct for container builds):
```dockerfile
RUN echo "throughput-performance" > /etc/tuned/active_profile
RUN echo "manual" > /etc/tuned/profile_mode
RUN systemctl enable tuned.service
```
`tuned-adm profile` is NOT correct for container builds — it requires a
running tuned daemon (D-Bus), which does not exist during `podman build`.
If the active profile is custom, copy its bundled files from
`tuned/etc/tuned/<name>/` before setting the active profile.

**Default include:** Enabled, with one exception: if the active profile
matches the base image's tuned state, it is Baseline and included but
generates no Containerfile output (it's already the default).

---

## UI Design

### Page Hierarchy

One sidebar section is active at a time. The page hierarchy is:

`Sidebar section -> bucket group -> decision row -> detail region`

Only one sidebar section is mounted in the main content pane at a time.
Within `Containers`, actionable subtypes (`Quadlets`, `Flatpak`) render in
bucket groups and `Compose` remains a read-only reference subsection at the
bottom of that same pane.

### Layout: Collapsible Buckets with Smart Defaults

Within the active section, triage buckets render as collapsible groups.
Smart expand defaults encode the workflow into the layout.

**Fleet mode:**
- **Investigate:** Expanded (highest uncertainty, fewest items).
- **Divergent:** Expanded (needs strategy decisions).
- **Partial:** Collapsed. Summary: "N items, all excluded."
- **Universal:** Collapsed. Summary: "N items, all included."

**Single-host mode:**
- **Investigate:** Expanded (if any items).
- **Site:** Expanded (the interesting stuff).
- **Baseline:** Collapsed. Summary: "N items, all included."

**Sections with fewer than 3 items default expanded** regardless of
bucket — a collapsed section with 1 item is just a click tax.

**Empty buckets:** Show the header with "(0)" and disable expand. The zero
is information — "nothing to investigate" is a positive triage signal.

### Divergent Row States

`Divergent` rows surface three explicit states:

- **Defaulted:** majority value preselected, not yet reviewed
- **Confirmed:** operator confirmed the default or chose a variant
- **Excluded:** operator chose not to carry the item

Progress treats only **Confirmed** and **Excluded** as resolved. A checked
row is not automatically complete if it is still `Defaulted`.

### Status Bar

Compact chip bar above the active section for instant orientation:

```
Investigate: 1 | Divergent: 3 (2 unconfirmed) | Partial: 4 | Universal: 12
```

Investigate and Divergent counts use attention colors (red, orange).
Partial and Universal use muted styling. Chips are passive orientation
labels — they do not filter. The collapsible bucket sections below are the
focus mechanism (expand what you're working on, collapse what you're not).

### Visual Treatment

- **Left border:** Color-coded per bucket for quick scanning.
  - Investigate: red
  - Divergent: orange
  - Partial: gray
  - Universal: accent/blue
- **Collapsed summary:** Semantic, not just counts. "12 items, all
  included" / "4 items, all excluded" / "3 items, 1 excluded."
- **Reason lines:** Every item shows a user-facing reason below the
  title explaining why it's in its bucket.

### DecisionItem Reuse

All promoted decision surfaces use the existing `DecisionItem` row
component.
The toggle interaction, keyboard model, attention badge slot, and
expand/collapse affordance are identical regardless of section type.

`ContextItem` (read-only, no toggle) remains for sections that stay as
reference: network, storage, SELinux modules/fcontext, audit rules, and
compose in this spec.

### Parent-Child Toggle (Services + Drop-ins)

Drop-ins render as indented child rows (12-16px additional left padding)
with a thin left border connecting them visually to the parent service.
This is a shallow tree (max depth 1) — no full tree widget needed.

Drop-ins are independently toggleable but with a symmetric cascade:
- Excluding a service auto-excludes its drop-ins.
- Re-including a service auto-re-includes its drop-ins.
- A drop-in CAN be excluded without excluding its parent service.
- A drop-in CANNOT be included if its parent service is excluded.
- No confirmation modal — the cascade is the obvious default. If the user
  wants the service without a specific drop-in, they re-enable (which
  re-enables all), then exclude the unwanted drop-in.
- The Containerfile renderer enforces: if a service is excluded, its
  drop-in files are not COPYd regardless of individual include state.

When a parent service is excluded:
- Drop-in checkboxes become disabled.
- Drop-in rows render at `opacity: 0.55` (matching ExcludedZone treatment).
- A "Service excluded" badge appears in the badge slot.
- Attempting to check a disabled drop-in is a no-op (no toast, no modal).

When a drop-in is independently excluded (parent still included):
- Drop-in moves to ExcludedZone treatment.
- Parent service is unaffected.

**Keyboard:** Tab order is parent then children (flat within section).
Space on a disabled drop-in is a no-op. No focus trap.

### Lifecycle Badges (Containers)

Rows in `Containers` get both a subtype badge and a lifecycle cue:

- `Quadlet` + `Image content`
- `Flatpak` + `First boot`
- `Compose` + `Reference only`

This makes the trust model visible instead of implying that all container
artifacts share one execution path.

### Fleet Value Display (Sysctls)

Reuses the existing variant system. Summary row shows majority value
inline with prevalence badge and variant button:

```
vm.swappiness = 10    [45/50 hosts]  [2 variants]
```

Expanding shows per-variant host lists with human-readable values:
`10 (45 hosts)` vs `60 (5 hosts)`. Content hashes are not shown for
sysctl values since they are short scalars.

### Package Section: Repo-First Exception

Packages are the one section where triage buckets are **secondary**, not
primary. The existing RepoBar / RepoGroup layout stays as the primary
grouping — packages are organized by source repository (baseos,
appstream, third-party, @commandline, etc.).

Repo provenance already encodes the triage signal naturally:
- Distro repos (baseos, appstream) → roughly Baseline
- Third-party / official-optional repos → roughly Site
- @commandline / no-repo → roughly Investigate

Each package still gets a `TriageTag` with bucket and reason, surfaced
as a badge on the package row. Users can filter by triage bucket within
the repo view. The status-bar chips stay visible in Packages, but they
filter rows within repo groups rather than replacing repo-first layout.
The visual grouping stays repo-first because repo IS the provenance signal
that users already rely on.

The new sections (services, containers, sysctls, tuned) don't have a
repo-equivalent grouping axis, so they use the bucket layout natively.

This is a deliberate asymmetry, not an inconsistency. The triage model
applies everywhere — the visual grouping adapts to each section's
natural organizing axis.

### ARIA Contract

- Status-bar chips are passive labels (not interactive buttons).
- Bucket headers are disclosure buttons and announce count + state, e.g.
  "Investigate, 3 items, expanded."
- Reason text binds to the row via `aria-describedby`; bucket meaning
  cannot rely on color or left border alone.
- Badge announcement order is: lifecycle/type, triage annotation, primary
  reason.
- If an action removes the currently focused row, move focus to the nearest
  visible sibling or the bucket header.

### Section Navigation

The sidebar has two groups: **Review** (toggleable sections) and
**Reference** (read-only context). Promotion moves services, containers,
sysctls, and tuned from Reference to Review:

**Review** (after promotion):
- Packages, Config Files, Users & Groups *(existing)*
- Services, Containers, Sysctls, Tuned Profiles *(promoted)*

**Reference** (remaining):
- Version Changes, Network, Storage, SELinux, Kernel/Boot,
  Scheduled Tasks, Non-RPM Software

`Containers` is in Review because it now contains actionable `Quadlet` and
`Flatpak` surfaces, but the pane also retains a `Compose (reference)`
subsection until compose rendering is designed.

In code: items move from the `CONTEXT_SECTIONS` array to
`DECISION_SECTIONS`. Badge counts on Review sections show actionable item
counts.

---

## Deferred Work

### Tier 2 Sections (next spec)

| Section | Key complexity | Estimated effort |
|---------|---------------|-----------------|
| Compose rendering + promotion | No truthful materialization contract yet | Medium-High |
| Scheduled tasks (cron + timers) | RPM-owned vs user-created filtering | Medium |
| SELinux booleans | JSON value dedup (not prevalence-based) | Medium |
| Boot parameters (kargs) | Cmdline decomposition for per-arg prevalence | Medium |

### Tier 3 Sections (future)

| Section | Key complexity |
|---------|---------------|
| Non-RPM software | `review_status` → `include` model redesign |
| SELinux custom modules | Complex policy migration, needs own design |

### Content Editability (future)

This spec covers include/exclude plus divergent-review confirmation only. A
future extension adds real content editing — changing a service state
(enabled → masked), switching a tuned profile, modifying a sysctl value.
That future model should stay typed:

```rust
enum ValueOp {
    SetServiceState { unit: String, state: ServiceUnitState },
    SetTunedProfile { profile: TunedProfileRef },
}
```

Services and tuned profiles are natural first candidates for editability
because their edit space is constrained (known set of states/profiles).
Sections with unconstrained edit spaces (configs, sysctls) are harder and
should not be attempted without a dedicated design effort.

### Sections That Stay as Reference

These sections remain as read-only `ContextItem` display:

- **Network connections** — point-in-time runtime state, no Containerfile
  representation.
- **Storage (fstab, LVM)** — environment-specific, no meaningful image
  representation.
- **SELinux custom modules / fcontext** — complex policy migration, not
  ready for automated handling.
- **SELinux audit rules** — security policy, not item-level triage.
- **Kernel modules** — loaded by applications, not manually. Config files
  that trigger loading are already in the config section.
- **Runtime-only sysctl observations** — not a stable image-build contract.
- **Unused custom tuned profiles** — shown as context, not independent
  toggle rows in v1.

### Users/Groups Fleet UI

Users/groups is not a promotion — the feature exists in single-host mode
and needs fleet exposure. Should be sequenced alongside or just after
Tier 1 promotions. Separate spec.

### Validation Matrix

| Surface | Proof required |
|---------|----------------|
| Service drop-ins | excluded drop-in absent from both `drop-ins/` and generic config output |
| Baseline service state | included Baseline row generates no `systemctl` line |
| Baseline quadlet | included Baseline row generates no `quadlet/` materialization |
| Sysctl shared source | excluding one key from a shared source file does not leak the original file |
| Tuned bundle | custom active profile carries bundled files and blocks on missing `tuned` package |
| Flatpak fleet identity | remote URL divergence changes classification and generated manifest truth |
| Autosave migration | v1 journal with active + inactive ops and non-terminal cursor preserves replay semantics after v2 rewrite |

---

## Migration Notes

### Backward Compatibility

- **Autosave format:** bump `schema_version` from `1` to `2`. The loader
  must accept the old tagged op enum, preserve active/inactive ops plus
  `cursor`, and rewrite the journal before session validation:
  - `ExcludePackage { name, arch }` →
    `SetInclude { item_id: ItemId::Package { name, arch }, include: false }`
  - equivalent rewrites for config include/exclude ops
  - `ExcludeRepo { section_id }` →
    `SetInclude { item_id: ItemId::Repo { section_id }, include: false }`
  - `IncludeRepo { section_id }` →
    `SetInclude { item_id: ItemId::Repo { section_id }, include: true }`
  - preserve undo/redo semantics, not just final projected state
- **Snapshot contract:** keep `inspection-snapshot.json` raw fields
  unchanged in this spec. `kernel_boot.sysctl_overrides`,
  `kernel_boot.tuned_active`, and `kernel_boot.tuned_custom_profiles`
  remain the durable snapshot truth; promoted sysctl/tuned decision rows are
  synthesized at the refine/view layer. `containers.compose_files` also
  stays unchanged while compose remains reference-only.
- **API contract:** The `/api/refine` endpoint response changes shape.
  Frontend and backend must be updated together — this is a single binary,
  so no version skew risk.

### Existing Section Reclassification

Packages, configs, repos, and users/groups are reclassified under the new
triage model. The classification logic is reviewed and updated, not just
renamed:

- **Packages:** `PackageBaselineMatch` → Baseline. `PackageUserAdded` →
  Site (was Routine). `PackageLocalInstall` → Investigate (was
  NeedsReview). `PackageProvenanceUnavailable` → Investigate (was
  Informational).
- **Configs:** `ConfigDefault` → Baseline. `ConfigModified` → Site (was
  NeedsReview — but "modified from RPM default" is a site customization,
  not an investigation). `ConfigUnowned` → Site (user-created files are
  customizations). `SensitivePath` becomes a triage annotation layered on
  top of the primary bucket, not a separate primary reason.
- **Repos:** Enabled/disabled toggle is already action-oriented.
  Reclassify per the new model.
- **Users/groups:** Already action-oriented. Reclassify per the new model.

Note: `ConfigModified` moving from NeedsReview to Site is a deliberate
reclassification. A modified RPM-owned config file is a site
customization the user made intentionally. It does not need "review" —
it needs to be carried forward. The old classification treated user work
as suspicious. `SensitivePath` still surfaces security-sensitive modified
configs (e.g., `/etc/ssh/sshd_config`) as a triage annotation, but it does
not rewrite their primary bucket.

---

## Success Criteria

1. Users can include/exclude services, quadlets, flatpak provisioning
   entries, file-backed sysctls, and tuned selections in both single-host
   and fleet modes. Compose remains reference-only in this spec.
2. The triage model (Baseline/Site/Investigate for single-host,
   Investigate/Divergent/Partial/Universal for fleet) applies to ALL
   sections — existing and new.
3. Excluded promoted artifacts are absent from both their dedicated
   materialized roots and the generic config tree. No promoted item leaks
   back in through `COPY config/...`.
4. Fleet merge and UI carry `count + total` prevalence and explicit
   divergent review state (`defaulted` vs `confirmed`) end to end.
5. Autosave migration preserves op history, active/inactive ops, cursor,
   projected state, and undo/redo behavior for old sessions.
6. Snapshot import/export remains backward-compatible; promoted sysctl and
   tuned review surfaces are synthesized from existing `kernel_boot` fields
   rather than requiring immediate snapshot schema churn.
7. No tree widget or modal workflow is required. Grouped service rows,
   bucket groups, and divergent-review confirmation all fit the existing
   row/detail model.
