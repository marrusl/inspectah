package renderer

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// RenderReadme produces README.md summarizing the scan output.
func RenderReadme(snap *schema.InspectionSnapshot, outputDir string) error {
	var lines []string

	lines = append(lines, "# inspectah output")
	lines = append(lines, "")

	// Summary of findings
	osName := ""
	if snap.OsRelease != nil {
		osName = snap.OsRelease.PrettyName
		if osName == "" {
			osName = snap.OsRelease.Name
		}
		lines = append(lines, fmt.Sprintf("Generated from **%s**.", osName))
		lines = append(lines, "")
	}

	hostname := ""
	if snap.Meta != nil {
		if h, ok := snap.Meta["hostname"].(string); ok {
			hostname = h
		}
	}
	if hostname != "" {
		lines = append(lines, fmt.Sprintf("Hostname: `%s`", hostname))
		lines = append(lines, "")
	}

	// Findings summary table
	lines = append(lines, "## Findings summary")
	lines = append(lines, "")

	pkgAdded := 0
	if snap.Rpm != nil {
		for _, p := range snap.Rpm.PackagesAdded {
			if p.Include {
				pkgAdded++
			}
		}
	}

	configsModified := 0
	configsUnowned := 0
	if snap.Config != nil {
		for _, f := range snap.Config.Files {
			if strings.TrimPrefix(f.Path, "/") != "" && !strings.HasPrefix(strings.TrimPrefix(f.Path, "/"), quadletPrefix) {
				switch f.Kind {
				case schema.ConfigFileKindRpmOwnedModified:
					configsModified++
				case schema.ConfigFileKindUnowned:
					configsUnowned++
				}
			}
		}
	}

	svcEnabled := 0
	svcDisabled := 0
	if snap.Services != nil {
		svcEnabled = len(snap.Services.EnabledUnits)
		svcDisabled = len(snap.Services.DisabledUnits)
	}

	warningsCount := len(snap.Warnings)
	redactionsCount := CountActiveRedactions(snap.Redactions)

	fixmes := extractFIXMEs(outputDir)

	lines = append(lines, "| Category | Count |")
	lines = append(lines, "|---|---|")
	if snap.Rpm != nil && snap.Rpm.NoBaseline {
		lines = append(lines, fmt.Sprintf("| Packages (all -- no baseline) | %d |", pkgAdded))
	} else {
		lines = append(lines, fmt.Sprintf("| Packages added (beyond base image) | %d |", pkgAdded))
	}
	if configsModified > 0 {
		lines = append(lines, fmt.Sprintf("| Config files (RPM-modified) | %d |", configsModified))
	}
	if configsUnowned > 0 {
		lines = append(lines, fmt.Sprintf("| Config files (unowned) | %d |", configsUnowned))
	}
	lines = append(lines, fmt.Sprintf("| Services (%d enabled, %d disabled) | %d |", svcEnabled, svcDisabled, svcEnabled+svcDisabled))

	if snap.NonRpmSoftware != nil && len(snap.NonRpmSoftware.Items) > 0 {
		lines = append(lines, fmt.Sprintf("| Non-RPM software items | %d |", len(snap.NonRpmSoftware.Items)))
	}
	if snap.Containers != nil {
		q := len(snap.Containers.QuadletUnits)
		c := len(snap.Containers.ComposeFiles)
		if q > 0 || c > 0 {
			lines = append(lines, fmt.Sprintf("| Container workloads | %d quadlet, %d compose |", q, c))
		}
	}
	if redactionsCount > 0 {
		lines = append(lines, fmt.Sprintf("| Secrets redacted | %d |", redactionsCount))
	}
	lines = append(lines, fmt.Sprintf("| Warnings | %d |", warningsCount))
	lines = append(lines, fmt.Sprintf("| FIXME items | %d |", len(fixmes)))
	lines = append(lines, "")

	// Build and deploy
	base := baseImageFromSnapshot(snap)
	lines = append(lines, "## Build and deploy")
	lines = append(lines, "")
	lines = append(lines, "```bash")
	lines = append(lines, fmt.Sprintf("podman build -t my-bootc-image -f Containerfile ."))
	lines = append(lines, "```")
	lines = append(lines, "")
	lines = append(lines, "```bash")

	hasKargs := snap.KernelBoot != nil && snap.KernelBoot.Cmdline != ""
	if hasKargs {
		lines = append(lines, "# Custom kernel args detected -- verify they are baked into the image")
		lines = append(lines, "# or pass them via the bootloader configuration at deploy time.")
	}
	lines = append(lines, "# Switch an existing system to the new image:")
	lines = append(lines, "bootc switch my-bootc-image:latest")
	lines = append(lines, "")
	lines = append(lines, "# Or install to a new disk:")

	var installFlags []string
	isCentos := snap.OsRelease != nil && snap.OsRelease.ID == "centos"
	if isCentos {
		installFlags = append(installFlags, "--target-no-signature-verification")
	}
	hasSELinux := snap.Selinux != nil
	if hasSELinux && snap.Selinux.Mode == "enforcing" {
		installFlags = append(installFlags, "--enforce-container-sigpolicy")
	}
	if len(installFlags) > 0 {
		lines = append(lines, fmt.Sprintf("bootc install to-disk %s /dev/sdX", strings.Join(installFlags, " ")))
	} else {
		lines = append(lines, "bootc install to-disk /dev/sdX")
	}
	lines = append(lines, "```")
	lines = append(lines, "")
	lines = append(lines, "Review `kickstart-suggestion.ks` for deployment-time settings (hostname, DHCP, DNS).")
	lines = append(lines, "")

	// Artifacts
	lines = append(lines, "## Artifacts")
	lines = append(lines, "")
	lines = append(lines, "| File | Description |")
	lines = append(lines, "|---|---|")
	lines = append(lines, "| `Containerfile` | Image definition |")
	lines = append(lines, "| `config/` | Files to COPY into the image |")
	lines = append(lines, "| `audit-report.md` | Full findings (markdown) |")
	lines = append(lines, "| `report.html` | Interactive report (open in browser) |")
	lines = append(lines, "| `secrets-review.md` | Redacted items requiring manual handling |")
	lines = append(lines, "| `kickstart-suggestion.ks` | Suggested deploy-time settings |")
	lines = append(lines, "| `inspection-snapshot.json` | Raw data for re-rendering (`--from-snapshot`) |")
	if _, err := os.Stat(filepath.Join(outputDir, "merge-notes.md")); err == nil {
		lines = append(lines, "| `merge-notes.md` | Fleet merge decisions -- ties, non-unanimous items |")
	}
	lines = append(lines, "")

	// FIXME items
	if len(fixmes) > 0 {
		lines = append(lines, "## FIXME items (resolve before production)")
		lines = append(lines, "")
		for i, fixme := range fixmes {
			lines = append(lines, fmt.Sprintf("%d. %s", i+1, fixme))
		}
		lines = append(lines, "")
	}

	// Warnings
	if len(snap.Warnings) > 0 {
		lines = append(lines, "## Warnings")
		lines = append(lines, "")
		for _, w := range snap.Warnings {
			msg, _ := w["message"].(string)
			src, _ := w["source"].(string)
			if msg == "" {
				msg = "--"
			}
			prefix := ""
			if src != "" {
				prefix = fmt.Sprintf("**%s:** ", src)
			}
			lines = append(lines, fmt.Sprintf("- %s%s", prefix, msg))
		}
		lines = append(lines, "")
	}

	// User creation strategies
	if snap.UsersGroups != nil && len(snap.UsersGroups.Users) > 0 {
		lines = append(lines, "## User Creation Strategies")
		lines = append(lines, "")
		lines = append(lines, "bootc performs a three-way merge on `/etc` during image updates. "+
			"Users baked into `/etc/passwd` in the image can conflict with runtime changes. "+
			"Declarative and deploy-time approaches avoid this.")
		lines = append(lines, "")
		lines = append(lines, "| Strategy | What it does | When to use | Risk |")
		lines = append(lines, "|----------|-------------|-------------|------|")
		lines = append(lines, "| **sysusers** | systemd-sysusers drop-in creates users at boot | Service accounts (nologin shell) | Users not visible until first boot |")
		lines = append(lines, "| **useradd** | Explicit `RUN useradd` in Containerfile | Accounts needing precise control in the image | Conflicts with bootc `/etc` merge on updates |")
		lines = append(lines, "| **kickstart** | User directives in kickstart at deploy time | Human users, site-specific accounts | Users missing if kickstart not applied |")
		lines = append(lines, "| **blueprint** | bootc-image-builder TOML customization | When using image-builder as build pipeline | Only works with bootc-image-builder |")
		lines = append(lines, "")
	}

	lines = append(lines, "See [`audit-report.md`](audit-report.md) or [`report.html`](report.html) for full details.")
	lines = append(lines, "")

	_ = base // used for potential future FROM reference
	content := strings.Join(lines, "\n")
	return os.WriteFile(filepath.Join(outputDir, "README.md"), []byte(content), 0644)
}

// extractFIXMEs pulls FIXME comments from the generated Containerfile.
func extractFIXMEs(outputDir string) []string {
	data, err := os.ReadFile(filepath.Join(outputDir, "Containerfile"))
	if err != nil {
		return nil
	}

	var fixmes []string
	for _, line := range strings.Split(string(data), "\n") {
		stripped := strings.TrimSpace(line)
		if strings.Contains(stripped, "FIXME") && strings.HasPrefix(stripped, "#") {
			fixme := strings.TrimLeft(stripped, "# ")
			fixmes = append(fixmes, strings.TrimSpace(fixme))
		}
	}
	return fixmes
}
