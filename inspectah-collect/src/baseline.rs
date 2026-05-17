//! Baseline extraction: pulls a container image and extracts the
//! installed RPM package list via nsenter + podman.

use std::collections::HashMap;

use inspectah_core::baseline::{BaselineData, BaselinePackageEntry, NormalizedImageRef};
use inspectah_core::traits::executor::{ExecResult, Executor};

/// Errors that can occur during baseline extraction.
#[derive(Debug, thiserror::Error)]
pub enum ExtractionError {
    #[error("failed to pull image {image_ref}: {reason}")]
    PullFailed { image_ref: String, reason: String },
    #[error("failed to create baseline container: {0}")]
    CreateFailed(String),
    #[error("failed to start baseline container: {0}")]
    StartFailed(String),
    #[error("failed to extract packages: {0}")]
    ExecFailed(String),
    #[error("failed to capture image digest: {0}")]
    DigestFailed(String),
}

/// The nsenter prefix used to enter the host mount/UTS/IPC/net namespaces.
const NSENTER_PREFIX: &[&str] = &["nsenter", "-t", "1", "-m", "-u", "-i", "-n", "--"];

/// Guard that ensures `podman rm -f <container>` runs when dropped.
///
/// The `Drop` impl is best-effort — errors from rm are ignored since
/// we cannot propagate them from `drop()`.
struct CleanupGuard<'a> {
    container_name: Option<String>,
    executor: &'a dyn Executor,
}

impl<'a> CleanupGuard<'a> {
    fn new(executor: &'a dyn Executor) -> Self {
        Self {
            container_name: None,
            executor,
        }
    }

    fn set_container(&mut self, name: String) {
        self.container_name = Some(name);
    }

    /// Disarm the guard (container already cleaned up or never created).
    fn disarm(&mut self) {
        self.container_name = None;
    }
}

impl Drop for CleanupGuard<'_> {
    fn drop(&mut self) {
        if let Some(ref name) = self.container_name {
            let mut args: Vec<&str> = NSENTER_PREFIX.to_vec();
            args.extend_from_slice(&["podman", "rm", "-f", name.as_str()]);
            // Best-effort cleanup — ignore result.
            let _ = self.executor.run(args[0], &args[1..]);
        }
    }
}

/// Build the container name with a timestamp suffix.
fn container_name() -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("inspectah-baseline-{ts}")
}

/// Extract baseline package data from a container image.
///
/// Runs the full extraction sequence: pull, create, start, exec rpm,
/// cleanup, then inspect the image for digest. The container is always
/// removed when it was successfully created, even on error.
pub fn extract_baseline(
    executor: &dyn Executor,
    normalized_ref: &NormalizedImageRef,
) -> Result<BaselineData, ExtractionError> {
    let image_ref = normalized_ref.as_str();
    let ctr_name = container_name();

    let mut guard = CleanupGuard::new(executor);

    // 1. Pull
    let pull_result = run_nsenter(executor, &["podman", "pull", image_ref]);
    if !pull_result.success() {
        return Err(ExtractionError::PullFailed {
            image_ref: image_ref.to_string(),
            reason: pull_result.stderr.trim().to_string(),
        });
    }

    // 2. Create
    let create_result = run_nsenter(
        executor,
        &[
            "podman",
            "create",
            "--name",
            &ctr_name,
            "--entrypoint",
            r#"["sleep","infinity"]"#,
            "--network",
            "none",
            image_ref,
        ],
    );
    if !create_result.success() {
        return Err(ExtractionError::CreateFailed(
            create_result.stderr.trim().to_string(),
        ));
    }
    // Container exists now — arm the guard.
    guard.set_container(ctr_name.clone());

    // 3. Start
    let start_result = run_nsenter(executor, &["podman", "start", &ctr_name]);
    if !start_result.success() {
        // Guard will rm on drop.
        return Err(ExtractionError::StartFailed(
            start_result.stderr.trim().to_string(),
        ));
    }

    // 4. Exec rpm -qa
    let exec_result = run_nsenter(
        executor,
        &[
            "podman",
            "exec",
            &ctr_name,
            "rpm",
            "-qa",
            "--queryformat",
            "%{NAME}\\t%{EPOCH}\\t%{VERSION}\\t%{RELEASE}\\t%{ARCH}\\n",
        ],
    );
    if !exec_result.success() {
        // Guard will rm on drop.
        return Err(ExtractionError::ExecFailed(
            exec_result.stderr.trim().to_string(),
        ));
    }

    let packages = parse_nevra_output(&exec_result.stdout);

    // 5. Explicit cleanup — disarm guard after successful rm.
    let rm_result = run_nsenter(executor, &["podman", "rm", "-f", &ctr_name]);
    if rm_result.success() {
        guard.disarm();
    }
    // If rm failed, guard will try again in drop — belt and suspenders.

    // 6. Inspect image for digest (on the IMAGE, not container).
    let digest = capture_image_digest(executor, image_ref)?;

    let extracted_at = chrono_now_utc();

    Ok(BaselineData {
        image_digest: digest,
        packages,
        extracted_at,
    })
}

/// Run a command through the nsenter prefix.
fn run_nsenter(executor: &dyn Executor, cmd_and_args: &[&str]) -> ExecResult {
    let mut full_args: Vec<&str> = NSENTER_PREFIX.to_vec();
    full_args.extend_from_slice(cmd_and_args);
    executor.run(full_args[0], &full_args[1..])
}

/// Capture the image digest. Primary: `podman inspect --format '{{.Digest}}'`.
/// Fallback: `podman inspect --format '{{index .RepoDigests 0}}'` and extract
/// the digest after `@`.
fn capture_image_digest(
    executor: &dyn Executor,
    image_ref: &str,
) -> Result<String, ExtractionError> {
    // Primary attempt.
    let primary = run_nsenter(
        executor,
        &[
            "podman",
            "inspect",
            "--format",
            "{{.Digest}}",
            image_ref,
        ],
    );
    if primary.success() {
        let digest = primary.stdout.trim().to_string();
        if !digest.is_empty() {
            return Ok(digest);
        }
    }

    // Fallback: RepoDigests[0].
    let fallback = run_nsenter(
        executor,
        &[
            "podman",
            "inspect",
            "--format",
            "{{index .RepoDigests 0}}",
            image_ref,
        ],
    );
    if fallback.success() {
        let raw = fallback.stdout.trim();
        if let Some(pos) = raw.rfind('@') {
            let digest = raw[pos + 1..].to_string();
            if !digest.is_empty() {
                return Ok(digest);
            }
        }
    }

    Err(ExtractionError::DigestFailed(format!(
        "no digest found for {image_ref}"
    )))
}

/// Parse NEVRA tab-separated output into a package map keyed by `name.arch`.
fn parse_nevra_output(output: &str) -> HashMap<String, BaselinePackageEntry> {
    let mut packages = HashMap::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 5 {
            continue;
        }

        let name = parts[0].to_string();
        let epoch_raw = parts[1].trim();
        let epoch = if epoch_raw == "0" || epoch_raw == "(none)" || epoch_raw.is_empty() {
            None
        } else {
            Some(epoch_raw.to_string())
        };
        let version = parts[2].to_string();
        let release = parts[3].to_string();
        let arch = parts[4].to_string();

        let key = format!("{}.{}", name, arch);
        packages.insert(
            key,
            BaselinePackageEntry {
                name,
                epoch,
                version,
                release,
                arch,
            },
        );
    }

    packages
}

/// Returns current UTC time as ISO-8601 string.
///
/// Uses a simple approach without requiring the `chrono` crate.
fn chrono_now_utc() -> String {
    // Use SystemTime for a simple UTC timestamp.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();

    // Convert to rough ISO-8601 (good enough for extraction metadata).
    let days = secs / 86400;
    let remaining = secs % 86400;
    let hours = remaining / 3600;
    let minutes = (remaining % 3600) / 60;
    let seconds = remaining % 60;

    // Calculate year/month/day from days since epoch.
    let (year, month, day) = days_to_ymd(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_ymd(days_since_epoch: u64) -> (u64, u64, u64) {
    // Algorithm adapted from Howard Hinnant's civil_from_days.
    let z = days_since_epoch as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as u64, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_nevra_basic() {
        let output = "bash\t0\t5.2.26\t3.el9\tx86_64\ncoreutils\t0\t9.1\t13.el9\tx86_64\n";
        let pkgs = parse_nevra_output(output);
        assert_eq!(pkgs.len(), 2);
        assert!(pkgs.contains_key("bash.x86_64"));
        assert!(pkgs.contains_key("coreutils.x86_64"));
        // epoch "0" → None
        assert_eq!(pkgs["bash.x86_64"].epoch, None);
    }

    #[test]
    fn test_parse_nevra_with_epoch() {
        let output = "vim\t2\t9.0\t1.el9\tx86_64\n";
        let pkgs = parse_nevra_output(output);
        assert_eq!(pkgs["vim.x86_64"].epoch, Some("2".to_string()));
    }

    #[test]
    fn test_parse_nevra_mixed_arch() {
        let output = "bash\t0\t5.2.26\t3.el9\taarch64\n";
        let pkgs = parse_nevra_output(output);
        assert!(pkgs.contains_key("bash.aarch64"));
        assert!(!pkgs.contains_key("bash.x86_64"));
    }

    #[test]
    fn test_chrono_now_utc_format() {
        let ts = chrono_now_utc();
        // Should match ISO-8601 pattern.
        assert!(ts.ends_with('Z'));
        assert_eq!(ts.len(), 20); // YYYY-MM-DDTHH:MM:SSZ
    }
}
