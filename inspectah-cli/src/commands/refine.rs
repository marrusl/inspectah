use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

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

pub fn run_refine(args: &RefineArgs) -> anyhow::Result<()> {
    eprintln!("Loading snapshot...");

    let session = if args.fresh {
        // Delete any existing session file when --fresh is passed
        let session_path = inspectah_refine::autosave::session_file_path(&args.tarball);
        if session_path.exists() {
            let _ = std::fs::remove_file(&session_path);
            eprintln!("Discarded previous session.");
        }
        let mut s = inspectah_refine::tarball::from_tarball(&args.tarball)?;
        s.set_tarball_path(args.tarball.clone());
        s
    } else {
        // Try to resume from a saved session
        match inspectah_refine::session::RefineSession::resume_from(&args.tarball) {
            Ok(Some(session)) => {
                let ops_count = session.view().stats.ops_applied;
                eprintln!("Resumed session with {ops_count} operations.");
                session
            }
            Ok(None) => {
                // No saved session — start fresh
                let mut s = inspectah_refine::tarball::from_tarball(&args.tarball)?;
                s.set_tarball_path(args.tarball.clone());
                s
            }
            Err(e) => {
                eprintln!("Warning: could not resume session ({e}), starting fresh.");
                let mut s = inspectah_refine::tarball::from_tarball(&args.tarball)?;
                s.set_tarball_path(args.tarball.clone());
                s
            }
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
