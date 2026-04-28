package cli

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestScanCmd_Exists(t *testing.T) {
	cmd := newScanCmd(&GlobalOpts{Pull: "missing", Image: "test:latest"})
	assert.Equal(t, "scan", cmd.Use[:4])
	assert.Contains(t, cmd.Short, "Inspect")
}

func TestScanCmd_Flags(t *testing.T) {
	cmd := newScanCmd(&GlobalOpts{Pull: "missing", Image: "test:latest"})

	f := cmd.Flags()
	assert.NotNil(t, f.Lookup("output"))
	// Native pipeline flags (previously passthrough to container)
	assert.NotNil(t, f.Lookup("host-root"))
	assert.NotNil(t, f.Lookup("from-snapshot"))
	assert.NotNil(t, f.Lookup("output-dir"))
	assert.NotNil(t, f.Lookup("no-redaction"))
	assert.NotNil(t, f.Lookup("sensitivity"))
}
