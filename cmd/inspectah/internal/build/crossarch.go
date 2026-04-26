package build

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"sort"
	"strings"
)

// PkgSubstitution describes a single arch-specific package replacement.
type PkgSubstitution struct {
	From string
	To   string
}

// SubstitutionResult holds the outcome of a cross-arch substitution scan.
type SubstitutionResult struct {
	Substitutions   []PkgSubstitution
	Unmapped        []string
	ModifiedContent string
}

// archPkgMapping defines the bidirectional mapping between arch-specific packages.
// Key: package name on one arch. Value: equivalent on the other arch.
var archPkgMapping = map[string]string{
	// aarch64 -> x86_64
	"grub2-efi-aa64":         "grub2-efi-x64",
	"grub2-efi-aa64-cdboot":  "grub2-efi-x64-cdboot",
	"grub2-efi-aa64-modules": "grub2-efi-x64-modules",
	"shim-aa64":              "shim-x64",
	// x86_64 -> aarch64
	"grub2-efi-x64":         "grub2-efi-aa64",
	"grub2-efi-x64-cdboot":  "grub2-efi-aa64-cdboot",
	"grub2-efi-x64-modules": "grub2-efi-aa64-modules",
	"shim-x64":              "shim-aa64",
}

// archOnlyPackages are packages that exist only on a specific arch with no equivalent.
var archOnlyPackages = map[string]string{
	"grub2-pc":              "x86_64",
	"grub2-pc-modules":      "x86_64",
	"grub2-efi-ia32":        "x86_64",
	"grub2-efi-ia32-modules": "x86_64",
	"grub2-efi-ia32-cdboot": "x86_64",
	"grub2-ppc64le":         "ppc64le",
}

// pkgToSourceArch identifies which arch a package belongs to.
var pkgToSourceArch = map[string]string{
	"grub2-efi-aa64":         "aarch64",
	"grub2-efi-aa64-cdboot":  "aarch64",
	"grub2-efi-aa64-modules": "aarch64",
	"shim-aa64":              "aarch64",
	"grub2-efi-x64":          "x86_64",
	"grub2-efi-x64-cdboot":   "x86_64",
	"grub2-efi-x64-modules":  "x86_64",
	"shim-x64":               "x86_64",
	"grub2-pc":               "x86_64",
	"grub2-pc-modules":       "x86_64",
	"grub2-efi-ia32":         "x86_64",
	"grub2-efi-ia32-modules": "x86_64",
	"grub2-efi-ia32-cdboot":  "x86_64",
	"grub2-ppc64le":          "ppc64le",
}

// allArchSpecificPackages is the sorted list of all known arch-specific package names,
// longest first to prevent partial-match collisions during substitution.
var allArchSpecificPackages []string

func init() {
	seen := make(map[string]bool)
	for pkg := range pkgToSourceArch {
		if !seen[pkg] {
			allArchSpecificPackages = append(allArchSpecificPackages, pkg)
			seen[pkg] = true
		}
	}
	// Sort longest-first so "grub2-efi-aa64-cdboot" is matched before "grub2-efi-aa64".
	sort.Slice(allArchSpecificPackages, func(i, j int) bool {
		return len(allArchSpecificPackages[i]) > len(allArchSpecificPackages[j])
	})
}

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

	// On non-Linux hosts (macOS, Windows), podman runs builds inside a Linux VM,
	// so the effective build OS is always linux regardless of runtime.GOOS.
	buildOS := runtime.GOOS
	if buildOS != "linux" {
		buildOS = "linux"
	}

	var warnings []string
	warnings = append(warnings, fmt.Sprintf("Note: Building %s on %s/%s via QEMU — build will be slower.",
		platform, buildOS, hostArch))

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

// MapGoArchToRPMArch converts a Go architecture string to its RPM equivalent.
func MapGoArchToRPMArch(goarch string) string {
	switch goarch {
	case "amd64":
		return "x86_64"
	case "arm64":
		return "aarch64"
	case "s390x":
		return "s390x"
	case "ppc64le":
		return "ppc64le"
	default:
		return ""
	}
}

// InferSourceArch examines a Containerfile for arch-specific package names
// and returns the RPM arch of the source system (e.g., "aarch64", "x86_64").
// Returns "" if no arch-specific packages are found.
func InferSourceArch(containerfile string) string {
	for _, pkg := range allArchSpecificPackages {
		// Match as a standalone word (not a substring of another package name).
		pattern := `(?:^|[\s\\])` + regexp.QuoteMeta(pkg) + `(?:$|[\s\\])`
		if matched, _ := regexp.MatchString(pattern, containerfile); matched {
			return pkgToSourceArch[pkg]
		}
	}
	return ""
}

// FindArchSpecificPackages scans a Containerfile for arch-specific packages and returns:
//   - subs: packages that have a known equivalent on the target arch
//   - unmapped: packages that belong to the source arch but have no target equivalent
//
// When sourceArch == targetArch, both return values are empty.
func FindArchSpecificPackages(containerfile, sourceArch, targetArch string) ([]PkgSubstitution, []string, error) {
	if sourceArch == targetArch {
		return nil, nil, nil
	}

	var subs []PkgSubstitution
	var unmapped []string
	seen := make(map[string]bool)

	for _, pkg := range allArchSpecificPackages {
		// Check if package appears in the Containerfile as a standalone token.
		pattern := `(?:^|[\s\\])` + regexp.QuoteMeta(pkg) + `(?:$|[\s\\])`
		if matched, _ := regexp.MatchString(pattern, containerfile); !matched {
			continue
		}
		if seen[pkg] {
			continue
		}
		seen[pkg] = true

		pkgArch := pkgToSourceArch[pkg]

		// Package belongs to the target arch — no action needed.
		if pkgArch == targetArch {
			continue
		}

		// Package belongs to the source arch (or another arch) — needs substitution.
		if equiv, ok := archPkgMapping[pkg]; ok {
			subs = append(subs, PkgSubstitution{From: pkg, To: equiv})
		} else if _, isArchOnly := archOnlyPackages[pkg]; isArchOnly {
			unmapped = append(unmapped, pkg)
		}
	}

	return subs, unmapped, nil
}

// ApplySubstitutions replaces arch-specific package names in a Containerfile string.
// Substitutions are applied longest-first to prevent partial-match corruption.
func ApplySubstitutions(containerfile string, subs []PkgSubstitution) string {
	// Sort substitutions longest-first by the From field.
	sorted := make([]PkgSubstitution, len(subs))
	copy(sorted, subs)
	sort.Slice(sorted, func(i, j int) bool {
		return len(sorted[i].From) > len(sorted[j].From)
	})

	result := containerfile
	for _, s := range sorted {
		// Use word-boundary-aware replacement: the package name must be
		// surrounded by whitespace, backslash, or start/end of string.
		pattern := regexp.MustCompile(`(^|[\s\\])` + regexp.QuoteMeta(s.From) + `($|[\s\\])`)
		result = pattern.ReplaceAllStringFunc(result, func(match string) string {
			// Preserve the leading and trailing delimiters.
			prefix := ""
			suffix := ""
			trimmed := strings.TrimLeft(match, " \t\n\\")
			if len(trimmed) < len(match) {
				prefix = match[:len(match)-len(trimmed)]
			}
			trimmed2 := strings.TrimRight(trimmed, " \t\n\\")
			if len(trimmed2) < len(trimmed) {
				suffix = trimmed[len(trimmed2):]
			}
			return prefix + s.To + suffix
		})
	}
	return result
}

// WriteTempContainerfile writes content to a temporary Containerfile in dir.
// Returns the path, a cleanup function, and any error.
func WriteTempContainerfile(dir, content string) (string, func(), error) {
	f, err := os.CreateTemp(dir, "Containerfile.crossarch.*")
	if err != nil {
		return "", func() {}, fmt.Errorf("creating temp Containerfile: %w", err)
	}
	path := f.Name()
	if _, err := f.WriteString(content); err != nil {
		f.Close()
		os.Remove(path)
		return "", func() {}, fmt.Errorf("writing temp Containerfile: %w", err)
	}
	f.Close()

	cleanup := func() {
		os.Remove(path)
	}
	return path, cleanup, nil
}

// CrossArchSubstitute is the high-level entry point for cross-arch package handling.
// It takes GO arch names (amd64, arm64), converts internally.
//
// If strict is true, returns an error listing all arch-specific packages and their
// suggested substitutions instead of applying them.
//
// If any arch-specific packages have no known equivalent on the target arch, returns
// an error regardless of the strict flag.
func CrossArchSubstitute(containerfile string, sourceGoArch, targetGoArch string, strict bool) (*SubstitutionResult, error) {
	sourceRPM := MapGoArchToRPMArch(sourceGoArch)
	targetRPM := MapGoArchToRPMArch(targetGoArch)

	// If we can't determine RPM arch, try inferring from the Containerfile.
	if sourceRPM == "" {
		sourceRPM = InferSourceArch(containerfile)
	}
	if targetRPM == "" {
		return nil, fmt.Errorf("unsupported target architecture: %s", targetGoArch)
	}

	if sourceRPM == targetRPM || sourceRPM == "" {
		return &SubstitutionResult{ModifiedContent: containerfile}, nil
	}

	subs, unmapped, err := FindArchSpecificPackages(containerfile, sourceRPM, targetRPM)
	if err != nil {
		return nil, err
	}

	// Unmapped packages always error — we can't silently drop them.
	if len(unmapped) > 0 {
		var lines []string
		for _, pkg := range unmapped {
			lines = append(lines, fmt.Sprintf("  %s — no equivalent on %s", pkg, targetRPM))
		}
		return nil, fmt.Errorf("cross-arch build blocked: the following packages have no equivalent on %s:\n%s\n\nRemove these packages from the Containerfile or use --no-baseline to rebuild the package list.",
			targetRPM, strings.Join(lines, "\n"))
	}

	if len(subs) == 0 {
		return &SubstitutionResult{ModifiedContent: containerfile}, nil
	}

	// Strict mode: error with the substitution list so the user can fix manually.
	if strict {
		var lines []string
		for _, s := range subs {
			lines = append(lines, fmt.Sprintf("  %s → %s", s.From, s.To))
		}
		return nil, fmt.Errorf("cross-arch build: --strict-arch is set. The following arch-specific packages need substitution for %s:\n%s\n\nApply these changes to the Containerfile manually, or remove --strict-arch to auto-substitute.",
			targetRPM, strings.Join(lines, "\n"))
	}

	modified := ApplySubstitutions(containerfile, subs)
	return &SubstitutionResult{
		Substitutions:   subs,
		ModifiedContent: modified,
	}, nil
}
