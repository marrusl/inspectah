# Baseline Visibility Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface baseline image metadata (image ref, resolution strategy, digest, extraction timestamp, version comparison) in CLI output, README, and audit report — plus replace raw podman pull noise with a compact viewport.

**Architecture:** A shared `baseline_fmt` module provides pure formatting helpers. README and audit renderers call these helpers. CLI uses the same helpers for provenance/comparison output. The executor gains a `run_with_line_callback` method for streaming stderr, and the CLI renders a 3-line viewport (TTY) or prefixed passthrough (non-TTY).

**Tech Stack:** Rust (inspectah-core, inspectah-collect, inspectah-pipeline, inspectah-cli). No new dependencies for tasks 1-3. Task 4 may need `terminal_size` crate for TTY width detection.

**Spec:** `docs/specs/proposed/2026-05-18-baseline-visibility-design.md` (revision 4)

**Repo:** `/Users/mrussell/Work/bootc-migration/inspectah/`

**Cargo:** `export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"`

**No team member names in code or commits** (public repo).

---

### Task 1: Shared Presentation Helpers — `baseline_fmt` module

**Files:**
- Create: `inspectah-pipeline/src/render/baseline_fmt.rs`
- Modify: `inspectah-pipeline/src/render/mod.rs`

This is the foundation. All subsequent tasks consume these helpers.

- [ ] **Step 1: Write failing test for `strategy_label`**

```rust
// At the bottom of baseline_fmt.rs, inside #[cfg(test)] mod tests
#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::baseline::ResolutionStrategy;

    #[test]
    fn strategy_label_all_variants() {
        assert_eq!(strategy_label(&ResolutionStrategy::CliOverride), "--base-image (user-specified)");
        assert_eq!(strategy_label(&ResolutionStrategy::UniversalBlue), "ublue image-info.json");
        assert_eq!(strategy_label(&ResolutionStrategy::BootcStatus), "bootc status (booted deployment)");
        assert_eq!(strategy_label(&ResolutionStrategy::FedoraAtomicDesktop), "fedora-atomic-desktop image-info.json");
        assert_eq!(strategy_label(&ResolutionStrategy::OsRelease), "os-release (auto-detected)");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-pipeline strategy_label_all_variants -- --nocapture`
Expected: compilation error — module doesn't exist yet.

- [ ] **Step 3: Create `baseline_fmt.rs` with `strategy_label` implementation**

```rust
//! Shared presentation helpers for baseline metadata.
//!
//! Used by CLI, README, and audit renderers to produce consistent
//! baseline provenance and version comparison output.

use inspectah_core::baseline::ResolutionStrategy;

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
```

Register in `mod.rs` by adding `pub mod baseline_fmt;` after the existing module declarations.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p inspectah-pipeline strategy_label_all_variants -- --nocapture`
Expected: PASS

- [ ] **Step 5: Write failing tests for `version_comparison_summary`**

Add to the `tests` module in `baseline_fmt.rs`:

```rust
    use inspectah_core::types::rpm::{VersionChange, VersionChangeDirection};

    fn make_vc(direction: VersionChangeDirection) -> VersionChange {
        VersionChange {
            name: "test".into(),
            direction,
            ..Default::default()
        }
    }

    #[test]
    fn version_comparison_data_unavailable() {
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
        let result = version_comparison_summary(Some(&vcs), 447);
        assert!(result.contains("2 shared packages with version changes"));
        assert!(result.contains("all target-newer"));
    }

    #[test]
    fn version_comparison_all_downgrades() {
        let vcs = vec![make_vc(VersionChangeDirection::Downgrade)];
        let result = version_comparison_summary(Some(&vcs), 447);
        assert!(result.contains("1 shared packages with version changes"));
        assert!(result.contains("all host-newer"));
    }

    #[test]
    fn version_comparison_mixed() {
        let vcs = vec![
            make_vc(VersionChangeDirection::Upgrade),
            make_vc(VersionChangeDirection::Upgrade),
            make_vc(VersionChangeDirection::Downgrade),
        ];
        let result = version_comparison_summary(Some(&vcs), 447);
        assert!(result.contains("3 shared packages with version changes"));
        assert!(result.contains("2 target-newer, 1 host-newer"));
    }
```

- [ ] **Step 6: Run tests to verify they fail**

Run: `cargo test -p inspectah-pipeline version_comparison -- --nocapture`
Expected: compilation error — function doesn't exist.

- [ ] **Step 7: Implement `version_comparison_summary`**

Add to `baseline_fmt.rs`:

```rust
use inspectah_core::types::rpm::{VersionChange, VersionChangeDirection};

/// Version comparison summary line.
///
/// Takes `None` when RPM data is unavailable (degraded inspector),
/// `Some(&[])` when no packages differ, or `Some(&[...])` with actual changes.
/// This distinction prevents false "zero drift" when the real state is
/// "comparison data unavailable."
pub fn version_comparison_summary(
    version_changes: Option<&[VersionChange]>,
    baseline_count: usize,
) -> String {
    match version_changes {
        None => "comparison data unavailable".to_string(),
        Some(vcs) if vcs.is_empty() => {
            format!("all {baseline_count} shared packages at same version")
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

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test -p inspectah-pipeline version_comparison -- --nocapture`
Expected: all 5 PASS

- [ ] **Step 9: Write failing tests for `baseline_section_lines`**

Add to `tests` module:

```rust
    use inspectah_core::baseline::{BaselineData, TargetImageIdentity};
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
    fn baseline_section_full_state() {
        let ti = test_target_image();
        let mut bl = test_baseline();
        // Simulate 447 packages
        for i in 0..447 {
            bl.packages.insert(
                format!("pkg{i}.x86_64"),
                inspectah_core::baseline::BaselinePackageEntry {
                    name: format!("pkg{i}"),
                    epoch: None,
                    version: "1.0".into(),
                    release: "1.el9".into(),
                    arch: "x86_64".into(),
                },
            );
        }
        let vcs = vec![make_vc(VersionChangeDirection::Upgrade)];
        let lines = baseline_section_lines(Some(&ti), Some(&bl), Some(&vcs), false);
        let text = lines.join("\n");
        assert!(text.contains("quay.io/centos-bootc/centos-bootc:stream9"));
        assert!(text.contains("os-release (auto-detected)"));
        assert!(text.contains("sha256:abc123def456"));
        assert!(text.contains("2026-05-18T14:32:00Z"));
        assert!(text.contains("447"));
        assert!(text.contains("1 shared packages with version changes"));
    }

    #[test]
    fn baseline_section_comparison_unavailable() {
        let ti = test_target_image();
        let bl = test_baseline();
        let lines = baseline_section_lines(Some(&ti), Some(&bl), None, false);
        let text = lines.join("\n");
        assert!(text.contains("quay.io/centos-bootc/centos-bootc:stream9"));
        assert!(text.contains("comparison data unavailable"));
    }

    #[test]
    fn baseline_section_degraded_no_baseline() {
        let ti = test_target_image();
        let lines = baseline_section_lines(Some(&ti), None, None, true);
        let text = lines.join("\n");
        assert!(text.contains("quay.io/centos-bootc/centos-bootc:stream9"));
        assert!(text.contains("skipped (--no-baseline)"));
    }

    #[test]
    fn baseline_section_degraded_unavailable() {
        let ti = test_target_image();
        let lines = baseline_section_lines(Some(&ti), None, None, false);
        let text = lines.join("\n");
        assert!(text.contains("quay.io/centos-bootc/centos-bootc:stream9"));
        assert!(text.contains("unavailable"));
    }

    #[test]
    fn baseline_section_unknown_omitted() {
        let lines = baseline_section_lines(None, None, None, false);
        assert!(lines.is_empty());
    }
```

- [ ] **Step 10: Implement `baseline_section_lines`**

Add to `baseline_fmt.rs`:

```rust
use inspectah_core::baseline::{BaselineData, TargetImageIdentity};

/// Build markdown lines for the baseline comparison section.
///
/// Handles four states:
/// - Full: target_image + baseline + version_changes all present
/// - Comparison unavailable: target_image + baseline present, version_changes is None
/// - Degraded: target_image present, baseline absent
/// - Unknown: target_image absent → returns empty vec
pub fn baseline_section_lines(
    target_image: Option<&TargetImageIdentity>,
    baseline: Option<&BaselineData>,
    version_changes: Option<&[VersionChange]>,
    no_baseline: bool,
) -> Vec<String> {
    let ti = match target_image {
        Some(ti) => ti,
        None => return Vec::new(),
    };

    let mut lines = vec![
        "## Baseline comparison".into(),
        String::new(),
        "| | |".into(),
        "|---|---|".into(),
        format!("| Target image | {} |", ti.image_ref),
        format!("| Resolution | {} |", strategy_label(&ti.strategy)),
    ];

    match baseline {
        Some(bl) => {
            lines.push(format!("| Image digest | {} |", bl.image_digest));
            lines.push(format!("| Baseline extracted | {} |", bl.extracted_at));
            lines.push(format!("| Baseline packages | {} |", bl.packages.len()));
            lines.push(format!(
                "| Version changes | {} |",
                version_comparison_summary(version_changes, bl.packages.len())
            ));
        }
        None => {
            let status = if no_baseline {
                "skipped (--no-baseline)"
            } else {
                "unavailable"
            };
            lines.push(format!("| Baseline | {} |", status));
        }
    }

    lines.push(String::new());
    lines
}
```

- [ ] **Step 11: Run all `baseline_fmt` tests**

Run: `cargo test -p inspectah-pipeline baseline_section -- --nocapture && cargo test -p inspectah-pipeline version_comparison -- --nocapture && cargo test -p inspectah-pipeline strategy_label -- --nocapture`
Expected: all PASS

- [ ] **Step 12: Run clippy**

Run: `cargo clippy -p inspectah-pipeline -- -W clippy::all`
Expected: no new warnings

- [ ] **Step 13: Commit**

```bash
git add inspectah-pipeline/src/render/baseline_fmt.rs inspectah-pipeline/src/render/mod.rs
git commit -m "feat(render): add baseline_fmt shared presentation helpers

Strategy labels, version comparison summary (with Option for
data-unavailable vs zero-changes), and markdown section builder
for baseline metadata. Used by CLI, README, and audit renderers.

Assisted-by: Claude Code (Opus)"
```

---

### Task 2: README Baseline Section

**Files:**
- Modify: `inspectah-pipeline/src/render/readme.rs`

- [ ] **Step 1: Write failing test for README baseline section**

Add to `readme.rs` `#[cfg(test)] mod tests`:

```rust
    use inspectah_core::baseline::{BaselineData, TargetImageIdentity, ResolutionStrategy};
    use inspectah_core::types::rpm::{VersionChange, VersionChangeDirection, RpmSection};
    use std::collections::HashMap;

    #[test]
    fn readme_includes_baseline_section_full() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(TargetImageIdentity {
            image_ref: "quay.io/centos-bootc/centos-bootc:stream9".into(),
            strategy: ResolutionStrategy::OsRelease,
        });
        snap.baseline = Some(BaselineData {
            image_digest: "sha256:abc123".into(),
            packages: HashMap::new(),
            extracted_at: "2026-05-18T14:32:00Z".into(),
        });
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
        assert!(md.contains("quay.io/centos-bootc/centos-bootc:stream9"));
        assert!(md.contains("os-release (auto-detected)"));
        assert!(md.contains("sha256:abc123"));
    }

    #[test]
    fn readme_baseline_degraded_shows_target_image() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(TargetImageIdentity {
            image_ref: "quay.io/centos-bootc/centos-bootc:stream9".into(),
            strategy: ResolutionStrategy::OsRelease,
        });
        snap.no_baseline = true;
        let md = render_readme(&snap);
        assert!(md.contains("## Baseline comparison"));
        assert!(md.contains("skipped (--no-baseline)"));
    }

    #[test]
    fn readme_baseline_omitted_when_no_target_image() {
        let snap = InspectionSnapshot::new();
        let md = render_readme(&snap);
        assert!(!md.contains("## Baseline comparison"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-pipeline readme_includes_baseline -- --nocapture`
Expected: FAIL — no baseline section in output.

- [ ] **Step 3: Wire `baseline_section_lines` into `render_readme`**

In `readme.rs`, add `use super::baseline_fmt;` at the top with the other imports.

Replace the line:
```rust
    let _ = base_image_from_snapshot(snap); // retained for future FROM reference
```

With:
```rust
    let version_changes = snap.rpm.as_ref().map(|r| r.version_changes.as_slice());
    let baseline_lines = baseline_fmt::baseline_section_lines(
        snap.target_image.as_ref(),
        snap.baseline.as_ref(),
        version_changes,
        snap.no_baseline,
    );
    for line in baseline_lines {
        lines.push(line);
    }
```

Also remove the `use super::containerfile::base_image_from_snapshot;` import if it becomes unused.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p inspectah-pipeline readme_ -- --nocapture`
Expected: all PASS (new and existing)

- [ ] **Step 5: Run clippy and full pipeline tests**

Run: `cargo clippy -p inspectah-pipeline -- -W clippy::all && cargo test -p inspectah-pipeline`
Expected: clean clippy, all tests pass

- [ ] **Step 6: Commit**

```bash
git add inspectah-pipeline/src/render/readme.rs
git commit -m "feat(readme): add baseline comparison section

Shows target image, resolution strategy, digest, extraction time,
and version comparison. Handles full, degraded, and unknown states.
Replaces unused base_image_from_snapshot stub.

Assisted-by: Claude Code (Opus)"
```

---

### Task 3: Audit Report Baseline Section

**Files:**
- Modify: `inspectah-pipeline/src/render/audit.rs`

- [ ] **Step 1: Write failing test for audit baseline section**

Add to `audit.rs` `#[cfg(test)] mod tests`:

```rust
    use inspectah_core::baseline::{BaselineData, TargetImageIdentity, ResolutionStrategy};
    use std::collections::HashMap;

    #[test]
    fn audit_includes_baseline_section() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(TargetImageIdentity {
            image_ref: "quay.io/centos-bootc/centos-bootc:stream9".into(),
            strategy: ResolutionStrategy::OsRelease,
        });
        snap.baseline = Some(BaselineData {
            image_digest: "sha256:abc123".into(),
            packages: HashMap::new(),
            extracted_at: "2026-05-18T14:32:00Z".into(),
        });
        let output = render_audit(&snap);
        assert!(output.contains("## Baseline comparison"));
        assert!(output.contains("quay.io/centos-bootc/centos-bootc:stream9"));
    }

    #[test]
    fn audit_baseline_before_packages() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(TargetImageIdentity {
            image_ref: "quay.io/centos-bootc/centos-bootc:stream9".into(),
            strategy: ResolutionStrategy::OsRelease,
        });
        snap.baseline = Some(BaselineData {
            image_digest: "sha256:abc123".into(),
            packages: HashMap::new(),
            extracted_at: "2026-05-18T14:32:00Z".into(),
        });
        snap.rpm = Some(RpmSection::default());
        let output = render_audit(&snap);
        let baseline_pos = output.find("## Baseline comparison").unwrap();
        let packages_pos = output.find("## Packages").unwrap();
        assert!(baseline_pos < packages_pos, "baseline must come before packages");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-pipeline audit_includes_baseline -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Wire `baseline_section_lines` into `render_audit`**

In `audit.rs`, add `use super::baseline_fmt;` at the top.

Insert the baseline section after the "Incomplete Sections" block but before the "Packages" block. Find the line that starts the packages section (around line 55: `if let Some(rpm) = &snap.rpm {`) and insert before it:

```rust
    // Baseline comparison
    let version_changes = snap.rpm.as_ref().map(|r| r.version_changes.as_slice());
    let baseline_lines = baseline_fmt::baseline_section_lines(
        snap.target_image.as_ref(),
        snap.baseline.as_ref(),
        version_changes,
        snap.no_baseline,
    );
    for line in baseline_lines {
        lines.push(line);
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p inspectah-pipeline audit_ -- --nocapture`
Expected: all PASS

- [ ] **Step 5: Write parity test**

Add to `audit.rs` tests:

```rust
    #[test]
    fn audit_and_readme_baseline_parity() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(TargetImageIdentity {
            image_ref: "quay.io/centos-bootc/centos-bootc:stream9".into(),
            strategy: ResolutionStrategy::OsRelease,
        });
        snap.baseline = Some(BaselineData {
            image_digest: "sha256:abc123".into(),
            packages: HashMap::new(),
            extracted_at: "2026-05-18T14:32:00Z".into(),
        });
        let audit = render_audit(&snap);
        let readme = super::readme::render_readme(&snap);
        // Both must contain the same baseline metadata
        for needle in &[
            "quay.io/centos-bootc/centos-bootc:stream9",
            "os-release (auto-detected)",
            "sha256:abc123",
            "2026-05-18T14:32:00Z",
        ] {
            assert!(audit.contains(needle), "audit missing: {needle}");
            assert!(readme.contains(needle), "readme missing: {needle}");
        }
    }
```

- [ ] **Step 6: Run parity test and full suite**

Run: `cargo test -p inspectah-pipeline audit_and_readme -- --nocapture && cargo test -p inspectah-pipeline`
Expected: all PASS

- [ ] **Step 7: Commit**

```bash
git add inspectah-pipeline/src/render/audit.rs
git commit -m "feat(audit): add baseline comparison section before packages

Same content as README baseline section via shared helpers.
Includes parity test verifying README and audit consistency.

Assisted-by: Claude Code (Opus)"
```

---

### Task 4: CLI Provenance and Version Comparison Output

**Files:**
- Modify: `inspectah-cli/src/commands/scan.rs`
- Create: `inspectah-pipeline/src/render/baseline_fmt.rs` (add CLI helpers)

CLI integration tests require a seam that doesn't exist yet. This task adds pure formatting helpers and wires them into `scan.rs`. The helpers are tested directly; the CLI wiring is thin enough to verify by inspection.

- [ ] **Step 1: Write failing tests for CLI formatting helpers**

Add to `baseline_fmt.rs` tests:

```rust
    #[test]
    fn cli_provenance_lines_full() {
        let ti = test_target_image();
        let bl = test_baseline();
        let lines = cli_provenance_lines(&ti, &bl);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("447") || lines[0].contains("0")); // package count
        assert!(lines[1].contains("os-release (auto-detected)"));
    }

    #[test]
    fn cli_version_comparison_line_with_data() {
        let vcs = vec![make_vc(VersionChangeDirection::Upgrade)];
        let line = cli_version_comparison_line(Some(&vcs), 447);
        assert!(line.contains("Version changes:"));
        assert!(line.contains("1 shared packages"));
    }

    #[test]
    fn cli_version_comparison_line_unavailable() {
        let line = cli_version_comparison_line(None, 447);
        assert!(line.contains("Version comparison:"));
        assert!(line.contains("data unavailable"));
    }

    #[test]
    fn cli_degraded_line_no_baseline() {
        let line = cli_degraded_line(true);
        assert!(line.contains("skipped (--no-baseline)"));
    }

    #[test]
    fn cli_degraded_line_unavailable() {
        let line = cli_degraded_line(false);
        assert!(line.contains("unavailable"));
    }
```

- [ ] **Step 2: Implement CLI formatting helpers**

Add to `baseline_fmt.rs`:

```rust
/// CLI provenance lines printed immediately after baseline extraction.
///
/// Returns two indented lines: package count and resolution strategy.
pub fn cli_provenance_lines(
    target_image: &TargetImageIdentity,
    baseline: &BaselineData,
) -> Vec<String> {
    vec![
        format!("  Baseline extracted: {} packages", baseline.packages.len()),
        format!("  Resolved via: {}", strategy_label(&target_image.strategy)),
    ]
}

/// CLI version comparison line printed after collection completes.
pub fn cli_version_comparison_line(
    version_changes: Option<&[VersionChange]>,
    baseline_count: usize,
) -> String {
    match version_changes {
        None => "  Version comparison: data unavailable".to_string(),
        Some(_) => format!(
            "  Version changes: {}",
            version_comparison_summary(version_changes, baseline_count)
        ),
    }
}

/// CLI degraded-mode status line.
pub fn cli_degraded_line(no_baseline: bool) -> String {
    if no_baseline {
        "  Baseline: skipped (--no-baseline)".to_string()
    } else {
        "  Baseline: unavailable".to_string()
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-pipeline cli_provenance -- --nocapture && cargo test -p inspectah-pipeline cli_version -- --nocapture && cargo test -p inspectah-pipeline cli_degraded -- --nocapture`
Expected: all PASS

- [ ] **Step 4: Wire CLI helpers into `scan.rs`**

In `scan.rs`, add the import:
```rust
use inspectah_pipeline::render::baseline_fmt;
```

After the existing `eprintln!("Extracting baseline... {} packages", data.packages.len());` line (around line 168), add the provenance block:

```rust
            // Provenance confirmation
            if let Some(ref ti) = target_image {
                for line in baseline_fmt::cli_provenance_lines(ti, &data) {
                    eprintln!("{line}");
                }
            }
```

In the `--no-baseline` branch (the `_ => None` match arm for baseline_data, around line 170), add the degraded line:

```rust
        _ => {
            if target_image.is_some() {
                eprintln!("{}", baseline_fmt::cli_degraded_line(args.no_baseline));
            }
            None
        }
```

After `eprintln!("Scanning host {hostname}... done");` (around line 191), add the version comparison:

```rust
    // Version comparison (requires collection results)
    if let (Some(ref bl), Some(ref rpm)) = (&baseline_data, &collected.snapshot.rpm) {
        eprintln!(
            "{}",
            baseline_fmt::cli_version_comparison_line(
                Some(&rpm.version_changes),
                bl.packages.len(),
            )
        );
    } else if baseline_data.is_some() {
        eprintln!(
            "{}",
            baseline_fmt::cli_version_comparison_line(None, 0)
        );
    }
```

- [ ] **Step 5: Build and verify**

Run: `cargo build -p inspectah-cli && cargo clippy -p inspectah-cli -- -W clippy::all`
Expected: compiles clean, no new clippy warnings

- [ ] **Step 6: Commit**

```bash
git add inspectah-pipeline/src/render/baseline_fmt.rs inspectah-cli/src/commands/scan.rs
git commit -m "feat(cli): add baseline provenance and version comparison output

Prints provenance block (package count, resolution strategy) after
extraction, and version comparison line after collection. Shows
degraded status when --no-baseline or baseline unavailable.

Assisted-by: Claude Code (Opus)"
```

---

### Task 5: Executor `run_with_line_callback` Method

**Files:**
- Modify: `inspectah-core/src/traits/executor.rs`
- Modify: `inspectah-collect/src/executor/real.rs`
- Modify: `inspectah-collect/src/executor/mock.rs`

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
    /// Default implementation falls back to `run_passthrough_stderr()` and
    /// calls the callback with each line of captured stderr after completion.
    fn run_with_line_callback(
        &self,
        cmd: &str,
        args: &[&str],
        on_stderr_line: &mut dyn FnMut(&str),
    ) -> ExecResult {
        let result = self.run_passthrough_stderr(cmd, args);
        for line in result.stderr.lines() {
            on_stderr_line(line);
        }
        result
    }
```

- [ ] **Step 2: Build to verify trait is valid**

Run: `cargo build -p inspectah-core`
Expected: compiles — default impl means no downstream breakage.

- [ ] **Step 3: Implement in MockExecutor**

In `mock.rs`, add inside the `impl Executor for MockExecutor` block:

```rust
    fn run_with_line_callback(
        &self,
        cmd: &str,
        args: &[&str],
        on_stderr_line: &mut dyn FnMut(&str),
    ) -> ExecResult {
        let result = self.run(cmd, args);
        for line in result.stderr.lines() {
            on_stderr_line(line);
        }
        result
    }
```

- [ ] **Step 4: Write mock executor test**

Add to `mock.rs` tests:

```rust
    #[test]
    fn test_mock_line_callback() {
        let mock = MockExecutor::new().with_command(
            "podman pull test:latest",
            ExecResult {
                stderr: "Copying blob sha256:aaa... done\nCopying blob sha256:bbb... done\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        );
        let mut lines = Vec::new();
        let result = mock.run_with_line_callback(
            "podman",
            &["pull", "test:latest"],
            &mut |line| lines.push(line.to_string()),
        );
        assert_eq!(result.exit_code, 0);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("aaa"));
        assert!(lines[1].contains("bbb"));
    }
```

- [ ] **Step 5: Run test**

Run: `cargo test -p inspectah-collect test_mock_line_callback -- --nocapture`
Expected: PASS

- [ ] **Step 6: Implement in RealExecutor**

In `real.rs`, add inside `impl Executor for RealExecutor`:

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
                    stderr: format!("failed to spawn {cmd}: {e}"),
                    exit_code: -1,
                    ..Default::default()
                };
            }
        };

        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();

        // Use the same 600s pull timeout as run_passthrough_stderr.
        let pull_timeout = Duration::from_secs(600);

        std::thread::scope(|s| {
            let stdout_thread = s.spawn(|| {
                let mut buf = Vec::new();
                if let Some(mut r) = stdout_handle {
                    let mut limited = io::Read::take(r, STDOUT_SIZE_CAP as u64 + 1);
                    let _ = io::Read::read_to_end(&mut limited, &mut buf);
                }
                buf
            });

            // Read stderr line-by-line, call callback, accumulate full transcript.
            let stderr_thread = s.spawn(|| {
                let mut full_stderr = String::new();
                if let Some(r) = stderr_handle {
                    let reader = io::BufReader::new(r);
                    use io::BufRead;
                    for line_result in reader.lines() {
                        match line_result {
                            Ok(line) => {
                                on_stderr_line(&line);
                                full_stderr.push_str(&line);
                                full_stderr.push('\n');
                            }
                            Err(_) => break,
                        }
                    }
                }
                full_stderr
            });

            match child.wait_timeout(pull_timeout) {
                Ok(Some(status)) => {
                    let stdout_raw = stdout_thread.join().unwrap();
                    let stderr_text = stderr_thread.join().unwrap();
                    let stdout = if stdout_raw.len() > STDOUT_SIZE_CAP {
                        String::from_utf8_lossy(&stdout_raw[..STDOUT_SIZE_CAP]).into_owned()
                    } else {
                        String::from_utf8_lossy(&stdout_raw).into_owned()
                    };
                    ExecResult {
                        stdout,
                        stderr: stderr_text,
                        exit_code: status.code().unwrap_or(-1),
                    }
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_thread.join();
                    let _ = stderr_thread.join();
                    ExecResult {
                        stderr: format!(
                            "command timed out after {}s: {} {}",
                            pull_timeout.as_secs(),
                            cmd,
                            args.join(" ")
                        ),
                        exit_code: -1,
                        ..Default::default()
                    }
                }
                Err(e) => {
                    let _ = stdout_thread.join();
                    let _ = stderr_thread.join();
                    ExecResult {
                        stderr: format!("failed to wait on child process: {e}"),
                        exit_code: -1,
                        ..Default::default()
                    }
                }
            }
        })
    }
```

**Note:** `on_stderr_line` is called from a scoped thread. This requires the callback to be `Send`. Since `&mut dyn FnMut(&str)` is not `Send`, the callback must be called from the main thread or the implementation must be adjusted. The simplest approach: read stderr on the main thread (blocking), and use a timeout thread for the kill. Alternatively, accumulate lines in the thread and call the callback after joining. Since the spec says "full stderr always captured regardless of callback," accumulating and calling post-hoc is acceptable for the initial implementation — live callbacks can be added when the viewport is wired up.

**Revised approach** — accumulate stderr in thread, call callback after join:

Replace the `stderr_thread` spawn with a plain `BufRead` accumulator (same pattern as `run`), then after joining, iterate lines and call the callback:

```rust
            // ... after joining threads and getting stderr_text:
            for line in stderr_text.lines() {
                on_stderr_line(line);
            }
```

This preserves the 600s timeout and full stderr capture. Live streaming comes in a follow-up when the viewport needs it.

- [ ] **Step 7: Build and run tests**

Run: `cargo build -p inspectah-collect && cargo test -p inspectah-collect test_mock_line_callback -- --nocapture`
Expected: compiles, test passes

- [ ] **Step 8: Commit**

```bash
git add inspectah-core/src/traits/executor.rs inspectah-collect/src/executor/real.rs inspectah-collect/src/executor/mock.rs
git commit -m "feat(executor): add run_with_line_callback method

Object-safe Executor trait method for streaming stderr access.
Preserves 600s pull timeout and full stderr capture. Mock executor
splits pre-recorded stderr and calls callback per-line.

Assisted-by: Claude Code (Opus)"
```

---

### Task 6: Update `extract_baseline` Signature and Switch to Callback

**Files:**
- Modify: `inspectah-collect/src/baseline.rs`
- Modify: `inspectah-collect/tests/baseline_test.rs`
- Modify: `inspectah-cli/src/commands/scan.rs`

- [ ] **Step 1: Update `extract_baseline` signature**

In `baseline.rs`, change:
```rust
pub fn extract_baseline(
    executor: &dyn Executor,
    normalized_ref: &NormalizedImageRef,
) -> Result<BaselineData, ExtractionError> {
```
To:
```rust
pub fn extract_baseline(
    executor: &dyn Executor,
    normalized_ref: &NormalizedImageRef,
    on_pull_line: &mut dyn FnMut(&str),
) -> Result<BaselineData, ExtractionError> {
```

- [ ] **Step 2: Switch pull step from `run_nsenter_passthrough` to `run_with_line_callback`**

Replace:
```rust
    let pull_result = run_nsenter_passthrough(executor, &["podman", "pull", image_ref]);
```
With:
```rust
    let pull_result = run_nsenter_with_callback(executor, &["podman", "pull", image_ref], on_pull_line);
```

Add the new helper function:
```rust
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

- [ ] **Step 3: Update all callers**

In `scan.rs`, change:
```rust
            let data = inspectah_collect::baseline::extract_baseline(&executor, norm)
```
To:
```rust
            let mut on_pull_line: Box<dyn FnMut(&str)> = Box::new(|_| {});
            let data = inspectah_collect::baseline::extract_baseline(&executor, norm, &mut *on_pull_line)
```

In `baseline_test.rs`, update all `extract_baseline` calls to pass `&mut |_| {}` as the third argument.

- [ ] **Step 4: Build and run tests**

Run: `cargo build && cargo test -p inspectah-collect baseline -- --nocapture`
Expected: compiles, all baseline tests pass

- [ ] **Step 5: Commit**

```bash
git add inspectah-collect/src/baseline.rs inspectah-collect/tests/baseline_test.rs inspectah-cli/src/commands/scan.rs
git commit -m "refactor(baseline): add on_pull_line callback to extract_baseline

Always uses run_with_line_callback for the pull step, preserving
the 600s timeout. CLI passes a no-op callback for now; viewport
rendering will be wired in the next task.

Assisted-by: Claude Code (Opus)"
```

---

### Task 7: Pull Viewport and Non-TTY Passthrough

**Files:**
- Create: `inspectah-cli/src/pull_progress.rs`
- Modify: `inspectah-cli/src/commands/scan.rs`
- Modify: `inspectah-cli/src/main.rs` (or `lib.rs` — register module)

This is the riskiest task. Everything above works without it.

- [ ] **Step 1: Write failing tests for ANSI stripping**

Create `inspectah-cli/src/pull_progress.rs`:

```rust
//! Pull progress display: TTY viewport and non-TTY passthrough.

/// Strip ANSI escape sequences from a string.
pub fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_escape = false;
    for ch in s.chars() {
        if in_escape {
            if ch.is_ascii_alphabetic() {
                in_escape = false;
            }
            continue;
        }
        if ch == '\x1b' {
            in_escape = true;
            continue;
        }
        result.push(ch);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_no_escapes() {
        assert_eq!(strip_ansi("hello world"), "hello world");
    }

    #[test]
    fn strip_ansi_color_codes() {
        assert_eq!(strip_ansi("\x1b[32mgreen\x1b[0m"), "green");
    }

    #[test]
    fn strip_ansi_cursor_movement() {
        assert_eq!(strip_ansi("\x1b[3Atext"), "text");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p inspectah-cli strip_ansi -- --nocapture`
Expected: PASS

- [ ] **Step 3: Write failing tests for layer counting**

Add to `pull_progress.rs`:

```rust
/// Count completed blob transfers from podman pull stderr lines.
///
/// Looks for lines matching `Copying blob ... done` or `... skipped`.
/// Returns the count, or None if no blob lines were found.
pub fn count_completed_blobs(stderr_lines: &[String]) -> Option<usize> {
    let count = stderr_lines
        .iter()
        .filter(|l| {
            let stripped = strip_ansi(l);
            stripped.contains("Copying blob")
                && (stripped.ends_with("done") || stripped.ends_with("skipped"))
        })
        .count();
    if count == 0 { None } else { Some(count) }
}
```

Tests:

```rust
    #[test]
    fn count_blobs_normal() {
        let lines = vec![
            "Copying blob sha256:aaa... done".into(),
            "Copying blob sha256:bbb... done".into(),
            "Copying blob sha256:ccc... skipped".into(),
        ];
        assert_eq!(count_completed_blobs(&lines), Some(3));
    }

    #[test]
    fn count_blobs_with_progress() {
        let lines = vec![
            "Copying blob sha256:aaa... 42 MiB / 89 MiB".into(),
            "Copying blob sha256:aaa... done".into(),
        ];
        assert_eq!(count_completed_blobs(&lines), Some(1));
    }

    #[test]
    fn count_blobs_empty() {
        let lines: Vec<String> = vec!["Writing manifest".into()];
        assert_eq!(count_completed_blobs(&lines), None);
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-cli count_blobs -- --nocapture`
Expected: PASS

- [ ] **Step 5: Write failing test for pull summary line**

```rust
/// Format the pull summary line shown after pull completes.
pub fn pull_summary_line(image_ref: &str, digest: &str, blob_count: Option<usize>) -> String {
    let short_digest = if digest.len() > 19 {
        &digest[..19]
    } else {
        digest
    };
    match blob_count {
        Some(n) => format!("Pulled {image_ref} ({n} layers, {short_digest})"),
        None => format!("Pulled {image_ref} ({short_digest})"),
    }
}
```

Test:

```rust
    #[test]
    fn pull_summary_with_layers() {
        let line = pull_summary_line("quay.io/test:latest", "sha256:abc123def456789", Some(7));
        assert!(line.contains("7 layers"));
        assert!(line.contains("sha256:abc123def45678"));
    }

    #[test]
    fn pull_summary_without_layers() {
        let line = pull_summary_line("quay.io/test:latest", "sha256:abc123", None);
        assert!(!line.contains("layers"));
        assert!(line.contains("sha256:abc123"));
    }
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p inspectah-cli pull_summary -- --nocapture`
Expected: PASS

- [ ] **Step 7: Implement viewport and non-TTY callback builders**

Add to `pull_progress.rs`:

```rust
use std::io::Write;

/// Build a pull progress callback for non-TTY (CI/pipes).
///
/// Prints each stderr line with a `  pull: ` prefix.
pub fn non_tty_callback(stderr: &mut dyn Write) -> impl FnMut(&str) + '_ {
    move |line: &str| {
        let _ = writeln!(stderr, "  pull: {}", strip_ansi(line));
    }
}

/// Build a pull progress callback for TTY with a 3-line viewport.
///
/// Maintains a ring buffer of the last 3 lines, redraws with box borders.
/// Call `viewport_cleanup` after the pull completes to clear the box.
pub fn tty_viewport_callback(
    stderr: &mut dyn Write,
    width: usize,
) -> (impl FnMut(&str) + '_, Vec<String>) {
    let content_width = width.saturating_sub(6).min(66); // box borders + padding
    let mut buffer: Vec<String> = Vec::new();
    let mut line_count: usize = 0;
    let collected = Vec::new();

    let callback = move |line: &str| {
        let cleaned = strip_ansi(line);
        let truncated = if cleaned.len() > content_width {
            format!("{}…", &cleaned[..content_width - 1])
        } else {
            cleaned
        };

        buffer.push(truncated);
        if buffer.len() > 3 {
            buffer.remove(0);
        }

        // Move cursor up to redraw
        if line_count > 0 {
            let up = buffer.len().min(3) + 2; // content + borders
            let _ = write!(stderr, "\x1b[{up}A");
        }

        // Draw box
        let bar = "─".repeat(content_width + 2);
        let _ = writeln!(stderr, "  ┌{bar}┐");
        for row in &buffer {
            let padded = format!("{:width$}", row, width = content_width);
            let _ = writeln!(stderr, "  │ {padded} │");
        }
        // Pad empty rows
        for _ in buffer.len()..3 {
            let padded = " ".repeat(content_width);
            let _ = writeln!(stderr, "  │ {padded} │");
        }
        let _ = writeln!(stderr, "  └{bar}┘");
        let _ = stderr.flush();
        line_count += 1;
    };

    (callback, collected)
}

/// Clear the viewport after pull completes.
///
/// Moves cursor up past the box (5 lines: top border + 3 content + bottom border)
/// and clears each line.
pub fn viewport_cleanup(stderr: &mut dyn Write) {
    let _ = write!(stderr, "\x1b[5A"); // up past box
    for _ in 0..5 {
        let _ = write!(stderr, "\x1b[2K\n"); // clear line, move down
    }
    let _ = write!(stderr, "\x1b[5A"); // back to top of cleared area
    let _ = stderr.flush();
}
```

- [ ] **Step 8: Wire into `scan.rs`**

Replace the no-op callback in `scan.rs` with TTY/non-TTY dispatch:

```rust
            use std::io::IsTerminal;

            let is_tty = std::io::stderr().is_terminal();
            let term_width = if is_tty {
                terminal_size::terminal_size()
                    .map(|(w, _)| w.0 as usize)
                    .unwrap_or(80)
            } else {
                80
            };

            // Use viewport for TTY >= 40 cols, passthrough otherwise
            let use_viewport = is_tty && term_width >= 40;

            eprintln!("Pulling {}...", norm.as_str());

            let mut collected_lines: Vec<String> = Vec::new();
            let mut stderr_handle = std::io::stderr();

            let mut callback: Box<dyn FnMut(&str)> = if use_viewport {
                Box::new(|line: &str| {
                    // Viewport rendering — simplified for initial implementation.
                    // Full viewport with box drawing can be iterated on.
                    let cleaned = pull_progress::strip_ansi(line);
                    collected_lines.push(cleaned);
                })
            } else {
                Box::new(|line: &str| {
                    let cleaned = pull_progress::strip_ansi(line);
                    eprintln!("  pull: {}", cleaned);
                    collected_lines.push(cleaned.to_string());
                })
            };

            let data = inspectah_collect::baseline::extract_baseline(
                &executor, norm, &mut *callback,
            ).context("baseline extraction failed")?;

            let blob_count = pull_progress::count_completed_blobs(&collected_lines);
            eprintln!(
                "{}",
                pull_progress::pull_summary_line(
                    norm.as_str(),
                    &data.image_digest,
                    blob_count,
                )
            );
```

**Note:** The full TTY viewport with box-drawing and cursor movement is complex to get right in the initial pass. The above wires the infrastructure (callback, collected lines, summary line). The visual viewport can be iterated on — the non-TTY passthrough and summary line already deliver the core value.

- [ ] **Step 9: Add `terminal_size` dependency if needed**

Run: `cargo add terminal_size --manifest-path inspectah-cli/Cargo.toml`

Or add manually to `inspectah-cli/Cargo.toml`:
```toml
terminal_size = "0.4"
```

- [ ] **Step 10: Register module and build**

Add `mod pull_progress;` to `inspectah-cli/src/main.rs` (or `lib.rs`, whichever hosts the module tree).

Run: `cargo build -p inspectah-cli && cargo clippy -p inspectah-cli -- -W clippy::all`
Expected: compiles, clean clippy

- [ ] **Step 11: Run all tests**

Run: `cargo test -p inspectah-cli && cargo test -p inspectah-collect && cargo test -p inspectah-pipeline`
Expected: all pass

- [ ] **Step 12: Commit**

```bash
git add inspectah-cli/src/pull_progress.rs inspectah-cli/src/main.rs inspectah-cli/src/commands/scan.rs inspectah-cli/Cargo.toml inspectah-collect/src/baseline.rs inspectah-collect/tests/baseline_test.rs
git commit -m "feat(cli): add pull progress display and baseline provenance

TTY: viewport placeholder with collected lines and summary.
Non-TTY: prefixed line passthrough for CI liveness.
Both: pull summary line with image ref, digest, and layer count.
Provenance block and version comparison output from shared helpers.

Assisted-by: Claude Code (Opus)"
```

---

## Implementation Notes

**Task dependency graph:**
```
Task 1 (helpers) → Task 2 (README) → Task 3 (audit + parity)
Task 1 (helpers) → Task 4 (CLI output)
Task 5 (executor) → Task 6 (extract_baseline sig) → Task 7 (viewport)
```

Tasks 1-4 and Task 5 are independent and can run in parallel. Task 6 depends on Task 5. Task 7 depends on Tasks 4 and 6.

**Ship points:** Tasks 1-3 can ship independently (rendered artifacts). Task 4 adds CLI output. Tasks 5-7 are the viewport plumbing.

**Risks:**
- Task 7's viewport rendering involves cursor math that's hard to test without a real terminal. The plan starts with a simplified implementation and iterates.
- The `run_with_line_callback` real executor implementation may need adjustment for the scoped-thread + callback ownership pattern. The plan notes this and provides a simpler accumulate-then-call approach.
