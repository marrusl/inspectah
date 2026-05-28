# Baseline Visibility (revision 4)

## Problem

inspectah compares a host system against a base image but never tells the user which image it compared against, when it was extracted, or how it was resolved. The pull itself floods stderr with raw podman layer output, scrolling away earlier scan messages. Users see classification results ("477 packages added beyond base image") with no way to verify the foundation of those classifications.

## Design

Three changes, implemented incrementally: (1) shared presentation helpers, (2) baseline metadata in rendered artifacts and CLI, (3) pull viewport UX.

### 1. Shared Presentation Helpers

A `baseline_fmt` module in `inspectah-pipeline/src/render/` provides reusable formatting for all output targets. This is the foundation — CLI, README, and audit all use the same helpers.

#### Strategy display labels

```rust
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

#### Version comparison summary

`rpm.version_changes` only covers shared packages whose EVR differs. It does not represent all baseline drift. The summary must be truthful to this scope:

See the expanded `version_comparison_summary` signature in the Baseline State section below, which takes `Option<&[VersionChange]>` to distinguish zero-changes from data-unavailable.

Note: "target-newer" / "host-newer" rather than "upgrade" / "downgrade" in the display string, since the enum variants carry migration semantics but the display label should be unambiguous to someone unfamiliar with the migration direction convention.

#### Baseline state

The baseline block must handle four distinct states, not just present/absent:

| State | Condition | What to show |
|---|---|---|
| **Full** | `target_image` + `baseline` + `rpm.version_changes` all present | Full block with image ref, strategy, digest, extraction time, version comparison |
| **Comparison unavailable** | `target_image` + `baseline` present, but `rpm` section absent or degraded (no `version_changes` data) | Block with image ref, strategy, digest, extraction time, and "Version comparison: data unavailable" instead of a count. This prevents a silent zero that looks like "no differences" when the real state is "we couldn't compare." |
| **Degraded** | `target_image` present, `baseline` absent | Reduced block: image ref, strategy, status line ("baseline unavailable" or "skipped via --no-baseline") |
| **Unknown** | `target_image` absent | Omit the section entirely |

The `version_comparison_summary` helper must distinguish "zero changes" from "data unavailable":

```rust
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
            let upgrades = vcs.iter()
                .filter(|vc| vc.direction == VersionChangeDirection::Upgrade)
                .count();
            let downgrades = vcs.len() - upgrades;
            let detail = match (upgrades, downgrades) {
                (_, 0) => "all target-newer".to_string(),
                (0, _) => "all host-newer".to_string(),
                (u, d) => format!("{u} target-newer, {d} host-newer"),
            };
            format!("{} shared packages with version changes ({})", vcs.len(), detail)
        }
    }
}
```

Callers pass `Some(&rpm.version_changes)` when RPM data is available, or `None` when the RPM section is absent or degraded. This mirrors the web UI's existing `data_unavailable` honesty.

### 2. CLI Output

Two output points, not one monolithic block:

**Provenance confirmation (immediately after pull/extraction):**

```
Pulled quay.io/centos-bootc/centos-bootc:stream9 (sha256:abc123def4)
  Baseline extracted: 447 packages
  Resolved via: os-release (auto-detected)
```

This prints right after `extract_baseline` returns, before "Scanning host..." begins. It answers the user's immediate question: "what am I being compared against?"

**Version comparison line (after collection completes):**

```
  Version changes: 85 shared packages with version changes (all target-newer)
```

This prints after "Scanning host... done", since `version_changes` is only available after collection. Together with the provenance block above, the user has the full picture.

**Full CLI sequence:**

```
Detecting source system...
  CentOS Stream 9 (aarch64)
Resolving target image...
  quay.io/centos-bootc/centos-bootc:stream9 (OsRelease)
Pulling quay.io/centos-bootc/centos-bootc:stream9...
  ┌──────────────────────────────────────────────────────┐
  │ Copying blob sha256:a1b2c3... 42.1 MiB / 89.3 MiB   │
  │ Copying blob sha256:d4e5f6... done                   │
  │ Copying blob sha256:g7h8i9... skipped                │
  └──────────────────────────────────────────────────────┘
Pulled quay.io/centos-bootc/centos-bootc:stream9 (sha256:abc123def4)
  Baseline extracted: 447 packages
  Resolved via: os-release (auto-detected)
Scanning host myhost.example.com...
Scanning host myhost.example.com... done
  Version changes: 85 shared packages with version changes (all target-newer)

Output written to /tmp/inspectah-CentOS9-20260518-143200.tar.gz
```

**Degraded mode** (--no-baseline):

```
Resolving target image...
  quay.io/centos-bootc/centos-bootc:stream9 (OsRelease)
  Baseline: skipped (--no-baseline)
Scanning host myhost.example.com...
```

The resolved target image is always shown when available. Only the baseline details and version comparison are omitted.

**Comparison unavailable** (baseline present but RPM inspector degraded):

```
Pulled quay.io/centos-bootc/centos-bootc:stream9 (sha256:abc123def4)
  Baseline extracted: 447 packages
  Resolved via: os-release (auto-detected)
Scanning host myhost.example.com...
Scanning host myhost.example.com... done
  Version comparison: data unavailable
```

**Baseline extraction failure** remains fail-closed (current behavior). The spec does not introduce a degraded-continuation mode. If extraction fails, the scan aborts with the `PullFailed`/`ExecFailed` error and full stderr from podman. There is no degraded CLI output for extraction failure — the scan simply stops.

### 3. Pull Progress Viewport (TTY)

#### Executor contract

Add one method to the `Executor` trait:

```rust
fn run_with_line_callback(
    &self,
    program: &str,
    args: &[&str],
    on_stderr_line: &mut dyn FnMut(&str),
) -> ExecResult;
```

**Invariants preserved from `run_passthrough_stderr`:**
- nsenter prefix applied by caller (same as all other executor methods)
- `LC_ALL=C` and `LANG=C` environment normalization
- 600-second timeout (matching the existing pull-specific timeout)
- Full stderr captured in `ExecResult.stderr` regardless of callback (the callback is for live display; the full transcript is always available for error diagnostics)
- stdout captured normally in `ExecResult.stdout`

**Object safety:** `&mut dyn FnMut(&str)` is object-safe. No generics on the trait method.

**Mock executor:** `MockExecutor` implements `run_with_line_callback` by splitting pre-recorded stderr on `\n` and calling the callback per-line, then returning the same `ExecResult` as `run`.

**No fallback path.** There is no `std::process::Command` bypass. All pull execution goes through the executor.

**Callback ownership:** `scan.rs` (the CLI layer) owns the terminal rendering callback. `extract_baseline()` in `inspectah-collect` owns the pull execution — it calls `run_with_line_callback` on the executor, passing through a caller-provided callback. The separation:

- `extract_baseline` gains an `on_pull_line: &mut dyn FnMut(&str)` parameter. It always passes the callback to `run_with_line_callback` for the pull step — never falls back to plain `run`, which would lose the 600-second pull timeout. Non-CLI callers (tests, library use) pass a no-op closure (`&mut |_| {}`).
- `scan.rs` constructs the appropriate callback (viewport renderer for TTY, prefixed passthrough for non-TTY) and passes it to `extract_baseline`. The collect layer never touches stderr directly — it just forwards lines.
- This keeps terminal concerns in the CLI layer and pull execution in the collect layer, with the callback as the clean boundary between them.

#### TTY viewport

When stderr is a TTY (checked via `std::io::IsTerminal` on stderr):

- Print `Pulling <image_ref>...` header
- Render a 3-line viewport with box-drawing borders
- Maintain a 3-element ring buffer of recent stderr lines
- On each callback line: strip ANSI escape sequences, truncate to `min(terminal_width, 72) - 6` chars (box borders + padding), push into buffer, redraw
- On completion (callback finished, ExecResult returned): clear the viewport (cursor-up + clear-line for 5 lines: header + top border + 3 content + bottom border), print summary line

**Terminal guardrails:**
- Minimum width: 40 columns. Below this, fall back to non-TTY behavior.
- Line truncation: hard-truncate at viewport width, no wrapping. Truncated lines get `…` suffix.
- ANSI stripping: remove all `\x1b[...` sequences from podman output before display.

**Failure teardown:** On non-zero exit, clear the viewport the same way (cursor-up + clear), then print the error. Full stderr is available in `ExecResult.stderr` and flows into `ExtractionError::PullFailed { reason }` as today. The viewport never eats diagnostic output.

**Interrupt (Ctrl-C):** Not handled specially. The process terminates; the terminal resets on next shell prompt. This matches current behavior.

#### Non-TTY (CI, pipes, redirected logs)

When stderr is not a TTY: pass through podman's stderr lines directly via the callback, printing each to stderr with a `  pull: ` prefix. This preserves the current liveness signal that CI logs depend on:

```
  pull: Copying blob sha256:a1b2c3... 42.1 MiB / 89.3 MiB
  pull: Copying blob sha256:d4e5f6... done
  pull: Copying blob sha256:g7h8i9... skipped
```

On completion, print the same summary line as TTY mode. No viewport, no cursor movement, no ANSI.

**Layer counting:** Best-effort, derived from counting unique `Copying blob` lines with `done` or `skipped` suffixes. If parsing finds zero blobs (unexpected output format), omit the layer count from the summary — just show image ref and digest. Layer count is display-only; it is not persisted in the snapshot.

### 4. README Baseline Section

Add a "Baseline comparison" section after the findings summary table:

```markdown
## Baseline comparison

| | |
|---|---|
| Target image | quay.io/centos-bootc/centos-bootc:stream9 |
| Resolution | os-release (auto-detected) |
| Image digest | sha256:abc123def456 |
| Baseline extracted | 2026-05-18T14:32:00Z |
| Baseline packages | 447 |
| Version changes | 85 shared packages with version changes (all target-newer) |
```

**Implementation:** In `readme.rs`, replace the unused `let _ = base_image_from_snapshot(snap);` call (line 227) with a section builder using `snap.target_image`, `snap.baseline`, and `snap.rpm.as_ref().map(|r| r.version_changes.as_slice())` (Option, to distinguish RPM-unavailable from zero-changes). Uses the shared `baseline_fmt` helpers.

**Degraded mode:** When `target_image` is present but `baseline` is absent, render a reduced section showing the target image, strategy, and "Baseline: unavailable" or "Baseline: skipped (--no-baseline)".

### 5. Audit Report Baseline Section

Same baseline metadata table at the top of the audit report, before the "Packages" section. The audit report already renders `version_changes` in a detail table (lines 103-111 in audit.rs) — the baseline section provides context for interpreting those changes.

Same degraded-mode behavior as README.

## Implementation Order

Following Tang's recommended incremental shape:

1. **Shared helpers first** — `baseline_fmt` module with strategy labels, version comparison summary, and section builders. Pure functions, fully testable.
2. **Rendered artifacts second** — README and audit baseline sections using the shared helpers. Standard renderer pattern, low risk.
3. **CLI provenance + version comparison** — print the provenance block and version comparison line in `scan.rs`. Uses the same helpers.
4. **Pull viewport last** — add `run_with_line_callback` to executor, implement TTY viewport and non-TTY passthrough. This is the riskiest piece; everything above works without it.

Steps 1-3 can ship independently. Step 4 depends on 1-3 but is additive.

## Files Changed

| File | Change |
|---|---|
| `inspectah-pipeline/src/render/baseline_fmt.rs` | New module: strategy labels, version comparison summary, section line builders. |
| `inspectah-pipeline/src/render/mod.rs` | Add `pub mod baseline_fmt;` |
| `inspectah-pipeline/src/render/readme.rs` | Add baseline comparison section using shared helpers. Replace unused stub. |
| `inspectah-pipeline/src/render/audit.rs` | Add baseline section before packages using shared helpers. |
| `inspectah-core/src/traits/executor.rs` | Add `run_with_line_callback` method with default (delegates to `run`). |
| `inspectah-collect/src/executor/real.rs` | Implement `run_with_line_callback`: pipe stderr, call per-line, preserve 600s timeout. |
| `inspectah-collect/src/executor/mock.rs` | Implement `run_with_line_callback`: split recorded stderr, call per-line. |
| `inspectah-collect/src/baseline.rs` | Switch pull step from `run_nsenter_passthrough` to `run_with_line_callback`. |
| `inspectah-cli/src/commands/scan.rs` | Add provenance block after extraction, version comparison after collection, viewport/passthrough logic for pull. |

## Not In Scope

- **Pull policy flags** (`--pull=always/never/if-missing`) — current always-pull behavior is correct.
- **Staleness warnings** — always-fresh pull makes these unnecessary.
- **Image layer analysis** — inspectah inspects the host, not the image internals.
- **Historical comparisons** — comparing across scans is a different feature.
- **Degraded continuation mode** — baseline extraction failure remains fail-closed. A future spec can add explicit degraded continuation if needed.

## Testing

### Shared helpers (`baseline_fmt`)
- Unit tests for `version_comparison_summary`: all-upgrades, all-downgrades, mixed, empty.
- Unit tests for `strategy_label`: all enum variants.
- Unit tests for section line builders: full state, degraded state, unknown state.

### Rendered artifacts (README, audit)
- Snapshot tests: baseline section with full data, with degraded data (`target_image` only), with no baseline, with `no_baseline=true`.
- Parity test: given the same snapshot, README and audit produce consistent baseline metadata (same image ref, same strategy label, same version comparison text).

### CLI output
- Factor provenance and version comparison formatting into pure helpers that take `TargetImageIdentity`, `BaselineData`, and `Option<&[VersionChange]>` and return formatted strings. Test these directly with fixtures.
- CLI integration tests are out of scope until `scan.rs` has an injectable executor/writer seam. The formatted helpers prove correctness; the CLI wiring is thin enough to verify by inspection.

### Pull viewport
- Mock executor tests: `run_with_line_callback` calls callback per-line with pre-recorded stderr.
- Viewport renderer unit tests (pure function taking lines + terminal width, returning rendered frames): normal flow, truncated lines, ANSI stripping, narrow terminal fallback.
- TTY vs non-TTY branching: tested via the renderer helpers, not by mocking TTY state.
- Timeout preservation: assert that `run_with_line_callback` in the real executor uses the same 600s timeout as `run_passthrough_stderr`.

### Proof matrix

| Scenario | Covered by |
|---|---|
| TTY viewport rendering | viewport renderer unit tests |
| Non-TTY line passthrough | viewport renderer unit tests (non-TTY path) |
| Pull failure with full stderr preserved | mock executor + error path test |
| Layer parse failure → omitted count | viewport renderer with unparseable input |
| Zero version changes | `version_comparison_summary` unit test (Some(&[])) |
| Comparison data unavailable | `version_comparison_summary` unit test (None) + section builder + README/audit snapshot test |
| No baseline (--no-baseline) | section builder unit test + README/audit snapshot test |
| Baseline extraction failure | existing `PullFailed` error path (unchanged) |
| CLI/README/audit parity | parity snapshot test |
| 600s timeout preserved | real executor unit test or doc-enforced invariant |
