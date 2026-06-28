use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::nonrpm::UnmanagedFile;
use std::collections::BTreeMap;

/// Render Containerfile lines for unmanaged files.
///
/// Groups files by parent directory for readability.
/// Includes warning block per spec.
pub fn unmanaged_file_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let section = match &snap.unmanaged_files {
        Some(s) if !s.items.is_empty() => s,
        _ => return Vec::new(),
    };

    let included: Vec<&UnmanagedFile> = section.items.iter().filter(|f| f.include).collect();

    if included.is_empty() {
        return Vec::new();
    }

    let mut lines = Vec::new();
    lines.push(String::new());
    lines.push("# === Unmanaged files (no package manager provenance) ===".into());
    lines.push("# These files were copied directly from the source host. They have".into());
    lines.push("# no upstream update path and must be manually maintained.".into());

    // Group by parent directory
    let mut groups: BTreeMap<String, Vec<&UnmanagedFile>> = BTreeMap::new();
    for file in &included {
        let parent = std::path::Path::new(&file.path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());
        groups.entry(parent).or_default().push(file);
    }

    for (dir, files) in &groups {
        let rel_dir = dir.trim_start_matches('/');
        if files.len() > 1 {
            // Directory-level COPY
            lines.push(format!("COPY unmanaged/{rel_dir}/ /{rel_dir}/"));
        } else {
            // Single file COPY
            for file in files {
                let rel_path = file.path.trim_start_matches('/');
                lines.push(format!("COPY unmanaged/{rel_path} {}", file.path));
            }
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::nonrpm::{FileType, UnmanagedFile, UnmanagedFileSection};

    fn test_snapshot_with_unmanaged(items: Vec<UnmanagedFile>) -> InspectionSnapshot {
        let total_size = items.iter().map(|f| f.size).sum();
        let total_count = items.len();
        let mut snap = InspectionSnapshot::default();
        snap.unmanaged_files = Some(UnmanagedFileSection {
            items,
            total_size,
            total_count,
        });
        snap
    }

    #[test]
    fn renders_copy_with_warning_block() {
        let snap = test_snapshot_with_unmanaged(vec![UnmanagedFile {
            path: "/opt/splunk/bin/splunkd".into(),
            size: 1024,
            file_type: FileType::ElfBinary,
            include: true,
            ..Default::default()
        }]);
        let lines = unmanaged_file_lines(&snap);
        assert!(lines.iter().any(|l| l.contains("=== Unmanaged files")));
        assert!(lines.iter().any(|l| l.contains("COPY unmanaged/")));
        assert!(lines.iter().any(|l| l.contains("manually maintained")));
    }

    #[test]
    fn excluded_files_not_rendered() {
        let snap = test_snapshot_with_unmanaged(vec![UnmanagedFile {
            path: "/opt/app/server".into(),
            include: false,
            ..Default::default()
        }]);
        let lines = unmanaged_file_lines(&snap);
        assert!(lines.is_empty());
    }

    #[test]
    fn groups_by_directory() {
        let snap = test_snapshot_with_unmanaged(vec![
            UnmanagedFile {
                path: "/opt/splunk/bin/splunkd".into(),
                include: true,
                ..Default::default()
            },
            UnmanagedFile {
                path: "/opt/splunk/bin/btool".into(),
                include: true,
                ..Default::default()
            },
        ]);
        let lines = unmanaged_file_lines(&snap);
        assert!(
            lines
                .iter()
                .any(|l| l.contains("COPY unmanaged/opt/splunk/bin/ /opt/splunk/bin/"))
        );
    }
}
