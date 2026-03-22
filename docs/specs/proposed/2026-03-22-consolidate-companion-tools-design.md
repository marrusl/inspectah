# Consolidate Companion Tools into yoinkc

**Date:** 2026-03-22
**Status:** Proposed

## Problem

yoinkc ships three separate entry points for what is logically one tool:

- `yoinkc` — host inspection and report generation
- `yoinkc-fleet` — fleet snapshot aggregation (console script)
- `yoinkc-refine` — interactive editing with live re-render (standalone script)

This creates distribution friction (three things to install/discover), CLI inconsistency (different invocation patterns), and an unnecessary container-in-container pattern for refine re-rendering. The companion shell scripts (`run-yoinkc.sh`, `run-yoinkc-fleet.sh`) duplicate podman setup logic.

## Design

Two phases: Phase 1 restructures the CLI and consolidates scripts (structural, no behavior changes). Phase 2 simplifies refine's re-render path (behavioral change, isolated).

---

## Phase 1: CLI Restructure and Script Consolidation

### CLI structure

Replace the flat argparse in `cli.py` with subcommand-based parsing:

```
yoinkc                         → inspect (default subcommand)
yoinkc inspect [flags]         → explicit inspect
yoinkc fleet <input-dir> [flags] → fleet aggregation
yoinkc refine <tarball> [flags]  → refine mode
```

Top-level parser uses `add_subparsers(dest='command')`. All existing inspect flags move under the `inspect` subparser. To support bare `yoinkc --from-snapshot foo.json` (no explicit subcommand), `cli.py` pre-processes `sys.argv`: if the first non-program argument starts with `-` (a flag, not a subcommand name), prepend `inspect` to the argument list before parsing. This keeps backwards compatibility without fighting argparse.

`yoinkc --help` shows available subcommands (inspect, fleet, refine) with one-line descriptions. `yoinkc inspect --help` shows inspect-specific flags.

### Fleet integration

- `yoinkc fleet <input-dir>` replaces `yoinkc-fleet aggregate <input-dir>`. The `aggregate` subcommand is removed — fleet only does one thing.
- All existing fleet flags (`-p/--min-prevalence`, `--no-hosts`, `--json-only`, `-o/--output-file`, `--output-dir`) move to the `fleet` subparser.
- Fleet's core logic (directory scanning, snapshot loading, merging, output) stays in `src/yoinkc/fleet/` modules. The top-level CLI routes to a `run_fleet()` function that orchestrates the same steps currently in `fleet/__main__.py:main()`.
- `src/yoinkc/fleet/__main__.py` becomes a thin wrapper that calls the top-level `main()` with `['fleet'] + sys.argv[1:]`, preserving `python -m yoinkc.fleet` as a convenience.
- `src/yoinkc/fleet/cli.py` retains fleet-specific flag definitions (called by the top-level CLI to populate the `fleet` subparser) but drops the `aggregate` subcommand layer.
- Remove `yoinkc-fleet` from `pyproject.toml` console_scripts.

### Refine integration

Lift the standalone `yoinkc-refine` script into `src/yoinkc/refine.py` (new module):

- `yoinkc refine <tarball> [--no-browser] [--port PORT]`
- Extracts tarball to a temp dir, serves `report.html` via stdlib `http.server`, handles `/api/re-render` POST.
- Auto-launches browser unless `--no-browser`.
- **Phase 1 re-render path:** In Phase 1, re-rendering still shells out to `yoinkc` as a subprocess — but now as a direct `yoinkc inspect --from-snapshot --refine-mode` process call (not a container launch). This works because in the container context, `yoinkc` is installed and available as a command. On the host (if pip-installed), same thing. No podman-in-podman needed. Phase 2 replaces this subprocess call with a direct `run_pipeline()` function call.
- Delete the standalone `yoinkc-refine` script.
- Adapt tests from `test_yoinkc_refine.py` to the new module path.

### Shell script consolidation

`run-yoinkc.sh` becomes the single wrapper:

```
run-yoinkc.sh                      → inspect (default)
run-yoinkc.sh fleet [flags]        → fleet aggregation
run-yoinkc.sh refine <tarball>     → refine mode
```

Changes:
- Detect first argument: if `fleet` or `refine`, pass through as the subcommand. Otherwise, default to inspect (current behavior preserved).
- Fleet mode: absorb podman volume/output setup from `run-yoinkc-fleet.sh`.
- Refine mode: `run-yoinkc.sh refine` calls `podman run -p 8642:8642 ... yoinkc refine --port 8642` with a fixed port inside the container. The `--port` flag on `yoinkc refine` defaults to 8642. The wrapper uses a fixed mapping rather than dynamic port discovery (the current `_find_free_port()` approach doesn't work across the container boundary).

Delete:
- `run-yoinkc-fleet.sh`
- `yoinkc-refine` (standalone script)

Update:
- driftify's `run-fleet-test.sh`: change `run-yoinkc-fleet.sh` references to `run-yoinkc.sh fleet`
- README for both projects

### Testing

- CLI parsing: bare `yoinkc` defaults to inspect, `yoinkc inspect` works, `yoinkc fleet` works, `yoinkc refine` works
- Flag routing: inspect flags on bare `yoinkc` work, fleet flags on `yoinkc fleet` work
- Exit messages: `pipeline.py` prints `./yoinkc-refine <tarball>` as a next step after inspection — update to `yoinkc refine <tarball>`. Similarly update any help text, comments, or `prog=` references that mention `yoinkc-refine` or `yoinkc-fleet` (in `cli.py`, `fleet/cli.py`, `schema.py`).
- Refine module: existing tests in `test_yoinkc_refine.py` adapted to new module path. Tests that reference the standalone script path (subprocess calls to `./yoinkc-refine`) must be rewritten to import from `yoinkc.refine`.
- No behavioral changes to test — Phase 1 is structural only

### Backwards compatibility

**Preserved:**
- `python -m yoinkc` (inspect default)
- `python -m yoinkc.fleet` (thin wrapper)
- `run-yoinkc.sh` without arguments (identical behavior)
- All existing CLI flags for inspect

**Breaking:**
- `yoinkc-fleet` console script removed → use `yoinkc fleet`
- `yoinkc-refine` script removed → use `yoinkc refine`
- `run-yoinkc-fleet.sh` removed → use `run-yoinkc.sh fleet`

---

## Phase 2: Refine Re-render Simplification

### Current flow (after Phase 1)

```
Host: run-yoinkc.sh refine foo.tar.gz
  → podman run ... yoinkc refine /input/foo.tar.gz
    → (inside container) HTTP server starts
    → (inside container) re-render calls subprocess:
      yoinkc inspect --from-snapshot --refine-mode (same container, no nesting)
```

### New flow

```
Host: run-yoinkc.sh refine foo.tar.gz
  → podman run ... yoinkc refine /input/foo.tar.gz
    → (inside container) HTTP server starts
    → (inside container) re-render calls run_pipeline() directly
```

Replace `subprocess.run(['podman', ...])` in the HTTP handler with a direct call to `run_pipeline()`. The function already accepts snapshot dicts. May need a small tweak to return rendered HTML as a string instead of writing to disk.

### Benefits

- No container-in-container (no nested podman dependency)
- Faster re-renders (no container startup overhead)
- Testable without podman
- `yoinkc refine` also works locally if yoinkc is pip-installed (no container required), though container remains the primary distribution path

### Container-only principle

Users are not expected to install yoinkc locally. The container remains the distribution method. `run-yoinkc.sh refine` runs everything inside the container. Direct `yoinkc refine` on the host works if installed, but is not the documented path.

## Affected files

### Phase 1

| Action | File | Change |
|--------|------|--------|
| Rewrite | `src/yoinkc/cli.py` | Subcommand-based argparse |
| Modify | `src/yoinkc/__main__.py` | Route to subcommand handlers |
| Modify | `src/yoinkc/fleet/__main__.py` | Thin wrapper for `python -m yoinkc.fleet` |
| Modify | `src/yoinkc/fleet/cli.py` | Remove `aggregate` subcommand, adapt flags |
| Create | `src/yoinkc/refine.py` | HTTP server + tarball extraction (from standalone script) |
| Rewrite | `run-yoinkc.sh` | Consolidated wrapper with subcommand routing |
| Delete | `run-yoinkc-fleet.sh` | Absorbed into `run-yoinkc.sh` |
| Delete | `yoinkc-refine` | Absorbed into `src/yoinkc/refine.py` |
| Modify | `pyproject.toml` | Remove `yoinkc-fleet` console script |
| Modify | `tests/test_yoinkc_refine.py` | Adapt to new module path |
| Modify | `tests/test_cli.py` | Add subcommand parsing tests |
| Modify | `README.md` | Update CLI reference |
| Modify | `docs/design.md` | Update CLI reference |

### Phase 2

| Action | File | Change |
|--------|------|--------|
| Modify | `src/yoinkc/refine.py` | Replace subprocess re-render with `run_pipeline()` call |
| Modify | `src/yoinkc/pipeline.py` | Return rendered HTML string option |
| Modify | tests | Add re-render integration tests |

### Cross-repo

| Action | File | Change |
|--------|------|--------|
| Modify | `driftify/run-fleet-test.sh` | `run-yoinkc-fleet.sh` → `run-yoinkc.sh fleet` |
| Modify | `driftify/README.md` | Update fleet testing reference |
