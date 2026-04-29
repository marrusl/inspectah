package renderer

import (
	"encoding/json"
	"fmt"
	"html/template"
	"os"
	"path/filepath"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// HTMLReportOptions configures the HTML report renderer.
type HTMLReportOptions struct {
	RefineMode       bool
	OriginalSnapshot *schema.InspectionSnapshot
}

// reportData is the template data passed to report.html.
type reportData struct {
	PatternFlyCSS  template.CSS
	CodeMirrorJS   template.JS
	SnapshotJSON   template.JS
	Containerfile  template.JS
	TriageManifest template.JS
}

// RenderHTMLReport produces the interactive HTML report using
// html/template with embedded PatternFly CSS and CodeMirror JS.
func RenderHTMLReport(snap *schema.InspectionSnapshot, outputDir string, opts HTMLReportOptions) error {
	tmpl, err := template.New("report").Parse(reportTemplate)
	if err != nil {
		return fmt.Errorf("parse report template: %w", err)
	}

	snapJSON, err := json.Marshal(snap)
	if err != nil {
		return fmt.Errorf("marshal snapshot: %w", err)
	}

	manifest := ClassifySnapshot(snap, opts.OriginalSnapshot)
	manifestJSON, err := json.Marshal(manifest)
	if err != nil {
		return fmt.Errorf("marshal triage manifest: %w", err)
	}

	cfData, _ := os.ReadFile(filepath.Join(outputDir, "Containerfile"))
	cfJSON, _ := json.Marshal(string(cfData))

	data := reportData{
		PatternFlyCSS:  template.CSS(patternFlyCSS),
		CodeMirrorJS:   template.JS(codeMirrorJS),
		SnapshotJSON:   template.JS(escapeScriptClose(string(snapJSON))),
		Containerfile:  template.JS(escapeScriptClose(string(cfJSON))),
		TriageManifest: template.JS(escapeScriptClose(string(manifestJSON))),
	}

	outPath := filepath.Join(outputDir, "report.html")
	f, err := os.Create(outPath)
	if err != nil {
		return fmt.Errorf("create report.html: %w", err)
	}
	defer f.Close()

	return tmpl.Execute(f, data)
}

// escapeScriptClose prevents </script> sequences in embedded data from
// prematurely closing the script tag. This is a standard defense for
// inline JSON in HTML.
func escapeScriptClose(s string) string {
	return strings.ReplaceAll(s, "</", `<\/`)
}

// marshalJSString returns s as a JSON-encoded string literal (with quotes).
func marshalJSString(s string) string {
	data, _ := json.Marshal(s)
	return string(data)
}
