package renderer

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// --- Audit Report ---

func TestRenderAuditReportMinimal(t *testing.T) {
	outDir := t.TempDir()
	snap := schema.NewSnapshot()

	err := RenderAuditReport(snap, outDir, nil)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	data, err := os.ReadFile(filepath.Join(outDir, "audit-report.md"))
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	if !strings.Contains(string(data), "Audit Report") {
		t.Error("missing header")
	}
}

func TestRenderAuditReportWithPackages(t *testing.T) {
	outDir := t.TempDir()
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "httpd", Include: true, State: schema.PackageStateAdded},
			{Name: "nginx", Include: true, State: schema.PackageStateAdded},
		},
		VersionChanges: []schema.VersionChange{
			{Name: "openssl", HostVersion: "3.0.8", BaseVersion: "3.0.7", Direction: schema.VersionChangeUpgrade},
		},
	}

	err := RenderAuditReport(snap, outDir, nil)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	data, _ := os.ReadFile(filepath.Join(outDir, "audit-report.md"))
	content := string(data)
	if !strings.Contains(content, "httpd") {
		t.Error("missing httpd")
	}
	if !strings.Contains(content, "openssl") {
		t.Error("missing version change")
	}
}

func TestRenderAuditReportWithConfig(t *testing.T) {
	outDir := t.TempDir()
	snap := schema.NewSnapshot()
	diff := "--- a/file\n+++ b/file\n@@ -1 +1 @@\n-old\n+new"
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/httpd/conf/httpd.conf", Kind: schema.ConfigFileKindRpmOwnedModified,
				Include: true, DiffAgainstRpm: &diff},
		},
	}

	err := RenderAuditReport(snap, outDir, nil)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	data, _ := os.ReadFile(filepath.Join(outDir, "audit-report.md"))
	content := string(data)
	if !strings.Contains(content, "httpd.conf") {
		t.Error("missing config file")
	}
}

// --- README ---

func TestRenderReadmeMinimal(t *testing.T) {
	outDir := t.TempDir()
	snap := schema.NewSnapshot()

	err := RenderReadme(snap, outDir)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	data, err := os.ReadFile(filepath.Join(outDir, "README.md"))
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	content := string(data)
	if !strings.Contains(content, "inspectah") {
		t.Error("missing header")
	}
	if !strings.Contains(content, "Containerfile") {
		t.Error("missing artifact reference")
	}
}

func TestRenderReadmeWithFIXMEs(t *testing.T) {
	outDir := t.TempDir()
	// Write a Containerfile with FIXME comments
	os.WriteFile(filepath.Join(outDir, "Containerfile"),
		[]byte("FROM test\n# FIXME: resolve this issue\n# FIXME: another issue\n"), 0644)

	snap := schema.NewSnapshot()
	err := RenderReadme(snap, outDir)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	data, _ := os.ReadFile(filepath.Join(outDir, "README.md"))
	content := string(data)
	if !strings.Contains(content, "FIXME") {
		t.Error("missing FIXME section")
	}
}

// --- Kickstart ---

func TestRenderKickstartMinimal(t *testing.T) {
	outDir := t.TempDir()
	snap := schema.NewSnapshot()

	err := RenderKickstart(snap, outDir)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	data, err := os.ReadFile(filepath.Join(outDir, "kickstart-suggestion.ks"))
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	if !strings.Contains(string(data), "Kickstart") {
		t.Error("missing header")
	}
}

func TestRenderKickstartWithNetwork(t *testing.T) {
	outDir := t.TempDir()
	snap := schema.NewSnapshot()
	snap.Network = &schema.NetworkSection{
		Connections: []schema.NMConnection{
			{Name: "eth0", Method: "auto", Path: "/etc/NetworkManager/system-connections/eth0.nmconnection"},
		},
		HostsAdditions: []string{"192.168.1.1 myserver"},
	}

	err := RenderKickstart(snap, outDir)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	data, _ := os.ReadFile(filepath.Join(outDir, "kickstart-suggestion.ks"))
	content := string(data)
	if !strings.Contains(content, "DHCP") || !strings.Contains(content, "eth0") {
		t.Error("missing DHCP connection info")
	}
}

func TestRenderKickstartWithUsers(t *testing.T) {
	outDir := t.TempDir()
	snap := schema.NewSnapshot()
	snap.UsersGroups = &schema.UserGroupSection{
		Users: []map[string]interface{}{
			{"name": "deployer", "strategy": "kickstart", "include": true, "uid": float64(1001)},
		},
	}

	err := RenderKickstart(snap, outDir)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	data, _ := os.ReadFile(filepath.Join(outDir, "kickstart-suggestion.ks"))
	content := string(data)
	if !strings.Contains(content, "deployer") {
		t.Error("missing kickstart user")
	}
}

// --- Secrets Review ---

func TestRenderSecretsReviewEmpty(t *testing.T) {
	outDir := t.TempDir()
	snap := schema.NewSnapshot()

	err := RenderSecretsReview(snap, outDir)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	data, _ := os.ReadFile(filepath.Join(outDir, "secrets-review.md"))
	content := string(data)
	if !strings.Contains(content, "No redactions") {
		t.Error("expected 'No redactions' message")
	}
}

func TestRenderSecretsReviewWithFindings(t *testing.T) {
	outDir := t.TempDir()
	snap := schema.NewSnapshot()
	snap.Redactions = append(snap.Redactions,
		mustMarshal(schema.RedactionFinding{
			Path:        "/etc/pki/tls/private/server.key",
			Source:      "file",
			Kind:        "excluded",
			Pattern:     "private_key",
			Remediation: "provision",
		}),
		mustMarshal(schema.RedactionFinding{
			Path:        "/etc/shadow",
			Source:      "shadow",
			Kind:        "inline",
			Pattern:     "password_hash",
			Remediation: "value-removed",
		}),
	)

	err := RenderSecretsReview(snap, outDir)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	data, _ := os.ReadFile(filepath.Join(outDir, "secrets-review.md"))
	content := string(data)
	if !strings.Contains(content, "server.key") {
		t.Error("missing excluded file")
	}
	if !strings.Contains(content, "shadow") {
		t.Error("missing inline redaction")
	}
}

// --- Merge Notes ---

func TestRenderMergeNotesEmpty(t *testing.T) {
	outDir := t.TempDir()
	snap := schema.NewSnapshot()

	err := RenderMergeNotes(snap, outDir)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	// No merge-notes.md when no fleet data
	if _, err := os.Stat(filepath.Join(outDir, "merge-notes.md")); !os.IsNotExist(err) {
		t.Error("merge-notes.md should not be created for non-fleet snapshots")
	}
}

func TestRenderMergeNotesWithTies(t *testing.T) {
	outDir := t.TempDir()
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/test.conf", Include: true, Content: "variant1",
				Tie: true, TieWinner: true,
				Fleet: &schema.FleetPrevalence{Count: 2, Total: 3, Hosts: []string{"host1", "host2"}}},
			{Path: "/etc/test.conf", Include: false, Content: "variant2",
				Tie: true, TieWinner: false,
				Fleet: &schema.FleetPrevalence{Count: 1, Total: 3, Hosts: []string{"host3"}}},
		},
	}

	err := RenderMergeNotes(snap, outDir)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	data, err := os.ReadFile(filepath.Join(outDir, "merge-notes.md"))
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	content := string(data)
	if !strings.Contains(content, "Fleet Merge Notes") {
		t.Error("missing header")
	}
	if !strings.Contains(content, "test.conf") {
		t.Error("missing tied file")
	}
}
