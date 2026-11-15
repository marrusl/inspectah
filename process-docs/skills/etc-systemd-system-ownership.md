---
name: etc-systemd-system-ownership
description: Who owns each thing under /etc/systemd/system. A mask is a state, a link is a state or a link finding, a storage unit's file and a local unit's file are config entries, a full shadow and a unit's drop-ins are drop-in rows, the default target is its own record; the config walk excludes the three dependency directories; a timer's state, file and schedule each have one control; an unmanaged file there is withheld.
---

# Who owns what under `/etc/systemd/system`

Several inspectors read `/etc/systemd/system`: the config walk records
unowned and modified files there, the units inspector reads states, links,
drop-ins and shadows, and the scheduled inspector reads timers. Each thing
found there has one owner and one control, so the export has one writer
for each file and one row that decides it. This is the contract, from
§ 7.7 and § 8 of the units spec. A new reader of the directory takes its
place in these tables or leaves the thing to its owner.

## Each path directly under the directory

The units inspector decides by the form of what is at
`/etc/systemd/system/<name>`, testing a symlink before anything else:

| What is at the path | Recorded as | Its file is written by |
|---|---|---|
| A link to `/dev/null`, or a zero-length regular file | A `Masked` state change when the name and the fragment under the mask classify as carried; a mount finding with state `Masked` when they classify as advisory. Nothing else. | Nobody. `systemctl mask` makes the link in the image. |
| A link to another unit's file | Nothing, or a `ManualLink` with relation `Alias` when the unit's `Alias=` does not name it. | Nobody. |
| A regular, non-empty file of a storage kind (`.mount`, `.automount`, `.swap`) | A mount finding. The file is a config entry. | The config tree, on the config row's decision. |
| A regular, non-empty file of an ordinary kind with a vendor file of the same name | A full shadow in `units.drop_ins`. | The drop-in writer, on that row's decision. The config entry at the path is `Inventory` with `shadowed_by` set. |
| A regular, non-empty file of an ordinary kind with no vendor file | A `Local` fragment. The file is a config entry. | The config tree, on the config row's decision. |
| `default.target`, a link | The `DefaultTarget` record, its own row and its own `set-default` line. It is not an alias. | Nobody. `systemctl set-default` makes the link in the image. |

- **A mask has one representation.** However the host wrote it, a mask is
  a state and the export carries no file for it. The config walk skips
  symlinks, and zero-length regular files with a unit suffix, directly
  under the directory (`is_systemd_unit_symlink`), and `staged_writes`
  withholds a zero-length file at a unit path from any other route.
- **A storage unit's file has one owner, the config section.** A regular
  `/etc/systemd/system/tmp.mount` beside a vendor `tmp.mount` is a config
  entry under its own toggle, not a full shadow, and the unit is advisory.

## Drop-ins

- `/etc/systemd/system/<unit>.d/*.conf` is collected for every unit name
  that parses, plus `*.device.d` and `*.scope.d`, and for generated
  parents. The drop-in row owns the file; the config entry at the same path
  is `Inventory` with `shadowed_by` set (`shadow_service_owned_paths` in
  `crates/pipeline/src/collect.rs`). A drop-in on a generated parent is
  carried and does not bring the parent back.
- A `.conf` in a type-wide directory (`service.d`) or a dash-prefix
  directory (`a-.service.d`) belongs to no unit. It is not a drop-in row:
  an unowned one reaches the snapshot as a config entry like any other
  unowned file under `/etc`. Whether a package-owned, unmodified one
  reaches a single-host snapshot is not verified (§ 13 run R2). Either way
  it is never an install file, because `enable` does not read
  `[Install]` there (measured).
- Which drop-ins are install files, and what excluding one does to a
  unit's command, is in
  [service-omission-model](service-omission-model.md) (rule 5's drop-in
  half and rule 7).
- `SystemdDropIn.unit` stays a `String`: a drop-in may name a `.device` or
  `.scope`, which `UnitName` does not admit.

## Dependency directories

`*.wants/`, `*.requires/` and `*.upholds/` hold links, and a link is unit
state, never configuration. The units inspector reads them for the install
state and the link findings ([unit-install-state](unit-install-state.md)).
**The config walk excludes all three** (`UNOWNED_EXCLUDE_GLOBS` in
`crates/collect/src/inspectors/config/walk.rs`). The walk follows links, so
without the exclusion it records the linked unit's content at the link's
path, and the export stages a plain file where the link was. Every
dependency directory systemd reads links from belongs in the list, for that
reason: `*.upholds/*` is as much a link directory as `*.wants/*`, and a new
one systemd adds goes there too.

## What stays with the scheduled inspector

| Fact | Owner | Control |
|---|---|---|
| A timer is enabled, disabled or masked | Units (`state_changes`) | The unit row |
| A local timer's file, and its paired service's file | Config (`config.files`), like every other local unit file | The config row |
| When a timer fires and what it runs (`OnCalendar=`, `ExecStart=`, description, trigger) | Scheduled (`systemd_timers`) | None. It is a description. |
| A timer generated from a cron entry: its content, its staging, its enable | Scheduled (`generated_timer_units`) | The generated-timer row |
| Cron jobs, at jobs | Scheduled | Unchanged |

**Each fact has one control, and a fact that depends on another is
resolved at render by omission, never by a second writer.** An `enable`
depends on the unit file being in the image: if the operator excludes a
local timer's file, the `enable` is omitted with a reason, and nothing
else writes the file or enables the unit. A generated unit never lands
over a collected one ([unit-state-correctness](unit-state-correctness.md)).

## Unmanaged files

An unmanaged file anywhere below the directory is withheld by one shared
decision, `configtree::unmanaged_withheld`, before bundling, extraction and
render ([unit-state-correctness](unit-state-correctness.md)). A unit file
belongs to the config section; the unmanaged route never writes there.

## What this contract does not cover

The general rules for two inspectors claiming one path anywhere under
`/etc`, and the other claimants (quadlets, repo files, GPG keys), are not
written down yet; [cross-inspector-shadow-passes](cross-inspector-shadow-passes.md)
has the mechanism.

## Related

- [service-omission-model](service-omission-model.md): what each row's command needs from the export
- [unit-install-state](unit-install-state.md): how the links become a state
- [unit-state-correctness](unit-state-correctness.md): the staging decision and the unmanaged rule
- [cross-inspector-shadow-passes](cross-inspector-shadow-passes.md): one file in two inventories
