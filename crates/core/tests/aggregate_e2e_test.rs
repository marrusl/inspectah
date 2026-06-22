//! End-to-end integration tests for aggregate merge.
//!
//! Exercises the full merge_snapshots() pipeline with rich, multi-section
//! snapshots. Builds on the unit-level tests in aggregate_merge_test.rs,
//! aggregate_validate_test.rs, and aggregate_orchestrator_test.rs by combining
//! multiple populated sections per host and verifying cross-cutting
//! invariants (prevalence totals, variant selection, aggregate_meta, baseline
//! provisionality, deterministic output, validation errors).

use inspectah_core::aggregate::merge_snapshots;
use inspectah_core::aggregate::validate::AggregateValidationError;
use inspectah_core::baseline::{ResolutionStrategy, TargetImageIdentity};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::aggregate::VariantSelection;
use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::rpm::{PackageEntry, RpmSection};
use inspectah_core::types::services::{
    PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState,
};

// ===========================================================================
// Helpers
// ===========================================================================

/// Build a snapshot with hostname, arch, version_id, and OS name populated.
fn make_rich_snap(hostname: &str, version_id: &str) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.meta.insert(
        "hostname".to_string(),
        serde_json::Value::String(hostname.into()),
    );
    snap.meta.insert(
        "architecture".to_string(),
        serde_json::Value::String("x86_64".into()),
    );
    snap.os_release = Some(OsRelease {
        name: "Red Hat Enterprise Linux".into(),
        version_id: version_id.into(),
        id: "rhel".into(),
        ..Default::default()
    });
    snap.target_image = Some(TargetImageIdentity {
        image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.4".into(),
        strategy: ResolutionStrategy::BootcStatus,
    });
    snap
}

/// Add an RPM section with the given package names (all x86_64).
fn with_rpms(snap: &mut InspectionSnapshot, packages: &[&str]) {
    let entries = packages
        .iter()
        .map(|name| PackageEntry {
            name: name.to_string(),
            arch: "x86_64".into(),
            version: "1.0".into(),
            release: "1.el9".into(),
            ..Default::default()
        })
        .collect();
    snap.rpm = Some(RpmSection {
        packages_added: entries,
        ..Default::default()
    });
}

/// Add a config section with the given file paths and contents.
fn with_configs(snap: &mut InspectionSnapshot, files: &[(&str, &str)]) {
    let entries = files
        .iter()
        .map(|(path, content)| ConfigFileEntry {
            path: path.to_string(),
            content: content.to_string(),
            ..Default::default()
        })
        .collect();
    snap.config = Some(ConfigSection { files: entries });
}

/// Add a services section with the given unit names (all enabled).
fn with_services(snap: &mut InspectionSnapshot, units: &[&str]) {
    let changes = units
        .iter()
        .map(|unit| ServiceStateChange {
            unit: unit.to_string(),
            current_state: ServiceUnitState::Enabled,
            default_state: Some(PresetDefault::Disable),
            include: true,
            locked: false,
            owning_package: None,
            aggregate: None,
            attention_reason: None,
        })
        .collect();
    snap.services = Some(ServiceSection {
        state_changes: changes,
        enabled_units: units.iter().map(|u| u.to_string()).collect(),
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });
}

// ===========================================================================
// E2E: 3 hosts, shared packages, config variants
// ===========================================================================

#[test]
fn test_e2e_three_hosts_shared_packages_config_variants() {
    // Setup: 3 hosts, all share httpd, two share nginx, one has unique postgres.
    // Config /etc/app.conf has two variants across the aggregate.
    let mut s1 = make_rich_snap("web-01", "9.4");
    with_rpms(&mut s1, &["httpd", "nginx"]);
    with_configs(&mut s1, &[("/etc/app.conf", "version=1")]);
    with_services(&mut s1, &["httpd.service"]);

    let mut s2 = make_rich_snap("web-02", "9.4");
    with_rpms(&mut s2, &["httpd", "nginx"]);
    with_configs(&mut s2, &[("/etc/app.conf", "version=1")]);
    with_services(&mut s2, &["httpd.service"]);

    let mut s3 = make_rich_snap("web-03", "9.4");
    with_rpms(&mut s3, &["httpd", "postgres"]);
    with_configs(&mut s3, &[("/etc/app.conf", "version=2")]);
    with_services(&mut s3, &["httpd.service", "postgresql.service"]);

    let (merged, warnings) = merge_snapshots(vec![s1, s2, s3], None, None).unwrap();

    // aggregate_meta populated correctly
    let meta = merged.aggregate_meta.as_ref().unwrap();
    assert_eq!(meta.host_count, 3);
    assert_eq!(
        meta.hostnames,
        vec!["web-01", "web-02", "web-03"],
        "hostnames should be sorted"
    );
    assert_eq!(meta.label, "aggregate");

    // section_host_counts reflects which hosts had each section
    assert_eq!(meta.section_host_counts.get("rpm"), Some(&3));
    assert_eq!(meta.section_host_counts.get("config"), Some(&3));
    assert_eq!(meta.section_host_counts.get("services"), Some(&3));

    // RPM prevalence: httpd on all 3, nginx on 2, postgres on 1
    let rpm = merged.rpm.unwrap();
    let httpd = rpm
        .packages_added
        .iter()
        .find(|p| p.name == "httpd")
        .expect("httpd should be in merged RPMs");
    let httpd_agg = httpd.aggregate.as_ref().unwrap();
    assert_eq!(httpd_agg.count, 3);
    assert_eq!(httpd_agg.total, 3);

    let nginx = rpm
        .packages_added
        .iter()
        .find(|p| p.name == "nginx")
        .expect("nginx should be in merged RPMs");
    let nginx_agg = nginx.aggregate.as_ref().unwrap();
    assert_eq!(nginx_agg.count, 2);
    assert_eq!(nginx_agg.total, 3);

    let postgres = rpm
        .packages_added
        .iter()
        .find(|p| p.name == "postgres")
        .expect("postgres should be in merged RPMs");
    let postgres_agg = postgres.aggregate.as_ref().unwrap();
    assert_eq!(postgres_agg.count, 1);
    assert_eq!(postgres_agg.total, 3);

    // Config variant selection: /etc/app.conf has 2 variants.
    // "version=1" on 2 hosts => Selected, "version=2" on 1 host => Alternative.
    let config = merged.config.unwrap();
    let v1_entries: Vec<_> = config
        .files
        .iter()
        .filter(|f| f.path == "/etc/app.conf" && f.content == "version=1")
        .collect();
    assert_eq!(
        v1_entries.len(),
        1,
        "majority variant collapsed to one entry"
    );
    assert_eq!(
        v1_entries[0].variant_selection,
        VariantSelection::Selected,
        "majority variant should be Selected"
    );
    let v1_agg = v1_entries[0].aggregate.as_ref().unwrap();
    assert_eq!(v1_agg.count, 2);
    assert_eq!(v1_agg.total, 3);

    let v2_entries: Vec<_> = config
        .files
        .iter()
        .filter(|f| f.path == "/etc/app.conf" && f.content == "version=2")
        .collect();
    assert_eq!(v2_entries.len(), 1, "minority variant preserved");
    assert_eq!(
        v2_entries[0].variant_selection,
        VariantSelection::Alternative,
        "minority variant should be Alternative"
    );

    // Services: httpd.service on all 3, postgresql.service on 1
    let services = merged.services.unwrap();
    let httpd_svc = services
        .state_changes
        .iter()
        .find(|s| s.unit == "httpd.service")
        .expect("httpd.service should be in merged services");
    let httpd_svc_agg = httpd_svc.aggregate.as_ref().unwrap();
    assert_eq!(httpd_svc_agg.count, 3);
    assert_eq!(httpd_svc_agg.total, 3);

    let pg_svc = services
        .state_changes
        .iter()
        .find(|s| s.unit == "postgresql.service")
        .expect("postgresql.service should be in merged services");
    let pg_svc_agg = pg_svc.aggregate.as_ref().unwrap();
    assert_eq!(pg_svc_agg.count, 1);
    assert_eq!(pg_svc_agg.total, 3);

    // baseline_provisional should be false (all same target image)
    assert!(!meta.baseline_provisional);

    // No hard warnings expected for same-version, same-arch aggregate
    let has_arch_warning = warnings.iter().any(|w| {
        matches!(
            w,
            inspectah_core::aggregate::validate::AggregateWarning::SystemTypeMismatch { .. }
        )
    });
    assert!(!has_arch_warning);
}

// ===========================================================================
// E2E: Validation hard errors
// ===========================================================================

#[test]
fn test_e2e_validation_mixed_architecture() {
    let mut s1 = make_rich_snap("host-a", "9.4");
    with_rpms(&mut s1, &["httpd"]);

    let mut s2 = make_rich_snap("host-b", "9.4");
    with_rpms(&mut s2, &["httpd"]);
    // Override RPM arch to aarch64 — extract_architecture infers from
    // package arch fields, not the meta map.
    for pkg in &mut s2.rpm.as_mut().unwrap().packages_added {
        pkg.arch = "aarch64".into();
    }

    let result = merge_snapshots(vec![s1, s2], None, None);
    assert!(result.is_err(), "mixed architectures should fail");
    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| matches!(
        e,
        AggregateValidationError::ArchitectureMismatch { architectures }
        if architectures.len() == 2
    )));
}

#[test]
fn test_e2e_validation_duplicate_hostname() {
    let mut s1 = make_rich_snap("web-01", "9.4");
    with_rpms(&mut s1, &["httpd"]);
    with_configs(&mut s1, &[("/etc/app.conf", "a")]);

    let mut s2 = make_rich_snap("web-01", "9.4");
    with_rpms(&mut s2, &["nginx"]);
    with_configs(&mut s2, &[("/etc/app.conf", "b")]);

    let result = merge_snapshots(vec![s1, s2], None, None);
    assert!(result.is_err(), "duplicate hostnames should fail");
    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| matches!(
        e,
        AggregateValidationError::DuplicateHostname { hostname }
        if hostname == "web-01"
    )));
}

#[test]
fn test_e2e_validation_os_major_mismatch() {
    let mut s1 = make_rich_snap("host-a", "9.4");
    with_rpms(&mut s1, &["httpd"]);
    with_services(&mut s1, &["httpd.service"]);

    let mut s2 = make_rich_snap("host-b", "8.9");
    with_rpms(&mut s2, &["httpd"]);
    with_services(&mut s2, &["httpd.service"]);

    let result = merge_snapshots(vec![s1, s2], None, None);
    assert!(result.is_err(), "OS major version mismatch should fail");
    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| matches!(
        e,
        AggregateValidationError::OsMajorVersionMismatch { versions }
        if versions.len() == 2
    )));
}

// ===========================================================================
// E2E: Missing sections use global denominator
// ===========================================================================

#[test]
fn test_e2e_missing_section_uses_global_denominator() {
    // host-a has RPM + config, host-b has only config, host-c has RPM + config.
    // RPM total denominator should still be 3 (the aggregate size), not 2.
    let mut s1 = make_rich_snap("host-a", "9.4");
    with_rpms(&mut s1, &["httpd"]);
    with_configs(&mut s1, &[("/etc/app.conf", "v1")]);

    let mut s2 = make_rich_snap("host-b", "9.4");
    // No RPM section — host-b is RPM-free
    with_configs(&mut s2, &[("/etc/app.conf", "v1")]);
    // Need at least one section so validation doesn't flag EmptySnapshot
    // config satisfies that

    let mut s3 = make_rich_snap("host-c", "9.4");
    with_rpms(&mut s3, &["httpd"]);
    with_configs(&mut s3, &[("/etc/app.conf", "v1")]);

    let (merged, _) = merge_snapshots(vec![s1, s2, s3], None, None).unwrap();

    // aggregate_meta should show rpm present on 2 hosts
    let meta = merged.aggregate_meta.as_ref().unwrap();
    assert_eq!(meta.section_host_counts.get("rpm"), Some(&2));
    assert_eq!(meta.section_host_counts.get("config"), Some(&3));
    assert_eq!(meta.host_count, 3);

    // RPM prevalence uses total=3 (global aggregate size), not 2 (hosts with RPMs)
    let rpm = merged.rpm.unwrap();
    let httpd = rpm
        .packages_added
        .iter()
        .find(|p| p.name == "httpd")
        .unwrap();
    let agg = httpd.aggregate.as_ref().unwrap();
    assert_eq!(agg.count, 2, "httpd present on 2 hosts");
    assert_eq!(
        agg.total, 3,
        "total should be global aggregate size, not section count"
    );

    // Config prevalence also uses total=3
    let config = merged.config.unwrap();
    let app_conf = config
        .files
        .iter()
        .find(|f| f.path == "/etc/app.conf")
        .unwrap();
    let conf_agg = app_conf.aggregate.as_ref().unwrap();
    assert_eq!(conf_agg.count, 3);
    assert_eq!(conf_agg.total, 3);
}

#[test]
fn test_e2e_host_missing_services_still_counted_in_aggregate() {
    // host-a has services, host-b doesn't. Aggregate size = 2.
    let mut s1 = make_rich_snap("host-a", "9.4");
    with_rpms(&mut s1, &["httpd"]);
    with_services(&mut s1, &["httpd.service"]);

    let mut s2 = make_rich_snap("host-b", "9.4");
    with_rpms(&mut s2, &["httpd"]);
    // No services section

    let (merged, _) = merge_snapshots(vec![s1, s2], None, None).unwrap();

    let meta = merged.aggregate_meta.as_ref().unwrap();
    assert_eq!(meta.section_host_counts.get("services"), Some(&1));
    assert_eq!(meta.host_count, 2);

    let services = merged.services.unwrap();
    let httpd = services
        .state_changes
        .iter()
        .find(|s| s.unit == "httpd.service")
        .unwrap();
    let agg = httpd.aggregate.as_ref().unwrap();
    assert_eq!(agg.count, 1);
    assert_eq!(
        agg.total, 2,
        "services total should be global aggregate size"
    );
}

// ===========================================================================
// E2E: Baseline selection with provisionality
// ===========================================================================

#[test]
fn test_e2e_baseline_provisional_when_multiple_target_images() {
    // Two hosts point to different target images => baseline_provisional = true
    let mut s1 = make_rich_snap("host-a", "9.4");
    s1.target_image = Some(TargetImageIdentity {
        image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.4".into(),
        strategy: ResolutionStrategy::BootcStatus,
    });
    with_rpms(&mut s1, &["httpd"]);

    let mut s2 = make_rich_snap("host-b", "9.4");
    s2.target_image = Some(TargetImageIdentity {
        image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.3".into(),
        strategy: ResolutionStrategy::BootcStatus,
    });
    with_rpms(&mut s2, &["httpd"]);

    let mut s3 = make_rich_snap("host-c", "9.4");
    s3.target_image = Some(TargetImageIdentity {
        image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.4".into(),
        strategy: ResolutionStrategy::BootcStatus,
    });
    with_rpms(&mut s3, &["httpd"]);

    let (merged, warnings) = merge_snapshots(vec![s1, s2, s3], None, None).unwrap();

    let meta = merged.aggregate_meta.as_ref().unwrap();
    assert!(
        meta.baseline_provisional,
        "baseline should be provisional when target images differ"
    );

    // The selected target image should be the most common one (9.4, 2 hosts)
    let target = merged.target_image.as_ref().unwrap();
    assert_eq!(
        target.image_ref, "registry.redhat.io/rhel9/rhel-bootc:9.4",
        "most common target image should win"
    );

    // Should have a BaselineConflict warning
    assert!(
        warnings.iter().any(|w| matches!(
            w,
            inspectah_core::aggregate::validate::AggregateWarning::BaselineConflict { .. }
        )),
        "conflicting baselines should produce a warning"
    );
}

#[test]
fn test_e2e_baseline_not_provisional_when_unanimous() {
    let mut s1 = make_rich_snap("host-a", "9.4");
    with_rpms(&mut s1, &["httpd"]);

    let mut s2 = make_rich_snap("host-b", "9.4");
    with_rpms(&mut s2, &["httpd"]);

    let (merged, _) = merge_snapshots(vec![s1, s2], None, None).unwrap();

    let meta = merged.aggregate_meta.as_ref().unwrap();
    assert!(
        !meta.baseline_provisional,
        "unanimous target image should not be provisional"
    );
}

// ===========================================================================
// E2E: Deterministic output (input order independence)
// ===========================================================================

#[test]
fn test_e2e_deterministic_output_regardless_of_input_order() {
    // Build snapshots with distinct data per host
    let build_aggregate = || {
        let mut s1 = make_rich_snap("alpha", "9.4");
        with_rpms(&mut s1, &["httpd", "nginx"]);
        with_configs(&mut s1, &[("/etc/app.conf", "alpha-v1")]);
        with_services(&mut s1, &["httpd.service"]);

        let mut s2 = make_rich_snap("bravo", "9.4");
        with_rpms(&mut s2, &["httpd", "postgres"]);
        with_configs(&mut s2, &[("/etc/app.conf", "bravo-v1")]);
        with_services(&mut s2, &["httpd.service", "postgresql.service"]);

        let mut s3 = make_rich_snap("charlie", "9.4");
        with_rpms(&mut s3, &["httpd"]);
        with_configs(
            &mut s3,
            &[
                ("/etc/app.conf", "alpha-v1"),
                ("/etc/extra.conf", "charlie-only"),
            ],
        );
        with_services(&mut s3, &["httpd.service"]);

        (s1, s2, s3)
    };

    // Forward order
    let (s1a, s2a, s3a) = build_aggregate();
    let (merged_fwd, warnings_fwd) = merge_snapshots(vec![s1a, s2a, s3a], None, None).unwrap();

    // Reversed order
    let (s1b, s2b, s3b) = build_aggregate();
    let (merged_rev, warnings_rev) = merge_snapshots(vec![s3b, s2b, s1b], None, None).unwrap();

    // Compare aggregate_meta (except merged_at timestamp)
    let meta_fwd = merged_fwd.aggregate_meta.as_ref().unwrap();
    let meta_rev = merged_rev.aggregate_meta.as_ref().unwrap();
    assert_eq!(meta_fwd.host_count, meta_rev.host_count);
    assert_eq!(meta_fwd.hostnames, meta_rev.hostnames);
    assert_eq!(meta_fwd.label, meta_rev.label);
    assert_eq!(meta_fwd.baseline_provisional, meta_rev.baseline_provisional);
    assert_eq!(meta_fwd.section_host_counts, meta_rev.section_host_counts);

    // Compare RPM sections — same packages, same prevalence
    let rpm_fwd = merged_fwd.rpm.as_ref().unwrap();
    let rpm_rev = merged_rev.rpm.as_ref().unwrap();
    assert_eq!(
        rpm_fwd.packages_added.len(),
        rpm_rev.packages_added.len(),
        "same package count"
    );
    for pkg_fwd in &rpm_fwd.packages_added {
        let pkg_rev = rpm_rev
            .packages_added
            .iter()
            .find(|p| p.name == pkg_fwd.name)
            .unwrap_or_else(|| panic!("package {} missing in reversed merge", pkg_fwd.name));
        assert_eq!(
            pkg_fwd.aggregate, pkg_rev.aggregate,
            "prevalence for {} should match",
            pkg_fwd.name
        );
    }

    // Compare config sections — same files, same variants, same prevalence
    let config_fwd = merged_fwd.config.as_ref().unwrap();
    let config_rev = merged_rev.config.as_ref().unwrap();
    assert_eq!(
        config_fwd.files.len(),
        config_rev.files.len(),
        "same config file count"
    );
    for file_fwd in &config_fwd.files {
        let file_rev = config_rev
            .files
            .iter()
            .find(|f| f.path == file_fwd.path && f.content == file_fwd.content)
            .unwrap_or_else(|| {
                panic!(
                    "config {}:{} missing in reversed merge",
                    file_fwd.path, file_fwd.content
                )
            });
        assert_eq!(
            file_fwd.aggregate, file_rev.aggregate,
            "prevalence for {}:{} should match",
            file_fwd.path, file_fwd.content
        );
        assert_eq!(
            file_fwd.variant_selection, file_rev.variant_selection,
            "variant selection for {}:{} should match",
            file_fwd.path, file_fwd.content
        );
    }

    // Compare service sections
    let svc_fwd = merged_fwd.services.as_ref().unwrap();
    let svc_rev = merged_rev.services.as_ref().unwrap();
    assert_eq!(
        svc_fwd.state_changes.len(),
        svc_rev.state_changes.len(),
        "same service count"
    );
    for sc_fwd in &svc_fwd.state_changes {
        let sc_rev = svc_rev
            .state_changes
            .iter()
            .find(|s| s.unit == sc_fwd.unit)
            .unwrap_or_else(|| panic!("service {} missing in reversed merge", sc_fwd.unit));
        assert_eq!(
            sc_fwd.aggregate, sc_rev.aggregate,
            "prevalence for {} should match",
            sc_fwd.unit
        );
    }

    // Compare target image
    assert_eq!(
        merged_fwd.target_image, merged_rev.target_image,
        "target image should be identical"
    );

    // Warnings should match (order-independent comparison)
    assert_eq!(warnings_fwd.len(), warnings_rev.len(), "same warning count");
}

// ===========================================================================
// E2E: Serialization roundtrip of merged snapshot
// ===========================================================================

#[test]
fn test_e2e_merged_snapshot_serialization_roundtrip() {
    let mut s1 = make_rich_snap("host-a", "9.4");
    with_rpms(&mut s1, &["httpd", "nginx"]);
    with_configs(&mut s1, &[("/etc/httpd.conf", "ServerName host-a")]);
    with_services(&mut s1, &["httpd.service"]);

    let mut s2 = make_rich_snap("host-b", "9.4");
    with_rpms(&mut s2, &["httpd"]);
    with_configs(&mut s2, &[("/etc/httpd.conf", "ServerName host-b")]);
    with_services(&mut s2, &["httpd.service", "nginx.service"]);

    let (merged, _) = merge_snapshots(vec![s1, s2], None, None).unwrap();

    let json = serde_json::to_string_pretty(&merged).unwrap();
    let parsed: InspectionSnapshot = serde_json::from_str(&json).unwrap();

    // aggregate_meta survives roundtrip
    assert_eq!(merged.aggregate_meta, parsed.aggregate_meta);

    // RPM prevalence survives roundtrip
    let orig_rpm = merged.rpm.as_ref().unwrap();
    let parsed_rpm = parsed.rpm.as_ref().unwrap();
    assert_eq!(
        orig_rpm.packages_added.len(),
        parsed_rpm.packages_added.len()
    );
    for pkg in &orig_rpm.packages_added {
        let found = parsed_rpm
            .packages_added
            .iter()
            .find(|p| p.name == pkg.name)
            .unwrap();
        assert_eq!(pkg.aggregate, found.aggregate);
    }

    // Config variant_selection survives roundtrip
    let orig_config = merged.config.as_ref().unwrap();
    let parsed_config = parsed.config.as_ref().unwrap();
    for file in &orig_config.files {
        let found = parsed_config
            .files
            .iter()
            .find(|f| f.path == file.path && f.content == file.content)
            .unwrap();
        assert_eq!(file.variant_selection, found.variant_selection);
        assert_eq!(file.aggregate, found.aggregate);
    }
}

// ===========================================================================
// E2E: Multi-section snapshot with varying section coverage
// ===========================================================================

#[test]
fn test_e2e_heterogeneous_section_coverage() {
    // host-a: rpm + config + services (full)
    // host-b: rpm + services (no config)
    // host-c: config only (no rpm, no services)
    let mut s1 = make_rich_snap("host-a", "9.4");
    with_rpms(&mut s1, &["httpd", "curl"]);
    with_configs(
        &mut s1,
        &[
            ("/etc/httpd.conf", "v1"),
            ("/etc/sysctl.conf", "net.ipv4.ip_forward=1"),
        ],
    );
    with_services(&mut s1, &["httpd.service"]);

    let mut s2 = make_rich_snap("host-b", "9.4");
    with_rpms(&mut s2, &["httpd"]);
    with_services(&mut s2, &["httpd.service", "crond.service"]);

    let mut s3 = make_rich_snap("host-c", "9.4");
    with_configs(&mut s3, &[("/etc/sysctl.conf", "net.ipv4.ip_forward=1")]);
    // Need at least one section, config provides it

    let (merged, _) = merge_snapshots(vec![s1, s2, s3], None, None).unwrap();

    let meta = merged.aggregate_meta.as_ref().unwrap();
    assert_eq!(meta.host_count, 3);
    assert_eq!(meta.section_host_counts.get("rpm"), Some(&2));
    assert_eq!(meta.section_host_counts.get("config"), Some(&2));
    assert_eq!(meta.section_host_counts.get("services"), Some(&2));

    // RPM: httpd on 2 hosts (a,b), curl on 1 (a only)
    let rpm = merged.rpm.unwrap();
    let httpd = rpm
        .packages_added
        .iter()
        .find(|p| p.name == "httpd")
        .unwrap();
    assert_eq!(httpd.aggregate.as_ref().unwrap().count, 2);
    assert_eq!(httpd.aggregate.as_ref().unwrap().total, 3);

    let curl = rpm
        .packages_added
        .iter()
        .find(|p| p.name == "curl")
        .unwrap();
    assert_eq!(curl.aggregate.as_ref().unwrap().count, 1);
    assert_eq!(curl.aggregate.as_ref().unwrap().total, 3);

    // Config: /etc/sysctl.conf identical on 2 hosts => Only variant
    let config = merged.config.unwrap();
    let sysctl: Vec<_> = config
        .files
        .iter()
        .filter(|f| f.path == "/etc/sysctl.conf")
        .collect();
    assert_eq!(
        sysctl.len(),
        1,
        "identical configs should merge to one entry"
    );
    assert_eq!(sysctl[0].aggregate.as_ref().unwrap().count, 2);

    // Services: httpd on 2 hosts, crond on 1
    let services = merged.services.unwrap();
    let httpd_svc = services
        .state_changes
        .iter()
        .find(|s| s.unit == "httpd.service")
        .unwrap();
    assert_eq!(httpd_svc.aggregate.as_ref().unwrap().count, 2);
    assert_eq!(httpd_svc.aggregate.as_ref().unwrap().total, 3);

    let crond = services
        .state_changes
        .iter()
        .find(|s| s.unit == "crond.service")
        .unwrap();
    assert_eq!(crond.aggregate.as_ref().unwrap().count, 1);
    assert_eq!(crond.aggregate.as_ref().unwrap().total, 3);
}

// ===========================================================================
// E2E: Empty snapshot validation
// ===========================================================================

#[test]
fn test_e2e_validation_empty_snapshot_rejected() {
    // A truly empty snapshot: hostname in meta but no os_release, no sections.
    // is_empty_snapshot checks all section Options AND os_release.
    let mut s1 = InspectionSnapshot::new();
    s1.meta.insert(
        "hostname".to_string(),
        serde_json::Value::String("host-a".into()),
    );
    s1.meta.insert(
        "architecture".to_string(),
        serde_json::Value::String("x86_64".into()),
    );
    // No os_release, no rpm, no config, no services => empty

    let mut s2 = make_rich_snap("host-b", "9.4");
    with_rpms(&mut s2, &["httpd"]);

    let result = merge_snapshots(vec![s1, s2], None, None);
    assert!(
        result.is_err(),
        "empty snapshot should cause validation error"
    );
    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| matches!(
        e,
        AggregateValidationError::EmptySnapshot { hostname }
        if hostname == "host-a"
    )));
}
