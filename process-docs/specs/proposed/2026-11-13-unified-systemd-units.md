# Unified systemd Unit Collection

## Metadata

| Field | Value |
|---|---|
| Date | 2026-11-13 |
| Status | Proposed. Scope and the rules marked with a name and date are decided (§ 2 and where each rule is written). The rest of §§ 5 to 12 is the spec author's design. No question is open (§ 16). |
| Scope | One model of systemd unit state for every unit type that exists as a static unit file, replacing the service-only collection. Collector, core types, snapshot schema, Containerfile render, the data contract for refine, and what the fleet merge does meanwhile. |
| Backlog items | Driving: `inspectah-socket-units-are-never-collected`. Folded in: `inspectah-local-timer-has-two-controls-and-neither-works`, `inspectah-timer-enable-lines-unquoted-and-trust-empty-units`, `inspectah-write-the-collector-overlap-contract` (the systemd half). Others in § 14. |
| Verified | Code citations read at `70aa8996` (2026-11-13); no source file has changed since. Line numbers are a convenience; the symbol name is the identifier. "Measured" means the host and build measurements recorded in `process-docs/skills/real-host-validation-runs.md` (§ 13). Claims about systemd behavior that were not measured are marked as such and listed in § 13. |

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
2026-11-13)** `tmp.mount` is the case the first half was written for.

**A mask on `tmp.mount` is refused, as a mask on a protected unit is
(§ 10.3). `systemctl enable` and `disable` stay the rendered verbs, and a
command for a unit with `Also=` is carried only when every member's state
on the source matches what it would produce (§ 10.2 rule 7). (Mark
Russell, 2026-11-13)**

**Enabled template instances are carried, each with a toggle like any other
unit. Links made by hand in a `.wants` or `.requires` directory are detected
and reported with the command that recreates them; they have no toggle and no
carried line. (Mark Russell, 2026-11-13)**

**Phases 1 to 5 land on one feature branch, which merges to `main` after
phase 5 and gives `main` one schema bump, 27 to 28. Phase 6, the refine
section, follows the merge as its own change (§ 15). (Mark Russell,
2026-11-13)**

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
  | `/etc/crypttab` | No dedicated collector. It reaches the snapshot only if the config walk records it (not measured on a stock host), and then `MERGE_HOSTILE_PATHS` locks it out (`normalize.rs:204`). | `systemd-cryptsetup@*.service` units and the encrypted devices behind them: host storage, like fstab, but with no report row. |
  | SysV init scripts in `/etc/init.d` | Partly. `modernization.rs:56-63` raises an advisory for a script with no same-named vendor service. The config walk can record an unowned script's content and can meet the `rc*.d` links. | The script's enablement: there is no model of the `rc*.d` runlevel links, and a link the walk records is carried as a flattened file. And its mode: a carried init script loses its exec bit for the reason in the next row, and the SysV generator is understood to skip a script that is not executable (not measured). |
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
  detected (§ 10.1). Where a rule needs "what the image would have", the
  host's presets stand in for the image's.
- **systemd's full search path.** Only `/etc/systemd/system` and
  `/usr/lib/systemd/system` are read. A unit the other directories would
  change is refused with a warning, not resolved (§ 7.1).
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
`is_real_service` gate at `:330`, and `readme.rs:89-98` counts them as
services.

**`generated` and `transient` are already dropped or warned**
(`services.rs:55`, `:57`). `static`, `alias` and `indirect` are dropped
silently. An enabled template instance shows only as its template reading
`indirect` (measured), so it never reaches a snapshot.

**Template units are skipped whole.** Any name with `@` is dropped at
`services.rs:303`, instances included, before the mask branch at `:336`,
so a masked template, which masks every instance of it, is dropped too.

**`enabled` is taken on systemd's word.** A state of `enabled` goes
straight to the preset comparison (`services.rs:308-324`). Nothing asks
which links made the unit enabled, so a unit that has only a hand-made
link in some `.wants` directory is carried as `systemctl enable`, which
creates the links its `[Install]` section names and not the one the host
had. A unit with no `[Install]` and a hand-made link reads `enabled` too
(measured), and its `enable` line exits 0 in the image and does nothing.
Nothing reads `Also=` either, so the units a carried `enable` or
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
snapshot is unchanged.

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
  missing unit exits 1 in a build (measured). The end-to-end case is run R2
  (§ 13).
- A full shadow is copied after its enable line, so the enable resolves the
  vendor fragment. That is the observation recorded in
  `inspectah-full-shadow-wants-link-points-at-the-vendor-path`. The link
  target is cosmetic: the `/etc` file is the one that loads (measured).

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
regular file takes the same route. systemd treats that file as a mask
(measured), and the export relies on it: today the `COPY` lands after
`systemctl mask` and replaces the link with the empty file.

**Two more places write `systemctl enable`.** The container workloads
section copies `flatpak-provision.service`, which the stager generates
(`configtree.rs:586`), and enables it (`containerfile.rs:1652`). The kernel
and boot section enables `tuned.service` whenever tuned activation is
included (`containerfile.rs:1885`). Neither goes through
`systemctl_lines`, and neither looks at the units section.

**The stager skips writes it cannot make.** `safe_write_file`
(`configtree.rs:158`) returns without writing when the destination is
already a directory or its parent cannot be created, and discards the
result of the write itself. The Containerfile is rendered as if the file
were there.

**A failed `rpm -Va` reads as a clean one.** A failure is `None`
(`crates/collect/src/inspectors/rpm/mod.rs:242-250`), stored as an empty
list (`:532`). The config walk then skips every package-owned path
(`crates/collect/src/inspectors/config/mod.rs:191-198`), so a package's
configuration file the administrator changed has no entry.

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
| `.mount`, `.automount`, `.swap`, package-owned under the vendor directory | Carry state, like a service. A mask on `tmp.mount` is refused (§ 10.3). | Decided (§ 2). The unit is the distribution's, the same in the image as on the host, and switching it is configuration. |
| `.mount`, `.automount`, `.swap`, written by the administrator | **Advisory only.** Collected, reported, never a `systemctl` line. The unit file stays a config entry. | Decided (§ 2). It is a statement about one host's storage, and on an image-based target several mount points are owned by bootc, are links into `/var`, or sit in the read-only image (§ 10.6). § 6.2 says which units count as the administrator's. |
| `.device`, `.scope` | Excluded. | They never exist as unit files. Administrator drop-ins for them (`*.device.d`, `*.scope.d`) are still collected as drop-ins (§ 7.7). |
| Template (`name@.service`) | A mask is carried. Nothing else is: a template has no enablement without an instance name. | A masked template is durable state with its own name, and it masks every instance (§ 7.4). |
| Enabled template instance (`wg-quick@wg0.service`) | Carry state, when the configuration the instance reads is in the export (§ 7.5, § 10.8). Instances tied to one host's devices are advisory. | Decided (§ 2). |
| Hand-made link in `.wants` or `.requires`, a hand-made alias link, and every `.upholds` link | **Advisory only**, with the `systemctl add-wants` or `add-requires` line where one exists (§ 7.3, § 10.9). | Decided (§ 2) for hand-made links. That no `.upholds` link is carried, enable-made or not, is this spec's design: the link starts and restarts its unit (§ 7.3). |
| Generated units | Excluded (§ 3). Administrator drop-ins on them are kept. | Decided. The generator's input is the fact, not its output. |
| Transient units, runtime and `linked` states | Excluded, with today's warning. | Decided, and they do not survive a reboot or need manual handling. |
| User units | Out of scope (§ 3). | Not part of this change. |
| Quadlets | Excluded. | Decided. |

On a stock and a drifted host of each release, no socket, path, slice or
storage unit differed from preset, one target did with no administrator
involved (§ 7.4), and the only enabled instance was `getty@tty1.service`
(measured; two clean installs and one drift tool are not a fleet, § 17).

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

/// The parts of an instance name, borrowed from the `UnitName`. The
/// fields are private: `template()` builds a proved name from them
/// without a re-parse.
pub struct InstanceName<'a> { prefix: &'a str, instance: &'a str, suffix: &'a str }

impl<'a> InstanceName<'a> {
    pub fn prefix(&self) -> &'a str { /* ... */ }
    pub fn instance(&self) -> &'a str { /* ... */ }
    /// `prefix@.suffix`, the template an instance falls back to.
    pub fn template(&self) -> UnitName { /* ... */ }
}
```

The kind is derived from the name and never stored beside it, so a snapshot
cannot hold a `cockpit.socket` labelled as a service. "Is an instance" and
"is a template" are read from the name too. The character set is the one
`systemd.unit(5)` gives, and a snapshot holding a name outside it fails to
load. What the type proves is what quoting needs: no quote, no whitespace,
no control character, a known suffix. A name that passes here and that
systemd still rejects fails at `RUN systemctl`, at build time and loudly.

### 6.2 An administrator's mount unit cannot reach a `systemctl` line

The rule in § 2 depends on the kind and on where the unit file came from,
so the type that guards a `systemctl` line holds both.

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

/// The unit file behind a name on the source host (§ 7.2). `file` is the
/// name of the file that loads: the unit's own name, or its template's
/// for an instance that has no file of its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "fragment", rename_all = "snake_case")]
pub enum UnitFragment {
    /// Under `/usr/lib/systemd/system` only. `owning_package` is `None`
    /// when no package owns the file or the RPM query failed.
    Vendor { file: UnitName, owning_package: Option<String> },
    /// Under `/etc/systemd/system` only: a file the administrator wrote.
    Local { file: UnitName },
    /// Both. The `/etc` file replaces the vendor one.
    Shadowed { file: UnitName, owning_package: Option<String> },
    /// No unit file at all. Only a masked name can have this.
    Absent,
}

/// The wire form of a unit and its fragment:
/// `{"name": "tmp.mount", "fragment": "vendor", "file": "tmp.mount",
///   "owning_package": "systemd"}`.
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
    Local,
    Shadowed { owning_package: Option<String> },
    /// Under the vendor directory, with no package known to own it.
    UnownedVendor,
    /// A mask on a name that has no unit file anywhere.
    MaskOnly,
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
collector and the snapshot loader apply one rule. The checked constructor
also refuses a `file` that is neither the unit's name nor its template's.

**A mask is a state, and the fragment is what the mask hides.** A mask
occupies the `/etc/systemd/system` path, so a masked unit's fragment is
`Vendor` when a vendor file of that name exists and `Absent` when none
does, and it classifies like any other unit. A masked
`dev-hugepages.mount` that `systemd` owns is `PackagedStorage` and its
mask is a state change. A zero-length `/etc/systemd/system/srv.mount` with
no vendor file is `MaskOnly` and becomes a mount finding (§ 6.5).

What the types prevent, for one observation:

- **A state change on an administrator's mount unit.** A `UnitStateChange`
  holds a `CarriedUnit`, `PackagedStorage` has no field in which a local
  path can sit, and there is no function from an `AdvisoryUnit` to a
  `UnitAction`. A hand-edited snapshot that moves a local mount into
  `state_changes` fails `try_from` and does not load.
- **A package-owned mount unit reported as an advisory.** The reverse
  `try_from` refuses it.
- **A `set-default` on something that is not a target.** `DefaultTarget`
  holds a `TargetUnit` (§ 7.6).
- **`.device` and `.scope` units.** Neither kind has a variant.

What the types do not prevent: an export in which one host's package-owned
mount unit's state change sits beside another host's administrator-written
file of the same name. Each observation is valid, and the enable would run
against the administrator's file. § 10.2 closes that.

**Two cases the § 2 rule does not name count as the administrator's, and
are advisory: a vendor mount, automount or swap unit with a full copy in
`/etc`, and one under the vendor directory with no known owner. (Mark
Russell, 2026-11-13)**

- **`Shadowed`: a vendor mount unit with a full copy in `/etc`.** The `/etc`
  file replaces the vendor one, so `What=`, `Where=` and `Options=` are what
  the administrator wrote, and an enable would activate that content, which
  is what the advisory rule exists to stop. The advisory carries the line
  to add by hand, and the file is a config entry (§ 7.7).
- **`UnownedVendor`.** The rule names a unit "owned by a package". A
  vendor file no package claims, or whose owner could not be read, does
  not meet it, so for storage kinds the owner lookup runs before
  classification.

A vendor mount unit with a drop-in stays `Vendor` and is carried. A
drop-in is collected and toggled on its own (§ 7.7).

### 6.3 State changes

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnitState { Enabled, Disabled, Masked }

/// What the links on the source host say about a unit that `enable` or
/// `disable` on another unit also acts on (§ 7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberLinks {
    /// Every link its `[Install]` names is present, under `/etc` or
    /// shipped, at least one of them under `/etc`, and it names at least
    /// one. `enable` would add nothing. Links made by hand may sit beside
    /// them.
    Installed,
    /// Its `[Install]` names at least one link and no link under
    /// `/etc/systemd/system` names the unit. `disable` would remove
    /// nothing.
    Unlinked,
    /// It has no link of its own to make (no `[Install]`, only `Also=`,
    /// or no unit file on the source host) and no link names it. Neither
    /// verb changes it.
    Inert,
    /// Anything else: some of its links, only links made by hand, a mask,
    /// or an `[Install]` that could not be read. Neither verb reproduces
    /// the host.
    Other,
}

/// One unit an `enable` or `disable` on the owning row also acts on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlsoMember {
    pub unit: UnitName,
    pub links: MemberLinks,
    /// File names of the vendor drop-ins that apply to the member
    /// (§ 7.2). Names only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vendor_drop_ins: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "UnitStateChangeWire")]
pub struct UnitStateChange {
    pub unit: CarriedUnit,
    pub current_state: UnitState,
    #[serde(deserialize_with = "require_explicit_null")]
    pub default_state: Option<PresetDefault>,
    /// Every unit `Also=` reaches from this one, the unit itself left
    /// out (§ 7.3). Empty for most units.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also: Vec<AlsoMember>,
    /// Drop-ins under `/etc/systemd/system` that apply to the unit and
    /// are install files (§ 10.2): they hold an `[Install]` section or
    /// carry a vendor drop-in's file name. Beside them, a drop-in in a
    /// directory systemd ranks above `/etc/systemd/system` that hides one
    /// of them (§ 7.2), since the image reads the hidden one whenever the
    /// hiding one is not there. Each is checked as a `Destination` when
    /// the plan reads it (§ 10.2 rule 5).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub install_drop_ins: Vec<String>,
    /// File names of the vendor drop-ins that apply to the unit (§ 7.2).
    /// Names only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vendor_drop_ins: Vec<String>,
    /// Paths under `/etc` a template instance cannot start without
    /// (§ 7.5).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_config: Vec<String>,
    /// Paths under `/var` that hold a template instance's persistent data
    /// (§ 7.5).
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
`shadow_type` and `shadow_rationale` pair. Today `owning_package: None`
means both "the administrator wrote this unit" and "the RPM query failed",
and Tier 1 emits either one. The two need different answers at render
(§ 10.2), so the type separates them.

The checked constructor behind `try_from` refuses two things:

- **A template name in any state but `Masked`.** Enabling or disabling a
  bare template means nothing without an instance.
- **An `Absent` fragment in any state but `Masked`.** A unit with no file
  can be masked and nothing else.

`also`, `install_drop_ins`, `required_config` and `data_paths` record what
the collector saw and decide nothing. The four `MemberLinks` values are the four answers to
"would `enable` add a link" and "would `disable` remove one", and
`required_config` holds every path the instance names, whether or not the
file exists. The decisions are made at render, against the export
(§ 10.2, § 10.8).

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
    /// Links inspectah reports and does not carry (§ 6.6).
    pub manual_links: Vec<ManualLink>,
    /// Units whose `[Install]` cannot be relied on, where the image's
    /// links can differ from the host's with nothing else to say so
    /// (§ 6.6, § 7.3).
    pub undetermined_installs: Vec<UndeterminedInstall>,
    /// The default target, when the host sets one (§ 7.6).
    pub default_target: Option<DefaultTarget>,
}
```

`inventory` replaces the three string lists `enabled_units`, `disabled_units`
and `preset_matched_units`. One list with a state per entry cannot place a
unit in two of them, and every consumer counts or filters by
`unit.kind()` and `state`. The `state` is the one § 7.3 derives, so a unit
that systemd calls `enabled` on the strength of a hand-made link is
`Disabled` here, and a unit with no install state has no entry.

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

The finding has no `disposition`: there is nothing to include or exclude.
It records what the host had. Whether a mount point collides with the
target layout is a property of the target, so the hazards in § 10.6 are
computed at render from `mount_point`, not stored.

The finding does not hold `What=`. A mount source can carry a credential
(`What=https://files.example/volume?token=...`), and a copied field would
be a second copy the config pass never visits (§ 6.7). Refine shows the
source by linking the row to the unit file's config entry.

A finding is recorded for every `.mount`, `.automount` or `.swap` unit that
classifies as `Advisory`:

- every regular, non-empty file of those kinds under `/etc/systemd/system`;
- an unowned one under the vendor directory, when it is enabled or masked;
- a mask on a name with no unit file (`MaskOnly`), with `state_change`
  holding `Masked` and no `mount_point`.

A unit whose effective `[Install]` is `Undetermined` (§ 7.2), or that has
an `UndeterminedInstall` record from the comparison of § 7.4, gets no
finding. Its state is not known, so nothing says it is not enabled, and
its record's advisory (§ 10.9) is what the export says about it. It loses
the whole mount advisory of § 10.6, not only the fact line.

A package-owned vendor unit of these kinds never produces a finding. When
its state differs from preset, or it is masked, it is a `UnitStateChange`
like a service.

### 6.6 Hand-made links

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkRelation {
    Wants, Requires, Upholds, Alias,
    /// Not a link: `unit` is named in `parent`'s `Also=` (§ 7.4).
    Also,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualLinkCause {
    /// The linked unit has no link of its own to make: no `[Install]`
    /// section, or one that names no link (only `Also=`, or lists a
    /// drop-in reset to empty, § 7.2).
    StaticUnit,
    /// The unit's `[Install]` section does not name this parent for this
    /// relation.
    NotInInstall,
    /// The section names this link and others that are absent, so neither
    /// `enable` nor `disable` reproduces the host.
    PartialInstall,
    /// The `[Install]` section cannot be relied on (§ 7.2), one that
    /// holds `UpheldBy=` included. The reason is kept so the advisory can
    /// name it (§ 10.9).
    Undetermined { reason: InstallUnknown },
    /// An instance of a template tied to one host's devices (§ 7.5).
    HostDevice,
    /// The unit's `/etc` files make this link, and the package's
    /// `[Install]` alone does not: the image lacks it (§ 7.4).
    HostInstallOnly,
    /// The package's `[Install]` alone makes this link, and the unit's
    /// `/etc` files do not: the image has it and the host did not (§ 7.4).
    VendorInstallOnly,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManualLink {
    pub relation: LinkRelation,
    /// The unit whose `.wants`, `.requires` or `.upholds` directory holds
    /// the link. For an alias, the alias name.
    pub parent: UnitName,
    pub unit: UnitName,
    pub cause: ManualLinkCause,
    /// For the two install-difference causes: the `/etc` install files
    /// that produce the difference (§ 7.4). They are the files of the
    /// unit whose `[Install]` was compared: `unit` for a link, `parent`
    /// for relation `Also`. Empty otherwise.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub install_files: Vec<String>,
    pub aggregate: Option<AggregatePrevalence>,
}

/// A unit whose `[Install]` on the source host is `Undetermined` and
/// whose `/etc` install files change it, or that assigns `Also=`, so its
/// members cannot be listed or checked (§ 7.3); or a unit whose package's
/// `[Install]` is `Undetermined` and whose `/etc` install files change it
/// (§ 7.4). One per unit, whether or not the unit has a link, since the
/// image's links can differ from the host's with no link on either side
/// to report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UndeterminedInstall {
    pub unit: UnitName,
    pub reason: InstallUnknown,
    /// The unit's `/etc` install files: its unit file or shadow, and its
    /// install drop-ins (§ 10.2). Empty when it has none.
    pub install_files: Vec<String>,
    pub aggregate: Option<AggregatePrevalence>,
}
```

A `ManualLink` holds two `UnitName`s and no `CarriedUnit`, nothing turns
one into a `UnitAction`, and it has no `disposition`. The last two causes
are the one case where it describes something other than a link on the
host: a difference between the links the host's files make and the links
the package's files make. The decision that
these links get no toggle and no carried line (§ 2) is a property of the
type.

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
| `required_config`, `data_paths` | Paths in the template's unit file, expanded | Stored only when the expanded value is an absolute path under `/etc` or `/var`. Classified as identifiers. |
| `ManualLink.install_files`, `UndeterminedInstall.install_files` | A directory listing | Paths under `/etc/systemd/system` of the files of the unit whose `[Install]` was compared (`unit` for a link, `parent` for relation `Also`). Not validated on load: each is read through `Destination` when the plan uses it, and one that does not parse counts as not staged, so the advisory gives way to its omission line (§ 10.9). Classified as identifiers. |
| `install_drop_ins` | A directory listing | Paths of `.conf` files under `/etc/systemd/system`, and of a hiding drop-in in a directory ranked above it (§ 7.2), read through `Destination` before use. Classified as identifiers. |
| `vendor_drop_ins`, on a state change and on an `AlsoMember` | A directory listing under `/usr/lib/systemd/system` | File names only, each one path component ending in `.conf`; a value that is not fails to load. No content is read into the snapshot. Classified as identifiers. |
| `AlsoMember.unit`, `UnitFragment::file`, `ManualLink.parent`, `ManualLink.unit`, `UndeterminedInstall.unit`, `DefaultTarget.target` | `[Install]` values, link names and a directory listing | Proved `UnitName`s. |
| `SystemdTimer.exec_start`, `description` | The timer and service files | Scrubbed by the existing scheduled pass, unchanged. |
| `SystemdTimer.trigger` | The timer's `Unit=`, or its own name | A proved `UnitName`, or the marker `unresolved` when the value cannot be made into one. The raw value is not stored. A snapshot that lacks the field loads it as `unresolved`, so a snapshot written before the field omits its timers' `enable` lines until it is rescanned (§ 8). |
| `SystemdTimer.trigger_drop_ins` | A directory listing | Paths of `.conf` files under `/etc/systemd/system`, read through `Destination` before use. Classified as identifiers. |
| `SystemdDropIn.content` | The drop-in or shadow file | Scrubbed by the existing drop-in pass, renamed with the section. |
| `attention_reason`, advisory strings | Generated | Built from the fields above and fixed text only. Never from file content. |

The unvisited-field guard in `crates/pipeline/src/render/mod.rs`
(`every_field_of_a_content_bearing_type_is_classified`) gains
`MountUnitFinding`, `ManualLink`, `UndeterminedInstall`, `UnitStateChange`,
`AlsoMember`, `UnitInventoryEntry` and `DefaultTarget`, each destructured
with no `..`.
Its runtime half gains a sentinel case: a local mount unit whose `What=`
holds a token the generic pattern matches, absent from the whole
serialized snapshot and from the refine view.

## 7. The collector

`UnitsInspector` replaces `ServicesInspector`. It is the one owner of three
facts for every kind: whether a unit is enabled, disabled or masked against
what the image would have; where its file lives; and which package owns it.

### 7.1 Sources

1. `systemctl list-unit-files --no-pager`, with no `--type`. The state column
   goes through `parse_unit_state` unchanged. It supplies the list of names
   and the `masked`, `generated` and runtime states. Its words `enabled`,
   `disabled` and `indirect` decide no state: § 7.3 derives enablement
   from the links, and a unit whose `[Install]` files cannot be read as
   text is undetermined, not taken on the word (§ 7.2). The one place the
   word is recorded is the degraded scan in which no preset rule could be
   read: that scan reads no links, makes no state change, and records
   each unit's listed word in `inventory` with no preset.
2. The host's preset files, read as today, with the instance list of an
   `enable` line kept (§ 7.4). A line that starts with `#` or `;` is a
   comment (`;` read from systemd's source, not measured).
3. A direct read of `/etc/systemd/system`: regular unit files, zero-length
   unit files and symlinks to `/dev/null` (masks), other symlinks directly
   under it (aliases, and `default.target`), and the `*.wants/`,
   `*.requires/` and `*.upholds/` directories.
4. The same three dependency directories and alias links under
   `/usr/lib/systemd/system`. These are the links packages ship. They are
   read only to decide whether an install is complete (§ 7.3) and never
   become a row.
5. The effective unit for each name the other sources produce (§ 7.2).
6. The instances a package install would link, derived from the presets
   and from the `Also=` of enabled units, from sources 1, 2 and 5, and
   checked against source 3, so a disabled template-backed instance is
   found (§ 7.5). Nothing more is read from the host for them.

Source 1 cannot answer what the links answer (all measured). An enabled
instance has no row and shows only as its template reading `indirect`. A
link made by hand makes a unit read `enabled`, a unit with no `[Install]`
included. `indirect` also covers a unit whose only link is an undeclared
alias, and the units whose `[Install]` holds only `Also=`, which every
stock host has (`sssd-kcm.service` and its siblings).

**The search path.** systemd looks for a unit in more directories than the
two this collector reads, and a file in one of them can outrank the file
the collector would pick. The collector does not implement the search
path. It refuses the observation where the search path would change it:

- The other directories are one named constant, in the order
  `systemd.unit(5)` gives them: above `/etc/systemd/system`,
  `/etc/systemd/system.control`, `/run/systemd/system.control`,
  `/run/systemd/transient` and `/run/systemd/generator.early`; between it
  and `/usr/lib/systemd/system`, `/etc/systemd/system.attached`,
  `/run/systemd/system`, `/run/systemd/system.attached`,
  `/run/systemd/generator` and `/usr/local/lib/systemd/system`.
  `/run/systemd/generator.late` comes after `/usr/lib/systemd/system`,
  outranks nothing the collector reads, and is not in the constant.
- A unit file on the list that systemd loads in place of the fragment the
  collector chose means systemd loads something the collector did not
  read. That is a file for the name in a directory that outranks the
  chosen fragment; when the collector chose a template, a template file in
  a directory that outranks it; and, when the collector chose the template
  for an instance, the instance's own file in any directory on the list,
  since a concrete instance file loads over its template wherever each one
  sits. A template file beside a concrete instance file the collector
  chose changes nothing.
- A drop-in in the name's own `.d` or, for an instance, in its template's
  `.d`, in any directory on the list, that holds an `[Install]` section or
  has the file name of a vendor drop-in, changes what `enable` does. These
  are the only drop-in directories `enable` reads `[Install]` from (§ 7.2
  step 4). That was measured under `/etc` and `/usr/lib`; the test applies
  the same rule to the directories on the list. A drop-in in one of the
  four directories above `/etc/systemd/system` that does neither, such as
  the `[Service]` file `systemctl set-property` writes, does not refuse
  the unit; it hides a lower drop-in of its file name (§ 7.2, load order).

Either finding makes the unit `Unsupported` (§ 7.2): a structured warning
that names the unit and the path, and no state for it. Any other drop-in
elsewhere is ignored, as it is today. The order is measured on both
releases (§ 13 R5): `systemd-analyze unit-paths` prints the constant's
directories, `/etc/systemd/system` and `/usr/lib/systemd/system` in
exactly this order, then `generator.late`, and unit files planted in pairs
loaded as it says. A concrete instance under `/usr/lib` loaded over its
template under `/etc`, `/run/systemd/system` or
`/usr/local/lib/systemd/system`.

### 7.2 The effective unit

Every question below about a unit (where its file is, which package owns
it, what `enable` would do, what an instance reads) is asked of one thing:
the file systemd would load for that name, with the drop-ins that apply to
it. The collector resolves that once per name, with one function, and every
later step takes its result.

```rust
/// What systemd has for a name on the source host.
pub enum Resolved {
    /// A unit file loads.
    File { fragment: UnitFragment, install: InstallRead },
    /// A link to `/dev/null`, or a zero-length regular file, is at the
    /// `/etc/systemd/system` path. `hidden` is the unit file under the
    /// mask: `Vendor` or `Absent`.
    Masked { hidden: UnitFragment },
    /// A link under `/etc/systemd/system` to another unit's file.
    AliasOf(UnitName),
    /// No file in the two directories this collector reads.
    NotFound,
    /// The search path changes the answer (§ 7.1).
    Unsupported { path: String },
}

pub struct InstallSection {
    pub wanted_by: Vec<UnitName>,
    pub required_by: Vec<UnitName>,
    pub alias: Vec<UnitName>,
    pub also: Vec<UnitName>,
    pub default_instance: Option<String>,
}

pub enum InstallRead {
    /// The unit has no `[Install]` section.
    Absent,
    Known(InstallSection),
    /// The section cannot be relied on. The cause is kept for the warning
    /// and travels with every finding the section produces (§ 6.6).
    /// `assigns_also` says whether any line assigns `Also=`, an empty
    /// assignment included, which decides whether a `disable` may be
    /// carried for it (§ 7.3). For `Unreadable` it is taken as true.
    /// Written `Undetermined(<reason>)` in this spec.
    Undetermined { reason: InstallUnknown, assigns_also: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallUnknown {
    /// A value holds a specifier other than `%i`, `%p`, `%N` or `%%`.
    /// `systemctl enable` does not expand `%I`.
    Specifier,
    /// An empty `RequiredBy=` follows a `RequiredBy=` value in load
    /// order. Whether it clears the value was not measured.
    RequiredByReset,
    /// A value is not a unit name this model admits.
    ForeignName,
    /// The section assigns `UpheldBy=`, an empty assignment included
    /// (§ 7.3).
    UpheldBy,
    /// The unit file, or a drop-in it reads `[Install]` from, could not
    /// be read as text, so whether the section assigns `Also=` is not
    /// known either.
    Unreadable,
}
```

Resolution, for a name `N`:

1. **The search-path test** of § 7.1. A hit ends the resolution.
2. **The fragment.** `/etc/systemd/system/N`, then
   `/usr/lib/systemd/system/N`. When `N` is an instance and neither exists,
   the template `prefix@.suffix`, in the same order. A concrete
   `worker@site.service` under `/etc` wins over a vendor
   `worker@.service` (measured: `FragmentPath` and `ExecStart` come from
   the concrete file, and `enable` links the instance to it). So does the
   reverse placement: a concrete instance under `/usr/lib` loads over its
   template under `/etc`, and `enable` links the instance to the
   `/usr/lib` file (measured, § 13 R5). A concrete instance file loads
   over its template wherever each one sits, which is why § 7.1 refuses an
   instance file on the other directories whatever their rank.
3. **Its form.** A symlink is tested before anything else: to `/dev/null`
   it is a mask, to another unit's file it is an alias, anywhere else the
   unit is `linked`, which § 3 excludes. A regular file of exactly zero
   length is a mask. A file that holds only a comment or only blank lines
   is **not**: it loads as a broken unit that replaces the vendor one (all
   measured). So the test is size zero and nothing looser. For a mask, the
   vendor directory is then read for the file the mask hides.
4. **Its `[Install]` section**, read once from the fragment and the
   drop-ins that apply, with `%i`, `%p`, `%N` and `%%` expanded. **`%I`
   in `[Install]` is declined: a value holding it is
   `Undetermined(Specifier)`. (Mark Russell, 2026-11-15)** systemd's
   install-time expansion has no `%I` (read from the source of systemd
   252 and 257, not measured), so `enable` does not expand it as the
   loaded unit would. The scheduled inspector's `Unit=` expansion keeps
   `%I` (§ 8). For `[Install]`, the drop-ins that apply to `N` are the
   `.conf` files in `N.d` and, for an instance, in its template's `.d`,
   under both directories, and no others. A file whose name starts with
   `.` is not a drop-in and is skipped, here and wherever this spec names
   an install file (read from systemd's source, not measured on either
   release). `systemctl enable` honors `[Install]` keys
   from those drop-ins, under `/etc` and under `/usr/lib` (measured, for an
   added `WantedBy=`). It ignores `[Install]` in the type-wide directory
   (`service.d`) and in the dash-prefix directories (`a-.service.d` for
   `a-b.service`), with or without an `/etc` file there: a vendor drop-in
   holding `WantedBy=` in either made no link, while the same file in the
   unit's own or the template's `.d` did (measured, § 13 R5). Those
   directories still apply to the unit for every other section, and
   systemd loads them; they are not read for `[Install]`. Their files are
   carried as configuration where § 7.7 carries them, and are never
   install files (§ 10.2).

   **Load order.** The main unit file first. Then the selected drop-ins,
   one per file name, in file-name order across every directory that
   applies, not one root at a time. **The drop-in read for a file name
   follows systemd's source: every directory of the instance's own name
   is searched before every directory of its template's, with root
   priority within each, and the first file of that name wins. (Mark
   Russell, 2026-11-15)** The roots, in priority order, are the four
   directories systemd ranks above `/etc/systemd/system`
   (`/etc/systemd/system.control`, `/run/systemd/system.control`,
   `/run/systemd/transient`, `/run/systemd/generator.early`, in that
   order), then `/etc/systemd/system`, then `/usr/lib/systemd/system`.
   **A drop-in in one of the four directories above `/etc` hides a
   same-named drop-in lower in that ranking. (Mark Russell, 2026-11-15)**
   So there is one order: directory first (the instance's `.d` before the
   template's), then root. Within one directory name, a file under `/etc`
   replaces a file of the same name under `/usr/lib`; that was measured
   within the unit's own `.d` and within the template's `.d` (§ 13 R5).
   Across the two directories, a vendor file in the instance's `.d` hides
   an `/etc` file of its name in the template's `.d`, and an `/etc` file
   in the instance's `.d` hides a vendor file of its name in the
   template's. Both placements, and the hiding from above `/etc`, are read
   from the source of systemd 252 and 257 (the search order in
   `install.c`, the first file of a name kept in `conf-files.c`) and not
   measured; all three are open items of § 13 R5. A drop-in above `/etc`
   that holds `[Install]` or has a vendor drop-in's file name has already
   refused the unit (§ 7.1), so one read here only hides; the file it
   hides is not read, though it stays an install file (§ 10.2), since the
   image reads it whenever the hiding file is not there.

**The reader applies the reset systemd performs: an empty assignment of
`WantedBy=` or `Alias=` clears every value of that key assigned before it
in load order, and `Also=` never clears. (Mark Russell, 2026-11-15)** A
drop-in that resets a key and then assigns it is therefore read like any
other drop-in: its section is `Known`, with the values that follow the
reset. Measured on both releases (§ 13 R5), with
`WantedBy=multi-user.target` in a unit under `/etc` or `/usr/lib` and an
`/etc` drop-in: `WantedBy=` alone makes `enable` exit 0 and link nothing,
and the unit reads `static`; `WantedBy=` followed by
`WantedBy=graphical.target` links `graphical.target.wants` only; an empty
`Alias=` drops the alias link; an empty `Also=` leaves the member enabled.
A section whose lists were all reset names no link, and § 7.3 treats it
like a section that holds only `Also=`.

Two list keys were not measured. `RequiredBy=` was not tried. systemd's
source parses it with the same list parser as `WantedBy=`, so its reset
most likely works the same way (read from source, not measured). The
spec still declines it: an empty `RequiredBy=` that follows a
`RequiredBy=` value in load order makes the section
`Undetermined(RequiredByReset)`, because a wrong inference changes what
the image starts with nothing to say so, where declining costs an
advisory, which § 10.9 renders whether or not the unit has a link. An
empty `RequiredBy=` with no value before it clears nothing under either
reading and is ignored. `UpheldBy=` needs no rule of its own: any
assignment of it, an empty one included, makes the section
`Undetermined(UpheldBy)` (§ 7.3). Both resets are open items of § 13 run
R5.

A reason, once found, stands: an empty assignment that later clears the
value that caused it does not make the section `Known` again. **An empty
`Also=` counts as assigning `Also=`, so an undetermined section that holds
one goes to the advisory rather than to a `disable` row (§ 7.3). (Mark
Russell, 2026-11-15)**

**A unit file or drop-in that cannot be read as text makes the section
undetermined from the links, with `assigns_also` taken as true and a
warning naming the file. systemctl's listing word decides nothing. (Mark
Russell, 2026-11-15)** That covers the unit file and every drop-in that
applies for `[Install]` (step 4), in any root, over the size cap or not
UTF-8: the section is `Undetermined(Unreadable)`. § 7.3's table then
gives no state, each `/etc` link is a `ManualLink` with cause
`Undetermined(Unreadable)`, and the unit always gets an
`UndeterminedInstall` record, listing its `/etc` install files. The
structured warning reads:

```
unit <unit> has state '<word>' - <path>, <path> cannot be read as text, so its [Install] section is undetermined and its state is read from its links alone
```

with `unit`, `raw_state` and `paths` among its fields. A mask over such a
file is still a mask: a mask reads no `[Install]`.

The reader is a pure function over text. Only the collector calls it. The
render-time plan reads no `[Install]` key; it tests only whether a file
holds an `[Install]` section (§ 10.2).

A carried `enable` follows the `[Install]` the image has, and with no
`[Install]` it exits 0 and links nothing (measured). So the reader, which
already reads each drop-in, records on the state change the path of every
drop-in under `/etc/systemd/system` that applies to the unit and is an
install file by the definition in § 10.2 (`install_drop_ins`, § 6.3).
With the fragment, that is the row's install evidence, and rule 5 of
§ 10.2 carries an `enable` or `disable` only when the export holds exactly
that. A vendor drop-in arrives with its package and its content is not
recorded. Its file name is: the collector lists the `.conf` names in the
vendor drop-in directories that apply to the unit for `[Install]` (step
4), and to each `Also=` member, as `vendor_drop_ins`. An `/etc` drop-in of
the same name replaces the vendor one whatever either holds: with no
`[Install]` of its own it hides the vendor one's, and `enable` makes no
link (measured, § 13 R5). An `/etc` file of that name in the type-wide
directory hides nothing there. So the plan needs the names to tell which
`/etc` drop-ins can change `[Install]`.

**The vendor-side read.** For a unit with a `Vendor` or `Shadowed`
fragment that has an install file under `/etc` (a shadow, or a non-empty
`install_drop_ins`), the reader runs a second time over what the package
alone provides: the vendor fragment and the vendor drop-ins, with nothing
from `/etc`. The result is the `[Install]` the image's package install
acts on, before any `COPY`. It is not stored. § 7.4 compares it with the
effective section. A vendor drop-in that cannot be read as text makes the
effective section `Unreadable` too, so § 7.4's test does not apply. The
one file only this read opens is the package's unit file under a shadow:
when that cannot be read as text, there is no vendor-side read, no
comparison is made, nothing is said, and the unit is judged by its
effective section alone.

### 7.3 Enablement is read from the links

`list-unit-files` reports `enabled` when it finds a link to the unit. It
does not say who made the link. A unit with `WantedBy=multi-user.target`
and one hand-made `rescue.target.wants/` link reads `enabled` (measured),
and `systemctl enable` in the image would create the `multi-user.target`
link the host never had. A unit with no `[Install]` and a hand-made link
reads `enabled` too, and `enable` on it in the image exits 0 and does
nothing. So `enabled` is not admitted on that word.

For each unit `U` that source 1 lists, the states § 3 excludes aside, or
that a link under `/etc/systemd/system` names, the collector compares
three sets. A unit listed `static` or `indirect` is read only when it has
a link, a unit file or a drop-in directory under `/etc/systemd/system`;
reading every listed one would turn units a stock RHEL 9 host links
through `UpheldBy=` into `disable` rows.

- **expected:** the links `systemctl enable U` would make, from the
  effective `[Install]` section: one per `WantedBy=` and `RequiredBy=`
  value, and one alias link per `Alias=` value.
- **admin:** the links to `U` under `/etc/systemd/system` (source 3).
- **vendor:** the links to `U` a package ships under
  `/usr/lib/systemd/system` (source 4).

| `[Install]` | The links | Install state of `U` | Each admin link |
|---|---|---|---|
| `Known`, expected not empty | Every expected link is in admin or vendor, and at least one is in admin | `Enabled` | One outside expected is a `ManualLink`, cause `NotInInstall`. The rest are enable-made and produce nothing of their own. |
| `Known`, expected not empty | No expected link is in admin | `Disabled` | A `ManualLink`, cause `NotInInstall`. |
| `Known`, expected not empty | Some expected links are in admin, and one is in neither admin nor vendor | None. No state change is recorded. | One the section names: a `ManualLink`, cause `PartialInstall`. **Any other `/etc` link of the unit, an alias included, is a `ManualLink` with cause `NotInInstall`; only the links the section names keep `PartialInstall`. (Mark Russell, 2026-11-15)** |
| `Known` with nothing expected (only `Also=`, or lists a drop-in reset to empty), or `Absent` | (nothing is expected) | None. The unit has no enablement of its own. § 7.4 still compares a vendor unit's section with the package's. | A `ManualLink`, cause `StaticUnit`, except a link § 7.4 finds the package install makes in the image too, which produces nothing. |
| `Undetermined`, not assigning `Also=` | No link to `U` under `/etc/systemd/system`, and no install file under `/etc` | `Disabled`. `disable` removes every link to `U` under `/etc` and does not need the expected set, so an absent set of links is `Disabled` whatever the section would expect. | (none) |
| `Undetermined`, not assigning `Also=` | No link to `U` under `/etc/systemd/system`, and an install file under `/etc` (§ 10.2) | None, and no inventory entry. One `UndeterminedInstall` record (§ 6.6). A `disable` would reproduce a host with no link, but the image's package install links the unit by the package's section before the `/etc` files are copied, and the record is what says so. | (none) |
| `Undetermined`, assigning `Also=` (`Unreadable` included) | No link to `U` under `/etc/systemd/system` | None. A `disable` would also act on members the collector could not list. One `UndeterminedInstall` record (§ 6.6), with or without an `/etc` install file. | (none) |
| `Undetermined` | Some link to `U` under `/etc/systemd/system` | None. No state change is recorded. When the unit has an install file under `/etc` (§ 10.2), or assigns `Also=`, one `UndeterminedInstall` record (§ 6.6). | A `ManualLink`, cause `Undetermined` with its reason. |

The install state, not systemd's word, is what § 7.4 compares with the
preset and what `inventory` records. Three points about the table:

- **Vendor links complete an install and never start one.** `disable`
  removes only links under `/etc`, and a unit a package also links from
  `/usr/lib` then reads `disabled` (measured,
  `remote-cryptsetup.target`). A link a package ships under `/usr/lib` is
  never a `ManualLink`.
- **A unit whose `[Install]` holds only `Also=` has no state.** Enabling it
  enables its members, and each member has its own state. So
  `sssd-kcm.service` produces no row on a stock host, and an
  administrator's `enable` on it is carried through `sssd-kcm.socket`.
- **Nothing carries an `.upholds` link.** `UpheldBy=` makes links on
  systemd 257 (measured on RHEL 10); systemd 252 on RHEL 9 does not accept
  it. An upheld unit is started as soon as the link exists and restarted
  whenever it goes inactive, so the link cannot be separated from "start":
  a `Type=oneshot` unit without `RemainAfterExit=yes` loops until it hits
  its start limit and leaves the host degraded on every boot (measured on
  RHEL 10). An `[Install]` section that holds `UpheldBy=` is therefore
  `Undetermined(UpheldBy)`: the unit never gets an `enable`, and each of
  its links, `.upholds/` ones included, is advised (§ 10.9). With no link
  at all it is `Disabled` by the table, so a preset-enabled vendor unit
  with `UpheldBy=` that the administrator disabled gets a `disable` row,
  and the image, whose package install linked it, ends without the links.
  The `.upholds/` directories are read so that those links are seen.

Every row that does not end in `Enabled` or `Disabled` falls on the
advisory side on purpose: an advisory costs the operator one line, and a
carried enable for links the host lacked changes what the image starts.

**`Disabled` means no installation link under `/etc`, nothing more.** It
says the administrator's links are absent, which is what `disable`
reproduces. A carried `disable` does not stop the unit from starting
through a link a package ships under `/usr/lib`, or as a dependency of
another unit, on the host or in the image; neither is changed by the
export.

Only `/etc` produces rows. Links a generator makes live under `/run`, which
is why `serial-getty@ttyS0.service` on a host with a serial console is not
seen (§ 3). A link whose parent or own name does not parse as a `UnitName`
(a `.device` parent, for one) produces a structured warning and nothing
else. A hand-made alias, a link directly under `/etc/systemd/system` to a
unit whose `Alias=` does not name it, is a `ManualLink` with relation
`Alias` and the alias name as `parent`. `default.target` is not an alias;
§ 7.6 owns it.

An alias belongs to the unit its whole chain ends at. The collector
follows the link through every hop: a link to `/dev/null` or to a
zero-length file is a mask; a chain that ends at a file with content
directly in one of the two unit directories is an alias of that unit,
keyed by it, so a relay link in a chain is its own alias of the end unit
and is reported as `NotInInstall` when `[Install]` does not name it. A
link to a file outside both unit directories, a dangling chain, one that
runs through a file where a directory should be, and a loop end
elsewhere, and are no alias. A chain that cannot be followed because a
link on the way cannot be read (permission, I/O) is a failed read under
`/etc`: the scan degrades with `unit file read failures:`, and no unit
that is not masked gets an install state from the incomplete links.

**`Also=` members.** `enable` and `disable` also act on every unit
`Also=` reaches (§ 10.2 rule 7). For a unit that gets a state change, the
collector follows `Also=` through the effective unit of each name until no
new name appears and records every name reached, the unit itself left out,
as an `AlsoMember` with its `MemberLinks`, in the order the walk reaches
them: breadth first, each `Also=` list in the order it names its units.
§ 6.3 defines the four values;
"every link its `[Install]` names" is judged as in the table above, with
vendor links completing an install. `Unlinked` and `Inert` count every
link, not only expected ones, because `disable` removes every link to a
member under `/etc`, hand-made links and links in other targets' `.wants`
included (measured). A member with no `[Install]` and one hand-made link
is therefore `Other`, as is a member that is masked, `Unsupported`, an
alias, or whose section is `Undetermined`, and every member of a scan
whose read under `/etc` failed.

A template named in `Also=` stands for its `DefaultInstance=`; with none,
the member is recorded under the template name as `Other`. The walk does
not continue through a member whose section could not be read. § 10.2
rule 7 is the only reader of the list.

### 7.4 What makes a state change

For each name with an install state (§ 7.3) or a mask, whose name and
fragment classify as a `CarriedUnit` (§ 6.2):

- **Masked** is always a state change, as today, whichever form the mask
  takes (§ 7.2, § 7.7).
- **A masked template** is a `Masked` state change on the template name.
  The mask test runs before the template test.
- **`Vendor` or `Shadowed` fragment, and every `PackagedStorage` unit:** a
  state change when the install state differs from the preset, as today.
  With no matching preset rule there is no evidence of a change and none is
  recorded, as today.
- **`Local` fragment:** the image does not have this unit unless the export
  carries it, so the comparison with a preset is the wrong one. An enabled
  local unit is a state change whatever the presets say. A disabled local
  unit is not: a unit file copied into the image is not enabled until
  something enables it.

**A unit at its preset whose `/etc` files change `[Install]`.** The
package install in the image applies the preset and links the unit by the
vendor `[Install]` (measured for `cups` and `libvirt-daemon`). The export
then copies the `/etc` files. A link made before a `COPY` stays as it is,
and the `/etc` file is the one that loads (both measured). So for a unit
that is `Enabled` under an `enable` preset, which the rule above gives no
row, the image keeps the vendor's links, and the host's are lost with
nothing to say so. This covers a full shadow, an `/etc` drop-in that
supplies or changes `[Install]` (a vendor unit with no `[Install]` of its
own, made installable by a drop-in, included), and an `/etc` drop-in that
hides a vendor `[Install]` drop-in by file name.

**Where the host's `/etc` files only add links to what the vendor
`[Install]` makes, the unit is an ordinary `enable` state change. Where
they drop or replace a link, or change `Also=`, each differing link gets
an advisory with the line to add or remove by hand, and no command. A
shadow or drop-in that leaves `[Install]` alone changes nothing. (Mark
Russell, 2026-11-14)**

The test applies to a unit with a `Vendor` or `Shadowed` fragment, a
preset of `enable`, an install file under `/etc`, and either install state
`Enabled` or no install state because its effective `[Install]` names no
link (§ 7.3): a drop-in that resets `WantedBy=` with nothing added, or a
shadow with no `[Install]` section. It compares two pairs of sets, read by
the same reader (§ 7.2), so a drop-in that resets and then adds compares
exactly like any other:

- **links:** the links `enable` would make from the effective `[Install]`
  and from the vendor-side one. A link is a relation and a parent, for
  `WantedBy=` and `RequiredBy=`, or an alias name, for `Alias=`. Order and
  spelling of the section do not matter.
- **`Also=` names:** the names each section assigns directly, a template
  standing for its `DefaultInstance=` as in § 7.3.

| The sets | Result |
|---|---|
| Both pairs equal | Nothing. No row and no advisory. |
| Every vendor link is an effective link, at least one effective link is not a vendor link, and the `Also=` names are equal | **Additive.** A `UnitStateChange` of `Enabled`, whatever the preset says, with the fragment as resolved (`Shadowed`, or `Vendor` with its `install_drop_ins`) and the `attention_reason` `Its /etc files add links the package's unit does not make.` |
| Anything else | **Not additive.** No state change. One `ManualLink` per difference: cause `HostInstallOnly` for an effective link that is not a vendor link, `VendorInstallOnly` for a vendor link that is not an effective link and is not on the host, and relation `Also` with the same two causes for a name only one side assigns. |
| The vendor-side section is `Undetermined` | **No state change and no inventory entry. The unit gets the `UndeterminedInstall` record, with the package section's reason and its `/etc` install files, and the unit-level block of § 10.9, with or without a link, and keeps its per-link findings: each `/etc` link is a `ManualLink` with cause `Undetermined` and that reason. (Mark Russell, 2026-11-15)** |

The vendor-side record reaches only as far as this comparison runs: a
`Vendor` or `Shadowed` fragment, an `/etc` install file, an `enable`
preset, and `Enabled` or no install state as above. A `Disabled` unit at
its `enable` preset whose package section is `Undetermined` still gets
its ordinary `disable` row and no record (§ 17). The record's block uses
the effective side's words (§ 10.9), since the snapshot has no field that
says which side's section is undetermined; for the vendor side the
section that cannot be relied on is the package's, which is also on the
source host and is what the `/etc` files change. The `Also=` names are
compared with a template standing for its `DefaultInstance=` on both
sides.

An additive row is planned like any other (§ 10.2). The image already has
the vendor links, and `enable` after the `COPY` lines adds the ones the
`/etc` files name. That sequence was measured on both bases (§ 13 R5):
after the package install linked a preset-enabled unit, a `COPY` of an
`/etc` drop-in adding `WantedBy=extra.target`, and separately of a full
shadow adding it, then `enable`, gave exactly one new link each, with the
existing link's target and mtime unchanged, and the shadow's new link
pointing at the `/etc` file. The unit had no alias, so an existing alias
link was not part of the test. Rule 5 holds the row to the file that supplies the added links:
the shadow, or every drop-in in `install_drop_ins`, must be staged. Rule 7
applies if the unit has `Also=` members, and since the unit has an install
file, it is then an advisory. With the shadow or a drop-in excluded, rule
5 omits the command as `InstallEvidenceChanged`, and the unit gets no
command and no advisory. With its one install file excluded, the image
loads the package's unit and is at its preset by that unit's links. With
one of several excluded, what the image loads is a mix nobody recorded,
and declining is the conservative result, not a claim that the image is
right.

**A link that is on the host is never advised for removal.** A vendor
link the effective `[Install]` does not name can still be on the host,
made by hand, and § 7.3 already reports it as a `NotInInstall` link. The
comparison is therefore checked against the admin links of § 7.3: such a
link is not `VendorInstallOnly`, since the host and the image both have
it.

**A unit whose `/etc` files leave it no link.** A vendor unit enabled by
its preset, with an `/etc` drop-in that holds only `WantedBy=`: on the
host `enable` makes no link for it and, with no link left, it reads
`static` (measured). It has no install state, so the rule above gives it
no row. In the image, the package install links it by the vendor
`[Install]` before the `COPY`, and copying the drop-in does not remove
that link, so the image would start a unit the host does not. The
comparison above covers it: no vendor link is an effective link, so the
result is not additive, and each vendor link the host does not have is a
`VendorInstallOnly` link with its removal line (§ 10.9). A full shadow
with no `[Install]` section is the same case. If the host still has a
link the package's section names, the host and the image both have it:
it is not advised for removal, and it is not reported as a link made by
hand either, since the package install makes it in the image too. It
produces nothing. A unit whose effective section was reset by a drop-in
but is still `Disabled` against its preset, because the links its
remaining values expect are absent, gets the ordinary `disable` row and
no install-difference finding: the test above applies only to `Enabled`
or to no install state.

**An install-difference advisory holds only while the files that cause it
are in the export, and an exclusion that breaks it is said.** Each
`HostInstallOnly` and `VendorInstallOnly` link records, in
`install_files`, the `/etc` install files of the unit that produce the
difference: its shadow and its install drop-ins. The advisory and its
repair line are rendered only when every one of them is in
`staged_writes`. When one is excluded, the repair lines are not rendered:
their premise, the difference between those files and the package's, no
longer holds for the export, so a line to add or remove a link can no
longer be trusted. In their place the unit gets one `# Omitted:` line
that names it and every excluded file and says its links on the source
host were not reproduced (§ 10.9). That matters because the files that
remain still act: with `10-reset.conf` clearing the vendor's `WantedBy=`
and `20-add.conf` adding another target, excluding only `20-add.conf`
leaves the reset staged and the package's link in the image, which no
longer matches the host and which nothing else would mention. The line
does not claim what the image then has; it says that the recorded
enablement is not what the export reproduces and which file to look at.
The refine row shows the advisory as `not_applied`, with the excluded
paths and the omission text (§ 11.1). The same holds for an
`UndeterminedInstall` record (§ 6.6).

Under the other matching presets the rule above already gives a row. An
`Enabled` unit under a `disable` preset gets `enable` after the `COPY`,
which makes the host's links. A `Disabled` unit under an `enable` preset
gets `disable`, which removes every link to it. Three cases stay
uncovered, and § 17 lists them:

- **A unit that matches no preset rule** gets no row from the rule above
  and is outside this test, so it has no command and no advisory whatever
  its `/etc` files do.
- **A unit under a `disable` preset that the package install enabled
  anyway,** through another unit's `Also=` (the second class below). The
  image then has the vendor's links before the command, and an `enable`
  for a shadow that changed the target adds the host's link and leaves the
  vendor's.
- **A changed `DefaultInstance=`.** If the `/etc` files change it from
  `old` to `new`, the image's package install linked the `old` instance.
  An additive row enables the `new` one (§ 7.5) and nothing removes the
  `old` link.

**The preset for an instance follows what the package ships. An instance
that has its own unit file is looked up by its own name: rules that name
it or a glob that matches it, in file order, and a list-form template
rule that lists it. An instance backed only by its template takes the
template's first matching rule: a bare `enable` gives `enable` to the
`DefaultInstance=` only, the list form gives `enable` to the listed
instances only, and `disable` reaches every instance; rules that name an
instance are not read for it. (Mark Russell, 2026-11-15)** The two cases
follow the two names a package's scriptlet can preset. A package that
ships a concrete `t@two.service` can preset that name, and `systemctl
preset t@two.service` applies rules that name it or list it (measured). A
package that ships only `t@.service` presets the template, and a preset
of the template ignores rules that name an instance (measured). The
snapshot does not record which names a scriptlet passes, so the rule
takes the name the shipped file suggests. That is an assumption until the
stock scriptlets are read (§ 13 R5, open), and § 17 states what it
misses.

*An instance with its own unit file,* `t@i.service`, rules in file order:

| Rule | Matches when | Preset when it matches | Evidence |
|---|---|---|---|
| `enable` or `disable` naming `t@i.service` | always | its verb | Measured: `preset t@two.service` under `enable t@two.service` links it. |
| A glob that matches `t@i.service` (`disable *`, `enable t@*`) | always | its verb | Assumed for an `enable` glob; `disable *` was the catch-all in every R5 case. |
| `enable t@.service a b` (the list form) | `i` is listed | `enable` | Measured: `preset t@two.service` under `enable t@.service two three` links it. |
| The list form, `i` not listed; a bare `enable` or `disable t@.service` | never; later rules decide | | Assumed. R5 saw no link from `preset t@one.service` under either rule, but `disable *` followed, so whether the rule stopped the lookup is not shown. |
| `Unknown` whose pattern matches `t@i.service` | always | none, and a warning | Not measured. |
| No rule matches | | none: no row | |

*An instance backed only by its template,* `t@i` with no file of its own:
the template's name, `t@.service`, is looked up as any unit's is, first
match wins, and the instance's preset is read off the rule found.

| The template's first matching rule | Instance | Preset | Evidence |
|---|---|---|---|
| `enable t@.service`, or an `enable` glob matching the template's name | the `DefaultInstance=` | `enable` | Measured for the bare rule: a preset of the template links `t@one`. The glob is assumed. |
| The same | any other instance, or every instance when there is no `DefaultInstance=` | `disable` | Measured for other instances: the preset of the template links `t@one` only. No `DefaultInstance=` is assumed. |
| `enable t@.service two three` | `two`, `three` | `enable` | Measured: a preset of the template links both. |
| The same | any other, the default instance included | `disable` | Measured: not `t@one`. |
| `disable t@.service`, or a `disable` glob matching the template's name (`disable *`) | every instance | `disable` | Assumed for a preset of the template. R5 measured `preset` on each enabled instance removing its link, which `disable *` would also explain. |
| `Unknown` matching the template's name | every instance | none, and a warning | Not measured. |
| No rule matches | every instance | none: no row | |

A rule that names an instance, `enable t@two.service`, matches neither
the template's name nor, for this case, anything read: a preset of the
template links nothing for it (measured).

**Whether a rule that does not count stops the lookup** arises only in the
first case, for the fourth row. The second case reads one rule, the
template's first match, and a preset of the template measurably links
only the instances that rule names. The first-case question stays an
open item of § 13 R5.

**A matching `Unknown` line stops the lookup and is reported.** systemd
skips a line whose verb it does not accept and goes on to later rules;
inspectah stops with no preset, which is a deliberate, conservative
difference: inspectah cannot tell whether the line is a verb a newer
systemd reads, so it does not guess what a later rule would mean. **The
line decides, and the warning names, only `Vendor`, `Shadowed` and
`PackagedStorage` units that are not masked. An enabled `Local` unit and
a mask keep their state changes. (Mark Russell, 2026-11-15)** Neither is
compared with a preset in the first place (the rules above), so the line
has nothing to decide for them. The collector raises one structured
warning per such line that decides at least one unit:

```
preset line <line> of <file> starts with a verb this scan does not read and is the first rule matching <unit>, <unit>; their state was not compared with a preset
```

with `file`, `line` and `units` among its fields. Each unit it decides is
in the inventory with no preset and gets no state change. This holds for
every such unit, not only instances. A vendor storage unit whose owner
lookup fails is still named, though it ends as a mount finding. The
warning is in the scan's diagnostics; the Containerfile does not name
those units (§ 17).

**The named cases.**

- **`getty@tty1` and `getty@tty2`.** Neither has a unit file of its own,
  so both take the template's rule. Both bases have `90-systemd.preset:
  enable getty@.service`, and `getty@.service` has `DefaultInstance=tty1`
  (measured). RHEL 9.8's `90-default.preset: enable getty@tty1.service`,
  which sorts first, names an instance and is not read for it. So on both
  releases `tty1` is `enable` and `tty2` is `disable`. Both bases already
  have `getty@tty1` linked (measured), so the image has it: a stock host
  has no row; an administrator who disabled `tty1` gets a `disable` row,
  found as § 7.5 says; an administrator who enabled `tty2` gets an
  `enable` row, matching `systemctl preset getty@tty2.service` making no
  link on either release (measured).
- **A package that ships a concrete `t@two.service`, with `enable
  t@two.service` then `disable *`, and the administrator disabled it.**
  First case: the own-name rule gives `enable`, the unit file is listed
  by `list-unit-files`, its install state is `Disabled`, and it gets a
  `disable` row. The image's package install enables it again, and the
  row's `disable` removes it.
- **A package that ships only `t@.service`, with `enable t@two.service`
  then `disable *`, and the administrator enabled `t@two`.** Second case:
  the template's first match is `disable *`, so `t@two` is `disable`, its
  install state is `Enabled`, and it gets an `enable` row. A preset of the
  template makes no link for it, so the image needs that `enable`.

The collector's preset rule gains the list form. Today's parser keeps the
first two words of a line, so `enable t@.service two three` reads as the
bare rule and counts for the default instance, which systemd does not.

```rust
/// One line of the host's `.preset` files, in the order systemd reads
/// them (§ 7.1 source 2).
pub(crate) enum PresetRule {
    /// `enable` or `disable` and one unit name or glob. A template name
    /// here is the bare form.
    Plain { verb: PresetDefault, pattern: String },
    /// `enable`, a template name and the instances that follow it.
    EnableInstances { template: String, instances: Vec<String> },
    /// A first word that is neither verb. Kept with where it came from,
    /// for the warning.
    Unknown { pattern: String, file: String, line: usize },
}
```

An `enable` line is `EnableInstances` when its second word is a template
name and more words follow. Every other line is read by its first two
words, as today.

**Stock state that differs from preset.** One named constant in
`crates/core/src/baseline.rs` lists units a stock install leaves away from
their preset with no administrator involved, each with the state it is
left in. It holds one entry: `remote-cryptsetup.target`, `Disabled`, its
state against an `enable` preset on a stock RHEL 9 host (measured). A unit
on the list, in the listed state, gets an inventory entry and no state
change, so the scan does not carry a `disable` nobody chose.

A second class is left alone: a unit a package install enables through
another unit's `Also=`. On RHEL 9, `NetworkManager-wait-online.service`
and, once `device-mapper-multipath` is installed, `multipathd.socket` are
enabled against a `disable` preset with no administrator involved
(measured). Each gets an `enable` row nobody chose, and it is carried.
The asymmetry with the list is deliberate. Installing the package in the
image enables these units the same way, so the carried line changes
nothing there, while a carried `disable remote-cryptsetup.target` would
switch off a unit the base image has enabled. Suppressing them would also
need the collector to know which enables came from an install, which the
host does not record. Run R4 reviews both classes.

**An owner at its preset whose member is off. The check applies to
every `Also=` member, not only instances. (Mark Russell, 2026-11-15)** A
unit with a `Vendor` or `Shadowed` fragment that is `Enabled` under an
`enable` preset has no row by the rules above, so rule 7 has no command
of its own to check. The image's package install enables it, and through its
`Also=` every member, whatever the member's own preset says. So the
collector walks `Also=` from such a unit as § 7.3 walks it for a state
change, and when the walk records any member with `MemberLinks`
`Unlinked` or `Other`, the owner becomes an `Enabled` state change
whatever the preset says, with its `also` list and the `attention_reason`
`A unit it enables through Also= is not enabled on the source host.` The
note keeps those words on a fleet aggregate row. **The check runs
whether or not the install comparison above finds a difference, so an
owner whose comparison is not additive, or whose package section is
`Undetermined`, can be an `Enabled` row that rule 7 advises beside its
per-link findings. (Mark Russell, 2026-11-15)** Rule 7 of § 10.2 then finds a member that does not match
`enable` and makes the command an advisory, `AlsoWhy::LinksDiffer`, so
the row never carries a line of its own: the operator gets the owner's
`enable`, which the image already has, and the member's `disable`, which
is what the image needs, as two lines to add by hand. A member that is
`Installed` (every link its `[Install]` names is present) does not
trigger it, and neither does one that is `Inert`: by § 6.3 that is a
member with no link of its own to make (no `[Install]`, only `Also=`, or
no unit file) and no link naming it, which the package install's
`enable` does not link either. A member with nothing to make and a link
made by hand is `Other`, and triggers it.

**`multipathd.socket`.** Installing `device-mapper-multipath` enables
`multipathd.socket` through another unit's `Also=` against its `disable`
preset (measured on RHEL 9); the record does not name the owner, and the
trace takes it to be `multipathd.service`, at an `enable` preset. On a
host where the administrator disabled only the socket, the socket is
`Unlinked`, `Disabled` at its own preset and without a row; the owner
becomes an `Enabled` row, and the Containerfile holds rule 7's advisory
naming `multipathd.socket`, with `RUN systemctl enable --
'multipathd.service'` and `RUN systemctl disable -- 'multipathd.socket'`
as the lines to add, and no `systemctl` instruction for either unit. On a
host where both are as the package left them, the socket is `Installed`,
the owner has no row, and this check emits nothing. The socket's own
`enable` row, enabled against its `disable` preset, is the second class
above and is carried as before.

**Stock hosts.** An owner whose package leaves a member unlinked in stock
state would get this advisory on every scan. The 2026-11-13 `Also=`
survey names no such owner: it records that every member of the 60
`Also=` units is installable, and the two members it names enabled
through `Also=` (`NetworkManager-wait-online.service`,
`multipathd.socket`) are linked in stock state, so their owners have no
row. The survey did not inventory every stock member's links, so this
is not shown to be the only answer. Run R4, still open, lists every
advisory the check produces on a stock host of each release and
classifies each one that recurs.

The name gate becomes: the name parses as a `UnitName` and is not a D-Bus
activation alias. The alias test narrows to the `dbus-org.` prefix, which
is what the comment at `services.rs:26-27` describes, so
`dbus-broker.service` is no longer dropped.

An entry that classifies as an `AdvisoryUnit` becomes a mount finding
(§ 6.5), a masked one included. A template name that is not masked is
skipped. Units inspectah must not switch off are marked at collect and
refused at render (§ 10.3), and units that update packages at runtime are
collected and excluded by default (§ 10.4).

### 7.5 Template instances

An instance whose install state is `Enabled` is a `UnitStateChange` of
`Enabled`, with the fragment and owner of its effective unit, under the
same rule as § 7.4 applied to the instance name. An instance linked from
two parents is one state change. Cleanup of an instance the image linked
and the host no longer has, after a changed `DefaultInstance=`, is not
carried (§ 7.4).

An enabled instance backed by its template is found from its links under
`/etc/systemd/system`; one with its own unit file, and a masked one, are
listed by source 1. A masked instance needs nothing new to be found: its
mask under `/etc/systemd/system` is found by form and its name parses.
**A masked instance is carried as `systemctl mask` once instances are
carried; until then the phase 4 placeholder omits it with every other
instance (§ 15). (Mark Russell, 2026-11-15)**

**A disabled instance is found from the presets.** An instance with its
own unit file is listed by `list-unit-files` and gets its install state
like any unit. An instance backed only by its template leaves no trace on
the host once it is disabled: no link and no row of its own. So the
collector derives the instances the image's package install would link,
from the presets, and checks each against the host's links. For each
template whose first matching rule (§ 7.4, second case) is a bare
`enable`, the candidate is its `DefaultInstance=`; for the list form,
each listed instance. Each candidate's install state is read by § 7.3,
with the template's effective `[Install]` expanded for the instance and
the links under `/etc/systemd/system`. One with no expected link there is
`Disabled` against an `enable` preset, a state change like any other,
and renders `disable`. A candidate with its own unit file is already
listed by source 1 and is not derived twice. This reads nothing new from
the host: the preset files (source 2), the links (source 3) and the
template's unit file, which source 1 lists and source 5 resolves.

**Instances named by an enabled unit's `Also=` are candidates too. (Mark
Russell, 2026-11-15)** A package install that enables a unit enables
every unit its `Also=` names, an instance whose package ships only the
template included, whatever that instance's own preset says. So for each
unit that is `Enabled` (§ 7.3) and whose effective `[Install]` is
`Known`, every instance its `Also=` reaches is a candidate, followed
through each member's effective unit until no new name appears, as
`enable` and `disable` follow it at build (measured). Each candidate's
install state is read as above; one with no link is `Disabled`. Its own
preset is usually `disable`, so it gets no row of its own; what reports
it is the owner's `Also=` check, which applies to every member (§ 7.4,
"An owner at its preset whose
member is off"). An owner whose section is `Undetermined` has no
readable `Also=`, so it derives no candidate; § 7.3 gives it an
`UndeterminedInstall` whenever it assigns `Also=`, which says its members
could not be checked.

Both kinds of candidate are built in phase 5, with carried instances
(§ 15). The owner check that reports a member, instance or not, is
phase 4's: it needs only the `Also=` walk, which resolves an instance
through its template. Until then such an instance is not collected, and phase 4 says
nothing about it.

**Instances tied to one host's devices are never carried.** An instance of
`systemd-cryptsetup@.service` or `systemd-fsck@.service` names a volume or
an escaped device path of the source host. Whatever its `[Install]` says,
such a link is a `ManualLink` with cause `HostDevice`. The list is one
named constant holding those two templates. A hardware-tied template that
is not on it is carried like any other instance. On both releases the
`systemd-fsck@` instance for `/boot/efi` has no link anywhere: it reads
`static`, its template has no `[Install]`, and the generated
`boot-efi.mount` pulls it in through `Requires=` (measured, § 13 R5). So a
stock host produces no `HostDevice` link from it, and the constant is for a
link made by hand. `systemd-cryptsetup@` was not measured: neither host
had an encrypted volume.

**What an instance needs.** An instance usually reads a file named for it,
and the enable is useless or harmful without that file. The collector fills
two lists on the state change from two sources each:

| List | Derived from the effective unit file | Fixed table (named constant) |
|---|---|---|
| `required_config` | Every path under `/etc` in the file that holds `%i` or `%I`, expanded for the instance. An `EnvironmentFile=` value with the `-` prefix is optional to systemd and is left out. | `wg-quick@` reads `/etc/wireguard/%i.conf`; `openvpn-client@` reads `/etc/openvpn/client/%i.conf`; `openvpn-server@` reads `/etc/openvpn/server/%i.conf`. |
| `data_paths` | Every path under `/var` in the file that holds `%i` or `%I`, and `/var/lib/<value>` for a `StateDirectory=` value that holds one. | `dirsrv@` keeps `/var/lib/dirsrv/slapd-%i`. |

**The three fixed lists start with exactly these contents: instance
configuration for `wg-quick@`, `openvpn-client@` and `openvpn-server@`;
instance data for `dirsrv@`; tied to a host's devices,
`systemd-cryptsetup@` and `systemd-fsck@`. (Mark Russell, 2026-11-13)**

**Every `required_config` path is kept**, whether or not anything exists
there. § 10.8 decides, from what the export holds.

**`data_paths` holds persistent data only.** A path under `/var/run` or
`/var/lock` resolves into `/run`, which is empty at every boot, so a
`PIDFile=/var/run/worker/%i.pid` is not data to migrate. The two prefixes
are one named constant. `RuntimeDirectory=` is not read. A remaining path
is kept only when it exists on the source host.

The table exists because the derivation cannot see the motivating cases
(the installed units were read on 2026-11-13). `wg-quick@.service` runs
`wg-quick up %i` and names no path under `/etc`. The OpenVPN templates
pass `--config %i.conf` with a `WorkingDirectory=`. `dirsrv@.service`
names its `/etc/dirsrv/slapd-%i` directory, which the derivation finds,
and never names its data directory. **An instance of a template on no
list, with neither list filled, is carried on its unit file alone, with an
`attention_reason` saying inspectah does not know what the instance
reads. (Mark Russell, 2026-11-13)**

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
fragment kept for the file check. The target is named by the last path
component of the link's text, and its file is resolved by that name as
§ 7.2 resolves any unit, with its owner looked up (§ 7.8). So a link
whose own target path is dangling still records the target when a unit
file of that name loads. A link to anything else produces a structured
warning and no record: a name that is not a `.target`, and a target the
resolver finds masked, as an alias, refused by the search path or with no
file, which reads `<name> has no unit file this scan reads`. The warning
is

```
/etc/systemd/system/default.target links to <link> - <detail>; the default target is not carried
```

and, when the link itself cannot be read,
`/etc/systemd/system/default.target - could not be read as a link (<error>); the default target is not carried`,
with `path`, and `target` when it was read, among its fields. This is its own check and render line:
not an enable, not compared with a preset, and recorded as included
whatever the target is (§ 10.5).

### 7.7 Drop-ins, shadows and masks

One rule decides who owns each thing found at
`/etc/systemd/system/<name>`, and the form test of § 7.2 runs first:

| What is at the path | Recorded as | Its file is written by |
|---|---|---|
| A link to `/dev/null`, or a zero-length regular file | A `Masked` state change when the name and the fragment under the mask classify as carried; a mount finding with state `Masked` when they classify as advisory (§ 6.2). Nothing else. | Nobody. `systemctl mask` makes the link in the image. |
| A link to another unit's file | Nothing, or a `ManualLink` when `Alias=` does not name it (§ 7.3). | Nobody. |
| A regular, non-empty file of a storage kind (`.mount`, `.automount`, `.swap`) | A mount finding (§ 6.5). The file is a config entry. | The config tree, on the config row's decision. |
| A regular, non-empty file of an ordinary kind with a vendor file of the same name | A full shadow in `drop_ins`. | The drop-in writer, on that entry's decision. The config entry at the path is `Inventory` with `shadowed_by` set. |
| A regular, non-empty file of an ordinary kind with no vendor file | A `Local` fragment. The file is a config entry. | The config tree, on the config row's decision. |

- **A mask has one representation.** However the host wrote it, a mask is
  a state and the export carries no file for it. The config walk's
  exclusion for symlinks directly under `/etc/systemd/system`
  (`is_systemd_unit_symlink`) extends to zero-length regular files with a
  unit suffix there. § 10.2 holds the same line for an export assembled
  some other way.
- **A storage unit's file has one owner, the config section.** A regular
  `/etc/systemd/system/tmp.mount` beside a vendor `tmp.mount` is a config
  entry under its own toggle, not a full shadow in `drop_ins`, and the
  unit is advisory (§ 6.2).
- **Drop-ins** are collected from `/etc/systemd/system/<unit>.d/*.conf` for
  every unit name that parses, plus `*.device.d` and `*.scope.d`, and for
  units that are generated (the parent is recorded as generated, detected by
  state or by a file under `/run/systemd/generator*`). The existing rule
  holds: the units section owns the file, and the config entry for the same
  path is `Inventory` with `shadowed_by` set
  (`shadow_service_owned_paths`, `crates/pipeline/src/collect.rs:274`). A
  drop-in on a generated parent is carried and does not bring the parent
  back. A `.conf` in a type-wide directory (`service.d`) belongs to no
  unit and is not a drop-in row: it reaches the snapshot as a config
  entry, like any other unowned file under `/etc`, and is carried on that
  entry's decision. That holds for an unowned file. Whether a
  package-owned, unmodified one there reaches a single-host snapshot is
  not verified in source and not measured (§ 13 run R2). Neither it nor a
  file in a dash-prefix directory is ever an install file, since `enable`
  does not read `[Install]` there (§ 7.2 step 4).

`SystemdDropIn` keeps its fields. `unit` stays a `String`, because a drop-in
may name a `.device` or `.scope` that `UnitName` does not admit.

**The config walk excludes all three dependency directories.**
`/etc/systemd/system/*.upholds/*` joins `*.wants/*` and `*.requires/*` in
the walk's exclusion list (`walk.rs:154-155`). Without it the walk follows
an `.upholds` link to a local unit and records the unit's content at the
link's path, which would stage a flattened file where the link was. This
is the answer to the question the overlap item asked about that
exclusion.

### 7.8 Owning package

`populate_owning_packages` runs over every `Vendor` and `Shadowed` state
change and mount finding, and asks about the file `UnitFragment::file`
names; for a masked unit, the vendor file under the mask. For storage
kinds the lookup runs before classification, so a failed query for a
vendor mount unit yields an advisory, not a carried line.

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
unit. The same rule is why the kernel section stops enabling
`tuned.service` (§ 10.2).

What changes in scheduled:

- `SystemdTimer` becomes a description: `name`, `on_calendar`, `exec_start`,
  `description`, `source`, `path`, `trigger`, `trigger_drop_ins`,
  `aggregate`. It loses
  `disposition` and `locked`, since it controls nothing, and
  `timer_content` and
  `service_content`, since the local file's content is already in
  `config.files` and a second copy is a second thing to redact.
- `scan_systemd_timers` keeps reading the files to fill those fields.
- The local-timer loop in the stager (`configtree.rs:440-449`) is removed.
  The config loop already writes the same files to the same place.
- The local-timer enable in `scheduled_tasks_section_lines`
  (`containerfile.rs:1382-1386`) is removed. Generated units keep theirs,
  as a request to `plan_units` rendered by the shared helper (§ 10.2).
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

**The generated-timer row's control applies. (Mark Russell, 2026-11-14)**
`SetInclude(false)` on a generated-timer row removes its generated files
from the staged tree and its `enable` from the Containerfile, and
`SetInclude(true)` restores both, through the refine export. The same
holds for the other scheduled rows that take an operation, cron jobs and
at jobs: an operation on one either changes the export or is refused by
name. Accepting an operation and applying nothing, which is what the
session's validation does today for these rows, in the same arm § 4
describes for the timer toggle, is not allowed for any of the three.

The invariant is met this way, and this is the contract for each row:

- **Generated-timer row.** Applies, as above. Including a generated unit
  whose cron job is excluded is refused with `BadRequest`, naming both
  rows.
- **One name from two cron sources.** Two cron jobs can generate units of
  one name (an `/etc/crontab` line and a file under `/etc/cron.d`). The
  name is one decision, and an operation applies to every record of it.
  The row shows included while any of its records is included, which is
  what the export carries: an included record still stages its files and
  its `enable`. So `SetInclude(false)` stays available while any record is
  included, even when one source's cron job is excluded and
  `SetInclude(true)` would be refused.
- **Cron-job row.** Applies, with a cascade: the cron job is the source of
  its generated units, so excluding it drops every unit generated from it
  and including it brings them back (a locked unit stays out). Refine's
  projection also excludes the units of an excluded cron job, so the two
  rows cannot disagree. A cron job that generates no unit has no export
  effect, and its operation is refused with `NoopTarget`, naming it.
- **At-job row.** Refused with `BadRequest`, naming the file. The export
  never carries an at job: the Containerfile lists every at job in a FIXME
  whatever its row says, so there is nothing to apply.
- **Bulk toggle.** The services-and-scheduling toggle sends no at-job
  operation, and no cron-job operation for a cron job that generates
  nothing, since each would only be refused.

**A carried timer needs its trigger. (Mark Russell, 2026-11-14)** The
second writer used to put a local timer's service file in the image
whatever its row said. With one writer, the operator can exclude the
service and keep the timer, and systemd refuses to start a timer whose
trigger unit is not in the image. Measured on both releases (§ 13 R5): at
build `enable` on such a timer exits 0 with no warning and makes its one
link; after boot the trigger is `not-found`, the timer is `inactive
(dead)`, the journal says the start is refused because the unit to
trigger is not loaded, `systemctl --failed` is empty and the system is
`running`. The timer never runs, and the journal lines are the only
trace. The scheduled inspector therefore records each
timer's trigger on its description, and § 10.2 rule 5 omits the timer's
`enable` when that trigger is a local unit whose file is not staged.

```rust
#[serde(rename_all = "snake_case")]
pub enum TimerTrigger {
    /// The unit the timer starts.
    Unit(UnitName),
    /// `Unit=` holds a value the inspector cannot make into a unit name.
    /// Also what a description with no recorded trigger reads as.
    #[default]
    Unresolved,
}
// on SystemdTimer:
#[serde(default)]
pub trigger: TimerTrigger,
/// Drop-ins under `/etc/systemd/system` that assign `Unit=` for this
/// timer.
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub trigger_drop_ins: Vec<String>,
```

**The trigger is declined on changed evidence. (Mark Russell,
2026-11-14)** The recorded name is only true while the files that set it
are in the export. A local `report.timer` with a drop-in holding
`[Timer] Unit=other.service` records `other.service`; with that drop-in
excluded the image's timer starts `report.service`, which the record
never checked. So the inspector records the files that set the trigger
beside its name, and the plan omits the `enable` when one of them is not
staged. The plan does not read unit files to find the trigger again.

- **What the list holds.** `trigger_drop_ins` is every drop-in under
  `/etc/systemd/system` that applies to the timer and assigns `Unit=` in
  `[Timer]`, whether or not a later one overrides it. The timer's own
  file is not listed: rule 5 already requires a local or shadowed timer's
  file to be staged, and a second entry would report one exclusion twice.
  When the trigger is the default, the same-named `.service`, and no
  drop-in assigns `Unit=`, the list is empty. A vendor drop-in arrives
  with its package and is not listed.
- **Specifiers.** In a `Unit=` value the inspector expands `%i`, `%I`,
  `%p`, `%N` and `%%`, as systemd does when it loads the timer, for a
  concrete instance timer and for a plain one. This is the `[Install]`
  reader's set (§ 7.2) plus `%I`, which `enable` does not expand and a
  loaded unit does. Any other specifier, or a
  result that is not a unit name this model admits, makes the trigger
  `Unresolved`. A value that cannot be resolved is never read as "no
  local trigger".
- **Where `Unit=` is read.** The timer's own file, then the `.conf` files
  in `<name>.timer.d` and, for an instance timer, in its template's
  `<prefix>@.timer.d`, under `/etc/systemd/system` and
  `/usr/lib/systemd/system`, in file-name order, an `/etc` file
  overriding a vendor file of the same name. The last assignment in
  `[Timer]` wins and an empty one resets to the default. No other drop-in
  directory is read (§ 17). An instance timer has no timer file of its
  own, so its description is recorded from its template's file; that and
  the template's directory come with carried instances (§ 15). Until
  then an instance timer has no description, and its `enable` would be
  omitted as `TimerTriggerUnresolved`.
- **An unrecorded trigger.** A timer with no description, or a
  description from a snapshot written before the field, has trigger
  `Unresolved`, so its `enable` is omitted as `TimerTriggerUnresolved`
  with its `# Omitted:` comment. A snapshot written before the field
  therefore loses its timers' `enable` lines until it is rescanned. That
  is accepted under the standing decision: **no upgrade or migration code
  for pre-1.0 file breaks while the spec is still moving (Mark Russell,
  2026-11-14)**. Reading the missing field as "no trigger to check"
  would pass exactly the timers the check exists for.

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
  28, once, and phases 1 to 5 of § 15 land on one feature branch that
  merges to `main` after phase 5. (Mark Russell, 2026-11-13) The first
  phase on the branch sets 28. Later phases change the serialized types
  again without a further bump, so a snapshot written by one phase is not
  valid input to a later one. Nothing outside the branch reads such a
  snapshot, and fixtures are regenerated by the phase that changes them.
- No shim, no migration: a version 27 snapshot is re-scanned. The bump
  carries the refusal test through all three loaders that
  `process-docs/skills/snapshot-schema-versioning.md` requires. Its payload
  is a snapshot with a `services` section and no `units`: it parses, and
  every socket and timer state the host had is absent from it with nothing
  to say so. The test is written in the first phase and re-run against the
  final types before the merge.

### 9.1 Consumers the compiler does not find

A renamed Rust field stops the build at every use. A string does not: the
section's name is also a string key, a section id, a TypeScript field and
a discriminator value, and each keeps compiling and running after the
rename. A frontend that still reads `service_states ?? []` shows an empty
section and reports no error.

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
| Frontend reads with an empty fallback | `crates/web/ui/src/components/MainContent.tsx:710-714`, `Sidebar.tsx`, `GlobalSearch.tsx`, `useKeyboard.ts`, `App.tsx` | `viewData?.service_states ?? []`, section id `"services"`, a shadow count over state rows with `s.shadow_type === "full_shadow"` | The new fields, section id `"units"`, and the shadow count over `unit_dropins` entries (§ 11.3) |
| Refine operation id | `ItemId::Service`, `crates/refine/src/session.rs:2807` and its TypeScript mirror | `Service { unit }` | `Unit { unit }` |
| Refine normalization and synthesis | `normalize_incompatible_services` (`crates/refine/src/normalize.rs:242`), `synthesize_orphan_shadows` (`crates/refine/src/classify.rs:547`) | Read and write `ServiceSection` | § 10.4 and § 11.3 |
| Redaction pass and its guard | The drop-in loop in `redact` (`crates/pipeline/src/redaction/engine.rs:978`) and the field guard in `crates/pipeline/src/render/mod.rs` | `snapshot.services` | `snapshot.units`, with the additions in § 6.7 |
| Reports | `crates/pipeline/src/render/audit.rs`, `report.rs`, `readme.rs` | Section name and counts | Renamed; counts by `unit.kind()` |
| Aggregate timer control | `crates/web/src/aggregate_handlers.rs:1158`, `AggregateItemRow.tsx` | Emits `ItemId::SystemdTimer` with an include flag | Removed in phase 3 with the control (§ 8) |

Phase 1 ends with a search of the tree for the quoted strings `"services"`,
`service_states` and `service_dropins`, and each remaining hit is renamed
or listed in the phase report with the reason it stays. The frontend test
suites that mock the view response are migrated in the same phase, since
they are the only check that the two ends agree.

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
    'report.timer'
RUN systemctl disable -- \
    'kdump.service'
RUN systemctl mask -- \
    'bluetooth.service' \
    'dev-hugepages.mount'
```

- One `RUN` per verb. Units within it are ordered by kind, then name, so the
  output is stable and a reader finds the sockets together. Instances,
  masked templates and package-owned storage units sit in the same lines.
- **Every unit is single-quoted and each list is preceded by `--`.** The
  operand is a proved `UnitName`, whose character set holds no quote and no
  line break, so `single_quote` (`safety.rs:162`) is total on it. Quoting
  replaces the refusal: a name with `\x2d` is legal and now renders, and
  `-.slice` is safe after `--`. Both forms work in a build (measured).
- **`systemctl_lines` takes the planned default target and
  `&[PlannedUnit]`, and nothing else.** A `PlannedTarget` and a
  `PlannedUnit` are built only by the check in § 10.2, so no line can name
  a unit that check did not pass.
- The same function emits the comment-only blocks: `Also=` advisories
  (§ 10.2 rule 7), mount advisories (§ 10.6), instance data advisories
  (§ 10.8) and hand-made link advisories (§ 10.9). None is an instruction.

`enable`, `disable`, `mask`, `set-default`, `add-wants` and `add-requires`
all work in a `RUN` step with no manager running, on both bases
(measured). Two more measured facts bound what a carried line means on the
target, and the exported README states each in a sentence:

- **First boot keeps it with the base's empty `/etc/machine-id`.** **The
  rendered `systemctl` lines are kept, the condition is stated, and the
  machine-id is guarded. No preset file is written. (Mark Russell,
  2026-11-14)** Both bootc bases ship a zero-length `/etc/machine-id`,
  and both were booted through `bootc install to-existing-root`, which
  deploys it unchanged. With it, first boot applies no presets. What was
  measured to hold: on RHEL 9.8, rendered `enable`, `disable` and `mask`,
  through a scan, build and install, three runs, each read after its
  first boot; on RHEL 10.2, on 2026-11-13, `disable`, `mask` and
  `add-wants` through two boots. The two-boot record does not document a
  rendered `enable` or `set-default`. `to-disk` was measured to deploy the
  zero-length file on RHEL 10.2 only, and that disk was not booted. An
  image whose `/etc/machine-id` reads `uninitialized`, or that has no
  such file, gets a preset pass at first boot. The pass reverses state
  that is contrary to the preset and leaves the rest: it undoes a
  rendered `disable` against an `enable` preset on RHEL 9, and that and a
  rendered `enable` against a `disable` preset on RHEL 10, where a unit
  with no preset rule also falls under `disable *`. A mask survives on
  both (measured for `uninitialized`; the missing-file case is read from
  systemd's source, as is the third case, a file that already holds a
  valid identity, which is not a first boot either). The two releases
  differ by a build option of their systemd packages, read from the
  package sources and not measured further. Read from source and not
  booted: none of bootc's install paths, the image builder's bootc disk
  and ISO pipelines, or the installer's container payloads writes
  `uninitialized` or removes the file, so the pass runs only when the
  image itself carries one or the other, which in practice means a later
  Containerfile layer or an installer `%post` did it (§ 13 run R3). A
  later `systemctl preset-all` likewise puts rendered `enable` and
  `disable` state back to preset.
- **`disable` removes the administrator's links and nothing more.** A unit
  that a package's `.wants` link or another unit's `Wants=` pulls in
  reads `disabled` afterwards, is still wanted, and is started at boot
  (measured: a `disable` of such a unit in a `RUN` step removed nothing).
  inspectah does not compare dependency graphs (§ 3), and does not turn a
  `disable` into a `mask`.

### 10.2 One check of the assembled export

Every rule about a `systemctl` line is a rule about the image: which file
the name resolves to there, and what the command does to it. A snapshot
that reaches render may be one host's scan, a fleet merge or an imported
file, and a refine session has switched rows on and off. Each row was
valid alone (§ 6). Only the assembled export can say whether the rows are
valid together, so the rules are checked once, on the export, by one
function, and the action plan is its output.

```rust
/// Where a staged file lands in the image: a normalized absolute path.
/// One constructor, which drops empty and `.` components and refuses
/// `..`, a NUL byte and a control character.
pub struct Destination(String);

/// Every file the export writes, and every file it was offered and
/// withholds. The stager writes `writes` and nothing else.
pub struct StagedWrites<'a> {
    pub writes: BTreeMap<Destination, StagedWrite<'a>>,
    pub withheld: Vec<(Destination, StagedFileKind, WithheldWhy)>,
}
/// `content` is borrowed from the snapshot, or owned when inspectah
/// generates it (the flatpak provisioning service).
pub struct StagedWrite<'a> { pub source: StagedFileKind, pub content: Cow<'a, str> }

pub enum WithheldWhy {
    /// A zero-length file at a unit path is a mask (rule 1).
    ZeroLengthUnitFile,
    /// Another staged file has this destination, or needs it as a
    /// directory.
    Conflict { other: Destination },
    /// A unit inspectah generates has the name of a collected unit.
    /// `unit` is the collected unit whose name the file has (the paired
    /// service, when only it collides); `reserved_by` is where that name
    /// came from, which chooses the FIXME's advice.
    GeneratedNameCollision { unit: UnitName, reserved_by: Reservation },
    /// The destination is `/etc/machine-id`.
    MachineId,
    /// An unmanaged file offered a path under `/etc/systemd/system`.
    UnmanagedUnitPath,
}

pub fn staged_writes(snap: &InspectionSnapshot) -> StagedWrites<'_>;

/// A unit and the one command the plan will write for it. The fields are
/// private to the planning module, the only place a value is built.
pub struct PlannedUnit { name: UnitName, action: UnitAction }

/// The default target the plan sets (§ 10.5), built only after rule 5.
pub struct PlannedTarget { name: TargetUnitName }

pub struct UnitPlan {
    pub planned: Vec<PlannedUnit>,
    /// The included default target, when it passed rule 5.
    pub default_target: Option<PlannedTarget>,
    /// The included default target, when rule 5 declined it, and why.
    pub default_target_declined: Option<(TargetUnitName, OmissionReason)>,
    /// Every command the plan did not write, and why.
    pub declined: Vec<(UnitName, UnitAction, OmissionReason)>,
}

pub fn plan_units(snap: &InspectionSnapshot, writes: &StagedWrites<'_>) -> UnitPlan;
pub fn systemctl_lines(default_target: Option<&PlannedTarget>, units: &[PlannedUnit]) -> Vec<String>;
```

**The default target has its own planned type and its own declined
field, and `systemctl_lines` takes the target first. (Mark Russell,
2026-11-15)** So `set-default` is not a `UnitAction`, and no `enable`,
`disable` or `mask` can name the target.

**`staged_writes` is the stager's decision, and what it lists is what
lands on disk.**

- **One list.** Today `staged_sources` (`configtree.rs:257`) lists every
  included file, and the writer then drops the ones another section owns
  and the ones `validate_path` refuses (`configtree.rs:375-388`). Both
  tests move into `staged_writes`.
- **One key.** Every check here and every writer uses the `Destination`,
  so `/etc/systemd/./system//bootc-x.timer` is the same file as the plain
  spelling. `..` stays refused, as `validate_path` (`configtree.rs:35`)
  refuses it today. The staging directory is created empty by the export
  and only the stager writes into it, so no symlink sits on a path.
- **No silent skip.** The two conflicts `safe_write_file` skips today
  (§ 4) are visible in the list before anything is written: two writes
  with one destination, and a destination another write needs as a
  directory. `staged_writes` withholds both files of such a pair, so a
  file and every command that depends on it are omitted together (rule
  5), and the Containerfile names both in a FIXME through the existing
  `unstaged_files` route. Any write that then fails, the flatpak writer's
  included, is an I/O error and fails the export.
- **No other writer under `/etc/systemd/system`.** **An unmanaged file
  whose destination is under `/etc/systemd/system` is refused by name
  before extraction and render. (Mark Russell, 2026-11-14)** The
  unmanaged-files section copies its files after the unit state section,
  from its own tree, without passing `staged_writes`. A scan does not
  collect unmanaged files there, but an imported snapshot can offer one,
  and it would be a second writer of a unit path that no rule above
  sees: it could replace a unit whose command was just planned, or put a
  zero-length file where rule 1 withheld one. The entry is tested by
  `Destination`, and the refusal covers every path below the directory,
  a file in a `.d` or `.wants` directory included. One shared decision
  withholds the entry, and only that entry, before a scan bundles its
  payload, before refine extracts it, and before render: it is not in the
  export and gets no `COPY`, and the Containerfile names it in a FIXME
  that says a unit file belongs under Configuration Files. The FIXME is
  written in the Configuration Files section, through the existing
  `unstaged_files` route, and not in the unmanaged-files section. Nothing else
  under the directory changes owner: drop-ins, shadows, local unit files
  and links are handled as § 7.7 says. Two types grow for it, neither of
  which has the field today: the withheld record gains an unmanaged
  source kind beside `StagedFileKind`, and the unmanaged row of the view
  response gains `withheld` (§ 11.1), which the UI shows.
- **Never the machine-id.** The image keeps the base's zero-length
  `/etc/machine-id`, which is what makes first boot keep the rendered unit
  state (§ 10.1). inspectah never stages the source host's machine-id and
  never writes `uninitialized`. The config walk excludes the path on a
  scan (`walk.rs:45`, the exact-exclusion list). `staged_writes` holds the
  same line for every writer: a write whose `Destination` is
  `/etc/machine-id` is withheld as `MachineId`, whichever section of an
  imported snapshot or fleet mix offered it and however the path is
  spelled, and the Containerfile names it in a FIXME. The guard runs as
  soon as an offer's `Destination` is built, before anything else
  `staged_writes` does to it: before a config entry at a path another
  section owns is dropped (`owned_elsewhere`), which would otherwise drop a
  config entry at a drop-in's path with no reason, and before offers of
  one destination are deduplicated. So every row that offered the path
  keeps its withheld reason. An unmanaged file at the path is withheld as
  `MachineId` by the same shared check that refuses unmanaged files under
  `/etc/systemd/system` (`unmanaged_withheld`): scan bundling, refine
  extraction, the unmanaged-files render and `staged_writes` all ask it,
  so the file is not bundled, not extracted and not copied. The FIXME's
  reason reads: `the image keeps the base image's empty /etc/machine-id,
  which is what makes first boot keep the rendered unit states; the source
  host's machine identity is not copied`. The README written with a scan
  export says it in its unit state paragraph; a refine export writes no
  README, by its existing contract. The paragraph reads:
  `The unit states in the Containerfile's Systemd Unit State section
  survive first boot with the base image's empty /etc/machine-id. A
  later layer or installer script that removes that file or writes
  uninitialized to it makes first boot apply the presets, which can undo
  the rendered enable and disable lines (measured: RHEL 9 kept an enable
  and undid a disable, RHEL 10 undid both). A rendered mask is kept. A
  later systemctl preset-all returns enabled and disabled units to their
  presets and leaves masks in place.` (in the README, the path, the word
  `uninitialized` and each verb are in backticks). The paragraph, with
  § 10.1's sentence on `disable`, is written when the plan carries at
  least one `enable`, `disable` or `mask`. A plan whose only line is
  `set-default`, or that declines every command, gets neither.

**What the plan takes.** Three kinds of request. No `systemctl` line in
the Containerfile comes from anywhere else.

1. Each included state change.
2. The default target.
3. Each unit inspectah writes from its own template and enables: the
   timers and `@reboot` services the scheduled section generates from cron
   entries (`make_timer_service`, `scheduled.rs:676`), and
   `flatpak-provision.service`. Their files are in `staged_writes` at their
   image destinations. Each is copied and enabled in its own section, in
   that order, with the line the shared helper produces. A generated unit
   whose name is also a collected unit's is resolved before the list is
   final: `staged_writes` withholds its files, the paired service of a
   generated timer included, the request is declined with a FIXME, and the
   collected unit is then planned against the files that remain. So a
   collected vendor timer is never enabled over a generated file of its
   name. **For this rule a collected unit is any unit the units section
   names: a state change, a mount finding, or an inventory entry. (Mark
   Russell, 2026-11-14)** A package's unit at its preset has no row and
   is only in the inventory, and a generated `cron-*` pair that took its
   name would replace it in the image just the same. Such a pair is
   withheld, both files. The collision reason carries the conflicting
   name and where the reservation came from, a row or the inventory, so
   the two FIXMEs can differ. Both name the collected unit. For a row it
   says to exclude the collected unit and its file, and the flatpak case
   below is unchanged: `inspectah generates this unit, and a unit
   collected from the host, <name>, has the name of one of its files, so
   the generated files are not written and the unit is not enabled here;
   to carry the generated unit, exclude the collected unit and its file`.
   For an inventory name there is no row to exclude: `inspectah generates
   this unit, and <name>, a unit the host already has, has the name of one
   of its files, so the generated files are not written and the unit is
   not enabled here; <name> has no row to exclude, so to carry the job,
   write its timer and its service by hand under other names, keeping the
   timer's Unit= or their shared name consistent`. A mount finding counts
   as an inventory reservation. It cannot meet a generated name today,
   since generated names end in `.timer` or `.service` and a mount
   finding's in `.mount`, `.automount` or `.swap`. Nowhere else does this spec widen
   "collected unit": § 10.1 and § 10.7 mean the units that have a row. A
   known case is not designed around: a host built from an inspectah
   image has a local, enabled `flatpak-provision.service`, so a rescan of
   it carries the collected copy and withholds the generated one until
   the operator excludes the collected row and its file.

**The kernel and boot section's own `RUN systemctl enable tuned.service` is
removed; `tuned.service` is enabled or not through its own unit row. (Mark
Russell, 2026-11-13)** That section enables `tuned.service` whenever tuned
activation is included (§ 4), whatever the unit's state on the host, and
with the `tuned` package excluded the line fails the build. Whether the
unit is enabled is unit state, and the units section is its one owner
(§ 8). The stock preset enables it: in both bootc bases, which do not
ship `tuned`, `dnf install tuned` leaves `tuned.service` enabled and
linked through `90-default.preset: enable tuned.service`, ahead of
`disable *` (measured, § 13 R5). So a host at preset gets the same from
the image's preset, and a host that enabled it
against a `disable` preset has a state change that goes through this plan.
A host with an active profile and the service disabled therefore gets an
image with the service off, which matches the host and differs from today.
The kernel section keeps the lines that write the active profile.

For each request the plan runs these rules in order. The first one that
fails decides.

| # | Rule | Applies to | When it fails |
|---|---|---|---|
| 1 | **No zero-length unit file is staged.** A zero-length file at `/etc/systemd/system/<name>` is a mask. `staged_writes` withholds it, whichever section offered it. | Every staged file, by `Destination` | Not written, and named in a FIXME. A mask reaches the image only as a `mask` command that passes rules 3 and 4. |
| 2 | **Storage units act only on a package's file.** If the export stages a unit file at the name of a `PackagedStorage` unit, the administrator's file is what the image would load. | `enable`, `disable`, `mask` on a storage kind | Omitted: `the export carries an administrator's unit file for it (<path>)`. The mount advisory (§ 10.6) is rendered for the staged file. |
| 3 | **Units the target needs are not switched off** (§ 10.3). | `disable` and `mask` on a protected unit; `mask` on `tmp.mount` | A FIXME line, no command. |
| 4 | **A mask needs the path free.** `systemctl mask` makes a link at `/etc/systemd/system/<name>`, after every `COPY` (§ 10.7), and exits 1 when a regular file is there (measured). | `mask` when a unit file is staged at the name | Omitted: `the export also carries a unit file at <path>; exclude one of the two`. |
| 5 | **The command acts on the file the row was recorded from, and its package is in the image.** See below. | `enable`, `disable`, `set-default`; the package half also `mask` | Omitted: `UnitFileNotStaged`, `InstallEvidenceChanged`, one of the three `TimerTrigger` reasons, or the package reason the presence tiers give today. |
| 6 | **An instance needs its configuration** (§ 10.8). | `enable` on an instance | Omitted, naming the path. |
| 7 | **`Also=` members match, and nothing in `/etc` changes what `Also=` means.** See below. | `enable`, `disable` with a non-empty `also` list | An advisory, no command. |

Rules 2 and 4 read the same fact, what `staged_writes` holds at the unit's
name, and they answer the fleet case directly. One host's enabled,
package-owned mount unit and another host's administrator-written file of
that name merge into two valid rows. While the file's config row is
included, rule 2 omits the enable and the advisory explains the file;
when the operator excludes the file, the enable returns.

**Install files.** Rules 5 and 7 ask one question of the export: which
files under `/etc/systemd/system` can supply or change `[Install]` for a
name `N`. They are:

- the unit file at `N`, and for an instance its template's unit file;
- every `.conf` in `N.d` or, for an instance, in its template's `.d` that
  holds an `[Install]` section, or that has the file name of a vendor
  drop-in in one of those directories.

A file whose name starts with `.` is never an install file (§ 7.2 step
4). Nothing else is an install file. The type-wide directory (`service.d`)
and the dash-prefix directories (`a-.service.d`) still apply to `N` for
its other sections, and their files are carried as configuration (§ 7.7),
but `enable` does not read `[Install]` from them (measured, § 7.2 step 4),
so neither rule asks about them.

**An `/etc` drop-in is an install file if it holds an `[Install]`
section, or if a vendor drop-in with the same file name exists in a
drop-in directory that applies to the same name. (Mark Russell,
2026-11-13)** systemd picks between drop-ins of one file name by
directory before it reads them, so an `/etc` file with no `[Install]`
section hides a vendor file of that name that has one, and excluding the
`/etc` file brings the vendor section back. The names come from
`vendor_drop_ins` on the row and on each member (§ 7.2). This precedence
was measured in the unit's own `.d` and in the template's `.d`: with the
`/etc` file, `enable` made no link, and without it, `enable` linked the
unit (§ 13 R5).

"Holds an `[Install]` section" is the only lexical read the plan makes.
After lines that end in a backslash are joined to the next, the file has a
line whose trimmed text is `[Install]`. The keys are not read, so an
assignment with spaces around `=`, one continued over several lines and an
empty reset all count, and a file whose content was not captured counts
too. A drop-in with no such section and a file name no vendor drop-in
has, the usual `[Service]` override, is not an install file.

**Rule 5, the row's own file.** `enable` and `disable` read the `[Install]`
of whatever file the image loads. The row recorded one file. If the export
changes which file that is, by excluding it or by staging another, the
command acts on an `[Install]` nobody recorded, its `Also=` included.

| The row's fragment | The export must | Otherwise |
|---|---|---|
| `Local` | Stage the unit file at `/etc/systemd/system/<file>`. | Omitted: `UnitFileNotStaged`. |
| `Shadowed` | Stage the shadow at `/etc/systemd/system/<file>`. | Omitted: `InstallEvidenceChanged`, with the shadow's path. |
| `Vendor`, or a `PackagedStorage` unit | Stage no unit file at that name, and have the owning package in the image by the presence tiers (`classify_service_presence`, unchanged; `owning_package: None` keeps Tier 1: emit). | Omitted: `InstallEvidenceChanged`, or the package reason the tiers give today. |

And for every fragment, the install drop-ins the export stages for the
unit are exactly the row's `install_drop_ins`: none missing, none added.
Each recorded path is read through `Destination`, and one that does not
parse fails the rule. In a fleet aggregate, a staged install file whose
row holds more than one content variant fails it too, since the row may
have been recorded from another variant. For an instance whose `file` is
its template, a unit file staged at the instance's own name fails it as
well. All of these are `InstallEvidenceChanged`, reading `the [Install]
section in the image would not be the one recorded on the source host
(<path>)`.

**A hiding drop-in is install evidence.** A drop-in in a directory ranked
above `/etc` that hides one of the unit's `/etc` install drop-ins (§ 7.2,
load order) is in the row's `install_drop_ins` beside the files it
hides, since an image without it reads the hidden file. So rule 5
carries the command only when the export stages the hiding file too. A
file under `/run` is never staged, so its unit's `enable` or `disable` is
always omitted. A file under `/etc/systemd/system.control` is staged, and
so counts, only while its configuration entry is included. The omission
is `InstallEvidenceChanged` naming the hiding file, for example
`# Omitted: app.service (the [Install] section in the image would not be the one recorded on the source host (/run/systemd/system.control/app.service.d/50-CPUQuota.conf))`.
Of several above-`/etc` files of one name, only the one systemd reads is
recorded: the lowest directory rank, then the first in search-path order.
The hiding file is not added to a link finding's or an undetermined
record's `install_files`, nor to the § 7.4 comparison. On a fleet, the
union of `install_drop_ins` holds every host's hiding file, so the merged
row needs every one staged.

So a unit recorded `Shadowed` whose shadow is excluded gets no `enable`
and no `disable`: the vendor file's `[Install]` is not the row's install
evidence. A unit
whose install drop-in is excluded gets no `enable` that would exit 0 and
link nothing. A generated unit's request needs only its own file, which is
in `staged_writes` by construction. `disable` on an absent unit exits 1 on
RHEL 9 (0 on RHEL 10), as `enable` does on both (all measured).

**A timer's trigger is held to the same rule.** After the timer's own
file and package pass, an `enable` on a timer is checked against its
recorded trigger (§ 8), in three steps. Each omits the `enable` with an
`# Omitted:` line.

1. **The evidence is unchanged.** Every path in `trigger_drop_ins` is in
   `staged_writes`, each read through `Destination`, and one that does
   not parse fails. This is rule 5's own test, none of the recorded files
   missing, asked of the files that set `Unit=`. It has its own reason,
   `TimerTriggerEvidenceChanged`, because `InstallEvidenceChanged`'s text
   is about `[Install]`: `the drop-in <path> that sets its Unit= is not in
   the export, so the unit it would start is not the one recorded`.
2. **The trigger is known.** A trigger recorded `Unresolved` is
   `TimerTriggerUnresolved`: `its Unit= could not be resolved to a unit
   name, so inspectah cannot tell whether that unit is in the image`.
3. **The trigger is in the image.** `TimerTriggerNotStaged` when the
   snapshot holds the trigger as a local unit, a unit file under
   `/etc/systemd/system` with no vendor file of that name, that file is
   not in the final `staged_writes`, and nothing else provides the unit:
   no generated unit of that name is staged, and no package in the image
   owns it. "No package in the image owns it" reads the `package` of the
   trigger's config entry against the presence tiers' target package set.
   With no baseline the package is taken as present, as the tiers take an
   unproved absence for `enable` (Tier 6 emits the line with its
   `BaselineUnavailable` advisory). A trigger the base image or an included package ships, and
   one that is a full shadow, pass, since the vendor file loads. The line
   reads `# Omitted: report.timer (its unit report.service is not in the
   export, and a timer whose unit is missing does not start)`. It says
   unit, since `Unit=` may name any kind.

The check sits here, and not as a rule of its own, because it is rule 5's
question asked of a second unit, and it comes after the timer's own file
so that a timer whose file is missing is reported for that. `disable` and
`mask` on a timer need no trigger. Passing it is not being carried: rules
6 and 7 still apply, and every gate is computed again each time a row is
included or excluded.

What step 3 can and cannot know: it reads recorded config ownership,
fragment and shadow evidence, and the staged generated files. It does not
prove what a target base supplies. So it can omit a timer that would have
worked, when the trigger is provided by an alias, by a template behind an
excluded concrete instance, by an implicit mount, or by a generator at
boot. Each such omission carries its `# Omitted:` line. Step 1 checks
only for a recorded file that is missing: a drop-in from another host of
a fleet that assigns `Unit=` is not seen.

**A mask follows the package check. (Mark Russell, 2026-11-14)** `mask`
reads no `[Install]`, so the file and drop-in halves of this rule do not
apply to it. The package half does, through the same presence tiers,
unchanged:

- The owning package is not in the image: the mask is omitted with the
  tiers' package reason on an `# Omitted:` line, so the host's mask stays
  visible.
- The base image ships the package: the mask is carried.
- No owning package is known, which includes a mask on a name with no unit
  file (`Absent`): the mask is carried.

Rules run in order, so rule 3 and rule 4 decide first. A mask on a
protected unit or on `tmp.mount` is a FIXME whatever its package's state.
A mask whose path a staged file occupies is `MaskPathOccupied`. Only then
is the package asked. A zero-length shadow is withheld by rule 1 either
way, and its unit's mask is then judged like any other. On a fleet the
row's owner is the representative host's (§ 12), as for `enable`. The
first-boot condition of § 10.1 does not rely on masks, so the two are
independent: a mask that is carried survives a preset pass, and one that
is omitted was never in the image. Because
the rule asks `staged_writes`, the renderer and the stager cannot
disagree, which is the defect in
`inspectah-timer-enable-lines-unquoted-and-trust-empty-units` item 2.

**Rule 7, `Also=` members.** `systemctl enable U` also enables every unit
`U`'s `[Install]` names in `Also=`, and the units those name in turn, in
whatever package they live. `systemctl disable U` removes every link under
`/etc` to `U` and to each of those members, links made by hand and links
in other targets' `.wants` included: `disable libvirtd.service` removes
socket links that `virtqemud.service` relies on. `mask` does not follow
`Also=`. All measured.

**The rendered verbs stay `systemctl enable` and `systemctl disable`. A
command for a unit whose effective `[Install]` has `Also=` is carried when
every member's recorded state on the source host matches what the command
would produce: an `enable` needs every member on, a `disable` needs every
member off. On a mismatch the command becomes an advisory: no command, and
a comment giving the reason and the lines to add by hand. Rendering the
host's individual links with `add-wants` and `ln -s` in place of `enable`
was considered and rejected: the Containerfile should read like one a
person would write. (Mark Russell, 2026-11-13)**

**In a fleet aggregate export, a command for a unit with a non-empty
`Also=` list is always an advisory, with the lines to add by hand. (Mark
Russell, 2026-11-13)** The merge does not keep every host's file paths
(§ 12), so an aggregate cannot show that no install file exists.

In a single-host export, a command on `U` with a non-empty `also` list is
carried only when all three conditions hold. Each is one pass over data
already in hand, and nothing follows `Also=` at render. When more than
one fails, the first in this order gives the `AlsoWhy`.

1. **Every member's links match the verb.** For `enable`, each member is
   `Installed` or `Inert`. For `disable`, each is `Unlinked` or `Inert`.
   `Other` matches neither (§ 6.3). The state is the one read from the
   links (§ 7.3), so a member with no `[Install]` and a hand-made link,
   which systemd reports as `enabled`, is `Other`.
2. **Every member is acted on only as its own rules allow.** A member that
   has a state change row: the row is included, has the same verb, and
   passes rules 1 to 6. A member with no row is refused by name in two
   cases the evidence cannot speak to: a template instance under
   `enable`, whose configuration only its own row checks, and a protected
   unit under `disable`, which a package-mode source does not have and the
   image does.
3. **The install evidence is what was recorded.** Neither `U` nor any
   member has an install file in the snapshot, staged or excluded. The
   list and the links were then read from vendor files alone, and vendor
   files are what the image loads for every one of them, on the
   assumption § 17 states: that the image's vendor files hold the
   `[Install]` the source's did. **A unit whose
   staged file sets `Also=` is always an advisory, with the lines to add
   by hand. (Mark Russell, 2026-11-13)** That is one case of this
   condition, and it holds on a single host where the list and the file
   agree.

One pass is enough because the `also` list is everything `Also=` reaches
from `U`, so a member's own members are in it, and condition 3 means no
file in the export can add an edge, drop one, or change what `enable`
does to a member. What that costs: a local unit or full shadow that has
`Also=` members, and a unit or member with an install drop-in, are
never carried. The units that use `Also=` in the measured package set are
vendor units with nothing under `/etc` but links, so the ordinary case, an
administrator who ran `enable`, is carried.

Three things rule 7 leaves alone. A member whose file is not in the image
is skipped by `enable` with exit 0 (measured), and `disable` of the unit
exits 0 on both releases, removes its own link and any present member's,
and says nothing about the absent one (measured, § 13 R5). Only an absent
unit named on the command line fails, and only on RHEL 9. A member with no row is at its preset on the source, and by
§ 3 the image has it there too. And a snapshot whose recorded list
disagrees with its own unit file, which a scan cannot produce, is believed:
the plan checks that the files are the recorded ones, not that the record
is true.

`OmissionReason` gains:

```rust
/// A local unit's own file is not in the export (rule 5).
UnitFileNotStaged,
/// The file or drop-ins that supply `[Install]` in the image are not the
/// ones the row recorded (rule 5).
InstallEvidenceChanged { path: String },
/// The export carries an administrator's file for a storage unit (rule 2).
StorageFileStaged { path: String },
/// The target needs the unit, or needs it unmasked (rule 3). A FIXME.
TargetNeedsUnit,
/// A timer's trigger unit is local and its file is not in the export
/// (rule 5).
TimerTriggerNotStaged { trigger: UnitName },
/// A drop-in that sets a timer's `Unit=` is not in the export (rule 5).
TimerTriggerEvidenceChanged { path: String },
/// A timer's `Unit=` could not be resolved to a unit name (rule 5).
TimerTriggerUnresolved,
/// A mask and a unit file claim one path (rule 4).
MaskPathOccupied { path: String },
/// A unit inspectah generates has a collected unit's name. A FIXME.
GeneratedNameCollision { name: UnitName, reserved_by: Reservation },
/// The command would also act on `member` (rule 7). An advisory.
/// `member` is the first member in `also` order that fails the first
/// failing condition; for `InstallFile` on the unit's own file and for
/// `FleetAggregate`, the first member. `named` is what the first line
/// names first: every member failing as `member` does, the unit whose
/// install file it is, or every member on a fleet aggregate. `further`
/// holds the members failing the same condition the other way it can
/// fail, with that reason. `add` is the members given a line of the
/// opposite verb.
AlsoMember {
    member: UnitName,
    why: AlsoWhy,
    verb: UnitAction,
    named: Vec<UnitName>,
    further: Option<Box<(AlsoWhy, Vec<UnitName>)>>,
    add: Vec<UnitName>,
},

/// Where a name that blocks a generated unit came from.
pub enum Reservation { Row, Inventory }

pub enum AlsoWhy {
    /// The member's links on the source are not what the verb produces.
    LinksDiffer(MemberLinks),
    /// The member has a row the plan does not carry with this verb.
    RowNotCarried,
    /// The member has no row and is an instance or a protected unit.
    NeedsOwnRow,
    /// The unit or the member has an install file in the snapshot.
    InstallFile { path: String },
    /// The export is a fleet aggregate.
    FleetAggregate,
}
```

Every reason not marked renders as an `# Omitted:` line. The `Also=`
advisory, for a links mismatch:

```
# Advisory: libvirtd.service is enabled on the source host. Not carried: systemctl enable on it also enables libvirtd.socket (named by Also=), which is not enabled there.
#   To reproduce the source host, add after the configuration COPY:
#     RUN systemctl enable -- 'libvirtd.service'
#     RUN systemctl disable -- 'libvirtd.socket'
#   Each command follows Also= in its turn. Check the result with systemctl is-enabled.
```

The lines are `U`'s own command, then the opposite verb for each member
recorded on the other side: `Unlinked` under an `enable`, `Installed`
under a `disable`. That pair reproduced the host exactly for
`cups.service` and `cups.socket` (measured).

**Rule 7's first line, its clauses and its lines to add read as below.
(Mark Russell, 2026-11-15)** The first line is `# Advisory: <unit> is
<enabled|disabled> on the source host. Not carried: ` and the reason's
text. **It names every member that fails the first failing condition, one
clause group per link value or condition 2 cause. (Mark Russell,
2026-11-15)** The groups, each with `which is` or `which are` by number:

| `why` | Clause after `<members> (named by Also=), ` |
|---|---|
| `LinksDiffer(Unlinked)` | `which is not enabled there` / `which are not enabled there` |
| `LinksDiffer(Installed)` | `which is enabled there` / `which are enabled there` |
| `LinksDiffer(Other)` or `(Inert)` | `whose links there neither command reproduces` |
| `RowNotCarried` | `whose own row is not carried with this command` / `whose own rows are not carried with this command` |
| `NeedsOwnRow` under `enable` | `an instance with no row here to check its configuration` / `instances with no row here to check their configuration` |
| `NeedsOwnRow` under `disable` | `which the image needs` |
| `FleetAggregate` | `and fleet evidence may omit install metadata` |

A second group, for `further`, follows as `, and <members>, <clause>`. An
`InstallFile` reason reads `<path> sets [Install] for <unit>, so the
units systemctl <verb> reaches through Also= in the image may not be the
ones recorded.`, and is followed by `#   After reading that file, add
after the configuration COPY:` and `U`'s command alone. **A
`FleetAggregate` advisory says `on the source hosts` in its first line,
names every recorded member, and is followed by `#   Before adding this
after the configuration COPY, check the effective [Install] and Also= of
<unit> in the image, and the intended state of each member:` and `U`'s
command. (Mark Russell, 2026-11-15)** The plural holds even when one host
recorded the unit. For `RowNotCarried` and `NeedsOwnRow` the block names
the members and gives no line. A links mismatch whose `add` is empty, all
its failing members `Other`, also gives no line. Each line to add reads
`#     RUN systemctl <verb> -- '<unit>'`.

**Rule 7's recipe never suggests disabling a protected unit (§ 10.3): it
names the unit and says the image needs it. The "To reproduce the source
host" header and the footer are dropped when any member is `Other` or a
suggestion is withheld. (Mark Russell, 2026-11-15)** This is the one
exception to the opposite-verb rule above. A member in `add` that § 10.3
refuses for the opposite verb gets no line; after the suggested lines it
gets `#   No <verb> line for <unit>: the image needs this unit.` When a
line is withheld, or `why` or `further` is `LinksDiffer(Other)`, the
header is `#   These lines do not reproduce the source host. Check each
unit, then add after the configuration COPY:` and no footer follows. A
block in which every member can be reproduced keeps the header and footer
quoted above.

**The suggested `disable` follows the member's own `Also=`.** `systemctl
disable <member>` acts on every unit the member's own `Also=` reaches, in
its turn. Where that walk comes back to `U` or to a sibling that should
stay on, the suggested line unlinks it again. The recipe does not guard
against this: before keeping the lines, the reader checks every unit in
the closure with `systemctl is-enabled` (§ 17). The footer's "Each
command follows Also= in its turn" is the reminder.

**The no-RPM path runs every rule.** `render_service_intent` returns early
into `render_without_rpm` when the snapshot has no package section
(`service_intent.rs:519-522`). `plan_units` has no such branch. Without
package data only the presence tiers inside rule 5 have nothing to read,
and they emit, as that path does today.

`# Omitted:` lines for not-carried and excluded owners, and the not-carried
list's "was enabled" annotation (`not_carried.rs:259-290`), work for every
kind with no change, since neither looked at the suffix.

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

**A mask on `tmp.mount` is refused the same way. (Mark Russell,
2026-11-13)** Both bootc bases ship
`/usr/lib/systemd/system/local-fs.target.wants/tmp.mount`, so on the
target `/tmp` is mounted from that unit whatever `is-enabled` reads. With
the unit masked on a booted RHEL 10 bootc host, `/tmp` is the read-only
image's directory, `bootc status` fails, and a bootc service fails at
boot. On a package-mode source the same mask is harmless: after a reboot
`/tmp` is a plain directory on the writable root filesystem. A masked
`tmp.mount` on a booted RHEL 9.8 bootc host gives the same read-only
`/tmp` and the same failures (all measured). The plan
writes:

```
# FIXME: tmp.mount is masked on the source host, where /tmp is then a directory on the root filesystem. Not carried: the bootc base image mounts /tmp from this unit, and without it /tmp is read-only.
```

This narrows the rule that vendor mount units carry state and does not
reverse it: `enable` and `disable` on `tmp.mount` are carried, and so are
masks on other package-owned storage units. The name is one constant
beside the protected list.

The guard holds on every route by which the export could switch such a
unit off:

- **The command.** Rule 3 of § 10.2.
- **A mask written as a file.** The collector records a zero-length unit
  file as a `Masked` state and no file (§ 7.7), and rule 1 withholds one
  at any unit path, whichever section of an assembled or imported snapshot
  offers it. The test is on the `Destination`, not on the row's `unit`
  field or the path as spelled.
- **Another unit's `Also=`.** Rule 7: a `disable` with a protected member
  is not carried, with or without a row for the member, and rule 7's
  recipe never suggests a `disable` for a protected member (§ 10.2).

What the guard does not cover: a carried drop-in or full shadow for one of
these units that changes what the unit does (an emptied `ExecStart=`, a
file holding only a comment, which loads as a broken unit). Those are unit
content, carried on their own rows. The list names units whose state
inspectah will not reverse, and claims nothing more.

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
The row is not locked: including one is right for the `-notifyonly` and
`-download` variants. While excluded, the Containerfile names it:

```
# Omitted: dnf-automatic.timer (updates packages in place, which an image-based host does not support; excluded by default)
```

The names are one named constant in `crates/core/src/baseline.rs`: the
glob `dnf-automatic*.timer` and `packagekit-offline-update.service`. One
function beside it, `apply_runtime_updater_default`, sets the disposition
and the reason. The collector calls it on a fresh scan and the fleet merge
on a fresh aggregate row (§ 12). Refine's loader does not, so an
operator's include survives export and reload.

**The refine loader's lock gives way.** `normalize_incompatible_services`
excludes and locks every unit in `INCOMPATIBLE_SERVICES`, and
`packagekit-offline-update.service` is one of the four names there
(`crates/core/src/baseline.rs:141`). A locked row cannot be included, so
that name is removed from `INCOMPATIBLE_SERVICES` and the updater constant
is its one owner. A unit test asserts the two constants share no name. The
three names that stay (`dnf-makecache.service`, `dnf-makecache.timer`,
`packagekit.service`) keep today's locked exclusion, and the normalizer is
otherwise only migrated to the new types.

### 10.5 The default target

**`RUN systemctl set-default -- '<target>'` is emitted whenever the source
host set a default target, with no test of what the target is and no
comment beside it. (Mark Russell, 2026-11-13)** A host that boots to
`graphical.target` produces an image that does. `default_target` is
recorded as included and has a toggle like any other row. Rule 5 of § 10.2
applies, since `set-default` on a target whose file is not in the image
cannot succeed, in its file and package halves; `set-default` reads no
`[Install]`, so the drop-in half does not apply. The file half: a `Local`
target whose file is excluded is omitted with `UnitFileNotStaged`; a
`Shadowed` target whose shadow is excluded, and a `Vendor` target with a
unit file staged at its name, are omitted with `InstallEvidenceChanged`
and its standard text, which speaks of the `[Install]` section. The
package half: a packaged target whose package is not in the image is
omitted with the presence tiers' reason. A declined target is one
`# Omitted: <target> (<reason>)` line, written after the other declined
commands. The operand is a `TargetUnit`, so no other kind can reach the
line.

### 10.6 Mount, automount and swap advisories

For each `MountUnitFinding`, which is to say each mount, automount or swap
unit the administrator wrote or masked (§ 6.2), comment lines and nothing
else. The first line states the fact. Further lines are added per
condition, computed from `mount_point`:

| Condition | Text added |
|---|---|
| Always | `# Advisory: <unit> is <state> on the source host. inspectah does not enable, disable or mask a mount, automount or swap unit the administrator wrote.` |
| The unit's file is in `staged_writes` | `#   Its unit file is copied into the image. No enable line is written, but the unit still starts if another unit requires it or names its mount point in RequiresMountsFor=, and it replaces a unit of the same name that the image ships.` |
| `mount_point` is under `/usr` | `#   /usr is part of the read-only image on the target. The mount point must already exist in the image, and a mount there hides the image's own content under <path>.` |
| `mount_point` is `/`, `/boot`, `/boot/efi`, `/sysroot` or `/var` | `#   bootc and ostree own this mount point on the target.` |
| `mount_point` is, or is under, `/home`, `/srv`, `/mnt` or `/root` | `#   <path> is a symlink into /var in the RHEL bootc base image. The unit must be renamed for the resolved path (for /srv: var-srv.mount with Where=/var/srv).` |
| `mount_point` is, or is under, `/opt` | `#   /opt is a directory of the read-only image in the RHEL bootc base image, not a link into /var. The unit keeps its name. The mount point must already exist in the image, and a mount there hides the image's own content under <path>.` |
| The unit is `tmp.mount` and its file is in `staged_writes` | `#   The bootc base image starts tmp.mount from a link it ships, so this file replaces the unit that mounts /tmp on the target.` |
| A mount or swap with `nofail` absent | `#   No nofail option. If the unit is started at boot and its device is missing, units that require it fail, and boot can stop in emergency mode.` |
| The state is enabled or masked, and the unit is not a masked `tmp.mount` | `#   To carry it, add after the configuration COPY: RUN systemctl <verb> -- '<unit>'` |

**Layout claims are tied to a base.** The two symlink lists are named
constants beside `VAR_SYMLINKED_TOPLEVELS`
(`crates/pipeline/src/render/selinux.rs:29`). On both RHEL bootc bases,
`/home`, `/mnt`, `/root` and `/srv` are symlinks into `/var`, and `/opt`
and `/usr/local` are real directories (measured). A fedora-bootc or
centos-bootc base creates a different set and is not covered. On a
booted host of either release, a mount under `/opt` or `/usr` whose
directory is in the image mounts over it and hides the image's file, and
the vendor link starts a copied `tmp.mount` by name (measured). A mount
under `/opt` whose directory is missing fails and leaves the host
degraded (measured); the same under `/usr` is inferred from it.

**The unit file itself stays a config entry, included by default, under the
config walk's control. No enable line is written for it, and the advisory
is where the hazards are explained. (Mark Russell, 2026-11-13)** Excluding
the file is the config row's toggle, as for any other file. This is so for
every administrator-written storage unit, a full copy of a vendor unit
included (§ 7.7): one owner, the config row.

A package-owned mount unit gets none of the table: it is a state change
(§ 10.1), a mask on `tmp.mount` aside (§ 10.3).

### 10.7 Order

The unit state section moves from step 2 to directly after the Configuration
Files section, so every `COPY` that can stage a file for a collected unit
precedes it: drop-ins and shadows, and the config tree. This closes the
local-service ordering defect in § 4, and a full shadow's enable now links
to the `/etc` file, which is the one that loads either way (measured).

A unit inspectah generates is the one exception, and it is closed on
itself. Its `COPY` and its enable sit together in its own section
(scheduled tasks before the unit state section, container workloads
after), and § 10.2 withholds the files of a generated unit whose name a
collected unit shares. So no line in the unit state section depends on a
`COPY` that follows it.

The move is also why rules 1 and 4 of § 10.2 exist. With the commands
last, `systemctl mask` meets a file already at its path and fails. A mask
is therefore never a staged file, and never shares a path with one.

### 10.8 An instance enable needs its configuration

An enable for `wg-quick@wg0.service` with no `/etc/wireguard/wg0.conf` in
the image builds, and then the unit fails on every boot. So an instance's
enable is carried only when every path in its `required_config` is in the
export as the host had it. This is rule 6 of § 10.2. For each path, the
plan looks at what the snapshot holds there (for a directory, at or under
it):

| The snapshot holds at the path | Result |
|---|---|
| An entry that is excluded, or that `staged_writes` leaves out or withholds | Omitted: `its configuration <path> is excluded from the export` |
| An entry with `content_captured()` false | Omitted: `its configuration <path> was not captured` |
| An entry, and `snapshot.redactions` holds a finding for the path whose kind is `Inline` or `Excluded` | Omitted: `its configuration <path> holds a redacted value; provision the secret on the deployed host and enable the unit there` |
| An entry that is written, captured and has no such finding | This path passes. |
| Nothing | Omitted: `its configuration <path> is not in the export` |

```rust
/// A template instance whose configuration is not in the export as the
/// source host had it.
InstanceConfigNotStaged { path: String, gap: ConfigGap },

pub enum ConfigGap { Excluded, NotCaptured, Redacted, NotInSnapshot }
```

**Only a file the export carries passes.** A path with no entry has three
causes the snapshot cannot tell apart: the file does not exist on the
source; a package owns it and it is unmodified, which the config inspector
does not record (`config/mod.rs:145-156`); or a package owns it, the
administrator changed it, and `rpm -Va` failed (§ 4), which would put the
package's default behind an enabled instance. So ownership is not
evidence, and no row passes on it. The cost is an instance whose
configuration really is a package's untouched file: its enable is omitted
and the operator adds one line. A package-owned file the administrator
changed, on a host where `rpm -Va` worked, is a config entry and is
treated like an unowned one.

"Redacted" reads the findings the redaction pass already wrote
(`InspectionSnapshot.redactions`, `crates/core/src/snapshot.rs:49`), so the
gate has no secret detection of its own and cannot disagree with it. That
is what makes the WireGuard case come out right. A `PrivateKey =` line
matches the WireGuard pattern
(`crates/pipeline/src/redaction/patterns.rs:132-139`), the match is
swapped for a token, and `scan_content` (`engine.rs:608`) records an
`Inline` finding against the path. The file is still staged, without its
key. Under default redaction `wg-quick@wg0.service` is therefore always
omitted, and the wording says what to do next: the key reaches the host by
a route other than the image, and the enable belongs with that step.

What the gate does not cover: configuration found by a program's own
convention for a template not in the table; configuration outside `/etc`;
a file the listed configuration points to in turn; and a secret the
redaction patterns miss. An instance with an empty `required_config` is
carried on its unit file alone, with the `attention_reason` from § 7.5.
Two cases carry the enable with an `attention_reason` naming the path: a
scan taken with `--no-redaction`
(`crates/cli/src/commands/scan.rs:728-732`), which has no findings and
stages the configuration with its key, as the flag asks; and a `Flagged`
finding, which marks content the pass did not change. Whether the config
walk records `/etc/wireguard/wg0.conf` on a stock host is not verified
(run R2).

**Data under `/var`.** When an instance's enable is carried and its
`data_paths` is not empty, one comment per instance precedes the `RUN`
lines, and the row's `attention_reason` carries the same sentence:

```
# Advisory: dirsrv@example.service keeps data under /var/lib/dirsrv/slapd-example. The image does not carry it, and the carried enable starts the unit on first boot. Put the data in place before that boot, or the unit starts without it.
```

What a unit does when its data is absent is the application's business:
some fail, and some create a fresh, empty state and run. An instance whose
template does not name its `/var` path and is not in the table of § 7.5
gets no advisory.

### 10.9 Hand-made link advisories

For each `ManualLink`, comment lines and nothing else. Most are links made
by hand; the links of a unit whose section holds `UpheldBy=` (§ 7.3) and
the install differences of a unit at its preset (§ 7.4) are here too:

| Cause | Text |
|---|---|
| `StaticUnit` | `# Advisory: <parent>.<relation>/<unit> is a link on the source host that <unit>'s [Install] section there does not make (it names no link: none, only Also=, or a list a drop-in reset to empty). Not carried.` The link may have been made by hand or by an `enable` before the section changed; the text does not guess which. |
| `NotInInstall` | `# Advisory: <parent>.<relation>/<unit> is a link made by hand on the source host (the unit's [Install] section does not name <parent>). Not carried.` |
| `PartialInstall` | `# Advisory: <unit> has some of the links its [Install] section names and not others, so neither enable nor disable reproduces the source host. Not carried. Present: <parent>.<relation>/<unit>.` |
| `Undetermined` | `# Advisory: <parent>.<relation>/<unit> is not carried: the unit's [Install] section cannot be relied on (<cause>).` The cause is the link's recorded `InstallUnknown` reason in words: for `Specifier`, `a specifier inspectah does not expand`; for `RequiredByReset`, `an empty RequiredBy= whose effect was not measured`; for `ForeignName`, `a name this model does not admit`; for `UpheldBy`, `enabling the unit would also make an .upholds link`; for `Unreadable`, `a file of it inspectah could not read as text`. |
| `HostDevice` | `# Advisory: <unit> is tied to a device of the source host. Not carried.` |
| `HostInstallOnly` | `# Advisory: <parent>.<relation>/<unit> is on the source host because <unit>'s files under /etc/systemd/system name it, and the package's unit does not. The image was linked from the package's unit, so it lacks this link. Not carried.` |
| `VendorInstallOnly` | `# Advisory: <parent>.<relation>/<unit> is not on the source host: <unit>'s files under /etc/systemd/system do not name it, and the package's unit does. The image was linked from the package's unit, so it has this link.` Then the removal line, `#   To remove it, add after the configuration COPY: RUN test -L '<path>' && rm -- '<path>'`, where `<path>` is `/etc/systemd/system/<parent>.<relation>/<unit>`. |

**Alias forms. (Mark Russell, 2026-11-15)** For a link with relation
`Alias`, the texts that open with the link's path open instead with
`<parent> is a link to <unit>` in place of `<parent>.<relation>/<unit>
is a link`. The texts that use the link as a subject without saying it is
a link (`Undetermined`, `HostInstallOnly`, `VendorInstallOnly`) use
`<parent>, a link to <unit>,` in its place, and `PartialInstall` ends
`Present: <parent>, a link to <unit>.`

For the first four causes and `HostInstallOnly`, with relation `Wants` or
`Requires`, one more line gives the command:

```
#   To carry it, add after the configuration COPY: RUN systemctl add-wants -- '<parent>' '<unit>'
```

with `add-requires` for a `.requires` link. An `Upholds` link gets no
command line, since `systemctl` has no verb that makes one. Its second
line is:

```
#   An .upholds link starts <unit> while <parent> is active and restarts it whenever it stops, so adding it for a unit that becomes inactive after it exits, such as a oneshot service without RemainAfterExit=yes, produces a restart loop. A RHEL 9 target does not honor it. To add it, put Upholds=<unit> in a drop-in for <parent>.
```

An `Alias` link gets no command line either, since no verb but `enable`
makes one (measured). Its second line, for every cause that gets one, is:

```
#   No systemctl verb but enable makes an alias link. To carry it, make the link in the Containerfile after the configuration COPY, or add Alias=<parent> in a drop-in for <unit> and run systemctl enable on <unit> after that COPY, which also makes every other link its [Install] section and Also= name: check those first.
```

A `HostDevice` link gets no command at all, and both operands are quoted
like every other.

**The removal line never deletes a file.** It tests that the path is a
link before it removes it, so a destination that is not a link fails the
build where the operator sees it. When the export itself stages a file at
the link's path, the copied file replaces the link in the image, and the
removal line gives way to `#   The export copies a file to <path>, which
replaces this link in the image, so no line removes it.` The path is
single-quoted after `--` for the same reason the operands are: an escaped
unit name holds backslashes the shell would otherwise take. `test` takes
no `--`; the path always starts with `/etc`. For a `VendorInstallOnly`
alias the path is `'/etc/systemd/system/<alias>'`.

**Relation `Also`. (Mark Russell, 2026-11-15)** There is no link and no
command. The two texts are:

```
# Advisory: <owner>'s files under /etc/systemd/system name <unit> in Also=, and the package's unit does not. The package install in the image acted on the package's list.
# Advisory: <owner>'s package unit names <unit> in Also=, and its files under /etc/systemd/system do not. The package install in the image acted on the package's list.
```

for `HostInstallOnly` and `VendorInstallOnly`, each followed by `#   Check
<unit> with systemctl is-enabled in the image.` `<owner>` is the link's
`parent`.

**An undetermined section with `/etc` files gets a unit-level
advisory.** Each `UndeterminedInstall` record renders one block, whether
or not the unit has a link, beside any per-link `Undetermined` advisories:

```
# Advisory: <unit> is not carried: its [Install] section on the source host cannot be relied on (<cause>), and these files under /etc/systemd/system change it: <path>, <path>.
#   The image's links for <unit> can differ from the source host's, including links its package install made before these files are copied. Check them with systemctl is-enabled in the image.
```

`<cause>` is the record's reason in the words above. A record with no
install file ends the first line at `(<cause>).` and adds `It assigns
Also=, so the units it names are enabled or disabled with it in the
image, and inspectah could not list them to check.`, or, for
`Unreadable`, `It may assign Also=, so units it names may be enabled or
disabled with it in the image, and inspectah could not read it to
check.` Its second line is the same as above. A preset-enabled
packaged unit whose only relation is `RequiredBy=`, reset by a drop-in,
has no link on the host and so no per-link advisory; this block is what
tells the operator that the image's package install linked it anyway.
There is no command line.

**An excluded install file is said once per unit.** While one of the
`install_files` of a unit's install-difference links or of its
`UndeterminedInstall` record is excluded, none of those blocks is
rendered (§ 7.4), and the unit gets this line in their place:

```
# Omitted: <unit> links (excluded from the export: <path>, <path>; the [Install] the image reads is not the one recorded, so the source host's links for <unit> are not reproduced, and links its package install made may remain in the image. Check them with systemctl is-enabled in the image.)
```

It names every excluded file of the unit, gives no repair line, and is
rendered once per unit however many blocks it replaces.

`add-wants` is offered in place of a literal `ln -s` because of what was
measured for it: it works at build time, links to the `/etc` file when the
unit has a full shadow, exits 1 when the unit is absent where a dangling
link would build and do nothing, and does not read `[Install]`, so it
makes exactly the one link.

## 11. Refine

### 11.1 Data contract

The view response carries, in place of `service_states`, the fields
below. **They describe the single-host view response (`ViewResponse`,
§ 9.1). The `declined` and `withheld` reasons are a requirement of that
view only; the fleet view is left as it is. (Mark Russell, 2026-11-14)**

| Field | Content |
|---|---|
| `unit_states[]` | One per `UnitStateChange`, package-owned mount units, template instances and masked templates included: `unit`, `kind` (derived, lowercase), `instance_of` (the template name, derived, when the unit is an instance), `current_state`, `default_state`, `fragment` (`vendor`, `local`, `shadowed`, `absent`), `unit_file_path` when not vendor, `owning_package`, `also[]` (each `unit` and `links`) and `install_drop_ins[]` (each absent when empty), `required_config[]`, `data_paths[]`, `include`, `locked`, `triage`, `attention_reason`, and `declined` when the plan of § 10.2 does not write the unit's command: how it renders (`omitted`, `fixme` or `advisory`), the `OmissionReason` variant, and its `describe` text. A runtime package updater (§ 10.4) is an ordinary row with `include: false`. |
| `unit_dropins[]` | As `service_dropins` today, with `kind` of the parent. A drop-in whose parent has no state change is still listed, since a unit at its preset has a drop-in and no parent row; the list shows such drop-ins under "Other drop-in overrides", with the note `These overrides have no unit-state decision row.` A full shadow carries its `shadow_type` and rationale here, which is where the "base image updates to this unit are ignored" explanation is shown. |
| `default_target` | `target`, `include`, `locked`, `declined`, or absent. |
| `mount_findings[]` | `unit`, `kind`, `state`, `mount_point`, `fragment` (`local`, `shadowed`, `vendor` for an unowned one, or `absent` for a mask with no unit file), `unit_file_path` (the config row that holds the unit file and its toggle), and `advisories[]`, the same strings § 10.6 renders. No `include`: these rows have no toggle. No mount source: the row links to the config entry, which holds the scrubbed file (§ 6.5). |
| `manual_links[]` | `relation` (`also` included), `parent`, `unit`, `cause` (`host_install_only` and `vendor_install_only` included), and `advisories[]`, the same strings § 10.9 renders. No `include`. An install-difference row whose `install_files` are not all staged stays in the list, as a withheld file's row does, with `not_applied` holding the excluded paths and `advisories[]` holding the unit's `# Omitted:` line of § 10.9 in place of the advisory, so the operator sees why the advisory left the Containerfile, what was not reproduced, and that re-including the file brings it back. `cause` for `undetermined` carries its `reason`, as its wire word (`unreadable` included). The flat list shows the rows through `advisories[]`; words of its own for a cause or a reason are the section design's (§ 11.2). |
| `undetermined_installs[]` | One per `UndeterminedInstall`: `unit`, `reason`, `install_files[]`, and `advisories[]`, the block of § 10.9. No `include`. While one of its `install_files` is excluded, `not_applied` holds the excluded paths and `advisories[]` the unit's `# Omitted:` line, as for `manual_links[]`. |
| `scheduled_tasks` timers | Each collected timer row gains `unit` (`<name>.timer`) and `unit_row` (whether `unit_states` holds it), so the row can link to its control. No `include`: the timer's state is its unit row's. |
| `generated_timers[]` | One per generated-timer name, always present: `name` (the `generated_timer_units` name, the key of `ItemId::GeneratedTimer`), `include`, `locked`, and `cron_job_excluded`, present only while an excluded cron job generates a unit of that name, holding that cron job's path. A generated scheduled row joins to its entry through the row's `generated_timer` field, the same join `declined_generated[]` uses. `include` is true while any record of the name is included (§ 8). `cron_job_excluded` is computed on the projected snapshot with the test the session uses to refuse `SetInclude(true)` on the row, so a row that is excluded and carries it is one the server will not include: the row says which cron job to include instead of offering a toggle that would be refused. |

`declined` is computed by calling `plan_units` on the session's current
snapshot, so the row shows what the export would do with the toggles as
they stand, including a result caused by another row (rules 2, 4, 5 and 7
of § 10.2). It is structured, so a refusal reaches a row whose
`attention_reason` is null, as an imported row's may be. A file
`staged_writes` withholds is shown the same way on the row that offered
it, a config entry or a drop-in found by path, as `withheld` with the
`WithheldWhy` variant and its text. A declined generated unit is shown on
its scheduled or flatpak row, through `declined_generated[]`. An unmanaged
file refused under `/etc/systemd/system` (§ 10.2) carries `withheld` on
its own row in the unmanaged-files list, a field that row gains for it.

A scheduled row is identified by the record it describes, not by its
name. In the sections response, which lists the reference rows (not
`ViewResponse`), a scheduled row's `id` is its kind and its source:

| Row | `id` |
|---|---|
| Cron job | `cron:<path>` |
| Collected timer | `timer:<path>`, the timer file's path; `timer:<name>` when the snapshot records no path |
| At job | `at:<file>` |
| Generated unit | `generated:<index>:<name>`, `<index>` being its place in `generated_timer_units` |

Names are not unique among scheduled rows. A shadowed timer's `/etc` and
vendor descriptions share one; a collected timer can be named
`generated:x.timer`, which a name-based id cannot tell from a generated
row; and two lines of one cron file generate two units of one name and
one source path, so only the index tells them apart. The kind ends at the
first `:` and no kind contains one, and within a kind the source is
unique, so no two rows share an id. Nothing joins on the id or parses it:
a generated row carries `generated_timer`, its name, which keys its
`generated_timers[]` entry, its `declined_generated[]` entry and its
decision. The refine projection's `GenericRefItem` carries `scheduled`,
the record a scheduled row describes, from which the id is built:
`CronJob { path }`, `Timer { path, name }`, `AtJob { file }` or
`GeneratedTimer { index, name }`, and `None` on every other section's
row. The projection's own `id`s, and every other section's row ids, are
unchanged.

Two results of § 10.2 come from a toggle on a different row, and the unit
row has to say so. Excluding a full shadow or an install drop-in
removes the unit's `enable` or `disable`: the unit row's `declined` is
`InstallEvidenceChanged` with the path of that file. And an `Also=`
advisory caused by an install file is `AlsoMember` with
`AlsoWhy::InstallFile` and the path, whether the file is staged or
excluded, since toggling it does not bring the command back.

Operations: `ItemId::Service { unit }` becomes `ItemId::Unit { unit }` and
keeps its cascade to drop-ins and its membership check before it applies.
`ItemId::DefaultTarget` is added; it toggles the recorded target and takes
no target name of its own. **The services bulk toggle does not send
`DefaultTarget`; the target has its own toggle. (Mark Russell,
2026-11-15)** While the view holds a default target, the units bulk menu
reads `Include all except the default target` and `Exclude all except
the default target`, and its announcement names the exception; the
section's decision count includes the target. `ItemId::SystemdTimer` is
removed. `SetInclude`
on a mount finding or a hand-made link does not exist, so there is nothing
to refuse. What the server does with each scheduled operation is § 8's
contract; what single-host refine offers is narrower. A generated-timer
row has its own include control, bound to `ItemId::GeneratedTimer`. It is
shown unless the row is excluded and carries `cron_job_excluded`; then
the row names the cron job to include instead. Cron-job and at-job rows
have no per-row control and stay reference rows. The only route to a
cron-job operation is the services-and-scheduling bulk toggle, which
sends `CronJob` and `GeneratedTimer` operations and nothing for an at job
or for a cron job that generates no unit (§ 8).

The cascade from a unit row to its drop-ins is kept, full shadows
included. It cannot produce an enable on an administrator's storage file:
that file is a config entry, not a drop-in (§ 7.7), and whatever the
toggles say, § 10.2 decides from the files the export stages. The reasons
and advisory strings have one source each, in the pipeline crate.

### 11.2 Recommended section design

**This needs the frontend's own design pass. It is a recommendation.**

One "Systemd units" section replaces "Services". It shows only units that
differ from preset or carry a drop-in, grouped by kind. Services, sockets,
timers and paths get their own groups, with a template instance in the
group of its kind; targets, slices, mount units and the mount and link
advisories sit in one collapsed group, since they are few. Scheduled Tasks
keeps showing the schedule and command for each timer and links to the
timer's unit row. The main alternative, each unit inside the section it
relates to, recreates two places to look for one timer.

The design pass owns grouping and presentation only. Every field and
string key the frontend reads is migrated before it starts (§ 9.1).

### 11.3 Orphan-shadow synthesis is retired

`synthesize_orphan_shadows` (`crates/refine/src/classify.rs:547`) is
removed, with its call in `RefineSession::new`.

It exists to give a full shadow a parent row to toggle when the unit has no
state divergence. It does that by writing a state change the collector
never produced: an enabled unit at its preset becomes a row that says its
state changed, with `default_state: None`. That is a claim about the host
the host did not make, and under this model it would also need a fragment
and a member list invented for it. What it provided is kept, with one
exception:

- **The toggle.** A full shadow is already its own row with its own
  operation, `ItemId::DropIn { path }` (`session.rs:2823`).
- **The explanation.** `shadow_type` and `shadow_rationale` stay on the
  drop-in (§ 11.1). The state-change copies go.
- **The count.** The frontend's shadow count reads `unit_dropins` entries
  whose `shadow_type` is `full_shadow`.
- **The command, which is the exception.** The synthetic row produced a
  `systemctl` line in a refine export, and that line is gone. For a unit
  whose `/etc` files leave `[Install]` as the package wrote it, the image
  already has the same links. For one whose files change it, § 7.4
  decides: an `enable` row when they only add links, an advisory per
  differing link otherwise. The old line ran before the shadow's `COPY`,
  so it linked from the package's unit and did not reproduce the shadow's
  links.

## 12. Fleet

The merge compiles against the new types. Its identity rule and its
narrowing are unchanged.

- `UnitStateChange` keeps identity on the unit name
  (`merge.rs:147-150`) and merges with `merge_items`.
- `inventory` merges by unit name, keeping the most prevalent state, where
  the three string lists were unioned. A tie goes to the state the first
  host in hostname order recorded, and the entry's preset is the one
  recorded by the first host that has the winning state.
- `mount_findings` merge by unit name, and `manual_links` by relation,
  parent and unit. Neither has an include flag to narrow. The two
  install-difference causes of § 7.4 merge the same way, with
  `install_files` the union, so an aggregate keeps the advisory and the
  merged row carries its prevalence in `aggregate`. The aggregate's
  Containerfile renders the advisory; no view shows the prevalence, since
  the fleet view shows no link or record rows. The cause is not part of
  the identity. When two hosts record
  opposing causes for one relation, parent and unit, the merged row keeps
  the cause of the first host in hostname order, and its prevalence
  counts every host that recorded either. The advisory is then wrong for
  the other hosts, which is a fleet limit (§ 17). An additive row is a
  state change like any other and is held to rule 5 on the aggregate.
- `undetermined_installs` merge by unit, with `install_files` the union
  and the reason of the first host in hostname order; the prevalence
  counts every host that recorded one.
- **A mask follows its package on an aggregate too.** Rule 5's package
  half applies to `mask` (§ 10.2), and the merge excludes by prevalence a
  package that not every host has. So an aggregate export that nobody
  refined omits the mask of each such package's unit, each with its
  `# Omitted:` line, until the operator includes the package.
- A template instance merges as the `UnitStateChange` it is. The list
  fields of every state change (`also`, `install_drop_ins`,
  `vendor_drop_ins`, `required_config`, `data_paths`) are the union across
  every host that has the unit. Within `also`, a member two hosts record
  with different `links` becomes `Other`, and a member's `vendor_drop_ins`
  is the union of the names. Neither can produce a carried `Also=`
  command, because of the next rule.
- **A command for a unit with a non-empty `also` list is always an
  advisory in an aggregate (§ 10.2 rule 7).** The owner check of § 7.4
  runs on each host before the merge, and the row it makes merges as any
  state change, so in an aggregate it is an advisory as well, with
  `AlsoWhy::FleetAggregate`. An aggregate is a snapshot
  whose rows carry `aggregate`. The merge keeps one `file_ownership` entry
  per package name (`merge.rs:1332`), so a package-owned, unmodified
  `[Install]` drop-in that one host has can be absent from the aggregate,
  and rule 7 cannot establish that no install file exists. The merge is
  not changed.
- **A limit for rows without `Also=`.** Rule 5 compares a row's install
  drop-ins with the same aggregate, which may lack another host's
  package-owned, unmodified drop-in. If the image's package ships that
  drop-in and it holds `[Install]`, a carried `enable` could link more
  than any source host had. It needs a package that ships an `[Install]`
  drop-in under `/etc`, which none of the surveyed packages does.
- `default_target` takes the most prevalent target, and is excluded when the
  hosts do not all agree, the same rule `narrow_non_universal` applies.
- `SystemdTimer` no longer has an include flag to narrow.

**The exception: a runtime package updater starts excluded.** `merge_items`
calls `set_include(true)` on every representative (`merge.rs:557`, `:577`,
`:658`), and narrowing then excludes only what is not on every host. Left
alone, a `dnf-automatic.timer` that every host has, each one excluded by
the collector, comes out included. After `merge_items`, the units merge
calls `apply_runtime_updater_default` (§ 10.4) on its result, so a fresh
aggregate row for such a unit is excluded with the stated reason. The
operator's include in refine, made afterward on the aggregate, is kept
through export and reload. A per-host include made before the merge is
not carried into the aggregate, as for every row today.

**A merged export is validated at render, not at merge.** The merge joins
rows by name and does not ask whether they agree. A package-owned mount
unit's state change beside another host's administrator-written file of
that name, a mask beside a unit file, and a row recorded from a vendor
unit beside another host's shadow or install drop-in are all kept, and
each is decided by § 10.2 from the files the operator leaves selected.
Teaching the merge to reconcile them would be the aggregation redesign § 3
rules out, and would still miss a snapshot edited or imported after the
merge.

**A fleet refine row shows the unit and its toggle, and no `declined` or
`withheld` reason.** The plan runs on the aggregate export all the same,
and its reasons are in the Containerfile's `# Omitted:`, FIXME and
advisory comments, which is where a fleet user reads them (§ 11.1).

One aggregate defect closes as a side effect: a local timer present on some
hosts was excluded by both of its owners and written anyway, because the
scheduled writer ignored disposition. One existing behavior is recorded and
not changed: identity on the name alone means hosts that disagree about a
unit's state, or about its fragment, collapse to one row. The state is the
most prevalent one, and the fragment and owner are the representative
host's. Rule 5 of § 10.2 then holds the export to that one fragment: a
`Vendor` row beside another host's staged shadow is omitted, and so is a
`Shadowed` row whose shadow is excluded. A staged unit file or `[Install]`
drop-in whose row holds more than one content variant fails the rule too,
because the row may have been recorded from a variant the operator did
not select. What is left is the case with nothing staged: the presence
tiers run on the representative host's owner, and a host where another
package owned the unit is not consulted.

## 13. Verification

Tests the implementation plan must contain. The numbering has no T14.
Three rules apply to all of them:

- **A refusal test names what refused.** A snapshot that must fail to load
  is written at the current schema version and asserts the error that
  names the intended rule.
- **An absence test proves presence first:** the finding, the refine row
  or the marker exists before the test asserts no `systemctl` line.
- **An omission test has a positive control:** the same fixture and route
  with the one condition reversed produces the line.

Phase 1 (the rename):

- **T1. Schema.** Version 27 is refused through
  `InspectionSnapshot::load`, refine's `from_tarball` and aggregate's
  `load_snapshot_from_tarball`, each asserting the version refusal. A
  valid version 28 payload loads through the same three.
- **T3. Consumers.** Through the production handlers: the view response
  holds `unit_states` and `unit_dropins` and neither old key; a config
  entry that a drop-in owns serializes `shadowed_by: "units"`; the section
  host counts and `SectionGroup::for_section` use `units`. In the frontend
  suite: the section renders a non-empty list from `unit_states`, and the
  ownership link resolves for `"units"`.
- **T4. No synthetic rows.** An enabled, preset-matched service with a full
  shadow, through `RefineSession::new`: no `unit_states` row, the shadow
  in `unit_dropins` with its `shadow_type`, the shadow count read from
  there, and toggling the drop-in alone removes and restores the staged
  file.

Phase 2 (every kind):

- **T2. Types.** For each storage kind: a `state_changes` entry with a
  `local`, `shadowed` or `absent` fragment, or `vendor` with a null owner,
  fails to load; `vendor` with an owner loads, and fails when placed in
  `mount_findings`. A `TargetUnit`, tested directly, refuses
  `cockpit.socket` and a packaged `tmp.mount`, and a `.target`
  round-trips. Separately, in `state_changes`: a template name loads with
  `masked` and fails with `enabled`, and so does an `absent` fragment. A
  `file` that is neither the unit's name nor its template's fails. A
  property test shows `classify` is total and stable through
  serialization. A signature test shows `systemctl_lines` takes
  `PlannedUnit` only and that it has no public constructor.
- **T5. Collector, socket.** `MockExecutor` with `cockpit.socket`
  `enabled` against a `disable` preset: one `UnitStateChange` of kind
  `Socket`, with a `Vendor` fragment and the owner taken from the mocked
  filesystem and RPM answers. A local unit and a vendor unit whose owner
  lookup fails get different fragments and render differently.
- **T6. Render, excluded owner.** That snapshot yields the enable. With
  the owner excluded and nothing else changed, it yields
  `# Omitted: cockpit.socket (...)` and no enable.
- **T7. Collector edges.** `dbus-broker.service` is kept and a `dbus-org.`
  alias is dropped. A local enabled unit with no matching preset rule is a
  state change. A `generated` unit and each runtime state produce none.
- **T8. Storage units.** A package-owned mount unit enabled against its
  preset appears in `RUN systemctl enable`; masked, in `mask`. A local
  unit of each storage kind, enabled, and a vendor one whose owner lookup
  fails, yield an advisory and no instruction. A `tmp.mount` with a full
  copy in `/etc` yields the advisory with the `tmp.mount` line, and its
  file is a config entry whose toggle removes and restores the staged
  file.
- **T9. Mount advisory text.** A local `srv.mount`: the `/var` rename
  line. `Where=/opt/vendor`: the `/opt` line and no rename line. A mount
  under `/usr`: the `/usr` line. The unit-file line follows the config
  row's toggle. No instruction names any of them.
- **T10. Extracted fields.** A local mount unit whose `What=` holds a token
  the generic pattern matches, through scan and through a refine export:
  the token is absent from the whole serialized snapshot and from the
  view response. The field guard destructures every § 6.7 type that
  exists so far, and each later phase adds its own.
- **T11. Masks.** A `/dev/null` link and a zero-length file over a vendor
  unit each yield one `Masked` state change with a `Vendor` fragment and
  that file's owner, no drop-in entry and no staged file. A comment-only
  file is a full shadow. A mask with no vendor file has an `Absent`
  fragment and is carried. A masked template is carried under its own
  name. For each storage kind and both mask forms: a package-owned vendor
  file gives a carried mask; an unowned one, or none, gives a mount
  finding with state `Masked` and no instruction.
- **T12. Units the target needs.** A table over the protected names, for
  `disable` and `mask`: the FIXME and no command; `enable` is carried. A
  masked `tmp.mount`: the FIXME; its `enable` and `disable` are carried; a
  masked `dev-hugepages.mount` is carried. A zero-length file for a
  protected unit, from a scan and in an imported snapshot as a drop-in and
  as a config entry: no `mask` and no staged file. The imported cases
  repeat with a `unit` field naming another unit, an interior `.` and a
  doubled separator in the path; `..` is refused. Each refusal appears on
  the view response as a structured reason on the owning row, for a row
  whose `attention_reason` is null.
- **T13. Order and quoting.** A local service with no paired timer: the
  config `COPY` and the enable both exist, and the enable follows. A unit
  named with `\x2d` and one named `-.slice` appear as quoted operands
  after `--`.
- **T15. Unit file.** A local enabled socket whose config entry is
  excluded: no staged file, the `UnitFileNotStaged` line, no enable. Each
  of the two toggles is driven through `RefineSession` on its own and
  shown to change the staged tree or the Containerfile, as
  `cross-inspector-shadow-passes.md` requires. The same holds with no
  package section. Config entries for `worker.service/extra.conf` and
  `worker.service` under `/etc/systemd/system`, in either order: neither
  is staged, both are named in a FIXME, and the enable is omitted; without
  the first, the enable is carried. A failed write fails the export. A
  unit recorded `Shadowed` whose shadow is excluded: no `enable` and no
  `disable`, with `InstallEvidenceChanged`; a `Vendor` row with a unit
  file staged at its name by another row: the same; each returns when the
  export matches the row again, and the unit's refine row shows the reason
  and the path while it is declined.
- **T16. Mask path.** A masked unit and a staged unit file of the same
  name: `MaskPathOccupied`. Excluding the file brings the mask back.
- **T17. Runtime package updaters.** For `dnf-automatic.timer`, each
  variant and `packagekit-offline-update.service`: a state change with
  `include: false` and the stated reason, the `# Omitted:` comment, and no
  enable. Through the production refine loader the row is not locked;
  `SetInclude` yields the enable; the include survives export and reload.
  The two name constants share no entry.
- **T18. Fleet.** (a) One host with a package-owned enabled mount unit and
  one with an administrator's full copy, merged and taken through
  `RefineSession`: while the file's config row is included there is no
  enable and the advisory is present, whatever the unit row's toggle and
  its cascade do; excluding the file yields the enable. (b) Two hosts with
  `dnf-automatic.timer`: the merged row is excluded; an explicit include
  yields the enable. (c) A mask on one host and a local unit file of the
  same name on another, with the package included: T16's result. (d) Two hosts with a full shadow of
  one unit and different content: the unit's command is omitted with
  `InstallEvidenceChanged` whichever variant is selected.

Phase 3 (one control per fact):

- **T19. Disabled local timer stays disabled.** Through the real
  inspectors: a local `.timer` and paired `.service`, the timer
  `disabled`. No enable names the timer. Excluding the timer's config row
  removes the timer file and leaves the service file, whose row is still
  included; excluding that row too leaves neither. No other writer
  restores either file.
- **T20. The timer control is gone.** No view or aggregate response offers
  an include flag on a scheduled timer's descriptive row, the timer's row
  in `unit_states` keeps its flag, and a request naming the removed
  operation is rejected.
- **T21. Every `systemctl` line is planned.** A snapshot with flatpak
  applications, tuned activation, a generated timer and collected state
  changes: every `RUN systemctl` instruction in the rendered Containerfile
  matches a `PlannedUnit`, found by scanning the output. The flatpak
  enable is quoted and follows its own `COPY`. The kernel section writes
  the tuned profile and no `systemctl` line; `tuned.service` enabled
  against a `disable` preset is enabled from the unit state section, and
  omitted when the `tuned` package is excluded. A generated cron timer
  and service named for a collected, enabled vendor timer, through the
  scan export and the refine export: the request is declined with a
  FIXME, neither generated file is in the staged tree, and the vendor
  timer's enable is carried against the package's unit.

- **T35. The scheduled controls apply.** Through `RefineSession` and the
  refine export: `SetInclude(false)` on a generated-timer row removes its
  timer and service from the staged tree and its `enable` from the
  Containerfile, and `SetInclude(true)` restores all three. On the cron
  job it is generated from, `SetInclude(false)` removes the same three and
  `SetInclude(true)` restores them; with the cron job excluded,
  `SetInclude(true)` on the generated-timer row returns `BadRequest`
  naming both rows and leaves the export unchanged. On a cron job that
  generates no unit, `SetInclude` in either direction returns
  `NoopTarget` naming it, and on an at job `BadRequest` naming the file;
  the Containerfile is unchanged in each. Through the production
  handlers, the scheduling bulk toggle sends the cron-job and
  generated-timer operations and no at-job operation and no cron-job
  operation for a cron job that generates nothing, and its exclude and
  include change the staged tree and the plan. Through the production
  handlers, the view's `generated_timers[]` entry follows its row: an
  exclude clears `include` and the enable from the preview, an include
  restores both, and with the cron job excluded the entry carries
  `cron_job_excluded` naming it and the include is refused. Two cron
  sources generating one name, one of them excluded, in either record
  order: one entry with `include` true and `cron_job_excluded` set, the
  preview still carrying the unit; the exclude applies and the include
  after it is refused. In the sections response, a cron job, a shadowed
  timer's two files, a collected timer named `generated:cron-backup.timer`,
  an at job and two generated units of one name from one cron file each
  have the id § 11.1 gives, all distinct, and only the generated rows
  carry `generated_timer`; a timer with no recorded path has
  `timer:<name>`. The UI offers the generated row's toggle and
  no control on a cron-job or at-job row.
- **T36. An inventory name blocks a generated pair.** A package timer at
  its preset, in the inventory with no state change, and a generated cron
  pair of the same name: neither generated file is in the staged tree,
  the request is declined with the FIXME and its rename advice, and no
  `enable` names the unit. The same with only the generated service's
  name in the inventory: both files withheld, and the FIXME names the
  service. The control: a generated pair whose names are in no list is
  staged and enabled.
- **T37. A timer whose trigger is not in the export.** A local enabled
  timer whose paired local service's config row is excluded: no `enable`
  for the timer, the `TimerTriggerNotStaged` line naming the unit, and
  the reason on the timer's refine row; the service row included again:
  the enable returns. The same with `Unit=other.service`. A local
  `report.timer` with a drop-in holding `[Timer] Unit=other.service`:
  the drop-in is in `trigger_drop_ins`; with it and `report.service`
  excluded, `TimerTriggerEvidenceChanged` naming the drop-in and no
  enable, although `other.service` is staged; with it included again,
  the enable returns. A timer whose `Unit=` holds a specifier the
  inspector does not expand: `trigger` is `unresolved`,
  `TimerTriggerUnresolved`, and no enable. An instance timer with
  `Unit=worker@%i.service`: the trigger is the expanded name. No scan
  records a description for an instance timer, which has no timer file
  of its own, so this case is tested at the reader only; a scan-built
  case waits on instance timers being recorded, which the carried
  instances of phase 5 need. These pass
  the trigger check, and are carried when no later rule declines them: a
  timer whose trigger a package in the image owns, one whose trigger is a
  full shadow with the shadow excluded, and one whose trigger is a
  staged generated service. A `disable` and a `mask` on a timer with its
  service excluded pass it too.
- **T38. An unmanaged file under `/etc/systemd/system` is refused.** An
  imported snapshot whose unmanaged-files section offers
  `/etc/systemd/system/x.service`, and one spelled with an interior `.`:
  the file is not in the export, no `COPY` names it, the Containerfile
  holds the FIXME, and its row shows `withheld`. The control: an
  unmanaged file under `/opt` in the same snapshot is extracted and
  copied. A zero-length unmanaged file at a masked unit's path does not
  reach the image.

Phase 4 (the `/etc/systemd/system` scan):

- **T22. Install state.** `backup.service` with
  `WantedBy=multi-user.target`, reported `enabled`, whose only link is in
  `rescue.target.wants/`: with a `disable` preset, no state change, one
  `NotInInstall` link, inventory `Disabled`, no enable; with an `enable`
  preset, a `Disabled` state change. With both links: `Enabled`, and an
  advisory for the extra link. Two `WantedBy=` values and one link:
  `PartialInstall`; with the second link shipped under `/usr/lib`:
  `Enabled`, and no advisory for the vendor link. Only vendor links:
  `Disabled`. An `[Install]` holding only `Also=`: no state change and no
  inventory entry. `getty@tty1.service` with `enable getty@.service` then
  `disable *` in the presets and `DefaultInstance=tty1`: no state change;
  `getty@tty2.service`: `Enabled`. `remote-cryptsetup.target` disabled
  against an `enable` preset: an inventory entry and no state change. The
  first three cases repeat for an instance backed by its template, found
  by its links; the fourth, only vendor links, for an instance with its
  own unit file, since a template-backed instance with no `/etc` link is
  not listed. A partial unit with a third `/etc` link its section does
  not name, and an alias it does not name: those two are `NotInInstall`,
  and only the named link is listed as present in the `PartialInstall`
  advisory.
- **T23. Hand-made links.** A `multi-user.target.wants/backup.service`
  link to a unit with no `[Install]`: the `StaticUnit` link and its refine
  row exist; then the advisory holds the `add-wants` line, no `RUN` names
  the unit, and the row has no include field. Table cases for
  `.requires`, an unexpanded specifier, `systemd-fsck@` (no command line)
  and a hand-made alias, whose second line is the alias line of § 10.9.
  Alias chains, through the scan and both exports: a relay link in a
  chain is a `NotInInstall` alias of the unit the chain ends at, which
  reads `Enabled` and is not disabled; a link to a file outside both unit
  directories is no alias; a chain whose relay cannot be read degrades the
  scan naming the link, and no state and no `disable` follow. A unit with `UpheldBy=multi-user.target` and its
  `.upholds` link, through the units and the config inspectors: no state
  change, no enable, the restart-loop text, and the link's path absent
  from `config.files` and the staged tree.
- **T24. Effective unit.** A concrete `worker@site.service` under `/etc`
  beside a vendor template: a `Local` fragment whose `file` is the
  instance; carried with the template's package excluded and the file
  staged; `UnitFileNotStaged` with the file excluded. An instance with no
  file of its own has the template as `file`. A `WantedBy=` that comes
  only from a drop-in: carried, with the drop-in in `install_drop_ins`;
  with that drop-in excluded, or with a second `[Install]` drop-in staged
  for the unit that the row does not list, `InstallEvidenceChanged` and no
  enable. An `install_drop_ins` path holding `..`: the same. An `/etc`
  drop-in with no `[Install]` section and a vendor drop-in's file name is
  in `install_drop_ins`, and one with a file name of its own is not. A
  vendor drop-in holding `WantedBy=multi-user.target` in `service.d` or
  in `a-.service.d` for `a-b.service`: no link expected from it, and an `/etc` file of any
  name there is not in `install_drop_ins`; the same vendor file in the
  unit's own `.d` and in the template's `.d`: its link is expected, and an
  `/etc` file of its name in that directory hides it; an `/etc` file of
  its name in `service.d` does not. Resets, each with
  `WantedBy=multi-user.target` in the unit: an `/etc` drop-in holding only
  `WantedBy=` gives a `Known` section with nothing expected, so, for a
  local unit, a `multi-user.target.wants/` link on the host is a
  `StaticUnit` link, and with no link the unit has no install state and no
  inventory entry, asserted after the drop-in is shown collected; `WantedBy=` then
  `WantedBy=graphical.target` in one drop-in expects
  `graphical.target.wants` only, and a link there is `Enabled`; with
  `Alias=x-alias.service` in the unit, an empty `Alias=` drops the alias
  from the expected links; with `Also=helper.service` in the unit, an
  empty `Also=` leaves `helper.service` in `also`. `RequiredBy=a.target`
  in the unit and an empty `RequiredBy=` in a drop-in:
  `Undetermined(RequiredByReset)`, no state change, each link advised with
  that reason, and one `UndeterminedInstall` naming the drop-in; an empty
  `RequiredBy=` with no value before it is ignored. An `/etc` drop-in in
  `t@x.service.d` named like a vendor drop-in in `t@.service.d`: the
  vendor one's `[Install]` is not read; a vendor drop-in in
  `t@x.service.d` named like an `/etc` drop-in in `t@.service.d`: the
  `/etc` one is not read; an `/etc` file replaces a vendor file of its
  name in the template's `.d` (§ 7.2). `%I` in `WantedBy=`, in an `Alias=`
  drop-in and in `DefaultInstance=`: `Undetermined(Specifier)`, while `%i`,
  `%p` and `%N` expand on the same instance. A drop-in whose name starts
  with `.` and holds `[Install]`: not recorded, staged as configuration,
  and the enable carried on both routes, with the file excluded too; the
  same file under a visible name: recorded, and excluding it gives
  `InstallEvidenceChanged`. A drop-in in
  `/etc/systemd/system.control/<unit>.d/` or
  `/run/systemd/system.control/<unit>.d/` with the file name of an `/etc`
  install drop-in: the `/etc` file's `[Install]` is not read, and the
  row's `install_drop_ins` holds both paths; with the hiding file under
  `/run`, the command is omitted as `InstallEvidenceChanged` naming it on
  both routes; under `/etc/systemd/system.control`, carried while its
  configuration entry is included and omitted, naming it, while excluded;
  on a fleet with one host of each, omitted naming the `/run` file. In the
  template's `.d` it does not hide an `/etc` file in the instance's. A concrete
  instance file under `/run/systemd/system`, a copy under
  `/usr/local/lib/systemd/system`, and a drop-in with `[Install]` in the
  unit's own `.d` under `/run/systemd/system`: each yields the warning
  naming the path and no state change, and the same unit without the
  extra file yields its row. A concrete instance under
  `/usr/local/lib/systemd/system` with its template under `/etc`: the
  warning. A template under `/run/systemd/system` beside a concrete
  instance under `/usr/lib`: no warning, and the instance file is the
  fragment.
- **T25. `Also=`.** Each case asserts the block or the carried line, and
  the structured reason on the refine row. (a) A packaged unit with three
  members, all `Installed`, and nothing in `/etc` but links: `enable` is
  carried. It stays carried with a `[Service]`-only drop-in on the unit
  and on a member, each under a file name no vendor drop-in has. One
  member `Unlinked`: an advisory holding the `enable` line and a `disable`
  line for that member. (b) `disable` with every
  member `Unlinked`: carried. A member with no `[Install]` and a hand-made
  link: an advisory and no line to add. (c) A member that is an instance
  with its own row, which this phase omits: `RowNotCarried`. (d) A member
  whose row the operator excluded: an advisory; included again: carried.
  (e) A member with no row that is an instance (`enable`) or a `bootc-*`
  unit (`disable`): `NeedsOwnRow`. (f) A member with no unit file:
  carried. (g) A chain `a`, `b`, `c` with `c` `Unlinked`: `enable a` and
  `enable b` are both advisories naming `c`. (h) Install evidence, each
  with the unit otherwise carried. A packaged unit whose vendor file has
  `Also=bootc-x.timer`, recorded `Shadowed` with an empty list because the
  shadow drops the key, shadow excluded: no `enable` and no `disable`
  (rule 5). A member recorded `Inert`, with another host's full shadow of
  it that adds `WantedBy=` staged: `InstallFile`. A member with no row
  whose `/etc` drop-in supplies its `WantedBy=`, drop-in excluded and
  drop-in included: `InstallFile` both times. An instance recorded from a
  vendor template, with a `name@.service` that sets `Also=` staged from
  another row: omitted by rule 5. A staged local unit that sets `Also=`,
  and a local mount unit as a member: `InstallFile`. A staged drop-in
  with `[Install]` and `WantedBy=multi-user.target` in `service.d/` or in
  a dash-prefix directory is not an install file, and the enable is carried; the same
  file in the unit's own `.d`: `InstallFile`. A drop-in whose `[Install]`
  section holds `Also = x`, an assignment continued over two lines, or an
  empty `WantedBy=`: an install file each time. (i) `mask` on a unit with
  members in any state: carried. (j) Fleet, through the real collector
  and merge. Host A: a vendor `bundle.service` enabled against a `disable`
  preset with `Also=helper.timer`, the helper `Inert`. Host B: a newer
  version of one package ships an unmodified
  `/etc/systemd/system/helper.timer.d/10-install.conf` with an
  `[Install]` section, no `bundle` unit, and no row for the helper. Merged, with the
  bundle row included: an advisory with `FleetAggregate` in the plan and
  in the Containerfile. Host A alone: the enable is carried. (k) A vendor
  `bundle.service.d/10-members.conf` with `[Install]` and
  `Also=helper.service`, `bundle.service`'s unit file having its own
  `WantedBy=`, and an `/etc` drop-in of the same file name that
  holds only `[Service]`, so the recorded list is empty: with the `/etc`
  file included the enable is carried and the file is in
  `install_drop_ins`; with it excluded, `InstallEvidenceChanged` and no
  enable. The same `/etc` file on a member of a unit with `Also=`:
  `InstallFile`. (l) `Also=` naming a template is
  recorded as its `DefaultInstance=`, and as `Other` under the template
  name when there is none. (m) Protected members, through the real
  inspectors and both routes: an enabled packaged unit whose unlinked
  members are `bootc-fetch-apply-updates.timer` and `helper.socket`: the
  partial header, the owner's `enable`, `disable` for `helper.socket`, the
  `No disable line for bootc-fetch-apply-updates.timer: the image needs
  this unit.` line, no footer, and no `disable` naming the timer anywhere;
  `helper.socket` alone gets the header and the footer. A member `Other`
  beside one `Unlinked`, in either order: the partial header and no
  footer.
- **T26. Default target.** A `default.target` link to `graphical.target`
  yields `RUN systemctl set-default -- 'graphical.target'` with no
  comment beside it; no link, no line. The row's toggle removes and
  restores the line. A local target whose config row is excluded is
  omitted. A `default_target` naming `cockpit.socket` fails to load.
- **T27. The instance floor.** An enabled instance found through the
  directory scan: a state change and a refine row exist, an `# Omitted:`
  line carries the placeholder reason, and no enable names it. An
  instance linked only under `/run` is not found.

- **T30. A unit at its preset whose `/etc` files change `[Install]`:
  the collector.** Each through the real inspectors, with an `enable`
  preset and the unit enabled. (a) A full shadow with the vendor's
  `[Install]` unchanged, and one with the same values reordered: first the
  shadow is in `drop_ins` and the unit has an `Enabled` inventory entry,
  then there is no row and no advisory. (b) A drop-in that adds
  `WantedBy=extra.target`: an `Enabled` state change with a `Vendor` fragment, the drop-in in
  `install_drop_ins`, and the attention note. (c) A full shadow that adds
  a second `WantedBy=` and one that adds an `Alias=`: an `Enabled` state
  change with a `Shadowed` fragment. (d) A full shadow that replaces
  `WantedBy=multi-user.target` with `graphical.target`: no state change,
  one `HostInstallOnly` link for `graphical.target` and one
  `VendorInstallOnly` link for `multi-user.target`. (e) A vendor unit with
  no `[Install]`, made installable by an `/etc` drop-in: an `Enabled`
  state change. (f) An `/etc` drop-in holding only `[Service]`, with the
  file name of a vendor drop-in that has `WantedBy=extra.target`: no state
  change and one `VendorInstallOnly` link. (g) A shadow that drops
  `Also=helper.service`: no state change and one `ManualLink` with
  relation `Also` and cause `VendorInstallOnly`. (h) The same shadows
  under a `disable` preset: the ordinary row and no advisory. (i) A vendor
  unit whose own `[Install]` holds a specifier the reader does not expand,
  shadowed by a file with a plain `WantedBy=`: no state change, each of
  the unit's links is a `ManualLink` with cause `Undetermined`, and one
  `UndeterminedInstall` naming the shadow. The package's section with
  `UpheldBy=` under a shadow with no `[Install]` and no link, and under a
  shadow naming `multi-user.target` with that link: no state change, no
  inventory entry, the record with the shadow, its block on both routes
  and on refine's row, and the per-link finding exactly when linked; a
  storage unit in that case gets no mount finding. (j) The
  replacing shadow of (d) on a host that also has a hand-made
  `multi-user.target.wants/` link: the `NotInInstall` link of § 7.3 and
  no `VendorInstallOnly` link for it. Resets, each over a vendor
  `WantedBy=multi-user.target`: (k) an `/etc` drop-in holding only
  `WantedBy=`, with no link on the host: no state change and one
  `VendorInstallOnly` link for `multi-user.target`; the same with the
  package's link still on the host: no `ManualLink` of any cause for that
  link, asserted after the drop-in is shown collected; a full shadow with no `[Install]` section and
  no link on the host: as the first. (l) A drop-in holding `WantedBy=`
  then `WantedBy=multi-user.target extra.target`: an `Enabled` state
  change, as (b); one holding `WantedBy=` then
  `WantedBy=graphical.target`: as (d). (m) A vendor
  `Alias=x-alias.service` and an `/etc` drop-in holding `Alias=`: no state
  change and a `VendorInstallOnly` link with relation `Alias`. (n) A
  vendor `Also=helper.service` and an `/etc` drop-in holding only
  `Also=`: the drop-in in `drop_ins` and the unit's `Enabled` inventory
  entry, then no row and no advisory. Controls: (o) a vendor unit with no
  `[Install]` and an `/etc` drop-in holding only `[Service]`, no link:
  the drop-in collected, then no state change, no `ManualLink`, no
  `UndeterminedInstall` and no `systemctl` line. (p) The drop-in of (l)'s
  second case, `WantedBy=` then `WantedBy=graphical.target`, on a host
  with no `graphical.target.wants/` link: a `Disabled` state change, its
  `disable` carried, and no install-difference link.
- **T31. The additive row, rendered.** For T30 (b), (c), (e) and the
  first case of (l): the unit is in `RUN systemctl enable` after the
  configuration `COPY`. With the drop-in of (b) or the shadow of (c)
  excluded: `InstallEvidenceChanged` and no enable. The unit of (a) is in
  no `systemctl` line.
- **T32. The advisories, rendered.** For T30 (d): the `HostInstallOnly`
  block with its `add-wants` line, the `VendorInstallOnly` block with
  `RUN test -L '<path>' && rm -- '<path>'`, a unit named with `\x2d` keeping its backslashes
  inside the quotes, no `systemctl` instruction naming the unit, and two refine
  rows with no include field. For (f) and the first case of (k): the
  removal line, and no `systemctl` instruction naming the unit. For (g):
  the `Also` text and no command line. A `VendorInstallOnly` alias names
  `'/etc/systemd/system/<alias>'`. A `VendorInstallOnly` alias whose path
  the export stages as a regular file: the line saying the copied file
  replaces the link, and no `rm` anywhere. With the replacing shadow of (d)
  excluded: no `rm` line, no `add-wants` line, no command for the unit,
  and the two refine rows marked `not_applied` with the shadow's path;
  included again, both blocks return.

- **T33. The machine-id is never staged.** Through the scan export and
  through the refine export, a snapshot that offers `/etc/machine-id`
  through a config entry, and separately one that offers it through a
  drop-in row's path, each with the path spelled relative
  (`etc/machine-id`), with a doubled separator, with an interior `.`, and
  with a trailing separator: nothing is staged there, the Containerfile
  names it in a FIXME, and each offering row shows `withheld` with
  `MachineId`, two rows that offer the one destination included. The
  control: `/etc/machine-info` offered in each of the same ways is
  staged. An imported unmanaged file at `/etc/machine-id`: not bundled,
  not extracted, no `COPY`, and withheld as `MachineId`. A scan of a host
  never records the path. The README of the scan export holds the two
  first-boot sentences; the refine export writes no README.
- **T34. A mask follows its package.** A masked vendor unit whose owning
  package is excluded and is not in the baseline: no `mask`, and an
  `# Omitted:` line with the package reason naming the unit. The same
  mask with the package included: carried. With the package excluded and
  in the baseline: carried. With no known owner, and with an `Absent`
  fragment: carried. A masked protected unit whose package is excluded:
  the FIXME of rule 3, not the package reason. A mask whose path a staged
  file occupies, package excluded: `MaskPathOccupied`.
- **T39. The preset for an instance.** Through the real inspectors.
  Each case asserts first that the instance is collected, with its
  inventory entry and the preset the lookup gave, then the state change
  or its absence. Templates have `DefaultInstance=one` unless a case says
  otherwise. An instance with its own unit file `t@two.service`: under
  `enable t@two.service` then `disable *`, enabled has no state change;
  the regression, the administrator disabled it, is a `Disabled` state
  change rendering `disable`. Under `enable t@*` then `disable *`, enabled
  has no state change. Under `enable t@.service two three` then `disable
  *`, enabled has no state change. Under a bare `enable t@.service` then
  `disable *`, enabled is an `Enabled` state change, the bare rule not
  read for it. An instance backed only by its template: under `enable
  t@.service` then `disable *`, `t@one` enabled has no state change and
  `t@two` enabled is an `Enabled` state change; with no
  `DefaultInstance=`, `t@one` enabled is an `Enabled` state change. Under
  `enable t@.service two three` then `disable *`, `t@two` enabled has no
  state change and `t@one` enabled is `Enabled`. The regression: under
  `enable t@two.service` then `disable *`, `t@two` enabled is an `Enabled`
  state change rendering `enable`. Under `disable t@.service`, `t@one`
  enabled is `Enabled`. Under a bare `enable t@.service` with no rule after
  it and no link for `t@one` on the host, `t@one` is found from the presets
  (§ 7.5) and is a `Disabled` state change rendering `disable`; the same
  with a list-form rule finds each listed instance; under `disable
  t@.service` no instance is derived. A matching `Unknown` line,
  `frobnicate t@.service`, before `enable t@.service`: `t@one` enabled has
  an inventory entry with no preset, no state change, and the structured
  warning naming the line, its file and `t@one`. No rule matching the
  template, no catch-all included: an inventory entry with no preset and
  no state change. `getty@tty1` and `getty@tty2`, with RHEL 9.8's rules
  (`enable getty@tty1.service` in `90-default.preset`, `enable
  getty@.service` in `90-systemd.preset`, `disable *` in
  `99-default-disable.preset`) and with RHEL 10.2's (the same without the
  first), on each: `tty1` enabled, no state change; `tty1` with no link,
  found from the presets and a `Disabled` state change rendering
  `disable`; `tty2` enabled, an `Enabled` state change. The parser:
  `enable t@.service two three` is `EnableInstances` with `two` and
  `three`; `enable t@two.service`, `enable t@.service` and `disable
  t@.service x` are `Plain`; a line whose first word is neither verb is
  `Unknown` with its file and line. An owner's `Also=`: a vendor
  `p.service` with `WantedBy=multi-user.target` and
  `Also=t@two.service`, enabled under `enable p.service` with its link,
  the package shipping only `t@.service` (`WantedBy=multi-user.target`)
  under `disable *`, and no link for `t@two`: `t@two` is collected with
  inventory state `Disabled` and no row of its own; `p.service` is an
  `Enabled` state change with the attention note, `t@two` in its `also`
  as `Unlinked`; the plan declines it with `AlsoWhy::LinksDiffer`; the
  Containerfile holds rule 7's advisory naming `t@two.service`, with
  `RUN systemctl enable -- 'p.service'` and `RUN systemctl disable --
  't@two.service'` as the lines to add, and no `systemctl` instruction
  names either unit. The control: the same host with
  `multi-user.target.wants/t@two.service` linked: `t@two` is `Installed`,
  `p.service` has no row and the Containerfile names neither. The same
  for a member that is not an instance: a vendor `m.service` with
  `Also=m.socket`, enabled under `enable m.service` with its link, and
  `m.socket` (`WantedBy=sockets.target`) under `disable *` with no link:
  `m.socket` is `Disabled` with no row, `m.service` an `Enabled` row with
  `m.socket` `Unlinked`, rule 7's advisory with `RUN systemctl enable --
  'm.service'` and `RUN systemctl disable -- 'm.socket'`, and no
  `systemctl` instruction for either. Its stock control, `m.socket`
  linked as the package left it: `Installed`, no owner row, and only
  `m.socket`'s own `enable` row against its `disable` preset (§ 7.4's
  second class). The `Inert` control: `Also=m-helper.service`, a member
  with no `[Install]` and no link: `Inert`, no owner row, no advisory.
- **T40. An undetermined section with `/etc` files and no link.** A
  vendor unit enabled by an `enable` preset whose only relation is
  `RequiredBy=a.target`, with an `/etc` drop-in holding `RequiredBy=` and
  no link on the host, through the real inspectors: the drop-in is
  collected and the section is `Undetermined(RequiredByReset)`; no state
  change, no `ManualLink`, one `UndeterminedInstall` naming the unit, the
  reason and the drop-in; the Containerfile holds the § 10.9 block naming
  all three and no `systemctl` line for the unit; the refine
  `undetermined_installs[]` row holds the same block. The same unit with a
  `UpheldBy=` drop-in on a RHEL 10 fixture: the same, with reason
  `UpheldBy`. A vendor unit under an `enable` preset whose own `[Install]`
  holds `WantedBy=multi-user.target` and `UpheldBy=multi-user.target`, no
  `/etc` install file and no link on the host: a `Disabled` state change
  rendering `disable`, and no `UndeterminedInstall`; with one link on the
  host: no state change and the per-link advisories. The same unit
  with `Also=helper.service` added and no link: no state change, and one
  `UndeterminedInstall` with no files, rendering the block without the
  file clause; with `Also=helper.service` and one link: the per-link
  advisories and the same record. With the drop-in excluded: the unit's `# Omitted:` line
  naming it, and the row's `not_applied` with its path. The control: the
  vendor unit with no `/etc` file has no record.
- **T41. A partly excluded set of install files.** A vendor unit enabled
  by an `enable` preset with `WantedBy=multi-user.target`, an `/etc`
  `10-reset.conf` holding `WantedBy=` and `20-add.conf` holding
  `WantedBy=graphical.target`, and a `graphical.target.wants/` link only:
  the `HostInstallOnly` and `VendorInstallOnly` blocks with both files
  staged. With only `20-add.conf` excluded through `RefineSession`: no
  `add-wants` or `rm` line, one `# Omitted:` line naming the unit and
  `20-add.conf`, and both refine rows `not_applied` with that path and the
  same line in `advisories[]`. Included again, both blocks return and the
  line goes.
- **T42. A file that cannot be read as text.** Through the real
  inspectors: an `/etc` unit file and a vendor unit file that are not
  text, each listed `enabled` with its links: no state change, an
  `Unreadable` record each (the `/etc` one listing its file), each link
  `Undetermined(Unreadable)`, and one warning per unit naming the file in
  its message and `paths`; a mask over a non-text vendor file is still a
  masked state change. An `/etc` install drop-in that is not text, listed
  `enabled` under `disable *` and listed `disabled` with no link under
  `enable *`: no row either way, one record listing the drop-in, and the
  warning; the readable drop-in gives the `enable` row and the `disable`
  row. A vendor unit file that is not text, listed `enabled` against
  `disable`, through both exports: the block with the `Unreadable` words
  and the `It may assign Also=` sentence, and no `systemctl` line; the
  readable file is enabled on both routes. A full shadow that is not text:
  one record listing the shadow and no state change; the export does not
  stage the shadow, so it renders the unit's `# Omitted:` line in place
  of the block. A storage unit with an
  undetermined section (`Unreadable`, `Specifier` or `UpheldBy`) and an
  `/etc` link: the record, its block, and no mount finding or "is not
  enabled" text; the same unit with a readable section and no link is
  still recorded as not enabled.

Phase 5 (carried instances):

- **T28. Carried instance.** A
  `multi-user.target.wants/openvpn-client@site.service` link, a vendor
  template that names `multi-user.target`, and an included
  `/etc/openvpn/client/site.conf` with no redaction finding, through the
  real inspectors: one `UnitStateChange` named for the instance with the
  template's owner, and the instance in `RUN systemctl enable`. An
  instance linked from two parents is one state change.
  `dirsrv@example.service` with an existing data directory has the data
  advisory beside a carried enable, and a template that names
  `/var/run/worker/%i.pid` has an empty `data_paths`.
- **T29. Instance whose configuration is not in the export.** Each case
  asserts the matching `# Omitted:` wording and no enable: the config
  entry excluded through `RefineSession` (and the enable returning when it
  is included again); content not captured; `wg-quick@wg0.service` with a
  `PrivateKey =` line through the real redaction pass, which also asserts
  the staged file holds no key; the file missing on the source; a
  package-owned, unmodified file; and a package-owned file the
  administrator changed on a host where `rpm -Va` fails. Positive
  controls: a clean unowned file, and a package-owned file the
  administrator changed with `rpm -Va` working. Under `--no-redaction`
  the WireGuard instance is carried with the attention note, and a
  `Flagged` finding leaves the enable carried with its note. A unit whose
  `Also=` names the instance is an advisory while the instance is omitted
  and carried once its configuration is clean.

**Real-host evidence.** "Measured" in this spec means the `Measured,
2026-11-13` subsection of
`process-docs/skills/real-host-validation-runs.md`, and for the boot and
`cockpit.socket` results its sections "Measured, 2026-11-14:
cockpit.socket end to end" and "Measured, 2026-11-14: unit state at first
boot", and for run R5 its section "Measured, 2026-11-14: the small
systemd facts phase 4 relies on": RHEL 9.8 (systemd 252)
and RHEL 10.2 (systemd 257) hosts, and builds from `rhel9/rhel-bootc:9.8`
and `rhel10/rhel-bootc:10.2`. A few facts quoted here were measured the
same day and are not in that record yet: the survey results quoted in § 5,
§ 7.4 and § 17 (what differed from preset, the one enabled instance,
the rows a package install makes through `Also=`, that the packaged units
with `Also=` have nothing under `/etc` but links), that a package install
in the base image links a preset-enabled unit, the `enable` then
`disable` pair for `cups`, the contents of the four template units in §
7.5, the upheld-unit restart loop, and `/tmp` on a package-mode host with
`tmp.mount` masked. Phase 2 adds them.

The runs below are still open, wholly or in part (§ 15 says when). **Do
not use the CentOS9 or fedora44 VMs for them.**

- **R1. `cockpit.socket` end to end.** On a RHEL 9 and a RHEL 10 host:
  record the socket's preset on the host and `systemctl is-enabled
  cockpit.socket` in the unmodified base image. Switch the socket against
  its preset on the host, scan, confirm the Containerfile holds the
  line, build, and confirm the image reads the host's state. Run once
  with the phase 2 build: the built image read the host's state on both
  releases. One gap remains. RHEL 10 presets the socket `enable`, so the
  host there had it disabled against its preset, and the
  enabled-against-preset case was exercised on RHEL 9 only. The run
  repeats at the branch tip.
- **R2. With the branch build.** A local service with no paired timer
  builds (the § 4 ordering defect, before and after). A full shadow,
  built and booted. An enabled `wg-quick@wg0` whose configuration holds a
  key: whether the config walk recorded the file, and that the enable is
  omitted. An instance with clean configuration is active after boot. A
  libvirt service with one socket disabled: the rendered advisory against
  the host's links. A package that owns an unmodified
  `/etc/systemd/system/service.d/*.conf` holding an `[Install]` section,
  scanned on a single host: whether the snapshot holds any entry for that
  path, in `config.files`, in `units.drop_ins` or in `file_ownership`.
- **R3. Boot checks on the install routes read from source.** Settled by
  measurement: first boot with a zero-length machine-id applies no
  presets and keeps rendered `enable`, `disable` and `mask`, through
  `to-existing-root`; the `uninitialized` case and what it undoes on each
  release; `/tmp` with and without a masked `tmp.mount` on both; a
  `disable` against a vendor `.wants` link; a `graphical.target` default
  with no display manager, which both bases already have and which boots
  with no failed unit; mounts under `/opt` and `/usr`; and a copied
  `tmp.mount`. Still open, before the merge: a boot of each route whose
  handling of the machine-id was only read from source. An image-builder
  disk image, the installer ISO route, and a plain `ostreecontainer`
  kickstart. For each, the size of the deployed `/etc/machine-id` before
  first boot, and the journal's first-boot line. A route that cannot be
  set up, or is not run, stays assessed from source and unbooted: it is
  recorded as such and is never a passed boot result. `to-disk` is in
  that state now: it was measured to deploy a zero-length file on RHEL
  10.2 only, and the disk was not booted. Both bases were booted through
  `to-existing-root`.
- **R4. Stock-host false positives.** With the branch build, scan a stock
  host of each release before a drifted one, and review every row the new
  kinds add, every link advisory, every instance and every `Also=`
  advisory. Known before the run: `remote-cryptsetup.target` on RHEL 9,
  and the rows a package install makes through `Also=` (§ 7.4). List
  every advisory the owner check of § 7.4 ("An owner at its preset whose
  member is off") produces on the stock host of each release, each with
  its owner and member, and classify each one that recurs: the 2026-11-13
  survey did not inventory every stock member's links, so this run is
  where a stock owner advisory is found. Keep the classified rows and
  their reasons as the evidence.
- **R5. Small systemd facts, no inspectah build.** Run on 2026-11-14 on
  both releases, and each answer is written where the spec relies on it:
  the reset of an empty `[Install]` key (§ 7.2), the drop-in directories
  `enable` reads `[Install]` from and same-name hiding (§ 7.2, § 10.2),
  the search-path order and a concrete instance over its template (§ 7.1,
  § 7.2), `disable` with an absent `Also=` member (§ 10.2 rule 7),
  `systemctl preset` on templates and instances (§ 7.4), `systemd-fsck@`
  (§ 7.5), the additive sequence (§ 7.4), a timer whose service is not in
  the image (§ 8), and `tuned` in each base (§ 10.2). Still open, none of
  it needing an inspectah build, to run with the next real-host run and
  at the latest with the branch-tip runs:
  - `systemd-cryptsetup@` links on a host with an encrypted volume;
    neither guest had one, and none was fabricated.
  - An empty `RequiredBy=` after a `RequiredBy=` value, and an empty
    `UpheldBy=` on RHEL 10, in an `/etc` drop-in: whether `enable`
    clears the value. Until then the reader declines the first (§ 7.2)
    and every `UpheldBy=` section stays undetermined.
  - Both bases' stock preset files searched for a list-form `enable`
    line, `enable <template> <instance>...`, recorded with the result,
    so § 7.4 can say whether a stock host meets the list form.
  - The additive sequence for a unit with an existing alias link, which
    the measured unit did not have.
  - Hiding across the unit's own and the template's `.d`, both
    placements: an `/etc` drop-in with no `[Install]` in `t@x.service.d`
    named like a vendor drop-in with `WantedBy=multi-user.target` in
    `t@.service.d`, then `enable t@x.service`; and a vendor drop-in with
    no `[Install]` in `t@x.service.d` named like an `/etc` one with
    `WantedBy=multi-user.target` in `t@.service.d`. § 7.2, read from
    systemd's source, says the instance's file hides the template's in
    both.
  - A drop-in above `/etc` hiding an `/etc` drop-in: a `system.control`
    file from `systemctl set-property` with the file name of an `/etc`
    drop-in that holds `WantedBy=`, then `enable`, which § 7.2 says links
    nothing from the hidden file; the same against a vendor drop-in, and
    across the instance's and the template's `.d`. Then `enable` in an
    image without the `system.control` file, which reads the hidden
    `[Install]`.
  - `%I` in an `[Install]` value, such as `WantedBy=%I.target` on an
    instance whose name holds an escape, and what `enable` links; § 7.2
    declines it because systemd's install-time expansion has no `%I`.
  - A preset line that starts with `;`, and a drop-in whose name starts
    with `.` holding `[Install]`: that the first is a comment and the
    second is not read by `enable`.
  - For an instance with its own unit file (§ 7.4, first case), whether a
    rule that does not count for it lets the lookup go on: with a unit
    file `t@three.service`, `enable t@.service two`, then `enable
    t@three.service`, then `disable *`, and `systemctl preset
    t@three.service`; the same with a bare `enable t@.service` before
    `enable t@three.service`. A link means the lookup went on, as § 7.4
    assumes.
  - On both bases, `rpm -q --scripts` for every package that owns a
    template unit, recording which names each scriptlet passes to
    `systemctl preset`: a template, an instance, or both. § 7.4 takes the
    name from what the package ships, and a scriptlet that presets a
    template-backed instance by name, or a concrete instance's template,
    is where it can be wrong.

## 14. Backlog items

Each item was read in full before the design, and its claims checked
against `70aa8996`.

| Item | Result | Still right? |
|---|---|---|
| `inspectah-socket-units-are-never-collected` | Closed by phase 2 and run R1. | Yes. All four cited sites confirmed. |
| `inspectah-local-timer-has-two-controls-and-neither-works` | Closed by phase 3. Control 1 goes with the second writer; control 2 is removed, and its cause is in § 4. | Yes, with stale line numbers: the writer is now `configtree.rs:440-449`. |
| `inspectah-timer-enable-lines-unquoted-and-trust-empty-units` | Closed by phases 2 (quoting, file check) and 3 (single writer). | Partly stale. Since `2c2e52aa` the names pass `is_shell_operand`. They are still bare, with no `--`, and glob characters pass. Items 2 and 3 stand. |
| `inspectah-write-the-collector-overlap-contract` | Partly covered. § 8 and § 7.7 are the ownership table for `/etc/systemd/system`, and the `*.wants/*` exclusion is answered and extended to `*.upholds/*`. Phase 4 writes this part into `process-docs/` and fixes `docs/reference/inspector-coverage.md`. Remaining: the general rules document, and the non-systemd claimants (quadlets, repo files, GPG keys). | Yes. |
| `inspectah-masked-units-are-recorded-as-full-shadows` | Closed by phase 2 (§ 7.7), for both mask forms. The alias question is answered: an alias `[Install]` names produces nothing, and a hand-made one is advised (§ 7.3). | Yes. |
| `inspectah-full-shadow-wants-link-points-at-the-vendor-path` | Partly covered. The order change in § 10.7 changes what the enable resolves, and the item is the reason § 10.9 offers `add-wants` and not a literal link. The item's question is measured: the `/etc` file loads whatever the link names, with systemd as PID 1 in a container built from each base. A booted host is run R2. | Yes. |
| `inspectah-srv-mount-units-must-target-var-srv` | Partly covered. The advisory in § 10.6 names the problem and the rename, for the four paths measured as links into `/var` on both bases. Remaining: the unit file is still carried as config by default, which is decided; there is no automatic rewrite; fstab lines and the audit report are untouched. | Yes. |
| `inspectah-etc-unit-files-accessor` | Out of scope. Its blocker moves: after phase 4 the `/etc/systemd/system` claimants are config and units, which changes the accessor's signature. | Yes. |
| `inspectah-var-unit-grep-matches-comments-and-longer-names`, `inspectah-etc-systemd-user-has-no-symlink-rule` | Out of scope: the `/var` backing search in `storage.rs`, and user units (§ 3). | The first yes, as narrowed in the item; the second not re-measured. |
| `inspectah-redaction-pseudo-paths-miss-systemd-comment-rule`, `inspectah-unredacted-export-carries-no-marker`, `inspectah-redaction-token-eats-the-key-it-replaces` | Out of scope, with three effects: the `timer:` pseudo-path is left for generated units only; under `--no-redaction` an instance is carried with its key and the row's attention note is the only sign (§ 10.8); and a redacted instance configuration is staged with a token in place of the key line, which is why § 10.8 omits the enable. | Not re-checked. |

## 15. Phases

**Phases 1 to 5 land on one feature branch, and the branch merges to
`main` once, after phase 5, carrying the single schema bump. Units are
shown in refine's existing flat list when it merges. The branch does not
merge before phase 5. Phase 6 waits on its frontend design pass and
follows the merge as its own change. (Mark Russell, 2026-11-13)** Each
phase on the branch is still its own unit of work: it compiles, passes the workspace gates
at its last commit, and is reviewed alone before the next phase starts.
Phase 1 sets schema 28 (§ 9). `main` is merged into the branch between
phases, so the final merge is not the first time the two meet.

**Phase 1: the rename. Size M, wide and mechanical.**
`UnitName` (§ 6.1), `UnitState`, the single inventory list (§ 6.4); the
section rename and schema 28 (§ 9); every consumer in § 9.1, the frontend's
fields, ids and test mocks included; and the retirement of orphan-shadow
synthesis with the two shadow fields on a state change (§ 11.3). A state
change keeps `owning_package: Option<String>` with today's meaning and no
fragment is introduced, so nothing has to be discovered that the collector
does not already know. The collector still asks for services only. The
one intended behavior change is in refine: a preset-matched unit with a
full shadow no longer gets a synthetic state row. Tests T1, T3 and T4.

**Phase 2: every kind. Size L, at the upper end.**
The fragment and the carried and advisory unit types (§ 6.2, § 6.3 without
`also` and the instance fields), filled by the two-directory fragment
resolution and the mask form test of § 7.2; `UnitsInspector` over every
kind from `list-unit-files` (§ 7.1 sources 1 and 2, § 7.4), with the
`dbus-org.` narrowing; masks, drop-ins and shadows for every kind with one
owner per path (§ 7.7); mount findings and their advisories (§ 10.6); the
redaction ownership of every extracted field (§ 6.7); quoting and `--` at
every `systemctl` site; the unit state section moved after the config
`COPY` (§ 10.7); `staged_writes` and `plan_units` with rules 1 to 4 and the
fragment half of rule 5 (§ 10.2), with and without package data; the guard for units the target
needs (§ 10.3); the runtime-updater default, the normalizer change and the
merge exception (§ 10.4, § 12); and the structured `declined` and
`withheld` reasons on refine rows (§ 11.1). Refine shows every kind in the
existing flat list. Tests T2 and T5 to T18. Local timers keep today's
deferral to the scheduled section.

Known limit of phases 1 to 3: `enabled` is still taken on
`list-unit-files`'s word and `Also=` is not read, which is today's
behavior for services extended to the new kinds. Phase 4 closes both,
before the merge. Neither may reach `main`: a hand-made link to a unit with
no `[Install]` makes it read `enabled` against a `disable` preset, and the
`enable` phases 1 to 3 would write for it exits 0 in the image and links
nothing (measured), a silently wrong line, not a missing one.

**Phase 3: one control per fact. Size M.**
Every change in § 8: the second writer and the local-timer enable removed,
`SystemdTimer` reduced to a description, `ItemId::SystemdTimer` and the
aggregate timer control removed. And the rest of § 10.2's "what the plan
takes": the scheduled section's generated units and the flatpak
provisioning service become requests to `plan_units`, their files join
`staged_writes` and are withheld on a name collision, and the tuned enable
is removed, so that after this phase every `systemctl` line in the
Containerfile is a planned one. Four things follow from having one writer
and land with it: the scheduled rows' operations apply or are refused
(§ 8), an inventory name blocks a generated pair (§ 10.2), a timer's
`enable` needs its trigger (§ 8, § 10.2 rule 5), and an unmanaged file
under `/etc/systemd/system` is refused (§ 10.2). Tests T19 to T21 and T35
to T38.

**Phase 4: the `/etc/systemd/system` scan. Size L, at the upper end.**
The `[Install]` reader with `install_drop_ins`, `vendor_drop_ins` and the
drop-in half of rule 5 (the fragment half is phase 2), and the search-path
test (§ 7.1, § 7.2); the link scan with vendor links, the install state,
the instance preset lookup and the stock-state list (§ 7.3, § 7.4); the
`also` list and rule 7 of § 10.2, with the check of an owner at its
preset whose member is off, for every member (§ 7.4); `ManualLink` and
its advisories (§ 10.9); the vendor-side read and the install comparison for a unit at
its preset (§ 7.2, § 7.4); the `.upholds` exclusion in the config walk;
the default target (§ 7.6, § 10.5); and the overlap contract text. Enabled
and masked instances are collected as state changes and every one is
omitted at render with a placeholder reason, `template instances are not
carried yet`, on an `# Omitted:` line and a refine row. Two rules that do not
depend on the scan land here as well, since the earlier phases are done:
the machine-id guard in `staged_writes` and its README sentences (§ 10.2,
§ 10.1), and the package check on `mask` (§ 10.2 rule 5). Tests T22 to
T27, T30 to T34 and T39 to T42.

**Phase 5: carried instances. Size S to M.**
`required_config` and `data_paths` with their derivation and fixed tables
(§ 7.5); the instances derived from the presets and from enabled units'
`Also=`, so a disabled template-backed instance is found (§ 7.1 source
6, § 7.5); rule 6 of
§ 10.2 and `InstanceConfigNotStaged`; the attention
notes for unredacted and flagged configuration; and the data advisory
(§ 10.8). Before instances are carried, the scheduled inspector records
an instance timer's description and reads the template's
`<prefix>@.timer.d`, so an enabled instance timer's trigger is known and
rule 5 does not omit it as `TimerTriggerUnresolved` (§ 8). The
placeholder omission from phase 4 is removed, and a masked instance is
carried as `systemctl mask` (§ 7.5). Tests T28, T29, and T39's derived
instances and carried commands.

**The floor for instances** is the end of phase 4: every enabled or
masked instance named in an `# Omitted:` line, none carried. A disabled instance backed
only by its template is not found until phase 5 derives it from the
presets, so at the floor it is silent: nothing names it, and the image's
package install links it again. It is a state of the branch
between phases, not a point at which it merges.

**Phase 6: the Systemd units section. Size M, frontend.** § 11.2 after its
own design pass. Grouping and presentation only. It is not on the feature
branch: it starts once the design pass is done and lands on `main` after
the merge, as its own change. It reads the view response of § 11.1, which
phases 1 to 5 complete, and changes no serialized snapshot type, so it
needs no schema version of its own. Its design pass is where showing the
plan's reasons in the fleet view would be taken up (§ 17). That is not a
requirement of this phase.

**Real-host runs.** What is left (§ 13) happens on the branch, before the
merge, and results go into `process-docs/skills/real-host-validation-runs.md`
there, so the merge carries the evidence with the code.

| When | Runs | Why then |
|---|---|---|
| Before phase 4's scan-dependent code starts | R5 | No inspectah build is needed, and each answer confirms a guard phase 4 writes or lets it go. Run on 2026-11-14; the answers are in this spec, and the phase 4 tasks they change are rewritten to them. Its open items (§ 13 R5) gate no phase 4 code and run with the next real-host run. |
| After phase 2 | R1, R3 | R1 is the driving fix. R3's `to-existing-root` boots decide how "off" has to be rendered. Both were run. |
| At the branch tip, after phase 5 | R1, R2, R4, the open routes of R3, and R5's open items if no earlier run took them | The merge gate. The branch does not merge to `main` until they pass. R5's open items are recorded and gate nothing. |

## 16. Open questions

No question is open.

## 17. The case against this design

**The strongest objection is that the real loss is narrow and this is a wide
change.** The measured user-visible defect is one socket, `cockpit.socket`,
whose state in the image nobody checked. The survey sharpens it: on a
stock and a drifted host of each release, no socket, path, slice or
storage unit differed from preset, and the only differing units that were
not services were one local timer and `remote-cryptsetup.target`. The fix
proposed here renames a snapshot section, replaces its types, bumps the
schema, moves a Containerfile section, rewires timers, and reads
`[Install]` sections. A much smaller change exists: add `--type=socket`
and `--type=path` to the existing call, widen `is_real_service` to three
suffixes, and gate the inventory lists on the same test. It closes the
driving item and leaves everything else as it is.

What that alternative does not do is the argument for this spec. It leaves
timers with two owners, so a disabled local timer still comes out enabled
and a vendor timer enabled against preset is still lost. It enables local
sockets before their file is copied. It keeps an enabled instance
invisible. And it leaves "which collector owns which fact under
`/etc/systemd/system`" unwritten, which two independent assessments named
as the cause of this cluster of defects.

Other objections worth weighing:

- **One branch holds the driving fix back.** The socket fix is complete at
  the end of phase 2 and reaches `main` only when phase 5 does,
  while roughly 60 renamed files collect conflicts. The single bump buys
  one schema break in place of several.
- **Targets, slices and storage units were noise on the hosts measured.**
  The one row they produced needed a list to suppress it, that list is
  only as complete as the last stock host scanned, and the rows a package
  install makes through `Also=` are carried as redundant lines (§ 7.4).
- **The rules compare the source with the export, not with the image.**
  They assume the vendor unit files and drop-ins in the image hold the
  same `[Install]` as those on the source. A different package version
  can change an `Also=` list, and then a carried command has an indirect
  effect nobody recorded, silently. The spec does not close it: comparing
  against the target image is a non-goal (§ 3), and no version check is
  made.
- **The fleet view does not say why a command was not written.** The
  `declined` and `withheld` reasons are on single-host rows only (§ 11.1,
  § 12). Every `Also=` command on a fleet aggregate is an advisory, so a
  fleet user meets this for common packaged units, sees an included row
  with no command behind it, and has to read the Containerfile to learn
  why. It is left for fleet work or for the section design pass (§ 15).
- **Four limits are stated and not closed.** On a fleet aggregate, a row
  without `Also=` is compared against a snapshot that may lack another
  host's package-owned, unmodified install drop-in, so a carried `enable`
  could link more than any source had; it needs a package that ships an
  `[Install]` drop-in under `/etc`, which none of the surveyed packages
  does (§ 12). A rescan of a host built
  from an inspectah image carries the old flatpak provisioning unit and
  withholds the new one (§ 10.2). A fleet whose hosts disagree about a
  unit's fragment is judged on one host's owner when nothing is staged
  (§ 12). And an edited snapshot whose recorded `also` list disagrees with its own
  unit file is believed: the plan checks that the files are the recorded
  ones, not that the record is true (§ 10.2).
- **The install comparison leaves four cases.** A unit that matches no
  preset rule gets no row and no advisory, whatever its `/etc` files do.
  A unit under a `disable` preset that the package install enabled through
  another unit's `Also=` keeps the vendor's links beside the host's when
  its shadow changed the target. A changed `DefaultInstance=` leaves the
  old instance linked in the image (all § 7.4). And on a fleet, two hosts
  that record opposing causes for one link merge to the first host's, so
  the advisory is wrong for the others (§ 12).
- **An undetermined package section is said only inside the
  comparison.** The vendor-side record of § 7.4 reaches only the units
  the install comparison runs on. A `Disabled` unit at its `enable`
  preset whose package section is `Undetermined` still gets `disable`
  with no record, so the operator is not told that the package's
  `[Install]` could not be read.
- **Rule 7's recipe can switch off what it means to keep.** A suggested
  `disable` for a member follows that member's own `Also=` in its turn.
  Through a return edge it can unlink the owner, or a sibling that should
  stay on, after the owner's `enable` linked it. The recipe does not
  guard against it; the reader has to check every unit in the closure
  with `systemctl is-enabled` before keeping the lines (§ 10.2).
- **A hiding drop-in withholds commands.** A unit with a `set-property`
  drop-in under `/run/systemd/system.control` that hides an `/etc`
  install drop-in never gets its `enable` or `disable`, since a `/run`
  file is never staged, even where the image would read the same
  `[Install]` (§ 10.2). Where a `/run` file in the instance's `.d` wins
  over a persistent file in the template's `.d` that would still hide the
  `/etc` file in the image, the command is withheld though the image
  would match.
- **The type split costs ergonomics.** `CarriedUnit` is an enum over two
  name types and a fragment, where one struct and a render `match` would
  do, and it still needs § 10.2 behind it. The types earn their place only
  if a snapshot that states the impossible should fail to load and not
  merely fail to render.
- **A changed `[Install]` is carried only when it adds.** An administrator
  who copies a unit to `/etc` to change where it starts, replacing
  `WantedBy=multi-user.target` with another target, gets two advisories
  and no command, and the image keeps the package's link until the lines
  are added by hand. That may be the commoner reason to edit `[Install]`,
  and no measured host says. The rule also costs a second read of
  `[Install]` for every shadowed unit, two more causes and a relation that
  is not a link, to carry what one more verb (`reenable`) would carry
  whole. That verb was not measured at build time and is not one of the
  two the Containerfile renders. Two smaller costs: a shadow that drops an
  `Also=` name is advised with text and no line, because the collector
  knows what the host's files name and not what state the dropped member
  should have; and the line that removes a `VendorInstallOnly` link is an
  `rm`, the one place an advisory offers something other than `systemctl`,
  because `disable` would remove every link to the unit.
- **Rules 5 and 7 decline commands an operator would expect.** A libvirt
  service that is enabled while one of its sockets is disabled loses its
  line, although the two lines the advisory prints reproduce that host
  exactly. A local unit with `Also=` members is never carried, and neither
  is a packaged one once it or a member has any install file under `/etc`,
  even on a single host where the file and the record agree. Excluding a
  full shadow takes the unit's `enable` or `disable` with it, for every
  shadowed unit and not only those with `Also=`. On a fleet, every
  command for a unit with `Also=` members is an advisory, and hosts that
  disagree about whether a unit is shadowed lose its command.
  Sixty packaged units carry `Also=` once common daemons are installed,
  so this will be visible.
- **The declining rules add up.** The search-path test drops a unit with
  a runtime copy that would be gone at the next boot, and the instance
  gate omits an enable whose configuration is a package's untouched file
  while carrying, ungated, any template it does not know.
- **Some systemd behaviors are read from its source, not measured.** The
  drop-in order across an instance's and its template's `.d`, the hiding
  from the four directories above `/etc`, `%I` in `[Install]`, `;` preset
  comments and hidden drop-in names were read from the source of systemd
  252 and 257. Each is on § 13 R5's open list for the RHEL hosts; a
  measurement that differs changes the reader.
- **Two systemd behaviors are modeled from two releases.** The reader
  applies the reset of an empty `WantedBy=` or `Alias=` and ignores an
  empty `Also=`, as measured on systemd 252 and 257; a release that
  behaves otherwise would get a wrong `enable` or a missed link with
  nothing to say so, where declining every reset could only have cost an
  advisory. The preset lookup takes the name a package presets from what
  it ships: an instance's own name when it has a unit file, its
  template's otherwise. The snapshot does not record which names the
  scriptlets pass, and no scriptlet was read. A scriptlet that presets a
  template-backed instance by name, or presets a concrete instance only
  through its template, makes the model disagree with the image's
  package install in either direction, with no comment: a missing
  `enable` or `disable`, or a redundant one. For an instance with its
  own unit file the lookup also assumes that a list-form rule which does
  not list it, or a bare template rule, lets later rules decide; if such a
  rule stops the lookup instead, a later positive rule, or no rule at all,
  can hide the `enable` an administrator-enabled instance needs, with no
  comment. No stock preset file was searched for the list form, so how
  often a host meets it is unknown.
- **An owner at its preset is advised about every member it enables
  that the host has off,** which is noise on every scan of a stock host
  whose package ships an `Also=` member it does not link, and two lines
  to add by hand, one an `enable` the image already has, where a command
  could have done it.
- **A preset line inspectah cannot read is reported only in the scan's
  diagnostics.** The lookup stops there with no preset, where systemd
  skips the line, and the units it decided get no state change. The
  structured warning names them; the Containerfile does not, so an
  operator who reads only the Containerfile does not learn that those
  units were not compared.
- **Two controls operators have today go away:** the timer toggle, which
  does nothing, and the unconditional tuned enable.
- **Rendered unit state depends on the image's machine-id.** It survives
  first boot with the zero-length `/etc/machine-id` the bases ship. A
  user whose own later layer or installer script writes
  `uninitialized`, or removes the file, gets a preset pass that undoes
  rendered `disable` lines on RHEL 9 and rendered `enable` and `disable`
  lines on RHEL 10, with nothing in the build to say so. inspectah guards
  its own output and states the condition in the README; it cannot guard
  what is layered on afterwards. A shipped preset file was considered and
  not taken: presets cannot express a mask, a unit with no `[Install]`, a
  hand-made link, `add-wants`, an alias link or the default target, so it
  would be a partial guarantee beside a complete one.
- **Four rules that come with the single writer each have a cost.** A
  cron or at job row whose operation cannot be given an export effect is
  refused by name, so a control the operator can see stops working with
  an error where it used to appear to work. Counting inventory names
  withholds a generated cron job whenever any package unit at its preset
  has its name, and the operator cannot exclude that unit: the job has to
  be renamed by hand, and that FIXME says "timer and its service" even
  for an `@reboot` service or the flatpak service, where there is no
  timer. The trigger check omits a working timer whose trigger an alias, a template, an implicit
  mount or a boot generator would have provided, since it cannot see
  those, and it does not notice another fleet host's drop-in that sets
  `Unit=`. It is conservative the other way too: excluding a drop-in that
  sets `Unit=` omits the timer's `enable` even when a later drop-in
  overrides it and the effective trigger is unchanged, and that omission
  carries its comment. The inspector reads `Unit=` only from
  `<name>.timer.d` and, for an instance timer, its template's
  `<prefix>@.timer.d` (from phase 5, § 15), under `/etc/systemd/system`
  and `/usr/lib/systemd/system`, not from `/run`, `/usr/local/lib` or the
  type-wide `timer.d`, so a `Unit=` set only there is not seen and the
  recorded trigger is the timer file's or the default. And a timer with no recorded trigger is
  declined, so an older snapshot omits every timer `enable` until it is
  rescanned, where reading the missing field as passing would have kept
  them. Excluding a generated timer does not exclude the cron entry
  it came from: if cron is carried and running, the job still runs, and
  carrying both can run it twice. And refusing an
  unmanaged file under `/etc/systemd/system` drops a file an imported
  snapshot asked for, with a FIXME, when routing it through
  `staged_writes` would have carried it.
- **A mask is dropped with its package.** A mask on a unit whose package
  is not in the image is a harmless link. Once it is omitted, nothing
  keeps the unit off if a dependency, another package that provides the
  name, or a later layer supplies the unit. The `# Omitted:` comment
  records the intent that was lost and is all that is left of it.
- **The measurements are narrow.** One stock VM per release with synthetic
  drift, one booted install route, and RHEL only. A fleet with LUKS, NFS
  automounts or a hardening baseline could put mounts and targets in the
  differing set, and the routes read from source have not been booted.

Evidence that would change the design:

- Run R1 shows `cockpit.socket` already in the host's state in the built
  image by some other route. Then the driving loss is smaller than
  stated, and the omission half is the only real defect.
- Run R3 shows an install route that deploys a machine-id other than the
  image's zero-length one. Then rendered state does not hold on that
  route, and the condition in § 10.1 has to name it.
- Run R4 finds stock rows beyond the known ones. Each either joins the
  stock-state list or shows the list is the wrong tool, in which case
  comparing against the target image's presets is the fix.
- A release where an empty `WantedBy=` or `Alias=` does not reset, where
  an empty `RequiredBy=` does, or where `systemctl preset` on a template
  links a different set of instances. Then the reader or the preset
  lookup follows the measurement, per release if the two disagree.
- A stock preset file with a list-form `enable` line (§ 13 R5). Then the
  list form is a stock case, and T39 gains it as a fixture.
- The R5 case with a later positive rule shows a non-counting rule stops
  the lookup with `disable`. Then § 7.4's table gives that instance
  `disable` and T39's cases follow it.
- Hosts where rule 7's advisories are common enough to be a wall. Then
  the relaxation the measurements support is to let a member's own
  `disable` row, rendered after the `enable`, satisfy the check.
- The frontend design pass finds that one section for all kinds is harder
  to use than units in place. Then the data contract in § 11.1 stands and
  § 11.2 does not.
