package architect

import (
	"archive/tar"
	"compress/gzip"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

// snapshotJSON is the minimal snapshot structure needed by the architect
// loader. We only parse the fields we actually need rather than importing
// the full schema package.
type snapshotJSON struct {
	Meta      map[string]interface{} `json:"meta"`
	OsRelease *struct {
		VersionID string `json:"version_id"`
	} `json:"os_release"`
	Rpm *struct {
		PackagesAdded []struct {
			Name string `json:"name"`
			NVRA string `json:"nvra"`
		} `json:"packages_added"`
		BaseImage *string `json:"base_image"`
	} `json:"rpm"`
	Config *struct {
		Files []struct {
			Path string `json:"path"`
		} `json:"files"`
	} `json:"config"`
	Preflight *struct {
		Unavailable   []string `json:"unavailable"`
		DirectInstall []string `json:"direct_install"`
		Unverifiable  []string `json:"unverifiable"`
		Status        string   `json:"status"`
	} `json:"preflight"`
}

// LoadRefinedFleets loads fleet tarballs from a directory and returns
// FleetInput objects ready for the analyzer.
//
// Each tarball should contain an inspection-snapshot.json with fleet
// metadata. Non-tarball files are skipped.
func LoadRefinedFleets(inputDir string) ([]FleetInput, error) {
	entries, err := os.ReadDir(inputDir)
	if err != nil {
		return nil, fmt.Errorf("read input directory: %w", err)
	}

	sort.Slice(entries, func(i, j int) bool {
		return entries[i].Name() < entries[j].Name()
	})

	var fleets []FleetInput
	for _, entry := range entries {
		if entry.IsDir() {
			continue
		}
		name := entry.Name()
		if !strings.HasSuffix(name, ".tar.gz") {
			continue
		}

		path := filepath.Join(inputDir, name)
		snap, err := extractSnapshot(path)
		if err != nil {
			log.Printf("warning: skipping %s: %v", name, err)
			continue
		}
		if snap == nil {
			log.Printf("warning: no inspection-snapshot.json in %s", name)
			continue
		}

		fi := snapshotToFleetInput(snap)
		fleets = append(fleets, fi)
	}

	return fleets, nil
}

// ValidateFleetVersions enforces that all fleets share the same OS major
// version. Returns an error if versions are mixed.
func ValidateFleetVersions(fleets []FleetInput, inputDir string) error {
	if len(fleets) < 2 {
		return nil
	}

	// Reload snapshots to check version_id
	entries, err := os.ReadDir(inputDir)
	if err != nil {
		return nil
	}

	var versions []string
	for _, entry := range entries {
		if entry.IsDir() || !strings.HasSuffix(entry.Name(), ".tar.gz") {
			continue
		}
		path := filepath.Join(inputDir, entry.Name())
		snap, err := extractSnapshot(path)
		if err != nil || snap == nil {
			continue
		}
		if snap.OsRelease != nil && snap.OsRelease.VersionID != "" {
			versions = append(versions, snap.OsRelease.VersionID)
		}
	}

	if len(versions) < 2 {
		return nil
	}

	// Extract major versions (e.g., "9" from "9.4")
	majors := make(map[string]bool)
	for _, v := range versions {
		major := v
		if idx := strings.Index(v, "."); idx > 0 {
			major = v[:idx]
		}
		majors[major] = true
	}

	if len(majors) > 1 {
		ms := make([]string, 0, len(majors))
		for m := range majors {
			ms = append(ms, m)
		}
		sort.Strings(ms)
		return fmt.Errorf("mixed OS major versions: %v (architect requires all fleets to share the same major version)", ms)
	}

	return nil
}

func extractSnapshot(tarballPath string) (*snapshotJSON, error) {
	f, err := os.Open(tarballPath)
	if err != nil {
		return nil, err
	}
	defer f.Close()

	gz, err := gzip.NewReader(f)
	if err != nil {
		return nil, fmt.Errorf("gzip: %w", err)
	}
	defer gz.Close()

	tr := tar.NewReader(gz)
	for {
		hdr, err := tr.Next()
		if err == io.EOF {
			break
		}
		if err != nil {
			return nil, fmt.Errorf("tar: %w", err)
		}
		if strings.HasSuffix(hdr.Name, "inspection-snapshot.json") {
			data, err := io.ReadAll(tr)
			if err != nil {
				return nil, fmt.Errorf("read snapshot: %w", err)
			}
			var snap snapshotJSON
			if err := json.Unmarshal(data, &snap); err != nil {
				return nil, fmt.Errorf("parse snapshot: %w", err)
			}
			return &snap, nil
		}
	}
	return nil, nil
}

func snapshotToFleetInput(snap *snapshotJSON) FleetInput {
	hostname := "unknown"
	if h, ok := snap.Meta["hostname"].(string); ok {
		hostname = h
	}

	hostCount := 1
	if fleet, ok := snap.Meta["fleet"].(map[string]interface{}); ok {
		if th, ok := fleet["total_hosts"].(float64); ok {
			hostCount = int(th)
		}
	}

	var packages []string
	if snap.Rpm != nil {
		for _, pkg := range snap.Rpm.PackagesAdded {
			nvra := pkg.NVRA
			if nvra == "" {
				nvra = pkg.Name
			}
			if nvra != "" {
				packages = append(packages, nvra)
			}
		}
	}

	var configs []string
	if snap.Config != nil {
		for _, f := range snap.Config.Files {
			if f.Path != "" {
				configs = append(configs, f.Path)
			}
		}
	}

	baseImage := ""
	if snap.Rpm != nil && snap.Rpm.BaseImage != nil {
		baseImage = *snap.Rpm.BaseImage
	}

	var unavailable, directInstall, unverifiable []string
	preflightStatus := "skipped"
	if snap.Preflight != nil {
		unavailable = snap.Preflight.Unavailable
		directInstall = snap.Preflight.DirectInstall
		unverifiable = snap.Preflight.Unverifiable
		if snap.Preflight.Status != "" {
			preflightStatus = snap.Preflight.Status
		}
	}

	return FleetInput{
		Name:                  hostname,
		Packages:              packages,
		Configs:               configs,
		HostCount:             hostCount,
		BaseImage:             baseImage,
		UnavailablePackages:   unavailable,
		DirectInstallPackages: directInstall,
		UnverifiablePackages:  unverifiable,
		PreflightStatus:       preflightStatus,
	}
}
