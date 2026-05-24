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
    /// Interactively refine scan output and re-render
    Refine(commands::refine::RefineArgs),
    /// Aggregate and manage fleet-wide migration snapshots
    Fleet(commands::fleet::FleetArgs),
    /// Print version, commit, and build date
    Version,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Scan(args) => match commands::scan::run_scan(&args) {
            Ok(outcome) => {
                let code = outcome.exit_code();
                if code != 0 {
                    std::process::exit(code);
                }
            }
            Err(e) => {
                eprintln!("error: {e:#}");
                std::process::exit(1);
            }
        },
        Commands::Refine(args) => {
            if let Err(e) = commands::refine::run_refine(&args) {
                eprintln!("error: {e:#}");
                std::process::exit(1);
            }
        }
        Commands::Fleet(args) => {
            if let Err(e) = commands::fleet::run_fleet(&args) {
                eprintln!("error: {e:#}");
                std::process::exit(1);
            }
        }
        Commands::Version => {
            commands::version::print_version();
        }
    }
}
