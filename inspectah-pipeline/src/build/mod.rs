//! Build planning and execution -- lives in pipeline, not CLI.
//!
//! CLI handles clap args and terminal output. This module handles:
//! - Tarball extraction with archive safety validation
//! - RHEL pass-through detection with ambient bundle validation
//! - Subscription mount planning
//! - Bundle completeness preflight
//! - Cert expiry checking
//! - Podman command construction
//! - Typed build outcome for exit code mapping
//!
//! `inspectah build` v1 accepts tarball input only. Edited-directory builds
//! use manual `podman build` from the extracted working directory, as
//! documented in the generated README.

pub mod extract;
pub mod rhel;

use anyhow::{Context, Result, bail};
use inspectah_core::types::subscription::{SubscriptionFile, match_entitlement_pairs};
use std::path::{Path, PathBuf};
use std::process::Command;

use self::extract::TarballExtractor;
use self::rhel::{AmbientSubscription, detect_ambient_subscription};

/// Typed build outcome -- encodes all exit conditions.
#[derive(Debug)]
pub enum BuildOutcome {
    /// Build succeeded. Includes image tag and digest.
    Success { tag: String, digest: Option<String> },
    /// Dry run -- command emitted, nothing executed.
    DryRun { command: String },
    /// Podman not found.
    PodmanNotFound,
    /// Podman build failed with exit code.
    PodmanFailed { exit_code: i32 },
    /// No subscription data available (not RHEL, no tarball bundle).
    NoSubscription,
    /// Preflight failed (missing Containerfile, invalid tarball, etc.)
    PreflightFailed { reason: String },
}

impl BuildOutcome {
    /// Map outcome to process exit code per spec contract.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Success { .. } | Self::DryRun { .. } => 0,
            Self::PodmanNotFound => 127,
            Self::PodmanFailed { exit_code } => *exit_code,
            Self::NoSubscription | Self::PreflightFailed { .. } => 1,
        }
    }
}

/// Build-time warning.
#[derive(Debug)]
pub enum BuildWarning {
    CertExpiringSoon { days_remaining: i64, path: String },
    CertExpired { path: String },
    AmbientBundleIncomplete { reason: String },
    NoSubscriptionData,
}

/// Configuration for a build operation.
pub struct BuildConfig {
    pub tarball: PathBuf,
    pub tag: String,
    pub dry_run: bool,
    pub keep_context: bool,
    pub podman_args: Vec<String>,
}

/// Plan and optionally execute a build.
///
/// Returns the outcome and any warnings. The caller (CLI) is responsible
/// for rendering warnings and the outcome to the terminal.
pub fn plan_and_execute(config: &BuildConfig) -> Result<(BuildOutcome, Vec<BuildWarning>)> {
    let mut warnings = Vec::new();

    // Validate tag format.
    if !config.tag.contains(':') || config.tag.ends_with(':') {
        return Ok((
            BuildOutcome::PreflightFailed {
                reason: format!(
                    "tag must include a version: '{}:v1', not '{}'",
                    config.tag.split(':').next().unwrap_or(&config.tag),
                    config.tag
                ),
            },
            warnings,
        ));
    }

    // Check podman availability.
    let podman = match find_podman() {
        Some(p) => p,
        None => return Ok((BuildOutcome::PodmanNotFound, warnings)),
    };

    // Extract tarball with full safety validation.
    // Use tempfile::TempDir for automatic cleanup on all exit paths
    // (success, error, panic). Only --keep-context persists material.
    let temp_dir =
        tempfile::tempdir().context("failed to create temporary extraction directory")?;

    let extract_dir = if config.keep_context {
        // Move to named location so user can find it after build.
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("inspectah/builds");
        let named_dir = cache_dir.join(format!("build-{}", std::process::id()));

        // Fail fast if the directory exists and is non-empty.
        // This prevents reuse of directories that could contain attacker-placed symlinks.
        if named_dir.exists() {
            let is_empty = std::fs::read_dir(&named_dir)
                .context("failed to read keep-context directory")?
                .next()
                .is_none();

            if !is_empty {
                return Ok((
                    BuildOutcome::PreflightFailed {
                        reason: format!(
                            "extraction directory already exists and is non-empty: {}. \
                             Remove it first or omit --keep-context.",
                            named_dir.display()
                        ),
                    },
                    warnings,
                ));
            }
        } else {
            std::fs::create_dir_all(&named_dir)?;
        }

        named_dir
    } else {
        // TempDir owns the path -- Drop cleans it up automatically.
        temp_dir.path().to_path_buf()
    };

    let extractor = TarballExtractor::new(extract_dir.clone());
    if let Err(e) = extractor.extract(&config.tarball) {
        // temp_dir Drop fires here -- cleanup is automatic.
        return Ok((
            BuildOutcome::PreflightFailed {
                reason: format!("tarball extraction failed: {e}"),
            },
            warnings,
        ));
    }

    // Find Containerfile.
    let containerfile = extract_dir.join("Containerfile");
    if !containerfile.exists() {
        return Ok((
            BuildOutcome::PreflightFailed {
                reason: "no Containerfile found in tarball".into(),
            },
            warnings,
        ));
    }

    // Detect RHEL pass-through with ambient validation
    // (check FIRST -- skip tarball validation if RHEL).
    let ambient = detect_ambient_subscription();

    // Detect subscription material and validate bundle completeness.
    let sub_dir = extract_dir.join("subscription");
    let has_subscription = match &ambient {
        AmbientSubscription::Available => {
            // RHEL pass-through handles it -- skip tarball bundle validation.
            false
        }
        _ => {
            // Non-RHEL or incomplete ambient -- validate tarball bundle
            // with full four-component check.
            match validate_subscription_bundle(&sub_dir) {
                Ok(present) => present,
                Err(e) => {
                    return Ok((
                        BuildOutcome::PreflightFailed {
                            reason: e.to_string(),
                        },
                        warnings,
                    ));
                }
            }
        }
    };

    let use_subscription_mounts = match &ambient {
        AmbientSubscription::Available => false, // RHEL pass-through handles it
        AmbientSubscription::IncompleteBundle { reason } => {
            warnings.push(BuildWarning::AmbientBundleIncomplete {
                reason: reason.clone(),
            });
            has_subscription // fall back to tarball certs (already validated above)
        }
        AmbientSubscription::NotAvailable => has_subscription,
    };

    // Check cert expiry at build time (only for tarball-sourced certs).
    if has_subscription && ambient != AmbientSubscription::Available {
        check_cert_expiry_at_build(&sub_dir, &mut warnings);
    }

    if !has_subscription && ambient == AmbientSubscription::NotAvailable {
        warnings.push(BuildWarning::NoSubscriptionData);
    }

    // Build podman command.
    let mut cmd_args: Vec<String> = vec!["build".into()];
    cmd_args.push("-t".into());
    cmd_args.push(config.tag.clone());

    if use_subscription_mounts {
        let ent_path = sub_dir.join("entitlement");
        let rhsm_path = sub_dir.join("rhsm");
        let repo_path = sub_dir.join("redhat.repo");

        cmd_args.push("-v".into());
        cmd_args.push(format!(
            "{}:/run/secrets/etc-pki-entitlement:z",
            ent_path.display()
        ));
        cmd_args.push("-v".into());
        cmd_args.push(format!("{}:/run/secrets/rhsm:z", rhsm_path.display()));
        if repo_path.exists() {
            cmd_args.push("-v".into());
            cmd_args.push(format!(
                "{}:/run/secrets/redhat.repo:z",
                repo_path.display()
            ));
        }
    }

    cmd_args.extend(config.podman_args.clone());
    cmd_args.push("-f".into());
    cmd_args.push("Containerfile".into());
    cmd_args.push(".".into());

    if config.dry_run {
        let mut full_cmd = format!("cd {}\n{}", extract_dir.display(), podman);
        for arg in &cmd_args {
            if arg.contains(':') || arg.contains(' ') {
                full_cmd.push_str(&format!(" \\\n  {arg}"));
            } else {
                full_cmd.push_str(&format!(" {arg}"));
            }
        }
        return Ok((BuildOutcome::DryRun { command: full_cmd }, warnings));
    }

    // Execute podman build.
    let output = Command::new(&podman)
        .args(&cmd_args)
        .current_dir(&extract_dir)
        .output()
        .context("failed to execute podman")?;

    let exit_code = output.status.code().unwrap_or(1);

    if exit_code == 0 {
        // Try to extract image digest from podman output.
        let stdout = String::from_utf8_lossy(&output.stdout);
        let digest = stdout
            .lines()
            .last()
            .filter(|l| l.starts_with("sha256:") || l.len() == 64)
            .map(|l| {
                if l.starts_with("sha256:") {
                    l.to_string()
                } else {
                    format!("sha256:{l}")
                }
            });

        Ok((
            BuildOutcome::Success {
                tag: config.tag.clone(),
                digest,
            },
            warnings,
        ))
    } else {
        Ok((BuildOutcome::PodmanFailed { exit_code }, warnings))
    }
}

/// Validate that the extracted subscription bundle has all four required components.
///
/// Uses the SAME completeness rule as scan-side `evaluate_bundle_completeness()`:
/// 1. At least one serial-matched entitlement cert+key pair
/// 2. rhsm.conf present
/// 3. At least one CA cert
/// 4. redhat.repo present
///
/// Returns `Ok(true)` if bundle is complete and usable, `Err` if subscription
/// directory exists but is incomplete (hard error -- mount plan must NOT be emitted).
/// Returns `Ok(false)` if no subscription directory exists at all.
fn validate_subscription_bundle(sub_dir: &Path) -> Result<bool> {
    if !sub_dir.exists() {
        return Ok(false);
    }

    let ent_dir = sub_dir.join("entitlement");
    let rhsm_conf = sub_dir.join("rhsm/rhsm.conf");
    let ca_dir = sub_dir.join("rhsm/ca");
    let redhat_repo = sub_dir.join("redhat.repo");

    let mut missing = Vec::new();

    // 1. Serial-matched entitlement pair.
    if !ent_dir.exists() {
        missing.push("entitlement directory");
    } else {
        let files = collect_subscription_files(&ent_dir)?;
        let (pairs, _orphans) = match_entitlement_pairs(&files);
        if pairs.is_empty() {
            missing.push("serial-matched entitlement cert+key pair");
        }
    }

    // 2. rhsm.conf.
    if !rhsm_conf.exists() {
        missing.push("rhsm.conf");
    }

    // 3. CA certs.
    let has_ca = ca_dir.exists()
        && std::fs::read_dir(&ca_dir)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .any(|e| e.file_name().to_string_lossy().ends_with(".pem"))
            })
            .unwrap_or(false);
    if !has_ca {
        missing.push("CA certs");
    }

    // 4. redhat.repo.
    if !redhat_repo.exists() {
        missing.push("redhat.repo");
    }

    if !missing.is_empty() {
        bail!(
            "subscription bundle incomplete (missing: {}). \
             Mount plan will not be emitted.",
            missing.join(", ")
        );
    }

    Ok(true)
}

/// Collect .pem files from a directory into SubscriptionFile structs
/// for use with `match_entitlement_pairs`.
fn collect_subscription_files(dir: &Path) -> Result<Vec<SubscriptionFile>> {
    let mut files = Vec::new();
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("cannot read entitlement directory: {}", dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".pem") {
            files.push(SubscriptionFile {
                path: entry.path().to_string_lossy().to_string(),
                content: String::new(), // Content not needed for pairing.
                size_bytes: entry.metadata().map(|m| m.len()).unwrap_or(0),
                cert_expiry: None,
            });
        }
    }

    Ok(files)
}

/// Check cert expiry for entitlement certs in the subscription directory.
/// Emits warnings for certs expiring within 14 days or already expired.
fn check_cert_expiry_at_build(sub_dir: &Path, warnings: &mut Vec<BuildWarning>) {
    let ent_dir = sub_dir.join("entitlement");
    let entries = match std::fs::read_dir(&ent_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let now = time::OffsetDateTime::now_utc();

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // Only check cert files, not key files.
        if !name.ends_with(".pem") || name.ends_with("-key.pem") {
            continue;
        }

        let path_str = entry.path().to_string_lossy().to_string();

        // Try to read the cert and extract the expiry from PEM.
        // Simple heuristic: look for "Not After" in openssl-style output.
        // Since we don't have an x509 parser, check file mtime as a proxy,
        // or rely on the cert_expiry from the original scan data if available.
        // For now, check if the cert file is suspiciously small (likely invalid).
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        // Certs smaller than 100 bytes are almost certainly invalid.
        if metadata.len() < 100 {
            warnings.push(BuildWarning::CertExpired {
                path: path_str.clone(),
            });
            continue;
        }

        // If we have cert expiry data from the scan (embedded in the tarball),
        // check it. The subscription tarball may include a metadata.json with
        // expiry info. For now, use the 14-day window heuristic with file age.
        if let Ok(modified) = metadata.modified()
            && let Ok(duration) = modified.elapsed()
        {
            // If the cert file hasn't been modified in 14+ days, warn.
            // This is a rough heuristic; real expiry checking happens
            // when we have parsed x509 data from the scan.
            let days_old = duration.as_secs() / 86400;
            // Only warn if there's cert expiry metadata available.
            // Without x509 parsing, we can't determine actual expiry.
            let _ = (days_old, now); // Acknowledge usage for compiler.
        }
    }

    // Check for expiry metadata file if present.
    let metadata_path = sub_dir.join("metadata.json");
    if metadata_path.exists()
        && let Ok(content) = std::fs::read_to_string(&metadata_path)
    {
        check_expiry_from_metadata(&content, now, warnings);
    }
}

/// Parse cert expiry from subscription metadata JSON and emit warnings.
fn check_expiry_from_metadata(
    content: &str,
    now: time::OffsetDateTime,
    warnings: &mut Vec<BuildWarning>,
) {
    // The metadata.json may contain earliest_expiry as RFC3339.
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(content);
    let value = match parsed {
        Ok(v) => v,
        Err(_) => return,
    };

    if let Some(expiry_str) = value.get("earliest_expiry").and_then(|v| v.as_str()) {
        let format = time::format_description::well_known::Rfc3339;
        if let Ok(expiry) = time::OffsetDateTime::parse(expiry_str, &format) {
            let duration = expiry - now;
            let days_remaining = duration.whole_days();

            if days_remaining < 0 {
                warnings.push(BuildWarning::CertExpired {
                    path: "subscription/entitlement (from metadata)".into(),
                });
            } else if days_remaining <= 14 {
                warnings.push(BuildWarning::CertExpiringSoon {
                    days_remaining,
                    path: "subscription/entitlement (from metadata)".into(),
                });
            }
        }
    }
}

/// Find podman in PATH.
fn find_podman() -> Option<String> {
    which_command("podman")
}

/// Look up a command in PATH.
fn which_command(name: &str) -> Option<String> {
    Command::new("which")
        .arg(name)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Build a minimal tarball with a Containerfile for testing.
    fn build_test_tarball(
        dir: &Path,
        containerfile_content: &str,
        include_subscription: bool,
    ) -> PathBuf {
        let tarball_path = dir.join("test-output.tar.gz");
        let mut builder = tar::Builder::new(Vec::new());

        // Root directory.
        let mut dir_header = tar::Header::new_gnu();
        dir_header.set_entry_type(tar::EntryType::Directory);
        dir_header.set_path("output/").ok();
        dir_header.set_size(0);
        dir_header.set_mode(0o755);
        dir_header.set_cksum();
        builder.append(&dir_header, std::io::empty()).ok();

        // Containerfile.
        let cf_bytes = containerfile_content.as_bytes();
        let mut cf_header = tar::Header::new_gnu();
        cf_header.set_entry_type(tar::EntryType::Regular);
        cf_header.set_path("output/Containerfile").ok();
        cf_header.set_size(cf_bytes.len() as u64);
        cf_header.set_mode(0o644);
        cf_header.set_cksum();
        builder.append(&cf_header, cf_bytes).ok();

        if include_subscription {
            // subscription/entitlement/ directory.
            let mut ent_dir = tar::Header::new_gnu();
            ent_dir.set_entry_type(tar::EntryType::Directory);
            ent_dir.set_path("output/subscription/entitlement/").ok();
            ent_dir.set_size(0);
            ent_dir.set_mode(0o755);
            ent_dir.set_cksum();
            builder.append(&ent_dir, std::io::empty()).ok();

            // Cert file.
            let cert = b"-----BEGIN CERTIFICATE-----\nMIIDx...\n-----END CERTIFICATE-----";
            let mut cert_h = tar::Header::new_gnu();
            cert_h.set_entry_type(tar::EntryType::Regular);
            cert_h
                .set_path("output/subscription/entitlement/12345.pem")
                .ok();
            cert_h.set_size(cert.len() as u64);
            cert_h.set_mode(0o644);
            cert_h.set_cksum();
            builder.append(&cert_h, &cert[..]).ok();

            // Key file.
            let key = b"-----BEGIN RSA PRIVATE KEY-----\nMIIEp...\n-----END RSA PRIVATE KEY-----";
            let mut key_h = tar::Header::new_gnu();
            key_h.set_entry_type(tar::EntryType::Regular);
            key_h
                .set_path("output/subscription/entitlement/12345-key.pem")
                .ok();
            key_h.set_size(key.len() as u64);
            key_h.set_mode(0o600);
            key_h.set_cksum();
            builder.append(&key_h, &key[..]).ok();

            // rhsm directory.
            let mut rhsm_dir = tar::Header::new_gnu();
            rhsm_dir.set_entry_type(tar::EntryType::Directory);
            rhsm_dir.set_path("output/subscription/rhsm/").ok();
            rhsm_dir.set_size(0);
            rhsm_dir.set_mode(0o755);
            rhsm_dir.set_cksum();
            builder.append(&rhsm_dir, std::io::empty()).ok();

            // rhsm.conf.
            let conf = b"[rhsm]\nbaseurl = https://cdn.redhat.com";
            let mut conf_h = tar::Header::new_gnu();
            conf_h.set_entry_type(tar::EntryType::Regular);
            conf_h.set_path("output/subscription/rhsm/rhsm.conf").ok();
            conf_h.set_size(conf.len() as u64);
            conf_h.set_mode(0o644);
            conf_h.set_cksum();
            builder.append(&conf_h, &conf[..]).ok();

            // CA directory.
            let mut ca_dir = tar::Header::new_gnu();
            ca_dir.set_entry_type(tar::EntryType::Directory);
            ca_dir.set_path("output/subscription/rhsm/ca/").ok();
            ca_dir.set_size(0);
            ca_dir.set_mode(0o755);
            ca_dir.set_cksum();
            builder.append(&ca_dir, std::io::empty()).ok();

            // CA cert.
            let ca = b"-----BEGIN CERTIFICATE-----\nCA_CERT...\n-----END CERTIFICATE-----";
            let mut ca_h = tar::Header::new_gnu();
            ca_h.set_entry_type(tar::EntryType::Regular);
            ca_h.set_path("output/subscription/rhsm/ca/redhat-uep.pem")
                .ok();
            ca_h.set_size(ca.len() as u64);
            ca_h.set_mode(0o644);
            ca_h.set_cksum();
            builder.append(&ca_h, &ca[..]).ok();

            // redhat.repo.
            let repo = b"[rhel-base]\nname=RHEL Base\nbaseurl=https://cdn.redhat.com/...";
            let mut repo_h = tar::Header::new_gnu();
            repo_h.set_entry_type(tar::EntryType::Regular);
            repo_h.set_path("output/subscription/redhat.repo").ok();
            repo_h.set_size(repo.len() as u64);
            repo_h.set_mode(0o644);
            repo_h.set_cksum();
            builder.append(&repo_h, &repo[..]).ok();
        }

        let tar_bytes = builder.into_inner().unwrap_or_default();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar_bytes).ok();
        let gz_bytes = gz.finish().unwrap_or_default();
        std::fs::write(&tarball_path, &gz_bytes).ok();
        tarball_path
    }

    #[test]
    fn test_build_outcome_exit_codes() {
        assert_eq!(
            BuildOutcome::Success {
                tag: "img:v1".into(),
                digest: None
            }
            .exit_code(),
            0
        );
        assert_eq!(
            BuildOutcome::DryRun {
                command: "cmd".into()
            }
            .exit_code(),
            0
        );
        assert_eq!(BuildOutcome::PodmanNotFound.exit_code(), 127);
        assert_eq!(BuildOutcome::PodmanFailed { exit_code: 2 }.exit_code(), 2);
        assert_eq!(BuildOutcome::NoSubscription.exit_code(), 1);
        assert_eq!(
            BuildOutcome::PreflightFailed {
                reason: "bad".into()
            }
            .exit_code(),
            1
        );
    }

    #[test]
    fn test_invalid_tag_format() {
        let tmp = tempfile::tempdir().unwrap();
        let tarball = build_test_tarball(tmp.path(), "FROM ubi9", false);

        let config = BuildConfig {
            tarball,
            tag: "no-version".into(),
            dry_run: true,
            keep_context: false,
            podman_args: vec![],
        };

        let (outcome, _) = plan_and_execute(&config).unwrap();
        assert_eq!(outcome.exit_code(), 1);
        match outcome {
            BuildOutcome::PreflightFailed { reason } => {
                assert!(reason.contains("tag must include a version"), "{reason}");
            }
            other => panic!("expected PreflightFailed, got: {other:?}"),
        }
    }

    #[test]
    fn test_tag_trailing_colon() {
        let tmp = tempfile::tempdir().unwrap();
        let tarball = build_test_tarball(tmp.path(), "FROM ubi9", false);

        let config = BuildConfig {
            tarball,
            tag: "myimage:".into(),
            dry_run: true,
            keep_context: false,
            podman_args: vec![],
        };

        let (outcome, _) = plan_and_execute(&config).unwrap();
        match outcome {
            BuildOutcome::PreflightFailed { reason } => {
                assert!(reason.contains("tag must include a version"), "{reason}");
            }
            other => panic!("expected PreflightFailed, got: {other:?}"),
        }
    }

    #[test]
    fn test_validate_subscription_bundle_missing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let sub_dir = tmp.path().join("subscription");
        // No subscription directory at all -- returns Ok(false).
        assert_eq!(validate_subscription_bundle(&sub_dir).unwrap(), false);
    }

    #[test]
    fn test_validate_subscription_bundle_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let sub_dir = tmp.path().join("subscription");
        std::fs::create_dir_all(&sub_dir).unwrap();
        // Directory exists but empty -- hard error.
        let result = validate_subscription_bundle(&sub_dir);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("incomplete"), "{err}");
    }

    #[test]
    fn test_validate_subscription_bundle_complete() {
        let tmp = tempfile::tempdir().unwrap();
        let sub_dir = tmp.path().join("subscription");

        // Build complete bundle.
        let ent_dir = sub_dir.join("entitlement");
        let rhsm_dir = sub_dir.join("rhsm/ca");
        std::fs::create_dir_all(&ent_dir).unwrap();
        std::fs::create_dir_all(&rhsm_dir).unwrap();
        std::fs::write(ent_dir.join("999.pem"), "cert").unwrap();
        std::fs::write(ent_dir.join("999-key.pem"), "key").unwrap();
        std::fs::write(sub_dir.join("rhsm/rhsm.conf"), "conf").unwrap();
        std::fs::write(rhsm_dir.join("ca.pem"), "ca").unwrap();
        std::fs::write(sub_dir.join("redhat.repo"), "repo").unwrap();

        assert_eq!(validate_subscription_bundle(&sub_dir).unwrap(), true);
    }

    #[test]
    fn test_validate_subscription_bundle_missing_key() {
        let tmp = tempfile::tempdir().unwrap();
        let sub_dir = tmp.path().join("subscription");
        let ent_dir = sub_dir.join("entitlement");
        let rhsm_dir = sub_dir.join("rhsm/ca");
        std::fs::create_dir_all(&ent_dir).unwrap();
        std::fs::create_dir_all(&rhsm_dir).unwrap();
        // Cert without key -- no matched pair.
        std::fs::write(ent_dir.join("999.pem"), "cert").unwrap();
        std::fs::write(sub_dir.join("rhsm/rhsm.conf"), "conf").unwrap();
        std::fs::write(rhsm_dir.join("ca.pem"), "ca").unwrap();
        std::fs::write(sub_dir.join("redhat.repo"), "repo").unwrap();

        let result = validate_subscription_bundle(&sub_dir);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("serial-matched"));
    }

    #[test]
    fn test_validate_subscription_bundle_missing_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let sub_dir = tmp.path().join("subscription");
        let ent_dir = sub_dir.join("entitlement");
        let rhsm_dir = sub_dir.join("rhsm/ca");
        std::fs::create_dir_all(&ent_dir).unwrap();
        std::fs::create_dir_all(&rhsm_dir).unwrap();
        std::fs::write(ent_dir.join("999.pem"), "cert").unwrap();
        std::fs::write(ent_dir.join("999-key.pem"), "key").unwrap();
        std::fs::write(sub_dir.join("rhsm/rhsm.conf"), "conf").unwrap();
        std::fs::write(rhsm_dir.join("ca.pem"), "ca").unwrap();
        // Missing redhat.repo.

        let result = validate_subscription_bundle(&sub_dir);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("redhat.repo"));
    }

    #[test]
    fn test_check_expiry_from_metadata_expired() {
        let now = time::OffsetDateTime::now_utc();
        // Expiry 10 days ago.
        let expired = now - time::Duration::days(10);
        let format = time::format_description::well_known::Rfc3339;
        let expiry_str = expired.format(&format).unwrap();

        let json = format!(r#"{{"earliest_expiry": "{expiry_str}"}}"#);
        let mut warnings = Vec::new();
        check_expiry_from_metadata(&json, now, &mut warnings);
        assert_eq!(warnings.len(), 1);
        assert!(matches!(warnings[0], BuildWarning::CertExpired { .. }));
    }

    #[test]
    fn test_check_expiry_from_metadata_soon() {
        let now = time::OffsetDateTime::now_utc();
        // Expiry in 7 days.
        let soon = now + time::Duration::days(7);
        let format = time::format_description::well_known::Rfc3339;
        let expiry_str = soon.format(&format).unwrap();

        let json = format!(r#"{{"earliest_expiry": "{expiry_str}"}}"#);
        let mut warnings = Vec::new();
        check_expiry_from_metadata(&json, now, &mut warnings);
        assert_eq!(warnings.len(), 1);
        match &warnings[0] {
            BuildWarning::CertExpiringSoon {
                days_remaining,
                path: _,
            } => {
                assert!(*days_remaining <= 14 && *days_remaining >= 0);
            }
            other => panic!("expected CertExpiringSoon, got: {other:?}"),
        }
    }

    #[test]
    fn test_check_expiry_from_metadata_ok() {
        let now = time::OffsetDateTime::now_utc();
        // Expiry in 60 days -- no warning.
        let far = now + time::Duration::days(60);
        let format = time::format_description::well_known::Rfc3339;
        let expiry_str = far.format(&format).unwrap();

        let json = format!(r#"{{"earliest_expiry": "{expiry_str}"}}"#);
        let mut warnings = Vec::new();
        check_expiry_from_metadata(&json, now, &mut warnings);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_check_expiry_from_metadata_invalid_json() {
        let mut warnings = Vec::new();
        check_expiry_from_metadata("not json", time::OffsetDateTime::now_utc(), &mut warnings);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_collect_subscription_files_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let files = collect_subscription_files(tmp.path()).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn test_collect_subscription_files_pem() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("123.pem"), "cert-data").unwrap();
        std::fs::write(tmp.path().join("123-key.pem"), "key-data").unwrap();
        std::fs::write(tmp.path().join("readme.txt"), "not a pem").unwrap();

        let files = collect_subscription_files(tmp.path()).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| f.path.ends_with(".pem")));
    }

    #[test]
    fn test_dry_run_no_podman_args() {
        // Dry run skips podman execution, so we can test command construction.
        let tmp = tempfile::tempdir().unwrap();
        let tarball = build_test_tarball(tmp.path(), "FROM ubi9:latest", false);

        let config = BuildConfig {
            tarball,
            tag: "test:v1".into(),
            dry_run: true,
            keep_context: false,
            podman_args: vec![],
        };

        // On macOS, podman may not exist -- PodmanNotFound is acceptable.
        let result = plan_and_execute(&config);
        assert!(result.is_ok());
        let (outcome, _) = result.unwrap();
        match outcome {
            BuildOutcome::DryRun { command } => {
                assert!(command.contains("build"));
                assert!(command.contains("test:v1"));
                assert!(command.contains("Containerfile"));
            }
            BuildOutcome::PodmanNotFound => {
                // Acceptable on dev machines without podman.
            }
            other => panic!("expected DryRun or PodmanNotFound, got: {other:?}"),
        }
    }

    #[test]
    fn test_keep_context_fails_on_nonempty_dir() {
        // This test verifies the fail-fast behavior when --keep-context points
        // to an existing non-empty directory. Since we can't easily override
        // dirs::cache_dir() in tests, we verify the behavior by:
        // 1. Running a build with keep_context=true (creates the directory)
        // 2. Adding a file to that directory
        // 3. Running another build with keep_context=true (should fail)

        let tmp = tempfile::tempdir().unwrap();
        let tarball = build_test_tarball(tmp.path(), "FROM ubi9:latest", false);

        // Determine the keep-context directory that will be used.
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("inspectah/builds");
        let pid_dir = cache_dir.join(format!("build-{}", std::process::id()));

        // Clean up any existing directory from previous test runs.
        if pid_dir.exists() {
            std::fs::remove_dir_all(&pid_dir).ok();
        }

        // First build: should succeed (or fail for other reasons, but not "non-empty").
        let config1 = BuildConfig {
            tarball: tarball.clone(),
            tag: "test:v1".into(),
            dry_run: true,
            keep_context: true,
            podman_args: vec![],
        };

        let result1 = plan_and_execute(&config1);
        // On macOS without podman, we get PodmanNotFound, which is fine.
        // We just need the directory to be created.
        assert!(result1.is_ok() || pid_dir.exists());

        // Add a leftover file to the directory to make it non-empty.
        std::fs::create_dir_all(&pid_dir).ok();
        std::fs::write(pid_dir.join("leftover.txt"), "attacker symlink here").unwrap();

        // Second build: should fail with PreflightFailed due to non-empty directory.
        let tarball2 = build_test_tarball(tmp.path(), "FROM ubi9:latest", false);
        let config2 = BuildConfig {
            tarball: tarball2,
            tag: "test:v2".into(),
            dry_run: true,
            keep_context: true,
            podman_args: vec![],
        };

        let result2 = plan_and_execute(&config2);
        assert!(result2.is_ok());
        let (outcome, _) = result2.unwrap();
        match outcome {
            BuildOutcome::PreflightFailed { reason } => {
                assert!(reason.contains("already exists and is non-empty"), "{reason}");
                assert!(reason.contains(&pid_dir.to_string_lossy().to_string()), "{reason}");
            }
            other => panic!("expected PreflightFailed for non-empty dir, got: {other:?}"),
        }

        // Cleanup.
        std::fs::remove_dir_all(&pid_dir).ok();
    }
}
