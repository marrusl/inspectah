use inspectah_core::baseline::BaselineData;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RepoFile, RpmSection};

fn empty_baseline() -> BaselineData {
    BaselineData {
        image_digest: "sha256:test".into(),
        packages: std::collections::HashMap::new(),
        extracted_at: "2026-05-17T00:00:00Z".into(),
    }
}

pub fn make_snap_with_repos() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.baseline = Some(empty_baseline());
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
                content: "[baseos]\nname=CentOS BaseOS\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial\n\n[appstream]\nname=CentOS AppStream\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial\n".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            RepoFile {
                path: "/etc/yum.repos.d/epel.repo".into(),
                content: "[epel]\nname=EPEL 9\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9\n".into(),
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
        baseline_package_names: Some(vec![]),
        ..Default::default()
    });
    snap
}

pub fn make_snap_with_multi_section_third_party() -> InspectionSnapshot {
    let mut snap = make_snap_with_repos();
    let rpm = snap.rpm.as_mut().unwrap();
    rpm.repo_files.push(RepoFile {
        path: "/etc/yum.repos.d/custom-multi.repo".into(),
        content: "[custom-a]\nname=Custom A\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-custom\n\n[custom-b]\nname=Custom B\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-custom\n".into(),
        disposition: FindingKind::included(),
        ..Default::default()
    });
    rpm.gpg_keys.push(RepoFile {
        path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-custom".into(),
        content: "key-data".into(),
        disposition: FindingKind::included(),
        ..Default::default()
    });
    rpm.packages_added.push(PackageEntry {
        name: "pkg-a".into(),
        arch: "x86_64".into(),
        state: PackageState::Added,
        source_repo: "custom-a".into(),
        disposition: FindingKind::included(),
        ..Default::default()
    });
    rpm.packages_added.push(PackageEntry {
        name: "pkg-b".into(),
        arch: "x86_64".into(),
        state: PackageState::Added,
        source_repo: "custom-b".into(),
        disposition: FindingKind::included(),
        ..Default::default()
    });
    snap
}
