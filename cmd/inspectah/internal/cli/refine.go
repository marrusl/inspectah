package cli

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/container"
	"github.com/spf13/cobra"
)

func newRefineCmd(opts *GlobalOpts) *cobra.Command {
	var (
		port      int
		noBrowser bool
		dryRun    bool
		verbose   bool
	)

	cmd := &cobra.Command{
		Use:   "refine <tarball> [flags] [-- extra-flags...]",
		Short: "Serve the interactive report for operator refinement",
		Long: `Serve an inspectah tarball as an interactive web UI where operators
can toggle packages, configs, and services, then re-render the
Containerfile.`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			runner := container.NewRealRunner()

			if err := container.EnsureImage(context.Background(), runner, opts.Image, opts.Pull, os.Stderr); err != nil {
				return err
			}

			tarball, err := filepath.Abs(args[0])
			if err != nil {
				return fmt.Errorf("cannot resolve tarball path: %w", err)
			}
			if _, err := os.Stat(tarball); err != nil {
				return fmt.Errorf("tarball not found: %s", tarball)
			}
			tarballName := filepath.Base(tarball)

			portStr := fmt.Sprintf("%d:%d", port, port)
			containerInput := fmt.Sprintf("/input/%s", tarballName)

			runOpts := container.RunOpts{
				Image: opts.Image,
				Ports: []string{portStr},
				Mounts: []container.Mount{
					{Source: tarball, Target: containerInput, Options: "ro"},
				},
				Command: append([]string{"refine", "--port", fmt.Sprintf("%d", port), "--bind", "0.0.0.0", "--no-browser", containerInput}, args[1:]...),
			}

			podmanArgs := container.BuildArgs(runOpts)

			if dryRun || verbose {
				fmt.Fprintf(os.Stderr, "podman %s\n", strings.Join(podmanArgs, " "))
				if dryRun {
					return nil
				}
			}

			url := fmt.Sprintf("http://localhost:%d", port)
			fmt.Fprintf(os.Stderr, "Starting refine server at %s\n", url)

			if !noBrowser {
				go func() {
					if container.WaitForServer(url+"/api/health", 30*time.Second) {
						// Cache-bust: browsers may cache a failed/empty response
						// from the port before the container server was ready.
						browserURL := fmt.Sprintf("%s?t=%d", url, time.Now().Unix())
						container.OpenBrowser(browserURL)
					}
				}()
			}

			return container.RunChild(context.Background(), runner.PodmanPath, podmanArgs)
		},
	}

	cmd.Flags().IntVar(&port, "port", 8642, "port for the refine server")
	cmd.Flags().BoolVar(&noBrowser, "no-browser", false, "do not open browser automatically")
	cmd.Flags().BoolVar(&dryRun, "dry-run", false, "print the podman command without executing")
	cmd.Flags().BoolVar(&verbose, "verbose", false, "print the podman command before executing")

	return cmd
}
