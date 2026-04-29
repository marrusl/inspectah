package renderer

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// RenderContainerfile produces the Containerfile and config/ tree from
// the snapshot. This is a code-based renderer — no templates needed.
func RenderContainerfile(snap *schema.InspectionSnapshot, outputDir string) error {
	if err := os.MkdirAll(outputDir, 0755); err != nil {
		return fmt.Errorf("create output dir: %w", err)
	}

	writeConfigTree(snap, outputDir)

	content := renderContainerfileContent(snap, outputDir)
	return os.WriteFile(filepath.Join(outputDir, "Containerfile"), []byte(content), 0644)
}

// renderContainerfileContent builds the Containerfile content from snapshot.
func renderContainerfileContent(snap *schema.InspectionSnapshot, outputDir string) string {
	base := baseImageFromSnapshot(snap)
	cExtPip, purePip := classifyPip(snap)
	needsMultistage := len(cExtPip) > 0
	dhcpPaths := dhcpConnectionPaths(snap)

	var lines []string

	// Layer order matches design doc for cache efficiency
	lines = append(lines, packagesSectionLines(snap, base, cExtPip, needsMultistage)...)

	// bootc label for ostree-desktops base images
	if snap.SystemType == schema.SystemTypeRpmOstree || snap.SystemType == schema.SystemTypeBootc {
		if strings.Contains(base, "fedora-ostree-desktops") {
			lines = append(lines, "# ostree-desktops images may need bootc label for compatibility")
			lines = append(lines, `LABEL containers.bootc 1`)
			lines = append(lines, "")
		}
	}

	lines = append(lines, servicesSectionLines(snap)...)
	lines = append(lines, networkSectionLines(snap, true)...)
	lines = append(lines, scheduledTasksSectionLines(snap)...)
	lines = append(lines, configSectionLines(snap, outputDir, dhcpPaths)...)
	lines = append(lines, nonRpmSectionLines(snap, purePip, needsMultistage)...)
	lines = append(lines, containersSectionLines(snap)...)
	lines = append(lines, usersSectionLines(snap)...)
	lines = append(lines, kernelBootSectionLines(snap)...)
	lines = append(lines, selinuxSectionLines(snap)...)
	lines = append(lines, networkSectionLines(snap, false)...)

	// Secrets comment blocks
	lines = append(lines, secretsCommentLines(snap)...)

	// Epilogue
	lines = append(lines, tmpfilesLines()...)
	lines = append(lines, validateLines()...)

	return strings.Join(lines, "\n")
}

// classifyPip separates pip items into C-extension (need build stage)
// and pure-python lists.
func classifyPip(snap *schema.InspectionSnapshot) (cExt, pure []schema.NonRpmItem) {
	if snap.NonRpmSoftware == nil {
		return nil, nil
	}
	for _, item := range snap.NonRpmSoftware.Items {
		if !item.Include {
			continue
		}
		if item.Method == "pip dist-info" && item.HasCExtensions {
			cExt = append(cExt, item)
		} else if item.Method == "pip dist-info" && !item.HasCExtensions && item.Version != "" {
			pure = append(pure, item)
		}
	}
	return
}

// tmpfilesLines generates the epilogue for tmp directory setup.
func tmpfilesLines() []string {
	return []string{
		"# === Finalize: systemd-tmpfiles for /tmp, /run, /var, /etc/ above",
		"",
	}
}

// validateLines generates the bootc validation epilogue.
func validateLines() []string {
	return []string{
		"# === Validate bootc compatibility ===",
		"RUN bootc container lint",
	}
}

// secretsCommentLines generates Containerfile comment blocks for
// redacted secrets. Only file-backed findings appear here.
func secretsCommentLines(snap *schema.InspectionSnapshot) []string {
	var excluded, flagged []schema.RedactionFinding
	for _, raw := range snap.Redactions {
		finding, err := schema.ParseRedaction(raw)
		if err != nil {
			continue
		}
		if finding.Source != "file" {
			continue
		}
		switch finding.Kind {
		case "excluded":
			excluded = append(excluded, *finding)
		case "flagged":
			flagged = append(flagged, *finding)
		}
	}

	if len(excluded) == 0 && len(flagged) == 0 {
		return nil
	}

	var lines []string
	if len(excluded) > 0 {
		lines = append(lines, "# === Secrets: Excluded Files ===")
		lines = append(lines, fmt.Sprintf("# %d file(s) excluded from the image for security:", len(excluded)))
		for _, f := range excluded {
			lines = append(lines, fmt.Sprintf("#   %s (%s)", f.Path, f.Remediation))
		}
		lines = append(lines, "# See secrets-review.md for details and remediation steps.")
		lines = append(lines, "")
	}
	if len(flagged) > 0 {
		lines = append(lines, "# === Secrets: Flagged for Review ===")
		lines = append(lines, fmt.Sprintf("# %d file(s) flagged for manual review:", len(flagged)))
		for _, f := range flagged {
			lines = append(lines, fmt.Sprintf("#   %s", f.Path))
		}
		lines = append(lines, "# See secrets-review.md for details.")
		lines = append(lines, "")
	}
	return lines
}

// --- Packages section ---

func packagesSectionLines(snap *schema.InspectionSnapshot, base string, cExtPip []schema.NonRpmItem, needsMultistage bool) []string {
	var lines []string

	if needsMultistage {
		lines = append(lines, "# === Build stage: compile pip packages with C extensions ===")
		lines = append(lines, fmt.Sprintf("FROM %s AS builder", base))
		lines = append(lines, "RUN dnf install -y gcc python3-devel make && dnf clean all && rm -rf /var/cache/dnf /var/lib/dnf/history* /var/log/dnf* /var/log/hawkey.log /var/log/rhsm")
		lines = append(lines, "RUN python3 -m venv /opt/venv")
		for _, pkg := range cExtPip {
			if pkg.Version != "" {
				lines = append(lines, fmt.Sprintf("RUN /opt/venv/bin/pip install --no-cache-dir %s==%s", pkg.Name, pkg.Version))
			} else {
				lines = append(lines, fmt.Sprintf("RUN /opt/venv/bin/pip install --no-cache-dir %s", pkg.Name))
			}
		}
		lines = append(lines, "")
	}

	lines = append(lines, fmt.Sprintf("FROM %s", base))
	lines = append(lines, "")

	if snap.Rpm == nil {
		return lines
	}

	// Repo files
	if len(snap.Rpm.RepoFiles) > 0 {
		included := 0
		for _, r := range snap.Rpm.RepoFiles {
			if r.Include && !r.IsDefaultRepo {
				included++
			}
		}
		if included > 0 {
			lines = append(lines, fmt.Sprintf("# === Custom Repositories (%d) ===", included))
			lines = append(lines, "COPY config/etc/yum.repos.d/ /etc/yum.repos.d/")
			lines = append(lines, "")
		}
	}

	// GPG keys
	if len(snap.Rpm.GpgKeys) > 0 {
		included := 0
		for _, k := range snap.Rpm.GpgKeys {
			if k.Include {
				included++
			}
		}
		if included > 0 {
			lines = append(lines, fmt.Sprintf("# === GPG Keys (%d) ===", included))
			lines = append(lines, "COPY config/etc/pki/rpm-gpg/ /etc/pki/rpm-gpg/")
			lines = append(lines, "")
		}
	}

	// Module streams
	if len(snap.Rpm.ModuleStreams) > 0 {
		var enabled []schema.EnabledModuleStream
		for _, ms := range snap.Rpm.ModuleStreams {
			if ms.Include && !ms.BaselineMatch {
				enabled = append(enabled, ms)
			}
		}
		if len(enabled) > 0 {
			lines = append(lines, "# === Module Streams ===")
			for _, ms := range enabled {
				profiles := ""
				if len(ms.Profiles) > 0 {
					profiles = "/" + strings.Join(ms.Profiles, ",")
				}
				lines = append(lines, fmt.Sprintf("RUN dnf module enable -y %s:%s%s", ms.ModuleName, ms.Stream, profiles))
			}
			lines = append(lines, "")
		}
	}

	// Packages
	var installNames []string
	if snap.Rpm.LeafPackages != nil {
		for _, name := range *snap.Rpm.LeafPackages {
			if sanitizeShellValue(name, "dnf install") != nil {
				installNames = append(installNames, name)
			}
		}
	} else {
		for _, pkg := range snap.Rpm.PackagesAdded {
			if pkg.Include && sanitizeShellValue(pkg.Name, "dnf install") != nil {
				installNames = append(installNames, pkg.Name)
			}
		}
	}

	if len(installNames) > 0 {
		sort.Strings(installNames)
		lines = append(lines, fmt.Sprintf("# === Packages (%d) ===", len(installNames)))
		if len(installNames) <= 10 {
			lines = append(lines, "RUN dnf install -y "+strings.Join(installNames, " ")+
				" && dnf clean all && rm -rf /var/cache/dnf /var/lib/dnf/history* /var/log/dnf* /var/log/hawkey.log /var/log/rhsm")
		} else {
			lines = append(lines, "RUN dnf install -y \\")
			for i, name := range installNames {
				if i < len(installNames)-1 {
					lines = append(lines, "    "+name+" \\")
				} else {
					lines = append(lines, "    "+name+" \\")
				}
			}
			lines = append(lines, "    && dnf clean all && rm -rf /var/cache/dnf /var/lib/dnf/history* /var/log/dnf* /var/log/hawkey.log /var/log/rhsm")
		}
		lines = append(lines, "")
	}

	// Version locks
	var includedLocks []schema.VersionLockEntry
	for _, vl := range snap.Rpm.VersionLocks {
		if vl.Include {
			includedLocks = append(includedLocks, vl)
		}
	}
	if len(includedLocks) > 0 {
		lines = append(lines, "# === Version Locks ===")
		lines = append(lines, "RUN dnf install -y python3-dnf-plugin-versionlock && \\")
		for _, vl := range includedLocks {
			lines = append(lines, fmt.Sprintf("    dnf versionlock add %s && \\", vl.RawPattern))
		}
		lines = append(lines, "    dnf clean all")
		lines = append(lines, "")
	}

	return lines
}

// --- Services section ---

func servicesSectionLines(snap *schema.InspectionSnapshot) []string {
	var lines []string

	// Build set of config-tree units to exclude
	configTreeUnits := make(map[string]bool)
	if snap.ScheduledTasks != nil {
		for _, t := range snap.ScheduledTasks.SystemdTimers {
			if t.Source == "local" && t.Name != "" {
				configTreeUnits[t.Name+".timer"] = true
				configTreeUnits[t.Name+".service"] = true
			}
		}
		for _, u := range snap.ScheduledTasks.GeneratedTimerUnits {
			if u.Include && u.Name != "" {
				configTreeUnits[u.Name+".timer"] = true
				configTreeUnits[u.Name+".service"] = true
			}
		}
	}

	if snap.Services == nil {
		return lines
	}

	enabled := snap.Services.EnabledUnits
	disabled := snap.Services.DisabledUnits

	if len(enabled) == 0 && len(disabled) == 0 {
		return lines
	}

	lines = append(lines, "# === Service Enablement ===")

	// Build installable package set
	installable := make(map[string]bool)
	if snap.Rpm != nil {
		if snap.Rpm.BaselinePackageNames != nil {
			for _, n := range *snap.Rpm.BaselinePackageNames {
				installable[n] = true
			}
		}
		if snap.Rpm.LeafPackages != nil {
			for _, n := range *snap.Rpm.LeafPackages {
				installable[n] = true
			}
		}
		for _, p := range snap.Rpm.PackagesAdded {
			if p.Include {
				installable[p.Name] = true
			}
		}
	}

	// Build unit -> owning_package lookup
	unitOwner := make(map[string]string)
	for _, sc := range snap.Services.StateChanges {
		if sc.OwningPackage != nil {
			unitOwner[sc.Unit] = *sc.OwningPackage
		}
	}

	unitInstallable := func(unit string) bool {
		owner, ok := unitOwner[unit]
		if !ok {
			return true
		}
		if len(installable) == 0 {
			return true
		}
		return installable[owner]
	}

	var safeEnabled, safeDisabled []string
	var deferred []string

	for _, u := range enabled {
		if sanitizeShellValue(u, "systemctl enable") == nil {
			continue
		}
		if configTreeUnits[u] {
			deferred = append(deferred, u)
			continue
		}
		if unitInstallable(u) {
			safeEnabled = append(safeEnabled, u)
		}
	}
	for _, u := range disabled {
		if sanitizeShellValue(u, "systemctl disable") == nil {
			continue
		}
		if unitInstallable(u) {
			safeDisabled = append(safeDisabled, u)
		}
	}

	if len(safeEnabled) > 0 {
		lines = append(lines, "RUN systemctl enable "+strings.Join(safeEnabled, " "))
	}
	if len(safeDisabled) > 0 {
		lines = append(lines, "RUN systemctl disable "+strings.Join(safeDisabled, " "))
	}
	if len(deferred) > 0 {
		lines = append(lines, fmt.Sprintf("# %d unit(s) deferred to Scheduled Tasks section: %s",
			len(deferred), strings.Join(deferred, ", ")))
	}

	lines = append(lines, "")
	return lines
}

// --- Network section ---

func networkSectionLines(snap *schema.InspectionSnapshot, firewallOnly bool) []string {
	var lines []string
	if snap.Network == nil {
		return lines
	}

	if firewallOnly {
		// Firewall zones
		var includedZones []schema.FirewallZone
		for _, z := range snap.Network.FirewallZones {
			if z.Include {
				includedZones = append(includedZones, z)
			}
		}

		if len(includedZones) > 0 || len(snap.Network.FirewallDirectRules) > 0 {
			lines = append(lines, "# === Firewall Configuration ===")
			if len(includedZones) > 0 {
				lines = append(lines, fmt.Sprintf("# %d custom firewall zone(s) — included in COPY config/etc/ below", len(includedZones)))
			}
			lines = append(lines, "")
		}
		return lines
	}

	// Non-firewall network config
	if len(snap.Network.StaticRoutes) > 0 {
		lines = append(lines, "# === Static Routes ===")
		for _, r := range snap.Network.StaticRoutes {
			lines = append(lines, fmt.Sprintf("# Static route file: %s", r.Path))
		}
		lines = append(lines, "")
	}

	if len(snap.Network.HostsAdditions) > 0 {
		lines = append(lines, "# === /etc/hosts Additions ===")
		lines = append(lines, "# FIXME: These /etc/hosts entries need to be added to the image:")
		for _, h := range snap.Network.HostsAdditions {
			lines = append(lines, fmt.Sprintf("#   %s", h))
		}
		lines = append(lines, "")
	}

	if len(snap.Network.Proxy) > 0 {
		lines = append(lines, "# === Proxy Configuration ===")
		for _, p := range snap.Network.Proxy {
			lines = append(lines, fmt.Sprintf("# %s: %s", p.Source, p.Line))
		}
		lines = append(lines, "")
	}

	return lines
}

// --- Scheduled Tasks section ---

func scheduledTasksSectionLines(snap *schema.InspectionSnapshot) []string {
	var lines []string

	st := snap.ScheduledTasks
	if st == nil {
		return lines
	}

	hasContent := len(st.GeneratedTimerUnits) > 0 || len(st.SystemdTimers) > 0 || len(st.CronJobs) > 0 || len(st.AtJobs) > 0
	if !hasContent {
		return lines
	}

	lines = append(lines, "# === Scheduled Tasks ===")

	var localTimers []schema.SystemdTimer
	for _, t := range st.SystemdTimers {
		if t.Source == "local" && isIncluded(t.Include) {
			localTimers = append(localTimers, t)
		}
	}

	var includedTimers []schema.GeneratedTimerUnit
	for _, u := range st.GeneratedTimerUnits {
		if u.Include {
			includedTimers = append(includedTimers, u)
		}
	}

	if len(localTimers) > 0 || len(includedTimers) > 0 {
		lines = append(lines, "COPY config/etc/systemd/system/ /etc/systemd/system/")
	}

	if len(localTimers) > 0 {
		names := make([]string, len(localTimers))
		for i, t := range localTimers {
			names[i] = t.Name + ".timer"
		}
		lines = append(lines, fmt.Sprintf("# Existing local timers (%d): %s", len(localTimers), strings.Join(names, ", ")))
	}

	if len(includedTimers) > 0 {
		names := make([]string, 0, len(includedTimers))
		for _, u := range includedTimers {
			if u.Name != "" {
				names = append(names, u.Name)
			}
		}
		lines = append(lines, fmt.Sprintf("# Converted from cron: %d timer(s): %s", len(includedTimers), strings.Join(names, ", ")))
	}

	// Consolidate timer enables
	var timerNames []string
	for _, t := range localTimers {
		timerNames = append(timerNames, t.Name+".timer")
	}
	for _, u := range includedTimers {
		if u.Name != "" {
			timerNames = append(timerNames, u.Name+".timer")
		}
	}
	if len(timerNames) > 0 {
		lines = append(lines, "RUN systemctl enable "+strings.Join(timerNames, " "))
	}

	if len(st.AtJobs) > 0 {
		lines = append(lines, fmt.Sprintf("# FIXME: %d at job(s) found — convert to systemd timers or cron", len(st.AtJobs)))
		for _, a := range st.AtJobs {
			lines = append(lines, fmt.Sprintf("#   at job: %s", a.Command))
		}
	}

	lines = append(lines, "")
	return lines
}

// --- Config section ---

func configSectionLines(snap *schema.InspectionSnapshot, outputDir string, dhcpPaths map[string]bool) []string {
	var lines []string

	lines = append(lines, "# === Configuration Files ===")

	// Inventory comment
	inventory := configInventoryComment(snap, dhcpPaths)
	lines = append(lines, inventory...)

	if snap.Config != nil {
		hasDiffs := false
		for _, f := range snap.Config.Files {
			if f.DiffAgainstRpm != nil {
				hasDiffs = true
				break
			}
		}
		if hasDiffs {
			lines = append(lines, "# Config diffs (--config-diffs): see audit-report.md and report.html for per-file diffs.")
		}
	}
	lines = append(lines, "")

	// COPY per top-level dir
	configDir := filepath.Join(outputDir, "config")
	roots := configCopyRoots(configDir)
	for _, root := range roots {
		lines = append(lines, fmt.Sprintf("COPY config/%s/ /%s/", root, root))
	}
	if len(roots) == 0 {
		lines = append(lines, "# (no config files captured)")
	}
	lines = append(lines, "")

	// CA trust anchors
	caAnchorPrefix := "etc/pki/ca-trust/source/anchors/"
	hasCAAnchors := false
	if snap.Config != nil {
		for _, f := range snap.Config.Files {
			if f.Include && strings.HasPrefix(strings.TrimPrefix(f.Path, "/"), caAnchorPrefix) {
				hasCAAnchors = true
				break
			}
		}
	}
	if hasCAAnchors {
		lines = append(lines, "# === CA Trust Store ===")
		lines = append(lines, "# Custom CA certificates detected in /etc/pki/ca-trust/source/anchors/")
		lines = append(lines, "RUN update-ca-trust")
		lines = append(lines, "")
	}

	// Crypto policy
	lines = append(lines, cryptoPolicyLines(snap)...)

	return lines
}

// cryptoPolicyLines emits update-crypto-policies --set if a custom policy is configured.
func cryptoPolicyLines(snap *schema.InspectionSnapshot) []string {
	if snap.Config == nil {
		return nil
	}

	for _, f := range snap.Config.Files {
		if f.Category == schema.ConfigCategoryCryptoPolicy &&
			f.Path == "/etc/crypto-policies/config" &&
			f.Include {
			policy := ""
			if f.Content != "" {
				lines := strings.SplitN(f.Content, "\n", 2)
				policy = strings.TrimSpace(strings.SplitN(lines[0], "#", 2)[0])
			}
			if policy == "" || policy == "DEFAULT" {
				return nil
			}
			// Validate policy name
			if !tunedProfileRe.MatchString(policy) {
				return []string{
					fmt.Sprintf("# WARNING: crypto policy name contains unexpected characters, skipped: %q", policy),
					"",
				}
			}
			return []string{
				fmt.Sprintf("# System crypto policy: %s", policy),
				fmt.Sprintf("RUN update-crypto-policies --set %s", policy),
				"",
			}
		}
	}
	return nil
}

// --- Containers section ---

func containersSectionLines(snap *schema.InspectionSnapshot) []string {
	var lines []string
	if snap.Containers == nil {
		return lines
	}

	var includedQuadlets []schema.QuadletUnit
	for _, u := range snap.Containers.QuadletUnits {
		if u.Include {
			includedQuadlets = append(includedQuadlets, u)
		}
	}

	var includedCompose []schema.ComposeFile
	for _, c := range snap.Containers.ComposeFiles {
		if c.Include {
			includedCompose = append(includedCompose, c)
		}
	}

	if len(includedQuadlets) == 0 && len(includedCompose) == 0 {
		return lines
	}

	lines = append(lines, "# === Container Workloads ===")
	if len(includedQuadlets) > 0 {
		lines = append(lines, "COPY quadlet/ /etc/containers/systemd/")
	}
	if len(includedCompose) > 0 {
		for _, cf := range includedCompose {
			lines = append(lines, fmt.Sprintf("# Compose file included: %s", cf.Path))
		}
		lines = append(lines, "# Compose file(s) included as-is. For native systemd integration,")
		lines = append(lines, "# consider converting to Quadlet units — see https://github.com/containers/podlet")
		lines = append(lines, "# or https://docs.podman.io/en/latest/markdown/podman-systemd.unit.5.html")
	}
	lines = append(lines, "")
	return lines
}

// --- Non-RPM Software section ---

func nonRpmSectionLines(snap *schema.InspectionSnapshot, purePip []schema.NonRpmItem, needsMultistage bool) []string {
	var lines []string
	if snap.NonRpmSoftware == nil || len(snap.NonRpmSoftware.Items) == 0 {
		return lines
	}

	var includedItems []schema.NonRpmItem
	for _, item := range snap.NonRpmSoftware.Items {
		if item.Include {
			includedItems = append(includedItems, item)
		}
	}
	if len(includedItems) == 0 {
		return lines
	}

	lines = append(lines, "# === Non-RPM Software ===")

	// Group by method
	var pipItems, npmItems, goItems, standaloneItems []schema.NonRpmItem
	for _, item := range includedItems {
		switch {
		case strings.HasPrefix(item.Method, "pip"):
			pipItems = append(pipItems, item)
		case strings.Contains(item.Method, "npm") || strings.Contains(item.Method, "yarn"):
			npmItems = append(npmItems, item)
		case item.Lang == "go" || item.Method == "go binary":
			goItems = append(goItems, item)
		default:
			standaloneItems = append(standaloneItems, item)
		}
	}

	if len(pipItems) > 0 {
		lines = append(lines, fmt.Sprintf("# pip packages (%d):", len(pipItems)))
		for _, p := range pipItems {
			if p.Version != "" {
				lines = append(lines, fmt.Sprintf("#   %s==%s", p.Name, p.Version))
			} else {
				lines = append(lines, fmt.Sprintf("#   %s", p.Name))
			}
		}
		if needsMultistage {
			lines = append(lines, "COPY --from=builder /opt/venv /opt/venv")
		}
	}

	if len(npmItems) > 0 {
		lines = append(lines, fmt.Sprintf("# Node.js packages (%d):", len(npmItems)))
		for _, n := range npmItems {
			lines = append(lines, fmt.Sprintf("#   %s (%s)", n.Name, n.Method))
		}
	}

	if len(goItems) > 0 {
		lines = append(lines, fmt.Sprintf("# Go binaries (%d):", len(goItems)))
		for _, g := range goItems {
			lines = append(lines, fmt.Sprintf("#   %s at %s", g.Name, g.Path))
		}
	}

	if len(standaloneItems) > 0 {
		lines = append(lines, fmt.Sprintf("# Other non-RPM software (%d):", len(standaloneItems)))
		for _, s := range standaloneItems {
			lines = append(lines, fmt.Sprintf("#   %s at %s (%s)", s.Name, s.Path, s.Method))
		}
	}

	lines = append(lines, "")
	return lines
}

// --- Users section ---

func usersSectionLines(snap *schema.InspectionSnapshot) []string {
	var lines []string
	if snap.UsersGroups == nil {
		return lines
	}

	var includedUsers []map[string]interface{}
	for _, u := range snap.UsersGroups.Users {
		if include, ok := u["include"]; ok {
			if b, ok := include.(bool); ok && b {
				includedUsers = append(includedUsers, u)
			}
		} else {
			// Default to included if no "include" field
			includedUsers = append(includedUsers, u)
		}
	}

	if len(includedUsers) == 0 {
		return lines
	}

	lines = append(lines, "# === Users and Groups ===")

	// Group by strategy
	var sysusersUsers, useraddUsers, blueprintUsers, kickstartUsers []map[string]interface{}
	for _, u := range includedUsers {
		strategy, _ := u["strategy"].(string)
		switch strategy {
		case "sysusers":
			sysusersUsers = append(sysusersUsers, u)
		case "useradd":
			useraddUsers = append(useraddUsers, u)
		case "blueprint":
			blueprintUsers = append(blueprintUsers, u)
		case "kickstart":
			kickstartUsers = append(kickstartUsers, u)
		default:
			useraddUsers = append(useraddUsers, u)
		}
	}

	if len(sysusersUsers) > 0 {
		lines = append(lines, fmt.Sprintf("# systemd-sysusers entries (%d):", len(sysusersUsers)))
		lines = append(lines, "# These are system users created via sysusers.d drop-ins in config/.")
	}

	if len(useraddUsers) > 0 {
		lines = append(lines, fmt.Sprintf("# useradd users (%d):", len(useraddUsers)))
		for _, u := range useraddUsers {
			name, _ := u["name"].(string)
			uid, _ := u["uid"].(float64)
			if name != "" && sanitizeShellValue(name, "useradd") != nil {
				if uid > 0 {
					lines = append(lines, fmt.Sprintf("RUN useradd -u %d %s", int(uid), name))
				} else {
					lines = append(lines, fmt.Sprintf("RUN useradd %s", name))
				}
			}
		}
	}

	if len(blueprintUsers) > 0 {
		lines = append(lines, fmt.Sprintf("# FIXME: %d user(s) with blueprint strategy — provision via image builder blueprint", len(blueprintUsers)))
	}
	if len(kickstartUsers) > 0 {
		lines = append(lines, fmt.Sprintf("# FIXME: %d user(s) with kickstart strategy — see kickstart.ks", len(kickstartUsers)))
	}

	lines = append(lines, "")
	return lines
}

// --- Kernel/Boot section ---

func kernelBootSectionLines(snap *schema.InspectionSnapshot) []string {
	var lines []string

	kb := snap.KernelBoot
	if kb == nil {
		return lines
	}

	hasContent := kb.Cmdline != "" || len(kb.ModulesLoadD) > 0 || len(kb.ModprobeD) > 0 ||
		len(kb.DracutConf) > 0 || len(kb.SysctlOverrides) > 0 || len(kb.NonDefaultModules) > 0 ||
		kb.TunedActive != "" || len(kb.TunedCustomProfiles) > 0
	if !hasContent {
		return lines
	}

	lines = append(lines, "# === Kernel and Boot Configuration ===")

	// Kernel arguments
	safeKargs := operatorKargs(kb.Cmdline)
	if len(safeKargs) > 0 {
		lines = append(lines, "# === Kernel Arguments (bootc-native kargs.d) ===")
		lines = append(lines, "# These are applied at install and honored across image upgrades. See bootc documentation:")
		lines = append(lines, "# https://containers.github.io/bootc/building/kernel-arguments.html")
		lines = append(lines, "RUN mkdir -p /usr/lib/bootc/kargs.d")
		lines = append(lines, "COPY config/usr/lib/bootc/kargs.d/inspectah-migrated.toml /usr/lib/bootc/kargs.d/")
	}

	// Non-default modules
	var includedMods []schema.KernelModule
	for _, m := range kb.NonDefaultModules {
		if m.Include {
			includedMods = append(includedMods, m)
		}
	}
	if len(includedMods) > 0 {
		lines = append(lines, fmt.Sprintf("# %d non-default kernel module(s) — config files in COPY config/etc/ above", len(includedMods)))
	}

	// Sysctl overrides
	var includedSysctl []schema.SysctlOverride
	for _, s := range kb.SysctlOverrides {
		if s.Include {
			includedSysctl = append(includedSysctl, s)
		}
	}
	if len(includedSysctl) > 0 {
		lines = append(lines, fmt.Sprintf("# %d sysctl override(s) — config files in COPY config/etc/ above", len(includedSysctl)))
	}

	// Tuned
	if kb.TunedActive != "" {
		if tunedProfileRe.MatchString(kb.TunedActive) {
			lines = append(lines, fmt.Sprintf("# Tuned profile: %s", kb.TunedActive))
			lines = append(lines, fmt.Sprintf(`RUN echo "%s" > /etc/tuned/active_profile`, kb.TunedActive))
			lines = append(lines, `RUN echo "manual" > /etc/tuned/profile_mode`)
			lines = append(lines, "RUN systemctl enable tuned.service")
		} else {
			lines = append(lines, fmt.Sprintf("# FIXME: tuned profile name contains unsafe characters: %q", kb.TunedActive))
		}
	}

	lines = append(lines, "")
	return lines
}

// --- SELinux section ---

func selinuxSectionLines(snap *schema.InspectionSnapshot) []string {
	var lines []string
	if snap.Selinux == nil {
		return lines
	}

	hasContent := len(snap.Selinux.CustomModules) > 0 || len(snap.Selinux.BooleanOverrides) > 0 ||
		len(snap.Selinux.FcontextRules) > 0 || len(snap.Selinux.AuditRules) > 0 ||
		snap.Selinux.FipsMode || len(snap.Selinux.PortLabels) > 0
	if !hasContent {
		return lines
	}

	lines = append(lines, "# === SELinux Customizations ===")

	if len(snap.Selinux.CustomModules) > 0 {
		lines = append(lines, fmt.Sprintf("# FIXME: %d custom policy module(s) detected — "+
			"export .pp files to config/selinux/ and uncomment the COPY + semodule lines below", len(snap.Selinux.CustomModules)))
		lines = append(lines, "# COPY config/selinux/ /tmp/selinux/")
		lines = append(lines, "# RUN semodule -i /tmp/selinux/*.pp && rm -rf /tmp/selinux/")
	}

	// Non-default booleans
	var nonDefault []map[string]interface{}
	for _, b := range snap.Selinux.BooleanOverrides {
		// Check dynamic include key (same pattern as users)
		if inc, ok := b["include"]; ok {
			if incBool, ok := inc.(bool); ok && !incBool {
				continue
			}
		}
		if nd, ok := b["non_default"]; ok {
			if ndBool, ok := nd.(bool); ok && ndBool {
				nonDefault = append(nonDefault, b)
			}
		}
	}
	if len(nonDefault) > 0 {
		lines = append(lines, fmt.Sprintf("# FIXME: %d non-default boolean(s) detected — verify each is still needed", len(nonDefault)))
		for _, b := range nonDefault {
			bname, _ := b["name"].(string)
			bval, _ := b["current_value"].(string)
			if bname == "" {
				continue
			}
			if sanitizeShellValue(bname, "setsebool name") != nil &&
				sanitizeShellValue(bval, "setsebool value") != nil {
				lines = append(lines, fmt.Sprintf("RUN setsebool -P %s %s", bname, bval))
			} else {
				lines = append(lines, fmt.Sprintf("# FIXME: boolean name/value contains unsafe characters, skipped: %q=%q", bname, bval))
			}
		}
	}

	if len(snap.Selinux.FcontextRules) > 0 {
		lines = append(lines, fmt.Sprintf("# FIXME: %d custom fcontext rule(s) detected — apply in image", len(snap.Selinux.FcontextRules)))
		limit := len(snap.Selinux.FcontextRules)
		if limit > 10 {
			limit = 10
		}
		for _, fc := range snap.Selinux.FcontextRules[:limit] {
			if sanitizeShellValue(fc, "semanage fcontext") != nil {
				lines = append(lines, fmt.Sprintf("# RUN semanage fcontext -a %s", fc))
			} else {
				lines = append(lines, fmt.Sprintf("# FIXME: fcontext rule contains unsafe characters: %q", fc))
			}
		}
		lines = append(lines, "# RUN restorecon -Rv /  # apply fcontext changes after all COPYs")
	}

	if len(snap.Selinux.AuditRules) > 0 {
		lines = append(lines, fmt.Sprintf("# %d audit rule file(s) — included in COPY config/etc/ above", len(snap.Selinux.AuditRules)))
	}

	if len(snap.Selinux.PortLabels) > 0 {
		lines = append(lines, fmt.Sprintf("# %d custom SELinux port label(s) detected", len(snap.Selinux.PortLabels)))
		for _, pl := range snap.Selinux.PortLabels {
			proto := sanitizeShellValue(pl.Protocol, "semanage port protocol")
			port := sanitizeShellValue(pl.Port, "semanage port number")
			ptype := sanitizeShellValue(pl.Type, "semanage port type")
			if proto != nil && port != nil && ptype != nil {
				lines = append(lines, fmt.Sprintf("RUN semanage port -a -t %s -p %s %s", *ptype, *proto, *port))
			} else {
				lines = append(lines, fmt.Sprintf("# FIXME: port label contains unsafe characters, skipped: %q %q %q", pl.Type, pl.Protocol, pl.Port))
			}
		}
	}

	if snap.Selinux.FipsMode {
		lines = append(lines, "# FIXME: host has FIPS mode enabled — enable FIPS in the bootc image via fips-mode-setup")
	}

	lines = append(lines, "")
	return lines
}
