---
name: cross-inspector-shadow-passes
description: Where a file lands in two inventories with two dispositions, one of them must be marked Inventory at collect time -- in its own pass, below finalize_cross_inspector, never inside a sibling pass's early returns.
---

# Cross-Inspector Shadow Passes

Several collectors write into overlapping path space. The `/etc` walk
records a file into `config.files`; the services inspector records the
same `*.service.d/*.conf` or full unit shadow into `services.drop_ins`;
the kernel/boot inspector records a `/etc/sysctl.d/` file's keys into
`kernel_boot.sysctl_overrides`. The double-listing is deliberate: each
section needs the data.

What is not safe is **two live dispositions over one carriage
decision.** The renderer picks one writer per output path, so the other
disposition becomes a control that lies:

- `render/configtree.rs` subtracts service-owned paths from the config
  tree **unconditionally**, while the drop-ins writer skips excluded
  entries. Config included plus drop-in excluded wrote the file to
  neither tree with the config checkbox still checked; config excluded
  plus drop-in included shipped it anyway. Measured end to end on a RHEL
  9.8 host, 2026-11-01, with `x-export-warnings: []` in every case.

## The rule

The section that materializes the file owns the decision. Every other
section's entry for that path is marked `FindingKind::inventory()` with
`shadowed_by` set, **at collect time**, in
`pipeline::collect::finalize_cross_inspector`.

Doing it there and not in the renderer is what makes it stick: the
refine session refuses `SetInclude` on an inventory item with
`InventoryNotToggleable`, so the bad state is unreachable from the web,
CLI and TUI surfaces alike rather than merely warned about downstream.
`orchestrate.rs` is the wrong home — the production `scan` command calls
`collect()` directly and never `run_pipeline()`.

## The trap: a pass inside another pass's early returns

`finalize_cross_inspector` is a **dispatcher**. It calls each pass
unconditionally and holds no logic of its own:

```rust
pub fn finalize_cross_inspector(snapshot: &mut InspectionSnapshot) {
    shadow_sysctl_sources(snapshot);
    shadow_service_owned_paths(snapshot);
}
```

Each pass owns its own missing-section `return`s. The sysctl pass has
three: absent `kernel_boot`, empty override list, absent `config`.
Before this split the body of `finalize_cross_inspector` **was** the
sysctl pass, so a second pass appended to that body would not run on any
host that took one of those returns — a host with no sysctl overrides,
which is most of them. Nothing would report it: the annotation's absence
looks exactly like "no overlap on this host," no warning fires, and the
export is merely wrong.

Two unit tests in `collect::finalize_tests` pin this, one per direction:
`the_service_pass_runs_where_the_sysctl_pass_returns_early` and
`the_sysctl_pass_runs_where_the_service_pass_returns_early`.

## Name the variant for the set, not the field

`ConfigShadow::Services`, not `ServiceDropIn`: `services.drop_ins`
carries full unit shadows under `/etc/systemd/system/` as well as
`*.service.d/*.conf`, and a name describing the field would promise less
than the variant owns. This is the same defect as the earlier
`is_systemd_unit_alias`, which was named for aliases while testing for
something broader.

`ConfigShadow` is matched exhaustively in
`refine/projection/types.rs::config_ref_summary`; the compiler finds it,
but a new variant always needs a real summary string there, not a
catch-all arm.

## Testing an overlap

A verdict-only or tree-only assertion misses half of this class. For an
overlapping path, drive **all four combinations** of the two toggles
through `RefineSession` and assert on the staged file tree **and** the
Containerfile. Assert the contract, not a fixed table: *every decision
the session accepted is one the export obeys*, and exactly one of the
two toggles is accepted. A table of expected outcomes passes against the
buggy code, because carriage already followed the owner's disposition —
what was broken was that a second control was also accepted.

Use collector-produced input: `MockExecutor` plus the real inspectors
through `collect()`, as `crates/cli/tests/scan_service_shadow_test.rs`
and `scan_shadow_test.rs` do. No JSON fixture in the repository had a
non-empty `services.drop_ins` before that test, which is why this
survived a 26-task review cycle and two whole-branch reviews, and a
hand-built fixture is free to encode a layout the collectors cannot
produce.

## Known and unmeasured

Only the service case above has been measured. A read of the source
suggests the same or related failures for quadlets
(`containers.quadlet_units` plus the unconditional `QUADLET_PREFIX`
skip), repo files and GPG keys (an **OR** rule rather than a
subtraction: either inclusion stages the file), and local systemd timers
(`configtree.rs` writes them without checking `t.disposition`). None of
these has a host measurement. **Do not fix one from a source trace** —
this subsystem has produced a chain of confident, wrong conclusions
reasoned from unverified premises. Measure first.

## See Also

- `crates/pipeline/src/collect.rs` -- `finalize_cross_inspector` and the passes
- `crates/core/src/types/config.rs` -- `ConfigShadow`, `ConfigFileEntry::shadowed_by`
- `crates/pipeline/src/render/configtree.rs` -- `service_owned_paths`, `container_owned_paths`
- `crates/cli/tests/scan_service_shadow_test.rs` -- the four-state regression test
- `real-host-validation-runs.md` -- why measurement beats fixtures here
