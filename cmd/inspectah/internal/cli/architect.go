package cli

import (
	"fmt"
	"net/http"
	"os"
	"path/filepath"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/architect"
	"github.com/spf13/cobra"
)

func newArchitectCmd() *cobra.Command {
	var (
		port      int
		noBrowser bool
	)

	cmd := &cobra.Command{
		Use:   "architect <input-dir> [flags]",
		Short: "Launch the architect decomposition dashboard",
		Long: `Analyze fleet tarballs in a directory and serve an interactive
architect dashboard for multi-fleet layer decomposition planning.

The input directory should contain refined fleet tarballs (.tar.gz)
produced by the fleet command.`,
		Args: cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			inputDir, err := filepath.Abs(args[0])
			if err != nil {
				return fmt.Errorf("cannot resolve input path: %w", err)
			}

			info, err := os.Stat(inputDir)
			if err != nil || !info.IsDir() {
				return fmt.Errorf("input path is not a directory: %s", inputDir)
			}

			// Load fleet tarballs
			fmt.Fprintf(os.Stderr, "── [architect]  Loading fleet tarballs from %s\n", inputDir)
			fleets, err := architect.LoadRefinedFleets(inputDir)
			if err != nil {
				return fmt.Errorf("load fleets: %w", err)
			}
			if len(fleets) == 0 {
				return fmt.Errorf("no fleet tarballs found in %s", inputDir)
			}
			fmt.Fprintf(os.Stderr, "── [architect]  Found %d fleet(s)\n", len(fleets))

			// Validate same major version
			if err := architect.ValidateFleetVersions(fleets, inputDir); err != nil {
				return err
			}

			// Determine base image from first fleet
			baseImage := "registry.redhat.io/rhel9/rhel-bootc:latest"
			for _, f := range fleets {
				if f.BaseImage != "" {
					baseImage = f.BaseImage
					break
				}
			}

			// Analyze topology
			fmt.Fprintf(os.Stderr, "── [architect]  Analyzing layer topology\n")
			topo := architect.AnalyzeFleets(fleets)

			base := topo.GetLayer("base")
			if base != nil {
				fmt.Fprintf(os.Stderr, "── [architect]  Base: %d shared packages\n", len(base.Packages))
			}
			for _, layer := range topo.Layers {
				if layer.Parent != nil {
					fmt.Fprintf(os.Stderr, "── [architect]  %s: %d packages\n", layer.Name, len(layer.Packages))
				}
			}

			// Start server
			actualPort, srv, err := architect.StartServer(architect.ServerConfig{
				Topology:    topo,
				BaseImage:   baseImage,
				Port:        port,
				OpenBrowser: !noBrowser,
			})
			if err != nil {
				return fmt.Errorf("start server: %w", err)
			}

			url := fmt.Sprintf("http://127.0.0.1:%d", actualPort)
			fmt.Fprintf(os.Stderr, "── [architect]  Serving at %s\n", url)

			if !noBrowser {
				go architect.OpenBrowser(url)
			}

			fmt.Fprintln(os.Stderr, "Press Ctrl+C to stop.")
			if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
				return err
			}
			return nil
		},
	}

	cmd.Flags().IntVar(&port, "port", 8643, "port for the architect server")
	cmd.Flags().BoolVar(&noBrowser, "no-browser", false, "do not open browser automatically")

	return cmd
}
