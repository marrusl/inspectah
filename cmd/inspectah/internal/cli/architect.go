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

func newArchitectCmd(opts *GlobalOpts) *cobra.Command {
	var (
		port      int
		noBrowser bool
		dryRun    bool
		verbose   bool
	)

	cmd := &cobra.Command{
		Use:   "architect <tarball-or-dir> [flags] [-- extra-flags...]",
		Short: "Launch the architect decomposition dashboard",
		Long: `Serve an inspectah tarball or snapshot directory as an interactive
architect dashboard for multi-artifact decomposition planning.`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			runner := container.NewRealRunner()

			if err := container.EnsureImage(context.Background(), runner, opts.Image, opts.Pull, os.Stderr); err != nil {
				return err
			}

			inputPath, err := filepath.Abs(args[0])
			if err != nil {
				return fmt.Errorf("cannot resolve input path: %w", err)
			}

			var mountSpec container.Mount
			var cmdArg string

			fi, err := os.Stat(inputPath)
			if err != nil {
				return fmt.Errorf("input path not found: %s", inputPath)
			}

			if fi.IsDir() {
				mountSpec = container.Mount{Source: inputPath, Target: "/input", Options: "ro"}
				cmdArg = "/input"
			} else {
				inputName := filepath.Base(inputPath)
				mountSpec = container.Mount{Source: inputPath, Target: "/input/" + inputName, Options: "ro"}
				cmdArg = "/input/" + inputName
			}

			portStr := fmt.Sprintf("%d:%d", port, port)

			runOpts := container.RunOpts{
				Image:   opts.Image,
				Ports:   []string{portStr},
				Mounts:  []container.Mount{mountSpec},
				Command: append([]string{"architect", "--port", fmt.Sprintf("%d", port), "--bind", "0.0.0.0", "--no-browser", cmdArg}, args[1:]...),
			}

			podmanArgs := container.BuildArgs(runOpts)

			if dryRun || verbose {
				fmt.Fprintf(os.Stderr, "podman %s\n", strings.Join(podmanArgs, " "))
				if dryRun {
					return nil
				}
			}

			url := fmt.Sprintf("http://localhost:%d", port)
			fmt.Fprintf(os.Stderr, "Starting architect dashboard at %s\n", url)

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

	cmd.Flags().IntVar(&port, "port", 8643, "port for the architect server")
	cmd.Flags().BoolVar(&noBrowser, "no-browser", false, "do not open browser automatically")
	cmd.Flags().BoolVar(&dryRun, "dry-run", false, "print the podman command without executing")
	cmd.Flags().BoolVar(&verbose, "verbose", false, "print the podman command before executing")

	return cmd
}
