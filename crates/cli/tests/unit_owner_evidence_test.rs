//! What decides that a package owning an enabled unit is carried, and
//! what that carries with it, on a host built from collector output.
//!
//! Each case is a [`UnitHost`] the real RPM, units and config inspectors
//! scan, with the dependency answers dnf would give, so every field the
//! decision reads comes from the collector. Each is exported both ways:
//! the scan tarball and an unchanged refine export.
//!
//! - An enabled template instance credits its template's owner.
//! - A carried owner's dependency from a disabled repo is installed from
//!   that repo, and a carried owner's dependency keeps its recorded
//!   `systemctl disable`.
//! - An owner is carried when the collector could not tell leaves from
//!   dependencies and fell back to the dependency graph.
//! - A fleet names an owner one host enabled whatever the hostname order,
//!   and does not name one a kept leaf pulls in on some host.

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
use crate::unit_hosts::{UnitHost, acts};

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

// ── No leaf data from dnf ────────────────────────────────────────────

/// dnf cannot say what the operator installed, and the two added packages
/// need each other, so the dependency graph makes both auto, no leaf, and
/// an empty tree: too little to leave any auto package out. `beta` owns an
/// enabled unit, which that does not change: both paths install it and
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
    assert_eq!(rpm.leaf_packages.as_deref(), Some(&[][..]), "no leaf");
    assert_eq!(
        rpm.leaf_dep_tree,
        serde_json::json!({}),
        "the tree is empty while auto packages are not: the fallback"
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
    let session =
        inspectah_refine::tarball::from_tarball(&aggregate).expect("refine opens the aggregate");
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
