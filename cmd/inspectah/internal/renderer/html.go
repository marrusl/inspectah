package renderer

import (
	"encoding/json"
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
		return err
	}

	// Marshal snapshot to JSON for embedding
	snapJSON, err := json.Marshal(snap)
	if err != nil {
		return err
	}

	// Read Containerfile from outputDir
	containerfile := ""
	cfPath := filepath.Join(outputDir, "Containerfile")
	if data, err := os.ReadFile(cfPath); err == nil {
		containerfile = string(data)
	}

	// Build template data
	data := reportData{
		PatternFlyCSS:  template.CSS(patternFlyCSS),
		CodeMirrorJS:   template.JS(codeMirrorJS),
		SnapshotJSON:   template.JS(escapeScriptClose(string(snapJSON))),
		Containerfile:  template.JS(escapeScriptClose(marshalJSString(containerfile))),
		TriageManifest: template.JS("[]"),
	}

	outPath := filepath.Join(outputDir, "report.html")
	f, err := os.Create(outPath)
	if err != nil {
		return err
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
