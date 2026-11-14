# Service omission model

`plan_units` in `crates/pipeline/src/render/unit_plan.rs` is the single
authority for which `systemctl enable`, `disable` and `mask` lines the
Containerfile carries, for every unit kind: services, sockets, paths,
timers, targets, slices, and package-owned mount, automount and swap
units. It reads the snapshot's `units` section
(`UnitSection.state_changes`, `UnitStateChange` in
`crates/core/src/types/units.rs`) and checks each included row against
`staged_writes`, the stager's own decision, so the plan and the files the
export writes cannot disagree. `containerfile.rs` writes the plan's
lines in the `# === Systemd Unit State ===` section, after the
configuration `COPY`, and refine's projection reads the same plan.

## The rules, per verb

`plan_units` runs the rules of § 10.2 of the units spec in this order for
each included state change. The first that fails declines the command.

| Order | Rule | Verbs | Declines with |
|---|---|---|---|
| 1 | Rule 2: a package-owned storage unit (`PackagedStorage`) acts only on its package's file. Any file staged at `/etc/systemd/system/<name>` is what the image would load. | all | `StorageFileStaged` |
| 2 | Rule 3: no `disable` or `mask` of a unit on `TARGET_NEEDED_UNITS` (`bootc-*`, `ostree-*`, `systemd-remount-fs.service`), and no `mask` of `TMP_MOUNT` (`tmp.mount`). Both constants are in `crates/core/src/baseline.rs`. | `disable`, `mask` | `TargetNeedsUnit` (a FIXME) |
| 3 | Rule 4: `systemctl mask` makes a link at `/etc/systemd/system/<name>` after every `COPY` and exits 1 on a regular file there, so the path must be free of any staged file. | `mask` | `MaskPathOccupied` |
| 4 | Rule 5, the row's own file (`rule_5_file`): `Local` needs its unit file staged; `Shadowed` needs its shadow staged; `Vendor` and `PackagedStorage` need no unit file staged at the unit's name. On a fleet aggregate, a staged `Local` or `Shadowed` file whose path holds more than one content variant fails too. | `enable`, `disable` | `UnitFileNotStaged` (`Local`), `InstallEvidenceChanged` (the rest) |
| 5 | Rule 5, the package (`needs_its_package`): the presence tiers below, for `Vendor` and `PackagedStorage` only. | `enable`, `disable` | `PackageExcluded`, `NotCarried`, `NotInTargetImage` |
| 6 | An `enable` of a unit the Scheduled Tasks section carries is left to that section (`deferred`). | `enable` | not declined |

A `mask` that passes rule 4 is planned. It reads no `[Install]` and
succeeds on an absent unit, so rule 5 does not apply to it, and the
presence tiers do not either: a mask of a unit whose package is excluded
is carried.

The presence tiers run only for `Vendor` and `PackagedStorage`, whose
image file comes from the package. A `Shadowed` unit's one condition is
its staged shadow, which the image loads with or without the package, so
a shadowed unit of an excluded package keeps its command. A `Local` unit
has no owner.

Rule 1 is the stager's, not the plan's: `staged_writes` withholds a
zero-length file at a unit path (a mask is carried only as a `systemctl
mask`), and two offers with different content at one destination, or a
file and a directory at one path, are a `Conflict` and both are withheld
and named in a FIXME. Identical content offered twice at one destination
is one write.

**Every rule that asks whether a file is staged asks `is_staged`**, which
looks the path up in `staged_writes` by `Destination`, never by a string
compare, and counts `beside` as well as `writes`. A path spelled two
ways is one destination, and a generated unit in `beside` can be the
only file at a destination whose collected offer was withheld.

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

`AdvisoryReason::PackageUnreachable` and `BaselineUnavailable` are the
emitted-with-context tiers.

`declined_line` is the one place the marker (`# Omitted:` or
`# FIXME:`) and the text of a declined command are built, from
`OmissionReason::describe` or, for `TargetNeedsUnit`,
`target_needs_unit_text` (in `core/baseline.rs`, beside the list it
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
