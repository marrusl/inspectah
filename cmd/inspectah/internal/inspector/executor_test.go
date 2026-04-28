package inspector

import (
	"os"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestFakeExecutor_Run(t *testing.T) {
	fake := NewFakeExecutor(map[string]ExecResult{
		"bootc status": {Stdout: "bootc is active", Stderr: "", ExitCode: 0},
		"rpm -qa":      {Stdout: "pkg1\npkg2\n", Stderr: "", ExitCode: 0},
	})

	t.Run("known command returns canned result", func(t *testing.T) {
		result := fake.Run("bootc", "status")
		assert.Equal(t, 0, result.ExitCode)
		assert.Equal(t, "bootc is active", result.Stdout)
		assert.Equal(t, "", result.Stderr)
	})

	t.Run("another known command", func(t *testing.T) {
		result := fake.Run("rpm", "-qa")
		assert.Equal(t, 0, result.ExitCode)
		assert.Equal(t, "pkg1\npkg2\n", result.Stdout)
	})
}

func TestFakeExecutor_UnknownCommand(t *testing.T) {
	fake := NewFakeExecutor(nil)

	result := fake.Run("nonexistent", "arg1")
	assert.Equal(t, 127, result.ExitCode)
	assert.Equal(t, "", result.Stdout)
	assert.Contains(t, result.Stderr, "unknown command")
}

func TestFakeExecutor_WithFiles(t *testing.T) {
	fake := NewFakeExecutor(nil).WithFiles(map[string]string{
		"/etc/os-release": "ID=rhel\nVERSION_ID=9.4\n",
		"/ostree":         "", // exists but empty (directory marker)
	})

	t.Run("ReadFile returns content", func(t *testing.T) {
		content, err := fake.ReadFile("/etc/os-release")
		require.NoError(t, err)
		assert.Equal(t, "ID=rhel\nVERSION_ID=9.4\n", content)
	})

	t.Run("ReadFile returns error for missing file", func(t *testing.T) {
		_, err := fake.ReadFile("/nonexistent")
		assert.Error(t, err)
	})

	t.Run("FileExists returns true for known path", func(t *testing.T) {
		assert.True(t, fake.FileExists("/ostree"))
	})

	t.Run("FileExists returns false for unknown path", func(t *testing.T) {
		assert.False(t, fake.FileExists("/no-such-path"))
	})

	t.Run("ReadDir lists entries", func(t *testing.T) {
		// WithDirs adds virtual directory entries
		fake2 := NewFakeExecutor(nil).WithDirs(map[string][]string{
			"/etc/yum.repos.d": {"redhat.repo", "epel.repo"},
		})
		entries, err := fake2.ReadDir("/etc/yum.repos.d")
		require.NoError(t, err)
		require.Len(t, entries, 2)
		names := []string{entries[0].Name(), entries[1].Name()}
		assert.Contains(t, names, "redhat.repo")
		assert.Contains(t, names, "epel.repo")
	})

	t.Run("ReadDir returns error for unknown path", func(t *testing.T) {
		_, err := fake.ReadDir("/nonexistent")
		assert.Error(t, err)
	})
}

func TestFakeExecutor_HostRoot(t *testing.T) {
	fake := NewFakeExecutor(nil)
	// FakeExecutor always returns "/" as host root
	assert.Equal(t, "/", fake.HostRoot())
}

func TestRealExecutor_HostRoot(t *testing.T) {
	real := NewRealExecutor("/sysroot")
	assert.Equal(t, "/sysroot", real.HostRoot())
}

func TestRealExecutor_FileExists(t *testing.T) {
	// Use the real filesystem -- /tmp should always exist
	tmpDir := t.TempDir()
	real := NewRealExecutor(tmpDir)

	t.Run("existing file", func(t *testing.T) {
		f, err := os.CreateTemp(tmpDir, "test-*")
		require.NoError(t, err)
		f.Close()
		// Path relative to host root
		relPath := "/" + f.Name()[len(tmpDir):]
		assert.True(t, real.FileExists(relPath))
	})

	t.Run("missing file", func(t *testing.T) {
		assert.False(t, real.FileExists("/definitely-not-here-"+t.Name()))
	})
}

func TestRealExecutor_ReadFile(t *testing.T) {
	tmpDir := t.TempDir()
	real := NewRealExecutor(tmpDir)

	content := "hello world\n"
	path := tmpDir + "/testfile.txt"
	require.NoError(t, os.WriteFile(path, []byte(content), 0644))

	got, err := real.ReadFile("/testfile.txt")
	require.NoError(t, err)
	assert.Equal(t, content, got)
}

func TestRealExecutor_Run(t *testing.T) {
	real := NewRealExecutor("/")

	result := real.Run("echo", "hello")
	assert.Equal(t, 0, result.ExitCode)
	assert.Equal(t, "hello\n", result.Stdout)
}
