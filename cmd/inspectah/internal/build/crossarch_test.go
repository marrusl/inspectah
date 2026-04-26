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

func TestCrossArchCheck_InvalidFormat(t *testing.T) {
	_, err := CrossArchCheck("justanarch")
	assert.ErrorContains(t, err, "format")
}
