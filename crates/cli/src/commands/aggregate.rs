//! `inspectah aggregate` top-level command.
//!
//! Combines multiple host scan tarballs into a single aggregate snapshot.

use anyhow::{Context, Result, bail};
use clap::Args;
use std::path::{Path, PathBuf};

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
}

/// Entry point for `inspectah aggregate`.
pub fn run_aggregate_command(args: &AggregateArgs) -> Result<()> {
    run_aggregate(args)
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
    let (tarball_paths, label) = resolve_inputs(args)?;

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

    let (merged, warnings) = merge_snapshots(snapshots, Some(&label), args.target_image.as_deref())
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
// Input resolution
// ---------------------------------------------------------------------------

/// Resolve CLI arguments into a list of tarball paths and a label.
fn resolve_inputs(args: &AggregateArgs) -> Result<(Vec<PathBuf>, String)> {
    // Mode 1: Single directory input
    if args.inputs.len() == 1 && args.inputs[0].is_dir() {
        let dir = &args.inputs[0];
        let label = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("aggregate")
            .to_string();

        let mut paths = list_tarballs_in_dir(dir)?;
        paths.sort();

        return Ok((paths, label));
    }

    // Mode 2: Multiple explicit tarball paths
    if !args.inputs.is_empty() {
        let label = "aggregate".to_string();
        let paths = args.inputs.clone();

        return Ok((paths, label));
    }

    bail!("no inputs specified — provide tarball paths or a directory");
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
        header.push_str(&format!(
            "# Merged from {} hosts\n",
            aggregate_meta.host_count
        ));
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
            target_image: None,
            output_dir,
            output_file,
            json_only,
            strict: false,
            verbose: false,
            ack_sensitive: false,
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
            "schema_version": 20,
            "meta": {"hostname": "host-a.example.com"},
            "os_release": {"name": "RHEL", "version_id": "9.6", "id": "rhel"},
            "target_image": {"image_ref": "registry.example.com/img:1", "strategy": "bootc-status"}
        });
        let json_b = serde_json::json!({
            "schema_version": 20,
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

            target_image: None,
            output_dir: Some(out_dir.clone()),
            output_file: None,
            json_only: true,
            strict: false,
            verbose: false,
            ack_sensitive: false,
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

            target_image: None,
            output_dir: None,
            output_file: Some(out_file.clone()),
            json_only: true,
            strict: false,
            verbose: false,
            ack_sensitive: false,
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
            "schema_version": 20,
            "meta": {"hostname": "host-normal.example.com"},
            "os_release": {"name": "RHEL", "version_id": "9.6", "id": "rhel"},
            "target_image": {"image_ref": "registry.example.com/img:1", "strategy": "bootc-status"},
            "sensitive_snapshot": false
        });

        let json_sensitive = serde_json::json!({
            "schema_version": 20,
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

            target_image: None,
            output_dir: None,
            output_file: None,
            json_only: false,
            strict: false,
            verbose: false,
            ack_sensitive: false,
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
            "schema_version": 20,
            "meta": {"hostname": "host-a.example.com"},
            "os_release": {"name": "RHEL", "version_id": "9.6", "id": "rhel"},
            "target_image": {"image_ref": "registry.example.com/img:1", "strategy": "bootc-status"},
            "sensitive_snapshot": true,
            "preserved_subscription": true,
            "preserved_credentials": false,
            "preserved_ssh_keys": false
        });

        let json_b = serde_json::json!({
            "schema_version": 20,
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

            target_image: None,
            output_dir: Some(out_dir.clone()),
            output_file: None,
            json_only: false,
            strict: false,
            verbose: false,
            ack_sensitive: true,
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
