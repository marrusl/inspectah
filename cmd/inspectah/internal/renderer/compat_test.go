package renderer

import (
	"html/template"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// TestJinja2TemplateInventory walks the Python template directory and
// catalogues all .j2 templates. This test documents the compatibility
// analysis that led to the decision to use Go's html/template instead
// of pongo2.
//
// === Compatibility Analysis Results ===
//
// Total templates: 33
// pongo2-compilable: 6 (simple variable interpolation only)
// Needs full rewrite: 27
//
// === Why html/template over pongo2 ===
//
// Decision: Use Go stdlib html/template for all template-based renderers.
// Approved by Mark on 2026-04-28.
//
// Rationale:
//
//  1. pongo2 cannot parse 27 of 33 Jinja2 templates due to:
//     - {% from "_macros.html.j2" import ... %} — no "from" tag
//     - {% macro name(args) %} ... {% endmacro %} — no macro tag
//     - {% call macro_name() %} ... {% endcall %} — no call/caller()
//     - Missing filters: tojson, selectattr, rejectattr, groupby,
//       map, unique, dictsort, xmlattr
//     - report.html.j2 fails at lexer level (comment syntax)
//     - architect/ templates fail on include resolution
//
//  2. Since 27/33 templates need a full rewrite anyway, there is no
//     benefit to maintaining a third-party dependency for the 6 that
//     happen to compile.
//
//  3. html/template is stdlib — zero dependencies, automatic context-
//     aware escaping for HTML output, well-documented, and idiomatic Go.
//
//  4. Template functions (FuncMap) replace Jinja2 macros naturally.
//
// === Renderer port strategy ===
//
// Template-based renderers (HTML report, architect):
//   - Rewrite templates using html/template syntax
//   - Macros become Go template functions via FuncMap
//   - Jinja2 includes become {{template "name" .}} calls
//   - go:embed for embedding at compile time
//
// Code-based renderers (Containerfile, audit, readme, kickstart,
// secrets-review, merge-notes):
//   - Port Python string-building logic directly to Go
//   - No templates needed — these build output programmatically
func TestJinja2TemplateInventory(t *testing.T) {
	templatesDir := filepath.Join("..", "..", "..", "..", "src", "inspectah", "templates")

	if _, err := os.Stat(templatesDir); os.IsNotExist(err) {
		t.Skipf("Python templates directory not found at %s — run from repo root", templatesDir)
	}

	var templateFiles []string

	err := filepath.Walk(templatesDir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		if info.IsDir() || filepath.Ext(path) != ".j2" {
			return nil
		}
		rel, _ := filepath.Rel(templatesDir, path)
		templateFiles = append(templateFiles, rel)
		return nil
	})
	if err != nil {
		t.Fatalf("walk: %v", err)
	}

	t.Logf("Found %d .j2 templates in Python source:", len(templateFiles))
	for _, f := range templateFiles {
		t.Logf("  %s", f)
	}

	// Verify we found the expected set
	if len(templateFiles) == 0 {
		t.Error("no .j2 templates found — expected at least 30")
	}
}

// TestGoTemplateBasicSyntax validates that html/template features needed
// by the renderer port work correctly. This replaces the earlier pongo2
// syntax tests.
func TestGoTemplateBasicSyntax(t *testing.T) {
	tests := []struct {
		name     string
		tmpl     string
		data     interface{}
		want     string
		funcs    template.FuncMap
	}{
		{
			name: "variable interpolation",
			tmpl: "Hello {{.Name}}!",
			data: struct{ Name string }{"inspectah"},
			want: "Hello inspectah!",
		},
		{
			name: "if/else",
			tmpl: `{{if gt .X 10}}big{{else if gt .X 5}}medium{{else}}small{{end}}`,
			data: struct{ X int }{7},
			want: "medium",
		},
		{
			name: "range loop",
			tmpl: `{{range .Items}}{{.}} {{end}}`,
			data: struct{ Items []string }{[]string{"a", "b", "c"}},
			want: "a b c",
		},
		{
			name: "len function",
			tmpl: `{{len .Items}}`,
			data: struct{ Items []string }{[]string{"a", "b"}},
			want: "2",
		},
		{
			name: "custom function",
			tmpl: `{{join .Items ", "}}`,
			data: struct{ Items []string }{[]string{"x", "y", "z"}},
			want: "x, y, z",
			funcs: template.FuncMap{
				"join": strings.Join,
			},
		},
		{
			name: "nested template",
			tmpl: `{{define "greeting"}}hello{{end}}before-{{template "greeting"}}-after`,
			data: nil,
			want: "before-hello-after",
		},
		{
			name: "html auto-escaping",
			tmpl: `{{.HTML}}`,
			data: struct{ HTML string }{"<b>bold</b>"},
			want: "&lt;b&gt;bold&lt;/b&gt;",
		},
		{
			name: "with block nil check",
			tmpl: `{{with .Name}}has name{{else}}no name{{end}}`,
			data: struct{ Name string }{""},
			want: "no name",
		},
		{
			name: "with block present",
			tmpl: `{{with .Name}}name={{.}}{{end}}`,
			data: struct{ Name string }{"test"},
			want: "name=test",
		},
		{
			name: "printf formatting",
			tmpl: `{{printf "%d items" .Count}}`,
			data: struct{ Count int }{42},
			want: "42 items",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpl := template.New("test")
			if tt.funcs != nil {
				tmpl = tmpl.Funcs(tt.funcs)
			}
			tmpl, err := tmpl.Parse(tt.tmpl)
			if err != nil {
				t.Fatalf("parse: %v", err)
			}
			var buf strings.Builder
			if err := tmpl.Execute(&buf, tt.data); err != nil {
				t.Fatalf("execute: %v", err)
			}
			got := strings.TrimSpace(buf.String())
			if got != tt.want {
				t.Errorf("got %q, want %q", got, tt.want)
			}
		})
	}
}
