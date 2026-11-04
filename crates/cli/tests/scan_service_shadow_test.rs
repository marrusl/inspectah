//! A path that `services.drop_ins` owns has exactly one include
//! decision, and the export agrees with it.
//!
//! `render::configtree` subtracts every service-owned path from the
//! generic config tree **unconditionally**, while the drop-ins writer
//! skips excluded entries. Before schema 24 the config entry over the
//! same path was an ordinary actionable finding, so the operator had two
//! live toggles over one carriage decision: an included config entry
//! with an excluded drop-in wrote the file to neither tree while its
//! checkbox stayed checked, and an excluded config entry with an
//! included drop-in shipped it anyway. Neither emitted a warning.
//!
//! These run the `scan` command's own path — `collect()` then `validate`
//! then `redact`, the three calls `scan.rs` makes — against collector
//! output, because the repository has no fixture with a non-empty
//! `services.drop_ins` and a hand-built one would be free to encode a
//! layout the collectors cannot produce.

use std::sync::atomic::AtomicBool;

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::config::ConfigInspector;
use inspectah_collect::inspectors::rpm::RpmInspector;
use inspectah_collect::inspectors::services::ServicesInspector;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::inspector::Inspector;
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;
use inspectah_pipeline::collect::collect;
use inspectah_pipeline::redaction::{RedactOptions, redact};
use inspectah_pipeline::render;
use inspectah_pipeline::validate::validate;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, RefineError, RefinementOp};

/// A `*.service.d/*.conf` drop-in: the field's namesake.
const DROP_IN: &str = "/etc/systemd/system/httpd.service.d/limits.conf";
/// A full unit shadow. `services.drop_ins` owns this too, which is why
/// the `ConfigShadow` variant is named `Services` and not `ServiceDropIn`.
const FULL_SHADOW: &str = "/etc/systemd/system/custom.service";
/// A config file no other section owns — the negative control that keeps
/// the assertions below from passing because config rows never decide.
const PLAIN_CONFIG: &str = "/etc/httpd/conf/httpd.conf";

/// `httpd.service` sits **at** its preset, so the services inspector
/// writes it to `preset_matched_units` and `state_changes` has no row
/// for it. That is the common case on a real host and the one the
/// drop-in guard used to read as "parent excluded", which made the
/// drop-in toggle move only one way. `custom.service` diverges, so the
/// two parents between them cover both sides of the guard.
const SYSTEMCTL: &str = "UNIT FILE                 STATE     PRESET\n\
                         httpd.service             enabled   enabled\n\
                         custom.service            enabled   disabled\n\
                         \n\
                         2 unit files listed.\n";
const PRESET: &str = "enable httpd.service\ndisable custom.service\n";

/// `DROP_IN`'s parent: at its preset, so it has no `state_changes` row.
const PRESET_MATCHED_UNIT: &str = "httpd.service";
/// `FULL_SHADOW`'s parent: diverges from its preset, so it does.
const PRESET_DIVERGING_UNIT: &str = "custom.service";

const DROP_IN_CONTENT: &str = "[Service]\nLimitNOFILE=65535\n";
const FULL_SHADOW_CONTENT: &str =
    "[Unit]\nDescription=Local custom unit\n\n[Service]\nExecStart=/usr/local/bin/custom\n";
const PLAIN_CONFIG_CONTENT: &str = "ServerRoot \"/etc/httpd\"\n";

fn pkg_source() -> SourceSystem {
    SourceSystem::PackageBased {
        os_release: OsRelease {
            name: "Red Hat Enterprise Linux".into(),
            version_id: "9.4".into(),
            id: "rhel".into(),
            ..Default::default()
        },
    }
}

fn ok(stdout: &str) -> ExecResult {
    ExecResult {
        stdout: stdout.into(),
        exit_code: 0,
        ..Default::default()
    }
}

/// A host carrying one drop-in, one full unit shadow, and one ordinary
/// config file. The `/etc` walk reaches all three: drop-ins are
/// explicitly not excluded from it, and a **regular** file directly
/// under `/etc/systemd/system/` is an admin-written unit rather than
/// systemd's own link state.
fn overlapping_paths_mock() -> MockExecutor {
    MockExecutor::new()
        // -- RPM prerequisite for the config walker --------------------
        .with_command(
            "rpm -qa --queryformat %{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n",
            ok("0:bash-5.2.26-3.el9.x86_64\n"),
        )
        .with_command("rpm -Va", ok(""))
        .with_command(
            "rpm -qa --queryformat [%{NAME}\\t%{FILENAMES}\\n]",
            ok("bash\t/usr/bin/bash\n"),
        )
        // -- /etc walk -------------------------------------------------
        .with_dir("/etc", vec!["systemd", "httpd"])
        .with_dir("/etc/systemd", vec!["system"])
        .with_dir(
            "/etc/systemd/system",
            vec!["httpd.service.d", "custom.service"],
        )
        .with_dir("/etc/systemd/system/httpd.service.d", vec!["limits.conf"])
        .with_file(DROP_IN, DROP_IN_CONTENT)
        .with_file(FULL_SHADOW, FULL_SHADOW_CONTENT)
        .with_dir("/etc/httpd", vec!["conf"])
        .with_dir("/etc/httpd/conf", vec!["httpd.conf"])
        .with_file(PLAIN_CONFIG, PLAIN_CONFIG_CONTENT)
        // -- services --------------------------------------------------
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ok(SYSTEMCTL),
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file("/usr/lib/systemd/system-preset/90-default.preset", PRESET)
        // `custom.service` in /etc shadows the vendor unit in /usr/lib,
        // so the full-shadow scan records it in `services.drop_ins`.
        .with_command("test -f /etc/systemd/system/custom.service", ok(""))
        .with_command("test -f /usr/lib/systemd/system/custom.service", ok(""))
        .with_command("test -d /etc/systemd/system/httpd.service.d", ok(""))
}

/// `collect()` then `validate` then `redact` — the three calls the scan
/// command makes, in its order.
fn scan_like(exec: &MockExecutor) -> InspectionSnapshot {
    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(RpmInspector::new()),
        Box::new(ConfigInspector::new()),
        Box::new(ServicesInspector::new()),
    ];
    let collected = collect(
        &pkg_source(),
        exec,
        &inspectors,
        None,
        &NullProgress,
        &AtomicBool::new(false),
    );
    let validated = validate(collected).expect("the snapshot validates");
    let mut snapshot = validated.state.snapshot;
    redact(&mut snapshot, &RedactOptions::default());
    snapshot
}

/// The snapshot as it is written: serialized, so the assertions read the
/// wire form the refine session and every downstream consumer load.
fn written(snapshot: &InspectionSnapshot) -> serde_json::Value {
    serde_json::to_value(snapshot).expect("the snapshot serializes")
}

fn config_entry<'a>(written: &'a serde_json::Value, path: &str) -> &'a serde_json::Value {
    written["config"]["files"]
        .as_array()
        .expect("the config section carries files")
        .iter()
        .find(|f| f["path"] == path)
        .unwrap_or_else(|| panic!("no config entry for {path}"))
}

fn set_include(item_id: ItemId, include: bool) -> RefinementOp {
    RefinementOp::SetInclude { item_id, include }
}

/// What the export did with one path: where it was staged, and whether
/// the Containerfile carries the directory that would copy it in.
struct Carriage {
    in_config_tree: bool,
    in_drop_ins_tree: bool,
    containerfile_copies_drop_ins: bool,
}

impl Carriage {
    /// The file reaches the image on exactly one route, or on none.
    fn reaches_image(&self) -> bool {
        assert!(
            !(self.in_config_tree && self.in_drop_ins_tree),
            "a path staged into both trees would be copied twice"
        );
        if self.in_drop_ins_tree {
            assert!(
                self.containerfile_copies_drop_ins,
                "a staged drop-in that the Containerfile never copies is a file \
                 written nowhere the build can see"
            );
        }
        self.in_config_tree || self.in_drop_ins_tree
    }
}

fn carriage_of(snapshot: &InspectionSnapshot, path: &str) -> Carriage {
    let dir = tempfile::tempdir().expect("tempdir");
    render::render_all(snapshot, &RenderContext { target: None }, dir.path()).expect("render");
    let rel = path.trim_start_matches('/');
    let containerfile =
        std::fs::read_to_string(dir.path().join("Containerfile")).expect("Containerfile");
    Carriage {
        in_config_tree: dir.path().join("config").join(rel).exists(),
        in_drop_ins_tree: dir.path().join("drop-ins").join(rel).exists(),
        containerfile_copies_drop_ins: containerfile
            .contains("COPY drop-ins/etc/systemd/system/ /etc/systemd/system/"),
    }
}

/// Apply a `SetInclude` and report whether the session accepted it as a
/// decision. A refusal must be the explicit `InventoryNotToggleable`
/// error and must not extend the timeline: a silent no-op would leave a
/// surface free to render a live control whose activation does nothing.
fn accepted(session: &mut RefineSession, item_id: ItemId, include: bool) -> bool {
    let before = session.timeline_len();
    match session.apply(set_include(item_id.clone(), include)) {
        Ok(_) => true,
        Err(RefineError::InventoryNotToggleable(_)) => {
            assert_eq!(
                session.timeline_len(),
                before,
                "a refused op must not extend the timeline"
            );
            false
        }
        other => panic!(
            "SetInclude({item_id:?}, {include}) must be accepted or refused as \
                         inventory, got {other:?}"
        ),
    }
}

// ---------------------------------------------------------------------

/// The collector hands downstream a single decision for the overlapping
/// path, on the services side, and says in the snapshot where it went.
#[test]
fn a_service_owned_config_entry_is_inventory_shadowed_by_services() {
    let snapshot = scan_like(&overlapping_paths_mock());

    let drop_in_paths: Vec<&str> = snapshot
        .services
        .as_ref()
        .expect("the services section")
        .drop_ins
        .iter()
        .map(|d| d.path.as_str())
        .collect();
    assert!(
        drop_in_paths.contains(&DROP_IN) && drop_in_paths.contains(&FULL_SHADOW),
        "the collector must put both the drop-in and the full shadow in \
         services.drop_ins, got {drop_in_paths:?}"
    );

    let written = written(&snapshot);
    for path in [DROP_IN, FULL_SHADOW] {
        let entry = config_entry(&written, path);
        assert_eq!(
            entry["shadowed_by"], "services",
            "{path} is carried by services.drop_ins, got {entry}"
        );
        assert_eq!(
            entry["disposition"]["kind"], "inventory",
            "a shadowed entry is inventory, got {entry}"
        );
    }

    let plain = config_entry(&written, PLAIN_CONFIG);
    assert!(
        plain.get("shadowed_by").is_none(),
        "a file no other section owns is not shadowed, got {plain}"
    );
    assert_eq!(
        plain["disposition"]["kind"], "actionable",
        "an unshadowed entry stays actionable, got {plain}"
    );
}

/// The wire contract the web UI reads. A bare `Inventory` badge on the
/// Config Files row would tell the operator the control is gone without
/// saying where it went, so `shadowed_by` has to reach the client, and
/// the owner row has to be findable from it.
#[test]
fn the_view_response_names_the_owner_of_a_shadowed_config_row() {
    let snapshot = scan_like(&overlapping_paths_mock());
    let session = RefineSession::new(snapshot);
    let view = serde_json::to_value(inspectah_web::adapter::build_web_view(&session))
        .expect("the view response serializes");

    let row = view["config_files"]
        .as_array()
        .expect("config_files")
        .iter()
        .find(|c| c["entry"]["path"] == DROP_IN)
        .unwrap_or_else(|| panic!("no config row for {DROP_IN}"));
    assert_eq!(
        row["entry"]["shadowed_by"], "services",
        "the config row must name its owner, got {row}"
    );
    assert_eq!(row["entry"]["disposition"]["kind"], "inventory");

    // `service_dropins[].path` is the same string, which is what makes
    // the owner row reachable from the shadowed row.
    assert!(
        view["service_dropins"]
            .as_array()
            .expect("service_dropins")
            .iter()
            .any(|d| d["path"] == DROP_IN),
        "the owner row must be keyed by the same path, got {}",
        view["service_dropins"]
    );
}

/// All four combinations of the two dispositions that used to exist over
/// one overlapping path, driven through the session the web and CLI
/// surfaces share, and read back off the staged tree **and** the
/// Containerfile.
///
/// The assertion is the product contract rather than a fixed table:
/// exactly one of the two toggles is a decision, and the export agrees
/// with it. Before schema 24 both were decisions, and the pair
/// (config included, drop-in excluded) staged the file into neither tree
/// while reporting it as included — an accepted include that the export
/// contradicted.
#[test]
fn one_accepted_decision_governs_an_overlapping_path() {
    let snapshot = scan_like(&overlapping_paths_mock());

    for path in [DROP_IN, FULL_SHADOW] {
        for config_include in [true, false] {
            for drop_in_include in [true, false] {
                let mut session = RefineSession::new(snapshot.clone());

                let mut decisions = Vec::new();
                if accepted(
                    &mut session,
                    ItemId::Config { path: path.into() },
                    config_include,
                ) {
                    decisions.push(("Config", config_include));
                }
                if accepted(
                    &mut session,
                    ItemId::DropIn { path: path.into() },
                    drop_in_include,
                ) {
                    decisions.push(("DropIn", drop_in_include));
                }

                let carriage = carriage_of(&session.snapshot_projected(), path);
                assert!(
                    !carriage.in_config_tree,
                    "{path} is subtracted from the config tree unconditionally, \
                     so it must never be staged there"
                );

                // Every decision the session accepted is a promise about
                // the export. Two accepted decisions over one path can
                // contradict each other, and before schema 24 they did:
                // (config included, drop-in excluded) staged the file
                // into neither tree while the config row reported it
                // included.
                for (surface, include) in &decisions {
                    assert_eq!(
                        carriage.reaches_image(),
                        *include,
                        "{path}: the {surface} surface accepted include={include}, \
                         and the export disagrees (config toggle {config_include}, \
                         drop-in toggle {drop_in_include}, staged under drop-ins/ \
                         {}, Containerfile copies drop-ins/ {})",
                        carriage.in_drop_ins_tree,
                        carriage.containerfile_copies_drop_ins
                    );
                }

                assert_eq!(
                    decisions,
                    vec![("DropIn", drop_in_include)],
                    "{path} must offer exactly one include decision, on the \
                     section that carries it (config toggle {config_include}, \
                     drop-in toggle {drop_in_include})"
                );
            }
        }
    }
}

/// The negative control. Without it every assertion above would hold
/// just as well if config rows never decided anything at all.
#[test]
fn an_unowned_config_file_still_decides_its_own_carriage() {
    let snapshot = scan_like(&overlapping_paths_mock());

    for include in [true, false] {
        let mut session = RefineSession::new(snapshot.clone());
        assert!(
            accepted(
                &mut session,
                ItemId::Config {
                    path: PLAIN_CONFIG.into()
                },
                include,
            ),
            "a config file no other section owns is toggleable"
        );
        let carriage = carriage_of(&session.snapshot_projected(), PLAIN_CONFIG);
        assert_eq!(
            carriage.in_config_tree, include,
            "{PLAIN_CONFIG} is staged exactly when its own decision says so"
        );
    }
}

/// The premise the two tests below rest on, asserted against collector
/// output rather than assumed: a unit at its preset state has no
/// `state_changes` row, and a unit that diverges does.
#[test]
fn only_a_preset_diverging_unit_gets_a_state_change_row() {
    let snapshot = scan_like(&overlapping_paths_mock());
    let services = snapshot.services.as_ref().expect("the services section");

    let changed: Vec<&str> = services
        .state_changes
        .iter()
        .map(|s| s.unit.as_str())
        .collect();
    assert_eq!(
        changed,
        vec![PRESET_DIVERGING_UNIT],
        "only the preset-diverging unit gets a state change row"
    );
    assert!(
        services
            .preset_matched_units
            .iter()
            .any(|u| u == PRESET_MATCHED_UNIT),
        "the preset-matched unit goes to preset_matched_units, got {:?}",
        services.preset_matched_units
    );
}

/// The defect this file's drop-in fixture used to have to work around.
///
/// `DROP_IN`'s parent is at its preset, so it has no `state_changes`
/// row. The guard on `SetInclude` used to require the parent to be an
/// *included* state change, which absence can never satisfy, so the
/// operator could switch the file off and never back on. Since the
/// Configuration Files row over the same path became inventory, this
/// toggle is the only control over the file, and a control that moves
/// one way loses the file for the rest of the session.
#[test]
fn a_drop_in_over_a_preset_matched_parent_toggles_both_ways() {
    let snapshot = scan_like(&overlapping_paths_mock());
    let mut session = RefineSession::new(snapshot);

    for include in [false, true, false, true] {
        session
            .apply(set_include(
                ItemId::DropIn {
                    path: DROP_IN.into(),
                },
                include,
            ))
            .unwrap_or_else(|e| {
                panic!(
                    "SetInclude(DropIn {DROP_IN}, {include}) must be accepted \
                     when the parent took no decision, got {e:?}"
                )
            });
        assert_eq!(
            carriage_of(&session.snapshot_projected(), DROP_IN).reaches_image(),
            include,
            "the export must follow the drop-in's own decision (include={include})"
        );
    }
}

/// The guard, kept where it means something. An explicit exclusion of
/// the parent cascades onto its drop-ins, and re-including one of them
/// alone would undo that decision for a single file.
#[test]
fn a_drop_in_cannot_contradict_an_explicit_parent_exclusion() {
    let snapshot = scan_like(&overlapping_paths_mock());
    let mut session = RefineSession::new(snapshot);
    let parent = ItemId::Service {
        unit: PRESET_DIVERGING_UNIT.into(),
    };

    session
        .apply(set_include(parent.clone(), false))
        .expect("excluding the parent service is a decision the session takes");
    assert!(
        !carriage_of(&session.snapshot_projected(), FULL_SHADOW).reaches_image(),
        "excluding the parent cascades onto its drop-ins"
    );

    let before = session.timeline_len();
    match session.apply(set_include(
        ItemId::DropIn {
            path: FULL_SHADOW.into(),
        },
        true,
    )) {
        Err(RefineError::BadRequest(msg)) => {
            assert!(
                msg.contains("parent service is excluded"),
                "the refusal must name the parent exclusion, got {msg}"
            );
            assert_eq!(
                session.timeline_len(),
                before,
                "a refused op must not extend the timeline"
            );
        }
        other => panic!(
            "re-including a drop-in under an explicitly excluded parent must \
             be refused, got {other:?}"
        ),
    }

    session
        .apply(set_include(parent, true))
        .expect("re-including the parent service is a decision the session takes");
    assert!(
        carriage_of(&session.snapshot_projected(), FULL_SHADOW).reaches_image(),
        "re-including the parent cascades back onto its drop-ins"
    );
}
