use crate::types::{ContentHash, RefinementOp, TimelineEntry};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub schema_version: u32,
    pub tarball_path: PathBuf,
    pub tarball_hash: ContentHash,
    pub timeline: Vec<TimelineEntry>,
    pub cursor: usize,
    pub saved_at: String,
}

/// Compute the session sidecar file path for a given tarball.
///
/// Strips `.tar.gz` or `.tgz` from the tarball filename, prefixes with
/// `.inspectah-session-`, suffixes with `.json`, and places in the same
/// directory as the tarball.
pub fn session_file_path(tarball: &Path) -> PathBuf {
    let stem = tarball
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let base = if stem.ends_with(".tar.gz") {
        stem.strip_suffix(".tar.gz").unwrap().to_string()
    } else if stem.ends_with(".tgz") {
        stem.strip_suffix(".tgz").unwrap().to_string()
    } else {
        // Fall back to Path::file_stem for other extensions
        tarball
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    };

    let session_name = format!(".inspectah-session-{base}.json");
    tarball.with_file_name(session_name)
}

/// Atomically save session state to a sidecar file next to the tarball.
///
/// Writes to a temporary file in the same directory, then renames to the
/// final path. This prevents partial writes from corrupting the session file.
pub fn save_session(state: &SessionState, tarball: &Path) -> Result<(), std::io::Error> {
    let dest = session_file_path(tarball);
    let parent = dest
        .parent()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no parent dir"))?;

    let mut tmp = NamedTempFile::new_in(parent)?;
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    tmp.write_all(json.as_bytes())?;
    tmp.flush()?;
    tmp.persist(&dest)?;
    Ok(())
}

/// Load session state from the sidecar file next to the tarball.
///
/// Supports schema v2 (ops-only) and v3 (full timeline with view directives).
/// v2 files are migrated to v3 on load by wrapping each op in `TimelineEntry::Op`.
///
/// Returns `Ok(None)` if no session file exists. Returns an error if the
/// file exists but has an unsupported schema version or is malformed.
pub fn load_session(tarball: &Path) -> Result<Option<SessionState>, Box<dyn std::error::Error>> {
    let path = session_file_path(tarball);
    if !path.exists() {
        return Ok(None);
    }

    let contents = std::fs::read_to_string(&path)?;

    // Check schema_version before full deserialization.
    let raw: serde_json::Value = serde_json::from_str(&contents)?;
    let version = raw
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    match version {
        2 => {
            // v2 stores ops directly; migrate to v3 by wrapping in TimelineEntry::Op.
            let v2: SessionStateV2 = serde_json::from_str(&contents)?;
            let timeline = v2.ops.into_iter().map(TimelineEntry::Op).collect();
            Ok(Some(SessionState {
                schema_version: 3,
                tarball_path: v2.tarball_path,
                tarball_hash: v2.tarball_hash,
                timeline,
                cursor: v2.cursor,
                saved_at: v2.saved_at,
            }))
        }
        3 => {
            let state: SessionState = serde_json::from_str(&contents)?;
            Ok(Some(state))
        }
        other => Err(format!(
            "unsupported session schema version: {other} (expected 2 or 3)"
        )
        .into()),
    }
}

/// Legacy v2 session format for migration. Only used during deserialization
/// of v2 sidecar files.
#[derive(Deserialize)]
struct SessionStateV2 {
    #[allow(dead_code)]
    schema_version: u32,
    tarball_path: PathBuf,
    tarball_hash: ContentHash,
    ops: Vec<RefinementOp>,
    cursor: usize,
    saved_at: String,
}

/// Compute the SHA-256 hash of a tarball file.
///
/// Uses streaming hash to avoid loading large tarballs into memory.
pub fn compute_tarball_hash(tarball: &Path) -> Result<ContentHash, std::io::Error> {
    let mut file = std::fs::File::open(tarball)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let hash = format!("{:x}", hasher.finalize());
    ContentHash::new(hash).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ItemId, ViewDirective};

    /// Helper: write a tiny tarball so compute_tarball_hash works.
    fn write_dummy_tarball(path: &Path) {
        let f = std::fs::File::create(path).unwrap();
        let gz = flate2::write::GzEncoder::new(f, flate2::Compression::default());
        let mut tar = tar::Builder::new(gz);
        let data = b"hello";
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_cksum();
        tar.append_data(&mut header, "dummy.txt", &data[..])
            .unwrap();
        tar.finish().unwrap();
    }

    #[test]
    fn v2_json_loads_directly() {
        let dir = tempfile::tempdir().unwrap();
        let tarball = dir.path().join("test.tar.gz");
        write_dummy_tarball(&tarball);
        let hash = compute_tarball_hash(&tarball).unwrap();

        let v2_json = serde_json::json!({
            "schema_version": 2,
            "tarball_path": tarball.to_string_lossy(),
            "tarball_hash": hash.as_str(),
            "ops": [
                {
                    "op": "SetInclude",
                    "target": {
                        "item_id": {"kind": "Package", "key": {"name": "vim", "arch": "x86_64"}},
                        "include": false
                    }
                }
            ],
            "cursor": 1,
            "saved_at": "200s"
        });
        let session_path = session_file_path(&tarball);
        std::fs::write(
            &session_path,
            serde_json::to_string_pretty(&v2_json).unwrap(),
        )
        .unwrap();

        let loaded = load_session(&tarball).unwrap().unwrap();
        // v2 files are migrated to v3 on load
        assert_eq!(loaded.schema_version, 3);
        assert_eq!(loaded.timeline.len(), 1);
        assert_eq!(loaded.cursor, 1);

        match &loaded.timeline[0] {
            TimelineEntry::Op(RefinementOp::SetInclude { item_id, include }) => {
                assert!(!include);
                match item_id {
                    ItemId::Package { name, arch } => {
                        assert_eq!(name, "vim");
                        assert_eq!(arch, "x86_64");
                    }
                    other => panic!("expected Package, got {:?}", other),
                }
            }
            other => panic!("expected TimelineEntry::Op(SetInclude), got {:?}", other),
        }
    }

    #[test]
    fn v3_save_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let tarball = dir.path().join("test.tar.gz");
        write_dummy_tarball(&tarball);
        let hash = compute_tarball_hash(&tarball).unwrap();

        let state = SessionState {
            schema_version: 3,
            tarball_path: tarball.clone(),
            tarball_hash: hash,
            timeline: vec![
                TimelineEntry::Op(RefinementOp::SetInclude {
                    item_id: ItemId::Package {
                        name: "httpd".into(),
                        arch: "x86_64".into(),
                    },
                    include: false,
                }),
                TimelineEntry::Op(RefinementOp::SetInclude {
                    item_id: ItemId::Config {
                        path: "/etc/httpd/httpd.conf".into(),
                    },
                    include: true,
                }),
            ],
            cursor: 1,
            saved_at: "300s".into(),
        };

        save_session(&state, &tarball).unwrap();
        let loaded = load_session(&tarball).unwrap().unwrap();

        assert_eq!(loaded.schema_version, 3);
        assert_eq!(loaded.timeline.len(), 2);
        assert_eq!(loaded.cursor, 1);
        assert_eq!(loaded.timeline, state.timeline);
    }

    #[test]
    fn v1_session_file_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let tarball = dir.path().join("test.tar.gz");
        write_dummy_tarball(&tarball);
        let hash = compute_tarball_hash(&tarball).unwrap();

        let v1_json = serde_json::json!({
            "schema_version": 1,
            "tarball_path": tarball.to_string_lossy(),
            "tarball_hash": hash.as_str(),
            "ops": [
                {"op": "ExcludePackage", "target": {"name": "httpd", "arch": "x86_64"}}
            ],
            "cursor": 1,
            "saved_at": "100s"
        });
        let session_path = session_file_path(&tarball);
        std::fs::write(
            &session_path,
            serde_json::to_string_pretty(&v1_json).unwrap(),
        )
        .unwrap();

        let result = load_session(&tarball);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unsupported"));
    }

    #[test]
    fn unsupported_version_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let tarball = dir.path().join("test.tar.gz");
        write_dummy_tarball(&tarball);
        let hash = compute_tarball_hash(&tarball).unwrap();

        let bad_json = serde_json::json!({
            "schema_version": 99,
            "tarball_path": tarball.to_string_lossy(),
            "tarball_hash": hash.as_str(),
            "timeline": [],
            "cursor": 0,
            "saved_at": "0s"
        });
        let session_path = session_file_path(&tarball);
        std::fs::write(
            &session_path,
            serde_json::to_string_pretty(&bad_json).unwrap(),
        )
        .unwrap();

        let result = load_session(&tarball);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unsupported"));
    }

    #[test]
    fn v3_session_round_trips_with_timeline_entries() {
        let dir = tempfile::tempdir().unwrap();
        let tarball = dir.path().join("test.tar.gz");
        write_dummy_tarball(&tarball);
        let hash = compute_tarball_hash(&tarball).unwrap();

        // Create a session with both Op and View entries
        let state = SessionState {
            schema_version: 3,
            tarball_path: tarball.clone(),
            tarball_hash: hash,
            timeline: vec![
                TimelineEntry::Op(RefinementOp::SetInclude {
                    item_id: ItemId::Package {
                        name: "httpd".into(),
                        arch: "x86_64".into(),
                    },
                    include: false,
                }),
                TimelineEntry::View(ViewDirective::UngroupGroup {
                    group_name: "web-server".into(),
                }),
                TimelineEntry::Op(RefinementOp::SetInclude {
                    item_id: ItemId::Config {
                        path: "/etc/httpd/httpd.conf".into(),
                    },
                    include: true,
                }),
            ],
            cursor: 2,
            saved_at: "400s".into(),
        };

        save_session(&state, &tarball).unwrap();
        let loaded = load_session(&tarball).unwrap().unwrap();

        assert_eq!(loaded.schema_version, 3);
        assert_eq!(loaded.timeline.len(), 3);
        assert_eq!(loaded.cursor, 2);
        assert_eq!(loaded.timeline, state.timeline);

        // Verify the View directive survived
        match &loaded.timeline[1] {
            TimelineEntry::View(ViewDirective::UngroupGroup { group_name }) => {
                assert_eq!(group_name, "web-server");
            }
            other => panic!("expected TimelineEntry::View(UngroupGroup), got {:?}", other),
        }
    }

    #[test]
    fn v2_session_migrates_to_v3_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let tarball = dir.path().join("test.tar.gz");
        write_dummy_tarball(&tarball);
        let hash = compute_tarball_hash(&tarball).unwrap();

        // Write a v2-format session file manually (with "ops" field)
        let v2_json = serde_json::json!({
            "schema_version": 2,
            "tarball_path": tarball.to_string_lossy(),
            "tarball_hash": hash.as_str(),
            "ops": [
                {
                    "op": "SetInclude",
                    "target": {
                        "item_id": {"kind": "Package", "key": {"name": "httpd", "arch": "x86_64"}},
                        "include": false
                    }
                },
                {
                    "op": "SetInclude",
                    "target": {
                        "item_id": {"kind": "Config", "key": {"path": "/etc/sshd_config"}},
                        "include": true
                    }
                }
            ],
            "cursor": 2,
            "saved_at": "500s"
        });
        let session_path = session_file_path(&tarball);
        std::fs::write(
            &session_path,
            serde_json::to_string_pretty(&v2_json).unwrap(),
        )
        .unwrap();

        let loaded = load_session(&tarball).unwrap().unwrap();

        // Verify migration to v3
        assert_eq!(loaded.schema_version, 3);
        assert_eq!(loaded.timeline.len(), 2);
        assert_eq!(loaded.cursor, 2);

        // Ops should be wrapped in TimelineEntry::Op
        match &loaded.timeline[0] {
            TimelineEntry::Op(RefinementOp::SetInclude { item_id, include }) => {
                assert!(!include);
                match item_id {
                    ItemId::Package { name, arch } => {
                        assert_eq!(name, "httpd");
                        assert_eq!(arch, "x86_64");
                    }
                    other => panic!("expected Package, got {:?}", other),
                }
            }
            other => panic!("expected TimelineEntry::Op(SetInclude), got {:?}", other),
        }
        match &loaded.timeline[1] {
            TimelineEntry::Op(RefinementOp::SetInclude { item_id, include }) => {
                assert!(include);
                match item_id {
                    ItemId::Config { path } => {
                        assert_eq!(path, "/etc/sshd_config");
                    }
                    other => panic!("expected Config, got {:?}", other),
                }
            }
            other => panic!("expected TimelineEntry::Op(SetInclude), got {:?}", other),
        }
    }
}
