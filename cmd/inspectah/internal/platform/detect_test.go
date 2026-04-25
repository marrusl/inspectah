package platform

import (
	"errors"
	"runtime"
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestCheckScanPlatform(t *testing.T) {
	err := CheckScanPlatform()
	if runtime.GOOS == "linux" {
		assert.NoError(t, err)
	} else {
		assert.Error(t, err)
		assert.True(t, errors.Is(err, ErrPlatformUnsupported))
		assert.Contains(t, err.Error(), "scan requires a Linux host")
	}
}

func TestIsLinux(t *testing.T) {
	assert.Equal(t, runtime.GOOS == "linux", IsLinux())
}
