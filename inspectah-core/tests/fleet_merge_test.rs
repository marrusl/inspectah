use inspectah_core::fleet::merge::{
    FleetMergeable, dedup_json_values, dedup_strings, merge_config_sections,
    merge_container_sections, merge_items, merge_kernelboot_sections, merge_network_sections,
    merge_nonrpm_sections, merge_rpm_sections, merge_scheduled_sections, merge_selinux_sections,
    merge_service_sections, merge_storage_sections, merge_usersgroups_sections,
};
use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
use inspectah_core::types::containers::{ComposeFile, ContainerSection, FlatpakApp, QuadletUnit};
use inspectah_core::types::fleet::VariantSelection;
use inspectah_core::types::kernelboot::{
    AlternativeEntry, ConfigSnippet, KernelBootSection, KernelModule, SysctlOverride,
};
use inspectah_core::types::network::{FirewallZone, NMConnection, NetworkSection, ProxyEntry};
use inspectah_core::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection};
use inspectah_core::types::rpm::{
    EnabledModuleStream, PackageEntry, RepoFile, RpmSection, VersionChange, VersionLockEntry,
};
use inspectah_core::types::scheduled::{
    AtJob, CronJob, GeneratedTimerUnit, ScheduledTaskSection, SystemdTimer,
};
use inspectah_core::types::selinux::{CarryForwardFile, SelinuxPortLabel, SelinuxSection};
use inspectah_core::types::services::{
    ServiceSection, ServiceStateChange, ServiceUnitState, SystemdDropIn,
};
use inspectah_core::types::storage::{FstabEntry, MountPoint, StorageSection};
use inspectah_core::types::users::UserGroupSection;

// ---------------------------------------------------------------------------
// PackageEntry
// ---------------------------------------------------------------------------

#[test]
fn test_package_entry_identity_key_is_name_dot_arch() {
    let pkg = PackageEntry {
        name: "httpd".into(),
        arch: "x86_64".into(),
        ..Default::default()
    };
    assert_eq!(pkg.identity_key().as_ref(), "httpd.x86_64");
}

#[test]
fn test_package_entry_has_no_variant_key() {
    assert!(PackageEntry::default().content_variant_key().is_none());
}

#[test]
fn test_package_entry_fleet_mut() {
    let mut pkg = PackageEntry::default();
    assert!(pkg.fleet_mut().is_none());
}

#[test]
fn test_package_entry_set_include() {
    let mut pkg = PackageEntry::default();
    assert!(!pkg.include);
    pkg.set_include(true);
    assert!(pkg.include);
}

// ---------------------------------------------------------------------------
// ConfigFileEntry (variant-capable)
// ---------------------------------------------------------------------------

#[test]
fn test_config_file_identity_key_is_path() {
    let entry = ConfigFileEntry {
        path: "/etc/foo.conf".into(),
        ..Default::default()
    };
    assert_eq!(entry.identity_key().as_ref(), "/etc/foo.conf");
}

#[test]
fn test_config_file_has_variant_key() {
    let entry = ConfigFileEntry {
        path: "/etc/foo.conf".into(),
        content: "val".into(),
        ..Default::default()
    };
    assert!(entry.content_variant_key().is_some());
}

#[test]
fn test_config_file_different_content_different_variant_key() {
    let a = ConfigFileEntry {
        content: "abc".into(),
        ..Default::default()
    };
    let b = ConfigFileEntry {
        content: "xyz".into(),
        ..Default::default()
    };
    assert_ne!(
        a.content_variant_key().unwrap().as_ref(),
        b.content_variant_key().unwrap().as_ref()
    );
}

#[test]
fn test_config_file_has_variant_selection_mut() {
    let mut entry = ConfigFileEntry::default();
    assert!(entry.variant_selection_mut().is_some());
}

// ---------------------------------------------------------------------------
// ComposeFile (variant-capable, uses images hash)
// ---------------------------------------------------------------------------

#[test]
fn test_compose_file_identity_key_is_path() {
    let cf = ComposeFile {
        path: "/opt/app/docker-compose.yml".into(),
        ..Default::default()
    };
    assert_eq!(cf.identity_key().as_ref(), "/opt/app/docker-compose.yml");
}

#[test]
fn test_compose_file_variant_key_uses_images() {
    let cf = ComposeFile {
        path: "/opt/app/docker-compose.yml".into(),
        images: vec![],
        ..Default::default()
    };
    assert!(cf.content_variant_key().is_some());
}

#[test]
fn test_compose_file_has_variant_selection_mut() {
    let mut cf = ComposeFile::default();
    assert!(cf.variant_selection_mut().is_some());
}

// ---------------------------------------------------------------------------
// SystemdDropIn (variant-capable)
// ---------------------------------------------------------------------------

#[test]
fn test_systemd_dropin_identity_key_is_path() {
    let d = SystemdDropIn {
        path: "/etc/systemd/system/httpd.service.d/override.conf".into(),
        ..Default::default()
    };
    assert_eq!(
        d.identity_key().as_ref(),
        "/etc/systemd/system/httpd.service.d/override.conf"
    );
}

#[test]
fn test_systemd_dropin_has_variant_key() {
    let d = SystemdDropIn {
        content: "[Service]\nLimitNOFILE=65535\n".into(),
        ..Default::default()
    };
    assert!(d.content_variant_key().is_some());
}

#[test]
fn test_systemd_dropin_has_variant_selection_mut() {
    let mut d = SystemdDropIn::default();
    assert!(d.variant_selection_mut().is_some());
}

// ---------------------------------------------------------------------------
// QuadletUnit (variant-capable)
// ---------------------------------------------------------------------------

#[test]
fn test_quadlet_unit_identity_key_is_path() {
    let q = QuadletUnit {
        path: "/etc/containers/systemd/myapp.container".into(),
        ..Default::default()
    };
    assert_eq!(
        q.identity_key().as_ref(),
        "/etc/containers/systemd/myapp.container"
    );
}

#[test]
fn test_quadlet_unit_has_variant_key() {
    let q = QuadletUnit {
        content: "[Container]\nImage=quay.io/myorg/myapp:latest\n".into(),
        ..Default::default()
    };
    assert!(q.content_variant_key().is_some());
}

#[test]
fn test_quadlet_unit_has_variant_selection_mut() {
    let mut q = QuadletUnit::default();
    assert!(q.variant_selection_mut().is_some());
}

// ---------------------------------------------------------------------------
// Types without variants
// ---------------------------------------------------------------------------

#[test]
fn test_repo_file_identity_is_path() {
    let r = RepoFile {
        path: "/etc/yum.repos.d/custom.repo".into(),
        ..Default::default()
    };
    assert_eq!(r.identity_key().as_ref(), "/etc/yum.repos.d/custom.repo");
}

#[test]
fn test_repo_file_has_no_variant_key() {
    assert!(RepoFile::default().content_variant_key().is_none());
}

#[test]
fn test_enabled_module_stream_identity() {
    let m = EnabledModuleStream {
        module_name: "nodejs".into(),
        stream: "18".into(),
        ..Default::default()
    };
    assert_eq!(m.identity_key().as_ref(), "nodejs:18");
}

#[test]
fn test_version_lock_entry_identity() {
    let v = VersionLockEntry {
        name: "kernel".into(),
        arch: "x86_64".into(),
        ..Default::default()
    };
    assert_eq!(v.identity_key().as_ref(), "kernel.x86_64");
}

#[test]
fn test_selinux_port_label_identity() {
    let s = SelinuxPortLabel {
        protocol: "tcp".into(),
        port: "8080".into(),
        ..Default::default()
    };
    assert_eq!(s.identity_key().as_ref(), "tcp:8080");
}

#[test]
fn test_nm_connection_identity_is_path() {
    let n = NMConnection {
        path: "/etc/NetworkManager/system-connections/eth0.nmconnection".into(),
        ..Default::default()
    };
    assert_eq!(
        n.identity_key().as_ref(),
        "/etc/NetworkManager/system-connections/eth0.nmconnection"
    );
}

#[test]
fn test_nm_connection_set_include_uses_option() {
    let mut n = NMConnection::default();
    assert!(n.include.is_none());
    n.set_include(true);
    assert_eq!(n.include, Some(true));
}

#[test]
fn test_firewall_zone_identity_is_path() {
    let f = FirewallZone {
        path: "/etc/firewalld/zones/public.xml".into(),
        ..Default::default()
    };
    assert_eq!(f.identity_key().as_ref(), "/etc/firewalld/zones/public.xml");
}

#[test]
fn test_kernel_module_identity_is_name() {
    let k = KernelModule {
        name: "vfio_pci".into(),
        ..Default::default()
    };
    assert_eq!(k.identity_key().as_ref(), "vfio_pci");
}

#[test]
fn test_sysctl_override_identity_is_key() {
    let s = SysctlOverride {
        key: "net.ipv4.ip_forward".into(),
        ..Default::default()
    };
    assert_eq!(s.identity_key().as_ref(), "net.ipv4.ip_forward");
}

#[test]
fn test_nonrpm_item_identity_is_name() {
    let n = NonRpmItem {
        name: "myapp".into(),
        ..Default::default()
    };
    assert_eq!(n.identity_key().as_ref(), "myapp");
}

#[test]
fn test_cron_job_identity_is_path() {
    let c = CronJob {
        path: "/etc/cron.d/backup".into(),
        ..Default::default()
    };
    assert_eq!(c.identity_key().as_ref(), "/etc/cron.d/backup");
}

// ===========================================================================
// merge_items — basic prevalence (non-variant types)
// ===========================================================================

#[test]
fn test_merge_items_two_hosts_same_package() {
    let items: Vec<(usize, PackageEntry)> = vec![
        (
            0,
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                ..Default::default()
            },
        ),
        (
            1,
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                ..Default::default()
            },
        ),
    ];
    let hostnames = vec!["host-a".to_string(), "host-b".to_string()];
    let merged = merge_items(items, 2, &hostnames);
    assert_eq!(merged.len(), 1);
    let fleet = merged[0].fleet.as_ref().unwrap();
    assert_eq!(fleet.count, 2);
    assert_eq!(fleet.total, 2);
    assert_eq!(fleet.hosts, vec!["host-a", "host-b"]);
    assert!(merged[0].include);
}

#[test]
fn test_merge_items_different_packages_stay_separate() {
    let items: Vec<(usize, PackageEntry)> = vec![
        (
            0,
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                ..Default::default()
            },
        ),
        (
            1,
            PackageEntry {
                name: "nginx".into(),
                arch: "x86_64".into(),
                ..Default::default()
            },
        ),
    ];
    let hostnames = vec!["host-a".to_string(), "host-b".to_string()];
    let merged = merge_items(items, 2, &hostnames);
    assert_eq!(merged.len(), 2);
    // Sorted by identity key
    assert_eq!(merged[0].name, "httpd");
    assert_eq!(merged[1].name, "nginx");
    assert_eq!(merged[0].fleet.as_ref().unwrap().count, 1);
    assert_eq!(merged[1].fleet.as_ref().unwrap().count, 1);
}

#[test]
fn test_merge_items_empty_input() {
    let items: Vec<(usize, PackageEntry)> = vec![];
    let hostnames: Vec<String> = vec!["host-a".into()];
    let merged = merge_items(items, 1, &hostnames);
    assert!(merged.is_empty());
}

#[test]
fn test_merge_items_single_host() {
    let items: Vec<(usize, PackageEntry)> = vec![(
        0,
        PackageEntry {
            name: "curl".into(),
            arch: "x86_64".into(),
            ..Default::default()
        },
    )];
    let hostnames = vec!["solo-host".to_string()];
    let merged = merge_items(items, 1, &hostnames);
    assert_eq!(merged.len(), 1);
    let fleet = merged[0].fleet.as_ref().unwrap();
    assert_eq!(fleet.count, 1);
    assert_eq!(fleet.total, 1);
    assert_eq!(fleet.hosts, vec!["solo-host"]);
}

#[test]
fn test_merge_items_deduplicates_same_host_index() {
    // Same host index appearing twice (duplicate item in a single snapshot)
    let items: Vec<(usize, PackageEntry)> = vec![
        (
            0,
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                ..Default::default()
            },
        ),
        (
            0,
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                ..Default::default()
            },
        ),
    ];
    let hostnames = vec!["host-a".to_string()];
    let merged = merge_items(items, 1, &hostnames);
    assert_eq!(merged.len(), 1);
    let fleet = merged[0].fleet.as_ref().unwrap();
    // Count should be 1 (one unique host), not 2
    assert_eq!(fleet.count, 1);
    assert_eq!(fleet.hosts, vec!["host-a"]);
}

#[test]
fn test_merge_items_output_sorted_by_identity_key() {
    let items: Vec<(usize, PackageEntry)> = vec![
        (
            0,
            PackageEntry {
                name: "zlib".into(),
                arch: "x86_64".into(),
                ..Default::default()
            },
        ),
        (
            0,
            PackageEntry {
                name: "curl".into(),
                arch: "x86_64".into(),
                ..Default::default()
            },
        ),
        (
            0,
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                ..Default::default()
            },
        ),
    ];
    let hostnames = vec!["host-a".to_string()];
    let merged = merge_items(items, 1, &hostnames);
    assert_eq!(merged.len(), 3);
    assert_eq!(merged[0].name, "curl");
    assert_eq!(merged[1].name, "httpd");
    assert_eq!(merged[2].name, "zlib");
}

#[test]
fn test_merge_items_hosts_list_sorted() {
    let items: Vec<(usize, PackageEntry)> = vec![
        (
            2,
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                ..Default::default()
            },
        ),
        (
            0,
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                ..Default::default()
            },
        ),
        (
            1,
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                ..Default::default()
            },
        ),
    ];
    let hostnames = vec![
        "charlie".to_string(),
        "bravo".to_string(),
        "alpha".to_string(),
    ];
    let merged = merge_items(items, 3, &hostnames);
    assert_eq!(merged.len(), 1);
    let fleet = merged[0].fleet.as_ref().unwrap();
    // Hosts sorted alphabetically
    assert_eq!(fleet.hosts, vec!["alpha", "bravo", "charlie"]);
}

// ===========================================================================
// merge_items — variant handling (ConfigFileEntry)
// ===========================================================================

#[test]
fn test_merge_items_variant_selection() {
    let items: Vec<(usize, ConfigFileEntry)> = vec![
        (
            0,
            ConfigFileEntry {
                path: "/etc/foo.conf".into(),
                content: "version_a".into(),
                ..Default::default()
            },
        ),
        (
            1,
            ConfigFileEntry {
                path: "/etc/foo.conf".into(),
                content: "version_a".into(),
                ..Default::default()
            },
        ),
        (
            2,
            ConfigFileEntry {
                path: "/etc/foo.conf".into(),
                content: "version_b".into(),
                ..Default::default()
            },
        ),
    ];
    let hostnames = vec!["h1".into(), "h2".into(), "h3".into()];
    let merged = merge_items(items, 3, &hostnames);
    assert_eq!(merged.len(), 2);
    let selected = merged
        .iter()
        .find(|e| e.variant_selection == VariantSelection::Selected)
        .unwrap();
    let alt = merged
        .iter()
        .find(|e| e.variant_selection == VariantSelection::Alternative)
        .unwrap();
    assert_eq!(selected.content, "version_a");
    assert_eq!(selected.fleet.as_ref().unwrap().count, 2);
    assert_eq!(alt.content, "version_b");
    assert_eq!(alt.fleet.as_ref().unwrap().count, 1);
}

#[test]
fn test_merge_items_single_variant_is_only() {
    let items: Vec<(usize, ConfigFileEntry)> = vec![
        (
            0,
            ConfigFileEntry {
                path: "/etc/foo.conf".into(),
                content: "same".into(),
                ..Default::default()
            },
        ),
        (
            1,
            ConfigFileEntry {
                path: "/etc/foo.conf".into(),
                content: "same".into(),
                ..Default::default()
            },
        ),
    ];
    let hostnames = vec!["h1".into(), "h2".into()];
    let merged = merge_items(items, 2, &hostnames);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].variant_selection, VariantSelection::Only);
}

#[test]
fn test_merge_items_variant_tie_break_deterministic() {
    // Two variants with equal prevalence — tie broken by content hash.
    // Verify that reversing input order produces the same Selected winner.
    let items_forward: Vec<(usize, ConfigFileEntry)> = vec![
        (
            0,
            ConfigFileEntry {
                path: "/etc/app.conf".into(),
                content: "alpha_content".into(),
                ..Default::default()
            },
        ),
        (
            1,
            ConfigFileEntry {
                path: "/etc/app.conf".into(),
                content: "beta_content".into(),
                ..Default::default()
            },
        ),
    ];
    let items_reverse: Vec<(usize, ConfigFileEntry)> = vec![
        (
            0,
            ConfigFileEntry {
                path: "/etc/app.conf".into(),
                content: "beta_content".into(),
                ..Default::default()
            },
        ),
        (
            1,
            ConfigFileEntry {
                path: "/etc/app.conf".into(),
                content: "alpha_content".into(),
                ..Default::default()
            },
        ),
    ];
    let hostnames = vec!["h1".into(), "h2".into()];

    let merged_fwd = merge_items(items_forward, 2, &hostnames);
    let merged_rev = merge_items(items_reverse, 2, &hostnames);

    assert_eq!(merged_fwd.len(), 2);
    assert_eq!(merged_rev.len(), 2);

    let selected_fwd = merged_fwd
        .iter()
        .find(|e| e.variant_selection == VariantSelection::Selected)
        .unwrap();
    let selected_rev = merged_rev
        .iter()
        .find(|e| e.variant_selection == VariantSelection::Selected)
        .unwrap();

    // Same content wins regardless of input order
    assert_eq!(selected_fwd.content, selected_rev.content);
}

#[test]
fn test_merge_items_three_variants() {
    let items: Vec<(usize, ConfigFileEntry)> = vec![
        (
            0,
            ConfigFileEntry {
                path: "/etc/app.conf".into(),
                content: "v1".into(),
                ..Default::default()
            },
        ),
        (
            1,
            ConfigFileEntry {
                path: "/etc/app.conf".into(),
                content: "v1".into(),
                ..Default::default()
            },
        ),
        (
            2,
            ConfigFileEntry {
                path: "/etc/app.conf".into(),
                content: "v1".into(),
                ..Default::default()
            },
        ),
        (
            3,
            ConfigFileEntry {
                path: "/etc/app.conf".into(),
                content: "v2".into(),
                ..Default::default()
            },
        ),
        (
            4,
            ConfigFileEntry {
                path: "/etc/app.conf".into(),
                content: "v3".into(),
                ..Default::default()
            },
        ),
    ];
    let hostnames: Vec<String> = (0..5).map(|i| format!("host-{}", i)).collect();
    let merged = merge_items(items, 5, &hostnames);
    assert_eq!(merged.len(), 3);

    let selected = merged
        .iter()
        .find(|e| e.variant_selection == VariantSelection::Selected)
        .unwrap();
    assert_eq!(selected.content, "v1");
    assert_eq!(selected.fleet.as_ref().unwrap().count, 3);

    let alternatives: Vec<&ConfigFileEntry> = merged
        .iter()
        .filter(|e| e.variant_selection == VariantSelection::Alternative)
        .collect();
    assert_eq!(alternatives.len(), 2);
    for alt in &alternatives {
        assert_eq!(alt.fleet.as_ref().unwrap().count, 1);
    }
}

#[test]
fn test_merge_items_mixed_paths_with_variants() {
    // Two different config paths, each with its own variants
    let items: Vec<(usize, ConfigFileEntry)> = vec![
        (
            0,
            ConfigFileEntry {
                path: "/etc/a.conf".into(),
                content: "content_a".into(),
                ..Default::default()
            },
        ),
        (
            1,
            ConfigFileEntry {
                path: "/etc/a.conf".into(),
                content: "content_a".into(),
                ..Default::default()
            },
        ),
        (
            0,
            ConfigFileEntry {
                path: "/etc/b.conf".into(),
                content: "content_b1".into(),
                ..Default::default()
            },
        ),
        (
            1,
            ConfigFileEntry {
                path: "/etc/b.conf".into(),
                content: "content_b2".into(),
                ..Default::default()
            },
        ),
    ];
    let hostnames = vec!["h1".into(), "h2".into()];
    let merged = merge_items(items, 2, &hostnames);

    // /etc/a.conf: 1 item (Only), /etc/b.conf: 2 items (Selected + Alternative)
    assert_eq!(merged.len(), 3);

    let a_items: Vec<&ConfigFileEntry> =
        merged.iter().filter(|e| e.path == "/etc/a.conf").collect();
    assert_eq!(a_items.len(), 1);
    assert_eq!(a_items[0].variant_selection, VariantSelection::Only);

    let b_items: Vec<&ConfigFileEntry> =
        merged.iter().filter(|e| e.path == "/etc/b.conf").collect();
    assert_eq!(b_items.len(), 2);
    assert!(
        b_items
            .iter()
            .any(|e| e.variant_selection == VariantSelection::Selected)
    );
    assert!(
        b_items
            .iter()
            .any(|e| e.variant_selection == VariantSelection::Alternative)
    );
}

#[test]
fn test_merge_items_all_variants_included() {
    // Even alternative variants should have include = true
    let items: Vec<(usize, ConfigFileEntry)> = vec![
        (
            0,
            ConfigFileEntry {
                path: "/etc/foo.conf".into(),
                content: "aaa".into(),
                ..Default::default()
            },
        ),
        (
            1,
            ConfigFileEntry {
                path: "/etc/foo.conf".into(),
                content: "bbb".into(),
                ..Default::default()
            },
        ),
    ];
    let hostnames = vec!["h1".into(), "h2".into()];
    let merged = merge_items(items, 2, &hostnames);
    for item in &merged {
        assert!(item.include, "all merged items should be included");
    }
}

#[test]
fn test_merge_items_variant_total_reflects_fleet_size() {
    let items: Vec<(usize, ConfigFileEntry)> = vec![
        (
            0,
            ConfigFileEntry {
                path: "/etc/foo.conf".into(),
                content: "v1".into(),
                ..Default::default()
            },
        ),
        (
            1,
            ConfigFileEntry {
                path: "/etc/foo.conf".into(),
                content: "v2".into(),
                ..Default::default()
            },
        ),
    ];
    let hostnames = vec!["h1".into(), "h2".into()];
    let merged = merge_items(items, 5, &hostnames);
    // total should reflect the fleet size (5), not just the hosts that have this item
    for item in &merged {
        assert_eq!(item.fleet.as_ref().unwrap().total, 5);
    }
}

// ===========================================================================
// New FleetMergeable impls (Task 9)
// ===========================================================================

#[test]
fn test_systemd_timer_identity_is_name() {
    let t = SystemdTimer {
        name: "backup.timer".into(),
        ..Default::default()
    };
    assert_eq!(t.identity_key().as_ref(), "backup.timer");
}

#[test]
fn test_at_job_identity_is_file() {
    let a = AtJob {
        file: "/var/spool/at/a00001".into(),
        ..Default::default()
    };
    assert_eq!(a.identity_key().as_ref(), "/var/spool/at/a00001");
}

#[test]
fn test_generated_timer_unit_identity_is_name() {
    let g = GeneratedTimerUnit {
        name: "cron-daily-backup.timer".into(),
        ..Default::default()
    };
    assert_eq!(g.identity_key().as_ref(), "cron-daily-backup.timer");
}

#[test]
fn test_fstab_entry_identity_is_mount_point() {
    let f = FstabEntry {
        mount_point: "/data".into(),
        ..Default::default()
    };
    assert_eq!(f.identity_key().as_ref(), "/data");
}

#[test]
fn test_systemd_timer_set_include_uses_option() {
    let mut t = SystemdTimer::default();
    assert!(t.include.is_none());
    t.set_include(true);
    assert_eq!(t.include, Some(true));
}

#[test]
fn test_fstab_entry_set_include_uses_option() {
    let mut f = FstabEntry::default();
    assert!(f.include.is_none());
    f.set_include(true);
    assert_eq!(f.include, Some(true));
}

// ===========================================================================
// dedup_strings
// ===========================================================================

#[test]
fn test_dedup_strings_merges_and_sorts() {
    let lists = vec![vec!["c".into(), "a".into()], vec!["b".into(), "a".into()]];
    let result = dedup_strings(lists);
    assert_eq!(result, vec!["a", "b", "c"]);
}

#[test]
fn test_dedup_strings_empty_input() {
    let result = dedup_strings(vec![]);
    assert!(result.is_empty());
}

#[test]
fn test_dedup_strings_single_list() {
    let lists = vec![vec!["x".into(), "y".into(), "x".into()]];
    let result = dedup_strings(lists);
    assert_eq!(result, vec!["x", "y"]);
}

// ===========================================================================
// dedup_json_values
// ===========================================================================

#[test]
fn test_dedup_json_values_removes_duplicates() {
    let lists = vec![
        vec![serde_json::json!({"a": 1}), serde_json::json!({"b": 2})],
        vec![serde_json::json!({"a": 1}), serde_json::json!({"c": 3})],
    ];
    let result = dedup_json_values(lists);
    assert_eq!(result.len(), 3);
}

// ===========================================================================
// Section adapter: RPM
// ===========================================================================

#[test]
fn test_merge_rpm_sections_all_none() {
    let result = merge_rpm_sections(vec![None, None], 2, &["h1".into(), "h2".into()], None);
    assert!(result.is_none());
}

#[test]
fn test_merge_rpm_sections_packages_merged() {
    let s1 = RpmSection {
        packages_added: vec![PackageEntry {
            name: "httpd".into(),
            arch: "x86_64".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let s2 = RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "nginx".into(),
                arch: "x86_64".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let (result, _) = merge_rpm_sections(vec![Some(s1), Some(s2)], 2, &hostnames, Some(0)).unwrap();

    assert_eq!(result.packages_added.len(), 2);
    let httpd = result
        .packages_added
        .iter()
        .find(|p| p.name == "httpd")
        .unwrap();
    assert_eq!(httpd.fleet.as_ref().unwrap().count, 2);
    let nginx = result
        .packages_added
        .iter()
        .find(|p| p.name == "nginx")
        .unwrap();
    assert_eq!(nginx.fleet.as_ref().unwrap().count, 1);
}

#[test]
fn test_merge_rpm_sections_dedup_strings() {
    let s1 = RpmSection {
        dnf_history_removed: vec!["pkg-a".into(), "pkg-b".into()],
        multiarch_packages: vec!["glibc".into()],
        ..Default::default()
    };
    let s2 = RpmSection {
        dnf_history_removed: vec!["pkg-b".into(), "pkg-c".into()],
        multiarch_packages: vec!["glibc".into(), "openssl".into()],
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let (result, _) = merge_rpm_sections(vec![Some(s1), Some(s2)], 2, &hostnames, Some(0)).unwrap();

    assert_eq!(result.dnf_history_removed, vec!["pkg-a", "pkg-b", "pkg-c"]);
    assert_eq!(result.multiarch_packages, vec!["glibc", "openssl"]);
}

#[test]
fn test_merge_rpm_sections_version_changes_dedup() {
    let s1 = RpmSection {
        version_changes: vec![VersionChange {
            name: "kernel".into(),
            arch: "x86_64".into(),
            host_version: "5.14.0-200".into(),
            base_version: "5.14.0-100".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let s2 = RpmSection {
        version_changes: vec![VersionChange {
            name: "kernel".into(),
            arch: "x86_64".into(),
            host_version: "5.14.0-200".into(),
            base_version: "5.14.0-100".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let (result, _) = merge_rpm_sections(vec![Some(s1), Some(s2)], 2, &hostnames, Some(0)).unwrap();

    // Should be deduped to 1 entry
    assert_eq!(result.version_changes.len(), 1);
    assert_eq!(result.version_changes[0].name, "kernel");
}

#[test]
fn test_merge_rpm_sections_passthrough_scalars() {
    let s1 = RpmSection {
        no_baseline: true,
        base_image: Some("registry.example.com/image:latest".into()),
        leaf_packages: Some(vec!["httpd.x86_64".into()]),
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["h1".into()];
    let (result, _) = merge_rpm_sections(vec![Some(s1)], 1, &hostnames, Some(0)).unwrap();

    assert!(result.no_baseline);
    assert_eq!(
        result.base_image,
        Some("registry.example.com/image:latest".into())
    );
    assert_eq!(result.leaf_packages, Some(vec!["httpd.x86_64".into()]));
}

// ===========================================================================
// Section adapter: Config
// ===========================================================================

#[test]
fn test_merge_config_sections_all_none() {
    let result = merge_config_sections(vec![None, None], 2, &["h1".into(), "h2".into()]);
    assert!(result.is_none());
}

#[test]
fn test_merge_config_sections_files_with_variants() {
    let s1 = ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/httpd/conf/httpd.conf".into(),
            content: "ServerName h1".into(),
            ..Default::default()
        }],
    };
    let s2 = ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/httpd/conf/httpd.conf".into(),
            content: "ServerName h2".into(),
            ..Default::default()
        }],
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let result = merge_config_sections(vec![Some(s1), Some(s2)], 2, &hostnames).unwrap();

    // Two variants of the same path
    assert_eq!(result.files.len(), 2);
    assert!(
        result
            .files
            .iter()
            .any(|f| f.variant_selection == VariantSelection::Selected)
    );
    assert!(
        result
            .files
            .iter()
            .any(|f| f.variant_selection == VariantSelection::Alternative)
    );
}

// ===========================================================================
// Section adapter: Services
// ===========================================================================

#[test]
fn test_merge_service_sections_all_none() {
    let result = merge_service_sections(vec![None], 1, &["h1".into()]);
    assert!(result.is_none());
}

#[test]
fn test_merge_service_sections_dedup_units() {
    let make_sc = |unit: &str| ServiceStateChange {
        unit: unit.into(),
        current_state: ServiceUnitState::Enabled,
        default_state: None,
        include: false,
        owning_package: None,
        fleet: None,
        attention_reason: None,
    };
    let s1 = ServiceSection {
        state_changes: vec![make_sc("httpd.service")],
        enabled_units: vec!["sshd.service".into(), "httpd.service".into()],
        disabled_units: vec!["firewalld.service".into()],
        drop_ins: vec![],
        preset_matched_units: vec!["chronyd.service".into()],
    };
    let s2 = ServiceSection {
        state_changes: vec![make_sc("httpd.service")],
        enabled_units: vec!["httpd.service".into(), "crond.service".into()],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec!["chronyd.service".into(), "sshd.service".into()],
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let result = merge_service_sections(vec![Some(s1), Some(s2)], 2, &hostnames).unwrap();

    // state_changes merged: httpd present on both hosts
    assert_eq!(result.state_changes.len(), 1);
    assert_eq!(result.state_changes[0].fleet.as_ref().unwrap().count, 2);

    // String lists deduped and sorted
    assert_eq!(
        result.enabled_units,
        vec!["crond.service", "httpd.service", "sshd.service"]
    );
    assert_eq!(result.disabled_units, vec!["firewalld.service"]);
    assert_eq!(
        result.preset_matched_units,
        vec!["chronyd.service", "sshd.service"]
    );
}

// ===========================================================================
// Section adapter: Containers
// ===========================================================================

#[test]
fn test_merge_container_sections_all_none() {
    let result = merge_container_sections(vec![None], 1, &["h1".into()]);
    assert!(result.is_none());
}

#[test]
fn test_merge_container_sections_flatpak_dedup() {
    let s1 = ContainerSection {
        flatpak_apps: vec![FlatpakApp {
            app_id: "org.gnome.Calculator".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let s2 = ContainerSection {
        flatpak_apps: vec![
            FlatpakApp {
                app_id: "org.gnome.Calculator".into(),
                ..Default::default()
            },
            FlatpakApp {
                app_id: "org.mozilla.Firefox".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let result = merge_container_sections(vec![Some(s1), Some(s2)], 2, &hostnames).unwrap();

    assert_eq!(result.flatpak_apps.len(), 2);
    assert!(result.running_containers.is_empty()); // runtime state skipped
}

#[test]
fn test_merge_container_sections_quadlets_with_variants() {
    let s1 = ContainerSection {
        quadlet_units: vec![QuadletUnit {
            path: "/etc/containers/systemd/app.container".into(),
            content: "Image=quay.io/app:v1".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let s2 = ContainerSection {
        quadlet_units: vec![QuadletUnit {
            path: "/etc/containers/systemd/app.container".into(),
            content: "Image=quay.io/app:v2".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let result = merge_container_sections(vec![Some(s1), Some(s2)], 2, &hostnames).unwrap();

    // Two variants
    assert_eq!(result.quadlet_units.len(), 2);
    assert!(
        result
            .quadlet_units
            .iter()
            .any(|q| q.variant_selection == VariantSelection::Selected)
    );
}

// ===========================================================================
// Section adapter: Network
// ===========================================================================

#[test]
fn test_merge_network_sections_all_none() {
    let result = merge_network_sections(vec![None], 1, &["h1".into()]);
    assert!(result.is_none());
}

#[test]
fn test_merge_network_sections_dedup_proxy_by_source() {
    let s1 = NetworkSection {
        proxy: vec![ProxyEntry {
            source: "/etc/profile.d/proxy.sh".into(),
            line: "HTTP_PROXY=http://proxy:8080".into(),
        }],
        ip_routes: vec!["10.0.0.0/8 via 192.168.1.1".into()],
        ..Default::default()
    };
    let s2 = NetworkSection {
        proxy: vec![ProxyEntry {
            source: "/etc/profile.d/proxy.sh".into(),
            line: "HTTP_PROXY=http://proxy:8080".into(),
        }],
        ip_routes: vec![
            "10.0.0.0/8 via 192.168.1.1".into(),
            "172.16.0.0/12 via 192.168.1.1".into(),
        ],
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let result = merge_network_sections(vec![Some(s1), Some(s2)], 2, &hostnames).unwrap();

    assert_eq!(result.proxy.len(), 1);
    assert_eq!(result.ip_routes.len(), 2);
}

#[test]
fn test_merge_network_resolv_provenance_most_prevalent() {
    // 3 hosts: 2 have "systemd-resolved", 1 has "NetworkManager".
    // Most-prevalent should win even though "NetworkManager" sorts first.
    let s1 = NetworkSection {
        resolv_provenance: "NetworkManager".into(),
        ..Default::default()
    };
    let s2 = NetworkSection {
        resolv_provenance: "systemd-resolved".into(),
        ..Default::default()
    };
    let s3 = NetworkSection {
        resolv_provenance: "systemd-resolved".into(),
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into(), "h3".into()];
    let result = merge_network_sections(vec![Some(s1), Some(s2), Some(s3)], 3, &hostnames).unwrap();

    assert_eq!(result.resolv_provenance, "systemd-resolved");
}

#[test]
fn test_merge_network_resolv_provenance_tie_break() {
    // 2 hosts with different values — tie broken by first-seen
    let s1 = NetworkSection {
        resolv_provenance: "NetworkManager".into(),
        ..Default::default()
    };
    let s2 = NetworkSection {
        resolv_provenance: "systemd-resolved".into(),
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let result = merge_network_sections(vec![Some(s1), Some(s2)], 2, &hostnames).unwrap();

    // Tie: first-seen wins (h1's value)
    assert_eq!(result.resolv_provenance, "NetworkManager");
}

// ===========================================================================
// Section adapter: Storage
// ===========================================================================

#[test]
fn test_merge_storage_sections_all_none() {
    let result = merge_storage_sections(vec![None], 1, &["h1".into()]);
    assert!(result.is_none());
}

#[test]
fn test_merge_storage_sections_fstab_merged() {
    let s1 = StorageSection {
        fstab_entries: vec![FstabEntry {
            mount_point: "/data".into(),
            device: "/dev/sda1".into(),
            ..Default::default()
        }],
        mount_points: vec![MountPoint {
            target: "/".into(),
            source: "/dev/sda2".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let s2 = StorageSection {
        fstab_entries: vec![FstabEntry {
            mount_point: "/data".into(),
            device: "/dev/sdb1".into(),
            ..Default::default()
        }],
        mount_points: vec![
            MountPoint {
                target: "/".into(),
                source: "/dev/sda2".into(),
                ..Default::default()
            },
            MountPoint {
                target: "/home".into(),
                source: "/dev/sda3".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let result = merge_storage_sections(vec![Some(s1), Some(s2)], 2, &hostnames).unwrap();

    // fstab merged by mount_point identity
    assert_eq!(result.fstab_entries.len(), 1);
    assert_eq!(result.fstab_entries[0].fleet.as_ref().unwrap().count, 2);

    // mount_points deduped by target
    assert_eq!(result.mount_points.len(), 2);
}

// ===========================================================================
// Section adapter: Scheduled Tasks
// ===========================================================================

#[test]
fn test_merge_scheduled_sections_all_none() {
    let result = merge_scheduled_sections(vec![None], 1, &["h1".into()]);
    assert!(result.is_none());
}

#[test]
fn test_merge_scheduled_sections_cron_and_timers() {
    let s1 = ScheduledTaskSection {
        cron_jobs: vec![CronJob {
            path: "/etc/cron.d/backup".into(),
            ..Default::default()
        }],
        systemd_timers: vec![SystemdTimer {
            name: "logrotate.timer".into(),
            ..Default::default()
        }],
        at_jobs: vec![],
        generated_timer_units: vec![],
    };
    let s2 = ScheduledTaskSection {
        cron_jobs: vec![CronJob {
            path: "/etc/cron.d/backup".into(),
            ..Default::default()
        }],
        systemd_timers: vec![
            SystemdTimer {
                name: "logrotate.timer".into(),
                ..Default::default()
            },
            SystemdTimer {
                name: "fstrim.timer".into(),
                ..Default::default()
            },
        ],
        at_jobs: vec![AtJob {
            file: "/var/spool/at/a00001".into(),
            ..Default::default()
        }],
        generated_timer_units: vec![GeneratedTimerUnit {
            name: "cron-daily-backup.timer".into(),
            ..Default::default()
        }],
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let result = merge_scheduled_sections(vec![Some(s1), Some(s2)], 2, &hostnames).unwrap();

    assert_eq!(result.cron_jobs.len(), 1);
    assert_eq!(result.cron_jobs[0].fleet.as_ref().unwrap().count, 2);
    assert_eq!(result.systemd_timers.len(), 2);
    assert_eq!(result.at_jobs.len(), 1);
    assert_eq!(result.generated_timer_units.len(), 1);
}

// ===========================================================================
// Section adapter: SELinux
// ===========================================================================

#[test]
fn test_merge_selinux_sections_all_none() {
    let result = merge_selinux_sections(vec![None], 1, &["h1".into()]);
    assert!(result.is_none());
}

#[test]
fn test_merge_selinux_sections_dedup_and_merge() {
    let s1 = SelinuxSection {
        mode: "enforcing".into(),
        port_labels: vec![SelinuxPortLabel {
            protocol: "tcp".into(),
            port: "8080".into(),
            ..Default::default()
        }],
        custom_modules: vec!["mymodule".into()],
        fcontext_rules: vec!["/opt/app(/.*)?".into()],
        boolean_overrides: vec![serde_json::json!({"httpd_can_network_connect": true})],
        audit_rules: vec![CarryForwardFile {
            path: "etc/audit/rules.d/custom.rules".into(),
            content: "-w /etc/passwd".into(),
        }],
        pam_configs: vec![],
        fips_mode: false,
    };
    let s2 = SelinuxSection {
        mode: "enforcing".into(),
        port_labels: vec![SelinuxPortLabel {
            protocol: "tcp".into(),
            port: "8080".into(),
            ..Default::default()
        }],
        custom_modules: vec!["mymodule".into(), "othermodule".into()],
        fcontext_rules: vec!["/opt/app(/.*)?".into()],
        boolean_overrides: vec![serde_json::json!({"httpd_can_network_connect": true})],
        audit_rules: vec![CarryForwardFile {
            path: "etc/audit/rules.d/custom.rules".into(),
            content: "-w /etc/passwd".into(),
        }],
        pam_configs: vec![],
        fips_mode: false,
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let result = merge_selinux_sections(vec![Some(s1), Some(s2)], 2, &hostnames).unwrap();

    // Port labels merged via merge_items
    assert_eq!(result.port_labels.len(), 1);
    assert_eq!(result.port_labels[0].fleet.as_ref().unwrap().count, 2);

    // String lists deduped
    assert_eq!(result.custom_modules, vec!["mymodule", "othermodule"]);
    assert_eq!(result.fcontext_rules, vec!["/opt/app(/.*)?"]);

    // JSON deduped
    assert_eq!(result.boolean_overrides.len(), 1);

    // CarryForwardFile deduped by path
    assert_eq!(result.audit_rules.len(), 1);

    // Most-prevalent scalar (both hosts agree here)
    assert_eq!(result.mode, "enforcing");
}

#[test]
fn test_merge_selinux_most_prevalent_mode() {
    // 3 hosts: 2 enforcing, 1 permissive — enforcing wins by prevalence
    let make = |mode: &str, fips: bool| SelinuxSection {
        mode: mode.into(),
        fips_mode: fips,
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into(), "h3".into()];
    let result = merge_selinux_sections(
        vec![
            Some(make("permissive", true)),
            Some(make("enforcing", false)),
            Some(make("enforcing", false)),
        ],
        3,
        &hostnames,
    )
    .unwrap();

    // enforcing is most prevalent (2 of 3)
    assert_eq!(result.mode, "enforcing");
    // false is most prevalent for fips_mode (2 of 3)
    assert!(!result.fips_mode);
}

#[test]
fn test_merge_selinux_most_prevalent_tie_break() {
    // 2 hosts with different modes — tie broken by first-seen (first in sorted order)
    let make = |mode: &str| SelinuxSection {
        mode: mode.into(),
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let result = merge_selinux_sections(
        vec![Some(make("permissive")), Some(make("enforcing"))],
        2,
        &hostnames,
    )
    .unwrap();

    // Tie: first-seen wins. Sections are pre-sorted by hostname, so h1's
    // value ("permissive") is first-seen.
    assert_eq!(result.mode, "permissive");
}

#[test]
fn test_merge_selinux_fips_mode_most_prevalent_true() {
    // 3 hosts: 2 true, 1 false — true wins
    let make = |fips: bool| SelinuxSection {
        mode: "enforcing".into(),
        fips_mode: fips,
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into(), "h3".into()];
    let result = merge_selinux_sections(
        vec![Some(make(false)), Some(make(true)), Some(make(true))],
        3,
        &hostnames,
    )
    .unwrap();

    assert!(result.fips_mode);
}

// ===========================================================================
// Section adapter: KernelBoot
// ===========================================================================

#[test]
fn test_merge_kernelboot_sections_all_none() {
    let result = merge_kernelboot_sections(vec![None], 1, &["h1".into()]);
    assert!(result.is_none());
}

#[test]
fn test_merge_kernelboot_sections_modules_and_snippets() {
    let s1 = KernelBootSection {
        cmdline: "root=/dev/sda1 console=ttyS0".into(),
        sysctl_overrides: vec![SysctlOverride {
            key: "net.ipv4.ip_forward".into(),
            runtime: "1".into(),
            ..Default::default()
        }],
        loaded_modules: vec![KernelModule {
            name: "vfio_pci".into(),
            ..Default::default()
        }],
        modules_load_d: vec![ConfigSnippet {
            path: "/etc/modules-load.d/vfio.conf".into(),
            content: "vfio_pci".into(),
        }],
        alternatives: vec![AlternativeEntry {
            name: "python3".into(),
            path: "/usr/bin/python3.11".into(),
            status: "auto".into(),
        }],
        ..Default::default()
    };
    let s2 = KernelBootSection {
        cmdline: "root=/dev/sda1 console=ttyS0".into(),
        sysctl_overrides: vec![SysctlOverride {
            key: "net.ipv4.ip_forward".into(),
            runtime: "1".into(),
            ..Default::default()
        }],
        loaded_modules: vec![
            KernelModule {
                name: "vfio_pci".into(),
                ..Default::default()
            },
            KernelModule {
                name: "br_netfilter".into(),
                ..Default::default()
            },
        ],
        modules_load_d: vec![ConfigSnippet {
            path: "/etc/modules-load.d/vfio.conf".into(),
            content: "vfio_pci".into(),
        }],
        alternatives: vec![AlternativeEntry {
            name: "python3".into(),
            path: "/usr/bin/python3.11".into(),
            status: "auto".into(),
        }],
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let result = merge_kernelboot_sections(vec![Some(s1), Some(s2)], 2, &hostnames).unwrap();

    // sysctl merged
    assert_eq!(result.sysctl_overrides.len(), 1);
    assert_eq!(result.sysctl_overrides[0].fleet.as_ref().unwrap().count, 2);

    // loaded_modules merged
    assert_eq!(result.loaded_modules.len(), 2);

    // ConfigSnippets deduped by path
    assert_eq!(result.modules_load_d.len(), 1);

    // alternatives deduped by name
    assert_eq!(result.alternatives.len(), 1);

    // Most-prevalent scalar (both hosts agree here)
    assert_eq!(result.cmdline, "root=/dev/sda1 console=ttyS0");
}

#[test]
fn test_merge_kernelboot_most_prevalent_scalars() {
    // 3 hosts: 2 share cmdline/grub_defaults/tuned_active, 1 differs.
    // Most-prevalent value should win even though the differing host sorts first.
    let s_minority = KernelBootSection {
        cmdline: "root=/dev/sda1 quiet".into(),
        grub_defaults: "GRUB_TIMEOUT=3".into(),
        tuned_active: "balanced".into(),
        locale: Some("en_US.UTF-8".into()),
        ..Default::default()
    };
    let s_majority1 = KernelBootSection {
        cmdline: "root=/dev/sda1 console=ttyS0".into(),
        grub_defaults: "GRUB_TIMEOUT=5".into(),
        tuned_active: "throughput-performance".into(),
        locale: Some("de_DE.UTF-8".into()),
        ..Default::default()
    };
    let s_majority2 = KernelBootSection {
        cmdline: "root=/dev/sda1 console=ttyS0".into(),
        grub_defaults: "GRUB_TIMEOUT=5".into(),
        tuned_active: "throughput-performance".into(),
        locale: Some("fr_FR.UTF-8".into()),
        ..Default::default()
    };
    // h1 sorts first but is minority; h2 and h3 are majority
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into(), "h3".into()];
    let result = merge_kernelboot_sections(
        vec![Some(s_minority), Some(s_majority1), Some(s_majority2)],
        3,
        &hostnames,
    )
    .unwrap();

    // Most-prevalent wins (2 of 3)
    assert_eq!(result.cmdline, "root=/dev/sda1 console=ttyS0");
    assert_eq!(result.grub_defaults, "GRUB_TIMEOUT=5");
    assert_eq!(result.tuned_active, "throughput-performance");

    // locale uses first-host (h1), NOT most-prevalent
    assert_eq!(result.locale, Some("en_US.UTF-8".into()));
}

#[test]
fn test_merge_kernelboot_scalar_tie_break() {
    // 2 hosts with different cmdlines — tie broken by first-seen
    let s1 = KernelBootSection {
        cmdline: "root=/dev/sda1 quiet".into(),
        ..Default::default()
    };
    let s2 = KernelBootSection {
        cmdline: "root=/dev/sda1 console=ttyS0".into(),
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let result = merge_kernelboot_sections(vec![Some(s1), Some(s2)], 2, &hostnames).unwrap();

    // Tie: first-seen wins (h1's value)
    assert_eq!(result.cmdline, "root=/dev/sda1 quiet");
}

// ===========================================================================
// Section adapter: NonRpm
// ===========================================================================

#[test]
fn test_merge_nonrpm_sections_all_none() {
    let result = merge_nonrpm_sections(vec![None], 1, &["h1".into()]);
    assert!(result.is_none());
}

#[test]
fn test_merge_nonrpm_sections_items_and_env_files() {
    let s1 = NonRpmSoftwareSection {
        items: vec![NonRpmItem {
            name: "myapp".into(),
            path: "/opt/myapp".into(),
            ..Default::default()
        }],
        env_files: vec![ConfigFileEntry {
            path: "/etc/sysconfig/myapp".into(),
            content: "FOO=bar".into(),
            ..Default::default()
        }],
    };
    let s2 = NonRpmSoftwareSection {
        items: vec![NonRpmItem {
            name: "myapp".into(),
            path: "/opt/myapp".into(),
            ..Default::default()
        }],
        env_files: vec![ConfigFileEntry {
            path: "/etc/sysconfig/myapp".into(),
            content: "FOO=baz".into(), // different content = different variant
            ..Default::default()
        }],
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let result = merge_nonrpm_sections(vec![Some(s1), Some(s2)], 2, &hostnames).unwrap();

    // Items merged by name
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].fleet.as_ref().unwrap().count, 2);

    // env_files: same path, different content = 2 variants
    assert_eq!(result.env_files.len(), 2);
}

// ===========================================================================
// Section adapter: UsersGroups
// ===========================================================================

#[test]
fn test_merge_usersgroups_sections_all_none() {
    let result = merge_usersgroups_sections(vec![None], 1, &["h1".into()]);
    assert!(result.is_none());
}

#[test]
fn test_merge_usersgroups_sections_dedup_by_name() {
    let s1 = UserGroupSection {
        users: vec![serde_json::json!({
            "name": "alice",
            "uid": 1000,
            "groups": ["wheel"]
        })],
        groups: vec![serde_json::json!({
            "name": "devops",
            "gid": 2000,
            "members": ["alice"]
        })],
        sudoers_rules: vec!["alice ALL=(ALL) NOPASSWD:ALL".into()],
        passwd_entries: vec!["alice:x:1000:1000::/home/alice:/bin/bash".into()],
        ..Default::default()
    };
    let s2 = UserGroupSection {
        users: vec![
            serde_json::json!({
                "name": "alice",
                "uid": 1000,
                "groups": ["docker"]
            }),
            serde_json::json!({
                "name": "bob",
                "uid": 1001,
                "groups": ["wheel"]
            }),
        ],
        groups: vec![serde_json::json!({
            "name": "devops",
            "gid": 2000,
            "members": ["alice", "bob"]
        })],
        sudoers_rules: vec![
            "alice ALL=(ALL) NOPASSWD:ALL".into(),
            "bob ALL=(ALL) NOPASSWD:ALL".into(),
        ],
        passwd_entries: vec![
            "alice:x:1000:1000::/home/alice:/bin/bash".into(),
            "bob:x:1001:1001::/home/bob:/bin/bash".into(),
        ],
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let result = merge_usersgroups_sections(vec![Some(s1), Some(s2)], 2, &hostnames).unwrap();

    // Users deduped by name: alice (merged groups), bob
    assert_eq!(result.users.len(), 2);

    // alice's groups should be union: ["docker", "wheel"]
    let alice = result.users.iter().find(|u| u["name"] == "alice").unwrap();
    let groups = alice["groups"].as_array().unwrap();
    assert!(groups.contains(&serde_json::json!("wheel")));
    assert!(groups.contains(&serde_json::json!("docker")));

    // Groups deduped: devops with merged members
    assert_eq!(result.groups.len(), 1);
    let devops = &result.groups[0];
    let members = devops["members"].as_array().unwrap();
    assert!(members.contains(&serde_json::json!("alice")));
    assert!(members.contains(&serde_json::json!("bob")));

    // String lists deduped
    assert_eq!(result.sudoers_rules.len(), 2);
    assert_eq!(result.passwd_entries.len(), 2);
}

// ===========================================================================
// Regression: baseline fields sourced from winning baseline host, not first
// ===========================================================================

#[test]
fn test_merge_rpm_sections_baseline_from_winning_host_not_first() {
    // host-a (index 0) has a DIFFERENT target_image than host-b/host-c.
    // The winning baseline should come from host-b (index 1), not host-a.
    let s_a = RpmSection {
        base_image: Some("quay.io/rhel:9.3".into()),
        baseline_package_names: Some(vec!["old-pkg".into()]),
        no_baseline: false,
        baseline_suppressed: Some(vec!["not-suppressed-a".into()]),
        ..Default::default()
    };
    let s_b = RpmSection {
        base_image: Some("quay.io/rhel:9.4".into()),
        baseline_package_names: Some(vec!["correct-pkg".into()]),
        no_baseline: true,
        baseline_suppressed: Some(vec!["suppressed-b".into()]),
        ..Default::default()
    };
    let s_c = RpmSection {
        base_image: Some("quay.io/rhel:9.4".into()),
        baseline_package_names: Some(vec!["correct-pkg".into()]),
        no_baseline: true,
        baseline_suppressed: Some(vec!["suppressed-b".into()]),
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["host-a".into(), "host-b".into(), "host-c".into()];

    // Baseline host is index 1 (host-b), NOT index 0 (host-a)
    let (result, _) = merge_rpm_sections(
        vec![Some(s_a), Some(s_b), Some(s_c)],
        3,
        &hostnames,
        Some(1),
    )
    .unwrap();

    // RPM section must use host-b's baseline data, not host-a's
    assert_eq!(result.base_image, Some("quay.io/rhel:9.4".into()));
    assert_eq!(
        result.baseline_package_names,
        Some(vec!["correct-pkg".into()])
    );
    assert!(result.no_baseline);
    assert_eq!(
        result.baseline_suppressed,
        Some(vec!["suppressed-b".into()])
    );
}

#[test]
fn test_merge_rpm_sections_no_baseline_gives_defaults() {
    // When no baseline is selected, baseline-bearing fields should be defaults
    let s1 = RpmSection {
        base_image: Some("quay.io/rhel:9.4".into()),
        baseline_package_names: Some(vec!["some-pkg".into()]),
        no_baseline: true,
        baseline_suppressed: Some(vec!["suppressed".into()]),
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["host-a".into()];

    // baseline_host_idx = None means no baseline was selected
    let (result, _) = merge_rpm_sections(vec![Some(s1)], 1, &hostnames, None).unwrap();

    assert_eq!(result.base_image, None);
    assert_eq!(result.baseline_package_names, None);
    assert!(!result.no_baseline);
    assert_eq!(result.baseline_suppressed, None);
}

// ===========================================================================
// Regression: non-variant merge_items picks most-prevalent payload, not first
// ===========================================================================

#[test]
fn test_merge_items_representative_is_most_prevalent_payload() {
    // httpd.x86_64 on 3 hosts: host-a has version "1.0", host-b and host-c
    // have version "2.0". The merged representative must carry "2.0" (majority),
    // not "1.0" (first by hostname sort order).
    let items: Vec<(usize, PackageEntry)> = vec![
        (
            0,
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                version: "1.0".into(),
                ..Default::default()
            },
        ),
        (
            1,
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                version: "2.0".into(),
                ..Default::default()
            },
        ),
        (
            2,
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                version: "2.0".into(),
                ..Default::default()
            },
        ),
    ];
    let hostnames: Vec<String> = vec!["host-a".into(), "host-b".into(), "host-c".into()];
    let merged = merge_items(items, 3, &hostnames);
    assert_eq!(merged.len(), 1);
    // Majority version wins — NOT first host
    assert_eq!(merged[0].version, "2.0");
    // All 3 hosts contributed this identity key
    let fleet = merged[0].fleet.as_ref().unwrap();
    assert_eq!(fleet.count, 3);
    assert_eq!(fleet.total, 3);
    assert_eq!(fleet.hosts, vec!["host-a", "host-b", "host-c"]);
    assert!(merged[0].include);
}

#[test]
fn test_merge_items_representative_tie_break_first_seen() {
    // httpd.x86_64 on 4 hosts: 2 have version "1.0", 2 have version "2.0".
    // Tie-break: first-seen by sorted hostname order wins. host-a (index 0)
    // has "1.0", so "1.0" should win the tie.
    let items: Vec<(usize, PackageEntry)> = vec![
        (
            0,
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                version: "1.0".into(),
                ..Default::default()
            },
        ),
        (
            1,
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                version: "2.0".into(),
                ..Default::default()
            },
        ),
        (
            2,
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                version: "1.0".into(),
                ..Default::default()
            },
        ),
        (
            3,
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                version: "2.0".into(),
                ..Default::default()
            },
        ),
    ];
    let hostnames: Vec<String> = vec![
        "host-a".into(),
        "host-b".into(),
        "host-c".into(),
        "host-d".into(),
    ];
    let merged = merge_items(items, 4, &hostnames);
    assert_eq!(merged.len(), 1);
    // Tie: 2x "1.0" vs 2x "2.0". First-seen (host-a, index 0) has "1.0".
    assert_eq!(merged[0].version, "1.0");
    let fleet = merged[0].fleet.as_ref().unwrap();
    assert_eq!(fleet.count, 4);
    assert_eq!(fleet.total, 4);
}
