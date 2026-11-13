# Unified systemd Unit Collection

## Metadata

| Field | Value |
|---|---|
| Date | 2026-11-13 |
| Status | Proposed. Scope is decided (§ 2). Everything in §§ 5 to 12 is the spec author's design and is open to review. § 16 lists what needs the project owner's call. |
| Scope | One model of systemd unit state for every unit type that exists as a static unit file, replacing the service-only collection. Collector, core types, snapshot schema, Containerfile render, the data contract for refine, and what the fleet merge does meanwhile. |
| Backlog items | Driving: `inspectah-socket-units-are-never-collected`. Folded in: `inspectah-local-timer-has-two-controls-and-neither-works`, `inspectah-timer-enable-lines-unquoted-and-trust-empty-units`, `inspectah-write-the-collector-overlap-contract` (the systemd half). Others in § 14. |
| Verified | Code citations read at `70aa8996` (HEAD, 2026-11-13). Line numbers are a convenience; the symbol name is the identifier. Claims about systemd behavior that were not checked on a host are marked as such and are listed for the real-host run in § 13. |

**No code, test, fixture, documentation or CHANGELOG entry was written to
produce this file.**

## 1. Problem

inspectah carries systemd state for `.service` units and nothing else.

`crates/collect/src/inspectors/services.rs` asks for services only
(`systemctl list-unit-files --type=service`, `:226`), gates state changes on
the `.service` suffix (`is_real_service`, `:25-36`), and scans
`/etc/systemd/system` for `.service` shadows (`:456`) and `.service.d`
drop-ins (`:763`) only. The config walk does not fill the gap: it excludes
`/etc/systemd/system/*.wants/*` and `*.requires/*`
(`crates/collect/src/inspectors/config/walk.rs:154-155`) and every symlink
directly under `/etc/systemd/system/` (`is_systemd_unit_symlink`, `:241`),
which is where enablement, masks and the default target live.

So a socket, path or timer that the administrator enabled, disabled or masked
against its preset never enters the model. Two consequences, both from the
driving backlog item and both confirmed against the code:

1. **Not carried.** The Containerfile has no `systemctl` line for the unit and
   the image keeps the preset. `systemctl enable --now cockpit.socket` on the
   source host is lost.
2. **Not omittable.** The presence tiers and the `# Omitted:` line are built
   on `services.state_changes`
   (`crates/pipeline/src/render/service_intent.rs:503`). A unit that is not in
   that list can be neither omitted with a reason nor preserved.

Timers are worse than uncollected: they are collected by a second inspector
that does not know about enablement. § 4 has the detail.

## 2. Decided scope

**One unified model of systemd units replaces the service-only collection.
Quadlets are excluded. Every standard unit type that exists as a static unit
file is collected. Units produced by generators, and transient units, are out.
Mount, automount and swap units are collected but are advisory only: they
never get an automatic `systemctl` line. (Mark Russell, 2026-11-13)**

Standing constraints this spec is written under:

- Pre-1.0: no compatibility shims for the snapshot schema. (Mark Russell,
  2026-11-11)
- Single host first. Clear aggregate bugs are fixed; aggregation is not
  redesigned. (Mark Russell, 2026-11-11)
- Admin configuration in `/etc` is fine to carry. Loss in the `/etc` 3-way
  merge is not a design driver. (Mark Russell, 2026-11-11)
- RHEL 9 and RHEL 10 ship dnf4. dnf5 is Fedora only.

## 3. Non-goals

- **Quadlets.** They stay with the containers section.
- **User units.** `/etc/systemd/user/`, `systemctl --global` and per-user
  units are not collected by this model. The walk's two exact entries and two
  globs for that directory stay as they are.
- **Generated units.** A unit whose `list-unit-files` state is `generated`,
  or whose file sits under `/run/systemd/generator`, `generator.early` or
  `generator.late`, is not carried. What that loses depends on whether the
  generator's input is collected elsewhere. Checked at `70aa8996`:

  | Generator input | Collected today | What is lost |
  |---|---|---|
  | `/etc/fstab` | Yes. `storage.rs:84-92` reads and parses it; refine locks every entry out as host state (`crates/refine/src/normalize.rs:204-218`). | Nothing new. Mounts from fstab are host storage and are already reported, not carried. |
  | `/etc/crypttab` | No dedicated collector. It reaches the snapshot only if the config walk records it, and then `MERGE_HOSTILE_PATHS` locks it out (`normalize.rs:204`). Whether the walk records it on a stock host was not measured. | `systemd-cryptsetup@*.service` units and the encrypted devices behind them. Host storage, same class as fstab, but there is no report row for it. |
  | SysV init scripts in `/etc/init.d` | Partly. `modernization.rs:56-63` raises an advisory for a script with no same-named vendor service. | The script's enablement: nothing in `crates/collect/src` reads the `rc*.d` links, so a script enabled for a runlevel and one that is merely present look the same. |
  | `/etc/rc.d/rc.local` exec bit | No. Nothing in `crates/collect/src` names `rc.local`. `ConfigFileEntry` has no mode field (`crates/core/src/types/config.rs:62-84`) and the config tree stager sets no permissions. | A host that runs `rc.local` at boot produces an image that does not, with no finding. |

  The last three are gaps this spec records and does not close.
- **Transient units and runtime states.** `transient`, `enabled-runtime`,
  `masked-runtime`, `linked-runtime` and `linked` keep today's treatment: a
  structured warning and no state change (`parse_unit_state`,
  `services.rs:49-80`).
- **Comparing host presets with the target image's presets.** The collector
  compares against the host's preset files, as it does today.
- **Rewriting unit file content.** No automatic rewrite of a mount unit's
  `Where=`, and no change to how the `/var` backing search reads unit files.
- **Aggregation redesign.** § 12 says what the merge does with the new types
  and no more.
- **The refine UI design.** § 11 gives the data contract and a recommended
  section design. The frontend's own design pass owns the rest.

## 4. What the code does today

Each item was read at `70aa8996`.

**Already independent of unit type.** `parse_unit_state` (`services.rs:49`),
preset glob resolution (`resolve_preset_default`, `:95`), the owner lookup
(`populate_owning_packages`, `:113`), the presence tiers
(`classify_service_presence`, `service_intent.rs:359`), `systemctl_lines`
(`:292`), merge identity on the unit name
(`crates/core/src/aggregate/merge.rs:147-150`) and `enabled_units_by_owner`
(`crates/pipeline/src/render/not_carried.rs:259`). None of them names
`.service`. The service restriction is the `--type` argument and four suffix
tests.

**Lifting the filter alone floods the inventories.** `enabled_units` and
`disabled_units` are pushed at `services.rs:320-324`, before the
`is_real_service` gate at `:330`. `readme.rs:89-98` counts them as services.

**`generated` and `transient` are already dropped or warned**
(`services.rs:55`, `:57`). `static`, `alias` and `indirect` are dropped
silently. `static` means the unit has no `[Install]` section, so
`list-unit-files` reports no enablement for it even when an administrator
linked it into a `.wants` directory by hand.

**Template units are skipped whole.** Any name with `@` is dropped at
`services.rs:303`, instances included.

**The `dbus-` filter is broader than its comment.** `services.rs:28` drops
every name starting `dbus-`. The comment names D-Bus activation aliases
(`dbus-org.*`). `dbus-broker.service` is a real service and is dropped with
them.

**Timers have two owners and neither tracks enablement.**
`scan_systemd_timers` (`scheduled.rs:772-826`) reads each `.timer` file and
its paired `.service`, sets `FindingKind::included()`, and never asks whether
the timer is enabled. Render then enables every included local timer
(`containerfile.rs:1382-1401`). A local timer that is disabled on the host
comes out enabled. A vendor timer enabled against its preset gets no line at
all, because the services inspector never saw it and the scheduled section
enables only `source == "local"` timers (`containerfile.rs:1328-1332`).

**The local timer file has two writers and one of them ignores its control.**
A local `.timer` is a regular file under `/etc/systemd/system`, so the config
walk records it in `config.files`, and the config tree writes it. The
scheduled block of the same stager writes it again, without reading
`disposition` (`crates/pipeline/src/render/configtree.rs:440-449`).

**The timer toggle is accepted and never applied.** The bulk operation emits
`SetInclude` for `ItemId::SystemdTimer` (`crates/web/src/handlers.rs:949-958`).
The session's validation accepts it in an arm commented "not yet handled,
accept without validation" (`crates/refine/src/session.rs:2190-2193`), and the
apply arms near `:2807` have no case for it. The request returns 200 and the
snapshot is unchanged. This is the cause the two-controls backlog item asked
for.

**Enable lines run before the files they name.** The Containerfile renders
the service section, then the drop-ins `COPY`, at step 2
(`containerfile.rs:177-178`), the scheduled-tasks `COPY` at step 4 (`:192`)
and the config `COPY` at step 5 (`:198`). Three effects follow:

- A local timer and its paired service avoid the problem only because
  `config_tree_units` (`service_intent.rs:308-329`) defers them to the
  scheduled section, which copies before it enables.
- **A local `.service` that is not paired with a timer does not avoid it.** It
  is unowned, so Tier 1 emits it (`service_intent.rs:366-372`); it has no
  vendor counterpart, so it is not a full shadow and its file is in
  `config.files`, copied at step 5. Its `RUN systemctl enable` runs at step 2,
  against a unit file that is not in the image yet. `systemctl enable` on a
  missing unit fails the build. This is a source reading at `70aa8996`, not a
  host measurement; § 13 adds the measurement.
- A full shadow is copied after its enable line, so the enable resolves the
  vendor fragment. That is the observation recorded in
  `inspectah-full-shadow-wants-link-points-at-the-vendor-path`.

**Unit names reach `RUN` lines bare.** `systemctl_lines` and the timer enable
line (`containerfile.rs:1400`) write names unquoted. Both sites refuse names
that fail `shell_operand_refusal` (`crates/pipeline/src/render/safety.rs:142`),
which rejects shell metacharacters and a leading `-` or `#`. It does not
reject `*`, `?` or `[`, and it does reject `\`, which legitimate escaped unit
names contain (`\x2d`). The repository's own quoting rule says a bare token in
a command should be quoted and never blacklisted
(`process-docs/skills/containerfile-value-quoting.md`).

**A masked unit is recorded twice, once wrongly.** `masked` produces a state
change and a `systemctl mask` line (`services.rs:336-352`). The independent
shadow scan (`:456-485`) then tests only that a vendor unit of the same name
exists, reads the `/etc` path through the `/dev/null` link, and records a
`FullShadow` drop-in with empty content.

**The default target is not recorded.** `/etc/systemd/system/default.target`
is in the walk's exact exclusion list (`walk.rs:61`) and no inspector reads
it.

## 5. Scope per unit type

| Unit type | Treatment | Reason |
|---|---|---|
| `.service` | Carry state | Today's behavior, unchanged in meaning. |
| `.socket` | Carry state | Packages ship sockets disabled that administrators enable (`cockpit.socket`). The driving item. |
| `.path` | Carry state | Same model as a socket: an activation unit with `[Install]`. |
| `.timer` | Carry state. Schedule and command stay with scheduled tasks (§ 8). | Enablement is unit state; what runs and when is the scheduled-task fact. |
| `.target` | Carry state for targets that have `[Install]`. The default target is separate (§ 7.4). | Rare, and identical in mechanism. `default.target` is a link written by `systemctl set-default`, not an enable. |
| `.slice` | Carry state. In practice this means a mask; slices are otherwise `static`. Drop-ins are the common customization and follow § 7.5. | Uniform treatment costs nothing; a special case would. |
| `.mount`, `.automount`, `.swap` | **Advisory only.** Collected, reported, never a `systemctl` line. | Decided (§ 2). A mount is a statement about one host's storage, and on an image-based target several mount points are owned by bootc or are symlinks into `/var` (§ 10.6). |
| `.device`, `.scope` | Excluded. | They never exist as unit files. Administrator drop-ins for them (`*.device.d`, `*.scope.d`) are still collected as drop-ins (§ 7.5). |
| Template (`name@.service`) | Not a state carrier itself. Instances are § 7.3. | A template has no enablement without an instance name. |
| Generated units | Excluded (§ 3). Administrator drop-ins on them are kept. | Decided. The generator's input is the fact, not its output. |
| Transient units, runtime and `linked` states | Excluded, with today's warning. | Decided, and they do not survive a reboot or need manual handling. |
| User units | Out of scope (§ 3). | Not part of this change. |
| Quadlets | Excluded. | Decided. |

The assumption that targets, slices and mounts rarely differ from preset is
untested. § 13 specifies the survey that tests it.

## 6. The unified unit model

Types live in a new `crates/core/src/types/units.rs`, replacing
`types/services.rs`.

### 6.1 A unit name is proved once

```rust
/// A systemd unit name with a recognized type suffix.
///
/// Parsed at the collector and at deserialization, so every later use is
/// of a proved value: 1 to 255 bytes, drawn from `[A-Za-z0-9:_.\\@-]`,
/// ending in the suffix of a kind this model knows.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct UnitName(String);

impl UnitName {
    pub fn kind(&self) -> UnitKind { /* from the suffix */ }
    pub fn as_str(&self) -> &str { &self.0 }
}
```

The kind is derived from the name and never stored beside it, so a snapshot
cannot hold a `cockpit.socket` labelled as a service. The character set is
the one `systemd.unit(5)` gives for unit names. A snapshot holding a name
outside it fails to load, which is the refusal an imported snapshot should
get.

### 6.2 Advisory-only kinds cannot reach a `systemctl` line

```rust
/// Kinds whose enablement inspectah carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CarriedKind { Service, Socket, Path, Timer, Target, Slice }

/// Kinds inspectah reports and never acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AdvisoryKind { Mount, Automount, Swap }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitKind { Carried(CarriedKind), Advisory(AdvisoryKind) }

/// A `UnitName` whose kind is carried. The only name type a state change
/// holds, and the only one `systemctl_lines` accepts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "UnitName", into = "UnitName")]
pub struct CarriedUnit(UnitName);

/// A `UnitName` whose kind is advisory only.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "UnitName", into = "UnitName")]
pub struct AdvisoryUnit(UnitName);
```

`.device` and `.scope` have no variant, so no value of either can exist.
`UnitAction` (enable, disable, mask) is reachable only through
`UnitStateChange::implied_action`, and a `UnitStateChange` holds a
`CarriedUnit`. There is no function from an `AdvisoryUnit` to a `UnitAction`,
and `systemctl_lines` takes `&[CarriedUnit]`. A mount unit placed in
`state_changes` by a hand-edited snapshot fails deserialization. The compiler
enforces the decision in § 2 and no `match` arm has to remember it.

### 6.3 State changes

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnitState { Enabled, Disabled, Masked }

/// Where the unit's file lives on the source host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "fragment", rename_all = "snake_case")]
pub enum UnitFragment {
    /// Only under `/usr/lib/systemd/system`. `owning_package` is `None`
    /// when the RPM query failed.
    Vendor { owning_package: Option<String> },
    /// Only under `/etc/systemd/system`: a file the administrator wrote.
    Local { path: String },
    /// Both. The `/etc` file replaces the vendor one.
    Shadowed { path: String, owning_package: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitStateChange {
    pub unit: CarriedUnit,
    pub current_state: UnitState,
    #[serde(deserialize_with = "require_explicit_null")]
    pub default_state: Option<PresetDefault>,
    pub fragment: UnitFragment,
    #[serde(default)]
    pub disposition: FindingKind,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    pub aggregate: Option<AggregatePrevalence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_reason: Option<String>,
}
```

`UnitFragment` replaces `owning_package: Option<String>` and the
`shadow_type` and `shadow_rationale` pair on the state change. Today
`owning_package: None` means both "the administrator wrote this unit" and
"the RPM query failed", and Tier 1 emits either one unconditionally. The two
cases need different answers at render (§ 10.2), so the type separates them.
No production code that writes `shadow_type` on a state change was found by
grep at `70aa8996`; the plan confirms that before removing the fields.

### 6.4 The section

```rust
/// One unit file's enablement as `list-unit-files` reported it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitInventoryEntry {
    pub unit: UnitName,
    pub state: UnitState,
    pub preset: Option<PresetDefault>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UnitSection {
    /// Units whose state differs from what the image would have.
    pub state_changes: Vec<UnitStateChange>,
    /// Every unit with a durable state, changed or not.
    pub inventory: Vec<UnitInventoryEntry>,
    /// Drop-ins and full shadows, for every kind (§ 7.5).
    pub drop_ins: Vec<SystemdDropIn>,
    /// Mount, automount and swap units worth reporting (§ 6.5).
    pub mount_findings: Vec<MountUnitFinding>,
    /// The default target, when the host sets one (§ 7.4).
    pub default_target: Option<DefaultTarget>,
}
```

`inventory` replaces the three string lists `enabled_units`, `disabled_units`
and `preset_matched_units`. One list with a state per entry cannot place a
unit in two of them, and every consumer that counts or filters does so by
`unit.kind()`. `readme.rs` counts entries whose kind is `Service` or, better,
reports units. `enabled_units_by_owner` filters `state == Enabled`.

### 6.5 Mount findings

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MountUnitFinding {
    pub unit: AdvisoryUnit,
    /// Present when enablement differs from preset, or the unit is masked.
    pub state_change: Option<(UnitState, Option<PresetDefault>)>,
    pub fragment: UnitFragment,
    /// `Where=` for a mount or automount.
    pub mount_point: Option<String>,
    /// `What=`.
    pub what: Option<String>,
    /// Whether `Options=` holds `nofail`.
    pub nofail: bool,
    pub aggregate: Option<AggregatePrevalence>,
}
```

The finding has no `disposition`. There is nothing to include or exclude, so
there is no control to offer. It records what the host had. Whether a mount
point collides with the target layout is a property of the target, so the
hazards in § 10.6 are computed at render from `mount_point`, not stored.

A finding is recorded for every `.mount`, `.automount` or `.swap` regular
file under `/etc/systemd/system`, and for a vendor one only when its state
differs from preset or it is masked (`tmp.mount` is the expected case).

## 7. The collector

`UnitsInspector` replaces `ServicesInspector`. It is the one owner of three
facts for every kind: whether a unit is enabled, disabled or masked against
what the image would have; where its file lives; and which package owns it.

### 7.1 Sources

1. `systemctl list-unit-files --no-pager`, with no `--type`. The state column
   goes through `parse_unit_state` unchanged.
2. The host's preset files, as today.
3. A direct read of `/etc/systemd/system`: regular unit files (for
   `UnitFragment`), symlinks to `/dev/null` (masks), `default.target`, and
   the `*.wants/` and `*.requires/` directories (§ 7.3).

Source 3 exists because source 1 cannot see two things: an administrator's
`ln -s` into a `.wants` directory for a unit with no `[Install]` section,
which `list-unit-files` reports as `static`, and enabled template instances.

### 7.2 What makes a state change

For each entry with a durable state whose name parses as a `CarriedUnit`:

- **Masked** is always a state change, as today.
- **`Vendor` or `Shadowed` fragment:** a state change when the state differs
  from the preset, as today. With no matching preset rule there is no
  evidence of a change and none is recorded, as today.
- **`Local` fragment:** the image does not have this unit unless the export
  carries it, so the comparison with a preset is the wrong one. An enabled
  local unit is a state change whatever the presets say. A disabled local
  unit is not: a unit file copied into the image is not enabled until
  something enables it. On RHEL, CentOS and Fedora the stock
  `disable *` preset gives the same answer today; this rule makes it hold
  where no such preset exists.

The name gate becomes: the name parses as a `UnitName`, and it is not a D-Bus
activation alias. The alias test narrows to the `dbus-org.` prefix, which is
what the comment at `services.rs:26-27` describes, so `dbus-broker.service`
is no longer dropped.

Units inspectah must not switch off are marked at collect and refused at
render (§ 10.3).

### 7.3 `.wants`, `.requires` and template instances

The collector lists each `*.wants/` and `*.requires/` directory under
`/etc/systemd/system` and reads each link. It records a state change of
`Enabled` for:

- a link to a unit that `list-unit-files` reported as `static`, since the
  link is the administrator's doing and nothing else records it;
- a link whose name is a template instance (`name@instance.service`), since
  `list-unit-files` lists the template and not its instances.

The config walk keeps excluding these directories. They hold links, not
content, and the collector now turns them into state, which is the answer to
the question the overlap item asked about that exclusion.

A link written by hand may name a `.requires` relationship or a target other
than the unit's own `WantedBy=`. `systemctl enable` cannot reproduce that.
For those the render emits the link itself (§ 10.1). Whether instances and
hand-made links are in scope at all is § 16 question 1.

### 7.4 The default target

The collector reads the link at `/etc/systemd/system/default.target`. If it
exists and its target's name parses as a `.target`, it records:

```rust
pub struct DefaultTarget {
    pub target: CarriedUnit,          // always a `.target`
    pub disposition: FindingKind,
    pub locked: bool,
    pub aggregate: Option<AggregatePrevalence>,
}
```

This is its own check and its own render line. It is not an enable, and it
does not go through the preset comparison.

### 7.5 Drop-ins, shadows and masks

- **Drop-ins** are collected from `/etc/systemd/system/<unit>.d/*.conf` for
  every unit name that parses, plus `*.device.d` and `*.scope.d`, and for
  units that are generated (the parent is recorded as generated, detected by
  state or by a file under `/run/systemd/generator*`). The existing rule
  holds: the units section owns the file, and the config entry for the same
  path is `Inventory` with `shadowed_by` set
  (`shadow_service_owned_paths`, `crates/pipeline/src/collect.rs:274`).
- **Full shadows** are collected for every kind: a regular file under
  `/etc/systemd/system` with a vendor file of the same name.
- **A symlink is never a shadow.** The shadow scan tests the link first. A
  link to `/dev/null` is a mask and is already a `Masked` state change; it
  produces no drop-in entry and no zero-length file. A link to another unit
  is an alias and produces nothing.

`SystemdDropIn` keeps its fields. `unit` stays a `String`, because a drop-in
may name a `.device` or `.scope` that `UnitName` does not admit.

### 7.6 Owning package

`populate_owning_packages` runs over every `Vendor` and `Shadowed` state
change and mount finding. It is unchanged apart from its input type.

## 8. What stays in scheduled tasks

| Fact | Owner | Control |
|---|---|---|
| A timer is enabled, disabled or masked | Units (`state_changes`) | The unit row |
| A local timer's file, and its paired service's file | Config (`config.files`), like every other local unit file | The config row |
| When a timer fires and what it runs (`OnCalendar=`, `ExecStart=`, description) | Scheduled (`systemd_timers`) | None. It is a description. |
| A timer generated from a cron entry: its content, its staging, its enable | Scheduled (`generated_timer_units`) | The generated-timer row |
| Cron jobs, at jobs | Scheduled | Unchanged |

**The single-control rule: each fact has one control, and a fact that
depends on another is resolved at render by omission, never by a second
writer.** An enable line depends on the unit file being in the image. If the
operator excludes a local timer's file, the enable is omitted with a stated
reason (§ 10.2). Nothing else writes the file and nothing else enables the
unit.

What changes in scheduled:

- `SystemdTimer` becomes a description: `name`, `on_calendar`, `exec_start`,
  `description`, `source`, `path`, `aggregate`. It loses `disposition` and
  `locked`, since it controls nothing, and `timer_content` and
  `service_content`, since the local file's content is already in
  `config.files` and a second copy is a second thing to redact.
- `scan_systemd_timers` keeps reading the files to fill those fields.
- The local-timer loop in the stager (`configtree.rs:440-449`) is removed.
  The config loop already writes the same files to the same place.
- The local-timer enable in `scheduled_tasks_section_lines`
  (`containerfile.rs:1382-1386`) is removed. Generated units keep theirs.
- `config_tree_units` (`service_intent.rs:308`) keeps only its
  generated-unit half.
- `ItemId::SystemdTimer` is removed from refine, with the bulk operation that
  emits it and the validation arm that accepts it. The silent no-op is closed
  by removing the control, not by wiring it up.

The cron-to-timer conversion (`make_timer_service`, `scheduled.rs:676`) is
untouched. A generated unit does not exist on the host, so it has no host
state to collect, and the scheduled section stays its single control.

## 9. Snapshot schema

- `InspectionSnapshot.services: Option<ServiceSection>`
  (`crates/core/src/snapshot.rs:33`) becomes `units: Option<UnitSection>`.
  The JSON key changes from `services` to `units`. `InspectorId::Services`
  and `SectionData::Services` are renamed to match. A section that holds
  sockets and timers under the name "services" would be a name the code does
  not satisfy.
- `ServiceStateChange`, `ServiceUnitState`, `ServiceAction` and
  `ServiceSection` become `UnitStateChange`, `UnitState`, `UnitAction` and
  `UnitSection`, with the field changes in § 6.
- `SystemdTimer` loses four fields (§ 8).
- `SCHEMA_VERSION` and `MIN_SCHEMA` move together from 27. Each phase in
  § 15 that changes a serialized type takes one bump, so the sequence is 28,
  29, 30 if the phases merge separately. No shim, no migration: an older
  snapshot is re-scanned. Each bump carries the refusal test through all
  three loaders that
  `process-docs/skills/snapshot-schema-versioning.md` requires, with a
  payload that shows why the closed version is refused. For 27 that payload
  is a snapshot with a `services` section and no `units`: it parses, and
  every socket and timer state the host had is absent from it with nothing
  to say so.

## 10. Render

### 10.1 Lines

One function, the successor of `render_service_intent`, produces every
`systemctl` line for collected units. It emits, in this order, under one
section header:

```
# === Systemd Unit State ===
# Omitted: cockpit.socket (package 'cockpit-ws' was excluded)
RUN systemctl set-default -- 'multi-user.target'
RUN systemctl enable -- \
    'cockpit.socket' \
    'myapp.path' \
    'report.timer'
RUN systemctl disable -- \
    'kdump.service'
RUN systemctl mask -- \
    'bluetooth.service'
```

- One `RUN` per verb. Units within it are ordered by kind, then name, so the
  output is stable and a reader finds the sockets together.
- **Every unit is single-quoted and each list is preceded by `--`.** The
  operand is a `CarriedUnit`, whose character set holds no quote and no line
  break, so `single_quote` (`safety.rs:162`) is total on it. Quoting replaces
  the refusal: a name with `\x2d` is legal and now renders, and `-.slice` is
  safe after `--`. `systemctl_lines` takes `&[CarriedUnit]`; the generated
  timer and `@reboot` service enables in the scheduled section call the same
  helper. That `systemctl` accepts `--` before unit operands, and that a
  backslash inside single quotes survives the Containerfile parser, are not
  verified here; § 13 checks both.
- A template instance renders with its instance name.
- A hand-made `.wants` or `.requires` link that `systemctl enable` cannot
  reproduce renders as `RUN ln -s` of the same link, with both paths proved
  as export paths.

### 10.2 An enable needs a unit file

Before a unit reaches an enable or disable line, the plan decides whether its
file will be in the image:

| Fragment | The file is in the image when |
|---|---|
| `Vendor` | The presence tiers say the owning package is (`classify_service_presence`, unchanged). `owning_package: None` keeps Tier 1: emit. |
| `Local` | An included entry stages the file at `path`: a `config.files` entry, or a generated unit of that name. |
| `Shadowed` | Either of the above. |

When it is not, the unit is omitted and recorded, exactly as an absent
package's unit is today. `OmissionReason` gains one variant:

```rust
/// The unit's own file is not in the export: its config entry is excluded,
/// or its content was not captured.
UnitFileNotStaged,
```

with the wording `unit file /etc/systemd/system/<name> is not in the export`.
The check asks the stager's own predicate for "this entry is written", so the
renderer and the stager cannot disagree, which is the defect in
`inspectah-timer-enable-lines-unquoted-and-trust-empty-units` item 2.

`systemctl mask` needs no unit file, so a masked `Local` unit is not subject
to this check. That `mask` succeeds for a unit that does not exist is not
verified here.

`# Omitted:` lines for not-carried and excluded owners work for every kind
with no change, since the tiers never looked at the suffix. The not-carried
list's "was enabled" annotation (`not_carried.rs:259-290`) picks up sockets
and timers the same way.

### 10.3 Units inspectah will not switch off

A `disable` or `mask` on a unit the target needs to boot or update would
produce an image that builds and then fails in service. For a name matching
`bootc-*`, `ostree-*` or `systemd-remount-fs.service`, the plan emits no
`disable` or `mask` line and writes:

```
# FIXME: bootc-fetch-apply-updates.timer is disabled on the source host. Not carried: the image needs this unit.
```

The list is one named constant. The collector sets `attention_reason` on the
same units so refine shows why. An `enable` on these names is carried.

### 10.4 Units that update packages at runtime

`dnf-automatic.timer`, its `-install`, `-download` and `-notifyonly`
variants, and `packagekit-offline-update.service` change or prepare to
change installed packages on a running host. `/usr` is read-only on the
target and the update path is a new image.

**Recommended:** these units are collected as ordinary state changes with
`disposition: Actionable { include: false }` and an `attention_reason`
saying the unit updates packages in place, which the target does not
support, and naming `bootc-fetch-apply-updates.timer` as the equivalent. The
operator can include one, which is right for the `-notifyonly` and
`-download` variants. The Containerfile carries one comment naming what was
left out.

**Alternative:** carry the state as the host had it and attach a `# FIXME`.
That is the more literal migration and leaves a unit that fails on every
run until someone reads the comment.

This is a recommendation, not a decision: § 16 question 2.

### 10.5 The default target

`RUN systemctl set-default -- '<target>'` when `default_target` is included.
For a `Local` target the file check in § 10.2 applies. When the target is
`graphical.target`, the line is preceded by a comment saying the image needs
a display manager for the target to mean anything. Whether a graphical
default should be excluded by default is § 16 question 4.

### 10.6 Mount, automount and swap advisories

For each `MountUnitFinding`, comment lines and nothing else. The first line
states the fact. Further lines are added per hazard, computed from
`mount_point`:

| Condition | Text added |
|---|---|
| Always | `# Advisory: <unit> is <state> on the source host. inspectah does not enable, disable or mask mount, automount or swap units.` |
| `mount_point` is under `/usr` | `#   /usr is read-only on the target. This mount cannot be satisfied.` |
| `mount_point` is `/`, `/boot`, `/boot/efi`, `/sysroot` or `/var` | `#   bootc and ostree own this mount point on the target.` |
| `mount_point` is, or is under, `/home`, `/srv`, `/opt`, `/mnt` or `/root` | `#   <path> is a symlink into /var on the target. The unit must be renamed for the resolved path (for /srv: var-srv.mount with Where=/var/srv).` |
| A mount or swap with `nofail` absent | `#   No nofail option. If the device is missing, every host built from this image boots to emergency mode.` |
| The state is enabled or masked | `#   To carry it, add after the configuration COPY: RUN systemctl <verb> -- '<unit>'` |

The mount-point lists are named constants beside the existing target-layout
knowledge. The claims in the table about the target layout come from earlier
analysis and from `inspectah-srv-mount-units-must-target-var-srv`; they were
not re-measured for this spec.

The unit file itself stays a config entry under the config walk's control,
as it is today. Whether it should stop being included by default is § 16
question 3.

### 10.7 Order

The unit state section moves from step 2 to directly after the Configuration
Files section. Every `COPY` that can stage a unit file precedes it: drop-ins
and shadows, generated timers, and the config tree. The drop-ins `COPY`
stays where it is. This closes the local-service ordering defect in § 4 and
changes which fragment a full shadow's enable resolves.

## 11. Refine

### 11.1 Data contract

The view response carries, in place of `service_states`:

| Field | Content |
|---|---|
| `unit_states[]` | One per `UnitStateChange`: `unit`, `kind` (derived, lowercase), `current_state`, `default_state`, `fragment` (`vendor`, `local`, `shadowed`), `unit_file_path` when not vendor, `owning_package`, `include`, `locked`, `triage`, `attention_reason`, and `omitted` with the `OmissionReason::describe` text when the render plan omits the unit. |
| `unit_dropins[]` | As `service_dropins` today, with `kind` of the parent. A drop-in whose parent has no state change is still listed, since a unit at its preset has a drop-in and no parent row. |
| `default_target` | `target`, `include`, `locked`, or absent. |
| `mount_findings[]` | `unit`, `kind`, `state`, `mount_point`, `what`, `fragment`, and `advisories[]`, the same strings § 10.6 renders. No `include`: these rows have no toggle. |
| `scheduled_tasks` timers | Each row gains `unit` (`<name>.timer`) and `unit_row` (whether `unit_states` holds it), so the row can link to its control. No `include`. |

Operations: `ItemId::Service { unit }` becomes `ItemId::Unit { unit }` and
keeps its cascade to drop-ins. `ItemId::DefaultTarget` is added.
`ItemId::SystemdTimer` is removed. `SetInclude` on a mount finding does not
exist, so there is nothing to refuse.

The omission reasons and advisory strings have one source each, in the
pipeline crate, as `OmissionReason::describe` is today.

### 11.2 Recommended section design

**This needs the frontend's own design pass. It is a recommendation.**

One "Systemd units" section replaces "Services". Rows are grouped by kind,
and the section shows only units that differ from preset or carry a drop-in,
which is what `unit_states` and `unit_dropins` hold. Services, sockets,
timers and paths get their own groups; targets, slices and the mount
advisories sit in one collapsed group, since they are expected to be few.
Scheduled Tasks keeps showing the schedule and command for each timer and
links to the timer's unit row.

The main alternative is to show each unit inside the section it relates to
(timers under Scheduled Tasks, sockets beside their service). That recreates
two places to look for one timer, which is the condition this spec removes.

## 12. Fleet

The merge compiles against the new types and its behavior is otherwise
unchanged:

- `UnitStateChange` keeps identity on the unit name
  (`merge.rs:147-150`) and merges with `merge_items`.
- `inventory` merges by unit name, keeping the most prevalent state, where
  the three string lists were unioned.
- `mount_findings` merge by unit name. They have no include flag, so
  narrowing does nothing to them.
- `default_target` takes the most prevalent target, and is excluded when the
  hosts do not all agree, the same rule `narrow_non_universal` applies.
- `SystemdTimer` no longer has an include flag to narrow.

One aggregate defect closes as a side effect: a local timer present on some
hosts was excluded by both of its owners and written anyway, because the
scheduled writer ignored disposition. With one writer that honors the config
entry's disposition, the exclusion holds.

One existing behavior is recorded and not changed: identity on the name
alone means hosts that disagree about a unit's state collapse to the most
prevalent state, with a prevalence count of every host that had any state
change for it. That is an aggregation design question and stays out.

## 13. Verification

Tests the implementation plan must contain:

1. **Collector, socket.** `MockExecutor` with `cockpit.socket` reported
   `enabled` against a `disable` preset. Assert one `UnitStateChange` with
   kind `Socket`, `Vendor` fragment and the owning package.
2. **Render, excluded owner.** That snapshot with the owning package
   excluded. Assert an `# Omitted: cockpit.socket (...)` line and no
   `systemctl enable` naming the socket.
3. **Disabled local timer stays disabled.** Collected through the real
   inspectors: a local `.timer` and paired `.service`, the timer reported
   `disabled`. Assert the files are staged once, and no `systemctl enable`
   names the timer.
4. **Excluded local unit file.** A local enabled socket whose config entry
   is excluded. Assert no file in the staged tree, an `# Omitted:` line with
   the `UnitFileNotStaged` wording, and no enable. Drive both toggles
   through `RefineSession` and assert on the staged tree and the
   Containerfile, as `cross-inspector-shadow-passes.md` requires for an
   overlapping path.
5. **Mount advisory.** A local `srv.mount`, enabled. Assert the advisory
   lines including the `/var` rename, and no `systemctl` instruction line
   naming it.
6. **Type-level.** A snapshot with a `.mount` name in `state_changes` fails
   to load. A compile-fail or signature test showing `systemctl_lines`
   takes `CarriedUnit` only.
7. **Mask is not a shadow.** A unit linked to `/dev/null` with a vendor
   counterpart yields one `Masked` state change, no drop-in entry and no
   zero-length staged file.
8. **Order.** A local service with no paired timer: its enable line comes
   after the config `COPY`.
9. **Quoting.** A unit named with `\x2d` and one named `-.slice` render
   quoted after `--`.
10. **Protected units.** `bootc-fetch-apply-updates.timer` disabled on the
    host yields the FIXME and no `disable`.
11. **Schema.** Version 27 is refused through `InspectionSnapshot::load`,
    refine's `from_tarball` and aggregate's `load_snapshot_from_tarball`.

Real-host runs, specified here and not run for this spec. **Do not use the
CentOS9 or fedora44 VMs for these runs.**

12. **`cockpit.socket` end to end.** On a RHEL 9 host and a RHEL 10 host:
    `systemctl enable --now cockpit.socket`, scan, build the exported
    Containerfile, and confirm `systemctl is-enabled cockpit.socket` reads
    `enabled` in the built image.
13. **Same run, four more checks.** A local service with no paired timer
    builds (the § 4 ordering defect, before and after). A full shadow's
    wants-link target and the fragment `systemctl cat` reports. A unit with
    `\x2d` in its name survives `RUN systemctl enable -- '...'`. `systemctl
    mask` on a unit absent from the image succeeds at build time.
14. **Survey.** `systemctl list-unit-files --no-pager` on RHEL 9 and RHEL 10
    hosts, stock and after the driftify standard profile. Count, per unit
    type, units whose state differs from preset. This tests the assumption
    that targets, slices and mounts rarely differ, and sizes the collapsed
    group in § 11.2. Record the result in
    `process-docs/skills/real-host-validation-runs.md`.
15. **Stock-host false positives.** Scan a stock host of each release
    before a drifted one, and review every row the new kinds add.

## 14. Backlog items

Each item was read in full before the design, and its claims checked
against `70aa8996`.

| Item | Result | Still right? |
|---|---|---|
| `inspectah-socket-units-are-never-collected` | Closed by phase 1 and run 12. | Yes. All four cited sites confirmed. |
| `inspectah-local-timer-has-two-controls-and-neither-works` | Closed by phase 2. Control 1 goes with the second writer; control 2 is removed, and its cause is in § 4. | Yes, with stale line numbers: the writer is now `configtree.rs:440-449`. |
| `inspectah-timer-enable-lines-unquoted-and-trust-empty-units` | Closed by phases 1 (quoting) and 2 (file check, single writer). | Partly stale. Since `2c2e52aa` the names pass `is_shell_operand`, so `#skip` and a leading option are refused. They are still bare, with no `--`, and glob characters pass. Items 2 and 3 stand. |
| `inspectah-write-the-collector-overlap-contract` | Partly covered. § 8 and § 7.5 are the ownership table for `/etc/systemd/system`, the `*.wants/*` exclusion is answered in § 7.3, and the missing `scheduled_owned_paths` is made unnecessary. Phase 3 writes this part into `process-docs/` and fixes `docs/reference/inspector-coverage.md`. Remaining: the general rules document, and the non-systemd claimants (quadlets, repo files, GPG keys). | Yes. |
| `inspectah-masked-units-are-recorded-as-full-shadows` | Closed by phase 3 (§ 7.5). The alias question is answered: an alias produces nothing. | Yes. |
| `inspectah-full-shadow-wants-link-points-at-the-vendor-path` | Partly covered. The order change in § 10.7 changes what the enable resolves. The measurement the item asks for is run 13. | Yes; still unmeasured. |
| `inspectah-srv-mount-units-must-target-var-srv` | Partly covered. The advisory in § 10.6 names the problem and the rename. Remaining: the unit file is still carried as config by default (§ 16 question 3), there is no automatic rewrite, and fstab lines and the audit report are untouched. | Yes. |
| `inspectah-etc-unit-files-accessor` | Out of scope. Its blocker moves: after phase 3 the `/etc/systemd/system` claimants are config and units, with scheduled no longer one, which changes the accessor's signature. | Yes. |
| `inspectah-var-unit-grep-matches-comments-and-longer-names` | Out of scope. It concerns the `/var` backing search in `storage.rs`, which reads unit files for a different question. The unanchored pattern is still at `storage.rs:579`. | Yes, as narrowed in the item. |
| `inspectah-etc-systemd-user-has-no-symlink-rule` | Out of scope. User units are a non-goal (§ 3). | Not re-measured. |
| `inspectah-redaction-pseudo-paths-miss-systemd-comment-rule` | Out of scope, with one effect: once `SystemdTimer` holds no content, the `timer:` pseudo-path is left for generated units only. | Not re-checked. |

## 15. Phases

Each phase compiles, passes the workspace gates and is reviewable alone.

**Phase 1: unit kinds. Size L.**
`units.rs` types (§ 6), `UnitsInspector` over every kind from
`list-unit-files` (§ 7.1 source 1 and 2, § 7.2), the single inventory list,
the `dbus-org.` narrowing, mount findings and their advisories (§ 10.6), the
quoting helper and `--` at every `systemctl` site, the unit state section
moved after the config `COPY` (§ 10.7), the protected-unit guard (§ 10.3),
the runtime-update default (§ 10.4) if ratified, the rename, and schema 28.
Refine shows the existing section with every kind in one flat list.
Tests 1, 2, 5, 6, 8, 9, 10, 11. Local timers keep today's deferral to the
scheduled section, so their defects are unchanged by this phase.

Known limit of phase 1 alone: a local enabled socket whose config entry is
excluded fails the build, as a local service does today. Phase 2 closes it.

**Phase 2: one control per fact. Size M.**
The file check and `UnitFileNotStaged` (§ 10.2), every change in § 8, and
schema 29. Tests 3 and 4.

**Phase 3: the `/etc/systemd/system` scan. Size M.**
`.wants` and `.requires` (§ 7.3) if in scope, the default target (§ 7.4,
§ 10.5), drop-ins and shadows for every kind and the mask fix (§ 7.5), the
overlap contract text, and schema 30. Test 7.

**Phase 4: the Systemd units section. Size M, frontend.**
§ 11.2 after its own design pass.

Real-host runs 12 to 15 follow phase 1 and are repeated after phase 3.

## 16. Open questions

These need the project owner's decision. The spec recommends where it has a
view and does not decide.

1. **Template instances and hand-made links (§ 7.3).** In scope for phase 3,
   or deferred? They are static unit files in the sense of § 2, but they are
   also the part with the least evidence of real use and the only part that
   needs an `ln -s` render.
2. **Units that update packages at runtime (§ 10.4).** Excluded by default
   with a reason (recommended), carried with a FIXME, or a non-toggleable
   advisory?
3. **A mount unit's file.** Today a local `.mount` file is a config entry,
   included by default. It is inert without an enable, and wrong on the
   target if its mount point is a `/var` symlink. Leave it included (the
   spec's default, the smaller change), or exclude it by default with the
   advisory as the reason?
4. **A `graphical.target` default (§ 10.5).** Carry with a comment
   (the spec's default), or exclude by default when no display manager
   package is included?
5. **Does advisory-only cover vendor mount units?** `tmp.mount` enabled or
   masked against preset is a common hardening step. Under § 2 it gets an
   advisory with the line to add by hand. Confirm that is intended, or
   narrow advisory-only to mount units the administrator wrote.
6. **The rename (§ 9).** `services` to `units` in the snapshot JSON,
   `InspectorId` and the section types touches every consumer of the
   section (61 files name one of its fields today). Recommended, because the
   old name would be false. The alternative keeps the key and changes only
   the contents.
7. **Schema bumps.** One per phase (28, 29, 30), or hold the phases on one
   branch and bump once?
8. **The protected-unit list (§ 10.3).** `bootc-*`, `ostree-*` and
   `systemd-remount-fs.service` is the starting list. Confirm or amend.

## 17. The case against this design

**The strongest objection is that the real loss is narrow and this is a wide
change.** The measured user-visible defect is one socket, `cockpit.socket`,
that was enabled on validation hosts and whose state in the image nobody
checked. The fix proposed here renames a snapshot section, replaces its
types, bumps the schema up to three times, moves a Containerfile section and
rewires timers. A much smaller change does exist: add `--type=socket` and
`--type=path` to the existing call, widen `is_real_service` to three
suffixes, and gate the inventory lists on the same test. That is the
sockets-and-paths-first alternative. It closes the driving item, needs no
schema change beyond a semantic bump, and leaves timers, targets, slices and
mounts exactly as they are.

What that alternative does not do is the argument for this spec. It leaves
timers with two owners, so a disabled local timer still comes out enabled
and a vendor timer enabled against preset is still lost. It adds local
sockets to the set of units enabled before their file is copied, turning a
silent miss into a build failure unless the order moves anyway. And it
leaves "which collector owns which fact under `/etc/systemd/system`"
unwritten, which two independent assessments named as the cause of this
cluster of defects.

Other objections worth weighing:

- **Targets and slices may be noise.** If the survey finds no host where a
  target or slice differs from preset, collecting them buys nothing and adds
  rows to explain. The model handles them for free, but the UI and the
  documentation do not.
- **Advisory-only for mounts may be too blunt** for vendor mount units
  (question 5), and too gentle for a local mount file that is still copied
  (question 3).
- **The type split costs ergonomics.** `CarriedUnit` and `AdvisoryUnit` over
  `UnitName` is three name types where one enum and a `match` would do. The
  split makes one mistake impossible. If reviewers judge that mistake
  unlikely, a single `UnitKind` field with a render `match` is simpler.
- **Removing the timer toggle removes a control operators can see today**,
  even though it does nothing. An operator who wants a local timer out now
  excludes its file in the config section. If the frontend pass cannot make
  that route obvious, the removal is a usability regression.
- **Much of the systemd behavior this relies on was not measured.** Offline
  `enable` for sockets, `mask` on an absent unit, `--` before operands, the
  fragment a shadow's enable resolves. This subsystem has a record of sound
  reasoning from unverified premises.

Evidence that would change the design:

- The survey (run 14) shows timers, targets and slices never differ from
  preset on RHEL 9 and 10 outside local units. Then sockets and paths first
  is the right first step, with timers fixed separately as a scheduled-tasks
  defect.
- Run 12 shows `cockpit.socket` already enabled in the built image by some
  other route (a preset in the base image). Then the driving loss is
  smaller than stated, and the omission half is the only real defect.
- Run 13 shows a local service with no timer builds today. Then the order
  move is not needed in phase 1, and the source reading in § 4 is wrong
  about something that should be understood before anything else is built
  on it.
- The frontend design pass finds that one section for all kinds is harder
  to use than units in place. Then the data contract in § 11.1 stands and
  § 11.2 does not.
