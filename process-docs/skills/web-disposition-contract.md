---
name: web-disposition-contract
description: The refine web API sends `FindingKind` as a three-variant tagged union, not a bool — the TypeScript layer must branch on `kind`, and `adapter.rs` must restore config advisories from the original snapshot because the view is classified from the projection.
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

## 3. Config advisories must be read from the *original* snapshot

`RefinedView.config_files` is `classify_configs(&projected)`, where
`projected = project_snapshot()`. `project_snapshot` applies every
`SetInclude` in the timeline with `FindingKind::from_bool`, which
overwrites an advisory disposition wholesale.

`apply()` refuses `SetInclude` on an advisory, but **`resume_from` does
not re-validate** — it restores an autosaved timeline directly, on the
grounds that the ops were validated when first applied. A session saved
before that guard shipped can replay a toggle on an advisory config path,
and the projection then reports the finding as
`{"kind":"actionable","include":true}`.

So `adapter::build_web_view` calls `restore_config_advisories`, which
takes both the set of advisory paths and their content from a single
`pipeline::render::advisory::config_advisories(session.snapshot())` call
against the original snapshot.

**Take the advisory list and any row filter from one call on one
snapshot.** Reading advisories from the original while filtering rows
against the projection lets an entry render twice — once as an advisory
and once as a config row. The TUI (`screen/single_host.rs`) follows the
same rule for the same reason.

`normalize.rs` guarantees non-actionable dispositions survive
normalization, so the original is safe to read.

## Regression Test Shape

The disposition path fails silently, so a test that builds structs in
Rust and never round-trips through a session proves nothing. The
stale-projection case needs a real resume:
`crates/web/tests/config_advisory_resume_test.rs` writes a tarball, saves
an autosave sidecar with a stale `SetInclude`, calls
`RefineSession::resume_from`, and asserts on the serialized view.

## See Also

- `crates/web/src/adapter.rs` — `restore_config_advisories`
- `crates/web/ui/src/api/disposition.ts` — the predicates
- `crates/pipeline/src/render/advisory.rs` — the shared collector
- [finding-disposition-serde-defaults](finding-disposition-serde-defaults.md)
  — the deserialization half of the same vocabulary
