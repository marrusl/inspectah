# `inspectah build` Subcommand Design

**Status:** Approved
**Date:** 2026-04-26
**Replaces:** `inspectah-build` standalone Python script

## Overview

Integrate the build step into the Go CLI as a native subcommand. One binary
for the full workflow: `inspectah scan` → `inspectah refine` → `inspectah build`.

The existing `inspectah-build` Python script is retired. All build logic moves
into Go with no container or external script dependencies beyond podman.

Supports Linux and macOS hosts. On macOS, builds run through `podman machine`.
Entitled and non-entitled builds work on both platforms.

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
Accept `.tar.gz` or `.tgz`. Extract to a temp directory under `$HOME`
(ensures paths are accessible to `podman machine` on macOS). Clean up on
exit, including SIGINT/SIGTERM signal handling.

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

### Structure
Entitlement configuration consists of two directories:

- `entitlement/` — subscription certificates (`.pem` files), mounted at
  `/etc/pki/entitlement` during build
- `rhsm/` — subscription-manager config, mounted at `/etc/rhsm` during build.
  Optional companion directory; looked for alongside `entitlement/`.

### RHEL detection
Parse all `FROM` directives in the Containerfile to classify the build as
`entitled`, `ambiguous`, or `non-entitled`.

**Entitled:** any stage references `registry.redhat.io` with a non-UBI
image. UBI images are freely available without subscription — match by
repo path pattern: `registry.redhat.io/ubi{7,8,9}`, and anything under
`registry.redhat.io/ubi{7,8,9}/*` (covers `ubi9/ubi`, `ubi9/ubi-minimal`,
`ubi8/ubi-micro`, etc.).

**Ambiguous:** at least one stage has an image reference that cannot be
classified as entitled or non-entitled. This includes unresolved `ARG`
substitution (no default value) and mixed cases (e.g., one known public
stage plus one unresolved stage). Any unresolved stage makes the overall
build ambiguous, even if other stages are definitively non-entitled.

**Non-entitled:** all stages are definitively non-RHEL or UBI. No
unresolved references remain.

The parser must handle:
- `ARG`-substituted references (resolve if default value defined in the
  Containerfile; do NOT attempt to resolve `--build-arg` passthrough values
  — those are runtime inputs invisible to static analysis)
- `FROM --platform=... <image>` syntax
- Multi-stage builds (check every `FROM`)
- Comments and blank lines (skip them)

Note: `--build-arg` values passed via `--` can change the image reference
at invocation time, turning an ambiguous build into entitled or non-entitled.
Static analysis cannot account for this — the ambiguous classification and
its warn-and-proceed behavior cover this case.

### Cert discovery cascade
Checked in order, first match wins. If a flag or env var points to a path
that does not exist or contains no `.pem` files, fail immediately — an
explicit override that doesn't resolve is a user error, not a fallthrough
signal.

**`--no-entitlements`** skips the entire cascade AND overrides the
entitlement detection result to `non-entitled`, regardless of what the
Containerfile says. This is a deliberate escape hatch: the user is
asserting that the build does not need subscription repos. On any
platform, `--no-entitlements` produces a clean `non-entitled + no-certs`
path — no warnings, no errors, no cert mounting. Mutually exclusive with
`--entitlements-dir` (error if both specified).

1. CLI flag — `--entitlements-dir <path>` (fail if path invalid)
2. Environment variable — `INSPECTAH_ENTITLEMENT_DIR` (fail if path invalid)
3. RHEL host native — if `/etc/pki/entitlement/*.pem` exists with valid
   certs, the host is subscribed. Mark discovery result as `host-native`.
4. Bundled in tarball — `<output>/entitlement/`
5. User config — `~/.config/inspectah/entitlement/`

At levels 1-2 and 4-5, also check for a sibling `rhsm/` directory.

### Cert validation
When certs are discovered (levels 1-2, 4-5), validate expiry using Go's
`crypto/x509` stdlib. Expired certs fail with a clear message (expiry date,
which certs, `subscription-manager refresh` hint). `--ignore-expired-certs`
overrides.

Host-native entitlement (level 3) does not perform expiry validation —
podman and subscription-manager handle cert lifecycle on subscribed hosts.

### Platform-specific decision matrix

The entitlement detection result (`entitled` / `ambiguous` / `non-entitled`)
and the discovery result (`certs-found` / `host-native` / `no-certs`) combine
with the platform to produce one action:

**Linux:**

| Detection | Certs found | Host-native | No certs |
|-----------|-------------|-------------|----------|
| Entitled | Mount via `-v :ro` | No mount flags (podman handles) | Warn, proceed |
| Ambiguous | Mount via `-v :ro` | No mount flags | Warn, proceed |
| Non-entitled | Ignore certs | Ignore | Proceed silently |

**macOS:**

| Detection | Certs found | Host-native | No certs |
|-----------|-------------|-------------|----------|
| Entitled | Mount via `-v :ro` | N/A (macOS has no host entitlement) | Warn, proceed (or `--no-entitlements` to silence) |
| Ambiguous | Mount via `-v :ro` | N/A | Warn, proceed |
| Non-entitled | Ignore certs | N/A | Proceed silently |

macOS entitled builds work because `podman machine` shares the user's
home directory into the VM by default. On macOS, all paths used for `-v`
mounts must be under `$HOME` (or another explicitly shared podman machine
mount). This is handled at two points:

- **Tarball extraction:** uses a temp directory under `$HOME`.
- **Cert discovery and `--entitlements-dir`:** on macOS, preflight the
  resolved entitlement path. If it falls outside `$HOME`, warn that the
  path may not be accessible to `podman machine` and suggest copying certs
  to a location under `$HOME`.

**Mount flags** (both platforms):
- `-v <entitlement-dir>:/etc/pki/entitlement:ro`
- `-v <rhsm-dir>:/etc/rhsm:ro` (if `rhsm/` found)

No `:Z` relabeling — do not relabel system directories.

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

### Cross-arch note
When `--platform` specifies a different architecture than the CLI host,
print: `Note: Building <target> on <host> via QEMU — build will be slower.`

The current tarball format does not persist the scanned host's architecture,
so no scan-vs-target mismatch warning is possible. This could be added in
a future schema revision.

## Output Behavior

### Success
```
Built: localhost/my-migration:latest

Next steps:
  Test:   bcvk ephemeral run-ssh localhost/my-migration:latest
  Switch: bootc switch localhost/my-migration:latest
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
- Entitlement volume mounts per the platform-specific decision matrix
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
