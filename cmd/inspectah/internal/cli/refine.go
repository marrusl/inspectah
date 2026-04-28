package cli

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/refine"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/renderer"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
	"github.com/spf13/cobra"
)

func newRefineCmd(_ *GlobalOpts) *cobra.Command {
	var (
		port      int
		noBrowser bool
	)

	cmd := &cobra.Command{
		Use:   "refine <tarball>",
		Short: "Serve the interactive report for operator refinement",
		Long: `Serve an inspectah tarball as an interactive web UI where operators
can toggle packages, configs, and services, then re-render the
Containerfile.`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			tarball, err := filepath.Abs(args[0])
			if err != nil {
				return fmt.Errorf("cannot resolve tarball path: %w", err)
			}

			return refine.RunRefine(refine.RunRefineOptions{
				TarballPath: tarball,
				Port:        port,
				NoBrowser:   noBrowser,
				ReRenderFn:  nativeReRender,
			})
		},
	}

	cmd.Flags().IntVar(&port, "port", 8642, "port for the refine server")
	cmd.Flags().BoolVar(&noBrowser, "no-browser", false, "do not open browser automatically")

	return cmd
}

// nativeReRender re-renders the output by loading the snapshot and running
// the renderer pipeline directly — no subprocess or container needed.
func nativeReRender(snapData []byte, origData []byte, outputDir string) (refine.ReRenderResult, error) {
	var snap schema.InspectionSnapshot
	if err := json.Unmarshal(snapData, &snap); err != nil {
		return refine.ReRenderResult{}, fmt.Errorf("parse snapshot: %w", err)
	}

	// Write the snapshot to the output dir for renderers to reference
	snapPath := filepath.Join(outputDir, "inspection-snapshot.json")
	if err := os.WriteFile(snapPath, snapData, 0644); err != nil {
		return refine.ReRenderResult{}, fmt.Errorf("write snapshot: %w", err)
	}

	// Handle original snapshot for diff display
	origSnapPath := ""
	if origData != nil {
		origFile := filepath.Join(outputDir, ".original-snapshot.json")
		if err := os.WriteFile(origFile, origData, 0644); err != nil {
			return refine.ReRenderResult{}, fmt.Errorf("write original snapshot: %w", err)
		}
		origSnapPath = origFile
	}

	// Run all renderers
	if err := renderer.RunAll(&snap, outputDir, renderer.RunAllOptions{
		RefineMode:           true,
		OriginalSnapshotPath: origSnapPath,
	}); err != nil {
		return refine.ReRenderResult{}, fmt.Errorf("render: %w", err)
	}

	// Read back the results
	htmlData, err := os.ReadFile(filepath.Join(outputDir, "report.html"))
	if err != nil {
		return refine.ReRenderResult{}, fmt.Errorf("read report.html: %w", err)
	}

	containerfileData, _ := os.ReadFile(filepath.Join(outputDir, "Containerfile"))

	return refine.ReRenderResult{
		HTML:          string(htmlData),
		Snapshot:      json.RawMessage(snapData),
		Containerfile: string(containerfileData),
	}, nil
}
