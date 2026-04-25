package cli

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/container"
	"github.com/spf13/cobra"
)

func newBuildCmd() *cobra.Command {
	var (
		tag     string
		pull    string
		dryRun  bool
		verbose bool
	)

	cmd := &cobra.Command{
		Use:   "build <output-dir> [flags] [-- extra-podman-args...]",
		Short: "Build a bootc image from inspectah output",
		Long: `Build a bootc container image from an inspectah output directory
containing a Containerfile and config tree.

This runs podman build on the host — it does not use the inspectah
container image. The --pull flag here controls base image pulls
during the build, not the inspectah scanner image.

Extra arguments after -- are passed directly to podman build
(e.g., --build-arg, --secret, --squash).`,
		Args: cobra.ArbitraryArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			if len(args) == 0 {
				return fmt.Errorf("requires an output directory argument")
			}

			outputDir, err := filepath.Abs(args[0])
			if err != nil {
				return fmt.Errorf("cannot resolve output directory: %w", err)
			}

			containerfile := filepath.Join(outputDir, "Containerfile")
			if _, err := os.Stat(containerfile); err != nil {
				return fmt.Errorf("no Containerfile found in %s — run inspectah scan first", outputDir)
			}

			runner := container.NewRealRunner()

			podmanArgs := []string{"build"}
			if tag != "" {
				podmanArgs = append(podmanArgs, "-t", tag)
			}
			if pull != "" {
				podmanArgs = append(podmanArgs, "--pull="+pull)
			}
			podmanArgs = append(podmanArgs, args[1:]...)
			podmanArgs = append(podmanArgs, "-f", containerfile, outputDir)

			if dryRun || verbose {
				fmt.Fprintf(os.Stderr, "podman %s\n", strings.Join(podmanArgs, " "))
				if dryRun {
					return nil
				}
			}

			fmt.Fprintf(os.Stderr, "Building image from %s\n", outputDir)

			code, err := runner.Run(cmd.Context(), podmanArgs, os.Stdout, os.Stderr)
			if err != nil {
				return fmt.Errorf("podman build failed: %w", err)
			}
			if code != 0 {
				return fmt.Errorf("podman build exited with code %d", code)
			}

			fmt.Fprintf(os.Stderr, "Build complete.\n")
			return nil
		},
	}

	cmd.Flags().StringVarP(&tag, "tag", "t", "", "tag for the built image (e.g., my-migration:latest)")
	cmd.Flags().StringVar(&pull, "pull", "", "base image pull policy for podman build (always, missing, never)")
	cmd.Flags().BoolVar(&dryRun, "dry-run", false, "print the podman command without executing")
	cmd.Flags().BoolVar(&verbose, "verbose", false, "print the podman command before executing")

	return cmd
}
