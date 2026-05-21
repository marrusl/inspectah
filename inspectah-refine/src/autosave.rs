use crate::types::{ContentHash, RefinementOp};
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
    pub ops: Vec<RefinementOp>,
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
/// Returns `Ok(None)` if no session file exists. Returns an error if the
/// file exists but has an unknown schema version or is malformed.
pub fn load_session(
    tarball: &Path,
) -> Result<Option<SessionState>, Box<dyn std::error::Error>> {
    let path = session_file_path(tarball);
    if !path.exists() {
        return Ok(None);
    }

    let contents = std::fs::read_to_string(&path)?;
    let state: SessionState = serde_json::from_str(&contents)?;

    if state.schema_version != 1 {
        return Err(format!(
            "unsupported session schema version: {} (expected 1)",
            state.schema_version
        )
        .into());
    }

    Ok(Some(state))
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
    ContentHash::new(hash).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
}
