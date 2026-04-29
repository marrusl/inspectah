package refine

import (
	"archive/tar"
	"compress/gzip"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

// ExtractTarball extracts a .tar.gz file into destDir.
// Path traversal attempts (entries containing "..") are sanitized.
func ExtractTarball(path, destDir string) error {
	f, err := os.Open(path)
	if err != nil {
		return fmt.Errorf("open tarball: %w", err)
	}
	defer f.Close()

	gr, err := gzip.NewReader(f)
	if err != nil {
		return fmt.Errorf("gzip reader: %w", err)
	}
	defer gr.Close()

	tr := tar.NewReader(gr)
	for {
		hdr, err := tr.Next()
		if err == io.EOF {
			break
		}
		if err != nil {
			return fmt.Errorf("read tar entry: %w", err)
		}

		// Sanitize path: strip ".." components and leading slashes
		safeName := sanitizeTarPath(hdr.Name)
		if safeName == "" {
			continue
		}

		target := filepath.Join(destDir, safeName)

		// Ensure the target stays within destDir
		if !strings.HasPrefix(filepath.Clean(target), filepath.Clean(destDir)+string(os.PathSeparator)) &&
			filepath.Clean(target) != filepath.Clean(destDir) {
			continue
		}

		switch hdr.Typeflag {
		case tar.TypeDir:
			if err := os.MkdirAll(target, 0755); err != nil {
				return fmt.Errorf("create dir %s: %w", safeName, err)
			}
		case tar.TypeReg:
			if err := os.MkdirAll(filepath.Dir(target), 0755); err != nil {
				return fmt.Errorf("create parent dir for %s: %w", safeName, err)
			}
			out, err := os.OpenFile(target, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, os.FileMode(hdr.Mode&0777))
			if err != nil {
				return fmt.Errorf("create file %s: %w", safeName, err)
			}
			if _, err := io.Copy(out, tr); err != nil {
				out.Close()
				return fmt.Errorf("write file %s: %w", safeName, err)
			}
			out.Close()
		}
	}

	return nil
}

// RepackTarball creates a .tar.gz at destPath from the contents of srcDir.
// Files are sorted for deterministic output.
func RepackTarball(srcDir, destPath string) error {
	f, err := os.Create(destPath)
	if err != nil {
		return fmt.Errorf("create tarball: %w", err)
	}
	defer f.Close()

	gw := gzip.NewWriter(f)
	tw := tar.NewWriter(gw)

	// Collect all paths first for sorted iteration
	var paths []string
	err = filepath.Walk(srcDir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		if path == srcDir {
			return nil
		}
		paths = append(paths, path)
		return nil
	})
	if err != nil {
		tw.Close()
		gw.Close()
		return fmt.Errorf("walk source dir: %w", err)
	}

	sort.Strings(paths)

	for _, path := range paths {
		info, err := os.Stat(path)
		if err != nil {
			tw.Close()
			gw.Close()
			return fmt.Errorf("stat %s: %w", path, err)
		}

		relPath, err := filepath.Rel(srcDir, path)
		if err != nil {
			tw.Close()
			gw.Close()
			return fmt.Errorf("rel path %s: %w", path, err)
		}

		if info.IsDir() {
			hdr := &tar.Header{
				Name:     relPath + "/",
				Mode:     int64(info.Mode()),
				Typeflag: tar.TypeDir,
			}
			if err := tw.WriteHeader(hdr); err != nil {
				tw.Close()
				gw.Close()
				return fmt.Errorf("write dir header %s: %w", relPath, err)
			}
			continue
		}

		hdr := &tar.Header{
			Name: relPath,
			Mode: int64(info.Mode()),
			Size: info.Size(),
		}
		if err := tw.WriteHeader(hdr); err != nil {
			tw.Close()
			gw.Close()
			return fmt.Errorf("write header %s: %w", relPath, err)
		}

		data, err := os.Open(path)
		if err != nil {
			tw.Close()
			gw.Close()
			return fmt.Errorf("open %s: %w", path, err)
		}
		if _, err := io.Copy(tw, data); err != nil {
			data.Close()
			tw.Close()
			gw.Close()
			return fmt.Errorf("copy %s: %w", relPath, err)
		}
		data.Close()
	}

	if err := tw.Close(); err != nil {
		gw.Close()
		return fmt.Errorf("close tar writer: %w", err)
	}
	if err := gw.Close(); err != nil {
		return fmt.Errorf("close gzip writer: %w", err)
	}

	return nil
}

// tarballExcluded lists files that should NOT be included in exported tarballs.
// The sidecar is server-internal state — it should never leak into exports.
var tarballExcluded = map[string]bool{
	"original-inspection-snapshot.json": true,
}

// RepackTarballFiltered creates a .tar.gz at destPath from the contents of
// srcDir, excluding files in the tarballExcluded set.
func RepackTarballFiltered(srcDir, destPath string) error {
	f, err := os.Create(destPath)
	if err != nil {
		return fmt.Errorf("create tarball: %w", err)
	}
	defer f.Close()

	gw := gzip.NewWriter(f)
	tw := tar.NewWriter(gw)

	// Collect all paths first for sorted iteration
	var paths []string
	err = filepath.Walk(srcDir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		if path == srcDir {
			return nil
		}
		// Exclude filtered files at the top level
		relPath, _ := filepath.Rel(srcDir, path)
		if tarballExcluded[relPath] {
			return nil
		}
		paths = append(paths, path)
		return nil
	})
	if err != nil {
		tw.Close()
		gw.Close()
		return fmt.Errorf("walk source dir: %w", err)
	}

	sort.Strings(paths)

	for _, path := range paths {
		info, err := os.Stat(path)
		if err != nil {
			tw.Close()
			gw.Close()
			return fmt.Errorf("stat %s: %w", path, err)
		}

		relPath, err := filepath.Rel(srcDir, path)
		if err != nil {
			tw.Close()
			gw.Close()
			return fmt.Errorf("rel path %s: %w", path, err)
		}

		if info.IsDir() {
			hdr := &tar.Header{
				Name:     relPath + "/",
				Mode:     int64(info.Mode()),
				Typeflag: tar.TypeDir,
			}
			if err := tw.WriteHeader(hdr); err != nil {
				tw.Close()
				gw.Close()
				return fmt.Errorf("write dir header %s: %w", relPath, err)
			}
			continue
		}

		hdr := &tar.Header{
			Name: relPath,
			Mode: int64(info.Mode()),
			Size: info.Size(),
		}
		if err := tw.WriteHeader(hdr); err != nil {
			tw.Close()
			gw.Close()
			return fmt.Errorf("write header %s: %w", relPath, err)
		}

		data, err := os.Open(path)
		if err != nil {
			tw.Close()
			gw.Close()
			return fmt.Errorf("open %s: %w", path, err)
		}
		if _, err := io.Copy(tw, data); err != nil {
			data.Close()
			tw.Close()
			gw.Close()
			return fmt.Errorf("copy %s: %w", relPath, err)
		}
		data.Close()
	}

	if err := tw.Close(); err != nil {
		gw.Close()
		return fmt.Errorf("close tar writer: %w", err)
	}
	if err := gw.Close(); err != nil {
		return fmt.Errorf("close gzip writer: %w", err)
	}

	return nil
}

// sanitizeTarPath removes ".." components and leading slashes from a tar entry name.
func sanitizeTarPath(name string) string {
	// Normalize path separators
	name = strings.ReplaceAll(name, "\\", "/")

	var parts []string
	for _, part := range strings.Split(name, "/") {
		if part == "" || part == "." || part == ".." {
			continue
		}
		parts = append(parts, part)
	}
	return strings.Join(parts, "/")
}
