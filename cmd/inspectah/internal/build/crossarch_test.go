package build

import (
	"runtime"
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestCrossArchCheck_SamePlatform(t *testing.T) {
	hostArch := runtime.GOARCH
	platform := "linux/" + hostArch
	warnings, err := CrossArchCheck(platform)
	assert.NoError(t, err)
	assert.Empty(t, warnings)
}

func TestCrossArchCheck_EmptyPlatform(t *testing.T) {
	warnings, err := CrossArchCheck("")
	assert.NoError(t, err)
	assert.Empty(t, warnings)
}

func TestCrossArchCheck_CrossArchWarning(t *testing.T) {
	// Pick an arch that differs from the host
	targetArch := "amd64"
	if runtime.GOARCH == "amd64" {
		targetArch = "arm64"
	}
	platform := "linux/" + targetArch

	warnings, err := CrossArchCheck(platform)
	assert.NoError(t, err)
	assert.Len(t, warnings, 1)

	// Warning must always say "linux/<hostArch>" as the build host,
	// even on macOS/Windows, because podman builds run in a Linux VM.
	assert.Contains(t, warnings[0], "linux/"+runtime.GOARCH)
	if runtime.GOOS != "linux" {
		assert.NotContains(t, warnings[0], runtime.GOOS+"/"+runtime.GOARCH,
			"warning should not use runtime.GOOS on non-Linux hosts")
	}
}

func TestCrossArchCheck_InvalidFormat(t *testing.T) {
	_, err := CrossArchCheck("justanarch")
	assert.ErrorContains(t, err, "format")
}
