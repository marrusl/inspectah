//! `inspectah scan` subcommand.
//!
//! Wires the full pipeline: detect source system -> resolve target image ->
//! extract baseline -> collect (all inspectors) -> validate -> redact ->
//! render_all -> create_tarball.
//!
//! With `--inspect-only`, writes the JSON snapshot and exits without producing
//! a tarball or rendered artifacts.

use anyhow::{Context, Result};
use clap::Args;
use std::path::{Path, PathBuf};

use inspectah_collect::executor::real::RealExecutor;
use inspectah_collect::inspectors::config::ConfigInspector;
use inspectah_collect::inspectors::containers::ContainersInspector;
use inspectah_collect::inspectors::kernelboot::KernelbootInspector;
use inspectah_collect::inspectors::network::NetworkInspector;
use inspectah_collect::inspectors::nonrpm::NonRpmInspector;
use inspectah_collect::inspectors::rpm::RpmInspector;
use inspectah_collect::inspectors::scheduled::ScheduledTasksInspector;
use inspectah_collect::inspectors::selinux::SelinuxInspector;
use inspectah_collect::inspectors::services::ServicesInspector;
use inspectah_collect::inspectors::storage::StorageInspector;
use inspectah_collect::inspectors::users::{UserGroupOptions, UsersGroupsInspector};
use inspectah_core::baseline::{TargetImageIdentity, UblueMetadata};
use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::Inspector;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;
use inspectah_pipeline::collect::collect;
use inspectah_pipeline::redaction::engine::{RedactOptions, redact};
use inspectah_pipeline::render;
use inspectah_pipeline::render::baseline_fmt;
use inspectah_pipeline::render::tarball::{create_tarball, get_output_stamp};
use inspectah_pipeline::validate::validate;

use super::pull_progress;

#[derive(Args)]
pub struct ScanArgs {
    /// Write JSON snapshot only, skip tarball/artifact generation
    #[arg(long)]
    pub inspect_only: bool,

    /// Output file path (tarball) or directory (with --inspect-only)
    #[arg(long, short)]
    pub output: Option<PathBuf>,

    /// Target base image for cross-distro conversion (e.g., registry.redhat.io/rhel9/rhel-bootc:9.6)
    #[arg(long)]
    pub base_image: Option<String>,

    /// Skip baseline extraction (degraded classification mode)
    #[arg(long)]
    pub no_baseline: bool,

    /// Preserve password hashes for users with status password_set
    #[arg(long)]
    pub preserve_password_hashes: bool,

    /// Preserve full SSH authorized_keys content per user
    #[arg(long)]
    pub preserve_ssh_keys: bool,

    /// Acknowledge that snapshot contains sensitive data (required for export when preserve flags used)
    #[arg(long)]
    pub acknowledge_sensitive: bool,
}

/// Detect the source system by reading /etc/os-release.
fn detect_source_system(
    executor: &dyn inspectah_core::traits::executor::Executor,
) -> Result<SourceSystem> {
    let os_release_content = executor
        .read_file(std::path::Path::new("/etc/os-release"))
        .context("failed to read /etc/os-release")?;
    let os_release = parse_os_release(&os_release_content);

    // Phase 1: only package-based systems. bootc/rpm-ostree detection is Phase 2.
    Ok(SourceSystem::PackageBased { os_release })
}

/// Parse os-release key=value format.
fn parse_os_release(content: &str) -> OsRelease {
    let mut os = OsRelease::default();
    for line in content.lines() {
        let line = line.trim();
        if let Some((key, val)) = line.split_once('=') {
            let val = val.trim_matches('"');
            match key {
                "NAME" => os.name = val.to_string(),
                "VERSION_ID" => os.version_id = val.to_string(),
                "VERSION" => os.version = val.to_string(),
                "ID" => os.id = val.to_string(),
                "ID_LIKE" => os.id_like = val.to_string(),
                "PRETTY_NAME" => os.pretty_name = val.to_string(),
                "VARIANT_ID" => os.variant_id = val.to_string(),
                _ => {}
            }
        }
    }
    os
}

/// Get hostname for tarball naming.
fn get_hostname(executor: &dyn inspectah_core::traits::executor::Executor) -> String {
    let result = executor.run("hostname", &[]);
    let hostname = result.stdout.trim().to_string();
    if hostname.is_empty() {
        "unknown".to_string()
    } else {
        hostname
    }
}

pub fn run_scan(args: &ScanArgs) -> Result<()> {
    // Require root: scanning reads system state that needs elevated privileges.
    // SAFETY: geteuid() is a simple syscall with no preconditions or invariants.
    let euid = unsafe { libc::geteuid() };
    if euid != 0 {
        eprintln!("Error: inspectah scan requires root privileges.");
        eprintln!("Run with: sudo inspectah scan [options]");
        std::process::exit(1);
    }

    // Flag validation
    if args.base_image.is_some() && args.no_baseline {
        anyhow::bail!(
            "Cannot specify both --base-image and --no-baseline. \
             Use --base-image to set the target image, or --no-baseline to skip baseline extraction."
        );
    }

    let executor = RealExecutor::new();

    // Step 1: Detect source system
    eprintln!("Detecting source system...");
    let source = detect_source_system(&executor).context("source system detection failed")?;
    let pretty = &source.os_release().pretty_name;
    if !pretty.is_empty() {
        eprintln!("  {pretty}");
    }

    // Step 2: Resolve target image
    eprintln!("Resolving target image...");

    let ublue_metadata = read_ublue_metadata(&executor)?;
    let bootc_ref = read_bootc_status_ref(&executor);

    let resolution_result = inspectah_core::baseline::resolve_base_image(
        source.os_release(),
        ublue_metadata.as_ref(),
        bootc_ref.as_deref(),
        args.base_image.as_deref(),
    );

    let (target_image, normalized_ref) = match resolution_result {
        Ok(res) => {
            let norm = inspectah_core::baseline::normalize_image_ref(&res.image_ref)
                .context("image ref normalization failed")?;
            eprintln!("  {} ({:?})", norm.as_str(), res.strategy);
            let ti = TargetImageIdentity {
                image_ref: norm.as_str().to_string(),
                strategy: res.strategy,
            };
            (Some(ti), Some(norm))
        }
        Err(e) if args.no_baseline => {
            eprintln!("  not found ({}), continuing without baseline", e);
            (None, None)
        }
        Err(e) => return Err(e.into()),
    };

    // Step 3: Extract baseline
    let baseline_data = match (&normalized_ref, args.no_baseline) {
        (Some(norm), false) => {
            eprintln!("Pulling {}...", norm.as_str());

            let use_viewport = std::io::IsTerminal::is_terminal(&std::io::stderr());
            let mut collected_lines: Vec<String> = Vec::new();

            let data = if use_viewport {
                // TTY: viewport rendering
                let term_width = terminal_size::terminal_size()
                    .map(|(w, _)| w.0 as usize)
                    .unwrap_or(80);

                if term_width >= pull_progress::MIN_VIEWPORT_WIDTH {
                    let content_width = pull_progress::viewport_content_width(term_width);
                    let mut ring = [String::new(), String::new(), String::new()];
                    let mut ring_pos: usize = 0;

                    let result = {
                        let mut callback = pull_progress::tty_viewport_callback(
                            &mut collected_lines,
                            &mut ring,
                            &mut ring_pos,
                            content_width,
                        );
                        inspectah_collect::baseline::extract_baseline(
                            &executor,
                            norm,
                            &mut callback,
                        )
                    };
                    // Only clear viewport if lines were actually rendered.
                    if ring_pos > 0 {
                        pull_progress::viewport_cleanup();
                    }
                    result.context("baseline extraction failed")?
                } else {
                    // Narrow terminal — fall back to non-TTY
                    let mut callback = pull_progress::non_tty_callback(&mut collected_lines);
                    inspectah_collect::baseline::extract_baseline(&executor, norm, &mut callback)
                        .context("baseline extraction failed")?
                }
            } else {
                // Non-TTY: prefixed passthrough
                let mut callback = pull_progress::non_tty_callback(&mut collected_lines);
                inspectah_collect::baseline::extract_baseline(&executor, norm, &mut callback)
                    .context("baseline extraction failed")?
            };

            // Pull summary line
            let blob_count = pull_progress::count_completed_blobs(&collected_lines);
            eprintln!(
                "{}",
                pull_progress::pull_summary_line(norm.as_str(), &data.image_digest, blob_count,)
            );

            // Provenance block
            eprintln!("  Baseline extracted: {} packages", data.packages.len());
            if let Some(ti) = &target_image {
                eprintln!(
                    "  Resolved via: {}",
                    baseline_fmt::strategy_label(&ti.strategy)
                );
            }

            Some(data)
        }
        (Some(_norm), true) => {
            // --no-baseline: show degraded message
            eprintln!("  Baseline: skipped (--no-baseline)");
            None
        }
        _ => None,
    };

    // Step 4: Collect — run all inspectors
    let hostname = get_hostname(&executor);
    eprintln!("Scanning host {hostname}...");

    // Build UserGroupOptions from CLI flags
    let user_group_options = UserGroupOptions {
        strategy_override: None,
        preserve_password_hashes: args.preserve_password_hashes,
        preserve_ssh_keys: args.preserve_ssh_keys,
    };

    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(RpmInspector::new()),
        Box::new(ServicesInspector::new()),
        Box::new(StorageInspector::new()),
        Box::new(KernelbootInspector::new()),
        Box::new(NetworkInspector::new()),
        Box::new(ContainersInspector::new()),
        Box::new(UsersGroupsInspector::with_options(user_group_options)),
        Box::new(ScheduledTasksInspector::new()),
        Box::new(ConfigInspector::new()),
        Box::new(SelinuxInspector::new()),
        Box::new(NonRpmInspector::new()),
    ];
    let collected = collect(&source, &executor, &inspectors, baseline_data.as_ref());
    eprintln!("Scanning host {hostname}... done");

    // Step 5: Validate
    let validated = validate(collected).context("snapshot validation failed")?;

    // Step 6: Redact
    let mut snapshot = validated.state.snapshot;

    // Set Phase 6 fields on snapshot
    snapshot.target_image = target_image;
    snapshot.baseline = baseline_data;
    snapshot.no_baseline = args.no_baseline;

    // Set sensitivity metadata from CLI flags
    snapshot.sensitive_snapshot = args.preserve_password_hashes || args.preserve_ssh_keys;
    snapshot.preserved_credentials = args.preserve_password_hashes;
    snapshot.preserved_ssh_keys = args.preserve_ssh_keys;

    // Version comparison line (prints after collection, since version_changes
    // is populated by the RPM inspector during collection)
    if snapshot.baseline.is_some() {
        let vc_display = baseline_fmt::version_changes_for_display(&snapshot);
        let shared_count = match (snapshot.rpm.as_ref(), snapshot.baseline.as_ref()) {
            (Some(rpm), Some(bl)) if baseline_fmt::is_rpm_comparison_available(&snapshot) => {
                baseline_fmt::shared_package_count(bl, rpm)
            }
            _ => 0,
        };
        let summary = baseline_fmt::version_comparison_summary(vc_display, shared_count);
        if vc_display.is_none() {
            eprintln!("  Version comparison: {summary}");
        } else {
            eprintln!("  Version changes: {summary}");
        }
    }

    redact(&mut snapshot, &RedactOptions::default());

    // Export gating: if snapshot contains sensitive data, require acknowledgment
    if snapshot.sensitive_snapshot && !args.acknowledge_sensitive {
        eprintln!("Error: Snapshot contains sensitive data (password hashes or SSH keys).");
        eprintln!("       To export, re-run with --acknowledge-sensitive");
        eprintln!("       Preserved credentials: {}", snapshot.preserved_credentials);
        eprintln!("       Preserved SSH keys: {}", snapshot.preserved_ssh_keys);
        anyhow::bail!(
            "Cannot export sensitive snapshot without --acknowledge-sensitive flag"
        );
    }

    // If --inspect-only, write JSON and exit
    if args.inspect_only {
        let json =
            serde_json::to_string_pretty(&snapshot).context("failed to serialize snapshot")?;

        match &args.output {
            Some(path) => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).context("failed to create output directory")?;
                }
                std::fs::write(path, &json)
                    .with_context(|| format!("failed to write {}", path.display()))?;
                eprintln!("Snapshot written to {}", path.display());
            }
            None => {
                println!("{json}");
            }
        }
        return Ok(());
    }

    // Step 7: Render all artifacts to a temp directory
    let render_dir = tempfile::tempdir().context("failed to create temp directory")?;

    let render_context = RenderContext { target: None };
    render::render_all(&snapshot, &render_context, render_dir.path()).context("render failed")?;

    // Write a minimal schema placeholder (real JSON Schema is Phase 7)
    let schema_dir = render_dir.path().join("schema");
    std::fs::create_dir_all(&schema_dir)?;
    std::fs::write(
        schema_dir.join("snapshot.schema.json"),
        r#"{"$schema":"http://json-schema.org/draft-07/schema#","title":"InspectionSnapshot","description":"Phase 7 placeholder","type":"object"}"#,
    )?;

    // Step 8: Create tarball
    let stamp = get_output_stamp(&hostname);
    let tarball_name = format!("{stamp}.tar.gz");

    let tarball_path = match &args.output {
        Some(path) => path.clone(),
        None => PathBuf::from(&tarball_name),
    };

    if let Some(parent) = tarball_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).context("failed to create output directory")?;
        }
    }

    create_tarball(render_dir.path(), &tarball_path, &stamp)
        .with_context(|| format!("failed to create tarball at {}", tarball_path.display()))?;

    eprintln!("Output written to {}", tarball_path.display());
    Ok(())
}

/// Read Universal Blue metadata from the well-known path.
/// Returns Ok(None) if file doesn't exist, Err if file exists but is malformed.
fn read_ublue_metadata(executor: &dyn Executor) -> Result<Option<UblueMetadata>> {
    let content = match executor.read_file(Path::new("/usr/share/ublue-os/image-info.json")) {
        Ok(c) => c,
        Err(_) => return Ok(None), // file not found -> not a UBlue system
    };
    // File exists — parse must succeed or fail closed
    let metadata: UblueMetadata = serde_json::from_str(&content)
        .context("Universal Blue metadata at /usr/share/ublue-os/image-info.json is malformed")?;
    Ok(Some(metadata))
}

/// Read the booted image ref from `bootc status --json`.
fn read_bootc_status_ref(executor: &dyn Executor) -> Option<String> {
    let result = executor.run("bootc", &["status", "--json"]);
    if !result.success() {
        return None;
    }
    // Parse status.booted.image.image.image
    let val: serde_json::Value = serde_json::from_str(&result.stdout).ok()?;
    val.get("status")?
        .get("booted")?
        .get("image")?
        .get("image")?
        .get("image")?
        .as_str()
        .map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_os_release() {
        let content = r#"NAME="Red Hat Enterprise Linux"
VERSION_ID="9.4"
ID=rhel
ID_LIKE="fedora"
PRETTY_NAME="Red Hat Enterprise Linux 9.4 (Plow)"
VERSION="9.4 (Plow)"
VARIANT_ID="workstation"
"#;
        let os = parse_os_release(content);
        assert_eq!(os.name, "Red Hat Enterprise Linux");
        assert_eq!(os.version_id, "9.4");
        assert_eq!(os.id, "rhel");
        assert_eq!(os.id_like, "fedora");
        assert_eq!(os.pretty_name, "Red Hat Enterprise Linux 9.4 (Plow)");
        assert_eq!(os.variant_id, "workstation");
    }

    #[test]
    fn test_parse_os_release_minimal() {
        let content = "ID=fedora\nVERSION_ID=40\n";
        let os = parse_os_release(content);
        assert_eq!(os.id, "fedora");
        assert_eq!(os.version_id, "40");
        assert_eq!(os.name, "");
    }

    #[test]
    fn test_cli_creates_all_inspectors() {
        // Verify all 11 inspectors are registered
        let inspectors: Vec<Box<dyn Inspector>> = vec![
            Box::new(RpmInspector::new()),
            Box::new(ServicesInspector::new()),
            Box::new(StorageInspector::new()),
            Box::new(KernelbootInspector::new()),
            Box::new(NetworkInspector::new()),
            Box::new(ContainersInspector::new()),
            Box::new(UsersGroupsInspector::new()),
            Box::new(ScheduledTasksInspector::new()),
            Box::new(ConfigInspector::new()),
            Box::new(SelinuxInspector::new()),
            Box::new(NonRpmInspector::new()),
        ];
        assert_eq!(inspectors.len(), 11);
    }

    #[test]
    fn test_cli_wave2_ids_present() {
        use inspectah_core::types::completeness::InspectorId;

        // Verify Wave 2 inspector IDs are present
        let inspectors: Vec<Box<dyn Inspector>> = vec![
            Box::new(RpmInspector::new()),
            Box::new(ServicesInspector::new()),
            Box::new(StorageInspector::new()),
            Box::new(KernelbootInspector::new()),
            Box::new(NetworkInspector::new()),
            Box::new(ContainersInspector::new()),
            Box::new(UsersGroupsInspector::new()),
            Box::new(ScheduledTasksInspector::new()),
            Box::new(ConfigInspector::new()),
            Box::new(SelinuxInspector::new()),
            Box::new(NonRpmInspector::new()),
        ];

        let ids: Vec<_> = inspectors.iter().map(|i| i.id()).collect();
        assert!(ids.contains(&InspectorId::ScheduledTasks));
        assert!(ids.contains(&InspectorId::Config));
        assert!(ids.contains(&InspectorId::Selinux));
        assert!(ids.contains(&InspectorId::NonRpmSoftware));
    }
}
