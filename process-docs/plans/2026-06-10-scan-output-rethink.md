# Scan Output Rethink — Implementation Plan (rev 3)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the block-redraw scan progress output with an append-only streaming receipt, collapse three progress modes to two (pretty/flat), and add a findings summary block.

**Architecture:** The progress rendering pipeline is refactored in three layers: (1) a shared data model (`ReceiptLine`, `ScanEndState`, `ScanSummary`) consumed by both renderers — illegal end-state combinations are unrepresentable at the type level, (2) a new `PrettyRenderer` that replaces both `RichRenderer` and `PlainRenderer`, and (3) updates to `FlatRenderer` to use the shared model and respect verbosity. The current `print_completion()` in `scan.rs` is absorbed into the renderer's finalize path via `ScanEndState`.

**Tech Stack:** Rust, crossterm (ANSI), insta (snapshot tests)

**Spec:** `process-docs/specs/proposed/2026-06-10-scan-output-rethink.md`

**Ordering model:** Arrival order — inspectors print as they complete, no display-order buffering. Only per-inspector verbose child lines are buffered atomically with their parent.

**Build invariant:** Every task commit must leave the repo in a buildable, test-passing state. No task may break compilation for a subsequent task to fix.

---

## File Map

| Action | File | Purpose |
|--------|------|---------|
| Create | `crates/cli/src/progress/receipt.rs` | Shared types: `ReceiptLine`, `ScanEndState`, `ScanSummary`, `InspectorState`, formatting |
| Create | `crates/cli/src/progress/pretty.rs` | New `PrettyRenderer` (append-only, safety valve spinner, summary, footer) |
| Modify | `crates/cli/src/progress/flat.rs` | Use shared model, respect verbosity, dynamic inspector count, summary, footer |
| Modify | `crates/cli/src/progress/display.rs` | Extend `DISPLAY_ORDER` with conditional `Subscription` entry |
| Modify | `crates/cli/src/progress/mod.rs` | Replace `ProgressMode` enum (`Pretty`/`Flat`), update `TerminalProgress` dispatcher |
| Delete | `crates/cli/src/progress/rich.rs` | Replaced by `pretty.rs` |
| Delete | `crates/cli/src/progress/plain.rs` | Merged into `pretty.rs` |
| Modify | `crates/cli/src/commands/scan.rs` | Remove `print_completion()`, wire `ScanEndState` into renderer, update CLI args |
| Modify | `docs/reference/cli.md` | Update `--progress` and `--verbose` documentation |
| Modify | `docs/reference/configuration.md` | Update `INSPECTAH_PROGRESS` env var docs |
| Modify | `docs/how-to/customize-output.md` | Update progress mode references |
| Modify | `docs/how-to/ci-integration.md` | Update flat mode references |
| Modify | `docs/getting-started.md` | Update output examples if present |
| Modify | `docs/tutorials/first-migration.md` | Update output examples if present |
| Modify | `README.md` | Update any progress mode references |

---

## Task 1: Shared Data Model (`receipt.rs`)

**Owner:** Tang
**Files:**
- Create: `crates/cli/src/progress/receipt.rs`
- Modify: `crates/cli/src/progress/mod.rs` (add `pub mod receipt;`)

- [ ] **Step 1: Create `receipt.rs` with type definitions**

```rust
// crates/cli/src/progress/receipt.rs

//! Shared data model for scan receipt output.
//!
//! Both `PrettyRenderer` and `FlatRenderer` consume these types
//! so their output cannot drift.

use std::path::PathBuf;
use std::time::Duration;

use inspectah_core::types::completeness::InspectorId;

/// Inspector completion state for receipt rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectorState {
    Success,
    Degraded,
    Skipped,
    Failed,
    Interrupted,
}

/// One inspector's receipt line.
#[derive(Debug, Clone)]
pub struct ReceiptLine {
    pub id: InspectorId,
    pub state: InspectorState,
    /// Summary metric (e.g., "613 packages, 6 repos"). None → "done".
    pub metric: Option<String>,
    /// Reason string for non-success states.
    pub reason: Option<String>,
    /// Child lines (Non-RPM breakdown, verbose sub-steps). Plain text, no symbols.
    pub sub_lines: Vec<String>,
    /// Authoritative typed counts for summary aggregation.
    /// Renderers use `metric` for display; `ScanSummary::build()` uses
    /// these counts to construct hotspot lines without parsing strings.
    pub typed_counts: TypedCounts,
}

/// Authoritative numeric counts per inspector — source of truth for
/// summary aggregation. Populated from `MetricKind` events during
/// collection, not parsed from the formatted `metric` string.
#[derive(Debug, Clone, Default)]
pub struct TypedCounts {
    pub configs_modified: Option<usize>,
    pub pip_packages: Option<usize>,
    pub npm_packages: Option<usize>,
    pub gem_packages: Option<usize>,
    pub git_repos: Option<usize>,
    // Extensible: add fields as new inspectors emit counts
}

/// Version change summary — typed, not stringly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionChangeSummary {
    pub total: usize,
    pub target_newer: usize,
    pub host_newer: usize,
}

/// One hotspot line in the findings summary.
#[derive(Debug, Clone)]
pub struct HotspotLine {
    pub segments: Vec<HotspotSegment>,
}

/// A single segment within a hotspot line (typed, not a raw string).
#[derive(Debug, Clone)]
pub struct HotspotSegment {
    pub count: usize,
    pub label: &'static str, // e.g., "modified configs", "pip packages"
}

impl HotspotSegment {
    pub fn format(&self) -> String {
        format!("{} {}", self.count, self.label)
    }
}

impl HotspotLine {
    /// Format segments joined by " · " (pretty) or ", " (flat).
    pub fn format_pretty(&self) -> String {
        self.segments.iter().map(|s| s.format()).collect::<Vec<_>>().join(" · ")
    }

    pub fn format_flat(&self) -> String {
        self.segments.iter().map(|s| s.format()).collect::<Vec<_>>().join(", ")
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
}

/// Non-success tally for the timing line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NonSuccessTally {
    pub failed: usize,
    pub degraded: usize,
    pub skipped: usize,
    pub interrupted: usize,
}

impl NonSuccessTally {
    pub fn is_empty(&self) -> bool {
        self.failed == 0 && self.degraded == 0 && self.skipped == 0 && self.interrupted == 0
    }

    /// Format as parenthetical: "(1 failed, 2 degraded)"
    pub fn format(&self) -> String {
        let mut parts = Vec::new();
        if self.failed > 0 {
            parts.push(format!("{} failed", self.failed));
        }
        if self.degraded > 0 {
            parts.push(format!("{} degraded", self.degraded));
        }
        if self.skipped > 0 {
            parts.push(format!("{} skipped", self.skipped));
        }
        if self.interrupted > 0 {
            parts.push(format!("{} interrupted", self.interrupted));
        }
        format!("({})", parts.join(", "))
    }
}

/// Scan end state — mutually exclusive outcomes.
/// Illegal combinations (e.g., interrupted with a report path) are
/// unrepresentable.
#[derive(Debug, Clone)]
pub enum ScanEndState {
    /// Normal scan: tarball written to disk.
    Completed {
        path: PathBuf,
        sensitivity: Option<String>,
    },
    /// `--inspect-only` with explicit `--output <path>`.
    InspectOnly {
        path: PathBuf,
    },
    /// `--inspect-only` without `--output` — JSON went to stdout.
    InspectOnlyStdout,
    /// Tarball or inspect-only write failed.
    WriteFailure {
        error: String,
    },
    /// SIGINT interrupted the scan before all inspectors finished.
    Interrupted {
        completed: usize,
        total: usize,
    },
}

/// Wrapper passed to `finalize()` — shared fields + end state.
#[derive(Debug, Clone)]
pub struct ScanFinalize {
    pub elapsed: Duration,
    pub end_state: ScanEndState,
    pub version_changes: Option<VersionChangeSummary>,
}

/// Aggregate scan summary — computed from receipt lines.
#[derive(Debug, Clone)]
pub struct ScanSummary {
    pub version_changes: Option<VersionChangeSummary>,
    pub hotspots: Vec<HotspotLine>,
    pub non_success_tally: NonSuccessTally,
}

impl ScanSummary {
    /// Build summary from receipt lines and version change data.
    pub fn build(
        lines: &[ReceiptLine],
        version_changes: Option<VersionChangeSummary>,
    ) -> Self {
        let mut tally = NonSuccessTally::default();
        for line in lines {
            match line.state {
                InspectorState::Failed => tally.failed += 1,
                InspectorState::Degraded => tally.degraded += 1,
                InspectorState::Skipped => tally.skipped += 1,
                InspectorState::Interrupted => tally.interrupted += 1,
                InspectorState::Success => {}
            }
        }

        // Build hotspot line from authoritative TypedCounts — no string parsing.
        let mut hotspot_segments = Vec::new();
        for line in lines {
            let tc = &line.typed_counts;
            if let Some(c) = tc.configs_modified {
                if c > 0 { hotspot_segments.push(HotspotSegment { count: c, label: "modified configs" }); }
            }
            if let Some(c) = tc.pip_packages {
                if c > 0 { hotspot_segments.push(HotspotSegment { count: c, label: "pip packages" }); }
            }
            if let Some(c) = tc.npm_packages {
                if c > 0 { hotspot_segments.push(HotspotSegment { count: c, label: "npm packages" }); }
            }
            if let Some(c) = tc.gem_packages {
                if c > 0 { hotspot_segments.push(HotspotSegment { count: c, label: "gem packages" }); }
            }
            if let Some(c) = tc.git_repos {
                if c > 0 { hotspot_segments.push(HotspotSegment { count: c, label: "git repos" }); }
            }
        }

        let hotspots = if hotspot_segments.is_empty() {
            Vec::new()
        } else {
            vec![HotspotLine { segments: hotspot_segments }]
        };

        Self {
            version_changes,
            hotspots,
            non_success_tally: tally,
        }
    }

    pub fn has_content(&self) -> bool {
        self.version_changes.is_some() || !self.hotspots.is_empty()
    }
}

impl InspectorState {
    /// Pretty-mode symbol.
    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Success => "✓",
            Self::Degraded => "⚠",
            Self::Skipped => "○",
            Self::Failed => "✗",
            Self::Interrupted => "△",
        }
    }

    /// Flat-mode text equivalent.
    pub fn flat_label(&self) -> &'static str {
        match self {
            Self::Success => "ok",
            Self::Degraded => "WARN",
            Self::Skipped => "skip",
            Self::Failed => "FAIL",
            Self::Interrupted => "INT",
        }
    }

    /// ANSI color code.
    pub fn color_code(&self) -> &'static str {
        match self {
            Self::Success => "\x1b[32m",
            Self::Degraded => "\x1b[33m",
            Self::Skipped => "\x1b[2m",
            Self::Failed => "\x1b[31m",
            Self::Interrupted => "\x1b[33m",
        }
    }
}

impl VersionChangeSummary {
    pub fn format(&self) -> String {
        format!(
            "{} version changes ({} target-newer, {} host-newer)",
            self.total, self.target_newer, self.host_newer
        )
    }
}

// No string-parsing helpers needed — TypedCounts provides authoritative data.
```

- [ ] **Step 2: Add module to `mod.rs`**

Add `pub mod receipt;` to `crates/cli/src/progress/mod.rs`.

- [ ] **Step 3: Write unit tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_tally_is_empty() {
        assert!(NonSuccessTally::default().is_empty());
    }

    #[test]
    fn tally_format_single() {
        let t = NonSuccessTally { failed: 1, ..Default::default() };
        assert_eq!(t.format(), "(1 failed)");
    }

    #[test]
    fn tally_format_multiple_preserves_order() {
        let t = NonSuccessTally { failed: 1, degraded: 2, skipped: 0, interrupted: 3 };
        assert_eq!(t.format(), "(1 failed, 2 degraded, 3 interrupted)");
    }

    #[test]
    fn state_symbols() {
        assert_eq!(InspectorState::Success.symbol(), "✓");
        assert_eq!(InspectorState::Failed.symbol(), "✗");
        assert_eq!(InspectorState::Interrupted.symbol(), "△");
    }

    #[test]
    fn state_flat_labels() {
        assert_eq!(InspectorState::Success.flat_label(), "ok");
        assert_eq!(InspectorState::Failed.flat_label(), "FAIL");
        assert_eq!(InspectorState::Interrupted.flat_label(), "INT");
    }

    #[test]
    fn version_change_summary_format() {
        let v = VersionChangeSummary { total: 58, target_newer: 54, host_newer: 4 };
        assert_eq!(v.format(), "58 version changes (54 target-newer, 4 host-newer)");
    }

    #[test]
    fn hotspot_line_pretty_format() {
        let line = HotspotLine {
            segments: vec![
                HotspotSegment { count: 37, label: "modified configs" },
                HotspotSegment { count: 23, label: "pip packages" },
            ],
        };
        assert_eq!(line.format_pretty(), "37 modified configs · 23 pip packages");
    }

    #[test]
    fn hotspot_line_flat_format() {
        let line = HotspotLine {
            segments: vec![
                HotspotSegment { count: 37, label: "modified configs" },
                HotspotSegment { count: 23, label: "pip packages" },
            ],
        };
        assert_eq!(line.format_flat(), "37 modified configs, 23 pip packages");
    }

    #[test]
    fn summary_omits_zero_version_changes() {
        let summary = ScanSummary::build(&[], None);
        assert!(summary.version_changes.is_none());
        assert!(!summary.has_content());
    }

    #[test]
    fn summary_counts_non_success() {
        let lines = vec![
            ReceiptLine {
                id: InspectorId::Rpm,
                state: InspectorState::Success,
                metric: None, reason: None, sub_lines: vec![],
            },
            ReceiptLine {
                id: InspectorId::Containers,
                state: InspectorState::Failed,
                metric: None, reason: Some("podman not found".into()), sub_lines: vec![],
            },
            ReceiptLine {
                id: InspectorId::Config,
                state: InspectorState::Degraded,
                metric: Some("37 modified".into()), reason: Some("rpm verify timed out".into()),
                sub_lines: vec![],
            },
        ];
        let summary = ScanSummary::build(&lines, None);
        assert_eq!(summary.non_success_tally.failed, 1);
        assert_eq!(summary.non_success_tally.degraded, 1);
    }

    #[test]
    fn summary_uses_typed_counts_not_strings() {
        let lines = vec![
            ReceiptLine {
                id: InspectorId::Config,
                state: InspectorState::Success,
                metric: Some("37 modified".into()),
                reason: None, sub_lines: vec![],
                typed_counts: TypedCounts { configs_modified: Some(37), ..Default::default() },
            },
            ReceiptLine {
                id: InspectorId::NonRpmSoftware,
                state: InspectorState::Success,
                metric: Some("2 ecosystems".into()),
                reason: None,
                sub_lines: vec!["pip 23 · npm 69".into()],
                typed_counts: TypedCounts {
                    pip_packages: Some(23), npm_packages: Some(69), ..Default::default()
                },
            },
        ];
        let summary = ScanSummary::build(&lines, None);
        assert_eq!(summary.hotspots.len(), 1);
        assert_eq!(summary.hotspots[0].segments.len(), 3);
        assert_eq!(summary.hotspots[0].segments[0].count, 37);
        assert_eq!(summary.hotspots[0].segments[1].count, 23);
        assert_eq!(summary.hotspots[0].segments[2].count, 69);
    }

    #[test]
    fn scan_end_state_exhaustive() {
        // Compile-time proof: all variants are matchable.
        let states = vec![
            ScanEndState::Completed { path: PathBuf::from("/tmp/test"), sensitivity: None },
            ScanEndState::InspectOnly { path: PathBuf::from("/tmp/test") },
            ScanEndState::InspectOnlyStdout,
            ScanEndState::WriteFailure { error: "disk full".into() },
            ScanEndState::Interrupted { completed: 5, total: 11 },
        ];
        for state in &states {
            match state {
                ScanEndState::Completed { .. } => {},
                ScanEndState::InspectOnly { .. } => {},
                ScanEndState::InspectOnlyStdout => {},
                ScanEndState::WriteFailure { .. } => {},
                ScanEndState::Interrupted { .. } => {},
            }
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-cli receipt -- --nocapture`
Expected: all pass

- [ ] **Step 5: Commit**

```
feat(progress): add shared receipt data model with typed end states

Assisted-by: Claude Code (Opus 4.6)
```

---

## Task 2: Extend `DISPLAY_ORDER` for Subscription

**Owner:** Tang
**Files:**
- Modify: `crates/cli/src/progress/display.rs`

- [ ] **Step 1: Write failing test for subscription display position**

```rust
#[test]
fn display_position_subscription() {
    assert_eq!(display_position(InspectorId::Subscription), 12);
}

#[test]
fn display_name_subscription() {
    assert_eq!(display_name(InspectorId::Subscription), "Subscription");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-cli display -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Add Subscription to DISPLAY_ORDER and add `active_display_order()` helper**

```rust
pub const DISPLAY_ORDER: &[(InspectorId, &str)] = &[
    (InspectorId::Rpm, "RPM packages"),
    (InspectorId::Services, "Services"),
    (InspectorId::Storage, "Storage"),
    (InspectorId::KernelBoot, "Kernel & boot"),
    (InspectorId::Network, "Network"),
    (InspectorId::Containers, "Containers"),
    (InspectorId::UsersGroups, "Users & groups"),
    (InspectorId::ScheduledTasks, "Scheduled tasks"),
    (InspectorId::Config, "Config files"),
    (InspectorId::Selinux, "SELinux"),
    (InspectorId::NonRpmSoftware, "Non-RPM packages"),
    (InspectorId::Subscription, "Subscription"),
];

pub fn active_display_order(has_subscription: bool) -> &'static [(InspectorId, &'static str)] {
    if has_subscription {
        DISPLAY_ORDER
    } else {
        &DISPLAY_ORDER[..11]
    }
}
```

- [ ] **Step 4: Update existing count test + add new tests**

```rust
#[test]
fn display_order_has_12_entries() {
    assert_eq!(DISPLAY_ORDER.len(), 12);
}

#[test]
fn active_display_order_without_subscription() {
    assert_eq!(active_display_order(false).len(), 11);
}

#[test]
fn active_display_order_with_subscription() {
    assert_eq!(active_display_order(true).len(), 12);
}
```

- [ ] **Step 5: Run all display tests**

Run: `cargo test -p inspectah-cli display -- --nocapture`
Expected: all pass

- [ ] **Step 6: Commit**

```
feat(progress): extend DISPLAY_ORDER with conditional Subscription entry

Assisted-by: Claude Code (Opus 4.6)
```

---

## Task 3: `PrettyRenderer` — Core Receipt Output

**Owner:** Tang
**Files:**
- Create: `crates/cli/src/progress/pretty.rs`
- Modify: `crates/cli/src/progress/mod.rs` (add `pub mod pretty;`)

This task builds the core append-only receipt renderer WITHOUT the safety
valve spinner (Task 4) and WITHOUT summary/footer (Task 5). It produces
correct receipt lines for all scans using arrival-order output.

- [ ] **Step 1: Create `pretty.rs` with state tracking and event handling**

The renderer must:
- Accept `ProgressEvent`s from the pipeline
- Build `ReceiptLine` per inspector from events
- Print each inspector's line immediately when it finishes (arrival order)
- Track per-inspector metrics, sub-steps (for verbose), and probe results
- In verbose mode, buffer child lines per inspector and print atomically
  with the parent (the only buffering in the system)

Key implementation decisions:
- Internal state tracks per-inspector: `started_at`, metric accumulator,
  sub-step buffer, probe buffer, outcome, `TypedCounts`
- On `InspectorFinished`: build `ReceiptLine`, print immediately
- Thread safety: `Mutex<PrettyState>` (same pattern as current renderers)
- Constructor receives `active_display_order(has_subscription)` slice
  (used for total count in flat mode, not for ordering)
- No display-order buffering — print on arrival

Structure:
1. `PrettyState` (inner mutable state) — inspector tracking, print logic
2. `PrettyRenderer` (public API) — `new()`, `handle()`, `finalize()`
3. Formatting helpers — `format_receipt_line()`, `format_sub_lines()`

Line format:
```
  ✓ RPM packages               613 packages, 6 repos
```

Column alignment: 2-space indent, name left-aligned, metric right-aligned
to fixed column (28 chars from left margin).

- [ ] **Step 2: Write snapshot tests using `insta`**

Test cases:
- `pretty_normal_11_inspectors` — all 11 success, various metrics
- `pretty_with_subscription` — 12 inspectors, subscription enabled
- `pretty_with_failures` — mix of success, degraded, skipped, failed
- `pretty_with_interrupted` — 5 complete + 6 interrupted
- `pretty_nonrpm_sub_lines` — Non-RPM with ecosystem breakdown
- `pretty_verbose_rpm_substeps` — verbose mode shows RPM sub-steps
- `pretty_no_color` — same events, `use_color: false`
- `pretty_arrival_order` — fast inspectors print before slow RPM
  (events arrive out of display order, output reflects arrival order)
- `pretty_skipped_without_start` — inapplicable inspector, prints ○
- `pretty_verbose_atomic_parent_child` — verbose parent + children
  print as single atomic block, no interleaving across inspectors

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p inspectah-cli pretty -- --nocapture`
Expected: FAIL — module not found

- [ ] **Step 4: Implement `PrettyRenderer`**

Full implementation (~200-300 lines — simpler than rev 2 since no
buffering logic). Key points:
- Constructor takes `active_order`, `use_color`, `verbose`
- `handle()` matches on `ProgressEvent` variants, updates state
- On `InspectorFinished`: immediately build and print `ReceiptLine`.
  In verbose mode, child lines are accumulated during the inspector's
  lifetime and flushed atomically with the parent.
- `finalize()` takes `ScanFinalize` but in this task only handles
  any cleanup. Summary + footer rendering added in Task 5.

- [ ] **Step 5: Run tests, update snapshots**

Run: `cargo test -p inspectah-cli pretty -- --nocapture`
Then: `cargo insta review`
Expected: all pass

- [ ] **Step 6: Commit**

```
feat(progress): add PrettyRenderer with display-order buffering

Assisted-by: Claude Code (Opus 4.6)
```

---

## Task 4: `PrettyRenderer` — Safety Valve Spinner

**Owner:** Tang
**Files:**
- Modify: `crates/cli/src/progress/pretty.rs`

- [ ] **Step 1: Write tests for spinner behavior**

Test the state logic (not real-time output):
- `spinner_triggers_after_threshold` — verify state marks spinner active
  when any inspector exceeds `ELAPSED_THRESHOLD`
- `spinner_replaced_by_result` — verify final output has no spinner residue
- `spinner_interrupted_by_other_completion` — another inspector finishes
  while spinner is active → spinner cancelled, result printed, spinner
  restarted for the still-slow inspector
- `spinner_transfers_to_next_slow` — slow inspector finishes, another
  slow inspector remains → spinner transfers to the longest-running

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-cli pretty::tests::spinner -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Add spinner logic**

Add to `PrettyState`:
- `spinner_active: Option<InspectorId>`
- Background tick thread (100ms, same pattern as current `RichRenderer`):
  1. Check if any running inspector exceeds `ELAPSED_THRESHOLD`
  2. If yes and no spinner active, show spinner for longest-running
  3. If spinner active, redraw with next braille frame

Spinner line: `  ⠋ <Name>               (N.Ns)`

On inspector finish:
1. If that inspector has active spinner, clear line (`\x1b[2K\r`), print result
2. If another inspector is still slow, restart spinner for it
3. If no slow inspectors remain, no restart

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-cli pretty -- --nocapture`
Expected: all pass

- [ ] **Step 5: Commit**

```
feat(progress): add safety valve spinner to PrettyRenderer

Assisted-by: Claude Code (Opus 4.6)
```

---

## Task 5: Findings Summary + Footer (Pretty)

**Owner:** Tang
**Files:**
- Modify: `crates/cli/src/progress/pretty.rs`

This task adds the summary block and footer zone to `PrettyRenderer`'s
`finalize()`. The `ScanSummary::build()` and `ScanEndState` types from
`receipt.rs` (Task 1) are consumed here.

- [ ] **Step 1: Implement summary + footer rendering in `finalize()`**

`finalize(scan: ScanFinalize)` now:
1. Flush any remaining buffered receipt lines
2. Build `ScanSummary` from collected `ReceiptLine`s + `scan.version_changes`
3. If summary has content, print blank line + summary lines
4. Print blank line
5. Print timing line based on `scan.end_state`:

```rust
match &scan.end_state {
    ScanEndState::Completed { path, sensitivity } => {
        let tally = if summary.non_success_tally.is_empty() {
            String::new()
        } else {
            format!(" {}", summary.non_success_tally.format())
        };
        writeln!(w, "  Inspected in {secs:.1}s{tally}");
        writeln!(w, "  Report: {}", path.display());
        writeln!(w, "  To review: inspectah refine {}", path.display());
        if let Some(notice) = sensitivity {
            for line in notice.lines() {
                writeln!(w, "  {line}");
            }
        }
    }
    ScanEndState::InspectOnly { path } => {
        writeln!(w, "  Inspected in {secs:.1}s{tally}");
        writeln!(w, "  Output: {}", path.display());
    }
    ScanEndState::InspectOnlyStdout => {
        writeln!(w, "  Inspected in {secs:.1}s{tally}");
    }
    ScanEndState::WriteFailure { error } => {
        writeln!(w, "  Inspected in {secs:.1}s{tally}");
        writeln!(w, "  Error: {error}");
    }
    ScanEndState::Interrupted { completed, total } => {
        writeln!(w, "  Interrupted after {secs:.1}s ({completed} of {total} inspectors completed)");
    }
}
```

- [ ] **Step 2: Snapshot tests for summary + footer**

- `pretty_summary_with_version_changes` — version changes + hotspots
- `pretty_summary_clean_host` — no summary block
- `pretty_footer_completed` — report path + refine hint
- `pretty_footer_inspect_only` — output path only
- `pretty_footer_inspect_only_stdout` — timing line only
- `pretty_footer_write_failure` — error line
- `pretty_footer_interrupted` — interrupted timing line
- `pretty_footer_completed_with_sensitivity` — sensitivity notice
- `pretty_footer_non_success_tally` — timing line with tally

- [ ] **Step 3: Run tests, accept snapshots**

Run: `cargo test -p inspectah-cli pretty -- --nocapture`
Then: `cargo insta review`
Expected: all pass

- [ ] **Step 4: Commit**

```
feat(progress): add findings summary and typed footer to PrettyRenderer

Assisted-by: Claude Code (Opus 4.6)
```

---

## Task 6: Update `FlatRenderer` — Shared Model + Verbosity + Footer

**Owner:** Tang
**Files:**
- Modify: `crates/cli/src/progress/flat.rs`

T6 depends on T1+T2+T5 (uses `ScanFinalize`, `ScanEndState` from receipt.rs).

- [ ] **Step 1: Write tests for new flat behavior**

- `flat_normal_hides_substeps` — default verbosity, sub-steps suppressed
- `flat_verbose_shows_substeps` — `--verbose`, sub-steps numbered
  `[01/11.1]`, `[01/11.2]`, etc.
- `flat_dynamic_count_12` — subscription enabled, `[N/12]` numbering
- `flat_summary_block` — findings summary with `, ` separators (not ` · `)
- `flat_footer_completed` — timing + path + refine hint
- `flat_footer_interrupted` — interrupted timing line
- `flat_non_success_states` — ok/FAIL/WARN/skip/INT text labels
- `flat_arrival_order` — fast inspectors print before slow RPM
- `flat_skipped_without_start` — inapplicable inspector prints "skip"
- `flat_completion_counter` — `[N/total]` increments per arrival, not
  by display position

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-cli flat -- --nocapture`
Expected: FAIL on new tests

- [ ] **Step 3: Refactor `FlatRenderer`**

Key changes:
- Constructor takes `active_order`, `verbose` (no longer `_verbose`)
- Event handling builds `ReceiptLine` per inspector, prints on arrival
- No display-order buffering — same arrival-order model as pretty
- Completion counter: `[N/total]` increments per arrival, not by
  display position (e.g., Services might be `[1/11]` if it finishes first)
- Normal mode: skip sub-step events. Verbose: accumulate in `sub_lines`,
  flush atomically with parent
- Output: `[N/total] Name... status (metric)`
- Sub-step: `  [N/total.S] Step name... result`
- `finalize(scan: ScanFinalize)` renders summary + footer using flat formatting
  (`, ` separators, text labels, no ANSI)

- [ ] **Step 4: Run tests, update snapshots**

Run: `cargo test -p inspectah-cli flat -- --nocapture`
Then: `cargo insta review`
Expected: all pass

- [ ] **Step 5: Commit**

```
refactor(progress): update FlatRenderer with shared model, verbosity, and footer

Assisted-by: Claude Code (Opus 4.6)
```

---

## Thorn Checkpoint 1

**After Tasks 1-6.** Verify:
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] Shared data model types (`ReceiptLine`, `ScanEndState`, `ScanSummary`)
      are used by both renderers — grep confirms
- [ ] `ScanEndState` enum covers all spec footer paths
- [ ] Arrival-order tests cover: fast-before-slow completion,
      skipped-without-start, verbose atomic parent+child flush
- [ ] No display-order buffering logic present in either renderer

---

## Task 7: Rewire `scan.rs` + Collapse Dispatcher + Delete Old Renderers

**Owner:** Tang
**Files:**
- Modify: `crates/cli/src/commands/scan.rs`
- Modify: `crates/cli/src/progress/mod.rs`
- Delete: `crates/cli/src/progress/rich.rs`
- Delete: `crates/cli/src/progress/plain.rs`

This task merges the old T7+T8+T9 into a single atomic commit that
transitions from old renderers to new. Every intermediate state builds.

- [ ] **Step 1: Update `ProgressMode` enum in `mod.rs`**

```rust
#[derive(Clone, Debug, clap::ValueEnum)]
pub enum ProgressMode {
    Pretty,
    Flat,
}
```

- [ ] **Step 2: Update `Mode` enum and `detect_mode()`**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Pretty,
    Flat,
}

pub fn detect_mode(cli_flag: Option<&ProgressMode>) -> Mode {
    if let Some(flag) = cli_flag {
        return match flag {
            ProgressMode::Pretty => Mode::Pretty,
            ProgressMode::Flat => Mode::Flat,
        };
    }
    if let Ok(val) = std::env::var("INSPECTAH_PROGRESS") {
        return match val.to_lowercase().as_str() {
            "pretty" => Mode::Pretty,
            "flat" => Mode::Flat,
            _ => Mode::Pretty,
        };
    }
    let is_tty = std::io::IsTerminal::is_terminal(&std::io::stderr());
    let is_dumb = std::env::var("TERM").map(|t| t == "dumb").unwrap_or(false);
    if !is_tty || is_dumb { Mode::Flat } else { Mode::Pretty }
}
```

- [ ] **Step 3: Update `TerminalProgress` dispatcher**

```rust
enum TerminalProgressInner {
    Pretty(pretty::PrettyRenderer),
    Flat(flat::FlatRenderer),
    Null,
}
```

Update `new()` to construct `PrettyRenderer` or `FlatRenderer`.
Update `finalize()` signature to take `ScanFinalize`.
Remove `pub mod rich;` and `pub mod plain;`.

- [ ] **Step 4: Update `scan.rs` — remove old, wire new**

- Remove `print_completion()` and `build_summary_counts()`
- Remove the version-changes `eprintln!` block (lines 461-483)
- At each former `print_completion()` call site, construct `ScanFinalize`
  with the appropriate `ScanEndState` variant:

```rust
// Normal tarball success
let end_state = ScanEndState::Completed {
    path: tarball_path.clone(),
    sensitivity: build_sensitivity_notice(&snapshot),
};
progress.finalize(ScanFinalize {
    elapsed: scan_start.elapsed(),
    end_state,
    version_changes: build_version_change_summary(&snapshot),
});
```

Handle all 5 end-state paths:
- Tarball success → `ScanEndState::Completed`
- `--inspect-only` with `--output` → `ScanEndState::InspectOnly`
- `--inspect-only` without `--output` → `ScanEndState::InspectOnlyStdout`
- Write failure → `ScanEndState::WriteFailure`
- SIGINT → `ScanEndState::Interrupted` (see Step 4a)

- [ ] **Step 4a: SIGINT interrupted-path reconciliation**

The CLI layer (`run_scan()`) owns interrupted reconciliation. The
renderer is the authoritative outcome ledger — it already tracks which
inspectors have received `InspectorFinished` events (and therefore have
`ReceiptLine`s). No new fields on `Collected` or separate ledger needed.

**Outcome source:** The renderer's internal state. During normal
collection, every `InspectorFinished` event triggers a receipt line.
After SIGINT, the renderer knows which inspectors in `active_display_order`
have receipt lines and which don't. The reconciliation step uses this.

**Add a method to `TerminalProgress`:**

```rust
impl TerminalProgress {
    /// Return the set of InspectorIds that have received an
    /// InspectorFinished event (i.e., have a ReceiptLine).
    pub fn finished_inspectors(&self) -> HashSet<InspectorId> {
        // Delegates to inner renderer's tracked state
    }
}
```

**Reconciliation in `run_scan()` after SIGINT early return:**

```rust
// The renderer already has receipt lines for inspectors that finished
// during collection (it received their InspectorFinished events).
// Synthesize Interrupted for the rest.
let finished = progress.finished_inspectors();
let active_order = display::active_display_order(has_subscription);

for (id, _name) in active_order {
    if !finished.contains(id) {
        progress.handle(ProgressEvent::InspectorFinished {
            id: *id,
            outcome: InspectorOutcome::Interrupted,
        });
    }
}

// Counts come from the same source as receipt lines — the renderer's
// tracked state. completed = finished.len(), total = active_order.len().
progress.finalize(ScanFinalize {
    elapsed: scan_start.elapsed(),
    end_state: ScanEndState::Interrupted {
        completed: finished.len(),
        total: active_order.len(),
    },
    version_changes: None,
});
```

This ensures receipt lines and footer counts come from the **same
source** — the renderer's tracked `InspectorFinished` events. An
inspector cannot appear as `△ interrupted` in the receipt but be counted
as `completed` in the footer, or vice versa.

- [ ] **Step 5: Add `build_version_change_summary()` helper**

Extract version change data from `snapshot.rpm` and `snapshot.baseline`
into `VersionChangeSummary`. This replaces the old `eprintln!` logic:

```rust
fn build_version_change_summary(snapshot: &InspectionSnapshot) -> Option<VersionChangeSummary> {
    let rpm = snapshot.rpm.as_ref()?;
    let _baseline = snapshot.baseline.as_ref()?;
    let total = rpm.version_changes.len();
    if total == 0 { return None; }
    let target_newer = rpm.version_changes.iter()
        .filter(|vc| matches!(vc.direction, VersionChangeDirection::Upgrade))
        .count();
    let host_newer = total - target_newer;
    Some(VersionChangeSummary { total, target_newer, host_newer })
}
```

- [ ] **Step 6: Pass `has_subscription` to renderer construction**

```rust
let active_order = display::active_display_order(has_subscription);
let progress = TerminalProgress::new(mode, color, verbosity, active_order);
```

- [ ] **Step 7: Delete `rich.rs` and `plain.rs`**

```bash
rm crates/cli/src/progress/rich.rs crates/cli/src/progress/plain.rs
```

- [ ] **Step 8: Leave `--quiet` unchanged (deferred per spec)**

The approved spec defers `--quiet` semantics. This refactor must not
break the existing `--quiet` path but does not redesign it.

Existing behavior: `Verbosity::Quiet` maps to `TerminalProgressInner::Null`,
which swallows all progress events. The old `print_completion()` in
`scan.rs` handled footer output separately (outside the renderer).

Since this refactor moves footer output into `finalize()`, the `Null`
backend's `finalize()` must be a **no-op** — it does not print footer,
receipt, or summary. When `--quiet` is active, `run_scan()` prints the
footer directly (timing + path + sensitivity) using a small inline block
that bypasses the renderer, preserving the pre-rethink quiet contract.

```rust
// In run_scan(), after progress.finalize() for non-quiet paths:
if verbosity == Verbosity::Quiet {
    // Quiet mode: renderer was Null (no receipt/summary printed).
    // Print footer directly, same as pre-rethink print_completion().
    let secs = scan_start.elapsed().as_secs_f64();
    eprintln!("Scan complete ({secs:.1}s)");
    if let Some(path) = &output_path {
        eprintln!("Report: {}", path.display());
        eprintln!("To review: inspectah refine {}", path.display());
    }
}
```

This is intentionally minimal — a future spec will define proper quiet
semantics within the new renderer architecture. The inline bypass keeps
the existing contract without entangling it with the new `ScanEndState`
model.

Test:

```rust
#[test]
fn quiet_mode_null_backend_finalize_is_noop() {
    let mut buf = Vec::new();
    // Construct Null-backed progress, feed events, finalize
    let output = String::from_utf8(buf).unwrap();
    assert!(output.is_empty()); // Null finalize produces nothing
}
```

- [ ] **Step 9: Update `detect_mode` tests in `mod.rs`**

Update for new enum values. Remove tests referencing `Rich` or `Plain`.

- [ ] **Step 10: Build + test + clippy**

Run: `cargo test --workspace && cargo clippy --workspace -- -D warnings`
Expected: all pass, zero warnings

- [ ] **Step 11: Grep for stale references**

Run: `grep -rn 'RichRenderer\|PlainRenderer\|print_completion\|build_summary_counts\|Mode::Rich\|Mode::Plain\|ProgressMode::Rich\|ProgressMode::Plain' crates/ --include='*.rs'`
Expected: zero matches

- [ ] **Step 12: Commit**

```
refactor(progress): collapse to Pretty/Flat, wire ScanEndState, remove old renderers

Assisted-by: Claude Code (Opus 4.6)
```

---

## Thorn Checkpoint 2

**After Task 7.** Verify:
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] Stale reference grep returns zero matches
- [ ] Manual test: `cargo run -- scan` produces expected receipt output
- [ ] Manual test: `cargo run -- scan --progress flat` produces expected flat output
- [ ] Manual test: `cargo run -- scan -v` produces verbose output with sub-steps

---

## Task 8: End-to-End Snapshot Tests

**Owner:** Tang
**Files:**
- Modify: `crates/cli/src/progress/pretty.rs` (add tests)
- Modify: `crates/cli/src/progress/flat.rs` (add tests)

- [ ] **Step 1: Add full-transcript snapshot tests**

These test the complete output from header through footer for each
spec example. Each test feeds events, calls `finalize()` with the
appropriate `ScanEndState`, and snapshots the full output.

Pretty mode:
- `e2e_pretty_fast_scan` — spec Section 10 example 1 (all fast, ~display order)
- `e2e_pretty_slow_rpm_scan` — spec slow VM example (fast inspectors
  print first, RPM prints after ~68s, wave 2 follows)
- `e2e_pretty_failures_degradation` — spec example 2
- `e2e_pretty_clean_host` — spec example 3
- `e2e_pretty_interrupted` — spec example 4
- `e2e_pretty_subscription` — spec subscription example
- `e2e_pretty_inspect_only_stdout` — no path in footer

Flat mode:
- `e2e_flat_arrival_order` — spec flat example (completion counter, not position)
- `e2e_flat_verbose` — spec flat verbose example
- `e2e_flat_12_inspectors` — dynamic `[N/12]` numbering
- `e2e_flat_interrupted` — interrupted with text labels

- [ ] **Step 2: Run tests, accept snapshots**

Run: `cargo test -p inspectah-cli -- --nocapture`
Then: `cargo insta review`
Expected: all pass

- [ ] **Step 3: Commit**

```
test(progress): add end-to-end snapshot tests for pretty and flat renderers

Assisted-by: Claude Code (Opus 4.6)
```

---

## Task 9a: Documentation Inventory (pre-snapshot)

**Owner:** Mango
**Files:**
- Create: `process-docs/scan-output-rethink-doc-inventory.md`

- [ ] **Step 1: Grep all docs for stale references**

Run: `grep -rn 'rich\|plain\|--progress rich\|--progress plain\|three.*mode\|3.*mode' docs/ README.md --include='*.md'`

List all files with stale references. Expected candidates:
- `docs/reference/cli.md`
- `docs/reference/configuration.md` (INSPECTAH_PROGRESS env var)
- `docs/how-to/customize-output.md`
- `docs/how-to/ci-integration.md`
- `docs/getting-started.md`
- `docs/tutorials/first-migration.md`
- `README.md`

- [ ] **Step 2: Write inventory to a durable artifact**

Create `process-docs/scan-output-rethink-doc-inventory.md` with:

```markdown
# Scan Output Rethink — Doc Update Inventory

Generated by T9a grep. Consumed by T9b for post-snapshot finalization.

## Files Requiring Updates

| File | Stale references | Update type |
|------|-----------------|-------------|
| docs/reference/cli.md | --progress rich/plain | Rewrite mode section |
| docs/reference/configuration.md | INSPECTAH_PROGRESS values | Update env var values |
| ... | ... | ... |

## Verification Steps (for T9b)
- [ ] Reconcile `scan --help` against docs
- [ ] Compare transcript snippets against accepted T8 snapshots
- [ ] Final stale-reference grep (zero matches outside CHANGELOG)
```

- [ ] **Step 3: Commit**

```
chore(docs): inventory stale progress mode references for scan output rethink

Assisted-by: Claude Code (Sonnet 4.6)
```

---

## Task 9b: Documentation Update (post-snapshot)

**Owner:** Mango
**Files:** All files from Task 9a inventory

Depends on Task 8 (snapshots frozen).

- [ ] **Step 1: Update `docs/reference/cli.md`**

Replace three-mode description with two modes. Update `--verbose` docs.

- [ ] **Step 2: Update `docs/reference/configuration.md`**

Update `INSPECTAH_PROGRESS` env var: accepts `pretty` and `flat`.

- [ ] **Step 3: Update output examples in all affected docs**

For each file in the Task 9a inventory, update output examples to match
the accepted Task 8 snapshots. Compare transcript snippets against the
accepted insta snapshots to ensure consistency.

- [ ] **Step 4: Update `README.md`**

Update any progress mode references or output examples.

- [ ] **Step 5: Reconcile `scan --help`**

Run: `cargo run -- scan --help`
Verify the help text matches the updated docs. Flag any discrepancies.

- [ ] **Step 6: Final stale reference grep**

Run: `grep -rn 'rich\|plain\|--progress rich\|--progress plain\|three.*mode\|3.*mode\|RichRenderer\|PlainRenderer' docs/ README.md --include='*.md'`
Expected: zero matches (excluding historical CHANGELOG entries)

- [ ] **Step 7: Add CHANGELOG entry**

Add to CHANGELOG under `## [Unreleased]`:

```markdown
### Changed
- Scan progress output redesigned as append-only streaming receipt
- Progress modes simplified from three (rich/plain/flat) to two (pretty/flat)
- Sub-step detail moved behind `--verbose` flag
- Findings summary block added after inspector receipt
- `--verbose` now works with both pretty and flat modes
- Flat mode now respects `--verbose` (previously always showed sub-steps)

### Removed
- `--progress rich` and `--progress plain` modes (use `--progress pretty`)
```

- [ ] **Step 8: Commit**

```
docs: update all references for scan output rethink

Assisted-by: Claude Code (Sonnet 4.6)
```

---

## Thorn Checkpoint 3 (Final)

**After all tasks.** Verify:
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo build --release` succeeds
- [ ] Stale reference grep across `crates/` and `docs/` returns zero
- [ ] CLI reference docs match `scan --help` output
- [ ] CHANGELOG entry present
- [ ] All insta snapshots accepted and committed

---

## Task Dependency Graph

```
T1 (receipt.rs) ──┐
T2 (display.rs) ──┤
                  ├── T3 (pretty core) ── T4 (spinner) ── T5 (summary+footer)
                  │                                              │
                  │                                              ├── T6 (flat update)
                  │                                              │
                  └──────────────────────────────────────────────┤
                                                                 │
                                              T7 (rewire scan.rs + collapse + delete old)
                                                                 │
                                              T8 (e2e snapshots)
                                                 │           │
                                             T9a (doc inventory — can run parallel with T8)
                                                             │
                                                         T9b (doc finalization)
```

**Strict sequencing (every commit builds):**
- T1 and T2: independent, can parallel
- T3: depends on T1+T2 (no buffering — simpler than rev 2)
- T4: depends on T3
- T5: depends on T4
- T6: depends on T1+T2+T5 (uses ScanFinalize/ScanEndState)
- T7: depends on T5+T6 (all renderers ready before rewiring)
- T8: depends on T7
- T9a: depends on T7, can parallel with T8
- T9b: depends on T8+T9a

**Parallelism:**
- T1 ‖ T2 (independent foundations)
- T8 ‖ T9a (snapshots + doc inventory)

All other tasks are strictly sequential.

**Rev 3 note:** Arrival-order output simplified T3 and T6 significantly
(no display-order buffering logic). T4 spinner is also simpler (no
interaction with buffering). Overall implementation LOC is reduced.
