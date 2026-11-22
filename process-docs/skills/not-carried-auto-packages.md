# Not-carried auto packages

An auto package is one in `rpm.auto_packages`: dnf recorded it as not
user-installed on the source host. The scan used to count every one of
them as part of the image. It now splits them by whether anything the
image installs needs them, and lists the ones nothing needs as **Not
carried**.

## The class

An entry of `packages_added` is in the class when it is in
`auto_packages` and no default root the export installs pulls it in, as
`unit_plan::pulled_in_by_default_roots` answers. The default roots are
the leaves and the carried unit owners (below). A root counts only when
`installs_on_its_own_route` holds for it: an included root the export
renders only as a manual step (no repo serves it and no RPM is carried,
or a repo-enabling package demoted for a dropped repo with no upload) is
never installed, so its dependencies are not pulled in. A demoted
package that was uploaded installs from the upload, so it still counts.
Identity is `name.arch`.

It is never in the class, and keeps its existing handling, when it is:

- a leaf package;
- locked;
- `source_repo == "anaconda"` (refine's anaconda classifier decides it);
- named in `baseline_package_names` (matched by name);
- `pkg.uploaded` (refine sets it when it stages an upload);
- routed `PackageRoute::Repoless` by `render::repoless::route_for`.

A package routed `DisabledRepo` is not exempt: it can be in the class.

## Defaults and presence count different roots

The class settles the defaults, so it counts the leaves and carried
owners only. The scan decides it while every auto package is still
included: counting every included package would pull in the
dependencies of the packages it is about to exclude, and the class would
change when they were.

Everything that asks whether a package is in the image asks
`unit_plan::export_installs`, which keeps two sets (Mark Russell,
2026-11-21):

- **certain**: every package the Containerfile names on an install line
  (a leaf, a carried owner no installed leaf brings, a locked or
  anaconda package, one installed from its carried RPM, and a dependency
  the disabled-repo transaction names), and nothing else. A package
  present only as another's dependency is not certain, however sure the
  source host's graph is: the target base image may provide what it is
  needed for through a package the host never had, and dnf then leaves
  it out (Mark Russell, 2026-11-21);
- **possible**: that, every included package, and everything an included
  package may pull in: every operand of an `or`, both branches of every
  conditional, and every weak dependency, whatever held on the source
  host (`DepExpr::possible`). A Recommends or Supplements is never
  certain, since dnf skips one it cannot install.

The unit plan writes a package's commands plain when it is certain,
guarded by its unit file when it is only possible, and omits them only
when it is outside both. The not-carried list names a class member only
when nothing included may pull it in (`possibly_pulled_in_by_included`):
**a package is listed as not carried only when the export certainly
leaves it out**. One dnf may install is neither listed nor called
carried; its commands wait for its unit file.

**Package installs and the disabled-repo transaction are the released
ones** (Mark Russell, 2026-11-21). The Containerfile names the packages
beta.6 named, plus the carried unit owners (below), and names a package
only a disabled repo serves in the transaction that enables that repo
when `unit_plan::leaf_tree_pulled_in` holds it: the `leaf_dep_tree`
entries of the installed leaves, what each leaf pulled in on the source
host. Nothing else moves a package into that transaction, and the
possible and certain closures never route anything. Every attempt to
route on them surfaced a new dnf solver failure: naming a provider dnf
may skip makes it mandatory, moving a root into the transaction lets dnf
pick a broken version that repo serves, and a package kept only because
it may be installed made its own dependencies mandatory.

So once an operator includes `tool`, which needs `helper`, `helper`
leaves the list and its recorded `systemctl` commands are emitted,
guarded, although the class still holds `helper` and its disposition
still reads excluded. The commands are guarded because the plan does not
see refine's `original_includes`, so it cannot tell `tool` from an
included auto package the Containerfile does not name; the guard passes,
since dnf installs `helper`. A `helper` only a disabled repo serves is
not routed, as in beta.6: `tool` is no leaf, so that build cannot
resolve it.

**Requirements, not choices.** The collector records two maps (schema
29), both over every leaf and auto package, **leaves included**:

- `dep_closures`: what installing it **certainly** installs. An edge
  counts only when the dependency has exactly one installed provider
  (`DepExpr::required`), and the evaluator only has to be right in that
  direction: anything it is unsure of, such as a condition that holds a
  condition, is left out and guarded downstream. A capability the base
  image, or the requiring package itself, also provides needs no added
  package, and one several added packages provide is a choice dnf makes.
- `dep_alternatives`: what else it **may** install, those providers and
  what they bring.

A root's closure is `unit_plan::closure_of`, at a `Reach`: `Certain`
reads the `dep_closures` entry, `Possible` both. `export_installs` puts
no closure in its certain set; the `/var` evidence
(`pulled_in_by_included`) is what still reads `Reach::Certain`.
`leaf_dep_tree`
is what the leaf pulled in on the host (`RpmDepGraph::held`: each `or`
by its first operand present, each conditional evaluated there), auto
packages only: within the auto part of the leaf's two entries joined,
never wider. The routing reads it, and refine's
`normalize_package_defaults` reads a leaf listed in it as a dependency of
another leaf. Pulled-in membership is transitive in the closures
already, so a pulled-in package's own closure adds nothing.

The class (`pulled_in_by_default_roots`) reads `Reach::Possible`, as it
always counted every provider: an auto package a default root may pull
in stays included by default, as the source host had it. It is not
named on an install line, so its commands are guarded. So is every
package a named one requires: `httpd-core`'s `httpd.service`, which the
leaf `httpd` brings, runs only if its unit file is in the image
(`the_fixture_guards_only_a_dependencys_command`).

The collector's dnf fallback cannot tell one provider from several, so
it records every `dep_closures` entry empty and the whole closure in
`dep_alternatives`: on that route every dependency's command is guarded.
Its graph is what dnf resolved on the host, so the leaf tree, built on
it, routes a leaf's disabled-repo dependency as beta.6 did. The rpm
route is the one real hosts take; the CLI test hosts (`UnitHost`, the
parity fixture) answer its queries, and `UnitHost::rpm_deps_unavailable`
answers the fallback's.

## Unit owners are carried by default

An auto package that owns a unit enabled on the source host is carried
by default (Mark Russell, 2026-11-21, reversing the not-carried default
of 2026-11-10 for this case only). `default_carried_unit_owners` is the
set: auto, not a leaf, outside the existing handling above, and an
owner. **Membership reads the snapshot alone.** It does not depend on
whether something pulls the package in, or the owner of `httpd.service`
(`httpd-core`, pulled in by `httpd`) would start excluded in refine and
be lost when the operator excludes `httpd`. It does not depend on the
tree fallback either, which decides only whether other auto packages can
safely be left out. In the class an owner nothing pulls in is
`AutoCarriage::UnitOwner`; the rest are `Unneeded`.

**Unless it needs a repo the image does not enable** (Mark Russell,
2026-11-21). The owner is named on the plain install line, which enables
no disabled repo, and nothing routes an auto package's dependency into
the transaction that does. So `not_carried::owner_hold` holds back an
owner that comes from a repo the host disabled, or whose possible
closure (`unit_plan::possible_closure`: every provider, every branch,
weak dependencies) holds a package that routes `DisabledRepo`
(`OwnerHold::DisabledRepo`) or that no enabled repo serves by name, a
local RPM or one from a repo the export lacks (`OwnerHold::NoRepo`).
Every closure entry found in `packages_added` counts, whatever its name:
the collector subtracts the base image by `name.arch`, so an added
`libvendor.i686` beside the base image's `libvendor.x86_64` is another
architecture the base image does not provide. Skipping an entry because
its bare name is in `baseline_package_names` named an owner whose
dependency the build cannot fetch
(`another_arch_of_a_base_image_package_holds_its_owner`). The repo each
entry is checked by is its own architecture's (`source_repos`, keyed by
`name.arch`): a bare-name key gave `libvendor.x86_64` from a local RPM
the AppStream repo of `libvendor.i686`
(`a_sibling_archs_repo_never_vouches_for_a_package`).

The closures name installed packages only, so they cannot show a
package the host never had. The collector records the added packages
with a Requires that may need one in `rpm.unresolved_requirements`
(schema 29): `DepExpr::may_need_uninstalled`, an operand no installed
package provides in some branch the image may take, conditions never
read and an `or` only when none of its operands is installed, or a
dependency line the parser could not read. `(vendor-runtime unless
local-switch)` with `local-switch` installed needed nothing on the host
and needs `vendor-runtime` in an image without `local-switch`. An owner
that is, or whose possible closure holds, such a package is
`OwnerHold::Unresolved` (Mark Russell, 2026-11-21;
`a_requirement_the_host_could_not_resolve_holds_its_owner`). The dnf
fallback reads no requirement, so it lists every package and holds
every owner.

A package whose repo the export drops is no better than one no repo
serves. An owner that is, or whose possible closure holds, a package
installed from a host-enabled section of a captured custom `.repo` file
that the build's repo files no longer carry enabled
(`repos_lost_in_export`, an export question asked of
`InstallRepos::effective`) is `OwnerHold::RepoExcluded` (Mark Russell,
2026-11-21; `excluding_a_dependencys_repo_holds_its_owner`). An `or`
does not save it: `agent` requiring `(vendor-runtime or helper)` with
`helper` from an excluded `vendor` has neither operand at build time.
Distro repos, sections a default repo file defines and host-disabled
sections are never lost. A file that still stages, because another of
its sections is kept or an included config entry writes the same path,
still carries the repo, and dnf can fetch from it. This hold is decided
by the export, so it follows refine's repo selection: refine's defaults
are set when a session opens, before any repo is excluded, so
`project_snapshot` runs `hold_owners_losing_a_repo` last, excluding such
an owner while the repo is out and leaving it carried once it is back.
An owner the operator included explicitly is left as they set it: the
last op that reaches it includes it, whether it names the package, a repo
it is attributed to or a group it is a member of
(`RefineSession::packages_included_explicitly`, which must reach the
same packages the projection's cascade does). Counting package ops alone
dropped an owner included through its repo while its profile writes
stayed, and the build failed
(`an_owner_included_through_its_repo_or_group_is_not_held`). The scan's
defaults exclude it with the other holds.

An included `RepoExcluded` owner did not necessarily come from the
operator. The scan classifies `PulledIn` before it asks about holds, so
an owner a leaf may pull in, a recommendation among them, stays included
whatever its hold, and a repo the stager refuses (a `.repo` file name
with a control character, which dnf still reads) is lost in a fresh
scan's own export. Naming that owner turns a weak dependency dnf may
skip into a requested package whose missing requirement fails the build
under dnf's default `strict=True`
(`a_held_owner_the_operator_never_included_is_not_named`). So the
install line names a `RepoExcluded` owner on the operator's word alone:
`RenderContext::operator_includes`, an `OperatorIncludes` refine fills
from `packages_included_explicitly`, the same set the projection's hold
spares. Every other render has none. The operator's include matches
refine's open-time default (the repo was carried then), so the install
line's include-changed test does not name it; `named_unit_owners`, the
carried owners plus the operator's `RepoExcluded` ones, is what the
install line and refine's package list key off.
`default_carried_unit_owners` stays the default set (normalize, scan
defaults, unit guards). A refine-projected snapshot rendered outside
refine has lost its operator includes with the session, as it has its
group states.

Precedence: `RepoExcluded` first (until the repo is back nothing the
operator does to the owner helps), then `DisabledRepo` (the one the
operator can act on by including the owner), then the closure's
`NoRepo` or `Unresolved`.
`unit_owner_holds` is every owner with its hold; `default_carried_unit_owners`
is the ones with none. A held owner is `AutoCarriage::HeldOwner`,
excluded by default and listed with the hold's reason
(`OwnerHold::reason`: "needs a repo disabled on the host", "needs a
package no enabled repo serves", "may need a package the host did not
have", "needs a package from a repo excluded in this export"). Refine's
leaf default excludes it
too, since it is not in the carried set. Included by the operator, it
installs as any auto package an operator includes does, as in beta.6.

"Owns" and "enabled" are `enabled_units_by_owner`. A unit is enabled
when its inventory entry says so, or, for an aggregate, when it is in
`units.enabled_on_any_host`. It is credited to its `state_changes` row's
`owning_package` when it has a row; to the owner of a vendor file of its
own name in `file_ownership`; and, for a template instance with neither,
to the owner of its template's vendor file (`worker@blue.service` loads
`worker@.service`). The collector records no row for an instance
enabled as its preset says, so without the template step its owner went
unseen. A row decides on its own, since its fragment names the file
that loads, a local replacement included. Units in
`INCOMPATIBLE_SERVICES` never count, because refine's `load_for_refine`
drops them from the inventory and the two paths would then disagree.

Three places must carry one, and each asks
`default_carried_unit_owners` rather than deciding it:

| Where | What it does |
|---|---|
| `apply_scan_package_defaults` | excludes only `Unneeded` and `HeldOwner`, so the scan keeps a carried unit owner included |
| refine `normalize_package_defaults` | includes a Site-bucket unit owner after the leaf default, pulled in or not |
| Containerfile leaf filter (`packages_section_lines`) | names an included unit owner in `dnf install` although it is no leaf, unless an installed leaf pulls it in (`pulled_in_by_installed_leaves`) |

The Containerfile asks the leaves only, not every root: two owners that
need each other would each read as pulled in by the other, and neither
would be named. The refine view's leaf filter (`session.rs`) shows the
owners too, so the operator can exclude one. Miss any of the three and
the two exports disagree (`the_install_names_match`) or neither installs
the package (`the_fixture_is_baseline_backed_and_has_a_not_carried_list`).
`excluding_the_leaf_keeps_the_unit_owner_it_pulled_in` pins the
membership rule; `crates/cli/tests/unit_owner_evidence_test.rs` pins the
template instance, the holds and their surfaces
(`an_owner_needing_a_repo_the_image_does_not_enable_is_not_carried_by_default`),
a held owner once included
(`including_an_owner_that_needs_a_disabled_repo_installs_it_as_released`),
a dependency's recorded `disable`, the graph fallback, the released
routing of competing providers, the dnf fallback and an unread
requirement, and a hand-included package's dependency, each from
collector output.

## The one decider

`crates/pipeline/src/render/not_carried.rs` decides membership, through
`classify_auto_carriage`. `apply_scan_package_defaults` excludes the
`Unneeded` and `HeldOwner` parts and `not_carried_packages` lists every
excluded `Unneeded`, `UnitOwner` or `HeldOwner` package nothing installed
pulls in; both reach
the class through the classifier. No function outside the module decides
membership, and the module calls `pulled_in_by_default_roots` and never copies
it. The aggregate guard lives only in `classify_auto_carriage`.

The scan path calls `apply_scan_package_defaults` right after the dnf-cache
repo-less pass (on the scan path the only writer of `repoless_annotation`,
which the classifier reads; refine's `demote_unserved_repo_enabling` also
writes it, but runs later, in a session) and before the `--inspect-only` branch, so the JSON
snapshot and the tarball carry the same dispositions. Nothing after that
point may write the annotation.
`orchestrate::run_pipeline` follows the same order: the dnf-cache pass,
then the defaults, and it stages the cached RPMs after rendering as the
scan does.

## The fallback

`classify_auto_carriage` returns `None`, and so nothing is excluded and
no list is emitted, when the list below holds. The unit owners are still
carried in every case but the first and the last. The collector's graph
fallback, for a host where dnf could not name the user-installed
packages, reads the edges that held on the host and classifies as beta.6
did: a package nothing reaches is a leaf, and every member of a
dependency cycle no root reaches is auto (Mark Russell, 2026-11-21).
Promoting such members to leaves named packages beta.6 never named and
put them outside the owner holds, so a cycle that needs a local RPM
failed the build (`the_graph_fallback_leaves_an_unrooted_cycle_auto`).
Two packages that need only each other therefore give an empty tree
over auto packages, the third case below, and an owner among them still
installs (`the_graph_fallback_still_carries_a_unit_owner`). The cases:

- `leaf_packages` or `auto_packages` is `None` (the collector's
  unavailable or degraded state);
- `leaf_dep_tree` is not a JSON object;
- `leaf_dep_tree` is `{}` while `auto_packages` is not empty, since
  reading that as "no leaf needs anything" would put every auto package in
  the class;
- `aggregate_meta` is set, because an aggregate's `packages_added` is
  leaf-only.

## The fleet list

The merge keeps in `packages_added` only packages that are a leaf on
every host (or, for one not on every host, a leaf where it is), so a
fleet export cannot carry an auto package at all. It does not get the
single-host default; it gets the notice. `FleetDrops` says which
packages the merge provably dropped: in neither `baseline_package_names`
nor `packages_added`, by name, and pulled in by nothing the fleet export
installs, not even as one of several providers
(`possibly_pulled_in_by_included`), a partial package the operator
includes in refine as much as a universal leaf. It needs a baseline
(else a base-image owner such as `openssh-server` would be named),
`leaf_authority_hosts > 0`, not `leaf_packages`, which refine clears
when it opens an aggregate, and a `dep_closures` entry for every package
the export installs (`every_installed_package_has_a_closure`). Without
one, what that package pulls in is unknown, so `FleetDrops::of` is
`None` and every unit emits. A `leaf_dep_tree` entry does not count: it
leaves out a dependency the host also user-installed.
`fleet_not_carried_packages` names the dropped owners of enabled units.
On a fleet, as on one host, only a named package is certain; see
`service-omission-model.md`.
The merge keeps only names, so a fleet entry's `arch` and `source_repo`
are empty and the audit table prints `-`.

Two pieces of per-host evidence survive the merge for this, and nothing
else in it changed:

- **`dep_closures` and `dep_alternatives` map every package the merge
  kept, the partial ones included, from every authoritative host that
  has it.** A partial package (one some hosts lack) is kept as excluded,
  and once the operator includes it, it installs what it needs; without
  its closure a dependency the merge drops would read as absent and lose
  its `systemctl` command. One host's closure cannot prove a package
  absent either, and neither can an auto-only one: a host whose operator
  also installed the dependency records it as a leaf, and the merge
  drops it for not being a leaf on the hosts where it is a dependency.
  The closures keep it. The merged `dep_closures` entry is the
  **intersection** of the hosts' entries: a host where something else
  provides the capability shows the image need not install it. The
  merged `dep_alternatives` entry is everything either map records on
  any host, less that intersection, so the absence claim still rests on
  the union. `leaf_dep_tree` is joined across hosts for the universal
  leaves.
- **`units.enabled_on_any_host`** lists every unit any host enabled. The
  merged inventory keeps the state most hosts have and breaks a tie by
  hostname order, so with the inventory alone an owner one host enabled
  appeared in the notice or not depending on the names of the hosts.

The fleet unit plan reads `FleetDrops` too: a unit whose owner the merge
dropped is omitted (`NotCarried` when the fleet list names the owner,
`NotInTargetImage` otherwise), where it used to fall through to
`aggregate_presence`'s conservative `Emit` and `systemctl enable` a unit
the image does not have, which fails the build. An owner neither certain
nor dropped, an unknown one included, is guarded.

`export_not_carried_packages` is what the export's block, table and
notice read: the fleet list for an aggregate, `not_carried_packages`
otherwise. Refine's view rows and the unit plan read
`not_carried_packages` only, so a fleet has no rows (a row's Include
would name a package the merged snapshot does not hold) and no
pre-download notice; the dialog shows the header entry after download.
`the_fleet_export_notices_the_unit_owners_its_merge_drops` runs the
`inspectah aggregate` binary and a refine export of its tarball over two
collector-built hosts.
`including_a_partial_fleet_leaf_keeps_its_dependencys_unit_command`
includes a partial leaf in refine, with its dependency auto on both
hosts and user-installed where the leaf is.

## Surfaces

- Containerfile: a `# Not carried: <NOT_CARRIED_REASON> (N)` comment block
  with `#   name (unit was enabled)` lines. One unit reads "was enabled",
  several read "were enabled". A held owner's line adds its reason:
  `#   agent (agent.service was enabled; needs a repo disabled on the host)`.
- Audit report: `### Not carried (N)`, columns `Package | Source Repo |
  Enabled Units | Reason`. The Enabled Units cell lists the units with no
  verb, or `-`. The Reason is `NotCarriedPackage::reason`:
  `NOT_CARRIED_REASON`, then `; ` and the hold's reason for a held owner.
- Services: a unit is omitted only when the existing presence rules prove
  its owner absent; the reason is `NotCarried` when every entry of the
  owner's name is in the list, else `PackageExcluded`. On a fleet, an
  owner the merge dropped is proved absent as above.
- `/var`: the evidence counts pulled-in packages as carried, so their
  directories are not provisioned.
- Refine export warnings: one `notice` entry in `x-export-warnings` when
  the export list holds packages that own an enabled unit (by default
  only on a fleet, for a held owner, or once an operator excludes a unit
  owner), naming the count and the first three by display name, a held
  one followed by its reason in parentheses
  (`agent (needs a repo disabled on the host)`), then `and N more`, and
  pointing at the
  audit table and the `# Not carried` block. `not_carried_notice`, in
  this module, builds it from `not_carried_packages` filtered to entries
  with `enabled_units`; it never decides "owns an enabled unit" itself.
  Names are sanitized and bounded because the entry rides in a header;
  the reasons are constants.
  Contract: `refine-export-warnings-header.md`.
- Scan stderr: `inspectah scan` prints the same notice as one
  `Notice: ...` line after it writes the tarball, quiet mode included,
  and nothing with `--inspect-only`. A single host prints it for a held
  owner, or for a snapshot whose owners were excluded before the scan's
  defaults. `inspectah aggregate` prints the
  fleet notice the same way, last.
- Refine view: `GET /api/view` carries the list as
  `not_carried_packages` and the notice as `not_carried_notice`; see
  [Refine projection contract](#refine-projection-contract).

Order on every surface: packages owning an enabled unit first, then the
rest; each part by name, then arch.
The display name is `name`, or `name.arch` when the list holds more
than one arch of that name. All reason wording comes from the constant
`NOT_CARRIED_REASON` and `OwnerHold::reason`.

## What it does not promise

- The class counts only leaves' and carried owners' closures, so an auto
  package that only a hand-included one needs stays in the class, and its
  disposition stays excluded, although dnf installs it. Every surface
  that says whether it is in the image counts it as installed (above).
- An auto package that only another not-carried package needs is in the
  class too, which is correct.
- The routing is beta.6's, with its limits, which are not this class's:
  a dependency of an auto package carried on its own, an owner the
  operator includes or a hand-included package, is never routed into the
  disabled-repo transaction, so such a package that needs a disabled
  repo fails to build; a requirement the collector cannot read routes
  nothing, so a leaf whose unread requirement needs a disabled repo
  fails to build; and the tree names every installed provider of a
  capability, so a provider from a disabled repo that cannot be
  installed fails the build even when another provider would do.
- The hold over-counts on purpose: it reads the possible closure, so an
  owner whose alternative provider comes from a disabled repo is held
  even when dnf would pick one an enabled repo serves. The operator can
  include it.
- A closure is the source host's dependency graph, not the target's: the
  image's dnf resolves against the target repos, which may offer a
  provider the host lacked. That is why only a named package is certain.
- Without leaf data, a dependency cycle no root reaches is auto, as in
  beta.6: with an unrelated leaf beside it, its members are not carried
  and their recorded unit commands are omitted.
- The repo-loss hold knows only the repo a package was installed from:
  a package another enabled repo also serves is held for its owner all
  the same when its own repo is excluded. Including the repo again
  carries the owner.
- The repo-loss hold is applied to the projection, so refine's Packages
  view shows such an owner excluded while the repo is out. An explicit
  include of the owner, its repo or its group stands, and installs as
  any held owner the operator includes does, which fails the build while
  the repo is out if the owner requires the lost package (a weak
  dependency does not).
- The possible side over-counts on purpose. It counts every `or`
  operand and both branches of every conditional without evaluating
  them, so a package the image will not install can stay included by
  default and keep a guarded command; that is the price of never calling
  an installed package absent. A Supplements is the exception: it is
  still judged on the host, so one whose condition fails there pulls
  nothing in, and so does one the parser cannot read. A Requires or
  Recommends it cannot read, such as one nested past its bound, which
  rpm accepts, makes the package possibly need every other added
  package.
- When several added packages provide a capability and the export
  installs none of them itself, dnf installs one: each one's commands
  are guarded, so whichever dnf picks keeps its own. The class counts
  every provider, so an auto package that is only one choice stays
  included by default.
- Providers of a capability are matched by name, as `--whatprovides`
  matches a plain requirement: two arches of one package both count, so
  neither is certain.
- The units shown are for display and ordering. Whether a unit is emitted
  is the presence rules' decision, not this list's.
- Both owners of a unit (a `state_changes` `owning_package` and the vendor
  unit file in `file_ownership`) record the owner by name, so every arch of
  a name is credited with the same units.
- Refine has no code of its own for the class: it reads the dispositions
  its tier defaults produce, and its view shows `not_carried_packages`
  as built here. `crates/cli/tests/not_carried_parity_test.rs`
  holds the two exports equal only for a classified, baseline-backed host;
  read its scope note before claiming more.

## Refine projection contract

`DecisionProjection` (`crates/refine/src/projection/decisions.rs`)
builds both fields from one `not_carried_packages` call on
`snapshot_projected()`, the snapshot the export renders from, and
`build_web_view` copies them into the view payload. Every response that
returns a view (`GET /api/view`, `POST /api/op`, `/api/undo`,
`/api/redo`, `/api/reset`) carries them at the top level:

```json
"not_carried_packages": [
  {
    "item_id": {"kind": "Package", "key": {"name": "firewalld", "arch": "noarch"}},
    "display_name": "firewalld",
    "source_repo": "baseos",
    "enabled_units": ["firewalld.service"],
    "owns_enabled_unit": true
  }
],
"not_carried_notice": {"kind": "notice", "text": "1 package that owns a unit enabled on the source host is not carried: firewalld. See the Not carried table in audit-report.md and the # Not carried block in the Containerfile."}
```

- `not_carried_packages` is always present, in the `# Not carried`
  block's order. It is `[]` when nothing is listed and under the
  classifier's fallback (an aggregate, or no leaf data); the payload does
  not say which.
- `item_id` is the identity `SetInclude` takes. `display_name` is the
  block's name (`name.arch` only when two arches of a name are listed).
  `enabled_units` are sorted unit names, display only.
- `owns_enabled_unit` is `NotCarriedPackage::owns_enabled_unit`, the test
  `not_carried_notice` filters on, so the rows with it set are exactly the
  packages the notice counts. A client reads the flag and never decides
  "owns an enabled unit" itself.
- `not_carried_notice` is the `ExportWarning` the next `POST /api/tarball`
  puts first in `x-export-warnings`, byte for byte, or `null`. Both come
  from `not_carried_notice_for` on the same list, so a client can show it
  before download and skip the header entry equal to it afterwards.

**Including one** is the ordinary package op, no new action (and
`include: false` on a unit owner, which refine carries by default, lists
it):

```json
POST /api/op
{"kind": "Op", "op": "SetInclude",
 "target": {"item_id": <row.item_id>, "include": true}}
```

The response is the new view: the row is gone from
`not_carried_packages`, the notice no longer names it (or is `null`),
and the package appears in `packages` as a changed decision, since the
view's leaf filter keeps a package whose disposition differs from the
session's original. The export then installs it (the `original_includes`
bypass of the Containerfile's leaf filter), drops it from the `# Not
carried` block and the audit table, emits its units again, and stops
provisioning its `/var` directories, which the package now creates.
`include: false` on the same `item_id` (or `/api/undo`) restores all of
it. `including_a_listed_package_carries_it_and_excluding_it_restores_the_list`
in `crates/cli/tests/not_carried_parity_test.rs` pins every surface;
`the_view_carries_the_not_carried_rows_and_notice` in
`crates/web/tests/api_test.rs` pins the payload and the header match.

**Including a package clears its dependencies.** Any package the export
installs credits what it may pull in, from `dep_closures` and
`dep_alternatives` (schema 29): with `firewalld` carried,
`python3-firewall` is not listed, and once an operator includes `tool`,
the `helper` it needs leaves the rows too.

**The UI.** `NotCarriedGroup.tsx` renders the rows in the packages view,
below the package list, in array order, with the owners marked by the
flag; its Include sends the op above from `MainContent.tsx`. It renders
nothing for `[]`, since the payload cannot say why the list is empty.
Its `not-carried-known-limit` paragraph still states the limit above
closed (an included package's dependencies staying listed) and has to
go. The Export dialog's pre-download
notice and its dedupe are in `refine-export-warnings-header.md`.

## Rule for new code

A new consumer that needs "is this package in the image" asks
`export_installs`, and decides what it does with a package that is only
possible, never the leaf filter. The leaf filter says which packages the
operator named, not which ones dnf installs. An absence claim asks
`possibly_pulled_in_by_included`. Both take the whole snapshot, as
`pulled_in_by_default_roots` does, whose carried owners come from the
units section. Only the class may ask `pulled_in_by_default_roots`.

## Measured-host evidence

Observations about where a package came from stay here, attributed to the
hosts they were seen on. The class is defined by `auto_packages` and the
dependency tree, not by origin, and no surface says otherwise.

Measured at commit `e55a45f7` (2026-11-09):

- RHEL 9.8: 38 packages in the class. 18 are recorded by dnf as group
  installs (for example `cronie`, `firewalld`, `rsyslog`,
  `insights-client`, `rhc`, `grubby` and `dracut-config-rescue`); the other
  20 are needed only by those. None was a dependency the tree should have
  caught.
- RHEL 10.2: 24 packages in the class.

The snapshot does not record dnf's install reason, so a surface cannot
say per package why it was not user-installed: on the RHEL 9.8 host the
group-install reason holds for 18 of the 38 and is indirect for the
20 that only they need, and an orphan left by `dnf remove` would fall in
the class too.

Measured at commit `af96710c` (2026-11-09), same hosts, drift and plants
(details in `real-host-validation-runs.md`):

- The scan's list is the same 38 and 24 packages, by `name.arch`. Owners
  of an enabled unit lead: `cronie`, `firewalld`, `insights-client` (RHEL
  9 only) and `rsyslog`. `audit` is not in the class on these hosts,
  because the planted `cockpit` pulls it in.
- No unit was omitted as `NotCarried`: every unit the class owns was
  enabled at its preset, so no `systemctl` line existed to drop.
- An unchanged refine export lists one package more on each host,
  `dnf-plugins-core`, whose only puller is `epel-release`, which refine
  excludes as repo-less. On RHEL 10 the scan keeps that `epel-release`
  included as a manual step, so the scan neither installed nor listed
  `dnf-plugins-core`. A leaf left as a manual step no longer pulls
  anything in, so the scan should now list `dnf-plugins-core` there as
  well; that has not been measured on a host.
- In every image built, `firewalld` and `cronie` are absent and the
  `systemctl` steps succeed.
- A cached `@commandline` package marked as a dependency (`dnf mark
  remove`) stays out of the class and is installed from its cached RPM,
  which confirms the call-site order.

## Related

- `service-omission-model.md`: `OmissionReason::NotCarried` and the
  presence rules that decide whether a unit is omitted.
- `refine-export-warnings-header.md`: the header the export notice rides
  in, and its kinds.
- `package-identity-is-name-dot-arch.md`: why the class keys on `name.arch`.
- `rpm-output-field-parsing.md`: how `leaf_dep_tree`, `dep_closures`
  and `dep_alternatives` are built.
