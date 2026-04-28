package inspector

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

func loadKernelBootFixture(t *testing.T, name string) string {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("testdata", "kernelboot", name))
	require.NoError(t, err, "fixture %s", name)
	return string(data)
}

// ---------------------------------------------------------------------------
// parseSysctlConf
// ---------------------------------------------------------------------------

func TestParseSysctlConf(t *testing.T) {
	tests := []struct {
		name  string
		input string
		want  map[string]string
	}{
		{
			name:  "basic key=value pairs",
			input: "net.ipv4.ip_forward = 1\nvm.swappiness = 10",
			want:  map[string]string{"net.ipv4.ip_forward": "1", "vm.swappiness": "10"},
		},
		{
			name:  "comments and blank lines",
			input: "# comment\n; another comment\n\nnet.core.somaxconn = 4096\n",
			want:  map[string]string{"net.core.somaxconn": "4096"},
		},
		{
			name:  "no equals sign",
			input: "this has no value\nkey = val",
			want:  map[string]string{"key": "val"},
		},
		{
			name:  "empty input",
			input: "",
			want:  map[string]string{},
		},
		{
			name:  "no whitespace around equals",
			input: "vm.swappiness=10",
			want:  map[string]string{"vm.swappiness": "10"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := parseSysctlConf(tt.input)
			assert.Equal(t, tt.want, got)
		})
	}
}

// ---------------------------------------------------------------------------
// parseLsmod
// ---------------------------------------------------------------------------

func TestParseLsmod(t *testing.T) {
	input := loadKernelBootFixture(t, "lsmod_output")
	modules := parseLsmod(input)

	assert.Len(t, modules, 10)
	assert.Equal(t, "vfat", modules[0].Name)
	assert.Equal(t, "20480", modules[0].Size)
	assert.Equal(t, "", modules[0].UsedBy)

	assert.Equal(t, "fat", modules[1].Name)
	assert.Equal(t, "vfat", modules[1].UsedBy)

	// bridge has 0 used_by count, no module list
	assert.Equal(t, "bridge", modules[5].Name)
	assert.Equal(t, "", modules[5].UsedBy)
}

func TestParseLsmodEmpty(t *testing.T) {
	modules := parseLsmod("Module                  Size  Used by\n")
	assert.Empty(t, modules)
}

// ---------------------------------------------------------------------------
// diffModules
// ---------------------------------------------------------------------------

func TestDiffModules(t *testing.T) {
	loaded := []schema.KernelModule{
		{Name: "bridge", Size: "409600", UsedBy: ""},
		{Name: "stp", Size: "16384", UsedBy: "bridge"},
		{Name: "llc", Size: "16384", UsedBy: "bridge,stp"},
		{Name: "nf_tables", Size: "323584", UsedBy: ""},
		{Name: "vfat", Size: "20480", UsedBy: ""},
	}
	expected := map[string]struct{}{
		"bridge": {},
		"vfat":   {},
	}

	nonDefault := diffModules(loaded, expected)

	// stp and llc are dependencies (non-empty UsedBy), bridge and vfat are expected.
	// Only nf_tables should remain.
	require.Len(t, nonDefault, 1)
	assert.Equal(t, "nf_tables", nonDefault[0].Name)
}

// ---------------------------------------------------------------------------
// collectExpectedModules
// ---------------------------------------------------------------------------

func TestCollectExpectedModules(t *testing.T) {
	fake := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/etc/modules-load.d": {"custom.conf"},
			"/usr/lib/modules-load.d": {"base.conf"},
		}).
		WithFiles(map[string]string{
			"/etc/modules-load.d/custom.conf":      "bridge\nvfat\n",
			"/usr/lib/modules-load.d/base.conf": "# base modules\next4\n",
		})

	expected := collectExpectedModules(fake)
	assert.Contains(t, expected, "bridge")
	assert.Contains(t, expected, "vfat")
	assert.Contains(t, expected, "ext4")
	assert.NotContains(t, expected, "# base modules")
}

// ---------------------------------------------------------------------------
// Sysctl diff
// ---------------------------------------------------------------------------

func TestDiffSysctl(t *testing.T) {
	fake := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			// Runtime values in /proc/sys
			"/proc/sys/net/ipv4/ip_forward":            "1",
			"/proc/sys/net/ipv4/conf/default/rp_filter": "2",
			"/proc/sys/vm/swappiness":                   "10",
			"/proc/sys/kernel/sysrq":                    "16",
			"/proc/sys/net/core/somaxconn":              "4096",
		})

	defaults := map[string]sysctlEntry{
		"net.ipv4.ip_forward":            {value: "0", source: "usr/lib/sysctl.d/50-default.conf"},
		"net.ipv4.conf.default.rp_filter": {value: "2", source: "usr/lib/sysctl.d/50-default.conf"},
		"vm.swappiness":                   {value: "30", source: "usr/lib/sysctl.d/50-default.conf"},
		"kernel.sysrq":                    {value: "16", source: "usr/lib/sysctl.d/50-default.conf"},
	}
	overrides := map[string]sysctlEntry{
		"net.ipv4.ip_forward": {value: "1", source: "etc/sysctl.d/99-custom.conf"},
		"vm.swappiness":       {value: "10", source: "etc/sysctl.d/99-custom.conf"},
		"net.core.somaxconn":  {value: "4096", source: "etc/sysctl.d/99-custom.conf"},
	}

	result := diffSysctl(fake, defaults, overrides)

	// rp_filter matches default (2==2), sysrq matches default (16==16) => skip
	// ip_forward: runtime 1 != default 0 => include
	// swappiness: runtime 10 != default 30 => include
	// somaxconn: no default, override present => include
	assert.Len(t, result, 3)

	byKey := make(map[string]schema.SysctlOverride)
	for _, o := range result {
		byKey[o.Key] = o
	}

	assert.Equal(t, "1", byKey["net.ipv4.ip_forward"].Runtime)
	assert.Equal(t, "0", byKey["net.ipv4.ip_forward"].Default)
	assert.Equal(t, "etc/sysctl.d/99-custom.conf", byKey["net.ipv4.ip_forward"].Source)

	assert.Equal(t, "10", byKey["vm.swappiness"].Runtime)
	assert.Equal(t, "30", byKey["vm.swappiness"].Default)

	assert.Equal(t, "4096", byKey["net.core.somaxconn"].Runtime)
	assert.Equal(t, "", byKey["net.core.somaxconn"].Default)
}

// ---------------------------------------------------------------------------
// collectConfigDir
// ---------------------------------------------------------------------------

func TestCollectConfigDir(t *testing.T) {
	content := loadKernelBootFixture(t, "modules_load.conf")
	fake := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/etc/modules-load.d": {"bridge.conf"},
		}).
		WithFiles(map[string]string{
			"/etc/modules-load.d/bridge.conf": content,
		})

	var snippets []schema.ConfigSnippet
	collectConfigDir(fake, "/etc/modules-load.d", &snippets)

	require.Len(t, snippets, 1)
	assert.Equal(t, "etc/modules-load.d/bridge.conf", snippets[0].Path)
	assert.Contains(t, snippets[0].Content, "bridge")
}

func TestCollectConfigDirSkipsNonConf(t *testing.T) {
	fake := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/etc/dracut.conf.d": {"readme.txt", "custom.conf"},
		}).
		WithFiles(map[string]string{
			"/etc/dracut.conf.d/readme.txt":   "not a config",
			"/etc/dracut.conf.d/custom.conf": "add_dracutmodules+=\" lvm \"",
		})

	var snippets []schema.ConfigSnippet
	collectConfigDir(fake, "/etc/dracut.conf.d", &snippets)

	require.Len(t, snippets, 1)
	assert.Equal(t, "etc/dracut.conf.d/custom.conf", snippets[0].Path)
}

// ---------------------------------------------------------------------------
// detectLocale
// ---------------------------------------------------------------------------

func TestDetectLocale(t *testing.T) {
	tests := []struct {
		name    string
		content string
		want    *string
	}{
		{
			name:    "standard LANG",
			content: "LANG=\"en_US.UTF-8\"\nLC_MESSAGES=\"en_US.UTF-8\"\n",
			want:    strPtr("en_US.UTF-8"),
		},
		{
			name:    "LANG without quotes",
			content: "LANG=en_GB.UTF-8\n",
			want:    strPtr("en_GB.UTF-8"),
		},
		{
			name:    "single quotes",
			content: "LANG='ja_JP.UTF-8'\n",
			want:    strPtr("ja_JP.UTF-8"),
		},
		{
			name:    "no LANG line",
			content: "LC_ALL=C\n",
			want:    nil,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			fake := NewFakeExecutor(nil).WithFiles(map[string]string{
				"/etc/locale.conf": tt.content,
			})
			got := detectLocale(fake)
			if tt.want == nil {
				assert.Nil(t, got)
			} else {
				require.NotNil(t, got)
				assert.Equal(t, *tt.want, *got)
			}
		})
	}
}

func TestDetectLocaleNoFile(t *testing.T) {
	fake := NewFakeExecutor(nil)
	got := detectLocale(fake)
	assert.Nil(t, got)
}

// ---------------------------------------------------------------------------
// detectTimezone
// ---------------------------------------------------------------------------

func TestDetectTimezone(t *testing.T) {
	fake := NewFakeExecutor(nil).WithFiles(map[string]string{
		"/etc/timezone": "America/New_York\n",
	})
	got := detectTimezone(fake)
	require.NotNil(t, got)
	assert.Equal(t, "America/New_York", *got)
}

func TestDetectTimezoneFallbackTimedatectl(t *testing.T) {
	fake := NewFakeExecutor(map[string]ExecResult{
		"timedatectl show --property=Timezone --value": {
			Stdout:   "Europe/Berlin\n",
			ExitCode: 0,
		},
	})
	got := detectTimezone(fake)
	require.NotNil(t, got)
	assert.Equal(t, "Europe/Berlin", *got)
}

func TestDetectTimezoneNone(t *testing.T) {
	fake := NewFakeExecutor(nil)
	got := detectTimezone(fake)
	assert.Nil(t, got)
}

// ---------------------------------------------------------------------------
// detectAlternatives
// ---------------------------------------------------------------------------

func TestDetectAlternatives(t *testing.T) {
	fake := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/etc/alternatives": {"java", "python3"},
		}).
		WithFiles(map[string]string{
			"/etc/alternatives/java":        "/usr/lib/jvm/java-17/bin/java",
			"/etc/alternatives/python3":     "/usr/bin/python3.11",
			"/var/lib/alternatives/java":    "manual\n",
			"/var/lib/alternatives/python3": "auto\n",
		})

	result := detectAlternatives(fake)
	require.Len(t, result, 2)

	assert.Equal(t, "java", result[0].Name)
	assert.Equal(t, "/usr/lib/jvm/java-17/bin/java", result[0].Path)
	assert.Equal(t, "manual", result[0].Status)

	assert.Equal(t, "python3", result[1].Name)
	assert.Equal(t, "/usr/bin/python3.11", result[1].Path)
	assert.Equal(t, "auto", result[1].Status)
}

func TestDetectAlternativesNoDir(t *testing.T) {
	fake := NewFakeExecutor(nil)
	result := detectAlternatives(fake)
	assert.Nil(t, result)
}

// ---------------------------------------------------------------------------
// Tuned profile collection
// ---------------------------------------------------------------------------

func TestCollectTunedFromFile(t *testing.T) {
	tunedConf := loadKernelBootFixture(t, "tuned_profile.conf")
	fake := NewFakeExecutor(nil).
		WithFiles(map[string]string{
			"/etc/tuned/active_profile": "db-custom\n",
			"/etc/tuned/db-custom/tuned.conf": tunedConf,
		}).
		WithDirs(map[string][]string{
			"/etc/tuned": {"active_profile", "db-custom"},
			"/etc/tuned/db-custom": {"tuned.conf"},
		})

	section := &schema.KernelBootSection{
		TunedCustomProfiles: []schema.ConfigSnippet{},
	}
	collectTuned(fake, section)

	assert.Equal(t, "db-custom", section.TunedActive)
	require.Len(t, section.TunedCustomProfiles, 1)
	assert.Equal(t, "etc/tuned/db-custom/tuned.conf", section.TunedCustomProfiles[0].Path)
	assert.Contains(t, section.TunedCustomProfiles[0].Content, "governor=performance")
}

func TestCollectTunedFromCommand(t *testing.T) {
	fake := NewFakeExecutor(map[string]ExecResult{
		"tuned-adm active": {
			Stdout:   "Current active profile: throughput-performance\n",
			ExitCode: 0,
		},
	}).
		WithDirs(map[string][]string{
			"/etc/tuned": {},
		})

	section := &schema.KernelBootSection{
		TunedCustomProfiles: []schema.ConfigSnippet{},
	}
	collectTuned(fake, section)

	assert.Equal(t, "throughput-performance", section.TunedActive)
}

// ---------------------------------------------------------------------------
// GRUB defaults
// ---------------------------------------------------------------------------

func TestCollectGrubDefaults(t *testing.T) {
	grubContent := loadKernelBootFixture(t, "grub_defaults")
	fake := NewFakeExecutor(nil).WithFiles(map[string]string{
		"/etc/default/grub": grubContent,
	})

	section := &schema.KernelBootSection{}
	collectGrubDefaults(fake, section, schema.SystemTypePackageMode)

	assert.Contains(t, section.GrubDefaults, "GRUB_TIMEOUT=5")
	assert.Contains(t, section.GrubDefaults, "GRUB_CMDLINE_LINUX=")
}

func TestCollectGrubDefaultsOstree(t *testing.T) {
	fake := NewFakeExecutor(nil).WithFiles(map[string]string{
		"/etc/default/grub": "GRUB_TIMEOUT=5\n",
	})

	section := &schema.KernelBootSection{}
	collectGrubDefaults(fake, section, schema.SystemTypeBootc)

	assert.Empty(t, section.GrubDefaults, "ostree systems should skip GRUB defaults")
}

func TestCollectGrubDefaultsRpmOstree(t *testing.T) {
	fake := NewFakeExecutor(nil).WithFiles(map[string]string{
		"/etc/default/grub": "GRUB_TIMEOUT=5\n",
	})

	section := &schema.KernelBootSection{}
	collectGrubDefaults(fake, section, schema.SystemTypeRpmOstree)

	assert.Empty(t, section.GrubDefaults, "rpm-ostree systems should skip GRUB defaults")
}

func TestCollectGrubDefaultsTruncation(t *testing.T) {
	fake := NewFakeExecutor(nil).WithFiles(map[string]string{
		"/etc/default/grub": strings.Repeat("A", 600),
	})

	section := &schema.KernelBootSection{}
	collectGrubDefaults(fake, section, schema.SystemTypePackageMode)

	assert.Len(t, section.GrubDefaults, 500, "GRUB defaults should be truncated to 500 chars")
}

// ---------------------------------------------------------------------------
// Full integration: RunKernelBoot
// ---------------------------------------------------------------------------

func TestRunKernelBootIntegration(t *testing.T) {
	cmdline := loadKernelBootFixture(t, "cmdline")
	grub := loadKernelBootFixture(t, "grub_defaults")
	lsmod := loadKernelBootFixture(t, "lsmod_output")
	modulesLoad := loadKernelBootFixture(t, "modules_load.conf")
	modprobe := loadKernelBootFixture(t, "modprobe_options.conf")
	dracut := loadKernelBootFixture(t, "dracut_custom.conf")
	tunedConf := loadKernelBootFixture(t, "tuned_profile.conf")
	locale := loadKernelBootFixture(t, "locale.conf")

	fake := NewFakeExecutor(map[string]ExecResult{
		"lsmod": {Stdout: lsmod, ExitCode: 0},
	}).
		WithFiles(map[string]string{
			"/proc/cmdline":                    cmdline,
			"/etc/default/grub":                grub,
			"/etc/modules-load.d/custom.conf":  modulesLoad,
			"/etc/modprobe.d/nouveau.conf":     modprobe,
			"/etc/dracut.conf.d/custom.conf":   dracut,
			"/etc/tuned/active_profile":        "db-custom",
			"/etc/tuned/db-custom/tuned.conf":  tunedConf,
			"/etc/locale.conf":                 locale,
			"/etc/timezone":                    "America/Chicago",
			"/etc/alternatives/java":           "/usr/lib/jvm/java-17/bin/java",
			"/var/lib/alternatives/java":       "manual\n",
		}).
		WithDirs(map[string][]string{
			"/etc/modules-load.d": {"custom.conf"},
			"/etc/modprobe.d":     {"nouveau.conf"},
			"/etc/dracut.conf.d":  {"custom.conf"},
			"/etc/tuned":          {"active_profile", "db-custom"},
			"/etc/tuned/db-custom": {"tuned.conf"},
			"/etc/alternatives":   {"java"},
		})

	section, warnings, err := RunKernelBoot(fake, KernelBootOptions{
		SystemType: schema.SystemTypePackageMode,
	})
	require.NoError(t, err)

	// Cmdline
	assert.Contains(t, section.Cmdline, "BOOT_IMAGE=")
	assert.Contains(t, section.Cmdline, "root=/dev/mapper/rhel-root")

	// GRUB defaults
	assert.Contains(t, section.GrubDefaults, "GRUB_TIMEOUT=5")

	// Modules-load.d
	require.Len(t, section.ModulesLoadD, 1)
	assert.Equal(t, "etc/modules-load.d/custom.conf", section.ModulesLoadD[0].Path)

	// Modprobe.d
	require.Len(t, section.ModprobeD, 1)
	assert.Contains(t, section.ModprobeD[0].Content, "blacklist nouveau")

	// Dracut
	require.Len(t, section.DracutConf, 1)
	assert.Contains(t, section.DracutConf[0].Content, "add_dracutmodules")

	// Loaded modules from lsmod
	assert.NotEmpty(t, section.LoadedModules)

	// Tuned
	assert.Equal(t, "db-custom", section.TunedActive)
	require.Len(t, section.TunedCustomProfiles, 1)

	// Locale
	require.NotNil(t, section.Locale)
	assert.Equal(t, "en_US.UTF-8", *section.Locale)

	// Timezone
	require.NotNil(t, section.Timezone)
	assert.Equal(t, "America/Chicago", *section.Timezone)

	// Alternatives
	require.Len(t, section.Alternatives, 1)
	assert.Equal(t, "java", section.Alternatives[0].Name)
	assert.Equal(t, "manual", section.Alternatives[0].Status)

	// No warnings expected in this setup (no /usr/lib/sysctl.d directory)
	_ = warnings
}

func TestRunKernelBootMinimalSystem(t *testing.T) {
	// System with almost nothing — should not crash.
	fake := NewFakeExecutor(nil)

	section, warnings, err := RunKernelBoot(fake, KernelBootOptions{
		SystemType: schema.SystemTypePackageMode,
	})
	require.NoError(t, err)
	assert.NotNil(t, section)

	// Should have a warning about /proc/cmdline.
	assert.NotEmpty(t, warnings)

	// All slices should be initialized (not nil).
	assert.NotNil(t, section.SysctlOverrides)
	assert.NotNil(t, section.ModulesLoadD)
	assert.NotNil(t, section.ModprobeD)
	assert.NotNil(t, section.DracutConf)
	assert.NotNil(t, section.LoadedModules)
	assert.NotNil(t, section.NonDefaultModules)
	assert.NotNil(t, section.TunedCustomProfiles)

	// Nullable fields should be nil.
	assert.Nil(t, section.Locale)
	assert.Nil(t, section.Timezone)
}

// strPtr is defined in config.go — reused here.
