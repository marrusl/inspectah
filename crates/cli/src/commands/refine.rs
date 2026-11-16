use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use inspectah_refine::session::RefineSession;

#[derive(clap::Args)]
pub struct RefineArgs {
    /// Path to scan output tarball (.tar.gz)
    pub tarball: PathBuf,

    /// Port to bind (default: 8642, use 0 for ephemeral)
    #[arg(long, default_value = "8642")]
    pub port: u16,

    /// Open browser automatically (use --no-open to suppress)
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    pub open: bool,

    /// Start a fresh session, discarding any saved progress
    #[arg(long)]
    pub fresh: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionChoice {
    Resume,
    Fresh,
    Quit,
}

pub(crate) enum ResolveResult {
    Session(Box<RefineSession>),
    Quit,
}

pub(crate) fn resolve_session(
    tarball: &Path,
    fresh: bool,
    choice: Option<SessionChoice>,
    fresh_confirmed: bool,
) -> anyhow::Result<ResolveResult> {
    let session_path = inspectah_refine::autosave::session_file_path(tarball);

    if !fresh && session_path.exists() {
        let choice = choice.unwrap_or(SessionChoice::Quit);
        match choice {
            SessionChoice::Resume => match RefineSession::resume_from(tarball) {
                Ok(Some(s)) => {
                    let ops_count = s.view().stats.ops_applied;
                    eprintln!("Resumed session ({ops_count} ops active).");
                    Ok(ResolveResult::Session(Box::new(s)))
                }
                Err(e) => {
                    eprintln!("Warning: could not resume: {e}");
                    eprintln!("Starting fresh session.");
                    let mut s = inspectah_refine::tarball::from_tarball(tarball)?;
                    s.set_tarball_path(tarball.to_path_buf());
                    Ok(ResolveResult::Session(Box::new(s)))
                }
                Ok(None) => unreachable!("sidecar was just verified to exist"),
            },
            SessionChoice::Fresh => {
                let _ = std::fs::remove_file(&session_path);
                eprintln!("Discarded previous session.");
                let mut s = inspectah_refine::tarball::from_tarball(tarball)?;
                s.set_tarball_path(tarball.to_path_buf());
                Ok(ResolveResult::Session(Box::new(s)))
            }
            SessionChoice::Quit => Ok(ResolveResult::Quit),
        }
    } else if fresh && session_path.exists() {
        if !fresh_confirmed {
            return Ok(ResolveResult::Quit);
        }
        let _ = std::fs::remove_file(&session_path);
        eprintln!("Discarded previous session.");
        let mut s = inspectah_refine::tarball::from_tarball(tarball)?;
        s.set_tarball_path(tarball.to_path_buf());
        Ok(ResolveResult::Session(Box::new(s)))
    } else {
        let mut s = inspectah_refine::tarball::from_tarball(tarball)?;
        s.set_tarball_path(tarball.to_path_buf());
        Ok(ResolveResult::Session(Box::new(s)))
    }
}

fn read_session_choice(tarball: &Path, input: &mut impl BufRead) -> anyhow::Result<SessionChoice> {
    use std::io::Write;

    match inspectah_refine::autosave::load_session(tarball) {
        Ok(Some(state)) => {
            eprintln!("Found saved session:");
            eprintln!(
                "  Operations: {} ({} active, {} redo)",
                state.timeline.len(),
                state.cursor,
                state.timeline.len() - state.cursor
            );
            eprintln!("  Saved at: {}", state.saved_at);
            eprintln!();

            eprint!("[r] Resume  [f] Start fresh  [q] Quit: ");
            std::io::stderr().flush()?;
            let mut answer = String::new();
            input.read_line(&mut answer)?;
            match answer.trim().to_lowercase().as_str() {
                "r" | "resume" => Ok(SessionChoice::Resume),
                "f" | "fresh" => Ok(SessionChoice::Fresh),
                _ => Ok(SessionChoice::Quit),
            }
        }
        Ok(None) => Ok(SessionChoice::Fresh),
        // Never discard saved decisions the operator has not seen: keep the
        // file and say how to drop it.
        Err(e) => anyhow::bail!(
            "could not load the saved session {}: {e}\n\
             The saved session was kept. To discard it and start over, run \
             again with --fresh.",
            inspectah_refine::autosave::session_file_path(tarball).display()
        ),
    }
}

fn read_fresh_confirm(tarball: &Path, input: &mut impl BufRead) -> anyhow::Result<bool> {
    use std::io::Write;

    match inspectah_refine::autosave::load_session(tarball) {
        Ok(Some(state)) => {
            eprint!("Discard {} saved operations? [y/N]: ", state.timeline.len());
            std::io::stderr().flush()?;
            let mut answer = String::new();
            input.read_line(&mut answer)?;
            Ok(answer.trim().to_lowercase() == "y")
        }
        _ => Ok(true),
    }
}

/// Ask the operator about any saved session, reading answers from `input`,
/// and resolve the session to serve.
pub(crate) fn open_session(
    tarball: &Path,
    fresh: bool,
    input: &mut impl BufRead,
) -> anyhow::Result<ResolveResult> {
    let session_path = inspectah_refine::autosave::session_file_path(tarball);

    let choice = if !fresh && session_path.exists() {
        Some(read_session_choice(tarball, input)?)
    } else {
        None
    };

    let fresh_confirmed = if fresh && session_path.exists() {
        read_fresh_confirm(tarball, input)?
    } else {
        false
    };

    resolve_session(tarball, fresh, choice, fresh_confirmed)
}

pub fn run_refine(args: &RefineArgs) -> anyhow::Result<()> {
    eprintln!("Loading snapshot...");

    let session = match open_session(&args.tarball, args.fresh, &mut std::io::stdin().lock())? {
        ResolveResult::Session(s) => *s,
        ResolveResult::Quit => {
            eprintln!("Quit.");
            return Ok(());
        }
    };

    let is_dirty_on_exit = {
        let state = Arc::new(inspectah_web::handlers::AppState {
            session: Arc::new(Mutex::new(session)),
        });

        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async {
            let addr = std::net::SocketAddr::from(([127, 0, 0, 1], args.port));
            let listener = tokio::net::TcpListener::bind(addr).await?;
            let actual_addr = listener.local_addr()?;
            let origin = format!("http://{actual_addr}");

            eprintln!("Starting refine server on {origin}");
            eprintln!("Press Ctrl-C to stop.");

            if args.open {
                let url = origin.clone();
                tokio::task::spawn_blocking(move || {
                    let _ = open::that(&url);
                });
            }

            let app = inspectah_web::router(state.clone(), &origin);

            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal())
                .await?;

            let session = state.session.lock().unwrap();
            Ok::<bool, anyhow::Error>(session.is_dirty())
        })?
    };

    if is_dirty_on_exit {
        eprintln!("Warning: unsaved changes. Use POST /api/tarball to export before stopping.");
    }

    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
    eprintln!("\nShutting down...");
}

#[cfg(test)]
mod tests {
    /// A carried unit from the vendor directory, owned as given.
    fn test_carried_unit(
        name: inspectah_core::types::units::UnitName,
        owning_package: Option<String>,
    ) -> inspectah_core::types::units::CarriedUnit {
        use inspectah_core::types::units::{CarriedUnit, UnitFragment, UnitSubject};
        let fragment = UnitFragment::Vendor {
            file: name.clone(),
            owning_package,
        };
        CarriedUnit::try_from(UnitSubject::new(name, fragment).expect("file is the unit"))
            .expect("a test unit is carried")
    }

    use super::*;
    use inspectah_core::snapshot::InspectionSnapshot;
    use inspectah_core::types::redaction::RedactionState;

    fn redacted_snapshot() -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.redaction_state = Some(RedactionState::FullyRedacted {
            redacted_by: "test".into(),
            config_hash: "test".into(),
        });
        snap
    }

    fn write_test_tarball(path: &Path) {
        write_snapshot_tarball(path, &redacted_snapshot());
    }

    fn write_snapshot_tarball(path: &Path, snap: &InspectionSnapshot) {
        let json = serde_json::to_string(snap).unwrap();
        let file = std::fs::File::create(path).unwrap();
        let gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut tar = tar::Builder::new(gz);
        let data = json.as_bytes();
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, "inspection-snapshot.json", data)
            .unwrap();
        tar.finish().unwrap();
    }

    fn setup_with_session() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let tarball = dir.path().join("test.tar.gz");
        write_test_tarball(&tarball);
        let mut session = inspectah_refine::tarball::from_tarball(&tarball).unwrap();
        session.set_tarball_path(tarball.clone());
        // Force a session file by saving directly
        let state = inspectah_refine::autosave::SessionState {
            schema_version: 3,
            tarball_path: tarball.clone(),
            tarball_hash: inspectah_refine::autosave::compute_tarball_hash(&tarball).unwrap(),
            timeline: vec![],
            cursor: 0,
            saved_at: "2026-05-21T00:00:00Z".into(),
        };
        inspectah_refine::autosave::save_session(&state, &tarball).unwrap();
        (dir, tarball)
    }

    #[test]
    fn resolve_no_session_loads_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let tarball = dir.path().join("test.tar.gz");
        write_test_tarball(&tarball);

        let result = resolve_session(&tarball, false, None, false).unwrap();
        assert!(matches!(result, ResolveResult::Session(_)));
    }

    #[test]
    fn resolve_with_session_resume_loads_session() {
        let (_dir, tarball) = setup_with_session();

        let result = resolve_session(&tarball, false, Some(SessionChoice::Resume), false).unwrap();
        assert!(matches!(result, ResolveResult::Session(_)));
    }

    #[test]
    fn resolve_with_session_fresh_discards_and_loads() {
        let (_dir, tarball) = setup_with_session();
        let session_path = inspectah_refine::autosave::session_file_path(&tarball);
        assert!(session_path.exists(), "sidecar must exist before test");

        let result = resolve_session(&tarball, false, Some(SessionChoice::Fresh), false).unwrap();
        assert!(matches!(result, ResolveResult::Session(_)));
        assert!(
            !session_path.exists(),
            "sidecar must be deleted after fresh"
        );
    }

    #[test]
    fn resolve_with_session_quit_returns_quit() {
        let (_dir, tarball) = setup_with_session();

        let result = resolve_session(&tarball, false, Some(SessionChoice::Quit), false).unwrap();
        assert!(matches!(result, ResolveResult::Quit));
    }

    #[test]
    fn resolve_fresh_flag_confirmed_discards() {
        let (_dir, tarball) = setup_with_session();
        let session_path = inspectah_refine::autosave::session_file_path(&tarball);

        let result = resolve_session(&tarball, true, None, true).unwrap();
        assert!(matches!(result, ResolveResult::Session(_)));
        assert!(!session_path.exists(), "sidecar must be deleted");
    }

    #[test]
    fn resolve_fresh_flag_not_confirmed_quits() {
        let (_dir, tarball) = setup_with_session();
        let session_path = inspectah_refine::autosave::session_file_path(&tarball);

        let result = resolve_session(&tarball, true, None, false).unwrap();
        assert!(matches!(result, ResolveResult::Quit));
        assert!(
            session_path.exists(),
            "sidecar must survive when not confirmed"
        );
    }

    const STATE_ROW_UNIT: &str = "httpd.service";

    /// A tarball whose snapshot has one unit state row, `httpd.service`.
    fn write_units_tarball(path: &Path) {
        use inspectah_core::types::units::{
            PresetDefault, UnitName, UnitSection, UnitState, UnitStateChange,
        };

        let mut snap = redacted_snapshot();
        snap.units = Some(UnitSection {
            state_changes: vec![
                UnitStateChange::new(
                    test_carried_unit(
                        UnitName::try_from(STATE_ROW_UNIT.to_string()).unwrap(),
                        None,
                    ),
                    UnitState::Enabled,
                    Some(PresetDefault::Disable),
                )
                .unwrap(),
            ],
            ..Default::default()
        });
        write_snapshot_tarball(path, &snap);
    }

    /// Write a sidecar for `tarball` holding one exclude of
    /// `STATE_ROW_UNIT`, with its item id spelled with `kind`, so the
    /// operation kind retired with the units rename can be written.
    fn write_sidecar_excluding_unit(tarball: &Path, kind: &str) -> Vec<u8> {
        use inspectah_core::types::units::UnitName;
        use inspectah_refine::types::{ItemId, RefinementOp, TimelineEntry};

        let state = inspectah_refine::autosave::SessionState {
            schema_version: 3,
            tarball_path: tarball.to_path_buf(),
            tarball_hash: inspectah_refine::autosave::compute_tarball_hash(tarball).unwrap(),
            timeline: vec![TimelineEntry::Op(RefinementOp::SetInclude {
                item_id: ItemId::Unit {
                    unit: UnitName::try_from(STATE_ROW_UNIT.to_string()).unwrap(),
                },
                include: false,
            })],
            cursor: 1,
            saved_at: "2026-11-13T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&state)
            .unwrap()
            .replace(r#""kind":"Unit""#, &format!(r#""kind":"{kind}""#));
        assert!(
            json.contains(&format!(r#""kind":"{kind}""#)),
            "the sidecar must hold the `{kind}` kind it was asked to write: {json}"
        );
        let session_path = inspectah_refine::autosave::session_file_path(tarball);
        std::fs::write(&session_path, &json).unwrap();
        json.into_bytes()
    }

    /// A sidecar saved before the units rename holds a `Service` operation,
    /// which no longer loads. The command stops with the load error and
    /// keeps the sidecar untouched instead of discarding it as if the
    /// operator had chosen to start fresh. The same sidecar with the
    /// `Unit` kind is the positive control: it resumes and applies.
    #[test]
    fn open_unloadable_session_stops_and_keeps_it() {
        let dir = tempfile::tempdir().unwrap();
        let tarball = dir.path().join("test.tar.gz");
        write_units_tarball(&tarball);
        let session_path = inspectah_refine::autosave::session_file_path(&tarball);

        let saved = write_sidecar_excluding_unit(&tarball, "Service");
        // An answer is offered so a prompt, had one been shown, could not
        // block or quit; the refusal must come before any prompt.
        let err = match open_session(&tarball, false, &mut std::io::Cursor::new("f\n")) {
            Err(e) => format!("{e:#}"),
            Ok(_) => panic!("a sidecar that cannot be loaded must stop the command"),
        };
        assert!(
            err.contains("`Service`"),
            "the error must carry the load error naming the refused kind: {err}"
        );
        assert!(
            err.contains(&session_path.display().to_string()),
            "the error must say where the kept session is: {err}"
        );
        assert!(
            err.contains("kept") && err.contains("--fresh"),
            "the error must say the session was kept and how to discard it: {err}"
        );
        assert_eq!(
            std::fs::read(&session_path).expect("the sidecar must still exist"),
            saved,
            "the sidecar must be byte-for-byte intact"
        );

        // Positive control: the same operation under the `Unit` kind resumes.
        write_sidecar_excluding_unit(&tarball, "Unit");
        let session = match open_session(&tarball, false, &mut std::io::Cursor::new("r\n")) {
            Ok(ResolveResult::Session(s)) => s,
            Ok(ResolveResult::Quit) => panic!("answering resume must not quit"),
            Err(e) => panic!("a saved `Unit` operation must resume: {e:#}"),
        };
        assert_eq!(session.view().stats.ops_applied, 1);
        let units = session.snapshot_projected().units.unwrap();
        let row = units
            .state_changes
            .iter()
            .find(|s| s.unit().name().as_str() == STATE_ROW_UNIT)
            .expect("the state row is present");
        assert!(
            !row.disposition.is_included(),
            "the resumed exclude must apply to {STATE_ROW_UNIT}"
        );
    }

    /// `--fresh` still discards a sidecar that cannot be loaded.
    #[test]
    fn open_fresh_flag_discards_unloadable_session() {
        let dir = tempfile::tempdir().unwrap();
        let tarball = dir.path().join("test.tar.gz");
        write_units_tarball(&tarball);
        let session_path = inspectah_refine::autosave::session_file_path(&tarball);
        write_sidecar_excluding_unit(&tarball, "Service");

        let result = open_session(&tarball, true, &mut std::io::Cursor::new("")).unwrap();
        assert!(matches!(result, ResolveResult::Session(_)));
        assert!(!session_path.exists(), "--fresh must discard the sidecar");
    }
}
