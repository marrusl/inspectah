mod commands;
mod progress;

use clap::{CommandFactory, Parser, Subcommand};

/// Version string with commit hash and build date, built at compile time.
const LONG_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (commit ",
    env!("INSPECTAH_COMMIT"),
    ", built ",
    env!("INSPECTAH_DATE"),
    ")",
);

/// inspectah — inspect and prepare RHEL systems for image-mode migration.
#[derive(Parser)]
#[command(name = "inspectah", version = LONG_VERSION, about)]
struct Cli {
    /// Print full CLI reference in markdown format
    #[arg(long, hide = true)]
    markdown_help: bool,

    /// Assume yes to all interactive prompts (for CI/automation)
    #[arg(short = 'y', long = "yes", global = true)]
    pub yes: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Scan the current system and produce a migration snapshot
    Scan(commands::scan::ScanArgs),
    /// Interactively refine scan output and re-render
    Refine(commands::refine::RefineArgs),
    /// Combine multiple host scan tarballs into an aggregate snapshot
    Aggregate(commands::aggregate::AggregateArgs),
    /// Build a bootc container image from an inspectah tarball snapshot
    Build(commands::build::BuildArgs),
    /// Print version, commit, and build date
    Version,
    /// Generate shell completions
    #[command(hide = true)]
    Completions {
        /// Shell to generate for
        shell: clap_complete::Shell,
    },
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
        Commands::Scan(args) => match commands::scan::run_scan(&args, cli.yes) {
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
        Commands::Aggregate(args) => {
            if let Err(e) = commands::aggregate::run_aggregate_command(&args) {
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
        Commands::Completions { shell } => {
            clap_complete::generate(
                shell,
                &mut Cli::command(),
                "inspectah",
                &mut std::io::stdout(),
            );
        }
    }
}
