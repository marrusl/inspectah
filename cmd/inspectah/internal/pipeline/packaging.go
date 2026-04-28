package pipeline

import (
	"archive/tar"
	"compress/gzip"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"time"
)

var unsafeFilenameRe = regexp.MustCompile(`[^\w.-]`)

// SanitizeHostname removes characters unsafe for filenames.
func SanitizeHostname(hostname string) string {
	cleaned := unsafeFilenameRe.ReplaceAllString(hostname, "")
	if cleaned == "" {
		return "unknown"
	}
	return cleaned
}

// GetOutputStamp returns "HOSTNAME-YYYYMMDD-HHMMSS" for tarball naming.
func GetOutputStamp(hostname string) string {
	resolved := SanitizeHostname(hostname)
	now := time.Now().Format("20060102-150405")
	return fmt.Sprintf("%s-%s", resolved, now)
}

// CreateTarball creates a gzipped tarball from sourceDir with entries
// under prefix/.
func CreateTarball(sourceDir, tarballPath, prefix string) error {
	f, err := os.Create(tarballPath)
	if err != nil {
		return fmt.Errorf("create tarball: %w", err)
	}
	defer f.Close()

	gw := gzip.NewWriter(f)
	defer gw.Close()

	tw := tar.NewWriter(gw)
	defer tw.Close()

	// Collect and sort paths for deterministic output
	var paths []string
	filepath.Walk(sourceDir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return nil
		}
		paths = append(paths, path)
		return nil
	})
	sort.Strings(paths)

	for _, path := range paths {
		info, err := os.Stat(path)
		if err != nil {
			continue
		}

		rel, err := filepath.Rel(sourceDir, path)
		if err != nil {
			continue
		}
		arcname := filepath.Join(prefix, rel)

		header, err := tar.FileInfoHeader(info, "")
		if err != nil {
			continue
		}
		header.Name = arcname

		if err := tw.WriteHeader(header); err != nil {
			return fmt.Errorf("write header %s: %w", arcname, err)
		}

		if !info.IsDir() {
			f, err := os.Open(path)
			if err != nil {
				continue
			}
			io.Copy(tw, f)
			f.Close()
		}
	}

	return nil
}
