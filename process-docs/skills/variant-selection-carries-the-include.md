---
name: variant-selection-carries-the-include
description: On a fleet aggregate a path's include is one decision and the selected variant is the only entry that carries it; the renderer reads dispositions, never variant_selection, so selection and include must move together in refine's projection.
---

# Variant selection carries the include

A fleet aggregate keeps several entries at one path when hosts disagree on
its content: one `VariantSelection::Selected`, the rest `Alternative`. Two
facts make this easy to get wrong:

- **The renderer reads dispositions only.** `configtree::offers` and the
  quadlet writer take every included entry; nothing in `crates/pipeline`
  consults `variant_selection` to decide what ships. Two included entries
  at one destination with different content are a `Conflict` and both are
  withheld.
- **The refine view shows the Selected entry's include.** The aggregate
  handlers emit the Selected (or Only) row and fold the rest into
  `variants`, so a path whose include sits on an Alternative reads as
  excluded while its Alternative ships.

## The rule

After replay, `RefineSession::project_ops` runs
`variant_ops::carry_path_include`: for each path with more than one entry
and a `Selected` one, the path is included when any entry is, and only the
`Selected` entry carries that include. It covers config files, drop-ins,
quadlets, compose files and unmanaged files.

So every writer of a variant-kind disposition sets the **path**:

- `SetInclude` on `Config`, `DropIn` or `Quadlet` sets every entry at the
  path, never the first one found (the first is the merge's winner, not the
  user's choice). A unit toggle's drop-in cascade already reaches every
  entry.
- A no-op or already-held check reads the path's include (any entry), not
  the first entry: `config_path_included` in `session.rs`.
- `SelectVariant` only changes `variant_selection`; the pass moves the
  include. Both op orders give the same projection.

Language environments have no `variant_selection` field and no include
toggle in the projection: selecting one includes it and excludes every
other environment at the path (`materialize_language_env_variants`).

Single-host snapshots have one entry per path (`Only`), so the pass leaves
them alone. A group with no `Selected` entry is also left alone.

Tests: `selected_variant_is_the_one_the_export_ships` and its neighbours in
`crates/refine/tests/aggregate_variant_ops_test.rs` assert the staged write,
not only the flags.
