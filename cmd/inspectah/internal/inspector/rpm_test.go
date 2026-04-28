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

func loadRpmFixture(t *testing.T, name string) string {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("testdata", "rpm", name))
	require.NoError(t, err, "loading fixture %s", name)
	return string(data)
}

// ---------------------------------------------------------------------------
// NEVRA parsing
// ---------------------------------------------------------------------------

func TestParseNEVRA(t *testing.T) {
	tests := []struct {
		name   string
		input  string
		expect *schema.PackageEntry
	}{
		{
			name:  "standard package with epoch 0",
			input: "(none):bash-5.1.8-6.el9_1.x86_64",
			expect: &schema.PackageEntry{
				Name: "bash", Epoch: "0", Version: "5.1.8",
				Release: "6.el9_1", Arch: "x86_64", State: schema.PackageStateAdded,
			},
		},
		{
			name:  "package with numeric epoch",
			input: "1:curl-7.76.1-26.el9.x86_64",
			expect: &schema.PackageEntry{
				Name: "curl", Epoch: "1", Version: "7.76.1",
				Release: "26.el9", Arch: "x86_64", State: schema.PackageStateAdded,
			},
		},
		{
			name:  "multi-hyphen package name",
			input: "(none):python3-urllib3-1.26.5-3.el9.noarch",
			expect: &schema.PackageEntry{
				Name: "python3-urllib3", Epoch: "0", Version: "1.26.5",
				Release: "3.el9", Arch: "noarch", State: schema.PackageStateAdded,
			},
		},
		{
			name:  "i686 architecture",
			input: "(none):glibc-2.34-60.el9.i686",
			expect: &schema.PackageEntry{
				Name: "glibc", Epoch: "0", Version: "2.34",
				Release: "60.el9", Arch: "i686", State: schema.PackageStateAdded,
			},
		},
		{name: "empty string", input: "", expect: nil},
		{name: "no colon", input: "bash-5.1.8-6.el9_1.x86_64", expect: nil},
		{name: "garbage", input: "not-a-package", expect: nil},
		{name: "no dot for arch", input: "(none):bash-5", expect: nil},
		{name: "too few dashes", input: "(none):bash.x86_64", expect: nil},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			got := ParseNEVRA(tc.input)
			if tc.expect == nil {
				assert.Nil(t, got)
			} else {
				require.NotNil(t, got)
				assert.Equal(t, tc.expect.Name, got.Name)
				assert.Equal(t, tc.expect.Epoch, got.Epoch)
				assert.Equal(t, tc.expect.Version, got.Version)
				assert.Equal(t, tc.expect.Release, got.Release)
				assert.Equal(t, tc.expect.Arch, got.Arch)
			}
		})
	}
}

func TestParseRpmQA(t *testing.T) {
	input := loadRpmFixture(t, "rpm-qa.txt")
	var warnings []Warning
	packages := parseRpmQA(input, &warnings)

	assert.GreaterOrEqual(t, len(packages), 5, "should parse multiple packages")

	// Check specific packages exist.
	found := make(map[string]bool)
	for _, p := range packages {
		found[p.Name] = true
	}
	assert.True(t, found["bash"], "should contain bash")
	assert.True(t, found["curl"], "should contain curl")
	assert.True(t, found["python3-urllib3"], "should contain python3-urllib3")

	// Virtual packages should not be in the output (they're filtered in rpmQA).
	// But parseRpmQA doesn't filter — rpmQA does. Let's just check parsing works.
}

func TestParseRpmQAWithParseFailures(t *testing.T) {
	input := `(none):bash-5.1.8-6.el9_1.x86_64
garbage-line-here
not-parseable
(none):curl-7.76.1-26.el9.x86_64`

	var warnings []Warning
	packages := parseRpmQA(input, &warnings)

	assert.Len(t, packages, 2)
	assert.Len(t, warnings, 1)
	assert.Contains(t, warnings[0]["message"], "could not be parsed")
}

// ---------------------------------------------------------------------------
// rpm -Va parsing
// ---------------------------------------------------------------------------

func TestParseRpmVa(t *testing.T) {
	input := loadRpmFixture(t, "rpm-va.txt")
	entries := parseRpmVa(input)

	assert.GreaterOrEqual(t, len(entries), 2, "should parse multiple entries")

	// Check specific entries.
	var foundEtcFoo, foundEtcBar bool
	for _, e := range entries {
		switch e.Path {
		case "/etc/foo.conf":
			foundEtcFoo = true
			assert.Equal(t, "S.5....T.", e.Flags)
		case "/etc/bar.conf":
			foundEtcBar = true
			assert.Equal(t, "..5....T.", e.Flags)
		}
	}
	assert.True(t, foundEtcFoo, "should contain /etc/foo.conf")
	assert.True(t, foundEtcBar, "should contain /etc/bar.conf")
}

func TestParseRpmVaSkipsBoot(t *testing.T) {
	input := `S.5....T.    /etc/foo.conf
.......T.    /boot/vmlinuz
S.5....T.    /etc/bar.conf`

	entries := parseRpmVa(input)
	assert.Len(t, entries, 2)
	for _, e := range entries {
		assert.False(t, e.Path == "/boot/vmlinuz", "should skip /boot/ paths")
	}
}

func TestParseRpmVaConfigTypePrefix(t *testing.T) {
	input := `S.5....T.  c /etc/httpd/conf/httpd.conf
..5....T.  d /etc/sysconfig/network`

	entries := parseRpmVa(input)
	require.Len(t, entries, 2)
	assert.Equal(t, "/etc/httpd/conf/httpd.conf", entries[0].Path)
	assert.Equal(t, "/etc/sysconfig/network", entries[1].Path)
}

// ---------------------------------------------------------------------------
// RPM version comparison
// ---------------------------------------------------------------------------

func TestRpmvercmp(t *testing.T) {
	tests := []struct {
		a, b string
		want int // -1, 0, or 1
	}{
		{"1.0", "1.0", 0},
		{"1.0", "2.0", -1},
		{"2.0", "1.0", 1},
		{"1.0.1", "1.0.2", -1},
		{"1.0.10", "1.0.2", 1},    // numeric comparison
		{"1.0~rc1", "1.0", -1},    // tilde sorts before
		{"1.0", "1.0~rc1", 1},     // tilde sorts before
		{"1.0^post1", "1.0", 1},   // caret sorts after
		{"1.0", "1.0^post1", -1},  // caret sorts after
		{"1.0~rc1", "1.0~rc2", -1},
		{"1.0a", "1.0b", -1},
		{"1.0", "1.0a", -1},
	}

	for _, tc := range tests {
		t.Run(tc.a+"_vs_"+tc.b, func(t *testing.T) {
			got := rpmvercmp(tc.a, tc.b)
			switch {
			case tc.want < 0:
				assert.Less(t, got, 0, "%s should be < %s", tc.a, tc.b)
			case tc.want > 0:
				assert.Greater(t, got, 0, "%s should be > %s", tc.a, tc.b)
			default:
				assert.Equal(t, 0, got, "%s should == %s", tc.a, tc.b)
			}
		})
	}
}

func TestCompareEVR(t *testing.T) {
	tests := []struct {
		name string
		host schema.PackageEntry
		base schema.PackageEntry
		want int
	}{
		{
			name: "equal versions",
			host: schema.PackageEntry{Epoch: "0", Version: "1.0", Release: "1.el9"},
			base: schema.PackageEntry{Epoch: "0", Version: "1.0", Release: "1.el9"},
			want: 0,
		},
		{
			name: "host upgraded",
			host: schema.PackageEntry{Epoch: "0", Version: "2.0", Release: "1.el9"},
			base: schema.PackageEntry{Epoch: "0", Version: "1.0", Release: "1.el9"},
			want: 1,
		},
		{
			name: "host downgraded",
			host: schema.PackageEntry{Epoch: "0", Version: "1.0", Release: "1.el9"},
			base: schema.PackageEntry{Epoch: "0", Version: "2.0", Release: "1.el9"},
			want: -1,
		},
		{
			name: "epoch trumps version",
			host: schema.PackageEntry{Epoch: "2", Version: "1.0", Release: "1.el9"},
			base: schema.PackageEntry{Epoch: "1", Version: "9.0", Release: "1.el9"},
			want: 1,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			got := compareEVR(tc.host, tc.base)
			switch {
			case tc.want < 0:
				assert.Less(t, got, 0)
			case tc.want > 0:
				assert.Greater(t, got, 0)
			default:
				assert.Equal(t, 0, got)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Version lock parsing
// ---------------------------------------------------------------------------

func TestParseNEVRAPattern(t *testing.T) {
	tests := []struct {
		name    string
		input   string
		wantErr bool
		expect  *schema.VersionLockEntry
	}{
		{
			name:  "standard with epoch",
			input: "1:curl-7.76.1-26.el9.x86_64",
			expect: &schema.VersionLockEntry{
				RawPattern: "1:curl-7.76.1-26.el9.x86_64",
				Name:       "curl",
				Epoch:      1,
				Version:    "7.76.1",
				Release:    "26.el9",
				Arch:       "x86_64",
			},
		},
		{
			name:  "no epoch",
			input: "curl-7.76.1-26.el9.x86_64",
			expect: &schema.VersionLockEntry{
				RawPattern: "curl-7.76.1-26.el9.x86_64",
				Name:       "curl",
				Epoch:      0,
				Version:    "7.76.1",
				Release:    "26.el9",
				Arch:       "x86_64",
			},
		},
		{
			name:  "wildcard arch",
			input: "curl-7.76.1-26.el9.*",
			expect: &schema.VersionLockEntry{
				RawPattern: "curl-7.76.1-26.el9.*",
				Name:       "curl",
				Epoch:      0,
				Version:    "7.76.1",
				Release:    "26.el9",
				Arch:       "*",
			},
		},
		{
			name:  "multi-hyphen name",
			input: "python3-urllib3-1.26.5-3.el9.noarch",
			expect: &schema.VersionLockEntry{
				RawPattern: "python3-urllib3-1.26.5-3.el9.noarch",
				Name:       "python3-urllib3",
				Epoch:      0,
				Version:    "1.26.5",
				Release:    "3.el9",
				Arch:       "noarch",
			},
		},
		{
			name:    "unparseable",
			input:   "notapackage",
			wantErr: true,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			got, err := parseNEVRAPattern(tc.input)
			if tc.wantErr {
				assert.Error(t, err)
				return
			}
			require.NoError(t, err)
			require.NotNil(t, got)
			assert.Equal(t, tc.expect.RawPattern, got.RawPattern)
			assert.Equal(t, tc.expect.Name, got.Name)
			assert.Equal(t, tc.expect.Epoch, got.Epoch)
			assert.Equal(t, tc.expect.Version, got.Version)
			assert.Equal(t, tc.expect.Release, got.Release)
			assert.Equal(t, tc.expect.Arch, got.Arch)
		})
	}
}

// ---------------------------------------------------------------------------
// Multi-arch and duplicate detection
// ---------------------------------------------------------------------------

func TestDetectMultiarch(t *testing.T) {
	installed := []schema.PackageEntry{
		{Name: "glibc", Arch: "x86_64"},
		{Name: "glibc", Arch: "i686"},
		{Name: "bash", Arch: "x86_64"},
		{Name: "openssl", Arch: "x86_64"},
	}

	result := detectMultiarch(installed)
	assert.Contains(t, result, "glibc.i686")
	assert.NotContains(t, result, "glibc.x86_64") // x86_64 is the primary
	assert.Len(t, result, 1)
}

func TestDetectDuplicates(t *testing.T) {
	installed := []schema.PackageEntry{
		{Name: "kernel", Version: "5.14.0", Arch: "x86_64"},
		{Name: "kernel", Version: "5.14.1", Arch: "x86_64"},
		{Name: "bash", Version: "5.1.8", Arch: "x86_64"},
	}

	result := detectDuplicates(installed)
	assert.Contains(t, result, "kernel.x86_64")
	assert.Len(t, result, 1)
}

// ---------------------------------------------------------------------------
// Module stream parsing
// ---------------------------------------------------------------------------

func TestParseModuleINI(t *testing.T) {
	input := loadRpmFixture(t, "modules.module")
	result := parseModuleINI(input)

	assert.Contains(t, result, "nodejs")
	assert.Equal(t, "18", result["nodejs"].stream)

	assert.Contains(t, result, "postgresql")
	assert.Equal(t, "15", result["postgresql"].stream)
}

// ---------------------------------------------------------------------------
// Repo file classification
// ---------------------------------------------------------------------------

func TestClassifyDefaultRepo(t *testing.T) {
	tests := []struct {
		name   string
		repo   schema.RepoFile
		expect bool
	}{
		{
			name:   "redhat.repo filename",
			repo:   schema.RepoFile{Path: "etc/yum.repos.d/redhat.repo", Content: "[main]\nenabled=1"},
			expect: true,
		},
		{
			name:   "baseos section ID",
			repo:   schema.RepoFile{Path: "etc/yum.repos.d/custom.repo", Content: "[baseos]\nenabled=1"},
			expect: true,
		},
		{
			name:   "appstream section ID",
			repo:   schema.RepoFile{Path: "etc/yum.repos.d/other.repo", Content: "[appstream-rpms]\nenabled=1"},
			expect: true,
		},
		{
			name:   "custom EPEL repo",
			repo:   schema.RepoFile{Path: "etc/yum.repos.d/epel.repo", Content: "[epel]\nenabled=1"},
			expect: false,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			got := classifyDefaultRepo(tc.repo)
			assert.Equal(t, tc.expect, got)
		})
	}
}

// ---------------------------------------------------------------------------
// Warning helper
// ---------------------------------------------------------------------------

func TestMakeWarning(t *testing.T) {
	w := makeWarning("rpm", "test message", "info")
	assert.Equal(t, "rpm", w["inspector"])
	assert.Equal(t, "test message", w["message"])
	assert.Equal(t, "info", w["severity"])

	w2 := makeWarning("rpm", "error message")
	assert.Equal(t, "error", w2["severity"])
}

// ---------------------------------------------------------------------------
// GPG key collection
// ---------------------------------------------------------------------------

func TestCollectGpgKeys(t *testing.T) {
	exec := NewFakeExecutor(nil).WithFiles(map[string]string{
		"/etc/pki/rpm-gpg/RPM-GPG-KEY-redhat-release": "-----BEGIN PGP PUBLIC KEY BLOCK-----\nfake key\n-----END PGP PUBLIC KEY BLOCK-----",
	})

	repoFiles := []schema.RepoFile{
		{
			Path:    "etc/yum.repos.d/redhat.repo",
			Content: "[baseos]\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-redhat-release\nenabled=1",
		},
	}

	keys := collectGpgKeys(exec, repoFiles)
	require.Len(t, keys, 1)
	assert.Equal(t, "etc/pki/rpm-gpg/RPM-GPG-KEY-redhat-release", keys[0].Path)
	assert.Contains(t, keys[0].Content, "BEGIN PGP")
}

func TestCollectGpgKeysSkipsHttps(t *testing.T) {
	exec := NewFakeExecutor(nil)

	repoFiles := []schema.RepoFile{
		{
			Path:    "etc/yum.repos.d/custom.repo",
			Content: "[custom]\ngpgkey=https://example.com/key.gpg\nenabled=1",
		},
	}

	keys := collectGpgKeys(exec, repoFiles)
	assert.Len(t, keys, 0)
}

func TestCollectGpgKeysContinuationLines(t *testing.T) {
	exec := NewFakeExecutor(nil).WithFiles(map[string]string{
		"/etc/pki/rpm-gpg/KEY1": "key1-content",
		"/etc/pki/rpm-gpg/KEY2": "key2-content",
	})

	repoFiles := []schema.RepoFile{
		{
			Path: "etc/yum.repos.d/multi.repo",
			Content: `[section]
gpgkey=file:///etc/pki/rpm-gpg/KEY1
       file:///etc/pki/rpm-gpg/KEY2
enabled=1`,
		},
	}

	keys := collectGpgKeys(exec, repoFiles)
	assert.Len(t, keys, 2)
}

// ---------------------------------------------------------------------------
// Integration test: full RunRpm
// ---------------------------------------------------------------------------

// rpmQACmdKey returns the FakeExecutor command key for rpm -qa.
func rpmQACmdKey() string {
	return "rpm -qa --queryformat " + RpmQAQueryformat + "\n"
}

func TestRunRpmNoBaseline(t *testing.T) {
	rpmQAOutput := loadRpmFixture(t, "rpm-qa.txt")
	rpmVaOutput := loadRpmFixture(t, "rpm-va.txt")

	exec := NewFakeExecutor(map[string]ExecResult{
		// rpm -qa
		rpmQACmdKey(): {
			Stdout: rpmQAOutput, ExitCode: 0,
		},
		// rpm -Va
		"rpm -Va --nodeps --noscripts": {
			Stdout: rpmVaOutput, ExitCode: 1, // rc != 0 is normal for rpm -Va
		},
		// dnf repoquery for source repos — probe (fail so we skip)
		"dnf repoquery --installed --queryformat %{name} %{from_repo}\n bash": {
			ExitCode: 1,
		},
		// rpm -qi fallback for source repos
		"rpm -qi bash curl glibc python3-urllib3 systemd vim-enhanced": {
			Stdout: `Name        : bash
From repo   : baseos
Name        : curl
From repo   : baseos
Name        : glibc
From repo   : baseos
Name        : python3-urllib3
From repo   : appstream
Name        : systemd
From repo   : baseos
Name        : vim-enhanced
From repo   : appstream`,
			ExitCode: 0,
		},
		// dnf history
		"dnf history list -q": {ExitCode: 1},
		// rpm -qf for repo-providing packages
		"rpm -qf --queryformat %{NAME}\n /etc/yum.repos.d/redhat.repo": {
			Stdout:   "redhat-release\n",
			ExitCode: 0,
		},
		// dnf versionlock
		"dnf versionlock list": {ExitCode: 1},
	}).WithFiles(map[string]string{
		"/etc/yum.repos.d/redhat.repo": "[baseos]\nenabled=1\n",
	}).WithDirs(map[string][]string{
		"/etc/yum.repos.d":    {"redhat.repo"},
		"/etc/dnf":            {},
		"/etc/dnf/modules.d":  {},
	})

	section, warnings, err := RunRpm(exec, RpmOptions{
		SystemType: schema.SystemTypePackageMode,
	})

	require.NoError(t, err)
	require.NotNil(t, section)

	// No baseline means all installed are added.
	assert.True(t, section.NoBaseline)
	assert.GreaterOrEqual(t, len(section.PackagesAdded), 5)
	assert.Empty(t, section.BaseImageOnly)
	assert.Empty(t, section.VersionChanges)

	// rpm -Va should have entries.
	assert.GreaterOrEqual(t, len(section.RpmVa), 2)

	// Repo files.
	assert.GreaterOrEqual(t, len(section.RepoFiles), 1)

	// No fatal warnings expected.
	_ = warnings
}

func TestRunRpmWithBaseline(t *testing.T) {
	rpmQAOutput := loadRpmFixture(t, "rpm-qa.txt")

	// Create a baseline that includes bash and glibc but NOT vim-enhanced.
	baseline := map[string]schema.PackageEntry{
		"bash.x86_64": {
			Name: "bash", Epoch: "0", Version: "5.1.8",
			Release: "6.el9_1", Arch: "x86_64",
		},
		"glibc.x86_64": {
			Name: "glibc", Epoch: "0", Version: "2.34",
			Release: "60.el9", Arch: "x86_64",
		},
		"glibc.i686": {
			Name: "glibc", Epoch: "0", Version: "2.34",
			Release: "60.el9", Arch: "i686",
		},
		"curl.x86_64": {
			Name: "curl", Epoch: "1", Version: "7.76.1",
			Release: "26.el9", Arch: "x86_64",
		},
		"python3-urllib3.noarch": {
			Name: "python3-urllib3", Epoch: "0", Version: "1.26.5",
			Release: "3.el9", Arch: "noarch",
		},
		"systemd.x86_64": {
			Name: "systemd", Epoch: "0", Version: "252",
			Release: "14.el9_2", Arch: "x86_64",
		},
		// Package in baseline but NOT installed (base_image_only).
		"missing-pkg.x86_64": {
			Name: "missing-pkg", Epoch: "0", Version: "1.0",
			Release: "1.el9", Arch: "x86_64",
		},
	}

	exec := NewFakeExecutor(map[string]ExecResult{
		rpmQACmdKey(): {
			Stdout: rpmQAOutput, ExitCode: 0,
		},
		"rpm -Va --nodeps --noscripts": {Stdout: "", ExitCode: 0},
		// dnf repoquery probe fails.
		"dnf repoquery --installed --queryformat %{name} %{from_repo}\n vim-enhanced": {ExitCode: 1},
		// rpm -qi fallback.
		"rpm -qi vim-enhanced": {
			Stdout:   "Name        : vim-enhanced\nFrom repo   : appstream\n",
			ExitCode: 0,
		},
		// leaf/auto: dnf --userinstalled.
		"dnf repoquery --userinstalled --queryformat %{name}\n": {
			Stdout: "vim-enhanced\n", ExitCode: 0,
		},
		// leaf/auto: dnf repoquery --requires for vim-enhanced.
		"dnf repoquery --requires --resolve --recursive --installed --queryformat %{name}\n vim-enhanced": {
			Stdout: "", ExitCode: 0,
		},
		// dnf history.
		"dnf history list -q": {ExitCode: 1},
		// repo providing packages.
		"rpm -qf --queryformat %{NAME}\n /etc/yum.repos.d/redhat.repo": {
			Stdout: "redhat-release\n", ExitCode: 0,
		},
		// dnf versionlock.
		"dnf versionlock list": {ExitCode: 1},
	}).WithDirs(map[string][]string{
		"/etc/yum.repos.d":   {"redhat.repo"},
		"/etc/dnf":           {},
		"/etc/dnf/modules.d": {},
	}).WithFiles(map[string]string{
		"/etc/yum.repos.d/redhat.repo": "[baseos]\nenabled=1\n",
	})

	section, _, err := RunRpm(exec, RpmOptions{
		SystemType:       schema.SystemTypePackageMode,
		BaselinePackages: baseline,
		BaseImage:        "registry.redhat.io/rhel9/rhel-bootc:9.4",
	})

	require.NoError(t, err)
	require.NotNil(t, section)

	// vim-enhanced is installed but not in baseline → should be added.
	assert.False(t, section.NoBaseline)

	addedNames := make(map[string]bool)
	for _, p := range section.PackagesAdded {
		addedNames[p.Name] = true
		assert.Equal(t, schema.PackageStateAdded, p.State)
	}
	assert.True(t, addedNames["vim-enhanced"], "vim-enhanced should be in added")
	assert.False(t, addedNames["bash"], "bash should NOT be in added (in baseline)")

	// missing-pkg is in baseline but not installed → base_image_only.
	bioNames := make(map[string]bool)
	for _, p := range section.BaseImageOnly {
		bioNames[p.Name] = true
		assert.Equal(t, schema.PackageStateBaseImageOnly, p.State)
	}
	assert.True(t, bioNames["missing-pkg"], "missing-pkg should be in base_image_only")

	// Leaf classification.
	require.NotNil(t, section.LeafPackages)
	assert.Contains(t, *section.LeafPackages, "vim-enhanced")

	// BaseImage set.
	require.NotNil(t, section.BaseImage)
	assert.Equal(t, "registry.redhat.io/rhel9/rhel-bootc:9.4", *section.BaseImage)
}

func TestRunRpmVersionChanges(t *testing.T) {
	// Install curl with a newer version than baseline.
	rpmQAOutput := "1:curl-8.0.0-1.el9.x86_64\n"
	baseline := map[string]schema.PackageEntry{
		"curl.x86_64": {
			Name: "curl", Epoch: "1", Version: "7.76.1",
			Release: "26.el9", Arch: "x86_64",
		},
	}

	exec := NewFakeExecutor(map[string]ExecResult{
		rpmQACmdKey(): {
			Stdout: rpmQAOutput, ExitCode: 0,
		},
		"rpm -Va --nodeps --noscripts": {Stdout: "", ExitCode: 0},
		"dnf history list -q":          {ExitCode: 1},
		"dnf versionlock list":         {ExitCode: 1},
	}).WithDirs(map[string][]string{
		"/etc/yum.repos.d":   {},
		"/etc/dnf":           {},
		"/etc/dnf/modules.d": {},
	})

	section, warnings, err := RunRpm(exec, RpmOptions{
		SystemType:       schema.SystemTypePackageMode,
		BaselinePackages: baseline,
	})

	require.NoError(t, err)
	require.Len(t, section.VersionChanges, 1)

	vc := section.VersionChanges[0]
	assert.Equal(t, "curl", vc.Name)
	assert.Equal(t, "x86_64", vc.Arch)
	assert.Equal(t, "8.0.0-1.el9", vc.HostVersion)
	assert.Equal(t, "7.76.1-26.el9", vc.BaseVersion)
	// host has higher version → downgrade direction (base will downgrade the host).
	assert.Equal(t, schema.VersionChangeDowngrade, vc.Direction)

	// Should have a downgrade warning.
	hasDowngradeWarning := false
	for _, w := range warnings {
		msg, _ := w["message"].(string)
		if len(msg) > 0 && msg[0] == '1' {
			hasDowngradeWarning = true
		}
	}
	assert.True(t, hasDowngradeWarning, "should warn about downgrades")
}

func TestRunRpmOstreeMode(t *testing.T) {
	rpmQAOutput := "(none):bash-5.1.8-6.el9_1.x86_64\n"

	ostreeJSON := `{
  "deployments": [{
    "booted": true,
    "requested-packages": ["vim-enhanced", "htop"],
    "base-removals": [{"name": "nano"}],
    "base-local-replacements": [{
      "name": "kernel",
      "nevra": "kernel-5.14.1-1.el9.x86_64",
      "base-nevra": "kernel-5.14.0-1.el9.x86_64"
    }]
  }]
}`

	exec := NewFakeExecutor(map[string]ExecResult{
		rpmQACmdKey(): {
			Stdout: rpmQAOutput, ExitCode: 0,
		},
		"rpm-ostree status --json": {Stdout: ostreeJSON, ExitCode: 0},
		"dnf history list -q":      {ExitCode: 1},
		"dnf versionlock list":     {ExitCode: 1},
	}).WithDirs(map[string][]string{
		"/etc/yum.repos.d":   {},
		"/etc/dnf":           {},
		"/etc/dnf/modules.d": {},
	})

	section, _, err := RunRpm(exec, RpmOptions{
		SystemType: schema.SystemTypeRpmOstree,
	})

	require.NoError(t, err)

	// rpm -Va should be empty for non-package-mode.
	assert.Empty(t, section.RpmVa)

	// Ostree layered packages.
	addedNames := make(map[string]bool)
	for _, p := range section.PackagesAdded {
		addedNames[p.Name] = true
	}
	assert.True(t, addedNames["vim-enhanced"], "vim-enhanced should be layered")
	assert.True(t, addedNames["htop"], "htop should be layered")

	// Ostree removals.
	assert.Contains(t, section.OstreeRemovals, "nano")

	// Ostree overrides.
	require.Len(t, section.OstreeOverrides, 1)
	assert.Equal(t, "kernel", section.OstreeOverrides[0].Name)
	assert.Equal(t, "kernel-5.14.1-1.el9.x86_64", section.OstreeOverrides[0].ToNevra)
	assert.Equal(t, "kernel-5.14.0-1.el9.x86_64", section.OstreeOverrides[0].FromNevra)
}

func TestRunRpmDnfHistoryRemoved(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		rpmQACmdKey(): {
			Stdout: "(none):bash-5.1.8-6.el9_1.x86_64\n", ExitCode: 0,
		},
		"rpm -Va --nodeps --noscripts": {Stdout: "", ExitCode: 0},
		"dnf history list -q": {
			Stdout: `   1 | root           | 2024-01-15 10:00 | Install   | 5
   2 | root           | 2024-01-16 11:00 | Removed   | 2`,
			ExitCode: 0,
		},
		"dnf history info 2 -q": {
			Stdout: `Transaction ID : 2
Altered:
    Removed nano-5.6.1-5.el9.x86_64`,
			ExitCode: 0,
		},
		"dnf versionlock list":                                                              {ExitCode: 1},
		"dnf repoquery --installed --queryformat %{name} %{from_repo}\n bash":               {ExitCode: 1},
		"rpm -qi bash":                                                                       {ExitCode: 1},
		"rpm -qf --queryformat %{NAME}\n /etc/yum.repos.d/redhat.repo":                      {Stdout: "redhat-release\n", ExitCode: 0},
	}).WithDirs(map[string][]string{
		"/etc/yum.repos.d":   {"redhat.repo"},
		"/etc/dnf":           {},
		"/etc/dnf/modules.d": {},
	}).WithFiles(map[string]string{
		"/etc/yum.repos.d/redhat.repo": "[baseos]\nenabled=1\n",
	})

	section, _, err := RunRpm(exec, RpmOptions{
		SystemType: schema.SystemTypePackageMode,
	})

	require.NoError(t, err)
	assert.Contains(t, section.DnfHistoryRemoved, "nano")
}

func TestRunRpmModuleStreams(t *testing.T) {
	moduleContent := loadRpmFixture(t, "modules.module")

	exec := NewFakeExecutor(map[string]ExecResult{
		rpmQACmdKey(): {
			Stdout: "(none):bash-5.1.8-6.el9_1.x86_64\n", ExitCode: 0,
		},
		"rpm -Va --nodeps --noscripts": {Stdout: "", ExitCode: 0},
		"dnf history list -q":          {ExitCode: 1},
		"dnf versionlock list":         {ExitCode: 1},
		"dnf repoquery --installed --queryformat %{name} %{from_repo}\n bash": {ExitCode: 1},
		"rpm -qi bash": {ExitCode: 1},
	}).WithDirs(map[string][]string{
		"/etc/yum.repos.d":   {},
		"/etc/dnf":           {},
		"/etc/dnf/modules.d": {"nodejs.module", "postgresql.module"},
	}).WithFiles(map[string]string{
		"/etc/dnf/modules.d/nodejs.module":     moduleContent,
		"/etc/dnf/modules.d/postgresql.module": moduleContent,
	})

	section, _, err := RunRpm(exec, RpmOptions{
		SystemType: schema.SystemTypePackageMode,
	})

	require.NoError(t, err)
	assert.GreaterOrEqual(t, len(section.ModuleStreams), 2, "should find module streams")
}

func TestRunRpmVersionLocks(t *testing.T) {
	vlContent := loadRpmFixture(t, "versionlock.list")

	exec := NewFakeExecutor(map[string]ExecResult{
		rpmQACmdKey(): {
			Stdout: "(none):bash-5.1.8-6.el9_1.x86_64\n", ExitCode: 0,
		},
		"rpm -Va --nodeps --noscripts": {Stdout: "", ExitCode: 0},
		"dnf history list -q":          {ExitCode: 1},
		"dnf versionlock list": {
			Stdout:   "curl-7.76.1-26.el9.*\n",
			ExitCode: 0,
		},
		"dnf repoquery --installed --queryformat %{name} %{from_repo}\n bash": {ExitCode: 1},
		"rpm -qi bash": {ExitCode: 1},
	}).WithDirs(map[string][]string{
		"/etc/yum.repos.d":   {},
		"/etc/dnf":           {},
		"/etc/dnf/modules.d": {},
	}).WithFiles(map[string]string{
		"/etc/dnf/plugins/versionlock.list": vlContent,
	})

	section, _, err := RunRpm(exec, RpmOptions{
		SystemType: schema.SystemTypePackageMode,
	})

	require.NoError(t, err)
	assert.GreaterOrEqual(t, len(section.VersionLocks), 1)
	require.NotNil(t, section.VersionlockCommandOutput)
	assert.Contains(t, *section.VersionlockCommandOutput, "curl")
}

func TestRunRpmRepoFiles(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		rpmQACmdKey(): {
			Stdout: "(none):bash-5.1.8-6.el9_1.x86_64\n", ExitCode: 0,
		},
		"rpm -Va --nodeps --noscripts": {Stdout: "", ExitCode: 0},
		"dnf history list -q":          {ExitCode: 1},
		"dnf versionlock list":         {ExitCode: 1},
		"dnf repoquery --installed --queryformat %{name} %{from_repo}\n bash": {ExitCode: 1},
		"rpm -qi bash": {ExitCode: 1},
		"rpm -qf --queryformat %{NAME}\n /etc/yum.repos.d/redhat.repo /etc/yum.repos.d/epel.repo": {
			Stdout:   "redhat-release\nepel-release\n",
			ExitCode: 0,
		},
	}).WithDirs(map[string][]string{
		"/etc/yum.repos.d":   {"redhat.repo", "epel.repo"},
		"/etc/dnf":           {},
		"/etc/dnf/modules.d": {},
	}).WithFiles(map[string]string{
		"/etc/yum.repos.d/redhat.repo": "[baseos]\nenabled=1\ngpgcheck=1\n",
		"/etc/yum.repos.d/epel.repo":   "[epel]\nenabled=1\ngpgcheck=1\n",
	})

	section, _, err := RunRpm(exec, RpmOptions{
		SystemType: schema.SystemTypePackageMode,
	})

	require.NoError(t, err)
	assert.Len(t, section.RepoFiles, 2)

	// Check default repo classification.
	repoByPath := make(map[string]schema.RepoFile)
	for _, rf := range section.RepoFiles {
		repoByPath[rf.Path] = rf
	}
	assert.True(t, repoByPath["etc/yum.repos.d/redhat.repo"].IsDefaultRepo)
	assert.False(t, repoByPath["etc/yum.repos.d/epel.repo"].IsDefaultRepo)
}
