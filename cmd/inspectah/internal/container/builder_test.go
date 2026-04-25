package container

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestBuildArgs_Scan(t *testing.T) {
	args := BuildArgs(RunOpts{
		Image:      "ghcr.io/marrusl/inspectah:latest",
		Privileged: true,
		PIDHost:    true,
		Workdir:    "/output",
		Mounts: []Mount{
			{Source: "/", Target: "/host", Options: "ro"},
			{Source: "/home/user", Target: "/output"},
		},
		Env: map[string]string{
			"INSPECTAH_HOST_CWD": "/home/user",
		},
		Command: []string{"scan"},
	})

	assert.Contains(t, args, "--privileged")
	assert.Contains(t, args, "--pid=host")
	assert.Contains(t, args, "-w")
	assert.Contains(t, args, "/output")
	assert.Contains(t, args, "-v")
	assert.Contains(t, args, "/:/host:ro")
	assert.Contains(t, args, "ghcr.io/marrusl/inspectah:latest")
	assert.Equal(t, "scan", args[len(args)-1])
}

func TestBuildArgs_Minimal(t *testing.T) {
	args := BuildArgs(RunOpts{
		Image:   "test:latest",
		Command: []string{"version"},
	})

	assert.Equal(t, "run", args[0])
	assert.Equal(t, "--rm", args[1])
	assert.NotContains(t, args, "--privileged")
	assert.NotContains(t, args, "--pid=host")
	assert.Contains(t, args, "test:latest")
	assert.Equal(t, "version", args[len(args)-1])
}
