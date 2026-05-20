use inspectah_core::fleet::merge::{merge_items, FleetMergeable};
use inspectah_core::types::config::ConfigFileEntry;
use inspectah_core::types::containers::{ComposeFile, QuadletUnit};
use inspectah_core::types::fleet::VariantSelection;
use inspectah_core::types::kernelboot::{KernelModule, SysctlOverride};
use inspectah_core::types::network::{FirewallZone, NMConnection};
use inspectah_core::types::nonrpm::NonRpmItem;
use inspectah_core::types::rpm::{EnabledModuleStream, PackageEntry, RepoFile, VersionLockEntry};
use inspectah_core::types::scheduled::CronJob;
use inspectah_core::types::selinux::SelinuxPortLabel;
use inspectah_core::types::services::SystemdDropIn;

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

    let a_items: Vec<&ConfigFileEntry> = merged.iter().filter(|e| e.path == "/etc/a.conf").collect();
    assert_eq!(a_items.len(), 1);
    assert_eq!(a_items[0].variant_selection, VariantSelection::Only);

    let b_items: Vec<&ConfigFileEntry> = merged.iter().filter(|e| e.path == "/etc/b.conf").collect();
    assert_eq!(b_items.len(), 2);
    assert!(b_items
        .iter()
        .any(|e| e.variant_selection == VariantSelection::Selected));
    assert!(b_items
        .iter()
        .any(|e| e.variant_selection == VariantSelection::Alternative));
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
