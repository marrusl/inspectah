package cli

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/container"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/paths"
	"github.com/spf13/cobra"
)

func newFleetCmd(opts *GlobalOpts) *cobra.Command {
	var (
		outputDir string
		dryRun    bool
		verbose   bool
	)

	cmd := &cobra.Command{
		Use:   "fleet <input-dir> [flags] [-- extra-flags...]",
		Short: "Aggregate inspections from multiple hosts into a fleet report",
		Long: `Aggregate inspectah snapshots from multiple hosts, merge by prevalence
threshold, and produce a unified Containerfile and report.

The input directory should contain inspectah tarballs or JSON snapshots.`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			runner := container.NewRealRunner()

			if err := container.EnsureImage(context.Background(), runner, opts.Image, opts.Pull, os.Stderr); err != nil {
				return err
			}

			inputDir, err := filepath.Abs(args[0])
			if err != nil {
				return fmt.Errorf("cannot resolve input directory: %w", err)
			}
			info, err := os.Stat(inputDir)
			if err != nil || !info.IsDir() {
				return fmt.Errorf("input path is not a directory: %s", inputDir)
			}
			dirName := filepath.Base(inputDir)

			outDir, err := paths.ResolveOutputDir(outputDir)
			if err != nil {
				return err
			}

			containerOutFile := fmt.Sprintf("/output/%s.tar.gz", dirName)

			runOpts := container.RunOpts{
				Image:   opts.Image,
				Workdir: "/output",
				Mounts: []container.Mount{
					{Source: inputDir, Target: "/input", Options: "ro"},
					{Source: outDir, Target: "/output"},
				},
				Command: append([]string{"fleet", "/input", "-o", containerOutFile}, args[1:]...),
			}

			podmanArgs := container.BuildArgs(runOpts)

			if dryRun || verbose {
				fmt.Fprintf(os.Stderr, "podman %s\n", strings.Join(podmanArgs, " "))
				if dryRun {
					return nil
				}
			}

			return runner.Exec(podmanArgs)
		},
	}

	cmd.Flags().StringVarP(&outputDir, "output", "o", "", "output directory (default: current directory)")
	cmd.Flags().BoolVar(&dryRun, "dry-run", false, "print the podman command without executing")
	cmd.Flags().BoolVar(&verbose, "verbose", false, "print the podman command before executing")

	return cmd
}
