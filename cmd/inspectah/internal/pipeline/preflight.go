// Package pipeline — RPM preflight check.
//
// Validates package availability against the target base image's repos
// before rendering the Containerfile. Runs a two-phase check inside a
// temporary container via nsenter:
//
//   Phase 1: Bootstrap repo-providing packages (e.g., epel-release)
//   Phase 2: dnf repoquery --available to check package existence
//
// Results are stored as schema.PreflightResult in the InspectionSnapshot.
package pipeline

import (
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/inspector"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

// nsenterPrefix runs commands in the host's PID 1 namespaces (the
// inspectah container does not have podman installed).
var nsenterPrefix = []string{"nsenter", "-t", "1", "-m", "-u", "-i", "-n", "--"}

// directInstallRepos are source_repo values that mean "not from a repo".
var directInstallRepos = map[string]bool{
	"":             true,
	"(none)":       true,
	"commandline":  true,
	"(commandline)": true,
	"installed":    true,
}

// repoFailurePatterns are DNF stderr strings indicating repo metadata
// download failure.
var repoFailurePatterns = []string{
	"Failed to synchronize cache for repo",
	"Failed to download metadata for repo",
	"Cannot download repomd.xml",
	"Errors during downloading metadata for repository",
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

// PreflightOptions configures the preflight check.
type PreflightOptions struct {
	// Snapshot is the populated inspection snapshot (needs RPM data).
	Snapshot *schema.InspectionSnapshot

	// ContainerName is an optional override for the preflight container
	// name. When empty, a timestamped name is generated.
	ContainerName string
}

// RunPreflight runs the package availability preflight check.
//
// It classifies packages as available, unavailable, or unverifiable
// relative to the target base image's repos. Direct-install packages
// (no repo origin) are noted separately.
func RunPreflight(exec inspector.Executor, opts PreflightOptions) (*schema.PreflightResult, error) {
	timestamp := time.Now().UTC().Format(time.RFC3339)
	snap := opts.Snapshot

	// --- Prerequisites ---
	baseImage := ""
	if snap.Rpm != nil && snap.Rpm.BaseImage != nil {
		baseImage = *snap.Rpm.BaseImage
	}
	if baseImage == "" {
		result := &schema.PreflightResult{
			Status:      "failed",
			Timestamp:   timestamp,
			Available:   []string{},
			Unavailable: []string{},
		}
		reason := "No base image configured — cannot run preflight check"
		result.StatusReason = &reason
		return result, nil
	}

	// --- Classify direct installs vs repo packages ---
	repoPackages, directInstalls := classifyDirectInstalls(snap)

	if len(repoPackages) == 0 {
		return &schema.PreflightResult{
			Status:        "completed",
			DirectInstall: directInstalls,
			BaseImage:     baseImage,
			Timestamp:     timestamp,
			Available:     []string{},
			Unavailable:   []string{},
		}, nil
	}

	// --- Pull the base image (via nsenter) ---
	pullResult := exec.Run(nsenterPrefix[0], append(nsenterPrefix[1:], "podman", "pull", "-q", baseImage)...)
	if pullResult.ExitCode != 0 {
		reason := fmt.Sprintf("Base image %s could not be pulled: %s", baseImage, truncate(pullResult.Stderr, 200))
		return &schema.PreflightResult{
			Status:        "failed",
			StatusReason:  &reason,
			DirectInstall: directInstalls,
			BaseImage:     baseImage,
			Timestamp:     timestamp,
			Available:     []string{},
			Unavailable:   []string{},
		}, nil
	}

	// --- Start persistent preflight container ---
	containerName := opts.ContainerName
	if containerName == "" {
		containerName = fmt.Sprintf("inspectah-preflight-%d", time.Now().UnixNano())
	}

	runArgs := append(nsenterPrefix[1:], "podman", "run", "-d", "--name", containerName, baseImage, "sleep", "infinity")
	startResult := exec.Run(nsenterPrefix[0], runArgs...)
	if startResult.ExitCode != 0 {
		reason := fmt.Sprintf("Could not start preflight container: %s", truncate(startResult.Stderr, 200))
		return &schema.PreflightResult{
			Status:        "failed",
			StatusReason:  &reason,
			DirectInstall: directInstalls,
			BaseImage:     baseImage,
			Timestamp:     timestamp,
			Available:     []string{},
			Unavailable:   []string{},
		}, nil
	}

	// Always clean up
	defer func() {
		exec.Run(nsenterPrefix[0], append(nsenterPrefix[1:], "podman", "rm", "-f", containerName)...)
	}()

	return runChecksInContainer(exec, snap, containerName, baseImage, repoPackages, directInstalls, timestamp)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

// classifyDirectInstalls splits packages_added into (repo, direct) sets.
// A direct-install package has no repo origin and cannot be verified via
// repoquery.
func classifyDirectInstalls(snap *schema.InspectionSnapshot) (repoPackages, directInstalls []string) {
	if snap.Rpm == nil || len(snap.Rpm.PackagesAdded) == 0 {
		return nil, nil
	}

	// Build source_repo lookup from packages_added
	sourceRepos := map[string]string{}
	for _, p := range snap.Rpm.PackagesAdded {
		if _, exists := sourceRepos[p.Name]; !exists {
			sourceRepos[p.Name] = p.SourceRepo
		}
	}

	// Use packages_added names as the install set (simplified — the full
	// resolve_install_set is Phase 3). Only packages with include=true.
	seen := map[string]bool{}
	for _, p := range snap.Rpm.PackagesAdded {
		if !p.Include || seen[p.Name] {
			continue
		}
		seen[p.Name] = true

		src := strings.TrimSpace(strings.ToLower(sourceRepos[p.Name]))
		if directInstallRepos[src] {
			directInstalls = append(directInstalls, p.Name)
		} else {
			repoPackages = append(repoPackages, p.Name)
		}
	}

	sort.Strings(repoPackages)
	sort.Strings(directInstalls)
	return repoPackages, directInstalls
}

// runChecksInContainer runs bootstrap + repoquery inside an already-running
// container.
func runChecksInContainer(
	exec inspector.Executor,
	snap *schema.InspectionSnapshot,
	containerName, baseImage string,
	repoPackages, directInstalls []string,
	timestamp string,
) (*schema.PreflightResult, error) {

	execBase := append(nsenterPrefix, "podman", "exec", containerName)

	// --- Phase 1: Bootstrap repo-providing packages ---
	var bootstrapFailed map[string]bool
	repoProviders := snap.Rpm.RepoProvidingPackages
	if len(repoProviders) > 0 {
		bootstrapArgs := append(execBase, "dnf", "install", "-y")
		bootstrapArgs = append(bootstrapArgs, repoProviders...)
		bootstrapResult := exec.Run(bootstrapArgs[0], bootstrapArgs[1:]...)
		if bootstrapResult.ExitCode != 0 {
			bootstrapFailed = map[string]bool{}
			for _, p := range repoProviders {
				bootstrapFailed[p] = true
			}
		}
	}

	// --- Phase 2: dnf repoquery --available ---
	queryArgs := append(execBase, "dnf", "repoquery", "--available", "--queryformat", "%{name}")
	queryArgs = append(queryArgs, repoPackages...)
	queryResult := exec.Run(queryArgs[0], queryArgs[1:]...)

	// Detect unreachable repos from stderr
	repoUnreachable := DetectUnreachableRepos(queryResult.Stderr)

	// Build source_repo lookup
	sourceRepos := map[string]string{}
	if snap.Rpm != nil {
		for _, p := range snap.Rpm.PackagesAdded {
			if _, exists := sourceRepos[p.Name]; !exists {
				sourceRepos[p.Name] = p.SourceRepo
			}
		}
	}

	// Populate affected_packages on unreachable repos
	for i := range repoUnreachable {
		var affected []string
		for _, name := range repoPackages {
			if sourceRepos[name] == repoUnreachable[i].RepoID {
				affected = append(affected, name)
			}
		}
		sort.Strings(affected)
		repoUnreachable[i].AffectedPackages = affected
	}

	// Query available repo list
	repolistArgs := append(execBase, "dnf", "repolist", "--quiet")
	repolistResult := exec.Run(repolistArgs[0], repolistArgs[1:]...)
	var reposQueried []string
	if repolistResult.ExitCode == 0 {
		for _, line := range strings.Split(repolistResult.Stdout, "\n") {
			fields := strings.Fields(line)
			if len(fields) > 0 {
				reposQueried = append(reposQueried, fields[0])
			}
		}
	}

	// Total failure — no results at all
	if queryResult.ExitCode != 0 && strings.TrimSpace(queryResult.Stdout) == "" {
		if len(repoUnreachable) > 0 {
			unreachableIDs := map[string]bool{}
			for _, rs := range repoUnreachable {
				unreachableIDs[rs.RepoID] = true
			}
			allRepoIDs := map[string]bool{}
			for id := range unreachableIDs {
				allRepoIDs[id] = true
			}
			for _, id := range reposQueried {
				allRepoIDs[id] = true
			}
			allUnreachable := len(allRepoIDs) > 0 && len(unreachableIDs) >= len(allRepoIDs)
			if allUnreachable {
				reason := "all repos unreachable — no meaningful validation possible"
				return &schema.PreflightResult{
					Status:          "failed",
					StatusReason:    &reason,
					DirectInstall:   directInstalls,
					RepoUnreachable: repoUnreachable,
					BaseImage:       baseImage,
					ReposQueried:    reposQueried,
					Timestamp:       timestamp,
					Available:       []string{},
					Unavailable:     []string{},
				}, nil
			}
			reason := fmt.Sprintf("%d repo(s) unreachable", len(repoUnreachable))
			return &schema.PreflightResult{
				Status:          "partial",
				StatusReason:    &reason,
				DirectInstall:   directInstalls,
				RepoUnreachable: repoUnreachable,
				BaseImage:       baseImage,
				ReposQueried:    reposQueried,
				Timestamp:       timestamp,
				Available:       []string{},
				Unavailable:     []string{},
			}, nil
		}
		reason := fmt.Sprintf("dnf repoquery failed: %s", truncate(queryResult.Stderr, 200))
		return &schema.PreflightResult{
			Status:       "failed",
			StatusReason: &reason,
			BaseImage:    baseImage,
			ReposQueried: reposQueried,
			Timestamp:    timestamp,
			Available:    []string{},
			Unavailable:  []string{},
		}, nil
	}

	// Parse results — each line is a plain package name
	foundNames := map[string]bool{}
	for _, line := range strings.Split(queryResult.Stdout, "\n") {
		name := strings.TrimSpace(line)
		if name != "" {
			foundNames[name] = true
		}
	}

	var available, notFound []string
	for _, name := range repoPackages {
		if foundNames[name] {
			available = append(available, name)
		} else {
			notFound = append(notFound, name)
		}
	}
	sort.Strings(available)
	sort.Strings(notFound)

	// Classify not-found: unavailable vs unverifiable
	var unavailable []string
	var unverifiable []schema.UnverifiablePackage

	if len(bootstrapFailed) > 0 && len(notFound) > 0 {
		failedProviderNames := sortedKeys(bootstrapFailed)
		for _, pkg := range notFound {
			pkgSource := strings.TrimSpace(strings.ToLower(sourceRepos[pkg]))
			isFromFailedProvider := false
			for provider := range bootstrapFailed {
				if pkgSource == strings.ToLower(provider) {
					isFromFailedProvider = true
					break
				}
			}
			if isFromFailedProvider {
				unverifiable = append(unverifiable, schema.UnverifiablePackage{
					Name:   pkg,
					Reason: fmt.Sprintf("repo-providing package(s) %s unavailable", strings.Join(failedProviderNames, ", ")),
				})
			} else {
				unavailable = append(unavailable, pkg)
			}
		}
	} else {
		unavailable = notFound
	}

	// Remove packages from unavailable whose source_repo matches
	// an unreachable repo — those are tracked via RepoUnreachable.
	if len(repoUnreachable) > 0 && len(unavailable) > 0 {
		unreachableIDs := map[string]bool{}
		for _, rs := range repoUnreachable {
			unreachableIDs[rs.RepoID] = true
		}
		filtered := unavailable[:0]
		for _, pkg := range unavailable {
			if !unreachableIDs[sourceRepos[pkg]] {
				filtered = append(filtered, pkg)
			}
		}
		unavailable = filtered
	}

	// Determine status
	var status string
	var statusReason *string
	if len(unverifiable) > 0 || len(repoUnreachable) > 0 {
		status = "partial"
		var reasons []string
		if len(bootstrapFailed) > 0 {
			failedProviderNames := sortedKeys(bootstrapFailed)
			reasons = append(reasons, fmt.Sprintf(
				"repo-providing package(s) %s unavailable; %d package(s) unverifiable",
				strings.Join(failedProviderNames, ", "), len(unverifiable),
			))
		}
		if len(repoUnreachable) > 0 {
			reasons = append(reasons, fmt.Sprintf("%d repo(s) unreachable", len(repoUnreachable)))
		}
		if len(reasons) > 0 {
			joined := strings.Join(reasons, "; ")
			statusReason = &joined
		}
	} else {
		status = "completed"
	}

	if available == nil {
		available = []string{}
	}
	if unavailable == nil {
		unavailable = []string{}
	}

	return &schema.PreflightResult{
		Status:          status,
		StatusReason:    statusReason,
		Available:       available,
		Unavailable:     unavailable,
		Unverifiable:    unverifiable,
		DirectInstall:   directInstalls,
		RepoUnreachable: repoUnreachable,
		BaseImage:       baseImage,
		ReposQueried:    reposQueried,
		Timestamp:       timestamp,
	}, nil
}

// DetectUnreachableRepos parses DNF stderr for repo failure messages.
// Returns a list of RepoStatus for repos whose metadata could not be
// downloaded.
func DetectUnreachableRepos(stderr string) []schema.RepoStatus {
	var unreachable []schema.RepoStatus
	seen := map[string]bool{}

	for _, line := range strings.Split(stderr, "\n") {
		for _, pattern := range repoFailurePatterns {
			if !strings.Contains(line, pattern) {
				continue
			}
			repoID := extractQuotedRepoID(line)
			if repoID != "" && !seen[repoID] {
				seen[repoID] = true
				unreachable = append(unreachable, schema.RepoStatus{
					RepoID:   repoID,
					RepoName: repoID,
					Error:    strings.TrimSpace(line),
				})
			}
			break
		}
	}
	return unreachable
}

// extractQuotedRepoID pulls a repo ID from single or double quotes in a
// DNF error line, e.g. "Failed to synchronize cache for repo 'epel'".
func extractQuotedRepoID(line string) string {
	for _, quote := range []byte{'\'', '"'} {
		q := string(quote)
		parts := strings.SplitN(line, q, 3)
		if len(parts) >= 3 {
			return parts[1]
		}
	}
	return ""
}

// truncate returns at most n bytes of s.
func truncate(s string, n int) string {
	if len(s) <= n {
		return strings.TrimSpace(s)
	}
	return strings.TrimSpace(s[:n])
}

// sortedKeys returns the keys of a map[string]bool in sorted order.
func sortedKeys(m map[string]bool) []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}
