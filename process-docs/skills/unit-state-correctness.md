---
name: unit-state-correctness
description: Correctness requirements found while building unit state for every kind. A type that feeds an unchecked constructor of a proved type has no public fields, and a source-level test guards PlannedUnit because stable rustdoc does not check compile_fail error codes; zero length is decided without decoding a unit file, and a file that is not text never degrades the scan; every staged-file question goes to staged_writes through Destination; absence tests on systemctl lines use the strict helper.
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
compare would answer differently from the stager. `is_staged` counts
`beside` as well as `writes`: a generated unit in `beside` can be the
only file landing at a destination whose collected offer was withheld,
and a `mask` there would still fail.

Identical content offered twice at one destination is one write, not a
conflict. A custom repo file is both a config entry and an
`rpm.repo_files` row, an unowned GPG key and a firewall zone likewise,
so withholding both offers would drop every one of them.

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
