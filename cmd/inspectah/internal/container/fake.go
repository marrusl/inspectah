package container

import (
	"context"
	"fmt"
	"io"
)

type FakeRunner struct {
	RunFunc  func(ctx context.Context, args []string, stdout, stderr io.Writer) (int, error)
	ExecFunc func(args []string) error
	Calls    [][]string
}

func (f *FakeRunner) Run(ctx context.Context, args []string, stdout, stderr io.Writer) (int, error) {
	f.Calls = append(f.Calls, args)
	if f.RunFunc != nil {
		return f.RunFunc(ctx, args, stdout, stderr)
	}
	return 0, nil
}

func (f *FakeRunner) Exec(args []string) error {
	f.Calls = append(f.Calls, args)
	if f.ExecFunc != nil {
		return f.ExecFunc(args)
	}
	return fmt.Errorf("FakeRunner.Exec called but not expected in unit tests")
}
