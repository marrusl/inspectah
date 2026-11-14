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
const UNIT_SECTION: &str = "# === Service Enablement ===";

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

/// Whether a `RUN systemctl <verb> --` line in `containerfile` names
/// `unit` as one of its quoted operands.
fn acts(containerfile: &str, verb: &str, unit: &str) -> bool {
    let operand = format!("'{unit}'");
    let mut current = None;
    for line in containerfile.lines() {
        if let Some(rest) = line.strip_prefix("RUN systemctl ") {
            current = rest.split_whitespace().next();
        } else if !line.starts_with("    ") {
            current = None;
        }
        if current == Some(verb) && line.split_whitespace().any(|w| w == operand) {
            return true;
        }
    }
    false
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
    assert!(!acts(&excluded, "enable", "cockpit.socket"), "{excluded}");
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
    assert!(!acts(&excluded, "enable", "local.service"), "{excluded}");
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
    assert!(!acts(&staged, "enable", "orphan.service"), "{staged}");
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
            assert_eq!(
                acts(&containerfile, "enable", "app.socket"),
                file_in && row_in,
                "{label}:\n{containerfile}"
            );
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
        for any_verb in ["enable", "disable"] {
            assert!(
                !acts(&excluded, any_verb, "shadow.service"),
                "{verb}: {any_verb}\n{excluded}"
            );
        }
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
        !acts(&containerfile, "enable", "blob.service"),
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
            !acts(&containerfile, "enable", "app.service"),
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
