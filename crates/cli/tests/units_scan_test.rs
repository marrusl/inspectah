//! The units section through the scan path, on collector-built hosts.
//!
//! Every test here runs the real RPM, config and units inspectors over a
//! [`UnitHost`] through the scan command's own calls (`collect()`, then
//! `validate`, then `redact`), and follows the snapshot to the surface it
//! asserts on: the rendered export, a refine session, or that session's
//! export. A hand-built `UnitSection` would be free to encode a layout the
//! collectors cannot produce.

#[path = "support/unit_hosts.rs"]
mod unit_hosts;

use inspectah_core::aggregate::merge_snapshots;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::FindingKind;
use inspectah_core::types::units::{
    AdminStorageFragment, CarriedUnit, MountUnitFinding, UnitFragment, UnitState,
};
use inspectah_pipeline::render;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, RefineError, RefinementOp};
use unit_hosts::{UnitHost, export_session, reload, render_export, scan, scan_unredacted, session};

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
    assert!(finding.nofail, "{route}");
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
/// A config file no other section owns — the negative control that keeps
/// the assertions below from passing because config rows never decide.
const PLAIN_CONFIG: &str = "/etc/httpd/conf/httpd.conf";

/// `DROP_IN`'s parent: at its preset, so it has no `state_changes` row.
const PRESET_MATCHED_UNIT: &str = "httpd.service";
/// `FULL_SHADOW`'s parent: diverges from its preset, so it does.
const PRESET_DIVERGING_UNIT: &str = "custom.service";

const DROP_IN_CONTENT: &str = "[Service]\nLimitNOFILE=65535\n";
const FULL_SHADOW_CONTENT: &str =
    "[Unit]\nDescription=Local custom unit\n\n[Service]\nExecStart=/usr/local/bin/custom\n";
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
            "[Unit]\nDescription=Vendor custom unit\n",
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
/// while reporting it as included — an accepted include that the export
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
/// stages nothing, so the shadow is not staged: the enable is declined
/// with `InstallEvidenceChanged`. Control: the readable twin is enabled.
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
            host.etc_unit("blob.service", "[Service]\nExecStart=/usr/bin/blob -v\n")
        } else {
            host.file_error(BLOB_UNIT, std::io::ErrorKind::InvalidData)
        }
    };

    let snap = scan(host(false));
    assert!(
        matches!(
            fragment_of(&snap, "blob.service"),
            UnitFragment::Shadowed { .. }
        ),
        "{:?}",
        fragment_of(&snap, "blob.service")
    );
    assert!(
        snap.units
            .as_ref()
            .is_some_and(|u| u.drop_ins.iter().all(|d| d.path != BLOB_UNIT)),
        "no drop-in row for a shadow that was not read"
    );
    let containerfile = containerfile_in(&render_export(&snap));
    assert!(
        in_no_systemctl_instruction(&containerfile, "blob.service"),
        "{containerfile}"
    );
    assert!(
        omitted(&containerfile, "blob.service")
            .is_some_and(|l| l.ends_with(&evidence_changed(BLOB_UNIT))),
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
/// content. Whichever variant is selected, the unit's command is declined
/// with `InstallEvidenceChanged`: the row may have been recorded from the
/// other variant. Control: the same content on both hosts is one variant,
/// and the enable is carried.
#[test]
fn t18_d_a_shadow_with_two_variants_declines_the_command() {
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

    for selected in &contents {
        let mut snap = merged.clone();
        for row in snap
            .units
            .as_mut()
            .expect("a units section")
            .drop_ins
            .iter_mut()
            .filter(|d| d.path == APP_UNIT)
        {
            row.disposition = FindingKind::from_bool(row.content == *selected);
        }
        let tree = render_export(&snap);
        assert_eq!(
            std::fs::read_to_string(tree.path().join("drop-ins").join(&APP_UNIT[1..])).ok(),
            Some(selected.clone()),
            "the selected variant is staged"
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
    let control = containerfile_in(&render_export(&same));
    assert!(acts(&control, "enable", "app.service"), "{control}");
    assert!(omitted(&control, "app.service").is_none(), "{control}");
}

/// T13 quoting and plan review focus 1: an instance name with `\x2d`, the
/// root slice `-.slice` and a 255-byte name render as single-quoted
/// operands after `--`, in kind then name order, and each survives a refine
/// export unchanged (the backslashes stay inside the quotes).
///
/// Built by hand: the collector records no instance state changes until a
/// later phase, so the instance is a vendor unit whose file is its
/// template, which the state change type admits for an enabled instance.
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
            change(&long, &long, UnitState::Disabled),
        ],
        ..Default::default()
    });
    let expected = vec![
        "RUN systemctl enable -- \\".to_owned(),
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

/// § 10.1's two sentences about what a carried unit state means on the
/// target.
const README_FIRST_BOOT: &str = "The unit states in the Containerfile's Systemd Unit State \
     section survive first boot: on both bootc base images, installed with `bootc install \
     to-existing-root`, first boot does not re-apply presets. A later `systemctl \
     preset-all` puts every unit back to preset.";
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
            carry_line("enable", "srv.mount"),
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
fn opt_line(path: &str) -> String {
    format!(
        "#   /opt is a directory of the read-only image in the RHEL bootc base image, not a link \
         into /var. The unit keeps its name. The mount point must already exist in the image, \
         and a mount there hides the image's own content under {path}."
    )
}
const TMP_MOUNT_LINE: &str = "#   The bootc base image starts tmp.mount from a link it ships, \
     so this file replaces the unit that mounts /tmp on the target.";
const NOFAIL_LINE: &str = "#   No nofail option. If the unit is started at boot and its device \
     is missing, units that require it fail, and boot can stop in emergency mode.";
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
                carry_line("enable", "srv.mount"),
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
    let without_file = vec![
        advisory_fact(UNIT, "enabled"),
        var_symlink_line("/srv"),
        carry_line("enable", UNIT),
    ];
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
                carry_line("enable", "srv-data.mount"),
            ],
        ),
        (
            "srv-data.automount",
            vec![
                advisory_fact("srv-data.automount", "enabled"),
                FILE_STAGED_LINE.to_owned(),
                var_symlink_line("/srv"),
                carry_line("enable", "srv-data.automount"),
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
/// `tmp.mount` gets no `To carry it` line, since § 10.3 refuses that mask.
/// Controls: the same mask over a package-owned file is a `mask` command
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
            &vendor_file,
            None,
        ));
        let mut expected = vec![advisory_fact(unit, "enabled")];
        if under_srv {
            expected.push(var_symlink_line("/srv"));
        }
        if has_options {
            expected.push(NOFAIL_LINE.to_owned());
        }
        expected.push(carry_line("enable", unit));
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
    const UNIT: &str = "var-lib-pkg.mount";
    const PACKAGE: &str = "pkgmount";
    let path = etc_path(UNIT);
    let drop_in = format!("{path}.d/x.conf");
    let vendor = mount_file("/var/lib/pkg", None);
    let package_host = named(owned_unit_host(UNIT, PACKAGE, "enabled", &vendor), "host-a");
    let copy_host = named(
        owned_unit_host(UNIT, PACKAGE, "enabled", &vendor)
            .etc_unit(UNIT, &mount_file("/var/lib/pkg", Some("nofail")))
            .file(&drop_in, "[Mount]\nTimeoutSec=30\n"),
        "host-b",
    );
    assert!(
        matches!(
            change(&package_host, UNIT).unit(),
            CarriedUnit::PackagedStorage(_)
        ),
        "host-a records a package-owned state change"
    );
    let copy_units = copy_host.units.as_ref().expect("a units section");
    assert!(
        copy_units
            .state_changes
            .iter()
            .all(|c| c.unit().name().as_str() != UNIT),
        "host-b records no state change for the unit"
    );
    assert!(
        copy_units
            .mount_findings
            .iter()
            .any(|f| f.unit.name().as_unit_name().as_str() == UNIT),
        "host-b records a finding"
    );

    let merged = merged(package_host, copy_host);
    let units = merged.units.as_ref().expect("a units section");
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
        assert!(staged_at(&tree, &path), "{step}: the file is staged");
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
/// brings the mask back.
#[test]
fn t18_c_a_mask_beside_another_hosts_unit_file_is_t16() {
    const UNIT: &str = "kdump.service";
    let path = etc_path(UNIT);
    let masked = named(
        owned_unit_host(UNIT, "kexec-tools", "masked", VENDOR_SERVICE),
        "host-a",
    );
    let local = named(
        UnitHost::new()
            .unit(UNIT, "enabled")
            .preset("disable *")
            .etc_unit(UNIT, "[Service]\nExecStart=/usr/local/bin/kdump\n[Install]\nWantedBy=multi-user.target\n"),
        "host-b",
    );
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

    session
        .apply(set_include(config_item(&path), false))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let tree = export_session(&session);
    let freed = containerfile_in(&tree);
    assert!(!staged_at(&tree, &path), "the file is excluded");
    assert!(acts(&freed, "mask", UNIT), "{freed}");
    assert!(omitted(&freed, UNIT).is_none(), "{freed}");
}

/// § 12: two hosts with the same administrator's mount unit merge to one
/// finding counted on both, and the aggregate's Containerfile shows its
/// advisory once, on the scan and the refine route. No instruction names
/// the unit.
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
    for (route, containerfile) in both_routes(&merged) {
        assert_eq!(
            advisory_for(&containerfile, UNIT),
            [
                advisory_fact(UNIT, "enabled"),
                FILE_STAGED_LINE.to_owned(),
                var_symlink_line("/srv"),
                carry_line("enable", UNIT),
            ],
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
/// still has no line warning that the file lacks `nofail`. Control: with
/// the hostnames swapped, the finding is the host without `nofail` and the
/// line is there.
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
    assert!(findings[0].nofail, "host-a's finding: {findings:?}");
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
                FILE_STAGED_LINE.to_owned(),
                var_symlink_line("/srv"),
                carry_line("enable", UNIT),
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
/// while the other host's unit file is included, and not once it is
/// excluded.
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
                .etc_unit(UNIT, "[Service]\nExecStart=/usr/local/bin/kdump\n"),
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
    assert_eq!(declined_in(&refine, UNIT), None);
    assert!(acts(
        &containerfile_in(&export_session(&refine)),
        "mask",
        UNIT
    ));
}
