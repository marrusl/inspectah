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

func loadServiceFixture(t *testing.T, name string) string {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("testdata", "services", name))
	require.NoError(t, err, "loading fixture %s", name)
	return string(data)
}

// ---------------------------------------------------------------------------
// systemctl output parsing
// ---------------------------------------------------------------------------

func TestParseSystemctlListUnitFiles(t *testing.T) {
	input := loadServiceFixture(t, "systemctl-list-unit-files.txt")
	units := parseSystemctlListUnitFiles(input)

	assert.Equal(t, "enabled", units["sshd.service"])
	assert.Equal(t, "enabled", units["httpd.service"])
	assert.Equal(t, "disabled", units["postfix.service"])
	assert.Equal(t, "masked", units["firewalld.service"])
	assert.Equal(t, "static", units["dbus.service"])
	assert.Equal(t, "enabled", units["foobar.timer"])
	assert.Equal(t, "disabled", units["cleanup.timer"])
	assert.GreaterOrEqual(t, len(units), 18)
}

func TestParseSystemctlEmptyOutput(t *testing.T) {
	units := parseSystemctlListUnitFiles("")
	assert.Empty(t, units)
}

func TestParseSystemctlMalformedLines(t *testing.T) {
	input := "sshd.service enabled\njunk\nhttpd.service disabled\n"
	units := parseSystemctlListUnitFiles(input)
	assert.Len(t, units, 2)
	assert.Equal(t, "enabled", units["sshd.service"])
	assert.Equal(t, "disabled", units["httpd.service"])
}

// ---------------------------------------------------------------------------
// Preset parsing
// ---------------------------------------------------------------------------

func TestParsePresetLines(t *testing.T) {
	tests := []struct {
		name           string
		input          string
		wantEnabled    []string
		wantDisabled   []string
		wantDisableAll bool
	}{
		{
			name:         "basic presets",
			input:        "enable sshd.service\ndisable cups.service\n",
			wantEnabled:  []string{"sshd.service"},
			wantDisabled: []string{"cups.service"},
		},
		{
			name:           "disable all",
			input:          "enable sshd.service\ndisable *\n",
			wantEnabled:    []string{"sshd.service"},
			wantDisableAll: true,
		},
		{
			name:         "comments and empty lines",
			input:        "# comment\n\nenable sshd.service\n  # another comment\ndisable cups.service\n",
			wantEnabled:  []string{"sshd.service"},
			wantDisabled: []string{"cups.service"},
		},
		{
			name:        "first match wins for duplicates",
			input:       "enable sshd.service\ndisable sshd.service\n",
			wantEnabled: []string{"sshd.service"},
		},
		{
			name:  "empty input",
			input: "",
		},
		{
			name:         "timer units",
			input:        "enable fstrim.timer\ndisable dnf-makecache.timer\n",
			wantEnabled:  []string{"fstrim.timer"},
			wantDisabled: []string{"dnf-makecache.timer"},
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			enabled, disabled, hasDisableAll, _ := parsePresetLines(
				splitLines(tc.input),
			)
			for _, u := range tc.wantEnabled {
				assert.True(t, enabled[u], "expected %s in enabled set", u)
			}
			for _, u := range tc.wantDisabled {
				assert.True(t, disabled[u], "expected %s in disabled set", u)
			}
			assert.Equal(t, tc.wantDisableAll, hasDisableAll)
		})
	}
}

func TestParsePresetGlobRules(t *testing.T) {
	input := "enable sshd.service\nenable systemd-*.service\ndisable *\n"
	_, _, _, globs := parsePresetLines(splitLines(input))

	require.Len(t, globs, 2)
	assert.Equal(t, "enable", globs[0].action)
	assert.Equal(t, "systemd-*.service", globs[0].pattern)
	assert.Equal(t, "disable", globs[1].action)
	assert.Equal(t, "*", globs[1].pattern)
}

// ---------------------------------------------------------------------------
// Default state resolution
// ---------------------------------------------------------------------------

func TestResolveDefaultState(t *testing.T) {
	enabled := map[string]bool{"sshd.service": true, "crond.service": true}
	disabled := map[string]bool{"cups.service": true}
	globsWithDisableAll := []presetGlobRule{
		{action: "enable", pattern: "systemd-*.service"},
		{action: "disable", pattern: "*"},
	}
	globsWithoutDisableAll := []presetGlobRule{
		{action: "enable", pattern: "systemd-*.service"},
	}

	tests := []struct {
		name          string
		unit          string
		hasDisableAll bool
		globs         []presetGlobRule
		want          string
	}{
		{name: "explicitly enabled", unit: "sshd.service", globs: globsWithDisableAll, want: "enabled"},
		{name: "explicitly disabled", unit: "cups.service", globs: globsWithDisableAll, want: "disabled"},
		{name: "glob enable match", unit: "systemd-resolved.service", globs: globsWithDisableAll, want: "enabled"},
		{name: "glob disable all fallback", unit: "unknown.service", hasDisableAll: true, globs: globsWithDisableAll, want: "disabled"},
		{name: "no match no disable-all", unit: "unknown.service", hasDisableAll: false, globs: globsWithoutDisableAll, want: "unknown"},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			got := resolveDefaultState(tc.unit, enabled, disabled, tc.hasDisableAll, tc.globs)
			assert.Equal(t, tc.want, got)
		})
	}
}

// ---------------------------------------------------------------------------
// Glob matching
// ---------------------------------------------------------------------------

func TestGlobMatch(t *testing.T) {
	tests := []struct {
		pattern string
		name    string
		want    bool
	}{
		{"*", "anything.service", true},
		{"sshd.service", "sshd.service", true},
		{"sshd.service", "httpd.service", false},
		{"systemd-*.service", "systemd-resolved.service", true},
		{"systemd-*.service", "sshd.service", false},
		{"*.timer", "fstrim.timer", true},
		{"*.timer", "sshd.service", false},
		{"??d.service", "ssd.service", true},
		{"??d.service", "sshd.service", false},
	}

	for _, tc := range tests {
		t.Run(tc.pattern+"_"+tc.name, func(t *testing.T) {
			got := globMatch(tc.pattern, tc.name)
			assert.Equal(t, tc.want, got)
		})
	}
}

// ---------------------------------------------------------------------------
// State change comparison
// ---------------------------------------------------------------------------

func TestRunServicesStateChanges(t *testing.T) {
	systemctlOutput := loadServiceFixture(t, "systemctl-list-unit-files.txt")
	presetText := loadServiceFixture(t, "presets.txt")

	exec := NewFakeExecutor(map[string]ExecResult{
		"systemctl list-unit-files --no-pager --no-legend": {
			Stdout: systemctlOutput, ExitCode: 0,
		},
		// rpm -qf batch query — fail so we skip.
		"rpm -qf --queryformat %{NAME}\n /usr/lib/systemd/system/httpd.service /usr/lib/systemd/system/postfix.service /usr/lib/systemd/system/firewalld.service /usr/lib/systemd/system/foobar.timer /usr/lib/systemd/system/cleanup.timer": {
			ExitCode: 1,
		},
	}).WithDirs(map[string][]string{
		"/etc/systemd/system": {},
	})

	section, warnings, err := RunServices(exec, ServiceOptions{
		BaseImagePresetText: presetText,
	})

	require.NoError(t, err)
	require.NotNil(t, section)

	// httpd.service: currently enabled, preset says disabled → action=enable
	assertStateChange(t, section, "httpd.service", "enabled", "disabled", "enable")

	// postfix.service: currently disabled, preset says enabled → action=disable
	assertStateChange(t, section, "postfix.service", "disabled", "enabled", "disable")

	// firewalld.service: currently masked → action=mask
	assertStateChange(t, section, "firewalld.service", "masked", "enabled", "mask")

	// sshd.service: currently enabled, preset says enabled → action=unchanged
	assertStateChange(t, section, "sshd.service", "enabled", "enabled", "unchanged")

	// cups.service: currently disabled, preset says disabled → action=unchanged
	assertStateChange(t, section, "cups.service", "disabled", "disabled", "unchanged")

	// Timer units
	assertStateChange(t, section, "foobar.timer", "enabled", "disabled", "enable")
	assertStateChange(t, section, "cleanup.timer", "disabled", "enabled", "disable")

	// EnabledUnits should contain httpd and foobar.timer
	assert.Contains(t, section.EnabledUnits, "httpd.service")
	assert.Contains(t, section.EnabledUnits, "foobar.timer")

	// DisabledUnits should contain postfix and cleanup.timer
	assert.Contains(t, section.DisabledUnits, "postfix.service")
	assert.Contains(t, section.DisabledUnits, "cleanup.timer")

	// With base image preset, no warning expected.
	assert.Empty(t, warnings)
}

func TestRunServicesNoPresets(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"systemctl list-unit-files --no-pager --no-legend": {
			Stdout:   "sshd.service enabled\nhttpd.service disabled\n",
			ExitCode: 0,
		},
	}).WithDirs(map[string][]string{
		"/etc/systemd/system":        {},
		"/etc/systemd/system-preset": {},
		"/usr/lib/systemd/system-preset": {},
	})

	section, warnings, err := RunServices(exec, ServiceOptions{})

	require.NoError(t, err)
	require.NotNil(t, section)

	// Without base image presets, should produce a warning.
	require.Len(t, warnings, 1)
	assert.Contains(t, warnings[0]["message"], "No base image service presets")
}

func TestRunServicesEmptyOutput(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"systemctl list-unit-files --no-pager --no-legend": {
			Stdout: "", ExitCode: 1,
		},
	}).WithDirs(map[string][]string{})

	section, _, err := RunServices(exec, ServiceOptions{})
	require.NoError(t, err)
	assert.Empty(t, section.StateChanges)
	assert.Empty(t, section.EnabledUnits)
	assert.Empty(t, section.DisabledUnits)
}

// ---------------------------------------------------------------------------
// Drop-in override detection
// ---------------------------------------------------------------------------

func TestRunServicesDropIns(t *testing.T) {
	overrideContent := loadServiceFixture(t, "httpd-override.conf")

	exec := NewFakeExecutor(map[string]ExecResult{
		"systemctl list-unit-files --no-pager --no-legend": {
			Stdout: "httpd.service enabled\n", ExitCode: 0,
		},
	}).WithDirs(map[string][]string{
		"/etc/systemd/system":              {"httpd.service.d"},
		"/etc/systemd/system/httpd.service.d": {"override.conf"},
		"/etc/systemd/system-preset":       {},
		"/usr/lib/systemd/system-preset":   {},
	}).WithFiles(map[string]string{
		"/etc/systemd/system/httpd.service.d/override.conf": overrideContent,
	})

	section, _, err := RunServices(exec, ServiceOptions{
		BaseImagePresetText: "disable *\n",
	})

	require.NoError(t, err)
	require.Len(t, section.DropIns, 1)
	assert.Equal(t, "httpd.service", section.DropIns[0].Unit)
	assert.Equal(t, "etc/systemd/system/httpd.service.d/override.conf", section.DropIns[0].Path)
	assert.Contains(t, section.DropIns[0].Content, "LimitNOFILE=65536")
}

func TestRunServicesTimerDropIns(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"systemctl list-unit-files --no-pager --no-legend": {
			Stdout: "fstrim.timer enabled\n", ExitCode: 0,
		},
	}).WithDirs(map[string][]string{
		"/etc/systemd/system":            {"fstrim.timer.d"},
		"/etc/systemd/system/fstrim.timer.d": {"custom.conf"},
		"/etc/systemd/system-preset":     {},
		"/usr/lib/systemd/system-preset": {},
	}).WithFiles(map[string]string{
		"/etc/systemd/system/fstrim.timer.d/custom.conf": "[Timer]\nOnCalendar=weekly\n",
	})

	section, _, err := RunServices(exec, ServiceOptions{
		BaseImagePresetText: "enable fstrim.timer\n",
	})

	require.NoError(t, err)
	require.Len(t, section.DropIns, 1)
	assert.Equal(t, "fstrim.timer", section.DropIns[0].Unit)
	assert.Contains(t, section.DropIns[0].Content, "OnCalendar=weekly")
}

func TestRunServicesNoDropInDir(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"systemctl list-unit-files --no-pager --no-legend": {
			Stdout: "sshd.service enabled\n", ExitCode: 0,
		},
	}).WithDirs(map[string][]string{
		"/etc/systemd/system-preset":     {},
		"/usr/lib/systemd/system-preset": {},
	})

	section, _, err := RunServices(exec, ServiceOptions{
		BaseImagePresetText: "enable sshd.service\n",
	})

	require.NoError(t, err)
	assert.Empty(t, section.DropIns)
}

// ---------------------------------------------------------------------------
// Owning package resolution
// ---------------------------------------------------------------------------

func TestRunServicesOwningPackages(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"systemctl list-unit-files --no-pager --no-legend": {
			Stdout: "httpd.service enabled\nsshd.service enabled\n", ExitCode: 0,
		},
		"rpm -qf --queryformat %{NAME}\n /usr/lib/systemd/system/httpd.service": {
			Stdout: "httpd\n", ExitCode: 0,
		},
	}).WithDirs(map[string][]string{
		"/etc/systemd/system": {},
	})

	section, _, err := RunServices(exec, ServiceOptions{
		BaseImagePresetText: "enable sshd.service\ndisable httpd.service\n",
	})

	require.NoError(t, err)

	// httpd.service changed state → should have owning package resolved.
	for _, sc := range section.StateChanges {
		if sc.Unit == "httpd.service" {
			require.NotNil(t, sc.OwningPackage, "httpd.service should have owning package")
			assert.Equal(t, "httpd", *sc.OwningPackage)
		}
	}
}

// ---------------------------------------------------------------------------
// Filesystem scan fallback
// ---------------------------------------------------------------------------

func TestScanUnitFilesFromFS(t *testing.T) {
	exec := NewFakeExecutor(nil).WithDirs(map[string][]string{
		"/etc/systemd/system":                        {"multi-user.target.wants", "sshd.service"},
		"/etc/systemd/system/multi-user.target.wants": {"sshd.service", "httpd.service"},
		"/usr/lib/systemd/system":                    {"sshd.service", "httpd.service", "cups.service", "fstrim.timer"},
	}).WithFiles(map[string]string{
		// sshd.service has empty content in admin dir → masked
		"/etc/systemd/system/sshd.service": "",
		// vendor units with [Install] section
		"/usr/lib/systemd/system/sshd.service":  "[Unit]\nDescription=OpenSSH\n[Service]\nExecStart=/usr/sbin/sshd\n[Install]\nWantedBy=multi-user.target\n",
		"/usr/lib/systemd/system/httpd.service":  "[Unit]\nDescription=Apache\n[Service]\nExecStart=/usr/sbin/httpd\n[Install]\nWantedBy=multi-user.target\n",
		"/usr/lib/systemd/system/cups.service":   "[Unit]\nDescription=CUPS\n[Service]\nExecStart=/usr/sbin/cupsd\n[Install]\nWantedBy=multi-user.target\n",
		"/usr/lib/systemd/system/fstrim.timer":   "[Unit]\nDescription=fstrim\n[Timer]\nOnCalendar=weekly\n",
	})

	units := scanUnitFilesFromFS(exec)

	// sshd.service: in admin dir with empty content → masked
	assert.Equal(t, "masked", units["sshd.service"])
	// httpd.service: in .wants/ → enabled
	assert.Equal(t, "enabled", units["httpd.service"])
	// cups.service: has [Install] but not in .wants/ → disabled
	assert.Equal(t, "disabled", units["cups.service"])
	// fstrim.timer: no [Install] → static
	assert.Equal(t, "static", units["fstrim.timer"])
}

// ---------------------------------------------------------------------------
// Non-root host path
// ---------------------------------------------------------------------------

func TestRunServicesNonRootHost(t *testing.T) {
	exec := &fakeExecutorWithRoot{
		FakeExecutor: NewFakeExecutor(map[string]ExecResult{
			"systemctl --root /sysroot list-unit-files --no-pager --no-legend": {
				Stdout: "sshd.service enabled\n", ExitCode: 0,
			},
		}).WithDirs(map[string][]string{
			"/etc/systemd/system": {},
		}),
		root: "/sysroot",
	}

	section, _, err := RunServices(exec, ServiceOptions{
		BaseImagePresetText: "enable sshd.service\n",
	})

	require.NoError(t, err)
	assert.Len(t, section.StateChanges, 1)
	assert.Equal(t, "unchanged", section.StateChanges[0].Action)
}

// fakeExecutorWithRoot overrides HostRoot for non-root testing.
type fakeExecutorWithRoot struct {
	*FakeExecutor
	root string
}

func (f *fakeExecutorWithRoot) HostRoot() string { return f.root }

// ---------------------------------------------------------------------------
// Preset file reading from host
// ---------------------------------------------------------------------------

func TestParsePresetFilesFromHost(t *testing.T) {
	exec := NewFakeExecutor(nil).WithDirs(map[string][]string{
		"/etc/systemd/system-preset":     {"90-custom.preset"},
		"/usr/lib/systemd/system-preset": {"90-default.preset"},
	}).WithFiles(map[string]string{
		"/etc/systemd/system-preset/90-custom.preset":     "enable custom-app.service\n",
		"/usr/lib/systemd/system-preset/90-default.preset": "enable sshd.service\ndisable cups.service\ndisable *\n",
	})

	enabled, disabled, hasDisableAll, _ := parsePresetFilesFromHost(exec)

	assert.True(t, enabled["custom-app.service"])
	assert.True(t, enabled["sshd.service"])
	assert.True(t, disabled["cups.service"])
	assert.True(t, hasDisableAll)
}

func TestParsePresetFilesSkipsNonPreset(t *testing.T) {
	exec := NewFakeExecutor(nil).WithDirs(map[string][]string{
		"/usr/lib/systemd/system-preset": {"90-default.preset", "README.txt"},
	}).WithFiles(map[string]string{
		"/usr/lib/systemd/system-preset/90-default.preset": "enable sshd.service\n",
		"/usr/lib/systemd/system-preset/README.txt":        "This is not a preset file\n",
	})

	enabled, _, _, _ := parsePresetFilesFromHost(exec)

	assert.True(t, enabled["sshd.service"])
	assert.Len(t, enabled, 1)
}

// ---------------------------------------------------------------------------
// Full integration: disable-all preset
// ---------------------------------------------------------------------------

func TestRunServicesDisableAllPreset(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"systemctl list-unit-files --no-pager --no-legend": {
			Stdout: "sshd.service enabled\nhttpd.service enabled\ncustom.service enabled\n",
			ExitCode: 0,
		},
	}).WithDirs(map[string][]string{
		"/etc/systemd/system": {},
	})

	section, _, err := RunServices(exec, ServiceOptions{
		BaseImagePresetText: "enable sshd.service\ndisable *\n",
	})

	require.NoError(t, err)

	// sshd: enabled, preset=enabled → unchanged
	assertStateChange(t, section, "sshd.service", "enabled", "enabled", "unchanged")
	// httpd: enabled, preset=disabled (via disable *) → enable
	assertStateChange(t, section, "httpd.service", "enabled", "disabled", "enable")
	// custom: enabled, preset=disabled (via disable *) → enable
	assertStateChange(t, section, "custom.service", "enabled", "disabled", "enable")
}

// ---------------------------------------------------------------------------
// Masked units
// ---------------------------------------------------------------------------

func TestRunServicesMaskedUnit(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"systemctl list-unit-files --no-pager --no-legend": {
			Stdout: "postfix.service masked\n", ExitCode: 0,
		},
	}).WithDirs(map[string][]string{
		"/etc/systemd/system": {},
	})

	section, _, err := RunServices(exec, ServiceOptions{
		BaseImagePresetText: "enable postfix.service\n",
	})

	require.NoError(t, err)
	assertStateChange(t, section, "postfix.service", "masked", "enabled", "mask")
}

// ---------------------------------------------------------------------------
// Static units (skipped for non-service/timer)
// ---------------------------------------------------------------------------

func TestRunServicesSkipsNonServiceTimer(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"systemctl list-unit-files --no-pager --no-legend": {
			Stdout: "sshd.service enabled\ndbus.socket enabled\n", ExitCode: 0,
		},
	}).WithDirs(map[string][]string{
		"/etc/systemd/system": {},
	})

	section, _, err := RunServices(exec, ServiceOptions{
		BaseImagePresetText: "enable sshd.service\n",
	})

	require.NoError(t, err)
	// dbus.socket should be skipped — only .service and .timer are tracked.
	for _, sc := range section.StateChanges {
		assert.NotEqual(t, "dbus.socket", sc.Unit)
	}
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

func assertStateChange(t *testing.T, section *schema.ServiceSection, unit, currentState, defaultState, action string) {
	t.Helper()
	for _, sc := range section.StateChanges {
		if sc.Unit == unit {
			assert.Equal(t, currentState, sc.CurrentState, "unit %s: current_state", unit)
			assert.Equal(t, defaultState, sc.DefaultState, "unit %s: default_state", unit)
			assert.Equal(t, action, sc.Action, "unit %s: action", unit)
			return
		}
	}
	t.Errorf("unit %s not found in state_changes", unit)
}

func splitLines(s string) []string {
	if s == "" {
		return nil
	}
	return append([]string{}, splitByNewline(s)...)
}

func splitByNewline(s string) []string {
	var lines []string
	for _, line := range append([]string{}, splitN(s)...) {
		lines = append(lines, line)
	}
	return lines
}

func splitN(s string) []string {
	result := []string{}
	start := 0
	for i := 0; i < len(s); i++ {
		if s[i] == '\n' {
			result = append(result, s[start:i])
			start = i + 1
		}
	}
	if start < len(s) {
		result = append(result, s[start:])
	}
	return result
}
