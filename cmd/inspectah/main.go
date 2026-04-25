package main

import (
	"fmt"
	"os"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/cli"
)

var (
	version = "dev"
	commit  = "unknown"
	date    = "unknown"
)

func main() {
	root := cli.NewRootCmd(version, commit, date)
	if err := root.Execute(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
