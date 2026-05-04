// SELinux/Security inspector: mode, custom modules, boolean overrides,
// fcontext rules, port labels, audit rules, FIPS mode, PAM configs.
//
// Combines executor-based commands (getenforce, semanage) with filesystem
// reads for fallback paths and file scanning.
package inspector

import (
	"path/filepath"
	"regexp"
	"sort"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

// SelinuxOptions configures the SELinux inspector.
type SelinuxOptions struct {
	// RpmOwnedPaths is the set of paths owned by RPM packages. When
	// non-nil, audit rule files and PAM configs owned by RPM are skipped.
	RpmOwnedPaths map[string]bool
}

// RunSelinux runs the SELinux/security inspector and returns the populated
// section, accumulated warnings, and any fatal error.
func RunSelinux(exec Executor, opts SelinuxOptions) (*schema.SelinuxSection, []Warning, error) {
	var warnings []Warning
	section := &schema.SelinuxSection{
		CustomModules:    []string{},
		BooleanOverrides: []map[string]interface{}{},
		FcontextRules:    []string{},
		AuditRules:       []string{},
		PamConfigs:       []string{},
		PortLabels:       []schema.SelinuxPortLabel{},
	}

	collectSELinuxMode(exec, section)
	ptype := readPolicyType(exec)
	collectCustomModules(exec, section, ptype)
	collectBooleanOverrides(exec, section, &warnings)
	collectFcontextRules(exec, section, ptype)
	collectPortLabels(exec, section)
	collectAuditRules(exec, section, opts.RpmOwnedPaths)
	collectFIPSMode(exec, section)
	collectPAMConfigs(exec, section, opts.RpmOwnedPaths)

	return section, warnings, nil
}

// ---------------------------------------------------------------------------
// SELinux mode
// ---------------------------------------------------------------------------

// collectSELinuxMode reads the SELinux mode from /etc/selinux/config.
func collectSELinuxMode(exec Executor, section *schema.SelinuxSection) {
	content, err := exec.ReadFile("/etc/selinux/config")
	if err != nil {
		return
	}
	for _, line := range strings.Split(content, "\n") {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, "SELINUX=") {
			section.Mode = strings.TrimPrefix(line, "SELINUX=")
			return
		}
	}
}

// ---------------------------------------------------------------------------
// Policy type
// ---------------------------------------------------------------------------

// readPolicyType reads SELINUXTYPE from /etc/selinux/config, defaulting
// to "targeted".
func readPolicyType(exec Executor) string {
	content, err := exec.ReadFile("/etc/selinux/config")
	if err != nil {
		return "targeted"
	}
	for _, line := range strings.Split(content, "\n") {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, "SELINUXTYPE=") {
			val := strings.TrimPrefix(line, "SELINUXTYPE=")
			if val != "" {
				return val
			}
		}
	}
	return "targeted"
}

// ---------------------------------------------------------------------------
// Custom modules (priority-400 store)
// ---------------------------------------------------------------------------

// collectCustomModules discovers custom SELinux modules from the
// priority-400 module store. Modules at priority 400 were installed
// locally via "semodule -i". Purely filesystem-based.
func collectCustomModules(exec Executor, section *schema.SelinuxSection, policyType string) {
	storePath := filepath.Join("/etc/selinux", policyType, "active/modules/400")
	entries, err := exec.ReadDir(storePath)
	if err != nil {
		return
	}

	var names []string
	for _, e := range entries {
		if e.IsDir() {
			names = append(names, e.Name())
		}
	}
	sort.Strings(names)
	section.CustomModules = names
}

// ---------------------------------------------------------------------------
// Boolean overrides
// ---------------------------------------------------------------------------

// boolRE matches semanage boolean -l output lines:
//   name  (current , default)  description
var boolRE = regexp.MustCompile(`^(\S+)\s+\((\w+)\s*,\s*(\w+)\)\s+(.*)`)

// parseSemanageBooleans parses "semanage boolean -l" output and returns
// booleans where current state differs from default.
func parseSemanageBooleans(text string) []map[string]interface{} {
	var results []map[string]interface{}
	for _, line := range strings.Split(text, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "SELinux boolean") {
			continue
		}
		m := boolRE.FindStringSubmatch(line)
		if m == nil {
			continue
		}
		name, current, defaultVal, desc := m[1], m[2], m[3], strings.TrimSpace(m[4])
		if current != defaultVal {
			results = append(results, map[string]interface{}{
				"name":        name,
				"current":     current,
				"default":     defaultVal,
				"non_default": true,
				"description": desc,
			})
		}
	}
	return results
}

// readBoolsFromFS is a fallback that reads boolean runtime values from
// /sys/fs/selinux/booleans/. Returns only booleans whose current value
// differs from the pending (policy-loaded) value.
func readBoolsFromFS(exec Executor) []map[string]interface{} {
	boolDir := "/sys/fs/selinux/booleans"
	entries, err := exec.ReadDir(boolDir)
	if err != nil {
		return nil
	}

	var results []map[string]interface{}
	for _, e := range entries {
		if e.IsDir() {
			continue
		}
		content, err := exec.ReadFile(filepath.Join(boolDir, e.Name()))
		if err != nil {
			continue
		}
		parts := strings.Fields(strings.TrimSpace(content))
		if len(parts) < 2 {
			continue
		}
		current := "off"
		if parts[0] == "1" {
			current = "on"
		}
		pending := "off"
		if parts[1] == "1" {
			pending = "on"
		}
		if current != pending {
			results = append(results, map[string]interface{}{
				"name":        e.Name(),
				"current":     current,
				"default":     pending,
				"non_default": true,
				"description": "",
			})
		}
	}
	return results
}

// collectBooleanOverrides tries semanage boolean -l via chroot, then
// falls back to reading /sys/fs/selinux/booleans/.
func collectBooleanOverrides(exec Executor, section *schema.SelinuxSection, warnings *[]Warning) {
	hostRoot := exec.HostRoot()
	res := exec.Run("chroot", hostRoot, "semanage", "boolean", "-l")
	if res.ExitCode == 0 && strings.TrimSpace(res.Stdout) != "" {
		section.BooleanOverrides = parseSemanageBooleans(res.Stdout)
		return
	}

	// Fallback to filesystem
	fallback := readBoolsFromFS(exec)
	if fallback != nil {
		section.BooleanOverrides = fallback
		return
	}

	// Neither method worked — check if the bool dir exists
	if !exec.FileExists("/sys/fs/selinux/booleans") {
		*warnings = append(*warnings, makeWarning(
			"selinux",
			"SELinux boolean override detection unavailable — semanage failed and /sys/fs/selinux/booleans not accessible.",
		))
	}
}

// ---------------------------------------------------------------------------
// Custom fcontext rules
// ---------------------------------------------------------------------------

// collectFcontextRules tries "semanage fcontext -l -C" via chroot, then
// falls back to reading file_contexts.local from the policy store.
func collectFcontextRules(exec Executor, section *schema.SelinuxSection, policyType string) {
	hostRoot := exec.HostRoot()
	res := exec.Run("chroot", hostRoot, "semanage", "fcontext", "-l", "-C")
	if res.ExitCode == 0 && strings.TrimSpace(res.Stdout) != "" {
		for _, line := range strings.Split(strings.TrimSpace(res.Stdout), "\n") {
			line = strings.TrimSpace(line)
			if line != "" && !strings.HasPrefix(line, "SELinux") {
				section.FcontextRules = append(section.FcontextRules, line)
			}
		}
		if len(section.FcontextRules) > 0 {
			return
		}
	}

	// Fallback: read file_contexts.local
	fcLocal := filepath.Join("/etc/selinux", policyType, "contexts/files/file_contexts.local")
	content, err := exec.ReadFile(fcLocal)
	if err != nil {
		return
	}
	for _, line := range strings.Split(content, "\n") {
		line = strings.TrimSpace(line)
		if line != "" && !strings.HasPrefix(line, "#") {
			section.FcontextRules = append(section.FcontextRules, line)
		}
	}
}

// ---------------------------------------------------------------------------
// Custom port labels
// ---------------------------------------------------------------------------

// portRE matches semanage port -l -C output lines:
//   type  protocol  port[,port...]
var portRE = regexp.MustCompile(`(?i)^(\S+)\s+(tcp|udp)\s+([\d,\-\s]+)`)

// parseSemanagePorts parses "semanage port -l -C" output into port labels.
func parseSemanagePorts(text string) []schema.SelinuxPortLabel {
	var results []schema.SelinuxPortLabel
	for _, line := range strings.Split(text, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "SELinux") {
			continue
		}
		m := portRE.FindStringSubmatch(line)
		if m == nil {
			continue
		}
		portType := m[1]
		protocol := strings.ToLower(m[2])
		portsRaw := strings.TrimSpace(m[3])

		// Emit one entry per port/range in comma-separated list
		for _, port := range strings.Split(portsRaw, ",") {
			port = strings.TrimSpace(port)
			if port != "" {
				results = append(results, schema.SelinuxPortLabel{
					Protocol: protocol,
					Port:     port,
					Type:     portType,
					Include:  true,
				})
			}
		}
	}
	return results
}

// collectPortLabels runs semanage port -l -C via chroot.
func collectPortLabels(exec Executor, section *schema.SelinuxSection) {
	hostRoot := exec.HostRoot()
	res := exec.Run("chroot", hostRoot, "semanage", "port", "-l", "-C")
	if res.ExitCode == 0 && strings.TrimSpace(res.Stdout) != "" {
		section.PortLabels = parseSemanagePorts(res.Stdout)
	}
}

// ---------------------------------------------------------------------------
// Audit rules
// ---------------------------------------------------------------------------

// collectAuditRules scans /etc/audit/rules.d/ for custom audit rule files,
// skipping files owned by RPM.
func collectAuditRules(exec Executor, section *schema.SelinuxSection, rpmOwned map[string]bool) {
	auditDir := "/etc/audit/rules.d"
	entries, err := exec.ReadDir(auditDir)
	if err != nil {
		return
	}

	var rules []string
	for _, e := range entries {
		if e.IsDir() {
			continue
		}
		rel := filepath.Join("etc/audit/rules.d", e.Name())
		absPath := "/" + rel
		if rpmOwned != nil && rpmOwned[absPath] {
			continue
		}
		rules = append(rules, rel)
	}
	sort.Strings(rules)
	section.AuditRules = rules
}

// ---------------------------------------------------------------------------
// FIPS mode
// ---------------------------------------------------------------------------

// collectFIPSMode reads /proc/sys/crypto/fips_enabled.
func collectFIPSMode(exec Executor, section *schema.SelinuxSection) {
	content, err := exec.ReadFile("/proc/sys/crypto/fips_enabled")
	if err != nil {
		return
	}
	section.FipsMode = strings.TrimSpace(content) == "1"
}

// ---------------------------------------------------------------------------
// PAM configs
// ---------------------------------------------------------------------------

// collectPAMConfigs scans /etc/pam.d/ for custom PAM configuration files,
// skipping files owned by RPM and system-generated exclusions.
func collectPAMConfigs(exec Executor, section *schema.SelinuxSection, rpmOwned map[string]bool) {
	pamDir := "/etc/pam.d"
	entries, err := exec.ReadDir(pamDir)
	if err != nil {
		return
	}

	var configs []string
	for _, e := range entries {
		if e.IsDir() {
			continue
		}
		rel := filepath.Join("etc/pam.d", e.Name())
		absPath := "/" + rel
		if rpmOwned != nil && rpmOwned[absPath] {
			continue
		}
		if isExcludedUnowned(absPath) {
			continue
		}
		configs = append(configs, rel)
	}
	sort.Strings(configs)
	section.PamConfigs = configs
}
