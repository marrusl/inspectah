# CLI Scan Progress UX — Design Spec

**Status:** Proposed
**Date:** 2026-05-24
**ROADMAP ref:** "CLI UX: Scan Progress Reporting (MEDIUM)", "CLI UX: Baseline Pull Viewport Height (LOW)"

## Problem

`inspectah scan` goes completely silent while inspectors run. The only
output is `Scanning host <hostname>...` followed by silence until
`Scanning host <hostname>... done`. RPM package collection dominates
wall-clock time with multiple slow shell commands (`rpm -qa`,
`dnf repoquery`, `rpm -Va`). The user has no visibility into what the
tool is doing or how far along it is.

## Design Goal

Make the scan feel like a competent diagnostic that is *seeing* the
system. The user should trust the report before they open it.

**Frame:** Confidence-building, not entertainment. Delight is a side
effect of competence, not a goal.

## 1. Overall Pattern — Full Checklist

All 11 inspectors are listed upfront as a checklist. Items transition
from pending to active to complete as the scan runs. Three inspectors
expand into nested sub-checklists. The rest stay single-line.

```
Inspecting host rhel9-web01...
  ✓ RPM packages                            847 found (3.4s)
      ✓ Querying installed packages          847 found
      ✓ Classifying packages                 done
      ✓ Resolving source repositories        8 repos mapped
      ✓ Resolving dependency tree            done
      ✓ Verifying package integrity          done
      ✓ Mapping file ownership               done
  ✓ Config files                             12 modified (1.8s)
      ✓ Verifying package integrity          done
      ✓ Walking filesystem                   done
      ✓ Classifying configs                  12 modified
  ✓ Services                                 4 units (0.1s)
  ✓ Containers                               2 found (0.1s)
  ✓ Kernel & boot                            done (0.1s)
  ✓ Network                                  done (0.1s)
  ✓ Storage                                  done (0.1s)
  ✓ SELinux                                  enforcing (0.1s)
  ✓ Users & groups                           done (0.1s)
  ✓ Scheduled tasks                          2 timers (0.1s)
  ✓ Non-RPM packages                         3 ecosystems (0.8s)
      ✓ Python virtualenvs                   1 found
      ✓ pip packages                         12 found
      ✓ git repos                            4 found

Scan complete (14.2s) — 847 packages, 12 configs, 4 services, 2 containers
Report: inspectah-rhel9-web01-20260524.tar.gz
To review: inspectah refine inspectah-rhel9-web01-20260524.tar.gz
```

### Visual states

- **Pending:** `◌` prefix. Visible from the start, waiting to run.
- **Active:** Spinner prefix (braille animation on TTY). Shows elapsed
  time after ~3-4 seconds (`⣟ Services (4s)`).
- **Complete:** `✓` prefix. Shows a count where meaningful plus elapsed
  time.

### Inspector order

Matches the current execution order in `scan.rs`:

1. RPM Packages
2. Services
3. Storage
4. Kernel & Boot
5. Network
6. Containers
7. Users & Groups
8. Scheduled Tasks
9. Config Files
10. SELinux
11. Non-RPM Packages

## 2. Nested Sub-Checklists

Three inspectors expand into sub-checklists because they have
reportable internal phases with meaningful wall-clock time.

### RPM Packages — 6 sub-steps (all visible upfront)

| Sub-step | Source function | Shell command | Count shown |
|----------|---------------|---------------|-------------|
| Querying installed packages | `query_packages()` | `rpm -qa --queryformat` | packages found |
| Classifying packages | `classify_packages()` | Pure computation | "done" |
| Resolving source repositories | `populate_source_repos()` | `dnf repoquery` / `rpm -qi` | repos mapped |
| Resolving dependency tree | `classify_leaf_auto()` | `dnf repoquery --recursive` | "done" |
| Verifying package integrity | `collect_supplementary()` | `rpm -Va` | "done" |
| Mapping file ownership | `query_file_ownership()` | `rpm -qa --queryformat` | "done" |

All 6 sub-steps are visible from the start of the RPM phase. Each
transitions pending → active → complete.

### Config Files — 3 sub-steps (all visible upfront)

| Sub-step | Source | Count shown |
|----------|--------|-------------|
| Verifying package integrity | rpm -Va results (from RpmState) | "done" |
| Walking filesystem | `walk` module | "done" |
| Classifying configs | `classify` module | N modified |

### Non-RPM Packages — discoveries only

Sub-items appear only when an ecosystem is found. The 7 ecosystem
checks run in order:

1. ELF binaries (/opt, /srv, /usr/local)
2. Python virtualenvs
3. pip packages (system-level dist-info)
4. npm packages (package-lock.json)
5. gem packages (Gemfile.lock)
6. .env files
7. git repos

The active scanner shows as a spinner line that disappears when it
finishes empty, or settles as a checkmark with a count when it finds
results. If nothing is found across all checks:

```
  ✓ Non-RPM packages          none found (0.3s)
```

**Rationale for the asymmetry:** RPM and Config are structured
inspection of bounded, known domains — every RHEL box has them.
Non-RPM is opportunistic detection of situational ecosystems. Showing
seven "none" lines on a bare database server makes the tool look
unfocused. The pattern difference reinforces the semantics: structured
inspection vs. opportunistic detection.

## 3. Terminal Capability & Fallback

### TTY mode (stderr is a terminal)

Full checklist with ANSI formatting, braille spinner animation,
`\r` in-place updates for the active line, elapsed timer, color for
checkmarks and counts.

Detection: `std::io::IsTerminal::is_terminal(&std::io::stderr())`
(already in codebase) plus terminal width via `terminal_size` crate
(already a dependency).

### Non-TTY mode

Triggers when stderr is not a terminal, or `NO_COLOR` env var is set,
or `$TERM` is `dumb`.

Flat sequential lines, no ANSI escape codes, no cursor manipulation:

```
[1/11] RPM packages...
  Querying installed packages... 847 found
  Classifying packages... done
  Resolving source repositories... 8 repos mapped
  Resolving dependency tree... done
  Verifying package integrity... done
  Mapping file ownership... done
[1/11] RPM packages... done (3.4s)
[2/11] Config files...
  Verifying package integrity... done
  Walking filesystem... done
  Classifying configs... 12 modified
[2/11] Config files... done (1.8s)
[3/11] Services... done (0.1s)
```

### Accessibility

- Semantic prefixes (`✓`, `◌`, counters) carry meaning independently
  of color. No information conveyed by color alone.
- Non-TTY fallback produces clean, parseable output suitable for
  logging and screen readers.
- No animation that relies on cursor rewriting in non-TTY mode.
- Terminal screen readers handle sequential line output natively — the
  discoveries-only pattern is no different from any other CLI that
  prints lines as work completes.

## 4. Completion Output

```
Scan complete (14.2s) — 847 packages, 12 configs, 4 services, 2 containers
Report: inspectah-rhel9-web01-20260524.tar.gz
To review: inspectah refine inspectah-rhel9-web01-20260524.tar.gz
```

- **Summary line:** Total elapsed time plus key counts. Categories with
  zero findings are omitted. Non-RPM contributes a count only if
  ecosystems were found (e.g., `3 non-RPM ecosystems`).
- **Report path:** Relative path to the tarball in cwd.
- **Next-step hint:** Copy-pasteable `inspectah refine` command with
  the report path as an argument. Only printed when the report was
  saved successfully.
- Same format in both TTY and non-TTY modes (no ANSI in the completion
  block).

## 5. Architecture — ProgressSink Trait

Progress reporting lives in `inspectah-collect` as a trait. The CLI
layer provides the rendering implementation.

### ProgressSink trait (inspectah-collect)

```rust
pub enum ProgressEvent<'a> {
    SubStepStarted(&'a str),
    SubStepCompleted(&'a str),
    Count(&'a str, usize),       // "packages", 847
    Discovery(&'a str),          // Non-RPM: ecosystem found
}

pub trait ProgressSink {
    fn emit(&self, event: ProgressEvent<'_>);
}
```

### Implementations

- **`NullProgress`** (inspectah-collect) — no-op. Existing tests and
  library consumers unchanged.
- **`TerminalProgress`** (inspectah-cli) — renders the checklist to
  stderr. Owns TTY detection, ANSI formatting, elapsed timer, spinner,
  and flat-line fallback.
- **`VecProgress`** (test utility) — collects events into a
  `RefCell<Vec>` for assertion in unit tests.

### Inspector trait change

```rust
// Before
fn inspect(&self, ctx: &InspectionContext)
    -> Result<InspectorOutput, InspectorError>;

// After
fn inspect(&self, ctx: &InspectionContext, progress: &dyn ProgressSink)
    -> Result<InspectorOutput, InspectorError>;
```

Each inspector gets `progress.emit()` calls at key sub-steps:
- RPM: 6 emit points
- Config: 3 emit points
- Non-RPM: up to 7 emit points (one per ecosystem check)
- Other 8 inspectors: no sub-step emits needed. The `collect()` loop
  handles their outer checklist line.

### collect() changes

The `collect()` function passes the `ProgressSink` through to each
inspector. Before each inspector runs, it emits an inspector-level
start event. After, a completion event. This gives `TerminalProgress`
enough information to render the outer checklist without inspectors
needing to know about it.

Tang determines final API details during implementation: enum variant
naming, `#[non_exhaustive]` on `ProgressEvent`, whether `collect()`
emits inspector-level events or the CLI loop does.

## 6. Pull Viewport Height Fix

Bundled with this work because we are already touching scan CLI output
code.

**Current state:** Fixed-height viewport for baseline image pull
progress. Too short — progress lines scroll too fast to read.

**Change:** Dynamic viewport height based on terminal size.
- Height = 30% of terminal rows
- Floor: 8 rows (minimum readable)
- Cap: 16 rows (prevent dominating the screen)
- Non-TTY: skip viewport entirely, emit plain log lines

Uses the existing `terminal_size` crate dependency for height
detection.

## Scope Boundaries

### In scope

- Full checklist output with 11 inspector lines
- Nested sub-checklists for RPM (6), Config (3), Non-RPM (discoveries)
- Elapsed timer on slow phases (>3-4s threshold)
- TTY vs non-TTY rendering modes
- `ProgressSink` trait in inspectah-collect
- `TerminalProgress` renderer in inspectah-cli
- Completion summary with copy-pasteable refine command
- Pull viewport dynamic height fix

### Out of scope (deferred)

- **Fleet scan progress** — scanning N hosts is a different UX problem,
  deferred to fleet work
- **Early headlines** — editorial findings during scan (Ember's idea,
  interesting but not in v1)
- **`--verbose` / `--quiet` flags** — could layer on later
- **TUI refine** — already on ROADMAP (deferred items), separate feature
