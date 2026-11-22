use inspectah_core::machine_state::{Screened, ScreenedExecutor};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::nonrpm::{FileType, UnmanagedFile};
use std::collections::BTreeMap;

use super::configtree::{unmanaged_withheld, validate_path};
use super::language_packages::sanitize_for_comment;
use super::safety::{CopyToken, has_line_break_or_control};

/// Single-quote a path for safe shell interpolation in RUN directives.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// `path` proved for a bare `COPY` argument. `validate_path` is asked
/// first, as the config tree stager asks it, so the two refuse alike.
fn copy_path(path: &str) -> Result<CopyToken, String> {
    validate_path(path.trim_start_matches('/')).map_err(|e| e.to_string())?;
    CopyToken::parse(path).map_err(|e| e.to_string())
}

/// Why `ln -sf` cannot be emitted for a symlink, if it cannot. Both
/// operands are single-quoted, which keeps every byte but a line break
/// inert to the shell, and a line break inside the quotes still ends the
/// Containerfile instruction. The link path must also pass
/// `validate_path`; the target may be relative and may climb with `..`.
fn symlink_refusal(file: &UnmanagedFile) -> Option<String> {
    if let Err(e) = validate_path(file.path.trim_start_matches('/')) {
        return Some(format!("the link path is refused: {e}"));
    }
    if has_line_break_or_control(&file.link_target) {
        return Some("the target holds a line break or control character".into());
    }
    None
}

/// The scan root that the bootc target family links into `/var`.
///
/// On fedora-bootc, centos-bootc and rhel-bootc `/srv` is a symlink to
/// `/var/srv`, so a `COPY` into it writes into `/var/srv`, which is unpacked
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
        "# Not copied: /srv is a symlink to /var/srv on bootc images, so content".into(),
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

    // A file under `/etc/systemd/system`, or at the machine-id, is
    // withheld, and the configuration section names it in a FIXME
    // (`staged_writes`).
    let included: Vec<&UnmanagedFile> = section
        .items
        .iter()
        .filter(|f| f.disposition.is_included() && unmanaged_withheld(&f.path).is_none())
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
        // Separate symlinks from regular files for distinct rendering.
        let symlinks: Vec<&&UnmanagedFile> = files
            .iter()
            .filter(|f| f.file_type == FileType::Symlink)
            .collect();
        let regular: Vec<&&UnmanagedFile> = files
            .iter()
            .filter(|f| f.file_type != FileType::Symlink)
            .collect();

        // Every path on a COPY line is a proved `CopyToken`: COPY has no
        // quoting, and a line terminator in a path would end the
        // instruction and start one the snapshot chose.
        if regular.len() > 1 {
            // Directory-level COPY for regular files
            match copy_path(dir) {
                Ok(path) => {
                    let rel = path.as_str().trim_start_matches('/');
                    lines.push(format!("COPY unmanaged/{rel}/ /{rel}/"));
                }
                Err(why) => lines.push(format!(
                    "# FIXME: unmanaged files under {} not copied: the directory path {why}",
                    sanitize_for_comment(dir)
                )),
            }
        } else {
            for file in &regular {
                match copy_path(&file.path) {
                    Ok(path) => {
                        let rel = path.as_str().trim_start_matches('/');
                        lines.push(format!("COPY unmanaged/{rel} {path}"));
                    }
                    Err(why) => lines.push(format!(
                        "# FIXME: unmanaged file {} not copied: the path {why}",
                        sanitize_for_comment(&file.path)
                    )),
                }
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
            } else if let Some(why) = symlink_refusal(file) {
                lines.push(format!(
                    "# FIXME: unmanaged symlink {} -> {} not recreated: {why}",
                    sanitize_for_comment(&file.path),
                    sanitize_for_comment(&file.link_target)
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

/// Copy each included unmanaged file into `render_dir/unmanaged/`, for
/// the scan tarball. `host_root` is where the host's filesystem is
/// (`/` on a real scan); item paths are host paths below it.
///
/// Symlinks are recreated as symlinks, not followed, so nothing outside
/// the scan roots is copied through one.
///
/// Every included regular file is first asked of the scan's read gate
/// again, through the walk's own check
/// ([`ScreenedExecutor::screen_payload`]). The walk already moved every
/// withheld file out of `items`, so a withheld answer here means the file
/// changed after the walk, and a read error means it can no longer be
/// checked: either fails the bundle with an error naming the path, before
/// anything is copied. A file is never skipped, which would leave a
/// `COPY` with no source.
pub fn bundle_unmanaged_files(
    items: &[UnmanagedFile],
    render_dir: &std::path::Path,
    host_root: &std::path::Path,
    gate: &ScreenedExecutor<'_>,
) -> anyhow::Result<()> {
    use anyhow::Context;

    let payloads: Vec<&UnmanagedFile> = items
        .iter()
        .filter(|item| item.disposition.is_included() && unmanaged_withheld(&item.path).is_none())
        .collect();
    for item in &payloads {
        if item.file_type == FileType::Symlink {
            continue;
        }
        match gate.screen_payload(&item.path) {
            Ok(Screened::Withheld(member)) => anyhow::bail!(
                "{} became a machine-state file ({}) after the scan walked it; re-run the scan",
                item.path,
                member.as_str()
            ),
            Ok(_) => {}
            Err(e) => anyhow::bail!(
                "{} could not be checked for credentials before copying it ({}); \
                 re-run the scan",
                item.path,
                e.kind()
            ),
        }
    }
    for item in payloads {
        let rel_path = item.path.trim_start_matches('/');
        let dest = render_dir.join("unmanaged").join(rel_path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create dir for {}", dest.display()))?;
        }
        let source = host_root.join(rel_path);
        if item.file_type == FileType::Symlink {
            // Recreate the link rather than following its target, from
            // the recorded target when there is one.
            let target = if !item.link_target.is_empty() {
                item.link_target.clone()
            } else {
                std::fs::read_link(&source)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default()
            };
            if !target.is_empty() {
                std::os::unix::fs::symlink(&target, &dest)
                    .with_context(|| format!("failed to recreate symlink {}", item.path))?;
            }
        } else {
            std::fs::copy(&source, &dest)
                .with_context(|| format!("failed to copy {} to tarball", item.path))?;
        }
    }
    Ok(())
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
                withheld: Vec::new(),
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
            lines.iter().any(|l| l.contains("symlink to /var/srv")),
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
            !lines.iter().any(|l| l.contains("symlink to /var/srv")),
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

    fn payload(path: &str, file_type: FileType) -> UnmanagedFile {
        UnmanagedFile {
            path: path.into(),
            file_type,
            disposition: FindingKind::included(),
            ..Default::default()
        }
    }

    fn link(path: &str, target: &str) -> UnmanagedFile {
        UnmanagedFile {
            link_target: target.into(),
            ..payload(path, FileType::Symlink)
        }
    }

    /// The bundler fails, naming the path and writing nothing, for an item
    /// the gate now withholds: by inode (a hard link to a key), by content
    /// (a keytab header, a whole-file key) and one it cannot read to check.
    #[test]
    fn bundling_fails_on_a_payload_the_gate_withholds_or_cannot_check() {
        use inspectah_collect::executor::mock::MockExecutor;
        use inspectah_core::machine_state::ScreenedExecutor;
        // One base64 line, as long as the smallest real key's body.
        const KEY: &str = "-----BEGIN PRIVATE KEY-----\n\
            MC4CAQAwBQYDK2VwBCIEIBQh0WfR6aXEBHdgfX4ix5qDYmYRBZGZEmNFdzJ1Owlp\n\
            -----END PRIVATE KEY-----\n";
        let host = MockExecutor::new()
            .with_dir("/etc/pki/tls/private", vec!["server.key"])
            .with_file("/etc/pki/tls/private/server.key", KEY)
            .with_hard_link("/opt/app/k", "/etc/pki/tls/private/server.key")
            .with_file_bytes("/opt/app/client.kt", &[0x05, 0x02, 0xff, 0x00])
            .with_file("/opt/app/client.pem", KEY);
        let gate = ScreenedExecutor::new(&host);
        let host_root = tempfile::tempdir().expect("a host root");
        for path in [
            "/opt/app/k",
            "/opt/app/client.kt",
            "/opt/app/client.pem",
            "/opt/app/gone",
        ] {
            let render = tempfile::tempdir().expect("a render directory");
            let items = vec![payload(path, FileType::DataFile)];
            let err = bundle_unmanaged_files(&items, render.path(), host_root.path(), &gate)
                .expect_err("the bundle fails");
            assert!(err.to_string().contains(path), "{err}");
            assert!(!render.path().join("unmanaged").exists(), "{path}");
        }
    }

    /// D4 on the scan route: the bundler writes no payload for an included
    /// unmanaged file or symlink under `/etc/systemd/system`, however the
    /// path is spelled. Controls: an ordinary file, binary and not UTF-8,
    /// is copied byte for byte from the host root, and an ordinary symlink
    /// is recreated.
    #[test]
    fn bundling_writes_no_payload_under_the_unit_directory() {
        use inspectah_collect::executor::mock::MockExecutor;
        use inspectah_core::machine_state::ScreenedExecutor;
        const CONTROL_BYTES: &[u8] = b"\x7fELF\x02\x01\xff\xfe binary";
        const CONTROL: &str = "/opt/app/run";
        let host_root = tempfile::tempdir().expect("a host root");
        std::fs::create_dir_all(host_root.path().join("opt/app")).unwrap();
        std::fs::write(host_root.path().join("opt/app/run"), CONTROL_BYTES).unwrap();
        let host = MockExecutor::new().with_file_bytes(CONTROL, CONTROL_BYTES);
        let gate = ScreenedExecutor::new(&host);
        let render = tempfile::tempdir().expect("a render directory");

        let items = vec![
            payload("/etc/systemd/system/backup.service", FileType::Script),
            payload("/etc/systemd/./system/dotted.service", FileType::Script),
            payload("/etc//systemd/system/doubled.service", FileType::Script),
            payload("/etc/systemd/system/./here.service", FileType::Script),
            link(
                "/etc/systemd/system/multi-user.target.wants/backup.service",
                "/usr/lib/systemd/system/backup.service",
            ),
            link(
                "/etc/systemd//system/linked.service",
                "/usr/lib/systemd/system/linked.service",
            ),
            payload(CONTROL, FileType::ElfBinary),
            link("/opt/app/current", "/opt/app/releases/1"),
        ];
        bundle_unmanaged_files(&items, render.path(), host_root.path(), &gate)
            .expect("bundling succeeds");

        let unmanaged = render.path().join("unmanaged");
        assert!(!unmanaged.join("etc").exists(), "no payload under etc/");
        assert_eq!(
            std::fs::read(unmanaged.join("opt/app/run")).expect("the control file is bundled"),
            CONTROL_BYTES
        );
        assert_eq!(
            std::fs::read_link(unmanaged.join("opt/app/current"))
                .expect("the control symlink is bundled"),
            std::path::Path::new("/opt/app/releases/1")
        );
    }
}
