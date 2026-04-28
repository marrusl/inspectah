package renderer

import (
	"crypto/sha256"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// variantInfo holds data about a fleet variant item.
type variantInfo struct {
	path        string
	itemType    string
	contentHash string
	fleetCount  int
	fleetTotal  int
	isWinner    bool
	isTie       bool
}

// RenderMergeNotes produces merge-notes.md documenting fleet merge
// decisions where hosts disagreed on file content.
func RenderMergeNotes(snap *schema.InspectionSnapshot, outputDir string) error {
	items := collectVariantItems(snap)
	if len(items) == 0 {
		return nil // No fleet data = no merge-notes.md
	}

	var lines []string
	lines = append(lines, "# Fleet Merge Notes")
	lines = append(lines, "")
	lines = append(lines, "This file documents fleet merge decisions where hosts disagreed on file content.")
	lines = append(lines, "Review these items to verify the auto-selected variant is correct for your target image.")
	lines = append(lines, "")

	// Group by path
	tied := make(map[string][]variantInfo)
	nonUnanimous := make(map[string][]variantInfo)

	for _, item := range items {
		path := item.path
		if item.isTie {
			tied[path] = append(tied[path], item)
		} else if item.fleetCount < item.fleetTotal {
			nonUnanimous[path] = append(nonUnanimous[path], item)
		}
	}

	if len(tied) > 0 {
		lines = append(lines, "## Tied Items (auto-resolved by content hash)")
		lines = append(lines, "")

		// Sort paths for deterministic output
		paths := make([]string, 0, len(tied))
		for p := range tied {
			paths = append(paths, p)
		}
		sort.Strings(paths)

		for _, path := range paths {
			variants := tied[path]
			if len(variants) == 0 {
				continue
			}
			itemType := variants[0].itemType
			total := variants[0].fleetTotal
			var winner *variantInfo
			for i := range variants {
				if variants[i].isWinner {
					winner = &variants[i]
					break
				}
			}

			lines = append(lines, fmt.Sprintf("### `%s` (%s)", path, itemType))
			lines = append(lines, "")
			lines = append(lines, fmt.Sprintf("- **Variants:** %d", len(variants)))
			lines = append(lines, fmt.Sprintf("- **Fleet total:** %d hosts", total))
			if winner != nil {
				lines = append(lines, fmt.Sprintf("- **Auto-selected:** hash `%s` (%d/%d hosts)",
					winner.contentHash, winner.fleetCount, total))
			}
			lines = append(lines, "")
			lines = append(lines, "| Variant hash | Hosts | Selected |")
			lines = append(lines, "|---|---|---|")
			sort.Slice(variants, func(i, j int) bool {
				return variants[i].contentHash < variants[j].contentHash
			})
			for _, v := range variants {
				selected := "--"
				if v.isWinner {
					selected = "**winner**"
				}
				lines = append(lines, fmt.Sprintf("| `%s` | %d/%d | %s |",
					v.contentHash, v.fleetCount, v.fleetTotal, selected))
			}
			lines = append(lines, "")
		}
	}

	if len(nonUnanimous) > 0 {
		lines = append(lines, "## Non-Unanimous Items")
		lines = append(lines, "")
		lines = append(lines, "These items have a clear winner but were not present on all hosts.")
		lines = append(lines, "")

		paths := make([]string, 0, len(nonUnanimous))
		for p := range nonUnanimous {
			paths = append(paths, p)
		}
		sort.Strings(paths)

		for _, path := range paths {
			variants := nonUnanimous[path]
			if len(variants) == 0 {
				continue
			}
			itemType := variants[0].itemType
			total := variants[0].fleetTotal

			var winner *variantInfo
			for i := range variants {
				if variants[i].isWinner {
					winner = &variants[i]
					break
				}
			}
			if winner != nil {
				lines = append(lines, fmt.Sprintf("- `%s` (%s): winner at %d/%d hosts, %d variant(s)",
					path, itemType, winner.fleetCount, total, len(variants)))
			} else {
				lines = append(lines, fmt.Sprintf("- `%s` (%s): %d variant(s), no winner selected",
					path, itemType, len(variants)))
			}
		}
	}

	content := strings.Join(lines, "\n") + "\n"
	return os.WriteFile(filepath.Join(outputDir, "merge-notes.md"), []byte(content), 0644)
}

// collectVariantItems extracts items with fleet prevalence data for merge notes.
func collectVariantItems(snap *schema.InspectionSnapshot) []variantInfo {
	var items []variantInfo

	// Config files
	if snap.Config != nil {
		for _, f := range snap.Config.Files {
			if f.Fleet == nil {
				continue
			}
			items = append(items, variantInfo{
				path:        f.Path,
				itemType:    "config",
				contentHash: contentHash(f.Content),
				fleetCount:  f.Fleet.Count,
				fleetTotal:  f.Fleet.Total,
				isWinner:    f.TieWinner,
				isTie:       f.Tie,
			})
		}
	}

	// Drop-ins
	if snap.Services != nil {
		for _, d := range snap.Services.DropIns {
			if d.Fleet == nil {
				continue
			}
			items = append(items, variantInfo{
				path:        d.Path,
				itemType:    "drop-in",
				contentHash: contentHash(d.Content),
				fleetCount:  d.Fleet.Count,
				fleetTotal:  d.Fleet.Total,
				isWinner:    d.TieWinner,
				isTie:       d.Tie,
			})
		}
	}

	// Quadlet units
	if snap.Containers != nil {
		for _, q := range snap.Containers.QuadletUnits {
			if q.Fleet == nil {
				continue
			}
			items = append(items, variantInfo{
				path:        q.Path,
				itemType:    "quadlet",
				contentHash: contentHash(q.Content),
				fleetCount:  q.Fleet.Count,
				fleetTotal:  q.Fleet.Total,
				isWinner:    q.TieWinner,
				isTie:       q.Tie,
			})
		}

		for _, c := range snap.Containers.ComposeFiles {
			if c.Fleet == nil {
				continue
			}
			items = append(items, variantInfo{
				path:        c.Path,
				itemType:    "compose",
				contentHash: fmt.Sprintf("%x", sha256.Sum256([]byte(c.Path)))[:8],
				fleetCount:  c.Fleet.Count,
				fleetTotal:  c.Fleet.Total,
				isWinner:    c.TieWinner,
				isTie:       c.Tie,
			})
		}
	}

	// Non-RPM env files
	if snap.NonRpmSoftware != nil {
		for _, e := range snap.NonRpmSoftware.EnvFiles {
			if e.Fleet == nil {
				continue
			}
			items = append(items, variantInfo{
				path:        e.Path,
				itemType:    "env",
				contentHash: contentHash(e.Content),
				fleetCount:  e.Fleet.Count,
				fleetTotal:  e.Fleet.Total,
				isWinner:    e.TieWinner,
				isTie:       e.Tie,
			})
		}
	}

	return items
}

// contentHash returns a short SHA-256 hash of content, matching the
// Python merge normalization.
func contentHash(content string) string {
	// Normalize: strip trailing whitespace per line, strip leading/trailing blank lines
	lines := strings.Split(content, "\n")
	var normalized []string
	for _, l := range lines {
		normalized = append(normalized, strings.TrimRight(l, " \t\r"))
	}
	text := strings.TrimSpace(strings.Join(normalized, "\n"))
	hash := sha256.Sum256([]byte(text))
	return fmt.Sprintf("%x", hash)[:8]
}
