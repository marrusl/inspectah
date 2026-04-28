package architect

import (
	"archive/tar"
	"compress/gzip"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestLoadRefinedFleets_EmptyDir(t *testing.T) {
	dir := t.TempDir()
	fleets, err := LoadRefinedFleets(dir)
	require.NoError(t, err)
	assert.Empty(t, fleets)
}

func TestLoadRefinedFleets_NonExistentDir(t *testing.T) {
	_, err := LoadRefinedFleets("/nonexistent/path")
	assert.Error(t, err)
}

func TestLoadRefinedFleets_SkipsNonTarballs(t *testing.T) {
	dir := t.TempDir()
	os.WriteFile(filepath.Join(dir, "readme.txt"), []byte("hello"), 0644)
	os.WriteFile(filepath.Join(dir, "data.json"), []byte("{}"), 0644)

	fleets, err := LoadRefinedFleets(dir)
	require.NoError(t, err)
	assert.Empty(t, fleets)
}

func TestLoadRefinedFleets_LoadsTarballs(t *testing.T) {
	dir := t.TempDir()

	snap := map[string]interface{}{
		"meta": map[string]interface{}{
			"hostname": "web-01",
			"fleet": map[string]interface{}{
				"total_hosts": 5.0,
			},
		},
		"rpm": map[string]interface{}{
			"packages_added": []map[string]interface{}{
				{"name": "httpd", "nvra": "httpd-2.4.57-5.el9.x86_64"},
				{"name": "mod_ssl", "nvra": "mod_ssl-2.4.57-5.el9.x86_64"},
			},
			"base_image": "registry.redhat.io/rhel9/rhel-bootc:9.4",
		},
		"config": map[string]interface{}{
			"files": []map[string]interface{}{
				{"path": "/etc/httpd/httpd.conf"},
			},
		},
		"preflight": map[string]interface{}{
			"unavailable":    []string{"pkg-a"},
			"direct_install": []string{"pkg-b"},
			"unverifiable":   []string{"pkg-c"},
			"status":         "passed",
		},
	}

	writeTarball(t, dir, "web-fleet.tar.gz", snap)

	fleets, err := LoadRefinedFleets(dir)
	require.NoError(t, err)
	require.Len(t, fleets, 1)

	f := fleets[0]
	assert.Equal(t, "web-01", f.Name)
	assert.Equal(t, 5, f.HostCount)
	assert.ElementsMatch(t, []string{
		"httpd-2.4.57-5.el9.x86_64",
		"mod_ssl-2.4.57-5.el9.x86_64",
	}, f.Packages)
	assert.Equal(t, []string{"/etc/httpd/httpd.conf"}, f.Configs)
	assert.Equal(t, "registry.redhat.io/rhel9/rhel-bootc:9.4", f.BaseImage)
	assert.Equal(t, []string{"pkg-a"}, f.UnavailablePackages)
	assert.Equal(t, []string{"pkg-b"}, f.DirectInstallPackages)
	assert.Equal(t, []string{"pkg-c"}, f.UnverifiablePackages)
	assert.Equal(t, "passed", f.PreflightStatus)
}

func TestLoadRefinedFleets_MultipleTarballs(t *testing.T) {
	dir := t.TempDir()

	snap1 := map[string]interface{}{
		"meta": map[string]interface{}{"hostname": "web-01"},
		"rpm": map[string]interface{}{
			"packages_added": []map[string]interface{}{
				{"name": "httpd", "nvra": "httpd-2.4.57-5.el9.x86_64"},
			},
		},
	}
	snap2 := map[string]interface{}{
		"meta": map[string]interface{}{"hostname": "db-01"},
		"rpm": map[string]interface{}{
			"packages_added": []map[string]interface{}{
				{"name": "postgresql", "nvra": "postgresql-15.4-1.el9.x86_64"},
			},
		},
	}

	writeTarball(t, dir, "a-web-fleet.tar.gz", snap1)
	writeTarball(t, dir, "b-db-fleet.tar.gz", snap2)

	fleets, err := LoadRefinedFleets(dir)
	require.NoError(t, err)
	require.Len(t, fleets, 2)
	// Sorted by filename
	assert.Equal(t, "web-01", fleets[0].Name)
	assert.Equal(t, "db-01", fleets[1].Name)
}

func TestLoadRefinedFleets_MissingSnapshot(t *testing.T) {
	dir := t.TempDir()

	// Tarball with no inspection-snapshot.json
	tarPath := filepath.Join(dir, "empty.tar.gz")
	f, err := os.Create(tarPath)
	require.NoError(t, err)
	gz := gzip.NewWriter(f)
	tw := tar.NewWriter(gz)
	tw.Close()
	gz.Close()
	f.Close()

	fleets, err := LoadRefinedFleets(dir)
	require.NoError(t, err)
	assert.Empty(t, fleets) // skipped
}

func TestLoadRefinedFleets_FallbackName(t *testing.T) {
	dir := t.TempDir()

	snap := map[string]interface{}{
		"meta": map[string]interface{}{},
		"rpm": map[string]interface{}{
			"packages_added": []map[string]interface{}{
				{"name": "vim", "nvra": ""},
			},
		},
	}
	writeTarball(t, dir, "test.tar.gz", snap)

	fleets, err := LoadRefinedFleets(dir)
	require.NoError(t, err)
	require.Len(t, fleets, 1)
	assert.Equal(t, "unknown", fleets[0].Name)
	// NVRA empty, should use name
	assert.Equal(t, []string{"vim"}, fleets[0].Packages)
}

func TestValidateFleetVersions_SameMajor(t *testing.T) {
	dir := t.TempDir()

	snap1 := map[string]interface{}{
		"meta":       map[string]interface{}{"hostname": "h1"},
		"os_release": map[string]interface{}{"version_id": "9.4"},
	}
	snap2 := map[string]interface{}{
		"meta":       map[string]interface{}{"hostname": "h2"},
		"os_release": map[string]interface{}{"version_id": "9.2"},
	}
	writeTarball(t, dir, "a.tar.gz", snap1)
	writeTarball(t, dir, "b.tar.gz", snap2)

	fleets := []FleetInput{{Name: "h1"}, {Name: "h2"}}
	err := ValidateFleetVersions(fleets, dir)
	assert.NoError(t, err)
}

func TestValidateFleetVersions_MixedMajor(t *testing.T) {
	dir := t.TempDir()

	snap1 := map[string]interface{}{
		"meta":       map[string]interface{}{"hostname": "h1"},
		"os_release": map[string]interface{}{"version_id": "8.9"},
	}
	snap2 := map[string]interface{}{
		"meta":       map[string]interface{}{"hostname": "h2"},
		"os_release": map[string]interface{}{"version_id": "9.4"},
	}
	writeTarball(t, dir, "a.tar.gz", snap1)
	writeTarball(t, dir, "b.tar.gz", snap2)

	fleets := []FleetInput{{Name: "h1"}, {Name: "h2"}}
	err := ValidateFleetVersions(fleets, dir)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "mixed OS major versions")
}

func TestValidateFleetVersions_SingleFleet(t *testing.T) {
	fleets := []FleetInput{{Name: "h1"}}
	err := ValidateFleetVersions(fleets, "/tmp")
	assert.NoError(t, err)
}

// --- helpers ---

func writeTarball(t *testing.T, dir, name string, snapshot interface{}) {
	t.Helper()

	data, err := json.Marshal(snapshot)
	require.NoError(t, err)

	tarPath := filepath.Join(dir, name)
	f, err := os.Create(tarPath)
	require.NoError(t, err)
	defer f.Close()

	gz := gzip.NewWriter(f)
	tw := tar.NewWriter(gz)

	hdr := &tar.Header{
		Name: "inspection-snapshot.json",
		Size: int64(len(data)),
		Mode: 0644,
	}
	require.NoError(t, tw.WriteHeader(hdr))
	_, err = tw.Write(data)
	require.NoError(t, err)

	require.NoError(t, tw.Close())
	require.NoError(t, gz.Close())
}
