---
name: unit-install-state
description: How the units inspector reads enablement. Install state comes from the links a unit's effective [Install] names, never from systemctl's word; vendor links complete an install and never start one; the [Install] reader, its load order and resets; one line model in core; hidden drop-ins skipped; the search-path refusal; link findings and undetermined records; the Also= walk; the install comparison for a unit at its preset; template instances found from links, and disabled ones derived from the presets and Also=; the default target; test hosts must carry the enable links; the fleet merge of the new lists.
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
  had. **Every listing that decides install state is strict**
  (`read_dir_strict`): the unit roots and dependency directories in
  `read_links`, the drop-in directories in `resolve::conf_files`, the
  drop-in rows' listings in `collect_drop_ins`, the storage pass's root,
  the runtime drop-in reader, the timer trigger reader, both preset
  directories in `read_preset_rules` and the scheduled inspector's timer
  listings (`scan_systemd_timers`). `read_dir` drops an entry its
  iteration fails on and returns the rest, so a lost link, `[Install]`
  drop-in, preset file or local timer would pass silently. Any preset
  read failure leaves the degraded section with no state change and the
  `partial_inventory` inventory, as when no preset can be read: a rule
  missing from the presets can turn a change either way. A failed `/etc`
  timer listing leaves every vendor timer description's trigger
  `Unresolved`, since a local file it hid would load over the vendor one. A test makes a partial
  listing with `UnitHost::partial_listing` (an iteration error after
  leaving entries out) or `MockExecutor::with_dir_iteration_error`.

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
  reader skips a source from any other directory. An instance's needs are
  read from them all the same, through their own selection (below).
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
  plan's rule 5 and rule 7 install-file checks, the timer trigger reader
  and the runtime drop-in reader use the same function: a
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

A link under `/etc` that none of this reads gets a scan warning and
nothing else (`UnreportedLinks`, `unreported_link_warnings`): a masked
unit's link, a link to a template that is not masked, to a unit the
search path refuses (beside the refusal warning), to an alias name, to
a name with no unit file, or to a unit the listing does not print as one
the scan reads. `build_state_changes` records the masked and refused
units it reads; `unexamined_links` resolves every other name with an
`/etc` link that never reached it, skipping a name the gate already
warned about. One warning per cause lists every link, with `cause` and
`links` (`path`, `unit`, and `listed_state`, `search_path` or
`alias_of`) among its fields. They are never read failures: a resolve
error falls back to the not-read cause, so the scan does not degrade.
A dangling link to an instance or to a listed `static` name already
gets the unit's "no unit file" warning and no other. A test host with an
empty listing ends the section before any link is read, so a link case
needs at least one listed unit.

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
unit. Preset lines that list instances (`enable t@.service a b`) apply
only to those instances.

**A disabled instance is derived** (§ 7.1 source 6, § 7.5): it leaves no
link and no listing row, yet the image's package install links it again.
Two kinds of candidate are read like any unit, as `Listed` with no
listing word (`DERIVED_INSTANCE_STATE`):

- From the presets (`preset_instances`, before `build_state_changes`):
  for each listed template whose first matching rule is a bare `enable`,
  its `DefaultInstance=` (read through `also::also_name`); for the list
  form, each listed instance. A `disable` rule, an unknown verb, no rule,
  or a masked template derives none.
- From `Also=` (inside `build_state_changes`): every instance the `Also=`
  walk reaches from a unit whose install state is `Enabled` and whose
  section is `Known`, whether or not that unit is a row. They are read
  after every listed unit, from a worklist, so they are never read twice.
  An `Undetermined` owner has no readable `Also=` and derives none.

Each candidate must pass `derivable`: not a name the listing gave (that is
the own-unit-file case, and also how a name the gate refused stays
refused), not already found by its links, no link under `/etc` at all
(`any_admin_link`), not a `HOST_DEVICE_TEMPLATES` instance. With no link
it is `Disabled`: a `disable` row against the `enable` its template's rule
gives it, or only an inventory entry under its own `disable` preset (the
owner's `Also=` check reports it). An instance with its own unit file is
read by its own name, so a bare template rule does not match it.

**The unknown-template note.** An `Enabled` instance of a template on no
§ 7.5 list (`instance::on_a_list`: the config and data tables and the
device templates) with both lists empty gets `INSTANCE_READS_UNKNOWN`
(in core) as its `attention_reason` (appended to any earlier note). The
note claims no carriage: the plan adds `INSTANCE_CARRIED_ALONE` to
`instance_notes` only once that enable is planned, so an omitted instance
keeps its other notes with no carriage claim (a provisional ruling for the
owner, 2026-11-15).

**What an enabled instance needs** (`instance.rs`). An `Enabled` instance
row records `required_config` (paths under `/etc`, kept whether or not
they exist) and `data_paths` (paths under `/var` that exist, never under
`/var/run` or `/var/lock`, `RUNTIME_VAR_DIRS`). They come from the unit
file (`InstallEvidence::fragment_text`) and the runtime drop-in set, plus
the fixed tables. Non-obvious points:

- **The runtime drop-in set is not the `[Install]` reader's**
  (`runtime_drop_ins`, from `unit_file_find_dropin_paths` in
  `src/shared/dropin.c`, 252 and 257): on every directory of the search
  path (`resolve::unit_search_path`, all twelve, `/run` and
  `/usr/local/lib` included), the `.d` of each of the unit's names
  (`named_drop_in_dirs`: own, template, then the dash-prefix cuts,
  `worker-job@site` giving `worker-.service.d` and
  `worker-@site.service.d`), then the type-wide `service.d` on every
  directory. One file per name, the first directory in that list keeping
  it, applied in file-name order. G2 and the `[Install]` reader stay as
  they are: `enable` does not read `[Install]` from a type-wide or
  dash-prefix directory (measured), while the manager applies their
  `[Service]` settings.
- **A runtime source that cannot be read declines the enable by name.**
  A listing that fails other than `NotFound` (an iteration error too,
  through `read_dir_strict`) or a drop-in that is not text is
  `NeedsUnknown::Unreadable`: no state change, a warning naming the unit
  and the path, and the path in `read_failures`, so the scan degrades.

- **Each setting is read with its own syntax** (`Syntax`), as systemd's
  parser for it reads it (252 and 257, `load-fragment.c`): a command line
  (`Exec*=`) is split into words with quotes and C escapes removed before
  any path is looked for (`extract_first_word`, `cunescape_one`, ports of
  systemd's own), so `--config="/etc/a b/%i.conf"` is one word and one
  path; `EnvironmentFile=`, a condition and `WorkingDirectory=` are one
  path, spaces and quotes included; `ReadWritePaths=` and its kin are
  quoted words; `BindPaths=` takes the source before `:`. A quote opens
  anywhere in a word, as in systemd. Never split a value on whitespace
  before removing quotes: the path goes missing and rule 6 passes.
- **`=`, `:` and `,` split a word only where the syntax calls for it.**
  In a command argument or a setting whose syntax is not modeled, a word
  that starts with a path is that path whole (`/etc/a:b/%i.conf` is one
  path); otherwise the operand is the rest of the word after the first
  `=`, `:` or `,` that a path follows (`operand_in_word`). A path start is
  `/` or a directory specifier and `/` (`%E/x`).
- **A value that cannot be interpreted declines the enable, never drops
  the path.** A modeled setting whose value holds `%i` or `%I` and that
  systemd's parser refuses (unbalanced quote, unknown escape), a path
  operand holding a specifier `expand_unit_specifiers` does not know, and
  a configuration path the form check refuses are `NeedsUnknown`: the
  instance gets no state change and a warning (`needs_unknown_warning`)
  names the unit, the setting's key and the file and line it is on
  (`numbered_logical_lines` in core, `key`, `path` and `line` among its
  fields). **Never the value**: a command line can carry a credential
  (`--token=...` beside a `%E` path), and the redaction pass scrubs file
  contents, not warnings, so the value would reach the snapshot JSON, the
  README and refine's Services section in the clear. A setting whose syntax is not modeled
  (`Description=` and the rest) is read leniently and never declines.
  The plan cannot decline it by name without a snapshot field, so the
  row is left out (schema 28 has none).
- **Resets are applied before paths are derived** (`effective_assignments`,
  `Syntax::resets`, from each parser's empty-value branch in
  `load-fragment.c`, 252 and 257): over the unit file then the drop-ins in
  load order, an empty command, `EnvironmentFile=`, path list,
  `StateDirectory=` or single-path setting clears its own key, an empty
  `BindPaths=` or `BindReadOnlyPaths=` clears both (one list), an empty
  condition clears every condition and an empty assertion every
  assertion, and a single-path setting keeps its last value. A setting
  whose syntax is not modeled keeps every value: a stale path then omits
  an enable, never carries one. A reset is not the `[Install]` reader's:
  that one has its own measured rules.
- **A reset or replacement counts only where systemd applies it**
  (`applies`, `section_takes`, `single_path_accepted`). systemd looks a
  setting up by section and key together and ignores what its table does
  not hold (`conf-parser.c`), so `assignments` keeps each line's section:
  an `EnvironmentFile=` under `[Unit]`, a condition under `[Service]`, a
  condition name systemd does not know (`CONDITION_KEYS`, the same 65 in
  252 and 257), an `X-` section and a line before any section reset
  nothing. The execution settings belong to the type's own section
  (`EXEC_SECTIONS`: service, socket, mount, swap), the commands to the
  service's or socket's list, `PIDFile=` to `[Service]`. A single-path
  parser validates the new value before it frees the old one
  (`load-fragment.c`), so a replacement counts only when it is accepted:
  `WorkingDirectory=` `~` or an absolute path (after `-`), `RootDirectory=`
  and `RootImage=` an absolute path (no `-`), `PIDFile=` any path, a
  relative one under `/run`, none under `/proc`, `/sys` or `/dev` (257);
  each expanded to at most 4095 bytes (`unit_path_printf`, measured on the
  expansion as written, after `WorkingDirectory=`'s `-` and before the
  `/run` prefix: `-/` and 2048 `./` and `srv` replaces nothing although it
  simplifies to `/srv`), then simplified, within `PATH_MAX` and
  `NAME_MAX`, with no `..`. Never check the limit after simplifying. A value
  inspectah cannot expand cannot be judged, so it replaces nothing. An
  ignored assignment that adds a value still adds its path: toward
  omitting. Never apply a reset before checking its section: the earlier
  union kept the path, and a misplaced line would erase a live one.
- **A drop-in that removes a requirement is evidence, never a
  requirement** (`InstanceNeeds::overriding_drop_ins`). Applying a reset
  is right only while the drop-in that holds it is in the image: excluded
  in refine, or under `/run` (never exported), its reset is gone and the
  image reads the earlier path again, which rule 6 never sees. So a
  drop-in whose reset or replacement removed an assignment naming a path
  the two lists would otherwise hold, or that hides a drop-in of its file
  name naming one (systemd reads one file per name), joins the row's
  `install_drop_ins`, as a drop-in hiding an install file does, and rule 5
  withholds the enable unless the export stages it. A removed or hidden
  value that cannot be interpreted, and a hidden file that cannot be read,
  count. A reset that removes nothing for the instance, one followed by
  the same path, and one in the unit file itself record nothing. Never add
  the removed path to `required_config` instead: the included-reset case
  would then omit an enable the host does not need. The plan tells such a
  drop-in from an install file by `unit_plan::install_drop_in_role` (where
  it is, and whether the snapshot's copy holds `[Install]`), so its
  decline is `InstanceNeedsEvidenceChanged`, not the `[Install]` reason,
  and refine's `install_drop_ins` (the `[Install]` badge) leaves it out. A
  vendor drop-in recorded this way is never staged either, so its enable
  is withheld: toward omitting.
- Values are expanded with `SpecifierSet::Runtime`, so `%I` is
  unescaped; `[Install]` never is (`%I` is not expanded there).
- A path counts only when the value itself holds `%i` or `%I`. An
  `EnvironmentFile=` with the `-` prefix and a negated condition (`!`) are
  left out. A triggering condition (`|`) is recorded as required although
  it is one of several alternatives: the spec says every path in the file,
  and the error is toward omitting an enable, never carrying one unchecked.
- The form check (`is_instance_config_path`, `is_instance_data_path` in
  core) is shared by the collector and the snapshot loader, which refuses
  any other value: empty or `.` components, control characters, the root
  itself, and `..` in a data path. A path with a line break would
  otherwise reach a Containerfile comment. The collector first simplifies
  an expanded value as systemd does (`simplify_path`: runs of `/` made
  one, `.` components and a trailing `/` dropped, `..` kept), so
  `/var/lib/x/%i/` and `/etc/x/./%i` are kept, not refused; the loader
  stays strict. Dropping a `required_config` path would leave an enable
  unchecked by rule 6, so every value under `/etc` is kept: a
  configuration path with a `..` component passes the form check as
  written, and rule 6 declines its enable as `Unverifiable` naming it,
  since `Destination` refuses `..` and which file it reads cannot be told.
  A data path with `..` is still dropped: it gives only an advisory.
- Existence is `Executor::file_exists`, true for a directory on both
  executors (`MockExecutor` counts a registered directory).

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
  `install_drop_ins`, `vendor_drop_ins`, `required_config` and
  `data_paths` after `merge_items` picks the representative
  (`union_lists`), so the lists do not depend on which host won.
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
