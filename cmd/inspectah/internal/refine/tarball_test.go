package refine

import (
	"archive/tar"
	"compress/gzip"
	"io"
	"os"
	"path/filepath"
	"sort"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// helper: create a .tar.gz with the given file map (path -> content).
func createTestTarball(t *testing.T, files map[string]string) string {
	t.Helper()
	f, err := os.CreateTemp(t.TempDir(), "test-*.tar.gz")
	require.NoError(t, err)
	defer f.Close()

	gw := gzip.NewWriter(f)
	tw := tar.NewWriter(gw)

	for name, content := range files {
		hdr := &tar.Header{
			Name: name,
			Mode: 0644,
			Size: int64(len(content)),
		}
		require.NoError(t, tw.WriteHeader(hdr))
		_, err := tw.Write([]byte(content))
		require.NoError(t, err)
	}

	require.NoError(t, tw.Close())
	require.NoError(t, gw.Close())
	return f.Name()
}

func TestExtractTarball(t *testing.T) {
	files := map[string]string{
		"report.html":              "<html>test</html>",
		"inspection-snapshot.json": `{"meta":{}}`,
		"Containerfile":            "FROM ubi9",
	}
	tarball := createTestTarball(t, files)
	dest := t.TempDir()

	err := ExtractTarball(tarball, dest)
	require.NoError(t, err)

	for name, content := range files {
		data, err := os.ReadFile(filepath.Join(dest, name))
		require.NoError(t, err)
		assert.Equal(t, content, string(data))
	}
}

func TestExtractTarball_StripsPathTraversal(t *testing.T) {
	// Create a tarball with a path traversal attempt
	f, err := os.CreateTemp(t.TempDir(), "traversal-*.tar.gz")
	require.NoError(t, err)
	defer f.Close()

	gw := gzip.NewWriter(f)
	tw := tar.NewWriter(gw)

	// Entry with ".." in path should be sanitized
	hdr := &tar.Header{
		Name: "../../../etc/passwd",
		Mode: 0644,
		Size: 4,
	}
	require.NoError(t, tw.WriteHeader(hdr))
	_, err = tw.Write([]byte("evil"))
	require.NoError(t, err)

	// Normal entry
	hdr = &tar.Header{
		Name: "report.html",
		Mode: 0644,
		Size: 6,
	}
	require.NoError(t, tw.WriteHeader(hdr))
	_, err = tw.Write([]byte("<html>"))
	require.NoError(t, err)

	require.NoError(t, tw.Close())
	require.NoError(t, gw.Close())

	dest := t.TempDir()
	err = ExtractTarball(f.Name(), dest)
	require.NoError(t, err)

	// The traversal entry should be stripped — "etc/passwd" at most
	assert.NoFileExists(t, filepath.Join(dest, "..", "..", "..", "etc", "passwd"))
	// Normal file should exist
	assert.FileExists(t, filepath.Join(dest, "report.html"))
}

func TestExtractTarball_InvalidFile(t *testing.T) {
	err := ExtractTarball("/nonexistent/file.tar.gz", t.TempDir())
	assert.Error(t, err)
}

func TestExtractTarball_NotGzip(t *testing.T) {
	notGzip := filepath.Join(t.TempDir(), "bad.tar.gz")
	require.NoError(t, os.WriteFile(notGzip, []byte("not a tarball"), 0644))

	err := ExtractTarball(notGzip, t.TempDir())
	assert.Error(t, err)
}

func TestRepackTarball(t *testing.T) {
	// Set up a source directory
	srcDir := t.TempDir()
	require.NoError(t, os.WriteFile(filepath.Join(srcDir, "report.html"), []byte("<html>repacked</html>"), 0644))
	require.NoError(t, os.MkdirAll(filepath.Join(srcDir, "config"), 0755))
	require.NoError(t, os.WriteFile(filepath.Join(srcDir, "config", "test.conf"), []byte("key=val"), 0644))

	destPath := filepath.Join(t.TempDir(), "output.tar.gz")
	err := RepackTarball(srcDir, destPath)
	require.NoError(t, err)

	// Verify the tarball can be extracted and has correct contents
	verifyDir := t.TempDir()
	err = ExtractTarball(destPath, verifyDir)
	require.NoError(t, err)

	data, err := os.ReadFile(filepath.Join(verifyDir, "report.html"))
	require.NoError(t, err)
	assert.Equal(t, "<html>repacked</html>", string(data))

	data, err = os.ReadFile(filepath.Join(verifyDir, "config", "test.conf"))
	require.NoError(t, err)
	assert.Equal(t, "key=val", string(data))
}

func TestRepackTarball_EmptyDir(t *testing.T) {
	srcDir := t.TempDir()
	destPath := filepath.Join(t.TempDir(), "empty.tar.gz")

	err := RepackTarball(srcDir, destPath)
	require.NoError(t, err)

	// Should produce a valid tarball with no entries
	f, err := os.Open(destPath)
	require.NoError(t, err)
	defer f.Close()

	gr, err := gzip.NewReader(f)
	require.NoError(t, err)
	defer gr.Close()

	tr := tar.NewReader(gr)
	_, err = tr.Next()
	assert.Equal(t, io.EOF, err)
}

func TestRepackTarball_PreservesOrder(t *testing.T) {
	srcDir := t.TempDir()
	names := []string{"aaa.txt", "bbb.txt", "ccc.txt"}
	for _, name := range names {
		require.NoError(t, os.WriteFile(filepath.Join(srcDir, name), []byte(name), 0644))
	}

	destPath := filepath.Join(t.TempDir(), "ordered.tar.gz")
	err := RepackTarball(srcDir, destPath)
	require.NoError(t, err)

	// Read entries and verify sorted order
	f, err := os.Open(destPath)
	require.NoError(t, err)
	defer f.Close()

	gr, err := gzip.NewReader(f)
	require.NoError(t, err)
	defer gr.Close()

	tr := tar.NewReader(gr)
	var got []string
	for {
		hdr, err := tr.Next()
		if err == io.EOF {
			break
		}
		require.NoError(t, err)
		if hdr.Typeflag == tar.TypeReg {
			got = append(got, hdr.Name)
		}
	}

	sorted := make([]string, len(got))
	copy(sorted, got)
	sort.Strings(sorted)
	assert.Equal(t, sorted, got)
}
