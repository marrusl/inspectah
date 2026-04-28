package cli

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestFleetCmd_Exists(t *testing.T) {
	cmd := newFleetCmd(&GlobalOpts{Pull: "missing", Image: "test:latest"})
	assert.Equal(t, "fleet", cmd.Use[:5])
	assert.Contains(t, cmd.Short, "Aggregate")
}

func TestFleetCmd_Flags(t *testing.T) {
	cmd := newFleetCmd(&GlobalOpts{Pull: "missing", Image: "test:latest"})
	f := cmd.Flags()
	assert.NotNil(t, f.Lookup("min-prevalence"))
	assert.NotNil(t, f.Lookup("output-file"))
	assert.NotNil(t, f.Lookup("output-dir"))
	assert.NotNil(t, f.Lookup("json-only"))
	assert.NotNil(t, f.Lookup("no-hosts"))
}

func TestFleetCmd_RequiresArg(t *testing.T) {
	cmd := newFleetCmd(&GlobalOpts{Pull: "missing", Image: "test:latest"})
	err := cmd.Args(cmd, []string{})
	assert.Error(t, err)
}
