// Package inspector provides the RPM inspector for the Go-native port.
//
// The RPM inspector collects installed packages, repo files, module streams,
// version locks, dnf history, rpm-ostree state, and rpm -Va output. It
// classifies packages as added/base_image_only/modified relative to a
// baseline when one is available.
package inspector

import (
	"encoding/json"
	"fmt"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"unicode"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

// RpmOptions configures the RPM inspector.
type RpmOptions struct {
	// BaselinePackages maps "name.arch" → PackageEntry from the base image.
	// nil means no-baseline mode (all installed treated as added).
	BaselinePackages map[string]schema.PackageEntry

	// SystemType is the detected system type (package-mode, rpm-ostree, bootc).
	SystemType schema.SystemType

	// TargetVersion is the target RHEL version string (e.g. "9.4").
	TargetVersion string

	// TargetImage is the target base image reference.
	TargetImage string

	// BaseImage is the resolved base image string to record in the output.
	BaseImage string
}

// Warning is an inspector warning matching the Python make_warning() output.
type Warning = map[string]interface{}

// makeWarning creates a warning map matching the Python make_warning() format.
func makeWarning(inspector, message string, severity ...string) Warning {
	sev := "error"
	if len(severity) > 0 {
		sev = severity[0]
	}
	return Warning{
		"inspector": inspector,
		"message":   message,
		"severity":  sev,
	}
}

// RunRpm runs the full RPM inspection and returns the populated section,
// accumulated warnings, and any fatal error.
func RunRpm(exec Executor, opts RpmOptions) (*schema.RpmSection, []Warning, error) {
	var warnings []Warning

	section := &schema.RpmSection{
		PackagesAdded:         []schema.PackageEntry{},
		BaseImageOnly:         []schema.PackageEntry{},
		RpmVa:                 []schema.RpmVaEntry{},
		RepoFiles:             []schema.RepoFile{},
		GpgKeys:               []schema.RepoFile{},
		DnfHistoryRemoved:     []string{},
		VersionChanges:        []schema.VersionChange{},
		ModuleStreams:          []schema.EnabledModuleStream{},
		VersionLocks:          []schema.VersionLockEntry{},
		ModuleStreamConflicts: []string{},
		MultiarchPackages:     []string{},
		DuplicatePackages:     []string{},
		RepoProvidingPackages: []string{},
		OstreeOverrides:       []schema.OstreePackageOverride{},
		OstreeRemovals:        []string{},
	}

	if opts.BaseImage != "" {
		bi := opts.BaseImage
		section.BaseImage = &bi
	}

	// Step 1: rpm -qa — get installed packages.
	installed := rpmQA(exec, &warnings)

	// Step 1b: Detect multi-arch and duplicate packages on full list.
	if len(installed) > 0 {
		section.MultiarchPackages = detectMultiarch(installed)
		section.DuplicatePackages = detectDuplicates(installed)
		for _, variant := range section.MultiarchPackages {
			name := variant
			if idx := strings.LastIndex(variant, "."); idx >= 0 {
				name = variant[:idx]
			}
			warnings = append(warnings, makeWarning("rpm",
				fmt.Sprintf("Package '%s' is installed in multiple architectures — verify affected variants are needed.", name),
				"warning"))
		}
		for _, key := range section.DuplicatePackages {
			warnings = append(warnings, makeWarning("rpm",
				fmt.Sprintf("Package '%s' has multiple versions installed — possible upgrade inconsistency.", key),
				"warning"))
		}
	}

	// Step 2: Baseline comparison.
	installedNames := make(map[string]bool, len(installed))
	for _, p := range installed {
		installedNames[p.Name] = true
	}

	if opts.BaselinePackages != nil {
		// We have a baseline — classify packages.
		section.NoBaseline = false

		baselineNameSet := make(map[string]bool)
		for _, bp := range opts.BaselinePackages {
			baselineNameSet[bp.Name] = true
		}

		addedNames := make(map[string]bool)
		for name := range installedNames {
			if !baselineNameSet[name] {
				addedNames[name] = true
			}
		}
		baseOnlyNames := make(map[string]bool)
		for name := range baselineNameSet {
			if !installedNames[name] {
				baseOnlyNames[name] = true
			}
		}

		blNames := sortedKeys(baselineNameSet)
		section.BaselinePackageNames = &blNames

		for i := range installed {
			if addedNames[installed[i].Name] {
				installed[i].State = schema.PackageStateAdded
				section.PackagesAdded = append(section.PackagesAdded, installed[i])
			}
		}

		// Populate base_image_only with full NEVRA from baseline when available.
		baselineByName := make(map[string]schema.PackageEntry)
		for _, bp := range opts.BaselinePackages {
			if _, exists := baselineByName[bp.Name]; !exists {
				baselineByName[bp.Name] = bp
			}
		}
		for _, name := range sortedKeys(baseOnlyNames) {
			if bp, ok := baselineByName[name]; ok && bp.Version != "" {
				section.BaseImageOnly = append(section.BaseImageOnly, schema.PackageEntry{
					Name:    bp.Name,
					Epoch:   bp.Epoch,
					Version: bp.Version,
					Release: bp.Release,
					Arch:    bp.Arch,
					State:   schema.PackageStateBaseImageOnly,
				})
			} else {
				section.BaseImageOnly = append(section.BaseImageOnly, schema.PackageEntry{
					Name:  name,
					Epoch: "0",
					Arch:  "noarch",
					State: schema.PackageStateBaseImageOnly,
				})
			}
		}

		// Version comparison for matched packages (only with NEVRA baseline).
		hasNevra := false
		for _, bp := range opts.BaselinePackages {
			if bp.Version != "" {
				hasNevra = true
				break
			}
		}
		if hasNevra {
			installedByKey := make(map[string]schema.PackageEntry)
			for _, p := range installed {
				key := fmt.Sprintf("%s.%s", p.Name, p.Arch)
				installedByKey[key] = p
			}
			for _, key := range sortedKeys(opts.BaselinePackages) {
				hostPkg, ok := installedByKey[key]
				if !ok {
					continue
				}
				basePkg := opts.BaselinePackages[key]
				cmp := compareEVR(hostPkg, basePkg)
				if cmp != 0 {
					dir := schema.VersionChangeUpgrade
					if cmp > 0 {
						dir = schema.VersionChangeDowngrade
					}
					section.VersionChanges = append(section.VersionChanges, schema.VersionChange{
						Name:        hostPkg.Name,
						Arch:        hostPkg.Arch,
						HostVersion: fmt.Sprintf("%s-%s", hostPkg.Version, hostPkg.Release),
						BaseVersion: fmt.Sprintf("%s-%s", basePkg.Version, basePkg.Release),
						HostEpoch:   hostPkg.Epoch,
						BaseEpoch:   basePkg.Epoch,
						Direction:   dir,
					})
				}
			}
			if len(section.VersionChanges) > 0 {
				sort.Slice(section.VersionChanges, func(i, j int) bool {
					di := 0
					if section.VersionChanges[i].Direction != schema.VersionChangeDowngrade {
						di = 1
					}
					dj := 0
					if section.VersionChanges[j].Direction != schema.VersionChangeDowngrade {
						dj = 1
					}
					if di != dj {
						return di < dj
					}
					return section.VersionChanges[i].Name < section.VersionChanges[j].Name
				})
				nDown := 0
				for _, vc := range section.VersionChanges {
					if vc.Direction == schema.VersionChangeDowngrade {
						nDown++
					}
				}
				if nDown > 0 {
					warnings = append(warnings, makeWarning("rpm",
						fmt.Sprintf("%d package(s) will be downgraded by the base image — review the Version Changes section.", nDown),
						"warning"))
				}
			}
		}
	} else {
		// No baseline — all installed packages are added.
		section.NoBaseline = true
		for i := range installed {
			installed[i].State = schema.PackageStateAdded
			section.PackagesAdded = append(section.PackagesAdded, installed[i])
		}
	}

	// Step 2b: Source repo per added package.
	if len(section.PackagesAdded) > 0 {
		populateSourceRepos(exec, section.PackagesAdded)
	}

	// Step 3: rpm -Va.
	if opts.SystemType != schema.SystemTypePackageMode {
		section.RpmVa = []schema.RpmVaEntry{}
	} else {
		result := exec.Run("rpm", "-Va", "--nodeps", "--noscripts")
		section.RpmVa = parseRpmVa(result.Stdout)
	}

	// Step 3b: rpm-ostree package state.
	if opts.SystemType != schema.SystemTypePackageMode {
		parseRpmOstreePackageState(exec, section, &warnings, opts.SystemType)
	}

	// Step 4: Leaf/auto classification.
	if len(section.PackagesAdded) > 0 && !section.NoBaseline {
		leaf, auto, depTree := classifyLeafAuto(exec, section.PackagesAdded)
		section.LeafPackages = &leaf
		section.AutoPackages = &auto
		section.LeafDepTree = depTree
	}

	// Step 5: Repo files.
	section.RepoFiles = collectRepoFiles(exec)
	section.GpgKeys = collectGpgKeys(exec, section.RepoFiles)

	// Step 5-rpp: Repo-providing packages.
	section.RepoProvidingPackages = detectRepoProvidingPackages(exec)

	// Step 5a: DNF module streams.
	section.ModuleStreams = collectModuleStreams(exec)

	// Step 5b: Version locks.
	locks, vlOutput := collectVersionLocks(exec)
	section.VersionLocks = locks
	section.VersionlockCommandOutput = vlOutput

	// Step 6: dnf history removed.
	section.DnfHistoryRemoved = dnfHistoryRemoved(exec, &warnings)

	return section, warnings, nil
}

// ---------------------------------------------------------------------------
// NEVRA parsing
// ---------------------------------------------------------------------------

// rpmQAQueryformat is the queryformat used with rpm -qa.
const rpmQAQueryformat = `%{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}`

// virtualPackages are package names to skip (not real packages).
var virtualPackages = map[string]bool{
	"gpg-pubkey":         true,
	"gpg-pubkey-release": true,
}

// parseNEVRA parses a single line from rpm -qa --queryformat output.
// Format: epoch:name-version-release.arch
// Returns nil if the line cannot be parsed.
func parseNEVRA(line string) *schema.PackageEntry {
	s := strings.TrimSpace(line)
	if s == "" {
		return nil
	}

	colonIdx := strings.Index(s, ":")
	if colonIdx < 0 {
		return nil
	}

	epochPart := s[:colonIdx]
	rest := s[colonIdx+1:]

	var epoch string
	switch {
	case isDigits(epochPart):
		epoch = epochPart
	case epochPart == "(none)":
		epoch = "0"
	default:
		return nil
	}

	dotIdx := strings.LastIndex(rest, ".")
	if dotIdx < 0 {
		return nil
	}
	base := rest[:dotIdx]
	arch := rest[dotIdx+1:]

	parts := strings.Split(base, "-")
	if len(parts) < 3 {
		return nil
	}
	release := parts[len(parts)-1]
	version := parts[len(parts)-2]
	name := strings.Join(parts[:len(parts)-2], "-")

	return &schema.PackageEntry{
		Name:    name,
		Epoch:   epoch,
		Version: version,
		Release: release,
		Arch:    arch,
		State:   schema.PackageStateAdded,
	}
}

// parseRpmQA parses the full output of rpm -qa --queryformat.
func parseRpmQA(stdout string, warnings *[]Warning) []schema.PackageEntry {
	var packages []schema.PackageEntry
	var failed []string

	for _, line := range strings.Split(strings.TrimSpace(stdout), "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		pkg := parseNEVRA(line)
		if pkg != nil {
			packages = append(packages, *pkg)
		} else {
			failed = append(failed, line)
		}
	}

	total := len(packages) + len(failed)
	if len(failed) > 0 && total > 0 {
		pct := float64(len(failed)) / float64(total) * 100
		sev := "info"
		if pct >= 5 {
			sev = "warning"
		}
		if warnings != nil {
			*warnings = append(*warnings, makeWarning("rpm",
				fmt.Sprintf("rpm -qa: %d package line(s) could not be parsed (%.0f%% of output) — package list may be incomplete.", len(failed), pct),
				sev))
		}
	}
	return packages
}

// rpmQA runs rpm -qa and returns the parsed package list, filtering out
// virtual packages.
func rpmQA(exec Executor, warnings *[]Warning) []schema.PackageEntry {
	result := exec.Run("rpm", "-qa", "--queryformat", rpmQAQueryformat+"\n")
	if result.ExitCode != 0 {
		// Fallback: try with --root.
		result = exec.Run("rpm", "--root", exec.HostRoot(), "-qa",
			"--queryformat", rpmQAQueryformat+"\n")
		if result.ExitCode == 0 && warnings != nil {
			*warnings = append(*warnings, makeWarning("rpm",
				"rpm -qa used --root fallback (--dbpath query failed); results are correct but may be slower.",
				"info"))
		}
	}

	all := parseRpmQA(result.Stdout, warnings)
	var filtered []schema.PackageEntry
	for _, p := range all {
		if !virtualPackages[p.Name] {
			filtered = append(filtered, p)
		}
	}
	return filtered
}

// ---------------------------------------------------------------------------
// Multi-arch and duplicate detection
// ---------------------------------------------------------------------------

// detectMultiarch returns name.arch keys for packages installed in multiple
// architectures.
func detectMultiarch(installed []schema.PackageEntry) []string {
	byName := make(map[string]map[string]bool)
	for _, p := range installed {
		if byName[p.Name] == nil {
			byName[p.Name] = make(map[string]bool)
		}
		byName[p.Name][p.Arch] = true
	}

	var flagged []string
	for name, arches := range byName {
		if len(arches) <= 1 {
			continue
		}
		if arches["x86_64"] {
			for arch := range arches {
				if arch != "x86_64" {
					flagged = append(flagged, fmt.Sprintf("%s.%s", name, arch))
				}
			}
		} else {
			for arch := range arches {
				flagged = append(flagged, fmt.Sprintf("%s.%s", name, arch))
			}
		}
	}
	sort.Strings(flagged)
	return flagged
}

// detectDuplicates returns name.arch keys that have more than one version
// installed.
func detectDuplicates(installed []schema.PackageEntry) []string {
	counts := make(map[string]int)
	for _, p := range installed {
		key := fmt.Sprintf("%s.%s", p.Name, p.Arch)
		counts[key]++
	}
	var dups []string
	for key, count := range counts {
		if count > 1 {
			dups = append(dups, key)
		}
	}
	sort.Strings(dups)
	return dups
}

// ---------------------------------------------------------------------------
// rpm -Va parsing
// ---------------------------------------------------------------------------

// parseRpmVa parses rpm -Va output.
// Format: flags type path (e.g. "S.5....T.  c /etc/foo")
func parseRpmVa(stdout string) []schema.RpmVaEntry {
	var entries []schema.RpmVaEntry
	for _, line := range strings.Split(strings.TrimSpace(stdout), "\n") {
		line = strings.TrimSpace(line)
		if line == "" || len(line) < 11 {
			continue
		}
		flags := strings.TrimSpace(line[:9])
		rest := strings.TrimLeft(line[9:], " \t")

		var path string
		if strings.HasPrefix(rest, "c ") || strings.HasPrefix(rest, "d ") {
			path = strings.TrimSpace(rest[2:])
		} else {
			path = strings.TrimSpace(rest)
		}

		if path != "" && !strings.HasPrefix(path, "/boot/") {
			entries = append(entries, schema.RpmVaEntry{
				Path:  path,
				Flags: flags,
			})
		}
	}
	return entries
}

// ---------------------------------------------------------------------------
// RPM version comparison
// ---------------------------------------------------------------------------

// rpmvercmp compares two RPM version/release strings using the rpmvercmp
// algorithm. Returns negative if a < b, 0 if equal, positive if a > b.
func rpmvercmp(a, b string) int {
	if a == b {
		return 0
	}

	i, j := 0, 0
	for i < len(a) || j < len(b) {
		// Skip non-alphanumeric, non-tilde, non-caret separators.
		for i < len(a) && !isAlnum(a[i]) && a[i] != '~' && a[i] != '^' {
			i++
		}
		for j < len(b) && !isAlnum(b[j]) && b[j] != '~' && b[j] != '^' {
			j++
		}

		if i >= len(a) && j >= len(b) {
			return 0
		}
		if i >= len(a) {
			if b[j] == '^' {
				return -1
			}
			if b[j] == '~' {
				return 1
			}
			return -1
		}
		if j >= len(b) {
			if a[i] == '^' {
				return 1
			}
			if a[i] == '~' {
				return -1
			}
			return 1
		}

		// Tilde sorts before everything (pre-release).
		if a[i] == '~' {
			if b[j] != '~' {
				return -1
			}
			i++
			j++
			continue
		}
		if b[j] == '~' {
			return 1
		}

		// Caret sorts after empty but before any alphanumeric.
		if a[i] == '^' {
			if b[j] == '^' {
				i++
				j++
				continue
			}
			return 1
		}
		if b[j] == '^' {
			return -1
		}

		// Compare numeric or alpha segments.
		if isDigit(a[i]) {
			if !isDigit(b[j]) {
				return 1 // numeric > alpha
			}
			// Extract numeric segments.
			si := i
			for i < len(a) && isDigit(a[i]) {
				i++
			}
			segA := a[si:i]

			sj := j
			for j < len(b) && isDigit(b[j]) {
				j++
			}
			segB := b[sj:j]

			// Strip leading zeros for numeric comparison.
			segA = strings.TrimLeft(segA, "0")
			segB = strings.TrimLeft(segB, "0")

			if len(segA) != len(segB) {
				if len(segA) > len(segB) {
					return 1
				}
				return -1
			}
			if segA != segB {
				if segA > segB {
					return 1
				}
				return -1
			}
		} else {
			si := i
			for i < len(a) && isAlpha(a[i]) {
				i++
			}
			segA := a[si:i]

			sj := j
			if j < len(b) && isAlpha(b[j]) {
				for j < len(b) && isAlpha(b[j]) {
					j++
				}
				segB := b[sj:j]
				if segA != segB {
					if segA > segB {
						return 1
					}
					return -1
				}
			} else {
				return -1 // alpha < numeric
			}
		}
	}
	return 0
}

// compareEVR compares epoch:version-release between two PackageEntry values.
// Returns negative if host < base, 0 if equal, positive if host > base.
func compareEVR(host, base schema.PackageEntry) int {
	hEpoch := epochInt(host.Epoch)
	bEpoch := epochInt(base.Epoch)
	if hEpoch != bEpoch {
		if hEpoch > bEpoch {
			return 1
		}
		return -1
	}

	vc := rpmvercmp(host.Version, base.Version)
	if vc != 0 {
		return vc
	}
	return rpmvercmp(host.Release, base.Release)
}

// ---------------------------------------------------------------------------
// Source repo attribution
// ---------------------------------------------------------------------------

// populateSourceRepos sets SourceRepo on each PackageEntry.
// Primary: dnf repoquery --installed.
// Fallback: rpm -qi (checking "From repo" and "Repository").
func populateSourceRepos(exec Executor, packages []schema.PackageEntry) {
	nameSet := make(map[string]bool)
	for _, p := range packages {
		nameSet[p.Name] = true
	}
	names := sortedKeys(nameSet)
	if len(names) == 0 {
		return
	}

	repoMap := make(map[string]string)

	// Primary: dnf repoquery.
	if !tryDnfSourceRepo(exec, names, nameSet, repoMap) {
		tryRpmSourceRepo(exec, names, repoMap)
	}

	for i := range packages {
		if repo, ok := repoMap[packages[i].Name]; ok {
			packages[i].SourceRepo = repo
		}
	}
}

func tryDnfSourceRepo(exec Executor, names []string, nameSet map[string]bool, repoMap map[string]string) bool {
	// Probe with the first package.
	probe := exec.Run("dnf", "repoquery", "--installed", "--queryformat", "%{name} %{from_repo}\n", names[0])
	if probe.ExitCode != 0 {
		return false
	}
	parseDnfRepoLines(probe.Stdout, nameSet, repoMap)

	// Process remaining in batches.
	batchSize := 100
	for i := 1; i < len(names); i += batchSize {
		end := i + batchSize
		if end > len(names) {
			end = len(names)
		}
		args := append([]string{"repoquery", "--installed", "--queryformat", "%{name} %{from_repo}\n"}, names[i:end]...)
		result := exec.Run("dnf", args...)
		if result.ExitCode != 0 {
			continue
		}
		parseDnfRepoLines(result.Stdout, nameSet, repoMap)
	}
	return true
}

func parseDnfRepoLines(stdout string, nameSet map[string]bool, repoMap map[string]string) {
	for _, line := range strings.Split(strings.TrimSpace(stdout), "\n") {
		parts := strings.SplitN(strings.TrimSpace(line), " ", 2)
		if len(parts) == 2 && nameSet[parts[0]] {
			if _, exists := repoMap[parts[0]]; !exists {
				repoMap[parts[0]] = parts[1]
			}
		}
	}
}

func tryRpmSourceRepo(exec Executor, names []string, repoMap map[string]string) {
	batchSize := 100
	for i := 0; i < len(names); i += batchSize {
		end := i + batchSize
		if end > len(names) {
			end = len(names)
		}
		args := append([]string{"-qi"}, names[i:end]...)
		result := exec.Run("rpm", args...)
		if result.ExitCode != 0 {
			continue
		}
		var curName string
		for _, line := range strings.Split(result.Stdout, "\n") {
			if strings.HasPrefix(line, "Name") {
				parts := strings.SplitN(line, ":", 2)
				if len(parts) == 2 {
					curName = strings.TrimSpace(parts[1])
				}
			} else if strings.HasPrefix(line, "From repo") || strings.HasPrefix(line, "Repository") {
				parts := strings.SplitN(line, ":", 2)
				if len(parts) == 2 && curName != "" {
					if _, exists := repoMap[curName]; !exists {
						repoMap[curName] = strings.TrimSpace(parts[1])
					}
				}
			}
		}
	}
}

// ---------------------------------------------------------------------------
// Leaf/auto classification
// ---------------------------------------------------------------------------

// classifyLeafAuto splits added packages into leaf (explicitly installed) vs
// auto (dependency) with a per-leaf transitive dependency tree.
func classifyLeafAuto(exec Executor, packagesAdded []schema.PackageEntry) ([]string, []string, map[string]interface{}) {
	addedNames := make(map[string]bool)
	for _, p := range packagesAdded {
		addedNames[p.Name] = true
	}

	// Try dnf --userinstalled first.
	userInstalled := queryUserInstalled(exec)

	// Build dependency graph.
	dependsOn, transitive := classifyDepsDnf(exec, addedNames)
	if dependsOn == nil {
		dependsOn = classifyDepsRpm(exec, addedNames)
	}

	var leaf, auto []string

	if userInstalled != nil {
		leafSet := intersect(userInstalled, addedNames)
		if len(leafSet) == 0 && len(addedNames) > 0 {
			// Fallback to graph-based.
			userInstalled = nil
		} else {
			for name := range addedNames {
				if leafSet[name] {
					leaf = append(leaf, name)
				} else {
					auto = append(auto, name)
				}
			}
			sort.Strings(leaf)
			sort.Strings(auto)
		}
	}

	if userInstalled == nil {
		dependedOn := make(map[string]bool)
		for _, deps := range dependsOn {
			for dep := range deps {
				dependedOn[dep] = true
			}
		}
		for name := range addedNames {
			if dependedOn[name] {
				auto = append(auto, name)
			} else {
				leaf = append(leaf, name)
			}
		}
		sort.Strings(leaf)
		sort.Strings(auto)
	}

	// Build per-leaf transitive dependency tree.
	autoSet := make(map[string]bool, len(auto))
	for _, a := range auto {
		autoSet[a] = true
	}
	depTree := make(map[string]interface{})
	if transitive {
		// dnf repoquery --recursive already gave transitive closure.
		for _, lf := range leaf {
			deps := dependsOn[lf]
			var filtered []string
			for dep := range deps {
				if autoSet[dep] {
					filtered = append(filtered, dep)
				}
			}
			sort.Strings(filtered)
			depTree[lf] = filtered
		}
	} else {
		// rpm gives only direct deps; walk the graph.
		for _, lf := range leaf {
			reachable := make(map[string]bool)
			stack := make([]string, 0)
			for dep := range dependsOn[lf] {
				stack = append(stack, dep)
			}
			for len(stack) > 0 {
				dep := stack[len(stack)-1]
				stack = stack[:len(stack)-1]
				if reachable[dep] {
					continue
				}
				reachable[dep] = true
				for next := range dependsOn[dep] {
					if !reachable[next] {
						stack = append(stack, next)
					}
				}
			}
			var filtered []string
			for dep := range reachable {
				if autoSet[dep] {
					filtered = append(filtered, dep)
				}
			}
			sort.Strings(filtered)
			depTree[lf] = filtered
		}
	}

	return leaf, auto, depTree
}

// queryUserInstalled queries dnf for user-installed packages.
// Returns nil if dnf is unavailable.
func queryUserInstalled(exec Executor) map[string]bool {
	result := exec.Run("dnf", "repoquery", "--userinstalled", "--queryformat", "%{name}\n")
	if result.ExitCode != 0 {
		return nil
	}
	names := make(map[string]bool)
	for _, line := range strings.Split(strings.TrimSpace(result.Stdout), "\n") {
		name := strings.TrimSpace(line)
		if name != "" {
			names[name] = true
		}
	}
	return names
}

// classifyDepsDnf builds a transitive dependency graph using dnf repoquery.
// Returns (dependsOn, true) if successful, or (nil, false) if dnf is unavailable.
func classifyDepsDnf(exec Executor, addedNames map[string]bool) (map[string]map[string]bool, bool) {
	if len(addedNames) == 0 {
		return make(map[string]map[string]bool), true
	}

	nameList := sortedKeys(addedNames)

	// Probe with first package.
	first := exec.Run("dnf", "repoquery", "--requires", "--resolve", "--recursive",
		"--installed", "--queryformat", "%{name}\n", nameList[0])
	if first.ExitCode != 0 {
		return nil, false
	}

	dependsOn := make(map[string]map[string]bool, len(addedNames))
	for name := range addedNames {
		dependsOn[name] = make(map[string]bool)
	}

	parseDnfDeps(first.Stdout, nameList[0], addedNames, dependsOn)

	for _, pkgName := range nameList[1:] {
		result := exec.Run("dnf", "repoquery", "--requires", "--resolve", "--recursive",
			"--installed", "--queryformat", "%{name}\n", pkgName)
		if result.ExitCode != 0 {
			continue
		}
		parseDnfDeps(result.Stdout, pkgName, addedNames, dependsOn)
	}

	return dependsOn, true
}

func parseDnfDeps(stdout, pkgName string, addedNames map[string]bool, dependsOn map[string]map[string]bool) {
	for _, line := range strings.Split(strings.TrimSpace(stdout), "\n") {
		depName := strings.TrimSpace(line)
		if depName != "" && addedNames[depName] && depName != pkgName {
			dependsOn[pkgName][depName] = true
		}
	}
}

// classifyDepsRpm builds a dependency graph using rpm -qR + --whatprovides.
func classifyDepsRpm(exec Executor, addedNames map[string]bool) map[string]map[string]bool {
	dependsOn := make(map[string]map[string]bool, len(addedNames))
	for name := range addedNames {
		dependsOn[name] = make(map[string]bool)
	}

	nameList := sortedKeys(addedNames)
	nameRe := regexp.MustCompile(`^(.+?)-\d`)
	batchSize := 50

	for i := 0; i < len(nameList); i += batchSize {
		end := i + batchSize
		if end > len(nameList) {
			end = len(nameList)
		}
		for _, pkgName := range nameList[i:end] {
			result := exec.Run("rpm", "-qR", pkgName)
			if result.ExitCode != 0 {
				continue
			}

			caps := make(map[string]bool)
			for _, line := range strings.Split(result.Stdout, "\n") {
				cap := strings.TrimSpace(line)
				if cap != "" && !strings.HasPrefix(cap, "rpmlib(") && !strings.HasPrefix(cap, "/") {
					caps[strings.Fields(cap)[0]] = true
				}
			}
			if len(caps) == 0 {
				continue
			}

			capList := sortedKeys(caps)
			for j := 0; j < len(capList); j += batchSize {
				capEnd := j + batchSize
				if capEnd > len(capList) {
					capEnd = len(capList)
				}
				args := append([]string{"-q", "--whatprovides"}, capList[j:capEnd]...)
				wpResult := exec.Run("rpm", args...)
				if wpResult.ExitCode != 0 {
					continue
				}
				for _, pline := range strings.Split(wpResult.Stdout, "\n") {
					pline = strings.TrimSpace(pline)
					if pline == "" || strings.Contains(pline, "no package provides") {
						continue
					}
					match := nameRe.FindStringSubmatch(pline)
					var provider string
					if match != nil {
						provider = match[1]
					} else {
						parts := strings.SplitN(pline, "-", 2)
						provider = parts[0]
					}
					if addedNames[provider] && provider != pkgName {
						dependsOn[pkgName][provider] = true
					}
				}
			}
		}
	}
	return dependsOn
}

// ---------------------------------------------------------------------------
// Repo files
// ---------------------------------------------------------------------------

var defaultRepoFilenamePatterns = []string{"redhat.repo", "redhat-rhui", "redhat.redhat"}
var defaultRepoIDPrefixes = []string{
	"rhel-", "baseos", "appstream", "rhui-", "crb", "codeready",
	"fedora", "updates",
}

// classifyDefaultRepo returns true if a repo file looks like a default distro
// repository.
func classifyDefaultRepo(rf schema.RepoFile) bool {
	basename := rf.Path
	if idx := strings.LastIndex(rf.Path, "/"); idx >= 0 {
		basename = rf.Path[idx+1:]
	}
	for _, pat := range defaultRepoFilenamePatterns {
		if strings.Contains(basename, pat) {
			return true
		}
	}
	for _, line := range strings.Split(rf.Content, "\n") {
		stripped := strings.TrimSpace(line)
		if strings.HasPrefix(stripped, "[") && strings.HasSuffix(stripped, "]") {
			sectionID := stripped[1 : len(stripped)-1]
			for _, prefix := range defaultRepoIDPrefixes {
				if strings.HasPrefix(sectionID, prefix) {
					return true
				}
			}
		}
	}
	return false
}

// collectRepoFiles reads repo files from /etc/yum.repos.d and /etc/dnf.
func collectRepoFiles(exec Executor) []schema.RepoFile {
	var repoFiles []schema.RepoFile
	for _, subdir := range []string{"/etc/yum.repos.d", "/etc/dnf"} {
		entries, err := exec.ReadDir(subdir)
		if err != nil {
			continue
		}
		for _, entry := range entries {
			if entry.IsDir() {
				continue
			}
			name := entry.Name()
			ext := filepath.Ext(name)

			// Skip .module files; accept .repo, .conf, or anything in /etc/dnf.
			if ext == ".module" {
				continue
			}
			if ext != ".repo" && ext != ".conf" && subdir != "/etc/dnf" {
				continue
			}

			path := filepath.Join(subdir, name)
			content, err := exec.ReadFile(path)
			if err != nil {
				content = ""
			}

			// Store relative path (strip leading /).
			relPath := strings.TrimPrefix(path, "/")
			rf := schema.RepoFile{
				Path:    relPath,
				Content: content,
			}
			rf.IsDefaultRepo = classifyDefaultRepo(rf)
			repoFiles = append(repoFiles, rf)
		}
	}
	return repoFiles
}

// collectGpgKeys reads GPG key files referenced by gpgkey=file:///...
// in repo configs.
func collectGpgKeys(exec Executor, repoFiles []schema.RepoFile) []schema.RepoFile {
	seen := make(map[string]string) // relPath → content

	for _, repo := range repoFiles {
		lines := strings.Split(repo.Content, "\n")
		for i := 0; i < len(lines); i++ {
			stripped := strings.TrimSpace(lines[i])
			if !strings.HasPrefix(stripped, "gpgkey") {
				continue
			}
			_, value, found := strings.Cut(stripped, "=")
			if !found {
				continue
			}

			// Accumulate continuation lines.
			parts := []string{value}
			for i+1 < len(lines) {
				next := lines[i+1]
				if len(next) > 0 && (next[0] == ' ' || next[0] == '\t') && !strings.Contains(next, "=") {
					parts = append(parts, strings.TrimSpace(next))
					i++
				} else {
					break
				}
			}

			combined := strings.Join(parts, " ")
			for _, token := range splitGpgKeyTokens(combined) {
				token = strings.TrimSpace(token)
				if !strings.HasPrefix(token, "file://") {
					continue
				}
				absPath := token[len("file://"):]
				relPath := strings.TrimLeft(absPath, "/")
				if _, exists := seen[relPath]; exists {
					continue
				}
				content, err := exec.ReadFile("/" + relPath)
				if err != nil {
					continue
				}
				seen[relPath] = content
			}
		}
	}

	// Sort by path for deterministic output.
	var keys []schema.RepoFile
	var paths []string
	for p := range seen {
		paths = append(paths, p)
	}
	sort.Strings(paths)
	for _, p := range paths {
		keys = append(keys, schema.RepoFile{
			Path:    p,
			Content: seen[p],
		})
	}
	return keys
}

// splitGpgKeyTokens splits a gpgkey value on commas and whitespace.
var gpgKeySplitter = regexp.MustCompile(`[,\s]+`)

func splitGpgKeyTokens(s string) []string {
	s = strings.TrimSpace(s)
	if s == "" {
		return nil
	}
	return gpgKeySplitter.Split(s, -1)
}

// detectRepoProvidingPackages finds packages that own .repo files in
// /etc/yum.repos.d/.
func detectRepoProvidingPackages(exec Executor) []string {
	entries, err := exec.ReadDir("/etc/yum.repos.d")
	if err != nil {
		return []string{}
	}
	var repoFilePaths []string
	for _, e := range entries {
		if !e.IsDir() && filepath.Ext(e.Name()) == ".repo" {
			repoFilePaths = append(repoFilePaths, filepath.Join("/etc/yum.repos.d", e.Name()))
		}
	}
	if len(repoFilePaths) == 0 {
		return []string{}
	}

	args := append([]string{"-qf", "--queryformat", "%{NAME}\n"}, repoFilePaths...)
	result := exec.Run("rpm", args...)
	if result.ExitCode != 0 && strings.TrimSpace(result.Stdout) == "" {
		return []string{}
	}

	owners := make(map[string]bool)
	for _, line := range strings.Split(result.Stdout, "\n") {
		name := strings.TrimSpace(line)
		if name != "" && !strings.Contains(name, "is not owned by") {
			owners[name] = true
		}
	}
	return sortedKeys(owners)
}

// ---------------------------------------------------------------------------
// DNF module streams
// ---------------------------------------------------------------------------

// collectModuleStreams parses enabled/installed module streams from
// /etc/dnf/modules.d/*.module.
func collectModuleStreams(exec Executor) []schema.EnabledModuleStream {
	entries, err := exec.ReadDir("/etc/dnf/modules.d")
	if err != nil {
		return []schema.EnabledModuleStream{}
	}

	var streams []schema.EnabledModuleStream
	for _, entry := range entries {
		if entry.IsDir() || filepath.Ext(entry.Name()) != ".module" {
			continue
		}
		path := filepath.Join("/etc/dnf/modules.d", entry.Name())
		content, err := exec.ReadFile(path)
		if err != nil {
			continue
		}
		parsed := parseModuleINI(content)
		for modName, info := range parsed {
			streams = append(streams, schema.EnabledModuleStream{
				ModuleName: modName,
				Stream:     info.stream,
				Profiles:   info.profiles,
			})
		}
	}

	sort.Slice(streams, func(i, j int) bool {
		return streams[i].ModuleName < streams[j].ModuleName
	})
	return streams
}

type moduleInfo struct {
	stream   string
	profiles []string
}

// parseModuleINI parses INI-format module files. We implement a minimal INI
// parser rather than pulling in a dependency.
func parseModuleINI(text string) map[string]moduleInfo {
	result := make(map[string]moduleInfo)
	var currentSection string

	kv := make(map[string]map[string]string) // section → key → value

	for _, line := range strings.Split(text, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") || strings.HasPrefix(line, ";") {
			continue
		}
		if strings.HasPrefix(line, "[") && strings.HasSuffix(line, "]") {
			currentSection = line[1 : len(line)-1]
			if kv[currentSection] == nil {
				kv[currentSection] = make(map[string]string)
			}
			continue
		}
		if currentSection == "" {
			continue
		}
		if idx := strings.Index(line, "="); idx >= 0 {
			key := strings.TrimSpace(line[:idx])
			val := strings.TrimSpace(line[idx+1:])
			kv[currentSection][key] = val
		}
	}

	for section, vals := range kv {
		state := strings.ToLower(strings.TrimSpace(vals["state"]))
		if state != "enabled" && state != "installed" {
			continue
		}
		stream := strings.TrimSpace(vals["stream"])
		if stream == "" {
			continue
		}
		var profiles []string
		if p := strings.TrimSpace(vals["profiles"]); p != "" {
			for _, prof := range strings.Split(p, ",") {
				prof = strings.TrimSpace(prof)
				if prof != "" {
					profiles = append(profiles, prof)
				}
			}
		}
		if profiles == nil {
			profiles = []string{}
		}
		result[section] = moduleInfo{stream: stream, profiles: profiles}
	}
	return result
}

// ---------------------------------------------------------------------------
// Version locks
// ---------------------------------------------------------------------------

var versionlockNameRe = regexp.MustCompile(`-(\d)`)

// parseNEVRAPattern parses a versionlock NEVRA pattern into a VersionLockEntry.
func parseNEVRAPattern(rawLine string) (*schema.VersionLockEntry, error) {
	s := strings.TrimSpace(rawLine)

	var arch string
	if dotIdx := strings.LastIndex(s, "."); dotIdx >= 0 {
		rest := s[:dotIdx]
		arch = s[dotIdx+1:]
		s = rest
	}

	// Split epoch if colon before first dash.
	epoch := 0
	colonPos := strings.Index(s, ":")
	dashPos := strings.Index(s, "-")
	if colonPos >= 0 && (dashPos < 0 || colonPos < dashPos) {
		e := s[:colonPos]
		parsed := 0
		for _, ch := range e {
			if ch < '0' || ch > '9' {
				return nil, fmt.Errorf("invalid epoch in %q", rawLine)
			}
			parsed = parsed*10 + int(ch-'0')
		}
		epoch = parsed
		s = s[colonPos+1:]
	}

	// Name/version boundary: first '-' followed by a digit.
	loc := versionlockNameRe.FindStringIndex(s)
	if loc == nil {
		return nil, fmt.Errorf("cannot locate name/version boundary in %q", rawLine)
	}

	name := s[:loc[0]]
	verRel := s[loc[0]+1:]

	var version, release string
	if dashIdx := strings.LastIndex(verRel, "-"); dashIdx >= 0 {
		version = verRel[:dashIdx]
		release = verRel[dashIdx+1:]
	} else {
		version = verRel
	}

	return &schema.VersionLockEntry{
		RawPattern: strings.TrimSpace(rawLine),
		Name:       name,
		Epoch:      epoch,
		Version:    version,
		Release:    release,
		Arch:       arch,
	}, nil
}

// collectVersionLocks reads version locks from the versionlock file and
// dnf versionlock list.
func collectVersionLocks(exec Executor) ([]schema.VersionLockEntry, *string) {
	var entries []schema.VersionLockEntry

	// Try the file first.
	for _, path := range []string{
		"/etc/dnf/plugins/versionlock.list",
		"/etc/yum/pluginconf.d/versionlock.list",
	} {
		content, err := exec.ReadFile(path)
		if err != nil {
			continue
		}
		for _, line := range strings.Split(content, "\n") {
			line = strings.TrimSpace(line)
			if line == "" || strings.HasPrefix(line, "#") {
				continue
			}
			entry, err := parseNEVRAPattern(line)
			if err != nil {
				continue
			}
			entries = append(entries, *entry)
		}
		break // Use the first file found.
	}

	// Also try the dnf command.
	var commandOutput *string
	result := exec.Run("dnf", "versionlock", "list")
	if result.ExitCode == 0 {
		out := result.Stdout
		commandOutput = &out
	}

	return entries, commandOutput
}

// ---------------------------------------------------------------------------
// DNF history removed
// ---------------------------------------------------------------------------

var nameFromNevraRe = regexp.MustCompile(`^([^-]+(?:-[^-]+)*?)-\d`)

// dnfHistoryRemoved collects package names from Remove transactions in dnf
// history.
func dnfHistoryRemoved(exec Executor, warnings *[]Warning) []string {
	result := exec.Run("dnf", "history", "list", "-q")
	if result.ExitCode != 0 {
		if warnings != nil {
			*warnings = append(*warnings, makeWarning("rpm",
				"dnf history unavailable — orphaned config detection (packages removed after install) is incomplete."))
		}
		return []string{}
	}

	var removed []string
	for _, line := range strings.Split(result.Stdout, "\n") {
		parts := strings.Split(line, "|")
		if len(parts) < 4 {
			continue
		}
		action := strings.TrimSpace(parts[3])
		if !strings.Contains(action, "Removed") {
			continue
		}

		tidStr := strings.TrimSpace(parts[0])
		// Parse as integer to validate.
		tid := 0
		valid := true
		for _, ch := range tidStr {
			if ch < '0' || ch > '9' {
				valid = false
				break
			}
			tid = tid*10 + int(ch-'0')
		}
		if !valid || tid == 0 {
			continue
		}

		infoResult := exec.Run("dnf", "history", "info", tidStr, "-q")
		if infoResult.ExitCode != 0 {
			continue
		}
		for _, iline := range strings.Split(infoResult.Stdout, "\n") {
			if !strings.Contains(iline, "Removed") {
				continue
			}
			pkgPart := strings.TrimSpace(strings.SplitN(iline, "Removed", 2)[1])
			fields := strings.Fields(pkgPart)
			if len(fields) == 0 {
				continue
			}
			nevra := fields[0]
			match := nameFromNevraRe.FindStringSubmatch(nevra)
			if match != nil {
				removed = append(removed, match[1])
			} else if strings.Contains(nevra, "-") {
				removed = append(removed, strings.SplitN(nevra, "-", 2)[0])
			} else {
				removed = append(removed, nevra)
			}
		}
	}
	return removed
}

// ---------------------------------------------------------------------------
// rpm-ostree package state
// ---------------------------------------------------------------------------

// parseRpmOstreePackageState parses rpm-ostree status --json for layered,
// removed, and overridden packages.
func parseRpmOstreePackageState(exec Executor, section *schema.RpmSection, warnings *[]Warning, systemType schema.SystemType) {
	result := exec.Run("rpm-ostree", "status", "--json")
	if result.ExitCode != 0 {
		if systemType == schema.SystemTypeBootc && warnings != nil {
			*warnings = append(*warnings, makeWarning("rpm",
				"Package diff is approximate -- rpm-ostree status is not available on this bootc system. "+
					"Package detection used rpm -qa against the base image, which may differ due to tag drift "+
					"or NVR skew. Results require manual review.",
				"warning"))
		}
		return
	}

	var data map[string]interface{}
	if err := json.Unmarshal([]byte(result.Stdout), &data); err != nil {
		if warnings != nil {
			*warnings = append(*warnings, makeWarning("rpm",
				"rpm-ostree status returned invalid JSON. "+
					"Layered, overridden, and removed package information is unavailable."))
		}
		return
	}

	deploymentsRaw, ok := data["deployments"]
	if !ok {
		if warnings != nil {
			*warnings = append(*warnings, makeWarning("rpm",
				"rpm-ostree status returned no deployments. "+
					"Layered, overridden, and removed package information is unavailable."))
		}
		return
	}

	deployments, ok := deploymentsRaw.([]interface{})
	if !ok || len(deployments) == 0 {
		if warnings != nil {
			*warnings = append(*warnings, makeWarning("rpm",
				"rpm-ostree status returned no deployments. "+
					"Layered, overridden, and removed package information is unavailable."))
		}
		return
	}

	// Find the booted deployment.
	var booted map[string]interface{}
	for _, dep := range deployments {
		depMap, ok := dep.(map[string]interface{})
		if !ok {
			continue
		}
		if b, ok := depMap["booted"].(bool); ok && b {
			booted = depMap
			break
		}
	}
	if booted == nil {
		if warnings != nil {
			*warnings = append(*warnings, makeWarning("rpm",
				"rpm-ostree status has no booted deployment. "+
					"Layered, overridden, and removed package information is unavailable."))
		}
		return
	}

	// Layered packages.
	existingNames := make(map[string]bool)
	for _, p := range section.PackagesAdded {
		existingNames[p.Name] = true
	}
	if reqPkgs, ok := booted["requested-packages"].([]interface{}); ok {
		for _, pkg := range reqPkgs {
			name, ok := pkg.(string)
			if !ok || name == "" || existingNames[name] {
				continue
			}
			section.PackagesAdded = append(section.PackagesAdded, schema.PackageEntry{
				Name:  name,
				Epoch: "0",
				Arch:  "noarch",
				State: schema.PackageStateAdded,
			})
			existingNames[name] = true
		}
	}

	// Removed packages.
	if removals, ok := booted["base-removals"].([]interface{}); ok {
		for _, r := range removals {
			rMap, ok := r.(map[string]interface{})
			if !ok {
				continue
			}
			if name, ok := rMap["name"].(string); ok && name != "" {
				section.OstreeRemovals = append(section.OstreeRemovals, name)
			}
		}
	}

	// Overridden packages.
	if replacements, ok := booted["base-local-replacements"].([]interface{}); ok {
		for _, r := range replacements {
			rMap, ok := r.(map[string]interface{})
			if !ok {
				continue
			}
			name, _ := rMap["name"].(string)
			if name == "" {
				continue
			}
			toNevra, _ := rMap["nevra"].(string)
			fromNevra, _ := rMap["base-nevra"].(string)
			section.OstreeOverrides = append(section.OstreeOverrides, schema.OstreePackageOverride{
				Name:      name,
				ToNevra:   toNevra,
				FromNevra: fromNevra,
			})
		}
	}
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

func isDigits(s string) bool {
	if s == "" {
		return false
	}
	for _, r := range s {
		if !unicode.IsDigit(r) {
			return false
		}
	}
	return true
}

func isDigit(b byte) bool { return b >= '0' && b <= '9' }
func isAlpha(b byte) bool { return (b >= 'a' && b <= 'z') || (b >= 'A' && b <= 'Z') }
func isAlnum(b byte) bool { return isDigit(b) || isAlpha(b) }

func epochInt(s string) int {
	if s == "" || s == "(none)" {
		return 0
	}
	n := 0
	for _, ch := range s {
		if ch < '0' || ch > '9' {
			return 0
		}
		n = n*10 + int(ch-'0')
	}
	return n
}

// sortedKeys returns sorted keys from various map types.
func sortedKeys[V any](m map[string]V) []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}

func intersect(a, b map[string]bool) map[string]bool {
	result := make(map[string]bool)
	for k := range a {
		if b[k] {
			result[k] = true
		}
	}
	return result
}
