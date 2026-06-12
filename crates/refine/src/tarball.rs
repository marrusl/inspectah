use crate::normalize::load_for_refine;
use crate::session::RefineSession;
use crate::types::RefineError;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::redaction::RedactionState;
use std::path::Path;

const MAX_UNPACKED_SIZE: u64 = 512 * 1024 * 1024; // 512 MiB
const MAX_FILE_COUNT: usize = 10_000;
const MAX_SINGLE_FILE: u64 = 256 * 1024 * 1024; // 256 MiB

pub fn from_tarball(path: &Path) -> Result<RefineSession, RefineError> {
    let tempdir = tempfile::tempdir().map_err(|e| RefineError::TarballError(e.to_string()))?;

    // Extract with safety checks
    extract_safe(path, tempdir.path())?;

    // Flatten prefixed archives
    let root = flatten_if_needed(tempdir.path())?;

    // Load snapshot
    let snap_path = root.join("inspection-snapshot.json");
    if !snap_path.exists() {
        return Err(RefineError::SnapshotLoad(
            "missing inspection-snapshot.json".into(),
        ));
    }

    let snap_json = std::fs::read_to_string(&snap_path)?;

    // load_for_refine handles the full pipeline:
    // raw-JSON include patching → deserialize → schema version check
    let mut snapshot = load_for_refine(&snap_json)?;

    // Check provenance — FullyRedacted only
    validate_provenance(&snapshot)?;

    // Normalize: Raw redaction state always implies sensitive data
    if matches!(snapshot.redaction_state, Some(RedactionState::Raw)) {
        snapshot.sensitive_snapshot = true;
    }

    Ok(RefineSession::new(snapshot))
}

fn extract_safe(tarball_path: &Path, dest: &Path) -> Result<(), RefineError> {
    let file = std::fs::File::open(tarball_path)?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);

    let mut total_size: u64 = 0;
    let mut file_count: usize = 0;

    for entry_result in archive
        .entries()
        .map_err(|e| RefineError::TarballError(e.to_string()))?
    {
        let mut entry = entry_result.map_err(|e| RefineError::TarballError(e.to_string()))?;

        file_count += 1;
        if file_count > MAX_FILE_COUNT {
            return Err(RefineError::ArchiveSafety(format!(
                "archive exceeds {MAX_FILE_COUNT} entry limit"
            )));
        }

        let path = entry
            .path()
            .map_err(|e| RefineError::TarballError(e.to_string()))?
            .to_path_buf();

        // Path traversal check
        for component in path.components() {
            if let std::path::Component::ParentDir = component {
                return Err(RefineError::ArchiveSafety(format!(
                    "path traversal: {}",
                    path.display()
                )));
            }
        }
        if path.is_absolute() {
            return Err(RefineError::ArchiveSafety(format!(
                "path traversal: absolute path {}",
                path.display()
            )));
        }

        // Entry type check
        let entry_type = entry.header().entry_type();
        if !matches!(
            entry_type,
            tar::EntryType::Regular | tar::EntryType::Directory
        ) {
            return Err(RefineError::ArchiveSafety(format!(
                "unsupported entry type: {:?} for {}",
                entry_type,
                path.display()
            )));
        }

        // Single file size check
        let size = entry
            .header()
            .size()
            .map_err(|e| RefineError::TarballError(e.to_string()))?;
        if size > MAX_SINGLE_FILE {
            return Err(RefineError::ArchiveSafety(format!(
                "single file exceeds {} MiB",
                MAX_SINGLE_FILE / (1024 * 1024)
            )));
        }

        // Total size check
        total_size += size;
        if total_size > MAX_UNPACKED_SIZE {
            return Err(RefineError::ArchiveSafety(format!(
                "archive exceeds {} MiB unpacked limit",
                MAX_UNPACKED_SIZE / (1024 * 1024)
            )));
        }

        entry
            .unpack_in(dest)
            .map_err(|e| RefineError::TarballError(e.to_string()))?;
    }

    Ok(())
}

fn flatten_if_needed(dir: &Path) -> Result<std::path::PathBuf, RefineError> {
    // Check if extraction produced a single subdirectory
    let entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();

    if entries.len() == 1 && entries[0].file_type().map(|t| t.is_dir()).unwrap_or(false) {
        let subdir = entries[0].path();
        // Check if the snapshot is inside the subdirectory
        if subdir.join("inspection-snapshot.json").exists() {
            return Ok(subdir);
        }
    }

    Ok(dir.to_path_buf())
}

fn validate_provenance(snap: &InspectionSnapshot) -> Result<(), RefineError> {
    match &snap.redaction_state {
        Some(RedactionState::FullyRedacted { .. })
        | Some(RedactionState::PartiallyRedacted { .. })
        | Some(RedactionState::SensitiveRetained { .. }) => Ok(()),
        Some(RedactionState::Raw) => {
            eprintln!(
                "warning: snapshot was scanned with --no-redaction. Sensitive data may be present."
            );
            Ok(())
        }
        Some(RedactionState::Unknown) => {
            eprintln!(
                "warning: snapshot has unknown redaction state. It may contain unredacted sensitive data."
            );
            Ok(())
        }
        None => {
            if snap.fleet_meta.is_some() {
                Ok(())
            } else {
                eprintln!(
                    "warning: snapshot has no redaction metadata. It may contain unredacted sensitive data."
                );
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_provenance_accepts_fully_redacted() {
        let snap = InspectionSnapshot {
            redaction_state: Some(RedactionState::FullyRedacted {
                redacted_by: "inspectah 0.8.0".into(),
                config_hash: "abc".into(),
            }),
            ..Default::default()
        };
        assert!(validate_provenance(&snap).is_ok());
    }

    #[test]
    fn validate_provenance_accepts_partially_redacted() {
        let snap = InspectionSnapshot {
            redaction_state: Some(RedactionState::PartiallyRedacted {
                redacted_by: "inspectah 0.8.0".into(),
                config_hash: "abc".into(),
                unresolved_count: 1,
                unresolved_hints: vec![],
            }),
            ..Default::default()
        };
        assert!(validate_provenance(&snap).is_ok());
    }

    #[test]
    fn validate_provenance_accepts_sensitive_retained() {
        let snap = InspectionSnapshot {
            redaction_state: Some(RedactionState::SensitiveRetained {
                redacted_by: "inspectah 0.8.0".into(),
                config_hash: "abc".into(),
                unresolved_count: 0,
                unresolved_hints: vec![],
            }),
            ..Default::default()
        };
        assert!(validate_provenance(&snap).is_ok());
    }

    #[test]
    fn validate_provenance_accepts_raw() {
        let snap = InspectionSnapshot {
            redaction_state: Some(RedactionState::Raw),
            ..Default::default()
        };
        assert!(validate_provenance(&snap).is_ok());
    }

    #[test]
    fn validate_provenance_raw_implies_sensitive() {
        // Provenance accepts Raw — normalization (sensitive_snapshot = true)
        // happens in from_tarball(), not here.
        let snap = InspectionSnapshot {
            redaction_state: Some(RedactionState::Raw),
            sensitive_snapshot: false,
            ..Default::default()
        };
        assert!(validate_provenance(&snap).is_ok());
    }

    #[test]
    fn validate_provenance_warns_on_none() {
        let snap = InspectionSnapshot {
            redaction_state: None,
            ..Default::default()
        };
        assert!(validate_provenance(&snap).is_ok());
    }
}
