// cmd/inspectah/internal/build/input_test.go
package build

import (
	"archive/tar"
	"compress/gzip"
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func createTestTarball(t *testing.T, entries []tarEntry) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "test.tar.gz")
	f, err := os.Create(path)
	require.NoError(t, err)
	defer f.Close()

	gw := gzip.NewWriter(f)
	defer gw.Close()
	tw := tar.NewWriter(gw)
	defer tw.Close()

	for _, e := range entries {
		hdr := &tar.Header{
			Name:     e.Name,
			Typeflag: e.Type,
			Size:     int64(len(e.Body)),
			Mode:     0644,
		}
		if e.Linkname != "" {
			hdr.Linkname = e.Linkname
		}
		require.NoError(t, tw.WriteHeader(hdr))
		if len(e.Body) > 0 {
			_, err := tw.Write([]byte(e.Body))
			require.NoError(t, err)
		}
	}
	return path
}

type tarEntry struct {
	Name     string
	Type     byte
	Body     string
	Linkname string
}

func TestResolveInput_Tarball(t *testing.T) {
	tb := createTestTarball(t, []tarEntry{
		{Name: "host/Containerfile", Type: tar.TypeReg, Body: "FROM fedora:43\n"},
		{Name: "host/config/", Type: tar.TypeDir},
	})
	result, cleanup, err := ResolveInput(tb)
	require.NoError(t, err)
	defer cleanup()

	assert.DirExists(t, result.Dir)
	assert.FileExists(t, filepath.Join(result.Dir, "Containerfile"))
	assert.True(t, result.IsTarball)
}

func TestResolveInput_Directory(t *testing.T) {
	dir := t.TempDir()
	os.WriteFile(filepath.Join(dir, "Containerfile"), []byte("FROM fedora:43\n"), 0644)

	result, cleanup, err := ResolveInput(dir)
	require.NoError(t, err)
	defer cleanup()

	assert.Equal(t, dir, result.Dir)
	assert.False(t, result.IsTarball)
}

func TestResolveInput_MissingContainerfile(t *testing.T) {
	dir := t.TempDir()
	_, _, err := ResolveInput(dir)
	assert.ErrorContains(t, err, "No Containerfile found")
}

func TestResolveInput_RejectsPathTraversal(t *testing.T) {
	tb := createTestTarball(t, []tarEntry{
		{Name: "../escape.txt", Type: tar.TypeReg, Body: "evil"},
	})
	_, _, err := ResolveInput(tb)
	assert.ErrorContains(t, err, "path traversal")
}

func TestResolveInput_RejectsAbsolutePath(t *testing.T) {
	tb := createTestTarball(t, []tarEntry{
		{Name: "/etc/passwd", Type: tar.TypeReg, Body: "evil"},
	})
	_, _, err := ResolveInput(tb)
	assert.ErrorContains(t, err, "absolute path")
}

func TestResolveInput_RejectsSymlink(t *testing.T) {
	tb := createTestTarball(t, []tarEntry{
		{Name: "link", Type: tar.TypeSymlink, Linkname: "/etc/passwd"},
	})
	_, _, err := ResolveInput(tb)
	assert.ErrorContains(t, err, "symlink")
}

func TestResolveInput_RejectsHardlink(t *testing.T) {
	tb := createTestTarball(t, []tarEntry{
		{Name: "link", Type: tar.TypeLink, Linkname: "/etc/passwd"},
	})
	_, _, err := ResolveInput(tb)
	assert.ErrorContains(t, err, "hard link")
}

func TestResolveInput_RejectsPostStripDuplicate(t *testing.T) {
	tb := createTestTarball(t, []tarEntry{
		{Name: "host/Containerfile", Type: tar.TypeReg, Body: "FROM fedora:43\n"},
		{Name: "other/Containerfile", Type: tar.TypeReg, Body: "FROM evil:latest\n"},
	})
	_, _, err := ResolveInput(tb)
	assert.ErrorContains(t, err, "duplicate path")
}

func TestResolveInput_RejectsDeviceNode(t *testing.T) {
	tb := createTestTarball(t, []tarEntry{
		{Name: "dev", Type: tar.TypeBlock},
	})
	_, _, err := ResolveInput(tb)
	assert.ErrorContains(t, err, "unsupported entry type")
}
