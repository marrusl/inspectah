---
name: unit-state-correctness
description: Correctness requirements found while building unit state for every kind. A type that feeds an unchecked constructor of a proved type has no public fields, and a source-level test guards PlannedUnit because stable rustdoc does not check compile_fail error codes; zero length is decided without decoding a unit file, and a file that is not text never degrades the scan; every staged-file question goes to staged_writes through Destination, which covers every file under config/, drop-ins/ and flatpak/ except the flatpak manifest and the tmpfiles fragment; settings are extracted from logical lines; a path is one destination per staging filesystem, not only per spelling; absence tests on systemctl lines use the strict helper.
---

# Unit State Correctness

Requirements the unit state work turned up, each with the reason. The
rules themselves are in [service-omission-model](service-omission-model.md).

## A type that feeds a proved type's unchecked constructor has no public fields

`UnitName` is proved once, by its parser, and code downstream trusts it.
Any type that can produce one, or a value built from one, without that
parser is a way around the proof. So each of these keeps its fields
private and offers accessors, and deserializes through a checked wire
form where it is serialized:

- `UnitName` (`crates/core/src/types/units.rs`): the parsed name.
- `UnitSubject`: a name and its fragment, checked together (the template
  and absent rules).
- `UnitStateChange`: its `unit` and `current_state`. A struct literal
  with public fields could pair a unit with a state the collector never
  produces.
- `PlannedUnit` (`crates/pipeline/src/render/unit_plan.rs`): only
  `plan_units` builds one, so `systemctl_lines`, which takes nothing
  else, cannot write a command the plan did not pass.
- `Destination` (`crates/pipeline/src/render/configtree.rs`): only
  `Destination::new` normalizes a path, so two spellings of one path are
  one destination everywhere.

**Stable rustdoc does not check a `compile_fail` error code.** A
`compile_fail,E0451` doctest passes on any compile error, a typo
included, and a struct literal cannot see a public constructor added
later. `planned_unit_has_no_public_field_or_constructor` in
`crates/pipeline/tests/unit_plan_test.rs` reads the source instead: the
field list, the derives, every `impl` block and every public function
returning the type. Guard a new proved type the same way.

## Zero length is decided without decoding the file

A zero-length unit file is a mask; a file with any byte is not (a file
holding only a comment is a broken unit, see
[real-host-validation-runs](real-host-validation-runs.md)). The executor's
`read_file` fails with `InvalidData` for content that is not UTF-8 or is
over its size cap, and that error proves the file has bytes. So
`resolve.rs` reads an `InvalidData` result as "exists, not empty", never
as a read failure. systemd loads a unit whose comment or `Description=`
holds Latin-1 bytes, so a unit file, full shadow or drop-in that is not
valid UTF-8 keeps its unit's state change and does not degrade the scan.
Only a real read failure degrades it.

## Every staged-file question asks `staged_writes` through `Destination`

Rules 2, 4 and 5 and the mount advisories ask whether a file is staged
with `is_staged`, which looks the path up in `staged_writes` by
`Destination`, never by comparing strings. `/etc/systemd/system//x.mount`
and `/etc/systemd/system/x.mount` are one destination, and a string
compare would answer differently from the stager. `is_staged` reads
`writes`, the one list of what lands.

**A generated unit never lands beside or over a collected one.** The
interim rules of phase 2 are retired: generated and flatpak offers
written `beside` a collected file, the per-unit withhold that followed,
and `plan.deferred`, which left a collected unit's `enable` to the
Scheduled Tasks section. In their place, rule 3 of `staged_writes` is the
generated-name collision of § 10.2, and for this rule only a collected
unit is any unit the units section names (`collected_unit_paths`):

- `Reservation::Row`: an included state change (its unit path), or an
  included config entry or drop-in row at the generated path, whatever
  its content, a zero-length mask file included. Excluding that row and
  its file lets the generated unit back in.
- `Reservation::Inventory`: an inventory entry or mount finding with no
  state change, such as a package's unit at its preset. It has no row to
  exclude, so the advice is to write the timer and its service by hand
  under other names. A unit with a state change is decided by that row
  alone, included or excluded, which keeps the flatpak rescan case as it
  was.

Every file of a generated unit with such a name is withheld as
`WithheldWhy::GeneratedNameCollision { unit, reserved_by }`, the paired
service of a generated timer included, and the plan declines its request
with `OmissionReason::GeneratedNameCollision { name, reserved_by }`.
`name` is the file that collided, not the generated unit: a collision on
the paired service alone must name the service, or the FIXME describes a
unit the host does not have. Two
cron units of one name are one destination twice and the last one is
written; every other clash at one destination is an ordinary
`Conflict`.

**No other writer under `/etc/systemd/system`.** The unmanaged-files
section copies its own tree after the unit state section, outside
`staged_writes`. `configtree::unmanaged_withheld`, by `Destination`, is
the one decision on an unmanaged path: a path under the unit directory
(`unmanaged_unit_path`, every path below it) is `UnmanagedUnitPath`, and
the machine-id is `MachineId`. The scan's bundling, refine's payload
extraction (asked of the snapshot path and of the tarball entry's own
path) and the unmanaged section all skip the file, and `staged_writes`
lists it as withheld (`StagedFileKind::UnmanagedFile`) so the
configuration FIXME names it. A new payload route for unmanaged files
must ask the same function.

**The machine-id is never staged.** Rendered unit state survives first
boot only because the image keeps the base image's zero-length
`/etc/machine-id`; a file that reads `uninitialized`, or none, makes
first boot apply the presets and undo rendered `enable` and `disable`
lines (measured, [real-host-validation-runs](real-host-validation-runs.md)).
The config walk excludes the path on a scan (`UNOWNED_EXCLUDE_EXACT` in
`walk.rs`), but an imported snapshot can offer it from any section, so
`staged_writes` withholds every offer whose `Destination` is
`MACHINE_ID_PATH` as `WithheldWhy::MachineId`. The check runs right after
the `Destination` is built, before a config entry yields to a drop-in at
its path and before identical offers collapse into one write, so every
row that offered the path keeps its own withheld reason. Do not move it
later: two rows with one content would otherwise become one write, and
a config entry under a drop-in of the same path would show no reason.

Identical content offered twice at one destination is one write, not a
conflict. A custom repo file is both a config entry and an
`rpm.repo_files` row, an unowned GPG key and a firewall zone likewise,
so withholding both offers would drop every one of them.

**A file two sections record is decided by its config entry.** Where an
included `ConfigFile` offer shares a destination with a `RepoFile`,
`GpgKey` or `FirewallZone` offer (`StagedFileKind::is_inventory_record`),
the inventory offer yields. In a fleet aggregate the repo, key and zone
inventories keep one representative payload per path while the config
entry keeps the content variants, so without this rule two differing
hosts withheld both and the repo file left the export. The write is the
included config entry's content. Refine's `SelectVariant` sets only a
config entry's `variant_selection`, and `SetInclude` on the path
includes the first-ranked entry, so selecting another variant does not
yet change which one is written. These three are the only kinds the config walk can record at
the same path, because it excludes none of their directories. Two
differing config entries at one destination still conflict.

**What the decision covers.** Under `config/`, `drop-ins/` and
`flatpak/`, `write_config_tree` writes `writes` and nothing else except
the flatpak manifest, and the export's only other file there is the
tmpfiles fragment (both below). The decision covers config entries, repo
files, GPG keys, firewall zones, drop-in rows (a local timer's file is
one of these two, never a scheduled-section offer), the units the
scheduled section generates from cron, the flatpak provisioning
service, and the kernel arguments file (`KargsFile`, at
`KARGS_PATH`), which is decided like a collected offer, not like a
generated unit. Still written outside the decision: the flatpak manifest
(`flatpak/flatpak-install.json`), quadlets (`quadlet/`), env files
(`env-files/`), subscription files (`subscription/`), and the `/var`
tmpfiles fragment, which `write_tmpfiles_conf` writes after the stager
with a plain write at `config/usr/lib/tmpfiles.d/inspectah-var.conf`.
That last one is under `config/` and no offer is checked against its
path. A new writer under those three directories goes through
`offers` and `staged_writes`, or the Containerfile and the export can
disagree again.

**`Destination::new` refuses a path too long for a filesystem.** A raw
path over `MAX_EXPORT_PATH_LEN` (4096 bytes, Linux `PATH_MAX`) is
`TooLong`, checked before anything else so that normalizing never walks
an unbounded input, and a component over `MAX_PATH_COMPONENT_LEN` (255
bytes, `NAME_MAX`) is `ComponentTooLong`. The offer lands in `refused`
with that rule and is named in a FIXME.

## Settings are read from logical lines

A value copied out of a unit file into a field that redaction treats as
an identifier (`MountUnitFinding.mount_point`, classified in
`every_field_of_a_content_bearing_type_is_classified`) must provably
come from the directive it is named for. systemd reads a unit file by
logical line (`config_parse` and `read_line_full`): lines end at `\n`,
`\r` or `\0`; a comment line is skipped, even inside a continuation; a
line ending in an unescaped backslash continues, the backslash becomes a
space and the next line is appended; an empty line ends a continuation.
A reader that walks physical lines takes the tail of a continued
`What=` for a directive of its own:

```ini
What=https://files.example/volume?\
Where=/opt/token=secret
```

is one `What=` to systemd, and a physical-line reader recorded
`/opt/token=secret` as the mount point, where redaction of the captured
file never reached it. `mount_settings` reads `logical_lines`. Any new
setting taken from a unit file goes through the same function. Option
lists follow systemd's own matcher too (`fstab_filter_options`): split
only at unescaped commas, nothing trimmed inside the value, `name` or
`name=...` matches, and the last match wins.

## A path is one destination per staging filesystem, not only per spelling

`Destination` decides identity lexically and case-sensitively, as the
Linux target does, and the Containerfile, the unit plan and refine's
view read the decision with no staging directory. The export is staged
on the operator's filesystem first, and that one may disagree: APFS is
case-insensitive and normalizes Unicode by default, so two destinations
can be one file there, and a second plain write replaces the first
without an error.

So the stager, not the decision, proves each staged path is new on the
staging filesystem. `Stager` in `configtree.rs` creates every directory
with `create_dir` and reuses one only when it created that exact path,
and creates every file with `create_new`. Any `AlreadyExists` fails the
export: when the existing entry is, by device
and inode, one the stager created, the error names it and the file it
was staged for. Never fold case in the decision instead: that is wrong
for the target and misses Unicode normalization. The cost is that on
macOS such a snapshot does not export until the operator excludes one of
the two files, and refine's view cannot show it beforehand. The staging
directory must be fresh, and nothing else may write into it before the
stager runs.

## Absence tests on `systemctl` lines use the strict helper

`acts(containerfile, verb, unit)` in `crates/cli/tests/units_scan_test.rs`
matches a quoted operand after `--`. Its negation is not an absence test:
a regression that wrote the name bare, or under another verb, still
passes `!acts(...)`. `in_no_systemctl_instruction` checks that the name
appears in no `systemctl` instruction in any form, quoted, bare or inside
another word, and `only_comments_name` that no instruction of any kind
names it. Use one of those, after proving presence on the same route.

## Related

- [service-omission-model](service-omission-model.md): the rules, in order, per verb
- [containerfile-value-quoting](containerfile-value-quoting.md): unit operands after `--`
- [cross-inspector-shadow-passes](cross-inspector-shadow-passes.md): masks are states; storage files belong to config
