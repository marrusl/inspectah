package cli

import (
	"errors"

	ierrors "github.com/marrusl/inspectah/cmd/inspectah/internal/errors"
	"github.com/spf13/cobra"
)

func AsWrapperError(err error) (*ierrors.WrapperError, bool) {
	var werr *ierrors.WrapperError
	if errors.As(err, &werr) {
		return werr, true
	}
	return nil, false
}

// GlobalOpts holds options available to all subcommands.
type GlobalOpts struct {
	Version string
}

func NewRootCmd(ver, commit, date string) *cobra.Command {
	opts := &GlobalOpts{Version: ver}

	root := &cobra.Command{
		Use:   "inspectah",
		Short: "Inspect package-mode hosts and produce bootc image artifacts",
		Long: `inspectah inspects package-based RHEL, CentOS, and Fedora hosts
and produces bootc-compatible image artifacts including Containerfiles,
configuration trees, and migration reports.`,
		SilenceUsage:  true,
		SilenceErrors: true,
	}

	root.AddCommand(newVersionCmd(ver, commit, date))
	root.AddCommand(newScanCmd(opts))
	root.AddCommand(newFleetCmd(opts))
	root.AddCommand(newRefineCmd(opts))
	root.AddCommand(newArchitectCmd())
	root.AddCommand(newBuildCmd())

	return root
}
