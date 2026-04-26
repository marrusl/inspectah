package cli

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestBuildCmd_Exists(t *testing.T) {
	cmd := newBuildCmd()
	assert.Equal(t, "build", cmd.Use[:5])
	assert.Contains(t, cmd.Short, "Build")
}

func TestBuildCmd_Flags(t *testing.T) {
	cmd := newBuildCmd()
	f := cmd.Flags()
	assert.NotNil(t, f.Lookup("tag"))
	assert.NotNil(t, f.Lookup("platform"))
	assert.NotNil(t, f.Lookup("entitlements-dir"))
	assert.NotNil(t, f.Lookup("no-entitlements"))
	assert.NotNil(t, f.Lookup("ignore-expired-certs"))
	assert.NotNil(t, f.Lookup("no-cache"))
	assert.NotNil(t, f.Lookup("pull"))
	assert.NotNil(t, f.Lookup("dry-run"))
	assert.NotNil(t, f.Lookup("verbose"))
}

func TestBuildCmd_RequiresInput(t *testing.T) {
	cmd := newBuildCmd()
	cmd.SetArgs([]string{})
	err := cmd.Execute()
	assert.Error(t, err)
	assert.ErrorContains(t, err, "requires")
}

func TestBuildCmd_AcceptsExtraArgs(t *testing.T) {
	cmd := newBuildCmd()
	err := cmd.Args(cmd, []string{"test.tar.gz"})
	assert.NoError(t, err)

	err = cmd.Args(cmd, []string{"test.tar.gz", "--build-arg", "FOO=bar"})
	assert.NoError(t, err)
}

func TestBuildCmd_TagShorthand(t *testing.T) {
	cmd := newBuildCmd()
	f := cmd.Flags().Lookup("tag")
	assert.Equal(t, "t", f.Shorthand)
}

func TestBuildCmd_MutualExclusion(t *testing.T) {
	cmd := newBuildCmd()
	cmd.SetArgs([]string{"test.tar.gz", "-t", "img:latest",
		"--no-entitlements", "--entitlements-dir", "/some/path"})
	err := cmd.Execute()
	assert.ErrorContains(t, err, "mutually exclusive")
}

func TestBuildCmd_AbsoluteContainerfilePath(t *testing.T) {
	// Regression: build must use the Containerfile from the output dir,
	// not an absolute path that leaks the extraction location.
	dir := t.TempDir()
	require.NoError(t, os.WriteFile(filepath.Join(dir, "Containerfile"), []byte("FROM fedora:43\n"), 0644))

	// Capture os.Stderr since dry-run writes there directly
	origStderr := os.Stderr
	r, w, err := os.Pipe()
	require.NoError(t, err)
	os.Stderr = w

	cmd := newBuildCmd()
	cmd.SetArgs([]string{dir, "-t", "test:latest", "--dry-run"})
	execErr := cmd.Execute()

	w.Close()
	os.Stderr = origStderr

	var buf bytes.Buffer
	buf.ReadFrom(r)
	r.Close()

	require.NoError(t, execErr)

	output := buf.String()
	assert.Contains(t, output, "-f "+filepath.Join(dir, "Containerfile"))
}
