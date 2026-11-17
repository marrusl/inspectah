---
name: web-disposition-contract
description: The refine web API sends `FindingKind` as a three-variant tagged union, not a bool — the TypeScript layer must branch on `kind`, and `adapter.rs` must restore config advisories from the original snapshot because the view is classified from the projection. Section stats partition into included/excluded/advisory, where `excluded` means a decision the user made.
---

# Finding Dispositions Across the Web API

`FindingKind` reaches the single-host refine web UI two different ways,
and the two have different failure modes.

## 1. `PackageEntry` / `ConfigFileEntry` — the enum crosses intact

`ViewResponse` carries `RefinedView` under `#[serde(flatten)]`, so
`packages[].entry.disposition` and `config_files[].entry.disposition`
serialize the core enum directly:

```json
{"kind": "actionable", "include": true}
{"kind": "advisory", "advisory_type": "modernization", "rationale": "…"}
{"kind": "inventory"}
```

**Only `actionable` has an `include` key.** The TypeScript type is a
discriminated union (`Disposition` in `api/types.ts`) and the predicates
live in `api/disposition.ts`:

- `isIncluded(d)` — mirrors `FindingKind::is_included()`
- `isToggleable(d)` — false for advisory and inventory

Never read `.include` directly and never fall back to `?? true`. That
fallback is what shipped advisories to the browser as items the user had
chosen to bake into the image, with toggles the session refuses.
`crates/web/src/error.rs` maps both refusals to 422, so a UI that still
offers the toggle produces a visible error rather than a silent no-op.

## 1a. `ContextItem` — the reference-row decision path

Reference sections (`ReferenceSection`, `crates/web/src/web_types.rs`)
render rows as `ContextItem`, which carries two optional fields:

```rust
pub struct ContextItem {
    // ... id, title, subtitle, detail, searchable_text ...
    pub decision: Option<ContextDecision>,
    pub advisory: Option<ContextAdvisory>,
}
```

`decision` (`item_id: ItemId`, `include: bool`, `locked: bool`) is
present when the row is the place a `SetInclude` op targets; `advisory`
(`advisory_type`, `rationale`) is present when the row states something
the export reports and nobody decides. Both are
`#[serde(skip_serializing_if = "Option::is_none")]`, so a plain
reference row carrying neither serializes byte-identically to the
pre-phase-0 shape — this is the only byte-identity guarantee the
design makes.

`adapter::context_decision()` is the single place that fills `decision`:
it returns `Some` only for `FindingKind::Actionable`, `None` for
`Advisory` and `Inventory`, so a row backed by either of the
non-actionable variants renders with no control to offer.

## 1b. Scheduled Tasks rows: identity, and the generated-timer exception

**Generated timer rows are an exception to the `decision` contract.** A
timer or `@reboot` service converted from cron is decided on its
Scheduled Tasks row (`ItemId::GeneratedTimer { name }`), but its row
carries no `decision`. Reference sections come from
`/api/snapshot/sections`, which the UI fetches once; the row's state
changes with every operation, including one on another row (excluding
its cron job excludes it). So the state is in `/api/view`, which is
recomputed per operation:

- `generated_timers[]`: `name`, `include`, `locked`, and
  `cron_job_excluded` (the excluded cron job it is generated from).
  `include` is true while any record of the name is included, since the
  export carries the name while one is: two cron files can generate one
  name.
- `declined_generated[]`: a generated request the export declines, with
  `row: { row: "scheduled", name }`.

**Join both through the row's `generated_timer` field, the generated
name.** Never derive the name by parsing the row's id. The id is an
identity, not a key into the view: an earlier version joined on a
`generated:` prefix, and a collected timer whose own name began with that
prefix took the generated row's decline note.

The row offers a toggle unless it is excluded and `cron_job_excluded` is
set: the server refuses that include, so the row shows why instead.
While any record is still included, excluding stays available even when
one source cron job is excluded.

**Scheduled row ids are kind and source.** `adapter::generic_row_id`
builds them from `GenericRefItem.scheduled` (`ScheduledSource`, set by
the reference projection):

| Row | Id |
|---|---|
| Cron job | `cron:<path>` |
| Collected timer | `timer:<path>`, or `timer:<name>` when the snapshot records no path |
| Instance timer described from its template's file | `timer-instance:` and the JSON-encoded `[path, name]` |
| At job | `at:<file>` |
| Generated timer | `generated:<index>:<name>`, `<index>` its place in `generated_timer_units` |

The kind ends at the first `:` and contains none, so rows of two kinds
never share an id, and within a kind the source is unique. Names are not:
a shadowed timer's `/etc` and vendor files, a collected timer and a
generated one, and two lines of one cron file all share names. A shared
id is a shared React key, DOM id and GlobalSearch key, so search and
reveal land on the wrong row. Rows of other reference sections keep the
projection's ids.

Paths are not unique either. An instance timer found by its link is
described from its template's file and records that file as its `path`,
the evidence its row shows, so `worker@.timer`, `worker@a.timer` and
`worker@b.timer` all record `etc/systemd/system/worker@.timer`. A timer
whose path's file is not `<name>.timer` (or `<name>`) is therefore
`timer-instance:` and `[path, name]` as JSON, never the two joined with a
separator, since a path and a unit name can both hold `:`. A timer that
describes its own file keeps `timer:<path>`. The JSON form holds quotes,
so a test that looks a row up by DOM id compares strings, as reveal does,
never through a CSS attribute selector.

**Reveal matches test ids exactly.** After a navigation, App finds the
target row by comparing each `data-testid` as a string
(`elementByTestId` in `App.tsx`). Do not build a CSS selector from an
item id: ids are host data, and a unit name can hold a literal backslash
(`backup\x2djob.timer`), which a selector reads as an escape, so the link
opens the section and focuses nothing.

The `units` section is the exception: it finds its row by `data-unit-key`
(section 1c), and `REVEAL_TESTID_PREFIXES` does not apply to it. The
prefixes stay for every other section.

## 1c. Units: link rows, records and the default target

`/api/view` passes the projection's unit views through untouched
(`ManualLinkView`, `UndeterminedInstallView`, `DefaultTargetView`), so the
TypeScript types mirror serde exactly. Four wire forms are easy to get
wrong:

- **`ManualLinkView.cause` is externally tagged.** Six causes are bare
  strings (`"static_unit"`); `undetermined` is an object,
  `{"undetermined":{"reason":"upheld_by"}}`. Branch on `typeof`, never on
  string equality alone (`causeName` in `ServiceSection.tsx`).
- **`ItemId::DefaultTarget` has no `key`.** It is `{"kind":"DefaultTarget"}`,
  the one item id without one, since a snapshot records at most one
  default target. Code that reads `item_id.key` from an arbitrary item
  breaks on it.
- **Absent versus `null`.** `default_target` is absent, not `null`, when
  the host records none. `DefaultTargetView.declined` is `null`, not
  absent, when the target is carried. A unit row's `also` and
  `install_drop_ins` are absent when empty.
- **A unit row's `instance_of`, `required_config` and `data_paths`.**
  `instance_of` (the template) is absent on a unit that is not an
  instance. `required_config` and `data_paths` are always present, as
  plain lists, empty for a plain unit, so the TypeScript type has them as
  required `string[]` and every hand-made unit row, in a Vitest factory or
  an e2e fixture, carries both. Unit rows reach `/api/view` through
  `ServiceDecisionDto` in `crates/web/src/adapter.rs`, which copies fields
  one by one from the projection's `UnitStateView`, so a new projection
  field reaches the browser only once the DTO names it. The units list
  shows the three under the row (`instance-of-<unit>`,
  `required-config-<unit>`, `data-paths-<unit>`) and ties them to the
  row's toggle. A carried instance's § 10.8 data sentence needs no field
  of its own: it is already in `attention_reason`, and the row does not
  repeat it.
- **Link rows and undetermined records have no toggle.** They carry no
  `include`; their state follows the unit's install files, and
  `not_applied` lists the files the export leaves out, absent while the
  advisory applies.

**Row identity in the Systemd units section.** Two identities, each built
by one function in `unitOutcome.ts` and spelled nowhere else:

- **Row key**, `rowKey(record)`: `JSON.stringify([kind, ...identity])`,
  the row's logical identity. Unit state `["unit_state", unit]`, drop-in
  `["dropin", path]`, default target `["default_target"]`, link
  `["manual_link", relation, parent, unit]`, undetermined install
  `["undetermined_install", unit]`, mount finding
  `["mount_finding", unit]`. JSON, never a joined string, because a unit
  name can hold `-`, even `.service-` (`a.service-b.service`), and a
  joined key gives two links one id. Every row carries it in
  `data-unit-key`, on the element that carries its row test id. Global
  search entries and "Go to owner" send it as the item id.
- **Occurrence key**: the row key, `#`, and the record's ordinal among
  records of its own list with the same row key. A snapshot can hold a
  true duplicate (two records with one row key), and React reconciles
  repeated sibling keys wrongly, so the occurrence key is the React key,
  the pin, the roving tab stop and the focus key. It never reaches a DOM
  id, a URL, `data-unit-key`, a search entry or a test id, and nothing
  parses one.

The `units` reveal resolves the item id through `rowKeyForItemId`: a bare
unit name is always the `unit_states` row, and an id that starts with `[`
resolves only when it is exactly the bytes `JSON.stringify` writes for a
known kind and identity length, so a lookalike names no row. It then finds
the first row in document order whose `data-unit-key` equals the key,
compared as a string (`elementByUnitKey` in `App.tsx`). Anything that
navigates into the section sends a row key or a bare unit name.

A later occurrence of a duplicated row gets DOM ids of its own (a
generated suffix on today's ids), so its `aria-describedby` names its own
notes, while test ids stay shared with the first. Never look a note up by
`<prefix>-<unit>` id; use the test id. A units global search entry carries
its text on the entry (`searchText`), never in `searchTextMap`, whose bare
and `sectionId:itemId` keys could hand one entry another's text.

Every rule of the section (keys, outcomes, counts, group order, the group
header's badge and accessible name (`groupHeader`), which key presses a
row ignores (`rowKeyIgnored`), what is
visible under the filter, query and pins, the reveal plan, the empty
states, the filter bar's presentation and the order of shown rows) lives
in `unitOutcome.ts`. `useUnitsSectionState.ts` holds the React state and
calls those functions; `ServiceSection.tsx`, `UnitGroup.tsx` and
`UnitOutcomeFilter.tsx` render. A rule added to a component cannot be
tested without React and drifts from search and reveal, which share the
same functions.

The default target's pending-guard key is `["default_target"]`.

**Testing the units section in Vitest.**

- A test that loads a JSON fixture uses `import.meta.glob`:
  `tsconfig.json` has no `resolveJsonModule`, so a direct import fails
  `tsc`.
- `localStorage` is not defined in this jsdom environment; stub it with
  `vi.stubGlobal("localStorage", ...)`, as `Sidebar.test.tsx` does.
- PatternFly 6 `Label color="grey"` renders no `pf-m-grey` class; assert
  grey as the absence of a colour modifier.
- State set in an `onFocus` handler needs `act(() => el.focus())`.
  `blur()` on a disabled, focused checkbox does not move focus in jsdom,
  so a test of focus during a pending op starts from a focused row.
- Opening a section moves focus to its first `role="row"` a frame later;
  a test that presses `/` right after opening waits two animation frames.
- A child that must change state its parent owns cannot call the setter
  during render (React warns). Set a request state in render and hand it
  over in a `useLayoutEffect` that resets it; the parent's update then
  flushes before paint and before a `requestAnimationFrame` lookup.
- Vitest does not process CSS, so a style a jsdom test must see is inline.

The Systemd units bulk toggle does not send `DefaultTarget`: the target has its
own toggle. The fleet view shows none of these rows and still calls its section Services.

## 2. The hand-built decision DTOs — still `include: bool`

`ServiceDecisionDto`, `DropInDecisionDto`, `QuadletDecisionDto`,
`FlatpakDecisionDto`, `TunedDecisionDto`,
`LanguagePackageEnvDto`, and `UnmanagedFileItemDto` collapse the
disposition with `.is_included()`. `UnmanagedFileItemDto` also carries
`withheld`, as a config row does, found by the row's recorded `path` and
present only on an included row: a file under `/etc/systemd/system/` is
withheld there (`UnmanagedUnitPath`).

That is currently sound: as of v0.9.0-beta.2 the only producers of
non-actionable dispositions are the config inspector (modernization,
cross-tree symlink), the storage inspector (unbacked `/var`, which reaches
renderers through a dedicated field, not these DTOs), and the network
inspector (inventory, which renders as reference sections). **None of
these DTOs can carry a non-actionable disposition today.**

If you ever make one of those entry types advisory or inventory, the
collapse becomes the same bug: convert the DTO to carry the disposition
rather than teaching the frontend to guess.

**Sysctl is not a decision.** `ViewResponse.sysctl` is a
`SysctlViewDto`, the read-only System Tuning panel built by
`sysctl_view::project_sysctl`; it carries no `include` and no `ItemId`.
Each `/etc` sysctl file is decided by its Configuration Files row, which
the panel names by path. `tuned` stays on `ViewResponse` as decision
rows.

## 3. The projection preserves finding semantics — `with_include`, not `from_bool`

`RefinedView.config_files` is `classify_configs(&projected)`, where
`projected = project_snapshot()`.

`apply()` refuses `SetInclude` on an advisory, but **`resume_from` does
not re-validate** — it restores an autosaved timeline directly, on the
grounds that the ops were validated when first applied. A session saved
before that guard shipped can replay a toggle on an advisory config path.

`project_snapshot` therefore applies a config `SetInclude` with
`entry.disposition.with_include(*include)`, the same merge-safe setter
`aggregate/merge.rs` uses at 22 sites. **Do not reintroduce
`FindingKind::from_bool` here.** `from_bool` overwrites an advisory
wholesale, and the consequence is not cosmetic: `render_refine_export`
renders from `snapshot_projected()`, so a downgraded advisory means the
modernization-flagged file is copied into the image. The web view alone
being correct is not enough — the export is the other consumer, and it is
the one that changes what ships.

The remaining ten `from_bool` sites in `project_snapshot` cover item kinds
no collector makes advisory or inventory today. Config is the exception
because the config inspector folds advisories onto the entry disposition.

`adapter::build_web_view` also calls `restore_config_advisories`, which
takes both the set of advisory paths and their content from a single
`pipeline::render::advisory::config_advisories(session.snapshot())` call
against the original snapshot. Since the projection fix that restore is
defense in depth rather than the load-bearing guard it was written as.

**Take the advisory list and any row filter from one call on one
snapshot.** Reading advisories from the original while filtering rows
against the projection lets an entry render twice — once as an advisory
and once as a config row.

`normalize.rs` guarantees non-actionable dispositions survive
normalization, so the original is safe to read.

**Config locks use `with_include(false)` too, never `excluded()`.** That
covers the merge-hostile lock (`normalize_merge_hostile_configs`), the
inspectah COPR repo lock (`normalize_config_defaults`) and the config arm
of `clamp_locked_items`. A lock is about inclusion, not content: rewriting
a `content_not_captured` advisory as an ordinary exclusion makes the refine
export carry empty content as if it were the file's, and `inspectah
aggregate` over two such exports included it and staged an empty
`/etc/crypttab`. `a_locked_uncaptured_config_survives_refine_export_and_aggregation`
in `crates/cli/tests/not_carried_parity_test.rs` holds this.

## 4. `SectionStats` — three buckets, and `excluded` means a decision

`stats.sections[]` carries `total`, `included`, `excluded`, and
`advisory`. They partition the section:

```
included + excluded + advisory === total
```

`SectionStats::from_dispositions` asserts that in Rust by matching
exhaustively on `FindingKind` rather than counting off `is_included()`.
Counting both buckets off that predicate is what filed every advisory
under `excluded`, because it is false for an advisory as well as for a
file the user turned off.

**`excluded` now means strictly "the user excluded this actionable
finding."** It is a decision, not a state. Anything display-only is in
`advisory`, and no consumer should derive one bucket from the others.
The bug that proved it: a group badge that derived advisories as
`total - (included + excluded)` read `0 adv` for the whole time
advisories were mis-bucketed, and said nothing.

**The Inventory caveat:** `FindingKind::Inventory` is counted in the
`advisory` bucket, on the grounds that it is the other display-only
disposition. It is not purely a network-findings caveat: a config file
whose path is also a Service Drop-ins entry is given
`shadowed_by: Some(ConfigShadow::Units)` and
`disposition: FindingKind::Inventory` (`ConfigFileEntry`,
`crates/core/src/types/config.rs`), and because it is still a member of
`config.files`, it is counted into the Config section's `advisory`
bucket through the same `from_dispositions` call network findings never
reach. `session::validate_target` refuses a `SetInclude` against such an
entry's `ItemId::Config` with `InventoryNotToggleable`, regardless of
the requested direction — `entry.disposition.is_inventory()` is checked
before `*include` is ever read.

`loaded_modules[].disposition` also defaults to `FindingKind::Inventory`
(`KernelModule`, `crates/core/src/types/kernelboot.rs`), but this is a
narrower case than it looks: `ItemId::KernelModule` was removed in phase
0, so there is no `ItemId` a client could even construct to target a
kernel module, and the web adapter builds no `ContextItem` for
`loaded_modules` at all. A loaded module's `Inventory` disposition is a
data-model fact with no reachable view or toggle path, and it is not
counted in any `SectionStats` bucket — `from_dispositions` is called
only over `packages` and `config_files` today. So of the two, only the
service-shadowed config entries actually reach this `advisory` bucket and
the `InventoryNotToggleable` refusal; `loaded_modules` reaches neither.
If that ever changes — a view renders loaded modules, or
`SectionStats` gains a Kernel & Boot section — the bucket needs
revisiting along with the user-facing label.

On the TypeScript side `advisory` is optional in `SectionStats` **only**
so the many pre-existing fixtures need not restate a zero. The server
always sends it (`usize`, not `Option`), so an absent value means a
hand-built fixture, never real data. Reading it as `0` renders one bucket
less, which is the safe direction — unlike the `include ?? true` fallback
in section 1, it cannot assert a decision the user never made.

Two web surfaces consume the bucket, both in `crates/web/ui/src/components/`:

- `StatsBar.tsx` — `SectionCounts` renders `N advisory` as a third
  counter, and omits it when the count is zero so the common host with no
  advisories keeps a two-counter bar.
- `ExportDialog.tsx` — sums `advisory` across sections and reports it
  apart from the exclusion counts.

The export dialog's copy has to stay true to both halves of what export
does with an advisory: `render::audit` lists it in `audit-report.md`,
which ships in the tarball, while `configtree::write_config_tree` skips
it (the write is gated on `is_included()`), so the file itself is never
copied into the image. Saying only "excluded" or only "not exported"
gets one of those halves wrong.

So there are two surfaces reporting the same count: the HTML report's
group badge and `StatsBar.tsx`. Change the bucket and check both.

**One ordering caveat, pre-existing.** `build_web_view` calls
`restore_config_advisories` *after* `session.view()` has already computed
`view.stats` from the unrestored projection. For the
one case that restore exists for — a legacy autosave replaying a
`SetInclude` against an advisory path — the stats-driven surface
would undercount while the rows still render as advisories, and the HTML
badge (which counts off the original snapshot) would disagree with it.
Current code refuses such a toggle, so this is reachable only through an
old sidecar.

## Regression Test Shape

The disposition path fails silently, so a test that builds structs in
Rust and never round-trips through a session proves nothing. The
stale-projection case needs a real resume:
`crates/web/tests/config_advisory_resume_test.rs` writes a tarball, saves
an autosave sidecar with a stale `SetInclude`, calls
`RefineSession::resume_from`, and asserts on both consumers — the
serialized web view *and* `snapshot_projected()`, which is what the export
renders from. Asserting only the view passes while the export still bakes
the file in.

## See Also

- `crates/refine/src/session.rs` — `project_snapshot`, the `ItemId::Config` arm
- `crates/web/src/adapter.rs` — `restore_config_advisories`
- `crates/web/ui/src/api/disposition.ts` — the predicates
- `crates/pipeline/src/render/advisory.rs` — the shared collector
- [finding-disposition-serde-defaults](finding-disposition-serde-defaults.md)
  — the deserialization half of the same vocabulary
