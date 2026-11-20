//! Excluding a repo drops its `.repo` file and its GPG key from the export.
//!
//! EPEL's `epel.repo` holds `[epel]`, enabled, beside `[epel-debuginfo]`
//! and `[epel-source]`, which the host itself disabled, and all three name
//! the same key. A section the host disabled is not something the image
//! uses, so it does not keep the file or the key: once every host-enabled
//! section in a file is excluded the file is not carried, and a key is
//! carried only while a carried, host-enabled section names it. Everything
//! that reports the file or the key (the staged config tree, the
//! Containerfile, the audit report) reads that one decision from the
//! projected snapshot.

use std::collections::HashMap;
use std::path::Path;

use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::FindingKind;
use inspectah_core::types::repo::RepoId;
use inspectah_core::types::rpm::{FileOwnershipEntry, PackageEntry, RepoFile, RpmSection};
use inspectah_pipeline::render;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, RefinementOp};

const EPEL_REPO_PATH: &str = "/etc/yum.repos.d/epel.repo";
const EPEL_TESTING_REPO_PATH: &str = "/etc/yum.repos.d/epel-testing.repo";
const CORP_REPO_PATH: &str = "/etc/yum.repos.d/corp.repo";
/// EPEL's key where dnf and rpm look for keys by default.
const STANDARD_KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9";
/// The same key outside `/etc/pki/rpm-gpg/`, which the Containerfile
/// imports with its own `rpm --import` line.
const NONSTANDARD_KEY: &str = "/etc/pki/custom/RPM-GPG-KEY-EPEL-9";
const CORP_KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-corp";

/// `epel.repo` as EPEL 9 ships it: one enabled section and two the host
/// disabled, all naming `key`.
fn epel_repo(key: &str) -> String {
    format!(
        "\
[epel]
name=Extra Packages for Enterprise Linux 9 - $basearch
metalink=https://mirrors.fedoraproject.org/metalink?repo=epel-9&arch=$basearch
enabled=1
gpgcheck=1
gpgkey=file://{key}

[epel-debuginfo]
metalink=https://mirrors.fedoraproject.org/metalink?repo=epel-debug-9&arch=$basearch
enabled=0
gpgkey=file://{key}

[epel-source]
metalink=https://mirrors.fedoraproject.org/metalink?repo=epel-source-9&arch=$basearch
enabled=0
gpgkey=file://{key}
"
    )
}

/// `epel-testing.repo` with its section enabled, naming the same key as
/// `epel.repo`.
fn epel_testing_repo(key: &str) -> String {
    format!(
        "\
[epel-testing]
metalink=https://mirrors.fedoraproject.org/metalink?repo=testing-epel9&arch=$basearch
enabled=1
gpgkey=file://{key}

[epel-testing-source]
metalink=https://mirrors.fedoraproject.org/metalink?repo=testing-source-epel9&arch=$basearch
enabled=0
gpgkey=file://{key}
"
    )
}

/// A site repo file with two enabled sections and its own key.
const CORP_REPO: &str = "\
[corp-a]
baseurl=https://repo.example.com/a
enabled=1
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-corp

[corp-b]
baseurl=https://repo.example.com/b
enabled=1
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-corp
";

/// A repo file whose only section the host disabled, and the key it
/// alone names.
const DISABLED_REPO_PATH: &str = "/etc/yum.repos.d/corp-testing.repo";
const DISABLED_ONLY_KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-corp-testing";
const DISABLED_REPO: &str = "\
[corp-testing]
baseurl=https://repo.example.com/testing
enabled=0
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-corp-testing
";

/// A baseline without any of the fixture's packages, so refine's
/// classifier keeps them included.
fn baseline() -> BaselineData {
    let mut packages = HashMap::new();
    packages.insert(
        "bash.x86_64".to_string(),
        BaselinePackageEntry {
            name: "bash".into(),
            epoch: Some("0".into()),
            version: "5.1".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
        },
    );
    BaselineData {
        image_digest: "sha256:test".into(),
        packages,
        extracted_at: "2026-11-12T00:00:00Z".into(),
    }
}

fn package(name: &str, source_repo: &str) -> PackageEntry {
    PackageEntry {
        name: name.into(),
        version: "1".into(),
        release: "1.el9".into(),
        arch: "noarch".into(),
        source_repo: source_repo.into(),
        disposition: FindingKind::included(),
        ..Default::default()
    }
}

fn repo_file(path: &str, content: &str) -> RepoFile {
    RepoFile {
        path: path.into(),
        content: content.into(),
        disposition: FindingKind::included(),
        ..Default::default()
    }
}

fn key(path: &str) -> RepoFile {
    repo_file(path, "-----BEGIN PGP PUBLIC KEY BLOCK-----\nkey\n")
}

/// An EPEL host: `epel-release` from `epel_release_repo` and served by
/// `[epel]`, `htop` from `[epel]`, and the site repo with `corp-tool` from
/// `[corp-a]` and `corp-agent` from `[corp-b]`.
fn epel_host(epel_key: &str, epel_release_repo: &str) -> InspectionSnapshot {
    let mut epel_release = package("epel-release", epel_release_repo);
    epel_release.repo_enabling_from = vec![RepoId::new("epel")];
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            epel_release,
            package("htop", "epel"),
            package("corp-tool", "corp-a"),
            package("corp-agent", "corp-b"),
        ],
        repo_files: vec![
            repo_file(EPEL_REPO_PATH, &epel_repo(epel_key)),
            repo_file(CORP_REPO_PATH, CORP_REPO),
        ],
        gpg_keys: vec![key(epel_key), key(CORP_KEY)],
        file_ownership: vec![FileOwnershipEntry {
            package_name: "epel-release".into(),
            paths: vec![EPEL_REPO_PATH.into(), epel_key.into()],
        }],
        ..Default::default()
    });
    snap.baseline = Some(baseline());
    snap
}

fn set_repo(session: &mut RefineSession, id: &str, include: bool) {
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Repo { path: id.into() },
            include,
        })
        .unwrap_or_else(|e| panic!("repo {id} can be set to {include}: {e:?}"));
}

/// Whether the projected snapshot carries the repo file or key at `path`.
fn carried(session: &RefineSession, path: &str) -> bool {
    let rpm = session.snapshot_projected().rpm.expect("rpm section");
    let entries: Vec<&RepoFile> = rpm
        .repo_files
        .iter()
        .chain(rpm.gpg_keys.iter())
        .filter(|f| f.path == path)
        .collect();
    assert!(!entries.is_empty(), "{path} is recorded");
    entries.iter().any(|f| f.disposition.is_included())
}

/// What one export of the session writes.
struct Export {
    containerfile: String,
    audit: String,
    dir: tempfile::TempDir,
}

impl Export {
    fn of(session: &RefineSession) -> Self {
        let snap = session.snapshot_projected();
        let dir = tempfile::tempdir().expect("tempdir");
        render::render_all(&snap, &RenderContext { target: None }, dir.path()).expect("render");
        let containerfile =
            std::fs::read_to_string(dir.path().join("Containerfile")).expect("Containerfile");
        let audit = std::fs::read_to_string(dir.path().join("audit-report.md")).expect("audit");
        Self {
            containerfile,
            audit,
            dir,
        }
    }

    /// Whether the config tree stages the file at host path `path`.
    fn stages(&self, path: &str) -> bool {
        self.dir
            .path()
            .join("config")
            .join(path.trim_start_matches('/'))
            .is_file()
    }

    fn imports(&self, key: &str) -> bool {
        self.containerfile
            .lines()
            .any(|l| l.trim() == format!("RUN rpm --import {key}"))
    }
}

/// Excluding `epel` drops `epel.repo` and EPEL's key from every surface,
/// and re-including it brings both back.
#[test]
fn excluding_a_repo_drops_its_file_and_key_despite_host_disabled_sections() {
    struct Case {
        name: &'static str,
        key: &'static str,
        /// Where `epel-release` came from: `epel` excludes it with the
        /// repo, `@commandline` keeps it and demotes it.
        epel_release_repo: &'static str,
    }
    let cases = [
        Case {
            name: "standard key dir, epel-release from epel",
            key: STANDARD_KEY,
            epel_release_repo: "epel",
        },
        Case {
            name: "non-standard key dir, epel-release from epel",
            key: NONSTANDARD_KEY,
            epel_release_repo: "epel",
        },
        Case {
            name: "standard key dir, epel-release from the command line",
            key: STANDARD_KEY,
            epel_release_repo: "@commandline",
        },
    ];

    for case in cases {
        let name = case.name;
        let mut session = RefineSession::new(epel_host(case.key, case.epel_release_repo));
        // Refine leaves a package of unknown provenance out until the
        // operator takes it; this one the operator took.
        if case.epel_release_repo == "@commandline" {
            session
                .apply(RefinementOp::SetInclude {
                    item_id: ItemId::Package {
                        name: "epel-release".into(),
                        arch: "noarch".into(),
                    },
                    include: true,
                })
                .expect("epel-release can be included");
        }

        let before = Export::of(&session);
        assert!(
            before.stages(EPEL_REPO_PATH) && before.stages(case.key),
            "{name}: the scan's export carries epel.repo and its key"
        );
        assert!(
            before
                .containerfile
                .contains("# === Custom Repositories (2) ==="),
            "{name}:\n{}",
            before.containerfile
        );
        assert_eq!(
            before.imports(case.key),
            case.key == NONSTANDARD_KEY,
            "{name}: a key outside the standard dir is imported:\n{}",
            before.containerfile
        );

        set_repo(&mut session, "epel", false);

        assert!(!carried(&session, EPEL_REPO_PATH), "{name}: epel.repo");
        assert!(!carried(&session, case.key), "{name}: EPEL's key");
        assert!(
            carried(&session, CORP_REPO_PATH) && carried(&session, CORP_KEY),
            "{name}: the site repo is untouched"
        );

        let after = Export::of(&session);
        assert!(
            !after.stages(EPEL_REPO_PATH) && !after.stages(case.key),
            "{name}: neither epel.repo nor its key is staged"
        );
        assert!(
            after.stages(CORP_REPO_PATH) && after.stages(CORP_KEY),
            "{name}"
        );
        let cf = &after.containerfile;
        assert!(
            cf.contains("# === Custom Repositories (1) ===")
                && cf.contains("# === GPG Keys (1) ==="),
            "{name}: only the site repo and its key are copied:\n{cf}"
        );
        assert!(!after.imports(case.key), "{name}: no rpm --import:\n{cf}");
        assert!(
            !cf.lines()
                .any(|l| l.contains("FIXME") && l.contains("GPG key")),
            "{name}: no key FIXME:\n{cf}"
        );
        // `corp-a` and `corp-b` stay carried with their key, so nothing
        // that installs from them is demoted.
        assert!(
            cf.lines().any(|l| l.contains("corp-tool"))
                && cf.lines().any(|l| l.contains("corp-agent")),
            "{name}: the site repo's packages still install:\n{cf}"
        );

        match case.epel_release_repo {
            "epel" => {
                // Excluded with its repo: nothing claims a definition is
                // missing, and the audit has no repo-enabling row for it.
                assert!(
                    !cf.contains("which this export does not carry"),
                    "{name}: no not-carried FIXME:\n{cf}"
                );
                assert!(
                    !after.audit.contains("| epel-release |"),
                    "{name}: the audit report agrees:\n{}",
                    after.audit
                );
            }
            _ => {
                // Kept by the operator: the export does not carry the
                // definition it installs from, and both surfaces say so.
                assert!(
                    cf.contains(
                        "epel-release installs from repo epel, whose definition \
                         this export does not carry"
                    ),
                    "{name}: the Containerfile names the loss:\n{cf}"
                );
                assert!(
                    after.audit.contains("| epel-release | epel | no |"),
                    "{name}: the audit report agrees:\n{}",
                    after.audit
                );
            }
        }

        set_repo(&mut session, "epel", true);

        assert!(
            carried(&session, EPEL_REPO_PATH),
            "{name}: epel.repo is back"
        );
        assert!(carried(&session, case.key), "{name}: EPEL's key is back");
        let back = Export::of(&session);
        assert!(
            back.stages(EPEL_REPO_PATH) && back.stages(case.key),
            "{name}: both are staged again"
        );
        assert_eq!(
            back.containerfile, before.containerfile,
            "{name}: re-including restores the scan's Containerfile"
        );
        assert_eq!(back.audit, before.audit, "{name}: and its audit report");
    }
}

/// A file stays carried while any of its host-enabled sections is still
/// included, whatever the host disabled.
#[test]
fn a_file_stays_while_one_of_its_enabled_sections_is_included() {
    let mut session = RefineSession::new(epel_host(STANDARD_KEY, "epel"));

    set_repo(&mut session, "corp-a", false);
    assert!(carried(&session, CORP_REPO_PATH), "corp-b still uses it");
    assert!(carried(&session, CORP_KEY), "corp-b still names the key");

    set_repo(&mut session, "corp-b", false);
    assert!(
        !carried(&session, CORP_REPO_PATH),
        "no enabled section left"
    );
    assert!(!carried(&session, CORP_KEY), "no carried section names it");

    set_repo(&mut session, "corp-a", true);
    assert!(carried(&session, CORP_REPO_PATH));
    assert!(carried(&session, CORP_KEY));
}

/// A key two repo files name stays while either of them is carried, and
/// goes with the second.
#[test]
fn a_key_named_by_two_repos_stays_while_either_is_carried() {
    for (first, second) in [("epel", "epel-testing"), ("epel-testing", "epel")] {
        let mut snap = epel_host(STANDARD_KEY, "epel");
        snap.rpm
            .as_mut()
            .expect("rpm section")
            .repo_files
            .push(repo_file(
                EPEL_TESTING_REPO_PATH,
                &epel_testing_repo(STANDARD_KEY),
            ));
        snap.rpm
            .as_mut()
            .expect("rpm section")
            .packages_added
            .push(package("nethogs", "epel-testing"));
        let mut session = RefineSession::new(snap);
        let file_of = |id: &str| match id {
            "epel" => EPEL_REPO_PATH,
            _ => EPEL_TESTING_REPO_PATH,
        };

        set_repo(&mut session, first, false);
        assert!(!carried(&session, file_of(first)), "{first} excluded");
        assert!(carried(&session, file_of(second)), "{second} untouched");
        assert!(
            carried(&session, STANDARD_KEY),
            "{second} still names the key"
        );
        assert!(Export::of(&session).stages(STANDARD_KEY), "{first} first");

        set_repo(&mut session, second, false);
        assert!(!carried(&session, file_of(second)), "{second} excluded");
        assert!(!carried(&session, STANDARD_KEY), "no carried repo names it");
        assert!(!Export::of(&session).stages(STANDARD_KEY), "{first} first");

        set_repo(&mut session, first, true);
        assert!(carried(&session, file_of(first)), "{first} back");
        assert!(!carried(&session, file_of(second)), "{second} stays out");
        assert!(carried(&session, STANDARD_KEY), "{first} names it again");
    }
}

/// With no operator decision, refine's projection is the scan's export:
/// the rule only acts on an exclusion. That includes a file whose
/// sections the host all disabled and a key only that file names, which
/// the rule alone would not count as carried.
#[test]
fn no_exclusion_changes_nothing() {
    let mut snap = epel_host(STANDARD_KEY, "epel");
    let rpm = snap.rpm.as_mut().expect("rpm section");
    rpm.repo_files
        .push(repo_file(DISABLED_REPO_PATH, DISABLED_REPO));
    rpm.gpg_keys.push(key(DISABLED_ONLY_KEY));
    let scan_dir = tempfile::tempdir().expect("tempdir");
    render::render_all(&snap, &RenderContext { target: None }, scan_dir.path()).expect("render");
    let session = RefineSession::new(snap);
    let export = Export::of(&session);
    let scan = std::fs::read_to_string(scan_dir.path().join("Containerfile")).expect("read");
    assert_eq!(export.containerfile, scan);
    for path in [
        EPEL_REPO_PATH,
        STANDARD_KEY,
        CORP_REPO_PATH,
        CORP_KEY,
        DISABLED_REPO_PATH,
        DISABLED_ONLY_KEY,
    ] {
        assert!(
            Path::new(
                &scan_dir
                    .path()
                    .join("config")
                    .join(path.trim_start_matches('/'))
            )
            .is_file(),
            "{path}"
        );
        assert!(export.stages(path), "{path}");
    }
}

/// Whether the Containerfile names `name` as a `dnf install` operand.
fn installs(containerfile: &str, name: &str) -> bool {
    containerfile
        .lines()
        .any(|l| l.trim_end_matches(" \\").trim() == name)
}

/// `[site]` and `[SITE]` are two repos to dnf, each in its own file with
/// its own key and its own package. Excluding one spelling leaves the
/// other's package, file and key carried and its package installed; the
/// exclusion never reaches across a case variant. Each spelling is
/// excluded in turn, with the files in both orders.
#[test]
fn excluding_one_case_variant_of_a_repo_keeps_the_other() {
    // Paths that differ in more than case: the stager treats two
    // spellings of one path as one file.
    const LOWER_PATH: &str = "/etc/yum.repos.d/site-lower.repo";
    const UPPER_PATH: &str = "/etc/yum.repos.d/site-upper.repo";
    const LOWER_KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-site-lower";
    const UPPER_KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-site-upper";
    let lower_repo = format!(
        "[site]\nbaseurl=https://repo.example.com/lower\nenabled=1\ngpgkey=file://{LOWER_KEY}\n"
    );
    let upper_repo = format!(
        "[SITE]\nbaseurl=https://repo.example.com/upper\nenabled=1\ngpgkey=file://{UPPER_KEY}\n"
    );
    // `(spelling, its file, its key, its package)`.
    let lower = ("site", LOWER_PATH, LOWER_KEY, "site-tool");
    let upper = ("SITE", UPPER_PATH, UPPER_KEY, "site-agent");

    for (excluded, kept) in [(lower, upper), (upper, lower)] {
        for reverse in [false, true] {
            let what = format!("exclude {}, files reversed {reverse}", excluded.0);
            let mut snap = InspectionSnapshot::new();
            let mut repo_files = vec![
                repo_file(LOWER_PATH, &lower_repo),
                repo_file(UPPER_PATH, &upper_repo),
            ];
            if reverse {
                repo_files.reverse();
            }
            snap.rpm = Some(RpmSection {
                packages_added: vec![package(lower.3, lower.0), package(upper.3, upper.0)],
                repo_files,
                gpg_keys: vec![key(LOWER_KEY), key(UPPER_KEY)],
                ..Default::default()
            });
            snap.baseline = Some(baseline());
            let mut session = RefineSession::new(snap);

            set_repo(&mut session, excluded.0, false);

            let rpm = session.snapshot_projected().rpm.expect("rpm section");
            let included = |name: &str| {
                rpm.packages_added
                    .iter()
                    .find(|p| p.name == name)
                    .is_some_and(|p| p.disposition.is_included())
            };
            assert!(!included(excluded.3), "{what}: {} excluded", excluded.3);
            assert!(included(kept.3), "{what}: {} kept", kept.3);
            assert!(!carried(&session, excluded.1), "{what}: {}", excluded.1);
            assert!(!carried(&session, excluded.2), "{what}: {}", excluded.2);
            assert!(carried(&session, kept.1), "{what}: {}", kept.1);
            assert!(carried(&session, kept.2), "{what}: {}", kept.2);

            let export = Export::of(&session);
            let cf = &export.containerfile;
            assert!(
                export.stages(kept.1) && export.stages(kept.2),
                "{what}: the kept repo's file and key are staged"
            );
            assert!(
                !export.stages(excluded.1) && !export.stages(excluded.2),
                "{what}: the excluded repo's file and key are not"
            );
            assert!(installs(cf, kept.3), "{what}:\n{cf}");
            assert!(!installs(cf, excluded.3), "{what}:\n{cf}");
            assert_eq!(
                session.pending_changes().repos_excluded(),
                [excluded.0.to_string()],
                "{what}"
            );
        }
    }
}

/// A package recorded in another case than the one section that defines
/// it still belongs to that section: excluding the section takes the
/// package, file and key, from either spelling, and re-including it with
/// the package's spelling brings them back.
#[test]
fn a_lone_case_variant_still_answers_for_a_repo_toggle() {
    const PATH: &str = "/etc/yum.repos.d/vendor.repo";
    const KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-vendor";
    let content =
        format!("[vendor]\nbaseurl=https://repo.example.com/v\nenabled=1\ngpgkey=file://{KEY}\n");
    for (exclude_as, include_as) in [("vendor", "Vendor"), ("Vendor", "vendor")] {
        let what = format!("exclude {exclude_as}, include {include_as}");
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![package("vendor-tool", "Vendor")],
            repo_files: vec![repo_file(PATH, &content)],
            gpg_keys: vec![key(KEY)],
            ..Default::default()
        });
        snap.baseline = Some(baseline());
        let mut session = RefineSession::new(snap);
        let included = |session: &RefineSession| {
            session
                .snapshot_projected()
                .rpm
                .expect("rpm section")
                .packages_added[0]
                .disposition
                .is_included()
        };

        set_repo(&mut session, exclude_as, false);
        assert!(!included(&session), "{what}: package excluded");
        assert!(!carried(&session, PATH), "{what}: file");
        assert!(!carried(&session, KEY), "{what}: key");

        set_repo(&mut session, include_as, true);
        assert!(included(&session), "{what}: package back");
        assert!(carried(&session, PATH), "{what}: file back");
        assert!(carried(&session, KEY), "{what}: key back");
        assert!(
            session.pending_changes().repos_excluded().is_empty(),
            "{what}"
        );
    }
}
