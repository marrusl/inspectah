package container

import (
	"context"
	"io"
	"os/exec"
)

type PodmanRunner interface {
	Run(ctx context.Context, args []string, stdout, stderr io.Writer) (int, error)
	Exec(args []string) error
}

type RealRunner struct {
	PodmanPath string
}

func NewRealRunner() *RealRunner {
	path, err := exec.LookPath("podman")
	if err != nil {
		path = "podman"
	}
	return &RealRunner{PodmanPath: path}
}

func (r *RealRunner) Run(ctx context.Context, args []string, stdout, stderr io.Writer) (int, error) {
	cmd := exec.CommandContext(ctx, r.PodmanPath, args...)
	cmd.Stdout = stdout
	cmd.Stderr = stderr
	err := cmd.Run()
	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			return exitErr.ExitCode(), nil
		}
		return -1, err
	}
	return 0, nil
}

func (r *RealRunner) Exec(args []string) error {
	return execPodman(r.PodmanPath, args)
}
