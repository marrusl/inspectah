package renderer

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// RenderAuditReport produces audit-report.md summarizing changes, risks,
// and recommendations. This is a code-based renderer.
func RenderAuditReport(snap *schema.InspectionSnapshot, outputDir string, originalSnap *schema.InspectionSnapshot) error {
	var lines []string

	lines = append(lines, "# Audit Report")
	lines = append(lines, "")

	// OS info
	if snap.OsRelease != nil {
		name := snap.OsRelease.PrettyName
		if name == "" {
			name = snap.OsRelease.Name
		}
		lines = append(lines, fmt.Sprintf("**Source system:** %s", name))
		lines = append(lines, "")
	}

	// Packages
	if snap.Rpm != nil {
		lines = append(lines, "## Packages")
		lines = append(lines, "")

		// Added packages
		var included int
		for _, p := range snap.Rpm.PackagesAdded {
			if p.Include {
				included++
			}
		}
		if included > 0 {
			lines = append(lines, fmt.Sprintf("### Added Packages (%d)", included))
			lines = append(lines, "")
			lines = append(lines, "| Name | Version | Release | Arch | Repo |")
			lines = append(lines, "|------|---------|---------|------|------|")
			for _, p := range snap.Rpm.PackagesAdded {
				if !p.Include {
					continue
				}
				lines = append(lines, fmt.Sprintf("| %s | %s | %s | %s | %s |",
					p.Name, p.Version, p.Release, p.Arch, p.SourceRepo))
			}
			lines = append(lines, "")
		}

		// Version changes
		if len(snap.Rpm.VersionChanges) > 0 {
			lines = append(lines, fmt.Sprintf("### Version Changes (%d)", len(snap.Rpm.VersionChanges)))
			lines = append(lines, "")
			lines = append(lines, "| Package | Host Version | Base Version | Direction |")
			lines = append(lines, "|---------|--------------|--------------|-----------|")
			for _, vc := range snap.Rpm.VersionChanges {
				lines = append(lines, fmt.Sprintf("| %s | %s | %s | %s |",
					vc.Name, vc.HostVersion, vc.BaseVersion, vc.Direction))
			}
			lines = append(lines, "")
		}

		// Module streams
		if len(snap.Rpm.ModuleStreams) > 0 {
			var nonBaseline []schema.EnabledModuleStream
			for _, ms := range snap.Rpm.ModuleStreams {
				if ms.Include && !ms.BaselineMatch {
					nonBaseline = append(nonBaseline, ms)
				}
			}
			if len(nonBaseline) > 0 {
				lines = append(lines, fmt.Sprintf("### Module Streams (%d)", len(nonBaseline)))
				lines = append(lines, "")
				for _, ms := range nonBaseline {
					lines = append(lines, fmt.Sprintf("- %s:%s", ms.ModuleName, ms.Stream))
				}
				lines = append(lines, "")
			}
		}
	}

	// Config files
	if snap.Config != nil && len(snap.Config.Files) > 0 {
		lines = append(lines, "## Configuration Files")
		lines = append(lines, "")

		var modified, unowned int
		for _, f := range snap.Config.Files {
			if !f.Include {
				continue
			}
			switch f.Kind {
			case schema.ConfigFileKindRpmOwnedModified:
				modified++
			case schema.ConfigFileKindUnowned:
				unowned++
			}
		}

		if modified > 0 {
			lines = append(lines, fmt.Sprintf("### Modified RPM-Owned Files (%d)", modified))
			lines = append(lines, "")
			for _, f := range snap.Config.Files {
				if !f.Include || f.Kind != schema.ConfigFileKindRpmOwnedModified {
					continue
				}
				lines = append(lines, fmt.Sprintf("#### `%s`", f.Path))
				lines = append(lines, "")
				if f.DiffAgainstRpm != nil && *f.DiffAgainstRpm != "" {
					lines = append(lines, "```diff")
					lines = append(lines, *f.DiffAgainstRpm)
					lines = append(lines, "```")
					lines = append(lines, "")
					// Summarize changes
					summary := summariseDiff(*f.DiffAgainstRpm)
					if len(summary) > 0 {
						lines = append(lines, "Changes:")
						for _, s := range summary {
							lines = append(lines, fmt.Sprintf("- %s", s))
						}
						lines = append(lines, "")
					}
				}
			}
		}

		if unowned > 0 {
			lines = append(lines, fmt.Sprintf("### Unowned Config Files (%d)", unowned))
			lines = append(lines, "")
			for _, f := range snap.Config.Files {
				if !f.Include || f.Kind != schema.ConfigFileKindUnowned {
					continue
				}
				lines = append(lines, fmt.Sprintf("- `%s` (%s)", f.Path, f.Category))
			}
			lines = append(lines, "")
		}
	}

	// Services
	if snap.Services != nil {
		if len(snap.Services.StateChanges) > 0 {
			lines = append(lines, "## Service State Changes")
			lines = append(lines, "")
			lines = append(lines, "| Unit | Current | Default | Action |")
			lines = append(lines, "|------|---------|---------|--------|")
			for _, sc := range snap.Services.StateChanges {
				lines = append(lines, fmt.Sprintf("| %s | %s | %s | %s |",
					sc.Unit, sc.CurrentState, sc.DefaultState, sc.Action))
			}
			lines = append(lines, "")
		}
	}

	// Redactions
	if len(snap.Redactions) > 0 {
		lines = append(lines, "## Redactions")
		lines = append(lines, "")
		lines = append(lines, fmt.Sprintf("%d item(s) redacted. See `secrets-review.md` for details.", len(snap.Redactions)))
		lines = append(lines, "")
	}

	// Warnings
	if len(snap.Warnings) > 0 {
		lines = append(lines, "## Warnings")
		lines = append(lines, "")
		for _, w := range snap.Warnings {
			msg, _ := w["message"].(string)
			if msg != "" {
				lines = append(lines, fmt.Sprintf("- %s", msg))
			}
		}
		lines = append(lines, "")
	}

	// Refine diff section
	if originalSnap != nil {
		lines = append(lines, "## Refine Session Changes")
		lines = append(lines, "")
		lines = append(lines, "This report was generated during an interactive refine session.")
		lines = append(lines, "Compare with the original snapshot for changes.")
		lines = append(lines, "")
	}

	content := strings.Join(lines, "\n")
	return os.WriteFile(filepath.Join(outputDir, "audit-report.md"), []byte(content), 0644)
}
