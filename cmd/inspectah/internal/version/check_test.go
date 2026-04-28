package version

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestNormalizeTag_WithPrefix(t *testing.T) {
	assert.Equal(t, "0.5.1", NormalizeTag("v0.5.1"))
}

func TestNormalizeTag_WithoutPrefix(t *testing.T) {
	assert.Equal(t, "0.5.1", NormalizeTag("0.5.1"))
}

func TestNormalizeTag_Latest(t *testing.T) {
	assert.Equal(t, "latest", NormalizeTag("latest"))
}

func TestToImageRef(t *testing.T) {
	ref := ToImageRef("ghcr.io", "marrusl/inspectah", "v0.5.1")
	assert.Equal(t, "ghcr.io/marrusl/inspectah:0.5.1", ref)
}

func TestToImageRef_NoVPrefix(t *testing.T) {
	ref := ToImageRef("ghcr.io", "marrusl/inspectah", "0.5.1")
	assert.Equal(t, "ghcr.io/marrusl/inspectah:0.5.1", ref)
}

func TestDefaultImageRef(t *testing.T) {
	ref := DefaultImageRef()
	assert.Equal(t, "ghcr.io/marrusl/inspectah:0.6.0", ref)
}
