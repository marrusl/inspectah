package paths

import (
	"fmt"
	"os"
	"path/filepath"
)

const ContainerHostRoot = "/host"

func HostToContainer(hostPath string) string {
	return filepath.Join(ContainerHostRoot, hostPath)
}

func ResolveOutputDir(flag string) (string, error) {
	dir := flag
	if dir == "" {
		var err error
		dir, err = os.Getwd()
		if err != nil {
			return "", fmt.Errorf("cannot determine working directory: %w", err)
		}
	}

	abs, err := filepath.Abs(dir)
	if err != nil {
		return "", fmt.Errorf("cannot resolve output path: %w", err)
	}

	info, err := os.Stat(abs)
	if err != nil {
		return "", fmt.Errorf("output directory does not exist: %s", abs)
	}
	if !info.IsDir() {
		return "", fmt.Errorf("output path is not a directory: %s", abs)
	}

	return abs, nil
}
