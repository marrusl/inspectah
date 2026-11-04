---
name: cross-inspector-shadow-passes
description: Where a file lands in two inventories with two dispositions, one of them must be marked Inventory at collect time -- in its own pass, below finalize_cross_inspector, never inside a sibling pass's early returns. Also: absence from services.state_changes is not an exclusion.
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
`InventoryNotToggleable`, so the bad state is unreachable from the web
and CLI surfaces alike rather than merely warned about downstream.
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

## Shadowing moves the control; the surfaces have to follow it

Marking an entry `Inventory` only makes the bad state unreachable. On
its own it leaves a row whose checkbox vanished and nothing naming what
replaced it, which is a functional answer and not a usable one. Each
surface has to name the new owner and route to it.

Two things are easy to get wrong here:

- **There is no subtitle to wait for.** `CONTROLLED_IN_SERVICE_DROP_INS`
  is unreachable for a service shadow: only `project_ref_kernel_boot`
  builds a `RefConfigRef`, and a drop-in's `ConfigCategory` is `Other`,
  so it never enters `customizations_from_config`. Render the owner
  label from `shadowed_by` directly.
- **The owner row may not render at all.** A drop-in reaches
  `services.drop_ins` whenever the file exists, but its unit reaches
  `services.state_changes` only when the unit's enable state diverges
  from its preset. A unit at its preset state therefore has a drop-in
  and no parent service row, and the web `ServiceSection` nested
  drop-ins under their parent, so the owning row rendered nowhere. That
  is the common case, not an edge one. Before pointing an operator at an
  owner, check the owner is on screen.

The join key needs no new field: `service_dropins[].path` and
`sysctls[].source` are the same strings
`shadow_matching_config_entries` matched on. `shadowed_by` is
`skip_serializing_if = "Option::is_none"`, so a TypeScript mirror
declares it optional and never `null`.

## Absence from `state_changes` is not an exclusion

The same preset fact bites a second time, one layer down, and this one
cost the operator the file.

`session.rs` guarded `SetInclude(DropIn, true)` by requiring the parent
unit to be an **included** `state_changes` row. A unit at its preset
state has no row at all, so the test could never pass, and the drop-in
switched off once stayed off for the rest of the session. Exclusion was
unguarded, so the sequence was reachable in one click. The comment above
it said "cannot include a drop-in when its parent service is excluded",
which is a different and correct intent that the predicate did not
implement.

Write the predicate as the positive test for the state you mean to
refuse:

```rust
let parent_excluded = svc_section.state_changes.iter().any(|s| {
    s.unit == dropin.unit
        && matches!(s.disposition, FindingKind::Actionable { include: false })
});
```

Three things make the narrowed guard the right one, all measured on RHEL
9.8, 2026-11-01:

- **A drop-in whose parent carries no `systemctl enable` line is not
  broken.** Built the exported Containerfile and looked:
  `/etc/systemd/system/multi-user.target.wants/chronyd.service` comes
  from the base image's own preset, the drop-in sits beside it, and
  `systemctl is-enabled` reads `enabled` — the host's state, reproduced,
  with inspectah emitting nothing for that unit. There was no hazard to
  protect against.
- **The export cannot tell "no row" from "excluded row" anyway.**
  `render_service_intent` filters `state_changes` by `is_included()`, so
  both produce the same Containerfile.
- **What the guard really protects is the cascade.** `SetInclude` on a
  `Service` sets every drop-in for that unit to the same disposition, so
  re-including one drop-in alone would undo the operator's parent
  decision for a single file. That is a decision-model conflict, not an
  export defect, and it is the only state worth refusing.

The cascade is also the answer to a question that looks open from the
source: the web owner badge reads `service_dropins[].include`, so it
cannot report "Owner included" under an excluded parent — the cascade
has already set it false. Measured through `/api/view`.

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
- `crates/cli/tests/scan_service_shadow_test.rs` -- the four-state regression test, and both legs of the drop-in guard
- `crates/refine/src/session.rs` -- the `ItemId::DropIn` arm of `SetInclude` validation, and the `ItemId::Service` cascade
- `real-host-validation-runs.md` -- why measurement beats fixtures here
