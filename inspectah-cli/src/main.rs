mod commands;

use clap::{Parser, Subcommand};

/// inspectah — inspect and prepare RHEL systems for image-mode migration.
#[derive(Parser)]
#[command(name = "inspectah", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scan the current system and produce a migration snapshot
    Scan(commands::scan::ScanArgs),
    /// Print version, commit, and build date
    Version,
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Scan(args) => commands::scan::run_scan(&args),
        Commands::Version => {
            commands::version::print_version();
            Ok(())
        }
    };

    if let Err(err) = result {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
