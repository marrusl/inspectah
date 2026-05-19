# Service Intent Inference

> Replace state filtering with intent inference so the Containerfile
> renderer only emits service instructions for deliberate user choices.

**Status:** Proposed  
**Created:** 2026-05-19  
**Area:** inspectah-collect, inspectah-pipeline, inspectah-web

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
do nothing.

## Design

### Principle: intent inference, not filtering

Only `enabled`, `disabled`, and `masked` represent durable user intent.
Everything else is a side effect, transient state, or packaging
artifact. The collector drops non-actionable states at parse time —
they never enter the data model.

| State | User intent? | Action |
|-------|-------------|--------|
| `enabled` | Yes | Compare against presets |
| `disabled` | Yes | Compare against presets |
| `masked` | Yes | Always emit (deliberate suppression) |
| `static` | No | Skip (already handled) |
| `alias` | No | Skip |
| `indirect` | No | Skip |
| `enabled-runtime` | No | Skip |
| `masked-runtime` | No | Skip |
| `generated` | No | Skip |
| `transient` | No | Skip |
| `linked` | Partially | Skip, emit warning |
| `linked-runtime` | No | Skip |
| `bad` | No | Skip, emit warning |

### 1. Data Model

#### `ServiceUnitState` enum

Replaces the stringly-typed `current_state` and `default_state` fields
on `ServiceStateChange`.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceUnitState {
    Enabled,
    Disabled,
    Masked,
}
```

Three variants only. Non-actionable states are dropped at parse time
and never enter the data model.

#### `ServiceAction` — dropped in favor of `implied_action()`

The action (`enable`, `disable`, `mask`) is derivable from
`current_state` and was previously a redundant stored field. Replaced
by a method on `ServiceStateChange`:

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
- `default_state`: `String` → `ServiceUnitState` (non-optional —
  every `state_changes` entry has a known preset)
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

No `warnings` field — service warnings (`linked`, `bad`) flow through
the existing `Warning` type on `InspectionSnapshot.warnings` with
`inspector: "services"` and raw state data in the `extra` HashMap.

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
- `"linked"` / `"bad"` — emit a `Warning` via
  `InspectionSnapshot.warnings` with `inspector: "services"` and
  `extra: {"raw_state": "<state>"}`, skip from service data
- Everything else — silently drop

**Preset comparison.** Unchanged logic, tighter types. For actionable
units, compare `ServiceUnitState` against the preset:

- **Divergent** (current state differs from preset) — push to
  `state_changes` with `current_state` and `default_state` as
  `ServiceUnitState` values
- **Matched** (current state matches preset) — push to
  `preset_matched_units`
- **No preset rule** — unit appears in `enabled_units`/`disabled_units`
  only (preset-unknown). No `ServiceStateChange` entry is created.

**Masked units skip preset comparison entirely.** Masking is always
user intent regardless of what the preset says. They go straight into
`state_changes`. If a preset rule exists for the unit, `default_state`
is set to the preset value (e.g., `Enabled` if the preset says
`enable`). If no preset rule exists, `default_state` is set to
`Disabled` — the conservative assumption for a masked unit whose
original default is unknown.

### 3. Renderer Logic

The Containerfile renderer in
`inspectah-pipeline/src/render/containerfile.rs` gets two changes.

#### `implied_action()` replaces the stored `action` field

The renderer calls `sc.implied_action()` on each `ServiceStateChange`
to determine whether to emit `enable`, `disable`, or `mask`. The
method maps purely from `current_state` — it does not inspect
`default_state`.

#### Missing-package suppression

Before emitting service lines, the renderer builds a `HashSet` of
package names being installed from
`snap.rpm.packages_added.iter().map(|p| &p.name)`. For each
`state_change`:

- If `owning_package` is `Some(pkg)` and `pkg` is NOT in the install
  set → suppress (don't emit)
- If `owning_package` is `None` → emit (conservative — don't suppress
  what you can't verify)

**Suppress beats defer.** If a service would be deferred by
`config_tree_units` but its owning package isn't in the install set,
suppress it entirely. No point deferring a service whose package won't
be installed. Missing-package check runs before `config_tree_units`
deferral.

The `include` filter is unchanged — only `sc.include == true` entries
enter the renderer loop.

### 4. Refine UI Changes

The web handler's `normalize_services()` in
`inspectah-web/src/handlers.rs` already implements the three-way split
from the post-leaf fixes. Changes here are labeling improvements with
typed states.

#### Subtitle labels

| Situation | Subtitle |
|-----------|----------|
| Divergent (incl. masked), preset known | `"{current_state} (diverges from preset: {default_state})"` |
| Preset-matched, has drop-in | `"{state} (matches preset, has drop-in override)"` |
| Preset-matched, no drop-in | Suppressed — not rendered |
| Preset-unknown (enabled/disabled lists only) | `"enabled (no preset rule)"` / `"disabled (no preset rule)"` |

The handler uses `implied_action()` and pattern matching on
`ServiceUnitState` variants instead of `match sc.action.as_str()`
string comparisons.

#### Service warnings subsection

`linked` and `bad` warnings from `snap.warnings` (filtered by
`inspector == "services"`) render as a "Service Warnings" subsection
below the service items list. Each warning is a `ContextItem`:

- `id`: unit name
- `title`: unit name
- `subtitle`: `"linked (requires manual handling)"` or
  `"bad (unit file has errors)"`
- `detail`: the warning message from `Warning.message`

Warnings are not mixed with actionable services and are rendered in
a separate subsection.

Drop-in override handling is unchanged from the post-leaf fixes.

## Testing

### Collector

- Unit: `"enabled"` state with matching preset → in
  `preset_matched_units`, not in `state_changes`
- Unit: `"enabled"` state with divergent preset → in `state_changes`
  with typed `ServiceUnitState::Enabled`
- Unit: `"masked"` state → in `state_changes` regardless of preset
- Unit: `"alias"` state → silently dropped, not in any list
- Unit: `"indirect"` state → silently dropped
- Unit: `"enabled-runtime"` state → silently dropped
- Unit: `"linked"` state → warning emitted with
  `inspector: "services"`, not in service data
- Unit: `"bad"` state → warning emitted, not in service data
- Unit: `"generated"` state → silently dropped
- Unit: no matching preset rule → in `enabled_units`/`disabled_units`
  only, no `ServiceStateChange`
- Integration: clean RHEL/CentOS install with no user service changes
  produces zero `state_changes` entries

### Renderer

- `implied_action()` returns correct `ServiceAction` for all three
  `ServiceUnitState` variants
- Service with `owning_package: Some("sssd")` where sssd is not in
  `packages_added` → suppressed from output
- Service with `owning_package: None` → emitted (conservative)
- Service that is both config-tree-deferred and missing-package →
  suppressed entirely (suppress beats defer)
- Integration: CentOS 9 snapshot produces no sssd/dbus lines in
  Containerfile output

### Refine UI

- Divergent service shows typed subtitle with preset context
- Preset-matched service with drop-in renders with override detail
- Preset-matched service without drop-in is suppressed
- Preset-unknown service shows `"(no preset rule)"` subtitle
- `linked` warning renders in "Service Warnings" subsection
- `bad` warning renders in "Service Warnings" subsection
- Warnings are not mixed with actionable service items

## Done When

- Stock-default services (preset-matched, no drop-ins) do not appear
  in the Containerfile output
- Non-actionable states (`alias`, `indirect`, `enabled-runtime`, etc.)
  never produce `state_changes` entries
- Services for packages not in the target image do not appear in the
  Containerfile output
- User-intent services (preset-divergent, masked, drop-in overrides)
  are faithfully preserved
- The refine UI shows intent signal strength via subtitles
- `linked` and `bad` states produce warnings, not false divergences
- Automated tests prove that a clean RHEL install with no user service
  changes produces zero `systemctl enable/disable` lines
