use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

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
    Session(RefineSession),
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
            SessionChoice::Resume => {
                match RefineSession::resume_from(tarball) {
                    Ok(Some(s)) => {
                        let ops_count = s.view().stats.ops_applied;
                        eprintln!("Resumed session ({ops_count} ops active).");
                        Ok(ResolveResult::Session(s))
                    }
                    Err(e) => {
                        eprintln!("Warning: could not resume: {e}");
                        eprintln!("Starting fresh session.");
                        let mut s = inspectah_refine::tarball::from_tarball(tarball)?;
                        s.set_tarball_path(tarball.to_path_buf());
                        Ok(ResolveResult::Session(s))
                    }
                    Ok(None) => unreachable!("sidecar was just verified to exist"),
                }
            }
            SessionChoice::Fresh => {
                let _ = std::fs::remove_file(&session_path);
                eprintln!("Discarded previous session.");
                let mut s = inspectah_refine::tarball::from_tarball(tarball)?;
                s.set_tarball_path(tarball.to_path_buf());
                Ok(ResolveResult::Session(s))
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
        Ok(ResolveResult::Session(s))
    } else {
        let mut s = inspectah_refine::tarball::from_tarball(tarball)?;
        s.set_tarball_path(tarball.to_path_buf());
        Ok(ResolveResult::Session(s))
    }
}

fn read_session_choice(tarball: &Path) -> anyhow::Result<SessionChoice> {
    use std::io::Write;

    match inspectah_refine::autosave::load_session(tarball) {
        Ok(Some(state)) => {
            eprintln!("Found saved session:");
            eprintln!(
                "  Operations: {} ({} active, {} redo)",
                state.ops.len(),
                state.cursor,
                state.ops.len() - state.cursor
            );
            eprintln!("  Saved at: {}", state.saved_at);
            eprintln!();

            eprint!("[r] Resume  [f] Start fresh  [q] Quit: ");
            std::io::stderr().flush()?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            match input.trim().to_lowercase().as_str() {
                "r" | "resume" => Ok(SessionChoice::Resume),
                "f" | "fresh" => Ok(SessionChoice::Fresh),
                _ => Ok(SessionChoice::Quit),
            }
        }
        _ => Ok(SessionChoice::Fresh),
    }
}

fn read_fresh_confirm(tarball: &Path) -> anyhow::Result<bool> {
    use std::io::Write;

    match inspectah_refine::autosave::load_session(tarball) {
        Ok(Some(state)) => {
            eprint!("Discard {} saved operations? [y/N]: ", state.ops.len());
            std::io::stderr().flush()?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            Ok(input.trim().to_lowercase() == "y")
        }
        _ => Ok(true),
    }
}

pub fn run_refine(args: &RefineArgs) -> anyhow::Result<()> {
    eprintln!("Loading snapshot...");

    let session_path = inspectah_refine::autosave::session_file_path(&args.tarball);

    let choice = if !args.fresh && session_path.exists() {
        Some(read_session_choice(&args.tarball)?)
    } else {
        None
    };

    let fresh_confirmed = if args.fresh && session_path.exists() {
        read_fresh_confirm(&args.tarball)?
    } else {
        false
    };

    let session = match resolve_session(&args.tarball, args.fresh, choice, fresh_confirmed)? {
        ResolveResult::Session(s) => s,
        ResolveResult::Quit => {
            eprintln!("Quit.");
            return Ok(());
        }
    };
    let is_dirty_on_exit = {
        let state = Arc::new(inspectah_web::handlers::AppState {
            session: Arc::new(Mutex::new(session)),
            sections_cache: OnceLock::new(),
        });

        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async {
            let addr = std::net::SocketAddr::from(([127, 0, 0, 1], args.port));
            let listener = tokio::net::TcpListener::bind(addr).await?;
            let actual_addr = listener.local_addr()?;
            let origin = format!("http://{actual_addr}");

            eprintln!("Starting refine server on {origin}");
            eprintln!(
                "If accessing remotely: ssh -L {0}:localhost:{0} <host>",
                actual_addr.port()
            );
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
    use super::*;
    use inspectah_core::snapshot::InspectionSnapshot;
    use inspectah_core::types::redaction::RedactionState;

    fn write_test_tarball(path: &Path) {
        let mut snap = InspectionSnapshot::new();
        snap.redaction_state = Some(RedactionState::FullyRedacted {
            redacted_by: "test".into(),
            config_hash: "test".into(),
        });
        let json = serde_json::to_string(&snap).unwrap();
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
        let op = inspectah_refine::types::RefinementOp::ExcludeConfig {
            path: "/nonexistent".into(),
        };
        // Force a session file by saving directly
        let state = inspectah_refine::autosave::SessionState {
            schema_version: 1,
            tarball_path: tarball.clone(),
            tarball_hash: inspectah_refine::autosave::compute_tarball_hash(&tarball).unwrap(),
            ops: vec![],
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

        let result =
            resolve_session(&tarball, false, Some(SessionChoice::Resume), false).unwrap();
        assert!(matches!(result, ResolveResult::Session(_)));
    }

    #[test]
    fn resolve_with_session_fresh_discards_and_loads() {
        let (_dir, tarball) = setup_with_session();
        let session_path = inspectah_refine::autosave::session_file_path(&tarball);
        assert!(session_path.exists(), "sidecar must exist before test");

        let result =
            resolve_session(&tarball, false, Some(SessionChoice::Fresh), false).unwrap();
        assert!(matches!(result, ResolveResult::Session(_)));
        assert!(!session_path.exists(), "sidecar must be deleted after fresh");
    }

    #[test]
    fn resolve_with_session_quit_returns_quit() {
        let (_dir, tarball) = setup_with_session();

        let result =
            resolve_session(&tarball, false, Some(SessionChoice::Quit), false).unwrap();
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
        assert!(session_path.exists(), "sidecar must survive when not confirmed");
    }
}
