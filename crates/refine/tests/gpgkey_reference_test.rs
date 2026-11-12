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

/// How many entries a snapshot records for a key several sections name.
#[derive(Debug, Clone, Copy)]
enum KeyEntries {
    /// The collector records each key path once.
    OnePerPath,
    /// Snapshots collected before that recorded one entry per section
    /// naming the key, and must still load and behave the same.
    OnePerSection,
}

/// `epel-release` installed `@commandline` and served by `[epel]`, with
/// `htop` from `[epel]` and `nethogs` from `[epel-testing]`.
fn snapshot(repo: &str, entries: KeyEntries) -> InspectionSnapshot {
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
    let key_entries = match entries {
        KeyEntries::OnePerPath => 1,
        KeyEntries::OnePerSection => repo.matches("gpgkey=").count(),
    };

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

fn include_repo(session: &mut RefineSession, id: &str) {
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Repo { path: id.into() },
            include: true,
        })
        .unwrap_or_else(|e| panic!("repo {id} can be included: {e:?}"));
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
    for entries in [KeyEntries::OnePerPath, KeyEntries::OnePerSection] {
        carries_its_repo_enabling_package(entries);
    }
}

fn carries_its_repo_enabling_package(entries: KeyEntries) {
    let snap = snapshot(EL10_EPEL_REPO, entries);

    let containerfile = scan_containerfile(&snap);
    assert!(
        containerfile.contains("# === Repo-enabling packages (1) ===")
            && containerfile.lines().any(|l| l.trim() == "epel-release \\"),
        "{entries:?}: epel-release installs from the carried definition:\n{containerfile}"
    );
    assert!(
        !containerfile.contains("which this export does not carry"),
        "{entries:?}: no false not-carried FIXME:\n{containerfile}"
    );

    let audit = render::audit::render_audit(&snap);
    assert!(
        audit.contains("| epel-release | epel | yes |"),
        "{entries:?}: the audit report says the export carries it:\n{audit}"
    );

    let session = RefineSession::new(snap);
    assert_eq!(
        session.repo_index().provenance(&RepoId::new("epel")),
        RepoProvenance::Verified,
        "{entries:?}: the key the section names was captured"
    );
}

#[test]
fn refine_cascades_an_exclude_to_a_key_named_with_a_dnf_variable() {
    for (entries, count) in [(KeyEntries::OnePerPath, 1), (KeyEntries::OnePerSection, 2)] {
        let mut session = RefineSession::new(snapshot(TWO_ENABLED_SECTIONS, entries));
        for id in ["epel", "epel-testing"] {
            assert_eq!(
                session.repo_index().provenance(&RepoId::new(id)),
                RepoProvenance::Verified,
                "{entries:?}: {id}"
            );
        }

        exclude_repo(&mut session, "epel");
        assert_eq!(
            key_dispositions(&session),
            vec![true; count],
            "{entries:?}: epel-testing still names the key"
        );

        exclude_repo(&mut session, "epel-testing");
        assert_eq!(
            key_dispositions(&session),
            vec![false; count],
            "{entries:?}: no included section names the key, so every entry of it is excluded"
        );

        include_repo(&mut session, "epel");
        assert_eq!(
            key_dispositions(&session),
            vec![true; count],
            "{entries:?}: a section naming the key is back in, so every entry of it is"
        );
    }
}

const STOCK_KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-redhat-release";
const STOCK_REASON: &str =
    "the base image ships it (redhat-release, unmodified on the source host)";

/// RHEL's `redhat.repo`: two enabled sections naming the stock key.
const REDHAT_REPO: &str = "\
[rhel-10-baseos]
baseurl=https://cdn.redhat.com/content/dist/rhel10/10/aarch64/baseos/os
enabled=1
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-redhat-release

[rhel-10-appstream]
baseurl=https://cdn.redhat.com/content/dist/rhel10/10/aarch64/appstream/os
enabled=1
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-redhat-release
";

/// A site repo signed with the Red Hat key, which refine can toggle;
/// distro repos it cannot.
const CORP_REPO: &str = "\
[corp-a]
baseurl=https://repo.example.com/a
enabled=1
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-redhat-release

[corp-b]
baseurl=https://repo.example.com/b
enabled=1
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-redhat-release
";

/// The EPEL snapshot plus `redhat.repo`, a site repo and the stock key, which the
/// scan left out because the base image ships it. `corp-release` is
/// repo-enabling for the site repo and owns the stock key's path, so its
/// own files include a key the export does not copy.
fn snapshot_with_stock_key() -> InspectionSnapshot {
    let mut snap = snapshot(EL10_EPEL_REPO, KeyEntries::OnePerPath);
    let rpm = snap.rpm.as_mut().expect("rpm section");
    rpm.repo_files.push(RepoFile {
        path: "/etc/yum.repos.d/redhat.repo".into(),
        content: REDHAT_REPO.into(),
        is_default_repo: true,
        disposition: FindingKind::excluded(),
        ..Default::default()
    });
    for (name, repo) in [("corp-tool", "corp-a"), ("corp-agent", "corp-b")] {
        rpm.packages_added.push(PackageEntry {
            name: name.into(),
            version: "1".into(),
            release: "1".into(),
            arch: "noarch".into(),
            source_repo: repo.into(),
            disposition: FindingKind::included(),
            ..Default::default()
        });
    }
    rpm.packages_added.push(PackageEntry {
        name: "corp-release".into(),
        version: "1".into(),
        release: "1".into(),
        arch: "noarch".into(),
        source_repo: "@commandline".into(),
        repo_enabling_from: vec![RepoId::new("corp-a")],
        disposition: FindingKind::included(),
        ..Default::default()
    });
    rpm.file_ownership.push(FileOwnershipEntry {
        package_name: "corp-release".into(),
        paths: vec!["/etc/yum.repos.d/corp.repo".into(), STOCK_KEY.into()],
    });
    rpm.repo_files.push(RepoFile {
        path: "/etc/yum.repos.d/corp.repo".into(),
        content: CORP_REPO.into(),
        disposition: FindingKind::included(),
        ..Default::default()
    });
    rpm.gpg_keys.push(RepoFile {
        path: STOCK_KEY.into(),
        content: "-----BEGIN PGP PUBLIC KEY BLOCK-----\nrh\n".into(),
        disposition: FindingKind::excluded(),
        shipped_by_base: Some("redhat-release".into()),
        ..Default::default()
    });
    snap
}

fn stock_key(session: &RefineSession) -> RepoFile {
    session
        .snapshot_projected()
        .rpm
        .expect("rpm section")
        .gpg_keys
        .into_iter()
        .find(|k| k.path == STOCK_KEY)
        .expect("stock key recorded")
}

/// A key the base image ships is not copied, and every surface that shows
/// keys says so and why. A repo-enabling package that owns the key still
/// installs by name, since the image has the file. Including a repo that
/// names it again does not copy it: the base image's own file is the one
/// the image should keep.
#[test]
fn a_key_the_base_image_ships_is_shown_but_not_copied() {
    let snap = snapshot_with_stock_key();

    let containerfile = scan_containerfile(&snap);
    assert!(
        containerfile.contains("# === GPG Keys (1) ==="),
        "only EPEL's key is copied:\n{containerfile}"
    );
    assert!(
        containerfile
            .lines()
            .any(|l| l.starts_with('#') && l.contains(STOCK_KEY) && l.contains(STOCK_REASON)),
        "the Containerfile names the key it leaves to the base image:\n{containerfile}"
    );
    assert!(
        containerfile.contains("# === Repo-enabling packages (2) ===")
            && containerfile.lines().any(|l| l.trim() == "epel-release \\")
            && containerfile.lines().any(|l| l.trim() == "corp-release \\"),
        "EPEL and corp-release install by name:\n{containerfile}"
    );
    assert!(
        !containerfile.contains("which this export does not carry"),
        "a key the base image ships is not a dropped file:\n{containerfile}"
    );

    let audit = render::audit::render_audit(&snap);
    assert!(
        audit.contains("| corp-release | corp-a | yes |"),
        "the audit report says the export carries corp-release's repo:\n{audit}"
    );
    assert!(
        audit.contains(&format!("| {STOCK_KEY} | {STOCK_REASON} |")),
        "the audit report names the key and the reason:\n{audit}"
    );

    let mut session = RefineSession::new(snap);
    assert_eq!(
        session
            .repo_index()
            .provenance(&RepoId::new("rhel-10-baseos")),
        RepoProvenance::Verified,
        "the stock key was captured"
    );
    exclude_repo(&mut session, "corp-a");
    exclude_repo(&mut session, "corp-b");
    include_repo(&mut session, "corp-a");
    let key = stock_key(&session);
    assert!(
        !key.disposition.is_included() && key.shipped_by_base.as_deref() == Some("redhat-release"),
        "re-including a repo leaves the base image's key alone: {key:?}"
    );
}
