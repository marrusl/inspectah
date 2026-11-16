//! The units section through the scan path, on collector-built hosts.
//!
//! Every test here runs the real RPM, config, units and scheduled-task
//! inspectors over a [`UnitHost`] through the scan command's own calls
//! (`collect()`, then `validate`, then `redact`), and follows the snapshot
//! to the surface it asserts on: the rendered export, a refine session, or
//! that session's export. A hand-built `UnitSection` would be free to
//! encode a layout the collectors cannot produce.

#[path = "support/unit_hosts.rs"]
mod unit_hosts;

use inspectah_core::aggregate::merge_snapshots;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::FindingKind;
use inspectah_core::types::completeness::Completeness;
use inspectah_core::types::units::{
    AdminStorageFragment, CarriedUnit, InstallUnknown, LinkRelation, ManualLink, ManualLinkCause,
    MountUnitFinding, PresetDefault, UnitFragment, UnitName, UnitState,
};
use inspectah_pipeline::render;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, RefineError, RefinementOp};
use unit_hosts::{
    CONTINUED_MOUNT_TOKEN, UnitHost, continued_mount_host, export_session, reload, render_export,
    scan, scan_unredacted, session,
};

// ---------------------------------------------------------------------
// T10: the mount source is copied into no field (§ 6.5, § 6.7).
// ---------------------------------------------------------------------

/// A token the generic secret pattern matches (`token=...`), planted in a
/// local mount unit's `What=`.
const MOUNT_TOKEN: &str = "t10-mount-source-7f3a";
const MOUNT_UNIT: &str = "srv-data.mount";
const MOUNT_UNIT_PATH: &str = "/etc/systemd/system/srv-data.mount";

/// A host with one local mount unit, enabled, whose `What=` holds a
/// credential.
fn t10_host() -> UnitHost {
    UnitHost::new()
        .unit(MOUNT_UNIT, "enabled")
        .preset("disable *")
        .etc_unit(
            MOUNT_UNIT,
            &format!(
                "[Unit]\nDescription=Data volume\n\n[Mount]\n\
                 What=https://files.example/volume?token={MOUNT_TOKEN}\n\
                 Where=/srv/data\nOptions=defaults,nofail\n\n\
                 [Install]\nWantedBy=multi-user.target\n"
            ),
        )
}

/// The one mount finding for `MOUNT_UNIT`.
fn mount_finding(snap: &InspectionSnapshot) -> &MountUnitFinding {
    let units = snap.units.as_ref().expect("the units section");
    let found: Vec<_> = units
        .mount_findings
        .iter()
        .filter(|f| f.unit.name().as_unit_name().as_str() == MOUNT_UNIT)
        .collect();
    assert_eq!(found.len(), 1, "{:?}", units.mount_findings);
    found[0]
}

/// The finding records the mount point and `nofail` from the file, and
/// nothing of `What=`.
fn assert_finding_fields(finding: &MountUnitFinding, route: &str) {
    assert_eq!(
        finding.unit.fragment(),
        &AdminStorageFragment::Local,
        "{route}"
    );
    assert_eq!(
        finding.state_change.map(|(state, _)| state),
        Some(UnitState::Enabled),
        "{route}"
    );
    assert_eq!(finding.mount_point.as_deref(), Some("/srv/data"), "{route}");
    assert_eq!(finding.nofail, Some(true), "{route}");
}

/// Every file under `dir` whose text holds `needle`.
fn files_holding(dir: &std::path::Path, needle: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            for entry in std::fs::read_dir(&path).expect("the export lists") {
                pending.push(entry.expect("an entry").path());
            }
        } else if std::fs::read(&path)
            .is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains(needle))
        {
            found.push(path.display().to_string());
        }
    }
    found
}

/// T10, scan route. Presence first: before redaction the token is in the
/// snapshot, in the unit file's config entry, and the finding exists. After
/// the scan's redaction the token is absent from the whole serialized
/// snapshot, from the reloaded one, and from every file of the rendered
/// export, while the finding keeps its fields.
#[test]
fn t10_the_mount_source_is_in_no_snapshot_after_scan() {
    let unredacted = scan_unredacted(t10_host());
    assert_finding_fields(mount_finding(&unredacted), "unredacted");
    let config = unredacted.config.as_ref().expect("the config section");
    let entry = config
        .files
        .iter()
        .find(|f| f.path == MOUNT_UNIT_PATH)
        .expect("the unit file is a config entry");
    assert!(entry.content.contains(MOUNT_TOKEN), "{}", entry.content);
    let written = serde_json::to_string(&unredacted).expect("the snapshot serializes");
    assert!(written.contains(MOUNT_TOKEN));

    let scanned = scan(t10_host());
    assert_finding_fields(mount_finding(&scanned), "scan");
    let written = serde_json::to_string(&scanned).expect("the snapshot serializes");
    assert!(!written.contains(MOUNT_TOKEN), "{written}");

    let reloaded = reload(&scanned);
    assert_finding_fields(mount_finding(&reloaded), "reload");
    let written = serde_json::to_string(&reloaded).expect("the snapshot serializes");
    assert!(!written.contains(MOUNT_TOKEN));

    let export = render_export(&scanned);
    assert!(
        export.path().join("inspection-snapshot.json").is_file(),
        "the export holds the snapshot"
    );
    assert_eq!(
        files_holding(export.path(), MOUNT_TOKEN),
        Vec::<String>::new()
    );
}

/// T10, refine route: a session loaded from the scan tarball and exported
/// through the production export path. The exported snapshot holds the
/// finding with its fields, and no file of the export holds the token.
/// The view-response half belongs with the finding's view row.
#[test]
fn t10_the_mount_source_is_in_no_snapshot_after_a_refine_export() {
    let refine = session(&scan(t10_host()));
    let export = export_session(&refine);
    let json = std::fs::read_to_string(export.path().join("inspection-snapshot.json"))
        .expect("the export holds the snapshot");
    let exported = InspectionSnapshot::load(&json).expect("the exported snapshot loads");
    assert_finding_fields(mount_finding(&exported), "refine export");
    assert!(!json.contains(MOUNT_TOKEN));
    assert_eq!(
        files_holding(export.path(), MOUNT_TOKEN),
        Vec::<String>::new()
    );
}

/// T10, a continued `What=`: the line after it reads like a second
/// `Where=` holding a token, and is the rest of the mount source as systemd
/// reads it. Presence first: before redaction the token is in the unit
/// file's config entry. The finding records the unit's real mount point,
/// so after the scan's redaction the token is in no field of the
/// serialized snapshot, the reloaded one, or any file of the export.
#[test]
fn t10_a_continued_mount_source_is_in_no_snapshot_after_scan() {
    let unredacted = scan_unredacted(continued_mount_host());
    assert_finding_fields(mount_finding(&unredacted), "unredacted");
    let config = unredacted.config.as_ref().expect("the config section");
    let entry = config
        .files
        .iter()
        .find(|f| f.path == MOUNT_UNIT_PATH)
        .expect("the unit file is a config entry");
    assert!(
        entry.content.contains(CONTINUED_MOUNT_TOKEN),
        "{}",
        entry.content
    );

    let scanned = scan(continued_mount_host());
    assert_finding_fields(mount_finding(&scanned), "scan");
    let written = serde_json::to_string(&scanned).expect("the snapshot serializes");
    assert!(!written.contains(CONTINUED_MOUNT_TOKEN), "{written}");

    let reloaded = reload(&scanned);
    assert_finding_fields(mount_finding(&reloaded), "reload");
    let written = serde_json::to_string(&reloaded).expect("the snapshot serializes");
    assert!(!written.contains(CONTINUED_MOUNT_TOKEN));

    let export = render_export(&scanned);
    assert!(
        export.path().join("inspection-snapshot.json").is_file(),
        "the export holds the snapshot"
    );
    assert_eq!(
        files_holding(export.path(), CONTINUED_MOUNT_TOKEN),
        Vec::<String>::new()
    );
}

/// T10, a continued `What=`, refine route: the exported snapshot keeps the
/// finding with its real mount point, and no file of the refine export
/// holds the token.
#[test]
fn t10_a_continued_mount_source_is_in_no_snapshot_after_a_refine_export() {
    let refine = session(&scan(continued_mount_host()));
    let export = export_session(&refine);
    let json = std::fs::read_to_string(export.path().join("inspection-snapshot.json"))
        .expect("the export holds the snapshot");
    let exported = InspectionSnapshot::load(&json).expect("the exported snapshot loads");
    assert_finding_fields(mount_finding(&exported), "refine export");
    assert!(!json.contains(CONTINUED_MOUNT_TOKEN));
    assert_eq!(
        files_holding(export.path(), CONTINUED_MOUNT_TOKEN),
        Vec::<String>::new()
    );
}

// ---------------------------------------------------------------------
// A path that `units.drop_ins` owns has exactly one include decision,
// and the export agrees with it.
//
// `render::configtree` subtracts every unit-owned path from the generic
// config tree **unconditionally**, while the drop-ins writer skips
// excluded entries. Before schema 24 the config entry over the same path
// was an ordinary actionable finding, so the operator had two live
// toggles over one carriage decision: an included config entry with an
// excluded drop-in wrote the file to neither tree while its checkbox
// stayed checked, and an excluded config entry with an included drop-in
// shipped it anyway. Neither emitted a warning.
// ---------------------------------------------------------------------

/// A `*.service.d/*.conf` drop-in: the field's namesake.
const DROP_IN: &str = "/etc/systemd/system/httpd.service.d/limits.conf";
/// A full unit shadow. `units.drop_ins` owns this too, which is why
/// the `ConfigShadow` variant is named `Services` and not `ServiceDropIn`.
const FULL_SHADOW: &str = "/etc/systemd/system/custom.service";
/// A config file no other section owns: the negative control that keeps
/// the assertions below from passing because config rows never decide.
const PLAIN_CONFIG: &str = "/etc/httpd/conf/httpd.conf";

/// `DROP_IN`'s parent: at its preset, so it has no `state_changes` row.
const PRESET_MATCHED_UNIT: &str = "httpd.service";
/// `FULL_SHADOW`'s parent: diverges from its preset, so it does.
const PRESET_DIVERGING_UNIT: &str = "custom.service";

const DROP_IN_CONTENT: &str = "[Service]\nLimitNOFILE=65535\n";
const FULL_SHADOW_CONTENT: &str = "[Unit]\nDescription=Local custom unit\n\n[Service]\n\
     ExecStart=/usr/local/bin/custom\n\n[Install]\nWantedBy=multi-user.target\n";
const PLAIN_CONFIG_CONTENT: &str = "ServerRoot \"/etc/httpd\"\n";

/// A host carrying one drop-in, one full unit shadow, and one ordinary
/// config file. The `/etc` walk reaches all three: drop-ins are
/// explicitly not excluded from it, and a **regular** file directly
/// under `/etc/systemd/system/` is an admin-written unit rather than
/// systemd's own link state. `custom.service` in /etc shadows the vendor
/// unit in /usr/lib, so the full-shadow scan records it in
/// `units.drop_ins`.
///
/// `httpd.service` sits **at** its preset, so the services inspector
/// writes it to the inventory as preset-matched and `state_changes` has no row
/// for it. That is the common case on a real host and the one the
/// drop-in guard used to read as "parent excluded", which made the
/// drop-in toggle move only one way. `custom.service` diverges, so the
/// two parents between them cover both sides of the guard.
fn overlapping_host() -> UnitHost {
    UnitHost::new()
        .unit("httpd.service", "enabled")
        .unit("custom.service", "enabled")
        .preset("enable httpd.service")
        .preset("disable custom.service")
        .file(DROP_IN, DROP_IN_CONTENT)
        .file(FULL_SHADOW, FULL_SHADOW_CONTENT)
        .file(PLAIN_CONFIG, PLAIN_CONFIG_CONTENT)
        .vendor_unit(
            "custom.service",
            "[Unit]\nDescription=Vendor custom unit\n\n[Install]\nWantedBy=multi-user.target\n",
            None,
        )
        .vendor_unit(
            "httpd.service",
            "[Unit]\nDescription=Vendor httpd unit\n\n[Install]\nWantedBy=multi-user.target\n",
            None,
        )
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
    let snapshot = scan(overlapping_host());

    let drop_in_paths: Vec<&str> = snapshot
        .units
        .as_ref()
        .expect("the services section")
        .drop_ins
        .iter()
        .map(|d| d.path.as_str())
        .collect();
    assert!(
        drop_in_paths.contains(&DROP_IN) && drop_in_paths.contains(&FULL_SHADOW),
        "the collector must put both the drop-in and the full shadow in \
         units.drop_ins, got {drop_in_paths:?}"
    );

    let written = written(&snapshot);
    for path in [DROP_IN, FULL_SHADOW] {
        let entry = config_entry(&written, path);
        assert_eq!(
            entry["shadowed_by"], "units",
            "{path} is carried by units.drop_ins, got {entry}"
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
    let snapshot = scan(overlapping_host());
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
        row["entry"]["shadowed_by"], "units",
        "the config row must name its owner, got {row}"
    );
    assert_eq!(row["entry"]["disposition"]["kind"], "inventory");

    // `unit_dropins[].path` is the same string, which is what makes
    // the owner row reachable from the shadowed row.
    assert!(
        view["unit_dropins"]
            .as_array()
            .expect("unit_dropins")
            .iter()
            .any(|d| d["path"] == DROP_IN),
        "the owner row must be keyed by the same path, got {}",
        view["unit_dropins"]
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
/// while reporting it as included: an accepted include that the export
/// contradicted.
#[test]
fn one_accepted_decision_governs_an_overlapping_path() {
    let snapshot = scan(overlapping_host());

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
    let snapshot = scan(overlapping_host());

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
    let snapshot = scan(overlapping_host());
    let services = snapshot.units.as_ref().expect("the services section");

    let changed: Vec<&str> = services
        .state_changes
        .iter()
        .map(|s| s.unit().name().as_str())
        .collect();
    assert_eq!(
        changed,
        vec![PRESET_DIVERGING_UNIT],
        "only the preset-diverging unit gets a state change row"
    );
    assert!(
        services
            .inventory
            .iter()
            .any(|e| e.unit.as_str() == PRESET_MATCHED_UNIT && e.matches_preset()),
        "the preset-matched unit is a preset-matched inventory entry, got {:?}",
        services.inventory
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
    let snapshot = scan(overlapping_host());
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
    let snapshot = scan(overlapping_host());
    let mut session = RefineSession::new(snapshot);
    let parent = ItemId::Unit {
        unit: PRESET_DIVERGING_UNIT
            .to_string()
            .try_into()
            .expect("the fixture unit name is valid"),
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

// ---------------------------------------------------------------------
// § 10.1 and § 10.2: the unit plan through the scan and refine routes.
// ---------------------------------------------------------------------

/// The unit state section's header.
const UNIT_SECTION: &str = "# === Systemd Unit State ===";

/// The `Containerfile` of an export tree.
fn containerfile_in(tree: &tempfile::TempDir) -> String {
    std::fs::read_to_string(tree.path().join("Containerfile")).expect("Containerfile")
}

/// The unit state section's lines, without its header.
fn unit_lines(containerfile: &str) -> Vec<String> {
    containerfile
        .lines()
        .skip_while(|l| *l != UNIT_SECTION)
        .skip(1)
        .take_while(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Every `systemctl` instruction in `containerfile`: each non-comment
/// instruction joined with its `\` continuation lines, kept when it runs
/// `systemctl`, wherever in the file it is.
fn systemctl_instructions(containerfile: &str) -> Vec<String> {
    let mut instructions = Vec::new();
    let mut current = String::new();
    for line in containerfile.lines() {
        if current.is_empty() && line.trim_start().starts_with('#') {
            continue;
        }
        let line = line.trim_end();
        current.push_str(line.trim_end_matches('\\'));
        current.push(' ');
        if !line.ends_with('\\') {
            let instruction = std::mem::take(&mut current);
            if instruction.contains("systemctl") {
                instructions.push(instruction);
            }
        }
    }
    if current.contains("systemctl") {
        instructions.push(current);
    }
    instructions
}

/// Whether a `RUN systemctl <verb> --` instruction in `containerfile`
/// names `unit` as one of its quoted operands after `--`.
fn acts(containerfile: &str, verb: &str, unit: &str) -> bool {
    let prefix = format!("RUN systemctl {verb} -- ");
    let operand = format!("'{unit}'");
    systemctl_instructions(containerfile).iter().any(|i| {
        i.strip_prefix(&prefix)
            .is_some_and(|operands| operands.split_whitespace().any(|w| w == operand))
    })
}

/// Whether `unit`'s name appears in no `systemctl` instruction of
/// `containerfile`, whatever the verb and in any form: quoted, bare or
/// inside another word. The absence half of [`acts`], so a regression that
/// wrote a bare name fails it.
fn in_no_systemctl_instruction(containerfile: &str, unit: &str) -> bool {
    systemctl_instructions(containerfile)
        .iter()
        .all(|i| !i.contains(unit))
}

/// The `# Omitted:` line for `unit`, if any.
fn omitted<'a>(containerfile: &'a str, unit: &str) -> Option<&'a str> {
    let prefix = format!("# Omitted: {unit} (");
    containerfile.lines().find(|l| l.starts_with(&prefix))
}

/// The state change for `unit`.
fn change<'a>(
    snap: &'a InspectionSnapshot,
    unit: &str,
) -> &'a inspectah_core::types::units::UnitStateChange {
    snap.units
        .as_ref()
        .expect("a units section")
        .state_changes
        .iter()
        .find(|c| c.unit().name().as_str() == unit)
        .unwrap_or_else(|| panic!("no state change for {unit}"))
}

/// The fragment `unit` was recorded with.
fn fragment_of<'a>(snap: &'a InspectionSnapshot, unit: &str) -> &'a UnitFragment {
    match change(snap, unit).unit() {
        CarriedUnit::Ordinary(ordinary) => ordinary.fragment(),
        CarriedUnit::PackagedStorage(_) => panic!("{unit} is a storage unit"),
    }
}

/// A fresh session over `snap` with `ops` applied in order, exported.
fn exported_after(snap: &InspectionSnapshot, ops: &[(ItemId, bool)]) -> tempfile::TempDir {
    let mut session = session(snap);
    for (item_id, include) in ops {
        session
            .apply(set_include(item_id.clone(), *include))
            .unwrap_or_else(|e| panic!("SetInclude({item_id:?}, {include}): {e:?}"));
    }
    export_session(&session)
}

fn config_item(path: &str) -> ItemId {
    ItemId::Config { path: path.into() }
}

fn unit_item(unit: &str) -> ItemId {
    ItemId::Unit {
        unit: unit.to_owned().try_into().expect("a unit name"),
    }
}

/// The `InstallEvidenceChanged` text for `path`.
fn evidence_changed(path: &str) -> String {
    format!(
        "the [Install] section in the image would not be the one recorded on the source host \
         ({path}))"
    )
}

/// The `UnitFileNotStaged` text.
const NOT_STAGED: &str = "its unit file is not in the export)";

/// T5 and T6: `cockpit.socket` enabled against a `disable` preset, its
/// vendor file owned by the listed package `cockpit-ws`, on a host scanned
/// against a base image, so refine sorts `cockpit-ws` as user-added and
/// keeps it included as the scan does.
fn t6_host() -> UnitHost {
    UnitHost::new()
        .base_package("glibc")
        .package("cockpit-ws")
        .unit("cockpit.socket", "enabled")
        .preset("disable cockpit.socket")
        .vendor_unit(
            "cockpit.socket",
            "[Socket]\nListenStream=9090\n[Install]\nWantedBy=sockets.target\n",
            Some("cockpit-ws"),
        )
}

/// T6: the scan yields the enable. With the owner excluded through refine
/// and nothing else changed, the export yields the `# Omitted:` line for
/// the excluded package and no enable.
#[test]
fn t6_an_excluded_owner_omits_the_socket_enable() {
    let snap = scan(t6_host());
    assert!(
        snap.rpm
            .as_ref()
            .is_some_and(|rpm| rpm.packages_added.iter().any(|p| p.name == "cockpit-ws")),
        "the owner is a package of the scan"
    );
    let scanned = containerfile_in(&render_export(&snap));
    assert!(acts(&scanned, "enable", "cockpit.socket"), "{scanned}");
    assert!(omitted(&scanned, "cockpit.socket").is_none(), "{scanned}");

    let excluded = containerfile_in(&exported_after(
        &snap,
        &[(
            ItemId::Package {
                name: "cockpit-ws".into(),
                arch: "x86_64".into(),
            },
            false,
        )],
    ));
    assert_eq!(
        omitted(&excluded, "cockpit.socket"),
        Some("# Omitted: cockpit.socket (package 'cockpit-ws' was excluded)"),
        "{excluded}"
    );
    assert!(
        in_no_systemctl_instruction(&excluded, "cockpit.socket"),
        "{excluded}"
    );
}

/// Plan review focus 4: with no toggle changed, the scan export and a
/// refine export of the same snapshot carry the same unit lines.
///
/// Claimed for a baseline-backed snapshot, as `not_carried_parity_test`
/// claims its parity: with no baseline, refine sorts every added package
/// into its Investigate tier and excludes it on load while the scan keeps
/// it, so the owner's presence differs before the plan reads it.
#[test]
fn the_scan_and_refine_exports_carry_the_same_unit_lines() {
    let snap = scan(t6_host());
    let scanned = unit_lines(&containerfile_in(&render_export(&snap)));
    assert_eq!(
        scanned,
        ["RUN systemctl enable -- \\", "    'cockpit.socket'"],
        "the scan carries the enable"
    );
    let refined = unit_lines(&containerfile_in(&export_session(&session(&snap))));
    assert_eq!(scanned, refined);
}

/// Focus 4 on the common scan, with no baseline: a local unit's lines do
/// not depend on any package's default, so the scan export and a refine
/// export with no toggle carry the same unit lines.
#[test]
fn the_scan_and_refine_exports_carry_the_same_unit_lines_with_no_baseline() {
    let snap = scan(t15_host());
    assert!(snap.baseline.is_none(), "the scan has no baseline");
    assert!(
        snap.rpm
            .as_ref()
            .is_some_and(|rpm| rpm.baseline_package_names.is_none()),
        "the package section records no baseline"
    );
    let scanned = unit_lines(&containerfile_in(&render_export(&snap)));
    assert_eq!(
        scanned,
        ["RUN systemctl enable -- \\", "    'app.socket'"],
        "the scan carries the enable"
    );
    let refined = unit_lines(&containerfile_in(&export_session(&session(&snap))));
    assert_eq!(scanned, refined);
}

const LOCAL_UNIT: &str = "/etc/systemd/system/local.service";
const ORPHAN_UNIT: &str = "/etc/systemd/system/orphan.service";

/// T5, render half: a local unit and a vendor unit whose owner lookup
/// fails, enabled against a `disable` preset on one host.
fn t5_host() -> UnitHost {
    UnitHost::new()
        .unit("local.service", "enabled")
        .unit("orphan.service", "enabled")
        .preset("disable *")
        .etc_unit(
            "local.service",
            "[Service]\nExecStart=/usr/local/bin/local\n[Install]\nWantedBy=multi-user.target\n",
        )
        .vendor_unit(
            "orphan.service",
            "[Service]\nExecStart=/usr/bin/orphan\n[Install]\nWantedBy=multi-user.target\n",
            None,
        )
}

/// T5, render half, from one scan: the two units get different fragments
/// and render differently. The local unit's enable needs its own file:
/// declined with `UnitFileNotStaged` while its config entry is excluded,
/// carried when it is staged. The vendor unit with no known owner is
/// enabled by Tier 1, and declined with `InstallEvidenceChanged` when
/// another row (an imported config entry) stages a unit file at its name;
/// excluding that row brings the enable back.
#[test]
fn t5_a_local_unit_and_an_unowned_vendor_unit_render_differently() {
    let snap = scan(t5_host());
    assert!(
        matches!(
            fragment_of(&snap, "local.service"),
            UnitFragment::Local { .. }
        ),
        "{:?}",
        fragment_of(&snap, "local.service")
    );
    assert!(
        matches!(
            fragment_of(&snap, "orphan.service"),
            UnitFragment::Vendor {
                owning_package: None,
                ..
            }
        ),
        "{:?}",
        fragment_of(&snap, "orphan.service")
    );
    let scanned = containerfile_in(&render_export(&snap));
    assert!(acts(&scanned, "enable", "local.service"), "{scanned}");
    assert!(acts(&scanned, "enable", "orphan.service"), "{scanned}");

    // The local unit follows its file.
    let excluded = containerfile_in(&exported_after(&snap, &[(config_item(LOCAL_UNIT), false)]));
    assert!(
        in_no_systemctl_instruction(&excluded, "local.service"),
        "{excluded}"
    );
    assert!(
        omitted(&excluded, "local.service").is_some_and(|l| l.ends_with(NOT_STAGED)),
        "{excluded}"
    );
    assert!(acts(&excluded, "enable", "orphan.service"), "{excluded}");
    let restored = containerfile_in(&exported_after(
        &snap,
        &[
            (config_item(LOCAL_UNIT), false),
            (config_item(LOCAL_UNIT), true),
        ],
    ));
    assert!(acts(&restored, "enable", "local.service"), "{restored}");
    assert!(omitted(&restored, "local.service").is_none(), "{restored}");

    // The vendor unit declines when a unit file is staged at its name.
    let mut imported = snap.clone();
    imported
        .config
        .as_mut()
        .expect("a config section")
        .files
        .push(inspectah_core::types::config::ConfigFileEntry {
            path: ORPHAN_UNIT.into(),
            category: inspectah_core::types::config::ConfigCategory::Other,
            content: "[Service]\nExecStart=/usr/local/bin/other\n".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        });
    let staged = containerfile_in(&export_session(&session(&imported)));
    assert!(
        in_no_systemctl_instruction(&staged, "orphan.service"),
        "{staged}"
    );
    assert!(
        omitted(&staged, "orphan.service")
            .is_some_and(|l| l.ends_with(&evidence_changed(ORPHAN_UNIT))),
        "{staged}"
    );
    assert!(acts(&staged, "enable", "local.service"), "{staged}");
    let matched = containerfile_in(&exported_after(
        &imported,
        &[(config_item(ORPHAN_UNIT), false)],
    ));
    assert!(acts(&matched, "enable", "orphan.service"), "{matched}");
    assert!(omitted(&matched, "orphan.service").is_none(), "{matched}");
}

const APP_SOCKET: &str = "/etc/systemd/system/app.socket";

/// T15: a local socket enabled against a `disable` preset.
fn t15_host() -> UnitHost {
    UnitHost::new()
        .unit("app.socket", "enabled")
        .preset("disable *")
        .etc_unit(
            "app.socket",
            "[Socket]\nListenStream=7000\n[Install]\nWantedBy=sockets.target\n",
        )
}

/// T15, with a package section and without one (the no-RPM route runs the
/// same rules). Each combination of the two toggles, the config entry of
/// the socket's file and the unit row, is driven through `RefineSession`
/// alone from the scan's state, and every decision the session accepted is
/// one the export obeys: the file is staged exactly when its entry is
/// included, the enable is carried exactly when both are, and the
/// `UnitFileNotStaged` line is there exactly when the row is included and
/// the file is not. Each toggle on its own changes the staged tree or the
/// Containerfile.
#[test]
fn t15_the_socket_enable_follows_both_toggles() {
    for with_packages in [true, false] {
        let mut snap = scan(t15_host());
        if !with_packages {
            snap.rpm = None;
        }
        let route = if with_packages {
            "packages"
        } else {
            "no packages"
        };
        assert!(
            matches!(fragment_of(&snap, "app.socket"), UnitFragment::Local { .. }),
            "{route}"
        );
        let mut seen = Vec::new();
        for (file_in, row_in) in [(true, true), (false, true), (true, false), (false, false)] {
            let mut ops = Vec::new();
            if !file_in {
                ops.push((config_item(APP_SOCKET), false));
            }
            if !row_in {
                ops.push((unit_item("app.socket"), false));
            }
            let tree = exported_after(&snap, &ops);
            let containerfile = containerfile_in(&tree);
            let label = format!("{route}, file {file_in}, row {row_in}");
            let staged = tree.path().join("config").join(&APP_SOCKET[1..]).exists();
            assert_eq!(staged, file_in, "{label}");
            if file_in && row_in {
                assert!(
                    acts(&containerfile, "enable", "app.socket"),
                    "{label}:\n{containerfile}"
                );
            } else {
                assert!(
                    in_no_systemctl_instruction(&containerfile, "app.socket"),
                    "{label}:\n{containerfile}"
                );
            }
            let declined = omitted(&containerfile, "app.socket");
            assert_eq!(
                declined.is_some(),
                row_in && !file_in,
                "{label}:\n{containerfile}"
            );
            if let Some(line) = declined {
                assert!(line.ends_with(NOT_STAGED), "{label}: {line}");
            }
            seen.push((staged, unit_lines(&containerfile)));
        }
        // Each toggle alone, from the scan's state, changes what the export
        // carries.
        assert_ne!(seen[0], seen[1], "{route}: the config toggle");
        assert_ne!(seen[0], seen[2], "{route}: the unit toggle");
        assert_ne!(
            seen[1], seen[3],
            "{route}: the unit toggle with the file excluded"
        );
    }
}

const SHADOW_UNIT: &str = "/etc/systemd/system/shadow.service";

/// A vendor unit with a full copy under `/etc`, in `state` against the
/// opposite preset.
fn shadow_host(state: &str, preset: &str) -> UnitHost {
    UnitHost::new()
        .unit("shadow.service", state)
        .preset(&format!("{preset} shadow.service"))
        .vendor_unit(
            "shadow.service",
            "[Service]\nExecStart=/usr/bin/shadow\n[Install]\nWantedBy=multi-user.target\n",
            Some("shadowpkg"),
        )
        .etc_unit(
            "shadow.service",
            "[Service]\nExecStart=/usr/bin/shadow --local\n[Install]\nWantedBy=multi-user.target\n",
        )
}

/// T15: a unit recorded `Shadowed` whose shadow is excluded gets no
/// `enable` and no `disable`: the vendor file's `[Install]` is not the
/// row's install evidence. The command returns when the shadow is included
/// again.
#[test]
fn t15_a_shadowed_unit_whose_shadow_is_excluded_gets_no_command() {
    for (state, preset, verb) in [
        ("enabled", "disable", "enable"),
        ("disabled", "enable", "disable"),
    ] {
        let snap = scan(shadow_host(state, preset));
        assert!(
            matches!(
                fragment_of(&snap, "shadow.service"),
                UnitFragment::Shadowed { .. }
            ),
            "{verb}: {:?}",
            fragment_of(&snap, "shadow.service")
        );
        let scanned = containerfile_in(&render_export(&snap));
        assert!(acts(&scanned, verb, "shadow.service"), "{verb}:\n{scanned}");

        let shadow = ItemId::DropIn {
            path: SHADOW_UNIT.into(),
        };
        let excluded = containerfile_in(&exported_after(&snap, &[(shadow.clone(), false)]));
        assert!(
            in_no_systemctl_instruction(&excluded, "shadow.service"),
            "{verb}:\n{excluded}"
        );
        assert!(
            omitted(&excluded, "shadow.service")
                .is_some_and(|l| l.ends_with(&evidence_changed(SHADOW_UNIT))),
            "{verb}:\n{excluded}"
        );

        let restored = containerfile_in(&exported_after(
            &snap,
            &[(shadow.clone(), false), (shadow, true)],
        ));
        assert!(
            acts(&restored, verb, "shadow.service"),
            "{verb}:\n{restored}"
        );
        assert!(
            omitted(&restored, "shadow.service").is_none(),
            "{verb}:\n{restored}"
        );
    }
}

/// The package that owns both units of [`twin_package_host`].
const TWIN_PACKAGE: &str = "twinpkg";

/// Two vendor units of one listed package, `twinpkg`, both enabled against
/// a `disable` preset, on a host scanned against a base image (so refine
/// keeps the package included as the scan does). `shadow.service` also has
/// a full copy under `/etc`, so it is recorded `Shadowed`; `twin.service`
/// has none, so it is recorded `Vendor`.
fn twin_package_host() -> UnitHost {
    UnitHost::new()
        .base_package("glibc")
        .package(TWIN_PACKAGE)
        .unit("shadow.service", "enabled")
        .unit("twin.service", "enabled")
        .preset("disable *")
        .vendor_unit(
            "shadow.service",
            "[Service]\nExecStart=/usr/bin/shadow\n[Install]\nWantedBy=multi-user.target\n",
            Some(TWIN_PACKAGE),
        )
        .etc_unit(
            "shadow.service",
            "[Service]\nExecStart=/usr/bin/shadow --local\n[Install]\nWantedBy=multi-user.target\n",
        )
        .vendor_unit(
            "twin.service",
            "[Service]\nExecStart=/usr/bin/twin\n[Install]\nWantedBy=multi-user.target\n",
            Some(TWIN_PACKAGE),
        )
}

/// § 10.2 rule 5: a `Shadowed` unit's one condition is its staged shadow.
/// The image loads the `/etc` copy, so the command needs no package, and
/// the presence tiers are asked only for a `Vendor` or package-owned
/// storage unit. With the owner excluded through refine, the shadowed
/// unit keeps its enable; the `Vendor` twin of the same package, under the
/// same exclusion, is omitted for it (the control).
#[test]
fn a_shadowed_unit_keeps_its_enable_when_its_package_is_excluded() {
    let snap = scan(twin_package_host());
    assert!(
        matches!(
            fragment_of(&snap, "shadow.service"),
            UnitFragment::Shadowed { owning_package: Some(p), .. } if p == TWIN_PACKAGE
        ),
        "{:?}",
        fragment_of(&snap, "shadow.service")
    );
    assert!(
        matches!(
            fragment_of(&snap, "twin.service"),
            UnitFragment::Vendor { owning_package: Some(p), .. } if p == TWIN_PACKAGE
        ),
        "{:?}",
        fragment_of(&snap, "twin.service")
    );
    let scanned = containerfile_in(&render_export(&snap));
    assert!(acts(&scanned, "enable", "shadow.service"), "{scanned}");
    assert!(acts(&scanned, "enable", "twin.service"), "{scanned}");

    let tree = exported_after(
        &snap,
        &[(
            ItemId::Package {
                name: TWIN_PACKAGE.into(),
                arch: "x86_64".into(),
            },
            false,
        )],
    );
    let excluded = containerfile_in(&tree);
    assert_eq!(
        omitted(&excluded, "twin.service"),
        Some("# Omitted: twin.service (package 'twinpkg' was excluded)"),
        "{excluded}"
    );
    assert!(
        in_no_systemctl_instruction(&excluded, "twin.service"),
        "{excluded}"
    );
    assert!(
        tree.path()
            .join("drop-ins")
            .join(&SHADOW_UNIT[1..])
            .is_file(),
        "the shadow is still staged"
    );
    assert!(acts(&excluded, "enable", "shadow.service"), "{excluded}");
    assert!(omitted(&excluded, "shadow.service").is_none(), "{excluded}");
}

/// A `Shadowed` unit whose `/etc` file cannot be read as text has no
/// drop-in row, and its config entry (a `ContentNotCaptured` advisory)
/// stages nothing, so the shadow is not staged. Its `[Install]` cannot be
/// read either, so the unit has no state change and an undetermined record
/// listing the shadow, which the export omits by that file: no
/// `systemctl` line names the unit. Control: the readable twin is enabled.
#[test]
fn a_shadow_that_could_not_be_read_declines_its_enable() {
    const BLOB_UNIT: &str = "/etc/systemd/system/blob.service";
    let host = |readable: bool| {
        let host = UnitHost::new()
            .unit("blob.service", "enabled")
            .preset("disable blob.service")
            .vendor_unit(
                "blob.service",
                "[Service]\nExecStart=/usr/bin/blob\n[Install]\nWantedBy=multi-user.target\n",
                Some("blobpkg"),
            );
        if readable {
            host.etc_unit(
                "blob.service",
                "[Service]\nExecStart=/usr/bin/blob -v\n[Install]\nWantedBy=multi-user.target\n",
            )
        } else {
            host.file_error(BLOB_UNIT, std::io::ErrorKind::InvalidData)
        }
    };

    let snap = scan(host(false));
    let units = snap.units.as_ref().expect("a units section");
    assert_eq!(changes_named(&snap, "blob.service"), 0);
    assert_eq!(
        units
            .undetermined_installs
            .iter()
            .map(|r| (r.unit.as_str(), r.reason, r.install_files.clone()))
            .collect::<Vec<_>>(),
        [(
            "blob.service",
            InstallUnknown::Unreadable,
            vec![BLOB_UNIT.to_owned()]
        )]
    );
    assert!(
        units.drop_ins.iter().all(|d| d.path != BLOB_UNIT),
        "no drop-in row for a shadow that was not read"
    );
    let containerfile = containerfile_in(&render_export(&snap));
    assert!(
        in_no_systemctl_instruction(&containerfile, "blob.service"),
        "{containerfile}"
    );
    assert_eq!(
        count_lines(
            &containerfile,
            &install_omitted("blob.service", &[BLOB_UNIT])
        ),
        1,
        "{containerfile}"
    );

    let control = containerfile_in(&render_export(&scan(host(true))));
    assert!(acts(&control, "enable", "blob.service"), "{control}");
    assert!(omitted(&control, "blob.service").is_none(), "{control}");
}

/// Plan review focus 5, render side: a drop-in row whose `unit` does not
/// parse as a unit name (`foo.servic` from `foo.servic.d`, and a
/// `*.device.d` directory) is collected and staged, and never reaches a
/// command line. Control: a drop-in for a unit that parses, whose unit is
/// enabled against its preset, sits beside an enable of that unit.
#[test]
fn a_drop_in_for_a_name_that_does_not_parse_reaches_no_command() {
    const TYPO: &str = "/etc/systemd/system/foo.servic.d/typo.conf";
    const DEVICE: &str = "/etc/systemd/system/dev-sda.device.d/tag.conf";
    const REAL: &str = "/etc/systemd/system/real.service.d/override.conf";
    let snap = scan(
        UnitHost::new()
            .unit("real.service", "enabled")
            .preset("disable real.service")
            .vendor_unit(
                "real.service",
                "[Service]\nExecStart=/usr/bin/real\n[Install]\nWantedBy=multi-user.target\n",
                Some("realpkg"),
            )
            .file(TYPO, "[Service]\nNice=5\n")
            .file(DEVICE, "[Unit]\nDescription=disk\n")
            .file(REAL, "[Service]\nNice=5\n"),
    );
    let rows: Vec<(&str, &str)> = snap
        .units
        .as_ref()
        .expect("a units section")
        .drop_ins
        .iter()
        .map(|d| (d.unit.as_str(), d.path.as_str()))
        .collect();
    for (unit, path) in [
        ("foo.servic", TYPO),
        ("dev-sda.device", DEVICE),
        ("real.service", REAL),
    ] {
        assert!(rows.contains(&(unit, path)), "{unit}: {rows:?}");
    }
    let tree = render_export(&snap);
    for path in [TYPO, DEVICE, REAL] {
        assert!(
            tree.path().join("drop-ins").join(&path[1..]).is_file(),
            "{path}"
        );
    }
    let containerfile = containerfile_in(&tree);
    for line in containerfile.lines().filter(|l| !l.starts_with('#')) {
        assert!(
            !line.contains("foo.servic") && !line.contains("dev-sda.device"),
            "an unparsed name reached a command line: {line:?}\n{containerfile}"
        );
    }
    assert!(
        acts(&containerfile, "enable", "real.service"),
        "{containerfile}"
    );
}

/// A fleet host named `hostname` with a full shadow of `app.service`
/// holding `content`, enabled against its preset.
fn fleet_host(hostname: &str, content: &str) -> InspectionSnapshot {
    reload(&scan(
        UnitHost::new()
            .file("/etc/hostname", &format!("{hostname}\n"))
            .unit("app.service", "enabled")
            .preset("disable app.service")
            .vendor_unit(
                "app.service",
                "[Service]\nExecStart=/usr/bin/app\n[Install]\nWantedBy=multi-user.target\n",
                Some("apppkg"),
            )
            .etc_unit("app.service", content),
    ))
}

const APP_UNIT: &str = "/etc/systemd/system/app.service";

/// T18 (d): two hosts with a full shadow of one unit and different
/// content. Through a refine session, the operator includes the shadow's
/// row and selects each variant in turn with `SelectVariant`, the
/// operation refine's view uses: whichever is selected, the unit's command
/// is declined with `InstallEvidenceChanged`, since the row may have been
/// recorded from the other variant. Presence first: a shadow is staged at
/// the unit's path. Control: the same content on both hosts is one
/// variant, and the enable is carried through the same route.
///
/// Which variant's bytes land is not asserted: `SelectVariant` changes
/// which row is `Selected`, while `SetInclude` on the path includes the
/// first-ranked row, so the staged bytes do not follow the selection. That
/// is recorded for the project owner, not pinned here.
#[test]
fn t18_d_a_shadow_with_two_variants_declines_the_command() {
    use inspectah_core::types::aggregate::VariantSelection;
    use inspectah_refine::types::ContentHash;
    let a = "[Service]\nExecStart=/usr/bin/app --a\n[Install]\nWantedBy=multi-user.target\n";
    let b = "[Service]\nExecStart=/usr/bin/app --b\n[Install]\nWantedBy=multi-user.target\n";
    let (merged, _) = merge_snapshots(
        vec![fleet_host("host-a", a), fleet_host("host-b", b)],
        None,
        None,
    )
    .expect("the two hosts merge");
    let merged = reload(&merged);
    assert!(merged.aggregate_meta.is_some(), "an aggregate");
    assert!(
        change(&merged, "app.service").disposition.is_included(),
        "the merged unit row is included"
    );
    let variants = |snap: &InspectionSnapshot| -> Vec<String> {
        snap.units
            .as_ref()
            .expect("a units section")
            .drop_ins
            .iter()
            .filter(|d| d.path == APP_UNIT)
            .map(|d| d.content.clone())
            .collect()
    };
    let contents = variants(&merged);
    assert_eq!(contents.len(), 2, "one drop-in row per variant");

    let drop_in = ItemId::DropIn {
        path: APP_UNIT.into(),
    };
    let mut fleet_session = session(&merged);
    fleet_session
        .apply(set_include(drop_in.clone(), true))
        .unwrap_or_else(|e| panic!("{e:?}"));
    for selected in &contents {
        fleet_session
            .apply(RefinementOp::SelectVariant {
                item_id: drop_in.clone(),
                target: ContentHash::from_content(selected.as_bytes()),
            })
            .unwrap_or_else(|e| panic!("SelectVariant: {e:?}"));
        let projected = fleet_session.snapshot_projected();
        let selection: Vec<_> = projected
            .units
            .as_ref()
            .expect("a units section")
            .drop_ins
            .iter()
            .filter(|d| d.path == APP_UNIT)
            .map(|d| (d.content == *selected, d.variant_selection))
            .collect();
        assert!(
            selection.contains(&(true, VariantSelection::Selected)),
            "the operator's variant is selected: {selection:?}"
        );
        let tree = export_session(&fleet_session);
        assert!(
            contents.contains(
                &std::fs::read_to_string(tree.path().join("drop-ins").join(&APP_UNIT[1..]))
                    .expect("a shadow is staged")
            ),
            "a variant is staged"
        );
        let containerfile = containerfile_in(&tree);
        assert!(
            in_no_systemctl_instruction(&containerfile, "app.service"),
            "{containerfile}"
        );
        assert!(
            omitted(&containerfile, "app.service")
                .is_some_and(|l| l.ends_with(&evidence_changed(APP_UNIT))),
            "{containerfile}"
        );
    }

    let (same, _) = merge_snapshots(
        vec![fleet_host("host-a", a), fleet_host("host-b", a)],
        None,
        None,
    )
    .expect("the two hosts merge");
    let same = reload(&same);
    assert_eq!(variants(&same).len(), 1, "one variant");
    let mut same_session = session(&same);
    same_session
        .apply(set_include(drop_in, true))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let tree = export_session(&same_session);
    assert!(staged_at_drop_ins(&tree, APP_UNIT), "the shadow is staged");
    let control = containerfile_in(&tree);
    assert!(acts(&control, "enable", "app.service"), "{control}");
    assert!(omitted(&control, "app.service").is_none(), "{control}");
}

/// Whether the export tree stages `path` under `drop-ins/`.
fn staged_at_drop_ins(tree: &tempfile::TempDir, path: &str) -> bool {
    tree.path().join("drop-ins").join(&path[1..]).is_file()
}

/// The vendor socket file of the fleet matrix below.
const MATRIX_VENDOR: &str = "[Socket]\nListenStream=9000\n[Install]\nWantedBy=sockets.target\n";
/// The administrator's file of the matrix: a local unit or a full shadow.
const MATRIX_ETC: &str = "[Socket]\nListenStream=9001\n[Install]\nWantedBy=sockets.target\n";
const MATRIX_UNIT: &str = "app.socket";

/// What the other host of the matrix has for `app.socket`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum MatrixOther {
    /// Its own file in `/etc` and no vendor file: a `Local` row and a
    /// config entry.
    Local,
    /// The vendor file and a full copy in `/etc`: a `Shadowed` row and a
    /// drop-in row.
    Shadowed,
}

/// A fleet host for the matrix: `sockpkg` installed and `app.socket`
/// enabled against a `disable` preset, with the vendor file (`other` is
/// `None`) or what `other` names.
fn matrix_host(hostname: &str, other: Option<MatrixOther>) -> InspectionSnapshot {
    let host = UnitHost::new()
        .package("sockpkg")
        .unit(MATRIX_UNIT, "enabled")
        .preset("disable app.socket");
    let host = match other {
        Some(MatrixOther::Local) => host.etc_unit(MATRIX_UNIT, MATRIX_ETC),
        Some(MatrixOther::Shadowed) => host
            .vendor_unit(MATRIX_UNIT, MATRIX_VENDOR, Some("sockpkg"))
            .etc_unit(MATRIX_UNIT, MATRIX_ETC),
        None => host.vendor_unit(MATRIX_UNIT, MATRIX_VENDOR, Some("sockpkg")),
    };
    named(host, hostname)
}

/// § 12's merged `Vendor` row beside another host's `Local` or `Shadowed`
/// row for one ordinary unit, in both hostname orders, through
/// `merge_snapshots`, a refine session, its projection and its export.
/// The two state change rows tie, so the merged row is the first host's.
/// The other host's file (its config entry or drop-in row) is then
/// included and excluded:
///
/// - a `Vendor` row: with the file included, the command is declined with
///   `InstallEvidenceChanged`; excluded, the enable is carried;
/// - a `Local` row: included, carried; excluded, `UnitFileNotStaged`;
/// - a `Shadowed` row: included, carried; excluded,
///   `InstallEvidenceChanged`.
///
/// Included, the file's bytes are what the export stages.
#[test]
fn a_merged_vendor_row_beside_a_local_or_shadowed_row_in_both_orders() {
    let path = etc_path(MATRIX_UNIT);
    for other in [MatrixOther::Local, MatrixOther::Shadowed] {
        for vendor_first in [true, false] {
            let label = format!("{other:?}, vendor host first {vendor_first}");
            let (first, second) = if vendor_first {
                (None, Some(other))
            } else {
                (Some(other), None)
            };
            let merged = merged(matrix_host("host-a", first), matrix_host("host-b", second));
            let mut session = session(&merged);
            // Refine sorts an aggregate's packages with no leaf data as
            // excluded; the unit's package is in the image here.
            session
                .apply(set_include(package_item("sockpkg"), true))
                .unwrap_or_else(|e| panic!("{label}: {e:?}"));
            let representative = |session: &RefineSession| match fragment_of(
                &session.snapshot_projected(),
                MATRIX_UNIT,
            ) {
                UnitFragment::Vendor { .. } => "vendor",
                UnitFragment::Local { .. } => "local",
                UnitFragment::Shadowed { .. } => "shadowed",
                UnitFragment::Absent => "absent",
            };
            let expected_row = match (vendor_first, other) {
                (true, _) => "vendor",
                (false, MatrixOther::Local) => "local",
                (false, MatrixOther::Shadowed) => "shadowed",
            };
            assert_eq!(representative(&session), expected_row, "{label}");
            let file_item = match other {
                MatrixOther::Local => config_item(&path),
                MatrixOther::Shadowed => ItemId::DropIn { path: path.clone() },
            };
            let root = match other {
                MatrixOther::Local => "config",
                MatrixOther::Shadowed => "drop-ins",
            };
            for included in [true, false] {
                let step = format!("{label}, file included {included}");
                session
                    .apply(set_include(file_item.clone(), included))
                    .unwrap_or_else(|e| panic!("{step}: {e:?}"));
                assert_eq!(representative(&session), expected_row, "{step}");
                let tree = export_session(&session);
                let staged = std::fs::read_to_string(tree.path().join(root).join(&path[1..])).ok();
                assert_eq!(
                    staged.as_deref(),
                    included.then_some(MATRIX_ETC),
                    "{step}: what is staged"
                );
                let containerfile = containerfile_in(&tree);
                let declined = match (expected_row, included) {
                    ("vendor", true) | ("shadowed", false) => Some(evidence_changed(&path)),
                    ("local", false) => Some(NOT_STAGED.to_owned()),
                    _ => None,
                };
                match declined {
                    Some(reason) => {
                        assert!(
                            omitted(&containerfile, MATRIX_UNIT)
                                .is_some_and(|l| l.ends_with(&reason)),
                            "{step}: declined with {reason}:\n{containerfile}"
                        );
                        assert!(
                            in_no_systemctl_instruction(&containerfile, MATRIX_UNIT),
                            "{step}:\n{containerfile}"
                        );
                    }
                    None => {
                        assert!(
                            acts(&containerfile, "enable", MATRIX_UNIT),
                            "{step}:\n{containerfile}"
                        );
                        assert!(
                            omitted(&containerfile, MATRIX_UNIT).is_none(),
                            "{step}:\n{containerfile}"
                        );
                    }
                }
            }
        }
    }
}

/// T13 quoting and plan review focus 1: an instance name with `\x2d`, the
/// root slice `-.slice` and a 255-byte name render as single-quoted
/// operands after `--`, in kind then name order, and each survives a refine
/// export unchanged (the backslashes stay inside the quotes).
///
/// Built by hand, with the instance a vendor unit whose file is its
/// template, which the state change type admits for an enabled instance,
/// and a name that is not an instance with the same `\x2d`.
#[test]
fn t13_unusual_unit_names_are_quoted_operands_through_both_exports() {
    use inspectah_core::types::units::{
        UnitName, UnitSection, UnitState, UnitStateChange, UnitSubject,
    };
    let long = format!("{}.service", "a".repeat(255 - ".service".len()));
    assert_eq!(long.len(), 255);
    let name = |s: &str| UnitName::try_from(s.to_owned()).expect("a unit name");
    let change = |unit: &str, file: &str, state: UnitState| {
        let subject = UnitSubject::new(
            name(unit),
            UnitFragment::Vendor {
                file: name(file),
                owning_package: None,
            },
        )
        .expect("a subject");
        UnitStateChange::new(
            CarriedUnit::try_from(subject).expect("a carried unit"),
            state,
            None,
        )
        .expect("a state change")
    };
    let mut snap = InspectionSnapshot::new();
    snap.units = Some(UnitSection {
        state_changes: vec![
            change("-.slice", "-.slice", UnitState::Enabled),
            change(
                r"probe@a\x2db.service",
                "probe@.service",
                UnitState::Enabled,
            ),
            change(
                r"probe-a\x2db.service",
                r"probe-a\x2db.service",
                UnitState::Enabled,
            ),
            change(&long, &long, UnitState::Disabled),
        ],
        ..Default::default()
    });
    let expected = vec![
        "RUN systemctl enable -- \\".to_owned(),
        r"    'probe-a\x2db.service' \".to_owned(),
        r"    'probe@a\x2db.service' \".to_owned(),
        "    '-.slice'".to_owned(),
        "RUN systemctl disable -- \\".to_owned(),
        format!("    '{long}'"),
    ];
    let scanned = unit_lines(&containerfile_in(&render_export(&snap)));
    assert_eq!(scanned, expected, "scan route");
    let refined = unit_lines(&containerfile_in(&export_session(&session(&snap))));
    assert_eq!(refined, expected, "refine route");
}

// ---------------------------------------------------------------------
// § 10.2 rules 2, 3 and 4, § 10.3, and T11's commands, through the scan.
// ---------------------------------------------------------------------

/// The `/etc/systemd/system` path of `unit`.
fn etc_path(unit: &str) -> String {
    format!("/etc/systemd/system/{unit}")
}

/// Whether the export tree stages a file at `path`, in either tree.
fn staged_at(tree: &tempfile::TempDir, path: &str) -> bool {
    ["config", "drop-ins"]
        .iter()
        .any(|root| tree.path().join(root).join(&path[1..]).exists())
}

/// The `# FIXME:` line that starts with `unit`'s name, if any.
fn fixme_for<'a>(containerfile: &'a str, unit: &str) -> Option<&'a str> {
    let prefix = format!("# FIXME: {unit} ");
    containerfile.lines().find(|l| l.starts_with(&prefix))
}

/// The § 10.3 FIXME for a protected unit in `state`.
fn protected_fixme(unit: &str, state: &str) -> String {
    format!(
        "# FIXME: {unit} is {state} on the source host. Not carried: the image needs this unit."
    )
}

/// § 10.3's FIXME for a masked `tmp.mount`.
const TMP_MOUNT_FIXME: &str = "# FIXME: tmp.mount is masked on the source host, where /tmp is \
     then a directory on the root filesystem. Not carried: the bootc base image mounts /tmp \
     from this unit, and without it /tmp is read-only.";

/// A vendor unit file with an `[Install]` section.
const VENDOR_SERVICE: &str =
    "[Service]\nExecStart=/usr/bin/true\n[Install]\nWantedBy=multi-user.target\n";

/// T12 through the scan and through a refine export: a protected unit
/// disabled against its preset and one masked get the FIXME, no command and
/// the same sentence as their `attention_reason`; a protected unit enabled
/// against its preset is enabled, with no attention reason (the control).
#[test]
fn t12_protected_units_through_the_scan() {
    const TIMER: &str = "bootc-fetch-apply-updates.timer";
    const OSTREE: &str = "ostree-remount.service";
    const REMOUNT: &str = "systemd-remount-fs.service";
    let snap = scan(
        UnitHost::new()
            .unit(TIMER, "disabled")
            .unit(OSTREE, "masked")
            .unit(REMOUNT, "enabled")
            .preset(&format!("enable {TIMER}"))
            .preset("disable *")
            .vendor_unit(
                TIMER,
                "[Timer]\nOnCalendar=daily\n[Install]\nWantedBy=timers.target\n",
                Some("bootc"),
            )
            .vendor_unit(OSTREE, VENDOR_SERVICE, Some("ostree"))
            .vendor_unit(REMOUNT, VENDOR_SERVICE, Some("systemd"))
            .mask(OSTREE),
    );
    let expected = [
        (TIMER, Some(protected_fixme(TIMER, "disabled"))),
        (OSTREE, Some(protected_fixme(OSTREE, "masked"))),
        (REMOUNT, None),
    ];
    for (unit, fixme) in &expected {
        assert_eq!(
            change(&snap, unit).attention_reason.as_deref(),
            fixme.as_deref().and_then(|l| l.strip_prefix("# FIXME: ")),
            "{unit}"
        );
    }
    let scanned = containerfile_in(&render_export(&snap));
    let refined = containerfile_in(&export_session(&session(&snap)));
    for (route, containerfile) in [("scan", &scanned), ("refine", &refined)] {
        for (unit, fixme) in &expected {
            match fixme {
                Some(line) => {
                    assert_eq!(
                        fixme_for(containerfile, unit),
                        Some(line.as_str()),
                        "{route}:\n{containerfile}"
                    );
                    assert!(
                        in_no_systemctl_instruction(containerfile, unit),
                        "{route}:\n{containerfile}"
                    );
                }
                None => {
                    assert!(
                        acts(containerfile, "enable", unit),
                        "{route}:\n{containerfile}"
                    );
                    assert!(fixme_for(containerfile, unit).is_none(), "{route}");
                }
            }
        }
    }
}

/// T12, every route: a zero-length file over a protected unit's vendor
/// file, from a scan, gives a `Masked` row whose mask is refused, and no
/// file is staged at the unit's path. The same snapshot with the
/// zero-length file also offered by hand (an imported snapshot the
/// collector cannot produce) as a drop-in or a config entry, with a `unit`
/// field naming another unit, an interior `.` or a doubled separator,
/// stages nothing at the path either: the stager's FIXME names it. Control:
/// the same for `kdump.service`, which is not protected, carries the mask
/// and still stages no file.
#[test]
fn t12_a_zero_length_protected_unit_file_is_neither_a_mask_nor_a_file() {
    for (unit, protected) in [
        ("bootc-fetch-apply-updates.service", true),
        ("kdump.service", false),
    ] {
        let path = etc_path(unit);
        let snap = scan(
            UnitHost::new()
                .unit(unit, "masked")
                .vendor_unit(unit, VENDOR_SERVICE, Some("pkg"))
                .etc_unit(unit, ""),
        );
        assert_eq!(
            change(&snap, unit).current_state(),
            UnitState::Masked,
            "{unit}"
        );
        let check = |label: &str, snap: &InspectionSnapshot| {
            let tree = render_export(snap);
            let containerfile = containerfile_in(&tree);
            assert!(!staged_at(&tree, &path), "{unit}, {label}");
            if protected {
                assert_eq!(
                    fixme_for(&containerfile, unit),
                    Some(protected_fixme(unit, "masked").as_str()),
                    "{unit}, {label}:\n{containerfile}"
                );
                assert!(
                    in_no_systemctl_instruction(&containerfile, unit),
                    "{unit}, {label}:\n{containerfile}"
                );
            } else {
                assert!(
                    acts(&containerfile, "mask", unit),
                    "{unit}, {label}:\n{containerfile}"
                );
            }
            containerfile
        };
        check("scan", &snap);

        for (label, spelling, as_drop_in, unit_field) in [
            ("drop-in", path.clone(), true, unit.to_owned()),
            (
                "drop-in naming another unit",
                path.clone(),
                true,
                "sshd.service".to_owned(),
            ),
            ("config entry", path.clone(), false, String::new()),
            (
                "config entry, interior .",
                format!("/etc/systemd/./system/{unit}"),
                false,
                String::new(),
            ),
            (
                "drop-in, doubled separator",
                format!("/etc/systemd//system/{unit}"),
                true,
                unit.to_owned(),
            ),
        ] {
            let mut imported = snap.clone();
            if as_drop_in {
                imported
                    .units
                    .as_mut()
                    .expect("a units section")
                    .drop_ins
                    .push(inspectah_core::types::units::SystemdDropIn {
                        unit: unit_field,
                        path: spelling.clone(),
                        content: String::new(),
                        disposition: FindingKind::included(),
                        ..Default::default()
                    });
            } else {
                imported
                    .config
                    .as_mut()
                    .expect("a config section")
                    .files
                    .push(inspectah_core::types::config::ConfigFileEntry {
                        path: spelling.clone(),
                        category: inspectah_core::types::config::ConfigCategory::Other,
                        content: String::new(),
                        disposition: FindingKind::included(),
                        ..Default::default()
                    });
            }
            let containerfile = check(label, &imported);
            assert!(
                containerfile
                    .lines()
                    .any(|l| l.starts_with("# FIXME:") && l.contains(&path)),
                "{unit}, {label}: the offer is named as withheld:\n{containerfile}"
            );
        }
    }
}

/// § 10.3 through the scan: a masked `tmp.mount` gets its FIXME, no
/// command and the same sentence as its attention reason, while a masked
/// `dev-hugepages.mount` on the same host is in the `mask` line. With
/// `tmp.mount` enabled or disabled against its preset, the command is
/// carried and there is no attention reason.
#[test]
fn t12_a_masked_tmp_mount_through_the_scan() {
    let mount = "[Mount]\nWhat=tmpfs\nWhere=/tmp\n[Install]\nWantedBy=local-fs.target\n";
    let snap = scan(
        UnitHost::new()
            .unit("tmp.mount", "masked")
            .unit("dev-hugepages.mount", "masked")
            .vendor_unit("tmp.mount", mount, Some("systemd"))
            .vendor_unit("dev-hugepages.mount", mount, Some("systemd"))
            .mask("tmp.mount")
            .mask("dev-hugepages.mount"),
    );
    assert_eq!(
        change(&snap, "tmp.mount").attention_reason.as_deref(),
        TMP_MOUNT_FIXME.strip_prefix("# FIXME: ")
    );
    assert_eq!(change(&snap, "dev-hugepages.mount").attention_reason, None);
    let containerfile = containerfile_in(&render_export(&snap));
    assert_eq!(
        fixme_for(&containerfile, "tmp.mount"),
        Some(TMP_MOUNT_FIXME),
        "{containerfile}"
    );
    assert!(
        in_no_systemctl_instruction(&containerfile, "tmp.mount"),
        "{containerfile}"
    );
    assert!(
        acts(&containerfile, "mask", "dev-hugepages.mount"),
        "{containerfile}"
    );

    for (state, preset, verb) in [
        ("enabled", "disable", "enable"),
        ("disabled", "enable", "disable"),
    ] {
        let snap = scan(
            UnitHost::new()
                .unit("tmp.mount", state)
                .preset(&format!("{preset} tmp.mount"))
                .vendor_unit("tmp.mount", mount, Some("systemd")),
        );
        assert_eq!(change(&snap, "tmp.mount").attention_reason, None, "{verb}");
        let containerfile = containerfile_in(&render_export(&snap));
        assert!(
            acts(&containerfile, verb, "tmp.mount"),
            "{verb}:\n{containerfile}"
        );
        assert!(fixme_for(&containerfile, "tmp.mount").is_none(), "{verb}");
    }
}

/// A host whose package `owner` owns a vendor `unit`, in `state`, scanned
/// against a base image so refine keeps the package included.
fn owned_unit_host(unit: &str, owner: &str, state: &str, content: &str) -> UnitHost {
    let host = UnitHost::new()
        .base_package("glibc")
        .package(owner)
        .unit(unit, state)
        .preset(&format!("disable {unit}"))
        .vendor_unit(unit, content, Some(owner));
    if state == "masked" {
        host.mask(unit)
    } else {
        host
    }
}

/// `snap` with a config entry at `path`, as an imported snapshot may hold
/// one (the collector records an administrator's file at a vendor unit's
/// name as a shadow of that unit, not beside a package-owned row).
fn with_config_entry(snap: &InspectionSnapshot, path: &str, content: &str) -> InspectionSnapshot {
    let mut imported = snap.clone();
    imported
        .config
        .as_mut()
        .expect("a config section")
        .files
        .push(inspectah_core::types::config::ConfigFileEntry {
            path: path.into(),
            category: inspectah_core::types::config::ConfigCategory::Other,
            content: content.into(),
            disposition: FindingKind::included(),
            ..Default::default()
        });
    imported
}

/// T8, render, and rule 2 through refine: a package-owned mount unit
/// enabled against its preset is in `RUN systemctl enable`, and masked, in
/// `mask`. With an administrator's file at its name staged by an imported
/// config entry, the refine export declines the command with
/// `StorageFileStaged` and stages the file; excluding that entry through
/// `RefineSession` alone removes the file and brings the command back.
#[test]
fn t8_a_package_owned_mount_unit_through_the_scan_and_refine() {
    const UNIT: &str = "var-lib-pkg.mount";
    let path = etc_path(UNIT);
    let mount = "[Mount]\nWhat=/dev/vdb\nWhere=/var/lib/pkg\n[Install]\nWantedBy=local-fs.target\n";
    for (state, verb) in [("enabled", "enable"), ("masked", "mask")] {
        let snap = scan(owned_unit_host(UNIT, "pkgmount", state, mount));
        assert!(
            matches!(change(&snap, UNIT).unit(), CarriedUnit::PackagedStorage(_)),
            "{verb}"
        );
        let scanned = containerfile_in(&render_export(&snap));
        assert!(acts(&scanned, verb, UNIT), "{verb}:\n{scanned}");

        let imported = with_config_entry(&snap, &path, "[Mount]\nWhat=/dev/vdc\n");
        let tree = export_session(&session(&imported));
        let declined = containerfile_in(&tree);
        assert!(
            staged_at(&tree, &path),
            "{verb}: the administrator's file is staged"
        );
        assert_eq!(
            omitted(&declined, UNIT),
            Some(
                format!(
                    "# Omitted: {UNIT} (the export carries an administrator's unit file for it \
                     ({path}))"
                )
                .as_str()
            ),
            "{verb}:\n{declined}"
        );
        assert!(
            in_no_systemctl_instruction(&declined, UNIT),
            "{verb}:\n{declined}"
        );

        let tree = exported_after(&imported, &[(config_item(&path), false)]);
        let restored = containerfile_in(&tree);
        assert!(!staged_at(&tree, &path), "{verb}: the file is excluded");
        assert!(acts(&restored, verb, UNIT), "{verb}:\n{restored}");
        assert!(omitted(&restored, UNIT).is_none(), "{verb}:\n{restored}");
    }
}

/// T16 through refine: a masked unit and a unit file of the same name,
/// staged by an imported config entry, give `MaskPathOccupied` and no
/// mask. Excluding the entry through `RefineSession` alone removes the file
/// and brings the mask back.
#[test]
fn t16_excluding_the_file_brings_the_mask_back() {
    const UNIT: &str = "kdump.service";
    let path = etc_path(UNIT);
    let snap = scan(owned_unit_host(
        UNIT,
        "kexec-tools",
        "masked",
        VENDOR_SERVICE,
    ));
    let scanned = containerfile_in(&render_export(&snap));
    assert!(acts(&scanned, "mask", UNIT), "{scanned}");

    let imported = with_config_entry(&snap, &path, "[Service]\nExecStart=/usr/bin/kdump\n");
    let tree = export_session(&session(&imported));
    let occupied = containerfile_in(&tree);
    assert!(staged_at(&tree, &path), "the unit file is staged");
    assert_eq!(
        omitted(&occupied, UNIT),
        Some(
            format!(
                "# Omitted: {UNIT} (the export also carries a unit file at {path}; exclude one \
                 of the two)"
            )
            .as_str()
        ),
        "{occupied}"
    );
    assert!(in_no_systemctl_instruction(&occupied, UNIT), "{occupied}");

    let tree = exported_after(&imported, &[(config_item(&path), false)]);
    let freed = containerfile_in(&tree);
    assert!(!staged_at(&tree, &path), "the file is excluded");
    assert!(acts(&freed, "mask", UNIT), "{freed}");
    assert!(omitted(&freed, UNIT).is_none(), "{freed}");
}

/// The two mask forms the collector reads: a link to `/dev/null`, and a
/// zero-length file.
const MASK_FORMS: [&str; 2] = ["link", "zero-length file"];

/// `host` with `unit` masked in `form`.
fn masked_by(host: UnitHost, unit: &str, form: &str) -> UnitHost {
    if form == "link" {
        host.mask(unit)
    } else {
        host.etc_unit(unit, "")
    }
}

/// The `systemctl` instructions that name `unit` in any form.
fn instructions_naming(containerfile: &str, unit: &str) -> usize {
    systemctl_instructions(containerfile)
        .iter()
        .filter(|i| i.contains(unit))
        .count()
}

/// T11, command half: each mask form over a vendor unit gives one
/// `RUN systemctl mask` instruction naming the unit, and no file staged at
/// its path.
#[test]
fn t11_each_mask_form_over_a_vendor_unit_is_one_mask_command() {
    const UNIT: &str = "kdump.service";
    for form in MASK_FORMS {
        let snap = scan(masked_by(
            UnitHost::new().unit(UNIT, "masked").vendor_unit(
                UNIT,
                VENDOR_SERVICE,
                Some("kexec-tools"),
            ),
            UNIT,
            form,
        ));
        assert!(
            matches!(
                fragment_of(&snap, UNIT),
                UnitFragment::Vendor { owning_package: Some(p), .. } if p == "kexec-tools"
            ),
            "{form}: {:?}",
            fragment_of(&snap, UNIT)
        );
        let tree = render_export(&snap);
        let containerfile = containerfile_in(&tree);
        assert!(
            acts(&containerfile, "mask", UNIT),
            "{form}:\n{containerfile}"
        );
        assert_eq!(
            instructions_naming(&containerfile, UNIT),
            1,
            "{form}:\n{containerfile}"
        );
        assert!(!staged_at(&tree, &etc_path(UNIT)), "{form}");
    }
}

/// T11: a mask with no vendor file (`Absent`) and a masked template are in
/// the `mask` instruction under their own names.
#[test]
fn t11_an_absent_unit_and_a_template_are_masked_by_their_own_names() {
    let snap = scan(
        UnitHost::new()
            .unit("ghost.service", "masked")
            .unit("getty@.service", "masked")
            .vendor_unit("getty@.service", VENDOR_SERVICE, Some("systemd"))
            .mask("ghost.service")
            .mask("getty@.service"),
    );
    assert_eq!(fragment_of(&snap, "ghost.service"), &UnitFragment::Absent);
    let containerfile = containerfile_in(&render_export(&snap));
    for unit in ["ghost.service", "getty@.service"] {
        assert!(
            acts(&containerfile, "mask", unit),
            "{unit}:\n{containerfile}"
        );
    }
}

/// T11, storage kinds: for each kind and both mask forms, a package-owned
/// vendor file gives a `mask` command; an unowned vendor file, or none,
/// gives a mount finding with state `Masked` and no instruction.
#[test]
fn t11_a_storage_mask_is_carried_only_over_a_package_file() {
    for unit in ["srv-data.mount", "srv-data.automount", "dev-vdb1.swap"] {
        for form in MASK_FORMS {
            for owner in ["owned", "unowned", "no vendor file"] {
                let label = format!("{unit}, {form}, {owner}");
                let host = UnitHost::new().unit(unit, "masked");
                let host = match owner {
                    "owned" => host.vendor_unit(unit, "[Unit]\n", Some("pkg")),
                    "unowned" => host.vendor_unit(unit, "[Unit]\n", None),
                    _ => host,
                };
                let snap = scan(masked_by(host, unit, form));
                let containerfile = containerfile_in(&render_export(&snap));
                if owner == "owned" {
                    assert!(
                        acts(&containerfile, "mask", unit),
                        "{label}:\n{containerfile}"
                    );
                    continue;
                }
                let findings: Vec<_> = snap
                    .units
                    .as_ref()
                    .expect("a units section")
                    .mount_findings
                    .iter()
                    .filter(|f| f.unit.name().as_unit_name().as_str() == unit)
                    .collect();
                assert_eq!(findings.len(), 1, "{label}");
                assert!(
                    matches!(findings[0].state_change, Some((UnitState::Masked, _))),
                    "{label}: {:?}",
                    findings[0].state_change
                );
                assert!(
                    in_no_systemctl_instruction(&containerfile, unit),
                    "{label}:\n{containerfile}"
                );
            }
        }
    }
}

/// T11, a link that ends at a zero-length file: directly to one in the unit
/// directory, through a chain of links, and to one outside both unit
/// directories. Each is a mask, as systemd reads it: one `Masked` state
/// change on the link's own name, over that name's vendor file and owner,
/// and nothing else, so no drop-in row and no config entry at its path.
/// Presence first: a local socket beside them is a config entry, so the
/// walk covers the directory. Control: the same links ending at a
/// non-empty file are an alias and a linked unit, with today's warnings
/// and no state change.
#[test]
fn t11_a_link_ending_at_an_empty_file_is_one_mask() {
    const VENDOR_SOCKET: &str = "[Socket]\nListenStream=1\n";
    const LOCAL: &str = "/etc/systemd/system/plain.socket";
    let host = |end: &str| {
        let mut host = UnitHost::new()
            .unit("plain.socket", "enabled")
            .etc_unit("plain.socket", VENDOR_SOCKET)
            .etc_unit("b.socket", end)
            .file("/opt/masks/off", end)
            .link(&etc_path("a.socket"), "b.socket")
            .link(&etc_path("c.socket"), "/etc/alternatives/c")
            .link("/etc/alternatives/c", &etc_path("b.socket"))
            .link(&etc_path("o.socket"), "/opt/masks/off");
        for unit in ["a.socket", "c.socket", "o.socket"] {
            host = host
                .unit(unit, if end.is_empty() { "masked" } else { "enabled" })
                .vendor_unit(unit, VENDOR_SOCKET, Some("sockpkg"));
        }
        host
    };

    let snap = scan(host(""));
    let config = snap.config.as_ref().expect("the config section");
    assert!(
        config.files.iter().any(|f| f.path == LOCAL),
        "the walk records a regular file in the unit directory"
    );
    let units = snap.units.as_ref().expect("the units section");
    for unit in ["a.socket", "c.socket", "o.socket"] {
        let path = etc_path(unit);
        let changes: Vec<_> = units
            .state_changes
            .iter()
            .filter(|c| c.unit().name().as_str() == unit)
            .collect();
        assert_eq!(changes.len(), 1, "{unit}: {:?}", units.state_changes);
        assert_eq!(changes[0].current_state(), UnitState::Masked, "{unit}");
        assert!(
            matches!(
                fragment_of(&snap, unit),
                UnitFragment::Vendor { file, owning_package: Some(p) }
                    if file.as_str() == unit && p == "sockpkg"
            ),
            "{unit}: {:?}",
            fragment_of(&snap, unit)
        );
        assert!(
            !units.drop_ins.iter().any(|d| d.path == path),
            "{unit}: {:?}",
            units.drop_ins
        );
        assert!(!config.files.iter().any(|f| f.path == path), "{unit}");
    }

    let full = scan(host(VENDOR_SOCKET));
    let units = full.units.as_ref().expect("the units section");
    for unit in ["a.socket", "c.socket", "o.socket"] {
        assert!(
            !units
                .state_changes
                .iter()
                .any(|c| c.unit().name().as_str() == unit),
            "{unit}"
        );
    }
    let warned = |unit: &str, detail: &str| {
        full.warnings
            .iter()
            .any(|w| w.message.starts_with(&format!("unit {unit} ")) && w.message.contains(detail))
    };
    assert!(
        warned("a.socket", "unit is an alias of b.socket"),
        "{:?}",
        full.warnings
    );
    assert!(warned("c.socket", "unit is an alias of b.socket"));
    assert!(warned("o.socket", "linked unit requires manual handling"));
}

/// T11: a comment-only file over a vendor unit is a full shadow, staged by
/// the drop-in writer, and no `mask` names its unit. Control: the
/// zero-length form of the same file over the same unit is a mask, in the
/// `mask` line, with no file staged.
#[test]
fn t11_a_comment_only_shadow_is_staged_and_not_masked() {
    const UNIT: &str = "quiet.service";
    let content = "# switched off by hand\n";
    let snap = scan(
        UnitHost::new()
            .unit(UNIT, "static")
            .vendor_unit(UNIT, VENDOR_SERVICE, Some("quietpkg"))
            .etc_unit(UNIT, content),
    );
    let tree = render_export(&snap);
    assert_eq!(
        std::fs::read_to_string(tree.path().join("drop-ins").join(&etc_path(UNIT)[1..])).ok(),
        Some(content.to_owned()),
        "the shadow is staged under drop-ins/"
    );
    let containerfile = containerfile_in(&tree);
    assert!(
        in_no_systemctl_instruction(&containerfile, UNIT),
        "{containerfile}"
    );

    let zero_length = scan(
        UnitHost::new()
            .unit(UNIT, "masked")
            .vendor_unit(UNIT, VENDOR_SERVICE, Some("quietpkg"))
            .etc_unit(UNIT, ""),
    );
    let tree = render_export(&zero_length);
    assert!(!staged_at(&tree, &etc_path(UNIT)), "no file is staged");
    let containerfile = containerfile_in(&tree);
    assert!(acts(&containerfile, "mask", UNIT), "{containerfile}");
}

// ---------------------------------------------------------------------
// § 10.7 order, § 10.1 header and README sentences, through the scan.
// ---------------------------------------------------------------------

/// The Containerfile's section headers, in order.
fn section_headers(containerfile: &str) -> Vec<&str> {
    containerfile
        .lines()
        .filter(|l| l.starts_with("# === ") && l.ends_with(" ==="))
        .collect()
}

/// T13, order: a local service with no paired timer. The config `COPY` that
/// stages its file and its enable both exist, and the enable follows. The
/// unit state section, under § 10.1's header, is the section directly
/// after Configuration Files.
#[test]
fn t13_a_local_unit_is_enabled_after_the_copy_of_its_file() {
    let snap = scan(t5_host());
    let tree = render_export(&snap);
    assert!(
        tree.path().join("config").join(&LOCAL_UNIT[1..]).is_file(),
        "the local unit's file is staged in the config tree"
    );
    let containerfile = containerfile_in(&tree);
    let copy = containerfile
        .find("\nCOPY config/etc/ /etc/\n")
        .unwrap_or_else(|| panic!("no config COPY:\n{containerfile}"));
    let enable = containerfile
        .find("\n    'local.service'")
        .unwrap_or_else(|| panic!("no enable:\n{containerfile}"));
    assert!(
        acts(&containerfile, "enable", "local.service"),
        "{containerfile}"
    );
    assert!(
        copy < enable,
        "the enable precedes the COPY:\n{containerfile}"
    );

    let headers = section_headers(&containerfile);
    let config = headers
        .iter()
        .position(|h| *h == "# === Configuration Files ===")
        .unwrap_or_else(|| panic!("no Configuration Files section: {headers:?}"));
    assert_eq!(headers.get(config + 1), Some(&UNIT_SECTION), "{headers:?}");
}

/// § 10.7: a full shadow's enable follows the drop-ins `COPY` that stages
/// the shadow, so it links to the `/etc` file.
#[test]
fn a_full_shadow_is_enabled_after_the_copy_of_its_shadow() {
    let tree = render_export(&scan(shadow_host("enabled", "disable")));
    assert!(staged_at(&tree, SHADOW_UNIT), "the shadow is staged");
    let containerfile = containerfile_in(&tree);
    let copy = containerfile
        .find("\nCOPY drop-ins/etc/systemd/system/ /etc/systemd/system/\n")
        .unwrap_or_else(|| panic!("no drop-ins COPY:\n{containerfile}"));
    assert!(
        acts(&containerfile, "enable", "shadow.service"),
        "{containerfile}"
    );
    let enable = containerfile
        .find("\n    'shadow.service'")
        .expect("the enable's operand line");
    assert!(
        copy < enable,
        "the enable precedes the COPY:\n{containerfile}"
    );
}

/// § 10.1's sentences about what a carried unit state means on the
/// target: the two first-boot sentences of § 10.2 "Never the machine-id",
/// then `preset-all`.
const README_FIRST_BOOT: &str = "The unit states in the Containerfile's Systemd Unit State \
     section survive first boot with the base image's empty `/etc/machine-id`. A later layer or \
     installer script that removes that file or writes `uninitialized` to it makes first boot \
     apply the presets, which can undo the rendered `enable` and `disable` lines (measured: \
     RHEL 9 kept an `enable` and undid a `disable`, RHEL 10 undid both). A rendered `mask` is \
     kept. A later `systemctl preset-all` returns enabled and disabled units to their presets \
     and leaves masks in place.";
const README_DISABLE: &str = "A `disable` removes the administrator's links and nothing more: \
     a unit that a package's `.wants` link or another unit's `Wants=` pulls in reads \
     `disabled` afterwards and is still wanted.";

/// § 10.1: the README states both sentences when the unit state section
/// carries a command. Controls: a host whose only unit is a vendor unit at
/// its preset has no unit state section, and a host whose only line there
/// is a refused command, or a mount advisory, has a section that carries
/// nothing; no such README has either sentence.
#[test]
fn the_readme_explains_carried_unit_states_when_there_are_any() {
    let readme = |tree: &tempfile::TempDir| {
        std::fs::read_to_string(tree.path().join("README.md")).expect("README.md")
    };
    let tree = render_export(&scan(t5_host()));
    let containerfile = containerfile_in(&tree);
    assert!(containerfile.contains(UNIT_SECTION), "{containerfile}");
    let text = readme(&tree);
    for sentence in [README_FIRST_BOOT, README_DISABLE] {
        assert!(text.contains(sentence), "missing {sentence:?}:\n{text}");
    }

    let at_preset = render_export(&scan(
        UnitHost::new()
            .unit("sshd.service", "enabled")
            .preset("enable sshd.service")
            .vendor_unit("sshd.service", VENDOR_SERVICE, Some("openssh-server")),
    ));
    let containerfile = containerfile_in(&at_preset);
    assert!(!containerfile.contains(UNIT_SECTION), "{containerfile}");
    let text = readme(&at_preset);
    for sentence in [README_FIRST_BOOT, README_DISABLE] {
        assert!(
            !text.contains(sentence),
            "{sentence:?} without a section:\n{text}"
        );
    }

    // A section that carries nothing: its only line is a protected unit's
    // refused `disable`. The section exists, and neither sentence does.
    const TIMER: &str = "bootc-fetch-apply-updates.timer";
    let declined_only = render_export(&scan(
        UnitHost::new()
            .unit(TIMER, "disabled")
            .preset(&format!("enable {TIMER}"))
            .vendor_unit(
                TIMER,
                "[Timer]\nOnCalendar=daily\n[Install]\nWantedBy=timers.target\n",
                Some("bootc"),
            ),
    ));
    let containerfile = containerfile_in(&declined_only);
    assert_eq!(
        unit_lines(&containerfile),
        [protected_fixme(TIMER, "disabled")],
        "{containerfile}"
    );
    let text = readme(&declined_only);
    for sentence in [README_FIRST_BOOT, README_DISABLE] {
        assert!(
            !text.contains(sentence),
            "{sentence:?} with nothing carried:\n{text}"
        );
    }

    // A section that holds only a mount advisory (§ 10.1, § 10.6) carries
    // nothing either.
    let advisory_only = render_export(&scan(
        UnitHost::new()
            .unit("srv.mount", "enabled")
            .etc_unit("srv.mount", &mount_file("/srv", Some("nofail"))),
    ));
    let containerfile = containerfile_in(&advisory_only);
    assert_eq!(
        unit_lines(&containerfile),
        [
            advisory_fact("srv.mount", "enabled"),
            FILE_STAGED_LINE.to_owned(),
            var_symlink_line("/srv"),
        ],
        "{containerfile}"
    );
    let text = readme(&advisory_only);
    for sentence in [README_FIRST_BOOT, README_DISABLE] {
        assert!(
            !text.contains(sentence),
            "{sentence:?} with only an advisory:\n{text}"
        );
    }
}

// ---------------------------------------------------------------------
// § 10.6: mount, automount and swap advisories, through the scan (T8's
// advisory cases, T9, and T11's unowned storage masks).
// ---------------------------------------------------------------------

/// § 10.6's lines, verbatim.
fn advisory_fact(unit: &str, state: &str) -> String {
    format!(
        "# Advisory: {unit} is {state} on the source host. inspectah does not enable, disable \
         or mask a mount, automount or swap unit the administrator wrote."
    )
}
const FILE_STAGED_LINE: &str = "#   Its unit file is copied into the image. No enable line is \
     written, but the unit still starts if another unit requires it or names its mount point \
     in RequiresMountsFor=, and it replaces a unit of the same name that the image ships.";
fn usr_line(path: &str) -> String {
    format!(
        "#   /usr is part of the read-only image on the target. The mount point must already \
         exist in the image, and a mount there hides the image's own content under {path}."
    )
}
fn var_symlink_line(top: &str) -> String {
    format!(
        "#   {top} is a symlink into /var in the RHEL bootc base image. The unit must be renamed \
         for the resolved path (for /srv: var-srv.mount with Where=/var/srv)."
    )
}
/// The rename line for an automount, which keeps its suffix and has a
/// matching mount unit (decided by the project owner, 2026-11-14).
fn automount_var_symlink_line(top: &str) -> String {
    format!(
        "#   {top} is a symlink into /var in the RHEL bootc base image. The unit must be renamed \
         for the resolved path (for /srv: var-srv.automount with Where=/var/srv), and so must \
         its matching mount unit (var-srv.mount)."
    )
}
/// The line for a finding merged from more than one host.
const MERGED_LINE: &str =
    "#   This describes one host's copy of the unit; the copy staged may differ.";
fn opt_line(path: &str) -> String {
    format!(
        "#   /opt is a directory of the read-only image in the RHEL bootc base image, not a link \
         into /var. The unit keeps its name. The mount point must already exist in the image, \
         and a mount there hides the image's own content under {path}."
    )
}
const TMP_MOUNT_LINE: &str = "#   The bootc base image starts tmp.mount from a link it ships, \
     so this file replaces the unit that mounts /tmp on the target.";
const NOFAIL_LINE: &str = "#   No nofail option in the unit file. If the unit is started at \
     boot and its device is missing, units that require it fail, and boot can stop in emergency \
     mode.";
fn carry_line(verb: &str, unit: &str) -> String {
    format!("#   To carry it, add after the configuration COPY: RUN systemctl {verb} -- '{unit}'")
}

/// The advisory for `unit`: its `# Advisory:` line and the indented lines
/// after it. Empty when there is none.
fn advisory_for(containerfile: &str, unit: &str) -> Vec<String> {
    let first = format!("# Advisory: {unit} is ");
    let mut lines = containerfile.lines().skip_while(|l| !l.starts_with(&first));
    let Some(head) = lines.next() else {
        return Vec::new();
    };
    std::iter::once(head)
        .chain(lines.take_while(|l| l.starts_with("#   ")))
        .map(str::to_owned)
        .collect()
}

/// Whether every line of `containerfile` that names `unit` is a comment,
/// so no instruction (`RUN`, `COPY` or any other) names it.
fn only_comments_name(containerfile: &str, unit: &str) -> bool {
    containerfile
        .lines()
        .filter(|l| l.contains(unit))
        .all(|l| l.trim_start().starts_with('#'))
}

/// A mount unit file with `Where=` and, when given, `Options=`.
fn mount_file(mount_point: &str, options: Option<&str>) -> String {
    let options = options
        .map(|o| format!("Options={o}\n"))
        .unwrap_or_default();
    format!(
        "[Mount]\nWhat=/dev/vdb\nWhere={mount_point}\n{options}\
         [Install]\nWantedBy=local-fs.target\n"
    )
}

/// The control every advisory host carries: a package-owned mount unit
/// enabled against its preset, which is in `RUN systemctl enable`.
const PACKAGED_MOUNT: &str = "var-lib-pkg.mount";

/// A host with the package-owned control and `disable *` as its preset,
/// scanned against a base image so refine keeps the package included.
fn advisory_host() -> UnitHost {
    UnitHost::new()
        .base_package("glibc")
        .package("pkgmount")
        .unit(PACKAGED_MOUNT, "enabled")
        .preset("disable *")
        .vendor_unit(
            PACKAGED_MOUNT,
            &mount_file("/var/lib/pkg", None),
            Some("pkgmount"),
        )
}

/// The scan's and a refine session's Containerfile for `snap`.
fn both_routes(snap: &InspectionSnapshot) -> [(&'static str, String); 2] {
    [
        ("scan", containerfile_in(&render_export(snap))),
        ("refine", containerfile_in(&export_session(&session(snap)))),
    ]
}

/// T9: a local `srv.mount` gets the `/var` rename line; `Where=/opt/vendor`
/// the `/opt` line and no rename line; a mount under `/usr` the `/usr`
/// line. Each local unit file is staged, so each gets the file line. No
/// instruction names any of them, while the package-owned control on the
/// same host is enabled. The advisories sit in the unit state section,
/// after its commands (§ 10.1).
#[test]
fn t9_the_mount_advisory_text_through_the_scan() {
    let snap = scan(
        advisory_host()
            .unit("srv.mount", "enabled")
            .unit("opt-vendor.mount", "enabled")
            .unit("usr-share-data.mount", "enabled")
            .etc_unit("srv.mount", &mount_file("/srv", None))
            .etc_unit(
                "opt-vendor.mount",
                &mount_file("/opt/vendor", Some("nofail")),
            )
            .etc_unit(
                "usr-share-data.mount",
                &mount_file("/usr/share/data", Some("defaults,nofail")),
            ),
    );
    let expected = [
        (
            "srv.mount",
            vec![
                advisory_fact("srv.mount", "enabled"),
                FILE_STAGED_LINE.to_owned(),
                var_symlink_line("/srv"),
                NOFAIL_LINE.to_owned(),
            ],
        ),
        (
            "opt-vendor.mount",
            vec![
                advisory_fact("opt-vendor.mount", "enabled"),
                FILE_STAGED_LINE.to_owned(),
                opt_line("/opt/vendor"),
                carry_line("enable", "opt-vendor.mount"),
            ],
        ),
        (
            "usr-share-data.mount",
            vec![
                advisory_fact("usr-share-data.mount", "enabled"),
                FILE_STAGED_LINE.to_owned(),
                usr_line("/usr/share/data"),
                carry_line("enable", "usr-share-data.mount"),
            ],
        ),
    ];
    for (route, containerfile) in both_routes(&snap) {
        for (unit, lines) in &expected {
            assert_eq!(
                &advisory_for(&containerfile, unit),
                lines,
                "{route}, {unit}:\n{containerfile}"
            );
            assert!(
                in_no_systemctl_instruction(&containerfile, unit),
                "{route}, {unit}:\n{containerfile}"
            );
            assert!(
                only_comments_name(&containerfile, unit),
                "{route}, {unit}:\n{containerfile}"
            );
        }
        assert!(
            acts(&containerfile, "enable", PACKAGED_MOUNT),
            "{route}: the control:\n{containerfile}"
        );
        // § 10.1: the advisories are in the unit state section, after its
        // last command line, with no section header of their own.
        let section = unit_lines(&containerfile);
        let last_command = section.iter().rposition(|l| !l.starts_with('#'));
        let first_advisory = section.iter().position(|l| l.starts_with("# Advisory: "));
        assert!(
            last_command.is_some() && first_advisory > last_command,
            "{route}: {section:?}"
        );
        for (unit, lines) in &expected {
            assert!(
                section.windows(lines.len()).any(|w| w == lines.as_slice()),
                "{route}, {unit}: not in the unit state section: {section:?}"
            );
        }
        let headers = section_headers(&containerfile);
        assert!(
            headers.iter().all(|h| !h.contains("Mount")),
            "{route}: {headers:?}"
        );
    }
}

/// T9: the file line follows the unit file's config row. Excluding the row
/// through `RefineSession` alone removes the staged file and the line;
/// including it again restores both. The rest of the advisory stays, and
/// no instruction names the unit either way.
#[test]
fn t9_the_file_line_follows_the_config_row() {
    const UNIT: &str = "srv.mount";
    let path = etc_path(UNIT);
    let snap = scan(
        advisory_host()
            .unit(UNIT, "enabled")
            .etc_unit(UNIT, &mount_file("/srv", Some("nofail"))),
    );
    let without_file = vec![advisory_fact(UNIT, "enabled"), var_symlink_line("/srv")];
    let mut with_file = without_file.clone();
    with_file.insert(1, FILE_STAGED_LINE.to_owned());

    let mut session = session(&snap);
    for (step, include, lines) in [
        ("at scan", None, &with_file),
        ("excluded", Some(false), &without_file),
        ("included again", Some(true), &with_file),
    ] {
        if let Some(include) = include {
            session
                .apply(set_include(config_item(&path), include))
                .unwrap_or_else(|e| panic!("{step}: {e:?}"));
        }
        let tree = export_session(&session);
        let containerfile = containerfile_in(&tree);
        assert_eq!(
            staged_at(&tree, &path),
            include != Some(false),
            "{step}: the staged file"
        );
        assert_eq!(
            &advisory_for(&containerfile, UNIT),
            lines,
            "{step}:\n{containerfile}"
        );
        assert!(
            only_comments_name(&containerfile, UNIT),
            "{step}:\n{containerfile}"
        );
    }
}

/// T8, advisory cases: a local unit of each storage kind, enabled, and a
/// vendor mount unit whose owner lookup fails, enabled: each gets the
/// advisory and no instruction. A mount without `nofail` and a swap
/// without it get the `nofail` line; a swap with it and an automount do
/// not. The package-owned control is enabled.
#[test]
fn t8_an_administrators_storage_unit_gets_the_advisory_and_no_instruction() {
    let snap = scan(
        advisory_host()
            .unit("srv-data.mount", "enabled")
            .unit("srv-data.automount", "enabled")
            .unit("dev-vdb1.swap", "enabled")
            .unit("dev-vdb2.swap", "enabled")
            .unit("data-vendor.mount", "enabled")
            .etc_unit("srv-data.mount", &mount_file("/srv/data", None))
            .etc_unit(
                "srv-data.automount",
                "[Automount]\nWhere=/srv/data\n[Install]\nWantedBy=local-fs.target\n",
            )
            .etc_unit(
                "dev-vdb1.swap",
                "[Swap]\nWhat=/dev/vdb1\n[Install]\nWantedBy=swap.target\n",
            )
            .etc_unit(
                "dev-vdb2.swap",
                "[Swap]\nWhat=/dev/vdb2\nOptions=nofail\n[Install]\nWantedBy=swap.target\n",
            )
            .vendor_unit(
                "data-vendor.mount",
                &mount_file("/data/vendor", Some("nofail")),
                None,
            ),
    );
    let expected = [
        (
            "srv-data.mount",
            vec![
                advisory_fact("srv-data.mount", "enabled"),
                FILE_STAGED_LINE.to_owned(),
                var_symlink_line("/srv"),
                NOFAIL_LINE.to_owned(),
            ],
        ),
        (
            "srv-data.automount",
            vec![
                advisory_fact("srv-data.automount", "enabled"),
                FILE_STAGED_LINE.to_owned(),
                automount_var_symlink_line("/srv"),
            ],
        ),
        (
            "dev-vdb1.swap",
            vec![
                advisory_fact("dev-vdb1.swap", "enabled"),
                FILE_STAGED_LINE.to_owned(),
                NOFAIL_LINE.to_owned(),
                carry_line("enable", "dev-vdb1.swap"),
            ],
        ),
        (
            "dev-vdb2.swap",
            vec![
                advisory_fact("dev-vdb2.swap", "enabled"),
                FILE_STAGED_LINE.to_owned(),
                carry_line("enable", "dev-vdb2.swap"),
            ],
        ),
        (
            "data-vendor.mount",
            vec![
                advisory_fact("data-vendor.mount", "enabled"),
                carry_line("enable", "data-vendor.mount"),
            ],
        ),
    ];
    for (route, containerfile) in both_routes(&snap) {
        for (unit, lines) in &expected {
            assert_eq!(
                &advisory_for(&containerfile, unit),
                lines,
                "{route}, {unit}:\n{containerfile}"
            );
            assert!(
                in_no_systemctl_instruction(&containerfile, unit),
                "{route}, {unit}:\n{containerfile}"
            );
            assert!(
                only_comments_name(&containerfile, unit),
                "{route}, {unit}:\n{containerfile}"
            );
        }
        assert!(
            acts(&containerfile, "enable", PACKAGED_MOUNT),
            "{route}: the control:\n{containerfile}"
        );
    }
}

/// A swap unit whose file in `/etc/systemd/system` is not text (or is over
/// the size cap) was not read, so nothing is known of its options or its
/// `[Install]`: it is an undetermined record with no mount finding, and no
/// advisory states whether it is enabled or claims a `nofail` line. The
/// control on the same host and routes is a readable swap file without
/// `nofail`, which gets the line. The scan export and a refine export.
#[test]
fn an_unread_swap_unit_file_gets_no_nofail_line() {
    const UNREAD: &str = "dev-vdb5.swap";
    const READ: &str = "dev-vdb1.swap";
    let snap = scan(
        advisory_host()
            .unit(UNREAD, "enabled")
            .unit(READ, "enabled")
            .file_error(
                &format!("/etc/systemd/system/{UNREAD}"),
                std::io::ErrorKind::InvalidData,
            )
            .etc_unit(
                READ,
                "[Swap]\nWhat=/dev/vdb1\n[Install]\nWantedBy=swap.target\n",
            ),
    );
    let units = snap.units.as_ref().expect("a units section");
    let finding = |unit: &str| {
        units
            .mount_findings
            .iter()
            .find(|f| f.unit.name().as_unit_name().as_str() == unit)
    };
    assert_eq!(
        finding(READ).map(|f| f.nofail),
        Some(Some(false)),
        "the control was read"
    );
    assert!(
        units
            .undetermined_installs
            .iter()
            .any(|r| r.unit.as_str() == UNREAD && r.reason == InstallUnknown::Unreadable),
        "{:?}",
        units.undetermined_installs
    );
    assert!(finding(UNREAD).is_none(), "the file was not read");
    for (route, containerfile) in both_routes(&snap) {
        let control = advisory_for(&containerfile, READ);
        assert!(
            control.contains(&NOFAIL_LINE.to_owned()),
            "{route}, control:\n{containerfile}"
        );
        assert!(
            advisory_for(&containerfile, UNREAD).is_empty(),
            "{route}:\n{containerfile}"
        );
        assert!(
            in_no_systemctl_instruction(&containerfile, UNREAD),
            "{route}:\n{containerfile}"
        );
    }
}

/// An `/etc` storage unit whose `[Install]` cannot be relied on, with a
/// link to it under `/etc/systemd/system`, has no known enablement: one
/// with `%I` in `WantedBy=` (`Specifier`) and one assigning `UpheldBy=`.
/// Each keeps its undetermined record and block, and gets no mount
/// finding, so nothing says it is not enabled. Controls on the same host
/// and routes: the readable section with the same link is an enabled
/// finding, and the readable section with no link is still recorded by the
/// storage pass as not enabled. The snapshot, the refine view, the scan
/// export and a refine export.
#[test]
fn an_undetermined_storage_unit_is_never_said_to_be_not_enabled() {
    const SPECIFIER: &str = "srv-spec.mount";
    const UPHELD: &str = "srv-upheld.mount";
    const LINKED: &str = "srv-linked.mount";
    const UNLINKED: &str = "srv-unlinked.mount";
    let unit_file = |mount_point: &str, install: &str| {
        format!("[Mount]\nWhat=/dev/vdb\nWhere={mount_point}\n[Install]\n{install}")
    };
    let wants_link = |unit: &str| format!("/etc/systemd/system/local-fs.target.wants/{unit}");
    let mut host = advisory_host();
    for (unit, mount_point, install) in [
        (
            SPECIFIER,
            "/srv/spec",
            "WantedBy=local-fs.target %I.target\n",
        ),
        (
            UPHELD,
            "/srv/upheld",
            "WantedBy=local-fs.target\nUpheldBy=local-fs.target\n",
        ),
        (LINKED, "/srv/linked", "WantedBy=local-fs.target\n"),
    ] {
        host = host
            .unit(unit, "enabled")
            .etc_unit(unit, &unit_file(mount_point, install))
            .link(&wants_link(unit), &etc_path(unit));
    }
    let snap = scan(host.unit(UNLINKED, "disabled").etc_unit(
        UNLINKED,
        &unit_file("/srv/unlinked", "WantedBy=local-fs.target\n"),
    ));

    let units = snap.units.as_ref().expect("a units section");
    let finding = |unit: &str| {
        units
            .mount_findings
            .iter()
            .find(|f| f.unit.name().as_unit_name().as_str() == unit)
            .map(|f| f.state_change.map(|(state, _)| state))
    };
    assert_eq!(finding(LINKED), Some(Some(UnitState::Enabled)), "control");
    assert_eq!(finding(UNLINKED), Some(None), "control: the storage pass");
    let undetermined = [
        (SPECIFIER, InstallUnknown::Specifier),
        (UPHELD, InstallUnknown::UpheldBy),
    ];
    for (unit, reason) in undetermined {
        assert!(
            units
                .undetermined_installs
                .iter()
                .any(|r| r.unit.as_str() == unit
                    && r.reason == reason
                    && r.install_files == [etc_path(unit)]),
            "{unit}: {:?}",
            units.undetermined_installs
        );
        assert_eq!(finding(unit), None, "{unit}: no mount finding");
    }

    let refine = session(&snap);
    let decisions = refine.decisions();
    let mount_row = |unit: &str| {
        decisions
            .mount_findings
            .iter()
            .any(|r| r.unit.as_str() == unit)
    };
    assert!(mount_row(LINKED) && mount_row(UNLINKED), "control rows");
    for (unit, _) in undetermined {
        assert!(!mount_row(unit), "{unit}: no mount row");
        assert!(
            decisions
                .undetermined_installs
                .iter()
                .any(|r| r.unit.as_str() == unit),
            "{unit}: the undetermined row"
        );
    }

    for (route, containerfile) in both_routes(&snap) {
        assert_eq!(
            advisory_for(&containerfile, LINKED).first(),
            Some(&advisory_fact(LINKED, "enabled")),
            "{route}, control:\n{containerfile}"
        );
        assert_eq!(
            advisory_for(&containerfile, UNLINKED).first(),
            Some(&advisory_fact(UNLINKED, "not enabled")),
            "{route}, control:\n{containerfile}"
        );
        for (unit, _) in undetermined {
            assert!(
                !containerfile.contains(&format!("{unit} is not enabled")),
                "{route}: {unit}:\n{containerfile}"
            );
            assert!(
                containerfile.contains(&format!(
                    "# Advisory: {unit} is not carried: its [Install] section"
                )),
                "{route}: {unit}'s block:\n{containerfile}"
            );
            assert!(
                in_no_systemctl_instruction(&containerfile, unit),
                "{route}: {unit}:\n{containerfile}"
            );
        }
    }
}

/// T8: a `tmp.mount` with a full copy in `/etc` over the vendor file gets
/// the advisory with the `tmp.mount` line, and its file is a config entry
/// whose toggle, through `RefineSession` alone, removes and restores the
/// staged file and, with it, the file and `tmp.mount` lines. No
/// instruction names it at any step.
#[test]
fn t8_a_tmp_mount_copy_in_etc_gets_the_tmp_mount_line() {
    const UNIT: &str = "tmp.mount";
    let path = etc_path(UNIT);
    let vendor = "[Mount]\nWhat=tmpfs\nWhere=/tmp\nOptions=mode=1777,strictatime\n";
    let snap = scan(
        advisory_host()
            .unit(UNIT, "static")
            .vendor_unit(UNIT, vendor, Some("systemd"))
            .etc_unit(
                UNIT,
                "[Mount]\nWhat=tmpfs\nWhere=/tmp\nOptions=mode=1777,size=2G\n",
            ),
    );
    let entry = snap
        .config
        .as_ref()
        .expect("a config section")
        .files
        .iter()
        .find(|f| f.path == path);
    assert!(entry.is_some(), "the copy is a config entry");
    let staged_lines = vec![
        advisory_fact(UNIT, "not enabled"),
        FILE_STAGED_LINE.to_owned(),
        TMP_MOUNT_LINE.to_owned(),
        NOFAIL_LINE.to_owned(),
    ];
    let unstaged_lines = vec![advisory_fact(UNIT, "not enabled"), NOFAIL_LINE.to_owned()];

    let mut session = session(&snap);
    for (step, include, lines) in [
        ("at scan", None, &staged_lines),
        ("excluded", Some(false), &unstaged_lines),
        ("included again", Some(true), &staged_lines),
    ] {
        if let Some(include) = include {
            session
                .apply(set_include(config_item(&path), include))
                .unwrap_or_else(|e| panic!("{step}: {e:?}"));
        }
        let tree = export_session(&session);
        let containerfile = containerfile_in(&tree);
        assert_eq!(
            staged_at(&tree, &path),
            include != Some(false),
            "{step}: the staged file"
        );
        assert_eq!(
            &advisory_for(&containerfile, UNIT),
            lines,
            "{step}:\n{containerfile}"
        );
        assert!(
            only_comments_name(&containerfile, UNIT),
            "{step}:\n{containerfile}"
        );
        assert!(
            acts(&containerfile, "enable", PACKAGED_MOUNT),
            "{step}: the control:\n{containerfile}"
        );
    }
}

/// T11, unowned storage masks: for each storage kind and both mask forms,
/// an unowned vendor file or none gives the advisory with state `masked`,
/// the `To carry it` line with `mask`, and no instruction. The zero-length
/// form is withheld, so no file line. A masked unit does not mount, so its
/// advisory has no layout line and no `nofail` line, though the vendor
/// file's `Where=` is under `/srv` and it has no `Options=`. A masked
/// `tmp.mount` gets no `To carry it` line, since § 10.3 refuses that mask,
/// and an enabled unit under `/srv` none, since it is told to rename
/// itself. Controls: the same mask over a package-owned file is a `mask` command
/// and has no advisory; the same unowned unit enabled gets the layout line
/// and, for a mount or swap, the `nofail` line.
#[test]
fn t11_an_unowned_storage_mask_gets_the_advisory_and_no_instruction() {
    for (unit, vendor_file) in [
        ("srv-data.mount", mount_file("/srv/data", None)),
        (
            "srv-data.automount",
            "[Automount]\nWhere=/srv/data\n".to_owned(),
        ),
        ("dev-vdb1.swap", "[Swap]\nWhat=/dev/vdb1\n".to_owned()),
        ("tmp.mount", "[Mount]\nWhat=tmpfs\nWhere=/tmp\n".to_owned()),
    ] {
        let has_options = !unit.ends_with(".automount");
        let under_srv = unit.starts_with("srv-data.");

        let enabled = scan(UnitHost::new().unit(unit, "enabled").vendor_unit(
            unit,
            &format!("{vendor_file}\n[Install]\nWantedBy=multi-user.target\n"),
            None,
        ));
        let mut expected = vec![advisory_fact(unit, "enabled")];
        if under_srv && unit.ends_with(".automount") {
            expected.push(automount_var_symlink_line("/srv"));
        } else if under_srv {
            expected.push(var_symlink_line("/srv"));
        }
        if has_options {
            expected.push(NOFAIL_LINE.to_owned());
        }
        // A unit told to rename itself gets no recipe under its old name.
        if !under_srv {
            expected.push(carry_line("enable", unit));
        }
        let containerfile = containerfile_in(&render_export(&enabled));
        assert_eq!(
            advisory_for(&containerfile, unit),
            expected,
            "{unit}, enabled, unowned:\n{containerfile}"
        );

        for form in MASK_FORMS {
            for owner in ["owned", "unowned", "no vendor file"] {
                let label = format!("{unit}, {form}, {owner}");
                let host = UnitHost::new().unit(unit, "masked");
                let host = match owner {
                    "owned" => host.vendor_unit(unit, &vendor_file, Some("pkg")),
                    "unowned" => host.vendor_unit(unit, &vendor_file, None),
                    _ => host,
                };
                let snap = scan(masked_by(host, unit, form));
                let tree = render_export(&snap);
                assert!(!staged_at(&tree, &etc_path(unit)), "{label}");
                let containerfile = containerfile_in(&tree);
                if owner == "owned" {
                    if unit == "tmp.mount" {
                        assert_eq!(
                            fixme_for(&containerfile, unit),
                            Some(TMP_MOUNT_FIXME),
                            "{label}"
                        );
                    } else {
                        assert!(
                            acts(&containerfile, "mask", unit),
                            "{label}:\n{containerfile}"
                        );
                    }
                    assert!(advisory_for(&containerfile, unit).is_empty(), "{label}");
                    continue;
                }
                let mut expected = vec![advisory_fact(unit, "masked")];
                if unit != "tmp.mount" {
                    expected.push(carry_line("mask", unit));
                }
                assert_eq!(
                    advisory_for(&containerfile, unit),
                    expected,
                    "{label}:\n{containerfile}"
                );
                assert!(
                    in_no_systemctl_instruction(&containerfile, unit),
                    "{label}:\n{containerfile}"
                );
                assert!(
                    only_comments_name(&containerfile, unit),
                    "{label}:\n{containerfile}"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------
// § 10.4 and § 12 "The exception": runtime package updaters (T17, T18 (b)).
// ---------------------------------------------------------------------

/// Every name on the updater list, with the package that owns it.
const UPDATERS: [(&str, &str); 5] = [
    ("dnf-automatic.timer", "dnf-automatic"),
    ("dnf-automatic-install.timer", "dnf-automatic"),
    ("dnf-automatic-download.timer", "dnf-automatic"),
    ("dnf-automatic-notifyonly.timer", "dnf-automatic"),
    ("packagekit-offline-update.service", "PackageKit"),
];

/// A near miss and the positive control: the service the timers start is
/// not on the list, and its enable is carried.
const UPDATER_CONTROL: &str = "dnf-automatic.service";

/// § 10.4's `# Omitted:` line for an excluded updater.
fn updater_omitted(unit: &str) -> String {
    format!(
        "# Omitted: {unit} (updates packages in place, which an image-based host does not \
         support; excluded by default)"
    )
}

/// A host with every updater and the control `state` against the opposite
/// preset, scanned against a base image so refine keeps the packages.
fn updater_host(state: &str) -> UnitHost {
    let preset = if state == "enabled" {
        "disable"
    } else {
        "enable"
    };
    let mut host = UnitHost::new()
        .base_package("glibc")
        .package("dnf-automatic")
        .package("PackageKit")
        .unit(UPDATER_CONTROL, state)
        .preset(&format!("{preset} *"))
        .vendor_unit(UPDATER_CONTROL, VENDOR_SERVICE, Some("dnf-automatic"));
    for (unit, owner) in UPDATERS {
        let content = if unit.ends_with(".timer") {
            "[Timer]\nOnCalendar=daily\n[Install]\nWantedBy=timers.target\n"
        } else {
            VENDOR_SERVICE
        };
        host = host
            .unit(unit, state)
            .vendor_unit(unit, content, Some(owner));
    }
    host
}

/// `session` exported to a tarball and opened again by the production
/// refine loader, as `inspectah refine` opens a refined export.
fn reopened(session: &RefineSession) -> RefineSession {
    let packed = tempfile::tempdir().expect("tempdir");
    let tarball = packed.path().join("refined.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball)
        .expect("the export writes");
    inspectah_refine::tarball::from_tarball(&tarball).expect("refine opens the export")
}

/// T17: each updater enabled against its preset is a state change with
/// `include: false`, the reason, and no lock; the Containerfile has
/// § 10.4's `# Omitted:` line and no instruction naming it, while the near
/// miss `dnf-automatic.service` on the same host is enabled. Through the
/// production refine loader the row is still excluded and not locked;
/// `SetInclude(true)` yields the enable and drops the line; the include
/// survives an export and a reload.
#[test]
fn t17_a_runtime_updater_is_excluded_by_default_and_can_be_included() {
    let snap = scan(updater_host("enabled"));
    for (unit, _) in UPDATERS {
        let row = change(&snap, unit);
        assert!(!row.disposition.is_included(), "{unit}");
        assert!(!row.locked, "{unit}");
        assert_eq!(
            row.attention_reason.as_deref(),
            Some(inspectah_core::baseline::RUNTIME_UPDATER_REASON),
            "{unit}"
        );
    }
    assert!(change(&snap, UPDATER_CONTROL).disposition.is_included());

    let mut session = session(&snap);
    for (route, containerfile) in [
        ("scan", containerfile_in(&render_export(&snap))),
        ("refine", containerfile_in(&export_session(&session))),
    ] {
        for (unit, _) in UPDATERS {
            assert_eq!(
                omitted(&containerfile, unit),
                Some(updater_omitted(unit).as_str()),
                "{route}, {unit}:\n{containerfile}"
            );
            assert!(
                in_no_systemctl_instruction(&containerfile, unit),
                "{route}, {unit}:\n{containerfile}"
            );
        }
        assert!(
            acts(&containerfile, "enable", UPDATER_CONTROL),
            "{route}:\n{containerfile}"
        );
    }

    for (unit, _) in UPDATERS {
        let row = change(session.snapshot(), unit);
        assert!(
            !row.locked && !row.disposition.is_included(),
            "{unit} through the loader: {row:?}"
        );
        session
            .apply(set_include(unit_item(unit), true))
            .unwrap_or_else(|e| panic!("SetInclude({unit}, true): {e:?}"));
    }
    let included = containerfile_in(&export_session(&session));
    let reloaded = reopened(&session);
    let after_reload = containerfile_in(&export_session(&reloaded));
    for (step, containerfile) in [("included", &included), ("reloaded", &after_reload)] {
        for (unit, _) in UPDATERS {
            assert!(
                acts(containerfile, "enable", unit),
                "{step}, {unit}:\n{containerfile}"
            );
            assert!(
                omitted(containerfile, unit).is_none(),
                "{step}, {unit}:\n{containerfile}"
            );
        }
    }
    for (unit, _) in UPDATERS {
        assert!(
            change(reloaded.snapshot(), unit).disposition.is_included(),
            "{unit}: the include survives the reload"
        );
    }
}

/// § 10.4 names units, not commands: an updater disabled against an
/// `enable` preset is excluded by default too, with the same reason and
/// line, and including it yields the `disable`. Control: the near miss is
/// disabled.
#[test]
fn t17_the_updater_default_applies_to_a_disable_too() {
    let snap = scan(updater_host("disabled"));
    let containerfile = containerfile_in(&render_export(&snap));
    for (unit, _) in UPDATERS {
        assert!(!change(&snap, unit).disposition.is_included(), "{unit}");
        assert_eq!(
            omitted(&containerfile, unit),
            Some(updater_omitted(unit).as_str()),
            "{unit}:\n{containerfile}"
        );
        assert!(
            in_no_systemctl_instruction(&containerfile, unit),
            "{unit}:\n{containerfile}"
        );
    }
    assert!(
        acts(&containerfile, "disable", UPDATER_CONTROL),
        "{containerfile}"
    );
    let included = exported_after(&snap, &[(unit_item("dnf-automatic.timer"), true)]);
    let containerfile = containerfile_in(&included);
    assert!(
        acts(&containerfile, "disable", "dnf-automatic.timer"),
        "{containerfile}"
    );
}

/// T18 (b): two hosts with `dnf-automatic.timer` enabled, each excluded by
/// the scan, merge to one excluded row with the reason; the merged
/// Containerfile has the `# Omitted:` line and no instruction naming it,
/// while the near miss is enabled. An explicit include in a refine session
/// on the aggregate, with the owning package included, yields the enable.
#[test]
fn t18_b_a_runtime_updater_merges_excluded_and_can_be_included() {
    const UNIT: &str = "dnf-automatic.timer";
    let host = |hostname: &str| {
        reload(&scan(
            updater_host("enabled").file("/etc/hostname", &format!("{hostname}\n")),
        ))
    };
    let (merged, _) =
        merge_snapshots(vec![host("host-a"), host("host-b")], None, None).expect("merge");
    let merged = reload(&merged);
    assert!(merged.aggregate_meta.is_some(), "an aggregate");
    let row = change(&merged, UNIT);
    assert!(!row.disposition.is_included() && !row.locked, "{row:?}");
    assert_eq!(
        row.attention_reason.as_deref(),
        Some(inspectah_core::baseline::RUNTIME_UPDATER_REASON)
    );
    let containerfile = containerfile_in(&render_export(&merged));
    assert_eq!(
        omitted(&containerfile, UNIT),
        Some(updater_omitted(UNIT).as_str()),
        "{containerfile}"
    );
    assert!(
        in_no_systemctl_instruction(&containerfile, UNIT),
        "{containerfile}"
    );
    assert!(
        acts(&containerfile, "enable", UPDATER_CONTROL),
        "{containerfile}"
    );

    // Refine on an aggregate starts the merged packages excluded, which
    // would decline every enable of theirs by the presence tiers; the
    // package is included first, and the near miss shows it is present.
    let package = ItemId::Package {
        name: "dnf-automatic".into(),
        arch: "x86_64".into(),
    };
    let included = containerfile_in(&exported_after(
        &merged,
        &[(package, true), (unit_item(UNIT), true)],
    ));
    assert!(acts(&included, "enable", UNIT), "{included}");
    assert!(omitted(&included, UNIT).is_none(), "{included}");
    assert!(acts(&included, "enable", UPDATER_CONTROL), "{included}");
}

// ---------------------------------------------------------------------
// § 12: a merged export is validated at render (T18 (a), (c)), and mount
// findings merge by unit name.
// ---------------------------------------------------------------------

/// `host` scanned as the host named `hostname`, written and read back.
fn named(host: UnitHost, hostname: &str) -> InspectionSnapshot {
    reload(&scan(host.file("/etc/hostname", &format!("{hostname}\n"))))
}

/// The two hosts merged, written and read back.
fn merged(a: InspectionSnapshot, b: InspectionSnapshot) -> InspectionSnapshot {
    let (merged, _) = merge_snapshots(vec![a, b], None, None).expect("the two hosts merge");
    let merged = reload(&merged);
    assert!(merged.aggregate_meta.is_some(), "an aggregate");
    merged
}

/// The package item for `name` as the host builder installs it.
fn package_item(name: &str) -> ItemId {
    ItemId::Package {
        name: name.into(),
        arch: "x86_64".into(),
    }
}

/// T18 (a): one host with a package-owned enabled mount unit, one with an
/// administrator's full copy of it in `/etc` and a drop-in for it, merged
/// and taken through `RefineSession`. Both rows are kept: the state change
/// from the first host, the finding, the config entry and the drop-in from
/// the second. While the file's config row is included, the file is
/// staged, no instruction names the unit and the advisory is present,
/// whichever way the unit row is toggled: its toggle cascades to the
/// drop-in, which leaves and rejoins the export, and does not move the
/// file decision. Excluding the file, with the unit row included, yields
/// the enable.
#[test]
fn t18_a_a_package_unit_beside_another_hosts_copy_is_decided_by_the_file() {
    for (package_hostname, copy_hostname) in [("host-a", "host-b"), ("host-b", "host-a")] {
        t18_a_case(package_hostname, copy_hostname);
    }
}

/// T18 (a) with the package-owned unit on `package_hostname` and the
/// administrator's copy on `copy_hostname`.
fn t18_a_case(package_hostname: &str, copy_hostname: &str) {
    const UNIT: &str = "var-lib-pkg.mount";
    const PACKAGE: &str = "pkgmount";
    let path = etc_path(UNIT);
    let drop_in = format!("{path}.d/x.conf");
    let vendor = mount_file("/var/lib/pkg", None);
    let copy = mount_file("/var/lib/pkg", Some("nofail"));
    let package_host = named(
        owned_unit_host(UNIT, PACKAGE, "enabled", &vendor),
        package_hostname,
    );
    let copy_host = named(
        owned_unit_host(UNIT, PACKAGE, "enabled", &vendor)
            .etc_unit(UNIT, &copy)
            .file(&drop_in, "[Mount]\nTimeoutSec=30\n"),
        copy_hostname,
    );
    assert!(
        matches!(
            change(&package_host, UNIT).unit(),
            CarriedUnit::PackagedStorage(_)
        ),
        "{package_hostname} records a package-owned state change"
    );
    let copy_units = copy_host.units.as_ref().expect("a units section");
    assert!(
        copy_units
            .state_changes
            .iter()
            .all(|c| c.unit().name().as_str() != UNIT),
        "{copy_hostname} records no state change for the unit"
    );
    assert!(
        copy_units
            .mount_findings
            .iter()
            .any(|f| f.unit.name().as_unit_name().as_str() == UNIT),
        "{copy_hostname} records a finding"
    );

    let merged = merged(package_host, copy_host);
    let units = merged.units.as_ref().expect("a units section");
    assert_eq!(
        change(&merged, UNIT)
            .aggregate
            .as_ref()
            .map(|a| a.hosts.clone()),
        Some(vec![package_hostname.to_owned()]),
        "the state change is {package_hostname}'s row"
    );
    assert_eq!(
        units
            .mount_findings
            .iter()
            .find(|f| f.unit.name().as_unit_name().as_str() == UNIT)
            .and_then(|f| f.aggregate.as_ref())
            .map(|a| a.hosts.clone()),
        Some(vec![copy_hostname.to_owned()]),
        "the finding is {copy_hostname}'s row"
    );
    assert!(matches!(
        change(&merged, UNIT).unit(),
        CarriedUnit::PackagedStorage(_)
    ));
    assert_eq!(
        units
            .mount_findings
            .iter()
            .filter(|f| f.unit.name().as_unit_name().as_str() == UNIT)
            .count(),
        1,
        "one merged finding"
    );

    let mut session = session(&merged);
    for (item, include) in [(package_item(PACKAGE), true), (config_item(&path), true)] {
        session
            .apply(set_include(item, include))
            .unwrap_or_else(|e| panic!("{e:?}"));
    }
    for unit_included in [true, false, true] {
        session
            .apply(set_include(unit_item(UNIT), unit_included))
            .unwrap_or_else(|e| panic!("{e:?}"));
        let tree = export_session(&session);
        let containerfile = containerfile_in(&tree);
        let step = format!("file included, unit row included {unit_included}");
        assert_eq!(
            std::fs::read_to_string(tree.path().join("config").join(&path[1..])).ok(),
            Some(copy.clone()),
            "{step}: {copy_hostname}'s file is staged"
        );
        assert_eq!(
            carriage_of(&session.snapshot_projected(), &drop_in).reaches_image(),
            unit_included,
            "{step}: the unit row's toggle cascades to its drop-in"
        );
        assert!(
            in_no_systemctl_instruction(&containerfile, UNIT),
            "{step}:\n{containerfile}"
        );
        let advisory = advisory_for(&containerfile, UNIT);
        assert_eq!(
            advisory.first(),
            Some(&advisory_fact(UNIT, "enabled")),
            "{step}:\n{containerfile}"
        );
        assert!(
            advisory.contains(&FILE_STAGED_LINE.to_owned()),
            "{step}:\n{containerfile}"
        );
        if unit_included {
            assert_eq!(
                omitted(&containerfile, UNIT),
                Some(
                    format!(
                        "# Omitted: {UNIT} (the export carries an administrator's unit file \
                         for it ({path}))"
                    )
                    .as_str()
                ),
                "{step}:\n{containerfile}"
            );
        }
    }

    session
        .apply(set_include(config_item(&path), false))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let tree = export_session(&session);
    let containerfile = containerfile_in(&tree);
    assert!(!staged_at(&tree, &path), "the file is excluded");
    assert!(acts(&containerfile, "enable", UNIT), "{containerfile}");
    assert!(omitted(&containerfile, UNIT).is_none(), "{containerfile}");
    // Current behavior, recorded for the project owner, not a requirement:
    // the merged finding's advisory still renders beside the enable the
    // plan now writes. Only its unit-file line follows the excluded file.
    // The finding is one host's, the only host that recorded the unit, so
    // the advisory has no line saying the staged copy may differ.
    let advisory = advisory_for(&containerfile, UNIT);
    assert_eq!(
        advisory,
        [advisory_fact(UNIT, "enabled"), carry_line("enable", UNIT)],
        "{containerfile}"
    );
    assert!(
        !advisory.contains(&FILE_STAGED_LINE.to_owned()),
        "{containerfile}"
    );
}

/// T18 (c): a mask on one host and a local unit file of the same name on
/// another. The merged row is the first host's mask (the two rows tie),
/// and the local file is a config entry on one host of two. Through
/// `RefineSession`: with the file included, T16's result, the mask
/// declined with `MaskPathOccupied` and the file staged; excluding it
/// brings the mask back. The mask follows its package (§ 10.2 rule 5,
/// T34): `kexec-tools` is on one host of two, so the merge excludes it,
/// and with the file excluded the mask is omitted for the package until
/// the package is included too.
#[test]
fn t18_c_a_mask_beside_another_hosts_unit_file_is_t16() {
    const UNIT: &str = "kdump.service";
    let path = etc_path(UNIT);
    let masked = named(
        owned_unit_host(UNIT, "kexec-tools", "masked", VENDOR_SERVICE),
        "host-a",
    );
    let local = named(t18_c_local_host(), "host-b");
    assert!(
        matches!(fragment_of(&local, UNIT), UnitFragment::Local { .. }),
        "host-b records a local unit"
    );
    let merged = merged(masked, local);
    assert_eq!(change(&merged, UNIT).current_state(), UnitState::Masked);

    let mut session = session(&merged);
    session
        .apply(set_include(config_item(&path), true))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let tree = export_session(&session);
    let occupied = containerfile_in(&tree);
    assert_eq!(
        std::fs::read_to_string(tree.path().join("config").join(&path[1..])).ok(),
        Some(T18_C_LOCAL.to_owned()),
        "host-b's unit file is staged"
    );
    assert_eq!(
        omitted(&occupied, UNIT),
        Some(
            format!(
                "# Omitted: {UNIT} (the export also carries a unit file at {path}; exclude one \
                 of the two)"
            )
            .as_str()
        ),
        "{occupied}"
    );
    assert!(in_no_systemctl_instruction(&occupied, UNIT), "{occupied}");

    session
        .apply(set_include(config_item(&path), false))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let package_out = containerfile_in(&export_session(&session));
    assert_eq!(
        omitted(&package_out, UNIT),
        Some("# Omitted: kdump.service (package 'kexec-tools' was excluded)"),
        "{package_out}"
    );
    session
        .apply(set_include(package_item("kexec-tools"), true))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let tree = export_session(&session);
    let freed = containerfile_in(&tree);
    assert!(!staged_at(&tree, &path), "the file is excluded");
    assert!(acts(&freed, "mask", UNIT), "{freed}");
    assert!(omitted(&freed, UNIT).is_none(), "{freed}");
}

/// The administrator's own `kdump.service` of T18 (c).
const T18_C_LOCAL: &str =
    "[Service]\nExecStart=/usr/local/bin/kdump\n[Install]\nWantedBy=multi-user.target\n";

/// The T18 (c) host with a local `kdump.service`, enabled.
fn t18_c_local_host() -> UnitHost {
    UnitHost::new()
        .unit("kdump.service", "enabled")
        .preset("disable *")
        .etc_unit("kdump.service", T18_C_LOCAL)
}

/// T18 (c) with the hostnames swapped: the local unit file's host is first,
/// so the two state change rows tie and the merged row is that host's,
/// `enabled` from a `Local` fragment, not the mask. With the file
/// included, the file's bytes are staged and the enable is carried; with
/// it excluded, the enable is declined because its unit file is not in the
/// export, and nothing masks the unit.
#[test]
fn t18_c_with_the_local_host_first_the_row_is_the_local_unit() {
    const UNIT: &str = "kdump.service";
    let path = etc_path(UNIT);
    let local = named(t18_c_local_host(), "host-a");
    let masked = named(
        owned_unit_host(UNIT, "kexec-tools", "masked", VENDOR_SERVICE),
        "host-b",
    );
    let merged = merged(local, masked);
    assert_eq!(change(&merged, UNIT).current_state(), UnitState::Enabled);
    assert!(
        matches!(fragment_of(&merged, UNIT), UnitFragment::Local { .. }),
        "host-a's row"
    );

    let mut session = session(&merged);
    session
        .apply(set_include(config_item(&path), true))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let tree = export_session(&session);
    assert_eq!(
        std::fs::read_to_string(tree.path().join("config").join(&path[1..])).ok(),
        Some(T18_C_LOCAL.to_owned()),
        "host-a's unit file is staged"
    );
    let included = containerfile_in(&tree);
    assert!(acts(&included, "enable", UNIT), "{included}");
    assert!(omitted(&included, UNIT).is_none(), "{included}");

    session
        .apply(set_include(config_item(&path), false))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let tree = export_session(&session);
    let excluded = containerfile_in(&tree);
    assert!(!staged_at(&tree, &path), "the file is excluded");
    assert!(in_no_systemctl_instruction(&excluded, UNIT), "{excluded}");
    assert_eq!(
        omitted(&excluded, UNIT),
        Some(format!("# Omitted: {UNIT} ({NOT_STAGED}").as_str()),
        "{excluded}"
    );
}

/// A local mount unit whose file has no `nofail` but which has a drop-in:
/// the collector reads `Options=` from the unit file alone and a drop-in
/// may set `nofail`, so the advisory gives no `nofail` line, on the scan
/// and the refine route. Reversed control: the same host without the
/// drop-in has the line.
#[test]
fn a_mount_unit_with_drop_ins_gets_no_nofail_line() {
    const UNIT: &str = "data.mount";
    let host = |with_drop_in: bool| {
        let host = UnitHost::new()
            .unit(UNIT, "enabled")
            .etc_unit(UNIT, &mount_file("/data", None));
        if with_drop_in {
            host.file(
                &format!("{}.d/options.conf", etc_path(UNIT)),
                "[Mount]\nOptions=nofail\n",
            )
        } else {
            host
        }
    };
    let with = scan(host(true));
    assert!(
        with.units
            .as_ref()
            .expect("a units section")
            .drop_ins
            .iter()
            .any(|d| d.unit == UNIT),
        "the drop-in row is recorded"
    );
    for (route, containerfile) in both_routes(&with) {
        let advisory = advisory_for(&containerfile, UNIT);
        assert_eq!(
            advisory.first(),
            Some(&advisory_fact(UNIT, "enabled")),
            "{route}:\n{containerfile}"
        );
        assert!(
            !advisory.contains(&NOFAIL_LINE.to_owned()),
            "{route}:\n{containerfile}"
        );
    }
    for (route, containerfile) in both_routes(&scan(host(false))) {
        assert!(
            advisory_for(&containerfile, UNIT).contains(&NOFAIL_LINE.to_owned()),
            "{route}, control:\n{containerfile}"
        );
    }
}

/// A merged mask finding beside another host's administrator file for the
/// same mount unit: one host masked `data.mount`, the other wrote it. The
/// merged finding is the first host's mask. While the file's config row is
/// included, the advisory gives no mask recipe, since `systemctl mask`
/// fails on the occupied path; excluded (the default, as the file is on
/// one host of two), the recipe is there. Both hosts recorded the unit, so
/// the advisory says the staged copy may differ from the one it describes.
/// Through `merge_snapshots`, a refine session and its export.
#[test]
fn a_merged_mask_finding_beside_a_staged_file_gets_no_mask_recipe() {
    const UNIT: &str = "data.mount";
    let path = etc_path(UNIT);
    let masked = named(UnitHost::new().unit(UNIT, "masked").mask(UNIT), "host-a");
    let written = named(
        UnitHost::new()
            .unit(UNIT, "enabled")
            .etc_unit(UNIT, &mount_file("/data", Some("nofail"))),
        "host-b",
    );
    let merged = merged(masked, written);
    let findings = &merged
        .units
        .as_ref()
        .expect("a units section")
        .mount_findings;
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(
        findings[0].state_change.map(|(state, _)| state),
        Some(UnitState::Masked),
        "the first host's mask"
    );

    let mut session = session(&merged);
    let tree = export_session(&session);
    let containerfile = containerfile_in(&tree);
    assert!(!staged_at(&tree, &path), "the file starts excluded");
    assert_eq!(
        advisory_for(&containerfile, UNIT),
        [
            advisory_fact(UNIT, "masked"),
            MERGED_LINE.to_owned(),
            carry_line("mask", UNIT)
        ],
        "{containerfile}"
    );

    session
        .apply(set_include(config_item(&path), true))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let tree = export_session(&session);
    let containerfile = containerfile_in(&tree);
    assert!(staged_at(&tree, &path), "the file is staged");
    assert_eq!(
        advisory_for(&containerfile, UNIT),
        [
            advisory_fact(UNIT, "masked"),
            MERGED_LINE.to_owned(),
            FILE_STAGED_LINE.to_owned()
        ],
        "{containerfile}"
    );
    assert!(only_comments_name(&containerfile, UNIT), "{containerfile}");
}

/// § 12: two hosts with the same administrator's mount unit merge to one
/// finding counted on both, and the aggregate's Containerfile shows its
/// advisory once, on the scan and the refine route. No instruction names
/// the unit. The advisory says, after its first line, that it describes
/// one host's copy, and the refine view's row carries the same lines. The
/// control, a single-host scan with no such line, is T9.
#[test]
fn an_aggregate_shows_a_shared_mount_advisory_once() {
    const UNIT: &str = "srv.mount";
    let host = |hostname| {
        named(
            UnitHost::new()
                .unit(UNIT, "enabled")
                .etc_unit(UNIT, &mount_file("/srv", Some("nofail"))),
            hostname,
        )
    };
    let merged = merged(host("host-a"), host("host-b"));
    let findings = &merged
        .units
        .as_ref()
        .expect("a units section")
        .mount_findings;
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(
        findings[0].aggregate.as_ref().map(|a| (a.count, a.total)),
        Some((2, 2))
    );
    let expected = [
        advisory_fact(UNIT, "enabled"),
        MERGED_LINE.to_owned(),
        FILE_STAGED_LINE.to_owned(),
        var_symlink_line("/srv"),
    ];
    let refine = session(&merged);
    let rows = &refine.decisions().mount_findings;
    let row = rows
        .iter()
        .find(|r| r.unit.as_str() == UNIT)
        .unwrap_or_else(|| panic!("no row for {UNIT}: {rows:?}"));
    assert_eq!(row.advisories, expected, "the refine view's row");
    for (route, containerfile) in both_routes(&merged) {
        assert_eq!(
            advisory_for(&containerfile, UNIT),
            expected,
            "{route}:\n{containerfile}"
        );
        let first = format!("# Advisory: {UNIT} is ");
        assert_eq!(
            containerfile
                .lines()
                .filter(|l| l.starts_with(&first))
                .count(),
            1,
            "{route}:\n{containerfile}"
        );
        assert!(
            only_comments_name(&containerfile, UNIT),
            "{route}:\n{containerfile}"
        );
    }
}

/// A known aggregate limit, pinned as current behavior for the project
/// owner, not a requirement: two hosts whose administrator's files for one
/// mount unit differ (`host-a`'s has `nofail`, `host-b`'s has none) merge
/// to one finding, `host-a`'s whole finding (the two tie, and the tie goes
/// to the first host by hostname). The advisory describes that finding
/// whichever config variant is staged, so with `host-b`'s file staged it
/// still has no line warning that the file lacks `nofail`. The limit
/// remains; the advisory now discloses it, with a line after its first
/// saying it describes one host's copy and the copy staged may differ.
/// Control: with the hostnames swapped, the finding is the host without
/// `nofail` and the line is there.
#[test]
fn a_merged_mount_finding_describes_one_hosts_file_whatever_variant_is_staged() {
    const UNIT: &str = "srv.mount";
    let path = etc_path(UNIT);
    let host = |hostname, options| {
        named(
            UnitHost::new()
                .unit(UNIT, "enabled")
                .etc_unit(UNIT, &mount_file("/srv", options)),
            hostname,
        )
    };
    let with_nofail = mount_file("/srv", Some("nofail"));
    let without = mount_file("/srv", None);
    let fleet = merged(host("host-a", Some("nofail")), host("host-b", None));
    let findings = &fleet
        .units
        .as_ref()
        .expect("a units section")
        .mount_findings;
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(
        findings[0].nofail,
        Some(true),
        "host-a's finding: {findings:?}"
    );
    let variants: Vec<String> = fleet
        .config
        .as_ref()
        .expect("a config section")
        .files
        .iter()
        .filter(|f| f.path == path)
        .map(|f| f.content.clone())
        .collect();
    assert_eq!(variants.len(), 2, "one config variant per host");

    for (selected, has_nofail) in [(&with_nofail, true), (&without, false)] {
        let mut snap = fleet.clone();
        for file in snap
            .config
            .as_mut()
            .expect("a config section")
            .files
            .iter_mut()
            .filter(|f| f.path == path)
        {
            file.disposition = FindingKind::from_bool(file.content == *selected);
        }
        let tree = render_export(&snap);
        let step = format!("variant with nofail {has_nofail}");
        assert_eq!(
            std::fs::read_to_string(tree.path().join("config").join(&path[1..])).ok(),
            Some(selected.clone()),
            "{step}: the selected variant is staged"
        );
        let containerfile = containerfile_in(&tree);
        assert_eq!(
            advisory_for(&containerfile, UNIT),
            [
                advisory_fact(UNIT, "enabled"),
                MERGED_LINE.to_owned(),
                FILE_STAGED_LINE.to_owned(),
                var_symlink_line("/srv"),
            ],
            "{step}: no nofail line, whichever file is staged:\n{containerfile}"
        );
        assert!(
            !containerfile.contains(NOFAIL_LINE),
            "{step}:\n{containerfile}"
        );
    }

    // Control: the same two files with the hostnames swapped. The finding
    // is again the first host's, now the one without `nofail`, and the
    // advisory has the line.
    let swapped = merged(host("host-a", None), host("host-b", Some("nofail")));
    let containerfile = containerfile_in(&render_export(&swapped));
    assert!(
        advisory_for(&containerfile, UNIT).contains(&NOFAIL_LINE.to_owned()),
        "{containerfile}"
    );
}

// ---------------------------------------------------------------------
// § 11.1: the refine view shows what the export does with each row.
// ---------------------------------------------------------------------

/// The `declined` of `unit`'s row in `session`'s view.
fn declined_in(
    session: &RefineSession,
    unit: &str,
) -> Option<inspectah_refine::projection::DeclinedView> {
    session
        .decisions()
        .unit_states
        .iter()
        .find(|r| r.unit.as_str() == unit)
        .unwrap_or_else(|| panic!("no unit row for {unit}"))
        .declined
        .clone()
}

/// `InstallEvidenceChanged` for `unit`, naming `path`, as the view holds it.
fn evidence_declined(unit: &str, path: &str) -> inspectah_refine::projection::DeclinedView {
    inspectah_refine::projection::DeclinedView {
        renders: inspectah_refine::projection::DeclinedRenders::Omitted,
        reason: "InstallEvidenceChanged".into(),
        text: format!(
            "{unit} (the [Install] section in the image would not be the one recorded on the \
             source host ({path}))"
        ),
    }
}

/// T10, view half: through the scan and a refine session, the mount
/// finding's row exists with its fields, and the planted token is in
/// neither the view nor the decision projection, serialized whole.
#[test]
fn t10_the_mount_source_is_in_no_view_response() {
    let refine = session(&scan(t10_host()));
    let rows = &refine.decisions().mount_findings;
    let row = rows
        .iter()
        .find(|r| r.unit.as_str() == MOUNT_UNIT)
        .unwrap_or_else(|| panic!("no row for {MOUNT_UNIT}: {rows:?}"));
    assert_eq!(row.mount_point.as_deref(), Some("/srv/data"));
    assert_eq!(row.unit_file_path.as_deref(), Some(MOUNT_UNIT_PATH));
    assert!(!row.advisories.is_empty());
    let view = serde_json::to_string(refine.view()).expect("the view serializes");
    let decisions = serde_json::to_string(refine.decisions()).expect("the projection serializes");
    assert!(decisions.contains(MOUNT_UNIT), "presence first");
    assert!(!view.contains(MOUNT_TOKEN));
    assert!(!decisions.contains(MOUNT_TOKEN));
}

/// T15, last clause, the `Shadowed` case: excluding the shadow through
/// `RefineSession` alone puts `InstallEvidenceChanged`, naming the shadow,
/// on the unit row, while the Containerfile omits the command; including
/// it again clears both.
#[test]
fn t15_a_shadowed_row_shows_the_excluded_shadow() {
    let snap = scan(shadow_host("enabled", "disable"));
    let mut refine = session(&snap);
    assert_eq!(declined_in(&refine, "shadow.service"), None, "at scan");
    let shadow = ItemId::DropIn {
        path: SHADOW_UNIT.into(),
    };
    for (include, expected) in [
        (
            false,
            Some(evidence_declined("shadow.service", SHADOW_UNIT)),
        ),
        (true, None),
    ] {
        refine
            .apply(set_include(shadow.clone(), include))
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            declined_in(&refine, "shadow.service"),
            expected,
            "shadow included {include}"
        );
        let containerfile = containerfile_in(&export_session(&refine));
        assert_eq!(
            acts(&containerfile, "enable", "shadow.service"),
            include,
            "shadow included {include}:\n{containerfile}"
        );
    }
}

/// T15, last clause, the `Vendor` case: another row's unit file staged at
/// the vendor unit's name (an imported config entry) puts
/// `InstallEvidenceChanged`, naming that file, on the unit row; excluding
/// the config row through `RefineSession` alone clears it and the enable
/// returns.
#[test]
fn t15_a_vendor_row_shows_another_rows_unit_file() {
    const UNIT: &str = "orphan.service";
    let snap = with_config_entry(
        &scan(t5_host()),
        ORPHAN_UNIT,
        "[Service]\nExecStart=/usr/bin/other\n",
    );
    let mut refine = session(&snap);
    assert_eq!(
        declined_in(&refine, UNIT),
        Some(evidence_declined(UNIT, ORPHAN_UNIT))
    );
    assert!(in_no_systemctl_instruction(
        &containerfile_in(&export_session(&refine)),
        UNIT
    ));
    refine
        .apply(set_include(config_item(ORPHAN_UNIT), false))
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(declined_in(&refine, UNIT), None);
    let containerfile = containerfile_in(&export_session(&refine));
    assert!(acts(&containerfile, "enable", UNIT), "{containerfile}");
}

/// On an aggregate, the view shows what the merged export renders: T18
/// (c)'s pair of hosts, the mask row declined with `MaskPathOccupied`
/// while the other host's unit file is included. Once it is excluded the
/// mask follows its package (§ 10.2 rule 5): `kexec-tools` is on one host
/// of two, so the merge excludes it and the row shows `PackageExcluded`;
/// with the package included the row is not declined.
#[test]
fn an_aggregate_row_shows_the_merged_exports_reason() {
    const UNIT: &str = "kdump.service";
    let path = etc_path(UNIT);
    let merged = merged(
        named(
            owned_unit_host(UNIT, "kexec-tools", "masked", VENDOR_SERVICE),
            "host-a",
        ),
        named(
            UnitHost::new()
                .unit(UNIT, "enabled")
                .preset("disable *")
                .etc_unit(
                    UNIT,
                    "[Service]\nExecStart=/usr/local/bin/kdump\n[Install]\nWantedBy=multi-user.target\n",
                ),
            "host-b",
        ),
    );
    let mut refine = session(&merged);
    refine
        .apply(set_include(config_item(&path), true))
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        declined_in(&refine, UNIT),
        Some(inspectah_refine::projection::DeclinedView {
            renders: inspectah_refine::projection::DeclinedRenders::Omitted,
            reason: "MaskPathOccupied".into(),
            text: format!(
                "{UNIT} (the export also carries a unit file at {path}; exclude one of the two)"
            ),
        })
    );
    refine
        .apply(set_include(config_item(&path), false))
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        declined_in(&refine, UNIT).map(|d| d.reason),
        Some("PackageExcluded".to_owned())
    );
    refine
        .apply(set_include(package_item("kexec-tools"), true))
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(declined_in(&refine, UNIT), None);
    assert!(acts(
        &containerfile_in(&export_session(&refine)),
        "mask",
        UNIT
    ));
}

// ---------------------------------------------------------------------
// § 8: one control per fact for a local timer (T19).
// ---------------------------------------------------------------------

const REPORT_TIMER: &str = "/etc/systemd/system/report.timer";
const REPORT_SERVICE: &str = "/etc/systemd/system/report.service";
const SCHEDULED_SECTION: &str = "# === Scheduled Tasks ===";

/// A host with the administrator's own `report.timer` in `state` and the
/// `report.service` it starts, both under `/etc/systemd/system` and owned
/// by no package.
fn t19_host(state: &str) -> UnitHost {
    UnitHost::new()
        .unit("report.timer", state)
        .unit("report.service", "static")
        .preset("disable *")
        .etc_unit(
            "report.timer",
            "[Unit]\nDescription=Nightly report\n[Timer]\nOnCalendar=daily\n\
             [Install]\nWantedBy=timers.target\n",
        )
        .etc_unit(
            "report.service",
            "[Service]\nType=oneshot\nExecStart=/usr/local/bin/report\n",
        )
}

/// Every file under `dir` named `name`, in any tree of the export.
fn files_named(dir: &std::path::Path, name: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            for entry in std::fs::read_dir(&path).expect("the export lists") {
                pending.push(entry.expect("an entry").path());
            }
        } else if path.file_name().is_some_and(|n| n == name) {
            found.push(path.display().to_string());
        }
    }
    found
}

/// Whether any instruction of `containerfile`, comments aside, names
/// `needle`: a `systemctl` line, a `COPY` of the one file or a heredoc
/// that writes it.
fn named_by_an_instruction(containerfile: &str, needle: &str) -> bool {
    containerfile
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .any(|l| l.contains(needle))
}

/// The scheduled tasks section's lines, without its header.
fn scheduled_lines(containerfile: &str) -> Vec<String> {
    containerfile
        .lines()
        .skip_while(|l| *l != SCHEDULED_SECTION)
        .skip(1)
        .take_while(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// T19: a local timer, `disabled`, and its paired service. Presence first:
/// the scheduled section describes the timer, each file is an included
/// config entry, and the timer is in the inventory with no state change.
/// No instruction names the timer. Excluding the timer's config row
/// removes the timer file and leaves the service file, whose row is still
/// included; excluding that row too leaves neither. Each file is found in
/// no tree of the export once its row is excluded, so no other writer
/// restores it.
#[test]
fn t19_a_disabled_local_timer_stays_disabled() {
    let snap = scan(t19_host("disabled"));
    let timers = &snap
        .scheduled_tasks
        .as_ref()
        .expect("a scheduled section")
        .systemd_timers;
    assert!(
        timers
            .iter()
            .any(|t| t.name == "report" && t.source == "local"),
        "{timers:?}"
    );
    let config = &snap.config.as_ref().expect("a config section").files;
    for path in [REPORT_TIMER, REPORT_SERVICE] {
        assert!(
            config
                .iter()
                .any(|f| f.path == path && f.disposition.is_included()),
            "{path} is an included config entry"
        );
    }
    let units = snap.units.as_ref().expect("a units section");
    assert!(
        units
            .inventory
            .iter()
            .any(|e| e.unit.as_str() == "report.timer" && e.state == UnitState::Disabled),
        "{:?}",
        units.inventory
    );
    assert!(
        units
            .state_changes
            .iter()
            .all(|c| c.unit().name().as_str() != "report.timer"),
        "{:?}",
        units.state_changes
    );

    for (label, ops, timer_staged, service_staged) in [
        ("no toggle", vec![], true, true),
        (
            "timer row excluded",
            vec![(config_item(REPORT_TIMER), false)],
            false,
            true,
        ),
        (
            "both rows excluded",
            vec![
                (config_item(REPORT_TIMER), false),
                (config_item(REPORT_SERVICE), false),
            ],
            false,
            false,
        ),
    ] {
        let tree = exported_after(&snap, &ops);
        let containerfile = containerfile_in(&tree);
        assert!(
            in_no_systemctl_instruction(&containerfile, "report.timer"),
            "{label}:\n{containerfile}"
        );
        assert!(
            scheduled_lines(&containerfile)
                .iter()
                .all(|l| !l.contains("systemctl")),
            "{label}: the scheduled section writes no systemctl line:\n{containerfile}"
        );
        for (name, staged) in [
            ("report.timer", timer_staged),
            ("report.service", service_staged),
        ] {
            let found = files_named(tree.path(), name);
            if staged {
                assert_eq!(found.len(), 1, "{label}, {name}: {found:?}");
                assert!(
                    found[0].contains("/config/etc/systemd/system/"),
                    "{found:?}"
                );
            } else {
                assert!(found.is_empty(), "{label}, {name}: {found:?}");
                assert!(
                    !named_by_an_instruction(&containerfile, name),
                    "{label}, {name}:\n{containerfile}"
                );
            }
        }
    }
}

/// T19's positive control: the same timer `enabled` is a state change, and
/// its enable comes from the unit state section, not the scheduled one.
/// With its file excluded, the enable is omitted with the stated reason
/// (§ 8, the single-control rule) and nothing else enables it.
#[test]
fn t19_an_enabled_local_timer_is_enabled_from_the_unit_state_section() {
    let snap = scan(t19_host("enabled"));
    assert!(
        matches!(
            fragment_of(&snap, "report.timer"),
            UnitFragment::Local { .. }
        ),
        "{:?}",
        fragment_of(&snap, "report.timer")
    );
    let containerfile = containerfile_in(&render_export(&snap));
    assert!(
        acts(&containerfile, "enable", "report.timer"),
        "{containerfile}"
    );
    assert!(
        unit_lines(&containerfile)
            .iter()
            .any(|l| l.contains("'report.timer'")),
        "{containerfile}"
    );
    assert!(
        scheduled_lines(&containerfile)
            .iter()
            .all(|l| !l.contains("systemctl")),
        "{containerfile}"
    );

    // The refine route with no operation carries the enable: the control
    // for the omission below, on the same route.
    let unchanged = containerfile_in(&exported_after(&snap, &[]));
    assert!(acts(&unchanged, "enable", "report.timer"), "{unchanged}");
    assert!(omitted(&unchanged, "report.timer").is_none(), "{unchanged}");

    let excluded = containerfile_in(&exported_after(
        &snap,
        &[(config_item(REPORT_TIMER), false)],
    ));
    assert!(
        in_no_systemctl_instruction(&excluded, "report.timer"),
        "{excluded}"
    );
    assert!(
        omitted(&excluded, "report.timer").is_some_and(|l| l.ends_with(NOT_STAGED)),
        "{excluded}"
    );
}

/// A vendor timer with a full copy under `/etc`, in `state` against the
/// opposite preset.
fn shadowed_timer_host(state: &str, preset: &str) -> UnitHost {
    UnitHost::new()
        .unit("site.timer", state)
        .preset(&format!("{preset} site.timer"))
        .vendor_unit(
            "site.timer",
            "[Timer]\nOnCalendar=daily\n[Install]\nWantedBy=timers.target\n",
            Some("sitepkg"),
        )
        .etc_unit(
            "site.timer",
            "[Timer]\nOnCalendar=hourly\n[Install]\nWantedBy=timers.target\n",
        )
}

/// A timer's full shadow is a drop-in row like any ordinary unit's (§ 7.7),
/// so excluding it declines the timer's command, `disable` as well as
/// `enable`, where before the scheduled section staged the file whatever
/// the row said. Presence first: the scan carries the command, the shadow
/// row exists and the config entry at its path is inventory. Restoring
/// the row restores the command.
#[test]
fn a_shadowed_timer_whose_shadow_is_excluded_gets_no_command() {
    let path = etc_path("site.timer");
    for (state, preset, verb) in [
        ("enabled", "disable", "enable"),
        ("disabled", "enable", "disable"),
    ] {
        let snap = scan(shadowed_timer_host(state, preset));
        assert!(
            matches!(
                fragment_of(&snap, "site.timer"),
                UnitFragment::Shadowed { .. }
            ),
            "{verb}: {:?}",
            fragment_of(&snap, "site.timer")
        );
        let units = snap.units.as_ref().expect("a units section");
        assert!(
            units.drop_ins.iter().any(|d| d.path == path),
            "{verb}: {:?}",
            units.drop_ins
        );
        let entry = snap
            .config
            .as_ref()
            .and_then(|c| c.files.iter().find(|f| f.path == path))
            .expect("a config entry at the shadow's path");
        assert!(entry.disposition.is_inventory(), "{verb}: {entry:?}");
        let scanned = containerfile_in(&render_export(&snap));
        assert!(acts(&scanned, verb, "site.timer"), "{verb}:\n{scanned}");

        let shadow = ItemId::DropIn { path: path.clone() };
        let excluded = exported_after(&snap, &[(shadow.clone(), false)]);
        let containerfile = containerfile_in(&excluded);
        assert!(
            in_no_systemctl_instruction(&containerfile, "site.timer"),
            "{verb}:\n{containerfile}"
        );
        assert!(
            omitted(&containerfile, "site.timer")
                .is_some_and(|l| l.ends_with(&evidence_changed(&path))),
            "{verb}:\n{containerfile}"
        );
        assert!(
            files_named(excluded.path(), "site.timer").is_empty(),
            "{verb}: no other writer stages the file"
        );

        let restored = containerfile_in(&exported_after(
            &snap,
            &[(shadow.clone(), false), (shadow, true)],
        ));
        assert!(acts(&restored, verb, "site.timer"), "{verb}:\n{restored}");
    }
}

/// § 10.4 holds for a runtime updater timer under `/etc`, local or a full
/// shadow of the vendor one: the row is excluded by default with the
/// stated reason, and no instruction names it, where before the scheduled
/// section enabled every local timer itself. Including the row yields the
/// enable (the control).
#[test]
fn a_local_or_shadowed_updater_timer_keeps_the_updater_default() {
    const UPDATER: &str = "dnf-automatic.timer";
    const BODY: &str = "[Timer]\nOnCalendar=daily\n[Install]\nWantedBy=timers.target\n";
    let local = UnitHost::new()
        .unit(UPDATER, "enabled")
        .preset("disable *")
        .etc_unit(UPDATER, BODY);
    let shadowed = UnitHost::new()
        .unit(UPDATER, "enabled")
        .preset("disable *")
        .vendor_unit(UPDATER, BODY, Some("dnf-automatic"))
        .etc_unit(
            UPDATER,
            "[Timer]\nOnCalendar=hourly\n[Install]\nWantedBy=timers.target\n",
        );
    for (label, host) in [("local", local), ("shadowed", shadowed)] {
        let snap = scan(host);
        let row = change(&snap, UPDATER);
        assert!(!row.disposition.is_included(), "{label}: {row:?}");
        let containerfile = containerfile_in(&render_export(&snap));
        assert_eq!(
            omitted(&containerfile, UPDATER),
            Some(updater_omitted(UPDATER).as_str()),
            "{label}:\n{containerfile}"
        );
        assert!(
            in_no_systemctl_instruction(&containerfile, UPDATER),
            "{label}:\n{containerfile}"
        );

        let included = containerfile_in(&exported_after(&snap, &[(unit_item(UPDATER), true)]));
        assert!(acts(&included, "enable", UPDATER), "{label}:\n{included}");
    }
}

/// The scheduled row of `name`'s timer in `session`'s reference view.
fn scheduled_timer_row(
    session: &RefineSession,
    name: &str,
) -> inspectah_refine::projection::GenericRefItem {
    session
        .reference()
        .scheduled_tasks
        .into_iter()
        .find(|row| row.id == name && row.tags.iter().any(|t| t == "timer"))
        .unwrap_or_else(|| panic!("no scheduled timer row for {name}"))
}

/// § 11.1 and T20, refine half: a scheduled timer row names its unit and
/// says whether `unit_states` holds a row for it, where its control is.
/// An enabled local timer is a state change, so its row links to a unit
/// row that keeps its include flag and accepts a `SetInclude`; the same
/// timer `disabled` is no state change, so its row says there is no unit
/// row to link to.
#[test]
fn a_scheduled_timer_row_links_to_its_unit_row() {
    use inspectah_refine::projection::TimerUnitLink;
    let unit: inspectah_core::types::units::UnitName =
        "report.timer".to_owned().try_into().expect("a unit name");

    let mut enabled = session(&scan(t19_host("enabled")));
    assert_eq!(
        scheduled_timer_row(&enabled, "report").unit_link,
        Some(TimerUnitLink {
            unit: unit.clone(),
            unit_row: true
        })
    );
    let include = |s: &RefineSession| {
        s.decisions()
            .unit_states
            .iter()
            .find(|row| row.unit == unit)
            .map(|row| row.include)
    };
    let before = include(&enabled).expect("the timer's unit_states row");
    enabled
        .apply(set_include(unit_item("report.timer"), !before))
        .expect("the unit row's toggle applies");
    assert_eq!(
        include(&enabled),
        Some(!before),
        "the unit row keeps its flag"
    );

    let disabled = session(&scan(t19_host("disabled")));
    assert_eq!(include(&disabled), None, "no unit_states row");
    assert_eq!(
        scheduled_timer_row(&disabled, "report").unit_link,
        Some(TimerUnitLink {
            unit,
            unit_row: false
        })
    );
}

// ---------------------------------------------------------------------
// § 10.2 "What the plan takes" item 3: generated units are plan requests
// (T21, generated and flatpak cases).
// ---------------------------------------------------------------------

/// The cron file the scheduled inspector converts to `cron-backup.timer`
/// and `cron-backup.service`.
const CRON_FILE: &str = "/etc/cron.d/backup";
const CRON_LINE: &str = "0 2 * * * root /usr/local/bin/backup\n";
const CRON_BACKUP_TIMER: &str = "cron-backup.timer";
/// The host's own `cron-backup.timer` body, a local or vendor file.
const HOST_CRON_BACKUP_TIMER: &str =
    "[Timer]\nOnCalendar=weekly\n[Install]\nWantedBy=timers.target\n";

/// The FIXME the export writes for a generated request declined because a
/// collected unit with a row has its name, by its prefix and unit.
fn collision_fixme<'a>(containerfile: &'a str, unit: &str) -> Option<&'a str> {
    let prefix = format!("# FIXME: {unit} (");
    containerfile
        .lines()
        .find(|l| l.starts_with(&prefix) && l.contains("a unit collected from the host, "))
}

/// A host with the cron file and, from `host`, whatever else.
fn with_cron_backup(host: UnitHost) -> UnitHost {
    host.file(CRON_FILE, CRON_LINE)
}

/// The scan export and the refine export of `snap`, each with its label.
fn both_exports(snap: &InspectionSnapshot) -> [(&'static str, tempfile::TempDir); 2] {
    [
        ("scan", render_export(snap)),
        ("refine", export_session(&session(snap))),
    ]
}

/// T21: a generated cron timer and service named for a collected, enabled
/// vendor timer. Through the scan export and the refine export the
/// generated request is declined with a FIXME, neither generated file is
/// in the staged tree, and the vendor timer's enable is carried in the
/// unit state section against the package's unit. Control: the same cron
/// file on a host with no unit of that name stages the generated pair and
/// enables the timer from the scheduled section.
#[test]
fn t21_a_generated_timer_named_for_a_vendor_timer_is_declined() {
    let host = with_cron_backup(
        UnitHost::new()
            .base_package("glibc")
            .package("backuppkg")
            .unit(CRON_BACKUP_TIMER, "enabled")
            .preset("disable *")
            .vendor_unit(CRON_BACKUP_TIMER, HOST_CRON_BACKUP_TIMER, Some("backuppkg")),
    );
    let snap = scan(host);
    assert!(
        matches!(
            fragment_of(&snap, CRON_BACKUP_TIMER),
            UnitFragment::Vendor { .. }
        ),
        "presence: the vendor timer's state change"
    );
    assert!(
        snap.scheduled_tasks.as_ref().is_some_and(|st| st
            .generated_timer_units
            .iter()
            .any(|u| u.name == "cron-backup")),
        "presence: the generated unit"
    );
    for (route, tree) in both_exports(&snap) {
        let cf = containerfile_in(&tree);
        assert!(
            collision_fixme(&cf, CRON_BACKUP_TIMER).is_some(),
            "{route}:\n{cf}"
        );
        for name in [CRON_BACKUP_TIMER, "cron-backup.service"] {
            assert!(
                files_named(tree.path(), name).is_empty(),
                "{route}: a generated {name} is staged"
            );
        }
        assert!(acts(&cf, "enable", CRON_BACKUP_TIMER), "{route}:\n{cf}");
        assert!(
            unit_lines(&cf)
                .iter()
                .any(|l| l.contains("'cron-backup.timer'")),
            "{route}: the enable is the unit state section's:\n{cf}"
        );
        assert!(
            scheduled_lines(&cf)
                .iter()
                .all(|l| !l.contains("systemctl")),
            "{route}:\n{cf}"
        );
    }

    let control = scan(with_cron_backup(UnitHost::new().preset("disable *")));
    for (route, tree) in both_exports(&control) {
        let cf = containerfile_in(&tree);
        assert!(
            collision_fixme(&cf, CRON_BACKUP_TIMER).is_none(),
            "{route}:\n{cf}"
        );
        for name in [CRON_BACKUP_TIMER, "cron-backup.service"] {
            assert_eq!(files_named(tree.path(), name).len(), 1, "{route}: {name}");
        }
        assert!(
            scheduled_lines(&cf)
                .iter()
                .any(|l| l.contains("'cron-backup.timer'")),
            "{route}: the scheduled section enables the generated timer:\n{cf}"
        );
        assert!(acts(&cf, "enable", CRON_BACKUP_TIMER), "{route}:\n{cf}");
    }
}

/// A local `cron-backup.timer`, `disabled` on the host, beside a generated
/// `cron-backup`: the local timer's own row decides. Through both routes no
/// instruction names it, the generated request is declined with the
/// collision FIXME, and the timer file staged is the host's. Control: the
/// same local timer `enabled` is enabled from the unit state section, with
/// the generated request declined all the same.
#[test]
fn a_disabled_local_timer_named_like_a_generated_one_stays_disabled() {
    let host = |state: &str| {
        with_cron_backup(
            UnitHost::new()
                .unit(CRON_BACKUP_TIMER, state)
                .preset("disable *")
                .etc_unit(CRON_BACKUP_TIMER, HOST_CRON_BACKUP_TIMER),
        )
    };
    let disabled = scan(host("disabled"));
    for (route, tree) in both_exports(&disabled) {
        let cf = containerfile_in(&tree);
        assert!(
            in_no_systemctl_instruction(&cf, CRON_BACKUP_TIMER),
            "{route}:\n{cf}"
        );
        assert!(
            collision_fixme(&cf, CRON_BACKUP_TIMER).is_some(),
            "{route}:\n{cf}"
        );
        let timers = files_named(tree.path(), CRON_BACKUP_TIMER);
        assert_eq!(timers.len(), 1, "{route}: {timers:?}");
        assert_eq!(
            std::fs::read_to_string(&timers[0]).unwrap(),
            HOST_CRON_BACKUP_TIMER,
            "{route}: the host's timer is staged"
        );
        assert!(
            files_named(tree.path(), "cron-backup.service").is_empty(),
            "{route}: no generated service"
        );
    }

    let enabled = scan(host("enabled"));
    for (route, tree) in both_exports(&enabled) {
        let cf = containerfile_in(&tree);
        assert!(
            unit_lines(&cf)
                .iter()
                .any(|l| l.contains("'cron-backup.timer'")),
            "{route}:\n{cf}"
        );
        assert!(
            collision_fixme(&cf, CRON_BACKUP_TIMER).is_some(),
            "{route}:\n{cf}"
        );
    }
}

const FLATPAK_PROVISION: &str = "flatpak-provision.service";
const FLATPAK_PROVISION_COPY: &str =
    "COPY flatpak/flatpak-provision.service /etc/systemd/system/flatpak-provision.service";

/// `snap` with one included flatpak application.
fn with_flatpak_app(mut snap: InspectionSnapshot) -> InspectionSnapshot {
    use inspectah_core::types::containers::{ContainerSection, FlatpakApp};
    snap.containers = Some(ContainerSection {
        flatpak_apps: vec![FlatpakApp {
            app_id: "org.example.App".into(),
            remote: "flathub".into(),
            remote_url: "https://dl.flathub.org/repo/".into(),
            branch: "stable".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    snap
}

/// The index of the first line of `containerfile` equal to `line`.
fn line_index(containerfile: &str, line: &str) -> Option<usize> {
    containerfile.lines().position(|l| l == line)
}

/// T21, flatpak: the provisioning service's enable is quoted, after `--`,
/// and follows its own `COPY`, through both routes.
#[test]
fn t21_the_flatpak_enable_is_quoted_and_follows_its_copy() {
    let snap = with_flatpak_app(scan(UnitHost::new()));
    for (route, tree) in both_exports(&snap) {
        let cf = containerfile_in(&tree);
        let copy = line_index(&cf, FLATPAK_PROVISION_COPY)
            .unwrap_or_else(|| panic!("{route}: no COPY:\n{cf}"));
        let enable = line_index(&cf, "RUN systemctl enable -- \\")
            .filter(|i| cf.lines().nth(i + 1) == Some("    'flatpak-provision.service'"))
            .unwrap_or_else(|| panic!("{route}: no quoted enable:\n{cf}"));
        assert!(enable > copy, "{route}: the enable follows its COPY:\n{cf}");
        assert!(acts(&cf, "enable", FLATPAK_PROVISION), "{route}:\n{cf}");
    }
}

/// § 10.2's flatpak rescan case: a host built from an inspectah image has a
/// local, enabled `flatpak-provision.service`. Its rescan carries the
/// collected copy, enabled from the unit state section, and withholds the
/// generated one with the collision FIXME and no COPY of it. With the
/// collected row and its file excluded in refine, the generated service
/// is copied and enabled again (the control).
#[test]
fn a_rescanned_flatpak_provisioning_service_keeps_the_collected_copy() {
    const COLLECTED: &str =
        "[Service]\nType=oneshot\nExecStart=/usr/bin/true\n[Install]\nWantedBy=multi-user.target\n";
    let snap = with_flatpak_app(scan(
        UnitHost::new()
            .unit(FLATPAK_PROVISION, "enabled")
            .preset("disable *")
            .etc_unit(FLATPAK_PROVISION, COLLECTED),
    ));
    assert!(
        matches!(
            fragment_of(&snap, FLATPAK_PROVISION),
            UnitFragment::Local { .. }
        ),
        "presence: the collected row"
    );
    for (route, tree) in both_exports(&snap) {
        let cf = containerfile_in(&tree);
        assert!(
            collision_fixme(&cf, FLATPAK_PROVISION).is_some(),
            "{route}:\n{cf}"
        );
        assert!(!cf.contains(FLATPAK_PROVISION_COPY), "{route}:\n{cf}");
        assert!(
            !tree.path().join("flatpak").join(FLATPAK_PROVISION).exists(),
            "{route}: the generated service is staged"
        );
        let collected = tree
            .path()
            .join("config/etc/systemd/system")
            .join(FLATPAK_PROVISION);
        assert_eq!(
            std::fs::read_to_string(&collected).unwrap(),
            COLLECTED,
            "{route}"
        );
        assert!(
            unit_lines(&cf)
                .iter()
                .any(|l| l.contains("'flatpak-provision.service'")),
            "{route}:\n{cf}"
        );
    }

    let path = etc_path(FLATPAK_PROVISION);
    let excluded = containerfile_in(&exported_after(
        &snap,
        &[
            (unit_item(FLATPAK_PROVISION), false),
            (config_item(&path), false),
        ],
    ));
    assert!(
        collision_fixme(&excluded, FLATPAK_PROVISION).is_none(),
        "{excluded}"
    );
    assert!(excluded.contains(FLATPAK_PROVISION_COPY), "{excluded}");
    assert!(acts(&excluded, "enable", FLATPAK_PROVISION), "{excluded}");
}

/// § 11.1: a declined generated request shows on the row that asked for
/// it. Through the refine session, the declined `cron-backup.timer`
/// request names its scheduled row with the plan's FIXME; with no unit of
/// that name, nothing is declined (the control).
#[test]
fn a_declined_generated_request_shows_on_its_scheduled_row() {
    use inspectah_refine::projection::{DeclinedRenders, GeneratedRowView};
    let snap = scan(with_cron_backup(
        UnitHost::new()
            .unit(CRON_BACKUP_TIMER, "disabled")
            .preset("disable *")
            .etc_unit(CRON_BACKUP_TIMER, HOST_CRON_BACKUP_TIMER),
    ));
    let refine = session(&snap);
    let views = &refine.decisions().declined_generated;
    assert_eq!(views.len(), 1, "{views:?}");
    assert_eq!(
        views[0].row,
        GeneratedRowView::Scheduled {
            name: "cron-backup".into()
        }
    );
    assert_eq!(views[0].unit.as_str(), CRON_BACKUP_TIMER);
    assert_eq!(views[0].declined.renders, DeclinedRenders::Fixme);
    assert_eq!(views[0].declined.reason, "GeneratedNameCollision");
    let cf = containerfile_in(&export_session(&refine));
    assert_eq!(
        collision_fixme(&cf, CRON_BACKUP_TIMER),
        Some(format!("# FIXME: {}", views[0].declined.text).as_str())
    );

    let control = session(&scan(with_cron_backup(UnitHost::new().preset("disable *"))));
    assert!(control.decisions().declined_generated.is_empty());
}

// ---------------------------------------------------------------------
// § 10.2 the tuned decision, § 8 the single-control rule: every
// `systemctl` line is planned (T21, tuned case and full scan).
// ---------------------------------------------------------------------

const TUNED: &str = "tuned.service";
const TUNED_PROFILE: &str = "throughput-performance";
const KERNEL_SECTION: &str = "# === Kernel and Boot Configuration ===";
const TUNED_VENDOR_SERVICE: &str =
    "[Service]\nExecStart=/usr/sbin/tuned -l -P\n[Install]\nWantedBy=multi-user.target\n";

/// `tuned.service` in `state` against the opposite preset, its vendor file
/// owned by the `tuned` package, on a host scanned against a base image so
/// refine keeps the package included as the scan does.
fn tuned_host(state: &str) -> UnitHost {
    let preset = if state == "enabled" {
        "disable"
    } else {
        "enable"
    };
    let host = UnitHost::new()
        .base_package("glibc")
        .package("tuned")
        .unit(TUNED, state)
        .preset(&format!("{preset} {TUNED}"))
        .vendor_unit(TUNED, TUNED_VENDOR_SERVICE, Some("tuned"));
    if state == "masked" {
        host.mask(TUNED)
    } else {
        host
    }
}

/// `snap` with an included tuned activation of a stock profile. The test
/// scan runs no kernel and boot inspector, so the section is added here.
fn with_tuned_activation(mut snap: InspectionSnapshot) -> InspectionSnapshot {
    snap.kernel_boot = Some(inspectah_core::types::kernelboot::KernelBootSection {
        tuned_active: TUNED_PROFILE.into(),
        tuned_disposition: FindingKind::included(),
        ..Default::default()
    });
    snap
}

/// The kernel and boot section's lines, without its header.
fn kernel_lines(containerfile: &str) -> Vec<String> {
    containerfile
        .lines()
        .skip_while(|l| *l != KERNEL_SECTION)
        .skip(1)
        .take_while(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The kernel and boot section writes the active profile and no
/// `systemctl` line.
fn assert_kernel_section_writes_the_profile_only(containerfile: &str, label: &str) {
    let kernel = kernel_lines(containerfile);
    assert!(
        kernel.contains(&format!(
            "RUN echo \"{TUNED_PROFILE}\" > /etc/tuned/active_profile"
        )),
        "{label}: the profile is written:\n{containerfile}"
    );
    assert!(
        kernel.iter().all(|l| !l.contains("systemctl")),
        "{label}: the kernel section writes a systemctl line:\n{containerfile}"
    );
}

/// T21, tuned: with tuned activation included, `tuned.service` enabled
/// against a `disable` preset is enabled from the unit state section and
/// nowhere else, and the kernel section writes the profile and no
/// `systemctl` line. With the `tuned` package excluded in refine, the
/// enable is omitted for the package and no instruction names the unit.
#[test]
fn t21_tuned_service_is_enabled_from_its_unit_row_only() {
    let snap = with_tuned_activation(scan(tuned_host("enabled")));
    for (route, tree) in both_exports(&snap) {
        let cf = containerfile_in(&tree);
        assert_kernel_section_writes_the_profile_only(&cf, route);
        assert!(acts(&cf, "enable", TUNED), "{route}:\n{cf}");
        let instructions: Vec<String> = systemctl_instructions(&cf)
            .into_iter()
            .filter(|i| i.contains(TUNED))
            .collect();
        assert_eq!(instructions.len(), 1, "{route}: {instructions:?}");
        assert!(
            unit_lines(&cf)
                .iter()
                .any(|l| l.contains("'tuned.service'")),
            "{route}: the enable is the unit state section's:\n{cf}"
        );
    }

    let excluded = containerfile_in(&exported_after(
        &snap,
        &[(
            ItemId::Package {
                name: "tuned".into(),
                arch: "x86_64".into(),
            },
            false,
        )],
    ));
    assert_eq!(
        omitted(&excluded, TUNED),
        Some("# Omitted: tuned.service (package 'tuned' was excluded)"),
        "{excluded}"
    );
    assert!(in_no_systemctl_instruction(&excluded, TUNED), "{excluded}");
    assert_kernel_section_writes_the_profile_only(&excluded, "excluded");
}

/// The phase 2 defect this closes: the kernel section enabled
/// `tuned.service` after the unit state section whatever the unit's row
/// said, so a carried `disable` was undone and a carried `mask` failed the
/// build. With tuned activation included, a disabled or masked
/// `tuned.service` gets its own command and no `enable` anywhere, through
/// both routes. Control: the same unit enabled gets its enable.
#[test]
fn a_carried_disable_or_mask_of_tuned_is_not_undone() {
    for (state, verb) in [("disabled", "disable"), ("masked", "mask")] {
        let snap = with_tuned_activation(scan(tuned_host(state)));
        for (route, tree) in both_exports(&snap) {
            let cf = containerfile_in(&tree);
            assert!(acts(&cf, verb, TUNED), "{state}, {route}:\n{cf}");
            assert!(!acts(&cf, "enable", TUNED), "{state}, {route}:\n{cf}");
            assert_kernel_section_writes_the_profile_only(&cf, route);
        }
    }
    let control = with_tuned_activation(scan(tuned_host("enabled")));
    for (route, tree) in both_exports(&control) {
        let cf = containerfile_in(&tree);
        assert!(acts(&cf, "enable", TUNED), "control, {route}:\n{cf}");
    }
}

/// Every `systemctl` instruction of `containerfile` as a person reads it:
/// comment lines skipped, `\` continuations joined, and each operand of
/// each instruction paired with its verb.
fn systemctl_commands(containerfile: &str) -> Vec<(String, String)> {
    systemctl_instructions(containerfile)
        .iter()
        .flat_map(|instruction| {
            let (head, operands) = instruction
                .split_once(" -- ")
                .unwrap_or_else(|| panic!("no `--` in {instruction:?}"));
            let verb = head
                .strip_prefix("RUN systemctl ")
                .unwrap_or_else(|| panic!("not a RUN systemctl instruction: {instruction:?}"))
                .to_owned();
            operands
                .split_whitespace()
                .map(|operand| {
                    let unit = operand
                        .strip_prefix('\'')
                        .and_then(|o| o.strip_suffix('\''))
                        .unwrap_or_else(|| panic!("an unquoted operand: {operand:?}"));
                    (verb.clone(), unit.to_owned())
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// T21, the full scan: a snapshot with flatpak applications, tuned
/// activation, a generated timer and collected state changes. Every
/// `systemctl` command in the rendered Containerfile, read from its text,
/// is a `PlannedUnit` of the export's plan, and every planned unit is
/// rendered, through the scan export and the refine export. Presence first: the plan holds a collected command, the
/// generated timer's enable, the flatpak service's enable and tuned's.
#[test]
fn t21_every_systemctl_command_in_the_containerfile_is_planned() {
    use inspectah_pipeline::render::configtree::staged_writes;
    use inspectah_pipeline::render::unit_plan::plan_units;
    let host = with_cron_backup(
        tuned_host("enabled")
            .package("cockpit-ws")
            .unit("cockpit.socket", "enabled")
            .preset("disable cockpit.socket")
            .vendor_unit(
                "cockpit.socket",
                "[Socket]\nListenStream=9090\n[Install]\nWantedBy=sockets.target\n",
                Some("cockpit-ws"),
            )
            .unit("app.service", "enabled")
            .etc_unit(
                "app.service",
                "[Service]\nExecStart=/usr/local/bin/app\n[Install]\nWantedBy=multi-user.target\n",
            ),
    );
    let snap = with_flatpak_app(with_tuned_activation(scan(host)));

    let plan = plan_units(&snap, &staged_writes(&snap));
    let mut planned: Vec<(String, String)> = plan
        .planned
        .iter()
        .chain(&plan.scheduled)
        .chain(&plan.workloads)
        .map(|p| (p.action().to_string(), p.name().as_str().to_owned()))
        .collect();
    planned.sort();
    for unit in [
        "cockpit.socket",
        "app.service",
        TUNED,
        CRON_BACKUP_TIMER,
        FLATPAK_PROVISION,
    ] {
        assert!(
            planned.contains(&("enable".to_owned(), unit.to_owned())),
            "presence: {unit} is planned: {planned:?}"
        );
    }

    // The scan export renders the plan computed here; the refine export,
    // with no toggle changed, carries the same commands.
    for (route, tree) in both_exports(&snap) {
        let cf = containerfile_in(&tree);
        let mut rendered = systemctl_commands(&cf);
        rendered.sort();
        assert_eq!(rendered, planned, "{route}:\n{cf}");
        assert_kernel_section_writes_the_profile_only(&cf, route);
    }
}

/// The scheduled section's comment says which local timers the export
/// carries: one line for the timers whose file is staged (by its config
/// row, or its drop-in row for a copy of a vendor timer), one for those
/// whose file is not, with the plan's `UnitFileNotStaged` text. Control:
/// with both config rows included, both timers are on the carried line
/// and there is no not-carried line; with both excluded, only the
/// not-carried line is written.
#[test]
fn the_local_timers_comment_says_which_are_carried() {
    const CARRIED: &str = "# Local timers carried with their configuration or drop-in row";
    const NOT_CARRIED: &str = "# Local timers not carried";
    let body = "[Timer]\nOnCalendar=daily\n[Install]\nWantedBy=timers.target\n";
    let snap = scan(
        UnitHost::new()
            .unit("report.timer", "disabled")
            .unit("audit.timer", "disabled")
            .preset("disable *")
            .etc_unit("report.timer", body)
            .etc_unit("audit.timer", body),
    );
    let line = |cf: &str, prefix: &str| {
        cf.lines()
            .find(|l| l.starts_with(prefix))
            .map(str::to_owned)
    };

    let scanned = containerfile_in(&render_export(&snap));
    let carried = line(&scanned, CARRIED).expect("a carried line");
    assert!(
        carried.ends_with("(2): audit.timer, report.timer")
            || carried.ends_with("(2): report.timer, audit.timer"),
        "{carried}"
    );
    assert_eq!(line(&scanned, NOT_CARRIED), None, "{scanned}");

    let excluded = containerfile_in(&exported_after(
        &snap,
        &[(config_item(&etc_path("audit.timer")), false)],
    ));
    assert_eq!(
        line(&excluded, CARRIED).as_deref(),
        Some("# Local timers carried with their configuration or drop-in row (1): report.timer"),
        "{excluded}"
    );
    assert_eq!(
        line(&excluded, NOT_CARRIED).as_deref(),
        Some("# Local timers not carried (1): audit.timer (its unit file is not in the export)"),
        "{excluded}"
    );
    assert!(!excluded.contains("Existing local timers"), "{excluded}");

    // Every timer's row excluded: no carried line, both not carried.
    let all_excluded = containerfile_in(&exported_after(
        &snap,
        &[
            (config_item(&etc_path("audit.timer")), false),
            (config_item(&etc_path("report.timer")), false),
        ],
    ));
    assert_eq!(line(&all_excluded, CARRIED), None, "{all_excluded}");
    let not_carried = line(&all_excluded, NOT_CARRIED).expect("a not-carried line");
    assert!(
        not_carried.starts_with("# Local timers not carried (2): "),
        "{not_carried}"
    );
    for timer in ["audit.timer", "report.timer"] {
        assert!(
            not_carried.contains(&format!("{timer} (its unit file is not in the export)")),
            "{not_carried}"
        );
    }
}

// ---------------------------------------------------------------------
// § 8, § 11.1: every scheduled control applies or is refused by name
// (T35).
// ---------------------------------------------------------------------

/// The service the scheduled inspector generates beside
/// [`CRON_BACKUP_TIMER`].
const CRON_BACKUP_SERVICE: &str = "cron-backup.service";
/// The generated-timer row of the [`CRON_FILE`] entry.
const CRON_BACKUP: &str = "cron-backup";
/// The cron-job row of [`CRON_FILE`], as the scheduled inspector records
/// its path.
const CRON_ROW: &str = "etc/cron.d/backup";
/// The scheduled section's copy of the generated units it enables.
const SCHEDULED_COPY: &str = "COPY config/etc/systemd/system/ /etc/systemd/system/";

fn generated_item(name: &str) -> ItemId {
    ItemId::GeneratedTimer { name: name.into() }
}

fn cron_item(path: &str) -> ItemId {
    ItemId::CronJob { path: path.into() }
}

/// What an export carries of the generated `cron-backup` pair: how many
/// files of each name are in its tree, whether the scheduled section
/// copies the generated units, and whether the timer is enabled.
#[derive(Debug, PartialEq, Eq)]
struct GeneratedCarriage {
    timer: usize,
    service: usize,
    copy: bool,
    enable: bool,
}

const PAIR_CARRIED: GeneratedCarriage = GeneratedCarriage {
    timer: 1,
    service: 1,
    copy: true,
    enable: true,
};

const PAIR_NOT_CARRIED: GeneratedCarriage = GeneratedCarriage {
    timer: 0,
    service: 0,
    copy: false,
    enable: false,
};

fn generated_carriage(tree: &tempfile::TempDir) -> GeneratedCarriage {
    let cf = containerfile_in(tree);
    GeneratedCarriage {
        timer: files_named(tree.path(), CRON_BACKUP_TIMER).len(),
        service: files_named(tree.path(), CRON_BACKUP_SERVICE).len(),
        copy: scheduled_lines(&cf).iter().any(|l| l == SCHEDULED_COPY),
        enable: acts(&cf, "enable", CRON_BACKUP_TIMER),
    }
}

/// The include flag refine's decisions give the generated-timer row
/// `name`, if it has one.
fn generated_decision(session: &RefineSession, name: &str) -> Option<bool> {
    session
        .decisions()
        .generated_timers
        .iter()
        .find(|row| row.name == name)
        .map(|row| row.include)
}

/// T35, the generated-timer row: through the refine session and its
/// export, `SetInclude(false)` removes the generated timer, its service,
/// the scheduled section's copy and the timer's enable, and `true`
/// restores all four. Presence first. The excluded unit is not shown as
/// declined for another reason: no FIXME and no `declined_generated`
/// entry, and its decision row reads excluded.
#[test]
fn t35_the_generated_timer_row_removes_and_restores_its_unit() {
    let snap = scan(with_cron_backup(UnitHost::new().preset("disable *")));
    let mut refine = session(&snap);
    assert_eq!(
        generated_carriage(&export_session(&refine)),
        PAIR_CARRIED,
        "presence"
    );
    assert_eq!(generated_decision(&refine, CRON_BACKUP), Some(true));

    refine
        .apply(set_include(generated_item(CRON_BACKUP), false))
        .expect("the generated-timer row's exclusion applies");
    let tree = export_session(&refine);
    assert_eq!(generated_carriage(&tree), PAIR_NOT_CARRIED);
    let cf = containerfile_in(&tree);
    assert!(collision_fixme(&cf, CRON_BACKUP_TIMER).is_none(), "{cf}");
    assert!(in_no_systemctl_instruction(&cf, CRON_BACKUP_TIMER), "{cf}");
    assert!(refine.decisions().declined_generated.is_empty());
    assert_eq!(generated_decision(&refine, CRON_BACKUP), Some(false));

    refine
        .apply(set_include(generated_item(CRON_BACKUP), true))
        .expect("the generated-timer row's inclusion applies");
    assert_eq!(
        generated_carriage(&export_session(&refine)),
        PAIR_CARRIED,
        "re-included"
    );
    assert_eq!(generated_decision(&refine, CRON_BACKUP), Some(true));
}

/// T35, the cron-job row: excluding the cron job removes every unit
/// generated from it, through the refine export, and including it
/// restores them. While the cron job is excluded, including one of its
/// generated units is refused, naming both rows, and the export does not
/// change, so the two controls cannot contradict each other.
#[test]
fn t35_a_cron_job_row_carries_or_drops_the_units_it_generates() {
    let snap = scan(with_cron_backup(UnitHost::new().preset("disable *")));
    assert!(
        snap.scheduled_tasks
            .as_ref()
            .is_some_and(|st| st.cron_jobs.iter().any(|c| c.path == CRON_ROW)),
        "presence: the cron-job row"
    );
    let mut refine = session(&snap);
    assert_eq!(
        generated_carriage(&export_session(&refine)),
        PAIR_CARRIED,
        "presence"
    );

    refine
        .apply(set_include(cron_item(CRON_ROW), false))
        .expect("the cron-job row's exclusion applies");
    assert_eq!(
        generated_carriage(&export_session(&refine)),
        PAIR_NOT_CARRIED
    );
    assert_eq!(generated_decision(&refine, CRON_BACKUP), Some(false));

    let refused = refine
        .apply(set_include(generated_item(CRON_BACKUP), true))
        .expect_err("a generated unit of an excluded cron job stays excluded");
    let message = refused.to_string();
    assert!(
        message.contains(CRON_BACKUP) && message.contains(CRON_ROW),
        "{message}"
    );
    assert_eq!(
        generated_carriage(&export_session(&refine)),
        PAIR_NOT_CARRIED
    );

    refine
        .apply(set_include(cron_item(CRON_ROW), true))
        .expect("the cron-job row's inclusion applies");
    assert_eq!(
        generated_carriage(&export_session(&refine)),
        PAIR_CARRIED,
        "re-included"
    );
}

/// T35, the rows whose operation is refused: a cron job that generates no
/// unit (its file holds no entry), and an at job, which the export never
/// carries. Each `SetInclude`, either way, returns an error naming the
/// row, and the export is unchanged. The positive control is the cron job
/// above, whose operation applies.
#[test]
fn t35_a_cron_job_that_generates_nothing_and_an_at_job_are_refused_by_name() {
    const QUIET_CRON: &str = "/etc/cron.d/quiet";
    const QUIET_ROW: &str = "etc/cron.d/quiet";
    const AT_JOB: &str = "/var/spool/at/a0000101b2c3d4";
    const AT_ROW: &str = "var/spool/at/a0000101b2c3d4";
    let snap = scan(
        with_cron_backup(UnitHost::new().preset("disable *"))
            .file(QUIET_CRON, "# no entries\n")
            .file(AT_JOB, "#!/bin/sh\n/usr/local/bin/one-off\n"),
    );
    let st = snap.scheduled_tasks.as_ref().expect("a scheduled section");
    assert!(
        st.cron_jobs.iter().any(|c| c.path == QUIET_ROW),
        "presence: the quiet cron-job row"
    );
    assert!(
        !st.generated_timer_units
            .iter()
            .any(|u| u.source_path == QUIET_ROW),
        "presence: it generates nothing"
    );
    let at_row = st
        .at_jobs
        .iter()
        .find(|a| a.file == AT_ROW)
        .unwrap_or_else(|| panic!("presence: the at-job row: {:?}", st.at_jobs));

    let mut refine = session(&snap);
    let before = containerfile_in(&export_session(&refine));
    for include in [false, true] {
        for (item, row) in [
            (cron_item(QUIET_ROW), QUIET_ROW),
            (
                ItemId::AtJob {
                    file: at_row.file.clone(),
                },
                AT_ROW,
            ),
        ] {
            let refused = refine
                .apply(set_include(item.clone(), include))
                .expect_err("refused");
            assert!(refused.to_string().contains(row), "{item:?}: {refused}");
        }
    }
    assert_eq!(containerfile_in(&export_session(&refine)), before);
}

// ---------------------------------------------------------------------
// § 10.2 "What the plan takes" item 3: an inventory name blocks a
// generated pair (T36), and the decline names the unit that collided.
// ---------------------------------------------------------------------

/// The declined lines the export writes for generated `unit`, by prefix.
fn generated_fixmes<'a>(containerfile: &'a str, unit: &str) -> Vec<&'a str> {
    let prefix = format!("# FIXME: {unit} (inspectah generates this unit, ");
    containerfile
        .lines()
        .filter(|l| l.starts_with(&prefix))
        .collect()
}

/// The FIXME for a generated `cron-backup.timer` request declined because
/// `collided`, a unit the host has with no row, has a file's name.
fn inventory_collision_fixme(collided: &str) -> String {
    format!(
        "# FIXME: {CRON_BACKUP_TIMER} (inspectah generates this unit, and {collided}, a unit the \
         host already has, has the name of one of its files, so the generated files are not \
         written and the unit is not enabled here; {collided} has no row to exclude, so to \
         carry the job, write its timer and its service by hand under other names, keeping the \
         timer's Unit= or their shared name consistent)"
    )
}

/// The FIXME for a generated `cron-backup.timer` request declined because
/// `collided`, a unit with a row, has a file's name.
fn row_collision_fixme(collided: &str) -> String {
    format!(
        "# FIXME: {CRON_BACKUP_TIMER} (inspectah generates this unit, and a unit collected from \
         the host, {collided}, has the name of one of its files, so the generated files are not \
         written and the unit is not enabled here; to carry the generated unit, exclude the \
         collected unit and its file)"
    )
}

/// A package's `vendor` unit in `state`, at a preset that says the same,
/// beside the cron file that generates `cron-backup`.
fn inventory_collision_host(vendor: &str, state: &str) -> UnitHost {
    let preset = if state == "enabled" {
        "enable"
    } else {
        "disable"
    };
    let body = if vendor.ends_with(".timer") {
        HOST_CRON_BACKUP_TIMER
    } else {
        "[Service]\nExecStart=/usr/bin/vendor-backup\n[Install]\nWantedBy=multi-user.target\n"
    };
    with_cron_backup(
        UnitHost::new()
            .package("backuppkg")
            .unit(vendor, state)
            .preset(&format!("{preset} {vendor}"))
            .preset("disable *")
            .vendor_unit(vendor, body, Some("backuppkg")),
    )
}

/// T36: a package's unit named like one of the generated `cron-backup`
/// files, at its preset, enabled or disabled, so it is in the inventory
/// with no state change. Through the scan export and the refine export,
/// neither generated file is in the tree, no instruction names the
/// generated timer, and the request is declined with exactly one FIXME,
/// the inventory text naming the unit that collided: the timer's own name,
/// or the service's when only the service collides. Refine shows the same
/// decline on the scheduled row. Control: with no unit of either name the
/// pair is staged and enabled.
#[test]
fn t36_an_inventory_name_blocks_a_generated_pair() {
    use inspectah_refine::projection::{DeclinedRenders, GeneratedRowView};
    for vendor in [CRON_BACKUP_TIMER, CRON_BACKUP_SERVICE] {
        for state in ["enabled", "disabled"] {
            let label = format!("{vendor} {state}");
            let snap = scan(inventory_collision_host(vendor, state));
            let units = snap.units.as_ref().expect("a units section");
            assert!(
                units.inventory.iter().any(|e| e.unit.as_str() == vendor),
                "{label}: presence: the inventory entry"
            );
            assert!(
                !units
                    .state_changes
                    .iter()
                    .any(|sc| sc.unit().name().as_str() == vendor),
                "{label}: no state change"
            );
            assert!(
                snap.scheduled_tasks.as_ref().is_some_and(|st| st
                    .generated_timer_units
                    .iter()
                    .any(|u| u.name == CRON_BACKUP)),
                "{label}: presence: the generated unit"
            );
            let fixme = inventory_collision_fixme(vendor);
            for (route, tree) in both_exports(&snap) {
                let cf = containerfile_in(&tree);
                assert_eq!(
                    generated_fixmes(&cf, CRON_BACKUP_TIMER),
                    [fixme.as_str()],
                    "{label} {route}:\n{cf}"
                );
                for name in [CRON_BACKUP_TIMER, CRON_BACKUP_SERVICE] {
                    assert!(
                        files_named(tree.path(), name).is_empty(),
                        "{label} {route}: {name} is staged"
                    );
                }
                assert!(
                    in_no_systemctl_instruction(&cf, CRON_BACKUP_TIMER),
                    "{label} {route}:\n{cf}"
                );
            }
            let refine = session(&snap);
            let views = &refine.decisions().declined_generated;
            assert_eq!(views.len(), 1, "{label}: {views:?}");
            assert_eq!(
                views[0].row,
                GeneratedRowView::Scheduled {
                    name: CRON_BACKUP.into()
                }
            );
            assert_eq!(views[0].declined.renders, DeclinedRenders::Fixme);
            assert_eq!(views[0].declined.reason, "GeneratedNameCollision");
            assert_eq!(format!("# FIXME: {}", views[0].declined.text), fixme);
        }
    }

    let control = scan(with_cron_backup(UnitHost::new().preset("disable *")));
    for (route, tree) in both_exports(&control) {
        assert_eq!(generated_carriage(&tree), PAIR_CARRIED, "{route}");
        assert!(
            generated_fixmes(&containerfile_in(&tree), CRON_BACKUP_TIMER).is_empty(),
            "{route}"
        );
    }
}

/// The decline names the unit that collided. A package's
/// `cron-backup.service`, enabled against a `disable` preset, is a state
/// change row with the name of the generated pair's service only. Through
/// both exports the timer's request is declined with exactly one FIXME,
/// which names the service, not the timer, and gives the row advice; the
/// service's own enable is carried. Refine's decline says the same.
/// Control: the timer-name collision names the timer.
#[test]
fn a_generated_timer_declined_for_its_service_names_the_service() {
    let snap = scan(with_cron_backup(
        UnitHost::new()
            .base_package("glibc")
            .package("backuppkg")
            .unit(CRON_BACKUP_SERVICE, "enabled")
            .preset("disable *")
            .vendor_unit(
                CRON_BACKUP_SERVICE,
                "[Service]\nExecStart=/usr/bin/vendor-backup\n[Install]\nWantedBy=multi-user.target\n",
                Some("backuppkg"),
            ),
    ));
    assert!(
        matches!(
            fragment_of(&snap, CRON_BACKUP_SERVICE),
            UnitFragment::Vendor { .. }
        ),
        "presence: the service's state change"
    );
    let fixme = row_collision_fixme(CRON_BACKUP_SERVICE);
    for (route, tree) in both_exports(&snap) {
        let cf = containerfile_in(&tree);
        assert_eq!(
            generated_fixmes(&cf, CRON_BACKUP_TIMER),
            [fixme.as_str()],
            "{route}:\n{cf}"
        );
        assert!(acts(&cf, "enable", CRON_BACKUP_SERVICE), "{route}:\n{cf}");
        assert!(
            in_no_systemctl_instruction(&cf, CRON_BACKUP_TIMER),
            "{route}:\n{cf}"
        );
    }
    let refine = session(&snap);
    let views = &refine.decisions().declined_generated;
    assert_eq!(views.len(), 1, "{views:?}");
    assert_eq!(format!("# FIXME: {}", views[0].declined.text), fixme);

    let control = scan(with_cron_backup(
        UnitHost::new()
            .base_package("glibc")
            .package("backuppkg")
            .unit(CRON_BACKUP_TIMER, "enabled")
            .preset("disable *")
            .vendor_unit(CRON_BACKUP_TIMER, HOST_CRON_BACKUP_TIMER, Some("backuppkg")),
    ));
    let cf = containerfile_in(&render_export(&control));
    assert_eq!(
        generated_fixmes(&cf, CRON_BACKUP_TIMER),
        [row_collision_fixme(CRON_BACKUP_TIMER).as_str()],
        "{cf}"
    );
}

// ---------------------------------------------------------------------
// § 10.2 "No other writer under /etc/systemd/system": an unmanaged file
// there is withheld by name (T38).
// ---------------------------------------------------------------------

/// The unit the T38 host masks, at whose path an unmanaged zero-length
/// file is offered.
const T38_MASKED: &str = "masked.service";
/// An unmanaged file outside the unit directory, the control.
const T38_OPT: &str = "/opt/app/bin/tool";

/// T38's unmanaged entries: each path under `/etc/systemd/system` with the
/// payload the source tarball holds for it, the `/opt` control last. One
/// entry is spelled with an interior `.`, one sits in a `.d` directory,
/// one is a `.wants` link, and one is zero-length at a masked unit's path.
const T38_FILES: [(&str, &str); 5] = [
    (
        "/etc/systemd/system/x.service",
        "[Service]\nExecStart=/usr/bin/x\n",
    ),
    (
        "/etc/systemd/./system/y.service",
        "[Service]\nExecStart=/usr/bin/y\n",
    ),
    (
        "/etc/systemd/system/sshd.service.d/override.conf",
        "[Service]\nRestart=always\n",
    ),
    ("/etc/systemd/system/masked.service", ""),
    (T38_OPT, "#!/bin/sh\n"),
];
const T38_LINK: &str = "/etc/systemd/system/multi-user.target.wants/x.service";

/// An imported snapshot of a host that masks [`T38_MASKED`], whose
/// unmanaged-files section offers [`T38_FILES`] and [`T38_LINK`].
fn t38_snapshot() -> InspectionSnapshot {
    use inspectah_core::types::nonrpm::{FileType, UnmanagedFile, UnmanagedFileSection};
    let mut snap = scan(UnitHost::new().unit(T38_MASKED, "masked").mask(T38_MASKED));
    let mut items: Vec<UnmanagedFile> = T38_FILES
        .iter()
        .map(|(path, content)| UnmanagedFile {
            path: (*path).into(),
            size: content.len() as u64,
            file_type: FileType::Config,
            disposition: FindingKind::included(),
            ..Default::default()
        })
        .collect();
    items.push(UnmanagedFile {
        path: T38_LINK.into(),
        file_type: FileType::Symlink,
        link_target: "/etc/systemd/system/x.service".into(),
        disposition: FindingKind::included(),
        ..Default::default()
    });
    snap.unmanaged_files = Some(UnmanagedFileSection {
        total_count: items.len(),
        items,
        ..Default::default()
    });
    snap
}

/// A refine session over `snap`'s scan tarball with each of [`T38_FILES`]
/// in its `unmanaged/` payload, at the normal spelling of its path, and
/// [`T38_LINK`] as a link: what an imported tarball can hold. The session
/// exports from that tarball, which lives as long as the returned
/// directory.
fn t38_session(snap: &InspectionSnapshot) -> (RefineSession, tempfile::TempDir) {
    let tree = render_export(snap);
    for (path, content) in T38_FILES {
        let normal = path.replace("/./", "/");
        let at = tree
            .path()
            .join("unmanaged")
            .join(normal.trim_start_matches('/'));
        std::fs::create_dir_all(at.parent().expect("a parent")).expect("mkdir");
        std::fs::write(&at, content).expect("the payload writes");
    }
    let link = tree
        .path()
        .join("unmanaged")
        .join(T38_LINK.trim_start_matches('/'));
    std::fs::create_dir_all(link.parent().expect("a parent")).expect("mkdir");
    std::os::unix::fs::symlink("/etc/systemd/system/x.service", &link).expect("the link");
    let packed = tempfile::tempdir().expect("tempdir");
    let tarball = packed.path().join("scan.tar.gz");
    inspectah_pipeline::render::tarball::create_tarball(tree.path(), &tarball, "t38-host")
        .expect("the tarball packs");
    let mut session =
        inspectah_refine::tarball::from_tarball(&tarball).expect("refine opens the tarball");
    session.set_tarball_path(tarball);
    (session, packed)
}

/// The FIXME for an unmanaged file at `path` withheld under the unit
/// directory.
fn unmanaged_unit_path_fixme(path: &str) -> String {
    format!(
        "# FIXME: unmanaged file {path} not copied: an unmanaged file is not written under \
         /etc/systemd/system, where the export writes unit files only from their own rows; a \
         unit file belongs under Configuration Files"
    )
}

/// T38: an imported snapshot and tarball whose unmanaged-files section
/// offers files under `/etc/systemd/system`. Through the refine export,
/// none of them is extracted (`unmanaged/etc` holds nothing), no `COPY`
/// or `ln` names one, the Containerfile holds one FIXME per entry by its
/// destination, and refine's decisions show each row `withheld` with
/// `UnmanagedUnitPath`. The scan export writes the same lines. The
/// masked unit is still masked, so the zero-length file at its path does
/// not reach the image. Control: the `/opt` file in the same snapshot is
/// extracted and copied.
#[test]
fn t38_an_unmanaged_file_under_the_unit_directory_is_withheld() {
    let snap = t38_snapshot();
    let (refine, _source) = t38_session(&snap);
    let tree = export_session(&refine);
    let cf = containerfile_in(&tree);

    let opt = tree
        .path()
        .join("unmanaged")
        .join(T38_OPT.trim_start_matches('/'));
    assert!(opt.exists(), "control: the /opt file is extracted");
    assert!(
        cf.lines()
            .any(|l| l == format!("COPY unmanaged{T38_OPT} {T38_OPT}")),
        "control: the /opt file is copied:\n{cf}"
    );

    assert!(
        !tree.path().join("unmanaged/etc").exists(),
        "nothing under the unit directory is extracted"
    );
    let withheld: Vec<String> = T38_FILES
        .iter()
        .map(|(path, _)| *path)
        .filter(|path| *path != T38_OPT)
        .chain([T38_LINK])
        .map(|path| path.replace("/./", "/"))
        .collect();
    for route_cf in [&cf, &containerfile_in(&render_export(&snap))] {
        for path in &withheld {
            assert!(
                route_cf
                    .lines()
                    .any(|l| *l == unmanaged_unit_path_fixme(path)),
                "{path}:\n{route_cf}"
            );
        }
        assert!(
            route_cf
                .lines()
                .filter(|l| !l.starts_with('#'))
                .all(|l| !l.contains("unmanaged/etc") && !l.contains("ln -sf")),
            "{route_cf}"
        );
    }
    assert!(acts(&cf, "mask", T38_MASKED), "{cf}");

    let views = &refine.decisions().unmanaged_withheld;
    for (path, _) in T38_FILES.iter().filter(|(p, _)| *p != T38_OPT) {
        let view = views
            .iter()
            .find(|v| v.path == *path)
            .unwrap_or_else(|| panic!("no withheld view for {path}: {views:?}"));
        assert!(
            matches!(
                &view.withheld,
                inspectah_refine::projection::WithheldView::Withheld { why, .. }
                    if why == "UnmanagedUnitPath"
            ),
            "{view:?}"
        );
    }
    assert!(
        !views.iter().any(|v| v.path == T38_OPT),
        "control: the /opt row is not withheld"
    );
}

// ---------------------------------------------------------------------
// The generated-name collision across routes, hosts and partial
// exclusions.
// ---------------------------------------------------------------------

/// A local `cron-backup.timer`, enabled, on one host and the cron file that
/// generates `cron-backup` on the other, merged in both hostname orders.
/// Narrowing excludes each host's own rows. With the timer's unit row, its
/// config row and the cron job included in refine, the collected timer
/// wins whichever host is first: its file is staged with the host's
/// content, neither generated file is, the generated request is declined
/// with exactly one FIXME naming the timer, and the timer is enabled from
/// the unit state section. Control, on the same merged fleet and route:
/// with only the cron job included, the generated pair is staged and
/// enabled with no FIXME.
#[test]
fn a_local_and_a_generated_timer_of_one_name_on_two_hosts_in_both_orders() {
    let local = || {
        UnitHost::new()
            .unit(CRON_BACKUP_TIMER, "enabled")
            .preset("disable *")
            .etc_unit(CRON_BACKUP_TIMER, HOST_CRON_BACKUP_TIMER)
    };
    let cron = || with_cron_backup(UnitHost::new().preset("disable *"));
    for (local_name, cron_name) in [("host-a", "host-b"), ("host-b", "host-a")] {
        let label = format!("local on {local_name}");
        let fleet = merged(named(local(), local_name), named(cron(), cron_name));
        assert!(
            matches!(
                fragment_of(&fleet, CRON_BACKUP_TIMER),
                UnitFragment::Local { .. }
            ),
            "{label}: presence: the local timer's row"
        );
        assert!(
            fleet.scheduled_tasks.as_ref().is_some_and(|st| st
                .generated_timer_units
                .iter()
                .any(|u| u.name == CRON_BACKUP)),
            "{label}: presence: the generated unit"
        );

        let tree = exported_after(
            &fleet,
            &[
                (unit_item(CRON_BACKUP_TIMER), true),
                (config_item(&etc_path(CRON_BACKUP_TIMER)), true),
                (cron_item(CRON_ROW), true),
            ],
        );
        let cf = containerfile_in(&tree);
        let timers = files_named(tree.path(), CRON_BACKUP_TIMER);
        assert_eq!(timers.len(), 1, "{label}: {timers:?}");
        assert_eq!(
            std::fs::read_to_string(&timers[0]).unwrap(),
            HOST_CRON_BACKUP_TIMER,
            "{label}: the host's timer is staged"
        );
        assert!(
            files_named(tree.path(), CRON_BACKUP_SERVICE).is_empty(),
            "{label}: no generated service"
        );
        assert_eq!(
            generated_fixmes(&cf, CRON_BACKUP_TIMER),
            [row_collision_fixme(CRON_BACKUP_TIMER).as_str()],
            "{label}:\n{cf}"
        );
        assert!(
            unit_lines(&cf)
                .iter()
                .any(|l| l.contains("'cron-backup.timer'")),
            "{label}:\n{cf}"
        );

        let control = exported_after(&fleet, &[(cron_item(CRON_ROW), true)]);
        assert_eq!(generated_carriage(&control), PAIR_CARRIED, "{label}");
        assert!(
            generated_fixmes(&containerfile_in(&control), CRON_BACKUP_TIMER).is_empty(),
            "{label}"
        );
    }
}

/// A shadowed `site.timer` on two hosts with different `/etc` content:
/// with the shadow row included, the timer's enable is declined with
/// `InstallEvidenceChanged`, since the row may have been recorded from the
/// other variant. Presence first: a shadow is staged. Control: the same
/// content on both hosts is one variant, and the enable is carried through
/// the same route.
#[test]
fn a_timer_shadow_with_two_variants_declines_the_enable() {
    let host = |hostname: &str, on_calendar: &str| {
        named(
            UnitHost::new()
                .unit("site.timer", "enabled")
                .preset("disable site.timer")
                .vendor_unit(
                    "site.timer",
                    "[Timer]\nOnCalendar=daily\n[Install]\nWantedBy=timers.target\n",
                    Some("sitepkg"),
                )
                .etc_unit(
                    "site.timer",
                    &format!(
                        "[Timer]\nOnCalendar={on_calendar}\n[Install]\nWantedBy=timers.target\n"
                    ),
                ),
            hostname,
        )
    };
    let path = etc_path("site.timer");
    let shadow = ItemId::DropIn { path: path.clone() };

    let variants = merged(host("host-a", "hourly"), host("host-b", "weekly"));
    let tree = exported_after(&variants, &[(shadow.clone(), true)]);
    assert!(
        staged_at_drop_ins(&tree, &path),
        "presence: a shadow is staged"
    );
    let cf = containerfile_in(&tree);
    assert!(in_no_systemctl_instruction(&cf, "site.timer"), "{cf}");
    assert!(
        omitted(&cf, "site.timer").is_some_and(|l| l.ends_with(&evidence_changed(&path))),
        "{cf}"
    );

    let same = merged(host("host-a", "hourly"), host("host-b", "hourly"));
    let control = containerfile_in(&exported_after(&same, &[(shadow, true)]));
    assert!(acts(&control, "enable", "site.timer"), "{control}");
    assert!(omitted(&control, "site.timer").is_none(), "{control}");
}

/// An imported snapshot whose shadow of a vendor `cron-backup.timer` is
/// zero-length, beside the generated `cron-backup`. Through the scan
/// export, the refine export and the view: nothing lands at either
/// generated path (the zero-length shadow is withheld as a mask and the
/// generated pair for the collision), the generated request is declined
/// with exactly one FIXME naming the timer, and the timer's enable is
/// omitted as `InstallEvidenceChanged`, which its unit row shows. Control:
/// the shadow with content is staged and the enable carried, with the
/// generated request declined all the same.
#[test]
fn an_imported_zero_length_timer_shadow_beside_a_generated_timer() {
    let path = etc_path(CRON_BACKUP_TIMER);
    let scanned = scan(with_cron_backup(
        UnitHost::new()
            .base_package("glibc")
            .package("backuppkg")
            .unit(CRON_BACKUP_TIMER, "enabled")
            .preset("disable *")
            .vendor_unit(
                CRON_BACKUP_TIMER,
                "[Timer]\nOnCalendar=daily\n[Install]\nWantedBy=timers.target\n",
                Some("backuppkg"),
            )
            .etc_unit(CRON_BACKUP_TIMER, HOST_CRON_BACKUP_TIMER),
    ));
    assert!(
        matches!(
            fragment_of(&scanned, CRON_BACKUP_TIMER),
            UnitFragment::Shadowed { .. }
        ),
        "presence: the shadowed row"
    );
    let mut imported = scanned.clone();
    let shadow = imported
        .units
        .as_mut()
        .expect("a units section")
        .drop_ins
        .iter_mut()
        .find(|d| d.path == path)
        .expect("presence: the shadow's drop-in row");
    shadow.content = String::new();

    for (route, tree) in both_exports(&imported) {
        let cf = containerfile_in(&tree);
        for name in [CRON_BACKUP_TIMER, CRON_BACKUP_SERVICE] {
            assert!(
                files_named(tree.path(), name).is_empty(),
                "{route}: {name} lands"
            );
        }
        assert_eq!(
            generated_fixmes(&cf, CRON_BACKUP_TIMER),
            [row_collision_fixme(CRON_BACKUP_TIMER).as_str()],
            "{route}:\n{cf}"
        );
        assert!(
            in_no_systemctl_instruction(&cf, CRON_BACKUP_TIMER),
            "{route}:\n{cf}"
        );
        assert!(
            omitted(&cf, CRON_BACKUP_TIMER).is_some_and(|l| l.ends_with(&evidence_changed(&path))),
            "{route}:\n{cf}"
        );
    }
    assert_eq!(
        declined_in(&session(&imported), CRON_BACKUP_TIMER),
        Some(evidence_declined(CRON_BACKUP_TIMER, &path))
    );

    for (route, tree) in both_exports(&scanned) {
        let cf = containerfile_in(&tree);
        assert!(
            staged_at_drop_ins(&tree, &path),
            "{route}: the shadow is staged"
        );
        assert!(acts(&cf, "enable", CRON_BACKUP_TIMER), "{route}:\n{cf}");
        assert_eq!(
            generated_fixmes(&cf, CRON_BACKUP_TIMER),
            [row_collision_fixme(CRON_BACKUP_TIMER).as_str()],
            "{route}:\n{cf}"
        );
    }
}

/// § 10.2's flatpak rescan case, the two partial exclusions: with only the
/// collected unit row excluded, or only its config row, the collected name
/// still holds a row, so the generated service is withheld with exactly one
/// FIXME and no `COPY`. The control, both excluded, is
/// `a_rescanned_flatpak_provisioning_service_keeps_the_collected_copy`.
#[test]
fn a_rescanned_flatpak_service_with_one_of_its_rows_excluded_stays_collected() {
    const COLLECTED: &str =
        "[Service]\nType=oneshot\nExecStart=/usr/bin/true\n[Install]\nWantedBy=multi-user.target\n";
    let snap = with_flatpak_app(scan(
        UnitHost::new()
            .unit(FLATPAK_PROVISION, "enabled")
            .preset("disable *")
            .etc_unit(FLATPAK_PROVISION, COLLECTED),
    ));
    let path = etc_path(FLATPAK_PROVISION);
    for (label, op) in [
        ("unit row excluded", (unit_item(FLATPAK_PROVISION), false)),
        ("config row excluded", (config_item(&path), false)),
    ] {
        let tree = exported_after(&snap, &[op]);
        let cf = containerfile_in(&tree);
        let prefix = format!("# FIXME: {FLATPAK_PROVISION} (inspectah generates this unit, ");
        assert_eq!(
            cf.lines().filter(|l| l.starts_with(&prefix)).count(),
            1,
            "{label}:\n{cf}"
        );
        assert!(
            collision_fixme(&cf, FLATPAK_PROVISION).is_some(),
            "{label}:\n{cf}"
        );
        assert!(!cf.contains(FLATPAK_PROVISION_COPY), "{label}:\n{cf}");
        assert!(
            !tree.path().join("flatpak").join(FLATPAK_PROVISION).exists(),
            "{label}: the generated service is staged"
        );
    }
}

// ---------------------------------------------------------------------
// § 8 and § 10.2 rule 5: a carried timer needs its trigger (T37).
// ---------------------------------------------------------------------

/// A local `report.timer` whose `[Timer]` adds `timer_extra`, enabled
/// against a `disable` preset, and the local units `report.service` and
/// `other.service`.
fn t37_host(timer_extra: &str) -> UnitHost {
    UnitHost::new()
        .base_package("glibc")
        .unit("report.timer", "enabled")
        .unit("report.service", "static")
        .unit("other.service", "static")
        .preset("disable *")
        .etc_unit(
            "report.timer",
            &format!(
                "[Unit]\nDescription=Nightly report\n[Timer]\nOnCalendar=daily\n{timer_extra}\
                 [Install]\nWantedBy=timers.target\n"
            ),
        )
        .etc_unit(
            "report.service",
            "[Service]\nType=oneshot\nExecStart=/usr/local/bin/report\n",
        )
        .etc_unit(
            "other.service",
            "[Service]\nType=oneshot\nExecStart=/usr/local/bin/other\n",
        )
}

/// The local description of `report.timer`.
fn report_timer(snap: &InspectionSnapshot) -> &inspectah_core::types::scheduled::SystemdTimer {
    snap.scheduled_tasks
        .as_ref()
        .and_then(|st| {
            st.systemd_timers
                .iter()
                .find(|t| t.name == "report" && t.source == "local")
        })
        .expect("presence: report.timer's description")
}

fn trigger_unit(name: &str) -> inspectah_core::types::scheduled::TimerTrigger {
    inspectah_core::types::scheduled::TimerTrigger::Unit(name.to_owned().try_into().unwrap())
}

/// The `TimerTriggerNotStaged` line for `report.timer` and `trigger`.
fn trigger_not_staged(trigger: &str) -> String {
    format!(
        "# Omitted: report.timer (its unit {trigger} is not in the export, and a timer whose \
         unit is missing does not start)"
    )
}

/// T37: an enabled local timer whose trigger, the paired `report.service`
/// or the `other.service` its `Unit=` names, has its config row excluded:
/// no instruction names the timer, the `TimerTriggerNotStaged` line names
/// the trigger, and the timer's refine row shows the reason. Including the
/// trigger's row again brings the enable back. Presence and the positive
/// control: the unchanged refine export enables the timer.
#[test]
fn t37_a_timer_whose_trigger_is_excluded_is_not_enabled() {
    for (extra, trigger) in [
        ("", "report.service"),
        ("Unit=other.service\n", "other.service"),
    ] {
        let snap = scan(t37_host(extra));
        let timer = report_timer(&snap);
        assert_eq!(timer.trigger, trigger_unit(trigger), "{trigger}");
        assert!(timer.trigger_drop_ins.is_empty(), "{trigger}");
        let unchanged = containerfile_in(&exported_after(&snap, &[]));
        assert!(acts(&unchanged, "enable", "report.timer"), "{unchanged}");

        let excluded_op = (config_item(&etc_path(trigger)), false);
        let tree = exported_after(&snap, std::slice::from_ref(&excluded_op));
        let cf = containerfile_in(&tree);
        assert!(in_no_systemctl_instruction(&cf, "report.timer"), "{cf}");
        assert_eq!(
            omitted(&cf, "report.timer"),
            Some(trigger_not_staged(trigger).as_str()),
            "{cf}"
        );
        let mut refine = session(&snap);
        refine
            .apply(set_include(excluded_op.0.clone(), false))
            .expect("applies");
        let declined = declined_in(&refine, "report.timer").expect("a declined row");
        assert_eq!(declined.reason, "TimerTriggerNotStaged");
        assert_eq!(
            format!("# Omitted: {}", declined.text),
            trigger_not_staged(trigger)
        );

        let restored = containerfile_in(&exported_after(
            &snap,
            &[excluded_op.clone(), (excluded_op.0, true)],
        ));
        assert!(acts(&restored, "enable", "report.timer"), "{restored}");
    }
}

/// T37, changed evidence: `report.timer` gets `Unit=other.service` from a
/// drop-in under `/etc`. The drop-in is recorded in `trigger_drop_ins`.
/// With it and `report.service` excluded, the enable is omitted as
/// `TimerTriggerEvidenceChanged` naming the drop-in, although
/// `other.service` is staged: in the image the timer would start
/// `report.service`. With the drop-in included again the enable returns.
#[test]
fn t37_a_timer_whose_unit_drop_in_is_excluded_is_not_enabled() {
    const DROP_IN: &str = "/etc/systemd/system/report.timer.d/unit.conf";
    let snap = scan(t37_host("").file(DROP_IN, "[Timer]\nUnit=other.service\n"));
    let timer = report_timer(&snap);
    assert_eq!(timer.trigger, trigger_unit("other.service"));
    assert_eq!(timer.trigger_drop_ins, [DROP_IN]);
    let drop_in = ItemId::DropIn {
        path: DROP_IN.into(),
    };
    let excluded = [
        (drop_in.clone(), false),
        (config_item(&etc_path("report.service")), false),
    ];
    let tree = exported_after(&snap, &excluded);
    assert!(
        !files_named(tree.path(), "other.service").is_empty(),
        "other.service is staged"
    );
    let cf = containerfile_in(&tree);
    assert!(in_no_systemctl_instruction(&cf, "report.timer"), "{cf}");
    assert_eq!(
        omitted(&cf, "report.timer"),
        Some(
            format!(
                "# Omitted: report.timer (the drop-in {DROP_IN} that sets its Unit= is not in \
                 the export, so the unit it would start is not the one recorded)"
            )
            .as_str()
        ),
        "{cf}"
    );

    let mut restored = excluded.to_vec();
    restored.push((drop_in, true));
    let cf = containerfile_in(&exported_after(&snap, &restored));
    assert!(acts(&cf, "enable", "report.timer"), "{cf}");
}

/// T37, an unresolved trigger: `Unit=` with a specifier the inspector does
/// not expand records the trigger as `Unresolved`, and the enable is
/// omitted as `TimerTriggerUnresolved`, through both exports.
#[test]
fn t37_a_timer_whose_unit_cannot_be_resolved_is_not_enabled() {
    let snap = scan(t37_host("Unit=report-%H.service\n"));
    assert_eq!(
        report_timer(&snap).trigger,
        inspectah_core::types::scheduled::TimerTrigger::Unresolved
    );
    for (route, tree) in both_exports(&snap) {
        let cf = containerfile_in(&tree);
        assert!(
            in_no_systemctl_instruction(&cf, "report.timer"),
            "{route}:\n{cf}"
        );
        assert_eq!(
            omitted(&cf, "report.timer"),
            Some(
                "# Omitted: report.timer (its Unit= could not be resolved to a unit name, so \
                 inspectah cannot tell whether that unit is in the image)"
            ),
            "{route}:\n{cf}"
        );
    }
}

/// T37, the triggers that pass: one a package in the image owns, one that
/// is a full shadow with the shadow excluded (the vendor file loads), and
/// one that is a staged generated service. Each timer is enabled through
/// the refine export. A `disable` and a `mask` of a timer whose service is
/// excluded pass too.
#[test]
fn t37_timers_whose_trigger_is_provided_are_carried() {
    let packaged = scan(
        t37_host("Unit=vendor.service\n")
            .package("vendorpkg")
            .unit("vendor.service", "static")
            .vendor_unit(
                "vendor.service",
                "[Service]\nExecStart=/usr/bin/vendor\n",
                Some("vendorpkg"),
            ),
    );
    assert_eq!(
        report_timer(&packaged).trigger,
        trigger_unit("vendor.service")
    );
    let cf = containerfile_in(&exported_after(&packaged, &[]));
    assert!(acts(&cf, "enable", "report.timer"), "packaged:\n{cf}");

    let shadow = etc_path("site.service");
    let shadowed = scan(
        t37_host("Unit=site.service\n")
            .package("sitepkg")
            .unit("site.service", "static")
            .vendor_unit(
                "site.service",
                "[Service]\nExecStart=/usr/bin/site\n",
                Some("sitepkg"),
            )
            .etc_unit(
                "site.service",
                "[Service]\nExecStart=/usr/bin/site --local\n",
            ),
    );
    let cf = containerfile_in(&exported_after(
        &shadowed,
        &[(ItemId::DropIn { path: shadow }, false)],
    ));
    assert!(acts(&cf, "enable", "report.timer"), "shadowed:\n{cf}");

    let generated = scan(with_cron_backup(t37_host("Unit=cron-backup.service\n")));
    let cf = containerfile_in(&exported_after(&generated, &[]));
    assert!(acts(&cf, "enable", CRON_BACKUP_TIMER), "generated:\n{cf}");
    assert!(acts(&cf, "enable", "report.timer"), "generated:\n{cf}");

    // A local timer has no `disable` (a local unit is a state change only
    // when enabled), so the `disable` case is a shadowed vendor timer.
    let disabled = scan(
        shadowed_timer_host("disabled", "enable")
            .unit("site.service", "static")
            .etc_unit("site.service", "[Service]\nExecStart=/usr/bin/site\n"),
    );
    let cf = containerfile_in(&exported_after(
        &disabled,
        &[(config_item(&etc_path("site.service")), false)],
    ));
    assert!(acts(&cf, "disable", "site.timer"), "disable:\n{cf}");

    let masked = scan(
        UnitHost::new()
            .unit("report.timer", "masked")
            .unit("report.service", "static")
            .preset("disable *")
            .mask("report.timer")
            .etc_unit("report.service", "[Service]\nExecStart=/usr/bin/report\n"),
    );
    let cf = containerfile_in(&exported_after(
        &masked,
        &[(config_item(&etc_path("report.service")), false)],
    ));
    assert!(acts(&cf, "mask", "report.timer"), "mask:\n{cf}");
}

const WORKER_INSTANCE_TIMER: &str = "worker@a.timer";
const WORKER_UNIT_DROP_IN: &str = "/etc/systemd/system/worker@.timer.d/10-unit.conf";

/// The local template `worker@.timer`, whose `[Timer]` adds `timer_extra`,
/// with `worker@.service` and `other@.service`, and its instance
/// `worker@a.timer` enabled by its link under `/etc` alone.
fn t37_instance_host(timer_extra: &str) -> UnitHost {
    let host = UnitHost::new()
        .base_package("glibc")
        .unit("worker@.timer", "indirect")
        .unit("worker@.service", "static")
        .unit("other@.service", "static")
        .preset("disable *")
        .etc_unit(
            "worker@.timer",
            &format!(
                "[Unit]\nDescription=Worker %i\n[Timer]\nOnCalendar=daily\n{timer_extra}\
                 [Install]\nWantedBy=timers.target\n"
            ),
        )
        .etc_unit(
            "worker@.service",
            "[Service]\nType=oneshot\nExecStart=/usr/local/bin/worker %i\n",
        )
        .etc_unit(
            "other@.service",
            "[Service]\nType=oneshot\nExecStart=/usr/local/bin/other %i\n",
        );
    instance_link(
        host,
        "/etc/systemd/system",
        "timers.target",
        WORKER_INSTANCE_TIMER,
        &etc_path("worker@.timer"),
    )
}

/// The description recorded for `worker@a.timer`.
fn worker_timer(snap: &InspectionSnapshot) -> &inspectah_core::types::scheduled::SystemdTimer {
    snap.scheduled_tasks
        .as_ref()
        .and_then(|st| st.systemd_timers.iter().find(|t| t.name == "worker@a"))
        .expect("presence: worker@a.timer's description")
}

/// T37's scan-built instance case through the real inspectors (Task 5.1a):
/// an enabled instance timer, found by its link, is described from its
/// local template with its trigger expanded for the instance, from the
/// template's file or from a drop-in in the template's `<prefix>@.timer.d`,
/// which is then the trigger's evidence and a row refine can exclude. Its
/// enable is carried. With the trigger's file excluded (the template
/// `worker@.service` or `other@.service`, which is what loads for the
/// instance), it is omitted as `TimerTriggerNotStaged`; with the template
/// directory's drop-in excluded, as `TimerTriggerEvidenceChanged`; with
/// the timer template's own file excluded, as `UnitFileNotStaged`.
#[test]
fn t37_an_instance_timers_trigger_is_recorded_from_its_template() {
    for (extra, trigger) in [
        ("", "worker@a.service"),
        ("Unit=other@%i.service\n", "other@a.service"),
    ] {
        let snap = scan(t37_instance_host(extra));
        let timer = worker_timer(&snap);
        assert_eq!(timer.trigger, trigger_unit(trigger), "{extra:?}");
        assert!(timer.trigger_drop_ins.is_empty(), "{extra:?}");
        assert_eq!(timer.source, "local");
        assert_eq!(timer.description, "Worker %i");
        assert_eq!(
            change(&snap, WORKER_INSTANCE_TIMER).current_state(),
            UnitState::Enabled
        );
        assert_instance_carried(&snap, WORKER_INSTANCE_TIMER, extra);
        let template = format!("{}@.service", trigger.split('@').next().expect("a prefix"));
        let not_staged = containerfile_in(&exported_after(
            &snap,
            &[(config_item(&etc_path(&template)), false)],
        ));
        assert_eq!(
            omitted(&not_staged, WORKER_INSTANCE_TIMER),
            Some(
                format!(
                    "# Omitted: {WORKER_INSTANCE_TIMER} (its unit {trigger} is not in the \
                     export, and a timer whose unit is missing does not start)"
                )
                .as_str()
            ),
            "{extra:?}:\n{not_staged}"
        );
        assert!(
            in_no_systemctl_instruction(&not_staged, WORKER_INSTANCE_TIMER),
            "{extra:?}:\n{not_staged}"
        );
        // The instance's file is its template's, which is staged: the
        // scheduled section lists it as carried on both routes, never as
        // not carried.
        for (route, containerfile) in both_routes(&snap) {
            let line = |prefix: &str| {
                containerfile
                    .lines()
                    .find(|l| l.starts_with(prefix))
                    .map(str::to_owned)
            };
            assert!(
                line("# Local timers not carried")
                    .is_none_or(|l| !l.contains(WORKER_INSTANCE_TIMER)),
                "{extra:?}, {route}:\n{containerfile}"
            );
            assert!(
                line("# Local timers carried with their configuration or drop-in row")
                    .is_some_and(|l| l.contains(WORKER_INSTANCE_TIMER)),
                "{extra:?}, {route}:\n{containerfile}"
            );
        }
        // With the template's config row excluded, the instance moves to
        // the "not carried" line.
        let excluded = containerfile_in(&exported_after(
            &snap,
            &[(config_item(&etc_path("worker@.timer")), false)],
        ));
        let line = |prefix: &str| {
            excluded
                .lines()
                .find(|l| l.starts_with(prefix))
                .map(str::to_owned)
        };
        assert!(
            line("# Local timers not carried").is_some_and(|l| l.contains(WORKER_INSTANCE_TIMER)),
            "{extra:?}, excluded:\n{excluded}"
        );
        assert!(
            line("# Local timers carried with their configuration or drop-in row")
                .is_none_or(|l| !l.contains(WORKER_INSTANCE_TIMER)),
            "{extra:?}, excluded:\n{excluded}"
        );
        assert_eq!(
            omitted(&excluded, WORKER_INSTANCE_TIMER),
            Some(format!("# Omitted: {WORKER_INSTANCE_TIMER} ({NOT_STAGED}").as_str()),
            "{extra:?}, excluded:\n{excluded}"
        );
    }

    let snap =
        scan(t37_instance_host("").file(WORKER_UNIT_DROP_IN, "[Timer]\nUnit=other@%i.service\n"));
    let timer = worker_timer(&snap);
    assert_eq!(timer.trigger, trigger_unit("other@a.service"));
    assert_eq!(timer.trigger_drop_ins, [WORKER_UNIT_DROP_IN]);
    let mut refine = session(&snap);
    refine
        .apply(set_include(
            ItemId::DropIn {
                path: WORKER_UNIT_DROP_IN.into(),
            },
            false,
        ))
        .expect("the template drop-in is a row refine can exclude");
    assert_instance_carried(&snap, WORKER_INSTANCE_TIMER, "drop-in");
    let changed = containerfile_in(&exported_after(
        &snap,
        &[(drop_in_item(WORKER_UNIT_DROP_IN), false)],
    ));
    assert_eq!(
        omitted(&changed, WORKER_INSTANCE_TIMER),
        Some(
            format!(
                "# Omitted: {WORKER_INSTANCE_TIMER} (the drop-in {WORKER_UNIT_DROP_IN} that sets \
                 its Unit= is not in the export, so the unit it would start is not the one \
                 recorded)"
            )
            .as_str()
        ),
        "{changed}"
    );
    assert_eq!(
        declined_in(&refine, WORKER_INSTANCE_TIMER).map(|d| d.reason),
        Some("TimerTriggerEvidenceChanged".to_owned())
    );
}

/// T37, a partial listing: when the listing of `report.timer.d` fails part
/// way, the drop-in that sets `Unit=` may be the one left out, so the
/// trigger is `Unresolved` and the enable omitted on both routes. Healthy
/// first: the drop-in read, the trigger is the unit it names.
#[test]
fn t37_a_partly_listed_timer_drop_in_directory_leaves_the_trigger_unknown() {
    const DIR: &str = "/etc/systemd/system/report.timer.d";
    let host = || t37_host("").file(&format!("{DIR}/unit.conf"), "[Timer]\nUnit=other.service\n");
    assert_eq!(
        report_timer(&scan(host())).trigger,
        trigger_unit("other.service")
    );
    let snap = scan(host().partial_listing(DIR, "unit.conf"));
    assert_eq!(
        report_timer(&snap).trigger,
        inspectah_core::types::scheduled::TimerTrigger::Unresolved
    );
    for (route, containerfile) in both_routes(&snap) {
        assert!(
            in_no_systemctl_instruction(&containerfile, "report.timer"),
            "{route}:\n{containerfile}"
        );
    }
}

/// T37, a partial listing of the timer directories: when the listing of
/// `/etc/systemd/system` fails part way after leaving out the local
/// `report.timer`, which sets `Unit=other.service` over a vendor file of
/// the same name, the vendor description must not stand in for it: its
/// trigger is `Unresolved`, the scheduled tasks degrade naming the
/// directory, and the enable is omitted on both routes. Healthy first:
/// the local description is read and its trigger is `other.service`.
#[test]
fn t37_a_partly_listed_timer_directory_leaves_the_vendor_trigger_unknown() {
    use inspectah_core::types::scheduled::TimerTrigger;
    const DIR: &str = "/etc/systemd/system";
    let host = || {
        t37_host("Unit=other.service\n").vendor_unit(
            "report.timer",
            "[Timer]\nOnCalendar=weekly\n[Install]\nWantedBy=timers.target\n",
            Some("glibc"),
        )
    };
    let descriptions = |snap: &InspectionSnapshot| -> Vec<(String, TimerTrigger)> {
        snap.scheduled_tasks
            .iter()
            .flat_map(|st| &st.systemd_timers)
            .filter(|t| t.name == "report")
            .map(|t| (t.source.clone(), t.trigger.clone()))
            .collect()
    };
    let healthy = scan(host());
    assert_eq!(
        report_timer(&healthy).trigger,
        trigger_unit("other.service")
    );

    let snap = scan(host().partial_listing(DIR, "report.timer"));
    let scheduled_degraded: Vec<&str> = snap
        .warnings
        .iter()
        .filter(|w| w.inspector == "scheduled_tasks" && w.message.starts_with("degraded: "))
        .map(|w| w.message.as_str())
        .collect();
    assert!(
        scheduled_degraded
            .iter()
            .any(|m| m.contains(&format!("{DIR}: "))),
        "the scheduled tasks degrade naming {DIR}: {:?}",
        snap.warnings
    );
    let described = descriptions(&snap);
    assert!(
        described
            .iter()
            .all(|(_, trigger)| *trigger == TimerTrigger::Unresolved),
        "no description of report.timer has a trigger: {described:?}"
    );
    for (route, containerfile) in both_routes(&snap) {
        assert!(
            in_no_systemctl_instruction(&containerfile, "report.timer"),
            "{route}:\n{containerfile}"
        );
    }
}

/// T37, hidden drop-ins: systemd skips a drop-in whose name starts with
/// `.` (`systemd_reads_drop_in`), so a vendor `.override.conf` that sets
/// `Unit=` to a unit the export stages is not trigger evidence. For a plain
/// timer and an instance timer, the trigger recorded is the real one, the
/// evidence list is empty, and with the real trigger's file excluded the
/// enable is omitted as `TimerTriggerNotStaged` naming it. Control: the
/// same file visible changes the recorded trigger.
#[test]
fn t37_a_hidden_drop_in_is_not_trigger_evidence() {
    let plain = (
        t37_host(""),
        t37_host(""),
        "report.timer",
        "/usr/lib/systemd/system/report.timer.d",
        "[Timer]\nUnit=other.service\n",
        "report.service",
        "report.service",
        "other.service",
    );
    let instance = (
        t37_instance_host(""),
        t37_instance_host(""),
        WORKER_INSTANCE_TIMER,
        "/usr/lib/systemd/system/worker@.timer.d",
        "[Timer]\nUnit=other@%i.service\n",
        "worker@a.service",
        "worker@.service",
        "other@a.service",
    );
    for (hidden_host, visible_host, timer, dir, text, real, real_file, other) in [plain, instance] {
        let name = timer.strip_suffix(".timer").expect("a timer");
        let described = |snap: &InspectionSnapshot| {
            snap.scheduled_tasks
                .as_ref()
                .and_then(|st| st.systemd_timers.iter().find(|t| t.name == name))
                .cloned()
                .expect("presence: the timer's description")
        };
        let hidden = scan(hidden_host.file(&format!("{dir}/.override.conf"), text));
        let description = described(&hidden);
        assert_eq!(description.trigger, trigger_unit(real), "{timer}");
        assert!(description.trigger_drop_ins.is_empty(), "{timer}");
        let excluded = [(config_item(&etc_path(real_file)), false)];
        let cf = containerfile_in(&exported_after(&hidden, &excluded));
        assert_eq!(
            omitted(&cf, timer),
            Some(
                format!(
                    "# Omitted: {timer} (its unit {real} is not in the export, and a timer \
                     whose unit is missing does not start)"
                )
                .as_str()
            ),
            "{timer}:\n{cf}"
        );
        assert!(in_no_systemctl_instruction(&cf, timer), "{timer}:\n{cf}");
        assert_eq!(
            declined_in(&session_after(&hidden, &excluded), timer).map(|d| d.reason),
            Some("TimerTriggerNotStaged".to_owned()),
            "{timer}"
        );

        let visible = scan(visible_host.file(&format!("{dir}/override.conf"), text));
        assert_eq!(described(&visible).trigger, trigger_unit(other), "{timer}");
    }
}

// ---------------------------------------------------------------------
// § 10.2 "Never the machine-id" (T33).
// ---------------------------------------------------------------------

const MACHINE_ID: &str = "/etc/machine-id";
/// The control path: a file beside the machine-id that the stager writes.
const MACHINE_INFO: &str = "/etc/machine-info";

/// What T33's offers hold: the value whose presence makes first boot apply
/// the presets (§ 10.1), so a write of it anywhere would show.
const UNINITIALIZED: &str = "uninitialized";

/// T33's odd spellings of `path`: relative, a doubled separator, an
/// interior `.`, a trailing separator.
fn t33_spellings(path: &str) -> [String; 4] {
    let (dir, name) = path.rsplit_once('/').expect("an absolute path");
    [
        path.trim_start_matches('/').to_owned(),
        format!("{dir}//{name}"),
        format!("{dir}/./{name}"),
        format!("{path}/"),
    ]
}

/// The two routes a T33 snapshot offers a path through.
#[derive(Debug, Clone, Copy, PartialEq)]
enum T33Route {
    ConfigEntry,
    DropIn,
}

/// `snap` with two included rows of `route` offering one destination: one
/// at `spelling` and one at the plain `path`, with one content.
fn offering(
    snap: &InspectionSnapshot,
    route: T33Route,
    path: &str,
    spelling: &str,
) -> InspectionSnapshot {
    let mut imported = snap.clone();
    let content = format!("{UNINITIALIZED}\n");
    for p in [spelling, path] {
        match route {
            T33Route::ConfigEntry => imported = with_config_entry(&imported, p, &content),
            T33Route::DropIn => imported
                .units
                .as_mut()
                .expect("a units section")
                .drop_ins
                .push(inspectah_core::types::units::SystemdDropIn {
                    unit: "sshd.service".into(),
                    path: p.into(),
                    content: content.clone(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                }),
        }
    }
    imported
}

/// The withheld view of the included row of `route` at `path`, as offered.
fn row_withheld(
    refine: &RefineSession,
    route: T33Route,
    path: &str,
) -> Option<inspectah_refine::projection::WithheldView> {
    let decisions = refine.decisions();
    match route {
        T33Route::ConfigEntry => decisions
            .config_withheld
            .iter()
            .find(|v| v.path == path)
            .map(|v| v.withheld.clone()),
        T33Route::DropIn => decisions
            .unit_dropins
            .iter()
            .find(|v| v.entry.path == path)
            .and_then(|v| v.withheld.clone()),
    }
}

/// The FIXME the Containerfile writes for a `route` row offering the
/// machine-id.
fn machine_id_fixme(route: T33Route) -> String {
    let label = match route {
        T33Route::ConfigEntry => "config file",
        T33Route::DropIn => "unit drop-in",
    };
    format!(
        "# FIXME: {label} {MACHINE_ID} not copied: the image keeps the base image's empty \
         /etc/machine-id, which is what makes first boot keep the rendered unit states; the \
         source host's machine identity is not copied"
    )
}

/// Every file under the export's staged trees (`config/`, `drop-ins/`,
/// `unmanaged/`, `flatpak/`) whose content holds `needle`.
fn staged_files_holding(tree: &tempfile::TempDir, needle: &str) -> Vec<String> {
    ["config", "drop-ins", "unmanaged", "flatpak"]
        .iter()
        .flat_map(|root| files_holding(&tree.path().join(root), needle))
        .collect()
}

/// T33 through the scan export and the refine export: a snapshot that
/// offers `/etc/machine-id` through two config entries, and separately
/// through two drop-in rows, one at each odd spelling and one at the
/// plain path, stages nothing there; the Containerfile names each in a
/// FIXME; each offering row shows `withheld` with `MachineId`, both rows of
/// the one destination included. No staged file and no instruction holds
/// `uninitialized`, and the scan export's README holds the two first-boot
/// sentences (the refine export writes no README). Control: `/etc/machine-info`, offered in each of the same ways, is
/// staged and its rows are not withheld.
#[test]
fn t33_the_machine_id_is_never_staged() {
    let snap = scan(t5_host());
    for route in [T33Route::ConfigEntry, T33Route::DropIn] {
        for spelling in t33_spellings(MACHINE_ID) {
            let label = format!("{route:?} at {spelling:?}");
            let imported = offering(&snap, route, MACHINE_ID, &spelling);
            let refine = session(&imported);
            for (via, tree) in [
                ("scan", render_export(&imported)),
                ("refine", export_session(&refine)),
            ] {
                assert!(!staged_at(&tree, MACHINE_ID), "{label}, {via}");
                assert!(
                    staged_files_holding(&tree, UNINITIALIZED).is_empty(),
                    "{label}, {via}: {:?}",
                    staged_files_holding(&tree, UNINITIALIZED)
                );
                let cf = containerfile_in(&tree);
                assert_eq!(
                    cf.lines().filter(|l| *l == machine_id_fixme(route)).count(),
                    2,
                    "{label}, {via}: one FIXME per row:\n{cf}"
                );
                assert!(
                    cf.lines()
                        .filter(|l| !l.trim_start().starts_with('#'))
                        .all(|l| !l.contains(UNINITIALIZED) && !l.contains("machine-id")),
                    "{label}, {via}:\n{cf}"
                );
                // The refine export's file set has no README (its
                // contract, `render_refine_export`).
                if via == "scan" {
                    let readme =
                        std::fs::read_to_string(tree.path().join("README.md")).expect("README.md");
                    assert!(
                        readme.contains(README_FIRST_BOOT),
                        "{label}, {via}:\n{readme}"
                    );
                }
            }
            for row in [spelling.as_str(), MACHINE_ID] {
                assert!(
                    matches!(
                        row_withheld(&refine, route, row),
                        Some(inspectah_refine::projection::WithheldView::Withheld { ref why, .. })
                            if why == "MachineId"
                    ),
                    "{label}, row {row:?}: {:?}",
                    row_withheld(&refine, route, row)
                );
            }

            let spelling = spelling.replace("machine-id", "machine-info");
            let control = offering(&snap, route, MACHINE_INFO, &spelling);
            let refine = session(&control);
            for (via, tree) in [
                ("scan", render_export(&control)),
                ("refine", export_session(&refine)),
            ] {
                assert!(staged_at(&tree, MACHINE_INFO), "control {label}, {via}");
            }
            for row in [spelling.as_str(), MACHINE_INFO] {
                assert_eq!(
                    row_withheld(&refine, route, row),
                    None,
                    "control {label}, row {row:?}"
                );
            }
        }
    }
}

/// T33, the scan: the config walk records no entry for `/etc/machine-id`
/// on a host that has one (`walk.rs`'s exact-exclusion list). Presence
/// first: an unowned file beside it, `/etc/app.conf`, is recorded.
#[test]
fn t33_a_host_scan_records_no_machine_id() {
    let snap = scan(
        t5_host()
            .file(MACHINE_ID, "4c4c4544004d3510804bc4c04f383432\n")
            .file("/etc/app.conf", "a=1\n"),
    );
    let paths: Vec<&str> = snap
        .config
        .iter()
        .flat_map(|c| &c.files)
        .map(|f| f.path.as_str())
        .chain(
            snap.units
                .iter()
                .flat_map(|u| &u.drop_ins)
                .map(|d| d.path.as_str()),
        )
        .collect();
    assert!(paths.contains(&"/etc/app.conf"), "presence: {paths:?}");
    assert!(!paths.contains(&MACHINE_ID), "{paths:?}");
}

/// An imported snapshot's unmanaged-files section that offers
/// `/etc/machine-id`: the shared decision withholds it as `MachineId`
/// before the scan bundles a payload, before refine extracts one, and
/// before render, so no `COPY` names it, the Containerfile names it in a
/// FIXME, and refine's unmanaged row shows `withheld` with `MachineId`.
/// Control: an unmanaged `/etc/machine-info` is copied.
#[test]
fn t33_an_unmanaged_machine_id_is_withheld() {
    use inspectah_core::types::nonrpm::{FileType, UnmanagedFile, UnmanagedFileSection};
    let mut snap = scan(t5_host());
    snap.unmanaged_files = Some(UnmanagedFileSection {
        total_count: 2,
        items: [MACHINE_ID, MACHINE_INFO]
            .iter()
            .map(|path| UnmanagedFile {
                path: (*path).into(),
                size: 14,
                file_type: FileType::Config,
                disposition: FindingKind::included(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    });
    let fixme = format!(
        "# FIXME: unmanaged file {MACHINE_ID} not copied: the image keeps the base image's \
         empty /etc/machine-id, which is what makes first boot keep the rendered unit states; \
         the source host's machine identity is not copied"
    );
    let cf = containerfile_in(&render_export(&snap));
    assert!(cf.lines().any(|l| l == fixme), "{cf}");
    assert!(
        cf.lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .all(|l| !l.contains("machine-id")),
        "{cf}"
    );
    assert!(
        cf.lines()
            .any(|l| l.starts_with("COPY") && l.contains("machine-info")),
        "control: the machine-info file is copied:\n{cf}"
    );
    let refine = session(&snap);
    let views = &refine.decisions().unmanaged_withheld;
    assert!(
        views.iter().any(|v| v.path == MACHINE_ID
            && matches!(
                &v.withheld,
                inspectah_refine::projection::WithheldView::Withheld { why, .. }
                    if why == "MachineId"
            )),
        "{views:?}"
    );
    assert!(
        !views.iter().any(|v| v.path == MACHINE_INFO),
        "control: {views:?}"
    );
}

// ---------------------------------------------------------------------
// § 10.2 rule 5 "A mask follows the package check" (T34).
// ---------------------------------------------------------------------

/// T34 through the scan and refine: a vendor unit masked on a host whose
/// package `maskpkg` owns it, scanned against a base image that does not
/// ship the package. The scan export carries the mask. Excluding the
/// package through `RefineSession` gives no `mask`, an `# Omitted:` line
/// with the package reason, and the row's `declined` with the same reason;
/// including it again brings the mask back. A masked protected unit whose
/// package is excluded keeps rule 3's FIXME.
#[test]
fn t34_a_mask_follows_its_package_through_the_scan_and_refine() {
    const UNIT: &str = "maskme.service";
    const OWNER: &str = "maskpkg";
    let snap = scan(owned_unit_host(UNIT, OWNER, "masked", VENDOR_SERVICE));
    assert_eq!(change(&snap, UNIT).current_state(), UnitState::Masked);
    assert!(
        matches!(
            fragment_of(&snap, UNIT),
            UnitFragment::Vendor { owning_package: Some(p), .. } if p == OWNER
        ),
        "{:?}",
        fragment_of(&snap, UNIT)
    );
    let scanned = containerfile_in(&render_export(&snap));
    assert!(acts(&scanned, "mask", UNIT), "presence: {scanned}");

    let omitted_line = format!("# Omitted: {UNIT} (package '{OWNER}' was excluded)");
    let mut refine = session(&snap);
    refine
        .apply(set_include(package_item(OWNER), false))
        .expect("excluding the package is a decision the session takes");
    let excluded = containerfile_in(&export_session(&refine));
    assert_eq!(
        omitted(&excluded, UNIT),
        Some(omitted_line.as_str()),
        "{excluded}"
    );
    assert!(in_no_systemctl_instruction(&excluded, UNIT), "{excluded}");
    let declined = declined_in(&refine, UNIT).expect("the row is declined");
    assert_eq!(declined.reason, "PackageExcluded", "{declined:?}");
    assert_eq!(
        format!("# Omitted: {}", declined.text),
        omitted_line,
        "{declined:?}"
    );

    refine
        .apply(set_include(package_item(OWNER), true))
        .expect("including the package again is a decision the session takes");
    let included = containerfile_in(&export_session(&refine));
    assert!(acts(&included, "mask", UNIT), "{included}");
    assert!(declined_in(&refine, UNIT).is_none());

    const PROTECTED: &str = "bootc-fetch-apply-updates.service";
    let snap = scan(owned_unit_host(
        PROTECTED,
        "bootc",
        "masked",
        VENDOR_SERVICE,
    ));
    let mut refine = session(&snap);
    refine
        .apply(set_include(package_item("bootc"), false))
        .expect("excluding the package is a decision the session takes");
    let cf = containerfile_in(&export_session(&refine));
    assert_eq!(
        fixme_for(&cf, PROTECTED),
        Some(protected_fixme(PROTECTED, "masked").as_str()),
        "{cf}"
    );
    assert!(omitted(&cf, PROTECTED).is_none(), "{cf}");
}

// ---------------------------------------------------------------------
// A generated name's lock: the view agrees with what the session enforces.
// ---------------------------------------------------------------------

/// The scan of the [`CRON_FILE`] host with a second record of the
/// generated name `cron-backup`, as a second cron file of the same name
/// gives one, and only the record at `locked_at` locked (none for `None`).
fn two_records_of_one_name(locked_at: Option<usize>) -> InspectionSnapshot {
    let mut snap = scan(with_cron_backup(UnitHost::new().preset("disable *")));
    let units = &mut snap
        .scheduled_tasks
        .as_mut()
        .expect("a scheduled section")
        .generated_timer_units;
    assert_eq!(units.len(), 1, "presence: one generated record");
    let mut second = units[0].clone();
    second.source_path = "/var/spool/cron/backup".into();
    units.push(second);
    for (i, unit) in units.iter_mut().enumerate() {
        unit.locked = Some(i) == locked_at;
    }
    snap
}

/// Two records of one generated name with only one of them locked, in
/// both orders. The session enforces a lock when any record of the name
/// is locked: `SetInclude(true)` on the name is a silent no-op
/// (`RefineSession::apply`), so after an exclusion the row cannot be
/// included again. The generated-timer row in refine's decisions must say
/// `locked` too, so the UI offers no toggle the server ignores. Control:
/// with no record locked, the same sequence includes the row again and
/// the view reports it unlocked.
#[test]
fn a_generated_name_is_shown_locked_when_any_of_its_records_is() {
    for locked_at in [Some(1), Some(0), None] {
        let snap = two_records_of_one_name(locked_at);
        let mut refine = session(&snap);
        let row = |refine: &RefineSession| {
            refine
                .decisions()
                .generated_timers
                .iter()
                .find(|row| row.name == CRON_BACKUP)
                .cloned()
                .expect("the generated-timer row")
        };
        let shown_locked = row(&refine).locked;
        refine
            .apply(set_include(generated_item(CRON_BACKUP), false))
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert!(!row(&refine).include, "presence: the exclusion applies");
        refine
            .apply(set_include(generated_item(CRON_BACKUP), true))
            .unwrap_or_else(|e| panic!("{e:?}"));
        let enforced_locked = !row(&refine).include;
        assert_eq!(
            enforced_locked,
            locked_at.is_some(),
            "{locked_at:?} locked: the session keeps the name excluded only when a record is locked"
        );
        assert_eq!(
            shown_locked, enforced_locked,
            "{locked_at:?} locked: the view agrees with the session"
        );
        assert_eq!(
            row(&refine).locked,
            locked_at.is_some(),
            "{locked_at:?} locked"
        );
    }
}

// ---------------------------------------------------------------------
// T24 and T25 (k): install drop-ins and rule 5's drop-in half (§ 6.3,
// § 7.2, § 10.2 "Install files").
// ---------------------------------------------------------------------

/// The package that owns the vendor units of [`install_host`].
const INSTALL_PACKAGE: &str = "apppkg";

/// A vendor `unit` owned by [`INSTALL_PACKAGE`] whose own file holds
/// `vendor_content`, enabled against a `disable` preset, on a host scanned
/// against a base image so refine keeps the package included.
fn install_host(unit: &str, vendor_content: &str) -> UnitHost {
    UnitHost::new()
        .base_package("glibc")
        .package(INSTALL_PACKAGE)
        .unit(unit, "enabled")
        .preset("disable *")
        .vendor_unit(unit, vendor_content, Some(INSTALL_PACKAGE))
}

/// A unit file with no `[Install]` section.
const NO_INSTALL_UNIT: &str = "[Service]\nExecStart=/usr/bin/app\n";
/// A unit file wanted by `multi-user.target`.
const WANTED_UNIT: &str =
    "[Service]\nExecStart=/usr/bin/app\n\n[Install]\nWantedBy=multi-user.target\n";
/// A drop-in that sets an `[Install]` section.
const INSTALL_DROP_IN: &str = "[Install]\nWantedBy=multi-user.target\n";
/// A drop-in that changes no `[Install]` key.
const SERVICE_DROP_IN: &str = "[Service]\nNice=5\n";

/// T24: a `WantedBy=` that comes only from an `/etc` drop-in is carried,
/// with the drop-in in `install_drop_ins`; with that drop-in excluded
/// through refine, `InstallEvidenceChanged` naming it and no enable; with
/// it included again, the enable returns.
#[test]
fn t24_a_wanted_by_from_a_drop_in_is_carried_with_that_drop_in() {
    const DROP_IN: &str = "/etc/systemd/system/app.service.d/10-install.conf";
    let snap = scan(install_host("app.service", NO_INSTALL_UNIT).file(DROP_IN, INSTALL_DROP_IN));
    assert_eq!(change(&snap, "app.service").install_drop_ins, [DROP_IN]);

    let scanned = containerfile_in(&render_export(&snap));
    assert!(acts(&scanned, "enable", "app.service"), "{scanned}");

    let drop_in = ItemId::DropIn {
        path: DROP_IN.into(),
    };
    let excluded = containerfile_in(&exported_after(&snap, &[(drop_in.clone(), false)]));
    assert!(
        in_no_systemctl_instruction(&excluded, "app.service"),
        "{excluded}"
    );
    assert!(
        omitted(&excluded, "app.service").is_some_and(|l| l.ends_with(&evidence_changed(DROP_IN))),
        "{excluded}"
    );

    let restored = containerfile_in(&exported_after(
        &snap,
        &[(drop_in.clone(), false), (drop_in, true)],
    ));
    assert!(acts(&restored, "enable", "app.service"), "{restored}");
}

/// The collector's hidden-name rule, carried through the render and
/// refine: an `/etc` `.#override.conf` holding `[Install]` (an editor's
/// lock file) is not recorded and, although the export copies it, is no
/// install file there either, so the unit's enable is carried on both
/// routes, with the file excluded or not. Control: the visible
/// `override.conf` is recorded, carried with the enable, and excluding it
/// through refine leaves the enable out with `InstallEvidenceChanged`.
#[test]
fn hidden_drop_in_names_are_not_install_files_through_both_exports() {
    const HIDDEN: &str = "/etc/systemd/system/app.service.d/.#override.conf";
    const VISIBLE: &str = "/etc/systemd/system/app.service.d/override.conf";

    let control = scan(install_host("app.service", WANTED_UNIT).file(VISIBLE, INSTALL_DROP_IN));
    assert_eq!(change(&control, "app.service").install_drop_ins, [VISIBLE]);
    for (route, containerfile) in both_routes(&control) {
        assert!(
            acts(&containerfile, "enable", "app.service"),
            "control {route}:\n{containerfile}"
        );
    }
    let excluded = containerfile_in(&exported_after(&control, &[(drop_in_item(VISIBLE), false)]));
    assert!(
        omitted(&excluded, "app.service").is_some_and(|l| l.ends_with(&evidence_changed(VISIBLE))),
        "the visible file is guarded:\n{excluded}"
    );

    let snap = scan(install_host("app.service", WANTED_UNIT).file(HIDDEN, INSTALL_DROP_IN));
    assert!(change(&snap, "app.service").install_drop_ins.is_empty());
    let scanned = render_export(&snap);
    assert!(
        staged_at(&scanned, HIDDEN),
        "the export copies the hidden file"
    );
    let refined = export_session(&session(&snap));
    assert!(staged_at(&refined, HIDDEN), "refine copies it too");
    for (route, tree) in [("scan", &scanned), ("refine", &refined)] {
        let containerfile = containerfile_in(tree);
        assert!(
            acts(&containerfile, "enable", "app.service"),
            "{route}:\n{containerfile}"
        );
    }
    let without = containerfile_in(&exported_after(&snap, &[(drop_in_item(HIDDEN), false)]));
    assert!(acts(&without, "enable", "app.service"), "{without}");
}

/// T24 and T25 (k) first half: an `/etc` drop-in with no `[Install]` and
/// the file name of a vendor drop-in in the unit's own `.d` is an install
/// file, since it hides the vendor one's section; the vendor name is in
/// `vendor_drop_ins`. With the `/etc` file included the enable is carried;
/// excluded, `InstallEvidenceChanged`. An `/etc` `[Service]` drop-in under
/// a name of its own is not an install file. The unit file names its own
/// link, so the unit has an install state with the vendor drop-in hidden.
#[test]
fn t25_k_an_etc_file_named_like_a_vendor_drop_in_is_an_install_file() {
    const VENDOR: &str = "/usr/lib/systemd/system/bundle.service.d/10-members.conf";
    const HIDING: &str = "/etc/systemd/system/bundle.service.d/10-members.conf";
    const OWN: &str = "/etc/systemd/system/bundle.service.d/20-limits.conf";
    let snap = scan(
        install_host("bundle.service", WANTED_UNIT)
            .file(
                VENDOR,
                "[Install]\nWantedBy=multi-user.target\nAlso=helper.service\n",
            )
            .file(HIDING, SERVICE_DROP_IN)
            .file(OWN, SERVICE_DROP_IN),
    );
    let recorded = change(&snap, "bundle.service");
    assert_eq!(recorded.install_drop_ins, [HIDING]);
    assert_eq!(recorded.vendor_drop_ins, ["10-members.conf"]);

    let scanned = containerfile_in(&render_export(&snap));
    assert!(acts(&scanned, "enable", "bundle.service"), "{scanned}");

    let excluded = containerfile_in(&exported_after(
        &snap,
        &[(
            ItemId::DropIn {
                path: HIDING.into(),
            },
            false,
        )],
    ));
    assert!(
        in_no_systemctl_instruction(&excluded, "bundle.service"),
        "{excluded}"
    );
    assert!(
        omitted(&excluded, "bundle.service")
            .is_some_and(|l| l.ends_with(&evidence_changed(HIDING))),
        "{excluded}"
    );

    // The own-named override alone excluded changes nothing.
    let own_excluded = containerfile_in(&exported_after(
        &snap,
        &[(ItemId::DropIn { path: OWN.into() }, false)],
    ));
    assert!(
        acts(&own_excluded, "enable", "bundle.service"),
        "{own_excluded}"
    );
}

/// T24: the type-wide directory and a dash-prefix directory are never
/// read for `[Install]`. A vendor drop-in holding `WantedBy=` there is not
/// in `vendor_drop_ins`; an `/etc` drop-in with `[Install]` there is not in
/// `install_drop_ins`, and staging it beside the carried enable leaves the
/// enable carried; an `/etc` file in `service.d` named like a vendor
/// drop-in in the unit's own `.d` is not an install file and does not hide
/// it, which stays in `vendor_drop_ins`.
#[test]
fn t24_type_wide_and_dash_prefix_drop_ins_are_not_install_files() {
    let hosts = [
        (
            "a-b.service",
            "/usr/lib/systemd/system/service.d/10-vendor.conf",
            "/etc/systemd/system/service.d/10-site.conf",
        ),
        (
            "a-b.service",
            "/usr/lib/systemd/system/a-.service.d/10-vendor.conf",
            "/etc/systemd/system/a-.service.d/10-site.conf",
        ),
    ];
    for (unit, vendor, etc) in hosts {
        let snap = scan(
            install_host(
                unit,
                "[Service]\nExecStart=/usr/bin/ab\n[Install]\nWantedBy=multi-user.target\n",
            )
            .file(vendor, INSTALL_DROP_IN)
            .file(etc, INSTALL_DROP_IN),
        );
        let recorded = change(&snap, unit);
        assert!(
            recorded.vendor_drop_ins.is_empty(),
            "{vendor}: {:?}",
            recorded.vendor_drop_ins
        );
        assert!(
            recorded.install_drop_ins.is_empty(),
            "{etc}: {:?}",
            recorded.install_drop_ins
        );

        let as_config = snap.config.as_ref().is_some_and(|c| {
            c.files
                .iter()
                .any(|f| (f.path == etc[1..] || f.path == etc) && f.disposition.is_included())
        });
        let as_drop_in = snap.units.as_ref().is_some_and(|u| {
            u.drop_ins
                .iter()
                .any(|d| d.path == etc && d.disposition.is_included())
        });
        assert!(
            as_config || as_drop_in,
            "{etc} is collected, included, and so staged"
        );
        let scanned = containerfile_in(&render_export(&snap));
        assert!(acts(&scanned, "enable", unit), "{etc}:\n{scanned}");
    }

    // A type-wide /etc file named like a vendor drop-in in the unit's own
    // .d hides nothing there.
    const OWN_VENDOR: &str = "/usr/lib/systemd/system/a-b.service.d/10-members.conf";
    const TYPEWIDE: &str = "/etc/systemd/system/service.d/10-members.conf";
    let snap = scan(
        install_host("a-b.service", NO_INSTALL_UNIT)
            .file(OWN_VENDOR, INSTALL_DROP_IN)
            .file(TYPEWIDE, SERVICE_DROP_IN),
    );
    let recorded = change(&snap, "a-b.service");
    assert_eq!(recorded.vendor_drop_ins, ["10-members.conf"]);
    assert!(
        recorded.install_drop_ins.is_empty(),
        "{:?}",
        recorded.install_drop_ins
    );
    let scanned = containerfile_in(&render_export(&snap));
    assert!(acts(&scanned, "enable", "a-b.service"), "{scanned}");
}

/// T24 resets, through the scan: a drop-in that resets `WantedBy=` and
/// then assigns it is an install drop-in like any other. An empty
/// `RequiredBy=` after `RequiredBy=a.target` makes the section
/// undetermined: no state change and no enable (its advisory comes with
/// the link findings). Control: without the drop-in, the unit is a state
/// change and enabled.
#[test]
fn t24_resets_through_the_scan() {
    const RESET: &str = "/etc/systemd/system/app.service.d/10-reset.conf";
    let vendor = "[Service]\nExecStart=/usr/bin/app\n[Install]\nWantedBy=multi-user.target\n";
    let snap = scan(
        install_host("app.service", vendor)
            .file(RESET, "[Install]\nWantedBy=\nWantedBy=extra.target\n"),
    );
    assert_eq!(change(&snap, "app.service").install_drop_ins, [RESET]);

    let required = "[Service]\nExecStart=/usr/bin/app\n[Install]\nRequiredBy=a.target\n";
    let control = scan(install_host("app.service", required));
    assert_eq!(
        change(&control, "app.service").current_state(),
        UnitState::Enabled
    );
    let scanned = containerfile_in(&render_export(&control));
    assert!(acts(&scanned, "enable", "app.service"), "{scanned}");

    let snap = scan(install_host("app.service", required).file(RESET, "[Install]\nRequiredBy=\n"));
    assert!(
        snap.units
            .as_ref()
            .expect("a units section")
            .drop_ins
            .iter()
            .any(|d| d.path == RESET),
        "the drop-in is collected"
    );
    assert!(
        !snap
            .units
            .as_ref()
            .expect("a units section")
            .state_changes
            .iter()
            .any(|c| c.unit().name().as_str() == "app.service"),
        "no state change"
    );
    let scanned = containerfile_in(&render_export(&snap));
    assert!(
        in_no_systemctl_instruction(&scanned, "app.service"),
        "{scanned}"
    );
}

/// T24 with Review focus 3: a drop-in whose lines end in a lone CR is read
/// as systemd reads it, so its `[Install]` makes it an install file. It is
/// in `install_drop_ins`, and with it excluded, `InstallEvidenceChanged`
/// naming it and no enable.
#[test]
fn t24_a_lone_cr_install_drop_in_is_an_install_file() {
    const DROP_IN: &str = "/etc/systemd/system/app.service.d/10-install.conf";
    let snap = scan(
        install_host("app.service", NO_INSTALL_UNIT)
            .file(DROP_IN, "[Install]\rWantedBy=multi-user.target\r"),
    );
    assert_eq!(change(&snap, "app.service").install_drop_ins, [DROP_IN]);
    let scanned = containerfile_in(&render_export(&snap));
    assert!(acts(&scanned, "enable", "app.service"), "{scanned}");

    let excluded = containerfile_in(&exported_after(
        &snap,
        &[(
            ItemId::DropIn {
                path: DROP_IN.into(),
            },
            false,
        )],
    ));
    assert!(
        in_no_systemctl_instruction(&excluded, "app.service"),
        "{excluded}"
    );
    assert!(
        omitted(&excluded, "app.service").is_some_and(|l| l.ends_with(&evidence_changed(DROP_IN))),
        "{excluded}"
    );
}

// ---------------------------------------------------------------------
// Hand-made links and undetermined installs (§ 6.6, § 7.3, § 10.9, § 11.1;
// T22 to T24, T40).
// ---------------------------------------------------------------------

/// A service with no `[Install]` section.
const NO_INSTALL: &str = "[Service]\nExecStart=/bin/true\n";
/// A service wanted by `multi-user.target`.
const WANTS_MULTI_USER: &str =
    "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\n";
/// The second line of § 10.9's unit-level block for `unit`.
fn undetermined_second_line(unit: &str) -> String {
    format!(
        "#   The image's links for {unit} can differ from the source host's, including links \
         its package install made before these files are copied. Check them with systemctl \
         is-enabled in the image."
    )
}

/// The second line of § 10.9's unit-level block for `unit` when the record
/// has no install file: no files are copied, so it says nothing about a
/// copy (Mark Russell, 2026-11-15).
fn undetermined_no_file_second_line(unit: &str) -> String {
    format!(
        "#   The image's links for {unit} can differ from the source host's, including links \
         its package install made. Check them with systemctl is-enabled in the image."
    )
}

/// The block in `containerfile` that starts with the line `first`: that
/// line and the `#   ` lines after it.
fn block_from(containerfile: &str, first: &str) -> Vec<String> {
    let mut lines = containerfile.lines().skip_while(|l| *l != first);
    let Some(head) = lines.next() else {
        return Vec::new();
    };
    std::iter::once(head)
        .chain(lines.take_while(|l| l.starts_with("#   ")))
        .map(str::to_owned)
        .collect()
}

/// The link findings of a scanned snapshot.
fn links_of(snap: &InspectionSnapshot) -> &[ManualLink] {
    &snap.units.as_ref().expect("a units section").manual_links
}

/// Whether a refine row, serialized, has an `include` field.
fn has_include(row: serde_json::Value) -> bool {
    row.get("include").is_some()
}

fn name(unit: &str) -> UnitName {
    UnitName::try_from(unit.to_owned()).expect("a unit name")
}

/// A drop-in in a directory systemd ranks above `/etc/systemd/system`
/// hides an `/etc` drop-in of its file name (Mark Russell, 2026-11-15):
/// `systemctl set-property`'s `50-CPUQuota.conf` under
/// `/etc/systemd/system.control/app.service.d/` over an `/etc`
/// `50-CPUQuota.conf` holding `WantedBy=graphical.target`. The hidden
/// `[Install]` is not read, so the unit, with its `multi-user.target` link,
/// is `Enabled` at its preset with no finding. Control: the control
/// directory's drop-in under another name hides nothing, the `/etc`
/// `[Install]` is read, and the unit is a partial install with no state.
#[test]
fn a_drop_in_ranked_above_etc_hides_an_etc_drop_in_of_its_name() {
    const ETC_DROP_IN: &str = "/etc/systemd/system/app.service.d/50-CPUQuota.conf";
    let host = |control_name: &str| {
        wants_link(
            diff_host(
                DIFF_UNIT,
                "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\n",
                "enabled",
            ),
            "multi-user.target",
            DIFF_UNIT,
        )
        .file(ETC_DROP_IN, "[Install]\nWantedBy=graphical.target\n")
        .file(
            &format!("/etc/systemd/system.control/app.service.d/{control_name}"),
            "[Service]\nCPUQuota=20%\n",
        )
    };
    let inventory = |snap: &InspectionSnapshot| {
        snap.units
            .as_ref()
            .expect("a units section")
            .inventory
            .iter()
            .find(|e| e.unit.as_str() == DIFF_UNIT)
            .map(|e| e.state)
    };
    let warnings_of = |snap: &InspectionSnapshot| -> Vec<String> {
        snap.warnings
            .iter()
            .filter(|w| w.message.contains(DIFF_UNIT) || w.message.contains("system.control"))
            .map(|w| w.message.clone())
            .collect()
    };
    let causes = |snap: &InspectionSnapshot| {
        links_of(snap)
            .iter()
            .map(|l| (l.parent.as_str().to_owned(), l.cause))
            .collect::<Vec<_>>()
    };

    let control = scan(host("50-MemoryMax.conf"));
    assert!(
        warnings_of(&control).is_empty(),
        "control: no search-path refusal: {:?}",
        warnings_of(&control)
    );
    assert_eq!(inventory(&control), None, "control: no state");
    assert_eq!(
        causes(&control),
        [(
            "multi-user.target".to_owned(),
            ManualLinkCause::PartialInstall
        )],
        "control: the /etc [Install] is read"
    );

    let hidden = scan(host("50-CPUQuota.conf"));
    assert!(
        warnings_of(&hidden).is_empty(),
        "no search-path refusal: {:?}",
        warnings_of(&hidden)
    );
    assert_eq!(inventory(&hidden), Some(UnitState::Enabled));
    assert_eq!(causes(&hidden), [], "the hidden [Install] is not read");
    assert_eq!(changes_named(&hidden, DIFF_UNIT), 0, "at its preset");
    assert!(
        hidden
            .units
            .as_ref()
            .expect("a units section")
            .undetermined_installs
            .is_empty()
    );
}

/// The `/etc` drop-in of the hiding cases, adding `graphical.target`.
const HIDDEN_DROP_IN: &str = "/etc/systemd/system/app.service.d/50-CPUQuota.conf";

/// The drop-in of [`HIDDEN_DROP_IN`]'s file name under `dir`, a directory
/// systemd ranks above `/etc/systemd/system`.
fn hiding_path(dir: &str) -> String {
    format!("{dir}/app.service.d/50-CPUQuota.conf")
}

/// The worked case of a drop-in ranked above `/etc` that hides an `/etc`
/// install drop-in: `app.service` from its package, `WantedBy=
/// multi-user.target` and linked, under a `disable` preset, with
/// [`HIDDEN_DROP_IN`] adding `WantedBy=graphical.target` and a drop-in of
/// its name under `dir` hiding it. The host reads `Enabled`, so the scan
/// writes a row whose `enable` in an image without the hiding file would
/// make a `graphical.target` link the host does not have.
fn hiding_host(dir: &str) -> UnitHost {
    wants_link(
        UnitHost::new()
            .base_package("glibc")
            .package(INSTALL_PACKAGE)
            .unit(DIFF_UNIT, "enabled")
            .preset(&format!("disable {DIFF_UNIT}"))
            .vendor_unit(
                DIFF_UNIT,
                "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\n",
                Some(INSTALL_PACKAGE),
            ),
        "multi-user.target",
        DIFF_UNIT,
    )
    .file(HIDDEN_DROP_IN, "[Install]\nWantedBy=graphical.target\n")
    .file(&hiding_path(dir), "[Service]\nCPUQuota=20%\n")
}

/// `snap` with every config entry at `path` given `disposition`, as the
/// scan route renders an operator's decision.
fn with_config_disposition(
    snap: &InspectionSnapshot,
    path: &str,
    disposition: FindingKind,
) -> InspectionSnapshot {
    let mut decided = snap.clone();
    let entries: Vec<_> = decided
        .config
        .as_mut()
        .expect("a config section")
        .files
        .iter_mut()
        .filter(|f| f.path == path)
        .collect();
    assert!(!entries.is_empty(), "no config entry at {path}");
    for entry in entries {
        entry.disposition = disposition.clone();
    }
    decided
}

/// `app.service`'s command is withheld on both routes and in refine's
/// view, with rule 5's `InstallEvidenceChanged` naming `path`. The scan
/// route renders `snap`; the refine route applies `refine_ops` to a
/// session over it.
fn assert_hiding_withholds(snap: &InspectionSnapshot, refine_ops: &[(ItemId, bool)], path: &str) {
    assert_eq!(
        declined_in(&session_after(snap, refine_ops), DIFF_UNIT),
        Some(evidence_declined(DIFF_UNIT, path)),
        "the refine row"
    );
    for (route, containerfile) in [
        ("scan", containerfile_in(&render_export(snap))),
        (
            "refine",
            containerfile_in(&exported_after(snap, refine_ops)),
        ),
    ] {
        assert!(
            in_no_systemctl_instruction(&containerfile, DIFF_UNIT),
            "{route}:\n{containerfile}"
        );
        assert!(
            omitted(&containerfile, DIFF_UNIT)
                .is_some_and(|l| l.ends_with(&evidence_changed(path))),
            "{route}:\n{containerfile}"
        );
    }
}

/// A drop-in under `/run/systemd/system.control` hides an `/etc` drop-in
/// holding `[Install]`, and the unit's `enable` depends on that: the
/// hiding file is install evidence beside the hidden one, and since a
/// `/run` file is never staged, rule 5 withholds the `enable` with
/// `InstallEvidenceChanged` naming it, on both routes and in refine's
/// view.
#[test]
fn a_runtime_drop_in_hiding_an_etc_install_drop_in_withholds_the_command() {
    let hiding = hiding_path("/run/systemd/system.control");
    let snap = scan(hiding_host("/run/systemd/system.control"));
    let row = change(&snap, DIFF_UNIT);
    assert_eq!(row.current_state(), UnitState::Enabled);
    assert_eq!(row.default_state, Some(PresetDefault::Disable));
    assert_hiding_withholds(&snap, &[], &hiding);
    assert_eq!(
        row.install_drop_ins,
        [HIDDEN_DROP_IN.to_owned(), hiding.clone()],
        "the hiding file is recorded beside the hidden one"
    );
}

/// The same with the hiding drop-in under the persistent
/// `/etc/systemd/system.control`: the configuration export carries it as a
/// config entry. Excluded, on either route, the `enable` is withheld with
/// `InstallEvidenceChanged` naming it. Included (the control), it is
/// staged and the `enable` is carried.
#[test]
fn a_persistent_drop_in_hiding_an_etc_install_drop_in_is_carried_only_when_staged() {
    let hiding = hiding_path("/etc/systemd/system.control");
    let snap = scan(hiding_host("/etc/systemd/system.control"));
    assert_hiding_withholds(
        &with_config_disposition(&snap, &hiding, FindingKind::excluded()),
        &[(config_item(&hiding), false)],
        &hiding,
    );
    assert_eq!(
        change(&snap, DIFF_UNIT).install_drop_ins,
        [hiding.clone(), HIDDEN_DROP_IN.to_owned()],
        "the hiding file is recorded beside the hidden one"
    );

    assert_eq!(declined_in(&session(&snap), DIFF_UNIT), None);
    for (route, tree) in [
        ("scan", render_export(&snap)),
        ("refine", export_session(&session(&snap))),
    ] {
        assert!(staged_at(&tree, &hiding), "{route}: the hiding file");
        assert!(staged_at(&tree, HIDDEN_DROP_IN), "{route}: the hidden file");
        let containerfile = containerfile_in(&tree);
        assert!(
            acts(&containerfile, "enable", DIFF_UNIT),
            "{route}:\n{containerfile}"
        );
        assert!(
            omitted(&containerfile, DIFF_UNIT).is_none(),
            "{route}:\n{containerfile}"
        );
    }
}

/// The fleet merge unions a row's `install_drop_ins` across hosts (§ 12),
/// so a hiding drop-in any host recorded has to be staged. One host with
/// the persistent hiding file and one with the `/run` one: the merged row
/// records all three files, and with the persistent file's config entry
/// included (the merge leaves a one-host entry excluded) the `enable` is
/// withheld naming the `/run` file. Control: both hosts with the
/// persistent one, which the export stages: the `enable` is carried.
#[test]
fn a_fleet_row_needs_every_hosts_hiding_drop_in_staged() {
    let persistent = hiding_path("/etc/systemd/system.control");
    let runtime = hiding_path("/run/systemd/system.control");
    let mixed = merged(
        named(hiding_host("/etc/systemd/system.control"), "host-a"),
        named(hiding_host("/run/systemd/system.control"), "host-b"),
    );
    let included = with_config_disposition(&mixed, &persistent, FindingKind::included());
    let tree = render_export(&included);
    assert!(
        staged_at(&tree, &persistent),
        "the persistent file is staged"
    );
    let containerfile = containerfile_in(&tree);
    assert!(
        in_no_systemctl_instruction(&containerfile, DIFF_UNIT),
        "{containerfile}"
    );
    assert!(
        omitted(&containerfile, DIFF_UNIT)
            .is_some_and(|l| l.ends_with(&evidence_changed(&runtime))),
        "{containerfile}"
    );
    assert_eq!(
        change(&mixed, DIFF_UNIT).install_drop_ins,
        [
            persistent.clone(),
            HIDDEN_DROP_IN.to_owned(),
            runtime.clone()
        ]
    );

    let same = merged(
        named(hiding_host("/etc/systemd/system.control"), "host-a"),
        named(hiding_host("/etc/systemd/system.control"), "host-b"),
    );
    let tree = render_export(&same);
    assert!(staged_at(&tree, &persistent), "the hiding file is staged");
    let containerfile = containerfile_in(&tree);
    assert!(acts(&containerfile, "enable", DIFF_UNIT), "{containerfile}");
    assert!(
        omitted(&containerfile, DIFF_UNIT).is_none(),
        "{containerfile}"
    );
}

/// A unit with some of the links its `[Install]` names and not others,
/// and two links under `/etc` its section does not name, a `.wants` link
/// and an alias (Mark Russell, 2026-11-15): the named link is
/// `PartialInstall`, with its "Present:" line, and each unnamed one is
/// `NotInInstall`, with the hand-made text, in the snapshot, refine's
/// `manual_links` and both exports. The unit has no state change.
#[test]
fn a_partial_unit_reports_a_link_its_section_does_not_name_as_hand_made() {
    const UNIT: &str = "part.service";
    let snap = scan(
        UnitHost::new()
            .unit(UNIT, "enabled")
            .vendor_unit(
                UNIT,
                "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target graphical.target\n",
                None,
            )
            .link(
                "/etc/systemd/system/multi-user.target.wants/part.service",
                "/usr/lib/systemd/system/part.service",
            )
            .link(
                "/etc/systemd/system/rescue.target.wants/part.service",
                "/usr/lib/systemd/system/part.service",
            )
            .link(
                "/etc/systemd/system/part-alias.service",
                "/usr/lib/systemd/system/part.service",
            ),
    );
    assert_eq!(changes_named(&snap, UNIT), 0);
    let mut found: Vec<_> = links_of(&snap)
        .iter()
        .map(|l| (l.relation, l.parent.as_str(), l.unit.as_str(), l.cause))
        .collect();
    found.sort_by_key(|(_, parent, ..)| *parent);
    assert_eq!(
        found,
        [
            (
                LinkRelation::Wants,
                "multi-user.target",
                UNIT,
                ManualLinkCause::PartialInstall
            ),
            (
                LinkRelation::Alias,
                "part-alias.service",
                UNIT,
                ManualLinkCause::NotInInstall
            ),
            (
                LinkRelation::Wants,
                "rescue.target",
                UNIT,
                ManualLinkCause::NotInInstall
            ),
        ]
    );
    let blocks = [
        (
            "multi-user.target",
            [
                "# Advisory: part.service has some of the links its [Install] section names and \
                 not others, so neither enable nor disable reproduces the source host. Not \
                 carried. Present: multi-user.target.wants/part.service."
                    .to_owned(),
                "#   To carry it, add after the configuration COPY: RUN systemctl add-wants -- \
                 'multi-user.target' 'part.service'"
                    .to_owned(),
            ],
        ),
        (
            "rescue.target",
            [
                "# Advisory: rescue.target.wants/part.service is a link made by hand on the \
                 source host (the unit's [Install] section does not name rescue.target). Not \
                 carried."
                    .to_owned(),
                "#   To carry it, add after the configuration COPY: RUN systemctl add-wants -- \
                 'rescue.target' 'part.service'"
                    .to_owned(),
            ],
        ),
        (
            "part-alias.service",
            [
                "# Advisory: part-alias.service is a link to part.service made by hand on the \
                 source host (the unit's [Install] section does not name part-alias.service). \
                 Not carried."
                    .to_owned(),
                "#   To carry it, make the link in the Containerfile after the configuration \
                 COPY, or add Alias=part-alias.service in a drop-in for part.service and run \
                 systemctl enable on part.service after that COPY, which also makes every other \
                 link its [Install] section and Also= name: check those first."
                    .to_owned(),
            ],
        ),
    ];
    let refine = session(&snap);
    let rows = &refine.decisions().manual_links;
    assert_eq!(rows.len(), 3, "{rows:?}");
    for (parent, expected) in &blocks {
        let row = rows
            .iter()
            .find(|r| r.parent.as_str() == *parent)
            .unwrap_or_else(|| panic!("{parent}: a refine row: {rows:?}"));
        assert_eq!(&row.advisories, expected, "{parent}: the refine row");
    }
    for (route, containerfile) in both_routes(&snap) {
        for (parent, expected) in &blocks {
            assert_eq!(
                block_from(&containerfile, &expected[0]),
                expected,
                "{route} {parent}:\n{containerfile}"
            );
        }
        assert!(
            !containerfile.contains("Present: rescue.target")
                && !containerfile.contains("Present: part-alias.service"),
            "{route}: an unnamed link is not Present:\n{containerfile}"
        );
        assert!(
            only_comments_name(&containerfile, UNIT),
            "{route}:\n{containerfile}"
        );
    }
}

/// T23: a `multi-user.target.wants/backup.service` link to a unit with no
/// `[Install]`. The `StaticUnit` link and its refine row exist first; then
/// the advisory holds § 10.9's text and the `add-wants` line on the scan
/// and refine routes, no instruction names the unit, and the row has no
/// include field.
#[test]
fn t23_a_link_to_a_unit_with_no_install_section_is_advised_with_its_command() {
    const UNIT: &str = "backup.service";
    let snap = scan(
        UnitHost::new()
            .unit(UNIT, "static")
            .vendor_unit(UNIT, NO_INSTALL, None)
            .link(
                "/etc/systemd/system/multi-user.target.wants/backup.service",
                "/usr/lib/systemd/system/backup.service",
            ),
    );
    assert_eq!(
        links_of(&snap)
            .iter()
            .map(|l| (l.relation, l.parent.as_str(), l.unit.as_str(), l.cause))
            .collect::<Vec<_>>(),
        [(
            LinkRelation::Wants,
            "multi-user.target",
            UNIT,
            ManualLinkCause::StaticUnit
        )]
    );
    let refine = session(&snap);
    let rows = &refine.decisions().manual_links;
    assert_eq!(rows.len(), 1, "{rows:?}");
    let expected = [
        "# Advisory: multi-user.target.wants/backup.service is a link on the source host that \
         backup.service's [Install] section there does not make (it names no link: none, only \
         Also=, or a list a drop-in reset to empty). Not carried."
            .to_owned(),
        "#   To carry it, add after the configuration COPY: RUN systemctl add-wants -- \
         'multi-user.target' 'backup.service'"
            .to_owned(),
    ];
    assert_eq!(rows[0].advisories, expected, "the refine row");
    assert!(
        !has_include(serde_json::to_value(&rows[0]).expect("the row serializes")),
        "the row has no toggle"
    );
    for (route, containerfile) in both_routes(&snap) {
        assert_eq!(
            block_from(&containerfile, &expected[0]),
            expected,
            "{route}:\n{containerfile}"
        );
        assert!(
            only_comments_name(&containerfile, UNIT),
            "{route}:\n{containerfile}"
        );
    }
}

/// T23's table on § 10.9's renderer: a `.requires` link gets the
/// `add-requires` line; a link of a section with a specifier the
/// `Undetermined` text naming it in words and the `add-wants` line; a
/// `systemd-fsck@` instance the device text and no command line; a
/// hand-made alias the alias wording and no command line.
#[test]
fn t23_link_advisory_table() {
    let link = |relation, parent: &str, unit: &str, cause| ManualLink {
        relation,
        parent: name(parent),
        unit: name(unit),
        cause,
        install_files: Vec::new(),
        aggregate: None,
    };
    let fsck = r"systemd-fsck@dev-disk-by\x2duuid-0a1b.service";
    let cases = [
        (
            link(
                LinkRelation::Requires,
                "a.target",
                "backup.service",
                ManualLinkCause::StaticUnit,
            ),
            vec![
                "# Advisory: a.target.requires/backup.service is a link on the source host that \
                 backup.service's [Install] section there does not make (it names no link: \
                 none, only Also=, or a list a drop-in reset to empty). Not carried."
                    .to_owned(),
                "#   To carry it, add after the configuration COPY: RUN systemctl add-requires \
                 -- 'a.target' 'backup.service'"
                    .to_owned(),
            ],
        ),
        (
            link(
                LinkRelation::Wants,
                "multi-user.target",
                "host.service",
                ManualLinkCause::Undetermined {
                    reason: InstallUnknown::Specifier,
                },
            ),
            vec![
                "# Advisory: multi-user.target.wants/host.service is not carried: the unit's \
                 [Install] section cannot be relied on (a specifier inspectah does not expand)."
                    .to_owned(),
                "#   To carry it, add after the configuration COPY: RUN systemctl add-wants -- \
                 'multi-user.target' 'host.service'"
                    .to_owned(),
            ],
        ),
        (
            link(
                LinkRelation::Wants,
                "local-fs.target",
                fsck,
                ManualLinkCause::HostDevice,
            ),
            vec![format!(
                "# Advisory: {fsck} is tied to a device of the source host. Not carried."
            )],
        ),
        (
            link(
                LinkRelation::Alias,
                "backup-alias.service",
                "backup.service",
                ManualLinkCause::NotInInstall,
            ),
            vec![
                "# Advisory: backup-alias.service is a link to backup.service made by hand on \
                 the source host (the unit's [Install] section does not name \
                 backup-alias.service). Not carried."
                    .to_owned(),
                "#   To carry it, make the link in the Containerfile after the configuration \
                 COPY, or add Alias=backup-alias.service in a drop-in for backup.service and \
                 run systemctl enable on backup.service after that COPY, which also makes every \
                 other link its [Install] section and Also= name: check those first."
                    .to_owned(),
            ],
        ),
    ];
    let empty = InspectionSnapshot::new();
    let nothing_staged = render::configtree::staged_writes(&empty);
    for (link, expected) in cases {
        assert_eq!(
            render::unit_plan::manual_link_lines(&link, &nothing_staged),
            expected,
            "{link:?}"
        );
    }
}

/// T23: a local unit with `UpheldBy=multi-user.target` and its `.upholds`
/// link, through the units and config inspectors. The link is on the host
/// and the unit's own file is a config entry (the walk's positive
/// control); then no state change, no instruction names the unit, the
/// advisory holds the restart-loop text, and the link's path is in neither
/// `config.files` nor the staged tree.
#[test]
fn t23_an_upholds_link_is_advised_and_never_carried() {
    const UNIT: &str = "watch.service";
    const LINK: &str = "/etc/systemd/system/multi-user.target.upholds/watch.service";
    let snap = scan(
        UnitHost::new()
            .unit(UNIT, "enabled")
            .preset("disable *")
            .etc_unit(
                UNIT,
                "[Service]\nExecStart=/bin/true\n\n[Install]\nUpheldBy=multi-user.target\n",
            )
            .link(LINK, "/etc/systemd/system/watch.service"),
    );
    let config_paths: Vec<&str> = snap
        .config
        .iter()
        .flat_map(|c| &c.files)
        .map(|f| f.path.as_str())
        .collect();
    assert!(
        config_paths.contains(&"/etc/systemd/system/watch.service"),
        "{config_paths:?}"
    );
    assert!(!config_paths.contains(&LINK), "{config_paths:?}");
    let units = snap.units.as_ref().expect("a units section");
    assert!(units.state_changes.is_empty(), "{:?}", units.state_changes);
    let first = "# Advisory: multi-user.target.upholds/watch.service is not carried: the unit's \
                 [Install] section cannot be relied on (enabling the unit would also make an \
                 .upholds link).";
    let expected = [
        first.to_owned(),
        "#   An .upholds link starts watch.service while multi-user.target is active and \
         restarts it whenever it stops, so adding it for a unit that becomes inactive after it \
         exits, such as a oneshot service without RemainAfterExit=yes, produces a restart loop. \
         A RHEL 9 target does not honor it. To add it, put Upholds=watch.service in a drop-in \
         for multi-user.target."
            .to_owned(),
    ];
    for (route, tree) in both_exports(&snap) {
        let containerfile = containerfile_in(&tree);
        assert_eq!(
            block_from(&containerfile, first),
            expected,
            "{route}:\n{containerfile}"
        );
        assert!(
            in_no_systemctl_instruction(&containerfile, UNIT),
            "{route}:\n{containerfile}"
        );
        assert!(
            staged_at(&tree, "/etc/systemd/system/watch.service"),
            "{route}"
        );
        assert!(!staged_at(&tree, LINK), "{route}");
    }
}

/// T24's reset links through the scan:
/// - a local unit with `WantedBy=multi-user.target`, an `/etc` drop-in
///   holding only `WantedBy=` and its `multi-user.target.wants` link: the
///   `StaticUnit` advisory and its `add-wants` line;
/// - `RequiredBy=a.target` in a vendor unit, an empty `RequiredBy=` in an
///   `/etc` drop-in and the `a.target.requires` link: the `Undetermined`
///   advisory naming `RequiredByReset` in words, and the unit-level block
///   naming the drop-in. No instruction names either unit.
#[test]
fn t24_reset_links_are_advised() {
    let snap = scan(
        UnitHost::new()
            .unit("app.service", "enabled")
            .preset("disable *")
            .etc_unit("app.service", WANTS_MULTI_USER)
            .file(
                "/etc/systemd/system/app.service.d/10-reset.conf",
                "[Install]\nWantedBy=\n",
            )
            .link(
                "/etc/systemd/system/multi-user.target.wants/app.service",
                "/etc/systemd/system/app.service",
            )
            .unit("need.service", "enabled")
            .base_package("needpkg")
            .vendor_unit(
                "need.service",
                "[Service]\nExecStart=/bin/true\n\n[Install]\nRequiredBy=a.target\n",
                Some("needpkg"),
            )
            .file(
                "/etc/systemd/system/need.service.d/10-reset.conf",
                "[Install]\nRequiredBy=\n",
            )
            .link(
                "/etc/systemd/system/a.target.requires/need.service",
                "/usr/lib/systemd/system/need.service",
            ),
    );
    let static_first = "# Advisory: multi-user.target.wants/app.service is a link on the source \
                        host that app.service's [Install] section there does not make (it names \
                        no link: none, only Also=, or a list a drop-in reset to empty). Not \
                        carried.";
    let reset_first = "# Advisory: a.target.requires/need.service is not carried: the unit's \
                       [Install] section cannot be relied on (an empty RequiredBy= whose effect \
                       was not measured).";
    let record_first = "# Advisory: need.service is not carried: its [Install] section on the \
                        source host cannot be relied on (an empty RequiredBy= whose effect was \
                        not measured), and these files under /etc/systemd/system change it: \
                        /etc/systemd/system/need.service.d/10-reset.conf.";
    for (route, containerfile) in both_routes(&snap) {
        assert_eq!(
            block_from(&containerfile, static_first),
            [
                static_first.to_owned(),
                "#   To carry it, add after the configuration COPY: RUN systemctl add-wants -- \
                 'multi-user.target' 'app.service'"
                    .to_owned()
            ],
            "{route}:\n{containerfile}"
        );
        assert_eq!(
            block_from(&containerfile, reset_first),
            [
                reset_first.to_owned(),
                "#   To carry it, add after the configuration COPY: RUN systemctl add-requires \
                 -- 'a.target' 'need.service'"
                    .to_owned()
            ],
            "{route}:\n{containerfile}"
        );
        assert_eq!(
            block_from(&containerfile, record_first),
            [
                record_first.to_owned(),
                undetermined_second_line("need.service")
            ],
            "{route}:\n{containerfile}"
        );
        for unit in ["app.service", "need.service"] {
            assert!(
                in_no_systemctl_instruction(&containerfile, unit),
                "{route} {unit}:\n{containerfile}"
            );
        }
    }
}

/// An alias reached through a chain, through the scan, the render and
/// refine. `real.service`'s `[Install]` is `Alias=public.service`, its
/// preset `enable`, and the host's links, planted by hand, are
/// `public.service -> relay.service -> real.service`. The chain installs
/// the unit: the inventory reads `Enabled` and the relay is reported as a
/// second alias. So the unit is at its preset, with no state row in refine
/// and no `disable` on either route.
#[test]
fn an_alias_chain_installs_its_unit_through_both_exports() {
    const ALIASED: &str = "[Service]\nExecStart=/bin/true\n\n[Install]\nAlias=public.service\n";
    let snap = scan(
        UnitHost::new()
            .unit("real.service", "enabled")
            .unit("public.service", "alias")
            .unit("relay.service", "alias")
            .preset("enable real.service")
            .base_package("realpkg")
            .vendor_unit("real.service", ALIASED, Some("realpkg"))
            .link("/etc/systemd/system/public.service", "relay.service")
            .link(
                "/etc/systemd/system/relay.service",
                "/usr/lib/systemd/system/real.service",
            ),
    );
    assert_eq!(
        inventory_entry(&snap, "real.service").map(|e| e.state),
        Some(UnitState::Enabled),
        "the chain installs real.service"
    );
    assert_eq!(changes_named(&snap, "real.service"), 0);
    let relay: Vec<_> = links_of(&snap)
        .iter()
        .map(|l| (l.relation, l.parent.as_str(), l.unit.as_str(), l.cause))
        .collect();
    assert_eq!(
        relay,
        [(
            LinkRelation::Alias,
            "relay.service",
            "real.service",
            ManualLinkCause::NotInInstall
        )]
    );

    let refine = session(&snap);
    assert!(
        refine
            .decisions()
            .manual_links
            .iter()
            .any(|row| row.parent.as_str() == "relay.service"),
        "refine shows the relay link"
    );
    assert!(
        refine
            .decisions()
            .unit_states
            .iter()
            .all(|row| row.unit.as_str() != "real.service"),
        "no state row for real.service: {:?}",
        refine.decisions().unit_states
    );
    for (route, containerfile) in both_routes(&snap) {
        assert!(
            containerfile.contains("relay.service"),
            "{route}: the relay link is advised:\n{containerfile}"
        );
        assert!(
            !acts(&containerfile, "disable", "real.service"),
            "{route}:\n{containerfile}"
        );
    }
}

/// A directory listing under `/etc/systemd/system` that fails part way
/// (`read_dir_strict`) leaves the links or drop-ins incomplete, so no
/// command is derived from it: the unit root missing its `.wants`
/// directory, the dependency directory missing the unit's link, and the
/// unit's own `.d` missing its install drop-in. Healthy first: each host
/// read whole gives the unit its state and no `disable`. Each partial
/// listing degrades the scan naming the directory, and the unit gets no
/// state change and no `systemctl` line on either route; the first two
/// would otherwise read an enabled unit as disabled and carry a `disable`.
#[test]
fn a_partial_directory_listing_derives_no_command() {
    const APP: &str = "app.service";
    const WANTS: &str = "/etc/systemd/system/multi-user.target.wants";
    const DROP_IN_DIR: &str = "/etc/systemd/system/app.service.d";
    let vendor = "[Service]\nExecStart=/usr/bin/app\n\n[Install]\nWantedBy=multi-user.target\n";
    let linked = || {
        UnitHost::new()
            .unit(APP, "enabled")
            .preset("enable *")
            .base_package("apppkg")
            .vendor_unit(APP, vendor, Some("apppkg"))
    };
    let with_drop_in = || {
        UnitHost::new()
            .unit(APP, "enabled")
            .preset("disable *")
            .base_package("apppkg")
            .vendor_unit(APP, vendor, Some("apppkg"))
            .file(
                &format!("{DROP_IN_DIR}/10-wants.conf"),
                "[Install]\nWantedBy=graphical.target\n",
            )
    };
    let degraded = |snap: &InspectionSnapshot| -> Vec<String> {
        snap.warnings
            .iter()
            .filter(|w| w.inspector == "Units" && w.message.starts_with("degraded: "))
            .map(|w| w.message.clone())
            .collect()
    };

    let healthy = scan(linked());
    assert_eq!(
        inventory_entry(&healthy, APP).map(|e| e.state),
        Some(UnitState::Enabled)
    );
    assert!(degraded(&healthy).is_empty(), "{:?}", healthy.warnings);
    let healthy_drop_in = scan(with_drop_in());
    assert_eq!(
        change(&healthy_drop_in, APP).install_drop_ins,
        [format!("{DROP_IN_DIR}/10-wants.conf")]
    );
    assert!(degraded(&healthy_drop_in).is_empty());

    for (label, host, dir) in [
        (
            "unit root",
            linked().partial_listing("/etc/systemd/system", "multi-user.target.wants"),
            "/etc/systemd/system",
        ),
        (
            "dependency directory",
            linked().partial_listing(WANTS, APP),
            WANTS,
        ),
        (
            "install drop-in directory",
            with_drop_in().partial_listing(DROP_IN_DIR, "10-wants.conf"),
            DROP_IN_DIR,
        ),
    ] {
        let snap = scan(host);
        let failures = degraded(&snap);
        assert!(
            failures.iter().any(|f| f.contains(&format!("{dir}: "))),
            "{label}: the scan degrades naming {dir}: {failures:?}"
        );
        assert_eq!(changes_named(&snap, APP), 0, "{label}");
        for (route, containerfile) in both_routes(&snap) {
            assert!(
                in_no_systemctl_instruction(&containerfile, APP),
                "{label}, {route}:\n{containerfile}"
            );
        }
    }
}

/// A preset directory whose listing fails part way (`read_dir_strict`)
/// may have left out the file holding a unit's rule, so the presets are
/// incomplete and no state change is derived from them. Healthy first:
/// with `enable app.service` read ahead of `disable *`, the enabled unit
/// is at its preset and has no state change. With the file holding that
/// rule left out of either directory's listing, a partial read would put
/// the unit against `disable *` and carry an `enable` the presets never
/// asked for; instead the scan degrades naming the directory, and the
/// unit gets no state change and no `systemctl` line on either route.
#[test]
fn a_partial_preset_directory_listing_derives_no_state_change() {
    const APP: &str = "app.service";
    const USR_PRESETS: &str = "/usr/lib/systemd/system-preset";
    const ETC_PRESETS: &str = "/etc/systemd/system-preset";
    const RULE_FILE: &str = "50-app.preset";
    let vendor = "[Service]\nExecStart=/usr/bin/app\n\n[Install]\nWantedBy=multi-user.target\n";
    let host = |dir: &str| {
        UnitHost::new()
            .unit(APP, "enabled")
            .preset("disable *")
            .base_package("apppkg")
            .vendor_unit(APP, vendor, Some("apppkg"))
            .file(&format!("{dir}/{RULE_FILE}"), "enable app.service\n")
    };
    let degraded = |snap: &InspectionSnapshot| -> Vec<String> {
        snap.warnings
            .iter()
            .filter(|w| w.inspector == "Units" && w.message.starts_with("degraded: "))
            .map(|w| w.message.clone())
            .collect()
    };

    for dir in [USR_PRESETS, ETC_PRESETS] {
        let healthy = scan(host(dir));
        assert!(
            degraded(&healthy).is_empty(),
            "{dir}: {:?}",
            healthy.warnings
        );
        assert_eq!(changes_named(&healthy, APP), 0, "{dir}: at its preset");

        let snap = scan(host(dir).partial_listing(dir, RULE_FILE));
        let failures = degraded(&snap);
        assert!(
            failures.iter().any(|f| f.contains(&format!("{dir}: "))),
            "the scan degrades naming {dir}: {failures:?}"
        );
        assert_eq!(changes_named(&snap, APP), 0, "{dir}");
        for (route, containerfile) in both_routes(&snap) {
            assert!(
                in_no_systemctl_instruction(&containerfile, APP),
                "{dir}, {route}:\n{containerfile}"
            );
        }
    }
}

/// An alias chain the collector cannot follow to its end: the link
/// `/etc/systemd/system/public.service` goes through a link under `/opt`
/// that cannot be read on the way to the packaged `real.service`, whose
/// `[Install]` names the alias and whose preset is `enable`. The links
/// under `/etc` are incomplete, so the scan degrades naming the alias, and
/// `real.service` gets no state: no state change, no state row in refine,
/// and no `disable` on either route. Control: the same chain readable
/// installs `real.service`, with no `disable` either. The relay is named
/// after the unit, so the host builder takes the chain as its enable link
/// and adds none of its own.
#[test]
fn an_alias_chain_that_cannot_be_followed_disables_nothing_through_both_exports() {
    const ALIASED: &str = "[Service]\nExecStart=/bin/true\n\n[Install]\nAlias=public.service\n";
    const ALIAS: &str = "/etc/systemd/system/public.service";
    const RELAY: &str = "/opt/units/real.service";
    let host = |readable: bool| {
        let host = UnitHost::new()
            .unit("real.service", "enabled")
            .unit("public.service", "alias")
            .preset("enable real.service")
            .base_package("realpkg")
            .vendor_unit("real.service", ALIASED, Some("realpkg"))
            .link(ALIAS, RELAY)
            .link(RELAY, "/usr/lib/systemd/system/real.service");
        if readable {
            host
        } else {
            host.link_error(RELAY, std::io::ErrorKind::PermissionDenied)
        }
    };
    let degraded = |snap: &InspectionSnapshot| -> Vec<String> {
        snap.warnings
            .iter()
            .filter(|w| w.inspector == "Units" && w.message.starts_with("degraded: "))
            .map(|w| w.message.clone())
            .collect()
    };

    let control = scan(host(true));
    assert_eq!(
        inventory_entry(&control, "real.service").map(|e| e.state),
        Some(UnitState::Enabled),
        "control: the readable chain installs real.service"
    );
    assert!(degraded(&control).is_empty(), "{:?}", control.warnings);
    for (route, containerfile) in both_routes(&control) {
        assert!(
            !acts(&containerfile, "disable", "real.service"),
            "{route}, control:\n{containerfile}"
        );
    }

    let snap = scan(host(false));
    let failures = degraded(&snap);
    assert!(
        failures.len() == 1
            && failures[0].starts_with("degraded: unit file read failures: ")
            && failures[0].contains(&format!("{ALIAS}: "))
            && failures[0].contains(RELAY),
        "the scan degrades naming the alias: {failures:?}"
    );
    assert!(
        inventory_entry(&snap, "real.service").is_none(),
        "no state for real.service"
    );
    assert_eq!(changes_named(&snap, "real.service"), 0);
    let refine = session(&snap);
    assert!(
        refine
            .decisions()
            .unit_states
            .iter()
            .all(|row| row.unit.as_str() != "real.service"),
        "no state row for real.service: {:?}",
        refine.decisions().unit_states
    );
    for (route, containerfile) in both_routes(&snap) {
        assert!(
            !acts(&containerfile, "disable", "real.service"),
            "{route}:\n{containerfile}"
        );
    }
}

/// A vendor unit file that cannot be read as text, with the link its
/// `[Install]` names and no file under `/etc`, through the scan, the render
/// and refine. Its section is undetermined and `Also=` is not known: no
/// state change, one record with no files, whose block says inspectah
/// could not tell whether it assigns `Also=` (never that it does), the
/// link reported with the same reason, and no `systemctl` line on either
/// route, although the listing says `enabled` against a `disable` preset.
/// Control: the readable file is enabled on both routes.
#[test]
fn an_unreadable_unit_file_is_an_undetermined_record_through_both_exports() {
    const FILE: &str = "/usr/lib/systemd/system/need.service";
    let host = |readable: bool| {
        let host = UnitHost::new()
            .unit("need.service", "enabled")
            .preset("disable need.service")
            .base_package("needpkg")
            .vendor_unit("need.service", WANTS_MULTI_USER, Some("needpkg"));
        if readable {
            host
        } else {
            host.file_error(FILE, std::io::ErrorKind::InvalidData)
        }
    };

    let control = scan(host(true));
    for (route, containerfile) in both_routes(&control) {
        assert!(
            acts(&containerfile, "enable", "need.service"),
            "{route}, control:\n{containerfile}"
        );
    }

    let snap = scan(host(false));
    assert_eq!(changes_named(&snap, "need.service"), 0);
    let expected = [
        "# Advisory: need.service is not carried: its [Install] section on the source host \
         cannot be relied on (a file of it inspectah could not read as text). It may assign \
         Also=, so units it names may be enabled or disabled with it in the image, and \
         inspectah could not read it to check."
            .to_owned(),
        undetermined_no_file_second_line("need.service"),
    ];
    let refine = session(&snap);
    let rows = &refine.decisions().undetermined_installs;
    assert_eq!(
        rows.iter()
            .map(|r| (r.unit.as_str(), r.reason, r.install_files.len()))
            .collect::<Vec<_>>(),
        [("need.service", InstallUnknown::Unreadable, 0)]
    );
    assert_eq!(rows[0].advisories, expected);
    assert_eq!(
        links_of(&snap)
            .iter()
            .map(|l| (l.parent.as_str(), l.cause))
            .collect::<Vec<_>>(),
        [(
            "multi-user.target",
            ManualLinkCause::Undetermined {
                reason: InstallUnknown::Unreadable
            }
        )]
    );
    for (route, containerfile) in both_routes(&snap) {
        assert_eq!(
            block_from(&containerfile, &expected[0]),
            expected,
            "{route}:\n{containerfile}"
        );
        assert!(
            in_no_systemctl_instruction(&containerfile, "need.service"),
            "{route}:\n{containerfile}"
        );
    }
}

/// The T40 host: `need.service`, a vendor unit of an installed package
/// under an `enable` preset, with `unit_file` and, when given, an `/etc`
/// drop-in `10-reset.conf` holding `drop_in`. No link to it anywhere.
fn t40_host(unit_file: &str, drop_in: Option<&str>) -> UnitHost {
    let host = UnitHost::new()
        .unit("need.service", "disabled")
        .preset("enable need.service")
        .base_package("needpkg")
        .vendor_unit("need.service", unit_file, Some("needpkg"));
    match drop_in {
        Some(text) => host.file(T40_DROP_IN, text),
        None => host,
    }
}

const T40_DROP_IN: &str = "/etc/systemd/system/need.service.d/10-reset.conf";

/// T40 through the scan, the render and refine: the `RequiredBy=`-only
/// vendor unit with a drop-in holding `RequiredBy=` and no link has its
/// record, the block naming the unit, the reason and the drop-in on both
/// routes, the refine row with the same block and no include field, and no
/// `systemctl` line for the unit. The `UpheldBy=` drop-in gives the same
/// with its reason. Control: the vendor unit with no `/etc` file and
/// `UpheldBy=` in its own file has no record, and its `disable` is carried.
#[test]
fn t40_an_undetermined_section_with_an_etc_file_and_no_link() {
    let required = "[Service]\nExecStart=/bin/true\n\n[Install]\nRequiredBy=a.target\n";
    for (drop_in, reason, words) in [
        (
            "[Install]\nRequiredBy=\n",
            InstallUnknown::RequiredByReset,
            "an empty RequiredBy= whose effect was not measured",
        ),
        (
            "[Install]\nUpheldBy=a.target\n",
            InstallUnknown::UpheldBy,
            "enabling the unit would also make an .upholds link",
        ),
    ] {
        let snap = scan(t40_host(required, Some(drop_in)));
        let units = snap.units.as_ref().expect("a units section");
        assert!(
            units.drop_ins.iter().any(|d| d.path == T40_DROP_IN),
            "{reason:?}: the drop-in is collected"
        );
        assert!(units.state_changes.is_empty(), "{reason:?}");
        assert!(units.manual_links.is_empty(), "{reason:?}");
        let expected = [
            format!(
                "# Advisory: need.service is not carried: its [Install] section on the source \
                 host cannot be relied on ({words}), and these files under \
                 /etc/systemd/system change it: {T40_DROP_IN}."
            ),
            undetermined_second_line("need.service"),
        ];
        let refine = session(&snap);
        let rows = &refine.decisions().undetermined_installs;
        assert_eq!(rows.len(), 1, "{reason:?}: {rows:?}");
        assert_eq!(
            (
                rows[0].unit.as_str(),
                rows[0].reason,
                &rows[0].install_files
            ),
            ("need.service", reason, &vec![T40_DROP_IN.to_owned()]),
            "{reason:?}"
        );
        assert_eq!(rows[0].advisories, expected, "{reason:?}");
        assert!(
            !has_include(serde_json::to_value(&rows[0]).expect("the row serializes")),
            "{reason:?}"
        );
        for (route, containerfile) in both_routes(&snap) {
            assert_eq!(
                block_from(&containerfile, &expected[0]),
                expected,
                "{route} {reason:?}:\n{containerfile}"
            );
            assert!(
                in_no_systemctl_instruction(&containerfile, "need.service"),
                "{route} {reason:?}:\n{containerfile}"
            );
        }
    }

    let upheld = "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\nUpheldBy=multi-user.target\n";
    let control = scan(t40_host(upheld, None));
    assert!(
        control
            .units
            .as_ref()
            .expect("a units section")
            .undetermined_installs
            .is_empty()
    );
    for (route, containerfile) in both_routes(&control) {
        assert!(
            acts(&containerfile, "disable", "need.service"),
            "{route}:\n{containerfile}"
        );
        assert!(
            !containerfile.contains("# Advisory: need.service"),
            "{route}:\n{containerfile}"
        );
    }
}

/// T40's `UpheldBy=` vendor unit with no `/etc` file: with one link, the
/// per-link advisory and no `systemctl` line for the unit; with
/// `Also=helper.service` and no link, no state change and one record with
/// no files, its block without the file clause and with the `Also=`
/// sentence.
#[test]
fn t40_an_upheld_vendor_unit_with_a_link_or_also() {
    let upheld = "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\nUpheldBy=multi-user.target\n";
    let linked = scan(t40_host(upheld, None).link(
        "/etc/systemd/system/multi-user.target.wants/need.service",
        "/usr/lib/systemd/system/need.service",
    ));
    let first = "# Advisory: multi-user.target.wants/need.service is not carried: the unit's \
                 [Install] section cannot be relied on (enabling the unit would also make an \
                 .upholds link).";
    for (route, containerfile) in both_routes(&linked) {
        assert_eq!(
            block_from(&containerfile, first),
            [
                first.to_owned(),
                "#   To carry it, add after the configuration COPY: RUN systemctl add-wants -- \
                 'multi-user.target' 'need.service'"
                    .to_owned()
            ],
            "{route}:\n{containerfile}"
        );
        assert!(
            in_no_systemctl_instruction(&containerfile, "need.service"),
            "{route}:\n{containerfile}"
        );
    }

    let also = scan(t40_host(&format!("{upheld}Also=helper.service\n"), None));
    let units = also.units.as_ref().expect("a units section");
    assert!(units.state_changes.is_empty(), "{:?}", units.state_changes);
    assert_eq!(units.undetermined_installs.len(), 1);
    assert!(units.undetermined_installs[0].install_files.is_empty());
    let expected = [
        "# Advisory: need.service is not carried: its [Install] section on the source host \
         cannot be relied on (enabling the unit would also make an .upholds link). It assigns \
         Also=, so the units it names are enabled or disabled with it in the image, and \
         inspectah could not list them to check."
            .to_owned(),
        undetermined_no_file_second_line("need.service"),
    ];
    for (route, containerfile) in both_routes(&also) {
        assert_eq!(
            block_from(&containerfile, &expected[0]),
            expected,
            "{route}:\n{containerfile}"
        );
        assert!(
            in_no_systemctl_instruction(&containerfile, "need.service"),
            "{route}:\n{containerfile}"
        );
    }
}

/// A package `[Install]` that cannot be relied on, under an `/etc` shadow
/// that can (Mark Russell, 2026-11-15): the vendor unit's `UpheldBy=`, at
/// its `enable` preset. With a shadow that names no link and no link on the
/// host, and with a shadow naming `multi-user.target` and that link, the
/// unit has one record naming the shadow, the block on both routes and in
/// refine's `undetermined_installs`, no state change and no `systemctl`
/// line; with the link, the per-link `Undetermined` finding stays, as on the
/// effective side. Control: the `UpheldBy=` in the shadow itself, the
/// effective side, gives the same record, block and finding.
#[test]
fn a_vendor_side_undetermined_section_has_its_record() {
    const UPHELD: &str = "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\nUpheldBy=multi-user.target\n";
    const NO_INSTALL: &str = "[Service]\nExecStart=/bin/true\n";
    let wants = "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\n";
    let undetermined = ManualLinkCause::Undetermined {
        reason: InstallUnknown::UpheldBy,
    };
    let expected = [
        format!(
            "# Advisory: {DIFF_UNIT} is not carried: its [Install] section on the source host \
             cannot be relied on (enabling the unit would also make an .upholds link), and \
             these files under /etc/systemd/system change it: {DIFF_SHADOW}."
        ),
        undetermined_second_line(DIFF_UNIT),
    ];
    let link_line = format!(
        "# Advisory: multi-user.target.wants/{DIFF_UNIT} is not carried: the unit's [Install] \
         section cannot be relied on (enabling the unit would also make an .upholds link)."
    );
    // (case, the package's file, the shadow, the listed state, a link)
    for (case, vendor, shadow, state, linked) in [
        ("vendor side, no link", UPHELD, NO_INSTALL, "static", false),
        ("vendor side, linked", UPHELD, wants, "enabled", true),
        (
            "effective side, linked (control)",
            wants,
            UPHELD,
            "enabled",
            true,
        ),
    ] {
        let host = diff_host(DIFF_UNIT, vendor, state).etc_unit(DIFF_UNIT, shadow);
        let host = if linked {
            wants_link(host, "multi-user.target", DIFF_UNIT)
        } else {
            host
        };
        let snap = scan(host);
        let units = snap.units.as_ref().expect("a units section");
        assert_eq!(changes_named(&snap, DIFF_UNIT), 0, "{case}");
        assert!(
            !units.inventory.iter().any(|e| e.unit.as_str() == DIFF_UNIT),
            "{case}: no state: {:?}",
            units.inventory
        );
        assert_eq!(
            units
                .undetermined_installs
                .iter()
                .map(|r| (r.unit.as_str(), r.reason, r.install_files.clone()))
                .collect::<Vec<_>>(),
            [(
                DIFF_UNIT,
                InstallUnknown::UpheldBy,
                vec![DIFF_SHADOW.to_owned()]
            )],
            "{case}"
        );
        let expected_links: Vec<(&str, ManualLinkCause)> = if linked {
            vec![("multi-user.target", undetermined)]
        } else {
            Vec::new()
        };
        assert_eq!(
            links_of(&snap)
                .iter()
                .map(|l| (l.parent.as_str(), l.cause))
                .collect::<Vec<_>>(),
            expected_links,
            "{case}"
        );

        let refine = session(&snap);
        let rows = &refine.decisions().undetermined_installs;
        assert_eq!(
            rows.iter()
                .map(|r| (r.unit.as_str(), r.reason, r.install_files.clone()))
                .collect::<Vec<_>>(),
            [(
                DIFF_UNIT,
                InstallUnknown::UpheldBy,
                vec![DIFF_SHADOW.to_owned()]
            )],
            "{case}"
        );
        assert_eq!(rows[0].advisories, expected, "{case}");

        for (route, containerfile) in both_routes(&snap) {
            assert_eq!(
                block_from(&containerfile, &expected[0]),
                expected,
                "{route} {case}:\n{containerfile}"
            );
            assert_eq!(
                containerfile.lines().any(|l| l == link_line),
                linked,
                "{route} {case}: the per-link advisory:\n{containerfile}"
            );
            assert!(
                in_no_systemctl_instruction(&containerfile, DIFF_UNIT),
                "{route} {case}:\n{containerfile}"
            );
        }
    }
}

/// A shadowed mount unit whose package section cannot be relied on is kept
/// out of the storage pass, as an effective-side one is: it has the record
/// and no mount finding, so nothing says it is not enabled. Control: the
/// same shadow over a package section that can be relied on is a storage
/// unit with no state.
#[test]
fn a_vendor_side_undetermined_storage_unit_is_never_said_to_be_not_enabled() {
    const UNIT: &str = "srv-pkg.mount";
    let shadow = "[Mount]\nWhat=/dev/vdb\nWhere=/srv/pkg\n";
    for (vendor_install, undetermined) in [
        ("WantedBy=local-fs.target\n", false),
        ("WantedBy=local-fs.target\nUpheldBy=local-fs.target\n", true),
    ] {
        let vendor = format!("{shadow}[Install]\n{vendor_install}");
        let snap = scan(
            diff_host(UNIT, &vendor, "static")
                .preset(&format!("enable {UNIT}"))
                .etc_unit(UNIT, shadow),
        );
        let units = snap.units.as_ref().expect("a units section");
        let finding = units
            .mount_findings
            .iter()
            .find(|f| f.unit.name().as_unit_name().as_str() == UNIT)
            .map(|f| f.state_change.map(|(state, _)| state));
        let records: Vec<&str> = units
            .undetermined_installs
            .iter()
            .map(|r| r.unit.as_str())
            .collect();
        if undetermined {
            assert_eq!(finding, None, "no mount finding");
            assert_eq!(records, [UNIT]);
            for (route, containerfile) in both_routes(&snap) {
                assert!(
                    !containerfile.contains(&format!("{UNIT} is not enabled")),
                    "{route}:\n{containerfile}"
                );
            }
        } else {
            assert_eq!(finding, Some(None), "control: the storage pass");
            assert!(records.is_empty(), "control: {records:?}");
        }
    }
}

// ---------------------------------------------------------------------
// § 7.4 and § 10.9: a unit at its preset whose `/etc` files change
// `[Install]`, rendered (T31, T32, T41, T40's excluded file).
// ---------------------------------------------------------------------

/// The unit of the install-difference hosts.
const DIFF_UNIT: &str = "app.service";
/// Its full shadow.
const DIFF_SHADOW: &str = "/etc/systemd/system/app.service";
/// A section wanted by `multi-user.target` and `extra.target`.
const WANTS_BOTH: &str =
    "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target extra.target\n";
/// A section wanted by `graphical.target` only.
const WANTS_GRAPHICAL: &str =
    "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=graphical.target\n";

/// `unit` from [`INSTALL_PACKAGE`] with `vendor` as its file, listed
/// `state` under an `enable` preset for it, scanned against a base image
/// so refine keeps the package included.
fn diff_host(unit: &str, vendor: &str, state: &str) -> UnitHost {
    UnitHost::new()
        .base_package("glibc")
        .package(INSTALL_PACKAGE)
        .unit(unit, state)
        .preset(&format!("enable {unit}"))
        .vendor_unit(unit, vendor, Some(INSTALL_PACKAGE))
}

/// `host` with a link to `unit` in `<parent>.wants` under `/etc`.
fn wants_link(host: UnitHost, parent: &str, unit: &str) -> UnitHost {
    host.link(
        &format!("/etc/systemd/system/{parent}.wants/{unit}"),
        &format!("/usr/lib/systemd/system/{unit}"),
    )
}

/// The drop-in `file` in `unit`'s own `.d` under `/etc`.
fn etc_drop_in(unit: &str, file: &str) -> String {
    format!("/etc/systemd/system/{unit}.d/{file}")
}

/// A refine session over `snap` with `ops` applied in order.
fn session_after(snap: &InspectionSnapshot, ops: &[(ItemId, bool)]) -> RefineSession {
    let mut session = session(snap);
    for (item_id, include) in ops {
        session
            .apply(set_include(item_id.clone(), *include))
            .unwrap_or_else(|e| panic!("SetInclude({item_id:?}, {include}): {e:?}"));
    }
    session
}

fn drop_in_item(path: &str) -> ItemId {
    ItemId::DropIn { path: path.into() }
}

/// A refine row's `not_applied` paths, read from its serialized form.
fn not_applied_of(row: serde_json::Value) -> Vec<String> {
    match row.get("not_applied") {
        None => Vec::new(),
        Some(paths) => serde_json::from_value(paths.clone()).expect("a list of paths"),
    }
}

/// § 10.9's `# Omitted:` line for `unit`'s install files `excluded`.
fn install_omitted(unit: &str, excluded: &[&str]) -> String {
    format!(
        "# Omitted: {unit} links (excluded from the export: {}; the [Install] the image reads \
         is not the one recorded, so the source host's links for {unit} are not reproduced, and \
         links its package install made may remain in the image. Check them with systemctl \
         is-enabled in the image.)",
        excluded.join(", ")
    )
}

/// How many lines of `containerfile` are `line`.
fn count_lines(containerfile: &str, line: &str) -> usize {
    containerfile.lines().filter(|l| *l == line).count()
}

/// § 10.9's `HostInstallOnly` block for a `.wants` link.
fn host_only_block(parent: &str, unit: &str) -> [String; 2] {
    [
        format!(
            "# Advisory: {parent}.wants/{unit} is on the source host because {unit}'s files \
             under /etc/systemd/system name it, and the package's unit does not. The image was \
             linked from the package's unit, so it lacks this link. Not carried."
        ),
        format!(
            "#   To carry it, add after the configuration COPY: RUN systemctl add-wants -- \
             '{parent}' '{unit}'"
        ),
    ]
}

/// § 10.9's `VendorInstallOnly` block for a link named `link` (the
/// dependency-directory path, or the alias's words), removed at `path`.
fn vendor_only_block(link: &str, unit: &str, path: &str) -> [String; 2] {
    [
        format!(
            "# Advisory: {link} is not on the source host: {unit}'s files under \
             /etc/systemd/system do not name it, and the package's unit does. The image was \
             linked from the package's unit, so it has this link."
        ),
        format!(
            "#   To remove it, add after the configuration COPY: RUN test -L '{path}' && rm -- \
             '{path}'"
        ),
    ]
}

/// T31: the units of T30 (b), (c), (e) and the first case of (l) are in
/// `RUN systemctl enable` after every `COPY`, the one that stages their
/// `/etc` files included, on both routes.
/// With the drop-in of (b) or the shadow of (c) excluded through refine:
/// `InstallEvidenceChanged` naming it and no enable. The units of (a) and
/// (o) are in no `systemctl` line.
#[test]
fn t31_added_links_are_enabled_after_the_copy() {
    let b_drop_in = etc_drop_in(DIFF_UNIT, "10-extra.conf");
    let with_both = |host| {
        wants_link(
            wants_link(host, "multi-user.target", DIFF_UNIT),
            "extra.target",
            DIFF_UNIT,
        )
    };
    let b = with_both(diff_host(DIFF_UNIT, WANTS_MULTI_USER, "enabled"))
        .file(&b_drop_in, "[Install]\nWantedBy=extra.target\n");
    let c = with_both(diff_host(DIFF_UNIT, WANTS_MULTI_USER, "enabled"))
        .etc_unit(DIFF_UNIT, WANTS_BOTH);
    let cases: Vec<(&str, UnitHost, Option<String>)> = vec![
        ("(b)", b, Some(b_drop_in.clone())),
        ("(c) WantedBy=", c, Some(DIFF_SHADOW.to_owned())),
        (
            "(c) Alias=",
            wants_link(diff_host(DIFF_UNIT, WANTS_MULTI_USER, "enabled"), "multi-user.target", DIFF_UNIT)
                .etc_unit(
                    DIFF_UNIT,
                    "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\nAlias=app-alias.service\n",
                )
                .link("/etc/systemd/system/app-alias.service", DIFF_SHADOW),
            None,
        ),
        (
            "(e)",
            wants_link(diff_host(DIFF_UNIT, NO_INSTALL, "enabled"), "multi-user.target", DIFF_UNIT)
                .file(&etc_drop_in(DIFF_UNIT, "10-install.conf"), INSTALL_DROP_IN),
            None,
        ),
        (
            "(l)",
            with_both(diff_host(DIFF_UNIT, WANTS_MULTI_USER, "enabled")).file(
                &etc_drop_in(DIFF_UNIT, "10-reset.conf"),
                "[Install]\nWantedBy=\nWantedBy=multi-user.target extra.target\n",
            ),
            None,
        ),
    ];
    for (case, host, excluded_file) in cases {
        let snap = scan(host);
        for (route, containerfile) in both_routes(&snap) {
            assert!(
                acts(&containerfile, "enable", DIFF_UNIT),
                "{case} {route}:\n{containerfile}"
            );
            // The install files are staged through the drop-ins COPY or the
            // configuration COPY: the enable follows every COPY.
            let copy = containerfile
                .rfind("\nCOPY ")
                .unwrap_or_else(|| panic!("{case} {route}: no COPY:\n{containerfile}"));
            let enable = containerfile
                .find(&format!("'{DIFF_UNIT}'"))
                .unwrap_or_else(|| panic!("{case} {route}: no enable:\n{containerfile}"));
            assert!(
                copy < enable,
                "{case} {route}: the enable precedes the COPY:\n{containerfile}"
            );
        }
        if let Some(path) = excluded_file {
            let excluded =
                containerfile_in(&exported_after(&snap, &[(drop_in_item(&path), false)]));
            assert!(
                in_no_systemctl_instruction(&excluded, DIFF_UNIT),
                "{case}:\n{excluded}"
            );
            assert!(
                omitted(&excluded, DIFF_UNIT)
                    .is_some_and(|l| l.ends_with(&evidence_changed(&path))),
                "{case}:\n{excluded}"
            );
        }
    }

    let unchanged = wants_link(
        diff_host(DIFF_UNIT, WANTS_MULTI_USER, "enabled"),
        "multi-user.target",
        DIFF_UNIT,
    )
    .etc_unit(DIFF_UNIT, WANTS_MULTI_USER);
    let service_only = diff_host(DIFF_UNIT, NO_INSTALL, "static")
        .file(&etc_drop_in(DIFF_UNIT, "10-nice.conf"), SERVICE_DROP_IN);
    for (case, host, path) in [
        ("(a)", unchanged, DIFF_SHADOW.to_owned()),
        ("(o)", service_only, etc_drop_in(DIFF_UNIT, "10-nice.conf")),
    ] {
        let snap = scan(host);
        assert!(
            snap.units
                .as_ref()
                .expect("a units section")
                .drop_ins
                .iter()
                .any(|d| d.path == path),
            "{case}: the /etc file is collected"
        );
        for (route, containerfile) in both_routes(&snap) {
            assert!(
                in_no_systemctl_instruction(&containerfile, DIFF_UNIT),
                "{case} {route}:\n{containerfile}"
            );
        }
    }
}

/// T32 for an alias: a shadow whose `[Install]` names `Alias=` in place of
/// the package's `WantedBy=multi-user.target`, with the alias link on the
/// host. The alias's `HostInstallOnly` block does not tell the operator to
/// add an `Alias=` the copied shadow already holds: its second line says the
/// export copies those files and gives the two ways to make the link in the
/// image (Mark Russell, 2026-11-15). The same block on both routes and in
/// refine's row, and no `systemctl` instruction names the unit.
#[test]
fn t32_a_host_only_alias_is_advised_without_adding_alias() {
    const ALIAS: &str = "app-alias.service";
    let snap = scan(
        diff_host(DIFF_UNIT, WANTS_MULTI_USER, "enabled")
            .etc_unit(
                DIFF_UNIT,
                "[Service]\nExecStart=/bin/true\n\n[Install]\nAlias=app-alias.service\n",
            )
            .link(&format!("/etc/systemd/system/{ALIAS}"), DIFF_SHADOW),
    );
    let expected = [
        format!(
            "# Advisory: {ALIAS}, a link to {DIFF_UNIT}, is on the source host because \
             {DIFF_UNIT}'s files under /etc/systemd/system name it, and the package's unit does \
             not. The image was linked from the package's unit, so it lacks this link. Not \
             carried."
        ),
        format!(
            "#   The export copies those files. To carry it, make the link in the Containerfile \
             after the configuration COPY, or run systemctl enable on {DIFF_UNIT} after that \
             COPY, which also makes every other link its [Install] section and Also= name: check \
             those first."
        ),
    ];
    let refine = session(&snap);
    let row = refine
        .decisions()
        .manual_links
        .iter()
        .find(|r| r.parent.as_str() == ALIAS)
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "the alias's refine row: {:?}",
                refine.decisions().manual_links
            )
        });
    assert_eq!(row.cause, ManualLinkCause::HostInstallOnly);
    assert_eq!(row.advisories, expected, "the refine row");
    for (route, containerfile) in both_routes(&snap) {
        assert_eq!(
            block_from(&containerfile, &expected[0]),
            expected,
            "{route}:\n{containerfile}"
        );
        assert!(
            !containerfile.contains("add Alias="),
            "{route}:\n{containerfile}"
        );
        assert!(
            in_no_systemctl_instruction(&containerfile, DIFF_UNIT),
            "{route}:\n{containerfile}"
        );
    }
}

/// T32 for T30 (d), the shadow replacing `multi-user.target` with
/// `graphical.target`: the `HostInstallOnly` block with its `add-wants`
/// line and the `VendorInstallOnly` block with its quoted `rm` line on both
/// routes, no `systemctl` instruction naming the unit, and two refine rows
/// holding the same blocks with no include field and nothing not applied.
/// A unit named with `\x2d` keeps its backslashes inside the quotes. With
/// the shadow excluded: no `rm` or `add-wants` line, no command for the
/// unit, one `# Omitted:` line naming the unit and the shadow, and both
/// rows `not_applied` with the shadow and that line; included again, both
/// blocks return and the line goes.
#[test]
fn t32_a_replaced_link_is_advised_both_ways() {
    for unit in [DIFF_UNIT, "my\\x2dapp.service"] {
        let shadow = format!("/etc/systemd/system/{unit}");
        let snap = scan(
            wants_link(
                diff_host(unit, WANTS_MULTI_USER, "enabled"),
                "graphical.target",
                unit,
            )
            .etc_unit(unit, WANTS_GRAPHICAL),
        );
        let host_only = host_only_block("graphical.target", unit);
        let vendor_only = vendor_only_block(
            &format!("multi-user.target.wants/{unit}"),
            unit,
            &format!("/etc/systemd/system/multi-user.target.wants/{unit}"),
        );
        let refine = session(&snap);
        let rows = &refine.decisions().manual_links;
        assert_eq!(rows.len(), 2, "{unit}: {rows:?}");
        assert_eq!(
            (rows[0].cause, rows[1].cause),
            (
                ManualLinkCause::HostInstallOnly,
                ManualLinkCause::VendorInstallOnly
            ),
            "{unit}"
        );
        assert_eq!(rows[0].advisories, host_only, "{unit}");
        assert_eq!(rows[1].advisories, vendor_only, "{unit}");
        for row in rows {
            let value = serde_json::to_value(row).expect("the row serializes");
            assert!(!has_include(value), "{unit}: the row has no toggle");
            assert_eq!(
                not_applied_of(serde_json::to_value(row).expect("the row serializes")),
                Vec::<String>::new(),
                "{unit}"
            );
        }
        for (route, containerfile) in both_routes(&snap) {
            assert_eq!(
                block_from(&containerfile, &host_only[0]),
                host_only,
                "{unit} {route}:\n{containerfile}"
            );
            assert_eq!(
                block_from(&containerfile, &vendor_only[0]),
                vendor_only,
                "{unit} {route}:\n{containerfile}"
            );
            assert!(
                in_no_systemctl_instruction(&containerfile, unit),
                "{unit} {route}:\n{containerfile}"
            );
        }

        let excluded = session_after(&snap, &[(drop_in_item(&shadow), false)]);
        let line = install_omitted(unit, &[&shadow]);
        let containerfile = containerfile_in(&export_session(&excluded));
        assert!(
            !containerfile.contains("RUN rm --"),
            "{unit}:\n{containerfile}"
        );
        assert!(
            !containerfile.contains("add-wants"),
            "{unit}:\n{containerfile}"
        );
        assert!(
            in_no_systemctl_instruction(&containerfile, unit),
            "{unit}:\n{containerfile}"
        );
        assert_eq!(
            count_lines(&containerfile, &line),
            1,
            "{unit}:\n{containerfile}"
        );
        for row in &excluded.decisions().manual_links {
            assert_eq!(
                not_applied_of(serde_json::to_value(row).expect("the row serializes")),
                std::slice::from_ref(&shadow),
                "{unit}"
            );
            assert_eq!(row.advisories, std::slice::from_ref(&line), "{unit}");
        }

        let restored = containerfile_in(&exported_after(
            &snap,
            &[
                (drop_in_item(&shadow), false),
                (drop_in_item(&shadow), true),
            ],
        ));
        assert_eq!(
            block_from(&restored, &host_only[0]),
            host_only,
            "{unit}:\n{restored}"
        );
        assert_eq!(
            block_from(&restored, &vendor_only[0]),
            vendor_only,
            "{unit}:\n{restored}"
        );
        assert_eq!(count_lines(&restored, &line), 0, "{unit}:\n{restored}");
    }
}

/// T32 for T30 (f), the first case of (k), (g) and (m): the removal line,
/// with the alias's own path for (m), and no `systemctl` instruction naming
/// the unit; for (g) the `Also` text and no command line.
#[test]
fn t32_package_only_links_and_also_names_are_advised() {
    let f = wants_link(
        diff_host(DIFF_UNIT, WANTS_MULTI_USER, "enabled"),
        "multi-user.target",
        DIFF_UNIT,
    )
    .file(
        "/usr/lib/systemd/system/app.service.d/10-extra.conf",
        "[Install]\nWantedBy=extra.target\n",
    )
    .file(&etc_drop_in(DIFF_UNIT, "10-extra.conf"), SERVICE_DROP_IN);
    let k = diff_host(DIFF_UNIT, WANTS_MULTI_USER, "static").file(
        &etc_drop_in(DIFF_UNIT, "10-reset.conf"),
        "[Install]\nWantedBy=\n",
    );
    let m = wants_link(
        diff_host(
            DIFF_UNIT,
            "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\nAlias=x-alias.service\n",
            "enabled",
        ),
        "multi-user.target",
        DIFF_UNIT,
    )
    .file(&etc_drop_in(DIFF_UNIT, "10-alias.conf"), "[Install]\nAlias=\n");
    let cases = [
        (
            "(f)",
            f,
            vendor_only_block(
                "extra.target.wants/app.service",
                DIFF_UNIT,
                "/etc/systemd/system/extra.target.wants/app.service",
            ),
        ),
        (
            "(k)",
            k,
            vendor_only_block(
                "multi-user.target.wants/app.service",
                DIFF_UNIT,
                "/etc/systemd/system/multi-user.target.wants/app.service",
            ),
        ),
        (
            "(m)",
            m,
            vendor_only_block(
                "x-alias.service, a link to app.service,",
                DIFF_UNIT,
                "/etc/systemd/system/x-alias.service",
            ),
        ),
    ];
    for (case, host, block) in cases {
        let snap = scan(host);
        assert_eq!(session(&snap).decisions().manual_links.len(), 1, "{case}");
        for (route, containerfile) in both_routes(&snap) {
            assert_eq!(
                block_from(&containerfile, &block[0]),
                block,
                "{case} {route}:\n{containerfile}"
            );
            assert!(
                in_no_systemctl_instruction(&containerfile, DIFF_UNIT),
                "{case} {route}:\n{containerfile}"
            );
        }
    }

    let g = wants_link(
        diff_host(
            DIFF_UNIT,
            "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\nAlso=helper.service\n",
            "enabled",
        ),
        "multi-user.target",
        DIFF_UNIT,
    )
    .etc_unit(DIFF_UNIT, WANTS_MULTI_USER);
    let snap = scan(g);
    let also = [
        "# Advisory: app.service's package unit names helper.service in Also=, and its files \
         under /etc/systemd/system do not. The package install in the image acted on the \
         package's list."
            .to_owned(),
        "#   Check helper.service with systemctl is-enabled in the image.".to_owned(),
    ];
    let refine = session(&snap);
    let rows = &refine.decisions().manual_links;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].advisories, also);
    for (route, containerfile) in both_routes(&snap) {
        let block = block_from(&containerfile, &also[0]);
        assert_eq!(block, also, "(g) {route}:\n{containerfile}");
        assert!(
            block.iter().all(|l| !l.contains("RUN ")),
            "(g) {route}: no command line"
        );
        assert!(
            in_no_systemctl_instruction(&containerfile, DIFF_UNIT),
            "(g) {route}:\n{containerfile}"
        );
    }
}

/// A removal line never deletes a file the export copies. The package's
/// `app.service` names `Alias=compat.service`, an `/etc` drop-in resets
/// it, and the host has its own regular `compat.service` under `/etc`,
/// which the export copies. The `VendorInstallOnly` finding for the alias
/// then says that file replaces the link in the image, with no `rm` line,
/// on both routes and in refine. Control: with no such file, the same
/// finding gets the removal line, which removes only a link.
#[test]
fn a_removal_line_never_deletes_a_staged_file() {
    const COMPAT: &str = "/etc/systemd/system/compat.service";
    let host = || {
        wants_link(
            diff_host(
                DIFF_UNIT,
                "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\nAlias=compat.service\n",
                "enabled",
            ),
            "multi-user.target",
            DIFF_UNIT,
        )
        .file(&etc_drop_in(DIFF_UNIT, "10-alias.conf"), "[Install]\nAlias=\n")
    };
    let subject = "compat.service, a link to app.service,";

    let control = scan(host());
    let block = vendor_only_block(subject, DIFF_UNIT, COMPAT);
    for (route, containerfile) in both_routes(&control) {
        assert_eq!(
            block_from(&containerfile, &block[0]),
            block,
            "control {route}:\n{containerfile}"
        );
    }

    let snap = scan(host().etc_unit("compat.service", "[Service]\nExecStart=/usr/bin/compat\n"));
    let expected = [
        block[0].clone(),
        format!(
            "#   The export copies a file to {COMPAT}, which replaces this link in the image, \
             so no line removes it."
        ),
    ];
    let refine = session(&snap);
    let rows = &refine.decisions().manual_links;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].advisories, expected);
    for (route, tree) in both_exports(&snap) {
        assert!(staged_at(&tree, COMPAT), "{route}: the file is copied");
        let containerfile = containerfile_in(&tree);
        assert_eq!(
            block_from(&containerfile, &expected[0]),
            expected,
            "{route}:\n{containerfile}"
        );
        assert!(
            !containerfile.contains("rm --"),
            "{route}:\n{containerfile}"
        );
    }
}

/// T41: `WantedBy=multi-user.target` in the vendor unit, an `/etc`
/// `10-reset.conf` holding `WantedBy=` and `20-add.conf` holding
/// `WantedBy=graphical.target`, and a `graphical.target.wants/` link only.
/// With both files staged, the two blocks. With only `20-add.conf`
/// excluded through refine: no `add-wants` or `rm` line, one `# Omitted:`
/// line naming the unit and `20-add.conf` alone, and both rows
/// `not_applied` with that path and the line. Included again, both blocks
/// return and the line goes.
#[test]
fn t41_a_partly_excluded_set_of_install_files() {
    let reset = etc_drop_in(DIFF_UNIT, "10-reset.conf");
    let add = etc_drop_in(DIFF_UNIT, "20-add.conf");
    let snap = scan(
        wants_link(
            diff_host(DIFF_UNIT, WANTS_MULTI_USER, "enabled"),
            "graphical.target",
            DIFF_UNIT,
        )
        .file(&reset, "[Install]\nWantedBy=\n")
        .file(&add, "[Install]\nWantedBy=graphical.target\n"),
    );
    let host_only = host_only_block("graphical.target", DIFF_UNIT);
    let vendor_only = vendor_only_block(
        "multi-user.target.wants/app.service",
        DIFF_UNIT,
        "/etc/systemd/system/multi-user.target.wants/app.service",
    );
    for (route, containerfile) in both_routes(&snap) {
        assert_eq!(
            block_from(&containerfile, &host_only[0]),
            host_only,
            "{route}:\n{containerfile}"
        );
        assert_eq!(
            block_from(&containerfile, &vendor_only[0]),
            vendor_only,
            "{route}:\n{containerfile}"
        );
    }

    let excluded = session_after(&snap, &[(drop_in_item(&add), false)]);
    let line = install_omitted(DIFF_UNIT, &[&add]);
    let containerfile = containerfile_in(&export_session(&excluded));
    assert!(!containerfile.contains("add-wants"), "{containerfile}");
    assert!(!containerfile.contains("RUN rm --"), "{containerfile}");
    assert_eq!(count_lines(&containerfile, &line), 1, "{containerfile}");
    let rows = &excluded.decisions().manual_links;
    assert_eq!(rows.len(), 2, "{rows:?}");
    for row in rows {
        assert_eq!(
            not_applied_of(serde_json::to_value(row).expect("the row serializes")),
            std::slice::from_ref(&add)
        );
        assert_eq!(row.advisories, std::slice::from_ref(&line));
    }

    let restored = containerfile_in(&exported_after(
        &snap,
        &[(drop_in_item(&add), false), (drop_in_item(&add), true)],
    ));
    assert_eq!(
        block_from(&restored, &host_only[0]),
        host_only,
        "{restored}"
    );
    assert_eq!(
        block_from(&restored, &vendor_only[0]),
        vendor_only,
        "{restored}"
    );
    assert_eq!(count_lines(&restored, &line), 0, "{restored}");
}

/// T40 with the drop-in excluded: the `RequiredBy=` reset unit's
/// `# Omitted:` line naming it and the drop-in, in place of its block, and
/// the `undetermined_installs[]` row `not_applied` with the path and that
/// line. Control: with the drop-in staged, the block and no line.
#[test]
fn t40_an_excluded_install_file_omits_the_undetermined_block() {
    let required = "[Service]\nExecStart=/bin/true\n\n[Install]\nRequiredBy=a.target\n";
    let snap = scan(t40_host(required, Some("[Install]\nRequiredBy=\n")));
    let line = install_omitted("need.service", &[T40_DROP_IN]);
    let block_start = "# Advisory: need.service is not carried:";

    let staged = containerfile_in(&export_session(&session(&snap)));
    assert!(
        staged.lines().any(|l| l.starts_with(block_start)),
        "{staged}"
    );
    assert_eq!(count_lines(&staged, &line), 0, "{staged}");

    let excluded = session_after(&snap, &[(drop_in_item(T40_DROP_IN), false)]);
    let containerfile = containerfile_in(&export_session(&excluded));
    assert_eq!(count_lines(&containerfile, &line), 1, "{containerfile}");
    assert!(
        !containerfile.lines().any(|l| l.starts_with(block_start)),
        "{containerfile}"
    );
    assert!(
        in_no_systemctl_instruction(&containerfile, "need.service"),
        "{containerfile}"
    );
    let rows = &excluded.decisions().undetermined_installs;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        not_applied_of(serde_json::to_value(&rows[0]).expect("the row serializes")),
        [T40_DROP_IN.to_owned()]
    );
    assert_eq!(rows[0].advisories, [line]);
}

// ---------------------------------------------------------------------
// § 7.5, § 10.2 rule 6 and § 10.8: template instances are collected and
// carried like any unit, an enable only with its configuration (T27, T22's
// instance repeats, T39's state changes, T23's device link, T24's
// instance cases, T28 and T29, through the scan).
// ---------------------------------------------------------------------

/// The vendor template of the instance hosts.
const T_TEMPLATE: &str = "t@.service";
const T_TEMPLATE_PATH: &str = "/usr/lib/systemd/system/t@.service";

/// § 7.5's attention note for an instance of a template on no list whose
/// unit file names nothing it reads, as the collector records it.
const INSTANCE_READS_UNKNOWN: &str = "inspectah does not know what this instance reads: its \
     template is on no list, and its unit file names no path under /etc or /var for it.";

/// The note the plan adds to [`INSTANCE_READS_UNKNOWN`] on the refine row
/// once the instance's enable is planned.
const INSTANCE_CARRIED_ALONE: &str = "It is carried on its unit file alone.";

/// A template `t@.service` wanted by `multi-user.target`, with
/// `DefaultInstance=` when given.
fn t_template(default_instance: Option<&str>) -> String {
    let default = default_instance
        .map(|i| format!("DefaultInstance={i}\n"))
        .unwrap_or_default();
    format!(
        "[Service]\nExecStart=/usr/bin/t %i\n\n[Install]\nWantedBy=multi-user.target\n{default}"
    )
}

/// `host` with a link to the instance `unit` in `<parent>.wants` under
/// `root`, pointing at `template_path`, as `systemctl enable` makes it.
fn instance_link(
    host: UnitHost,
    root: &str,
    parent: &str,
    unit: &str,
    template_path: &str,
) -> UnitHost {
    host.link(&format!("{root}/{parent}.wants/{unit}"), template_path)
}

/// A host shipping only `t@.service`, listed `indirect` as systemd lists a
/// template with linked instances, under `presets`, with each instance in
/// `linked` linked from `multi-user.target.wants` under `/etc`.
fn template_host(presets: &[&str], default_instance: Option<&str>, linked: &[&str]) -> UnitHost {
    let mut host = UnitHost::new().unit(T_TEMPLATE, "indirect").vendor_unit(
        T_TEMPLATE,
        &t_template(default_instance),
        None,
    );
    for rule in presets {
        host = host.preset(rule);
    }
    for instance in linked {
        host = instance_link(
            host,
            "/etc/systemd/system",
            "multi-user.target",
            &format!("t@{instance}.service"),
            T_TEMPLATE_PATH,
        );
    }
    host
}

/// A host whose package ships the concrete `t@two.service`, listed in
/// `state`, under `presets`. Listed `enabled`, it has the link `enable`
/// makes.
fn own_file_host(presets: &[&str], state: &str) -> UnitHost {
    let mut host = UnitHost::new().unit("t@two.service", state).vendor_unit(
        "t@two.service",
        &t_template(None),
        None,
    );
    for rule in presets {
        host = host.preset(rule);
    }
    host
}

/// `unit`'s inventory entry, if any.
fn inventory_entry<'a>(
    snap: &'a InspectionSnapshot,
    unit: &str,
) -> Option<&'a inspectah_core::types::units::UnitInventoryEntry> {
    snap.units
        .as_ref()
        .expect("a units section")
        .inventory
        .iter()
        .find(|e| e.unit.as_str() == unit)
}

/// How many state changes name `unit`.
fn changes_named(snap: &InspectionSnapshot, unit: &str) -> usize {
    snap.units
        .as_ref()
        .expect("a units section")
        .state_changes
        .iter()
        .filter(|c| c.unit().name().as_str() == unit)
        .count()
}

/// `unit`'s row is carried with its own verb on both routes, with no
/// `# Omitted:` line, and its refine row declines nothing.
fn assert_instance_carried(snap: &InspectionSnapshot, unit: &str, label: &str) {
    let verb = change(snap, unit).implied_action().to_string();
    assert_eq!(
        declined_in(&session(snap), unit),
        None,
        "{label}: the refine row"
    );
    for (route, containerfile) in both_routes(snap) {
        assert!(
            acts(&containerfile, &verb, unit),
            "{label}, {route}:\n{containerfile}"
        );
        assert!(
            omitted(&containerfile, unit).is_none(),
            "{label}, {route}:\n{containerfile}"
        );
    }
}

/// `unit`'s row is declined on both routes with the `# Omitted:` line
/// `line`, its refine row holds the same text under `reason`, and no
/// `systemctl` instruction names it.
fn assert_instance_omitted(snap: &InspectionSnapshot, unit: &str, line: &str, reason: &str) {
    for (route, containerfile) in both_routes(snap) {
        assert_eq!(
            omitted(&containerfile, unit),
            Some(line),
            "{route}:\n{containerfile}"
        );
        assert!(
            in_no_systemctl_instruction(&containerfile, unit),
            "{route}:\n{containerfile}"
        );
    }
    assert_eq!(
        declined_in(&session(snap), unit),
        Some(inspectah_refine::projection::DeclinedView {
            renders: inspectah_refine::projection::DeclinedRenders::Omitted,
            reason: reason.into(),
            text: line
                .strip_prefix("# Omitted: ")
                .expect("an omitted line")
                .to_owned(),
        })
    );
}

/// `unit`'s row as the refine view shows it.
fn unit_row(snap: &InspectionSnapshot, unit: &str) -> inspectah_refine::projection::UnitStateView {
    session(snap)
        .decisions()
        .unit_states
        .iter()
        .find(|r| r.unit.as_str() == unit)
        .cloned()
        .unwrap_or_else(|| panic!("no unit row for {unit}"))
}

/// T27: an enabled instance found through the directory scan, its only
/// trace a link under `/etc`, has a state change with the template as its
/// file and an inventory entry, and a refine row. Its template names no
/// path for it and is on no list, so its enable is carried on both routes
/// with § 7.5's attention note on the row and in refine. Linked from two
/// parents, it is one state change and one operand. With an
/// `EnvironmentFile=` naming `/etc/t/%i.conf` and no such file, rule 6
/// omits the enable; with the file, it is carried and has no note.
#[test]
fn t27_an_enabled_instance_found_by_its_link_is_carried() {
    const UNIT: &str = "t@site.service";
    let snap = scan(template_host(&["disable *"], None, &["site"]));
    assert_eq!(change(&snap, UNIT).current_state(), UnitState::Enabled);
    assert_eq!(
        fragment_of(&snap, UNIT),
        &UnitFragment::Vendor {
            file: name(T_TEMPLATE),
            owning_package: None,
        }
    );
    assert_eq!(
        change(&snap, UNIT).attention_reason.as_deref(),
        Some(INSTANCE_READS_UNKNOWN)
    );
    assert_eq!(
        unit_row(&snap, UNIT).attention_reason,
        Some(format!("{INSTANCE_READS_UNKNOWN} {INSTANCE_CARRIED_ALONE}"))
    );
    assert_instance_carried(&snap, UNIT, "one parent");

    let two_parents = "[Service]\nExecStart=/usr/bin/t %i\n\n[Install]\n\
                       WantedBy=multi-user.target graphical.target\n";
    let host = UnitHost::new()
        .unit(T_TEMPLATE, "indirect")
        .preset("disable *")
        .vendor_unit(T_TEMPLATE, two_parents, None);
    let host = instance_link(
        host,
        "/etc/systemd/system",
        "multi-user.target",
        UNIT,
        T_TEMPLATE_PATH,
    );
    let host = instance_link(
        host,
        "/etc/systemd/system",
        "graphical.target",
        UNIT,
        T_TEMPLATE_PATH,
    );
    let snap = scan(host);
    assert_eq!(changes_named(&snap, UNIT), 1);
    assert_eq!(change(&snap, UNIT).current_state(), UnitState::Enabled);
    assert!(links_of(&snap).is_empty(), "{:?}", links_of(&snap));
    assert_instance_carried(&snap, UNIT, "two parents");
    for (route, containerfile) in both_routes(&snap) {
        assert_eq!(
            count_lines(&containerfile, &format!("    '{UNIT}'")),
            1,
            "{route}:\n{containerfile}"
        );
    }

    // Rule 6: the template names the instance's configuration.
    let reads = "[Service]\nEnvironmentFile=/etc/t/%i.conf\nExecStart=/usr/bin/t %i\n\n\
                 [Install]\nWantedBy=multi-user.target\n";
    let host = |with_file: bool| {
        let host = instance_link(
            UnitHost::new()
                .unit(T_TEMPLATE, "indirect")
                .preset("disable *")
                .vendor_unit(T_TEMPLATE, reads, None),
            "/etc/systemd/system",
            "multi-user.target",
            UNIT,
            T_TEMPLATE_PATH,
        );
        if with_file {
            host.file("/etc/t/site.conf", "PORT=1\n")
        } else {
            host
        }
    };
    let missing = scan(host(false));
    assert_eq!(change(&missing, UNIT).required_config, ["/etc/t/site.conf"]);
    assert_instance_omitted(
        &missing,
        UNIT,
        "# Omitted: t@site.service (its configuration /etc/t/site.conf is not in the export)",
        "InstanceConfigNotStaged",
    );
    let present = scan(host(true));
    assert_eq!(change(&present, UNIT).attention_reason, None);
    assert_instance_carried(&present, UNIT, "configuration present");
}

/// R3: the unknown-template note claims carriage only for an enable the
/// plan carries. `worker@a.timer`'s template is on no list and names
/// nothing it reads, so its row has the collector's note; carried, refine
/// adds that it is carried on its unit file alone. With its trigger's file
/// excluded, the enable is declined as `TimerTriggerNotStaged` and the row
/// keeps the collector's note with no carriage claim. With its unit file
/// excluded, likewise.
#[test]
fn the_unknown_template_note_claims_carriage_only_when_carried() {
    let snap = scan(t37_instance_host(""));
    assert_eq!(
        change(&snap, WORKER_INSTANCE_TIMER)
            .attention_reason
            .as_deref(),
        Some(INSTANCE_READS_UNKNOWN)
    );
    assert_eq!(
        unit_row(&snap, WORKER_INSTANCE_TIMER).attention_reason,
        Some(format!("{INSTANCE_READS_UNKNOWN} {INSTANCE_CARRIED_ALONE}"))
    );
    for (label, file, reason) in [
        ("trigger", "worker@.service", "TimerTriggerNotStaged"),
        ("unit file", "worker@.timer", "UnitFileNotStaged"),
    ] {
        let excluded = [(config_item(&etc_path(file)), false)];
        let refine = session_after(&snap, &excluded);
        assert_eq!(
            declined_in(&refine, WORKER_INSTANCE_TIMER).map(|d| d.reason),
            Some(reason.to_owned()),
            "{label}"
        );
        let row = refine
            .decisions()
            .unit_states
            .iter()
            .find(|r| r.unit.as_str() == WORKER_INSTANCE_TIMER)
            .cloned()
            .expect("the row");
        assert_eq!(
            row.attention_reason.as_deref(),
            Some(INSTANCE_READS_UNKNOWN),
            "{label}"
        );
    }
}

/// Rule 6 (§ 10.8): a configuration path the template names with a `..`
/// component is kept as written, so its enable is never carried with the
/// path unchecked. inspectah cannot tell which file such a path reads, so
/// the enable is omitted with that reason, naming the path, on both routes
/// and in refine, although the file it reads as is present, clean and
/// staged. Control: the same template naming the plain path carries it.
#[test]
fn an_instance_configuration_path_with_a_parent_component_omits_the_enable() {
    const UNIT: &str = "t@site.service";
    let reads = "[Service]\nEnvironmentFile=/etc/t/../t/%i.conf\nExecStart=/usr/bin/t %i\n\n\
                 [Install]\nWantedBy=multi-user.target\n";
    let snap = scan(
        instance_link(
            UnitHost::new()
                .unit(T_TEMPLATE, "indirect")
                .preset("disable *")
                .vendor_unit(T_TEMPLATE, reads, None),
            "/etc/systemd/system",
            "multi-user.target",
            UNIT,
            T_TEMPLATE_PATH,
        )
        .file("/etc/t/site.conf", "PORT=1\n"),
    );
    assert_eq!(
        change(&snap, UNIT).required_config,
        ["/etc/t/../t/site.conf"]
    );
    assert_instance_omitted(
        &snap,
        UNIT,
        "# Omitted: t@site.service (its configuration path /etc/t/../t/site.conf holds a .. \
         component, so inspectah cannot tell which file it reads)",
        "InstanceConfigNotStaged",
    );
    for (route, tree) in both_exports(&snap) {
        assert!(
            staged_at(&tree, "/etc/t/site.conf"),
            "{route}: the file it reads as is staged"
        );
    }

    let plain = scan(t_reads_host(
        "EnvironmentFile=/etc/t/%i.conf\nExecStart=/usr/bin/t %i",
        &[("/etc/t/site.conf", "PORT=1\n")],
    ));
    assert_instance_carried(&plain, UNIT, "the plain path");
}

/// The host of `t@site.service`, linked under `/etc` and enabled against
/// `disable *`, whose vendor template's `[Service]` holds `service`, with
/// each of `files` (path, content) on the host.
fn t_reads_host(service: &str, files: &[(&str, &str)]) -> UnitHost {
    let text = format!("[Service]\n{service}\n\n[Install]\nWantedBy=multi-user.target\n");
    let mut host = instance_link(
        UnitHost::new()
            .unit(T_TEMPLATE, "indirect")
            .preset("disable *")
            .vendor_unit(T_TEMPLATE, &text, None),
        "/etc/systemd/system",
        "multi-user.target",
        "t@site.service",
        T_TEMPLATE_PATH,
    );
    for (path, content) in files {
        host = host.file(path, content);
    }
    host
}

/// Rule 6 through the scan, with a command line read as systemd reads it:
/// the quoted and unquoted forms of one configuration argument record the
/// same path, and so do a quoted path holding a space and a path holding
/// `:`. For each form, the configuration missing omits the enable on both
/// routes and in refine, the configuration excluded through refine omits
/// it and including it again restores it, and the configuration clean and
/// present carries it.
#[test]
fn a_quoted_configuration_argument_is_checked_through_the_scan() {
    const UNIT: &str = "t@site.service";
    for (service, path) in [
        (
            "ExecStart=/usr/bin/t --config=/etc/t/%i.conf",
            "/etc/t/site.conf",
        ),
        (
            "ExecStart=/usr/bin/t --config=\"/etc/t/%i.conf\"",
            "/etc/t/site.conf",
        ),
        (
            "ExecStart=/usr/bin/t --config \"/etc/t sites/%i.conf\"",
            "/etc/t sites/site.conf",
        ),
        (
            "ExecStart=/usr/bin/t /etc/t:secrets/%i.conf",
            "/etc/t:secrets/site.conf",
        ),
    ] {
        let missing = scan(t_reads_host(service, &[]));
        assert_eq!(change(&missing, UNIT).required_config, [path], "{service}");
        assert_instance_omitted(
            &missing,
            UNIT,
            &config_omitted(UNIT, path, "is not in the export"),
            "InstanceConfigNotStaged",
        );

        let clean = scan(t_reads_host(service, &[(path, "PORT=1\n")]));
        assert_eq!(change(&clean, UNIT).required_config, [path], "{service}");
        assert_instance_carried(&clean, UNIT, service);

        let excluded = (config_item(path), false);
        let containerfile =
            containerfile_in(&exported_after(&clean, std::slice::from_ref(&excluded)));
        let line = config_omitted(UNIT, path, "is excluded from the export");
        assert_eq!(
            omitted(&containerfile, UNIT),
            Some(line.as_str()),
            "{service}:\n{containerfile}"
        );
        assert!(
            in_no_systemctl_instruction(&containerfile, UNIT),
            "{service}:\n{containerfile}"
        );
        let declined = declined_in(
            &session_after(&clean, std::slice::from_ref(&excluded)),
            UNIT,
        )
        .expect("a declined row");
        assert_eq!(format!("# Omitted: {}", declined.text), line);
        let restored = containerfile_in(&exported_after(
            &clean,
            &[excluded.clone(), (excluded.0, true)],
        ));
        assert!(acts(&restored, "enable", UNIT), "{service}:\n{restored}");
    }
}

/// Rule 6 through the scan, a required directory with a member the
/// stager refuses: the template names `/etc/t/%i.d`, which holds a good
/// file and one whose name holds a line break. The stager refuses that one,
/// so the enable is omitted naming it on both routes and in refine, though
/// the good file is staged. Control: the member renamed, the enable is
/// carried.
#[test]
fn a_refused_member_of_a_required_directory_omits_the_enable() {
    const UNIT: &str = "t@site.service";
    const GOOD: &str = "/etc/t/site.d/good.conf";
    const REFUSED: &str = "/etc/t/site.d/bad\nname.conf";
    let service = "ExecStart=/usr/bin/t --conf-dir=/etc/t/%i.d";
    let snap = scan(t_reads_host(
        service,
        &[(GOOD, "A=1\n"), (REFUSED, "B=2\n")],
    ));
    assert_eq!(change(&snap, UNIT).required_config, ["/etc/t/site.d"]);
    assert!(
        snap.config
            .iter()
            .flat_map(|c| &c.files)
            .any(|f| f.path == REFUSED),
        "presence: the refused member is a config entry"
    );
    for (route, tree) in both_exports(&snap) {
        assert!(staged_at(&tree, GOOD), "{route}: the good member is staged");
        let containerfile = containerfile_in(&tree);
        let line = omitted(&containerfile, UNIT)
            .unwrap_or_else(|| panic!("{route}: an omitted line:\n{containerfile}"));
        assert!(
            line.starts_with("# Omitted: t@site.service (its configuration /etc/t/site.d/bad")
                && line.ends_with("name.conf is excluded from the export)"),
            "{route}: {line}"
        );
        assert!(
            in_no_systemctl_instruction(&containerfile, UNIT),
            "{route}:\n{containerfile}"
        );
    }
    let declined = declined_in(&session(&snap), UNIT).expect("a declined row");
    assert_eq!(declined.reason, "InstanceConfigNotStaged");
    assert_eq!(
        declined.text,
        format!("{UNIT} (its configuration {REFUSED} is excluded from the export)")
    );

    let renamed = scan(t_reads_host(
        service,
        &[(GOOD, "A=1\n"), ("/etc/t/site.d/badname.conf", "B=2\n")],
    ));
    assert_instance_carried(&renamed, UNIT, "renamed");
}

/// Rule 6 through the scan, with systemd's resets: the template names
/// `/etc/t/old/%i.env`, and the instance's drop-in under `/etc` resets
/// `EnvironmentFile=` and names nothing else, so with the old file missing
/// the enable is carried on both routes and in refine. Control: the drop-in
/// without the reset line, the old file is required and the enable is
/// omitted naming it.
#[test]
fn a_reset_environment_file_is_not_required_through_the_scan() {
    const UNIT: &str = "t@site.service";
    const DROP_IN: &str = "/etc/systemd/system/t@site.service.d/10-env.conf";
    let host = |drop_in: &str| {
        t_reads_host(
            "EnvironmentFile=/etc/t/old/%i.env\nExecStart=/usr/bin/t %i",
            &[(DROP_IN, drop_in)],
        )
    };
    let reset = scan(host("[Service]\nEnvironmentFile=\n"));
    assert!(change(&reset, UNIT).required_config.is_empty());
    assert_instance_carried(&reset, UNIT, "reset");

    let control = scan(host("[Service]\nNice=5\n"));
    assert_eq!(
        change(&control, UNIT).required_config,
        ["/etc/t/old/site.env"]
    );
    assert_instance_omitted(
        &control,
        UNIT,
        &config_omitted(UNIT, "/etc/t/old/site.env", "is not in the export"),
        "InstanceConfigNotStaged",
    );
}

/// The `InstanceNeedsEvidenceChanged` line for `unit` and the drop-in
/// `path`.
fn needs_evidence_omitted(unit: &str, path: &str) -> String {
    format!(
        "# Omitted: {unit} (the drop-in {path} clears or replaces a path the instance reads and \
         is not in the export as the source host had it, so the instance in the image may read \
         a path inspectah did not check)"
    )
}

/// Rule 5 for what an instance reads, through the scan: the template
/// requires `/etc/t/old/%i.env` and a drop-in resets `EnvironmentFile=`.
/// With the reset drop-in under `/etc` and included, nothing is required
/// and the enable is carried (the positive control). Excluded through
/// refine, the image reads the template's requirement again, unchecked, so
/// the enable is omitted naming the drop-in, in the export and in refine,
/// and including it again restores the enable. The refine row's install
/// drop-ins do not list it: it holds no `[Install]`. A reset under `/run`,
/// which no export carries, omits the enable on both routes and in refine.
#[test]
fn a_reset_drop_in_the_export_does_not_carry_omits_the_enable() {
    const UNIT: &str = "t@site.service";
    const DROP_IN: &str = "/etc/systemd/system/t@site.service.d/10-env.conf";
    const RUN_RESET: &str = "/run/systemd/system/service.d/90-reset.conf";
    const RESET: &str = "[Service]\nEnvironmentFile=\n";
    let service = "EnvironmentFile=/etc/t/old/%i.env\nExecStart=/usr/bin/t %i";

    let included = scan(t_reads_host(service, &[(DROP_IN, RESET)]));
    let row = change(&included, UNIT);
    assert!(row.required_config.is_empty());
    assert_eq!(row.install_drop_ins, [DROP_IN]);
    assert_instance_carried(&included, UNIT, "the reset included");
    assert!(
        unit_row(&included, UNIT).install_drop_ins.is_empty(),
        "the reset drop-in is not an install file in refine"
    );

    let line = needs_evidence_omitted(UNIT, DROP_IN);
    let excluded = (drop_in_item(DROP_IN), false);
    let containerfile =
        containerfile_in(&exported_after(&included, std::slice::from_ref(&excluded)));
    assert_eq!(
        omitted(&containerfile, UNIT),
        Some(line.as_str()),
        "{containerfile}"
    );
    assert!(
        in_no_systemctl_instruction(&containerfile, UNIT),
        "{containerfile}"
    );
    let declined = declined_in(
        &session_after(&included, std::slice::from_ref(&excluded)),
        UNIT,
    )
    .expect("a declined row");
    assert_eq!(declined.reason, "InstanceNeedsEvidenceChanged");
    assert_eq!(format!("# Omitted: {}", declined.text), line);
    let restored = containerfile_in(&exported_after(
        &included,
        &[excluded.clone(), (excluded.0, true)],
    ));
    assert!(acts(&restored, "enable", UNIT), "{restored}");

    let run = scan(t_reads_host(service, &[(RUN_RESET, RESET)]));
    assert!(change(&run, UNIT).required_config.is_empty());
    assert_instance_omitted(
        &run,
        UNIT,
        &needs_evidence_omitted(UNIT, RUN_RESET),
        "InstanceNeedsEvidenceChanged",
    );
}

/// Rule 6 through the scan, with an override systemd ignores: a drop-in
/// replacing `WorkingDirectory=/etc/t/%i` with the relative `-relative`,
/// which systemd refuses and so keeps the earlier directory, and a drop-in
/// resetting `EnvironmentFile=` under `[Unit]`, a section that does not
/// take the key. For each, the requirement stands: the file missing omits
/// the enable on both routes and in refine, present it is carried, and
/// excluded through refine it is omitted naming it. The same for an
/// optional `WorkingDirectory=` replacement whose expansion is longer than
/// systemd's 4095 bytes but simplifies to `/srv` (`unit_path_printf`
/// refuses it before anything simplifies it, and the `-` keeps the earlier
/// directory). Control: the override systemd accepts (an absolute
/// directory, a short optional one, the reset under `[Service]`) removes
/// the requirement, and the enable is carried with the file missing.
#[test]
fn an_override_systemd_ignores_keeps_the_requirement_through_the_scan() {
    const UNIT: &str = "t@site.service";
    const DROP_IN: &str = "/etc/systemd/system/t@site.service.d/10-override.conf";
    let overlong = format!("[Service]\nWorkingDirectory=-/{}srv\n", "./".repeat(2048));
    for (label, service, ignored, accepted, path, file) in [
        (
            "an invalid WorkingDirectory= replacement",
            "WorkingDirectory=/etc/t/%i\nExecStart=/usr/bin/t",
            "[Service]\nWorkingDirectory=-relative\n",
            "[Service]\nWorkingDirectory=/srv/t\n",
            "/etc/t/site",
            "/etc/t/site/app.conf",
        ),
        (
            "an optional WorkingDirectory= replacement expanding past the limit",
            "WorkingDirectory=/etc/t/%i\nExecStart=/usr/bin/t",
            overlong.as_str(),
            "[Service]\nWorkingDirectory=-/srv/t\n",
            "/etc/t/site",
            "/etc/t/site/app.conf",
        ),
        (
            "an EnvironmentFile= reset under [Unit]",
            "EnvironmentFile=/etc/t/%i.env\nExecStart=/usr/bin/t %i",
            "[Unit]\nEnvironmentFile=\n",
            "[Service]\nEnvironmentFile=\n",
            "/etc/t/site.env",
            "/etc/t/site.env",
        ),
    ] {
        let missing = scan(t_reads_host(service, &[(DROP_IN, ignored)]));
        assert_eq!(change(&missing, UNIT).required_config, [path], "{label}");
        assert_instance_omitted(
            &missing,
            UNIT,
            &config_omitted(UNIT, path, "is not in the export"),
            "InstanceConfigNotStaged",
        );

        let present = scan(t_reads_host(
            service,
            &[(DROP_IN, ignored), (file, "A=1\n")],
        ));
        assert_instance_carried(&present, UNIT, label);
        let excluded = [(config_item(file), false)];
        let containerfile = containerfile_in(&exported_after(&present, &excluded));
        assert_eq!(
            omitted(&containerfile, UNIT),
            Some(config_omitted(UNIT, file, "is excluded from the export").as_str()),
            "{label}:\n{containerfile}"
        );
        assert!(
            in_no_systemctl_instruction(&containerfile, UNIT),
            "{label}:\n{containerfile}"
        );
        assert_eq!(
            declined_in(&session_after(&present, &excluded), UNIT).map(|d| d.reason),
            Some("InstanceConfigNotStaged".into()),
            "{label}"
        );

        let control = scan(t_reads_host(service, &[(DROP_IN, accepted)]));
        assert!(
            change(&control, UNIT).required_config.is_empty(),
            "{label}: control"
        );
        assert_instance_carried(&control, UNIT, &format!("{label}: control"));
    }
}

/// R4 through the scan: an instance's `EnvironmentFile=` set by a
/// type-wide drop-in (`/etc/systemd/system/service.d`) or a dash-prefix one
/// (`/etc/systemd/system/t-.service.d` for `t-job@site.service`) is
/// required. For each, the file missing omits the enable on both routes
/// and in refine, the file excluded through refine omits it, and the file
/// present and clean carries it.
#[test]
fn a_type_wide_or_dash_prefix_drop_in_sets_what_an_instance_needs() {
    const ENV: &str = "/etc/t/site.env";
    const DROP_IN: &str = "[Service]\nEnvironmentFile=/etc/t/%i.env\n";
    let template = "[Service]\nExecStart=/usr/bin/t %i\n\n[Install]\nWantedBy=multi-user.target\n";
    for (template_name, unit, drop_in) in [
        (
            "t@.service",
            "t@site.service",
            "/etc/systemd/system/service.d/50-env.conf",
        ),
        (
            "t-job@.service",
            "t-job@site.service",
            "/etc/systemd/system/t-.service.d/50-env.conf",
        ),
    ] {
        let host = |with_env: bool| {
            let host = instance_link(
                UnitHost::new()
                    .unit(template_name, "indirect")
                    .preset("disable *")
                    .vendor_unit(template_name, template, None),
                "/etc/systemd/system",
                "multi-user.target",
                unit,
                &format!("/usr/lib/systemd/system/{template_name}"),
            )
            .file(drop_in, DROP_IN);
            if with_env {
                host.file(ENV, "PORT=1\n")
            } else {
                host
            }
        };
        let missing = scan(host(false));
        assert_eq!(change(&missing, unit).required_config, [ENV], "{drop_in}");
        assert_instance_omitted(
            &missing,
            unit,
            &config_omitted(unit, ENV, "is not in the export"),
            "InstanceConfigNotStaged",
        );

        let present = scan(host(true));
        assert_instance_carried(&present, unit, drop_in);

        let excluded = [(config_item(ENV), false)];
        let containerfile = containerfile_in(&exported_after(&present, &excluded));
        assert_eq!(
            omitted(&containerfile, unit),
            Some(config_omitted(unit, ENV, "is excluded from the export").as_str()),
            "{drop_in}:\n{containerfile}"
        );
        assert!(
            in_no_systemctl_instruction(&containerfile, unit),
            "{drop_in}:\n{containerfile}"
        );
        assert_eq!(
            declined_in(&session_after(&present, &excluded), unit).map(|d| d.reason),
            Some("InstanceConfigNotStaged".into()),
            "{drop_in}"
        );
    }
}

/// T25 (c) with the member's configuration named by a quoted argument:
/// carried with the owner while the configuration is in the export, and
/// with it excluded, the member omitted naming the whole path and the
/// owner `RowNotCarried`.
#[test]
fn t25_c_a_member_whose_configuration_argument_is_quoted() {
    const MEMBER: &str = "t@two.service";
    const CONF: &str = "/etc/t sites/two.conf";
    let reads = "[Service]\nExecStart=/usr/bin/t --config=\"/etc/t sites/%i.conf\"\n\n\
                 [Install]\nWantedBy=multi-user.target\n";
    let host = rule7_host(
        &[],
        &[(BUNDLE, "enabled"), (T_TEMPLATE, "indirect")],
        &[
            (BUNDLE, &wanted_with(&format!("Also={MEMBER}\n"))),
            (T_TEMPLATE, reads),
        ],
    )
    .file(CONF, "PORT=2\n");
    let snap = scan(instance_link(
        host,
        "/etc/systemd/system",
        "multi-user.target",
        MEMBER,
        T_TEMPLATE_PATH,
    ));
    assert_eq!(change(&snap, MEMBER).required_config, [CONF]);
    assert_also_carried(&snap, MEMBER, "enable", "member");
    assert_also_carried(&snap, BUNDLE, "enable", "owner");

    let head = also_head(BUNDLE, "enable", MEMBER, ROW_NOT_CARRIED);
    let excluded = [(config_item(CONF), false)];
    let containerfile = containerfile_in(&exported_after(&snap, &excluded));
    assert_eq!(
        advisory_for(&containerfile, BUNDLE),
        std::slice::from_ref(&head),
        "{containerfile}"
    );
    assert_eq!(
        omitted(&containerfile, MEMBER),
        Some(config_omitted(MEMBER, CONF, "is excluded from the export").as_str()),
        "{containerfile}"
    );
    for unit in [BUNDLE, MEMBER] {
        assert!(
            in_no_systemctl_instruction(&containerfile, unit),
            "{unit}:\n{containerfile}"
        );
    }
}

/// § 7.5 through the scan: a command argument that names a path for the
/// instance and that systemd's parser refuses (an unbalanced quote) makes
/// what the instance reads unknown. The instance has no state change and
/// no refine row, no `systemctl` line names it on either route, and the
/// snapshot's warning names the unit, the setting and the file, never the
/// value. Control: the quote closed, the instance is carried.
#[test]
fn an_uninterpretable_instance_argument_declines_the_enable_through_the_scan() {
    const UNIT: &str = "t@site.service";
    let broken = "ExecStart=/usr/bin/t --config=\"/etc/t/%i.conf";
    let snap = scan(t_reads_host(broken, &[("/etc/t/site.conf", "PORT=1\n")]));
    assert_eq!(changes_named(&snap, UNIT), 0);
    assert!(
        session(&snap)
            .decisions()
            .unit_states
            .iter()
            .all(|r| r.unit.as_str() != UNIT),
        "no refine row"
    );
    for (route, containerfile) in both_routes(&snap) {
        assert!(
            in_no_systemctl_instruction(&containerfile, UNIT),
            "{route}:\n{containerfile}"
        );
    }
    assert!(
        snap.warnings.iter().any(|w| {
            let message = &w.message;
            message.contains(UNIT)
                && message.contains(&format!(
                    "its ExecStart= setting in {T_TEMPLATE_PATH} (line 2)"
                ))
                && message.contains("its enable is not carried")
                && !message.contains("--config")
        }),
        "{:?}",
        snap.warnings
    );

    let control = scan(t_reads_host(
        "ExecStart=/usr/bin/t --config=\"/etc/t/%i.conf\"",
        &[("/etc/t/site.conf", "PORT=1\n")],
    ));
    assert_instance_carried(&control, UNIT, "the quote closed");
}

/// Every file under `dir`, recursively.
fn files_under(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).expect("the export directory reads") {
        let path = entry.expect("an export entry").path();
        if path.is_dir() {
            files.extend(files_under(&path));
        } else {
            files.push(path);
        }
    }
    files
}

/// The unknown-needs warning names the unit, the setting and the file it
/// is in, with its line, and never the value, which can hold a credential
/// the redaction pass never sees in a warning. A command line holding a
/// token and a path inspectah cannot expand (`%E`) declines the enable,
/// and the token is in no exported artifact: not in the snapshot or its
/// warnings, not in any file either route writes (the snapshot JSON, the
/// Containerfile, the scan's README, refine's audit report and the rest),
/// and not in refine's view or
/// its Services section, where the warnings are listed.
#[test]
fn an_uninterpretable_value_is_reported_without_its_value() {
    const UNIT: &str = "t@site.service";
    const SENTINEL: &str = "inspectah-sentinel-credential-5b1e";
    let service = format!("ExecStart=/usr/bin/t --token={SENTINEL} --config=%E/t/%i.conf");
    let snap = scan(t_reads_host(&service, &[]));
    assert_eq!(changes_named(&snap, UNIT), 0);
    let warning = snap
        .warnings
        .iter()
        .find(|w| w.message.contains(UNIT) && w.message.contains("ExecStart="))
        .unwrap_or_else(|| panic!("a warning for {UNIT}: {:?}", snap.warnings));
    assert_eq!(
        warning.message,
        format!(
            "unit {UNIT} has state 'enabled' - its ExecStart= setting in {T_TEMPLATE_PATH} \
             (line 2) names a path for the instance that inspectah cannot read as systemd \
             does, so what the instance reads is unknown and its enable is not carried"
        )
    );
    assert_eq!(
        warning.extra.get("key"),
        Some(&serde_json::json!("ExecStart"))
    );
    assert_eq!(
        warning.extra.get("path"),
        Some(&serde_json::json!(T_TEMPLATE_PATH))
    );
    assert_eq!(warning.extra.get("line"), Some(&serde_json::json!(2)));

    let snapshot = serde_json::to_string(&snap).expect("the snapshot serializes");
    assert!(
        !snapshot.contains(SENTINEL),
        "the snapshot holds the credential"
    );
    for (route, tree) in both_exports(&snap) {
        let files = files_under(tree.path());
        assert!(
            files
                .iter()
                .any(|f| f.ends_with("inspection-snapshot.json"))
                && files.iter().any(|f| f.ends_with("Containerfile"))
                && (route != "scan" || files.iter().any(|f| f.ends_with("README.md"))),
            "{route}: the export holds the snapshot and Containerfile, and the scan's its \
             README: {files:?}"
        );
        for file in files {
            let bytes = std::fs::read(&file).expect("an export file reads");
            assert!(
                !String::from_utf8_lossy(&bytes).contains(SENTINEL),
                "{route}: {} holds the credential",
                file.display()
            );
        }
    }
    let session = session(&snap);
    let view = serde_json::to_string(&inspectah_web::adapter::build_web_view(&session))
        .expect("the view serializes");
    assert!(
        !view.contains(SENTINEL),
        "refine's view holds the credential"
    );
    let sections = inspectah_web::adapter::build_web_sections(
        &inspectah_refine::projection::project_reference(&session.snapshot_projected()),
    );
    let sections = serde_json::to_string(&sections).expect("the sections serialize");
    assert!(
        sections.contains(&format!("its ExecStart= setting in {T_TEMPLATE_PATH}")),
        "refine lists the warning"
    );
    assert!(
        !sections.contains(SENTINEL),
        "refine's sections hold the credential"
    );
}

/// T27: an instance linked only under `/run` is not found: no state
/// change, no inventory entry, no line. Control: the same link under
/// `/etc` is found.
#[test]
fn t27_an_instance_linked_only_under_run_is_not_found() {
    const UNIT: &str = "t@site.service";
    let control = scan(template_host(&["disable *"], None, &["site"]));
    assert_eq!(changes_named(&control, UNIT), 1);
    assert!(inventory_entry(&control, UNIT).is_some());

    let host = template_host(&["disable *"], None, &[]);
    let snap = scan(instance_link(
        host,
        "/run/systemd/system",
        "multi-user.target",
        UNIT,
        T_TEMPLATE_PATH,
    ));
    assert_eq!(changes_named(&snap, UNIT), 0);
    assert!(inventory_entry(&snap, UNIT).is_none());
    for (route, containerfile) in both_routes(&snap) {
        assert!(!containerfile.contains(UNIT), "{route}:\n{containerfile}");
    }
}

/// T22's first four cases for an instance of `backup@.service`, which is
/// wanted by `multi-user.target`. Found by a link under `/etc`: its only
/// link in `rescue.target.wants` is `Disabled` with one `NotInInstall`
/// link, no state change under `disable *`, and a `Disabled` state change
/// under a list-form `enable` naming it; with both links, `Enabled` and
/// the extra link advised; with two `WantedBy=` values and one link,
/// `PartialInstall` and no state; with the second link shipped under
/// `/usr/lib`, `Enabled` and no advisory for it. The fourth case, only
/// vendor links, needs a unit the scan lists: the package's concrete
/// `backup@x.service`, `Disabled` with no state change.
#[test]
fn t22_the_install_state_cases_repeat_for_an_instance() {
    const UNIT: &str = "backup@x.service";
    const TEMPLATE: &str = "backup@.service";
    const TEMPLATE_PATH: &str = "/usr/lib/systemd/system/backup@.service";
    const ETC: &str = "/etc/systemd/system";
    let host = |presets: &[&str], template: &str| {
        let mut host = UnitHost::new()
            .unit(TEMPLATE, "indirect")
            .vendor_unit(TEMPLATE, template, None);
        for rule in presets {
            host = host.preset(rule);
        }
        host
    };
    let rescue_only = |presets: &[&str]| {
        instance_link(
            host(presets, WANTS_MULTI_USER),
            ETC,
            "rescue.target",
            UNIT,
            TEMPLATE_PATH,
        )
    };
    let found = |snap: &InspectionSnapshot| {
        links_of(snap)
            .iter()
            .map(|l| {
                (
                    l.relation,
                    l.parent.as_str().to_owned(),
                    l.unit.as_str().to_owned(),
                    l.cause,
                )
            })
            .collect::<Vec<_>>()
    };
    let rescue_link = (
        LinkRelation::Wants,
        "rescue.target".to_owned(),
        UNIT.to_owned(),
        ManualLinkCause::NotInInstall,
    );

    // Only the rescue link, under `disable *`.
    let snap = scan(rescue_only(&["disable *"]));
    let entry = inventory_entry(&snap, UNIT).expect("the inventory entry");
    assert_eq!(
        (entry.state, entry.preset),
        (UnitState::Disabled, Some(PresetDefault::Disable))
    );
    assert_eq!(found(&snap), std::slice::from_ref(&rescue_link));
    assert_eq!(changes_named(&snap, UNIT), 0);
    for (route, containerfile) in both_routes(&snap) {
        assert!(
            in_no_systemctl_instruction(&containerfile, UNIT),
            "{route}:\n{containerfile}"
        );
    }

    // The same under a list-form `enable` naming it.
    let snap = scan(rescue_only(&["enable backup@.service x", "disable *"]));
    assert_eq!(change(&snap, UNIT).current_state(), UnitState::Disabled);
    assert_eq!(found(&snap), std::slice::from_ref(&rescue_link));
    assert_instance_carried(&snap, UNIT, "enable preset");

    // Both links, under `disable *`.
    let snap = scan(instance_link(
        rescue_only(&["disable *"]),
        ETC,
        "multi-user.target",
        UNIT,
        TEMPLATE_PATH,
    ));
    assert_eq!(change(&snap, UNIT).current_state(), UnitState::Enabled);
    assert_eq!(found(&snap), [rescue_link]);
    assert_instance_carried(&snap, UNIT, "both links");
    let containerfile = containerfile_in(&render_export(&snap));
    assert!(
        containerfile
            .lines()
            .any(|l| l.starts_with(&format!("# Advisory: rescue.target.wants/{UNIT} "))),
        "{containerfile}"
    );

    // Two `WantedBy=` values and one link: `PartialInstall`.
    let two = "[Service]\nExecStart=/bin/true\n\n[Install]\n\
               WantedBy=multi-user.target graphical.target\n";
    let partial = instance_link(
        host(&["disable *"], two),
        ETC,
        "multi-user.target",
        UNIT,
        TEMPLATE_PATH,
    );
    let snap = scan(partial);
    assert_eq!(
        found(&snap),
        [(
            LinkRelation::Wants,
            "multi-user.target".to_owned(),
            UNIT.to_owned(),
            ManualLinkCause::PartialInstall
        )]
    );
    assert_eq!(changes_named(&snap, UNIT), 0);
    assert!(inventory_entry(&snap, UNIT).is_none());

    // The second link shipped under `/usr/lib`: `Enabled`, no advisory.
    let partial = instance_link(
        host(&["disable *"], two),
        ETC,
        "multi-user.target",
        UNIT,
        TEMPLATE_PATH,
    );
    let snap = scan(instance_link(
        partial,
        "/usr/lib/systemd/system",
        "graphical.target",
        UNIT,
        TEMPLATE_PATH,
    ));
    assert_eq!(change(&snap, UNIT).current_state(), UnitState::Enabled);
    assert!(found(&snap).is_empty(), "{:?}", found(&snap));
    assert_instance_carried(&snap, UNIT, "vendor second link");

    // Only vendor links, on the package's concrete instance file.
    let host = UnitHost::new()
        .unit(UNIT, "disabled")
        .preset("disable *")
        .vendor_unit(UNIT, WANTS_MULTI_USER, None)
        .link(
            &format!("/usr/lib/systemd/system/multi-user.target.wants/{UNIT}"),
            &format!("/usr/lib/systemd/system/{UNIT}"),
        );
    let snap = scan(host);
    let entry = inventory_entry(&snap, UNIT).expect("the inventory entry");
    assert_eq!(entry.state, UnitState::Disabled);
    assert_eq!(changes_named(&snap, UNIT), 0);
}

/// One T39 case through the scan: `unit` is collected with an inventory
/// entry in `state` and the preset the lookup gave, then has the state
/// change `change` or none. A state change is carried with its verb.
fn assert_t39(
    snap: &InspectionSnapshot,
    unit: &str,
    state: UnitState,
    preset: Option<PresetDefault>,
    expected_change: Option<UnitState>,
    label: &str,
) {
    let entry =
        inventory_entry(snap, unit).unwrap_or_else(|| panic!("{label}: no inventory entry"));
    assert_eq!((entry.state, entry.preset), (state, preset), "{label}");
    match expected_change {
        Some(expected) => {
            assert_eq!(change(snap, unit).current_state(), expected, "{label}");
            assert_instance_carried(snap, unit, label);
        }
        None => assert_eq!(changes_named(snap, unit), 0, "{label}"),
    }
}

/// T39 for an instance with its own unit file, `t@two.service`, through
/// the scan.
#[test]
fn t39_an_instance_with_its_own_unit_file() {
    use PresetDefault::{Disable, Enable};
    use UnitState::{Disabled, Enabled};
    // (presets, listed state, inventory state, preset, state change, label)
    type Case<'a> = (
        &'a [&'a str],
        &'a str,
        UnitState,
        Option<PresetDefault>,
        Option<UnitState>,
        &'a str,
    );
    let cases: [Case; 5] = [
        (
            &["enable t@two.service", "disable *"],
            "enabled",
            Enabled,
            Some(Enable),
            None,
            "named",
        ),
        (
            &["enable t@two.service", "disable *"],
            "disabled",
            Disabled,
            Some(Enable),
            Some(Disabled),
            "named, the administrator disabled it",
        ),
        (
            &["enable t@*", "disable *"],
            "enabled",
            Enabled,
            Some(Enable),
            None,
            "a glob",
        ),
        (
            &["enable t@.service two three", "disable *"],
            "enabled",
            Enabled,
            Some(Enable),
            None,
            "the list form",
        ),
        (
            &["enable t@.service", "disable *"],
            "enabled",
            Enabled,
            Some(Disable),
            Some(Enabled),
            "the bare rule, not read for it",
        ),
    ];
    for (presets, listed, state, preset, expected, label) in cases {
        let snap = scan(own_file_host(presets, listed));
        assert_eq!(
            fragment_of_entry(&snap, "t@two.service", expected),
            expected.map(|_| "t@two.service"),
            "{label}"
        );
        assert_t39(&snap, "t@two.service", state, preset, expected, label);
    }
}

/// The file of `unit`'s state change, when one is expected.
fn fragment_of_entry<'a>(
    snap: &'a InspectionSnapshot,
    unit: &str,
    expected: Option<UnitState>,
) -> Option<&'a str> {
    expected.map(|_| match fragment_of(snap, unit) {
        UnitFragment::Vendor { file, .. }
        | UnitFragment::Local { file }
        | UnitFragment::Shadowed { file, .. } => file.as_str(),
        UnitFragment::Absent => panic!("{unit} has no file"),
    })
}

/// T39 for an instance backed only by its template, through the scan: the
/// template's first matching rule decides, and rules that name an
/// instance are not read for it. A matching `Unknown` line gives no preset
/// and the structured warning naming the line, its file and the unit; no
/// matching rule gives no preset. The derived cases (an instance with no
/// link, found from the presets) are
/// `t39_a_disabled_instance_is_found_from_the_presets`.
#[test]
fn t39_an_instance_backed_only_by_its_template() {
    use PresetDefault::{Disable, Enable};
    use UnitState::Enabled;
    // (presets, DefaultInstance=, instance, preset, state change, label)
    type Case<'a> = (
        &'a [&'a str],
        Option<&'a str>,
        &'a str,
        Option<PresetDefault>,
        Option<UnitState>,
        &'a str,
    );
    let cases: [Case; 8] = [
        (
            &["enable t@.service", "disable *"],
            Some("one"),
            "one",
            Some(Enable),
            None,
            "the default instance",
        ),
        (
            &["enable t@.service", "disable *"],
            Some("one"),
            "two",
            Some(Disable),
            Some(Enabled),
            "another instance",
        ),
        (
            &["enable t@.service", "disable *"],
            None,
            "one",
            Some(Disable),
            Some(Enabled),
            "no DefaultInstance=",
        ),
        (
            &["enable t@.service two three", "disable *"],
            Some("one"),
            "two",
            Some(Enable),
            None,
            "listed",
        ),
        (
            &["enable t@.service two three", "disable *"],
            Some("one"),
            "one",
            Some(Disable),
            Some(Enabled),
            "not listed, the default instance",
        ),
        (
            &["enable t@two.service", "disable *"],
            Some("one"),
            "two",
            Some(Disable),
            Some(Enabled),
            "a rule naming the instance is not read",
        ),
        (
            &["disable t@.service"],
            Some("one"),
            "one",
            Some(Disable),
            Some(Enabled),
            "disable",
        ),
        (
            &["enable other.service"],
            Some("one"),
            "one",
            None,
            None,
            "no rule matches",
        ),
    ];
    for (presets, default, instance, preset, expected, label) in cases {
        let unit = format!("t@{instance}.service");
        let snap = scan(template_host(presets, default, &[instance]));
        assert_eq!(
            fragment_of_entry(&snap, &unit, expected),
            expected.map(|_| T_TEMPLATE),
            "{label}"
        );
        assert_t39(&snap, &unit, Enabled, preset, expected, label);
    }

    // A matching `Unknown` line before `enable t@.service`.
    let snap = scan(template_host(
        &["frobnicate t@.service", "enable t@.service"],
        Some("one"),
        &["one"],
    ));
    assert_t39(&snap, "t@one.service", Enabled, None, None, "Unknown");
    let warned = snap.warnings.iter().any(|w| {
        w.extra.get("file") == Some(&serde_json::json!(PRESET_PATH))
            && w.extra.get("line") == Some(&serde_json::json!(1))
            && w.extra.get("units") == Some(&serde_json::json!(["t@one.service"]))
    });
    assert!(warned, "{:?}", snap.warnings);
}

/// The preset file every [`UnitHost::preset`] rule is written to.
const PRESET_PATH: &str = "/usr/lib/systemd/system-preset/90-default.preset";

/// T39's getty cases, enabled instances only, with RHEL 9.8's preset files
/// and with RHEL 10.2's: `tty1` enabled has no state change and `tty2`
/// enabled is an `Enabled` state change, carried. `tty1` with no link is
/// `t39_an_unlinked_getty_tty1_is_found_on_both_releases`.
#[test]
fn t39_getty_instances_on_both_releases() {
    const GETTY: &str = "[Service]\nExecStart=-/sbin/agetty %I\n\n[Install]\n\
                         WantedBy=getty.target\nDefaultInstance=tty1\n";
    const PRESET_DIR: &str = "/usr/lib/systemd/system-preset";
    let release = |rhel_98: bool| {
        let mut host = UnitHost::new()
            .unit("getty@.service", "indirect")
            .vendor_unit("getty@.service", GETTY, Some("systemd"))
            .file(
                &format!("{PRESET_DIR}/90-systemd.preset"),
                "enable getty@.service\n",
            )
            .file(
                &format!("{PRESET_DIR}/99-default-disable.preset"),
                "disable *\n",
            );
        if rhel_98 {
            host = host.preset("enable getty@tty1.service");
        }
        for tty in ["tty1", "tty2"] {
            host = instance_link(
                host,
                "/etc/systemd/system",
                "getty.target",
                &format!("getty@{tty}.service"),
                "/usr/lib/systemd/system/getty@.service",
            );
        }
        host
    };
    for (label, rhel_98) in [("RHEL 9.8", true), ("RHEL 10.2", false)] {
        let snap = scan(release(rhel_98));
        assert_t39(
            &snap,
            "getty@tty1.service",
            UnitState::Enabled,
            Some(PresetDefault::Enable),
            None,
            &format!("{label}: tty1"),
        );
        assert_t39(
            &snap,
            "getty@tty2.service",
            UnitState::Enabled,
            Some(PresetDefault::Disable),
            Some(UnitState::Enabled),
            &format!("{label}: tty2"),
        );
    }
}

/// One T39 instance derived from the presets (§ 7.5): collected with no
/// link, an inventory entry `Disabled` against the `enable` the template's
/// rule gives it, and a `Disabled` state change whose file is `template`,
/// rendering `RUN systemctl disable` on both routes.
fn assert_derived_disable(snap: &InspectionSnapshot, unit: &str, template: &str, label: &str) {
    let entry = inventory_entry(snap, unit).unwrap_or_else(|| panic!("{label}: no entry"));
    assert_eq!(
        (entry.state, entry.preset),
        (UnitState::Disabled, Some(PresetDefault::Enable)),
        "{label}"
    );
    assert_eq!(changes_named(snap, unit), 1, "{label}");
    assert_eq!(
        change(snap, unit).current_state(),
        UnitState::Disabled,
        "{label}"
    );
    assert_eq!(
        fragment_of_entry(snap, unit, Some(UnitState::Disabled)),
        Some(template),
        "{label}"
    );
    assert_instance_carried(snap, unit, label);
}

/// T39's derived instances (§ 7.5, "A disabled instance is found from the
/// presets"), through the real inspectors. Under a bare `enable t@.service`
/// with no rule after it and no link for `t@one`, `t@one` is found and is a
/// `Disabled` state change. A list-form rule finds each listed instance the
/// same way. Under `disable t@.service` no instance is derived. A candidate
/// with its own unit file is one row, not two.
#[test]
fn t39_a_disabled_instance_is_found_from_the_presets() {
    let snap = scan(template_host(&["enable t@.service"], Some("one"), &[]));
    assert_derived_disable(&snap, "t@one.service", T_TEMPLATE, "bare enable");

    let snap = scan(template_host(
        &["enable t@.service two three", "disable *"],
        Some("one"),
        &[],
    ));
    for unit in ["t@two.service", "t@three.service"] {
        assert_derived_disable(&snap, unit, T_TEMPLATE, "list form");
    }
    assert!(
        inventory_entry(&snap, "t@one.service").is_none(),
        "list form"
    );

    let snap = scan(template_host(&["disable t@.service"], Some("one"), &[]));
    assert!(inventory_entry(&snap, "t@one.service").is_none(), "disable");
    assert_eq!(changes_named(&snap, "t@one.service"), 0, "disable");

    // The bare rule's candidate with its own unit file, listed `disabled`:
    // read once, as a listed unit, by its own name, which the bare rule
    // does not match, so `disable *` is its preset and it has no row.
    let snap = scan(
        template_host(&["enable t@.service", "disable *"], Some("one"), &[])
            .unit("t@one.service", "disabled")
            .vendor_unit("t@one.service", &t_template(None), None),
    );
    let units = snap.units.as_ref().expect("a units section");
    let entries: Vec<_> = units
        .inventory
        .iter()
        .filter(|e| e.unit.as_str() == "t@one.service")
        .map(|e| (e.state, e.preset))
        .collect();
    assert_eq!(
        entries,
        [(UnitState::Disabled, Some(PresetDefault::Disable))],
        "own file"
    );
    assert_eq!(changes_named(&snap, "t@one.service"), 0, "own file");
}

/// T39's derivation safeguards (§ 7.5), each with a control that reverses
/// one condition, through the real inspectors. Presence is asserted before
/// absence.
///
/// - A masked template under `enable t@.service one` derives no instance:
///   the template's mask is a row and `t@one` has no entry. Unmasked,
///   `t@one` is a derived `disable` on both routes. (The list form: under
///   a bare rule the masked template has no `DefaultInstance=` to read.)
/// - An instance linked under a dependency directory whose parent is not a
///   unit name is not derived: the link is reported and `t@one` has no
///   entry. Without the link, it is derived.
/// - An enabled owner with no row and no `enable` preset (no rule matches
///   it) derives the instance its `Also=` names: an inventory entry,
///   `Disabled` under the `disable` its template's bare rule gives an
///   instance that is not its `DefaultInstance=`, so no row and no line.
///   Disabled, or with an undetermined section, it derives none.
/// - A transitive `Also=` chain (`bundle` names `b`, `b` names `t@two`)
///   derives the instance at its end. With the chain cut, none.
#[test]
fn t39_the_derivation_safeguards_have_controls() {
    const ONE: &str = "t@one.service";
    const TWO: &str = "t@two.service";

    // A masked template.
    let masked = scan(
        UnitHost::new()
            .unit(T_TEMPLATE, "masked")
            .preset("enable t@.service one")
            .vendor_unit(T_TEMPLATE, &t_template(None), None)
            .mask(T_TEMPLATE),
    );
    assert_eq!(
        change(&masked, T_TEMPLATE).current_state(),
        UnitState::Masked,
        "presence: the template's mask"
    );
    assert!(inventory_entry(&masked, ONE).is_none(), "masked template");
    assert_eq!(changes_named(&masked, ONE), 0, "masked template");
    let unmasked = scan(template_host(&["enable t@.service one"], None, &[]));
    assert_derived_disable(&unmasked, ONE, T_TEMPLATE, "unmasked");

    // A link under a parent that is not a unit name.
    const UNPARSED: &str = "/etc/systemd/system/not a unit.wants/t@one.service";
    let linked = scan(
        template_host(&["enable t@.service"], Some("one"), &[]).link(UNPARSED, T_TEMPLATE_PATH),
    );
    assert!(
        linked
            .warnings
            .iter()
            .any(|w| w.extra.get("path") == Some(&serde_json::json!(UNPARSED))),
        "presence: the link is reported: {:?}",
        linked.warnings
    );
    assert!(inventory_entry(&linked, ONE).is_none(), "unparsed parent");
    assert_eq!(changes_named(&linked, ONE), 0, "unparsed parent");
    for (route, containerfile) in both_routes(&linked) {
        assert!(
            in_no_systemctl_instruction(&containerfile, ONE),
            "{route}:\n{containerfile}"
        );
    }

    // An enabled owner with no row and no `enable` preset.
    let owner = |state: &str, install: &str| {
        UnitHost::new()
            .unit("bundle.service", state)
            .unit(T_TEMPLATE, "indirect")
            .preset("enable t@.service")
            .vendor_unit(
                "bundle.service",
                &format!("[Service]\nExecStart=/bin/true\n\n[Install]\n{install}"),
                None,
            )
            .vendor_unit(T_TEMPLATE, &t_template(None), None)
    };
    let wanted = "WantedBy=multi-user.target\nAlso=t@one.service\n";
    let enabled = scan(owner("enabled", wanted));
    assert_eq!(
        inventory_entry(&enabled, "bundle.service").map(|e| (e.state, e.preset)),
        Some((UnitState::Enabled, None)),
        "presence: the owner is enabled with no preset"
    );
    assert_eq!(changes_named(&enabled, "bundle.service"), 0, "no owner row");
    let derived_entry = |snap: &InspectionSnapshot, unit: &str| {
        inventory_entry(snap, unit).map(|e| (e.state, e.preset))
    };
    assert_eq!(
        derived_entry(&enabled, ONE),
        Some((UnitState::Disabled, Some(PresetDefault::Disable))),
        "enabled owner"
    );
    assert_eq!(changes_named(&enabled, ONE), 0, "enabled owner");
    let disabled = scan(owner("disabled", wanted));
    assert_eq!(
        inventory_entry(&disabled, "bundle.service").map(|e| e.state),
        Some(UnitState::Disabled),
        "presence: the owner is disabled"
    );
    assert!(inventory_entry(&disabled, ONE).is_none(), "disabled owner");
    let undetermined = scan(owner(
        "enabled",
        "WantedBy=multi-user.target\nUpheldBy=x.service\nAlso=t@one.service\n",
    ));
    assert!(
        undetermined
            .units
            .as_ref()
            .expect("a units section")
            .undetermined_installs
            .iter()
            .any(|r| r.unit.as_str() == "bundle.service"),
        "presence: the owner's section is undetermined"
    );
    assert!(
        inventory_entry(&undetermined, ONE).is_none(),
        "undetermined owner"
    );
    assert_eq!(changes_named(&undetermined, ONE), 0, "undetermined owner");

    // A transitive `Also=` chain.
    let chain = |b_install: &str| {
        owner("enabled", "WantedBy=multi-user.target\nAlso=b.service\n").vendor_unit(
            "b.service",
            &format!("[Service]\nExecStart=/bin/true\n\n[Install]\n{b_install}"),
            None,
        )
    };
    let through = scan(chain("Also=t@two.service\n"));
    assert_eq!(
        derived_entry(&through, TWO),
        Some((UnitState::Disabled, Some(PresetDefault::Disable))),
        "transitive chain"
    );
    let cut = scan(chain(""));
    assert!(inventory_entry(&cut, TWO).is_none(), "the chain cut");
    assert_eq!(changes_named(&cut, TWO), 0, "the chain cut");
}

/// T39's getty case with no link (§ 7.5), on RHEL 9.8's preset files and on
/// RHEL 10.2's: `getty@tty1` is found from `enable getty@.service` and its
/// `DefaultInstance=tty1`, a `Disabled` state change.
#[test]
fn t39_an_unlinked_getty_tty1_is_found_on_both_releases() {
    const GETTY: &str = "[Service]\nExecStart=-/sbin/agetty %I\n\n[Install]\n\
                         WantedBy=getty.target\nDefaultInstance=tty1\n";
    const PRESET_DIR: &str = "/usr/lib/systemd/system-preset";
    for (label, rhel_98) in [("RHEL 9.8", true), ("RHEL 10.2", false)] {
        let mut host = UnitHost::new()
            .unit("getty@.service", "disabled")
            .vendor_unit("getty@.service", GETTY, Some("systemd"))
            .file(
                &format!("{PRESET_DIR}/90-systemd.preset"),
                "enable getty@.service\n",
            )
            .file(
                &format!("{PRESET_DIR}/99-default-disable.preset"),
                "disable *\n",
            );
        if rhel_98 {
            host = host.preset("enable getty@tty1.service");
        }
        let snap = scan(host);
        assert_derived_disable(&snap, "getty@tty1.service", "getty@.service", label);
        assert!(
            inventory_entry(&snap, "getty@tty2.service").is_none(),
            "{label}"
        );
    }
}

/// T40's `UpheldBy=` vendor unit with `Also=t@two.service` and one link,
/// the package shipping `t@.service`: the per-link advisory and the
/// unit's `UndeterminedInstall`, and, since an undetermined owner has no
/// readable `Also=`, no candidate instance derived from it.
#[test]
fn t40_an_undetermined_owner_with_also_derives_no_instance() {
    let upheld = "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\n\
                  UpheldBy=multi-user.target\nAlso=t@two.service\n";
    let snap = scan(
        t40_host(upheld, None)
            .unit(T_TEMPLATE, "disabled")
            .vendor_unit(T_TEMPLATE, &t_template(None), Some("needpkg"))
            .link(
                "/etc/systemd/system/multi-user.target.wants/need.service",
                "/usr/lib/systemd/system/need.service",
            ),
    );
    let units = snap.units.as_ref().expect("a units section");
    assert!(units.state_changes.is_empty(), "{:?}", units.state_changes);
    assert_eq!(
        units
            .undetermined_installs
            .iter()
            .map(|r| r.unit.as_str())
            .collect::<Vec<_>>(),
        ["need.service"]
    );
    assert_eq!(
        links_of(&snap)
            .iter()
            .map(|l| (l.parent.as_str(), l.unit.as_str(), l.cause))
            .collect::<Vec<_>>(),
        [(
            "multi-user.target",
            "need.service",
            ManualLinkCause::Undetermined {
                reason: InstallUnknown::UpheldBy
            }
        )]
    );
    let first = "# Advisory: multi-user.target.wants/need.service is not carried: the unit's \
                 [Install] section cannot be relied on (enabling the unit would also make an \
                 .upholds link).";
    for (route, containerfile) in both_routes(&snap) {
        assert_eq!(
            block_from(&containerfile, first),
            [
                first.to_owned(),
                "#   To carry it, add after the configuration COPY: RUN systemctl add-wants -- \
                 'multi-user.target' 'need.service'"
                    .to_owned()
            ],
            "{route}:\n{containerfile}"
        );
    }
    assert!(inventory_entry(&snap, "t@two.service").is_none());
    assert_eq!(changes_named(&snap, "t@two.service"), 0);
}

/// T23 through the scan: an `/etc` link to an instance of
/// `systemd-fsck@.service` is a `HostDevice` link whose advisory has no
/// command line, with no state change, no inventory entry and no
/// `# Omitted:` line. Control: the same link to an instance of a template
/// on no list, wanted by `local-fs.target`, is an `Enabled` state change.
#[test]
fn t23_a_host_device_instance_link_through_the_scan() {
    const FSCK: &str = "systemd-fsck@dev-vdb1.service";
    const FSCK_TEMPLATE: &str = "/usr/lib/systemd/system/systemd-fsck@.service";
    let fsck = "[Unit]\nDescription=File System Check on %f\n\n[Service]\nType=oneshot\n\
                ExecStart=/usr/lib/systemd/systemd-fsck %f\n";
    let host = UnitHost::new()
        .unit("systemd-fsck@.service", "static")
        .preset("disable *")
        .vendor_unit("systemd-fsck@.service", fsck, Some("systemd"));
    let snap = scan(instance_link(
        host,
        "/etc/systemd/system",
        "local-fs.target",
        FSCK,
        FSCK_TEMPLATE,
    ));
    assert_eq!(
        links_of(&snap)
            .iter()
            .map(|l| (l.relation, l.parent.as_str(), l.unit.as_str(), l.cause))
            .collect::<Vec<_>>(),
        [(
            LinkRelation::Wants,
            "local-fs.target",
            FSCK,
            ManualLinkCause::HostDevice
        )]
    );
    assert_eq!(changes_named(&snap, FSCK), 0);
    assert!(inventory_entry(&snap, FSCK).is_none());
    let advisory =
        format!("# Advisory: {FSCK} is tied to a device of the source host. Not carried.");
    for (route, containerfile) in both_routes(&snap) {
        assert_eq!(
            block_from(&containerfile, &advisory),
            std::slice::from_ref(&advisory),
            "{route}:\n{containerfile}"
        );
        assert!(
            omitted(&containerfile, FSCK).is_none(),
            "{route}:\n{containerfile}"
        );
        assert!(
            only_comments_name(&containerfile, FSCK),
            "{route}:\n{containerfile}"
        );
    }

    let local_fs = "[Service]\nExecStart=/usr/bin/t %i\n\n[Install]\nWantedBy=local-fs.target\n";
    let control = scan(instance_link(
        UnitHost::new()
            .unit(T_TEMPLATE, "indirect")
            .preset("disable *")
            .vendor_unit(T_TEMPLATE, local_fs, None),
        "/etc/systemd/system",
        "local-fs.target",
        "t@dev-vdb1.service",
        T_TEMPLATE_PATH,
    ));
    assert!(
        links_of(&control)
            .iter()
            .all(|l| l.cause != ManualLinkCause::HostDevice)
    );
    assert_eq!(changes_named(&control, "t@dev-vdb1.service"), 1);

    // The fsck template's text under a name on no list: a link to a unit
    // with no `[Install]`, not a device link.
    const CHECK: &str = "check@dev-vdb1.service";
    let host = UnitHost::new()
        .unit("check@.service", "static")
        .preset("disable *")
        .vendor_unit("check@.service", fsck, None);
    let renamed = scan(instance_link(
        host,
        "/etc/systemd/system",
        "local-fs.target",
        CHECK,
        "/usr/lib/systemd/system/check@.service",
    ));
    assert_eq!(
        links_of(&renamed)
            .iter()
            .map(|l| (l.relation, l.parent.as_str(), l.unit.as_str(), l.cause))
            .collect::<Vec<_>>(),
        [(
            LinkRelation::Wants,
            "local-fs.target",
            CHECK,
            ManualLinkCause::StaticUnit
        )]
    );
}

/// T24 through the scan: a concrete `worker@site.service` under `/etc`
/// beside a vendor template is a `Local` fragment whose file is the
/// instance, carried with the template's package excluded and its own file
/// staged, and omitted with `UnitFileNotStaged` when its file is excluded.
/// An instance with no file of its own has the template as its file and is
/// carried. Task 4.3's scan-level instance cases: an `[Install]` drop-in in
/// the template's `.d` is recorded and the enable is carried with it, and
/// omitted as `InstallEvidenceChanged` when it is excluded; a unit file
/// staged at the instance's own name, from another row, omits the enable
/// recorded from the template the same way.
#[test]
fn t24_the_effective_file_of_an_instance_through_the_scan() {
    const UNIT: &str = "worker@site.service";
    const TEMPLATE_PATH: &str = "/usr/lib/systemd/system/worker@.service";
    let base = || {
        UnitHost::new()
            .base_package("glibc")
            .package("workerpkg")
            .unit("worker@.service", "indirect")
            .preset("disable *")
            .vendor_unit("worker@.service", WANTS_MULTI_USER, Some("workerpkg"))
    };
    let snap = scan(
        base()
            .unit(UNIT, "enabled")
            .etc_unit(UNIT, WANTS_MULTI_USER),
    );
    assert_eq!(
        fragment_of(&snap, UNIT),
        &UnitFragment::Local { file: name(UNIT) }
    );
    assert_instance_carried(&snap, UNIT, "a concrete instance under /etc");
    let package_out = containerfile_in(&exported_after(
        &snap,
        &[(package_item("workerpkg"), false)],
    ));
    assert!(
        acts(&package_out, "enable", UNIT),
        "the template's package excluded:\n{package_out}"
    );
    let file_out = containerfile_in(&exported_after(
        &snap,
        &[(config_item(&etc_path(UNIT)), false)],
    ));
    assert_eq!(
        omitted(&file_out, UNIT),
        Some(format!("# Omitted: {UNIT} ({NOT_STAGED}").as_str()),
        "{file_out}"
    );
    assert!(in_no_systemctl_instruction(&file_out, UNIT), "{file_out}");

    let linked = || {
        instance_link(
            base(),
            "/etc/systemd/system",
            "multi-user.target",
            UNIT,
            TEMPLATE_PATH,
        )
    };
    let snap = scan(linked());
    assert_eq!(
        fragment_of(&snap, UNIT),
        &UnitFragment::Vendor {
            file: name("worker@.service"),
            owning_package: Some("workerpkg".into()),
        }
    );
    assert_instance_carried(&snap, UNIT, "the template");

    // A unit file at the instance's own name, staged from another row.
    let own_name = with_config_entry(&snap, &etc_path(UNIT), WANTS_MULTI_USER);
    let containerfile = containerfile_in(&exported_after(&own_name, &[]));
    assert_eq!(
        omitted(&containerfile, UNIT),
        Some(format!("# Omitted: {UNIT} ({}", evidence_changed(&etc_path(UNIT))).as_str()),
        "{containerfile}"
    );

    // An `[Install]` drop-in in the template's `.d`.
    let drop_in = etc_drop_in("worker@.service", "10-extra.conf");
    let snap = scan(
        instance_link(
            linked(),
            "/etc/systemd/system",
            "extra.target",
            UNIT,
            TEMPLATE_PATH,
        )
        .file(&drop_in, "[Install]\nWantedBy=extra.target\n"),
    );
    assert_eq!(change(&snap, UNIT).install_drop_ins, [drop_in.as_str()]);
    assert_instance_carried(&snap, UNIT, "the template's .d drop-in");
    let excluded = containerfile_in(&exported_after(&snap, &[(drop_in_item(&drop_in), false)]));
    assert_eq!(
        omitted(&excluded, UNIT),
        Some(format!("# Omitted: {UNIT} ({}", evidence_changed(&drop_in)).as_str()),
        "{excluded}"
    );
}

/// T24's search-path cases for an instance, through the scan: an instance
/// found by its `/etc` link with its own file under `/run` beside the
/// vendor template, or under `/usr/local/lib` beside an `/etc` template,
/// or with an `[Install]` drop-in in the template's `.d` under `/run`, is
/// refused with the structured warning naming it and the path, and has no
/// state change. Control: with none of those, or with a drop-in holding no
/// `[Install]` there, it is a state change.
#[test]
fn t24_the_search_path_refuses_an_instance_through_the_scan() {
    const UNIT: &str = "worker@site.service";
    const VENDOR_TEMPLATE: &str = "/usr/lib/systemd/system/worker@.service";
    const ETC_TEMPLATE: &str = "/etc/systemd/system/worker@.service";
    let linked = |host: UnitHost, template: &str| {
        instance_link(
            host.unit("worker@.service", "indirect").preset("disable *"),
            "/etc/systemd/system",
            "multi-user.target",
            UNIT,
            template,
        )
    };
    let vendor = || {
        linked(
            UnitHost::new().vendor_unit("worker@.service", WANTS_MULTI_USER, None),
            VENDOR_TEMPLATE,
        )
    };
    let local = || {
        linked(
            UnitHost::new().etc_unit("worker@.service", WANTS_MULTI_USER),
            ETC_TEMPLATE,
        )
    };

    let refused = [
        (
            "an instance file under /run",
            vendor().file("/run/systemd/system/worker@site.service", WANTS_MULTI_USER),
            "/run/systemd/system/worker@site.service",
        ),
        (
            "an instance file under /usr/local/lib",
            local().file(
                "/usr/local/lib/systemd/system/worker@site.service",
                WANTS_MULTI_USER,
            ),
            "/usr/local/lib/systemd/system/worker@site.service",
        ),
        (
            "an [Install] drop-in in the template's .d under /run",
            vendor().file(
                "/run/systemd/system/worker@.service.d/50-x.conf",
                "[Install]\nWantedBy=multi-user.target\n",
            ),
            "/run/systemd/system/worker@.service.d/50-x.conf",
        ),
    ];
    for (label, host, path) in refused {
        let snap = scan(host);
        assert_eq!(changes_named(&snap, UNIT), 0, "{label}");
        let warned = snap.warnings.iter().any(|w| {
            w.message.starts_with(&format!("unit {UNIT} "))
                && w.message.contains("is on the systemd search path")
                && w.extra.get("path") == Some(&serde_json::json!(path))
        });
        assert!(warned, "{label}: {:?}", snap.warnings);
    }

    let carried = [
        ("the vendor template", vendor()),
        ("the /etc template", local()),
        (
            "a drop-in with no [Install] in the template's .d under /run",
            vendor().file(
                "/run/systemd/system/worker@.service.d/50-x.conf",
                "[Service]\nNice=5\n",
            ),
        ),
    ];
    for (label, host) in carried {
        let snap = scan(host);
        assert_eq!(changes_named(&snap, UNIT), 1, "{label}");
        assert!(
            !snap
                .warnings
                .iter()
                .any(|w| w.message.contains("is on the systemd search path")),
            "{label}: {:?}",
            snap.warnings
        );
    }
}

/// A listed instance the name gate refuses stays refused: `t@site.service`
/// listed `bad` beside its `/etc` link gets today's warning, no state
/// change and no inventory entry. Nor do the presets derive it again: the
/// same name as the bare `enable` rule's `DefaultInstance=`, with no link.
/// The control is `t27_an_enabled_instance_found_by_its_link_is_carried`,
/// the same link with no listing row.
#[test]
fn a_listed_instance_the_gate_refuses_is_not_found_by_its_link() {
    const UNIT: &str = "t@site.service";
    let refused = |snap: &InspectionSnapshot, label: &str| {
        assert!(
            snap.warnings.iter().any(|w| w.message
                == format!("unit {UNIT} has state 'bad' - unit file is invalid or unreadable")),
            "{label}: {:?}",
            snap.warnings
        );
        assert_eq!(changes_named(snap, UNIT), 0, "{label}");
        assert!(inventory_entry(snap, UNIT).is_none(), "{label}");
        for (route, containerfile) in both_routes(snap) {
            assert!(
                !containerfile.contains(UNIT),
                "{label}, {route}:\n{containerfile}"
            );
        }
    };
    refused(
        &scan(template_host(&["disable *"], None, &["site"]).unit(UNIT, "bad")),
        "linked",
    );
    refused(
        &scan(template_host(&["enable t@.service"], Some("site"), &[]).unit(UNIT, "bad")),
        "derived",
    );
}

/// § 7.5 paragraph 2 through the scan: a listed `getty@tty3.service`
/// `masked`, with its `/dev/null` link and a leftover `getty.target.wants`
/// link, is one `Masked` state change carried as `RUN systemctl mask` on
/// both routes, named once. The link does not find it a second time. The
/// same for an instance with its own unit file. Like any mask, it is
/// omitted under rule 4 when a unit file is staged at its name, and under
/// rule 5's package half when the template's package is excluded; a
/// protected instance's mask is rule 3's FIXME.
#[test]
fn a_masked_instance_is_carried_as_a_mask() {
    const UNIT: &str = "getty@tty3.service";
    const GETTY: &str = "[Service]\nExecStart=-/sbin/agetty %I\n\n[Install]\n\
                         WantedBy=getty.target\nDefaultInstance=tty1\n";
    let host = UnitHost::new()
        .unit("getty@.service", "indirect")
        .unit(UNIT, "masked")
        .preset("enable getty@.service")
        .vendor_unit("getty@.service", GETTY, Some("systemd"))
        .mask(UNIT);
    let snap = scan(instance_link(
        host,
        "/etc/systemd/system",
        "getty.target",
        UNIT,
        "/usr/lib/systemd/system/getty@.service",
    ));
    assert_eq!(changes_named(&snap, UNIT), 1);
    assert_eq!(change(&snap, UNIT).current_state(), UnitState::Masked);
    assert_instance_carried(&snap, UNIT, "a masked instance");
    for (route, containerfile) in both_routes(&snap) {
        assert_eq!(
            instructions_naming(&containerfile, UNIT),
            1,
            "{route}:\n{containerfile}"
        );
    }

    // Its own unit file, under the mask.
    const OWN: &str = "t@own.service";
    let own = scan(
        UnitHost::new()
            .unit(OWN, "masked")
            .preset("disable *")
            .vendor_unit(OWN, &t_template(None), None)
            .mask(OWN),
    );
    assert_eq!(change(&own, OWN).current_state(), UnitState::Masked);
    assert_instance_carried(&own, OWN, "its own unit file");

    // Rule 4: a unit file staged at the instance's name.
    let occupied = with_config_entry(&own, &etc_path(OWN), "[Service]\nExecStart=/bin/true\n");
    let path = etc_path(OWN);
    assert_instance_omitted(
        &occupied,
        OWN,
        &format!(
            "# Omitted: {OWN} (the export also carries a unit file at {path}; exclude one of the \
             two)"
        ),
        "MaskPathOccupied",
    );

    // Rule 5's package half: the template's package excluded.
    let packaged = scan(
        UnitHost::new()
            .base_package("glibc")
            .package("ttypkg")
            .unit(T_TEMPLATE, "indirect")
            .unit("t@tty9.service", "masked")
            .preset("disable *")
            .vendor_unit(T_TEMPLATE, &t_template(None), Some("ttypkg"))
            .mask("t@tty9.service"),
    );
    assert_instance_carried(&packaged, "t@tty9.service", "the package included");
    let containerfile = containerfile_in(&exported_after(
        &packaged,
        &[(package_item("ttypkg"), false)],
    ));
    assert_eq!(
        omitted(&containerfile, "t@tty9.service"),
        Some("# Omitted: t@tty9.service (package 'ttypkg' was excluded)"),
        "{containerfile}"
    );
    assert!(
        in_no_systemctl_instruction(&containerfile, "t@tty9.service"),
        "{containerfile}"
    );

    // Rule 3: a protected instance's mask is a FIXME.
    const PROTECTED: &str = "ostree-fin@a.service";
    let protected = scan(
        UnitHost::new()
            .unit("ostree-fin@.service", "indirect")
            .unit(PROTECTED, "masked")
            .preset("disable *")
            .vendor_unit("ostree-fin@.service", &t_template(None), None)
            .mask(PROTECTED),
    );
    for (route, containerfile) in both_routes(&protected) {
        assert!(
            fixme_for(&containerfile, PROTECTED).is_some(),
            "{route}:\n{containerfile}"
        );
        assert!(
            in_no_systemctl_instruction(&containerfile, PROTECTED),
            "{route}:\n{containerfile}"
        );
    }
}

/// The OpenVPN client template, as the package ships it: its unit file
/// names no path for the instance, and § 7.5's table gives
/// `/etc/openvpn/client/%i.conf`.
const OPENVPN_CLIENT: &str = "[Unit]\nDescription=OpenVPN tunnel for %I\n\n[Service]\n\
     Type=notify\nWorkingDirectory=/etc/openvpn/client\n\
     ExecStart=/usr/sbin/openvpn --suppress-timestamps --nobind --config %i.conf\n\n\
     [Install]\nWantedBy=multi-user.target\n";
const OPENVPN_SITE: &str = "openvpn-client@site.service";
const OPENVPN_CONF: &str = "/etc/openvpn/client/site.conf";
const OPENVPN_CONF_TEXT: &str = "client\nremote vpn.example.net 1194\n";

/// The WireGuard template: `wg-quick up %i` names no path, and § 7.5's
/// table gives `/etc/wireguard/%i.conf`.
const WG_QUICK: &str = "[Unit]\nDescription=WireGuard via wg-quick(8) for %I\n\n[Service]\n\
     Type=oneshot\nRemainAfterExit=yes\nExecStart=/usr/bin/wg-quick up %i\n\
     ExecStop=/usr/bin/wg-quick down %i\n\n[Install]\nWantedBy=multi-user.target\n";
const WG0: &str = "wg-quick@wg0.service";
const WG0_CONF: &str = "/etc/wireguard/wg0.conf";
/// A WireGuard private key, in the form the redaction pattern matches.
const WG_KEY: &str = "yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=";

/// A host whose `package` ships `template` (listed `indirect`), with
/// `unit` linked from `multi-user.target.wants` under `/etc` and enabled
/// against `disable *`. Scanned against a base image.
fn instance_host(package: &str, template: &str, text: &str, unit: &str) -> UnitHost {
    instance_link(
        UnitHost::new()
            .base_package("glibc")
            .package(package)
            .unit(template, "indirect")
            .preset("disable *")
            .vendor_unit(template, text, Some(package)),
        "/etc/systemd/system",
        "multi-user.target",
        unit,
        &format!("/usr/lib/systemd/system/{template}"),
    )
}

/// [`instance_host`] for `openvpn-client@site.service`.
fn openvpn_host() -> UnitHost {
    instance_host(
        "openvpn",
        "openvpn-client@.service",
        OPENVPN_CLIENT,
        OPENVPN_SITE,
    )
}

/// The `InstanceConfigNotStaged` line for `unit` and `path`, whose text
/// after the path is `rest`.
fn config_omitted(unit: &str, path: &str, rest: &str) -> String {
    format!("# Omitted: {unit} (its configuration {path} {rest})")
}

/// T28 through the real inspectors: the `openvpn-client@site` link, the
/// vendor template naming `multi-user.target`, and an included, clean
/// `/etc/openvpn/client/site.conf`: one state change named for the
/// instance with the template's owner, the configuration in
/// `required_config`, the instance in `RUN systemctl enable` on both
/// routes, and no attention note.
#[test]
fn t28_a_carried_instance() {
    let snap = scan(openvpn_host().file(OPENVPN_CONF, OPENVPN_CONF_TEXT));
    assert_eq!(changes_named(&snap, OPENVPN_SITE), 1);
    let row = change(&snap, OPENVPN_SITE);
    assert_eq!(row.current_state(), UnitState::Enabled);
    assert_eq!(
        fragment_of(&snap, OPENVPN_SITE),
        &UnitFragment::Vendor {
            file: name("openvpn-client@.service"),
            owning_package: Some("openvpn".into()),
        }
    );
    assert_eq!(row.required_config, [OPENVPN_CONF]);
    assert_eq!(row.attention_reason, None);
    assert_instance_carried(&snap, OPENVPN_SITE, "T28");
    assert_eq!(unit_row(&snap, OPENVPN_SITE).attention_reason, None);
}

/// The 389 Directory Server template, cut to what § 7.5 reads: its unit
/// file names the instance's `/etc/dirsrv/slapd-%i` directory, and § 7.5's
/// table gives `/var/lib/dirsrv/slapd-%i` as its data.
const DIRSRV: &str = "[Unit]\nDescription=389 Directory Server %i.\n\n[Service]\nType=notify\n\
     ExecStart=/usr/sbin/ns-slapd -D /etc/dirsrv/slapd-%i -i /run/dirsrv/slapd-%i.pid\n\n\
     [Install]\nWantedBy=multi-user.target\n";
const DIRSRV_EXAMPLE: &str = "dirsrv@example.service";
const DIRSRV_DATA: &str = "/var/lib/dirsrv/slapd-example";

/// § 10.8's data sentence for `dirsrv@example.service`.
const DIRSRV_DATA_SENTENCE: &str = "dirsrv@example.service keeps data under \
     /var/lib/dirsrv/slapd-example. The image does not carry it, and the carried enable may \
     start the unit on first boot. Put the data in place before that boot, or the unit starts \
     without it.";

/// T28's data case (§ 10.8 "Data under `/var`"), through the real
/// inspectors: `dirsrv@example.service` with its configuration in the
/// export and an existing data directory has `data_paths`, its enable
/// carried, one `# Advisory:` line before the `RUN` lines on both routes,
/// and the same sentence as its refine row's `attention_reason`; the row
/// shows `required_config` and `data_paths`. With the configuration
/// missing, the enable is omitted and there is no data advisory. The
/// WireGuard row shows its `required_config` too.
#[test]
fn t28_a_carried_instance_with_data_has_the_data_advisory() {
    let host = |with_config: bool| {
        let host = instance_host("389-ds-base", "dirsrv@.service", DIRSRV, DIRSRV_EXAMPLE)
            .dir(DIRSRV_DATA);
        if with_config {
            host.file("/etc/dirsrv/slapd-example/dse.ldif", "dn: cn=config\n")
        } else {
            host
        }
    };
    let advisory = format!("# Advisory: {DIRSRV_DATA_SENTENCE}");

    let snap = scan(host(true));
    let row = change(&snap, DIRSRV_EXAMPLE);
    assert_eq!(row.required_config, ["/etc/dirsrv/slapd-example"]);
    assert_eq!(row.data_paths, [DIRSRV_DATA]);
    assert_instance_carried(&snap, DIRSRV_EXAMPLE, "data");
    for (route, containerfile) in both_routes(&snap) {
        let lines: Vec<&str> = containerfile.lines().collect();
        let at = lines
            .iter()
            .position(|l| *l == advisory)
            .unwrap_or_else(|| panic!("{route}: no data advisory:\n{containerfile}"));
        let run = lines
            .iter()
            .position(|l| l.starts_with("RUN systemctl "))
            .unwrap_or_else(|| panic!("{route}: no RUN line:\n{containerfile}"));
        assert!(at < run, "{route}: the advisory precedes the RUN lines");
        assert_eq!(
            count_lines(&containerfile, &advisory),
            1,
            "{route}:\n{containerfile}"
        );
    }
    let view = unit_row(&snap, DIRSRV_EXAMPLE);
    assert_eq!(view.attention_reason.as_deref(), Some(DIRSRV_DATA_SENTENCE));
    assert_eq!(view.required_config, ["/etc/dirsrv/slapd-example"]);
    assert_eq!(view.data_paths, [DIRSRV_DATA]);

    let omitted = scan(host(false));
    assert_eq!(change(&omitted, DIRSRV_EXAMPLE).data_paths, [DIRSRV_DATA]);
    assert_instance_omitted(
        &omitted,
        DIRSRV_EXAMPLE,
        "# Omitted: dirsrv@example.service (its configuration /etc/dirsrv/slapd-example is not \
         in the export)",
        "InstanceConfigNotStaged",
    );
    for (route, containerfile) in both_routes(&omitted) {
        assert!(
            !containerfile.contains("keeps data under"),
            "{route}:\n{containerfile}"
        );
    }
    assert_eq!(unit_row(&omitted, DIRSRV_EXAMPLE).attention_reason, None);

    let wg = scan(
        instance_host("wireguard-tools", "wg-quick@.service", WG_QUICK, WG0)
            .file(WG0_CONF, "[Interface]\nAddress = 10.0.0.1/24\n"),
    );
    let wg_row = unit_row(&wg, WG0);
    assert_eq!(wg_row.required_config, [WG0_CONF]);
    assert!(wg_row.data_paths.is_empty());
}

/// T29 through the real inspectors: each case where the instance's
/// configuration is not in the export as the host had it gives its
/// `# Omitted:` line and no enable, on both routes and on the refine row.
/// Excluded through `RefineSession`, the enable returns when the row is
/// included again. Content not captured (a file over the size cap). The
/// WireGuard key, through the real redaction pass: the staged file holds no
/// key. The file missing on the source; a package-owned, unmodified file;
/// a package-owned file the administrator changed on a host where
/// `rpm -Va` fails. Positive controls: a clean unowned file (T28), and a
/// package-owned file the administrator changed with `rpm -Va` working.
#[test]
fn t29_an_instance_whose_configuration_is_not_in_the_export() {
    let clean = scan(openvpn_host().file(OPENVPN_CONF, OPENVPN_CONF_TEXT));

    // Excluded through refine, then included again.
    let excluded_op = (config_item(OPENVPN_CONF), false);
    let containerfile =
        containerfile_in(&exported_after(&clean, std::slice::from_ref(&excluded_op)));
    let excluded_line = config_omitted(OPENVPN_SITE, OPENVPN_CONF, "is excluded from the export");
    assert_eq!(
        omitted(&containerfile, OPENVPN_SITE),
        Some(excluded_line.as_str()),
        "{containerfile}"
    );
    assert!(
        in_no_systemctl_instruction(&containerfile, OPENVPN_SITE),
        "{containerfile}"
    );
    let declined = declined_in(
        &session_after(&clean, std::slice::from_ref(&excluded_op)),
        OPENVPN_SITE,
    )
    .expect("a declined row");
    assert_eq!(declined.reason, "InstanceConfigNotStaged");
    assert_eq!(format!("# Omitted: {}", declined.text), excluded_line);
    let restored = containerfile_in(&exported_after(
        &clean,
        &[excluded_op.clone(), (excluded_op.0, true)],
    ));
    assert!(acts(&restored, "enable", OPENVPN_SITE), "{restored}");

    // Content not captured: a file over the config inspector's size cap.
    let too_big = format!("client\n{}\n", "# padding\n".repeat(30_000));
    let snap = scan(openvpn_host().file(OPENVPN_CONF, &too_big));
    assert_instance_omitted(
        &snap,
        OPENVPN_SITE,
        &config_omitted(OPENVPN_SITE, OPENVPN_CONF, "was not captured"),
        "InstanceConfigNotStaged",
    );

    // The WireGuard key, through the real redaction pass.
    let wg_conf = format!("[Interface]\nPrivateKey = {WG_KEY}\nAddress = 10.0.0.1/24\n");
    let wg = scan(
        instance_host("wireguard-tools", "wg-quick@.service", WG_QUICK, WG0)
            .file(WG0_CONF, &wg_conf),
    );
    assert_eq!(change(&wg, WG0).required_config, [WG0_CONF]);
    assert!(
        wg.redactions.iter().any(|f| f.path == WG0_CONF),
        "the redaction pass recorded the key: {:?}",
        wg.redactions
    );
    assert_instance_omitted(
        &wg,
        WG0,
        &config_omitted(
            WG0,
            WG0_CONF,
            "holds a redacted value; provision the secret on the deployed host and enable the \
             unit there",
        ),
        "InstanceConfigNotStaged",
    );
    for (route, tree) in both_exports(&wg) {
        assert!(staged_at(&tree, WG0_CONF), "{route}: the file is staged");
        assert!(
            files_holding(tree.path(), WG_KEY).is_empty(),
            "{route}: no file holds the key"
        );
    }

    // Nothing in the snapshot at the path: the file missing on the source,
    // a package-owned file left as the package shipped it, and a changed
    // package-owned file on a host where `rpm -Va` fails.
    let not_in = config_omitted(OPENVPN_SITE, OPENVPN_CONF, "is not in the export");
    for (label, host) in [
        ("missing", openvpn_host()),
        (
            "package-owned, unmodified",
            openvpn_host().owned_file(OPENVPN_CONF, OPENVPN_CONF_TEXT, "openvpn"),
        ),
        (
            "package-owned, changed, rpm -Va failing",
            openvpn_host()
                .owned_file(OPENVPN_CONF, OPENVPN_CONF_TEXT, "openvpn")
                .rpm_va("", 2),
        ),
    ] {
        let snap = scan(host);
        assert!(
            snap.config
                .iter()
                .flat_map(|c| &c.files)
                .all(|f| f.path != OPENVPN_CONF),
            "{label}: no config entry"
        );
        assert_instance_omitted(&snap, OPENVPN_SITE, &not_in, "InstanceConfigNotStaged");
    }

    // The controls.
    assert_instance_carried(&clean, OPENVPN_SITE, "clean, unowned");
    let changed = scan(
        openvpn_host()
            .owned_file(OPENVPN_CONF, OPENVPN_CONF_TEXT, "openvpn")
            .rpm_va(&format!("S.5....T.  c {OPENVPN_CONF}\n"), 1),
    );
    assert!(
        changed
            .config
            .iter()
            .flat_map(|c| &c.files)
            .any(|f| f.path == OPENVPN_CONF && f.package.as_deref() == Some("openvpn")),
        "a package-owned config entry"
    );
    assert_instance_carried(
        &changed,
        OPENVPN_SITE,
        "package-owned, changed, rpm -Va working",
    );
}

/// T29's notes (§ 10.8): under `--no-redaction` the WireGuard instance is
/// carried with an attention note naming the path, on its refine row; a
/// `Flagged` finding on the configuration leaves the enable carried with
/// its own note. The notes are not Containerfile lines.
#[test]
fn t29_an_unredacted_or_flagged_configuration_is_carried_with_a_note() {
    let wg_conf = format!("[Interface]\nPrivateKey = {WG_KEY}\nAddress = 10.0.0.1/24\n");
    let host = || {
        instance_host("wireguard-tools", "wg-quick@.service", WG_QUICK, WG0)
            .file(WG0_CONF, &wg_conf)
    };

    // The state the scan command's `--no-redaction` flag sets
    // (`redaction_state` `Raw` on an unredacted scan), set here by hand.
    let mut raw = scan_unredacted(host());
    raw.redaction_state = Some(inspectah_core::types::redaction::RedactionState::Raw);
    assert_instance_carried(&raw, WG0, "--no-redaction");
    assert_eq!(
        unit_row(&raw, WG0).attention_reason.as_deref(),
        Some(
            "Its configuration /etc/wireguard/wg0.conf is staged as the source host had it: the \
             scan was taken with --no-redaction, so no secret in it was redacted."
        )
    );

    // A `Flagged` finding on a clean file.
    let mut flagged = scan(openvpn_host().file(OPENVPN_CONF, OPENVPN_CONF_TEXT));
    flagged
        .redactions
        .push(inspectah_core::types::redaction::RedactionFinding {
            path: OPENVPN_CONF.into(),
            source: "inspector_hint".into(),
            kind: inspectah_core::types::redaction::RedactionKind::Flagged,
            pattern: "inspector_hint".into(),
            remediation: "a value that looks like a credential".into(),
            line: None,
            replacement: None,
            detection_method: inspectah_core::types::redaction::DetectionMethod::Heuristic,
            confidence: None,
            finding_kind: None,
        });
    assert_instance_carried(&flagged, OPENVPN_SITE, "flagged");
    assert_eq!(
        unit_row(&flagged, OPENVPN_SITE).attention_reason.as_deref(),
        Some(
            "Its configuration /etc/openvpn/client/site.conf holds a value the redaction pass \
             flagged and did not change. Check it before the image is used."
        )
    );
    for (route, containerfile) in both_routes(&flagged) {
        assert!(
            !containerfile.contains("redaction pass flagged"),
            "{route}:\n{containerfile}"
        );
    }
}

// ---------------------------------------------------------------------
// § 10.2 rule 7, `Also=` members, on a single host (T25 (a) to (h) and
// (k), T31's additive row with a member, T39's owner cases).
// ---------------------------------------------------------------------

/// The owner of the rule 7 hosts.
const BUNDLE: &str = "bundle.service";

/// [`WANTED_UNIT`] with `also` (`Also=...` lines) added to its `[Install]`.
fn wanted_with(also: &str) -> String {
    format!("{WANTED_UNIT}{also}")
}

/// A host whose [`INSTALL_PACKAGE`] ships every `(name, text)` of
/// `vendor`, with each `(name, state)` of `listed` listed by systemctl
/// (listed `enabled`, a unit gets the links `enable` makes) under
/// `presets`, then `disable *`. Scanned against a base image so refine
/// keeps the package included.
fn rule7_host(presets: &[&str], listed: &[(&str, &str)], vendor: &[(&str, &str)]) -> UnitHost {
    let mut host = UnitHost::new()
        .base_package("glibc")
        .package(INSTALL_PACKAGE);
    for (name, state) in listed {
        host = host.unit(name, state);
    }
    for rule in presets {
        host = host.preset(rule);
    }
    host = host.preset("disable *");
    for (name, text) in vendor {
        host = host.vendor_unit(name, text, Some(INSTALL_PACKAGE));
    }
    host
}

/// § 10.2's first line of rule 7's advisory for `unit`, whose command is
/// `verb`, naming `named` with `clause`.
fn also_head(unit: &str, verb: &str, named: &str, clause: &str) -> String {
    format!(
        "# Advisory: {unit} is {verb}d on the source host. Not carried: systemctl {verb} on it \
         also {verb}s {named} (named by Also=), {clause}."
    )
}

/// The clause for members that are not enabled under an `enable`.
const NOT_ENABLED: &str = "which is not enabled there";
/// The clause for a member whose links neither verb reproduces.
const NEITHER: &str = "whose links there neither command reproduces";
/// The clause for a member whose own row the plan does not carry.
const ROW_NOT_CARRIED: &str = "whose own row is not carried with this command";

/// § 10.2's lines to add, between their header and footer: `unit`'s own
/// command, then each `(verb, member)`.
fn also_lines(verb: &str, unit: &str, members: &[(&str, &str)]) -> Vec<String> {
    let mut lines =
        vec!["#   To reproduce the source host, add after the configuration COPY:".to_owned()];
    lines.push(format!("#     RUN systemctl {verb} -- '{unit}'"));
    for (member_verb, member) in members {
        lines.push(format!("#     RUN systemctl {member_verb} -- '{member}'"));
    }
    lines.push(
        "#   Each command follows Also= in its turn. Check the result with systemctl is-enabled."
            .to_owned(),
    );
    lines
}

/// The `InstallFile` block for `unit`'s `verb`, naming `path` and the
/// unit `of` whose install file it is.
fn install_file_block(unit: &str, verb: &str, path: &str, of: &str) -> Vec<String> {
    vec![
        format!(
            "# Advisory: {unit} is {verb}d on the source host. Not carried: {path} sets \
             [Install] for {of}, so the units systemctl {verb} reaches through Also= in the \
             image may not be the ones recorded."
        ),
        "#   After reading that file, add after the configuration COPY:".to_owned(),
        format!("#     RUN systemctl {verb} -- '{unit}'"),
    ]
}

/// The refine row's `declined` for a rule 7 block whose first line is
/// `head`.
fn also_declined(head: &str) -> inspectah_refine::projection::DeclinedView {
    inspectah_refine::projection::DeclinedView {
        renders: inspectah_refine::projection::DeclinedRenders::Advisory,
        reason: "AlsoMember".into(),
        text: head
            .strip_prefix("# Advisory: ")
            .expect("an advisory line")
            .to_owned(),
    }
}

/// Asserts, on both routes, that `unit` gets exactly `block` and that no
/// `systemctl` instruction names `unit` or any of `quiet`, then that
/// `unit`'s refine row declines with the block's first line.
fn assert_also_advisory(snap: &InspectionSnapshot, unit: &str, block: &[String], quiet: &[&str]) {
    for (route, containerfile) in both_routes(snap) {
        assert_eq!(
            advisory_for(&containerfile, unit),
            block,
            "{route}:\n{containerfile}"
        );
        for name in std::iter::once(&unit).chain(quiet) {
            assert!(
                in_no_systemctl_instruction(&containerfile, name),
                "{route}: {name}:\n{containerfile}"
            );
        }
    }
    assert_eq!(
        declined_in(&session(snap), unit),
        Some(also_declined(&block[0]))
    );
}

/// Asserts, on both routes, that `unit`'s `verb` is carried with no rule 7
/// block, and that its refine row declines nothing.
fn assert_also_carried(snap: &InspectionSnapshot, unit: &str, verb: &str, label: &str) {
    for (route, containerfile) in both_routes(snap) {
        assert!(
            acts(&containerfile, verb, unit),
            "{label} {route}:\n{containerfile}"
        );
        assert!(
            advisory_for(&containerfile, unit).is_empty(),
            "{label} {route}:\n{containerfile}"
        );
    }
    assert_eq!(declined_in(&session(snap), unit), None, "{label}");
}

/// T39's owner cases, render half (§ 7.4, "An owner at its preset whose
/// member is off"). `m.service`, at its `enable` preset, with `m.socket`
/// unlinked: the plan declines the owner's `enable` with
/// `LinksDiffer(Unlinked)`, and the Containerfile holds § 10.2's advisory
/// with the owner's `enable` and the member's `disable`, and no `systemctl`
/// instruction names either. The stock control, `m.socket` linked: no owner
/// row, and only `m.socket`'s own `enable` against its `disable` preset,
/// carried. The `Inert` control: no row and no advisory.
#[test]
fn t39_an_owner_whose_member_is_off_is_an_advisory() {
    let socket = "[Socket]\nListenStream=/run/m.sock\n\n[Install]\nWantedBy=sockets.target\n";
    let host = |also: &str, socket_state: &str| {
        rule7_host(
            &["enable m.service"],
            &[("m.service", "enabled"), ("m.socket", socket_state)],
            &[
                ("m.service", &wanted_with(also)),
                ("m.socket", socket),
                ("m-helper.service", NO_INSTALL_UNIT),
            ],
        )
    };

    let snap = scan(host("Also=m.socket\n", "disabled"));
    let block = [
        "# Advisory: m.service is enabled on the source host. Not carried: systemctl enable on it \
         also enables m.socket (named by Also=), which is not enabled there.",
        "#   To reproduce the source host, add after the configuration COPY:",
        "#     RUN systemctl enable -- 'm.service'",
        "#     RUN systemctl disable -- 'm.socket'",
        "#   Each command follows Also= in its turn. Check the result with systemctl is-enabled.",
    ]
    .map(str::to_owned);
    assert_also_advisory(&snap, "m.service", &block, &["m.socket"]);

    let stock = scan(host("Also=m.socket\n", "enabled"));
    assert_eq!(changes_named(&stock, "m.service"), 0);
    assert_also_carried(&stock, "m.socket", "enable", "stock");
    for (route, containerfile) in both_routes(&stock) {
        assert!(
            in_no_systemctl_instruction(&containerfile, "m.service")
                && advisory_for(&containerfile, "m.service").is_empty(),
            "stock {route}:\n{containerfile}"
        );
    }

    let inert = scan(host("Also=m-helper.service\n", "disabled"));
    assert_eq!(changes_named(&inert, "m.service"), 0);
    for (route, containerfile) in both_routes(&inert) {
        assert!(
            in_no_systemctl_instruction(&containerfile, "m.service")
                && advisory_for(&containerfile, "m.service").is_empty(),
            "inert {route}:\n{containerfile}"
        );
    }
}

/// T39's owner note (§ 7.4), on one host and on a fleet aggregate of two
/// hosts, in the scanned snapshot, the scan and refine exports' snapshots
/// and refine's row: one sentence that names no host, so it fits a merged
/// row as well as a single host's (Mark Russell, 2026-11-15).
#[test]
fn t39_the_member_off_note_fits_a_host_and_a_fleet() {
    const NOTE: &str = "A unit it enables through Also= is not enabled alongside it.";
    let socket = "[Socket]\nListenStream=/run/m.sock\n\n[Install]\nWantedBy=sockets.target\n";
    let host = |hostname: &str| {
        reload(&scan(
            rule7_host(
                &["enable m.service"],
                &[("m.service", "enabled"), ("m.socket", "disabled")],
                &[
                    ("m.service", &wanted_with("Also=m.socket\n")),
                    ("m.socket", socket),
                ],
            )
            .file("/etc/hostname", &format!("{hostname}\n")),
        ))
    };
    let single = host("host-a");
    let (merged, _) =
        merge_snapshots(vec![host("host-a"), host("host-b")], None, None).expect("merge");
    let merged = reload(&merged);
    assert!(merged.aggregate_meta.is_some(), "an aggregate");
    for (label, snap) in [("single host", &single), ("fleet", &merged)] {
        assert_eq!(
            change(snap, "m.service").attention_reason.as_deref(),
            Some(NOTE),
            "{label}"
        );
        assert_eq!(
            unit_row(snap, "m.service").attention_reason.as_deref(),
            Some(NOTE),
            "{label}: the refine row"
        );
        for (route, export) in [
            ("scan", render_export(snap)),
            ("refine", export_session(&session(snap))),
        ] {
            let json = std::fs::read_to_string(export.path().join("inspection-snapshot.json"))
                .expect("the export holds the snapshot");
            let exported = InspectionSnapshot::load(&json).expect("the exported snapshot loads");
            assert_eq!(
                change(&exported, "m.service").attention_reason.as_deref(),
                Some(NOTE),
                "{label} {route}"
            );
        }
    }
}

/// T39's owner case for an instance member, render half: `p.service` with
/// `Also=t@two.service`, the package shipping only `t@.service`, and no
/// link for `t@two`: rule 7's advisory naming `t@two.service`, with
/// `p.service`'s `enable` and `t@two.service`'s `disable`, and no
/// `systemctl` instruction naming either. The control, `t@two` linked:
/// `t@two` is `Installed` and its own row (enabled against `disable *`)
/// carries its `enable`; `p.service` has no row, and no rule 7 advisory and
/// no `systemctl` instruction names it.
#[test]
fn t39_an_owner_whose_instance_member_is_off_is_an_advisory() {
    let host = |linked: bool| {
        let host = rule7_host(
            &["enable p.service"],
            &[("p.service", "enabled"), (T_TEMPLATE, "indirect")],
            &[
                ("p.service", &wanted_with("Also=t@two.service\n")),
                (T_TEMPLATE, &t_template(None)),
            ],
        );
        if linked {
            instance_link(
                host,
                "/etc/systemd/system",
                "multi-user.target",
                "t@two.service",
                T_TEMPLATE_PATH,
            )
        } else {
            host
        }
    };
    let snap = scan(host(false));
    // Collected from `p.service`'s `Also=` (§ 7.5): a `Disabled` inventory
    // entry and no row of its own, beside the owner row.
    let entry = inventory_entry(&snap, "t@two.service").expect("t@two's inventory entry");
    assert_eq!(
        (entry.state, entry.preset),
        (UnitState::Disabled, Some(PresetDefault::Disable))
    );
    assert_eq!(changes_named(&snap, "t@two.service"), 0);
    assert_eq!(
        change(&snap, "p.service").current_state(),
        UnitState::Enabled
    );
    let mut block = vec![also_head(
        "p.service",
        "enable",
        "t@two.service",
        NOT_ENABLED,
    )];
    block.extend(also_lines(
        "enable",
        "p.service",
        &[("disable", "t@two.service")],
    ));
    assert_also_advisory(&snap, "p.service", &block, &["t@two.service"]);

    let control = scan(host(true));
    assert_eq!(changes_named(&control, "p.service"), 0);
    assert!(
        inventory_entry(&control, "p.service").is_some(),
        "control: p.service is collected"
    );
    assert_instance_carried(&control, "t@two.service", "control");
    for (route, containerfile) in both_routes(&control) {
        assert!(
            in_no_systemctl_instruction(&containerfile, "p.service")
                && advisory_for(&containerfile, "p.service").is_empty()
                && !containerfile.contains("# Advisory: p.service "),
            "control {route}:\n{containerfile}"
        );
    }
}

/// T25 (a): a packaged unit with three members, all `Installed`, nothing
/// in `/etc` but links: the `enable` is carried, with equal unit lines on
/// the two routes. It stays carried with a `[Service]`-only drop-in on the
/// unit and on a member. One member `Unlinked`: the advisory with the
/// `enable` line and a `disable` line for that member.
#[test]
fn t25_a_every_member_installed_is_carried_and_one_unlinked_is_advised() {
    let host = |c_state: &str| {
        let presets: &[&str] = if c_state == "enabled" {
            &["enable a.service", "enable b.service", "enable c.service"]
        } else {
            &["enable a.service", "enable b.service"]
        };
        rule7_host(
            presets,
            &[
                (BUNDLE, "enabled"),
                ("a.service", "enabled"),
                ("b.service", "enabled"),
                ("c.service", c_state),
            ],
            &[
                (BUNDLE, &wanted_with("Also=a.service b.service c.service\n")),
                ("a.service", WANTED_UNIT),
                ("b.service", WANTED_UNIT),
                ("c.service", WANTED_UNIT),
            ],
        )
    };
    let snap = scan(host("enabled"));
    assert_also_carried(&snap, BUNDLE, "enable", "(a)");
    let [(_, scanned), (_, refined)] = both_routes(&snap);
    assert_eq!(unit_lines(&scanned), unit_lines(&refined));

    let with_overrides = scan(
        host("enabled")
            .file(&etc_drop_in(BUNDLE, "20-own.conf"), SERVICE_DROP_IN)
            .file(&etc_drop_in("a.service", "20-own.conf"), SERVICE_DROP_IN),
    );
    assert_also_carried(&with_overrides, BUNDLE, "enable", "(a) overrides");

    // `c.service` unlinked, at its `disable *` preset, so with no row.
    let snap = scan(host("disabled"));
    let mut block = vec![also_head(BUNDLE, "enable", "c.service", NOT_ENABLED)];
    block.extend(also_lines("enable", BUNDLE, &[("disable", "c.service")]));
    assert_also_advisory(&snap, BUNDLE, &block, &["c.service"]);
}

/// Rule 7's recipe never suggests disabling a unit the image needs, through
/// the scan, the render and refine. An enabled packaged unit names
/// `bootc-fetch-apply-updates.timer`, a protected unit, and `helper.socket`
/// in `Also=`, and neither is enabled on the source host: the advisory
/// names both, keeps the `disable` line for `helper.socket`, says the image
/// needs the timer instead of a `disable` line for it, and its header says
/// the lines do not reproduce the source host, with no footer. Control:
/// `helper.socket` alone gets the full recipe.
#[test]
fn rule_7_never_suggests_disabling_a_protected_member_through_both_exports() {
    const TIMER: &str = "bootc-fetch-apply-updates.timer";
    let host = |also: &str| {
        rule7_host(
            &[],
            &[
                (BUNDLE, "enabled"),
                (TIMER, "disabled"),
                ("helper.socket", "disabled"),
            ],
            &[
                (BUNDLE, &wanted_with(also)),
                (
                    TIMER,
                    "[Timer]\nOnCalendar=daily\n\n[Install]\nWantedBy=timers.target\n",
                ),
                (
                    "helper.socket",
                    "[Socket]\nListenStream=/run/helper.sock\n\n[Install]\nWantedBy=sockets.target\n",
                ),
            ],
        )
    };

    let control = scan(host("Also=helper.socket\n"));
    let mut block = vec![also_head(BUNDLE, "enable", "helper.socket", NOT_ENABLED)];
    block.extend(also_lines(
        "enable",
        BUNDLE,
        &[("disable", "helper.socket")],
    ));
    assert_also_advisory(&control, BUNDLE, &block, &["helper.socket"]);

    let snap = scan(host(&format!("Also={TIMER} helper.socket\n")));
    let block = [
        also_head(
            BUNDLE,
            "enable",
            &format!("{TIMER} and helper.socket"),
            "which are not enabled there",
        ),
        "#   These lines do not reproduce the source host. Check each unit, then add after the \
         configuration COPY:"
            .to_owned(),
        format!("#     RUN systemctl enable -- '{BUNDLE}'"),
        "#     RUN systemctl disable -- 'helper.socket'".to_owned(),
        format!("#   No disable line for {TIMER}: the image needs this unit."),
    ];
    assert_also_advisory(&snap, BUNDLE, &block, &[TIMER, "helper.socket"]);
}

/// T25 (b): a `disable` with every member `Unlinked` is carried. A member
/// with no `[Install]` and a hand-made link is `Other`: an advisory with no
/// line to add.
#[test]
fn t25_b_a_disable_with_members() {
    let host = |also: &str| {
        rule7_host(
            &["enable bundle.service"],
            &[
                (BUNDLE, "disabled"),
                ("x.service", "disabled"),
                ("y.service", "disabled"),
            ],
            &[
                (BUNDLE, &wanted_with(also)),
                ("x.service", WANTED_UNIT),
                ("y.service", WANTED_UNIT),
                ("z.service", NO_INSTALL_UNIT),
            ],
        )
    };
    let snap = scan(host("Also=x.service y.service\n"));
    assert_also_carried(&snap, BUNDLE, "disable", "(b)");

    let snap = scan(wants_link(
        host("Also=x.service z.service\n"),
        "multi-user.target",
        "z.service",
    ));
    let block = vec![also_head(BUNDLE, "disable", "z.service", NEITHER)];
    assert_also_advisory(&snap, BUNDLE, &block, &["z.service", "x.service"]);
}

/// T25 (c), phase 5 (with T29's last sentence): a member that is an
/// instance with its own row. The member's enable is carried when its
/// configuration is in the export, and the owner's `enable` with it; with
/// the member's configuration excluded, the member is omitted by rule 6
/// and the owner is `RowNotCarried`, with no line to add.
#[test]
fn t25_c_an_instance_member_with_its_own_row() {
    const MEMBER: &str = "t@two.service";
    const CONF: &str = "/etc/t/two.conf";
    let reads = "[Service]\nEnvironmentFile=/etc/t/%i.conf\nExecStart=/usr/bin/t %i\n\n\
                 [Install]\nWantedBy=multi-user.target\n";
    let host = rule7_host(
        &[],
        &[(BUNDLE, "enabled"), (T_TEMPLATE, "indirect")],
        &[
            (BUNDLE, &wanted_with(&format!("Also={MEMBER}\n"))),
            (T_TEMPLATE, reads),
        ],
    )
    .file(CONF, "PORT=2\n");
    let snap = scan(instance_link(
        host,
        "/etc/systemd/system",
        "multi-user.target",
        MEMBER,
        T_TEMPLATE_PATH,
    ));
    assert_eq!(changes_named(&snap, MEMBER), 1);
    assert_also_carried(&snap, MEMBER, "enable", "(c) member");
    assert_also_carried(&snap, BUNDLE, "enable", "(c) owner");

    let head = also_head(BUNDLE, "enable", MEMBER, ROW_NOT_CARRIED);
    let excluded = [(config_item(CONF), false)];
    let containerfile = containerfile_in(&exported_after(&snap, &excluded));
    assert_eq!(
        advisory_for(&containerfile, BUNDLE),
        std::slice::from_ref(&head),
        "{containerfile}"
    );
    assert_eq!(
        omitted(&containerfile, MEMBER),
        Some(
            format!("# Omitted: {MEMBER} (its configuration {CONF} is excluded from the export)")
                .as_str()
        ),
        "{containerfile}"
    );
    for unit in [BUNDLE, MEMBER] {
        assert!(
            in_no_systemctl_instruction(&containerfile, unit),
            "{unit}:\n{containerfile}"
        );
    }
    assert_eq!(
        declined_in(&session_after(&snap, &excluded), BUNDLE),
        Some(also_declined(&head))
    );
}

/// T25 (d): a member whose row the operator excluded: an advisory; the
/// row included again: carried, with the member's own `enable`.
#[test]
fn t25_d_a_member_whose_row_is_excluded() {
    let snap = scan(rule7_host(
        &[],
        &[(BUNDLE, "enabled"), ("helper.service", "enabled")],
        &[
            (BUNDLE, &wanted_with("Also=helper.service\n")),
            ("helper.service", WANTED_UNIT),
        ],
    ));
    assert_also_carried(&snap, BUNDLE, "enable", "(d) both included");
    assert_also_carried(&snap, "helper.service", "enable", "(d) member");

    let head = also_head(BUNDLE, "enable", "helper.service", ROW_NOT_CARRIED);
    let excluded = [(unit_item("helper.service"), false)];
    let containerfile = containerfile_in(&exported_after(&snap, &excluded));
    assert_eq!(
        advisory_for(&containerfile, BUNDLE),
        std::slice::from_ref(&head),
        "{containerfile}"
    );
    assert!(
        in_no_systemctl_instruction(&containerfile, BUNDLE),
        "{containerfile}"
    );
    assert_eq!(
        declined_in(&session_after(&snap, &excluded), BUNDLE),
        Some(also_declined(&head))
    );

    let restored = [
        (unit_item("helper.service"), false),
        (unit_item("helper.service"), true),
    ];
    let containerfile = containerfile_in(&exported_after(&snap, &restored));
    assert!(
        acts(&containerfile, "enable", BUNDLE) && advisory_for(&containerfile, BUNDLE).is_empty(),
        "{containerfile}"
    );
    assert_eq!(declined_in(&session_after(&snap, &restored), BUNDLE), None);
}

/// T25 (e): a member with no row that is an instance (`enable`) or a
/// `bootc-*` unit (`disable`): `NeedsOwnRow`, and no line to add.
#[test]
fn t25_e_a_member_with_no_row_that_needs_its_own() {
    let snap = scan(rule7_host(
        &["enable t@two.service"],
        &[(BUNDLE, "enabled"), ("t@two.service", "enabled")],
        &[
            (BUNDLE, &wanted_with("Also=t@two.service\n")),
            ("t@two.service", &t_template(None)),
        ],
    ));
    assert_eq!(changes_named(&snap, "t@two.service"), 0);
    let block = vec![also_head(
        BUNDLE,
        "enable",
        "t@two.service",
        "an instance with no row here to check its configuration",
    )];
    assert_also_advisory(&snap, BUNDLE, &block, &["t@two.service"]);

    let snap = scan(rule7_host(
        &["enable bundle.service"],
        &[(BUNDLE, "disabled"), ("bootc-x.service", "disabled")],
        &[
            (BUNDLE, &wanted_with("Also=bootc-x.service\n")),
            ("bootc-x.service", WANTED_UNIT),
        ],
    ));
    assert_eq!(changes_named(&snap, "bootc-x.service"), 0);
    let block = vec![also_head(
        BUNDLE,
        "disable",
        "bootc-x.service",
        "which the image needs",
    )];
    assert_also_advisory(&snap, BUNDLE, &block, &["bootc-x.service"]);
}

/// T25 (f): a member with no unit file is `Inert`, and the `enable` is
/// carried.
#[test]
fn t25_f_a_member_with_no_unit_file_is_carried() {
    let snap = scan(rule7_host(
        &[],
        &[(BUNDLE, "enabled")],
        &[(BUNDLE, &wanted_with("Also=missing.service\n"))],
    ));
    assert_also_carried(&snap, BUNDLE, "enable", "(f)");
}

/// T25 (g): a chain `a`, `b`, `c` with `c` `Unlinked`: `enable a` and
/// `enable b` are both advisories naming `c`.
#[test]
fn t25_g_a_chain_names_its_unlinked_end_from_each_owner() {
    let snap = scan(rule7_host(
        &[],
        &[("a.service", "enabled"), ("b.service", "enabled")],
        &[
            ("a.service", &wanted_with("Also=b.service\n")),
            ("b.service", &wanted_with("Also=c.service\n")),
            ("c.service", WANTED_UNIT),
        ],
    ));
    for unit in ["a.service", "b.service"] {
        let mut block = vec![also_head(unit, "enable", "c.service", NOT_ENABLED)];
        block.extend(also_lines("enable", unit, &[("disable", "c.service")]));
        assert_also_advisory(
            &snap,
            unit,
            &block,
            &["a.service", "b.service", "c.service"],
        );
    }
}

/// T25 (h), install evidence, each with the unit otherwise carried.
/// - A packaged unit whose vendor file has `Also=bootc-x.timer`, recorded
///   `Shadowed` with an empty list because the shadow drops the key, the
///   shadow excluded: no `enable` and no `disable` (rule 5).
/// - A member recorded `Inert`, with an imported full shadow of it that
///   adds `WantedBy=` staged: `InstallFile`.
/// - A member with no row whose `/etc` drop-in supplies its `WantedBy=`,
///   the drop-in included and excluded: `InstallFile` both times.
/// - A staged local unit that sets `Also=`, and a local mount unit as a
///   member: `InstallFile`.
/// - An instance recorded from a vendor template, with a `t@.service` that
///   sets `Also=` staged from another row: omitted by rule 5, naming that
///   file.
#[test]
fn t25_h_install_evidence() {
    // The shadow that drops `Also=`.
    let shadowed = scan(
        rule7_host(
            &[],
            &[(BUNDLE, "enabled")],
            &[
                (BUNDLE, &wanted_with("Also=bootc-x.timer\n")),
                (
                    "bootc-x.timer",
                    "[Timer]\nOnCalendar=daily\n\n[Install]\nWantedBy=timers.target\n",
                ),
            ],
        )
        .etc_unit(BUNDLE, WANTED_UNIT),
    );
    assert!(change(&shadowed, BUNDLE).also.is_empty());
    assert_also_carried(&shadowed, BUNDLE, "enable", "(h) shadow included");
    let shadow = format!("/etc/systemd/system/{BUNDLE}");
    let excluded = containerfile_in(&exported_after(
        &shadowed,
        &[(drop_in_item(&shadow), false)],
    ));
    assert!(in_no_systemctl_instruction(&excluded, BUNDLE), "{excluded}");
    assert!(
        omitted(&excluded, BUNDLE).is_some_and(|l| l.ends_with(&evidence_changed(&shadow))),
        "{excluded}"
    );

    // An `Inert` member and an imported full shadow of it.
    let inert = scan(rule7_host(
        &[],
        &[(BUNDLE, "enabled")],
        &[
            (BUNDLE, &wanted_with("Also=helper.service\n")),
            ("helper.service", NO_INSTALL_UNIT),
        ],
    ));
    assert_also_carried(&inert, BUNDLE, "enable", "(h) inert member");
    let helper_shadow = "/etc/systemd/system/helper.service";
    let imported = with_config_entry(&inert, helper_shadow, WANTED_UNIT);
    let block = install_file_block(BUNDLE, "enable", helper_shadow, "helper.service");
    assert_also_advisory(&imported, BUNDLE, &block, &[]);

    // A member's `WantedBy=` from its own `/etc` drop-in, linked, and not
    // listed, so with no row.
    let drop_in = etc_drop_in("helper.service", "10-install.conf");
    let snap = scan(
        wants_link(
            rule7_host(
                &[],
                &[(BUNDLE, "enabled")],
                &[
                    (BUNDLE, &wanted_with("Also=helper.service\n")),
                    ("helper.service", NO_INSTALL_UNIT),
                ],
            ),
            "multi-user.target",
            "helper.service",
        )
        .file(&drop_in, INSTALL_DROP_IN),
    );
    assert_eq!(
        change(&snap, BUNDLE).also[0].links,
        inspectah_core::types::units::MemberLinks::Installed
    );
    assert_eq!(changes_named(&snap, "helper.service"), 0);
    let block = install_file_block(BUNDLE, "enable", &drop_in, "helper.service");
    assert_also_advisory(&snap, BUNDLE, &block, &[]);
    let ops = [(drop_in_item(&drop_in), false)];
    let containerfile = containerfile_in(&exported_after(&snap, &ops));
    assert_eq!(
        advisory_for(&containerfile, BUNDLE),
        block,
        "{containerfile}"
    );
    assert!(
        in_no_systemctl_instruction(&containerfile, BUNDLE),
        "{containerfile}"
    );
    assert_eq!(
        declined_in(&session_after(&snap, &ops), BUNDLE),
        Some(also_declined(&block[0]))
    );

    // A staged local unit that sets `Also=`.
    let local = "/etc/systemd/system/local.service";
    let snap = scan(
        rule7_host(
            &["enable helper.service"],
            &[("local.service", "enabled"), ("helper.service", "enabled")],
            &[("helper.service", WANTED_UNIT)],
        )
        .etc_unit("local.service", &wanted_with("Also=helper.service\n")),
    );
    let block = install_file_block("local.service", "enable", local, "local.service");
    assert_also_advisory(&snap, "local.service", &block, &[]);

    // A local mount unit as a member.
    let mount = "/etc/systemd/system/srv-data.mount";
    let snap = scan(
        rule7_host(
            &[],
            &[(BUNDLE, "enabled"), ("srv-data.mount", "enabled")],
            &[(BUNDLE, &wanted_with("Also=srv-data.mount\n"))],
        )
        .etc_unit("srv-data.mount", &mount_file("/srv/data", None)),
    );
    let block = install_file_block(BUNDLE, "enable", mount, "srv-data.mount");
    assert_also_advisory(&snap, BUNDLE, &block, &[]);

    // An instance recorded from its vendor template, and a template file
    // that sets `Also=` staged from another row.
    let instance = scan(instance_link(
        rule7_host(
            &[],
            &[(T_TEMPLATE, "indirect")],
            &[(T_TEMPLATE, &t_template(None))],
        ),
        "/etc/systemd/system",
        "multi-user.target",
        "t@one.service",
        T_TEMPLATE_PATH,
    ));
    assert_instance_carried(&instance, "t@one.service", "(h) instance");
    let template = etc_path(T_TEMPLATE);
    let imported = with_config_entry(
        &instance,
        &template,
        &format!("{}Also=helper.service\n", t_template(None)),
    );
    assert_instance_omitted(
        &imported,
        "t@one.service",
        &format!("# Omitted: t@one.service ({}", evidence_changed(&template)),
        "InstallEvidenceChanged",
    );
}

/// T25 (k), second half: an `/etc` drop-in that holds only `[Service]`,
/// with the file name of a member's vendor drop-in that has `[Install]`,
/// is an install file of the member: `InstallFile`.
#[test]
fn t25_k_a_members_file_named_like_its_vendor_drop_in() {
    const VENDOR: &str = "/usr/lib/systemd/system/helper.service.d/10-members.conf";
    let hiding = etc_drop_in("helper.service", "10-members.conf");
    let snap = scan(
        rule7_host(
            &["enable helper.service"],
            &[(BUNDLE, "enabled"), ("helper.service", "enabled")],
            &[
                (BUNDLE, &wanted_with("Also=helper.service\n")),
                ("helper.service", WANTED_UNIT),
            ],
        )
        .file(VENDOR, "[Install]\nWantedBy=multi-user.target\n")
        .file(&hiding, SERVICE_DROP_IN),
    );
    let block = install_file_block(BUNDLE, "enable", &hiding, "helper.service");
    assert_also_advisory(&snap, BUNDLE, &block, &[]);
}

/// T31 with § 7.4: T30 (b)'s additive unit with `Also=helper.service`, the
/// helper `Installed`: since the unit has an install file, rule 7 makes the
/// command an advisory (`InstallFile`), and no `enable` is carried.
#[test]
fn t31_an_additive_row_with_a_member_is_an_advisory() {
    let drop_in = etc_drop_in(DIFF_UNIT, "10-extra.conf");
    let host = wants_link(
        wants_link(
            diff_host(DIFF_UNIT, &wanted_with("Also=helper.service\n"), "enabled"),
            "multi-user.target",
            DIFF_UNIT,
        ),
        "extra.target",
        DIFF_UNIT,
    )
    .unit("helper.service", "enabled")
    .preset("enable helper.service")
    .vendor_unit("helper.service", WANTED_UNIT, Some(INSTALL_PACKAGE))
    .file(&drop_in, "[Install]\nWantedBy=extra.target\n");
    let snap = scan(host);
    assert_eq!(
        change(&snap, DIFF_UNIT).install_drop_ins,
        [drop_in.as_str()]
    );
    let block = install_file_block(DIFF_UNIT, "enable", &drop_in, DIFF_UNIT);
    assert_also_advisory(&snap, DIFF_UNIT, &block, &["helper.service"]);
}

/// § 7.4's owner check where the comparison is not additive (a provisional
/// ruling): a full shadow replacing `WantedBy=multi-user.target` with
/// `graphical.target` and keeping `Also=helper.service`, the helper
/// unlinked. The unit gets both install-difference blocks and rule 7's
/// advisory, and no `systemctl` instruction names the unit or the helper.
#[test]
fn a_not_additive_owner_whose_member_is_off_gets_both_advisories() {
    let vendor = wanted_with("Also=helper.service\n");
    let shadow = "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=graphical.target\n\
                  Also=helper.service\n";
    let snap = scan(
        wants_link(
            diff_host(DIFF_UNIT, &vendor, "enabled"),
            "graphical.target",
            DIFF_UNIT,
        )
        .unit("helper.service", "disabled")
        .vendor_unit("helper.service", WANTED_UNIT, Some(INSTALL_PACKAGE))
        .etc_unit(DIFF_UNIT, shadow),
    );
    assert_eq!(change(&snap, DIFF_UNIT).current_state(), UnitState::Enabled);
    let mut block = vec![also_head(
        DIFF_UNIT,
        "enable",
        "helper.service",
        NOT_ENABLED,
    )];
    block.extend(also_lines(
        "enable",
        DIFF_UNIT,
        &[("disable", "helper.service")],
    ));
    assert_also_advisory(&snap, DIFF_UNIT, &block, &["helper.service"]);
    let wants = format!("/etc/systemd/system/multi-user.target.wants/{DIFF_UNIT}");
    for (route, containerfile) in both_routes(&snap) {
        for line in host_only_block("graphical.target", DIFF_UNIT)
            .into_iter()
            .chain(vendor_only_block(
                &format!("multi-user.target.wants/{DIFF_UNIT}"),
                DIFF_UNIT,
                &wants,
            ))
        {
            assert_eq!(
                count_lines(&containerfile, &line),
                1,
                "{route}: {line}\n{containerfile}"
            );
        }
    }
}

// ---------------------------------------------------------------------
// § 10.2 rule 7 and § 12 on a fleet aggregate (T25 (j)).
// ---------------------------------------------------------------------

/// The `FleetAggregate` block for `unit`'s `verb`, naming `members`.
fn fleet_block(unit: &str, verb: &str, members: &str) -> Vec<String> {
    vec![
        format!(
            "# Advisory: {unit} is {verb}d on the source hosts. Not carried: systemctl {verb} on \
             it also {verb}s {members} (named by Also=), and fleet evidence may omit install \
             metadata."
        ),
        format!(
            "#   Before adding this after the configuration COPY, check the effective [Install] \
             and Also= of {unit} in the image, and the intended state of each member:"
        ),
        format!("#     RUN systemctl {verb} -- '{unit}'"),
    ]
}

/// `snap` with `unit`'s row included, as the operator includes a row the
/// merge narrowed out because not every host has it.
fn with_row_included(snap: &InspectionSnapshot, unit: &str) -> InspectionSnapshot {
    let mut snap = snap.clone();
    let row = snap
        .units
        .as_mut()
        .expect("a units section")
        .state_changes
        .iter_mut()
        .find(|c| c.unit().name().as_str() == unit)
        .unwrap_or_else(|| panic!("no state change for {unit}"));
    row.disposition = inspectah_core::types::FindingKind::included();
    snap
}

/// Asserts, with `unit`'s row included, that `unit` gets exactly `block`
/// and that no `systemctl` instruction names `unit` or any of `quiet`, on
/// both routes: the scan's export of the aggregate, and a refine session
/// that includes the row and [`INSTALL_PACKAGE`] (an aggregate starts
/// refine with its added packages excluded). Then that the plan declines
/// it with `FleetAggregate`, and that the row the fleet view shows carries
/// no reason from the plan (§ 12): its one text field, `attention_reason`,
/// is `collected`, what the scan recorded on the host.
fn assert_fleet_advisory(
    fleet: &InspectionSnapshot,
    unit: &str,
    block: &[String],
    quiet: &[&str],
    collected: Option<&str>,
) {
    let snap = &with_row_included(fleet, unit);
    let ops = [
        (package_item(INSTALL_PACKAGE), true),
        (unit_item(unit), true),
    ];
    let routes = [
        ("scan", containerfile_in(&render_export(snap))),
        ("refine", containerfile_in(&exported_after(fleet, &ops))),
    ];
    for (route, containerfile) in routes {
        assert_eq!(
            advisory_for(&containerfile, unit),
            block,
            "{route}:\n{containerfile}"
        );
        for name in std::iter::once(&unit).chain(quiet) {
            assert!(
                in_no_systemctl_instruction(&containerfile, name),
                "{route}: {name}:\n{containerfile}"
            );
        }
    }
    let staged = inspectah_pipeline::render::configtree::staged_writes(snap);
    let plan = inspectah_pipeline::render::unit_plan::plan_units(snap, &staged);
    let why =
        plan.declined
            .iter()
            .find_map(|(name, _, reason)| match reason {
                inspectah_pipeline::render::unit_plan::OmissionReason::AlsoMember {
                    why, ..
                } if name.as_str() == unit => Some(why.clone()),
                _ => None,
            });
    assert_eq!(
        why,
        Some(inspectah_pipeline::render::unit_plan::AlsoWhy::FleetAggregate)
    );
    let row = change(snap, unit);
    assert!(row.aggregate.is_some(), "a fleet row");
    assert_eq!(
        row.attention_reason.as_deref(),
        collected,
        "the fleet row carries no reason from the plan"
    );
}

/// T25 (j), through the real collector and merge. Host A: a vendor
/// `bundle.service` enabled against a `disable` preset with
/// `Also=helper.timer`, the helper `Inert`. Host B: the package ships
/// `helper.timer` and an unmodified `/etc` drop-in for it with an
/// `[Install]` section, owned by the package, no `bundle` unit, and no row
/// for the helper. Host A alone: the
/// enable is carried. Merged, with the bundle row included: the
/// `FleetAggregate` advisory. The same with a host B that has no drop-in,
/// where nothing else would stop the enable.
#[test]
fn t25_j_a_fleet_aggregate_advises_every_command_with_members() {
    const HELPER: &str = "helper.timer";
    const HELPER_TIMER: &str = "[Timer]\nOnCalendar=daily\n";
    let host_a = || {
        rule7_host(
            &[],
            &[(BUNDLE, "enabled")],
            &[
                (BUNDLE, &wanted_with("Also=helper.timer\n")),
                (HELPER, HELPER_TIMER),
            ],
        )
    };
    let host_b = |drop_in: bool| {
        let host = rule7_host(&[], &[], &[(HELPER, HELPER_TIMER)]);
        if drop_in {
            host.owned_file(
                &etc_drop_in(HELPER, "10-install.conf"),
                "[Install]\nWantedBy=timers.target\n",
                INSTALL_PACKAGE,
            )
        } else {
            host
        }
    };

    let alone = scan(host_a());
    assert_eq!(
        change(&alone, BUNDLE).also[0].links,
        inspectah_core::types::units::MemberLinks::Inert
    );
    assert_also_carried(&alone, BUNDLE, "enable", "(j) host A alone");

    let block = fleet_block(BUNDLE, "enable", HELPER);
    for drop_in in [false, true] {
        let fleet = merged(named(host_a(), "host-a"), named(host_b(drop_in), "host-b"));
        assert_eq!(changes_named(&fleet, HELPER), 0, "no row for the helper");
        assert_fleet_advisory(&fleet, BUNDLE, &block, &[HELPER], None);
    }
}

/// § 12: an owner row made by § 7.4's owner check on one host merges as
/// any state change, and in the aggregate it is the `FleetAggregate`
/// advisory. Host A: `m.service` at its `enable` preset with `m.socket`
/// unlinked, so an owner row. Host B: `m.socket` linked, so no owner row.
#[test]
fn an_owner_row_from_one_host_is_a_fleet_advisory() {
    let socket = "[Socket]\nListenStream=/run/m.sock\n\n[Install]\nWantedBy=sockets.target\n";
    let host = |socket_state: &str| {
        rule7_host(
            &["enable m.service"],
            &[("m.service", "enabled"), ("m.socket", socket_state)],
            &[
                ("m.service", &wanted_with("Also=m.socket\n")),
                ("m.socket", socket),
            ],
        )
    };
    let collected = change(&scan(host("disabled")), "m.service")
        .attention_reason
        .clone();
    let fleet = merged(
        named(host("disabled"), "host-a"),
        named(host("enabled"), "host-b"),
    );
    assert_eq!(
        change(&fleet, "m.service")
            .aggregate
            .as_ref()
            .map(|a| a.count),
        Some(1),
        "the owner row is host A's"
    );
    let block = fleet_block("m.service", "enable", "m.socket");
    assert_fleet_advisory(&fleet, "m.service", &block, &[], collected.as_deref());
}

/// § 12 for the other verbs (T25 (j)'s fleet rule, `disable` and `mask`).
/// Two hosts, each with a vendor `bundle.service` disabled against an
/// `enable` preset, with `Also=c.service` and `c.service` `Unlinked`: the
/// fleet block in its `disable` form. Each host alone carries the
/// `disable`. A masked `bundle.service` whose row records a member: the
/// `mask` is carried on the aggregate, as on a single host (T25 (i)).
#[test]
fn a_fleet_aggregate_advises_a_disable_and_carries_a_mask() {
    let host = || {
        rule7_host(
            &["enable bundle.service"],
            &[(BUNDLE, "disabled")],
            &[
                (BUNDLE, &wanted_with("Also=c.service\n")),
                ("c.service", WANTED_UNIT),
            ],
        )
    };
    let alone = scan(host());
    assert_eq!(
        change(&alone, BUNDLE).also[0].links,
        inspectah_core::types::units::MemberLinks::Unlinked
    );
    assert_also_carried(&alone, BUNDLE, "disable", "one host");
    let fleet = merged(named(host(), "host-a"), named(host(), "host-b"));
    let block = fleet_block(BUNDLE, "disable", "c.service");
    assert_fleet_advisory(&fleet, BUNDLE, &block, &["c.service"], None);

    let masked = || {
        rule7_host(
            &[],
            &[(BUNDLE, "masked")],
            &[
                (BUNDLE, &wanted_with("Also=c.service\n")),
                ("c.service", WANTED_UNIT),
            ],
        )
        .mask(BUNDLE)
    };
    let mut fleet = merged(named(masked(), "host-a"), named(masked(), "host-b"));
    // The collector reads no `Also=` for a mask, so the member is put on
    // the merged row: rule 7 has to leave a mask alone whatever it holds.
    let row = fleet
        .units
        .as_mut()
        .expect("a units section")
        .state_changes
        .iter_mut()
        .find(|c| c.unit().name().as_str() == BUNDLE)
        .expect("the mask row");
    assert_eq!(
        row.current_state(),
        inspectah_core::types::units::UnitState::Masked
    );
    assert!(row.also.is_empty(), "{row:?}");
    row.also = change(&alone, BUNDLE).also.clone();
    let routes = [
        ("scan", containerfile_in(&render_export(&fleet))),
        (
            "refine",
            containerfile_in(&exported_after(
                &fleet,
                &[(package_item(INSTALL_PACKAGE), true)],
            )),
        ),
    ];
    for (route, containerfile) in routes {
        assert!(
            acts(&containerfile, "mask", BUNDLE),
            "{route}:\n{containerfile}"
        );
        assert!(
            advisory_for(&containerfile, BUNDLE).is_empty(),
            "{route}:\n{containerfile}"
        );
    }
}

// ---------------------------------------------------------------------
// § 7.6 and § 10.5: the default target (T26).
// ---------------------------------------------------------------------

/// The link `systemctl set-default` writes.
const DEFAULT_TARGET_LINK: &str = "/etc/systemd/system/default.target";

/// A host whose `systemd` package (in the base image) ships
/// `graphical.target` and `cockpit.socket`, with `app.service` enabled
/// against `disable *`, and `default.target` linked to `link` when given.
fn target_host(link: Option<&str>) -> UnitHost {
    let host = UnitHost::new()
        .base_package("glibc")
        .base_package("systemd")
        .package(INSTALL_PACKAGE)
        .unit("app.service", "enabled")
        .preset("disable *")
        .vendor_unit("app.service", WANTED_UNIT, Some(INSTALL_PACKAGE))
        .vendor_unit(
            "graphical.target",
            "[Unit]\nDescription=Graphical Interface\n",
            Some("systemd"),
        )
        .vendor_unit(
            "cockpit.socket",
            "[Socket]\nListenStream=9090\n",
            Some("systemd"),
        );
    match link {
        Some(target) => host.link(DEFAULT_TARGET_LINK, target),
        None => host,
    }
}

/// The `set-default` line for `target`.
fn set_default_line(target: &str) -> String {
    format!("RUN systemctl set-default -- '{target}'")
}

/// The lines of `containerfile` that name `unit`.
fn lines_naming<'a>(containerfile: &'a str, unit: &str) -> Vec<&'a str> {
    containerfile.lines().filter(|l| l.contains(unit)).collect()
}

/// T26: a `default.target` link to `graphical.target` is recorded as an
/// included default target and yields `RUN systemctl set-default --
/// 'graphical.target'`, the only line naming the target, after the unit
/// state section's `# Omitted:` lines and before its `enable` (§ 10.1).
/// No link: no record and no `set-default`.
#[test]
fn t26_the_default_target_is_set_in_the_image() {
    let snap = scan(target_host(Some(
        "/usr/lib/systemd/system/graphical.target",
    )));
    let recorded = snap
        .units
        .as_ref()
        .and_then(|u| u.default_target.as_ref())
        .expect("a default target");
    assert_eq!(
        recorded.target.name().as_unit_name().as_str(),
        "graphical.target"
    );
    assert!(recorded.disposition.is_included());
    assert!(recorded.aggregate.is_none());
    for (route, containerfile) in both_routes(&snap) {
        assert_eq!(
            lines_naming(&containerfile, "graphical.target"),
            [set_default_line("graphical.target")],
            "{route}:\n{containerfile}"
        );
        let at = |needle: &str| {
            containerfile
                .find(needle)
                .unwrap_or_else(|| panic!("{route}: no {needle}:\n{containerfile}"))
        };
        assert!(
            at("# === Systemd Unit State ===") < at("RUN systemctl set-default")
                && at("RUN systemctl set-default") < at("RUN systemctl enable"),
            "{route}:\n{containerfile}"
        );
    }

    let none = scan(target_host(None));
    assert!(
        none.units
            .as_ref()
            .is_some_and(|u| u.default_target.is_none())
    );
    for (route, containerfile) in both_routes(&none) {
        assert!(
            !containerfile.contains("set-default"),
            "{route}:\n{containerfile}"
        );
        assert!(acts(&containerfile, "enable", "app.service"), "{route}");
    }
}

/// T26: the row's toggle removes and restores the line. A refine operation
/// on a snapshot with no default target is refused, naming it.
#[test]
fn t26_the_default_target_toggle_removes_and_restores_the_line() {
    let snap = scan(target_host(Some(
        "/usr/lib/systemd/system/graphical.target",
    )));
    let line = set_default_line("graphical.target");
    let off = containerfile_in(&exported_after(&snap, &[(ItemId::DefaultTarget, false)]));
    assert!(!off.contains("set-default"), "{off}");
    assert!(acts(&off, "enable", "app.service"), "{off}");
    let on = containerfile_in(&exported_after(
        &snap,
        &[
            (ItemId::DefaultTarget, false),
            (ItemId::DefaultTarget, true),
        ],
    ));
    assert!(on.lines().any(|l| l == line), "{on}");

    let none = scan(target_host(None));
    let err = session(&none)
        .apply(set_include(ItemId::DefaultTarget, false))
        .expect_err("no default target to toggle");
    assert!(err.to_string().contains("default target"), "{err}");
}

/// T26: a local target, `default.target` linked to its file under `/etc`:
/// carried while its config row is included, and omitted with
/// `UnitFileNotStaged` when the row is excluded (§ 10.5, rule 5).
#[test]
fn t26_a_local_default_target_follows_its_config_row() {
    const LOCAL: &str = "/etc/systemd/system/custom.target";
    let snap =
        scan(target_host(Some(LOCAL)).etc_unit("custom.target", "[Unit]\nDescription=Custom\n"));
    assert_eq!(
        snap.units
            .as_ref()
            .and_then(|u| u.default_target.as_ref())
            .map(|t| t.target.fragment().clone()),
        Some(UnitFragment::Local {
            file: "custom.target".to_owned().try_into().expect("a unit name"),
        })
    );
    for (route, containerfile) in both_routes(&snap) {
        assert!(
            containerfile
                .lines()
                .any(|l| l == set_default_line("custom.target")),
            "{route}:\n{containerfile}"
        );
    }
    let excluded = containerfile_in(&exported_after(&snap, &[(config_item(LOCAL), false)]));
    assert!(!excluded.contains("set-default"), "{excluded}");
    assert_eq!(
        omitted(&excluded, "custom.target"),
        Some("# Omitted: custom.target (its unit file is not in the export)"),
        "{excluded}"
    );
}

/// T26: a replaced target, `default.target` linked to a full shadow of
/// `graphical.target` under `/etc`: carried while the shadow's drop-in row
/// is included, and with it excluded omitted with its own reason, which
/// names the shadow and says the image cannot set it, not the `[Install]`
/// text `set-default` never reads (Mark Russell, 2026-11-15).
/// The same line on the scan export (the row excluded in the snapshot) and
/// the refine export, and the same text in refine's view.
#[test]
fn t26_a_shadowed_default_target_whose_shadow_is_excluded_names_it() {
    const SHADOW: &str = "/etc/systemd/system/graphical.target";
    let snap = scan(target_host(Some(SHADOW)).etc_unit(
        "graphical.target",
        "[Unit]\nDescription=Graphical, edited\n",
    ));
    assert!(
        matches!(
            snap.units
                .as_ref()
                .and_then(|u| u.default_target.as_ref())
                .map(|t| t.target.fragment()),
            Some(UnitFragment::Shadowed { .. })
        ),
        "a Shadowed default target"
    );
    for (route, containerfile) in both_routes(&snap) {
        assert!(
            containerfile
                .lines()
                .any(|l| l == set_default_line("graphical.target")),
            "{route}:\n{containerfile}"
        );
    }

    let text = format!(
        "graphical.target (the replacement file recorded for it, {SHADOW}, is not in the \
         export, so the image cannot set that file as the default target)"
    );
    let line = format!("# Omitted: {text}");
    let refine = session_after(&snap, &[(drop_in_item(SHADOW), false)]);
    assert_eq!(
        refine
            .decisions()
            .default_target
            .clone()
            .and_then(|v| v.declined),
        Some(inspectah_refine::projection::DeclinedView {
            renders: inspectah_refine::projection::DeclinedRenders::Omitted,
            reason: "DefaultTargetFileNotStaged".into(),
            text: text.clone(),
        })
    );
    let mut excluded_snap = snap.clone();
    excluded_snap
        .units
        .as_mut()
        .expect("a units section")
        .drop_ins
        .iter_mut()
        .find(|d| d.path == SHADOW)
        .expect("the shadow's drop-in row")
        .disposition = FindingKind::excluded();
    for (route, containerfile) in [
        ("scan", containerfile_in(&render_export(&excluded_snap))),
        ("refine", containerfile_in(&export_session(&refine))),
    ] {
        assert!(
            !containerfile.contains("set-default"),
            "{route}:\n{containerfile}"
        );
        assert_eq!(
            omitted(&containerfile, "graphical.target"),
            Some(line.as_str()),
            "{route}:\n{containerfile}"
        );
    }
}

/// T26: a `Shadowed` default target whose package is not in the base
/// image, with its replacement excluded. The text says only that the
/// recorded file is missing, never that the package's file would be set:
/// with the package excluded too, the image has no file for the target and
/// `set-default` fails. The same line with the package in the export (the
/// control) and excluded, on the scan export, the refine export and in
/// refine's view.
#[test]
fn t26_a_shadowed_default_target_with_shadow_and_package_excluded_claims_no_file() {
    const SHADOW: &str = "/etc/systemd/system/app.target";
    const PACKAGE: &str = "apptargets";
    let snap = scan(
        target_host(Some(SHADOW))
            .package(PACKAGE)
            .vendor_unit("app.target", "[Unit]\nDescription=App\n", Some(PACKAGE))
            .etc_unit("app.target", "[Unit]\nDescription=App, edited\n"),
    );
    assert!(
        matches!(
            snap.units
                .as_ref()
                .and_then(|u| u.default_target.as_ref())
                .map(|t| t.target.fragment()),
            Some(UnitFragment::Shadowed { owning_package: Some(p), .. }) if p == PACKAGE
        ),
        "a Shadowed default target owned by {PACKAGE}"
    );

    let text = format!(
        "app.target (the replacement file recorded for it, {SHADOW}, is not in the export, so \
         the image cannot set that file as the default target)"
    );
    let line = format!("# Omitted: {text}");
    for (case, package_kept) in [("package kept", true), ("package excluded", false)] {
        let mut ops = vec![(drop_in_item(SHADOW), false)];
        if !package_kept {
            ops.push((package_item(PACKAGE), false));
        }
        let refine = session_after(&snap, &ops);
        assert_eq!(
            default_target_declined(&refine),
            Some(inspectah_refine::projection::DeclinedView {
                renders: inspectah_refine::projection::DeclinedRenders::Omitted,
                reason: "DefaultTargetFileNotStaged".into(),
                text: text.clone(),
            }),
            "{case}"
        );
        let mut excluded_snap = snap.clone();
        let units = excluded_snap.units.as_mut().expect("a units section");
        units
            .drop_ins
            .iter_mut()
            .find(|d| d.path == SHADOW)
            .expect("the shadow's drop-in row")
            .disposition = FindingKind::excluded();
        if !package_kept {
            excluded_snap
                .rpm
                .as_mut()
                .expect("an rpm section")
                .packages_added
                .iter_mut()
                .find(|p| p.name == PACKAGE)
                .expect("the target's package")
                .disposition = FindingKind::excluded();
        }
        for (route, containerfile) in [
            ("scan", containerfile_in(&render_export(&excluded_snap))),
            ("refine", containerfile_in(&export_session(&refine))),
        ] {
            assert_eq!(
                containerfile.contains(PACKAGE),
                package_kept,
                "{case}, {route}: the package is in the export only when kept:\n{containerfile}"
            );
            assert!(
                !containerfile.contains("set-default"),
                "{case}, {route}:\n{containerfile}"
            );
            assert_eq!(
                omitted(&containerfile, "app.target"),
                Some(line.as_str()),
                "{case}, {route}:\n{containerfile}"
            );
        }
    }
}

/// `snap` with the first drop-in row at `path` included, as the scan
/// export of an aggregate reads an operator's include.
fn with_first_drop_in_included(snap: &InspectionSnapshot, path: &str) -> InspectionSnapshot {
    let mut snap = snap.clone();
    snap.units
        .as_mut()
        .expect("a units section")
        .drop_ins
        .iter_mut()
        .find(|d| d.path == path)
        .unwrap_or_else(|| panic!("no drop-in row at {path}"))
        .disposition = FindingKind::included();
    snap
}

/// The default target's decline as `session`'s view holds it.
fn default_target_declined(
    session: &RefineSession,
) -> Option<inspectah_refine::projection::DeclinedView> {
    session
        .decisions()
        .default_target
        .clone()
        .and_then(|v| v.declined)
}

/// T26 on a fleet aggregate: `host-a` sets the package's
/// `graphical.target`, `host-b` a full replacement of it under `/etc`.
/// The aggregate keeps `host-a`'s `Vendor` target and sets it while the
/// replacement is not in the export. With the replacement included, the
/// image would load that file for the name, so the target is omitted with
/// a reason that names the file and says the image cannot set the recorded
/// one, not the `[Install]` text `set-default` never reads. The same line on the
/// scan export and the refine export, and the same text in refine's view.
#[test]
fn t26_a_vendor_default_target_with_a_file_staged_at_its_name_names_it() {
    const SHADOW: &str = "/etc/systemd/system/graphical.target";
    const VENDOR: &str = "/usr/lib/systemd/system/graphical.target";
    let merged = merged(
        named(target_host(Some(VENDOR)), "host-a"),
        named(
            target_host(Some(SHADOW)).etc_unit(
                "graphical.target",
                "[Unit]\nDescription=Graphical, edited\n",
            ),
            "host-b",
        ),
    );
    let recorded = merged
        .units
        .as_ref()
        .and_then(|u| u.default_target.as_ref())
        .expect("a default target");
    assert!(
        matches!(recorded.target.fragment(), UnitFragment::Vendor { .. }),
        "host-a's Vendor target: {:?}",
        recorded.target.fragment()
    );
    assert!(recorded.disposition.is_included(), "both hosts set it");
    for (route, containerfile) in both_routes(&merged) {
        assert!(
            containerfile
                .lines()
                .any(|l| l == set_default_line("graphical.target")),
            "{route}:\n{containerfile}"
        );
    }

    let text = format!(
        "graphical.target (the export carries a unit file at {SHADOW}, which is not the file \
         recorded for it, so the image cannot set the recorded file as the default target)"
    );
    let line = format!("# Omitted: {text}");
    let refine = session_after(&merged, &[(drop_in_item(SHADOW), true)]);
    assert_eq!(
        default_target_declined(&refine),
        Some(inspectah_refine::projection::DeclinedView {
            renders: inspectah_refine::projection::DeclinedRenders::Omitted,
            reason: "DefaultTargetFileStaged".into(),
            text: text.clone(),
        })
    );
    for (route, containerfile) in [
        (
            "scan",
            containerfile_in(&render_export(&with_first_drop_in_included(
                &merged, SHADOW,
            ))),
        ),
        ("refine", containerfile_in(&export_session(&refine))),
    ] {
        assert!(
            !containerfile.contains("set-default"),
            "{route}:\n{containerfile}"
        );
        assert_eq!(
            omitted(&containerfile, "graphical.target"),
            Some(line.as_str()),
            "{route}:\n{containerfile}"
        );
    }
}

/// T26 on a fleet aggregate: both hosts set a full replacement of
/// `graphical.target` under `/etc`, with different content. With a
/// replacement included, the export carries one host's file for every
/// host, so the target is omitted with a reason that says so, not the
/// `[Install]` text `set-default` never reads. The same line on the scan
/// export and the refine export, and the same text in refine's view.
#[test]
fn t26_a_shadowed_default_target_with_two_variants_says_so() {
    const SHADOW: &str = "/etc/systemd/system/graphical.target";
    let host = |hostname: &str, description: &str| {
        named(
            target_host(Some(SHADOW)).etc_unit(
                "graphical.target",
                &format!("[Unit]\nDescription={description}\n"),
            ),
            hostname,
        )
    };
    let merged = merged(
        host("host-a", "Graphical, a"),
        host("host-b", "Graphical, b"),
    );
    let recorded = merged
        .units
        .as_ref()
        .and_then(|u| u.default_target.as_ref())
        .expect("a default target");
    assert!(
        matches!(recorded.target.fragment(), UnitFragment::Shadowed { .. }),
        "a Shadowed target: {:?}",
        recorded.target.fragment()
    );
    assert!(recorded.disposition.is_included(), "both hosts set it");

    let text = format!(
        "graphical.target (the source hosts' files for it at {SHADOW} differ and the export \
         carries only one of them, so the image cannot set each host's file as the default \
         target)"
    );
    let line = format!("# Omitted: {text}");
    let refine = session_after(&merged, &[(drop_in_item(SHADOW), true)]);
    assert_eq!(
        default_target_declined(&refine),
        Some(inspectah_refine::projection::DeclinedView {
            renders: inspectah_refine::projection::DeclinedRenders::Omitted,
            reason: "DefaultTargetFileVariants".into(),
            text: text.clone(),
        })
    );
    for (route, containerfile) in [
        (
            "scan",
            containerfile_in(&render_export(&with_first_drop_in_included(
                &merged, SHADOW,
            ))),
        ),
        ("refine", containerfile_in(&export_session(&refine))),
    ] {
        assert!(
            !containerfile.contains("set-default"),
            "{route}:\n{containerfile}"
        );
        assert_eq!(
            omitted(&containerfile, "graphical.target"),
            Some(line.as_str()),
            "{route}:\n{containerfile}"
        );
    }
}

/// § 7.6: a `default.target` link to anything but a target gives a
/// structured warning naming the link and no record.
#[test]
fn a_default_target_link_to_a_socket_is_a_warning() {
    let snap = scan(target_host(Some("/usr/lib/systemd/system/cockpit.socket")));
    assert!(
        snap.units
            .as_ref()
            .is_some_and(|u| u.default_target.is_none())
    );
    assert!(
        snap.warnings.iter().any(|w| w.inspector == "units"
            && w.message.contains(DEFAULT_TARGET_LINK)
            && w.message.contains("cockpit.socket")),
        "{:?}",
        snap.warnings
    );
    for (route, containerfile) in both_routes(&snap) {
        assert!(
            !containerfile.contains("set-default"),
            "{route}:\n{containerfile}"
        );
    }
}

/// § 7.6's other warning paths: a dangling link to `missing.target`, a
/// regular file at `default.target`, and a link whose last component is
/// not a unit name. Each gives no record, a units warning naming the link
/// and saying why, and no `set-default`.
#[test]
fn a_default_target_that_cannot_be_recorded_is_a_warning() {
    let cases = [
        (
            "dangling link",
            target_host(Some("/usr/lib/systemd/system/missing.target")),
            "missing.target has no unit file this scan reads",
        ),
        (
            "regular file",
            target_host(None).file(DEFAULT_TARGET_LINK, "graphical.target\n"),
            "could not be read as a link",
        ),
        (
            "not a unit name",
            target_host(Some("/usr/lib/systemd/system/graphical")),
            "/usr/lib/systemd/system/graphical",
        ),
    ];
    for (label, host, why) in cases {
        let snap = scan(host);
        assert!(
            snap.units
                .as_ref()
                .is_some_and(|u| u.default_target.is_none()),
            "{label}"
        );
        assert!(
            snap.warnings.iter().any(|w| w.inspector == "units"
                && w.message.contains(DEFAULT_TARGET_LINK)
                && w.message.contains(why)),
            "{label}: {:?}",
            snap.warnings
        );
        for (route, containerfile) in both_routes(&snap) {
            assert!(
                !containerfile.contains("set-default"),
                "{label} {route}:\n{containerfile}"
            );
        }
    }
}

/// § 10.5, rule 5's package half on the default target: a target its
/// package ships is omitted while refine excludes the package, with the
/// package reason, and set again when the package is included.
#[test]
fn a_default_target_follows_its_package() {
    let snap = scan(
        target_host(Some("/usr/lib/systemd/system/app.target")).vendor_unit(
            "app.target",
            "[Unit]\nDescription=App\n",
            Some(INSTALL_PACKAGE),
        ),
    );
    let package = package_item(INSTALL_PACKAGE);
    let excluded = containerfile_in(&exported_after(&snap, &[(package.clone(), false)]));
    assert!(!excluded.contains("set-default"), "{excluded}");
    assert_eq!(
        omitted(&excluded, "app.target"),
        Some(format!("# Omitted: app.target (package '{INSTALL_PACKAGE}' was excluded)").as_str()),
        "{excluded}"
    );
    let included = containerfile_in(&exported_after(
        &snap,
        &[(package.clone(), false), (package, true)],
    ));
    assert!(
        included
            .lines()
            .any(|l| l == set_default_line("app.target")),
        "{included}"
    );
    assert_eq!(omitted(&included, "app.target"), None, "{included}");
}

/// The warnings that list the link at `path` among the links the scan does
/// not report (§ 7.3), by message.
fn unreported_link_warnings<'s>(snap: &'s InspectionSnapshot, path: &str) -> Vec<&'s str> {
    snap.warnings
        .iter()
        .filter(|w| {
            w.extra
                .get("links")
                .and_then(|links| links.as_array())
                .is_some_and(|links| links.iter().any(|link| link["path"] == path))
        })
        .map(|w| w.message.as_str())
        .collect()
}

/// § 7.3's units that a link under `/etc` names and the listing does not
/// print as a unit this scan reads, through the scan. None of these links
/// is reported as a `ManualLink` or carried, and each that no other warning
/// accounts for gets a warning naming the link, its unit and why: a unit
/// the listing does not print at all, a masked unit's leftover link, a link
/// to a unit the search path refuses (beside the refusal warning), a link
/// to an alias name, and a dangling link to a name the listing does not
/// print. A dangling link to a listed `static` name, or to an instance,
/// keeps the unit's "no unit file" warning and gets no other. Each host
/// lists one unrelated unit, since an empty listing ends the section before
/// any link is read.
#[test]
fn units_an_etc_link_names_that_the_listing_does_not_print() {
    let wants = |unit: &str| format!("/etc/systemd/system/multi-user.target.wants/{unit}");
    let vendor = |unit: &str| format!("/usr/lib/systemd/system/{unit}");
    let host = || {
        UnitHost::new()
            .unit("other.service", "disabled")
            .vendor_unit("other.service", WANTS_MULTI_USER, None)
    };
    let alias_unit = "[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\n\
                      Alias=al.service\n";
    let no_unit_file = "no unit file under /etc/systemd/system or /usr/lib/systemd/system";
    // (label, host, the linked unit, the unit's own warning, the link's warning)
    type Case<'a> = (&'a str, UnitHost, &'a str, Option<&'a str>, Option<String>);
    let cases: Vec<Case> = vec![
        (
            "not listed, a vendor unit with [Install]",
            host()
                .vendor_unit("ghost.service", WANTS_MULTI_USER, None)
                .link(&wants("ghost.service"), &vendor("ghost.service")),
            "ghost.service",
            None,
            Some(format!(
                "link {} (ghost.service) is not reported or carried: systemctl \
                 list-unit-files does not list its unit as one this scan reads, so the scan \
                 does not read the unit or its links",
                wants("ghost.service")
            )),
        ),
        (
            "listed masked, with a leftover link",
            host()
                .unit("m.service", "masked")
                .vendor_unit("m.service", WANTS_MULTI_USER, None)
                .mask("m.service")
                .link(&wants("m.service"), &vendor("m.service")),
            "m.service",
            None,
            Some(format!(
                "link {} (m.service) is not reported or carried: its unit is masked, so the \
                 link has no effect",
                wants("m.service")
            )),
        ),
        (
            "refused by the search path",
            host()
                .unit("u.service", "enabled")
                .vendor_unit("u.service", WANTS_MULTI_USER, None)
                .file("/run/systemd/system/u.service", WANTS_MULTI_USER),
            "u.service",
            Some("is on the systemd search path"),
            Some(format!(
                "link {} (u.service, changed by /run/systemd/system/u.service) is not \
                 reported or carried: a file on the systemd search path outside the two unit \
                 directories this scan reads changes its unit, so the scan does not read the \
                 unit's links",
                wants("u.service")
            )),
        ),
        (
            "an alias name, listed alias",
            host()
                .unit("real.service", "disabled")
                .unit("al.service", "alias")
                .vendor_unit("real.service", alias_unit, None)
                .link(&vendor("al.service"), &vendor("real.service"))
                .link(&wants("al.service"), &vendor("real.service")),
            "al.service",
            None,
            Some(format!(
                "link {} (al.service, an alias of real.service) is not reported or carried: \
                 it names its unit by an alias, and the scan reads a unit's links under the \
                 unit's own name only",
                wants("al.service")
            )),
        ),
        (
            "dangling, an instance of no template",
            host().link(&wants("nof@x.service"), &vendor("nof@.service")),
            "nof@x.service",
            Some(no_unit_file),
            None,
        ),
        (
            "dangling, not listed",
            host().link(&wants("gone.service"), &vendor("gone.service")),
            "gone.service",
            None,
            Some(format!(
                "link {} (gone.service) is not reported or carried: there is no unit file of \
                 its name under /etc/systemd/system or /usr/lib/systemd/system, so the link \
                 points at nothing",
                wants("gone.service")
            )),
        ),
        (
            "dangling, listed static",
            host()
                .unit("gone.service", "static")
                .link(&wants("gone.service"), &vendor("gone.service")),
            "gone.service",
            Some(no_unit_file),
            None,
        ),
    ];
    for (label, host, unit, unit_warning, link_warning) in cases {
        let snap = scan(host.preset("disable *"));
        assert!(
            links_of(&snap).iter().all(|l| l.unit.as_str() != unit),
            "{label}: {:?}",
            links_of(&snap)
        );
        let named: Vec<&str> = snap
            .warnings
            .iter()
            .filter(|w| w.message.starts_with(&format!("unit {unit} ")))
            .map(|w| w.message.as_str())
            .collect();
        match unit_warning {
            Some(text) => assert!(
                named.len() == 1 && named[0].contains(text),
                "{label}: {named:?}"
            ),
            None => assert!(named.is_empty(), "{label}: {named:?}"),
        }
        assert_eq!(
            unreported_link_warnings(&snap, &wants(unit)),
            link_warning.as_slice(),
            "{label}: {:?}",
            snap.warnings
        );
        let expected_changes = usize::from(unit == "m.service");
        assert_eq!(changes_named(&snap, unit), expected_changes, "{label}");
        // Not a read failure: the section is not degraded.
        assert_eq!(snap.completeness, Completeness::Complete, "{label}");
    }

    // Many links of one kind are one warning listing every link, with the
    // path and unit of each among its fields.
    let masked = scan(
        host()
            .preset("disable *")
            .unit("m1.service", "masked")
            .unit("m2.service", "masked")
            .vendor_unit("m1.service", WANTS_MULTI_USER, None)
            .vendor_unit("m2.service", WANTS_MULTI_USER, None)
            .mask("m1.service")
            .mask("m2.service")
            .link(&wants("m1.service"), &vendor("m1.service"))
            .link(&wants("m2.service"), &vendor("m2.service")),
    );
    let grouped: Vec<_> = masked
        .warnings
        .iter()
        .filter(|w| w.extra.contains_key("links"))
        .collect();
    assert_eq!(grouped.len(), 1, "{:?}", masked.warnings);
    assert_eq!(
        grouped[0].message,
        format!(
            "2 links are not reported or carried: for each, its unit is masked, so the link \
             has no effect. The links: {} (m1.service); {} (m2.service)",
            wants("m1.service"),
            wants("m2.service")
        )
    );
    assert_eq!(grouped[0].extra["cause"], serde_json::json!("masked"));
    assert_eq!(
        grouped[0].extra["links"],
        serde_json::json!([
            {"path": wants("m1.service"), "unit": "m1.service"},
            {"path": wants("m2.service"), "unit": "m2.service"},
        ])
    );

    // The control: a listed `static` unit with no `[Install]` and the same
    // link is admitted, its link is a `StaticUnit` link, and no warning
    // lists it.
    let control = scan(
        UnitHost::new()
            .unit("ghost.service", "static")
            .preset("disable *")
            .vendor_unit("ghost.service", NO_INSTALL, None)
            .link(&wants("ghost.service"), &vendor("ghost.service")),
    );
    assert_eq!(
        links_of(&control)
            .iter()
            .map(|l| (l.unit.as_str(), l.cause))
            .collect::<Vec<_>>(),
        [("ghost.service", ManualLinkCause::StaticUnit)]
    );
    assert!(
        control
            .warnings
            .iter()
            .all(|w| !w.extra.contains_key("links")),
        "{:?}",
        control.warnings
    );
}
