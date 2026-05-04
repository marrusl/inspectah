package inspector

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

func loadSelinuxFixture(t *testing.T, name string) string {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("testdata", "selinux", name))
	require.NoError(t, err, "loading fixture %s", name)
	return string(data)
}

// ---------------------------------------------------------------------------
// SELinux mode parsing
// ---------------------------------------------------------------------------

func TestCollectSELinuxMode(t *testing.T) {
	tests := []struct {
		name     string
		fixture  string
		wantMode string
	}{
		{
			name:     "enforcing",
			fixture:  "selinux-config-enforcing.txt",
			wantMode: "enforcing",
		},
		{
			name:     "permissive",
			fixture:  "selinux-config-permissive.txt",
			wantMode: "permissive",
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			content := loadSelinuxFixture(t, tt.fixture)
			fake := NewFakeExecutor(nil).WithFiles(map[string]string{
				"/etc/selinux/config": content,
			})
			section := &schema.SelinuxSection{}
			collectSELinuxMode(fake, section)
			assert.Equal(t, tt.wantMode, section.Mode)
		})
	}
}

func TestCollectSELinuxMode_NoConfig(t *testing.T) {
	fake := NewFakeExecutor(nil)
	section := &schema.SelinuxSection{}
	collectSELinuxMode(fake, section)
	assert.Equal(t, "", section.Mode)
}

// ---------------------------------------------------------------------------
// Policy type
// ---------------------------------------------------------------------------

func TestReadPolicyType(t *testing.T) {
	tests := []struct {
		name     string
		config   string
		wantType string
	}{
		{
			name:     "targeted",
			config:   "SELINUX=enforcing\nSELINUXTYPE=targeted\n",
			wantType: "targeted",
		},
		{
			name:     "mls",
			config:   "SELINUX=enforcing\nSELINUXTYPE=mls\n",
			wantType: "mls",
		},
		{
			name:     "missing config defaults to targeted",
			config:   "",
			wantType: "targeted",
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			files := map[string]string{}
			if tt.config != "" {
				files["/etc/selinux/config"] = tt.config
			}
			fake := NewFakeExecutor(nil).WithFiles(files)
			got := readPolicyType(fake)
			assert.Equal(t, tt.wantType, got)
		})
	}
}

// ---------------------------------------------------------------------------
// Boolean override parsing
// ---------------------------------------------------------------------------

func TestParseSemanageBooleans(t *testing.T) {
	input := loadSelinuxFixture(t, "semanage-boolean.txt")
	got := parseSemanageBooleans(input)

	// Only non-default booleans are returned (samba_enable_home_dirs
	// has current==default so it's filtered out)
	require.Len(t, got, 3)

	// httpd_can_network_connect: on != off → non_default=true
	assert.Equal(t, "httpd_can_network_connect", got[0]["name"])
	assert.Equal(t, "on", got[0]["current"])
	assert.Equal(t, "off", got[0]["default"])
	assert.Equal(t, true, got[0]["non_default"])
	assert.Equal(t, "Allow httpd to make outbound network connections", got[0]["description"])

	// virt_use_nfs: on != off → non_default=true
	assert.Equal(t, "virt_use_nfs", got[1]["name"])
	assert.Equal(t, true, got[1]["non_default"])

	// container_manage_cgroup: off != on → non_default=true
	assert.Equal(t, "container_manage_cgroup", got[2]["name"])
	assert.Equal(t, "off", got[2]["current"])
	assert.Equal(t, "on", got[2]["default"])
	assert.Equal(t, true, got[2]["non_default"])
}

func TestParseSemanageBooleansEmpty(t *testing.T) {
	got := parseSemanageBooleans("")
	assert.Empty(t, got)
}

// ---------------------------------------------------------------------------
// Port label parsing
// ---------------------------------------------------------------------------

func TestParseSemanagePorts(t *testing.T) {
	input := loadSelinuxFixture(t, "semanage-port.txt")
	got := parseSemanagePorts(input)

	// ssh_port_t tcp 2222 → 1 entry
	// http_port_t tcp 8080, 8443 → 2 entries
	// redis_port_t tcp 6380 → 1 entry
	require.Len(t, got, 4)

	assert.Equal(t, schema.SelinuxPortLabel{
		Protocol: "tcp", Port: "2222", Type: "ssh_port_t", Include: true,
	}, got[0])

	assert.Equal(t, schema.SelinuxPortLabel{
		Protocol: "tcp", Port: "8080", Type: "http_port_t", Include: true,
	}, got[1])

	assert.Equal(t, schema.SelinuxPortLabel{
		Protocol: "tcp", Port: "8443", Type: "http_port_t", Include: true,
	}, got[2])

	assert.Equal(t, schema.SelinuxPortLabel{
		Protocol: "tcp", Port: "6380", Type: "redis_port_t", Include: true,
	}, got[3])
}

func TestParseSemanagePortsEmpty(t *testing.T) {
	got := parseSemanagePorts("")
	assert.Empty(t, got)
}

func TestParseSemanagePortsHeaderOnly(t *testing.T) {
	got := parseSemanagePorts("SELinux port     Type              Proto    Port Number\n")
	assert.Empty(t, got)
}

// ---------------------------------------------------------------------------
// Fcontext rules
// ---------------------------------------------------------------------------

func TestCollectFcontextRules_Semanage(t *testing.T) {
	input := loadSelinuxFixture(t, "semanage-fcontext.txt")
	fake := NewFakeExecutor(map[string]ExecResult{
		"chroot / semanage fcontext -l -C": {Stdout: input, ExitCode: 0},
	})
	section := &schema.SelinuxSection{FcontextRules: []string{}}
	collectFcontextRules(fake, section, "targeted")

	require.Len(t, section.FcontextRules, 2)
	assert.Contains(t, section.FcontextRules[0], "/opt/myapp")
	assert.Contains(t, section.FcontextRules[1], "/srv/data")
}

func TestCollectFcontextRules_Fallback(t *testing.T) {
	content := loadSelinuxFixture(t, "fcontext-local.txt")
	fake := NewFakeExecutor(map[string]ExecResult{
		"chroot / semanage fcontext -l -C": {Stdout: "", ExitCode: 1},
	}).WithFiles(map[string]string{
		"/etc/selinux/targeted/contexts/files/file_contexts.local": content,
	})
	section := &schema.SelinuxSection{FcontextRules: []string{}}
	collectFcontextRules(fake, section, "targeted")

	require.Len(t, section.FcontextRules, 2)
	assert.Contains(t, section.FcontextRules[0], "/opt/myapp")
}

// ---------------------------------------------------------------------------
// Audit rules
// ---------------------------------------------------------------------------

func TestCollectAuditRules(t *testing.T) {
	fake := NewFakeExecutor(nil).WithDirs(map[string][]string{
		"/etc/audit/rules.d": {"custom.rules", "compliance.rules"},
	})
	section := &schema.SelinuxSection{AuditRules: []string{}}
	collectAuditRules(fake, section, nil)

	require.Len(t, section.AuditRules, 2)
	assert.Equal(t, "etc/audit/rules.d/compliance.rules", section.AuditRules[0])
	assert.Equal(t, "etc/audit/rules.d/custom.rules", section.AuditRules[1])
}

func TestCollectAuditRules_SkipsRpmOwned(t *testing.T) {
	fake := NewFakeExecutor(nil).WithDirs(map[string][]string{
		"/etc/audit/rules.d": {"custom.rules", "base.rules"},
	})
	rpmOwned := map[string]bool{
		"/etc/audit/rules.d/base.rules": true,
	}
	section := &schema.SelinuxSection{AuditRules: []string{}}
	collectAuditRules(fake, section, rpmOwned)

	require.Len(t, section.AuditRules, 1)
	assert.Equal(t, "etc/audit/rules.d/custom.rules", section.AuditRules[0])
}

func TestCollectAuditRules_MissingDir(t *testing.T) {
	fake := NewFakeExecutor(nil)
	section := &schema.SelinuxSection{AuditRules: []string{}}
	collectAuditRules(fake, section, nil)
	assert.Empty(t, section.AuditRules)
}

// ---------------------------------------------------------------------------
// FIPS mode
// ---------------------------------------------------------------------------

func TestCollectFIPSMode(t *testing.T) {
	tests := []struct {
		name     string
		content  string
		wantFIPS bool
	}{
		{name: "enabled", content: "1\n", wantFIPS: true},
		{name: "disabled", content: "0\n", wantFIPS: false},
		{name: "empty", content: "", wantFIPS: false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			fake := NewFakeExecutor(nil).WithFiles(map[string]string{
				"/proc/sys/crypto/fips_enabled": tt.content,
			})
			section := &schema.SelinuxSection{}
			collectFIPSMode(fake, section)
			assert.Equal(t, tt.wantFIPS, section.FipsMode)
		})
	}
}

func TestCollectFIPSMode_NoFile(t *testing.T) {
	fake := NewFakeExecutor(nil)
	section := &schema.SelinuxSection{}
	collectFIPSMode(fake, section)
	assert.False(t, section.FipsMode)
}

// ---------------------------------------------------------------------------
// PAM configs
// ---------------------------------------------------------------------------

func TestCollectPAMConfigs(t *testing.T) {
	// "login" is in the isExcludedUnowned list, so it should be skipped
	fake := NewFakeExecutor(nil).WithDirs(map[string][]string{
		"/etc/pam.d": {"custom-sshd", "login", "other"},
	})
	section := &schema.SelinuxSection{PamConfigs: []string{}}
	collectPAMConfigs(fake, section, nil)

	require.Len(t, section.PamConfigs, 2)
	assert.Equal(t, "etc/pam.d/custom-sshd", section.PamConfigs[0])
	assert.Equal(t, "etc/pam.d/other", section.PamConfigs[1])
}

func TestCollectPAMConfigs_SkipsRpmOwned(t *testing.T) {
	fake := NewFakeExecutor(nil).WithDirs(map[string][]string{
		"/etc/pam.d": {"custom-sshd", "system-auth"},
	})
	rpmOwned := map[string]bool{
		"/etc/pam.d/system-auth": true,
	}
	section := &schema.SelinuxSection{PamConfigs: []string{}}
	collectPAMConfigs(fake, section, rpmOwned)

	require.Len(t, section.PamConfigs, 1)
	assert.Equal(t, "etc/pam.d/custom-sshd", section.PamConfigs[0])
}

func TestCollectPAMConfigs_MissingDir(t *testing.T) {
	fake := NewFakeExecutor(nil)
	section := &schema.SelinuxSection{PamConfigs: []string{}}
	collectPAMConfigs(fake, section, nil)
	assert.Empty(t, section.PamConfigs)
}

// ---------------------------------------------------------------------------
// Boolean fallback (filesystem)
// ---------------------------------------------------------------------------

func TestReadBoolsFromFS(t *testing.T) {
	fake := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/sys/fs/selinux/booleans": {"httpd_can_connect", "virt_sandbox"},
		}).
		WithFiles(map[string]string{
			"/sys/fs/selinux/booleans/httpd_can_connect": "1 0",
			"/sys/fs/selinux/booleans/virt_sandbox":      "1 1",
		})

	got := readBoolsFromFS(fake)

	// httpd_can_connect: current=1 pending=0 → different → included
	// virt_sandbox: current=1 pending=1 → same → excluded
	require.Len(t, got, 1)
	assert.Equal(t, "httpd_can_connect", got[0]["name"])
	assert.Equal(t, "on", got[0]["current"])
	assert.Equal(t, "off", got[0]["default"])
	assert.Equal(t, true, got[0]["non_default"])
}

func TestReadBoolsFromFS_NoBoolDir(t *testing.T) {
	fake := NewFakeExecutor(nil)
	got := readBoolsFromFS(fake)
	assert.Nil(t, got)
}

// ---------------------------------------------------------------------------
// Custom modules
// ---------------------------------------------------------------------------

func TestCollectCustomModules(t *testing.T) {
	// FakeExecutor marks entries as dirs when their full child path exists
	// in the dirs map, so register child paths for module directories.
	fake := NewFakeExecutor(nil).WithDirs(map[string][]string{
		"/etc/selinux/targeted/active/modules/400":        {"myapp", "custom_db", "webapp"},
		"/etc/selinux/targeted/active/modules/400/myapp":      {},
		"/etc/selinux/targeted/active/modules/400/custom_db":  {},
		"/etc/selinux/targeted/active/modules/400/webapp":     {},
	})
	section := &schema.SelinuxSection{CustomModules: []string{}}
	collectCustomModules(fake, section, "targeted")

	require.Len(t, section.CustomModules, 3)
	// sorted
	assert.Equal(t, []string{"custom_db", "myapp", "webapp"}, section.CustomModules)
}

func TestCollectCustomModules_NoDir(t *testing.T) {
	fake := NewFakeExecutor(nil)
	section := &schema.SelinuxSection{CustomModules: []string{}}
	collectCustomModules(fake, section, "targeted")
	assert.Empty(t, section.CustomModules)
}

// ---------------------------------------------------------------------------
// Integration test: RunSelinux
// ---------------------------------------------------------------------------

func TestRunSelinux_Full(t *testing.T) {
	boolOutput := loadSelinuxFixture(t, "semanage-boolean.txt")
	fcontextOutput := loadSelinuxFixture(t, "semanage-fcontext.txt")
	portOutput := loadSelinuxFixture(t, "semanage-port.txt")
	selinuxConfig := loadSelinuxFixture(t, "selinux-config-enforcing.txt")

	fake := NewFakeExecutor(map[string]ExecResult{
		"chroot / semanage boolean -l":      {Stdout: boolOutput, ExitCode: 0},
		"chroot / semanage fcontext -l -C":  {Stdout: fcontextOutput, ExitCode: 0},
		"chroot / semanage port -l -C":      {Stdout: portOutput, ExitCode: 0},
	}).WithFiles(map[string]string{
		"/etc/selinux/config":          selinuxConfig,
		"/proc/sys/crypto/fips_enabled": "1\n",
	}).WithDirs(map[string][]string{
		"/etc/selinux/targeted/active/modules/400":                {"custom_policy"},
		"/etc/selinux/targeted/active/modules/400/custom_policy":  {},
		"/etc/audit/rules.d":                                      {"compliance.rules"},
		"/etc/pam.d":                                              {"custom-sshd"},
	})

	section, warnings, err := RunSelinux(fake, SelinuxOptions{})
	require.NoError(t, err)
	assert.Empty(t, warnings)

	// Mode
	assert.Equal(t, "enforcing", section.Mode)

	// Custom modules
	assert.Equal(t, []string{"custom_policy"}, section.CustomModules)

	// Boolean overrides — only non-default booleans
	require.Len(t, section.BooleanOverrides, 3)
	assert.Equal(t, "httpd_can_network_connect", section.BooleanOverrides[0]["name"])

	// Fcontext rules
	require.Len(t, section.FcontextRules, 2)

	// Port labels
	require.Len(t, section.PortLabels, 4)
	assert.Equal(t, "ssh_port_t", section.PortLabels[0].Type)
	assert.Equal(t, "2222", section.PortLabels[0].Port)

	// Audit rules
	assert.Equal(t, []string{"etc/audit/rules.d/compliance.rules"}, section.AuditRules)

	// FIPS
	assert.True(t, section.FipsMode)

	// PAM
	assert.Equal(t, []string{"etc/pam.d/custom-sshd"}, section.PamConfigs)
}

func TestRunSelinux_AllFallbacks(t *testing.T) {
	// All semanage commands fail; test filesystem fallbacks
	fcontextLocal := loadSelinuxFixture(t, "fcontext-local.txt")

	fake := NewFakeExecutor(map[string]ExecResult{
		"chroot / semanage boolean -l":     {Stdout: "", ExitCode: 1, Stderr: "command not found"},
		"chroot / semanage fcontext -l -C": {Stdout: "", ExitCode: 1},
		"chroot / semanage port -l -C":     {Stdout: "", ExitCode: 1},
	}).WithFiles(map[string]string{
		"/etc/selinux/config":          "SELINUX=disabled\nSELINUXTYPE=targeted\n",
		"/proc/sys/crypto/fips_enabled": "0\n",
		"/etc/selinux/targeted/contexts/files/file_contexts.local": fcontextLocal,
		"/sys/fs/selinux/booleans/test_bool": "1 0",
	}).WithDirs(map[string][]string{
		"/sys/fs/selinux/booleans": {"test_bool"},
	})

	section, warnings, err := RunSelinux(fake, SelinuxOptions{})
	require.NoError(t, err)
	assert.Empty(t, warnings)

	assert.Equal(t, "disabled", section.Mode)
	assert.False(t, section.FipsMode)

	// Boolean fallback
	require.Len(t, section.BooleanOverrides, 1)
	assert.Equal(t, "test_bool", section.BooleanOverrides[0]["name"])

	// Fcontext fallback
	require.Len(t, section.FcontextRules, 2)
	assert.Contains(t, section.FcontextRules[0], "/opt/myapp")
}

func TestRunSelinux_NoData(t *testing.T) {
	// Nothing available at all — should produce clean empty output
	fake := NewFakeExecutor(map[string]ExecResult{
		"chroot / semanage boolean -l":     {ExitCode: 127},
		"chroot / semanage fcontext -l -C": {ExitCode: 127},
		"chroot / semanage port -l -C":     {ExitCode: 127},
	})

	section, warnings, err := RunSelinux(fake, SelinuxOptions{})
	require.NoError(t, err)

	// Warning about missing booleans dir
	require.Len(t, warnings, 1)
	assert.Contains(t, warnings[0]["message"], "boolean override detection unavailable")

	assert.Equal(t, "", section.Mode)
	assert.Empty(t, section.CustomModules)
	assert.Empty(t, section.BooleanOverrides)
	assert.Empty(t, section.FcontextRules)
	assert.Empty(t, section.PortLabels)
	assert.Empty(t, section.AuditRules)
	assert.False(t, section.FipsMode)
	assert.Empty(t, section.PamConfigs)
}
