// Config advisories must survive a resumed session.
//
// `resume_from` restores an autosaved timeline without re-validating each op,
// on the grounds that the ops were validated when they were first applied. A
// session autosaved before `AdvisoryNotToggleable` shipped was validated by a
// build that had no such guard, so its timeline can carry a `SetInclude` on an
// advisory config path. `project_snapshot` applies that op with
// `FindingKind::from_bool`, and the view's config entries are classified from
// the projection — so the finding reaches the web contract as an ordinary
// actionable file, with a live toggle, and the modernization rationale gone.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::redaction::RedactionState;
use inspectah_core::types::{AdvisoryType, FindingKind};
use inspectah_refine::autosave::{SessionState, compute_tarball_hash, save_session};
use inspectah_refine::session::{RefineSession, should_include_in_containerfile};
use inspectah_refine::types::{ItemId, RefinementOp, TimelineEntry};
use inspectah_web::adapter::build_web_view;

const ADVISORY_PATH: &str = "/etc/init.d/legacy-app";
const RATIONALE: &str = "sysvinit script — port to a systemd unit";
const AUTOSAVE_SCHEMA_VERSION: u32 = 3;

fn advisory_snapshot() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: ADVISORY_PATH.into(),
            kind: ConfigFileKind::Unowned,
            disposition: FindingKind::advisory(AdvisoryType::Modernization, RATIONALE),
            ..Default::default()
        }],
    });
    // from_tarball() rejects snapshots without provenance.
    snap.redaction_state = Some(RedactionState::FullyRedacted {
        redacted_by: "inspectah-test".into(),
        config_hash: "abc".into(),
    });
    snap
}

fn write_tarball(path: &std::path::Path, snap: &InspectionSnapshot) {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    let json = serde_json::to_string_pretty(snap).unwrap();
    let gz = GzEncoder::new(std::fs::File::create(path).unwrap(), Compression::default());
    let mut tar = tar::Builder::new(gz);
    let bytes = json.as_bytes();
    let mut header = tar::Header::new_gnu();
    header.set_path("inspection-snapshot.json").unwrap();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append(&header, bytes).unwrap();
    tar.finish().unwrap();
}

/// Resume a session whose autosaved timeline carries the stale toggle a
/// pre-guard build would have recorded.
fn resume_with_stale_advisory_toggle(dir: &std::path::Path) -> RefineSession {
    let tarball = dir.join("scan.tar.gz");
    write_tarball(&tarball, &advisory_snapshot());

    let stale = TimelineEntry::Op(RefinementOp::SetInclude {
        item_id: ItemId::Config {
            path: ADVISORY_PATH.into(),
        },
        include: true,
    });
    save_session(
        &SessionState {
            schema_version: AUTOSAVE_SCHEMA_VERSION,
            tarball_path: tarball.clone(),
            tarball_hash: compute_tarball_hash(&tarball).unwrap(),
            timeline: vec![stale],
            cursor: 1,
            saved_at: "2026-08-01T00:00:00Z".into(),
        },
        &tarball,
    )
    .unwrap();

    RefineSession::resume_from(&tarball)
        .expect("resume succeeds")
        .expect("a session file exists")
}

/// The projection is what the export renders from, so a replayed stale toggle
/// must not reach it as an include decision either. The web view restores
/// advisories from the original snapshot, which protects the *display* — this
/// covers the other consumer, where the consequence is the flagged file being
/// copied into the image rather than merely mislabelled in the browser.
#[test]
fn resumed_stale_toggle_does_not_reach_the_export_projection() {
    let dir = tempfile::tempdir().unwrap();
    let session = resume_with_stale_advisory_toggle(dir.path());

    let projected = session.snapshot_projected();
    let entry = &projected
        .config
        .as_ref()
        .expect("config section survives projection")
        .files[0];

    assert!(
        entry.disposition.is_advisory(),
        "a replayed stale toggle must not rewrite an advisory as actionable in \
         the projection: {:?}",
        entry.disposition
    );
    assert!(
        !should_include_in_containerfile(&entry.disposition),
        "the export must not copy a modernization-flagged file into the image \
         on the strength of a stale toggle: {:?}",
        entry.disposition
    );
}

#[test]
fn resumed_stale_toggle_does_not_downgrade_a_config_advisory() {
    let dir = tempfile::tempdir().unwrap();
    let session = resume_with_stale_advisory_toggle(dir.path());

    let json = serde_json::to_value(build_web_view(&session)).expect("serialize");
    let entry = json["config_files"]
        .as_array()
        .expect("config_files array")
        .iter()
        .find(|f| f["entry"]["path"] == ADVISORY_PATH)
        .expect("the advisory entry reaches the client");

    assert_eq!(
        entry["entry"]["disposition"]["kind"], "advisory",
        "a replayed stale toggle must not turn a modernization finding into a \
         file the user chose to include: {}",
        entry["entry"]["disposition"]
    );
    assert_eq!(
        entry["entry"]["disposition"]["advisory_type"],
        "modernization"
    );
    assert_eq!(entry["entry"]["disposition"]["rationale"], RATIONALE);
}
