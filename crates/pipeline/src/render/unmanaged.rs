use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::nonrpm::{FileType, UnmanagedFile};
use std::collections::BTreeMap;

/// Single-quote a path for safe shell interpolation in RUN directives.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

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

    let mut lines = vec![
        String::new(),
        "# === Unmanaged files (no package manager provenance) ===".into(),
        "# These files were copied directly from the source host. They have".into(),
        "# no upstream update path and must be manually maintained.".into(),
    ];

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
        // Separate symlinks from regular files for distinct rendering.
        let symlinks: Vec<&&UnmanagedFile> = files
            .iter()
            .filter(|f| f.file_type == FileType::Symlink)
            .collect();
        let regular: Vec<&&UnmanagedFile> = files
            .iter()
            .filter(|f| f.file_type != FileType::Symlink)
            .collect();

        if regular.len() > 1 {
            // Directory-level COPY for regular files
            lines.push(format!("COPY unmanaged/{rel_dir}/ /{rel_dir}/"));
        } else {
            for file in &regular {
                let rel_path = file.path.trim_start_matches('/');
                lines.push(format!("COPY unmanaged/{rel_path} {}", file.path));
            }
        }

        // Symlinks are recreated with ln -sf so the image preserves
        // the original link topology. Unknown targets get advisory comments.
        for file in &symlinks {
            if file.link_target.is_empty() {
                lines.push(format!(
                    "# SYMLINK: {} -> unknown (recreate manually)",
                    file.path
                ));
            } else {
                lines.push(format!(
                    "RUN ln -sf {} {}",
                    shell_escape(&file.link_target),
                    shell_escape(&file.path)
                ));
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

    #[test]
    fn symlinks_render_as_ln_sf() {
        let snap = test_snapshot_with_unmanaged(vec![UnmanagedFile {
            path: "/opt/app/bin/tool".into(),
            file_type: FileType::Symlink,
            link_target: "/opt/app/lib/tool-1.2".into(),
            include: true,
            ..Default::default()
        }]);
        let lines = unmanaged_file_lines(&snap);
        assert!(
            lines
                .iter()
                .any(|l| l.contains("RUN ln -sf") && l.contains("/opt/app/lib/tool-1.2")),
            "symlink must produce RUN ln -sf, got: {lines:?}"
        );
    }

    #[test]
    fn symlink_only_dir_produces_output() {
        let snap = test_snapshot_with_unmanaged(vec![
            UnmanagedFile {
                path: "/opt/myapp/bin/run".into(),
                file_type: FileType::Symlink,
                link_target: "/opt/myapp/lib/run-2.0".into(),
                include: true,
                ..Default::default()
            },
            UnmanagedFile {
                path: "/opt/myapp/bin/debug".into(),
                file_type: FileType::Symlink,
                link_target: "/opt/myapp/lib/debug-2.0".into(),
                include: true,
                ..Default::default()
            },
        ]);
        let lines = unmanaged_file_lines(&snap);
        let ln_lines: Vec<_> = lines.iter().filter(|l| l.contains("RUN ln -sf")).collect();
        assert_eq!(ln_lines.len(), 2, "each symlink must produce a RUN ln -sf");
    }

    #[test]
    fn mixed_regular_and_symlink_dir() {
        let snap = test_snapshot_with_unmanaged(vec![
            UnmanagedFile {
                path: "/opt/app/server".into(),
                file_type: FileType::ElfBinary,
                include: true,
                ..Default::default()
            },
            UnmanagedFile {
                path: "/opt/app/current".into(),
                file_type: FileType::Symlink,
                link_target: "/opt/app/server".into(),
                include: true,
                ..Default::default()
            },
        ]);
        let lines = unmanaged_file_lines(&snap);
        assert!(
            lines.iter().any(|l| l.starts_with("COPY unmanaged/")),
            "regular file must produce COPY"
        );
        assert!(
            lines.iter().any(|l| l.contains("RUN ln -sf")),
            "symlink must produce RUN ln -sf"
        );
    }
}
