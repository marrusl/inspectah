package baseline

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// ---------------------------------------------------------------------------
// SelectBaseImage
// ---------------------------------------------------------------------------

func TestSelectBaseImage(t *testing.T) {
	tests := []struct {
		name          string
		osID          string
		versionID     string
		targetVersion string
		wantImage     string
		wantVersion   string
	}{
		// RHEL 9
		{
			name:        "RHEL 9.4 (clamped to 9.6)",
			osID:        "rhel",
			versionID:   "9.4",
			wantImage:   "registry.redhat.io/rhel9/rhel-bootc:9.6",
			wantVersion: "9.6",
		},
		{
			name:        "RHEL 9.6 (at minimum)",
			osID:        "rhel",
			versionID:   "9.6",
			wantImage:   "registry.redhat.io/rhel9/rhel-bootc:9.6",
			wantVersion: "9.6",
		},
		{
			name:        "RHEL 9.8 (above minimum)",
			osID:        "rhel",
			versionID:   "9.8",
			wantImage:   "registry.redhat.io/rhel9/rhel-bootc:9.8",
			wantVersion: "9.8",
		},
		{
			name:          "RHEL 9.4 with target 9.6",
			osID:          "rhel",
			versionID:     "9.4",
			targetVersion: "9.6",
			wantImage:     "registry.redhat.io/rhel9/rhel-bootc:9.6",
			wantVersion:   "9.6",
		},
		{
			name:          "RHEL 9.4 with target 9.8",
			osID:          "rhel",
			versionID:     "9.4",
			targetVersion: "9.8",
			wantImage:     "registry.redhat.io/rhel9/rhel-bootc:9.8",
			wantVersion:   "9.8",
		},
		// RHEL 10
		{
			name:        "RHEL 10.0",
			osID:        "rhel",
			versionID:   "10.0",
			wantImage:   "registry.redhat.io/rhel10/rhel-bootc:10.0",
			wantVersion: "10.0",
		},
		{
			name:        "RHEL 10.1",
			osID:        "rhel",
			versionID:   "10.1",
			wantImage:   "registry.redhat.io/rhel10/rhel-bootc:10.1",
			wantVersion: "10.1",
		},
		// RHEL unsupported major
		{
			name:      "RHEL 8 unsupported",
			osID:      "rhel",
			versionID: "8.9",
			wantImage: "",
		},
		// CentOS Stream
		{
			name:        "CentOS Stream 9",
			osID:        "centos",
			versionID:   "9",
			wantImage:   "quay.io/centos-bootc/centos-bootc:stream9",
			wantVersion: "9",
		},
		{
			name:        "CentOS Stream 10",
			osID:        "centos",
			versionID:   "10",
			wantImage:   "quay.io/centos-bootc/centos-bootc:stream10",
			wantVersion: "10",
		},
		{
			name:      "CentOS 7 unsupported",
			osID:      "centos",
			versionID: "7",
			wantImage: "",
		},
		// Fedora
		{
			name:        "Fedora 41 (at minimum)",
			osID:        "fedora",
			versionID:   "41",
			wantImage:   "quay.io/fedora/fedora-bootc:41",
			wantVersion: "41",
		},
		{
			name:        "Fedora 42",
			osID:        "fedora",
			versionID:   "42",
			wantImage:   "quay.io/fedora/fedora-bootc:42",
			wantVersion: "42",
		},
		{
			name:        "Fedora 39 (clamped to 41)",
			osID:        "fedora",
			versionID:   "39",
			wantImage:   "quay.io/fedora/fedora-bootc:41",
			wantVersion: "41",
		},
		{
			name:          "Fedora with target version",
			osID:          "fedora",
			versionID:     "39",
			targetVersion: "42",
			wantImage:     "quay.io/fedora/fedora-bootc:42",
			wantVersion:   "42",
		},
		// Case insensitivity
		{
			name:        "RHEL uppercase",
			osID:        "RHEL",
			versionID:   "9.6",
			wantImage:   "registry.redhat.io/rhel9/rhel-bootc:9.6",
			wantVersion: "9.6",
		},
		// Unknown OS
		{
			name:      "Ubuntu unknown",
			osID:      "ubuntu",
			versionID: "22.04",
			wantImage: "",
		},
		// Empty inputs
		{
			name:      "empty os_id",
			osID:      "",
			versionID: "9.4",
			wantImage: "",
		},
		{
			name:      "empty version_id",
			osID:      "rhel",
			versionID: "",
			wantImage: "",
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			gotImage, gotVersion := SelectBaseImage(tc.osID, tc.versionID, tc.targetVersion)
			assert.Equal(t, tc.wantImage, gotImage, "image ref")
			if tc.wantImage != "" {
				assert.Equal(t, tc.wantVersion, gotVersion, "resolved version")
			}
		})
	}
}

// ---------------------------------------------------------------------------
// clampVersion
// ---------------------------------------------------------------------------

func TestClampVersion(t *testing.T) {
	tests := []struct {
		name    string
		version string
		minimum string
		want    string
	}{
		{"below minimum", "9.4", "9.6", "9.6"},
		{"at minimum", "9.6", "9.6", "9.6"},
		{"above minimum", "9.8", "9.6", "9.8"},
		{"major below", "8.9", "9.6", "9.6"},
		{"major above", "10.0", "9.6", "10.0"},
		{"invalid version", "abc", "9.6", "9.6"},
		{"empty version", "", "9.6", "9.6"},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			got := clampVersion(tc.version, tc.minimum)
			assert.Equal(t, tc.want, got)
		})
	}
}

// ---------------------------------------------------------------------------
// LoadBaselinePackagesFile
// ---------------------------------------------------------------------------

func TestLoadBaselinePackagesFile_NEVRAFormat(t *testing.T) {
	content := `0:bash-5.2.15-3.el9.x86_64
0:coreutils-8.32-34.el9.x86_64
(none):zlib-1.2.11-40.el9.x86_64
`
	path := writeTempFile(t, content)

	result, err := LoadBaselinePackagesFile(path)
	require.NoError(t, err)
	require.Len(t, result, 3)

	bash := result["bash.x86_64"]
	assert.Equal(t, "bash", bash.Name)
	assert.Equal(t, "0", bash.Epoch)
	assert.Equal(t, "5.2.15", bash.Version)
	assert.Equal(t, "3.el9", bash.Release)
	assert.Equal(t, "x86_64", bash.Arch)

	// (none) epoch normalized to "0"
	zlib := result["zlib.x86_64"]
	assert.Equal(t, "0", zlib.Epoch)
}

func TestLoadBaselinePackagesFile_NamesOnly(t *testing.T) {
	content := "bash\ncoreutils\nzlib\n"
	path := writeTempFile(t, content)

	result, err := LoadBaselinePackagesFile(path)
	require.NoError(t, err)
	require.Len(t, result, 3)

	bash := result["bash"]
	assert.Equal(t, "bash", bash.Name)
	assert.Equal(t, "0", bash.Epoch)
	assert.Equal(t, "", bash.Version)
}

func TestLoadBaselinePackagesFile_EmptyFile(t *testing.T) {
	path := writeTempFile(t, "")

	result, err := LoadBaselinePackagesFile(path)
	require.NoError(t, err)
	assert.Empty(t, result)
}

func TestLoadBaselinePackagesFile_MissingFile(t *testing.T) {
	_, err := LoadBaselinePackagesFile("/nonexistent/path/packages.txt")
	require.Error(t, err)
}

func TestLoadBaselinePackagesFile_WhitespaceLines(t *testing.T) {
	content := "\n  \n0:bash-5.2.15-3.el9.x86_64\n  \n"
	path := writeTempFile(t, content)

	result, err := LoadBaselinePackagesFile(path)
	require.NoError(t, err)
	require.Len(t, result, 1)
}

// ---------------------------------------------------------------------------
// BaseImageForSnapshot
// ---------------------------------------------------------------------------

func TestBaseImageForSnapshot_FromRpmSection(t *testing.T) {
	img := "registry.redhat.io/rhel9/rhel-bootc:9.6"
	snap := &schema.InspectionSnapshot{
		Rpm: &schema.RpmSection{BaseImage: &img},
	}
	assert.Equal(t, img, BaseImageForSnapshot(snap))
}

func TestBaseImageForSnapshot_FromOsRelease(t *testing.T) {
	snap := &schema.InspectionSnapshot{
		OsRelease: &schema.OsRelease{ID: "rhel", VersionID: "9.6"},
	}
	assert.Equal(t, "registry.redhat.io/rhel9/rhel-bootc:9.6", BaseImageForSnapshot(snap))
}

func TestBaseImageForSnapshot_Fallback(t *testing.T) {
	snap := &schema.InspectionSnapshot{}
	assert.Equal(t, DefaultFallbackImage, BaseImageForSnapshot(snap))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

func writeTempFile(t *testing.T, content string) string {
	t.Helper()
	dir := t.TempDir()
	path := filepath.Join(dir, "packages.txt")
	err := os.WriteFile(path, []byte(content), 0644)
	require.NoError(t, err)
	return path
}
