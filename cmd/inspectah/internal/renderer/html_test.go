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
		"inspectah Report",
		"FROM test:latest",
		"pf-v6-theme-dark",
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

	// Snapshot data should be embedded in the JSON
	checks := []string{
		"Red Hat Enterprise Linux 9.4",
		"test-host.example.com",
		"httpd",
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

	// Report should be self-contained: inline CSS and JS, no external refs
	if !strings.Contains(content, "<style>") {
		t.Error("expected inline <style> tag (PatternFly CSS)")
	}
	if !strings.Contains(content, "<script>") {
		t.Error("expected inline <script> tag (CodeMirror JS)")
	}
	if !strings.Contains(content, "</html>") {
		t.Error("expected closing html tag")
	}

	// Must NOT contain external resource references
	externalPatterns := []string{
		`<link rel="stylesheet"`,
		`<script src=`,
		"cdn.jsdelivr.net",
		"unpkg.com",
		"cdnjs.cloudflare.com",
	}
	for _, p := range externalPatterns {
		if strings.Contains(content, p) {
			t.Errorf("report should be self-contained but contains external ref: %q", p)
		}
	}
}

func TestHTMLEscaping(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.OsRelease = &schema.OsRelease{
		PrettyName: `Test <script>alert('xss')</script>`,
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

	// XSS: unescaped script tags must not appear in the output.
	// The JSON marshaling escapes < and > in strings, and
	// escapeScriptClose handles </script> sequences.
	if strings.Contains(content, "<script>alert") {
		t.Error("XSS: unescaped script tag found in output")
	}
	if strings.Contains(content, "<b>evil</b>") {
		t.Error("XSS: unescaped HTML tag found in output")
	}
}

func TestHTMLSectionLandmarks(t *testing.T) {
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

	landmarks := []string{
		`id="main-content"`,
		`role="banner"`,
		`role="navigation"`,
		`role="main"`,
		`aria-label="Section navigation"`,
		`aria-label="Containerfile preview"`,
		`href="#main-content"`,
	}
	for _, l := range landmarks {
		if !strings.Contains(content, l) {
			t.Errorf("missing landmark: %q", l)
		}
	}
}

func TestHTMLDataEmbedding(t *testing.T) {
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

	// The SPA template must embed data as JavaScript constants
	constants := []string{
		"const SNAPSHOT =",
		"const INITIAL_CONTAINERFILE =",
		"const TRIAGE_MANIFEST =",
	}
	for _, c := range constants {
		if !strings.Contains(content, c) {
			t.Errorf("missing embedded data constant: %q", c)
		}
	}
}

func TestHTMLScriptCloseEscape(t *testing.T) {
	// Snapshot with </script> in a field — must be escaped
	snap := schema.NewSnapshot()
	snap.Meta = map[string]interface{}{
		"notes": `payload</script><script>alert(1)`,
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

	// Count script tags — there should be exactly 2 opening <script> tags
	// (one for CodeMirror, one for app code). If </script> in data wasn't
	// escaped, the parser would see extra tags.
	scriptOpens := strings.Count(content, "<script>")
	if scriptOpens != 2 {
		t.Errorf("expected 2 <script> tags, got %d (possible unescaped </script> in data)", scriptOpens)
	}
}

func TestHTMLNoContainerfile(t *testing.T) {
	outDir := t.TempDir()
	snap := schema.NewSnapshot()
	// No Containerfile written — renderer should handle gracefully

	err := RenderHTMLReport(snap, outDir, HTMLReportOptions{})
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	data, err := os.ReadFile(filepath.Join(outDir, "report.html"))
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	content := string(data)

	if !strings.Contains(content, "<!DOCTYPE html>") {
		t.Error("report should still render when Containerfile is absent")
	}
}

func TestEscapeScriptClose(t *testing.T) {
	tests := []struct {
		input string
		want  string
	}{
		{"no tags here", "no tags here"},
		{`</script>`, `<\/script>`},
		{`data</script><script>alert(1)`, `data<\/script><script>alert(1)`},
		{`</script></script>`, `<\/script><\/script>`},
		{`</Script>`, `<\/Script>`},
		{`</style>`, `<\/style>`},
	}
	for _, tt := range tests {
		got := escapeScriptClose(tt.input)
		if got != tt.want {
			t.Errorf("escapeScriptClose(%q) = %q, want %q", tt.input, got, tt.want)
		}
	}
}

func TestMarshalJSString(t *testing.T) {
	tests := []struct {
		input string
		want  string
	}{
		{"hello", `"hello"`},
		{"", `""`},
		{`line1\nline2`, `"line1\\nline2"`},
		{`"quoted"`, `"\"quoted\""`},
	}
	for _, tt := range tests {
		got := marshalJSString(tt.input)
		if got != tt.want {
			t.Errorf("marshalJSString(%q) = %q, want %q", tt.input, got, tt.want)
		}
	}
}
