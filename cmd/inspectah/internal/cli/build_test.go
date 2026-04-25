package cli

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestBuildCmd_Exists(t *testing.T) {
	cmd := newBuildCmd()
	assert.Equal(t, "build", cmd.Use[:5])
	assert.Contains(t, cmd.Short, "Build")
}

func TestBuildCmd_Flags(t *testing.T) {
	cmd := newBuildCmd()
	f := cmd.Flags()
	assert.NotNil(t, f.Lookup("tag"))
	assert.NotNil(t, f.Lookup("pull"))
	assert.NotNil(t, f.Lookup("dry-run"))
	assert.NotNil(t, f.Lookup("verbose"))
}

func TestBuildCmd_RequiresExactlyOneArg(t *testing.T) {
	cmd := newBuildCmd()
	err := cmd.Args(cmd, []string{})
	assert.Error(t, err)

	err = cmd.Args(cmd, []string{"dir1", "dir2"})
	assert.Error(t, err)

	err = cmd.Args(cmd, []string{"dir1"})
	assert.NoError(t, err)
}
