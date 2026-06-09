use anyhow::Result;
use clap::Parser;
use inspectah_pipeline::build::{BuildConfig, BuildOutcome, BuildWarning, plan_and_execute};
use std::path::PathBuf;

/// Build a bootc container image from an inspectah tarball snapshot
#[derive(Parser)]
pub struct BuildArgs {
    /// Path to inspectah tarball (.tar.gz snapshot)
    tarball: PathBuf,

    /// Image tag (must include version, e.g., 'myimage:v1')
    #[arg(short, long)]
    tag: String,

    /// Show the build command without executing it
    #[arg(long)]
    dry_run: bool,

    /// Keep the extracted build context after build completes
    #[arg(long)]
    keep_context: bool,

    /// Additional arguments to pass to podman build (after --)
    #[arg(last = true)]
    podman_args: Vec<String>,
}

pub fn run_build(args: &BuildArgs) -> Result<BuildOutcome> {
    // Construct build configuration from CLI args.
    let config = BuildConfig {
        tarball: args.tarball.clone(),
        tag: args.tag.clone(),
        dry_run: args.dry_run,
        keep_context: args.keep_context,
        podman_args: args.podman_args.clone(),
    };

    // Delegate to pipeline module.
    let (outcome, warnings) = plan_and_execute(&config)?;

    // Render warnings to stderr.
    for warning in warnings {
        match warning {
            BuildWarning::CertExpiringSoon {
                days_remaining,
                path,
            } => {
                eprintln!("warning: entitlement cert expires in {days_remaining} days: {path}");
            }
            BuildWarning::CertExpired { path } => {
                eprintln!("warning: entitlement cert has expired: {path}");
            }
            BuildWarning::AmbientBundleIncomplete { reason } => {
                eprintln!("warning: ambient subscription bundle incomplete: {reason}");
            }
            BuildWarning::NoSubscriptionData => {
                eprintln!(
                    "warning: no subscription data available (not RHEL or no bundle in tarball)"
                );
            }
        }
    }

    // Render outcome to stdout/stderr.
    match &outcome {
        BuildOutcome::Success { tag, digest } => {
            println!("✓ Build succeeded: {tag}");
            if let Some(d) = digest {
                println!("  Digest: {d}");
            }
        }
        BuildOutcome::DryRun { command } => {
            println!("Dry run — would execute:\n{command}");
        }
        BuildOutcome::PodmanNotFound => {
            eprintln!("error: podman not found in PATH");
            eprintln!("Install podman to build container images.");
        }
        BuildOutcome::PodmanFailed { exit_code } => {
            eprintln!("error: podman build failed with exit code {exit_code}");
        }
        BuildOutcome::NoSubscription => {
            eprintln!("error: no subscription data available");
            eprintln!("This build requires RHEL subscription data.");
            eprintln!("Re-scan the source host with --preserve subscription --ack-sensitive.");
        }
        BuildOutcome::PreflightFailed { reason } => {
            eprintln!("error: preflight check failed: {reason}");
        }
    }

    Ok(outcome)
}
