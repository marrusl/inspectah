// Package inspector — orchestrator runs all 11 inspectors in sequence.
//
// RunAll is the main entry point for a full inspection. It handles
// system-type detection, os-release reading, cross-version warnings,
// RPM-owned path sharing, and safe execution (inspector failures are
// captured as warnings, not hard errors).
package inspector

import (
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

// InspectOptions configures the full inspection pipeline.
type InspectOptions struct {
	// ConfigDiffs enables unified diff output for modified config files.
	ConfigDiffs bool

	// DeepBinaryScan enables full strings analysis for ELF binaries.
	DeepBinaryScan bool

	// QueryPodman enables querying running containers via podman.
	QueryPodman bool

	// TargetVersion is the target RHEL version (e.g., "9.4").
	TargetVersion string

	// TargetImage is the target base image reference.
	TargetImage string

	// NoBaseline opts in to degraded all-packages mode.
	NoBaseline bool

	// BaselinePackages is a pre-loaded baseline map (name.arch → entry).
	// nil means no baseline is available.
	BaselinePackages map[string]schema.PackageEntry

	// UserStrategyOverride forces a specific user migration strategy.
	UserStrategyOverride string

	// Version is the inspectah version string for metadata.
	Version string
}

// totalInspectors is the number of inspection sections.
const totalInspectors = 11

// RunAll runs system-type detection and all 11 inspectors, returning
// a populated InspectionSnapshot. Individual inspector failures are
// captured as warnings — they do not abort the pipeline.
func RunAll(exec Executor, opts InspectOptions) (*schema.InspectionSnapshot, error) {
	snapshot := schema.NewSnapshot()
	warnings := &snapshot.Warnings

	// --- Metadata ---
	snapshot.Meta["timestamp"] = time.Now().UTC().Format(time.RFC3339)
	if opts.Version != "" {
		snapshot.Meta["inspectah_version"] = opts.Version
	}
	populateHostname(exec, snapshot)

	// --- os-release ---
	osRelease := readOsRelease(exec)
	snapshot.OsRelease = osRelease

	if err := validateSupportedHost(osRelease); err != "" {
		return nil, fmt.Errorf("%s", err)
	}

	// --- System-type detection ---
	// DetectSystemType lives in the pipeline package; we accept it as a
	// pre-resolved value or detect it here via the executor.
	systemType, stErr := detectSystemTypeLocal(exec)
	if stErr != nil {
		return nil, stErr
	}
	snapshot.SystemType = systemType

	// Cross-major-version warning
	if opts.TargetVersion != "" && osRelease != nil && osRelease.VersionID != "" {
		sourceMajor := strings.SplitN(osRelease.VersionID, ".", 2)[0]
		targetMajor := strings.SplitN(opts.TargetVersion, ".", 2)[0]
		if sourceMajor != targetMajor {
			msg := fmt.Sprintf(
				"Source host is %s %s but target image is version %s. "+
					"Cross-major-version migration may require significant manual adjustment. "+
					"Package names, service names, and config formats may have changed.",
				strings.ToUpper(osRelease.ID), osRelease.VersionID, opts.TargetVersion,
			)
			*warnings = append(*warnings, makeWarning("pipeline", msg, "error"))
			sectionBanner("Cross-version warning", 0, totalInspectors)
		}
	}

	// Resolve base image for the RPM inspector
	baseImage := opts.TargetImage

	// --- Inspector execution (safe pattern) ---

	// 1. RPM
	sectionBanner("Packages", 1, totalInspectors)
	rpmSection, rpmWarn := safeRun("rpm", func() (*schema.RpmSection, []Warning, error) {
		return RunRpm(exec, RpmOptions{
			BaselinePackages: opts.BaselinePackages,
			SystemType:       systemType,
			TargetVersion:    opts.TargetVersion,
			TargetImage:      opts.TargetImage,
			BaseImage:        baseImage,
		})
	})
	snapshot.Rpm = rpmSection
	*warnings = append(*warnings, rpmWarn...)

	// Build RPM-owned path set once — shared by Config and Scheduled Tasks
	rpmOwned, ownedWarn := BuildRpmOwnedPaths(exec)
	*warnings = append(*warnings, ownedWarn...)

	// Extract rpm -Va from RPM output for Config inspector
	var rpmVa []schema.RpmVaEntry
	var removedPackages []string
	if snapshot.Rpm != nil {
		rpmVa = snapshot.Rpm.RpmVa
		removedPackages = snapshot.Rpm.DnfHistoryRemoved
	}

	// 2. Config
	sectionBanner("Config files", 2, totalInspectors)
	configSection, configWarn := safeRun("config", func() (*schema.ConfigSection, []Warning, error) {
		return RunConfig(exec, ConfigOptions{
			RpmVa:           rpmVa,
			RpmOwnedPaths:   rpmOwned,
			ConfigDiffs:     opts.ConfigDiffs,
			SystemType:      systemType,
			RemovedPackages: removedPackages,
		})
	})
	snapshot.Config = configSection
	*warnings = append(*warnings, configWarn...)

	// 3. Services
	sectionBanner("Services", 3, totalInspectors)
	serviceSection, serviceWarn := safeRun("service", func() (*schema.ServiceSection, []Warning, error) {
		return RunServices(exec, ServiceOptions{
			SystemType: systemType,
			// BaseImagePresetText is Phase 3 (requires BaselineResolver)
		})
	})
	snapshot.Services = serviceSection
	*warnings = append(*warnings, serviceWarn...)

	// 4. Network
	sectionBanner("Network", 4, totalInspectors)
	networkSection, networkWarn := safeRun("network", func() (*schema.NetworkSection, []Warning, error) {
		return RunNetwork(exec, NetworkOptions{
			SystemType: systemType,
		})
	})
	snapshot.Network = networkSection
	*warnings = append(*warnings, networkWarn...)

	// 5. Storage
	sectionBanner("Storage", 5, totalInspectors)
	storageSection, storageWarn := safeRun("storage", func() (*schema.StorageSection, []Warning, error) {
		return RunStorage(exec, StorageOptions{
			SystemType: systemType,
		})
	})
	snapshot.Storage = storageSection
	*warnings = append(*warnings, storageWarn...)

	// 6. Scheduled Tasks
	sectionBanner("Scheduled tasks", 6, totalInspectors)
	scheduledSection, scheduledWarn := safeRun("scheduled_tasks", func() (*schema.ScheduledTaskSection, []Warning, error) {
		return RunScheduledTasks(exec, ScheduledTaskOptions{
			RpmOwnedPaths: rpmOwned,
			SystemType:    systemType,
		})
	})
	snapshot.ScheduledTasks = scheduledSection
	*warnings = append(*warnings, scheduledWarn...)

	// 7. Containers
	sectionBanner("Containers", 7, totalInspectors)
	containerSection, containerWarn := safeRun("containers", func() (*schema.ContainerSection, []Warning, error) {
		return RunContainers(exec, ContainerOptions{
			QueryPodman: opts.QueryPodman,
			SystemType:  systemType,
		})
	})
	snapshot.Containers = containerSection
	*warnings = append(*warnings, containerWarn...)

	// 8. Non-RPM Software
	sectionBanner("Non-RPM software", 8, totalInspectors)
	nonRpmSection, nonRpmWarn := safeRun("non_rpm_software", func() (*schema.NonRpmSoftwareSection, []Warning, error) {
		return RunNonRpmSoftware(exec, NonRpmOptions{
			DeepBinaryScan: opts.DeepBinaryScan,
			SystemType:     systemType,
		})
	})
	snapshot.NonRpmSoftware = nonRpmSection
	*warnings = append(*warnings, nonRpmWarn...)

	// 9. Kernel / Boot
	sectionBanner("Kernel / boot", 9, totalInspectors)
	kernelSection, kernelWarn := safeRun("kernel_boot", func() (*schema.KernelBootSection, []Warning, error) {
		return RunKernelBoot(exec, KernelBootOptions{
			SystemType: systemType,
		})
	})
	snapshot.KernelBoot = kernelSection
	*warnings = append(*warnings, kernelWarn...)

	// 10. SELinux / Security
	sectionBanner("SELinux / security", 10, totalInspectors)
	selinuxSection, selinuxWarn := safeRun("selinux", func() (*schema.SelinuxSection, []Warning, error) {
		return RunSelinux(exec, SelinuxOptions{
			RpmOwnedPaths: rpmOwned,
		})
	})
	snapshot.Selinux = selinuxSection
	*warnings = append(*warnings, selinuxWarn...)

	// 11. Users / Groups
	sectionBanner("Users / groups", 11, totalInspectors)
	usersSection, usersWarn := safeRun("users_groups", func() (*schema.UserGroupSection, []Warning, error) {
		return RunUsersGroups(exec, UserGroupOptions{
			UserStrategyOverride: opts.UserStrategyOverride,
		})
	})
	snapshot.UsersGroups = usersSection
	*warnings = append(*warnings, usersWarn...)

	return snapshot, nil
}

// ---------------------------------------------------------------------------
// Safe execution pattern
// ---------------------------------------------------------------------------

// inspectorFunc is a function that returns a section pointer, warnings,
// and an error — matching the signature of all Run* inspectors.
type inspectorFunc[T any] func() (*T, []Warning, error)

// safeRun executes an inspector function with panic recovery and error
// capture. If the inspector panics or returns an error, the section is
// nil and a warning is appended — matching the Python _safe_run() pattern
// where inspector failures are non-fatal.
func safeRun[T any](name string, fn inspectorFunc[T]) (result *T, warnings []Warning) {
	defer func() {
		if r := recover(); r != nil {
			warnings = append(warnings, makeWarning(name, fmt.Sprintf("%s inspector panicked: %v", name, r)))
			fmt.Fprintf(os.Stderr, "WARNING: %s inspector panicked: %v\n", name, r)
			result = nil
		}
	}()

	section, warn, err := fn()
	if err != nil {
		warn = append(warn, makeWarning(name, fmt.Sprintf("%s inspector: %v", name, err)))
		fmt.Fprintf(os.Stderr, "WARNING: %s inspector skipped: %v\n", name, err)
		return nil, warn
	}
	return section, warn
}

// ---------------------------------------------------------------------------
// os-release reading
// ---------------------------------------------------------------------------

// readOsRelease reads /etc/os-release from the host via the executor.
func readOsRelease(exec Executor) *schema.OsRelease {
	content, err := exec.ReadFile("/etc/os-release")
	if err != nil {
		return nil
	}

	data := map[string]string{}
	for _, line := range strings.Split(content, "\n") {
		if idx := strings.Index(line, "="); idx >= 0 {
			key := line[:idx]
			val := strings.Trim(line[idx+1:], " \t\"")
			data[key] = val
		}
	}

	return &schema.OsRelease{
		Name:      data["NAME"],
		VersionID: data["VERSION_ID"],
		Version:   data["VERSION"],
		ID:        data["ID"],
		IDLike:    data["ID_LIKE"],
		PrettyName: data["PRETTY_NAME"],
		VariantID: data["VARIANT_ID"],
	}
}

// supportedRHELMajors are the supported RHEL major versions.
var supportedRHELMajors = map[string]bool{"9": true, "10": true}

// supportedCentOSMajors are the supported CentOS major versions.
var supportedCentOSMajors = map[string]bool{"9": true, "10": true}

// validateSupportedHost returns an error message if the host OS is
// unsupported, or empty string if OK.
func validateSupportedHost(osRelease *schema.OsRelease) string {
	if osRelease == nil || osRelease.VersionID == "" {
		return ""
	}
	major := strings.SplitN(osRelease.VersionID, ".", 2)[0]
	osID := strings.ToLower(osRelease.ID)

	supported := buildSupportedList()

	if osID == "rhel" {
		if !supportedRHELMajors[major] {
			return fmt.Sprintf(
				"Host is running RHEL %s. This version of inspectah supports %s.",
				osRelease.VersionID, supported,
			)
		}
	} else if strings.Contains(osID, "centos") {
		if !supportedCentOSMajors[major] {
			return fmt.Sprintf(
				"Host is running CentOS %s. This version of inspectah supports %s.",
				osRelease.VersionID, supported,
			)
		}
	}
	return ""
}

// buildSupportedList produces the human-readable supported OS list.
func buildSupportedList() string {
	var parts []string
	for _, m := range sortedMapKeys(supportedRHELMajors) {
		parts = append(parts, fmt.Sprintf("RHEL %s.x", m))
	}
	for _, m := range sortedMapKeys(supportedCentOSMajors) {
		parts = append(parts, fmt.Sprintf("CentOS Stream %s", m))
	}
	parts = append(parts, "Fedora")
	return strings.Join(parts, ", ")
}

// sortedMapKeys returns sorted keys from a map[string]bool.
func sortedMapKeys(m map[string]bool) []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	// Simple sort — major version numbers only
	for i := 0; i < len(keys); i++ {
		for j := i + 1; j < len(keys); j++ {
			if keys[j] < keys[i] {
				keys[i], keys[j] = keys[j], keys[i]
			}
		}
	}
	return keys
}

// ---------------------------------------------------------------------------
// System-type detection (local delegation)
// ---------------------------------------------------------------------------

// detectSystemTypeLocal detects the system type. This duplicates the
// pipeline.DetectSystemType logic so the orchestrator stays self-contained
// within the inspector package. The pipeline package's DetectSystemType
// is the canonical version; this is the same algorithm.
func detectSystemTypeLocal(exec Executor) (schema.SystemType, error) {
	if !exec.FileExists("/ostree") {
		return schema.SystemTypePackageMode, nil
	}

	result := exec.Run("bootc", "status")
	if result.ExitCode == 0 {
		return schema.SystemTypeBootc, nil
	}

	result = exec.Run("rpm-ostree", "status")
	if result.ExitCode == 0 {
		return schema.SystemTypeRpmOstree, nil
	}

	return "", fmt.Errorf(
		"detected ostree system (/ostree exists) but could not determine " +
			"system type — both 'bootc status' and 'rpm-ostree status' failed",
	)
}

// ---------------------------------------------------------------------------
// Hostname resolution
// ---------------------------------------------------------------------------

// populateHostname resolves the hostname for snapshot metadata.
// Priority: INSPECTAH_HOSTNAME env → /etc/hostname → hostnamectl hostname.
func populateHostname(exec Executor, snapshot *schema.InspectionSnapshot) {
	if envHost := strings.TrimSpace(os.Getenv("INSPECTAH_HOSTNAME")); envHost != "" {
		snapshot.Meta["hostname"] = envHost
		return
	}

	content, err := exec.ReadFile("/etc/hostname")
	if err == nil {
		lines := strings.SplitN(content, "\n", 2)
		if name := strings.TrimSpace(lines[0]); name != "" {
			snapshot.Meta["hostname"] = name
			return
		}
	}

	result := exec.Run("hostnamectl", "hostname")
	if result.ExitCode == 0 {
		if name := strings.TrimSpace(result.Stdout); name != "" {
			snapshot.Meta["hostname"] = name
		}
	}
}

// ---------------------------------------------------------------------------
// Section banner
// ---------------------------------------------------------------------------

// sectionBanner prints a progress banner to stderr.
func sectionBanner(name string, step, total int) {
	fmt.Fprintf(os.Stderr, "[%d/%d] %s\n", step, total, name)
}
