package build

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestFormatSuccess(t *testing.T) {
	msg := FormatSuccess("localhost/my-migration:latest")
	assert.Contains(t, msg, "Built: localhost/my-migration:latest")
	assert.Contains(t, msg, "bcvk ephemeral run-ssh")
	assert.Contains(t, msg, "bootc switch")
	assert.Contains(t, msg, "podman push")
}

func TestFormatMissingPodman(t *testing.T) {
	msg := FormatMissingPodman()
	assert.Contains(t, msg, "podman not found")
	assert.Contains(t, msg, "sudo dnf install podman")
	assert.Contains(t, msg, "brew install podman")
}
