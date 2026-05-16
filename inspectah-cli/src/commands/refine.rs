use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(clap::Args)]
pub struct RefineArgs {
    /// Path to scan output tarball (.tar.gz)
    pub tarball: PathBuf,

    /// Port to bind (default: 8642, use 0 for ephemeral)
    #[arg(long, default_value = "8642")]
    pub port: u16,

    /// Open browser automatically
    #[arg(long, default_value = "true")]
    pub open: bool,
}

pub fn run_refine(args: &RefineArgs) -> anyhow::Result<()> {
    eprintln!("Loading snapshot...");

    let session = inspectah_refine::tarball::from_tarball(&args.tarball)?;
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
        eprintln!(
            "Warning: unsaved changes. Use POST /api/tarball to export before stopping."
        );
    }

    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
    eprintln!("\nShutting down...");
}
