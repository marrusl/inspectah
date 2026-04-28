// Services inspector: systemd unit state vs preset baseline.
//
// Parses systemctl output (or falls back to filesystem scan) to determine
// current unit states, compares against preset defaults, detects drop-in
// overrides, and resolves owning packages for changed units.
package inspector

import (
	"path/filepath"
	"sort"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

// ServiceOptions configures the service inspector.
type ServiceOptions struct {
	// BaseImagePresetText is raw preset text from the base image.
	// Empty string means no baseline — fall back to host presets.
	BaseImagePresetText string

	// SystemType is the detected system type.
	SystemType schema.SystemType
}

// RunServices runs the service inspector and returns the populated section,
// accumulated warnings, and any fatal error.
func RunServices(exec Executor, opts ServiceOptions) (*schema.ServiceSection, []Warning, error) {
	var warnings []Warning
	section := &schema.ServiceSection{
		StateChanges:  []schema.ServiceStateChange{},
		EnabledUnits:  []string{},
		DisabledUnits: []string{},
		DropIns:       []schema.SystemdDropIn{},
	}

	// Collect current unit states.
	current := collectUnitStates(exec)
	if len(current) == 0 {
		return section, warnings, nil
	}

	// Parse preset defaults.
	presetEnabled, presetDisabled, hasDisableAll, globRules := parsePresets(exec, opts.BaseImagePresetText)

	if opts.BaseImagePresetText == "" {
		warnings = append(warnings, makeWarning(
			"service",
			"No base image service presets available — service state changes are "+
				"reported without comparison to base image defaults. "+
				"All non-default-enabled units will appear as changes.",
		))
	}

	// Compare current states to preset defaults.
	units := svcSortedKeys(current)
	for _, unit := range units {
		state := current[unit]
		if !strings.HasSuffix(unit, ".service") && !strings.HasSuffix(unit, ".timer") {
			continue
		}

		defaultState := resolveDefaultState(unit, presetEnabled, presetDisabled, hasDisableAll, globRules)

		action := "unchanged"
		if state == "enabled" && defaultState != "enabled" {
			action = "enable"
			section.EnabledUnits = append(section.EnabledUnits, unit)
		} else if state == "disabled" && defaultState == "enabled" {
			action = "disable"
			section.DisabledUnits = append(section.DisabledUnits, unit)
		} else if state == "masked" {
			action = "mask"
		}

		section.StateChanges = append(section.StateChanges, schema.ServiceStateChange{
			Unit:         unit,
			CurrentState: state,
			DefaultState: defaultState,
			Action:       action,
		})
	}

	// Resolve owning packages for changed units.
	resolveOwningPackages(exec, section)

	// Scan for drop-in overrides.
	scanDropIns(exec, section)

	return section, warnings, nil
}

// ---------------------------------------------------------------------------
// systemctl output parsing
// ---------------------------------------------------------------------------

// collectUnitStates runs systemctl list-unit-files and parses the output.
// Falls back to filesystem scan if systemctl fails.
func collectUnitStates(exec Executor) map[string]string {
	hostRoot := exec.HostRoot()

	var args []string
	if hostRoot != "/" {
		args = []string{"--root", hostRoot, "list-unit-files", "--no-pager", "--no-legend"}
	} else {
		args = []string{"list-unit-files", "--no-pager", "--no-legend"}
	}

	result := exec.Run("systemctl", args...)
	if result.ExitCode == 0 && strings.TrimSpace(result.Stdout) != "" {
		return parseSystemctlListUnitFiles(result.Stdout)
	}

	// Fallback to filesystem scan.
	return scanUnitFilesFromFS(exec)
}

// parseSystemctlListUnitFiles parses output of systemctl list-unit-files.
// Each line: UNIT STATE [PRESET]
func parseSystemctlListUnitFiles(stdout string) map[string]string {
	units := make(map[string]string)
	for _, line := range strings.Split(strings.TrimSpace(stdout), "\n") {
		parts := strings.Fields(line)
		if len(parts) >= 2 {
			units[parts[0]] = parts[1]
		}
	}
	return units
}

// scanUnitFilesFromFS determines unit states by scanning the filesystem.
func scanUnitFilesFromFS(exec Executor) map[string]string {
	units := make(map[string]string)
	enabledUnits := make(map[string]bool)
	maskedUnits := make(map[string]bool)

	adminDir := "/etc/systemd/system"

	// Scan .wants/ directories for enabled units.
	entries, err := exec.ReadDir(adminDir)
	if err == nil {
		for _, entry := range entries {
			name := entry.Name()
			if strings.HasSuffix(name, ".wants") {
				// Try reading the path as a directory — works whether
				// the DirEntry reports IsDir or not (FakeExecutor always
				// returns non-dir entries).
				wantsPath := filepath.Join(adminDir, name)
				wantEntries, wErr := exec.ReadDir(wantsPath)
				if wErr == nil {
					for _, we := range wantEntries {
						enabledUnits[we.Name()] = true
					}
				}
			}
		}
	}

	// Check for masked units (symlinks to /dev/null are files in admin dir
	// that contain nothing; in our abstraction we just check existence).
	if err == nil {
		for _, entry := range entries {
			name := entry.Name()
			if (strings.HasSuffix(name, ".service") || strings.HasSuffix(name, ".timer")) {
				// Read the file; if it's empty, treat as masked.
				content, readErr := exec.ReadFile(filepath.Join(adminDir, name))
				if readErr == nil && content == "" {
					maskedUnits[name] = true
				}
			}
		}
	}

	// Collect vendor unit files.
	vendorDir := "/usr/lib/systemd/system"
	vendorUnits := make(map[string]bool)
	vendorEntries, err := exec.ReadDir(vendorDir)
	if err == nil {
		for _, entry := range vendorEntries {
			name := entry.Name()
			if (strings.HasSuffix(name, ".service") || strings.HasSuffix(name, ".timer")) && !entry.IsDir() {
				vendorUnits[name] = true
			}
		}
	}

	// Merge all known units and determine states.
	allKnown := make(map[string]bool)
	for u := range vendorUnits {
		allKnown[u] = true
	}
	for u := range enabledUnits {
		allKnown[u] = true
	}
	for u := range maskedUnits {
		allKnown[u] = true
	}

	for unit := range allKnown {
		if !strings.HasSuffix(unit, ".service") && !strings.HasSuffix(unit, ".timer") {
			continue
		}
		if maskedUnits[unit] {
			units[unit] = "masked"
		} else if enabledUnits[unit] {
			units[unit] = "enabled"
		} else {
			// Check for [Install] section to distinguish static vs disabled.
			unitPath := filepath.Join(vendorDir, unit)
			content, err := exec.ReadFile(unitPath)
			if err == nil && strings.Contains(content, "[Install]") {
				units[unit] = "disabled"
			} else {
				units[unit] = "static"
			}
		}
	}

	return units
}

// ---------------------------------------------------------------------------
// Preset parsing
// ---------------------------------------------------------------------------

// presetGlobRule is an ordered preset rule with a glob pattern.
type presetGlobRule struct {
	action  string // "enable" or "disable"
	pattern string
}

// parsePresets loads preset defaults, either from base image text or host files.
func parsePresets(exec Executor, baseImagePresetText string) (enabled, disabled map[string]bool, hasDisableAll bool, globs []presetGlobRule) {
	if baseImagePresetText != "" {
		return parsePresetLines(strings.Split(baseImagePresetText, "\n"))
	}
	return parsePresetFilesFromHost(exec)
}

// parsePresetLines parses preset content lines into enabled/disabled sets
// and glob rules with first-match-wins semantics per systemd-preset(5).
func parsePresetLines(lines []string) (enabled, disabled map[string]bool, hasDisableAll bool, globs []presetGlobRule) {
	enabled = make(map[string]bool)
	disabled = make(map[string]bool)
	alreadyMatched := make(map[string]bool)

	for _, raw := range lines {
		line := strings.TrimSpace(raw)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		parts := strings.Fields(line)
		if len(parts) < 2 {
			continue
		}
		action := strings.ToLower(parts[0])
		pattern := parts[1]

		if strings.ContainsAny(pattern, "*?") {
			if pattern == "*" && action == "disable" {
				hasDisableAll = true
			}
			globs = append(globs, presetGlobRule{action: action, pattern: pattern})
			continue
		}

		if alreadyMatched[pattern] {
			continue
		}
		alreadyMatched[pattern] = true

		switch action {
		case "enable":
			enabled[pattern] = true
		case "disable":
			disabled[pattern] = true
		}
	}

	return enabled, disabled, hasDisableAll, globs
}

// parsePresetFilesFromHost reads preset files from the host filesystem.
func parsePresetFilesFromHost(exec Executor) (enabled, disabled map[string]bool, hasDisableAll bool, globs []presetGlobRule) {
	presetDirs := []string{
		"/etc/systemd/system-preset",
		"/usr/lib/systemd/system-preset",
	}

	var allLines []string
	for _, dir := range presetDirs {
		entries, err := exec.ReadDir(dir)
		if err != nil {
			continue
		}
		// Sort entries for deterministic ordering.
		names := make([]string, 0, len(entries))
		for _, e := range entries {
			names = append(names, e.Name())
		}
		sort.Strings(names)

		for _, name := range names {
			if !strings.HasSuffix(name, ".preset") {
				continue
			}
			content, err := exec.ReadFile(filepath.Join(dir, name))
			if err != nil {
				continue
			}
			allLines = append(allLines, strings.Split(content, "\n")...)
		}
	}

	return parsePresetLines(allLines)
}

// resolveDefaultState determines the preset default state for a unit.
func resolveDefaultState(unit string, presetEnabled, presetDisabled map[string]bool, hasDisableAll bool, globs []presetGlobRule) string {
	if presetEnabled[unit] {
		return "enabled"
	}
	if presetDisabled[unit] {
		return "disabled"
	}

	// First-match-wins over glob rules (systemd-preset(5) semantics).
	for _, rule := range globs {
		if globMatch(rule.pattern, unit) {
			if rule.action == "enable" {
				return "enabled"
			}
			return "disabled"
		}
	}

	if hasDisableAll {
		return "disabled"
	}
	return "unknown"
}

// globMatch implements simple glob matching with * and ? wildcards,
// matching systemd-preset(5) / fnmatch behaviour.
func globMatch(pattern, name string) bool {
	return matchGlob(pattern, name)
}

// matchGlob performs recursive glob matching.
func matchGlob(pattern, name string) bool {
	for len(pattern) > 0 {
		switch pattern[0] {
		case '*':
			// Skip consecutive stars.
			for len(pattern) > 0 && pattern[0] == '*' {
				pattern = pattern[1:]
			}
			if len(pattern) == 0 {
				return true
			}
			for i := 0; i <= len(name); i++ {
				if matchGlob(pattern, name[i:]) {
					return true
				}
			}
			return false
		case '?':
			if len(name) == 0 {
				return false
			}
			pattern = pattern[1:]
			name = name[1:]
		default:
			if len(name) == 0 || pattern[0] != name[0] {
				return false
			}
			pattern = pattern[1:]
			name = name[1:]
		}
	}
	return len(name) == 0
}

// ---------------------------------------------------------------------------
// Owning package resolution
// ---------------------------------------------------------------------------

// resolveOwningPackages populates OwningPackage for non-unchanged state changes
// via rpm -qf, batching paths to minimize subprocess calls.
func resolveOwningPackages(exec Executor, section *schema.ServiceSection) {
	var changed []int
	for i, sc := range section.StateChanges {
		if sc.Action != "unchanged" {
			changed = append(changed, i)
		}
	}
	if len(changed) == 0 {
		return
	}

	// Try vendor paths first (/usr/lib/systemd/system/).
	paths := make([]string, len(changed))
	for i, idx := range changed {
		paths[i] = "/usr/lib/systemd/system/" + section.StateChanges[idx].Unit
	}

	args := append([]string{"-qf", "--queryformat", "%{NAME}\n"}, paths...)
	result := exec.Run("rpm", args...)
	if result.ExitCode == 0 && strings.TrimSpace(result.Stdout) != "" {
		names := strings.Split(strings.TrimSpace(result.Stdout), "\n")
		if len(names) == len(changed) {
			for i, idx := range changed {
				pkg := strings.TrimSpace(names[i])
				if pkg != "" && !strings.Contains(pkg, "not owned") {
					section.StateChanges[idx].OwningPackage = &pkg
				}
			}
			return
		}
	}

	// Batch failed — fall back to individual queries.
	for _, idx := range changed {
		unit := section.StateChanges[idx].Unit
		for _, prefix := range []string{"/usr/lib/systemd/system/", "/etc/systemd/system/"} {
			r := exec.Run("rpm", "-qf", "--queryformat", "%{NAME}\n", prefix+unit)
			if r.ExitCode == 0 && strings.TrimSpace(r.Stdout) != "" {
				pkg := strings.TrimSpace(strings.Split(r.Stdout, "\n")[0])
				if pkg != "" && !strings.Contains(pkg, "not owned") {
					section.StateChanges[idx].OwningPackage = &pkg
					break
				}
			}
		}
	}
}

// ---------------------------------------------------------------------------
// Drop-in override scanning
// ---------------------------------------------------------------------------

// dropInSuffixes lists the unit type suffixes we scan for drop-in dirs.
var dropInSuffixes = []string{".service.d", ".timer.d", ".socket.d"}

// scanDropIns scans /etc/systemd/system/ for drop-in override directories
// and captures their .conf files.
func scanDropIns(exec Executor, section *schema.ServiceSection) {
	adminDir := "/etc/systemd/system"
	entries, err := exec.ReadDir(adminDir)
	if err != nil {
		return
	}

	// Sort for deterministic output.
	names := make([]string, 0, len(entries))
	for _, e := range entries {
		names = append(names, e.Name())
	}
	sort.Strings(names)

	for _, name := range names {
		isDropIn := false
		for _, suffix := range dropInSuffixes {
			if strings.HasSuffix(name, suffix) {
				isDropIn = true
				break
			}
		}
		if !isDropIn {
			continue
		}

		// Unit name is the dir name minus trailing ".d".
		unitName := name[:len(name)-2]

		dropInPath := filepath.Join(adminDir, name)
		confEntries, err := exec.ReadDir(dropInPath)
		if err != nil {
			continue
		}

		confNames := make([]string, 0, len(confEntries))
		for _, ce := range confEntries {
			confNames = append(confNames, ce.Name())
		}
		sort.Strings(confNames)

		for _, confName := range confNames {
			if !strings.HasSuffix(confName, ".conf") {
				continue
			}
			confPath := filepath.Join(dropInPath, confName)
			content, err := exec.ReadFile(confPath)
			if err != nil {
				content = ""
			}

			// Store path relative to host root (strip leading /).
			relPath := strings.TrimPrefix(confPath, "/")

			section.DropIns = append(section.DropIns, schema.SystemdDropIn{
				Unit:    unitName,
				Path:    relPath,
				Content: content,
			})
		}
	}
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// svcSortedKeys returns the keys of a map in sorted order.
func svcSortedKeys(m map[string]string) []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}
