# Service omission model

`render_service_intent` in `crates/pipeline/src/render/service_intent.rs`
is the single authority for which `systemctl enable`, `disable` and
`mask` lines the Containerfile carries. `containerfile.rs` delegates its
service section to it, and refine's projection reads the same plan.

## Omission versus advisory

A unit whose owning package is not in the image cannot be acted on:
`systemctl enable` on a missing unit fails the build with
`Failed to enable unit, unit <name> does not exist`. So the engine has
two outcomes, and they are not interchangeable:

- **Omission** (`ServiceOmission`): the unit is left out of every
  `systemctl` line and recorded. It renders as a
  `# Omitted: <unit> (<reason>)` comment in the Containerfile and as a
  row in refine's Omitted Services list.
- **Advisory** (`ServiceAdvisory`): the unit is emitted, with context
  saying its presence could not be proved. An advisory is supplemental;
  it never removes the unit.

Choose omission only when the package is known to be absent. An advisory
on a unit that will fail the build is not a mitigation: until
2026-11-08, an excluded package's unit was emitted with a
`PackageExcluded` advisory that no surface showed, and the build failed
with no warning anywhere.

## The reasons are enums

- `OmissionReason::PackageExcluded`: every `packages_added` entry for the
  owner is excluded and the baseline does not ship it (Tier 2).
- `OmissionReason::NotCarried`: the owner is in the not-carried class
  (`not_carried.rs`): an auto package nothing included pulls in, which
  the scan excluded. It is chosen over `PackageExcluded` only when every
  `packages_added` entry with the owner's name is in the not-carried
  list; one carried arch of that name makes it `PackageExcluded`. It
  never creates an omission by itself: the unit is omitted only when the
  presence rules already prove the owner absent, which is why a mixed-arch
  owner with one carried arch keeps its unit. Wording is
  `describe(NotCarried)`: `package 'rhc' is not carried: <NOT_CARRIED_REASON>`.
- `OmissionReason::NotInTargetImage`: a baseline is available and the
  owner is in neither it nor `packages_added` (Tier 7).
- `AdvisoryReason::PackageUnreachable` and `BaselineUnavailable` for the
  emitted-with-context tiers.

`OmissionReason::describe` is the only wording source. The Containerfile
comment and the web adapter's subtitle both call it, so the surfaces
cannot drift. `RefOmittedService` carries the enum and is not serialized
(`Debug, Clone` only); the wire form is the adapter's subtitle string.

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

`pulled_in_by_included` in `service_intent.rs` is the one answer:
every auto dependency `leaf_dep_tree` lists under a leaf the export
installs (`installs_on_its_own_route`). An included leaf that renders
only as a manual step (a `# TODO`, the repo-less `# MANUAL` block with
no cached or uploaded RPM, or the repo-enabling `# FIXME` after its
serving repo was dropped) is never installed, so it pulls nothing in.
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
