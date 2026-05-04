package renderer

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// RenderSecretsReview produces secrets-review.md listing all redaction
// findings and recommended actions.
func RenderSecretsReview(snap *schema.InspectionSnapshot, outputDir string) error {
	path := filepath.Join(outputDir, "secrets-review.md")

	if len(snap.Redactions) == 0 {
		return os.WriteFile(path, []byte("# Secrets Review\n\nNo redactions recorded.\n"), 0644)
	}

	var lines []string
	lines = append(lines, "# Secrets Review")
	lines = append(lines, "")

	// Parse findings
	var excluded, inlineRedacted, flagged, overridden []schema.RedactionFinding
	var legacy []map[string]interface{}

	for _, raw := range snap.Redactions {
		var finding schema.RedactionFinding
		if err := json.Unmarshal(raw, &finding); err == nil && finding.Source != "" {
			switch finding.Kind {
			case "excluded":
				excluded = append(excluded, finding)
			case "inline":
				inlineRedacted = append(inlineRedacted, finding)
			case "flagged":
				flagged = append(flagged, finding)
			case "overridden":
				overridden = append(overridden, finding)
			}
		} else {
			var m map[string]interface{}
			if err := json.Unmarshal(raw, &m); err == nil {
				legacy = append(legacy, m)
			}
		}
	}

	nRedacted := len(excluded) + len(inlineRedacted)
	var parts []string
	if len(excluded) > 0 {
		parts = append(parts, fmt.Sprintf("%d excluded", len(excluded)))
	}
	if len(inlineRedacted) > 0 {
		parts = append(parts, fmt.Sprintf("%d inline", len(inlineRedacted)))
	}
	breakdown := ""
	if len(parts) > 0 {
		breakdown = " (" + strings.Join(parts, ", ") + ")"
	}
	flaggedPart := ""
	if len(flagged) > 0 {
		flaggedPart = fmt.Sprintf(", %d flagged for review", len(flagged))
	}
	overriddenPart := ""
	if len(overridden) > 0 {
		overriddenPart = fmt.Sprintf(", %d overridden", len(overridden))
	}

	lines = append(lines, fmt.Sprintf("> Detected secrets: %d redacted%s%s%s", nRedacted, breakdown, flaggedPart, overriddenPart))
	lines = append(lines, "")

	lines = append(lines, "The following items were redacted or excluded. Handle them according to")
	lines = append(lines, "the action specified for each item.")
	lines = append(lines, "")

	// Excluded files
	if len(excluded) > 0 {
		lines = append(lines, "## Excluded Files")
		lines = append(lines, "")
		lines = append(lines, "These files were removed from the output entirely.")
		lines = append(lines, "")
		lines = append(lines, "| Path | Pattern | Remediation |")
		lines = append(lines, "|------|---------|-------------|")
		for _, f := range excluded {
			rem := remediationLabel(f.Remediation)
			lines = append(lines, fmt.Sprintf("| %s | %s | %s |", f.Path, f.Pattern, rem))
		}
		lines = append(lines, "")
	}

	// Inline redactions
	if len(inlineRedacted) > 0 {
		lines = append(lines, "## Inline Redactions")
		lines = append(lines, "")
		lines = append(lines, "Secret values in these files/entries were replaced with `[REDACTED-*]` tokens.")
		lines = append(lines, "")
		lines = append(lines, "| Path | Line | Pattern | Detection |")
		lines = append(lines, "|------|------|---------|-----------|")
		for _, f := range inlineRedacted {
			lineStr := "--"
			if f.Line != nil {
				lineStr = fmt.Sprintf("%d", *f.Line)
			}
			detection := detectionLabel(f)
			lines = append(lines, fmt.Sprintf("| %s | %s | %s | %s |", f.Path, lineStr, f.Pattern, detection))
		}
		lines = append(lines, "")
	}

	// Flagged for review
	if len(flagged) > 0 {
		lines = append(lines, "## Flagged for Review")
		lines = append(lines, "")
		lines = append(lines, "| Path | Line | Confidence | Why Flagged |")
		lines = append(lines, "|------|------|------------|-------------|")
		for _, f := range flagged {
			lineStr := "--"
			if f.Line != nil {
				lineStr = fmt.Sprintf("%d", *f.Line)
			}
			conf := "--"
			if f.Confidence != nil {
				conf = *f.Confidence
			}
			why := f.Pattern
			if why == "" {
				why = "--"
			}
			lines = append(lines, fmt.Sprintf("| %s | %s | %s | %s |", f.Path, lineStr, conf, why))
		}
		lines = append(lines, "")
	}

	// Legacy dict entries
	if len(legacy) > 0 {
		lines = append(lines, "## Other Redactions")
		lines = append(lines, "")
		lines = append(lines, "| Path | Pattern | Line | Remediation |")
		lines = append(lines, "|------|---------|------|-------------|")
		for _, r := range legacy {
			rpath := fmt.Sprintf("%v", r["path"])
			pattern := fmt.Sprintf("%v", r["pattern"])
			line := fmt.Sprintf("%v", r["line"])
			rem := fmt.Sprintf("%v", r["remediation"])
			lines = append(lines, fmt.Sprintf("| %s | %s | %s | %s |",
				strings.ReplaceAll(rpath, "|", "\\|"),
				strings.ReplaceAll(pattern, "|", "\\|"),
				strings.ReplaceAll(line, "|", "\\|"),
				strings.ReplaceAll(rem, "|", "\\|")))
		}
		lines = append(lines, "")
	}

	// Overridden exclusions
	if len(overridden) > 0 {
		lines = append(lines, "## Overridden Exclusions")
		lines = append(lines, "")
		lines = append(lines, "These files were originally excluded by the scanner but deliberately")
		lines = append(lines, "re-included by the operator during triage.")
		lines = append(lines, "")
		for _, f := range overridden {
			lines = append(lines, fmt.Sprintf("- **%s** — %s (originally excluded for: %s)", f.Path, f.Pattern, f.DetectionMethod))
		}
		lines = append(lines, "")
	}

	content := strings.Join(lines, "\n")
	return os.WriteFile(path, []byte(content), 0644)
}

// CountActiveRedactions returns the number of redaction findings that
// represent actual redactions (excluded + inline), excluding overridden
// findings which are audit-trail-only and should not inflate counts.
func CountActiveRedactions(redactions []json.RawMessage) int {
	count := 0
	for _, raw := range redactions {
		var finding schema.RedactionFinding
		if err := json.Unmarshal(raw, &finding); err == nil {
			if finding.Kind == "overridden" {
				continue
			}
		}
		count++
	}
	return count
}

func remediationLabel(rem string) string {
	switch rem {
	case "regenerate":
		return "Regenerate on target"
	case "provision":
		return "Provision from secret store"
	case "value-removed":
		return "Value removed inline"
	default:
		return rem
	}
}

func detectionLabel(f schema.RedactionFinding) string {
	method := f.DetectionMethod
	if method == "" {
		method = "pattern"
	}
	if method == "pattern" {
		return "pattern"
	}
	if method == "heuristic" {
		conf := "unknown"
		if f.Confidence != nil {
			conf = *f.Confidence
		}
		return fmt.Sprintf("heuristic (%s)", conf)
	}
	return method
}
