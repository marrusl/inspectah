# Service Intent Inference

> Replace state filtering with intent inference so the Containerfile
> renderer only emits service instructions for deliberate user choices.

**Status:** Proposed (revision 2)  
**Created:** 2026-05-19  
**Area:** inspectah-collect, inspectah-pipeline, inspectah-web

> **Revision 2** (2026-05-19): Addresses round 1 review findings.
> `default_state` is now `Option<ServiceUnitState>` — masked units
> with no preset rule carry `None`, not a fabricated `Disabled`.
> Missing-package suppression uses `packages_added ∪
> baseline_package_names` as the truthful target-image package set.
> Silent-drop policy narrowed: known inert states drop silently,
> runtime/unknown states emit warnings. Renderer omissions surface
> explicitly in Containerfile comments and refine UI. Warning payloads
> carry structured `unit` + `raw_state` keys. Test coverage expanded
> for all new branches.

## Problem

The Containerfile renderer emits `systemctl disable` commands for
services the user never touched. From a real CentOS 9 scan:

```dockerfile
RUN systemctl disable \
    dbus.service \
    sssd-autofs.service \
    sssd-kcm.service \
    sssd-nss.service \
    sssd-pac.service \
    sssd-pam.service \
    sssd-ssh.service \
    sssd-sudo.service \
    systemd-remount-fs.service \
    systemd-sysupdate-reboot.service \
    systemd-sysupdate.service
```

**Root cause:** The collector treats `alias`, `indirect`, and
`enabled-runtime` as real service states and compares them against
systemd presets. Since presets only define `enable` or `disable`,
these non-actionable states always appear as "divergent" and produce
false `state_changes` entries.

From the latest snapshot:

- `dbus.service` — state `alias` (symlink to `dbus-broker.service`)
- `sssd-autofs.service` etc. — state `indirect` (pulled in via
  `sssd.service`'s `Also=` directive)
- `systemd-remount-fs.service` — state `enabled-runtime` (transient)
- `systemd-sysupdate-*.service` — state `indirect`

Meanwhile, `sssd.service` itself correctly appears in
`preset_matched_units` — the parent is fine, the children leak through.

### Additional failure mode

**Pointless disables for missing packages.** If sssd isn't in the
base image and isn't being installed via `RUN dnf install`, there's
no `sssd-*.service` to disable. The commands either fail silently or
do nothing. Conversely, if the base image drops a package in a future
release, suppressing the orphaned disable prevents stale symlinks
in the Containerfile.

## Design

### Principle: intent inference, not filtering

Only `enabled`, `disabled`, and `masked` represent durable user intent.
Everything else is a side effect, transient state, or packaging
artifact. The collector gates non-actionable states at parse time.

**State classification:**

| State | User intent? | Collector action |
|-------|-------------|------------------|
| `enabled` | Yes | Compare against presets |
| `disabled` | Yes | Compare against presets |
| `masked` | Yes | Always emit (deliberate suppression) |
| `static` | No | Silent drop (already handled) |
| `alias` | No | Silent drop (packaging artifact) |
| `indirect` | No | Silent drop (pulled in by parent) |
| `generated` | No | Silent drop (synthesized by generator) |
| `enabled-runtime` | No | Warning (transient runtime enablement) |
| `masked-runtime` | No | Warning (transient runtime mask) |
| `transient` | No | Warning (runtime-only unit) |
| `linked` | Partially | Warning (requires manual handling) |
| `linked-runtime` | No | Warning (transient linked unit) |
| `bad` | No | Warning (unit file has errors) |
| *(unrecognized)* | Unknown | Warning (unknown state) |

**Silent drop vs. warning distinction:** Known inert states whose
meaning is well-understood and which never carry migration-relevant
information drop silently. Runtime-only states, error states, and any
unrecognized state string emit warnings — these are either edge cases
that need human attention or future systemd states we haven't
classified yet. The warning ensures nothing disappears as a
successful-looking non-event.

### 1. Data Model

#### `ServiceUnitState` enum

Replaces the stringly-typed `current_state` field on
`ServiceStateChange`. Represents observed durable service state.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceUnitState {
    Enabled,
    Disabled,
    Masked,
}
```

Three variants only. Non-actionable states are gated at parse time
and never enter the data model.

#### Observed state vs. preset knowledge

The spec separates what was observed on the system (`current_state`)
from what the preset system says the default should be
(`default_state`). These are independent facts:

- `current_state: ServiceUnitState` — what `systemctl is-enabled`
  reported (always known for `state_changes` entries)
- `default_state: Option<ServiceUnitState>` — what the preset rule
  says the package default is. `None` means no matching preset rule
  was found.

The `Option` on `default_state` is load-bearing. A masked unit with
`default_state: None` means "the operator masked this, but we don't
know what the package default was." The UI renders this honestly as
`"masked (no preset rule)"` — not as a fabricated divergence from a
made-up default.

#### `ServiceAction` — derived, not stored

The action (`enable`, `disable`, `mask`) is derivable from
`current_state` and is not stored on `ServiceStateChange`. Replaced
by a method:

```rust
impl ServiceStateChange {
    /// Derives the systemctl action from current_state.
    /// Does not inspect default_state — purely current_state -> action.
    pub fn implied_action(&self) -> ServiceAction {
        match self.current_state {
            ServiceUnitState::Enabled => ServiceAction::Enable,
            ServiceUnitState::Disabled => ServiceAction::Disable,
            ServiceUnitState::Masked => ServiceAction::Mask,
        }
    }
}
```

`ServiceAction` is an internal enum used only by the renderer and
handler — not serialized to the snapshot:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceAction {
    Enable,
    Disable,
    Mask,
}
```

#### `ServiceStateChange` field changes

- `current_state`: `String` → `ServiceUnitState`
- `default_state`: `String` → `Option<ServiceUnitState>` (`None` =
  no preset rule found)
- `action`: removed (replaced by `implied_action()`)

#### `ServiceSection`

```rust
pub struct ServiceSection {
    pub state_changes: Vec<ServiceStateChange>,
    pub enabled_units: Vec<String>,
    pub disabled_units: Vec<String>,
    pub drop_ins: Vec<SystemdDropIn>,
    pub preset_matched_units: Vec<String>,
}
```

No `serde(default)` on any field. Old tarballs are re-scanned, not
migrated.

No `warnings` field — service warnings flow through the existing
`Warning` type on `InspectionSnapshot.warnings`.

#### Warning payload contract

Service warnings use the existing `Warning` type with documented
keys in the `extra` HashMap for stable programmatic access:

```rust
Warning {
    inspector: "services".into(),
    message: "unit foo.service has state 'linked' — requires manual handling".into(),
    severity: Some(WarningSeverity::Medium),
    extra: HashMap::from([
        ("unit".into(), json!("foo.service")),
        ("raw_state".into(), json!("linked")),
    ]),
}
```

The `unit` key provides stable identity for the UI without parsing
the message string. The `raw_state` key carries the raw `systemctl`
output for diagnostics.

#### Follow-up flag

Fields `include`, `owning_package`, `fleet`, `attention_reason` on
`ServiceStateChange` and `include`, `tie`, `tie_winner` on
`SystemdDropIn` are refinement-layer concerns populated downstream,
not at collection time. Separating them into a refinement overlay
type is a follow-up, not part of this spec.

### 2. Collector Logic

The collector's service loop in
`inspectah-collect/src/inspectors/services.rs` changes from "record
everything, let downstream filter" to "only record user intent."

**Parse-time gate.** When iterating `systemctl list-unit-files`
output, the collector matches each unit's state:

- `"enabled"` / `"disabled"` / `"masked"` — parse into
  `ServiceUnitState`, continue to preset comparison
- `"linked"` / `"bad"` — emit warning with `unit` + `raw_state`,
  skip from service data
- `"enabled-runtime"` / `"masked-runtime"` / `"transient"` /
  `"linked-runtime"` — emit warning (transient state, not migrated),
  skip from service data
- `"static"` / `"alias"` / `"indirect"` / `"generated"` — silently
  drop (known inert, no migration relevance)
- Any unrecognized state string — emit warning with `raw_state`,
  skip from service data (future-proofs against new systemd states)

**Preset comparison.** Unchanged logic, tighter types. For actionable
units, compare `ServiceUnitState` against the preset:

- **Divergent** (current state differs from preset) — push to
  `state_changes` with `current_state: ServiceUnitState` and
  `default_state: Some(preset_value)`
- **Matched** (current state matches preset) — push to
  `preset_matched_units`
- **No preset rule** — unit appears in `enabled_units`/`disabled_units`
  only (preset-unknown). No `ServiceStateChange` entry is created
  for `enabled` or `disabled` units without a preset rule.

**Masked units skip preset comparison entirely.** Masking is always
user intent regardless of what the preset says. They go straight into
`state_changes` with:

- `default_state: Some(preset_value)` if a preset rule exists
- `default_state: None` if no preset rule exists

The `None` case is honest — the spec does not fabricate a preset
value. The UI and renderer handle `None` explicitly.

### 3. Renderer Logic

The Containerfile renderer in
`inspectah-pipeline/src/render/containerfile.rs` gets three changes.

#### `implied_action()` replaces the stored `action` field

The renderer calls `sc.implied_action()` on each `ServiceStateChange`
to determine whether to emit `enable`, `disable`, or `mask`. The
method maps purely from `current_state` — it does not inspect
`default_state`.

#### Target-image package suppression

The renderer builds the target-image package set from two sources
already present on `InspectionSnapshot`:

- `snap.rpm.packages_added` — packages inspectah will install via
  `RUN dnf install`
- `snap.rpm.baseline_package_names` — packages already present in
  the base image

The union of these two lists represents all packages that will exist
in the target image. The renderer builds a `HashSet<&str>` from
`packages_added.iter().map(|p| p.name.as_str())` chained with
`baseline_package_names.iter().map(|s| s.as_str())`.

For each `state_change`:

- If `owning_package` is `Some(pkg)` and `pkg` IS in the target set
  → emit (the package will exist, the user's intent is preserved)
- If `owning_package` is `Some(pkg)` and `pkg` is NOT in the target
  set → omit and emit a Containerfile comment:
  `# Omitted: <unit> (package '<pkg>' not in target image)`
- If `owning_package` is `None` → emit (conservative — don't suppress
  what you can't verify)

**Degraded mode.** When `snap.rpm.no_baseline` is `true` (baseline
data unavailable), `baseline_package_names` is empty. In this case
the target set only contains `packages_added`, which may undercount.
The renderer emits all services whose `owning_package` is `Some(pkg)`
where `pkg` is not in `packages_added` — it does NOT suppress them,
because it cannot prove the package is absent from the target image.
Instead, it emits the service instruction with a comment:
`# NOTE: baseline unavailable — cannot verify '<pkg>' in target image`

**Suppress beats defer.** If a service would be deferred by
`config_tree_units` but its owning package isn't in the target set,
suppress it entirely. No point deferring a service whose package won't
be installed. Missing-package check runs before `config_tree_units`
deferral.

The `include` filter is unchanged — only `sc.include == true` entries
enter the renderer loop.

### 4. Refine UI Changes

The web handler's `normalize_services()` in
`inspectah-web/src/handlers.rs` already implements the three-way split
from the post-leaf fixes. Changes here are labeling improvements with
typed states and new visibility for omissions and warnings.

#### Subtitle labels

| Situation | Subtitle |
|-----------|----------|
| Divergent, preset known | `"{current_state} (diverges from preset: {default_state})"` |
| Divergent, preset unknown (`None`) | `"{current_state} (no preset rule)"` |
| Masked, preset known | `"masked (diverges from preset: {default_state})"` |
| Masked, preset unknown (`None`) | `"masked (no preset rule)"` |
| Preset-matched, has drop-in | `"{state} (matches preset, has drop-in override)"` |
| Preset-matched, no drop-in | Suppressed — not rendered |
| Preset-unknown (enabled/disabled lists only) | `"enabled (no preset rule)"` / `"disabled (no preset rule)"` |

The handler uses `implied_action()` and pattern matching on
`ServiceUnitState` variants instead of `match sc.action.as_str()`
string comparisons. When `default_state` is `None`, the subtitle
omits the preset reference entirely.

#### Omitted services subsection

When the renderer suppresses a service due to target-image package
absence, the refine UI surfaces this in an "Omitted Services"
subsection. Each omission is a `ContextItem`:

- `id`: unit name
- `title`: unit name
- `subtitle`: `"omitted (package '<pkg>' not in target image)"`
- `detail`: explanation of why the service was omitted

This makes renderer decisions visible — the user can see what was
excluded and why, rather than the Containerfile looking fully
authoritative with silently dropped instructions.

#### Service warnings subsection

`linked`, `bad`, runtime, and unrecognized-state warnings from
`snap.warnings` (filtered by `inspector == "services"`) render as a
"Service Warnings" subsection below the service items list. Each
warning is a `ContextItem`:

- `id`: `extra["unit"]` value (stable identity)
- `title`: unit name from `extra["unit"]`
- `subtitle`: derived from `extra["raw_state"]`, e.g.,
  `"linked (requires manual handling)"` or
  `"enabled-runtime (transient, not migrated)"`
- `detail`: the warning message from `Warning.message`

Warnings are not mixed with actionable services and are rendered in
a separate subsection.

Drop-in override handling is unchanged from the post-leaf fixes.

## Testing

### Collector

- Unit: `"enabled"` state with matching preset → in
  `preset_matched_units`, not in `state_changes`
- Unit: `"enabled"` state with divergent preset → in `state_changes`
  with typed `ServiceUnitState::Enabled` and
  `default_state: Some(ServiceUnitState::Disabled)`
- Unit: `"masked"` state with preset rule → in `state_changes` with
  `default_state: Some(preset_value)`
- Unit: `"masked"` state with no preset rule → in `state_changes`
  with `default_state: None`
- Unit: `"alias"` state → silently dropped, not in any list, no
  warning
- Unit: `"indirect"` state → silently dropped, no warning
- Unit: `"static"` state → silently dropped, no warning
- Unit: `"generated"` state → silently dropped, no warning
- Unit: `"enabled-runtime"` state → warning emitted with
  `extra["unit"]` and `extra["raw_state"]`, not in service data
- Unit: `"masked-runtime"` state → warning emitted, not in service
  data
- Unit: `"transient"` state → warning emitted, not in service data
- Unit: `"linked"` state → warning emitted with
  `inspector: "services"`, not in service data
- Unit: `"linked-runtime"` state → warning emitted, not in service
  data
- Unit: `"bad"` state → warning emitted, not in service data
- Unit: unrecognized state string (e.g., `"future-state"`) → warning
  emitted with `raw_state: "future-state"`, not in service data
- Unit: no matching preset rule, state `"enabled"` → in
  `enabled_units` only, no `ServiceStateChange`
- Integration: clean RHEL/CentOS install with no user service changes
  produces zero `state_changes` entries

### Renderer

- `implied_action()` returns correct `ServiceAction` for all three
  `ServiceUnitState` variants
- Service with `owning_package: Some("firewalld")` where firewalld
  IS in `baseline_package_names` but NOT in `packages_added` → emitted
  (base image package, user intent preserved)
- Service with `owning_package: Some("sssd")` where sssd is NOT in
  `packages_added` AND NOT in `baseline_package_names` → omitted with
  Containerfile comment
- Service with `owning_package: None` → emitted (conservative)
- Service that is both config-tree-deferred and missing-package →
  suppressed entirely (suppress beats defer)
- Degraded mode: `no_baseline == true`, service with
  `owning_package: Some("pkg")` not in `packages_added` → emitted
  with `# NOTE: baseline unavailable` comment
- Omitted services produce `# Omitted:` comments in Containerfile
  output
- Integration: CentOS 9 snapshot produces no sssd/dbus lines in
  Containerfile output
- Integration: CentOS 9 snapshot preserves `firewalld` mask if
  firewalld is in `baseline_package_names` and was masked by the user

### Refine UI

- Divergent service with `default_state: Some(...)` shows typed
  subtitle with preset context
- Divergent service with `default_state: None` shows
  `"(no preset rule)"` subtitle without fabricated preset reference
- Masked service with `default_state: None` shows
  `"masked (no preset rule)"` — not `"masked (diverges from preset:
  disabled)"`
- Preset-matched service with drop-in renders with override detail
- Preset-matched service without drop-in is suppressed
- Preset-unknown service shows `"(no preset rule)"` subtitle
- Omitted services appear in "Omitted Services" subsection with
  package absence reason
- `linked` warning renders in "Service Warnings" subsection with
  `extra["unit"]` as stable identity
- `enabled-runtime` warning renders in "Service Warnings" with
  transient explanation
- `bad` warning renders in "Service Warnings" subsection
- Unrecognized state warning renders with raw state value
- Warnings are not mixed with actionable service items

## Done When

- Stock-default services (preset-matched, no drop-ins) do not appear
  in the Containerfile output
- Non-actionable states (`alias`, `indirect`, `enabled-runtime`, etc.)
  never produce `state_changes` entries
- Runtime-only and unrecognized states produce warnings, not silent
  drops
- Services for packages not in the target image (verified via
  `packages_added ∪ baseline_package_names`) do not appear in the
  Containerfile output
- Services for packages present in the target image (including base
  image packages) ARE preserved in the Containerfile output
- Masked units with no preset rule carry `default_state: None`, not
  a fabricated value
- User-intent services (preset-divergent, masked, drop-in overrides)
  are faithfully preserved
- The refine UI shows intent signal strength via subtitles
- Renderer omissions are visible in both Containerfile comments and
  the refine UI "Omitted Services" subsection
- `linked` and `bad` states produce warnings, not false divergences
- Warning payloads carry structured `unit` + `raw_state` keys for
  stable UI identity
- Degraded mode (no baseline) emits services conservatively with
  advisory comments
- Automated tests prove that a clean RHEL install with no user service
  changes produces zero `systemctl enable/disable` lines
