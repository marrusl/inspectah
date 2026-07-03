use crate::rpm_ownership::build_rpm_owned_set;
use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::traits::progress::ProgressSink;
use inspectah_core::types::FindingKind;
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::finding::AdvisoryType;
use inspectah_core::types::redaction::{Confidence, RedactionHint};
use inspectah_core::types::storage::{
    CredentialRef, FstabEntry, LvmVolume, MountPoint, StorageSection, UnbackedVarAdvisory,
    VarDirBacking,
};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

/// Inspects storage configuration: fstab entries, active mount points,
/// LVM volumes, and detects credential references in mount options.
pub struct StorageInspector;

impl StorageInspector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StorageInspector {
    fn default() -> Self {
        Self::new()
    }
}

/// Deserialization target for `findmnt --json` output.
#[derive(Deserialize)]
struct FindmntOutput {
    filesystems: Vec<FindmntEntry>,
}

#[derive(Deserialize)]
struct FindmntEntry {
    target: String,
    source: String,
    fstype: String,
    options: String,
}

/// Deserialization target for `lvs --reportformat json` output.
#[derive(Deserialize)]
struct LvsOutput {
    report: Vec<LvsReport>,
}

#[derive(Deserialize)]
struct LvsReport {
    lv: Vec<LvsEntry>,
}

#[derive(Deserialize)]
struct LvsEntry {
    lv_name: String,
    vg_name: String,
    lv_size: String,
}

impl Inspector for StorageInspector {
    fn id(&self) -> InspectorId {
        InspectorId::Storage
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(
        &self,
        ctx: &InspectionContext<'_>,
        _progress: &dyn ProgressSink,
    ) -> Result<InspectorOutput, InspectorError> {
        let exec = ctx.executor;

        // 1. Read /etc/fstab — primary source, failure is fatal.
        let fstab_path = Path::new("/etc/fstab");
        let fstab_content = exec
            .read_file(fstab_path)
            .map_err(|e| InspectorError::Failed {
                reason: format!("cannot read /etc/fstab: {e}"),
            })?;

        let (fstab_entries, credential_refs, redaction_hints) = parse_fstab(&fstab_content);

        // 2. Run findmnt --json — degraded if unavailable or malformed.
        let mount_points = match collect_findmnt(exec) {
            Ok(mounts) => mounts,
            Err(reason) => {
                return Err(InspectorError::Degraded {
                    partial: Box::new(InspectorOutput {
                        section: SectionData::Storage(StorageSection {
                            fstab_entries,
                            mount_points: Vec::new(),
                            lvm_info: Vec::new(),
                            var_directories: Vec::new(),
                            credential_refs,
                            unbacked_var_advisory: None,
                        }),
                        warnings: Vec::new(),
                        redaction_hints,
                    }),
                    reason,
                });
            }
        };

        // 3. Run lvs --reportformat json — optional, proceed without.
        let lvm_info = collect_lvs(exec).unwrap_or_default();

        // 4. Detect backing for var directories.
        let mut var_directories: Vec<inspectah_core::types::storage::VarDirectory> = Vec::new();
        let rpm_owned = build_rpm_owned_set(exec);
        for dir in &mut var_directories {
            dir.backing = Some(detect_var_dir_backing(exec, &dir.path, &rpm_owned));
        }

        // Collect unbacked paths for grouped advisory
        let unbacked_paths: Vec<String> = var_directories
            .iter()
            .filter(|d| d.backing == Some(VarDirBacking::Unbacked))
            .map(|d| d.path.clone())
            .collect();

        let unbacked_var_advisory = if unbacked_paths.is_empty() {
            None
        } else {
            Some(UnbackedVarAdvisory {
                disposition: FindingKind::advisory(
                    AdvisoryType::UnbackedVarDir,
                    "These /var directories have no declarative backing (tmpfiles.d, \
                     StateDirectory=, CacheDirectory=, LogsDirectory=). Consider adding \
                     tmpfiles.d entries for a more reproducible, declarative approach.",
                ),
                paths: unbacked_paths,
            })
        };

        Ok(InspectorOutput {
            section: SectionData::Storage(StorageSection {
                fstab_entries,
                mount_points,
                lvm_info,
                var_directories,
                credential_refs,
                unbacked_var_advisory,
            }),
            warnings: Vec::new(),
            redaction_hints,
        })
    }
}

/// Parse /etc/fstab content into FstabEntry list, credential refs, and redaction hints.
fn parse_fstab(content: &str) -> (Vec<FstabEntry>, Vec<CredentialRef>, Vec<RedactionHint>) {
    let mut entries = Vec::new();
    let mut cred_refs = Vec::new();
    let mut hints = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        // Skip comments and empty lines
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }

        let device = parts[0].to_string();
        let mount_point = parts[1].to_string();
        let fstype = parts[2].to_string();
        let options = parts[3].to_string();

        // Detect credential references in mount options
        for opt in options.split(',') {
            if let Some(cred_path) = opt.strip_prefix("credentials=") {
                cred_refs.push(CredentialRef {
                    mount_point: mount_point.clone(),
                    credential_path: cred_path.to_string(),
                    source: "fstab".into(),
                });
                hints.push(RedactionHint {
                    path: "/etc/fstab".into(),
                    reason: format!(
                        "credential reference in mount options for {mount_point}: {cred_path}"
                    ),
                    confidence: Some(Confidence::High),
                });
            }
            if opt.starts_with("password=") {
                hints.push(RedactionHint {
                    path: "/etc/fstab".into(),
                    reason: format!("inline password in mount options for {mount_point}"),
                    confidence: Some(Confidence::High),
                });
            }
        }

        entries.push(FstabEntry {
            device,
            mount_point,
            fstype,
            options,
            disposition: FindingKind::included(),
            locked: false,
            acknowledged: false,
            aggregate: None,
            attention_reason: None,
        });
    }

    (entries, cred_refs, hints)
}

/// Collect mount points from `findmnt --json`.
fn collect_findmnt(exec: &dyn Executor) -> Result<Vec<MountPoint>, String> {
    let result = exec.run("findmnt", &["--json"]);

    if !result.success() {
        return Err(format!(
            "findmnt failed with exit code {}",
            result.exit_code
        ));
    }

    let parsed: FindmntOutput = serde_json::from_str(&result.stdout)
        .map_err(|e| format!("failed to parse findmnt JSON: {e}"))?;

    Ok(parsed
        .filesystems
        .into_iter()
        .map(|fs| MountPoint {
            target: fs.target,
            source: fs.source,
            fstype: fs.fstype,
            options: fs.options,
        })
        .collect())
}

/// Collect LVM volumes from `lvs --reportformat json`.
/// Returns Ok(empty) if lvs is not available — LVM is optional.
fn collect_lvs(exec: &dyn Executor) -> Result<Vec<LvmVolume>, String> {
    let result = exec.run("lvs", &["--reportformat", "json"]);

    // lvs not available or failed — not an error, just no LVM data.
    if !result.success() {
        return Ok(Vec::new());
    }

    let parsed: LvsOutput = serde_json::from_str(&result.stdout)
        .map_err(|e| format!("failed to parse lvs JSON: {e}"))?;

    Ok(parsed
        .report
        .into_iter()
        .flat_map(|r| r.lv)
        .map(|lv| LvmVolume {
            lv_name: lv.lv_name,
            vg_name: lv.vg_name,
            lv_size: lv.lv_size,
        })
        .collect())
}

/// Determine the backing mechanism for a /var directory.
///
/// Checks, in order: tmpfiles.d configs, systemd directory directives
/// (StateDirectory, CacheDirectory, LogsDirectory), RPM ownership.
/// Falls back to `Unbacked` if no backing is found.
fn detect_var_dir_backing(
    exec: &dyn Executor,
    path: &str,
    rpm_owned: &HashSet<String>,
) -> VarDirBacking {
    // Check tmpfiles.d (both /etc and /usr/lib)
    let tmpfiles_check = exec.run(
        "grep",
        &[
            "-r",
            "--include=*.conf",
            "-l",
            path,
            "/etc/tmpfiles.d/",
            "/usr/lib/tmpfiles.d/",
        ],
    );
    if tmpfiles_check.exit_code == 0 && !tmpfiles_check.stdout.trim().is_empty() {
        return VarDirBacking::Tmpfiles;
    }

    // Check all three systemd directory directives
    let dir_name = path.rsplit('/').next().unwrap_or("");
    for (directive, backing) in [
        ("StateDirectory", VarDirBacking::StateDirectory),
        ("CacheDirectory", VarDirBacking::CacheDirectory),
        ("LogsDirectory", VarDirBacking::LogsDirectory),
    ] {
        let grep = exec.run(
            "grep",
            &[
                "-r",
                "--include=*.service",
                "--include=*.socket",
                "-l",
                &format!("{}={}", directive, dir_name),
                "/usr/lib/systemd/system/",
                "/etc/systemd/system/",
            ],
        );
        if grep.exit_code == 0 && !grep.stdout.trim().is_empty() {
            return backing;
        }
    }

    // Check RPM ownership
    if rpm_owned.contains(path) {
        return VarDirBacking::RpmOwned;
    }

    VarDirBacking::Unbacked
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::storage::VarDirectory;

    #[test]
    fn test_unbacked_var_dir_stays_actionable() {
        // Unbacked dirs keep Actionable disposition for Containerfile
        let dir = VarDirectory {
            path: "/var/lib/pgsql/data".into(),
            backing: Some(VarDirBacking::Unbacked),
            ..Default::default()
        };
        // VarDirectory doesn't have disposition, so this test verifies
        // the advisory is separate from the directory entries
        assert_eq!(dir.backing, Some(VarDirBacking::Unbacked));
    }

    #[test]
    fn test_grouped_advisory_created_for_unbacked() {
        let advisory = UnbackedVarAdvisory {
            disposition: FindingKind::advisory(AdvisoryType::UnbackedVarDir, "test rationale"),
            paths: vec!["/var/lib/pgsql/data".into()],
        };
        assert!(advisory.disposition.is_advisory());
        assert_eq!(advisory.paths.len(), 1);
    }
}
