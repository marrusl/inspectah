# Fleet Tarball Output Design

## Problem

`yoinkc-fleet aggregate` currently outputs a bare JSON file. The operator must
then manually run `yoinkc --from-snapshot merged.json --output-dir ...` to get a
Containerfile, HTML report, and tarball. This two-step workflow is friction for
both users and developers testing with driftify.

Additionally, running `yoinkc-fleet` requires activating a Python venv on the
operator's workstation — inconsistent with how `run-yoinkc.sh` provides a
zero-install container-based experience for inspection.

## Decisions

| Decision | Answer |
|---|---|
| Default output | Tarball (Containerfile + report + snapshot + configs) |
| JSON-only escape hatch | `--json-only` flag skips rendering |
| Workstation ergonomics | `run-yoinkc-fleet.sh` wrapper runs in container |
| Wrapper location | yoinkc repo, alongside `run-yoinkc.sh` |
| Tarball naming | `<dir-name>-YYYYMMDD-HHMMSS.tar.gz` |
| Pipeline integration | Call `run_pipeline(from_snapshot_path=...)` via temp file |
| Dev test script | `run-fleet-test.sh` updated to use wrapper, fully automated |

## Design

### 1. Fleet `__main__.py` Changes

Current flow: merge -> write JSON -> done.

New flow: merge -> write JSON to tempfile -> `run_pipeline()` -> tarball.

After `merge_snapshots()` returns the merged snapshot:

1. Write merged snapshot JSON to a `NamedTemporaryFile`.
2. Call `run_pipeline()` with:
   - `from_snapshot_path=temp_path`
   - `output_file=tarball_path` (default: `<dir-name>-YYYYMMDD-HHMMSS.tar.gz`
     in CWD)
   - `run_inspectors=None` (not needed)
   - `run_renderers` imported from yoinkc renderers
3. Print the tarball path.

For `--json-only`, skip step 2 and write JSON directly (current behavior).

### CLI Changes

- `-o` / `--output`: specifies the tarball path (not JSON path).
- `--json-only`: write merged JSON only, skip rendering. `-o` in this mode
  specifies the JSON path.
- `--output-dir`: write to a directory instead of tarball (mirrors yoinkc's
  `--output-dir`).

### Tarball Naming

Uses `<dir-name>-YYYYMMDD-HHMMSS.tar.gz` where `<dir-name>` comes from
`input_dir.resolve().name`. Parallels how single-host tarballs use the hostname.

### 2. `run-yoinkc-fleet.sh` Wrapper

Follows the same pattern as `run-yoinkc.sh` — a thin shell script that runs
the yoinkc container with appropriate mounts.

**Usage:**

```bash
./run-yoinkc-fleet.sh ./fleet-tarballs/ -p 67
```

**Behavior:**

1. Resolve input directory to an absolute path.
2. Pull the yoinkc container image (same image `run-yoinkc.sh` uses).
3. Run `podman run` with:
   - Input directory mounted read-only: `-v "$INPUT_DIR":/input:ro`
   - Output directory mounted read-write: `-v "$OUTPUT_DIR":/output`
   - Entry point: `yoinkc-fleet aggregate /input -o /output/<name>.tar.gz`
   - Pass through `-p`, `--json-only`, `--no-hosts` flags.
4. Print the output tarball path on the host.

Output tarball written to CWD by default (same as `run-yoinkc.sh`).

### 3. `run-fleet-test.sh` Updates (driftify)

Updated to run the full loop automatically instead of printing manual
instructions.

After all three profile runs:

1. Curl `run-yoinkc-fleet.sh` (same pattern as curling `run-yoinkc.sh`).
2. Collect tarballs into a temp directory.
3. Run the fleet wrapper against that directory.
4. Print the final fleet tarball path.

One command (`./run-fleet-test.sh`) produces a fleet tarball. No manual steps.

### 4. Testing Strategy

**Python tests** (in `tests/test_fleet_cli.py`):

- `test_fleet_tarball_output` — call `main()` with test snapshots, assert
  `.tar.gz` produced containing Containerfile, report.html,
  inspection-snapshot.json.
- `test_fleet_json_only` — call `main()` with `--json-only`, assert JSON
  written, no tarball.
- `test_fleet_output_dir` — call `main()` with `--output-dir`, assert directory
  with rendered files.
- `test_fleet_tarball_naming` — assert tarball name matches
  `<dir-name>-YYYYMMDD-HHMMSS.tar.gz` pattern.

**Manual testing:**

- `run-yoinkc-fleet.sh ./fleet-test/ -p 67` produces tarball.
- `run-yoinkc-fleet.sh ./fleet-test/ --json-only` produces JSON.
- `run-fleet-test.sh` runs full driftify loop end-to-end.

## Scope

### In scope

- Fleet `__main__.py` tarball output via `run_pipeline()`
- CLI flag changes (`--json-only`, `--output-dir`)
- `run-yoinkc-fleet.sh` container wrapper
- `run-fleet-test.sh` automation update
- Python tests for tarball output

### Out of scope

- Refactoring pipeline.py to extract `render_and_package()` (future cleanup)
- Fleet collection (SSH/Ansible push to hosts)
- RPM packaging

## Future Work

- Extract `render_and_package()` from pipeline.py for cleaner shared interface.
- Fleet collection sub-project: generalize `run-fleet-test.sh` pattern into
  `yoinkc-fleet collect` with SSH/Ansible.
