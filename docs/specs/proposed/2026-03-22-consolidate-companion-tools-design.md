# Consolidate Companion Tools into inspectah

**Date:** 2026-03-22
**Status:** Proposed

## Problem

inspectah ships three separate entry points for what is logically one tool:

- `inspectah` — host inspection and report generation
- `inspectah-fleet` — fleet snapshot aggregation (console script)
- `inspectah-refine` — interactive editing with live re-render (standalone script)

This creates distribution friction (three things to install/discover), CLI inconsistency (different invocation patterns), and an unnecessary container-in-container pattern for refine re-rendering. The companion shell scripts (`run-inspectah.sh`, `run-inspectah-fleet.sh`) duplicate podman setup logic.

## Design

Two phases: Phase 1 restructures the CLI and consolidates scripts (structural, no behavior changes). Phase 2 simplifies refine's re-render path (behavioral change, isolated).

---

## Phase 1: CLI Restructure and Script Consolidation

### CLI structure

Replace the flat argparse in `cli.py` with subcommand-based parsing:

```
inspectah                         → inspect (default subcommand)
inspectah inspect [flags]         → explicit inspect
inspectah fleet <input-dir> [flags] → fleet aggregation
inspectah refine <tarball> [flags]  → refine mode
```

Top-level parser uses `add_subparsers(dest='command')`. All existing inspect flags move under the `inspect` subparser. To support bare `inspectah --from-snapshot foo.json` (no explicit subcommand), `cli.py` pre-processes `sys.argv`: if the first non-program argument starts with `-` (a flag, not a subcommand name), prepend `inspect` to the argument list before parsing. This keeps backwards compatibility without fighting argparse.

`inspectah --help` shows available subcommands (inspect, fleet, refine) with one-line descriptions. `inspectah inspect --help` shows inspect-specific flags.

### Fleet integration

- `inspectah fleet <input-dir>` replaces `inspectah-fleet aggregate <input-dir>`. The `aggregate` subcommand is removed — fleet only does one thing.
- All existing fleet flags (`-p/--min-prevalence`, `--no-hosts`, `--json-only`, `-o/--output-file`, `--output-dir`) move to the `fleet` subparser.
- Fleet's core logic (directory scanning, snapshot loading, merging, output) stays in `src/inspectah/fleet/` modules. The top-level CLI routes to a `run_fleet()` function that orchestrates the same steps currently in `fleet/__main__.py:main()`.
- `src/inspectah/fleet/__main__.py` becomes a thin wrapper that calls the top-level `main()` with `['fleet'] + sys.argv[1:]`, preserving `python -m inspectah.fleet` as a convenience.
- `src/inspectah/fleet/cli.py` retains fleet-specific flag definitions (called by the top-level CLI to populate the `fleet` subparser) but drops the `aggregate` subcommand layer.
- Remove `inspectah-fleet` from `pyproject.toml` console_scripts.

### Refine integration

Lift the standalone `inspectah-refine` script into `src/inspectah/refine.py` (new module):

- `inspectah refine <tarball> [--no-browser] [--port PORT]`
- Extracts tarball to a temp dir, serves `report.html` via stdlib `http.server`, handles `/api/re-render` POST.
- Auto-launches browser unless `--no-browser`.
- **Phase 1 re-render path:** In Phase 1, re-rendering still shells out to `inspectah` as a subprocess — but now as a direct `inspectah inspect --from-snapshot --refine-mode` process call (not a container launch). This works because in the container context, `inspectah` is installed and available as a command. On the host (if pip-installed), same thing. No podman-in-podman needed. Phase 2 replaces this subprocess call with a direct `run_pipeline()` function call.
- Delete the standalone `inspectah-refine` script.
- Adapt tests from `test_inspectah_refine.py` to the new module path.

### Shell script consolidation

`run-inspectah.sh` becomes the single wrapper:

```
run-inspectah.sh                      → inspect (default)
run-inspectah.sh fleet [flags]        → fleet aggregation
run-inspectah.sh refine <tarball>     → refine mode
```

Changes:
- Detect first argument: if `fleet` or `refine`, pass through as the subcommand. Otherwise, default to inspect (current behavior preserved).
- Fleet mode: absorb podman volume/output setup from `run-inspectah-fleet.sh`.
- Refine mode: `run-inspectah.sh refine` calls `podman run -p 8642:8642 ... inspectah refine --port 8642` with a fixed port inside the container. The `--port` flag on `inspectah refine` defaults to 8642. The wrapper uses a fixed mapping rather than dynamic port discovery (the current `_find_free_port()` approach doesn't work across the container boundary).

Delete:
- `run-inspectah-fleet.sh`
- `inspectah-refine` (standalone script)

Update:
- driftify's `run-fleet-test.sh`: change `run-inspectah-fleet.sh` references to `run-inspectah.sh fleet`
- README for both projects

### Testing

- CLI parsing: bare `inspectah` defaults to inspect, `inspectah inspect` works, `inspectah fleet` works, `inspectah refine` works
- Flag routing: inspect flags on bare `inspectah` work, fleet flags on `inspectah fleet` work
- Exit messages: `pipeline.py` prints `./inspectah-refine <tarball>` as a next step after inspection — update to `inspectah refine <tarball>`. Similarly update any help text, comments, or `prog=` references that mention `inspectah-refine` or `inspectah-fleet` (in `cli.py`, `fleet/cli.py`, `schema.py`).
- Refine module: existing tests in `test_inspectah_refine.py` adapted to new module path. Tests that reference the standalone script path (subprocess calls to `./inspectah-refine`) must be rewritten to import from `inspectah.refine`.
- No behavioral changes to test — Phase 1 is structural only

### Backwards compatibility

**Preserved:**
- `python -m inspectah` (inspect default)
- `python -m inspectah.fleet` (thin wrapper)
- `run-inspectah.sh` without arguments (identical behavior)
- All existing CLI flags for inspect

**Breaking:**
- `inspectah-fleet` console script removed → use `inspectah fleet`
- `inspectah-refine` script removed → use `inspectah refine`
- `run-inspectah-fleet.sh` removed → use `run-inspectah.sh fleet`

---

## Phase 2: Refine Re-render Simplification

### Current flow (after Phase 1)

```
Host: run-inspectah.sh refine foo.tar.gz
  → podman run ... inspectah refine /input/foo.tar.gz
    → (inside container) HTTP server starts
    → (inside container) re-render calls subprocess:
      inspectah inspect --from-snapshot --refine-mode (same container, no nesting)
```

### New flow

```
Host: run-inspectah.sh refine foo.tar.gz
  → podman run ... inspectah refine /input/foo.tar.gz
    → (inside container) HTTP server starts
    → (inside container) re-render calls run_pipeline() directly
```

Replace `subprocess.run(['podman', ...])` in the HTTP handler with a direct call to `run_pipeline()`. The function already accepts snapshot dicts. May need a small tweak to return rendered HTML as a string instead of writing to disk.

### Benefits

- No container-in-container (no nested podman dependency)
- Faster re-renders (no container startup overhead)
- Testable without podman
- `inspectah refine` also works locally if inspectah is pip-installed (no container required), though container remains the primary distribution path

### Container-only principle

Users are not expected to install inspectah locally. The container remains the distribution method. `run-inspectah.sh refine` runs everything inside the container. Direct `inspectah refine` on the host works if installed, but is not the documented path.

## Affected files

### Phase 1

| Action | File | Change |
|--------|------|--------|
| Rewrite | `src/inspectah/cli.py` | Subcommand-based argparse |
| Modify | `src/inspectah/__main__.py` | Route to subcommand handlers |
| Modify | `src/inspectah/fleet/__main__.py` | Thin wrapper for `python -m inspectah.fleet` |
| Modify | `src/inspectah/fleet/cli.py` | Remove `aggregate` subcommand, adapt flags |
| Create | `src/inspectah/refine.py` | HTTP server + tarball extraction (from standalone script) |
| Rewrite | `run-inspectah.sh` | Consolidated wrapper with subcommand routing |
| Delete | `run-inspectah-fleet.sh` | Absorbed into `run-inspectah.sh` |
| Delete | `inspectah-refine` | Absorbed into `src/inspectah/refine.py` |
| Modify | `pyproject.toml` | Remove `inspectah-fleet` console script |
| Modify | `tests/test_inspectah_refine.py` | Adapt to new module path |
| Modify | `tests/test_cli.py` | Add subcommand parsing tests |
| Modify | `README.md` | Update CLI reference |
| Modify | `docs/design.md` | Update CLI reference |

### Phase 2

| Action | File | Change |
|--------|------|--------|
| Modify | `src/inspectah/refine.py` | Replace subprocess re-render with `run_pipeline()` call |
| Modify | `src/inspectah/pipeline.py` | Return rendered HTML string option |
| Modify | tests | Add re-render integration tests |

### Cross-repo

| Action | File | Change |
|--------|------|--------|
| Modify | `driftify/run-fleet-test.sh` | `run-inspectah-fleet.sh` → `run-inspectah.sh fleet` |
| Modify | `driftify/README.md` | Update fleet testing reference |
