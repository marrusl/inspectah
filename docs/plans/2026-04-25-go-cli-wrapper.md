# Go CLI Wrapper Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `run-inspectah.sh` with a compiled Go CLI wrapper that manages the container lifecycle, translates errors, supports tab completion, and provides a polished UX -- without reimplementing any inspectah logic.

**Architecture:** A Cobra-based CLI binary that constructs and executes `podman run` / `podman build` commands. Two execution strategies: exec replacement (scan, fleet) for direct terminal control, and child process mode (refine, architect, build) for server management, signal forwarding, and browser launch. All inspectah logic stays in the container -- the wrapper is pure orchestration.

**Tech Stack:** Go 1.21+, Cobra (CLI framework), podman >= 4.4 (runtime dependency), testify (test assertions).

**Spec:** `docs/specs/proposed/2026-04-25-go-cli-wrapper-design.md`

---

## File Structure

### New files

| File | Responsibility |
|------|---------------|
| `cmd/inspectah/go.mod` | Go module definition, dependencies |
| `cmd/inspectah/go.sum` | Dependency checksums |
| `cmd/inspectah/main.go` | Entry point, version vars, root command |
| `cmd/inspectah/internal/cli/root.go` | Root Cobra command, global flags, env var binding |
| `cmd/inspectah/internal/cli/scan.go` | `scan` subcommand -- exec strategy, privileged mount setup |
| `cmd/inspectah/internal/cli/fleet.go` | `fleet` subcommand -- exec strategy, directory bind-mount |
| `cmd/inspectah/internal/cli/refine.go` | `refine` subcommand -- child process, browser launch, port polling |
| `cmd/inspectah/internal/cli/architect.go` | `architect` subcommand -- child process, browser launch, port polling |
| `cmd/inspectah/internal/cli/build.go` | `build` subcommand -- host `podman build`, Containerfile validation |
| `cmd/inspectah/internal/cli/image.go` | `image use|update|info` subcommand family |
| `cmd/inspectah/internal/cli/version.go` | `version` subcommand -- wrapper + container versions |
| `cmd/inspectah/internal/cli/completion.go` | `completion` subcommand -- Cobra built-in generation |
| `cmd/inspectah/internal/container/run.go` | Podman invocation builder -- mounts, env, flags |
| `cmd/inspectah/internal/container/exec.go` | Exec strategy -- `syscall.Exec()` replacement |
| `cmd/inspectah/internal/container/child.go` | Child process strategy -- spawn, signal forward, wait |
| `cmd/inspectah/internal/container/image.go` | Image resolution (4-step precedence), pull, version check, local presence |
| `cmd/inspectah/internal/container/progress.go` | Pull progress bar parsing and display |
| `cmd/inspectah/internal/errors/errors.go` | `ErrorKind` enum, `WrapperError` struct |
| `cmd/inspectah/internal/errors/translate.go` | Exit code + stderr pattern -> `WrapperError` classification |
| `cmd/inspectah/internal/errors/render.go` | Format `WrapperError` for terminal output |
| `cmd/inspectah/internal/paths/resolve.go` | Host path <-> container path mapping, validation |
| `cmd/inspectah/internal/platform/detect.go` | Platform detection, macOS gating for scan |
| `cmd/inspectah/internal/version/check.go` | Compatibility matrix, version normalization |
| `cmd/inspectah/internal/container/run_test.go` | Unit tests for podman command construction |
| `cmd/inspectah/internal/errors/translate_test.go` | Unit tests for error classification |
| `cmd/inspectah/internal/paths/resolve_test.go` | Unit tests for path mapping |
| `cmd/inspectah/internal/version/check_test.go` | Unit tests for version normalization and compat |
| `cmd/inspectah/internal/platform/detect_test.go` | Unit tests for platform detection |
| `cmd/inspectah/internal/cli/scan_test.go` | Unit tests for scan flag parsing |
| `cmd/inspectah/internal/cli/build_test.go` | Unit tests for build flag parsing |
| `cmd/inspectah/internal/cli/image_test.go` | Unit tests for image subcommand family |
| `cmd/inspectah/internal/integration_test.go` | Integration tests (build-tagged, requires podman) |
| `cmd/inspectah/testdata/` | Test fixtures directory |
| `packaging/inspectah.spec` | RPM spec file |
| `packaging/inspectah.rb` | Homebrew formula |
| `.github/workflows/go-cli.yml` | Go build/test/lint CI workflow |

### Modified files

| File | Change |
|------|--------|
| `run-inspectah.sh` | Add deprecation notice pointing to Go wrapper |
| `.github/workflows/release.yml` | Add Go cross-compilation and binary upload steps |
| `.gitignore` | Add Go build artifacts (`cmd/inspectah/inspectah`, `dist/`) |
| `README.md` | Update installation section for RPM/Homebrew, keep shell script docs |

---

## Task 1: Foundation -- Go module, root command, version

**Files:** `cmd/inspectah/go.mod`, `cmd/inspectah/main.go`, `cmd/inspectah/internal/cli/root.go`, `cmd/inspectah/internal/cli/version.go`, `.gitignore`

This task bootstraps the Go project. After completion, `go run .` prints version info and `--help` shows the command tree.

- [ ] **Step 1: Initialize Go module**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
mkdir -p cmd/inspectah/internal/{cli,container,errors,paths,platform,version}
cd cmd/inspectah
go mod init github.com/marrusl/inspectah/cmd/inspectah
go get github.com/spf13/cobra@latest
go get github.com/stretchr/testify@latest
```

- [ ] **Step 2: Create main.go with ldflags version injection**

`cmd/inspectah/main.go`:
```go
package main

import (
    "fmt"
    "os"

    "github.com/marrusl/inspectah/cmd/inspectah/internal/cli"
)

// Set via ldflags at build time.
var (
    version = "dev"
    commit  = "none"
    date    = "unknown"
)

func main() {
    root := cli.NewRootCmd(version, commit, date)
    if err := root.Execute(); err != nil {
        fmt.Fprintln(os.Stderr, err)
        os.Exit(1)
    }
}
```

- [ ] **Step 3: Create root.go with global flags**

`cmd/inspectah/internal/cli/root.go`:
```go
package cli

import (
    "github.com/spf13/cobra"
)

const defaultImage = "ghcr.io/marrusl/inspectah"

// GlobalOpts holds flags owned by the wrapper (not passed to container).
type GlobalOpts struct {
    Image   string
    Podman  string
    Pull    string
    Verbose bool
    DryRun  bool
}

func NewRootCmd(version, commit, date string) *cobra.Command {
    opts := &GlobalOpts{}

    root := &cobra.Command{
        Use:   "inspectah",
        Short: "Migration assessment tool (package-mode to image-mode)",
        Long: `inspectah scans RHEL and CentOS hosts, identifies workload
characteristics, and generates bootc image artifacts for migration
to image mode.`,
        SilenceUsage:  true,
        SilenceErrors: true,
    }

    // Global flags -- wrapper-owned, not passed to container.
    pf := root.PersistentFlags()
    pf.StringVar(&opts.Image, "image", "", "Override container image (default: auto)")
    pf.StringVar(&opts.Podman, "podman", "", "Path to podman binary (default: auto-detect)")
    pf.StringVar(&opts.Pull, "pull", "missing", "Pull policy: always, missing, never")
    pf.BoolVar(&opts.Verbose, "verbose", false, "Show podman command being executed")
    pf.BoolVar(&opts.DryRun, "dry-run", false, "Print podman command without executing")

    // Bind env vars: INSPECTAH_IMAGE, INSPECTAH_PULL
    // cobra doesn't have native env binding, so we check in PersistentPreRun.
    root.PersistentPreRun = func(cmd *cobra.Command, args []string) {
        bindEnvDefaults(opts, cmd)
    }

    // Register subcommands (stubs for now, fleshed out in later tasks).
    root.AddCommand(newVersionCmd(version, commit, date))

    return root
}
```

- [ ] **Step 4: Create version.go subcommand**

`cmd/inspectah/internal/cli/version.go`:
```go
package cli

import (
    "fmt"

    "github.com/spf13/cobra"
)

func newVersionCmd(version, commit, date string) *cobra.Command {
    return &cobra.Command{
        Use:   "version",
        Short: "Print wrapper and container image versions",
        RunE: func(cmd *cobra.Command, args []string) error {
            fmt.Printf("inspectah wrapper %s\n", version)
            fmt.Printf("  commit: %s\n", commit)
            fmt.Printf("  built:  %s\n", date)
            // Container image version will be added in Task 6.
            return nil
        },
    }
}
```

- [ ] **Step 5: Add Go build artifacts to .gitignore**

Append to the project root `.gitignore`:
```
# Go build artifacts
cmd/inspectah/inspectah
dist/
```

- [ ] **Step 6: Verify build and help output**

```bash
cd cmd/inspectah
go build -ldflags "-X main.version=0.1.0-dev" -o inspectah .
./inspectah --help
./inspectah version
```

Expected: help shows global flags and `version` subcommand. Version prints `0.1.0-dev`.

- [ ] **Step 7: Commit**

```bash
git add cmd/inspectah/ .gitignore
git commit -m "feat(go-cli): bootstrap Go module with root command and version

Initialize Go module at cmd/inspectah/ with Cobra root command,
global flags (--image, --podman, --pull, --verbose, --dry-run),
and version subcommand with ldflags injection.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 2: Platform detection and podman discovery

**Files:** `cmd/inspectah/internal/platform/detect.go`, `cmd/inspectah/internal/platform/detect_test.go`

Platform detection gates `scan` to Linux-only and locates the podman binary. Test-first: write detection tests before implementation.

- [ ] **Step 1: Write failing tests for platform detection**

`cmd/inspectah/internal/platform/detect_test.go`:
```go
package platform

import (
    "runtime"
    "testing"

    "github.com/stretchr/testify/assert"
)

func TestIsLinux(t *testing.T) {
    result := IsLinux()
    if runtime.GOOS == "linux" {
        assert.True(t, result)
    } else {
        assert.False(t, result)
    }
}

func TestScanGateRejectsNonLinux(t *testing.T) {
    if runtime.GOOS == "linux" {
        t.Skip("test only meaningful on non-Linux")
    }
    err := CheckScanPlatform()
    assert.Error(t, err)
    assert.Contains(t, err.Error(), "Linux host")
}

func TestScanGateRejectsNonLinux_ErrPlatformUnsupported(t *testing.T) {
    // Acceptance test: on macOS, `inspectah scan` must return
    // ErrPlatformUnsupported without ever invoking podman.
    if runtime.GOOS == "linux" {
        t.Skip("test only meaningful on non-Linux")
    }
    err := CheckScanPlatform()
    assert.ErrorIs(t, err, ErrPlatformUnsupported)
}

func TestFindPodman_ExplicitPath(t *testing.T) {
    // Non-existent path should fail.
    _, err := FindPodman("/nonexistent/podman")
    assert.Error(t, err)
}
```

- [ ] **Step 2: Implement platform detection**

`cmd/inspectah/internal/platform/detect.go`:
```go
package platform

import (
    "errors"
    "fmt"
    "os/exec"
    "runtime"
)

// ErrPlatformUnsupported is returned when scan is attempted on a non-Linux host.
// This is a sentinel error so callers (and tests) can check with errors.Is().
var ErrPlatformUnsupported = errors.New("platform unsupported")

// IsLinux returns true when running on a Linux host.
func IsLinux() bool {
    return runtime.GOOS == "linux"
}

// CheckScanPlatform returns ErrPlatformUnsupported if scan is attempted on
// a non-Linux host. The check fires before any podman invocation, so macOS
// users get an immediate, clear error without triggering container operations.
func CheckScanPlatform() error {
    if !IsLinux() {
        return fmt.Errorf(
            "%w: scan requires a Linux host -- use on a RHEL/CentOS system\n\n" +
            "The scan subcommand inspects the host filesystem and requires\n" +
            "direct access to /etc, /var, and package databases.\n\n" +
            "On macOS, use fleet, refine, architect, or build instead.",
            ErrPlatformUnsupported)
    }
    return nil
}

// FindPodman locates the podman binary. If explicit is non-empty, it
// validates that path exists. Otherwise searches PATH.
func FindPodman(explicit string) (string, error) {
    if explicit != "" {
        path, err := exec.LookPath(explicit)
        if err != nil {
            return "", fmt.Errorf("podman not found at %s", explicit)
        }
        return path, nil
    }
    path, err := exec.LookPath("podman")
    if err != nil {
        return "", fmt.Errorf(
            "podman not found in PATH\n\n" +
            "Install podman:\n" +
            "  Fedora/RHEL: sudo dnf install podman\n" +
            "  macOS:       brew install podman")
    }
    return path, nil
}
```

- [ ] **Step 3: Run tests**

```bash
cd cmd/inspectah && go test ./internal/platform/ -v
```

- [ ] **Step 4: Commit**

```bash
git add cmd/inspectah/internal/platform/
git commit -m "feat(go-cli): platform detection and podman discovery

Gate scan to Linux-only with actionable error message. Auto-discover
podman binary via PATH lookup with explicit override support.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 3: Error types and translation layer

**Files:** `cmd/inspectah/internal/errors/errors.go`, `cmd/inspectah/internal/errors/translate.go`, `cmd/inspectah/internal/errors/render.go`, `cmd/inspectah/internal/errors/translate_test.go`

Structured errors are used throughout the wrapper. Build and test the classification logic before any subcommand wiring.

- [ ] **Step 1: Write failing tests for error classification**

`cmd/inspectah/internal/errors/translate_test.go`:
```go
package errors

import (
    "testing"

    "github.com/stretchr/testify/assert"
)

func TestClassify_PodmanNotFound(t *testing.T) {
    err := Classify(127, "bash: podman: command not found", "")
    assert.Equal(t, ErrPodmanNotFound, err.Kind)
}

func TestClassify_PermissionDenied(t *testing.T) {
    err := Classify(125, "", "Error: permission denied while trying to connect")
    assert.Equal(t, ErrPermissionDenied, err.Kind)
}

func TestClassify_ImageNotFound(t *testing.T) {
    err := Classify(125, "", "Error: ghcr.io/marrusl/inspectah:bad: image not known")
    assert.Equal(t, ErrImageNotFound, err.Kind)
}

func TestClassify_SELinux(t *testing.T) {
    err := Classify(1, "", "avc:  denied  { read } for")
    assert.Equal(t, ErrSELinux, err.Kind)
}

func TestClassify_OOMKilled(t *testing.T) {
    err := Classify(137, "", "")
    assert.Equal(t, ErrOOMKilled, err.Kind)
}

func TestClassify_DiskSpace(t *testing.T) {
    err := Classify(1, "", "No space left on device")
    assert.Equal(t, ErrDiskSpace, err.Kind)
}

func TestClassify_PortConflict(t *testing.T) {
    err := Classify(125, "", "address already in use")
    assert.Equal(t, ErrPortConflict, err.Kind)
}

func TestClassify_Unclassified(t *testing.T) {
    err := Classify(1, "", "something unexpected happened")
    assert.Equal(t, ErrUnclassified, err.Kind)
    assert.Contains(t, err.Stderr, "something unexpected happened")
}

func TestWrapperError_HasSuggestion(t *testing.T) {
    err := Classify(125, "", "Error: ghcr.io/marrusl/inspectah:bad: image not known")
    assert.NotEmpty(t, err.Suggestion)
}
```

- [ ] **Step 2: Implement error types**

`cmd/inspectah/internal/errors/errors.go`:
```go
package errors

import "fmt"

// ErrorKind classifies errors for translation and future telemetry.
type ErrorKind int

const (
    ErrPodmanNotFound      ErrorKind = iota // podman binary missing
    ErrPodmanStart                          // podman failed to start container
    ErrPermissionDenied                     // insufficient privileges
    ErrImageNotFound                        // container image not available
    ErrDiskSpace                            // ENOSPC
    ErrSELinux                              // AVC denial
    ErrNetworkTimeout                       // pull timed out
    ErrPortConflict                         // port already in use
    ErrOOMKilled                            // container OOM
    ErrBuildFailed                          // podman build failure
    ErrPlatformUnsupported                  // scan on macOS
    ErrIncompatibleVersion                  // image/wrapper version mismatch
    ErrUnclassified                         // fallback
)

// WrapperError is the structured error type for user-facing errors.
type WrapperError struct {
    Kind       ErrorKind
    Message    string // User-facing one-line summary
    Suggestion string // Actionable next step
    Stderr     string // Raw stderr (shown with --verbose)
    ExitCode   int    // Container/podman exit code
}

func (e *WrapperError) Error() string {
    return e.Message
}

func (e *WrapperError) Unwrap() error {
    if e.Stderr != "" {
        return fmt.Errorf("%s", e.Stderr)
    }
    return nil
}
```

- [ ] **Step 3: Implement error classification from exit codes and stderr patterns**

`cmd/inspectah/internal/errors/translate.go` -- implement `Classify(exitCode int, stdout, stderr string) *WrapperError` with pattern matching on stderr substrings. Exit code 127 -> `ErrPodmanNotFound`, 137 -> `ErrOOMKilled`, and stderr patterns for permission denied, image not found, SELinux AVC, disk space, port conflict, network timeout. Include actionable `Suggestion` strings for each kind (e.g., "Run with sudo or ensure your user is in the 'podman' group" for permission denied).

- [ ] **Step 4: Implement error rendering**

`cmd/inspectah/internal/errors/render.go` -- implement `Render(err *WrapperError, verbose bool) string` that formats the error for terminal output:
```
Error: <message>

<suggestion>
```
When `verbose` is true, also include the raw stderr.

- [ ] **Step 5: Run tests**

```bash
cd cmd/inspectah && go test ./internal/errors/ -v
```

- [ ] **Step 6: Commit**

```bash
git add cmd/inspectah/internal/errors/
git commit -m "feat(go-cli): structured error types and translation layer

Classify podman exit codes and stderr patterns into actionable
error messages. Covers permission denied, image not found, SELinux,
disk space, port conflicts, and OOM kills.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 4: Path resolution and validation

**Files:** `cmd/inspectah/internal/paths/resolve.go`, `cmd/inspectah/internal/paths/resolve_test.go`

Host-to-container path mapping is used by every subcommand. Build and test it independently.

- [ ] **Step 1: Write failing tests for path resolution**

`cmd/inspectah/internal/paths/resolve_test.go`:
```go
package paths

import (
    "testing"

    "github.com/stretchr/testify/assert"
    "github.com/stretchr/testify/require"
)

func TestResolveOutputPath_DefaultCwd(t *testing.T) {
    // No explicit -o: output goes to /output in container, maps to cwd.
    m, containerPath := ResolveOutputPath("", "/home/user")
    assert.Equal(t, "/output", containerPath)
    assert.Contains(t, m, Mount{Host: "/home/user", Container: "/output", ReadOnly: false})
}

func TestResolveOutputPath_ExplicitFile(t *testing.T) {
    // Explicit -o /tmp/report.tar.gz: mount /tmp, rewrite path.
    m, containerPath := ResolveOutputPath("/tmp/report.tar.gz", "/home/user")
    assert.Equal(t, "/output-custom/report.tar.gz", containerPath)
    assert.Contains(t, m, Mount{Host: "/tmp", Container: "/output-custom", ReadOnly: false})
}

func TestResolveInputPath_Fleet(t *testing.T) {
    m, containerPath := ResolveInputPath("/data/fleet-inputs", false)
    assert.Equal(t, "/input", containerPath)
    assert.Contains(t, m, Mount{Host: "/data/fleet-inputs", Container: "/input", ReadOnly: true})
}

func TestResolveInputPath_RefineTarball(t *testing.T) {
    m, containerPath := ResolveInputPath("/data/host.tar.gz", false)
    assert.Equal(t, "/input/host.tar.gz", containerPath)
    assert.Contains(t, m, Mount{Host: "/data", Container: "/input", ReadOnly: true})
}

func TestValidatePath_MissingFile(t *testing.T) {
    err := ValidateInputExists("/nonexistent/file.tar.gz")
    require.Error(t, err)
    assert.Contains(t, err.Error(), "not found")
}

func TestValidatePath_ColonInPath(t *testing.T) {
    err := ValidatePathSafe("/path/with:colon/file")
    require.Error(t, err)
    assert.Contains(t, err.Error(), "colon")
}
```

- [ ] **Step 2: Implement path resolution**

`cmd/inspectah/internal/paths/resolve.go`:

Implement:
- `Mount` struct with `Host`, `Container`, `ReadOnly` fields and a `String()` method returning the `-v` flag value
- `ResolveOutputPath(explicit, cwd string) ([]Mount, containerPath string)` -- maps host output paths to container paths
- `ResolveInputPath(hostPath string, isDir bool) ([]Mount, containerPath string)` -- maps host input paths (tarballs, directories) to container paths
- `ValidateInputExists(path string) error` -- checks file/dir existence with user-friendly error
- `ValidateOutputDir(path string) error` -- checks parent directory is writable
- `ValidatePathSafe(path string) error` -- rejects paths with `:` that break bind-mount syntax

- [ ] **Step 3: Run tests**

```bash
cd cmd/inspectah && go test ./internal/paths/ -v
```

- [ ] **Step 4: Commit**

```bash
git add cmd/inspectah/internal/paths/
git commit -m "feat(go-cli): host-to-container path resolution

Map host output/input paths to container bind-mount paths. Validate
file existence, directory writability, and path safety (no colons).

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 5: Version normalization and compatibility checking

**Files:** `cmd/inspectah/internal/version/check.go`, `cmd/inspectah/internal/version/check_test.go`

Version logic is used by the `image` subcommand family and the compatibility check on every run. Test-first.

- [ ] **Step 1: Write failing tests for version normalization**

`cmd/inspectah/internal/version/check_test.go`:
```go
package version

import (
    "testing"

    "github.com/stretchr/testify/assert"
)

func TestNormalize_StripV(t *testing.T) {
    assert.Equal(t, "0.5.1", Normalize("v0.5.1"))
    assert.Equal(t, "0.5.1", Normalize("0.5.1"))
}

func TestNormalize_DisplayPrefix(t *testing.T) {
    assert.Equal(t, "v0.5.1", Display("0.5.1"))
    assert.Equal(t, "v0.5.1", Display("v0.5.1"))
}

func TestIsCompatible_InRange(t *testing.T) {
    ok, msg := IsCompatible("0.5.1", "0.5.0", "0.99.99")
    assert.True(t, ok)
    assert.Empty(t, msg)
}

func TestIsCompatible_BelowMin(t *testing.T) {
    ok, msg := IsCompatible("0.4.0", "0.5.0", "0.99.99")
    assert.False(t, ok)
    assert.Contains(t, msg, "older")
}

func TestIsCompatible_AboveMax(t *testing.T) {
    ok, msg := IsCompatible("1.0.0", "0.5.0", "0.99.99")
    assert.False(t, ok)
    assert.Contains(t, msg, "newer")
}

func TestParseImageVersion_Valid(t *testing.T) {
    v, err := ParseImageVersion("org.opencontainers.image.version=0.5.1")
    assert.NoError(t, err)
    assert.Equal(t, "0.5.1", v)
}
```

- [ ] **Step 2: Implement version utilities**

`cmd/inspectah/internal/version/check.go`:

Implement:
- `Normalize(v string) string` -- strip leading `v` prefix
- `Display(v string) string` -- add `v` prefix if missing
- `IsCompatible(imageVer, minVer, maxVer string) (bool, string)` -- semver range check
- `ParseImageVersion(label string) (string, error)` -- extract version from OCI label
- Embedded compatibility range: `MinImageVersion = "0.5.0"`, `MaxImageVersion = "0.99.99"`

Use `golang.org/x/mod/semver` or a simple 3-part integer comparison (no external dependency needed for this).

- [ ] **Step 3: Run tests**

```bash
cd cmd/inspectah && go test ./internal/version/ -v
```

- [ ] **Step 4: Commit**

```bash
git add cmd/inspectah/internal/version/
git commit -m "feat(go-cli): version normalization and compatibility checking

Normalize v-prefix handling, check container image version against
embedded compatibility matrix, parse OCI image version labels.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 6: Core container orchestration -- image resolution, PodmanRunner, invocation builder

**Files:** `cmd/inspectah/internal/container/run.go`, `cmd/inspectah/internal/container/exec.go`, `cmd/inspectah/internal/container/child.go`, `cmd/inspectah/internal/container/image.go`, `cmd/inspectah/internal/container/run_test.go`, `cmd/inspectah/internal/cli/env.go`, `cmd/inspectah/internal/cli/root.go` (modify)

The heart of the wrapper. This task wires up three things every subcommand needs from task 8 onward: resolved image reference, PodmanRunner interface for testability, and the invocation builder.

### 6a: Image resolution and env binding (must exist before any subcommand)

Every subcommand consumes a resolved image ref -- none should build its own. Wire the full 4-step resolution precedence and env binding here so tasks 8+ inherit it.

- [ ] **Step 1: Implement image resolution function**

`cmd/inspectah/internal/container/image.go`:

Implement:
- `ResolveImage(flagOverride, envOverride, pinnedVersion, compiledDefault string) string` -- 4-step resolution order: `--image` flag > `INSPECTAH_IMAGE` env > pinned config > compiled default. Returns a fully-qualified image reference. This is a pure function with no side effects -- easy to unit test.
- `LoadPinnedVersion() (string, error)` -- read from `~/.config/inspectah/config.yaml`
- `ImageExists(podman, image string) (bool, error)` -- check if image exists locally via `podman image exists`

```go
// ResolveImage determines which container image to use.
// Precedence: --image flag > INSPECTAH_IMAGE env > pinned config > compiled default.
func ResolveImage(flagOverride, envOverride, pinnedVersion, compiledDefault string) string {
    if flagOverride != "" {
        return flagOverride
    }
    if envOverride != "" {
        return envOverride
    }
    if pinnedVersion != "" {
        return pinnedVersion
    }
    return compiledDefault
}
```

- [ ] **Step 2: Implement env var binding**

`cmd/inspectah/internal/cli/env.go`:

Implement `bindEnvDefaults(opts *GlobalOpts, cmd *cobra.Command)`:
```go
func bindEnvDefaults(opts *GlobalOpts, cmd *cobra.Command) {
    if v := os.Getenv("INSPECTAH_IMAGE"); v != "" && !cmd.Flags().Changed("image") {
        opts.Image = v
    }
    if v := os.Getenv("INSPECTAH_PULL"); v != "" && !cmd.Flags().Changed("pull") {
        opts.Pull = v
    }
}
```

- [ ] **Step 3: Wire resolved image into GlobalOpts**

Update `root.go` `PersistentPreRun` to call `bindEnvDefaults`, then resolve the image reference via `container.ResolveImage(opts.Image, os.Getenv("INSPECTAH_IMAGE"), pinnedVersion, defaultImage)` and store the result back into `opts.Image`. Every subcommand reads `opts.Image` as already-resolved.

**Note:** Resolution is pure string logic — no network, no podman, no side effects. It computes the image ref string but does not pull or verify the image exists. Pulling only happens in Task 7's `EnsureImage()`, which is called explicitly by subcommands that need the container (scan, fleet, refine, architect). Commands like `build` and `completion` never call `EnsureImage()`, so no pull occurs. Resolution runs everywhere; pulling does not.

- [ ] **Step 4: Write unit tests for image resolution**

```go
func TestResolveImage_FlagWins(t *testing.T) {
    img := container.ResolveImage("flag-image", "env-image", "pinned", "default")
    assert.Equal(t, "flag-image", img)
}

func TestResolveImage_EnvFallback(t *testing.T) {
    img := container.ResolveImage("", "env-image", "pinned", "default")
    assert.Equal(t, "env-image", img)
}

func TestResolveImage_PinnedFallback(t *testing.T) {
    img := container.ResolveImage("", "", "pinned", "default")
    assert.Equal(t, "pinned", img)
}

func TestResolveImage_CompiledDefault(t *testing.T) {
    img := container.ResolveImage("", "", "", "default")
    assert.Equal(t, "default", img)
}
```

### 6b: PodmanRunner interface for testability

Define an interface so unit tests never require a real podman binary. Reserve `//go:build integration` for tests that invoke podman.

- [ ] **Step 5: Define PodmanRunner interface**

`cmd/inspectah/internal/container/run.go`:

```go
// PodmanRunner abstracts podman invocation so unit tests can use fakes.
// Integration tests (//go:build integration) test against real podman.
type PodmanRunner interface {
    // Run executes a podman command and returns the exit code.
    Run(args []string) (int, error)
    // Exec replaces the current process with podman (for scan/fleet).
    Exec(args []string) error
    // ImageExists checks if an image is available locally.
    ImageExists(image string) (bool, error)
    // Pull pulls a container image with the given policy.
    Pull(image, policy string) error
}

// RealRunner invokes the actual podman binary.
type RealRunner struct {
    PodmanPath string
}

// FakeRunner records calls for unit testing. Never invokes podman.
type FakeRunner struct {
    RunCalls  [][]string
    ExecCalls [][]string
    ExitCode  int
    RunErr    error
}
```

Implement `RealRunner` methods that delegate to the exec/child process functions. Implement `FakeRunner` methods that record calls and return configured values.

### 6c: Invocation builder and execution strategies

- [ ] **Step 6: Write failing tests for command construction**

`cmd/inspectah/internal/container/run_test.go`:
```go
package container

import (
    "testing"

    "github.com/stretchr/testify/assert"

    "github.com/marrusl/inspectah/cmd/inspectah/internal/paths"
)

func TestBuildScanCommand(t *testing.T) {
    opts := &RunOpts{
        Podman:     "/usr/bin/podman",
        Image:      "ghcr.io/marrusl/inspectah:0.5.0",
        PullPolicy: "missing",
        Mounts: []paths.Mount{
            {Host: "/", Container: "/host", ReadOnly: true},
            {Host: "/home/user", Container: "/output", ReadOnly: false},
        },
        Subcommand: "scan",
        PassthroughArgs: []string{"--target-version", "9.6"},
        Privileged: true,
        PIDHost:    true,
    }
    args := BuildArgs(opts)

    assert.Contains(t, args, "--privileged")
    assert.Contains(t, args, "--pid=host")
    assert.Contains(t, args, "--pull=missing")
    assert.Contains(t, args, "-v")
    // Last args should be: image, subcommand, passthrough flags.
    last4 := args[len(args)-4:]
    assert.Equal(t, "ghcr.io/marrusl/inspectah:0.5.0", last4[0])
    assert.Equal(t, "scan", last4[1])
    assert.Equal(t, "--target-version", last4[2])
    assert.Equal(t, "9.6", last4[3])
}

func TestBuildRefineCommand_PortMapping(t *testing.T) {
    opts := &RunOpts{
        Podman:     "/usr/bin/podman",
        Image:      "ghcr.io/marrusl/inspectah:0.5.0",
        PullPolicy: "missing",
        Mounts: []paths.Mount{
            {Host: "/data", Container: "/input", ReadOnly: true},
        },
        Subcommand:      "refine",
        PassthroughArgs: []string{"/input/host.tar.gz"},
        PortMappings:    []string{"8642:8642"},
    }
    args := BuildArgs(opts)

    assert.Contains(t, args, "-p")
    // Find the index of -p and check the next element.
    for i, a := range args {
        if a == "-p" {
            assert.Equal(t, "8642:8642", args[i+1])
            break
        }
    }
}

func TestBuildArgs_DryRunOutput(t *testing.T) {
    opts := &RunOpts{
        Podman:     "podman",
        Image:      "ghcr.io/marrusl/inspectah:0.5.0",
        PullPolicy: "missing",
        Subcommand: "scan",
    }
    cmd := FormatDryRun(opts)
    assert.Contains(t, cmd, "podman run")
    assert.Contains(t, cmd, "inspectah:0.5.0")
}

func TestFakeRunner_RecordsCalls(t *testing.T) {
    fake := &FakeRunner{ExitCode: 0}
    code, err := fake.Run([]string{"run", "--rm", "image", "scan"})
    assert.NoError(t, err)
    assert.Equal(t, 0, code)
    assert.Len(t, fake.RunCalls, 1)
}
```

- [ ] **Step 7: Implement RunOpts and BuildArgs**

`cmd/inspectah/internal/container/run.go`:

Implement:
- `RunOpts` struct: `Podman`, `Image`, `PullPolicy`, `Mounts []paths.Mount`, `Subcommand`, `PassthroughArgs []string`, `Privileged`, `PIDHost`, `PortMappings []string`, `EnvVars map[string]string`, `Interactive bool`, `Remove bool` (default true)
- `BuildArgs(opts *RunOpts) []string` -- constructs the full `podman run` argument list. Order: `run`, `--rm`, `--pull=<policy>`, mount flags, port flags, env flags, privileged/pid flags, image, subcommand, passthrough args.
- `FormatDryRun(opts *RunOpts) string` -- format the full command for `--dry-run` display.

- [ ] **Step 8: Implement exec replacement strategy**

`cmd/inspectah/internal/container/exec.go`:

Implement `ExecReplace(podmanPath string, args []string) error` -- calls `syscall.Exec()` to replace the current process with podman. This is the strategy for `scan` and `fleet`.

```go
func ExecReplace(podmanPath string, args []string) error {
    fullArgs := append([]string{podmanPath}, args...)
    return syscall.Exec(podmanPath, fullArgs, os.Environ())
}
```

- [ ] **Step 9: Implement child process strategy**

`cmd/inspectah/internal/container/child.go`:

Implement `RunChild(podmanPath string, args []string) (int, error)` -- spawns podman as a child process, forwards SIGINT/SIGTERM, waits for exit, returns exit code. This is the strategy for `refine`, `architect`, and `build`.

```go
func RunChild(podmanPath string, args []string) (int, error) {
    cmd := exec.Command(podmanPath, args...)
    cmd.Stdout = os.Stdout
    cmd.Stderr = os.Stderr
    cmd.Stdin = os.Stdin

    if err := cmd.Start(); err != nil {
        return -1, err
    }

    // Forward signals to child.
    sigCh := make(chan os.Signal, 1)
    signal.Notify(sigCh, syscall.SIGINT, syscall.SIGTERM)
    go func() {
        sig := <-sigCh
        _ = cmd.Process.Signal(sig)
    }()

    err := cmd.Wait()
    signal.Stop(sigCh)

    if exitErr, ok := err.(*exec.ExitError); ok {
        return exitErr.ExitCode(), nil
    }
    if err != nil {
        return -1, err
    }
    return 0, nil
}
```

- [ ] **Step 10: Run tests**

```bash
cd cmd/inspectah && go test ./internal/container/ -v
```

- [ ] **Step 11: Commit**

```bash
git add cmd/inspectah/internal/container/ cmd/inspectah/internal/cli/env.go \
    cmd/inspectah/internal/cli/root.go
git commit -m "feat(go-cli): image resolution, PodmanRunner interface, invocation builder

Wire 4-step image resolution (flag > env > pin > default) into
PersistentPreRun so every subcommand gets a resolved image ref.
Define PodmanRunner interface with FakeRunner for unit tests --
real podman only needed for //go:build integration tests.
Two execution strategies: exec replacement and child process.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 7: Image pull, progress, and local presence check

**Files:** `cmd/inspectah/internal/container/image.go` (extend), `cmd/inspectah/internal/container/progress.go`

Orchestration-layer image operations that subcommands need: pull with progress, local presence check, and version compatibility. Config pin persistence and the `image use/update/info` UX stay in the `image` subcommand (task 12).

- [ ] **Step 1: Extend image.go with pull and version check**

Add to `cmd/inspectah/internal/container/image.go`:

- `PullImage(podman, image, policy string, verbose bool) error` -- run `podman pull` with the specified policy, parse progress output
- `CheckImageVersion(podman, image string) (string, error)` -- inspect the image for `org.opencontainers.image.version` label

These are orchestration functions consumed by subcommands. They work with the `PodmanRunner` interface defined in task 6 -- implementations take a runner, not a raw podman path, so unit tests can use `FakeRunner`.

- [ ] **Step 2: Implement pull progress display**

`cmd/inspectah/internal/container/progress.go`:

Implement:
- `DisplayPullProgress(reader io.Reader) error` -- parse podman pull progress lines and render a compact progress bar
- Progress format: `[################............]  58%  142 MB / 245 MB  12.3 MB/s`
- Parse JSON pull progress events from `podman pull --format json` (if available) or fall back to raw output passthrough

- [ ] **Step 3: Commit**

```bash
git add cmd/inspectah/internal/container/image.go cmd/inspectah/internal/container/progress.go
git commit -m "feat(go-cli): image pull with progress and version check

Pull with progress bar and version compatibility check against
embedded version matrix. Orchestration-layer ops consumed by
subcommands via PodmanRunner interface.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 8: `scan` subcommand

**Files:** `cmd/inspectah/internal/cli/scan.go`, `cmd/inspectah/internal/cli/scan_test.go`

The most complex subcommand: privileged, `--pid=host`, host root bind-mount, exec replacement, macOS gating.

- [ ] **Step 1: Write failing tests for scan flag handling**

`cmd/inspectah/internal/cli/scan_test.go`:
```go
package cli

import (
    "testing"

    "github.com/stretchr/testify/assert"
)

func TestScanCmd_DefaultFlags(t *testing.T) {
    cmd := newScanCmd(&GlobalOpts{Pull: "missing"})
    assert.Equal(t, "scan", cmd.Use)

    // Scan should accept passthrough flags without validation.
    err := cmd.Flags().Parse([]string{})
    assert.NoError(t, err)
}

func TestScanCmd_PassthroughArgs(t *testing.T) {
    // Verify that scan accepts unknown flags (passthrough to container).
    cmd := newScanCmd(&GlobalOpts{Pull: "missing"})
    cmd.FParseErrWhitelist.UnknownFlags = true
    // This verifies the configuration; actual passthrough tested in integration.
}
```

- [ ] **Step 2: Implement scan subcommand**

`cmd/inspectah/internal/cli/scan.go`:

Implement `newScanCmd(opts *GlobalOpts) *cobra.Command` that:
1. Checks `platform.CheckScanPlatform()` -- returns `ErrPlatformUnsupported` on macOS without invoking podman
2. Discovers podman via `platform.FindPodman(opts.Podman)`
3. Uses `opts.Image` (already resolved in `PersistentPreRun` -- flag > env > pin > default)
4. Pulls image if needed (`container.PullImage(...)`)
5. Checks version compatibility (`version.IsCompatible(...)`)
6. Builds mounts: `/:/host:ro` for host root, CWD to `/output` for output
7. Resolves output path if `-o` is specified
8. Constructs `RunOpts` with `Privileged: true`, `PIDHost: true`
9. If `--dry-run`: print command and exit
10. If `--verbose`: print command before executing
11. Calls `container.ExecReplace(...)` -- exec replacement strategy
12. Passes through all unrecognized flags via `cobra.ArbitraryArgs`

Register with root command.

- [ ] **Step 3: Wire scan into root command**

Add `root.AddCommand(newScanCmd(opts))` in `root.go`.

- [ ] **Step 4: Test manually (Linux only)**

```bash
cd cmd/inspectah
go build -o inspectah . && ./inspectah scan --dry-run
```

Expected on macOS: platform error. Expected on Linux: prints the podman command.

- [ ] **Step 5: Commit**

```bash
git add cmd/inspectah/internal/cli/scan.go cmd/inspectah/internal/cli/scan_test.go cmd/inspectah/internal/cli/root.go
git commit -m "feat(go-cli): scan subcommand with exec replacement strategy

Privileged container with host root bind-mount, pid=host, exec
replacement for direct terminal control. macOS gated with
actionable error message. Passthrough flags forwarded to container.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 8.5: Minimal CI job -- green-on-main gate

**Files:** `.github/workflows/go-cli.yml`

Don't wait until task 18 for CI. Stand up a minimal workflow now so every PR gets `go test ./... + lint` from the start. The full cross-compilation matrix stays in task 18.

- [ ] **Step 1: Write minimal Go CI workflow**

`.github/workflows/go-cli.yml`:
```yaml
name: Go CLI

on:
  push:
    paths:
      - 'cmd/inspectah/**'
      - '.github/workflows/go-cli.yml'
  pull_request:
    paths:
      - 'cmd/inspectah/**'

defaults:
  run:
    working-directory: cmd/inspectah

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-go@v5
        with:
          go-version: '1.21'
          cache-dependency-path: cmd/inspectah/go.sum

      - name: Build
        run: go build -v ./...

      - name: Unit tests
        run: go test -v ./...

      - name: Lint
        uses: golangci/golangci-lint-action@v6
        with:
          working-directory: cmd/inspectah
```

This is intentionally minimal: build + unit tests + lint on every PR. Cross-compilation matrix, integration tests, and release binaries are added in tasks 18-19.

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/go-cli.yml
git commit -m "ci(go-cli): minimal CI gate -- build, test, lint on every PR

Stand up early CI so green-on-main exists before subcommand
proliferation. Cross-compilation matrix added later in task 18.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 9: `fleet` subcommand

**Files:** `cmd/inspectah/internal/cli/fleet.go`

Similar to scan but takes a directory argument, maps it to `/input` inside the container.

- [ ] **Step 1: Implement fleet subcommand**

`cmd/inspectah/internal/cli/fleet.go`:

Implement `newFleetCmd(opts *GlobalOpts) *cobra.Command` that:
1. Takes a positional arg: `<dir>` (directory of scan tarballs)
2. Validates the directory exists and is readable
3. Bind-mounts the input directory to `/input:ro`
4. Bind-mounts CWD to `/output` (or resolves explicit `-o`)
5. Uses exec replacement strategy (same as scan)
6. No macOS gating -- fleet works on macOS
7. Does NOT require `--privileged` or `--pid=host`
8. Passes through all unrecognized flags

Register with root command.

- [ ] **Step 2: Test manually**

```bash
cd cmd/inspectah && go build -o inspectah . && ./inspectah fleet /tmp/test-dir --dry-run
```

Expected: prints the podman command with the directory bind-mounted.

- [ ] **Step 3: Commit**

```bash
git add cmd/inspectah/internal/cli/fleet.go cmd/inspectah/internal/cli/root.go
git commit -m "feat(go-cli): fleet subcommand with directory bind-mount

Map host input directory to /input in container. Exec replacement
strategy, no privilege escalation required. Works on macOS.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 10: `refine` and `architect` subcommands

**Files:** `cmd/inspectah/internal/cli/refine.go`, `cmd/inspectah/internal/cli/architect.go`

Child process mode with server readiness polling and browser launch. These two share enough structure to implement together.

- [ ] **Step 1: Implement shared server-mode helper**

Add to `cmd/inspectah/internal/container/child.go` (or a new `server.go`):

`RunServerChild(podman string, args []string, port int, noBrowser bool) (int, error)`:
1. Start podman as child process
2. Poll `http://127.0.0.1:<port>` for readiness (with timeout)
3. Print `Listening on http://127.0.0.1:<port>`
4. Open browser unless `--no-browser` (use `exec.Command("open", url)` on macOS, `xdg-open` on Linux)
5. Forward SIGINT/SIGTERM to child
6. Wait for child exit
7. Print shutdown message

- [ ] **Step 2: Implement refine subcommand**

`cmd/inspectah/internal/cli/refine.go`:

Implement `newRefineCmd(opts *GlobalOpts) *cobra.Command`:
1. Positional arg: `<tarball>` (`.tar.gz` file)
2. Validate tarball exists
3. Bind-mount tarball's parent directory to `/input:ro`
4. Port mapping: `8642:8642` (configurable via `--port`)
5. Uses child process strategy with server readiness polling
6. `--no-browser` flag to skip browser launch
7. Works on macOS

- [ ] **Step 3: Implement architect subcommand**

`cmd/inspectah/internal/cli/architect.go`:

Same pattern as refine with:
- Default port: 8643
- Different input types (can accept a directory or tarball)

- [ ] **Step 4: Register with root command**

Wire both into `root.go`.

- [ ] **Step 5: Test manually**

```bash
cd cmd/inspectah && go build -o inspectah .
./inspectah refine /tmp/test.tar.gz --dry-run
./inspectah architect /tmp/test-dir --dry-run
```

- [ ] **Step 6: Commit**

```bash
git add cmd/inspectah/internal/cli/refine.go cmd/inspectah/internal/cli/architect.go \
    cmd/inspectah/internal/container/ cmd/inspectah/internal/cli/root.go
git commit -m "feat(go-cli): refine and architect subcommands with server mode

Child process strategy with server readiness polling, auto browser
launch, and signal forwarding. Ports 8642 (refine) and 8643 (architect).

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 11: `build` subcommand

**Files:** `cmd/inspectah/internal/cli/build.go`, `cmd/inspectah/internal/cli/build_test.go`

Host-side `podman build` -- not a container passthrough. Validates Containerfile presence, constructs build command.

- [ ] **Step 1: Write failing tests for Containerfile validation**

`cmd/inspectah/internal/cli/build_test.go`:
```go
package cli

import (
    "os"
    "path/filepath"
    "testing"

    "github.com/stretchr/testify/assert"
    "github.com/stretchr/testify/require"
)

func TestValidateOutputDir_MissingContainerfile(t *testing.T) {
    dir := t.TempDir()
    err := validateBuildDir(dir)
    assert.Error(t, err)
    assert.Contains(t, err.Error(), "no Containerfile found")
}

func TestValidateOutputDir_WithContainerfile(t *testing.T) {
    dir := t.TempDir()
    err := os.WriteFile(filepath.Join(dir, "Containerfile"), []byte("FROM ubi9"), 0644)
    require.NoError(t, err)
    err = validateBuildDir(dir)
    assert.NoError(t, err)
}

func TestValidateOutputDir_WithDockerfile(t *testing.T) {
    dir := t.TempDir()
    err := os.WriteFile(filepath.Join(dir, "Dockerfile"), []byte("FROM ubi9"), 0644)
    require.NoError(t, err)
    err = validateBuildDir(dir)
    assert.NoError(t, err)
}

func TestBuildTag_Default(t *testing.T) {
    tag := deriveBuildTag("", "/data/my-host-output")
    assert.Equal(t, "inspectah-built/my-host-output:latest", tag)
}

func TestBuildTag_Explicit(t *testing.T) {
    tag := deriveBuildTag("myimage:v1", "/data/whatever")
    assert.Equal(t, "myimage:v1", tag)
}
```

- [ ] **Step 2: Implement build subcommand**

`cmd/inspectah/internal/cli/build.go`:

Implement `newBuildCmd(opts *GlobalOpts) *cobra.Command`:
1. Positional arg: `<output-dir>` (extracted scan output)
2. Flags: `--tag/-t`, `--no-cache`, `--pull` (build-specific, independent of global `--pull`), `--platform`, `--squash`
3. `validateBuildDir(dir)` -- check for `Containerfile` or `Dockerfile`
4. `deriveBuildTag(explicit, dir)` -- derive tag from dir name if not explicit
5. Construct `podman build -t <tag> -f <dir>/Containerfile <dir>/`
6. Run as child process (needs to report success/failure after build)
7. On success: print image reference and next-steps guidance
8. On failure: translate common build errors via the error layer
9. Works on both Linux and macOS

Register with root command.

- [ ] **Step 3: Run tests**

```bash
cd cmd/inspectah && go test ./internal/cli/ -v -run TestBuild
```

- [ ] **Step 4: Commit**

```bash
git add cmd/inspectah/internal/cli/build.go cmd/inspectah/internal/cli/build_test.go \
    cmd/inspectah/internal/cli/root.go
git commit -m "feat(go-cli): build subcommand for podman build on scan output

Validate Containerfile presence, derive image tag, construct and run
podman build. Child process mode for success/failure reporting.
Works on both Linux and macOS.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 12: `image` subcommand family -- config pin and UX

**Files:** `cmd/inspectah/internal/cli/image.go`, `cmd/inspectah/internal/cli/image_test.go`

User-facing image management: version pinning, update checking, and image info display. The orchestration-layer operations (pull, progress, local presence check) live in `container/image.go` from task 7. This task owns the UX layer: `image use`, `image update`, `image info`, and config pin persistence (`SavePinnedVersion`).

- [ ] **Step 1: Write failing tests for version pin normalization**

`cmd/inspectah/internal/cli/image_test.go`:
```go
package cli

import (
    "testing"

    "github.com/stretchr/testify/assert"
)

func TestImageUse_NormalizesVersion(t *testing.T) {
    // v0.5.1 and 0.5.1 should both resolve to the same image tag.
    tag := normalizeImageTag("v0.5.1")
    assert.Equal(t, "0.5.1", tag)

    tag = normalizeImageTag("0.5.1")
    assert.Equal(t, "0.5.1", tag)
}
```

- [ ] **Step 2: Implement image subcommand family**

`cmd/inspectah/internal/cli/image.go`:

Implement `newImageCmd(opts *GlobalOpts) *cobra.Command` with three sub-subcommands:

**`image use <version>`:**
1. Normalize version (strip `v` prefix)
2. Pull `ghcr.io/marrusl/inspectah:<version>` (via orchestration-layer `PullImage` from task 7)
3. Check version compatibility -- warn if outside range, prompt to confirm
4. Persist pin via `SavePinnedVersion()` to `~/.config/inspectah/config.yaml` (defined in this task, not task 7)
5. Print confirmation

**`image update`:**
1. Determine current version (pinned or compiled default)
2. Pull `:latest` tag
3. Inspect for version label
4. Update pin if version changed
5. Print old -> new version transition

**`image info`:**
1. Show current image reference, pin status, pull date, size, digest
2. Check for available updates (compare local vs remote `:latest`)
3. Show wrapper compatibility range

Register with root command.

- [ ] **Step 3: Run tests**

```bash
cd cmd/inspectah && go test ./internal/cli/ -v -run TestImage
```

- [ ] **Step 4: Commit**

```bash
git add cmd/inspectah/internal/cli/image.go cmd/inspectah/internal/cli/image_test.go \
    cmd/inspectah/internal/cli/root.go
git commit -m "feat(go-cli): image subcommand family (use, update, info)

Version pinning with v-prefix normalization, compatibility warnings,
update checking, and detailed image info display.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 13: Tab completion

**Files:** `cmd/inspectah/internal/cli/completion.go`

Cobra generates completion scripts. Register passthrough flags as hidden so they appear in completions.

- [ ] **Step 1: Implement completion subcommand**

`cmd/inspectah/internal/cli/completion.go`:

Implement `newCompletionCmd() *cobra.Command`:
1. Accept one positional arg: `bash`, `zsh`, `fish`, or `powershell`
2. Call the appropriate `cobra.Command.GenBashCompletion()` / etc. method
3. Output to stdout

- [ ] **Step 2: Register commonly used passthrough flags as hidden**

In `scan.go`, `fleet.go`, `refine.go`, register commonly used passthrough flags:
```go
// Hidden flags for completion only. Not validated by the wrapper.
cmd.Flags().StringP("output-file", "o", "", "")
cmd.Flags().String("from-snapshot", "", "")
cmd.Flags().String("target-version", "", "")
cmd.Flags().Bool("no-subscription", false, "")
cmd.Flags().Bool("inspect-only", false, "")
// Mark hidden so they don't appear in --help but do appear in completions.
cmd.Flags().MarkHidden("output-file")
// etc.
```

- [ ] **Step 3: Register with root command and test**

```bash
cd cmd/inspectah && go build -o inspectah .
./inspectah completion bash > /dev/null && echo "OK"
./inspectah completion zsh > /dev/null && echo "OK"
./inspectah completion fish > /dev/null && echo "OK"
```

- [ ] **Step 4: Commit**

```bash
git add cmd/inspectah/internal/cli/completion.go cmd/inspectah/internal/cli/root.go \
    cmd/inspectah/internal/cli/scan.go cmd/inspectah/internal/cli/fleet.go \
    cmd/inspectah/internal/cli/refine.go
git commit -m "feat(go-cli): tab completion generation for bash/zsh/fish

Cobra-generated completions with hidden passthrough flags registered
for scan, fleet, and refine. Completions always in sync with CLI.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 14: Container env passthrough and output dir binding

**Files:** `cmd/inspectah/internal/container/run.go` (modify)

Core env binding (`INSPECTAH_IMAGE`, `INSPECTAH_PULL`) was wired in task 6a. This task adds the remaining env vars that pass through to the container or affect output behavior.

- [ ] **Step 1: Pass INSPECTAH_DEBUG through to container env**

In `container/run.go`, add `INSPECTAH_DEBUG` to container env vars when set on host:
```go
if debug := os.Getenv("INSPECTAH_DEBUG"); debug != "" {
    opts.EnvVars["INSPECTAH_DEBUG"] = debug
}
```

- [ ] **Step 2: Add INSPECTAH_OUTPUT_DIR support**

In `env.go` (created in task 6a), add binding for `INSPECTAH_OUTPUT_DIR` -- used as the default output directory when `-o` is not specified and CWD is not appropriate.

- [ ] **Step 3: Commit**

```bash
git add cmd/inspectah/internal/container/run.go cmd/inspectah/internal/cli/env.go
git commit -m "feat(go-cli): container env passthrough (DEBUG, OUTPUT_DIR)

Pass INSPECTAH_DEBUG through to container env. Add OUTPUT_DIR
binding for default output directory. Core env binding (IMAGE, PULL)
was established in task 6a.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 15: Progress and first-run experience

**Files:** `cmd/inspectah/internal/container/image.go` (modify), `cmd/inspectah/internal/container/progress.go` (modify)

Polish the image pull experience, especially for first-time users.

- [ ] **Step 1: Detect first-run state**

In `image.go`, before pulling, check `ImageExists()`. If the image is not present locally, display first-run messaging:
```
inspectah v0.5.0 -- first run setup

Pulling container image (this may take a minute on slow connections)...
  [################............]  58%  142 MB / 245 MB  12.3 MB/s

Image ready. Starting scan...
```

- [ ] **Step 2: Add progress passthrough for scan/fleet output**

The container's own progress output (e.g., `[1/7] Collecting system info...`) passes through directly via stdout since we use exec replacement. No wrapper changes needed -- document this behavior.

- [ ] **Step 3: Add build progress passthrough**

Build output streams via child process stdout. No transformation needed, but add a completion message:
```
Build complete: inspectah-built/my-host:latest

Next steps:
  podman run -it inspectah-built/my-host:latest /bin/bash   # test it
  podman push inspectah-built/my-host:latest <registry>     # ship it
```

- [ ] **Step 4: Commit**

```bash
git add cmd/inspectah/internal/container/
git commit -m "feat(go-cli): first-run experience and progress feedback

Detect first-run state, display setup messaging with pull progress.
Build completion message with next-steps guidance.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 16: Integration tests

**Files:** `cmd/inspectah/internal/integration_test.go`, `cmd/inspectah/testdata/`

Integration tests require podman and are gated behind a build tag.

- [ ] **Step 1: Create test fixtures**

```bash
mkdir -p cmd/inspectah/testdata/fleet-input
# Create minimal test fixtures for scan and fleet integration tests.
```

Create `cmd/inspectah/testdata/minimal-snapshot.json` -- a minimal inspectah snapshot for `--from-snapshot` testing.

- [ ] **Step 2: Write build-tagged integration tests**

`cmd/inspectah/internal/integration_test.go`:
```go
//go:build integration

package internal

import (
    "os/exec"
    "path/filepath"
    "testing"

    "github.com/stretchr/testify/assert"
)

func TestScanDryRun(t *testing.T) {
    cmd := exec.Command("go", "run", "../../", "scan", "--dry-run")
    out, err := cmd.CombinedOutput()
    assert.NoError(t, err)
    assert.Contains(t, string(out), "podman run")
    assert.Contains(t, string(out), "--privileged")
}

func TestVersionOutput(t *testing.T) {
    cmd := exec.Command("go", "run", "../../", "version")
    out, err := cmd.CombinedOutput()
    assert.NoError(t, err)
    assert.Contains(t, string(out), "inspectah wrapper")
}

func TestCompletionBash(t *testing.T) {
    cmd := exec.Command("go", "run", "../../", "completion", "bash")
    out, err := cmd.CombinedOutput()
    assert.NoError(t, err)
    assert.Contains(t, string(out), "bash completion")
}

func TestScanRejectsNonLinux(t *testing.T) {
    if isLinux() {
        t.Skip("test only meaningful on non-Linux")
    }
    cmd := exec.Command("go", "run", "../../", "scan")
    out, _ := cmd.CombinedOutput()
    assert.Contains(t, string(out), "Linux host")
}
```

- [ ] **Step 3: Run integration tests (requires podman)**

```bash
cd cmd/inspectah && go test -tags=integration ./internal/ -v
```

- [ ] **Step 4: Commit**

```bash
git add cmd/inspectah/internal/integration_test.go cmd/inspectah/testdata/
git commit -m "test(go-cli): integration tests with podman (build-tagged)

Gated behind //go:build integration tag so they don't run in CI
without podman. Test dry-run, version, completion, and platform gating.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 17: Packaging -- RPM spec and Homebrew formula

**Files:** `packaging/inspectah.spec`, `packaging/inspectah.rb`

Packaging files for RPM (Fedora/RHEL COPR) and Homebrew (macOS).

- [ ] **Step 1: Create packaging directory**

```bash
mkdir -p /Users/mrussell/Work/bootc-migration/inspectah/packaging
```

- [ ] **Step 2: Write RPM spec file**

`packaging/inspectah.spec` -- based on the spec's RPM section. Key properties:
- `BuildRequires: golang >= 1.21`
- `Requires: podman >= 4.4`
- Build step: `cd cmd/inspectah && go build -ldflags "-X main.version=%{version}" -o ../../inspectah .`
- Install: binary to `%{_bindir}`, completion files generated at build time
- No Python, no pip, no venv

- [ ] **Step 3: Write Homebrew formula**

`packaging/inspectah.rb`:
```ruby
class Inspectah < Formula
  desc "Migration assessment tool (package-mode to image-mode)"
  homepage "https://github.com/marrusl/inspectah"
  url "https://github.com/marrusl/inspectah/archive/refs/tags/v0.5.0.tar.gz"
  sha256 "PLACEHOLDER"
  license "MIT"

  depends_on "go" => :build
  depends_on "podman"

  def install
    cd "cmd/inspectah" do
      ldflags = "-X main.version=#{version}"
      system "go", "build", "-ldflags", ldflags, "-o", bin/"inspectah", "."
    end

    generate_completions_from_executable(bin/"inspectah", "completion")
  end

  test do
    assert_match "inspectah wrapper", shell_output("#{bin}/inspectah version")
  end
end
```

- [ ] **Step 4: Commit**

```bash
git add packaging/
git commit -m "feat(go-cli): RPM spec and Homebrew formula

RPM spec for Fedora/RHEL COPR distribution with completion files
generated at build time. Homebrew formula for macOS installation.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 18: CI workflow expansion -- cross-compilation matrix

**Files:** `.github/workflows/go-cli.yml` (extend)

Expand the minimal CI from task 8.5 with cross-compilation matrix and integration test job. Does not replace the existing Python CI.

- [ ] **Step 1: Add cross-compilation job to existing CI workflow**

Extend `.github/workflows/go-cli.yml` (created in task 8.5) with a `cross-compile` job:

```yaml
  cross-compile:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        goos: [linux, darwin]
        goarch: [amd64, arm64]
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-go@v5
        with:
          go-version: '1.21'
          cache-dependency-path: cmd/inspectah/go.sum

      - name: Cross-compile
        env:
          GOOS: ${{ matrix.goos }}
          GOARCH: ${{ matrix.goarch }}
        run: |
          go build -ldflags "-X main.version=ci-test" \
            -o inspectah-${{ matrix.goos }}-${{ matrix.goarch }} .
```

Also add an integration test job (runs on a podman-equipped runner with `go test -tags=integration`).

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/go-cli.yml
git commit -m "ci(go-cli): expand CI with cross-compile matrix and integration tests

Add cross-compilation matrix (linux/darwin x amd64/arm64) and
integration test job to the minimal CI established in task 8.5.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 19: Release workflow updates

**Files:** `.github/workflows/release.yml` (modify)

Add Go binary cross-compilation and upload to the existing release workflow.

- [ ] **Step 1: Add Go build step to release workflow**

Add a job that:
1. Cross-compiles for `linux/amd64`, `linux/arm64`, `darwin/amd64`, `darwin/arm64`
2. Uses `go build -ldflags "-X main.version=${{ github.ref_name }} -X main.commit=${{ github.sha }} -X main.date=$(date -u +%Y-%m-%dT%H:%M:%SZ)"`
3. Uploads binaries as release assets with platform suffix: `inspectah-linux-amd64`, `inspectah-darwin-arm64`, etc.
4. Generates checksums file (`checksums.txt`)

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci(go-cli): add Go binary cross-compilation to release workflow

Build and upload Go wrapper binaries for linux/darwin x amd64/arm64
alongside existing Python container image. SHA256 checksums included.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 20: Migration -- deprecation notice and docs

**Files:** `run-inspectah.sh` (modify), `README.md` (modify)

Add deprecation breadcrumb to the shell script. Update README installation instructions.

- [ ] **Step 1: Add deprecation notice to run-inspectah.sh**

Add at the top of `run-inspectah.sh`, after the shebang:
```bash
echo "NOTE: run-inspectah.sh is deprecated. Install the inspectah CLI instead:"
echo "  sudo dnf copr enable marrusl/inspectah && sudo dnf install inspectah"
echo "  or: brew install marrusl/tap/inspectah"
echo ""
echo "Continuing with shell script..."
echo ""
```

The notice is informational, not blocking. The script continues to work.

- [ ] **Step 2: Update README.md**

Add a new "Installation" section near the top:
```markdown
## Installation

### RPM (Fedora/RHEL)

```bash
sudo dnf copr enable marrusl/inspectah
sudo dnf install inspectah
```

### Homebrew (macOS)

```bash
brew install marrusl/tap/inspectah
```

### From source

```bash
cd cmd/inspectah
go build -o inspectah .
sudo install inspectah /usr/local/bin/
```

### Shell script (legacy)

The `run-inspectah.sh` script still works but is deprecated.
The Go CLI provides error translation, tab completion, version
management, and a build subcommand.
```

- [ ] **Step 3: Commit**

```bash
git add run-inspectah.sh README.md
git commit -m "docs(go-cli): deprecation notice and updated installation docs

Add informational deprecation notice to run-inspectah.sh (non-blocking).
Update README with RPM, Homebrew, and source installation instructions.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Implementation Notes

**Incremental buildability:** Each task produces something that can be built and tested independently. Tasks 1-5 establish the foundation with no subcommands. Task 6 wires up the three things every subcommand needs: resolved image reference (with full 4-step precedence), PodmanRunner interface (for testability without podman), and the invocation builder. Task 7 adds orchestration-layer image ops (pull, progress, presence check). Task 8 delivers the first subcommand (scan), and task 8.5 stands up minimal CI (build + test + lint) so green-on-main exists before subcommand proliferation. Tasks 9-11 add remaining subcommands. Task 12 adds the `image` subcommand family (UX-layer pin/use/update/info). Tasks 13-15 add polish. Tasks 16-17 add packaging. Task 18 expands CI with cross-compilation. Tasks 19-20 handle release and migration.

**Test strategy:** Unit tests cover argument parsing, error classification, path resolution, version normalization, and image resolution. These run without podman -- the `PodmanRunner` interface with `FakeRunner` keeps unit tests honest without a podman dependency. Integration tests use `//go:build integration` and require a podman-equipped runner. Minimal CI (task 8.5) runs unit tests from day one; the full matrix expansion lands in task 18.

**Passthrough flag philosophy:** The wrapper registers commonly used passthrough flags as hidden Cobra flags for completion purposes, but does NOT validate them. Unknown flags are forwarded to the container's argparse, which is authoritative. This means the wrapper never needs to be updated when the Python CLI adds new flags -- only completions need updating.

**Two execution strategies, clear boundaries:** Exec replacement (scan, fleet) hands off completely to podman -- the Go process is replaced and never returns. Child process (refine, architect, build) maintains control for server management, browser launch, and exit code reporting. The strategy choice is per-subcommand and baked into the subcommand implementation.

**Config file location:** Version pins are stored in `~/.config/inspectah/config.yaml` following XDG conventions. The file is created on first `inspectah image use` and is optional -- the wrapper works without it.

**No feature flags or gradual rollout:** The user base is small enough (a few people) that the Go wrapper replaces the shell script cleanly. The deprecation notice in `run-inspectah.sh` is informational, not a gate.
