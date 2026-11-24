use inspectah_core::machine_state::{Screened, ScreenedExecutor, payload_credential};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::destination::Destination;
use inspectah_core::types::machine_state::MachineStateMember;
use inspectah_core::types::nonrpm::{
    FileType, PayloadRedaction, UncheckedPayload, UnmanagedFile, UnmanagedFileSection,
};
use inspectah_core::types::redaction::{
    Confidence, DetectionMethod, RedactionFinding, RedactionKind, RedactionState,
};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path};

use crate::export_warning::ExportWarning;
use crate::redaction::engine::{
    SnapshotText, is_redacted_copy_of, record_late_findings, redact_payload, screen_link_target,
};

use super::configtree::{unmanaged_file_withheld, validate_path};
use super::language_packages::sanitize_for_comment;
use super::safety::{CopyToken, has_line_break_or_control};

/// The export directory unmanaged payloads ship under, which every
/// unmanaged `COPY` reads from.
pub const UNMANAGED_DIR: &str = "unmanaged";

/// The mode bits an unmanaged payload keeps from the host: its permission
/// bits. `COPY` gives the image file the mode of its build-context copy,
/// so the bundle, both export tarballs and `inspectah build`'s extraction
/// all carry them. Setuid, setgid and sticky are dropped: a file no
/// package installs does not gain privilege in the image unreviewed.
pub const PAYLOAD_MODE_BITS: u32 = 0o777;

/// Whether `rel`, a path relative to an export root, is an unmanaged
/// payload: [`UNMANAGED_DIR`] followed by at least one more component,
/// every one an ordinary name. A path with a root, a leading `.` or any
/// `..` is never one, so the answer does not rest on the caller having
/// refused traversal first.
pub fn is_unmanaged_payload(rel: &Path) -> bool {
    let mut components = rel.components();
    if components.next() != Some(Component::Normal(OsStr::new(UNMANAGED_DIR))) {
        return false;
    }
    let mut below = false;
    for component in components {
        if !matches!(component, Component::Normal(_)) {
            return false;
        }
        below = true;
    }
    below
}

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

/// One clause per kind of included unmanaged payload redaction changed
/// or the export leaves out, each with its count: the copies holding
/// placeholders, the files redaction could not check, and the symlinks
/// whose target held a credential. Empty when every included payload
/// ships as read.
///
/// The bundler records redaction only on the files it bundles, which are
/// the included ones, and a refine export carries only included files,
/// so both the scan and the export count those. The refine route's
/// sensitivity summary lists these clauses, and
/// [`unmanaged_redaction_notice`] joins them, so the 428 reason, the
/// scan's line and the export header say the same thing.
pub fn unmanaged_redaction_reasons(snap: &InspectionSnapshot) -> Vec<String> {
    let included: Vec<&UnmanagedFile> = snap
        .unmanaged_files
        .iter()
        .flat_map(|s| &s.items)
        .filter(|f| f.disposition.is_included())
        .collect();
    let count = |pred: fn(PayloadRedaction) -> bool| {
        included
            .iter()
            .filter(|f| pred(f.payload_redaction))
            .count()
    };
    let files = |n: usize| if n == 1 { "file" } else { "files" };
    let redacted = count(|r| r == PayloadRedaction::Redacted);
    let withheld = count(|r| r.withheld().is_some());
    let links = count(|r| r == PayloadRedaction::LinkTargetCredential);
    let mut reasons = Vec::new();
    if redacted > 0 {
        reasons.push(format!(
            "{redacted} unmanaged {} redacted before bundling",
            files(redacted)
        ));
    }
    if withheld > 0 {
        reasons.push(format!(
            "{withheld} unmanaged {} left out, not checkable for credentials",
            files(withheld)
        ));
    }
    if links > 0 {
        let noun = if links == 1 { "symlink" } else { "symlinks" };
        reasons.push(format!(
            "{links} unmanaged {noun} left out, the target holding a credential"
        ));
    }
    reasons
}

/// The notice that the unmanaged payloads ship redacted copies, or leave
/// out files, by count, pointing at `secrets-review.md`; `None` when every
/// included payload ships as read. `inspectah scan` prints it after the
/// tarball is written and a refine export carries it in its warnings.
pub fn unmanaged_redaction_notice(snap: &InspectionSnapshot) -> Option<ExportWarning> {
    let reasons = unmanaged_redaction_reasons(snap);
    (!reasons.is_empty())
        .then(|| ExportWarning::notice(format!("{}; see secrets-review.md.", reasons.join("; "))))
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

/// The included unmanaged files the Containerfile renders: a file under
/// `/etc/systemd/system`, at the machine-id, or one the bundle could not
/// check is withheld, and the configuration section names it in a FIXME
/// (`staged_writes`).
fn rendered_files(section: &UnmanagedFileSection) -> Vec<&UnmanagedFile> {
    section
        .items
        .iter()
        .filter(|f| f.disposition.is_included() && unmanaged_file_withheld(f).is_none())
        .collect()
}

/// How the Containerfile carries one directory's regular files. Every
/// path on a `COPY` line is a proved `CopyToken`: `COPY` has no quoting,
/// and a line terminator in a path would end the instruction and start
/// one the snapshot chose. A refused path gets a FIXME instead.
enum RegularCopy<'a> {
    /// Two or more files: one `COPY` of the directory, which carries them
    /// all, or the reason its path is refused.
    Directory(Result<CopyToken, String>, Vec<&'a UnmanagedFile>),
    /// One file or none: a `COPY` of each, or the reason its path is
    /// refused.
    Files(Vec<(&'a UnmanagedFile, Result<CopyToken, String>)>),
}

impl<'a> RegularCopy<'a> {
    /// The directory's regular files, whether or not a `COPY` carries
    /// them.
    fn files(&self) -> Vec<&'a UnmanagedFile> {
        match self {
            Self::Directory(_, files) => files.clone(),
            Self::Files(files) => files.iter().map(|(file, _)| *file).collect(),
        }
    }

    /// Whether an emitted `COPY` line carries `file`. A directory `COPY`
    /// is recursive, so it carries every file under the directory, by
    /// path component: `/opt/app2/x` is not under `/opt/app`.
    fn carries(&self, file: &UnmanagedFile) -> bool {
        match self {
            Self::Directory(Ok(dir), _) => {
                std::path::Path::new(&file.path).starts_with(dir.as_str())
            }
            Self::Directory(Err(_), _) => false,
            Self::Files(files) => files
                .iter()
                .any(|(f, copy)| copy.is_ok() && f.path == file.path),
        }
    }
}

/// One parent directory's image-bound files, as the Containerfile
/// carries them.
struct DirectoryCopy<'a> {
    dir: String,
    regular: RegularCopy<'a>,
    symlinks: Vec<&'a UnmanagedFile>,
}

/// The `COPY` and `ln -sf` decisions for `included`, one per parent
/// directory in path order. Files under `/srv` are left out: the `/var`
/// strategy carries them, never a `COPY`.
fn directory_copies<'a>(included: &[&'a UnmanagedFile]) -> Vec<DirectoryCopy<'a>> {
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
    groups
        .into_iter()
        .map(|(dir, files)| {
            // Separate symlinks from regular files for distinct rendering.
            let (symlinks, regular): (Vec<_>, Vec<_>) = files
                .into_iter()
                .partition(|f| f.file_type == FileType::Symlink);
            let regular = if regular.len() > 1 {
                RegularCopy::Directory(copy_path(&dir), regular)
            } else {
                RegularCopy::Files(
                    regular
                        .into_iter()
                        .map(|f| (f, copy_path(&f.path)))
                        .collect(),
                )
            };
            DirectoryCopy {
                dir,
                regular,
                symlinks,
            }
        })
        .collect()
}

/// The unmanaged files a `COPY` line of the Containerfile carries, by the
/// decisions [`unmanaged_file_lines`] renders: a file's own `COPY`, or a
/// directory `COPY` of it or of an ancestor. A file under `/srv`, or one
/// no emitted `COPY` covers, reaches only the export archive.
pub(crate) fn copied_files(snap: &InspectionSnapshot) -> Vec<&UnmanagedFile> {
    let Some(section) = &snap.unmanaged_files else {
        return Vec::new();
    };
    let copies = directory_copies(&rendered_files(section));
    copies
        .iter()
        .flat_map(|d| d.regular.files())
        .filter(|file| copies.iter().any(|d| d.regular.carries(file)))
        .collect()
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

    let included = rendered_files(section);

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
    lines.extend(super::unmanaged_owners::containerfile_lines(snap));

    for DirectoryCopy {
        dir,
        regular,
        symlinks,
    } in &directory_copies(&included)
    {
        match regular {
            RegularCopy::Directory(Ok(path), _) => {
                let rel = path.as_str().trim_start_matches('/');
                lines.push(format!("COPY unmanaged/{rel}/ /{rel}/"));
            }
            RegularCopy::Directory(Err(why), _) => lines.push(format!(
                "# FIXME: unmanaged files under {} not copied: the directory path {why}",
                sanitize_for_comment(dir)
            )),
            RegularCopy::Files(files) => {
                for (file, copy) in files {
                    match copy {
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
        }

        // Symlinks are recreated with ln -sf so the image preserves
        // the original link topology. Unknown targets get advisory comments.
        // `mkdir -p` first: no COPY creates a directory that holds only
        // links, and `ln` does not create its link's parent. `--` ends
        // option parsing, so a target such as `--x` stays an operand.
        for file in symlinks {
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
                    "RUN mkdir -p -- {} && ln -sf -- {} {}",
                    shell_escape(dir),
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

/// Whether a payload opening with `head` is binary, which ships byte for
/// byte: it is not UTF-8 (`is_utf8`), holds a NUL byte (`has_nul`), and
/// does not open with a UTF-16 or UTF-32 byte-order mark. UTF-8 is text
/// whatever bytes it holds, NUL included, and goes through redaction; a
/// payload that is neither is withheld, never shipped as read. ELF, gzip
/// and image files are not UTF-8 within their first bytes.
fn is_binary(head: &[u8], is_utf8: bool, has_nul: bool) -> bool {
    !is_utf8 && has_nul && !WIDE_TEXT_BOMS.iter().any(|bom| head.starts_with(bom))
}

/// What the bundler did with one regular payload.
enum PayloadCopy {
    /// Written as read: binary, text redaction left alone, or anything
    /// under `--no-redaction`.
    Written,
    /// Written redacted, with the findings to record for it: none for a
    /// file the snapshot holds, whose copy's findings stand.
    Redacted(Vec<RedactionFinding>),
    /// Not written: text redaction could not check.
    Unchecked(UncheckedPayload),
    /// Not written: the bytes read hold what the key screening withholds,
    /// so the file changed after the screening passed it.
    KeyFound(MachineStateMember),
}

/// Whether a stream of bytes is UTF-8, fed a chunk at a time: a character
/// split between two chunks is carried over to the next.
#[derive(Default)]
struct Utf8Stream {
    carry: Vec<u8>,
    invalid: bool,
}

impl Utf8Stream {
    fn feed(&mut self, bytes: &[u8]) {
        if self.invalid {
            return;
        }
        let joined;
        let bytes = if self.carry.is_empty() {
            bytes
        } else {
            joined = [std::mem::take(&mut self.carry).as_slice(), bytes].concat();
            &joined
        };
        if let Err(e) = std::str::from_utf8(bytes) {
            match e.error_len() {
                Some(_) => self.invalid = true,
                None => self.carry = bytes[e.valid_up_to()..].to_vec(),
            }
        }
    }

    /// Whether every byte fed was UTF-8, the last character whole.
    fn is_utf8(&self) -> bool {
        !self.invalid && self.carry.is_empty()
    }
}

/// Reads `inner` and writes each byte it hands out to `out`, noting any
/// NUL and whether the bytes are UTF-8, so what the key screening reads
/// is exactly what lands in the bundle.
struct TeeReader<R, W> {
    inner: R,
    out: W,
    has_nul: bool,
    utf8: Utf8Stream,
}

impl<R: Read, W: Write> Read for TeeReader<R, W> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.out.write_all(&buf[..n])?;
        self.has_nul |= buf[..n].contains(&0);
        self.utf8.feed(&buf[..n]);
        Ok(n)
    }
}

/// What [`copy_screened`] found in the bytes it wrote.
struct Streamed {
    /// The machine-state member the key screening found, if any.
    member: Option<MachineStateMember>,
    has_nul: bool,
    is_utf8: bool,
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
/// for keys as they pass ([`payload_credential`]), and say what they held.
/// `dest` is removed when the screening finds a key.
fn copy_screened(reader: impl Read, dest: &Path) -> io::Result<Streamed> {
    let mut tee = TeeReader {
        inner: reader,
        out: io::BufWriter::new(create_at(dest)?),
        has_nul: false,
        utf8: Utf8Stream::default(),
    };
    let member = payload_credential(&mut tee)?;
    tee.out.flush()?;
    if member.is_some() {
        std::fs::remove_file(dest)?;
    }
    Ok(Streamed {
        member,
        has_nul: tee.has_nul,
        is_utf8: tee.utf8.is_utf8(),
    })
}

/// A snapshot field holding a redacted copy of a payload's text.
struct Twin {
    kind: SnapshotText,
    /// The path the copy was redacted under.
    path: String,
    /// The copy, as `redact` left it.
    text: String,
}

/// Write the regular payload at `source`, host path `path`, to `dest`,
/// from one open of the file, and say what was written. Every byte
/// written is screened for keys as written, so nothing the screening did
/// not read ships, even if the file changes meanwhile.
///
/// Under `--no-redaction` (`redacting` false) the file streams through as
/// read. Otherwise a payload is classified by encoding: UTF-8 is text,
/// whatever bytes it holds; binary ([`is_binary`]) ships as read; anything
/// else, and text larger than [`PAYLOAD_REDACTION_LIMIT`], is not written.
/// Text the snapshot also holds (`twins`) ships the snapshot's own
/// redacted copy, when that copy was read from this text
/// ([`is_redacted_copy_of`]), and is not written otherwise; such a file
/// is never binary. Other text is redacted ([`redact_payload`]).
fn payload_copy(
    source: &Path,
    dest: &Path,
    path: &str,
    redacting: bool,
    twins: &[Twin],
) -> io::Result<PayloadCopy> {
    let mut file = std::fs::File::open(source)?;
    if !redacting {
        let streamed = copy_screened(file, dest)?;
        return Ok(streamed
            .member
            .map_or(PayloadCopy::Written, PayloadCopy::KeyFound));
    }
    let mut head = Vec::new();
    (&mut file)
        .take(PAYLOAD_REDACTION_LIMIT + 1)
        .read_to_end(&mut head)?;
    if head.len() as u64 > PAYLOAD_REDACTION_LIMIT {
        let streamed = copy_screened(io::Cursor::new(&head).chain(file), dest)?;
        if let Some(member) = streamed.member {
            return Ok(PayloadCopy::KeyFound(member));
        }
        if twins.is_empty() && is_binary(&head, streamed.is_utf8, streamed.has_nul) {
            return Ok(PayloadCopy::Written);
        }
        std::fs::remove_file(dest)?;
        return Ok(PayloadCopy::Unchecked(UncheckedPayload::TooLarge));
    }
    if let Some(member) = payload_credential(&mut head.as_slice())? {
        return Ok(PayloadCopy::KeyFound(member));
    }
    let Ok(text) = std::str::from_utf8(&head) else {
        if twins.is_empty() && is_binary(&head, false, head.contains(&0)) {
            write_at(dest, &head)?;
            return Ok(PayloadCopy::Written);
        }
        return Ok(PayloadCopy::Unchecked(UncheckedPayload::NotUtf8));
    };
    let (shipped, findings) = match twins {
        [] => match redact_payload(text, path) {
            Some((redacted, findings)) => (redacted, findings),
            None => {
                write_at(dest, &head)?;
                return Ok(PayloadCopy::Written);
            }
        },
        [first, ..] => {
            if !twins
                .iter()
                .all(|t| is_redacted_copy_of(t.kind, &t.path, &t.text, text))
            {
                return Ok(PayloadCopy::Unchecked(UncheckedPayload::ChangedSinceScan));
            }
            if first.text == text {
                write_at(dest, &head)?;
                return Ok(PayloadCopy::Written);
            }
            (first.text.clone(), Vec::new())
        }
    };
    // The screening again, over the text that ships.
    if let Some(member) = payload_credential(&mut shipped.as_bytes())? {
        return Ok(PayloadCopy::KeyFound(member));
    }
    write_at(dest, shipped.as_bytes())?;
    Ok(PayloadCopy::Redacted(findings))
}

/// The snapshot's redacted copies of whole files, by destination: an
/// `.env` file's content (written to `env-files/`) and a compose file's
/// `raw_content`. `redact` redacted each from an earlier read.
fn twins_by_key(snap: &InspectionSnapshot) -> BTreeMap<String, Vec<Twin>> {
    let env = snap
        .non_rpm_software
        .iter()
        .flat_map(|nrs| &nrs.env_files)
        .filter(|e| !e.content.trim().is_empty())
        .map(|e| (SnapshotText::EnvFile, &e.path, &e.content));
    let compose = snap
        .containers
        .iter()
        .flat_map(|c| &c.compose_files)
        .filter_map(|c| Some((SnapshotText::ComposeFile, &c.path, c.raw_content.as_ref()?)));
    let mut twins: BTreeMap<String, Vec<Twin>> = BTreeMap::new();
    for (kind, path, text) in env.chain(compose) {
        if let Ok(key) = Destination::new(path) {
            twins
                .entry(key.as_str().to_owned())
                .or_default()
                .push(Twin {
                    kind,
                    path: path.clone(),
                    text: text.clone(),
                });
        }
    }
    twins
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
/// the scan roots is copied through one. Unless the snapshot is raw, one
/// whose target holds a credential is withheld instead
/// ([`screen_link_target`]).
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
/// Unless the snapshot is raw (`--no-redaction`), UTF-8 text is redacted
/// as a config file is ([`redact_payload`]) and the redacted text ships:
/// the image gets the `REDACTED_*` placeholders a carried config file
/// gets. A file the snapshot also holds (an `.env` file, a compose file)
/// ships the snapshot's redacted copy itself, so the export holds one
/// text for it, and that copy's findings stand. Text redaction cannot
/// check (not UTF-8 and not binary, over [`PAYLOAD_REDACTION_LIMIT`], or
/// changed since the snapshot's copy was read) is not written at all: the
/// item records why ([`PayloadRedaction::Withheld`]), every export
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
    let twins = twins_by_key(snap);
    let Some(section) = snap.unmanaged_files.as_mut() else {
        return Ok(());
    };
    let payloads: Vec<usize> = section
        .items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            item.disposition.is_included() && unmanaged_file_withheld(item).is_none()
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
    for i in payloads {
        let item = &mut section.items[i];
        let rel_path = item.path.trim_start_matches('/');
        let dest = render_dir.join(UNMANAGED_DIR).join(rel_path);
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
            // Whichever read it came from, a target holding a credential
            // is withheld, never recreated: the scan's own pass
            // (`redact_unmanaged_link_targets`) never saw a target only
            // the fallback read.
            if redacting && let Some((masked, finding)) = screen_link_target(&item.path, &target) {
                item.link_target = masked;
                item.payload_redaction = PayloadRedaction::LinkTargetCredential;
                findings.push(finding);
                continue;
            }
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create dir for {}", dest.display()))?;
            }
            if !target.is_empty() {
                std::os::unix::fs::symlink(&target, &dest)
                    .with_context(|| format!("failed to recreate symlink {}", item.path))?;
            }
            continue;
        }
        let item_twins = Destination::new(&item.path)
            .ok()
            .and_then(|d| twins.get(d.as_str()))
            .map_or(&[][..], Vec::as_slice);
        let copy = payload_copy(&source, &dest, &item.path, redacting, item_twins)
            .with_context(|| format!("failed to copy {} to tarball", item.path))?;
        match copy {
            PayloadCopy::Written => {}
            PayloadCopy::Redacted(found) => {
                item.payload_redaction = PayloadRedaction::Redacted;
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
            .permissions()
            .mode();
        std::fs::set_permissions(
            &dest,
            std::fs::Permissions::from_mode(mode & PAYLOAD_MODE_BITS),
        )
        .with_context(|| format!("failed to set the mode of {}", item.path))?;
    }
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
    fn the_redaction_notice_counts_included_payloads_only() {
        let file = |path: &str, redaction, included: bool| UnmanagedFile {
            path: path.into(),
            disposition: if included {
                FindingKind::included()
            } else {
                FindingKind::excluded()
            },
            payload_redaction: redaction,
            ..Default::default()
        };
        let cases: Vec<(Vec<UnmanagedFile>, Option<&str>)> = vec![
            (
                vec![file("/opt/a", PayloadRedaction::Unchanged, true)],
                None,
            ),
            (
                vec![file("/opt/a", PayloadRedaction::Redacted, false)],
                None,
            ),
            (
                vec![
                    file("/opt/a", PayloadRedaction::Redacted, true),
                    file("/opt/b", PayloadRedaction::Redacted, true),
                    file(
                        "/opt/c",
                        PayloadRedaction::Withheld(UncheckedPayload::TooLarge),
                        true,
                    ),
                    file("/opt/d", PayloadRedaction::Redacted, false),
                ],
                Some(
                    "2 unmanaged files redacted before bundling; 1 unmanaged file left out, \
                     not checkable for credentials; see secrets-review.md.",
                ),
            ),
            (
                vec![file("/opt/l", PayloadRedaction::LinkTargetCredential, true)],
                Some(
                    "1 unmanaged symlink left out, the target holding a credential; \
                     see secrets-review.md.",
                ),
            ),
        ];
        for (items, want) in cases {
            let snap = test_snapshot_with_unmanaged(items);
            let notice = unmanaged_redaction_notice(&snap);
            assert_eq!(notice.as_ref().map(ExportWarning::text), want);
            if let Some(n) = notice {
                assert_eq!(n.kind(), crate::export_warning::ExportWarningKind::Notice);
            }
        }
    }

    #[test]
    fn is_unmanaged_payload_needs_ordinary_components_below_the_dir() {
        let cases: &[(&str, bool)] = &[
            ("unmanaged/opt/app/run", true),
            ("unmanaged/opt", true),
            ("unmanaged//opt/./run", true),
            ("unmanaged/../config/etc/app.conf", false),
            ("unmanaged/opt/../../config/etc/app.conf", false),
            ("unmanaged/..", false),
            ("unmanaged", false),
            ("unmanaged/", false),
            ("./unmanaged/opt", false),
            ("unmanaged-notes.txt", false),
            ("unmanaged-notes/opt", false),
            ("config/unmanaged/opt", false),
            ("/unmanaged/opt", false),
            ("/", false),
            ("", false),
            // A backslash is an ordinary byte on Linux, never a separator.
            ("unmanaged\\opt\\run", false),
            ("unmanaged\\..\\config", false),
            ("unmanaged/opt\\run", true),
        ];
        for (rel, want) in cases {
            assert_eq!(is_unmanaged_payload(Path::new(rel)), *want, "{rel:?}");
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
            lines.contains(
                &"RUN mkdir -p -- '/opt/app/bin' && ln -sf -- '/opt/app/lib/tool-1.2' '/opt/app/bin/tool'"
                    .to_string()
            ),
            "symlink must produce mkdir -p of its directory, then ln -sf, got: {lines:?}"
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
        let ln_lines: Vec<_> = lines
            .iter()
            .filter(|l| l.contains(" && ln -sf -- "))
            .collect();
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
            lines.contains(
                &"RUN mkdir -p -- '/opt/app' && ln -sf -- '/srv/app/data' '/opt/app/data'"
                    .to_string()
            ),
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
            lines.iter().any(|l| l.contains(" && ln -sf -- ")),
            "symlink must produce ln -sf"
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

    /// A symlink whose target holds a URL credential is withheld, from
    /// the recorded target and from the fallback read of the link on disk
    /// when none was recorded: no link is written, the snapshot keeps the
    /// target masked, an excluded finding names it, and the Containerfile
    /// has a FIXME for it and no `ln`. An ordinary link is still
    /// recreated, and a raw snapshot keeps every target as read.
    #[test]
    fn bundling_withholds_a_link_whose_target_holds_a_credential() {
        use inspectah_collect::executor::mock::MockExecutor;
        use inspectah_core::machine_state::ScreenedExecutor;
        const SECRET: &str = "linkpw8812";
        const RECORDED: &str = "/opt/tool/bin/source";
        const FALLBACK: &str = "/opt/tool/bin/mirror";
        const CONTROL: &str = "/opt/tool/bin/run";
        const TARGET: &str = "https://u:linkpw8812@host.example/repo";
        let host_root = tempfile::tempdir().expect("a host root");
        std::fs::create_dir_all(host_root.path().join("opt/tool/bin")).unwrap();
        std::os::unix::fs::symlink(TARGET, host_root.path().join(&FALLBACK[1..])).unwrap();
        let host = MockExecutor::new();
        let gate = ScreenedExecutor::new(&host);
        let items = || {
            vec![
                link(RECORDED, TARGET),
                // No recorded target: the bundler reads the link itself.
                link(FALLBACK, ""),
                link(CONTROL, "/usr/local/bin/run-tool"),
            ]
        };

        let mut snap = test_snapshot_with_unmanaged(items());
        let render = tempfile::tempdir().expect("a render directory");
        bundle_unmanaged_files(&mut snap, render.path(), host_root.path(), &gate)
            .expect("bundling succeeds");
        let unmanaged = render.path().join("unmanaged");
        for path in [RECORDED, FALLBACK] {
            assert!(
                std::fs::symlink_metadata(unmanaged.join(&path[1..])).is_err(),
                "{path} is not written"
            );
            assert_eq!(
                redaction_of(&snap, path),
                PayloadRedaction::LinkTargetCredential
            );
            assert!(
                snap.redactions
                    .iter()
                    .any(|f| f.path == path && f.kind == RedactionKind::Excluded),
                "{path}: {:?}",
                snap.redactions
            );
        }
        assert_eq!(
            std::fs::read_link(unmanaged.join(&CONTROL[1..])).expect("the control is bundled"),
            std::path::Path::new("/usr/local/bin/run-tool")
        );
        let json = serde_json::to_string(&snap).unwrap();
        assert!(!json.contains(SECRET), "{json}");
        assert!(
            json.contains("host.example/repo"),
            "the masked target stays"
        );
        let containerfile = super::super::containerfile::render_containerfile(&snap, None, None);
        assert!(!containerfile.contains(SECRET), "{containerfile}");
        for path in [RECORDED, FALLBACK] {
            assert!(
                containerfile
                    .lines()
                    .any(|l| l.starts_with("# FIXME:") && l.contains(path)),
                "{path}: no FIXME in:\n{containerfile}"
            );
            assert!(
                !containerfile
                    .lines()
                    .any(|l| l.contains("ln -sf") && l.contains(path)),
                "{path}: recreated in:\n{containerfile}"
            );
        }
        assert!(containerfile.contains("ln -sf -- '/usr/local/bin/run-tool' '/opt/tool/bin/run'"));

        // `--no-redaction` keeps the targets, as it keeps every payload.
        let mut raw = test_snapshot_with_unmanaged(items());
        raw.redaction_state = Some(RedactionState::Raw);
        let render = tempfile::tempdir().expect("a render directory");
        bundle_unmanaged_files(&mut raw, render.path(), host_root.path(), &gate)
            .expect("bundling succeeds");
        for path in [RECORDED, FALLBACK] {
            assert_eq!(
                std::fs::read_link(render.path().join("unmanaged").join(&path[1..]))
                    .expect("a raw scan recreates the link"),
                std::path::Path::new(TARGET)
            );
            assert_eq!(redaction_of(&raw, path), PayloadRedaction::Unchanged);
        }
    }

    /// The start of an x86-64 ELF executable: its 64-byte header, the first
    /// instructions (`endbr64`, `xor ebp,ebp`, `mov r9,rdx`), then text
    /// that reads as a credential, which a binary still ships with.
    const ELF: &[u8] = b"\x7fELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\
        \x02\x00\x3e\x00\x01\x00\x00\x00\x40\x10\x40\x00\x00\x00\x00\x00\
        \x40\x00\x00\x00\x00\x00\x00\x00\x98\x3a\x00\x00\x00\x00\x00\x00\
        \x00\x00\x00\x00\x40\x00\x38\x00\x0d\x00\x40\x00\x1f\x00\x1e\x00\
        \xf3\x0f\x1e\xfa\x31\xed\x49\x89\xd1password=binarysecret\x00";

    /// `hello` compressed by gzip.
    const GZIP: &[u8] = b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x00\x03\xcb\x48\xcd\xc9\xc9\x07\x00\
        \x86\xa6\x10\x36\x05\x00\x00\x00";

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
    /// leaves nothing to do, binary (a real ELF and gzip header),
    /// private-key delimiters in code and a certificate ship byte for
    /// byte; the file mode is kept.
    #[test]
    fn bundling_redacts_text_payloads_and_ships_the_rest_as_read() {
        use inspectah_core::machine_state::ScreenedExecutor;
        use std::os::unix::fs::PermissionsExt;
        const ENV: &[u8] = b"API_TOKEN=supersecretvalue123\nMODE=prod\n";
        const SCRIPT: &[u8] =
            b"#!/bin/sh\nexport DB_URL=postgres://app:pgsecret99@db/app\nexec /opt/app/run\n";
        const PLAIN: &[u8] = b"[server]\nport = 8080\n";
        const DELIMITERS: &[u8] =
            br#"{"begin":"-----BEGIN PRIVATE KEY-----","end":"-----END PRIVATE KEY-----"}"#;
        const CERT: &[u8] =
            b"-----BEGIN CERTIFICATE-----\nMIIBszCCAVmgAwIBAgIUQ\n-----END CERTIFICATE-----\n";
        let files: &[(&str, &[u8])] = &[
            ("/opt/app/.env", ENV),
            ("/opt/app/start.sh", SCRIPT),
            ("/opt/app/app.ini", PLAIN),
            ("/opt/app/run", ELF),
            ("/opt/app/data.gz", GZIP),
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
            ("/opt/app/run", ELF),
            ("/opt/app/data.gz", GZIP),
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

    /// A bundled payload keeps the host's permission bits, redacted or
    /// not, and drops setuid, setgid and sticky.
    #[test]
    fn bundling_keeps_permission_bits_and_drops_special_bits() {
        use inspectah_core::machine_state::ScreenedExecutor;
        use std::os::unix::fs::PermissionsExt;
        let files: &[(&str, &[u8], u32, u32)] = &[
            (
                "/opt/app/.env",
                b"API_TOKEN=supersecretvalue123\n",
                0o600,
                0o600,
            ),
            (
                "/opt/app/run.sh",
                b"#!/bin/sh\nexec /opt/app/run\n",
                0o755,
                0o755,
            ),
            ("/opt/app/tool", ELF, 0o4755, 0o755),
            ("/opt/app/shared.ini", b"[a]\nb = 1\n", 0o3664, 0o664),
        ];
        let contents: Vec<(&str, &[u8])> = files.iter().map(|(p, b, _, _)| (*p, *b)).collect();
        let (root, host) = bundle_host(&contents);
        for (path, _, mode, _) in files {
            std::fs::set_permissions(
                root.path().join(&path[1..]),
                std::fs::Permissions::from_mode(*mode),
            )
            .unwrap();
        }
        let gate = ScreenedExecutor::new(&host);
        let mut snap = test_snapshot_with_unmanaged(
            files
                .iter()
                .map(|(p, _, _, _)| payload(p, FileType::DataFile))
                .collect(),
        );
        let render = tempfile::tempdir().unwrap();
        bundle_unmanaged_files(&mut snap, render.path(), root.path(), &gate).expect("bundles");
        assert_eq!(
            redaction_of(&snap, "/opt/app/.env"),
            PayloadRedaction::Redacted
        );
        for (path, _, _, expected) in files {
            let mode = std::fs::metadata(render.path().join(UNMANAGED_DIR).join(&path[1..]))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777;
            assert_eq!(mode, *expected, "{path}");
        }
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

    /// A snapshot holding the `.env` file at `ENV_TWIN` as `content`,
    /// redacted by `redact` itself, with another `.env` before it so the
    /// snapshot numbers placeholders differently from a file alone.
    fn snapshot_with_env_twin(content: &str) -> InspectionSnapshot {
        use crate::redaction::engine::{RedactOptions, redact};
        use inspectah_core::types::config::ConfigFileEntry;
        use inspectah_core::types::nonrpm::NonRpmSoftwareSection;
        let mut snap = test_snapshot_with_unmanaged(vec![payload(ENV_TWIN, FileType::DataFile)]);
        snap.non_rpm_software = Some(NonRpmSoftwareSection {
            items: Vec::new(),
            env_files: vec![
                ConfigFileEntry {
                    path: "opt/other/.env".into(),
                    content: "API_TOKEN=anothersecret456\n".into(),
                    ..Default::default()
                },
                ConfigFileEntry {
                    path: ENV_TWIN[1..].into(),
                    content: content.into(),
                    ..Default::default()
                },
            ],
        });
        redact(&mut snap, &RedactOptions::default());
        snap
    }

    const ENV_TWIN: &str = "/opt/app/.env";

    fn env_twin(snap: &InspectionSnapshot) -> &str {
        &snap.non_rpm_software.as_ref().unwrap().env_files[1].content
    }

    /// A file the snapshot also holds ships the snapshot's own redacted
    /// copy, byte for byte, and that copy and its findings are left as
    /// `redact` made them. The finding's reproducers: a NUL byte in a
    /// valid UTF-8 `.env` does not make it binary, and a proxy password
    /// opening a private key whose body is split by spaces leaves no key
    /// material.
    #[test]
    fn bundling_ships_the_twin_redact_made() {
        use inspectah_core::machine_state::ScreenedExecutor;
        const BODY: &str =
            "MC4C AQAw BQYD K2Vw BCIE IBQh 0WfR 6aXE BHdg fX4i x5qD YmYR BZGZ EmNF dzJ1 Owlp";
        let cases = [
            "#\x00\nAPI_TOKEN=supersecretvalue123\n".to_string(),
            format!(
                "proxy_password=-----BEGIN PRIVATE KEY-----\n{BODY}\n-----END PRIVATE KEY-----\n\
                 API_TOKEN=supersecretvalue123\n"
            ),
        ];
        for env in cases {
            let (root, host) = bundle_host(&[(ENV_TWIN, env.as_bytes())]);
            let gate = ScreenedExecutor::new(&host);
            let mut snap = snapshot_with_env_twin(&env);
            let twin = env_twin(&snap).to_owned();
            let recorded = snap.redactions.clone();
            assert!(!twin.contains("supersecretvalue123") && !twin.contains("0WfR"));
            let render = tempfile::tempdir().unwrap();
            bundle_unmanaged_files(&mut snap, render.path(), root.path(), &gate).expect("bundles");
            assert_eq!(shipped(render.path(), ENV_TWIN), twin.as_bytes(), "{env:?}");
            assert_eq!(env_twin(&snap), twin);
            assert_eq!(snap.redactions, recorded);
            assert_eq!(
                redaction_of(&snap, ENV_TWIN),
                PayloadRedaction::Redacted,
                "{env:?}"
            );
        }
    }

    /// A file the snapshot also holds is withheld when it changed after
    /// the snapshot read it, or when it is not UTF-8, binary-looking bytes
    /// included: its text never ships unless it is the snapshot's own
    /// redacted copy. The copy itself is left alone.
    #[test]
    fn bundling_withholds_a_twin_it_cannot_match() {
        use inspectah_core::machine_state::ScreenedExecutor;
        const ENV: &str = "API_TOKEN=supersecretvalue123\n";
        for (on_disk, reason) in [
            (
                b"API_TOKEN=supersecretvalue123\nDB_PASSWORD=hunter2hunter2\n".as_slice(),
                UncheckedPayload::ChangedSinceScan,
            ),
            (
                b"API_TOKEN=supersecretvalue123\n\xff\x00".as_slice(),
                UncheckedPayload::NotUtf8,
            ),
        ] {
            let (root, host) = bundle_host(&[(ENV_TWIN, on_disk)]);
            let gate = ScreenedExecutor::new(&host);
            let mut snap = snapshot_with_env_twin(ENV);
            let twin = env_twin(&snap).to_owned();
            let render = tempfile::tempdir().unwrap();
            bundle_unmanaged_files(&mut snap, render.path(), root.path(), &gate).expect("bundles");
            assert!(!render.path().join("unmanaged/opt/app/.env").exists());
            assert_eq!(
                redaction_of(&snap, ENV_TWIN),
                PayloadRedaction::Withheld(reason)
            );
            assert_eq!(env_twin(&snap), twin);
            assert!(
                snap.redactions
                    .iter()
                    .any(|f| f.path == ENV_TWIN && f.kind == RedactionKind::Excluded),
                "{:?}",
                snap.redactions
            );
        }
    }

    /// Text redaction cannot check is withheld: not UTF-8 (Latin-1, or
    /// UTF-16 with its byte-order mark), or over the read limit, a NUL in
    /// it or not. Nothing is written for it, the item records why, an
    /// excluded finding names it, the snapshot stays fully redacted, and
    /// the Containerfile copies nothing for it. A NUL does not make UTF-8
    /// binary: a small file holding one is redacted. Binary that is not
    /// UTF-8, small or over the limit, ships byte for byte.
    #[test]
    fn bundling_withholds_text_redaction_cannot_check() {
        use inspectah_core::machine_state::ScreenedExecutor;
        const LATIN1: &str = "/opt/app/.env";
        const UTF16: &str = "/opt/app/settings.ini";
        const HUGE: &str = "/opt/app/huge.log";
        const HUGE_NUL: &str = "/opt/app/nul.log";
        const SMALL_NUL: &str = "/opt/app/app.conf";
        const BINARY: &str = "/opt/app/run";
        const HUGE_BINARY: &str = "/opt/app/data.bin";
        let latin1: &[u8] = b"# caf\xe9\nAPI_TOKEN=supersecretvalue123\n";
        let utf16: &[u8] = b"\xff\xfep\x00a\x00s\x00s\x00w\x00o\x00r\x00d\x00=\x00x\x00";
        let mut huge = b"password=hunter2hunter2\n".to_vec();
        huge.resize(PAYLOAD_REDACTION_LIMIT as usize + 1, b'x');
        let mut huge_nul = b"password=hunter2hunter2\n\x00".to_vec();
        huge_nul.resize(PAYLOAD_REDACTION_LIMIT as usize + 1, b'x');
        let small_nul: &[u8] = b"#\x00\nAPI_TOKEN=supersecretvalue123\n";
        let binary: &[u8] = b"\x7fELF\x00\xff\xfepassword=binarysecret\x00";
        let mut huge_binary = b"\x7fELF\x00".to_vec();
        huge_binary.resize(PAYLOAD_REDACTION_LIMIT as usize + 1, b'\xfe');
        let files: &[(&str, &[u8])] = &[
            (LATIN1, latin1),
            (UTF16, utf16),
            (HUGE, &huge),
            (HUGE_NUL, &huge_nul),
            (SMALL_NUL, small_nul),
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
            (HUGE_NUL, UncheckedPayload::TooLarge),
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
        let small = shipped(render.path(), SMALL_NUL);
        assert!(!contains(&small, b"supersecretvalue123") && contains(&small, b"REDACTED_"));
        assert!(small.starts_with(b"#\x00\n"));
        assert_eq!(redaction_of(&snap, SMALL_NUL), PayloadRedaction::Redacted);
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
            4,
            "{:?}",
            staged.withheld
        );
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    /// The streamed UTF-8 check carries a character split between chunks,
    /// and fails on an invalid byte or a last character cut off.
    #[test]
    fn utf8_stream_reads_across_chunks() {
        let check = |chunks: &[&[u8]]| {
            let mut stream = Utf8Stream::default();
            for chunk in chunks {
                stream.feed(chunk);
            }
            stream.is_utf8()
        };
        assert!(check(&[b"caf\xc3", b"\xa9 \x00", b"\xe2\x82", b"\xac"]));
        assert!(check(&[b"", b"plain"]));
        assert!(!check(&[b"caf\xe9", b" latin-1"]));
        assert!(!check(&[b"ok", b"\xe2\x82"]));
        assert!(!check(&[b"\x1f\x8b\x08\x00"]));
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
