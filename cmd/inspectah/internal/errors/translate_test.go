package errors

import (
	"bytes"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestTranslateExitError_PodmanNotFound(t *testing.T) {
	err := TranslateExitError(127, `exec: "podman": executable file not found in $PATH`)
	require.NotNil(t, err)
	assert.Equal(t, ErrPodmanNotFound, err.Kind)
	assert.Contains(t, err.Hint, "Install podman")
}

func TestTranslateExitError_PermissionDenied(t *testing.T) {
	err := TranslateExitError(1, "Error: permission denied while trying to connect to the Podman socket")
	require.NotNil(t, err)
	assert.Equal(t, ErrPermissionDenied, err.Kind)
	assert.Contains(t, err.Hint, "sudo")
}

func TestTranslateExitError_ImageNotFound(t *testing.T) {
	err := TranslateExitError(125, "Error: ghcr.io/marrusl/inspectah:latest: image not known")
	require.NotNil(t, err)
	assert.Equal(t, ErrImageNotFound, err.Kind)
}

func TestTranslateExitError_PullFailed(t *testing.T) {
	err := TranslateExitError(125, "Error: unable to pull ghcr.io/marrusl/inspectah:v0.5.1")
	require.NotNil(t, err)
	assert.Equal(t, ErrImagePullFailed, err.Kind)
}

func TestTranslateExitError_ManifestUnknown(t *testing.T) {
	err := TranslateExitError(125, "manifest unknown: manifest unknown")
	require.NotNil(t, err)
	assert.Equal(t, ErrImagePullFailed, err.Kind)
}

func TestTranslateExitError_GenericFailure(t *testing.T) {
	err := TranslateExitError(1, "something unexpected happened")
	require.NotNil(t, err)
	assert.Equal(t, ErrContainerFailed, err.Kind)
}

func TestTranslateExitError_Success(t *testing.T) {
	err := TranslateExitError(0, "")
	assert.Nil(t, err)
}

func TestRender(t *testing.T) {
	var buf bytes.Buffer
	err := New(ErrPodmanNotFound, "podman is not installed", "Install podman: dnf install podman", nil)
	Render(&buf, err)
	assert.Contains(t, buf.String(), "Error: podman is not installed")
	assert.Contains(t, buf.String(), "Hint: Install podman")
}
