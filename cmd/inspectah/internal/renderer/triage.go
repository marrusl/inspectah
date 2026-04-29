package renderer

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// TriageItem represents a classified snapshot item for the SPA.
type TriageItem struct {
	Section    string `json:"section"`
	Key        string `json:"key"`
	Tier       int    `json:"tier"`
	Reason     string `json:"reason"`
	Name       string `json:"name"`
	Meta       string `json:"meta"`
	IsSecret   bool   `json:"is_secret,omitempty"`
	SourcePath string `json:"source_path,omitempty"`
}

// ClassifySnapshot classifies all triageable items in the snapshot.
// Returns a manifest sorted by section, then tier (3->2->1).
func ClassifySnapshot(snap *schema.InspectionSnapshot) []TriageItem {
	secretPaths := buildSecretPathSet(snap)

	var items []TriageItem
	items = append(items, classifyPackages(snap, secretPaths)...)
	items = append(items, classifyConfigFiles(snap, secretPaths)...)
	items = append(items, classifyRuntime(snap, secretPaths)...)
	items = append(items, classifyContainerItems(snap, secretPaths)...)
	items = append(items, classifyIdentity(snap, secretPaths)...)
	items = append(items, classifySystemItems(snap, secretPaths)...)
	items = append(items, classifySecretItems(snap, secretPaths)...)
	return items
}

func isIncluded(b *bool) bool {
	return b == nil || *b
}

func boolPtr(v bool) *bool {
	return &v
}

func buildSecretPathSet(snap *schema.InspectionSnapshot) map[string]bool {
	paths := make(map[string]bool)
	for _, r := range snap.Redactions {
		var finding struct {
			Path string `json:"path"`
			Name string `json:"name"`
		}
		if json.Unmarshal(r, &finding) == nil {
			if finding.Path != "" {
				paths[finding.Path] = true
			}
			if finding.Name != "" {
				paths[finding.Name] = true
			}
		}
	}
	return paths
}

func classifyPackages(snap *schema.InspectionSnapshot, secrets map[string]bool) []TriageItem {
	if snap.Rpm == nil {
		return nil
	}
	var items []TriageItem
	baselineNames := make(map[string]bool)
	if snap.Rpm.BaselinePackageNames != nil {
		for _, n := range *snap.Rpm.BaselinePackageNames {
			baselineNames[n] = true
		}
	}

	for _, pkg := range snap.Rpm.PackagesAdded {
		if secrets[pkg.Name] {
			continue
		}
		tier, reason := classifyPackage(pkg, baselineNames)
		items = append(items, TriageItem{
			Section: "packages",
			Key:     fmt.Sprintf("pkg-%s-%s", pkg.Name, pkg.Arch),
			Tier:    tier,
			Reason:  reason,
			Name:    pkg.Name,
			Meta:    joinNonEmpty(" | ", pkg.Version+"-"+pkg.Release, pkg.Arch, pkg.SourceRepo),
		})
	}

	for _, ms := range snap.Rpm.ModuleStreams {
		if ms.BaselineMatch {
			continue
		}
		items = append(items, TriageItem{
			Section: "packages",
			Key:     fmt.Sprintf("ms-%s-%s", ms.ModuleName, ms.Stream),
			Tier:    2,
			Reason:  "Module stream package. Verify compatibility.",
			Name:    ms.ModuleName + ":" + ms.Stream,
			Meta:    strings.Join(ms.Profiles, ", "),
		})
	}
	return items
}

func classifyPackage(pkg schema.PackageEntry, baseline map[string]bool) (int, string) {
	state := string(pkg.State)
	repo := strings.ToLower(pkg.SourceRepo)

	if state == "local_install" || state == "no_repo" {
		return 3, "Package installed locally (no repository). Verify provenance."
	}
	if baseline[pkg.Name] {
		return 1, "Standard package matching base image."
	}
	if isThirdPartyRepo(repo) {
		return 2, fmt.Sprintf("Third-party repository (%s). Not in base image.", pkg.SourceRepo)
	}
	return 2, "Package from standard repo, not in base image."
}

func isThirdPartyRepo(repo string) bool {
	standard := []string{"baseos", "appstream", "rhel", "fedora"}
	if repo == "" {
		return false
	}
	lower := strings.ToLower(repo)
	for _, s := range standard {
		if strings.Contains(lower, s) {
			return false
		}
	}
	return true
}

func classifyConfigFiles(snap *schema.InspectionSnapshot, secrets map[string]bool) []TriageItem {
	if snap.Config == nil {
		return nil
	}
	var items []TriageItem
	for _, f := range snap.Config.Files {
		if secrets[f.Path] {
			continue
		}
		if isQuadletPath(f.Path) {
			continue
		}
		tier, reason := classifyConfigFile(f)
		items = append(items, TriageItem{
			Section: "config",
			Key:     "cfg-" + f.Path,
			Tier:    tier,
			Reason:  reason,
			Name:    f.Path,
			Meta:    joinNonEmpty(" | ", string(f.Kind), string(f.Category)),
		})
	}
	return items
}

func classifyConfigFile(f schema.ConfigFileEntry) (int, string) {
	switch f.Kind {
	case schema.ConfigFileKindRpmOwnedDefault, "baseline_match":
		return 1, "Config file matches base image content."
	case schema.ConfigFileKindRpmOwnedModified:
		return 2, "Config file modified from RPM default."
	case "systemd_dropin":
		return 2, "Systemd drop-in override file."
	default:
		return 2, "Config file not in base image."
	}
}

func isQuadletPath(path string) bool {
	exts := []string{".container", ".volume", ".network", ".kube"}
	for _, ext := range exts {
		if strings.HasSuffix(path, ext) && strings.Contains(path, "/containers/") {
			return true
		}
	}
	return false
}

func classifyRuntime(snap *schema.InspectionSnapshot, secrets map[string]bool) []TriageItem {
	var items []TriageItem
	if snap.Services != nil {
		for _, svc := range snap.Services.StateChanges {
			if secrets[svc.Unit] {
				continue
			}
			isDefault := svc.CurrentState == svc.DefaultState
			tier := 2
			reason := fmt.Sprintf("Service state changed (%s -> %s).", svc.DefaultState, svc.CurrentState)
			if isDefault {
				tier = 1
				reason = "Service in default state."
			}
			meta := svc.CurrentState
			if svc.OwningPackage != nil {
				meta += " | " + *svc.OwningPackage
			}
			items = append(items, TriageItem{
				Section: "runtime", Key: "svc-" + svc.Unit,
				Tier: tier, Reason: reason, Name: svc.Unit, Meta: meta,
			})
		}
	}
	if snap.ScheduledTasks != nil {
		for _, job := range snap.ScheduledTasks.CronJobs {
			items = append(items, TriageItem{
				Section: "runtime", Key: "cron-" + job.Path,
				Tier: 2, Reason: "Scheduled cron job.",
				Name: job.Path, Meta: job.Source,
			})
		}
		for _, timer := range snap.ScheduledTasks.SystemdTimers {
			items = append(items, TriageItem{
				Section: "runtime", Key: "timer-" + timer.Name,
				Tier: 2, Reason: "Systemd timer unit.",
				Name: timer.Name, Meta: timer.OnCalendar,
			})
		}
	}
	return items
}

func classifyContainerItems(snap *schema.InspectionSnapshot, secrets map[string]bool) []TriageItem {
	var items []TriageItem
	if snap.Containers != nil {
		quadletNames := make(map[string]bool)
		for _, q := range snap.Containers.QuadletUnits {
			quadletNames[q.Name] = true
			items = append(items, TriageItem{
				Section: "containers", Key: "quadlet-" + q.Name,
				Tier: 2, Reason: "Quadlet file with container unit.",
				Name: q.Name, Meta: q.Image,
			})
		}
		for _, c := range snap.Containers.RunningContainers {
			tier, reason := 2, "Running container with quadlet backing."
			if !quadletNames[c.Name] {
				tier, reason = 3, "Running container without quadlet. May not survive reboot."
			}
			items = append(items, TriageItem{
				Section: "containers", Key: "container-" + c.Name,
				Tier: tier, Reason: reason, Name: c.Name, Meta: c.Image,
			})
		}
	}
	if snap.NonRpmSoftware != nil {
		for _, item := range snap.NonRpmSoftware.Items {
			if secrets[item.Path] {
				continue
			}
			name := item.Path
			if name == "" {
				name = item.Name
			}
			items = append(items, TriageItem{
				Section: "containers", Key: "nonrpm-" + name,
				Tier: 3, Reason: "Non-RPM binary with unclear provenance.",
				Name: name, Meta: item.Method,
			})
		}
	}
	return items
}

func classifyIdentity(snap *schema.InspectionSnapshot, secrets map[string]bool) []TriageItem {
	var items []TriageItem
	if snap.UsersGroups != nil {
		for _, u := range snap.UsersGroups.Users {
			name, _ := u["name"].(string)
			uid, _ := u["uid"].(float64)
			isSystem := uid < 1000
			tier, reason := 2, "User-created account (UID >= 1000)."
			if isSystem {
				tier, reason = 1, "System user (UID < 1000), matches base."
			}
			items = append(items, TriageItem{
				Section: "identity", Key: "user-" + name,
				Tier: tier, Reason: reason, Name: name,
				Meta: fmt.Sprintf("UID %.0f", uid),
			})
		}
		for _, g := range snap.UsersGroups.Groups {
			name, _ := g["name"].(string)
			gid, _ := g["gid"].(float64)
			isSystem := gid < 1000
			tier, reason := 2, "User-created group."
			if isSystem {
				tier, reason = 1, "System group (GID < 1000)."
			}
			items = append(items, TriageItem{
				Section: "identity", Key: "group-" + name,
				Tier: tier, Reason: reason, Name: name,
				Meta: fmt.Sprintf("GID %.0f", gid),
			})
		}
	}
	if snap.Selinux != nil {
		for _, b := range snap.Selinux.BooleanOverrides {
			name, _ := b["name"].(string)
			val, _ := b["current_value"].(string)
			items = append(items, TriageItem{
				Section: "identity", Key: "sebool-" + name,
				Tier: 2, Reason: "SELinux boolean changed from default.",
				Name: name, Meta: val,
			})
		}
		for _, m := range snap.Selinux.CustomModules {
			items = append(items, TriageItem{
				Section: "identity", Key: "semod-" + m,
				Tier: 3, Reason: "Custom SELinux policy module.",
				Name: m,
			})
		}
		for _, p := range snap.Selinux.PortLabels {
			items = append(items, TriageItem{
				Section: "identity", Key: fmt.Sprintf("seport-%s-%s", p.Protocol, p.Port),
				Tier: 2, Reason: "Custom SELinux port label.",
				Name: fmt.Sprintf("%s/%s -> %s", p.Protocol, p.Port, p.Type),
			})
		}
	}
	return items
}

func classifySystemItems(snap *schema.InspectionSnapshot, secrets map[string]bool) []TriageItem {
	var items []TriageItem
	if snap.KernelBoot != nil {
		for _, s := range snap.KernelBoot.SysctlOverrides {
			items = append(items, TriageItem{
				Section: "system", Key: "sysctl-" + s.Key,
				Tier: 2, Reason: "Custom sysctl parameter.",
				Name: s.Key, Meta: s.Runtime,
			})
		}
		for _, m := range snap.KernelBoot.NonDefaultModules {
			items = append(items, TriageItem{
				Section: "system", Key: "kmod-" + m.Name,
				Tier: 2, Reason: "Kernel module loaded.",
				Name: m.Name, Meta: m.UsedBy,
			})
		}
	}
	if snap.Network != nil {
		for _, conn := range snap.Network.Connections {
			items = append(items, TriageItem{
				Section: "system", Key: "conn-" + conn.Name,
				Tier: 2, Reason: "Network connection configuration.",
				Name: conn.Name, Meta: conn.Type,
			})
		}
		for _, zone := range snap.Network.FirewallZones {
			items = append(items, TriageItem{
				Section: "system", Key: "fw-" + zone.Name,
				Tier: 2, Reason: "Custom firewall zone.",
				Name: zone.Name,
			})
		}
	}
	if snap.Storage != nil {
		for _, entry := range snap.Storage.FstabEntries {
			items = append(items, TriageItem{
				Section: "system", Key: "fstab-" + entry.MountPoint,
				Tier: 2, Reason: "Non-default mount point.",
				Name: entry.MountPoint, Meta: entry.Fstype,
			})
		}
	}
	return items
}

func classifySecretItems(snap *schema.InspectionSnapshot, secrets map[string]bool) []TriageItem {
	// Build a set of config file paths for source_path linking
	configPaths := make(map[string]bool)
	if snap.Config != nil {
		for _, f := range snap.Config.Files {
			configPaths[f.Path] = true
		}
	}

	var items []TriageItem
	for i, r := range snap.Redactions {
		var finding struct {
			Path        string `json:"path"`
			Name        string `json:"name"`
			FindingType string `json:"finding_type"`
			Type        string `json:"type"`
		}
		json.Unmarshal(r, &finding)
		name := finding.Path
		if name == "" {
			name = finding.Name
		}
		if name == "" {
			name = fmt.Sprintf("Redaction %d", i+1)
		}
		ftype := finding.FindingType
		if ftype == "" {
			ftype = finding.Type
		}

		// Determine if this secret is backed by a config file.
		// If the redaction's path matches a config file path, set SourcePath
		// so the SPA can toggle the config entry's Include field.
		var sourcePath string
		if finding.Path != "" && configPaths[finding.Path] {
			sourcePath = finding.Path
		}

		items = append(items, TriageItem{
			Section:    "secrets",
			Key:        fmt.Sprintf("secret-%d", i),
			Tier:       3,
			Reason:     "Secret or credential detected: " + ftype,
			Name:       name,
			Meta:       ftype,
			IsSecret:   true,
			SourcePath: sourcePath,
		})
	}
	return items
}

func joinNonEmpty(sep string, parts ...string) string {
	var filtered []string
	for _, p := range parts {
		if p != "" {
			filtered = append(filtered, p)
		}
	}
	return strings.Join(filtered, sep)
}
