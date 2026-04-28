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

func loadStorageFixture(t *testing.T, name string) string {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("testdata", "storage", name))
	require.NoError(t, err, "loading fixture %s", name)
	return string(data)
}

// ---------------------------------------------------------------------------
// fstab parsing
// ---------------------------------------------------------------------------

func TestParseFstab(t *testing.T) {
	tests := []struct {
		name         string
		fstab        string
		wantEntries  int
		wantCreds    int
		checkEntry   func(t *testing.T, entries []schema.FstabEntry)
		checkCreds   func(t *testing.T, creds []schema.CredentialRef)
	}{
		{
			name: "standard entries",
			fstab: `UUID=abcd-1234 / xfs defaults 0 0
UUID=efgh-5678 /boot ext4 defaults 1 2
/dev/mapper/data-home /home xfs defaults,noatime 0 0`,
			wantEntries: 3,
			checkEntry: func(t *testing.T, entries []schema.FstabEntry) {
				assert.Equal(t, "UUID=abcd-1234", entries[0].Device)
				assert.Equal(t, "/", entries[0].MountPoint)
				assert.Equal(t, "xfs", entries[0].Fstype)
				assert.Equal(t, "defaults", entries[0].Options)

				assert.Equal(t, "/home", entries[2].MountPoint)
				assert.Equal(t, "defaults,noatime", entries[2].Options)
			},
		},
		{
			name: "comments and blank lines",
			fstab: `# This is a comment

UUID=1234 / xfs defaults 0 0
# Another comment
UUID=5678 /boot ext4 defaults 0 0

`,
			wantEntries: 2,
		},
		{
			name: "bind mount",
			fstab: `/opt/app /srv/app none bind 0 0`,
			wantEntries: 1,
			checkEntry: func(t *testing.T, entries []schema.FstabEntry) {
				assert.Equal(t, "none", entries[0].Fstype)
				assert.Equal(t, "bind", entries[0].Options)
			},
		},
		{
			name: "NFS mount",
			fstab: `nfs-server:/export/data /mnt/data nfs rw,hard,intr 0 0`,
			wantEntries: 1,
			checkEntry: func(t *testing.T, entries []schema.FstabEntry) {
				assert.Equal(t, "nfs-server:/export/data", entries[0].Device)
				assert.Equal(t, "nfs", entries[0].Fstype)
			},
		},
		{
			name: "CIFS with credentials",
			fstab: `//fileserver/share /mnt/share cifs credentials=/etc/samba/creds.txt,uid=1000 0 0`,
			wantEntries: 1,
			wantCreds:   1,
			checkCreds: func(t *testing.T, creds []schema.CredentialRef) {
				assert.Equal(t, "/mnt/share", creds[0].MountPoint)
				assert.Equal(t, "/etc/samba/creds.txt", creds[0].CredentialPath)
				assert.Equal(t, "fstab", creds[0].Source)
			},
		},
		{
			name: "multiple credential options",
			fstab: `//server1/a /mnt/a cifs credentials=/etc/c1.txt 0 0
nfs:/b /mnt/b nfs secretfile=/etc/nfs.key 0 0`,
			wantEntries: 2,
			wantCreds:   2,
			checkCreds: func(t *testing.T, creds []schema.CredentialRef) {
				assert.Equal(t, "/etc/c1.txt", creds[0].CredentialPath)
				assert.Equal(t, "/etc/nfs.key", creds[1].CredentialPath)
			},
		},
		{
			name: "password_file option",
			fstab: `//srv/x /mnt/x cifs password_file=/etc/pw 0 0`,
			wantEntries: 1,
			wantCreds:   1,
			checkCreds: func(t *testing.T, creds []schema.CredentialRef) {
				assert.Equal(t, "/etc/pw", creds[0].CredentialPath)
			},
		},
		{
			name: "minimal entry without options",
			fstab: `proc /proc proc`,
			wantEntries: 1,
			checkEntry: func(t *testing.T, entries []schema.FstabEntry) {
				assert.Equal(t, "proc", entries[0].Device)
				assert.Equal(t, "/proc", entries[0].MountPoint)
				assert.Equal(t, "proc", entries[0].Fstype)
				assert.Equal(t, "", entries[0].Options)
			},
		},
		{
			name:        "empty fstab",
			fstab:       "",
			wantEntries: 0,
		},
		{
			name:        "only comments",
			fstab:       "# comment one\n# comment two\n",
			wantEntries: 0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			exec := NewFakeExecutor(nil).WithFiles(map[string]string{
				"/etc/fstab": tt.fstab,
			})
			section, _, err := RunStorage(exec, StorageOptions{})
			require.NoError(t, err)
			assert.Len(t, section.FstabEntries, tt.wantEntries)

			wantCreds := tt.wantCreds
			assert.Len(t, section.CredentialRefs, wantCreds)

			if tt.checkEntry != nil {
				tt.checkEntry(t, section.FstabEntries)
			}
			if tt.checkCreds != nil {
				tt.checkCreds(t, section.CredentialRefs)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// findmnt parsing
// ---------------------------------------------------------------------------

func TestParseFindmnt(t *testing.T) {
	findmntJSON := loadStorageFixture(t, "findmnt.json")
	exec := NewFakeExecutor(map[string]ExecResult{
		"findmnt --json --real": {Stdout: findmntJSON, ExitCode: 0},
	})

	section, _, err := RunStorage(exec, StorageOptions{})
	require.NoError(t, err)
	assert.Len(t, section.MountPoints, 4)

	// Verify first mount
	assert.Equal(t, "/", section.MountPoints[0].Target)
	assert.Equal(t, "/dev/mapper/root", section.MountPoints[0].Source)
	assert.Equal(t, "xfs", section.MountPoints[0].Fstype)

	// Verify NFS mount
	assert.Equal(t, "/mnt/data", section.MountPoints[3].Target)
	assert.Equal(t, "nfs4", section.MountPoints[3].Fstype)
}

func TestParseFindmntFailure(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"findmnt --json --real": {Stdout: "", ExitCode: 1},
	})

	section, _, err := RunStorage(exec, StorageOptions{})
	require.NoError(t, err)
	assert.Empty(t, section.MountPoints)
}

func TestParseFindmntInvalidJSON(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"findmnt --json --real": {Stdout: "not json", ExitCode: 0},
	})

	section, _, err := RunStorage(exec, StorageOptions{})
	require.NoError(t, err)
	assert.Empty(t, section.MountPoints)
}

// ---------------------------------------------------------------------------
// LVM parsing
// ---------------------------------------------------------------------------

func TestParseLVM(t *testing.T) {
	lvsJSON := loadStorageFixture(t, "lvs.json")
	exec := NewFakeExecutor(map[string]ExecResult{
		"lvs --reportformat json --units g": {Stdout: lvsJSON, ExitCode: 0},
	})

	section, _, err := RunStorage(exec, StorageOptions{})
	require.NoError(t, err)
	require.Len(t, section.LvmInfo, 3)

	assert.Equal(t, "root", section.LvmInfo[0].LvName)
	assert.Equal(t, "rhel", section.LvmInfo[0].VgName)
	assert.Equal(t, "50.00g", section.LvmInfo[0].LvSize)

	assert.Equal(t, "home", section.LvmInfo[1].LvName)
	assert.Equal(t, "data", section.LvmInfo[1].VgName)
}

func TestParseLVMFailure(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"lvs --reportformat json --units g": {ExitCode: 5},
	})

	section, _, err := RunStorage(exec, StorageOptions{})
	require.NoError(t, err)
	assert.Empty(t, section.LvmInfo)
}

func TestParseLVMEmptyReport(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"lvs --reportformat json --units g": {
			Stdout: `{"report": []}`, ExitCode: 0,
		},
	})

	section, _, err := RunStorage(exec, StorageOptions{})
	require.NoError(t, err)
	assert.Empty(t, section.LvmInfo)
}

// ---------------------------------------------------------------------------
// iSCSI / multipath / LVM config / dm-crypt detection
// ---------------------------------------------------------------------------

func TestDetectISCSI(t *testing.T) {
	exec := NewFakeExecutor(nil).WithFiles(map[string]string{
		"/etc/iscsi/initiatorname.iscsi": "InitiatorName=iqn.2024-01.com.example:host",
	})

	section, _, err := RunStorage(exec, StorageOptions{})
	require.NoError(t, err)

	found := false
	for _, m := range section.MountPoints {
		if m.Target == "iSCSI" {
			found = true
			assert.Equal(t, "iscsi", m.Fstype)
		}
	}
	assert.True(t, found, "expected iSCSI mount point")
}

func TestDetectMultipath(t *testing.T) {
	exec := NewFakeExecutor(nil).WithFiles(map[string]string{
		"/etc/multipath.conf": "defaults {\n    user_friendly_names yes\n}",
	})

	section, _, err := RunStorage(exec, StorageOptions{})
	require.NoError(t, err)

	found := false
	for _, m := range section.MountPoints {
		if m.Target == "multipath" {
			found = true
			assert.Equal(t, "dm-multipath", m.Fstype)
		}
	}
	assert.True(t, found, "expected multipath mount point")
}

func TestDetectLVMConfig(t *testing.T) {
	exec := NewFakeExecutor(nil).WithFiles(map[string]string{
		"/etc/lvm/lvm.conf": "global { ... }",
	}).WithDirs(map[string][]string{
		"/etc/lvm/profile": {"custom.profile", "another.profile"},
	})

	section, _, err := RunStorage(exec, StorageOptions{})
	require.NoError(t, err)

	var targets []string
	for _, m := range section.MountPoints {
		if m.Fstype == "lvm" {
			targets = append(targets, m.Target)
		}
	}
	assert.Contains(t, targets, "lvm-config")
	assert.Contains(t, targets, "lvm-profile (custom.profile)")
	assert.Contains(t, targets, "lvm-profile (another.profile)")
}

func TestDetectDMCrypt(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"dmsetup table --target crypt": {
			Stdout:   "luks-abcd: 0 1048576 crypt aes-xts-plain64\nluks-efgh: 0 2097152 crypt aes-xts-plain64\n",
			ExitCode: 0,
		},
	})

	section, _, err := RunStorage(exec, StorageOptions{})
	require.NoError(t, err)

	var cryptTargets []string
	for _, m := range section.MountPoints {
		if m.Fstype == "dm-crypt" {
			cryptTargets = append(cryptTargets, m.Target)
		}
	}
	assert.Contains(t, cryptTargets, "dm-crypt (luks-abcd)")
	assert.Contains(t, cryptTargets, "dm-crypt (luks-efgh)")
}

func TestDetectDMCryptNoDevices(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"dmsetup table --target crypt": {
			Stdout:   "No devices found\n",
			ExitCode: 0,
		},
	})

	section, _, err := RunStorage(exec, StorageOptions{})
	require.NoError(t, err)

	for _, m := range section.MountPoints {
		assert.NotEqual(t, "dm-crypt", m.Fstype, "should not add dm-crypt entries for 'No devices found'")
	}
}

// ---------------------------------------------------------------------------
// Automount detection
// ---------------------------------------------------------------------------

func TestDetectAutomount(t *testing.T) {
	exec := NewFakeExecutor(nil).WithFiles(map[string]string{
		"/etc/auto.master": "/home /etc/auto.home\n/net -hosts",
	}).WithDirs(map[string][]string{
		"/etc": {"auto.master", "auto.home", "auto.data", "fstab", "hostname"},
	})

	section, _, err := RunStorage(exec, StorageOptions{})
	require.NoError(t, err)

	var autoTargets []string
	for _, m := range section.MountPoints {
		if m.Fstype == "autofs" {
			autoTargets = append(autoTargets, m.Target)
		}
	}
	assert.Contains(t, autoTargets, "automount")
	assert.Contains(t, autoTargets, "automount (auto.home)")
	assert.Contains(t, autoTargets, "automount (auto.data)")
	// auto.master should not appear as a separate auto.* entry
	for _, tgt := range autoTargets {
		assert.NotEqual(t, "automount (auto.master)", tgt)
	}
}

// ---------------------------------------------------------------------------
// /var directory scan
// ---------------------------------------------------------------------------

func TestScanVarDirectories(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"du -sb /var/lib/mysql":      {Stdout: "5242880\t/var/lib/mysql", ExitCode: 0},
		"du -sb /var/lib/containers": {Stdout: "104857600\t/var/lib/containers", ExitCode: 0},
		"du -sb /var/log/httpd":      {Stdout: "1024\t/var/log/httpd", ExitCode: 0},
		"du -sb /var/www/html":       {Stdout: "2048\t/var/www/html", ExitCode: 0},
	}).WithDirs(map[string][]string{
		"/var/lib":            {"mysql", "containers", "systemd", "rpm"},
		"/var/lib/mysql":      {"data"},
		"/var/lib/containers": {"storage"},
		"/var/lib/systemd":    {"timers"},
		"/var/lib/rpm":        {"Packages"},
		"/var/log":            {"httpd"},
		"/var/log/httpd":      {"access.log"},
		"/var/www":            {"html"},
		"/var/www/html":       {"index.html"},
	})

	section, _, err := RunStorage(exec, StorageOptions{})
	require.NoError(t, err)

	paths := make(map[string]string)
	for _, vd := range section.VarDirectories {
		paths[vd.Path] = vd.Recommendation
	}

	// mysql should be present (not in skip list)
	assert.Contains(t, paths, "var/lib/mysql")
	assert.Contains(t, paths["var/lib/mysql"], "database")

	// containers should be present
	assert.Contains(t, paths, "var/lib/containers")
	assert.Contains(t, paths["var/lib/containers"], "container storage")

	// systemd and rpm should be skipped (OS-managed)
	assert.NotContains(t, paths, "var/lib/systemd")
	assert.NotContains(t, paths, "var/lib/rpm")

	// log directory
	assert.Contains(t, paths, "var/log/httpd")
	assert.Contains(t, paths["var/log/httpd"], "log retention")

	// web content
	assert.Contains(t, paths, "var/www/html")
	assert.Contains(t, paths["var/www/html"], "static")
}

func TestScanVarSkipsDotDirs(t *testing.T) {
	exec := NewFakeExecutor(nil).WithDirs(map[string][]string{
		"/var/lib":       {".hidden", "real"},
		"/var/lib/real":  {"data"},
		"/var/lib/.hidden": {"secret"},
	})

	section, _, err := RunStorage(exec, StorageOptions{})
	require.NoError(t, err)

	for _, vd := range section.VarDirectories {
		assert.False(t, strings.HasPrefix(filepath.Base(vd.Path), "."),
			"should skip dot directories: %s", vd.Path)
	}
}

func TestScanVarSkipsEmptyDirs(t *testing.T) {
	exec := NewFakeExecutor(nil).WithDirs(map[string][]string{
		"/var/lib":       {"empty"},
		"/var/lib/empty": {},
	})

	section, _, err := RunStorage(exec, StorageOptions{})
	require.NoError(t, err)
	assert.Empty(t, section.VarDirectories)
}

// ---------------------------------------------------------------------------
// varRecommendation
// ---------------------------------------------------------------------------

func TestVarRecommendation(t *testing.T) {
	tests := []struct {
		path     string
		category string
		contains string
	}{
		{"var/lib/mysql", "application data", "database"},
		{"var/lib/pgsql", "application data", "database"},
		{"var/lib/postgres", "application data", "database"},
		{"var/lib/mongodb", "application data", "database"},
		{"var/lib/mariadb", "application data", "database"},
		{"var/lib/containers", "application data", "container storage"},
		{"var/lib/docker", "application data", "container storage"},
		{"var/log/httpd", "log retention", "log retention"},
		{"var/www/html", "web content", "static"},
		{"var/lib/yum/cache", "application data", "Ephemeral"},
		{"var/spool/mail", "application data", "spool"},
		{"var/lib/someapp", "application data", "review application needs"},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			rec := varRecommendation(tt.path, tt.category)
			assert.Contains(t, rec, tt.contains)
		})
	}
}

// ---------------------------------------------------------------------------
// formatBytes
// ---------------------------------------------------------------------------

func TestFormatBytes(t *testing.T) {
	tests := []struct {
		input    string
		expected string
	}{
		{"500", "500 bytes"},
		{"2048", "~2 KB"},
		{"1048576", "~1 MB"},
		{"1073741824", "~1.0 GB"},
		{"15728640", "Over 10 MB"},
		{"invalid", "invalid"},
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			assert.Equal(t, tt.expected, formatBytes(tt.input))
		})
	}
}

// ---------------------------------------------------------------------------
// ostree mount filtering
// ---------------------------------------------------------------------------

func TestOstreeMountFiltering(t *testing.T) {
	findmntJSON := `{
		"filesystems": [
			{"target": "/", "source": "/dev/sda2", "fstype": "xfs", "options": "rw"},
			{"target": "/sysroot", "source": "/dev/sda2", "fstype": "xfs", "options": "rw"},
			{"target": "/ostree", "source": "/dev/sda2", "fstype": "xfs", "options": "rw"},
			{"target": "/ostree/deploy", "source": "/dev/sda2", "fstype": "xfs", "options": "rw"},
			{"target": "/sysroot/ostree", "source": "/dev/sda2", "fstype": "xfs", "options": "rw"},
			{"target": "/boot/efi", "source": "/dev/sda1", "fstype": "vfat", "options": "rw"},
			{"target": "/home", "source": "/dev/sda3", "fstype": "xfs", "options": "rw"}
		]
	}`

	// With bootc system type, ostree mounts should be filtered out.
	exec := NewFakeExecutor(map[string]ExecResult{
		"findmnt --json --real": {Stdout: findmntJSON, ExitCode: 0},
	})
	section, _, err := RunStorage(exec, StorageOptions{
		SystemType: schema.SystemTypeBootc,
	})
	require.NoError(t, err)

	targets := make(map[string]bool)
	for _, m := range section.MountPoints {
		targets[m.Target] = true
	}
	assert.True(t, targets["/"], "root should remain")
	assert.True(t, targets["/home"], "home should remain")
	assert.False(t, targets["/sysroot"], "sysroot should be filtered")
	assert.False(t, targets["/ostree"], "ostree should be filtered")
	assert.False(t, targets["/ostree/deploy"], "ostree prefix should be filtered")
	assert.False(t, targets["/sysroot/ostree"], "sysroot prefix should be filtered")
	assert.False(t, targets["/boot/efi"], "boot/efi should be filtered")
}

func TestNoOstreeFilteringPackageMode(t *testing.T) {
	findmntJSON := `{
		"filesystems": [
			{"target": "/", "source": "/dev/sda2", "fstype": "xfs", "options": "rw"},
			{"target": "/boot/efi", "source": "/dev/sda1", "fstype": "vfat", "options": "rw"}
		]
	}`

	exec := NewFakeExecutor(map[string]ExecResult{
		"findmnt --json --real": {Stdout: findmntJSON, ExitCode: 0},
	})
	section, _, err := RunStorage(exec, StorageOptions{
		SystemType: schema.SystemTypePackageMode,
	})
	require.NoError(t, err)

	assert.Len(t, section.MountPoints, 2, "package mode should keep all mounts")
}

// ---------------------------------------------------------------------------
// Full integration test (fixture-driven)
// ---------------------------------------------------------------------------

func TestRunStorageIntegration(t *testing.T) {
	fstab := loadStorageFixture(t, "fstab")
	findmntJSON := loadStorageFixture(t, "findmnt.json")
	lvsJSON := loadStorageFixture(t, "lvs.json")

	exec := NewFakeExecutor(map[string]ExecResult{
		"findmnt --json --real":         {Stdout: findmntJSON, ExitCode: 0},
		"lvs --reportformat json --units g": {Stdout: lvsJSON, ExitCode: 0},
		"dmsetup table --target crypt":  {Stdout: "No devices found\n", ExitCode: 0},
		"du -sb /var/lib/mysql":         {Stdout: "5242880\t/var/lib/mysql", ExitCode: 0},
	}).WithFiles(map[string]string{
		"/etc/fstab": fstab,
	}).WithDirs(map[string][]string{
		"/var/lib":       {"mysql"},
		"/var/lib/mysql": {"data"},
	})

	section, warnings, err := RunStorage(exec, StorageOptions{})
	require.NoError(t, err)
	assert.Empty(t, warnings)

	// fstab: 9 non-comment, non-blank entries
	assert.Len(t, section.FstabEntries, 9)

	// Credentials: credentials=... and secretfile=...
	assert.Len(t, section.CredentialRefs, 2)

	// findmnt: 4 entries
	assert.Len(t, section.MountPoints, 4)

	// LVM: 3 volumes
	assert.Len(t, section.LvmInfo, 3)

	// /var: mysql (systemd/rpm/etc are skipped)
	require.NotEmpty(t, section.VarDirectories)
	assert.Equal(t, "var/lib/mysql", section.VarDirectories[0].Path)
}

func TestRunStorageNoCommands(t *testing.T) {
	// Bare-minimum: no fstab, no commands succeed.
	exec := NewFakeExecutor(nil)

	section, _, err := RunStorage(exec, StorageOptions{})
	require.NoError(t, err)
	assert.Empty(t, section.FstabEntries)
	assert.Empty(t, section.MountPoints)
	assert.Empty(t, section.LvmInfo)
	assert.Empty(t, section.VarDirectories)
	assert.Empty(t, section.CredentialRefs)
}

// ---------------------------------------------------------------------------
// Credential detection edge cases
// ---------------------------------------------------------------------------

func TestCredentialDetection(t *testing.T) {
	tests := []struct {
		name       string
		opts       string
		wantCount  int
		wantPath   string
	}{
		{"credentials key", "credentials=/etc/samba/creds.txt,uid=1000", 1, "/etc/samba/creds.txt"},
		{"credential key", "credential=/etc/cred", 1, "/etc/cred"},
		{"password_file key", "password_file=/etc/pw", 1, "/etc/pw"},
		{"secretfile key", "secretfile=/etc/secret.key", 1, "/etc/secret.key"},
		{"no credentials", "rw,hard,intr", 0, ""},
		{"empty options", "", 0, ""},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			section := &schema.StorageSection{
				CredentialRefs: []schema.CredentialRef{},
			}
			detectCredentials("/mnt/test", tt.opts, section)
			assert.Len(t, section.CredentialRefs, tt.wantCount)
			if tt.wantCount > 0 {
				assert.Equal(t, tt.wantPath, section.CredentialRefs[0].CredentialPath)
			}
		})
	}
}
