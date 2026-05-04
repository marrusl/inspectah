// Package schema defines the inspection snapshot types ported from
// src/inspectah/schema.py. Every struct serialises to JSON with the same
// snake_case field names that the Python Pydantic models produce, so Go
// and Python outputs are golden-file compatible.
package schema

import (
	"encoding/json"
	"fmt"
)

// SchemaVersion is the current inspection snapshot schema version.
const SchemaVersion = 12

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

// SystemType is the detected source system type.
type SystemType string

const (
	SystemTypePackageMode SystemType = "package-mode"
	SystemTypeRpmOstree   SystemType = "rpm-ostree"
	SystemTypeBootc       SystemType = "bootc"
)

// MarshalJSON serialises a SystemType as a JSON string.
func (s SystemType) MarshalJSON() ([]byte, error) {
	return json.Marshal(string(s))
}

// UnmarshalJSON deserialises a JSON string into a SystemType, rejecting
// unknown values.
func (s *SystemType) UnmarshalJSON(data []byte) error {
	var raw string
	if err := json.Unmarshal(data, &raw); err != nil {
		return err
	}
	switch SystemType(raw) {
	case SystemTypePackageMode, SystemTypeRpmOstree, SystemTypeBootc:
		*s = SystemType(raw)
		return nil
	default:
		return fmt.Errorf("unknown SystemType %q", raw)
	}
}

// PackageState describes how a package relates to the base image.
type PackageState string

const (
	PackageStateAdded        PackageState = "added"
	PackageStateBaseImageOnly PackageState = "base_image_only"
	PackageStateModified     PackageState = "modified"
	PackageStateLocalInstall PackageState = "local_install"
	PackageStateNoRepo       PackageState = "no_repo"
)

// VersionChangeDirection indicates whether a package was upgraded or
// downgraded relative to the base image.
type VersionChangeDirection string

const (
	VersionChangeUpgrade   VersionChangeDirection = "upgrade"
	VersionChangeDowngrade VersionChangeDirection = "downgrade"
)

// ConfigFileKind classifies the origin of a captured config file.
type ConfigFileKind string

const (
	ConfigFileKindRpmOwnedDefault  ConfigFileKind = "rpm_owned_default"
	ConfigFileKindRpmOwnedModified ConfigFileKind = "rpm_owned_modified"
	ConfigFileKindUnowned          ConfigFileKind = "unowned"
	ConfigFileKindOrphaned         ConfigFileKind = "orphaned"
)

// ConfigCategory is a semantic category derived from a config file path.
type ConfigCategory string

const (
	ConfigCategoryTmpfiles      ConfigCategory = "tmpfiles"
	ConfigCategoryEnvironment   ConfigCategory = "environment"
	ConfigCategoryAudit         ConfigCategory = "audit"
	ConfigCategoryLibraryPath   ConfigCategory = "library_path"
	ConfigCategoryJournal       ConfigCategory = "journal"
	ConfigCategoryLogrotate     ConfigCategory = "logrotate"
	ConfigCategoryAutomount     ConfigCategory = "automount"
	ConfigCategorySysctl        ConfigCategory = "sysctl"
	ConfigCategoryCryptoPolicy  ConfigCategory = "crypto_policy"
	ConfigCategoryIdentity      ConfigCategory = "identity"
	ConfigCategoryLimits        ConfigCategory = "limits"
	ConfigCategoryOther         ConfigCategory = "other"
)

// ---------------------------------------------------------------------------
// Metadata types
// ---------------------------------------------------------------------------

// OsRelease mirrors /etc/os-release fields.
type OsRelease struct {
	Name      string `json:"name"`
	VersionID string `json:"version_id"`
	Version   string `json:"version"`
	ID        string `json:"id"`
	IDLike    string `json:"id_like"`
	PrettyName string `json:"pretty_name"`
	VariantID string `json:"variant_id"`
}

// FleetPrevalence carries fleet-wide prevalence data for a merged item.
type FleetPrevalence struct {
	Count int      `json:"count"`
	Total int      `json:"total"`
	Hosts []string `json:"hosts"`
}

// FleetMeta is fleet-level metadata for a merged snapshot.
type FleetMeta struct {
	SourceHosts   []string `json:"source_hosts"`
	TotalHosts    int      `json:"total_hosts"`
	MinPrevalence int      `json:"min_prevalence"`
}

// ---------------------------------------------------------------------------
// RPM types
// ---------------------------------------------------------------------------

// VersionChange records a package whose version differs between host and
// base image.
type VersionChange struct {
	Name        string                 `json:"name"`
	Arch        string                 `json:"arch"`
	HostVersion string                 `json:"host_version"`
	BaseVersion string                 `json:"base_version"`
	HostEpoch   string                 `json:"host_epoch"`
	BaseEpoch   string                 `json:"base_epoch"`
	Direction   VersionChangeDirection `json:"direction"`
}

// PackageEntry is a single package from rpm -qa or baseline diff.
type PackageEntry struct {
	Name         string           `json:"name"`
	Epoch        string           `json:"epoch"`
	Version      string           `json:"version"`
	Release      string           `json:"release"`
	Arch         string           `json:"arch"`
	State        PackageState     `json:"state"`
	Include      bool             `json:"include"`
	Acknowledged bool             `json:"acknowledged,omitempty"`
	SourceRepo   string           `json:"source_repo"`
	Fleet        *FleetPrevalence `json:"fleet"`
}

// EnabledModuleStream is a DNF module stream enabled or installed on the
// host.
type EnabledModuleStream struct {
	ModuleName    string           `json:"module_name"`
	Stream        string           `json:"stream"`
	Profiles      []string         `json:"profiles"`
	Include       bool             `json:"include"`
	BaselineMatch bool             `json:"baseline_match"`
	Fleet         *FleetPrevalence `json:"fleet"`
}

// VersionLockEntry is a package version pin from
// /etc/dnf/plugins/versionlock.list.
type VersionLockEntry struct {
	RawPattern string           `json:"raw_pattern"`
	Name       string           `json:"name"`
	Epoch      int              `json:"epoch"`
	Version    string           `json:"version"`
	Release    string           `json:"release"`
	Arch       string           `json:"arch"`
	Include    bool             `json:"include"`
	Fleet      *FleetPrevalence `json:"fleet"`
}

// RpmVaEntry is a single line from rpm -Va showing a modified file with
// verification flags.
type RpmVaEntry struct {
	Path    string  `json:"path"`
	Flags   string  `json:"flags"`
	Package *string `json:"package"`
}

// UnverifiablePackage is a package that could not be checked during
// preflight.
type UnverifiablePackage struct {
	Name   string `json:"name"`
	Reason string `json:"reason"`
}

// RepoStatus records the status of a repo that could not be queried
// during preflight.
type RepoStatus struct {
	RepoID           string   `json:"repo_id"`
	RepoName         string   `json:"repo_name"`
	Error            string   `json:"error"`
	AffectedPackages []string `json:"affected_packages"`
}

// PreflightResult is the result of package availability check against
// target repos.
type PreflightResult struct {
	Status         string                `json:"status"`
	StatusReason   *string               `json:"status_reason"`
	Available      []string              `json:"available"`
	Unavailable    []string              `json:"unavailable"`
	Unverifiable   []UnverifiablePackage `json:"unverifiable"`
	DirectInstall  []string              `json:"direct_install"`
	RepoUnreachable []RepoStatus         `json:"repo_unreachable"`
	BaseImage      string                `json:"base_image"`
	ReposQueried   []string              `json:"repos_queried"`
	Timestamp      string                `json:"timestamp"`
}

// OstreePackageOverride is an rpm-ostree package override (layered,
// replaced, or removed).
type OstreePackageOverride struct {
	Name     string `json:"name"`
	FromNevra string `json:"from_nevra"`
	ToNevra  string `json:"to_nevra"`
}

// RepoFile is a repo definition file (content or path).
type RepoFile struct {
	Path          string           `json:"path"`
	Content       string           `json:"content"`
	IsDefaultRepo bool             `json:"is_default_repo"`
	Include       bool             `json:"include"`
	Fleet         *FleetPrevalence `json:"fleet"`
}

// RpmSection is the output of the RPM inspector.
type RpmSection struct {
	PackagesAdded           []PackageEntry          `json:"packages_added"`
	BaseImageOnly           []PackageEntry          `json:"base_image_only"`
	RpmVa                   []RpmVaEntry            `json:"rpm_va"`
	RepoFiles               []RepoFile              `json:"repo_files"`
	GpgKeys                 []RepoFile              `json:"gpg_keys"`
	DnfHistoryRemoved       []string                `json:"dnf_history_removed"`
	VersionChanges          []VersionChange         `json:"version_changes"`
	LeafPackages            *[]string               `json:"leaf_packages"`
	AutoPackages            *[]string               `json:"auto_packages"`
	LeafDepTree             map[string]interface{}  `json:"leaf_dep_tree"`
	ModuleStreams            []EnabledModuleStream   `json:"module_streams"`
	VersionLocks            []VersionLockEntry      `json:"version_locks"`
	ModuleStreamConflicts   []string                `json:"module_stream_conflicts"`
	BaselineModuleStreams   *map[string]string       `json:"baseline_module_streams"`
	VersionlockCommandOutput *string                `json:"versionlock_command_output"`
	MultiarchPackages       []string                `json:"multiarch_packages"`
	DuplicatePackages       []string                `json:"duplicate_packages"`
	RepoProvidingPackages   []string                `json:"repo_providing_packages"`
	OstreeOverrides         []OstreePackageOverride  `json:"ostree_overrides"`
	OstreeRemovals          []string                `json:"ostree_removals"`
	BaseImage               *string                 `json:"base_image"`
	BaselinePackageNames    *[]string               `json:"baseline_package_names"`
	NoBaseline              bool                    `json:"no_baseline"`
}

// ---------------------------------------------------------------------------
// Config types
// ---------------------------------------------------------------------------

// ConfigFileEntry is a config file captured by the Config inspector.
type ConfigFileEntry struct {
	Path           string           `json:"path"`
	Kind           ConfigFileKind   `json:"kind"`
	Category       ConfigCategory   `json:"category"`
	Content        string           `json:"content"`
	RpmVaFlags     *string          `json:"rpm_va_flags"`
	Package        *string          `json:"package"`
	DiffAgainstRpm *string          `json:"diff_against_rpm"`
	Include        bool             `json:"include"`
	Tie            bool             `json:"tie"`
	TieWinner      bool             `json:"tie_winner"`
	Fleet          *FleetPrevalence `json:"fleet"`
}

// ConfigSection is the output of the Config inspector.
type ConfigSection struct {
	Files []ConfigFileEntry `json:"files"`
}

// ---------------------------------------------------------------------------
// Service types
// ---------------------------------------------------------------------------

// ServiceStateChange records a service enablement/state vs baseline.
type ServiceStateChange struct {
	Unit          string           `json:"unit"`
	CurrentState  string           `json:"current_state"`
	DefaultState  string           `json:"default_state"`
	Action        string           `json:"action"`
	Include       bool             `json:"include"`
	OwningPackage *string          `json:"owning_package"`
	Fleet         *FleetPrevalence `json:"fleet"`
}

// SystemdDropIn is a systemd drop-in override file.
type SystemdDropIn struct {
	Unit      string           `json:"unit"`
	Path      string           `json:"path"`
	Content   string           `json:"content"`
	Include   bool             `json:"include"`
	Tie       bool             `json:"tie"`
	TieWinner bool             `json:"tie_winner"`
	Fleet     *FleetPrevalence `json:"fleet"`
}

// ServiceSection is the output of the Service inspector.
type ServiceSection struct {
	StateChanges  []ServiceStateChange `json:"state_changes"`
	EnabledUnits  []string             `json:"enabled_units"`
	DisabledUnits []string             `json:"disabled_units"`
	DropIns       []SystemdDropIn      `json:"drop_ins"`
}

// ---------------------------------------------------------------------------
// Network types
// ---------------------------------------------------------------------------

// NMConnection is a NetworkManager connection profile.
type NMConnection struct {
	Path         string           `json:"path"`
	Name         string           `json:"name"`
	Method       string           `json:"method"`
	Type         string           `json:"type"`
	Include      *bool            `json:"include,omitempty"`
	Acknowledged bool             `json:"acknowledged,omitempty"`
	Fleet        *FleetPrevalence `json:"fleet,omitempty"`
}

// FirewallZone is a firewalld zone definition.
type FirewallZone struct {
	Path      string           `json:"path"`
	Name      string           `json:"name"`
	Content   string           `json:"content"`
	Services  []string         `json:"services"`
	Ports     []string         `json:"ports"`
	RichRules []string         `json:"rich_rules"`
	Include   bool             `json:"include"`
	Fleet     *FleetPrevalence `json:"fleet"`
}

// FirewallDirectRule is a firewalld direct rule.
type FirewallDirectRule struct {
	Ipv      string `json:"ipv"`
	Table    string `json:"table"`
	Chain    string `json:"chain"`
	Priority string `json:"priority"`
	Args     string `json:"args"`
	Include  bool   `json:"include"`
}

// StaticRouteFile is a static route file detected on the host.
type StaticRouteFile struct {
	Path string `json:"path"`
	Name string `json:"name"`
}

// ProxyEntry is a proxy configuration line.
type ProxyEntry struct {
	Source string `json:"source"`
	Line   string `json:"line"`
}

// NetworkSection is the output of the Network inspector.
type NetworkSection struct {
	Connections        []NMConnection       `json:"connections"`
	FirewallZones      []FirewallZone       `json:"firewall_zones"`
	FirewallDirectRules []FirewallDirectRule `json:"firewall_direct_rules"`
	StaticRoutes       []StaticRouteFile    `json:"static_routes"`
	IPRoutes           []string             `json:"ip_routes"`
	IPRules            []string             `json:"ip_rules"`
	ResolvProvenance   string               `json:"resolv_provenance"`
	HostsAdditions     []string             `json:"hosts_additions"`
	Proxy              []ProxyEntry         `json:"proxy"`
}

// ---------------------------------------------------------------------------
// Storage types
// ---------------------------------------------------------------------------

// FstabEntry is a single entry from /etc/fstab.
type FstabEntry struct {
	Device       string           `json:"device"`
	MountPoint   string           `json:"mount_point"`
	Fstype       string           `json:"fstype"`
	Options      string           `json:"options"`
	Include      *bool            `json:"include,omitempty"`
	Acknowledged bool             `json:"acknowledged,omitempty"`
	Fleet        *FleetPrevalence `json:"fleet,omitempty"`
}

// CredentialRef is a reference to a credential file discovered in mount
// options or config.
type CredentialRef struct {
	MountPoint     string `json:"mount_point"`
	CredentialPath string `json:"credential_path"`
	Source         string `json:"source"`
}

// MountPoint is a mounted filesystem from findmnt output.
type MountPoint struct {
	Target  string `json:"target"`
	Source  string `json:"source"`
	Fstype  string `json:"fstype"`
	Options string `json:"options"`
}

// LvmVolume is an LVM logical volume.
type LvmVolume struct {
	LvName string `json:"lv_name"`
	VgName string `json:"vg_name"`
	LvSize string `json:"lv_size"`
}

// VarDirectory is a non-empty directory under /var discovered for the data
// migration plan.
type VarDirectory struct {
	Path           string `json:"path"`
	SizeEstimate   string `json:"size_estimate"`
	Recommendation string `json:"recommendation"`
}

// StorageSection is the output of the Storage inspector.
type StorageSection struct {
	FstabEntries   []FstabEntry    `json:"fstab_entries"`
	MountPoints    []MountPoint    `json:"mount_points"`
	LvmInfo        []LvmVolume     `json:"lvm_info"`
	VarDirectories []VarDirectory  `json:"var_directories"`
	CredentialRefs []CredentialRef  `json:"credential_refs"`
}

// ---------------------------------------------------------------------------
// Scheduled task types
// ---------------------------------------------------------------------------

// CronJob is a cron job definition.
type CronJob struct {
	Path     string           `json:"path"`
	Source   string           `json:"source"`
	RpmOwned bool             `json:"rpm_owned"`
	Include  bool             `json:"include"`
	Fleet    *FleetPrevalence `json:"fleet"`
}

// SystemdTimer is a systemd timer unit.
type SystemdTimer struct {
	Name           string           `json:"name"`
	OnCalendar     string           `json:"on_calendar"`
	ExecStart      string           `json:"exec_start"`
	Description    string           `json:"description"`
	Source         string           `json:"source"`
	Path           string           `json:"path"`
	TimerContent   string           `json:"timer_content"`
	ServiceContent string           `json:"service_content"`
	Include        *bool            `json:"include,omitempty"`
	Fleet          *FleetPrevalence `json:"fleet,omitempty"`
}

// AtJob is an at(1) job.
type AtJob struct {
	File       string           `json:"file"`
	Command    string           `json:"command"`
	User       string           `json:"user"`
	WorkingDir string           `json:"working_dir"`
	Include    *bool            `json:"include,omitempty"`
	Fleet      *FleetPrevalence `json:"fleet,omitempty"`
}

// GeneratedTimerUnit is a systemd timer generated from a cron expression.
type GeneratedTimerUnit struct {
	Name           string           `json:"name"`
	TimerContent   string           `json:"timer_content"`
	ServiceContent string           `json:"service_content"`
	CronExpr       string           `json:"cron_expr"`
	SourcePath     string           `json:"source_path"`
	Command        string           `json:"command"`
	Include        bool             `json:"include"`
	Fleet          *FleetPrevalence `json:"fleet"`
}

// ScheduledTaskSection is the output of the Scheduled Task inspector.
type ScheduledTaskSection struct {
	CronJobs            []CronJob            `json:"cron_jobs"`
	SystemdTimers       []SystemdTimer       `json:"systemd_timers"`
	AtJobs              []AtJob              `json:"at_jobs"`
	GeneratedTimerUnits []GeneratedTimerUnit `json:"generated_timer_units"`
}

// ---------------------------------------------------------------------------
// Container types
// ---------------------------------------------------------------------------

// ContainerMount is a mount inside a running container.
type ContainerMount struct {
	Type        string `json:"type"`
	Source      string `json:"source"`
	Destination string `json:"destination"`
	Mode        string `json:"mode"`
	RW          bool   `json:"rw"`
}

// QuadletUnit is a Podman Quadlet unit file.
type QuadletUnit struct {
	Path      string           `json:"path"`
	Name      string           `json:"name"`
	Content   string           `json:"content"`
	Image     string           `json:"image"`
	Include   bool             `json:"include"`
	Tie       bool             `json:"tie"`
	TieWinner bool             `json:"tie_winner"`
	Fleet     *FleetPrevalence `json:"fleet"`
}

// ComposeService is a single service within a compose file.
type ComposeService struct {
	Service string `json:"service"`
	Image   string `json:"image"`
}

// ComposeFile is a docker-compose / podman-compose file.
type ComposeFile struct {
	Path      string            `json:"path"`
	Images    []ComposeService  `json:"images"`
	Include   bool              `json:"include"`
	Tie       bool              `json:"tie"`
	TieWinner bool              `json:"tie_winner"`
	Fleet     *FleetPrevalence  `json:"fleet"`
}

// RunningContainer is a running OCI container (podman/docker).
type RunningContainer struct {
	ID           string                 `json:"id"`
	Name         string                 `json:"name"`
	Image        string                 `json:"image"`
	ImageID      string                 `json:"image_id"`
	Status       string                 `json:"status"`
	Mounts       []ContainerMount       `json:"mounts"`
	Networks     map[string]interface{} `json:"networks"`
	Ports        map[string]interface{} `json:"ports"`
	Env          []string               `json:"env"`
	Include      *bool                  `json:"include,omitempty"`
	Acknowledged bool                   `json:"acknowledged,omitempty"`
	Fleet        *FleetPrevalence       `json:"fleet,omitempty"`
}

// FlatpakApp is a Flatpak application detected on an ostree system.
type FlatpakApp struct {
	AppID   string `json:"app_id"`
	Origin  string `json:"origin"`
	Branch  string `json:"branch"`
	Include bool   `json:"include"`
}

// ContainerSection is the output of the Container inspector.
type ContainerSection struct {
	QuadletUnits      []QuadletUnit      `json:"quadlet_units"`
	ComposeFiles      []ComposeFile      `json:"compose_files"`
	RunningContainers []RunningContainer `json:"running_containers"`
	FlatpakApps       []FlatpakApp       `json:"flatpak_apps"`
}

// ---------------------------------------------------------------------------
// Non-RPM software types
// ---------------------------------------------------------------------------

// PipPackage is a single pip package (name + version).
type PipPackage struct {
	Name    string `json:"name"`
	Version string `json:"version"`
}

// NonRpmItem is a single item found by the Non-RPM Software inspector.
type NonRpmItem struct {
	Path               string                  `json:"path"`
	Name               string                  `json:"name"`
	Method             string                  `json:"method"`
	Confidence         string                  `json:"confidence"`
	Include            bool                    `json:"include"`
	Acknowledged       bool                    `json:"acknowledged,omitempty"`
	Lang               string                  `json:"lang"`
	Static             bool                    `json:"static"`
	Version            string                  `json:"version"`
	SharedLibs         []string                `json:"shared_libs"`
	SystemSitePackages bool                    `json:"system_site_packages"`
	Packages           []PipPackage            `json:"packages"`
	HasCExtensions     bool                    `json:"has_c_extensions"`
	GitRemote          string                  `json:"git_remote"`
	GitCommit          string                  `json:"git_commit"`
	GitBranch          string                  `json:"git_branch"`
	Files              *map[string]interface{} `json:"files"`
	Content            string                  `json:"content"`
	Fleet              *FleetPrevalence        `json:"fleet"`
}

// NonRpmSoftwareSection is the output of the Non-RPM Software inspector.
type NonRpmSoftwareSection struct {
	Items    []NonRpmItem      `json:"items"`
	EnvFiles []ConfigFileEntry `json:"env_files"`
}

// ---------------------------------------------------------------------------
// Kernel/Boot types
// ---------------------------------------------------------------------------

// ConfigSnippet is a config file snippet (path + content), used for
// modules-load.d, modprobe.d, dracut.
type ConfigSnippet struct {
	Path    string `json:"path"`
	Content string `json:"content"`
}

// SysctlOverride is a sysctl value that differs from the shipped default.
type SysctlOverride struct {
	Key     string `json:"key"`
	Runtime string `json:"runtime"`
	Default string `json:"default"`
	Source  string `json:"source"`
	Include bool   `json:"include"`
}

// KernelModule is a loaded kernel module from lsmod output.
type KernelModule struct {
	Name    string `json:"name"`
	Size    string `json:"size"`
	UsedBy  string `json:"used_by"`
	Include bool   `json:"include"`
}

// AlternativeEntry is a system alternative (update-alternatives entry).
type AlternativeEntry struct {
	Name   string `json:"name"`
	Path   string `json:"path"`
	Status string `json:"status"`
}

// KernelBootSection is the output of the Kernel/Boot inspector.
type KernelBootSection struct {
	Cmdline              string          `json:"cmdline"`
	GrubDefaults         string          `json:"grub_defaults"`
	SysctlOverrides      []SysctlOverride `json:"sysctl_overrides"`
	ModulesLoadD         []ConfigSnippet `json:"modules_load_d"`
	ModprobeD            []ConfigSnippet `json:"modprobe_d"`
	DracutConf           []ConfigSnippet `json:"dracut_conf"`
	LoadedModules        []KernelModule  `json:"loaded_modules"`
	NonDefaultModules    []KernelModule  `json:"non_default_modules"`
	TunedActive          string          `json:"tuned_active"`
	TunedCustomProfiles  []ConfigSnippet `json:"tuned_custom_profiles"`
	Locale               *string         `json:"locale"`
	Timezone             *string         `json:"timezone"`
	Alternatives         []AlternativeEntry `json:"alternatives"`
}

// ---------------------------------------------------------------------------
// SELinux types
// ---------------------------------------------------------------------------

// SelinuxPortLabel is a custom SELinux port label assignment.
type SelinuxPortLabel struct {
	Protocol string           `json:"protocol"`
	Port     string           `json:"port"`
	Type     string           `json:"type"`
	Include  bool             `json:"include"`
	Fleet    *FleetPrevalence `json:"fleet"`
}

// SelinuxSection is the output of the SELinux/Security inspector.
type SelinuxSection struct {
	Mode             string                   `json:"mode"`
	CustomModules    []string                 `json:"custom_modules"`
	BooleanOverrides []map[string]interface{} `json:"boolean_overrides"`
	FcontextRules    []string                 `json:"fcontext_rules"`
	AuditRules       []string                 `json:"audit_rules"`
	FipsMode         bool                     `json:"fips_mode"`
	PamConfigs       []string                 `json:"pam_configs"`
	PortLabels       []SelinuxPortLabel       `json:"port_labels"`
}

// ---------------------------------------------------------------------------
// Users/Groups types
// ---------------------------------------------------------------------------

// UserGroupSection is the output of the User/Group inspector.
type UserGroupSection struct {
	Users                []map[string]interface{} `json:"users"`
	Groups               []map[string]interface{} `json:"groups"`
	SudoersRules         []string                 `json:"sudoers_rules"`
	SSHAuthorizedKeysRefs []map[string]interface{} `json:"ssh_authorized_keys_refs"`
	PasswdEntries        []string                 `json:"passwd_entries"`
	ShadowEntries        []string                 `json:"shadow_entries"`
	GroupEntries         []string                 `json:"group_entries"`
	GshadowEntries       []string                 `json:"gshadow_entries"`
	SubuidEntries        []string                 `json:"subuid_entries"`
	SubgidEntries        []string                 `json:"subgid_entries"`
}

// ---------------------------------------------------------------------------
// Redaction
// ---------------------------------------------------------------------------

// RedactionFinding records a single redaction event.
type RedactionFinding struct {
	Path            string  `json:"path"`
	Source          string  `json:"source"`
	Kind            string  `json:"kind"`
	Pattern         string  `json:"pattern"`
	Remediation     string  `json:"remediation"`
	Line            *int    `json:"line"`
	Replacement     *string `json:"replacement"`
	DetectionMethod string  `json:"detection_method"`
	Confidence      *string `json:"confidence"`
}
