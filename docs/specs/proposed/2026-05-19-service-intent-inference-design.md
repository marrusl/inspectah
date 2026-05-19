# Service Intent Inference

> Replace state filtering with intent inference so the Containerfile
> renderer only emits service instructions for deliberate user choices.

**Status:** Proposed (revision 5)  
**Created:** 2026-05-19  
**Area:** inspectah-collect, inspectah-pipeline, inspectah-web

> **Revision 5** (2026-05-19): `owning_package` population included
> in spec scope. The services collector now populates `owning_package`
> via batch `rpm -qf` with per-unit fallback, matching the Go
> codebase's `resolveOwningPackages` pattern. Suppression logic is
> no longer dormant — it activates end-to-end.
>
> **Revision 4** (2026-05-19): Addresses round 3 review finding.
> Clarifies that `packages_added` includes transitive dependencies
> (not just direct installs), closing Thorn's dependency-closure
> concern.
>
> **Revision 3** (2026-05-19): Addresses round 2 review findings.
> Preset knowledge uses its own `PresetDefault` enum (`Enable`,
> `Disable`) — `Masked` is no longer representable as a preset value.
> Target-image package set filters `packages_added` by `include ==
> true` to match the post-refine package plan. Package presence logic
> extracted into a shared `effective_target_packages()` function used
> by both package and service renderers. Package-name matching
> contract documented explicitly.
>
> **Revision 2** (2026-05-19): Addresses round 1 review findings.
> `default_state` is now `Option` — masked units with no preset rule
> carry `None`, not a fabricated `Disabled`. Missing-package
> suppression uses `packages_added ∪ baseline_package_names` as the
> truthful target-image package set. Silent-drop policy narrowed:
> known inert states drop silently, runtime/unknown states emit
> warnings. Renderer omissions surface explicitly in Containerfile
> comments and refine UI. Warning payloads carry structured `unit` +
> `raw_state` keys. Test coverage expanded for all new branches.

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

#### `PresetDefault` enum

Represents what the systemd preset system says a unit's default
enablement state should be. This is a separate type from
`ServiceUnitState` because presets occupy a narrower state space —
presets can only say `enable` or `disable`, never `mask`.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetDefault {
    Enable,
    Disable,
}
```

Using a dedicated type makes `Masked` unrepresentable as a preset
value at the type level — the compiler prevents this impossible state.

#### Observed state vs. preset knowledge

The spec separates what was observed on the system (`current_state`)
from what the preset system says the default should be
(`default_state`). These are independent facts with independent type
spaces:

- `current_state: ServiceUnitState` — what `systemctl is-enabled`
  reported. Three possible values: `Enabled`, `Disabled`, `Masked`.
- `default_state: Option<PresetDefault>` — what the preset rule says
  the package default is. Two possible values (`Enable`, `Disable`)
  or `None` when no matching preset rule was found.

The `Option` on `default_state` is load-bearing. A masked unit with
`default_state: None` means "the operator masked this, but we don't
know what the package default was." The UI renders this honestly as
`"masked (no preset rule)"` — not as a fabricated divergence from a
made-up default.

The type separation also prevents a class of comparison bugs: you
cannot accidentally compare `current_state == default_state` because
they are different types. The collector's divergence check explicitly
maps between them (e.g., `ServiceUnitState::Enabled` diverges from
`PresetDefault::Disable`).

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
- `default_state`: `String` → `Option<PresetDefault>` (`None` =
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
  `default_state: Some(PresetDefault)`. The divergence check maps
  between the two type spaces: `Enabled` diverges from
  `PresetDefault::Disable`, `Disabled` diverges from
  `PresetDefault::Enable`.
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

#### Target-image package presence

Service suppression keys off the same effective package set that the
package renderer uses for `RUN dnf install`. This is computed by a
shared function `effective_target_packages()` that both renderers
call, ensuring a single source of truth.

**`effective_target_packages(snap) -> HashSet<&str>`** returns the
set of package names that will exist in the target image:

1. `baseline_package_names` — packages already in the base image.
   These are always present in the target regardless of refine
   decisions. (The user cannot un-install base image packages through
   the refine UI — that would require a separate `RUN dnf remove`,
   which is a different rendering concern.)

2. `packages_added` filtered by `include == true` — packages that
   will be installed via `RUN dnf install`. Note: `packages_added`
   contains ALL packages present on the host beyond the baseline,
   including transitive dependencies pulled in by dnf — it is not
   limited to packages the operator explicitly installed. The
   leaf/auto classification (`leaf_packages`, `auto_packages`) is
   orthogonal to this set; both leaf and auto packages appear in
   `packages_added`. Packages where the user set `include: false`
   are excluded — if the package won't be installed, its services
   shouldn't be enabled.

The function builds a `HashSet<&str>` from
`baseline_package_names.iter().map(|s| s.as_str())` chained with
`packages_added.iter().filter(|p| p.include).map(|p| p.name.as_str())`.

**Package-name matching contract.** All three data sources
(`owning_package`, `packages_added.name`, `baseline_package_names`)
use the RPM `Name:` field — plain package names without arch suffix
or epoch. Matching is exact string comparison. Provider aliases and
subpackage relationships are resolved at RPM metadata time, not at
render time. If `owning_package` cannot be determined for a service
(e.g., the unit file is not owned by any RPM), it is `None` and the
service is emitted conservatively.

**`owning_package` population.** The services collector populates
`owning_package` for each `ServiceStateChange` entry via `rpm -qf`
after `state_changes` is built. This is the same pattern used in
the Go codebase's `resolveOwningPackages` function.

**Approach:** Batch-first with per-unit fallback:

1. Collect unit file paths for all `state_changes` entries
   (`/usr/lib/systemd/system/<unit>`)
2. Run batch `rpm -qf --queryformat "%{NAME}\n" <paths...>`
3. If the batch succeeds and output line count matches input count,
   zip results onto `state_changes`
4. If the batch fails (count mismatch, error), fall back to
   individual `rpm -qf` per unit, trying `/usr/lib/systemd/system/`
   then `/etc/systemd/system/`
5. Units not owned by any RPM (e.g., manually placed unit files)
   get `owning_package: None`

This runs after `state_changes` is populated (insertion point:
between state_changes assembly and drop-in collection). The executor
already supports arbitrary command execution via `exec.run()`.

**Suppression logic.** For each `state_change`:

- If `owning_package` is `Some(pkg)` and `pkg` IS in the effective
  target set → emit (the package will exist, user intent preserved)
- If `owning_package` is `Some(pkg)` and `pkg` is NOT in the
  effective target set → omit and emit a Containerfile comment:
  `# Omitted: <unit> (package '<pkg>' not in target image)`
- If `owning_package` is `None` → emit (conservative — don't suppress
  what you can't verify)

**Degraded mode.** When `snap.rpm.no_baseline` is `true` (baseline
data unavailable), `baseline_package_names` is empty and the
effective target set only contains included `packages_added`. The
renderer cannot prove whether a package is absent from the target
image, so it does NOT suppress — it emits the service instruction
with a comment:
`# NOTE: baseline unavailable — cannot verify '<pkg>' in target image`

**Suppress beats defer.** If a service would be deferred by
`config_tree_units` but its owning package isn't in the effective
target set, suppress it entirely. No point deferring a service whose
package won't be installed. The target-image check runs before
`config_tree_units` deferral.

The `include` filter on `ServiceStateChange` is unchanged — only
`sc.include == true` entries enter the renderer loop.

### 4. Refine UI Changes

The web handler's `normalize_services()` in
`inspectah-web/src/handlers.rs` already implements the three-way split
from the post-leaf fixes. Changes here are labeling improvements with
typed states and new visibility for omissions and warnings.

#### Subtitle labels

| Situation | Example subtitle |
|-----------|-----------------|
| Enabled, preset says disable | `"enabled (diverges from preset: disable)"` |
| Disabled, preset says enable | `"disabled (diverges from preset: enable)"` |
| Masked, preset known | `"masked (preset default: enable)"` |
| Masked, preset unknown | `"masked (no preset rule)"` |
| Preset-matched, has drop-in | `"enabled (matches preset, has drop-in override)"` |
| Preset-matched, no drop-in | Suppressed — not rendered |
| Preset-unknown (enabled/disabled lists only) | `"enabled (no preset rule)"` / `"disabled (no preset rule)"` |

Note: The "Divergent, preset unknown" case does not exist for
`enabled`/`disabled` units — those only enter `state_changes` when
a preset rule IS found and the state diverges. Units with no preset
rule stay in `enabled_units`/`disabled_units` only. The `None` case
for `default_state` only occurs with masked units.

The handler uses `implied_action()` and pattern matching on
`ServiceUnitState` variants instead of `match sc.action.as_str()`
string comparisons. `default_state` is `Option<PresetDefault>` and
the handler matches on `Some(PresetDefault::Enable)`,
`Some(PresetDefault::Disable)`, or `None` — `Masked` is not
representable as a preset value.

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
  `default_state: Some(PresetDefault::Disable)`
- Unit: `"masked"` state with preset rule → in `state_changes` with
  `default_state: Some(PresetDefault::Enable)`
- Unit: `"masked"` state with no preset rule → in `state_changes`
  with `default_state: None` (not fabricated `Disable`)
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
- `owning_package`: batch `rpm -qf` for 3 units → all 3 get
  `owning_package: Some(package_name)`
- `owning_package`: batch `rpm -qf` returns "not owned" for one
  unit → that unit gets `owning_package: None`
- `owning_package`: batch `rpm -qf` fails (exit code != 0) →
  falls back to individual queries per unit
- `owning_package`: unit in `/etc/systemd/system/` (not
  `/usr/lib/`) → fallback tries both paths
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
- Service with `owning_package: Some("custom-app")` where custom-app
  is in `packages_added` with `include: true` → emitted
- Service with `owning_package: Some("custom-app")` where custom-app
  is in `packages_added` with `include: false` → omitted (user
  excluded the package in refine, service shouldn't be enabled)
- `effective_target_packages()` returns same set that package renderer
  uses for `RUN dnf install` — packages excluded in refine are absent
  from both
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

- Divergent service with `default_state: Some(PresetDefault::Disable)`
  shows `"enabled (diverges from preset: disable)"` subtitle
- Masked service with `default_state: Some(PresetDefault::Enable)`
  shows `"masked (preset default: enable)"` subtitle
- Masked service with `default_state: None` shows
  `"masked (no preset rule)"` — not `"masked (diverges from preset:
  disabled)"` or any other fabricated value
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
  `effective_target_packages()`: included `packages_added` ∪
  `baseline_package_names`) do not appear in the Containerfile output
- Services for packages excluded in refine (`include: false`) are
  omitted from service output, matching the package renderer
- Services for packages present in the target image (including base
  image packages) ARE preserved in the Containerfile output
- Masked units with no preset rule carry `default_state: None`, not
  a fabricated value — `PresetDefault` enum makes `Masked` as a
  preset value unrepresentable at the type level
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
