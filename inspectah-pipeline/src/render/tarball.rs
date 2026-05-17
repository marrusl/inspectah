//! Tarball construction — creates deterministic .tar.gz archives from output.
//!
//! Mirrors Go `CreateTarball` with added path safety validation.

use flate2::Compression;
use flate2::write::GzEncoder;
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;
use tar::Header;

use super::configtree::validate_tarball_entry;

/// Regex for unsafe filename characters — replaced during hostname sanitization.
static UNSAFE_FILENAME_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\w.-]").unwrap());

/// Remove characters unsafe for filenames from a hostname.
pub fn sanitize_hostname(hostname: &str) -> String {
    let cleaned = UNSAFE_FILENAME_RE.replace_all(hostname, "").to_string();
    if cleaned.is_empty() {
        "unknown".to_string()
    } else {
        cleaned
    }
}

/// Returns "HOSTNAME-YYYYMMDD-HHMMSS" for tarball naming.
pub fn get_output_stamp(hostname: &str) -> String {
    let resolved = sanitize_hostname(hostname);
    let now = chrono::Local::now().format("%Y%m%d-%H%M%S");
    format!("{resolved}-{now}")
}

/// Create a gzipped tarball from source_dir with entries under prefix/.
///
/// Entries are sorted for deterministic output. Path safety is enforced:
/// entries with traversal components, absolute paths, or NUL bytes are
/// rejected.
pub fn create_tarball(
    source_dir: &Path,
    tarball_path: &Path,
    prefix: &str,
) -> Result<(), TarballError> {
    let f = std::fs::File::create(tarball_path)?;
    let gz = GzEncoder::new(f, Compression::default());
    let mut tar = tar::Builder::new(gz);

    // Collect and sort paths for deterministic output
    let mut paths: Vec<_> = walkdir::WalkDir::new(source_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .map(|e| e.into_path())
        .collect();
    paths.sort();

    for path in &paths {
        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let rel = match path.strip_prefix(source_dir) {
            Ok(r) => r.to_string_lossy().to_string(),
            Err(_) => continue,
        };

        let arcname = if rel == "." || rel.is_empty() {
            prefix.to_string()
        } else {
            format!("{prefix}/{rel}")
        };

        // Validate the archive entry path
        if validate_tarball_entry(&arcname).is_err() {
            continue;
        }

        let mut header = Header::new_gnu();
        header.set_size(if meta.is_dir() { 0 } else { meta.len() });
        header.set_mode(if meta.is_dir() { 0o755 } else { 0o644 });
        header.set_mtime(
            meta.modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0),
        );

        if meta.is_dir() {
            let dir_name = if arcname.ends_with('/') {
                arcname.clone()
            } else {
                format!("{arcname}/")
            };
            header.set_entry_type(tar::EntryType::Directory);
            header.set_size(0);
            header.set_cksum();
            tar.append_data(&mut header, &dir_name, &[][..])?;
        } else {
            header.set_entry_type(tar::EntryType::Regular);
            let data = std::fs::read(path)?;
            header.set_size(data.len() as u64);
            header.set_cksum();
            tar.append_data(&mut header, &arcname, &data[..])?;
        }
    }

    let gz = tar.into_inner()?;
    gz.finish()?;
    Ok(())
}

/// List all entry names in a gzipped tarball.
pub fn list_tarball_entries(tarball_path: &Path) -> Vec<String> {
    let f = match std::fs::File::open(tarball_path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let gz = flate2::read::GzDecoder::new(f);
    let mut ar = tar::Archive::new(gz);
    let mut entries = Vec::new();
    if let Ok(iter) = ar.entries() {
        for entry in iter.flatten() {
            if let Ok(path) = entry.path() {
                entries.push(path.to_string_lossy().to_string());
            }
        }
    }
    entries
}

/// Tarball construction errors.
#[derive(Debug, thiserror::Error)]
pub enum TarballError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_sanitize_hostname() {
        assert_eq!(sanitize_hostname("myhost"), "myhost");
        assert_eq!(sanitize_hostname("my-host.local"), "my-host.local");
        assert_eq!(sanitize_hostname("host name!@#"), "hostname");
        assert_eq!(sanitize_hostname(""), "unknown");
        assert_eq!(sanitize_hostname("!!!"), "unknown");
    }

    #[test]
    fn test_create_tarball_basic() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("source");
        std::fs::create_dir_all(source.join("subdir")).unwrap();
        std::fs::write(source.join("file.txt"), "hello").unwrap();
        std::fs::write(source.join("subdir/nested.txt"), "world").unwrap();

        let tarball_path = dir.path().join("test.tar.gz");
        create_tarball(&source, &tarball_path, "prefix").unwrap();

        assert!(tarball_path.exists());
        let entries = list_tarball_entries(&tarball_path);
        assert!(!entries.is_empty());
        // Check that entries have the prefix
        assert!(entries.iter().any(|e| e.contains("file.txt")));
        assert!(entries.iter().any(|e| e.contains("nested.txt")));
    }

    #[test]
    fn test_tarball_entries_sorted() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("source");
        std::fs::create_dir_all(&source).unwrap();
        // Create files in reverse alphabetical order
        std::fs::write(source.join("z.txt"), "z").unwrap();
        std::fs::write(source.join("a.txt"), "a").unwrap();
        std::fs::write(source.join("m.txt"), "m").unwrap();

        let tarball_path = dir.path().join("sorted.tar.gz");
        create_tarball(&source, &tarball_path, "test").unwrap();

        let entries = list_tarball_entries(&tarball_path);
        // Filter to just files (not directories)
        let file_entries: Vec<_> = entries
            .iter()
            .filter(|e| !e.ends_with('/'))
            .cloned()
            .collect();
        // Should be sorted
        let mut sorted = file_entries.clone();
        sorted.sort();
        assert_eq!(file_entries, sorted);
    }

    #[test]
    fn test_tarball_with_all_artifacts() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("output");
        std::fs::create_dir_all(source.join("config/etc")).unwrap();
        std::fs::create_dir_all(source.join("schema")).unwrap();

        // Write the 8 always-written artifacts
        std::fs::write(source.join("inspection-snapshot.json"), "{}").unwrap();
        std::fs::write(source.join("Containerfile"), "FROM ...").unwrap();
        std::fs::write(source.join("README.md"), "# Output").unwrap();
        std::fs::write(source.join("report.html"), "<html>").unwrap();
        std::fs::write(source.join("audit-report.md"), "# Audit").unwrap();
        std::fs::write(source.join("secrets-review.md"), "# Secrets").unwrap();
        std::fs::write(source.join("kickstart-suggestion.ks"), "# KS").unwrap();
        std::fs::write(source.join("schema/snapshot.schema.json"), "{}").unwrap();

        let tarball_path = dir.path().join("output.tar.gz");
        create_tarball(&source, &tarball_path, "inspectah-testhost-20260510-120000").unwrap();

        let expected = [
            "inspection-snapshot.json",
            "Containerfile",
            "README.md",
            "report.html",
            "audit-report.md",
            "secrets-review.md",
            "kickstart-suggestion.ks",
            "schema/snapshot.schema.json",
        ];
        let entries = list_tarball_entries(&tarball_path);
        for artifact in &expected {
            assert!(
                entries.iter().any(|e| e.ends_with(artifact)),
                "missing always-written artifact: {artifact}. entries: {entries:?}"
            );
        }
    }

    #[test]
    fn test_get_output_stamp_format() {
        let stamp = get_output_stamp("myhost");
        assert!(stamp.starts_with("myhost-"));
        // Should match YYYYMMDD-HHMMSS pattern
        let parts: Vec<_> = stamp.splitn(2, '-').collect();
        assert_eq!(parts[0], "myhost");
        // Rest should be date-time
        assert!(parts[1].len() >= 15); // YYYYMMDD-HHMMSS
    }
}
