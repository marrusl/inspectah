package renderer

import (
	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// HTMLReportOptions configures the HTML report renderer.
type HTMLReportOptions struct {
	RefineMode       bool
	OriginalSnapshot *schema.InspectionSnapshot
}

// RenderHTMLReport produces the interactive HTML report.
func RenderHTMLReport(snap *schema.InspectionSnapshot, outputDir string, opts HTMLReportOptions) error {
	// TODO: implement — Task 28
	return nil
}
