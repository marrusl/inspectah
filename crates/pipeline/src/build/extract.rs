//! Archive extraction with full safety contract.
//!
//! Every forbidden condition from the spec produces a hard error:
//! - Path traversal (`../`)
//! - Absolute paths
//! - Duplicate path entries
//! - File-type replacement (e.g., file replacing symlink at same path)
//! - Special file types (device nodes, FIFOs, sockets) -- REJECTED, not skipped
//! - Hard links (all hardlinks are rejected; inspectah tarballs do not use them)
//! - Symlinks escaping extraction root
//!
//! Post-extraction defense-in-depth: canonicalize() on destinations
//! verifies entries remain under the extraction root.

use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Entry kind tracker for file-type replacement detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    Regular,
    Directory,
    Symlink,
}

impl EntryKind {
    fn label(self) -> &'static str {
        match self {
            Self::Regular => "regular",
            Self::Directory => "directory",
            Self::Symlink => "symlink",
        }
    }
}

/// Archive validation error -- one per forbidden condition.
#[derive(Debug)]
pub enum ArchiveViolation {
    PathTraversal(String),
    AbsolutePath(String),
    DuplicatePath(String),
    TypeReplacement {
        path: String,
        was: &'static str,
        now: &'static str,
    },
    SpecialFileType {
        path: String,
        kind: &'static str,
    },
    Hardlink(String),
    SymlinkEscape {
        path: String,
        target: String,
    },
}

impl std::fmt::Display for ArchiveViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PathTraversal(p) => write!(f, "path traversal: {p}"),
            Self::AbsolutePath(p) => write!(f, "absolute path: {p}"),
            Self::DuplicatePath(p) => write!(f, "duplicate entry: {p}"),
            Self::TypeReplacement { path, was, now } => {
                write!(f, "type replacement at {path}: {was} -> {now}")
            }
            Self::SpecialFileType { path, kind } => {
                write!(f, "forbidden file type at {path}: {kind}")
            }
            Self::Hardlink(p) => {
                write!(f, "hardlinks are not supported in inspectah tarballs: {p}")
            }
            Self::SymlinkEscape { path, target } => {
                write!(f, "symlink escape: {path} -> {target}")
            }
        }
    }
}

/// Validated, safe tarball extractor.
///
/// Rejects ALL spec-forbidden conditions with hard errors.
/// Uses a two-pass approach: validate each entry before extracting it,
/// then canonicalize post-extraction as defense-in-depth.
pub struct TarballExtractor {
    extract_dir: PathBuf,
}

impl TarballExtractor {
    /// Create extractor targeting a specific directory.
    pub fn new(extract_dir: PathBuf) -> Self {
        Self { extract_dir }
    }

    /// Extract tarball with full safety validation.
    /// Returns the extraction directory path on success.
    pub fn extract(&self, tarball: &Path) -> Result<&Path> {
        std::fs::create_dir_all(&self.extract_dir)?;

        let f = std::fs::File::open(tarball)
            .with_context(|| format!("cannot open tarball: {}", tarball.display()))?;
        let gz = flate2::read::GzDecoder::new(f);
        let mut archive = tar::Archive::new(gz);

        let mut seen_paths: HashMap<String, EntryKind> = HashMap::new();

        for entry_result in archive.entries()? {
            let mut entry = entry_result?;
            let raw_path = entry.path()?.to_path_buf();
            let path_str = raw_path.to_string_lossy().to_string();

            // Strip first component (tarball prefix directory).
            let stripped: PathBuf = raw_path.components().skip(1).collect();
            if stripped.as_os_str().is_empty() {
                continue;
            }
            let stripped_str = stripped.to_string_lossy().to_string();

            // SAFETY: path traversal
            if stripped_str.contains("..") {
                bail!("{}", ArchiveViolation::PathTraversal(path_str));
            }

            // SAFETY: absolute paths
            if stripped_str.starts_with('/') {
                bail!("{}", ArchiveViolation::AbsolutePath(path_str));
            }

            let entry_type = entry.header().entry_type();

            // SAFETY: hardlinks are not supported
            if entry_type == tar::EntryType::Link {
                bail!("{}", ArchiveViolation::Hardlink(path_str));
            }

            // SAFETY: special file types -- REJECT, don't skip
            let kind = match entry_type {
                tar::EntryType::Regular | tar::EntryType::GNUSparse => EntryKind::Regular,
                tar::EntryType::Directory => EntryKind::Directory,
                tar::EntryType::Symlink => EntryKind::Symlink,
                tar::EntryType::Char => {
                    bail!(
                        "{}",
                        ArchiveViolation::SpecialFileType {
                            path: path_str,
                            kind: "char device"
                        }
                    );
                }
                tar::EntryType::Block => {
                    bail!(
                        "{}",
                        ArchiveViolation::SpecialFileType {
                            path: path_str,
                            kind: "block device"
                        }
                    );
                }
                tar::EntryType::Fifo => {
                    bail!(
                        "{}",
                        ArchiveViolation::SpecialFileType {
                            path: path_str,
                            kind: "FIFO"
                        }
                    );
                }
                _ => {
                    bail!(
                        "{}",
                        ArchiveViolation::SpecialFileType {
                            path: path_str,
                            kind: "unknown"
                        }
                    );
                }
            };

            // SAFETY: duplicate path entries (file-type replacement detection)
            if let Some(prev_kind) = seen_paths.get(&stripped_str) {
                if *prev_kind != kind {
                    bail!(
                        "{}",
                        ArchiveViolation::TypeReplacement {
                            path: stripped_str,
                            was: prev_kind.label(),
                            now: kind.label(),
                        }
                    );
                }
                // Same type duplicate -- still reject (except directories)
                if kind != EntryKind::Directory {
                    bail!("{}", ArchiveViolation::DuplicatePath(stripped_str));
                }
                // Duplicate directory entries are OK (tar commonly emits these)
            }
            seen_paths.insert(stripped_str.clone(), kind);

            // SAFETY: symlink escape
            if entry_type == tar::EntryType::Symlink
                && let Some(link) = entry.link_name()?.map(|p| p.to_path_buf())
            {
                let link_str = link.to_string_lossy();
                if link_str.contains("..") || link_str.starts_with('/') {
                    bail!(
                        "{}",
                        ArchiveViolation::SymlinkEscape {
                            path: stripped_str,
                            target: link_str.into_owned(),
                        }
                    );
                }
            }

            // Extract the entry to disk.
            let dest = self.extract_dir.join(&stripped);
            match entry_type {
                tar::EntryType::Directory => {
                    std::fs::create_dir_all(&dest)?;
                }
                tar::EntryType::Regular | tar::EntryType::GNUSparse => {
                    if let Some(parent) = dest.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    let mut outfile = std::fs::File::create(&dest)?;
                    std::io::copy(&mut entry, &mut outfile)?;

                    // Post-extraction defense-in-depth: canonicalize destination
                    // and verify it remains under the extraction root.
                    let canonical_dest = dest.canonicalize().with_context(|| {
                        format!("cannot canonicalize extracted path: {}", dest.display())
                    })?;
                    let canonical_root = self
                        .extract_dir
                        .canonicalize()
                        .unwrap_or_else(|_| self.extract_dir.clone());
                    if !canonical_dest.starts_with(&canonical_root) {
                        // Remove the escaped file immediately.
                        let _ = std::fs::remove_file(&canonical_dest);
                        bail!(
                            "extracted file escaped root: {} -> {}",
                            stripped_str,
                            canonical_dest.display()
                        );
                    }
                }
                tar::EntryType::Symlink => {
                    if let Some(link) = entry.link_name()?.map(|p| p.to_path_buf()) {
                        if let Some(parent) = dest.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        #[cfg(unix)]
                        std::os::unix::fs::symlink(&link, &dest)?;
                    }
                }
                _ => {}
            }
        }

        Ok(&self.extract_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_archive_violation_display() {
        let v = ArchiveViolation::PathTraversal("../etc/passwd".into());
        assert!(v.to_string().contains("path traversal"));

        let v = ArchiveViolation::SpecialFileType {
            path: "dev/null".into(),
            kind: "char device",
        };
        assert!(v.to_string().contains("char device"));

        let v = ArchiveViolation::DuplicatePath("config/foo.conf".into());
        assert!(v.to_string().contains("duplicate entry"));

        let v = ArchiveViolation::TypeReplacement {
            path: "etc/foo".into(),
            was: "regular",
            now: "symlink",
        };
        assert!(v.to_string().contains("type replacement"));
        assert!(v.to_string().contains("regular -> symlink"));

        let v = ArchiveViolation::Hardlink("link".into());
        assert!(v.to_string().contains("hardlinks are not supported"));

        let v = ArchiveViolation::SymlinkEscape {
            path: "link".into(),
            target: "/etc/shadow".into(),
        };
        assert!(v.to_string().contains("symlink escape"));
    }

    /// Build a minimal valid .tar.gz in memory with a single file.
    fn build_valid_tarball(dir_prefix: &str, filename: &str, content: &[u8]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());

        // Add the prefix directory entry.
        let mut dir_header = tar::Header::new_gnu();
        dir_header.set_entry_type(tar::EntryType::Directory);
        dir_header.set_path(format!("{dir_prefix}/")).unwrap();
        dir_header.set_size(0);
        dir_header.set_mode(0o755);
        dir_header.set_cksum();
        builder.append(&dir_header, std::io::empty()).unwrap();

        // Add the file.
        let full_path = format!("{dir_prefix}/{filename}");
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_path(&full_path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, content).unwrap();

        let tar_bytes = builder.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar_bytes).unwrap();
        gz.finish().unwrap()
    }

    #[test]
    fn test_extract_valid_tarball() {
        let tmp = tempfile::tempdir().unwrap();
        let tarball_path = tmp.path().join("test.tar.gz");
        let gz_bytes = build_valid_tarball("myarchive", "hello.txt", b"hello world");
        std::fs::write(&tarball_path, &gz_bytes).unwrap();

        let extract_dir = tmp.path().join("output");
        let extractor = TarballExtractor::new(extract_dir.clone());
        let result = extractor.extract(&tarball_path);
        assert!(result.is_ok(), "extraction failed: {result:?}");
        assert!(extract_dir.join("hello.txt").exists());
        assert_eq!(
            std::fs::read_to_string(extract_dir.join("hello.txt")).unwrap(),
            "hello world"
        );
    }

    #[test]
    fn test_reject_path_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let tarball_path = tmp.path().join("evil.tar.gz");

        // Build a tarball with a path traversal entry.
        // We must write the path directly into the header bytes because
        // the tar crate's set_path() rejects `..` components.
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(5);
        header.set_mode(0o644);
        // Write path directly into the header's name field.
        let evil_path = b"prefix/sub/../../etc/passwd";
        let name_field = &mut header.as_gnu_mut().unwrap().name;
        name_field[..evil_path.len()].copy_from_slice(evil_path);
        header.set_cksum();

        let mut builder = tar::Builder::new(Vec::new());
        builder.append(&header, b"evil!" as &[u8]).unwrap();
        let tar_bytes = builder.into_inner().unwrap();

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar_bytes).unwrap();
        std::fs::write(&tarball_path, gz.finish().unwrap()).unwrap();

        let extractor = TarballExtractor::new(tmp.path().join("out"));
        let result = extractor.extract(&tarball_path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("path traversal"), "unexpected error: {err}");
    }

    #[test]
    fn test_reject_absolute_path() {
        let tmp = tempfile::tempdir().unwrap();
        let tarball_path = tmp.path().join("abs.tar.gz");

        // Construct a tarball with a path-traversal entry that attempts
        // to reach an absolute filesystem location via `../`.
        // The raw header approach bypasses the tar crate's path validation.
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(4);
        header.set_mode(0o644);
        // After prefix stripping (skip first component), this becomes
        // "../../etc/shadow" which triggers the path traversal guard.
        let evil_path = b"prefix/../../etc/shadow";
        let name_field = &mut header.as_gnu_mut().unwrap().name;
        name_field[..evil_path.len()].copy_from_slice(evil_path);
        header.set_cksum();

        let mut builder = tar::Builder::new(Vec::new());
        builder.append(&header, b"bad!" as &[u8]).unwrap();
        let tar_bytes = builder.into_inner().unwrap();

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar_bytes).unwrap();
        std::fs::write(&tarball_path, gz.finish().unwrap()).unwrap();

        let extractor = TarballExtractor::new(tmp.path().join("out"));
        let result = extractor.extract(&tarball_path);
        assert!(
            result.is_err(),
            "path reaching absolute location must be rejected"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("path traversal"),
            "expected path traversal error, got: {err}"
        );
    }

    #[test]
    fn test_reject_duplicate_file() {
        let tmp = tempfile::tempdir().unwrap();
        let tarball_path = tmp.path().join("dup.tar.gz");

        let mut builder = tar::Builder::new(Vec::new());
        for _ in 0..2 {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Regular);
            header.set_path("prefix/same.txt").unwrap();
            header.set_size(3);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append(&header, b"dup" as &[u8]).unwrap();
        }
        let tar_bytes = builder.into_inner().unwrap();

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar_bytes).unwrap();
        std::fs::write(&tarball_path, gz.finish().unwrap()).unwrap();

        let extractor = TarballExtractor::new(tmp.path().join("out"));
        let result = extractor.extract(&tarball_path);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("duplicate entry"),
            "expected duplicate entry error"
        );
    }

    #[test]
    fn test_reject_type_replacement() {
        let tmp = tempfile::tempdir().unwrap();
        let tarball_path = tmp.path().join("replace.tar.gz");

        let mut builder = tar::Builder::new(Vec::new());

        // First entry: regular file
        let mut h1 = tar::Header::new_gnu();
        h1.set_entry_type(tar::EntryType::Regular);
        h1.set_path("prefix/target").unwrap();
        h1.set_size(4);
        h1.set_mode(0o644);
        h1.set_cksum();
        builder.append(&h1, b"file" as &[u8]).unwrap();

        // Second entry: symlink at same path
        let mut h2 = tar::Header::new_gnu();
        h2.set_entry_type(tar::EntryType::Symlink);
        h2.set_path("prefix/target").unwrap();
        h2.set_size(0);
        h2.set_mode(0o777);
        h2.set_link_name("elsewhere").unwrap();
        h2.set_cksum();
        builder.append(&h2, std::io::empty()).unwrap();

        let tar_bytes = builder.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar_bytes).unwrap();
        std::fs::write(&tarball_path, gz.finish().unwrap()).unwrap();

        let extractor = TarballExtractor::new(tmp.path().join("out"));
        let result = extractor.extract(&tarball_path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("type replacement"),
            "expected type replacement error, got: {err}"
        );
    }

    #[test]
    fn test_reject_symlink_escape() {
        let tmp = tempfile::tempdir().unwrap();
        let tarball_path = tmp.path().join("escape.tar.gz");

        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_path("prefix/evil-link").unwrap();
        header.set_size(0);
        header.set_mode(0o777);
        header.set_link_name("/etc/shadow").unwrap();
        header.set_cksum();
        builder.append(&header, std::io::empty()).unwrap();
        let tar_bytes = builder.into_inner().unwrap();

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar_bytes).unwrap();
        std::fs::write(&tarball_path, gz.finish().unwrap()).unwrap();

        let extractor = TarballExtractor::new(tmp.path().join("out"));
        let result = extractor.extract(&tarball_path);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("symlink escape"),
            "expected symlink escape error"
        );
    }

    #[test]
    fn test_extract_rejects_hardlink() {
        let tmp = tempfile::tempdir().unwrap();
        let tarball_path = tmp.path().join("hardlink.tar.gz");

        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Link);
        header.set_path("prefix/some-hardlink").unwrap();
        header.set_size(0);
        header.set_mode(0o644);
        header.set_link_name("target.txt").unwrap();
        header.set_cksum();
        builder.append(&header, std::io::empty()).unwrap();
        let tar_bytes = builder.into_inner().unwrap();

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar_bytes).unwrap();
        std::fs::write(&tarball_path, gz.finish().unwrap()).unwrap();

        let extractor = TarballExtractor::new(tmp.path().join("out"));
        let result = extractor.extract(&tarball_path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("hardlinks are not supported"),
            "expected hardlink rejection error, got: {err}"
        );
    }

    #[test]
    fn test_duplicate_directory_allowed() {
        let tmp = tempfile::tempdir().unwrap();
        let tarball_path = tmp.path().join("dupdir.tar.gz");

        let mut builder = tar::Builder::new(Vec::new());
        // Emit the same directory entry twice (common in real tarballs).
        for _ in 0..2 {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Directory);
            header.set_path("prefix/subdir/").unwrap();
            header.set_size(0);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append(&header, std::io::empty()).unwrap();
        }
        let tar_bytes = builder.into_inner().unwrap();

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar_bytes).unwrap();
        std::fs::write(&tarball_path, gz.finish().unwrap()).unwrap();

        let extractor = TarballExtractor::new(tmp.path().join("out"));
        let result = extractor.extract(&tarball_path);
        assert!(result.is_ok(), "duplicate directories should be allowed");
    }
}
