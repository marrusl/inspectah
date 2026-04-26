package build

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
)

func CrossArchCheck(platform string) ([]string, error) {
	if platform == "" {
		return nil, nil
	}

	parts := strings.SplitN(platform, "/", 2)
	if len(parts) != 2 {
		return nil, fmt.Errorf("invalid --platform format %q — expected os/arch (e.g., linux/amd64)", platform)
	}
	targetArch := parts[1]

	hostArch := runtime.GOARCH
	if hostArch == targetArch {
		return nil, nil
	}

	// On non-Linux hosts (macOS, Windows), podman runs builds inside a Linux VM,
	// so the effective build OS is always linux regardless of runtime.GOOS.
	buildOS := runtime.GOOS
	if buildOS != "linux" {
		buildOS = "linux"
	}

	var warnings []string
	warnings = append(warnings, fmt.Sprintf("Note: Building %s on %s/%s via QEMU — build will be slower.",
		platform, buildOS, hostArch))

	if runtime.GOOS == "linux" {
		binfmtArch := mapArchToBinfmt(targetArch)
		if binfmtArch != "" {
			handler := filepath.Join("/proc/sys/fs/binfmt_misc", "qemu-"+binfmtArch)
			if _, err := os.Stat(handler); err != nil {
				return nil, fmt.Errorf("cross-arch build requires qemu-user-static for %s\n  Install: sudo dnf install qemu-user-static\n  Then:    sudo systemctl restart systemd-binfmt",
					targetArch)
			}
		}
	}

	return warnings, nil
}

func mapArchToBinfmt(goarch string) string {
	switch goarch {
	case "amd64":
		return "x86_64"
	case "arm64":
		return "aarch64"
	case "arm":
		return "arm"
	case "s390x":
		return "s390x"
	case "ppc64le":
		return "ppc64le"
	default:
		return ""
	}
}
