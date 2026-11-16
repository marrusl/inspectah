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
| 0 | The instance placeholder: a template instance is collected and not carried yet. It decides before every rule, and Task 5.2 removes it. | all | `InstanceNotCarriedYet` |
| 1 | Rule 2: a package-owned storage unit (`PackagedStorage`) acts only on its package's file. Any file staged at `/etc/systemd/system/<name>` is what the image would load. | all | `StorageFileStaged` |
| 2 | Rule 3: no `disable` or `mask` of a unit on `TARGET_NEEDED_UNITS` (`bootc-*`, `ostree-*`, `systemd-remount-fs.service`), and no `mask` of `TMP_MOUNT` (`tmp.mount`). Both constants are in `crates/core/src/baseline.rs`. | `disable`, `mask` | `TargetNeedsUnit` (a FIXME) |
| 3 | Rule 4: `systemctl mask` makes a link at `/etc/systemd/system/<name>` after every `COPY` and exits 1 on a regular file there, so the path must be free of any staged file. | `mask` | `MaskPathOccupied` |
| 4 | Rule 5, the row's own file (`rule_5_file`): `Local` needs its unit file staged; `Shadowed` needs its shadow staged; `Vendor` and `PackagedStorage` need no unit file staged at the unit's name. On a fleet aggregate, a staged `Local` or `Shadowed` file whose path holds more than one content variant fails too. | `enable`, `disable` | `UnitFileNotStaged` (`Local`), `InstallEvidenceChanged` (the rest) |
| 5 | Rule 5, the install drop-ins (`rule_5_drop_ins`): the install drop-ins the export stages for the unit are exactly the row's `install_drop_ins`, none missing and none added. The list includes a drop-in ranked above `/etc` that hides one of them. | `enable`, `disable` | `InstallEvidenceChanged` |
| 6 | Rule 5, the package (`needs_its_package`): the presence tiers below, for `Vendor` and `PackagedStorage` only. | all, `mask` included | `PackageExcluded`, `NotCarried`, `NotInTargetImage` |
| 7 | Rule 5 asked of a timer's trigger (`timer_trigger_check`), below. | `enable` of a timer | the three `TimerTrigger` reasons |
| 8 | Rule 7, `Also=` members (`rule_7_also`), once every row has passed or failed the rules above. | `enable`, `disable` with a non-empty `also` | `AlsoMember`, an advisory |

Rule 6, an instance's configuration, is phase 5's and is not built; the
placeholder at order 0 stands in its place.

**A mask follows the package check.** `mask` reads no `[Install]`, so
rule 5's file and drop-in halves (orders 4 and 5) do not apply to it, but
the package half (order 6) does, through the same presence tiers, unchanged: a mask of a
`Vendor` or `PackagedStorage` unit whose package is not in the image is
omitted with the tiers' reason, so the host's mask stays visible as an
`# Omitted:` line. Rules 3 and 4 run first, so a protected unit or
`tmp.mount` keeps its FIXME and an occupied path keeps
`MaskPathOccupied` whatever the package. An `Absent` mask (no unit file)
and a mask with no known owner are carried (Tier 1). On a fleet, Tier 2
reads the representative host's owner, so a mask of a package the merge
excludes by prevalence is omitted until the package is included.

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

Runtime package updaters (`RUNTIME_UPDATERS` in `core/baseline.rs`) are
excluded by default at collect and in the fleet merge, and can be
included in refine. An excluded updater's row is not declined: the
section names it in an `# Omitted:` line from `excluded_updater_line`.

## Omission versus advisory

A unit whose owning package is not in the image cannot be acted on:
`systemctl enable` on a missing unit fails the build with
`Failed to enable unit, unit <name> does not exist`. So the plan has two
outcomes, and they are not interchangeable:

- **Declined** (`UnitPlan.declined`): the unit is left out of every
  `systemctl` line and recorded with its `OmissionReason`. It renders as
  a `# Omitted: <unit> (<reason>)` comment, or for `TargetNeedsUnit` as a
  `# FIXME:` with § 10.3's sentence, and refine's unit row shows it in
  `declined`.
- **Advisory** (`ServiceAdvisory`): the unit is emitted, with context
  saying its presence could not be proved. An advisory is supplemental;
  it never removes the unit.

Choose omission only when the package is known to be absent, or the
export cannot reproduce the command. An advisory on a unit that will fail
the build is not a mitigation: until 2026-11-08, an excluded package's
unit was emitted with a `PackageExcluded` advisory that no surface
showed, and the build failed with no warning anywhere.

## The reasons are enums

`OmissionReason` holds every reason the plan declines a command:

- `PackageExcluded`: every `packages_added` entry for the owner is
  excluded and the baseline does not ship it (Tier 2).
- `NotCarried`: the owner is in the not-carried class
  (`not_carried.rs`): an auto package nothing included pulls in, which
  the scan excluded. It is chosen over `PackageExcluded` only when every
  `packages_added` entry with the owner's name is in the not-carried
  list; an excluded entry of that name outside the not-carried class
  keeps it `PackageExcluded`. It
  never creates an omission by itself: the unit is omitted only when the
  presence rules already prove the owner absent, which is why a mixed-arch
  owner with one carried arch keeps its unit. Wording is
  `describe(NotCarried)`: `package 'rhc' is not carried: <NOT_CARRIED_REASON>`.
- `NotInTargetImage`: a baseline is available and the owner is in
  neither it nor `packages_added` (Tier 7).
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
  check declines rather than re-resolve it. A timer with no description,
  or an `unresolved` trigger, declines too: an unknown trigger is never
  read as no local trigger. A local trigger is a unit file under
  `/etc/systemd/system` with no vendor file (`local_trigger_missing`); a
  staged file at its path, a generated one included, or a package in the
  image that owns it, provides it. On the collector side
  (`scheduled.rs`, `timer_trigger`), drop-ins are selected with the
  `[Install]` reader's `selected_drop_ins`, so `Unit=` and `[Install]`
  cannot disagree on load order; for an instance timer that includes the
  template's `<prefix>@.timer.d`. An instance timer has no timer file, so
  `scan_instance_timers` describes each instance linked under `/etc`
  (the units inspector's `read_host_links`, the links instances are found
  by) from its template's file, once per root the template sits under.
  Without that description, every carried instance timer `enable` would
  decline as `TimerTriggerUnresolved`.
- `GeneratedNameCollision { name, reserved_by }`: a generated request
  whose files the stager withheld because a collected unit, `name`, has
  one file's name. A FIXME, with exclude advice for `Reservation::Row`
  and rename advice for `Reservation::Inventory`.
- `InstanceNotCarriedYet`: the phase 4 placeholder for every template
  instance.
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

`classify_service_presence`, evaluated in order:

1. No owning package: emit.
2. Every `packages_added` entry for the owner excluded, not in the
   baseline, and not pulled in by an included package: decline with
   `NotCarried` or `PackageExcluded`.
3. An entry included but not installable: emit with `PackageUnreachable`
   (and `BaselineUnavailable` with no baseline).
4. An entry included and installable: emit.
5. The owner in the effective target set (baseline or included): emit.
6. Not found, no baseline: emit with `BaselineUnavailable`.
7. Not found, baseline available: decline with `NotInTargetImage`.

## Aggregate snapshots

An aggregate's `packages_added` is leaf-only, so the tiers that read
absence from it (3, 6, 7) would omit the unit of an auto dependency the
image does install. Aggregate rendering applies Tier 2 alone
(`aggregate_presence`), because an explicit exclusion on an entry that is
present cannot be faked by leaf filtering, and emits everything else.

## What does not show omissions

The audit report (`audit-report.md` and `.html`) lists a unit's row
without saying it was omitted. That is true of Tier 7 as well and was
left as is; no new surface was built for omissions.

## Excluded is not absent

"Excluded" means the package will not be in the image. A package that an
included package depends on is in the image whatever its disposition,
because dnf installs it with the package that needs it, so Tier 2 must
not omit its unit. That covers refine's default exclusion of a non-leaf
package (the scan tarball leaves the same package included) and an
operator's explicit exclusion alike.

`pulled_in_by_included` in `unit_plan.rs` is the one answer:
every auto dependency `leaf_dep_tree` lists under a leaf the export
installs (`installs_on_its_own_route`). An included leaf that renders
only as a manual step (a `# TODO`, the repo-less `# MANUAL` block with
no cached or uploaded RPM, or the repo-enabling `# FIXME` after its
serving repo was dropped and with no upload) is never installed, so it
pulls nothing in. A demoted repo-enabling package that was uploaded
installs from the upload in the repo-less block, so it still counts.
`effective_target_packages` adds those names, so Tier 2 falls through to
Tier 5 for them, in single-host and aggregate mode. The disabled-repo
package selection in `containerfile.rs` asks the same function; do not
write a second copy.

Its limits are the tree's. `leaf_dep_tree` maps each leaf to the
transitive closure of its **auto** dependencies only. With no leaf data
the set is empty, and a leaf that another leaf depends on, or a
dependency of an auto package included on its own, is not recorded. In
those cases an excluded package's unit is still omitted although dnf may
install it, which is the behavior from before the rule existed.

### The not-carried class is this rule applied to auto packages

An auto package that no included package pulls in is excluded by default
on the scan path, so Tier 2 would omit its unit. `classify_auto_carriage`
in `not_carried.rs` decides the class and calls `pulled_in_by_included`
rather than reimplementing it; see `not-carried-auto-packages.md`. The
`/var` evidence in `var_dirs.rs` asks the same predicate, so a package a
leaf the export installs pulls in counts as carried there whatever its
disposition, subject to the repo-enabling demotion filter. Its own
install set uses `installs_on_its_own_route` too, so the two cannot
disagree about which leaves are installed.
