package baseline

import (
	"fmt"
	"os"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// ---------------------------------------------------------------------------
// ResolveOptions
// ---------------------------------------------------------------------------

// ResolveOptions configures the baseline resolution strategy.
type ResolveOptions struct {
	// OsID is the host OS identifier (e.g. "rhel", "centos", "fedora").
	OsID string

	// VersionID is the host OS version (e.g. "9.4").
	VersionID string

	// BaselineFile is a path to a pre-exported baseline package list.
	// When set, podman queries are skipped (air-gapped mode).
	BaselineFile string

	// TargetVersion overrides the host version for base image selection.
	TargetVersion string

	// TargetImage overrides automatic base image selection entirely.
	TargetImage string
}

// ---------------------------------------------------------------------------
// Resolver
// ---------------------------------------------------------------------------

// Resolver queries base images for package lists, systemd presets, and
// module streams. It caches results per image reference within a single
// instance so repeated calls with the same image skip redundant podman runs.
//
// Unlike the Python BaselineResolver, this does NOT use nsenter -- the Go
// binary runs directly on the host, so podman commands are executed directly.
type Resolver struct {
	exec CommandRunner

	// Session caches: keyed by image reference.
	packageCache map[string]map[string]schema.PackageEntry
	presetCache  map[string]string
	moduleCache  map[string]map[string]string
}

// NewResolver creates a Resolver that uses the given runner for podman
// commands. If exec is nil, all podman queries are skipped.
func NewResolver(exec CommandRunner) *Resolver {
	return &Resolver{
		exec:         exec,
		packageCache: make(map[string]map[string]schema.PackageEntry),
		presetCache:  make(map[string]string),
		moduleCache:  make(map[string]map[string]string),
	}
}

// ---------------------------------------------------------------------------
// Resolve — top-level entry point
// ---------------------------------------------------------------------------

// Resolve resolves the baseline package set, base image reference, and
// whether baseline mode is available.
//
// Returns (baselinePackages, baseImageRef, noBaseline).
//
// Strategy (in priority order):
//  1. TargetImage set: use that image (CLI override)
//  2. BaselineFile set: load from file (air-gapped mode)
//  3. Auto-detect: SelectBaseImage(osID, versionID, targetVersion) then query
//  4. Fall back to no-baseline mode
func (r *Resolver) Resolve(opts ResolveOptions) (map[string]schema.PackageEntry, string, bool) {
	// 1. CLI target-image override
	if opts.TargetImage != "" {
		if opts.BaselineFile != "" {
			pkgs, err := LoadBaselinePackagesFile(opts.BaselineFile)
			if err != nil || len(pkgs) == 0 {
				return nil, opts.TargetImage, true
			}
			return pkgs, opts.TargetImage, false
		}
		if r.exec != nil {
			pkgs, err := r.QueryPackages(opts.TargetImage)
			if err != nil {
				return nil, opts.TargetImage, true
			}
			return pkgs, opts.TargetImage, false
		}
		return nil, opts.TargetImage, true
	}

	// 2. Explicit baseline file
	if opts.BaselineFile != "" {
		pkgs, err := LoadBaselinePackagesFile(opts.BaselineFile)
		if err == nil && len(pkgs) > 0 {
			baseImage, _ := SelectBaseImage(opts.OsID, opts.VersionID, opts.TargetVersion)
			return pkgs, baseImage, false
		}
	}

	// 3. Auto-detect base image and query via podman
	baseImage, _ := SelectBaseImage(opts.OsID, opts.VersionID, opts.TargetVersion)
	if baseImage != "" && r.exec != nil {
		pkgs, err := r.QueryPackages(baseImage)
		if err == nil && len(pkgs) > 0 {
			return pkgs, baseImage, false
		}
	}

	// 4. No baseline available
	return nil, baseImage, true
}

// ---------------------------------------------------------------------------
// Podman queries
// ---------------------------------------------------------------------------

// QueryPackages runs podman to get the package list from a base image.
// Returns a map of "name.arch" -> PackageEntry. Uses the session cache.
func (r *Resolver) QueryPackages(baseImage string) (map[string]schema.PackageEntry, error) {
	if cached, ok := r.packageCache[baseImage]; ok {
		return cached, nil
	}

	if !r.checkRegistryAuth(baseImage) {
		return nil, fmt.Errorf("registry auth check failed for %s", baseImage)
	}

	if err := r.ensureImagePulled(baseImage); err != nil {
		return nil, err
	}

	queryformat := rpmQAQueryformat + `\n`
	result := r.exec.Run(
		"podman", "run", "--rm", "--cgroups=disabled", baseImage,
		"rpm", "-qa", "--queryformat", queryformat,
	)
	if result.ExitCode != 0 {
		return nil, fmt.Errorf("podman run rpm -qa failed (rc=%d): %s",
			result.ExitCode, truncate(result.Stderr, 800))
	}

	packages := make(map[string]schema.PackageEntry)
	for _, line := range strings.Split(strings.TrimSpace(result.Stdout), "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		pkg := parseNEVRA(line)
		if pkg != nil {
			key := fmt.Sprintf("%s.%s", pkg.Name, pkg.Arch)
			packages[key] = *pkg
		}
	}

	r.packageCache[baseImage] = packages
	return packages, nil
}

// QueryPresets gets systemd preset data from the base image.
// Returns the concatenated preset text, or empty string on failure.
func (r *Resolver) QueryPresets(baseImage string) (string, error) {
	if cached, ok := r.presetCache[baseImage]; ok {
		return cached, nil
	}

	if !r.checkRegistryAuth(baseImage) {
		return "", fmt.Errorf("registry auth check failed for %s", baseImage)
	}

	if err := r.ensureImagePulled(baseImage); err != nil {
		return "", err
	}

	result := r.exec.Run(
		"podman", "run", "--rm", "--cgroups=disabled", baseImage,
		"bash", "-c",
		"cat /usr/lib/systemd/system-preset/*.preset 2>/dev/null || true",
	)
	if result.ExitCode != 0 {
		return "", fmt.Errorf("preset query failed (rc=%d): %s",
			result.ExitCode, truncate(result.Stderr, 200))
	}

	text := strings.TrimSpace(result.Stdout)
	if text == "" {
		return "", nil
	}

	r.presetCache[baseImage] = result.Stdout
	return result.Stdout, nil
}

// QueryModuleStreams gets enabled module streams from the base image.
// Returns {module_name: stream}. Uses the session cache.
func (r *Resolver) QueryModuleStreams(baseImage string) (map[string]string, error) {
	if cached, ok := r.moduleCache[baseImage]; ok {
		return cached, nil
	}

	if !r.checkRegistryAuth(baseImage) {
		return nil, fmt.Errorf("registry auth check failed for %s", baseImage)
	}

	if err := r.ensureImagePulled(baseImage); err != nil {
		return nil, err
	}

	result := r.exec.Run(
		"podman", "run", "--rm", "--cgroups=disabled", baseImage,
		"bash", "-c",
		"cat /etc/dnf/modules.d/*.module 2>/dev/null || true",
	)
	if result.ExitCode != 0 {
		return nil, fmt.Errorf("module stream query failed (rc=%d)", result.ExitCode)
	}

	text := strings.TrimSpace(result.Stdout)
	if text == "" {
		return map[string]string{}, nil
	}

	streams := parseModuleStreams(text)
	r.moduleCache[baseImage] = streams
	return streams, nil
}

// ---------------------------------------------------------------------------
// Image management
// ---------------------------------------------------------------------------

// ensureImagePulled checks if the image is cached locally, and pulls it
// if not. Returns an error if the pull fails.
func (r *Resolver) ensureImagePulled(baseImage string) error {
	result := r.exec.Run("podman", "image", "exists", baseImage)
	if result.ExitCode == 0 {
		return nil
	}

	fmt.Fprintf(os.Stderr, "  Pulling baseline image %s...\n", baseImage)
	pullResult := r.exec.Run("podman", "pull", baseImage)
	if pullResult.ExitCode != 0 {
		return fmt.Errorf("podman pull failed (rc=%d): %s",
			pullResult.ExitCode, truncate(pullResult.Stderr, 200))
	}
	return nil
}

// checkRegistryAuth checks if podman has credentials for the image's
// registry. Only checks registry.redhat.io (public registries don't need auth).
func (r *Resolver) checkRegistryAuth(image string) bool {
	if !strings.Contains(image, "registry.redhat.io") {
		return true
	}
	result := r.exec.Run("podman", "login", "--get-login", "registry.redhat.io")
	if result.ExitCode != 0 {
		fmt.Fprintf(os.Stderr,
			"ERROR: No credentials for registry.redhat.io. "+
				"The base image cannot be pulled without authentication.\n"+
				"Fix: run 'podman login registry.redhat.io' on the host first.\n"+
				"Alternative: provide a pre-exported package list via --baseline-packages FILE.\n")
		return false
	}
	return true
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// truncate returns the first n bytes of s, or s if shorter.
func truncate(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n]
}
