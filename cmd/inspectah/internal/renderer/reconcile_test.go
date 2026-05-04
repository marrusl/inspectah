package renderer

import (
	"encoding/json"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// ---------------------------------------------------------------------------
// ReconcileSecretOverrides tests
// ---------------------------------------------------------------------------

func TestReconcileSecretOverrides_ExcludedToOverridden(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/myapp/db.conf", Include: true},
		},
	}
	snap.Redactions = []json.RawMessage{
		mustMarshal(schema.RedactionFinding{
			Path:   "/etc/myapp/db.conf",
			Source: "file",
			Kind:   "excluded",
		}),
	}

	got := ReconcileSecretOverrides(snap)

	require.Len(t, got, 1)
	assert.Equal(t, "overridden", got[0].Kind,
		"excluded + Include=true should become overridden")
	assert.Equal(t, "/etc/myapp/db.conf", got[0].Path)
	assert.Equal(t, "file", got[0].Source)
}

func TestReconcileSecretOverrides_InlineToExcluded(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/myapp/secrets.conf", Include: false},
		},
	}
	snap.Redactions = []json.RawMessage{
		mustMarshal(schema.RedactionFinding{
			Path:   "/etc/myapp/secrets.conf",
			Source: "file",
			Kind:   "inline",
		}),
	}

	got := ReconcileSecretOverrides(snap)

	require.Len(t, got, 1)
	assert.Equal(t, "excluded", got[0].Kind,
		"inline + Include=false should become excluded")
}

func TestReconcileSecretOverrides_FlaggedToExcluded(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/myapp/creds.conf", Include: false},
		},
	}
	snap.Redactions = []json.RawMessage{
		mustMarshal(schema.RedactionFinding{
			Path:   "/etc/myapp/creds.conf",
			Source: "file",
			Kind:   "flagged",
		}),
	}

	got := ReconcileSecretOverrides(snap)

	require.Len(t, got, 1)
	assert.Equal(t, "excluded", got[0].Kind,
		"flagged + Include=false should become excluded")
}

func TestReconcileSecretOverrides_OrderingPreserved(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/a.conf", Include: true},
			{Path: "/etc/b.conf", Include: false},
		},
	}
	snap.Redactions = []json.RawMessage{
		mustMarshal(schema.RedactionFinding{
			Path: "/etc/a.conf", Source: "file", Kind: "excluded",
		}),
		mustMarshal(schema.RedactionFinding{
			Path: "/etc/unrelated", Source: "env", Kind: "inline",
		}),
		mustMarshal(schema.RedactionFinding{
			Path: "/etc/b.conf", Source: "file", Kind: "inline",
		}),
	}

	got := ReconcileSecretOverrides(snap)

	require.Len(t, got, 3, "same length as input")
	assert.Equal(t, "/etc/a.conf", got[0].Path)
	assert.Equal(t, "overridden", got[0].Kind)
	assert.Equal(t, "/etc/unrelated", got[1].Path)
	assert.Equal(t, "inline", got[1].Kind, "non-file source unchanged")
	assert.Equal(t, "/etc/b.conf", got[2].Path)
	assert.Equal(t, "excluded", got[2].Kind)
}

func TestReconcileSecretOverrides_MultiRedactionSamePath(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/myapp/mixed.conf", Include: true},
		},
	}
	snap.Redactions = []json.RawMessage{
		mustMarshal(schema.RedactionFinding{
			Path: "/etc/myapp/mixed.conf", Source: "file", Kind: "excluded",
			Pattern: "password=",
		}),
		mustMarshal(schema.RedactionFinding{
			Path: "/etc/myapp/mixed.conf", Source: "file", Kind: "excluded",
			Pattern: "api_key=",
		}),
	}

	got := ReconcileSecretOverrides(snap)

	require.Len(t, got, 2)
	assert.Equal(t, "overridden", got[0].Kind)
	assert.Equal(t, "password=", got[0].Pattern)
	assert.Equal(t, "overridden", got[1].Kind)
	assert.Equal(t, "api_key=", got[1].Pattern)
}

func TestReconcileSecretOverrides_NonFileSourceUntouched(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/something", Include: true},
		},
	}
	snap.Redactions = []json.RawMessage{
		mustMarshal(schema.RedactionFinding{
			Path: "/etc/something", Source: "env", Kind: "excluded",
		}),
		mustMarshal(schema.RedactionFinding{
			Path: "/etc/something", Source: "cmdline", Kind: "flagged",
		}),
	}

	got := ReconcileSecretOverrides(snap)

	require.Len(t, got, 2)
	assert.Equal(t, "excluded", got[0].Kind, "env source should be untouched")
	assert.Equal(t, "flagged", got[1].Kind, "cmdline source should be untouched")
}

func TestReconcileSecretOverrides_NoMatchingConfig(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/other.conf", Include: true},
		},
	}
	snap.Redactions = []json.RawMessage{
		mustMarshal(schema.RedactionFinding{
			Path: "/etc/nomatch.conf", Source: "file", Kind: "excluded",
		}),
	}

	got := ReconcileSecretOverrides(snap)

	require.Len(t, got, 1)
	assert.Equal(t, "excluded", got[0].Kind,
		"finding with no matching config should pass through unchanged")
}

func TestReconcileSecretOverrides_NilRedactions(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Redactions = nil

	got := ReconcileSecretOverrides(snap)

	assert.Empty(t, got)
	assert.NotNil(t, got, "should return empty slice, not nil")
}

func TestReconcileSecretOverrides_EmptyRedactions(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Redactions = []json.RawMessage{}

	got := ReconcileSecretOverrides(snap)

	assert.Empty(t, got)
	assert.NotNil(t, got, "should return empty slice, not nil")
}

func TestReconcileSecretOverrides_DoesNotMutateOriginal(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/myapp/db.conf", Include: true},
		},
	}
	snap.Redactions = []json.RawMessage{
		mustMarshal(schema.RedactionFinding{
			Path: "/etc/myapp/db.conf", Source: "file", Kind: "excluded",
		}),
	}

	// Save original raw bytes for comparison
	origBytes := make([]byte, len(snap.Redactions[0]))
	copy(origBytes, snap.Redactions[0])

	got := ReconcileSecretOverrides(snap)

	assert.Equal(t, "overridden", got[0].Kind,
		"reconciled view should show overridden")

	// Verify original is unchanged
	var original schema.RedactionFinding
	require.NoError(t, json.Unmarshal(snap.Redactions[0], &original))
	assert.Equal(t, "excluded", original.Kind,
		"canonical snap.Redactions must NOT be modified")
	assert.Equal(t, origBytes, []byte(snap.Redactions[0]),
		"raw bytes in snap.Redactions must be unchanged")
}

func TestReconcileSecretOverrides_NilConfigIsGraceful(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = nil
	snap.Redactions = []json.RawMessage{
		mustMarshal(schema.RedactionFinding{
			Path: "/etc/something", Source: "file", Kind: "excluded",
		}),
	}

	got := ReconcileSecretOverrides(snap)

	require.Len(t, got, 1)
	assert.Equal(t, "excluded", got[0].Kind,
		"nil config means no overrides, pass through unchanged")
}

func TestReconcileSecretOverrides_PreservesAllFields(t *testing.T) {
	line := 42
	replacement := "***"
	confidence := "high"

	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/myapp/db.conf", Include: true},
		},
	}
	snap.Redactions = []json.RawMessage{
		mustMarshal(schema.RedactionFinding{
			Path:            "/etc/myapp/db.conf",
			Source:          "file",
			Kind:            "excluded",
			Pattern:         "password=.*",
			Remediation:     "Use vault reference",
			Line:            &line,
			Replacement:     &replacement,
			DetectionMethod: "regex",
			Confidence:      &confidence,
		}),
	}

	got := ReconcileSecretOverrides(snap)

	require.Len(t, got, 1)
	assert.Equal(t, "overridden", got[0].Kind)
	assert.Equal(t, "password=.*", got[0].Pattern)
	assert.Equal(t, "Use vault reference", got[0].Remediation)
	assert.Equal(t, 42, *got[0].Line)
	assert.Equal(t, "***", *got[0].Replacement)
	assert.Equal(t, "regex", got[0].DetectionMethod)
	assert.Equal(t, "high", *got[0].Confidence)
}
