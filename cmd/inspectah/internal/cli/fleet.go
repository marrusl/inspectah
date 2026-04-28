package cli

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/fleet"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/paths"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/pipeline"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/renderer"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
	"github.com/spf13/cobra"
)

func newFleetCmd(opts *GlobalOpts) *cobra.Command {
	cmd := &cobra.Command{
		Use:   "fleet <input-dir> [flags]",
		Short: "Aggregate inspections from multiple hosts into a fleet report",
		Long: `Aggregate inspectah snapshots from multiple hosts, merge by prevalence
threshold, and produce a unified Containerfile and report.

The input directory should contain inspectah tarballs or JSON snapshots.`,
		Args: cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			f := cmd.Flags()

			inputDir, err := filepath.Abs(args[0])
			if err != nil {
				return fmt.Errorf("cannot resolve input directory: %w", err)
			}
			info, err := os.Stat(inputDir)
			if err != nil || !info.IsDir() {
				return fmt.Errorf("input path is not a directory: %s", inputDir)
			}

			minPrevalence, _ := f.GetInt("min-prevalence")
			outputFile, _ := f.GetString("output-file")
			outputDir, _ := f.GetString("output-dir")
			jsonOnly, _ := f.GetBool("json-only")
			noHosts, _ := f.GetBool("no-hosts")
			_ = noHosts // reserved for future strip-host-lists support

			fmt.Fprintf(os.Stderr, "── [fleet]  Loading snapshots from %s\n", inputDir)

			// Step 1: Load snapshots
			snapshots, err := fleet.LoadSnapshots(inputDir)
			if err != nil {
				return fmt.Errorf("load snapshots: %w", err)
			}
			fmt.Fprintf(os.Stderr, "── [fleet]  Found %d snapshots\n", len(snapshots))

			// Step 2: Validate
			if err := fleet.ValidateSnapshots(snapshots); err != nil {
				return fmt.Errorf("validate: %w", err)
			}

			// Step 3: Merge
			fmt.Fprintf(os.Stderr, "── [fleet]  Merging with min-prevalence=%d%%\n", minPrevalence)
			merged, err := fleet.MergeSnapshots(snapshots, minPrevalence)
			if err != nil {
				return fmt.Errorf("merge: %w", err)
			}

			// Step 4: Redaction
			merged = pipeline.RedactSnapshot(merged)

			// JSON-only mode: write merged snapshot and exit
			if jsonOnly {
				outPath := "fleet-snapshot.json"
				if outputDir != "" {
					os.MkdirAll(outputDir, 0755)
					outPath = filepath.Join(outputDir, "fleet-snapshot.json")
				}
				data, err := json.MarshalIndent(merged, "", "  ")
				if err != nil {
					return fmt.Errorf("marshal merged snapshot: %w", err)
				}
				if err := os.WriteFile(outPath, data, 0644); err != nil {
					return fmt.Errorf("write fleet snapshot: %w", err)
				}
				fmt.Fprintf(os.Stderr, "Output: %s\n", outPath)
				return nil
			}

			// Step 5: Determine output directory
			tmpDir := outputDir
			useTmpDir := false
			if tmpDir == "" {
				var err error
				tmpDir, err = os.MkdirTemp("", "inspectah-fleet-")
				if err != nil {
					return fmt.Errorf("create temp dir: %w", err)
				}
				useTmpDir = true
				defer func() {
					if useTmpDir {
						os.RemoveAll(tmpDir)
					}
				}()
			}
			os.MkdirAll(tmpDir, 0755)

			// Step 6: Save merged snapshot
			if err := schema.SaveSnapshot(merged, filepath.Join(tmpDir, "inspection-snapshot.json")); err != nil {
				return fmt.Errorf("save snapshot: %w", err)
			}

			// Step 7: Render
			fmt.Fprintf(os.Stderr, "── [fleet]  Rendering fleet report\n")
			if err := renderer.RunAll(merged, tmpDir, renderer.RunAllOptions{}); err != nil {
				return fmt.Errorf("render: %w", err)
			}

			// Step 8: Package output
			if outputDir == "" {
				stamp := pipeline.GetOutputStamp("fleet-merged")
				tarballName := fmt.Sprintf("inspectah-%s.tar.gz", stamp)
				if outputFile != "" {
					tarballName = outputFile
				}

				outDir, err := paths.ResolveOutputDir("")
				if err != nil {
					return err
				}
				tarballPath := filepath.Join(outDir, tarballName)

				if err := pipeline.CreateTarball(tmpDir, tarballPath, "inspectah-"+stamp); err != nil {
					return fmt.Errorf("create tarball: %w", err)
				}
				fmt.Fprintf(os.Stderr, "Output: %s\n", tarballPath)
				useTmpDir = true
			} else {
				useTmpDir = false
				fmt.Fprintf(os.Stderr, "Output: %s\n", tmpDir)
			}

			return nil
		},
	}

	registerFleetPassthrough(cmd)

	return cmd
}
