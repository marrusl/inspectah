package schema

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestSnapshotRoundTrip(t *testing.T) {
	goldenPath := filepath.Join("testdata", "minimal-snapshot.json")

	// Load the golden file
	snap, err := LoadSnapshot(goldenPath)
	if err != nil {
		t.Fatalf("LoadSnapshot failed: %v", err)
	}

	// Re-marshal and unmarshal
	data, err := json.MarshalIndent(snap, "", "  ")
	if err != nil {
		t.Fatalf("MarshalIndent failed: %v", err)
	}

	var snap2 InspectionSnapshot
	if err := json.Unmarshal(data, &snap2); err != nil {
		t.Fatalf("Unmarshal failed: %v", err)
	}

	// Verify key fields
	if snap2.SchemaVersion != SchemaVersion {
		t.Errorf("schema_version mismatch: got %d, want %d", snap2.SchemaVersion, SchemaVersion)
	}

	if snap2.SystemType != SystemTypePackageMode {
		t.Errorf("system_type mismatch: got %q, want %q", snap2.SystemType, SystemTypePackageMode)
	}

	if snap2.Preflight.Status != "ok" {
		t.Errorf("preflight.status mismatch: got %q, want %q", snap2.Preflight.Status, "ok")
	}

	if len(snap2.Warnings) != 0 {
		t.Errorf("warnings not empty: got %d items", len(snap2.Warnings))
	}

	if len(snap2.Redactions) != 0 {
		t.Errorf("redactions not empty: got %d items", len(snap2.Redactions))
	}
}

func TestLoadSnapshot(t *testing.T) {
	goldenPath := filepath.Join("testdata", "minimal-snapshot.json")

	snap, err := LoadSnapshot(goldenPath)
	if err != nil {
		t.Fatalf("LoadSnapshot failed: %v", err)
	}

	if snap.SchemaVersion != SchemaVersion {
		t.Errorf("schema_version = %d, want %d", snap.SchemaVersion, SchemaVersion)
	}

	if snap.Meta == nil {
		t.Error("meta is nil, want empty map")
	}

	if snap.SystemType != SystemTypePackageMode {
		t.Errorf("system_type = %q, want %q", snap.SystemType, SystemTypePackageMode)
	}

	if snap.Preflight.Status != "ok" {
		t.Errorf("preflight.status = %q, want %q", snap.Preflight.Status, "ok")
	}
}

func TestLoadSnapshotVersionMismatch(t *testing.T) {
	// Create a temporary snapshot with wrong schema version
	tmpDir := t.TempDir()
	tmpPath := filepath.Join(tmpDir, "bad-version.json")

	badSnap := map[string]interface{}{
		"schema_version": 999,
		"meta":           map[string]interface{}{},
		"system_type":    "package-mode",
		"preflight": map[string]interface{}{
			"status":                 "ok",
			"unavailable_packages":   []interface{}{},
			"unverifiable_packages":  []interface{}{},
			"repo_errors":            []interface{}{},
		},
		"warnings":   []interface{}{},
		"redactions": []interface{}{},
	}

	data, err := json.MarshalIndent(badSnap, "", "  ")
	if err != nil {
		t.Fatalf("MarshalIndent failed: %v", err)
	}

	if err := os.WriteFile(tmpPath, data, 0644); err != nil {
		t.Fatalf("WriteFile failed: %v", err)
	}

	// Attempt to load and expect error
	_, err = LoadSnapshot(tmpPath)
	if err == nil {
		t.Fatal("LoadSnapshot should fail on version mismatch")
	}

	expectedMsg := "schema version mismatch"
	if !contains(err.Error(), expectedMsg) {
		t.Errorf("error message = %q, want substring %q", err.Error(), expectedMsg)
	}
}

func TestSaveSnapshot(t *testing.T) {
	tmpDir := t.TempDir()
	tmpPath := filepath.Join(tmpDir, "saved-snapshot.json")

	// Create a minimal snapshot
	snap := NewSnapshot()
	snap.SystemType = SystemTypePackageMode
	snap.Preflight = PreflightResult{
		Status:          "ok",
		Available:       []string{},
		Unavailable:     []string{},
		Unverifiable:    []UnverifiablePackage{},
		DirectInstall:   []string{},
		RepoUnreachable: []RepoStatus{},
		ReposQueried:    []string{},
	}

	// Save it
	if err := SaveSnapshot(snap, tmpPath); err != nil {
		t.Fatalf("SaveSnapshot failed: %v", err)
	}

	// Reload and verify
	snap2, err := LoadSnapshot(tmpPath)
	if err != nil {
		t.Fatalf("LoadSnapshot failed: %v", err)
	}

	if snap2.SchemaVersion != SchemaVersion {
		t.Errorf("schema_version = %d, want %d", snap2.SchemaVersion, SchemaVersion)
	}

	if snap2.SystemType != SystemTypePackageMode {
		t.Errorf("system_type = %q, want %q", snap2.SystemType, SystemTypePackageMode)
	}

	if snap2.Preflight.Status != "ok" {
		t.Errorf("preflight.status = %q, want %q", snap2.Preflight.Status, "ok")
	}
}

func TestParseRedaction(t *testing.T) {
	validJSON := json.RawMessage(`{
		"path": "/etc/foo.conf",
		"source": "config",
		"kind": "credential",
		"pattern": "password=.*",
		"remediation": "Move to secret store",
		"detection_method": "regex"
	}`)

	finding, err := ParseRedaction(validJSON)
	if err != nil {
		t.Fatalf("ParseRedaction failed: %v", err)
	}

	if finding.Path != "/etc/foo.conf" {
		t.Errorf("path = %q, want %q", finding.Path, "/etc/foo.conf")
	}

	if finding.Kind != "credential" {
		t.Errorf("kind = %q, want %q", finding.Kind, "credential")
	}

	// Test invalid JSON
	invalidJSON := json.RawMessage(`{"not": "a redaction"}`)
	_, err = ParseRedaction(invalidJSON)
	// Should not error, but fields will be empty/default
	// (Go json.Unmarshal is permissive with missing fields)
	if err != nil {
		t.Errorf("ParseRedaction with partial data should not error: %v", err)
	}
}

func TestNewSnapshot(t *testing.T) {
	snap := NewSnapshot()

	if snap.SchemaVersion != SchemaVersion {
		t.Errorf("schema_version = %d, want %d", snap.SchemaVersion, SchemaVersion)
	}

	if snap.Meta == nil {
		t.Error("meta is nil, want empty map")
	}

	if snap.SystemType != SystemTypePackageMode {
		t.Errorf("system_type = %q, want %q", snap.SystemType, SystemTypePackageMode)
	}

	if snap.Warnings == nil {
		t.Error("warnings is nil, want empty slice")
	}

	if snap.Redactions == nil {
		t.Error("redactions is nil, want empty slice")
	}
}

func TestNormalizeSnapshot_SetsNilIncludeToTrue(t *testing.T) {
	snap := NewSnapshot()
	snap.ScheduledTasks = &ScheduledTaskSection{
		SystemdTimers: []SystemdTimer{{Name: "test.timer", Source: "local"}},
		AtJobs:        []AtJob{{File: "a1", Command: "echo hi"}},
	}
	snap.Containers = &ContainerSection{
		RunningContainers: []RunningContainer{{Name: "web", Image: "nginx"}},
	}
	snap.Network = &NetworkSection{
		Connections: []NMConnection{{Name: "eth0", Type: "802-3-ethernet"}},
	}
	snap.Storage = &StorageSection{
		FstabEntries: []FstabEntry{{MountPoint: "/data", Fstype: "xfs"}},
	}
	snap.UsersGroups = &UserGroupSection{
		Users:  []map[string]interface{}{{"name": "app", "uid": float64(1001)}},
		Groups: []map[string]interface{}{{"name": "app", "gid": float64(1001)}},
	}
	snap.Selinux = &SelinuxSection{
		BooleanOverrides: []map[string]interface{}{{"name": "httpd_can_network_connect", "current_value": "on"}},
	}

	// Before normalization: nil/absent
	if snap.ScheduledTasks.SystemdTimers[0].Include != nil {
		t.Error("timer Include should be nil before normalization")
	}
	if snap.Network.Connections[0].Include != nil {
		t.Error("connection Include should be nil before normalization")
	}
	if snap.Storage.FstabEntries[0].Include != nil {
		t.Error("fstab Include should be nil before normalization")
	}
	if _, hasInclude := snap.UsersGroups.Users[0]["include"]; hasInclude {
		t.Error("user should not have include key before normalization")
	}

	NormalizeSnapshot(snap)

	// After: explicit true
	if snap.ScheduledTasks.SystemdTimers[0].Include == nil || !*snap.ScheduledTasks.SystemdTimers[0].Include {
		t.Error("timer Include should be true after normalization")
	}
	if snap.ScheduledTasks.AtJobs[0].Include == nil || !*snap.ScheduledTasks.AtJobs[0].Include {
		t.Error("at job Include should be true after normalization")
	}
	if snap.Containers.RunningContainers[0].Include == nil || !*snap.Containers.RunningContainers[0].Include {
		t.Error("running container Include should be true after normalization")
	}
	if snap.Network.Connections[0].Include == nil || !*snap.Network.Connections[0].Include {
		t.Error("connection Include should be true after normalization")
	}
	if snap.Storage.FstabEntries[0].Include == nil || !*snap.Storage.FstabEntries[0].Include {
		t.Error("fstab Include should be true after normalization")
	}
	if inc, ok := snap.UsersGroups.Users[0]["include"]; !ok || inc != true {
		t.Error("user include should be true after normalization")
	}
	if inc, ok := snap.UsersGroups.Groups[0]["include"]; !ok || inc != true {
		t.Error("group include should be true after normalization")
	}
	if inc, ok := snap.Selinux.BooleanOverrides[0]["include"]; !ok || inc != true {
		t.Error("boolean override include should be true after normalization")
	}
	if snap.SchemaVersion != SchemaVersion {
		t.Errorf("schema version should be %d after normalization, got %d", SchemaVersion, snap.SchemaVersion)
	}
}

// Helper to check if a string contains a substring
func contains(s, substr string) bool {
	return len(s) >= len(substr) && (s == substr || len(substr) == 0 ||
		(len(s) > 0 && len(substr) > 0 && containsAt(s, substr, 0)))
}

func containsAt(s, substr string, start int) bool {
	if start+len(substr) > len(s) {
		return false
	}
	for i := start; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}
