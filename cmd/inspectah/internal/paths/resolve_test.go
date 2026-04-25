package paths

import (
	"os"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestHostToContainer(t *testing.T) {
	assert.Equal(t, "/host/etc/ssh/sshd_config", HostToContainer("/etc/ssh/sshd_config"))
	assert.Equal(t, "/host/var/log", HostToContainer("/var/log"))
}

func TestResolveOutputDir_CWD(t *testing.T) {
	dir, err := ResolveOutputDir("")
	require.NoError(t, err)
	cwd, _ := os.Getwd()
	assert.Equal(t, cwd, dir)
}

func TestResolveOutputDir_Explicit(t *testing.T) {
	dir, err := ResolveOutputDir(os.TempDir())
	require.NoError(t, err)
	assert.NotEmpty(t, dir)
}

func TestResolveOutputDir_NonExistent(t *testing.T) {
	_, err := ResolveOutputDir("/nonexistent/path/that/should/not/exist")
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "does not exist")
}

func TestResolveOutputDir_NotADir(t *testing.T) {
	f, err := os.CreateTemp("", "inspectah-test-*")
	require.NoError(t, err)
	defer os.Remove(f.Name())
	f.Close()

	_, err = ResolveOutputDir(f.Name())
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "not a directory")
}
