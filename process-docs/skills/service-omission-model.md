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
every auto dependency `leaf_dep_tree` lists under an included leaf.
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
