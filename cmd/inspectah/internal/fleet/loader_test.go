package fleet

import (
	"archive/tar"
	"compress/gzip"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// makeSnapshot creates a minimal valid InspectionSnapshot for testing.
func makeSnapshot(hostname string) *schema.InspectionSnapshot {
	snap := schema.NewSnapshot()
	snap.Meta["hostname"] = hostname
	snap.OsRelease = &schema.OsRelease{
		Name:      "Red Hat Enterprise Linux",
		VersionID: "9.4",
		Version:   "9.4 (Plow)",
		ID:        "rhel",
		IDLike:    "fedora",
		PrettyName: "Red Hat Enterprise Linux 9.4 (Plow)",
	}
	return snap
}

// writeSnapshotJSON writes a snapshot as a JSON file.
func writeSnapshotJSON(t *testing.T, dir, filename string, snap *schema.InspectionSnapshot) {
	t.Helper()
	data, err := json.MarshalIndent(snap, "", "  ")
	if err != nil {
		t.Fatalf("marshal snapshot: %v", err)
	}
	if err := os.WriteFile(filepath.Join(dir, filename), data, 0644); err != nil {
		t.Fatalf("write snapshot: %v", err)
	}
}

// writeSnapshotTarball writes a snapshot as a .tar.gz containing inspection-snapshot.json.
func writeSnapshotTarball(t *testing.T, dir, filename string, snap *schema.InspectionSnapshot) {
	t.Helper()
	data, err := json.MarshalIndent(snap, "", "  ")
	if err != nil {
		t.Fatalf("marshal snapshot: %v", err)
	}

	path := filepath.Join(dir, filename)
	f, err := os.Create(path)
	if err != nil {
		t.Fatalf("create tarball: %v", err)
	}
	defer f.Close()

	gw := gzip.NewWriter(f)
	tw := tar.NewWriter(gw)

	hdr := &tar.Header{
		Name: "inspectah-host1/inspection-snapshot.json",
		Mode: 0644,
		Size: int64(len(data)),
	}
	if err := tw.WriteHeader(hdr); err != nil {
		t.Fatalf("write tar header: %v", err)
	}
	if _, err := tw.Write(data); err != nil {
		t.Fatalf("write tar data: %v", err)
	}

	tw.Close()
	gw.Close()
}

func TestLoadSnapshotsJSON(t *testing.T) {
	dir := t.TempDir()
	snap1 := makeSnapshot("host1.example.com")
	snap2 := makeSnapshot("host2.example.com")
	writeSnapshotJSON(t, dir, "host1.json", snap1)
	writeSnapshotJSON(t, dir, "host2.json", snap2)

	snapshots, err := LoadSnapshots(dir)
	if err != nil {
		t.Fatalf("LoadSnapshots: %v", err)
	}
	if len(snapshots) != 2 {
		t.Fatalf("expected 2 snapshots, got %d", len(snapshots))
	}
}

func TestLoadSnapshotsTarball(t *testing.T) {
	dir := t.TempDir()
	snap1 := makeSnapshot("host1.example.com")
	snap2 := makeSnapshot("host2.example.com")
	writeSnapshotTarball(t, dir, "host1.tar.gz", snap1)
	writeSnapshotTarball(t, dir, "host2.tar.gz", snap2)

	snapshots, err := LoadSnapshots(dir)
	if err != nil {
		t.Fatalf("LoadSnapshots: %v", err)
	}
	if len(snapshots) != 2 {
		t.Fatalf("expected 2 snapshots, got %d", len(snapshots))
	}
}

func TestLoadSnapshotsMixed(t *testing.T) {
	dir := t.TempDir()
	snap1 := makeSnapshot("host1.example.com")
	snap2 := makeSnapshot("host2.example.com")
	writeSnapshotJSON(t, dir, "host1.json", snap1)
	writeSnapshotTarball(t, dir, "host2.tar.gz", snap2)

	snapshots, err := LoadSnapshots(dir)
	if err != nil {
		t.Fatalf("LoadSnapshots: %v", err)
	}
	if len(snapshots) != 2 {
		t.Fatalf("expected 2 snapshots, got %d", len(snapshots))
	}
}

func TestLoadSnapshotsSkipsFleetSnapshot(t *testing.T) {
	dir := t.TempDir()
	snap1 := makeSnapshot("host1.example.com")
	snap2 := makeSnapshot("host2.example.com")
	fleetSnap := makeSnapshot("fleet-merged")
	writeSnapshotJSON(t, dir, "host1.json", snap1)
	writeSnapshotJSON(t, dir, "host2.json", snap2)
	writeSnapshotJSON(t, dir, "fleet-snapshot.json", fleetSnap)

	snapshots, err := LoadSnapshots(dir)
	if err != nil {
		t.Fatalf("LoadSnapshots: %v", err)
	}
	if len(snapshots) != 2 {
		t.Fatalf("expected 2 snapshots (fleet-snapshot.json skipped), got %d", len(snapshots))
	}
}

func TestLoadSnapshotsSkipsNonSnapshotFiles(t *testing.T) {
	dir := t.TempDir()
	snap1 := makeSnapshot("host1.example.com")
	snap2 := makeSnapshot("host2.example.com")
	writeSnapshotJSON(t, dir, "host1.json", snap1)
	writeSnapshotJSON(t, dir, "host2.json", snap2)
	// Write a non-snapshot file
	os.WriteFile(filepath.Join(dir, "readme.txt"), []byte("not a snapshot"), 0644)
	os.WriteFile(filepath.Join(dir, "notes.md"), []byte("# notes"), 0644)

	snapshots, err := LoadSnapshots(dir)
	if err != nil {
		t.Fatalf("LoadSnapshots: %v", err)
	}
	if len(snapshots) != 2 {
		t.Fatalf("expected 2 snapshots, got %d", len(snapshots))
	}
}

func TestLoadSnapshotsInvalidJSON(t *testing.T) {
	dir := t.TempDir()
	snap1 := makeSnapshot("host1.example.com")
	writeSnapshotJSON(t, dir, "host1.json", snap1)
	// Write invalid JSON
	os.WriteFile(filepath.Join(dir, "bad.json"), []byte("{invalid"), 0644)
	writeSnapshotJSON(t, dir, "host2.json", makeSnapshot("host2.example.com"))

	// Should skip invalid files with a warning, not error
	snapshots, err := LoadSnapshots(dir)
	if err != nil {
		t.Fatalf("LoadSnapshots: %v", err)
	}
	if len(snapshots) != 2 {
		t.Fatalf("expected 2 valid snapshots, got %d", len(snapshots))
	}
}

func TestLoadSnapshotsEmptyDir(t *testing.T) {
	dir := t.TempDir()
	snapshots, err := LoadSnapshots(dir)
	if err != nil {
		t.Fatalf("LoadSnapshots: %v", err)
	}
	if len(snapshots) != 0 {
		t.Fatalf("expected 0 snapshots from empty dir, got %d", len(snapshots))
	}
}

func TestValidateSnapshotsMinimum(t *testing.T) {
	snap := makeSnapshot("host1.example.com")
	err := ValidateSnapshots([]*schema.InspectionSnapshot{snap})
	if err == nil {
		t.Fatal("expected error for < 2 snapshots")
	}
}

func TestValidateSnapshotsSchemaVersionMismatch(t *testing.T) {
	snap1 := makeSnapshot("host1.example.com")
	snap2 := makeSnapshot("host2.example.com")
	snap2.SchemaVersion = 999

	err := ValidateSnapshots([]*schema.InspectionSnapshot{snap1, snap2})
	if err == nil {
		t.Fatal("expected error for schema version mismatch")
	}
}

func TestValidateSnapshotsOsReleaseMissing(t *testing.T) {
	snap1 := makeSnapshot("host1.example.com")
	snap2 := makeSnapshot("host2.example.com")
	snap2.OsRelease = nil

	err := ValidateSnapshots([]*schema.InspectionSnapshot{snap1, snap2})
	if err == nil {
		t.Fatal("expected error for missing os_release")
	}
}

func TestValidateSnapshotsOsReleaseIDMismatch(t *testing.T) {
	snap1 := makeSnapshot("host1.example.com")
	snap2 := makeSnapshot("host2.example.com")
	snap2.OsRelease.ID = "centos"

	err := ValidateSnapshots([]*schema.InspectionSnapshot{snap1, snap2})
	if err == nil {
		t.Fatal("expected error for os_release.id mismatch")
	}
}

func TestValidateSnapshotsOsReleaseVersionMismatch(t *testing.T) {
	snap1 := makeSnapshot("host1.example.com")
	snap2 := makeSnapshot("host2.example.com")
	snap2.OsRelease.VersionID = "8.9"

	err := ValidateSnapshots([]*schema.InspectionSnapshot{snap1, snap2})
	if err == nil {
		t.Fatal("expected error for os_release.version_id mismatch")
	}
}

func TestValidateSnapshotsBaseImageMismatch(t *testing.T) {
	snap1 := makeSnapshot("host1.example.com")
	snap2 := makeSnapshot("host2.example.com")
	bi1 := "quay.io/centos-bootc/centos-bootc:stream9"
	bi2 := "registry.redhat.io/rhel9/rhel-bootc:9.4"
	snap1.Rpm = &schema.RpmSection{BaseImage: &bi1}
	snap2.Rpm = &schema.RpmSection{BaseImage: &bi2}

	err := ValidateSnapshots([]*schema.InspectionSnapshot{snap1, snap2})
	if err == nil {
		t.Fatal("expected error for base image mismatch")
	}
}

func TestValidateSnapshotsValid(t *testing.T) {
	snap1 := makeSnapshot("host1.example.com")
	snap2 := makeSnapshot("host2.example.com")

	err := ValidateSnapshots([]*schema.InspectionSnapshot{snap1, snap2})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestComputeDisplayNames(t *testing.T) {
	tests := []struct {
		name      string
		hostnames []string
		want      []string
	}{
		{
			name:      "simple unique short names",
			hostnames: []string{"web1.example.com", "web2.example.com"},
			want:      []string{"web1", "web2"},
		},
		{
			name:      "collision needs domain segment",
			hostnames: []string{"web.prod.example.com", "web.staging.example.com"},
			want:      []string{"web.prod", "web.staging"},
		},
		{
			name:      "identical hostnames get numeric suffix",
			hostnames: []string{"web.example.com", "web.example.com"},
			want:      []string{"web (1)", "web (2)"},
		},
		{
			name:      "empty list",
			hostnames: []string{},
			want:      []string{},
		},
		{
			name:      "single host",
			hostnames: []string{"host1.example.com"},
			want:      []string{"host1"},
		},
		{
			name:      "no domain",
			hostnames: []string{"alpha", "beta"},
			want:      []string{"alpha", "beta"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := ComputeDisplayNames(tt.hostnames)
			if len(got) != len(tt.want) {
				t.Fatalf("ComputeDisplayNames(%v) = %v, want %v", tt.hostnames, got, tt.want)
			}
			for i, g := range got {
				if g != tt.want[i] {
					t.Errorf("ComputeDisplayNames(%v)[%d] = %q, want %q", tt.hostnames, i, g, tt.want[i])
				}
			}
		})
	}
}

func TestAssignDisplayNames(t *testing.T) {
	snap1 := makeSnapshot("host1.example.com")
	snap2 := makeSnapshot("host2.example.com")
	snapshots := []*schema.InspectionSnapshot{snap1, snap2}

	names := AssignDisplayNames(snapshots)
	if len(names) != 2 {
		t.Fatalf("expected 2 display names, got %d", len(names))
	}
	if names[0] != "host1" || names[1] != "host2" {
		t.Errorf("expected [host1, host2], got %v", names)
	}
	// Check that display_name was set in meta
	if snap1.Meta["display_name"] != "host1" {
		t.Errorf("snap1 display_name = %v, want host1", snap1.Meta["display_name"])
	}
}
