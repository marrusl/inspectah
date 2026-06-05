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

## Step 1: Define your types

Create a new file in `inspectah-core/src/types/` for your section's data
structures. For example, if adding a `Firewall` inspector:

```rust
// inspectah-core/src/types/firewall.rs

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
    /// Defaults to true (unified include-default model).
    #[serde(default = "crate::default_true")]
    pub include: bool,
    /// When true, the UI prevents toggling this item's include state.
    /// Used for non-negotiable decisions (e.g., baseline-subtracted items).
    #[serde(default)]
    pub locked: bool,
}
```

Every toggleable item type follows this pattern: `include` defaults to `true`
via `crate::default_true`, and `locked` defaults to `false`. This is the
unified include-default model -- all items start included and are narrowed
during triage and fleet aggregation. Locked items cannot be toggled in the
refine UI.

Then register the module in `inspectah-core/src/types/mod.rs`:

```rust
pub mod firewall;
```

## Step 2: Add the InspectorId variant

In `inspectah-core/src/types/completeness.rs`, add a variant to the
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
    Hardware,
    Ostree,
    OsRelease,
    Firewall,  // <-- new
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

## Step 3: Implement the inspector

Create your inspector file in `inspectah-collect/src/inspectors/`. Use
the `StorageInspector` (237 lines) as a minimal reference pattern.

```rust
// inspectah-collect/src/inspectors/firewall.rs

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

- **`Ok(InspectorOutput)`** -- collection succeeded. Contains the typed
  `SectionData`, any warnings, and redaction hints.
- **`Err(InspectorError::Skipped { reason })`** -- inspector does not
  apply (e.g., the subsystem is not installed).
- **`Err(InspectorError::Degraded { partial, reason })`** -- partial
  data collected. Return what you have in the `partial` field.
- **`Err(InspectorError::Failed { .. })`** -- collection failed entirely.

### Using the Executor

Inspectors never call system commands directly. They use the `Executor`
trait, which enables mock-based testing:

```rust
// Run a shell command
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

## Step 4: Register the inspector module

Add your module to `inspectah-collect/src/inspectors/mod.rs`:

```rust
pub mod config;
pub mod containers;
pub mod firewall;  // <-- new
pub mod kernelboot;
pub mod network;
pub mod nonrpm;
pub mod rpm;
pub mod scheduled;
pub mod selinux;
pub mod services;
pub mod storage;
pub mod users;
```

## Step 5: Wire into the pipeline

Add your inspector to the pipeline's inspector list in
`inspectah-pipeline/src/collect.rs`. The pipeline passes a
`Vec<Box<dyn Inspector>>` to the `collect()` function. Add your
inspector to wherever this list is constructed:

```rust
Box::new(FirewallInspector::new()),
```

The pipeline handles applicability filtering, parallel execution, and
error routing automatically.

## Step 6: Handle the output in the snapshot

Update `InspectionSnapshot` in `inspectah-core` to include a field for
your section data. The pipeline's `handle_result` function routes each
`SectionData` variant to the corresponding snapshot field.

## Step 7: Write tests

Use the `MockExecutor` to test your inspector without running real
commands:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;
    use inspectah_core::traits::executor::ExecResult;
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
                "firewall-cmd",
                ExecResult {
                    stdout: "public (active)\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let source = test_source_system();
        let inspector = FirewallInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        assert!(result.is_ok());
    }
}
```

Key testing patterns:

- **`MockExecutor::with_command(cmd, result)`** -- stub a shell command
- **`MockExecutor::with_file(path, content)`** -- stub a file read
- **`MockExecutor::with_dir(path, entries)`** -- stub a directory listing
- **`NullProgress`** -- a no-op progress sink for tests
- Use snapshot tests (`insta::assert_json_snapshot!`) for complex outputs

## Checklist

Before submitting your PR:

- [ ] Types defined in `inspectah-core/src/types/`
- [ ] `InspectorId` variant added to `completeness.rs`
- [ ] `SectionData` variant added to `completeness.rs`
- [ ] Inspector struct created in `inspectah-collect/src/inspectors/`
- [ ] Module registered in `inspectors/mod.rs`
- [ ] Inspector wired into the pipeline's inspector list
- [ ] Snapshot field added for the new section
- [ ] Tests written using `MockExecutor`
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace -- -D warnings` clean
