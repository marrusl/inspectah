//! `inspectah fleet` subcommand tree.
//!
//! Provides two subcommands:
//! - `fleet aggregate` — merge host tarballs into a fleet-aggregate snapshot
//! - `fleet init` — generate a fleet manifest from a directory of tarballs

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use std::path::{Path, PathBuf};

use inspectah_core::fleet::manifest::FleetManifest;
use inspectah_core::fleet::merge_snapshots;
use inspectah_core::fleet::validate::{FleetValidationError, FleetWarning};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_pipeline::render;
use inspectah_pipeline::render::tarball::{create_tarball, get_output_stamp};

#[derive(Debug, Args)]
pub struct FleetArgs {
    #[command(subcommand)]
    pub command: FleetSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum FleetSubcommand {
    /// Aggregate host tarballs into a fleet tarball
    Aggregate(FleetAggregateArgs),
    /// Generate a fleet manifest from a directory of tarballs
    Init(FleetInitArgs),
}

#[derive(Debug, Args)]
pub struct FleetAggregateArgs {
    /// Input tarballs or directory containing tarballs
    pub inputs: Vec<PathBuf>,

    /// Path to a fleet manifest (TOML) specifying sources
    #[arg(long)]
    pub manifest: Option<PathBuf>,

    /// Override the baseline image reference
    #[arg(long)]
    pub baseline: Option<String>,

    /// Output directory for the fleet tarball
    #[arg(long)]
    pub output_dir: Option<PathBuf>,

    /// Output file path for the fleet tarball
    #[arg(long)]
    pub output_file: Option<PathBuf>,

    /// Write JSON snapshot to stdout (or --output-file) instead of tarball
    #[arg(long)]
    pub json_only: bool,

    /// Treat warnings as errors
    #[arg(long)]
    pub strict: bool,

    /// Show per-host detail in output
    #[arg(long, short)]
    pub verbose: bool,
}

#[derive(Debug, Args)]
pub struct FleetInitArgs {
    /// Directory containing host tarballs
    pub directory: PathBuf,

    /// Output path for the generated manifest
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Overwrite an existing manifest file
    #[arg(long)]
    pub overwrite: bool,
}

/// Entry point for `inspectah fleet`.
pub fn run_fleet(args: &FleetArgs) -> Result<()> {
    match &args.command {
        FleetSubcommand::Aggregate(agg) => run_aggregate(agg),
        FleetSubcommand::Init(_init) => {
            bail!("fleet init is not yet implemented (coming in a future release)")
        }
    }
}

// ---------------------------------------------------------------------------
// Aggregate implementation
// ---------------------------------------------------------------------------

/// A successfully loaded host tarball.
struct HostInput {
    snapshot: InspectionSnapshot,
}

/// A file that could not be parsed as a valid snapshot tarball.
struct UnparseableFile {
    path: PathBuf,
    reason: String,
}

fn run_aggregate(args: &FleetAggregateArgs) -> Result<()> {
    // --- Step 1: Resolve inputs ---
    let (tarball_paths, label, manifest) = resolve_inputs(args)?;

    if tarball_paths.is_empty() {
        bail!("no tarball files found");
    }

    // --- Step 2: Load snapshots from tarballs ---
    let mut hosts: Vec<HostInput> = Vec::new();
    let mut unparseable: Vec<UnparseableFile> = Vec::new();

    for path in &tarball_paths {
        match load_snapshot_from_tarball(path) {
            Ok(snapshot) => hosts.push(HostInput { snapshot }),
            Err(e) => unparseable.push(UnparseableFile {
                path: path.clone(),
                reason: format!("{e:#}"),
            }),
        }
    }

    // Report unparseable files as warnings
    for uf in &unparseable {
        eprintln!(
            "warning: skipping {}: {}",
            uf.path.display(),
            uf.reason
        );
    }

    if hosts.is_empty() {
        bail!(
            "no valid snapshots found ({} file(s) could not be parsed)",
            unparseable.len()
        );
    }

    // --- Step 3: Merge snapshots ---
    let snapshots: Vec<InspectionSnapshot> = hosts
        .into_iter()
        .map(|h| h.snapshot)
        .collect();

    let (merged, warnings) = merge_snapshots(snapshots, manifest.as_ref())
        .map_err(|errors| format_validation_errors(&errors))?;

    // --- Step 4: Collect all warnings (core + CLI-layer) ---
    let mut all_warnings: Vec<String> = Vec::new();

    for w in &warnings {
        all_warnings.push(format_warning(w));
    }
    for uf in &unparseable {
        all_warnings.push(format!(
            "unparseable file: {} ({})",
            uf.path.display(),
            uf.reason
        ));
    }

    // Print warnings
    for w in &all_warnings {
        eprintln!("warning: {w}");
    }

    // --strict: promote warnings to errors
    if args.strict && !all_warnings.is_empty() {
        bail!(
            "aborting due to --strict: {} warning(s)",
            all_warnings.len()
        );
    }

    // --- Step 5: JSON-only output mode ---
    if args.json_only {
        let json = serde_json::to_string_pretty(&merged)
            .context("failed to serialize merged snapshot")?;

        match &args.output_file {
            Some(path) => {
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        std::fs::create_dir_all(parent)
                            .context("failed to create output directory")?;
                    }
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

    // --- Step 6: Render all artifacts to temp dir ---
    let render_dir = tempfile::tempdir().context("failed to create temp directory")?;

    let render_context = RenderContext { target: None };
    render::render_all(&merged, &render_context, render_dir.path())
        .context("render failed")?;

    // Write schema placeholder (same as scan.rs)
    let schema_dir = render_dir.path().join("schema");
    std::fs::create_dir_all(&schema_dir)?;
    std::fs::write(
        schema_dir.join("snapshot.schema.json"),
        r#"{"$schema":"http://json-schema.org/draft-07/schema#","title":"InspectionSnapshot","description":"Phase 7 placeholder","type":"object"}"#,
    )?;

    // --- Step 7: Create tarball ---
    let stamp = get_output_stamp(&format!("fleet-{label}"));
    let tarball_name = format!("{stamp}.tar.gz");

    let tarball_path = if let Some(path) = &args.output_file {
        path.clone()
    } else if let Some(dir) = &args.output_dir {
        dir.join(&tarball_name)
    } else {
        PathBuf::from(&tarball_name)
    };

    if let Some(parent) = tarball_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).context("failed to create output directory")?;
        }
    }

    create_tarball(render_dir.path(), &tarball_path, &stamp)
        .with_context(|| format!("failed to create tarball at {}", tarball_path.display()))?;

    // --- Step 8: Output summary ---
    let fleet_meta = merged.fleet_meta.as_ref();
    let host_count = fleet_meta.map_or(0, |m| m.host_count);
    let pkg_count = merged
        .rpm
        .as_ref()
        .map_or(0, |r| r.packages_added.len());
    let config_count = merged
        .config
        .as_ref()
        .map_or(0, |c| c.files.len());
    let svc_count = merged
        .services
        .as_ref()
        .map_or(0, |s| s.state_changes.len());

    eprintln!("Fleet: {label} ({host_count} hosts)");
    eprintln!("Merged: {pkg_count} packages, {config_count} config files, {svc_count} services");

    if args.verbose {
        if let Some(meta) = fleet_meta {
            eprintln!("Hosts:");
            for hostname in &meta.hostnames {
                eprintln!("  - {hostname}");
            }
            if !meta.section_host_counts.is_empty() {
                eprintln!("Section coverage:");
                for (section, count) in &meta.section_host_counts {
                    eprintln!("  {section}: {count}/{host_count} hosts");
                }
            }
        }
    }

    eprintln!("Output: {}", tarball_path.display());

    Ok(())
}

// ---------------------------------------------------------------------------
// Input resolution
// ---------------------------------------------------------------------------

/// Resolve CLI arguments into a list of tarball paths, a label, and an
/// optional manifest.
fn resolve_inputs(
    args: &FleetAggregateArgs,
) -> Result<(Vec<PathBuf>, String, Option<FleetManifest>)> {
    // Mutual exclusion: --manifest and positional inputs
    if args.manifest.is_some() && !args.inputs.is_empty() {
        bail!("cannot specify both --manifest and positional input paths");
    }

    // Mode 1: Manifest-driven
    if let Some(manifest_path) = &args.manifest {
        let mut manifest = FleetManifest::load(manifest_path)
            .map_err(|e| anyhow::anyhow!("failed to load manifest from {}: {e}", manifest_path.display()))?;

        // CLI --baseline overrides manifest baseline
        if let Some(baseline) = &args.baseline {
            manifest.baseline = Some(baseline.clone());
        }

        let label = manifest
            .label
            .clone()
            .unwrap_or_else(|| "fleet".into());
        let paths = manifest.sources.clone();

        return Ok((paths, label, Some(manifest)));
    }

    // Mode 2: Single directory input
    if args.inputs.len() == 1 && args.inputs[0].is_dir() {
        let dir = &args.inputs[0];
        let label = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("fleet")
            .to_string();

        let mut paths = list_tarballs_in_dir(dir)?;
        paths.sort();

        let manifest = build_manifest_from_args(&label, &paths, args);
        return Ok((paths, label, Some(manifest)));
    }

    // Mode 3: Multiple explicit tarball paths
    if !args.inputs.is_empty() {
        let label = "fleet".to_string();
        let paths = args.inputs.clone();

        let manifest = build_manifest_from_args(&label, &paths, args);
        return Ok((paths, label, Some(manifest)));
    }

    bail!("no inputs specified — provide tarball paths, a directory, or --manifest");
}

/// Build a FleetManifest from CLI arguments (for non-manifest modes).
fn build_manifest_from_args(
    label: &str,
    paths: &[PathBuf],
    args: &FleetAggregateArgs,
) -> FleetManifest {
    FleetManifest {
        label: Some(label.to_string()),
        baseline: args.baseline.clone(),
        sources: paths.to_vec(),
    }
}

/// List `.tar.gz` files in a directory (non-recursive).
fn list_tarballs_in_dir(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut tarballs = Vec::new();

    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("failed to read directory {}", dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.ends_with(".tar.gz") {
                    tarballs.push(path);
                }
            }
        }
    }

    Ok(tarballs)
}

// ---------------------------------------------------------------------------
// Tarball loading
// ---------------------------------------------------------------------------

/// Extract `inspection-snapshot.json` from a host tarball and parse it.
fn load_snapshot_from_tarball(tarball_path: &Path) -> Result<InspectionSnapshot> {
    let file = std::fs::File::open(tarball_path)
        .with_context(|| format!("failed to open {}", tarball_path.display()))?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);

    for entry_result in archive.entries().context("failed to read tarball entries")? {
        let mut entry = entry_result.context("failed to read tarball entry")?;
        let path = entry.path().context("invalid entry path")?;

        // Match inspection-snapshot.json at any prefix depth
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .map_or(false, |n| n == "inspection-snapshot.json")
        {
            let mut json = String::new();
            std::io::Read::read_to_string(&mut entry, &mut json)
                .context("failed to read inspection-snapshot.json from tarball")?;
            let snapshot = InspectionSnapshot::load(&json)
                .map_err(|e| anyhow::anyhow!("failed to parse snapshot: {e}"))?;
            return Ok(snapshot);
        }
    }

    bail!(
        "no inspection-snapshot.json found in {}",
        tarball_path.display()
    )
}

// ---------------------------------------------------------------------------
// Warning / error formatting
// ---------------------------------------------------------------------------

fn format_warning(w: &FleetWarning) -> String {
    match w {
        FleetWarning::StaleScanDates { spread_description } => {
            format!("stale scan dates: {spread_description}")
        }
        FleetWarning::BaselineConflict {
            distribution,
            selected,
        } => {
            let dist: Vec<String> = distribution
                .iter()
                .map(|(img, count)| format!("{img} ({count})"))
                .collect();
            format!(
                "baseline conflict: selected {selected} from [{}]",
                dist.join(", ")
            )
        }
        FleetWarning::MinorVersionSpread { versions } => {
            format!("minor version spread: {}", versions.join(", "))
        }
        FleetWarning::SystemTypeMismatch { types } => {
            format!("system type mismatch: {}", types.join(", "))
        }
    }
}

fn format_validation_errors(errors: &[FleetValidationError]) -> anyhow::Error {
    let msgs: Vec<String> = errors
        .iter()
        .map(|e| match e {
            FleetValidationError::TooFewSnapshots { count } => {
                format!("too few snapshots: {count} (need at least 2)")
            }
            FleetValidationError::SchemaVersionMismatch { versions } => {
                format!(
                    "schema version mismatch: {:?}",
                    versions
                )
            }
            FleetValidationError::DuplicateHostname { hostname } => {
                format!("duplicate hostname: {hostname}")
            }
            FleetValidationError::ArchitectureMismatch { architectures } => {
                format!(
                    "architecture mismatch: {}",
                    architectures.join(", ")
                )
            }
            FleetValidationError::EmptySnapshot { hostname } => {
                format!("empty snapshot: {hostname}")
            }
            FleetValidationError::OsMajorVersionMismatch { versions } => {
                format!(
                    "OS major version mismatch: {}",
                    versions.join(", ")
                )
            }
        })
        .collect();

    anyhow::anyhow!("fleet validation failed:\n  {}", msgs.join("\n  "))
}
