use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::nonrpm::{FileType, UnmanagedFile};
use std::collections::BTreeMap;

/// Single-quote a path for safe shell interpolation in RUN directives.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The scan root that the bootc target family links into `/var`.
///
/// On fedora-bootc, centos-bootc and rhel-bootc `/srv` is a symlink to
/// `var/srv`, so a `COPY` into it writes into `/var/srv`, which is unpacked
/// once on first boot and never touched by a later image update.
const VAR_BACKED_SCAN_ROOT: &str = "srv";

/// Where an included unmanaged finding lands on a bootc target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnmanagedDestination {
    /// A plain image directory (`/opt`, `/usr/local`): the Containerfile
    /// carries it with `COPY`.
    Image,
    /// Under `/srv`, which resolves into `/var/srv`: machine-local data
    /// for the `/var` strategy, never a `COPY`.
    VarSrv,
}

impl UnmanagedDestination {
    /// Classify a finding by its path, matching whole path components so
    /// that `/srvdata` is not mistaken for `/srv`.
    pub fn of(path: &str) -> Self {
        if std::path::Path::new(path.trim_start_matches('/')).starts_with(VAR_BACKED_SCAN_ROOT) {
            Self::VarSrv
        } else {
            Self::Image
        }
    }
}

/// Paths of included unmanaged findings that live under `/srv`, in
/// snapshot order. The Containerfile and the audit report both name them.
pub fn included_var_srv_paths(snap: &InspectionSnapshot) -> Vec<&str> {
    snap.unmanaged_files
        .iter()
        .flat_map(|s| &s.items)
        .filter(|f| {
            f.disposition.is_included()
                && UnmanagedDestination::of(&f.path) == UnmanagedDestination::VarSrv
        })
        .map(|f| f.path.as_str())
        .collect()
}

/// Comment block that replaces `COPY` for `/srv` findings.
fn var_srv_comment_lines(paths: &[&str]) -> Vec<String> {
    let mut lines = vec![
        "#".into(),
        "# Not copied: /srv is a symlink to var/srv on bootc images, so content".into(),
        "# there is machine-local data that belongs to the /var lifecycle, not".into(),
        "# image content. A COPY would land in /var/srv once, on first boot, and".into(),
        "# no later image update would change it. Carry it with your /var".into(),
        "# strategy: restore it into /var/srv after deployment, or give it its".into(),
        "# own mount. The files are in this export under unmanaged/srv/.".into(),
    ];
    lines.extend(paths.iter().map(|p| format!("#   {p}")));
    lines
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

    let included: Vec<&UnmanagedFile> = section
        .items
        .iter()
        .filter(|f| f.disposition.is_included())
        .collect();

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
    for file in included
        .iter()
        .filter(|f| UnmanagedDestination::of(&f.path) == UnmanagedDestination::Image)
    {
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

    let var_srv = included_var_srv_paths(snap);
    if !var_srv.is_empty() {
        lines.extend(var_srv_comment_lines(&var_srv));
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::FindingKind;
    use inspectah_core::types::nonrpm::{FileType, UnmanagedFile, UnmanagedFileSection};

    fn test_snapshot_with_unmanaged(items: Vec<UnmanagedFile>) -> InspectionSnapshot {
        let total_size = items.iter().map(|f| f.size).sum();
        let total_count = items.len();
        InspectionSnapshot {
            unmanaged_files: Some(UnmanagedFileSection {
                items,
                usr_entries: Vec::new(),
                total_size,
                total_count,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn renders_copy_with_warning_block() {
        let snap = test_snapshot_with_unmanaged(vec![UnmanagedFile {
            path: "/opt/splunk/bin/splunkd".into(),
            size: 1024,
            file_type: FileType::ElfBinary,
            disposition: FindingKind::included(),
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
            disposition: FindingKind::excluded(),
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
                disposition: FindingKind::included(),
                ..Default::default()
            },
            UnmanagedFile {
                path: "/opt/splunk/bin/btool".into(),
                disposition: FindingKind::included(),
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
            disposition: FindingKind::included(),
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
                disposition: FindingKind::included(),
                ..Default::default()
            },
            UnmanagedFile {
                path: "/opt/myapp/bin/debug".into(),
                file_type: FileType::Symlink,
                link_target: "/opt/myapp/lib/debug-2.0".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ]);
        let lines = unmanaged_file_lines(&snap);
        let ln_lines: Vec<_> = lines.iter().filter(|l| l.contains("RUN ln -sf")).collect();
        assert_eq!(ln_lines.len(), 2, "each symlink must produce a RUN ln -sf");
    }

    /// `/srv` resolves into `/var/srv` on the bootc target family, so a
    /// finding there gets the `/var` strategy note instead of `COPY` or
    /// `ln -sf`, while `/opt` beside it still copies.
    #[test]
    fn srv_findings_route_to_var_strategy_not_copy() {
        let snap = test_snapshot_with_unmanaged(vec![
            UnmanagedFile {
                path: "/srv/www/index.html".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            UnmanagedFile {
                path: "/srv/www/app.css".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            UnmanagedFile {
                path: "/srv/www/current".into(),
                file_type: FileType::Symlink,
                link_target: "/srv/www/index.html".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            UnmanagedFile {
                path: "/opt/app/server".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ]);
        let lines = unmanaged_file_lines(&snap);
        assert!(
            !lines
                .iter()
                .any(|l| !l.starts_with('#') && l.contains("srv")),
            "no directive may name /srv, got: {lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.contains("symlink to var/srv")),
            "the /var strategy note must be present, got: {lines:?}"
        );
        for path in [
            "/srv/www/index.html",
            "/srv/www/app.css",
            "/srv/www/current",
        ] {
            assert!(
                lines.iter().any(|l| l == &format!("#   {path}")),
                "{path} must be listed, got: {lines:?}"
            );
        }
        assert!(
            lines.contains(&"COPY unmanaged/opt/app/server /opt/app/server".to_string()),
            "/opt finding must still COPY, got: {lines:?}"
        );
    }

    #[test]
    fn destination_matches_whole_components() {
        let cases = [
            ("/srv", UnmanagedDestination::VarSrv),
            ("/srv/data/file", UnmanagedDestination::VarSrv),
            ("srv/data/file", UnmanagedDestination::VarSrv),
            ("/srvdata/file", UnmanagedDestination::Image),
            ("/opt/srv/file", UnmanagedDestination::Image),
            ("/usr/local/bin/tool", UnmanagedDestination::Image),
        ];
        for (path, expected) in cases {
            assert_eq!(UnmanagedDestination::of(path), expected, "{path}");
        }
    }

    #[test]
    fn excluded_srv_finding_renders_nothing() {
        let snap = test_snapshot_with_unmanaged(vec![UnmanagedFile {
            path: "/srv/www/index.html".into(),
            disposition: FindingKind::excluded(),
            ..Default::default()
        }]);
        assert!(unmanaged_file_lines(&snap).is_empty());
    }

    /// With an included sibling the section renders, so the early return
    /// no longer hides an excluded `/srv` finding: the toggle itself must
    /// keep it out of the `/var` strategy list.
    #[test]
    fn excluded_srv_finding_is_not_listed_beside_included_ones() {
        let snap = test_snapshot_with_unmanaged(vec![
            UnmanagedFile {
                path: "/srv/www/index.html".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            UnmanagedFile {
                path: "/srv/www/excluded.html".into(),
                disposition: FindingKind::excluded(),
                ..Default::default()
            },
            UnmanagedFile {
                path: "/opt/app/server".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ]);
        let lines = unmanaged_file_lines(&snap);
        assert!(
            lines.iter().any(|l| l == "#   /srv/www/index.html"),
            "included /srv finding must be listed, got: {lines:?}"
        );
        assert!(
            !lines.iter().any(|l| l.contains("excluded.html")),
            "excluded /srv finding must not appear, got: {lines:?}"
        );
    }

    /// An `/opt` symlink may point into `/srv`; it is still an image path
    /// and keeps its `ln -sf`, and no `/var` strategy note is emitted.
    #[test]
    fn opt_symlink_targeting_srv_is_still_recreated() {
        let snap = test_snapshot_with_unmanaged(vec![UnmanagedFile {
            path: "/opt/app/data".into(),
            file_type: FileType::Symlink,
            link_target: "/srv/app/data".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }]);
        let lines = unmanaged_file_lines(&snap);
        assert!(
            lines.contains(&"RUN ln -sf '/srv/app/data' '/opt/app/data'".to_string()),
            "got: {lines:?}"
        );
        assert!(
            !lines.iter().any(|l| l.contains("symlink to var/srv")),
            "an /opt finding must not get the /srv note, got: {lines:?}"
        );
    }

    #[test]
    fn mixed_regular_and_symlink_dir() {
        let snap = test_snapshot_with_unmanaged(vec![
            UnmanagedFile {
                path: "/opt/app/server".into(),
                file_type: FileType::ElfBinary,
                disposition: FindingKind::included(),
                ..Default::default()
            },
            UnmanagedFile {
                path: "/opt/app/current".into(),
                file_type: FileType::Symlink,
                link_target: "/opt/app/server".into(),
                disposition: FindingKind::included(),
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
