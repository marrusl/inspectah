use inspectah_core::machine_state::{Screened, ScreenedExecutor, payload_credential};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::destination::Destination;
use inspectah_core::types::machine_state::MachineStateMember;
use inspectah_core::types::nonrpm::{FileType, PayloadRedaction, UncheckedPayload, UnmanagedFile};
use inspectah_core::types::redaction::{
    Confidence, DetectionMethod, RedactionFinding, RedactionKind, RedactionState,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read, Write};
use std::path::Path;

use crate::redaction::engine::{PayloadRules, record_late_findings, redact_payload};

use super::configtree::{unmanaged_file_withheld, unmanaged_withheld, validate_path};
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
                && unmanaged_file_withheld(f).is_none()
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

    // A file under `/etc/systemd/system`, at the machine-id, or one the
    // bundle could not check is withheld, and the configuration section
    // names it in a FIXME (`staged_writes`).
    let included: Vec<&UnmanagedFile> = section
        .items
        .iter()
        .filter(|f| f.disposition.is_included() && unmanaged_file_withheld(f).is_none())
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
    if included
        .iter()
        .any(|f| f.payload_redaction == PayloadRedaction::Redacted)
    {
        lines
            .push("# Credentials in some were replaced with REDACTED_* placeholders before".into());
        lines.push("# bundling: secrets-review.md says how to supply the real values.".into());
    }

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

/// Largest text payload the bundler reads whole to redact. Larger text is
/// withheld ([`UncheckedPayload::TooLarge`]).
const PAYLOAD_REDACTION_LIMIT: u64 = 8 * 1024 * 1024;

/// Byte-order marks of UTF-16 and UTF-32 text, whose ASCII characters
/// carry NUL bytes.
const WIDE_TEXT_BOMS: &[&[u8]] = &[b"\xff\xfe", b"\xfe\xff", b"\x00\x00\xfe\xff"];

/// Whether a payload is binary, which ships byte for byte: it holds a NUL
/// byte, and does not open with a UTF-16 or UTF-32 byte-order mark. Every
/// other payload is text, which redaction reads or the export withholds,
/// so a file on the boundary is withheld rather than shipped as read.
fn is_binary(head: &[u8], has_nul: bool) -> bool {
    has_nul && !WIDE_TEXT_BOMS.iter().any(|bom| head.starts_with(bom))
}

/// What the bundler did with one regular payload.
enum PayloadCopy {
    /// Written as read: binary, text redaction left alone, or anything
    /// under `--no-redaction`.
    Written,
    /// Written redacted, with its findings.
    Redacted(String, Vec<RedactionFinding>),
    /// Not written: text redaction could not check.
    Unchecked(UncheckedPayload),
    /// Not written: the bytes read hold what the key screening withholds,
    /// so the file changed after the screening passed it.
    KeyFound(MachineStateMember),
}

/// Reads `inner` and writes each byte it hands out to `out`, noting any
/// NUL, so what the key screening reads is exactly what lands in the
/// bundle.
struct TeeReader<R, W> {
    inner: R,
    out: W,
    has_nul: bool,
}

impl<R: Read, W: Write> Read for TeeReader<R, W> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.out.write_all(&buf[..n])?;
        self.has_nul |= buf[..n].contains(&0);
        Ok(n)
    }
}

/// Create `dest` and the directories above it.
fn create_at(dest: &Path) -> io::Result<std::fs::File> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::File::create(dest)
}

/// Write `bytes` to `dest`, the directories above it created.
fn write_at(dest: &Path, bytes: &[u8]) -> io::Result<()> {
    create_at(dest)?.write_all(bytes)
}

/// Stream what `reader` yields into `dest`, screening exactly those bytes
/// for keys as they pass ([`payload_credential`]). Returns the member
/// found, after removing `dest`, and whether any byte was NUL.
fn copy_screened(reader: impl Read, dest: &Path) -> io::Result<(Option<MachineStateMember>, bool)> {
    let mut tee = TeeReader {
        inner: reader,
        out: io::BufWriter::new(create_at(dest)?),
        has_nul: false,
    };
    let member = payload_credential(&mut tee)?;
    tee.out.flush()?;
    if member.is_some() {
        std::fs::remove_file(dest)?;
    }
    Ok((member, tee.has_nul))
}

/// Write the regular payload at `source`, host path `path`, to `dest`,
/// from one open of the file, and say what was written. Every byte
/// written is screened for keys as written, so nothing the screening did
/// not read ships, even if the file changes meanwhile.
///
/// Under `--no-redaction` (`redacting` false) the file streams through as
/// read. Otherwise text is read whole and redacted by `rules`
/// ([`redact_payload`]); binary ([`is_binary`]) ships as read; text that is
/// not UTF-8, or larger than [`PAYLOAD_REDACTION_LIMIT`], is not written.
fn payload_copy(
    source: &Path,
    dest: &Path,
    path: &str,
    redacting: bool,
    rules: PayloadRules,
) -> io::Result<PayloadCopy> {
    let mut file = std::fs::File::open(source)?;
    if !redacting {
        let (member, _) = copy_screened(file, dest)?;
        return Ok(member.map_or(PayloadCopy::Written, PayloadCopy::KeyFound));
    }
    let mut head = Vec::new();
    (&mut file)
        .take(PAYLOAD_REDACTION_LIMIT + 1)
        .read_to_end(&mut head)?;
    if head.len() as u64 > PAYLOAD_REDACTION_LIMIT {
        let (member, has_nul) = copy_screened(io::Cursor::new(&head).chain(file), dest)?;
        if let Some(member) = member {
            return Ok(PayloadCopy::KeyFound(member));
        }
        if is_binary(&head, has_nul) {
            return Ok(PayloadCopy::Written);
        }
        std::fs::remove_file(dest)?;
        return Ok(PayloadCopy::Unchecked(UncheckedPayload::TooLarge));
    }
    if let Some(member) = payload_credential(&mut head.as_slice())? {
        return Ok(PayloadCopy::KeyFound(member));
    }
    if is_binary(&head, head.contains(&0)) {
        write_at(dest, &head)?;
        return Ok(PayloadCopy::Written);
    }
    let Ok(text) = std::str::from_utf8(&head) else {
        return Ok(PayloadCopy::Unchecked(UncheckedPayload::NotUtf8));
    };
    let Some((redacted, findings)) = redact_payload(text, path, rules) else {
        write_at(dest, &head)?;
        return Ok(PayloadCopy::Written);
    };
    // The screening again, over the text that ships.
    if let Some(member) = payload_credential(&mut redacted.as_bytes())? {
        return Ok(PayloadCopy::KeyFound(member));
    }
    write_at(dest, redacted.as_bytes())?;
    Ok(PayloadCopy::Redacted(redacted, findings))
}

/// The destinations of the files whose text the snapshot also holds: an
/// `.env` file's content (written to `env-files/`) and a compose file's
/// `raw_content`. Each was redacted by `redact`, from an earlier read.
fn twin_keys(snap: &InspectionSnapshot) -> BTreeSet<String> {
    let key = |path: &str| Destination::new(path).ok().map(|d| d.as_str().to_owned());
    let env = snap
        .non_rpm_software
        .iter()
        .flat_map(|nrs| &nrs.env_files)
        .filter(|e| !e.content.trim().is_empty())
        .filter_map(|e| key(&e.path));
    let compose = snap
        .containers
        .iter()
        .flat_map(|c| &c.compose_files)
        .filter(|c| c.raw_content.is_some())
        .filter_map(|c| key(&c.path));
    env.chain(compose).collect()
}

/// Make the snapshot's other copies of the file at `key` the bundle's
/// redacted `text` ([`twin_keys`]), so the export holds one text for the
/// file, the one the image gets. The bundle redacted that file by the
/// rules `redact` used for those copies ([`PayloadRules::SnapshotCopy`]),
/// so `text` is never less redacted than what it replaces.
fn adopt_into_twins(snap: &mut InspectionSnapshot, key: &str, text: &str) {
    let at_key = |path: &str| Destination::new(path).is_ok_and(|d| d.as_str() == key);
    if let Some(nrs) = snap.non_rpm_software.as_mut() {
        for env in nrs.env_files.iter_mut().filter(|e| at_key(&e.path)) {
            if !env.content.trim().is_empty() {
                env.content = text.to_owned();
            }
        }
    }
    if let Some(containers) = snap.containers.as_mut() {
        for compose in containers
            .compose_files
            .iter_mut()
            .filter(|c| at_key(&c.path))
        {
            if let Some(raw) = compose.raw_content.as_mut() {
                *raw = text.to_owned();
            }
        }
    }
}

/// The finding that records a payload the bundle left out unchecked.
fn unchecked_finding(path: &str, reason: UncheckedPayload) -> RedactionFinding {
    RedactionFinding {
        path: path.to_owned(),
        source: "unmanaged_payload".into(),
        kind: RedactionKind::Excluded,
        pattern: "unchecked_payload".into(),
        remediation: format!(
            "Not checked for credentials: {}. Left out of the export",
            reason.describe()
        ),
        line: None,
        replacement: None,
        detection_method: DetectionMethod::PathBased,
        confidence: Some(Confidence::High),
        finding_kind: None,
    }
}

/// Copy each included unmanaged file into `render_dir/unmanaged/`, for
/// the scan tarball, and record in `snap` what redaction did to each.
/// `host_root` is where the host's filesystem is (`/` on a real scan);
/// item paths are host paths below it. Runs before the export renders,
/// so the snapshot, the audit report and `secrets-review.md` describe
/// the copies this wrote.
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
/// anything is copied.
///
/// Then each file is opened once and written from that read
/// ([`payload_copy`]), and the key screening runs over the bytes written,
/// so a key that appeared after the first check fails the bundle too.
/// Unless the snapshot is raw (`--no-redaction`), text is redacted as a
/// config file is ([`redact_payload`]) and the redacted text ships: the
/// image gets the `REDACTED_*` placeholders a carried config file gets. A
/// file the snapshot also holds (an `.env` file, a compose file) is
/// redacted by the rules its snapshot copy was and that copy takes the
/// same text ([`adopt_into_twins`]); the bundle's findings replace that
/// copy's inline findings of the same kinds. Text redaction cannot check,
/// not UTF-8 or over [`PAYLOAD_REDACTION_LIMIT`], is not written at all:
/// the item records why ([`PayloadRedaction::Withheld`]), every export
/// surface reads that ([`unmanaged_file_withheld`]), and an excluded
/// finding names it in `secrets-review.md`. Binary files ship byte for
/// byte.
pub fn bundle_unmanaged_files(
    snap: &mut InspectionSnapshot,
    render_dir: &std::path::Path,
    host_root: &std::path::Path,
    gate: &ScreenedExecutor<'_>,
) -> anyhow::Result<()> {
    use anyhow::Context;

    let redacting = !matches!(snap.redaction_state, Some(RedactionState::Raw));
    let twins = twin_keys(snap);
    let Some(section) = snap.unmanaged_files.as_mut() else {
        return Ok(());
    };
    let payloads: Vec<usize> = section
        .items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            item.disposition.is_included() && unmanaged_withheld(&item.path).is_none()
        })
        .map(|(i, _)| i)
        .collect();
    for item in payloads.iter().map(|&i| &section.items[i]) {
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
    let mut findings = Vec::new();
    let mut adopted: Vec<(String, String, Vec<RedactionFinding>)> = Vec::new();
    for i in payloads {
        let item = &mut section.items[i];
        let rel_path = item.path.trim_start_matches('/');
        let dest = render_dir.join("unmanaged").join(rel_path);
        let source = host_root.join(rel_path);
        if item.file_type == FileType::Symlink {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create dir for {}", dest.display()))?;
            }
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
            continue;
        }
        let key = Destination::new(&item.path)
            .ok()
            .map(|d| d.as_str().to_owned());
        let twin = key.as_ref().filter(|k| twins.contains(*k));
        let rules = if twin.is_some() {
            PayloadRules::SnapshotCopy
        } else {
            PayloadRules::Payload
        };
        let copy = payload_copy(&source, &dest, &item.path, redacting, rules)
            .with_context(|| format!("failed to copy {} to tarball", item.path))?;
        match copy {
            PayloadCopy::Written => {}
            PayloadCopy::Redacted(text, found) => {
                item.payload_redaction = PayloadRedaction::Redacted;
                if let Some(key) = twin {
                    adopted.push((key.clone(), text, found.clone()));
                }
                findings.extend(found);
            }
            PayloadCopy::Unchecked(reason) => {
                item.payload_redaction = PayloadRedaction::Withheld(reason);
                findings.push(unchecked_finding(&item.path, reason));
                continue;
            }
            PayloadCopy::KeyFound(member) => anyhow::bail!(
                "{} became a machine-state file ({}) after the scan walked it; re-run the scan",
                item.path,
                member.as_str()
            ),
        }
        // The mode `COPY` gives the image file, an executable bit included.
        let mode = std::fs::metadata(&source)
            .with_context(|| format!("failed to read the mode of {}", item.path))?
            .permissions();
        std::fs::set_permissions(&dest, mode)
            .with_context(|| format!("failed to set the mode of {}", item.path))?;
    }
    // Each adopted file's bundle findings replace the inline findings an
    // earlier pass recorded for its other copies, kind for kind: a kind the
    // bundle did not report, a key among them, keeps its record.
    for (key, text, _) in &adopted {
        adopt_into_twins(snap, key, text);
    }
    snap.redactions.retain(|f| {
        f.kind != RedactionKind::Inline
            || !Destination::new(&f.path).is_ok_and(|d| {
                adopted.iter().any(|(key, _, found)| {
                    key == d.as_str() && found.iter().any(|b| b.finding_kind == f.finding_kind)
                })
            })
    });
    record_late_findings(snap, findings);
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
            let mut snap = test_snapshot_with_unmanaged(vec![payload(path, FileType::DataFile)]);
            let err = bundle_unmanaged_files(&mut snap, render.path(), host_root.path(), &gate)
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
        const CONTROL_BYTES: &[u8] = b"\x7fELF\x02\x01\x01\x00\xff\xfe binary";
        const CONTROL: &str = "/opt/app/run";
        let host_root = tempfile::tempdir().expect("a host root");
        std::fs::create_dir_all(host_root.path().join("opt/app")).unwrap();
        std::fs::write(host_root.path().join("opt/app/run"), CONTROL_BYTES).unwrap();
        let host = MockExecutor::new().with_file_bytes(CONTROL, CONTROL_BYTES);
        let gate = ScreenedExecutor::new(&host);
        let render = tempfile::tempdir().expect("a render directory");

        let mut snap = test_snapshot_with_unmanaged(vec![
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
        ]);
        bundle_unmanaged_files(&mut snap, render.path(), host_root.path(), &gate)
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

    /// A host root on disk and a mock host holding the same files, for the
    /// bundler: the gate screens the mock, the copy reads the disk.
    fn bundle_host(
        files: &[(&str, &[u8])],
    ) -> (
        tempfile::TempDir,
        inspectah_collect::executor::mock::MockExecutor,
    ) {
        let root = tempfile::tempdir().expect("a host root");
        let mut host = inspectah_collect::executor::mock::MockExecutor::new();
        for (path, bytes) in files {
            let on_disk = root.path().join(path.trim_start_matches('/'));
            std::fs::create_dir_all(on_disk.parent().unwrap()).unwrap();
            std::fs::write(&on_disk, bytes).unwrap();
            host = host.with_file_bytes(path, bytes);
        }
        (root, host)
    }

    fn shipped(render: &std::path::Path, path: &str) -> Vec<u8> {
        std::fs::read(render.join("unmanaged").join(path.trim_start_matches('/')))
            .unwrap_or_else(|e| panic!("{path} ships: {e}"))
    }

    fn redaction_of(snap: &InspectionSnapshot, path: &str) -> PayloadRedaction {
        snap.unmanaged_files
            .as_ref()
            .unwrap()
            .items
            .iter()
            .find(|f| f.path == path)
            .unwrap()
            .payload_redaction
    }

    /// Text payloads ship redacted and are recorded so; text redaction
    /// leaves nothing to do, binary, private-key delimiters in code and a
    /// certificate ship byte for byte; the file mode is kept.
    #[test]
    fn bundling_redacts_text_payloads_and_ships_the_rest_as_read() {
        use inspectah_core::machine_state::ScreenedExecutor;
        use std::os::unix::fs::PermissionsExt;
        const ENV: &[u8] = b"API_TOKEN=supersecretvalue123\nMODE=prod\n";
        const SCRIPT: &[u8] =
            b"#!/bin/sh\nexport DB_URL=postgres://app:pgsecret99@db/app\nexec /opt/app/run\n";
        const PLAIN: &[u8] = b"[server]\nport = 8080\n";
        const BINARY: &[u8] = b"\x7fELF\x00password=binarysecret\x00";
        const DELIMITERS: &[u8] =
            br#"{"begin":"-----BEGIN PRIVATE KEY-----","end":"-----END PRIVATE KEY-----"}"#;
        const CERT: &[u8] =
            b"-----BEGIN CERTIFICATE-----\nMIIBszCCAVmgAwIBAgIUQ\n-----END CERTIFICATE-----\n";
        let files: &[(&str, &[u8])] = &[
            ("/opt/app/.env", ENV),
            ("/opt/app/start.sh", SCRIPT),
            ("/opt/app/app.ini", PLAIN),
            ("/opt/app/run", BINARY),
            ("/opt/app/tmpl.json", DELIMITERS),
            ("/opt/app/ca.pem", CERT),
        ];
        let (root, host) = bundle_host(files);
        std::fs::set_permissions(
            root.path().join("opt/app/start.sh"),
            std::fs::Permissions::from_mode(0o750),
        )
        .unwrap();
        let gate = ScreenedExecutor::new(&host);
        let mut snap = test_snapshot_with_unmanaged(
            files
                .iter()
                .map(|(p, _)| payload(p, FileType::DataFile))
                .collect(),
        );
        let render = tempfile::tempdir().unwrap();
        bundle_unmanaged_files(&mut snap, render.path(), root.path(), &gate).expect("bundles");

        let env = String::from_utf8(shipped(render.path(), "/opt/app/.env")).unwrap();
        assert!(!env.contains("supersecretvalue123"), "{env}");
        assert!(
            env.contains("REDACTED_") && env.contains("MODE=prod"),
            "{env}"
        );
        let script = String::from_utf8(shipped(render.path(), "/opt/app/start.sh")).unwrap();
        assert!(!script.contains("pgsecret99"), "{script}");
        assert_eq!(
            std::fs::metadata(render.path().join("unmanaged/opt/app/start.sh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o750
        );
        for (path, bytes) in [
            ("/opt/app/app.ini", PLAIN),
            ("/opt/app/run", BINARY),
            ("/opt/app/tmpl.json", DELIMITERS),
            ("/opt/app/ca.pem", CERT),
        ] {
            assert_eq!(shipped(render.path(), path), bytes, "{path}");
            assert_eq!(
                redaction_of(&snap, path),
                PayloadRedaction::Unchanged,
                "{path}"
            );
        }
        for path in ["/opt/app/.env", "/opt/app/start.sh"] {
            assert_eq!(
                redaction_of(&snap, path),
                PayloadRedaction::Redacted,
                "{path}"
            );
            assert!(
                snap.redactions
                    .iter()
                    .any(|f| f.path == path && f.kind == RedactionKind::Inline),
                "a finding for {path}: {:?}",
                snap.redactions
            );
        }
        assert!(matches!(
            snap.redaction_state,
            Some(RedactionState::FullyRedacted { .. })
        ));
        let lines = unmanaged_file_lines(&snap);
        assert!(
            lines.iter().any(|l| l.contains("REDACTED_* placeholders")),
            "{lines:?}"
        );
    }

    /// `--no-redaction`: every payload ships as the host had it.
    #[test]
    fn bundling_a_raw_snapshot_ships_payloads_as_read() {
        use inspectah_core::machine_state::ScreenedExecutor;
        const ENV: &[u8] = b"API_TOKEN=supersecretvalue123\n";
        let (root, host) = bundle_host(&[("/opt/app/.env", ENV)]);
        let gate = ScreenedExecutor::new(&host);
        let mut snap =
            test_snapshot_with_unmanaged(vec![payload("/opt/app/.env", FileType::DataFile)]);
        snap.redaction_state = Some(RedactionState::Raw);
        let render = tempfile::tempdir().unwrap();
        bundle_unmanaged_files(&mut snap, render.path(), root.path(), &gate).expect("bundles");
        assert_eq!(shipped(render.path(), "/opt/app/.env"), ENV);
        assert_eq!(
            redaction_of(&snap, "/opt/app/.env"),
            PayloadRedaction::Unchanged
        );
        assert!(snap.redactions.is_empty());
    }

    /// The `.env` file's `env-files/` copy takes the bundle's text, and
    /// the bundle's findings replace that copy's inline ones.
    #[test]
    fn bundling_makes_the_env_file_twin_the_shipped_text() {
        use inspectah_core::machine_state::ScreenedExecutor;
        use inspectah_core::types::config::ConfigFileEntry;
        use inspectah_core::types::nonrpm::NonRpmSoftwareSection;
        use inspectah_core::types::redaction::FindingKind as RedactionFindingKind;
        const ENV: &[u8] = b"API_TOKEN=supersecretvalue123\n";
        let (root, host) = bundle_host(&[("/opt/app/.env", ENV)]);
        let gate = ScreenedExecutor::new(&host);
        let mut snap =
            test_snapshot_with_unmanaged(vec![payload("/opt/app/.env", FileType::DataFile)]);
        snap.non_rpm_software = Some(NonRpmSoftwareSection {
            items: Vec::new(),
            env_files: vec![ConfigFileEntry {
                path: "opt/app/.env".into(),
                content: "API_TOKEN=REDACTED_PASSWORD_7\n".into(),
                ..Default::default()
            }],
        });
        snap.redactions = vec![RedactionFinding {
            path: "opt/app/.env".into(),
            source: "pattern".into(),
            kind: RedactionKind::Inline,
            pattern: "password".into(),
            remediation: String::new(),
            line: Some(1),
            replacement: None,
            detection_method: DetectionMethod::Pattern,
            confidence: Some(Confidence::High),
            finding_kind: Some(RedactionFindingKind::Password),
        }];
        let render = tempfile::tempdir().unwrap();
        bundle_unmanaged_files(&mut snap, render.path(), root.path(), &gate).expect("bundles");
        let shipped = String::from_utf8(shipped(render.path(), "/opt/app/.env")).unwrap();
        assert_eq!(
            snap.non_rpm_software.as_ref().unwrap().env_files[0].content,
            shipped
        );
        assert!(!snap.redactions.iter().any(|f| f.path == "opt/app/.env"));
        assert!(snap.redactions.iter().any(|f| f.path == "/opt/app/.env"));
    }

    /// Text redaction cannot check is withheld: not UTF-8 (Latin-1, or
    /// UTF-16 with its byte-order mark), or over the read limit. Nothing
    /// is written for it, the item records why, an excluded finding names
    /// it, the snapshot stays fully redacted, and the Containerfile copies
    /// nothing for it. Binary that is not UTF-8, small or over the limit,
    /// ships byte for byte.
    #[test]
    fn bundling_withholds_text_redaction_cannot_check() {
        use inspectah_core::machine_state::ScreenedExecutor;
        const LATIN1: &str = "/opt/app/.env";
        const UTF16: &str = "/opt/app/settings.ini";
        const HUGE: &str = "/opt/app/huge.log";
        const BINARY: &str = "/opt/app/run";
        const HUGE_BINARY: &str = "/opt/app/data.bin";
        let latin1: &[u8] = b"# caf\xe9\nAPI_TOKEN=supersecretvalue123\n";
        let utf16: &[u8] = b"\xff\xfep\x00a\x00s\x00s\x00w\x00o\x00r\x00d\x00=\x00x\x00";
        let mut huge = b"password=hunter2hunter2\n".to_vec();
        huge.resize(PAYLOAD_REDACTION_LIMIT as usize + 1, b'x');
        let binary: &[u8] = b"\x7fELF\x00\xff\xfepassword=binarysecret\x00";
        let mut huge_binary = b"\x7fELF\x00".to_vec();
        huge_binary.resize(PAYLOAD_REDACTION_LIMIT as usize + 1, b'\xfe');
        let files: &[(&str, &[u8])] = &[
            (LATIN1, latin1),
            (UTF16, utf16),
            (HUGE, &huge),
            (BINARY, binary),
            (HUGE_BINARY, &huge_binary),
        ];
        let (root, host) = bundle_host(files);
        let gate = ScreenedExecutor::new(&host);
        let mut snap = test_snapshot_with_unmanaged(
            files
                .iter()
                .map(|(p, _)| payload(p, FileType::DataFile))
                .collect(),
        );
        let render = tempfile::tempdir().unwrap();
        bundle_unmanaged_files(&mut snap, render.path(), root.path(), &gate).expect("bundles");

        let lines = unmanaged_file_lines(&snap);
        for (path, reason) in [
            (LATIN1, UncheckedPayload::NotUtf8),
            (UTF16, UncheckedPayload::NotUtf8),
            (HUGE, UncheckedPayload::TooLarge),
        ] {
            assert!(
                !render
                    .path()
                    .join("unmanaged")
                    .join(path.trim_start_matches('/'))
                    .exists(),
                "{path} is not written"
            );
            assert_eq!(
                redaction_of(&snap, path),
                PayloadRedaction::Withheld(reason),
                "{path}"
            );
            assert!(
                snap.redactions
                    .iter()
                    .any(|f| f.path == path && f.kind == RedactionKind::Excluded),
                "{path}: {:?}",
                snap.redactions
            );
            assert!(!lines.iter().any(|l| l.contains(path)), "{path}: {lines:?}");
        }
        assert_eq!(shipped(render.path(), BINARY), binary);
        assert_eq!(shipped(render.path(), HUGE_BINARY), huge_binary);
        assert!(
            lines.contains(&"COPY unmanaged/opt/app/ /opt/app/".to_string()),
            "{lines:?}"
        );
        assert!(matches!(
            snap.redaction_state,
            Some(RedactionState::FullyRedacted { .. })
        ));
        let staged = super::super::configtree::staged_writes(&snap);
        assert_eq!(
            staged
                .withheld
                .iter()
                .filter(|(_, _, why)| why.name() == "UncheckedPayload")
                .count(),
            3,
            "{:?}",
            staged.withheld
        );
    }

    /// `--no-redaction` streams every payload through as read, over the
    /// read limit included, and still screens the bytes it writes: a key
    /// in them, after the gate passed the file, fails the bundle.
    #[test]
    fn raw_bundling_screens_the_bytes_it_writes() {
        use inspectah_core::machine_state::ScreenedExecutor;
        const PATH: &str = "/opt/app/huge.log";
        let mut big = b"password=hunter2hunter2\n".to_vec();
        big.resize(PAYLOAD_REDACTION_LIMIT as usize + 1, b'x');
        let (root, host) = bundle_host(&[(PATH, &big)]);
        let gate = ScreenedExecutor::new(&host);
        let mut snap = test_snapshot_with_unmanaged(vec![payload(PATH, FileType::DataFile)]);
        snap.redaction_state = Some(RedactionState::Raw);
        let render = tempfile::tempdir().unwrap();
        bundle_unmanaged_files(&mut snap, render.path(), root.path(), &gate).expect("bundles");
        assert_eq!(shipped(render.path(), PATH), big);
        assert_eq!(redaction_of(&snap, PATH), PayloadRedaction::Unchanged);

        const KEY: &str = "-----BEGIN PRIVATE KEY-----\n\
            MC4CAQAwBQYDK2VwBCIEIBQh0WfR6aXEBHdgfX4ix5qDYmYRBZGZEmNFdzJ1Owlp\n\
            -----END PRIVATE KEY-----\n";
        let mut keyed = big.clone();
        keyed.extend_from_slice(KEY.as_bytes());
        std::fs::write(root.path().join("opt/app/huge.log"), &keyed).unwrap();
        let render = tempfile::tempdir().unwrap();
        let err = bundle_unmanaged_files(&mut snap, render.path(), root.path(), &gate)
            .expect_err("the bundle fails");
        assert!(err.to_string().contains(PATH), "{err}");
        assert!(!render.path().join("unmanaged/opt/app/huge.log").exists());
    }

    /// A file the snapshot also holds ships the bundle's text, redacted by
    /// the rules its snapshot copy was, and that copy takes it. A key
    /// whose body is split by spaces is gone from both, and the key
    /// finding is kept.
    #[test]
    fn bundling_never_makes_a_twin_less_redacted() {
        use inspectah_core::machine_state::ScreenedExecutor;
        use inspectah_core::types::config::ConfigFileEntry;
        use inspectah_core::types::nonrpm::NonRpmSoftwareSection;
        use inspectah_core::types::redaction::FindingKind as RedactionFindingKind;
        const BODY: &str =
            "MC4C AQAw BQYD K2Vw BCIE IBQh 0WfR 6aXE BHdg fX4i x5qD YmYR BZGZ EmNF dzJ1 Owlp";
        let env = format!(
            "TLS_KEY=\"-----BEGIN PRIVATE KEY----- {BODY} -----END PRIVATE KEY-----\"\n\
             API_TOKEN=supersecretvalue123\n"
        );
        let (root, host) = bundle_host(&[("/opt/app/.env", env.as_bytes())]);
        let gate = ScreenedExecutor::new(&host);
        let mut snap =
            test_snapshot_with_unmanaged(vec![payload("/opt/app/.env", FileType::DataFile)]);
        snap.non_rpm_software = Some(NonRpmSoftwareSection {
            items: Vec::new(),
            env_files: vec![ConfigFileEntry {
                path: "opt/app/.env".into(),
                content: "TLS_KEY=\"REDACTED_PRIVATEKEY_1\"\nAPI_TOKEN=REDACTED_PASSWORD_1\n"
                    .into(),
                ..Default::default()
            }],
        });
        let finding = |kind: RedactionFindingKind| RedactionFinding {
            path: "opt/app/.env".into(),
            source: "pattern".into(),
            kind: RedactionKind::Inline,
            pattern: format!("{kind:?}").to_lowercase(),
            remediation: String::new(),
            line: Some(1),
            replacement: None,
            detection_method: DetectionMethod::Pattern,
            confidence: Some(Confidence::High),
            finding_kind: Some(kind),
        };
        snap.redactions = vec![
            finding(RedactionFindingKind::PrivateKey),
            finding(RedactionFindingKind::Password),
        ];
        let render = tempfile::tempdir().unwrap();
        bundle_unmanaged_files(&mut snap, render.path(), root.path(), &gate).expect("bundles");
        let shipped = String::from_utf8(shipped(render.path(), "/opt/app/.env")).unwrap();
        for secret in ["0WfR", "supersecretvalue123", "BEGIN"] {
            assert!(!shipped.contains(secret), "{shipped}");
        }
        assert_eq!(
            snap.non_rpm_software.as_ref().unwrap().env_files[0].content,
            shipped
        );
        assert!(
            snap.redactions.iter().any(|f| Destination::new(&f.path)
                .is_ok_and(|d| d.as_str() == "/opt/app/.env")
                && f.finding_kind == Some(RedactionFindingKind::PrivateKey)),
            "{:?}",
            snap.redactions
        );
    }

    /// A key in the bytes the bundler reads, after the gate passed the
    /// file, fails the bundle as the gate's own refusal does.
    #[test]
    fn bundling_fails_on_a_key_in_the_bytes_it_would_ship() {
        use inspectah_core::machine_state::ScreenedExecutor;
        const PATH: &str = "/opt/app/app.conf";
        const KEY: &str = "-----BEGIN PRIVATE KEY-----\n\
            MC4CAQAwBQYDK2VwBCIEIBQh0WfR6aXEBHdgfX4ix5qDYmYRBZGZEmNFdzJ1Owlp\n\
            -----END PRIVATE KEY-----\n";
        let (root, host) = bundle_host(&[(PATH, b"a = 1\n")]);
        std::fs::write(root.path().join("opt/app/app.conf"), KEY).unwrap();
        let gate = ScreenedExecutor::new(&host);
        let mut snap = test_snapshot_with_unmanaged(vec![payload(PATH, FileType::Config)]);
        let render = tempfile::tempdir().unwrap();
        let err = bundle_unmanaged_files(&mut snap, render.path(), root.path(), &gate)
            .expect_err("the bundle fails");
        assert!(err.to_string().contains(PATH), "{err}");
        assert!(!render.path().join("unmanaged/opt/app/app.conf").exists());
    }
}
