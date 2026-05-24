# CLI Scan Progress UX — Design Spec

**Status:** Approved (revision 4, approved 2026-05-24)
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
through visual states as the scan runs. Three inspectors expand into
nested sub-checklists. The rest stay single-line.

### Execution model

The collector runs inspectors in two waves via `std::thread::scope`:

- **Wave 1:** RPM (alone — produces `RpmState` that Config depends on)
- **Wave 2:** All other inspectors (in parallel)

The checklist is a **presentation layer over concurrent execution**.
Inspector rows are printed in a fixed display order (see below), but
wave-2 inspectors run simultaneously. Multiple checklist items may be
active at once during wave 2 — this is correct and intentional. Items
complete in arrival order, not display order.

### Happy-path example (TTY)

```
Inspecting host rhel9-web01...
  ✓ RPM packages                            847 found (3.4s)
      ✓ Querying installed packages          847 found
      ✓ Classifying packages                 done
      ✓ Resolving source repositories        8 repos mapped
      ✓ Resolving dependency tree            done
      ✓ Verifying package integrity          done
      ✓ Mapping file ownership               done
  ✓ Services                                 4 units (0.1s)
  ✓ Storage                                  done (0.1s)
  ✓ Kernel & boot                            done (0.1s)
  ✓ Network                                  done (0.1s)
  ✓ Containers                               2 found (0.1s)
  ✓ Users & groups                           done (0.1s)
  ✓ Scheduled tasks                          2 timers (0.1s)
  ✓ Config files                             12 modified (1.8s)
      ✓ Applying RPM verification results    done
      ✓ Walking filesystem                   done
      ✓ Classifying configs                  12 modified
  ✓ SELinux                                  enforcing (0.1s)
  ✓ Non-RPM packages                         3 ecosystems (0.8s)
      ✓ Python virtualenvs                   1 found
      ✓ pip packages                         12 found
      ✓ git repos                            4 found

Scan complete (14.2s) — 847 packages, 12 configs, 4 services, 2 containers
Report: inspectah-rhel9-web01-20260524.tar.gz
To review: inspectah refine inspectah-rhel9-web01-20260524.tar.gz
```

### Mid-scan example (wave 2 parallel)

```
Inspecting host rhel9-web01...
  ✓ RPM packages                            847 found (3.4s)
      ✓ Querying installed packages          847 found
      ...
  ✓ Services                                 4 units (0.1s)
  ✓ Storage                                  done (0.1s)
  ✓ Kernel & boot                            done (0.1s)
  ⣟ Network                                  (1s)
  ⣟ Containers
  ◌ Users & groups
  ◌ Scheduled tasks
  ⣟ Config files
      ⣟ Walking filesystem
      ◌ Classifying configs
  ◌ SELinux
  ◌ Non-RPM packages
```

Multiple spinners active at once. Items complete out of display order.

### Visual states

- **Pending:** `◌` prefix. Visible from the start, waiting to run.
- **Active:** Spinner prefix (braille animation on TTY). Shows elapsed
  time after ~3-4 seconds (`⣟ Network (4s)`).
- **Complete:** `✓` prefix. Count where meaningful plus elapsed time.
- **Skipped:** `–` prefix. Inspector was not applicable to this system
  (e.g., no container runtime, SELinux disabled). Informational, not a
  warning. Example: `– Containers  skipped (no runtime found)`
- **Degraded:** `~` prefix. Inspector ran but with reduced fidelity.
  Data is incomplete but usable. Example:
  `~ RPM packages  847 found (degraded: dep tree unavailable) (3.4s)`
- **Failed:** `✗` prefix. Inspector was expected to work but errored.
  The capability exists on the host but the inspector could not collect
  data. Example: `✗ Containers  failed: podman returned error`
- **Interrupted:** `■` prefix. User sent SIGINT during the scan. All
  active and pending items show interrupted state.

Sub-steps within expanded inspectors use the same state symbols.

### Display order

Fixed presentation order matching `scan.rs` inspector registration:

1. RPM Packages (wave 1 — runs alone)
2. Services (wave 2)
3. Storage (wave 2)
4. Kernel & Boot (wave 2)
5. Network (wave 2)
6. Containers (wave 2)
7. Users & Groups (wave 2)
8. Scheduled Tasks (wave 2)
9. Config Files (wave 2)
10. SELinux (wave 2)
11. Non-RPM Packages (wave 2)

This is display order, not completion order. Wave-2 items complete in
parallel and update their checklist row as results arrive.

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
transitions through the same visual states as outer inspectors. A
sub-step can degrade or fail independently (e.g., `dnf` unavailable
degrades "Resolving dependency tree" but the RPM inspector continues).

### Config Files — 3 sub-steps (all visible upfront)

| Sub-step | Source | Count shown |
|----------|--------|-------------|
| Applying RPM verification results | rpm -Va output from RpmState | "done" |
| Walking filesystem | `walk` module | "done" |
| Classifying configs | `classify` module | N modified |

The first sub-step reuses `rpm -Va` results already produced by the
RPM inspector. It does not re-run integrity verification. The label
"Applying RPM verification results" reflects this accurately.

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

Probe behavior is mode-specific:

**Rich mode:** On `ProbeStarted`, the renderer inserts a spinner line
below the Non-RPM parent. On `ProbeFinished { outcome: Empty }`, the
spinner line is removed (cursor-up, clear line). On
`ProbeFinished { outcome: Found { count } }`, the spinner line is
replaced with a checkmark + count. Only found probes remain in the
final scrollback.

**Plain mode:** On `ProbeStarted`, print a started line
(`▸ pip packages`). On `ProbeFinished { outcome: Found { count } }`,
print a completion line (`✓ pip packages  12 found`). On
`ProbeFinished { outcome: Empty }`, print a completion line
(`– pip packages  none`). All lines are permanent and append-only.
Empty probes are visible in the transcript — this is correct for an
audit log.

**Flat mode:** On `ProbeStarted`, print `pip packages...`. On
`ProbeFinished { outcome: Found { count } }`, print
`pip packages... 12 found`. On `ProbeFinished { outcome: Empty }`,
print `pip packages... none`. Same as plain — all probes visible.

If nothing is found across all checks, the parent line shows:

```
  ✓ Non-RPM packages          none found (0.3s)
```

In rich mode, no sub-items remain. In plain/flat mode, 7 `– none`
lines precede the parent completion line.

**Rationale for the asymmetry:** RPM and Config are structured
inspection of bounded, known domains — every RHEL box has them.
Non-RPM is opportunistic detection of situational ecosystems. In
rich mode, showing seven "none" lines on a bare database server makes
the tool look unfocused — so empty probes are removed. In plain/flat
mode, the transcript is a durable audit log where confirmed-absent is
a finding worth recording.

## 3. Terminal Rendering Modes

Three rendering modes, triggered independently.

### Rich mode (default on TTY)

Full checklist with ANSI color, braille spinner animation, and
cursor-based redraw.

**Detection:** `std::io::IsTerminal::is_terminal(&std::io::stderr())`
AND `$TERM` is not `dumb`.

**Redraw strategy:** The checklist occupies a fixed block of lines on
stderr. The renderer tracks the block height (number of inspector rows
plus expanded sub-steps). Redraws are triggered by two sources:

1. **Progress events:** Each `ProgressEvent` from an inspector
   triggers a full block redraw.
2. **Periodic tick:** A background timer thread (or `Instant`-based
   check in the event loop) triggers a redraw every ~100ms when any
   inspector is active. This drives spinner animation (braille frame
   cycling) and elapsed-time counter updates. Without this, long
   quiet steps (e.g., `rpm -Va` producing no sub-step events) would
   freeze the display.

On each redraw:
1. Cursor-up to the start of the block (`\x1b[{n}A`)
2. Rewrite all lines in the block
3. Cursor stays at the bottom of the block

The tick stops when all inspectors are finished. The tick is a
rendering concern only — it does not generate `ProgressEvent`s and
is not visible to inspectors or the collector.

**Terminal overflow:** If the checklist block exceeds terminal height
minus 2 (reserved for prompt and status), the renderer truncates
pending items from the display, showing only active and completed
items plus a `... and N more` line. As items complete and the active
set shrinks, pending items scroll into view.

**Final scrollback:** When the scan completes, the renderer prints
the final checklist state as permanent output (no cursor-up). This
is the durable artifact in scrollback history. All prior redraws
are overwritten.

### Plain mode (forced durable output on TTY)

Strictly append-only line output with ANSI color. No cursor
manipulation, no `\r` overwrite, no spinner glyphs. Every line
printed is permanent and final.

**Detection:** `INSPECTAH_PROGRESS=plain` env var, or `--progress=plain`
flag. This is for TTY users who want a durable transcript (e.g.,
terminal multiplexer recording, screen reader users who prefer
sequential output).

Active items print a "started" line. When the item completes, a
separate "done" line is appended — the started line is never
modified. Under wave-2 concurrency, started and done lines from
different inspectors may interleave. This is correct — the
transcript is a truthful, chronological log of events.

```
  ▸ RPM packages
      ▸ Querying installed packages
      ✓ Querying installed packages          847 found
      ▸ Classifying packages
      ✓ Classifying packages                 done
      ...
  ✓ RPM packages                            847 found (3.4s)
  ▸ Services
  ▸ Config files
      ▸ Applying RPM verification results
  ✓ Services                                 4 units (0.1s)
      ✓ Applying RPM verification results    done
      ▸ Walking filesystem
  ...
```

`▸` prefix for started lines (no animation). `✓` for complete. Same
state symbols as rich mode for skipped/degraded/failed/interrupted
completion lines. No braille spinners — the `▸` arrow is static.

### Flat mode (non-TTY / log-friendly)

No ANSI escape codes, no cursor manipulation, no spinner animation.
Numbered sequential lines.

**Detection:** stderr is not a terminal, OR `$TERM` is `dumb`.

```
[1/11] RPM packages...
  Querying installed packages... 847 found
  Classifying packages... done
  Resolving source repositories... 8 repos mapped
  Resolving dependency tree... done
  Verifying package integrity... done
  Mapping file ownership... done
[1/11] RPM packages... done (3.4s)
[2/11] Services... done (0.1s)
[3/11] Storage... done (0.1s)
...
```

### `NO_COLOR` interaction

`NO_COLOR` (per https://no-color.org/) strips color only. It does
NOT change the rendering mode. A TTY with `NO_COLOR` set gets rich
mode with cursor-based redraw but no ANSI color codes — monochrome
but animated. This is the correct interpretation of the `NO_COLOR`
spec.

### Accessibility

- Semantic prefixes (`✓`, `◌`, `–`, `~`, `✗`, `■`) carry state
  independently of color. No information conveyed by color alone.
- Plain mode provides a first-class accessible experience on TTY
  without forcing the user to pipe output.
- Flat mode produces clean, parseable output for logging.
- Terminal screen readers handle sequential line output natively.
  Rich mode's cursor-up redraw may not be re-announced by screen
  readers; plain mode is recommended for screen reader users.

## 4. Completion Output

### Successful scan (all inspectors complete or skipped)

```
Scan complete (14.2s) — 847 packages, 12 configs, 4 services, 2 containers
Report: inspectah-rhel9-web01-20260524.tar.gz
To review: inspectah refine inspectah-rhel9-web01-20260524.tar.gz
```

- **Summary line:** Total elapsed time plus key counts from successful
  inspectors. Categories with zero findings are omitted. Skipped
  inspectors do not appear in counts.
- **Report path:** Relative path to the tarball in cwd.
- **Next-step hint:** Copy-pasteable `inspectah refine` command with
  the report path as argument.
- Same format in all rendering modes (no ANSI in completion block).

### Scan with degraded inspectors

```
Scan complete (14.2s) — 847 packages, 12 configs, 4 services
  1 degraded (see report for details)
Report: inspectah-rhel9-web01-20260524.tar.gz
To review: inspectah refine inspectah-rhel9-web01-20260524.tar.gz
```

Report path and refine hint still printed — degraded data is still
useful for migration planning.

### Scan with failed inspectors

```
Scan complete (14.2s) — 847 packages, 12 configs
  1 failed, 1 degraded (see report for details)
Report: inspectah-rhel9-web01-20260524.tar.gz
To review: inspectah refine inspectah-rhel9-web01-20260524.tar.gz
```

Report is still written and path is still shown. Failed sections are
missing from the report but other sections are valid.

### `--inspect-only` with file output

```
Scan complete (14.2s) — 847 packages, 12 configs, 4 services, 2 containers
Output: inspectah-rhel9-web01-20260524.json
```

No refine hint — `--inspect-only` produces raw JSON, not a refineable
tarball.

### `--inspect-only` to stdout

Checklist renders to stderr. JSON renders to stdout. Completion block
on stderr:

```
Scan complete (14.2s) — 847 packages, 12 configs, 4 services, 2 containers
```

No file path (output went to stdout). No refine hint.

### Post-scan export failure

Collection succeeded but tarball/file write failed:

```
Scan complete (14.2s) — 847 packages, 12 configs, 4 services, 2 containers
Error: failed to write report: Permission denied (os error 13)
```

No report path. No refine hint. Exit code 1 (hard error).

### Interrupted scan (SIGINT)

All active and pending items transition to interrupted state. The
completion block reflects what was collected before interruption:

```
Scan interrupted after 6.3s — 847 packages (partial)
No report written.
```

## 5. Exit Codes

Exit codes reflect report trustworthiness, not scan perfection.

| Code | Meaning | When |
|------|---------|------|
| 0 | Report is trustworthy | All inspectors complete, skipped, or degraded. Skipped = not applicable (expected). Degraded = partial data but usable. |
| 1 | Hard error | Pipeline crash, tarball write failure, invalid arguments. No usable output. |
| 2 | Report has blind spots | At least one inspector failed — capability existed on the host but data collection errored. Report is missing sections. |
| 130 | Interrupted (SIGINT) | User sent Ctrl-C. Unix convention: 128 + signal number (SIGINT = 2). No report written. |

**Skipped inspectors do not affect exit code.** A host with no
container runtime exits 0 — absence is topology, not an error.

**Degraded inspectors do not affect exit code.** Degraded data is
still useful for migration planning. The visual warning in the
checklist and completion summary is sufficient.

**Only genuine failures produce exit 2.** The test: "Did the system
claim to have this capability and then fail to deliver?"

### ScanOutcome type (inspectah-cli)

```rust
enum ScanOutcome {
    Clean,
    Degraded,
    Incomplete,
    Interrupted,
}
```

Derived from `snapshot.completeness` (or SIGINT flag) after
`collect()` returns. `main.rs` maps:
- `ScanOutcome::Clean` → exit 0
- `ScanOutcome::Degraded` → exit 0
- `ScanOutcome::Incomplete` → exit 2
- `ScanOutcome::Interrupted` → exit 130
- `Err(_)` → exit 1

## 6. Architecture — Progress Events

Progress reporting lives in `inspectah-collect` as a typed event
model. The CLI layer provides the rendering implementation.

### Event model (inspectah-core or inspectah-collect)

```rust
#[derive(Debug, Clone)]
pub enum ProgressEvent {
    InspectorStarted(InspectorId),
    InspectorFinished {
        id: InspectorId,
        outcome: InspectorOutcome,
    },
    StepStarted {
        inspector: InspectorId,
        step: StepId,
    },
    StepFinished {
        inspector: InspectorId,
        step: StepId,
        outcome: StepOutcome,
    },
    Metric {
        inspector: InspectorId,
        kind: MetricKind,
        value: usize,
    },
    ProbeStarted {
        inspector: InspectorId,
        probe: ProbeId,
    },
    ProbeFinished {
        inspector: InspectorId,
        probe: ProbeId,
        outcome: ProbeOutcome,
    },
}

#[derive(Debug, Clone)]
pub enum ProbeId {
    ElfBinaries,
    PythonVenvs,
    PipPackages,
    NpmPackages,
    GemPackages,
    EnvFiles,
    GitRepos,
}

#[derive(Debug, Clone)]
pub enum ProbeOutcome {
    Found { count: usize },
    Empty,
}

#[derive(Debug, Clone)]
pub enum InspectorOutcome {
    Complete,
    Degraded { reason: String },
    Skipped { reason: String },
    Failed { reason: String },
    Interrupted,
}

#[derive(Debug, Clone)]
pub enum StepOutcome {
    Complete,
    Degraded { reason: String },
    Failed { reason: String },
    Skipped { reason: String },
    Interrupted,
}

// Reason strings are derived from InspectorError variants.
// Examples:
//   Degraded { reason: "dnf unavailable, dependency tree incomplete" }
//   Failed { reason: "podman returned exit code 1" }
//   Skipped { reason: "no container runtime found" }
//
// The renderer uses these to print detail text after the status:
//   ~ RPM packages  847 found (degraded: dnf unavailable) (3.4s)
//   ✗ Containers    failed: podman returned exit code 1

#[derive(Debug, Clone)]
pub enum MetricKind {
    PackagesFound,
    ReposMapped,
    ConfigsModified,
    UnitsFound,
    ContainersFound,
    TimersFound,
}
```

`InspectorId` and `StepId` are the existing typed enums from
`inspectah-core` (or new enums if they don't exist yet). All events
carry explicit identity — no string-keyed display labels at the
collector boundary.

Events are **owned, not borrowed**. This allows buffering, channel
transport, and cross-thread aggregation without lifetime constraints.

### ProgressSink trait

```rust
pub trait ProgressSink: Send + Sync {
    fn emit(&self, event: ProgressEvent);
}
```

`Send + Sync` is required because wave-2 inspectors run in parallel
via `std::thread::scope`. The sink must be safely shareable across
scoped threads.

### Implementations

- **`NullProgress`** (inspectah-collect) — no-op. `Send + Sync`
  trivially. Existing tests and library consumers unchanged.
- **`TerminalProgress`** (inspectah-cli) — renders the checklist to
  stderr. Owns TTY detection, rendering mode selection, ANSI
  formatting, spinner, elapsed timer. Internally uses a `Mutex` to
  serialize concurrent event writes from parallel inspectors.
- **`VecProgress`** (test utility) — collects events into a
  `Mutex<Vec<ProgressEvent>>` for assertion. Thread-safe.

### Inspector trait change

```rust
fn inspect(
    &self,
    ctx: &InspectionContext,
    progress: &dyn ProgressSink,
) -> Result<InspectorOutput, InspectorError>;
```

Each inspector emits events at its key phases:
- RPM: 6 `StepStarted`/`StepFinished` pairs
- Config: 3 `StepStarted`/`StepFinished` pairs
- Non-RPM: 7 `ProbeStarted`/`ProbeFinished` pairs (one per ecosystem
  check). The renderer shows a spinner for `ProbeStarted`, removes it
  on `ProbeFinished { outcome: Empty }`, or settles a checkmark with
  count on `ProbeFinished { outcome: Found { count } }`.
- Other 8 inspectors: no sub-step events needed

### collect() changes

`collect()` accepts `&dyn ProgressSink`. Before each inspector runs,
it emits `InspectorStarted`. After, `InspectorFinished` with the
outcome derived from the `Result<InspectorOutput, InspectorError>`.
The CLI does not need to duplicate this logic.

Outcome mapping:
- `Ok(_)` → `InspectorOutcome::Complete`
- `Err(InspectorError::Skipped { .. })` → `InspectorOutcome::Skipped`
- `Err(InspectorError::Degraded { .. })` → `InspectorOutcome::Degraded`
- `Err(InspectorError::Failed { .. })` → `InspectorOutcome::Failed`
### SIGINT handling

**Owner:** The CLI layer (`main.rs` / `scan.rs`) owns the SIGINT
handler. `collect()` does not install signal handlers.

**Mechanism:** The CLI installs a SIGINT handler before calling
`collect()` that sets an `Arc<AtomicBool>` flag. The flag is passed
to `collect()` as a cancellation token. `collect()` checks the flag:
- Between wave-1 and wave-2: if set, skip wave-2 entirely.
- Before each inspector launch within a wave: if set, skip launch.
- After joining each thread in a wave: results from threads that
  completed before the flag was set are kept. Results from threads
  still running when the flag was set are joined and discarded.

**Cutoff policy:** Results that completed before SIGINT are preserved
in the snapshot. Results in-flight at SIGINT time are discarded —
they are not marked interrupted, they are simply absent. The snapshot
records which inspectors ran and which did not via `completeness`.

**Event emission on SIGINT:** After `collect()` returns, the CLI
emits `InspectorFinished { outcome: Interrupted }` for all
inspectors that were not started or whose results were discarded.
This is a CLI-layer concern, not a collector concern.

Progress events are the ephemeral display channel. Outcomes are also
recorded durably in `snapshot.completeness` — the exit code is
derived from snapshot state, not from progress events.

## 7. Pull Viewport Height Fix

Bundled with this work because we are already touching scan CLI output
code.

**Current state:** Fixed-height viewport for baseline image pull
progress. Too short — progress lines scroll too fast to read.

**Change:** Dynamic viewport height based on terminal size.
- Height = 30% of terminal rows
- Floor: 8 rows (minimum readable)
- Cap: 16 rows (prevent dominating the screen)
- Non-TTY: skip viewport entirely, emit plain log lines
- Plain mode: skip viewport, emit sequential pull lines

Uses the existing `terminal_size` crate dependency for height
detection.

## Scope Boundaries

### In scope

- Full checklist output with 11 inspector lines
- Nested sub-checklists for RPM (6), Config (3), Non-RPM (discoveries)
- Visual states: pending, active, complete, skipped, degraded, failed,
  interrupted
- Three rendering modes: rich (TTY), plain (forced sequential), flat
  (non-TTY)
- `NO_COLOR` strips color only, does not change rendering mode
- Typed `ProgressEvent` model with `InspectorId`, `StepId`,
  `MetricKind`, and outcome enums
- `ProgressSink: Send + Sync` trait in inspectah-collect
- `TerminalProgress` renderer in inspectah-cli
- Exit codes: 0 (trustworthy), 1 (hard error), 2 (blind spots)
- Completion output for all paths: tarball, `--inspect-only` file,
  `--inspect-only` stdout, export failure, partial scan, interrupted
- Pull viewport dynamic height fix

### Out of scope (deferred)

- **Fleet scan progress** — scanning N hosts is a different UX problem,
  deferred to fleet work
- **Early headlines** — editorial findings during scan (interesting
  but not in v1)
- **`--verbose` / `--quiet` flags** — could layer on later
- **TUI refine** — already on ROADMAP (deferred items), separate feature
