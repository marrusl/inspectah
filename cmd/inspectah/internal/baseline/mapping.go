// Package baseline handles base image selection and package baseline
// resolution for migration analysis. It maps host OS identity to bootc
// base image references and queries those images for package lists via
// podman.
package baseline

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"unicode"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// ---------------------------------------------------------------------------
// CommandRunner interface
// ---------------------------------------------------------------------------

// CommandResult holds the output of a command execution.
type CommandResult struct {
	Stdout   string
	Stderr   string
	ExitCode int
}

// CommandRunner abstracts command execution for the baseline resolver.
// This is a subset of inspector.Executor — only Run is needed.
type CommandRunner interface {
	Run(name string, args ...string) CommandResult
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

// rhelBootcMin maps RHEL major version to the minimum bootc-supported release.
var rhelBootcMin = map[string]string{
	"9":  "9.6",
	"10": "10.0",
}

// fedoraBootcMin is the minimum Fedora version with bootc support.
const fedoraBootcMin = "41"

// centosStreamImages maps CentOS Stream major version to the bootc image ref.
var centosStreamImages = map[string]string{
	"9":  "quay.io/centos-bootc/centos-bootc:stream9",
	"10": "quay.io/centos-bootc/centos-bootc:stream10",
}

// DefaultFallbackImage is the fallback base image when no mapping is found.
const DefaultFallbackImage = "registry.redhat.io/rhel9/rhel-bootc:9.6"

// PullTimeoutSeconds is the timeout for podman pull operations.
const PullTimeoutSeconds = 600

// rpmQAQueryformat is the queryformat used with rpm -qa for baseline queries.
const rpmQAQueryformat = `%{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}`

// ---------------------------------------------------------------------------
// SelectBaseImage
// ---------------------------------------------------------------------------

// SelectBaseImage maps host OS identity to the bootc base image reference.
//
// targetVersion overrides the auto-detected versionID (e.g. user wants to
// target 9.6 from a 9.4 source host).
//
// Returns (imageRef, effectiveVersion). Both are empty strings if the OS
// is unmapped.
func SelectBaseImage(osID, versionID, targetVersion string) (imageRef string, resolvedVersion string) {
	osID = strings.ToLower(osID)
	major := ""
	if versionID != "" {
		major = strings.SplitN(versionID, ".", 2)[0]
	}

	// RHEL
	if osID == "rhel" {
		if min, ok := rhelBootcMin[major]; ok {
			effective := targetVersion
			if effective == "" {
				effective = versionID
			}
			effective = clampVersion(effective, min)
			return fmt.Sprintf("registry.redhat.io/rhel%s/rhel-bootc:%s", major, effective), effective
		}
	}

	// CentOS Stream
	if strings.Contains(osID, "centos") {
		if img, ok := centosStreamImages[major]; ok {
			return img, major
		}
	}

	// Fedora
	if osID == "fedora" && major != "" {
		effective := targetVersion
		if effective == "" {
			effective = major
		}
		effective = clampVersion(effective, fedoraBootcMin)
		return fmt.Sprintf("quay.io/fedora/fedora-bootc:%s", effective), effective
	}

	return "", ""
}

// BaseImageForSnapshot determines the base image for a snapshot, with safe
// fallback. Prefers the value already resolved during inspection; falls back
// to SelectBaseImage from os_release; ultimately returns a RHEL 9 default
// so renderers always have a usable FROM line.
func BaseImageForSnapshot(snapshot *schema.InspectionSnapshot) string {
	if snapshot.Rpm != nil && snapshot.Rpm.BaseImage != nil && *snapshot.Rpm.BaseImage != "" {
		return *snapshot.Rpm.BaseImage
	}
	if snapshot.OsRelease != nil {
		image, _ := SelectBaseImage(snapshot.OsRelease.ID, snapshot.OsRelease.VersionID, "")
		if image != "" {
			return image
		}
	}
	return DefaultFallbackImage
}

// clampVersion returns versionID if it is >= minimum, otherwise returns
// minimum. Compares dot-separated integer components.
func clampVersion(versionID, minimum string) string {
	vParts, vErr := parseVersionParts(versionID)
	mParts, mErr := parseVersionParts(minimum)
	if vErr != nil || mErr != nil {
		return minimum
	}

	maxLen := len(vParts)
	if len(mParts) > maxLen {
		maxLen = len(mParts)
	}
	for i := 0; i < maxLen; i++ {
		v := 0
		if i < len(vParts) {
			v = vParts[i]
		}
		m := 0
		if i < len(mParts) {
			m = mParts[i]
		}
		if v < m {
			return minimum
		}
		if v > m {
			return versionID
		}
	}
	return versionID
}

// parseVersionParts splits a version string into integer components.
func parseVersionParts(version string) ([]int, error) {
	if version == "" {
		return nil, fmt.Errorf("empty version")
	}
	parts := strings.Split(version, ".")
	result := make([]int, len(parts))
	for i, p := range parts {
		n, err := strconv.Atoi(p)
		if err != nil {
			return nil, err
		}
		result[i] = n
	}
	return result, nil
}

// ---------------------------------------------------------------------------
// LoadBaselinePackagesFile
// ---------------------------------------------------------------------------

// LoadBaselinePackagesFile reads a baseline package list from a file path.
//
// Auto-detects format:
//   - NEVRA lines (epoch:name-version-release.arch) -> map keyed by name.arch
//   - Names-only lines -> map keyed by name, with empty version fields
//
// Returns nil and an error if the file cannot be read. Returns an empty map
// (not nil) if the file is empty.
func LoadBaselinePackagesFile(path string) (map[string]schema.PackageEntry, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("cannot read baseline packages file: %w", err)
	}

	text := string(data)
	lines := nonEmptyLines(text)
	if len(lines) == 0 {
		return map[string]schema.PackageEntry{}, nil
	}

	// Auto-detect: if the first non-empty line contains ":" and "-", treat
	// as NEVRA format.
	isNEVRA := strings.Contains(lines[0], ":") && strings.Contains(lines[0], "-")

	result := make(map[string]schema.PackageEntry, len(lines))
	if isNEVRA {
		for _, line := range lines {
			pkg := parseNEVRA(line)
			if pkg != nil {
				key := fmt.Sprintf("%s.%s", pkg.Name, pkg.Arch)
				result[key] = *pkg
			}
		}
	} else {
		for _, line := range lines {
			result[line] = schema.PackageEntry{
				Name:  line,
				Epoch: "0",
			}
		}
	}

	return result, nil
}

// nonEmptyLines splits text into lines, trims whitespace, and removes empties.
func nonEmptyLines(text string) []string {
	var result []string
	for _, line := range strings.Split(text, "\n") {
		line = strings.TrimSpace(line)
		if line != "" {
			result = append(result, line)
		}
	}
	return result
}

// ---------------------------------------------------------------------------
// NEVRA parsing (local copy — avoids import cycle with inspector)
// ---------------------------------------------------------------------------

// parseNEVRA parses a single line from rpm -qa --queryformat output.
// Format: epoch:name-version-release.arch
// Returns nil if the line cannot be parsed.
func parseNEVRA(line string) *schema.PackageEntry {
	s := strings.TrimSpace(line)
	if s == "" {
		return nil
	}

	colonIdx := strings.Index(s, ":")
	if colonIdx < 0 {
		return nil
	}

	epochPart := s[:colonIdx]
	rest := s[colonIdx+1:]

	var epoch string
	switch {
	case isDigits(epochPart):
		epoch = epochPart
	case epochPart == "(none)":
		epoch = "0"
	default:
		return nil
	}

	dotIdx := strings.LastIndex(rest, ".")
	if dotIdx < 0 {
		return nil
	}
	base := rest[:dotIdx]
	arch := rest[dotIdx+1:]

	parts := strings.Split(base, "-")
	if len(parts) < 3 {
		return nil
	}
	release := parts[len(parts)-1]
	version := parts[len(parts)-2]
	name := strings.Join(parts[:len(parts)-2], "-")

	return &schema.PackageEntry{
		Name:    name,
		Epoch:   epoch,
		Version: version,
		Release: release,
		Arch:    arch,
		State:   schema.PackageStateAdded,
	}
}

func isDigits(s string) bool {
	if s == "" {
		return false
	}
	for _, r := range s {
		if !unicode.IsDigit(r) {
			return false
		}
	}
	return true
}

// ---------------------------------------------------------------------------
// Module stream parsing (local copy — avoids import cycle with inspector)
// ---------------------------------------------------------------------------

// parseModuleStreams parses concatenated module INI text and returns
// {module_name: stream} for sections whose state is enabled or installed.
func parseModuleStreams(text string) map[string]string {
	result := make(map[string]string)
	var currentSection string
	kv := make(map[string]map[string]string)

	for _, line := range strings.Split(text, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") || strings.HasPrefix(line, ";") {
			continue
		}
		if strings.HasPrefix(line, "[") && strings.HasSuffix(line, "]") {
			currentSection = line[1 : len(line)-1]
			if kv[currentSection] == nil {
				kv[currentSection] = make(map[string]string)
			}
			continue
		}
		if currentSection == "" {
			continue
		}
		if idx := strings.Index(line, "="); idx >= 0 {
			key := strings.TrimSpace(line[:idx])
			val := strings.TrimSpace(line[idx+1:])
			kv[currentSection][key] = val
		}
	}

	for section, vals := range kv {
		state := strings.ToLower(strings.TrimSpace(vals["state"]))
		if state != "enabled" && state != "installed" {
			continue
		}
		stream := strings.TrimSpace(vals["stream"])
		if stream != "" {
			result[section] = stream
		}
	}
	return result
}
