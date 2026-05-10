use serde::{Deserialize, Serialize};

/// Typed inspector identity — compiler-enforced exhaustive handling.
///
/// Note: Hardware, Ostree, and OsRelease are Phase 2 inspectors that do not
/// have SectionData variants yet. They appear here so Completeness can track
/// their failure state from Phase 2 onward. SectionData covers the 11
/// inspectors that produce snapshot sections; InspectorId covers all 14.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectorId {
    Rpm,
    Config,
    Services,
    Network,
    Storage,
    ScheduledTasks,
    Containers,
    NonRpmSoftware,
    KernelBoot,
    Selinux,
    UsersGroups,
    Hardware,
    Ostree,
    OsRelease,
}

/// Used by Inspector::applicable_to() — which source types this inspector runs on.
/// Pipeline-internal only (no Serialize/Deserialize) — never crosses a serde boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceSystemKind {
    PackageBased,
    RpmOstree,
    Bootc,
}

/// Typed inspector output envelope — the compiler proves inspectors emit valid sections.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "inspector", content = "data")]
#[allow(clippy::large_enum_variant)]
pub enum SectionData {
    #[serde(rename = "rpm")]
    Rpm(super::rpm::RpmSection),
    #[serde(rename = "config")]
    Config(super::config::ConfigSection),
    #[serde(rename = "services")]
    Services(super::services::ServiceSection),
    #[serde(rename = "network")]
    Network(super::network::NetworkSection),
    #[serde(rename = "storage")]
    Storage(super::storage::StorageSection),
    #[serde(rename = "scheduled_tasks")]
    ScheduledTasks(super::scheduled::ScheduledTaskSection),
    #[serde(rename = "containers")]
    Containers(super::containers::ContainerSection),
    #[serde(rename = "non_rpm_software")]
    NonRpmSoftware(super::nonrpm::NonRpmSoftwareSection),
    #[serde(rename = "kernel_boot")]
    KernelBoot(super::kernelboot::KernelBootSection),
    #[serde(rename = "selinux")]
    Selinux(super::selinux::SelinuxSection),
    #[serde(rename = "users_groups")]
    UsersGroups(super::users::UserGroupSection),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum Completeness {
    #[default]
    Full,
    Partial {
        incomplete_sections: Vec<InspectorId>,
        reason: String,
    },
    Unverified {
        missing: Vec<InspectorId>,
    },
}
