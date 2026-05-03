package inspector

import (
	"fmt"
	"strings"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// buildFullFakeExecutor returns a FakeExecutor with enough canned responses
// to let every inspector run without fatal errors. Individual tests can
// override specific commands.
func buildFullFakeExecutor() *FakeExecutor {
	return NewFakeExecutor(map[string]ExecResult{
		// rpm -qa
		"rpm -qa --queryformat %{NAME}\\t%{EPOCH}\\t%{VERSION}\\t%{RELEASE}\\t%{ARCH}\\n": {
			Stdout:   "bash\t(none)\t5.2.15\t3.el9\tx86_64\ncoreutils\t(none)\t8.32\t34.el9\tx86_64\n",
			ExitCode: 0,
		},
		// rpm -Va
		"rpm -Va": {
			Stdout:   "S.5....T.  c /etc/ssh/sshd_config\n",
			ExitCode: 0,
		},
		// rpm -qa --queryformat for owned paths
		"rpm -qa --queryformat %{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\\n": {
			Stdout:   "bash-5.2.15-3.el9.x86_64\ncoreutils-8.32-34.el9.x86_64\n",
			ExitCode: 0,
		},
		// rpm -ql for owned paths
		"rpm -ql bash-5.2.15-3.el9.x86_64": {
			Stdout:   "/etc/profile\n/bin/bash\n",
			ExitCode: 0,
		},
		"rpm -ql coreutils-8.32-34.el9.x86_64": {
			Stdout:   "/etc/profile.d/colorls.sh\n/bin/ls\n",
			ExitCode: 0,
		},
		// rpm -qf for package ownership
		"rpm -qf /etc/ssh/sshd_config": {
			Stdout:   "openssh-server-8.7p1-38.el9.x86_64\n",
			ExitCode: 0,
		},
		// systemctl for services
		"systemctl list-unit-files --type=service --type=socket --no-pager --no-legend": {
			Stdout:   "sshd.service     enabled  enabled\ncrond.service    enabled  enabled\n",
			ExitCode: 0,
		},
		// ip route
		"ip route show": {
			Stdout:   "default via 192.168.1.1 dev eth0\n",
			ExitCode: 0,
		},
		// ip rule
		"ip rule show": {
			Stdout:   "0:\tfrom all lookup local\n",
			ExitCode: 0,
		},
		// findmnt
		"findmnt -rn -o TARGET,SOURCE,FSTYPE,OPTIONS": {
			Stdout:   "/ /dev/sda1 ext4 rw,relatime\n",
			ExitCode: 0,
		},
		// lvs
		"lvs --noheadings --nosuffix -o lv_name,vg_name,lv_size --units g --separator ,": {
			Stdout:   "",
			ExitCode: 0,
		},
		// hostnamectl
		"hostnamectl hostname": {
			Stdout:   "test-host.example.com\n",
			ExitCode: 0,
		},
		// getenforce (SELinux)
		"getenforce": {
			Stdout:   "Enforcing\n",
			ExitCode: 0,
		},
		// sestatus
		"sestatus": {
			Stdout:   "SELinux status:                 enabled\nLoaded policy name:             targeted\n",
			ExitCode: 0,
		},
		// semanage boolean
		"semanage boolean -l -C": {
			Stdout:   "",
			ExitCode: 0,
		},
		// semanage fcontext
		"semanage fcontext -l -C": {
			Stdout:   "",
			ExitCode: 0,
		},
		// semanage port
		"semanage port -l -C": {
			Stdout:   "",
			ExitCode: 0,
		},
		// semodule
		"semodule -l": {
			Stdout:   "",
			ExitCode: 0,
		},
		// lsmod
		"lsmod": {
			Stdout:   "Module                  Size  Used by\next4                  495616  1\n",
			ExitCode: 0,
		},
		// cat /proc/cmdline
		"cat /proc/cmdline": {
			Stdout:   "BOOT_IMAGE=/vmlinuz root=/dev/sda1 ro\n",
			ExitCode: 0,
		},
		// sysctl -a
		"sysctl -a": {
			Stdout:   "net.ipv4.ip_forward = 0\nvm.swappiness = 60\n",
			ExitCode: 0,
		},
		// tuned-adm active
		"tuned-adm active": {
			Stdout:   "Current active profile: virtual-guest\n",
			ExitCode: 0,
		},
		// fips-mode-setup --check
		"fips-mode-setup --check": {
			Stdout:   "FIPS mode is disabled.\n",
			ExitCode: 0,
		},
		// at -l
		"atq": {
			Stdout:   "",
			ExitCode: 0,
		},
		// dnf history
		"dnf history list --reverse": {
			Stdout:   "",
			ExitCode: 0,
		},
		// dnf module
		"dnf module list --installed": {
			Stdout:   "",
			ExitCode: 0,
		},
		// repoquery for source repo
		"dnf repoquery --installed --queryformat %{name}\\t%{from_repo} --quiet": {
			Stdout:   "bash\tbaseos\ncoreutils\tbaseos\n",
			ExitCode: 0,
		},
		// rpm -qa for duplicate detection
		"rpm -qa --queryformat %{NAME}.%{ARCH}\\n": {
			Stdout:   "bash.x86_64\ncoreutils.x86_64\n",
			ExitCode: 0,
		},
		// nmcli for DNS provenance
		"nmcli -t -f NAME connection show --active": {
			Stdout:   "",
			ExitCode: 0,
		},
		// readelf probe
		"readelf -v": {
			Stdout:   "",
			ExitCode: 0,
		},
		// alternatives
		"alternatives --list": {
			Stdout:   "",
			ExitCode: 0,
		},
	}).WithFiles(map[string]string{
		// os-release
		"/etc/os-release": `NAME="Red Hat Enterprise Linux"
VERSION_ID="9.4"
VERSION="9.4 (Plow)"
ID="rhel"
ID_LIKE="fedora"
PRETTY_NAME="Red Hat Enterprise Linux 9.4 (Plow)"
VARIANT_ID="server"`,
		// hostname
		"/etc/hostname": "test-host.example.com\n",
		// fstab
		"/etc/fstab": "/dev/sda1 / ext4 defaults 0 1\n",
		// resolv.conf
		"/etc/resolv.conf": "nameserver 8.8.8.8\n",
		// hosts
		"/etc/hosts": "127.0.0.1 localhost\n",
		// grub
		"/etc/default/grub": "GRUB_TIMEOUT=5\n",
		// passwd
		"/etc/passwd": "root:x:0:0:root:/root:/bin/bash\nuser1:x:1000:1000:Test User:/home/user1:/bin/bash\n",
		// group
		"/etc/group": "root:x:0:\nuser1:x:1000:\n",
		// shadow
		"/etc/shadow": "root:!:19000:0:99999:7:::\nuser1:$6$hash:19000:0:99999:7:::\n",
		// sudoers
		"/etc/sudoers": "root ALL=(ALL) ALL\n",
	}).WithDirs(map[string][]string{
		"/etc":                             {"os-release", "hostname", "fstab", "resolv.conf", "hosts", "passwd", "group", "shadow", "sudoers", "default", "ssh"},
		"/etc/default":                     {"grub"},
		"/etc/ssh":                         {"sshd_config"},
		"/etc/cron.d":                      {},
		"/etc/yum.repos.d":                 {},
		"/etc/pki/rpm-gpg":                 {},
		"/etc/sysctl.d":                    {},
		"/etc/modules-load.d":              {},
		"/etc/modprobe.d":                  {},
		"/etc/dracut.conf.d":               {},
		"/etc/systemd/system":              {},
		"/usr/lib/systemd/system-preset":   {},
		"/etc/systemd/system-preset":       {},
		"/etc/tuned":                       {},
		"/opt":                             {},
		"/srv":                             {},
		"/usr/local":                       {},
		"/usr/local/bin":                   {},
		"/usr/local/sbin":                  {},
		"/etc/firewalld/zones":             {},
		"/etc/audit/rules.d":               {},
		"/etc/pam.d":                       {},
		"/etc/security":                    {},
		"/var/spool/cron":                  {},
	})
}

// ---------------------------------------------------------------------------
// RunAll tests
// ---------------------------------------------------------------------------

func TestRunAll_PopulatesAllSections(t *testing.T) {
	fake := buildFullFakeExecutor()

	snapshot, err := RunAll(fake, InspectOptions{
		Version:    "0.7.0-test",
		NoBaseline: true,
	})
	require.NoError(t, err)

	// Verify schema version
	assert.Equal(t, schema.SchemaVersion, snapshot.SchemaVersion)

	// Verify metadata
	assert.NotEmpty(t, snapshot.Meta["timestamp"])
	assert.Equal(t, "test-host.example.com", snapshot.Meta["hostname"])
	assert.Equal(t, "0.7.0-test", snapshot.Meta["inspectah_version"])

	// Verify os-release
	require.NotNil(t, snapshot.OsRelease)
	assert.Equal(t, "rhel", snapshot.OsRelease.ID)
	assert.Equal(t, "9.4", snapshot.OsRelease.VersionID)

	// Verify system type (no /ostree → package-mode)
	assert.Equal(t, schema.SystemTypePackageMode, snapshot.SystemType)

	// Verify all 11 sections are non-nil (inspectors ran successfully)
	assert.NotNil(t, snapshot.Rpm, "RPM section should be populated")
	assert.NotNil(t, snapshot.Config, "Config section should be populated")
	assert.NotNil(t, snapshot.Services, "Services section should be populated")
	assert.NotNil(t, snapshot.Network, "Network section should be populated")
	assert.NotNil(t, snapshot.Storage, "Storage section should be populated")
	assert.NotNil(t, snapshot.ScheduledTasks, "Scheduled Tasks section should be populated")
	assert.NotNil(t, snapshot.Containers, "Containers section should be populated")
	assert.NotNil(t, snapshot.NonRpmSoftware, "Non-RPM section should be populated")
	assert.NotNil(t, snapshot.KernelBoot, "Kernel/Boot section should be populated")
	assert.NotNil(t, snapshot.Selinux, "SELinux section should be populated")
	assert.NotNil(t, snapshot.UsersGroups, "Users/Groups section should be populated")
}

func TestRunAll_MetadataHostname(t *testing.T) {
	fake := buildFullFakeExecutor()

	snapshot, err := RunAll(fake, InspectOptions{NoBaseline: true})
	require.NoError(t, err)
	assert.Equal(t, "test-host.example.com", snapshot.Meta["hostname"])
}

func TestRunAll_SystemTypeDetection(t *testing.T) {
	// Default fake has no /ostree → package-mode
	fake := buildFullFakeExecutor()

	snapshot, err := RunAll(fake, InspectOptions{NoBaseline: true})
	require.NoError(t, err)
	assert.Equal(t, schema.SystemTypePackageMode, snapshot.SystemType)
}

func TestRunAll_BootcSystemType(t *testing.T) {
	fake := buildFullFakeExecutor()
	fake.WithFiles(map[string]string{
		"/ostree": "", // marker
	})
	fake.commands["bootc status"] = ExecResult{Stdout: "running", ExitCode: 0}

	snapshot, err := RunAll(fake, InspectOptions{NoBaseline: true})
	require.NoError(t, err)
	assert.Equal(t, schema.SystemTypeBootc, snapshot.SystemType)
}

func TestRunAll_UnsupportedHost(t *testing.T) {
	fake := buildFullFakeExecutor()
	fake.files["/etc/os-release"] = `NAME="Red Hat Enterprise Linux"
VERSION_ID="8.9"
ID="rhel"
PRETTY_NAME="Red Hat Enterprise Linux 8.9"`

	_, err := RunAll(fake, InspectOptions{})
	require.Error(t, err)
	assert.Contains(t, err.Error(), "RHEL 8.9")
	assert.Contains(t, err.Error(), "supports")
}

func TestRunAll_CrossVersionWarning(t *testing.T) {
	fake := buildFullFakeExecutor()

	snapshot, err := RunAll(fake, InspectOptions{
		TargetVersion: "10.0",
		NoBaseline:    true,
	})
	require.NoError(t, err)

	// Should have a cross-version warning
	found := false
	for _, w := range snapshot.Warnings {
		if msg, ok := w["message"].(string); ok {
			if strings.Contains(msg, "Cross-major-version") {
				found = true
				break
			}
		}
	}
	assert.True(t, found, "expected cross-major-version warning")
}

func TestRunAll_NoCrossVersionWarningWhenSameMajor(t *testing.T) {
	fake := buildFullFakeExecutor()

	snapshot, err := RunAll(fake, InspectOptions{
		TargetVersion: "9.5",
		NoBaseline:    true,
	})
	require.NoError(t, err)

	for _, w := range snapshot.Warnings {
		if msg, ok := w["message"].(string); ok {
			assert.NotContains(t, msg, "Cross-major-version")
		}
	}
}

func TestRunAll_VersionInMeta(t *testing.T) {
	fake := buildFullFakeExecutor()

	snapshot, err := RunAll(fake, InspectOptions{
		Version:    "0.7.0",
		NoBaseline: true,
	})
	require.NoError(t, err)
	assert.Equal(t, "0.7.0", snapshot.Meta["inspectah_version"])
}

// ---------------------------------------------------------------------------
// Safe execution tests
// ---------------------------------------------------------------------------

func TestSafeRun_PanicRecovery(t *testing.T) {
	result, warnings := safeRun("test", func() (*schema.RpmSection, []Warning, error) {
		panic("test panic")
	})

	assert.Nil(t, result)
	require.Len(t, warnings, 1)
	assert.Contains(t, warnings[0]["message"].(string), "panicked")
}

func TestSafeRun_NormalExecution(t *testing.T) {
	expected := &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "test", Include: true},
		},
	}

	result, warnings := safeRun("test", func() (*schema.RpmSection, []Warning, error) {
		return expected, nil, nil
	})

	assert.Equal(t, expected, result)
	assert.Empty(t, warnings)
}

func TestSafeRun_WithWarnings(t *testing.T) {
	result, warnings := safeRun("test", func() (*schema.RpmSection, []Warning, error) {
		return &schema.RpmSection{}, []Warning{
			makeWarning("test", "some warning"),
		}, nil
	})

	assert.NotNil(t, result)
	require.Len(t, warnings, 1)
	assert.Equal(t, "some warning", warnings[0]["message"])
}

func TestSafeRun_ErrorBecomesWarning(t *testing.T) {
	result, warnings := safeRun("test", func() (*schema.RpmSection, []Warning, error) {
		return nil, nil, fmt.Errorf("something broke")
	})

	assert.Nil(t, result)
	require.Len(t, warnings, 1)
	assert.Contains(t, warnings[0]["message"].(string), "something broke")
}

// ---------------------------------------------------------------------------
// readOsRelease tests
// ---------------------------------------------------------------------------

func TestReadOsRelease(t *testing.T) {
	fake := NewFakeExecutor(nil).WithFiles(map[string]string{
		"/etc/os-release": `NAME="Red Hat Enterprise Linux"
VERSION_ID="9.4"
VERSION="9.4 (Plow)"
ID="rhel"
ID_LIKE="fedora"
PRETTY_NAME="Red Hat Enterprise Linux 9.4 (Plow)"
VARIANT_ID="server"`,
	})

	osRelease := readOsRelease(fake)
	require.NotNil(t, osRelease)
	assert.Equal(t, "Red Hat Enterprise Linux", osRelease.Name)
	assert.Equal(t, "9.4", osRelease.VersionID)
	assert.Equal(t, "rhel", osRelease.ID)
	assert.Equal(t, "fedora", osRelease.IDLike)
	assert.Equal(t, "server", osRelease.VariantID)
}

func TestReadOsRelease_NotFound(t *testing.T) {
	fake := NewFakeExecutor(nil)
	assert.Nil(t, readOsRelease(fake))
}

// ---------------------------------------------------------------------------
// validateSupportedHost tests
// ---------------------------------------------------------------------------

func TestValidateSupportedHost(t *testing.T) {
	tests := []struct {
		name      string
		osRelease *schema.OsRelease
		wantErr   bool
	}{
		{
			name:      "nil os-release",
			osRelease: nil,
			wantErr:   false,
		},
		{
			name:      "RHEL 9 supported",
			osRelease: &schema.OsRelease{ID: "rhel", VersionID: "9.4"},
			wantErr:   false,
		},
		{
			name:      "RHEL 10 supported",
			osRelease: &schema.OsRelease{ID: "rhel", VersionID: "10.0"},
			wantErr:   false,
		},
		{
			name:      "RHEL 8 unsupported",
			osRelease: &schema.OsRelease{ID: "rhel", VersionID: "8.9"},
			wantErr:   true,
		},
		{
			name:      "CentOS Stream 9 supported",
			osRelease: &schema.OsRelease{ID: "centos", VersionID: "9"},
			wantErr:   false,
		},
		{
			name:      "CentOS 7 unsupported",
			osRelease: &schema.OsRelease{ID: "centos", VersionID: "7"},
			wantErr:   true,
		},
		{
			name:      "Fedora always supported",
			osRelease: &schema.OsRelease{ID: "fedora", VersionID: "41"},
			wantErr:   false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			msg := validateSupportedHost(tt.osRelease)
			if tt.wantErr {
				assert.NotEmpty(t, msg)
			} else {
				assert.Empty(t, msg)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// detectSystemTypeLocal tests
// ---------------------------------------------------------------------------

func TestDetectSystemTypeLocal_PackageMode(t *testing.T) {
	fake := NewFakeExecutor(nil)
	st, err := detectSystemTypeLocal(fake)
	require.NoError(t, err)
	assert.Equal(t, schema.SystemTypePackageMode, st)
}

func TestDetectSystemTypeLocal_Bootc(t *testing.T) {
	fake := NewFakeExecutor(map[string]ExecResult{
		"bootc status": {Stdout: "running", ExitCode: 0},
	}).WithFiles(map[string]string{"/ostree": ""})

	st, err := detectSystemTypeLocal(fake)
	require.NoError(t, err)
	assert.Equal(t, schema.SystemTypeBootc, st)
}

func TestDetectSystemTypeLocal_RpmOstree(t *testing.T) {
	fake := NewFakeExecutor(map[string]ExecResult{
		"bootc status":      {ExitCode: 127},
		"rpm-ostree status": {Stdout: "idle", ExitCode: 0},
	}).WithFiles(map[string]string{"/ostree": ""})

	st, err := detectSystemTypeLocal(fake)
	require.NoError(t, err)
	assert.Equal(t, schema.SystemTypeRpmOstree, st)
}

func TestDetectSystemTypeLocal_OstreeUnknown(t *testing.T) {
	fake := NewFakeExecutor(map[string]ExecResult{
		"bootc status":      {ExitCode: 127},
		"rpm-ostree status": {ExitCode: 127},
	}).WithFiles(map[string]string{"/ostree": ""})

	_, err := detectSystemTypeLocal(fake)
	require.Error(t, err)
	assert.Contains(t, err.Error(), "ostree")
}

// ---------------------------------------------------------------------------
// populateHostname tests
// ---------------------------------------------------------------------------

func TestPopulateHostname_FromFile(t *testing.T) {
	fake := NewFakeExecutor(nil).WithFiles(map[string]string{
		"/etc/hostname": "my-server\n",
	})
	snap := schema.NewSnapshot()
	populateHostname(fake, snap)
	assert.Equal(t, "my-server", snap.Meta["hostname"])
}

func TestPopulateHostname_FromHostnamectl(t *testing.T) {
	fake := NewFakeExecutor(map[string]ExecResult{
		"hostnamectl hostname": {Stdout: "ctl-host\n", ExitCode: 0},
	})
	snap := schema.NewSnapshot()
	populateHostname(fake, snap)
	assert.Equal(t, "ctl-host", snap.Meta["hostname"])
}

func TestPopulateHostname_NoHostname(t *testing.T) {
	fake := NewFakeExecutor(map[string]ExecResult{
		"hostnamectl hostname": {ExitCode: 1},
	})
	snap := schema.NewSnapshot()
	populateHostname(fake, snap)
	_, hasHostname := snap.Meta["hostname"]
	assert.False(t, hasHostname)
}
