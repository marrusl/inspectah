//! Serde roundtrip tests for golden-file compatibility.
//!
//! These tests prove that Rust types can deserialize Go-captured golden
//! JSON and re-serialize it without loss. They do NOT exercise actual Rust
//! inspector code — the real inspector-vs-golden parity gate lives in
//! `inspectah-collect/tests/parity_test.rs`, which runs inspectors on
//! fixture data and compares output against fixture-derived goldens.
//!
//! The golden files are real Go v13 output captured from a CentOS Stream 9
//! host during host validation. Serde roundtrip tests here prove type-level
//! compatibility between Rust structs and actual Go output.

use inspectah_core::normalize::{diff_snapshots, load_divergence_allowlist};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::ConfigSection;
use inspectah_core::types::containers::ContainerSection;
use inspectah_core::types::kernelboot::KernelBootSection;
use inspectah_core::types::network::NetworkSection;
use inspectah_core::types::nonrpm::NonRpmSoftwareSection;
use inspectah_core::types::os::SystemType;
use inspectah_core::types::scheduled::ScheduledTaskSection;
use inspectah_core::types::selinux::SelinuxSection;
use inspectah_core::types::services::ServiceSection;
use inspectah_core::types::storage::StorageSection;
use inspectah_core::types::users::UserGroupSection;
use std::collections::BTreeSet;

/// Shared divergence allowlist, loaded once per test from the canonical source.
fn allowlist() -> BTreeSet<String> {
    let md = include_str!("../../testdata/divergences.md");
    load_divergence_allowlist(md)
}

// ── Snapshot self-roundtrip ──────────────────────────────────────────

/// Proves Rust snapshot serde roundtrip fidelity.
/// Does NOT compare against Go output.
#[test]
fn test_snapshot_serde_roundtrip() {
    let divergences_md = include_str!("../../testdata/divergences.md");
    let allowlist = load_divergence_allowlist(divergences_md);

    let mut snap = InspectionSnapshot::new();
    snap.system_type = SystemType::PackageMode;
    snap.preflight.status = "ok".into();

    let json = serde_json::to_string(&snap).unwrap();
    let parsed: InspectionSnapshot = serde_json::from_str(&json).unwrap();
    let json2 = serde_json::to_string(&parsed).unwrap();

    let undocumented = diff_snapshots(&json, &json2, &allowlist).unwrap();

    assert!(
        undocumented.is_empty(),
        "Rust snapshot does not round-trip faithfully:\n{}",
        undocumented
            .iter()
            .map(|d| format!("  {}: a={}, b={}", d.path, d.go_value, d.rust_value))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

// ── Services section: legacy golden must fail typed deserialization ───

/// Legacy Go v13 services payloads use stringly typed fields (action,
/// current_state as plain strings). The v16 typed contract intentionally
/// rejects these — operators must re-scan to get typed snapshots.
#[test]
fn test_legacy_go_v13_services_section_requires_rescan() {
    let golden = include_str!("../../testdata/golden/go-v13-services-section.json");
    let err = serde_json::from_str::<ServiceSection>(golden).unwrap_err();
    assert!(
        err.to_string().contains("current_state")
            || err.to_string().contains("unknown variant"),
        "legacy services payload should fail typed deserialization, got: {err}"
    );
}

// ── Storage section serde roundtrip ──────────────────────────────────

/// Proves Go golden JSON deserializes into StorageSection and
/// re-serializes without undocumented field loss.
#[test]
fn test_storage_serde_roundtrip() {
    let golden = include_str!("../../testdata/golden/go-v13-storage-section.json");

    let section: StorageSection =
        serde_json::from_str(golden).expect("golden must deserialize into StorageSection");

    let rust_json = serde_json::to_string_pretty(&section).unwrap();
    let undocumented = diff_snapshots(golden, &rust_json, &allowlist()).unwrap();

    assert!(
        undocumented.is_empty(),
        "Storage section has undocumented divergences:\n{}",
        format_diffs(&undocumented)
    );
}

/// Validates golden storage structure.
/// Real host data: 3 fstab entries, 3 mount points, 2 LVM volumes,
/// 9 var directories, no credential refs on this host.
#[test]
fn test_storage_field_coverage() {
    let golden = include_str!("../../testdata/golden/go-v13-storage-section.json");
    let section: StorageSection = serde_json::from_str(golden).unwrap();

    assert!(
        !section.fstab_entries.is_empty(),
        "golden must contain fstab_entries"
    );
    assert!(
        !section.mount_points.is_empty(),
        "golden must contain mount_points"
    );
    assert!(!section.lvm_info.is_empty(), "golden must contain lvm_info");

    // credential_refs: empty on this host (no credential mounts)
    assert!(
        section.credential_refs.is_empty(),
        "golden has no credential_refs on this host"
    );

    let entry = &section.fstab_entries[0];
    assert!(!entry.device.is_empty(), "device must be populated");
    assert!(
        !entry.mount_point.is_empty(),
        "mount_point must be populated"
    );
    assert!(!entry.fstype.is_empty(), "fstype must be populated");
}

// ── Kernel boot section serde roundtrip ──────────────────────────────

/// Proves Go golden JSON deserializes into KernelBootSection and
/// re-serializes without undocumented field loss.
#[test]
fn test_kernelboot_serde_roundtrip() {
    let golden = include_str!("../../testdata/golden/go-v13-kernelboot-section.json");

    let section: KernelBootSection =
        serde_json::from_str(golden).expect("golden must deserialize into KernelBootSection");

    let rust_json = serde_json::to_string_pretty(&section).unwrap();
    let undocumented = diff_snapshots(golden, &rust_json, &allowlist()).unwrap();

    assert!(
        undocumented.is_empty(),
        "Kernelboot section has undocumented divergences:\n{}",
        format_diffs(&undocumented)
    );
}

/// Validates Go golden kernelboot structure matches expected field layout.
/// On the real CentOS Stream 9 host: no sysctl overrides, no dracut conf,
/// tuned not active (empty string), 73 loaded modules, 33 non_default
/// modules, 28 alternatives, 1 modprobe.d entry.
#[test]
fn test_kernelboot_field_coverage() {
    let golden = include_str!("../../testdata/golden/go-v13-kernelboot-section.json");
    let section: KernelBootSection = serde_json::from_str(golden).unwrap();

    assert!(!section.cmdline.is_empty(), "golden must contain cmdline");

    // sysctl_overrides: empty on this host — verify deserialized
    assert!(
        section.sysctl_overrides.is_empty(),
        "Go golden has no sysctl_overrides on this host"
    );

    // loaded_modules: 73 modules on the real host
    assert!(
        !section.loaded_modules.is_empty(),
        "golden must contain loaded_modules"
    );
    assert!(
        section.loaded_modules.len() > 50,
        "Go golden should have 70+ loaded_modules, got {}",
        section.loaded_modules.len()
    );

    // non_default_modules: 33 on the real host (Go collects these)
    assert!(
        !section.non_default_modules.is_empty(),
        "golden must contain non_default_modules"
    );

    // alternatives: 28 on the real host (Go collects these)
    assert!(
        !section.alternatives.is_empty(),
        "golden must contain alternatives"
    );

    // dracut_conf: empty on this host
    assert!(
        section.dracut_conf.is_empty(),
        "Go golden has no dracut_conf on this host"
    );

    assert!(section.locale.is_some(), "golden must contain locale");
    assert!(section.timezone.is_some(), "golden must contain timezone");

    // tuned_active: empty string on this host (tuned not running)
    assert!(
        section.tuned_active.is_empty(),
        "Go golden has empty tuned_active on this host"
    );

    // modprobe_d: 1 entry on this host
    assert!(
        !section.modprobe_d.is_empty(),
        "golden must contain modprobe_d"
    );
}

// ── Network section serde roundtrip ──────────────────────────────────

/// Proves Go golden JSON deserializes into NetworkSection and
/// re-serializes without undocumented field loss.
/// NOTE: Golden is provisional — will be replaced with real Go output
/// during host validation (Task 10).
#[test]
fn test_network_serde_roundtrip() {
    let golden = include_str!("../../testdata/golden/go-v13-network-section.json");

    let section: NetworkSection =
        serde_json::from_str(golden).expect("golden must deserialize into NetworkSection");

    let rust_json = serde_json::to_string_pretty(&section).unwrap();
    let undocumented = diff_snapshots(golden, &rust_json, &allowlist()).unwrap();

    assert!(
        undocumented.is_empty(),
        "Network section has undocumented divergences:\n{}",
        format_diffs(&undocumented)
    );
}

/// Validates golden network structure matches expected field layout.
/// Real host data: 2 NM connections, 1 firewall zone, 2 ip_routes,
/// 1 static route file, no hosts_additions, no proxy entries.
/// ip_rules is null in Go output (Go omitempty on nil slice).
#[test]
fn test_network_field_coverage() {
    let golden = include_str!("../../testdata/golden/go-v13-network-section.json");
    let section: NetworkSection = serde_json::from_str(golden).unwrap();

    // connections: 2 NM connection profiles
    assert!(
        !section.connections.is_empty(),
        "golden must contain connections"
    );
    let conn = &section.connections[0];
    assert!(!conn.path.is_empty(), "connection path must be populated");
    assert!(!conn.name.is_empty(), "connection name must be populated");
    assert!(
        !conn.method.is_empty(),
        "connection method must be populated"
    );
    assert!(
        !conn.conn_type.is_empty(),
        "connection type must be populated"
    );

    // firewall_zones: 1 zone on this host
    assert!(
        !section.firewall_zones.is_empty(),
        "golden must contain firewall_zones"
    );
    let zone = &section.firewall_zones[0];
    assert!(!zone.name.is_empty(), "zone name must be populated");
    assert!(!zone.services.is_empty(), "zone services must be populated");

    // firewall_direct_rules: empty on this host
    assert!(
        section.firewall_direct_rules.is_empty(),
        "golden has no firewall_direct_rules on this host"
    );

    // static_routes: 1 route file on this host
    assert!(
        !section.static_routes.is_empty(),
        "golden must contain static_routes"
    );

    // ip_routes: 2 routes
    assert!(
        !section.ip_routes.is_empty(),
        "golden must contain ip_routes"
    );

    // ip_rules: null in Go output → deserialized as empty Vec
    assert!(
        section.ip_rules.is_empty(),
        "ip_rules is null in Go golden (no iptables rules on this host)"
    );

    // resolv_provenance: should be populated
    assert!(
        !section.resolv_provenance.is_empty(),
        "resolv_provenance must be populated"
    );

    // hosts_additions: empty on this host
    assert!(
        section.hosts_additions.is_empty(),
        "golden has no hosts_additions on this host"
    );

    // proxy: empty on this host
    assert!(
        section.proxy.is_empty(),
        "golden has no proxy entries on this host"
    );
}

// ── Containers section serde roundtrip ──────────────────────────────

/// Proves Go golden JSON deserializes into ContainerSection and
/// re-serializes without undocumented field loss.
/// NOTE: Golden is provisional — will be replaced with real Go output
/// during host validation (Task 10).
#[test]
fn test_containers_serde_roundtrip() {
    let golden = include_str!("../../testdata/golden/go-v13-containers-section.json");

    let section: ContainerSection =
        serde_json::from_str(golden).expect("golden must deserialize into ContainerSection");

    let rust_json = serde_json::to_string_pretty(&section).unwrap();
    let undocumented = diff_snapshots(golden, &rust_json, &allowlist()).unwrap();

    assert!(
        undocumented.is_empty(),
        "Containers section has undocumented divergences:\n{}",
        format_diffs(&undocumented)
    );
}

/// Validates golden containers structure matches expected field layout.
/// Real host data: no containers running, no quadlets, no compose files,
/// no flatpak apps. All fields are null in Go output (nil slices).
/// The deserialize_null_default helper coerces null → empty Vec.
#[test]
fn test_containers_field_coverage() {
    let golden = include_str!("../../testdata/golden/go-v13-containers-section.json");
    let section: ContainerSection = serde_json::from_str(golden).unwrap();

    // All fields are null in Go golden (no containers on this host).
    // Verify they deserialized to empty Vecs via deserialize_null_default.
    assert!(
        section.quadlet_units.is_empty(),
        "golden has no quadlet_units (null in Go)"
    );
    assert!(
        section.compose_files.is_empty(),
        "golden has no compose_files (null in Go)"
    );
    assert!(
        section.running_containers.is_empty(),
        "golden has no running_containers (null in Go)"
    );
    assert!(
        section.flatpak_apps.is_empty(),
        "golden has no flatpak_apps (null in Go)"
    );
}

// ── Users/Groups section serde roundtrip ────────────────────────────

/// Proves Go golden JSON deserializes into UserGroupSection and
/// re-serializes without undocumented field loss.
/// NOTE: Golden is provisional — will be replaced with real Go output
/// during host validation (Task 10).
#[test]
fn test_users_groups_serde_roundtrip() {
    let golden = include_str!("../../testdata/golden/go-v13-users-groups-section.json");

    let section: UserGroupSection =
        serde_json::from_str(golden).expect("golden must deserialize into UserGroupSection");

    let rust_json = serde_json::to_string_pretty(&section).unwrap();
    let undocumented = diff_snapshots(golden, &rust_json, &allowlist()).unwrap();

    assert!(
        undocumented.is_empty(),
        "Users/Groups section has undocumented divergences:\n{}",
        format_diffs(&undocumented)
    );
}

/// Validates golden users/groups structure matches expected field layout.
/// Real host data: 1 human user (mark), 1 group, 2 sudoers rules,
/// 1 SSH key ref, 1 passwd entry, 1 shadow entry (redacted hash),
/// 1 group entry, 1 gshadow entry, 1 subuid, 1 subgid.
#[test]
fn test_users_groups_field_coverage() {
    let golden = include_str!("../../testdata/golden/go-v13-users-groups-section.json");
    let section: UserGroupSection = serde_json::from_str(golden).unwrap();

    // users: 1 human user on this host
    assert!(!section.users.is_empty(), "golden must contain users");
    assert_eq!(section.users.len(), 1, "golden has 1 user");
    let user = &section.users[0];
    assert!(user.get("name").is_some(), "user must have name");
    assert!(user.get("uid").is_some(), "user must have uid");
    assert!(user.get("shell").is_some(), "user must have shell");

    // groups: 1 group on this host
    assert!(!section.groups.is_empty(), "golden must contain groups");
    assert_eq!(section.groups.len(), 1, "golden has 1 group");
    let group = &section.groups[0];
    assert!(group.get("name").is_some(), "group must have name");
    assert!(group.get("gid").is_some(), "group must have gid");

    // sudoers_rules: 2 rules
    assert!(
        !section.sudoers_rules.is_empty(),
        "golden must contain sudoers_rules"
    );

    // ssh_authorized_keys_refs: 1 ref
    assert!(
        !section.ssh_authorized_keys_refs.is_empty(),
        "golden must contain ssh_authorized_keys_refs"
    );

    // passwd_entries: 1 entry
    assert!(
        !section.passwd_entries.is_empty(),
        "golden must contain passwd_entries"
    );

    // shadow_entries: 1 entry (redacted hash)
    assert!(
        !section.shadow_entries.is_empty(),
        "golden must contain shadow_entries (redacted hash)"
    );

    // group_entries: 1 entry
    assert!(
        !section.group_entries.is_empty(),
        "golden must contain group_entries"
    );

    // subuid_entries and subgid_entries: 1 each
    assert!(
        !section.subuid_entries.is_empty(),
        "golden must contain subuid_entries"
    );
    assert!(
        !section.subgid_entries.is_empty(),
        "golden must contain subgid_entries"
    );
}

// ── Scheduled tasks section serde roundtrip ──────────────────────────

/// Proves Go golden JSON deserializes into ScheduledTaskSection and
/// re-serializes without undocumented field loss.
/// NOTE: Golden is provisional — will be replaced with real Go output
/// during host validation (Task 13).
#[test]
fn test_serde_roundtrip_scheduled_tasks() {
    let golden = include_str!("../../testdata/golden/go-v13-scheduled-tasks-section.json");

    let section: ScheduledTaskSection =
        serde_json::from_str(golden).expect("golden must deserialize into ScheduledTaskSection");

    let rust_json = serde_json::to_string_pretty(&section).unwrap();
    let undocumented = diff_snapshots(golden, &rust_json, &allowlist()).unwrap();

    assert!(
        undocumented.is_empty(),
        "Scheduled tasks section has undocumented divergences:\n{}",
        format_diffs(&undocumented)
    );
}

/// Validates provisional golden scheduled tasks structure.
/// Provisional data: 3 cron_jobs, 2 systemd_timers, 2 at_jobs,
/// 2 generated_timer_units.
#[test]
fn test_scheduled_tasks_field_coverage() {
    let golden = include_str!("../../testdata/golden/go-v13-scheduled-tasks-section.json");
    let section: ScheduledTaskSection = serde_json::from_str(golden).unwrap();

    // cron_jobs: 3 cron entries
    assert!(
        !section.cron_jobs.is_empty(),
        "golden must contain cron_jobs"
    );
    assert_eq!(
        section.cron_jobs.len(),
        3,
        "provisional golden has 3 cron_jobs"
    );
    let cj = &section.cron_jobs[0];
    assert!(!cj.path.is_empty(), "cron path must be populated");
    assert!(!cj.source.is_empty(), "cron source must be populated");

    // systemd_timers: 2 timer units
    assert!(
        !section.systemd_timers.is_empty(),
        "golden must contain systemd_timers"
    );
    assert_eq!(
        section.systemd_timers.len(),
        2,
        "provisional golden has 2 systemd_timers"
    );
    let timer = &section.systemd_timers[0];
    assert!(!timer.name.is_empty(), "timer name must be populated");
    assert!(
        !timer.on_calendar.is_empty(),
        "timer on_calendar must be populated"
    );
    assert!(
        !timer.exec_start.is_empty(),
        "timer exec_start must be populated"
    );
    assert!(
        !timer.timer_content.is_empty(),
        "timer_content must be populated"
    );
    assert!(
        !timer.service_content.is_empty(),
        "service_content must be populated"
    );

    // at_jobs: 2 at jobs
    assert!(!section.at_jobs.is_empty(), "golden must contain at_jobs");
    assert_eq!(section.at_jobs.len(), 2, "provisional golden has 2 at_jobs");
    let at = &section.at_jobs[0];
    assert!(!at.file.is_empty(), "at file must be populated");
    assert!(!at.command.is_empty(), "at command must be populated");
    assert!(!at.user.is_empty(), "at user must be populated");

    // generated_timer_units: 2 generated timers
    assert!(
        !section.generated_timer_units.is_empty(),
        "golden must contain generated_timer_units"
    );
    assert_eq!(
        section.generated_timer_units.len(),
        2,
        "provisional golden has 2 generated_timer_units"
    );
    let timer = &section.generated_timer_units[0];
    assert!(
        !timer.name.is_empty(),
        "generated timer name must be populated"
    );
    assert!(
        !timer.cron_expr.is_empty(),
        "generated timer cron_expr must be populated"
    );
    assert!(
        !timer.source_path.is_empty(),
        "generated timer source_path must be populated"
    );
    assert!(
        !timer.command.is_empty(),
        "generated timer command must be populated"
    );
}

// ── Config section serde roundtrip ───────────────────────────────────

/// Proves Go golden JSON deserializes into ConfigSection and
/// re-serializes without undocumented field loss.
/// NOTE: Golden is provisional — will be replaced with real Go output
/// during host validation (Task 13).
#[test]
fn test_serde_roundtrip_config() {
    let golden = include_str!("../../testdata/golden/go-v13-config-section.json");

    let section: ConfigSection =
        serde_json::from_str(golden).expect("golden must deserialize into ConfigSection");

    let rust_json = serde_json::to_string_pretty(&section).unwrap();
    let undocumented = diff_snapshots(golden, &rust_json, &allowlist()).unwrap();

    assert!(
        undocumented.is_empty(),
        "Config section has undocumented divergences:\n{}",
        format_diffs(&undocumented)
    );
}

/// Validates provisional golden config structure.
/// Provisional data: 11 config files across multiple categories and kinds.
#[test]
fn test_config_field_coverage() {
    let golden = include_str!("../../testdata/golden/go-v13-config-section.json");
    let section: ConfigSection = serde_json::from_str(golden).unwrap();

    assert!(
        !section.files.is_empty(),
        "golden must contain config files"
    );
    assert!(
        section.files.len() >= 5,
        "provisional golden should have 5+ config files, got {}",
        section.files.len()
    );

    let file = &section.files[0];
    assert!(!file.path.is_empty(), "config path must be populated");
    assert!(
        !file.content.is_empty(),
        "first config file content must be populated"
    );

    // Verify multiple ConfigFileKind variants are exercised
    let kinds: Vec<String> = section
        .files
        .iter()
        .map(|f| {
            serde_json::to_value(&f.kind)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert!(
        kinds.contains(&"unowned".to_string()),
        "golden must contain unowned files"
    );
    assert!(
        kinds.contains(&"rpm_owned_default".to_string()),
        "golden must contain rpm_owned_default files"
    );
    assert!(
        kinds.contains(&"rpm_owned_modified".to_string()),
        "golden must contain rpm_owned_modified files"
    );
    assert!(
        kinds.contains(&"orphaned".to_string()),
        "golden must contain orphaned files"
    );

    // Verify multiple ConfigCategory variants are exercised
    let cats: Vec<String> = section
        .files
        .iter()
        .map(|f| {
            serde_json::to_value(&f.category)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert!(
        cats.contains(&"sysctl".to_string()),
        "golden must contain sysctl category"
    );
    assert!(
        cats.contains(&"limits".to_string()),
        "golden must contain limits category"
    );
    assert!(
        cats.contains(&"environment".to_string()),
        "golden must contain environment category"
    );
}

// ── SELinux section serde roundtrip ──────────────────────────────────

/// Proves Go golden JSON deserializes into SelinuxSection and
/// re-serializes without undocumented field loss.
/// NOTE: Golden is provisional — will be replaced with real Go output
/// during host validation (Task 13).
#[test]
fn test_serde_roundtrip_selinux() {
    let golden = include_str!("../../testdata/golden/go-v13-selinux-section.json");

    let section: SelinuxSection =
        serde_json::from_str(golden).expect("golden must deserialize into SelinuxSection");

    let rust_json = serde_json::to_string_pretty(&section).unwrap();
    let undocumented = diff_snapshots(golden, &rust_json, &allowlist()).unwrap();

    assert!(
        undocumented.is_empty(),
        "SELinux section has undocumented divergences:\n{}",
        format_diffs(&undocumented)
    );
}

/// Validates provisional golden SELinux structure.
/// Provisional data: enforcing mode, 2 custom modules, 2 boolean overrides,
/// 2 fcontext rules, 2 audit rules, 2 pam configs, 3 port labels.
#[test]
fn test_selinux_field_coverage() {
    let golden = include_str!("../../testdata/golden/go-v13-selinux-section.json");
    let section: SelinuxSection = serde_json::from_str(golden).unwrap();

    assert_eq!(section.mode, "enforcing", "mode must be enforcing");

    assert!(
        !section.custom_modules.is_empty(),
        "golden must contain custom_modules"
    );
    assert_eq!(
        section.custom_modules.len(),
        2,
        "provisional golden has 2 custom_modules"
    );

    assert!(
        !section.boolean_overrides.is_empty(),
        "golden must contain boolean_overrides"
    );
    assert_eq!(
        section.boolean_overrides.len(),
        2,
        "provisional golden has 2 boolean_overrides"
    );

    assert!(
        !section.fcontext_rules.is_empty(),
        "golden must contain fcontext_rules"
    );

    assert!(
        !section.audit_rules.is_empty(),
        "golden must contain audit_rules"
    );

    assert!(!section.fips_mode, "provisional golden has fips_mode=false");

    assert!(
        !section.pam_configs.is_empty(),
        "golden must contain pam_configs"
    );

    assert!(
        !section.port_labels.is_empty(),
        "golden must contain port_labels"
    );
    assert_eq!(
        section.port_labels.len(),
        3,
        "provisional golden has 3 port_labels"
    );
    let pl = &section.port_labels[0];
    assert!(!pl.protocol.is_empty(), "port protocol must be populated");
    assert!(!pl.port.is_empty(), "port number must be populated");
    assert!(
        !pl.label_type.is_empty(),
        "port label_type must be populated"
    );
}

// ── Non-RPM software section serde roundtrip ─────────────────────────

/// Proves Go golden JSON deserializes into NonRpmSoftwareSection and
/// re-serializes without undocumented field loss.
/// NOTE: Golden is provisional — will be replaced with real Go output
/// during host validation (Task 13).
#[test]
fn test_serde_roundtrip_non_rpm_software() {
    let golden = include_str!("../../testdata/golden/go-v13-non-rpm-software-section.json");

    let section: NonRpmSoftwareSection =
        serde_json::from_str(golden).expect("golden must deserialize into NonRpmSoftwareSection");

    let rust_json = serde_json::to_string_pretty(&section).unwrap();
    let undocumented = diff_snapshots(golden, &rust_json, &allowlist()).unwrap();

    assert!(
        undocumented.is_empty(),
        "Non-RPM software section has undocumented divergences:\n{}",
        format_diffs(&undocumented)
    );
}

/// Validates provisional golden non-RPM software structure.
/// Provisional data: 4 items (binary, python venv, npm project, static binary),
/// 2 env_files.
#[test]
fn test_non_rpm_software_field_coverage() {
    let golden = include_str!("../../testdata/golden/go-v13-non-rpm-software-section.json");
    let section: NonRpmSoftwareSection = serde_json::from_str(golden).unwrap();

    assert!(
        !section.items.is_empty(),
        "golden must contain non-rpm items"
    );
    assert_eq!(
        section.items.len(),
        4,
        "provisional golden has 4 non-rpm items"
    );

    // First item: C binary with shared libs
    let item = &section.items[0];
    assert!(!item.path.is_empty(), "item path must be populated");
    assert!(!item.name.is_empty(), "item name must be populated");
    assert!(!item.method.is_empty(), "item method must be populated");
    assert!(
        !item.confidence.is_empty(),
        "item confidence must be populated"
    );
    assert!(
        !item.shared_libs.is_empty(),
        "C binary must have shared_libs"
    );

    // Second item: Python venv
    let venv = &section.items[1];
    assert_eq!(venv.lang, "python", "second item should be python");
    assert!(
        venv.system_site_packages,
        "python venv should have system_site_packages=true"
    );
    assert!(
        venv.has_c_extensions,
        "python venv should have has_c_extensions=true"
    );

    // Third item: npm project with git info
    let npm = &section.items[2];
    assert!(
        !npm.git_remote.is_empty(),
        "npm project should have git_remote"
    );
    assert!(
        !npm.git_commit.is_empty(),
        "npm project should have git_commit"
    );
    assert!(
        !npm.git_branch.is_empty(),
        "npm project should have git_branch"
    );
    assert!(npm.files.is_some(), "npm project should have files");

    // Fourth item: static Go binary
    let static_bin = &section.items[3];
    assert!(static_bin.r#static, "Go binary should be static");
    assert!(!static_bin.include, "static tool has include=false");

    // env_files
    assert!(
        !section.env_files.is_empty(),
        "golden must contain env_files"
    );
    assert_eq!(
        section.env_files.len(),
        2,
        "provisional golden has 2 env_files"
    );
}

// ── Env file roundtrip test ──────────────────────────────────────────

/// Proves .env files survive roundtrip in the non_rpm_software section.
/// This is important because env files are ConfigFileEntry structs reused
/// across sections — verifies the shared type round-trips within the
/// non-RPM context.
#[test]
fn test_env_file_roundtrip() {
    let golden = include_str!("../../testdata/golden/go-v13-non-rpm-software-section.json");
    let section: NonRpmSoftwareSection = serde_json::from_str(golden).unwrap();

    assert!(
        !section.env_files.is_empty(),
        "golden must contain env_files for this test"
    );

    // Round-trip just the env_files through serde
    let env_json = serde_json::to_string_pretty(&section.env_files).unwrap();
    let parsed: Vec<inspectah_core::types::config::ConfigFileEntry> =
        serde_json::from_str(&env_json).unwrap();

    assert_eq!(
        section.env_files.len(),
        parsed.len(),
        "env_files count must survive roundtrip"
    );

    for (orig, rt) in section.env_files.iter().zip(parsed.iter()) {
        assert_eq!(orig.path, rt.path, "env_file path must survive roundtrip");
        assert_eq!(
            orig.content, rt.content,
            "env_file content must survive roundtrip"
        );
        assert_eq!(
            orig.include, rt.include,
            "env_file include must survive roundtrip"
        );
    }
}

// ── Full snapshot all-sections-present test ───────────────────────────

/// Proves a full snapshot with all 11 section keys present round-trips
/// through serde without losing any section.
///
/// Go goldens may contain null for empty arrays (Go nil-slice behavior).
/// The `deserialize_null_default` helper on Vec fields handles this
/// natively — no manual null→[] coercion needed.
#[test]
fn test_full_snapshot_serde_all_sections_present() {
    use serde_json::Value;

    // Build the snapshot as a Value tree
    let mut snap_value = serde_json::json!({
        "schema_version": 17,
        "meta": {},
        "os_release": null,
        "system_type": "package-mode",
        "preflight": {"status": "ok"},
        "warnings": [],
        "redactions": [],
        "redaction_hints": []
    });

    // Services excluded: v16 typed enums intentionally reject the legacy
    // Go golden (tested separately in test_legacy_go_v13_services_section_requires_rescan).
    let sections: &[(&str, &str)] = &[
        (
            "rpm",
            include_str!("../../testdata/golden/go-v13-rpm-section.json"),
        ),
        (
            "config",
            include_str!("../../testdata/golden/go-v13-config-section.json"),
        ),
        (
            "network",
            include_str!("../../testdata/golden/go-v13-network-section.json"),
        ),
        (
            "storage",
            include_str!("../../testdata/golden/go-v13-storage-section.json"),
        ),
        (
            "scheduled_tasks",
            include_str!("../../testdata/golden/go-v13-scheduled-tasks-section.json"),
        ),
        (
            "containers",
            include_str!("../../testdata/golden/go-v13-containers-section.json"),
        ),
        (
            "non_rpm_software",
            include_str!("../../testdata/golden/go-v13-non-rpm-software-section.json"),
        ),
        (
            "kernel_boot",
            include_str!("../../testdata/golden/go-v13-kernelboot-section.json"),
        ),
        (
            "selinux",
            include_str!("../../testdata/golden/go-v13-selinux-section.json"),
        ),
        (
            "users_groups",
            include_str!("../../testdata/golden/go-v13-users-groups-section.json"),
        ),
    ];

    for (key, json) in sections {
        let section: Value = serde_json::from_str(json)
            .unwrap_or_else(|e| panic!("golden for {key} is not valid JSON: {e}"));
        snap_value[key] = section;
    }

    // Serialize the assembled snapshot, then deserialize into typed struct.
    let full_json = serde_json::to_string_pretty(&snap_value).unwrap();
    let snap: InspectionSnapshot =
        serde_json::from_str(&full_json).expect("full snapshot with all sections must deserialize");

    // 10 section keys must be present (Some, not None).
    // Services excluded from this test — legacy golden uses stringly typed
    // fields that the v16 typed contract intentionally rejects.
    assert!(snap.rpm.is_some(), "rpm section must survive roundtrip");
    assert!(
        snap.config.is_some(),
        "config section must survive roundtrip"
    );
    assert!(
        snap.network.is_some(),
        "network section must survive roundtrip"
    );
    assert!(
        snap.storage.is_some(),
        "storage section must survive roundtrip"
    );
    assert!(
        snap.scheduled_tasks.is_some(),
        "scheduled_tasks section must survive roundtrip"
    );
    assert!(
        snap.containers.is_some(),
        "containers section must survive roundtrip"
    );
    assert!(
        snap.non_rpm_software.is_some(),
        "non_rpm_software section must survive roundtrip"
    );
    assert!(
        snap.kernel_boot.is_some(),
        "kernel_boot section must survive roundtrip"
    );
    assert!(
        snap.selinux.is_some(),
        "selinux section must survive roundtrip"
    );
    assert!(
        snap.users_groups.is_some(),
        "users_groups section must survive roundtrip"
    );
}

// ── Cross-section golden consistency ─────────────────────────────────

#[test]
fn test_all_section_goldens_are_valid_json() {
    // Ensure every golden file in testdata/golden/ is valid JSON.
    // This catches accidental corruption from manual edits.
    let goldens: &[(&str, &str)] = &[
        (
            "rpm",
            include_str!("../../testdata/golden/go-v13-rpm-section.json"),
        ),
        (
            "services",
            include_str!("../../testdata/golden/go-v13-services-section.json"),
        ),
        (
            "storage",
            include_str!("../../testdata/golden/go-v13-storage-section.json"),
        ),
        (
            "kernelboot",
            include_str!("../../testdata/golden/go-v13-kernelboot-section.json"),
        ),
        (
            "network",
            include_str!("../../testdata/golden/go-v13-network-section.json"),
        ),
        (
            "containers",
            include_str!("../../testdata/golden/go-v13-containers-section.json"),
        ),
        (
            "users-groups",
            include_str!("../../testdata/golden/go-v13-users-groups-section.json"),
        ),
        (
            "scheduled-tasks",
            include_str!("../../testdata/golden/go-v13-scheduled-tasks-section.json"),
        ),
        (
            "config",
            include_str!("../../testdata/golden/go-v13-config-section.json"),
        ),
        (
            "selinux",
            include_str!("../../testdata/golden/go-v13-selinux-section.json"),
        ),
        (
            "non-rpm-software",
            include_str!("../../testdata/golden/go-v13-non-rpm-software-section.json"),
        ),
    ];

    for (name, json) in goldens {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(json);
        assert!(
            parsed.is_ok(),
            "golden file for {name} is not valid JSON: {}",
            parsed.unwrap_err()
        );
    }
}

/// Format differences for assertion messages.
fn format_diffs(diffs: &[inspectah_core::normalize::Difference]) -> String {
    diffs
        .iter()
        .map(|d| format!("  {}: go={}, rust={}", d.path, d.go_value, d.rust_value))
        .collect::<Vec<_>>()
        .join("\n")
}
