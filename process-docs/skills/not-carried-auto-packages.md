# Not-carried auto packages

An auto package is one in `rpm.auto_packages`: dnf recorded it as not
user-installed on the source host. The scan used to count every one of
them as part of the image. It now splits them by whether anything the
image installs needs them, and lists the ones nothing needs as **Not
carried**.

## The class

An entry of `packages_added` is in the class when it is in
`auto_packages` and no leaf the export installs pulls it in, as
`unit_plan::pulled_in_by_included` answers from `leaf_dep_tree`.
A leaf counts only when `installs_on_its_own_route` holds for it: an
included leaf the export renders only as a manual step (no repo serves
it and no RPM is carried, or a repo-enabling package demoted for a
dropped repo with no upload) is never installed, so its dependencies are
not pulled in. A demoted package that was uploaded installs from the
upload, so it still counts.
Identity is `name.arch`.

It is never in the class, and keeps its existing handling, when it is:

- a leaf package;
- locked;
- `source_repo == "anaconda"` (refine's anaconda classifier decides it);
- named in `baseline_package_names` (matched by name);
- `pkg.uploaded` (refine sets it when it stages an upload);
- routed `PackageRoute::Repoless` by `render::repoless::route_for`.

A package routed `DisabledRepo` is not exempt: it can be in the class.

## Unit owners are carried by default

A package in the class that owns a unit enabled on the source host is
`AutoCarriage::UnitOwner`; the rest are `Unneeded` (Mark Russell,
2026-11-21, reversing the not-carried default of 2026-11-10 for this case
only). "Owns" and "enabled" are `enabled_units_by_owner`: an inventory
entry with `state == Enabled`, credited to a `state_changes`
`owning_package` or to the owner of the vendor unit file in
`file_ownership`, by name. Units in `INCOMPATIBLE_SERVICES` never count,
because refine's `load_for_refine` drops them from the inventory and the
two paths would then disagree.

Three places must carry one, and each asks
`default_carried_unit_owners` rather than deciding it:

| Where | What it does |
|---|---|
| `apply_scan_package_defaults` | excludes only `Unneeded`, so the scan keeps a unit owner included |
| refine `normalize_package_defaults` | includes a Site-bucket unit owner after the leaf default, from the leaf dispositions it just settled |
| Containerfile leaf filter (`packages_section_lines`) | names an included unit owner in `dnf install` although it is no leaf |

The refine view's leaf filter (`session.rs`) shows them too, so the
operator can exclude one. Miss any of the first three and the two exports
disagree (`the_install_names_match`) or neither installs the package
(`the_fixture_is_baseline_backed_and_has_a_not_carried_list`).

## The one decider

`crates/pipeline/src/render/not_carried.rs` decides membership, through
`classify_auto_carriage`. `apply_scan_package_defaults` excludes the
`Unneeded` part and `not_carried_packages` lists every excluded
`Unneeded` or `UnitOwner` package; both reach the class through the
classifier. No function outside the module decides
membership, and the module calls `pulled_in_by_included` and never copies
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
no list is emitted, when:

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
single-host default; it gets the notice. `fleet_not_carried_packages`
names the owners of enabled units (merged inventory and ownership) that
are in neither `baseline_package_names` nor `packages_added`, by name,
and that no installed fleet leaf pulls in. It needs a baseline (else a
base-image owner such as `openssh-server` would be named) and
`leaf_authority_hosts > 0`, not `leaf_packages`, which refine clears when
it opens an aggregate. The merge keeps only names, so a fleet entry's
`arch` and `source_repo` are empty and the audit table prints `-`.

`export_not_carried_packages` is what the export's block, table and
notice read: the fleet list for an aggregate, `not_carried_packages`
otherwise. Refine's view rows and the unit plan read
`not_carried_packages` only, so a fleet has no rows (a row's Include
would name a package the merged snapshot does not hold) and no
pre-download notice; the dialog shows the header entry after download.
`the_fleet_export_notices_the_unit_owners_its_merge_drops` runs the
`inspectah aggregate` binary and a refine export of its tarball over two
collector-built hosts.

## Surfaces

- Containerfile: a `# Not carried: <NOT_CARRIED_REASON> (N)` comment block
  with `#   name (unit was enabled)` lines. One unit reads "was enabled",
  several read "were enabled".
- Audit report: `### Not carried (N)`, columns `Package | Source Repo |
  Enabled Units | Reason`. The Enabled Units cell lists the units with no
  verb, or `-`.
- Services: a unit is omitted only when the existing presence rules prove
  its owner absent; the reason is `NotCarried` when every entry of the
  owner's name is in the list, else `PackageExcluded`.
- `/var`: the evidence counts pulled-in packages as carried, so their
  directories are not provisioned.
- Refine export warnings: one `notice` entry in `x-export-warnings` when
  the export list holds packages that own an enabled unit (by default
  only on a fleet, or once an operator excludes a unit owner), naming the count and
  the first three by display name, then `and N more`, and pointing at the
  audit table and the `# Not carried` block. `not_carried_notice`, in
  this module, builds it from `not_carried_packages` filtered to entries
  with `enabled_units`; it never decides "owns an enabled unit" itself.
  Names are sanitized and bounded because the entry rides in a header.
  Contract: `refine-export-warnings-header.md`.
- Scan stderr: `inspectah scan` prints the same notice as one
  `Notice: ...` line after it writes the tarball, quiet mode included,
  and nothing with `--inspect-only`. Since the scan carries every unit
  owner, a single host prints it only for a snapshot whose owners were
  excluded before the scan's defaults. `inspectah aggregate` prints the
  fleet notice the same way, last.
- Refine view: `GET /api/view` carries the list as
  `not_carried_packages` and the notice as `not_carried_notice`; see
  [Refine projection contract](#refine-projection-contract).

Order on every surface: packages owning an enabled unit in
`units.inventory` first, then the rest; each part by name, then arch.
The display name is `name`, or `name.arch` when the list holds more
than one arch of that name. All reason wording comes from the one constant `NOT_CARRIED_REASON`.

## What it does not promise

`leaf_dep_tree` maps each leaf to the transitive closure of its **auto**
dependencies and records nothing else.

- A dependency of an auto package that an operator includes on its own is
  not recorded, so it stays in the class although dnf installs it with that
  package.
- An auto package that only another not-carried package needs is in the
  class too, which is correct.
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

**Known limit, for the UI to explain:** a carried auto package, a unit
owner by default or one an operator included, does not credit its own
dependencies as pulled in (the tree records leaf closures only), so they
stay in `not_carried_packages` although dnf installs them with it. With
`firewalld` carried, `python3-firewall` is listed. Since unit owners are
carried by default this holds on every stock RHEL host (`cronie-anacron`
under `cronie`, for example). Nothing in the payload marks these rows;
it is text for the UI. Fixing it needs the collector to record the
closure of each auto package, which is a snapshot change.

**The UI.** `NotCarriedGroup.tsx` renders the rows in the packages view,
below the package list, in array order, with the owners marked by the
flag; its Include sends the op above from `MainContent.tsx`. It renders
nothing for `[]`, since the payload cannot say why the list is empty.
The group text states the known limit. The Export dialog's pre-download
notice and its dedupe are in `refine-export-warnings-header.md`.

## Rule for new code

A new consumer that needs "is this package in the image" asks the
disposition plus `pulled_in_by_included`, never the leaf filter. The leaf
filter says which packages the operator named, not which ones dnf installs.

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
- `rpm-output-field-parsing.md`: how `leaf_dep_tree` is built.
