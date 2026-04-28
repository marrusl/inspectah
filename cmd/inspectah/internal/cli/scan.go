package cli

import (
	"fmt"
	"os"

	ierrors "github.com/marrusl/inspectah/cmd/inspectah/internal/errors"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/pipeline"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/platform"
	"github.com/spf13/cobra"
)

func newScanCmd(opts *GlobalOpts) *cobra.Command {
	cmd := &cobra.Command{
		Use:   "scan [flags]",
		Short: "Inspect a package-mode host and produce bootc image artifacts",
		Long: `Scan the current host and generate a migration artifact bundle containing
a Containerfile, configuration tree, and interactive report.

Requires root privileges on a Linux host.`,
		DisableFlagParsing: false,
		RunE: func(cmd *cobra.Command, args []string) error {
			if err := platform.CheckScanPlatform(); err != nil {
				return ierrors.New(
					ierrors.ErrPlatformUnsupported,
					"scan requires a Linux host",
					"Use inspectah scan on a RHEL, CentOS, or Fedora system. For refine/fleet/build, any platform works.",
					err,
				)
			}

			if err := platform.CheckRoot(); err != nil {
				return ierrors.New(
					ierrors.ErrPermissionDenied,
					"scan requires root privileges",
					"Run with sudo: sudo inspectah scan",
					err,
				)
			}

			fmt.Fprintf(os.Stderr, "inspectah %s\n\n", opts.Version)

			// Read flags
			f := cmd.Flags()
			hostRoot, _ := f.GetString("host-root")
			if hostRoot == "" {
				hostRoot = "/"
			}
			fromSnapshot, _ := f.GetString("from-snapshot")
			inspectOnly, _ := f.GetBool("inspect-only")
			outputDir, _ := f.GetString("output-dir")
			noSubscription, _ := f.GetBool("no-subscription")
			sensitivity, _ := f.GetString("sensitivity")
			noRedaction, _ := f.GetBool("no-redaction")
			validate, _ := f.GetBool("validate")
			pushToGitHub, _ := f.GetString("push-to-github")
			githubToken, _ := f.GetString("github-token")
			public, _ := f.GetBool("public")
			skipConfirmation, _ := f.GetBool("yes")
			configDiffs, _ := f.GetBool("config-diffs")
			deepBinaryScan, _ := f.GetBool("deep-binary-scan")
			queryPodman, _ := f.GetBool("query-podman")
			targetVersion, _ := f.GetString("target-version")
			targetImage, _ := f.GetString("target-image")
			noBaseline, _ := f.GetBool("no-baseline")
			skipPreflight, _ := f.GetBool("skip-preflight")
			baselinePackages, _ := f.GetString("baseline-packages")
			userStrategy, _ := f.GetString("user-strategy")

			// Also check legacy -o flag
			if outputDir == "" {
				if o, _ := f.GetString("output"); o != "" {
					outputDir = o
				}
			}

			fmt.Fprintf(os.Stderr, "── [scan]   Scanning host...\n\n")

			_, err := pipeline.Run(pipeline.RunOptions{
				HostRoot:         hostRoot,
				FromSnapshotPath: fromSnapshot,
				InspectOnly:      inspectOnly,
				OutputDir:        outputDir,
				NoSubscription:   noSubscription,
				Sensitivity:      sensitivity,
				NoRedaction:      noRedaction,
				Validate:         validate,
				PushToGitHub:     pushToGitHub,
				GitHubToken:      githubToken,
				Public:           public,
				SkipConfirmation: skipConfirmation,
				ConfigDiffs:      configDiffs,
				DeepBinaryScan:   deepBinaryScan,
				QueryPodman:      queryPodman,
				TargetVersion:    targetVersion,
				TargetImage:      targetImage,
				NoBaseline:       noBaseline,
				SkipPreflight:    skipPreflight,
				BaselinePackages: baselinePackages,
				UserStrategy:     userStrategy,
			})
			return err
		},
	}

	cmd.Flags().StringP("output", "o", "", "output directory (default: current directory)")

	registerScanPassthrough(cmd)

	return cmd
}
