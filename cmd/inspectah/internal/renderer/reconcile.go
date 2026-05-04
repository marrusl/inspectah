package renderer

import (
	"encoding/json"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// ReconcileSecretOverrides produces a derived redaction view that
// reflects user override decisions on config files containing secrets.
// The canonical snap.Redactions slice is NOT modified — the returned
// slice is a copy with adjusted Kind values.
//
// Ordering is preserved because the SPA uses index-based binding
// (secret-<n> → redactions[n]).
func ReconcileSecretOverrides(snap *schema.InspectionSnapshot) []schema.RedactionFinding {
	result := make([]schema.RedactionFinding, 0, len(snap.Redactions))

	if len(snap.Redactions) == 0 {
		return result
	}

	// Build config file path → Include lookup.
	configInclude := make(map[string]bool)
	if snap.Config != nil {
		for _, f := range snap.Config.Files {
			configInclude[f.Path] = f.Include
		}
	}

	for _, raw := range snap.Redactions {
		var finding schema.RedactionFinding
		if err := json.Unmarshal(raw, &finding); err != nil {
			// Unmarshal failure: append a zero-value finding to
			// preserve index alignment.
			result = append(result, finding)
			continue
		}

		if finding.Source == "file" {
			if include, ok := configInclude[finding.Path]; ok {
				switch {
				case finding.Kind == "excluded" && include:
					finding.Kind = "overridden"
				case finding.Kind == "inline" && !include:
					finding.Kind = "excluded"
				case finding.Kind == "flagged" && !include:
					finding.Kind = "excluded"
				}
			}
		}

		result = append(result, finding)
	}

	return result
}
