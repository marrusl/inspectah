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
  | SysV init scripts in `/etc/init.d` | Partly. `modernization.rs:56-63` raises an advisory for a script with no same-named vendor service. The config walk can record an unowned script's content, and can meet the `rc*.d` links, as it does any other path under `/etc`. | Two things. The script's enablement: there is no model of the `rc*.d` runlevel links, so a script enabled for a runlevel and one that is merely present look the same, and a link the walk records is carried as a flattened file, not as a link. And the script's mode: a carried init script loses its exec bit for the reason in the next row, and the SysV generator is understood to skip a script that is not executable (not measured). |
  | `/etc/rc.d/rc.local` exec bit | No. Nothing in `crates/collect/src` names `rc.local`. `ConfigFileEntry` has no mode field (`crates/core/src/types/config.rs:62-84`) and the config tree stager sets no permissions. | A host that runs `rc.local` at boot produces an image that does not, with no finding. |

  The last three are gaps this spec records and does not close.
- **Transient units and runtime states.** `transient`, `enabled-runtime`,
  `masked-runtime`, `linked-runtime` and `linked` keep today's treatment: a
  structured warning and no state change (`parse_unit_state`,
  `services.rs:49-80`).
- **Comparing the host with the target image.** The collector compares
  against the host's preset files, as it does today, and not against the
  target's. It does not compare dependency graphs either: a unit a package
  pulls in on the target through a `.wants` link or a `Wants=` is not
  detected (§ 10.1).
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
`services.rs:303`, instances included. The skip runs before the mask
branch at `:336`, so a masked template, which masks every instance of it,
is dropped too.

**`enabled` is taken on systemd's word.** A state of `enabled` goes
straight to the preset comparison (`services.rs:308-324`). Nothing asks
which links made the unit enabled, so a unit that has only a hand-made
link in some `.wants` directory is carried as `systemctl enable`, which
creates the links its `[Install]` section names and not the one the host
had. Nothing reads `Also=` either, so the units a carried `enable` or
`disable` acts on beyond its operand are unknown to the renderer.

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
`FullShadow` drop-in with empty content. A mask written as a zero-length
regular file takes the same route. systemd treats that file as a mask, and
the export's own 0-byte copy of a `/dev/null` link relies on it
(`process-docs/skills/real-host-validation-runs.md`, "A zero-length unit
file is masked"): today the `COPY` lands after `systemctl mask` and
replaces the link with the empty file.

**The default target is not recorded.** `/etc/systemd/system/default.target`
is in the walk's exact exclusion list (`walk.rs:61`) and no inspector reads
it.

**`.upholds` directories are not excluded from the config walk.** The
exclusion list names `*.wants/*` and `*.requires/*` (`walk.rs:154-155`)
and nothing for `*.upholds/*`, and `is_systemd_unit_symlink` tests only
names directly under `/etc/systemd/system`.

**Refine adds state changes the collector did not record.**
`synthesize_orphan_shadows` (`crates/refine/src/classify.rs:547`) runs in
every new session (`crates/refine/src/session.rs:517-521`). For a full
shadow whose unit has no state change it writes one, with
`default_state: None` and the `shadow_type` and `shadow_rationale` pair
set. It is the one production writer of those two fields on a state
change.

**Refine locks four units out on load.** `load_for_refine` calls
`normalize_incompatible_services` (`crates/refine/src/normalize.rs:51`,
`:242`), which excludes and locks every unit in `INCOMPATIBLE_SERVICES`
(`crates/core/src/baseline.rs:128-143`): `dnf-makecache.service`,
`dnf-makecache.timer`, `packagekit.service` and
`packagekit-offline-update.service`, with their drop-ins.

**The fleet merge includes every row it produces.** `merge_items` calls
`set_include(true)` on each representative
(`crates/core/src/aggregate/merge.rs:557`, `:577`, `:658`) and then
excludes only rows that are not on every host. A row every host excluded
comes out included. State changes and drop-ins are merged as two
independent lists (`merge.rs:1533-1542`), and refine's unit toggle
cascades to every drop-in of that name, full shadows included
(`session.rs:2807-2821`).

## 5. Scope per unit type

| Unit type | Treatment | Reason |
|---|---|---|
| `.service` | Carry state | Today's behavior, unchanged in meaning. |
| `.socket` | Carry state | Packages ship sockets disabled that administrators enable (`cockpit.socket`). The driving item. |
| `.path` | Carry state | Same model as a socket: an activation unit with `[Install]`. |
| `.timer` | Carry state. Schedule and command stay with scheduled tasks (§ 8). | Enablement is unit state; what runs and when is the scheduled-task fact. |
| `.target` | Carry state for targets that have `[Install]`. The default target is separate (§ 7.6) and is carried whenever the host set one. | Rare, and identical in mechanism. `default.target` is a link written by `systemctl set-default`, not an enable. |
| `.slice` | Carry state. In practice this means a mask; slices are otherwise `static`. Drop-ins are the common customization and follow § 7.7. | Uniform treatment costs nothing; a special case would. |
| `.mount`, `.automount`, `.swap`, package-owned under the vendor directory | Carry state, like a service. | Decided (§ 2). The unit is the distribution's, the same in the image as on the host, and switching it is configuration (`tmp.mount`). |
| `.mount`, `.automount`, `.swap`, written by the administrator | **Advisory only.** Collected, reported, never a `systemctl` line. The unit file stays a config entry. | Decided (§ 2). It is a statement about one host's storage, and on an image-based target several mount points are owned by bootc, are links into `/var`, or sit in the read-only image (§ 10.6). § 6.2 says which units count as the administrator's. |
| `.device`, `.scope` | Excluded. | They never exist as unit files. Administrator drop-ins for them (`*.device.d`, `*.scope.d`) are still collected as drop-ins (§ 7.7). |
| Template (`name@.service`) | A mask is carried. Nothing else is: a template has no enablement without an instance name. | A masked template is durable state with its own name, and it masks every instance (§ 7.4). |
| Enabled template instance (`wg-quick@wg0.service`) | Carry state, when the configuration the instance reads is in the export (§ 7.5, § 10.8). Instances tied to one host's devices are advisory. | Decided (§ 2). |
| Hand-made link in `.wants`, `.requires` or `.upholds`, and a hand-made alias link | **Advisory only**, with the `systemctl add-wants` or `add-requires` line where one exists (§ 7.3, § 10.9). | Decided (§ 2). |
| Generated units | Excluded (§ 3). Administrator drop-ins on them are kept. | Decided. The generator's input is the fact, not its output. |
| Transient units, runtime and `linked` states | Excluded, with today's warning. | Decided, and they do not survive a reboot or need manual handling. |
| User units | Out of scope (§ 3). | Not part of this change. |
| Quadlets | Excluded. | Decided. |

The assumption that targets, slices and mounts rarely differ from preset is
untested, as is how often hosts enable template instances. § 13 specifies
the survey that tests it.

## 6. The unified unit model

Types live in a new `crates/core/src/types/units.rs`, replacing
`types/services.rs`.

The types in this section prove facts about **one observation**: one unit on
one host, as the collector or the snapshot loader saw it. They cannot prove
anything about the export a fleet merge and a refine session assemble from
many observations. That second proof is one render-time check, § 10.2, and
every `systemctl` line goes through both.

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
    /// `prefix@.suffix`, the template an instance falls back to.
    pub fn template(&self) -> UnitName { /* ... */ }
}
```

The kind is derived from the name and never stored beside it, so a snapshot
cannot hold a `cockpit.socket` labelled as a service. The same holds for
"is an instance" and "is a template": both are read from the name. The
character set is the one `systemd.unit(5)` gives for unit names. A snapshot
holding a name outside it fails to load, which is the refusal an imported
snapshot should get.

What the type proves is what quoting needs: no quote, no whitespace, no
control character, a known suffix. It is not the whole systemd name grammar.
A name that passes here and that systemd still rejects fails at
`RUN systemctl`, at build time and loudly.

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
/// A `UnitName` proved to be a `.target`.
pub struct TargetUnitName(UnitName);

/// Where the unit's effective file lives on the source host (§ 7.2). For
/// an instance that is its own file when one exists, and its template's
/// otherwise; `path` says which.
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

/// A unit inspectah may consider for a `systemctl` line. The only subject
/// a state change holds, and the only input the action plan accepts
/// (§ 10.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "UnitSubject", into = "UnitSubject")]
pub enum CarriedUnit {
    /// Any fragment: a local service is carried with its file (§ 10.2).
    Ordinary { name: OrdinaryUnitName, fragment: UnitFragment },
    /// A storage unit under the vendor directory with a known owner.
    /// There is no field here that could hold an `/etc` path.
    PackagedStorage { name: StorageUnitName, owning_package: String },
}

/// A `CarriedUnit` proved to be a `.target`: the only operand
/// `set-default` takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "UnitSubject", into = "UnitSubject")]
pub struct TargetUnit { name: TargetUnitName, fragment: UnitFragment }

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

What the types prevent, for one observation:

- **A state change on an administrator's mount unit.** A `UnitStateChange`
  holds a `CarriedUnit`. `CarriedUnit::PackagedStorage` has no variant or
  field in which a local path can sit, and there is no function from an
  `AdvisoryUnit` to a `UnitAction`.
- **A hand-edited snapshot that moves a local mount into `state_changes`.**
  `{"name": "srv.mount", "fragment": "local", ...}` fails `try_from` and the
  snapshot does not load. `{"name": "tmp.mount", "fragment": "vendor",
  "owning_package": "systemd"}` loads.
- **A package-owned mount unit reported as an advisory.** The reverse
  `try_from` refuses it, so a vendor `tmp.mount` cannot lose its toggle by
  landing in `mount_findings`.
- **A `set-default` on something that is not a target.** `DefaultTarget`
  holds a `TargetUnit` (§ 7.6). A snapshot that names `cockpit.socket` or
  `tmp.mount` there fails to load.
- **`.device` and `.scope` units.** Neither kind has a variant, so no value
  of either can exist.

What the types do not prevent: an export in which one host's package-owned
`tmp.mount` state change sits beside another host's administrator-written
`/etc/systemd/system/tmp.mount`. Each observation is valid. The enable
would run against the administrator's file. § 10.2 closes that, and
`systemctl_lines` accepts only what § 10.2 produces.

Three cases fall on the advisory side by this design. They are the spec
author's reading of the decision, not part of it (§ 16 question 1):

- **`Shadowed`: a vendor mount unit with a full copy in `/etc`.** The `/etc`
  file replaces the vendor one, so `What=`, `Where=` and `Options=` are what
  the administrator wrote, and an enable would activate that content, which
  is what the advisory rule exists to stop. The package owns the name; it no
  longer owns what the unit does. The advisory carries the line to add by
  hand, so the cost of this side is one line and the cost of the other is an
  unreviewed mount in every host built from the image. Its file is a config
  entry like any other local storage unit's (§ 7.7).
- **A vendor mount unit with a drop-in** stays `Vendor` and is carried. The
  fragment is where the unit file lives; a drop-in adds to a package's unit
  and is collected and toggled on its own (§ 7.7).
- **`UnownedVendor`.** The decision names a unit "owned by a package". A file
  under the vendor directory that no package claims, or whose owner could not
  be read, does not meet it. For storage kinds the collector therefore runs
  the owner lookup before it classifies.

### 6.3 State changes

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnitState { Enabled, Disabled, Masked }

/// What else `systemctl enable` or `disable` on a unit acts on (§ 7.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlsoEffect {
    /// The closure of `Also=`, the unit itself left out. Empty for most
    /// units.
    Known(Vec<UnitName>),
    /// An `[Install]` section in the closure could not be read with
    /// confidence.
    Undetermined,
}

/// A file a template instance reads and cannot start without (§ 7.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredConfig {
    pub path: String,
    pub on_host: ConfigOnHost,
}

/// What the collector found at a required path on the source host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "on_host", rename_all = "snake_case")]
pub enum ConfigOnHost {
    /// Nothing exists there.
    Missing,
    /// It exists and no package owns it.
    Unowned,
    /// It exists and `package` owns it.
    Owned { package: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "UnitStateChangeWire")]
pub struct UnitStateChange {
    pub unit: CarriedUnit,
    pub current_state: UnitState,
    #[serde(deserialize_with = "require_explicit_null")]
    pub default_state: Option<PresetDefault>,
    #[serde(default)]
    pub also: AlsoEffect,
    /// Filled for template instances (§ 7.5).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_config: Vec<RequiredConfig>,
    /// Paths under `/var` that hold this unit's persistent data. Filled
    /// for template instances (§ 7.5).
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
The one production writer of the shadow pair, refine's orphan-shadow
synthesis (§ 4), is retired with the fields (§ 11.3).

**A template name holds a mask and nothing else.** The checked constructor
behind `try_from` refuses a `UnitStateChange` whose name is a template and
whose state is not `Masked`. Enabling or disabling a bare template means
nothing without an instance, so no value of that kind can be built, by the
collector or by a snapshot.

`also`, `required_config` and `data_paths` are plain fields and not an
`Option<InstanceFacts>`, because "present exactly when the name is an
instance" would be an invariant the compiler cannot hold. An empty list is
true of every unit this design does not fill it for.

`RequiredConfig` records what the collector found and decides nothing. The
decision is made at render, against the export (§ 10.8). A path that is
missing on the source, or that a package owns, is still in the list: leaving
it out at collection would leave the gate nothing to reject.

### 6.4 The section

```rust
/// One unit's enablement on the source host.
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
    /// Drop-ins, and full shadows of ordinary kinds (§ 7.7).
    pub drop_ins: Vec<SystemdDropIn>,
    /// The administrator's mount, automount and swap units (§ 6.5).
    pub mount_findings: Vec<MountUnitFinding>,
    /// Links made by hand, and instances tied to one host (§ 6.6).
    pub manual_links: Vec<ManualLink>,
    /// The default target, when the host sets one (§ 7.6).
    pub default_target: Option<DefaultTarget>,
}
```

`inventory` replaces the three string lists `enabled_units`, `disabled_units`
and `preset_matched_units`. One list with a state per entry cannot place a
unit in two of them, and every consumer that counts or filters does so by
`unit.kind()`. `readme.rs` counts entries whose kind is `Service` or, better,
reports units. `enabled_units_by_owner` filters `state == Enabled`.

The `state` in an inventory entry is the one § 7.3 derives, so a unit that
systemd calls `enabled` on the strength of a hand-made link is `Disabled`
here. Enabled template instances found by the link scan are entries too.
§ 10.2 reads this list to learn what state a unit named by `Also=` had.

### 6.5 Mount findings

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MountUnitFinding {
    pub unit: AdvisoryUnit,
    /// Present when the unit is enabled or masked.
    pub state_change: Option<(UnitState, Option<PresetDefault>)>,
    /// `Where=` for a mount or automount, when it is an absolute path.
    pub mount_point: Option<String>,
    /// Whether `Options=` holds `nofail`.
    pub nofail: bool,
    pub aggregate: Option<AggregatePrevalence>,
}
```

The finding has no `disposition`. There is nothing to include or exclude, so
there is no control to offer. It records what the host had. Whether a mount
point collides with the target layout is a property of the target, so the
hazards in § 10.6 are computed at render from `mount_point`, not stored.

The finding does not hold `What=`. A mount source can carry a credential
(`What=https://files.example/volume?token=...`), and a field copied out of
the unit file would be a second copy that the config pass, which scrubs the
file itself, never visits. Refine shows the source by linking the row to the
unit file's config entry, which holds the scrubbed text. § 6.7 gives the
rule this follows.

A finding is recorded for every `.mount`, `.automount` or `.swap` unit that
classifies as `Advisory`: every regular, non-empty file of those kinds under
`/etc/systemd/system`, and an unowned one under the vendor directory when it
is enabled or masked. A package-owned vendor unit of these kinds never
produces a finding. When its state differs from preset, or it is masked, it
is a `UnitStateChange` like a service.

### 6.6 Hand-made links

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkRelation { Wants, Requires, Upholds, Alias }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualLinkCause {
    /// The linked unit has no `[Install]` section.
    StaticUnit,
    /// The unit's `[Install]` section does not name this parent for this
    /// relation.
    NotInInstall,
    /// The section names this link and others that are absent, so neither
    /// `enable` nor `disable` reproduces the host.
    PartialInstall,
    /// The `[Install]` section could not be read with confidence (§ 7.2).
    Undetermined,
    /// An instance of a template tied to one host's devices (§ 7.5).
    HostDevice,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManualLink {
    pub relation: LinkRelation,
    /// The unit whose `.wants`, `.requires` or `.upholds` directory holds
    /// the link. For an alias, the alias name.
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

### 6.7 Fields copied out of host files, and who scrubs them

Redaction scrubs content where it is stored. A value the collector copies
out of a file into a field of its own is a second copy, and it is scrubbed
only if something is told to. Every field this spec adds or keeps that is
read from a host file has one of three answers:

| Field | Read from | Answer |
|---|---|---|
| The mount source | `What=` | **Not stored.** `MountUnitFinding` has no field for it (§ 6.5). |
| `MountUnitFinding.mount_point` | `Where=` | Stored only when it is an absolute path; otherwise `None`. Classified as an identifier. |
| `MountUnitFinding.nofail` | `Options=` | A boolean. Nothing to scrub. |
| `RequiredConfig.path`, `data_paths` | Paths in the template's unit file, expanded | Stored only when the expanded value is an absolute path under `/etc` or `/var`. Classified as identifiers. |
| `AlsoEffect`, `ManualLink.parent`, `ManualLink.unit`, `DefaultTarget.target` | `[Install]` values and link names | Proved `UnitName`s. |
| `UnitFragment::path` | A directory listing | A path under `/etc/systemd/system/` ending in a proved name. |
| `SystemdTimer.exec_start`, `description` | The timer and service files | Scrubbed by the existing scheduled pass, unchanged. |
| `SystemdDropIn.content` | The drop-in or shadow file | Scrubbed by the existing drop-in pass, renamed with the section. |
| `attention_reason`, advisory strings | Generated | Built from the fields above and fixed text only. Never from file content. |

The unvisited-field guard in `crates/pipeline/src/render/mod.rs`
(`every_field_of_a_content_bearing_type_is_classified`) gains
`MountUnitFinding`, `ManualLink`, `UnitStateChange`, `RequiredConfig`,
`UnitInventoryEntry` and `DefaultTarget`, each destructured with no `..`, so
a field added later stops the build until it is classified. Its runtime
half gains a sentinel case: a local mount unit whose `What=` holds a token
the generic pattern matches, asserting the token is absent from the whole
serialized snapshot and from the refine view, not only from the config
entry.

## 7. The collector

`UnitsInspector` replaces `ServicesInspector`. It is the one owner of three
facts for every kind: whether a unit is enabled, disabled or masked against
what the image would have; where its file lives; and which package owns it.

### 7.1 Sources

1. `systemctl list-unit-files --no-pager`, with no `--type`. The state column
   goes through `parse_unit_state` unchanged. It supplies the list of names
   and the `disabled`, `masked`, `generated` and runtime states. Its word
   `enabled` is a claim that § 7.3 checks.
2. The host's preset files, as today.
3. A direct read of `/etc/systemd/system`: regular unit files, zero-length
   unit files and symlinks to `/dev/null` (masks), other symlinks directly
   under it (aliases, and `default.target`), and the `*.wants/`,
   `*.requires/` and `*.upholds/` directories.
4. The effective unit for each name the first three sources produce (§ 7.2).

Sources 3 and 4 exist because source 1 cannot answer three questions: which
links made a unit `enabled`, whether a `static` unit was linked into a
`.wants` directory by hand, and which template instances are enabled.
`list-unit-files` is believed to list the template only, with state
`indirect` when an instance other than its `DefaultInstance=` is enabled.
That was not checked on a host; § 13 run R5 settles it. The design does not
depend on the answer: instance names come from the directories, and a name
both sources report is recorded once.

Only `/etc/systemd/system` and `/usr/lib/systemd/system` are read. A unit
whose file sits in another directory of systemd's search path
(`/usr/local/lib/systemd/system`, `/run/systemd/system`) resolves to no file
here. The collector records a structured warning that names it and records
no state for it.

### 7.2 The effective unit

Every question below about a unit (where its file is, which package owns
it, what `enable` would do, what an instance reads) is asked of one thing:
the file systemd would load for that name, with the drop-ins that apply to
it. The collector resolves that once per name, with one function, and every
later step takes its result.

```rust
/// The file systemd loads for a name on the source host, and what
/// `systemctl enable` would act on.
pub struct EffectiveUnit {
    pub form: FragmentForm,
    pub install: InstallRead,
}

pub enum FragmentForm {
    /// A regular, non-empty unit file.
    File(UnitFragment),
    /// A link to `/dev/null`, or a zero-length regular file, at the
    /// `/etc/systemd/system` path. Either one masks the unit.
    Mask,
    /// A link under `/etc/systemd/system` to another unit's file.
    AliasOf(UnitName),
    /// No file in the two directories this collector reads.
    NotFound,
}

pub struct InstallSection {
    pub wanted_by: Vec<UnitName>,
    pub required_by: Vec<UnitName>,
    pub upheld_by: Vec<UnitName>,
    pub alias: Vec<UnitName>,
    pub also: Vec<UnitName>,
}

pub enum InstallRead {
    /// The unit has no `[Install]` section.
    Absent,
    Known(InstallSection),
    /// The section cannot be relied on. The cause is kept for the warning.
    Undetermined(InstallUnknown),
}

pub enum InstallUnknown {
    /// A value holds a specifier other than `%i`, `%I`, `%p` or `%N`.
    Specifier,
    /// A drop-in adds to, resets or replaces a key of the section.
    DropInChangesInstall,
    /// A value is not a unit name this model admits.
    ForeignName,
}
```

Resolution, for a name `N`:

1. **The fragment.** `/etc/systemd/system/N`, then
   `/usr/lib/systemd/system/N`. When `N` is an instance and neither exists,
   the template `prefix@.suffix`, in the same order. A concrete
   `worker@site.service` therefore wins over `worker@.service` wherever the
   template lives, and it is that file whose location, owner and content
   are used.
2. **Its form.** A symlink is tested before anything else: to `/dev/null`
   it is a mask, to another unit's file it is an alias, anywhere else the
   unit is `linked`, which § 3 excludes. A regular file of zero length is a
   mask (`process-docs/skills/real-host-validation-runs.md`, "A zero-length
   unit file is masked"). Anything else is a file, and its `UnitFragment`
   says where it and any vendor file of the same name live.
3. **Its `[Install]` section.** Read from the fragment, with `%i`, `%I`,
   `%p` and `%N` expanded. Then read again with the drop-ins that apply:
   `N.d/*.conf`, the template's `.d` for an instance, and the type-wide and
   dash-prefix directories `systemd.unit(5)` lists, under both directories,
   in systemd's order and with its rule that an empty assignment resets a
   list. If the two reads differ, the result is `Undetermined`.

Step 3 reads the section twice because whether `systemctl enable` honors an
`[Install]` key that comes from a drop-in was not checked on RHEL 9 or
RHEL 10. Under either answer the design is safe: where a drop-in would
change the answer, nothing is carried on the strength of it. § 13 run R5
records the behavior. If drop-ins are honored on both releases, the second
read becomes the answer and `DropInChangesInstall` goes.

The order in step 1 (a concrete instance file before the template, in
either directory) is systemd's documented lookup and was not measured here.
Run R5 measures it.

The reader is a pure function over text and lives in the core crate,
because render reads the `[Install]` section of a staged unit file with the
same function (§ 10.2).

### 7.3 Enablement is read from the links

`list-unit-files` reports `enabled` when it finds a link to the unit. It
does not say who made the link. A unit with `WantedBy=multi-user.target`
and one hand-made `rescue.target.wants/` link is `enabled` to systemd, and
`systemctl enable` in the image would create the `multi-user.target` link
the host never had. So `enabled` is not admitted on that word.

The collector lists each `*.wants/`, `*.requires/` and `*.upholds/`
directory under `/etc/systemd/system` and each symlink directly under it.
Only `/etc` is read. Links under `/usr/lib/systemd/system` belong to
packages and are in the image already. Links a generator makes live under
`/run`, which is why `serial-getty@ttyS0.service` on a host with a serial
console is not seen: it is generator output, and § 3 excludes it.
`.upholds/` is included because `UpheldBy=` produces links the same way
`WantedBy=` does. Which systemd release first honors it, and whether the
target base does, was not checked; run R5 records both.

A link whose parent or own name does not parse as a `UnitName` (a `.device`
parent, for one) produces a structured warning and nothing else.

For each unit `U` that source 1 reports `enabled` or that a link names, the
collector compares two sets:

- **expected:** the links `systemctl enable U` would make, from the
  effective `[Install]` section: one per `WantedBy=`, `RequiredBy=` and
  `UpheldBy=` value, and one alias link per `Alias=` value.
- **found:** the links to `U` that are on the host.

| `[Install]` | found against expected | Install state of `U` | Each found link |
|---|---|---|---|
| `Known`, expected not empty | Every expected link is found | `Enabled` | One outside expected is a `ManualLink`, cause `NotInInstall`. The rest are enable-made and produce nothing of their own. |
| `Known` | No expected link is found | `Disabled` | A `ManualLink`, cause `NotInInstall`. |
| `Known` | Some expected links are found and some are not | None. No state change is recorded. | A `ManualLink`, cause `PartialInstall`. |
| `Absent` | (nothing is expected) | None. The unit is `static`. | A `ManualLink`, cause `StaticUnit`. |
| `Undetermined` | (cannot be computed) | None. No state change is recorded. | A `ManualLink`, cause `Undetermined`. |

A unit that source 1 reports `disabled` and that no link names has the
install state `Disabled`. The install state, not systemd's word, is what
§ 7.4 compares with the preset and what `inventory` records. The mixed case is the first row: a
unit that is fully enabled and also has a hand-made link is carried as
enabled, and the extra link is advised.

Every row that does not end in `Enabled` or `Disabled` falls on the
advisory side on purpose. An advisory for a link that `enable` would have
made costs the operator one line. A carried enable for links it would not
have made changes what the image starts.

A hand-made alias (a link directly under `/etc/systemd/system` to a unit
whose `Alias=` does not name it) is a `ManualLink` with relation `Alias`
and the alias name as `parent`. `default.target` is not an alias; § 7.6
owns it.

What `list-unit-files` and `is-enabled` report for a unit that has only a
hand-made link, and for one that has only an alias link, was not measured.
The table does not depend on it, since the state comes from the links. Run
R5 records it so the warning text can say what the operator sees on the
host.

**`Also=`.** `systemctl enable` and `disable` act on the unit they are
given and on every unit its `[Install]` section names in `Also=`, and on
the units those name in turn. The collector follows `Also=` through the
effective unit of each name until no new name appears, and records the
result on the state change as `AlsoEffect::Known`, the unit itself left
out. A named unit with no `[Install]` section of its own is left out too:
neither verb changes it. If any `[Install]` section in that walk is `Undetermined`, or a named
unit resolves to `NotFound`, the result is `AlsoEffect::Undetermined`.
§ 10.2 decides what a non-empty closure means for the command. That
`enable` and `disable` follow `Also=` with no manager running, and that
`mask` does not, are documented behavior and were not measured; run R5
measures all three.

### 7.4 What makes a state change

For each name with an install state (§ 7.3) or a mask, whose name and
fragment classify as a `CarriedUnit` (§ 6.2):

- **Masked** is always a state change, as today. A mask is recognized by
  form (§ 7.2), so a zero-length unit file and a `/dev/null` link produce
  the same `Masked` state change and nothing else (§ 7.7).
- **A masked template** is a `Masked` state change on the template name.
  The mask test runs before the template test. It takes none of the
  instance facts in § 7.5 and no configuration gate applies to it.
- **`Vendor` or `Shadowed` fragment, and every `PackagedStorage` unit:** a
  state change when the install state differs from the preset, as today.
  With no matching preset rule there is no evidence of a change and none is
  recorded, as today.
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
(§ 6.5). A template name that is not masked is not a state carrier and is
skipped.

Units inspectah must not switch off are marked at collect and refused at
render (§ 10.3). Units that update packages at runtime are collected and
excluded by default (§ 10.4).

### 7.5 Template instances

An instance whose install state is `Enabled` is a `UnitStateChange` of
`Enabled`, with the fragment and owner of its effective unit, under the
same rule as § 7.4 applied to the instance name. An instance linked from
two parents is one state change. Where the effective unit is the instance's
own file, the file check in § 10.2 asks about that file and not about the
template.

A masked instance needs nothing new: its mask under `/etc/systemd/system`
is found by form and its name parses. An instance that is disabled leaves
no trace on the host and is not recorded.

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

| List | Derived from the effective unit file | Fixed table (named constant) |
|---|---|---|
| `required_config` | Every path under `/etc` in the file that holds `%i` or `%I`, expanded for the instance. An `EnvironmentFile=` value with the `-` prefix is optional to systemd and is left out. | `wg-quick@` reads `/etc/wireguard/%i.conf`; `openvpn-client@` reads `/etc/openvpn/client/%i.conf`; `openvpn-server@` reads `/etc/openvpn/server/%i.conf`. |
| `data_paths` | Every path under `/var` in the file that holds `%i` or `%I`, and `/var/lib/<value>` for a `StateDirectory=` value that holds one. | `dirsrv@` keeps `/var/lib/dirsrv/slapd-%i`. |

**Every `required_config` path is kept**, with what the collector found
there (`ConfigOnHost`, § 6.3): missing, present and unowned, or present and
owned by a package. The collector does not drop a path because it is
missing or because a package owns it. A missing file is the case the gate
most needs to see, and a package's file may be one the administrator
changed, which the config section then carries, redacts or excludes like
any other. § 10.8 decides.

**`data_paths` holds persistent data only.** A path under `/var/run` or
`/var/lock` is runtime state: both resolve into `/run`, which is empty at
every boot. A `PIDFile=/var/run/worker/%i.pid` is not data to migrate. The
two prefixes are one named constant, and a derived path under either is
left out. `RuntimeDirectory=` is not read. A remaining path is kept only
when it exists on the source host.

The table exists because the derivation cannot see the motivating case.
`wg-quick@.service` runs `wg-quick up %i` and never names `/etc/wireguard`.
The table paths were written from knowledge of those packages and were not
read from an installed unit; run R5 checks them. An instance with neither
list filled is carried on its unit file alone, and its row carries an
`attention_reason` saying inspectah does not know what the instance reads.
§ 10.8 says what render does with both lists and what this does not cover.

### 7.6 The default target

The collector reads the link at `/etc/systemd/system/default.target`. If it
exists and its target's name parses as a `.target`, it records:

```rust
pub struct DefaultTarget {
    pub target: TargetUnit,
    pub disposition: FindingKind,
    pub locked: bool,
    pub aggregate: Option<AggregatePrevalence>,
}
```

`TargetUnit` (§ 6.2) is a carried unit proved to be a target, with its
fragment kept for the file check. A link to anything else produces a
structured warning and no record.

This is its own check and its own render line. It is not an enable, and it
does not go through the preset comparison. It is recorded as included
whatever the target is (§ 10.5).

### 7.7 Drop-ins, shadows and masks

One rule decides who owns each thing found at
`/etc/systemd/system/<name>`, and the form test of § 7.2 runs first:

| What is at the path | Recorded as | Its file is written by |
|---|---|---|
| A link to `/dev/null`, or a zero-length regular file | A `Masked` state change. Nothing else. | Nobody. `systemctl mask` makes the link in the image. |
| A link to another unit's file | Nothing, or a `ManualLink` when `Alias=` does not name it (§ 7.3). | Nobody. |
| A regular file of a storage kind (`.mount`, `.automount`, `.swap`) | A mount finding (§ 6.5). The file is a config entry. | The config tree, on the config row's decision. |
| A regular file of an ordinary kind with a vendor file of the same name | A full shadow in `drop_ins`. | The drop-in writer, on that entry's decision. The config entry at the path is `Inventory` with `shadowed_by` set. |
| A regular file of an ordinary kind with no vendor file | A `Local` fragment. The file is a config entry. | The config tree, on the config row's decision. |

- **A mask has one representation.** However the host wrote it, a mask is
  a `Masked` state change and the export carries no file for it. The
  config walk's exclusion for symlinks directly under `/etc/systemd/system`
  (`is_systemd_unit_symlink`) extends to zero-length regular files with a
  unit suffix there, so no config entry exists to stage. § 10.2 holds the
  same line for an export assembled some other way.
- **A storage unit's file has one owner, the config section.** A regular
  `/etc/systemd/system/tmp.mount` beside a vendor `tmp.mount` is not a full
  shadow in `drop_ins`. It is a config entry under its own toggle, as
  § 10.6 says, and the `Shadowed` fragment on its mount finding records
  that a vendor file exists. This holds whichever way § 16 question 1 is
  answered: the question is whether the unit gets an enable line, never who
  writes the file.
- **Drop-ins** are collected from `/etc/systemd/system/<unit>.d/*.conf` for
  every unit name that parses, plus `*.device.d` and `*.scope.d`, and for
  units that are generated (the parent is recorded as generated, detected by
  state or by a file under `/run/systemd/generator*`). The existing rule
  holds: the units section owns the file, and the config entry for the same
  path is `Inventory` with `shadowed_by` set
  (`shadow_service_owned_paths`, `crates/pipeline/src/collect.rs:274`). A
  drop-in on a generated parent is carried and does not bring the parent
  back: the generated unit exists in the image only if its generator's
  input does.

`SystemdDropIn` keeps its fields. `unit` stays a `String`, because a drop-in
may name a `.device` or `.scope` that `UnitName` does not admit.

**The config walk excludes all three dependency directories.**
`/etc/systemd/system/*.upholds/*` joins `*.wants/*` and `*.requires/*` in
the walk's exclusion list (`walk.rs:154-155`). They hold links, not
content, and the collector turns them into state or into an advisory.
Without the third entry the walk follows an `.upholds` link to a local unit
and records the unit's content at the link's path, which would give the
link a config toggle and stage a flattened file where the link was. This is
the answer to the question the overlap item asked about that exclusion.

### 7.8 Owning package

`populate_owning_packages` runs over every `Vendor` and `Shadowed` state
change and mount finding, and asks about the effective unit's file: the
instance's own file when it has one, its template's otherwise. For storage
kinds the lookup runs before classification, since the answer decides which
side of § 6.2 the unit falls on. A failed query for a vendor mount unit
therefore yields an advisory, not a carried line.

For each `required_config` path that exists, the same lookup fills
`ConfigOnHost`. A failed query records `Unowned`, which sends the path to
the stricter side of the gate.

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
  (`containerfile.rs:1382-1386`) is removed. Generated units keep theirs,
  and it goes through `plan_units` and the shared helper (§ 10.2).
- `config_tree_units` (`service_intent.rs:308`) keeps only its
  generated-unit half.
- `ItemId::SystemdTimer` is removed from refine, with the bulk operation that
  emits it, the validation arm that accepts it, and the aggregate handler
  that lists each timer as an item with an include flag
  (`crates/web/src/aggregate_handlers.rs:1158-1166`). The silent no-op is
  closed by removing the control, not by wiring it up.

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

### 9.1 Consumers the compiler does not find

A renamed Rust field stops the build at every use. A string does not. The
section's name is also a string key, a section id, a TypeScript field and a
discriminator value, and each of those keeps compiling, and keeps running,
after the rename. A frontend that still reads `service_states ?? []` shows
an empty section and reports no error.

**The new spelling is `units` everywhere the old one was `services`.** The
checklist below was read at `70aa8996`. Every row is migrated in phase 1
(§ 15), with the Rust rename, and none is left for the frontend design
phase.

| Consumer | Where | Today | Becomes |
|---|---|---|---|
| Snapshot key | `crates/core/src/snapshot.rs:33` | `services` | `units` |
| Per-section host counts | `compute_section_host_counts`, `crates/core/src/aggregate/mod.rs:269` | `"services"` | `"units"` |
| Section routing | `SectionGroup::for_section`, `crates/pipeline/src/section_group.rs:60` | `"services"` | `"units"` |
| Config ownership discriminator | `ConfigShadow::Services`, `crates/core/src/types/config.rs:58`, serialized `shadowed_by: "services"` | `"services"` | `ConfigShadow::Units`, `"units"` |
| Ownership link in the frontend | `resolveShadowOwner`, `crates/web/ui/src/components/shadowOwner.ts:45` | `entry.shadowed_by === "services"` | `=== "units"` |
| View response fields | `crates/web/src/web_types.rs`, `crates/refine/src/projection/types.rs`, and `ViewResponse` in `crates/web/ui/src/api/types.ts:704` | `service_states`, `service_dropins` | `unit_states`, `unit_dropins` (§ 11.1) |
| Frontend reads with an empty fallback | `crates/web/ui/src/components/MainContent.tsx:710-714`, `Sidebar.tsx`, `GlobalSearch.tsx`, `useKeyboard.ts`, `App.tsx` | `viewData?.service_states ?? []`, section id `"services"`, `s.shadow_type === "full_shadow"` | The new fields, section id `"units"`, `s.fragment === "shadowed"` |
| Refine operation id | `ItemId::Service`, `crates/refine/src/session.rs:2807` and its TypeScript mirror | `Service { unit }` | `Unit { unit }` |
| Refine normalization and synthesis | `normalize_incompatible_services` (`crates/refine/src/normalize.rs:242`), `synthesize_orphan_shadows` (`crates/refine/src/classify.rs:547`) | Read and write `ServiceSection` | § 10.4 and § 11.3 |
| Redaction pass and its guard | The drop-in loop in `redact` (`crates/pipeline/src/redaction/engine.rs:978`) and the field guard in `crates/pipeline/src/render/mod.rs` | `snapshot.services` | `snapshot.units`, with the additions in § 6.7 |
| Reports | `crates/pipeline/src/render/audit.rs`, `report.rs`, `readme.rs` | Section name and counts | Renamed; counts by `unit.kind()` |
| Aggregate timer control | `crates/web/src/aggregate_handlers.rs:1158`, `AggregateItemRow.tsx` | Emits `ItemId::SystemdTimer` with an include flag | Removed in phase 3 with the control (§ 8) |

Phase 1 ends with a search of the tree for the quoted strings `"services"`,
`service_states` and `service_dropins`, and each remaining hit is either
renamed or listed in the phase report with the reason it stays (the
user-facing word "services" is not this section's name).
The frontend test suites that mock the view response are migrated in the
same phase, since they are the only check that the two ends agree.

## 10. Render

### 10.1 Lines

One function, the successor of `render_service_intent`, produces every
`systemctl` line for collected units. It emits, in this order, under one
section header:

```
# === Systemd Unit State ===
# Omitted: cockpit.socket (package 'cockpit-ws' was excluded)
# Omitted: wg-quick@wg0.service (its configuration /etc/wireguard/wg0.conf holds a redacted value; provision the secret on the deployed host and enable the unit there)
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
  operand is a proved `UnitName`, whose character set holds no quote and no
  line break, so `single_quote` (`safety.rs:162`) is total on it. Quoting
  replaces the refusal: a name with `\x2d` is legal and now renders, and
  `-.slice` is safe after `--`. That `systemctl` accepts `--` before unit
  operands, and that a backslash inside single quotes survives the
  Containerfile parser, are not verified here; § 13 checks both.
- **`systemctl_lines` takes `&[PlannedUnit]` and nothing else.** A
  `PlannedUnit` is built only by the check in § 10.2, so no line can name a
  unit that check did not pass. The generated timer and `@reboot` service
  enables in the scheduled section go through the same check and the same
  helper.
- A template instance renders with its instance name, in the same `RUN` as
  every other unit of its verb. A masked template renders with its template
  name in the `mask` line.
- A package-owned mount, automount or swap unit renders in the same `RUN`
  too. That `systemctl enable` and `mask` work for a mount unit at build
  time, with no running manager, is not verified here; § 13 run R5 checks
  it with `tmp.mount`.
- The same function emits the comment-only blocks: mount advisories
  (§ 10.6), instance data advisories (§ 10.8) and hand-made link advisories
  (§ 10.9). None of them is an instruction.

**`disable` removes the administrator's links and nothing more.** A carried
`RUN systemctl disable` reproduces what the source host's administrator
did. It does not stop the unit from starting in the image if something
else pulls it in: a `.wants` link a package ships under
`/usr/lib/systemd/system`, or a `Wants=` or `Requires=` in another unit.
Those are in the image whatever this line does. inspectah does not compare
the source's dependency graph with the target's (§ 3), so it cannot report
the difference, and it does not turn a `disable` into a `mask`, which is a
stronger statement than the source host made. The exported README says this in
one sentence, and run R5 measures one such unit on each base.

### 10.2 One check of the assembled export

Every rule about a `systemctl` line is a rule about the image: which file
the name resolves to there, and what the command does to it. A snapshot
that reaches render may be one host's scan, a fleet merge of many, an
imported file, and in each case a refine session has switched rows on and
off. Each row was valid alone (§ 6). Only the assembled export can say
whether the rows are valid together. So the rules are checked once, on the
export, after the merge and after refine selection, by one function, and
the action plan is its output.

```rust
/// Every file the stager will write, with its destination, after the
/// stager's own ownership subtractions and path validation. The stager
/// writes from this list and from nothing else.
pub(crate) fn staged_writes(snap: &InspectionSnapshot) -> StagedWrites<'_>;

/// What the export puts at `/etc/systemd/system/<name>`.
pub enum StagedAt<'a> {
    Nothing,
    /// A unit file, from whichever writer: a config entry, a full shadow,
    /// or a generated unit.
    UnitFile { source: StagedFileKind, content: &'a str },
}

/// A unit and the one command the plan will write for it. The fields are
/// private to the planning module, which is the only place a value is
/// built.
pub struct PlannedUnit { name: UnitName, action: UnitAction }

pub struct UnitPlan {
    pub planned: Vec<PlannedUnit>,
    pub default_target: Option<PlannedUnit>,
    pub omitted: Vec<UnitOmission>,
    pub refused: Vec<String>,      // FIXME lines (§ 10.3)
    pub advisories: Vec<String>,   // comment blocks
}

pub fn plan_units(snap: &InspectionSnapshot, writes: &StagedWrites<'_>) -> UnitPlan;
```

**`staged_writes` is the stager's decision, not its candidate list.** Today
`staged_sources` (`crates/pipeline/src/render/configtree.rs:257`) lists
every included file, and the writer then drops the ones another section
owns and the ones `validate_path` refuses (`configtree.rs:375-388`). A
check that reads the candidate list can find a file the writer never
writes. The subtraction and the validation move into `staged_writes`, the
writer loops over it with no further test, and every "is this file in the
export" question in this spec is asked of it. The scheduled section's
generated units are in the same list.

For each included state change, the default target, and each generated
unit the scheduled section enables, the plan runs these rules in order. The
first one that fails decides.

| # | Rule | Applies to | When it fails |
|---|---|---|---|
| 1 | **No zero-length unit file is staged.** A zero-length file at `/etc/systemd/system/<name>` is a mask. `staged_writes` leaves it out, whichever section offered it, and the Containerfile names it in a FIXME through the existing `unstaged_files` route. | Every staged file, tested by destination path | The file is not written. A mask reaches the image only as a `mask` command that passes rules 3 and 4. |
| 2 | **Storage units act only on a package's file.** If the export stages a unit file at the name of a `PackagedStorage` unit, the administrator's file is what the image would load. | `enable`, `disable`, `mask` on a storage kind | Omitted: `the export carries an administrator's unit file for it (<path>)`. The mount advisory (§ 10.6) is rendered for the staged file. |
| 3 | **Protected units are not switched off** (§ 10.3). | `disable`, `mask` | A FIXME line, no command. |
| 4 | **A mask needs the path free.** `systemctl mask` makes a link at `/etc/systemd/system/<name>`, and the order in § 10.7 puts it after every `COPY`. | `mask` when a unit file is staged at the name | Omitted: `the export also carries a unit file at <path>; exclude one of the two`. |
| 5 | **The command needs a unit file.** See below. | `enable`, `disable`, `set-default` | Omitted: `UnitFileNotStaged`, or the package reason the presence tiers give today. |
| 6 | **An instance needs its configuration** (§ 10.8). | `enable` on an instance | Omitted: `InstanceConfigNotStaged`. |
| 7 | **The command's whole effect passes.** See below. | `enable`, `disable`, and `mask` until run R5 shows it does not follow `Also=` | Omitted: `AlsoEffect`. |

Rules 2 and 4 read the same fact, `StagedAt` for the unit's name, and they
answer the fleet case directly. One host's enabled, package-owned
`tmp.mount` and another host's administrator-written
`/etc/systemd/system/tmp.mount` merge into two rows that are each valid.
While the file's config row is included, rule 2 omits the enable and the
advisory explains the file. When the operator excludes the file, the
enable returns. Nothing in refine's toggle cascade needs to change for
this, and the merge's identity rule is untouched (§ 12).

**Rule 5, the unit file.** For an instance the file is its effective unit
(§ 7.2): its own file when the fragment's path names one, its template
otherwise.

| What the export stages at the name | The fragment the row declares | The file is in the image when |
|---|---|---|
| A unit file | Any | Always. The declared fragment is not consulted. |
| Nothing | `Vendor`, or `Shadowed` | The presence tiers say the owning package is (`classify_service_presence`, unchanged). `owning_package: None` keeps Tier 1: emit. |
| Nothing | `Local` | Never. |

The declared fragment says where the file was on the host that was
observed. The plan uses it only for the question the export cannot answer:
whether a package ships the unit. `OmissionReason` gains:

```rust
/// The unit's own file is not in the export: its config entry is excluded,
/// or its content was not captured.
UnitFileNotStaged,
/// The export carries an administrator's file for a storage unit (rule 2).
StorageFileStaged { path: String },
/// A mask and a unit file claim one path (rule 4).
MaskPathOccupied { path: String },
/// The command would also act on `also`, and that fails (rule 7).
AlsoEffect { also: Option<UnitName>, why: AlsoRefusal },

pub enum AlsoRefusal {
    /// The closure could not be read with confidence.
    Undetermined,
    /// It would disable or mask a unit § 10.3 protects.
    Protected,
    /// It would switch a unit a mount finding names.
    AdvisoryStorage,
    /// The other unit's own row is excluded or omitted.
    NotPlanned,
    /// The other unit was not in this state on the source host.
    StateDiffers,
}
```

`UnitFileNotStaged` reads `unit file /etc/systemd/system/<name> is not in
the export`. Because rule 5 asks `staged_writes`, the renderer and the
stager cannot disagree, which is the defect in
`inspectah-timer-enable-lines-unquoted-and-trust-empty-units` item 2.

`systemctl mask` needs no unit file, so rule 5 does not apply to a mask.
That `mask` succeeds for a unit that does not exist, that it fails when a
regular file occupies the path, and that `disable` fails for a unit that
does not exist, are not verified here; run R2 measures all three.

**Rule 7, the command's whole effect.** `systemctl enable U` also enables
every unit in `U`'s `Also=` closure, and `disable` also disables them
(§ 7.3). A rule checked on `U` alone is checked on the wrong thing: an
enabled `bundle.service` with `Also=wg-quick@wg0.service` enables the
instance whose own enable rule 6 just omitted, a `disable` can reach a
`bootc-*` unit, and an ordinary service can enable an administrator's
mount. So the command on `U` is planned only when every unit `M` in its
closure passes one of:

1. `M` is itself planned, for the same verb, in this export. It passed
   rules 1 to 6 on its own row.
2. `M` has no state change row, `inventory` records it in the state the
   verb produces, the verb on `M` is not one § 10.3 refuses, and no mount
   finding names `M`. The command then changes nothing about `M` that the
   source host and the image do not already agree on.

Otherwise the command on `U` is omitted with `AlsoEffect`, naming `M` and
the reason, and nothing is written in its place. A closure recorded as
`Undetermined` omits the command the same way. Because one omission can
remove the support for another, the plan applies rule 7 until no further
unit is omitted. Each pass only removes units, so it ends.

Two details keep rule 7 honest in an assembled export:

- When the export stages `U`'s unit file, or an included drop-in for `U`,
  the plan reads `Also=` from the staged content with the reader of § 7.2
  and compares it with the recorded closure. A name the record does not
  hold makes the effect `Undetermined`. This is the fleet case again: the
  record came from one host and the file from another.
- A generated unit is written from inspectah's own template
  (`make_timer_service`, `scheduled.rs:676`), which has no `Also=`. A
  generated unit whose name is also a collected unit's, or whose path
  another staged file claims, is refused with a FIXME, since two writers
  would claim one path.

**The no-RPM path runs every rule.** `render_service_intent` returns early
into `render_without_rpm` when the snapshot has no package section
(`service_intent.rs:519-522`). `plan_units` has no such branch. Without
package data only the presence tiers inside rule 5 have nothing to read,
and they emit, as that path does today. Rules 1 to 4, 6 and 7, and the
`Local` row of rule 5, read the units section and the staged files and
apply either way.

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

The guard holds on every route by which the export could switch such a
unit off:

- **The command.** Rule 3 of § 10.2.
- **A mask written as a file.** A zero-length
  `/etc/systemd/system/bootc-fetch-apply-updates.timer` masks the unit as
  surely as the command does. The collector records it as a `Masked` state
  change and no file (§ 7.7), and rule 1 refuses to stage a zero-length
  file at any unit path whichever section of an assembled or imported
  snapshot offers it. The test is on the destination path, not on the
  row's `unit` field, since `SystemdDropIn.unit` and `path` are separate
  strings in a snapshot.
- **Another unit's `Also=`.** Rule 7.

What the guard does not cover: a carried drop-in or full shadow for a
protected unit that changes what the unit does (an emptied `ExecStart=`, a
changed `OnCalendar=`). Those are unit content, carried on their own rows
like any other drop-in. The list names units whose state inspectah will not
reverse; it is not a claim that the carried configuration leaves boot and
updates working.

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

The names are one named constant in `crates/core/src/baseline.rs`: the
glob `dnf-automatic*.timer` and `packagekit-offline-update.service`. One
function beside it, `apply_runtime_updater_default`, sets the disposition
and the reason. It has three callers, and the default is the same at each:

- **The collector**, on a fresh scan.
- **The fleet merge**, on a fresh aggregate row (§ 12). Without this the
  merge's `set_include(true)` re-includes the unit on every merge.
- **Nothing in refine.** Refine's loader does not call it, so an operator's
  include survives export and reload.

**The refine loader's lock gives way.** `normalize_incompatible_services`
excludes and locks every unit in `INCOMPATIBLE_SERVICES`, and
`packagekit-offline-update.service` is one of the four names there
(`crates/core/src/baseline.rs:141`). A locked row cannot be included, which
contradicts the decision above. `packagekit-offline-update.service` is
removed from `INCOMPATIBLE_SERVICES`; the updater constant is its one
owner. A unit test asserts the two constants share no name, so a later
addition to either cannot bring the contradiction back. The three names
that stay (`dnf-makecache.service`, `dnf-makecache.timer`,
`packagekit.service`) keep today's locked exclusion, and the normalizer is
otherwise only migrated to the new types: it filters `inventory` where it
filtered `enabled_units`.

### 10.5 The default target

**`RUN systemctl set-default -- '<target>'` is emitted whenever the source
host set a default target, with no test of what the target is and no
comment beside it. (Mark Russell, 2026-11-13)** A host that boots to
`graphical.target` produces an image that does. `default_target` is
recorded as included and has a toggle like any other row. Rule 5 of § 10.2
applies, since `set-default` on a target whose file is not in the image
cannot succeed: a `Local` target whose config entry is excluded is omitted
with the `UnitFileNotStaged` reason. The operand is a `TargetUnit`, so no
other kind can reach the line.

### 10.6 Mount, automount and swap advisories

For each `MountUnitFinding`, which is to say each mount, automount or swap
unit the administrator wrote (§ 6.2), comment lines and nothing else. The
first line states the fact. Further lines are added per condition, computed
from `mount_point`:

| Condition | Text added |
|---|---|
| Always | `# Advisory: <unit> is <state> on the source host. inspectah does not enable, disable or mask a mount, automount or swap unit the administrator wrote.` |
| The unit's file is in `staged_writes` | `#   Its unit file is copied into the image. No enable line is written, but the unit still starts if another unit requires it or names its mount point in RequiresMountsFor=, and it replaces a unit of the same name that the image ships.` |
| `mount_point` is under `/usr` | `#   /usr is part of the read-only image on the target. The mount point must already exist in the image, and a mount there hides the image's own content under <path>.` |
| `mount_point` is `/`, `/boot`, `/boot/efi`, `/sysroot` or `/var` | `#   bootc and ostree own this mount point on the target.` |
| `mount_point` is, or is under, `/home`, `/srv`, `/mnt` or `/root` | `#   <path> is a symlink into /var in the RHEL bootc base image. The unit must be renamed for the resolved path (for /srv: var-srv.mount with Where=/var/srv).` |
| `mount_point` is, or is under, `/opt` | `#   /opt is a directory of the read-only image in the RHEL bootc base image, not a link into /var. The unit keeps its name. The mount point must already exist in the image, and a mount there hides the image's own content under <path>.` |
| A mount or swap with `nofail` absent | `#   No nofail option. If the unit is started at boot and its device is missing, units that require it fail, and boot can stop in emergency mode.` |
| The state is enabled or masked | `#   To carry it, add after the configuration COPY: RUN systemctl <verb> -- '<unit>'` |

The text states conditions, not outcomes. A copied unit file is not inert
because nothing enables it. A mount under `/usr` or `/opt` is possible and
is usually a mistake. A missing `nofail` matters only when the unit is
started and something requires it.

**Layout claims are tied to a base.** The two symlink lists are named
constants beside `VAR_SYMLINKED_TOPLEVELS`
(`crates/pipeline/src/render/selinux.rs:29`), whose comment records the
measurement they rest on: on `rhel9/rhel-bootc:9.8`, `/srv`, `/home`,
`/root` and `/mnt` are symlinks into `/var`, and `/opt` and `/usr/local`
are real directories. That is the only base measured. The advisory names
the RHEL bootc base for that reason, and run R5 repeats the measurement on
the RHEL 10 base before the text is used for it. A fedora-bootc or
centos-bootc base creates a different set and is not a target this text
covers. The claims about `/usr`, the bootc-owned mount points, and what a
mount over an image directory hides come from earlier analysis and from
`inspectah-srv-mount-units-must-target-var-srv`; run R5 checks them.

**The unit file itself stays a config entry, included by default, under the
config walk's control. No enable line is written for it, and the advisory
is where the hazards are explained. (Mark Russell, 2026-11-13)** Excluding
the file is the config row's toggle, as for any other file. This is so for
every administrator-written storage unit, a full copy of a vendor unit
included (§ 7.7): one owner, the config row.

A package-owned mount unit gets none of the table. It is a state change and
it renders in § 10.1.

**A carried mask the target relies on.** A package-owned storage unit that
is masked on the source is carried as a mask (§ 2). For `tmp.mount` the
same mask can mean something else on the target: on a package-mode host a
masked `tmp.mount` leaves `/tmp` on the root filesystem, which is writable,
and on an image-based host the directory under the mount may belong to the
read-only image. When the plan carries `mask` for a unit on a fixed list
(one named constant, holding `tmp.mount`), it adds:

```
# Advisory: tmp.mount is masked on the source host and the mask is carried. On the target, /tmp is then whatever the image provides without that mount. Confirm /tmp is writable on a host built from this image.
```

Whether `/tmp` is in fact unwritable in that case was not measured, on
either base. Run R5 boots the image and records it. If it is writable on
both, the advisory and its list go.

### 10.7 Order

The unit state section moves from step 2 to directly after the Configuration
Files section. Every `COPY` that can stage a unit file precedes it: drop-ins
and shadows, generated timers, and the config tree. The drop-ins `COPY`
stays where it is. This closes the local-service ordering defect in § 4 and
changes which fragment a full shadow's enable resolves.

The move is also why rules 1 and 4 of § 10.2 exist. With the commands
first, a later `COPY` overwrote the mask link with a zero-length file and
the unit stayed masked. With the commands last, `systemctl mask` would meet
a file already at its path. A mask is therefore never a staged file, and a
mask never shares a path with one.

### 10.8 An instance enable needs its configuration

An enable for `wg-quick@wg0.service` with no `/etc/wireguard/wg0.conf` in
the image builds, and then the unit fails on every boot. So an instance's
enable is carried only when every entry in its `required_config` is in the
image as the host had it. This is rule 6 of § 10.2, and it reads the same
`staged_writes`.

For each `RequiredConfig`, the plan first looks at what the snapshot holds
at the path (for a directory, at or under it), and only when it holds
nothing does it fall back to what the collector found on the host:

| The snapshot holds at the path | Result |
|---|---|
| An entry that is excluded, or that `staged_writes` leaves out for any other reason | Omitted: `its configuration <path> is excluded from the export` |
| An entry with `content_captured()` false | Omitted: `its configuration <path> was not captured` |
| An entry, and `snapshot.redactions` holds a finding for the path whose kind is `Inline` or `Excluded` | Omitted: `its configuration <path> holds a redacted value; provision the secret on the deployed host and enable the unit there` |
| An entry that is written, captured and has no such finding | This path passes. |
| Nothing, and `on_host` is `Missing` | Omitted: `its configuration <path> does not exist on the source host` |
| Nothing, and `on_host` is `Unowned` | Omitted: `its configuration <path> was not captured` |
| Nothing, and `on_host` is `Owned { package }` | This path passes when the presence tiers say `package` is in the image: the file arrives with it, unmodified. Otherwise omitted: `its configuration <path> belongs to package '<package>', which is not in the image` |

```rust
/// A template instance whose configuration is not in the image as the
/// source host had it.
InstanceConfigNotStaged { path: String, gap: ConfigGap },

pub enum ConfigGap {
    Excluded,
    NotCaptured,
    Redacted,
    Missing,
    PackageAbsent { package: String },
}
```

The first four rows apply whoever owns the file. A package-owned
configuration file the administrator changed is a config entry
(`crates/collect/src/inspectors/config/mod.rs:145-156` keeps every
modified one), it is redacted and toggled like an unowned file, and the
gate treats it the same. The last row is the only place ownership counts,
and only for a file the snapshot has no entry for, which for a
package-owned path means the file is the package's own.

"Redacted" reads the findings the redaction pass already wrote
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
`# Omitted:` line and on its refine row. That is the intended result, and
the wording says what to do next: the key reaches the host by a route other
than the image, and the enable belongs with that step. Nothing in the text
suggests putting the key back into the export.

What the gate covers and what it does not:

- **Covered:** an instance whose effective unit names its `/etc` path with
  the instance specifier, and the three templates in the fixed table
  (§ 7.5), whether the file is unowned, package-owned and changed,
  package-owned and untouched, or absent.
- **Not covered:** configuration found by a program's own convention for a
  template not in the table; configuration outside `/etc`; a file the listed
  configuration points to in turn (a key file named inside a WireGuard or
  OpenVPN configuration); and a secret the redaction patterns miss, which is
  carried in the file as it would be for any config entry. An instance with
  an empty `required_config` is carried on its unit file alone, with the
  `attention_reason` from § 7.5.
- **`--no-redaction`.** A scan taken with `--no-redaction` skips the
  redaction pass (`crates/cli/src/commands/scan.rs:728-732`) and records
  `RedactionState::Raw`. There are no findings, so the `Redacted` row never
  fires: the enable is carried, and the configuration is staged with its
  key in it. That is what the operator asked for with that flag, and the
  gate does not second-guess it. The instance's row carries an
  `attention_reason` saying its configuration is carried unredacted.
  Preserving password hashes is a different switch and does not change
  this: it exempts shadow entries only, and config content is still
  scrubbed.
- **`Flagged` findings.** A `Flagged` finding marks content the pass
  thought might be sensitive and did not change. The file is as the host
  had it, so the enable is carried. The instance's row carries an
  `attention_reason` naming the path and saying it holds flagged content
  that was not redacted. The gate omits only on `Inline` and `Excluded`,
  the two kinds that mean the staged file differs from the host's.
- **Not verified:** that the config walk records
  `/etc/wireguard/wg0.conf` on a stock host. Run R5 measures it.

**Data under `/var`.** When an instance's enable is carried and its
`data_paths` is not empty, one comment per instance precedes the `RUN`
lines:

```
# Advisory: dirsrv@example.service keeps data under /var/lib/dirsrv/slapd-example. The image does not carry it, and the carried enable starts the unit on first boot. Put the data in place before that boot, or the unit starts without it.
```

The row's `attention_reason` carries the same sentence. An enabled unit
starts on the first boot of a host built from the image, so "migrate
before the unit first starts" means before that boot: at provisioning, or
by a step ordered ahead of the unit. What a unit does when its data is
absent is the application's business. Some fail, and some create a fresh,
empty state and run. inspectah knows about instance data only through the
derivation and the fixed table in § 7.5, which holds `dirsrv@` and nothing
else. An instance whose template does not name its `/var` path and is not
in the table gets no advisory.

### 10.9 Hand-made link advisories

For each `ManualLink`, comment lines and nothing else:

| Cause | Text |
|---|---|
| `StaticUnit` | `# Advisory: <parent>.<relation>/<unit> is a link made by hand on the source host (<unit> has no [Install] section). Not carried.` |
| `NotInInstall` | `# Advisory: <parent>.<relation>/<unit> is a link made by hand on the source host (the unit's [Install] section does not name <parent>). Not carried.` |
| `PartialInstall` | `# Advisory: <unit> has some of the links its [Install] section names and not others, so neither enable nor disable reproduces the source host. Not carried. Present: <parent>.<relation>/<unit>.` |
| `Undetermined` | `# Advisory: <parent>.<relation>/<unit> could not be matched to the unit's [Install] section. Not carried.` |
| `HostDevice` | `# Advisory: <unit> is tied to a device of the source host. Not carried.` |

For a link with relation `Alias` the first words are `<parent> is a link to
<unit>` in place of the directory path.

For the first four causes, with relation `Wants` or `Requires`, one more
line gives the command:

```
#   To carry it, add after the configuration COPY: RUN systemctl add-wants -- '<parent>' '<unit>'
```

with `add-requires` for a `.requires` link. An `Upholds` link gets no
command line, since `systemctl` has no verb that makes one; the text says
to add `Upholds=` to a drop-in for the parent instead. An `Alias` link gets
no command line either; the text says to add `Alias=` in a drop-in or make
the link in the Containerfile. A `HostDevice` link gets no command at all.

`add-wants` is offered in place of a literal `ln -s` for two reasons. It
resolves the unit's path inside the image, so the link cannot point at a
fragment the image does not load, which is the hazard recorded in
`inspectah-full-shadow-wants-link-points-at-the-vendor-path`. And it is
expected to fail the build when the unit is missing, where a dangling link
would build and do nothing. Both are expectations from the documented
behavior of `systemctl`. Whether `add-wants` runs at all at build time,
with no manager to talk to, was not checked; run R5 checks it.

Both names are proved `UnitName`s and are single-quoted like every other
operand.

## 11. Refine

### 11.1 Data contract

The view response carries, in place of `service_states`:

| Field | Content |
|---|---|
| `unit_states[]` | One per `UnitStateChange`, package-owned mount units, template instances and masked templates included: `unit`, `kind` (derived, lowercase), `instance_of` (the template name, derived, when the unit is an instance), `current_state`, `default_state`, `fragment` (`vendor`, `local`, `shadowed`), `unit_file_path` when not vendor, `owning_package`, `also[]` or `also_undetermined`, `required_config[]` (each `path` and `on_host`), `data_paths[]`, `include`, `locked`, `triage`, `attention_reason`, and `omitted` with the `OmissionReason::describe` text when the plan of § 10.2 omits the unit. A runtime package updater (§ 10.4) is an ordinary row with `include: false`. |
| `unit_dropins[]` | As `service_dropins` today, with `kind` of the parent. A drop-in whose parent has no state change is still listed, since a unit at its preset has a drop-in and no parent row. A full shadow carries its `shadow_type` and rationale here, which is where the "base image updates to this unit are ignored" explanation is shown. |
| `default_target` | `target`, `include`, `locked`, `omitted`, or absent. |
| `mount_findings[]` | `unit`, `kind`, `state`, `mount_point`, `fragment` (`local`, `shadowed`, or `vendor` for an unowned one), `unit_file_path` (the config row that holds the unit file and its toggle), and `advisories[]`, the same strings § 10.6 renders. No `include`: these rows have no toggle. No mount source: the row links to the config entry, which holds the scrubbed file (§ 6.5). |
| `manual_links[]` | `relation`, `parent`, `unit`, `cause`, and `advisories[]`, the same strings § 10.9 renders. No `include`. |
| `scheduled_tasks` timers | Each row gains `unit` (`<name>.timer`) and `unit_row` (whether `unit_states` holds it), so the row can link to its control. No `include`. |

`omitted` is computed by calling `plan_units` on the session's current
snapshot, so the row shows what the export would do with the toggles as
they stand, including an omission caused by another row (rules 2, 4 and 7
of § 10.2).

Operations: `ItemId::Service { unit }` becomes `ItemId::Unit { unit }` and
keeps its cascade to drop-ins and its membership check before it applies.
`ItemId::DefaultTarget` is added; it toggles the recorded target and takes
no target name of its own. `ItemId::SystemdTimer` is removed. `SetInclude`
on a mount finding or a hand-made link does not exist, so there is nothing
to refuse.

The cascade from a unit row to its drop-ins is kept, full shadows
included. It cannot produce an enable on an administrator's storage file:
a storage unit's file is a config entry, not a drop-in (§ 7.7), so the
cascade never touches it, and whatever the toggles say, § 10.2 decides
from the files the export stages.

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

The design pass owns grouping and presentation only. Every field and
string key the frontend reads is migrated before it starts (§ 9.1).

### 11.3 Orphan-shadow synthesis is retired

`synthesize_orphan_shadows` (`crates/refine/src/classify.rs:547`) is
removed, with its call in `RefineSession::new`.

It exists to give a full shadow a parent row to toggle when the unit has no
state divergence. It does that by writing a `ServiceStateChange` the
collector never produced: an enabled unit at its preset becomes a row that
says its state changed, with `default_state: None`. That is a claim about
the host the host did not make, and under this model it would also need a
fragment and an `AlsoEffect` invented for it.

Nothing it provides is lost:

- **The toggle.** A full shadow is already its own row with its own
  operation, `ItemId::DropIn { path }` (`session.rs:2823`). The parent
  cascade is a convenience where a parent exists.
- **The explanation.** `shadow_type` and `shadow_rationale` stay on the
  drop-in (§ 11.1). The state-change copies of those two fields go.
- **The count.** The frontend's shadow count reads `unit_dropins` entries
  whose `shadow_type` is `full_shadow` in place of state rows.

A unit that has both a state change and a full shadow has a `Shadowed`
fragment on the state change, from the collector.

## 12. Fleet

The merge compiles against the new types. Its identity rule and its
narrowing are unchanged. Two things are new: one named exception to how it
sets `include`, and the statement of where a merged export is validated.

- `UnitStateChange` keeps identity on the unit name
  (`merge.rs:147-150`) and merges with `merge_items`.
- `inventory` merges by unit name, keeping the most prevalent state, where
  the three string lists were unioned.
- `mount_findings` merge by unit name, and `manual_links` by relation,
  parent and unit. Neither has an include flag, so narrowing does nothing
  to them.
- A template instance is a `UnitStateChange` and merges as one. Three
  fields of a state change are merged toward the cautious side, across
  every host that has the unit and not only the representative:
  - `also`: the union of the closures; `Undetermined` on any host makes the
    merged value `Undetermined`.
  - `required_config`: the union of the paths; where hosts disagree about
    one path, `Missing` wins over `Unowned`, which wins over `Owned`.
  - `data_paths`: the union.
- `default_target` takes the most prevalent target, and is excluded when the
  hosts do not all agree, the same rule `narrow_non_universal` applies.
- `SystemdTimer` no longer has an include flag to narrow.

**The exception: a runtime package updater starts excluded.** `merge_items`
calls `set_include(true)` on every representative (`merge.rs:557`, `:577`,
`:658`), and narrowing then excludes only what is not on every host. Left
alone, a `dnf-automatic.timer` that every host has, each one excluded by
the collector, comes out of the merge included and gets an enable line
nobody chose. After `merge_items`, the units merge calls
`apply_runtime_updater_default` (§ 10.4) on its result, so a fresh
aggregate row for a unit on that list is excluded with the stated reason
whatever its prevalence. The operator's include in refine, made afterward
on the aggregate, is kept through export and reload like any other toggle.
A per-host include made before the merge is not carried into the
aggregate. That is today's behavior for every row, and it is not changed.

**A merged export is validated at render, not at merge.** The merge joins
rows by name and does not ask whether they agree. One host's
package-owned `tmp.mount` state change and another host's
administrator-written `tmp.mount` finding and unit file are both kept, as
are a mask from one host and a unit file of the same name from another,
and a recorded `Also=` closure beside a unit file that names more. Each of
those is decided by § 10.2 from the files the operator leaves selected,
which is the only point at which the answer exists. Teaching the merge to
reconcile them would be the aggregation redesign § 3 rules out, and would
still miss a snapshot edited or imported after the merge.

One aggregate defect closes as a side effect: a local timer present on some
hosts was excluded by both of its owners and written anyway, because the
scheduled writer ignored disposition. With one writer that honors the config
entry's disposition, the exclusion holds.

One existing behavior is recorded and not changed: identity on the name
alone means hosts that disagree about a unit's state collapse to the most
prevalent state, with a prevalence count of every host that had any state
change for it. That is an aggregation design question and stays out.

## 13. Verification

Tests the implementation plan must contain. Three rules apply to all of
them:

- **A refusal test names what refused.** A snapshot that must fail to load
  is written at the current schema version and asserts the error that
  names the intended rule, so it cannot pass because something unrelated
  rejected the payload.
- **An absence test proves presence first.** Before asserting that no
  `systemctl` line names a unit, the test asserts that the finding, the
  refine row, or the marker it compares against exists.
- **An omission test has a positive control.** The same fixture, through
  the same route, with the one condition reversed, produces the line.

Phase 1 (the rename and the types):

- **T1. Schema.** Version 27 is refused through
  `InspectionSnapshot::load`, refine's `from_tarball` and aggregate's
  `load_snapshot_from_tarball`, each asserting the version refusal and not
  a generic load failure. A valid version 28 payload loads through the
  same three.
- **T2. Types.** For each of `.mount`, `.automount` and `.swap`: a
  `state_changes` entry with a `local` fragment, with a `shadowed`
  fragment, and with a `vendor` fragment and a null owner each fail to
  load; the same name with a `vendor` fragment and an owner loads; that
  last one placed in `mount_findings` fails to load. A `default_target`
  naming `cockpit.socket` fails, one naming a packaged `tmp.mount` fails,
  and a `.target` round-trips. A template name with state `enabled` fails
  and with `masked` loads. A property test over `UnitSubject` shows
  `classify` is total and that serializing either side and loading it
  returns the same side; the enumerated cases above are what show the
  classification is the right one. A compile-fail or signature test shows
  `systemctl_lines` takes `PlannedUnit` only and that `PlannedUnit` has no
  public constructor.
- **T3. Consumers.** Through the production handlers: the view response
  holds `unit_states` and `unit_dropins` and neither old key; a config
  entry that a drop-in owns serializes `shadowed_by: "units"`; the section
  host counts and `SectionGroup::for_section` use `units`. In the frontend
  suite: the section renders a non-empty list from `unit_states`, and the
  ownership link resolves for `"units"`.
- **T4. No synthetic rows.** An enabled, preset-matched service with a full
  shadow, taken through `RefineSession::new`: `unit_states` has no row for
  it, `unit_dropins` has the shadow with its `shadow_type`, and toggling
  the drop-in alone removes the file from the staged tree and restores it.

Phase 2 (every kind):

- **T5. Collector, socket.** `MockExecutor` with `cockpit.socket` reported
  `enabled` against a `disable` preset. Assert one `UnitStateChange` with
  kind `Socket`, a `Vendor` fragment and the owning package, where the
  fragment and the owner come from the mocked filesystem and RPM answers
  and not from the fixture's expected output.
- **T6. Render, excluded owner.** That snapshot with the owner included
  yields the enable. With the owner excluded, and nothing else changed, it
  yields `# Omitted: cockpit.socket (...)` and no `systemctl enable`
  naming the socket.
- **T7. Collector edges.** `dbus-broker.service` is kept and a `dbus-org.`
  alias is dropped. A local enabled unit with no matching preset rule is a
  state change. A `generated` unit and each runtime state produce no state
  change.
- **T8. Storage units.** `tmp.mount` owned by a package and enabled against
  its preset yields a state change and appears in `RUN systemctl enable`;
  masked, in `RUN systemctl mask`. A local unit of each storage kind,
  enabled, yields an advisory and no instruction. A vendor mount unit
  whose owner lookup fails yields an advisory. A `tmp.mount` with a full
  copy in `/etc` yields an advisory and no instruction, its file is a
  config entry and not a `drop_ins` entry, and excluding and re-including
  that config row removes and restores the staged file.
- **T9. Mount advisory text.** A local `srv.mount`: the advisory with the
  `/var` rename line. A local `opt-vendor.mount` with `Where=/opt/vendor`:
  no rename line, and the `/opt` line. A mount under `/usr`: the `/usr`
  line. The unit-file line is present while the config row is included
  and absent after it is excluded. No `systemctl` instruction names any of
  them.
- **T10. Extracted fields.** A local mount unit whose `What=` holds a token
  the generic pattern matches, through scan and through a refine export:
  the token is absent from the whole serialized snapshot and from the
  view response. The field guard destructures every type § 6.7 lists.
- **T11. Masks.** A `/dev/null` link with a vendor counterpart yields one
  `Masked` state change, no drop-in entry and no staged file. A
  zero-length regular file yields the same, with and without a vendor
  counterpart. An alias link yields nothing. A masked template yields a
  `Masked` state change on the template name, its name in
  `RUN systemctl mask`, and no staged file.
- **T12. Protected units.** A table over `bootc-*`, `ostree-*` and
  `systemd-remount-fs.service`, for `disable` and for `mask`: the FIXME and
  no command. An `enable` on a protected name is carried. A zero-length
  file for a protected unit, once from a scan and once in an imported
  snapshot as a full-shadow drop-in and as a config entry: no `mask`
  command and no file at that path in the staged tree.
- **T13. Order.** A local service with no paired timer: the config `COPY`
  line and the enable line both exist, and the enable follows.
- **T14. Quoting.** A unit named with `\x2d` and one named `-.slice` appear
  as quoted operands after `--` inside a `RUN systemctl` instruction.
- **T15. Unit file.** A local enabled socket whose config entry is
  excluded: no file in the staged tree, an `# Omitted:` line with the
  `UnitFileNotStaged` wording, and no enable. Each of the two toggles is
  driven through `RefineSession` on its own, and each accepted operation
  is shown to change the staged tree or the Containerfile, as
  `cross-inspector-shadow-passes.md` requires for an overlapping path. The
  same omission holds for a snapshot with no package section.
- **T16. Mask path.** A masked unit and a staged unit file of the same
  name: the mask is omitted with the `MaskPathOccupied` wording. Excluding
  the file brings the mask back.
- **T17. Runtime package updaters.** For `dnf-automatic.timer`, each
  variant and `packagekit-offline-update.service`: a state change with
  `include: false` and the stated reason, the `# Omitted:` comment, and no
  enable. Through the production refine loader the row is not locked;
  `SetInclude` yields the enable; after export and reload the include is
  still set. The two name constants share no entry.
- **T18. Fleet.** (a) Two hosts, one with a package-owned enabled
  `tmp.mount` and one with an administrator's full copy, merged and taken
  through `RefineSession`: while the file's config row is included there
  is no enable and the advisory is present, whatever the unit row's toggle
  and its cascade do; excluding the file yields the enable. (b) Two hosts
  that both have `dnf-automatic.timer`: the merged row is excluded and
  there is no enable; an explicit include then yields it. (c) A mask on
  one host and a local unit file of the same name on another: T16's
  result, on the merged snapshot.

Phase 3 (one control per fact):

- **T19. Disabled local timer stays disabled.** Collected through the real
  inspectors: a local `.timer` and paired `.service`, the timer reported
  `disabled`. No `systemctl enable` names the timer. Excluding the timer's
  config row leaves no copy of either file anywhere in the staged tree,
  which is the assertion a second writer fails.
- **T20. The timer control is gone.** No view or aggregate response offers
  an include flag for a timer, and a request that names the removed
  operation is rejected, not accepted and ignored.
- **T21. Generated units.** A generated timer's enable is produced through
  `plan_units` and the shared helper. A generated unit whose name is a
  collected unit's is refused with a FIXME.

Phase 4 (the `/etc/systemd/system` scan):

- **T22. Install state.** `backup.service` with
  `WantedBy=multi-user.target`, reported `enabled`, whose only link is in
  `rescue.target.wants/`: with a `disable` preset, no state change, one
  `ManualLink` with cause `NotInInstall`, an inventory state of
  `Disabled`, and no enable; with an `enable` preset, a `Disabled` state
  change and the same advisory. With both links present: an `Enabled`
  state change and one advisory for the extra link. With two `WantedBy=`
  values and one link: `PartialInstall` and no state change. The same four
  cases for a template instance.
- **T23. Hand-made links.** A `multi-user.target.wants/backup.service`
  link to a unit with no `[Install]` section: the `ManualLink` exists with
  cause `StaticUnit` and its refine row exists; then the advisory holds
  the `add-wants` line, no `RUN` instruction names the unit, and the row
  has no include field. Table cases for a `.requires` link
  (`add-requires`), an unexpanded specifier (`Undetermined`),
  `systemd-fsck@` (`HostDevice`, no command line), and a hand-made alias.
  An `.upholds` link to a local unit, through both the units and the
  config inspectors: the advisory exists and the link's path is absent
  from `config.files` and from the staged tree.
- **T24. Effective unit.** A concrete `worker@site.service` under `/etc`
  beside a vendor `worker@.service`: the state change has a `Local`
  fragment naming the instance file; with the template's package excluded
  and the file staged, the enable is carried; with the file excluded it is
  omitted as `UnitFileNotStaged`. A unit whose `WantedBy=` comes only from
  a drop-in: an `Undetermined` advisory and no enable.
- **T25. `Also=`.** (a) An enabled `bundle.service` with
  `Also=wg-quick@wg0.service`, the instance omitted for a redacted key:
  both are omitted and the reason names the instance. (b) A disabled unit
  whose `Also=` names a `bootc-*` unit: omitted, reason `Protected`.
  (c) A service whose `Also=` names a local mount unit: omitted. (d) A
  service, socket and path that name each other and are all enabled: all
  three carried. (e) A closure recorded `Undetermined`: omitted. (f) A
  staged unit file whose `Also=` names a unit the recorded closure does
  not hold: omitted. (g) A chain of three in which the last is omitted:
  all three are.
- **T26. Default target.** A `default.target` link to `graphical.target`
  yields `RUN systemctl set-default -- 'graphical.target'` with no
  comment beside it. No link yields no line. Excluding the row through
  `RefineSession` removes the line and including it restores it. A local
  target whose config row is excluded is omitted.
- **T27. The instance floor.** An enabled instance found through the
  directory scan: a state change exists, a refine row exists, an
  `# Omitted:` line carries the placeholder reason, and no enable names
  it. An instance whose link is under `/run` is not found.

Phase 5 (carried instances):

- **T28. Carried instance.** A `multi-user.target.wants/openvpn-client@site.service`
  link, a vendor template whose `[Install]` names `multi-user.target`, and
  an included `/etc/openvpn/client/site.conf` with no redaction finding,
  collected through the real inspectors. Assert one `UnitStateChange`
  named for the instance with the template's owner, and the instance name
  in `RUN systemctl enable`. An instance linked from two parents is one
  state change. `dirsrv@example.service` with an existing data directory
  has the data advisory beside a carried enable, and a template that names
  `/var/run/worker/%i.pid` has an empty `data_paths`.
- **T29. Instance whose configuration is not in the image.** Each case
  asserts an `# Omitted:` line with the matching wording and no enable
  naming the instance: the config entry excluded through `RefineSession`
  (and the enable returning when it is included again); the content not
  captured; `wg-quick@wg0.service` with a `PrivateKey =` line taken
  through the real redaction pass, which also asserts the staged file
  holds no key and the reason tells the operator to provision the secret
  on the host; the file missing on the source host; a package-owned file
  the administrator changed, with a credential the pass redacts; and a
  package-owned, unmodified file whose package is excluded. The positive
  controls, through the same route: a clean unowned file, and a
  package-owned unmodified file whose package is in the image. Under
  `--no-redaction` the WireGuard instance is carried with the attention
  note, and a `Flagged` finding on the path leaves the enable carried
  with its note.

Real-host runs, specified here and not run for this spec. **Do not use the
CentOS9 or fedora44 VMs for these runs.** § 15 says when each happens.

- **R1. `cockpit.socket` end to end.** On a RHEL 9 host and a RHEL 10
  host: record `systemctl is-enabled cockpit.socket` in the unmodified
  base image first. Then `systemctl enable --now cockpit.socket` on the
  host, scan, confirm the exported Containerfile holds the enable line,
  build it, and confirm the built image reads `enabled`. If the base
  already enables the socket, the run proves the line and not the effect,
  and says so.
- **R2. Same run, more checks.** Each records the exact outcome and the
  resolved fragment, not only whether the build passed. A local service
  with no paired timer builds (the § 4 ordering defect, before and
  after). A full shadow's wants-link target and the fragment
  `systemctl cat` reports, in the built image and after booting it. A
  unit with `\x2d` in its name survives `RUN systemctl enable -- '...'`.
  `systemctl mask` on a unit absent from the image; on a name where a
  regular, non-empty file is already at the `/etc` path; and where a
  zero-length file is. `systemctl disable` on a unit absent from the
  image.
- **R3. Survey.** `systemctl list-unit-files --no-pager` on RHEL 9 and
  RHEL 10 hosts, stock and after the driftify standard profile. Count,
  per unit type, units whose state differs from preset, and count the
  instance links, hand-made links and `Also=` lines among enabled units.
  This tests the assumption that targets, slices and mounts rarely
  differ, and sizes the collapsed group in § 11.2. Record the result in
  `process-docs/skills/real-host-validation-runs.md`.
- **R4. Stock-host false positives.** Scan a stock host of each release
  before a drifted one, and review every row the new kinds add, every
  hand-made link advisory, every instance and every `AlsoEffect`
  omission. Keep the classified rows and their reasons as the evidence.
- **R5. Instances, links, layout and mount units.** On a RHEL 9 host and
  a RHEL 10 host, and in builds from the RHEL 9 and RHEL 10 bootc base
  images. Parts (a) to (k) need no inspectah build. Outcomes that are
  confirmed become fixtures where that is practical.
  a. `systemctl enable wg-quick@wg0 && systemctl list-unit-files
     'wg-quick@*'`. Record which names are listed, with which state, and
     where the link was made. Then, for a unit with
     `WantedBy=multi-user.target`: what `list-unit-files` and
     `is-enabled` report when its only link is hand-made in another
     target's `.wants`, and when its only link is an alias.
  b. Read the installed `wg-quick@.service`, `openvpn-client@.service`,
     `openvpn-server@.service` and `dirsrv@.service`. Record the path each
     really reads, to confirm or correct the two fixed tables in § 7.5,
     and what the derivation would produce for each.
  c. In a build from the bootc base image: `RUN systemctl add-wants --
     'multi-user.target' '<unit with no [Install]>'`. Record whether it
     succeeds with no manager running, where the link points, where it
     points when the unit has a full shadow in `/etc`, and whether the
     build fails when the unit is absent. The same for `add-requires`.
  d. In the same build: `RUN systemctl enable -- 'tmp.mount'`, and
     `mask`. Record the result and the state in the built image. Then
     boot each image and record whether `/tmp` is writable, what backs
     it, and whether the unit a `mask` was meant to stop is in fact
     stopped.
  e. `systemctl --version`, and whether a unit with `UpheldBy=` produces
     an `.upholds/` link on enable, on the source host and in the target
     base.
  f. On a host with a serial console, where the
     `serial-getty@ttyS0.service` link lives.
  g. `Also=`. A unit `a.service` with `Also=b.service`, and `b` with
     `Also=c.service`, in a build with no manager: which of the three
     `enable a`, `disable a` and `mask a` each act on, and what `enable a`
     does when `b` is absent from the image.
  h. The effective unit. A concrete instance file under `/etc` beside a
     vendor template: which one `systemctl cat` and `enable` use. A unit
     whose `WantedBy=` is set only in a drop-in: whether `enable` makes
     the link, on each release.
  i. A vendor dependency. A unit a package's `.wants` link pulls in,
     disabled by a `RUN systemctl disable` line: whether it starts on the
     booted image.
  j. The target layout. `ls -ld /opt /usr/local /home /srv /mnt /root`
     in each base image. A mount unit with `Where=/opt/x`, with and
     without the directory in the image, and one under `/usr`: whether
     each mounts on boot and what it hides.
  k. A `graphical.target` default with no display manager package in the
     image: whether it builds, and what state the booted host reaches.
  l. With the branch build: scan a host with an enabled
     `wg-quick@wg0` whose configuration holds a key. Record whether the
     config walk recorded the file and that the enable is omitted.
  m. With the branch build: an instance with clean configuration, carried
     end to end, reads `enabled` in the built image and is active after
     boot.

## 14. Backlog items

Each item was read in full before the design, and its claims checked
against `70aa8996`.

| Item | Result | Still right? |
|---|---|---|
| `inspectah-socket-units-are-never-collected` | Closed by phase 2 and run R1. | Yes. All four cited sites confirmed. |
| `inspectah-local-timer-has-two-controls-and-neither-works` | Closed by phase 3. Control 1 goes with the second writer; control 2 is removed, and its cause is in § 4. | Yes, with stale line numbers: the writer is now `configtree.rs:440-449`. |
| `inspectah-timer-enable-lines-unquoted-and-trust-empty-units` | Closed by phases 2 (quoting, file check) and 3 (single writer). | Partly stale. Since `2c2e52aa` the names pass `is_shell_operand`, so `#skip` and a leading option are refused. They are still bare, with no `--`, and glob characters pass. Items 2 and 3 stand. |
| `inspectah-write-the-collector-overlap-contract` | Partly covered. § 8 and § 7.7 are the ownership table for `/etc/systemd/system`, the `*.wants/*` exclusion is answered in § 7.7 and extended to `*.upholds/*`, and the missing `scheduled_owned_paths` is made unnecessary. Phase 4 writes this part into `process-docs/` and fixes `docs/reference/inspector-coverage.md`. Remaining: the general rules document, and the non-systemd claimants (quadlets, repo files, GPG keys). | Yes. |
| `inspectah-masked-units-are-recorded-as-full-shadows` | Closed by phase 2 (§ 7.7), for the `/dev/null` link and for the zero-length file. The alias question is answered: an alias `[Install]` names produces nothing, and a hand-made one is advised (§ 7.3). | Yes. |
| `inspectah-full-shadow-wants-link-points-at-the-vendor-path` | Partly covered. The order change in § 10.7 changes what the enable resolves, and the item is the reason § 10.9 offers `add-wants` and not a literal link. The measurement the item asks for is run R2, and run R5 (c) repeats it for `add-wants`. | Yes; still unmeasured. |
| `inspectah-srv-mount-units-must-target-var-srv` | Partly covered. The advisory in § 10.6 names the problem and the rename, for the four top-level paths measured as links into `/var`. Remaining: the unit file is still carried as config by default, which is decided (§ 10.6); there is no automatic rewrite, and fstab lines and the audit report are untouched. | Yes. |
| `inspectah-etc-unit-files-accessor` | Out of scope. Its blocker moves: after phase 4 the `/etc/systemd/system` claimants are config and units, with scheduled no longer one, which changes the accessor's signature. | Yes. |
| `inspectah-var-unit-grep-matches-comments-and-longer-names` | Out of scope. It concerns the `/var` backing search in `storage.rs`, which reads unit files for a different question. The unanchored pattern is still at `storage.rs:579`. | Yes, as narrowed in the item. |
| `inspectah-etc-systemd-user-has-no-symlink-rule` | Out of scope. User units are a non-goal (§ 3). | Not re-measured. |
| `inspectah-redaction-pseudo-paths-miss-systemd-comment-rule` | Out of scope, with one effect: once `SystemdTimer` holds no content, the `timer:` pseudo-path is left for generated units only. | Not re-checked. |
| `inspectah-unredacted-export-carries-no-marker` | Out of scope. § 10.8 touches the same ground: under `--no-redaction` an instance is carried with its key, and the row's attention note is the only sign. The export-level marker the item asks for is still missing. | The `--ack-sensitive` gate it cites is in place (`scan.rs:227`). The rest was not re-checked. |
| `inspectah-redaction-token-eats-the-key-it-replaces` | Out of scope, with one effect: a redacted instance configuration is staged with the token in place of the key line, which is why § 10.8 omits the enable. | Not re-checked. |

## 15. Phases

**All phases land on one feature branch, and the branch merges to `main`
once, carrying the single schema bump. (Mark Russell, 2026-11-13)** Each
phase is still its own unit of work: it compiles, passes the workspace gates
at its last commit, and is reviewed alone before the next phase starts.
Phase 1 sets schema 28 (§ 9). `main` is merged into the branch between
phases, so the final merge is not the first time the two meet.

**Phase 1: the rename and the types. Size L, wide and mechanical.**
`units.rs` types (§ 6) with the classification by kind and fragment, the
checked constructors, `TargetUnit`, and `PlannedUnit` built by today's
presence logic moved into the planning module; the section rename
and schema 28 (§ 9); every consumer in § 9.1, the string-keyed ones and the
frontend's fields, ids and test mocks included; the single inventory list;
and the retirement of orphan-shadow synthesis (§ 11.3). The collector
still asks for services only and behaves as it does today, so this phase
changes names and types and no collected fact. Refine shows the existing
section. Tests T1 to T4.

**Phase 2: every kind. Size L, at the upper end.**
`UnitsInspector` over every kind from `list-unit-files` (§ 7.1 sources 1
and 2, § 7.4) with the owner lookup ahead of classification for storage
kinds; the `dbus-org.` narrowing; masks by form, the zero-length file and
the masked template included, and drop-ins and shadows for every kind with
one owner per path (§ 7.7); package-owned mount units as state changes;
mount findings, their advisories and the carried-mask advisory (§ 10.6);
the redaction ownership of every extracted field (§ 6.7); the quoting
helper and `--` at every `systemctl` site; the unit state section moved
after the config `COPY` (§ 10.7); `staged_writes` and `plan_units` with
rules 1 to 5 (§ 10.2), on both the package and the no-package path; the
protected-unit guard (§ 10.3); the runtime-updater default, the normalizer
change and the merge exception (§ 10.4, § 12). Refine shows the existing
section with every kind in one flat list. Tests T5 to T18. Local timers
keep today's deferral to the scheduled section, so their defects are
unchanged by this phase.

Known limit of phases 1 and 2: `enabled` is still taken on
`list-unit-files`'s word, as it is today, and `Also=` is not read. A unit
that is `enabled` only through a hand-made link gets an enable line, and a
command's effect on the units its `[Install]` section names is unchecked.
Both are today's behavior for services, extended to the new kinds. Phase 4
closes both, and the branch does not merge before it (§ 16 question 3).

**Phase 3: one control per fact. Size M.**
Every change in § 8: the second writer and the local-timer enable removed,
`SystemdTimer` reduced to a description, `ItemId::SystemdTimer` and the
aggregate timer control removed, and generated units taken through
`plan_units`. Tests T19 to T21.

**Phase 4: the `/etc/systemd/system` scan. Size L, at the upper end.**
The effective-unit resolution and the `[Install]` reader in the core crate
(§ 7.2); the link scan, the install state and the `Also=` closure (§ 7.1
sources 3 and 4, § 7.3); `ManualLink` with its causes, the host-device
list and the advisories (§ 10.9); rule 7 of § 10.2; the `.upholds`
exclusion in the config walk; the default target (§ 7.6, § 10.5); and the
overlap contract text. Enabled instances are collected as state changes in
this phase and every one of them is omitted at render with a placeholder
reason, `template instances are not carried yet`, so each is named in an
`# Omitted:` line and on a refine row. Tests T22 to T27.

**Phase 5: carried instances. Size M.**
`required_config` with `ConfigOnHost`, and `data_paths`, with their
derivation and fixed tables (§ 7.5); rule 6 of § 10.2 and
`InstanceConfigNotStaged`; the attention notes for unredacted and flagged
configuration; and the data advisory (§ 10.8). The placeholder omission
from phase 4 is removed. Tests T28 and T29.

**The floor for instances** is the end of phase 4: every enabled instance
named in an advisory line, none carried. If phase 5 slips, the branch is
complete without it only by the project owner's call (§ 16 question 3).

**Phase 6: the Systemd units section. Size M, frontend.**
§ 11.2 after its own design pass. Grouping and presentation only: the
fields it reads were migrated in phase 1 and extended by the phase that
introduced each.

**Real-host runs all happen on the branch, before the merge.**

| When | Runs | Why then |
|---|---|---|
| Before phase 2 is planned | R5 (d), (j) and the mask cases of R2 | Phase 2 writes the mount-point text, the carried-mask advisory and the two mask rules, and each rests on behavior not yet measured. None needs an inspectah build. |
| Before phase 4 is planned | R5 (a) to (c), (e) to (i), (k) | They need no inspectah build, and phases 4 and 5 are designed on their answers: what `list-unit-files` lists, which file an instance loads, whether `enable` reads `[Install]` from drop-ins, what `Also=` does offline, whether `add-wants` works at build time, the table paths. |
| After phase 2 | R1, R2, R3, R4 | The results can overturn premises in § 4 and § 17 while only two phases rest on them. |
| At the branch tip, after the last phase | R1, R2, R4 and R5 in full | This is the merge gate. The branch does not merge to `main` until they pass. |

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
   One thing is fixed under either answer: the `/etc` file is a config
   entry under the config row's toggle (§ 7.7), and rule 2 of § 10.2 is
   what a "carry" answer would relax, for the shadowed case only.
2. **The fixed lists (§ 7.5).** Instance configuration: `wg-quick@`,
   `openvpn-client@`, `openvpn-server@`. Instance data: `dirsrv@`. Tied to a
   host's devices: `systemd-cryptsetup@`, `systemd-fsck@`. Confirm or amend
   the starting contents. And confirm the default for an instance inspectah
   knows nothing about: the spec carries its enable with an attention note;
   the alternative is an advisory until a template is added to a list.
3. **What the branch must hold to merge (§ 15).** All phases land on the
   branch. Two cases need a ruling. Phase 6 depends on a frontend design
   pass, and the driving fix for `cockpit.socket` waits for it; phase 2
   already shows every kind in the existing flat list. And if phase 5
   slips, the instance floor is in place after phase 4. May the branch
   merge after phase 5 with the flat list, or after phase 4 with the floor?
   The spec assumes it does not merge before phase 4: until then an enable
   is admitted on `list-unit-files`'s word and `Also=` is unchecked
   (§ 15).

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
  the end of phase 2 and reaches `main` only when the last phase does. A
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
  mistake impossible on one observation, and it still needs § 10.2 behind
  it for an assembled export. If that second check is judged enough on its
  own, a single `UnitKind` field and the plan's rule 2 are simpler. The
  types earn their place only if a snapshot that states the impossible
  should fail to load and not merely fail to render.
- **Rule 7 omits commands an operator would expect.** A unit whose
  `Also=` names a unit in a different state on the host, or one inspectah
  could not read, loses its line, and the operator adds it by hand. The
  rule prefers a missing line to a command with an effect nobody
  reviewed. On a fleet with many bundled units that preference could
  produce a wall of omissions, and nothing here measures how common
  `Also=` is among enabled units. The survey counts it.
- **The plan reads unit files twice.** The collector resolves the
  effective unit on the host, and render reads `[Install]` again from
  staged content. One reader serves both, but two call sites can drift in
  what they feed it.
- **Six phases on one branch.** The rename is now a phase of its own, and
  the scan phase carries the install state and `Also=` as well as the
  links. The branch is longer-lived than it was, and two of its phases
  are at the upper end of large.
- **The instance gate knows three templates by name.** For any other
  template whose unit file does not name its `/etc` path, the enable is
  carried with no gate, and the image may enable a unit that cannot start.
  A table like this is only ever as complete as the last host that failed.
  The data advisory is weaker still: one template.
- **Excluded by default can be wrong for a host that wants the timer.** A
  `dnf-automatic-notifyonly.timer` is harmless on the target and is still
  left out until someone includes it.
- **A carried `graphical.target` default with no display manager** in the
  image produces a host that reaches the graphical target and shows no
  graphical login. `graphical.target` wants a display manager and does not
  require one, so the boot is not expected to hang; that was not measured,
  and run R5 records it. The decision accepts the result: the source said
  graphical, and the package list is where a missing display manager
  shows.
- **Removing the timer toggle removes a control operators can see today**,
  even though it does nothing. An operator who wants a local timer out now
  excludes its file in the config section. If the frontend pass cannot make
  that route obvious, the removal is a usability regression.
- **Much of the systemd behavior this relies on was not measured.** Offline
  `enable` for sockets and for mount units, `mask` on an absent unit, `--`
  before operands, the fragment a shadow's enable resolves, what
  `list-unit-files` lists for an instance or for a unit with only a
  hand-made link, which file an instance loads, whether `enable` reads
  `[Install]` from a drop-in, what `Also=` does with no manager running,
  what `mask` does when a file is at its path, whether `add-wants` runs at
  build time, and the target layout beyond one RHEL 9 base. Where the
  answer was unknown the design takes the side that omits a line. This
  subsystem has a record of sound reasoning from unverified premises,
  which is why § 15 puts the runs that need no build ahead of the phases
  that depend on them.

Evidence that would change the design:

- The survey (run R3) shows timers, targets and slices never differ from
  preset on RHEL 9 and 10 outside local units. Then sockets and paths first
  is the right first step, with timers fixed separately as a scheduled-tasks
  defect. If it also finds no instance links, no hand-made links and no
  `Also=` among enabled units, phases 4 and 5 shrink to the default
  target.
- Run R1 shows `cockpit.socket` already enabled in the built image by some
  other route (a preset in the base image). Then the driving loss is
  smaller than stated, and the omission half is the only real defect.
- Run R2 shows a local service with no timer builds today. Then the order
  move is not needed in phase 2, and the source reading in § 4 is wrong
  about something that should be understood before anything else is built
  on it.
- Run R5 (a) shows `list-unit-files` lists enabled instances by name. Then the
  directory read is still needed for hand-made links, and instances could
  come from source 1 with the directories as a cross-check.
- Run R5 (c) shows `add-wants` does not run at build time. Then the advisory
  offers the literal link with the unit's resolved path, and loses the
  build failure on a missing unit.
- Run R5 (d) shows `enable` or `mask` fails for a mount unit at build time.
  Then a package-owned mount unit needs its link written another way, or
  falls back to an advisory with the reason.
- Run R5 (b) shows the table paths are wrong, or that most templates name
  their configuration in the unit file. Then the table shrinks or goes.
- Run R5 (g) shows `enable` and `disable` do not follow `Also=` when no
  manager is running. Then the closure and rule 7 of § 10.2 go. If it
  shows `mask` follows `Also=`, rule 7 stays on `mask` for good.
- Run R5 (h) shows `enable` reads `[Install]` from drop-ins on both
  releases. Then the reader's second read is the answer and the
  `DropInChangesInstall` advisories go. If it shows a template is loaded
  ahead of a concrete instance file, step 1 of § 7.2 is reversed.
- Run R2 shows `systemctl mask` replaces a file at its path. Then rule 4
  goes and the mask wins. If it shows `disable` succeeds on an absent
  unit, rule 5 stops applying to `disable`.
- Run R5 (d) shows `/tmp` is writable on a booted image with `tmp.mount`
  masked, on both bases. Then the carried-mask advisory and its list go.
- Run R5 (j) shows the RHEL 10 base lays out `/opt`, `/home`, `/srv`,
  `/mnt` or `/root` differently. Then the mount-point lists are keyed by
  base and the advisory names the one in use.
- The frontend design pass finds that one section for all kinds is harder
  to use than units in place. Then the data contract in § 11.1 stands and
  § 11.2 does not.
