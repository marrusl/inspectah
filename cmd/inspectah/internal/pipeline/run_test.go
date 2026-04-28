package pipeline

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

func TestRunFromSnapshot(t *testing.T) {
	tmpDir := t.TempDir()
	outDir := filepath.Join(tmpDir, "output")

	// Create a minimal snapshot file
	snap := schema.NewSnapshot()
	snapPath := filepath.Join(tmpDir, "test-snapshot.json")
	if err := schema.SaveSnapshot(snap, snapPath); err != nil {
		t.Fatalf("save snapshot: %v", err)
	}

	result, err := Run(RunOptions{
		FromSnapshotPath: snapPath,
		OutputDir:        outDir,
	})
	if err != nil {
		t.Fatalf("Run: %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil snapshot result")
	}

	// Check output files exist
	for _, name := range []string{"inspection-snapshot.json", "Containerfile", "README.md", "audit-report.md", "report.html"} {
		if _, err := os.Stat(filepath.Join(outDir, name)); err != nil {
			t.Errorf("missing output file: %s", name)
		}
	}
}

func TestRunWithRedaction(t *testing.T) {
	tmpDir := t.TempDir()
	outDir := filepath.Join(tmpDir, "output")

	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/app.conf", Content: "password=supersecret123", Include: true},
		},
	}
	snapPath := filepath.Join(tmpDir, "snap.json")
	schema.SaveSnapshot(snap, snapPath)

	result, err := Run(RunOptions{
		FromSnapshotPath: snapPath,
		OutputDir:        outDir,
	})
	if err != nil {
		t.Fatalf("Run: %v", err)
	}

	// Verify the password was redacted in the snapshot
	for _, f := range result.Config.Files {
		if f.Path == "/etc/app.conf" && strings.Contains(f.Content, "supersecret123") {
			t.Error("password not redacted in output snapshot")
		}
	}
}

func TestRunInspectOnly(t *testing.T) {
	tmpDir := t.TempDir()

	snap := schema.NewSnapshot()
	snapPath := filepath.Join(tmpDir, "snap.json")
	schema.SaveSnapshot(snap, snapPath)

	// Change to tmpDir so inspect-only output lands there
	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	_, err := Run(RunOptions{
		FromSnapshotPath: snapPath,
		InspectOnly:      true,
	})
	if err != nil {
		t.Fatalf("Run inspect-only: %v", err)
	}

	// Should create inspection-snapshot.json in current dir
	if _, err := os.Stat(filepath.Join(tmpDir, "inspection-snapshot.json")); err != nil {
		t.Error("missing inspect-only output")
	}
}

func TestRunNoRedaction(t *testing.T) {
	tmpDir := t.TempDir()
	outDir := filepath.Join(tmpDir, "output")

	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/app.conf", Content: "password=keepthis", Include: true},
		},
	}
	snapPath := filepath.Join(tmpDir, "snap.json")
	schema.SaveSnapshot(snap, snapPath)

	result, err := Run(RunOptions{
		FromSnapshotPath: snapPath,
		OutputDir:        outDir,
		NoRedaction:      true,
	})
	if err != nil {
		t.Fatalf("Run no-redaction: %v", err)
	}

	// Content should be preserved
	for _, f := range result.Config.Files {
		if f.Path == "/etc/app.conf" && !strings.Contains(f.Content, "keepthis") {
			t.Error("no-redaction should preserve original content")
		}
	}

	// But findings should still be recorded
	if len(result.Redactions) == 0 {
		t.Error("no-redaction should still record findings")
	}
}

func TestSanitizeHostname(t *testing.T) {
	tests := []struct {
		input string
		want  string
	}{
		{"myhost.example.com", "myhost.example.com"},
		{"host name with spaces", "hostnamewithspaces"},
		{"host/with/slashes", "hostwithslashes"},
		{"", "unknown"},
	}
	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			got := SanitizeHostname(tt.input)
			if got != tt.want {
				t.Errorf("got %q, want %q", got, tt.want)
			}
		})
	}
}

func TestCreateTarball(t *testing.T) {
	tmpDir := t.TempDir()
	srcDir := filepath.Join(tmpDir, "src")
	os.MkdirAll(srcDir, 0755)
	os.WriteFile(filepath.Join(srcDir, "test.txt"), []byte("hello"), 0644)

	tarPath := filepath.Join(tmpDir, "test.tar.gz")
	err := CreateTarball(srcDir, tarPath, "prefix")
	if err != nil {
		t.Fatalf("CreateTarball: %v", err)
	}

	info, err := os.Stat(tarPath)
	if err != nil {
		t.Fatalf("tarball not created: %v", err)
	}
	if info.Size() == 0 {
		t.Error("tarball is empty")
	}
}

func TestBundleSubscriptionCertsEmpty(t *testing.T) {
	tmpDir := t.TempDir()
	err := BundleSubscriptionCerts("/nonexistent", tmpDir)
	if err != nil {
		t.Errorf("should not error on missing host root: %v", err)
	}
}

func TestBundleSubscriptionCertsWithCerts(t *testing.T) {
	tmpDir := t.TempDir()
	outDir := filepath.Join(tmpDir, "output")
	hostRoot := filepath.Join(tmpDir, "host")

	// Create fake entitlement certs
	entDir := filepath.Join(hostRoot, "etc", "pki", "entitlement")
	os.MkdirAll(entDir, 0755)
	os.WriteFile(filepath.Join(entDir, "1234.pem"), []byte("cert"), 0644)
	os.WriteFile(filepath.Join(entDir, "1234-key.pem"), []byte("key"), 0644)

	err := BundleSubscriptionCerts(hostRoot, outDir)
	if err != nil {
		t.Fatalf("bundle: %v", err)
	}

	// Check certs were copied
	if _, err := os.Stat(filepath.Join(outDir, "entitlement", "1234.pem")); err != nil {
		t.Error("missing cert file")
	}
	if _, err := os.Stat(filepath.Join(outDir, "entitlement", "1234-key.pem")); err != nil {
		t.Error("missing key file")
	}
}
