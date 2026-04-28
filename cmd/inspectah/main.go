package main

import (
	"fmt"
	"os"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/cli"
	ierrors "github.com/marrusl/inspectah/cmd/inspectah/internal/errors"
)

var (
	version = "0.7.0"
	commit  = "unknown"
	date    = "unknown"
)

func main() {
	root := cli.NewRootCmd(version, commit, date)
	if err := root.Execute(); err != nil {
		if werr, ok := cli.AsWrapperError(err); ok {
			ierrors.Render(os.Stderr, werr)
		} else {
			fmt.Fprintln(os.Stderr, err)
		}
		os.Exit(1)
	}
}
