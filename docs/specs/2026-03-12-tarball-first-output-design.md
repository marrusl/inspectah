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
yoinkc -o ./output [--inspect-only] [--from-snapshot FILE]
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

**Breaking change:** `-o` currently means `--output-dir`. This change
reassigns `-o` to mean "output tarball file." Anyone using `-o ./dir`
must switch to `--output-dir ./dir`. This is acceptable because yoinkc
is pre-1.0 and the tarball is now the primary output mode. The long form
`--output-dir` continues to work unchanged.

#### Interactions with `--validate` and `--push-to-github`

`--validate` runs `podman build` against the output directory.
`--push-to-github` initializes a git repo in the output directory and
pushes it. Both require a persistent directory, not a tarball.

These flags **require `--output-dir`** when used. If either is provided
without `--output-dir`, yoinkc errors with a message explaining that
directory output mode is needed. This keeps the tarball path clean and
avoids hidden temp directories that outlive the process.

### Pipeline Flow

1. Create a temp directory (`tempfile.mkdtemp`).
2. Run inspectors → snapshot (or load from `--from-snapshot`).
3. Save snapshot to temp dir.
4. Run renderers → write files to temp dir.
5. Bundle entitlement certs into temp dir (unless `--no-entitlement`;
   see conditions below).
6. **If tarball mode (default):** Create tarball from temp dir, write to
   final location, clean up temp dir.
7. **If `--output-dir` mode:** Move temp dir contents to the specified
   directory, clean up temp dir. Then run `--validate` and/or
   `--push-to-github` if requested.

**`--inspect-only` short-circuits after step 2:** saves the snapshot to
`inspection-snapshot.json` in the current working directory (not as a
tarball) and exits. No renderers, no entitlement bundling, no tarball.

**Error handling:** If tarball creation fails (disk full, permissions),
the error message includes the temp directory path so the user can
recover the rendered output manually. The temp directory is not cleaned
up on failure.

### Entitlement Cert Bundling

Moves from `run-yoinkc.sh` into yoinkc as a pipeline step between
rendering and tarring.

Detection paths (relative to `HOST_ROOT`):

- `{HOST_ROOT}/etc/pki/entitlement/*.pem` → copied to
  `{temp_dir}/entitlement/`
- `{HOST_ROOT}/etc/rhsm/` → copied to `{temp_dir}/rhsm/`

**Conditions for skipping:**
- `--no-entitlement` flag is set.
- `HOST_ROOT` does not exist or the cert paths are not found (non-RHEL
  host, minimal image). Silently skipped, no warning.
- `--from-snapshot` mode where `HOST_ROOT` is not a real host mount.
  The re-render path (used by `yoinkc-refine`) runs without host
  filesystem access; entitlement bundling is silently skipped.

**Behavioral change from `run-yoinkc.sh`:** The current wrapper copies
certs from the machine running the script (the host, outside the
container). The new code copies from `{HOST_ROOT}` inside the container
(i.e. `/host/etc/pki/entitlement/`). When yoinkc runs directly on the
host (`HOST_ROOT=/`), behavior is identical. When running in a
container, certs come from the inspected host's filesystem via the bind
mount. This is the correct semantic — the certs belong to the host being
migrated, not the build machine.

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
    ├── kickstart-suggestion.ks      (conditional)
    ├── quadlet/                     (conditional, when quadlet units exist)
    ├── yoinkc-users.toml            (conditional, when blueprint-strategy users exist)
    ├── entitlement/                 (conditional, RHEL only)
    └── rhsm/                        (conditional, RHEL only)
```

Python's `tarfile` module produces the tarball — no external `tar`
dependency required.

**Hostname handling:** The hostname is resolved with a fallback chain
similar to the current shell logic: `socket.gethostname()`, falling back
to reading `/etc/hostname`, falling back to `"unknown"`. The hostname is
sanitized to remove characters unsafe for filenames.

### run-yoinkc.sh Slimdown

**Retains:**
- Check for and install `podman` if missing, with
  `YOINKC_EXCLUDE_PREREQS` tracking so yoinkc excludes tool
  prerequisites from the migration artifact.
- `registry.redhat.io` login checks.
- The `podman run` invocation.

**Removes:**
- `tar` installation (Python `tarfile` replaces it). This also means
  `tar` no longer appears in `YOINKC_EXCLUDE_PREREQS`, making the
  exclusion list shorter and more accurate.
- Output directory creation and handling.
- Entitlement cert bundling (moved into yoinkc).
- Tarball creation (moved into yoinkc).
- `--no-entitlement` flag stripping (now a real yoinkc flag, passed
  through).

**Simplified invocation:**
```sh
podman run --rm --pull=always \
  --pid=host --privileged --security-opt label=disable \
  -w /output \
  ${YOINKC_DEBUG:+-e YOINKC_DEBUG=1} \
  ${YOINKC_EXCLUDE_PREREQS:+--env YOINKC_EXCLUDE_PREREQS} \
  -v /:/host:ro \
  -v "$(pwd):/output" \
  "$IMAGE" "$@"
```

The user's current directory is mounted at `/output` and `-w /output`
sets the container's working directory to match, so yoinkc's default
tarball output lands directly in the user's directory with no setup
required. No `:z` volume flag — `--security-opt label=disable` already
disables SELinux confinement, making relabeling unnecessary. Relabeling
the user's CWD would risk corrupting host SELinux contexts.

The script shrinks from ~156 lines to ~80, with a single
responsibility: ensure podman is available, handle registry auth, launch
the container.

## Compatibility

- `yoinkc-refine` and `yoinkc-build` already accept tarballs — no
  changes needed.
- `--output-dir` preserves the old directory behavior for users or
  scripts that depend on it.
- The tarball internal structure matches what `run-yoinkc.sh` currently
  produces, so existing workflows are unaffected.
- **Breaking:** `-o` changes meaning from `--output-dir` to "output
  tarball file." The long form `--output-dir` is unaffected.

## Testing

- Pipeline tests verify tarball mode produces a valid `.tar.gz` with
  expected contents (Containerfile, config/, snapshot, reports).
- Pipeline tests verify `--output-dir` mode still writes a directory.
- Entitlement cert bundling: included when present, skipped when absent,
  suppressed with `--no-entitlement`, skipped in `--from-snapshot` mode.
- CLI tests: `-o` and `--output-dir` mutual exclusivity, default naming
  convention, `--validate`/`--push-to-github` require `--output-dir`.
- Integration: round-trip test — yoinkc produces tarball, `yoinkc-build`
  consumes it.
- Error path: tarball write failure preserves temp directory and reports
  its location.

## Out of Scope

- Changes to `--inspect-only` / `--from-snapshot` behavior.
- stdout/pipe output mode.
- Changes to how `yoinkc-refine` or `yoinkc-build` consume tarballs.
