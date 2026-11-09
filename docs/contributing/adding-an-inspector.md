---
title: Adding an Inspector
parent: Contributing
nav_order: 2
---

# Adding an Inspector

Step-by-step guide for implementing a new inspector that collects data
from a system domain.

## Overview

Inspectors are the data-collection layer. Each inspector examines one
aspect of a running system (storage, networking, SELinux, etc.) and
returns structured data. The pipeline orchestrates inspectors, handles
failures gracefully, and feeds results into rendering.

The `Firewall` inspector used throughout this walkthrough is hypothetical:
no `firewall.rs` exists in the tree, so the code blocks are schematic. They are written against the real trait, enum
and `MockExecutor` API, so they compile once the data-collection body is
filled in.

## Step 1: Define your types

Create a new file in `crates/core/src/types/` for your section's data
structures. For example, if adding a `Firewall` inspector:

```rust
// crates/core/src/types/firewall.rs

use super::FindingKind;
use super::aggregate::AggregatePrevalence;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FirewallSection {
    pub zones: Vec<FirewallZone>,
    pub rich_rules: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FirewallZone {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub services: Vec<String>,
    /// Whether this item is included in the generated Containerfile.
    /// An actionable item keeps the bare `serde(default)`, which resolves
    /// to `FindingKind::default()`: actionable and included.
    #[serde(default)]
    pub disposition: FindingKind,
    /// When true, the UI prevents toggling this item's disposition.
    /// Used for non-negotiable decisions (e.g., baseline-subtracted items).
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    /// Prevalence across hosts, filled in by `inspectah aggregate`.
    pub aggregate: Option<AggregatePrevalence>,
}
```

Every item type that a user can include or exclude carries a
`disposition: FindingKind` field, a `locked` flag, and an `aggregate`
field. `FindingKind` is `Actionable { include }`, `Advisory { .. }` or
`Inventory`.

A `disposition` field that is not actionable by default must name its
default function, because the bare `#[serde(default)]` always produces an
actionable, included item, and an absent key in a hand-edited or older
snapshot would silently render into the Containerfile. Use
`crate::types::finding::default_finding_inventory` for informational items
and `crate::types::finding::default_finding_excluded` for items that must
not be baked in without an explicit decision. `KernelModule` in
`crates/core/src/types/kernelboot.rs` is a real example of the first. See
`process-docs/skills/finding-disposition-serde-defaults.md` in the
repository for the reasoning and the test to add.

Then register the module in `crates/core/src/types/mod.rs`:

```rust
pub mod firewall;
```

## Step 2: Add the InspectorId variant

In `crates/core/src/types/completeness.rs`, add a variant to the
`InspectorId` enum:

```rust
pub enum InspectorId {
    Rpm,
    Config,
    Services,
    Network,
    Storage,
    ScheduledTasks,
    Containers,
    NonRpmSoftware,
    KernelBoot,
    Selinux,
    UsersGroups,
    Subscription,
    Firewall,  // <-- new
    Hardware,
    Ostree,
    OsRelease,
}
```

And add a variant to the `SectionData` enum in the same file:

```rust
pub enum SectionData {
    // ... existing variants ...
    #[serde(rename = "firewall")]
    Firewall(super::firewall::FirewallSection),
}
```

## Step 3: Bump the snapshot schema

`InspectorId` is serialized: it appears in the snapshot's `Completeness`
record (`degraded_sections`, `failed_sections`). Adding a variant to a
serialized enum is a schema change, so raise `SCHEMA_VERSION` and
`MIN_SCHEMA` together in `crates/core/src/snapshot.rs`. A new
`SectionData` variant and a new `Option` field on the snapshot would not
need a bump on their own; the new `InspectorId` variant is what does.

The bump touches more than the two constants:

- The test files that embed `"schema_version": N` literals, and the
  `snapshot.rs` unit tests that name accepted and rejected versions. The
  list is in `process-docs/skills/snapshot-schema-versioning.md`.
- `testdata/e2e-fixture.tar.gz`. The guard test
  `crates/refine/tests/testdata_tarballs_load.rs` loads every tarball under
  `testdata/` and fails until this one is regenerated at the new schema.

Raising the schema rejects every snapshot taken before it, so it belongs
in the changelog (see the checklist).

## Step 4: Implement the inspector

Create your inspector file in `crates/collect/src/inspectors/`. Use
the `KernelbootInspector` (`kernelboot.rs`, about 600 lines, the smallest
inspector in the tree) as a reference pattern.

```rust
// crates/collect/src/inspectors/firewall.rs

use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::traits::progress::ProgressSink;
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::firewall::FirewallSection;

pub struct FirewallInspector;

impl FirewallInspector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FirewallInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl Inspector for FirewallInspector {
    fn id(&self) -> InspectorId {
        InspectorId::Firewall
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        // Which system types this inspector runs on.
        // Most inspectors apply to PackageBased systems.
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(
        &self,
        ctx: &InspectionContext<'_>,
        _progress: &dyn ProgressSink,
    ) -> Result<InspectorOutput, InspectorError> {
        let exec = ctx.executor;

        // Collect data using the Executor trait.
        // exec.run(command, args) runs shell commands.
        // exec.read_file(path) reads file contents.
        let section = collect_firewall_data(exec)?;

        Ok(InspectorOutput {
            section: SectionData::Firewall(section),
            warnings: Vec::new(),
            redaction_hints: Vec::new(),
        })
    }
}

fn collect_firewall_data(
    exec: &dyn Executor,
) -> Result<FirewallSection, InspectorError> {
    // Implementation here -- use exec.run() for commands,
    // exec.read_file() for file contents.
    todo!()
}
```

### The Inspector trait

Every inspector implements three methods:

| Method | Purpose |
|---|---|
| `id()` | Returns the `InspectorId` variant for this inspector |
| `applicable_to()` | Returns which `SourceSystemKind` values this inspector supports (`PackageBased`, `RpmOstree`, `Bootc`) |
| `inspect()` | Performs the actual data collection and returns `InspectorOutput` |

### Return types

The `inspect` method returns `Result<InspectorOutput, InspectorError>`:

- **`Ok(InspectorOutput)`**: collection succeeded. Contains the typed
  `SectionData`, any warnings, and redaction hints.
- **`Err(InspectorError::Skipped { reason })`**: inspector does not
  apply (e.g., the subsystem is not installed).
- **`Err(InspectorError::Degraded { partial, reason })`**: partial
  data collected. Return what you have in the `partial` field.
- **`Err(InspectorError::Failed { .. })`**: collection failed entirely.

### RPM state in the second wave

The pipeline runs the RPM inspector first. Every other inspector except
`Subscription` runs in a second wave and receives the RPM inspector's
results as `ctx.rpm_state`. If that is `None`, the RPM inspector failed,
and a second-wave inspector must return `InspectorError::Failed` rather
than classify anything against missing ownership data:

```rust
if ctx.rpm_state.is_none() {
    return Err(InspectorError::Failed {
        reason: "RPM prerequisite unavailable".into(),
    });
}
```

`Some` with empty fields means the RPM inspector succeeded and found
nothing, which is a different fact and needs no special handling. The
contract is documented on `RpmState` in
`crates/core/src/traits/inspector.rs`, and `SelinuxInspector` in
`crates/collect/src/inspectors/selinux.rs` shows the check in use.

### Using the Executor

Inspectors never call system commands directly. They use the `Executor`
trait, which enables mock-based testing. Besides `run` and `read_file`,
it also provides `file_exists`, `read_dir`, `read_link` and `host_root`:

```rust
// Run a command. The command and its arguments are separate parameters.
let result = exec.run("firewall-cmd", &["--list-all-zones"]);
if !result.success() {
    return Err(InspectorError::Failed { /* ... */ });
}
let stdout = &result.stdout;

// Read a file
let content = exec.read_file(Path::new("/etc/firewalld/firewalld.conf"))
    .map_err(|e| InspectorError::Failed {
        reason: format!("cannot read firewalld.conf: {e}"),
    })?;
```

## Step 5: Register the inspector module

Add your module to `crates/collect/src/inspectors/mod.rs`:

```rust
pub mod config;
pub mod containers;
pub mod firewall;  // <-- new
pub mod kernelboot;
pub mod modernization;
pub mod network;
pub mod nonrpm;
pub mod rpm;
pub mod scheduled;
pub mod selinux;
pub mod services;
pub mod storage;
pub mod subscription;
pub mod users;
```

## Step 6: Wire into the pipeline

Three places need the new inspector. The compiler checks only the
second one, so check the other two by hand.

1. **Register it for scans.** The production inspector list is built in
   `crates/cli/src/commands/scan.rs`, in a `Vec<Box<dyn Inspector>>`.
   Import the type and add it to the list:

   ```rust
   Box::new(FirewallInspector::new()),
   ```

   The pipeline's `collect()` in `crates/pipeline/src/collect.rs` takes that
   list and handles applicability filtering, parallel execution and error
   routing. It does not build the list; only its tests do.

2. **Route the output.** See Step 7.

3. **Add it to the scan checklist.** The progress display in
   `crates/cli/src/progress/display.rs` has a fixed `DISPLAY_ORDER` table
   of 12 entries, `Subscription` last. Add the inspector before that
   trailing `Subscription` entry, making 13, then change the hard-coded
   `&DISPLAY_ORDER[..11]` slice in `active_display_order`, which drops the
   `Subscription` entry when it is inactive, to `&DISPLAY_ORDER[..12]`.
   The compiler does not check either: `display_position` returns `0` for
   an ID that is missing from the table, and
   `test_cli_creates_all_inspectors` in `scan.rs` builds its own copy of
   the list, so it passes whether or not you registered anything. Update
   the counts in the two doc comments in `display.rs` (12 inspectors on
   `DISPLAY_ORDER`, 11 items on `active_display_order`) and its three
   length assertions, 12, 11 and 12, to 13, 12 and 13, rename
   `display_order_has_12_entries` to match, and change the expected
   `Subscription` position in `display_position_subscription` from 12
   to 13.

## Step 7: Handle the output in the snapshot

Add a field for your section to `InspectionSnapshot` in
`crates/core/src/snapshot.rs`, following the existing `subscription`
field:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub firewall: Option<FirewallSection>,
```

The pipeline's `route_section` function in
`crates/pipeline/src/collect.rs` matches each `SectionData` variant to its
snapshot field. The match is exhaustive, so the compiler stops at the arm
you need to add:

```rust
SectionData::Firewall(s) => snapshot.firewall = Some(s),
```

The report renderer, `crates/pipeline/src/render/report.rs`, also matches
exhaustively on `InspectorId`, so the compiler points at those arms too.

Three things are not compiler-checked. The first two are hand-maintained
lists in `crates/core/src/aggregate/mod.rs`:

- **Aggregation merge.** `inspectah aggregate` builds its output from an
  empty snapshot and assigns each section from a `merge_*_sections` call
  (the merge functions are in `crates/core/src/aggregate/merge.rs`). A
  section with no merge call is silently dropped from every aggregate.
  Add a `merge_firewall_sections` and call it.
- **Aggregation coverage.** `compute_section_host_counts` keeps its own
  `section_checkers` list, which fills
  `aggregate_meta.section_host_counts`. `inspectah aggregate --verbose`
  prints that map as "Section coverage", and a section missing from the
  list is missing from Section coverage. Add a `firewall` entry.
- **Rendering and refine.** The Containerfile renderer, the refine
  engine and the web adapter see nothing from the new section until you
  wire it into each.

## Step 8: Write tests

Use the `MockExecutor` to test your inspector without running real
commands:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;
    use inspectah_core::traits::executor::ExecResult;
    use inspectah_core::traits::inspector::RpmState;
    use inspectah_core::traits::progress::NullProgress;
    use inspectah_core::types::os::OsRelease;
    use inspectah_core::types::system::SourceSystem;

    fn test_source_system() -> SourceSystem {
        SourceSystem::PackageBased {
            os_release: OsRelease {
                id: "rhel".into(),
                version_id: "9.4".into(),
                ..Default::default()
            },
        }
    }

    #[test]
    fn test_basic_collection() {
        let exec = MockExecutor::new()
            .with_command(
                "firewall-cmd --list-all-zones",
                ExecResult {
                    stdout: "public (active)\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let source = test_source_system();
        let rpm_state = RpmState::default();
        let inspector = FirewallInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        assert!(result.is_ok());
    }
}
```

Key testing patterns:

- **`MockExecutor::with_command(key, result)`**: stub a shell command.
  The key is the full command line, the command and its arguments joined
  by single spaces. `with_command("firewall-cmd", ..)` does not match
  `run("firewall-cmd", &["--list-all-zones"])`, which returns exit code 127.
  `with_command_prefix` matches on a prefix instead.
- **`MockExecutor::with_file(path, content)`**: stub a file read
- **`MockExecutor::with_dir(path, entries)`**: stub a directory listing
- **`MockExecutor::with_link`, `with_file_error`, `with_dir_error`**:
  stub a symlink or a read failure
- **`MockExecutor::command_log()`**: the ordered list of commands run
- **`NullProgress`**: a no-op progress sink for tests
- Pass `rpm_state: Some(&RpmState::default())` in the context; a
  second-wave inspector returns `Failed` when it is `None`
- Use snapshot tests (`insta::assert_json_snapshot!`) for complex outputs

## Checklist

Before submitting your PR:

- [ ] Types defined in `crates/core/src/types/`, with a `disposition`
      default that fits the item (Step 1)
- [ ] `InspectorId` variant added to `completeness.rs`
- [ ] `SectionData` variant added to `completeness.rs`
- [ ] `SCHEMA_VERSION` and `MIN_SCHEMA` bumped together, and
      `testdata/e2e-fixture.tar.gz` regenerated (Step 3)
- [ ] Inspector struct created in `crates/collect/src/inspectors/`
- [ ] Module registered in `inspectors/mod.rs`
- [ ] Inspector added to the list in `crates/cli/src/commands/scan.rs`
- [ ] Inspector added to `DISPLAY_ORDER` before `Subscription`, the
      `[..11]` slice changed to `[..12]`, and the counts in the
      `display.rs` doc comments and the length and position assertions
      updated
- [ ] Snapshot field added, and the `route_section` arm added
- [ ] `merge_*_sections` added and called for aggregation, and the
      section added to `section_checkers` in
      `compute_section_host_counts`
- [ ] Tests written using `MockExecutor`
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace --all-targets -- -D clippy::all` clean
- [ ] `CHANGELOG.md` entry under `## [Unreleased]`, per
      `process-docs/changelog-policy.md`; the schema bump is a
      breaking change and gets its own mention
- [ ] Run against a real host. Every new inspector needs one, because mocks
      encode what you expect the host to print. Test on a stock host
      before a drifted one; see [Testing](testing.md)
