---
name: unit-install-state
description: How the units inspector reads enablement. Install state comes from the links a unit's effective [Install] names, never from systemctl's word; vendor links complete an install and never start one; the [Install] reader, its load order and resets; one line model in core; hidden drop-ins skipped; the search-path refusal; link findings and undetermined records; the Also= walk; the install comparison for a unit at its preset; template instances found from links; the default target; test hosts must carry the enable links; the fleet merge of the new lists.
---

# Unit install state

`systemctl list-unit-files` says `enabled` when it finds any link to a
unit, and does not say who made it. A unit with
`WantedBy=multi-user.target` and one hand-made `rescue.target.wants/` link
reads `enabled`, and `enable` in the image would make the link the host
never had. So the units inspector reads the install state itself, and
systemctl's word decides almost nothing. § 7.1 to § 7.6 of the units spec
are the contract; this file is where the code is and what is easy to get
wrong.

## Where it lives

All under `crates/collect/src/inspectors/units/`:

| File | What |
|---|---|
| `install.rs` | The `[Install]` reader: `read_install`, `read_vendor_install`, `drop_in_dirs`, `InstallRead`, `InstallSection` |
| `links.rs` | The links and what they mean: `read_host_links` (`HostLinks`), `expected_links`, `install_state`, `link_findings`, `compare_install`, `apply_install_difference`, `HOST_DEVICE_TEMPLATES` |
| `also.rs` | The `Also=` walk: `also_name`, `also_members`, `is_off` |
| `specifiers.rs` | `expand_unit_specifiers` and its `SpecifierSet`: `Install` for the reader, `Runtime` for the scheduled inspector's timer trigger |
| `resolve.rs` | The file a name loads, and the search-path test (`UNREAD_SEARCH_PATH`) |
| `mod.rs` | The name gate (`gate`), `linked_instances`, `install_evidence`, `install_difference`, `build_state_changes`, `default_target` |

## The state is read from the links

`install_state` compares three sets for each unit: the links `enable`
would make from the effective section (`expected_links`: one per
`WantedBy=` and `RequiredBy=` value, one alias per `Alias=`), the links
under `/etc/systemd/system`, and the links packages ship under
`/usr/lib/systemd/system`.

- `Enabled` when every expected link is under `/etc` or shipped, and at
  least one is under `/etc`. `Disabled` when none is under `/etc`.
  Anything between has no state and no row.
- **Vendor links complete an install and never start one.** `disable`
  removes only links under `/etc`, so a unit only a package links reads
  `Disabled`. A shipped link is never a `ManualLink`.
- A section that names no link (none, only `Also=`, or a list a drop-in
  reset) gives no state: enabling it acts only through its members.
- `HostLinks` keys links by the unit they link: a dependency-directory
  link by its own name, an alias by the unit at the end of its whole
  chain, read by `resolve::link_end`, the resolver's own reading. A mask,
  a link to a file outside both unit directories (whatever its name) and
  a chain that does not resolve (dangling, through a path that is not a
  directory, or a loop) are no alias. A chain the collector cannot follow
  (`resolve_final_target` fails on a permission or I/O error on the way)
  or whose end cannot be read is a failed read under `/etc`, never "no
  alias": its end is unknown, and a lost alias reads its unit `Disabled`.
  `link_end` tells the two apart by error kind (`NotFound`,
  `NotADirectory`, `ELOOP` by OS error number). Never attribute an alias by its
  first target's name: `public -> relay -> real` would then count against
  `relay` and leave `real` reading `Disabled`. A link under a parent that
  does not parse is not a finding but still counts as a link to the unit
  (`admin_unparsed_parent`); one whose own name does not parse is a
  warning (`admin_unparsed`).
- Only `/etc` produces rows. Generator links under `/run` are not read.
- A failed read under `/etc` sets `admin_unreadable`. The scan then
  degrades, and no unit that is not masked gets an install state, since a
  missing link would read as `Disabled` and carry a command the host never
  had.

**A unit file or drop-in that cannot be read as text decides nothing
from the listing.** `install_evidence` makes the section
`Undetermined(Unreadable)` with `assigns_also` true, so the unit has no
install state, its `/etc` links are `Undetermined` findings, it always
gets an `UndeterminedInstall` record, and a warning names the files
(`unreadable_install_warning`, with a `paths` field). (Mark Russell,
2026-11-15.) The listing's word is still kept on the degraded path where
no preset rule could be read (`partial_inventory`); that is an open point
for the spec owner, not a rule to copy.

**An undetermined section keeps its unit out of the storage pass.**
`build_state_changes` puts every unit whose section is `Undetermined`, for
any reason (`Specifier`, `UpheldBy`, `Unreadable` and the rest), in
`reported`, so `etc_storage_findings` never records it as a storage unit
that is "not enabled": its enablement is unknown, and its
`UndeterminedInstall` record and block are what the export says about
it. An `/etc` storage unit's own file is one of its install files, so an
undetermined section there always has a record and never a state.

## The `[Install]` reader

`read_install` reads the unit file, then one drop-in per file name across
every directory of `drop_in_dirs` and every root, in file-name order. Of
drop-ins that share a file name, the first in systemd's search order wins
(G2): every directory of the instance's own name before every directory of
its template's, and within each `DropInRoot`'s order: a directory ranked
above `/etc` (`AboveEtc`, the four `UNREAD_SEARCH_PATH` entries tagged
`Etc`, in search-path order), then `/etc`, then the vendor one. So a
vendor file in `t@x.service.d` hides an `/etc` file of its name in
`t@.service.d`, and `systemctl set-property`'s
`/etc/systemd/system.control/<unit>.d/50-CPUQuota.conf` hides an `/etc`
`50-CPUQuota.conf` in `<unit>.d`, so an `[Install]` in the hidden file is
not read (Mark Russell, 2026-11-15). `install_evidence` reads the
`AboveEtc` drop-ins only for this: one that holds `[Install]`, shares a
vendor drop-in's name or cannot be decoded has already refused the unit in
`resolve`, so one that reaches the reader changes nothing but what it
hides. A hidden `/etc` file stays in `install_drop_ins`, since the image
reads it if the higher file is not there. **The hiding file is install
evidence too**: when it hides an `/etc` install drop-in, `install_evidence`
records it in `hiding_drop_ins`, and the row's `install_drop_ins` carries
both (`row_install_drop_ins`), so rule 5 withholds the command unless the
export stages the hiding file as well: never for one under `/run`, and for
an `/etc/systemd/system.control` one only while its config entry is
included. Without that, the scan reads the unit
as the host does and the image does not: a `disable`-preset unit whose
hidden drop-in adds `WantedBy=graphical.target` gets an `enable` that
makes a link the host never had. Which file hides is computed by the
reader's order: per file name, the above-`/etc` drop-in with the lowest
directory rank, the first in search-path order among equals, hides an
`/etc` drop-in of its name in a directory of that rank or after it. Only
the row gets the hiding file: the § 7.4 gate, the vendor-side read and the
findings' and records' `install_files` keep the `/etc` list, whose block
says "these files under /etc/systemd/system".

Within one directory name the `/etc` file winning over the vendor one was
measured. **Still needs a real-host run on RHEL 9 and RHEL 10:**

- the order across the two directory names (instance `.d` before template
  `.d`), read from systemd's source (252 and 257);
- the placement above `/etc`: a `system.control` drop-in (from
  `systemctl set-property`) hiding an `/etc` drop-in of its name, so that
  `systemctl enable` does not act on an `[Install]` in the hidden file,
  and the same against a vendor drop-in and across the instance and
  template `.d`, with the G2 order above.

- **Resets, as measured on RHEL 9 and 10:** an empty `WantedBy=` or
  `Alias=` clears the values before it; an empty `Also=` clears nothing.
  An empty `RequiredBy=` after a value is `Undetermined(RequiredByReset)`,
  since its effect was not measured.
- **Only the unit's own `.d` and, for an instance, its template's.** The
  type-wide (`service.d`) and dash-prefix (`a-.service.d`) directories
  apply to the unit's other sections but `enable` does not read
  `[Install]` there (measured), so `drop_in_dirs` never lists them and the
  reader skips a source from any other directory.
- **`Undetermined`** for any `UpheldBy=`, that `RequiredBy=` reset, a
  specifier other than `%i`, `%p`, `%N` and `%%`, or a value that is not a
  `UnitName`. `%I` is undetermined here: `systemctl enable` does not expand
  it (systemd's install expansion table, 252 and 257), while the scheduled
  inspector expands it in a timer's `Unit=`. `SpecifierSet` in
  `specifiers.rs` holds the two sets; never pass `Runtime` to the
  `[Install]` reader. The first reason in load order stands, even if a later
  reset clears the value that caused it. `assigns_also` is true for any
  `Also=` line, an empty one included, which sends the unit to the
  advisory side rather than a `disable`.
- **Hidden drop-ins are skipped.** `conf_files` drops a name starting with
  `.`, as systemd does when it lists drop-ins, so an editor's
  `.#override.conf` is never read. That was read from systemd's source,
  not measured. The rule is `systemd_reads_drop_in` in core, and the
  plan's rule 5 and rule 7 install-file checks use the same function: a
  hidden file can still be collected and staged as a drop-in row, and a
  check that counted it would decline a command over a file systemd never
  reads.

**One line model, in core.** `logical_lines` and `holds_install_section`
are in `crates/core/src/types/units.rs`, and the collector, the scheduled
inspector and the pipeline all use them. Lines end at `\n`, `\r` or `\0`,
comments are skipped, a trailing backslash continues the line.
A splitter that ends lines at `\n` alone misreads an `[Install]` header
after a lone `\r`, so the drop-in changes the section the collector reads
and still escapes `install_drop_ins` and rule 5. Never write a second
splitter.

## Install files and the search path

An install file of a name `N` is the unit file at `N` (for an instance,
its template's too), or a `.conf` in `N.d` (or the template's `.d`) that
holds an `[Install]` section or has the file name of a vendor drop-in
there. `install_evidence` reads each directory once and feeds both the
reader and the list, so the two cannot disagree. A drop-in whose text
cannot be read counts as an install file and makes the section
`Undetermined(Unreadable)`.
`vendor_drop_ins` on a row holds file names only; the plan checks names,
never content. `HostLinks::has_etc_files` is the gate for a unit with a
file or drop-in directory of its own name under `/etc`.

`UNREAD_SEARCH_PATH` lists the other nine directories on systemd's unit
search path, each tagged with the highest of the two read directories it
outranks. After the fragment is chosen, a file there for the chosen name
that outranks it, an instance's own file in any of them, or an `[Install]`
drop-in in any of them refuses the unit with a warning naming the path,
whatever its state: the scan cannot tell what would load or what `enable`
would do. `/usr/local/lib/systemd/system` outranks only the vendor
directory. A drop-in there that cannot be decoded counts as a hit. A
drop-in in a directory above `/etc` that is no hit is still read by the
`[Install]` reader as `AboveEtc` (`resolve::above_etc_dirs`), because it
hides an `/etc` drop-in of its file name.

## What becomes a finding

`link_findings` reads § 7.3's table for each unit:

- **`ManualLink`** for each link under `/etc` the unit's `[Install]` does
  not account for: `StaticUnit`, `NotInInstall`, `PartialInstall`,
  `Undetermined` (with its reason) or `HostDevice` (an instance of
  `HOST_DEVICE_TEMPLATES`, never carried). § 7.4's comparison adds
  `HostInstallOnly` and `VendorInstallOnly`, and relation `Also` for an
  `Also=` name only one side assigns. `PartialInstall` is only for a link
  the section names: a partial unit's link it does not name is
  `NotInInstall`, as for an `Enabled` or `Disabled` unit, so the
  "Present:" line never lists a link the section does not make (Mark
  Russell, 2026-11-15).
- **`UndeterminedInstall`**, one per unit, for an `Undetermined` section
  that assigns `Also=` or has an install file under `/etc`. Such a unit has
  no state and no row, so the record is what keeps it from disappearing
  silently. The section can be the package's: see the comparison below.

The advisory text for both is § 10.9's, built in
`crates/pipeline/src/render/unit_plan.rs` (`manual_link_lines`,
`undetermined_install_lines`, `install_omitted_line`) and nowhere else.
`UnitSection.manual_links` and `undetermined_installs` are required on
load, so hand-built snapshot JSON needs both keys; `default_target` is
skipped when `None`.

## A unit at its preset whose `/etc` files change `[Install]`

The package install links a preset-enabled unit from the vendor
`[Install]` before any `COPY`. `install_difference` in `mod.rs` decides
when the comparison applies (a `Vendor` or `Shadowed` fragment, an
`enable` preset, an install file under `/etc`, and `Enabled` or no state),
`read_vendor_install` reads the package's side with nothing from `/etc`,
and `compare_install` compares link sets and `Also=` name sets, never
text. `Same` does nothing, so an unchanged shadow is a no-op. `Additive`
makes an `Enabled` row with an attention note. `NotAdditive` makes one
finding per difference, and a link the host has is never advised for
removal. `VendorUndetermined` (the package's section has an `UpheldBy=`,
a specifier and the rest) is handled as an effective-side `Undetermined`
section with an `/etc` install file: no state, one `UndeterminedInstall`
with the package section's reason and the `/etc` install files, whether
or not the host has a link, each `/etc` link an `Undetermined` finding,
and the unit kept out of the storage pass (Mark Russell, 2026-11-15).
The record has no field saying which side could not be read; the block's
text is the same for both. A `static` unit with `/etc` files passes the name gate so that
this comparison can run. `Also=` names are compared after `also_name` maps
a template to its `DefaultInstance=` on both sides.

## `Also=` members

`also_members` follows `Also=` through each member's effective unit until
no new name appears and records each with its `MemberLinks`. `also_name`
is the one place a template in `Also=` becomes its `DefaultInstance=`;
with none, the member is recorded under the template's name as `Other`
and the walk stops there. `MemberLinks` counts every `/etc` link to a
member, not only expected ones, because `disable` removes them all. An
owner at its `enable` preset whose member `is_off` gets an `Enabled` row
with a note, so rule 7 turns it into an advisory; that check runs even
when the install comparison differs. How the plan reads the list is in
[service-omission-model](service-omission-model.md), rule 7.

## Template instances

`list-unit-files` shows an instance backed only by its template as the
template reading `indirect`, so `linked_instances` finds instances from
their links under `/etc`. It takes every name the listing gave, before the
gate: **a link-found instance must never re-admit a name the gate
refused** (`bad`, `transient`, a runtime state). A listed instance, a
concrete instance file or a masked one, goes through the gate like any
unit. In phase 4 every instance is recorded and then declined by the
`InstanceNotCarriedYet` placeholder in `plan_units`, which runs before
every rule; Task 5.2 removes it. Preset lines that list instances
(`enable t@.service a b`) apply only to those instances.

## The default target

`default_target` reads the link at `DEFAULT_TARGET_LINK` and records a
`DefaultTarget`, included, whatever the target is. A link to anything
that is not a target with a unit file the scan reads is a warning and no
record. In the plan, `PlannedTarget` is built only in
`plan_default_target`, and `systemctl_lines` takes the planned target
first, so `set-default` is the section's first command; the section's
`# Omitted:` lines come before it.

## Test hosts must carry the enable links

Since the state comes from the links, a test host that lists a unit
`enabled` without its links scans as not enabled.

- `UnitHost` (`crates/cli/tests/support/unit_hosts.rs`) makes the links
  `enable` would make for every unit listed `enabled` that has none under
  `/etc` yet (`add_enable_links`). A test about a hand-made or missing link
  places the link itself.
- `UnitHost::owned_file` makes a package-owned `/etc` file.
- In `crates/collect/tests/units_test.rs`, `with_enable_link` and
  `with_enable_links` add the links; `MockExecutor::with_dir_entry` adds a
  directory entry without replacing the listing.
- An instance's link is placed by hand, pointing at the template file,
  since `UnitHost` makes links only for listed `enabled` units.

## Fleet merge

- `merge_service_sections` unions a state change's `also`,
  `install_drop_ins` and `vendor_drop_ins` after `merge_items` picks the
  representative, so the lists do not depend on which host won.
- Link findings and undetermined records keep the first host's version by
  hostname order (`merge_by_first_host`).
- The default target keeps the most common target, excluded unless every
  host sets it (`merge_default_target`).
- An aggregate starts refine with added packages excluded, so a refine
  test on an aggregate must include the package before it can see a
  command.

## Related

- [etc-systemd-system-ownership](etc-systemd-system-ownership.md): who owns each thing under `/etc/systemd/system`
- [service-omission-model](service-omission-model.md): what the plan does with the rows and findings
- [unit-state-correctness](unit-state-correctness.md): the staging decision and proved types
- [real-host-validation-runs](real-host-validation-runs.md): the measured systemd behavior this rests on
