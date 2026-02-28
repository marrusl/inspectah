"""
Inspection snapshot schema.

Strongly typed contract between inspectors and renderers.
All inspectors produce data that fits into this schema; all renderers consume it.
"""

from enum import Enum
from typing import Any, List, Optional

from pydantic import BaseModel, Field


# --- Metadata (set by pipeline from host) ---


class OsRelease(BaseModel):
    """From /etc/os-release."""

    name: str
    version_id: str
    version: str = ""
    id: str = ""
    id_like: str = ""
    pretty_name: str = ""


# --- RPM Inspector ---


class PackageState(str, Enum):
    ADDED = "added"
    REMOVED = "removed"
    MODIFIED = "modified"


class PackageEntry(BaseModel):
    """Single package from rpm -qa or baseline diff."""

    name: str
    epoch: str = "0"
    version: str
    release: str
    arch: str
    state: PackageState = PackageState.ADDED


class RpmVaEntry(BaseModel):
    """Single line from rpm -Va: modified file with verification flags."""

    path: str
    flags: str  # e.g. "S.5....T."
    package: Optional[str] = None


class RepoFile(BaseModel):
    """Repo definition file (content or path)."""

    path: str
    content: str = ""


class RpmSection(BaseModel):
    """Output of the RPM inspector."""

    packages_added: List[PackageEntry] = Field(default_factory=list)
    packages_removed: List[PackageEntry] = Field(default_factory=list)
    packages_modified: List[PackageEntry] = Field(default_factory=list)
    rpm_va: List[RpmVaEntry] = Field(default_factory=list)
    repo_files: List[RepoFile] = Field(default_factory=list)
    dnf_history_removed: List[str] = Field(default_factory=list)  # package names

    # Baseline from target bootc base image (cached for --from-snapshot)
    base_image: Optional[str] = None  # e.g. "quay.io/centos-bootc/centos-bootc:stream9"
    baseline_package_names: Optional[List[str]] = None
    no_baseline: bool = False  # True when base image cannot be queried


# --- Config Inspector ---


class ConfigFileKind(str, Enum):
    RPM_OWNED_MODIFIED = "rpm_owned_modified"
    UNOWNED = "unowned"
    ORPHANED = "orphaned"  # from removed package


class ConfigFileEntry(BaseModel):
    """A config file captured by the Config inspector."""

    path: str
    kind: ConfigFileKind
    content: str = ""
    rpm_va_flags: Optional[str] = None  # if rpm-owned modified
    package: Optional[str] = None
    diff_against_rpm: Optional[str] = None  # unified diff when --config-diffs


class ConfigSection(BaseModel):
    """Output of the Config inspector."""

    files: List[ConfigFileEntry] = Field(default_factory=list)


# --- Service Inspector ---


class ServiceStateChange(BaseModel):
    """Service enablement/state vs baseline."""

    unit: str
    current_state: str  # enabled, disabled, masked, etc.
    default_state: str
    action: str  # "enable", "disable", "mask", or "unchanged"


class ServiceSection(BaseModel):
    """Output of the Service inspector."""

    state_changes: List[ServiceStateChange] = Field(default_factory=list)
    enabled_units: List[str] = Field(default_factory=list)
    disabled_units: List[str] = Field(default_factory=list)


# --- Placeholders for remaining inspectors (added in later steps) ---


class NetworkSection(BaseModel):
    """Output of the Network inspector."""

    connections: List[dict] = Field(default_factory=list)  # {path, name, method, type}
    firewall_zones: List[dict] = Field(default_factory=list)  # {path, name, content, services, ports, rich_rules}
    firewall_direct_rules: List[dict] = Field(default_factory=list)  # {ipv, table, chain, rule}
    static_routes: List[dict] = Field(default_factory=list)
    ip_routes: List[str] = Field(default_factory=list)  # lines from ip route
    ip_rules: List[str] = Field(default_factory=list)  # lines from ip rule (non-default only)
    resolv_provenance: str = ""  # "systemd-resolved", "networkmanager", "hand-edited", or ""
    hosts_additions: List[str] = Field(default_factory=list)
    proxy: List[dict] = Field(default_factory=list)


class StorageSection(BaseModel):
    """Output of the Storage inspector."""

    fstab_entries: List[dict] = Field(default_factory=list)
    mount_points: List[dict] = Field(default_factory=list)
    lvm_info: List[dict] = Field(default_factory=list)


class ScheduledTaskSection(BaseModel):
    """Output of the Scheduled Task inspector."""

    cron_jobs: List[dict] = Field(default_factory=list)
    systemd_timers: List[dict] = Field(default_factory=list)  # {name, on_calendar, exec_start, source, timer_content, service_content}
    at_jobs: List[dict] = Field(default_factory=list)  # {file, command, user, working_dir}
    generated_timer_units: List[dict] = Field(default_factory=list)  # name, timer_content, service_content, cron_expr


class ContainerSection(BaseModel):
    """Output of the Container inspector."""

    quadlet_units: List[dict] = Field(default_factory=list)  # path, name, content, image_ref
    compose_files: List[dict] = Field(default_factory=list)  # path, images: [{service, image}]
    running_containers: List[dict] = Field(default_factory=list)  # id, names, image, status, mounts, networks, env


class NonRpmSoftwareSection(BaseModel):
    """Output of the Non-RPM Software inspector."""

    items: List[dict] = Field(default_factory=list)


class KernelBootSection(BaseModel):
    """Output of the Kernel/Boot inspector."""

    cmdline: str = ""
    grub_defaults: str = ""
    sysctl_overrides: List[dict] = Field(default_factory=list)  # {key, runtime, default, source}
    modules_load_d: List[dict] = Field(default_factory=list)  # {path, content}
    modprobe_d: List[dict] = Field(default_factory=list)  # {path, content}
    dracut_conf: List[dict] = Field(default_factory=list)  # {path, content}
    loaded_modules: List[dict] = Field(default_factory=list)  # all from lsmod: {name, size, used_by}
    non_default_modules: List[dict] = Field(default_factory=list)  # not in modules-load.d or built-in deps


class SelinuxSection(BaseModel):
    """Output of the SELinux/Security inspector."""

    mode: str = ""
    custom_modules: List[str] = Field(default_factory=list)
    boolean_overrides: List[dict] = Field(default_factory=list)
    audit_rules: List[str] = Field(default_factory=list)
    fips_mode: bool = False
    pam_configs: List[str] = Field(default_factory=list)


class UserGroupSection(BaseModel):
    """Output of the User/Group inspector."""

    users: List[dict] = Field(default_factory=list)  # name, uid, gid, shell, home
    groups: List[dict] = Field(default_factory=list)  # name, gid
    sudoers_rules: List[str] = Field(default_factory=list)
    ssh_authorized_keys_refs: List[dict] = Field(default_factory=list)  # user, path
    passwd_entries: List[str] = Field(default_factory=list)
    shadow_entries: List[str] = Field(default_factory=list)
    group_entries: List[str] = Field(default_factory=list)
    gshadow_entries: List[str] = Field(default_factory=list)
    subuid_entries: List[str] = Field(default_factory=list)
    subgid_entries: List[str] = Field(default_factory=list)


# --- Root snapshot ---


class InspectionSnapshot(BaseModel):
    """
    Full inspection snapshot. Serialized as inspection-snapshot.json.
    All sections are optional so we can run a subset of inspectors.
    """

    meta: dict = Field(default_factory=dict)  # hostname, timestamp, profile, etc.
    os_release: Optional[OsRelease] = None

    rpm: Optional[RpmSection] = None
    config: Optional[ConfigSection] = None
    services: Optional[ServiceSection] = None

    network: Optional[NetworkSection] = None
    storage: Optional[StorageSection] = None
    scheduled_tasks: Optional[ScheduledTaskSection] = None
    containers: Optional[ContainerSection] = None
    non_rpm_software: Optional[NonRpmSoftwareSection] = None
    kernel_boot: Optional[KernelBootSection] = None
    selinux: Optional[SelinuxSection] = None
    users_groups: Optional[UserGroupSection] = None

    # Populated after redaction pass
    warnings: List[dict] = Field(default_factory=list)
    redactions: List[dict] = Field(default_factory=list)

    model_config = {"extra": "forbid"}
