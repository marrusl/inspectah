package renderer

import (
	"regexp"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// shellUnsafeRe matches characters that would change shell semantics if
// injected into a RUN command. The data comes from RPM databases / systemd
// on an operator-controlled host, so this is a safety net against
// corrupted snapshots, not a security boundary.
var shellUnsafeRe = regexp.MustCompile(`[;&|$` + "`" + `"'\\<>(){}\n\r]`)

// tunedProfileRe matches valid tuned profile names: alphanumeric, hyphens,
// underscores only. Stricter than sanitizeShellValue because the name is
// interpolated directly into an echo redirect.
var tunedProfileRe = regexp.MustCompile(`^[A-Za-z0-9_-]+$`)

// Exact bare-word kernel parameters that are always managed by the
// bootloader or base image and must never appear in a kargs.d drop-in.
var kargsBootloaderExact = map[string]bool{
	"ro":        true,
	"rhgb":      true,
	"quiet":     true,
	"splash":    true,
	"nosplash":  true,
	"noplymouth": true,
}

// Prefixes whose matching kargs are likewise bootloader/installer-owned.
var kargsBootloaderPrefixes = []string{
	"root=",
	"rd.lvm.lv=",
	"rd.luks.uuid=",
	"resume=",
	"BOOT_IMAGE=",
	"initrd=",
	"LANG=",
	"console=",
	"crashkernel=",
}

// sanitizeShellValue returns nil if val contains shell-unsafe characters.
func sanitizeShellValue(val, context string) *string {
	if shellUnsafeRe.MatchString(val) {
		return nil
	}
	return &val
}

// isBootloaderKarg returns true if karg is a bootloader-managed parameter.
func isBootloaderKarg(karg string) bool {
	if kargsBootloaderExact[karg] {
		return true
	}
	for _, prefix := range kargsBootloaderPrefixes {
		if strings.HasPrefix(karg, prefix) {
			return true
		}
	}
	return false
}

// operatorKargs extracts operator-defined kargs from a cmdline string,
// filtering out bootloader-managed ones and those with unsafe characters.
func operatorKargs(cmdline string) []string {
	var result []string
	for _, karg := range strings.Fields(cmdline) {
		if isBootloaderKarg(karg) {
			continue
		}
		if sanitizeShellValue(karg, "kargs") == nil {
			continue
		}
		result = append(result, karg)
	}
	return result
}

// baseImageFromSnapshot returns the base image reference string.
func baseImageFromSnapshot(snap *schema.InspectionSnapshot) string {
	if snap.Rpm != nil && snap.Rpm.BaseImage != nil && *snap.Rpm.BaseImage != "" {
		return *snap.Rpm.BaseImage
	}
	return "registry.redhat.io/rhel9/rhel-bootc:9.4"
}

// dhcpConnectionPaths returns the set of NM connection paths that use
// DHCP, which should be excluded from the config tree copy.
func dhcpConnectionPaths(snap *schema.InspectionSnapshot) map[string]bool {
	paths := make(map[string]bool)
	if snap.Network == nil {
		return paths
	}
	for _, conn := range snap.Network.Connections {
		if conn.Method == "auto" && conn.Path != "" {
			paths[strings.TrimPrefix(conn.Path, "/")] = true
		}
	}
	return paths
}

// summariseDiff produces human-readable change summaries from a unified diff.
func summariseDiff(diffText string) []string {
	additions := make(map[string]string)
	removals := make(map[string]string)
	var other []string

	for _, line := range strings.Split(strings.TrimSpace(diffText), "\n") {
		if strings.HasPrefix(line, "---") || strings.HasPrefix(line, "+++") || strings.HasPrefix(line, "@@") {
			continue
		}
		var stripped string
		if len(line) > 1 {
			stripped = strings.TrimSpace(line[1:])
		}
		if stripped == "" || strings.HasPrefix(stripped, "#") {
			continue
		}

		if strings.HasPrefix(line, "-") {
			if idx := strings.IndexAny(stripped, "=:"); idx >= 0 {
				sep := string(stripped[idx])
				key := strings.TrimSpace(stripped[:idx])
				val := strings.TrimSpace(stripped[idx+len(sep):])
				removals[key] = val
			} else {
				other = append(other, "removed: "+stripped)
			}
		} else if strings.HasPrefix(line, "+") {
			if idx := strings.IndexAny(stripped, "=:"); idx >= 0 {
				key := strings.TrimSpace(stripped[:idx])
				val := strings.TrimSpace(stripped[strings.IndexAny(stripped, "=:")+1:])
				additions[key] = val
			} else {
				other = append(other, "added: "+stripped)
			}
		}
	}

	var results []string
	matchedKeys := make(map[string]bool)
	for key, val := range additions {
		if oldVal, ok := removals[key]; ok {
			results = append(results, key+": "+oldVal+" → "+val)
			matchedKeys[key] = true
		} else {
			results = append(results, key+": added ("+val+")")
		}
	}
	for key := range removals {
		if !matchedKeys[key] {
			results = append(results, key+": removed")
		}
	}
	results = append(results, other...)
	if len(results) == 0 {
		return []string{"(diff available — see audit report)"}
	}
	return results
}

// quadletPrefix is the path prefix for quadlet unit files.
const quadletPrefix = "etc/containers/systemd/"
