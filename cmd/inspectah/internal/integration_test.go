//go:build integration

package internal

import (
	"bytes"
	"context"
	"os/exec"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/container"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func hasPodman() bool {
	_, err := exec.LookPath("podman")
	return err == nil
}

func TestIntegration_PodmanVersion(t *testing.T) {
	if !hasPodman() {
		t.Skip("podman not available")
	}
	runner := container.NewRealRunner()
	var stdout bytes.Buffer
	code, err := runner.Run(context.Background(), []string{"version", "--format", "{{.Client.Version}}"}, &stdout, &bytes.Buffer{})
	require.NoError(t, err)
	assert.Equal(t, 0, code)
	assert.NotEmpty(t, stdout.String())
	t.Logf("podman version: %s", stdout.String())
}

func TestIntegration_ImageExistsCheck(t *testing.T) {
	if !hasPodman() {
		t.Skip("podman not available")
	}
	runner := container.NewRealRunner()
	err := container.EnsureImage(context.Background(), runner, "does-not-exist:never-tagged", "never", &bytes.Buffer{})
	assert.Error(t, err)
}
