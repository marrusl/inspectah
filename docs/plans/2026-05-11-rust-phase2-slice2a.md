# Phase 2 Slice 2a: Foundation Inspectors Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement services, storage, and kernelboot inspectors with parallel execution, CI, and parity gate — proving the Phase 2 inspector pattern on the approved borrowed-context contract.

**Architecture:** Each inspector implements the approved `Inspector` trait from `inspectah-core`, receiving `&InspectionContext` with borrowed executor and source system. A three-wave `std::thread::scope` model runs RPM alongside independent inspectors in Wave 1, joins RPM for `RpmState`, then spawns dependent inspectors in Wave 2 (Slice 2c). All inspector output passes through the redaction engine before reaching renderers.

**Tech Stack:** Rust 2021 edition, serde/serde_json, insta (snapshot testing), GitHub Actions CI

**Spec:** `docs/specs/2026-05-11-phase2-inspector-parity-design.md`

**Reference implementation:** `inspectah-collect/src/inspectors/rpm/mod.rs`

**Revision 2 — addresses all review findings from Tang, Thorn, Collins, Press, Slate.**

---

## File Map

### New Files

| File | Responsibility |
|------|---------------|
| `inspectah-collect/src/inspectors/services.rs` | Services inspector |
| `inspectah-collect/src/inspectors/storage.rs` | Storage inspector |
| `inspectah-collect/src/inspectors/kernelboot.rs` | Kernel/boot inspector |
| `inspectah-collect/tests/services_test.rs` | Services unit tests with MockExecutor |
| `inspectah-collect/tests/storage_test.rs` | Storage unit tests with MockExecutor |
| `inspectah-collect/tests/kernelboot_test.rs` | Kernelboot unit tests with MockExecutor |
| `inspectah-pipeline/tests/smoke_render.rs` | Renderer smoke tests — all 3 sections against correct artifact consumers |
| `inspectah-pipeline/tests/failure_policy.rs` | Failure policy — degraded/failed/panic/dependency/redaction/trust |
| `inspectah-pipeline/tests/parallel_test.rs` | Parallel execution model tests |
| `testdata/fixtures/services/` | MockExecutor fixtures for services |
| `testdata/fixtures/storage/` | MockExecutor fixtures for storage |
| `testdata/fixtures/kernelboot/` | MockExecutor fixtures for kernelboot |
| `testdata/golden/go-v13-services-section.json` | Go golden — services parity |
| `testdata/golden/go-v13-storage-section.json` | Go golden — storage parity |
| `testdata/golden/go-v13-kernelboot-section.json` | Go golden — kernelboot parity |
| `testdata/evidence/slice-2a-host-validation.md` | Committed live-host evidence (filled, not template) |
| `.github/workflows/rust-ci.yml` | Rust CI (Tier 1 + Tier 2) |

### Modified Files

| File | Change |
|------|--------|
| `inspectah-core/src/traits/inspector.rs` | Refactor `InspectionContext` to borrowed shape with lifetime parameter |
| `inspectah-core/src/types/system.rs` | Add `SourceSystem::kind()` |
| `inspectah-core/src/types/completeness.rs` | Distinguish `Partial` (degraded) from `Incomplete` (failed) in `Completeness` |
| `inspectah-collect/src/inspectors/rpm/mod.rs` | Update to borrowed `InspectionContext<'_>` |
| `inspectah-collect/src/inspectors/mod.rs` | Register new inspector modules |
| `inspectah-collect/src/executor/mock.rs` | Add `LC_ALL=C` normalization, timeout simulation support |
| `inspectah-collect/src/executor/real.rs` | Add `LC_ALL=C` enforcement, timeout, output size cap |
| `inspectah-pipeline/src/collect.rs` | Refactor to scoped-thread three-wave parallel execution |
| `inspectah-pipeline/src/orchestrate.rs` | Wire new inspectors, applicability-based Skipped routing |
| `inspectah-pipeline/src/redaction.rs` | Extend patterns for new persisted surfaces |
| `inspectah-core/tests/parity_gate.rs` | Expand to services, storage, kernelboot sections |
| `testdata/divergences.md` | Add any new divergence entries |

---

## Task 1: CI Workflow

**Files:**
- Create: `.github/workflows/rust-ci.yml`

- [ ] **Step 1: Create Tier 1 + Tier 2 CI workflow**

```yaml
name: Rust CI

on:
  push:
    branches: [rust]
    paths:
      - 'inspectah-*/src/**'
      - 'inspectah-*/tests/**'
      - 'inspectah-*/Cargo.toml'
      - 'Cargo.toml'
      - 'Cargo.lock'
      - 'testdata/**'
      - '.github/workflows/rust-ci.yml'
  pull_request:
    branches: [rust]

jobs:
  tier1:
    name: Format, Lint, Test
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - uses: Swatinem/rust-cache@v2
      - name: Check formatting
        run: cargo fmt --all -- --check
      - name: Clippy
        run: cargo clippy --workspace -- -W clippy::all
      - name: Test (no ffi-rpm)
        run: cargo test --workspace

  tier2:
    name: Test with librpm FFI
    runs-on: ubuntu-latest
    container:
      image: fedora:latest
    steps:
      - uses: actions/checkout@v4
      - name: Install dependencies
        run: dnf install -y gcc rpm-devel pkg-config rust cargo clippy
      - name: Test with ffi-rpm
        run: cargo test --workspace --features ffi-rpm
```

Note: `push.paths` includes `testdata/**` (golden files, divergences, fixtures, evidence) and the workflow file itself — all parity-bearing surfaces trigger CI. The `pull_request` trigger has no path filter so all PRs run CI.

- [ ] **Step 2: Verify existing tests pass locally**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test --workspace 2>&1 | tail -5`
Expected: All existing tests pass.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/rust-ci.yml
git commit -m "ci: add Rust CI workflow with Tier 1 and Tier 2 gates"
```

---

## Task 2: Align InspectionContext to Approved Borrowed Shape

**Files:**
- Modify: `inspectah-core/src/traits/inspector.rs`
- Modify: `inspectah-core/src/types/system.rs`
- Modify: `inspectah-core/src/types/completeness.rs`
- Modify: `inspectah-collect/src/inspectors/rpm/mod.rs`
- Modify: `inspectah-pipeline/src/collect.rs`
- Modify: `inspectah-pipeline/src/orchestrate.rs`
- Modify: `inspectah-cli/src/commands/scan.rs`

This task is the prerequisite for all Phase 2 work. It aligns the codebase to the approved contract before any new inspector is added.

### Why first

The approved spec requires borrowed `&InspectionContext` with `&dyn Executor` and `Option<&RpmState>`. The current code uses owned `Box<dyn Executor>` and `Option<RpmState>`. If inspectors are written against the owned shape, Task 8 (parallel execution) would require undoing that work. The scoped-thread model needs two `InspectionContext` values sharing one executor — only possible with borrowed references.

- [ ] **Step 1: Write test for SourceSystem::kind()**

Add to `inspectah-core/src/types/system.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_system_kind_derivation() {
        let pkg = SourceSystem::PackageBased {
            os_release: OsRelease::default(),
        };
        assert_eq!(pkg.kind(), SourceSystemKind::PackageBased);

        let ostree = SourceSystem::RpmOstree {
            os_release: OsRelease::default(),
            variant: OstreeVariant::Unknown("test".into()),
            base_image: None,
        };
        assert_eq!(ostree.kind(), SourceSystemKind::RpmOstree);

        let bootc = SourceSystem::Bootc {
            os_release: OsRelease::default(),
            booted_image: "registry.example.com/img:latest".into(),
            staged_image: None,
        };
        assert_eq!(bootc.kind(), SourceSystemKind::Bootc);
    }
}
```

- [ ] **Step 2: Implement SourceSystem::kind()**

```rust
impl SourceSystem {
    pub fn kind(&self) -> SourceSystemKind {
        match self {
            SourceSystem::PackageBased { .. } => SourceSystemKind::PackageBased,
            SourceSystem::RpmOstree { .. } => SourceSystemKind::RpmOstree,
            SourceSystem::Bootc { .. } => SourceSystemKind::Bootc,
        }
    }
}
```

Add `PartialEq, Eq` derives to `SourceSystemKind` if not already present.

- [ ] **Step 3: Refactor InspectionContext to borrowed shape**

Change `inspectah-core/src/traits/inspector.rs`:

From:
```rust
pub struct InspectionContext {
    pub executor: Box<dyn Executor>,
    pub source: SourceSystem,
    pub rpm_state: Option<RpmState>,
}
```

To:
```rust
pub struct InspectionContext<'a> {
    pub source: &'a SourceSystem,
    pub executor: &'a dyn Executor,
    pub rpm_state: Option<&'a RpmState>,
}
```

Update the `Inspector` trait:
```rust
pub trait Inspector: Send + Sync {
    fn id(&self) -> InspectorId;
    fn applicable_to(&self) -> &[SourceSystemKind];
    fn inspect(&self, ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError>;
}
```

- [ ] **Step 4: Update Completeness to distinguish Degraded from Failed**

In `inspectah-core/src/types/completeness.rs`, the current `Completeness` enum has `Full`, `Partial`, `Unverified`. The approved spec requires distinguishing degraded (usable data with gaps) from failed (no usable data). Update:

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum Completeness {
    #[default]
    Complete,
    Partial {
        degraded_sections: Vec<InspectorId>,
        reason: String,
    },
    Incomplete {
        failed_sections: Vec<InspectorId>,
        degraded_sections: Vec<InspectorId>,
        reason: String,
    },
}
```

This replaces the prior `Full`/`Partial`/`Unverified` with the spec's `Complete`/`Partial`/`Incomplete`. Ensure JSON serialization remains backward-compatible (the parity gate normalizer already strips completeness, so the rename is safe for parity).

- [ ] **Step 5: Fix all compilation errors from the refactor**

Update the RPM inspector, pipeline `collect()`, `orchestrate()`, CLI `scan.rs`, and all existing tests to use the new borrowed shape. This is a mechanical refactor — every `Box::new(exec)` becomes `&exec`, every `.source` becomes `.source` (already a field, just now a reference).

The key pattern for test helpers:

```rust
fn make_ctx<'a>(
    executor: &'a dyn Executor,
    source: &'a SourceSystem,
) -> InspectionContext<'a> {
    InspectionContext {
        source,
        executor,
        rpm_state: None,
    }
}
```

- [ ] **Step 6: Run full test suite**

Run: `cargo test --workspace`
Expected: All existing tests pass with no behavioral changes.

- [ ] **Step 7: Run clippy**

Run: `cargo clippy --workspace -- -W clippy::all`
Expected: Zero warnings.

- [ ] **Step 8: Commit**

```bash
git add inspectah-core/ inspectah-collect/ inspectah-pipeline/ inspectah-cli/
git commit -m "refactor(core): align InspectionContext to borrowed shape for scoped-thread execution"
```

---

## Task 3: Collector Boundary Enforcement

**Files:**
- Modify: `inspectah-collect/src/executor/real.rs`
- Modify: `inspectah-collect/src/executor/mock.rs`
- Create: `inspectah-collect/tests/executor_boundary_test.rs`

The approved spec mandates: fixed argv (never shell strings), `LC_ALL=C` normalization, absolute PATH resolution, per-command timeout, stdout size cap (64 MB), file size cap (1 MB). These must be enforced at the `Executor` level before new inspectors add more command/file surfaces.

- [ ] **Step 1: Write boundary tests**

Create `inspectah-collect/tests/executor_boundary_test.rs`:

```rust
use inspectah_collect::executor::real::RealExecutor;
use inspectah_core::traits::executor::Executor;

#[test]
fn test_commands_run_with_c_locale() {
    let exec = RealExecutor::new();
    // Run `locale` and verify LANG=C or LC_ALL=C is set
    let result = exec.run("locale", &[]);
    assert!(
        result.stdout.contains("LC_ALL=C") || result.stdout.contains("LANG=C"),
        "executor must force C locale"
    );
}

#[test]
fn test_executor_uses_fixed_argv() {
    let exec = RealExecutor::new();
    // Shell metacharacters must not be interpreted
    let result = exec.run("echo", &["hello; rm -rf /"]);
    assert_eq!(result.stdout.trim(), "hello; rm -rf /");
}
```

- [ ] **Step 2: Implement locale and timeout enforcement in RealExecutor**

Modify `inspectah-collect/src/executor/real.rs`:
- In the `run()` method, set `LC_ALL=C` and `LANG=C` environment variables on the `Command`
- Add a configurable timeout (default 30s) that kills the child process and returns a non-zero `ExecResult`
- Add stdout size cap (64 MB) — read output in chunks, truncate if exceeded
- Resolve command paths against `/usr/bin/` and `/usr/sbin/` rather than relying on `$PATH`

Modify `inspectah-collect/src/executor/mock.rs`:
- No behavioral changes needed — MockExecutor already returns canned output
- Add a `with_timeout_simulation(cmd, duration)` builder for testing inspector timeout handling

- [ ] **Step 3: Add file size cap to read_file()**

In `RealExecutor::read_file()`, cap individual file reads at 1 MB. Files exceeding this return `Err(io::Error)` with a descriptive message.

- [ ] **Step 4: Run tests**

Run: `cargo test --workspace`
Expected: All tests pass. New boundary tests pass.

- [ ] **Step 5: Commit**

```bash
git add inspectah-collect/src/executor/ inspectah-collect/tests/executor_boundary_test.rs
git commit -m "feat(collect): enforce collector boundary contract — locale, timeout, size caps"
```

---

## Task 4: Services Inspector

**Files:**
- Create: `inspectah-collect/src/inspectors/services.rs`
- Create: `inspectah-collect/tests/services_test.rs`
- Create: `testdata/fixtures/services/systemctl-list-unit-files.txt`
- Create: `testdata/fixtures/services/preset-90-default.preset`
- Create: `testdata/fixtures/services/dropin-httpd-override.conf`
- Modify: `inspectah-collect/src/inspectors/mod.rs`

### Reference: Go services.go

1. Runs `systemctl list-unit-files --type=service --no-pager` — parses UNIT/STATE/PRESET columns
2. Reads preset files from `/usr/lib/systemd/system-preset/*.preset` and `/etc/systemd/system-preset/*.preset` — first-match-wins glob semantics
3. Compares current state vs. preset default — divergences become `ServiceStateChange`
4. Scans `/etc/systemd/system/*.service.d/` for drop-in `.conf` files
5. Filters template units (`@.service`) and static units

- [ ] **Step 1: Create fixture files**

Create `testdata/fixtures/services/systemctl-list-unit-files.txt`:
```
UNIT FILE                                  STATE           PRESET
auditd.service                             enabled         enabled
bluetooth.service                          enabled         disabled
chronyd.service                            enabled         enabled
cups.service                               disabled        disabled
firewalld.service                          enabled         enabled
gdm.service                               disabled        enabled
httpd.service                              enabled         disabled
kdump.service                              enabled         enabled
libvirtd.service                           enabled         disabled
NetworkManager.service                     enabled         enabled
sshd.service                               enabled         enabled
tuned.service                              enabled         enabled

12 unit files listed.
```

Create `testdata/fixtures/services/preset-90-default.preset`:
```
enable auditd.service
enable chronyd.service
enable firewalld.service
enable gdm.service
disable bluetooth.service
enable kdump.service
enable NetworkManager.service
enable sshd.service
enable tuned.service
```

Create `testdata/fixtures/services/dropin-httpd-override.conf`:
```
[Service]
Environment=LANG=C
LimitNOFILE=65535
```

- [ ] **Step 2: Register module**

Add to `inspectah-collect/src/inspectors/mod.rs`:
```rust
pub mod services;
```

- [ ] **Step 3: Write failing tests**

Create `inspectah-collect/tests/services_test.rs`. All tests construct `InspectionContext` using the **borrowed** shape from Task 2:

```rust
use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::services::ServicesInspector;
use inspectah_core::traits::inspector::{Inspector, InspectionContext, InspectorError};
use inspectah_core::types::system::{SourceSystem, SourceSystemKind};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::completeness::SectionData;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("../testdata/fixtures/services/{}", name)).unwrap()
}

fn pkg_source() -> SourceSystem {
    SourceSystem::PackageBased { os_release: OsRelease::default() }
}

// --- Applicability ---

#[test]
fn applicability_package_mode_only() {
    let inspector = ServicesInspector;
    assert_eq!(inspector.applicable_to(), &[SourceSystemKind::PackageBased]);
}

// --- Happy path ---

#[test]
fn happy_path_state_changes() {
    let exec = MockExecutor::new()
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            inspectah_core::traits::executor::ExecResult {
                stdout: fixture("systemctl-list-unit-files.txt"),
                stderr: String::new(),
                exit_code: 0,
            },
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file("/usr/lib/systemd/system-preset/90-default.preset",
            &fixture("preset-90-default.preset"))
        .with_dir("/etc/systemd/system-preset", vec![])
        .with_dir("/etc/systemd/system", vec![]);

    let source = pkg_source();
    let ctx = InspectionContext { source: &source, executor: &exec, rpm_state: None };
    let result = ServicesInspector.inspect(&ctx).unwrap();

    match &result.section {
        SectionData::Services(svc) => {
            // bluetooth enabled, preset disable → state change (action: enable→disable)
            assert!(svc.state_changes.iter().any(|s| s.unit == "bluetooth.service"
                && s.current_state == "enabled" && s.default_state == "disabled"));
            // gdm disabled, preset enable → state change
            assert!(svc.state_changes.iter().any(|s| s.unit == "gdm.service"
                && s.current_state == "disabled" && s.default_state == "enabled"));
            // httpd enabled, no preset → state change (not in preset file)
            assert!(svc.state_changes.iter().any(|s| s.unit == "httpd.service"));
            // libvirtd enabled, no preset → state change
            assert!(svc.state_changes.iter().any(|s| s.unit == "libvirtd.service"));
            // auditd enabled, preset enable → no state change
            assert!(!svc.state_changes.iter().any(|s| s.unit == "auditd.service"));
            // cups disabled, preset not present or disable → no state change
            assert!(!svc.state_changes.iter().any(|s| s.unit == "cups.service"));
        }
        _ => panic!("expected Services section"),
    }
}

// --- Preset matching ---

#[test]
fn preset_first_match_wins() {
    // Two preset files: first says "disable sshd", second says "enable sshd"
    // First-match-wins → sshd should be "disable" as default
    let exec = MockExecutor::new()
        .with_command("systemctl list-unit-files --type=service --no-pager",
            inspectah_core::traits::executor::ExecResult {
                stdout: "UNIT FILE STATE PRESET\nsshd.service enabled enabled\n\n1 unit files listed.\n".into(),
                stderr: String::new(), exit_code: 0,
            })
        .with_dir("/usr/lib/systemd/system-preset",
            vec!["50-custom.preset", "90-default.preset"])
        .with_file("/usr/lib/systemd/system-preset/50-custom.preset", "disable sshd.service\n")
        .with_file("/usr/lib/systemd/system-preset/90-default.preset", "enable sshd.service\n")
        .with_dir("/etc/systemd/system-preset", vec![])
        .with_dir("/etc/systemd/system", vec![]);

    let source = pkg_source();
    let ctx = InspectionContext { source: &source, executor: &exec, rpm_state: None };
    let result = ServicesInspector.inspect(&ctx).unwrap();

    match &result.section {
        SectionData::Services(svc) => {
            let sshd = svc.state_changes.iter().find(|s| s.unit == "sshd.service");
            assert!(sshd.is_some(), "sshd should have state change — enabled vs preset-disable");
            assert_eq!(sshd.unwrap().default_state, "disabled");
        }
        _ => panic!("expected Services section"),
    }
}

#[test]
fn preset_glob_matching() {
    // Preset with glob: "enable *" should match any service
    let exec = MockExecutor::new()
        .with_command("systemctl list-unit-files --type=service --no-pager",
            inspectah_core::traits::executor::ExecResult {
                stdout: "UNIT FILE STATE PRESET\ncustom.service disabled enabled\n\n1 unit files listed.\n".into(),
                stderr: String::new(), exit_code: 0,
            })
        .with_dir("/usr/lib/systemd/system-preset", vec!["99-catch-all.preset"])
        .with_file("/usr/lib/systemd/system-preset/99-catch-all.preset", "enable *\n")
        .with_dir("/etc/systemd/system-preset", vec![])
        .with_dir("/etc/systemd/system", vec![]);

    let source = pkg_source();
    let ctx = InspectionContext { source: &source, executor: &exec, rpm_state: None };
    let result = ServicesInspector.inspect(&ctx).unwrap();

    match &result.section {
        SectionData::Services(svc) => {
            let custom = svc.state_changes.iter().find(|s| s.unit == "custom.service");
            assert!(custom.is_some(), "custom.service disabled vs glob-enable → state change");
        }
        _ => panic!("expected Services section"),
    }
}

// --- Drop-in detection ---

#[test]
fn dropin_files_collected() {
    let exec = MockExecutor::new()
        .with_command("systemctl list-unit-files --type=service --no-pager",
            inspectah_core::traits::executor::ExecResult {
                stdout: "UNIT FILE STATE PRESET\nhttpd.service enabled disabled\n\n1 unit files listed.\n".into(),
                stderr: String::new(), exit_code: 0,
            })
        .with_dir("/usr/lib/systemd/system-preset", vec![])
        .with_dir("/etc/systemd/system-preset", vec![])
        .with_dir("/etc/systemd/system", vec!["httpd.service.d"])
        .with_dir("/etc/systemd/system/httpd.service.d", vec!["override.conf"])
        .with_file("/etc/systemd/system/httpd.service.d/override.conf",
            &fixture("dropin-httpd-override.conf"));

    let source = pkg_source();
    let ctx = InspectionContext { source: &source, executor: &exec, rpm_state: None };
    let result = ServicesInspector.inspect(&ctx).unwrap();

    match &result.section {
        SectionData::Services(svc) => {
            assert_eq!(svc.drop_ins.len(), 1);
            assert_eq!(svc.drop_ins[0].unit, "httpd.service");
            assert!(svc.drop_ins[0].content.contains("LimitNOFILE"));
        }
        _ => panic!("expected Services section"),
    }
}

// --- Degraded cases ---

#[test]
fn systemctl_missing_returns_degraded() {
    let exec = MockExecutor::new()
        .with_command("systemctl list-unit-files --type=service --no-pager",
            inspectah_core::traits::executor::ExecResult {
                stdout: String::new(),
                stderr: "command not found: systemctl".into(),
                exit_code: 127,
            });

    let source = pkg_source();
    let ctx = InspectionContext { source: &source, executor: &exec, rpm_state: None };
    let result = ServicesInspector.inspect(&ctx);

    match result {
        Err(InspectorError::Degraded { reason, .. }) => {
            assert!(reason.contains("systemctl"));
        }
        other => panic!("expected Degraded, got {:?}", other),
    }
}

#[test]
fn unreadable_preset_returns_degraded_not_ok() {
    // systemctl works, but preset directory is unreadable
    // Per spec: missing inputs materially reducing correctness → Degraded, not Ok+warnings
    let exec = MockExecutor::new()
        .with_command("systemctl list-unit-files --type=service --no-pager",
            inspectah_core::traits::executor::ExecResult {
                stdout: "UNIT FILE STATE PRESET\nhttpd.service enabled disabled\n\n1 unit files listed.\n".into(),
                stderr: String::new(), exit_code: 0,
            });
        // Note: no preset dirs registered → read_dir will fail

    let source = pkg_source();
    let ctx = InspectionContext { source: &source, executor: &exec, rpm_state: None };
    let result = ServicesInspector.inspect(&ctx);

    match result {
        Err(InspectorError::Degraded { partial, reason }) => {
            assert!(reason.contains("preset"));
            // Partial output should still contain the systemctl data
            match &partial.section {
                SectionData::Services(svc) => {
                    assert!(!svc.enabled_units.is_empty() || !svc.disabled_units.is_empty());
                }
                _ => panic!("partial should be Services"),
            }
        }
        other => panic!("expected Degraded, got {:?}", other),
    }
}

// --- Empty system ---

#[test]
fn empty_system_returns_empty_section() {
    let exec = MockExecutor::new()
        .with_command("systemctl list-unit-files --type=service --no-pager",
            inspectah_core::traits::executor::ExecResult {
                stdout: "UNIT FILE STATE PRESET\n\n0 unit files listed.\n".into(),
                stderr: String::new(), exit_code: 0,
            })
        .with_dir("/usr/lib/systemd/system-preset", vec![])
        .with_dir("/etc/systemd/system-preset", vec![])
        .with_dir("/etc/systemd/system", vec![]);

    let source = pkg_source();
    let ctx = InspectionContext { source: &source, executor: &exec, rpm_state: None };
    let result = ServicesInspector.inspect(&ctx).unwrap();

    match &result.section {
        SectionData::Services(svc) => {
            assert!(svc.state_changes.is_empty());
            assert!(svc.enabled_units.is_empty());
            assert!(svc.drop_ins.is_empty());
        }
        _ => panic!("expected Services section"),
    }
}

// --- Redaction ---

#[test]
fn dropin_with_secret_produces_redaction_hint() {
    // Drop-in content containing Environment=DB_PASSWORD=secret123
    let exec = MockExecutor::new()
        .with_command("systemctl list-unit-files --type=service --no-pager",
            inspectah_core::traits::executor::ExecResult {
                stdout: "UNIT FILE STATE PRESET\napp.service enabled disabled\n\n1 unit files listed.\n".into(),
                stderr: String::new(), exit_code: 0,
            })
        .with_dir("/usr/lib/systemd/system-preset", vec![])
        .with_dir("/etc/systemd/system-preset", vec![])
        .with_dir("/etc/systemd/system", vec!["app.service.d"])
        .with_dir("/etc/systemd/system/app.service.d", vec!["env.conf"])
        .with_file("/etc/systemd/system/app.service.d/env.conf",
            "[Service]\nEnvironment=DB_PASSWORD=secret123\n");

    let source = pkg_source();
    let ctx = InspectionContext { source: &source, executor: &exec, rpm_state: None };
    let result = ServicesInspector.inspect(&ctx).unwrap();

    // Inspector should produce redaction hints for the Environment line
    assert!(!result.redaction_hints.is_empty(),
        "drop-in with Environment=DB_PASSWORD should produce redaction hint");
}

// --- Snapshot ---

#[test]
fn services_snapshot() {
    // Same setup as happy_path
    let exec = /* same as happy_path_state_changes */;
    let source = pkg_source();
    let ctx = InspectionContext { source: &source, executor: &exec, rpm_state: None };
    let result = ServicesInspector.inspect(&ctx).unwrap();
    insta::assert_json_snapshot!("services_happy_path", result.section);
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test -p inspectah-collect services_test`
Expected: FAIL — `ServicesInspector` does not exist.

- [ ] **Step 5: Implement ServicesInspector**

Create `inspectah-collect/src/inspectors/services.rs`. Implementation follows the RPM inspector pattern with the **borrowed** `InspectionContext<'_>`:

```rust
pub struct ServicesInspector;

impl Inspector for ServicesInspector {
    fn id(&self) -> InspectorId { InspectorId::Services }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(&self, ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError> {
        // 1. Run systemctl — if fails, return Degraded with empty partial
        // 2. Read preset files — if unreadable, return Degraded with systemctl data as partial
        // 3. Compare state vs preset — build state_changes
        // 4. Scan drop-in directories — collect content
        // 5. For each drop-in, check for Environment= lines → produce RedactionHint
        // 6. Return Ok(InspectorOutput { section: SectionData::Services(...), warnings, redaction_hints })
    }
}
```

Key rules:
- Commands via `ctx.executor.run()` with fixed argv — `("systemctl", &["list-unit-files", "--type=service", "--no-pager"])`
- Preset files sorted by filename (numeric prefix ordering) and processed first-match-wins
- `*` and `?` glob matching for preset entries
- Template units (`@.service`) filtered out
- Static units excluded from state_changes
- Missing preset dirs → `Err(Degraded)`, not `Ok` with warnings
- Drop-in content with `Environment=` containing secret-like names → `RedactionHint`

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p inspectah-collect services_test`
Expected: All tests PASS.

- [ ] **Step 7: Clippy + accept insta snapshot**

Run: `cargo clippy -p inspectah-collect -- -W clippy::all`
Run: `cargo insta review`

- [ ] **Step 8: Commit**

```bash
git add inspectah-collect/src/inspectors/services.rs inspectah-collect/src/inspectors/mod.rs
git add inspectah-collect/tests/services_test.rs testdata/fixtures/services/
git commit -m "feat(collect): implement services inspector with TDD"
```

---

## Task 5: Storage Inspector

**Files:**
- Create: `inspectah-collect/src/inspectors/storage.rs`
- Create: `inspectah-collect/tests/storage_test.rs`
- Create: `testdata/fixtures/storage/findmnt.json`
- Create: `testdata/fixtures/storage/fstab`
- Create: `testdata/fixtures/storage/lvs.json`
- Modify: `inspectah-collect/src/inspectors/mod.rs`

### Tests must cover

1. **Happy path** — fstab parsed, findmnt JSON parsed, LVM detected, NFS entry flagged
2. **Credential detection** — mount option `credentials=/etc/cifs-creds` → `CredentialRef` + `RedactionHint`
3. **findmnt failure** → `Err(Degraded)` with fstab data as partial
4. **Malformed findmnt JSON** → `Err(Degraded)` with parse error in reason
5. **fstab unreadable** → `Err(Failed)` (fstab is the primary source)
6. **LVM not available** → proceeds without `lvm_info`
7. **Empty fstab** → empty `StorageSection`
8. **Applicability** → `&[PackageBased]`
9. **Insta snapshot**

### Artifact consumers (for smoke tests later)

Storage data appears in: `kickstart-suggestion.ks` (fstab-based storage plan), `audit-report.md` (storage findings), `report.html` (storage section). It does NOT drive Containerfile content directly.

Follow the same borrowed-context pattern as Task 4. See `testdata/fixtures/storage/` for fixture files (same content as the draft plan's fixtures — findmnt.json, fstab, lvs.json).

- [ ] **Steps 1-8: Same TDD cycle as Task 4** — fixtures, register module, write failing tests, implement, verify, clippy, snapshot, commit.

Commit message: `feat(collect): implement storage inspector with TDD`

---

## Task 6: Kernelboot Inspector

**Files:**
- Create: `inspectah-collect/src/inspectors/kernelboot.rs`
- Create: `inspectah-collect/tests/kernelboot_test.rs`
- Create: `testdata/fixtures/kernelboot/` (lsmod.txt, proc-cmdline.txt, sysctl-system.conf, sysctl-a.txt, dracut-conf, locale.conf, tuned-active.txt)
- Modify: `inspectah-collect/src/inspectors/mod.rs`

### Tests must cover

1. **Happy path** — cmdline, lsmod, sysctl overrides, locale, timezone, tuned profile
2. **Sysctl three-way diff** — file-defined value differs from runtime value
3. **lsmod failure** → `Err(Degraded)` with cmdline/sysctl as partial
4. **Partial failure** — dracut unreadable but everything else works → `Err(Degraded)` (spec: missing inputs reducing correctness = Degraded)
5. **tuned not installed** — `tuned-adm` fails → tuned_active = "", no error
6. **Config snippet content with secrets** — dracut/modprobe file containing password → `RedactionHint`
7. **Empty system** — no overrides, no custom modules
8. **Applicability** → `&[PackageBased]`
9. **Insta snapshot**

### Key implementation detail: degraded threshold

Per Slate's review: individual kernelboot step failures (e.g., dracut unreadable) that materially reduce section correctness must return `Err(Degraded)`, not `Ok` with warnings. The threshold: if any of the three primary sources (cmdline, lsmod, sysctl files) fail, the section is Degraded. Optional sources (tuned, alternatives) can fail without triggering Degraded.

### Artifact consumers

Kernelboot data appears in: `Containerfile` (sysctl overrides, module loading), `config/` tree (sysctl.d, modprobe.d, dracut.conf.d files), `audit-report.md` (kernel findings), `report.html`.

- [ ] **Steps 1-8: Same TDD cycle as Tasks 4-5** — fixtures, register, test, implement, verify, clippy, snapshot, commit.

Commit message: `feat(collect): implement kernelboot inspector with TDD`

---

## Task 7: Redaction Coverage for New Surfaces

**Files:**
- Modify: `inspectah-pipeline/src/redaction.rs`
- Create: `inspectah-pipeline/tests/redaction_new_surfaces_test.rs`

### New persisted content at risk (from Slate's review)

| Source | Risk | Handling |
|--------|------|----------|
| Systemd drop-in `Environment=` lines | Secrets in env vars | Detect `PASSWORD`, `SECRET`, `TOKEN`, `KEY`, `CREDENTIAL` in var names |
| Mount option `credentials=`, `password=` | CIFS/NFS credentials | Detect credential-pattern mount options |
| Kernel cmdline | May contain boot-time secrets | Scan for `password=`, `key=`, `secret=` substrings |
| Tuned/dracut/modprobe config content | Arbitrary config values | General credential pattern scan |

- [ ] **Step 1: Write planted-secret tests**

Create `inspectah-pipeline/tests/redaction_new_surfaces_test.rs`:

```rust
#[test]
fn test_dropin_env_secret_redacted() {
    // Build snapshot with services section containing drop-in with DB_PASSWORD=secret123
    // Run redaction engine
    // Assert: secret value is replaced, RedactionFinding is recorded
}

#[test]
fn test_mount_credential_path_flagged() {
    // Build snapshot with storage section containing credentials=/etc/cifs-creds
    // Run redaction engine
    // Assert: credential path is flagged in findings
}

#[test]
fn test_cmdline_password_redacted() {
    // Build snapshot with kernelboot section containing cmdline with password=hunter2
    // Run redaction engine
    // Assert: password value is redacted
}

#[test]
fn test_secrets_review_reports_all_findings() {
    // Build snapshot with all three planted secrets
    // Run full pipeline through redaction + rendering
    // Assert: secrets-review.md contains all three findings
}
```

- [ ] **Step 2: Extend redaction engine patterns**

Add detection patterns for:
- `Environment=<NAME>=<VALUE>` where NAME matches secret-like patterns
- Mount options `credentials=<path>`, `password=<value>`
- Kernel cmdline `password=<value>`, `key=<value>`

These extend the existing pattern registry in `inspectah-pipeline/src/redaction.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-pipeline redaction_new_surfaces`
Expected: All planted-secret tests pass.

- [ ] **Step 4: Commit**

```bash
git add inspectah-pipeline/src/redaction.rs inspectah-pipeline/tests/redaction_new_surfaces_test.rs
git commit -m "feat(pipeline): extend redaction engine for services/storage/kernelboot surfaces"
```

---

## Task 8: Parallel Execution

**Files:**
- Modify: `inspectah-pipeline/src/collect.rs`
- Create: `inspectah-pipeline/tests/parallel_test.rs`

The approved spec mandates the scoped-thread three-wave model. This is not optional. The borrowed `InspectionContext<'a>` from Task 2 enables it.

- [ ] **Step 1: Write parallel execution tests**

Create `inspectah-pipeline/tests/parallel_test.rs`:

```rust
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[test]
fn independent_inspectors_run_concurrently() {
    // Create 3 inspectors that each sleep 100ms (via MockExecutor with delay)
    // Run collect()
    // Assert total elapsed < 250ms (proves parallelism, not serialization)
    // Assert all 3 sections present in snapshot
    let start = Instant::now();
    // ... run collect with 3 slow mock inspectors ...
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_millis(250),
        "3 inspectors at 100ms each took {:?} — should be parallel", elapsed);
}

#[test]
fn rpm_state_flows_to_dependent_inspectors() {
    // Create mock RPM inspector that produces RpmState
    // Create mock dependent inspector that asserts ctx.rpm_state.is_some()
    // Run collect()
    // Assert dependent inspector received RpmState and succeeded
}

#[test]
fn rpm_failure_propagates_to_dependents() {
    // Create mock RPM inspector that returns Failed
    // Create mock dependent inspector
    // Run collect()
    // Assert dependent returns Failed with "RPM dependency unavailable"
}

#[test]
fn inspector_panic_contained() {
    // Create a mock inspector that panics
    // Create two normal mock inspectors
    // Run collect()
    // Assert: panicking inspector produces Failed, other two succeed
    // Assert: snapshot completeness is Incomplete
}

#[test]
fn orchestrator_skips_inapplicable_inspectors() {
    // Create inspector with applicable_to() = [PackageBased]
    // Run on a Bootc source system
    // Assert: inspector.inspect() is never called
    // Assert: snapshot has Skipped entry for that section
}
```

- [ ] **Step 2: Refactor collect() to scoped-thread model**

Modify `inspectah-pipeline/src/collect.rs`:

```rust
pub fn collect<'a>(
    source: &'a SourceSystem,
    executor: &'a dyn Executor,
    inspectors: &[Box<dyn Inspector>],
) -> Pipeline<Collected> {
    // 1. Partition inspectors: find RPM inspector, separate independent from dependent
    //    (dependent = those whose applicable_to includes PackageBased AND need rpm_state)
    //    For Slice 2a, all non-RPM inspectors are independent.

    // 2. Build base context (no rpm_state)
    let base_ctx = InspectionContext { source, executor, rpm_state: None };

    // 3. Applicability check: for each inspector, if source.kind() not in applicable_to(),
    //    record Skipped and don't spawn it

    // 4. std::thread::scope for Wave 1:
    //    - Spawn RPM inspector
    //    - Spawn all applicable independent inspectors
    //    - Join RPM handle → build RpmState
    //    - Build enriched context with &rpm_state
    //    - Spawn all applicable dependent inspectors
    //    - Join all remaining handles

    // 5. For each thread result:
    //    - Ok(join) → route_section() as before
    //    - Err(panic) → record Failed with "inspector panicked: {msg}"

    // 6. Build Completeness from collected results:
    //    - All Ok → Complete
    //    - Any Degraded, no Failed → Partial { degraded_sections }
    //    - Any Failed → Incomplete { failed_sections, degraded_sections }
}
```

The key: `InspectionContext<'a>` borrows `source` and `executor` from the calling scope. Within `thread::scope`, both the `base_ctx` (no rpm_state) and `enriched_ctx` (with rpm_state) borrow the same `source` and `executor`. No `Arc`, no cloning.

- [ ] **Step 3: Update orchestrate.rs**

Update `run_pipeline()` to pass separated `source`, `executor`, and inspector list to the new `collect()` signature.

- [ ] **Step 4: Run full test suite**

Run: `cargo test --workspace`
Expected: All existing tests pass. New parallel tests pass.

- [ ] **Step 5: Commit**

```bash
git add inspectah-pipeline/src/collect.rs inspectah-pipeline/src/orchestrate.rs
git add inspectah-pipeline/tests/parallel_test.rs
git commit -m "feat(pipeline): implement scoped-thread three-wave parallel execution"
```

---

## Task 9: Parity Gate Expansion

**Files:**
- Create: `testdata/golden/go-v13-services-section.json`
- Create: `testdata/golden/go-v13-storage-section.json`
- Create: `testdata/golden/go-v13-kernelboot-section.json`
- Modify: `inspectah-core/tests/parity_gate.rs`
- Modify: `testdata/divergences.md`

### Golden file generation

Golden files are generated by running Go inspectah on a real RHEL system:

```bash
inspectah scan --output /tmp/go-scan
jq '.services' /tmp/go-scan/inspection-snapshot.json > go-v13-services-section.json
jq '.storage' /tmp/go-scan/inspection-snapshot.json > go-v13-storage-section.json
jq '.kernel_boot' /tmp/go-scan/inspection-snapshot.json > go-v13-kernelboot-section.json
```

**Provisional goldens from Go test fixtures are acceptable for CI during development, but do NOT satisfy slice-closure evidence.** Before slice sign-off (Task 12), provisional goldens must be replaced with real scan output and the host-validation evidence must reference the same host.

- [ ] **Step 1: Generate golden files** (provisional or real)

- [ ] **Step 2: Expand parity gate tests**

Add per-section parity tests to `inspectah-core/tests/parity_gate.rs` for services, storage, kernelboot. Each test loads the Go golden JSON, runs the corresponding Rust inspector on the same fixture data, and diffs the output through the divergence allowlist.

- [ ] **Step 3: Document any divergences**

Add entries to `testdata/divergences.md` using the governed format (Go output, Rust output, reason, disposition, approval status).

- [ ] **Step 4: Commit**

```bash
git add testdata/golden/ inspectah-core/tests/parity_gate.rs testdata/divergences.md
git commit -m "test(parity): expand parity gate to services, storage, kernelboot sections"
```

---

## Task 10: Renderer Smoke Tests

**Files:**
- Create: `inspectah-pipeline/tests/smoke_render.rs`

### Correct artifact consumers per inspector (from Thorn's review)

| Section | Containerfile | config/ tree | audit-report.md | kickstart-suggestion.ks | report.html | secrets-review.md |
|---------|-------------|-------------|----------------|----------------------|------------|------------------|
| services | systemctl enable/disable | drop-in files | state changes | — | services tab | drop-in content scan |
| storage | — | — | fstab/LVM findings | fstab-based storage plan | storage tab | credential refs |
| kernelboot | sysctl, modules-load | sysctl.d, modprobe.d, dracut.conf.d | kernel findings | — | kernelboot tab | config snippet content |

- [ ] **Step 1: Write smoke tests targeting correct consumers**

```rust
#[test]
fn services_in_containerfile() {
    // Build snapshot with services section (httpd enabled)
    // Render Containerfile
    // Assert: contains systemctl enable/disable commands
}

#[test]
fn services_dropins_in_config_tree() {
    // Build snapshot with services section containing drop-ins
    // Render config tree
    // Assert: drop-in files materialized in output dir
}

#[test]
fn storage_in_kickstart() {
    // Build snapshot with storage section (fstab entries)
    // Render kickstart-suggestion.ks
    // Assert: contains storage-related kickstart directives
}

#[test]
fn storage_not_in_containerfile() {
    // Verify storage does NOT inject content into Containerfile
    // (storage is handled via kickstart, not Containerfile)
}

#[test]
fn kernelboot_sysctl_in_containerfile() {
    // Build snapshot with kernelboot section (sysctl overrides)
    // Render Containerfile
    // Assert: contains sysctl-related COPY/RUN commands
}

#[test]
fn kernelboot_configs_in_config_tree() {
    // Build snapshot with kernelboot section
    // Render config tree
    // Assert: sysctl.d, modprobe.d, dracut.conf.d files materialized
}

#[test]
fn all_sections_in_audit_report() {
    // Build snapshot with all 3 sections
    // Render audit-report.md
    // Assert: services, storage, kernelboot headings all present
}

#[test]
fn credential_refs_in_secrets_review() {
    // Build snapshot with storage credential refs + services drop-in secrets
    // Run through redaction + render secrets-review.md
    // Assert: findings reported
}
```

- [ ] **Step 2: Run smoke tests**

Run: `cargo test -p inspectah-pipeline smoke_render`
Expected: All pass.

- [ ] **Step 3: Commit**

```bash
git add inspectah-pipeline/tests/smoke_render.rs
git commit -m "test(pipeline): add renderer smoke tests targeting correct artifact consumers"
```

---

## Task 11: Failure & Trust Policy

**Files:**
- Create: `inspectah-pipeline/tests/failure_policy.rs`
- Modify: renderers as needed for degraded/failed handling

### Tests must cover (from Thorn, Collins, Slate reviews)

```rust
// --- Degraded vs Failed distinction ---

#[test]
fn degraded_section_contributes_to_containerfile_with_fixme() {
    // Snapshot with services Degraded (partial data, missing presets)
    // Render Containerfile
    // Assert: services content present WITH "# FIXME:" comment noting degradation
}

#[test]
fn failed_section_excluded_from_containerfile() {
    // Snapshot with services Failed
    // Completeness = Incomplete { failed_sections: [Services] }
    // Render Containerfile
    // Assert: NO services-related content
}

#[test]
fn failed_section_appears_in_audit_with_explanation() {
    // Snapshot with services Failed
    // Render audit-report.md
    // Assert: failure entry with reason string
}

#[test]
fn degraded_section_scanned_by_secrets_review() {
    // Snapshot with degraded services (partial data with drop-in containing secret)
    // Run redaction + render secrets-review.md
    // Assert: secret finding reported despite degradation
}

#[test]
fn failed_section_excluded_from_secrets_review() {
    // Snapshot with failed services
    // Render secrets-review.md
    // Assert: no services content in review
}

// --- Completeness aggregation ---

#[test]
fn all_success_produces_complete() {
    // All inspectors return Ok
    // Assert: snapshot.completeness == Complete
}

#[test]
fn one_degraded_produces_partial() {
    // One inspector returns Degraded, rest Ok
    // Assert: snapshot.completeness == Partial { degraded_sections: [that inspector] }
}

#[test]
fn one_failed_produces_incomplete() {
    // One inspector returns Failed
    // Assert: snapshot.completeness == Incomplete { failed_sections: [that inspector] }
}

// --- Panic containment ---

#[test]
fn panicking_inspector_produces_failed_status() {
    // Inspector that panics inside thread::scope
    // Assert: caught at join boundary, recorded as Failed
    // Assert: other inspectors unaffected
    // Assert: completeness = Incomplete
}

// --- Dependency failure propagation ---

#[test]
fn rpm_failed_causes_dependent_inspector_failed() {
    // RPM inspector returns Failed
    // Dependent inspector (will be tested fully in Slice 2c)
    // For Slice 2a: verify the orchestrator correctly withholds RpmState
    // and the dependent would receive None
}

// --- Redaction state ---

#[test]
fn redaction_state_set_after_redaction_engine_runs() {
    // Build snapshot with redaction-bearing content
    // Run pipeline
    // Assert: snapshot.redaction_state is FullyRedacted or PartiallyRedacted, not Raw
}

#[test]
fn redaction_state_reports_unresolved_hints() {
    // Build snapshot with content that produces redaction hints
    // but redaction engine can't fully resolve them
    // Assert: redaction_state is PartiallyRedacted with unresolved_count > 0
}
```

- [ ] **Step 1: Write all failure policy tests**
- [ ] **Step 2: Implement renderer changes for degraded/failed handling**

Modify renderers to check completeness and individual section status:
- Containerfile renderer: emit FIXME comments for degraded sections, skip failed sections
- Audit renderer: emit failure entries for failed sections, warning banners for degraded
- Secrets renderer: scan degraded sections, skip failed sections
- All renderers: respect the emission policy matrix from the spec

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-pipeline failure_policy`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add inspectah-pipeline/tests/failure_policy.rs inspectah-pipeline/src/render/
git commit -m "feat(pipeline): implement and test failure/trust policy for all artifact surfaces"
```

---

## Task 12: Wire Inspectors into Pipeline & Host Validation

**Files:**
- Modify: `inspectah-pipeline/src/orchestrate.rs` or `inspectah-cli/src/commands/scan.rs`
- Create: `testdata/evidence/slice-2a-host-validation.md` (filled with real data)

### Part A: Wire inspectors

- [ ] **Step 1: Register new inspectors in the pipeline**

Add `ServicesInspector`, `StorageInspector`, `KernelbootInspector` to the inspector list in the scan command or orchestrator.

- [ ] **Step 2: Run full workspace tests**

Run: `cargo test --workspace`
Expected: All tests pass.

- [ ] **Step 3: Commit wiring**

```bash
git add inspectah-pipeline/src/orchestrate.rs inspectah-cli/
git commit -m "feat(pipeline): wire services, storage, kernelboot into scan pipeline"
```

### Part B: Host validation (BLOCKING for slice closure)

This step produces the durable evidence artifact required by the spec. It is NOT a template — it is filled with real data from a real host.

- [ ] **Step 4: Run Go and Rust on same RHEL/CentOS host**

On a package-mode RHEL/CentOS/Fedora system:
```bash
# Go scan
inspectah scan --output /tmp/go-scan

# Rust scan
cargo run -p inspectah-cli -- scan --output /tmp/rust-scan
```

- [ ] **Step 5: Generate and compare golden files**

```bash
# Extract sections
jq '.services' /tmp/go-scan/inspection-snapshot.json > /tmp/go-services.json
jq '.storage' /tmp/go-scan/inspection-snapshot.json > /tmp/go-storage.json
jq '.kernel_boot' /tmp/go-scan/inspection-snapshot.json > /tmp/go-kernelboot.json

# Compare
diff /tmp/go-services.json testdata/golden/go-v13-services-section.json
# Replace provisional goldens with real scan output
cp /tmp/go-services.json testdata/golden/go-v13-services-section.json
cp /tmp/go-storage.json testdata/golden/go-v13-storage-section.json
cp /tmp/go-kernelboot.json testdata/golden/go-v13-kernelboot-section.json
```

- [ ] **Step 6: Fill evidence artifact with real data**

Create `testdata/evidence/slice-2a-host-validation.md` with actual values:

```markdown
# Slice 2a Host Validation Evidence

**Date:** [actual date]
**Validated by:** [actual name]

## Host Details
- **OS:** [actual, e.g., CentOS Stream 9]
- **Kernel:** [actual]
- **Architecture:** [actual]
- **Go inspectah version:** [actual]
- **Rust inspectah version:** 0.8.0-alpha.1
- **Rust toolchain:** [actual]

## Command Validation
[actual pass/fail for each command]

## Section Parity
[actual comparison results]

## Rendered Artifact Spot-Check
[actual verification that new sections appear in artifacts]

## Conclusion
[actual assessment]
```

- [ ] **Step 7: Run parity gate with real golden files**

Run: `cargo test -p inspectah-core parity`
Expected: All parity tests pass with real golden files.

- [ ] **Step 8: Commit evidence and real golden files**

```bash
git add testdata/golden/ testdata/evidence/slice-2a-host-validation.md
git commit -m "evidence: slice 2a host validation with real Go v13 golden files"
```

---

## Task 13: Final Verification

- [ ] **Step 1: Full test suite**

Run: `cargo test --workspace 2>&1 | grep 'test result'`
Record total test count. Target: Phase 1 baseline (~216) + Slice 2a additions.

- [ ] **Step 2: Clippy clean**

Run: `cargo clippy --workspace -- -W clippy::all`
Expected: Zero warnings.

- [ ] **Step 3: Format check**

Run: `cargo fmt --all -- --check`
Expected: No issues.

- [ ] **Step 4: Verify slice checklist**

- [ ] Services, storage, kernelboot inspectors implemented with borrowed `InspectionContext<'_>`
- [ ] All inspectors declare `applicable_to() -> &[PackageBased]`
- [ ] Three-wave parallel execution working (even if Slice 2a has no dependent inspectors yet)
- [ ] CI running Tier 1 + Tier 2 with full trigger coverage
- [ ] Section parity gate passing for RPM + 3 new sections with real golden files
- [ ] Renderer smoke tests passing for all 3 sections against correct artifact consumers
- [ ] Failure policy tested: degraded/failed/panic/dependency/redaction/trust
- [ ] Redaction engine extended for new persisted surfaces with planted-secret proofs
- [ ] Collector boundary enforced: LC_ALL=C, timeout, size caps
- [ ] Completeness distinguishes Partial (degraded) from Incomplete (failed)
- [ ] Host validation evidence committed with real data
- [ ] No provisional golden files remaining
- [ ] All divergence allowlist entries have review-approval annotations
- [ ] All commits follow conventional commit format

- [ ] **Step 5: Review commit history**

Run: `git log --oneline` (Slice 2a commits)
Verify focused, well-described commits.
