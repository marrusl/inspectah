package inspector

import (
	"strings"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// ---------------------------------------------------------------------------
// ClassifyConfigPath
// ---------------------------------------------------------------------------

func TestClassifyConfigPath(t *testing.T) {
	tests := []struct {
		path string
		want schema.ConfigCategory
	}{
		// tmpfiles
		{"/etc/tmpfiles.d/custom.conf", schema.ConfigCategoryTmpfiles},
		// environment
		{"/etc/environment", schema.ConfigCategoryEnvironment},
		{"/etc/profile.d/custom.sh", schema.ConfigCategoryEnvironment},
		// audit
		{"/etc/audit/rules.d/50-custom.rules", schema.ConfigCategoryAudit},
		// library_path
		{"/etc/ld.so.conf.d/custom.conf", schema.ConfigCategoryLibraryPath},
		// journal
		{"/etc/systemd/journald.conf.d/99-size.conf", schema.ConfigCategoryJournal},
		// logrotate
		{"/etc/logrotate.d/httpd", schema.ConfigCategoryLogrotate},
		// automount — exact match
		{"/etc/auto.master", schema.ConfigCategoryAutomount},
		// automount — prefix with dot
		{"/etc/auto.nfs", schema.ConfigCategoryAutomount},
		// sysctl
		{"/etc/sysctl.d/99-custom.conf", schema.ConfigCategorySysctl},
		{"/etc/sysctl.conf", schema.ConfigCategorySysctl},
		// crypto_policy
		{"/etc/crypto-policies/config", schema.ConfigCategoryCryptoPolicy},
		// identity
		{"/etc/nsswitch.conf", schema.ConfigCategoryIdentity},
		{"/etc/sssd/sssd.conf", schema.ConfigCategoryIdentity},
		{"/etc/krb5.conf", schema.ConfigCategoryIdentity},
		{"/etc/krb5.conf.d/custom.conf", schema.ConfigCategoryIdentity},
		{"/etc/ipa/default.conf", schema.ConfigCategoryIdentity},
		// limits
		{"/etc/security/limits.conf", schema.ConfigCategoryLimits},
		{"/etc/security/limits.d/99-custom.conf", schema.ConfigCategoryLimits},
		// other
		{"/etc/httpd/conf/httpd.conf", schema.ConfigCategoryOther},
		{"/etc/fstab", schema.ConfigCategoryOther},
		{"/etc/my.cnf", schema.ConfigCategoryOther},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			got := ClassifyConfigPath(tt.path)
			assert.Equal(t, tt.want, got)
		})
	}
}

// ---------------------------------------------------------------------------
// isExcludedUnowned
// ---------------------------------------------------------------------------

func TestIsExcludedUnowned(t *testing.T) {
	tests := []struct {
		path     string
		excluded bool
	}{
		// Exact matches
		{"/etc/machine-id", true},
		{"/etc/hostname", true},
		{"/etc/resolv.conf", true},
		{"/etc/ld.so.cache", true},
		{"/etc/dnf/dnf.conf", true},
		{"/etc/tuned/active_profile", true},
		// Glob matches
		{"/etc/ssh/ssh_host_rsa_key", true},
		{"/etc/ssh/ssh_host_ed25519_key.pub", true},
		{"/etc/alternatives/java", true},
		{"/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem", true},
		{"/etc/systemd/system/multi-user.target.wants/sshd.service", true},
		{"/etc/lvm/archive/vg00_00001.vg", true},
		// Not excluded
		{"/etc/httpd/conf/httpd.conf", false},
		{"/etc/sysctl.d/99-custom.conf", false},
		{"/etc/sssd/sssd.conf", false},
		{"/etc/fstab", false},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			got := isExcludedUnowned(tt.path)
			assert.Equal(t, tt.excluded, got)
		})
	}
}

// ---------------------------------------------------------------------------
// IsDevArtifact
// ---------------------------------------------------------------------------

func TestIsDevArtifact(t *testing.T) {
	tests := []struct {
		path     string
		hostRoot string
		want     bool
	}{
		{"/etc/myapp/config.yml", "/", false},
		{"/home/user/project/.git/config", "/", true},
		{"/opt/app/node_modules/pkg/index.js", "/", true},
		{"/srv/code/__pycache__/mod.pyc", "/", true},
		{"/etc/.vscode/settings.json", "/", true},
		{"/etc/httpd/conf/httpd.conf", "/", false},
		// hostRoot scoping
		{"/sysroot/etc/app/config", "/sysroot", false},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			got := IsDevArtifact(tt.path, tt.hostRoot)
			assert.Equal(t, tt.want, got)
		})
	}
}

// ---------------------------------------------------------------------------
// BuildRpmOwnedPaths
// ---------------------------------------------------------------------------

func TestBuildRpmOwnedPaths(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"rpm -qa --queryformat [%{FILENAMES}\n]": {
			Stdout: "/etc/httpd/conf/httpd.conf\n/usr/bin/httpd\n/etc/sysconfig/httpd\n/usr/lib64/libfoo.so\n",
		},
	})

	paths, warnings := BuildRpmOwnedPaths(exec)
	assert.Empty(t, warnings)
	assert.True(t, paths["/etc/httpd/conf/httpd.conf"])
	assert.True(t, paths["/etc/sysconfig/httpd"])
	assert.False(t, paths["/usr/bin/httpd"], "non-/etc paths should be excluded")
	assert.False(t, paths["/usr/lib64/libfoo.so"])
}

func TestBuildRpmOwnedPaths_Failure(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"rpm -qa --queryformat [%{FILENAMES}\n]": {ExitCode: 1},
		"rpm --root / -qa --queryformat [%{FILENAMES}\n]": {ExitCode: 1},
	})

	paths, warnings := BuildRpmOwnedPaths(exec)
	assert.Empty(t, paths)
	require.Len(t, warnings, 1)
	assert.Contains(t, warnings[0]["message"], "unowned file detection is unavailable")
}

// ---------------------------------------------------------------------------
// RunConfig — package-mode basics
// ---------------------------------------------------------------------------

func TestRunConfig_RpmOwnedModified(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/etc/httpd/conf/httpd.conf":   "ServerRoot /etc/httpd\nListen 8080\n",
			"/etc/sysctl.d/99-custom.conf": "vm.swappiness=10\n",
		}).
		WithDirs(map[string][]string{
			"/etc":            {"httpd", "sysctl.d"},
			"/etc/httpd":      {"conf"},
			"/etc/httpd/conf": {"httpd.conf"},
			"/etc/sysctl.d":   {"99-custom.conf"},
		})

	opts := ConfigOptions{
		RpmVa: []schema.RpmVaEntry{
			{Path: "/etc/httpd/conf/httpd.conf", Flags: "S.5....T.", Package: strPtr("httpd")},
		},
		RpmOwnedPaths: map[string]bool{
			"/etc/httpd/conf/httpd.conf": true,
			"/etc/sysctl.d/99-custom.conf": true,
		},
		SystemType: schema.SystemTypePackageMode,
	}

	section, warnings, err := RunConfig(exec, opts)
	require.NoError(t, err)
	assert.Empty(t, warnings)

	// Should have the modified file
	var modified []schema.ConfigFileEntry
	for _, f := range section.Files {
		if f.Kind == schema.ConfigFileKindRpmOwnedModified {
			modified = append(modified, f)
		}
	}
	require.Len(t, modified, 1)
	assert.Equal(t, "/etc/httpd/conf/httpd.conf", modified[0].Path)
	assert.Equal(t, schema.ConfigCategoryOther, modified[0].Category)
	assert.NotNil(t, modified[0].RpmVaFlags)
	assert.Equal(t, "S.5....T.", *modified[0].RpmVaFlags)
	assert.NotNil(t, modified[0].Package)
	assert.Equal(t, "httpd", *modified[0].Package)
}

func TestRunConfig_UnownedFiles(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/etc/myapp/app.conf":       "setting=value\n",
			"/etc/tmpfiles.d/myapp.conf": "d /run/myapp 0755\n",
		}).
		WithDirs(map[string][]string{
			"/etc":            {"myapp", "tmpfiles.d"},
			"/etc/myapp":      {"app.conf"},
			"/etc/tmpfiles.d": {"myapp.conf"},
		})

	opts := ConfigOptions{
		RpmVa:         nil,
		RpmOwnedPaths: map[string]bool{}, // nothing RPM-owned
		SystemType:    schema.SystemTypePackageMode,
	}

	section, _, err := RunConfig(exec, opts)
	require.NoError(t, err)

	assert.Len(t, section.Files, 2)
	for _, f := range section.Files {
		assert.Equal(t, schema.ConfigFileKindUnowned, f.Kind)
	}

	// Check category classification
	byPath := make(map[string]schema.ConfigFileEntry)
	for _, f := range section.Files {
		byPath[f.Path] = f
	}
	assert.Equal(t, schema.ConfigCategoryOther, byPath["/etc/myapp/app.conf"].Category)
	assert.Equal(t, schema.ConfigCategoryTmpfiles, byPath["/etc/tmpfiles.d/myapp.conf"].Category)
}

func TestRunConfig_ExcludedUnownedSkipped(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/etc/machine-id":    "abc123\n",
			"/etc/resolv.conf":   "nameserver 8.8.8.8\n",
			"/etc/myapp/custom.conf": "key=val\n",
		}).
		WithDirs(map[string][]string{
			"/etc":       {"machine-id", "resolv.conf", "myapp"},
			"/etc/myapp": {"custom.conf"},
		})

	opts := ConfigOptions{
		RpmOwnedPaths: map[string]bool{},
		SystemType:    schema.SystemTypePackageMode,
	}

	section, _, err := RunConfig(exec, opts)
	require.NoError(t, err)

	// Only custom.conf should appear (machine-id and resolv.conf are excluded)
	require.Len(t, section.Files, 1)
	assert.Equal(t, "/etc/myapp/custom.conf", section.Files[0].Path)
}

func TestRunConfig_OrphanedConfigs(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/etc/myoldpkg.conf": "old config\n",
		}).
		WithDirs(map[string][]string{
			"/etc": {"myoldpkg.conf"},
		})

	opts := ConfigOptions{
		RpmOwnedPaths:   map[string]bool{},
		RemovedPackages: []string{"myoldpkg"},
		SystemType:      schema.SystemTypePackageMode,
	}

	section, _, err := RunConfig(exec, opts)
	require.NoError(t, err)

	// File should appear as both unowned (from walk) and we need to check
	// the orphan pass found it. The walk finds it as unowned first, so
	// the orphan pass won't duplicate it (seenPaths check).
	// Let's use a file that ISN'T found by the unowned walk
	// by making it excluded from unowned but matching the pkg name.
	// Actually, let's test with a file that matches the orphan pattern.
	var orphaned []schema.ConfigFileEntry
	var unowned []schema.ConfigFileEntry
	for _, f := range section.Files {
		switch f.Kind {
		case schema.ConfigFileKindOrphaned:
			orphaned = append(orphaned, f)
		case schema.ConfigFileKindUnowned:
			unowned = append(unowned, f)
		}
	}
	// The file is found as unowned first (since it's in /etc and not RPM-owned).
	// The orphan pass checks seenPaths and won't duplicate.
	assert.Len(t, unowned, 1)
	assert.Len(t, orphaned, 0, "already captured as unowned, not duplicated as orphaned")
}

func TestRunConfig_OrphanedNotDuplicated(t *testing.T) {
	// Scenario: file matches orphan pattern but is NOT found by unowned walk
	// (e.g. it's in an excluded glob). Use a sub-path that dodges the exclusion
	// but wouldn't be found by the standard walk because it requires the
	// orphan scan pattern matching.
	exec := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/etc/removed-svc.conf": "old service config\n",
		}).
		WithDirs(map[string][]string{
			"/etc": {"removed-svc.conf"},
		})

	opts := ConfigOptions{
		RpmOwnedPaths:   map[string]bool{"/etc/removed-svc.conf": true}, // pretend RPM-owned
		RemovedPackages: []string{"removed-svc"},
		SystemType:      schema.SystemTypePackageMode,
	}

	section, _, err := RunConfig(exec, opts)
	require.NoError(t, err)

	// The file is RPM-owned so it won't appear as unowned.
	// The orphan pass also skips it because rpmOwned[absPath] is true.
	assert.Empty(t, section.Files)
}

func TestRunConfig_DevArtifactFiltering(t *testing.T) {
	// A directory containing .git is a source checkout — the entire subtree
	// is pruned (matching Python's filtered_rglob behavior). The node_modules
	// directory itself is skipped (never descended into).
	exec := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/etc/checkout/.git/config":       "git config\n",
			"/etc/checkout/src/main.go":       "package main\n",
			"/etc/builddir/node_modules/x":    "module x\n",
			"/etc/myapp/real.conf":            "real config\n",
		}).
		WithDirs(map[string][]string{
			"/etc":                       {"checkout", "builddir", "myapp"},
			"/etc/checkout":              {".git", "src"},
			"/etc/checkout/.git":         {"config"},
			"/etc/checkout/src":          {"main.go"},
			"/etc/builddir":              {"node_modules", "other.conf"},
			"/etc/builddir/node_modules": {"x"},
			"/etc/myapp":                 {"real.conf"},
		}).
		WithFiles(map[string]string{
			"/etc/builddir/other.conf": "other\n",
		})

	opts := ConfigOptions{
		RpmOwnedPaths: map[string]bool{},
		SystemType:    schema.SystemTypePackageMode,
	}

	section, _, err := RunConfig(exec, opts)
	require.NoError(t, err)

	// checkout/ is pruned (contains .git marker), node_modules/ is skipped.
	// Only real.conf and other.conf (sibling of node_modules) should appear.
	paths := make(map[string]bool)
	for _, f := range section.Files {
		paths[f.Path] = true
	}
	assert.True(t, paths["/etc/myapp/real.conf"], "real.conf should be found")
	assert.True(t, paths["/etc/builddir/other.conf"], "other.conf beside node_modules should be found")
	assert.False(t, paths["/etc/checkout/src/main.go"], "file under .git-containing dir should be pruned")
	assert.Len(t, section.Files, 2)
}

func TestRunConfig_NoEtc(t *testing.T) {
	exec := NewFakeExecutor(nil)
	// /etc doesn't exist (FileExists returns false for unknown paths)

	opts := ConfigOptions{
		SystemType: schema.SystemTypePackageMode,
	}

	section, warnings, err := RunConfig(exec, opts)
	require.NoError(t, err)
	assert.Empty(t, warnings)
	assert.Empty(t, section.Files)
}

func TestRunConfig_ConfigDiffs(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"rpm -qf /etc/httpd/conf/httpd.conf": {
			Stdout: "httpd-2.4.57-1.el9.x86_64\n",
		},
		// Cache lookup
		"sh -c find //var/cache/dnf -name 'httpd-*.rpm' 2>/dev/null | head -1": {
			Stdout: "/var/cache/dnf/httpd-2.4.57-1.el9.x86_64.rpm\n",
		},
		// Extract from RPM
		"sh -c rpm2cpio /var/cache/dnf/httpd-2.4.57-1.el9.x86_64.rpm | cpio -i --to-stdout ./etc/httpd/conf/httpd.conf 2>/dev/null": {
			Stdout: "ServerRoot /etc/httpd\nListen 80\n",
		},
	}).
		WithFiles(map[string]string{
			"/etc/httpd/conf/httpd.conf": "ServerRoot /etc/httpd\nListen 8080\n",
		}).
		WithDirs(map[string][]string{
			"/etc": {"httpd"},
			"/etc/httpd": {"conf"},
			"/etc/httpd/conf": {"httpd.conf"},
		})

	opts := ConfigOptions{
		RpmVa: []schema.RpmVaEntry{
			{Path: "/etc/httpd/conf/httpd.conf", Flags: "S.5....T.", Package: strPtr("httpd")},
		},
		RpmOwnedPaths: map[string]bool{"/etc/httpd/conf/httpd.conf": true},
		ConfigDiffs:   true,
		SystemType:    schema.SystemTypePackageMode,
	}

	section, _, err := RunConfig(exec, opts)
	require.NoError(t, err)

	require.Len(t, section.Files, 1)
	f := section.Files[0]
	assert.Equal(t, schema.ConfigFileKindRpmOwnedModified, f.Kind)
	assert.NotNil(t, f.DiffAgainstRpm)
	assert.Contains(t, *f.DiffAgainstRpm, "--- rpm")
	assert.Contains(t, *f.DiffAgainstRpm, "+++ current")
}

func TestRunConfig_ConfigDiffs_Failure(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"rpm -qf /etc/custom.conf": {ExitCode: 1},
		"rpm --root / -qf /etc/custom.conf": {ExitCode: 1},
	}).
		WithFiles(map[string]string{
			"/etc/custom.conf": "custom content\n",
		}).
		WithDirs(map[string][]string{
			"/etc": {"custom.conf"},
		})

	opts := ConfigOptions{
		RpmVa: []schema.RpmVaEntry{
			{Path: "/etc/custom.conf", Flags: "S.5....T."},
		},
		RpmOwnedPaths: map[string]bool{"/etc/custom.conf": true},
		ConfigDiffs:   true,
		SystemType:    schema.SystemTypePackageMode,
	}

	section, warnings, err := RunConfig(exec, opts)
	require.NoError(t, err)

	require.Len(t, section.Files, 1)
	assert.Nil(t, section.Files[0].DiffAgainstRpm)
	assert.Contains(t, section.Files[0].Content, "could not retrieve RPM default")

	// Should produce a warning about diff failures
	require.Len(t, warnings, 1)
	assert.Contains(t, warnings[0]["message"].(string), "--config-diffs")
}

// ---------------------------------------------------------------------------
// RunConfig — ostree mode
// ---------------------------------------------------------------------------

func TestRunConfig_OstreeModifiedFile(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/usr/etc/httpd/conf/httpd.conf": "Listen 80\n",
			"/etc/httpd/conf/httpd.conf":     "Listen 8080\n",
		}).
		WithDirs(map[string][]string{
			"/usr/etc":            {"httpd"},
			"/usr/etc/httpd":      {"conf"},
			"/usr/etc/httpd/conf": {"httpd.conf"},
			"/etc":                {"httpd"},
			"/etc/httpd":          {"conf"},
			"/etc/httpd/conf":     {"httpd.conf"},
		})

	opts := ConfigOptions{
		SystemType: schema.SystemTypeBootc,
	}

	section, _, err := RunConfig(exec, opts)
	require.NoError(t, err)

	require.Len(t, section.Files, 1)
	f := section.Files[0]
	assert.Equal(t, "etc/httpd/conf/httpd.conf", f.Path)
	assert.Equal(t, schema.ConfigFileKindRpmOwnedModified, f.Kind)
	assert.Equal(t, "Listen 8080\n", f.Content)
	assert.NotNil(t, f.DiffAgainstRpm)
}

func TestRunConfig_OstreeUnchangedSkipped(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/usr/etc/ntp.conf": "server pool.ntp.org\n",
			"/etc/ntp.conf":    "server pool.ntp.org\n",
		}).
		WithDirs(map[string][]string{
			"/usr/etc": {"ntp.conf"},
			"/etc":     {"ntp.conf"},
		})

	opts := ConfigOptions{
		SystemType: schema.SystemTypeBootc,
	}

	section, _, err := RunConfig(exec, opts)
	require.NoError(t, err)

	// Unchanged files should not appear
	assert.Empty(t, section.Files)
}

func TestRunConfig_OstreeVolatileSkipped(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/usr/etc/resolv.conf": "nameserver 1.1.1.1\n",
			"/etc/resolv.conf":    "nameserver 8.8.8.8\n",
		}).
		WithDirs(map[string][]string{
			"/usr/etc": {"resolv.conf"},
			"/etc":     {"resolv.conf"},
		})

	opts := ConfigOptions{
		SystemType: schema.SystemTypeRpmOstree,
	}

	section, _, err := RunConfig(exec, opts)
	require.NoError(t, err)

	// Volatile files should be skipped even if different
	assert.Empty(t, section.Files)
}

func TestRunConfig_OstreeTier2Unowned(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		// Not RPM-owned
		"rpm -qf /etc/custom/app.conf": {ExitCode: 1, Stderr: "not owned"},
		"rpm --root / -qf /etc/custom/app.conf": {ExitCode: 1, Stderr: "not owned"},
	}).
		WithFiles(map[string]string{
			"/etc/custom/app.conf": "key=value\n",
		}).
		WithDirs(map[string][]string{
			"/usr/etc":     {},
			"/etc":         {"custom"},
			"/etc/custom":  {"app.conf"},
		})

	opts := ConfigOptions{
		SystemType: schema.SystemTypeBootc,
	}

	section, _, err := RunConfig(exec, opts)
	require.NoError(t, err)

	require.Len(t, section.Files, 1)
	assert.Equal(t, schema.ConfigFileKindUnowned, section.Files[0].Kind)
	assert.Equal(t, "etc/custom/app.conf", section.Files[0].Path)
}

// ---------------------------------------------------------------------------
// Crypto policy detection
// ---------------------------------------------------------------------------

func TestDetectCryptoPolicy_NonDefault(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/etc/crypto-policies/config": "LEGACY\n",
		}).
		WithDirs(map[string][]string{
			"/etc": {},
		})

	var warnings []Warning
	detectCryptoPolicy(exec, &warnings)

	require.Len(t, warnings, 1)
	assert.Contains(t, warnings[0]["message"].(string), "LEGACY")
}

func TestDetectCryptoPolicy_Default(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/etc/crypto-policies/config": "DEFAULT\n",
		})

	var warnings []Warning
	detectCryptoPolicy(exec, &warnings)

	assert.Empty(t, warnings)
}

func TestDetectCryptoPolicy_WithComment(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/etc/crypto-policies/config": "FIPS  # set by admin\n",
		})

	var warnings []Warning
	detectCryptoPolicy(exec, &warnings)

	require.Len(t, warnings, 1)
	assert.Contains(t, warnings[0]["message"].(string), "FIPS")
}

func TestDetectCryptoPolicy_InvalidChars(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/etc/crypto-policies/config": "$(evil)\n",
		})

	var warnings []Warning
	detectCryptoPolicy(exec, &warnings)

	require.Len(t, warnings, 1)
	assert.Contains(t, warnings[0]["message"].(string), "unexpected characters")
}

// ---------------------------------------------------------------------------
// BuildRpmOwnedPaths with fallback
// ---------------------------------------------------------------------------

func TestBuildRpmOwnedPaths_Fallback(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"rpm -qa --queryformat [%{FILENAMES}\n]": {ExitCode: 1},
		"rpm --root / -qa --queryformat [%{FILENAMES}\n]": {
			Stdout: "/etc/fallback.conf\n/usr/bin/tool\n",
		},
	})

	paths, warnings := BuildRpmOwnedPaths(exec)
	assert.Empty(t, warnings)
	assert.True(t, paths["/etc/fallback.conf"])
	assert.False(t, paths["/usr/bin/tool"])
}

// ---------------------------------------------------------------------------
// Content capture
// ---------------------------------------------------------------------------

func TestRunConfig_ContentCapture(t *testing.T) {
	content := "# Custom sysctl\nvm.swappiness = 10\nnet.ipv4.ip_forward = 1\n"
	exec := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/etc/sysctl.d/99-custom.conf": content,
		}).
		WithDirs(map[string][]string{
			"/etc":          {"sysctl.d"},
			"/etc/sysctl.d": {"99-custom.conf"},
		})

	opts := ConfigOptions{
		RpmOwnedPaths: map[string]bool{},
		SystemType:    schema.SystemTypePackageMode,
	}

	section, _, err := RunConfig(exec, opts)
	require.NoError(t, err)

	require.Len(t, section.Files, 1)
	assert.Equal(t, content, section.Files[0].Content)
}

// ---------------------------------------------------------------------------
// Multiple file types in one run
// ---------------------------------------------------------------------------

func TestRunConfig_MixedTypes(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/etc/httpd/conf/httpd.conf":    "Listen 8080\n",
			"/etc/myapp/custom.conf":        "key=val\n",
			"/etc/sysctl.d/99-custom.conf":  "vm.swappiness=10\n",
		}).
		WithDirs(map[string][]string{
			"/etc":                {"httpd", "myapp", "sysctl.d"},
			"/etc/httpd":          {"conf"},
			"/etc/httpd/conf":     {"httpd.conf"},
			"/etc/myapp":          {"custom.conf"},
			"/etc/sysctl.d":       {"99-custom.conf"},
		})

	opts := ConfigOptions{
		RpmVa: []schema.RpmVaEntry{
			{Path: "/etc/httpd/conf/httpd.conf", Flags: "S.5....T.", Package: strPtr("httpd")},
		},
		RpmOwnedPaths: map[string]bool{
			"/etc/httpd/conf/httpd.conf": true,
		},
		SystemType: schema.SystemTypePackageMode,
	}

	section, _, err := RunConfig(exec, opts)
	require.NoError(t, err)

	counts := map[schema.ConfigFileKind]int{}
	for _, f := range section.Files {
		counts[f.Kind]++
	}
	assert.Equal(t, 1, counts[schema.ConfigFileKindRpmOwnedModified])
	assert.Equal(t, 2, counts[schema.ConfigFileKindUnowned])
}

// ---------------------------------------------------------------------------
// unifiedDiff
// ---------------------------------------------------------------------------

func TestUnifiedDiff(t *testing.T) {
	original := "line1\nline2\nline3\n"
	current := "line1\nline2-modified\nline3\n"

	diff := unifiedDiff(original, current, "/etc/test.conf")
	assert.True(t, strings.HasPrefix(diff, "--- rpm"))
	assert.Contains(t, diff, "+++ current")
	assert.Contains(t, diff, "-line2")
	assert.Contains(t, diff, "+line2-modified")
}
