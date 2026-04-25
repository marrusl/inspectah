# Go CLI Wrapper for inspectah

**Date:** 2026-04-25
**Status:** Accepted
**Author:** inspectah contributors

## Summary

Replace `run-inspectah.sh` with a compiled Go binary that wraps podman execution
transparently. The binary installs via RPM or Homebrew as `inspectah`, provides
cobra-generated tab completion, translates container errors into human-readable
messages, and surfaces progress feedback so the user never stares at a silent
terminal. The Python tool continues to run inside the container; the Go wrapper
is strictly an orchestration layer.

## Motivation

### Why the shell script falls short

`run-inspectah.sh` is a curl-pipe-sh bootstrap. It works, but:

- **No discoverability.** `inspectah --help` requires the container image to
  already be pulled. Tab completion doesn't exist. A sysadmin on RHEL 8 has
  no way to explore the CLI without reading docs.
- **Raw error output.** When podman fails (missing image, permission denied,
  SELinux denial), the user sees unfiltered container stderr. A "permission
  denied" from inside the container namespace means nothing without context.
- **No progress feedback.** Image pulls, inspection runs, and builds produce
  no progress indication. A 2GB image pull on a slow link looks like a hang.
- **No version management.** There's no mechanism to pin, check, or upgrade
  the container image version. No warning when the wrapper and container
  drift apart.
- **Platform friction.** The shell script assumes a POSIX environment. macOS
  users on zsh hit quoting edge cases. Windows/WSL users are unsupported.

### Why Go

- **Single static binary.** No runtime dependencies beyond podman. Critical for
  RHEL 8 where Python 3.11+ doesn't exist in base repos.
- **Cross-compilation.** One `GOOS/GOARCH` matrix covers linux/amd64,
  linux/aarch64, darwin/amd64, darwin/arm64.
- **Cobra ecosystem.** Structured subcommands, automatic `--help` generation,
  and shell completion for bash/zsh/fish with zero hand-maintained scripts.
- **Proven pattern.** This is the same approach as `podman` itself, `kubectl`,
  `oc`, and most modern CLI tools in the RHEL ecosystem.

### Container indirection as a feature

The wrapper doesn't hide the container -- it embraces it. The migration tool
runs the way the user's workloads will run after migration: as a container image
managed by podman. This is the toolbox/distrobox pattern, proven and familiar to
the target audience.

## Target Platforms

| Platform       | Arch          | Distribution    | Podman Version |
|----------------|---------------|-----------------|----------------|
| RHEL 8         | x86_64        | RPM (COPR)      | 4.x            |
| RHEL 9         | x86_64, aarch64 | RPM (COPR)    | 4.x / 5.x     |
| CentOS Stream 9 | x86_64, aarch64 | RPM (COPR)   | 4.x / 5.x     |
| RHEL 10        | x86_64, aarch64 | RPM (COPR)    | 5.x            |
| Fedora 41+     | x86_64, aarch64 | RPM (COPR)    | 5.x            |
| macOS 13+      | arm64, amd64  | Homebrew        | 5.x (podman machine) |

**RHEL 8 note:** Podman 4.x on EL8 uses cgroups v1 by default and has some
`--pid=host` behavioral differences. The wrapper must handle both podman 4.x
and 5.x command-line interfaces. The minimum supported podman version is 4.4.0
(EL8.8 baseline).

**macOS note:** macOS is a first-class platform for `fleet`, `refine`,
`architect`, and `build` — subcommands that operate on previously-collected
tarballs. The `scan` subcommand requires direct access to the host root
filesystem and is Linux-only. On macOS, `inspectah scan` exits immediately
with a clear error:

```
Error: scan requires a Linux host — use on a RHEL/CentOS system.

The scan subcommand inspects the host root filesystem directly and is
not supported on macOS. To scan a remote host, run inspectah there and
use 'inspectah refine' or 'inspectah fleet' on macOS to analyze the results.
```

## Architecture

### Process model

```
User
  |
  v
inspectah (Go binary)
  |
  |-- validates args, resolves paths
  |-- checks/pulls container image
  |-- constructs podman run command
  |-- exec's podman (replaces process for scan/fleet)
  |      or
  |-- spawns podman as child (for refine/architect -- needs port management)
  |
  v
podman run ghcr.io/marrusl/inspectah:0.5.1 <subcommand> <args>
  |
  v
inspectah (Python, inside container)
```

### Execution strategies

**Strategy A -- exec replacement (scan, fleet):**
The Go process constructs the full `podman run` invocation and calls
`syscall.Exec()` to replace itself with podman. This gives podman direct
control of the terminal (stdin/stdout/stderr), which is critical for
interactive prompts and signal handling. The Go wrapper's job is done once
podman starts.

**Strategy B -- child process (refine, architect):**
For subcommands that run a web server (refine on port 8642, architect on
port 8643), the wrapper spawns podman as a child process. This allows the
wrapper to:
- Poll for server readiness before opening the browser
- Forward SIGINT/SIGTERM to the child for clean shutdown
- Print the server URL and status to the user

```go
// Signal forwarding for child-process mode
sigCh := make(chan os.Signal, 1)
signal.Notify(sigCh, syscall.SIGINT, syscall.SIGTERM)
go func() {
    sig := <-sigCh
    cmd.Process.Signal(sig)
}()
```

### Repository layout

The Go wrapper lives in the inspectah repo alongside the Python code, as a
separate Go module. One repo, one product, one release cadence. The Go module
has its own `go.mod` under `cmd/inspectah/`, keeping Go and Python toolchain
concerns separate in CI while sharing the release workflow.

```
# Repo root (existing Python project)
inspectah/
  ...                   # Existing Python source
  cmd/
    inspectah/
      main.go           # Entry point
      go.mod            # Separate Go module
      go.sum
      internal/
        container/
          run.go        # podman invocation builder
          image.go      # Image pull, version check, air-gap load
          progress.go   # Pull progress parsing
        cli/
          root.go       # Root cobra command
          scan.go       # scan subcommand
          fleet.go      # fleet subcommand
          refine.go     # refine subcommand
          architect.go  # architect subcommand
          build.go      # build subcommand
          image.go      # image subcommand family (use, update, info)
          version.go    # version subcommand
          completion.go # completion subcommand (cobra built-in)
        errors/
          errors.go     # Structured error types
          translate.go  # Container error → user message translation
        paths/
          resolve.go    # Host path ↔ container path mapping
        platform/
          detect.go     # Platform detection (Linux vs macOS gating)
        version/
          check.go      # Version compatibility checking
```

## Subcommand Surface

The Go wrapper mirrors the Python CLI's subcommand structure. Each wrapper
subcommand maps 1:1 to a container subcommand.

### Command mapping

```
inspectah scan [flags]          → podman run ... inspectah scan [flags]
inspectah fleet <dir> [flags]   → podman run ... inspectah fleet /input [flags]
inspectah refine <tarball>      → podman run ... inspectah refine /input/<name> [flags]
inspectah architect <input>     → podman run ... inspectah architect /input [flags]
inspectah build <output-dir>    → podman build on generated Containerfile (wrapper-owned)
inspectah image use <version>   → pin container image to a specific version
inspectah image update          → pull latest compatible container image
inspectah image info            → show current image details and available updates
inspectah version               → local version + container image version
inspectah completion <shell>    → cobra-generated completion script
```

### Flags the wrapper owns (not passed to container)

| Flag                  | Scope   | Description                                    |
|-----------------------|---------|------------------------------------------------|
| `--image`             | Global  | Override container image (default: ghcr.io/marrusl/inspectah:TAG) |
| `--podman`            | Global  | Path to podman binary (default: auto-detect)   |
| `--pull`              | Global  | Pull policy: `always`, `missing`, `never` (default: `missing`) |
| `--verbose`           | Global  | Show podman command being executed              |
| `--dry-run`           | Global  | Print podman command without executing          |

### Flags the wrapper intercepts and translates

| Wrapper flag          | Container equivalent | Translation                        |
|-----------------------|---------------------|------------------------------------|
| `-o <path>`           | `-o /output/<name>` | Bind-mounts CWD, rewrites path     |
| `--output-dir <path>` | `--output-dir /output/dir` | Bind-mounts target dir     |
| `<tarball>` (refine)  | `/input/<name>`     | Bind-mounts tarball, rewrites path |
| `<dir>` (fleet)       | `/input`            | Bind-mounts directory              |

All other flags are passed through to the container verbatim. The wrapper
does not validate Python-side flags -- the container's argparse handles that.
This keeps the wrapper decoupled from inspectah's evolving flag surface.

### `build` subcommand

`build` is a first-class wrapper subcommand that takes an inspectah output
directory (the extracted tarball from a previous `scan`) and runs
`podman build` against the generated Containerfile. This is the natural next
step after scanning: inspect a host, then build the target image.

```
inspectah build <output-dir> [flags]
```

**Flags:**

| Flag              | Description                                              |
|-------------------|----------------------------------------------------------|
| `--tag`, `-t`     | Tag for the built image (default: derived from scan metadata) |
| `--no-cache`      | Pass `--no-cache` to `podman build`                      |
| `--pull`          | Pull policy for the base image during `podman build` (default: `missing`). This is independent of the global `--pull` flag, which controls the inspectah scanner image pull policy. |
| `--platform`      | Target platform (default: native, e.g., `linux/amd64`)   |
| `--squash`        | Squash layers in final image                             |

**Behavior:**

1. Validate `<output-dir>` exists and contains a `Containerfile` (or
   `Dockerfile`). Error with guidance if not:

   ```
   Error: no Containerfile found in ./my-host-output/

   Expected a directory from a previous 'inspectah scan' run.
   Run 'inspectah scan' first, then extract the output tarball.
   ```

2. Construct and execute `podman build`:

   ```bash
   podman build \
     -t inspectah-built/my-host:latest \
     -f ./my-host-output/Containerfile \
     ./my-host-output/
   ```

3. Stream build output to stdout in real time (child process mode, not exec
   replacement — the wrapper needs to report success/failure after build
   completes).

4. On success, print the built image reference:

   ```
   Build complete: inspectah-built/my-host:latest

   Next steps:
     podman run -it inspectah-built/my-host:latest /bin/bash   # test it
     podman push inspectah-built/my-host:latest <registry>     # ship it
   ```

5. On failure, translate common `podman build` errors (missing base image,
   syntax errors in Containerfile, disk space) using the same error
   translation infrastructure.

**Platform gating:** `build` works on both Linux and macOS. On macOS, it
runs against the podman machine VM via the standard podman remote interface.

**Not a container passthrough:** Unlike `scan`, `fleet`, `refine`, and
`architect`, the `build` subcommand does NOT exec into the inspectah
container image. It runs `podman build` directly on the host (or podman
machine). The inspectah container is a scanner, not a builder.

### `image` subcommand family

The `image` subcommand manages the inspectah container image lifecycle with
explicit version pinning from day one. Users must be able to target a
specific container image version, not just "latest".

```
inspectah image use <version>    # Pin to a specific version
inspectah image update           # Pull latest compatible version
inspectah image info             # Show current image details
```

#### `inspectah image use <version>`

Pins the wrapper to a specific container image version. The version is
persisted to a local config file (`~/.config/inspectah/config.yaml`).

**Version normalization:** Both `v0.5.1` and `0.5.1` are accepted as input.
The `v` prefix is stripped for registry tag resolution (OCI tags are bare
semver), but display output uses the `v` prefix for consistency with Go
tooling conventions.

```
$ inspectah image use v0.5.1
Pulling ghcr.io/marrusl/inspectah:0.5.1...
  [############################]  100%  245/245 MB

Image pinned to v0.5.1.
Subsequent commands will use ghcr.io/marrusl/inspectah:0.5.1

$ inspectah image use v0.4.0
Warning: v0.4.0 is older than this wrapper's minimum compatible
version (0.5.0). Some features may not work correctly.

Pin anyway? [y/N]
```

The pinned version takes precedence over the compiled default but is
overridden by `--image` and `INSPECTAH_IMAGE`. Resolution order becomes:

1. `--image` flag (explicit override)
2. `INSPECTAH_IMAGE` environment variable
3. Pinned version from `~/.config/inspectah/config.yaml`
4. Compiled default (`ghcr.io/marrusl/inspectah:<version>`)

#### `inspectah image update`

Pulls the latest tag matching the wrapper's compatibility range. If a version
is pinned, `update` pulls the latest within the pinned major version unless
`--latest` is passed.

**Behavior matrix:**

| State    | Flag        | Behavior                                              |
|----------|-------------|-------------------------------------------------------|
| Unpinned | (default)   | Pull latest within wrapper compatibility range        |
| Unpinned | `--latest`  | Same — no pin to constrain, pulls latest compatible   |
| Pinned   | (default)   | Pull latest within pinned major version               |
| Pinned   | `--latest`  | Ignore pin, pull latest compatible, update pin to match |

```
$ inspectah image update
Checking for updates...
Current: v0.5.1
Latest compatible: v0.6.0

Pulling ghcr.io/marrusl/inspectah:0.6.0...
  [############################]  100%  252/252 MB

Updated to v0.6.0. Pin updated.
```

#### `inspectah image info`

Shows detailed information about the current container image state:

```
$ inspectah image info
Image:     ghcr.io/marrusl/inspectah:0.5.1
Status:    pulled (local)
Pinned:    yes (via 'inspectah image use v0.5.1')
Pulled:    2026-04-20T14:30:00Z
Size:      245 MB
Digest:    sha256:abc123...

Wrapper compatibility: v0.5.0 – v0.99.99
Update available:      v0.6.0

Run 'inspectah image update' to pull the latest version.
Run 'inspectah image use <version>' to pin a specific version.
```

### Backwards compatibility

Bare flags without a subcommand (e.g., `inspectah --from-snapshot foo.json`)
are treated as `scan`, matching the Python CLI's behavior:

```go
// If first arg looks like a flag, prepend "scan"
if len(os.Args) > 1 && strings.HasPrefix(os.Args[1], "-") {
    os.Args = append([]string{os.Args[0], "scan"}, os.Args[1:]...)
}
```

## Container Lifecycle

### Image resolution

The default image reference is compiled into the binary at build time:

```go
var (
    DefaultRegistry = "ghcr.io"
    DefaultRepo     = "marrusl/inspectah"
    DefaultTag      = "0.5.1"  // Set by -ldflags at build time
)
```

Resolution order:
1. `--image` flag (explicit override)
2. `INSPECTAH_IMAGE` environment variable (backwards compat with run-inspectah.sh)
3. Pinned version from `~/.config/inspectah/config.yaml` (set via `inspectah image use`)
4. Compiled default (`ghcr.io/marrusl/inspectah:<version>`)

### Pull behavior

The `--pull` flag controls when the wrapper pulls the image:

| Value     | Behavior                                          |
|-----------|---------------------------------------------------|
| `missing` | Pull only if image not present locally (default)  |
| `always`  | Always pull (matches run-inspectah.sh behavior)   |
| `never`   | Never pull; fail if image missing (air-gap mode)  |

The default is `missing` rather than `always` (which is what the shell script
does) because repeated pulls on slow networks are the #1 UX complaint. Users
who want freshness can use `inspectah scan --pull=always` or the explicit
update command.

### Update checking

`inspectah version` shows the current state. `inspectah image info`
gives detailed image information including available updates:

```
$ inspectah version
inspectah wrapper  v1.0.0 (go1.22, linux/amd64)
inspectah image    v0.5.1 (ghcr.io/marrusl/inspectah:0.5.1)

$ inspectah version --check
inspectah wrapper  v1.0.0 (go1.22, linux/amd64)
inspectah image    v0.5.1 (ghcr.io/marrusl/inspectah:0.5.1)

Update available: v0.6.0
  Run: inspectah image update
```

The `image` subcommand family handles all image lifecycle operations.
See the Subcommand Surface section for full details on `image use`,
`image update`, and `image info`.

### Air-gap support

For disconnected environments:

```bash
# On a connected machine:
podman pull ghcr.io/marrusl/inspectah:0.5.1
podman save ghcr.io/marrusl/inspectah:0.5.1 -o inspectah-0.5.1.tar

# Transfer inspectah-0.5.1.tar to disconnected host

# On the disconnected host:
podman load -i inspectah-0.5.1.tar
inspectah scan --pull=never
```

The wrapper detects `--pull=never` and skips all network operations. If the
image is not present locally, the error message includes the `podman load`
instructions:

```
Error: inspectah container image not found locally.

The image ghcr.io/marrusl/inspectah:0.5.1 is not available and --pull=never
was specified.

For air-gapped environments, load the image from a tarball:
  podman load -i inspectah-0.5.1.tar

To download the image on a connected machine:
  podman pull ghcr.io/marrusl/inspectah:0.5.1
  podman save ghcr.io/marrusl/inspectah:0.5.1 -o inspectah-0.5.1.tar
```

## Progress & Feedback

This is a P0 UX requirement. The user should never stare at a silent terminal
wondering if something is happening.

### Image pull progress

When pulling, the wrapper intercepts podman's pull output and renders a
compact progress display:

```
Pulling ghcr.io/marrusl/inspectah:0.5.1...
  [################............]  58%  142 MB / 245 MB  12.3 MB/s
```

Implementation: Run `podman pull` as a child process, parse its stdout
line-by-line. Podman emits layer-by-layer progress in a machine-parseable
format when stdout is not a TTY. When stdout IS a TTY, podman renders its
own progress bars, which the wrapper passes through directly.

For non-TTY contexts (CI, logging), the wrapper emits periodic one-line
status updates:

```
Pulling ghcr.io/marrusl/inspectah:0.5.1... 58% (142/245 MB)
Pulling ghcr.io/marrusl/inspectah:0.5.1... 100% (245/245 MB)
Pulling ghcr.io/marrusl/inspectah:0.5.1... done (23.4s)
```

### Inspection progress passthrough

The Python inspectah tool writes structured progress to stderr during scan:

```
[1/7] Collecting system info...
[2/7] Scanning packages...
[3/7] Analyzing configs...
```

The wrapper passes stderr through unmodified. This is a design constraint:
the wrapper must never swallow or buffer stderr from the container, because
that's where inspectah's own progress reporting lives.

### Build progress

The `build` subcommand runs `podman build` as a child process and streams
build output to stdout in real time. The wrapper reports success or failure
after the build completes with a summary line and next-step guidance.

### First-run experience

On first invocation (image not present locally), the wrapper provides
additional context:

```
$ inspectah scan
inspectah v1.0.0 — first run setup

Pulling container image (this may take a minute on slow connections)...
  [################............]  58%  142 MB / 245 MB  12.3 MB/s

Image ready. Starting scan...
[1/7] Collecting system info...
```

The "first run" state is detected by checking whether the image exists
locally before pulling.

### Server-mode feedback (refine, architect)

For subcommands that start a web server:

```
$ inspectah refine my-host.tar.gz
Starting inspectah refine server...
  Report will be at: http://localhost:8642

Waiting for server... ready (1.2s)
Opening browser...

Press Ctrl+C to stop.
```

## Error Handling

Container failures are caught, classified, and translated to actionable
messages. Raw podman stderr is preserved and shown with `--verbose` or when
the error can't be classified.

### Structured error types

All user-visible errors use structured types from day one. `*WrapperError`
is for the CLI boundary — the errors users see. Internal helpers can use
standard `fmt.Errorf` with `%w` wrapping and convert to `*WrapperError` at
the point where the error surfaces to the user. This keeps internal code
idiomatic while making user-facing errors classifiable for future telemetry
retrofitting.

```go
// errors/errors.go

// ErrorKind classifies errors for translation and future telemetry.
type ErrorKind int

const (
    ErrPodmanNotFound    ErrorKind = iota // podman binary missing
    ErrPodmanStart                        // podman failed to start container
    ErrPermissionDenied                   // insufficient privileges
    ErrImageNotFound                      // container image not available
    ErrDiskSpace                          // ENOSPC
    ErrSELinux                            // AVC denial
    ErrNetworkTimeout                     // pull timed out
    ErrPortConflict                       // port already in use
    ErrOOMKilled                          // container OOM
    ErrBuildFailed                        // podman build failure
    ErrPlatformUnsupported                // scan on macOS
    ErrIncompatibleVersion                // image/wrapper version mismatch
    ErrUnclassified                       // fallback
)

// WrapperError is the structured error type used throughout the wrapper.
type WrapperError struct {
    Kind       ErrorKind
    Message    string   // User-facing one-line summary
    Suggestion string   // Actionable next step
    Stderr     string   // Raw stderr from podman (shown with --verbose)
    ExitCode   int      // Container/podman exit code
}

func (e *WrapperError) Error() string {
    return e.Message
}
```

The `classify()` function in `translate.go` maps exit codes and stderr
patterns to `ErrorKind` values. The renderer formats `WrapperError` into
the user-facing output structure described below.

### Error translation table

| Container exit / error               | User-facing message                                         |
|--------------------------------------|-------------------------------------------------------------|
| `podman: command not found`          | `Error: podman is required but not installed.`<br>`Install it with: sudo dnf install podman` |
| Exit 125 (podman error)             | `Error: podman failed to start the container.`<br>`Run with --verbose to see the full podman command.` |
| Exit 126 (permission denied)        | `Error: permission denied. The scan subcommand requires root.`<br>`Run with: sudo inspectah scan` |
| Exit 127 (image not found)          | `Error: container image not found.`<br>`Run: inspectah image update` |
| ENOSPC                               | `Error: not enough disk space for the container image.`<br>`Free up space and try again.` |
| SELinux AVC denial                   | `Error: SELinux is blocking container access to the host filesystem.`<br>`The container runs with --security-opt label=disable to avoid this.`<br>`If this persists, check for custom SELinux policies.` |
| Network timeout on pull              | `Error: image pull timed out. Check your network connection.`<br>`For offline use: inspectah scan --pull=never` |
| Port already in use (refine)         | `Error: port 8642 is already in use.`<br>`Use --port to specify a different port: inspectah refine --port 8643 report.tar.gz` |
| Container OOM killed                 | `Error: the container ran out of memory.`<br>`Try increasing container memory limits or reducing the scan scope.` |
| `scan` on macOS                      | `Error: scan requires a Linux host — use on a RHEL/CentOS system.` |
| Build Containerfile missing          | `Error: no Containerfile found in <dir>.`<br>`Run 'inspectah scan' first, then extract the output tarball.` |

### Error output structure

```
Error: <one-line summary>

<actionable suggestion>

Details (run with --verbose for full output):
  <abbreviated stderr, max 10 lines>
```

### Unclassified errors

When the exit code and stderr don't match a known pattern, the wrapper falls
back to showing the raw error with context:

```
Error: inspectah exited with code 1.

Container stderr:
  <full stderr output>

If this looks like a bug, file an issue:
  https://github.com/marrusl/inspectah/issues
```

## Output Path Mapping

The wrapper translates between host paths and container paths so that output
lands where the user expects.

### Scan output

The user's CWD is bind-mounted to `/output` inside the container. The
container writes to `/output`, which maps back to the user's CWD:

```go
mounts := []string{
    fmt.Sprintf("%s:/output", cwd),   // Output lands in user's CWD
    "/:/host:ro",                      // Host root for inspection
}
```

If the user specifies `-o /tmp/my-report.tar.gz`, the wrapper:
1. Resolves the path to an absolute host path
2. Bind-mounts the parent directory
3. Rewrites the flag to the container-internal path

```
$ inspectah scan -o /tmp/my-report.tar.gz

# Wrapper translates to:
podman run ... -v /tmp:/output-custom ... inspectah scan -o /output-custom/my-report.tar.gz
```

### Fleet output

Fleet takes a directory of tarballs as input and produces an output tarball.
The wrapper mounts the input directory read-only and the output directory
read-write. The output mount is resolved from the `-o` flag (or CWD as
default) — the wrapper resolves the actual parent directory at runtime, not
a hardcoded path:

```
$ inspectah fleet ./collected-scans/

# Wrapper translates to:
podman run ... \
  -v /home/user/collected-scans:/input:ro \
  -v /home/user:/output \
  inspectah fleet /input -o /output/collected-scans.tar.gz

# Result: /home/user/collected-scans.tar.gz
```

### Refine/architect input

These subcommands take a tarball or directory as input and serve a web UI.
The wrapper mounts the input and passes the container-internal path:

```
$ inspectah refine ./my-host-2026-04-25.tar.gz

# Wrapper translates to:
podman run ... \
  -v /home/user/my-host-2026-04-25.tar.gz:/input/my-host-2026-04-25.tar.gz:ro \
  -p 8642:8642 \
  inspectah refine /input/my-host-2026-04-25.tar.gz
```

### Path validation

Before executing, the wrapper validates that:
- Input paths exist and are readable
- Output directories exist and are writable
- Paths don't contain characters that break bind-mount syntax (`:` on Linux)

```
$ inspectah refine ./nonexistent.tar.gz
Error: file not found: ./nonexistent.tar.gz

Expected a tarball (.tar.gz) from a previous inspectah scan.
Run 'inspectah scan' first to generate one.
```

## Tab Completion

Cobra generates completion scripts for bash, zsh, and fish. The Go wrapper
replaces the hand-maintained static completion scripts from the packaging
spec with auto-generated ones that are always in sync with the actual CLI.

### Generation

```bash
inspectah completion bash > /etc/bash_completion.d/inspectah
inspectah completion zsh > /usr/local/share/zsh/site-functions/_inspectah
inspectah completion fish > ~/.config/fish/completions/inspectah.fish
```

### RPM installation

The RPM `%post` scriptlet installs completion files to standard paths:

```
/usr/share/bash-completion/completions/inspectah
/usr/share/zsh/site-functions/_inspectah
/usr/share/fish/vendor_completions.d/inspectah.fish
```

These are generated at RPM build time by running `inspectah completion <shell>`
during the build, so they don't require a post-install step. They're packaged
as regular files.

### Completion scope

Completions cover:
- Subcommand names (`scan`, `fleet`, `refine`, `architect`, `build`, `image`, `version`, `completion`)
- Image sub-subcommands (`use`, `update`, `info`)
- Wrapper-owned flags (`--image`, `--pull`, `--verbose`, `--dry-run`, `--podman`)
- Commonly used passthrough flags (`-o`, `--from-snapshot`, `--target-version`, etc.)
- File/directory completion for positional args (tarballs, directories)

Passthrough flags are registered with cobra as hidden commands so completion
works, but they're not validated by the wrapper -- the container's argparse
is authoritative.

### Drift prevention

CI runs `inspectah completion bash | diff - completions/inspectah.bash` to
ensure the committed completion files match what the binary generates. This
catches cases where cobra commands are added but completions aren't
regenerated.

## Version Management

### Two version numbers

The wrapper tracks two independent versions:

1. **Wrapper version** -- the Go binary version (e.g., `1.0.0`). Set at build
   time via `-ldflags`.
2. **Container image version** -- the inspectah Python tool version (e.g.,
   `0.5.1`). The tag the wrapper pulls/uses.

### Compatibility matrix

The wrapper embeds a minimum and maximum compatible container image version:

```go
var (
    MinImageVersion = "0.5.0"
    MaxImageVersion = "0.99.99"  // Upper bound for major version compat
)
```

On every run, the wrapper checks the pulled image's version label
(`org.opencontainers.image.version`) against this range. If out of range:

```
Warning: container image version 0.4.0 is older than this wrapper expects
(minimum: 0.5.0). Some features may not work correctly.

Update the image:  inspectah image update
Update the wrapper: sudo dnf update inspectah
```

### Version display

```
$ inspectah version
inspectah wrapper  v1.0.0
  built:    2026-04-25T10:00:00Z
  go:       go1.22.2
  platform: linux/amd64

inspectah image    v0.5.1
  image:    ghcr.io/marrusl/inspectah:0.5.1
  pulled:   2026-04-20T14:30:00Z
```

If the image is not present locally, `inspectah version` shows:

```
inspectah image    (not pulled)
  default:  ghcr.io/marrusl/inspectah:0.5.1
  Run 'inspectah image update' to pull.
```

## Packaging

### RPM spec

```spec
Name:           inspectah
Version:        1.0.0
Release:        1%{?dist}
Summary:        Migration assessment tool (package-mode to image-mode)
License:        MIT
URL:            https://github.com/marrusl/inspectah

Source0:        %{name}-%{version}.tar.gz

BuildRequires:  golang >= 1.21

Requires:       podman >= 4.4

%description
inspectah scans RHEL and CentOS hosts, identifies workload characteristics,
and generates bootc image artifacts for migration to image mode. This package
provides the CLI wrapper that manages the inspectah container image.

%prep
%setup -q

%build
cd cmd/inspectah
go build -ldflags "-X main.version=%{version}" -o ../../inspectah .

%install
install -Dm755 inspectah %{buildroot}%{_bindir}/inspectah
./inspectah completion bash > inspectah.bash
./inspectah completion zsh > _inspectah
./inspectah completion fish > inspectah.fish
install -Dm644 inspectah.bash %{buildroot}%{_datadir}/bash-completion/completions/inspectah
install -Dm644 _inspectah %{buildroot}%{_datadir}/zsh/site-functions/_inspectah
install -Dm644 inspectah.fish %{buildroot}%{_datadir}/fish/vendor_completions.d/inspectah.fish

%files
%license LICENSE
%{_bindir}/inspectah
%{_datadir}/bash-completion/completions/inspectah
%{_datadir}/zsh/site-functions/_inspectah
%{_datadir}/fish/vendor_completions.d/inspectah.fish
```

Key RPM properties:
- **Only runtime dependency:** `podman >= 4.4`
- **No Python, no pip, no venv.** The Go binary is self-contained.
- **Works on EL8** where Python 3.11+ is unavailable.

### COPR distribution

Published to a COPR repo for easy installation:

```bash
sudo dnf copr enable marrusl/inspectah
sudo dnf install inspectah
```

COPR builds are triggered by GitHub Releases. The COPR spec pulls the
pre-built binary from the GitHub Release assets rather than building from
source, keeping build times fast and reproducible.

### Homebrew formula

```ruby
class Inspectah < Formula
  desc "Migration assessment tool (package-mode to image-mode for RHEL)"
  homepage "https://github.com/marrusl/inspectah"
  url "https://github.com/marrusl/inspectah/releases/download/v1.0.0/inspectah-1.0.0-darwin-arm64.tar.gz"
  sha256 "..."
  license "MIT"

  depends_on "podman"

  def install
    bin.install "inspectah"
    bash_completion.install "completions/inspectah.bash" => "inspectah"
    zsh_completion.install "completions/_inspectah"
    fish_completion.install "completions/inspectah.fish"
  end

  test do
    assert_match "inspectah wrapper", shell_output("#{bin}/inspectah version 2>&1")
  end
end
```

Hosted in a tap repository: `marrusl/homebrew-inspectah`.

macOS primary use case: running `fleet`, `refine`, `architect`, and `build`
against tarballs collected from remote hosts. The `scan` subcommand is
Linux-only and errors out on macOS with a clear message.

## Build & Release

### Cross-compilation matrix

```yaml
# .github/workflows/go-release.yml
strategy:
  matrix:
    include:
      - goos: linux
        goarch: amd64
      - goos: linux
        goarch: arm64
      - goos: darwin
        goarch: amd64
      - goos: darwin
        goarch: arm64
```

### Build command

```bash
cd cmd/inspectah
CGO_ENABLED=0 GOOS=$GOOS GOARCH=$GOARCH \
  go build \
    -ldflags "-s -w \
      -X main.version=${VERSION} \
      -X main.commit=${COMMIT} \
      -X main.date=${DATE} \
      -X internal/container.DefaultTag=${VERSION}" \
    -o ../../inspectah-${GOOS}-${GOARCH} \
    .
```

`CGO_ENABLED=0` ensures truly static binaries with no libc dependency.

### Release workflow

Triggered on Git tags matching `v*`:

1. **Build** -- Cross-compile for all targets
2. **Test** -- Run unit tests and integration tests (see Testing Strategy)
3. **Package** -- Create tarballs with binary + completions + LICENSE
4. **Release** -- Create GitHub Release with assets
5. **COPR** -- Trigger COPR build from release assets
6. **Homebrew** -- Update tap formula with new version and SHA

### Relationship to existing workflows

The Go wrapper release is independent of the container image release. The
container image continues to be built and pushed by the existing
`build-image.yml` workflow. The Go release workflow sets `DefaultTag` to
match the latest compatible container image version.

## Testing Strategy

### Unit tests

Cover the wrapper's own logic -- no podman or container required:

- **Argument parsing:** Subcommand routing, flag interception, bare-flag
  backwards compatibility
- **Path resolution:** Host-to-container path mapping for all subcommands
- **Error translation:** Exit code + stderr pattern matching → user messages
- **Version checking:** Compatibility range validation, drift detection
- **Image reference resolution:** Flag → env var → default precedence

```go
func TestScanFlagPassthrough(t *testing.T) {
    args := parseScanArgs([]string{"scan", "--target-version", "10.2", "-o", "/tmp/out.tar.gz"})
    assert.Equal(t, "10.2", args.Passthrough["--target-version"])
    assert.Equal(t, "/output-custom/out.tar.gz", args.ContainerOutputPath)
}

func TestErrorTranslation(t *testing.T) {
    msg := translateError(125, "Error: unable to find image")
    assert.Contains(t, msg, "container image not found")
    assert.Contains(t, msg, "inspectah image update")
}

func TestBareFlagsDefaultToScan(t *testing.T) {
    cmd := resolveSubcommand([]string{"--from-snapshot", "foo.json"})
    assert.Equal(t, "scan", cmd)
}
```

### Integration tests

Require podman and run actual container operations. Gated behind a build
tag so they don't run in CI environments without podman:

```go
//go:build integration

func TestScanProducesOutput(t *testing.T) {
    // Requires: root, podman, host filesystem access
    // Tests run from cmd/inspectah/ (the Go module root)
    dir := t.TempDir()
    cmd := exec.Command("go", "run", ".", "scan",
        "--from-snapshot", "testdata/minimal-snapshot.json",
        "-o", filepath.Join(dir, "test.tar.gz"))
    cmd.Dir = "." // cmd/inspectah/ — the Go module root
    err := cmd.Run()
    assert.NoError(t, err)
    assert.FileExists(t, filepath.Join(dir, "test.tar.gz"))
}

func TestFleetPathMapping(t *testing.T) {
    dir := t.TempDir()
    cmd := exec.Command("go", "run", ".", "fleet",
        "testdata/fleet-input/",
        "-o", filepath.Join(dir, "fleet.tar.gz"))
    cmd.Dir = "." // cmd/inspectah/ — the Go module root
    err := cmd.Run()
    assert.NoError(t, err)
    assert.FileExists(t, filepath.Join(dir, "fleet.tar.gz"))
}
```

### CI pipeline

```yaml
jobs:
  unit-test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/setup-go@v5
      - run: go test ./...
        working-directory: cmd/inspectah

  integration-test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/setup-go@v5
      - run: sudo dnf install -y podman
      - run: go test -tags=integration ./...
        working-directory: cmd/inspectah

  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: golangci/golangci-lint-action@v4
        with:
          working-directory: cmd/inspectah
```

## Migration Path

### From run-inspectah.sh to the Go wrapper

The transition is designed to be zero-friction:

1. **Install the RPM:** `sudo dnf copr enable marrusl/inspectah && sudo dnf install inspectah`
2. **Use the same commands:** `inspectah scan` works identically
3. **Remove the shell script:** `rm run-inspectah.sh` (optional -- the RPM
   doesn't conflict with the script)

### Environment variable compatibility

The wrapper honors all environment variables from `run-inspectah.sh`:

| Variable              | Behavior                                       |
|-----------------------|------------------------------------------------|
| `INSPECTAH_IMAGE`     | Override default image (same as `--image`)     |
| `INSPECTAH_OUTPUT_DIR`| Override output directory                      |
| `INSPECTAH_DEBUG`     | Passed through to container                    |

### What changes for existing users

| Aspect                  | run-inspectah.sh          | Go wrapper                   |
|-------------------------|---------------------------|------------------------------|
| Default pull policy     | `always`                  | `missing`                    |
| Tab completion          | None                      | bash/zsh/fish                |
| Error messages          | Raw podman stderr         | Translated + actionable      |
| Progress on pull        | Podman default            | Compact progress bar         |
| Version checking        | None                      | Compatibility warnings       |
| Air-gap support         | Manual podman commands    | `--pull=never` with guidance |

The default pull policy change (`always` -> `missing`) is the most
user-visible behavioral difference. The existing user base is small enough
(a couple of people) that this is a clean break. Document the change in
release notes for completeness. Users who want the old behavior can use
`--pull=always` or set `INSPECTAH_PULL=always`.

## Scope Boundaries

### What the wrapper does

- Constructs and executes podman commands
- Manages container image lifecycle (pull, version pin, update)
- Runs `podman build` against scan output (`build` subcommand)
- Translates file paths between host and container
- Translates container errors to human-readable messages via structured error types
- Generates shell completions
- Provides progress feedback on image pulls and builds
- Gates platform-specific subcommands (scan is Linux-only)

### What the wrapper does NOT do

- **No reimplementation of inspectah logic.** The wrapper never parses
  snapshots, renders Containerfiles, or runs inspectors. That's all Python
  inside the container.
- **No flag validation for passthrough flags.** The wrapper doesn't know or
  care what `--target-version` means. It passes it through. The container's
  argparse is authoritative.
- **No automatic podman installation.** The shell script attempted `dnf install
  podman` automatically. The wrapper requires podman to already be installed
  (it's an RPM dependency). If podman is missing, the error message tells
  the user how to install it.
- **No registry login management.** The wrapper does not run `podman login` or
  manage credentials. If a registry login is required (for `scan` pulling RHEL
  base images), the container handles the check and the error message.
- **No rootless/rootful decision-making.** The wrapper runs podman however the
  user invokes it. If the user runs `sudo inspectah scan`, podman runs as root.
  If they run `inspectah scan` as a regular user, podman runs rootless. The
  `scan` subcommand requires privileged access and will fail with a clear
  error if run without sufficient permissions.
- **No scanning on macOS.** The `scan` subcommand errors out on macOS with a
  clear message directing users to run on a Linux host. macOS is supported
  for `fleet`, `refine`, `architect`, and `build` only.
- **No Windows support.** WSL is untested and unsupported.

## Decisions

All open questions from the proposal phase have been resolved.

1. **Image versioning → `image` subcommand family.** Confirmed as
   `inspectah image use|update|info`. Version pinning is a day-one
   requirement, not overengineering. Users must be able to target a specific
   container image version. The `image` namespace leaves room for future
   image management capabilities.

2. **macOS scan → error out.** `inspectah scan` on macOS exits with a
   human-readable error: "scan requires a Linux host — use on a RHEL/CentOS
   system." macOS is not a migration target. Scanning inside the podman
   machine VM was rejected — it would scan Fedora CoreOS, not the user's
   actual system, which is confusing and useless.

3. **Minimum podman → 4.4.0.** EL8.8 baseline confirmed. No need to
   support older podman versions. The `--pid=host` behavioral differences
   between 4.0 and 4.4 are the reason for the floor.

4. **Repo layout → same repo, separate Go module.** Go source lives in
   `cmd/inspectah/` with its own `go.mod`. One repo, one product, one
   release cadence. This avoids the coordination overhead of a separate
   repo while keeping Go and Python toolchains isolated in CI.

5. **Default pull policy → `missing`.** The existing user base is small
   enough that the behavioral change from `always` is a clean break.
   No deprecation warning needed.

6. **Build → first-class subcommand.** `inspectah build <output-dir>`
   takes a scan output directory and runs `podman build` against the
   generated Containerfile. This is not a `--validate` flag on scan —
   it's a standalone subcommand representing the natural next step in the
   scan → build → deploy workflow.

7. **Telemetry → skip for v1.** No usage metrics in the initial release.
   Structured error types (`WrapperError` with `ErrorKind` classification)
   are used from day one so errors are machine-classifiable when telemetry
   is added later. Telemetry can be cleanly retrofitted via cobra's
   `PersistentPreRun`/`PersistentPostRun` hooks without touching
   subcommand implementations.

## Future Considerations

- **Telemetry.** Opt-in anonymous usage metrics (subcommand distribution,
  error rates, platform distribution) via cobra's persistent hooks. The
  structured error types make this a straightforward addition. Design the
  consent UX when the time comes.
- **Plugin system.** If inspectah grows beyond scan/build/fleet/refine/
  architect, consider a plugin architecture rather than baking everything
  into the wrapper.
- **Podman Desktop integration.** The wrapper's structured errors and
  machine-readable output could feed into a Podman Desktop extension for
  GUI-driven migration workflows.
