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
	assert.NotNil(t, f.Lookup("platform"))
	assert.NotNil(t, f.Lookup("entitlements-dir"))
	assert.NotNil(t, f.Lookup("no-entitlements"))
	assert.NotNil(t, f.Lookup("ignore-expired-certs"))
	assert.NotNil(t, f.Lookup("no-cache"))
	assert.NotNil(t, f.Lookup("pull"))
	assert.NotNil(t, f.Lookup("dry-run"))
	assert.NotNil(t, f.Lookup("verbose"))
}

func TestBuildCmd_RequiresInput(t *testing.T) {
	cmd := newBuildCmd()
	cmd.SetArgs([]string{})
	err := cmd.Execute()
	assert.Error(t, err)
	assert.ErrorContains(t, err, "requires")
}

func TestBuildCmd_AcceptsExtraArgs(t *testing.T) {
	cmd := newBuildCmd()
	err := cmd.Args(cmd, []string{"test.tar.gz"})
	assert.NoError(t, err)

	err = cmd.Args(cmd, []string{"test.tar.gz", "--build-arg", "FOO=bar"})
	assert.NoError(t, err)
}

func TestBuildCmd_TagShorthand(t *testing.T) {
	cmd := newBuildCmd()
	f := cmd.Flags().Lookup("tag")
	assert.Equal(t, "t", f.Shorthand)
}

func TestBuildCmd_MutualExclusion(t *testing.T) {
	cmd := newBuildCmd()
	cmd.SetArgs([]string{"test.tar.gz", "-t", "img:latest",
		"--no-entitlements", "--entitlements-dir", "/some/path"})
	err := cmd.Execute()
	assert.ErrorContains(t, err, "mutually exclusive")
}
