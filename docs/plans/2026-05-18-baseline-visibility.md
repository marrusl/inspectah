# Baseline Visibility Implementation Plan (revision 2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Design spec:** `docs/specs/proposed/2026-05-18-baseline-visibility-design.md` (revision 4, approved)

**Revision history:**
- **Revision 1:** Five reviewers requested changes. Two must-fix blockers: (1) Tasks 5-7 reopened the rejected live-progress contract by allowing accumulate-then-replay and interim no-op callbacks; (2) Tasks 1-4 miswired comparison truth using `snap.rpm.is_some()` and `baseline.packages.len()`. Full rewrite required.
- **Revision 2:** (this document) Rewritten from scratch. All snippets verified against current source.

---

## Task 1: Shared Presentation Helpers — `baseline_fmt` module

**Files:**
- Create: `inspectah-pipeline/src/render/baseline_fmt.rs`
- Modify: `inspectah-pipeline/src/render/mod.rs` (register module)

This is a new module. Register it in `mod.rs` **first** so `cargo test` can find the file.

- [ ] **Step 1: Register the module**

In `inspectah-pipeline/src/render/mod.rs`, add after the existing `pub mod` declarations (after line 22, which is `pub mod tarball;`):

```rust
pub mod baseline_fmt;
```

Create `inspectah-pipeline/src/render/baseline_fmt.rs` with:

```rust
//! Shared presentation helpers for baseline metadata.
//!
//! Used by CLI, README, and audit renderers. All functions are pure —
//! they take typed inputs and return formatted strings.

use inspectah_core::baseline::{BaselineData, ResolutionStrategy, TargetImageIdentity};
use inspectah_core::types::rpm::{VersionChange, VersionChangeDirection};
```

Run: `cargo build -p inspectah-pipeline`
Expected: compiles (empty module with imports).

- [ ] **Step 2: Write failing tests for `strategy_label`**

Add to `baseline_fmt.rs`:

```rust
/// Human-readable label for a resolution strategy.
pub fn strategy_label(strategy: &ResolutionStrategy) -> &'static str {
    match strategy {
        ResolutionStrategy::CliOverride => "--base-image (user-specified)",
        ResolutionStrategy::UniversalBlue => "ublue image-info.json",
        ResolutionStrategy::BootcStatus => "bootc status (booted deployment)",
        ResolutionStrategy::FedoraAtomicDesktop => "fedora-atomic-desktop image-info.json",
        ResolutionStrategy::OsRelease => "os-release (auto-detected)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_label_all_variants() {
        assert_eq!(
            strategy_label(&ResolutionStrategy::CliOverride),
            "--base-image (user-specified)"
        );
        assert_eq!(
            strategy_label(&ResolutionStrategy::UniversalBlue),
            "ublue image-info.json"
        );
        assert_eq!(
            strategy_label(&ResolutionStrategy::BootcStatus),
            "bootc status (booted deployment)"
        );
        assert_eq!(
            strategy_label(&ResolutionStrategy::FedoraAtomicDesktop),
            "fedora-atomic-desktop image-info.json"
        );
        assert_eq!(
            strategy_label(&ResolutionStrategy::OsRelease),
            "os-release (auto-detected)"
        );
    }
}
```

Run: `cargo test -p inspectah-pipeline strategy_label -- --nocapture`
Expected: PASS.

- [ ] **Step 3: Write failing tests for `version_comparison_summary`**

Add before the tests module:

```rust
/// Summarize version comparison results.
///
/// Takes `Option<&[VersionChange]>` to distinguish three states:
/// - `None` — comparison data unavailable (RPM section absent or degraded)
/// - `Some(&[])` — comparison ran, zero differences found
/// - `Some(&[...])` — comparison ran, differences found
///
/// `shared_count` is the number of packages present in both host and baseline.
/// This is NOT `baseline.packages.len()` (which includes baseline-only packages).
pub fn version_comparison_summary(
    version_changes: Option<&[VersionChange]>,
    shared_count: usize,
) -> String {
    match version_changes {
        None => "comparison data unavailable".to_string(),
        Some(vcs) if vcs.is_empty() => {
            format!("all {shared_count} shared packages at same version")
        }
        Some(vcs) => {
            let upgrades = vcs
                .iter()
                .filter(|vc| vc.direction == VersionChangeDirection::Upgrade)
                .count();
            let downgrades = vcs.len() - upgrades;
            let detail = match (upgrades, downgrades) {
                (_, 0) => "all target-newer".to_string(),
                (0, _) => "all host-newer".to_string(),
                (u, d) => format!("{u} target-newer, {d} host-newer"),
            };
            format!(
                "{} shared packages with version changes ({})",
                vcs.len(),
                detail
            )
        }
    }
}
```

Add tests:

```rust
    fn make_vc(direction: VersionChangeDirection) -> VersionChange {
        VersionChange {
            name: "test-pkg".into(),
            direction,
            ..Default::default()
        }
    }

    #[test]
    fn version_comparison_unavailable() {
        assert_eq!(
            version_comparison_summary(None, 447),
            "comparison data unavailable"
        );
    }

    #[test]
    fn version_comparison_zero_changes() {
        assert_eq!(
            version_comparison_summary(Some(&[]), 447),
            "all 447 shared packages at same version"
        );
    }

    #[test]
    fn version_comparison_all_upgrades() {
        let vcs = vec![
            make_vc(VersionChangeDirection::Upgrade),
            make_vc(VersionChangeDirection::Upgrade),
        ];
        let s = version_comparison_summary(Some(&vcs), 447);
        assert!(s.contains("2 shared packages with version changes"));
        assert!(s.contains("all target-newer"));
    }

    #[test]
    fn version_comparison_all_downgrades() {
        let vcs = vec![make_vc(VersionChangeDirection::Downgrade)];
        let s = version_comparison_summary(Some(&vcs), 447);
        assert!(s.contains("1 shared packages with version changes"));
        assert!(s.contains("all host-newer"));
    }

    #[test]
    fn version_comparison_mixed() {
        let vcs = vec![
            make_vc(VersionChangeDirection::Upgrade),
            make_vc(VersionChangeDirection::Downgrade),
        ];
        let s = version_comparison_summary(Some(&vcs), 447);
        assert!(s.contains("2 shared packages with version changes"));
        assert!(s.contains("1 target-newer, 1 host-newer"));
    }
```

Run: `cargo test -p inspectah-pipeline version_comparison -- --nocapture`
Expected: PASS.

- [ ] **Step 4: Write failing tests for section builders**

Add before the tests module:

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::completeness::{Completeness, InspectorId};

/// Compute shared package count: baseline packages present on both host and baseline.
///
/// shared = total baseline packages - baseline-only packages.
/// Returns `None` when inputs don't allow a meaningful count.
pub fn shared_package_count(baseline: &BaselineData, rpm: &inspectah_core::types::rpm::RpmSection) -> usize {
    let total = baseline.packages.len();
    let baseline_only = rpm.base_image_only.len();
    total.saturating_sub(baseline_only)
}

/// Check whether RPM comparison data is trustworthy.
///
/// Returns `false` if the RPM inspector is degraded, failed, or if the RPM
/// section is absent. Uses the same completeness-check pattern as
/// `containerfile.rs:is_degraded`.
pub fn is_rpm_comparison_available(snap: &InspectionSnapshot) -> bool {
    if snap.rpm.is_none() {
        return false;
    }
    match &snap.completeness {
        Completeness::Partial { degraded_sections, .. } => {
            !degraded_sections.contains(&InspectorId::Rpm)
        }
        Completeness::Incomplete { failed_sections, degraded_sections, .. } => {
            !failed_sections.contains(&InspectorId::Rpm)
                && !degraded_sections.contains(&InspectorId::Rpm)
        }
        Completeness::Complete => true,
    }
}

/// Build version changes Option for the comparison summary.
///
/// Returns:
/// - `None` when RPM data is absent, degraded, or failed
/// - `Some(&[])` when comparison ran and found zero differences
/// - `Some(&vcs)` when differences exist
pub fn version_changes_for_display(snap: &InspectionSnapshot) -> Option<&[VersionChange]> {
    if !is_rpm_comparison_available(snap) {
        return None;
    }
    snap.rpm.as_ref().map(|r| r.version_changes.as_slice())
}

/// Build the baseline section lines for README and audit.
///
/// Returns an empty vec when `target_image` is absent (unknown state).
pub fn baseline_section_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let ti = match &snap.target_image {
        Some(ti) => ti,
        None => return vec![],
    };

    let mut lines = vec![
        "## Baseline comparison".into(),
        String::new(),
        "| | |".into(),
        "|---|---|".into(),
        format!("| Target image | {} |", ti.image_ref),
        format!("| Resolution | {} |", strategy_label(&ti.strategy)),
    ];

    match &snap.baseline {
        Some(bl) => {
            lines.push(format!("| Image digest | {} |", bl.image_digest));
            lines.push(format!("| Baseline extracted | {} |", bl.extracted_at));
            lines.push(format!("| Baseline packages | {} |", bl.packages.len()));

            let vc_display = version_changes_for_display(snap);
            let shared_count = match (snap.rpm.as_ref(), &snap.baseline) {
                (Some(rpm), Some(bl)) if is_rpm_comparison_available(snap) => {
                    shared_package_count(bl, rpm)
                }
                _ => 0,
            };
            lines.push(format!(
                "| Version changes | {} |",
                version_comparison_summary(vc_display, shared_count)
            ));
        }
        None => {
            if snap.no_baseline {
                lines.push("| Baseline | skipped (--no-baseline) |".into());
            } else {
                lines.push("| Baseline | unavailable |".into());
            }
        }
    }

    lines.push(String::new());
    lines
}
```

Add tests:

```rust
    use std::collections::HashMap;
    use inspectah_core::baseline::BaselinePackageEntry;
    use inspectah_core::types::rpm::RpmSection;

    fn test_target_image() -> TargetImageIdentity {
        TargetImageIdentity {
            image_ref: "quay.io/centos-bootc/centos-bootc:stream9".into(),
            strategy: ResolutionStrategy::OsRelease,
        }
    }

    fn test_baseline() -> BaselineData {
        let mut packages = HashMap::new();
        for i in 0..447 {
            packages.insert(
                format!("pkg-{i}"),
                BaselinePackageEntry {
                    name: format!("pkg-{i}"),
                    epoch: Some("0".into()),
                    version: "1.0".into(),
                    release: "1.el9".into(),
                    arch: "x86_64".into(),
                },
            );
        }
        BaselineData {
            image_digest: "sha256:abc123def456".into(),
            packages,
            extracted_at: "2026-05-18T14:32:00Z".into(),
        }
    }

    #[test]
    fn section_lines_full_state() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(test_target_image());
        snap.baseline = Some(test_baseline());
        snap.rpm = Some(RpmSection {
            version_changes: vec![make_vc(VersionChangeDirection::Upgrade)],
            ..Default::default()
        });
        let lines = baseline_section_lines(&snap);
        assert!(lines.iter().any(|l| l.contains("Baseline comparison")));
        assert!(lines.iter().any(|l| l.contains("centos-bootc:stream9")));
        assert!(lines.iter().any(|l| l.contains("os-release (auto-detected)")));
        assert!(lines.iter().any(|l| l.contains("sha256:abc123def456")));
        assert!(lines.iter().any(|l| l.contains("447")));
        assert!(lines.iter().any(|l| l.contains("1 shared packages with version changes")));
    }

    #[test]
    fn section_lines_degraded_no_baseline() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(test_target_image());
        snap.baseline = None;
        snap.no_baseline = false;
        let lines = baseline_section_lines(&snap);
        assert!(lines.iter().any(|l| l.contains("unavailable")));
    }

    #[test]
    fn section_lines_skipped_no_baseline() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(test_target_image());
        snap.baseline = None;
        snap.no_baseline = true;
        let lines = baseline_section_lines(&snap);
        assert!(lines.iter().any(|l| l.contains("skipped (--no-baseline)")));
    }

    #[test]
    fn section_lines_unknown_state() {
        let snap = InspectionSnapshot::new();
        let lines = baseline_section_lines(&snap);
        assert!(lines.is_empty());
    }

    #[test]
    fn section_lines_comparison_unavailable_rpm_degraded() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(test_target_image());
        snap.baseline = Some(test_baseline());
        snap.rpm = Some(RpmSection::default());
        snap.completeness = Completeness::Partial {
            degraded_sections: vec![InspectorId::Rpm],
            reason: "rpm inspector degraded".into(),
        };
        let lines = baseline_section_lines(&snap);
        assert!(lines.iter().any(|l| l.contains("comparison data unavailable")));
    }

    #[test]
    fn section_lines_comparison_unavailable_rpm_absent() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(test_target_image());
        snap.baseline = Some(test_baseline());
        snap.rpm = None;
        let lines = baseline_section_lines(&snap);
        assert!(lines.iter().any(|l| l.contains("comparison data unavailable")));
    }

    #[test]
    fn shared_package_count_excludes_baseline_only() {
        let bl = test_baseline(); // 447 packages
        let rpm = RpmSection {
            base_image_only: vec![
                inspectah_core::types::rpm::PackageEntry {
                    name: "baseline-only-pkg".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(shared_package_count(&bl, &rpm), 446);
    }

    #[test]
    fn is_rpm_comparison_available_complete() {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection::default());
        snap.completeness = Completeness::Complete;
        assert!(is_rpm_comparison_available(&snap));
    }

    #[test]
    fn is_rpm_comparison_available_degraded() {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection::default());
        snap.completeness = Completeness::Partial {
            degraded_sections: vec![InspectorId::Rpm],
            reason: "degraded".into(),
        };
        assert!(!is_rpm_comparison_available(&snap));
    }

    #[test]
    fn is_rpm_comparison_available_absent() {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = None;
        assert!(!is_rpm_comparison_available(&snap));
    }
```

Run: `cargo test -p inspectah-pipeline baseline_fmt -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Verify full crate builds and clippy**

Run: `cargo build -p inspectah-pipeline && cargo clippy -p inspectah-pipeline -- -W clippy::all && cargo fmt -p inspectah-pipeline --check`
Expected: clean build, zero clippy warnings, format passes.

- [ ] **Step 6: Commit**

```bash
git add inspectah-pipeline/src/render/baseline_fmt.rs inspectah-pipeline/src/render/mod.rs
git commit -m "feat(render): add baseline_fmt shared presentation helpers

Strategy labels, version comparison summary with Option<&[VersionChange]>
to distinguish zero-changes from data-unavailable, shared package count
excluding baseline-only entries, RPM comparison availability check using
completeness state, and section line builders for all four baseline states.

Assisted-by: Claude Code (Opus)"
```

---

## Task 2: README Baseline Section

**Files:**
- Modify: `inspectah-pipeline/src/render/readme.rs`

- [ ] **Step 1: Write failing tests for README baseline section**

Add to `readme.rs` `#[cfg(test)] mod tests`:

```rust
    use inspectah_core::baseline::{BaselineData, BaselinePackageEntry, TargetImageIdentity, ResolutionStrategy};
    use inspectah_core::types::rpm::{RpmSection, VersionChange, VersionChangeDirection};
    use std::collections::HashMap;

    fn test_target_image() -> TargetImageIdentity {
        TargetImageIdentity {
            image_ref: "quay.io/centos-bootc/centos-bootc:stream9".into(),
            strategy: ResolutionStrategy::OsRelease,
        }
    }

    fn test_baseline() -> BaselineData {
        BaselineData {
            image_digest: "sha256:abc123def456".into(),
            packages: HashMap::new(),
            extracted_at: "2026-05-18T14:32:00Z".into(),
        }
    }

    #[test]
    fn readme_includes_baseline_section_full() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(test_target_image());
        snap.baseline = Some(test_baseline());
        snap.rpm = Some(RpmSection {
            version_changes: vec![VersionChange {
                name: "glibc".into(),
                direction: VersionChangeDirection::Upgrade,
                ..Default::default()
            }],
            ..Default::default()
        });
        let md = render_readme(&snap);
        assert!(md.contains("## Baseline comparison"), "must have baseline section");
        assert!(md.contains("centos-bootc:stream9"));
        assert!(md.contains("os-release (auto-detected)"));
        assert!(md.contains("sha256:abc123def456"));
    }

    #[test]
    fn readme_baseline_section_degraded() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(test_target_image());
        snap.baseline = None;
        snap.no_baseline = false;
        let md = render_readme(&snap);
        assert!(md.contains("## Baseline comparison"));
        assert!(md.contains("unavailable"));
    }

    #[test]
    fn readme_baseline_section_skipped() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(test_target_image());
        snap.baseline = None;
        snap.no_baseline = true;
        let md = render_readme(&snap);
        assert!(md.contains("skipped (--no-baseline)"));
    }

    #[test]
    fn readme_baseline_section_absent_when_no_target() {
        let snap = InspectionSnapshot::new();
        let md = render_readme(&snap);
        assert!(!md.contains("Baseline comparison"));
    }
```

Run: `cargo test -p inspectah-pipeline readme_baseline -- --nocapture`
Expected: FAIL (no baseline section in readme yet).

- [ ] **Step 2: Implement README baseline section**

In `readme.rs`, add the import at the top:

```rust
use super::baseline_fmt;
```

Replace line 227 (`let _ = base_image_from_snapshot(snap); // retained for future FROM reference`) with:

```rust
    // Baseline comparison section
    let baseline_lines = baseline_fmt::baseline_section_lines(snap);
    if !baseline_lines.is_empty() {
        lines.extend(baseline_lines);
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-pipeline readme -- --nocapture`
Expected: all PASS (including new baseline tests and existing tests).

- [ ] **Step 4: Commit**

```bash
git add inspectah-pipeline/src/render/readme.rs
git commit -m "feat(readme): add baseline comparison section

Uses shared baseline_fmt helpers. Replaces the unused
base_image_from_snapshot stub. Shows target image, strategy, digest,
extraction time, package count, and version comparison summary.
Degraded and skipped states render reduced sections.

Assisted-by: Claude Code (Opus)"
```

---

## Task 3: Audit Report Baseline Section + Parity Test

**Files:**
- Modify: `inspectah-pipeline/src/render/audit.rs`

- [ ] **Step 1: Write failing tests for audit baseline section**

Add to `audit.rs` `#[cfg(test)] mod tests`:

```rust
    use inspectah_core::baseline::{BaselineData, TargetImageIdentity, ResolutionStrategy};
    use inspectah_core::types::rpm::{RpmSection, VersionChange, VersionChangeDirection};
    use std::collections::HashMap;

    fn test_target_image() -> TargetImageIdentity {
        TargetImageIdentity {
            image_ref: "quay.io/centos-bootc/centos-bootc:stream9".into(),
            strategy: ResolutionStrategy::OsRelease,
        }
    }

    fn test_baseline() -> BaselineData {
        BaselineData {
            image_digest: "sha256:abc123def456".into(),
            packages: HashMap::new(),
            extracted_at: "2026-05-18T14:32:00Z".into(),
        }
    }

    #[test]
    fn audit_includes_baseline_section() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(test_target_image());
        snap.baseline = Some(test_baseline());
        snap.rpm = Some(RpmSection {
            version_changes: vec![VersionChange {
                name: "glibc".into(),
                direction: VersionChangeDirection::Upgrade,
                ..Default::default()
            }],
            ..Default::default()
        });
        let md = render_audit(&snap);
        assert!(md.contains("## Baseline comparison"), "audit must have baseline section");
        assert!(md.contains("centos-bootc:stream9"));
        assert!(md.contains("os-release (auto-detected)"));
    }

    #[test]
    fn audit_baseline_absent_when_no_target() {
        let snap = InspectionSnapshot::new();
        let md = render_audit(&snap);
        assert!(!md.contains("Baseline comparison"));
    }
```

Run: `cargo test -p inspectah-pipeline audit_baseline -- --nocapture`
Expected: FAIL (no baseline section in audit yet).

- [ ] **Step 2: Implement audit baseline section**

In `audit.rs`, add the import:

```rust
use super::baseline_fmt;
```

After the completeness warning block (after the `if !failed_ids.is_empty() || !degraded_ids.is_empty()` block), add:

```rust
    // Baseline comparison section
    let baseline_lines = baseline_fmt::baseline_section_lines(snap);
    if !baseline_lines.is_empty() {
        lines.extend(baseline_lines);
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-pipeline audit -- --nocapture`
Expected: all PASS.

- [ ] **Step 4: Write parity test**

Add to `inspectah-pipeline/src/render/baseline_fmt.rs` tests:

```rust
    #[test]
    fn readme_audit_parity() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(test_target_image());
        snap.baseline = Some(test_baseline());
        snap.rpm = Some(RpmSection {
            version_changes: vec![make_vc(VersionChangeDirection::Upgrade)],
            ..Default::default()
        });
        let readme = crate::render::readme::render_readme(&snap);
        let audit = crate::render::audit::render_audit(&snap);

        // Both must contain the same baseline metadata
        assert!(readme.contains("centos-bootc:stream9"));
        assert!(audit.contains("centos-bootc:stream9"));
        assert!(readme.contains("os-release (auto-detected)"));
        assert!(audit.contains("os-release (auto-detected)"));
        assert!(readme.contains("sha256:abc123def456"));
        assert!(audit.contains("sha256:abc123def456"));
        assert!(readme.contains("1 shared packages with version changes"));
        assert!(audit.contains("1 shared packages with version changes"));
    }
```

Run: `cargo test -p inspectah-pipeline readme_audit_parity -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Verify full crate**

Run: `cargo test -p inspectah-pipeline && cargo clippy -p inspectah-pipeline -- -W clippy::all`
Expected: all pass, clean clippy.

- [ ] **Step 6: Commit**

```bash
git add inspectah-pipeline/src/render/audit.rs inspectah-pipeline/src/render/baseline_fmt.rs
git commit -m "feat(audit): add baseline comparison section with parity test

Same baseline_fmt section builder as README. Parity test verifies
README and audit produce consistent baseline metadata for the same
snapshot.

Assisted-by: Claude Code (Opus)"
```

---

## Task 4: Executor `run_with_line_callback` + `extract_baseline` Callback

**Files:**
- Modify: `inspectah-core/src/traits/executor.rs`
- Modify: `inspectah-collect/src/executor/real.rs`
- Modify: `inspectah-collect/src/executor/mock.rs`
- Modify: `inspectah-collect/src/baseline.rs`
- Modify: `inspectah-collect/tests/baseline_test.rs`

This task implements the executor method AND the `extract_baseline` signature change together. There is no intermediate state where `extract_baseline` uses a no-op callback — it always receives a real callback from the caller.

- [ ] **Step 1: Add `run_with_line_callback` to the Executor trait**

In `inspectah-core/src/traits/executor.rs`, add after `run_passthrough_stderr`:

```rust
    /// Run a command, calling `on_stderr_line` for each line of stderr output.
    ///
    /// Used for long-running commands (e.g., `podman pull`) where the caller
    /// wants streaming stderr access for progress display. Full stderr is
    /// still captured in `ExecResult.stderr`. Uses the same 600s timeout
    /// as `run_passthrough_stderr`.
    ///
    /// # Contract
    ///
    /// - Callback is called per-line **live** as stderr is produced, not
    ///   accumulated and replayed after completion.
    /// - Callback runs on the main thread. No `Send` required.
    /// - Full stderr transcript is always available in `ExecResult.stderr`
    ///   for error diagnostics regardless of callback behavior.
    fn run_with_line_callback(
        &self,
        cmd: &str,
        args: &[&str],
        on_stderr_line: &mut dyn FnMut(&str),
    ) -> ExecResult;
```

First, make `libc` unconditional in `inspectah-collect/Cargo.toml` — it is currently optional behind the `ffi-rpm` feature, but `run_with_line_callback` needs `libc::kill` unconditionally:

```toml
# Change from:
libc = { version = "0.2", optional = true }
# To:
libc = "0.2"
```

And update the `ffi-rpm` feature to no longer gate `libc`:

```toml
[features]
default = []
ffi-rpm = []
```

Run: `cargo build -p inspectah-collect`
Expected: compile error — trait requires `run_with_line_callback` implementation in `RealExecutor` and `MockExecutor`.

- [ ] **Step 2: Implement in MockExecutor**

In `inspectah-collect/src/executor/mock.rs`, add inside the `impl Executor for MockExecutor` block:

```rust
    fn run_with_line_callback(
        &self,
        cmd: &str,
        args: &[&str],
        on_stderr_line: &mut dyn FnMut(&str),
    ) -> ExecResult {
        let result = self.run(cmd, args);
        // Split pre-recorded stderr and call callback per-line.
        for line in result.stderr.lines() {
            on_stderr_line(line);
        }
        result
    }
```

- [ ] **Step 3: Implement in RealExecutor (live callback)**

In `inspectah-collect/src/executor/real.rs`, add inside the `impl Executor for RealExecutor` block:

```rust
    fn run_with_line_callback(
        &self,
        cmd: &str,
        args: &[&str],
        on_stderr_line: &mut dyn FnMut(&str),
    ) -> ExecResult {
        let resolved = resolve_command(cmd);
        let child = Command::new(&resolved)
            .args(args)
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                return ExecResult {
                    stderr: e.to_string(),
                    exit_code: 127,
                    ..Default::default()
                };
            }
        };

        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();

        // Architecture: main thread reads stderr line-by-line and calls
        // the callback (no Send required). A watchdog thread handles
        // the 600s timeout/kill.
        let pull_timeout = Duration::from_secs(600);

        std::thread::scope(|s| {
            // Drain stdout in a scoped thread.
            let stdout_thread = s.spawn(|| {
                let mut buf = Vec::new();
                if let Some(r) = stdout_handle {
                    let mut limited = io::Read::take(r, STDOUT_SIZE_CAP as u64 + 1);
                    let _ = io::Read::read_to_end(&mut limited, &mut buf);
                }
                buf
            });

            // Watchdog thread: waits for timeout, then kills the child.
            // Uses an Arc<AtomicBool> to coordinate with the main thread.
            let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let finished_clone = finished.clone();
            let child_id = child.id();
            let watchdog = s.spawn(move || {
                let start = std::time::Instant::now();
                while start.elapsed() < pull_timeout {
                    if finished_clone.load(std::sync::atomic::Ordering::Relaxed) {
                        return false; // main thread finished normally
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
                // Timeout — kill via signal (child.kill() requires &mut,
                // so we use the raw PID kill).
                unsafe {
                    libc::kill(child_id as i32, libc::SIGKILL);
                }
                true // timed out
            });

            // Main thread: read stderr line-by-line, call callback live.
            let mut stderr_lines = Vec::new();
            if let Some(r) = stderr_handle {
                use std::io::BufRead;
                let reader = io::BufReader::new(r);
                for line in reader.lines() {
                    match line {
                        Ok(l) => {
                            on_stderr_line(&l);
                            stderr_lines.push(l);
                        }
                        Err(_) => break,
                    }
                }
            }

            // Signal watchdog that we're done reading stderr.
            finished.store(true, std::sync::atomic::Ordering::Relaxed);
            let timed_out = watchdog.join().unwrap();

            // Wait for child to exit and collect status.
            let status = child.wait();
            let stdout_raw = stdout_thread.join().unwrap();

            let stdout = if stdout_raw.len() > STDOUT_SIZE_CAP {
                let s = String::from_utf8_lossy(&stdout_raw[..STDOUT_SIZE_CAP]).into_owned();
                format!("{s}\n[output truncated at 64 MB]")
            } else {
                String::from_utf8_lossy(&stdout_raw).into_owned()
            };

            if timed_out {
                ExecResult {
                    stdout,
                    stderr: format!(
                        "command timed out after {}s: {} {}",
                        pull_timeout.as_secs(),
                        cmd,
                        args.join(" ")
                    ),
                    exit_code: -1,
                }
            } else {
                let exit_code = match status {
                    Ok(s) => s.code().unwrap_or(-1),
                    Err(_) => -1,
                };
                ExecResult {
                    stdout,
                    stderr: stderr_lines.join("\n"),
                    exit_code,
                }
            }
        })
    }
```

Note: `real.rs` already imports `use std::io;` and `use std::time::Duration;`. The `libc` crate was made unconditional in Step 1 of this task.

- [ ] **Step 4: Build to verify all Executor impls compile**

Run: `cargo build -p inspectah-collect`
Expected: compiles.

- [ ] **Step 5: Write mock executor callback test**

Add to `inspectah-collect/src/executor/mock.rs` tests:

```rust
    #[test]
    fn test_mock_line_callback() {
        let mock = MockExecutor::new().with_command(
            "podman pull quay.io/test:latest",
            ExecResult {
                stderr: "Copying blob sha256:aaa... done\nCopying blob sha256:bbb... skipped\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        );
        let mut lines = Vec::new();
        let result = mock.run_with_line_callback(
            "podman",
            &["pull", "quay.io/test:latest"],
            &mut |line| lines.push(line.to_string()),
        );
        assert_eq!(result.exit_code, 0);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("aaa"));
        assert!(lines[1].contains("bbb"));
    }
```

Run: `cargo test -p inspectah-collect test_mock_line_callback -- --nocapture`
Expected: PASS.

- [ ] **Step 6: Update `extract_baseline` signature and pull step**

In `inspectah-collect/src/baseline.rs`, change the function signature:

```rust
pub fn extract_baseline(
    executor: &dyn Executor,
    normalized_ref: &NormalizedImageRef,
    on_pull_line: &mut dyn FnMut(&str),
) -> Result<BaselineData, ExtractionError> {
```

Add a new helper function after `run_nsenter_passthrough`:

```rust
/// Run a command through the nsenter prefix with per-line stderr callback.
fn run_nsenter_with_callback(
    executor: &dyn Executor,
    cmd_and_args: &[&str],
    on_line: &mut dyn FnMut(&str),
) -> ExecResult {
    let mut full_args: Vec<&str> = NSENTER_PREFIX.to_vec();
    full_args.extend_from_slice(cmd_and_args);
    executor.run_with_line_callback(full_args[0], &full_args[1..], on_line)
}
```

Replace the pull step (line 89):

```rust
    // 1. Pull (with per-line stderr callback for live progress)
    let pull_result = run_nsenter_with_callback(executor, &["podman", "pull", image_ref], on_pull_line);
```

- [ ] **Step 7: Update all callers in baseline_test.rs**

In `inspectah-collect/tests/baseline_test.rs`, update every `extract_baseline` call to pass `&mut |_| {}` as the third argument. For example:

```rust
    let result = extract_baseline(&mock, &normalized, &mut |_| {});
```

- [ ] **Step 8: Build and run tests**

Run: `cargo build -p inspectah-collect && cargo test -p inspectah-collect -- --nocapture`
Expected: compiles, all baseline tests pass.

- [ ] **Step 9: Commit**

```bash
git add inspectah-core/src/traits/executor.rs inspectah-collect/src/executor/real.rs inspectah-collect/src/executor/mock.rs inspectah-collect/src/baseline.rs inspectah-collect/tests/baseline_test.rs
git commit -m "feat(executor): add run_with_line_callback with live stderr

Object-safe Executor trait method for streaming stderr access.
RealExecutor reads stderr on main thread via BufReader, watchdog
thread handles 600s timeout/kill. Callback runs on main thread —
no Send required. MockExecutor splits pre-recorded stderr per-line.

extract_baseline takes on_pull_line callback parameter. Pull step
uses run_with_line_callback, preserving 600s timeout and full stderr
capture.

Assisted-by: Claude Code (Opus)"
```

---

## Task 5: Pull Progress Helpers + CLI Provenance + Version Comparison + Viewport/Passthrough

**Files:**
- Create: `inspectah-cli/src/commands/pull_progress.rs`
- Modify: `inspectah-cli/src/commands/mod.rs` (register module)
- Modify: `inspectah-cli/src/commands/scan.rs`

This task wires everything together in one pass — no intermediate state where scan.rs has a no-op callback.

### Final `scan.rs` message sequence

This is the complete replacement. Lines marked `REMOVE` are deleted; lines marked `NEW` are added. Lines marked `KEEP` are unchanged.

**TTY mode:**
```
Detecting source system...                          # KEEP
  CentOS Stream 9 (aarch64)                         # KEEP
Resolving target image...                           # KEEP
  quay.io/.../centos-bootc:stream9 (OsRelease)      # KEEP
Pulling quay.io/.../centos-bootc:stream9...          # NEW (replaces nothing — new line)
  ┌──────────────────────────────────────────────────┐  # NEW (TTY viewport)
  │ Copying blob sha256:a1b2c3... 42.1 MiB / 89.3   │  # NEW
  │ Copying blob sha256:d4e5f6... done               │  # NEW
  │ Copying blob sha256:g7h8i9... skipped            │  # NEW
  └──────────────────────────────────────────────────┘  # NEW
                                                     # viewport cleared on completion
Pulled quay.io/.../centos-bootc:stream9 (7 blob transfers, sha256:abc123def4)  # NEW (replaces old lines)
  Baseline extracted: 447 packages                   # NEW
  Resolved via: os-release (auto-detected)           # NEW
Scanning host myhost.example.com...                  # KEEP
Scanning host myhost.example.com... done             # KEEP
  Version changes: 85 shared packages with version changes (all target-newer)  # NEW
                                                     # REMOVE: "Pulling target image..."
                                                     # REMOVE: "Extracting baseline... N packages"
Output written to /tmp/inspectah-...tar.gz           # KEEP
```

**Non-TTY mode:** Same as TTY but viewport is replaced with prefixed passthrough:
```
Pulling quay.io/.../centos-bootc:stream9...
  pull: Copying blob sha256:a1b2c3... 42.1 MiB / 89.3 MiB
  pull: Copying blob sha256:d4e5f6... done
  pull: Copying blob sha256:g7h8i9... skipped
Pulled quay.io/.../centos-bootc:stream9 (7 blob transfers, sha256:abc123def4)
  Baseline extracted: 447 packages
  Resolved via: os-release (auto-detected)
```

**Degraded mode (--no-baseline):**
```
Resolving target image...
  quay.io/.../centos-bootc:stream9 (OsRelease)
  Baseline: skipped (--no-baseline)
Scanning host myhost.example.com...
```

**Comparison unavailable:**
```
Pulled quay.io/.../centos-bootc:stream9 (sha256:abc123def4)
  Baseline extracted: 447 packages
  Resolved via: os-release (auto-detected)
Scanning host myhost.example.com...
Scanning host myhost.example.com... done
  Version comparison: data unavailable
```

- [ ] **Step 1: Register `pull_progress` module**

In `inspectah-cli/src/commands/mod.rs`, add:

```rust
pub mod pull_progress;
```

Create `inspectah-cli/src/commands/pull_progress.rs` with:

```rust
//! Pull progress display helpers.
//!
//! Handles TTY viewport rendering and non-TTY passthrough for
//! podman pull stderr output. All rendering functions are pure —
//! they take typed inputs and return formatted strings or side-effect
//! through provided writers.
```

Run: `cargo build -p inspectah-cli`
Expected: compiles (empty module).

- [ ] **Step 2: Write failing tests for ANSI stripping**

Add to `pull_progress.rs`:

```rust
/// Strip ANSI escape sequences from a string.
///
/// Removes CSI sequences (\x1b[...X) and OSC sequences (\x1b]...\x07).
/// Used to clean podman's colored/cursor-controlled stderr output for
/// the TTY viewport.
pub fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // CSI: \x1b[ ... <letter>
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() || next == 'H' || next == 'J' || next == 'K' {
                        break;
                    }
                }
            }
            // OSC: \x1b] ... \x07
            else if chars.peek() == Some(&']') {
                chars.next();
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next == '\x07' {
                        break;
                    }
                }
            }
            // Other escape — skip next char
            else {
                chars.next();
            }
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_color() {
        assert_eq!(strip_ansi("\x1b[32mgreen\x1b[0m"), "green");
    }

    #[test]
    fn strip_ansi_cursor_movement() {
        assert_eq!(strip_ansi("\x1b[3Atext"), "text");
    }

    #[test]
    fn strip_ansi_no_escape() {
        assert_eq!(strip_ansi("plain text"), "plain text");
    }
}
```

Run: `cargo test -p inspectah-cli strip_ansi -- --nocapture`
Expected: PASS.

- [ ] **Step 3: Write failing tests for blob counting**

Add to `pull_progress.rs`:

```rust
/// Count unique completed blob transfers from podman pull stderr lines.
///
/// Looks for lines matching `Copying blob <sha256:...> done|skipped`.
/// Uses a HashSet on the sha256 prefix to deduplicate — podman may
/// emit multiple progress lines for the same blob before the final
/// `done`/`skipped` line.
///
/// Returns `None` if no completed blob lines were found (unexpected
/// output format or non-pull command). The count is best-effort
/// display-only; it is not persisted in the snapshot.
pub fn count_completed_blobs(stderr_lines: &[String]) -> Option<usize> {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    for line in stderr_lines {
        let stripped = strip_ansi(line);
        if !stripped.contains("Copying blob") {
            continue;
        }
        if !stripped.ends_with("done") && !stripped.ends_with("skipped") {
            continue;
        }
        // Extract the blob identifier (sha256:... prefix)
        if let Some(start) = stripped.find("sha256:") {
            let rest = &stripped[start..];
            let id = rest.split_whitespace().next().unwrap_or(rest);
            seen.insert(id.to_string());
        }
    }
    if seen.is_empty() {
        None
    } else {
        Some(seen.len())
    }
}
```

Add tests:

```rust
    #[test]
    fn count_blobs_normal() {
        let lines: Vec<String> = vec![
            "Copying blob sha256:aaa111 done".into(),
            "Copying blob sha256:bbb222 done".into(),
            "Copying blob sha256:ccc333 skipped".into(),
        ];
        assert_eq!(count_completed_blobs(&lines), Some(3));
    }

    #[test]
    fn count_blobs_deduplicates() {
        let lines: Vec<String> = vec![
            "Copying blob sha256:aaa111 42 MiB / 89 MiB".into(),
            "Copying blob sha256:aaa111 done".into(),
            "Copying blob sha256:aaa111 done".into(), // duplicate final
        ];
        assert_eq!(count_completed_blobs(&lines), Some(1));
    }

    #[test]
    fn count_blobs_with_progress_lines() {
        let lines: Vec<String> = vec![
            "Copying blob sha256:aaa111 42 MiB / 89 MiB".into(),
            "Copying blob sha256:aaa111 done".into(),
        ];
        assert_eq!(count_completed_blobs(&lines), Some(1));
    }

    #[test]
    fn count_blobs_empty() {
        let lines: Vec<String> = vec!["Writing manifest".into()];
        assert_eq!(count_completed_blobs(&lines), None);
    }

    #[test]
    fn count_blobs_with_ansi() {
        let lines: Vec<String> = vec![
            "\x1b[32mCopying blob sha256:aaa111 done\x1b[0m".into(),
        ];
        assert_eq!(count_completed_blobs(&lines), Some(1));
    }
```

Run: `cargo test -p inspectah-cli count_blobs -- --nocapture`
Expected: PASS.

- [ ] **Step 4: Write failing tests for pull summary**

Add to `pull_progress.rs`:

```rust
/// Format the pull summary line shown after pull completes.
///
/// Uses "blob transfers" rather than "layers" — the pull progress is
/// transport-level blob chatter, not stable image-model truth.
pub fn pull_summary_line(image_ref: &str, digest: &str, blob_count: Option<usize>) -> String {
    let short_digest = if digest.len() > 19 {
        &digest[..19]
    } else {
        digest
    };
    match blob_count {
        Some(n) => format!("Pulled {image_ref} ({n} blob transfers, {short_digest})"),
        None => format!("Pulled {image_ref} ({short_digest})"),
    }
}
```

Add tests:

```rust
    #[test]
    fn pull_summary_with_blobs() {
        let line = pull_summary_line("quay.io/test:latest", "sha256:abc123def456789", Some(7));
        assert!(line.contains("7 blob transfers"));
        assert!(line.contains("sha256:abc123def45678"));
        assert!(!line.contains("layers"));
    }

    #[test]
    fn pull_summary_without_blobs() {
        let line = pull_summary_line("quay.io/test:latest", "sha256:abc123", None);
        assert!(!line.contains("blob transfers"));
        assert!(line.contains("sha256:abc123"));
    }
```

Run: `cargo test -p inspectah-cli pull_summary -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Write viewport and non-TTY callback helpers**

Add to `pull_progress.rs`:

```rust
use std::io::Write;

/// Minimum terminal width for TTY viewport. Below this, fall back to non-TTY.
const MIN_VIEWPORT_WIDTH: usize = 40;

/// Maximum viewport content width.
const MAX_VIEWPORT_WIDTH: usize = 72;

/// Number of recent lines shown in the viewport.
const VIEWPORT_LINES: usize = 3;

/// Truncate a string to `max_len` characters, appending `…` if truncated.
fn truncate_line(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else if max_len > 1 {
        format!("{}…", &s[..max_len - 1])
    } else {
        "…".to_string()
    }
}

/// Create the non-TTY callback: prints each stderr line with `  pull: ` prefix.
///
/// Also collects lines for post-completion blob counting.
pub fn non_tty_callback(collected: &mut Vec<String>) -> impl FnMut(&str) + '_ {
    move |line: &str| {
        let cleaned = strip_ansi(line);
        if !cleaned.trim().is_empty() {
            eprintln!("  pull: {cleaned}");
        }
        collected.push(cleaned);
    }
}

/// Create the TTY viewport callback: renders a 3-line box-drawing viewport
/// with recent stderr lines.
///
/// Also collects lines for post-completion blob counting.
pub fn tty_viewport_callback(
    collected: &mut Vec<String>,
    ring: &mut [String; VIEWPORT_LINES],
    ring_pos: &mut usize,
    content_width: usize,
) -> impl FnMut(&str) + '_ {
    move |line: &str| {
        let cleaned = strip_ansi(line);
        if cleaned.trim().is_empty() {
            return;
        }
        collected.push(cleaned.clone());

        // Push into ring buffer
        ring[*ring_pos % VIEWPORT_LINES] = truncate_line(&cleaned, content_width);
        *ring_pos += 1;

        // Redraw viewport
        let stderr = std::io::stderr();
        let mut w = stderr.lock();

        // Move cursor up to clear previous viewport (if not first draw)
        if *ring_pos > 1 {
            let _ = write!(w, "\x1b[5A"); // up 5 lines (top border + 3 content + bottom border)
        }

        // Draw top border
        let _ = writeln!(w, "  \u{250c}{}\u{2510}", "\u{2500}".repeat(content_width + 2));
        // Draw content lines
        for i in 0..VIEWPORT_LINES {
            let idx = if *ring_pos >= VIEWPORT_LINES {
                (*ring_pos - VIEWPORT_LINES + i) % VIEWPORT_LINES
            } else if i < *ring_pos {
                i
            } else {
                // Empty slot
                let _ = writeln!(w, "  \u{2502} {:<width$} \u{2502}", "", width = content_width);
                continue;
            };
            let _ = writeln!(
                w,
                "  \u{2502} {:<width$} \u{2502}",
                ring[idx],
                width = content_width
            );
        }
        // Draw bottom border
        let _ = writeln!(w, "  \u{2514}{}\u{2518}", "\u{2500}".repeat(content_width + 2));
        let _ = w.flush();
    }
}

/// Clear the TTY viewport after pull completes (or fails).
///
/// Moves cursor up and clears each viewport line.
pub fn viewport_cleanup() {
    let stderr = std::io::stderr();
    let mut w = stderr.lock();
    // Clear 5 lines: top border + 3 content + bottom border
    let _ = write!(w, "\x1b[5A"); // move up 5
    for _ in 0..5 {
        let _ = write!(w, "\x1b[2K\n"); // clear line, move down
    }
    let _ = write!(w, "\x1b[5A"); // move back up
    let _ = w.flush();
}

/// Determine the viewport content width from terminal width.
pub fn viewport_content_width(term_width: usize) -> usize {
    // Box borders: 2 (left "  │ ") + 2 (right " │") = 6 chars overhead
    let max = term_width.saturating_sub(6);
    max.min(MAX_VIEWPORT_WIDTH - 6)
}
```

Add tests:

```rust
    #[test]
    fn truncate_line_short() {
        assert_eq!(truncate_line("hello", 10), "hello");
    }

    #[test]
    fn truncate_line_exact() {
        assert_eq!(truncate_line("hello", 5), "hello");
    }

    #[test]
    fn truncate_line_long() {
        let result = truncate_line("hello world", 6);
        assert!(result.ends_with('…'));
        assert_eq!(result.chars().count(), 6);
    }

    #[test]
    fn viewport_content_width_normal() {
        // 80 col terminal: 80 - 6 = 74, capped at 66 (MAX_VIEWPORT_WIDTH - 6)
        let w = viewport_content_width(80);
        assert!(w <= MAX_VIEWPORT_WIDTH - 6);
        assert!(w > 0);
    }

    #[test]
    fn viewport_content_width_narrow() {
        let w = viewport_content_width(45);
        assert!(w > 0);
    }

    #[test]
    fn non_tty_callback_collects() {
        let mut collected = Vec::new();
        {
            let mut cb = non_tty_callback(&mut collected);
            cb("Copying blob sha256:aaa done");
            cb("Copying blob sha256:bbb skipped");
        }
        assert_eq!(collected.len(), 2);
        assert!(collected[0].contains("aaa"));
    }
```

Run: `cargo test -p inspectah-cli pull_progress -- --nocapture`
Expected: PASS.

- [ ] **Step 6: Wire scan.rs — replace baseline extraction block**

In `inspectah-cli/src/commands/scan.rs`:

Add imports at the top:

```rust
use super::pull_progress;
use inspectah_pipeline::render::baseline_fmt;
```

Replace the entire Step 3 block (lines 162-172) with:

```rust
    // Step 3: Extract baseline
    let baseline_data = match (&normalized_ref, args.no_baseline) {
        (Some(norm), false) => {
            eprintln!("Pulling {}...", norm.as_str());

            let use_viewport = std::io::IsTerminal::is_terminal(&std::io::stderr());
            let mut collected_lines: Vec<String> = Vec::new();

            let data = if use_viewport {
                // TTY: viewport rendering
                let term_width = terminal_size::terminal_size()
                    .map(|(w, _)| w.0 as usize)
                    .unwrap_or(80);

                if term_width >= 40 {
                    let content_width = pull_progress::viewport_content_width(term_width);
                    let mut ring = [String::new(), String::new(), String::new()];
                    let mut ring_pos: usize = 0;

                    let result = {
                        let mut callback = pull_progress::tty_viewport_callback(
                            &mut collected_lines,
                            &mut ring,
                            &mut ring_pos,
                            content_width,
                        );
                        inspectah_collect::baseline::extract_baseline(
                            &executor, norm, &mut callback,
                        )
                    };
                    // Clear viewport on both success and failure before propagating.
                    pull_progress::viewport_cleanup();
                    result.context("baseline extraction failed")?
                } else {
                    // Narrow terminal — fall back to non-TTY
                    let mut callback = pull_progress::non_tty_callback(&mut collected_lines);
                    inspectah_collect::baseline::extract_baseline(
                        &executor, norm, &mut callback,
                    )
                    .context("baseline extraction failed")?
                }
            } else {
                // Non-TTY: prefixed passthrough
                let mut callback = pull_progress::non_tty_callback(&mut collected_lines);
                inspectah_collect::baseline::extract_baseline(
                    &executor, norm, &mut callback,
                )
                .context("baseline extraction failed")?
            };

            // Pull summary line
            let blob_count = pull_progress::count_completed_blobs(&collected_lines);
            eprintln!(
                "{}",
                pull_progress::pull_summary_line(
                    norm.as_str(),
                    &data.image_digest,
                    blob_count,
                )
            );

            // Provenance block
            eprintln!("  Baseline extracted: {} packages", data.packages.len());
            if let Some(ti) = &target_image {
                eprintln!("  Resolved via: {}", baseline_fmt::strategy_label(&ti.strategy));
            }

            Some(data)
        }
        (Some(_norm), true) => {
            // --no-baseline: show degraded message
            eprintln!("  Baseline: skipped (--no-baseline)");
            None
        }
        _ => None,
    };
```

- [ ] **Step 7: Wire scan.rs — add version comparison after collection**

The version comparison line prints after the Phase 6 fields are set on the snapshot (line 202: `snapshot.no_baseline = args.no_baseline;`) and before `redact(...)`.

Add after line 202:

```rust
    // Version comparison line (prints after collection, since version_changes
    // is populated by the RPM inspector during collection)
    if baseline_data.is_some() {
        let vc_display = baseline_fmt::version_changes_for_display(&snapshot);
        let shared_count = match (snapshot.rpm.as_ref(), snapshot.baseline.as_ref()) {
            (Some(rpm), Some(bl)) if baseline_fmt::is_rpm_comparison_available(&snapshot) => {
                baseline_fmt::shared_package_count(bl, rpm)
            }
            _ => 0,
        };
        let summary = baseline_fmt::version_comparison_summary(vc_display, shared_count);
        if vc_display.is_none() {
            eprintln!("  Version comparison: {summary}");
        } else {
            eprintln!("  Version changes: {summary}");
        }
    }
```

**Important:** `baseline_data` is moved into `snapshot.baseline` on line 201 (`snapshot.baseline = baseline_data;`). The version comparison block reads from `snapshot.baseline`, not from the local `baseline_data` variable. Adjust the guard accordingly — use `snapshot.baseline.is_some()` instead of `baseline_data.is_some()`.

- [ ] **Step 8: Add `terminal_size` dependency**

Run: `cargo add terminal_size --manifest-path inspectah-cli/Cargo.toml`

Or add manually to `inspectah-cli/Cargo.toml`:
```toml
terminal_size = "0.4"
```

- [ ] **Step 9: Build and verify full pipeline**

Run: `cargo build -p inspectah-cli && cargo clippy -p inspectah-cli -- -W clippy::all && cargo fmt --check`
Expected: compiles, clean clippy, format passes.

- [ ] **Step 10: Run full test suite**

Run: `cargo test -p inspectah-cli && cargo test -p inspectah-collect && cargo test -p inspectah-pipeline`
Expected: all pass.

- [ ] **Step 11: Commit**

```bash
git add inspectah-cli/src/commands/pull_progress.rs inspectah-cli/src/commands/mod.rs inspectah-cli/src/commands/scan.rs inspectah-cli/Cargo.toml
git commit -m "feat(cli): add pull progress viewport and baseline provenance

TTY: 3-line box-drawing viewport with ring buffer for live pull
progress. Non-TTY: prefixed line passthrough for CI liveness.
Both: pull summary line with blob transfer count (best-effort,
display-only) and image digest.

Provenance block (package count + resolution strategy) prints
immediately after extraction. Version comparison line prints
after collection, using shared baseline_fmt helpers with
Option<&[VersionChange]> for comparison-unavailable honesty
and shared_package_count for accurate counts.

Replaces old 'Pulling target image...' and 'Extracting baseline...'
messages with the new structured output sequence.

Assisted-by: Claude Code (Opus)"
```

---

## Dependency Graph

```
Task 1 (baseline_fmt helpers)
  ├── Task 2 (README baseline section)
  │     └── Task 3 (audit baseline section + parity test)
  └── Task 4 (executor + extract_baseline callback)
        └── Task 5 (pull_progress + scan.rs wiring)
```

Tasks 1 is the foundation. Tasks 2 and 4 can run in parallel after Task 1. Task 3 depends on Task 2 (parity test needs both readme and audit). Task 5 depends on Tasks 1 and 4.

**Ship points:**
- Tasks 1-3 can ship independently (rendered artifacts only).
- Task 4 changes the `extract_baseline` API but is backward-compatible for callers passing `&mut |_| {}`.
- Task 5 is the CLI-visible behavior change — it requires Tasks 1 and 4.

## Implementation Notes

**Comparison truth contract:** The design spec's `version_comparison_summary` takes `baseline_count` as a parameter. The review correctly identified that `baseline.packages.len()` is NOT the right value for this — it includes baseline-only packages that are not on the host. The `shared_package_count` helper computes `baseline.packages.len() - rpm.base_image_only.len()`, which is the actual count of packages present in both. Additionally, `is_rpm_comparison_available` checks `Completeness` state (the `is_degraded` pattern from `containerfile.rs`), not just `snap.rpm.is_some()`, to prevent treating degraded RPM data as comparison-ready.

**Live callback, not accumulate-then-replay:** The `RealExecutor::run_with_line_callback` reads stderr on the main thread via `BufReader::lines()` and calls the callback immediately. A watchdog thread handles the 600s timeout/kill via `libc::kill`. The callback never requires `Send` because it runs on the caller's thread. There is no fallback path to `run_passthrough_stderr` or plain `run`.

**"Blob transfers" not "layers":** Following Collins's review, the pull summary uses "blob transfers" to accurately describe what is being counted — transport-level blob completion lines, not stable image layers. The count uses a `HashSet` on `sha256:` prefixes to deduplicate, since podman may emit multiple progress lines for the same blob.

**Scan.rs message sequence:** Task 5 defines the complete replacement message sequence, not an additive layer. The old `"Pulling target image..."` and `"Extracting baseline... N packages"` lines are removed and replaced by the structured provenance block.

**`run_nsenter_passthrough` is retained:** The existing function is not deleted — it may be used by other callers in the future. Only the baseline pull step switches to `run_nsenter_with_callback`.

**Cargo path:** `export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"`
