# Feature: Scan Output Rethink

## What

Replace the `inspectah scan` progress output with an append-only streaming
receipt. Collapse three progress modes (rich/plain/flat) to two
(pretty/flat). Sub-steps move behind `--verbose`. A findings summary block
is added after the inspector receipt. The slow-inspector safety valve
provides a spinner fallback for any inspector exceeding ~3.5s.

## Why

The Rust rewrite dramatically reduced scan time — typically ~10 seconds,
though slower systems (constrained VMs, large RPM sets) can take 60-90
seconds with RPM dominating wall clock. The current rich-mode output —
per-inspector spinners with sub-step expansion, elapsed timers, ANSI
cursor-up block redraw — was designed for the old 12+ minute regime. The
block-redraw approach has a persistent line-redraw bug, and the sub-step
detail completes faster than the eye can track on most systems. The
output should show progress as inspectors complete (arrival order) rather
than animate a dashboard or buffer behind slow inspectors.

With rich mode becoming append-only, the rich/plain distinction no longer
earns its maintenance cost — both would be append-only with color and
Unicode symbols. Collapsing to two modes (pretty/flat) simplifies the
codebase and matches CLI conventions (podman, cargo, dnf all ship two
modes: human vs. machine).

## Surfaces Touched

- [x] Backend (core types, collect, pipeline) — progress renderer refactor
- [x] CLI (new flags, subcommands, output changes) — `--progress` mode collapse, `--verbose` behavior shift
- [ ] HTML report
- [ ] Web refine UI
- [ ] TUI refine UI
- [ ] Containerfile rendering
- [ ] Audit log output
- [x] Docs (user-facing documentation) — CLI reference update for `--progress`, `--verbose`, output format

## Pipeline Stages

| Stage | Status | Justification (if skipped) |
|-------|--------|---------------------------|
| Brainstorm | **complete** | — |
| Spec Review | **approved (R3)** | — |
| Plan | **complete** | — |
| Plan Review | **approved (R5)** | — |

## Feature Type Checklist

### Backend (core / collect / pipeline)

- [ ] `ReceiptLine` struct defined in shared types (see Section 2a)
- [ ] `ScanSummary` struct defined in shared types (see Section 2a)
- [ ] `RichRenderer` replaced with `PrettyRenderer` (append-only)
- [ ] `PlainRenderer` removed — functionality merged into `PrettyRenderer`
- [ ] `FlatRenderer` updated to use shared data model and respect verbosity
- [ ] `ProgressMode` enum: `Pretty`, `Flat` (remove `Rich`, `Plain`)
- [ ] `INSPECTAH_PROGRESS` env var accepts `pretty` and `flat` only
- [ ] `DISPLAY_ORDER` extended with conditional `Subscription` entry
- [ ] Per-inspector summary metric defined for each `InspectorId`
- [ ] Findings summary block generated from `ScanSummary`
- [ ] Slow-inspector safety valve spinner (ELAPSED_THRESHOLD)
- [ ] Arrival-order output (no display-order buffering)
- [ ] Unhappy-path expansion (degraded/skipped/failed/interrupted show reason)
- [ ] `print_completion()` replaced by receipt footer (see Section 6a)
- [ ] Unit tests updated for new renderer
- [ ] Snapshot tests updated with `insta`

### CLI

- [ ] `--progress` flag accepts `pretty` and `flat`
- [ ] `--verbose` restores sub-step detail (both modes)
- [ ] Old `--progress rich` and `--progress plain` values removed
- [ ] Help text updated

### Docs

- [ ] CLI reference updated for `--progress` and `--verbose`
- [ ] CHANGELOG entry added

---

## Design

### 1. Output Model

Pretty mode produces an append-only streaming receipt on stderr. Each
inspector prints one line when it finishes. No block-redraw, no cursor-up
ANSI sequences. The output has five zones:

```
[Header]      Inspecting host <hostname>...
[Receipt]     ✓/✗/⚠/○/△ lines, one per inspector, in arrival order
[Summary]     Findings hotspot lines (non-trivial only)
[Timing]      Inspected in N.Ns (+ non-success tally if applicable)
[Footer]      Output path + next-step hint
```

**Receipt ordering is arrival order.** Each inspector's line prints as
soon as it finishes — no buffering, no display-order constraint.
Inspectors run in parallel waves (`collect.rs`: wave 1 = RPM +
independents, wave 2 = RPM-dependent). Fast inspectors print immediately
even if slower inspectors in the same wave are still running.

This means receipt line order varies between runs depending on inspector
completion timing. On a fast system, most inspectors complete nearly
simultaneously and the output approximates display order. On slower
systems where RPM dominates (60s+), fast inspectors appear first, then
the slow inspector prints when it finishes, giving the user visible
progress throughout.

**Verbose child-line atomicity:** In verbose mode, an inspector's parent
line and its sub-step/probe child lines are buffered together and printed
as a single atomic block. Child lines never interleave across inspectors.
This per-inspector atomicity is the only buffering in the system.

### 2. Inspector Lines

Each inspector prints one line with a status symbol and a summary metric.
Format: `  ✓ <Name>               <metric>`.

**The inspector list is dynamic.** The base set is 11 inspectors in
fixed display order. `SubscriptionInspector` is conditionally added as
the 12th when `--preserve subscription` or `--preserve all` is used.

| Inspector | Metric | Condition | Notes |
|-----------|--------|-----------|-------|
| RPM packages | `613 packages, 6 repos` | always | Package count + repo count |
| Services | `6 units` | always | systemd unit count |
| Storage | `done` | always | No meaningful count currently |
| Kernel & boot | `done` | always | |
| Network | `done` | always | |
| Containers | `0 found` or `3 running` | always | Container count |
| Users & groups | `done` | always | |
| Scheduled tasks | `17 timers` | always | Timer/cron count |
| Config files | `37 modified` | always | Modified config count |
| SELinux | `enforcing` or `done` | always | Mode when available |
| Non-RPM packages | `3 ecosystems` | always | Ecosystem count, with sub-lines |
| Subscription | `done` | `--preserve subscription\|all` | Entitlement/subscription data |

`DISPLAY_ORDER` must be extended to include `InspectorId::Subscription`
as a conditional 12th entry. Flat mode numbering reflects the actual
inspector count: `[N/11]` or `[N/12]` depending on whether subscription
collection is active.

Each inspector defines its own `summary_metric()` → `Option<String>`.
When `None`, the line shows "done".

### 2a. Shared Data Model

Receipt metrics and findings summaries are **typed data**, not
renderer-owned strings. Both `PrettyRenderer` and `FlatRenderer` consume
the same structures so their output cannot drift.

```rust
/// One inspector's receipt line — built by the renderer from progress
/// events, consumed by both pretty and flat formatters.
struct ReceiptLine {
    id: InspectorId,
    state: InspectorState,       // Success | Degraded | Skipped | Failed | Interrupted
    metric: Option<String>,      // "613 packages, 6 repos" or None → "done"
    reason: Option<String>,      // only for non-success states
    sub_lines: Vec<String>,      // Non-RPM ecosystem breakdown, etc.
}

/// Aggregate scan summary — computed after all inspectors finish.
struct ScanSummary {
    version_changes: Option<VersionChangeSummary>,
    hotspots: Vec<HotspotLine>,  // each with a non-trivial threshold
    non_success_tally: NonSuccessTally,
}
```

Both renderers format these structs into their respective output styles
(pretty: color + symbols; flat: numbered lines, no ANSI). The data model
lives in a shared module, not inside either renderer.

### 3. Non-RPM Sub-Lines

Non-RPM packages get an indented breakdown line showing per-ecosystem
counts. Sub-lines are **plain text with no status symbol** — they are
detail of the parent line, not independent receipt entries.

```
  ✓ Non-RPM packages           3 ecosystems
       pip 23 · npm 69 · git 1
```

Ecosystems with zero packages are omitted from the breakdown.

**Under degradation:** If the Non-RPM inspector is degraded (e.g., one
ecosystem probe timed out), the parent line shows ⚠ with reason, and
sub-lines show only the ecosystems that completed successfully. Counts
from completed probes are authoritative; the degradation reason indicates
which probes failed. Sub-lines survive degradation — partial data is
better than no data.

### 4. Unhappy Path States

| State | Symbol | Color | Behavior |
|-------|--------|-------|----------|
| Success | ✓ | Green | Single line with metric |
| Degraded | ⚠ | Yellow | Line + reason in parentheses |
| Skipped | ○ | Dim | Line + skip reason |
| Failed | ✗ | Red | Line + failure reason |
| Interrupted | △ | Yellow | Line + "interrupted" |

Degraded, skipped, failed, and interrupted inspectors **always expand**
with a reason, even at default verbosity. The reason appears inline
after the metric or in place of it:

```
  ⚠ Config files               37 modified (rpm verify timed out)
  ✗ Containers                 podman not found
  ○ Non-RPM packages           skipped (--skip-nonrpm)
  △ Storage                    interrupted
```

**Degraded counts are authoritative for the scope that completed.** A
degraded inspector's metric reflects what it successfully collected. The
reason indicates what was missed. Consumers (summary block, timing tally)
treat degraded counts as real data, not estimates.

**Interrupted** maps to the SIGINT cancellation path. During a normal
scan, `Interrupted` only appears for wave-2 inspectors that never started
because SIGINT arrived during wave 1. The SIGINT handler in `run_scan()`
already handles full-scan interruption separately — `Interrupted` in the
receipt covers the partial case where some inspectors completed and others
did not.

Color is controlled by the existing `use_color()` function (respects
`NO_COLOR` env var). When color is disabled, the same symbols are emitted
without ANSI escape codes.

### 5. Slow-Inspector Safety Valve

If any single inspector exceeds `ELAPSED_THRESHOLD` (3.5s wall-clock from
inspector start), show a spinner line for that inspector. The threshold is
a named constant — not a hardcoded magic number.

**Interaction with arrival-order output:** With arrival ordering, the
spinner operates independently of other inspectors. Multiple spinners can
be active simultaneously if multiple inspectors exceed the threshold
(though only one spinner line is shown at a time — the longest-running).

Behavior:
1. When an inspector exceeds `ELAPSED_THRESHOLD` and no spinner is active,
   show a spinner line: `  ⠋ <Name>               (N.Ns)`
2. If another inspector finishes while a spinner is active, cancel the
   spinner (clear the line), print the finished inspector's receipt line,
   then restart the spinner for the still-slow inspector
3. If multiple inspectors are slow, the spinner shows for whichever has
   been running longest. When that one finishes, the spinner transfers
   to the next-longest
4. When the last slow inspector finishes, its result line replaces the
   spinner normally — no restart

This is the only case where pretty mode uses in-place line manipulation
(single-line clear + rewrite, not multi-line cursor-up). The existing
braille spinner frames are reused.

**Example: slow RPM scan (83s total):**

```
Inspecting host ...
  ✓ Services                   6 units          ← fast inspectors
  ✓ Storage                    done                print immediately
  ✓ Kernel & boot              done                while RPM runs
  ✓ Network                    done
  ✓ Containers                 0 found
  ✓ Users & groups             done
  ✓ Scheduled tasks            15 timers
  ⠋ RPM packages               (5.2s)           ← spinner after threshold
     ... spinner animates for ~63 more seconds ...
  ✓ RPM packages               544 packages, 6 repos  ← replaces spinner
  ⠋ Config files               (3.5s)           ← wave 2 spinner
     ... spinner for ~12 more seconds ...
  ✓ Config files               17 modified      ← wave 2 results
  ✓ SELinux                    done                arrive together
  ✓ Non-RPM packages           1 ecosystem
       pip 19
```

### 6. Findings Summary Block

After the receipt and before the timing line, a summary block shows
hotspot information — counts that tell the user something they didn't
already know from the receipt.

**Candidate lines (in fixed order):**

| Line | Source | Threshold (omit if) |
|------|--------|-------------------|
| Version changes | snapshot `version_summary` | 0 changes |
| Modified configs + non-RPM totals | receipt metrics | all zero |

```
  58 version changes (54 target-newer, 4 host-newer)
  37 modified configs · 23 pip packages · 69 npm packages
```

**Rules:**
- Lines appear in the fixed order above; no reordering
- A line is omitted when its values are at or below the threshold
- If all lines are omitted (clean host), the summary block is omitted
  entirely — no blank line, no placeholder
- The summary block is separated from the receipt by one blank line
- Maximum 2 lines in the current design; new candidates may be added in
  future specs as inspectors evolve
- The version changes line always appears first when present (it is the
  highest-signal finding)

The second line aggregates: `N modified configs` (from Config inspector)
concatenated with non-RPM ecosystem totals using ` · ` separator. Only
non-zero items appear. If only configs are non-zero: `37 modified configs`.
If only non-RPM is non-zero: `23 pip packages · 69 npm packages`.

### 6a. Scan Footer

The current `print_completion()` function in `scan.rs` is replaced by
the receipt's footer zone. The footer is part of the receipt output —
not a separate function.

**Footer structure — full path matrix:**

The footer varies by scan outcome and CLI flags. All paths are specified
below so the footer is fully snapshot-testable.

**Normal scan (tarball produced):**

```
  Inspected in 8.4s
  Report: /tmp/inspectah-rhel10-20260610.tar.gz
  To review: inspectah refine /tmp/inspectah-rhel10-20260610.tar.gz
```

**`--inspect-only` with `--output <path>`:**

```
  Inspected in 8.4s
  Output: /tmp/inspectah-rhel10-20260610/
```

**`--inspect-only` without `--output`:**

JSON snapshot is printed to stdout. The receipt (on stderr) shows only
the timing line — no output path, because the data went to stdout:

```
  Inspected in 8.4s
```

**Tarball write failure:**

If `create_tarball()` fails, the receipt prints the timing line with no
report path. The error is propagated via `anyhow::bail!` after the
receipt prints:

```
  Inspected in 8.4s
  Error: failed to write report: <error detail>
```

**`--inspect-only` write failure:**

Same pattern — timing line, then error:

```
  Inspected in 8.4s
  Error: failed to write output: <error detail>
```

**Non-success tally in timing line:**

When any inspector finished in a non-success state, the timing line
appends a parenthetical tally:

```
  Inspected in 8.4s (1 failed, 1 degraded, 1 skipped)
```

Only non-zero categories appear. Order: failed, degraded, skipped,
interrupted.

**Interrupted scan (SIGINT during scan):**

When the scan is interrupted, the receipt prints whatever inspectors
completed (with their normal receipt lines), marks remaining inspectors
as `△ interrupted`, and the footer reflects partial completion:

```
  Interrupted after 3.2s (5 of 11 inspectors completed)
```

The inspector count in the parenthetical reflects the actual total
(11 or 12 depending on subscription). No report/output path lines are
printed for interrupted scans (no artifact was produced).

**Sensitivity confirmation:**

When `--preserve` or `--no-redaction` is used, the existing sensitivity
confirmation lines print after the report path, unchanged from current
behavior.

### 7. Progress Modes

Two modes, selected via `--progress <mode>` flag or `INSPECTAH_PROGRESS`
env var:

| Mode | When | Behavior |
|------|------|----------|
| `pretty` | Default for TTY | Color, Unicode symbols, safety valve spinner |
| `flat` | Non-TTY / `TERM=dumb` / explicit | Numbered lines, no ANSI, no color |

Detection priority: CLI flag > env var > TTY auto-detect.

The `ProgressMode` enum drops `Rich` and `Plain` variants. The
`PlainRenderer` is removed. The `RichRenderer` is replaced by
`PrettyRenderer`. The `FlatRenderer` is updated to use the shared data
model (Section 2a).

**Per-mode behavior matrix:**

| Capability | Pretty | Flat |
|-----------|--------|------|
| ANSI color | Yes (respects NO_COLOR) | Never |
| Unicode symbols (✓/✗/⚠/○/△) | Yes | Text equivalents (ok/FAIL/WARN/skip/INT) |
| Safety valve spinner | Yes | No (lines print on arrival) |
| Arrival-order output | Yes | Yes |
| Findings summary block | Yes | Yes |
| Footer (path + hint) | Yes | Yes |
| `--verbose` sub-steps | Yes | Yes |

### 8. Verbosity

Two levels, **orthogonal to progress mode** — both pretty and flat
respect the verbosity flag:

| Level | Flag | Pretty behavior | Flat behavior |
|-------|------|-----------------|---------------|
| Normal | (default) | Receipt lines only, sub-steps hidden | Receipt lines only, sub-steps hidden |
| Verbose | `--verbose` | Receipt lines + indented sub-steps for all inspectors | Numbered receipt + numbered sub-step lines |

**Verbose child-line atomicity:** In verbose mode, sub-step and probe
lines are buffered per inspector and flushed together with the parent
receipt line. Child lines never interleave across inspectors. The
renderer collects `StepStarted`/`StepFinished`/`ProbeFinished` events
into the parent `ReceiptLine`'s `sub_lines` and prints them as an
indented block immediately after the parent line. This per-inspector
atomicity is the only buffering in the system.

**Pretty verbose example:**

```
Inspecting host rhel10...
  ✓ RPM packages               613 packages, 6 repos
       ✓ Querying installed packages        613 found
       ✓ Classifying packages               done
       ✓ Resolving source repositories      6 repos mapped
       ✓ Resolving dependency tree          done
       ✓ Verifying integrity                done
       ✓ Mapping file ownership             done
  ✓ Services                   6 units
  ✓ Storage                    done
  ...
```

**Flat verbose example:**

```
[1/11] RPM packages... ok (613 packages, 6 repos)
  [1/11.1] Querying installed packages... 613 found
  [1/11.2] Classifying packages... done
  [1/11.3] Resolving source repositories... 6 repos mapped
  [1/11.4] Resolving dependency tree... done
  [1/11.5] Verifying integrity... done
  [1/11.6] Mapping file ownership... done
[2/11] Services... ok (6 units)
[3/11] Storage... ok
...
```

**Implementation note:** The current `FlatRenderer` accepts a `_verbose`
parameter but ignores it — sub-steps always print. This spec changes flat
to respect verbosity: normal hides sub-steps, verbose shows them. This is
a behavior change for flat mode.

`--quiet` is out of scope for this feature. The `Verbosity::Quiet` enum
variant remains in code but is not wired to the new renderers. If
`--quiet` is passed, behavior is unchanged from pre-rethink: the enum
variant exists, the renderer ignores it, and progress output prints
normally. A future spec may define `--quiet` semantics; this spec
explicitly defers it.

### 9. Scope Boundaries

- **No machine-parseable summary.** The snapshot JSON file is the
  structured data output of `inspectah scan`. Progress stderr is purely
  human-facing.
- **No backwards compatibility aliases.** `--progress rich` and
  `--progress plain` are removed, not aliased. The tool is pre-1.0 with
  a small user base.

### 10. Example Output

**Default (pretty, normal verbosity, healthy scan):**

```
Inspecting host rhel10...
  ✓ RPM packages               613 packages, 6 repos
  ✓ Services                   6 units
  ✓ Storage                    done
  ✓ Kernel & boot              done
  ✓ Network                    done
  ✓ Containers                 0 found
  ✓ Users & groups             done
  ✓ Scheduled tasks            17 timers
  ✓ Config files               37 modified
  ✓ SELinux                    enforcing
  ✓ Non-RPM packages           3 ecosystems
       pip 23 · npm 69 · git 1

  58 version changes (54 target-newer, 4 host-newer)
  37 modified configs · 23 pip packages · 69 npm packages

  Inspected in 8.4s
  Report: /tmp/inspectah-rhel10-20260610.tar.gz
  To review: inspectah refine /tmp/inspectah-rhel10-20260610.tar.gz
```

**With failures and degradation:**

```
Inspecting host rhel10...
  ✓ RPM packages               613 packages, 6 repos
  ✓ Services                   6 units
  ✓ Storage                    done
  ✓ Kernel & boot              done
  ✓ Network                    done
  ✗ Containers                 podman not found
  ✓ Users & groups             done
  ✓ Scheduled tasks            17 timers
  ⚠ Config files               37 modified (rpm verify timed out)
  ✓ SELinux                    enforcing
  ○ Non-RPM packages           skipped (--skip-nonrpm)

  58 version changes (54 target-newer, 4 host-newer)
  37 modified configs

  Inspected in 8.4s (1 failed, 1 degraded, 1 skipped)
  Report: /tmp/inspectah-rhel10-20260610.tar.gz
  To review: inspectah refine /tmp/inspectah-rhel10-20260610.tar.gz
```

**Clean host (no drift):**

```
Inspecting host rhel10-clean...
  ✓ RPM packages               613 packages, 6 repos
  ✓ Services                   6 units
  ✓ Storage                    done
  ✓ Kernel & boot              done
  ✓ Network                    done
  ✓ Containers                 0 found
  ✓ Users & groups             done
  ✓ Scheduled tasks            0 timers
  ✓ Config files               0 modified
  ✓ SELinux                    enforcing
  ✓ Non-RPM packages           0 ecosystems

  Inspected in 7.1s
  Report: /tmp/inspectah-rhel10-clean-20260610.tar.gz
  To review: inspectah refine /tmp/inspectah-rhel10-clean-20260610.tar.gz
```

**Interrupted scan:**

```
Inspecting host rhel10...
  ✓ RPM packages               613 packages, 6 repos
  ✓ Services                   6 units
  ✓ Storage                    done
  ✓ Kernel & boot              done
  ✓ Network                    done
  △ Containers                 interrupted
  △ Users & groups             interrupted
  △ Scheduled tasks            interrupted
  △ Config files               interrupted
  △ SELinux                    interrupted
  △ Non-RPM packages           interrupted

  Interrupted after 3.2s (5 of 11 inspectors completed)
```

**With `--preserve subscription` (12 inspectors):**

```
Inspecting host rhel10...
  ✓ RPM packages               613 packages, 6 repos
  ✓ Services                   6 units
  ✓ Storage                    done
  ✓ Kernel & boot              done
  ✓ Network                    done
  ✓ Containers                 0 found
  ✓ Users & groups             done
  ✓ Scheduled tasks            17 timers
  ✓ Config files               37 modified
  ✓ SELinux                    enforcing
  ✓ Non-RPM packages           3 ecosystems
       pip 23 · npm 69 · git 1
  ✓ Subscription               done

  58 version changes (54 target-newer, 4 host-newer)
  37 modified configs · 23 pip packages · 69 npm packages

  Inspected in 8.7s
  ⚠ This snapshot contains sensitive data:
     Preserved: subscription
     Redaction: active
  Report: /tmp/inspectah-rhel10-20260610.tar.gz
  To review: inspectah refine /tmp/inspectah-rhel10-20260610.tar.gz
```

**`--inspect-only` without `--output` (JSON to stdout):**

```
{...JSON snapshot on stdout...}
```

stderr:
```
Inspecting host rhel10...
  ✓ RPM packages               613 packages, 6 repos
  ...
  ✓ Non-RPM packages           3 ecosystems
       pip 23 · npm 69 · git 1

  58 version changes (54 target-newer, 4 host-newer)
  37 modified configs · 23 pip packages · 69 npm packages

  Inspected in 8.4s
```

(No output path line — data went to stdout.)

**Slow VM scan (RPM-dominated, 83s total):**

```
Inspecting host rhel10-slow...
  ✓ Services                   6 units
  ✓ Storage                    done
  ✓ Kernel & boot              done
  ✓ Network                    done
  ✓ Containers                 0 found
  ✓ Users & groups             done
  ✓ Scheduled tasks            15 timers
  ✓ RPM packages               544 packages, 6 repos
  ✓ Config files               17 modified
  ✓ SELinux                    done
  ✓ Non-RPM packages           1 ecosystem
       pip 19

  44 version changes (40 target-newer, 4 host-newer)
  17 modified configs · 19 pip packages

  Inspected in 83.5s
  Report: /tmp/inspectah-rhel10-slow-20260610.tar.gz
  To review: inspectah refine /tmp/inspectah-rhel10-slow-20260610.tar.gz
```

(Fast inspectors printed first while RPM ran for ~68s. Wave-2 inspectors
printed after RPM finished.)

**Flat mode (normal verbosity):**

Flat mode uses a completion counter `[N/total]` instead of a fixed
position number, since arrival order means positions vary between runs:

```
[1/11] Services... ok (6 units)
[2/11] Storage... ok
[3/11] Kernel & boot... ok
[4/11] Network... ok
[5/11] Containers... ok (0 found)
[6/11] Users & groups... ok
[7/11] Scheduled tasks... ok (15 timers)
[8/11] RPM packages... ok (544 packages, 6 repos)
[9/11] Config files... ok (17 modified)
[10/11] SELinux... ok (done)
[11/11] Non-RPM packages... ok (1 ecosystem: pip 19)

44 version changes (40 target-newer, 4 host-newer)
17 modified configs, 19 pip packages

Inspected in 83.5s
Report: /tmp/inspectah-rhel10-slow-20260610.tar.gz
To review: inspectah refine /tmp/inspectah-rhel10-slow-20260610.tar.gz
```

---

## Source Material

- Pre-spec brainstorm: `comms/threads/2026-06-10-scan-output-rethink.md`
  (Fern UX analysis + Ember product framing)
- Current implementation: `crates/cli/src/progress/` (rich/plain/flat
  renderers, display helpers, mod.rs dispatcher)
- Current completion: `crates/cli/src/commands/scan.rs` (`print_completion()`,
  `build_summary_counts()`)
- Collect pipeline: `crates/pipeline/src/collect.rs` (wave-based parallel
  execution, display order)
- Feature pipeline: `processes/feature-pipeline.md`
- R1 reviews: `marks-inbox/reviews/2026-06-10-scan-output-rethink-r1-{tang,thorn,fern}.md`
- R2 reviews: `marks-inbox/reviews/2026-06-10-scan-output-rethink-r2-{tang,thorn,fern}.md`
- R3 reviews: `marks-inbox/reviews/2026-06-10-scan-output-rethink-r3-{tang,thorn}.md`
- Arrival-order revision: prompted by 83s VM scan observation; Fern+Ember
  unanimously recommended arrival order over strict display order
