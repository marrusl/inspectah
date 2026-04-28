// Package renderer produces output artifacts from an InspectionSnapshot.
//
// The renderer package contains the following renderers:
//
//   - Containerfile renderer (code-based, no templates)
//   - HTML report renderer (html/template-based)
//   - Audit report renderer (code-based)
//   - README renderer (code-based)
//   - Kickstart renderer (code-based)
//   - Secrets review renderer (code-based)
//   - Merge notes renderer (code-based)
//
// Code-based renderers build output programmatically from snapshot data.
// Template-based renderers use Go html/template with go:embed.
package renderer

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// RunAllOptions configures the renderer pipeline.
type RunAllOptions struct {
	// RefineMode indicates this is a re-render during interactive refine.
	RefineMode bool

	// OriginalSnapshotPath is the path to the pre-edit snapshot for
	// diff display in the HTML report. May be empty.
	OriginalSnapshotPath string
}

// RunAll executes all renderers against the snapshot, writing output to
// outputDir. This mirrors the Python renderers.__init__.run_all function.
func RunAll(snap *schema.InspectionSnapshot, outputDir string, opts RunAllOptions) error {
	if err := os.MkdirAll(outputDir, 0755); err != nil {
		return fmt.Errorf("create output dir: %w", err)
	}

	// Load original snapshot for diff display if provided
	var originalSnap *schema.InspectionSnapshot
	if opts.OriginalSnapshotPath != "" {
		if s, err := schema.LoadSnapshot(opts.OriginalSnapshotPath); err == nil {
			originalSnap = s
		}
	}

	// Renderer execution order matches Python: Containerfile first
	// (writes config/ tree), then remaining renderers.
	renderers := []struct {
		name string
		fn   func() error
	}{
		{"containerfile", func() error {
			return RenderContainerfile(snap, outputDir)
		}},
		{"redacted-dir", func() error {
			return WriteRedactedDir(snap, outputDir)
		}},
		{"merge-notes", func() error {
			return RenderMergeNotes(snap, outputDir)
		}},
		{"audit-report", func() error {
			return RenderAuditReport(snap, outputDir, originalSnap)
		}},
		{"html-report", func() error {
			return RenderHTMLReport(snap, outputDir, HTMLReportOptions{
				RefineMode:       opts.RefineMode,
				OriginalSnapshot: originalSnap,
			})
		}},
		{"readme", func() error {
			return RenderReadme(snap, outputDir)
		}},
		{"kickstart", func() error {
			return RenderKickstart(snap, outputDir)
		}},
		{"secrets-review", func() error {
			return RenderSecretsReview(snap, outputDir)
		}},
	}

	for _, r := range renderers {
		if err := r.fn(); err != nil {
			return fmt.Errorf("renderer %s: %w", r.name, err)
		}
	}

	return nil
}

// writeFile is a helper that creates parent directories and writes content.
func writeFile(dir, name, content string) error {
	path := filepath.Join(dir, name)
	if err := os.MkdirAll(filepath.Dir(path), 0755); err != nil {
		return err
	}
	return os.WriteFile(path, []byte(content), 0644)
}
