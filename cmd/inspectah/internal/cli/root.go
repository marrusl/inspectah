package cli

import (
	"os"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/container"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/version"
	"github.com/spf13/cobra"
)

type GlobalOpts struct {
	Image string
	Pull  string
}

func NewRootCmd(ver, commit, date string) *cobra.Command {
	opts := &GlobalOpts{}

	root := &cobra.Command{
		Use:   "inspectah",
		Short: "Inspect package-mode hosts and produce bootc image artifacts",
		Long: `inspectah inspects package-based RHEL, CentOS, and Fedora hosts
and produces bootc-compatible image artifacts including Containerfiles,
configuration trees, and migration reports.`,
		SilenceUsage:  true,
		SilenceErrors: true,
		PersistentPreRun: func(cmd *cobra.Command, args []string) {
			opts.Image = container.ResolveImage(
				opts.Image,
				os.Getenv("INSPECTAH_IMAGE"),
				container.LoadPinnedImage(),
				version.DefaultImageRef(),
			)
		},
	}

	root.PersistentFlags().StringVar(&opts.Image, "image", "", "container image to use (overrides env/config/default)")
	root.PersistentFlags().StringVar(&opts.Pull, "pull", "missing", "image pull policy: always, missing, never")

	root.AddCommand(newVersionCmd(ver, commit, date))

	return root
}
