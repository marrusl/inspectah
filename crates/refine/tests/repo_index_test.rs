use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RepoFile, RpmSection};
use inspectah_refine::repo_index::RepoIndex;
use inspectah_refine::types::RepoProvenance;

pub fn make_snap_with_repos() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                include: true,
                ..Default::default()
            },
            PackageEntry {
                name: "epel-release".into(),
                arch: "noarch".into(),
                state: PackageState::Added,
                source_repo: "epel".into(),
                include: true,
                ..Default::default()
            },
        ],
        repo_files: vec![
            RepoFile {
                path: "/etc/yum.repos.d/centos.repo".into(),
                content: "[baseos]\nname=CentOS BaseOS\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial\n\n[appstream]\nname=CentOS AppStream\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial\n".into(),
                include: true,
                ..Default::default()
            },
            RepoFile {
                path: "/etc/yum.repos.d/epel.repo".into(),
                content: "[epel]\nname=EPEL 9\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9\n".into(),
                include: true,
                ..Default::default()
            },
        ],
        gpg_keys: vec![
            RepoFile {
                path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial".into(),
                content: "key-data".into(),
                include: true,
                ..Default::default()
            },
            RepoFile {
                path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9".into(),
                content: "key-data".into(),
                include: true,
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    snap
}

#[test]
fn test_repo_index_packages_by_repo() {
    let snap = make_snap_with_repos();
    let index = RepoIndex::build(&snap);
    assert_eq!(index.packages_by_repo.get("appstream").unwrap().len(), 1);
    assert_eq!(index.packages_by_repo.get("epel").unwrap().len(), 1);
}

#[test]
fn test_repo_index_multi_section_repo_file() {
    let snap = make_snap_with_repos();
    let index = RepoIndex::build(&snap);
    let baseos_files = index.repo_file_by_section.get("baseos").unwrap();
    let appstream_files = index.repo_file_by_section.get("appstream").unwrap();
    assert!(baseos_files.contains(&"/etc/yum.repos.d/centos.repo".to_string()));
    assert!(appstream_files.contains(&"/etc/yum.repos.d/centos.repo".to_string()));
}

#[test]
fn test_repo_index_gpg_shared_key() {
    let snap = make_snap_with_repos();
    let index = RepoIndex::build(&snap);
    let sections = index
        .sections_by_gpg_key
        .get("/etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial")
        .unwrap();
    assert!(sections.contains("baseos"));
    assert!(sections.contains("appstream"));
}

#[test]
fn test_repo_index_provenance_verified() {
    let snap = make_snap_with_repos();
    let index = RepoIndex::build(&snap);
    assert_eq!(index.provenance("appstream"), RepoProvenance::Verified);
    assert_eq!(index.provenance("epel"), RepoProvenance::Verified);
}

#[test]
fn test_repo_index_provenance_incomplete() {
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
            include: true,
            ..Default::default()
        });
    let index = RepoIndex::build(&snap);
    assert_eq!(
        index.provenance("custom-internal"),
        RepoProvenance::Incomplete
    );
}

#[test]
fn test_repo_index_provenance_unknown_empty_repo() {
    let index = RepoIndex::build(&make_snap_with_repos());
    assert_eq!(index.provenance(""), RepoProvenance::Unknown);
}
