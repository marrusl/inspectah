package pipeline

import (
	"io"
	"os"
	"path/filepath"
	"strings"
)

// BundleSubscriptionCerts copies RHEL subscription certificates and RHSM
// config from the host filesystem into the output directory.
func BundleSubscriptionCerts(hostRoot, outputDir string) error {
	if hostRoot == "" {
		return nil
	}

	info, err := os.Stat(hostRoot)
	if err != nil || !info.IsDir() {
		return nil
	}

	// Copy .pem files from /etc/pki/entitlement/
	entSrc := filepath.Join(hostRoot, "etc", "pki", "entitlement")
	if info, err := os.Stat(entSrc); err == nil && info.IsDir() {
		entries, err := os.ReadDir(entSrc)
		if err == nil {
			entDst := filepath.Join(outputDir, "entitlement")
			for _, entry := range entries {
				if entry.IsDir() || !strings.HasSuffix(entry.Name(), ".pem") {
					continue
				}
				os.MkdirAll(entDst, 0755)
				copyFile(filepath.Join(entSrc, entry.Name()), filepath.Join(entDst, entry.Name()))
			}
		}
	}

	// Copy /etc/rhsm/ tree
	rhsmSrc := filepath.Join(hostRoot, "etc", "rhsm")
	if info, err := os.Stat(rhsmSrc); err == nil && info.IsDir() {
		rhsmDst := filepath.Join(outputDir, "rhsm")
		copyDir(rhsmSrc, rhsmDst)
	}

	return nil
}

func copyFile(src, dst string) error {
	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer in.Close()

	if err := os.MkdirAll(filepath.Dir(dst), 0755); err != nil {
		return err
	}

	out, err := os.Create(dst)
	if err != nil {
		return err
	}
	defer out.Close()

	_, err = io.Copy(out, in)
	return err
}

func copyDir(src, dst string) error {
	return filepath.Walk(src, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return nil // skip errors
		}
		rel, _ := filepath.Rel(src, path)
		target := filepath.Join(dst, rel)
		if info.IsDir() {
			return os.MkdirAll(target, info.Mode())
		}
		return copyFile(path, target)
	})
}
