use inspectah_core::aggregate::merge::{
    AggregateMergeable, dedup_json_values, dedup_strings, merge_config_sections,
    merge_container_sections, merge_items, merge_kernelboot_sections, merge_network_sections,
    merge_nonrpm_sections, merge_rpm_sections, merge_scheduled_sections, merge_selinux_sections,
    merge_service_sections, merge_storage_sections, merge_usersgroups_sections,
};
use inspectah_core::aggregate::merge_snapshots;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::aggregate::VariantSelection;
use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
use inspectah_core::types::containers::{ComposeFile, ContainerSection, FlatpakApp, QuadletUnit};
use inspectah_core::types::kernelboot::{AlternativeEntry, KernelBootSection, KernelModule};
use inspectah_core::types::network::{FirewallZone, NMConnection, NetworkSection, ProxyEntry};
use inspectah_core::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::rpm::{
    EnabledModuleStream, PackageEntry, RepoFile, RpmSection, VersionChange, VersionLockEntry,
};
use inspectah_core::types::scheduled::{
    AtJob, CronJob, GeneratedTimerUnit, ScheduledTaskSection, SystemdTimer,
};
use inspectah_core::types::selinux::{
    FcontextFileType, FcontextRule, FcontextSource, SelinuxContext, SelinuxPortLabel,
    SelinuxSection,
};
use inspectah_core::types::storage::{FstabEntry, MountPoint, StorageSection};
use inspectah_core::types::units::{
    PresetDefault, SystemdDropIn, UnitInventoryEntry, UnitSection, UnitState, UnitStateChange,
};
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
fn test_package_entry_aggregate_mut() {
    let mut pkg = PackageEntry::default();
    assert!(pkg.aggregate_mut().is_none());
}

#[test]
fn test_package_entry_set_include() {
    let mut pkg = PackageEntry::default();
    assert!(pkg.disposition.is_included());
    pkg.set_include(false);
    assert!(!pkg.disposition.is_included());
    pkg.set_include(true);
    assert!(pkg.disposition.is_included());
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
fn test_nm_connection_set_include_preserves_inventory() {
    // NMConnection defaults to Inventory — set_include must preserve
    // that semantic rather than collapsing to Actionable.
    let mut n = NMConnection::default();
    assert!(n.disposition.is_inventory());
    n.set_include(true);
    assert!(
        n.disposition.is_inventory(),
        "Inventory must not be coerced to Actionable by set_include"
    );
    n.set_include(false);
    assert!(
        n.disposition.is_inventory(),
        "Inventory must not be coerced to Actionable by set_include"
    );
}

#[test]
fn test_nm_connection_actionable_set_include() {
    // An NMConnection with explicit Actionable disposition should
    // still have its include flag toggled normally.
    let mut n = NMConnection {
        disposition: FindingKind::included(),
        ..Default::default()
    };
    assert!(n.disposition.is_included());
    n.set_include(false);
    assert!(!n.disposition.is_included());
    n.set_include(true);
    assert!(n.disposition.is_included());
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
    let agg = merged[0].aggregate.as_ref().unwrap();
    assert_eq!(agg.count, 2);
    assert_eq!(agg.total, 2);
    assert_eq!(agg.hosts, vec!["host-a", "host-b"]);
    assert!(merged[0].disposition.is_included());
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
    assert_eq!(merged[0].aggregate.as_ref().unwrap().count, 1);
    assert_eq!(merged[1].aggregate.as_ref().unwrap().count, 1);
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
    let agg = merged[0].aggregate.as_ref().unwrap();
    assert_eq!(agg.count, 1);
    assert_eq!(agg.total, 1);
    assert_eq!(agg.hosts, vec!["solo-host"]);
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
    let agg = merged[0].aggregate.as_ref().unwrap();
    // Count should be 1 (one unique host), not 2
    assert_eq!(agg.count, 1);
    assert_eq!(agg.hosts, vec!["host-a"]);
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
    let agg = merged[0].aggregate.as_ref().unwrap();
    // Hosts sorted alphabetically
    assert_eq!(agg.hosts, vec!["alpha", "bravo", "charlie"]);
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
    assert_eq!(selected.aggregate.as_ref().unwrap().count, 2);
    assert_eq!(alt.content, "version_b");
    assert_eq!(alt.aggregate.as_ref().unwrap().count, 1);
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
    assert_eq!(selected.aggregate.as_ref().unwrap().count, 3);

    let alternatives: Vec<&ConfigFileEntry> = merged
        .iter()
        .filter(|e| e.variant_selection == VariantSelection::Alternative)
        .collect();
    assert_eq!(alternatives.len(), 2);
    for alt in &alternatives {
        assert_eq!(alt.aggregate.as_ref().unwrap().count, 1);
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
    // With aggregate narrowing, per-variant prevalence determines include.
    // Each variant appears on 1/2 hosts (non-universal), so include=false.
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
        assert!(
            !item.disposition.is_included(),
            "non-universal variant (1/2 hosts) must have include=false"
        );
    }
}

#[test]
fn test_merge_items_variant_total_reflects_aggregate_size() {
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
    // total should reflect the aggregate size (5), not just the hosts that have this item
    for item in &merged {
        assert_eq!(item.aggregate.as_ref().unwrap().total, 5);
    }
}

// ===========================================================================
// New AggregateMergeable impls (Task 9)
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

/// A timer is a description with no include flag: the merge's include
/// step leaves it as it was.
#[test]
fn test_systemd_timer_set_include_changes_nothing() {
    let before = SystemdTimer {
        name: "backup.timer".into(),
        on_calendar: "daily".into(),
        ..Default::default()
    };
    let mut t = before.clone();
    t.set_include(false);
    assert_eq!(t, before);
}

#[test]
fn test_fstab_entry_set_include() {
    let mut f = FstabEntry::default();
    f.set_include(false);
    assert!(!f.disposition.is_included());
    f.set_include(true);
    assert!(f.disposition.is_included());
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
    assert_eq!(httpd.aggregate.as_ref().unwrap().count, 2);
    let nginx = result
        .packages_added
        .iter()
        .find(|p| p.name == "nginx")
        .unwrap();
    assert_eq!(nginx.aggregate.as_ref().unwrap().count, 1);
}

/// A key named with a dnf variable keeps that reference through the
/// merge: an EL9 and an EL10 host expand one literal to two keys, and both
/// answer it; a third host naming the EL10 key literally does not drop it.
#[test]
fn test_merge_rpm_sections_keeps_gpgkey_references() {
    const REFERENCE: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-$releasever_major";
    let key = |path: &str, references: &[&str]| RepoFile {
        path: path.into(),
        content: format!("-----BEGIN PGP PUBLIC KEY BLOCK-----\n{path}\n"),
        gpgkey_references: references.iter().map(|r| (*r).to_string()).collect(),
        ..Default::default()
    };
    let host = |k: RepoFile| RpmSection {
        gpg_keys: vec![k],
        ..Default::default()
    };
    let el9 = "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9";
    let el10 = "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-10";
    let hostnames: Vec<String> = vec!["el10-literal".into(), "el10".into(), "el9".into()];
    let (result, _) = merge_rpm_sections(
        vec![
            Some(host(key(el10, &[]))),
            Some(host(key(el10, &[REFERENCE]))),
            Some(host(key(el9, &[REFERENCE]))),
        ],
        3,
        &hostnames,
        None,
    )
    .unwrap();

    let named: Vec<&str> = result
        .gpg_keys
        .iter()
        .filter(|k| k.is_key_named_by(REFERENCE))
        .map(|k| k.path.as_str())
        .collect();
    assert_eq!(named, [el10, el9]);
    let el10_key = result.gpg_keys.iter().find(|k| k.path == el10).unwrap();
    assert_eq!(
        el10_key.aggregate.as_ref().map(|a| a.count),
        Some(2),
        "the reference does not split the key's prevalence"
    );
}

/// A key is left to the base image in an aggregate only when every host
/// that has it left it there. One host that carries it (modified, or its
/// owner not in that host's base) makes the fleet carry it.
#[test]
fn test_merge_rpm_sections_leaves_a_key_to_the_base_only_when_every_host_does() {
    const KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-redhat-release";
    let host = |shipped: bool| RpmSection {
        gpg_keys: vec![RepoFile {
            path: KEY.into(),
            content: "-----BEGIN PGP PUBLIC KEY BLOCK-----\nrh\n".into(),
            disposition: FindingKind::from_bool(!shipped),
            shipped_by_base: shipped.then(|| "redhat-release".to_string()),
            ..Default::default()
        }],
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["a".into(), "b".into()];
    let cases = [
        (
            "both hosts leave it",
            [true, true],
            Some("redhat-release"),
            false,
        ),
        ("one host carries it", [true, false], None, true),
        ("both hosts carry it", [false, false], None, true),
    ];
    for (name, shipped, expect_shipped, expect_included) in cases {
        let (result, _) = merge_rpm_sections(
            shipped.iter().map(|&s| Some(host(s))).collect(),
            2,
            &hostnames,
            None,
        )
        .unwrap();
        assert_eq!(result.gpg_keys.len(), 1, "{name}");
        let key = &result.gpg_keys[0];
        assert_eq!(key.shipped_by_base.as_deref(), expect_shipped, "{name}");
        assert_eq!(key.disposition.is_included(), expect_included, "{name}");
    }
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
        base_image: Some("registry.example.com/image:latest".into()),
        leaf_packages: Some(vec!["httpd.x86_64".into()]),
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["h1".into()];
    let (result, _) = merge_rpm_sections(vec![Some(s1)], 1, &hostnames, Some(0)).unwrap();

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
    let make_sc = |unit: &str| {
        let mut change = UnitStateChange::new(
            test_carried_unit(unit.to_owned().try_into().unwrap(), None),
            UnitState::Enabled,
            None,
        )
        .unwrap();
        change.disposition = FindingKind::excluded();
        change
    };
    let s1 = UnitSection {
        state_changes: vec![make_sc("httpd.service")],
        inventory: vec![
            inv("sshd.service", UnitState::Enabled, None),
            inv("httpd.service", UnitState::Enabled, None),
            inv("firewalld.service", UnitState::Disabled, None),
        ],
        drop_ins: vec![],
        mount_findings: Vec::new(),
        manual_links: Vec::new(),
        undetermined_installs: Vec::new(),
        default_target: None,
        enabled_on_any_host: Vec::new(),
    };
    let s2 = UnitSection {
        state_changes: vec![make_sc("httpd.service")],
        inventory: vec![
            inv("httpd.service", UnitState::Enabled, None),
            inv("crond.service", UnitState::Enabled, None),
        ],
        drop_ins: vec![],
        mount_findings: Vec::new(),
        manual_links: Vec::new(),
        undetermined_installs: Vec::new(),
        default_target: None,
        enabled_on_any_host: Vec::new(),
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let result = merge_service_sections(vec![Some(s1), Some(s2)], 2, &hostnames).unwrap();

    // state_changes merged: httpd present on both hosts
    assert_eq!(result.state_changes.len(), 1);
    assert_eq!(result.state_changes[0].aggregate.as_ref().unwrap().count, 2);

    // Inventory: one entry per unit, sorted by name
    assert_eq!(
        inventory_summary(&result),
        vec![
            ("crond.service", UnitState::Enabled, None),
            ("firewalld.service", UnitState::Disabled, None),
            ("httpd.service", UnitState::Enabled, None),
            ("sshd.service", UnitState::Enabled, None),
        ]
    );
}

fn inv(unit: &str, state: UnitState, preset: Option<PresetDefault>) -> UnitInventoryEntry {
    UnitInventoryEntry {
        unit: unit.to_owned().try_into().unwrap(),
        state,
        preset,
    }
}

fn inventory_section(inventory: Vec<UnitInventoryEntry>) -> Option<UnitSection> {
    Some(UnitSection {
        inventory,
        ..Default::default()
    })
}

fn inventory_summary(section: &UnitSection) -> Vec<(&str, UnitState, Option<PresetDefault>)> {
    section
        .inventory
        .iter()
        .map(|e| (e.unit.as_str(), e.state, e.preset))
        .collect()
}

/// The inventory keeps the state most hosts have, and a unit that only
/// one host has is kept.
#[test]
fn test_merge_inventory_keeps_most_prevalent_state() {
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into(), "h3".into()];
    let sections = vec![
        inventory_section(vec![
            inv(
                "sshd.service",
                UnitState::Enabled,
                Some(PresetDefault::Enable),
            ),
            inv("cups.service", UnitState::Disabled, None),
        ]),
        inventory_section(vec![inv(
            "sshd.service",
            UnitState::Enabled,
            Some(PresetDefault::Enable),
        )]),
        inventory_section(vec![inv(
            "sshd.service",
            UnitState::Disabled,
            Some(PresetDefault::Enable),
        )]),
    ];
    let result = merge_service_sections(sections, 3, &hostnames).unwrap();

    assert_eq!(
        inventory_summary(&result),
        vec![
            ("cups.service", UnitState::Disabled, None),
            (
                "sshd.service",
                UnitState::Enabled,
                Some(PresetDefault::Enable)
            ),
        ]
    );
}

/// A tie between states goes to the first host in hostname order, as
/// the other most-prevalent merges break ties, and the preset comes from
/// the first host that has the winning state.
#[test]
fn test_merge_inventory_tie_goes_to_first_host() {
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into(), "h3".into(), "h4".into()];
    let sections = vec![
        inventory_section(vec![inv("a.service", UnitState::Disabled, None)]),
        inventory_section(vec![inv(
            "a.service",
            UnitState::Enabled,
            Some(PresetDefault::Disable),
        )]),
        inventory_section(vec![inv(
            "a.service",
            UnitState::Disabled,
            Some(PresetDefault::Enable),
        )]),
        inventory_section(vec![inv("a.service", UnitState::Enabled, None)]),
    ];
    let result = merge_service_sections(sections, 4, &hostnames).unwrap();

    assert_eq!(
        inventory_summary(&result),
        vec![("a.service", UnitState::Disabled, None)]
    );
}

/// The preset comes from the first host with the winning state, not from
/// the first host overall: h1's state loses, so its missing preset is not
/// used, and of the two hosts with the winning state h2 comes first. The
/// winning state is not the first entry's, so keeping the first entry
/// would fail here.
#[test]
fn test_merge_inventory_preset_from_first_host_with_winning_state() {
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into(), "h3".into()];
    let sections = vec![
        inventory_section(vec![inv("a.service", UnitState::Disabled, None)]),
        inventory_section(vec![inv(
            "a.service",
            UnitState::Enabled,
            Some(PresetDefault::Disable),
        )]),
        inventory_section(vec![inv(
            "a.service",
            UnitState::Enabled,
            Some(PresetDefault::Enable),
        )]),
    ];
    let result = merge_service_sections(sections, 3, &hostnames).unwrap();

    assert_eq!(
        inventory_summary(&result),
        vec![(
            "a.service",
            UnitState::Enabled,
            Some(PresetDefault::Disable)
        )]
    );
}

/// A host snapshot whose only unit is `a.service` in `state`.
fn inventory_host(
    hostname: &str,
    state: UnitState,
    preset: Option<PresetDefault>,
) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.meta.insert(
        "hostname".to_string(),
        serde_json::Value::String(hostname.into()),
    );
    snap.os_release = Some(OsRelease {
        version_id: "9.4".into(),
        ..Default::default()
    });
    snap.rpm = Some(RpmSection::default());
    snap.units = inventory_section(vec![inv("a.service", state, preset)]);
    snap
}

/// Through `merge_snapshots()`, a tie goes to the first host by hostname
/// whatever order the snapshots are given in: `alpha` wins whether it is
/// passed first or second. Giving `alpha` the other state flips the
/// result, so the winner is the host, not a fixed state.
#[test]
fn test_merge_inventory_tie_follows_hostname_not_argument_order() {
    let pairs = [
        (
            (UnitState::Disabled, Some(PresetDefault::Enable)),
            (UnitState::Enabled, Some(PresetDefault::Disable)),
        ),
        (
            (UnitState::Enabled, Some(PresetDefault::Disable)),
            (UnitState::Disabled, Some(PresetDefault::Enable)),
        ),
    ];
    for ((alpha_state, alpha_preset), (bravo_state, bravo_preset)) in pairs {
        for alpha_first in [false, true] {
            let alpha = inventory_host("alpha", alpha_state, alpha_preset);
            let bravo = inventory_host("bravo", bravo_state, bravo_preset);
            let snapshots = if alpha_first {
                vec![alpha, bravo]
            } else {
                vec![bravo, alpha]
            };
            let (merged, _) = merge_snapshots(snapshots, None, None).unwrap();
            let units = merged.units.expect("the merge keeps the units section");
            assert_eq!(
                inventory_summary(&units),
                vec![("a.service", alpha_state, alpha_preset)],
                "alpha passed first: {alpha_first}"
            );
        }
    }
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

    // Calculator present on both hosts, Firefox on one
    let calc = result
        .flatpak_apps
        .iter()
        .find(|a| a.app_id == "org.gnome.Calculator")
        .expect("Calculator should be in merged output");
    let calc_agg = calc.aggregate.as_ref().expect("should have aggregate data");
    assert_eq!(calc_agg.count, 2);
    assert_eq!(calc_agg.total, 2);

    let firefox = result
        .flatpak_apps
        .iter()
        .find(|a| a.app_id == "org.mozilla.Firefox")
        .expect("Firefox should be in merged output");
    let ff_agg = firefox
        .aggregate
        .as_ref()
        .expect("should have aggregate data");
    assert_eq!(ff_agg.count, 1);
    assert_eq!(ff_agg.total, 2);
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
    assert_eq!(result.fstab_entries[0].aggregate.as_ref().unwrap().count, 2);

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
    assert_eq!(result.cron_jobs[0].aggregate.as_ref().unwrap().count, 2);
    assert_eq!(result.systemd_timers.len(), 2);
    assert_eq!(result.at_jobs.len(), 1);
    assert_eq!(result.generated_timer_units.len(), 1);
}

// ===========================================================================
// Section adapter: SELinux
// ===========================================================================

/// An included `Label` rule for a spec, with a fixed context -- content
/// doesn't matter for the dedup-by-key tests below.
fn fcontext_label(spec: &str) -> FcontextRule {
    FcontextRule::Label {
        spec: spec.into(),
        file_type: FcontextFileType::All,
        context: SelinuxContext {
            user: "system_u".into(),
            role: "object_r".into(),
            type_: "httpd_sys_content_t".into(),
            level: Some("s0".into()),
        },
        disposition: FindingKind::included(),
        aggregate: None,
    }
}

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
        fcontext_rules: vec![fcontext_label("/opt/app(/.*)?")],
        fcontext_source: FcontextSource::PolicyStore,
        boolean_overrides: vec![inspectah_core::types::selinux::SelinuxBoolean {
            name: "httpd_can_network_connect".into(),
            value: true,
        }],
        fips_mode: false,
        ..Default::default()
    };
    let s2 = SelinuxSection {
        mode: "enforcing".into(),
        port_labels: vec![SelinuxPortLabel {
            protocol: "tcp".into(),
            port: "8080".into(),
            ..Default::default()
        }],
        custom_modules: vec!["mymodule".into(), "othermodule".into()],
        fcontext_rules: vec![fcontext_label("/opt/app(/.*)?")],
        fcontext_source: FcontextSource::PolicyStore,
        boolean_overrides: vec![inspectah_core::types::selinux::SelinuxBoolean {
            name: "httpd_can_network_connect".into(),
            value: true,
        }],
        fips_mode: false,
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let result = merge_selinux_sections(vec![Some(s1), Some(s2)], 2, &hostnames).unwrap();

    // Port labels merged via merge_items
    assert_eq!(result.port_labels.len(), 1);
    assert_eq!(result.port_labels[0].aggregate.as_ref().unwrap().count, 2);

    // String lists deduped
    assert_eq!(result.custom_modules, vec!["mymodule", "othermodule"]);
    // Fcontext rules deduped by semantic key (same spec + file_type); both
    // hosts agree on content, so one rule survives, included (count ==
    // total), naming both hosts.
    assert_eq!(result.fcontext_rules.len(), 1);
    match &result.fcontext_rules[0] {
        FcontextRule::Label {
            spec,
            disposition,
            aggregate,
            ..
        } => {
            assert_eq!(spec, "/opt/app(/.*)?");
            assert!(disposition.is_included());
            let agg = aggregate.as_ref().unwrap();
            assert_eq!(agg.count, 2);
            assert_eq!(agg.hosts, vec!["h1".to_string(), "h2".to_string()]);
            // No disagreement -- aggregate_count/aggregate_hosts stay
            // unset, per their own doc comment.
            assert_eq!(agg.aggregate_count, None);
            assert_eq!(agg.aggregate_hosts, None);
        }
        other => panic!("expected Label, got {other:?}"),
    }
    assert_eq!(result.fcontext_source, FcontextSource::PolicyStore);

    // Equal booleans deduped
    assert_eq!(result.boolean_overrides.len(), 1);

    // Most-prevalent scalar (both hosts agree here)
    assert_eq!(result.mode, "enforcing");
}

/// A boolean the hosts set to different values is a conflict naming the
/// hosts on each side, not two agreed entries; one they agree on stays
/// agreed, and one only some hosts set is still carried. A host that
/// names a boolean twice holds the value it names last.
#[test]
fn test_merge_selinux_boolean_conflicts() {
    use inspectah_core::types::selinux::{SelinuxBoolean, SelinuxBooleanConflict};
    let section = |booleans: &[(&str, bool)]| SelinuxSection {
        boolean_overrides: booleans
            .iter()
            .map(|(name, value)| SelinuxBoolean {
                name: (*name).into(),
                value: *value,
            })
            .collect(),
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into(), "h3".into()];
    let result = merge_selinux_sections(
        vec![
            Some(section(&[
                ("httpd_can_network_connect", true),
                ("virt_use_nfs", true),
            ])),
            Some(section(&[
                ("virt_use_nfs", true),
                ("httpd_can_network_connect", true),
                ("httpd_can_network_connect", false),
            ])),
            Some(section(&[
                ("ftpd_full_access", false),
                ("httpd_can_network_connect", false),
            ])),
        ],
        3,
        &hostnames,
    )
    .unwrap();
    assert_eq!(
        result.boolean_overrides,
        [
            SelinuxBoolean {
                name: "virt_use_nfs".into(),
                value: true,
            },
            SelinuxBoolean {
                name: "ftpd_full_access".into(),
                value: false,
            },
        ]
    );
    assert_eq!(
        result.boolean_conflicts,
        [SelinuxBooleanConflict {
            name: "httpd_can_network_connect".into(),
            on_hosts: vec!["h1".into()],
            off_hosts: vec!["h2".into(), "h3".into()],
        }]
    );
}

/// Disabled modules are unioned by name; a name local on any host is local.
#[test]
fn test_merge_selinux_disabled_modules() {
    use inspectah_core::types::selinux::{DisabledModule, ModuleOrigin};
    let disabled = |modules: &[(&str, ModuleOrigin)]| SelinuxSection {
        disabled_modules: modules
            .iter()
            .map(|(name, origin)| DisabledModule {
                name: (*name).into(),
                origin: *origin,
            })
            .collect(),
        ..Default::default()
    };
    let result = merge_selinux_sections(
        vec![
            Some(disabled(&[
                ("myapp", ModuleOrigin::Packaged),
                ("zebra", ModuleOrigin::Packaged),
            ])),
            Some(disabled(&[
                ("apache", ModuleOrigin::Packaged),
                ("myapp", ModuleOrigin::Local),
            ])),
        ],
        2,
        &["h1".into(), "h2".into()],
    )
    .unwrap();
    assert_eq!(
        result.disabled_modules,
        [
            DisabledModule {
                name: "apache".into(),
                origin: ModuleOrigin::Packaged,
            },
            DisabledModule {
                name: "myapp".into(),
                origin: ModuleOrigin::Local,
            },
            DisabledModule {
                name: "zebra".into(),
                origin: ModuleOrigin::Packaged,
            },
        ]
    );
}

#[test]
fn test_merge_selinux_fcontext_order_and_disposition() {
    // h1: an h1-only rule, then a rule sharing a key with h2 but
    // disagreeing on content (context "y1_t" vs h2's "y2_t" below).
    let h1_shared = FcontextRule::Label {
        spec: "/opt/shared".into(),
        file_type: FcontextFileType::All,
        context: SelinuxContext {
            user: "system_u".into(),
            role: "object_r".into(),
            type_: "y1_t".into(),
            level: Some("s0".into()),
        },
        disposition: FindingKind::included(),
        aggregate: None,
    };
    let s1 = SelinuxSection {
        fcontext_rules: vec![fcontext_label("/opt/a"), h1_shared],
        ..Default::default()
    };

    // h2: the same key, different content, then an h2-only rule.
    let h2_shared = FcontextRule::Label {
        spec: "/opt/shared".into(),
        file_type: FcontextFileType::All,
        context: SelinuxContext {
            user: "system_u".into(),
            role: "object_r".into(),
            type_: "y2_t".into(),
            level: Some("s0".into()),
        },
        disposition: FindingKind::excluded(),
        aggregate: None,
    };
    let s2 = SelinuxSection {
        fcontext_rules: vec![h2_shared, fcontext_label("/opt/c")],
        ..Default::default()
    };

    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let result = merge_selinux_sections(vec![Some(s1), Some(s2)], 2, &hostnames).unwrap();

    // Order: hostname order, then each host's collected order -- not
    // sorted by spec and not grouped by host. Round 3's ruling: one row
    // survives per key (spec § 3.1), so "shared" is one row, not two --
    // 3 rows total, not 4.
    let specs_and_types: Vec<(&str, &str)> = result
        .fcontext_rules
        .iter()
        .map(|r| match r {
            FcontextRule::Label { spec, context, .. } => (spec.as_str(), context.type_.as_str()),
            other => panic!("expected Label, got {:?}", other),
        })
        .collect();
    assert_eq!(
        specs_and_types,
        vec![
            ("/opt/a", "httpd_sys_content_t"),
            ("/opt/shared", "y1_t"),
            ("/opt/c", "httpd_sys_content_t"),
        ],
        "one row per key, the first content seen in hostname order"
    );

    let included = |rule: &FcontextRule| match rule {
        FcontextRule::Label { disposition, .. } => disposition.is_included(),
        other => panic!("expected Label, got {:?}", other),
    };

    // "/opt/a" and "/opt/c" each sit on only 1 of 2 hosts -- narrowing
    // (count == total across the whole run) excludes these, where round
    // 1's per-host-disposition rule included them (the only host carrying
    // each had included it). This is the behaviour round 2 changed;
    // assert it directly rather than letting it ride on an existing
    // assertion.
    assert!(
        !included(&result.fcontext_rules[0]),
        "/opt/a is on 1 of 2 hosts, must be excluded"
    );
    assert!(
        !included(&result.fcontext_rules[2]),
        "/opt/c is on 1 of 2 hosts, must be excluded"
    );

    // "/opt/shared" the key is universal, but the hosts disagree on its
    // content -- narrowing is computed on content, matching the engine, so
    // the surviving row is excluded even though every host carries the key.
    // An included row would render as an active `semanage fcontext -a`,
    // silently imposing h1's content on h2.
    assert!(
        !included(&result.fcontext_rules[1]),
        "shared key's content disagrees across hosts, must be excluded"
    );

    // The surviving row is h1's (first in hostname order); its count/hosts
    // describe only h1, who shares that exact content. The disagreement
    // itself is not lost: aggregate_count/aggregate_hosts (set only
    // because this key has more than one variant) name every host
    // carrying the key under any content, which is what a future audit
    // report needs to name both sides by host.
    match &result.fcontext_rules[1] {
        FcontextRule::Label {
            context, aggregate, ..
        } => {
            assert_eq!(context.type_, "y1_t", "h1's content survives, not h2's");
            let agg = aggregate.as_ref().expect("aggregate must be populated");
            assert_eq!(agg.count, 1);
            assert_eq!(agg.total, 2);
            assert_eq!(agg.hosts, vec!["h1".to_string()]);
            assert_eq!(agg.aggregate_count, Some(2));
            assert_eq!(
                agg.aggregate_hosts,
                Some(vec!["h1".to_string(), "h2".to_string()])
            );
        }
        other => panic!("expected Label, got {:?}", other),
    }

    // "/opt/a" and "/opt/c" have no disagreement -- aggregate_count/
    // aggregate_hosts stay None, per their own doc comment ("only set for
    // items with content variants").
    for idx in [0, 2] {
        match &result.fcontext_rules[idx] {
            FcontextRule::Label { aggregate, .. } => {
                let agg = aggregate.as_ref().expect("aggregate must be populated");
                assert_eq!(agg.aggregate_count, None);
                assert_eq!(agg.aggregate_hosts, None);
            }
            other => panic!("expected Label, got {:?}", other),
        }
    }
}

#[test]
fn test_merge_selinux_fcontext_label_and_unlabel_share_a_key() {
    // h1 labels a spec; h2 unlabels the *same* spec/file_type. These are
    // the same local-database entry (round 1 review, item 3) -- a replay
    // that ran both in sequence would have the second silently overwrite
    // the first, so the merge must treat them as one key, not two rows
    // that happen to share a spec.
    let s1 = SelinuxSection {
        fcontext_rules: vec![fcontext_label("/opt/x")],
        ..Default::default()
    };
    let s2 = SelinuxSection {
        fcontext_rules: vec![FcontextRule::Unlabel {
            spec: "/opt/x".into(),
            file_type: FcontextFileType::All,
            disposition: FindingKind::included(),
            aggregate: None,
        }],
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let result = merge_selinux_sections(vec![Some(s1), Some(s2)], 2, &hostnames).unwrap();

    // One key, one surviving row (round 3 ruling, spec § 3.1: "hosts that
    // disagree on the context for one key keep the first in hostname
    // order"). Round 2 had this produce 2 rows; that would let both
    // render and, under § 3.1 rule 1's replay, the second silently
    // overwrites the first in the built image -- the exact failure the
    // shared-key merge exists to prevent, recreated through row count
    // instead of key count.
    assert_eq!(result.fcontext_rules.len(), 1);

    match &result.fcontext_rules[0] {
        FcontextRule::Label {
            spec,
            disposition,
            aggregate,
            ..
        } => {
            assert_eq!(
                spec, "/opt/x",
                "h1's Label survives, first in hostname order"
            );
            // The key itself is universal, but the hosts disagree on
            // content (Label vs Unlabel) -- narrowing is computed on
            // content, matching the engine, so the surviving row is
            // excluded even though every host carries the key.
            assert!(!disposition.is_included());
            let agg = aggregate.as_ref().expect("aggregate must be populated");
            // count/hosts describe only the hosts sharing the surviving
            // row's exact content: h1 alone.
            assert_eq!(agg.count, 1);
            assert_eq!(agg.hosts, vec!["h1".to_string()]);
            // aggregate_count/aggregate_hosts carry the disagreement: both
            // hosts carry the key, across its two variants (Label,
            // Unlabel) -- this is the record a future audit report needs
            // to name both sides by host without adding a row.
            assert_eq!(agg.aggregate_count, Some(2));
            assert_eq!(
                agg.aggregate_hosts,
                Some(vec!["h1".to_string(), "h2".to_string()])
            );
        }
        other => panic!("expected h1's Label to survive, got {other:?}"),
    }
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
        loaded_modules: vec![KernelModule {
            name: "vfio_pci".into(),
            ..Default::default()
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
        alternatives: vec![AlternativeEntry {
            name: "python3".into(),
            path: "/usr/bin/python3.11".into(),
            status: "auto".into(),
        }],
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let result = merge_kernelboot_sections(vec![Some(s1), Some(s2)], 2, &hostnames).unwrap();

    // loaded_modules merged
    assert_eq!(result.loaded_modules.len(), 2);

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
    assert_eq!(result.items[0].aggregate.as_ref().unwrap().count, 2);

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

        baseline_suppressed: Some(vec!["not-suppressed-a".into()]),
        ..Default::default()
    };
    let s_b = RpmSection {
        base_image: Some("quay.io/rhel:9.4".into()),
        baseline_package_names: Some(vec!["correct-pkg".into()]),

        baseline_suppressed: Some(vec!["suppressed-b".into()]),
        ..Default::default()
    };
    let s_c = RpmSection {
        base_image: Some("quay.io/rhel:9.4".into()),
        baseline_package_names: Some(vec!["correct-pkg".into()]),

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

        baseline_suppressed: Some(vec!["suppressed".into()]),
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["host-a".into()];

    // baseline_host_idx = None means no baseline was selected
    let (result, _) = merge_rpm_sections(vec![Some(s1)], 1, &hostnames, None).unwrap();

    assert_eq!(result.base_image, None);
    assert_eq!(result.baseline_package_names, None);
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
    let agg = merged[0].aggregate.as_ref().unwrap();
    assert_eq!(agg.count, 3);
    assert_eq!(agg.total, 3);
    assert_eq!(agg.hosts, vec!["host-a", "host-b", "host-c"]);
    assert!(merged[0].disposition.is_included());
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
    let agg = merged[0].aggregate.as_ref().unwrap();
    assert_eq!(agg.count, 4);
    assert_eq!(agg.total, 4);
}

// ---------------------------------------------------------------------------
// Leaf intersection in merge_rpm_sections
// ---------------------------------------------------------------------------

#[test]
fn test_aggregate_leaf_intersection_filters_packages_added() {
    let host_a = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "git".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "perl-libs".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["git.x86_64".into()]),
        auto_packages: Some(vec!["perl-libs.x86_64".into()]),
        leaf_dep_tree: serde_json::json!({"git.x86_64": ["perl-libs.x86_64"]}),
        ..Default::default()
    });
    let host_b = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "git".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "perl-libs".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["git.x86_64".into()]),
        auto_packages: Some(vec!["perl-libs.x86_64".into()]),
        leaf_dep_tree: serde_json::json!({"git.x86_64": ["perl-libs.x86_64"]}),
        ..Default::default()
    });

    let hostnames = vec!["host-a".into(), "host-b".into()];
    let (merged, _conflicts) =
        merge_rpm_sections(vec![host_a, host_b], 2, &hostnames, None).unwrap();

    // packages_added should contain ONLY the leaf package
    assert_eq!(merged.packages_added.len(), 1);
    assert_eq!(merged.packages_added[0].name, "git");

    // leaf_packages should be the intersection
    assert_eq!(merged.leaf_packages, Some(vec!["git.x86_64".into()]));

    // auto_packages should be None for aggregate
    assert_eq!(merged.auto_packages, None);

    // leaf_dep_tree should only contain entries for intersection packages
    let tree = merged.leaf_dep_tree.as_object().unwrap();
    assert_eq!(tree.len(), 1);
    assert!(tree.contains_key("git.x86_64"));

    // coverage metadata — uses total_hosts param (2), not sections.len()
    assert_eq!(merged.leaf_authority_hosts, Some(2));
    assert_eq!(merged.leaf_total_hosts, Some(2));
}

/// Every package the merge keeps gets its closures from every host with
/// leaf data that has it: `git`, a leaf on both, and `app`, which only
/// `host-a` has and the merge keeps left out. `app`'s closure keeps
/// `helper`, user-installed on `host-a`, which the merge drops for being a
/// dependency on `host-b`. A dropped package gets no entry.
///
/// What a package certainly installs is what it certainly installs on
/// every such host: `git` requires `helper` on `host-b` only, so the fleet
/// image may install it with `git` but need not, and it moves to the
/// alternatives. Those join everything either map records on any host:
/// `host-a`'s `git` may also install `ruby`, one of several providers
/// there.
#[test]
fn test_aggregate_dep_closures_cover_every_kept_package() {
    let pkg = |name: &str| PackageEntry {
        name: name.into(),
        arch: "x86_64".into(),
        disposition: FindingKind::included(),
        ..Default::default()
    };
    let closures =
        |entries: &[(&str, &[&str])]| -> std::collections::BTreeMap<String, Vec<String>> {
            entries
                .iter()
                .map(|(id, deps)| (id.to_string(), deps.iter().map(|d| d.to_string()).collect()))
                .collect()
        };
    let host_a = Some(RpmSection {
        packages_added: vec![pkg("git"), pkg("app"), pkg("helper"), pkg("perl-libs")],
        leaf_packages: Some(vec![
            "app.x86_64".into(),
            "git.x86_64".into(),
            "helper.x86_64".into(),
        ]),
        auto_packages: Some(vec!["perl-libs.x86_64".into()]),
        leaf_dep_tree: serde_json::json!({
            "app.x86_64": [],
            "git.x86_64": ["perl-libs.x86_64"],
            "helper.x86_64": [],
        }),
        dep_closures: Some(closures(&[
            ("app.x86_64", &["helper.x86_64"]),
            ("git.x86_64", &["perl-libs.x86_64"]),
            ("helper.x86_64", &[]),
            ("perl-libs.x86_64", &[]),
        ])),
        dep_alternatives: Some(closures(&[
            ("app.x86_64", &[]),
            ("git.x86_64", &["ruby.x86_64"]),
            ("helper.x86_64", &[]),
            ("perl-libs.x86_64", &[]),
        ])),
        ..Default::default()
    });
    let host_b = Some(RpmSection {
        packages_added: vec![pkg("git"), pkg("helper"), pkg("perl-libs")],
        leaf_packages: Some(vec!["git.x86_64".into()]),
        auto_packages: Some(vec!["helper.x86_64".into(), "perl-libs.x86_64".into()]),
        leaf_dep_tree: serde_json::json!({"git.x86_64": ["helper.x86_64", "perl-libs.x86_64"]}),
        dep_closures: Some(closures(&[
            ("git.x86_64", &["helper.x86_64", "perl-libs.x86_64"]),
            ("helper.x86_64", &[]),
            ("perl-libs.x86_64", &[]),
        ])),
        ..Default::default()
    });

    let hostnames = vec!["host-a".into(), "host-b".into()];
    let (merged, _) = merge_rpm_sections(vec![host_a, host_b], 2, &hostnames, None).unwrap();

    let kept: Vec<&str> = merged
        .packages_added
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(kept, ["app", "git"]);
    assert_eq!(
        merged.dep_closures,
        Some(closures(&[
            ("app.x86_64", &["helper.x86_64"]),
            ("git.x86_64", &["perl-libs.x86_64"]),
        ]))
    );
    assert_eq!(
        merged.dep_alternatives,
        Some(closures(&[
            ("app.x86_64", &[]),
            ("git.x86_64", &["helper.x86_64", "ruby.x86_64"]),
        ]))
    );
}

// ---------------------------------------------------------------------------
// Leaf intersection edge cases
// ---------------------------------------------------------------------------

#[test]
fn test_aggregate_leaf_intersection_excludes_partial_leaf() {
    // host_a: git + htop both leaf
    // host_b: git leaf, htop auto (not in leaf_packages)
    // Result: only git survives intersection, htop filtered from packages_added
    let host_a = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "git".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "htop".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["git.x86_64".into(), "htop.x86_64".into()]),
        auto_packages: Some(vec![]),
        leaf_dep_tree: serde_json::json!({"git.x86_64": [], "htop.x86_64": []}),
        ..Default::default()
    });
    let host_b = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "git".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "htop".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["git.x86_64".into()]),
        auto_packages: Some(vec!["htop.x86_64".into()]),
        leaf_dep_tree: serde_json::json!({"git.x86_64": ["htop.x86_64"]}),
        ..Default::default()
    });

    let hostnames = vec!["host-a".into(), "host-b".into()];
    let (merged, _) = merge_rpm_sections(vec![host_a, host_b], 2, &hostnames, None).unwrap();

    // Only git is leaf on ALL authoritative hosts
    assert_eq!(merged.leaf_packages, Some(vec!["git.x86_64".into()]));

    // packages_added filtered to leaf intersection
    assert_eq!(merged.packages_added.len(), 1);
    assert_eq!(merged.packages_added[0].name, "git");
}

#[test]
fn test_aggregate_leaf_intersection_skips_degraded_hosts() {
    // host_a: vim leaf (authoritative)
    // host_b: vim present but leaf_packages: None (degraded)
    // Result: vim in intersection, leaf_authority_hosts=1, leaf_total_hosts=2
    let host_a = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "vim".into(),
            arch: "x86_64".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        leaf_packages: Some(vec!["vim.x86_64".into()]),
        auto_packages: Some(vec![]),
        leaf_dep_tree: serde_json::json!({"vim.x86_64": []}),
        ..Default::default()
    });
    let host_b = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "vim".into(),
            arch: "x86_64".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        leaf_packages: None,
        auto_packages: None,
        leaf_dep_tree: serde_json::json!({}),
        ..Default::default()
    });

    let hostnames = vec!["host-a".into(), "host-b".into()];
    let (merged, _) = merge_rpm_sections(vec![host_a, host_b], 2, &hostnames, None).unwrap();

    // vim passes — only 1 authoritative host, so intersection = that host's set
    assert_eq!(merged.leaf_packages, Some(vec!["vim.x86_64".into()]));
    assert_eq!(merged.packages_added.len(), 1);
    assert_eq!(merged.packages_added[0].name, "vim");

    // Coverage metadata
    assert_eq!(merged.leaf_authority_hosts, Some(1));
    assert_eq!(merged.leaf_total_hosts, Some(2));
}

#[test]
fn test_aggregate_leaf_intersection_all_degraded() {
    // Both hosts: leaf_packages: None
    // Result: leaf_packages=None, auto_packages=None, leaf_dep_tree={},
    //         packages_added kept (no filtering), authority=0, total=2
    let host_a = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "git".into(),
            arch: "x86_64".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        leaf_packages: None,
        auto_packages: None,
        leaf_dep_tree: serde_json::json!({}),
        ..Default::default()
    });
    let host_b = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "git".into(),
            arch: "x86_64".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        leaf_packages: None,
        auto_packages: None,
        leaf_dep_tree: serde_json::json!({}),
        ..Default::default()
    });

    let hostnames = vec!["host-a".into(), "host-b".into()];
    let (merged, _) = merge_rpm_sections(vec![host_a, host_b], 2, &hostnames, None).unwrap();

    // Full degraded triplet
    assert_eq!(merged.leaf_packages, None);
    assert_eq!(merged.auto_packages, None);
    assert_eq!(merged.leaf_dep_tree, serde_json::json!({}));

    // packages_added NOT filtered (no authoritative data)
    assert_eq!(merged.packages_added.len(), 1);
    assert_eq!(merged.packages_added[0].name, "git");

    // Coverage metadata
    assert_eq!(merged.leaf_authority_hosts, Some(0));
    assert_eq!(merged.leaf_total_hosts, Some(2));
}

#[test]
fn test_aggregate_leaf_intersection_authoritative_empty() {
    // host_a: git leaf
    // host_b: leaf_packages = Some(vec![]) — authoritative but empty
    // Result: intersection = Some(vec![]) NOT None, packages_added = 0
    let host_a = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "git".into(),
            arch: "x86_64".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        leaf_packages: Some(vec!["git.x86_64".into()]),
        auto_packages: Some(vec![]),
        leaf_dep_tree: serde_json::json!({"git.x86_64": []}),
        ..Default::default()
    });
    let host_b = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "git".into(),
            arch: "x86_64".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        leaf_packages: Some(vec![]),
        auto_packages: Some(vec![]),
        leaf_dep_tree: serde_json::json!({}),
        ..Default::default()
    });

    let hostnames = vec!["host-a".into(), "host-b".into()];
    let (merged, _) = merge_rpm_sections(vec![host_a, host_b], 2, &hostnames, None).unwrap();

    // Intersection of {git.x86_64} and {} = empty, but Some (not None)
    assert_eq!(merged.leaf_packages, Some(vec![]));

    // packages_added filtered to empty
    assert_eq!(merged.packages_added.len(), 0);

    // Both hosts are authoritative
    assert_eq!(merged.leaf_authority_hosts, Some(2));
    assert_eq!(merged.leaf_total_hosts, Some(2));
}

#[test]
fn test_aggregate_leaf_dep_tree_donor_from_authoritative_host() {
    // "alpha" sorts first but is degraded (leaf_packages: None, leaf_dep_tree: {})
    // "beta" sorts second but is authoritative (leaf_packages: Some, leaf_dep_tree: real data)
    // Result: dep tree comes from beta, not alpha
    let host_alpha = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "git".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "curl".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        leaf_packages: None,
        auto_packages: None,
        leaf_dep_tree: serde_json::json!({}),
        ..Default::default()
    });
    let host_beta = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "git".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "curl".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["git.x86_64".into(), "curl.x86_64".into()]),
        auto_packages: Some(vec![]),
        leaf_dep_tree: serde_json::json!({
            "git.x86_64": ["perl-libs.x86_64"],
            "curl.x86_64": ["libcurl.x86_64"]
        }),
        ..Default::default()
    });

    // "alpha" sorts before "beta" — degraded host is first alphabetically
    let hostnames = vec!["alpha".into(), "beta".into()];
    let (merged, _) = merge_rpm_sections(vec![host_alpha, host_beta], 2, &hostnames, None).unwrap();

    // Dep tree must come from beta (authoritative), NOT alpha (degraded)
    let tree = merged.leaf_dep_tree.as_object().unwrap();
    assert_eq!(
        tree.len(),
        2,
        "dep tree must have entries from authoritative host"
    );
    assert!(tree.contains_key("git.x86_64"));
    assert!(tree.contains_key("curl.x86_64"));

    // Verify actual dep content
    assert_eq!(tree["git.x86_64"], serde_json::json!(["perl-libs.x86_64"]));
    assert_eq!(tree["curl.x86_64"], serde_json::json!(["libcurl.x86_64"]));

    // Coverage metadata
    assert_eq!(merged.leaf_authority_hosts, Some(1));
    assert_eq!(merged.leaf_total_hosts, Some(2));
}

#[test]
fn test_aggregate_leaf_intersection_order_independent() {
    // Same data, different hostname ordering
    // Result: identical leaf_packages in both cases, sorted by canonical identity
    let make_section = || RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "git".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "vim".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["git.x86_64".into(), "vim.x86_64".into()]),
        auto_packages: Some(vec![]),
        leaf_dep_tree: serde_json::json!({"git.x86_64": [], "vim.x86_64": []}),
        ..Default::default()
    };

    // Order 1: host-a first, host-b second
    let hostnames_1 = vec!["host-a".into(), "host-b".into()];
    let (merged_1, _) = merge_rpm_sections(
        vec![Some(make_section()), Some(make_section())],
        2,
        &hostnames_1,
        None,
    )
    .unwrap();

    // Order 2: host-b first, host-a second
    let hostnames_2 = vec!["host-b".into(), "host-a".into()];
    let (merged_2, _) = merge_rpm_sections(
        vec![Some(make_section()), Some(make_section())],
        2,
        &hostnames_2,
        None,
    )
    .unwrap();

    // leaf_packages must be identical regardless of input order
    assert_eq!(merged_1.leaf_packages, merged_2.leaf_packages);

    // packages_added names must be in the same order
    let names_1: Vec<&str> = merged_1
        .packages_added
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    let names_2: Vec<&str> = merged_2
        .packages_added
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names_1, names_2);
}

#[test]
fn test_aggregate_leaf_intersection_multiarch_identity() {
    // glibc.x86_64 is leaf, glibc.i686 is auto (on both hosts)
    // Result: only glibc.x86_64 in packages_added, glibc.i686 filtered
    let make_host = || RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "glibc".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "glibc".into(),
                arch: "i686".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["glibc.x86_64".into()]),
        auto_packages: Some(vec!["glibc.i686".into()]),
        leaf_dep_tree: serde_json::json!({"glibc.x86_64": ["glibc.i686"]}),
        ..Default::default()
    };

    let hostnames = vec!["host-a".into(), "host-b".into()];
    let (merged, _) = merge_rpm_sections(
        vec![Some(make_host()), Some(make_host())],
        2,
        &hostnames,
        None,
    )
    .unwrap();

    // Only x86_64 variant survives — i686 is auto
    assert_eq!(merged.packages_added.len(), 1);
    assert_eq!(merged.packages_added[0].name, "glibc");
    assert_eq!(merged.packages_added[0].arch, "x86_64");

    assert_eq!(merged.leaf_packages, Some(vec!["glibc.x86_64".into()]));
}

#[test]
fn test_aggregate_leaf_intersection_host_absent_package() {
    // host_a: git + vim both leaf
    // host_b: only vim leaf (git not present at all)
    // Result: git falls out of intersection (not leaf on host_b)
    let host_a = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "git".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "vim".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["git.x86_64".into(), "vim.x86_64".into()]),
        auto_packages: Some(vec![]),
        leaf_dep_tree: serde_json::json!({"git.x86_64": [], "vim.x86_64": []}),
        ..Default::default()
    });
    let host_b = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "vim".into(),
            arch: "x86_64".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        leaf_packages: Some(vec!["vim.x86_64".into()]),
        auto_packages: Some(vec![]),
        leaf_dep_tree: serde_json::json!({"vim.x86_64": []}),
        ..Default::default()
    });

    let hostnames = vec!["host-a".into(), "host-b".into()];
    let (merged, _) = merge_rpm_sections(vec![host_a, host_b], 2, &hostnames, None).unwrap();

    // Intersection is vim only (git not leaf on host_b because host_b
    // doesn't have it at all).
    assert_eq!(merged.leaf_packages, Some(vec!["vim.x86_64".into()]));
    // Both packages survive: vim (intersection leaf) + git (union leaf, partial)
    assert_eq!(merged.packages_added.len(), 2);
    let vim = merged
        .packages_added
        .iter()
        .find(|p| p.name == "vim")
        .unwrap();
    let git = merged
        .packages_added
        .iter()
        .find(|p| p.name == "git")
        .unwrap();
    assert!(
        vim.disposition.is_included(),
        "intersection leaf vim must have include=true"
    );
    assert!(
        !git.disposition.is_included(),
        "partial leaf git must have include=false"
    );
}

#[test]
fn test_aggregate_leaf_filtered_packages_absent_from_repo_conflicts() {
    // git: baseos on both hosts (no conflict, leaf)
    // perl-libs: epel on host_a, appstream on host_b (conflict, but auto)
    // Result: perl-libs NOT in repo_conflicts, NOT in packages_added
    let host_a = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "git".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                source_repo: "baseos".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "perl-libs".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                source_repo: "epel".into(),
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["git.x86_64".into()]),
        auto_packages: Some(vec!["perl-libs.x86_64".into()]),
        leaf_dep_tree: serde_json::json!({"git.x86_64": ["perl-libs.x86_64"]}),
        ..Default::default()
    });
    let host_b = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "git".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                source_repo: "baseos".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "perl-libs".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                source_repo: "appstream".into(),
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["git.x86_64".into()]),
        auto_packages: Some(vec!["perl-libs.x86_64".into()]),
        leaf_dep_tree: serde_json::json!({"git.x86_64": ["perl-libs.x86_64"]}),
        ..Default::default()
    });

    let hostnames = vec!["host-a".into(), "host-b".into()];
    let (merged, repo_conflicts) =
        merge_rpm_sections(vec![host_a, host_b], 2, &hostnames, None).unwrap();

    // perl-libs is auto — must be filtered before repo-conflict detection
    assert!(
        !repo_conflicts.contains_key("perl-libs.x86_64"),
        "auto package perl-libs must not appear in repo_conflicts"
    );
    assert!(
        merged.packages_added.iter().all(|p| p.name != "perl-libs"),
        "auto package perl-libs must not appear in packages_added"
    );

    // git should remain
    assert_eq!(merged.packages_added.len(), 1);
    assert_eq!(merged.packages_added[0].name, "git");
}

#[test]
fn test_aggregate_leaf_triplet_coherence() {
    // 3 packages, 2 hosts with different leaf sets
    // host_a: git + vim leaf, curl auto
    // host_b: git + curl leaf, vim auto
    // Intersection: only git (leaf on both)
    // Result: intersection consistent across leaf_packages, auto_packages (None),
    //         leaf_dep_tree, and packages_added. Every package in packages_added
    //         is in leaf_packages.
    let host_a = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "git".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "vim".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "curl".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["git.x86_64".into(), "vim.x86_64".into()]),
        auto_packages: Some(vec!["curl.x86_64".into()]),
        leaf_dep_tree: serde_json::json!({
            "git.x86_64": ["perl-libs.x86_64"],
            "vim.x86_64": ["ncurses.x86_64"]
        }),
        ..Default::default()
    });
    let host_b = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "git".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "vim".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "curl".into(),
                arch: "x86_64".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["git.x86_64".into(), "curl.x86_64".into()]),
        auto_packages: Some(vec!["vim.x86_64".into()]),
        leaf_dep_tree: serde_json::json!({
            "git.x86_64": ["perl-libs.x86_64"],
            "curl.x86_64": ["libcurl.x86_64"]
        }),
        ..Default::default()
    });

    let hostnames = vec!["host-a".into(), "host-b".into()];
    let (merged, _) = merge_rpm_sections(vec![host_a, host_b], 2, &hostnames, None).unwrap();

    // leaf_packages: intersection = {git.x86_64}
    assert_eq!(merged.leaf_packages, Some(vec!["git.x86_64".into()]));

    // auto_packages: None for aggregate
    assert_eq!(merged.auto_packages, None);

    // leaf_dep_tree: only contains entries for intersection packages
    let tree = merged.leaf_dep_tree.as_object().unwrap();
    assert_eq!(tree.len(), 1);
    assert!(tree.contains_key("git.x86_64"));
    assert!(!tree.contains_key("vim.x86_64"), "vim not in intersection");
    assert!(
        !tree.contains_key("curl.x86_64"),
        "curl not in intersection"
    );

    // packages_added: only git survives
    assert_eq!(merged.packages_added.len(), 1);
    assert_eq!(merged.packages_added[0].name, "git");

    // Coherence: every package in packages_added is in leaf_packages
    let leaf_set: std::collections::HashSet<String> = merged
        .leaf_packages
        .as_ref()
        .unwrap()
        .iter()
        .cloned()
        .collect();
    for pkg in &merged.packages_added {
        let id = format!("{}.{}", pkg.name, pkg.arch);
        assert!(
            leaf_set.contains(&id),
            "packages_added entry {id} must be in leaf_packages"
        );
    }

    // Coverage metadata
    assert_eq!(merged.leaf_authority_hosts, Some(2));
    assert_eq!(merged.leaf_total_hosts, Some(2));
}

#[test]
fn test_aggregate_leaf_survivor_not_suppressed_by_non_universal_narrowing() {
    // host_a (authoritative): vim is leaf, present
    // host_b (degraded): vim absent, leaf_packages: None
    // narrow_non_universal sets include=false (count=1, total=2)
    // BUT vim survived the leaf intersection -> include must be true
    let host_a = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "vim".into(),
            arch: "x86_64".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        leaf_packages: Some(vec!["vim.x86_64".into()]),
        ..Default::default()
    });
    let host_b = Some(RpmSection {
        packages_added: vec![],
        leaf_packages: None, // degraded
        ..Default::default()
    });

    let hostnames = vec!["alpha".into(), "beta".into()];
    let (merged, _) = merge_rpm_sections(vec![host_a, host_b], 2, &hostnames, None).unwrap();

    // vim must be in packages_added AND include=true
    assert_eq!(merged.packages_added.len(), 1);
    assert_eq!(merged.packages_added[0].name, "vim");
    assert!(
        merged.packages_added[0].disposition.is_included(),
        "leaf intersection survivor must have include=true despite non-universal narrowing"
    );
}

#[test]
fn test_aggregate_degraded_state_json_contract() {
    // All degraded aggregate merge
    let host_a = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "vim".into(),
            arch: "x86_64".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        leaf_packages: None,
        ..Default::default()
    });
    let host_b = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "vim".into(),
            arch: "x86_64".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        leaf_packages: None,
        ..Default::default()
    });

    let hostnames = vec!["a".into(), "b".into()];
    let (merged, _) = merge_rpm_sections(vec![host_a, host_b], 2, &hostnames, None).unwrap();

    let json = serde_json::to_value(&merged).unwrap();
    assert!(
        json["leaf_packages"].is_null(),
        "degraded leaf_packages must be null"
    );
    assert!(
        json["auto_packages"].is_null(),
        "degraded auto_packages must be null"
    );
    assert_eq!(
        json["leaf_dep_tree"],
        serde_json::json!({}),
        "degraded leaf_dep_tree must be empty object"
    );
    assert_eq!(
        json["leaf_authority_hosts"], 0,
        "all-degraded authority must be 0"
    );
    assert_eq!(
        json["leaf_total_hosts"], 2,
        "total hosts must reflect aggregate size"
    );
}

/// A carried unit from the vendor directory, owned as given.
fn test_carried_unit(
    name: inspectah_core::types::units::UnitName,
    owning_package: Option<String>,
) -> inspectah_core::types::units::CarriedUnit {
    use inspectah_core::types::units::{CarriedUnit, UnitFragment, UnitSubject};
    let fragment = UnitFragment::Vendor {
        file: name.clone(),
        owning_package,
    };
    CarriedUnit::try_from(UnitSubject::new(name, fragment).expect("file is the unit"))
        .expect("a test unit is carried")
}

/// § 12 "The exception", T18 (b): `merge_items` includes every row on
/// every host, so a runtime updater each host excluded would come out
/// included. The merge applies the updater default after it: the merged
/// `dnf-automatic.timer` row is excluded with the reason and not locked.
/// Control: an ordinary unit excluded on both hosts merges included, as
/// every merged row does today.
#[test]
fn a_runtime_updater_excluded_on_every_host_merges_excluded() {
    use inspectah_core::baseline::{RUNTIME_UPDATER_REASON, apply_runtime_updater_default};
    let make_sc = |unit: &str| {
        let mut change = UnitStateChange::new(
            test_carried_unit(unit.to_owned().try_into().unwrap(), None),
            UnitState::Enabled,
            Some(PresetDefault::Disable),
        )
        .unwrap();
        apply_runtime_updater_default(&mut change);
        change.disposition = FindingKind::excluded();
        change
    };
    let section = || UnitSection {
        state_changes: vec![make_sc("dnf-automatic.timer"), make_sc("httpd.service")],
        inventory: vec![],
        drop_ins: vec![],
        mount_findings: Vec::new(),
        manual_links: Vec::new(),
        undetermined_installs: Vec::new(),
        default_target: None,
        enabled_on_any_host: Vec::new(),
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into()];
    let result =
        merge_service_sections(vec![Some(section()), Some(section())], 2, &hostnames).unwrap();
    let row = |unit: &str| {
        result
            .state_changes
            .iter()
            .find(|c| c.unit().name().as_str() == unit)
            .unwrap_or_else(|| panic!("no merged row for {unit}"))
    };
    let updater = row("dnf-automatic.timer");
    assert!(!updater.disposition.is_included(), "{updater:?}");
    assert!(!updater.locked, "{updater:?}");
    assert_eq!(
        updater.attention_reason.as_deref(),
        Some(RUNTIME_UPDATER_REASON)
    );
    assert_eq!(updater.aggregate.as_ref().map(|a| a.count), Some(2));
    assert!(row("httpd.service").disposition.is_included());
}

/// An administrator's mount finding for `unit`, written in `/etc`.
fn admin_mount_finding(
    unit: &str,
    state: Option<UnitState>,
    mount_point: &str,
    nofail: bool,
) -> inspectah_core::types::units::MountUnitFinding {
    use inspectah_core::types::units::{AdvisoryUnit, MountUnitFinding, UnitFragment, UnitSubject};
    let name: inspectah_core::types::units::UnitName = unit.to_owned().try_into().unwrap();
    let fragment = UnitFragment::Local { file: name.clone() };
    MountUnitFinding {
        unit: AdvisoryUnit::try_from(UnitSubject::new(name, fragment).expect("a subject"))
            .expect("an administrator's storage unit"),
        state_change: state.map(|s| (s, None)),
        mount_point: Some(mount_point.into()),
        nofail: Some(nofail),
        aggregate: None,
    }
}

/// § 12: `mount_findings` merge by unit name, one row per name, its
/// `aggregate` naming every host that recorded it. Hosts that disagree
/// about a field keep one host's whole finding, as a state change does:
/// the value the most hosts recorded, a tie going to the first host by
/// hostname. A finding on some hosts only is still a row (it has no
/// include flag to narrow), with its count.
#[test]
fn mount_findings_merge_to_one_row_per_unit_name() {
    use UnitState::{Enabled, Masked};
    let section = |findings| UnitSection {
        mount_findings: findings,
        ..Default::default()
    };
    let hostnames: Vec<String> = vec!["h1".into(), "h2".into(), "h3".into()];
    let result = merge_service_sections(
        vec![
            Some(section(vec![
                admin_mount_finding("srv.mount", Some(Enabled), "/srv", false),
                admin_mount_finding("data.mount", Some(Enabled), "/data", true),
                admin_mount_finding("opt.mount", Some(Masked), "/opt", true),
            ])),
            Some(section(vec![
                admin_mount_finding("srv.mount", Some(Enabled), "/srv", true),
                admin_mount_finding("opt.mount", None, "/opt/x", false),
            ])),
            Some(section(vec![admin_mount_finding(
                "srv.mount",
                Some(Enabled),
                "/srv",
                true,
            )])),
        ],
        3,
        &hostnames,
    )
    .unwrap();
    // Unit name, mount point, nofail, count and hosts of each merged row.
    type Row<'a> = (&'a str, Option<&'a str>, Option<bool>, i32, Vec<String>);
    let rows: Vec<Row<'_>> = result
        .mount_findings
        .iter()
        .map(|f| {
            let aggregate = f.aggregate.as_ref().expect("an aggregate row");
            (
                f.unit.name().as_unit_name().as_str(),
                f.mount_point.as_deref(),
                f.nofail,
                aggregate.count,
                aggregate.hosts.clone(),
            )
        })
        .collect();
    let hosts = |names: &[&str]| names.iter().map(|h| (*h).to_owned()).collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            ("data.mount", Some("/data"), Some(true), 1, hosts(&["h1"])),
            // A tie between h1 and h2: h1's finding, both hosts counted.
            (
                "opt.mount",
                Some("/opt"),
                Some(true),
                2,
                hosts(&["h1", "h2"])
            ),
            // h2 and h3 agree: their finding, every host counted.
            (
                "srv.mount",
                Some("/srv"),
                Some(true),
                3,
                hosts(&["h1", "h2", "h3"])
            ),
        ]
    );
    let opt = &result.mount_findings[1];
    assert_eq!(opt.state_change, Some((Masked, None)), "h1's whole finding");
    assert!(
        result
            .mount_findings
            .iter()
            .all(|f| f.aggregate.as_ref().is_some_and(|a| a.total == 3))
    );
}

// ---------------------------------------------------------------------------
// § 12: the phase 4 fields of a state change, and the link findings
// ---------------------------------------------------------------------------

/// A unit name for a test.
fn unit_name(unit: &str) -> inspectah_core::types::units::UnitName {
    unit.to_owned().try_into().expect("a test unit name")
}

/// An `Also=` member recorded with `links` and the vendor drop-ins named.
fn member(
    unit: &str,
    links: inspectah_core::types::units::MemberLinks,
    vendor_drop_ins: &[&str],
) -> inspectah_core::types::units::AlsoMember {
    inspectah_core::types::units::AlsoMember {
        unit: unit_name(unit),
        links,
        vendor_drop_ins: vendor_drop_ins.iter().map(|n| (*n).to_owned()).collect(),
    }
}

/// `bundle.service` enabled, recorded with `also`, `install_drop_ins` and
/// `vendor_drop_ins`.
fn bundle_change(
    also: Vec<inspectah_core::types::units::AlsoMember>,
    install_drop_ins: &[&str],
    vendor_drop_ins: &[&str],
) -> UnitStateChange {
    let mut change = UnitStateChange::new(
        test_carried_unit(unit_name("bundle.service"), Some("bundle".into())),
        UnitState::Enabled,
        Some(PresetDefault::Disable),
    )
    .expect("a vendor unit can be enabled");
    change.also = also;
    change.install_drop_ins = install_drop_ins.iter().map(|p| (*p).to_owned()).collect();
    change.vendor_drop_ins = vendor_drop_ins.iter().map(|n| (*n).to_owned()).collect();
    change
}

/// A section holding `state_changes` alone.
fn changes_section(state_changes: Vec<UnitStateChange>) -> Option<UnitSection> {
    Some(UnitSection {
        state_changes,
        ..Default::default()
    })
}

/// Host names `h1`, `h2`, ... for `n` hosts, in hostname order.
fn host_names(n: usize) -> Vec<String> {
    (1..=n).map(|i| format!("h{i}")).collect()
}

/// § 12: the list fields of a state change are the union across every
/// host that has the unit. Within `also`, a member two hosts record with
/// different `links` becomes `Other`, a member both record alike keeps its
/// value, a member one host records is kept as recorded, and a member's
/// `vendor_drop_ins` is the union of the names. Members keep the order the
/// first host recorded, then each later host's new ones. The lists are the
/// same whichever host's row the merge keeps.
#[test]
fn a_state_change_merges_the_union_of_its_phase_4_lists() {
    use inspectah_core::types::units::MemberLinks::{Inert, Installed, Other, Unlinked};
    let h1 = bundle_change(
        vec![
            member("a.service", Installed, &["10-a.conf"]),
            member("b.service", Inert, &[]),
        ],
        &["/etc/systemd/system/bundle.service.d/10-x.conf"],
        &["10-x.conf"],
    );
    let h2 = bundle_change(
        vec![
            member("c.service", Installed, &[]),
            member("a.service", Unlinked, &["20-a.conf"]),
            member("b.service", Inert, &[]),
        ],
        &["/etc/systemd/system/bundle.service.d/20-y.conf"],
        &["20-y.conf", "10-x.conf"],
    );
    let result = merge_service_sections(
        vec![changes_section(vec![h1]), changes_section(vec![h2])],
        2,
        &host_names(2),
    )
    .expect("a merged section");
    assert_eq!(result.state_changes.len(), 1, "one row per unit name");
    let merged = &result.state_changes[0];
    assert_eq!(
        merged.also,
        vec![
            member("a.service", Other, &["10-a.conf", "20-a.conf"]),
            member("b.service", Inert, &[]),
            member("c.service", Installed, &[]),
        ]
    );
    assert_eq!(
        merged.install_drop_ins,
        [
            "/etc/systemd/system/bundle.service.d/10-x.conf",
            "/etc/systemd/system/bundle.service.d/20-y.conf",
        ]
    );
    assert_eq!(merged.vendor_drop_ins, ["10-x.conf", "20-y.conf"]);
    assert_eq!(merged.aggregate.as_ref().map(|a| a.count), Some(2));

    // Control: one host alone keeps its lists as recorded.
    let alone = merge_service_sections(
        vec![changes_section(vec![bundle_change(
            vec![member("a.service", Unlinked, &["20-a.conf"])],
            &[],
            &[],
        )])],
        1,
        &host_names(1),
    )
    .expect("a merged section");
    assert_eq!(
        alone.state_changes[0].also,
        vec![member("a.service", Unlinked, &["20-a.conf"])]
    );
}

/// `dirsrv@example.service` enabled, recorded with the configuration and
/// data paths given.
fn instance_change(required_config: &[&str], data_paths: &[&str]) -> UnitStateChange {
    let name = unit_name("dirsrv@example.service");
    let mut change = UnitStateChange::new(
        test_carried_unit(name, Some("389-ds-base".into())),
        UnitState::Enabled,
        Some(PresetDefault::Disable),
    )
    .expect("an instance can be enabled");
    change.required_config = required_config.iter().map(|p| (*p).to_owned()).collect();
    change.data_paths = data_paths.iter().map(|p| (*p).to_owned()).collect();
    change
}

/// T44: two hosts' `needs_unknown` merge, in either host order.
/// § 12: an instance whose needs one host could not read keeps that
/// host's `needs_unknown` on the merged row, whichever host's row the
/// merge keeps, so the plan omits the fleet's enable. Control: with no
/// host recording one, the merged row has none.
#[test]
fn an_instance_merges_the_needs_unknown_of_any_host() {
    use inspectah_core::types::units::InstanceNeedsUnknown;
    let known = instance_change(&["/etc/dirsrv/slapd-example"], &[]);
    let mut unknown = instance_change(&[], &[]);
    let cause = InstanceNeedsUnknown::Unreadable {
        path: "/etc/systemd/system/service.d".to_owned(),
    };
    unknown.needs_unknown = Some(cause.clone());
    for sections in [
        vec![
            changes_section(vec![known.clone()]),
            changes_section(vec![unknown.clone()]),
        ],
        vec![
            changes_section(vec![unknown.clone()]),
            changes_section(vec![known.clone()]),
        ],
    ] {
        let result = merge_service_sections(sections, 2, &host_names(2)).expect("a merged section");
        assert_eq!(result.state_changes.len(), 1, "one row per unit name");
        assert_eq!(result.state_changes[0].needs_unknown, Some(cause.clone()));
    }
    let control = merge_service_sections(
        vec![
            changes_section(vec![known.clone()]),
            changes_section(vec![known]),
        ],
        2,
        &host_names(2),
    )
    .expect("a merged section");
    assert_eq!(control.state_changes[0].needs_unknown, None);
}

/// § 12: an instance's `required_config` and `data_paths` are each the
/// union across every host that has it, sorted, whichever host's row the
/// merge keeps. Control: one host alone keeps its lists.
#[test]
fn an_instance_merges_the_union_of_what_it_needs() {
    let h1 = instance_change(
        &["/etc/dirsrv/slapd-example"],
        &["/var/lib/dirsrv/slapd-example"],
    );
    let h2 = instance_change(
        &["/etc/sysconfig/dirsrv-example", "/etc/dirsrv/slapd-example"],
        &[],
    );
    for sections in [
        vec![
            changes_section(vec![h1.clone()]),
            changes_section(vec![h2.clone()]),
        ],
        vec![
            changes_section(vec![h2.clone()]),
            changes_section(vec![h1.clone()]),
        ],
    ] {
        let result = merge_service_sections(sections, 2, &host_names(2)).expect("a merged section");
        assert_eq!(result.state_changes.len(), 1, "one row per unit name");
        let merged = &result.state_changes[0];
        assert_eq!(
            merged.required_config,
            ["/etc/dirsrv/slapd-example", "/etc/sysconfig/dirsrv-example"]
        );
        assert_eq!(merged.data_paths, ["/var/lib/dirsrv/slapd-example"]);
    }

    let alone = merge_service_sections(vec![changes_section(vec![h2])], 1, &host_names(1))
        .expect("a merged section");
    assert_eq!(
        alone.state_changes[0].required_config,
        ["/etc/dirsrv/slapd-example", "/etc/sysconfig/dirsrv-example"]
    );
    assert!(alone.state_changes[0].data_paths.is_empty());
}

/// A link finding for a test.
fn manual_link(
    relation: inspectah_core::types::units::LinkRelation,
    parent: &str,
    unit: &str,
    cause: inspectah_core::types::units::ManualLinkCause,
    install_files: &[&str],
) -> inspectah_core::types::units::ManualLink {
    inspectah_core::types::units::ManualLink {
        relation,
        parent: unit_name(parent),
        unit: unit_name(unit),
        cause,
        install_files: install_files.iter().map(|p| (*p).to_owned()).collect(),
        aggregate: None,
    }
}

/// A section holding `manual_links` alone.
fn links_section(
    manual_links: Vec<inspectah_core::types::units::ManualLink>,
) -> Option<UnitSection> {
    Some(UnitSection {
        manual_links,
        ..Default::default()
    })
}

/// The count and hosts of a merged finding's prevalence.
fn prevalence(
    aggregate: &Option<inspectah_core::types::aggregate::AggregatePrevalence>,
) -> (i32, i32, Vec<String>) {
    let aggregate = aggregate.as_ref().expect("an aggregate row");
    (aggregate.count, aggregate.total, aggregate.hosts.clone())
}

/// § 12: `manual_links` merge by relation, parent and unit. Two hosts with
/// the same `VendorInstallOnly` link and different `install_files` give one
/// row with the union of the files and the prevalence of both. Two hosts
/// with opposing install-difference causes for one triple give the first
/// host's cause in hostname order, with the prevalence of both. A link
/// that differs in relation, parent or unit is a row of its own.
#[test]
fn manual_links_merge_by_relation_parent_and_unit() {
    use inspectah_core::types::units::LinkRelation::{Also, Requires, Wants};
    use inspectah_core::types::units::ManualLinkCause::{
        HostInstallOnly, StaticUnit, VendorInstallOnly,
    };
    const X_DROP_IN: &str = "/etc/systemd/system/x.service.d/10-a.conf";
    const X_OTHER_DROP_IN: &str = "/etc/systemd/system/x.service.d/20-b.conf";
    let result = merge_service_sections(
        vec![
            links_section(vec![
                manual_link(
                    Wants,
                    "multi-user.target",
                    "x.service",
                    VendorInstallOnly,
                    &[X_DROP_IN],
                ),
                manual_link(
                    Also,
                    "y.service",
                    "z.service",
                    HostInstallOnly,
                    &["/etc/systemd/system/y.service"],
                ),
                manual_link(Requires, "multi-user.target", "x.service", StaticUnit, &[]),
            ]),
            links_section(vec![
                manual_link(
                    Wants,
                    "multi-user.target",
                    "x.service",
                    VendorInstallOnly,
                    &[X_OTHER_DROP_IN, X_DROP_IN],
                ),
                manual_link(
                    Also,
                    "y.service",
                    "z.service",
                    VendorInstallOnly,
                    &["/etc/systemd/system/y.service.d/10-c.conf"],
                ),
            ]),
        ],
        2,
        &host_names(2),
    )
    .expect("a merged section");
    let both = (2, 2, host_names(2));
    let rows: Vec<_> = result
        .manual_links
        .iter()
        .map(|link| {
            (
                link.relation,
                link.parent.as_str(),
                link.unit.as_str(),
                link.cause,
                link.install_files.clone(),
                prevalence(&link.aggregate),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (
                Wants,
                "multi-user.target",
                "x.service",
                VendorInstallOnly,
                vec![X_DROP_IN.to_owned(), X_OTHER_DROP_IN.to_owned()],
                both.clone(),
            ),
            (
                Also,
                "y.service",
                "z.service",
                HostInstallOnly,
                vec![
                    "/etc/systemd/system/y.service".to_owned(),
                    "/etc/systemd/system/y.service.d/10-c.conf".to_owned(),
                ],
                both,
            ),
            (
                Requires,
                "multi-user.target",
                "x.service",
                StaticUnit,
                vec![],
                (1, 2, vec!["h1".to_owned()]),
            ),
        ]
    );

    // The first host in hostname order decides the cause, so the other
    // order gives the other cause.
    let swapped = merge_service_sections(
        vec![
            links_section(vec![manual_link(
                Also,
                "y.service",
                "z.service",
                VendorInstallOnly,
                &[],
            )]),
            links_section(vec![manual_link(
                Also,
                "y.service",
                "z.service",
                HostInstallOnly,
                &[],
            )]),
        ],
        2,
        &host_names(2),
    )
    .expect("a merged section");
    assert_eq!(swapped.manual_links.len(), 1);
    assert_eq!(swapped.manual_links[0].cause, VendorInstallOnly);
}

/// § 12: `undetermined_installs` merge by unit, with `install_files` the
/// union and the reason of the first host in hostname order; the
/// prevalence counts every host that recorded one. A record for another
/// unit is a row of its own.
#[test]
fn undetermined_installs_merge_by_unit() {
    use inspectah_core::types::units::InstallUnknown::{ForeignName, Specifier, UpheldBy};
    use inspectah_core::types::units::UndeterminedInstall;
    let record = |unit: &str, reason, files: &[&str]| UndeterminedInstall {
        unit: unit_name(unit),
        reason,
        install_files: files.iter().map(|p| (*p).to_owned()).collect(),
        aggregate: None,
    };
    let section = |records| {
        Some(UnitSection {
            undetermined_installs: records,
            ..Default::default()
        })
    };
    const A: &str = "/etc/systemd/system/u.service.d/10-a.conf";
    const B: &str = "/etc/systemd/system/u.service.d/20-b.conf";
    let result = merge_service_sections(
        vec![
            section(vec![record("u.service", Specifier, &[A])]),
            section(vec![
                record("u.service", ForeignName, &[B, A]),
                record("v.service", UpheldBy, &[]),
            ]),
        ],
        2,
        &host_names(2),
    )
    .expect("a merged section");
    let rows: Vec<_> = result
        .undetermined_installs
        .iter()
        .map(|r| {
            (
                r.unit.as_str(),
                r.reason,
                r.install_files.clone(),
                prevalence(&r.aggregate),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (
                "u.service",
                Specifier,
                vec![A.to_owned(), B.to_owned()],
                (2, 2, host_names(2)),
            ),
            ("v.service", UpheldBy, vec![], (1, 2, vec!["h2".to_owned()])),
        ]
    );
}

/// The default target `name`, recorded included, from a vendor file.
fn default_target(name: &str) -> inspectah_core::types::units::DefaultTarget {
    use inspectah_core::types::units::{TargetUnit, UnitFragment, UnitSubject};
    let fragment = UnitFragment::Vendor {
        file: unit_name(name),
        owning_package: Some("systemd".into()),
    };
    inspectah_core::types::units::DefaultTarget {
        target: TargetUnit::try_from(
            UnitSubject::new(unit_name(name), fragment).expect("file is the unit"),
        )
        .expect("a target"),
        disposition: FindingKind::included(),
        locked: false,
        aggregate: None,
    }
}

/// A section holding a default target alone, or none.
fn target_section(name: Option<&str>) -> Option<UnitSection> {
    Some(UnitSection {
        default_target: name.map(default_target),
        ..Default::default()
    })
}

/// § 12: `default_target` takes the most prevalent target, and is excluded
/// when the hosts do not all agree, the rule `narrow_non_universal`
/// applies. Three hosts, two agreeing: their target, prevalence two of
/// three, excluded. Every host agreeing: included. A tie goes to the first
/// host in hostname order. A host with no default target disagrees too. No
/// host with one: none.
#[test]
fn the_default_target_merges_to_the_most_prevalent_one() {
    let merged = |names: &[Option<&str>]| {
        merge_service_sections(
            names.iter().map(|n| target_section(*n)).collect(),
            names.len(),
            &host_names(names.len()),
        )
        .expect("a merged section")
        .default_target
    };
    let summary = |target: Option<inspectah_core::types::units::DefaultTarget>| {
        target.map(|t| {
            (
                t.target.name().as_unit_name().as_str().to_owned(),
                t.disposition.is_included(),
                prevalence(&t.aggregate),
            )
        })
    };
    let hosts = |names: &[&str]| names.iter().map(|h| (*h).to_owned()).collect::<Vec<_>>();

    assert_eq!(
        summary(merged(&[
            Some("multi-user.target"),
            Some("graphical.target"),
            Some("graphical.target"),
        ])),
        Some((
            "graphical.target".to_owned(),
            false,
            (2, 3, hosts(&["h2", "h3"]))
        ))
    );
    assert_eq!(
        summary(merged(&[
            Some("graphical.target"),
            Some("graphical.target")
        ])),
        Some(("graphical.target".to_owned(), true, (2, 2, host_names(2))))
    );
    assert_eq!(
        summary(merged(&[
            Some("multi-user.target"),
            Some("graphical.target")
        ])),
        Some((
            "multi-user.target".to_owned(),
            false,
            (1, 2, hosts(&["h1"]))
        ))
    );
    assert_eq!(
        summary(merged(&[Some("graphical.target"), None])),
        Some(("graphical.target".to_owned(), false, (1, 2, hosts(&["h1"]))))
    );
    assert_eq!(summary(merged(&[None, None])), None);
}
