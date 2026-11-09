# Not-carried auto packages

An auto package is one in `rpm.auto_packages`: dnf recorded it as not
user-installed on the source host. The scan used to count every one of
them as part of the image. It now splits them by whether anything the
image installs needs them, and lists the ones nothing needs as **Not
carried**.

## The class

An entry of `packages_added` is in the class when it is in
`auto_packages` and no leaf the export installs pulls it in, as
`service_intent::pulled_in_by_included` answers from `leaf_dep_tree`.
A leaf counts only when `installs_on_its_own_route` holds for it: an
included leaf the export renders only as a manual step (no repo serves
it and no RPM is carried, or a repo-enabling package demoted for a
dropped repo) is never installed, so its dependencies are not pulled in.
Identity is `name.arch`.

It is never in the class, and keeps its existing handling, when it is:

- a leaf package;
- locked;
- `source_repo == "anaconda"` (refine's anaconda classifier decides it);
- named in `baseline_package_names` (matched by name);
- `pkg.uploaded` (refine sets it when it stages an upload);
- routed `PackageRoute::Repoless` by `render::repoless::route_for`.

A package routed `DisabledRepo` is not exempt: it can be in the class.

## The one decider

`crates/pipeline/src/render/not_carried.rs` decides membership, through
`classify_auto_carriage`. `apply_scan_package_defaults` excludes the
class and `not_carried_packages` builds the display list; both reach the
class through the classifier. No function outside the module decides
membership, and the module calls `pulled_in_by_included` and never copies
it. The aggregate guard lives only in `classify_auto_carriage`.

The scan path calls `apply_scan_package_defaults` right after the dnf-cache
repo-less pass (on the scan path the only writer of `repoless_annotation`,
which the classifier reads; refine's `demote_unserved_repo_enabling` also
writes it, but runs later, in a session) and before the `--inspect-only` branch, so the JSON
snapshot and the tarball carry the same dispositions. Nothing after that
point may write the annotation.

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

Order on every surface: packages owning a unit in `services.enabled_units`
first, then the rest; each part by name, then arch. The display name is
`name`, or `name.arch` when the list holds more than one arch of that
name. All reason wording comes from the one constant `NOT_CARRIED_REASON`.

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
  its tier defaults produce. `crates/cli/tests/not_carried_parity_test.rs`
  holds the two exports equal only for a classified, baseline-backed host;
  read its scope note before claiming more.

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
- `package-identity-is-name-dot-arch.md`: why the class keys on `name.arch`.
- `rpm-output-field-parsing.md`: how `leaf_dep_tree` is built.
