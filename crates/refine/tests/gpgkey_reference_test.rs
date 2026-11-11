//! A key named with a dnf variable is the key its repo section asks for.
//!
//! EL10's `epel.repo` names its key `RPM-GPG-KEY-EPEL-$releasever_major`.
//! The scan records the key at the expanded path, `RPM-GPG-KEY-EPEL-10`,
//! together with the `gpgkey=` path that named it. Everything that asks
//! "is this section's key carried?" has to accept either, or a carried key
//! reads as missing: the Containerfile demotes `epel-release` behind a
//! FIXME, the audit report says the repo is not carried, and refine marks
//! the section Incomplete and cannot cascade an exclude to the key.
//!
//! The key entries are built from JSON, the snapshot contract, so the
//! fixture is exactly what the collector writes.

use std::collections::HashMap;

use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::FindingKind;
use inspectah_core::types::repo::RepoId;
use inspectah_core::types::rpm::{FileOwnershipEntry, PackageEntry, RepoFile, RpmSection};
use inspectah_pipeline::render;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, RefinementOp, RepoProvenance};

const REPO_PATH: &str = "/etc/yum.repos.d/epel.repo";
const KEY_PATH: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-10";
const KEY_REFERENCE: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-$releasever_major";

/// EPEL 10's `epel.repo`: three sections naming one key with a variable.
const EL10_EPEL_REPO: &str = "\
[epel]
name=Extra Packages for Enterprise Linux $releasever_major - $basearch
metalink=https://mirrors.fedoraproject.org/metalink?repo=epel-$releasever_major&arch=$basearch
enabled=1
gpgcheck=1
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-$releasever_major

[epel-debuginfo]
metalink=https://mirrors.fedoraproject.org/metalink?repo=epel-debug-$releasever_major&arch=$basearch
enabled=0
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-$releasever_major

[epel-source]
metalink=https://mirrors.fedoraproject.org/metalink?repo=epel-source-$releasever_major&arch=$basearch
enabled=0
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-$releasever_major
";

/// Two enabled sections naming the key with a variable. The cascade
/// leaves a key alone while any section naming it is still in, and a
/// section disabled on the host is never excluded, so this is the layout
/// in which excluding repos reaches the key.
const TWO_ENABLED_SECTIONS: &str = "\
[epel]
metalink=https://mirrors.fedoraproject.org/metalink?repo=epel-$releasever_major&arch=$basearch
enabled=1
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-$releasever_major

[epel-testing]
metalink=https://mirrors.fedoraproject.org/metalink?repo=epel-testing-$releasever_major&arch=$basearch
enabled=1
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-$releasever_major
";

/// The key entry the collector records for one section naming it.
fn collected_key() -> RepoFile {
    let mut key = serde_json::to_value(RepoFile {
        path: KEY_PATH.into(),
        content: "-----BEGIN PGP PUBLIC KEY BLOCK-----\nepel10\n".into(),
        disposition: FindingKind::included(),
        ..Default::default()
    })
    .expect("the key entry serializes");
    key["gpgkey_references"] = serde_json::json!([KEY_REFERENCE]);
    serde_json::from_value(key).expect("the key entry deserializes")
}

/// A baseline without the EPEL packages, so refine's classifier keeps
/// them included.
fn baseline() -> BaselineData {
    let mut packages = HashMap::new();
    packages.insert(
        "bash.x86_64".to_string(),
        BaselinePackageEntry {
            name: "bash".into(),
            epoch: Some("0".into()),
            version: "5.2".into(),
            release: "1.el10".into(),
            arch: "x86_64".into(),
        },
    );
    BaselineData {
        image_digest: "sha256:test".into(),
        packages,
        extracted_at: "2026-11-11T00:00:00Z".into(),
    }
}

/// `epel-release` installed `@commandline` and served by `[epel]`, with
/// `htop` from `[epel]` and `nethogs` from `[epel-testing]`. The collector
/// records one key entry per section that names the key.
fn snapshot(repo: &str) -> InspectionSnapshot {
    let package = |name: &str, source_repo: &str| PackageEntry {
        name: name.into(),
        version: "10".into(),
        release: "1.el10".into(),
        arch: "noarch".into(),
        source_repo: source_repo.into(),
        disposition: FindingKind::included(),
        ..Default::default()
    };
    let mut epel_release = package("epel-release", "@commandline");
    epel_release.repo_enabling_from = vec![RepoId::new("epel")];
    let key_entries = repo.matches("gpgkey=").count();

    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            epel_release,
            package("htop", "epel"),
            package("nethogs", "epel-testing"),
        ],
        repo_files: vec![RepoFile {
            path: REPO_PATH.into(),
            content: repo.into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        gpg_keys: vec![collected_key(); key_entries],
        file_ownership: vec![FileOwnershipEntry {
            package_name: "epel-release".into(),
            paths: vec![REPO_PATH.into(), KEY_PATH.into()],
        }],
        ..Default::default()
    });
    snap.baseline = Some(baseline());
    snap
}

fn scan_containerfile(snap: &InspectionSnapshot) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    render::render_all(snap, &RenderContext { target: None }, dir.path()).expect("render");
    std::fs::read_to_string(dir.path().join("Containerfile")).expect("read")
}

fn exclude_repo(session: &mut RefineSession, id: &str) {
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Repo { path: id.into() },
            include: false,
        })
        .unwrap_or_else(|e| panic!("repo {id} can be excluded: {e:?}"));
}

/// Each key entry's disposition, after projection.
fn key_dispositions(session: &RefineSession) -> Vec<bool> {
    session
        .snapshot_projected()
        .rpm
        .expect("rpm section")
        .gpg_keys
        .iter()
        .map(|k| k.disposition.is_included())
        .collect()
}

#[test]
fn a_key_named_with_a_dnf_variable_carries_its_repo_enabling_package() {
    let snap = snapshot(EL10_EPEL_REPO);

    let containerfile = scan_containerfile(&snap);
    assert!(
        containerfile.contains("# === Repo-enabling packages (1) ===")
            && containerfile.lines().any(|l| l.trim() == "epel-release \\"),
        "epel-release installs from the carried definition:\n{containerfile}"
    );
    assert!(
        !containerfile.contains("which this export does not carry"),
        "no false not-carried FIXME:\n{containerfile}"
    );

    let audit = render::audit::render_audit(&snap);
    assert!(
        audit.contains("| epel-release | epel | yes |"),
        "the audit report says the export carries it:\n{audit}"
    );

    let session = RefineSession::new(snap);
    assert_eq!(
        session.repo_index().provenance(&RepoId::new("epel")),
        RepoProvenance::Verified,
        "the key the section names was captured"
    );
}

#[test]
fn refine_cascades_an_exclude_to_a_key_named_with_a_dnf_variable() {
    let mut session = RefineSession::new(snapshot(TWO_ENABLED_SECTIONS));
    for id in ["epel", "epel-testing"] {
        assert_eq!(
            session.repo_index().provenance(&RepoId::new(id)),
            RepoProvenance::Verified,
            "{id}"
        );
    }

    exclude_repo(&mut session, "epel");
    assert_eq!(
        key_dispositions(&session),
        [true, true],
        "epel-testing still names the key"
    );

    exclude_repo(&mut session, "epel-testing");
    assert_eq!(
        key_dispositions(&session),
        [false, false],
        "no included section names the key, so every entry of it is excluded"
    );
}
