package renderer

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

func TestRenderHTMLReportMinimal(t *testing.T) {
	outDir := t.TempDir()
	snap := schema.NewSnapshot()

	// Write a minimal Containerfile so the report can embed it
	os.WriteFile(filepath.Join(outDir, "Containerfile"), []byte("FROM test:latest\n"), 0644)

	err := RenderHTMLReport(snap, outDir, HTMLReportOptions{})
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	data, err := os.ReadFile(filepath.Join(outDir, "report.html"))
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	content := string(data)

	checks := []string{
		"<!DOCTYPE html>",
		"inspectah",
		"<table",
		"System Type",
		"FROM test:latest",
	}
	for _, c := range checks {
		if !strings.Contains(content, c) {
			t.Errorf("report.html missing expected content: %q", c)
		}
	}
}

func TestRenderHTMLReportWithData(t *testing.T) {
	outDir := t.TempDir()

	snap := schema.NewSnapshot()
	snap.OsRelease = &schema.OsRelease{
		PrettyName: "Red Hat Enterprise Linux 9.4",
		ID:         "rhel",
	}
	snap.Meta = map[string]interface{}{
		"hostname": "test-host.example.com",
	}
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "httpd", Version: "2.4.57", Release: "1.el9", Arch: "x86_64", Include: true, SourceRepo: "appstream"},
		},
	}
	snap.Services = &schema.ServiceSection{
		EnabledUnits:  []string{"httpd.service"},
		DisabledUnits: []string{"firewalld.service"},
	}
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/httpd/conf/httpd.conf", Kind: schema.ConfigFileKindRpmOwnedModified, Category: schema.ConfigCategoryOther, Include: true},
		},
	}

	os.WriteFile(filepath.Join(outDir, "Containerfile"), []byte("FROM rhel-bootc:9.4\nRUN dnf install -y httpd\n"), 0644)

	err := RenderHTMLReport(snap, outDir, HTMLReportOptions{})
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	data, err := os.ReadFile(filepath.Join(outDir, "report.html"))
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	content := string(data)

	checks := []string{
		"Red Hat Enterprise Linux 9.4",
		"test-host.example.com",
		"httpd",
		"httpd.service",
		"firewalld.service",
		"/etc/httpd/conf/httpd.conf",
		"appstream",
	}
	for _, c := range checks {
		if !strings.Contains(content, c) {
			t.Errorf("report.html missing: %q", c)
		}
	}
}

func TestRenderHTMLReportSelfContained(t *testing.T) {
	outDir := t.TempDir()
	snap := schema.NewSnapshot()
	os.WriteFile(filepath.Join(outDir, "Containerfile"), []byte("FROM test\n"), 0644)

	err := RenderHTMLReport(snap, outDir, HTMLReportOptions{})
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	data, err := os.ReadFile(filepath.Join(outDir, "report.html"))
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	content := string(data)

	// Report should be self-contained — CSS inline, no external refs
	if !strings.Contains(content, "<style>") {
		t.Error("expected inline CSS")
	}
	if !strings.Contains(content, "</html>") {
		t.Error("expected closing html tag")
	}
}

func TestBuildSummaryHTML(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "a", Include: true},
			{Name: "b", Include: false},
		},
	}

	html := buildSummaryHTML(snap)
	if !strings.Contains(html, "1") { // 1 included package
		t.Error("expected package count of 1")
	}
}

func TestHTMLEscaping(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.OsRelease = &schema.OsRelease{
		PrettyName: "Test <script>alert('xss')</script>",
	}
	snap.Meta = map[string]interface{}{
		"hostname": "<b>evil</b>",
	}

	outDir := t.TempDir()
	os.WriteFile(filepath.Join(outDir, "Containerfile"), []byte("FROM test\n"), 0644)

	err := RenderHTMLReport(snap, outDir, HTMLReportOptions{})
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	data, err := os.ReadFile(filepath.Join(outDir, "report.html"))
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	content := string(data)

	// Should NOT contain unescaped HTML
	if strings.Contains(content, "<script>alert") {
		t.Error("XSS: unescaped script tag found")
	}
	if strings.Contains(content, "<b>evil</b>") {
		t.Error("XSS: unescaped HTML tag found")
	}
}
