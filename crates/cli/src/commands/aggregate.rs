//! `inspectah aggregate` top-level command.
//!
//! Combines multiple host scan tarballs into a single aggregate snapshot.
//! Subcommand:
//! - `aggregate init` — generate an aggregate manifest from a directory of tarballs

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use std::path::{Path, PathBuf};

use inspectah_core::aggregate::manifest::AggregateManifest;
use inspectah_core::aggregate::merge_snapshots;
use inspectah_core::aggregate::validate::{AggregateValidationError, AggregateWarning};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::redaction::RedactionState;
use inspectah_pipeline::render;
use inspectah_pipeline::render::tarball::{create_tarball, get_output_stamp};

#[derive(Debug, Args)]
pub struct AggregateArgs {
    /// Input tarballs or directory containing tarballs
    pub inputs: Vec<PathBuf>,

    /// Path to an aggregate manifest (TOML) specifying sources
    #[arg(long)]
    pub manifest: Option<PathBuf>,

    /// Override the target image reference for baseline comparison
    #[arg(long)]
    pub target_image: Option<String>,

    /// Output directory for the aggregate tarball
    #[arg(long)]
    pub output_dir: Option<PathBuf>,

    /// Output file path for the aggregate tarball
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

    /// Acknowledge that the merged output may contain sensitive data (subscription certs, password hashes, SSH keys)
    #[arg(long = "ack-sensitive", visible_alias = "acknowledge-sensitive")]
    pub ack_sensitive: bool,

    #[command(subcommand)]
    pub subcommand: Option<AggregateSubcommand>,
}

#[derive(Debug, Subcommand)]
pub enum AggregateSubcommand {
    /// Generate an aggregate manifest from a directory of tarballs
    Init(AggregateInitArgs),
}

#[derive(Debug, Args)]
pub struct AggregateInitArgs {
    /// Directory containing host tarballs
    pub directory: PathBuf,

    /// Output path for the generated manifest
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Overwrite an existing manifest file
    #[arg(long)]
    pub overwrite: bool,
}

/// Entry point for `inspectah aggregate`.
pub fn run_aggregate_command(args: &AggregateArgs) -> Result<()> {
    match &args.subcommand {
        Some(AggregateSubcommand::Init(init)) => run_init(init),
        None => run_aggregate(args),
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

fn run_aggregate(args: &AggregateArgs) -> Result<()> {
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

    // --- Step 3.5: Check for sensitive data in input snapshots ---
    let has_sensitive = snapshots.iter().any(|s| s.sensitive_snapshot);

    if has_sensitive && !args.ack_sensitive {
        // Collect which types of sensitive data are present
        let mut sensitive_types = std::collections::HashSet::new();
        for snapshot in &snapshots {
            if snapshot.preserved_subscription {
                sensitive_types.insert("subscription certs");
            }
            if snapshot.preserved_credentials {
                sensitive_types.insert("password hashes");
            }
            if snapshot.preserved_ssh_keys {
                sensitive_types.insert("SSH keys");
            }
            if snapshot.redaction_state == Some(RedactionState::Raw) {
                sensitive_types.insert("unredacted secrets");
            }
        }

        let type_list: Vec<&str> = sensitive_types.into_iter().collect();
        let type_list_str = type_list.join(", ");

        bail!(
            "Aggregate contains snapshots with sensitive data ({}).\n\
             To export, re-run with --ack-sensitive",
            type_list_str
        );
    }

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

    // --- Step 6.5: Prepend Containerfile header ---
    prepend_containerfile_header(&merged, render_dir.path(), &label)?;

    // --- Step 7: Create tarball ---
    let stamp = get_output_stamp(&format!("aggregate-{label}"));
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
    let aggregate_meta = merged.aggregate_meta.as_ref();
    let host_count = aggregate_meta.map_or(0, |m| m.host_count);
    let pkg_count = merged.rpm.as_ref().map_or(0, |r| r.packages_added.len());
    let config_count = merged.config.as_ref().map_or(0, |c| c.files.len());
    let svc_count = merged
        .services
        .as_ref()
        .map_or(0, |s| s.state_changes.len());

    eprintln!("Aggregate: {label} ({host_count} hosts)");

    // Report baseline provenance
    if let Some(target_image) = &merged.target_image {
        let provenance = if let Some(meta) = aggregate_meta
            && meta.baseline_provisional
        {
            "provisional"
        } else {
            "unanimous"
        };
        eprintln!("Baseline ({}): {}", provenance, target_image.image_ref);
    }

    eprintln!("Merged: {pkg_count} packages, {config_count} config files, {svc_count} services");

    if args.verbose
        && let Some(meta) = aggregate_meta
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

fn run_init(args: &AggregateInitArgs) -> Result<()> {
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
        .unwrap_or_else(|| PathBuf::from("aggregate.toml"));

    // --- Step 5: Check for existing file ---
    if output_path.exists() && !args.overwrite {
        bail!(
            "{} already exists (use --overwrite to replace)",
            output_path.display()
        );
    }

    // --- Step 6: Detect target image conflicts ---
    let mut image_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for meta in &metadata_list {
        if let Some(img) = &meta.target_image {
            *image_counts.entry(img.clone()).or_insert(0) += 1;
        }
    }

    let target_image = if image_counts.is_empty() {
        None
    } else {
        // Pick the most common image. For deterministic tie-breaking when
        // multiple images have equal prevalence, sort by count (descending)
        // then by image ref (lexicographically ascending).
        let mut sorted_images: Vec<(String, usize)> = image_counts.into_iter().collect();
        sorted_images.sort_by(|(ref_a, count_a), (ref_b, count_b)| {
            count_b.cmp(count_a).then_with(|| ref_a.cmp(ref_b))
        });
        let (most_common, _count) = &sorted_images[0];

        // Warn if there are conflicts
        if sorted_images.len() > 1 {
            let dist: Vec<String> = sorted_images
                .iter()
                .map(|(img, count)| format!("{img} ({count})"))
                .collect();
            eprintln!(
                "warning: target image conflict: selected {} from [{}]",
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
        .unwrap_or("aggregate");

    let toml = generate_manifest_toml(label, target_image.as_deref(), &sources);

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
        target_image
            .as_ref()
            .map_or(String::new(), |b| format!(", target_image: {b}"))
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
fn generate_manifest_toml(label: &str, target_image: Option<&str>, sources: &[PathBuf]) -> String {
    let mut toml = String::new();

    toml.push_str("# inspectah aggregate manifest\n");
    toml.push_str("# Edit label and target_image as needed. Sources are relative to this file.\n\n");

    toml.push_str(&format!("label = \"{label}\"\n"));

    if let Some(b) = target_image {
        toml.push_str(&format!("target_image = \"{b}\"\n"));
    } else {
        toml.push_str("# target_image = \"registry.redhat.io/rhel9/rhel-bootc:9.6\"\n");
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
    args: &AggregateArgs,
) -> Result<(Vec<PathBuf>, String, Option<AggregateManifest>)> {
    // Mutual exclusion: --manifest and positional inputs
    if args.manifest.is_some() && !args.inputs.is_empty() {
        bail!("cannot specify both --manifest and positional input paths");
    }

    // Mode 1: Manifest-driven
    if let Some(manifest_path) = &args.manifest {
        let mut manifest = AggregateManifest::load(manifest_path).map_err(|e| {
            anyhow::anyhow!(
                "failed to load manifest from {}: {e}",
                manifest_path.display()
            )
        })?;

        // CLI --target-image overrides manifest target_image
        if let Some(target_image) = &args.target_image {
            manifest.target_image = Some(target_image.clone());
        }

        let label = manifest.label.clone().unwrap_or_else(|| "aggregate".into());
        let paths = manifest.sources.clone();

        return Ok((paths, label, Some(manifest)));
    }

    // Mode 2: Single directory input
    if args.inputs.len() == 1 && args.inputs[0].is_dir() {
        let dir = &args.inputs[0];
        let label = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("aggregate")
            .to_string();

        let mut paths = list_tarballs_in_dir(dir)?;
        paths.sort();

        let manifest = build_manifest_from_args(&label, &paths, args);
        return Ok((paths, label, Some(manifest)));
    }

    // Mode 3: Multiple explicit tarball paths
    if !args.inputs.is_empty() {
        let label = "aggregate".to_string();
        let paths = args.inputs.clone();

        let manifest = build_manifest_from_args(&label, &paths, args);
        return Ok((paths, label, Some(manifest)));
    }

    bail!("no inputs specified — provide tarball paths, a directory, or --manifest");
}

/// Build an AggregateManifest from CLI arguments (for non-manifest modes).
fn build_manifest_from_args(
    label: &str,
    paths: &[PathBuf],
    args: &AggregateArgs,
) -> AggregateManifest {
    AggregateManifest {
        label: Some(label.to_string()),
        target_image: args.target_image.clone(),
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
            let mut snapshot = InspectionSnapshot::load(&json)
                .map_err(|e| anyhow::anyhow!("failed to parse snapshot: {e}"))?;

            // Normalize: Raw redaction state always implies sensitive data
            if matches!(snapshot.redaction_state, Some(RedactionState::Raw)) {
                snapshot.sensitive_snapshot = true;
            }

            return Ok(snapshot);
        }
    }

    bail!(
        "no inspection-snapshot.json found in {}",
        tarball_path.display()
    )
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
    header.push_str("# DRAFT — Aggregate Containerfile\n");
    header.push_str("# Requires human review before use\n");

    if let Some(aggregate_meta) = &merged.aggregate_meta {
        header.push_str(&format!("# Merged from {} hosts\n", aggregate_meta.host_count));
    }

    // Baseline image reference
    if let Some(target_image) = &merged.target_image {
        header.push_str(&format!("# Baseline: {}\n", target_image.image_ref));
    }

    // Provisionality note
    if let Some(aggregate_meta) = &merged.aggregate_meta
        && aggregate_meta.baseline_provisional
    {
        header
            .push_str("# NOTE: Baseline selection is provisional — multiple target images were\n");
        header
            .push_str("#        detected across hosts. Verify the selected baseline is correct.\n");
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

fn format_warning(w: &AggregateWarning) -> String {
    match w {
        AggregateWarning::StaleScanDates { spread_description } => {
            format!("stale scan dates: {spread_description}")
        }
        AggregateWarning::BaselineConflict {
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
        AggregateWarning::MinorVersionSpread { versions } => {
            format!("minor version spread: {}", versions.join(", "))
        }
        AggregateWarning::SystemTypeMismatch { types } => {
            format!("system type mismatch: {}", types.join(", "))
        }
    }
}

fn format_validation_errors(errors: &[AggregateValidationError]) -> anyhow::Error {
    let msgs: Vec<String> = errors
        .iter()
        .map(|e| match e {
            AggregateValidationError::TooFewSnapshots { count } => {
                format!("too few snapshots: {count} (need at least 2)")
            }
            AggregateValidationError::SchemaVersionMismatch { versions } => {
                format!("schema version mismatch: {:?}", versions)
            }
            AggregateValidationError::DuplicateHostname { hostname } => {
                format!("duplicate hostname: {hostname}")
            }
            AggregateValidationError::ArchitectureMismatch { architectures } => {
                format!("architecture mismatch: {}", architectures.join(", "))
            }
            AggregateValidationError::EmptySnapshot { hostname } => {
                format!("empty snapshot: {hostname}")
            }
            AggregateValidationError::OsMajorVersionMismatch { versions } => {
                format!("OS major version mismatch: {}", versions.join(", "))
            }
        })
        .collect();

    anyhow::anyhow!("aggregate validation failed:\n  {}", msgs.join("\n  "))
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Aggregate init metadata extraction regression tests
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
            "schema_version": 19,
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
            "schema_version": 19,
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
    fn test_target_image_conflict_selects_most_common() {
        let dir = tempfile::tempdir().unwrap();

        let common_image = "registry.redhat.io/rhel9/rhel-bootc:9.6";
        let outlier_image = "registry.redhat.io/rhel9/rhel-bootc:9.4";

        // Two tarballs with the same target image
        let json_common = serde_json::json!({
            "schema_version": 19,
            "meta": {"hostname": "host-1"},
            "target_image": {"image_ref": common_image, "strategy": "BootcStatus"}
        });
        // One tarball with a different target image
        let json_outlier = serde_json::json!({
            "schema_version": 19,
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
            "should detect two distinct target images"
        );
        assert_eq!(image_counts[common_image], 2);
        assert_eq!(image_counts[outlier_image], 1);

        // Most-common wins
        let (winner, _) = image_counts.iter().max_by_key(|(_, c)| *c).unwrap();
        assert_eq!(
            winner, common_image,
            "conflict resolution should pick the most common target image"
        );
    }

    #[test]
    fn test_aggregate_init_target_image_tie_break_is_deterministic() {
        // Command-boundary regression test: when two images have equal
        // prevalence (1 host each), the generated aggregate.toml must contain
        // the lexicographically earlier image ref as the target_image.
        let dir = tempfile::tempdir().unwrap();
        let tarballs_dir = dir.path().join("tarballs");
        std::fs::create_dir_all(&tarballs_dir).unwrap();

        // Two images, each appearing exactly once (tie).
        // Lexicographically: "alpha:1.0" < "beta:1.0"
        let alpha_image = "registry.example.com/alpha:1.0";
        let beta_image = "registry.example.com/beta:1.0";

        let json_alpha = serde_json::json!({
            "schema_version": 19,
            "meta": {"hostname": "host-alpha"},
            "target_image": {"image_ref": alpha_image, "strategy": "BootcStatus"}
        });
        let json_beta = serde_json::json!({
            "schema_version": 19,
            "meta": {"hostname": "host-beta"},
            "target_image": {"image_ref": beta_image, "strategy": "BootcStatus"}
        });

        make_test_tarball(&tarballs_dir, "host-alpha.tar.gz", &json_alpha);
        make_test_tarball(&tarballs_dir, "host-beta.tar.gz", &json_beta);

        // Run the full init flow via run_init
        let output_path = dir.path().join("aggregate.toml");
        let args = AggregateInitArgs {
            directory: tarballs_dir,
            output: Some(output_path.clone()),
            overwrite: false,
        };

        run_init(&args).expect("aggregate init should succeed");

        // Read and verify the generated manifest
        let toml_content =
            std::fs::read_to_string(&output_path).expect("aggregate.toml should exist after init");

        assert!(
            toml_content.contains(&format!("target_image = \"{alpha_image}\"")),
            "tie-break should select lexicographically earlier image ref (alpha < beta), got:\n{}",
            toml_content
        );
    }

    // -----------------------------------------------------------------------
    // --json-only output matrix regression tests
    // -----------------------------------------------------------------------

    /// Helper: build AggregateArgs with specific output flags.
    fn make_aggregate_args(
        output_file: Option<PathBuf>,
        output_dir: Option<PathBuf>,
        json_only: bool,
    ) -> AggregateArgs {
        AggregateArgs {
            inputs: vec![],
            manifest: None,
            target_image: None,
            output_dir,
            output_file,
            json_only,
            strict: false,
            verbose: false,
            ack_sensitive: false,
            subcommand: None,
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

    /// Build two valid aggregate-ready tarballs in `dir`. Each snapshot has
    /// an `os_release` section so it passes the non-empty check, and uses
    /// distinct hostnames to avoid the duplicate-hostname error.
    fn make_aggregate_pair(dir: &Path) -> (PathBuf, PathBuf) {
        let json_a = serde_json::json!({
            "schema_version": 19,
            "meta": {"hostname": "host-a.example.com"},
            "os_release": {"name": "RHEL", "version_id": "9.6", "id": "rhel"},
            "target_image": {"image_ref": "registry.example.com/img:1", "strategy": "bootc-status"}
        });
        let json_b = serde_json::json!({
            "schema_version": 19,
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
        let (t1, t2) = make_aggregate_pair(dir.path());

        let out_dir = dir.path().join("json-output");
        let args = AggregateArgs {
            inputs: vec![t1, t2],
            manifest: None,
            target_image: None,
            output_dir: Some(out_dir.clone()),
            output_file: None,
            json_only: true,
            strict: false,
            verbose: false,
            ack_sensitive: false,
            subcommand: None,
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
        let (t1, t2) = make_aggregate_pair(dir.path());

        let out_file = dir.path().join("custom-output.json");
        let args = AggregateArgs {
            inputs: vec![t1, t2],
            manifest: None,
            target_image: None,
            output_dir: None,
            output_file: Some(out_file.clone()),
            json_only: true,
            strict: false,
            verbose: false,
            ack_sensitive: false,
            subcommand: None,
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

    // -----------------------------------------------------------------------
    // --ack-sensitive export gate tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_aggregate_refuses_sensitive_snapshot_without_ack() {
        let dir = tempfile::tempdir().unwrap();

        // Create one normal snapshot and one sensitive snapshot
        let json_normal = serde_json::json!({
            "schema_version": 19,
            "meta": {"hostname": "host-normal.example.com"},
            "os_release": {"name": "RHEL", "version_id": "9.6", "id": "rhel"},
            "target_image": {"image_ref": "registry.example.com/img:1", "strategy": "bootc-status"},
            "sensitive_snapshot": false
        });

        let json_sensitive = serde_json::json!({
            "schema_version": 19,
            "meta": {"hostname": "host-sensitive.example.com"},
            "os_release": {"name": "RHEL", "version_id": "9.6", "id": "rhel"},
            "target_image": {"image_ref": "registry.example.com/img:1", "strategy": "bootc-status"},
            "sensitive_snapshot": true,
            "preserved_subscription": true,
            "preserved_credentials": false,
            "preserved_ssh_keys": false
        });

        let t1 = make_test_tarball(dir.path(), "host-normal.tar.gz", &json_normal);
        let t2 = make_test_tarball(dir.path(), "host-sensitive.tar.gz", &json_sensitive);

        let args = AggregateArgs {
            inputs: vec![t1, t2],
            manifest: None,
            target_image: None,
            output_dir: None,
            output_file: None,
            json_only: false,
            strict: false,
            verbose: false,
            ack_sensitive: false,
            subcommand: None,
        };

        let result = run_aggregate(&args);
        assert!(
            result.is_err(),
            "should refuse to export sensitive data without --ack-sensitive"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("sensitive data"),
            "error should mention sensitive data, got: {err_msg}"
        );
        assert!(
            err_msg.contains("subscription certs"),
            "error should list subscription certs as present, got: {err_msg}"
        );
        assert!(
            err_msg.contains("--ack-sensitive"),
            "error should instruct to use --ack-sensitive, got: {err_msg}"
        );
    }

    #[test]
    fn test_aggregate_allows_sensitive_snapshot_with_ack() {
        let dir = tempfile::tempdir().unwrap();

        let json_sensitive = serde_json::json!({
            "schema_version": 19,
            "meta": {"hostname": "host-a.example.com"},
            "os_release": {"name": "RHEL", "version_id": "9.6", "id": "rhel"},
            "target_image": {"image_ref": "registry.example.com/img:1", "strategy": "bootc-status"},
            "sensitive_snapshot": true,
            "preserved_subscription": true,
            "preserved_credentials": false,
            "preserved_ssh_keys": false
        });

        let json_b = serde_json::json!({
            "schema_version": 19,
            "meta": {"hostname": "host-b.example.com"},
            "os_release": {"name": "RHEL", "version_id": "9.6", "id": "rhel"},
            "target_image": {"image_ref": "registry.example.com/img:1", "strategy": "bootc-status"},
            "sensitive_snapshot": false
        });

        let t1 = make_test_tarball(dir.path(), "host-a.tar.gz", &json_sensitive);
        let t2 = make_test_tarball(dir.path(), "host-b.tar.gz", &json_b);

        let out_dir = dir.path().join("output");
        let args = AggregateArgs {
            inputs: vec![t1, t2],
            manifest: None,
            target_image: None,
            output_dir: Some(out_dir.clone()),
            output_file: None,
            json_only: false,
            strict: false,
            verbose: false,
            ack_sensitive: true,
            subcommand: None,
        };

        let result = run_aggregate(&args);
        assert!(
            result.is_ok(),
            "should allow export with --ack-sensitive: {:?}",
            result.err()
        );

        // Verify output was created
        let tarballs: Vec<_> = std::fs::read_dir(&out_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("gz"))
            .collect();
        assert_eq!(tarballs.len(), 1, "should create one tarball");
    }
}
