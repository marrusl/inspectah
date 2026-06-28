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

use crate::progress::receipt::{ScanEndState, ScanFinalize, VersionChangeSummary};
use crate::progress::{TerminalProgress, detect_mode, use_color};
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
use inspectah_collect::inspectors::subscription::SubscriptionInspector;
use inspectah_collect::inspectors::users::{UserGroupOptions, UsersGroupsInspector};
use inspectah_core::baseline::{TargetImageIdentity, UblueMetadata};
use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::Inspector;
use inspectah_core::traits::progress::ProgressSink;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::completeness::Completeness;
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::redaction::RedactionState;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum PreserveItem {
    #[value(name = "password-hashes")]
    PasswordHashes,
    #[value(name = "ssh-keys")]
    SshKeys,
    #[value(name = "subscription")]
    Subscription,
    #[value(name = "all")]
    All,
}

impl PreserveItem {
    /// Expand `All` into concrete variants. `All` itself is consumed — it never
    /// appears in the returned vec.
    pub fn expand(items: &[PreserveItem]) -> Vec<PreserveItem> {
        let mut result = Vec::new();
        let has_all = items.iter().any(|i| matches!(i, PreserveItem::All));
        if has_all {
            result.push(PreserveItem::PasswordHashes);
            result.push(PreserveItem::SshKeys);
            result.push(PreserveItem::Subscription);
        } else {
            for item in items {
                if !result.contains(item) {
                    result.push(*item);
                }
            }
        }
        result
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

    /// Preserve sensitive data in the snapshot
    #[arg(long, value_delimiter = ',', value_name = "ITEM")]
    pub preserve: Vec<PreserveItem>,

    /// Skip the redaction phase — secrets remain unmasked in output
    #[arg(long)]
    pub no_redaction: bool,

    /// Acknowledge sensitive data in the snapshot (required with --preserve or --no-redaction)
    #[arg(long = "ack-sensitive", visible_alias = "acknowledge-sensitive")]
    pub ack_sensitive: bool,

    /// Progress display mode: pretty (default TTY), flat (non-TTY/CI)
    #[arg(long, value_name = "MODE")]
    pub progress: Option<crate::progress::ProgressMode>,

    /// Show sub-step detail for all inspectors, including fast ones
    #[arg(long, short, conflicts_with = "quiet")]
    pub verbose: bool,

    /// Suppress the scan progress checklist (completion summary still prints)
    #[arg(long, short, conflicts_with = "verbose")]
    pub quiet: bool,

    /// Catalog and bundle unmanaged files from /opt, /srv, /usr/local.
    /// Prompts with total size before bundling (suppressed by -y/--yes).
    #[arg(long)]
    pub include_unmanaged: bool,

    /// Exclude specific paths from unmanaged file collection (repeatable)
    #[arg(long = "exclude-path", value_name = "PATH")]
    pub exclude_path: Vec<String>,
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

fn validate_sensitivity_flags(args: &ScanArgs) -> Result<()> {
    let has_preserve = !args.preserve.is_empty();
    let has_no_redaction = args.no_redaction;

    if (has_preserve || has_no_redaction) && !args.ack_sensitive {
        let msg = match (has_preserve, has_no_redaction) {
            (true, true) => {
                "--preserve and --no-redaction require --ack-sensitive to acknowledge sensitive data in the snapshot"
            }
            (true, false) => {
                "--preserve requires --ack-sensitive to acknowledge sensitive data in the snapshot"
            }
            (false, true) => {
                "--no-redaction requires --ack-sensitive to acknowledge unredacted secrets in the snapshot"
            }
            (false, false) => unreachable!(),
        };
        anyhow::bail!(msg);
    }
    Ok(())
}

pub fn run_scan(args: &ScanArgs, assume_yes: bool) -> Result<ScanOutcome> {
    // Require root: scanning reads system state that needs elevated privileges.
    // SAFETY: geteuid() is a simple syscall with no preconditions or invariants.
    let euid = unsafe { libc::geteuid() };
    if euid != 0 {
        eprintln!("Error: inspectah scan requires root privileges.");
        eprintln!("Run with: sudo inspectah scan [options]");
        std::process::exit(1);
    }

    validate_sensitivity_flags(args)?;

    let preserved = PreserveItem::expand(&args.preserve);
    let has_password_hashes = preserved.contains(&PreserveItem::PasswordHashes);
    let has_ssh_keys = preserved.contains(&PreserveItem::SshKeys);
    let has_subscription = preserved.contains(&PreserveItem::Subscription);

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
        Err(e) => {
            let msg = super::pull_failure::format_resolution_error(&e.to_string());
            eprint!("{msg}");
            std::process::exit(1);
        }
    };

    // Resolve rendering mode early — governs both pull viewport and scan progress.
    // Priority: CLI flag > INSPECTAH_PROGRESS env > TTY auto-detect.
    let mode = detect_mode(args.progress.as_ref());

    // Step 3: Extract baseline
    let baseline_data = match &normalized_ref {
        Some(norm) => {
            eprintln!("Pulling {}...", norm.as_str());

            let use_viewport = mode == crate::progress::Mode::Pretty;
            let mut collected_lines: Vec<String> = Vec::new();

            let data = if use_viewport {
                // TTY: viewport rendering
                let (term_width, term_height) = terminal_size::terminal_size()
                    .map(|(w, h)| (w.0 as usize, h.0 as usize))
                    .unwrap_or((80, 24));

                if term_width >= pull_progress::MIN_VIEWPORT_WIDTH {
                    let content_width = pull_progress::viewport_content_width(term_width);
                    let viewport_lines = pull_progress::viewport_height(term_height);
                    let mut ring: Vec<String> =
                        (0..viewport_lines).map(|_| String::new()).collect();
                    let mut ring_pos: usize = 0;

                    let result = {
                        let mut stderr_out = std::io::stderr().lock();
                        let mut callback = pull_progress::tty_viewport_callback(
                            &mut collected_lines,
                            &mut ring,
                            &mut ring_pos,
                            content_width,
                            &mut stderr_out,
                        );
                        inspectah_collect::baseline::extract_baseline(
                            &executor,
                            norm,
                            &mut callback,
                        )
                    };
                    match result {
                        Ok(data) => {
                            // Only clear viewport if lines were actually rendered.
                            if ring_pos > 0 {
                                pull_progress::viewport_cleanup(viewport_lines);
                            }
                            data
                        }
                        Err(_e) => {
                            if ring_pos > 0 {
                                pull_progress::viewport_cleanup(viewport_lines);
                            }
                            let stderr_combined = collected_lines.join("\n");
                            let kind = super::pull_failure::classify_pull_failure(&stderr_combined);
                            let msg = super::pull_failure::format_pull_error(
                                &kind,
                                norm.as_str(),
                                &stderr_combined,
                            );
                            eprint!("{msg}");
                            std::process::exit(3);
                        }
                    }
                } else {
                    // Narrow terminal — fall back to non-TTY
                    let result = {
                        let mut stderr_out = std::io::stderr().lock();
                        let mut callback =
                            pull_progress::non_tty_callback(&mut collected_lines, &mut stderr_out);
                        inspectah_collect::baseline::extract_baseline(
                            &executor,
                            norm,
                            &mut callback,
                        )
                    };
                    match result {
                        Ok(data) => data,
                        Err(_e) => {
                            let stderr_combined = collected_lines.join("\n");
                            let kind = super::pull_failure::classify_pull_failure(&stderr_combined);
                            let msg = super::pull_failure::format_pull_error(
                                &kind,
                                norm.as_str(),
                                &stderr_combined,
                            );
                            eprint!("{msg}");
                            std::process::exit(3);
                        }
                    }
                }
            } else {
                // Non-TTY: prefixed passthrough
                let result = {
                    let mut stderr_out = std::io::stderr().lock();
                    let mut callback =
                        pull_progress::non_tty_callback(&mut collected_lines, &mut stderr_out);
                    inspectah_collect::baseline::extract_baseline(&executor, norm, &mut callback)
                };
                match result {
                    Ok(data) => data,
                    Err(_e) => {
                        let stderr_combined = collected_lines.join("\n");
                        let kind = super::pull_failure::classify_pull_failure(&stderr_combined);
                        let msg = super::pull_failure::format_pull_error(
                            &kind,
                            norm.as_str(),
                            &stderr_combined,
                        );
                        eprint!("{msg}");
                        std::process::exit(3);
                    }
                }
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
        None => None,
    };

    // Step 4: Collect — run all inspectors
    let hostname = get_hostname(&executor);
    eprintln!("Inspecting host {hostname}...");

    // Build UserGroupOptions from CLI flags
    let user_group_options = UserGroupOptions {
        strategy_override: None,
        preserve_password_hashes: has_password_hashes,
        preserve_ssh_keys: has_ssh_keys,
    };

    let mut inspectors: Vec<Box<dyn Inspector>> = vec![
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

    // Add SubscriptionInspector when subscription is preserved
    if has_subscription {
        inspectors.push(Box::new(SubscriptionInspector::new()));
    }

    let verbosity = if args.quiet {
        crate::progress::Verbosity::Quiet
    } else if args.verbose {
        crate::progress::Verbosity::Verbose
    } else {
        crate::progress::Verbosity::Normal
    };

    let color = use_color();
    let progress = TerminalProgress::new(mode, color, verbosity, has_subscription);
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

    // SIGINT: stop the spinner, reconcile unfinished inspectors, finalize.
    if cancelled.load(Ordering::SeqCst) {
        progress.cancel();

        // The renderer is the authoritative outcome ledger.
        // Synthesize Interrupted events for any inspector that didn't finish.
        let finished = progress.finished_inspectors();
        let active_order = crate::progress::display::active_display_order(has_subscription);

        for (id, _name) in active_order {
            if !finished.contains(id) {
                progress.emit(
                    inspectah_core::types::progress::ProgressEvent::InspectorFinished {
                        id: *id,
                        outcome: inspectah_core::types::progress::InspectorOutcome::Interrupted,
                    },
                );
            }
        }

        let end_state = ScanEndState::Interrupted {
            completed: finished.len(),
            total: active_order.len(),
        };
        progress.finalize(ScanFinalize {
            elapsed: scan_start.elapsed(),
            end_state: end_state.clone(),
            version_changes: None,
        });
        if verbosity == crate::progress::Verbosity::Quiet {
            print_quiet_footer(scan_start.elapsed(), &end_state, None);
        }

        return Ok(ScanOutcome::Interrupted);
    }

    // Derive exit outcome from collection completeness
    let outcome = ScanOutcome::from_completeness(&collected.state.snapshot.completeness);

    // Step 5: Validate
    let validated = validate(collected).context("snapshot validation failed")?;

    // Step 6: Redact
    let mut snapshot = validated.state.snapshot;

    // Set Phase 6 fields on snapshot
    snapshot.target_image = target_image;
    snapshot.baseline = baseline_data;
    // Set sensitivity metadata from CLI flags
    snapshot.sensitive_snapshot =
        has_password_hashes || has_ssh_keys || has_subscription || args.no_redaction;
    snapshot.preserved_credentials = has_password_hashes;
    snapshot.preserved_ssh_keys = has_ssh_keys;
    snapshot.preserved_subscription = has_subscription;

    // Build version change summary for renderer (populated by RPM inspector during collection).
    let version_changes = build_version_change_summary(&snapshot);

    if args.no_redaction {
        snapshot.redaction_state = Some(RedactionState::Raw);
    } else {
        redact(&mut snapshot, &RedactOptions::default());
    }

    // Prompt for unmanaged file bundling if --include-unmanaged was used
    if args.include_unmanaged
        && let Some(ref unmanaged) = snapshot.unmanaged_files
        && !unmanaged.items.is_empty()
    {
        let size_display = format_size(unmanaged.total_size);
        let roots = describe_scan_roots(&unmanaged.items);
        if !assume_yes {
            eprintln!(
                "Found {} unmanaged files in {} ({} total)",
                unmanaged.total_count, roots, size_display,
            );
            eprint!("Include in tarball? [Y/n] ");
            use std::io::Write;
            std::io::stderr().flush().ok();
            let mut input = String::new();
            std::io::stdin().read_line(&mut input).ok();
            let input = input.trim().to_lowercase();
            if input == "n" || input == "no" {
                // Clear unmanaged files from snapshot
                snapshot.unmanaged_files = None;
            }
        }
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
                        let end_state = ScanEndState::InspectOnly { path: path.clone() };
                        progress.finalize(ScanFinalize {
                            elapsed: scan_start.elapsed(),
                            end_state: end_state.clone(),
                            version_changes: version_changes.clone(),
                        });
                        if verbosity == crate::progress::Verbosity::Quiet {
                            print_quiet_footer(scan_start.elapsed(), &end_state, None);
                        }
                    }
                    Err(e) => {
                        let end_state = ScanEndState::WriteFailure {
                            error: e.to_string(),
                        };
                        progress.finalize(ScanFinalize {
                            elapsed: scan_start.elapsed(),
                            end_state,
                            version_changes: version_changes.clone(),
                        });
                        anyhow::bail!("failed to write output: {e}");
                    }
                }
            }
            None => {
                println!("{json}");
                let end_state = ScanEndState::InspectOnlyStdout;
                progress.finalize(ScanFinalize {
                    elapsed: scan_start.elapsed(),
                    end_state: end_state.clone(),
                    version_changes: version_changes.clone(),
                });
                if verbosity == crate::progress::Verbosity::Quiet {
                    print_quiet_footer(scan_start.elapsed(), &end_state, None);
                }
            }
        }
        return Ok(outcome);
    }

    // Step 7: Render all artifacts to a temp directory
    let render_dir = tempfile::tempdir().context("failed to create temp directory")?;

    let render_context = RenderContext { target: None };
    render::render_all(&snapshot, &render_context, render_dir.path()).context("render failed")?;

    // Bundle unmanaged files into render directory if present
    if let Some(ref unmanaged) = snapshot.unmanaged_files {
        bundle_unmanaged_files(&unmanaged.items, render_dir.path())
            .context("failed to bundle unmanaged files")?;
    }

    // Bundle repo-less RPMs from dnf cache into render directory
    if let Some(ref rpm) = snapshot.rpm {
        bundle_repoless_rpms(&rpm.packages_added, render_dir.path())
            .context("failed to bundle repo-less RPMs")?;
    }

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
            let sensitivity = build_sensitivity_notice(&snapshot);
            let end_state = ScanEndState::Completed {
                path: tarball_path.clone(),
                sensitivity: sensitivity.clone(),
            };
            progress.finalize(ScanFinalize {
                elapsed: scan_start.elapsed(),
                end_state: end_state.clone(),
                version_changes,
            });
            if verbosity == crate::progress::Verbosity::Quiet {
                print_quiet_footer(scan_start.elapsed(), &end_state, sensitivity.as_deref());
            }
            Ok(outcome)
        }
        Err(e) => {
            progress.finalize(ScanFinalize {
                elapsed: scan_start.elapsed(),
                end_state: ScanEndState::WriteFailure {
                    error: e.to_string(),
                },
                version_changes,
            });
            anyhow::bail!("failed to write report: {e}");
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

/// Build a `VersionChangeSummary` from the snapshot's RPM version change data.
///
/// Returns `None` when baseline is absent or RPM comparison data is unavailable.
fn build_version_change_summary(
    snapshot: &inspectah_core::snapshot::InspectionSnapshot,
) -> Option<VersionChangeSummary> {
    snapshot.baseline.as_ref()?;
    let vcs = baseline_fmt::version_changes_for_display(snapshot)?;
    if vcs.is_empty() {
        return None;
    }
    use inspectah_core::types::rpm::VersionChangeDirection;
    let target_newer = vcs
        .iter()
        .filter(|vc| vc.direction == VersionChangeDirection::Upgrade)
        .count();
    let host_newer = vcs.len() - target_newer;
    Some(VersionChangeSummary {
        total: vcs.len(),
        target_newer,
        host_newer,
    })
}

/// Format a cert expiry line for the scan summary.
///
/// Returns `None` when the snapshot has no subscription section or no expiry date.
/// Uses `time::OffsetDateTime::now_utc()` to compute days remaining.
fn format_cert_expiry_line(
    snapshot: &inspectah_core::snapshot::InspectionSnapshot,
) -> Option<String> {
    let sub = snapshot.subscription.as_ref()?;
    let expiry = sub.earliest_expiry?;

    let now = time::OffsetDateTime::now_utc();
    let diff = expiry - now;
    let days = diff.whole_days();

    let format =
        time::format_description::parse("[year]-[month]-[day]").expect("static format description");
    let date_str = expiry.format(&format).unwrap_or_else(|_| "unknown".into());

    if days < 0 {
        let abs_days = days.unsigned_abs();
        let day_word = if abs_days == 1 { "day" } else { "days" };
        Some(format!(
            "   \u{26a0} Subscription certs EXPIRED: {date_str} ({abs_days} {day_word} ago) \
             \u{2014} will not work on unregistered systems"
        ))
    } else if days < 7 {
        let day_word = if days == 1 { "day" } else { "days" };
        Some(format!(
            "   \u{26a0} Subscription certs expire: {date_str} ({days} {day_word}) \
             \u{2014} rebuild soon"
        ))
    } else {
        Some(format!(
            "   Subscription certs expire: {date_str} ({days} days)"
        ))
    }
}

/// Build the sensitivity notice string for the `Completed` footer.
///
/// Returns `None` when the snapshot has no sensitive data.
fn build_sensitivity_notice(
    snapshot: &inspectah_core::snapshot::InspectionSnapshot,
) -> Option<String> {
    if !snapshot.sensitive_snapshot {
        return None;
    }

    let mut preserved_items = Vec::new();
    if snapshot.preserved_credentials {
        preserved_items.push("password-hashes");
    }
    if snapshot.preserved_ssh_keys {
        preserved_items.push("ssh-keys");
    }
    if snapshot.preserved_subscription {
        preserved_items.push("subscription");
    }

    let is_raw = matches!(snapshot.redaction_state, Some(RedactionState::Raw));

    let mut lines = Vec::new();
    lines.push("\u{26a0}  Snapshot contains sensitive data:".to_string());
    if !preserved_items.is_empty() {
        lines.push(format!("   Preserved: {}", preserved_items.join(", ")));
    }

    // Show cert expiry warning when subscription material is preserved
    if snapshot.preserved_subscription
        && let Some(expiry_line) = format_cert_expiry_line(snapshot)
    {
        lines.push(expiry_line);
    }

    if is_raw {
        lines.push("   Redaction: skipped (raw secrets retained)".to_string());
    } else {
        lines.push("   Redaction: active".to_string());
    }

    Some(lines.join("\n"))
}

/// Print a minimal footer for `--quiet` mode (Null renderer swallows finalize).
///
/// Matches `ScanEndState` variants so each end-state gets the right output:
/// - Completed: timing + report path + refine hint + sensitivity notice
/// - InspectOnly: timing + output path (no refine hint)
/// - InspectOnlyStdout: timing only
/// - WriteFailure: timing + error
/// - Interrupted: cancellation message only
fn print_quiet_footer(
    elapsed: std::time::Duration,
    end_state: &ScanEndState,
    sensitivity: Option<&str>,
) {
    let secs = elapsed.as_secs_f64();
    match end_state {
        ScanEndState::Completed { path, .. } => {
            eprintln!("Scan complete ({secs:.0}s)");
            eprintln!("Report: {}", path.display());
            eprintln!("To review: inspectah refine {}", path.display());
            if let Some(notice) = sensitivity {
                for line in notice.lines() {
                    eprintln!("  {line}");
                }
            }
        }
        ScanEndState::InspectOnly { path } => {
            eprintln!("Scan complete ({secs:.0}s)");
            eprintln!("Output: {}", path.display());
        }
        ScanEndState::InspectOnlyStdout => {
            eprintln!("Scan complete ({secs:.0}s)");
        }
        ScanEndState::WriteFailure { error } => {
            eprintln!("Scan complete ({secs:.0}s)");
            eprintln!("Error: {error}");
        }
        ScanEndState::Interrupted { .. } => {
            eprintln!("Scan cancelled. No report written.");
        }
    }
}

/// Format a byte count as human-readable size string.
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

/// Describe which scan roots contain unmanaged files.
fn describe_scan_roots(items: &[inspectah_core::types::nonrpm::UnmanagedFile]) -> String {
    let mut roots: Vec<&str> = Vec::new();
    for item in items {
        for root in &["/opt", "/srv", "/usr/local"] {
            if item.path.starts_with(root) && !roots.contains(root) {
                roots.push(root);
            }
        }
    }
    if roots.is_empty() {
        "unknown".to_string()
    } else {
        roots.join(", ")
    }
}

/// Bundle unmanaged files into the render directory for tarball inclusion.
fn bundle_unmanaged_files(
    items: &[inspectah_core::types::nonrpm::UnmanagedFile],
    render_dir: &Path,
) -> Result<()> {
    for item in items {
        if !item.include {
            continue;
        }
        // Strip leading / to create relative path under unmanaged/
        let rel_path = item.path.trim_start_matches('/');
        let dest = render_dir.join("unmanaged").join(rel_path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create dir for {}", dest.display()))?;
        }
        std::fs::copy(&item.path, &dest)
            .with_context(|| format!("failed to copy {} to tarball", item.path))?;
    }
    Ok(())
}

/// Bundle cached repo-less RPMs into the render directory for tarball inclusion.
///
/// Copies each package's cached `.rpm` file (identified by `cache_path`)
/// into `repoless-packages/` under the render directory, using a
/// canonical NEVRA filename.
fn bundle_repoless_rpms(
    packages: &[inspectah_core::types::rpm::PackageEntry],
    render_dir: &Path,
) -> Result<()> {
    let dest_dir = render_dir.join("repoless-packages");
    for pkg in packages {
        if let Some(ref cache_path) = pkg.cache_path {
            std::fs::create_dir_all(&dest_dir).context("failed to create repoless-packages dir")?;
            let filename = format!(
                "{}-{}-{}.{}.rpm",
                pkg.name, pkg.version, pkg.release, pkg.arch
            );
            let dest = dest_dir.join(&filename);
            std::fs::copy(cache_path, &dest)
                .with_context(|| format!("failed to copy cached RPM {cache_path}"))?;
        }
    }
    Ok(())
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

    // --- Helper for test isolation ---

    fn base_args() -> ScanArgs {
        ScanArgs {
            inspect_only: false,
            output: None,
            base_image: None,
            preserve: vec![],
            no_redaction: false,
            ack_sensitive: false,
            progress: None,
            verbose: false,
            quiet: false,
            include_unmanaged: false,
            exclude_path: vec![],
        }
    }

    // --- ack-sensitive validation ---

    #[test]
    fn preserve_without_ack_is_error() {
        let args = ScanArgs {
            preserve: vec![PreserveItem::SshKeys],
            ..base_args()
        };
        let result = validate_sensitivity_flags(&args);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("--preserve requires --ack-sensitive"));
    }

    #[test]
    fn no_redaction_without_ack_is_error() {
        let args = ScanArgs {
            no_redaction: true,
            ..base_args()
        };
        let result = validate_sensitivity_flags(&args);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("--no-redaction requires --ack-sensitive"));
    }

    #[test]
    fn both_without_ack_is_error() {
        let args = ScanArgs {
            preserve: vec![PreserveItem::All],
            no_redaction: true,
            ..base_args()
        };
        let result = validate_sensitivity_flags(&args);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("--preserve and --no-redaction require --ack-sensitive"));
    }

    #[test]
    fn preserve_with_ack_is_ok() {
        let args = ScanArgs {
            preserve: vec![PreserveItem::SshKeys],
            ack_sensitive: true,
            ..base_args()
        };
        assert!(validate_sensitivity_flags(&args).is_ok());
    }

    #[test]
    fn no_redaction_with_ack_is_ok() {
        let args = ScanArgs {
            no_redaction: true,
            ack_sensitive: true,
            ..base_args()
        };
        assert!(validate_sensitivity_flags(&args).is_ok());
    }

    #[test]
    fn no_sensitive_flags_is_ok() {
        let args = base_args();
        assert!(validate_sensitivity_flags(&args).is_ok());
    }

    // --- PreserveItem expansion ---

    #[test]
    fn expand_all_returns_concrete_variants() {
        let items = vec![PreserveItem::All];
        let expanded = PreserveItem::expand(&items);
        assert_eq!(expanded.len(), 3);
        assert!(expanded.contains(&PreserveItem::PasswordHashes));
        assert!(expanded.contains(&PreserveItem::SshKeys));
        assert!(expanded.contains(&PreserveItem::Subscription));
        assert!(!expanded.contains(&PreserveItem::All));
    }

    #[test]
    fn expand_deduplicates_redundant_with_all() {
        let items = vec![PreserveItem::All, PreserveItem::SshKeys];
        let expanded = PreserveItem::expand(&items);
        assert_eq!(expanded.len(), 3);
    }

    #[test]
    fn expand_deduplicates_repeated_items() {
        let items = vec![PreserveItem::SshKeys, PreserveItem::SshKeys];
        let expanded = PreserveItem::expand(&items);
        assert_eq!(expanded.len(), 1);
        assert_eq!(expanded[0], PreserveItem::SshKeys);
    }

    #[test]
    fn expand_empty_returns_empty() {
        let items: Vec<PreserveItem> = vec![];
        let expanded = PreserveItem::expand(&items);
        assert!(expanded.is_empty());
    }

    #[test]
    fn expand_single_item() {
        let items = vec![PreserveItem::Subscription];
        let expanded = PreserveItem::expand(&items);
        assert_eq!(expanded.len(), 1);
        assert_eq!(expanded[0], PreserveItem::Subscription);
    }

    // --- cert expiry line formatting ---

    fn snapshot_with_expiry(
        expiry: time::OffsetDateTime,
    ) -> inspectah_core::snapshot::InspectionSnapshot {
        let mut snap = inspectah_core::snapshot::InspectionSnapshot::new();
        snap.preserved_subscription = true;
        snap.subscription = Some(inspectah_core::types::subscription::SubscriptionSection {
            earliest_expiry: Some(expiry),
            ..Default::default()
        });
        snap
    }

    #[test]
    fn cert_expiry_far_future_shows_days() {
        let expiry = time::OffsetDateTime::now_utc() + time::Duration::days(30);
        let snap = snapshot_with_expiry(expiry);
        let line = format_cert_expiry_line(&snap).expect("should produce line");
        assert!(line.contains("Subscription certs expire:"));
        assert!(line.contains("days)"));
        // No warning symbol in normal case
        assert!(!line.contains("\u{26a0}"));
    }

    #[test]
    fn cert_expiry_within_7_days_warns() {
        let expiry = time::OffsetDateTime::now_utc() + time::Duration::days(3);
        let snap = snapshot_with_expiry(expiry);
        let line = format_cert_expiry_line(&snap).expect("should produce line");
        assert!(line.contains("\u{26a0}"));
        assert!(line.contains("rebuild soon"));
    }

    #[test]
    fn cert_expiry_already_expired_shows_error() {
        let expiry = time::OffsetDateTime::now_utc() - time::Duration::days(2);
        let snap = snapshot_with_expiry(expiry);
        let line = format_cert_expiry_line(&snap).expect("should produce line");
        assert!(line.contains("EXPIRED"));
        assert!(line.contains("ago)"));
        assert!(line.contains("will not work"));
    }

    #[test]
    fn cert_expiry_none_returns_none() {
        let mut snap = inspectah_core::snapshot::InspectionSnapshot::new();
        snap.preserved_subscription = true;
        snap.subscription =
            Some(inspectah_core::types::subscription::SubscriptionSection::default());
        assert!(format_cert_expiry_line(&snap).is_none());
    }

    #[test]
    fn cert_expiry_no_subscription_returns_none() {
        let snap = inspectah_core::snapshot::InspectionSnapshot::new();
        assert!(format_cert_expiry_line(&snap).is_none());
    }

    #[test]
    fn sensitivity_notice_includes_cert_expiry() {
        let expiry = time::OffsetDateTime::now_utc() + time::Duration::days(5);
        let mut snap = snapshot_with_expiry(expiry);
        snap.sensitive_snapshot = true;
        let notice = build_sensitivity_notice(&snap).expect("should produce notice");
        assert!(notice.contains("Preserved: subscription"));
        assert!(notice.contains("Subscription certs expire:"));
        assert!(notice.contains("rebuild soon"));
    }

    #[test]
    fn sensitivity_notice_no_subscription_no_expiry() {
        let mut snap = inspectah_core::snapshot::InspectionSnapshot::new();
        snap.sensitive_snapshot = true;
        snap.preserved_ssh_keys = true;
        let notice = build_sensitivity_notice(&snap).expect("should produce notice");
        assert!(notice.contains("ssh-keys"));
        assert!(!notice.contains("Subscription certs"));
    }
}
