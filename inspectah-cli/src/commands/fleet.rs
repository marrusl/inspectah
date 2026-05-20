//! `inspectah fleet` subcommand tree.
//!
//! Provides two subcommands:
//! - `fleet aggregate` — merge host tarballs into a fleet-aggregate snapshot
//! - `fleet init` — generate a fleet manifest from a directory of tarballs

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use inspectah_core::fleet::manifest::FleetManifest;
use inspectah_core::fleet::merge_snapshots;
use inspectah_core::fleet::validate::{FleetValidationError, FleetWarning};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::fleet::VariantSelection;
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

    /// Write JSON snapshot instead of tarball (to stdout, --output-file, or --output-dir)
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
        FleetSubcommand::Init(init) => run_init(init),
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
    // --- Flag validation ---
    if args.output_file.is_some() && args.output_dir.is_some() {
        bail!("--output-file and --output-dir are mutually exclusive");
    }

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
        eprintln!("warning: skipping {}: {}", uf.path.display(), uf.reason);
    }

    if hosts.is_empty() {
        bail!(
            "no valid snapshots found ({} file(s) could not be parsed)",
            unparseable.len()
        );
    }

    // --- Step 3: Merge snapshots ---
    let snapshots: Vec<InspectionSnapshot> = hosts.into_iter().map(|h| h.snapshot).collect();

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
        let json =
            serde_json::to_string_pretty(&merged).context("failed to serialize merged snapshot")?;

        if let Some(path) = &args.output_file {
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent).context("failed to create output directory")?;
            }
            std::fs::write(path, &json)
                .with_context(|| format!("failed to write {}", path.display()))?;
            eprintln!("Snapshot written to {}", path.display());
        } else if let Some(dir) = &args.output_dir {
            std::fs::create_dir_all(dir).context("failed to create output directory")?;
            let path = dir.join("inspection-snapshot.json");
            std::fs::write(&path, &json)
                .with_context(|| format!("failed to write {}", path.display()))?;
            eprintln!("Snapshot written to {}", path.display());
        } else {
            println!("{json}");
        }
        return Ok(());
    }

    // --- Step 6: Render all artifacts to temp dir ---
    let render_dir = tempfile::tempdir().context("failed to create temp directory")?;

    let render_context = RenderContext { target: None };
    render::render_all(&merged, &render_context, render_dir.path()).context("render failed")?;

    // Write schema placeholder (same as scan.rs)
    let schema_dir = render_dir.path().join("schema");
    std::fs::create_dir_all(&schema_dir)?;
    std::fs::write(
        schema_dir.join("snapshot.schema.json"),
        r#"{"$schema":"http://json-schema.org/draft-07/schema#","title":"InspectionSnapshot","description":"Phase 7 placeholder","type":"object"}"#,
    )?;

    // --- Step 6.5: Write variant files and prepend Containerfile header ---
    write_variant_files(&merged, render_dir.path())?;
    prepend_containerfile_header(&merged, render_dir.path(), &label)?;

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

    if let Some(parent) = tarball_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).context("failed to create output directory")?;
    }

    create_tarball(render_dir.path(), &tarball_path, &stamp)
        .with_context(|| format!("failed to create tarball at {}", tarball_path.display()))?;

    // --- Step 8: Output summary ---
    let fleet_meta = merged.fleet_meta.as_ref();
    let host_count = fleet_meta.map_or(0, |m| m.host_count);
    let pkg_count = merged.rpm.as_ref().map_or(0, |r| r.packages_added.len());
    let config_count = merged.config.as_ref().map_or(0, |c| c.files.len());
    let svc_count = merged
        .services
        .as_ref()
        .map_or(0, |s| s.state_changes.len());

    eprintln!("Fleet: {label} ({host_count} hosts)");
    eprintln!("Merged: {pkg_count} packages, {config_count} config files, {svc_count} services");

    if args.verbose
        && let Some(meta) = fleet_meta
    {
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

    eprintln!("Output: {}", tarball_path.display());

    Ok(())
}

// ---------------------------------------------------------------------------
// Init implementation
// ---------------------------------------------------------------------------

/// Metadata extracted from a tarball for manifest generation.
struct TarballMetadata {
    path: PathBuf,
    target_image: Option<String>,
}

fn run_init(args: &FleetInitArgs) -> Result<()> {
    // --- Step 1: Verify directory exists ---
    if !args.directory.is_dir() {
        bail!("{} is not a directory", args.directory.display());
    }

    // --- Step 2: Scan directory for tarballs ---
    let tarball_paths = list_tarballs_in_dir(&args.directory)?;

    if tarball_paths.is_empty() {
        bail!("no .tar.gz files found in {}", args.directory.display());
    }

    // --- Step 3: Extract metadata from each tarball ---
    let mut metadata_list: Vec<TarballMetadata> = Vec::new();
    let mut failed: Vec<(PathBuf, String)> = Vec::new();

    for path in &tarball_paths {
        match extract_tarball_metadata(path) {
            Ok(meta) => metadata_list.push(meta),
            Err(e) => {
                failed.push((path.clone(), format!("{e:#}")));
                eprintln!("warning: skipping {}: {e:#}", path.display());
            }
        }
    }

    if metadata_list.is_empty() {
        bail!(
            "no valid snapshots found ({} file(s) could not be parsed)",
            failed.len()
        );
    }

    // --- Step 4: Determine output path ---
    let output_path = args
        .output
        .clone()
        .unwrap_or_else(|| PathBuf::from("fleet.toml"));

    // --- Step 5: Check for existing file ---
    if output_path.exists() && !args.overwrite {
        bail!(
            "{} already exists (use --overwrite to replace)",
            output_path.display()
        );
    }

    // --- Step 6: Detect baseline conflicts ---
    let mut image_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for meta in &metadata_list {
        if let Some(img) = &meta.target_image {
            *image_counts.entry(img.clone()).or_insert(0) += 1;
        }
    }

    let baseline = if image_counts.is_empty() {
        None
    } else {
        // Pick the most common image
        let (most_common, _count) = image_counts.iter().max_by_key(|(_, count)| *count).unwrap();

        // Warn if there are conflicts
        if image_counts.len() > 1 {
            let dist: Vec<String> = image_counts
                .iter()
                .map(|(img, count)| format!("{img} ({count})"))
                .collect();
            eprintln!(
                "warning: baseline conflict: selected {} from [{}]",
                most_common,
                dist.join(", ")
            );
        }

        Some(most_common.clone())
    };

    // --- Step 7: Generate relative paths for manifest ---
    let manifest_parent = output_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .context("failed to resolve manifest parent directory")?;

    let mut sources: Vec<PathBuf> = Vec::new();
    for meta in &metadata_list {
        let abs_path = meta
            .path
            .canonicalize()
            .with_context(|| format!("failed to resolve {}", meta.path.display()))?;

        // Use pathdiff to create a relative path from manifest dir to tarball
        let rel_path =
            pathdiff::diff_paths(&abs_path, &manifest_parent).unwrap_or_else(|| abs_path.clone());

        sources.push(rel_path);
    }

    // --- Step 8: Generate TOML manifest ---
    let label = args
        .directory
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("fleet");

    let toml = generate_manifest_toml(label, baseline.as_deref(), &sources);

    // --- Step 9: Write manifest file ---
    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).context("failed to create output directory")?;
    }

    std::fs::write(&output_path, &toml)
        .with_context(|| format!("failed to write {}", output_path.display()))?;

    // --- Step 10: Output summary ---
    eprintln!(
        "Wrote {} ({} sources{})",
        output_path.display(),
        sources.len(),
        baseline
            .as_ref()
            .map_or(String::new(), |b| format!(", baseline: {b}"))
    );

    Ok(())
}

/// Extract minimal metadata (hostname, target_image) from a tarball.
fn extract_tarball_metadata(tarball_path: &Path) -> Result<TarballMetadata> {
    let file = std::fs::File::open(tarball_path)
        .with_context(|| format!("failed to open {}", tarball_path.display()))?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);

    for entry_result in archive
        .entries()
        .context("failed to read tarball entries")?
    {
        let mut entry = entry_result.context("failed to read tarball entry")?;
        let path = entry.path().context("invalid entry path")?;

        if path.file_name().and_then(|n| n.to_str()) == Some("inspection-snapshot.json") {
            let mut json = String::new();
            std::io::Read::read_to_string(&mut entry, &mut json)
                .context("failed to read inspection-snapshot.json")?;

            // Parse just the fields we need
            let v: serde_json::Value =
                serde_json::from_str(&json).context("failed to parse JSON")?;

            let target_image = v
                .get("target_image")
                .and_then(|t| t.get("image_ref"))
                .and_then(|r| r.as_str())
                .map(|s| s.to_string());

            return Ok(TarballMetadata {
                path: tarball_path.to_path_buf(),
                target_image,
            });
        }
    }

    bail!(
        "no inspection-snapshot.json found in {}",
        tarball_path.display()
    )
}

/// Generate a TOML manifest string with comments.
fn generate_manifest_toml(label: &str, baseline: Option<&str>, sources: &[PathBuf]) -> String {
    let mut toml = String::new();

    toml.push_str("# inspectah fleet manifest\n");
    toml.push_str("# Edit label and baseline as needed. Sources are relative to this file.\n\n");

    toml.push_str(&format!("label = \"{label}\"\n"));

    if let Some(b) = baseline {
        toml.push_str(&format!("baseline = \"{b}\"\n"));
    } else {
        toml.push_str("# baseline = \"registry.redhat.io/rhel9/rhel-bootc:9.6\"\n");
    }

    toml.push_str("\nsources = [\n");
    for source in sources {
        let path_str = source.display().to_string();
        toml.push_str(&format!("  \"{path_str}\",\n"));
    }
    toml.push_str("]\n");

    toml
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
        let mut manifest = FleetManifest::load(manifest_path).map_err(|e| {
            anyhow::anyhow!(
                "failed to load manifest from {}: {e}",
                manifest_path.display()
            )
        })?;

        // CLI --baseline overrides manifest baseline
        if let Some(baseline) = &args.baseline {
            manifest.baseline = Some(baseline.clone());
        }

        let label = manifest.label.clone().unwrap_or_else(|| "fleet".into());
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
        if path.is_file()
            && let Some(name) = path.file_name().and_then(|n| n.to_str())
            && name.ends_with(".tar.gz")
        {
            tarballs.push(path);
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

    for entry_result in archive
        .entries()
        .context("failed to read tarball entries")?
    {
        let mut entry = entry_result.context("failed to read tarball entry")?;
        let path = entry.path().context("invalid entry path")?;

        // Match inspection-snapshot.json at any prefix depth
        if path.file_name().and_then(|n| n.to_str()) == Some("inspection-snapshot.json") {
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
// Variant file writing
// ---------------------------------------------------------------------------

/// Write alternative variant files to fleet/variants/ directory.
fn write_variant_files(merged: &InspectionSnapshot, render_dir: &Path) -> Result<()> {
    let variants_dir = render_dir.join("fleet").join("variants");

    // Walk all sections and write Alternative items to variant files

    // Config files
    if let Some(config) = &merged.config {
        for file in &config.files {
            if file.variant_selection == VariantSelection::Alternative {
                write_variant_file(&variants_dir, &file.path, "conf", &file.content)?;
            }
        }
    }

    // Systemd drop-ins
    if let Some(services) = &merged.services {
        for dropin in &services.drop_ins {
            if dropin.variant_selection == VariantSelection::Alternative {
                write_variant_file(&variants_dir, &dropin.path, "conf", &dropin.content)?;
            }
        }
    }

    // Quadlet units
    if let Some(containers) = &merged.containers {
        for unit in &containers.quadlet_units {
            if unit.variant_selection == VariantSelection::Alternative {
                // Extract extension from path
                let ext = Path::new(&unit.path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("container");
                write_variant_file(&variants_dir, &unit.path, ext, &unit.content)?;
            }
        }

        // Compose files (serialize images to JSON)
        for compose in &containers.compose_files {
            if compose.variant_selection == VariantSelection::Alternative {
                let json_content = serde_json::to_string_pretty(&compose.images)
                    .context("failed to serialize compose images")?;
                write_variant_file(&variants_dir, &compose.path, "json", &json_content)?;
            }
        }
    }

    Ok(())
}

/// Write a single variant file with 8-char hash prefix.
fn write_variant_file(
    variants_dir: &Path,
    item_path: &str,
    extension: &str,
    content: &str,
) -> Result<()> {
    // Compute 8-char hash prefix
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let hash = hasher.finalize();
    let hash_hex = hash
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();
    let hash_prefix = hash_hex.chars().take(8).collect::<String>();

    // Create subdirectory matching the item path structure.
    // Strip leading '/' so that joining never escapes the render tree
    // (Path::join replaces the base when the rhs is absolute).
    let sanitized_path = item_path.trim_start_matches('/');
    let item_parent = Path::new(sanitized_path).parent().unwrap_or(Path::new(""));
    let target_dir = variants_dir.join(item_parent);
    std::fs::create_dir_all(&target_dir)?;

    // Write file with hash-prefixed name
    let filename = format!("{}.{}", hash_prefix, extension);
    let file_path = target_dir.join(filename);

    std::fs::write(&file_path, content)
        .with_context(|| format!("failed to write variant file {}", file_path.display()))?;

    Ok(())
}

/// Prepend a draft header to the rendered Containerfile.
fn prepend_containerfile_header(
    merged: &InspectionSnapshot,
    render_dir: &Path,
    _label: &str,
) -> Result<()> {
    let containerfile_path = render_dir.join("Containerfile");

    // Read existing Containerfile
    let existing_content =
        std::fs::read_to_string(&containerfile_path).context("failed to read Containerfile")?;

    // Build header
    let mut header = String::new();
    header.push_str("# Fleet Aggregate Containerfile\n");

    if let Some(fleet_meta) = &merged.fleet_meta {
        header.push_str(&format!(
            "# Generated from {} hosts\n",
            fleet_meta.host_count
        ));
    }

    // Baseline image reference
    if let Some(target_image) = &merged.target_image {
        header.push_str(&format!("# Baseline: {}\n", target_image.image_ref));
    }

    // Provisionality note
    if let Some(fleet_meta) = &merged.fleet_meta
        && fleet_meta.baseline_provisional
    {
        header.push_str("# NOTE: Baseline selection is provisional (multiple images detected)\n");
    }

    header.push('\n');

    // Write combined content
    let combined = format!("{}{}", header, existing_content);
    std::fs::write(&containerfile_path, combined)
        .context("failed to write Containerfile with header")?;

    Ok(())
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
                format!("schema version mismatch: {:?}", versions)
            }
            FleetValidationError::DuplicateHostname { hostname } => {
                format!("duplicate hostname: {hostname}")
            }
            FleetValidationError::ArchitectureMismatch { architectures } => {
                format!("architecture mismatch: {}", architectures.join(", "))
            }
            FleetValidationError::EmptySnapshot { hostname } => {
                format!("empty snapshot: {hostname}")
            }
            FleetValidationError::OsMajorVersionMismatch { versions } => {
                format!("OS major version mismatch: {}", versions.join(", "))
            }
        })
        .collect();

    anyhow::anyhow!("fleet validation failed:\n  {}", msgs.join("\n  "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variant_file_stays_under_render_tree() {
        let dir = tempfile::tempdir().unwrap();
        let variants_dir = dir.path().join("fleet").join("variants");

        write_variant_file(
            &variants_dir,
            "/etc/httpd/conf/httpd.conf",
            "conf",
            "ServerRoot /etc/httpd",
        )
        .unwrap();

        // Must land under fleet/variants/etc/httpd/conf/
        let expected_parent = variants_dir.join("etc/httpd/conf");
        assert!(
            expected_parent.exists(),
            "variant dir should be under render tree, not at host /etc/httpd/conf"
        );

        let entries: Vec<_> = std::fs::read_dir(&expected_parent).unwrap().collect();
        assert_eq!(entries.len(), 1, "exactly one variant file expected");
    }

    #[test]
    fn test_variant_file_relative_path_works() {
        let dir = tempfile::tempdir().unwrap();
        let variants_dir = dir.path().join("fleet").join("variants");

        write_variant_file(&variants_dir, "etc/foo.conf", "conf", "key=value").unwrap();

        let expected_parent = variants_dir.join("etc");
        assert!(
            expected_parent.exists(),
            "relative path should resolve under variants_dir"
        );
    }

    // -----------------------------------------------------------------------
    // Fleet init metadata extraction regression tests
    // -----------------------------------------------------------------------

    /// Build a .tar.gz containing a single `inspection-snapshot.json` with
    /// the given JSON value. Returns the path to the tarball.
    fn make_test_tarball(dir: &Path, name: &str, json: &serde_json::Value) -> PathBuf {
        let tarball_path = dir.join(name);
        let file = std::fs::File::create(&tarball_path).unwrap();
        let gz = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
        let mut builder = tar::Builder::new(gz);

        let json_bytes = serde_json::to_vec(json).unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_size(json_bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();

        builder
            .append_data(
                &mut header,
                "host-a/inspection-snapshot.json",
                &json_bytes[..],
            )
            .unwrap();
        builder.finish().unwrap();

        tarball_path
    }

    #[test]
    fn test_extract_metadata_reads_top_level_target_image() {
        // Build a snapshot JSON that mirrors real InspectionSnapshot
        // serialization: target_image is a top-level struct with image_ref,
        // NOT inside the meta HashMap.
        let snapshot_json = serde_json::json!({
            "schema_version": 17,
            "meta": {
                "hostname": "host-a.example.com"
            },
            "target_image": {
                "image_ref": "registry.redhat.io/rhel9/rhel-bootc:9.6",
                "strategy": "BootcStatus"
            }
        });

        let dir = tempfile::tempdir().unwrap();
        let tarball = make_test_tarball(dir.path(), "host-a.tar.gz", &snapshot_json);

        let meta = extract_tarball_metadata(&tarball).unwrap();

        assert_eq!(
            meta.target_image.as_deref(),
            Some("registry.redhat.io/rhel9/rhel-bootc:9.6"),
            "should read target_image.image_ref from top-level, not meta"
        );
    }

    #[test]
    fn test_extract_metadata_ignores_meta_target_image() {
        // If target_image only exists inside meta (old/wrong shape),
        // extraction should return None — not silently read the wrong path.
        let snapshot_json = serde_json::json!({
            "schema_version": 17,
            "meta": {
                "hostname": "host-b.example.com",
                "target_image": "registry.redhat.io/rhel9/rhel-bootc:9.4"
            }
        });

        let dir = tempfile::tempdir().unwrap();
        let tarball = make_test_tarball(dir.path(), "host-b.tar.gz", &snapshot_json);

        let meta = extract_tarball_metadata(&tarball).unwrap();

        assert_eq!(
            meta.target_image, None,
            "should NOT read target_image from meta HashMap"
        );
    }

    #[test]
    fn test_baseline_conflict_selects_most_common() {
        let dir = tempfile::tempdir().unwrap();

        let common_image = "registry.redhat.io/rhel9/rhel-bootc:9.6";
        let outlier_image = "registry.redhat.io/rhel9/rhel-bootc:9.4";

        // Two tarballs with the same baseline
        let json_common = serde_json::json!({
            "schema_version": 17,
            "meta": {"hostname": "host-1"},
            "target_image": {"image_ref": common_image, "strategy": "BootcStatus"}
        });
        // One tarball with a different baseline
        let json_outlier = serde_json::json!({
            "schema_version": 17,
            "meta": {"hostname": "host-3"},
            "target_image": {"image_ref": outlier_image, "strategy": "BootcStatus"}
        });

        let t1 = make_test_tarball(dir.path(), "host-1.tar.gz", &json_common);
        let t2 = make_test_tarball(dir.path(), "host-2.tar.gz", &json_common);
        let t3 = make_test_tarball(dir.path(), "host-3.tar.gz", &json_outlier);

        // Extract metadata from all three
        let meta_list: Vec<TarballMetadata> = [t1, t2, t3]
            .iter()
            .map(|p| extract_tarball_metadata(p).unwrap())
            .collect();

        // Replicate the conflict-resolution logic from run_init
        let mut image_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for meta in &meta_list {
            if let Some(img) = &meta.target_image {
                *image_counts.entry(img.clone()).or_insert(0) += 1;
            }
        }

        assert_eq!(
            image_counts.len(),
            2,
            "should detect two distinct baselines"
        );
        assert_eq!(image_counts[common_image], 2);
        assert_eq!(image_counts[outlier_image], 1);

        // Most-common wins
        let (winner, _) = image_counts.iter().max_by_key(|(_, c)| *c).unwrap();
        assert_eq!(
            winner, common_image,
            "conflict resolution should pick the most common baseline"
        );
    }

    // -----------------------------------------------------------------------
    // --json-only output matrix regression tests
    // -----------------------------------------------------------------------

    /// Helper: build FleetAggregateArgs with specific output flags.
    fn make_aggregate_args(
        output_file: Option<PathBuf>,
        output_dir: Option<PathBuf>,
        json_only: bool,
    ) -> FleetAggregateArgs {
        FleetAggregateArgs {
            inputs: vec![],
            manifest: None,
            baseline: None,
            output_dir,
            output_file,
            json_only,
            strict: false,
            verbose: false,
        }
    }

    #[test]
    fn test_output_file_and_output_dir_conflict() {
        let args = make_aggregate_args(
            Some(PathBuf::from("/tmp/out.tar.gz")),
            Some(PathBuf::from("/tmp/outdir")),
            false,
        );
        let result = run_aggregate(&args);
        assert!(result.is_err(), "should reject conflicting output flags");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("mutually exclusive"),
            "error should mention mutual exclusivity, got: {msg}"
        );
    }

    /// Build two valid fleet-ready tarballs in `dir`. Each snapshot has
    /// an `os_release` section so it passes the non-empty check, and uses
    /// distinct hostnames to avoid the duplicate-hostname error.
    fn make_fleet_pair(dir: &Path) -> (PathBuf, PathBuf) {
        let json_a = serde_json::json!({
            "schema_version": 17,
            "meta": {"hostname": "host-a.example.com"},
            "os_release": {"name": "RHEL", "version_id": "9.6", "id": "rhel"},
            "target_image": {"image_ref": "registry.example.com/img:1", "strategy": "bootc-status"}
        });
        let json_b = serde_json::json!({
            "schema_version": 17,
            "meta": {"hostname": "host-b.example.com"},
            "os_release": {"name": "RHEL", "version_id": "9.6", "id": "rhel"},
            "target_image": {"image_ref": "registry.example.com/img:1", "strategy": "bootc-status"}
        });
        let a = make_test_tarball(dir, "host-a.tar.gz", &json_a);
        let b = make_test_tarball(dir, "host-b.tar.gz", &json_b);
        (a, b)
    }

    #[test]
    fn test_json_only_with_output_dir_writes_to_dir() {
        let dir = tempfile::tempdir().unwrap();
        let (t1, t2) = make_fleet_pair(dir.path());

        let out_dir = dir.path().join("json-output");
        let args = FleetAggregateArgs {
            inputs: vec![t1, t2],
            manifest: None,
            baseline: None,
            output_dir: Some(out_dir.clone()),
            output_file: None,
            json_only: true,
            strict: false,
            verbose: false,
        };

        run_aggregate(&args).expect("--json-only --output-dir should succeed");

        let expected = out_dir.join("inspection-snapshot.json");
        assert!(
            expected.exists(),
            "should write inspection-snapshot.json to output dir"
        );

        // Verify it's valid JSON
        let content = std::fs::read_to_string(&expected).unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(&content).expect("output should be valid JSON");
        assert!(parsed.is_object(), "parsed JSON should be an object");
    }

    #[test]
    fn test_json_only_with_output_file_writes_to_file() {
        let dir = tempfile::tempdir().unwrap();
        let (t1, t2) = make_fleet_pair(dir.path());

        let out_file = dir.path().join("custom-output.json");
        let args = FleetAggregateArgs {
            inputs: vec![t1, t2],
            manifest: None,
            baseline: None,
            output_dir: None,
            output_file: Some(out_file.clone()),
            json_only: true,
            strict: false,
            verbose: false,
        };

        run_aggregate(&args).expect("--json-only --output-file should succeed");

        assert!(
            out_file.exists(),
            "should write to the specified output file"
        );

        let content = std::fs::read_to_string(&out_file).unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(&content).expect("output should be valid JSON");
        assert!(parsed.is_object(), "parsed JSON should be an object");
    }
}
