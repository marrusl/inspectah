package cli

import (
	"context"
	"fmt"
	"os"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/container"
	ierrors "github.com/marrusl/inspectah/cmd/inspectah/internal/errors"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/paths"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/platform"
	"github.com/spf13/cobra"
)

func newScanCmd(opts *GlobalOpts) *cobra.Command {
	var (
		outputDir string
		dryRun    bool
		verbose   bool
		hostname  string
	)

	cmd := &cobra.Command{
		Use:   "scan [flags] [-- extra-flags...]",
		Short: "Inspect a package-mode host and produce bootc image artifacts",
		Long: `Scan the current host and generate a migration artifact bundle containing
a Containerfile, configuration tree, and interactive report.

Requires root privileges on a Linux host. Extra flags after -- are
passed through to the inspectah container.`,
		DisableFlagParsing: false,
		Args:               cobra.ArbitraryArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			if err := platform.CheckScanPlatform(); err != nil {
				return ierrors.New(
					ierrors.ErrPlatformUnsupported,
					"scan requires a Linux host",
					"Use inspectah scan on a RHEL, CentOS, or Fedora system. For refine/fleet/build, any platform works.",
					err,
				)
			}

			fmt.Fprintf(os.Stderr, "inspectah %s\n\n", opts.Version)

			runner := container.NewRealRunner()

			if err := container.EnsureImage(context.Background(), runner, opts.Image, opts.Pull, os.Stderr); err != nil {
				return err
			}

			outDir, err := paths.ResolveOutputDir(outputDir)
			if err != nil {
				return err
			}

			hn := hostname
			if hn == "" {
				hn, _ = os.Hostname()
			}

			env := map[string]string{
				"INSPECTAH_HOST_CWD": outDir,
				"INSPECTAH_HOSTNAME": hn,
			}
			if os.Getenv("INSPECTAH_DEBUG") != "" {
				env["INSPECTAH_DEBUG"] = "1"
			}

			runOpts := container.RunOpts{
				Image:      opts.Image,
				Privileged: true,
				PIDHost:    true,
				Workdir:    "/output",
				Mounts: []container.Mount{
					{Source: "/", Target: "/host", Options: "ro"},
					{Source: outDir, Target: "/output"},
				},
				Env:     env,
				Command: append([]string{"scan"}, args...),
			}

			podmanArgs := container.BuildArgs(runOpts)

			if dryRun {
				fmt.Fprintf(os.Stderr, "── [dry-run] Would execute:\n\n")
				fmt.Fprintf(os.Stderr, "podman %s\n", formatPodmanCmd(podmanArgs))
				fmt.Fprintf(os.Stderr, "\nNo changes made.\n")
				return nil
			}

			if verbose {
				fmt.Fprintf(os.Stderr, "podman %s\n", strings.Join(podmanArgs, " "))
			}

			fmt.Fprintf(os.Stderr, "── [scan]   Scanning host...\n\n")

			return runner.Exec(podmanArgs)
		},
	}

	cmd.Flags().StringVarP(&outputDir, "output", "o", "", "output directory (default: current directory)")
	cmd.Flags().BoolVar(&dryRun, "dry-run", false, "print the podman command without executing")
	cmd.Flags().BoolVar(&verbose, "verbose", false, "print the podman command before executing")
	cmd.Flags().StringVar(&hostname, "hostname", "", "override hostname for the inspection")

	registerScanPassthrough(cmd)

	return cmd
}

func formatPodmanCmd(args []string) string {
	if len(args) <= 1 {
		return strings.Join(args, " ")
	}
	var b strings.Builder
	b.WriteString(args[0])
	for _, arg := range args[1:] {
		if strings.HasPrefix(arg, "-") {
			b.WriteString(" \\\n  ")
		} else {
			b.WriteString(" ")
		}
		b.WriteString(arg)
	}
	return b.String()
}
