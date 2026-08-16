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

## 2. The hand-built decision DTOs — still `include: bool`

`ServiceDecisionDto`, `DropInDecisionDto`, `QuadletDecisionDto`,
`FlatpakDecisionDto`, `SysctlDecisionDto`, `TunedDecisionDto`,
`LanguagePackageEnvDto`, and `UnmanagedFileItemDto` collapse the
disposition with `.is_included()`.

That is currently sound: as of v0.9.0-beta.2 the only producers of
non-actionable dispositions are the config inspector (modernization,
cross-tree symlink), the storage inspector (unbacked `/var`, which reaches
renderers through a dedicated field, not these DTOs), and the network
inspector (inventory, which renders as reference sections). **None of
these DTOs can carry a non-actionable disposition today.**

If you ever make one of those entry types advisory or inventory, the
collapse becomes the same bug: convert the DTO to carry the disposition
rather than teaching the frontend to guess.

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
and once as a config row. The TUI (`screen/single_host.rs`) follows the
same rule for the same reason.

`normalize.rs` guarantees non-actionable dispositions survive
normalization, so the original is safe to read.

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
`advisory`, and no consumer should derive one bucket from the others —
the TUI group badge does derive advisories as `total - (included +
excluded)`, and that is precisely why it read `0 adv` while advisories
were mis-bucketed.

**The Inventory caveat:** `FindingKind::Inventory` is counted in the
`advisory` bucket, on the grounds that it is the other display-only
disposition. Only network findings are inventory today, and they belong
to no section counted here, so the bucket is in practice the advisory
count. If that ever stops being true, the bucket needs splitting and the
user-facing label needs revisiting with it.

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
