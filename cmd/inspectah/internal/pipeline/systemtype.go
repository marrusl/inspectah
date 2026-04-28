// Package pipeline implements the inspection pipeline stages: system-type
// detection, base image mapping, and (eventually) inspector orchestration.
package pipeline

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/inspector"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// OstreeDetectionError is returned when an ostree system is detected
// (the /ostree directory exists) but neither bootc nor rpm-ostree can
// identify the system type.
type OstreeDetectionError struct {
	Message string
}

func (e *OstreeDetectionError) Error() string { return e.Message }

// DetectSystemType determines whether the host is package-mode, bootc, or
// rpm-ostree. Detection order matches the Python reference:
//
//  1. No /ostree directory -> package-mode
//  2. /ostree + "bootc status" succeeds -> bootc
//  3. /ostree + "rpm-ostree status" succeeds -> rpm-ostree
//  4. /ostree + both fail -> OstreeDetectionError
func DetectSystemType(exec inspector.Executor) (schema.SystemType, error) {
	if !exec.FileExists("/ostree") {
		return schema.SystemTypePackageMode, nil
	}

	result := exec.Run("bootc", "status")
	if result.ExitCode == 0 {
		return schema.SystemTypeBootc, nil
	}

	result = exec.Run("rpm-ostree", "status")
	if result.ExitCode == 0 {
		return schema.SystemTypeRpmOstree, nil
	}

	return "", &OstreeDetectionError{
		Message: "Detected ostree system (/ostree exists) but could not determine\n" +
			"system type -- both 'bootc status' and 'rpm-ostree status' failed.\n" +
			"\n" +
			"This system may use an ostree configuration inspectah does not yet support.",
	}
}

// ---------------------------------------------------------------------------
// Base image mapping
// ---------------------------------------------------------------------------

// fedoraOstreeDesktops is the set of known Fedora Atomic desktop variant IDs.
var fedoraOstreeDesktops = map[string]bool{
	"silverblue":     true,
	"kinoite":        true,
	"sway-atomic":    true,
	"budgie-atomic":  true,
	"lxqt-atomic":    true,
	"xfce-atomic":    true,
	"cosmic-atomic":  true,
}

// MapOstreeBaseImage maps an ostree/bootc source system to its container
// base image reference. Returns ("", nil) when the system is unknown.
//
// Resolution order:
//  1. targetImageOverride (from --target-image CLI flag)
//  2. Universal Blue image-info.json
//  3. System-type-specific mapping (rpm-ostree variant or bootc status/os-release)
func MapOstreeBaseImage(
	exec inspector.Executor,
	osRelease *schema.OsRelease,
	systemType schema.SystemType,
	targetImageOverride string,
) (string, error) {
	// 1. CLI override takes absolute precedence
	if targetImageOverride != "" {
		return targetImageOverride, nil
	}

	// 2. Universal Blue check (applies to both rpm-ostree and bootc UBlue systems)
	ublueRef, ubluePresent := readUblueImageInfo(exec)
	if ubluePresent {
		// File exists -- use whatever we got (empty string = malformed, refuse to guess)
		return ublueRef, nil
	}
	// File doesn't exist -- fall through to standard mapping

	// 3. System-type-specific mapping
	switch systemType {
	case schema.SystemTypeRpmOstree:
		return mapRpmOstreeBaseImage(osRelease), nil
	case schema.SystemTypeBootc:
		return mapBootcBaseImage(exec, osRelease), nil
	default:
		// package-mode or unexpected type
		return "", nil
	}
}

// readUblueImageInfo reads /usr/share/ublue-os/image-info.json and returns
// (ref, true) if the file exists, or ("", false) if the file is absent.
// When the file exists but is malformed, returns ("", true) so the caller
// refuses to guess.
func readUblueImageInfo(exec inspector.Executor) (string, bool) {
	const ublueInfoPath = "/usr/share/ublue-os/image-info.json"

	content, err := exec.ReadFile(ublueInfoPath)
	if err != nil {
		// File doesn't exist
		return "", false
	}

	var data map[string]interface{}
	if err := json.Unmarshal([]byte(content), &data); err != nil {
		// Malformed JSON
		return "", true
	}

	// Require image-name and image-vendor for validity
	if _, hasName := data["image-name"]; !hasName {
		return "", true
	}
	if _, hasVendor := data["image-vendor"]; !hasVendor {
		return "", true
	}

	// Prefer explicit image-ref
	if ref, ok := data["image-ref"].(string); ok && ref != "" {
		return ref, true
	}

	// Synthesis fallback: construct from vendor/name/tag
	vendor, _ := data["image-vendor"].(string)
	name, _ := data["image-name"].(string)
	tag, _ := data["image-tag"].(string)
	if vendor != "" && name != "" && tag != "" {
		return fmt.Sprintf("ghcr.io/%s/%s:%s", vendor, name, tag), true
	}

	// Has the file but can't determine ref
	return "", true
}

// mapRpmOstreeBaseImage maps an rpm-ostree system to its base image using
// the Fedora ostree desktop variant table.
func mapRpmOstreeBaseImage(osRelease *schema.OsRelease) string {
	variant := osRelease.VariantID
	if fedoraOstreeDesktops[variant] {
		return fmt.Sprintf("quay.io/fedora-ostree-desktops/%s:%s", variant, osRelease.VersionID)
	}
	return ""
}

// mapBootcBaseImage maps a bootc system to its base image. Tries
// "bootc status --json" first, then falls back to os-release mapping.
func mapBootcBaseImage(exec inspector.Executor, osRelease *schema.OsRelease) string {
	// Try bootc status --json
	ref := bootcStatusImageRef(exec)
	if ref != "" {
		return ref
	}

	// Fall back to os-release mapping
	return mapBootcFromOsRelease(osRelease)
}

// bootcStatusImageRef parses "bootc status --json" for the booted image ref.
func bootcStatusImageRef(exec inspector.Executor) string {
	result := exec.Run("bootc", "status", "--json")
	if result.ExitCode != 0 {
		return ""
	}

	// Navigate: status.booted.image.image.image
	var data map[string]interface{}
	if err := json.Unmarshal([]byte(result.Stdout), &data); err != nil {
		return ""
	}

	status, _ := data["status"].(map[string]interface{})
	if status == nil {
		return ""
	}
	booted, _ := status["booted"].(map[string]interface{})
	if booted == nil {
		return ""
	}
	imageOuter, _ := booted["image"].(map[string]interface{})
	if imageOuter == nil {
		return ""
	}
	imageInner, _ := imageOuter["image"].(map[string]interface{})
	if imageInner == nil {
		return ""
	}
	ref, _ := imageInner["image"].(string)
	return ref
}

// mapBootcFromOsRelease maps a bootc system to its base image using
// os-release fields.
func mapBootcFromOsRelease(osRelease *schema.OsRelease) string {
	osID := osRelease.ID
	ver := osRelease.VersionID

	switch osID {
	case "fedora":
		return fmt.Sprintf("quay.io/fedora/fedora-bootc:%s", ver)
	case "centos":
		major := strings.SplitN(ver, ".", 2)[0]
		return fmt.Sprintf("quay.io/centos-bootc/centos-bootc:stream%s", major)
	case "rhel":
		major := strings.SplitN(ver, ".", 2)[0]
		return fmt.Sprintf("registry.redhat.io/rhel%s/rhel-bootc:%s", major, ver)
	default:
		return ""
	}
}
