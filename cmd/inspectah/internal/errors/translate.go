package errors

import (
	"strings"
)

type pattern struct {
	substr string
	kind   ErrorKind
	msg    string
	hint   string
}

var patterns = []pattern{
	{"executable file not found", ErrPodmanNotFound, "podman is not installed", "Install podman: dnf install podman (RHEL/Fedora) or brew install podman (macOS)"},
	{"permission denied", ErrPermissionDenied, "permission denied", "Run with sudo: sudo inspectah scan"},
	{"mount source path", ErrBindMountFailed, "failed to bind-mount host filesystem", "Check that the source path exists and is readable"},
	{"image not known", ErrImageNotFound, "container image not found locally", "Run: inspectah image update"},
	{"Error: unable to pull", ErrImagePullFailed, "failed to pull container image", "Check network connectivity and image name. For air-gapped environments: inspectah image load <tarball>"},
	{"manifest unknown", ErrImagePullFailed, "container image tag not found in registry", "Check the image version: inspectah image info"},
	{"no such file or directory", ErrOutputPathInvalid, "output path does not exist", "Create the output directory or use -o to specify a valid path"},
}

func TranslateExitError(exitCode int, stderr string) *WrapperError {
	lower := strings.ToLower(stderr)
	for _, p := range patterns {
		if strings.Contains(lower, strings.ToLower(p.substr)) {
			return New(p.kind, p.msg, p.hint, nil)
		}
	}
	if exitCode != 0 {
		return New(ErrContainerFailed, "container exited with an error", "Run with --pull=always to ensure the latest image, or check inspectah output above", nil)
	}
	return nil
}
