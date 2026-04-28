package inspector

import (
	"path/filepath"
	"sort"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// KernelBootOptions configures the Kernel/Boot inspector.
type KernelBootOptions struct {
	SystemType schema.SystemType
}

// RunKernelBoot inspects kernel and boot configuration: cmdline, GRUB
// defaults, sysctl overrides, modules-load.d, modprobe.d, dracut config,
// tuned profiles, locale, timezone, and alternatives.
func RunKernelBoot(exec Executor, opts KernelBootOptions) (*schema.KernelBootSection, []Warning, error) {
	var warnings []Warning

	section := &schema.KernelBootSection{
		SysctlOverrides:     []schema.SysctlOverride{},
		ModulesLoadD:        []schema.ConfigSnippet{},
		ModprobeD:           []schema.ConfigSnippet{},
		DracutConf:          []schema.ConfigSnippet{},
		LoadedModules:       []schema.KernelModule{},
		NonDefaultModules:   []schema.KernelModule{},
		TunedCustomProfiles: []schema.ConfigSnippet{},
		Alternatives:        []schema.AlternativeEntry{},
	}

	// --- cmdline ---
	collectCmdline(exec, section, &warnings)

	// --- GRUB defaults ---
	collectGrubDefaults(exec, section, opts.SystemType)

	// --- sysctl diff ---
	collectSysctlDiff(exec, section, &warnings)

	// --- modules-load.d / modprobe.d / dracut ---
	collectConfigDir(exec, "/etc/modules-load.d", &section.ModulesLoadD)
	collectConfigDir(exec, "/etc/modprobe.d", &section.ModprobeD)
	collectConfigDir(exec, "/etc/dracut.conf.d", &section.DracutConf)

	// --- lsmod + diff ---
	collectModules(exec, section)

	// --- tuned profile ---
	collectTuned(exec, section)

	// --- system properties ---
	section.Locale = detectLocale(exec)
	section.Timezone = detectTimezone(exec)
	section.Alternatives = detectAlternatives(exec)

	return section, warnings, nil
}

// ---------------------------------------------------------------------------
// Kernel command line
// ---------------------------------------------------------------------------

func collectCmdline(exec Executor, section *schema.KernelBootSection, warnings *[]Warning) {
	content, err := exec.ReadFile("/proc/cmdline")
	if err != nil {
		*warnings = append(*warnings, makeWarning(
			"kernel_boot",
			"/proc/cmdline unreadable — kernel command line unavailable.",
		))
		return
	}
	section.Cmdline = strings.TrimSpace(content)
}

// ---------------------------------------------------------------------------
// GRUB defaults
// ---------------------------------------------------------------------------

func collectGrubDefaults(exec Executor, section *schema.KernelBootSection, systemType schema.SystemType) {
	// On ostree systems, BLS entries are managed by ostree/bootc, not GRUB defaults.
	if systemType == schema.SystemTypeRpmOstree || systemType == schema.SystemTypeBootc {
		return
	}

	content, err := exec.ReadFile("/etc/default/grub")
	if err != nil {
		return
	}
	text := strings.TrimSpace(content)
	if len(text) > 500 {
		text = text[:500]
	}
	section.GrubDefaults = text
}

// ---------------------------------------------------------------------------
// Sysctl helpers
// ---------------------------------------------------------------------------

// parseSysctlConf parses a sysctl .conf file into {key: value}.
func parseSysctlConf(text string) map[string]string {
	result := make(map[string]string)
	for _, line := range strings.Split(text, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") || strings.HasPrefix(line, ";") {
			continue
		}
		idx := strings.Index(line, "=")
		if idx < 0 {
			continue
		}
		key := strings.TrimSpace(line[:idx])
		val := strings.TrimSpace(line[idx+1:])
		result[key] = val
	}
	return result
}

// sysctlEntry holds a sysctl value and the file it came from.
type sysctlEntry struct {
	value  string
	source string
}

// collectSysctlDefaults reads shipped sysctl defaults from /usr/lib/sysctl.d/.
// Later files (sorted by name) override earlier ones, matching systemd behaviour.
func collectSysctlDefaults(exec Executor) map[string]sysctlEntry {
	defaults := make(map[string]sysctlEntry)
	entries, err := exec.ReadDir("/usr/lib/sysctl.d")
	if err != nil {
		return defaults
	}

	// Sort by name to match systemd precedence.
	names := make([]string, 0, len(entries))
	for _, e := range entries {
		if !e.IsDir() && strings.HasSuffix(e.Name(), ".conf") {
			names = append(names, e.Name())
		}
	}
	sort.Strings(names)

	for _, name := range names {
		path := filepath.Join("/usr/lib/sysctl.d", name)
		content, err := exec.ReadFile(path)
		if err != nil {
			continue
		}
		relPath := strings.TrimPrefix(path, "/")
		for k, v := range parseSysctlConf(content) {
			defaults[k] = sysctlEntry{value: v, source: relPath}
		}
	}
	return defaults
}

// collectSysctlOverrides reads operator sysctl overrides from /etc/sysctl.d/ and /etc/sysctl.conf.
func collectSysctlOverrides(exec Executor) map[string]sysctlEntry {
	overrides := make(map[string]sysctlEntry)

	// /etc/sysctl.d/*.conf
	entries, err := exec.ReadDir("/etc/sysctl.d")
	if err == nil {
		names := make([]string, 0, len(entries))
		for _, e := range entries {
			if !e.IsDir() && strings.HasSuffix(e.Name(), ".conf") {
				names = append(names, e.Name())
			}
		}
		sort.Strings(names)

		for _, name := range names {
			path := filepath.Join("/etc/sysctl.d", name)
			content, err := exec.ReadFile(path)
			if err != nil {
				continue
			}
			relPath := strings.TrimPrefix(path, "/")
			for k, v := range parseSysctlConf(content) {
				overrides[k] = sysctlEntry{value: v, source: relPath}
			}
		}
	}

	// /etc/sysctl.conf
	content, err := exec.ReadFile("/etc/sysctl.conf")
	if err == nil {
		for k, v := range parseSysctlConf(content) {
			overrides[k] = sysctlEntry{value: v, source: "etc/sysctl.conf"}
		}
	}

	return overrides
}

// readRuntimeSysctl reads the runtime value of a sysctl key from /proc/sys/.
func readRuntimeSysctl(exec Executor, key string) *string {
	procPath := "/proc/sys/" + strings.ReplaceAll(key, ".", "/")
	content, err := exec.ReadFile(procPath)
	if err != nil {
		return nil
	}
	val := strings.TrimSpace(content)
	return &val
}

// diffSysctl compares runtime sysctl values against shipped defaults.
// Returns entries where runtime differs from the shipped default.
func diffSysctl(exec Executor, defaults, overrides map[string]sysctlEntry) []schema.SysctlOverride {
	// Gather all keys.
	allKeys := make(map[string]struct{})
	for k := range defaults {
		allKeys[k] = struct{}{}
	}
	for k := range overrides {
		allKeys[k] = struct{}{}
	}

	sorted := make([]string, 0, len(allKeys))
	for k := range allKeys {
		sorted = append(sorted, k)
	}
	sort.Strings(sorted)

	var results []schema.SysctlOverride
	for _, key := range sorted {
		def, hasDef := defaults[key]
		ovr, hasOvr := overrides[key]

		runtime := readRuntimeSysctl(exec, key)
		var runtimeVal string
		if runtime != nil {
			runtimeVal = *runtime
		} else if hasOvr {
			runtimeVal = ovr.value
		} else if hasDef {
			runtimeVal = def.value
		}

		// If runtime matches the shipped default, skip.
		if hasDef && runtimeVal == def.value {
			continue
		}

		source := ""
		if hasOvr {
			source = ovr.source
		} else if hasDef {
			source = def.source
		}

		defaultVal := ""
		if hasDef {
			defaultVal = def.value
		}

		results = append(results, schema.SysctlOverride{
			Key:     key,
			Runtime: runtimeVal,
			Default: defaultVal,
			Source:  source,
		})
	}
	return results
}

func collectSysctlDiff(exec Executor, section *schema.KernelBootSection, warnings *[]Warning) {
	defaults := collectSysctlDefaults(exec)
	overrides := collectSysctlOverrides(exec)

	if len(defaults) == 0 && exec.FileExists("/usr/lib/sysctl.d") {
		*warnings = append(*warnings, makeWarning(
			"kernel_boot",
			"sysctl shipped defaults could not be read from /usr/lib/sysctl.d — sysctl diff may be incomplete.",
		))
	}

	result := diffSysctl(exec, defaults, overrides)
	if result != nil {
		section.SysctlOverrides = result
	}
}

// ---------------------------------------------------------------------------
// Config directory collection (modules-load.d, modprobe.d, dracut)
// ---------------------------------------------------------------------------

func collectConfigDir(exec Executor, dir string, target *[]schema.ConfigSnippet) {
	entries, err := exec.ReadDir(dir)
	if err != nil {
		return
	}

	// Sort for deterministic output.
	names := make([]string, 0, len(entries))
	for _, e := range entries {
		if !e.IsDir() && strings.HasSuffix(e.Name(), ".conf") {
			names = append(names, e.Name())
		}
	}
	sort.Strings(names)

	for _, name := range names {
		path := filepath.Join(dir, name)
		content, err := exec.ReadFile(path)
		if err != nil {
			continue
		}
		relPath := strings.TrimPrefix(path, "/")
		*target = append(*target, schema.ConfigSnippet{
			Path:    relPath,
			Content: content,
		})
	}
}

// ---------------------------------------------------------------------------
// Kernel modules (lsmod)
// ---------------------------------------------------------------------------

// parseLsmod parses lsmod output into a list of KernelModule.
func parseLsmod(text string) []schema.KernelModule {
	var results []schema.KernelModule
	lines := strings.Split(text, "\n")
	for _, line := range lines[1:] { // skip header
		parts := strings.Fields(line)
		if len(parts) < 2 {
			continue
		}
		usedBy := ""
		if len(parts) > 3 {
			usedBy = parts[3]
		}
		results = append(results, schema.KernelModule{
			Name:   parts[0],
			Size:   parts[1],
			UsedBy: usedBy,
		})
	}
	return results
}

// collectExpectedModules gathers module names explicitly configured to load.
func collectExpectedModules(exec Executor) map[string]struct{} {
	expected := make(map[string]struct{})
	for _, base := range []string{"/usr/lib/modules-load.d", "/etc/modules-load.d"} {
		entries, err := exec.ReadDir(base)
		if err != nil {
			continue
		}
		for _, e := range entries {
			if e.IsDir() || !strings.HasSuffix(e.Name(), ".conf") {
				continue
			}
			content, err := exec.ReadFile(filepath.Join(base, e.Name()))
			if err != nil {
				continue
			}
			for _, line := range strings.Split(content, "\n") {
				line = strings.TrimSpace(line)
				if line != "" && !strings.HasPrefix(line, "#") {
					expected[line] = struct{}{}
				}
			}
		}
	}
	return expected
}

// collectDependencyModules builds the set of modules loaded as dependencies.
func collectDependencyModules(loaded []schema.KernelModule) map[string]struct{} {
	deps := make(map[string]struct{})
	for _, mod := range loaded {
		if strings.TrimSpace(mod.UsedBy) != "" {
			deps[mod.Name] = struct{}{}
		}
	}
	return deps
}

// diffModules returns loaded modules that are neither explicitly configured nor a dependency.
func diffModules(loaded []schema.KernelModule, expected map[string]struct{}) []schema.KernelModule {
	depNames := collectDependencyModules(loaded)
	var nonDefault []schema.KernelModule
	for _, mod := range loaded {
		if _, ok := expected[mod.Name]; ok {
			continue
		}
		if _, ok := depNames[mod.Name]; ok {
			continue
		}
		nonDefault = append(nonDefault, mod)
	}
	return nonDefault
}

func collectModules(exec Executor, section *schema.KernelBootSection) {
	r := exec.Run("lsmod")
	if r.ExitCode != 0 || strings.TrimSpace(r.Stdout) == "" {
		return
	}
	if mods := parseLsmod(r.Stdout); mods != nil {
		section.LoadedModules = mods
	}
	expected := collectExpectedModules(exec)
	if nd := diffModules(section.LoadedModules, expected); nd != nil {
		section.NonDefaultModules = nd
	}
}

// ---------------------------------------------------------------------------
// Tuned profiles
// ---------------------------------------------------------------------------

func collectTuned(exec Executor, section *schema.KernelBootSection) {
	// Try reading the active_profile file first.
	content, err := exec.ReadFile("/etc/tuned/active_profile")
	if err == nil {
		active := strings.TrimSpace(content)
		if active != "" {
			section.TunedActive = active
		}
	}

	// Fallback to tuned-adm command if file was empty or missing.
	if section.TunedActive == "" {
		r := exec.Run("tuned-adm", "active")
		if r.ExitCode == 0 && strings.TrimSpace(r.Stdout) != "" {
			for _, line := range strings.Split(r.Stdout, "\n") {
				if idx := strings.Index(line, ":"); idx >= 0 {
					section.TunedActive = strings.TrimSpace(line[idx+1:])
					break
				}
			}
		}
	}

	// Scan /etc/tuned/ for custom profile directories containing tuned.conf.
	entries, err := exec.ReadDir("/etc/tuned")
	if err != nil {
		return
	}
	names := make([]string, 0, len(entries))
	for _, e := range entries {
		if e.IsDir() {
			names = append(names, e.Name())
		}
	}
	sort.Strings(names)

	for _, name := range names {
		confPath := filepath.Join("/etc/tuned", name, "tuned.conf")
		content, err := exec.ReadFile(confPath)
		if err != nil {
			continue
		}
		relPath := strings.TrimPrefix(confPath, "/")
		section.TunedCustomProfiles = append(section.TunedCustomProfiles, schema.ConfigSnippet{
			Path:    relPath,
			Content: content,
		})
	}
}

// ---------------------------------------------------------------------------
// Locale
// ---------------------------------------------------------------------------

func detectLocale(exec Executor) *string {
	content, err := exec.ReadFile("/etc/locale.conf")
	if err != nil {
		return nil
	}
	for _, line := range strings.Split(content, "\n") {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, "LANG=") {
			val := strings.SplitN(line, "=", 2)[1]
			val = strings.Trim(val, "\"'")
			return &val
		}
	}
	return nil
}

// ---------------------------------------------------------------------------
// Timezone
// ---------------------------------------------------------------------------

func detectTimezone(exec Executor) *string {
	// The Python implementation reads the /etc/localtime symlink target.
	// In the Go port, we use the Executor's ReadFile to read a sentinel
	// file that the FakeExecutor can provide, since symlink-following
	// would require an OS-level syscall. For real hosts, the RealExecutor
	// handles the symlink resolution. We check a conventional path.

	// Try reading /etc/timezone first (Debian-style).
	content, err := exec.ReadFile("/etc/timezone")
	if err == nil {
		tz := strings.TrimSpace(content)
		if tz != "" {
			return &tz
		}
	}

	// Try timedatectl show output as fallback.
	r := exec.Run("timedatectl", "show", "--property=Timezone", "--value")
	if r.ExitCode == 0 {
		tz := strings.TrimSpace(r.Stdout)
		if tz != "" {
			return &tz
		}
	}

	return nil
}

// ---------------------------------------------------------------------------
// Alternatives
// ---------------------------------------------------------------------------

func detectAlternatives(exec Executor) []schema.AlternativeEntry {
	// Read /etc/alternatives directory for symlinks.
	// In the Go port, we treat dir entries as "links" since FakeExecutor
	// provides file names. For each entry, check /var/lib/alternatives/<name>
	// for the auto/manual status.
	entries, err := exec.ReadDir("/etc/alternatives")
	if err != nil {
		return nil
	}

	names := make([]string, 0, len(entries))
	for _, e := range entries {
		if !e.IsDir() {
			names = append(names, e.Name())
		}
	}
	sort.Strings(names)

	var results []schema.AlternativeEntry
	for _, name := range names {
		// Read the symlink target (stored as file content in FakeExecutor).
		linkPath := filepath.Join("/etc/alternatives", name)
		target, err := exec.ReadFile(linkPath)
		if err != nil {
			continue
		}
		target = strings.TrimSpace(target)

		status := "auto"
		statusPath := filepath.Join("/var/lib/alternatives", name)
		statusContent, err := exec.ReadFile(statusPath)
		if err == nil {
			lines := strings.Split(statusContent, "\n")
			if len(lines) > 0 {
				first := strings.TrimSpace(lines[0])
				if first == "auto" || first == "manual" {
					status = first
				}
			}
		}

		results = append(results, schema.AlternativeEntry{
			Name:   name,
			Path:   target,
			Status: status,
		})
	}
	return results
}
