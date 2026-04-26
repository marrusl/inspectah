// cmd/inspectah/internal/build/input.go
package build

import (
	"archive/tar"
	"compress/gzip"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
)

type InputResult struct {
	Dir       string
	IsTarball bool
}

func ResolveInput(path string) (*InputResult, func(), error) {
	noop := func() {}

	if isTarball(path) {
		return extractTarball(path)
	}

	info, err := os.Stat(path)
	if err != nil {
		return nil, noop, fmt.Errorf("input path %q does not exist — this should be an inspectah scan tarball or extracted directory", path)
	}
	if !info.IsDir() {
		return nil, noop, fmt.Errorf("input path %q is not a directory or tarball", path)
	}

	if err := validateContainerfile(path); err != nil {
		return nil, noop, err
	}
	return &InputResult{Dir: path, IsTarball: false}, noop, nil
}

func isTarball(path string) bool {
	return strings.HasSuffix(path, ".tar.gz") || strings.HasSuffix(path, ".tgz")
}

func extractTarball(path string) (*InputResult, func(), error) {
	noop := func() {}

	f, err := os.Open(path)
	if err != nil {
		return nil, noop, fmt.Errorf("cannot open tarball: %w", err)
	}
	defer f.Close()

	homeDir, err := os.UserHomeDir()
	if err != nil {
		homeDir = os.TempDir()
	}
	cacheDir := filepath.Join(homeDir, ".cache", "inspectah")
	if err := os.MkdirAll(cacheDir, 0755); err != nil {
		return nil, noop, fmt.Errorf("cannot create cache directory: %w", err)
	}
	extractDir, err := os.MkdirTemp(cacheDir, "build-")
	if err != nil {
		return nil, noop, fmt.Errorf("cannot create temp directory: %w", err)
	}
	cleanup := func() { os.RemoveAll(extractDir) }

	gr, err := gzip.NewReader(f)
	if err != nil {
		cleanup()
		return nil, noop, fmt.Errorf("cannot decompress tarball: %w", err)
	}
	defer gr.Close()

	tr := tar.NewReader(gr)
	seen := make(map[string]bool)

	for {
		hdr, err := tr.Next()
		if err == io.EOF {
			break
		}
		if err != nil {
			cleanup()
			return nil, noop, fmt.Errorf("corrupt tarball: %w", err)
		}

		// Validate entry type first (rejects symlinks, hardlinks, devices)
		if err := validateEntryType(hdr); err != nil {
			cleanup()
			return nil, noop, err
		}

		// Validate the raw path before stripping
		if err := validateRawPath(hdr.Name); err != nil {
			cleanup()
			return nil, noop, err
		}

		// Compute final extraction target and validate it
		stripped := stripTopLevel(hdr.Name)
		if stripped == "" || stripped == "." {
			continue
		}
		target := filepath.Join(extractDir, stripped)
		if err := validateTarget(target, extractDir, seen); err != nil {
			cleanup()
			return nil, noop, err
		}

		switch hdr.Typeflag {
		case tar.TypeDir:
			if err := os.MkdirAll(target, 0755); err != nil {
				cleanup()
				return nil, noop, err
			}
		case tar.TypeReg:
			if err := os.MkdirAll(filepath.Dir(target), 0755); err != nil {
				cleanup()
				return nil, noop, err
			}
			out, err := os.OpenFile(target, os.O_CREATE|os.O_WRONLY, os.FileMode(hdr.Mode))
			if err != nil {
				cleanup()
				return nil, noop, err
			}
			if _, err := io.Copy(out, tr); err != nil {
				out.Close()
				cleanup()
				return nil, noop, err
			}
			out.Close()
		}

		seen[filepath.Clean(target)] = true
	}

	if err := validateContainerfile(extractDir); err != nil {
		cleanup()
		return nil, noop, err
	}

	return &InputResult{Dir: extractDir, IsTarball: true}, cleanup, nil
}

func validateEntryType(hdr *tar.Header) error {
	switch hdr.Typeflag {
	case tar.TypeReg, tar.TypeDir:
		return nil
	case tar.TypeSymlink:
		return fmt.Errorf("archive safety: symlink %q rejected", hdr.Name)
	case tar.TypeLink:
		return fmt.Errorf("archive safety: hard link %q rejected", hdr.Name)
	default:
		return fmt.Errorf("archive safety: unsupported entry type %d for %q", hdr.Typeflag, hdr.Name)
	}
}

func validateRawPath(name string) error {
	// Reject absolute paths
	if filepath.IsAbs(name) {
		return fmt.Errorf("archive safety: absolute path rejected")
	}

	// Reject path traversal patterns
	cleaned := filepath.Clean(name)
	if strings.HasPrefix(cleaned, "..") || strings.Contains(cleaned, string(filepath.Separator)+"..") {
		return fmt.Errorf("archive safety: path traversal rejected")
	}

	return nil
}

func validateTarget(target, root string, seen map[string]bool) error {
	cleaned := filepath.Clean(target)

	if filepath.IsAbs(cleaned) && !strings.HasPrefix(cleaned, root+string(filepath.Separator)) {
		return fmt.Errorf("archive safety: absolute path rejected")
	}

	if !strings.HasPrefix(cleaned, root+string(filepath.Separator)) && cleaned != root {
		return fmt.Errorf("archive safety: path traversal rejected — resolved to %q", cleaned)
	}

	if seen[cleaned] {
		return fmt.Errorf("archive safety: duplicate path %q rejected", cleaned)
	}

	return nil
}

func stripTopLevel(name string) string {
	parts := strings.SplitN(name, "/", 2)
	if len(parts) < 2 {
		return name
	}
	return parts[1]
}

func validateContainerfile(dir string) error {
	cf := filepath.Join(dir, "Containerfile")
	if _, err := os.Stat(cf); err != nil {
		return fmt.Errorf("No Containerfile found in %s\n  This doesn't look like inspectah output. Run 'inspectah scan' first.", dir)
	}
	return nil
}
