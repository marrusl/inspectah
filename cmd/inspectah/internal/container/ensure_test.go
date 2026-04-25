package container

import (
	"bytes"
	"context"
	"fmt"
	"io"
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestEnsureImage_Missing_ImageExists(t *testing.T) {
	fake := &FakeRunner{
		RunFunc: func(ctx context.Context, args []string, stdout, stderr io.Writer) (int, error) {
			if args[0] == "image" && args[1] == "exists" {
				return 0, nil
			}
			return 1, nil
		},
	}
	var buf bytes.Buffer
	err := EnsureImage(context.Background(), fake, "test:latest", "missing", &buf)
	require.NoError(t, err)
	assert.Empty(t, buf.String())
}

func TestEnsureImage_Missing_NeedssPull(t *testing.T) {
	callCount := 0
	fake := &FakeRunner{
		RunFunc: func(ctx context.Context, args []string, stdout, stderr io.Writer) (int, error) {
			callCount++
			if args[0] == "image" && args[1] == "exists" {
				return 1, nil
			}
			if args[0] == "pull" {
				return 0, nil
			}
			return 1, nil
		},
	}
	var buf bytes.Buffer
	err := EnsureImage(context.Background(), fake, "test:latest", "missing", &buf)
	require.NoError(t, err)
	assert.Contains(t, buf.String(), "Pulling test:latest")
	assert.Contains(t, buf.String(), "Ready.")
}

func TestEnsureImage_Always_Pulls(t *testing.T) {
	fake := &FakeRunner{
		RunFunc: func(ctx context.Context, args []string, stdout, stderr io.Writer) (int, error) {
			if args[0] == "pull" {
				return 0, nil
			}
			return 0, nil
		},
	}
	var buf bytes.Buffer
	err := EnsureImage(context.Background(), fake, "test:latest", "always", &buf)
	require.NoError(t, err)
	assert.Contains(t, buf.String(), "Pulling")
}

func TestEnsureImage_Never_Missing(t *testing.T) {
	fake := &FakeRunner{
		RunFunc: func(ctx context.Context, args []string, stdout, stderr io.Writer) (int, error) {
			return 1, nil
		},
	}
	var buf bytes.Buffer
	err := EnsureImage(context.Background(), fake, "test:latest", "never", &buf)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "not found locally")
}

func TestEnsureImage_PullFailed_ManifestUnknown(t *testing.T) {
	fake := &FakeRunner{
		RunFunc: func(ctx context.Context, args []string, stdout, stderr io.Writer) (int, error) {
			if args[0] == "pull" {
				fmt.Fprint(stderr, "manifest unknown: manifest unknown to registry")
				return 1, nil
			}
			return 1, nil
		},
	}
	var buf bytes.Buffer
	err := EnsureImage(context.Background(), fake, "test:v99", "always", &buf)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "tag not found")
}

func TestStreamPullProgress(t *testing.T) {
	input := strings.NewReader("Copying blob abc123\nSome other line\nWriting manifest\nStoring signatures\n")
	var buf bytes.Buffer
	StreamPullProgress(input, &buf)
	assert.Contains(t, buf.String(), "Copying blob")
	assert.Contains(t, buf.String(), "Writing manifest")
	assert.Contains(t, buf.String(), "Storing signatures")
	assert.NotContains(t, buf.String(), "Some other line")
}
