// Config inspector: RPM-owned modified, unowned /etc files, orphaned configs.
//
// For package-mode systems: uses rpm -Va output (from RPM inspector) to find
// modified files, builds the full RPM-owned path set to find unowned files,
// and cross-references dnf history removed packages for orphan detection.
//
// For ostree/bootc systems: diffs /usr/etc against /etc overlays.
//
// Optional --config-diffs mode produces unified diffs against RPM originals.
package inspector

import (
	"fmt"
	"path/filepath"
	"regexp"
	"sort"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// ---------------------------------------------------------------------------
// Dev-artifact filtering (shared with other inspectors)
// ---------------------------------------------------------------------------

// pruneMarkers are VCS directory names whose presence causes the entire
// subtree to be skipped.
var pruneMarkers = map[string]bool{
	".git": true, ".svn": true, ".hg": true,
}

// skipDirNames are directory names always skipped during recursive walks.
var skipDirNames = map[string]bool{
	"__pycache__": true, ".mypy_cache": true, ".pytest_cache": true,
	".tox": true, ".nox": true,
	"node_modules": true, ".eggs": true,
	".vscode": true, ".idea": true, ".cursor": true,
}

// IsDevArtifact returns true if any path component is a dev/build directory.
// hostRoot, if non-empty, scopes the check to the relative portion of the
// path (prevents false positives from workspace mount paths).
func IsDevArtifact(path, hostRoot string) bool {
	rel := path
	if hostRoot != "" && hostRoot != "/" {
		trimmed := strings.TrimPrefix(path, hostRoot)
		if trimmed != path {
			rel = trimmed
		}
	}
	for _, part := range strings.Split(filepath.ToSlash(rel), "/") {
		if pruneMarkers[part] || skipDirNames[part] {
			return true
		}
	}
	return false
}

// ---------------------------------------------------------------------------
// System-generated exclusion lists
// ---------------------------------------------------------------------------

// unownedExcludeExact contains paths that are system-generated and should
// not appear as "unowned" config files.
var unownedExcludeExact = map[string]bool{
	// Machine identity
	"/etc/machine-id":  true,
	"/etc/adjtime":     true,
	"/etc/hostname":    true,
	"/etc/localtime":   true,

	// useradd/groupadd backups
	"/etc/.pwd.lock": true,
	"/etc/passwd-":   true,
	"/etc/shadow-":   true,
	"/etc/group-":    true,
	"/etc/gshadow-":  true,
	"/etc/subuid-":   true,
	"/etc/subgid-":   true,

	// systemd runtime state
	"/etc/.updated":    true,
	"/etc/machine-info": true,

	// standard systemd unit symlinks
	"/etc/systemd/system/default.target": true,
	"/etc/systemd/system/dbus.service":   true,
	"/etc/systemd/user/dbus.service":     true,

	// Network / DNS
	"/etc/resolv.conf":                            true,
	"/etc/NetworkManager/NetworkManager-intern.conf": true,

	// ld.so / system library state
	"/etc/ld.so.cache": true,
	"/etc/ld.so.conf":  true,
	"/etc/mtab":        true,
	"/etc/rpc":         true,

	// Package manager state
	"/etc/dnf/dnf.conf": true,
	"/etc/yum.conf":     true,
	"/etc/npmrc":        true,

	// Anaconda / installer artifacts
	"/etc/sysconfig/anaconda":          true,
	"/etc/sysconfig/kernel":            true,
	"/etc/sysconfig/network":           true,
	"/etc/sysconfig/selinux":           true,
	"/etc/sysconfig/network-scripts/readme-ifcfg-rh.txt": true,

	// Bootloader / kernel
	"/etc/kernel/cmdline": true,

	// systemd standard targets
	"/etc/systemd/system/ctrl-alt-del.target": true,

	// NVMe host identity
	"/etc/nvme/hostnqn": true,
	"/etc/nvme/hostid":  true,

	// Subscription manager / RHSM
	"/etc/rhsm/syspurpose/syspurpose.json": true,

	// OpenSSL configs (not RPM-owned on RHEL 10)
	"/etc/pki/tls/ct_log_list.cnf": true,
	"/etc/pki/tls/fips_local.cnf":  true,
	"/etc/pki/tls/openssl.cnf":     true,

	// SELinux policy store
	"/etc/selinux/targeted/setrans.conf":       true,
	"/etc/selinux/targeted/seusers":            true,
	"/etc/selinux/targeted/.policy.sha512":     true,
	"/etc/selinux/targeted/booleans.subs_dist": true,

	// udisks2
	"/etc/udisks2/udisks2.conf":              true,
	"/etc/udisks2/mount_options.conf.example": true,

	// PAM base configs
	"/etc/pam.d/chfn":     true,
	"/etc/pam.d/chsh":     true,
	"/etc/pam.d/login":    true,
	"/etc/pam.d/remote":   true,
	"/etc/pam.d/runuser":  true,
	"/etc/pam.d/runuser-l": true,
	"/etc/pam.d/su":       true,
	"/etc/pam.d/su-l":     true,

	// tuned runtime state
	"/etc/tuned/active_profile": true,
	"/etc/tuned/profile_mode":  true,
	"/etc/tuned/bootcmdline":   true,
}

// unownedExcludeGlobs are fnmatch-style patterns for system-generated files.
var unownedExcludeGlobs = []string{
	"/etc/pki/product-default/*.pem",
	"/etc/ssh/ssh_host_*",
	"/etc/alternatives/*",
	"/etc/X11/fontpath.d/*",
	"/etc/selinux/*/policy/policy.*",
	"/etc/selinux/*/contexts/*",
	"/etc/selinux/*/contexts/files/*",
	"/etc/selinux/*/contexts/users/*",
	"/etc/udev/hwdb.bin",
	"/etc/pki/ca-trust/extracted/*",
	"/etc/crypto-policies/back-ends/*",
	"/etc/pki/java/cacerts",
	"/etc/pki/tls/cert.pem",
	"/etc/pki/tls/certs/ca-bundle.crt",
	"/etc/pki/tls/certs/ca-bundle.trust.crt",
	"/etc/pki/consumer/*",
	"/etc/pki/entitlement/*",
	"/etc/depmod.d/*-dist.conf",
	"/etc/modprobe.d/*-blacklist.conf",
	"/etc/dconf/db/distro.d/*",
	"/etc/dconf/db/distro.d/locks/*",
	"/etc/dnf/protected.d/*",
	"/etc/profile.d/gnupg2.*",
	"/etc/logrotate.d/kvm_stat",
	"/etc/systemd/system/*.wants/*",
	"/etc/systemd/system/*.requires/*",
	"/etc/systemd/user/*.wants/*",
	"/etc/systemd/user/*.requires/*",
	"/etc/systemd/system/*.service.d/*.conf",
	"/etc/systemd/system/*.timer.d/*.conf",
	"/etc/systemd/system/*.socket.d/*.conf",
	"/etc/tuned/*/tuned.conf",
	"/etc/systemd/sleep.conf.d/*",
	"/etc/lvm/archive/*",
	"/etc/lvm/backup/*",
	"/etc/lvm/devices/*",
	"/etc/firewalld/zones/*.xml.old",
	"/etc/firewalld/*.xml.old",
	"/etc/NetworkManager/system-connections/*.nmconnection.bak",
	"/etc/sysconfig/network-scripts/readme-*",
	"/etc/pm/sleep.d/*",
}

// isExcludedUnowned returns true if path matches the system-generated
// exclusion list (exact match or glob).
func isExcludedUnowned(path string) bool {
	if unownedExcludeExact[path] {
		return true
	}
	for _, pattern := range unownedExcludeGlobs {
		if matched, _ := filepath.Match(pattern, path); matched {
			return true
		}
	}
	return false
}

// ---------------------------------------------------------------------------
// Category classification
// ---------------------------------------------------------------------------

type categoryRule struct {
	category schema.ConfigCategory
	prefixes []string
}

var categoryRules = []categoryRule{
	{schema.ConfigCategoryTmpfiles, []string{"/etc/tmpfiles.d/"}},
	{schema.ConfigCategoryEnvironment, []string{"/etc/environment", "/etc/profile.d/"}},
	{schema.ConfigCategoryAudit, []string{"/etc/audit/rules.d/"}},
	{schema.ConfigCategoryLibraryPath, []string{"/etc/ld.so.conf.d/"}},
	{schema.ConfigCategoryJournal, []string{"/etc/systemd/journald.conf.d/"}},
	{schema.ConfigCategoryLogrotate, []string{"/etc/logrotate.d/"}},
	{schema.ConfigCategoryAutomount, []string{"/etc/auto.master", "/etc/auto."}},
	{schema.ConfigCategorySysctl, []string{"/etc/sysctl.d/", "/etc/sysctl.conf"}},
	{schema.ConfigCategoryCryptoPolicy, []string{"/etc/crypto-policies/"}},
	{schema.ConfigCategoryIdentity, []string{"/etc/nsswitch.conf", "/etc/sssd/", "/etc/krb5.conf", "/etc/krb5.conf.d/", "/etc/ipa/"}},
	{schema.ConfigCategoryLimits, []string{"/etc/security/limits."}},
}

// ClassifyConfigPath assigns a semantic category to a config file path.
func ClassifyConfigPath(path string) schema.ConfigCategory {
	for _, rule := range categoryRules {
		for _, prefix := range rule.prefixes {
			if path == prefix {
				return rule.category
			}
			if (strings.HasSuffix(prefix, "/") || strings.HasSuffix(prefix, ".")) && strings.HasPrefix(path, prefix) {
				return rule.category
			}
		}
	}
	return schema.ConfigCategoryOther
}

// ---------------------------------------------------------------------------
// Crypto policy detection
// ---------------------------------------------------------------------------

var cryptoPolicyNameRe = regexp.MustCompile(`^[A-Z][A-Z0-9_:.\-]*$`)

func detectCryptoPolicy(exec Executor, warnings *[]Warning) {
	content, err := exec.ReadFile("/etc/crypto-policies/config")
	if err != nil {
		return
	}
	lines := strings.SplitN(content, "\n", 2)
	policy := strings.TrimSpace(lines[0])
	if policy == "" {
		return
	}
	// Strip inline comments
	if idx := strings.Index(policy, "#"); idx >= 0 {
		policy = strings.TrimSpace(policy[:idx])
	}
	if policy == "" {
		return
	}
	if !cryptoPolicyNameRe.MatchString(policy) {
		if warnings != nil {
			*warnings = append(*warnings, makeWarning("config",
				fmt.Sprintf("System crypto policy value %q contains unexpected characters — Containerfile update-crypto-policies command will be skipped", policy)))
		}
		return
	}
	if policy != "DEFAULT" && warnings != nil {
		*warnings = append(*warnings, makeWarning("config",
			fmt.Sprintf("System crypto policy is set to %s — base image may use DEFAULT", policy),
			"info"))
	}
}

// ---------------------------------------------------------------------------
// RPM-owned path set (shared with scheduled_tasks inspector)
// ---------------------------------------------------------------------------

// BuildRpmOwnedPaths builds the set of all /etc paths owned by any installed
// RPM package via a single bulk query. The result is used by both the Config
// and ScheduledTasks inspectors to classify files as RPM-owned vs unowned.
func BuildRpmOwnedPaths(exec Executor) (map[string]bool, []Warning) {
	var warnings []Warning
	paths := make(map[string]bool)

	result := exec.Run("rpm", "-qa", "--queryformat", "[%{FILENAMES}\n]")
	if result.ExitCode != 0 {
		// Fallback with --root
		result = exec.Run("rpm", "--root", exec.HostRoot(), "-qa", "--queryformat", "[%{FILENAMES}\n]")
	}
	if result.ExitCode != 0 {
		warnings = append(warnings, makeWarning("config",
			"rpm -qa --queryformat failed — unowned file detection is unavailable. "+
				"Config files not owned by any RPM package will not be captured."))
		return paths, warnings
	}

	for _, line := range strings.Split(result.Stdout, "\n") {
		p := strings.TrimSpace(line)
		if strings.HasPrefix(p, "/etc") {
			paths[p] = true
		}
	}
	return paths, warnings
}

// getOwningPackage returns the RPM package owning a path, or empty string.
func getOwningPackage(exec Executor, path string) string {
	r := exec.Run("rpm", "-qf", path)
	if r.ExitCode != 0 {
		r = exec.Run("rpm", "--root", exec.HostRoot(), "-qf", path)
	}
	if r.ExitCode != 0 || strings.TrimSpace(r.Stdout) == "" {
		return ""
	}
	lines := strings.SplitN(strings.TrimSpace(r.Stdout), "\n", 2)
	return strings.TrimSpace(lines[0])
}

// ---------------------------------------------------------------------------
// Config diff helpers
// ---------------------------------------------------------------------------

func unifiedDiff(original, current, path string) string {
	origLines := strings.Split(original, "\n")
	currLines := strings.Split(current, "\n")

	var out []string
	out = append(out, fmt.Sprintf("--- rpm\t%s", path))
	out = append(out, fmt.Sprintf("+++ current\t%s", path))

	// Simple unified diff: show full content if they differ.
	// For production fidelity a real diff algorithm would be used, but
	// the Go stdlib doesn't include one. We produce a simplified diff
	// that shows removed/added lines in a single hunk.
	maxCtx := len(origLines)
	if len(currLines) > maxCtx {
		maxCtx = len(currLines)
	}
	out = append(out, fmt.Sprintf("@@ -1,%d +1,%d @@", len(origLines), len(currLines)))
	for _, l := range origLines {
		out = append(out, "-"+l)
	}
	for _, l := range currLines {
		out = append(out, "+"+l)
	}
	return strings.Join(out, "\n")
}

// extractOriginalFromRpm tries to get the RPM-shipped original content for
// a config file. Strategy 1: find RPM in dnf cache. Strategy 2: download.
func extractOriginalFromRpm(exec Executor, pkg, pathInRpm string) string {
	// Strategy 1: look in dnf cache
	cacheResult := exec.Run("sh", "-c",
		fmt.Sprintf("find %s/var/cache/dnf -name '%s-*.rpm' 2>/dev/null | head -1",
			exec.HostRoot(), pkg))
	rpmPath := strings.TrimSpace(cacheResult.Stdout)
	if rpmPath != "" {
		content := extractFileFromRpm(exec, rpmPath, pathInRpm)
		if content != "" {
			return content
		}
	}

	// Strategy 2: download from repos
	content := downloadAndExtract(exec, pkg, pathInRpm)
	return content
}

func extractFileFromRpm(exec Executor, rpmPath, pathInRpm string) string {
	r := exec.Run("sh", "-c",
		fmt.Sprintf("rpm2cpio %s | cpio -i --to-stdout ./%s 2>/dev/null", rpmPath, pathInRpm))
	if r.ExitCode != 0 {
		return ""
	}
	return r.Stdout
}

func downloadAndExtract(exec Executor, pkg, pathInRpm string) string {
	// Use dnf download to a temp dir, extract, clean up
	r := exec.Run("sh", "-c",
		fmt.Sprintf("d=$(mktemp -d) && dnf download --destdir \"$d\" --installroot %s --releasever=/ %s 2>/dev/null && "+
			"rpm2cpio \"$d\"/%s-*.rpm 2>/dev/null | cpio -i --to-stdout ./%s 2>/dev/null; "+
			"rm -rf \"$d\"",
			exec.HostRoot(), pkg, pkg, pathInRpm))
	if r.ExitCode != 0 {
		return ""
	}
	return r.Stdout
}

// ---------------------------------------------------------------------------
// Ostree/bootc config detection
// ---------------------------------------------------------------------------

var ostreeVolatileNames = map[string]bool{
	"resolv.conf": true, "hostname": true, "machine-id": true,
	".updated": true, "ld.so.cache": true,
}

var ostreeSkipBasenames = map[string]bool{
	"os-release": true,
}

func runOstreeConfig(exec Executor) *schema.ConfigSection {
	section := &schema.ConfigSection{Files: []schema.ConfigFileEntry{}}
	_ = exec.HostRoot() // available via exec.HostRoot() where needed

	usrEtc := "/usr/etc"
	etc := "/etc"

	if !exec.FileExists(etc) {
		return section
	}

	// Track /etc paths covered by Tier 1 (have a /usr/etc counterpart)
	tier1Paths := make(map[string]bool)

	// Tier 1: /usr/etc -> /etc diff
	if exec.FileExists(usrEtc) {
		walkEtcRecursive(exec, usrEtc, func(relPath string) {
			basename := filepath.Base(relPath)
			if ostreeVolatileNames[basename] {
				return
			}

			etcPath := filepath.Join(etc, relPath)
			tier1Paths[etcPath] = true

			if !exec.FileExists(etcPath) {
				return // Only in /usr/etc — normal ostree behavior
			}

			displayPath := "etc/" + relPath

			// Content comparison
			usrContent, err1 := exec.ReadFile(filepath.Join(usrEtc, relPath))
			etcContent, err2 := exec.ReadFile(etcPath)
			if err1 != nil || err2 != nil {
				return
			}

			if usrContent != etcContent {
				diff := unifiedDiff(usrContent, etcContent, displayPath)
				section.Files = append(section.Files, schema.ConfigFileEntry{
					Path:           displayPath,
					Kind:           schema.ConfigFileKindRpmOwnedModified,
					Category:       ClassifyConfigPath("/" + displayPath),
					Content:        etcContent,
					DiffAgainstRpm: strPtr(diff),
				})
			}
		})
	}

	// Tier 2: /etc-only files (no /usr/etc counterpart)
	walkEtcRecursive(exec, etc, func(relPath string) {
		absPath := filepath.Join(etc, relPath)
		if tier1Paths[absPath] {
			return
		}

		basename := filepath.Base(relPath)
		if ostreeVolatileNames[basename] || ostreeSkipBasenames[basename] {
			return
		}

		displayPath := "etc/" + relPath
		canonPath := "/" + displayPath

		// Check RPM ownership
		r := exec.Run("rpm", "-qf", canonPath)
		if r.ExitCode != 0 {
			r = exec.Run("rpm", "--root", exec.HostRoot(), "-qf", canonPath)
		}
		if r.ExitCode == 0 && strings.TrimSpace(r.Stdout) != "" {
			// RPM-owned — check if modified
			pkg := strings.TrimSpace(strings.SplitN(r.Stdout, "\n", 2)[0])
			vResult := exec.Run("rpm", "-V", pkg)
			if vResult.ExitCode != 0 {
				vResult = exec.Run("rpm", "--root", exec.HostRoot(), "-V", pkg)
			}
			if strings.Contains(vResult.Stdout, canonPath) {
				content, _ := exec.ReadFile(absPath)
				section.Files = append(section.Files, schema.ConfigFileEntry{
					Path:     displayPath,
					Kind:     schema.ConfigFileKindRpmOwnedModified,
					Category: ClassifyConfigPath(canonPath),
					Content:  content,
					Package:  strPtr(pkg),
				})
			}
			return
		}

		// Unowned
		content, _ := exec.ReadFile(absPath)
		section.Files = append(section.Files, schema.ConfigFileEntry{
			Path:     displayPath,
			Kind:     schema.ConfigFileKindUnowned,
			Category: ClassifyConfigPath(canonPath),
			Content:  content,
		})
	})

	return section
}

// walkEtcRecursive walks a directory tree via the Executor, calling fn with
// each file's path relative to root. Prunes dev-artifact directories.
func walkEtcRecursive(exec Executor, root string, fn func(relPath string)) {
	var walk func(dir, rel string)
	walk = func(dir, rel string) {
		entries, err := exec.ReadDir(dir)
		if err != nil {
			return
		}

		// Check for VCS prune markers
		nameSet := make(map[string]bool)
		for _, e := range entries {
			nameSet[e.Name()] = true
		}
		for marker := range pruneMarkers {
			if nameSet[marker] {
				return
			}
		}

		for _, e := range entries {
			name := e.Name()
			childRel := name
			if rel != "" {
				childRel = rel + "/" + name
			}

			if e.IsDir() {
				if !skipDirNames[name] {
					walk(filepath.Join(dir, name), childRel)
				}
				continue
			}
			fn(childRel)
		}
	}
	walk(root, "")
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

// ConfigOptions configures the Config inspector.
type ConfigOptions struct {
	// RpmVa is the rpm -Va output from the RPM inspector.
	RpmVa []schema.RpmVaEntry

	// RpmOwnedPaths is a pre-built set of RPM-owned /etc paths.
	// If nil, BuildRpmOwnedPaths is called internally.
	RpmOwnedPaths map[string]bool

	// ConfigDiffs enables unified diff output against RPM originals.
	ConfigDiffs bool

	// SystemType is the detected system type.
	SystemType schema.SystemType

	// RemovedPackages from dnf history (for orphan detection).
	RemovedPackages []string
}

// RunConfig runs the Config inspector and returns the section, warnings,
// and any fatal error.
func RunConfig(exec Executor, opts ConfigOptions) (*schema.ConfigSection, []Warning, error) {
	var warnings []Warning

	// Branch early for ostree/bootc systems
	isOstree := opts.SystemType == schema.SystemTypeRpmOstree ||
		opts.SystemType == schema.SystemTypeBootc
	if isOstree {
		section := runOstreeConfig(exec)
		return section, warnings, nil
	}

	section := &schema.ConfigSection{Files: []schema.ConfigFileEntry{}}

	if !exec.FileExists("/etc") {
		return section, warnings, nil
	}

	detectCryptoPolicy(exec, &warnings)

	// Index rpm -Va entries that are under /etc
	rpmVaPaths := make(map[string]bool)
	rpmVaByPath := make(map[string]schema.RpmVaEntry)
	for _, entry := range opts.RpmVa {
		if strings.HasPrefix(entry.Path, "/etc") {
			rpmVaPaths[entry.Path] = true
			rpmVaByPath[entry.Path] = entry
		}
	}

	// 1) RPM-owned modified files (from rpm -Va)
	configDiffFailures := 0
	// Sort paths for deterministic output
	vaPaths := make([]string, 0, len(rpmVaByPath))
	for p := range rpmVaByPath {
		vaPaths = append(vaPaths, p)
	}
	sort.Strings(vaPaths)

	for _, path := range vaPaths {
		entry := rpmVaByPath[path]
		if !exec.FileExists(path) {
			continue
		}
		content, _ := exec.ReadFile(path)

		var diffAgainstRpm *string
		if opts.ConfigDiffs {
			pkg := getOwningPackage(exec, path)
			if pkg == "" && entry.Package != nil {
				pkg = *entry.Package
			}
			pathInRpm := strings.TrimPrefix(path, "/")
			original := ""
			if pkg != "" {
				original = extractOriginalFromRpm(exec, pkg, pathInRpm)
			}
			if original != "" {
				d := unifiedDiff(original, content, path)
				diffAgainstRpm = &d
			} else {
				configDiffFailures++
				content += "\n# NOTE: could not retrieve RPM default for diff — full file included\n"
			}
		}

		fe := schema.ConfigFileEntry{
			Path:           path,
			Kind:           schema.ConfigFileKindRpmOwnedModified,
			Category:       ClassifyConfigPath(path),
			Content:        content,
			RpmVaFlags:     strPtr(entry.Flags),
			DiffAgainstRpm: diffAgainstRpm,
		}
		if entry.Package != nil {
			fe.Package = entry.Package
		}
		section.Files = append(section.Files, fe)
	}

	if opts.ConfigDiffs && configDiffFailures > 0 {
		warnings = append(warnings, makeWarning("config",
			fmt.Sprintf("--config-diffs: %d file(s) could not be diffed against RPM defaults "+
				"(RPM not found in cache or repos) — full file content included instead.",
				configDiffFailures)))
	}

	// 2) Unowned files: in /etc but not RPM-owned
	rpmOwned := opts.RpmOwnedPaths
	if rpmOwned == nil {
		var ownedWarnings []Warning
		rpmOwned, ownedWarnings = BuildRpmOwnedPaths(exec)
		warnings = append(warnings, ownedWarnings...)
	}

	walkEtcRecursive(exec, "/etc", func(relPath string) {
		absPath := "/etc/" + relPath
		if rpmVaPaths[absPath] {
			return // already captured as modified
		}
		if rpmOwned[absPath] {
			return // RPM-owned, not modified
		}
		if isExcludedUnowned(absPath) {
			return
		}
		if IsDevArtifact(absPath, exec.HostRoot()) {
			return
		}

		content, _ := exec.ReadFile(absPath)
		section.Files = append(section.Files, schema.ConfigFileEntry{
			Path:     absPath,
			Kind:     schema.ConfigFileKindUnowned,
			Category: ClassifyConfigPath(absPath),
			Content:  content,
		})
	})

	// 3) Orphaned configs from removed packages
	if len(opts.RemovedPackages) > 0 {
		seenPaths := make(map[string]bool)
		for _, fe := range section.Files {
			seenPaths[fe.Path] = true
		}

		for _, pkgName := range opts.RemovedPackages {
			// Walk /etc looking for files whose name contains the package name
			walkEtcRecursive(exec, "/etc", func(relPath string) {
				if !strings.Contains(filepath.Base(relPath), pkgName) {
					return
				}
				absPath := "/etc/" + relPath
				if seenPaths[absPath] || rpmOwned[absPath] {
					return
				}
				seenPaths[absPath] = true

				content, _ := exec.ReadFile(absPath)
				section.Files = append(section.Files, schema.ConfigFileEntry{
					Path:     absPath,
					Kind:     schema.ConfigFileKindOrphaned,
					Category: ClassifyConfigPath(absPath),
					Content:  content,
					Package:  strPtr(pkgName),
				})
			})
		}
	}

	return section, warnings, nil
}

// strPtr returns a pointer to s.
func strPtr(s string) *string {
	return &s
}
