package renderer

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// safeWriteFile writes content to dest, handling directory/file
// collisions gracefully.
func safeWriteFile(dest, content string) {
	info, err := os.Stat(dest)
	if err == nil && info.IsDir() {
		fmt.Fprintf(os.Stderr, "inspectah: warning: skipping config file write — path is already a directory: %s\n", dest)
		return
	}

	dir := filepath.Dir(dest)
	if err := os.MkdirAll(dir, 0755); err != nil {
		fmt.Fprintf(os.Stderr, "inspectah: warning: skipping config file write — parent path conflict: %s\n", dest)
		return
	}

	os.WriteFile(dest, []byte(content), 0644)
}

// writeConfigTree writes all config files from snapshot to
// outputDir/config/ preserving paths.
func writeConfigTree(snap *schema.InspectionSnapshot, outputDir string) {
	configDir := filepath.Join(outputDir, "config")
	dhcpPaths := dhcpConnectionPaths(snap)

	// Config files
	if snap.Config != nil {
		for _, entry := range snap.Config.Files {
			if !entry.Include {
				continue
			}
			rel := strings.TrimPrefix(entry.Path, "/")
			if rel == "" {
				continue
			}
			if dhcpPaths[rel] {
				continue
			}
			if strings.HasPrefix(rel, quadletPrefix) {
				continue
			}
			dest := filepath.Join(configDir, rel)
			safeWriteFile(dest, entry.Content)
		}
	}

	// Repo files
	if snap.Rpm != nil {
		for _, repo := range snap.Rpm.RepoFiles {
			if !repo.Include || repo.Path == "" {
				continue
			}
			dest := filepath.Join(configDir, repo.Path)
			safeWriteFile(dest, repo.Content)
		}

		for _, key := range snap.Rpm.GpgKeys {
			if !key.Include || key.Path == "" {
				continue
			}
			dest := filepath.Join(configDir, key.Path)
			safeWriteFile(dest, key.Content)
		}
	}

	// Firewalld zones
	if snap.Network != nil {
		for _, z := range snap.Network.FirewallZones {
			if !z.Include || z.Path == "" {
				continue
			}
			dest := filepath.Join(configDir, z.Path)
			safeWriteFile(dest, z.Content)
		}
	}

	// Kernel boot files
	if snap.KernelBoot != nil {
		// modules-load.d
		for _, m := range snap.KernelBoot.ModulesLoadD {
			if m.Path != "" {
				dest := filepath.Join(configDir, m.Path)
				safeWriteFile(dest, m.Content)
			}
		}
		// modprobe.d
		for _, m := range snap.KernelBoot.ModprobeD {
			if m.Path != "" {
				dest := filepath.Join(configDir, m.Path)
				safeWriteFile(dest, m.Content)
			}
		}
		// dracut.conf.d
		for _, d := range snap.KernelBoot.DracutConf {
			if d.Path != "" {
				dest := filepath.Join(configDir, d.Path)
				safeWriteFile(dest, d.Content)
			}
		}
		// Custom tuned profiles
		for _, tp := range snap.KernelBoot.TunedCustomProfiles {
			if tp.Path != "" {
				dest := filepath.Join(configDir, tp.Path)
				safeWriteFile(dest, tp.Content)
			}
		}
		// Kernel arguments drop-in
		safeKargs := operatorKargs(snap.KernelBoot.Cmdline)
		if len(safeKargs) > 0 {
			kargsDir := filepath.Join(configDir, "usr", "lib", "bootc", "kargs.d")
			os.MkdirAll(kargsDir, 0755)
			var toml strings.Builder
			toml.WriteString("[kargs]\n")
			toml.WriteString(fmt.Sprintf("# Migrated from kernel cmdline by inspectah\n"))
			for _, k := range safeKargs {
				toml.WriteString(fmt.Sprintf(`append = ["%s"]`, k))
				toml.WriteString("\n")
			}
			os.WriteFile(filepath.Join(kargsDir, "inspectah-migrated.toml"), []byte(toml.String()), 0644)
		}
	}

	// Systemd drop-ins — write to both config/ and drop-ins/
	if snap.Services != nil {
		dropInsDir := filepath.Join(outputDir, "drop-ins")
		for _, di := range snap.Services.DropIns {
			if !di.Include {
				continue
			}
			dest := filepath.Join(configDir, di.Path)
			safeWriteFile(dest, di.Content)
			dropInDest := filepath.Join(dropInsDir, di.Path)
			safeWriteFile(dropInDest, di.Content)
		}
	}

	// Timer units (generated and local)
	if snap.ScheduledTasks != nil {
		st := snap.ScheduledTasks
		if len(st.GeneratedTimerUnits) > 0 || len(st.SystemdTimers) > 0 {
			systemdDir := filepath.Join(configDir, "etc", "systemd", "system")
			os.MkdirAll(systemdDir, 0755)

			for _, u := range st.GeneratedTimerUnits {
				if !u.Include {
					continue
				}
				os.WriteFile(filepath.Join(systemdDir, u.Name+".timer"), []byte(u.TimerContent), 0644)
				os.WriteFile(filepath.Join(systemdDir, u.Name+".service"), []byte(u.ServiceContent), 0644)
			}
			for _, t := range st.SystemdTimers {
				if t.Source == "local" {
					if t.Name != "" && t.TimerContent != "" {
						os.WriteFile(filepath.Join(systemdDir, t.Name+".timer"), []byte(t.TimerContent), 0644)
					}
					if t.Name != "" && t.ServiceContent != "" {
						os.WriteFile(filepath.Join(systemdDir, t.Name+".service"), []byte(t.ServiceContent), 0644)
					}
				}
			}
		}
	}

	// Quadlet units
	if snap.Containers != nil {
		for _, u := range snap.Containers.QuadletUnits {
			if !u.Include || u.Name == "" || u.Content == "" {
				continue
			}
			quadletDir := filepath.Join(outputDir, "quadlet")
			os.MkdirAll(quadletDir, 0755)
			os.WriteFile(filepath.Join(quadletDir, u.Name), []byte(u.Content), 0644)
		}
	}

	// Non-RPM env files
	if snap.NonRpmSoftware != nil {
		for _, entry := range snap.NonRpmSoftware.EnvFiles {
			if !entry.Include {
				continue
			}
			rel := strings.TrimPrefix(entry.Path, "/")
			if rel == "" {
				continue
			}
			dest := filepath.Join(configDir, rel)
			safeWriteFile(dest, entry.Content)
		}
	}
}

// configCopyRoots returns the sorted list of top-level directory names
// under configDir that contain files (excluding tmp/).
func configCopyRoots(configDir string) []string {
	entries, err := os.ReadDir(configDir)
	if err != nil {
		return nil
	}

	var roots []string
	for _, e := range entries {
		if !e.IsDir() {
			continue
		}
		name := e.Name()
		if name == "tmp" {
			continue
		}
		// Check if directory has any files
		hasFiles := false
		filepath.Walk(filepath.Join(configDir, name), func(path string, info os.FileInfo, err error) error {
			if err != nil {
				return err
			}
			if !info.IsDir() {
				hasFiles = true
				return filepath.SkipDir
			}
			return nil
		})
		if hasFiles {
			roots = append(roots, name)
		}
	}
	sort.Strings(roots)
	return roots
}

// configInventoryComment generates inventory comment lines listing
// captured config categories.
func configInventoryComment(snap *schema.InspectionSnapshot, dhcpPaths map[string]bool) []string {
	var lines []string

	if snap.Config == nil || len(snap.Config.Files) == 0 {
		return lines
	}

	var included int
	for _, f := range snap.Config.Files {
		if f.Include {
			rel := strings.TrimPrefix(f.Path, "/")
			if dhcpPaths[rel] {
				continue
			}
			included++
		}
	}

	if included > 0 {
		lines = append(lines, fmt.Sprintf("# %d config file(s) captured", included))
	}

	return lines
}

// --- Redacted directory ---

const regenerateTemplate = `REDACTED by inspectah — auto-generated credential
Original path: %s
Action: no action needed — this file is regenerated automatically on the target system
See secrets-review.md for details
`

const provisionTemplate = `REDACTED by inspectah — sensitive file detected
Original path: %s
Action: provision this file on the target system from your secrets management process
See secrets-review.md for details
`

// WriteRedactedDir writes .REDACTED placeholder files for excluded
// secrets to outputDir/redacted/.
func WriteRedactedDir(snap *schema.InspectionSnapshot, outputDir string) error {
	for _, raw := range snap.Redactions {
		var finding schema.RedactionFinding
		if err := json.Unmarshal(raw, &finding); err != nil {
			continue
		}
		if finding.Source != "file" || finding.Kind != "excluded" {
			continue
		}
		rel := strings.TrimPrefix(finding.Path, "/")
		if rel == "" {
			continue
		}

		redactedDir := filepath.Join(outputDir, "redacted")
		dest := filepath.Join(redactedDir, rel+".REDACTED")
		if err := os.MkdirAll(filepath.Dir(dest), 0755); err != nil {
			continue
		}

		var content string
		if finding.Remediation == "regenerate" {
			content = fmt.Sprintf(regenerateTemplate, finding.Path)
		} else {
			content = fmt.Sprintf(provisionTemplate, finding.Path)
		}
		os.WriteFile(dest, []byte(content), 0644)
	}
	return nil
}
