# `inspectah build` Subcommand Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the thin `build.go` wrapper with a full-featured native Go build command that handles tarball extraction, RHEL entitlement cert detection/validation, cross-arch preflight, and structured output.

**Architecture:** The build command is a single cobra subcommand (`build.go`) that orchestrates a `build` package with four files: `input.go` (tarball/directory handling), `rhel.go` (Containerfile RHEL/UBI classification), `entitlement.go` (cert discovery cascade and expiry validation), `crossarch.go` (QEMU preflight), and `output.go` (structured messages). `build.go` handles flag parsing, podman command assembly, and execution. Each file has one clear responsibility and is independently testable.

**Tech Stack:** Go 1.21, cobra, `archive/tar` + `compress/gzip`, `crypto/x509`, `os/exec`, testify/assert

**Spec:** `docs/specs/2026-04-26-build-subcommand-design.md`

---

## File Structure

```
cmd/inspectah/internal/
├── cli/
│   ├── build.go              # REPLACE — cobra command, flag parsing, orchestration
│   └── build_test.go         # REPLACE — command-level tests
├── build/
│   ├── input.go              # CREATE — tarball extraction, directory validation
│   ├── input_test.go         # CREATE
│   ├── entitlement.go        # CREATE — cert discovery cascade, expiry validation
│   ├── entitlement_test.go   # CREATE
│   ├── rhel.go               # CREATE — Containerfile FROM parser, RHEL/UBI classification
│   ├── rhel_test.go          # CREATE
│   ├── crossarch.go          # CREATE — QEMU binfmt preflight
│   ├── crossarch_test.go     # CREATE
│   └── output.go             # CREATE — success/error message formatting
├── platform/
│   └── detect.go             # MODIFY — add IsMacOS(), HomeDir()
```

Each package is independently testable. `build.go` orchestrates them in sequence: resolve input → detect RHEL → discover certs → validate certs → preflight cross-arch → assemble podman args → execute → print output.

---

### Task 1: Safe Tarball Extraction (`build/input`)

**Files:**
- Create: `cmd/inspectah/internal/build/input.go`
- Create: `cmd/inspectah/internal/build/input_test.go`

- [ ] **Step 1: Write failing tests for tarball extraction safety**

```go
// cmd/inspectah/internal/build/input_test.go
package build

import (
	"archive/tar"
	"compress/gzip"
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func createTestTarball(t *testing.T, entries []tarEntry) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "test.tar.gz")
	f, err := os.Create(path)
	require.NoError(t, err)
	defer f.Close()

	gw := gzip.NewWriter(f)
	defer gw.Close()
	tw := tar.NewWriter(gw)
	defer tw.Close()

	for _, e := range entries {
		hdr := &tar.Header{
			Name:     e.Name,
			Typeflag: e.Type,
			Size:     int64(len(e.Body)),
			Mode:     0644,
		}
		if e.Linkname != "" {
			hdr.Linkname = e.Linkname
		}
		require.NoError(t, tw.WriteHeader(hdr))
		if len(e.Body) > 0 {
			_, err := tw.Write([]byte(e.Body))
			require.NoError(t, err)
		}
	}
	return path
}

type tarEntry struct {
	Name     string
	Type     byte
	Body     string
	Linkname string
}

func TestResolveInput_Tarball(t *testing.T) {
	tb := createTestTarball(t, []tarEntry{
		{Name: "host/Containerfile", Type: tar.TypeReg, Body: "FROM fedora:43\n"},
		{Name: "host/config/", Type: tar.TypeDir},
	})
	result, cleanup, err := ResolveInput(tb)
	require.NoError(t, err)
	defer cleanup()

	assert.DirExists(t, result.Dir)
	assert.FileExists(t, filepath.Join(result.Dir, "Containerfile"))
	assert.True(t, result.IsTarball)
}

func TestResolveInput_Directory(t *testing.T) {
	dir := t.TempDir()
	os.WriteFile(filepath.Join(dir, "Containerfile"), []byte("FROM fedora:43\n"), 0644)

	result, cleanup, err := ResolveInput(dir)
	require.NoError(t, err)
	defer cleanup()

	assert.Equal(t, dir, result.Dir)
	assert.False(t, result.IsTarball)
}

func TestResolveInput_MissingContainerfile(t *testing.T) {
	dir := t.TempDir()
	_, _, err := ResolveInput(dir)
	assert.ErrorContains(t, err, "No Containerfile found")
}

func TestResolveInput_RejectsPathTraversal(t *testing.T) {
	tb := createTestTarball(t, []tarEntry{
		{Name: "../escape.txt", Type: tar.TypeReg, Body: "evil"},
	})
	_, _, err := ResolveInput(tb)
	assert.ErrorContains(t, err, "path traversal")
}

func TestResolveInput_RejectsAbsolutePath(t *testing.T) {
	tb := createTestTarball(t, []tarEntry{
		{Name: "/etc/passwd", Type: tar.TypeReg, Body: "evil"},
	})
	_, _, err := ResolveInput(tb)
	assert.ErrorContains(t, err, "absolute path")
}

func TestResolveInput_RejectsSymlink(t *testing.T) {
	tb := createTestTarball(t, []tarEntry{
		{Name: "link", Type: tar.TypeSymlink, Linkname: "/etc/passwd"},
	})
	_, _, err := ResolveInput(tb)
	assert.ErrorContains(t, err, "symlink")
}

func TestResolveInput_RejectsHardlink(t *testing.T) {
	tb := createTestTarball(t, []tarEntry{
		{Name: "link", Type: tar.TypeLink, Linkname: "/etc/passwd"},
	})
	_, _, err := ResolveInput(tb)
	assert.ErrorContains(t, err, "hard link")
}

func TestResolveInput_RejectsDeviceNode(t *testing.T) {
	tb := createTestTarball(t, []tarEntry{
		{Name: "dev", Type: tar.TypeBlock},
	})
	_, _, err := ResolveInput(tb)
	assert.ErrorContains(t, err, "unsupported entry type")
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/build/ -v -run TestResolveInput`
Expected: compilation error — package `build` does not exist yet.

- [ ] **Step 3: Implement `input.go`**

```go
// cmd/inspectah/internal/build/input.go
package build

import (
	"archive/tar"
	"compress/gzip"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
)

type InputResult struct {
	Dir       string
	IsTarball bool
}

func ResolveInput(path string) (*InputResult, func(), error) {
	noop := func() {}

	if isTarball(path) {
		return extractTarball(path)
	}

	info, err := os.Stat(path)
	if err != nil {
		return nil, noop, fmt.Errorf("input path %q does not exist — this should be an inspectah scan tarball or extracted directory", path)
	}
	if !info.IsDir() {
		return nil, noop, fmt.Errorf("input path %q is not a directory or tarball", path)
	}

	if err := validateContainerfile(path); err != nil {
		return nil, noop, err
	}
	return &InputResult{Dir: path, IsTarball: false}, noop, nil
}

func isTarball(path string) bool {
	return strings.HasSuffix(path, ".tar.gz") || strings.HasSuffix(path, ".tgz")
}

func extractTarball(path string) (*InputResult, func(), error) {
	noop := func() {}

	f, err := os.Open(path)
	if err != nil {
		return nil, noop, fmt.Errorf("cannot open tarball: %w", err)
	}
	defer f.Close()

	homeDir, err := os.UserHomeDir()
	if err != nil {
		homeDir = os.TempDir()
	}
	cacheDir := filepath.Join(homeDir, ".cache", "inspectah")
	if err := os.MkdirAll(cacheDir, 0755); err != nil {
		return nil, noop, fmt.Errorf("cannot create cache directory: %w", err)
	}
	extractDir, err := os.MkdirTemp(cacheDir, "build-")
	if err != nil {
		return nil, noop, fmt.Errorf("cannot create temp directory: %w", err)
	}
	cleanup := func() { os.RemoveAll(extractDir) }

	gr, err := gzip.NewReader(f)
	if err != nil {
		cleanup()
		return nil, noop, fmt.Errorf("cannot decompress tarball: %w", err)
	}
	defer gr.Close()

	tr := tar.NewReader(gr)
	seen := make(map[string]bool)

	for {
		hdr, err := tr.Next()
		if err == io.EOF {
			break
		}
		if err != nil {
			cleanup()
			return nil, noop, fmt.Errorf("corrupt tarball: %w", err)
		}

		// Validate entry type first (rejects symlinks, hardlinks, devices)
		if err := validateEntryType(hdr); err != nil {
			cleanup()
			return nil, noop, err
		}

		// Compute final extraction target and validate it
		stripped := stripTopLevel(hdr.Name)
		if stripped == "" || stripped == "." {
			continue
		}
		target := filepath.Join(extractDir, stripped)
		if err := validateTarget(target, extractDir, seen); err != nil {
			cleanup()
			return nil, noop, err
		}

		switch hdr.Typeflag {
		case tar.TypeDir:
			if err := os.MkdirAll(target, 0755); err != nil {
				cleanup()
				return nil, noop, err
			}
		case tar.TypeReg:
			if err := os.MkdirAll(filepath.Dir(target), 0755); err != nil {
				cleanup()
				return nil, noop, err
			}
			out, err := os.OpenFile(target, os.O_CREATE|os.O_WRONLY, os.FileMode(hdr.Mode))
			if err != nil {
				cleanup()
				return nil, noop, err
			}
			if _, err := io.Copy(out, tr); err != nil {
				out.Close()
				cleanup()
				return nil, noop, err
			}
			out.Close()
		}

		seen[filepath.Clean(target)] = true
	}

	if err := validateContainerfile(extractDir); err != nil {
		cleanup()
		return nil, noop, err
	}

	return &InputResult{Dir: extractDir, IsTarball: true}, cleanup, nil
}

func validateEntryType(hdr *tar.Header) error {
	switch hdr.Typeflag {
	case tar.TypeReg, tar.TypeDir:
		return nil
	case tar.TypeSymlink:
		return fmt.Errorf("archive safety: symlink %q rejected", hdr.Name)
	case tar.TypeLink:
		return fmt.Errorf("archive safety: hard link %q rejected", hdr.Name)
	default:
		return fmt.Errorf("archive safety: unsupported entry type %d for %q", hdr.Typeflag, hdr.Name)
	}
}

func validateTarget(target, root string, seen map[string]bool) error {
	cleaned := filepath.Clean(target)

	if filepath.IsAbs(cleaned) && !strings.HasPrefix(cleaned, root+string(filepath.Separator)) {
		return fmt.Errorf("archive safety: absolute path rejected")
	}

	if !strings.HasPrefix(cleaned, root+string(filepath.Separator)) && cleaned != root {
		return fmt.Errorf("archive safety: path traversal rejected — resolved to %q", cleaned)
	}

	if seen[cleaned] {
		return fmt.Errorf("archive safety: duplicate path %q rejected", cleaned)
	}

	return nil
}

func stripTopLevel(name string) string {
	parts := strings.SplitN(name, "/", 2)
	if len(parts) < 2 {
		return name
	}
	return parts[1]
}

func validateContainerfile(dir string) error {
	cf := filepath.Join(dir, "Containerfile")
	if _, err := os.Stat(cf); err != nil {
		return fmt.Errorf("No Containerfile found in %s\n  This doesn't look like inspectah output. Run 'inspectah scan' first.", dir)
	}
	return nil
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/build/ -v -run TestResolveInput`
Expected: all 8 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add cmd/inspectah/internal/build/input.go cmd/inspectah/internal/build/input_test.go
git commit -m "feat(build): safe tarball extraction and input resolution

Handles .tar.gz and directory inputs with archive safety validation:
rejects path traversal, symlinks, hardlinks, device nodes, duplicates.
Extracts under \$HOME/.cache/inspectah/ for macOS podman machine compat.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 2: Containerfile RHEL/UBI Classifier (`build/rhel`)

**Files:**
- Create: `cmd/inspectah/internal/build/rhel.go`
- Create: `cmd/inspectah/internal/build/rhel_test.go`

- [ ] **Step 1: Write failing tests for RHEL detection**

```go
// cmd/inspectah/internal/build/rhel_test.go
package build

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestClassifyBuild_NonRHEL(t *testing.T) {
	cf := "FROM quay.io/fedora/fedora-bootc:43\nRUN dnf install -y vim\n"
	assert.Equal(t, DetectionNonEntitled, ClassifyBuild(cf))
}

func TestClassifyBuild_EntitledRHEL(t *testing.T) {
	cf := "FROM registry.redhat.io/rhel9/rhel-bootc:9.4\nRUN dnf install -y httpd\n"
	assert.Equal(t, DetectionEntitled, ClassifyBuild(cf))
}

func TestClassifyBuild_UBI(t *testing.T) {
	cf := "FROM registry.redhat.io/ubi9/ubi:latest\nRUN dnf install -y vim\n"
	assert.Equal(t, DetectionNonEntitled, ClassifyBuild(cf))
}

func TestClassifyBuild_UBIMinimal(t *testing.T) {
	cf := "FROM registry.redhat.io/ubi9/ubi-minimal:latest\nRUN microdnf install -y vim\n"
	assert.Equal(t, DetectionNonEntitled, ClassifyBuild(cf))
}

func TestClassifyBuild_UBITopLevel(t *testing.T) {
	cf := "FROM registry.redhat.io/ubi8:latest\n"
	assert.Equal(t, DetectionNonEntitled, ClassifyBuild(cf))
}

func TestClassifyBuild_MultiStageEntitled(t *testing.T) {
	cf := "FROM registry.redhat.io/ubi9/ubi AS builder\nRUN echo hi\nFROM registry.redhat.io/rhel9/rhel-bootc:9.4\nRUN dnf install -y httpd\n"
	assert.Equal(t, DetectionEntitled, ClassifyBuild(cf))
}

func TestClassifyBuild_PlatformFlag(t *testing.T) {
	cf := "FROM --platform=linux/arm64 registry.redhat.io/rhel9/rhel-bootc:9.4\n"
	assert.Equal(t, DetectionEntitled, ClassifyBuild(cf))
}

func TestClassifyBuild_ARGWithDefault(t *testing.T) {
	// ARG with default is still ambiguous because --build-arg can override
	cf := "ARG BASE=registry.redhat.io/rhel9/rhel-bootc:9.4\nFROM ${BASE}\n"
	assert.Equal(t, DetectionAmbiguous, ClassifyBuild(cf))
}

func TestClassifyBuild_ARGWithUBIDefault(t *testing.T) {
	// Even UBI defaults are ambiguous — --build-arg could point elsewhere
	cf := "ARG BASE=registry.redhat.io/ubi9/ubi:latest\nFROM ${BASE}\n"
	assert.Equal(t, DetectionAmbiguous, ClassifyBuild(cf))
}

func TestClassifyBuild_ARGNoDefault(t *testing.T) {
	cf := "ARG BASE\nFROM ${BASE}\n"
	assert.Equal(t, DetectionAmbiguous, ClassifyBuild(cf))
}

func TestClassifyBuild_MixedPublicAndUnresolved(t *testing.T) {
	cf := "FROM quay.io/fedora/fedora:43 AS builder\nARG PROD\nFROM ${PROD}\n"
	assert.Equal(t, DetectionAmbiguous, ClassifyBuild(cf))
}

func TestClassifyBuild_CommentsAndBlanks(t *testing.T) {
	cf := "# This is a comment\n\nFROM registry.redhat.io/ubi9/ubi:latest\n"
	assert.Equal(t, DetectionNonEntitled, ClassifyBuild(cf))
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/build/ -v -run TestClassifyBuild`
Expected: compilation error — `ClassifyBuild` not defined.

- [ ] **Step 3: Implement `rhel.go`**

```go
// cmd/inspectah/internal/build/rhel.go
package build

import (
	"regexp"
	"strings"
)

type Detection int

const (
	DetectionNonEntitled Detection = iota
	DetectionEntitled
	DetectionAmbiguous
)

var ubiPattern = regexp.MustCompile(`^registry\.redhat\.io/ubi[789]($|/.*)`)

func ClassifyBuild(containerfile string) Detection {
	argNames := parseARGNames(containerfile)
	stages := parseFROMs(containerfile, argNames)

	hasEntitled := false
	hasAmbiguous := false

	for _, img := range stages {
		switch classifyImage(img, argNames) {
		case DetectionEntitled:
			hasEntitled = true
		case DetectionAmbiguous:
			hasAmbiguous = true
		}
	}

	if hasEntitled {
		return DetectionEntitled
	}
	if hasAmbiguous {
		return DetectionAmbiguous
	}
	return DetectionNonEntitled
}

func parseARGNames(content string) map[string]bool {
	names := make(map[string]bool)
	for _, line := range strings.Split(content, "\n") {
		line = strings.TrimSpace(line)
		if !strings.HasPrefix(strings.ToUpper(line), "ARG ") {
			continue
		}
		rest := strings.TrimSpace(line[4:])
		name := rest
		if idx := strings.Index(rest, "="); idx >= 0 {
			name = rest[:idx]
		}
		names[name] = true
	}
	return names
}

func parseFROMs(content string, argNames map[string]bool) []string {
	var images []string
	for _, line := range strings.Split(content, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		upper := strings.ToUpper(line)
		if !strings.HasPrefix(upper, "FROM ") {
			continue
		}

		rest := strings.TrimSpace(line[5:])
		if strings.HasPrefix(rest, "--platform=") {
			parts := strings.Fields(rest)
			if len(parts) < 2 {
				continue
			}
			rest = strings.Join(parts[1:], " ")
		}

		img := strings.Fields(rest)[0]
		images = append(images, img)
	}
	return images
}

func classifyImage(img string, argNames map[string]bool) Detection {
	// Any ARG reference makes this ambiguous — --build-arg can override defaults
	if strings.Contains(img, "${") || strings.Contains(img, "$") {
		return DetectionAmbiguous
	}

	if !strings.HasPrefix(img, "registry.redhat.io/") {
		return DetectionNonEntitled
	}

	repoPath := strings.TrimPrefix(img, "registry.redhat.io/")
	repoPath = strings.Split(repoPath, ":")[0]
	repoPath = strings.Split(repoPath, "@")[0]

	if ubiPattern.MatchString("registry.redhat.io/" + repoPath) {
		return DetectionNonEntitled
	}

	return DetectionEntitled
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/build/ -v -run TestClassifyBuild`
Expected: all 12 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add cmd/inspectah/internal/build/rhel.go cmd/inspectah/internal/build/rhel_test.go
git commit -m "feat(build): Containerfile RHEL/UBI classifier

Static analysis of FROM directives: classifies builds as entitled,
ambiguous, or non-entitled. Handles ARG substitution, --platform,
multi-stage, and UBI repo path pattern matching.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 3: Entitlement Cert Discovery and Validation (`build/entitlement`)

**Files:**
- Create: `cmd/inspectah/internal/build/entitlement.go`
- Create: `cmd/inspectah/internal/build/entitlement_test.go`

- [ ] **Step 1: Write failing tests for cert discovery**

```go
// cmd/inspectah/internal/build/entitlement_test.go
package build

import (
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/pem"
	"math/big"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func writeFakeCert(t *testing.T, dir string, name string, expiry time.Time) {
	t.Helper()
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	require.NoError(t, err)
	tmpl := &x509.Certificate{
		SerialNumber: big.NewInt(1),
		Subject:      pkix.Name{CommonName: "test"},
		NotBefore:    time.Now().Add(-1 * time.Hour),
		NotAfter:     expiry,
	}
	der, err := x509.CreateCertificate(rand.Reader, tmpl, tmpl, &key.PublicKey, key)
	require.NoError(t, err)
	os.MkdirAll(dir, 0755)
	f, err := os.Create(filepath.Join(dir, name))
	require.NoError(t, err)
	defer f.Close()
	pem.Encode(f, &pem.Block{Type: "CERTIFICATE", Bytes: der})
}

func TestDiscoverCerts_BundledInTarball(t *testing.T) {
	dir := t.TempDir()
	entDir := filepath.Join(dir, "entitlement")
	writeFakeCert(t, entDir, "123.pem", time.Now().Add(24*time.Hour))

	result, err := DiscoverCerts(DiscoverOpts{OutputDir: dir})
	require.NoError(t, err)
	assert.Equal(t, DiscoveryCertsFound, result.Status)
	assert.Equal(t, entDir, result.EntitlementDir)
}

func TestDiscoverCerts_WithRHSM(t *testing.T) {
	dir := t.TempDir()
	entDir := filepath.Join(dir, "entitlement")
	rhsmDir := filepath.Join(dir, "rhsm")
	writeFakeCert(t, entDir, "123.pem", time.Now().Add(24*time.Hour))
	os.MkdirAll(rhsmDir, 0755)

	result, err := DiscoverCerts(DiscoverOpts{OutputDir: dir})
	require.NoError(t, err)
	assert.Equal(t, rhsmDir, result.RHSMDir)
}

func TestDiscoverCerts_NoEntitlements(t *testing.T) {
	result, err := DiscoverCerts(DiscoverOpts{SkipEntitlements: true})
	require.NoError(t, err)
	assert.Equal(t, DiscoveryNoCerts, result.Status)
}

func TestDiscoverCerts_ExplicitDirInvalid(t *testing.T) {
	_, err := DiscoverCerts(DiscoverOpts{EntitlementsDir: "/nonexistent/path"})
	assert.Error(t, err)
	assert.ErrorContains(t, err, "does not exist")
}

func TestDiscoverCerts_MutualExclusion(t *testing.T) {
	_, err := DiscoverCerts(DiscoverOpts{
		EntitlementsDir:  "/some/path",
		SkipEntitlements: true,
	})
	assert.ErrorContains(t, err, "mutually exclusive")
}

func TestValidateCertExpiry_Valid(t *testing.T) {
	dir := t.TempDir()
	writeFakeCert(t, dir, "valid.pem", time.Now().Add(24*time.Hour))

	err := ValidateCertExpiry(dir, false)
	assert.NoError(t, err)
}

func TestValidateCertExpiry_Expired(t *testing.T) {
	dir := t.TempDir()
	writeFakeCert(t, dir, "expired.pem", time.Now().Add(-1*time.Hour))

	err := ValidateCertExpiry(dir, false)
	assert.ErrorContains(t, err, "expired")
}

func TestValidateCertExpiry_IgnoreExpired(t *testing.T) {
	dir := t.TempDir()
	writeFakeCert(t, dir, "expired.pem", time.Now().Add(-1*time.Hour))

	err := ValidateCertExpiry(dir, true)
	assert.NoError(t, err)
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/build/ -v -run "TestDiscoverCerts|TestValidateCertExpiry"`
Expected: compilation error — `DiscoverCerts`, `ValidateCertExpiry` not defined.

- [ ] **Step 3: Implement `entitlement.go`**

```go
// cmd/inspectah/internal/build/entitlement.go
package build

import (
	"crypto/x509"
	"encoding/pem"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"time"
)

type DiscoveryStatus int

const (
	DiscoveryNoCerts DiscoveryStatus = iota
	DiscoveryCertsFound
	DiscoveryHostNative
)

type DiscoverResult struct {
	Status         DiscoveryStatus
	EntitlementDir string
	RHSMDir        string
}

type DiscoverOpts struct {
	EntitlementsDir  string // --entitlements-dir flag
	EnvDir           string // INSPECTAH_ENTITLEMENT_DIR
	OutputDir        string // extracted tarball or input directory
	SkipEntitlements bool   // --no-entitlements
}

func DiscoverCerts(opts DiscoverOpts) (*DiscoverResult, error) {
	if opts.SkipEntitlements && opts.EntitlementsDir != "" {
		return nil, fmt.Errorf("--no-entitlements and --entitlements-dir are mutually exclusive")
	}

	if opts.SkipEntitlements {
		return &DiscoverResult{Status: DiscoveryNoCerts}, nil
	}

	// Level 1: explicit flag
	if opts.EntitlementsDir != "" {
		return resolveExplicitDir(opts.EntitlementsDir)
	}

	// Level 2: env var
	if opts.EnvDir != "" {
		return resolveExplicitDir(opts.EnvDir)
	}

	// Level 3: RHEL host native (Linux only)
	if runtime.GOOS == "linux" {
		hostEnt := "/etc/pki/entitlement"
		if hasPEMs(hostEnt) {
			return &DiscoverResult{Status: DiscoveryHostNative}, nil
		}
	}

	// Level 4: bundled in tarball
	if opts.OutputDir != "" {
		bundled := filepath.Join(opts.OutputDir, "entitlement")
		if hasPEMs(bundled) {
			return resolveDir(bundled)
		}
	}

	// Level 5: user config
	homeDir, _ := os.UserHomeDir()
	if homeDir != "" {
		userConf := filepath.Join(homeDir, ".config", "inspectah", "entitlement")
		if hasPEMs(userConf) {
			return resolveDir(userConf)
		}
	}

	return &DiscoverResult{Status: DiscoveryNoCerts}, nil
}

func resolveExplicitDir(dir string) (*DiscoverResult, error) {
	info, err := os.Stat(dir)
	if err != nil {
		return nil, fmt.Errorf("entitlement directory %q does not exist", dir)
	}
	if !info.IsDir() {
		return nil, fmt.Errorf("entitlement path %q is not a directory", dir)
	}
	if !hasPEMs(dir) {
		return nil, fmt.Errorf("entitlement directory %q contains no .pem files", dir)
	}
	return resolveDir(dir)
}

func resolveDir(dir string) (*DiscoverResult, error) {
	abs, err := filepath.Abs(dir)
	if err != nil {
		return nil, err
	}
	result := &DiscoverResult{
		Status:         DiscoveryCertsFound,
		EntitlementDir: abs,
	}
	rhsmDir := filepath.Join(filepath.Dir(abs), "rhsm")
	if info, err := os.Stat(rhsmDir); err == nil && info.IsDir() {
		result.RHSMDir = rhsmDir
	}
	return result, nil
}

func hasPEMs(dir string) bool {
	matches, _ := filepath.Glob(filepath.Join(dir, "*.pem"))
	return len(matches) > 0
}

func ValidateCertExpiry(dir string, ignoreExpired bool) error {
	pems, _ := filepath.Glob(filepath.Join(dir, "*.pem"))
	now := time.Now()

	var expired []string
	var earliestExpiry time.Time

	for _, p := range pems {
		data, err := os.ReadFile(p)
		if err != nil {
			continue
		}
		block, _ := pem.Decode(data)
		if block == nil {
			continue
		}
		cert, err := x509.ParseCertificate(block.Bytes)
		if err != nil {
			continue
		}
		if cert.NotAfter.Before(now) {
			expired = append(expired, p)
			if earliestExpiry.IsZero() || cert.NotAfter.Before(earliestExpiry) {
				earliestExpiry = cert.NotAfter
			}
		}
	}

	if len(expired) > 0 && !ignoreExpired {
		return fmt.Errorf("RHEL entitlement cert expired (%s)\n  Certs: %s\n  Fix:   sudo subscription-manager refresh\n  Skip:  inspectah build --ignore-expired-certs ...",
			earliestExpiry.Format("2006-01-02"),
			strings.Join(expired, ", "))
	}
	return nil
}

func CheckMacOSPath(dir string) string {
	if runtime.GOOS != "darwin" {
		return ""
	}
	homeDir, _ := os.UserHomeDir()
	if homeDir == "" {
		return ""
	}
	abs, _ := filepath.Abs(dir)
	if !strings.HasPrefix(abs, homeDir) {
		return fmt.Sprintf("Warning: entitlement path %q is outside $HOME — it may not be accessible to podman machine. Consider copying certs under %s.", abs, homeDir)
	}
	return ""
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/build/ -v -run "TestDiscoverCerts|TestValidateCertExpiry"`
Expected: all 8 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add cmd/inspectah/internal/build/entitlement.go cmd/inspectah/internal/build/entitlement_test.go
git commit -m "feat(build): entitlement cert discovery and validation

Five-level cascade: flag > env > host-native > bundled > user-config.
x509 cert expiry validation. macOS path preflight for podman machine
compatibility. rhsm/ companion directory detection.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 4: Cross-Architecture Preflight (`build/crossarch`)

**Files:**
- Create: `cmd/inspectah/internal/build/crossarch.go`
- Create: `cmd/inspectah/internal/build/crossarch_test.go`
- Modify: `cmd/inspectah/internal/platform/detect.go` — add `IsMacOS()`

- [ ] **Step 1: Add `IsMacOS()` to platform package**

```go
// Add to cmd/inspectah/internal/platform/detect.go
func IsMacOS() bool {
	return runtime.GOOS == "darwin"
}
```

- [ ] **Step 2: Write failing tests for cross-arch preflight**

```go
// cmd/inspectah/internal/build/crossarch_test.go
package build

import (
	"runtime"
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestCrossArchCheck_SamePlatform(t *testing.T) {
	hostArch := runtime.GOARCH
	platform := "linux/" + hostArch
	warnings, err := CrossArchCheck(platform)
	assert.NoError(t, err)
	assert.Empty(t, warnings)
}

func TestCrossArchCheck_EmptyPlatform(t *testing.T) {
	warnings, err := CrossArchCheck("")
	assert.NoError(t, err)
	assert.Empty(t, warnings)
}

func TestCrossArchCheck_InvalidFormat(t *testing.T) {
	_, err := CrossArchCheck("justanarch")
	assert.ErrorContains(t, err, "format")
}
```

- [ ] **Step 3: Implement `crossarch.go`**

```go
// cmd/inspectah/internal/build/crossarch.go
package build

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
)

func CrossArchCheck(platform string) ([]string, error) {
	if platform == "" {
		return nil, nil
	}

	parts := strings.SplitN(platform, "/", 2)
	if len(parts) != 2 {
		return nil, fmt.Errorf("invalid --platform format %q — expected os/arch (e.g., linux/amd64)", platform)
	}
	targetArch := parts[1]

	hostArch := runtime.GOARCH
	if hostArch == targetArch {
		return nil, nil
	}

	var warnings []string
	warnings = append(warnings, fmt.Sprintf("Note: Building %s on %s/%s via QEMU — build will be slower.",
		platform, runtime.GOOS, hostArch))

	if runtime.GOOS == "linux" {
		binfmtArch := mapArchToBinfmt(targetArch)
		if binfmtArch != "" {
			handler := filepath.Join("/proc/sys/fs/binfmt_misc", "qemu-"+binfmtArch)
			if _, err := os.Stat(handler); err != nil {
				return nil, fmt.Errorf("cross-arch build requires qemu-user-static for %s\n  Install: sudo dnf install qemu-user-static\n  Then:    sudo systemctl restart systemd-binfmt",
					targetArch)
			}
		}
	}

	return warnings, nil
}

func mapArchToBinfmt(goarch string) string {
	switch goarch {
	case "amd64":
		return "x86_64"
	case "arm64":
		return "aarch64"
	case "arm":
		return "arm"
	case "s390x":
		return "s390x"
	case "ppc64le":
		return "ppc64le"
	default:
		return ""
	}
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/build/ -v -run TestCrossArch`
Expected: all 3 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add cmd/inspectah/internal/build/crossarch.go cmd/inspectah/internal/build/crossarch_test.go cmd/inspectah/internal/platform/detect.go
git commit -m "feat(build): cross-architecture QEMU preflight

Validates binfmt_misc handler on Linux. Skips check on macOS (podman
machine handles QEMU). Warns about slower cross-arch builds.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 5: Output Formatting (`build/output`)

**Files:**
- Create: `cmd/inspectah/internal/build/output.go`
- Create: `cmd/inspectah/internal/build/output_test.go`

- [ ] **Step 1: Write failing tests for output formatting**

```go
// cmd/inspectah/internal/build/output_test.go
package build

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestFormatSuccess(t *testing.T) {
	msg := FormatSuccess("localhost/my-migration:latest")
	assert.Contains(t, msg, "Built: localhost/my-migration:latest")
	assert.Contains(t, msg, "bcvk ephemeral run-ssh")
	assert.Contains(t, msg, "bootc switch")
	assert.Contains(t, msg, "podman push")
}

func TestFormatMissingPodman(t *testing.T) {
	msg := FormatMissingPodman()
	assert.Contains(t, msg, "podman not found")
	assert.Contains(t, msg, "sudo dnf install podman")
	assert.Contains(t, msg, "brew install podman")
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/build/ -v -run "TestFormatSuccess|TestFormatMissingPodman"`
Expected: compilation error.

- [ ] **Step 3: Implement `output.go`**

```go
// cmd/inspectah/internal/build/output.go
package build

import "fmt"

func FormatSuccess(tag string) string {
	return fmt.Sprintf(`Built: %s

Next steps:
  Test:   bcvk ephemeral run-ssh %s
  Switch: bootc switch %s
  Push:   podman push %s <registry>/%s`,
		tag, tag, tag, tag, stripLocalhost(tag))
}

func FormatMissingPodman() string {
	return `Error: podman not found
  Linux:  sudo dnf install podman
  macOS:  brew install podman && podman machine init && podman machine start`
}

func stripLocalhost(tag string) string {
	if len(tag) > 10 && tag[:10] == "localhost/" {
		return tag[10:]
	}
	return tag
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/build/ -v -run "TestFormatSuccess|TestFormatMissingPodman"`
Expected: all 2 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add cmd/inspectah/internal/build/output.go cmd/inspectah/internal/build/output_test.go
git commit -m "feat(build): structured output formatting

Success message with next-step hints (run, switch, bcvk, push).
Platform-specific podman install guidance.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 6: Replace `build.go` — Full Command Orchestration

**Files:**
- Replace: `cmd/inspectah/internal/cli/build.go`
- Replace: `cmd/inspectah/internal/cli/build_test.go`

- [ ] **Step 1: Write failing tests for the new build command**

```go
// cmd/inspectah/internal/cli/build_test.go
package cli

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestBuildCmd_Exists(t *testing.T) {
	cmd := newBuildCmd()
	assert.Equal(t, "build", cmd.Use[:5])
	assert.Contains(t, cmd.Short, "Build")
}

func TestBuildCmd_Flags(t *testing.T) {
	cmd := newBuildCmd()
	f := cmd.Flags()
	assert.NotNil(t, f.Lookup("tag"))
	assert.NotNil(t, f.Lookup("platform"))
	assert.NotNil(t, f.Lookup("entitlements-dir"))
	assert.NotNil(t, f.Lookup("no-entitlements"))
	assert.NotNil(t, f.Lookup("ignore-expired-certs"))
	assert.NotNil(t, f.Lookup("no-cache"))
	assert.NotNil(t, f.Lookup("pull"))
	assert.NotNil(t, f.Lookup("dry-run"))
	assert.NotNil(t, f.Lookup("verbose"))
}

func TestBuildCmd_RequiresInput(t *testing.T) {
	cmd := newBuildCmd()
	cmd.SetArgs([]string{})
	err := cmd.Execute()
	assert.Error(t, err)
	assert.ErrorContains(t, err, "requires")
}

func TestBuildCmd_AcceptsExtraArgs(t *testing.T) {
	cmd := newBuildCmd()
	err := cmd.Args(cmd, []string{"test.tar.gz"})
	assert.NoError(t, err)

	err = cmd.Args(cmd, []string{"test.tar.gz", "--build-arg", "FOO=bar"})
	assert.NoError(t, err)
}

func TestBuildCmd_TagShorthand(t *testing.T) {
	cmd := newBuildCmd()
	f := cmd.Flags().Lookup("tag")
	assert.Equal(t, "t", f.Shorthand)
}

func TestBuildCmd_MutualExclusion(t *testing.T) {
	cmd := newBuildCmd()
	cmd.SetArgs([]string{"test.tar.gz", "-t", "img:latest",
		"--no-entitlements", "--entitlements-dir", "/some/path"})
	err := cmd.Execute()
	assert.ErrorContains(t, err, "mutually exclusive")
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/cli/ -v -run TestBuildCmd`
Expected: some tests fail (new flags don't exist yet).

- [ ] **Step 3: Replace `build.go` with full implementation**

```go
// cmd/inspectah/internal/cli/build.go
package cli

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"os/signal"
	"strings"
	"syscall"

	build "github.com/marrusl/inspectah/cmd/inspectah/internal/build"
	"github.com/spf13/cobra"
)

func newBuildCmd() *cobra.Command {
	var (
		tag              string
		platform         string
		entitlementsDir  string
		noEntitlements   bool
		ignoreExpired    bool
		noCache          bool
		pull             string
		dryRun           bool
		verbose          bool
	)

	cmd := &cobra.Command{
		Use:   "build <tarball|directory> -t <image:tag> [flags] [-- extra-podman-args...]",
		Short: "Build a bootc image from inspectah output",
		Long: `Build a bootc container image from an inspectah scan/refine tarball
or extracted directory.

Runs podman build natively on the workstation. Handles RHEL entitlement
cert detection, validation, and injection automatically.

Extra arguments after -- are passed directly to podman build
(e.g., --build-arg, --secret, --squash).`,
		Args: cobra.ArbitraryArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			if len(args) == 0 {
				return fmt.Errorf("requires a tarball or directory argument\n\nUsage: inspectah build <tarball|directory> -t <image:tag>")
			}

			if tag == "" {
				return fmt.Errorf("--tag (-t) is required")
			}

			// Check podman is available
			podmanPath, err := exec.LookPath("podman")
			if err != nil {
				return fmt.Errorf(build.FormatMissingPodman())
			}

			// Resolve input (tarball or directory)
			input, cleanup, err := build.ResolveInput(args[0])
			if err != nil {
				return err
			}
			defer cleanup()

			// Handle cleanup on signals — cancel context, wait for child, then clean up
			ctx, cancel := context.WithCancel(cmd.Context())
			defer cancel()
			var podProcess *os.Process
			sigCh := make(chan os.Signal, 1)
			signal.Notify(sigCh, syscall.SIGINT, syscall.SIGTERM)
			go func() {
				<-sigCh
				cancel()
				if podProcess != nil {
					podProcess.Wait() // reap the child before cleanup
				}
				cleanup()
				os.Exit(1)
			}()

			// Read Containerfile for RHEL detection
			cfPath := input.Dir + "/Containerfile"
			cfData, err := os.ReadFile(cfPath)
			if err != nil {
				return fmt.Errorf("cannot read Containerfile: %w", err)
			}

			// Mutual exclusion check happens before any detection gating
			if noEntitlements && entitlementsDir != "" {
				return fmt.Errorf("--no-entitlements and --entitlements-dir are mutually exclusive")
			}

			detection := build.ClassifyBuild(string(cfData))

			// --no-entitlements overrides detection to non-entitled
			if noEntitlements {
				detection = build.DetectionNonEntitled
			}

			// Only run cert discovery for entitled/ambiguous builds
			var certs *build.DiscoverResult
			if detection == build.DetectionNonEntitled {
				certs = &build.DiscoverResult{Status: build.DiscoveryNoCerts}
			} else {
				var err error
				certs, err = build.DiscoverCerts(build.DiscoverOpts{
					EntitlementsDir:  entitlementsDir,
					EnvDir:           os.Getenv("INSPECTAH_ENTITLEMENT_DIR"),
					OutputDir:        input.Dir,
					SkipEntitlements: false,
				})
				if err != nil {
					return err
				}
			}

			// Validate cert expiry (only when certs were actually discovered)
			if certs.Status == build.DiscoveryCertsFound {
				if err := build.ValidateCertExpiry(certs.EntitlementDir, ignoreExpired); err != nil {
					return err
				}
				if warning := build.CheckMacOSPath(certs.EntitlementDir); warning != "" {
					fmt.Fprintln(os.Stderr, warning)
				}
			}

			// Warn if entitled/ambiguous with no certs
			if certs.Status == build.DiscoveryNoCerts && detection != build.DetectionNonEntitled {
				fmt.Fprintln(os.Stderr, "Warning: RHEL entitlement certs not found. Build may fail if subscribed repos are needed.")
				fmt.Fprintln(os.Stderr, "  Copy from RHEL host:  scp root@rhel-host:/etc/pki/entitlement/*.pem ./entitlement/")
				fmt.Fprintln(os.Stderr, "  Silence this warning: inspectah build --no-entitlements ...")
			}

			// Cross-arch preflight
			if platform != "" {
				warnings, err := build.CrossArchCheck(platform)
				if err != nil {
					return err
				}
				for _, w := range warnings {
					fmt.Fprintln(os.Stderr, w)
				}
			}

			// Assemble podman build command
			podmanArgs := []string{"build", "-f", cfPath, "-t", tag}
			if platform != "" {
				podmanArgs = append(podmanArgs, "--platform="+platform)
			}
			if noCache {
				podmanArgs = append(podmanArgs, "--no-cache")
			}
			if pull != "" {
				podmanArgs = append(podmanArgs, "--pull="+pull)
			}

			// Entitlement volume mounts (only for entitled/ambiguous with discovered certs)
			if certs.Status == build.DiscoveryCertsFound {
				podmanArgs = append(podmanArgs, "-v", certs.EntitlementDir+":/etc/pki/entitlement:ro")
				if certs.RHSMDir != "" {
					podmanArgs = append(podmanArgs, "-v", certs.RHSMDir+":/etc/rhsm:ro")
				}
			}

			// Passthrough args (everything after the first positional arg)
			podmanArgs = append(podmanArgs, args[1:]...)
			podmanArgs = append(podmanArgs, input.Dir)

			if dryRun || verbose {
				fmt.Fprintf(os.Stderr, "podman %s\n", strings.Join(podmanArgs, " "))
				if dryRun {
					return nil
				}
			}

			fmt.Fprintf(os.Stderr, "Building image from %s\n", input.Dir)

			// Execute podman build
			podCmd := exec.CommandContext(ctx, podmanPath, podmanArgs...)
			podCmd.Stdout = os.Stdout
			podCmd.Stderr = os.Stderr

			if err := podCmd.Start(); err != nil {
				return fmt.Errorf("podman build failed to start: %w", err)
			}
			podProcess = podCmd.Process

			if err := podCmd.Wait(); err != nil {
				if exitErr, ok := err.(*exec.ExitError); ok {
					return fmt.Errorf("podman build exited with code %d", exitErr.ExitCode())
				}
				return fmt.Errorf("podman build failed: %w", err)
			}

			fmt.Fprintln(os.Stderr)
			fmt.Fprintln(os.Stderr, build.FormatSuccess(tag))
			return nil
		},
	}

	cmd.Flags().StringVarP(&tag, "tag", "t", "", "image name:tag (required)")
	cmd.Flags().StringVar(&platform, "platform", "", "target os/arch (e.g., linux/arm64)")
	cmd.Flags().StringVar(&entitlementsDir, "entitlements-dir", "", "explicit entitlement cert directory")
	cmd.Flags().BoolVar(&noEntitlements, "no-entitlements", false, "skip entitlement detection entirely")
	cmd.Flags().BoolVar(&ignoreExpired, "ignore-expired-certs", false, "proceed despite expired entitlement certs")
	cmd.Flags().BoolVar(&noCache, "no-cache", false, "do not use cache when building")
	cmd.Flags().StringVar(&pull, "pull", "", "base image pull policy (always, missing, never, newer)")
	cmd.Flags().BoolVar(&dryRun, "dry-run", false, "print the podman command without executing")
	cmd.Flags().BoolVar(&verbose, "verbose", false, "print the podman command before executing")

	return cmd
}
```

- [ ] **Step 4: Run all tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/cli/ -v -run TestBuildCmd && go test ./internal/build/ -v`
Expected: all tests PASS across both packages.

- [ ] **Step 5: Run full test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./... -v`
Expected: all Go tests PASS.

- [ ] **Step 6: Commit**

```bash
git add cmd/inspectah/internal/cli/build.go cmd/inspectah/internal/cli/build_test.go
git commit -m "feat(build): full build subcommand with entitlement and cross-arch support

Replaces the thin build wrapper with the complete implementation:
tarball/directory input, RHEL/UBI classification, entitlement cert
discovery cascade, x509 expiry validation, cross-arch QEMU preflight,
structured success/error output.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 7: Integration Smoke Test

**Files:**
- No new files — manual verification

- [ ] **Step 1: Verify `--help` output**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go run . build --help`
Expected: shows all flags including `--tag`, `--platform`, `--entitlements-dir`, `--no-entitlements`, `--ignore-expired-certs`, `--no-cache`, `--pull`, `--dry-run`, `--verbose`.

- [ ] **Step 2: Verify `--dry-run` with a test tarball**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go run . build <path-to-test-tarball> -t test:latest --dry-run`
Expected: prints the full `podman build` command without executing it.

- [ ] **Step 3: Verify error messages**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go run . build /nonexistent -t test:latest`
Expected: clear error about path not existing.

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go run . build /tmp -t test:latest`
Expected: "No Containerfile found" error.

- [ ] **Step 4: Run full test suite one final time**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./... && cd ../.. && python -m pytest tests/ -q`
Expected: all Go and Python tests pass.

- [ ] **Step 5: Commit**

```bash
git commit -m "test(build): integration smoke test pass

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 8: Retire `inspectah-build` References

**Lifecycle decision:** The standalone `inspectah-build` Python script is
removed in this change. No shim, no deprecation wrapper. The Go CLI `build`
subcommand is the sole build entry point going forward.

**Files to modify (known surfaces):**
- Remove: `inspectah-build` (standalone Python script at repo root)
- Modify: `README.md` — update build workflow examples
- Modify: `docs/how-to/build-bootc-image.md` — rewrite to reference `inspectah build`
- Modify: `docs/reference/cli.md` (if exists) — update command reference
- Modify: `src/inspectah/packaging.py` — update "Next steps" output text
- Modify: `src/inspectah/templates/report/_summary.html.j2` — update build instructions
- Modify: any other templates/renderers printing `inspectah-build` in output

- [ ] **Step 1: Find all references to `inspectah-build`**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && grep -rn 'inspectah-build\|inspectah_build\|inspectah\.build' --include='*.md' --include='*.py' --include='*.go' --include='*.sh' --include='*.j2' --include='*.html' | grep -v '.git/' | grep -v __pycache__`

Review the output and categorize each hit as: remove, update text, or update test assertion.

- [ ] **Step 2: Remove the standalone script**

```bash
git rm inspectah-build
```

- [ ] **Step 3: Update `src/inspectah/packaging.py` next-steps output**

Find the "Next steps" block that prints `./inspectah-build`. Update to:
```
Next steps:
  Copy to workstation:    scp {hostname}:{tarball_path} .
  Interactive refinement: inspectah refine {tarball_name}
  Build the image:        inspectah build {tarball_name} -t my-image:latest
```

- [ ] **Step 4: Update `docs/how-to/build-bootc-image.md`**

Replace `./inspectah-build` examples with `inspectah build` equivalents.
Remove docker-specific sections. Remove `--push`/`--registry` examples.
Update flag names to match the new CLI (`--entitlements-dir`, `--no-entitlements`,
`--ignore-expired-certs`, `--platform`).

- [ ] **Step 5: Update report templates**

Search `src/inspectah/templates/` for `inspectah-build` references.
Update any build instructions in HTML report output to reference
`inspectah build`.

- [ ] **Step 6: Update remaining docs**

Update `README.md` and any other docs found in step 1.

- [ ] **Step 7: Fix broken test assertions**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && python -m pytest tests/ -q`

If any tests fail due to changed output strings (e.g., "inspectah-build"
in expected output), update the test assertions to match the new text.

- [ ] **Step 8: Run full test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./... && cd ../.. && python -m pytest tests/ -q`
Expected: all tests pass.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "chore: retire inspectah-build standalone script

Remove the Python build script. All docs, templates, report output,
and test assertions now reference 'inspectah build' (Go CLI subcommand).

Assisted-by: Claude Code (Opus 4.6)"
```
