# Unified systemd Unit Collection

## Metadata

| Field | Value |
|---|---|
| Date | 2026-11-13 |
| Status | Proposed. Scope and the rules marked with a name and date are decided (§ 2 and where each rule is written). The rest of §§ 5 to 12 is the spec author's design and is open to review. § 16 lists what still needs the project owner's call. |
| Scope | One model of systemd unit state for every unit type that exists as a static unit file, replacing the service-only collection. Collector, core types, snapshot schema, Containerfile render, the data contract for refine, and what the fleet merge does meanwhile. |
| Backlog items | Driving: `inspectah-socket-units-are-never-collected`. Folded in: `inspectah-local-timer-has-two-controls-and-neither-works`, `inspectah-timer-enable-lines-unquoted-and-trust-empty-units`, `inspectah-write-the-collector-overlap-contract` (the systemd half). Others in § 14. |
| Verified | Code citations read at `70aa8996` (2026-11-13); no source file has changed since. Line numbers are a convenience; the symbol name is the identifier. Claims about systemd behavior that were not checked on a host are marked as such and are listed for the real-host run in § 13. |

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
file is collected. Units produced by generators, and transient units, are
out. (Mark Russell, 2026-11-13)**

**A mount, automount or swap unit that a package owns and ships under the
vendor unit directory carries enable, disable and mask like a service. A
mount, automount or swap unit the administrator wrote is collected and is
advisory only: it never gets an automatic `systemctl` line. (Mark Russell,
2026-11-13)** `tmp.mount`, enabled or masked as a hardening step, is the case
the first half exists for.

**Enabled template instances are carried, each with a toggle like any other
unit. Links made by hand in a `.wants` or `.requires` directory are detected
and reported with the command that recreates them; they have no toggle and no
carried line. (Mark Russell, 2026-11-13)**

**The work lands on one feature branch and `main` takes one schema bump, 27
to 28. (Mark Russell, 2026-11-13)**

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
| `.target` | Carry state for targets that have `[Install]`. The default target is separate (§ 7.4) and is carried whenever the host set one. | Rare, and identical in mechanism. `default.target` is a link written by `systemctl set-default`, not an enable. |
| `.slice` | Carry state. In practice this means a mask; slices are otherwise `static`. Drop-ins are the common customization and follow § 7.5. | Uniform treatment costs nothing; a special case would. |
| `.mount`, `.automount`, `.swap`, package-owned under the vendor directory | Carry state, like a service. | Decided (§ 2). The unit is the distribution's, the same in the image as on the host, and switching it is configuration (`tmp.mount`). |
| `.mount`, `.automount`, `.swap`, written by the administrator | **Advisory only.** Collected, reported, never a `systemctl` line. The unit file stays a config entry. | Decided (§ 2). It is a statement about one host's storage, and on an image-based target several mount points are owned by bootc or are symlinks into `/var` (§ 10.6). § 6.2 says which units count as the administrator's. |
| `.device`, `.scope` | Excluded. | They never exist as unit files. Administrator drop-ins for them (`*.device.d`, `*.scope.d`) are still collected as drop-ins (§ 7.5). |
| Template (`name@.service`) | Not a state carrier itself. | A template has no enablement without an instance name. |
| Enabled template instance (`wg-quick@wg0.service`) | Carry state, when the configuration the instance reads is in the export (§ 7.3, § 10.8). Instances tied to one host's devices are advisory. | Decided (§ 2). |
| Hand-made link in `.wants`, `.requires` or `.upholds` | **Advisory only**, with the `systemctl add-wants` or `add-requires` line (§ 7.3, § 10.9). | Decided (§ 2). |
| Generated units | Excluded (§ 3). Administrator drop-ins on them are kept. | Decided. The generator's input is the fact, not its output. |
| Transient units, runtime and `linked` states | Excluded, with today's warning. | Decided, and they do not survive a reboot or need manual handling. |
| User units | Out of scope (§ 3). | Not part of this change. |
| Quadlets | Excluded. | Decided. |

The assumption that targets, slices and mounts rarely differ from preset is
untested, as is how often hosts enable template instances. § 13 specifies the survey that tests it.

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
    /// `Some` for `prefix@instance.suffix` with a non-empty instance.
    pub fn instance(&self) -> Option<InstanceName<'_>> { /* ... */ }
    /// True for `prefix@.suffix`.
    pub fn is_template(&self) -> bool { /* ... */ }
}

/// The parts of an instance name, borrowed from the `UnitName`.
pub struct InstanceName<'a> { pub prefix: &'a str, pub instance: &'a str }

impl InstanceName<'_> {
    /// `prefix@.suffix`, the unit file an instance is made from.
    pub fn template(&self) -> UnitName { /* ... */ }
}
```

The kind is derived from the name and never stored beside it, so a snapshot
cannot hold a `cockpit.socket` labelled as a service. The same holds for
"is an instance": it is read from the name. The character set is the one
`systemd.unit(5)` gives for unit names. A snapshot holding a name outside it
fails to load, which is the refusal an imported snapshot should get.

### 6.2 An administrator's mount unit cannot reach a `systemctl` line

The rule in § 2 depends on two facts, the kind and where the unit file came
from, so the type that guards a `systemctl` line holds both. A kind alone no
longer decides anything.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OrdinaryKind { Service, Socket, Path, Timer, Target, Slice }

/// Kinds that describe storage. Carried only when a package owns the unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StorageKind { Mount, Automount, Swap }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitKind { Ordinary(OrdinaryKind), Storage(StorageKind) }

/// A `UnitName` proved to be of an ordinary kind.
pub struct OrdinaryUnitName(UnitName);
/// A `UnitName` proved to be a mount, automount or swap unit.
pub struct StorageUnitName(UnitName);

/// Where the unit's file lives on the source host. For an instance, where
/// its template lives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "fragment", rename_all = "snake_case")]
pub enum UnitFragment {
    /// Only under `/usr/lib/systemd/system`. `owning_package` is `None`
    /// when no package owns the file or the RPM query failed.
    Vendor { owning_package: Option<String> },
    /// Only under `/etc/systemd/system`: a file the administrator wrote.
    Local { path: String },
    /// Both. The `/etc` file replaces the vendor one.
    Shadowed { path: String, owning_package: Option<String> },
}

/// The wire form of a unit and its fragment:
/// `{"name": "tmp.mount", "fragment": "vendor", "owning_package": "systemd"}`.
#[derive(Serialize, Deserialize)]
pub struct UnitSubject {
    pub name: UnitName,
    #[serde(flatten)]
    pub fragment: UnitFragment,
}

/// A unit inspectah may write a `systemctl` line for. The only subject a
/// state change holds, and the only one `systemctl_lines` accepts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "UnitSubject", into = "UnitSubject")]
pub enum CarriedUnit {
    /// Any fragment: a local service is carried with its file (§ 10.2).
    Ordinary { name: OrdinaryUnitName, fragment: UnitFragment },
    /// A storage unit under the vendor directory with a known owner.
    /// There is no field here that could hold an `/etc` path.
    PackagedStorage { name: StorageUnitName, owning_package: String },
}

/// Why a storage unit counts as the administrator's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminStorageFragment {
    Local { path: String },
    Shadowed { path: String, owning_package: Option<String> },
    /// Under the vendor directory, with no package known to own it.
    UnownedVendor,
}

/// A storage unit inspectah reports and never acts on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "UnitSubject", into = "UnitSubject")]
pub struct AdvisoryUnit { name: StorageUnitName, fragment: AdminStorageFragment }

pub enum Classified { Carried(CarriedUnit), Advisory(AdvisoryUnit) }

impl UnitSubject {
    /// Total: every name and fragment is exactly one of the two. This is
    /// the one place the rule in § 2 is written.
    pub fn classify(self) -> Classified { /* ... */ }
}
```

`classify` sends a storage name to `Carried` only for
`Vendor { owning_package: Some(_) }`. Both `try_from` impls call it, so the
collector and the snapshot loader apply the same rule and there is no second
copy of it.

What the types prevent:

- **An enable, disable or mask on an administrator's mount unit.**
  `UnitAction` is reachable only through `UnitStateChange::implied_action`,
  a `UnitStateChange` holds a `CarriedUnit`, and `systemctl_lines` takes
  `&[CarriedUnit]`. `CarriedUnit::PackagedStorage` has no variant or field in
  which a local path can sit, and there is no function from an
  `AdvisoryUnit` to a `UnitAction`. Render has no check to forget.
- **A hand-edited snapshot that moves a local mount into `state_changes`.**
  `{"name": "srv.mount", "fragment": "local", ...}` fails `try_from` and the
  snapshot does not load. `{"name": "tmp.mount", "fragment": "vendor",
  "owning_package": "systemd"}` loads.
- **A package-owned mount unit reported as an advisory.** The reverse
  `try_from` refuses it, so a vendor `tmp.mount` cannot lose its toggle by
  landing in `mount_findings`.
- **`.device` and `.scope` units.** Neither kind has a variant, so no value
  of either can exist.

Three cases fall on the advisory side by this design. They are the spec
author's reading of the decision, not part of it (§ 16 question 1):

- **`Shadowed`: a vendor mount unit with a full copy in `/etc`.** The `/etc`
  file replaces the vendor one, so `What=`, `Where=` and `Options=` are what
  the administrator wrote, and an enable would activate that content, which
  is what the advisory rule exists to stop. The package owns the name; it no
  longer owns what the unit does. The advisory carries the line to add by
  hand, so the cost of this side is one line and the cost of the other is an
  unreviewed mount in every host built from the image.
- **A vendor mount unit with a drop-in** stays `Vendor` and is carried. The
  fragment is where the unit file lives; a drop-in adds to a package's unit
  and is collected and toggled on its own (§ 7.5).
- **`UnownedVendor`.** The decision names a unit "owned by a package". A file
  under the vendor directory that no package claims, or whose owner could not
  be read, does not meet it. For storage kinds the collector therefore runs
  the owner lookup before it classifies.

### 6.3 State changes

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnitState { Enabled, Disabled, Masked }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitStateChange {
    pub unit: CarriedUnit,
    pub current_state: UnitState,
    #[serde(deserialize_with = "require_explicit_null")]
    pub default_state: Option<PresetDefault>,
    /// Administrator-written files under `/etc` this unit reads and cannot
    /// start without. Filled for template instances (§ 7.3).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_config: Vec<String>,
    /// Paths under `/var` that hold this unit's data. Filled for template
    /// instances (§ 7.3).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_paths: Vec<String>,
    #[serde(default)]
    pub disposition: FindingKind,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    pub aggregate: Option<AggregatePrevalence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_reason: Option<String>,
}
```

The fragment inside `unit` replaces `owning_package: Option<String>` and the
`shadow_type` and `shadow_rationale` pair on the state change. Today
`owning_package: None` means both "the administrator wrote this unit" and
"the RPM query failed", and Tier 1 emits either one unconditionally. The two
cases need different answers at render (§ 10.2), so the type separates them.
No production code that writes `shadow_type` on a state change was found by
grep at `70aa8996`; the plan confirms that before removing the fields.

`required_config` and `data_paths` are plain lists and not an
`Option<InstanceFacts>`, because "present exactly when the name is an
instance" would be an invariant the compiler cannot hold. An empty list is
true of every unit this design does not fill it for.

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
    /// The administrator's mount, automount and swap units (§ 6.5).
    pub mount_findings: Vec<MountUnitFinding>,
    /// Links made by hand, and instances tied to one host (§ 6.6).
    pub manual_links: Vec<ManualLink>,
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
    /// Present when the unit is enabled or masked.
    pub state_change: Option<(UnitState, Option<PresetDefault>)>,
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

A finding is recorded for every `.mount`, `.automount` or `.swap` unit that
classifies as `Advisory`: every regular file of those kinds under
`/etc/systemd/system`, and an unowned one under the vendor directory when it
is enabled or masked. A package-owned vendor unit of these kinds never
produces a finding. When its state differs from preset, or it is masked, it
is a `UnitStateChange` like a service.

### 6.6 Hand-made links

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkRelation { Wants, Requires, Upholds }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualLinkCause {
    /// The linked unit has no `[Install]` section.
    StaticUnit,
    /// The unit's `[Install]` section does not name this parent for this
    /// relation.
    NotInInstall,
    /// The `[Install]` value holds a specifier the collector does not
    /// expand, so it could not tell.
    Undetermined,
    /// An instance of a template tied to one host's devices (§ 7.3).
    HostDevice,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManualLink {
    pub relation: LinkRelation,
    /// The unit whose `.wants`, `.requires` or `.upholds` directory holds
    /// the link.
    pub parent: UnitName,
    pub unit: UnitName,
    pub cause: ManualLinkCause,
    pub aggregate: Option<AggregatePrevalence>,
}
```

A `ManualLink` holds two `UnitName`s and no `CarriedUnit`, and nothing turns
one into a `UnitAction`. It has no `disposition`. The decision that these
links get no toggle and no carried line (§ 2) is therefore a property of the
type, the same way it is for `MountUnitFinding`.

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
   the `*.wants/`, `*.requires/` and `*.upholds/` directories (§ 7.3).
4. The `[Install]` section of each unit file a link in those directories
   names, read from the file (§ 7.3).

Sources 3 and 4 exist because source 1 cannot see two things: an
administrator's link into a `.wants` directory for a unit with no
`[Install]` section, which `list-unit-files` reports as `static`, and
enabled template instances. `list-unit-files` is believed to list the
template only, with state `indirect` when an instance other than its
`DefaultInstance=` is enabled. That was not checked on a host; § 13 run 22
settles it. The design does not depend on the answer: instance names come
from the directories, and a name both sources report is recorded once.

### 7.2 What makes a state change

For each entry with a durable state whose name and fragment classify as a
`CarriedUnit` (§ 6.2):

- **Masked** is always a state change, as today.
- **`Vendor` or `Shadowed` fragment, and every `PackagedStorage` unit:** a
  state change when the state differs from the preset, as today. With no matching preset rule there is no
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

An entry that classifies as an `AdvisoryUnit` becomes a mount finding
(§ 6.5). A template name (`name@.service`) is not a state carrier and is
skipped, as today.

Units inspectah must not switch off are marked at collect and refused at
render (§ 10.3). Units that update packages at runtime are collected and
excluded by default (§ 10.4).

### 7.3 `.wants`, `.requires`, `.upholds` and template instances

The collector lists each `*.wants/`, `*.requires/` and `*.upholds/`
directory under `/etc/systemd/system` and reads each link. Only `/etc` is
read. Links under `/usr/lib/systemd/system` belong to packages and are in
the image already. Links a generator makes live under `/run`, which is why
`serial-getty@ttyS0.service` on a host with a serial console is not seen:
it is generator output, and § 3 excludes it. `.upholds/` is included because
`UpheldBy=` produces links the same way `WantedBy=` does. Which systemd
release first honors it, and whether RHEL 9 has it, was not checked; § 13
run 22 records it.

**Each link is either enable-made or hand-made.** For a link named `U` in
the directory of parent `P`:

1. If `P` or `U` does not parse as a `UnitName` (a `.device` parent, for
   one), the collector records a structured warning and nothing else.
2. The collector finds the unit file for `U`, or for its template when `U`
   is an instance, under `/etc/systemd/system` and then
   `/usr/lib/systemd/system`, and reads its `[Install]` section.
3. The link is **enable-made** when `P` is named by the key for that
   directory: `WantedBy=` for `.wants`, `RequiredBy=` for `.requires`,
   `UpheldBy=` for `.upholds`. `%i` and `%p` in the value are expanded for
   the instance first. `systemctl enable` on the source host would have
   produced exactly this link, so `systemctl enable` in the image reproduces
   it.
4. Otherwise the link is **hand-made**, and the collector records a
   `ManualLink` (§ 6.6) with the cause: `StaticUnit` when there is no
   `[Install]` section, `NotInInstall` when the section does not name `P`
   for this relation, `Undetermined` when the value holds a specifier other
   than `%i` or `%p`. Undetermined falls on the advisory side on purpose:
   an advisory for a link that `enable` would have made costs the operator
   one line, and a carried enable for a link it would not have made changes
   what the image starts.

`Alias=` links sit directly under `/etc/systemd/system`, not in these
directories. § 7.5 already records nothing for them, and enabling the unit
recreates them.

**What an enable-made link becomes:**

- `U` is not an instance: nothing. `list-unit-files` reported the unit as
  enabled and § 7.2 has it.
- `U` is an instance: a `UnitStateChange` of `Enabled`, with the template's
  fragment and owner, under the same rule as § 7.2 applied to the instance
  name. An instance linked from two parents is one state change.

A masked instance needs nothing new: its `/dev/null` link under
`/etc/systemd/system` is found by the mask scan and its name parses. An
instance that is disabled leaves no trace on the host and is not recorded.

**Instances tied to one host's devices are never carried.** An instance of
`systemd-cryptsetup@.service` or `systemd-fsck@.service` names a volume or
an escaped device path of the source host. Whatever its `[Install]` section
says, such a link is recorded as a `ManualLink` with cause `HostDevice`.
This is a fixed list, one named constant, and it holds those two templates.
A template that is tied to hardware and is not on the list is carried like
any other instance.

**What an instance needs.** An instance usually reads a file named for it,
and the enable is useless or harmful without that file. The collector fills
two lists on the state change from two sources each:

| List | Derived from the template's unit file | Fixed table (named constant) |
|---|---|---|
| `required_config` | Every path under `/etc` in the file that holds `%i` or `%I`, expanded for the instance. An `EnvironmentFile=` value with the `-` prefix is optional to systemd and is left out. | `wg-quick@` reads `/etc/wireguard/%i.conf`; `openvpn-client@` reads `/etc/openvpn/client/%i.conf`; `openvpn-server@` reads `/etc/openvpn/server/%i.conf`. |
| `data_paths` | Every path under `/var` in the file that holds `%i` or `%I`, and `/var/lib/<value>` for a `StateDirectory=` value that holds one. | `dirsrv@` keeps `/var/lib/dirsrv/slapd-%i`. |

A path is kept only when it exists on the source host. A `required_config`
path is kept only when no package owns it: a package's file arrives with the
package, and the presence tiers already answer for that. The table exists
because the derivation cannot see the motivating case. `wg-quick@.service`
runs `wg-quick up %i` and never names `/etc/wireguard`. The table paths were
written from knowledge of those packages and were not read from an installed
unit; § 13 run 22 checks them. An instance with neither list filled is
carried on its unit file alone, and its row carries an `attention_reason`
saying inspectah does not know what the instance reads. § 10.8 says what
render does with both lists and what this does not cover.

The config walk keeps excluding the `.wants` and `.requires` directories.
They hold links, not content, and the collector now turns them into state or
into an advisory, which is the answer to the question the overlap item asked
about that exclusion.

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
does not go through the preset comparison. It is recorded as included
whatever the target is (§ 10.5).

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
change and mount finding, and for an instance it asks about the template's
file. For storage kinds the lookup runs before classification, since the
answer decides which side of § 6.2 the unit falls on. A failed query for a
vendor mount unit therefore yields an advisory, not a carried line.

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

- **The section is renamed.** `InspectionSnapshot.services:
  Option<ServiceSection>` (`crates/core/src/snapshot.rs:33`) becomes `units:
  Option<UnitSection>`. The JSON key changes from `services` to `units`, and
  `InspectorId::Services` and `SectionData::Services` are renamed to match.
  (Mark Russell, 2026-11-13) A section that holds sockets and timers under
  the name "services" would be a name the code does not satisfy. Roughly 60
  Rust files under `crates/` name the section or one of its fields today.
- `ServiceStateChange`, `ServiceUnitState`, `ServiceAction` and
  `ServiceSection` become `UnitStateChange`, `UnitState`, `UnitAction` and
  `UnitSection`, with the field changes in § 6.
- `SystemdTimer` loses four fields (§ 8).
- **One bump.** `SCHEMA_VERSION` and `MIN_SCHEMA` move together from 27 to
  28, once, and every phase in § 15 lands on one feature branch that merges
  to `main` when all of them are done. (Mark Russell, 2026-11-13) The first
  phase on the branch sets 28. Later phases change the serialized types
  again without a further bump, so a snapshot written by one phase of the
  branch is not valid input to a later one. Nothing outside the branch ever
  reads such a snapshot, and fixtures on the branch are regenerated by the
  phase that changes them.
- No shim, no migration: a version 27 snapshot is re-scanned. The bump
  carries the refusal test through all three loaders that
  `process-docs/skills/snapshot-schema-versioning.md` requires, with a
  payload that shows why the closed version is refused. For 27 that payload
  is a snapshot with a `services` section and no `units`: it parses, and
  every socket and timer state the host had is absent from it with nothing
  to say so. The test is written in the first phase and re-run against the
  final types before the merge.

## 10. Render

### 10.1 Lines

One function, the successor of `render_service_intent`, produces every
`systemctl` line for collected units. It emits, in this order, under one
section header:

```
# === Systemd Unit State ===
# Omitted: cockpit.socket (package 'cockpit-ws' was excluded)
# Omitted: wg-quick@wg0.service (its configuration /etc/wireguard/wg0.conf holds a redacted value)
RUN systemctl set-default -- 'multi-user.target'
RUN systemctl enable -- \
    'openvpn-client@site.service' \
    'cockpit.socket' \
    'myapp.path' \
    'report.timer' \
    'tmp.mount'
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
- A template instance renders with its instance name, in the same `RUN` as
  every other unit of its verb.
- A package-owned mount, automount or swap unit renders in the same `RUN`
  too. That `systemctl enable` and `mask` work for a mount unit at build
  time, with no running manager, is not verified here; § 13 run 22 checks
  it with `tmp.mount`.
- The same function emits the comment-only blocks: mount advisories
  (§ 10.6), instance data advisories (§ 10.8) and hand-made link advisories
  (§ 10.9). None of them is an instruction.

### 10.2 An enable needs a unit file

Before a unit reaches an enable or disable line, the plan decides whether its
file will be in the image. For an instance the file is its template. A
`PackagedStorage` unit is a `Vendor` fragment whose package is known:

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

The list is `bootc-*`, `ostree-*` and `systemd-remount-fs.service`. (Mark
Russell, 2026-11-13) It is one named constant. The collector sets
`attention_reason` on the same units so refine shows why. An `enable` on
these names is carried.

### 10.4 Units that update packages at runtime

`dnf-automatic.timer`, its `-install`, `-download` and `-notifyonly`
variants, and `packagekit-offline-update.service` change or prepare to
change installed packages on a running host. `/usr` is read-only on the
target and the update path is a new image.

**These units are collected, excluded by default with a stated reason, and
the operator can include them in refine. (Mark Russell, 2026-11-13)**

The collector records each as an ordinary state change with
`disposition: Actionable { include: false }` and an `attention_reason`
saying the unit updates packages in place, which the target does not
support, and naming `bootc-fetch-apply-updates.timer` as the equivalent.
The row is not locked. Including one is right for the `-notifyonly` and
`-download` variants. While a unit is excluded the Containerfile carries one
comment naming it and the reason:

```
# Omitted: dnf-automatic.timer (updates packages in place, which an image-based host does not support; excluded by default)
```

The names are one named constant: the glob `dnf-automatic*.timer` and
`packagekit-offline-update.service`.

### 10.5 The default target

**`RUN systemctl set-default -- '<target>'` is emitted whenever the source
host set a default target, with no test of what the target is and no
comment beside it. (Mark Russell, 2026-11-13)** A host that boots to
`graphical.target` produces an image that does. `default_target` is
recorded as included and has a toggle like any other row. The file check in
§ 10.2 applies, since `set-default` on a target whose file is not in the
image cannot succeed: a `Local` target whose config entry is excluded is
omitted with the `UnitFileNotStaged` reason.

### 10.6 Mount, automount and swap advisories

For each `MountUnitFinding`, which is to say each mount, automount or swap
unit the administrator wrote (§ 6.2), comment lines and nothing else. The
first line states the fact. Further lines are added per hazard, computed from
`mount_point`:

| Condition | Text added |
|---|---|
| Always | `# Advisory: <unit> is <state> on the source host. inspectah does not enable, disable or mask a mount, automount or swap unit the administrator wrote.` |
| The unit's file is an included config entry | `#   Its unit file is copied into the image and does nothing there until the unit is enabled.` |
| `mount_point` is under `/usr` | `#   /usr is read-only on the target. This mount cannot be satisfied.` |
| `mount_point` is `/`, `/boot`, `/boot/efi`, `/sysroot` or `/var` | `#   bootc and ostree own this mount point on the target.` |
| `mount_point` is, or is under, `/home`, `/srv`, `/opt`, `/mnt` or `/root` | `#   <path> is a symlink into /var on the target. The unit must be renamed for the resolved path (for /srv: var-srv.mount with Where=/var/srv).` |
| A mount or swap with `nofail` absent | `#   No nofail option. If the device is missing, every host built from this image boots to emergency mode.` |
| The state is enabled or masked | `#   To carry it, add after the configuration COPY: RUN systemctl <verb> -- '<unit>'` |

The mount-point lists are named constants beside the existing target-layout
knowledge. The claims in the table about the target layout come from earlier
analysis and from `inspectah-srv-mount-units-must-target-var-srv`; they were
not re-measured for this spec.

**The unit file itself stays a config entry, included by default, under the
config walk's control. No enable line is written for it, and the advisory
is where the hazards are explained. (Mark Russell, 2026-11-13)** Excluding
the file is the config row's toggle, as for any other file.

A package-owned mount unit gets none of this. It is a state change, it
renders in § 10.1, and it has no advisory.

### 10.7 Order

The unit state section moves from step 2 to directly after the Configuration
Files section. Every `COPY` that can stage a unit file precedes it: drop-ins
and shadows, generated timers, and the config tree. The drop-ins `COPY`
stays where it is. This closes the local-service ordering defect in § 4 and
changes which fragment a full shadow's enable resolves.

### 10.8 An instance enable needs its configuration

An enable for `wg-quick@wg0.service` with no `/etc/wireguard/wg0.conf` in
the image builds, and then the unit fails on every boot. So an instance's
enable is carried only when every path in its `required_config` is in the
export as the host had it. The test runs after § 10.2 and uses the same
sources of truth:

| Condition on a `required_config` path | Result |
|---|---|
| A `config.files` entry at the path (for a directory, at or under it) is excluded | Omitted: `its configuration <path> is excluded from the export` |
| No entry exists there, or one has `content_captured()` false | Omitted: `its configuration <path> was not captured` |
| `snapshot.redactions` holds a finding for the path, or a path under it, whose kind is `Inline` or `Excluded` | Omitted: `its configuration <path> holds a redacted value` |
| None of the above, for every path | The enable is carried |

```rust
/// A template instance whose configuration is not in the export as the
/// source host had it.
InstanceConfigNotStaged { path: String, gap: ConfigGap },

pub enum ConfigGap { Excluded, NotCaptured, Redacted }
```

"Included" is the stager's own list (`staged_sources`,
`crates/pipeline/src/render/configtree.rs:257`), as in § 10.2. "Redacted"
reads the findings the redaction pass already wrote
(`InspectionSnapshot.redactions`, `crates/core/src/snapshot.rs:49`), so the
gate has no secret detection of its own and cannot disagree with it.

That agreement is what makes the WireGuard case come out right. A
`PrivateKey =` line matches the WireGuard pattern
(`crates/pipeline/src/redaction/patterns.rs:132-139`). The pattern has no
replacement template, so `build_replacement`
(`crates/pipeline/src/redaction/engine.rs:459`) swaps the whole match for a
token, and `scan_content` (`engine.rs:608`) records an `Inline` finding
against the file's path. The file is still an included config entry and is
still staged, now without its key. Under default redaction
`wg-quick@wg0.service` is therefore always omitted, with the reason on the
`# Omitted:` line and on its refine row. That is the intended result: the
key has to reach the host by a route other than the image, and the enable
belongs with that step.

What the gate covers and what it does not:

- **Covered:** an instance whose template names its `/etc` path with the
  instance specifier, and the three templates in the fixed table (§ 7.3).
- **Not covered:** configuration found by a program's own convention for a
  template not in the table; configuration outside `/etc`; a file the listed
  configuration points to in turn (a key file named inside a WireGuard or
  OpenVPN configuration); and a secret the redaction patterns miss, which is
  carried in the file as it would be for any config entry. An instance with
  an empty `required_config` is carried on its unit file alone, with the
  `attention_reason` from § 7.3.
- **Not verified:** that the config walk records
  `/etc/wireguard/wg0.conf` on a stock host, and how a scan that retains
  credentials by operator choice records its findings. The plan confirms
  the second in source; § 13 run 22 measures the first.

**Data under `/var`.** When an instance's enable is carried and its
`data_paths` is not empty, one comment per instance precedes the `RUN`
lines:

```
# Advisory: dirsrv@example.service keeps data under /var/lib/dirsrv/slapd-example. The image does not carry it. Migrate that data before the unit first starts.
```

The row's `attention_reason` carries the same sentence. inspectah knows
about instance data only through the derivation and the fixed table in
§ 7.3, which holds `dirsrv@` and nothing else. An instance whose template
does not name its `/var` path and is not in the table gets no advisory.

### 10.9 Hand-made link advisories

For each `ManualLink`, comment lines and nothing else:

| Cause | Text |
|---|---|
| `StaticUnit` | `# Advisory: <parent>.<relation>/<unit> is a link made by hand on the source host (<unit> has no [Install] section). Not carried.` |
| `NotInInstall` | `# Advisory: <parent>.<relation>/<unit> is a link made by hand on the source host (the unit's [Install] section does not name <parent>). Not carried.` |
| `Undetermined` | `# Advisory: <parent>.<relation>/<unit> could not be matched to the unit's [Install] section. Not carried.` |
| `HostDevice` | `# Advisory: <unit> is tied to a device of the source host. Not carried.` |

For the first three causes, with relation `Wants` or `Requires`, one more
line gives the command:

```
#   To carry it, add after the configuration COPY: RUN systemctl add-wants -- '<parent>' '<unit>'
```

with `add-requires` for a `.requires` link. An `Upholds` link gets no
command line, since `systemctl` has no verb that makes one; the text says
to add `Upholds=` to a drop-in for the parent instead. A `HostDevice` link
gets no command at all.

`add-wants` is offered in place of a literal `ln -s` for two reasons. It
resolves the unit's path inside the image, so the link cannot point at a
fragment the image does not load, which is the hazard recorded in
`inspectah-full-shadow-wants-link-points-at-the-vendor-path`. And it is
expected to fail the build when the unit is missing, where a dangling link
would build and do nothing. Both are expectations from the documented
behavior of `systemctl`. Whether `add-wants` runs at all at build time,
with no manager to talk to, was not checked; § 13 run 22 checks it.

Both names are proved `UnitName`s and are single-quoted like every other
operand.

## 11. Refine

### 11.1 Data contract

The view response carries, in place of `service_states`:

| Field | Content |
|---|---|
| `unit_states[]` | One per `UnitStateChange`, package-owned mount units and template instances included: `unit`, `kind` (derived, lowercase), `instance_of` (the template name, derived, when the unit is an instance), `current_state`, `default_state`, `fragment` (`vendor`, `local`, `shadowed`), `unit_file_path` when not vendor, `owning_package`, `required_config[]`, `data_paths[]`, `include`, `locked`, `triage`, `attention_reason`, and `omitted` with the `OmissionReason::describe` text when the render plan omits the unit. A runtime package updater (§ 10.4) is an ordinary row with `include: false`. |
| `unit_dropins[]` | As `service_dropins` today, with `kind` of the parent. A drop-in whose parent has no state change is still listed, since a unit at its preset has a drop-in and no parent row. |
| `default_target` | `target`, `include`, `locked`, or absent. |
| `mount_findings[]` | `unit`, `kind`, `state`, `mount_point`, `what`, `fragment` (`local`, `shadowed`, or `vendor` for an unowned one), and `advisories[]`, the same strings § 10.6 renders. No `include`: these rows have no toggle. |
| `manual_links[]` | `relation`, `parent`, `unit`, `cause`, and `advisories[]`, the same strings § 10.9 renders. No `include`. |
| `scheduled_tasks` timers | Each row gains `unit` (`<name>.timer`) and `unit_row` (whether `unit_states` holds it), so the row can link to its control. No `include`. |

Operations: `ItemId::Service { unit }` becomes `ItemId::Unit { unit }` and
keeps its cascade to drop-ins. `ItemId::DefaultTarget` is added.
`ItemId::SystemdTimer` is removed. `SetInclude` on a mount finding or a
hand-made link does not exist, so there is nothing to refuse.

The omission reasons and advisory strings have one source each, in the
pipeline crate, as `OmissionReason::describe` is today.

### 11.2 Recommended section design

**This needs the frontend's own design pass. It is a recommendation.**

One "Systemd units" section replaces "Services". Rows are grouped by kind,
and the section shows only units that differ from preset or carry a drop-in,
which is what `unit_states` and `unit_dropins` hold. Services, sockets,
timers and paths get their own groups, with a template instance in the
group of its kind; targets, slices, mount units and the mount and link
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
- `mount_findings` merge by unit name, and `manual_links` by relation,
  parent and unit. Neither has an include flag, so narrowing does nothing
  to them.
- A template instance is a `UnitStateChange` and merges as one. Its
  `required_config` and `data_paths` are unioned across hosts.
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
   lines including the `/var` rename and the unit-file line, a staged unit
   file, and no `systemctl` instruction line naming it.
6. **Type-level.** A snapshot whose `state_changes` holds a mount name with
   a `local` fragment fails to load; so does one with a `shadowed` fragment,
   and one with a `vendor` fragment and a null owner. The same name with a
   `vendor` fragment and an owner loads. A package-owned mount in
   `mount_findings` fails to load. A round-trip property test over
   `UnitSubject` shows `classify` is total and that serializing either side
   and loading it returns the same side. A compile-fail or signature test
   shows `systemctl_lines` takes `CarriedUnit` only.
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
12. **Carried instance.** A `multi-user.target.wants/openvpn-client@site.service`
    link, a vendor template whose `[Install]` names `multi-user.target`, and
    an included `/etc/openvpn/client/site.conf` with no redaction finding.
    Assert one `UnitStateChange` named for the instance with the template's
    owner, and the instance name in `RUN systemctl enable`. A second case
    with `dirsrv@example.service` and an existing data directory asserts the
    data advisory beside a carried enable.
13. **Instance whose configuration is not in the export.** Three cases, each
    asserting an `# Omitted:` line with the matching wording and no enable
    naming the instance: the config entry excluded through `RefineSession`
    (and the enable returning when it is included again); the content not
    captured; and `wg-quick@wg0.service` with a `PrivateKey =` line taken
    through the real redaction pass, which also asserts the staged file
    holds no key.
14. **Hand-made link.** A `multi-user.target.wants/backup.service` link to
    a unit with no `[Install]` section. Assert one `ManualLink` with cause
    `StaticUnit`, the advisory with the `add-wants` line, no `RUN`
    instruction naming the unit, and no include field on its refine row.
    Table cases for `NotInInstall`, a `.requires` link (`add-requires`), an
    unexpanded specifier (`Undetermined`), and `systemd-fsck@` (`HostDevice`,
    no command line).
15. **Vendor and local mount units.** `tmp.mount` owned by a package and
    enabled against its preset yields a state change and appears in
    `RUN systemctl enable`; masked, it appears in `RUN systemctl mask`. A
    local `data.mount`, enabled, yields an advisory and no instruction. A
    `tmp.mount` with a full copy in `/etc` yields an advisory and no
    instruction.
16. **Runtime package updater.** `dnf-automatic.timer` enabled against its
    preset yields a state change with `include: false` and the stated
    reason, the `# Omitted:` comment, and no enable. `SetInclude` through
    `RefineSession` then yields the enable line.
17. **Default target.** A `default.target` link to `graphical.target`
    yields `RUN systemctl set-default -- 'graphical.target'` with no
    comment beside it. No link yields no line.

Real-host runs, specified here and not run for this spec. **Do not use the
CentOS9 or fedora44 VMs for these runs.** § 15 says when each happens.

18. **`cockpit.socket` end to end.** On a RHEL 9 host and a RHEL 10 host:
    `systemctl enable --now cockpit.socket`, scan, build the exported
    Containerfile, and confirm `systemctl is-enabled cockpit.socket` reads
    `enabled` in the built image.
19. **Same run, four more checks.** A local service with no paired timer
    builds (the § 4 ordering defect, before and after). A full shadow's
    wants-link target and the fragment `systemctl cat` reports. A unit with
    `\x2d` in its name survives `RUN systemctl enable -- '...'`. `systemctl
    mask` on a unit absent from the image succeeds at build time.
20. **Survey.** `systemctl list-unit-files --no-pager` on RHEL 9 and RHEL 10
    hosts, stock and after the driftify standard profile. Count, per unit
    type, units whose state differs from preset, and count the instance
    links and hand-made links under `/etc/systemd/system`. This tests the assumption
    that targets, slices and mounts rarely differ, and sizes the collapsed
    group in § 11.2. Record the result in
    `process-docs/skills/real-host-validation-runs.md`.
21. **Stock-host false positives.** Scan a stock host of each release
    before a drifted one, and review every row the new kinds add, every
    hand-made link advisory, and every instance.
22. **Instances, links and mount units.** On a RHEL 9 host and a RHEL 10
    host. Parts (a) to (f) need no inspectah build.
    a. `systemctl enable wg-quick@wg0 && systemctl list-unit-files
       'wg-quick@*'`. Record which names are listed, with which state, and
       where the link was made.
    b. Read the installed `wg-quick@.service`, `openvpn-client@.service`,
       `openvpn-server@.service` and `dirsrv@.service`. Record the path each
       really reads, to confirm or correct the two fixed tables in § 7.3,
       and what the derivation would produce for each.
    c. In a build from the bootc base image: `RUN systemctl add-wants --
       'multi-user.target' '<unit with no [Install]>'`. Record whether it
       succeeds with no manager running, where the link points, where it
       points when the unit has a full shadow in `/etc`, and whether the
       build fails when the unit is absent. The same for `add-requires`.
    d. In the same build: `RUN systemctl enable -- 'tmp.mount'`, and
       `mask`. Record the result and the state in the built image.
    e. `systemctl --version`, and whether a unit with `UpheldBy=` produces
       an `.upholds/` link on enable.
    f. On a host with a serial console, where the
       `serial-getty@ttyS0.service` link lives.
    g. With the branch build: scan a host with an enabled
       `wg-quick@wg0` whose configuration holds a key. Record whether the
       config walk recorded the file and that the enable is omitted.
    h. With the branch build: an instance with clean configuration, carried
       end to end, reads `enabled` in the built image; and a
       `graphical.target` default builds with no display manager package in
       the image.

## 14. Backlog items

Each item was read in full before the design, and its claims checked
against `70aa8996`.

| Item | Result | Still right? |
|---|---|---|
| `inspectah-socket-units-are-never-collected` | Closed by phase 1 and run 18. | Yes. All four cited sites confirmed. |
| `inspectah-local-timer-has-two-controls-and-neither-works` | Closed by phase 2. Control 1 goes with the second writer; control 2 is removed, and its cause is in § 4. | Yes, with stale line numbers: the writer is now `configtree.rs:440-449`. |
| `inspectah-timer-enable-lines-unquoted-and-trust-empty-units` | Closed by phases 1 (quoting) and 2 (file check, single writer). | Partly stale. Since `2c2e52aa` the names pass `is_shell_operand`, so `#skip` and a leading option are refused. They are still bare, with no `--`, and glob characters pass. Items 2 and 3 stand. |
| `inspectah-write-the-collector-overlap-contract` | Partly covered. § 8 and § 7.5 are the ownership table for `/etc/systemd/system`, the `*.wants/*` exclusion is answered in § 7.3, and the missing `scheduled_owned_paths` is made unnecessary. Phase 3 writes this part into `process-docs/` and fixes `docs/reference/inspector-coverage.md`. Remaining: the general rules document, and the non-systemd claimants (quadlets, repo files, GPG keys). | Yes. |
| `inspectah-masked-units-are-recorded-as-full-shadows` | Closed by phase 3 (§ 7.5). The alias question is answered: an alias produces nothing. | Yes. |
| `inspectah-full-shadow-wants-link-points-at-the-vendor-path` | Partly covered. The order change in § 10.7 changes what the enable resolves, and the item is the reason § 10.9 offers `add-wants` and not a literal link. The measurement the item asks for is run 19, and run 22c repeats it for `add-wants`. | Yes; still unmeasured. |
| `inspectah-srv-mount-units-must-target-var-srv` | Partly covered. The advisory in § 10.6 names the problem and the rename. Remaining: the unit file is still carried as config by default, which is decided (§ 10.6); there is no automatic rewrite, and fstab lines and the audit report are untouched. | Yes. |
| `inspectah-etc-unit-files-accessor` | Out of scope. Its blocker moves: after phase 3 the `/etc/systemd/system` claimants are config and units, with scheduled no longer one, which changes the accessor's signature. | Yes. |
| `inspectah-var-unit-grep-matches-comments-and-longer-names` | Out of scope. It concerns the `/var` backing search in `storage.rs`, which reads unit files for a different question. The unanchored pattern is still at `storage.rs:579`. | Yes, as narrowed in the item. |
| `inspectah-etc-systemd-user-has-no-symlink-rule` | Out of scope. User units are a non-goal (§ 3). | Not re-measured. |
| `inspectah-redaction-pseudo-paths-miss-systemd-comment-rule` | Out of scope, with one effect: once `SystemdTimer` holds no content, the `timer:` pseudo-path is left for generated units only. | Not re-checked. |

## 15. Phases

**All phases land on one feature branch, and the branch merges to `main`
once, carrying the single schema bump. (Mark Russell, 2026-11-13)** Each
phase is still its own unit of work: it compiles, passes the workspace gates
at its last commit, and is reviewed alone before the next phase starts.
Phase 1 sets schema 28 (§ 9). `main` is merged into the branch between
phases, so the final merge is not the first time the two meet.

**Phase 1: unit kinds. Size L, at the upper end.**
`units.rs` types (§ 6) with the classification by kind and fragment,
`UnitsInspector` over every kind from `list-unit-files` (§ 7.1 sources 1 and
2, § 7.2) with the owner lookup ahead of classification for storage kinds,
the single inventory list, the `dbus-org.` narrowing, package-owned mount
units as state changes, local mount findings and their advisories (§ 10.6),
the quoting helper and `--` at every `systemctl` site, the unit state
section moved after the config `COPY` (§ 10.7), the protected-unit guard
(§ 10.3), the runtime-updater default (§ 10.4), the rename, and schema 28.
Refine shows the existing section with every kind in one flat list.
Tests 1, 2, 5, 6, 8, 9, 10, 11, 15, 16. Local timers keep today's deferral
to the scheduled section, so their defects are unchanged by this phase.

Known limit of phase 1 alone: a local enabled socket whose config entry is
excluded fails the build, as a local service does today. Phase 2 closes it.

**Phase 2: one control per fact. Size M.**
The file check and `UnitFileNotStaged` (§ 10.2), and every change in § 8.
Tests 3 and 4.

**Phase 3: the `/etc/systemd/system` scan. Size L.**
The link scan and the `[Install]` reader (§ 7.1 sources 3 and 4, § 7.3),
the enable-made and hand-made classification, `ManualLink` with the
host-device list and the advisories (§ 10.9), the default target (§ 7.4,
§ 10.5), drop-ins and shadows for every kind and the mask fix (§ 7.5), and
the overlap contract text. Enabled instances are collected as state changes
in this phase and every one of them is omitted at render with a placeholder
reason, `template instances are not carried yet`, so each is named in an
`# Omitted:` line and on a refine row. Tests 7, 14, 17.

**Phase 4: carried instances. Size M.**
`required_config` and `data_paths` with their derivation and fixed tables
(§ 7.3), the configuration gate and `InstanceConfigNotStaged`, and the data
advisory (§ 10.8). The placeholder omission from phase 3 is removed.
Tests 12, 13.

**The floor for instances** is the end of phase 3: every enabled instance
named in an advisory line, none carried. If phase 4 slips, the branch is
complete without it only by the project owner's call (§ 16 question 3).

**Phase 5: the Systemd units section. Size M, frontend.**
§ 11.2 after its own design pass.

**Real-host runs all happen on the branch, before the merge.**

| When | Runs | Why then |
|---|---|---|
| Before phase 3 is planned | 22a to 22f | They need no inspectah build, and phases 3 and 4 are designed on their answers (what `list-unit-files` lists, whether `add-wants` works at build time, the table paths). |
| After phase 1 | 18, 19, 20, 21 | The results can overturn premises in § 4 and § 17 while only one phase rests on them. |
| At the branch tip, after the last phase | 18, 19, 21 and 22 in full | This is the merge gate. The branch does not merge to `main` until they pass. |

Results go into `process-docs/skills/real-host-validation-runs.md` on the
branch, so the merge carries the evidence with the code.

## 16. Open questions

These need the project owner's decision. The spec says what it does today
and does not decide.

1. **Which mount units count as the administrator's (§ 6.2).** The decision
   covers a unit a package owns under the vendor directory, and a local
   one. The spec puts two further cases on the advisory side: a vendor
   mount unit with a full copy in `/etc`, and a unit under the vendor
   directory with no known owner. The first matters if hosts harden
   `tmp.mount` by copying the unit to `/etc` and editing it, which some
   hardening guides are believed to have recommended; that was not checked.
   Confirm, or carry a shadowed mount unit when a package owns its name.
2. **The fixed lists (§ 7.3).** Instance configuration: `wg-quick@`,
   `openvpn-client@`, `openvpn-server@`. Instance data: `dirsrv@`. Tied to a
   host's devices: `systemd-cryptsetup@`, `systemd-fsck@`. Confirm or amend
   the starting contents. And confirm the default for an instance inspectah
   knows nothing about: the spec carries its enable with an attention note;
   the alternative is an advisory until a template is added to a list.
3. **What the branch must hold to merge (§ 15).** All phases land on the
   branch. Two cases need a ruling. Phase 5 depends on a frontend design
   pass, and the driving fix for `cockpit.socket` waits for it; phase 1
   already shows every kind in the existing flat list. And if phase 4
   slips, the instance floor is in place after phase 3. May the branch
   merge after phase 4 with the flat list, or after phase 3 with the floor?

## 17. The case against this design

**The strongest objection is that the real loss is narrow and this is a wide
change.** The measured user-visible defect is one socket, `cockpit.socket`,
that was enabled on validation hosts and whose state in the image nobody
checked. The fix proposed here renames a snapshot section, replaces its
types, bumps the schema, moves a Containerfile section, rewires timers, and
reads `[Install]` sections to tell an instance from a hand-made link. A much
smaller change does exist: add `--type=socket` and `--type=path` to the
existing call, widen `is_real_service` to three suffixes, and gate the
inventory lists on the same test. That is the sockets-and-paths-first
alternative. It closes the driving item, needs no schema change beyond a
semantic bump, and leaves timers, targets, slices, mounts and instances
exactly as they are.

What that alternative does not do is the argument for this spec. It leaves
timers with two owners, so a disabled local timer still comes out enabled
and a vendor timer enabled against preset is still lost. It adds local
sockets to the set of units enabled before their file is copied, turning a
silent miss into a build failure unless the order moves anyway. And it
leaves "which collector owns which fact under `/etc/systemd/system`"
unwritten, which two independent assessments named as the cause of this
cluster of defects.

Other objections worth weighing:

- **One branch holds the driving fix back.** The socket fix is complete at
  the end of phase 1 and reaches `main` only when the last phase does. A
  long-lived branch also drifts: roughly 60 files change in the rename, and
  every unrelated change to them on `main` is a conflict to resolve. The
  single bump buys one schema break for users in place of several; the
  price is that nothing ships early.
- **Targets and slices may be noise.** If the survey finds no host where a
  target or slice differs from preset, collecting them buys nothing and adds
  rows to explain. The model handles them for free, but the UI and the
  documentation do not.
- **The mount rule now turns on the RPM owner lookup.** A package-owned
  mount unit is carried and an unowned one is not, so a failed query moves
  a `tmp.mount` from a carried line to an advisory. That is the safe
  direction, and it is also a place where the same host can give two
  answers on two scans.
- **The type split costs ergonomics.** `CarriedUnit` is now an enum over two
  name types and a fragment, with a wire form and a classifier beside it,
  where one struct and a render `match` would do. The split makes one
  mistake impossible. If reviewers judge that mistake unlikely, a single
  `UnitKind` field with a render `match` is simpler.
- **The instance gate knows three templates by name.** For any other
  template whose unit file does not name its `/etc` path, the enable is
  carried with no gate, and the image may enable a unit that cannot start.
  A table like this is only ever as complete as the last host that failed.
  The data advisory is weaker still: one template.
- **Excluded by default can be wrong for a host that wants the timer.** A
  `dnf-automatic-notifyonly.timer` is harmless on the target and is still
  left out until someone includes it.
- **A carried `graphical.target` default with no display manager** in the
  image produces a host that waits on a target nothing satisfies. The
  decision accepts that: the source said graphical, and the package list is
  where a missing display manager shows.
- **Removing the timer toggle removes a control operators can see today**,
  even though it does nothing. An operator who wants a local timer out now
  excludes its file in the config section. If the frontend pass cannot make
  that route obvious, the removal is a usability regression.
- **Much of the systemd behavior this relies on was not measured.** Offline
  `enable` for sockets and for mount units, `mask` on an absent unit, `--`
  before operands, the fragment a shadow's enable resolves, what
  `list-unit-files` lists for an instance, and whether `add-wants` runs at
  build time. This subsystem has a record of sound reasoning from
  unverified premises, which is why § 15 puts the runs that need no build
  ahead of the phases that depend on them.

Evidence that would change the design:

- The survey (run 20) shows timers, targets and slices never differ from
  preset on RHEL 9 and 10 outside local units. Then sockets and paths first
  is the right first step, with timers fixed separately as a scheduled-tasks
  defect. If it also finds no instance links and no hand-made links, phases
  3 and 4 shrink to the default target and the mask fix.
- Run 18 shows `cockpit.socket` already enabled in the built image by some
  other route (a preset in the base image). Then the driving loss is
  smaller than stated, and the omission half is the only real defect.
- Run 19 shows a local service with no timer builds today. Then the order
  move is not needed in phase 1, and the source reading in § 4 is wrong
  about something that should be understood before anything else is built
  on it.
- Run 22a shows `list-unit-files` lists enabled instances by name. Then the
  directory read is still needed for hand-made links, and instances could
  come from source 1 with the directories as a cross-check.
- Run 22c shows `add-wants` does not run at build time. Then the advisory
  offers the literal link with the unit's resolved path, and loses the
  build failure on a missing unit.
- Run 22d shows `enable` or `mask` fails for a mount unit at build time.
  Then a package-owned mount unit needs its link written another way, or
  falls back to an advisory with the reason.
- Run 22b shows the table paths are wrong, or that most templates name
  their configuration in the unit file. Then the table shrinks or goes.
- The frontend design pass finds that one section for all kinds is harder
  to use than units in place. Then the data contract in § 11.1 stands and
  § 11.2 does not.
