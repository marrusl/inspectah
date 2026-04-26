# `inspectah build` Subcommand Design

**Status:** Draft
**Date:** 2026-04-26
**Replaces:** `inspectah-build` standalone Python script

## Overview

Integrate the build step into the Go CLI as a native subcommand. One binary
for the full workflow: `inspectah scan` → `inspectah refine` → `inspectah build`.

The existing `inspectah-build` Python script is retired. All build logic moves
into Go with no container or external script dependencies beyond podman.

Supports Linux and macOS hosts. On macOS, builds run through `podman machine`.
Non-entitled builds work on both platforms. RHEL-entitled builds on macOS
require a follow-on spec for the entitlement injection mechanism — see
`Build-time mounting (macOS)` section.

## Usage

```
inspectah build <tarball|directory> -t <image:tag> [flags] [-- extra-podman-args...]
```

## Design Decisions

### Native Go, no container
Build runs directly on the workstation. It needs direct access to the local
podman daemon, entitlement certs, and the build context. No reason to add a
container layer.

### No push
Build produces a local image. Push is a separate concern handled by
`podman push`. inspectah adds no domain-specific value on the push side.

After a successful build, print a hint with the push command.

If inspectah ever needs to do something inspectah-specific at push time
(attestations, scan metadata tagging), that would be a separate `inspectah push`
subcommand — not a flag on build.

### Podman only
Docker support is dropped. inspectah generates bootc images, which are a
podman/buildah concept. Docker can technically build the OCI image but cannot
consume the bootc result end-to-end. Supporting docker adds test surface for
a path that doesn't work.

### No interactive prompts
The Python script prompts for an image name if none is given on a TTY. CLIs
should fail fast with a clear error, not block waiting for input.

## Flags

| Flag | Type | Default | Purpose |
|------|------|---------|---------|
| `--tag, -t` | string | required | Image name:tag |
| `--platform` | string | omitted (defers to podman) | Target os/arch (e.g., `linux/arm64`) |
| `--entitlements-dir` | string | auto-detect | Explicit cert directory |
| `--no-entitlements` | bool | false | Skip entitlement detection |
| `--ignore-expired-certs` | bool | false | Proceed despite expired certs |
| `--no-cache` | bool | false | Skip build cache |
| `--pull` | string | omitted (defers to podman, default `missing`) | Base image pull policy (`always`, `missing`, `never`, `newer`) |
| `--dry-run` | bool | false | Print podman command only |
| `--verbose` | bool | false | Print command before running |
| `--` | passthrough | — | Extra podman build args |

Everything else (e.g., `--squash`, `--layers`, `--secret`) passes through `--`.

## Input Handling

### Tarball (primary path)
Accept `.tar.gz` or `.tgz`. Extract to a temp directory. Clean up on exit,
including SIGINT/SIGTERM signal handling.

**Archive safety:** Only extract regular files and directories. For every
entry, resolve the full destination path and verify it falls within the
extraction root before writing. Reject:

- `..` path components and absolute paths
- Symlinks and hard links (inspectah tarballs contain neither)
- Device nodes, FIFOs, sockets, and other special file types
- Duplicate paths that would overwrite a previously extracted entry

Use Go's `archive/tar` with explicit validation on each entry header.

Validate extracted contents: must contain a `Containerfile`. Fail fast with
a clear error if missing, pointing to `inspectah scan` output format.

### Directory (secondary path)
Accept an existing directory path. Same Containerfile validation. No temp
directory, no cleanup.

### Detection
If the argument ends in `.tar.gz` or `.tgz`, treat as tarball. Otherwise treat
as directory. If neither exists, fail with a usage hint.

## Entitlement Cert Handling

### Discovery and structure
Entitlement configuration consists of two directories:

- `entitlement/` — subscription certificates (`.pem` files), mounted at
  `/etc/pki/entitlement` during build
- `rhsm/` — subscription-manager config, mounted at `/etc/rhsm` during build.
  Optional companion directory; looked for alongside `entitlement/`.

### Auto-discovery cascade
Checked in order, first match wins. CLI flag and env var take precedence
over filesystem locations (standard CLI convention):

1. CLI flag — `--entitlements-dir <path>`
2. Environment variable — `INSPECTAH_ENTITLEMENT_DIR`

If neither flag nor env var is set, check for RHEL host auto-detection
before falling through to filesystem locations:

3. RHEL host auto-detection — if `/etc/pki/entitlement/*.pem` exists and
   contains valid certs, the host is subscribed and podman handles entitlement
   natively. Skip all mounting — return early with no explicit cert paths.
   This matches current `inspectah-build` behavior.
4. Bundled in tarball — `<output>/entitlement/`
5. User config — `~/.config/inspectah/entitlement/`

At each cascade level (1-2, 4-5), also check for a sibling `rhsm/`
directory. If found, include it in the build mounts.

`--no-entitlements` skips the cascade entirely.

### RHEL detection
Parse all `FROM` directives in the Containerfile. A stage requires
entitlements if it references `registry.redhat.io` AND the image is not
a UBI (Universal Base Image) variant. UBI images (`ubi8`, `ubi9`,
`ubi-minimal`, `ubi-micro`, `ubi-init`) are freely available from
`registry.redhat.io` without subscription. The parser must handle:

- `ARG`-substituted registry references (resolve `${REGISTRY}` if default
  value is defined in the Containerfile)
- `FROM --platform=... <image>` syntax
- Multi-stage builds (check every `FROM`, not just the last)
- Comments and blank lines (skip them)

If `ARG` substitution cannot be resolved statically (no default value),
treat the stage as ambiguous. Behavior: if certs are found via the
discovery cascade, mount them silently. If certs are not found, warn
but proceed — same as the definite-RHEL case.

### Cert validation
Validate expiry using Go's `crypto/x509` stdlib — no openssl dependency.

- **Expired certs:** fail with a clear message. Show expiry date, which certs
  are stale, and the `subscription-manager refresh` fix. `--ignore-expired-certs`
  overrides.
- **No certs found + definite RHEL base image:** warn with instructions on
  where to place certs, but proceed. The build may still succeed if the
  Containerfile doesn't install packages, or if the host handles entitlement
  natively (Satellite, SCA, etc.). This matches current `inspectah-build`
  behavior.
- **No certs found + ambiguous base image (unresolved ARG):** warn but proceed.
- **No certs found + non-RHEL base image:** proceed silently.

### Build-time mounting (Linux)
When certs are discovered via the cascade (levels 1-4) and need explicit
mounting, use `podman build -v` with `:ro` only (no `:Z` — do not relabel
system directories):

- `-v <entitlement-dir>:/etc/pki/entitlement:ro`
- `-v <rhsm-dir>:/etc/rhsm:ro` (if `rhsm/` found)

When the host handles entitlement natively (level 0), no mount flags are
added — podman's built-in subscription handling takes over.

### Build-time mounting (macOS)
macOS builds run through the podman remote client to a `podman machine` VM.
The remote client does not support `-v` with host paths during `podman build`.

The macOS entitlement injection mechanism is defined in a follow-on spec:
**`macOS build execution` (TBD)**. That spec must lock down:

- The exact injection mechanism (secret mount, build-context staging, or
  Containerfile wrapping)
- Whether inspectah stages a temporary build context outside the user's
  directory
- How `entitlement/` and `rhsm/` are consumed during `RUN dnf ...`
- Cleanup guarantees so secrets never persist in image layers or user workspace
- How unsupported passthrough args are handled (especially `-v` host binds)
- Test matrix for tarball vs directory input on macOS

Until the follow-on spec is complete, `inspectah build` on macOS supports
non-entitled builds only (Fedora, CentOS Stream, UBI). Boundary rules:

- **Definite RHEL base image:** error directing the user to build on a
  Linux host or use `--no-entitlements` if the Containerfile doesn't need
  subscribed repos.
- **Ambiguous base image (unresolved ARG):** warn that entitlement injection
  is not supported on macOS and proceed without certs. If the build fails
  due to missing entitlements, the error will be clear.
- **Passthrough `-v` with host paths:** on macOS, scan passthrough args for
  `-v`/`--volume` flags with host paths and warn that they may not work with
  the podman remote client. Do not block — some `-v` patterns work (e.g.,
  paths within the podman machine's shared mounts), and podman will report
  the error if they don't.

## Cross-Architecture Builds

`--platform` supports building for a different architecture than the host.
Cross-arch builds use QEMU user-mode emulation via `qemu-user-static` +
`binfmt_misc`.

### Preflight
Platform-dependent verification:

- **Linux:** Check that `qemu-user-static` is installed and the target arch's
  binfmt handler is registered (`/proc/sys/fs/binfmt_misc/qemu-*`). Fail fast
  with install instructions if not.
- **macOS:** Cross-arch support depends on the `podman machine` VM configuration.
  QEMU emulation is typically pre-configured in the VM. Skip the binfmt check
  (not accessible from the macOS host) and let podman report any errors directly.

### Arch mismatch warning
If `--platform` doesn't match the scanned host's architecture (detectable
from tarball metadata), print a warning but proceed — this is a valid use
case.

### Performance note
Print: `Note: Building <target> on <host> via QEMU — build will be slower.`

## Output Behavior

### Success
```
Built: localhost/my-migration:latest (847 MB)

Next steps:
  Run:    podman run -it localhost/my-migration:latest
  Switch: bootc switch <registry>/my-migration:latest
  Test:   bcvk ephemeral run-ssh localhost/my-migration:latest
  Push:   podman push localhost/my-migration:latest <registry>/my-migration:latest
```

### Cert expiry
```
Error: RHEL entitlement cert expired (2026-04-20)
  Certs: /etc/pki/entitlement/123456.pem
  Fix:   sudo subscription-manager refresh
  Skip:  inspectah build --ignore-expired-certs ...
```

### Cross-arch
```
Note: Building linux/arm64 on linux/amd64 via QEMU — build will be slower.
```

### Missing podman
```
Error: podman not found
  Linux:  sudo dnf install podman
  macOS:  brew install podman && podman machine init && podman machine start
```

### Missing Containerfile
```
Error: No Containerfile found in <path>
  This doesn't look like inspectah output. Run 'inspectah scan' first.
```

## Build Execution

Assemble the `podman build` command with:
- `-f <dir>/Containerfile`
- `-t <tag>`
- `--platform <platform>` (if specified)
- `--no-cache` (if specified)
- `--pull <policy>` (if specified)
- `-v <entitlement-dir>:/etc/pki/entitlement:ro` (Linux, if certs discovered)
- `-v <rhsm-dir>:/etc/rhsm:ro` (Linux, if `rhsm/` found)
- No mount flags when host handles entitlement natively (cascade level 3)
- On macOS, entitlement injection deferred to follow-on spec
- Any `--` passthrough args
- Build context: the output directory

Execute via `os/exec` (not `syscall.Exec` — we need to capture exit code and
print the success message after).

## What This Replaces

The standalone `inspectah-build` Python script is retired. Feature mapping:

| Python script feature | Go CLI |
|----------------------|--------|
| Tarball extraction | ✓ |
| Directory input | ✓ |
| RHEL entitlement detection | ✓ |
| Cert expiry validation (openssl) | ✓ (Go crypto/x509) |
| Runtime selection (podman/docker) | Podman only |
| Registry push | Dropped (use `podman push`) |
| Interactive name prompt | Dropped (fail fast) |
| Signal handling / cleanup | ✓ |
| `--dry-run` | ✓ |
| `--no-cache` | ✓ |
| `--pull` policy | ✓ |
| Cross-arch `--platform` | ✓ (new) |

## Implementation Notes

- Entitlement handling is inline in the build command package. No premature
  abstraction into a shared package — refactor if/when another consumer needs it.
- The existing thin `build.go` is replaced, not extended.
- `--` passthrough follows the same pattern as `scan` and `refine`.
