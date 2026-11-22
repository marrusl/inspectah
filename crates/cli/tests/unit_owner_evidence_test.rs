//! What decides that a package owning an enabled unit is carried, and
//! what that carries with it, on a host built from collector output.
//!
//! Each case is a [`UnitHost`] the real RPM, units and config inspectors
//! scan, with the dependency answers `rpm` would give, so every field the
//! decision reads comes from the collector. Each is exported both ways:
//! the scan tarball and an unchanged refine export.
//!
//! - An enabled template instance credits its template's owner.
//! - An owner that needs a repo the image does not enable, its own, a
//!   dependency's (another architecture of a base-image package, or of a
//!   package from a repo, among them), or none, or that may need a
//!   package the host did not have, is not carried by default and is
//!   listed with the reason; included, it installs as released. So is
//!   one that needs a package from a repo refine excludes, while it is
//!   excluded, unless the operator includes it through the package, its
//!   repo or a group. A carried owner's dependency keeps its
//!   recorded `systemctl disable`, guarded.
//! - An auto package the operator includes brings its dependencies: their
//!   unit commands survive, guarded. It installs as released.
//! - The transaction that enables a disabled repo names what the leaf
//!   tree, what held on the host, says installed leaves pulled in, as
//!   released: competing providers and the dnf fallback's dependency, but
//!   never a provider only the image might pick, and nothing for a
//!   requirement the collector cannot read.
//! - An owner is carried when the collector could not tell leaves from
//!   dependencies and fell back to the dependency graph. That fallback
//!   classifies as released: a dependency cycle nothing installs is auto,
//!   and a package only a branch the image may take would require stays a
//!   root.
//! - What the image may install counts every `or` operand and both
//!   branches of a conditional, and a recommended package is never
//!   certain: each keeps its unit command, guarded.
//! - A unit whose package dnf may or may not install keeps its command,
//!   run only if the unit file is in the image: an excluded or included
//!   one of several providers, a dependency a conditional requirement
//!   brings, on one host and on a fleet. Only a package the export names
//!   is certain: excluding the only provider the host had of what an
//!   included package requires guards its command, since the base image
//!   may provide it too.
//! - A fleet names an owner one host enabled whatever the hostname order,
//!   and does not name one a kept leaf pulls in on some host. A package
//!   only some hosts have, once included, keeps its dependency's unit
//!   command, guarded, whether that dependency is auto or user-installed
//!   there.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::config::ConfigInspector;
use inspectah_collect::inspectors::rpm::RpmInspector;
use inspectah_collect::inspectors::rpm::repoless::scan_dnf_cache_for_repoless;
use inspectah_collect::inspectors::units::UnitsInspector;
use inspectah_core::baseline::{ResolutionStrategy, TargetImageIdentity};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::inspector::Inspector;
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;
use inspectah_core::types::units::UnitState;
use inspectah_pipeline::collect::collect;
use inspectah_pipeline::redaction::{RedactOptions, redact};
use inspectah_pipeline::render::not_carried::{
    apply_scan_package_defaults, not_carried_notice, not_carried_packages,
};
use inspectah_pipeline::render::tarball::create_tarball;
use inspectah_pipeline::validate::validate;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, RefinementOp};

use crate::not_carried_parity_test::{
    Surfaces, export_session, export_warnings, ok, projected_not_carried, scan_export, set_package,
    surfaces_of, unpack,
};
use crate::unit_hosts::{UnitHost, acts, guarded_commands, guards, systemctl_instructions};

/// The repos the hosts enable.
const ENABLED_REPOS: &str = "repo id    repo name\nappstream  AppStream\nbaseos     BaseOS\n";

/// The image every fleet host's scan resolved.
const FLEET_TARGET_IMAGE: &str = "registry.redhat.io/rhel9/rhel-bootc:9.4";

/// A unit file wanted by `multi-user.target`, so `enable` links it there.
const WANTED_UNIT: &str =
    "[Service]\nExecStart=/usr/bin/true\n\n[Install]\nWantedBy=multi-user.target\n";

/// A host with a base image and one user-installed leaf, `app`, which
/// needs nothing: every case adds its auto packages to it.
fn base_host() -> UnitHost {
    UnitHost::new()
        .base_package("bash")
        .base_package("glibc")
        .package("app")
        .userinstalled(&["bash", "app"])
}

/// The scan command's calls before its dispositions step: `collect`,
/// `validate`, the baseline recorded, `redact`, then the dnf cache pass
/// that marks a package from a disabled repo.
fn collected(host: UnitHost) -> InspectionSnapshot {
    collected_with_repos(host, ENABLED_REPOS)
}

/// [`collected`] on a host whose `dnf repolist --enabled` prints
/// `enabled_repos`.
fn collected_with_repos(host: UnitHost, enabled_repos: &str) -> InspectionSnapshot {
    collected_with(host, enabled_repos, |exec| exec)
}

/// [`collected_with_repos`] on a host whose executor `more` also answers.
fn collected_with(
    host: UnitHost,
    enabled_repos: &str,
    more: impl FnOnce(MockExecutor) -> MockExecutor,
) -> InspectionSnapshot {
    let baseline = host.baseline();
    let exec: MockExecutor = more(host.build())
        .with_command("dnf repolist --enabled -q", ok(enabled_repos))
        .with_command_prefix("dnf repoquery --available", ok(""))
        .with_command("find /var/cache/dnf -name *.rpm -type f", ok(""));
    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(RpmInspector::new()),
        Box::new(UnitsInspector::new()),
        Box::new(ConfigInspector::new()),
    ];
    let source = SourceSystem::PackageBased {
        os_release: OsRelease {
            name: "Red Hat Enterprise Linux".into(),
            version_id: "9.4".into(),
            id: "rhel".into(),
            ..Default::default()
        },
    };
    let collected = collect(
        &source,
        &exec,
        &inspectors,
        baseline.as_ref(),
        &NullProgress,
        &AtomicBool::new(false),
    );
    let mut snapshot = validate(collected)
        .expect("the snapshot validates")
        .state
        .snapshot;
    snapshot.baseline = baseline;
    redact(&mut snapshot, &RedactOptions::default());
    if let Some(rpm) = snapshot.rpm.as_mut() {
        scan_dnf_cache_for_repoless(&exec, rpm);
    }
    snapshot
}

/// Both exports of one host: the scan tarball's tree, and an unchanged
/// refine export of the collected snapshot.
struct Exports {
    snapshot: InspectionSnapshot,
    paths: Vec<(&'static str, Surfaces)>,
    _dirs: Vec<tempfile::TempDir>,
}

fn exports(host: UnitHost) -> Exports {
    let snapshot = collected(host);
    let scan_dir = tempfile::tempdir().expect("tempdir");
    scan_export(snapshot.clone(), scan_dir.path());
    let refine_dir = tempfile::tempdir().expect("tempdir");
    let refine_root = export_session(RefineSession::new(snapshot.clone()), refine_dir.path());
    Exports {
        paths: vec![
            ("scan", Surfaces::read(scan_dir.path())),
            ("refine", Surfaces::read(&refine_root)),
        ],
        snapshot,
        _dirs: vec![scan_dir, refine_dir],
    }
}

// ── A template instance ──────────────────────────────────────────────

/// The auto package that ships the template `worker@.service`.
const TEMPLATE_OWNER: &str = "worker";

/// `worker@blue.service` is enabled as its preset says, so the collector
/// records it in the inventory with no state change: the template's
/// vendor file is what systemd loads for it, and its owner is carried, on
/// both paths. Excluding the owner lists it with the instance and the
/// notice; including it again restores the export.
#[test]
fn an_enabled_instance_carries_its_templates_owner() {
    let host = base_host()
        .package(TEMPLATE_OWNER)
        .unit("worker@.service", "indirect")
        .vendor_unit(
            "worker@.service",
            "[Service]\nExecStart=/usr/bin/worker %i\n\n[Install]\nWantedBy=multi-user.target\n",
            Some(TEMPLATE_OWNER),
        )
        .link(
            "/etc/systemd/system/multi-user.target.wants/worker@blue.service",
            "/usr/lib/systemd/system/worker@.service",
        )
        .preset("enable worker@.service blue")
        .preset("disable *");
    let run = exports(host);

    let units = run.snapshot.units.as_ref().expect("the units section");
    assert!(
        units
            .inventory
            .iter()
            .any(|e| e.unit.as_str() == "worker@blue.service" && e.state == UnitState::Enabled),
        "the collector records the enabled instance: {:?}",
        units.inventory
    );
    assert!(
        units.state_changes.is_empty(),
        "it matches its preset, so it has no state change: {:?}",
        units.state_changes
    );

    for (label, surfaces) in &run.paths {
        assert!(
            surfaces.install_names().iter().any(|n| n == TEMPLATE_OWNER),
            "{label}: {TEMPLATE_OWNER} is installed:\n{}",
            surfaces.containerfile
        );
        assert!(
            !surfaces.not_carried_block_names().contains(&TEMPLATE_OWNER),
            "{label}:\n{}",
            surfaces.containerfile
        );
    }

    let mut session = RefineSession::new(run.snapshot.clone());
    let before = surfaces_of(&session);
    set_package(&mut session, TEMPLATE_OWNER, "x86_64", false);
    let row = session
        .decisions()
        .not_carried
        .iter()
        .find(|row| row.display_name == TEMPLATE_OWNER)
        .cloned()
        .expect("the excluded owner is listed");
    assert_eq!(row.enabled_units, ["worker@blue.service"]);
    let notice = session
        .decisions()
        .not_carried_notice
        .clone()
        .expect("an excluded owner gives a notice");
    assert!(notice.text().contains(TEMPLATE_OWNER), "{notice:?}");
    set_package(&mut session, TEMPLATE_OWNER, "x86_64", true);
    assert_eq!(surfaces_of(&session).containerfile, before.containerfile);
}

/// An instance's own unit file outranks its template: `worker@blue.service`
/// has a vendor file of its own, from `blue-files`, and
/// `worker@green.service` a local one the collector recorded with its
/// enable. Neither loads `worker@.service`, so `worker` owns no enabled
/// unit and is left out like any unneeded package, without a notice, while
/// `blue-files` is carried.
#[test]
fn an_instance_file_of_its_own_outranks_the_template() {
    let template =
        "[Service]\nExecStart=/usr/bin/worker %i\n\n[Install]\nWantedBy=multi-user.target\n";
    let host = base_host()
        .package(TEMPLATE_OWNER)
        .package("blue-files")
        .vendor_unit("worker@.service", template, Some(TEMPLATE_OWNER))
        .unit("worker@blue.service", "enabled")
        .vendor_unit("worker@blue.service", template, Some("blue-files"))
        .unit("worker@green.service", "enabled")
        .etc_unit("worker@green.service", template)
        .preset("enable worker@.service blue")
        .preset("disable *");
    let run = exports(host);

    let units = run.snapshot.units.as_ref().expect("the units section");
    let green = units
        .state_changes
        .iter()
        .find(|c| c.unit().name().as_str() == "worker@green.service")
        .expect("the local instance's enable is recorded");
    assert_eq!(green.unit().owning_package(), None, "{green:?}");

    for (label, surfaces) in &run.paths {
        assert!(
            surfaces.install_names().iter().any(|n| n == "blue-files"),
            "{label}:\n{}",
            surfaces.containerfile
        );
        assert_eq!(
            surfaces.not_carried_block(),
            [
                "# Not carried: not user-installed on the source host, and nothing this image \
                 installs needs it (1)",
                "#   worker",
            ],
            "{label}: worker is listed with no unit:\n{}",
            surfaces.containerfile
        );
    }
    let mut scanned = run.snapshot.clone();
    apply_scan_package_defaults(&mut scanned);
    assert_eq!(not_carried_packages(&scanned).len(), 1);
    assert_eq!(not_carried_notice(&scanned), None);
}

// ── A carried owner's dependencies ───────────────────────────────────

/// The repo the host has disabled, carried with a source.
const PARTNER_REPO: &str = "[partner]\nname=Partner\nbaseurl=https://example.invalid/partner\n\
     enabled=0\ngpgcheck=0\n";

/// The reason a unit owner that needs a disabled repo is not carried by
/// default.
const DISABLED_REPO_HOLD: &str = "needs a repo disabled on the host";

/// The reason a unit owner that needs a package no enabled repo serves is
/// not carried by default.
const NO_REPO_HOLD: &str = "needs a package no enabled repo serves";

/// A host where the auto package `agent` owns the user-enabled
/// `agent.service`, with `edit` deciding where it and what it needs come
/// from.
fn agent_host(edit: impl FnOnce(UnitHost) -> UnitHost) -> UnitHost {
    edit(
        base_host()
            .file("/etc/yum.repos.d/partner.repo", PARTNER_REPO)
            .unit("agent.service", "enabled")
            .vendor_unit("agent.service", WANTED_UNIT, Some("agent"))
            .preset("disable *"),
    )
}

/// The `# Not carried` line, the notice entry and the table reason of a
/// held `agent`.
fn held_agent_surfaces(reason: &str) -> (String, String, String) {
    (
        format!("#   agent (agent.service was enabled; {reason})"),
        format!("agent ({reason})"),
        format!("nothing this image installs needs it; {reason} |"),
    )
}

/// An owner the plain install line cannot install is not carried by
/// default, on both paths: no install line names it, and the notice, the
/// `# Not carried` block and the audit table name it with the reason.
/// `agent` comes from the host's disabled `partner` repo itself; or needs
/// `agent-helper`, which only `partner` serves; or needs `agent-runtime`,
/// installed from a local RPM path the export carries nothing for.
#[test]
fn an_owner_needing_a_repo_the_image_does_not_enable_is_not_carried_by_default() {
    let cases: [(&str, UnitHost, &str); 3] = [
        (
            "agent from partner",
            agent_host(|h| h.package_from("agent", "partner")),
            DISABLED_REPO_HOLD,
        ),
        (
            "agent-helper from partner",
            agent_host(|h| {
                h.package("agent")
                    .package_from("agent-helper", "partner")
                    .requires("agent", &["agent-helper"])
            }),
            DISABLED_REPO_HOLD,
        ),
        (
            "agent-runtime from a local RPM",
            agent_host(|h| {
                h.package("agent")
                    .package_from("agent-runtime", "@commandline")
                    .requires("agent", &["agent-runtime"])
            }),
            NO_REPO_HOLD,
        ),
    ];
    for (case, host, reason) in cases {
        let run = exports(host);
        let (block_line, notice_entry, table_reason) = held_agent_surfaces(reason);

        let mut scanned = run.snapshot.clone();
        apply_scan_package_defaults(&mut scanned);
        let notice = not_carried_notice(&scanned).expect("the scan notices agent");
        assert!(
            notice.text().contains(&notice_entry),
            "{case}: {}",
            notice.text()
        );
        let warnings = export_warnings(&RefineSession::new(run.snapshot.clone()));
        assert_eq!(warnings, vec![notice], "{case}: refine notices agent alike");

        for (label, surfaces) in &run.paths {
            let ctx = format!("{case}, {label}");
            assert!(
                !surfaces
                    .install_names()
                    .iter()
                    .any(|n| n.starts_with("agent")),
                "{ctx}: agent is not installed:\n{}",
                surfaces.containerfile
            );
            assert_eq!(
                partner_install(&surfaces.containerfile),
                None,
                "{ctx}:\n{}",
                surfaces.containerfile
            );
            assert!(
                surfaces.not_carried_block().contains(&block_line.as_str()),
                "{ctx}: {block_line:?}:\n{}",
                surfaces.containerfile
            );
            assert!(
                surfaces
                    .not_carried_table()
                    .iter()
                    .any(|row| row.starts_with("| agent |") && row.ends_with(&table_reason)),
                "{ctx}: {:?}",
                surfaces.not_carried_table()
            );
        }
    }
}

/// The reason a unit owner that may need a package the host did not have
/// is not carried by default.
const UNRESOLVED_HOLD: &str = "may need a package the host did not have";

/// Both paths install `app` alone, as released, enable no disabled repo,
/// and list `agent` as not carried with `reason`.
fn assert_agent_held(run: &Exports, case: &str, reason: &str) {
    let (block_line, _, _) = held_agent_surfaces(reason);
    for (label, surfaces) in &run.paths {
        let ctx = format!("{case}, {label}");
        assert_eq!(
            surfaces.install_names(),
            ["app"],
            "{ctx}: the install line is beta.6's:\n{}",
            surfaces.containerfile
        );
        assert_eq!(
            partner_install(&surfaces.containerfile),
            None,
            "{ctx}:\n{}",
            surfaces.containerfile
        );
        assert!(
            surfaces.not_carried_block().contains(&block_line.as_str()),
            "{ctx}: {block_line:?}:\n{}",
            surfaces.containerfile
        );
    }
}

/// The base image has `libvendor.x86_64`, and the host also has
/// `libvendor.i686`, the only provider of the `libvendor.so.1` that
/// `agent` requires, installed from a local RPM path or from the host's
/// disabled `partner` repo. The collector subtracts the base image by
/// `name.arch`, so `libvendor.i686` is an added package, and the plain
/// install line cannot fetch it: `agent` is held, though its name is the
/// base image's.
#[test]
fn another_arch_of_a_base_image_package_holds_its_owner() {
    for (repo, reason) in [
        ("@commandline", NO_REPO_HOLD),
        ("partner", DISABLED_REPO_HOLD),
    ] {
        let host = agent_host(|h| {
            h.package("agent")
                .base_package("libvendor")
                .package_arch("libvendor", "i686", repo, &["libvendor.so.1"])
                .requires("agent", &["libvendor.so.1"])
        });
        let run = exports(host);
        let rpm = run.snapshot.rpm.as_ref().expect("the rpm section");
        assert!(
            rpm.packages_added
                .iter()
                .any(|p| p.name == "libvendor" && p.arch == "i686" && p.source_repo == repo),
            "{repo}: {:?}",
            rpm.packages_added
        );
        assert_eq!(
            rpm.dep_closures.as_ref().expect("closures")["agent.x86_64"],
            ["libvendor.i686"],
            "{repo}"
        );
        assert_agent_held(&run, repo, reason);
    }
}

/// Neither architecture of `libvendor` is in the base image. `agent`
/// requires `libvendor.so.1()(64bit)`, which only `libvendor.x86_64`
/// provides, installed from a local RPM path; `libvendor.i686`, from
/// `appstream`, provides only the 32-bit `libvendor.so.1`, and dnf lists
/// its repo first. Source attribution is per `name.arch`, so the 64-bit
/// library keeps `@commandline` and `agent` is held: no line names it and
/// no command enables its unit.
#[test]
fn a_sibling_archs_repo_never_vouches_for_a_package() {
    let host = agent_host(|h| {
        h.package("agent")
            .package_from("libvendor", "@commandline")
            .provides("libvendor", "libvendor.so.1()(64bit)")
            .package_arch("libvendor", "i686", "appstream", &["libvendor.so.1"])
            .requires("agent", &["libvendor.so.1()(64bit)"])
    });
    let run = exports(host);
    let rpm = run.snapshot.rpm.as_ref().expect("the rpm section");
    let repo_of = |arch: &str| {
        rpm.packages_added
            .iter()
            .find(|p| p.name == "libvendor" && p.arch == arch)
            .map(|p| p.source_repo.as_str())
    };
    assert_eq!(repo_of("x86_64"), Some("@commandline"), "{rpm:?}");
    assert_eq!(repo_of("i686"), Some("appstream"), "{rpm:?}");
    assert_eq!(
        rpm.dep_closures.as_ref().expect("closures")["agent.x86_64"],
        ["libvendor.x86_64"]
    );
    assert_agent_held(&run, "64-bit libvendor from a local RPM", NO_REPO_HOLD);
    assert_no_agent_command(&run);
}

/// Neither path runs any `systemctl` command for `agent.service`, plain or
/// guarded: nothing installs `agent`.
fn assert_no_agent_command(run: &Exports) {
    for (label, surfaces) in &run.paths {
        assert!(
            !names_agent_unit(&surfaces.containerfile),
            "{label}: no command names agent.service:\n{}",
            surfaces.containerfile
        );
    }
}

/// Whether any `systemctl` instruction, plain or guarded, names
/// `agent.service`.
fn names_agent_unit(containerfile: &str) -> bool {
    systemctl_instructions(containerfile)
        .iter()
        .any(|i| i.contains("agent.service"))
}

/// The host's enabled repos, the custom `vendor` among them.
const VENDOR_ENABLED_REPOS: &str =
    "repo id    repo name\nappstream  AppStream\nbaseos     BaseOS\nvendor     Vendor\n";

/// The enabled custom repo `vendor`, carried with a source.
const VENDOR_REPO: &str = "[vendor]\nname=Vendor\nbaseurl=https://example.invalid/vendor\n\
     enabled=1\ngpgcheck=0\n";

/// The reason a unit owner is held when a package it may pull in loses
/// its repo in the export.
const REPO_EXCLUDED_HOLD: &str = "needs a package from a repo excluded in this export";

/// Where the host keeps the `vendor` repo.
const VENDOR_REPO_PATH: &str = "/etc/yum.repos.d/vendor.repo";

/// Excludes the configuration file at `path` through refine's toggle.
fn exclude_config(session: &mut RefineSession, path: &str) {
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Config { path: path.into() },
            include: false,
        })
        .unwrap_or_else(|e| panic!("exclude config {path}: {e}"));
}

/// Sets the repo `id` to `include` through refine's repo toggle.
fn set_repo(session: &mut RefineSession, id: &str, include: bool) {
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Repo { path: id.into() },
            include,
        })
        .unwrap_or_else(|e| panic!("set repo {id} include={include}: {e}"));
}

/// `agent`, from `appstream`, requires `(vendor-runtime or helper)`. The
/// host never had `vendor-runtime`; it has `helper`, which only the
/// enabled custom repo `vendor` serves. With `helper` included and then
/// `vendor` excluded in refine, along with the config entry that would
/// still stage `vendor.repo` (a file either writer stages ships, and dnf
/// can then fetch from it),
/// neither operand can be fetched, so `agent` is held on that export,
/// opened from the collected snapshot or the scan tarball, and on the
/// scan path's render of what refine exports: only `app` is installed, no
/// command names `agent.service`, and the notice gives the reason.
/// Including `vendor` again carries `agent`.
#[test]
fn excluding_a_dependencys_repo_holds_its_owner() {
    let host = agent_host(|h| {
        h.file(VENDOR_REPO_PATH, VENDOR_REPO)
            .package("agent")
            .package_from("helper", "vendor")
            .requires("agent", &["(vendor-runtime or helper)"])
    });
    let collected = collected_with_repos(host, VENDOR_ENABLED_REPOS);
    let mut scanned = collected.clone();
    apply_scan_package_defaults(&mut scanned);
    let (block_line, notice_entry, _) = held_agent_surfaces(REPO_EXCLUDED_HOLD);

    for (opened, snapshot) in [("collected", collected), ("scan tarball", scanned)] {
        let mut session = RefineSession::new(snapshot);
        // Refine leaves a dependency to dnf by default, and the repo
        // toggle refuses a repo with nothing included: the operator
        // includes `helper` first.
        let helper_included = session
            .snapshot_projected()
            .rpm
            .iter()
            .flat_map(|rpm| &rpm.packages_added)
            .any(|p| p.name == "helper" && p.disposition.is_included());
        if !helper_included {
            set_package(&mut session, "helper", "x86_64", true);
        }
        let carried = surfaces_of(&session);
        assert!(
            carried.install_names().iter().any(|n| n == "agent"),
            "{opened}: vendor included, agent is carried:\n{}",
            carried.containerfile
        );

        set_repo(&mut session, "vendor", false);
        exclude_config(&mut session, VENDOR_REPO_PATH);
        let refine = surfaces_of(&session);
        let scan_dir = tempfile::tempdir().expect("tempdir");
        scan_export(session.snapshot_projected(), scan_dir.path());
        let scan = Surfaces::read(scan_dir.path());
        for (label, surfaces) in [("refine", &refine), ("scan", &scan)] {
            let ctx = format!("{opened}, {label}");
            assert_eq!(
                surfaces.install_names(),
                ["app"],
                "{ctx}: the install line is beta.6's:\n{}",
                surfaces.containerfile
            );
            assert!(
                !names_agent_unit(&surfaces.containerfile),
                "{ctx}: no command names agent.service:\n{}",
                surfaces.containerfile
            );
            assert!(
                surfaces.not_carried_block().contains(&block_line.as_str()),
                "{ctx}: {block_line:?}:\n{}",
                surfaces.containerfile
            );
        }
        let warnings = export_warnings(&session);
        assert!(
            warnings.iter().any(|w| w.text().contains(&notice_entry)),
            "{opened}: {warnings:?}"
        );

        set_repo(&mut session, "vendor", true);
        let restored = surfaces_of(&session);
        assert_eq!(
            restored.install_names(),
            carried.install_names(),
            "{opened}: including vendor again carries agent:\n{}",
            restored.containerfile
        );
        assert!(
            acts(&restored.containerfile, "enable", "agent.service"),
            "{opened}:\n{}",
            restored.containerfile
        );
        assert!(
            !export_warnings(&session)
                .iter()
                .any(|w| w.text().contains(REPO_EXCLUDED_HOLD)),
            "{opened}"
        );
    }
}

/// `agent` requires `(vendor-runtime unless local-switch)`. The host has
/// `local-switch`, from a local RPM path, so it needs no `vendor-runtime`,
/// which it never had; the image has neither, and needs `vendor-runtime`.
/// Or `agent` has a requirement nested deeper than the collector reads.
/// Either way it may need a package the host did not have: held.
#[test]
fn a_requirement_the_host_could_not_resolve_holds_its_owner() {
    let nested = format!(
        "{}vendor-runtime{}",
        "(bash and ".repeat(17),
        ")".repeat(17)
    );
    let cases = [
        (
            "unless",
            agent_host(|h| {
                h.package("agent")
                    .package_from("local-switch", "@commandline")
                    .requires("agent", &["(vendor-runtime unless local-switch)"])
            }),
        ),
        (
            "unread",
            agent_host(|h| h.package("agent").requires("agent", &[&nested])),
        ),
    ];
    for (case, host) in cases {
        let run = exports(host);
        assert_agent_held(&run, case, UNRESOLVED_HOLD);
    }
}

/// The host's enabled repos, the custom `tools` and `vendor` among them.
const TOOLS_ENABLED_REPOS: &str = "repo id    repo name\nappstream  AppStream\nbaseos     BaseOS\n\
     tools      Tools\nvendor     Vendor\n";

/// The enabled custom repo `tools`, carried with a source.
const TOOLS_REPO: &str = "[tools]\nname=Tools\nbaseurl=https://example.invalid/tools\n\
     enabled=1\ngpgcheck=0\n";

/// The installed group whose one member is `tuned`.
const TUNING_GROUP: &str = "Tuning";

/// The installed `Tuning` group, as `dnf group list --installed` and
/// `dnf group info` print it.
fn with_tuning_group(exec: MockExecutor) -> MockExecutor {
    exec.with_command(
        "env LC_ALL=C dnf group list --installed",
        ok(&format!("Installed Groups:\n   {TUNING_GROUP}\n")),
    )
    .with_command(
        &format!("env LC_ALL=C dnf group info {TUNING_GROUP}"),
        ok(&format!(
            "Group: {TUNING_GROUP}\n Mandatory Packages:\n   tuned\n"
        )),
    )
}

/// The refine export after the operator includes `dmidecode`, so the
/// `vendor` toggle has a member, then excludes `vendor` and the config
/// entry that would still stage `vendor.repo`, then includes `item_id`.
/// Asserts `tuned` is held before that last step, and listed as included
/// after it.
fn include_after_losing_vendor(snapshot: InspectionSnapshot, item_id: ItemId) -> Surfaces {
    let via = format!("{item_id:?}");
    let mut session = RefineSession::new(snapshot);
    set_package(&mut session, "dmidecode", "x86_64", true);
    set_repo(&mut session, "vendor", false);
    exclude_config(&mut session, VENDOR_REPO_PATH);
    let held = surfaces_of(&session);
    assert!(
        held.not_carried_block()
            .iter()
            .any(|line| line.contains("tuned") && line.contains(REPO_EXCLUDED_HOLD)),
        "{via}: vendor excluded, tuned is held:\n{}",
        held.containerfile
    );
    session
        .apply(RefinementOp::SetInclude {
            item_id,
            include: true,
        })
        .unwrap_or_else(|e| panic!("include {via}: {e}"));
    let shown = session
        .view()
        .packages
        .iter()
        .find(|p| p.entry.name == "tuned")
        .unwrap_or_else(|| panic!("{via}: tuned is in the package list"));
    assert!(shown.entry.disposition.is_included(), "{via}: {shown:?}");
    surfaces_of(&session)
}

/// `tuned`, from the enabled custom repo `tools`, owns the enabled
/// `tuned.service` and recommends `dmidecode`, from the enabled custom
/// repo `vendor`; `app` needs neither. With `vendor` excluded, `tuned` is
/// held. The operator then includes `tuned` through its repository
/// `tools`, or on a host where it is the member of the group `Tuning`,
/// through that group, never by its own toggle. That inclusion stands as
/// one by the package's toggle does: through the repo the install line is
/// beta.6's, `app tuned`; through the group, the group install names it.
#[test]
fn an_owner_included_through_its_repo_or_group_is_not_held() {
    let host = || {
        base_host()
            .file("/etc/yum.repos.d/tools.repo", TOOLS_REPO)
            .file(VENDOR_REPO_PATH, VENDOR_REPO)
            .unit("tuned.service", "enabled")
            .vendor_unit("tuned.service", WANTED_UNIT, Some("tuned"))
            .preset("disable *")
            .package_from("tuned", "tools")
            .package_from("dmidecode", "vendor")
            .recommends("tuned", &["dmidecode"])
    };

    let by_package = include_after_losing_vendor(
        collected_with_repos(host(), TOOLS_ENABLED_REPOS),
        ItemId::Package {
            name: "tuned".into(),
            arch: "x86_64".into(),
        },
    );
    assert_eq!(
        by_package.install_names(),
        ["app", "tuned"],
        "package tuned: the install line is beta.6's:\n{}",
        by_package.containerfile
    );

    let by_repo = include_after_losing_vendor(
        collected_with_repos(host(), TOOLS_ENABLED_REPOS),
        ItemId::Repo {
            path: "tools".into(),
        },
    );
    assert_eq!(
        by_repo.install_names(),
        ["app", "tuned"],
        "repo tools: the install line is beta.6's:\n{}",
        by_repo.containerfile
    );

    let by_group = include_after_losing_vendor(
        collected_with(host(), TOOLS_ENABLED_REPOS, with_tuning_group),
        ItemId::Group {
            name: TUNING_GROUP.into(),
        },
    );
    assert!(
        by_group.containerfile.contains(&format!(
            "RUN dnf group install -y \\\n    \"{TUNING_GROUP}\""
        )),
        "group Tuning: the group install names it:\n{}",
        by_group.containerfile
    );
    assert!(
        !by_group
            .not_carried_block()
            .iter()
            .any(|l| l.contains("tuned")),
        "group Tuning: tuned is not held:\n{}",
        by_group.containerfile
    );
}

/// `agent` owns the user-enabled `agent.service` and needs `agent-helper`,
/// which only the host's disabled `partner` repo serves. Once the operator
/// includes `agent` in refine, it installs as any auto package an operator
/// includes does, as released: named on the plain install line, with
/// `agent-helper` left to dnf there, and its unit's `enable` guarded.
/// Nothing routes an auto package's dependency into the transaction that
/// enables `partner`, so that build cannot resolve `agent`: the known
/// limitation the default avoids.
#[test]
fn including_an_owner_that_needs_a_disabled_repo_installs_it_as_released() {
    let host = agent_host(|h| {
        h.package("agent")
            .package_from("agent-helper", "partner")
            .requires("agent", &["agent-helper"])
    });
    let snapshot = collected(host);
    let helper = snapshot
        .rpm
        .as_ref()
        .and_then(|rpm| rpm.packages_added.iter().find(|p| p.name == "agent-helper"))
        .expect("agent-helper is an added package");
    assert_eq!(helper.source_repo, "partner");
    assert!(
        !helper.repoless_annotation.is_empty(),
        "the dnf cache pass marked it: {helper:?}"
    );

    let mut session = RefineSession::new(snapshot);
    assert!(
        projected_not_carried(&session).contains(&"agent".to_string()),
        "{:?}",
        projected_not_carried(&session)
    );
    set_package(&mut session, "agent", "x86_64", true);
    let surfaces = surfaces_of(&session);
    assert!(
        surfaces.install_names().iter().any(|n| n == "agent"),
        "{}",
        surfaces.containerfile
    );
    assert!(
        !surfaces.install_names().iter().any(|n| n == "agent-helper"),
        "{}",
        surfaces.containerfile
    );
    assert_eq!(
        partner_install(&surfaces.containerfile),
        None,
        "{}",
        surfaces.containerfile
    );
    // The unit plan does not see which packages an operator included by
    // hand, so the enable runs only if the unit file is in the image.
    assert!(
        guards(&surfaces.containerfile, "enable", "agent.service"),
        "{}",
        surfaces.containerfile
    );
    assert!(
        !surfaces.not_carried_block_names().contains(&"agent"),
        "{}",
        surfaces.containerfile
    );
    assert_eq!(export_warnings(&session), Vec::new());
}

/// `daemon` owns the user-enabled `daemon.service` and needs `helper`,
/// whose `helper.service` the host disabled against its enabling preset.
/// dnf installs `helper` with the carried `daemon`, preset and all,
/// unless the base image provides what `daemon` needs, so both paths keep
/// the recorded `systemctl disable`, guarded: no line names `helper`.
#[test]
fn a_carried_owners_dependency_keeps_its_disable() {
    let host = base_host()
        .package("daemon")
        .package("helper")
        .requires("daemon", &["helper"])
        .unit("daemon.service", "enabled")
        .vendor_unit("daemon.service", WANTED_UNIT, Some("daemon"))
        .unit("helper.service", "disabled")
        .vendor_unit("helper.service", WANTED_UNIT, Some("helper"))
        .preset("enable helper.service")
        .preset("disable *");
    let run = exports(host);

    for (label, surfaces) in &run.paths {
        assert!(
            guards(&surfaces.containerfile, "disable", "helper.service"),
            "{label}: the recorded disable survives, guarded:\n{}",
            surfaces.containerfile
        );
        assert!(
            acts(&surfaces.containerfile, "enable", "daemon.service"),
            "{label}:\n{}",
            surfaces.containerfile
        );
        assert!(
            !surfaces.not_carried_block_names().contains(&"helper"),
            "{label}:\n{}",
            surfaces.containerfile
        );
    }
}

/// The names the transaction that enables `partner` installs, or `None`
/// when no transaction enables it.
fn partner_install(containerfile: &str) -> Option<Vec<String>> {
    containerfile
        .split("\n\n")
        .find(|block| block.contains("--enablerepo=partner"))
        .map(|block| {
            block
                .lines()
                .skip_while(|l| !l.starts_with("RUN dnf install"))
                .skip(1)
                .map(|l| l.trim().trim_end_matches('\\').trim().to_owned())
                .take_while(|l| !l.starts_with("&&"))
                .collect()
        })
}

/// `app` requires `transport`, which two added packages provide and only
/// the host's disabled `partner` repo serves. Neither is certain to
/// install, but both were installed on the host, so the leaf tree, which
/// the routing reads, holds both: both paths name both providers in the
/// transaction that enables `partner`, as released, and install `app` on
/// the plain line.
#[test]
fn competing_providers_from_a_disabled_repo_install_from_it() {
    let host = base_host()
        .package_from("provider-a", "partner")
        .package_from("provider-b", "partner")
        .provides("provider-a", "transport")
        .provides("provider-b", "transport")
        .requires("app", &["transport"])
        .file("/etc/yum.repos.d/partner.repo", PARTNER_REPO)
        .preset("disable *");
    let run = exports(host);
    let rpm = run.snapshot.rpm.as_ref().expect("the rpm section");
    assert_eq!(
        rpm.dep_closures.as_ref().expect("closures")["app.x86_64"],
        Vec::<String>::new(),
        "two providers: neither is certain"
    );
    assert_eq!(
        rpm.leaf_dep_tree,
        serde_json::json!({"app.x86_64": ["provider-a.x86_64", "provider-b.x86_64"]})
    );

    for (label, surfaces) in &run.paths {
        let install = partner_install(&surfaces.containerfile).unwrap_or_else(|| {
            panic!(
                "{label}: a transaction enables partner:\n{}",
                surfaces.containerfile
            )
        });
        assert_eq!(
            install,
            ["provider-a", "provider-b"],
            "{label}:\n{}",
            surfaces.containerfile
        );
        assert!(
            surfaces.install_names().iter().any(|n| n == "app"),
            "{label}:\n{}",
            surfaces.containerfile
        );
    }
}

/// The collector's dnf fallback cannot tell one provider from several, so
/// it records every dependency as one dnf may pick. Its graph is what
/// held on the host, though, so the leaf tree holds `helper`, which only
/// the disabled `partner` repo serves: both paths name `helper` in the
/// transaction that enables `partner`, as released.
#[test]
fn the_dnf_fallbacks_disabled_repo_dependency_installs_from_it() {
    let host = base_host()
        .package_from("helper", "partner")
        .requires("app", &["helper"])
        .rpm_deps_unavailable()
        .file("/etc/yum.repos.d/partner.repo", PARTNER_REPO)
        .preset("disable *");
    let run = exports(host);
    let rpm = run.snapshot.rpm.as_ref().expect("the rpm section");
    assert_eq!(
        rpm.dep_closures.as_ref().expect("closures")["app.x86_64"],
        Vec::<String>::new(),
        "the dnf route records no certain dependency"
    );
    assert_eq!(
        rpm.dep_alternatives.as_ref().expect("alternatives")["app.x86_64"],
        ["helper.x86_64"],
        "the dnf route answered"
    );

    for (label, surfaces) in &run.paths {
        assert_eq!(
            partner_install(&surfaces.containerfile),
            Some(vec!["helper".to_owned()]),
            "{label}:\n{}",
            surfaces.containerfile
        );
    }
}

/// `helper` comes only from the host's disabled `partner` repo and
/// requires `vendor-runtime`, installed from a local RPM path the export
/// carries nothing for, so the image can never install `helper`. `app`
/// requires `(primary or helper)`, or `(bash or helper)` with `bash` in
/// the base image, so the image needs no `helper`. On the host the first
/// operand satisfied it, so the leaf tree holds no `helper`: no install
/// line names it, by default or once the operator excludes it, and as
/// released no transaction enables `partner`. `app` stays on the plain
/// install line, never moved into a transaction that would expose it to
/// another version `partner` serves.
#[test]
fn a_provider_the_image_may_skip_is_never_named() {
    for requirement in ["(primary or helper)", "(bash or helper)"] {
        let host = base_host()
            .package("primary")
            .package_from("helper", "partner")
            .package_from("vendor-runtime", "@commandline")
            .requires("app", &[requirement])
            .requires("helper", &["vendor-runtime"])
            .file("/etc/yum.repos.d/partner.repo", PARTNER_REPO)
            .preset("disable *");
        let run = exports(host);
        let mut session = RefineSession::new(run.snapshot.clone());
        set_package(&mut session, "helper", "x86_64", false);
        set_package(&mut session, "vendor-runtime", "x86_64", false);
        let excluded = surfaces_of(&session);
        let paths = run
            .paths
            .iter()
            .map(|(label, surfaces)| (*label, surfaces))
            .chain(std::iter::once(("refine, helper excluded", &excluded)));
        for (label, surfaces) in paths {
            let ctx = format!("{requirement}, {label}");
            for name in ["helper", "vendor-runtime"] {
                assert!(
                    !surfaces.install_names().iter().any(|n| n == name),
                    "{ctx}: {name} is named:\n{}",
                    surfaces.containerfile
                );
            }
            assert_eq!(
                partner_install(&surfaces.containerfile),
                None,
                "{ctx}: nothing enables partner:\n{}",
                surfaces.containerfile
            );
            assert!(
                surfaces.install_names().iter().any(|n| n == "app"),
                "{ctx}: app installs on the plain line:\n{}",
                surfaces.containerfile
            );
        }
    }
}

/// `app` requires `helper` through 17 nested `and` groups, valid to rpm
/// but past the 16 the collector reads, and `helper` comes only from the
/// host's disabled `partner` repo. `helper.service` was disabled against a
/// preset that enables it. The collector cannot say what `app` needs, so
/// it takes `app` to possibly need anything: both paths keep `helper` off
/// the not-carried list and keep its recorded `disable`, guarded. The
/// routing reads the leaf tree, which records no `helper`, so as released
/// no transaction enables `partner` and that build cannot resolve `app`:
/// a known limitation.
#[test]
fn a_requirement_too_deep_to_read_keeps_its_disable_guarded() {
    let nested = format!("{}helper{}", "(bash and ".repeat(17), ")".repeat(17));
    let host = base_host()
        .package_from("helper", "partner")
        .requires("app", &[&nested])
        .file("/etc/yum.repos.d/partner.repo", PARTNER_REPO)
        .unit("helper.service", "disabled")
        .vendor_unit("helper.service", WANTED_UNIT, Some("helper"))
        .preset("enable helper.service")
        .preset("disable *");
    let run = exports(host);

    for (label, surfaces) in &run.paths {
        assert_eq!(
            partner_install(&surfaces.containerfile),
            None,
            "{label}: routed as released:\n{}",
            surfaces.containerfile
        );
        assert!(
            !surfaces.not_carried_block_names().contains(&"helper"),
            "{label}: helper may be installed:\n{}",
            surfaces.containerfile
        );
        assert!(
            guards(&surfaces.containerfile, "disable", "helper.service"),
            "{label}: the recorded disable is kept, guarded:\n{}",
            surfaces.containerfile
        );
    }
}

/// The same unread requirement, which needs only `bash`, beside two
/// unrelated auto packages from the host's disabled `partner` repo:
/// `helper` requires `support`, which requires `vendor-runtime`, installed
/// from a local RPM path the export carries nothing for. The unread
/// requirement keeps both included by default, as packages `app` may
/// pull in, but neither is a leaf and nothing names them: both paths
/// install `app` alone on the plain line, as released, and no
/// transaction enables `partner`.
#[test]
fn an_unread_requirement_names_no_optional_package() {
    let nested = format!("{}bash{}", "(bash and ".repeat(17), ")".repeat(17));
    let host = base_host()
        .package_from("helper", "partner")
        .package_from("support", "partner")
        .package_from("vendor-runtime", "@commandline")
        .requires("app", &[&nested])
        .requires("helper", &["support"])
        .requires("support", &["vendor-runtime"])
        .file("/etc/yum.repos.d/partner.repo", PARTNER_REPO)
        .preset("disable *");
    let run = exports(host);

    for (label, surfaces) in &run.paths {
        assert_eq!(
            surfaces.install_names(),
            ["app"],
            "{label}:\n{}",
            surfaces.containerfile
        );
        assert_eq!(
            partner_install(&surfaces.containerfile),
            None,
            "{label}:\n{}",
            surfaces.containerfile
        );
    }
}

// ── Commands whose package dnf may or may not install ───────────────

/// `app` requires `helper` and `(plugin if helper)`, so installing it
/// installs both, and `plugin.service` was disabled against a preset that
/// enables it. The collector does not settle a condition through another
/// requirement, so `plugin` is not certain; the recorded `disable` is
/// carried, guarded by the unit file, on both paths, never lost and never
/// written plain.
#[test]
fn a_requirement_another_requirement_makes_hold_keeps_its_disable() {
    let host = base_host()
        .package("helper")
        .package("plugin")
        .requires("app", &["helper", "(plugin if helper)"])
        .unit("plugin.service", "disabled")
        .vendor_unit("plugin.service", WANTED_UNIT, Some("plugin"))
        .preset("enable plugin.service")
        .preset("disable *");
    let run = exports(host);

    for (label, surfaces) in &run.paths {
        assert!(
            guards(&surfaces.containerfile, "disable", "plugin.service"),
            "{label}: the recorded disable is kept, guarded:\n{}",
            surfaces.containerfile
        );
        assert!(
            !acts(&surfaces.containerfile, "disable", "plugin.service"),
            "{label}:\n{}",
            surfaces.containerfile
        );
    }
}

/// The same requirements on `tool`, an auto package nothing needs. Once
/// the operator includes it, dnf installs `helper` and `plugin` with it,
/// so `plugin.service`'s recorded `disable` is carried, guarded.
#[test]
fn including_an_auto_package_by_hand_keeps_a_conditional_dependencys_disable() {
    let host = base_host()
        .package("tool")
        .package("helper")
        .package("plugin")
        .requires("tool", &["helper", "(plugin if helper)"])
        .unit("plugin.service", "disabled")
        .vendor_unit("plugin.service", WANTED_UNIT, Some("plugin"))
        .preset("enable plugin.service")
        .preset("disable *");
    let mut session = RefineSession::new(collected(host));
    set_package(&mut session, "tool", "x86_64", true);
    let surfaces = surfaces_of(&session);
    assert!(
        surfaces.install_names().iter().any(|n| n == "tool"),
        "{}",
        surfaces.containerfile
    );
    assert!(
        guards(&surfaces.containerfile, "disable", "plugin.service"),
        "the recorded disable is kept, guarded:\n{}",
        surfaces.containerfile
    );
}

/// `app` requires `transport`, which the base image provides and the auto
/// package `alt` provides too, and `alt.service` was disabled against a
/// preset that enables it. The scan keeps `alt` included, as one provider
/// `app` may pull in, but names it on no install line, and dnf need not
/// install it: a plain `systemctl disable` would fail the build on RHEL 9
/// whenever dnf leaves it out. Both paths guard the command by the unit
/// file.
#[test]
fn an_included_alternatives_disable_is_guarded() {
    let host = base_host()
        .base_package("base-provider")
        .provides("base-provider", "transport")
        .package("alt")
        .provides("alt", "transport")
        .requires("app", &["transport"])
        .unit("alt.service", "disabled")
        .vendor_unit("alt.service", WANTED_UNIT, Some("alt"))
        .preset("enable alt.service")
        .preset("disable *");
    let run = exports(host);

    for (label, surfaces) in &run.paths {
        assert!(
            !surfaces.install_names().iter().any(|n| n == "alt"),
            "{label}: alt is not named:\n{}",
            surfaces.containerfile
        );
        assert!(
            !acts(&surfaces.containerfile, "disable", "alt.service"),
            "{label}: not plain:\n{}",
            surfaces.containerfile
        );
        assert_eq!(
            guarded_commands(&surfaces.containerfile),
            [(
                "disable".to_owned(),
                "alt.service".to_owned(),
                "/usr/lib/systemd/system/alt.service".to_owned()
            )],
            "{label}:\n{}",
            surfaces.containerfile
        );
    }
}

/// `app` requires `(alternate if (missing-cap if switch))` and nothing
/// provides `missing-cap`. With `switch` installed the inner condition
/// fails, so `app` needs no `alternate`; with `switch` left out it holds.
/// The operator keeps `app` and `switch` and excludes `alternate`: nothing
/// certainly installs it, so its enable is never a plain command.
#[test]
fn a_nested_condition_never_makes_an_excluded_package_certain() {
    let host = base_host()
        .package("switch")
        .package("alternate")
        .requires("app", &["(alternate if (missing-cap if switch))"])
        .unit("alternate.service", "enabled")
        .vendor_unit("alternate.service", WANTED_UNIT, Some("alternate"))
        .preset("disable *")
        .userinstalled(&["bash", "app", "switch", "alternate"]);
    let snapshot = collected(host);
    let rpm = snapshot.rpm.as_ref().expect("the rpm section");
    assert!(
        !rpm.dep_closures.as_ref().expect("closures")["app.x86_64"]
            .contains(&"alternate.x86_64".to_owned()),
        "{:?}",
        rpm.dep_closures
    );

    let mut session = RefineSession::new(snapshot);
    set_package(&mut session, "alternate", "x86_64", false);
    let surfaces = surfaces_of(&session);
    assert!(
        !acts(&surfaces.containerfile, "enable", "alternate.service"),
        "not plain:\n{}",
        surfaces.containerfile
    );
    assert!(
        guards(&surfaces.containerfile, "enable", "alternate.service"),
        "with switch left out dnf would install it, so the enable is guarded:\n{}",
        surfaces.containerfile
    );
    assert!(
        !surfaces.install_names().iter().any(|n| n == "alternate"),
        "{}",
        surfaces.containerfile
    );
}

// ── What the image may install, beyond what the host had ────────────

/// `app` requires `(local-a or helper)`, and the host has both. `local-a`
/// came from a local RPM path, and the export carries no RPM for it, so the
/// image cannot install it: dnf satisfies `app` with `helper`, the only
/// alternative a repo serves. `helper.service` was disabled against a
/// preset that enables it. Both paths keep `helper` out of the not-carried
/// list and its recorded `disable`, guarded: which operand dnf picks is not
/// known, so the command is never plain either.
#[test]
fn an_or_operand_the_host_did_not_need_keeps_its_disable() {
    let host = base_host()
        .package_from("local-a", "@commandline")
        .package("helper")
        .requires("app", &["(local-a or helper)"])
        .unit("helper.service", "disabled")
        .vendor_unit("helper.service", WANTED_UNIT, Some("helper"))
        .preset("enable helper.service")
        .preset("disable *");
    let run = exports(host);

    for (label, surfaces) in &run.paths {
        assert!(
            guards(&surfaces.containerfile, "disable", "helper.service"),
            "{label}: the recorded disable is kept, guarded:\n{}",
            surfaces.containerfile
        );
        assert!(
            !surfaces.not_carried_block_names().contains(&"helper"),
            "{label}: helper may be installed:\n{}",
            surfaces.containerfile
        );
        assert!(
            surfaces
                .audit
                .lines()
                .any(|l| l.starts_with("**Guarded commands:**")
                    && l.ends_with("`helper.service` (disable).")),
            "{label}: the audit report names the guarded command:\n{}",
            surfaces.audit
        );
    }
}

/// `app` requires `(plugin if switch else helper)`. On the host `switch`
/// is installed, so `app` pulled in `plugin`. `switch` came from a local
/// RPM path the export carries nothing for, so the image cannot install
/// it, and there the `else` applies: dnf installs `helper`.
/// `helper.service` was disabled against a preset that enables it. Both
/// paths keep the `disable`, guarded, and do not list `helper` as not
/// carried; nor does refine once the operator excludes `switch` too.
#[test]
fn the_branch_the_image_takes_keeps_its_disable() {
    let host = base_host()
        .package_from("switch", "@commandline")
        .package("plugin")
        .package("helper")
        .requires("app", &["(plugin if switch else helper)"])
        .userinstalled(&["bash", "app", "switch"])
        .unit("helper.service", "disabled")
        .vendor_unit("helper.service", WANTED_UNIT, Some("helper"))
        .preset("enable helper.service")
        .preset("disable *");
    let run = exports(host);

    for (label, surfaces) in &run.paths {
        assert!(
            guards(&surfaces.containerfile, "disable", "helper.service"),
            "{label}: the recorded disable is kept, guarded:\n{}",
            surfaces.containerfile
        );
        assert!(
            !surfaces.not_carried_block_names().contains(&"helper"),
            "{label}: helper may be installed:\n{}",
            surfaces.containerfile
        );
    }

    let mut session = RefineSession::new(run.snapshot.clone());
    set_package(&mut session, "switch", "x86_64", false);
    let surfaces = surfaces_of(&session);
    assert!(
        guards(&surfaces.containerfile, "disable", "helper.service"),
        "switch excluded:\n{}",
        surfaces.containerfile
    );
    assert!(
        !projected_not_carried(&session).contains(&"helper".to_string()),
        "{:?}",
        projected_not_carried(&session)
    );
}

/// `app` recommends `helper`, and `helper.service` was disabled against a
/// preset that enables it. The operator excludes `helper`. dnf installs a
/// recommended package when it can and skips it when it cannot, for one a
/// repo no longer serves in the image or one an option leaves out, so
/// `helper` may be absent: a plain `systemctl disable` of a missing unit
/// fails the build on systemd 252. Both the scan's closure and the export
/// treat it as possible only, so the command is guarded.
#[test]
fn a_recommended_package_is_never_certain() {
    let host = base_host()
        .package("helper")
        .recommends("app", &["helper"])
        .unit("helper.service", "disabled")
        .vendor_unit("helper.service", WANTED_UNIT, Some("helper"))
        .preset("enable helper.service")
        .preset("disable *");
    let snapshot = collected(host);
    let rpm = snapshot.rpm.as_ref().expect("the rpm section");
    assert!(
        rpm.dep_closures.as_ref().expect("closures")["app.x86_64"].is_empty(),
        "{:?}",
        rpm.dep_closures
    );

    let mut session = RefineSession::new(snapshot);
    set_package(&mut session, "helper", "x86_64", false);
    let surfaces = surfaces_of(&session);
    assert!(
        !acts(&surfaces.containerfile, "disable", "helper.service"),
        "not plain:\n{}",
        surfaces.containerfile
    );
    assert!(
        guards(&surfaces.containerfile, "disable", "helper.service"),
        "guarded:\n{}",
        surfaces.containerfile
    );
}

// ── An auto package the operator includes ───────────────────────────

/// The unit command `helper.service` was recorded with, and the preset
/// that makes it one: disabled against a preset that enables it, or
/// enabled against one that disables it.
const HELPER_STATES: [(&str, &str, &str); 2] = [
    ("disabled", "enable helper.service", "disable"),
    ("enabled", "disable helper.service", "enable"),
];

/// `tool` is an auto package that owns no unit and that nothing needs,
/// so refine leaves it out, and its dependency `helper` with it, and
/// omits `helper.service`'s recorded command. The operator includes
/// `tool`: dnf installs `helper` with it, so the export keeps the command
/// and no longer lists `helper` as not carried. The unit plan does not see
/// which packages an operator included by hand, so it cannot tell `tool`
/// from an unnamed one, and the command runs only if the unit file is in
/// the image.
#[test]
fn including_an_auto_package_by_hand_carries_its_dependencies() {
    for (state, preset, verb) in HELPER_STATES {
        let ctx = format!("helper.service {state}");
        let host = base_host()
            .package("tool")
            .package("helper")
            .requires("tool", &["helper"])
            .unit("helper.service", state)
            .vendor_unit("helper.service", WANTED_UNIT, Some("helper"))
            .preset(preset)
            .preset("disable *");
        let mut session = RefineSession::new(collected(host));
        set_package(&mut session, "helper", "x86_64", false);
        let before = surfaces_of(&session);
        assert!(
            !acts(&before.containerfile, verb, "helper.service"),
            "{ctx}: helper is not installed:\n{}",
            before.containerfile
        );
        assert!(
            before.not_carried_block_names().contains(&"helper"),
            "{ctx}:\n{}",
            before.containerfile
        );

        set_package(&mut session, "tool", "x86_64", true);
        let after = surfaces_of(&session);
        assert!(
            after.install_names().iter().any(|n| n == "tool"),
            "{ctx}:\n{}",
            after.containerfile
        );
        assert!(
            guards(&after.containerfile, verb, "helper.service"),
            "{ctx}: the recorded {verb} survives, guarded:\n{}",
            after.containerfile
        );
        assert!(
            !after.not_carried_block_names().contains(&"helper"),
            "{ctx}:\n{}",
            after.containerfile
        );
        assert!(
            !projected_not_carried(&session).contains(&"helper".to_string()),
            "{ctx}: {:?}",
            projected_not_carried(&session)
        );
    }
}

/// `tool` needs `helper`, which only the host's disabled `partner` repo
/// serves. Once the operator includes `tool`, it installs as released:
/// named on the plain install line, with `helper` left to dnf there.
/// Nothing routes the dependency of an auto package an operator includes
/// into the transaction that enables `partner`, so that build cannot
/// resolve `tool`: a known limitation.
#[test]
fn including_an_auto_package_by_hand_routes_as_released() {
    let host = base_host()
        .package("tool")
        .package_from("helper", "partner")
        .requires("tool", &["helper"])
        .file("/etc/yum.repos.d/partner.repo", PARTNER_REPO)
        .preset("disable *");
    let mut session = RefineSession::new(collected(host));
    set_package(&mut session, "tool", "x86_64", true);
    let surfaces = surfaces_of(&session);
    assert!(
        surfaces.install_names().iter().any(|n| n == "tool"),
        "{}",
        surfaces.containerfile
    );
    assert_eq!(
        partner_install(&surfaces.containerfile),
        None,
        "{}",
        surfaces.containerfile
    );
    assert!(
        !surfaces.not_carried_block_names().contains(&"helper"),
        "{}",
        surfaces.containerfile
    );
}

// ── No leaf data from dnf ────────────────────────────────────────────

/// dnf cannot say what the operator installed, and the two added packages
/// need each other with nothing needing either, so the dependency graph
/// makes both auto, as released. `beta` owns an enabled unit, which
/// carries it whatever the leaves: both paths install it and enable its
/// unit, and list nothing.
#[test]
fn the_graph_fallback_still_carries_a_unit_owner() {
    let host = UnitHost::new()
        .base_package("bash")
        .base_package("glibc")
        .package("alpha")
        .package("beta")
        .userinstalled_unavailable()
        .requires("alpha", &["beta"])
        .requires("beta", &["alpha"])
        .unit("beta.service", "enabled")
        .vendor_unit("beta.service", WANTED_UNIT, Some("beta"))
        .preset("disable *");
    let run = exports(host);

    let rpm = run.snapshot.rpm.as_ref().expect("the rpm section");
    assert_eq!(rpm.leaf_packages.as_deref(), Some(&[][..]));
    assert_eq!(
        rpm.auto_packages.as_deref(),
        Some(&["alpha.x86_64".to_owned(), "beta.x86_64".to_owned()][..]),
        "the unrooted cycle is auto"
    );

    for (label, surfaces) in &run.paths {
        assert!(
            surfaces.install_names().iter().any(|n| n == "beta"),
            "{label}: the unit owner is installed:\n{}",
            surfaces.containerfile
        );
        assert!(
            acts(&surfaces.containerfile, "enable", "beta.service"),
            "{label}:\n{}",
            surfaces.containerfile
        );
        assert!(
            surfaces.not_carried_block().is_empty(),
            "{label}:\n{}",
            surfaces.containerfile
        );
    }
}

/// No leaf data. `alpha` and `beta` need each other with nothing needing
/// either, and `beta` requires `vendor-runtime`, installed from a local
/// RPM path the export carries nothing for. As released, the dependency
/// graph makes the root the package nothing needs, `app`, and the cycle
/// auto: both paths install `app` alone.
#[test]
fn the_graph_fallback_leaves_an_unrooted_cycle_auto() {
    let host = UnitHost::new()
        .base_package("bash")
        .base_package("glibc")
        .package("app")
        .package("alpha")
        .package("beta")
        .package_from("vendor-runtime", "@commandline")
        .userinstalled_unavailable()
        .requires("alpha", &["beta"])
        .requires("beta", &["alpha", "vendor-runtime"])
        .preset("disable *");
    let run = exports(host);

    let rpm = run.snapshot.rpm.as_ref().expect("the rpm section");
    assert_eq!(
        rpm.leaf_packages.as_deref(),
        Some(&["app.x86_64".to_owned()][..])
    );
    for (label, surfaces) in &run.paths {
        assert_eq!(
            surfaces.install_names(),
            ["app"],
            "{label}: the install line is beta.6's:\n{}",
            surfaces.containerfile
        );
    }
}

/// No leaf data. `app` requires `(alternate if (missing-cap if switch))`,
/// and nothing provides `missing-cap`, so on this host, with `switch`
/// installed, `app` needs no `alternate`; `alternate` requires `app`.
/// `alternate.service` was disabled against a preset that enables it. In
/// the image `app` may need `alternate`, but that edge never held on the
/// host, so it does not make `alternate` a dependency: `alternate` and
/// `switch` are the roots, both paths install `alternate` and keep its
/// `disable`, plain.
#[test]
fn a_branch_only_the_image_may_take_keeps_the_hosts_roots() {
    let host = UnitHost::new()
        .base_package("bash")
        .base_package("glibc")
        .package("app")
        .package("alternate")
        .package("switch")
        .userinstalled_unavailable()
        .requires("app", &["(alternate if (missing-cap if switch))"])
        .requires("alternate", &["app"])
        .unit("alternate.service", "disabled")
        .vendor_unit("alternate.service", WANTED_UNIT, Some("alternate"))
        .preset("enable alternate.service")
        .preset("disable *");
    let run = exports(host);

    let rpm = run.snapshot.rpm.as_ref().expect("the rpm section");
    assert_eq!(
        rpm.leaf_packages.as_deref(),
        Some(&["alternate.x86_64".to_owned(), "switch.x86_64".to_owned()][..])
    );
    for (label, surfaces) in &run.paths {
        assert!(
            surfaces.install_names().iter().any(|n| n == "alternate"),
            "{label}:\n{}",
            surfaces.containerfile
        );
        assert!(
            acts(&surfaces.containerfile, "disable", "alternate.service"),
            "{label}: the recorded disable is kept:\n{}",
            surfaces.containerfile
        );
    }
}

// ── A capability several packages provide ───────────────────────────

/// A host where the leaf `app` requires the capability `transport`, which
/// the user-installed `alternate` provides, and `alternate` owns
/// `alternate.service`, enabled against a preset that disables it. With
/// `competitor`, a second package provides `transport` too: a base-image
/// package, or another user-installed leaf.
fn transport_host(competitor: Option<Competitor>) -> UnitHost {
    let mut userinstalled = vec!["bash", "app", "alternate"];
    let mut host = base_host()
        .package("alternate")
        .provides("alternate", "transport")
        .requires("app", &["transport"])
        .unit("alternate.service", "enabled")
        .vendor_unit("alternate.service", WANTED_UNIT, Some("alternate"))
        .preset("disable *");
    match competitor {
        Some(Competitor::BaseImage) => {
            host = host
                .base_package("base-provider")
                .provides("base-provider", "transport");
        }
        Some(Competitor::Leaf) => {
            host = host.package("other").provides("other", "transport");
            userinstalled.push("other");
        }
        None => {}
    }
    host.userinstalled(&userinstalled)
}

/// Who else provides `transport` on a [`transport_host`].
#[derive(Debug, Clone, Copy)]
enum Competitor {
    /// A base-image package, so the image has it whatever the export does.
    BaseImage,
    /// A user-installed leaf the export installs.
    Leaf,
}

/// The operator excludes `alternate`. Another installed package also
/// provides the `transport` that `app` requires, so dnf need not install
/// `alternate`, and it is not named on the install line: its enable runs
/// only if the unit file is in the image. Included, it is enabled plainly.
#[test]
fn excluding_one_of_several_providers_guards_its_unit() {
    for competitor in [Competitor::BaseImage, Competitor::Leaf] {
        let ctx = format!("competitor: {competitor:?}");
        let mut session = RefineSession::new(collected(transport_host(Some(competitor))));
        let included = surfaces_of(&session);
        assert!(
            acts(&included.containerfile, "enable", "alternate.service"),
            "{ctx}: included, its enable is written:\n{}",
            included.containerfile
        );

        set_package(&mut session, "alternate", "x86_64", false);
        let excluded = surfaces_of(&session);
        assert!(
            !acts(&excluded.containerfile, "enable", "alternate.service"),
            "{ctx}: nothing certainly installs alternate:\n{}",
            excluded.containerfile
        );
        assert!(
            guards(&excluded.containerfile, "enable", "alternate.service"),
            "{ctx}: dnf may install it:\n{}",
            excluded.containerfile
        );
        assert!(
            !excluded.install_names().iter().any(|n| n == "alternate"),
            "{ctx}:\n{}",
            excluded.containerfile
        );
        assert!(
            excluded.install_names().iter().any(|n| n == "app"),
            "{ctx}:\n{}",
            excluded.containerfile
        );
    }
}

/// `alternate` is the only provider on the host of the `transport` that
/// the included `app` requires, and the operator excluded it. dnf
/// installs it with `app` unless the image already provides `transport`,
/// which the host cannot show, so its enable survives, guarded: only a
/// package the export names is certain. The install line is beta.6's.
#[test]
fn excluding_the_only_provider_guards_its_unit() {
    let mut session = RefineSession::new(collected(transport_host(None)));
    set_package(&mut session, "alternate", "x86_64", false);
    let excluded = surfaces_of(&session);
    assert!(
        guards(&excluded.containerfile, "enable", "alternate.service"),
        "app requires it, so dnf may install it:\n{}",
        excluded.containerfile
    );
    assert!(
        !acts(&excluded.containerfile, "enable", "alternate.service"),
        "{}",
        excluded.containerfile
    );
    assert_eq!(
        excluded.install_names(),
        ["app"],
        "{}",
        excluded.containerfile
    );
}

/// The base image has `base-provider`, which provides `transport`, and the
/// source host never had it, so the host's only provider is `alternate`.
/// With `alternate` excluded, dnf installs `app` alone, as beta.6 did, and
/// `alternate.service` is not in the image: its enable is guarded, never
/// plain.
#[test]
fn a_target_only_provider_leaves_an_excluded_dependency_guarded() {
    let host = transport_host(None).target_only_package("base-provider");
    let mut session = RefineSession::new(collected(host));
    set_package(&mut session, "alternate", "x86_64", false);
    let excluded = surfaces_of(&session);
    assert_eq!(
        excluded.install_names(),
        ["app"],
        "the install line is beta.6's:\n{}",
        excluded.containerfile
    );
    assert!(
        guards(&excluded.containerfile, "enable", "alternate.service"),
        "{}",
        excluded.containerfile
    );
    assert!(
        !acts(&excluded.containerfile, "enable", "alternate.service"),
        "a plain enable fails on the missing unit:\n{}",
        excluded.containerfile
    );
}

// ── Fleets ───────────────────────────────────────────────────────────

/// What a fleet's two exports wrote, and what `inspectah aggregate`
/// printed and the refine export of the aggregate warned.
struct Fleet {
    stderr: String,
    refine_warnings: Vec<String>,
    paths: Vec<(&'static str, Surfaces)>,
    _dir: tempfile::TempDir,
}

/// Each host scanned, packed under its name, merged by `inspectah
/// aggregate`, and the aggregate opened in refine and exported.
fn fleet(hosts: Vec<(&str, UnitHost)>) -> Fleet {
    fleet_with(hosts, |_| {})
}

/// [`fleet`], with `edit` applied to the refine session of the aggregate
/// before it is exported. The `inspectah aggregate` export is unchanged.
fn fleet_with(hosts: Vec<(&str, UnitHost)>, edit: impl FnOnce(&mut RefineSession)) -> Fleet {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut tarballs: Vec<PathBuf> = Vec::new();
    for (hostname, host) in hosts {
        let out = dir.path().join(hostname);
        // The merge takes the fleet's baseline from a host whose target
        // matches; without one a fleet proves no package absent.
        let mut snapshot = collected(host.file("/etc/hostname", &format!("{hostname}\n")));
        snapshot.target_image = Some(TargetImageIdentity {
            image_ref: FLEET_TARGET_IMAGE.into(),
            strategy: ResolutionStrategy::OsRelease,
        });
        scan_export(snapshot, &out);
        let tarball = dir.path().join(format!("{hostname}.tar.gz"));
        create_tarball(&out, &tarball, hostname).expect("the scan tarball packs");
        tarballs.push(tarball);
    }

    let aggregate = dir.path().join("aggregate.tar.gz");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_inspectah"))
        .arg("aggregate")
        .args(&tarballs)
        .arg("--output-file")
        .arg(&aggregate)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("inspectah aggregate runs");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{stderr}");

    let aggregate_root = unpack(&aggregate, &dir.path().join("aggregate"));
    let mut session =
        inspectah_refine::tarball::from_tarball(&aggregate).expect("refine opens the aggregate");
    edit(&mut session);
    let refine_warnings = export_warnings(&session)
        .iter()
        .map(|w| w.text().to_owned())
        .collect();
    let refine_dir = dir.path().join("refine");
    std::fs::create_dir_all(&refine_dir).expect("mkdir");
    let refine_root = export_session(session, &refine_dir);
    Fleet {
        stderr,
        refine_warnings,
        paths: vec![
            ("inspectah aggregate", Surfaces::read(&aggregate_root)),
            (
                "refine export of the aggregate",
                Surfaces::read(&refine_root),
            ),
        ],
        _dir: dir,
    }
}

/// A fleet host: [`base_host`] with the auto package `daemon`, which owns
/// `daemon.service`, listed in `state` under a preset that enables it.
fn daemon_host(state: &str) -> UnitHost {
    base_host()
        .package("daemon")
        .unit("daemon.service", state)
        .vendor_unit("daemon.service", WANTED_UNIT, Some("daemon"))
        .preset("enable daemon.service")
        .preset("disable *")
}

/// One host enabled `daemon.service` as its preset says, with no state
/// change, and the other disabled it. The merged inventory keeps one state
/// for the tie, the first host's by name, but the merge drops `daemon`
/// either way, so the fleet notice names it in both hostname orders: in
/// the `inspectah aggregate` output, the refine export's warnings after
/// reopening the aggregate, and both exports' block.
#[test]
fn a_fleet_names_an_owner_any_host_enabled() {
    for (first, second) in [("disabled", "enabled"), ("enabled", "disabled")] {
        let ctx = format!("host-a {first}, host-b {second}");
        let fleet = fleet(vec![
            ("host-a", daemon_host(first)),
            ("host-b", daemon_host(second)),
        ]);
        let notice_line = fleet
            .stderr
            .lines()
            .find(|l| l.starts_with("Notice:"))
            .unwrap_or_else(|| panic!("{ctx}: a notice:\n{}", fleet.stderr));
        assert!(notice_line.contains("daemon"), "{ctx}: {notice_line}");
        assert!(
            fleet.refine_warnings.iter().any(|w| w.contains("daemon")),
            "{ctx}: {:?}",
            fleet.refine_warnings
        );
        for (label, surfaces) in &fleet.paths {
            assert_eq!(
                surfaces.not_carried_block_names(),
                ["daemon"],
                "{ctx}, {label}:\n{}",
                surfaces.containerfile
            );
        }
    }
}

/// Both hosts have the leaf `app`, which needs `daemon`, an owner of the
/// enabled `daemon.service`. Host `host-a`'s operator installed `daemon`
/// too, so it is a leaf there and the collector leaves it out of `app`'s
/// recorded dependencies; on `host-b` it is a dependency. The merge drops
/// `daemon`, not being a leaf everywhere, but installing `app` installs
/// it, so neither export lists it and nothing is noticed, whichever host
/// sorts first.
#[test]
fn a_fleet_does_not_list_an_owner_a_kept_leaf_pulls_in() {
    let host = |daemon_leaf: bool| {
        let userinstalled: &[&str] = if daemon_leaf {
            &["bash", "app", "daemon"]
        } else {
            &["bash", "app"]
        };
        base_host()
            .package("daemon")
            .userinstalled(userinstalled)
            .requires("app", &["daemon"])
            .unit("daemon.service", "enabled")
            .vendor_unit("daemon.service", WANTED_UNIT, Some("daemon"))
            .preset("enable daemon.service")
            .preset("disable *")
    };
    for daemon_leaf_first in [true, false] {
        let ctx = format!("daemon a leaf on host-a: {daemon_leaf_first}");
        let fleet = fleet(vec![
            ("host-a", host(daemon_leaf_first)),
            ("host-b", host(!daemon_leaf_first)),
        ]);
        assert!(
            !fleet.stderr.lines().any(|l| l.starts_with("Notice:")),
            "{ctx}:\n{}",
            fleet.stderr
        );
        assert!(
            fleet.refine_warnings.is_empty(),
            "{ctx}: {:?}",
            fleet.refine_warnings
        );
        for (label, surfaces) in &fleet.paths {
            assert!(
                surfaces.not_carried_block().is_empty(),
                "{ctx}, {label}:\n{}",
                surfaces.containerfile
            );
            assert!(
                !surfaces
                    .not_carried_table()
                    .iter()
                    .any(|l| l.contains("daemon")),
                "{ctx}, {label}:\n{}",
                surfaces.audit
            );
        }
    }
}

/// The fixture is not vacuous: a single host built like the fleet's lists
/// nothing by default, and gives the notice once its owner is excluded,
/// so an empty fleet notice above is the merge's doing.
#[test]
fn the_fleet_hosts_notice_their_owner_once_excluded() {
    let snapshot = collected(daemon_host("enabled"));
    assert_eq!(not_carried_notice(&snapshot), None);
    let mut session = RefineSession::new(snapshot);
    set_package(&mut session, "daemon", "x86_64", false);
    assert_eq!(projected_not_carried(&session), ["daemon"]);
}

/// A fleet host with the leaf `anchor` and the package `helper`, which
/// owns `helper.service`, recorded in `state` against `preset`. With
/// `app`, the host also has the leaf `app`, which needs `helper`.
/// `helper` is a leaf when `helper_leaf`, else auto.
fn helper_host(app: bool, helper_leaf: bool, state: &str, preset: &str) -> UnitHost {
    let mut userinstalled = vec!["bash", "anchor"];
    let mut host = UnitHost::new()
        .base_package("bash")
        .base_package("glibc")
        .package("anchor")
        .package("helper")
        .unit("helper.service", state)
        .vendor_unit("helper.service", WANTED_UNIT, Some("helper"))
        .preset(preset)
        .preset("disable *");
    if app {
        host = host.package("app").requires("app", &["helper"]);
        userinstalled.push("app");
    }
    if helper_leaf {
        userinstalled.push("helper");
    }
    host.userinstalled(&userinstalled)
}

/// Only `host-a` has the leaf `app`, which needs `helper`; `helper` is on
/// both hosts. The merge keeps `app` as a package not every host has, left
/// out by default, and drops `helper`, which is no leaf everywhere. Left
/// out, `app` installs nothing, so `helper.service`'s recorded command is
/// omitted. Once the operator includes `app` in refine, dnf installs
/// `helper` with it, so the command survives and `helper` is not listed.
/// A package only some hosts have cannot show that another host's
/// package will not take its dependency's place, so the command runs
/// only if the unit file is in the image.
///
/// Both ways `helper` can be dropped: auto on both hosts, and
/// user-installed on `host-a`, where `app` needs it, but auto on `host-b`.
#[test]
fn including_a_partial_fleet_leaf_keeps_its_dependencys_unit_command() {
    for helper_leaf_on_a in [false, true] {
        for (state, preset, verb) in HELPER_STATES {
            let ctx = format!("helper a leaf on host-a: {helper_leaf_on_a}; {state}");
            let fleet = fleet_with(
                vec![
                    ("host-a", helper_host(true, helper_leaf_on_a, state, preset)),
                    ("host-b", helper_host(false, false, state, preset)),
                ],
                |session| set_package(session, "app", "x86_64", true),
            );
            let (_, unchanged) = &fleet.paths[0];
            assert!(
                !acts(&unchanged.containerfile, verb, "helper.service"),
                "{ctx}: left out, app installs no helper:\n{}",
                unchanged.containerfile
            );
            let (label, included) = &fleet.paths[1];
            assert!(
                included.install_names().iter().any(|n| n == "app"),
                "{ctx}, {label}:\n{}",
                included.containerfile
            );
            assert!(
                guards(&included.containerfile, verb, "helper.service"),
                "{ctx}, {label}: the recorded {verb} survives, guarded:\n{}",
                included.containerfile
            );
            assert!(
                !included.not_carried_block_names().contains(&"helper"),
                "{ctx}, {label}:\n{}",
                included.containerfile
            );
            assert!(
                !fleet.refine_warnings.iter().any(|w| w.contains("helper")),
                "{ctx}, {label}: {:?}",
                fleet.refine_warnings
            );
        }
    }
}

/// Both hosts are a [`transport_host`] whose base image also provides
/// `transport`. The merge keeps `app` and `alternate`, leaves on both;
/// once the operator excludes `alternate` in refine, the fleet image need
/// not install it, so its enable runs only if the unit file is there.
#[test]
fn a_fleet_guards_the_unit_of_an_excluded_alternate_provider() {
    let host = || transport_host(Some(Competitor::BaseImage));
    let fleet = fleet_with(vec![("host-a", host()), ("host-b", host())], |session| {
        set_package(session, "alternate", "x86_64", false)
    });
    let (_, unchanged) = &fleet.paths[0];
    assert!(
        acts(&unchanged.containerfile, "enable", "alternate.service"),
        "kept and included, its enable is written:\n{}",
        unchanged.containerfile
    );
    let (label, excluded) = &fleet.paths[1];
    assert!(
        !acts(&excluded.containerfile, "enable", "alternate.service"),
        "{label}: nothing certainly installs alternate:\n{}",
        excluded.containerfile
    );
    assert!(
        guards(&excluded.containerfile, "enable", "alternate.service"),
        "{label}: dnf may install it:\n{}",
        excluded.containerfile
    );
}

/// A fleet host with the leaf `anchor`, the base image, and the auto
/// package `helper`, which provides `transport` and owns `helper.service`,
/// enabled against a preset that disables it. With `app`, the host has the
/// leaf `app`, which requires `transport`, so `helper` is its one provider
/// there. With `other`, the host has the leaf `other`, which provides
/// `transport` itself.
fn transport_fleet_host(app: bool, other: bool) -> UnitHost {
    let mut userinstalled = vec!["bash", "anchor"];
    let mut host = UnitHost::new()
        .base_package("bash")
        .base_package("glibc")
        .package("anchor")
        .package("helper")
        .provides("helper", "transport")
        .unit("helper.service", "enabled")
        .vendor_unit("helper.service", WANTED_UNIT, Some("helper"))
        .preset("disable *");
    if app {
        host = host.package("app").requires("app", &["transport"]);
        userinstalled.push("app");
    }
    if other {
        host = host.package("other").provides("other", "transport");
        userinstalled.push("other");
    }
    host.userinstalled(&userinstalled)
}

/// `app` is only on `host-a`, where `helper` is the one provider of the
/// `transport` it requires; `other`, only on `host-b`, provides
/// `transport` itself. Once the operator includes both, dnf installing
/// `app` beside `other` need not install `helper`, which no host's
/// closure can show, so `helper.service`'s enable is guarded, not plain.
#[test]
fn a_fleet_guards_a_dependency_another_hosts_package_may_replace() {
    let fleet = fleet_with(
        vec![
            ("host-a", transport_fleet_host(true, false)),
            ("host-b", transport_fleet_host(false, true)),
        ],
        |session| {
            set_package(session, "app", "x86_64", true);
            set_package(session, "other", "x86_64", true);
        },
    );
    let (label, included) = &fleet.paths[1];
    for name in ["app", "other"] {
        assert!(
            included.install_names().iter().any(|n| n == name),
            "{label}: {name} is installed:\n{}",
            included.containerfile
        );
    }
    assert!(
        !acts(&included.containerfile, "enable", "helper.service"),
        "{label}: not plain:\n{}",
        included.containerfile
    );
    assert!(
        guards(&included.containerfile, "enable", "helper.service"),
        "{label}: guarded:\n{}",
        included.containerfile
    );
}
