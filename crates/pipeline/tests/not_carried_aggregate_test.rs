//! The scan defaults never reach an aggregate.
//!
//! `apply_scan_package_defaults` excludes auto packages nothing included
//! needs, but the aggregate merge keeps only leaves in `packages_added`
//! and normalizes every disposition, so a fleet merged from hosts with
//! the defaults applied must match one merged from the same hosts
//! without them.

use inspectah_core::aggregate::merge_snapshots;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::rpm::{PackageEntry, RpmSection};
use inspectah_pipeline::render::not_carried::apply_scan_package_defaults;

fn pkg(name: &str, arch: &str) -> PackageEntry {
    PackageEntry {
        name: name.into(),
        arch: arch.into(),
        source_repo: "appstream".into(),
        disposition: FindingKind::included(),
        ..Default::default()
    }
}

/// A host with an included leaf, an auto package it pulls in, and auto
/// packages nothing pulls in. `extra_leaf` is a leaf only this host has.
fn host(hostname: &str, extra_leaf: &str) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot {
        rpm: Some(RpmSection {
            packages_added: vec![
                pkg("httpd", "x86_64"),
                pkg(extra_leaf, "x86_64"),
                pkg("apr", "x86_64"),
                pkg("firewalld", "noarch"),
                pkg("python3-firewall", "noarch"),
            ],
            leaf_packages: Some(vec!["httpd.x86_64".into(), format!("{extra_leaf}.x86_64")]),
            auto_packages: Some(vec![
                "apr.x86_64".into(),
                "firewalld.noarch".into(),
                "python3-firewall.noarch".into(),
            ]),
            leaf_dep_tree: serde_json::json!({ "httpd.x86_64": ["apr.x86_64"] }),
            ..Default::default()
        }),
        ..Default::default()
    };
    snap.meta.insert("hostname".into(), hostname.into());
    snap
}

fn packages_added(snap: &InspectionSnapshot) -> &[PackageEntry] {
    &snap.rpm.as_ref().expect("rpm section").packages_added
}

#[test]
fn aggregate_merge_is_the_same_with_or_without_scan_defaults() {
    let raw = vec![host("web1", "mod_ssl"), host("web2", "tmux")];
    let mut defaulted = raw.clone();
    apply_scan_package_defaults(&mut defaulted[0]);
    let excluded: Vec<String> = packages_added(&defaulted[0])
        .iter()
        .filter(|p| !p.disposition.is_included())
        .map(|p| format!("{}.{}", p.name, p.arch))
        .collect();
    assert_eq!(
        excluded,
        ["firewalld.noarch", "python3-firewall.noarch"],
        "precondition: the defaults exclude web1's unneeded auto packages"
    );

    let (from_raw, _) = merge_snapshots(raw, Some("fleet"), None).expect("raw hosts merge");
    let (from_defaulted, _) =
        merge_snapshots(defaulted, Some("fleet"), None).expect("defaulted hosts merge");

    assert_eq!(packages_added(&from_defaulted), packages_added(&from_raw));
    let dispositions = |snap: &InspectionSnapshot| -> Vec<(String, FindingKind)> {
        packages_added(snap)
            .iter()
            .map(|p| (format!("{}.{}", p.name, p.arch), p.disposition.clone()))
            .collect()
    };
    assert_eq!(dispositions(&from_defaulted), dispositions(&from_raw));
    assert!(
        !dispositions(&from_raw).is_empty(),
        "the merge keeps the leaves"
    );
}
