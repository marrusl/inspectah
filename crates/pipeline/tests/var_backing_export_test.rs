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
    ConfigCategory, ConfigFileEntry, ConfigFileKind, ConfigSection, ConfigShadow,
};
use inspectah_core::types::finding::ShadowType;
use inspectah_core::types::rpm::{
    FileOwnershipEntry, PackageEntry, PackageState, RepoFile, RpmSection,
};
use inspectah_core::types::services::{ServiceSection, SystemdDropIn};
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

/// `kind` is a parameter and not a constant because it decides whether a
/// real collector could have produced the entry at all. The config walk
/// consults its unowned exclusion lists only for unowned files, and one
/// of those checks drops bare filenames directly under
/// `/etc/systemd/system`. Choose this per case.
///
/// **Revised 2026-11-01.** That check used to be a nesting-depth test and
/// discarded *every* such filename, which made an `Unowned` entry at a
/// bare unit path a layout no scan produced. It now tests for a symlink,
/// so the walk keeps an admin-written regular file there and drops only
/// systemd's own alias and mask links. Measured on `rhel9`: a privileged
/// scan puts `/etc/systemd/system/sshd.service` in `config.files` as
/// `kind: unowned` while `bluetooth.service` and `kdump.service`, both
/// links to `/dev/null`, stay out. An `Unowned` entry at a bare unit path
/// is therefore collector-shaped now, provided the file is a regular one.
fn config_file(path: &str, kind: ConfigFileKind, included: bool) -> ConfigFileEntry {
    ConfigFileEntry {
        path: path.into(),
        kind,
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
    crond_shadow_exported: bool,
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
        crond_shadow_exported: out
            .path()
            .join("drop-ins/etc/systemd/system/crond.service")
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
        files: vec![config_file(
            "/etc/tmpfiles.d/appone.conf",
            ConfigFileKind::Unowned,
            true,
        )],
    });
    snap.rpm = Some(RpmSection {
        packages_added: vec![package("appone", true)],
        ..Default::default()
    });
    snap.users_groups = Some(materialized_user("appone", 1001));
    snap
}

/// Replace the declaring unit paths on a directory's backing, for the
/// cases that walk one directory through several collector verdicts.
fn set_declared_by(snap: &mut InspectionSnapshot, path: &str, declared_by: &[&str]) {
    let paths: Vec<String> = declared_by.iter().map(|p| p.to_string()).collect();
    let dir = snap
        .storage
        .as_mut()
        .expect("storage section")
        .var_directories
        .iter_mut()
        .find(|d| d.path == path)
        .expect("var directory");
    dir.backing = Some(match dir.backing.take() {
        Some(VarDirBacking::CacheDirectory { .. }) => {
            VarDirBacking::CacheDirectory { declared_by: paths }
        }
        Some(VarDirBacking::LogsDirectory { .. }) => {
            VarDirBacking::LogsDirectory { declared_by: paths }
        }
        _ => VarDirBacking::StateDirectory { declared_by: paths },
    });
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
        files: vec![config_file(
            "/run/tmpfiles.d/runapp.conf",
            ConfigFileKind::Unowned,
            true,
        )],
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
                declared_by: vec!["/etc/systemd/system/myapp.service".into()],
            },
            "0755",
            0,
            "root",
        )],
        ..Default::default()
    });
    snap.config = Some(ConfigSection {
        files: vec![config_file(
            "/etc/systemd/system/myapp.service",
            ConfigFileKind::Unowned,
            false,
        )],
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

    // The declaring file is the vendor unit instead, owned by a carried
    // package and named by no config entry.
    set_declared_by(
        &mut snap,
        "/var/lib/myapp",
        &["/usr/lib/systemd/system/myapp.service"],
    );
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

/// Two unit files declare the same directory, which neither of the two
/// single-declarer cases above reaches. systemd gives the `/etc` unit
/// precedence while it exists and falls back to the vendor unit when it
/// does not, so excluding the `/etc` override leaves the RPM-owned copy
/// running in the target. That copy declares `StateDirectory=` too, and
/// the collector recorded that it does, so the creator is carried and a
/// `d` line would duplicate it under whatever ownership the export
/// guessed.
///
/// The second half is the case this pair exists to tell apart, and the
/// one the filename-keyed lookup got wrong: the vendor unit is installed
/// and carried, but declares nothing. A unit of that name reaching the
/// image is not a unit declaring this directory reaching the image.
#[test]
fn a_vendor_unit_rescues_the_directory_only_when_it_declares_it() {
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        var_directories: vec![dir_with_mode(
            "/var/lib/myapp",
            VarDirBacking::StateDirectory {
                declared_by: vec![
                    "/etc/systemd/system/myapp.service".into(),
                    "/usr/lib/systemd/system/myapp.service".into(),
                ],
            },
            "0755",
            0,
            "root",
        )],
        ..Default::default()
    });
    snap.config = Some(ConfigSection {
        files: vec![config_file(
            "/etc/systemd/system/myapp.service",
            ConfigFileKind::Unowned,
            false,
        )],
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

    // Order must not decide it.
    set_declared_by(
        &mut snap,
        "/var/lib/myapp",
        &[
            "/usr/lib/systemd/system/myapp.service",
            "/etc/systemd/system/myapp.service",
        ],
    );
    assert_eq!(
        outcome_for(&effective_var_backing(&snap), "/var/lib/myapp"),
        &EffectiveBacking::Carried,
        "the verdict does not depend on the order grep printed the paths in"
    );

    // The same installed, carried vendor unit, declaring nothing. The
    // excluded `/etc` shadow is the only declarer, so the creator is
    // lost however well-carried the file of that name is.
    set_declared_by(
        &mut snap,
        "/var/lib/myapp",
        &["/etc/systemd/system/myapp.service"],
    );
    assert_eq!(
        outcome_for(&effective_var_backing(&snap), "/var/lib/myapp"),
        &EffectiveBacking::Provision {
            lost: Some("myapp.service".into())
        }
    );
    assert!(
        var_conf(&snap).contains("d /var/lib/myapp 0755 root root -"),
        "the export has to make the directory itself"
    );
}

/// `/var/lib/myapp` backed by `StateDirectory=` in `myapp.service`, with
/// the `/etc` unit excluded and one RPM-owned path carrying the same
/// basename. The question each row asks is whether that path is a unit
/// file the system manager would run once the `/etc` one is gone.
fn excluded_etc_unit_with_owner(owner_path: &str) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        var_directories: vec![dir_with_mode(
            "/var/lib/myapp",
            VarDirBacking::StateDirectory {
                // The collector's grep is recursive over the two unit
                // roots, so a nested path reaches `declared_by` exactly
                // as a top-level one does.
                declared_by: vec![
                    "/etc/systemd/system/myapp.service".into(),
                    owner_path.into(),
                ],
            },
            "0755",
            0,
            "root",
        )],
        ..Default::default()
    });
    snap.config = Some(ConfigSection {
        files: vec![config_file(
            "/etc/systemd/system/myapp.service",
            ConfigFileKind::Unowned,
            false,
        )],
    });
    snap.rpm = Some(RpmSection {
        packages_added: vec![package("myapp", true)],
        file_ownership: vec![FileOwnershipEntry {
            package_name: "myapp".into(),
            paths: vec![owner_path.into()],
        }],
        ..Default::default()
    });
    snap
}

/// What counts as the unit file that survives an exclusion, by path
/// shape.
///
/// Each row is a second declaring path beside the excluded `/etc` one.
/// The collector's grep recurses over both unit roots, so every shape
/// below really can reach `declared_by`; what the table pins is which
/// of them the export will read as a creator that makes the directory.
///
/// This table exists because the predicate behind it was narrowed three
/// times and each narrowing left another shape that satisfied it: a
/// substring test for `/systemd/` admitted user units, `/systemd/system/`
/// admitted `.wants/` links, and excluding those would have admitted
/// `.requires/` and drop-in directories next. A unit file lives at
/// exactly `<unit-dir>/<name>` and is never nested, so the rule is an
/// exact parent-directory match and the refusing rows below are all one
/// consequence of it rather than a list of special cases.
///
/// A row that changes here is a decision about what the export will
/// treat as proof that a creator reaches the image. Change it
/// deliberately.
#[test]
fn only_a_file_directly_in_a_system_unit_directory_survives_an_exclusion() {
    const CARRIED: bool = true;
    const PROVISION: bool = false;

    let cases: &[(&str, bool, &str)] = &[
        (
            "/usr/lib/systemd/system/myapp.service",
            CARRIED,
            "the vendor unit file, which is the whole point of the fallback",
        ),
        (
            "/etc/systemd/system/myapp.service",
            PROVISION,
            "the excluded file itself owns nothing the export still carries",
        ),
        (
            "/etc/systemd/system/multi-user.target.wants/myapp.service",
            PROVISION,
            "an enablement link enables a dependency and may be dangling",
        ),
        (
            "/usr/lib/systemd/system/multi-user.target.wants/myapp.service",
            PROVISION,
            "a preset link is the same, wherever it is shipped",
        ),
        (
            "/etc/systemd/system/myapp.service.requires/myapp.service",
            PROVISION,
            "a requires link is an enablement link by another name",
        ),
        (
            "/etc/systemd/system/myapp.service.d/myapp.service",
            PROVISION,
            "a drop-in directory nests under the unit directory",
        ),
        (
            "/usr/lib/systemd/user/myapp.service",
            PROVISION,
            "a user unit is run by a per-user manager, never the system one",
        ),
        (
            "/etc/systemd/user/myapp.service",
            PROVISION,
            "and that is true under /etc too",
        ),
        (
            "/usr/lib/systemd/system-generators/myapp.service",
            PROVISION,
            "an adjacent directory whose name merely starts the same way",
        ),
        (
            "/etc/containers/systemd/myapp.service",
            PROVISION,
            "a quadlet source is input to a generator, not a unit file",
        ),
        (
            "/usr/local/lib/systemd/system/myapp.service",
            PROVISION,
            "a real load-path member, left out because no RPM owns /usr/local \
             and the config walk is /etc only, so nothing can name it; this row \
             is scope, not safety",
        ),
    ];

    for (owner_path, carried, why) in cases {
        let snap = excluded_etc_unit_with_owner(owner_path);
        let want = if *carried {
            EffectiveBacking::Carried
        } else {
            EffectiveBacking::Provision {
                lost: Some("myapp.service".into()),
            }
        };
        assert_eq!(
            outcome_for(&effective_var_backing(&snap), "/var/lib/myapp"),
            &want,
            "{owner_path}: {why}"
        );
    }

    // The outcome has to reach the artifact, not just the classifier.
    let snap =
        excluded_etc_unit_with_owner("/etc/systemd/system/multi-user.target.wants/myapp.service");
    assert!(
        var_conf(&snap).contains("d /var/lib/myapp 0755 root root -"),
        "a refused shape leaves the export to make the directory itself"
    );
}

// --- the collector-shaped full shadow ----------------------------------

/// The admin shadow of `crond.service`, byte for byte as the scan
/// returned it in `services.drop_ins[].content`.
const CROND_SHADOW_CONTENT: &str = "[Unit]\n\
     Description=Command Scheduler (site override)\n\
     After=auditd.service nss-user-lookup.target systemd-user-sessions.service \
     time-sync.target ypbind.service autofs.service\n\
     \n\
     [Service]\n\
     EnvironmentFile=/etc/sysconfig/crond\n\
     ExecStart=/usr/sbin/crond -n $CRONDARGS\n\
     ExecReload=/bin/kill -HUP $MAINPID\n\
     KillMode=process\n\
     Restart=on-failure\n\
     StateDirectory=crondstate\n\
     \n\
     [Install]\n\
     WantedBy=multi-user.target\n";

/// The rationale string the collector writes beside a `FullShadow`.
const FULL_SHADOW_RATIONALE: &str = "Full shadow: this unit file in /etc/systemd/system/ \
     overrides the vendor unit in /usr/lib/systemd/system/. Base image updates to this unit \
     will be silently ignored.";

/// An admin full shadow of a vendor unit, as a real scan reports it.
///
/// **Provenance: measured, not constructed.** Every field below comes
/// from a privileged `inspectah scan --inspect-only` on the `rhel9` test
/// VM, 2026-11-01, at schema 23. The host had `driftify --profile
/// standard` applied plus one planted file: an admin-written
/// `/etc/systemd/system/crond.service` adding `StateDirectory=crondstate`
/// on top of the stock `cronie` unit, which declares no directory of its
/// own. What the scan returned:
///
/// - `storage.var_directories` carries `/var/lib/crondstate` with
///   `state_directory` naming `crond.service`.
/// - `services.drop_ins` carries the shadow at
///   `/etc/systemd/system/crond.service` with
///   `shadow_type: full_shadow`.
/// - `config.files` carries the same path as an **inventory** row whose
///   `shadowed_by` is `Services`: the walk keeps an admin-written
///   regular unit file, and the cross-inspector pass hands the one
///   include decision to `services.drop_ins`.
/// - `rpm.file_ownership` gives `cronie` exactly one path matching the
///   filename, `/usr/lib/systemd/system/crond.service`, and `cronie` is
///   an included `packages_added` entry. That file declares no
///   directory, so it is **not** in `declared_by`.
///
/// Only the package's arch and repo differ from the measured values,
/// because the shared `package()` helper supplies them and neither
/// reaches this classification.
///
/// Regenerated 2026-11-01 against a privileged `rhel9` scan on schema
/// 25. The earlier revision encoded the pre-walk-fix shape, with an
/// empty `config.files` and a bare `crond.service` filename on the
/// backing. Both are now wrong about what the collector produces.
fn crond_full_shadow_snapshot(shadow_included: bool) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        var_directories: vec![dir_with_mode(
            "/var/lib/crondstate",
            VarDirBacking::StateDirectory {
                // The shadow is the only file matching the grep.
                declared_by: vec!["/etc/systemd/system/crond.service".into()],
            },
            "0755",
            0,
            "root",
        )],
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        drop_ins: vec![SystemdDropIn {
            unit: "crond.service".into(),
            path: "/etc/systemd/system/crond.service".into(),
            content: CROND_SHADOW_CONTENT.into(),
            disposition: if shadow_included {
                FindingKind::included()
            } else {
                FindingKind::excluded()
            },
            shadow_type: Some(ShadowType::FullShadow),
            shadow_rationale: Some(FULL_SHADOW_RATIONALE.into()),
            ..Default::default()
        }],
        ..Default::default()
    });
    let mut shadow_row = config_file(
        "/etc/systemd/system/crond.service",
        ConfigFileKind::Unowned,
        false,
    );
    shadow_row.disposition = FindingKind::inventory();
    shadow_row.shadowed_by = Some(ConfigShadow::Services);
    snap.config = Some(ConfigSection {
        files: vec![shadow_row],
    });
    snap.rpm = Some(RpmSection {
        packages_added: vec![package("cronie", true)],
        file_ownership: vec![FileOwnershipEntry {
            package_name: "cronie".into(),
            paths: vec!["/usr/lib/systemd/system/crond.service".into()],
        }],
        ..Default::default()
    });
    snap
}

/// An operator exclusion of a full shadow has to reach the `/var`
/// verdict, and a vendor package shipping the same filename is not a
/// substitute for it.
///
/// The shadow is the only unit file declaring `StateDirectory=crondstate`
/// — the `cronie` unit declares no directory at all — so excluding the
/// shadow removes the directory's only creator from the image. The
/// export therefore has to say something about `/var/lib/crondstate`,
/// either provisioning it or flagging it. Which of the two is the open
/// question this test deliberately does not answer; silence is the one
/// answer it rules out.
///
/// **This test was `#[ignore]`d for measuring an open defect, and is
/// live again.** It failed while the backing recorded a bare
/// `crond.service` and the classifier resolved that filename against
/// `rpm.file_ownership`: `cronie` ships a file of that name, so its
/// installation read as proof the directory gets created, and the
/// verdict stayed `Carried` whichever way the operator set the shadow.
/// The backing now records the path that actually carried the
/// directive, and the only such path is the excluded shadow.
///
/// The assertions deliberately do not name `Provision` or `Unresolved`.
/// Which of the two an excluded sole declarer should produce is still
/// open; silence is the one answer this test rules out, and it rules it
/// out either way.
#[test]
fn an_excluded_full_shadow_is_invisible_to_var_backing_evidence() {
    let snap = crond_full_shadow_snapshot(false);
    let rendered = export(&snap);
    assert!(
        !rendered.crond_shadow_exported,
        "the excluded shadow is not in the export, so nothing it declares reaches the image"
    );
    assert_ne!(
        outcome_for(&effective_var_backing(&snap), "/var/lib/crondstate"),
        &EffectiveBacking::Carried,
        "cronie shipping a same-named unit is not evidence that a unit reaching the \
         image declares /var/lib/crondstate; the only unit that declares it is excluded"
    );
    let conf = rendered.var_conf.unwrap_or_default();
    assert!(
        conf.contains("/var/lib/crondstate"),
        "a directory whose only creator the export drops cannot go unmentioned, \
         as an active entry or as a commented one under a FIXME: {conf}"
    );

    // The control. Including the shadow carries the unit that declares
    // the directory, so `Carried` with no entry is right. The pair is
    // the point: the verdict moves with the operator's decision, which
    // is what it would not do while a same-named vendor unit could
    // stand in for the declarer.
    let snap = crond_full_shadow_snapshot(true);
    let rendered = export(&snap);
    assert!(rendered.crond_shadow_exported);
    assert_eq!(
        outcome_for(&effective_var_backing(&snap), "/var/lib/crondstate"),
        &EffectiveBacking::Carried
    );
    assert!(
        !rendered.var_conf.unwrap_or_default().contains("crondstate"),
        "the carried shadow makes the directory, so the export does not"
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

/// A package from a repo the source host keeps disabled installs only on
/// the route the Containerfile renders it by: from that repo while the
/// export carries its `.repo` file, and not at all once it does not, when
/// it has no cached or uploaded RPM. Counting it installed in the second
/// case left the directory it owns unprovisioned in an image without it.
#[test]
fn a_disabled_repo_package_is_installed_only_while_its_repo_ships() {
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        var_directories: vec![dir_with_mode(
            "/var/lib/openh264",
            VarDirBacking::RpmOwned {
                package: "openh264".into(),
            },
            "0755",
            0,
            "root",
        )],
        ..Default::default()
    });
    let mut pkg = package("openh264", true);
    pkg.source_repo = "epel-cisco-openh264".into();
    pkg.repoless_annotation =
        "No repo source \u{2014} repo 'epel-cisco-openh264' not in enabled repos".into();
    snap.rpm = Some(RpmSection {
        packages_added: vec![pkg],
        repo_files: vec![RepoFile {
            path: "/etc/yum.repos.d/fedora-cisco-openh264.repo".into(),
            content: "[epel-cisco-openh264]\nmetalink=https://example.invalid/m\nenabled=0\n"
                .into(),
            is_default_repo: false,
            disposition: FindingKind::included(),
            locked: false,
            aggregate: None,
        }],
        baseline_package_names: Some(vec!["filesystem".into()]),
        ..Default::default()
    });

    assert_eq!(
        outcome_for(&effective_var_backing(&snap), "/var/lib/openh264"),
        &EffectiveBacking::Carried,
        "it installs from its repo"
    );

    snap.rpm.as_mut().expect("rpm").repo_files[0].disposition = FindingKind::excluded();
    assert_eq!(
        outcome_for(&effective_var_backing(&snap), "/var/lib/openh264"),
        &EffectiveBacking::Provision {
            lost: Some("openh264".into())
        },
        "with no repo and no RPM it does not install"
    );
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
                    declared_by: vec!["/usr/lib/systemd/system/stated.service".into()],
                },
            ),
            dir(
                "/var/cache/cached",
                VarDirBacking::CacheDirectory {
                    declared_by: vec!["/usr/lib/systemd/system/cached.service".into()],
                },
            ),
            dir(
                "/var/log/logged",
                VarDirBacking::LogsDirectory {
                    declared_by: vec!["/usr/lib/systemd/system/logged.service".into()],
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
        files: vec![config_file(
            "/etc/tmpfiles.d/shared.conf",
            ConfigFileKind::Unowned,
            true,
        )],
    });
    let mut unbacked = fleet_host("host-a", VarDirBacking::Unbacked, "0700");
    unbacked.config = Some(ConfigSection {
        files: vec![config_file(
            "/etc/tmpfiles.d/shared.conf",
            ConfigFileKind::Unowned,
            true,
        )],
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

// --- auto packages: pulled in versus not carried -----------------------

/// A host with one included leaf, `nginx`, whose auto dependency
/// `nginx-core` owns the tmpfiles fragment that makes `/var/lib/nginx`,
/// and three auto packages nothing included needs (`rsyslog`, `audit`,
/// `insights-client`, the classes measured in the 2026-11-08 build
/// check), each owning a `/var` directory. The baseline is known and
/// holds none of them, so carriage comes from the install set alone.
///
/// Dispositions start as the collector writes them: everything included.
/// Each path then sets them the way it would.
fn auto_packages_snapshot() -> InspectionSnapshot {
    const NGINX_CONF: &str = "/usr/lib/tmpfiles.d/nginx.conf";
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        var_directories: vec![
            dir_with_mode(
                "/var/lib/nginx",
                tmpfiles(NGINX_CONF, "d /var/lib/nginx 0770 root root -"),
                "0770",
                0,
                "root",
            ),
            dir_with_mode(
                "/var/lib/rsyslog",
                VarDirBacking::RpmOwned {
                    package: "rsyslog".into(),
                },
                "0700",
                0,
                "root",
            ),
            dir_with_mode(
                "/var/log/audit",
                VarDirBacking::RpmOwned {
                    package: "audit".into(),
                },
                "0700",
                0,
                "root",
            ),
            dir_with_mode(
                "/var/lib/insights",
                VarDirBacking::RpmOwned {
                    package: "insights-client".into(),
                },
                "0755",
                0,
                "root",
            ),
        ],
        ..Default::default()
    });
    snap.rpm = Some(RpmSection {
        packages_added: ["nginx", "nginx-core", "rsyslog", "audit", "insights-client"]
            .into_iter()
            .map(|name| package(name, true))
            .collect(),
        leaf_packages: Some(vec!["nginx.x86_64".into()]),
        auto_packages: Some(vec![
            "nginx-core.x86_64".into(),
            "rsyslog.x86_64".into(),
            "audit.x86_64".into(),
            "insights-client.x86_64".into(),
        ]),
        leaf_dep_tree: serde_json::json!({ "nginx.x86_64": ["nginx-core.x86_64"] }),
        file_ownership: vec![FileOwnershipEntry {
            package_name: "nginx-core".into(),
            paths: vec![NGINX_CONF.into()],
        }],
        baseline_package_names: Some(vec!["filesystem".into()]),
        ..Default::default()
    });
    snap
}

/// The scan path: the scan's own package defaults applied.
fn scan_path(mut snap: InspectionSnapshot) -> InspectionSnapshot {
    inspectah_pipeline::render::not_carried::apply_scan_package_defaults(&mut snap);
    snap
}

/// The refine path, set by hand: refine starts every non-leaf package
/// excluded, so the leaf stays included and its auto dependencies do not.
/// The comparison against a real refine export is in the CLI parity test.
fn refine_path(mut snap: InspectionSnapshot) -> InspectionSnapshot {
    let rpm = snap.rpm.as_mut().expect("rpm");
    for pkg in &mut rpm.packages_added {
        if pkg.name != "nginx" {
            pkg.disposition = FindingKind::excluded();
        }
    }
    snap
}

#[test]
fn an_excluded_dependency_an_included_leaf_pulls_in_is_carried_on_both_paths() {
    for (label, snap) in [
        ("scan", scan_path(auto_packages_snapshot())),
        ("refine", refine_path(auto_packages_snapshot())),
    ] {
        assert_eq!(
            outcome_for(&effective_var_backing(&snap), "/var/lib/nginx"),
            &EffectiveBacking::Carried,
            "{label}: dnf installs nginx-core with nginx"
        );
        assert!(
            !var_conf(&snap).contains("/var/lib/nginx"),
            "{label}: a carried creator's directory is not provisioned again"
        );
    }
}

#[test]
fn a_directory_an_unneeded_package_owns_is_not_carried_on_the_scan_path() {
    let snap = scan_path(auto_packages_snapshot());
    let outcomes = effective_var_backing(&snap);
    for (path, owner) in [
        ("/var/lib/rsyslog", "rsyslog"),
        ("/var/log/audit", "audit"),
        ("/var/lib/insights", "insights-client"),
    ] {
        assert_eq!(
            outcome_for(&outcomes, path),
            &EffectiveBacking::Provision {
                lost: Some(owner.into())
            },
            "{path}: nothing included needs {owner}"
        );
    }
    let conf = var_conf(&snap);
    assert!(conf.contains("d /var/lib/rsyslog 0700 root root -"));
    assert!(conf.contains("d /var/log/audit 0700 root root -"));
    assert!(conf.contains("d /var/lib/insights 0755 root root -"));
}

/// `inspectah-tmpfiles-fragment-comments-contradict-the-install-list`:
/// the fragment said `/var/lib/nginx` was created by a file "this export
/// does not carry" while the same export installed `nginx`, which pulls
/// in the fragment's owner. The comment and the `d` line both follow
/// from the outcome, so neither appears.
#[test]
fn the_tmpfiles_fragment_does_not_call_a_pulled_in_creator_not_carried() {
    let conf = var_conf(&refine_path(auto_packages_snapshot()));
    assert!(
        !conf.contains("/usr/lib/tmpfiles.d/nginx.conf, which this export does not carry"),
        "the export installs nginx-core as a dependency of nginx:\n{conf}"
    );
    assert!(!conf.contains("d /var/lib/nginx "), "{conf}");
}

/// Being pulled in does not lift the repo-enabling demotion: a package
/// whose serving repo the export dropped renders as a FIXME and never
/// installs, so the directory it owns is still provisioned.
#[test]
fn a_demoted_repo_enabling_dependency_is_not_carried_by_being_pulled_in() {
    let mut snap = refine_path(auto_packages_snapshot());
    let rpm = snap.rpm.as_mut().expect("rpm");
    let core = rpm
        .packages_added
        .iter_mut()
        .find(|pkg| pkg.name == "nginx-core")
        .expect("nginx-core");
    core.repo_enabling_from = vec![inspectah_core::types::repo::RepoId::new("epel")];

    assert_eq!(
        outcome_for(&effective_var_backing(&snap), "/var/lib/nginx"),
        &EffectiveBacking::Provision {
            lost: Some("/usr/lib/tmpfiles.d/nginx.conf".into())
        }
    );
}
