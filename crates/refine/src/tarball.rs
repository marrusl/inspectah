use crate::normalize::load_for_refine;
use crate::session::RefineSession;
use crate::types::RefineError;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::redaction::RedactionState;
use inspectah_pipeline::build::extract::{BudgetProbe, BudgetReader};
use std::io::Read;
use std::path::Path;

const MAX_UNPACKED_SIZE: u64 = 512 * 1024 * 1024; // 512 MiB
const MAX_FILE_COUNT: usize = 10_000;
const MAX_SINGLE_FILE: u64 = 256 * 1024 * 1024; // 256 MiB

/// Decompressed bytes allowed per entry beyond its own data: a 512-byte
/// header, up to 511 bytes of padding, and one long-name or PAX record
/// for a path up to Linux's 4096-byte `PATH_MAX` with its own header and
/// padding, rounded up to 8 KiB.
const STREAM_OVERHEAD_PER_ENTRY: u64 = 8 * 1024;

/// The most decompressed bytes a scan tarball may hold: the unpacked
/// limit plus [`STREAM_OVERHEAD_PER_ENTRY`] for every entry allowed,
/// about 590 MiB. The entry checks in [`extract_safe`] cannot bound the
/// extension records the `tar` crate buffers before an entry exists;
/// this bound does. See [`BudgetReader`].
pub(crate) const MAX_STREAM_SIZE: u64 =
    MAX_UNPACKED_SIZE + MAX_FILE_COUNT as u64 * STREAM_OVERHEAD_PER_ENTRY;

/// Map a read error from a budgeted archive. Running out of budget is an
/// archive-safety refusal; anything else is an ordinary tarball error,
/// passed through `other`.
pub(crate) fn stream_error(
    probe: &BudgetProbe,
    e: std::io::Error,
    other: impl FnOnce(std::io::Error) -> RefineError,
) -> RefineError {
    if probe.exhausted() {
        RefineError::ArchiveSafety(format!(
            "archive exceeds the {} MiB decompressed stream limit",
            MAX_STREAM_SIZE / (1024 * 1024)
        ))
    } else {
        other(e)
    }
}

pub fn from_tarball(path: &Path) -> Result<RefineSession, RefineError> {
    let tempdir = tempfile::tempdir().map_err(|e| RefineError::TarballError(e.to_string()))?;

    // Extract with safety checks
    extract_safe(path, tempdir.path())?;

    // Flatten prefixed archives
    let root = flatten_if_needed(tempdir.path())?;

    // Load snapshot
    let snap_path = root.join("inspection-snapshot.json");
    let snap_json = read_snapshot(&snap_path)?;

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
    extract_safe_with_budget(tarball_path, dest, MAX_STREAM_SIZE)
}

/// [`extract_safe`] with the decompressed stream held to `stream_budget`
/// bytes, so tests can exercise the bound without large archives.
fn extract_safe_with_budget(
    tarball_path: &Path,
    dest: &Path,
    stream_budget: u64,
) -> Result<(), RefineError> {
    let file = std::fs::File::open(tarball_path)?;
    let gz = flate2::read::GzDecoder::new(file);
    let (bounded, probe) = BudgetReader::new(gz, stream_budget);
    let mut archive = tar::Archive::new(bounded);
    let tar_error =
        |e: std::io::Error| stream_error(&probe, e, |e| RefineError::TarballError(e.to_string()));

    let mut total_size: u64 = 0;
    let mut file_count: usize = 0;

    for entry_result in archive.entries().map_err(tar_error)? {
        let mut entry = entry_result.map_err(tar_error)?;

        file_count += 1;
        if file_count > MAX_FILE_COUNT {
            return Err(RefineError::ArchiveSafety(format!(
                "archive exceeds {MAX_FILE_COUNT} entry limit"
            )));
        }

        let path = entry.path().map_err(tar_error)?.to_path_buf();

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
            tar::EntryType::Regular | tar::EntryType::Directory | tar::EntryType::Symlink
        ) {
            return Err(RefineError::ArchiveSafety(format!(
                "unsupported entry type: {:?} for {}",
                entry_type,
                path.display()
            )));
        }

        // Single file size check. `entry.size()` is the size the reader
        // extracts, which a PAX `size` record overrides; the ustar header
        // can say 0 for an entry that unpacks to gigabytes.
        let size = entry.size();
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

        entry.unpack_in(dest).map_err(tar_error)?;
    }

    Ok(())
}

/// Read the extracted snapshot, which must be a regular file inside the
/// extraction root, within [`MAX_SINGLE_FILE`].
///
/// The archive may carry a symlink at the snapshot's name, and extraction
/// keeps symlinks with their targets unrestricted. Following one would
/// read whatever it names: `/dev/zero` never ends, and a FIFO or a
/// blocking device would hang the load. `symlink_metadata` refuses the
/// link before anything is opened. The root is a fresh private tempdir,
/// so nothing can replace the file between the check and the open.
fn read_snapshot(snap_path: &Path) -> Result<String, RefineError> {
    let meta = match std::fs::symlink_metadata(snap_path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(RefineError::SnapshotLoad(
                "missing inspection-snapshot.json".into(),
            ));
        }
        Err(e) => return Err(e.into()),
    };
    if !meta.file_type().is_file() {
        return Err(RefineError::ArchiveSafety(
            "inspection-snapshot.json is not a regular file".into(),
        ));
    }
    // Extraction already bounds the entry; the read is bounded as well so
    // the limit does not rest on that check alone.
    let mut snap_json = String::new();
    std::fs::File::open(snap_path)?
        .take(MAX_SINGLE_FILE + 1)
        .read_to_string(&mut snap_json)?;
    if snap_json.len() as u64 > MAX_SINGLE_FILE {
        return Err(RefineError::ArchiveSafety(format!(
            "inspection-snapshot.json exceeds {} MiB",
            MAX_SINGLE_FILE / (1024 * 1024)
        )));
    }
    Ok(snap_json)
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
            if snap.aggregate_meta.is_some() {
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

    /// A budget far below the real one, so the regressions need no large
    /// allocation. Each hostile record is twice this.
    const TEST_BUDGET: u64 = 64 * 1024;
    const OVER_BUDGET: usize = 2 * TEST_BUDGET as usize;

    /// A GNU-format header, which the reader needs to honour long-name
    /// and long-link records.
    fn header(entry_type: tar::EntryType, name: &str) -> tar::Header {
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(entry_type);
        h.set_mode(0o644);
        h.set_path(name).unwrap();
        h
    }

    /// One archive member: its header and its data.
    type Row = (tar::Header, Vec<u8>);

    /// Write `rows` into a gzip tarball, setting each header's size from
    /// its data.
    fn raw_tarball(dir: &Path, rows: Vec<Row>) -> std::path::PathBuf {
        let path = dir.join("in.tar.gz");
        let gz = flate2::write::GzEncoder::new(
            std::fs::File::create(&path).unwrap(),
            flate2::Compression::default(),
        );
        let mut builder = tar::Builder::new(gz);
        for (mut h, data) in rows {
            h.set_size(data.len() as u64);
            h.set_cksum();
            builder.append(&h, &data[..]).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap();
        path
    }

    fn pax_record(key: &str, value: &str) -> Vec<u8> {
        let body = format!(" {key}={value}\n");
        let mut len = body.len() + 1;
        while format!("{len}{body}").len() != len {
            len += 1;
        }
        format!("{len}{body}").into_bytes()
    }

    /// The `tar` crate buffers each of these records whole before it
    /// returns the entry they belong to. Past the budget, each is
    /// refused while it is being read, not after.
    #[test]
    fn extension_records_past_the_stream_budget_are_refused() {
        let mut long_name = vec![b'a'; OVER_BUDGET];
        long_name.push(0);
        let mut long_link = vec![b'b'; OVER_BUDGET];
        long_link.push(0);
        let pax = pax_record("comment", &"c".repeat(OVER_BUDGET));
        let cases: [(&str, Vec<Row>); 3] = [
            (
                "PAX",
                vec![
                    (header(tar::EntryType::XHeader, "PaxHeaders/x"), pax),
                    (header(tar::EntryType::Regular, "x"), Vec::new()),
                ],
            ),
            (
                "GNU long name",
                vec![
                    (header(tar::EntryType::GNULongName, "LongLink"), long_name),
                    (header(tar::EntryType::Regular, "x"), Vec::new()),
                ],
            ),
            (
                "GNU long link",
                vec![
                    (header(tar::EntryType::GNULongLink, "LongLink"), long_link),
                    (header(tar::EntryType::Symlink, "y"), Vec::new()),
                ],
            ),
        ];
        let mut failures = Vec::new();
        for (what, rows) in cases {
            let dir = tempfile::tempdir().unwrap();
            let dest = tempfile::tempdir().unwrap();
            let tarball = raw_tarball(dir.path(), rows);
            match extract_safe_with_budget(&tarball, dest.path(), TEST_BUDGET) {
                Err(RefineError::ArchiveSafety(msg)) if msg.contains("decompressed") => {}
                other => {
                    // The long-name case's error quotes the whole name.
                    let mut seen = format!("{other:?}");
                    seen.truncate(120);
                    failures.push(format!("{what}: {seen}"));
                }
            }
        }
        assert!(failures.is_empty(), "not refused: {failures:#?}");
    }

    #[test]
    fn an_ordinary_archive_fits_the_stream_budget() {
        let dir = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let tarball = raw_tarball(
            dir.path(),
            vec![
                (
                    header(tar::EntryType::Regular, "inspection-snapshot.json"),
                    b"{}".to_vec(),
                ),
                (header(tar::EntryType::Directory, "unmanaged/"), Vec::new()),
            ],
        );
        extract_safe_with_budget(&tarball, dest.path(), TEST_BUDGET).expect("fits");
        assert!(dest.path().join("inspection-snapshot.json").is_file());
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
