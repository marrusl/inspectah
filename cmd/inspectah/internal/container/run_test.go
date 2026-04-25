package container

import (
	"context"
	"io"
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestResolveImage_FlagWins(t *testing.T) {
	img := ResolveImage("flag-image", "env-image", "pinned", "default")
	assert.Equal(t, "flag-image", img)
}

func TestResolveImage_EnvFallback(t *testing.T) {
	img := ResolveImage("", "env-image", "pinned", "default")
	assert.Equal(t, "env-image", img)
}

func TestResolveImage_PinnedFallback(t *testing.T) {
	img := ResolveImage("", "", "pinned", "default")
	assert.Equal(t, "pinned", img)
}

func TestResolveImage_DefaultFallback(t *testing.T) {
	img := ResolveImage("", "", "", "default")
	assert.Equal(t, "default", img)
}

func TestLoadPinnedImage_NoFile(t *testing.T) {
	t.Setenv("XDG_CONFIG_HOME", t.TempDir())
	assert.Equal(t, "", LoadPinnedImage())
}

func TestSaveThenLoadPinnedImage(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("XDG_CONFIG_HOME", dir)
	require.NoError(t, SavePinnedImage("ghcr.io/marrusl/inspectah:0.5.1"))

	loaded := LoadPinnedImage()
	assert.Equal(t, "ghcr.io/marrusl/inspectah:0.5.1", loaded)

	data, _ := os.ReadFile(filepath.Join(dir, "inspectah", "config.json"))
	assert.Contains(t, string(data), "pinned_image")
}

func TestFakeRunner_RecordsCalls(t *testing.T) {
	fake := &FakeRunner{}
	code, err := fake.Run(context.Background(), []string{"run", "--rm", "test"}, io.Discard, io.Discard)
	require.NoError(t, err)
	assert.Equal(t, 0, code)
	assert.Len(t, fake.Calls, 1)
	assert.Equal(t, []string{"run", "--rm", "test"}, fake.Calls[0])
}

func TestFakeRunner_CustomRunFunc(t *testing.T) {
	fake := &FakeRunner{
		RunFunc: func(ctx context.Context, args []string, stdout, stderr io.Writer) (int, error) {
			stderr.Write([]byte("permission denied"))
			return 1, nil
		},
	}
	code, err := fake.Run(context.Background(), []string{"run", "test"}, io.Discard, io.Discard)
	require.NoError(t, err)
	assert.Equal(t, 1, code)
}
