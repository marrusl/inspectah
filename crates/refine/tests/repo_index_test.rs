use std::collections::HashSet;

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

/// One section names the key literally, another with a dnf variable the
/// scan expanded to it. Both resolve to the captured key, so the key is
/// shared between them and both are Verified.
#[test]
fn a_key_named_literally_and_with_a_variable_is_one_shared_key() {
    const KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-10";
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        repo_files: vec![RepoFile {
            path: "/etc/yum.repos.d/epel.repo".into(),
            content: "[epel]\nbaseurl=http://example.invalid/epel\n\
                      gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-$releasever_major\n\
                      [mirror]\nbaseurl=http://example.invalid/mirror\n\
                      gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-10\n\
                      [other]\nbaseurl=http://example.invalid/other\n\
                      gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-$nosuchvar\n"
                .into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        gpg_keys: vec![RepoFile {
            path: KEY.into(),
            content: "key-data".into(),
            disposition: FindingKind::included(),
            gpgkey_references: ["/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-$releasever_major".into()]
                .into(),
            ..Default::default()
        }],
        ..Default::default()
    });
    let index = RepoIndex::build(&snap);

    for id in ["epel", "mirror"] {
        assert_eq!(
            index.gpg_keys_by_section().get(&RepoId::new(id)),
            Some(&vec![KEY.to_string()]),
            "{id} names the key at its captured path"
        );
    }
    assert_eq!(
        index.provenance(&RepoId::new("epel")),
        RepoProvenance::Verified
    );
    assert_eq!(
        index.provenance(&RepoId::new("other")),
        RepoProvenance::Incomplete,
        "a reference no captured key answers is still missing"
    );
}

#[test]
fn test_repo_index_gpg_shared_key() {
    let snap = make_snap_with_repos();
    let index = RepoIndex::build(&snap);
    for id in ["baseos", "appstream"] {
        assert_eq!(
            index.gpg_keys_by_section().get(&RepoId::new(id)),
            Some(&vec![
                "/etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial".to_string()
            ]),
            "{id}"
        );
    }
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
    // A second package spelled like the file's own header is included so
    // `packages_by_repo().len() == 1` can actually fail if case-folding
    // regresses -- with one package only, that map always has exactly one
    // key regardless of whether folding works.
    let idx = index_with(
        "[EPEL_10_Everything_x86_64]\nname=EPEL 10 Everything\nbaseurl=http://example.invalid/epel10\n",
        &[
            ("some-pkg", "x86_64", "epel_10_everything_x86_64"),
            ("other-pkg", "x86_64", "EPEL_10_Everything_x86_64"),
        ],
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
    assert_eq!(
        idx.packages_by_repo()
            .get(&RepoId::new("epel_10_everything_x86_64"))
            .unwrap()
            .len(),
        2,
        "both case-variant packages must land under the same key"
    );
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
fn incomplete_when_declared_gpg_key_not_captured() {
    // has_source is true, but the declared gpgkey path was never captured
    // in rpm.gpg_keys (index_with's snapshot never populates it) -- the
    // "a gpgkey=file:// path not in gpg_keys" half of the Incomplete
    // definition, distinct from the "no baseurl/metalink/mirrorlist" half.
    let idx = index_with(
        "[some-repo]\nname=Some Repo\nbaseurl=http://example.invalid/some\ngpgkey=file:///missing\n",
        &[("some-pkg", "x86_64", "some-repo")],
    );
    assert_eq!(
        idx.provenance(&RepoId::new("some-repo")),
        RepoProvenance::Incomplete
    );
}

#[test]
fn a_replaced_uncaptured_key_does_not_make_a_repo_incomplete() {
    // dnf keeps the last gpgkey= of a reopened section, so a key the
    // first block named and a later block replaced is not one dnf asks for.
    let mut snap = make_snap_with_repos();
    let rpm = snap.rpm.as_mut().unwrap();
    rpm.repo_files[1].content = "[epel]\nname=EPEL 9\nbaseurl=http://example.invalid/epel\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-gone\n[epel]\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9\n".into();
    let idx = RepoIndex::build(&snap);
    assert_eq!(
        idx.provenance(&RepoId::new("epel")),
        RepoProvenance::Verified
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

    // Confirm the fixture actually produces Incomplete -- otherwise the
    // apply() below would pass just as well if the section were Verified,
    // and the test couldn't tell the two apart.
    assert_eq!(
        session
            .repo_index()
            .provenance(&RepoId::new("incomplete-repo")),
        RepoProvenance::Incomplete
    );

    // Incomplete -> Ok. `include: true`, not `false`: with no baseline on
    // this snapshot every package normalizes to Investigate -> excluded at
    // construction (`normalize_package_defaults`), so package "a" (and the
    // other two) already start excluded. Before Task 7's member-based no-op
    // rule (§ 3.2, position 29) that didn't matter -- the old rule read only
    // repo-exclusion history, which is empty here, and accepted either
    // direction. The new rule reads the real per-package state, so
    // `include: false` is now a genuine no-op (refused) and would no longer
    // exercise what this test is for: confirming `validate_target` accepts
    // Incomplete provenance. `include: true` is a real change to "a"'s one
    // effective member and reaches that check.
    assert!(
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Repo {
                    path: "incomplete-repo".into(),
                },
                include: true,
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

/// Only host-enabled sections keep a repo file or a key carried. A file
/// whose sections the host all disabled, and a key only such a file names,
/// are not carried even with nothing excluded; the session applies this
/// only when a repo exclusion reaches them, which is what keeps the scan's
/// own export unchanged.
#[test]
fn only_host_enabled_sections_keep_a_file_or_key_carried() {
    const ENABLED_FILE: &str = "/etc/yum.repos.d/site.repo";
    const DISABLED_FILE: &str = "/etc/yum.repos.d/site-testing.repo";
    const SHARED_KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-site";
    const DISABLED_ONLY_KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-site-testing";

    let repo = |path: &str, content: &str| RepoFile {
        path: path.into(),
        content: content.into(),
        disposition: FindingKind::included(),
        ..Default::default()
    };
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        repo_files: vec![
            repo(
                ENABLED_FILE,
                "[site]\nbaseurl=http://example.invalid/site\n\
                 gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-site\n\
                 [site-source]\nbaseurl=http://example.invalid/site-source\nenabled=0\n\
                 gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-site\n",
            ),
            repo(
                DISABLED_FILE,
                "[site-testing]\nbaseurl=http://example.invalid/testing\nenabled=0\n\
                 gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-site-testing\n\
                 [site-testing-source]\nbaseurl=http://example.invalid/testing-source\n\
                 enabled=false\n\
                 gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-site\n",
            ),
        ],
        gpg_keys: vec![
            repo(SHARED_KEY, "key-data"),
            repo(DISABLED_ONLY_KEY, "key-data"),
        ],
        ..Default::default()
    });
    let index = RepoIndex::build(&snap);
    let none = HashSet::new();
    let site = HashSet::from([RepoId::new("site")]);
    let disabled = HashSet::from([
        RepoId::new("site-testing"),
        RepoId::new("site-testing-source"),
        RepoId::new("site-source"),
    ]);

    // (what, carried with nothing excluded, with [site] excluded,
    //  with every disabled section excluded)
    let cases = [
        (
            ENABLED_FILE,
            index.repo_file_carried(ENABLED_FILE, &none),
            index.repo_file_carried(ENABLED_FILE, &site),
            index.repo_file_carried(ENABLED_FILE, &disabled),
            [true, false, true],
        ),
        (
            DISABLED_FILE,
            index.repo_file_carried(DISABLED_FILE, &none),
            index.repo_file_carried(DISABLED_FILE, &site),
            index.repo_file_carried(DISABLED_FILE, &disabled),
            [false, false, false],
        ),
        (
            SHARED_KEY,
            index.gpg_key_carried(SHARED_KEY, &none),
            index.gpg_key_carried(SHARED_KEY, &site),
            index.gpg_key_carried(SHARED_KEY, &disabled),
            [true, false, true],
        ),
        (
            DISABLED_ONLY_KEY,
            index.gpg_key_carried(DISABLED_ONLY_KEY, &none),
            index.gpg_key_carried(DISABLED_ONLY_KEY, &site),
            index.gpg_key_carried(DISABLED_ONLY_KEY, &disabled),
            [false, false, false],
        ),
    ];
    for (what, none, site, disabled, expected) in cases {
        assert_eq!([none, site, disabled], expected, "{what}");
    }
}

/// Provenance and the repo-toggle gate follow the definition dnf uses: the
/// first file in dnf's read order to define an id, never the last one in
/// `repo_files`, which is the host's `read_dir` order. Every case runs with
/// the files in both orders, and the inventory of defining files keeps
/// both whichever one wins.
#[test]
fn a_duplicate_repo_id_takes_dnfs_definition_in_either_file_order() {
    const KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-r";
    const ENABLED: &str = "[r]\nbaseurl=http://example.invalid/r\nenabled=1\n";
    const DISABLED: &str = "[r]\nbaseurl=http://example.invalid/r\nenabled=0\n";
    const KEYED: &str =
        "[r]\nbaseurl=http://example.invalid/r\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-r\n";
    const UNKEYED_MISSING: &str =
        "[r]\nbaseurl=http://example.invalid/r\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-gone\n";
    // `(what, a.repo, b.repo, provenance, toggle accepted)`.
    let cases = [
        (
            "a enabled, b disabled",
            ENABLED,
            DISABLED,
            RepoProvenance::Verified,
            true,
        ),
        (
            "a disabled, b enabled",
            DISABLED,
            ENABLED,
            RepoProvenance::Disabled,
            false,
        ),
        (
            "a's key captured, b's missing",
            KEYED,
            UNKEYED_MISSING,
            RepoProvenance::Verified,
            true,
        ),
        (
            "a's key missing, b's captured",
            UNKEYED_MISSING,
            KEYED,
            RepoProvenance::Incomplete,
            true,
        ),
    ];
    for (what, a, b, provenance, toggles) in cases {
        let file = |name: &str, content: &str| RepoFile {
            path: format!("/etc/yum.repos.d/{name}"),
            content: content.into(),
            disposition: FindingKind::included(),
            ..Default::default()
        };
        let forward = vec![file("a.repo", a), file("b.repo", b)];
        let mut reverse = forward.clone();
        reverse.reverse();
        for (order, repo_files) in [("in order", forward), ("reversed", reverse)] {
            let mut snap = InspectionSnapshot::new();
            snap.rpm = Some(RpmSection {
                packages_added: vec![PackageEntry {
                    name: "tool".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    source_repo: "r".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                }],
                repo_files,
                gpg_keys: vec![RepoFile {
                    path: KEY.into(),
                    content: "key-data".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                }],
                ..Default::default()
            });
            let index = RepoIndex::build(&snap);
            assert_eq!(
                index.provenance(&RepoId::new("r")),
                provenance,
                "{what}, {order}: provenance"
            );
            let mut defining = index.repo_file_by_section()[&RepoId::new("r")].clone();
            defining.sort();
            assert_eq!(
                defining,
                ["/etc/yum.repos.d/a.repo", "/etc/yum.repos.d/b.repo"],
                "{what}, {order}: both files stay in the inventory"
            );
            // `include: true`: with no baseline the package starts
            // excluded, so including the repo is a real change and reaches
            // the provenance gate.
            let mut session = RefineSession::new(snap);
            let toggled = session.apply(RefinementOp::SetInclude {
                item_id: ItemId::Repo { path: "r".into() },
                include: true,
            });
            assert_eq!(
                toggled.is_ok(),
                toggles,
                "{what}, {order}: toggle {toggled:?}"
            );
        }
    }
}

/// `[ab]` and `[AB]` are two repos to dnf: each spelling has its own
/// provenance, and a lone case variant answers for a package's spelling.
#[test]
fn case_variant_repo_ids_keep_their_own_provenance() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        repo_files: vec![RepoFile {
            path: "/etc/yum.repos.d/a.repo".into(),
            content: "[ab]\nbaseurl=http://example.invalid/l\nenabled=0\n\
                      [AB]\nbaseurl=http://example.invalid/u\nenabled=1\n\
                      [Solo]\nbaseurl=http://example.invalid/s\nenabled=0\n"
                .into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    let index = RepoIndex::build(&snap);
    let cases = [
        ("ab", RepoProvenance::Disabled),
        ("AB", RepoProvenance::Verified),
        ("Ab", RepoProvenance::Unknown),
        ("solo", RepoProvenance::Disabled),
    ];
    for (id, provenance) in cases {
        assert_eq!(index.provenance(&RepoId::new(id)), provenance, "{id}");
    }
}
