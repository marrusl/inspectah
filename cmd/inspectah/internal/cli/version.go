package cli

import (
	"fmt"

	"github.com/spf13/cobra"
)

func newVersionCmd(version, commit, date string) *cobra.Command {
	return &cobra.Command{
		Use:   "version",
		Short: "Print wrapper version information",
		Run: func(cmd *cobra.Command, args []string) {
			fmt.Printf("inspectah wrapper %s\n", version)
			fmt.Printf("  commit: %s\n", commit)
			fmt.Printf("  built:  %s\n", date)
		},
	}
}
