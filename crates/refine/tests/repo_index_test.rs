use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::repo::RepoId;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RepoFile, RpmSection};
use inspectah_refine::repo_index::RepoIndex;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, RefinementOp, RepoProvenance};

pub fn make_snap_with_repos() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "epel-release".into(),
                arch: "noarch".into(),
                state: PackageState::Added,
                source_repo: "epel".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        repo_files: vec![
            RepoFile {
                path: "/etc/yum.repos.d/centos.repo".into(),
                content: "[baseos]\nname=CentOS BaseOS\nbaseurl=http://example.invalid/baseos\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial\n\n[appstream]\nname=CentOS AppStream\nbaseurl=http://example.invalid/appstream\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial\n".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            RepoFile {
                path: "/etc/yum.repos.d/epel.repo".into(),
                content: "[epel]\nname=EPEL 9\nbaseurl=http://example.invalid/epel\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9\n".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        gpg_keys: vec![
            RepoFile {
                path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial".into(),
                content: "key-data".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            RepoFile {
                path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9".into(),
                content: "key-data".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    snap
}

/// Build a `RepoIndex` from one repo file (with `.repo`-style `content`) and
/// a list of `(name, arch, source_repo)` packages.
fn index_with(content: &str, packages: &[(&str, &str, &str)]) -> RepoIndex {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: packages
            .iter()
            .map(|(name, arch, source_repo)| PackageEntry {
                name: (*name).into(),
                arch: (*arch).into(),
                state: PackageState::Added,
                source_repo: (*source_repo).into(),
                disposition: FindingKind::included(),
                ..Default::default()
            })
            .collect(),
        repo_files: vec![RepoFile {
            path: "/etc/yum.repos.d/test.repo".into(),
            content: content.into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    RepoIndex::build(&snap)
}

#[test]
fn test_repo_index_packages_by_repo() {
    let snap = make_snap_with_repos();
    let index = RepoIndex::build(&snap);
    assert_eq!(
        index
            .packages_by_repo()
            .get(&RepoId::new("appstream"))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        index
            .packages_by_repo()
            .get(&RepoId::new("epel"))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn test_repo_index_multi_section_repo_file() {
    let snap = make_snap_with_repos();
    let index = RepoIndex::build(&snap);
    let baseos_files = index
        .repo_file_by_section()
        .get(&RepoId::new("baseos"))
        .unwrap();
    let appstream_files = index
        .repo_file_by_section()
        .get(&RepoId::new("appstream"))
        .unwrap();
    assert!(baseos_files.contains(&"/etc/yum.repos.d/centos.repo".to_string()));
    assert!(appstream_files.contains(&"/etc/yum.repos.d/centos.repo".to_string()));
}

#[test]
fn test_repo_index_gpg_shared_key() {
    let snap = make_snap_with_repos();
    let index = RepoIndex::build(&snap);
    let sections = index
        .sections_by_gpg_key()
        .get("/etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial")
        .unwrap();
    assert!(sections.contains(&RepoId::new("baseos")));
    assert!(sections.contains(&RepoId::new("appstream")));
}

#[test]
fn test_repo_index_provenance_verified() {
    let snap = make_snap_with_repos();
    let index = RepoIndex::build(&snap);
    assert_eq!(
        index.provenance(&RepoId::new("appstream")),
        RepoProvenance::Verified
    );
    assert_eq!(
        index.provenance(&RepoId::new("epel")),
        RepoProvenance::Verified
    );
}

#[test]
fn test_repo_index_provenance_stale_when_referenced_without_section() {
    let mut snap = make_snap_with_repos();
    snap.rpm
        .as_mut()
        .unwrap()
        .packages_added
        .push(PackageEntry {
            name: "custom-pkg".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: "custom-internal".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        });
    let index = RepoIndex::build(&snap);
    assert_eq!(
        index.provenance(&RepoId::new("custom-internal")),
        RepoProvenance::Stale
    );
}

#[test]
fn test_repo_index_provenance_unknown_empty_repo() {
    let index = RepoIndex::build(&make_snap_with_repos());
    assert_eq!(index.provenance(&RepoId::new("")), RepoProvenance::Unknown);
}

#[test]
fn case_split_collapses_to_one_verified_key() {
    // § 8.5: [EPEL_10_Everything_x86_64] in the file, "epel_10_everything_x86_64" on the package.
    let idx = index_with(
        "[EPEL_10_Everything_x86_64]\nname=EPEL 10 Everything\nbaseurl=http://example.invalid/epel10\n",
        &[("some-pkg", "x86_64", "epel_10_everything_x86_64")],
    );
    assert_eq!(
        idx.provenance(&RepoId::new("EPEL_10_Everything_x86_64")),
        RepoProvenance::Verified
    );
    assert_eq!(
        idx.provenance(&RepoId::new("epel_10_everything_x86_64")),
        RepoProvenance::Verified
    );
    assert_eq!(idx.packages_by_repo().len(), 1);
}

#[test]
fn disabled_section_reports_disabled() {
    let idx = index_with(
        "[EPEL_10_Everything_x86_64]\nname=EPEL 10 Everything\nenabled=0\nbaseurl=http://example.invalid/epel10\n",
        &[("some-pkg", "x86_64", "epel_10_everything_x86_64")],
    );
    assert_eq!(
        idx.provenance(&RepoId::new("epel_10_everything_x86_64")),
        RepoProvenance::Disabled
    );
}

#[test]
fn validate_target_accepts_incomplete_and_refuses_the_rest() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "a".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "incomplete-repo".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "b".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "disabled-repo".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "c".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "stale-repo".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        repo_files: vec![RepoFile {
            path: "/etc/yum.repos.d/test.repo".into(),
            // incomplete-repo: present, enabled, no baseurl/metalink/mirrorlist.
            // disabled-repo: present, has a source, but enabled = 0.
            // stale-repo has no section here at all.
            content: "[incomplete-repo]\nname=Incomplete\n\n[disabled-repo]\nname=Disabled\nenabled=0\nbaseurl=http://example.invalid/disabled\n".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });

    let mut session = RefineSession::new(snap);

    // Incomplete -> Ok.
    assert!(
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Repo {
                    path: "incomplete-repo".into(),
                },
                include: false,
            })
            .is_ok(),
        "Incomplete provenance should be accepted"
    );

    // Disabled -> Err(BadRequest), message names the provenance.
    let err = session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Repo {
                path: "disabled-repo".into(),
            },
            include: false,
        })
        .unwrap_err();
    assert!(format!("{err}").contains("Disabled"));

    // Stale (referenced by a package, no section on the host) -> Err(BadRequest).
    let err = session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Repo {
                path: "stale-repo".into(),
            },
            include: false,
        })
        .unwrap_err();
    assert!(format!("{err}").contains("Stale"));

    // Unknown (pseudo repo) -> Err(BadRequest).
    let err = session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Repo {
                path: "@commandline".into(),
            },
            include: false,
        })
        .unwrap_err();
    assert!(format!("{err}").contains("Unknown"));
}
