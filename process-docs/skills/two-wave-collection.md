---
name: two-wave-collection
description: Inspector execution uses a two-wave dispatch where RPM runs first and feeds state to all other inspectors.
---

# Two-Wave Collection Dispatch

Inspectors do not run in a flat parallel pool. The `collect()` function
in `crates/pipeline/src/collect.rs` partitions inspectors into two
waves using `is_wave2()`:

- **Wave 1:** RPM and Subscription (no dependencies on other inspectors)
- **Wave 2:** Everything else (Config, Services, Network, Storage, etc.)

Wave 1 runs first. After it completes, RPM output is extracted into an
`RpmState` struct via `extract_rpm_state()`. This `RpmState` is then
injected into the `InspectionContext` for all Wave 2 inspectors via
`ctx.rpm_state`.

```rust
// Wave 2 receives enriched context:
let wave2_ctx = InspectionContext {
    source_system: source,
    executor,
    rpm_state: if rpm_populated { Some(&rpm_state) } else { None },
    baseline_data: baseline,
};
```

### The None vs Some(empty) Contract

Wave 2 inspectors **must** distinguish two cases:

- `ctx.rpm_state: None` -- RPM inspector failed entirely. Wave 2
  inspectors that depend on ownership data should return
  `Err(InspectorError::Failed)` because classifications are
  untrustworthy.
- `ctx.rpm_state: Some(state)` where `state.owned_paths.is_empty()` --
  RPM succeeded but found zero owned paths. Wave 2 proceeds normally.

This is a correctness invariant, not an optimization. Getting it wrong
means the Config inspector would classify RPM-owned files as "unowned"
when RPM simply failed to run.

### Adding a New Inspector

Any new inspector automatically lands in Wave 2 (the `is_wave2` function
uses a negative match -- only `Rpm` and `Subscription` are Wave 1). If
your inspector genuinely has no dependency on RPM state, you need to
explicitly add its `InspectorId` to the Wave 1 list.

You must also add a variant to `InspectorId` in
`crates/core/src/types/completeness.rs` and a `SectionData` variant
(if it produces a snapshot section) before the compiler will let you wire
it in.

## Why This Matters

Skipping the wave contract (e.g., treating `None` as "no packages") silently
degrades Config classification. The bug is invisible in tests unless you
specifically test with an RPM-failed scenario. The codebase has explicit
contract tests for `None` vs `Some(empty)` -- check
`test_rpm_state_none_vs_empty` in `crates/core/src/traits/inspector.rs`.

## See Also

- `crates/pipeline/src/collect.rs` -- wave dispatch logic
- `crates/core/src/traits/inspector.rs` -- `RpmState`, `InspectionContext`
- `crates/core/src/types/completeness.rs` -- `InspectorId`, `SectionData`
- `docs/contributing/adding-an-inspector.md` -- high-level guide
