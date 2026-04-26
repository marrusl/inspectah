package platform

import (
	"fmt"
	"runtime"
)

var ErrPlatformUnsupported = fmt.Errorf("platform unsupported")

func CheckScanPlatform() error {
	if runtime.GOOS != "linux" {
		return fmt.Errorf("%w: scan requires a Linux host — use on a RHEL, CentOS, or Fedora system", ErrPlatformUnsupported)
	}
	return nil
}

func IsLinux() bool {
	return runtime.GOOS == "linux"
}

func IsMacOS() bool {
	return runtime.GOOS == "darwin"
}
