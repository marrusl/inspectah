mod commands;
mod progress;

use clap::{CommandFactory, Parser, Subcommand};

/// inspectah — inspect and prepare RHEL systems for image-mode migration.
#[derive(Parser)]
#[command(name = "inspectah", version, about)]
struct Cli {
    /// Print full CLI reference in markdown format
    #[arg(long, hide = true)]
    markdown_help: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Scan the current system and produce a migration snapshot
    Scan(commands::scan::ScanArgs),
    /// Interactively refine scan output and re-render
    Refine(commands::refine::RefineArgs),
    /// Aggregate and manage fleet-wide migration snapshots
    Fleet(commands::fleet::FleetArgs),
    /// Build a bootc container image from an inspectah tarball snapshot
    Build(commands::build::BuildArgs),
    /// Print version, commit, and build date
    Version,
}

fn main() {
    let cli = Cli::parse();

    if cli.markdown_help {
        clap_markdown::print_help_markdown::<Cli>();
        return;
    }

    let Some(command) = cli.command else {
        Cli::command().print_help().ok();
        std::process::exit(2);
    };

    match command {
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
        Commands::Build(args) => match commands::build::run_build(&args) {
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
        Commands::Version => {
            commands::version::print_version();
        }
    }
}
