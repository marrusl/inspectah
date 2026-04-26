package build

import (
	"crypto/x509"
	"encoding/pem"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"time"
)

type DiscoveryStatus int

const (
	DiscoveryNoCerts DiscoveryStatus = iota
	DiscoveryCertsFound
	DiscoveryHostNative
)

type DiscoverResult struct {
	Status         DiscoveryStatus
	EntitlementDir string
	RHSMDir        string
}

type DiscoverOpts struct {
	EntitlementsDir  string
	EnvDir           string
	OutputDir        string
	SkipEntitlements bool
}

func DiscoverCerts(opts DiscoverOpts) (*DiscoverResult, error) {
	if opts.SkipEntitlements && opts.EntitlementsDir != "" {
		return nil, fmt.Errorf("--no-entitlements and --entitlements-dir are mutually exclusive")
	}

	if opts.SkipEntitlements {
		return &DiscoverResult{Status: DiscoveryNoCerts}, nil
	}

	if opts.EntitlementsDir != "" {
		return resolveExplicitDir(opts.EntitlementsDir)
	}

	if opts.EnvDir != "" {
		return resolveExplicitDir(opts.EnvDir)
	}

	if runtime.GOOS == "linux" {
		hostEnt := "/etc/pki/entitlement"
		if hasPEMs(hostEnt) {
			return &DiscoverResult{Status: DiscoveryHostNative}, nil
		}
	}

	if opts.OutputDir != "" {
		bundled := filepath.Join(opts.OutputDir, "entitlement")
		if hasPEMs(bundled) {
			return resolveDir(bundled)
		}
	}

	homeDir, _ := os.UserHomeDir()
	if homeDir != "" {
		userConf := filepath.Join(homeDir, ".config", "inspectah", "entitlement")
		if hasPEMs(userConf) {
			return resolveDir(userConf)
		}
	}

	return &DiscoverResult{Status: DiscoveryNoCerts}, nil
}

func ValidateExplicitDir(dir string) error {
	info, err := os.Stat(dir)
	if err != nil {
		return fmt.Errorf("entitlement directory %q does not exist", dir)
	}
	if !info.IsDir() {
		return fmt.Errorf("entitlement path %q is not a directory", dir)
	}
	if !hasPEMs(dir) {
		return fmt.Errorf("entitlement directory %q contains no .pem files", dir)
	}
	return nil
}

func resolveExplicitDir(dir string) (*DiscoverResult, error) {
	if err := ValidateExplicitDir(dir); err != nil {
		return nil, err
	}
	return resolveDir(dir)
}

func resolveDir(dir string) (*DiscoverResult, error) {
	abs, err := filepath.Abs(dir)
	if err != nil {
		return nil, err
	}
	result := &DiscoverResult{
		Status:         DiscoveryCertsFound,
		EntitlementDir: abs,
	}
	rhsmDir := filepath.Join(filepath.Dir(abs), "rhsm")
	if info, err := os.Stat(rhsmDir); err == nil && info.IsDir() {
		result.RHSMDir = rhsmDir
	}
	return result, nil
}

func hasPEMs(dir string) bool {
	matches, _ := filepath.Glob(filepath.Join(dir, "*.pem"))
	return len(matches) > 0
}

func ValidateCertExpiry(dir string, ignoreExpired bool) error {
	pems, _ := filepath.Glob(filepath.Join(dir, "*.pem"))
	now := time.Now()

	var expired []string
	var earliestExpiry time.Time

	for _, p := range pems {
		data, err := os.ReadFile(p)
		if err != nil {
			continue
		}
		block, _ := pem.Decode(data)
		if block == nil {
			continue
		}
		cert, err := x509.ParseCertificate(block.Bytes)
		if err != nil {
			continue
		}
		if cert.NotAfter.Before(now) {
			expired = append(expired, p)
			if earliestExpiry.IsZero() || cert.NotAfter.Before(earliestExpiry) {
				earliestExpiry = cert.NotAfter
			}
		}
	}

	if len(expired) > 0 && !ignoreExpired {
		return fmt.Errorf("RHEL entitlement cert expired (%s)\n  Certs: %s\n  Fix:   sudo subscription-manager refresh\n  Skip:  inspectah build --ignore-expired-certs ...",
			earliestExpiry.Format("2006-01-02"),
			strings.Join(expired, ", "))
	}
	return nil
}

func CheckMacOSPath(dir string) string {
	if runtime.GOOS != "darwin" {
		return ""
	}
	homeDir, _ := os.UserHomeDir()
	if homeDir == "" {
		return ""
	}
	abs, _ := filepath.Abs(dir)
	if !strings.HasPrefix(abs, homeDir) {
		return fmt.Sprintf("Warning: entitlement path %q is outside $HOME — it may not be accessible to podman machine. Consider copying certs under %s.", abs, homeDir)
	}
	return ""
}
