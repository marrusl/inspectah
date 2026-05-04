package renderer

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// TriageItem represents a classified snapshot item for the SPA.
type TriageItem struct {
	Section        string `json:"section"`
	Key            string `json:"key"`
	Tier           int    `json:"tier"`
	Reason         string `json:"reason"`
	Name           string `json:"name"`
	Meta           string `json:"meta"`
	Group          string `json:"group,omitempty"`
	CardType       string `json:"card_type,omitempty"`
	DisplayOnly    bool   `json:"display_only,omitempty"`
	Acknowledged   bool   `json:"acknowledged,omitempty"`
	IsSecret       bool   `json:"is_secret,omitempty"`
	SourcePath     string `json:"source_path,omitempty"`
	DefaultInclude bool   `json:"default_include"`
}

// ClassifySnapshot classifies all triageable items in the snapshot.
// Returns a manifest sorted by section, then tier (3->2->1).
//
// If original is non-nil, DefaultInclude is set from the original
// snapshot's include values. This lets the SPA distinguish "user
// excluded this" from "this started as excluded (e.g. fleet merge
// below-threshold)."
func ClassifySnapshot(snap *schema.InspectionSnapshot, original *schema.InspectionSnapshot) []TriageItem {
	isFleet := isFleetSnapshot(snap)
	items := classifyAll(snap, isFleet)

	if original != nil {
		origItems := classifyAll(original, isFleet)
		origMap := make(map[string]bool)
		for _, oi := range origItems {
			origMap[oi.Key] = oi.DefaultInclude
		}
		for i := range items {
			if val, ok := origMap[items[i].Key]; ok {
				items[i].DefaultInclude = val
			}
			// Items not in original keep their current DefaultInclude (new items)
		}
	}

	return items
}

// classifyAll runs all classifiers against the snapshot and returns
// items with DefaultInclude set to each item's current include value.
func classifyAll(snap *schema.InspectionSnapshot, isFleet bool) []TriageItem {
	secretPaths := buildSecretPathSet(snap)

	var items []TriageItem
	items = append(items, classifyPackages(snap, secretPaths, isFleet)...)
	items = append(items, classifyConfigFiles(snap, secretPaths, isFleet)...)
	items = append(items, classifyRuntime(snap, secretPaths, isFleet)...)
	items = append(items, classifyContainerItems(snap, secretPaths, isFleet)...)
	items = append(items, classifyIdentity(snap, secretPaths, isFleet)...)
	items = append(items, classifySystemItems(snap, secretPaths, isFleet)...)
	items = append(items, classifySecretItems(snap, secretPaths)...)
	return items
}

func isIncluded(b *bool) bool {
	return b == nil || *b
}

func boolPtr(v bool) *bool {
	return &v
}

// mapInclude reads the "include" key from an untyped map, defaulting
// to true when the key is absent or not a bool.
func mapInclude(m map[string]interface{}) bool {
	v, ok := m["include"]
	if !ok {
		return true
	}
	b, ok := v.(bool)
	if !ok {
		return true
	}
	return b
}

func isFleetSnapshot(snap *schema.InspectionSnapshot) bool {
	if snap.Meta == nil {
		return false
	}
	_, ok := snap.Meta["fleet"]
	return ok
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

func classifyPackages(snap *schema.InspectionSnapshot, secrets map[string]bool, isFleet bool) []TriageItem {
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
		item := TriageItem{
			Section:        "packages",
			Key:            fmt.Sprintf("pkg-%s-%s", pkg.Name, pkg.Arch),
			Tier:           tier,
			Reason:         reason,
			Name:           pkg.Name,
			Meta:           joinNonEmpty(" | ", pkg.Version+"-"+pkg.Release, pkg.Arch, pkg.SourceRepo),
			DefaultInclude: pkg.Include,
		}

		if !isFleet {
			if tier == 3 && (pkg.State == schema.PackageStateLocalInstall || pkg.State == schema.PackageStateNoRepo) {
				item.CardType = "notification"
				item.Acknowledged = pkg.Acknowledged
				item.Reason = "No repository source available. inspectah cannot reconstruct installation steps for this package."
			} else if pkg.SourceRepo != "" {
				item.Group = "repo:" + strings.ToLower(pkg.SourceRepo)
			}
		}

		items = append(items, item)
	}

	for _, ms := range snap.Rpm.ModuleStreams {
		if ms.BaselineMatch {
			continue
		}
		items = append(items, TriageItem{
			Section:        "packages",
			Key:            fmt.Sprintf("ms-%s-%s", ms.ModuleName, ms.Stream),
			Tier:           2,
			Reason:         "Module stream package. Verify compatibility.",
			Name:           ms.ModuleName + ":" + ms.Stream,
			Meta:           strings.Join(ms.Profiles, ", "),
			DefaultInclude: ms.Include,
		})
	}
	return items
}

func classifyPackage(pkg schema.PackageEntry, baseline map[string]bool) (int, string) {
	repo := strings.ToLower(pkg.SourceRepo)

	if pkg.State == schema.PackageStateLocalInstall || pkg.State == schema.PackageStateNoRepo {
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

func classifyConfigFiles(snap *schema.InspectionSnapshot, secrets map[string]bool, isFleet bool) []TriageItem {
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
		item := TriageItem{
			Section:        "config",
			Key:            "cfg-" + f.Path,
			Tier:           tier,
			Reason:         reason,
			Name:           f.Path,
			Meta:           joinNonEmpty(" | ", string(f.Kind), string(f.Category)),
			DefaultInclude: f.Include,
		}

		if !isFleet {
			switch f.Kind {
			case schema.ConfigFileKindRpmOwnedDefault, "baseline_match":
				item.Group = "kind:unchanged"
			case "systemd_dropin":
				item.Group = "kind:drop-in"
			// RPM-owned-modified and custom/untracked: no group (individual cards)
			}
		}

		items = append(items, item)
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

var imageModeIncompatibleServices = map[string]bool{
	"dnf-makecache.service": true,
	"dnf-makecache.timer":   true,
	"packagekit.service":    true,
}

var riskyMountPrefixes = []string{"/", "/boot", "/var", "/sysroot", "/usr", "/etc"}

func isRiskyMount(mountPoint string) bool {
	for _, prefix := range riskyMountPrefixes {
		if mountPoint == prefix {
			return true
		}
		if prefix != "/" && strings.HasPrefix(mountPoint, prefix+"/") {
			return true
		}
	}
	return false
}

func isUnstableDevicePath(device string) bool {
	return strings.HasPrefix(device, "/dev/sd") || strings.HasPrefix(device, "/dev/hd")
}

func classifyRuntime(snap *schema.InspectionSnapshot, secrets map[string]bool, isFleet bool) []TriageItem {
	var items []TriageItem
	if snap.Services != nil {
		for _, svc := range snap.Services.StateChanges {
			if secrets[svc.Unit] {
				continue
			}

			// Check for image-mode incompatible services in single-machine mode
			if !isFleet && imageModeIncompatibleServices[svc.Unit] {
				items = append(items, TriageItem{
					Section: "runtime", Key: "svc-" + svc.Unit,
					Tier: 3, Reason: "This service assumes package management at runtime, which is unavailable in image mode. Consider disabling or removing it from the image.",
					Name: svc.Unit, Meta: svc.CurrentState,
					DefaultInclude: svc.Include,
				})
				continue
			}

			isDefault := svc.CurrentState == svc.DefaultState
			tier := 2
			reason := fmt.Sprintf("Service state changed (%s -> %s).", svc.DefaultState, svc.CurrentState)
			group := ""
			if isDefault {
				tier = 1
				reason = "Service in default state."
				if !isFleet {
					group = "sub:services-default"
				}
			} else if !isFleet {
				group = "sub:services-changed"
			}
			meta := svc.CurrentState
			if svc.OwningPackage != nil {
				meta += " | " + *svc.OwningPackage
			}
			items = append(items, TriageItem{
				Section: "runtime", Key: "svc-" + svc.Unit,
				Tier: tier, Reason: reason, Name: svc.Unit, Meta: meta,
				Group:          group,
				DefaultInclude: svc.Include,
			})
		}
	}
	if snap.ScheduledTasks != nil {
		for _, job := range snap.ScheduledTasks.CronJobs {
			group := ""
			if !isFleet {
				group = "sub:cron"
			}
			items = append(items, TriageItem{
				Section: "runtime", Key: "cron-" + job.Path,
				Tier: 2, Reason: "Scheduled cron job.",
				Name: job.Path, Meta: job.Source,
				Group:          group,
				DefaultInclude: job.Include,
			})
		}
		for _, timer := range snap.ScheduledTasks.SystemdTimers {
			group := ""
			if !isFleet {
				group = "sub:timers"
			}
			items = append(items, TriageItem{
				Section: "runtime", Key: "timer-" + timer.Name,
				Tier: 2, Reason: "Systemd timer unit.",
				Name: timer.Name, Meta: timer.OnCalendar,
				Group:          group,
				DefaultInclude: isIncluded(timer.Include),
			})
		}
	}
	return items
}

func classifyContainerItems(snap *schema.InspectionSnapshot, secrets map[string]bool, isFleet bool) []TriageItem {
	var items []TriageItem
	if snap.Containers != nil {
		quadletNames := make(map[string]bool)
		for _, q := range snap.Containers.QuadletUnits {
			quadletNames[q.Name] = true
			group := ""
			if !isFleet {
				group = "sub:quadlet"
			}
			items = append(items, TriageItem{
				Section: "containers", Key: "quadlet-" + q.Name,
				Tier: 2, Reason: "Quadlet file with container unit.",
				Name: q.Name, Meta: q.Image,
				Group:          group,
				DefaultInclude: q.Include,
			})
		}
		for _, c := range snap.Containers.RunningContainers {
			tier, reason := 2, "Running container with quadlet backing."
			if !quadletNames[c.Name] {
				tier = 3
				reason = "Running container without quadlet backing. This is runtime state — it will not be reproduced in the image. Consider converting to a Quadlet unit for image-mode compatibility."
			}
			item := TriageItem{
				Section: "containers", Key: "container-" + c.Name,
				Tier: tier, Reason: reason, Name: c.Name, Meta: c.Image,
				DefaultInclude: isIncluded(c.Include),
			}
			if !isFleet {
				item.DisplayOnly = true
				item.Acknowledged = c.Acknowledged
			}
			items = append(items, item)
		}
	}
	if snap.NonRpmSoftware != nil {
		for _, nri := range snap.NonRpmSoftware.Items {
			if secrets[nri.Path] {
				continue
			}
			name := nri.Path
			if name == "" {
				name = nri.Name
			}
			item := TriageItem{
				Section: "containers", Key: "nonrpm-" + name,
				Tier: 3, Reason: "Non-RPM binary with unclear provenance.",
				Name: name, Meta: nri.Method,
				DefaultInclude: nri.Include,
			}
			if !isFleet && nri.Method == "binary" {
				item.CardType = "notification"
				item.Acknowledged = nri.Acknowledged
				item.Reason = "inspectah cannot determine the provenance or installation method for this binary. To include it in the image, provide a reproducible build-time source and add it to your Containerfile."
			}
			items = append(items, item)
		}
	}
	return items
}

func classifyIdentity(snap *schema.InspectionSnapshot, secrets map[string]bool, isFleet bool) []TriageItem {
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
				Meta:           fmt.Sprintf("UID %.0f", uid),
				DefaultInclude: mapInclude(u),
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
			item := TriageItem{
				Section: "identity", Key: "group-" + name,
				Tier: tier, Reason: reason, Name: name,
				Meta:           fmt.Sprintf("GID %.0f", gid),
				DefaultInclude: mapInclude(g),
			}
			if !isFleet {
				item.DisplayOnly = true
			}
			items = append(items, item)
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
				DefaultInclude: mapInclude(b),
			})
		}
		for _, m := range snap.Selinux.CustomModules {
			items = append(items, TriageItem{
				Section: "identity", Key: "semod-" + m,
				Tier: 3, Reason: "Custom SELinux policy module.",
				Name: m,
				DefaultInclude: true,
			})
		}
		for _, p := range snap.Selinux.PortLabels {
			items = append(items, TriageItem{
				Section: "identity", Key: fmt.Sprintf("seport-%s-%s", p.Protocol, p.Port),
				Tier: 2, Reason: "Custom SELinux port label.",
				Name:           fmt.Sprintf("%s/%s -> %s", p.Protocol, p.Port, p.Type),
				DefaultInclude: p.Include,
			})
		}
	}
	return items
}

func classifySystemItems(snap *schema.InspectionSnapshot, secrets map[string]bool, isFleet bool) []TriageItem {
	var items []TriageItem
	if snap.KernelBoot != nil {
		for _, s := range snap.KernelBoot.SysctlOverrides {
			group := ""
			if !isFleet {
				group = "sub:sysctl"
			}
			items = append(items, TriageItem{
				Section: "system", Key: "sysctl-" + s.Key,
				Tier: 2, Reason: "Custom sysctl parameter.",
				Name: s.Key, Meta: s.Runtime,
				Group:          group,
				DefaultInclude: s.Include,
			})
		}
		for _, m := range snap.KernelBoot.NonDefaultModules {
			group := ""
			if !isFleet {
				group = "sub:kmod"
			}
			items = append(items, TriageItem{
				Section: "system", Key: "kmod-" + m.Name,
				Tier: 2, Reason: "Kernel module loaded.",
				Name: m.Name, Meta: m.UsedBy,
				Group:          group,
				DefaultInclude: m.Include,
			})
		}
	}
	if snap.Network != nil {
		for _, conn := range snap.Network.Connections {
			item := TriageItem{
				Section: "system", Key: "conn-" + conn.Name,
				Tier: 2, Reason: "Network connection configuration.",
				Name: conn.Name, Meta: conn.Type,
				DefaultInclude: isIncluded(conn.Include),
			}
			if !isFleet {
				item.Group = "sub:network"
				item.DisplayOnly = true
				item.Acknowledged = conn.Acknowledged
			}
			items = append(items, item)
		}
		for _, zone := range snap.Network.FirewallZones {
			group := ""
			if !isFleet {
				group = "sub:firewall"
			}
			items = append(items, TriageItem{
				Section: "system", Key: "fw-" + zone.Name,
				Tier: 2, Reason: "Custom firewall zone.",
				Name:           zone.Name,
				Group:          group,
				DefaultInclude: zone.Include,
			})
		}
	}
	if snap.Storage != nil {
		for _, entry := range snap.Storage.FstabEntries {
			item := TriageItem{
				Section: "system", Key: "fstab-" + entry.MountPoint,
				Tier: 2, Reason: "Non-default mount point.",
				Name: entry.MountPoint, Meta: entry.Fstype,
				DefaultInclude: isIncluded(entry.Include),
			}
			// All fstab entries are display-only
			if !isFleet {
				item.DisplayOnly = true
				item.Acknowledged = entry.Acknowledged
				// Risky mounts and unstable device paths get individual cards
				if !isRiskyMount(entry.MountPoint) && !isUnstableDevicePath(entry.Device) {
					item.Group = "sub:fstab"
				}
			}
			items = append(items, item)
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
			Section:        "secrets",
			Key:            fmt.Sprintf("secret-%d", i),
			Tier:           3,
			Reason:         "Secret or credential detected: " + ftype,
			Name:           name,
			Meta:           ftype,
			IsSecret:       true,
			SourcePath:     sourcePath,
			DefaultInclude: true,
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
