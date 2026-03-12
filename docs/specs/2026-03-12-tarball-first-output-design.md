# Tarball-First Output & run-yoinkc.sh Consolidation

**Date:** 2026-03-12
**Status:** Draft

## Problem

yoinkc currently writes output to a directory, and the wrapper script
`run-yoinkc.sh` handles tarball creation, entitlement cert bundling, and
argument wrangling after yoinkc exits. This split means:

- Users must create an output directory before running yoinkc.
- The tarball — the artifact that flows into `yoinkc-refine` and
  `yoinkc-build` — is a second-class post-processing step.
- `run-yoinkc.sh` has a fragile positional argument interface (first arg
  is the output dir, everything else is forwarded).
- Tool prerequisites installed by the wrapper (e.g. `tar`) can leak into
  the migration artifact as if they were user intent.

## Design

### Approach

**Renderers write to a temp directory; the pipeline tars at the end.**

Renderers keep their existing interface (`snapshot, env, output_dir`).
The pipeline creates a temp directory, passes it to renderers, runs a
new entitlement cert bundling step, then produces a tarball from the
result. The temp directory is cleaned up.

This was chosen over two alternatives:

- **Renderers write directly to a TarFile object** — significant refactor
  of every renderer for negligible performance gain on small output.
- **Add a `--tar` flag to the existing directory output** — doesn't
  achieve the tarball-as-default goal and adds complexity rather than
  removing it.

### CLI Changes

**Current:**
```
yoinkc --output-dir ./output [--inspect-only] [--from-snapshot FILE]
```

**New:**
```
yoinkc [-o FILE] [--output-dir DIR] [--inspect-only] [--from-snapshot FILE] [--no-entitlement]
```

| Flag | Behavior |
|------|----------|
| *(none)* | Produce `${HOSTNAME}-${TIMESTAMP}.tar.gz` in the current working directory |
| `-o FILE` | Write tarball to the specified path |
| `--output-dir DIR` | Write files to a directory instead of producing a tarball (debug/legacy mode) |
| `--no-entitlement` | Skip bundling RHEL entitlement certs into the output |
| `--inspect-only` | Unchanged — run inspectors, save snapshot only |
| `--from-snapshot` | Unchanged — load snapshot, run renderers only |

`-o` and `--output-dir` are mutually exclusive. Providing both is an
error.

### Pipeline Flow

1. Create a temp directory (`tempfile.mkdtemp`).
2. Run inspectors → snapshot (or load from `--from-snapshot`).
3. Save snapshot to temp dir.
4. Run renderers → write files to temp dir.
5. Bundle entitlement certs into temp dir (unless `--no-entitlement`).
6. **Tarball mode (default):** Create tarball from temp dir, write to
   final location, clean up temp dir.
7. **Directory mode (`--output-dir`):** Move temp dir contents to the
   specified directory, clean up temp dir.

### Tarball Format

The tarball uses the same structure that `run-yoinkc.sh` currently
produces. A single top-level directory named `${HOSTNAME}-${TIMESTAMP}`
containing all output files:

```
hostname-YYYYMMDD-HHMMSS.tar.gz
└── hostname-YYYYMMDD-HHMMSS/
    ├── Containerfile
    ├── config/
    ├── inspection-snapshot.json
    ├── report.html
    ├── audit-report.md
    ├── README.md
    ├── secrets-review.md
    ├── kickstart-suggestion.ks
    ├── quadlet/
    ├── yoinkc-users.toml
    ├── entitlement/          (optional, RHEL only)
    └── rhsm/                 (optional, RHEL only)
```

Python's `tarfile` module produces the tarball — no external `tar`
dependency required.

### Entitlement Cert Bundling

Moves from `run-yoinkc.sh` into yoinkc as a pipeline step between
rendering and tarring.

Detection paths (relative to `HOST_ROOT`):

- `{HOST_ROOT}/etc/pki/entitlement/*.pem` → copied to
  `{temp_dir}/entitlement/`
- `{HOST_ROOT}/etc/rhsm/` → copied to `{temp_dir}/rhsm/`

If certs are not found (non-RHEL host, minimal image), the step is
silently skipped. `--no-entitlement` suppresses the step entirely.

### run-yoinkc.sh Slimdown

**Retains:**
- Check for and install `podman` if missing, with
  `YOINKC_EXCLUDE_PREREQS` tracking so yoinkc excludes tool
  prerequisites from the migration artifact.
- `registry.redhat.io` login checks.
- The `podman run` invocation.

**Removes:**
- `tar` installation (Python `tarfile` replaces it).
- Output directory creation and handling.
- Entitlement cert bundling (moved into yoinkc).
- Tarball creation (moved into yoinkc).
- `--no-entitlement` flag stripping (now a real yoinkc flag, passed
  through).

**Simplified invocation:**
```sh
podman run --rm --pull=always \
  --pid=host --privileged --security-opt label=disable \
  ${YOINKC_DEBUG:+-e YOINKC_DEBUG=1} \
  ${YOINKC_EXCLUDE_PREREQS:+--env YOINKC_EXCLUDE_PREREQS} \
  -v /:/host:ro \
  -v "$(pwd):/output:z" \
  "$IMAGE" "$@"
```

All user arguments pass through directly. The current working directory
is mounted at `/output` so yoinkc can write the tarball there.

The script shrinks from ~157 lines to ~80, with a single
responsibility: ensure podman is available, handle registry auth, launch
the container.

## Compatibility

- `yoinkc-refine` and `yoinkc-build` already accept tarballs — no
  changes needed.
- `--output-dir` preserves the old directory behavior for users or
  scripts that depend on it.
- The tarball internal structure matches what `run-yoinkc.sh` currently
  produces, so existing workflows are unaffected.

## Testing

- Pipeline tests verify tarball mode produces a valid `.tar.gz` with
  expected contents (Containerfile, config/, snapshot, reports).
- Pipeline tests verify `--output-dir` mode still writes a directory.
- Entitlement cert bundling: included when present, skipped when absent,
  suppressed with `--no-entitlement`.
- CLI tests: `-o` and `--output-dir` mutual exclusivity, default naming
  convention.
- Integration: round-trip test — yoinkc produces tarball, `yoinkc-build`
  consumes it.

## Out of Scope

- Changes to `--inspect-only` / `--from-snapshot` behavior.
- stdout/pipe output mode.
- Changes to how `yoinkc-refine` or `yoinkc-build` consume tarballs.
