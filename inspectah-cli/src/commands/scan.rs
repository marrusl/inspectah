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
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
use crate::progress::{TerminalProgress, detect_mode, use_color};
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::completeness::Completeness;
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;
use inspectah_pipeline::collect::collect;
use inspectah_pipeline::redaction::engine::{RedactOptions, redact};
use inspectah_pipeline::render;
use inspectah_pipeline::render::baseline_fmt;
use inspectah_pipeline::render::tarball::{create_tarball, get_output_stamp};
use inspectah_pipeline::validate::validate;

use super::pull_progress;

/// Maps snapshot completeness to process exit semantics.
/// Exit codes reflect report trustworthiness, not scan perfection.
pub enum ScanOutcome {
    /// Exit 0 — report is trustworthy.
    Clean,
    /// Exit 0 — report is trustworthy but has caveats.
    Degraded,
    /// Exit 2 — report has blind spots (inspector failed).
    Incomplete,
    /// Exit 130 — user interrupted with SIGINT.
    Interrupted,
}

impl ScanOutcome {
    fn from_completeness(completeness: &Completeness) -> Self {
        match completeness {
            Completeness::Complete => ScanOutcome::Clean,
            Completeness::Partial { .. } => ScanOutcome::Degraded,
            Completeness::Incomplete { .. } => ScanOutcome::Incomplete,
        }
    }

    pub fn exit_code(&self) -> i32 {
        match self {
            ScanOutcome::Clean | ScanOutcome::Degraded => 0,
            ScanOutcome::Incomplete => 2,
            ScanOutcome::Interrupted => 130,
        }
    }
}

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

    /// Progress display mode: rich (default TTY), plain (durable scrollback), flat (non-TTY/CI)
    #[arg(long, value_name = "MODE")]
    pub progress: Option<crate::progress::ProgressMode>,
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

pub fn run_scan(args: &ScanArgs) -> Result<ScanOutcome> {
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

    // Resolve rendering mode early — governs both pull viewport and scan progress.
    // Priority: CLI flag > INSPECTAH_PROGRESS env > TTY auto-detect.
    let mode = detect_mode(args.progress.as_ref());

    // Step 3: Extract baseline
    let baseline_data = match (&normalized_ref, args.no_baseline) {
        (Some(norm), false) => {
            eprintln!("Pulling {}...", norm.as_str());

            let use_viewport = mode == crate::progress::Mode::Rich;
            let mut collected_lines: Vec<String> = Vec::new();

            let data = if use_viewport {
                // TTY: viewport rendering
                let (term_width, term_height) = terminal_size::terminal_size()
                    .map(|(w, h)| (w.0 as usize, h.0 as usize))
                    .unwrap_or((80, 24));

                if term_width >= pull_progress::MIN_VIEWPORT_WIDTH {
                    let content_width = pull_progress::viewport_content_width(term_width);
                    let viewport_lines = pull_progress::viewport_height(term_height);
                    let mut ring: Vec<String> = (0..viewport_lines).map(|_| String::new()).collect();
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
                        pull_progress::viewport_cleanup(viewport_lines);
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
    eprintln!("Inspecting host {hostname}...");

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

    let color = use_color();
    let progress = TerminalProgress::new(mode, color);
    let scan_start = std::time::Instant::now();

    // Install SIGINT handler so Ctrl-C exits cleanly with code 130.
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_hook = cancelled.clone();
    ctrlc::set_handler(move || {
        cancelled_hook.store(true, Ordering::SeqCst);
    })
    .expect("failed to install SIGINT handler");

    let collected = collect(
        &source,
        &executor,
        &inspectors,
        baseline_data.as_ref(),
        &progress,
        &cancelled,
    );

    // SIGINT is a cancellation — no output, no partial counts.
    // Check BEFORE finalize so rich mode doesn't reprint the checklist.
    if cancelled.load(Ordering::SeqCst) {
        progress.cancel();
        eprintln!("Scan cancelled. No report written.");
        return Ok(ScanOutcome::Interrupted);
    }

    progress.finalize();

    // Derive exit outcome from collection completeness
    let outcome = ScanOutcome::from_completeness(&collected.state.snapshot.completeness);

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
        eprintln!(
            "       Preserved credentials: {}",
            snapshot.preserved_credentials
        );
        eprintln!("       Preserved SSH keys: {}", snapshot.preserved_ssh_keys);
        anyhow::bail!("Cannot export sensitive snapshot without --acknowledge-sensitive flag");
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
                match std::fs::write(path, &json) {
                    Ok(()) => {
                        let elapsed = scan_start.elapsed();
                        print_completion(&outcome, elapsed, &snapshot, Some(path.as_path()), true);
                    }
                    Err(e) => {
                        let elapsed = scan_start.elapsed();
                        print_completion(&outcome, elapsed, &snapshot, None, true);
                        eprintln!("Error: failed to write output: {e}");
                        return Err(anyhow::anyhow!("failed to write {}", path.display()).context(e));
                    }
                }
            }
            None => {
                println!("{json}");
                let elapsed = scan_start.elapsed();
                print_completion(&outcome, elapsed, &snapshot, None, true);
            }
        }
        return Ok(outcome);
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

    if let Some(parent) = tarball_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).context("failed to create output directory")?;
    }

    match create_tarball(render_dir.path(), &tarball_path, &stamp) {
        Ok(()) => {
            let elapsed = scan_start.elapsed();
            print_completion(&outcome, elapsed, &snapshot, Some(&tarball_path), false);
            Ok(outcome)
        }
        Err(e) => {
            let elapsed = scan_start.elapsed();
            print_completion(&outcome, elapsed, &snapshot, None, false);
            eprintln!("Error: failed to write report: {e}");
            Err(e).with_context(|| {
                format!("failed to create tarball at {}", tarball_path.display())
            })
        }
    }
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

/// Build a human-readable summary of section item counts.
///
/// Returns a comma-separated string like "847 packages, 12 configs, 4 services, 2 containers".
/// Sections with zero items are omitted. Returns an empty string when all sections are absent
/// or empty (used by the interrupted path to detect whether any inspectors completed).
fn build_summary_counts(snapshot: &InspectionSnapshot) -> String {
    let mut parts = Vec::new();

    if let Some(rpm) = &snapshot.rpm {
        let count = rpm.packages_added.len();
        if count > 0 {
            parts.push(format!("{count} packages"));
        }
    }
    if let Some(config) = &snapshot.config {
        let count = config.files.len();
        if count > 0 {
            parts.push(format!("{count} configs"));
        }
    }
    if let Some(services) = &snapshot.services {
        let count = services.state_changes.len();
        if count > 0 {
            parts.push(format!("{count} services"));
        }
    }
    if let Some(containers) = &snapshot.containers {
        let count = containers.running_containers.len();
        if count > 0 {
            parts.push(format!("{count} containers"));
        }
    }

    parts.join(", ")
}

/// Render the scan completion block to stderr.
///
/// Output varies by `ScanOutcome`:
/// - **Clean / Degraded / Incomplete**: summary counts, optional degraded/failed detail,
///   report path and next-step hint.
/// - **Interrupted**: partial counts (if any inspectors completed), no report written.
fn print_completion(
    outcome: &ScanOutcome,
    elapsed: std::time::Duration,
    snapshot: &InspectionSnapshot,
    output_path: Option<&std::path::Path>,
    inspect_only: bool,
) {
    let secs = elapsed.as_secs_f64();
    let counts = build_summary_counts(snapshot);

    match outcome {
        ScanOutcome::Clean => {
            eprintln!("Scan complete ({secs:.1}s) — {counts}");
        }
        ScanOutcome::Degraded => {
            eprintln!("Scan complete ({secs:.1}s) — {counts}");
            if let Completeness::Partial {
                degraded_sections, ..
            } = &snapshot.completeness
            {
                eprintln!(
                    "  {} degraded (see report for details)",
                    degraded_sections.len()
                );
            }
        }
        ScanOutcome::Incomplete => {
            eprintln!("Scan complete ({secs:.1}s) — {counts}");
            if let Completeness::Incomplete {
                failed_sections,
                degraded_sections,
                ..
            } = &snapshot.completeness
            {
                let mut detail = Vec::new();
                if !failed_sections.is_empty() {
                    detail.push(format!("{} failed", failed_sections.len()));
                }
                if !degraded_sections.is_empty() {
                    detail.push(format!("{} degraded", degraded_sections.len()));
                }
                eprintln!("  {} (see report for details)", detail.join(", "));
            }
        }
        ScanOutcome::Interrupted => {
            // SIGINT path never reaches print_completion — the early
            // return in run_scan() handles it directly. This arm exists
            // only for exhaustive matching.
            return;
        }
    }

    // Report path and next-step hint
    if let Some(path) = output_path {
        if inspect_only {
            eprintln!("Output: {}", path.display());
        } else {
            eprintln!("Report: {}", path.display());
            eprintln!("To review: inspectah refine {}", path.display());
        }
    }
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
    fn test_build_summary_counts_full() {
        use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
        use inspectah_core::types::containers::{ContainerSection, RunningContainer};
        use inspectah_core::types::rpm::{PackageEntry, RpmSection};
        use inspectah_core::types::services::{
            PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState,
        };

        let mut snapshot = InspectionSnapshot::default();

        // 3 packages
        let mut rpm = RpmSection::default();
        rpm.packages_added = vec![
            PackageEntry::default(),
            PackageEntry::default(),
            PackageEntry::default(),
        ];
        snapshot.rpm = Some(rpm);

        // 2 configs
        let mut config = ConfigSection::default();
        config.files = vec![ConfigFileEntry::default(), ConfigFileEntry::default()];
        snapshot.config = Some(config);

        // 4 services
        let svc = || ServiceStateChange {
            unit: "test.service".into(),
            current_state: ServiceUnitState::Enabled,
            default_state: Some(PresetDefault::Enable),
            include: true,
            owning_package: None,
            fleet: None,
            attention_reason: None,
        };
        let mut services = ServiceSection::default();
        services.state_changes = vec![svc(), svc(), svc(), svc()];
        snapshot.services = Some(services);

        // 1 container
        let mut containers = ContainerSection::default();
        containers.running_containers = vec![RunningContainer::default()];
        snapshot.containers = Some(containers);

        assert_eq!(
            build_summary_counts(&snapshot),
            "3 packages, 2 configs, 4 services, 1 containers"
        );
    }

    #[test]
    fn test_build_summary_counts_empty() {
        let snapshot = InspectionSnapshot::default();
        assert_eq!(build_summary_counts(&snapshot), "");
    }

    #[test]
    fn test_build_summary_counts_partial() {
        use inspectah_core::types::rpm::{PackageEntry, RpmSection};

        let mut snapshot = InspectionSnapshot::default();
        let mut rpm = RpmSection::default();
        rpm.packages_added = (0..847).map(|_| PackageEntry::default()).collect();
        snapshot.rpm = Some(rpm);

        assert_eq!(build_summary_counts(&snapshot), "847 packages");
    }

    #[test]
    fn test_build_summary_counts_skips_empty_sections() {
        use inspectah_core::types::config::ConfigSection;
        use inspectah_core::types::rpm::RpmSection;

        let mut snapshot = InspectionSnapshot::default();
        // RPM section present but no packages_added
        snapshot.rpm = Some(RpmSection::default());
        // Config section present but no files
        snapshot.config = Some(ConfigSection::default());

        assert_eq!(build_summary_counts(&snapshot), "");
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
