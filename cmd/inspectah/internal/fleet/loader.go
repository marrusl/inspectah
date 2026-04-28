// Package fleet loads and merges multiple inspection snapshots into a
// fleet-level aggregate report with prevalence metadata.
package fleet

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

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// LoadSnapshots discovers and loads inspection snapshots from inputDir.
// Supports .tar.gz files (extracts inspection-snapshot.json) and bare
// .json files. The file fleet-snapshot.json is always skipped to prevent
// self-contamination from a previous fleet merge in the same directory.
// Invalid files are skipped with a log warning.
func LoadSnapshots(inputDir string) ([]*schema.InspectionSnapshot, error) {
	entries, err := os.ReadDir(inputDir)
	if err != nil {
		return nil, fmt.Errorf("read input directory: %w", err)
	}

	// Sort for deterministic ordering
	sort.Slice(entries, func(i, j int) bool {
		return entries[i].Name() < entries[j].Name()
	})

	var snapshots []*schema.InspectionSnapshot
	for _, entry := range entries {
		if entry.IsDir() {
			continue
		}
		name := entry.Name()
		path := filepath.Join(inputDir, name)

		// Skip fleet-snapshot.json to prevent self-contamination
		if name == "fleet-snapshot.json" {
			continue
		}

		var snap *schema.InspectionSnapshot
		if strings.HasSuffix(name, ".tar.gz") {
			snap = loadFromTarball(path)
		} else if strings.HasSuffix(name, ".json") {
			snap = loadFromJSON(path)
		} else {
			continue
		}

		if snap != nil {
			snapshots = append(snapshots, snap)
		}
	}
	return snapshots, nil
}

// ValidateSnapshots checks that all snapshots are compatible for merging:
// minimum count, matching schema versions, matching os_release, and
// matching base images.
func ValidateSnapshots(snapshots []*schema.InspectionSnapshot) error {
	if len(snapshots) < 2 {
		return fmt.Errorf("need at least 2 snapshots, found %d", len(snapshots))
	}

	// Schema version
	versions := make(map[int]bool)
	for _, s := range snapshots {
		versions[s.SchemaVersion] = true
	}
	if len(versions) > 1 {
		vs := make([]int, 0, len(versions))
		for v := range versions {
			vs = append(vs, v)
		}
		return fmt.Errorf("schema version mismatch: %v", vs)
	}

	// Duplicate hostnames — warn only
	seen := make(map[string]bool)
	for _, s := range snapshots {
		h, _ := s.Meta["hostname"].(string)
		if h != "" && seen[h] {
			log.Printf("warning: duplicate hostname: %s", h)
		}
		seen[h] = true
	}

	// os_release — all must be present and matching
	for _, s := range snapshots {
		if s.OsRelease == nil {
			hostname, _ := s.Meta["hostname"].(string)
			if hostname == "" {
				hostname = "unknown"
			}
			return fmt.Errorf("snapshot from %s has no os_release", hostname)
		}
	}

	osIDs := make(map[string]bool)
	for _, s := range snapshots {
		osIDs[s.OsRelease.ID] = true
	}
	if len(osIDs) > 1 {
		ids := make([]string, 0, len(osIDs))
		for id := range osIDs {
			ids = append(ids, id)
		}
		sort.Strings(ids)
		return fmt.Errorf("os_release.id mismatch: %v", ids)
	}

	osVersions := make(map[string]bool)
	for _, s := range snapshots {
		osVersions[s.OsRelease.VersionID] = true
	}
	if len(osVersions) > 1 {
		vs := make([]string, 0, len(osVersions))
		for v := range osVersions {
			vs = append(vs, v)
		}
		sort.Strings(vs)
		return fmt.Errorf("os_release.version_id mismatch: %v", vs)
	}

	// base_image — if present on any snapshot with rpm, must match
	baseImages := make(map[string]bool)
	for _, s := range snapshots {
		if s.Rpm != nil && s.Rpm.BaseImage != nil && *s.Rpm.BaseImage != "" {
			baseImages[*s.Rpm.BaseImage] = true
		}
	}
	if len(baseImages) > 1 {
		bis := make([]string, 0, len(baseImages))
		for bi := range baseImages {
			bis = append(bis, bi)
		}
		sort.Strings(bis)
		return fmt.Errorf("rpm.base_image mismatch: %v", bis)
	}

	return nil
}

// ComputeDisplayNames computes shortest unique display names from
// hostnames. Progressive disambiguation adds domain segments until
// collisions are resolved. Identical hostnames get numeric suffixes.
func ComputeDisplayNames(hostnames []string) []string {
	if len(hostnames) == 0 {
		return []string{}
	}

	segments := make([][]string, len(hostnames))
	depths := make([]int, len(hostnames))
	for i, h := range hostnames {
		if h == "" {
			segments[i] = []string{""}
		} else {
			segments[i] = strings.Split(h, ".")
		}
		depths[i] = 1
	}

	labelsForDepths := func() []string {
		labels := make([]string, len(hostnames))
		for i, parts := range segments {
			end := depths[i]
			if end > len(parts) {
				end = len(parts)
			}
			labels[i] = strings.Join(parts[:end], ".")
		}
		return labels
	}

	for {
		labels := labelsForDepths()
		groups := make(map[string][]int)
		for idx, label := range labels {
			groups[label] = append(groups[label], idx)
		}

		hasCollisions := false
		for _, indices := range groups {
			if len(indices) > 1 {
				hasCollisions = true
				break
			}
		}
		if !hasCollisions {
			return labels
		}

		changed := false
		for _, indices := range groups {
			if len(indices) < 2 {
				continue
			}
			for _, idx := range indices {
				if depths[idx] < len(segments[idx]) {
					depths[idx]++
					changed = true
				}
			}
		}

		if !changed {
			break
		}
	}

	// Handle remaining collisions with numeric suffixes
	labels := labelsForDepths()
	duplicateGroups := make(map[string][]int)
	for idx, label := range labels {
		duplicateGroups[label] = append(duplicateGroups[label], idx)
	}

	for _, indices := range duplicateGroups {
		if len(indices) < 2 {
			continue
		}
		for ordinal, idx := range indices {
			labels[idx] = fmt.Sprintf("%s (%d)", segments[idx][0], ordinal+1)
		}
	}

	return labels
}

// AssignDisplayNames computes and stores per-snapshot display names in
// snapshot metadata. Returns the display name list.
func AssignDisplayNames(snapshots []*schema.InspectionSnapshot) []string {
	hostnames := make([]string, len(snapshots))
	for i, s := range snapshots {
		h, ok := s.Meta["hostname"].(string)
		if !ok {
			h = fmt.Sprintf("host-%d", i)
		}
		hostnames[i] = h
	}

	displayNames := ComputeDisplayNames(hostnames)
	for i, name := range displayNames {
		if snapshots[i].Meta == nil {
			snapshots[i].Meta = make(map[string]interface{})
		}
		snapshots[i].Meta["display_name"] = name
	}
	return displayNames
}

// loadFromJSON reads a snapshot from a bare JSON file.
func loadFromJSON(path string) *schema.InspectionSnapshot {
	data, err := os.ReadFile(path)
	if err != nil {
		log.Printf("warning: skipping invalid JSON %s: %v", filepath.Base(path), err)
		return nil
	}

	var snap schema.InspectionSnapshot
	if err := json.Unmarshal(data, &snap); err != nil {
		log.Printf("warning: skipping invalid JSON %s: %v", filepath.Base(path), err)
		return nil
	}
	return &snap
}

// loadFromTarball extracts inspection-snapshot.json from a .tar.gz file.
func loadFromTarball(path string) *schema.InspectionSnapshot {
	f, err := os.Open(path)
	if err != nil {
		log.Printf("warning: skipping tarball %s: %v", filepath.Base(path), err)
		return nil
	}
	defer f.Close()

	gr, err := gzip.NewReader(f)
	if err != nil {
		log.Printf("warning: skipping tarball %s: %v", filepath.Base(path), err)
		return nil
	}
	defer gr.Close()

	tr := tar.NewReader(gr)
	for {
		hdr, err := tr.Next()
		if err == io.EOF {
			break
		}
		if err != nil {
			log.Printf("warning: skipping tarball %s: %v", filepath.Base(path), err)
			return nil
		}

		if strings.HasSuffix(hdr.Name, "inspection-snapshot.json") {
			data, err := io.ReadAll(tr)
			if err != nil {
				log.Printf("warning: skipping tarball %s: cannot read entry: %v", filepath.Base(path), err)
				return nil
			}

			var snap schema.InspectionSnapshot
			if err := json.Unmarshal(data, &snap); err != nil {
				log.Printf("warning: skipping tarball %s: invalid JSON: %v", filepath.Base(path), err)
				return nil
			}
			return &snap
		}
	}

	log.Printf("warning: skipping tarball %s: no inspection-snapshot.json found", filepath.Base(path))
	return nil
}
