package cli

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"os/signal"
	"strings"
	"syscall"

	build "github.com/marrusl/inspectah/cmd/inspectah/internal/build"
	"github.com/spf13/cobra"
)

func newBuildCmd() *cobra.Command {
	var (
		tag             string
		platform        string
		entitlementsDir string
		noEntitlements  bool
		ignoreExpired   bool
		noCache         bool
		pull            string
		dryRun          bool
		verbose         bool
		strictArch      bool
	)

	cmd := &cobra.Command{
		Use:   "build <tarball|directory> -t <image:tag> [flags] [-- extra-podman-args...]",
		Short: "Build a bootc image from inspectah output",
		Long: `Build a bootc container image from an inspectah scan/refine tarball
or extracted directory.

Runs podman build natively on the workstation. Handles RHEL entitlement
cert detection, validation, and injection automatically.

Extra arguments after -- are passed directly to podman build
(e.g., --build-arg, --secret, --squash).`,
		Args: cobra.ArbitraryArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			if len(args) == 0 {
				return fmt.Errorf("requires a tarball or directory argument\n\nUsage: inspectah build <tarball|directory> -t <image:tag>")
			}

			if tag == "" {
				return fmt.Errorf("--tag (-t) is required")
			}

			if noEntitlements && entitlementsDir != "" {
				return fmt.Errorf("--no-entitlements and --entitlements-dir are mutually exclusive")
			}

			podmanPath, err := exec.LookPath("podman")
			if err != nil {
				return fmt.Errorf(build.FormatMissingPodman())
			}

			input, cleanup, err := build.ResolveInput(args[0])
			if err != nil {
				return err
			}
			defer cleanup()

			ctx, cancel := context.WithCancel(cmd.Context())
			defer cancel()
			var podProcess *os.Process
			sigCh := make(chan os.Signal, 1)
			signal.Notify(sigCh, syscall.SIGINT, syscall.SIGTERM)
			go func() {
				<-sigCh
				cancel()
				if podProcess != nil {
					podProcess.Wait()
				}
				cleanup()
				os.Exit(1)
			}()

			cfPath := input.Dir + "/Containerfile"
			cfData, err := os.ReadFile(cfPath)
			if err != nil {
				return fmt.Errorf("cannot read Containerfile: %w", err)
			}

			envDir := os.Getenv("INSPECTAH_ENTITLEMENT_DIR")
			if entitlementsDir != "" {
				if err := build.ValidateExplicitDir(entitlementsDir); err != nil {
					return err
				}
			}
			if envDir != "" && entitlementsDir == "" {
				if err := build.ValidateExplicitDir(envDir); err != nil {
					return err
				}
			}

			detection := build.ClassifyBuild(string(cfData))

			if noEntitlements {
				detection = build.DetectionNonEntitled
			}

			var certs *build.DiscoverResult
			if detection == build.DetectionNonEntitled {
				certs = &build.DiscoverResult{Status: build.DiscoveryNoCerts}
			} else {
				var err error
				certs, err = build.DiscoverCerts(build.DiscoverOpts{
					EntitlementsDir:  entitlementsDir,
					EnvDir:           envDir,
					OutputDir:        input.Dir,
					SkipEntitlements: false,
				})
				if err != nil {
					return err
				}
			}

			if certs.Status == build.DiscoveryCertsFound {
				if err := build.ValidateCertExpiry(certs.EntitlementDir, ignoreExpired); err != nil {
					return err
				}
				if warning := build.CheckMacOSPath(certs.EntitlementDir); warning != "" {
					fmt.Fprintln(os.Stderr, warning)
				}
			}

			if certs.Status == build.DiscoveryNoCerts && detection != build.DetectionNonEntitled {
				fmt.Fprintln(os.Stderr, "Warning: RHEL entitlement certs not found. Build may fail if subscribed repos are needed.")
				fmt.Fprintln(os.Stderr, "  Copy from RHEL host:  scp root@rhel-host:/etc/pki/entitlement/*.pem ./entitlement/")
				fmt.Fprintln(os.Stderr, "  Silence this warning: inspectah build --no-entitlements ...")
			}

			// Cross-arch handling: check QEMU readiness and substitute
			// arch-specific packages when building for a different platform.
			var crossArchCleanup func()
			if platform != "" {
				warnings, err := build.CrossArchCheck(platform)
				if err != nil {
					return err
				}
				for _, w := range warnings {
					fmt.Fprintln(os.Stderr, w)
				}

				// Extract the target Go arch from --platform.
				parts := strings.SplitN(platform, "/", 2)
				targetGoArch := parts[1]

				// Infer source arch from the Containerfile content.
				sourceRPM := build.InferSourceArch(string(cfData))
				if sourceRPM != "" {
					sourceGoArch := ""
					switch sourceRPM {
					case "aarch64":
						sourceGoArch = "arm64"
					case "x86_64":
						sourceGoArch = "amd64"
					case "s390x":
						sourceGoArch = "s390x"
					case "ppc64le":
						sourceGoArch = "ppc64le"
					}

					if sourceGoArch != "" && sourceGoArch != targetGoArch {
						result, err := build.CrossArchSubstitute(string(cfData), sourceGoArch, targetGoArch, strictArch)
						if err != nil {
							return err
						}
						if len(result.Substitutions) > 0 {
							for _, s := range result.Substitutions {
								fmt.Fprintf(os.Stderr, "Note: substituted %s → %s (cross-arch)\n", s.From, s.To)
							}
							// Write the modified Containerfile to a temp file
							// in the same directory so build context is preserved.
							tmpPath, tmpCleanup, err := build.WriteTempContainerfile(input.Dir, result.ModifiedContent)
							if err != nil {
								return err
							}
							crossArchCleanup = tmpCleanup
							cfPath = tmpPath
						}
					}
				}
			}
			if crossArchCleanup != nil {
				defer crossArchCleanup()
			}

			podmanArgs := []string{"build", "-f", cfPath, "-t", tag}
			if platform != "" {
				podmanArgs = append(podmanArgs, "--platform="+platform)
			}
			if noCache {
				podmanArgs = append(podmanArgs, "--no-cache")
			}
			if pull != "" {
				podmanArgs = append(podmanArgs, "--pull="+pull)
			}

			if certs.Status == build.DiscoveryCertsFound {
				podmanArgs = append(podmanArgs, "-v", certs.EntitlementDir+":/etc/pki/entitlement:ro")
				if certs.RHSMDir != "" {
					podmanArgs = append(podmanArgs, "-v", certs.RHSMDir+":/etc/rhsm:ro")
				}
			}

			podmanArgs = append(podmanArgs, args[1:]...)
			podmanArgs = append(podmanArgs, input.Dir)

			if dryRun || verbose {
				fmt.Fprintf(os.Stderr, "podman %s\n", strings.Join(podmanArgs, " "))
				if dryRun {
					return nil
				}
			}

			fmt.Fprintf(os.Stderr, "Building image from %s\n", input.Dir)

			podCmd := exec.CommandContext(ctx, podmanPath, podmanArgs...)
			podCmd.Stdout = os.Stdout
			podCmd.Stderr = os.Stderr

			if err := podCmd.Start(); err != nil {
				return fmt.Errorf("podman build failed to start: %w", err)
			}
			podProcess = podCmd.Process

			if err := podCmd.Wait(); err != nil {
				if exitErr, ok := err.(*exec.ExitError); ok {
					return fmt.Errorf("podman build exited with code %d", exitErr.ExitCode())
				}
				return fmt.Errorf("podman build failed: %w", err)
			}

			fmt.Fprintln(os.Stderr)
			fmt.Fprintln(os.Stderr, build.FormatSuccess(tag))
			return nil
		},
	}

	cmd.Flags().StringVarP(&tag, "tag", "t", "", "image name:tag (required)")
	cmd.Flags().StringVar(&platform, "platform", "", "target os/arch (e.g., linux/arm64)")
	cmd.Flags().StringVar(&entitlementsDir, "entitlements-dir", "", "explicit entitlement cert directory")
	cmd.Flags().BoolVar(&noEntitlements, "no-entitlements", false, "skip entitlement detection entirely")
	cmd.Flags().BoolVar(&ignoreExpired, "ignore-expired-certs", false, "proceed despite expired entitlement certs")
	cmd.Flags().BoolVar(&noCache, "no-cache", false, "do not use cache when building")
	cmd.Flags().StringVar(&pull, "pull", "", "base image pull policy (always, missing, never, newer)")
	cmd.Flags().BoolVar(&dryRun, "dry-run", false, "print the podman command without executing")
	cmd.Flags().BoolVar(&verbose, "verbose", false, "print the podman command before executing")
	cmd.Flags().BoolVar(&strictArch, "strict-arch", false, "error on arch-specific packages instead of auto-substituting")

	return cmd
}
