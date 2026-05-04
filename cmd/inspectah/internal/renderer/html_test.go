package renderer

import (
	"encoding/json"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// normalizeWhitespace trims trailing spaces from each line, normalizes
// line endings to \n, and trims leading/trailing blank lines.
func normalizeWhitespace(s string) string {
	s = strings.ReplaceAll(s, "\r\n", "\n")
	lines := strings.Split(s, "\n")
	for i := range lines {
		lines[i] = strings.TrimRight(lines[i], " \t")
	}
	return strings.TrimSpace(strings.Join(lines, "\n")) + "\n"
}

// extractFragment finds the first HTML element that matches the given
// regex pattern and returns its outer HTML. The pattern should match
// the opening tag.
func extractFragment(html string, openTag string) string {
	idx := strings.Index(html, openTag)
	if idx == -1 {
		return ""
	}

	// Determine the tag name from the opening tag
	re := regexp.MustCompile(`<(\w+)`)
	m := re.FindStringSubmatch(openTag)
	if m == nil {
		return ""
	}
	tagName := m[1]

	// Simple nested-tag-aware extraction: count open/close tags
	depth := 0
	i := idx
	openPattern := "<" + tagName
	closePattern := "</" + tagName
	for i < len(html) {
		if strings.HasPrefix(html[i:], closePattern) {
			depth--
			if depth == 0 {
				// Find the end of this closing tag
				end := strings.Index(html[i:], ">")
				if end == -1 {
					return html[idx:]
				}
				return html[idx : i+end+1]
			}
			i += len(closePattern)
		} else if strings.HasPrefix(html[i:], openPattern) {
			// Check it's actually an open tag (not e.g. <navigator)
			nextChar := html[i+len(openPattern)]
			if nextChar == ' ' || nextChar == '>' || nextChar == '/' || nextChar == '\n' {
				depth++
			}
			i += len(openPattern)
		} else {
			i++
		}
	}
	return html[idx:]
}

// extractJSConst extracts the value of a JavaScript const from the
// rendered HTML. It looks for `const NAME = ...;` and returns the
// unquoted string value (for string constants).
func extractJSConst(html, name string) string {
	marker := "const " + name + " = "
	idx := strings.Index(html, marker)
	if idx == -1 {
		return ""
	}
	start := idx + len(marker)
	// The value is a JSON string literal — find the matching quotes
	if start >= len(html) || html[start] != '"' {
		return ""
	}
	// Parse JSON string manually: find unescaped closing quote
	i := start + 1
	var sb strings.Builder
	for i < len(html) {
		if html[i] == '\\' && i+1 < len(html) {
			switch html[i+1] {
			case '"':
				sb.WriteByte('"')
			case '\\':
				sb.WriteByte('\\')
			case 'n':
				sb.WriteByte('\n')
			case 'r':
				sb.WriteByte('\r')
			case 't':
				sb.WriteByte('\t')
			case '/':
				sb.WriteByte('/')
			default:
				sb.WriteByte('\\')
				sb.WriteByte(html[i+1])
			}
			i += 2
		} else if html[i] == '"' {
			return sb.String()
		} else {
			sb.WriteByte(html[i])
			i++
		}
	}
	return sb.String()
}

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

	// The SPA template must embed data as JavaScript variables
	constants := []string{
		"const SNAPSHOT =",
		"const INITIAL_CONTAINERFILE =",
		"const TRIAGE_MANIFEST =",
	}
	for _, c := range constants {
		if !strings.Contains(content, c) {
			t.Errorf("missing embedded data variable: %q", c)
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

// ── Golden-file tests ──

// goldenTestHelper renders a report and returns the full HTML content.
func goldenTestHelper(t *testing.T, snap *schema.InspectionSnapshot, containerfile string) string {
	t.Helper()
	outDir := t.TempDir()
	os.WriteFile(filepath.Join(outDir, "Containerfile"), []byte(containerfile), 0644)

	err := RenderHTMLReport(snap, outDir, HTMLReportOptions{})
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	data, err := os.ReadFile(filepath.Join(outDir, "report.html"))
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	return string(data)
}

// goldenCompare compares a fragment against a golden file. If
// UPDATE_GOLDEN is set, it writes the golden file instead.
func goldenCompare(t *testing.T, goldenPath, got string) {
	t.Helper()
	got = normalizeWhitespace(got)

	if os.Getenv("UPDATE_GOLDEN") != "" {
		err := os.WriteFile(goldenPath, []byte(got), 0644)
		if err != nil {
			t.Fatalf("write golden %s: %v", goldenPath, err)
		}
		t.Logf("updated golden file: %s", goldenPath)
		return
	}

	wantData, err := os.ReadFile(goldenPath)
	if err != nil {
		t.Fatalf("read golden %s: %v\n(run with UPDATE_GOLDEN=1 to generate)", goldenPath, err)
	}
	want := normalizeWhitespace(string(wantData))

	if got != want {
		// Show a helpful diff — find first difference
		gotLines := strings.Split(got, "\n")
		wantLines := strings.Split(want, "\n")
		diffLine := 0
		for diffLine < len(gotLines) && diffLine < len(wantLines) {
			if gotLines[diffLine] != wantLines[diffLine] {
				break
			}
			diffLine++
		}
		t.Errorf("golden mismatch in %s at line %d\n--- want (around line %d) ---\n%s\n--- got (around line %d) ---\n%s\n\n(run with UPDATE_GOLDEN=1 to regenerate)",
			goldenPath, diffLine+1,
			diffLine+1, contextLines(wantLines, diffLine, 3),
			diffLine+1, contextLines(gotLines, diffLine, 3))
	}
}

// contextLines returns a few lines around the given index for diff output.
func contextLines(lines []string, center, radius int) string {
	start := center - radius
	if start < 0 {
		start = 0
	}
	end := center + radius + 1
	if end > len(lines) {
		end = len(lines)
	}
	return strings.Join(lines[start:end], "\n")
}

func TestGoldenSidebar(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "httpd", Version: "2.4.57", Release: "1.el9", Arch: "x86_64", Include: true, SourceRepo: "appstream"},
		},
	}
	html := goldenTestHelper(t, snap, "FROM test:latest\n")

	// Extract the <aside sidebar element (includes progress bar and nav container)
	fragment := extractFragment(html, `<aside class="pf-v6-c-page__sidebar"`)
	if fragment == "" {
		t.Fatal("could not extract sidebar <aside> element from rendered HTML")
	}

	goldenPath := filepath.Join("testdata", "golden-sidebar.html")
	goldenCompare(t, goldenPath, fragment)
}

func TestGoldenTierSection(t *testing.T) {
	// Build a snapshot with items in all 3 tiers:
	// - Tier 1: base-image package (matches baseline)
	// - Tier 2: third-party package (epel repo)
	// - Tier 3: local-install package (no repo)
	baseline := []string{"bash"}
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		BaselinePackageNames: &baseline,
		PackagesAdded: []schema.PackageEntry{
			// Tier 1: in baseline
			{Name: "bash", Version: "5.2.26", Release: "2.el9", Arch: "x86_64", State: schema.PackageStateAdded, Include: true, SourceRepo: "baseos"},
			// Tier 2: third-party repo (epel)
			{Name: "htop", Version: "3.3.0", Release: "1.el9", Arch: "x86_64", State: schema.PackageStateAdded, Include: true, SourceRepo: "epel"},
			// Tier 3: local install
			{Name: "custom-tool", Version: "1.0", Release: "1", Arch: "x86_64", State: "local_install", Include: true, SourceRepo: ""},
		},
	}
	html := goldenTestHelper(t, snap, "FROM rhel-bootc:9.4\n")

	// The section HTML is JS-rendered at runtime, so we extract the
	// TRIAGE_MANIFEST JSON from the rendered template — this contains the
	// classified items with tier, section, key, reason, etc.
	marker := "const TRIAGE_MANIFEST = "
	idx := strings.Index(html, marker)
	if idx == -1 {
		t.Fatal("could not find TRIAGE_MANIFEST in rendered HTML")
	}
	start := idx + len(marker)
	// Find the end: the manifest is a JSON array followed by a semicolon
	// First, unescape the script-close escaping
	rest := html[start:]
	end := strings.Index(rest, ";\n")
	if end == -1 {
		end = strings.Index(rest, ";")
	}
	if end == -1 {
		t.Fatal("could not find end of TRIAGE_MANIFEST")
	}
	manifestRaw := strings.ReplaceAll(rest[:end], `<\/`, "</")

	// Parse and verify all 3 tiers are present
	var items []TriageItem
	if err := json.Unmarshal([]byte(manifestRaw), &items); err != nil {
		t.Fatalf("parse TRIAGE_MANIFEST: %v", err)
	}

	tiers := map[int]bool{}
	for _, item := range items {
		tiers[item.Tier] = true
	}
	if !tiers[1] || !tiers[2] || !tiers[3] {
		t.Errorf("expected items in all 3 tiers, got tiers: %v", tiers)
	}

	// Re-marshal with indentation for a stable golden file
	formatted, err := json.MarshalIndent(items, "", "  ")
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	goldenPath := filepath.Join("testdata", "golden-tier-section.html")
	goldenCompare(t, goldenPath, string(formatted))
}

func TestGoldenContainerfile(t *testing.T) {
	snap := schema.NewSnapshot()
	containerfile := "FROM rhel-bootc:9.4\nRUN dnf install -y httpd\nCOPY ./config/ /etc/\n"
	html := goldenTestHelper(t, snap, containerfile)

	// Extract the INITIAL_CONTAINERFILE value from the rendered template
	value := extractJSConst(html, "INITIAL_CONTAINERFILE")
	if value == "" {
		t.Fatal("could not extract INITIAL_CONTAINERFILE from rendered HTML")
	}

	goldenPath := filepath.Join("testdata", "golden-containerfile.txt")
	goldenCompare(t, goldenPath, value)
}

// extractTriageManifest extracts TRIAGE_MANIFEST JSON from rendered HTML.
func extractTriageManifest(t *testing.T, html string) string {
	t.Helper()
	marker := "const TRIAGE_MANIFEST = "
	start := strings.Index(html, marker)
	if start == -1 {
		t.Fatal("TRIAGE_MANIFEST not found in rendered HTML")
	}
	start += len(marker)
	end := strings.Index(html[start:], ";\n")
	if end == -1 {
		end = strings.Index(html[start:], ";")
	}
	if end == -1 {
		t.Fatal("could not find end of TRIAGE_MANIFEST")
	}
	// Unescape script-close escaping
	return strings.ReplaceAll(html[start:start+end], `<\/`, "</")
}

func TestHTMLReportGoldenGroupedPackages(t *testing.T) {
	baseline := []string{"bash"}
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		BaselinePackageNames: &baseline,
		PackagesAdded: []schema.PackageEntry{
			{Name: "bash", Arch: "x86_64", Include: true, SourceRepo: "baseos", Version: "5.2", Release: "1.el9"},
			{Name: "vim", Arch: "x86_64", Include: true, SourceRepo: "appstream", Version: "9.1", Release: "1.el9"},
			{Name: "htop", Arch: "x86_64", Include: true, SourceRepo: "epel", Version: "3.3", Release: "1.el9"},
			{Name: "custom", Arch: "x86_64", Include: true, State: "local_install", Version: "1.0", Release: "1"},
		},
	}

	dir := t.TempDir()
	os.WriteFile(filepath.Join(dir, "Containerfile"), []byte("FROM test\n"), 0644)
	err := RenderHTMLReport(snap, dir, HTMLReportOptions{})
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	reportBytes, err := os.ReadFile(filepath.Join(dir, "report.html"))
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	report := string(reportBytes)

	manifestJSON := extractTriageManifest(t, report)
	var items []TriageItem
	if err := json.Unmarshal([]byte(manifestJSON), &items); err != nil {
		t.Fatalf("unmarshal TRIAGE_MANIFEST: %v", err)
	}

	if len(items) != 4 {
		t.Errorf("expected 4 items, got %d", len(items))
	}

	bash := findItem(items, "pkg-bash-x86_64")
	if bash == nil {
		t.Fatal("bash package not found in manifest")
	}
	if bash.Group != "base:baseos" {
		t.Errorf("bash group = %q, want %q", bash.Group, "base:baseos")
	}

	vim := findItem(items, "pkg-vim-x86_64")
	if vim == nil {
		t.Fatal("vim package not found in manifest")
	}
	if vim.Group != "user:appstream" {
		t.Errorf("vim group = %q, want %q", vim.Group, "user:appstream")
	}

	htop := findItem(items, "pkg-htop-x86_64")
	if htop == nil {
		t.Fatal("htop package not found in manifest")
	}
	if htop.Group != "user:epel" {
		t.Errorf("htop group = %q, want %q", htop.Group, "user:epel")
	}

	custom := findItem(items, "pkg-custom-x86_64")
	if custom == nil {
		t.Fatal("custom package not found in manifest")
	}
	if custom.Group != "" {
		t.Errorf("custom group = %q, want empty (ungrouped)", custom.Group)
	}
	if custom.CardType != "notification" {
		t.Errorf("custom cardType = %q, want %q", custom.CardType, "notification")
	}
}

func TestHTMLReportGoldenDisplayOnly(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Network = &schema.NetworkSection{
		Connections: []schema.NMConnection{
			{Name: "eth0", Type: "ethernet"},
		},
	}
	snap.Storage = &schema.StorageSection{
		FstabEntries: []schema.FstabEntry{
			{MountPoint: "/data", Fstype: "xfs"},
			{MountPoint: "/var", Fstype: "xfs"},
		},
	}

	dir := t.TempDir()
	os.WriteFile(filepath.Join(dir, "Containerfile"), []byte("FROM test\n"), 0644)
	err := RenderHTMLReport(snap, dir, HTMLReportOptions{})
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	reportBytes, err := os.ReadFile(filepath.Join(dir, "report.html"))
	if err != nil {
		t.Fatalf("read: %v", err)
	}

	manifestJSON := extractTriageManifest(t, string(reportBytes))
	var items []TriageItem
	if err := json.Unmarshal([]byte(manifestJSON), &items); err != nil {
		t.Fatalf("unmarshal TRIAGE_MANIFEST: %v", err)
	}

	eth0 := findItem(items, "conn-eth0")
	if eth0 == nil {
		t.Fatal("eth0 connection not found in manifest")
	}
	if !eth0.DisplayOnly {
		t.Error("network connection must be display-only")
	}
	if eth0.Group != "sub:network" {
		t.Errorf("eth0 group = %q, want %q", eth0.Group, "sub:network")
	}

	data := findItem(items, "fstab-/data")
	if data == nil {
		t.Fatal("/data fstab entry not found in manifest")
	}
	if !data.DisplayOnly {
		t.Error("fstab /data must be display-only")
	}
	if data.Group != "sub:fstab" {
		t.Errorf("/data group = %q, want %q", data.Group, "sub:fstab")
	}

	varMount := findItem(items, "fstab-/var")
	if varMount == nil {
		t.Fatal("/var fstab entry not found in manifest")
	}
	if !varMount.DisplayOnly {
		t.Error("fstab /var must be display-only")
	}
	if varMount.Group != "" {
		t.Errorf("/var group = %q, want empty (risky mount must be ungrouped)", varMount.Group)
	}
}

func TestHTMLReportNotificationPackageNoInstallLine(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "vim", Arch: "x86_64", Include: true, SourceRepo: "appstream", Version: "9.1", Release: "1.el9"},
			{Name: "local-tool", Arch: "x86_64", Include: true, State: schema.PackageStateLocalInstall, Version: "1.0", Release: "1"},
		},
	}

	dir := t.TempDir()
	err := RenderHTMLReport(snap, dir, HTMLReportOptions{})
	require.NoError(t, err)

	// Check Containerfile output embedded in the report
	reportBytes, err := os.ReadFile(filepath.Join(dir, "report.html"))
	require.NoError(t, err)
	report := string(reportBytes)

	// The manifest should have local-tool as notification
	manifestJSON := extractTriageManifest(t, report)
	var items []TriageItem
	require.NoError(t, json.Unmarshal([]byte(manifestJSON), &items))

	localTool := findItem(items, "pkg-local-tool-x86_64")
	require.NotNil(t, localTool)
	assert.Equal(t, "notification", localTool.CardType)

	// vim should be in the manifest as a regular grouped item
	vim := findItem(items, "pkg-vim-x86_64")
	require.NotNil(t, vim)
	assert.Equal(t, "user:appstream", vim.Group)
}

func TestHTMLReportGoldenLeafDeps(t *testing.T) {
	leafNames := []string{"vim", "htop"}
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		LeafPackages: &leafNames,
		LeafDepTree: map[string]interface{}{
			"vim":  []interface{}{"vim-common", "gpm-libs"},
			"htop": nil,
		},
		PackagesAdded: []schema.PackageEntry{
			{Name: "vim", Arch: "x86_64", Include: false, SourceRepo: "appstream", Version: "9.1", Release: "1.el9"},
			{Name: "vim-common", Arch: "x86_64", Include: false, SourceRepo: "appstream", Version: "9.1", Release: "1.el9"},
			{Name: "gpm-libs", Arch: "x86_64", Include: false, SourceRepo: "appstream", Version: "1.20", Release: "1.el9"},
			{Name: "htop", Arch: "x86_64", Include: false, SourceRepo: "epel", Version: "3.3", Release: "1.el9"},
		},
	}

	dir := t.TempDir()
	err := RenderHTMLReport(snap, dir, HTMLReportOptions{})
	require.NoError(t, err)

	reportBytes, err := os.ReadFile(filepath.Join(dir, "report.html"))
	require.NoError(t, err)
	manifestJSON := extractTriageManifest(t, string(reportBytes))
	var items []TriageItem
	require.NoError(t, json.Unmarshal([]byte(manifestJSON), &items))

	assert.Equal(t, 2, len(items))

	vim := findItem(items, "pkg-vim-x86_64")
	require.NotNil(t, vim)
	assert.Equal(t, []string{"vim-common", "gpm-libs"}, vim.Deps)
	assert.True(t, vim.DefaultInclude, "leaf should have DefaultInclude true after normalization")

	htop := findItem(items, "pkg-htop-x86_64")
	require.NotNil(t, htop)
	assert.Nil(t, htop.Deps)
	assert.True(t, htop.DefaultInclude)
}

func TestHTMLReportGoldenVersionChanges(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		VersionChanges: []schema.VersionChange{
			{Name: "bash", Arch: "x86_64", HostVersion: "5.2.26", BaseVersion: "5.2.32", Direction: schema.VersionChangeUpgrade},
		},
	}

	dir := t.TempDir()
	err := RenderHTMLReport(snap, dir, HTMLReportOptions{})
	require.NoError(t, err)

	reportBytes, _ := os.ReadFile(filepath.Join(dir, "report.html"))
	manifestJSON := extractTriageManifest(t, string(reportBytes))
	var items []TriageItem
	require.NoError(t, json.Unmarshal([]byte(manifestJSON), &items))

	bash := findItem(items, "verchg-bash-x86_64")
	require.NotNil(t, bash)
	assert.Equal(t, "version-changes", bash.Section)
	assert.True(t, bash.DisplayOnly)
	assert.Equal(t, "sub:version-upgrades", bash.Group)
}
