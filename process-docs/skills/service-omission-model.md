# Service omission model

`plan_units` in `crates/pipeline/src/render/unit_plan.rs` is the
authority for which `systemctl enable`, `disable`, `mask` and
`set-default` lines the Containerfile carries for collected units of every
kind: services, sockets, paths, timers, targets, slices, and package-owned
mount, automount and swap units. It reads the snapshot's `units` section
(`UnitSection.state_changes`, `UnitStateChange` in
`crates/core/src/types/units.rs`, and the default target, link findings
and undetermined install records beside them) and checks each included row
against `staged_writes`, the stager's own decision, so the plan and the
files the export writes cannot disagree. `containerfile.rs` writes the
plan's lines in the `# === Systemd Unit State ===` section, after the
configuration `COPY`, and refine's projection reads the same plan. How
the collector reads the state the rows carry is in
[unit-install-state](unit-install-state.md).

## Every `systemctl` line is the plan's

**`RUN systemctl` is written by `unit_plan::systemctl_lines` and by
nothing else.** `systemctl_lines` takes only the two checked types:
`PlannedUnit`, which only `plan_units` builds, and `PlannedTarget`, the
default target, which only `plan_default_target` builds. So a line cannot
name a unit or a target the plan did not pass.
`t21_every_systemctl_command_in_the_containerfile_is_planned` in
`crates/cli/tests/units_scan_test.rs` enforces it: it reads every
`systemctl` instruction out of the rendered Containerfile, comments
skipped and continuations joined, on the scan and the refine export, and
requires the (verb, unit) pairs to equal the plan's. A new section that
needs a `systemctl` line adds a request to the plan.

The plan takes three kinds of request (§ 10.2):

- each included state change, and the included default target
  (`plan_default_target`), rendered in the unit state section;
- the units inspectah generates from cron entries (timers, and the
  service of an `@reboot` entry), in `UnitPlan.scheduled`, copied and
  enabled in the Scheduled Tasks section before the unit state section;
- `flatpak-provision.service`, in `UnitPlan.workloads`, copied and
  enabled in the container workloads section after it.

A generated request needs only its own files: the timer and its paired
service, or the `@reboot` service. When the stager withheld one as
`GeneratedNameCollision` the request is declined with
`OmissionReason::GeneratedNameCollision`, a FIXME in the generated
unit's own section; when one is missing for any other reason it is
declined with `UnitFileNotStaged`, an `# Omitted:` line. A generated unit
whose row is excluded, or whose cron job's row is excluded, is no
request at all, so it is never declined: refine's session excludes the
units of an excluded cron job in every projected snapshot, and refuses
including one of them while the cron job stays excluded. Declines are in
`UnitPlan.generated_declined` with their `GeneratedSource` (the cron
row's name, or the flatpak provisioning service), which refine projects
as `declined_generated` for the scheduled or flatpak row. A name that is
not a unit name, or that the stager refuses, is no request: the
Scheduled Tasks section names it in a `# FIXME:`
(`refused_generated_name_fixme`).

The kernel and boot section writes the tuned profile and no `systemctl`
line: `tuned.service` is enabled, disabled or masked only through its own
unit row. A local timer is enabled through its unit row too, never by the
Scheduled Tasks section, and a runtime updater timer with a local file
or a full shadow gets the updater default like any updater.

## The rules, per verb

`plan_units` runs the rules of § 10.2 of the units spec in this order for
each included state change. The first that fails declines the command.

| Order | Rule | Verbs | Declines with |
|---|---|---|---|
| 1 | Rule 2: a package-owned storage unit (`PackagedStorage`) acts only on its package's file. Any file staged at `/etc/systemd/system/<name>` is what the image would load. | all | `StorageFileStaged` |
| 2 | Rule 3: no `disable` or `mask` of a unit on `TARGET_NEEDED_UNITS` (`bootc-*`, `ostree-*`, `systemd-remount-fs.service`), and no `mask` of `TMP_MOUNT` (`tmp.mount`). Both constants are in `crates/core/src/baseline.rs`. | `disable`, `mask` | `TargetNeedsUnit` (a FIXME) |
| 3 | Rule 4: `systemctl mask` makes a link at `/etc/systemd/system/<name>` after every `COPY` and exits 1 on a regular file there, so the path must be free of any staged file. | `mask` | `MaskPathOccupied` |
| 4 | Rule 5, the row's own file (`rule_5_file`): `Local` needs its unit file staged; `Shadowed` needs its shadow staged; `Vendor` and `PackagedStorage` need no unit file staged at the unit's name, nor, for an instance recorded from its vendor template, at the template's. An instance recorded from a template under `/etc` needs that file staged and none at its own name. On a fleet aggregate, a staged `Local` or `Shadowed` file whose path holds more than one content variant fails too. | `enable`, `disable` | `UnitFileNotStaged` (`Local`), `InstallEvidenceChanged` (the rest) |
| 5 | Rule 5, the install drop-ins (`rule_5_drop_ins`): the install drop-ins the export stages for the unit are exactly the row's `install_drop_ins`, none missing and none added. The list includes a drop-in ranked above `/etc` that hides one of them, and, for an enabled instance, a runtime drop-in whose reset, replacement or hiding changed what the instance reads. | `enable`, `disable` | `InstallEvidenceChanged`; `InstanceNeedsEvidenceChanged` for a missing runtime drop-in of the second kind |
| 6 | Rule 5, the package (`needs_its_package`): the presence tiers below, for `Vendor` and `PackagedStorage` only. A package that may or may not be installed guards the command instead ([Guarded commands](#guarded-commands)). | all, `mask` included | `PackageExcluded`, `NotCarried`, `NotInTargetImage` |
| 7 | Rule 5 asked of a timer's trigger (`timer_trigger_check`), below. | `enable` of a timer | the three `TimerTrigger` reasons |
| 8 | Rule 6, an instance's configuration (`rule_6_instance_config`), below. A row whose `needs_unknown` is set declines first, since what it reads was never read. | `enable` of a template instance | `InstanceNeedsUnknown`; `InstanceConfigNotStaged` |
| 9 | Rule 7, `Also=` members (`rule_7_also`), once every row has passed or failed the rules above. | `enable`, `disable` with a non-empty `also` | `AlsoMember`, an advisory |

**Instances are planned like any row.** There is no instance step before
the rules: an instance's `disable` and `mask` meet the same rules as any
unit's (a masked instance is carried as `systemctl mask`, under rules 3
and 4 and rule 5's package half, the package being the template's), and
its `enable` meets rule 6 as well.

**Rule 6** reads the row's `required_config` and nothing else. For each
path, the config entries at it or under it (a directory) decide, by
`Destination`: none is `NotInSnapshot` naming the path; then, per entry in
path order, content not captured is `NotCaptured`, an entry
`staged_writes` does not write (excluded, left out or withheld) is
`Excluded`, and an `Inline` or `Excluded` finding in
`snapshot.redactions` at the entry is `Redacted`, and on a fleet
aggregate an entry whose path holds more than one content variant is
`FleetVariants` whichever variant is staged (`holds_variants`, which rule
5's `one_variant` uses too), each naming the entry. An entry under the
path whose own path `Destination` refuses (a line break in a file name)
is never staged: it is `Excluded` naming its path as written, found by
`lexically_under`, never filtered out, or the other members of a
required directory would pass it. A `required_config`
path with a `..` component is kept by the collector and fails first, as
`Unverifiable` naming it ("inspectah cannot tell which file it reads"),
whatever the export holds, since `Destination` refuses `..` (a
provisional ruling for the owner, 2026-11-15): "is not in the export"
would misdiagnose a clean, staged file.
Package ownership is never evidence: a package-owned file the
administrator did not change has no config entry and fails, by design
(§ 10.8 "Only a file the export carries passes"). Not captured is checked
before not staged, since an uncaptured entry is never staged either.
Rule 6 has no secret detection of its own: "redacted" is whatever the
redaction pass recorded, so the WireGuard key comes out right. A passing
instance can carry notes: one per entry when `redaction_state` is `Raw`
(a `--no-redaction` scan), one per entry with a `Flagged` finding. They go
to `UnitPlan.instance_notes` only once the enable is planned (after rule
7), and refine's `unit_state_views` appends them to the row's own
`attention_reason`; they are not Containerfile lines. An instance with an
empty `required_config` passes; when its template is on no § 7.5 list and
`data_paths` is empty too, the collector gives the row
`INSTANCE_READS_UNKNOWN` as its note, and a planned enable of such a row
gets `INSTANCE_CARRIED_ALONE` in `instance_notes` (found by the note on
the row, the only trace of the lists the plan cannot read).

**Data under `/var` (§ 10.8).** A carried instance `enable` (after rule
7) whose row has a non-empty `data_paths` gets one sentence in
`UnitPlan.data_advisories` (`data_advisory`, every path in one sentence,
`a, b and c`, "it" for one path and "them" for several; `None` for an
empty list, so no caller can write a sentence that names no path). It
says the enable *may* start the unit on first boot: enabling makes the
install links, and activation depends on the dependency graph and the
unit's conditions (a provisional ruling for the owner, 2026-11-15; § 10.8
still quotes "starts").
`unit_state_lines` writes it as an `# Advisory:` line
after the `# Omitted:` lines and before the `RUN` lines, escaped, and
refine appends the same sentence to the row's `attention_reason` after
the instance notes. A declined enable, a `disable` or `mask`, and a row
with no `data_paths` get none; the collector records `data_paths` only
for an `Enabled` instance anyway.

**A mask follows the package check.** `mask` reads no `[Install]`, so
rule 5's file and drop-in halves (orders 4 and 5) do not apply to it, but
the package half (order 6) does, through the same presence tiers: a mask of a
`Vendor` or `PackagedStorage` unit whose package is certainly not in the
image is omitted with the tiers' reason, so the host's mask stays visible
as an `# Omitted:` line. A mask is never guarded: `systemctl mask` makes
its link whether or not the unit exists (measured on systemd 252 and 257),
so a mask whose package may be missing stays plain. Rules 3 and 4 run
first, so a protected unit or `tmp.mount` keeps its FIXME and an occupied
path keeps `MaskPathOccupied` whatever the package. An `Absent` mask (no
unit file) and a mask with no known owner are carried (tier 1). On a
fleet, a mask of a package the merge excludes by prevalence is omitted
until the package is included.

The presence tiers run only for `Vendor` and `PackagedStorage`, whose
image file comes from the package. A `Shadowed` unit's one condition is
its staged shadow, which the image loads with or without the package, so
a shadowed unit of an excluded package keeps its command. A `Local` unit
has no owner.

Rule 1 is the stager's, not the plan's: `staged_writes` withholds a
zero-length file at a unit path (a mask is carried only as a `systemctl
mask`), and two collected offers with different content at one
destination, or a file and a directory at one path, are a `Conflict` and
both are withheld and named in a FIXME. Identical content offered twice
at one destination is one write, and a repo file, GPG key or firewall
zone yields to a config entry at its destination. A unit inspectah
generates whose name a collected unit shares is withheld as
`GeneratedNameCollision`, every file of it (the paired service of a
generated timer included), so a generated file never replaces the
host's own. The staging rules in full are in
[unit-state-correctness](unit-state-correctness.md).

## Install files: rule 5's drop-in half

Rules 5 and 7 ask one question of the export: which files under
`/etc/systemd/system` can supply or change `[Install]` for a name `N`.
They are the unit file at `N` (for an instance, its template's too), and
each `.conf` in `N.d` (or the template's `.d`) that holds an `[Install]`
section or has the file name of one of the row's `vendor_drop_ins`. A
drop-in with no `[Install]` hides a vendor drop-in of its name, so
excluding it brings the vendor section back. The type-wide (`service.d`)
and dash-prefix directories are never install files (`install_drop_in_dirs`).

- **"Holds an `[Install]` section" is the only lexical read the plan
  makes** (`holds_install_section` in core, the collector's line model).
  The plan never reads `[Install]` keys and never re-resolves a unit.
- `rule_5_drop_ins` checks each recorded path through `Destination` (one
  that does not parse fails), then scans the staged writes under each
  install directory by a `BTreeMap` range from the directory's
  destination, so a staged install file the row did not record fails too.
- **A drop-in that hides an install drop-in is recorded with it.** A
  drop-in in a directory systemd ranks above `/etc/systemd/system`
  (`/etc/systemd/system.control`, `/run/systemd/system.control`,
  `/run/systemd/transient`, `/run/systemd/generator.early`) hides an
  `/etc` drop-in of its file name, so the collector reads the unit without
  the hidden `[Install]`. The image reads the hidden file whenever the
  hiding one is not copied, so the collector puts the hiding path in the
  row's `install_drop_ins` beside the hidden one, and the first loop
  checks it like any other: a `/run` file is never staged, so the row is
  omitted with `InstallEvidenceChanged` naming it; an
  `/etc/systemd/system.control` file is a config entry, staged when
  included. The plan reads no new field and no new reason; the existing
  text ("the [Install] section in the image would not be the one recorded
  on the source host (<path>)") names the hiding file. The second loop
  scans only the `/etc` install directories, so an unrecorded staged file
  above `/etc` is not checked: one holding `[Install]` refuses its unit in
  the collector, and one that hides nothing changes nothing. Link findings
  and undetermined records keep the `/etc` files only, so
  `excluded_install_files` is unchanged. On a fleet aggregate the merge
  unions `install_drop_ins`, so any host's hiding file has to be staged,
  and `one_variant` applies to it as to the rest.
  In a fleet aggregate, a recorded file whose path holds more than one
  content variant fails (`one_variant`).
- **A drop-in that changed what an instance reads is recorded too, with
  its own reason.** The collector adds to an enabled instance's
  `install_drop_ins` each runtime drop-in whose reset, replacement or
  hiding of a drop-in of its name removed a path `required_config` or
  `data_paths` would hold (`InstanceNeeds::overriding_drop_ins`, see
  [unit-install-state](unit-install-state.md)): without it the image
  reads that path, unchecked by rule 6. The first loop checks it like the
  rest, so a `/run` one, or an `/etc` one excluded in refine, withholds
  the enable. Schema 28 has no field of its own for it, so the plan reads
  each failing path's role (`install_drop_in_role`): outside the own and
  template `.d`, or in one under `/etc` with no `[Install]` in the
  snapshot's copy and no vendor drop-in's name, or above `/etc` hiding no
  recorded `/etc` file of its name, it is `InstanceNeeds` and the decline
  is `InstanceNeedsEvidenceChanged` ("the drop-in <path> clears or
  replaces a path the instance reads and is not in the export as the
  source host had it, ..."); anything else, a path `Destination` refuses
  and an `/etc` path the snapshot holds no copy of included, keeps
  `InstallEvidenceChanged`. Refine's `install_drop_ins` keeps only the
  `Install` role, so the `[Install]` badge never marks a reset drop-in.
- § 10.2 says a file whose content was not captured counts as an install
  file, and it does reach rule 5. The collector records an `/etc` drop-in
  it cannot read as text in `install_drop_ins` (`install_evidence`:
  unreadable text counts). Such a file can never be staged, so rule 5's
  first loop omits the row with `InstallEvidenceChanged`, which is the
  right outcome; keep that branch. A file between 256 KiB and 1 MiB is
  over the config walk's cap but is read through its drop-in row, which
  holds the content. Rule 7 runs only on rows that passed rule 5, so no
  scan feeds empty uncaptured content to `holds_install_section`.

**Every rule that asks whether a file is staged asks `is_staged`**, which
looks the path up in `staged_writes.writes` by `Destination`, never by a
string compare. A path spelled two ways is one destination.

## Rule 7, `Also=` members

`enable` and `disable` also act on every unit `Also=` reaches. A command
for a unit with a non-empty `also` list is carried only when three
conditions hold, the first that fails giving the `AlsoWhy`:

1. Every member's `MemberLinks` match the verb: `Installed` or `Inert` for
   `enable`, `Unlinked` or `Inert` for `disable`.
2. A member with a row of its own is carried with the same verb; a member
   with no row is not an instance under `enable` nor a protected unit
   under `disable`.
3. Neither the unit nor any member has an install file in the snapshot,
   **staged or excluded** (`install_file_in_snapshot`). Excluding the file
   does not make the command safe: the list and the links were recorded
   from it.

- **`rule_7_also` runs after rules 1 to 6 for every row**, because
  condition 2 reads that first pass (`carried`). A member whose own row
  fails, or carries another verb, blocks its owner.
- **A fleet aggregate is `AlsoWhy::FleetAggregate` before any condition**,
  naming every member: the merge keeps no other host's file paths, so no
  condition can be established.
- **The reason names every failing member.** `AlsoMember.member` is the
  first member, in `also` order, failing the first failing condition;
  `named` lists every member failing it the same way and `further` the
  members failing it the other way it can fail (a condition fails at most
  two ways). `add` lists the members that get a line of the opposite verb.
- **The recipe never suggests a command § 10.3 refuses.** A member in
  `add` for which `target_needs_unit` holds under the opposite verb (a
  protected unit an owner's `enable` reaches) gets no line; it gets "No
  disable line for <unit>: the image needs this unit." instead. When a
  line is withheld, or a member is `LinksDiffer(Other)` (in `why` or
  `further`), the lines cannot reproduce the source host, so
  `ALSO_PARTIAL` heads them in place of `ALSO_TO_REPRODUCE` and the
  `ALSO_EACH_FOLLOWS` footer is dropped (Mark Russell, 2026-11-15).
- **Every rule 7 sentence is an `ALSO_*` constant, and
  `also_advisory_lines` is the only renderer**, for the Containerfile and
  refine. Its blocks follow the `systemctl` lines. The wording beyond the
  one block § 10.2 quotes is provisional until the spec quotes it; change
  the constants, not the callers.

## Findings the plan advises on

Beside the commands, the plan renders the collector's link findings and
undetermined install records (`link_advisories`,
`undetermined_advisories`), each a `FindingAdvisory` built from
`manual_link_lines` or `undetermined_install_lines`. A `VendorInstallOnly`
link's removal line is `RUN test -L '<path>' && rm -- '<path>'`, so it
removes only a link; when `staged_writes` writes a file at that path, the
line is replaced by one saying the copied file replaces the link, so
advice never deletes a carried file. While one of the
unit's install files is not staged (`excluded_install_files`, per unit, by
`Destination`), its advisories give way to one `install_omitted_line`
naming the excluded files, with those files in `not_applied`, and
`unit_state_lines` writes that line once per unit.

The default target is planned by `plan_default_target`: rule 5's file
half, then the package half for a `Vendor` target. `set-default` reads no
`[Install]`, so the drop-in half and rule 7 do not apply. A target that
fails is `default_target_declined`, written with `default_target_line`.
The file half's `InstallEvidenceChanged` text speaks of `[Install]`, so
`set_default_reason` restates it for the target: a `Shadowed` replacement
that is not staged is `DefaultTargetFileNotStaged`, a staged `Local` or
`Shadowed` file holding fleet variants is `DefaultTargetFileVariants`, and
a file staged at another path that would load in the recorded one's place
(a `Vendor` target's name) is `DefaultTargetFileStaged`. Their texts say
only that the image cannot set the recorded file, never which file it
would set instead: the package half is not checked for a `Shadowed`
target, so its package may be excluded too, and then `set-default` would
find no file and fail.

Runtime package updaters (`RUNTIME_UPDATERS` in `core/baseline.rs`) are
excluded by default at collect and in the fleet merge, and can be
included in refine. An excluded updater's row is not declined: the
section names it in an `# Omitted:` line from `excluded_updater_line`.

## Omission versus advisory

A unit whose owning package is not in the image cannot be acted on:
`systemctl enable` on a missing unit fails the build with
`Failed to enable unit, unit <name> does not exist` (systemd 252 and
257), and so does `disable` on 252 (RHEL 9); 257 (RHEL 10) prints the
error and exits 0. So the plan has these outcomes, and they are not
interchangeable:

- **Declined** (`UnitPlan.declined`): the unit is left out of every
  `systemctl` line and recorded with its `OmissionReason`. It renders as
  a `# Omitted: <unit> (<reason>)` comment, or for `TargetNeedsUnit` as a
  `# FIXME:` with § 10.3's sentence, and refine's unit row shows it in
  `declined`.
- **Guarded** (`PlannedUnit::guard`): the command is planned, and runs
  only if the unit's vendor file is in the image
  ([Guarded commands](#guarded-commands)).
- **Advisory** (`ServiceAdvisory`): the unit is emitted, plain or
  guarded, with context saying its presence could not be proved. An
  advisory is supplemental; it never removes the unit.

Choose omission only when the package is certainly absent, or the export
cannot reproduce the command. Choose a guard whenever the package may or
may not be installed. An advisory on a unit that will fail
the build is not a mitigation: until 2026-11-08, an excluded package's
unit was emitted with a `PackageExcluded` advisory that no surface
showed, and the build failed with no warning anywhere.

## The reasons are enums

`OmissionReason` holds every reason the plan declines a command:

- `PackageExcluded`: every `packages_added` entry for the owner is
  excluded, the baseline does not ship it, and nothing installed may pull
  it in (tier 4).
- `NotCarried`: the owner is in the not-carried class
  (`not_carried.rs`): an auto package nothing included pulls in, which
  the scan excluded. It is chosen over `PackageExcluded` only when every
  `packages_added` entry with the owner's name is in the not-carried
  list; an excluded entry of that name outside the not-carried class
  keeps it `PackageExcluded`. It
  never creates an omission by itself: the unit is omitted only when the
  presence rules already prove the owner absent, which is why a mixed-arch
  owner with one carried arch keeps its unit. The list holds only
  packages certainly left out, so the two agree. Wording is
  `describe(NotCarried)`: `package 'rhc' is not carried: <NOT_CARRIED_REASON>`.
- `NotInTargetImage`: a baseline is available and the owner is in
  neither it nor `packages_added` (tier 6).
- `UnitFileNotStaged`, `InstallEvidenceChanged { path }`,
  `StorageFileStaged { path }`, `TargetNeedsUnit` and
  `MaskPathOccupied { path }`: the rules above.
- `TimerTriggerEvidenceChanged { path }`, `TimerTriggerUnresolved` and
  `TimerTriggerNotStaged { trigger }`: rule 5 asked of a timer's trigger,
  for an `enable` only, after the timer's own file and package, in that
  order (`timer_trigger_check`). The plan reads the trigger the scheduled
  inspector recorded on the timer's description (`SystemdTimer.trigger`,
  `trigger_drop_ins`) and parses no unit file: a recorded drop-in that is
  not staged means the recorded trigger may not be the image's, so the
  check declines rather than re-resolve it. **An instance trigger loads
  from its template's file** (`trigger_file`): `worker@a.service` has no
  file of its own, so `trigger_presence` looks at
  `/etc/systemd/system/worker@.service` unless the snapshot holds
  something at the instance's own name. Looking at the instance's path
  finds nothing and reads the trigger as provided, which carries the
  timer with its unit excluded. A timer with no description,
  or an `unresolved` trigger, declines too: an unknown trigger is never
  read as no local trigger. A local trigger is a unit file under
  `/etc/systemd/system` with no vendor file (`trigger_presence`); a
  staged file at its path, a generated one included, or a package
  certainly in the image that owns it, provides it, and the enable is
  plain. **A trigger only a possible package ships guards the enable.**
  When the owner of its config entry, or of a vendor trigger recorded by
  a state change, is possible but not certain, the timer's `enable` also
  waits for the trigger's file (`/etc/systemd/system/<file>` or the
  vendor file), so a timer is never enabled plain while dnf may leave its
  unit out. Without a baseline the owner is taken to be possible. On the collector side
  (`scheduled.rs`, `timer_trigger`), drop-ins are listed with
  `systemd_reads_drop_in` (a hidden `.override.conf` is no evidence) and
  selected with the `[Install]` reader's `selected_drop_ins`, so `Unit=`
  and `[Install]` cannot disagree on load order; for an instance timer that includes the
  template's `<prefix>@.timer.d`. **The first `Unit=` systemd accepts
  is the trigger, not the last** (`config_parse_trigger_unit`, 252 and
  257, logs "Multiple units to trigger specified" for every later one),
  and an assignment systemd ignores counts as absent: an empty one, one
  holding a specifier outside `unit_name_printf`'s table, such as `%I` or
  a directory specifier ("Invalid slot", measured 2026-11-16), which
  `trigger_unit_ignored` in `specifiers.rs` tests, and one that names the
  timer itself ("Units cannot trigger themselves"), skipped in
  `accepted_trigger` before the first accepted value is chosen. Skipping
  it matters: recorded as the trigger, the timer's own staged file would
  satisfy `trigger_presence` while the real trigger is excluded.
  **Another timer's name is `Unresolved`.** systemd accepts a timer as a
  timer's trigger (no type check in `config_parse_trigger_unit` or
  `timer.c`, 252 and 257) but rejects any name of the timer itself
  (`unit_has_name`), and the scan cannot prove for the image whether a
  `.timer` name is the timer's own. So `accepted_trigger` stops on any
  `.timer` name other than the timer's own file name, in the unit file
  or a drop-in, and the enable is not carried. Do not try to tell the
  timer's other names apart from another timer's: each attempt left a
  case where a wrong trigger reached the plan. For an `Unresolved`
  trigger, `trigger_drop_ins` keeps every `/etc` drop-in up to and
  including the unresolved one.
  **`trigger_drop_ins` holds only the `/etc` drop-ins whose leaving out
  can change the trigger**: the one that holds the accepted `Unit=`, and
  one at or before it that hides a drop-in of its file name. A drop-in
  whose `Unit=` lines are all ignored, or that comes after the accepted
  one, is not evidence, so excluding it never omits the enable, unless it
  hides another applicable drop-in. The value is
  expanded with `SpecifierSet::TriggerUnit` (`%i`, `%p`, `%N`, `%%`); a
  specifier systemd does resolve and the scan does not (`%H`, `%n`, `%j`)
  leaves the trigger `Unresolved`. An instance timer has no timer file, so
  `scan_instance_timers` describes each instance linked under `/etc`
  (the units inspector's `read_host_links`, the links instances are found
  by) from its template's file, once per root the template sits under.
  Without that description, every carried instance timer `enable` would
  decline as `TimerTriggerUnresolved`.
- `GeneratedNameCollision { name, reserved_by }`: a generated request
  whose files the stager withheld because a collected unit, `name`, has
  one file's name. A FIXME, with exclude advice for `Reservation::Row`
  and rename advice for `Reservation::Inventory`.
- `InstanceConfigNotStaged { path, gap }`: rule 6, with `ConfigGap`
  `Excluded`, `NotCaptured`, `Redacted`, `NotInSnapshot`, `FleetVariants`
  or `Unverifiable`. The first four texts are § 10.8's table; the fleet
  and `..` texts are the plan's own. All are written only by
  `OmissionReason::describe`.
- `AlsoMember { member, why, verb, named, further, add }`: rule 7. Not an
  `# Omitted:` line but an advisory block (`also_advisory_lines`); refine
  marks it `DeclinedMarker::Advisory`.

`AdvisoryReason::PackageUnreachable` and `BaselineUnavailable` are the
emitted-with-context tiers.

`declined_line` is the one place the marker (`# Omitted:` or
`# FIXME:`) and the text of a declined command are built, from
`OmissionReason::describe` (a FIXME for `GeneratedNameCollision`) or,
for `TargetNeedsUnit`, `target_needs_unit_text` (in `core/baseline.rs`, beside the list it
reads). The Containerfile and refine's `declined` view both call it, so
the surfaces cannot drift. `OmissionReason::name` is the one spelling of
the variant, which refine's view sends as the reason.

## The presence tiers

The plan does not predict dnf's solver (Mark Russell, 2026-11-21). It
reads two sets of package names, built once per plan by `Presence::new`
from `export_installs`:

- **certain**: the baseline's packages, every package the Containerfile
  names on an install line, and what one of those certainly pulls in;
- **possible**: `certain`, every included `packages_added` entry, and
  everything an included package may pull in: every `or` operand, both
  branches of every conditional, and every weak dependency
  (`DepExpr::possible` in the collector).

`classify_service_presence`, evaluated in order:

1. No owning package: plain.
2. The owner certain: plain.
3. The owner possible: guarded. This covers an excluded provider of a
   capability another package provides too, an included auto package the
   Containerfile does not name (kept as one provider a leaf may pull in),
   a dependency only a conditional requirement brings, an `or` operand or
   branch the host did not take, a recommended or supplementing package,
   an auto package an operator included by hand and its dependencies, and
   an included entry rendered only as a manual step. When in doubt a package is possible,
   never absent.
4. The owner in `packages_added`, every entry excluded, and nothing
   installed may pull it in: decline with `NotCarried` or
   `PackageExcluded`.
5. Not found, no baseline: guarded, with `BaselineUnavailable`. A scan
   always records a baseline now, so only an older or hand-built snapshot
   gets here.
6. Not found, baseline available: decline with `NotInTargetImage`.

An included entry the export cannot install, with no included entry it
can, adds `PackageUnreachable` (and `BaselineUnavailable` with no
baseline) to whatever command is written.

### What counts as certain

`export_installs` in `unit_plan.rs` is the one answer. A package is
certain when:

- the Containerfile names it (`named_on_install_line`): an included
  package installed on its own route (`installs_on_its_own_route`) that
  is installed from its carried RPM, or is locked, anaconda-sourced, a
  carried unit owner no installed leaf brings, or a leaf (every included
  package when the host has no leaf data). This mirrors
  `packages_section_lines`' selection less refine's `original_includes`,
  which the plan does not see, so **an auto package an operator includes
  by hand reads as not named**: its commands and its dependencies' are
  guarded, though the Containerfile installs it. The guard then passes;
- the disabled-repo transaction names it: a package only a host-disabled
  repo serves that an installed leaf pulled in on the source host
  (`leaf_tree_pulled_in`, the `leaf_dep_tree` entries of the installed
  leaves), the routing beta.6 released and the Containerfile reads too;
- a named package's `dep_closures` entry lists it and a repo the image
  enables serves it, or the disabled-repo transaction names it.
  `dep_closures` follows Requires only: a Recommends or Supplements, and
  every path through one, is possible only, because dnf skips a weak
  dependency it cannot install and a plain `disable` of the missing unit
  then fails on systemd 252. A snapshot without `dep_closures` (older than schema 29, or
  hand-built) has only `leaf_dep_tree`, which credits every installed
  provider of a capability, so it makes nothing certain.

**On a fleet** a closure counts only through a package every host has, on
a fleet whose every host gave leaf data (`closure_is_certain`). The
merged `dep_closures` entry is the intersection over the hosts that have
the package, so it accounts for every provider installed beside it on
those hosts. A package only some hosts have cannot see a provider another
host's kept package brings: if `host-b`'s `other` provides the
`transport` that `app`, only on `host-a`, requires from `helper`, the
fleet image that installs both need not install `helper`. So `helper`'s
commands are guarded. A package every host has meets every possible
provider on some host, whose closure then leaves the dependency out.

### Guarded commands

A planned unit with a guard (`PlannedUnit::guard`, and
`PlannedTarget::guard` for `set-default`) carries the files, as absolute
paths, the command waits for, every one of which must exist: the vendor
file of a unit whose package is only possible (a `Vendor` fragment's
file, the unit's own name or its template's for an instance, or a
package-owned storage unit's name), and for a timer's `enable`, the file
of a trigger only a possible package ships (`timer_trigger_check`).
`systemctl_lines` writes the plain commands as before, then every guarded
one in one `RUN`, under `GUARDED_COMMANDS_COMMENT`, in the same order
(`set-default`, then enable, disable, mask; by kind, then name):

```dockerfile
# dnf may or may not install the packages these units come from, so each
# command runs only if the unit files it tests are in the image.
RUN if [ -e /usr/lib/systemd/system/alt.service ]; then systemctl disable -- alt.service; fi && \
    if [ -e /usr/lib/systemd/system/worker@.service ]; then systemctl enable -- worker@blue.service; fi && \
    if [ -e /etc/systemd/system/report.service ]; then systemctl enable -- report.timer; fi
```

A command with two files tests each, `[ -e a ] && [ -e b ]`. The audit
report lists every guarded command under its unit state changes
(`guarded_unit_commands` in `audit.rs`); refine's rows do not mark them
yet.

- **The test is a file test, not `systemctl`.** No service manager runs
  in an image build: `systemctl cat` exits 1 there even for a unit that
  exists, and `systemctl is-enabled` exits nonzero for a disabled unit and
  a missing one alike (measured in `rhel-bootc:10.3`, systemd 257, and
  `ubi9`, systemd 252, 2026-11-21). The vendor directory is where the
  package puts the file, and rule 5 already requires that nothing is
  staged at the unit's name under `/etc`.
- **`&&` joins them**, so a command that runs and fails still fails the
  build; an `if` whose test fails exits 0.
- **A mask is never guarded** (above).
- `t21_every_systemctl_command_in_the_containerfile_is_planned` reads the
  guarded commands too (`guarded_commands` in the CLI test support) and
  compares each one's file with the plan's guard.

## Aggregate snapshots

An aggregate's `packages_added` is leaf-only, so absence is never read
from a name missing there. `aggregate_presence`: an owner certain is
plain; an owner with entries, every one excluded and none possibly
installed, is omitted, since an explicit exclusion on an entry that is
present cannot be faked by leaf filtering; then a package the merge
provably dropped (`not_carried::FleetDrops`: in neither the baseline nor
`packages_added`, and pulled in by nothing the export installs, not even
as one of several providers, from the kept packages' closures and
alternatives joined across hosts) is omitted, `NotCarried` when the
fleet's not-carried list names it, `NotInTargetImage` otherwise. Every
other owner, an unknown one included, is guarded. Before 2026-11-10, a
fleet whose hosts enabled `rhcd.service` against its preset carried
`systemctl enable rhcd.service` although the merge had dropped `rhc`,
and the build failed.

## What does not show omissions

The audit report (`audit-report.md` and `.html`) lists a unit's row
without saying it was omitted or guarded. That is true of tier 6 as well
and was left as is; no new surface was built for omissions. Refine's
view shows a guarded command as planned: it does not say it is guarded.

## Excluded is not absent

"Excluded" means the export does not name the package. A package that an
installed package requires is in the image whatever its disposition,
because dnf installs it with the package that needs it, so its unit
command stays plain. One of several providers of a capability may or may
not be installed: with the base image or another installed package
providing it too, dnf need not install it, and with only added ones it
picks one. Its command is guarded. That covers refine's default exclusion
of a non-leaf package (the scan tarball leaves the same package included,
and unnamed) and an operator's explicit exclusion alike.

The collector's `dep_closures` hold real requirements only, a dependency
exactly one installed package provides; the providers dnf may pick among
are in `dep_alternatives`. The evaluator behind them only has to be right
in one direction: a dependency it leaves out of `dep_closures` is guarded,
never lost (see `rpm-output-field-parsing.md`). An included package that
renders only as a manual step (a `# TODO`, the repo-less `# MANUAL` block
with no cached or uploaded RPM, or the repo-enabling `# FIXME` after its
serving repo was dropped and with no upload) is never installed, so it
pulls nothing in. A demoted repo-enabling package that was uploaded
installs from the upload in the repo-less block, so it still counts.

`dep_closures` keeps leaves, so a leaf that another installed package
depends on counts once the operator excludes it. The not-carried class
alone asks the narrower `pulled_in_by_default_roots` (leaves and carried
owners, alternatives counted), since it is decided while every auto
package is still included. `pulled_in_by_included` (the certain closure
of every included root, named or not) is now read only by the `/var`
evidence in `var_dirs.rs`.

With no leaf data both sets hold only the baseline and the included
entries, so an excluded package is omitted although dnf may install it:
the behavior from before the rule existed. The collector's dnf fallback
cannot tell one provider from several, so it records every dependency in
`dep_alternatives`: on that route every dependency's command is guarded.

### The not-carried class is this rule applied to auto packages

An auto package that no included package may pull in is excluded by
default on the scan path, so tier 4 omits its unit. `classify_auto_carriage`
in `not_carried.rs` decides the class from `pulled_in_by_default_roots`;
see `not-carried-auto-packages.md`. The `/var` evidence in `var_dirs.rs`
asks `pulled_in_by_included`, so a package a leaf the export installs
pulls in counts as carried there whatever its disposition, subject to the
repo-enabling demotion filter. Its own install set uses
`installs_on_its_own_route` too, so the two cannot disagree about which
leaves are installed.
