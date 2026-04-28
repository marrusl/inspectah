package renderer

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/flosch/pongo2/v6"
)

// TestPongo2TemplateCompilation walks the Jinja2 template directory and
// attempts to compile each .j2 file with pongo2.  Failures catalogue
// syntax incompatibilities that must be fixed before the template port.
//
// Known Jinja2 → pongo2 gaps (catalogued from this test run):
//
// Result: 6 compiled, 27 need porting.
//
// Primary issues:
//
//   - {% from "_macros.html.j2" import ... %}  → pongo2 has no "from" tag.
//     Nearly all report/ partials use this to import macros.  Macros must
//     be converted to pongo2 includes or Go-side helper functions.
//
//   - {% macro name(args) %} ... {% endmacro %}  → pongo2 has no macro tag.
//     The _macros.html.j2 partial defines shared macros used by all other
//     report partials.  These become either pongo2 include blocks or Go
//     template helper functions that inject pre-rendered HTML.
//
//   - {% call macro_name() %} ... {% endcall %}  → no equivalent in pongo2.
//
//   - {{ caller() }}  → pongo2 has no caller() support.
//
//   - Jinja2 filters not in pongo2: tojson, selectattr, rejectattr,
//     groupby, map, unique, dictsort, xmlattr.
//
//   - report.html.j2 fails at lexer level (single-line comment with
//     newline).  This is the main shell template.
//
//   - architect/ templates fail on include resolution (expected — they
//     use relative includes that need a loader).
//
//   - For loops: pongo2 trims trailing whitespace (minor behavioral
//     difference, not a blocker).
//
// Strategy for port: The HTML report and its partials are complex Jinja2
// with heavy macro usage.  Rather than trying to make pongo2 parse them,
// the Go port uses Go-side context building (pre-render sections in Go
// code) and simpler pongo2 templates that receive pre-built HTML strings.
// This is the same approach the Python code already uses for several
// sections (file browser tree, audit report HTML).
func TestPongo2TemplateCompilation(t *testing.T) {
	templatesDir := filepath.Join("..", "..", "..", "..", "src", "inspectah", "templates")

	if _, err := os.Stat(templatesDir); os.IsNotExist(err) {
		t.Skipf("Python templates directory not found at %s — run from repo root", templatesDir)
	}

	var compiled, failed int
	var failures []string

	err := filepath.Walk(templatesDir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		if info.IsDir() {
			return nil
		}
		if filepath.Ext(path) != ".j2" {
			return nil
		}

		rel, _ := filepath.Rel(templatesDir, path)
		t.Run(rel, func(t *testing.T) {
			data, err := os.ReadFile(path)
			if err != nil {
				t.Fatalf("read: %v", err)
			}

			_, err = pongo2.FromString(string(data))
			if err != nil {
				failed++
				failures = append(failures, rel+": "+err.Error())
				// Log but don't fail — we expect many templates to need
				// porting.  This test catalogues the work needed.
				t.Logf("pongo2 compilation issue: %v", err)
			} else {
				compiled++
			}
		})
		return nil
	})
	if err != nil {
		t.Fatalf("walk: %v", err)
	}

	t.Logf("\n=== pongo2 Compatibility Summary ===")
	t.Logf("Compiled successfully: %d", compiled)
	t.Logf("Need porting:         %d", failed)
	if len(failures) > 0 {
		t.Logf("\nDetailed failures:")
		for _, f := range failures {
			t.Logf("  - %s", f)
		}
	}

	// This test is intentionally advisory — it documents compat gaps
	// rather than blocking CI.  Individual subtests log issues without
	// calling t.Error so the overall test passes while recording findings.
}

// TestPongo2BasicSyntax validates that core pongo2 features work as
// expected for the template patterns used in inspectah templates.
func TestPongo2BasicSyntax(t *testing.T) {
	tests := []struct {
		name     string
		template string
		ctx      pongo2.Context
		want     string
	}{
		{
			name:     "variable interpolation",
			template: "Hello {{ name }}!",
			ctx:      pongo2.Context{"name": "inspectah"},
			want:     "Hello inspectah!",
		},
		{
			name:     "if/elif/else",
			template: "{% if x > 10 %}big{% elif x > 5 %}medium{% else %}small{% endif %}",
			ctx:      pongo2.Context{"x": 7},
			want:     "medium",
		},
		{
			name:     "for loop",
			template: "{% for item in items %}{{ item }} {% endfor %}",
			ctx:      pongo2.Context{"items": []string{"a", "b", "c"}},
			want:     "a b c", // pongo2 trims trailing whitespace
		},
		{
			name:     "filter length",
			template: "{{ items|length }}",
			ctx:      pongo2.Context{"items": []string{"a", "b"}},
			want:     "2",
		},
		{
			name:     "filter default",
			template: "{{ missing|default:\"fallback\" }}",
			ctx:      pongo2.Context{},
			want:     "fallback",
		},
		{
			name:     "filter join",
			template: "{{ items|join:\", \" }}",
			ctx:      pongo2.Context{"items": []string{"x", "y", "z"}},
			want:     "x, y, z",
		},
		{
			name:     "set tag",
			template: "{% set greeting = \"hi\" %}{{ greeting }}",
			ctx:      pongo2.Context{},
			want:     "hi",
		},
		{
			name:     "include tag",
			template: "before{% include \"_partial.html\" %}after",
			ctx:      pongo2.Context{},
			// include won't resolve without a loader — just verify parse
		},
		{
			name:     "autoescape",
			template: "{{ html }}",
			ctx:      pongo2.Context{"html": "<b>bold</b>"},
			want:     "&lt;b&gt;bold&lt;/b&gt;",
		},
		{
			name:     "safe filter",
			template: "{{ html|safe }}",
			ctx:      pongo2.Context{"html": "<b>bold</b>"},
			want:     "<b>bold</b>",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tpl, err := pongo2.FromString(tt.template)
			if err != nil {
				if tt.name == "include tag" {
					// Include requires a loader — parse failure is expected
					t.Logf("expected parse issue for include without loader: %v", err)
					return
				}
				t.Fatalf("compile: %v", err)
			}
			if tt.want == "" {
				return // parse-only test
			}
			got, err := tpl.Execute(tt.ctx)
			if err != nil {
				t.Fatalf("execute: %v", err)
			}
			got = strings.TrimSpace(got)
			if got != tt.want {
				t.Errorf("got %q, want %q", got, tt.want)
			}
		})
	}
}
