package cli

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestImageCmd_Subcommands(t *testing.T) {
	cmd := newImageCmd(&GlobalOpts{Pull: "missing", Image: "test:latest"})
	assert.Equal(t, "image", cmd.Use)

	names := make([]string, 0)
	for _, sub := range cmd.Commands() {
		names = append(names, sub.Name())
	}
	assert.Contains(t, names, "info")
	assert.Contains(t, names, "update")
	assert.Contains(t, names, "use")
}
