package fleet

import (
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// MergeSnapshots merges N snapshots into a single fleet snapshot with
// prevalence metadata. Items are deduplicated by identity (name/path)
// and annotated with fleet prevalence (count, total, hosts). Items
// below the min-prevalence threshold get include=false.
func MergeSnapshots(snapshots []*schema.InspectionSnapshot, minPrevalence int) (*schema.InspectionSnapshot, error) {
	if len(snapshots) < 2 {
		return nil, fmt.Errorf("need at least 2 snapshots, got %d", len(snapshots))
	}

	total := len(snapshots)
	setMergeContext(total, minPrevalence)
	fullHostnames := make([]string, total)
	for i, s := range snapshots {
		h, ok := s.Meta["hostname"].(string)
		if !ok {
			h = fmt.Sprintf("host-%d", i)
		}
		fullHostnames[i] = h
	}
	hostNames := AssignDisplayNames(snapshots)

	// --- RPM ---
	var rpmSection *schema.RpmSection
	if hasSection(snapshots, func(s *schema.InspectionSnapshot) bool { return s.Rpm != nil }) {
		packagesAdded := mergeIdentityItems(
			collectSectionLists(snapshots, func(s *schema.InspectionSnapshot) interface{} {
				if s.Rpm != nil {
					return s.Rpm.PackagesAdded
				}
				return nil
			}),
			func(item interface{}) string { return item.(schema.PackageEntry).Name },
			total, minPrevalence, hostNames,
		)

		baseImageOnly := mergeIdentityItems(
			collectSectionLists(snapshots, func(s *schema.InspectionSnapshot) interface{} {
				if s.Rpm != nil {
					return s.Rpm.BaseImageOnly
				}
				return nil
			}),
			func(item interface{}) string { return item.(schema.PackageEntry).Name },
			total, minPrevalence, hostNames,
		)

		repoFiles := mergeIdentityItems(
			collectSectionLists(snapshots, func(s *schema.InspectionSnapshot) interface{} {
				if s.Rpm != nil {
					return s.Rpm.RepoFiles
				}
				return nil
			}),
			func(item interface{}) string { return item.(schema.RepoFile).Path },
			total, minPrevalence, hostNames,
		)

		gpgKeys := mergeIdentityItems(
			collectSectionLists(snapshots, func(s *schema.InspectionSnapshot) interface{} {
				if s.Rpm != nil {
					return s.Rpm.GpgKeys
				}
				return nil
			}),
			func(item interface{}) string { return item.(schema.RepoFile).Path },
			total, minPrevalence, hostNames,
		)

		dnfRemoved := deduplicateStrings(collectStringLists(snapshots, func(s *schema.InspectionSnapshot) []string {
			if s.Rpm != nil {
				return s.Rpm.DnfHistoryRemoved
			}
			return nil
		}))

		moduleStreams := mergeModuleStreams(
			collectSectionLists(snapshots, func(s *schema.InspectionSnapshot) interface{} {
				if s.Rpm != nil {
					return s.Rpm.ModuleStreams
				}
				return nil
			}),
			total, minPrevalence, hostNames,
		)

		versionLocks := mergeContentItems(
			collectSectionLists(snapshots, func(s *schema.InspectionSnapshot) interface{} {
				if s.Rpm != nil {
					return s.Rpm.VersionLocks
				}
				return nil
			}),
			func(item interface{}) string {
				e := item.(schema.VersionLockEntry)
				return fmt.Sprintf("%s.%s", e.Name, e.Arch)
			},
			func(item interface{}) string {
				e := item.(schema.VersionLockEntry)
				return fmt.Sprintf("%d:%s-%s", e.Epoch, e.Version, e.Release)
			},
			total, minPrevalence, hostNames,
		)

		// Pass-through fields from first snapshot with rpm
		var firstRpm *schema.RpmSection
		for _, s := range snapshots {
			if s.Rpm != nil {
				firstRpm = s.Rpm
				break
			}
		}

		// Leaf/auto packages: union of optional string slices
		rawLeaf := make([]*[]string, total)
		rawAuto := make([]*[]string, total)
		rawDepTrees := make([]map[string]interface{}, total)
		for i, s := range snapshots {
			if s.Rpm != nil {
				rawLeaf[i] = s.Rpm.LeafPackages
				rawAuto[i] = s.Rpm.AutoPackages
				rawDepTrees[i] = s.Rpm.LeafDepTree
			}
		}

		rpmSection = &schema.RpmSection{
			PackagesAdded:     toPackageEntries(packagesAdded),
			BaseImageOnly:     toPackageEntries(baseImageOnly),
			RepoFiles:         toRepoFiles(repoFiles),
			GpgKeys:           toRepoFiles(gpgKeys),
			DnfHistoryRemoved: dnfRemoved,
			ModuleStreams:      moduleStreams,
			VersionLocks:      toVersionLockEntries(versionLocks),
			BaseImage:         firstRpm.BaseImage,
			BaselinePackageNames:   firstRpm.BaselinePackageNames,
			BaselineModuleStreams:   firstRpm.BaselineModuleStreams,
			NoBaseline:             firstRpm.NoBaseline,
			LeafPackages:     deduplicateOptionalStrings(rawLeaf),
			AutoPackages:     deduplicateOptionalStrings(rawAuto),
			LeafDepTree:      mergeDepTrees(rawDepTrees),
		}
	}

	// --- Config ---
	var configSection *schema.ConfigSection
	if hasSection(snapshots, func(s *schema.InspectionSnapshot) bool { return s.Config != nil }) {
		files := mergeContentItems(
			collectSectionLists(snapshots, func(s *schema.InspectionSnapshot) interface{} {
				if s.Config != nil {
					return s.Config.Files
				}
				return nil
			}),
			func(item interface{}) string { return item.(schema.ConfigFileEntry).Path },
			func(item interface{}) string {
				return contentHash(normalizeContent(item.(schema.ConfigFileEntry).Content))
			},
			total, minPrevalence, hostNames,
		)
		configFiles := toConfigFileEntries(files)
		autoSelectVariants(configFiles)
		configSection = &schema.ConfigSection{Files: configFiles}
	}

	// --- Services ---
	var servicesSection *schema.ServiceSection
	if hasSection(snapshots, func(s *schema.InspectionSnapshot) bool { return s.Services != nil }) {
		stateChanges := mergeIdentityItems(
			collectSectionLists(snapshots, func(s *schema.InspectionSnapshot) interface{} {
				if s.Services != nil {
					return s.Services.StateChanges
				}
				return nil
			}),
			func(item interface{}) string {
				sc := item.(schema.ServiceStateChange)
				return fmt.Sprintf("%s:%s", sc.Unit, sc.Action)
			},
			total, minPrevalence, hostNames,
		)

		dropIns := mergeContentItems(
			collectSectionLists(snapshots, func(s *schema.InspectionSnapshot) interface{} {
				if s.Services != nil {
					return s.Services.DropIns
				}
				return nil
			}),
			func(item interface{}) string { return item.(schema.SystemdDropIn).Path },
			func(item interface{}) string {
				return contentHash(normalizeContent(item.(schema.SystemdDropIn).Content))
			},
			total, minPrevalence, hostNames,
		)

		enabledUnits := deduplicateStrings(collectStringLists(snapshots, func(s *schema.InspectionSnapshot) []string {
			if s.Services != nil {
				return s.Services.EnabledUnits
			}
			return nil
		}))

		disabledUnits := deduplicateStrings(collectStringLists(snapshots, func(s *schema.InspectionSnapshot) []string {
			if s.Services != nil {
				return s.Services.DisabledUnits
			}
			return nil
		}))

		dropInEntries := toSystemdDropIns(dropIns)
		autoSelectVariants(dropInEntries)

		servicesSection = &schema.ServiceSection{
			StateChanges:  toServiceStateChanges(stateChanges),
			DropIns:       dropInEntries,
			EnabledUnits:  enabledUnits,
			DisabledUnits: disabledUnits,
		}
	}

	// --- Network ---
	var networkSection *schema.NetworkSection
	if hasSection(snapshots, func(s *schema.InspectionSnapshot) bool { return s.Network != nil }) {
		firewallZones := mergeIdentityItems(
			collectSectionLists(snapshots, func(s *schema.InspectionSnapshot) interface{} {
				if s.Network != nil {
					return s.Network.FirewallZones
				}
				return nil
			}),
			func(item interface{}) string { return item.(schema.FirewallZone).Name },
			total, minPrevalence, hostNames,
		)
		networkSection = &schema.NetworkSection{
			FirewallZones: toFirewallZones(firewallZones),
		}
	}

	// --- Scheduled Tasks ---
	var schedSection *schema.ScheduledTaskSection
	if hasSection(snapshots, func(s *schema.InspectionSnapshot) bool { return s.ScheduledTasks != nil }) {
		genTimers := mergeIdentityItems(
			collectSectionLists(snapshots, func(s *schema.InspectionSnapshot) interface{} {
				if s.ScheduledTasks != nil {
					return s.ScheduledTasks.GeneratedTimerUnits
				}
				return nil
			}),
			func(item interface{}) string { return item.(schema.GeneratedTimerUnit).Name },
			total, minPrevalence, hostNames,
		)
		cronJobs := mergeIdentityItems(
			collectSectionLists(snapshots, func(s *schema.InspectionSnapshot) interface{} {
				if s.ScheduledTasks != nil {
					return s.ScheduledTasks.CronJobs
				}
				return nil
			}),
			func(item interface{}) string { return item.(schema.CronJob).Path },
			total, minPrevalence, hostNames,
		)
		// Systemd timers: simple dedup by name
		timerSeen := make(map[string]schema.SystemdTimer)
		for _, s := range snapshots {
			if s.ScheduledTasks == nil {
				continue
			}
			for _, t := range s.ScheduledTasks.SystemdTimers {
				if _, ok := timerSeen[t.Name]; !ok {
					timerSeen[t.Name] = t
				}
			}
		}
		var timers []schema.SystemdTimer
		for _, t := range timerSeen {
			timers = append(timers, t)
		}
		sort.Slice(timers, func(i, j int) bool { return timers[i].Name < timers[j].Name })

		schedSection = &schema.ScheduledTaskSection{
			GeneratedTimerUnits: toGeneratedTimerUnits(genTimers),
			CronJobs:            toCronJobs(cronJobs),
			SystemdTimers:       timers,
		}
	}

	// --- Containers ---
	var containersSection *schema.ContainerSection
	if hasSection(snapshots, func(s *schema.InspectionSnapshot) bool { return s.Containers != nil }) {
		quadletUnits := mergeContentItems(
			collectSectionLists(snapshots, func(s *schema.InspectionSnapshot) interface{} {
				if s.Containers != nil {
					return s.Containers.QuadletUnits
				}
				return nil
			}),
			func(item interface{}) string { return item.(schema.QuadletUnit).Path },
			func(item interface{}) string {
				return contentHash(normalizeContent(item.(schema.QuadletUnit).Content))
			},
			total, minPrevalence, hostNames,
		)
		composeFiles := mergeContentItems(
			collectSectionLists(snapshots, func(s *schema.InspectionSnapshot) interface{} {
				if s.Containers != nil {
					return s.Containers.ComposeFiles
				}
				return nil
			}),
			func(item interface{}) string { return item.(schema.ComposeFile).Path },
			func(item interface{}) string {
				cf := item.(schema.ComposeFile)
				pairs := make([]string, len(cf.Images))
				for i, img := range cf.Images {
					pairs[i] = fmt.Sprintf("(%s, %s)", img.Service, img.Image)
				}
				sort.Strings(pairs)
				return contentHash(fmt.Sprintf("[%s]", strings.Join(pairs, ", ")))
			},
			total, minPrevalence, hostNames,
		)
		quadletEntries := toQuadletUnits(quadletUnits)
		autoSelectVariants(quadletEntries)
		composeEntries := toComposeFiles(composeFiles)
		autoSelectVariants(composeEntries)

		containersSection = &schema.ContainerSection{
			QuadletUnits: quadletEntries,
			ComposeFiles: composeEntries,
		}
	}

	// --- Non-RPM Software ---
	var nonRpmSection *schema.NonRpmSoftwareSection
	if hasSection(snapshots, func(s *schema.InspectionSnapshot) bool { return s.NonRpmSoftware != nil }) {
		nonRpmItems := mergeIdentityItems(
			collectSectionLists(snapshots, func(s *schema.InspectionSnapshot) interface{} {
				if s.NonRpmSoftware != nil {
					return s.NonRpmSoftware.Items
				}
				return nil
			}),
			func(item interface{}) string { return item.(schema.NonRpmItem).Path },
			total, minPrevalence, hostNames,
		)
		nonRpmEnvFiles := mergeContentItems(
			collectSectionLists(snapshots, func(s *schema.InspectionSnapshot) interface{} {
				if s.NonRpmSoftware != nil {
					return s.NonRpmSoftware.EnvFiles
				}
				return nil
			}),
			func(item interface{}) string { return item.(schema.ConfigFileEntry).Path },
			func(item interface{}) string {
				return contentHash(normalizeContent(item.(schema.ConfigFileEntry).Content))
			},
			total, minPrevalence, hostNames,
		)
		envFileEntries := toConfigFileEntries(nonRpmEnvFiles)
		autoSelectVariants(envFileEntries)
		nonRpmSection = &schema.NonRpmSoftwareSection{
			Items:    toNonRpmItems(nonRpmItems),
			EnvFiles: envFileEntries,
		}
	}

	// --- SELinux ---
	var selinuxSection *schema.SelinuxSection
	if hasSection(snapshots, func(s *schema.InspectionSnapshot) bool { return s.Selinux != nil }) {
		portLabels := mergeIdentityItems(
			collectSectionLists(snapshots, func(s *schema.InspectionSnapshot) interface{} {
				if s.Selinux != nil {
					return s.Selinux.PortLabels
				}
				return nil
			}),
			func(item interface{}) string {
				p := item.(schema.SelinuxPortLabel)
				return fmt.Sprintf("%s/%s", p.Protocol, p.Port)
			},
			total, minPrevalence, hostNames,
		)
		customModules := deduplicateStrings(collectStringLists(snapshots, func(s *schema.InspectionSnapshot) []string {
			if s.Selinux != nil {
				return s.Selinux.CustomModules
			}
			return nil
		}))
		fcontextRules := deduplicateStrings(collectStringLists(snapshots, func(s *schema.InspectionSnapshot) []string {
			if s.Selinux != nil {
				return s.Selinux.FcontextRules
			}
			return nil
		}))
		auditRules := deduplicateStrings(collectStringLists(snapshots, func(s *schema.InspectionSnapshot) []string {
			if s.Selinux != nil {
				return s.Selinux.AuditRules
			}
			return nil
		}))
		pamConfigs := deduplicateStrings(collectStringLists(snapshots, func(s *schema.InspectionSnapshot) []string {
			if s.Selinux != nil {
				return s.Selinux.PamConfigs
			}
			return nil
		}))
		booleanOverrides := deduplicateDicts(
			collectDictLists(snapshots, func(s *schema.InspectionSnapshot) []map[string]interface{} {
				if s.Selinux != nil {
					return s.Selinux.BooleanOverrides
				}
				return nil
			}),
			"name", total, hostNames,
		)

		var firstSE *schema.SelinuxSection
		for _, s := range snapshots {
			if s.Selinux != nil {
				firstSE = s.Selinux
				break
			}
		}
		selinuxSection = &schema.SelinuxSection{
			Mode:             firstSE.Mode,
			FipsMode:         firstSE.FipsMode,
			PortLabels:       toSelinuxPortLabels(portLabels),
			CustomModules:    customModules,
			FcontextRules:    fcontextRules,
			AuditRules:       auditRules,
			PamConfigs:       pamConfigs,
			BooleanOverrides: booleanOverrides,
		}
	}

	// --- Users/Groups ---
	var ugSection *schema.UserGroupSection
	if hasSection(snapshots, func(s *schema.InspectionSnapshot) bool { return s.UsersGroups != nil }) {
		users := deduplicateDicts(
			collectDictLists(snapshots, func(s *schema.InspectionSnapshot) []map[string]interface{} {
				if s.UsersGroups != nil {
					return s.UsersGroups.Users
				}
				return nil
			}),
			"name", total, hostNames,
		)
		groups := deduplicateDicts(
			collectDictLists(snapshots, func(s *schema.InspectionSnapshot) []map[string]interface{} {
				if s.UsersGroups != nil {
					return s.UsersGroups.Groups
				}
				return nil
			}),
			"name", total, hostNames,
		)
		sudoers := deduplicateStrings(collectStringLists(snapshots, func(s *schema.InspectionSnapshot) []string {
			if s.UsersGroups != nil {
				return s.UsersGroups.SudoersRules
			}
			return nil
		}))
		ugSection = &schema.UserGroupSection{
			Users:        users,
			Groups:       groups,
			SudoersRules: sudoers,
		}
	}

	// --- Kernel/Boot (first-snapshot pass-through) ---
	var kernelBootSection *schema.KernelBootSection
	for _, s := range snapshots {
		if s.KernelBoot != nil {
			kernelBootSection = s.KernelBoot
			break
		}
	}

	// --- Warnings / Redactions ---
	warningsMerged := deduplicateWarnings(snapshots)
	redactionsMerged := deduplicateRedactions(snapshots)

	// --- Fleet metadata ---
	sourceHostsIface := make([]interface{}, len(hostNames))
	for i, h := range hostNames {
		sourceHostsIface[i] = h
	}
	fleetMeta := map[string]interface{}{
		"source_hosts":   sourceHostsIface,
		"total_hosts":    float64(total),
		"min_prevalence": float64(minPrevalence),
		"host_title_map": makeHostTitleMap(hostNames, fullHostnames),
	}

	// --- Build merged snapshot ---
	first := snapshots[0]
	merged := &schema.InspectionSnapshot{
		SchemaVersion: first.SchemaVersion,
		Meta: map[string]interface{}{
			"hostname":  "fleet-merged",
			"timestamp": time.Now().UTC().Format(time.RFC3339),
			"fleet":     fleetMeta,
		},
		OsRelease:      first.OsRelease,
		SystemType:     first.SystemType,
		Rpm:            rpmSection,
		Config:         configSection,
		Services:       servicesSection,
		Network:        networkSection,
		ScheduledTasks: schedSection,
		Containers:     containersSection,
		NonRpmSoftware: nonRpmSection,
		Selinux:        selinuxSection,
		KernelBoot:     kernelBootSection,
		UsersGroups:    ugSection,
		Warnings:       warningsMerged,
		Redactions:     redactionsMerged,
	}

	return merged, nil
}

// ---------------------------------------------------------------------------
// Generic merge helpers
// ---------------------------------------------------------------------------

type mergedEntry struct {
	item  interface{}
	hosts []string
}

func hasSection(snapshots []*schema.InspectionSnapshot, pred func(*schema.InspectionSnapshot) bool) bool {
	for _, s := range snapshots {
		if pred(s) {
			return true
		}
	}
	return false
}

// collectSectionLists collects a slice field from each snapshot, returning
// the raw interface{} value (which should be a slice type or nil).
func collectSectionLists(snapshots []*schema.InspectionSnapshot, getter func(*schema.InspectionSnapshot) interface{}) []interface{} {
	result := make([]interface{}, len(snapshots))
	for i, s := range snapshots {
		result[i] = getter(s)
	}
	return result
}

func collectStringLists(snapshots []*schema.InspectionSnapshot, getter func(*schema.InspectionSnapshot) []string) [][]string {
	result := make([][]string, len(snapshots))
	for i, s := range snapshots {
		result[i] = getter(s)
	}
	return result
}

func collectDictLists(snapshots []*schema.InspectionSnapshot, getter func(*schema.InspectionSnapshot) []map[string]interface{}) [][]map[string]interface{} {
	result := make([][]map[string]interface{}, len(snapshots))
	for i, s := range snapshots {
		result[i] = getter(s)
	}
	return result
}

// prevalenceInclude returns true if count/total meets the minPrevalence
// threshold (percentage).
func prevalenceInclude(count, total, minPrevalence int) bool {
	return (count * 100) >= (minPrevalence * total)
}

// mergeIdentityItems merges items keyed by identity only. Items with the
// same key are deduplicated; the first-seen instance is kept, and
// prevalence is tracked across hosts.
func mergeIdentityItems(allItems []interface{}, keyFn func(interface{}) string, total, minPrevalence int, hostNames []string) []mergedEntry {
	seen := make(map[string]*mergedEntry)
	var order []string

	for snapIdx, raw := range allItems {
		if raw == nil {
			continue
		}
		items := sliceToInterfaces(raw)
		hostname := hostNames[snapIdx]
		for _, item := range items {
			k := keyFn(item)
			if entry, ok := seen[k]; ok {
				entry.hosts = append(entry.hosts, hostname)
			} else {
				seen[k] = &mergedEntry{item: item, hosts: []string{hostname}}
				order = append(order, k)
			}
		}
	}

	var result []mergedEntry
	for _, k := range order {
		entry := seen[k]
		result = append(result, mergedEntry{item: entry.item, hosts: entry.hosts})
	}
	return result
}

// mergeContentItems merges items keyed by both identity and content variant.
// Items with the same identity but different content are kept as separate
// entries (variants).
func mergeContentItems(allItems []interface{}, identityFn, variantFn func(interface{}) string, total, minPrevalence int, hostNames []string) []mergedEntry {
	type compositeKey struct {
		identity string
		variant  string
	}
	seen := make(map[compositeKey]*mergedEntry)
	var order []compositeKey

	for snapIdx, raw := range allItems {
		if raw == nil {
			continue
		}
		items := sliceToInterfaces(raw)
		hostname := hostNames[snapIdx]
		for _, item := range items {
			ik := identityFn(item)
			vk := variantFn(item)
			key := compositeKey{identity: ik, variant: vk}
			if entry, ok := seen[key]; ok {
				entry.hosts = append(entry.hosts, hostname)
			} else {
				seen[key] = &mergedEntry{item: item, hosts: []string{hostname}}
				order = append(order, key)
			}
		}
	}

	var result []mergedEntry
	for _, k := range order {
		entry := seen[k]
		result = append(result, mergedEntry{item: entry.item, hosts: entry.hosts})
	}
	return result
}

// mergeModuleStreams merges module streams, keyed by module_name:stream,
// unioning profiles across hosts.
func mergeModuleStreams(allItems []interface{}, total, minPrevalence int, hostNames []string) []schema.EnabledModuleStream {
	type entry struct {
		item     schema.EnabledModuleStream
		hosts    []string
		profiles map[string]bool
	}
	seen := make(map[string]*entry)
	var order []string

	for snapIdx, raw := range allItems {
		if raw == nil {
			continue
		}
		items := sliceToInterfaces(raw)
		hostname := hostNames[snapIdx]
		for _, item := range items {
			ms := item.(schema.EnabledModuleStream)
			k := fmt.Sprintf("%s:%s", ms.ModuleName, ms.Stream)
			if e, ok := seen[k]; ok {
				e.hosts = append(e.hosts, hostname)
				for _, p := range ms.Profiles {
					e.profiles[p] = true
				}
			} else {
				profiles := make(map[string]bool)
				for _, p := range ms.Profiles {
					profiles[p] = true
				}
				seen[k] = &entry{item: ms, hosts: []string{hostname}, profiles: profiles}
				order = append(order, k)
			}
		}
	}

	var result []schema.EnabledModuleStream
	for _, k := range order {
		e := seen[k]
		ms := e.item
		// Build sorted profiles
		profiles := make([]string, 0, len(e.profiles))
		for p := range e.profiles {
			profiles = append(profiles, p)
		}
		sort.Strings(profiles)
		ms.Profiles = profiles
		count := len(e.hosts)
		ms.Fleet = &schema.FleetPrevalence{Count: count, Total: total, Hosts: e.hosts}
		ms.Include = prevalenceInclude(count, total, minPrevalence)
		result = append(result, ms)
	}
	return result
}

// deduplicateStrings returns the union of string slices, preserving
// first-seen order.
func deduplicateStrings(allLists [][]string) []string {
	seen := make(map[string]bool)
	var result []string
	for _, items := range allLists {
		for _, item := range items {
			if !seen[item] {
				seen[item] = true
				result = append(result, item)
			}
		}
	}
	return result
}

// deduplicateOptionalStrings unions optional string slices, returning nil
// when all inputs are nil.
func deduplicateOptionalStrings(raw []*[]string) *[]string {
	var nonNil [][]string
	for _, r := range raw {
		if r != nil {
			nonNil = append(nonNil, *r)
		}
	}
	if len(nonNil) == 0 {
		return nil
	}
	result := deduplicateStrings(nonNil)
	return &result
}

// deduplicateDicts deduplicates dict slices by a key field, injecting
// fleet prevalence metadata.
func deduplicateDicts(allLists [][]map[string]interface{}, keyField string, total int, hostNames []string) []map[string]interface{} {
	type entry struct {
		item  map[string]interface{}
		hosts []string
	}
	seen := make(map[string]*entry)
	var order []string

	for snapIdx, items := range allLists {
		hostname := hostNames[snapIdx]
		for _, item := range items {
			k, _ := item[keyField].(string)
			if e, ok := seen[k]; ok {
				e.hosts = append(e.hosts, hostname)
			} else {
				// Deep copy the map
				copied := make(map[string]interface{})
				for mk, mv := range item {
					copied[mk] = mv
				}
				seen[k] = &entry{item: copied, hosts: []string{hostname}}
				order = append(order, k)
			}
		}
	}

	var result []map[string]interface{}
	for _, k := range order {
		e := seen[k]
		e.item["fleet"] = map[string]interface{}{
			"count": len(e.hosts),
			"total": total,
			"hosts": toStringSliceInterface(e.hosts),
		}
		result = append(result, e.item)
	}
	return result
}

// deduplicateWarnings deduplicates warning dicts across snapshots.
func deduplicateWarnings(snapshots []*schema.InspectionSnapshot) []map[string]interface{} {
	type warnKey struct {
		path, pattern, source, message string
		line                           string
	}
	seen := make(map[warnKey]bool)
	var result []map[string]interface{}

	for _, s := range snapshots {
		for _, item := range s.Warnings {
			k := warnKey{
				path:    stringFromMap(item, "path"),
				pattern: stringFromMap(item, "pattern"),
				source:  stringFromMap(item, "source"),
				message: stringFromMap(item, "message"),
				line:    stringFromMap(item, "line"),
			}
			if !seen[k] {
				seen[k] = true
				result = append(result, item)
			}
		}
	}
	return result
}

// deduplicateRedactions deduplicates redaction entries across snapshots.
func deduplicateRedactions(snapshots []*schema.InspectionSnapshot) []json.RawMessage {
	type redactKey struct {
		path, pattern, source, replacement string
	}
	seen := make(map[redactKey]bool)
	var result []json.RawMessage

	for _, s := range snapshots {
		for _, raw := range s.Redactions {
			var m map[string]interface{}
			if json.Unmarshal(raw, &m) != nil {
				result = append(result, raw)
				continue
			}
			k := redactKey{
				path:        stringFromMap(m, "path"),
				pattern:     stringFromMap(m, "pattern"),
				source:      stringFromMap(m, "source"),
				replacement: stringFromMap(m, "replacement"),
			}
			if !seen[k] {
				seen[k] = true
				result = append(result, raw)
			}
		}
	}
	return result
}

func mergeDepTrees(allTrees []map[string]interface{}) map[string]interface{} {
	merged := make(map[string]interface{})
	for _, tree := range allTrees {
		if tree == nil {
			continue
		}
		for leaf, depsRaw := range tree {
			deps, ok := depsRaw.([]interface{})
			if !ok {
				continue
			}
			if _, exists := merged[leaf]; !exists {
				merged[leaf] = depsRaw
			} else {
				existing := make(map[string]bool)
				existingSlice, _ := merged[leaf].([]interface{})
				for _, d := range existingSlice {
					if s, ok := d.(string); ok {
						existing[s] = true
					}
				}
				for _, d := range deps {
					if s, ok := d.(string); ok {
						if !existing[s] {
							existingSlice = append(existingSlice, d)
							existing[s] = true
						}
					}
				}
				merged[leaf] = existingSlice
			}
		}
	}
	if len(merged) == 0 {
		return nil
	}
	return merged
}

// ---------------------------------------------------------------------------
// Auto-select variants for content-merged items
// ---------------------------------------------------------------------------

// variantSelectable is an interface for items that participate in variant
// auto-selection.
type variantSelectable interface {
	getPath() string
	getFleet() *schema.FleetPrevalence
	getInclude() bool
	setInclude(bool)
	setTie(bool)
	setTieWinner(bool)
	getContentForHash() string
}

type configFileVariant struct {
	entry *schema.ConfigFileEntry
}

func (c configFileVariant) getPath() string                  { return c.entry.Path }
func (c configFileVariant) getFleet() *schema.FleetPrevalence { return c.entry.Fleet }
func (c configFileVariant) getInclude() bool                 { return c.entry.Include }
func (c configFileVariant) setInclude(v bool)                { c.entry.Include = v }
func (c configFileVariant) setTie(v bool)                    { c.entry.Tie = v }
func (c configFileVariant) setTieWinner(v bool)              { c.entry.TieWinner = v }
func (c configFileVariant) getContentForHash() string        { return c.entry.Content }

type dropInVariant struct {
	entry *schema.SystemdDropIn
}

func (d dropInVariant) getPath() string                  { return d.entry.Path }
func (d dropInVariant) getFleet() *schema.FleetPrevalence { return d.entry.Fleet }
func (d dropInVariant) getInclude() bool                 { return d.entry.Include }
func (d dropInVariant) setInclude(v bool)                { d.entry.Include = v }
func (d dropInVariant) setTie(v bool)                    { d.entry.Tie = v }
func (d dropInVariant) setTieWinner(v bool)              { d.entry.TieWinner = v }
func (d dropInVariant) getContentForHash() string        { return d.entry.Content }

type quadletVariant struct {
	entry *schema.QuadletUnit
}

func (q quadletVariant) getPath() string                  { return q.entry.Path }
func (q quadletVariant) getFleet() *schema.FleetPrevalence { return q.entry.Fleet }
func (q quadletVariant) getInclude() bool                 { return q.entry.Include }
func (q quadletVariant) setInclude(v bool)                { q.entry.Include = v }
func (q quadletVariant) setTie(v bool)                    { q.entry.Tie = v }
func (q quadletVariant) setTieWinner(v bool)              { q.entry.TieWinner = v }
func (q quadletVariant) getContentForHash() string        { return q.entry.Content }

type composeVariant struct {
	entry *schema.ComposeFile
}

func (c composeVariant) getPath() string                  { return c.entry.Path }
func (c composeVariant) getFleet() *schema.FleetPrevalence { return c.entry.Fleet }
func (c composeVariant) getInclude() bool                 { return c.entry.Include }
func (c composeVariant) setInclude(v bool)                { c.entry.Include = v }
func (c composeVariant) setTie(v bool)                    { c.entry.Tie = v }
func (c composeVariant) setTieWinner(v bool)              { c.entry.TieWinner = v }
func (c composeVariant) getContentForHash() string {
	pairs := make([]string, len(c.entry.Images))
	for i, img := range c.entry.Images {
		pairs[i] = fmt.Sprintf("(%s, %s)", img.Service, img.Image)
	}
	sort.Strings(pairs)
	return fmt.Sprintf("[%s]", strings.Join(pairs, ", "))
}

// autoSelectVariants is a generic variant selector. It groups items by
// path, and within each group picks the winner by highest prevalence,
// with tie-breaking by content hash.
func autoSelectVariants[T any](items []T) {
	// Build wrappers
	wrappers := make([]variantSelectable, len(items))
	for i := range items {
		wrappers[i] = makeVariantWrapper(&items[i])
	}

	groups := make(map[string][]int)
	var order []string
	for i, w := range wrappers {
		if w == nil {
			continue
		}
		path := w.getPath()
		if path == "" || w.getFleet() == nil {
			continue
		}
		if _, ok := groups[path]; !ok {
			order = append(order, path)
		}
		groups[path] = append(groups[path], i)
	}

	for _, path := range order {
		indices := groups[path]
		if len(indices) == 1 {
			wrappers[indices[0]].setInclude(true)
			continue
		}

		// Sort by count descending
		sort.Slice(indices, func(a, b int) bool {
			fa := wrappers[indices[a]].getFleet()
			fb := wrappers[indices[b]].getFleet()
			return fa.Count > fb.Count
		})

		topCount := wrappers[indices[0]].getFleet().Count
		secondCount := wrappers[indices[1]].getFleet().Count

		if topCount == secondCount {
			// Tie — collect all at max count
			var tied, nonTied []int
			for _, idx := range indices {
				if wrappers[idx].getFleet().Count == topCount {
					tied = append(tied, idx)
				} else {
					nonTied = append(nonTied, idx)
				}
			}

			// Sort tied by content hash for deterministic pick
			sort.Slice(tied, func(a, b int) bool {
				ha := contentHash(normalizeContent(wrappers[tied[a]].getContentForHash()))
				hb := contentHash(normalizeContent(wrappers[tied[b]].getContentForHash()))
				return ha < hb
			})

			for _, idx := range tied {
				wrappers[idx].setTie(true)
				wrappers[idx].setTieWinner(false)
				wrappers[idx].setInclude(false)
			}
			wrappers[tied[0]].setTieWinner(true)
			wrappers[tied[0]].setInclude(true)

			for _, idx := range nonTied {
				wrappers[idx].setInclude(false)
			}
		} else {
			wrappers[indices[0]].setInclude(true)
			for _, idx := range indices[1:] {
				wrappers[idx].setInclude(false)
			}
		}
	}
}

func makeVariantWrapper(item interface{}) variantSelectable {
	switch v := item.(type) {
	case *schema.ConfigFileEntry:
		return configFileVariant{entry: v}
	case *schema.SystemdDropIn:
		return dropInVariant{entry: v}
	case *schema.QuadletUnit:
		return quadletVariant{entry: v}
	case *schema.ComposeFile:
		return composeVariant{entry: v}
	default:
		return nil
	}
}

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

func normalizeContent(text string) string {
	text = strings.ReplaceAll(text, "\r\n", "\n")
	text = strings.ReplaceAll(text, "\r", "\n")
	lines := strings.Split(text, "\n")
	for i, line := range lines {
		lines[i] = strings.TrimRight(line, " \t")
	}
	return strings.Join(lines, "\n")
}

func contentHash(text string) string {
	h := sha256.Sum256([]byte(text))
	return fmt.Sprintf("%x", h)
}

func stringFromMap(m map[string]interface{}, key string) string {
	v, _ := m[key].(string)
	return v
}

func toStringSliceInterface(ss []string) []interface{} {
	result := make([]interface{}, len(ss))
	for i, s := range ss {
		result[i] = s
	}
	return result
}

func makeHostTitleMap(displayNames, fullHostnames []string) map[string]interface{} {
	m := make(map[string]interface{})
	for i, dn := range displayNames {
		if i < len(fullHostnames) {
			m[dn] = fullHostnames[i]
		}
	}
	return m
}

// sliceToInterfaces converts a typed slice (via reflection-free approach
// using type switches) to []interface{}.
func sliceToInterfaces(v interface{}) []interface{} {
	switch s := v.(type) {
	case []schema.PackageEntry:
		r := make([]interface{}, len(s))
		for i, item := range s {
			r[i] = item
		}
		return r
	case []schema.RepoFile:
		r := make([]interface{}, len(s))
		for i, item := range s {
			r[i] = item
		}
		return r
	case []schema.ConfigFileEntry:
		r := make([]interface{}, len(s))
		for i, item := range s {
			r[i] = item
		}
		return r
	case []schema.ServiceStateChange:
		r := make([]interface{}, len(s))
		for i, item := range s {
			r[i] = item
		}
		return r
	case []schema.SystemdDropIn:
		r := make([]interface{}, len(s))
		for i, item := range s {
			r[i] = item
		}
		return r
	case []schema.FirewallZone:
		r := make([]interface{}, len(s))
		for i, item := range s {
			r[i] = item
		}
		return r
	case []schema.GeneratedTimerUnit:
		r := make([]interface{}, len(s))
		for i, item := range s {
			r[i] = item
		}
		return r
	case []schema.CronJob:
		r := make([]interface{}, len(s))
		for i, item := range s {
			r[i] = item
		}
		return r
	case []schema.QuadletUnit:
		r := make([]interface{}, len(s))
		for i, item := range s {
			r[i] = item
		}
		return r
	case []schema.ComposeFile:
		r := make([]interface{}, len(s))
		for i, item := range s {
			r[i] = item
		}
		return r
	case []schema.NonRpmItem:
		r := make([]interface{}, len(s))
		for i, item := range s {
			r[i] = item
		}
		return r
	case []schema.SelinuxPortLabel:
		r := make([]interface{}, len(s))
		for i, item := range s {
			r[i] = item
		}
		return r
	case []schema.EnabledModuleStream:
		r := make([]interface{}, len(s))
		for i, item := range s {
			r[i] = item
		}
		return r
	case []schema.VersionLockEntry:
		r := make([]interface{}, len(s))
		for i, item := range s {
			r[i] = item
		}
		return r
	default:
		return nil
	}
}

// ---------------------------------------------------------------------------
// Type assertion helpers — convert mergedEntry slices back to typed slices
// with fleet prevalence set.
// ---------------------------------------------------------------------------

func toPackageEntries(entries []mergedEntry) []schema.PackageEntry {
	result := make([]schema.PackageEntry, len(entries))
	total := totalFromEntries(entries)
	for i, e := range entries {
		pe := e.item.(schema.PackageEntry)
		count := len(e.hosts)
		pe.Fleet = &schema.FleetPrevalence{Count: count, Total: total, Hosts: e.hosts}
		pe.Include = prevalenceInclude(count, total, minPrevalenceFromContext(entries))
		result[i] = pe
	}
	return result
}

func toRepoFiles(entries []mergedEntry) []schema.RepoFile {
	result := make([]schema.RepoFile, len(entries))
	total := totalFromEntries(entries)
	for i, e := range entries {
		rf := e.item.(schema.RepoFile)
		count := len(e.hosts)
		rf.Fleet = &schema.FleetPrevalence{Count: count, Total: total, Hosts: e.hosts}
		rf.Include = prevalenceInclude(count, total, minPrevalenceFromContext(entries))
		result[i] = rf
	}
	return result
}

func toConfigFileEntries(entries []mergedEntry) []schema.ConfigFileEntry {
	result := make([]schema.ConfigFileEntry, len(entries))
	total := totalFromEntries(entries)
	for i, e := range entries {
		cf := e.item.(schema.ConfigFileEntry)
		count := len(e.hosts)
		cf.Fleet = &schema.FleetPrevalence{Count: count, Total: total, Hosts: e.hosts}
		cf.Include = prevalenceInclude(count, total, minPrevalenceFromContext(entries))
		result[i] = cf
	}
	return result
}

func toServiceStateChanges(entries []mergedEntry) []schema.ServiceStateChange {
	result := make([]schema.ServiceStateChange, len(entries))
	total := totalFromEntries(entries)
	for i, e := range entries {
		sc := e.item.(schema.ServiceStateChange)
		count := len(e.hosts)
		sc.Fleet = &schema.FleetPrevalence{Count: count, Total: total, Hosts: e.hosts}
		sc.Include = prevalenceInclude(count, total, minPrevalenceFromContext(entries))
		result[i] = sc
	}
	return result
}

func toSystemdDropIns(entries []mergedEntry) []schema.SystemdDropIn {
	result := make([]schema.SystemdDropIn, len(entries))
	total := totalFromEntries(entries)
	for i, e := range entries {
		di := e.item.(schema.SystemdDropIn)
		count := len(e.hosts)
		di.Fleet = &schema.FleetPrevalence{Count: count, Total: total, Hosts: e.hosts}
		di.Include = prevalenceInclude(count, total, minPrevalenceFromContext(entries))
		result[i] = di
	}
	return result
}

func toFirewallZones(entries []mergedEntry) []schema.FirewallZone {
	result := make([]schema.FirewallZone, len(entries))
	total := totalFromEntries(entries)
	for i, e := range entries {
		fz := e.item.(schema.FirewallZone)
		count := len(e.hosts)
		fz.Fleet = &schema.FleetPrevalence{Count: count, Total: total, Hosts: e.hosts}
		fz.Include = prevalenceInclude(count, total, minPrevalenceFromContext(entries))
		result[i] = fz
	}
	return result
}

func toGeneratedTimerUnits(entries []mergedEntry) []schema.GeneratedTimerUnit {
	result := make([]schema.GeneratedTimerUnit, len(entries))
	total := totalFromEntries(entries)
	for i, e := range entries {
		gtu := e.item.(schema.GeneratedTimerUnit)
		count := len(e.hosts)
		gtu.Fleet = &schema.FleetPrevalence{Count: count, Total: total, Hosts: e.hosts}
		gtu.Include = prevalenceInclude(count, total, minPrevalenceFromContext(entries))
		result[i] = gtu
	}
	return result
}

func toCronJobs(entries []mergedEntry) []schema.CronJob {
	result := make([]schema.CronJob, len(entries))
	total := totalFromEntries(entries)
	for i, e := range entries {
		cj := e.item.(schema.CronJob)
		count := len(e.hosts)
		cj.Fleet = &schema.FleetPrevalence{Count: count, Total: total, Hosts: e.hosts}
		cj.Include = prevalenceInclude(count, total, minPrevalenceFromContext(entries))
		result[i] = cj
	}
	return result
}

func toQuadletUnits(entries []mergedEntry) []schema.QuadletUnit {
	result := make([]schema.QuadletUnit, len(entries))
	total := totalFromEntries(entries)
	for i, e := range entries {
		qu := e.item.(schema.QuadletUnit)
		count := len(e.hosts)
		qu.Fleet = &schema.FleetPrevalence{Count: count, Total: total, Hosts: e.hosts}
		qu.Include = prevalenceInclude(count, total, minPrevalenceFromContext(entries))
		result[i] = qu
	}
	return result
}

func toComposeFiles(entries []mergedEntry) []schema.ComposeFile {
	result := make([]schema.ComposeFile, len(entries))
	total := totalFromEntries(entries)
	for i, e := range entries {
		cf := e.item.(schema.ComposeFile)
		count := len(e.hosts)
		cf.Fleet = &schema.FleetPrevalence{Count: count, Total: total, Hosts: e.hosts}
		cf.Include = prevalenceInclude(count, total, minPrevalenceFromContext(entries))
		result[i] = cf
	}
	return result
}

func toNonRpmItems(entries []mergedEntry) []schema.NonRpmItem {
	result := make([]schema.NonRpmItem, len(entries))
	total := totalFromEntries(entries)
	for i, e := range entries {
		nri := e.item.(schema.NonRpmItem)
		count := len(e.hosts)
		nri.Fleet = &schema.FleetPrevalence{Count: count, Total: total, Hosts: e.hosts}
		nri.Include = prevalenceInclude(count, total, minPrevalenceFromContext(entries))
		result[i] = nri
	}
	return result
}

func toSelinuxPortLabels(entries []mergedEntry) []schema.SelinuxPortLabel {
	result := make([]schema.SelinuxPortLabel, len(entries))
	total := totalFromEntries(entries)
	for i, e := range entries {
		pl := e.item.(schema.SelinuxPortLabel)
		count := len(e.hosts)
		pl.Fleet = &schema.FleetPrevalence{Count: count, Total: total, Hosts: e.hosts}
		pl.Include = prevalenceInclude(count, total, minPrevalenceFromContext(entries))
		result[i] = pl
	}
	return result
}

func toVersionLockEntries(entries []mergedEntry) []schema.VersionLockEntry {
	result := make([]schema.VersionLockEntry, len(entries))
	total := totalFromEntries(entries)
	for i, e := range entries {
		vl := e.item.(schema.VersionLockEntry)
		count := len(e.hosts)
		vl.Fleet = &schema.FleetPrevalence{Count: count, Total: total, Hosts: e.hosts}
		vl.Include = prevalenceInclude(count, total, minPrevalenceFromContext(entries))
		result[i] = vl
	}
	return result
}

// totalFromEntries infers the total host count from the first entry's
// fleet prevalence data. Returns 0 for empty slices.
func totalFromEntries(_ []mergedEntry) int {
	// This is called within MergeSnapshots where total is in scope.
	// We use a package-level variable approach instead.
	return mergeTotal
}

func minPrevalenceFromContext(_ []mergedEntry) int {
	return mergeMinPrevalence
}

// Package-level state for the current merge operation. These are set at
// the top of MergeSnapshots and read by the to* conversion helpers.
var (
	mergeTotal          int
	mergeMinPrevalence  int
)

// init sets up the merge context. Called at the top of MergeSnapshots.
func setMergeContext(total, minPrevalence int) {
	mergeTotal = total
	mergeMinPrevalence = minPrevalence
}
