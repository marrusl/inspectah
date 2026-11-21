//! What decides that a package owning an enabled unit is carried, and
//! what that carries with it, on a host built from collector output.
//!
//! Each case is a [`UnitHost`] the real RPM, units and config inspectors
//! scan, with the dependency answers `rpm` would give, so every field the
//! decision reads comes from the collector. Each is exported both ways:
//! the scan tarball and an unchanged refine export.
//!
//! - An enabled template instance credits its template's owner.
//! - A carried owner's dependency from a disabled repo is installed from
//!   that repo, and a carried owner's dependency keeps its recorded
//!   `systemctl disable`.
//! - An auto package the operator includes brings its dependencies: their
//!   unit commands survive, guarded, and one a disabled repo serves
//!   installs from it.
//! - Every provider an installed package may need from a disabled repo
//!   installs from it: competing providers, and the dnf fallback's.
//! - An owner is carried when the collector could not tell leaves from
//!   dependencies and fell back to the dependency graph, and that fallback
//!   keeps the roots the host had: a dependency cycle nothing installs,
//!   and a package only a branch the image may take would require.
//! - What the image may install counts every `or` operand and both
//!   branches of a conditional, and a recommended package is never
//!   certain: each keeps its unit command, guarded.
//! - A unit whose package dnf may or may not install keeps its command,
//!   run only if the unit file is in the image: an excluded or included
//!   one of several providers, a dependency a conditional requirement
//!   brings, on one host and on a fleet. Excluding the only provider an
//!   included package requires keeps it plain.
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

use crate::not_carried_parity_test::{
    Surfaces, export_session, export_warnings, ok, projected_not_carried, scan_export, set_package,
    surfaces_of, unpack,
};
use crate::unit_hosts::{UnitHost, acts, guarded_commands, guards};

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
    let baseline = host.baseline();
    let exec: MockExecutor = host
        .build()
        .with_command("dnf repolist --enabled -q", ok(ENABLED_REPOS))
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

/// `agent` owns the user-enabled `agent.service` and needs `agent-helper`,
/// which only the host's disabled `partner` repo serves. Both paths
/// carry `agent` and install `agent-helper` from that repo, in the
/// transaction that enables it; without it, the image build cannot
/// resolve `agent`.
#[test]
fn a_carried_owners_dependency_installs_from_its_disabled_repo() {
    let host = base_host()
        .package("agent")
        .package_from("agent-helper", "partner")
        .requires("agent", &["agent-helper"])
        .file("/etc/yum.repos.d/partner.repo", PARTNER_REPO)
        .unit("agent.service", "enabled")
        .vendor_unit("agent.service", WANTED_UNIT, Some("agent"))
        .preset("disable *");
    let run = exports(host);

    let helper = run
        .snapshot
        .rpm
        .as_ref()
        .and_then(|rpm| rpm.packages_added.iter().find(|p| p.name == "agent-helper"))
        .expect("agent-helper is an added package");
    assert_eq!(helper.source_repo, "partner");
    assert!(
        !helper.repoless_annotation.is_empty(),
        "the dnf cache pass marked it: {helper:?}"
    );

    for (label, surfaces) in &run.paths {
        let install = surfaces
            .containerfile
            .split("\n\n")
            .find(|block| block.contains("--enablerepo=partner"))
            .unwrap_or_else(|| {
                panic!(
                    "{label}: a transaction enables partner:\n{}",
                    surfaces.containerfile
                )
            });
        assert!(
            install
                .lines()
                .any(|l| l.trim().trim_end_matches('\\').trim() == "agent-helper"),
            "{label}: agent-helper installs from partner:\n{}",
            surfaces.containerfile
        );
        assert!(
            surfaces.install_names().iter().any(|n| n == "agent"),
            "{label}:\n{}",
            surfaces.containerfile
        );
        assert!(
            surfaces.not_carried_block().is_empty(),
            "{label}: nothing is left out:\n{}",
            surfaces.containerfile
        );
    }
}

/// `daemon` owns the user-enabled `daemon.service` and needs `helper`,
/// whose `helper.service` the host disabled against its enabling preset.
/// dnf installs `helper` with the carried `daemon`, preset and all, so
/// both paths keep the recorded `systemctl disable`.
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
            acts(&surfaces.containerfile, "disable", "helper.service"),
            "{label}: the recorded disable survives:\n{}",
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
                .skip(1)
                .map(|l| l.trim().trim_end_matches('\\').trim().to_owned())
                .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("&&"))
                .collect()
        })
}

/// `app` requires `transport`, which two added packages provide and only
/// the host's disabled `partner` repo serves. Neither is certain to
/// install, but one has to: both paths name both in the transaction that
/// enables `partner`, as the source host had them, or the build cannot
/// resolve `app`.
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

    for (label, surfaces) in &run.paths {
        let install = partner_install(&surfaces.containerfile).unwrap_or_else(|| {
            panic!(
                "{label}: a transaction enables partner:\n{}",
                surfaces.containerfile
            )
        });
        for provider in ["provider-a", "provider-b"] {
            assert!(
                install.iter().any(|n| n == provider),
                "{label}: {provider} installs from partner: {install:?}\n{}",
                surfaces.containerfile
            );
        }
    }
}

/// The collector's dnf fallback cannot tell one provider from several, so
/// it records every dependency as one dnf may pick. `helper`, which only
/// the disabled `partner` repo serves, is still the one `app` needs: both
/// paths install it from that repo.
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
        let install = partner_install(&surfaces.containerfile).unwrap_or_else(|| {
            panic!(
                "{label}: a transaction enables partner:\n{}",
                surfaces.containerfile
            )
        });
        assert!(
            install.iter().any(|n| n == "helper"),
            "{label}: helper installs from partner: {install:?}\n{}",
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
/// serves. Once the operator includes `tool`, the export installs
/// `helper` from that repo, in the transaction that enables it; without
/// it, the image build cannot resolve `tool`.
#[test]
fn including_an_auto_package_by_hand_installs_its_disabled_repo_dependency() {
    let host = base_host()
        .package("tool")
        .package_from("helper", "partner")
        .requires("tool", &["helper"])
        .file("/etc/yum.repos.d/partner.repo", PARTNER_REPO)
        .preset("disable *");
    let mut session = RefineSession::new(collected(host));
    set_package(&mut session, "tool", "x86_64", true);
    let surfaces = surfaces_of(&session);
    let install = surfaces
        .containerfile
        .split("\n\n")
        .find(|block| block.contains("--enablerepo=partner"))
        .unwrap_or_else(|| panic!("a transaction enables partner:\n{}", surfaces.containerfile));
    assert!(
        install
            .lines()
            .any(|l| l.trim().trim_end_matches('\\').trim() == "helper"),
        "helper installs from partner:\n{}",
        surfaces.containerfile
    );
    assert!(
        surfaces.install_names().iter().any(|n| n == "tool"),
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
/// need each other with nothing needing either, so one of them was
/// installed by someone and which is not known: the dependency graph keeps
/// both as leaves. `beta` owns an enabled unit: both paths install it and
/// enable its unit, and list nothing.
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
    assert_eq!(
        rpm.leaf_packages.as_deref(),
        Some(&["alpha.x86_64".to_owned(), "beta.x86_64".to_owned()][..]),
        "the unrooted cycle is kept"
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

/// The inverse: `alternate` is the only provider of the `transport` that
/// the included `app` requires, so dnf installs it with `app` although the
/// operator excluded it, and its enable survives.
#[test]
fn excluding_the_only_provider_keeps_its_unit() {
    let mut session = RefineSession::new(collected(transport_host(None)));
    set_package(&mut session, "alternate", "x86_64", false);
    let excluded = surfaces_of(&session);
    assert!(
        acts(&excluded.containerfile, "enable", "alternate.service"),
        "app requires it, so dnf installs it:\n{}",
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
