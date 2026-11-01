//! Effective `/var` backing at export (spec § 3.4, § 5.4).
//!
//! The snapshot verdict says what created the directory on the source
//! host; these tests pin what the export says about whether that creator
//! reaches the image. Every case asserts the `VarDirOutcome` the function
//! returns *and* the rendered artifacts.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::FindingKind;
use inspectah_core::types::config::{
    ConfigCategory, ConfigFileEntry, ConfigFileKind, ConfigSection,
};
use inspectah_core::types::rpm::{FileOwnershipEntry, PackageEntry, PackageState, RpmSection};
use inspectah_core::types::storage::{StorageSection, VarDirBacking, VarDirectory};
use inspectah_core::types::users::UserGroupSection;
use inspectah_pipeline::render::var_dirs::{
    EffectiveBacking, UnresolvedReason, VarDirOutcome, effective_var_backing,
};

// --- fixture helpers ---------------------------------------------------

fn dir(path: &str, backing: VarDirBacking) -> VarDirectory {
    VarDirectory {
        path: path.into(),
        backing: Some(backing),
        ..Default::default()
    }
}

fn dir_with_mode(
    path: &str,
    backing: VarDirBacking,
    mode: &str,
    uid: u32,
    owner: &str,
) -> VarDirectory {
    VarDirectory {
        path: path.into(),
        backing: Some(backing),
        mode: Some(mode.into()),
        owner_uid: Some(uid),
        owner_gid: Some(uid),
        owner_name: Some(owner.into()),
        group_name: Some(owner.into()),
    }
}

fn tmpfiles(conf: &str, line: &str) -> VarDirBacking {
    VarDirBacking::Tmpfiles {
        conf: conf.into(),
        line: line.into(),
    }
}

fn config_file(path: &str, included: bool) -> ConfigFileEntry {
    ConfigFileEntry {
        path: path.into(),
        kind: ConfigFileKind::Unowned,
        category: ConfigCategory::Tmpfiles,
        content: "# fixture\n".into(),
        disposition: if included {
            FindingKind::included()
        } else {
            FindingKind::excluded()
        },
        ..Default::default()
    }
}

fn package(name: &str, included: bool) -> PackageEntry {
    PackageEntry {
        name: name.into(),
        arch: "x86_64".into(),
        state: PackageState::Added,
        source_repo: "appstream".into(),
        disposition: if included {
            FindingKind::included()
        } else {
            FindingKind::excluded()
        },
        ..Default::default()
    }
}

/// A `useradd` user, so tmpfiles ownership resolves to the symbolic name
/// with no "not guaranteed in image" comment.
fn materialized_user(name: &str, uid: u32) -> UserGroupSection {
    UserGroupSection {
        users: vec![serde_json::json!({
            "name": name,
            "uid": uid,
            "gid": uid,
            "include": true,
            "containerfile_strategy": "useradd"
        })],
        ..Default::default()
    }
}

fn outcome_for<'a>(outcomes: &'a [VarDirOutcome], path: &str) -> &'a EffectiveBacking {
    &outcomes
        .iter()
        .find(|o| o.path == path)
        .unwrap_or_else(|| panic!("no outcome for {path}"))
        .outcome
}

/// Everything `render_all` wrote that this task's assertions read.
struct Export {
    containerfile: String,
    var_conf: Option<String>,
    appone_fragment_exported: bool,
    audit: String,
}

fn export(snap: &InspectionSnapshot) -> Export {
    let out = tempfile::tempdir().expect("tempdir");
    inspectah_pipeline::render::render_all(snap, &RenderContext { target: None }, out.path())
        .expect("render_all");
    Export {
        containerfile: std::fs::read_to_string(out.path().join("Containerfile")).expect("cf"),
        var_conf: std::fs::read_to_string(
            out.path()
                .join("config/usr/lib/tmpfiles.d/inspectah-var.conf"),
        )
        .ok(),
        appone_fragment_exported: out
            .path()
            .join("config/etc/tmpfiles.d/appone.conf")
            .exists(),
        audit: std::fs::read_to_string(out.path().join("audit-report.md")).expect("audit"),
    }
}

fn var_conf(snap: &InspectionSnapshot) -> String {
    export(snap).var_conf.unwrap_or_default()
}

// --- the base fixture --------------------------------------------------

/// `/var/lib/pgsql` is unbacked; `/var/lib/appone/cache` is created by an
/// `/etc` fragment the config section carries.
fn base_snapshot() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        var_directories: vec![
            dir_with_mode("/var/lib/pgsql", VarDirBacking::Unbacked, "0700", 0, "root"),
            dir_with_mode(
                "/var/lib/appone/cache",
                tmpfiles(
                    "/etc/tmpfiles.d/appone.conf",
                    "d /var/lib/appone/cache 0750 appone appone -",
                ),
                "0750",
                1001,
                "appone",
            ),
        ],
        ..Default::default()
    });
    snap.config = Some(ConfigSection {
        files: vec![config_file("/etc/tmpfiles.d/appone.conf", true)],
    });
    snap.rpm = Some(RpmSection {
        packages_added: vec![package("appone", true)],
        ..Default::default()
    });
    snap.users_groups = Some(materialized_user("appone", 1001));
    snap
}

fn set_config_included(snap: &mut InspectionSnapshot, path: &str, included: bool) {
    let entry = snap
        .config
        .as_mut()
        .expect("config section")
        .files
        .iter_mut()
        .find(|f| f.path == path)
        .expect("config entry");
    entry.disposition = if included {
        FindingKind::included()
    } else {
        FindingKind::excluded()
    };
}

const APPONE_LOST_COMMENT: &str = "# /var/lib/appone/cache: created on the host by \
     /etc/tmpfiles.d/appone.conf, which this export does not carry";

#[test]
fn provisions_what_the_export_does_not_carry() {
    // Nothing excluded: the unbacked directory is provisioned, the carried
    // one is not.
    let mut snap = base_snapshot();
    let outcomes = effective_var_backing(&snap);
    assert_eq!(
        outcome_for(&outcomes, "/var/lib/pgsql"),
        &EffectiveBacking::Provision { lost: None }
    );
    assert_eq!(
        outcome_for(&outcomes, "/var/lib/appone/cache"),
        &EffectiveBacking::Carried
    );

    let rendered = export(&snap);
    assert_eq!(
        rendered.var_conf.as_deref(),
        Some("d /var/lib/pgsql 0700 root root -\n")
    );
    assert!(
        rendered.appone_fragment_exported,
        "the included fragment is in the export"
    );

    // Exclude the fragment under Configuration Files: the directory it
    // created loses its creator and is provisioned instead.
    set_config_included(&mut snap, "/etc/tmpfiles.d/appone.conf", false);
    let outcomes = effective_var_backing(&snap);
    assert_eq!(
        outcome_for(&outcomes, "/var/lib/appone/cache"),
        &EffectiveBacking::Provision {
            lost: Some("/etc/tmpfiles.d/appone.conf".into())
        }
    );

    let rendered = export(&snap);
    let conf = rendered.var_conf.expect("conf");
    assert!(
        conf.contains(APPONE_LOST_COMMENT),
        "lost-creator comment missing: {conf}"
    );
    let comment_at = conf.find(APPONE_LOST_COMMENT).expect("comment");
    let entry_at = conf
        .find("d /var/lib/appone/cache 0750 appone appone -")
        .expect("entry");
    assert!(comment_at < entry_at, "comment precedes the entry: {conf}");
    assert!(
        !rendered.appone_fragment_exported,
        "the excluded fragment is not in the export"
    );

    // Include it again: both the outcome and both artifacts return.
    set_config_included(&mut snap, "/etc/tmpfiles.d/appone.conf", true);
    let outcomes = effective_var_backing(&snap);
    assert_eq!(
        outcome_for(&outcomes, "/var/lib/appone/cache"),
        &EffectiveBacking::Carried
    );
    let rendered = export(&snap);
    let conf = rendered.var_conf.expect("conf");
    assert!(!conf.contains("/var/lib/appone/cache"), "no entry: {conf}");
    assert!(!conf.contains(APPONE_LOST_COMMENT), "no comment: {conf}");
    assert!(rendered.appone_fragment_exported);
}

#[test]
fn run_fragments_are_provisioned_whatever_the_config_decision() {
    // No host's /run fragment reaches the image, so the directory is
    // provisioned even when a config entry for the fragment is included.
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        var_directories: vec![dir_with_mode(
            "/var/cache/runapp",
            tmpfiles(
                "/run/tmpfiles.d/runapp.conf",
                "d /var/cache/runapp 0755 root root -",
            ),
            "0755",
            0,
            "root",
        )],
        ..Default::default()
    });
    snap.config = Some(ConfigSection {
        files: vec![config_file("/run/tmpfiles.d/runapp.conf", true)],
    });

    let outcomes = effective_var_backing(&snap);
    assert_eq!(
        outcome_for(&outcomes, "/var/cache/runapp"),
        &EffectiveBacking::Provision {
            lost: Some("/run/tmpfiles.d/runapp.conf".into())
        }
    );
    let conf = var_conf(&snap);
    assert!(
        conf.contains(
            "# /var/cache/runapp: created on the host by /run/tmpfiles.d/runapp.conf, \
             which this export does not carry"
        ),
        "lost-creator comment missing: {conf}"
    );
    assert!(
        conf.contains("d /var/cache/runapp 0755 root root -"),
        "active entry missing: {conf}"
    );
}

#[test]
fn units_follow_their_unit_file_and_rpm_owner() {
    // A StateDirectory unit whose unit file is an excluded config entry is
    // provisioned, and carried when the entry is included.
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        var_directories: vec![dir_with_mode(
            "/var/lib/myapp",
            VarDirBacking::StateDirectory {
                unit: "myapp.service".into(),
            },
            "0755",
            0,
            "root",
        )],
        ..Default::default()
    });
    snap.config = Some(ConfigSection {
        files: vec![config_file("/etc/systemd/system/myapp.service", false)],
    });

    assert_eq!(
        outcome_for(&effective_var_backing(&snap), "/var/lib/myapp"),
        &EffectiveBacking::Provision {
            lost: Some("myapp.service".into())
        }
    );
    assert!(var_conf(&snap).contains("d /var/lib/myapp 0755 root root -"));

    set_config_included(&mut snap, "/etc/systemd/system/myapp.service", true);
    assert_eq!(
        outcome_for(&effective_var_backing(&snap), "/var/lib/myapp"),
        &EffectiveBacking::Carried
    );
    assert!(!var_conf(&snap).contains("/var/lib/myapp"));

    // No config entry, but the unit file is owned by a carried package.
    snap.config = Some(ConfigSection { files: Vec::new() });
    snap.rpm = Some(RpmSection {
        packages_added: vec![package("myapp", true)],
        file_ownership: vec![FileOwnershipEntry {
            package_name: "myapp".into(),
            paths: vec!["/usr/lib/systemd/system/myapp.service".into()],
        }],
        ..Default::default()
    });
    assert_eq!(
        outcome_for(&effective_var_backing(&snap), "/var/lib/myapp"),
        &EffectiveBacking::Carried
    );
}

/// Both unit sources name the same file, which neither of the two
/// single-source cases above reaches. systemd gives the `/etc` unit
/// precedence while it exists and falls back to the vendor unit when it
/// does not, so excluding the `/etc` override leaves the RPM-owned copy
/// running in the target. That copy still declares `StateDirectory=`, so
/// the creator is carried and a `d` line would duplicate it under
/// whatever ownership the export guessed.
#[test]
fn an_excluded_etc_unit_falls_back_to_the_vendor_unit_that_survives() {
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        var_directories: vec![dir_with_mode(
            "/var/lib/myapp",
            VarDirBacking::StateDirectory {
                unit: "myapp.service".into(),
            },
            "0755",
            0,
            "root",
        )],
        ..Default::default()
    });
    snap.config = Some(ConfigSection {
        files: vec![config_file("/etc/systemd/system/myapp.service", false)],
    });
    snap.rpm = Some(RpmSection {
        packages_added: vec![package("myapp", true)],
        file_ownership: vec![FileOwnershipEntry {
            package_name: "myapp".into(),
            paths: vec!["/usr/lib/systemd/system/myapp.service".into()],
        }],
        ..Default::default()
    });

    assert_eq!(
        outcome_for(&effective_var_backing(&snap), "/var/lib/myapp"),
        &EffectiveBacking::Carried
    );
    let conf = var_conf(&snap);
    assert!(
        !conf.contains("/var/lib/myapp"),
        "the surviving vendor unit makes the directory: {conf}"
    );

    // The same exclusion with the RPM owning the excluded file itself and
    // nothing else: no copy survives, so the creator really is lost.
    snap.rpm.as_mut().expect("rpm").file_ownership = vec![FileOwnershipEntry {
        package_name: "myapp".into(),
        paths: vec!["/etc/systemd/system/myapp.service".into()],
    }];
    assert_eq!(
        outcome_for(&effective_var_backing(&snap), "/var/lib/myapp"),
        &EffectiveBacking::Provision {
            lost: Some("myapp.service".into())
        }
    );
}

/// One package owning the same unit filename at two paths, which is what
/// a packaged override or an RPM-owned preset symlink under
/// `/etc/systemd/system/*.wants/` produces. `unit_owners` keeps one path
/// per filename, so whichever path it keeps decides the answer: record
/// the excluded `/etc` file and the `owner_path != config_path` guard
/// fails, calling a vendor unit that survives the exclusion lost. The
/// case above reaches the guard with one path each and never reaches the
/// rule that picks between two.
#[test]
fn a_packaged_etc_override_does_not_mask_its_own_vendor_unit() {
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        var_directories: vec![dir_with_mode(
            "/var/lib/myapp",
            VarDirBacking::StateDirectory {
                unit: "myapp.service".into(),
            },
            "0755",
            0,
            "root",
        )],
        ..Default::default()
    });
    snap.config = Some(ConfigSection {
        files: vec![config_file("/etc/systemd/system/myapp.service", false)],
    });
    snap.rpm = Some(RpmSection {
        packages_added: vec![package("myapp", true)],
        file_ownership: vec![FileOwnershipEntry {
            package_name: "myapp".into(),
            // `/etc` first, so the displacement rule is what runs rather
            // than the vacant-entry insert.
            paths: vec![
                "/etc/systemd/system/myapp.service".into(),
                "/usr/lib/systemd/system/myapp.service".into(),
            ],
        }],
        ..Default::default()
    });
    assert_eq!(
        outcome_for(&effective_var_backing(&snap), "/var/lib/myapp"),
        &EffectiveBacking::Carried,
        "the vendor copy displaces the recorded /etc path"
    );

    snap.rpm.as_mut().expect("rpm").file_ownership[0]
        .paths
        .reverse();
    assert_eq!(
        outcome_for(&effective_var_backing(&snap), "/var/lib/myapp"),
        &EffectiveBacking::Carried,
        "and the verdict does not depend on file_ownership order"
    );
}

#[test]
fn rpm_owned_absent_from_base_and_install_set_is_provisioned() {
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        var_directories: vec![dir_with_mode(
            "/var/lib/thirdparty",
            VarDirBacking::RpmOwned {
                package: "thirdparty".into(),
            },
            "0755",
            0,
            "root",
        )],
        ..Default::default()
    });
    snap.rpm = Some(RpmSection {
        packages_added: vec![package("thirdparty", false)],
        baseline_package_names: Some(vec!["filesystem".into(), "systemd".into()]),
        ..Default::default()
    });

    assert_eq!(
        outcome_for(&effective_var_backing(&snap), "/var/lib/thirdparty"),
        &EffectiveBacking::Provision {
            lost: Some("thirdparty".into())
        }
    );
    assert!(var_conf(&snap).contains("d /var/lib/thirdparty 0755 root root -"));
}

#[test]
fn a_demoted_repo_enabling_package_is_not_in_the_install_set() {
    // The Containerfile's install loop narrows the install set four
    // ways. Three of them drop packages the built image has anyway, so
    // reading those as carried is right. The repo-enabling demotion is
    // the one that removes a package the image will not have: its
    // serving repo is not carried, so it renders as a FIXME and never
    // installs, and the directory it owns has to be provisioned.
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        var_directories: vec![dir_with_mode(
            "/var/lib/epelapp",
            VarDirBacking::RpmOwned {
                package: "epel-release".into(),
            },
            "0755",
            0,
            "root",
        )],
        ..Default::default()
    });
    let mut epel = package("epel-release", true);
    epel.repo_enabling_from = vec![inspectah_core::types::repo::RepoId::new("epel")];
    snap.rpm = Some(RpmSection {
        packages_added: vec![epel],
        baseline_package_names: Some(vec!["filesystem".into()]),
        ..Default::default()
    });

    assert_eq!(
        outcome_for(&effective_var_backing(&snap), "/var/lib/epelapp"),
        &EffectiveBacking::Provision {
            lost: Some("epel-release".into())
        }
    );
    assert!(var_conf(&snap).contains("d /var/lib/epelapp 0755 root root -"));

    // The same package with its serving repo carried installs, so the
    // directory it owns is carried with it.
    let rpm = snap.rpm.as_mut().expect("rpm");
    rpm.packages_added[0].repo_enabling_from = Vec::new();
    assert_eq!(
        outcome_for(&effective_var_backing(&snap), "/var/lib/epelapp"),
        &EffectiveBacking::Carried
    );
    assert!(!var_conf(&snap).contains("/var/lib/epelapp"));
}

#[test]
fn unresolved_renders_a_fixme_and_no_active_line() {
    // (a) a /usr/lib fragment no file_ownership entry names, with a known
    //     baseline -> UnownedFragment
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        var_directories: vec![dir(
            "/var/lib/vendor",
            tmpfiles(
                "/usr/lib/tmpfiles.d/vendor.conf",
                "d /var/lib/vendor 0755 root root -",
            ),
        )],
        ..Default::default()
    });
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec!["filesystem".into()]),
        ..Default::default()
    });
    assert_eq!(
        outcome_for(&effective_var_backing(&snap), "/var/lib/vendor"),
        &EffectiveBacking::Unresolved {
            creator: "/usr/lib/tmpfiles.d/vendor.conf".into(),
            reason: UnresolvedReason::UnownedFragment,
        }
    );
    let rendered = export(&snap);
    let conf = rendered.var_conf.expect("conf");
    assert!(
        conf.contains(
            "# FIXME: confirm the base image creates /var/lib/vendor; on the host \
             /usr/lib/tmpfiles.d/vendor.conf created it and no package owns that file; \
             if the base does not, uncomment the next line"
        ),
        "FIXME missing: {conf}"
    );
    assert!(
        conf.contains("#d /var/lib/vendor 0755 root root -"),
        "commented entry missing: {conf}"
    );
    assert!(
        !conf.lines().any(|l| l.starts_with("d /var/lib/vendor")),
        "no active line: {conf}"
    );
    assert!(
        rendered.containerfile.contains(
            "# FIXME: confirm the base image creates /var/lib/vendor; on the host \
             /usr/lib/tmpfiles.d/vendor.conf created it and no package owns that file; \
             if the base does not, uncomment the next line"
        ),
        "Storage block FIXME missing: {}",
        rendered.containerfile
    );
    assert!(
        rendered
            .containerfile
            .contains("#d /var/lib/vendor 0755 root root -"),
        "Storage block commented entry missing: {}",
        rendered.containerfile
    );
    assert!(
        !rendered
            .containerfile
            .contains("RUN mkdir -p /var/lib/vendor"),
        "no active provisioning: {}",
        rendered.containerfile
    );

    // (b) a /usr/lib fragment owned by an excluded package, no baseline
    //     -> BaseUnknown
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        var_directories: vec![dir(
            "/var/lib/appone/cache",
            tmpfiles(
                "/usr/lib/tmpfiles.d/appone.conf",
                "d /var/lib/appone/cache 0750 appone appone -",
            ),
        )],
        ..Default::default()
    });
    snap.rpm = Some(RpmSection {
        packages_added: vec![package("appone", false)],
        file_ownership: vec![FileOwnershipEntry {
            package_name: "appone".into(),
            paths: vec!["/usr/lib/tmpfiles.d/appone.conf".into()],
        }],
        baseline_package_names: None,
        ..Default::default()
    });
    assert_eq!(
        outcome_for(&effective_var_backing(&snap), "/var/lib/appone/cache"),
        &EffectiveBacking::Unresolved {
            creator: "/usr/lib/tmpfiles.d/appone.conf".into(),
            reason: UnresolvedReason::BaseUnknown,
        }
    );
    let conf = var_conf(&snap);
    assert!(
        conf.contains(
            "# FIXME: confirm the base image creates /var/lib/appone/cache; on the host \
             /usr/lib/tmpfiles.d/appone.conf created it and the target base package set \
             is unknown; if the base does not, uncomment the next line"
        ),
        "FIXME missing: {conf}"
    );
    assert!(conf.contains("#d /var/lib/appone/cache 0755 root root -"));
    assert!(!conf.lines().any(|l| l.starts_with("d /var/lib/appone")));

    // (c) RpmOwned with the package excluded and no baseline -> BaseUnknown
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        var_directories: vec![dir(
            "/var/lib/appone",
            VarDirBacking::RpmOwned {
                package: "appone".into(),
            },
        )],
        ..Default::default()
    });
    snap.rpm = Some(RpmSection {
        packages_added: vec![package("appone", false)],
        baseline_package_names: None,
        ..Default::default()
    });
    assert_eq!(
        outcome_for(&effective_var_backing(&snap), "/var/lib/appone"),
        &EffectiveBacking::Unresolved {
            creator: "appone".into(),
            reason: UnresolvedReason::BaseUnknown,
        }
    );
    let conf = var_conf(&snap);
    assert!(
        conf.contains(
            "# FIXME: confirm the base image creates /var/lib/appone; on the host appone \
             created it and the target base package set is unknown; if the base does not, \
             uncomment the next line"
        ),
        "FIXME missing: {conf}"
    );
    assert!(!conf.lines().any(|l| l.starts_with("d /var/lib/appone")));
}

#[test]
fn provisioned_not_unresolved_and_carried_cases() {
    // The same /usr/lib fragment, owner known and lost against a baseline
    // that lacks it: an active line and the lost-creator comment.
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        var_directories: vec![dir_with_mode(
            "/var/lib/appone/cache",
            tmpfiles(
                "/usr/lib/tmpfiles.d/appone.conf",
                "d /var/lib/appone/cache 0750 appone appone -",
            ),
            "0750",
            1001,
            "appone",
        )],
        ..Default::default()
    });
    snap.users_groups = Some(materialized_user("appone", 1001));
    snap.rpm = Some(RpmSection {
        packages_added: vec![package("appone", false)],
        file_ownership: vec![FileOwnershipEntry {
            package_name: "appone".into(),
            paths: vec!["/usr/lib/tmpfiles.d/appone.conf".into()],
        }],
        baseline_package_names: Some(vec!["filesystem".into()]),
        ..Default::default()
    });
    assert_eq!(
        outcome_for(&effective_var_backing(&snap), "/var/lib/appone/cache"),
        &EffectiveBacking::Provision {
            lost: Some("/usr/lib/tmpfiles.d/appone.conf".into())
        }
    );
    let conf = var_conf(&snap);
    assert!(
        conf.contains(
            "# /var/lib/appone/cache: created on the host by /usr/lib/tmpfiles.d/appone.conf, \
             which this export does not carry"
        ),
        "lost-creator comment missing: {conf}"
    );
    assert!(conf.contains("d /var/lib/appone/cache 0750 appone appone -"));

    // appone included and installable: carried, with and without a baseline.
    for baseline in [Some(vec!["filesystem".to_string()]), None] {
        let rpm = snap.rpm.as_mut().expect("rpm");
        rpm.packages_added[0].disposition = FindingKind::included();
        rpm.baseline_package_names = baseline.clone();
        assert_eq!(
            outcome_for(&effective_var_backing(&snap), "/var/lib/appone/cache"),
            &EffectiveBacking::Carried,
            "baseline {baseline:?}"
        );
        assert!(!var_conf(&snap).contains("/var/lib/appone/cache"));
    }
}

#[test]
fn etc_fragments_follow_the_config_decision_never_the_provider() {
    // Whatever appone's disposition and whether a baseline exists, an /etc
    // fragment's directory follows the config decision and is never
    // Unresolved (§ 3.4 table, first row).
    for included in [true, false] {
        for provider_included in [true, false] {
            for baseline in [Some(vec!["filesystem".to_string()]), None] {
                let mut snap = base_snapshot();
                set_config_included(&mut snap, "/etc/tmpfiles.d/appone.conf", included);
                let rpm = snap.rpm.as_mut().expect("rpm");
                rpm.packages_added[0].disposition = if provider_included {
                    FindingKind::included()
                } else {
                    FindingKind::excluded()
                };
                rpm.baseline_package_names = baseline.clone();

                let expected = if included {
                    EffectiveBacking::Carried
                } else {
                    EffectiveBacking::Provision {
                        lost: Some("/etc/tmpfiles.d/appone.conf".into()),
                    }
                };
                assert_eq!(
                    outcome_for(&effective_var_backing(&snap), "/var/lib/appone/cache"),
                    &expected,
                    "included={included} provider_included={provider_included} \
                     baseline={baseline:?}"
                );
            }
        }
    }
}

#[test]
fn absent_sections_follow_the_table_rather_than_panicking() {
    // Review Focus 4. Storage directories, no `config` section and no
    // `rpm` section: the function reads the absent sections as absent
    // evidence, and each backing kind gets the outcome the § 3.4 table
    // assigns it with no config entry, no file_ownership, no baseline and
    // no install set.
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        var_directories: vec![
            dir(
                "/var/lib/etcfrag",
                tmpfiles(
                    "/etc/tmpfiles.d/etcfrag.conf",
                    "d /var/lib/etcfrag 0755 root root -",
                ),
            ),
            dir(
                "/var/lib/runfrag",
                tmpfiles(
                    "/run/tmpfiles.d/runfrag.conf",
                    "d /var/lib/runfrag 0755 root root -",
                ),
            ),
            dir("/var/lib/plain", VarDirBacking::Unbacked),
            dir(
                "/var/lib/usrfrag",
                tmpfiles(
                    "/usr/lib/tmpfiles.d/usrfrag.conf",
                    "d /var/lib/usrfrag 0755 root root -",
                ),
            ),
            dir(
                "/var/lib/stated",
                VarDirBacking::StateDirectory {
                    unit: "stated.service".into(),
                },
            ),
            dir(
                "/var/cache/cached",
                VarDirBacking::CacheDirectory {
                    unit: "cached.service".into(),
                },
            ),
            dir(
                "/var/log/logged",
                VarDirBacking::LogsDirectory {
                    unit: "logged.service".into(),
                },
            ),
            dir(
                "/var/lib/owned",
                VarDirBacking::RpmOwned {
                    package: "owner-pkg".into(),
                },
            ),
        ],
        ..Default::default()
    });
    assert!(snap.config.is_none());
    assert!(snap.rpm.is_none());

    let outcomes = effective_var_backing(&snap);
    assert_eq!(
        outcome_for(&outcomes, "/var/lib/etcfrag"),
        &EffectiveBacking::Provision {
            lost: Some("/etc/tmpfiles.d/etcfrag.conf".into())
        }
    );
    assert_eq!(
        outcome_for(&outcomes, "/var/lib/runfrag"),
        &EffectiveBacking::Provision {
            lost: Some("/run/tmpfiles.d/runfrag.conf".into())
        }
    );
    assert_eq!(
        outcome_for(&outcomes, "/var/lib/plain"),
        &EffectiveBacking::Provision { lost: None }
    );
    assert_eq!(
        outcome_for(&outcomes, "/var/lib/usrfrag"),
        &EffectiveBacking::Unresolved {
            creator: "/usr/lib/tmpfiles.d/usrfrag.conf".into(),
            reason: UnresolvedReason::UnownedFragment,
        }
    );
    for (path, unit) in [
        ("/var/lib/stated", "stated.service"),
        ("/var/cache/cached", "cached.service"),
        ("/var/log/logged", "logged.service"),
    ] {
        assert_eq!(
            outcome_for(&outcomes, path),
            &EffectiveBacking::Unresolved {
                creator: unit.into(),
                reason: UnresolvedReason::UnownedUnit,
            },
            "{path}"
        );
    }
    assert_eq!(
        outcome_for(&outcomes, "/var/lib/owned"),
        &EffectiveBacking::Unresolved {
            creator: "owner-pkg".into(),
            reason: UnresolvedReason::BaseUnknown,
        }
    );

    // The five Unresolved directories render a commented line and no
    // active one, so no FIXME-requiring case is silently provisioned.
    let conf = var_conf(&snap);
    for path in [
        "/var/lib/usrfrag",
        "/var/lib/stated",
        "/var/cache/cached",
        "/var/log/logged",
        "/var/lib/owned",
    ] {
        assert!(
            conf.contains(&format!("#d {path} 0755 root root -")),
            "commented entry for {path} missing: {conf}"
        );
        assert!(
            !conf.lines().any(|l| l.starts_with(&format!("d {path} "))),
            "active entry for {path} present: {conf}"
        );
    }
}

// --- the fleet `Unbacked` survivor preference --------------------------

/// One host of a two-host fleet, with `/var/lib/shared` classified as
/// `backing` at `mode`.
fn fleet_host(hostname: &str, backing: VarDirBacking, mode: &str) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.meta.insert(
        "hostname".into(),
        serde_json::Value::String(hostname.into()),
    );
    snap.storage = Some(StorageSection {
        var_directories: vec![dir_with_mode("/var/lib/shared", backing, mode, 0, "root")],
        ..Default::default()
    });
    snap
}

#[test]
fn a_carryable_backed_verdict_beats_another_hosts_unbacked() {
    // One image serves the whole fleet, so a fragment any host carries
    // creates the directory on every host. The first host by hostname
    // reports Unbacked — its scan could not read the fragment, so it has
    // no provenance to offer — and the second names the /etc fragment
    // both hosts carry. The export must not add a second `d` line for a
    // path that fragment already creates.
    let mut backed = fleet_host(
        "host-b",
        tmpfiles(
            "/etc/tmpfiles.d/shared.conf",
            "d /var/lib/shared 0755 root root -",
        ),
        "0755",
    );
    backed.config = Some(ConfigSection {
        files: vec![config_file("/etc/tmpfiles.d/shared.conf", true)],
    });
    let mut unbacked = fleet_host("host-a", VarDirBacking::Unbacked, "0700");
    unbacked.config = Some(ConfigSection {
        files: vec![config_file("/etc/tmpfiles.d/shared.conf", true)],
    });

    let (merged, _) =
        inspectah_core::aggregate::merge_snapshots(vec![backed, unbacked], None, None)
            .expect("merge");

    let outcomes = effective_var_backing(&merged);
    assert_eq!(
        outcome_for(&outcomes, "/var/lib/shared"),
        &EffectiveBacking::Carried
    );
    let conf = var_conf(&merged);
    assert!(
        !conf.contains("/var/lib/shared"),
        "the carried directory is not provisioned again: {conf}"
    );
}

#[test]
fn a_run_fragment_does_not_beat_another_hosts_unbacked() {
    // No host's /run fragment reaches the image, so it is not a creator
    // the export can carry: the `Unbacked` verdict, and the mode captured
    // with it, survive.
    let backed = fleet_host(
        "host-b",
        tmpfiles(
            "/run/tmpfiles.d/shared.conf",
            "d /var/lib/shared 0755 root root -",
        ),
        "0755",
    );
    let unbacked = fleet_host("host-a", VarDirBacking::Unbacked, "0700");

    let (merged, _) =
        inspectah_core::aggregate::merge_snapshots(vec![backed, unbacked], None, None)
            .expect("merge");

    let outcomes = effective_var_backing(&merged);
    let outcome = outcomes
        .iter()
        .find(|o| o.path == "/var/lib/shared")
        .expect("outcome");
    assert_eq!(outcome.source, Some(VarDirBacking::Unbacked));
    assert_eq!(outcome.outcome, EffectiveBacking::Provision { lost: None });

    let conf = var_conf(&merged);
    assert!(
        conf.contains("d /var/lib/shared 0700 root root -"),
        "the unbacked host's mode survives: {conf}"
    );
    assert!(
        !conf.contains("/run/tmpfiles.d/shared.conf"),
        "no /run fragment is named as a lost creator: {conf}"
    );
}

#[test]
fn the_audit_report_lists_the_source_verdict_beside_the_export_outcome() {
    let mut snap = base_snapshot();
    set_config_included(&mut snap, "/etc/tmpfiles.d/appone.conf", false);
    snap.storage
        .as_mut()
        .expect("storage")
        .var_directories
        .push(dir(
            "/var/lib/vendor",
            tmpfiles(
                "/usr/lib/tmpfiles.d/vendor.conf",
                "d /var/lib/vendor 0755 root root -",
            ),
        ));

    let audit = export(&snap).audit;
    assert!(
        audit.contains("| `/var/lib/pgsql` | unbacked on host | provisioned |"),
        "unbacked row missing: {audit}"
    );
    assert!(
        audit.contains(
            "| `/var/lib/appone/cache` | created on host by /etc/tmpfiles.d/appone.conf | \
             not carried, provisioned |"
        ),
        "lost row missing: {audit}"
    );
    assert!(
        audit.contains(
            "| `/var/lib/vendor` | created on host by /usr/lib/tmpfiles.d/vendor.conf | \
             unresolved: no package owns that file |"
        ),
        "unresolved row missing: {audit}"
    );
}
