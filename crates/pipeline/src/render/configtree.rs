//! Config tree materialization — writes config files from snapshot to output.
//!
//! Materializes the following config artifacts:
//! - Config files (snap.config.files where include=true)
//! - Repo files (snap.rpm.repo_files)
//! - GPG keys (snap.rpm.gpg_keys)
//! - Firewall zones (snap.network.firewall_zones)
//! - Kernel/boot: the kargs.d file (the kernel config files themselves,
//!   sysctl files included, are `snap.config.files` entries, written by
//!   the config loop above on their own decision)
//! - Systemd drop-ins (both config/ and drop-ins/)
//! - Generated/local timer units
//! - Quadlet units
//! - Flatpak manifests + provisioning service
//! - Non-RPM env files

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::RenderError;
use inspectah_core::types::units::UNIT_TYPE_SUFFIXES;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use super::safety::{BareWord, has_line_break_or_control, operator_kargs};
use crate::build::extract::is_single_normal_component;

/// Path prefix for quadlet unit files — excluded from config tree copy.
const QUADLET_PREFIX: &str = "/etc/containers/systemd/";

/// The directory systemd reads the administrator's unit files from. A
/// zero-length unit file directly in it is a mask.
const ETC_UNIT_DIR: &str = "/etc/systemd/system";

/// The provisioning service inspectah generates for flatpak applications.
const FLATPAK_SERVICE: &str = "flatpak-provision.service";

/// Where a staged file lands in the image: a normalized absolute path.
///
/// The one constructor, [`Destination::new`], drops empty and `.`
/// components, so `/etc/systemd/./system//x.timer`, a trailing separator
/// and a path given without its leading `/` are all one destination. It
/// refuses `..`, a NUL byte and a control character, and a path that names
/// no file. The reading is lexical: the staging directory is created
/// empty by the export and only the stager writes into it, so no symlink
/// sits on a path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Destination(String);

impl Destination {
    /// The normalized form of `path`, or the rule it breaks.
    pub fn new(path: &str) -> Result<Self, PathError> {
        if path.contains('\0') {
            return Err(PathError::NulByte);
        }
        if has_line_break_or_control(path) {
            return Err(PathError::LineBreakOrControl);
        }
        let mut normal = String::with_capacity(path.len() + 1);
        for component in path.split('/') {
            match component {
                "" | "." => {}
                ".." => return Err(PathError::Traversal),
                name => {
                    normal.push('/');
                    normal.push_str(name);
                }
            }
        }
        if normal.is_empty() {
            return Err(PathError::NoFileName);
        }
        Ok(Self(normal))
    }

    /// The absolute path, `/` first.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The path below `/`, for joining under a staging directory.
    fn relative(&self) -> &str {
        &self.0[1..]
    }

    /// Each proper ancestor directory, nearest first, `/` excluded.
    fn ancestors(&self) -> impl Iterator<Item = &str> {
        let path = self.0.as_str();
        path.match_indices('/')
            .rev()
            .map(move |(at, _)| &path[..at])
            .filter(|ancestor| !ancestor.is_empty())
    }

    /// Whether a zero-length file here is a mask: a file directly in
    /// `/etc/systemd/system` named with a unit type suffix.
    fn is_unit_file_path(&self) -> bool {
        self.0.rsplit_once('/').is_some_and(|(dir, name)| {
            dir == ETC_UNIT_DIR
                && name.rsplit_once('.').is_some_and(|(stem, suffix)| {
                    !stem.is_empty() && UNIT_TYPE_SUFFIXES.contains(&suffix)
                })
        })
    }
}

impl std::fmt::Display for Destination {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why the export withholds a file it was offered. The text is what the
/// Containerfile's FIXME says after the file's path.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WithheldWhy {
    /// A zero-length file at a unit path is a mask (§ 10.2 rule 1).
    #[error(
        "a zero-length file at a unit path is a mask; the export carries a mask only as a systemctl mask command"
    )]
    ZeroLengthUnitFile,
    /// Another staged file has this destination, or needs it as a
    /// directory.
    #[error("{other} is also staged and needs this path; exclude one of the two")]
    Conflict { other: Destination },
}

/// One file the export writes: what offered it, and its content, borrowed
/// from the snapshot or owned when inspectah generates it (the flatpak
/// provisioning service).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedWrite<'a> {
    pub source: StagedFileKind,
    pub content: Cow<'a, str>,
}

/// Every file the export writes, and every file it was offered and does
/// not write. The stager writes `writes` and nothing else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StagedWrites<'a> {
    pub writes: BTreeMap<Destination, StagedWrite<'a>>,
    /// Offered files with a valid destination that are not written.
    pub withheld: Vec<(Destination, StagedFileKind, WithheldWhy)>,
    /// Offered files whose path is not a [`Destination`], as offered.
    pub refused: Vec<(StagedFileKind, String, PathError)>,
}

/// Validate that a filesystem path is safe: no traversal, no NUL bytes,
/// no line breaks or other control characters, no absolute paths.
///
/// The Containerfile names what this accepts (the top-level directory of
/// every staged config file reaches a `COPY` line), so a path holding a
/// line terminator is refused here, not only at the line: a file the
/// stager refuses is never referenced.
pub fn validate_path(path: &str) -> Result<(), PathError> {
    if path.contains('\0') {
        return Err(PathError::NulByte);
    }
    if has_line_break_or_control(path) {
        return Err(PathError::LineBreakOrControl);
    }
    if path.starts_with('/') {
        return Err(PathError::AbsolutePath);
    }
    for component in path.split('/') {
        if component == ".." {
            return Err(PathError::Traversal);
        }
    }
    Ok(())
}

/// Validate that a tarball entry path is safe for extraction.
pub fn validate_tarball_entry(path: &str) -> Result<(), PathError> {
    if path.contains('\0') {
        return Err(PathError::NulByte);
    }
    if path.starts_with('/') {
        return Err(PathError::AbsolutePath);
    }
    for component in path.split('/') {
        if component == ".." {
            return Err(PathError::Traversal);
        }
    }
    Ok(())
}

/// Validate that a symlink target does not escape the tarball root.
pub fn validate_symlink_target(target: &str, _root: &str) -> Result<(), PathError> {
    if target.contains('\0') {
        return Err(PathError::NulByte);
    }
    // Count how many levels up the target goes
    let mut depth: i32 = 0;
    for component in target.split('/') {
        if component == ".." {
            depth -= 1;
        } else if !component.is_empty() && component != "." {
            depth += 1;
        }
        // If depth goes negative, we've escaped the root
        if depth < 0 {
            return Err(PathError::SymlinkEscape);
        }
    }
    Ok(())
}

/// Path safety errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    #[error("path contains NUL byte")]
    NulByte,
    #[error("path contains a line break or control character")]
    LineBreakOrControl,
    #[error("absolute path not allowed")]
    AbsolutePath,
    #[error("path traversal detected")]
    Traversal,
    #[error("symlink escapes root")]
    SymlinkEscape,
    #[error("path names no file")]
    NoFileName,
}

/// Destinations a config entry must not be staged at, because another
/// section carries the file or it is not carried at all:
///
/// - every `units.drop_ins` path: the units section stages those under
///   `drop-ins/` on the drop-in's own decision;
/// - every quadlet unit path: the containers section stages those under
///   `quadlet/`;
/// - every NetworkManager connection that uses DHCP (`method=auto`).
///
/// A path that is not a [`Destination`] matches nothing.
fn owned_elsewhere(snap: &InspectionSnapshot) -> HashSet<Destination> {
    let mut paths: Vec<&str> = Vec::new();
    if let Some(units) = &snap.units {
        paths.extend(units.drop_ins.iter().map(|d| d.path.as_str()));
    }
    if let Some(containers) = &snap.containers {
        paths.extend(containers.quadlet_units.iter().map(|q| q.path.as_str()));
    }
    if let Some(network) = &snap.network {
        paths.extend(
            network
                .connections
                .iter()
                .filter(|c| c.method == "auto")
                .map(|c| c.path.as_str()),
        );
    }
    paths
        .into_iter()
        .filter_map(|p| Destination::new(p).ok())
        .collect()
}

/// Write one staged file, creating its parent directories. A write that
/// does not land is an error naming the file, and fails the export.
fn write_staged(path: &Path, content: &str) -> Result<(), RenderError> {
    let named = |e: std::io::Error| {
        RenderError::Io(std::io::Error::new(
            e.kind(),
            format!("cannot stage {}: {e}", path.display()),
        ))
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(named)?;
    }
    std::fs::write(path, content).map_err(named)
}

/// Write `content` to `dir/name`, where `name` comes from the snapshot.
/// `name` must be one ordinary component: an absolute name would replace
/// `dir` in the join and `..` would leave it. A refused name is skipped
/// with a warning; a write that fails is an error.
fn write_named_file(dir: &Path, name: &str, content: &str) -> Result<(), RenderError> {
    if !is_single_normal_component(name) {
        eprintln!(
            "inspectah: warning: skipping {} -- not a plain file name",
            name.escape_debug()
        );
        return Ok(());
    }
    write_staged(&dir.join(name), content)
}

/// The file name a scheduled-task unit is staged under in
/// `config/etc/systemd/system/`, or `None` when the name is refused: an
/// absolute name or one with `..` would leave that directory.
///
/// [`staged_writes`] and the Containerfile renderer both ask this, so the
/// renderer never enables a unit under a name the stager refused.
pub(crate) fn staged_unit_file(name: &str, suffix: &str) -> Option<String> {
    let file = format!("{name}.{suffix}");
    is_single_normal_component(&file).then_some(file)
}

/// What offered a staged file. Each kind is staged under its own
/// directory of the export: a drop-in under `drop-ins/`, the flatpak
/// provisioning service under `flatpak/`, everything else under
/// `config/`, at the file's image path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StagedFileKind {
    ConfigFile,
    RepoFile,
    GpgKey,
    FirewallZone,
    DropIn,
    /// A local timer's `.timer` or `.service` file, written from the
    /// scheduled section where no other offer has its destination.
    LocalTimerUnit,
    /// A `.timer` or `.service` the scheduled section generates from a
    /// cron entry.
    GeneratedUnit,
    /// The flatpak provisioning service inspectah generates.
    FlatpakService,
}

impl StagedFileKind {
    /// What a FIXME calls a file of this kind.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::ConfigFile => "config file",
            Self::RepoFile => "repo file",
            Self::GpgKey => "GPG key",
            Self::FirewallZone => "firewall zone",
            Self::DropIn => "service drop-in",
            Self::LocalTimerUnit => "local timer unit",
            Self::GeneratedUnit => "generated unit",
            Self::FlatpakService => "flatpak provisioning service",
        }
    }

    /// Where a file of this kind at `dest` is staged under `output_dir`.
    fn staged_path(self, output_dir: &Path, dest: &Destination) -> PathBuf {
        match self {
            Self::DropIn => output_dir.join("drop-ins").join(dest.relative()),
            Self::FlatpakService => output_dir.join("flatpak").join(FLATPAK_SERVICE),
            _ => output_dir.join("config").join(dest.relative()),
        }
    }
}

/// One file offered to the stager, before [`staged_writes`] decides.
struct Offer<'a> {
    kind: StagedFileKind,
    path: Cow<'a, str>,
    content: Cow<'a, str>,
}

impl<'a> Offer<'a> {
    fn borrowed(kind: StagedFileKind, path: &'a str, content: &'a str) -> Self {
        Self {
            kind,
            path: Cow::Borrowed(path),
            content: Cow::Borrowed(content),
        }
    }

    /// A scheduled-section unit `name.suffix` under `/etc/systemd/system`,
    /// unless its body is empty (an `@reboot` entry has no timer) or
    /// [`staged_unit_file`] refuses its name. The Containerfile names a
    /// refused name in a FIXME where it would have enabled the unit.
    fn scheduled_unit(
        kind: StagedFileKind,
        name: &str,
        suffix: &str,
        content: &'a str,
    ) -> Option<Self> {
        if content.is_empty() {
            return None;
        }
        let file = staged_unit_file(name, suffix)?;
        Some(Self {
            kind,
            path: Cow::Owned(format!("{ETC_UNIT_DIR}/{file}")),
            content: Cow::Borrowed(content),
        })
    }
}

/// Every file a section offers the export, in section order: the
/// included config files, repo files, GPG keys, firewall zones and
/// drop-ins at their own paths; the scheduled section's units under
/// `/etc/systemd/system`; and the flatpak provisioning service.
///
/// Local timers are offered whatever their disposition, as the stager has
/// always written them: the scheduled section does not own their files
/// (§ 8), a config entry or drop-in row records the same file, and the
/// offer yields to theirs in [`staged_writes`].
fn offers(snap: &InspectionSnapshot) -> Vec<Offer<'_>> {
    let mut out = Vec::new();
    if let Some(config) = &snap.config {
        out.extend(
            config
                .files
                .iter()
                .filter(|f| f.disposition.is_included())
                .map(|f| Offer::borrowed(StagedFileKind::ConfigFile, &f.path, &f.content)),
        );
    }
    if let Some(rpm) = &snap.rpm {
        for (kind, files) in [
            (StagedFileKind::RepoFile, &rpm.repo_files),
            (StagedFileKind::GpgKey, &rpm.gpg_keys),
        ] {
            out.extend(
                files
                    .iter()
                    .filter(|f| f.disposition.is_included())
                    .map(|f| Offer::borrowed(kind, &f.path, &f.content)),
            );
        }
    }
    if let Some(network) = &snap.network {
        out.extend(
            network
                .firewall_zones
                .iter()
                .filter(|z| z.disposition.is_included())
                .map(|z| Offer::borrowed(StagedFileKind::FirewallZone, &z.path, &z.content)),
        );
    }
    if let Some(units) = &snap.units {
        out.extend(
            units
                .drop_ins
                .iter()
                .filter(|d| d.disposition.is_included())
                .map(|d| Offer::borrowed(StagedFileKind::DropIn, &d.path, &d.content)),
        );
    }
    if let Some(st) = &snap.scheduled_tasks {
        use StagedFileKind::{GeneratedUnit, LocalTimerUnit};
        for u in st
            .generated_timer_units
            .iter()
            .filter(|u| u.disposition.is_included())
        {
            out.extend(Offer::scheduled_unit(
                GeneratedUnit,
                &u.name,
                "timer",
                &u.timer_content,
            ));
            out.extend(Offer::scheduled_unit(
                GeneratedUnit,
                &u.name,
                "service",
                &u.service_content,
            ));
        }
        for t in st.systemd_timers.iter().filter(|t| t.source == "local") {
            out.extend(Offer::scheduled_unit(
                LocalTimerUnit,
                &t.name,
                "timer",
                &t.timer_content,
            ));
            out.extend(Offer::scheduled_unit(
                LocalTimerUnit,
                &t.name,
                "service",
                &t.service_content,
            ));
        }
    }
    if let Some(containers) = &snap.containers {
        let included: Vec<_> = containers
            .flatpak_apps
            .iter()
            .filter(|app| app.disposition.is_included())
            .collect();
        if !included.is_empty() {
            out.push(Offer {
                kind: StagedFileKind::FlatpakService,
                path: Cow::Owned(format!("{ETC_UNIT_DIR}/{FLATPAK_SERVICE}")),
                content: Cow::Owned(flatpak_provision_service(&included)),
            });
        }
    }
    out
}

/// The stager's decision: every file the export writes, keyed by where it
/// lands in the image, and every offered file it withholds or refuses.
///
/// In order:
///
/// 1. A path that is not a [`Destination`] is refused.
/// 2. A config entry at a destination another section carries
///    ([`owned_elsewhere`]) or under `/etc/containers/systemd/` is left to
///    that section, as before.
/// 3. A local timer's file yields to any other offer at its destination.
/// 4. Offers at one destination with the same content are one write: the
///    config walk and the repo, GPG key or firewall inventory record one
///    host file twice. Different contents at one destination, and a
///    destination another write needs as a directory, are a `Conflict`:
///    every file of the pair is withheld, so neither silently replaces or
///    blocks the other.
/// 5. A zero-length file directly in `/etc/systemd/system` named as a
///    unit is a mask, and is withheld as `ZeroLengthUnitFile` whichever
///    section offered it (§ 10.2 rule 1).
///
/// Everything else is written. [`write_config_tree`] writes `writes` and
/// nothing else, and [`unstaged_files`] names `withheld` and `refused` in
/// the Containerfile, so the export and the Containerfile read one
/// decision.
pub fn staged_writes(snap: &InspectionSnapshot) -> StagedWrites<'_> {
    let owned = owned_elsewhere(snap);
    let mut out = StagedWrites::default();
    let mut by_dest: BTreeMap<Destination, Vec<(StagedFileKind, Cow<'_, str>)>> = BTreeMap::new();
    for offer in offers(snap) {
        let dest = match Destination::new(&offer.path) {
            Ok(dest) => dest,
            Err(e) => {
                out.refused.push((offer.kind, offer.path.into_owned(), e));
                continue;
            }
        };
        if offer.kind == StagedFileKind::ConfigFile
            && (owned.contains(&dest) || dest.as_str().starts_with(QUADLET_PREFIX))
        {
            continue;
        }
        by_dest
            .entry(dest)
            .or_default()
            .push((offer.kind, offer.content));
    }

    for offered in by_dest.values_mut() {
        if offered
            .iter()
            .any(|(kind, _)| *kind != StagedFileKind::LocalTimerUnit)
        {
            offered.retain(|(kind, _)| *kind != StagedFileKind::LocalTimerUnit);
        }
        let first = offered[0].1.clone();
        if offered.iter().all(|(_, content)| *content == first) {
            offered.truncate(1);
        }
    }

    let mut conflicts: BTreeMap<Destination, Destination> = BTreeMap::new();
    for (dest, offered) in &by_dest {
        if offered.len() > 1 {
            conflicts
                .entry(dest.clone())
                .or_insert_with(|| dest.clone());
        }
        for ancestor in dest.ancestors() {
            let ancestor = Destination(ancestor.to_owned());
            if by_dest.contains_key(&ancestor) {
                conflicts
                    .entry(dest.clone())
                    .or_insert_with(|| ancestor.clone());
                conflicts.entry(ancestor).or_insert_with(|| dest.clone());
            }
        }
    }

    for (dest, offered) in by_dest {
        if let Some(other) = conflicts.get(&dest) {
            out.withheld.extend(offered.into_iter().map(|(kind, _)| {
                (
                    dest.clone(),
                    kind,
                    WithheldWhy::Conflict {
                        other: other.clone(),
                    },
                )
            }));
            continue;
        }
        // One offer is left: more than one is a conflict, above.
        let Some((source, content)) = offered.into_iter().next() else {
            continue;
        };
        if content.is_empty() && dest.is_unit_file_path() {
            out.withheld
                .push((dest, source, WithheldWhy::ZeroLengthUnitFile));
            continue;
        }
        out.writes.insert(dest, StagedWrite { source, content });
    }
    out
}

/// Why an offered file is not in the export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnstagedWhy {
    /// The path is not a [`Destination`].
    Refused(PathError),
    /// [`staged_writes`] withholds it.
    Withheld(WithheldWhy),
}

impl std::fmt::Display for UnstagedWhy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(e) => e.fmt(f),
            Self::Withheld(why) => why.fmt(f),
        }
    }
}

/// An offered file the export does not write: its kind, its path (as
/// offered when refused, its destination when withheld) and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Unstaged {
    pub(crate) kind: StagedFileKind,
    pub(crate) path: String,
    pub(crate) why: UnstagedWhy,
}

/// Every offered file [`write_config_tree`] does not write, read from
/// [`staged_writes`], for the Containerfile's FIXME lines.
pub(crate) fn unstaged_files(snap: &InspectionSnapshot) -> Vec<Unstaged> {
    let staged = staged_writes(snap);
    let refused = staged.refused.into_iter().map(|(kind, path, e)| Unstaged {
        kind,
        path,
        why: UnstagedWhy::Refused(e),
    });
    let withheld = staged
        .withheld
        .into_iter()
        .map(|(dest, kind, why)| Unstaged {
            kind,
            path: dest.0,
            why: UnstagedWhy::Withheld(why),
        });
    refused.chain(withheld).collect()
}

/// Whether the export writes a file at `path`, by [`staged_writes`].
pub(crate) fn is_staged(staged: &StagedWrites<'_>, path: &str) -> bool {
    Destination::new(path).is_ok_and(|dest| staged.writes.contains_key(&dest))
}

/// The remote, application id and branch of a flatpak `install` line,
/// each proved a [`BareWord`], or why one is refused. An empty branch is
/// kept empty, as the line has always rendered it.
fn flatpak_install_words(
    app: &inspectah_core::types::containers::FlatpakApp,
) -> Result<(BareWord, BareWord, String), String> {
    let remote =
        BareWord::parse_name(&app.remote).map_err(|e| format!("the remote is refused ({e})"))?;
    let app_id =
        BareWord::parse_name(&app.app_id).map_err(|e| format!("the app id is refused ({e})"))?;
    let branch = if app.branch.is_empty() {
        String::new()
    } else {
        BareWord::parse_name(&app.branch)
            .map_err(|e| format!("the branch {} is refused ({e})", app.branch))?
            .to_string()
    };
    Ok((remote, app_id, branch))
}

/// The flatpak provisioning service for the included flatpak `apps`: a
/// oneshot that adds each remote and installs each app on first boot.
fn flatpak_provision_service(apps: &[&inspectah_core::types::containers::FlatpakApp]) -> String {
    let mut seen = HashSet::new();
    let mut remotes = Vec::new();
    let mut unreconstructable = Vec::new();
    for app in apps {
        if app.remote.is_empty() || seen.contains(&app.remote) {
            continue;
        }
        seen.insert(&app.remote);
        if app.remote_url.is_empty() {
            unreconstructable.push(&app.remote);
        } else {
            remotes.push((&app.remote, &app.remote_url));
        }
    }

    let mut svc = String::new();
    svc.push_str("[Unit]\n");
    svc.push_str("Description=Provision Flatpak applications from inspectah manifest\n");
    svc.push_str("After=network-online.target\n");
    svc.push_str("Wants=network-online.target\n");
    svc.push_str("ConditionPathExists=!/var/lib/inspectah/.flatpak-provisioned\n");
    svc.push_str("\n[Service]\n");
    svc.push_str("Type=oneshot\n");
    svc.push_str("RemainAfterExit=yes\n");
    svc.push_str("Restart=on-failure\n");
    svc.push_str("RestartSec=30s\n\n");

    if !unreconstructable.is_empty() {
        svc.push_str("# WARNING: The following remote(s) could not be fully reconstructed\n");
        svc.push_str("# because no URL was captured. You must configure them manually.\n");
        svc.push_str("# See: flatpak remote-modify --help\n");
        for name in &unreconstructable {
            // A string-built file the join guard does not cover.
            svc.push_str(&super::comment::escape_comment_text(&format!(
                "# Remote '{name}': URL unknown"
            )));
            svc.push('\n');
        }
        svc.push('\n');
    }

    // Every value on an `Exec*=` line is a proved `BareWord`: the
    // unit runs as root at boot, and a line break in a value would
    // add a directive. A refused remote or app gets a FIXME, which
    // this string-built file escapes at the site.
    let mut fixmes = Vec::new();
    let mut pre = Vec::new();
    for (name, url) in &remotes {
        match (BareWord::parse_name(name), BareWord::parse_url(url)) {
            (Ok(name), Ok(url)) => pre.push(format!(
                "ExecStartPre=/usr/bin/flatpak remote-add --if-not-exists {name} {url}\n"
            )),
            (Err(e), _) => fixmes.push(format!(
                "# FIXME: flatpak remote {name} not added: the name is refused ({e})"
            )),
            (_, Err(e)) => fixmes.push(format!(
                "# FIXME: flatpak remote {name} not added: the URL {url} is refused ({e})"
            )),
        }
    }
    let mut installs = Vec::new();
    for app in apps {
        match flatpak_install_words(app) {
            Ok((remote, app_id, branch)) => installs.push(format!(
                "ExecStart=/usr/bin/flatpak install -y --noninteractive {remote} {app_id}//{branch}\n"
            )),
            Err(why) => fixmes.push(format!(
                "# FIXME: flatpak app {} from remote {} not installed: {why}",
                app.app_id, app.remote
            )),
        }
    }
    for fixme in &fixmes {
        svc.push_str(&super::comment::escape_comment_text(fixme));
        svc.push('\n');
    }
    if !fixmes.is_empty() {
        svc.push('\n');
    }
    for line in pre.iter().chain(&installs) {
        svc.push_str(line);
    }
    svc.push_str("ExecStartPost=/usr/bin/mkdir -p /var/lib/inspectah\n");
    svc.push_str("ExecStartPost=/usr/bin/touch /var/lib/inspectah/.flatpak-provisioned\n");
    svc.push_str("\n[Install]\n");
    svc.push_str("WantedBy=multi-user.target\n");
    svc.push_str("\n[Unit]\n");
    svc.push_str("StartLimitBurst=3\n");
    svc.push_str("StartLimitIntervalSec=300s\n");
    svc
}

/// Write all config files from snapshot to output_dir/config/ preserving
/// paths. Writes all artifact categories listed in the module doc.
///
/// Returns the sorted list of top-level directory names materialized under
/// `config/` so the Containerfile renderer can emit matching COPY lines.
/// This is the single source of truth — the Containerfile must not
/// compute its own directory list independently.
pub fn write_config_tree(
    snap: &InspectionSnapshot,
    output_dir: &Path,
) -> Result<Vec<String>, RenderError> {
    let config_dir = output_dir.join("config");
    std::fs::create_dir_all(&config_dir)?;

    // Every file `staged_writes` lists, and nothing else: config files,
    // repo files, GPG keys, firewall zones, the scheduled section's units
    // and the flatpak provisioning service under their kind's directory,
    // drop-ins under `drop-ins/`. A file it withholds or refuses is not
    // written, and the Containerfile names it from `unstaged_files`. A
    // write that fails fails the export.
    for (dest, write) in &staged_writes(snap).writes {
        write_staged(&write.source.staged_path(output_dir, dest), &write.content)?;
    }

    // Kernel boot files
    if let Some(ref kb) = snap.kernel_boot {
        // The modules-load.d, modprobe.d and dracut.conf.d files are
        // `config.files` entries, copied through `config/etc/` by the
        // config loop above on their own decision. Writing them here
        // too would materialize an excluded file on a path rule.

        // Kernel arguments drop-in
        let safe_kargs = operator_kargs(&kb.cmdline);
        if !safe_kargs.is_empty() {
            let kargs_dir = config_dir.join("usr/lib/bootc/kargs.d");
            let mut toml = String::new();
            toml.push_str("[kargs]\n");
            toml.push_str("# Migrated from kernel cmdline by inspectah\n");
            for k in &safe_kargs {
                toml.push_str(&format!("append = [\"{k}\"]\n"));
            }
            write_staged(&kargs_dir.join("inspectah-migrated.toml"), &toml)?;
        }
    }

    if let Some(ref containers) = snap.containers {
        for u in &containers.quadlet_units {
            if !u.disposition.is_included() || u.name.is_empty() || u.content.is_empty() {
                continue;
            }
            write_named_file(&output_dir.join("quadlet"), &u.name, &u.content)?;
        }

        let included_flatpaks: Vec<_> = containers
            .flatpak_apps
            .iter()
            .filter(|app| app.disposition.is_included())
            .collect();
        if !included_flatpaks.is_empty() {
            let flatpak_dir = output_dir.join("flatpak");

            // Write JSON manifest
            #[derive(serde::Serialize)]
            struct FlatpakManifestEntry {
                app_id: String,
                remote: String,
                branch: String,
                #[serde(skip_serializing_if = "String::is_empty")]
                remote_url: String,
            }
            let manifest: Vec<FlatpakManifestEntry> = included_flatpaks
                .iter()
                .map(|app| FlatpakManifestEntry {
                    app_id: app.app_id.clone(),
                    remote: app.remote.clone(),
                    branch: app.branch.clone(),
                    remote_url: app.remote_url.clone(),
                })
                .collect();
            let data = serde_json::to_string_pretty(&manifest)
                .map_err(|e| RenderError::Failed(format!("flatpak manifest: {e}")))?;
            write_staged(&flatpak_dir.join("flatpak-install.json"), &data)?;
        }
    }

    // Non-RPM env files are NOT written under config/ — they go to a separate
    // env-files/ directory via write_env_files(). See design decision: .env files
    // are high-probability secret carriers requiring operator review.

    // Stage subscription material (decoded from base64)
    if snap.preserved_subscription
        && let Some(ref sub) = snap.subscription
    {
        stage_subscription_files(output_dir, sub)?;
    }

    // Return the actual top-level directories materialized under config/
    Ok(config_copy_roots(&config_dir))
}

/// Stage subscription files into subscription/ directory, decoding from base64.
fn stage_subscription_files(
    output_dir: &Path,
    section: &inspectah_core::types::subscription::SubscriptionSection,
) -> Result<(), RenderError> {
    use base64::Engine;

    let sub_dir = output_dir.join("subscription");

    // Entitlement certs -> subscription/entitlement/
    let ent_dir = sub_dir.join("entitlement");
    for f in &section.entitlement_certs {
        let filename = Path::new(&f.path).file_name().unwrap_or_default();
        let dest = ent_dir.join(filename);
        std::fs::create_dir_all(dest.parent().unwrap())?;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&f.content)
            .map_err(|e| {
                RenderError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            })?;
        std::fs::write(&dest, decoded)?;
    }

    // CA certs -> subscription/rhsm/ca/
    let ca_dir = sub_dir.join("rhsm/ca");
    for f in &section.ca_certs {
        let filename = Path::new(&f.path).file_name().unwrap_or_default();
        let dest = ca_dir.join(filename);
        std::fs::create_dir_all(dest.parent().unwrap())?;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&f.content)
            .map_err(|e| {
                RenderError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            })?;
        std::fs::write(&dest, decoded)?;
    }

    // Config files (rhsm.conf -> subscription/rhsm/, redhat.repo -> subscription/)
    for f in &section.config_files {
        let dest = if f.path.contains("rhsm.conf") {
            sub_dir.join("rhsm/rhsm.conf")
        } else if f.path.contains("redhat.repo") {
            sub_dir.join("redhat.repo")
        } else {
            continue;
        };
        std::fs::create_dir_all(dest.parent().unwrap())?;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&f.content)
            .map_err(|e| {
                RenderError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            })?;
        std::fs::write(&dest, decoded)?;
    }

    Ok(())
}

/// Returns the sorted list of top-level directory names under config_dir
/// that contain files (excluding tmp/).
pub fn config_copy_roots(config_dir: &Path) -> Vec<String> {
    let entries = match std::fs::read_dir(config_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut roots = Vec::new();
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "tmp" {
            continue;
        }
        // Check if directory has any files
        let has_files = walkdir::WalkDir::new(entry.path())
            .into_iter()
            .flatten()
            .any(|e| e.file_type().is_file());
        if has_files {
            roots.push(name);
        }
    }
    roots.sort();
    roots
}

/// Write .env files to a separate `env-files/` directory under output_dir.
///
/// These are NOT written under `config/` because .env files are
/// high-probability secret carriers requiring operator review before
/// inclusion in a container image.
pub fn write_env_files(snap: &InspectionSnapshot, output_dir: &Path) -> Result<(), RenderError> {
    let nrs = match &snap.non_rpm_software {
        Some(n) => n,
        None => return Ok(()),
    };

    let included: Vec<_> = nrs
        .env_files
        .iter()
        .filter(|e| {
            e.disposition.is_included() && !e.path.is_empty() && !e.content.trim().is_empty()
        })
        .collect();

    if included.is_empty() {
        return Ok(());
    }

    let env_dir = output_dir.join("env-files");
    std::fs::create_dir_all(&env_dir)?;

    for entry in &included {
        let rel = entry.path.trim_start_matches('/');
        if rel.is_empty() {
            continue;
        }
        if validate_path(rel).is_err() {
            continue;
        }
        write_staged(&env_dir.join(rel), &entry.content)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::FindingKind;
    use inspectah_core::types::config::{ConfigCategory, ConfigFileEntry, ConfigSection};
    use inspectah_core::types::containers::{ContainerSection, FlatpakApp, QuadletUnit};
    use inspectah_core::types::kernelboot::KernelBootSection;
    use inspectah_core::types::network::{FirewallZone, NetworkSection};
    use inspectah_core::types::nonrpm::NonRpmSoftwareSection;
    use inspectah_core::types::rpm::{RepoFile, RpmSection};
    use inspectah_core::types::scheduled::{
        GeneratedTimerUnit, ScheduledTaskSection, SystemdTimer,
    };
    use inspectah_core::types::units::{SystemdDropIn, UnitSection};
    use tempfile::TempDir;

    fn snapshot_with_config(path: &str, content: &str) -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.config = Some(ConfigSection {
            files: vec![ConfigFileEntry {
                path: path.to_string(),
                content: content.to_string(),
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            }],
        });
        snap
    }

    fn snapshot_with_repo(path: &str, content: &str) -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            repo_files: vec![RepoFile {
                path: path.to_string(),
                content: content.to_string(),
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            }],
            ..Default::default()
        });
        snap
    }

    #[test]
    fn test_config_tree_materializes_etc() {
        let snap = snapshot_with_config("/etc/httpd/conf/httpd.conf", "ServerRoot /etc/httpd");
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(dir.path().join("config/etc/httpd/conf/httpd.conf").exists());
        let content =
            std::fs::read_to_string(dir.path().join("config/etc/httpd/conf/httpd.conf")).unwrap();
        assert_eq!(content, "ServerRoot /etc/httpd");
    }

    #[test]
    fn test_config_tree_includes_repo_files() {
        let snap = snapshot_with_repo(
            "etc/yum.repos.d/epel.repo",
            "[epel]\nbaseurl=https://example.com",
        );
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(dir.path().join("config/etc/yum.repos.d/epel.repo").exists());
    }

    #[test]
    fn test_config_tree_includes_gpg_keys() {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            gpg_keys: vec![RepoFile {
                path: "etc/pki/rpm-gpg/RPM-GPG-KEY-test".to_string(),
                content: "-----BEGIN PGP PUBLIC KEY BLOCK-----".to_string(),
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(
            dir.path()
                .join("config/etc/pki/rpm-gpg/RPM-GPG-KEY-test")
                .exists()
        );
    }

    /// The kernel/boot section's own contribution to the config tree is
    /// the kargs drop-in and nothing else.
    ///
    /// The earlier form of this test asserted that the section also wrote
    /// `modules-load.d`, `modprobe.d` and `dracut.conf.d` files straight
    /// into `config/etc/`, on no decision at all. Those are `config.files`
    /// entries now, each copied on its own: a file the export writes on a
    /// path rule cannot be excluded.
    #[test]
    fn test_config_tree_kernel_boot_writes_only_kargs() {
        let mut snap = InspectionSnapshot::new();
        snap.kernel_boot = Some(KernelBootSection {
            cmdline: "quiet crashkernel=auto nosmt=force".to_string(),
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        assert!(
            dir.path()
                .join("config/usr/lib/bootc/kargs.d/inspectah-migrated.toml")
                .exists()
        );

        let kargs = std::fs::read_to_string(
            dir.path()
                .join("config/usr/lib/bootc/kargs.d/inspectah-migrated.toml"),
        )
        .unwrap();
        assert!(kargs.contains("nosmt=force"));
        assert!(!kargs.contains("quiet"));
        assert!(!kargs.contains("crashkernel"));
    }

    /// Where the kernel directories went: the config loop copies them,
    /// and its `is_included()` gate refuses the excluded one.
    ///
    /// This is a guard on that gate, not a demonstration that the second
    /// writer is gone. The fixture leaves `kernel_boot` unset, and the
    /// deleted block read fields on that section, so it never ran here
    /// even before the deletion — this test passes unchanged against the
    /// parent commit. The contradiction case, an excluded config entry
    /// against a populated kernel section, is Task 22's
    /// `a_config_exclusion_is_not_overridden_by_a_kernel_list`.
    #[test]
    fn test_kernel_directories_copied_as_plain_config() {
        let mut snap = InspectionSnapshot::new();
        snap.config = Some(ConfigSection {
            files: vec![
                ConfigFileEntry {
                    path: "/etc/modprobe.d/blacklist.conf".to_string(),
                    category: ConfigCategory::KernelModule,
                    content: "blacklist nouveau".to_string(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                },
                ConfigFileEntry {
                    path: "/etc/dracut.conf.d/custom.conf".to_string(),
                    category: ConfigCategory::Initramfs,
                    content: "add_drivers+=\" vfio \"".to_string(),
                    disposition: FindingKind::excluded(),
                    ..Default::default()
                },
            ],
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        assert_eq!(
            std::fs::read_to_string(dir.path().join("config/etc/modprobe.d/blacklist.conf"))
                .expect("an included modprobe.d file is materialized under config/"),
            "blacklist nouveau"
        );
        assert!(
            !dir.path()
                .join("config/etc/dracut.conf.d/custom.conf")
                .exists(),
            "an excluded dracut.conf.d file stays out of the export"
        );
    }

    #[test]
    fn test_config_tree_dropin_only_in_dropins_dir() {
        // Service drop-ins are written to drop-ins/ only — NOT config/.
        // The services renderer owns these paths.
        let mut snap = InspectionSnapshot::new();
        snap.units = Some(UnitSection {
            drop_ins: vec![SystemdDropIn {
                unit: "httpd.service".to_string(),
                path: "etc/systemd/system/httpd.service.d/override.conf".to_string(),
                content: "[Service]\nLimitNOFILE=65535".to_string(),
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        // Must NOT exist in config/ (service-owned)
        assert!(
            !dir.path()
                .join("config/etc/systemd/system/httpd.service.d/override.conf")
                .exists(),
            "service drop-in must NOT be materialized under config/"
        );

        // Must exist in drop-ins/
        assert!(
            dir.path()
                .join("drop-ins/etc/systemd/system/httpd.service.d/override.conf")
                .exists(),
            "service drop-in must be written to drop-ins/"
        );
        let content = std::fs::read_to_string(
            dir.path()
                .join("drop-ins/etc/systemd/system/httpd.service.d/override.conf"),
        )
        .unwrap();
        assert_eq!(content, "[Service]\nLimitNOFILE=65535");
    }

    /// Timer and quadlet unit names are joined onto a fixed directory: an
    /// absolute name replaces it and `..` leaves it. Each must be one
    /// ordinary component, and a unit beside a refused one still writes.
    #[test]
    fn unit_names_cannot_write_outside_the_export() {
        use crate::render::victim_sandbox::VictimSandbox;

        let s = VictimSandbox::new();
        let victim = s.victim().to_string_lossy().into_owned();
        // `config/etc/systemd/system/` is four levels below the export
        // root, so five `..` reach the sandbox.
        let up_from_systemd = "../../../../../victim/planted";
        let mut snap = InspectionSnapshot::new();
        snap.scheduled_tasks = Some(ScheduledTaskSection {
            generated_timer_units: vec![
                GeneratedTimerUnit {
                    name: up_from_systemd.into(),
                    timer_content: "PWNED".into(),
                    service_content: "PWNED".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                },
                GeneratedTimerUnit {
                    name: "backup".into(),
                    timer_content: "[Timer]".into(),
                    service_content: "[Service]".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                },
            ],
            systemd_timers: vec![SystemdTimer {
                name: "../../../../../victim/local".into(),
                source: "local".into(),
                timer_content: "PWNED".into(),
                service_content: "PWNED".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        snap.containers = Some(ContainerSection {
            quadlet_units: vec![
                QuadletUnit {
                    name: victim.clone(),
                    content: "PWNED".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                },
                QuadletUnit {
                    name: "../../victim/planted.container".into(),
                    content: "PWNED".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                },
                QuadletUnit {
                    name: "app.container".into(),
                    content: "[Container]".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        });

        let before = s.outside();
        let result = write_config_tree(&snap, &s.out());
        s.assert_contained(&before, "unit names");
        result.expect("a refused name is skipped, not an error");
        for kept in [
            "config/etc/systemd/system/backup.timer",
            "config/etc/systemd/system/backup.service",
            "quadlet/app.container",
        ] {
            assert!(s.out().join(kept).exists(), "{kept} was not written");
        }
    }

    #[test]
    fn test_config_tree_generated_timer_units() {
        let mut snap = InspectionSnapshot::new();
        snap.scheduled_tasks = Some(ScheduledTaskSection {
            generated_timer_units: vec![GeneratedTimerUnit {
                name: "backup".to_string(),
                timer_content: "[Timer]\nOnCalendar=*-*-* 02:00:00".to_string(),
                service_content: "[Service]\nExecStart=/usr/bin/backup".to_string(),
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(
            dir.path()
                .join("config/etc/systemd/system/backup.timer")
                .exists()
        );
        assert!(
            dir.path()
                .join("config/etc/systemd/system/backup.service")
                .exists()
        );
    }

    #[test]
    fn test_config_tree_local_systemd_timers() {
        let mut snap = InspectionSnapshot::new();
        snap.scheduled_tasks = Some(ScheduledTaskSection {
            systemd_timers: vec![SystemdTimer {
                name: "myjob".to_string(),
                source: "local".to_string(),
                timer_content: "[Timer]\nOnCalendar=daily".to_string(),
                service_content: "[Service]\nExecStart=/usr/local/bin/myjob".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(
            dir.path()
                .join("config/etc/systemd/system/myjob.timer")
                .exists()
        );
        assert!(
            dir.path()
                .join("config/etc/systemd/system/myjob.service")
                .exists()
        );
    }

    #[test]
    fn test_config_tree_nonrpm_env_files_not_in_config() {
        // .env files are no longer written under config/ — they go to env-files/
        // via write_env_files(). Verify write_config_tree does NOT produce them.
        let mut snap = InspectionSnapshot::new();
        snap.non_rpm_software = Some(NonRpmSoftwareSection {
            env_files: vec![ConfigFileEntry {
                path: "/etc/environment.d/99-custom.conf".to_string(),
                content: "MY_VAR=value".to_string(),
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(
            !dir.path()
                .join("config/etc/environment.d/99-custom.conf")
                .exists(),
            ".env files must NOT be written under config/"
        );
    }

    #[test]
    fn test_write_env_files_to_separate_dir() {
        // .env files now go to env-files/ via write_env_files()
        let mut snap = InspectionSnapshot::new();
        snap.non_rpm_software = Some(NonRpmSoftwareSection {
            env_files: vec![ConfigFileEntry {
                path: "/etc/environment.d/99-custom.conf".to_string(),
                content: "MY_VAR=value".to_string(),
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_env_files(&snap, dir.path()).unwrap();
        assert!(
            dir.path()
                .join("env-files/etc/environment.d/99-custom.conf")
                .exists()
        );
        let content = std::fs::read_to_string(
            dir.path()
                .join("env-files/etc/environment.d/99-custom.conf"),
        )
        .unwrap();
        assert_eq!(content, "MY_VAR=value");
    }

    #[test]
    fn test_config_tree_firewall_zones() {
        let mut snap = InspectionSnapshot::new();
        snap.network = Some(NetworkSection {
            firewall_zones: vec![FirewallZone {
                path: "etc/firewalld/zones/public.xml".to_string(),
                name: "public".to_string(),
                content: "<zone>...</zone>".to_string(),
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(
            dir.path()
                .join("config/etc/firewalld/zones/public.xml")
                .exists()
        );
    }

    #[test]
    fn test_excluded_files_not_materialized() {
        let mut snap = InspectionSnapshot::new();
        snap.config = Some(ConfigSection {
            files: vec![ConfigFileEntry {
                path: "/etc/excluded.conf".to_string(),
                content: "secret".to_string(),
                disposition: FindingKind::excluded(), // not included
                ..Default::default()
            }],
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(!dir.path().join("config/etc/excluded.conf").exists());
    }

    #[test]
    fn test_dhcp_connections_excluded() {
        let mut snap = InspectionSnapshot::new();
        snap.network = Some(NetworkSection {
            connections: vec![inspectah_core::types::network::NMConnection {
                path: "/etc/NetworkManager/system-connections/eth0.nmconnection".to_string(),
                method: "auto".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        });
        snap.config = Some(ConfigSection {
            files: vec![ConfigFileEntry {
                path: "/etc/NetworkManager/system-connections/eth0.nmconnection".to_string(),
                content: "[connection]".to_string(),
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            }],
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(
            !dir.path()
                .join("config/etc/NetworkManager/system-connections/eth0.nmconnection")
                .exists()
        );
    }

    #[test]
    fn test_quadlet_prefix_excluded_from_config() {
        let mut snap = InspectionSnapshot::new();
        snap.config = Some(ConfigSection {
            files: vec![ConfigFileEntry {
                path: "/etc/containers/systemd/myapp.container".to_string(),
                content: "[Container]".to_string(),
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            }],
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(
            !dir.path()
                .join("config/etc/containers/systemd/myapp.container")
                .exists()
        );
    }

    // Path safety tests

    #[test]
    fn test_reject_path_traversal() {
        assert!(validate_path("../../etc/passwd").is_err());
    }

    #[test]
    fn test_reject_nul_bytes() {
        assert!(validate_path("etc/config\0.txt").is_err());
    }

    #[test]
    fn test_reject_absolute_paths_in_tarball() {
        assert!(validate_tarball_entry("/etc/passwd").is_err());
    }

    #[test]
    fn test_reject_symlink_escape() {
        assert!(validate_symlink_target("../../../etc/shadow", "config/").is_err());
    }

    #[test]
    fn test_valid_paths_accepted() {
        assert!(validate_path("etc/httpd/conf/httpd.conf").is_ok());
        assert!(validate_tarball_entry("prefix/etc/httpd.conf").is_ok());
        // A symlink that goes up then back down within root is valid
        assert!(validate_symlink_target("sibling/file", "config/sub/").is_ok());
        // Relative within same level
        assert!(validate_symlink_target("./other", "config/").is_ok());
    }

    #[test]
    fn test_config_copy_roots() {
        let dir = TempDir::new().unwrap();
        let config = dir.path().join("config");
        std::fs::create_dir_all(config.join("etc/httpd")).unwrap();
        std::fs::write(config.join("etc/httpd/conf"), "test").unwrap();
        std::fs::create_dir_all(config.join("usr/lib")).unwrap();
        std::fs::write(config.join("usr/lib/test"), "test").unwrap();
        // tmp/ should be excluded
        std::fs::create_dir_all(config.join("tmp")).unwrap();
        std::fs::write(config.join("tmp/junk"), "test").unwrap();

        let roots = config_copy_roots(&config);
        assert!(roots.contains(&"etc".to_string()));
        assert!(roots.contains(&"usr".to_string()));
        assert!(!roots.contains(&"tmp".to_string()));
        // Verify sorted
        assert_eq!(roots, vec!["etc", "usr"]);
    }

    #[test]
    fn test_service_dropin_not_in_config_tree() {
        // Service-owned drop-in paths must NOT be materialized under config/
        // — the services renderer owns these paths.
        let mut snap = InspectionSnapshot::new();
        snap.units = Some(UnitSection {
            drop_ins: vec![SystemdDropIn {
                unit: "sshd.service".to_string(),
                path: "etc/systemd/system/sshd.service.d/override.conf".to_string(),
                content: "[Service]\nPermitRootLogin=no".to_string(),
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            }],
            ..Default::default()
        });
        // Also add the same path as a config file entry to verify the
        // svc_paths exclusion catches it in the config files loop too.
        snap.config = Some(ConfigSection {
            files: vec![ConfigFileEntry {
                path: "/etc/systemd/system/sshd.service.d/override.conf".to_string(),
                content: "[Service]\nPermitRootLogin=no".to_string(),
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            }],
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        // Must NOT exist in config/
        assert!(
            !dir.path()
                .join("config/etc/systemd/system/sshd.service.d/override.conf")
                .exists(),
            "service-owned drop-in must NOT be materialized under config/"
        );
        // Must still exist in drop-ins/
        assert!(
            dir.path()
                .join("drop-ins/etc/systemd/system/sshd.service.d/override.conf")
                .exists(),
            "service-owned drop-in must still be written to drop-ins/"
        );
    }

    #[test]
    fn test_container_owned_quadlet_excluded_from_config_tree() {
        // Quadlet paths listed in snap.containers.quadlet_units must NOT be
        // materialized under config/ — the containers renderer owns them.
        let mut snap = InspectionSnapshot::new();
        snap.containers = Some(ContainerSection {
            quadlet_units: vec![QuadletUnit {
                path: "/etc/containers/systemd/myapp.container".to_string(),
                name: "myapp.container".to_string(),
                content: "[Container]\nImage=quay.io/test:latest".to_string(),
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            }],
            ..Default::default()
        });
        // Also add the same path as a config file to verify exclusion
        snap.config = Some(ConfigSection {
            files: vec![
                ConfigFileEntry {
                    path: "/etc/containers/systemd/myapp.container".to_string(),
                    content: "[Container]\nImage=quay.io/test:latest".to_string(),
                    disposition: FindingKind::included(),
                    locked: false,
                    ..Default::default()
                },
                // A non-quadlet config file should still be materialized
                ConfigFileEntry {
                    path: "/etc/httpd/conf/httpd.conf".to_string(),
                    content: "ServerRoot /etc/httpd".to_string(),
                    disposition: FindingKind::included(),
                    locked: false,
                    ..Default::default()
                },
            ],
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        // Quadlet path must NOT exist in config/
        assert!(
            !dir.path()
                .join("config/etc/containers/systemd/myapp.container")
                .exists(),
            "container-owned quadlet path must NOT be materialized under config/"
        );
        // Non-quadlet config file must still exist
        assert!(
            dir.path().join("config/etc/httpd/conf/httpd.conf").exists(),
            "non-quadlet config file must still be materialized"
        );
        // Quadlet must be written to quadlet/ directory instead
        assert!(
            dir.path().join("quadlet/myapp.container").exists(),
            "quadlet must be written to quadlet/ directory"
        );
    }

    #[test]
    fn test_excluded_quadlet_not_in_quadlet_dir() {
        // Quadlet with include=false must not be materialized in aggregate mode.
        // Single-host snapshots override include=false for quadlets.
        let mut snap = InspectionSnapshot::new();
        snap.aggregate_meta = Some(inspectah_core::types::aggregate::AggregateSnapshotMeta {
            label: "test".into(),
            host_count: 3,
            hostnames: vec![],
            merged_at: String::new(),
            baseline_provisional: false,
            section_host_counts: Default::default(),
        });
        snap.containers = Some(ContainerSection {
            quadlet_units: vec![QuadletUnit {
                path: "/etc/containers/systemd/excluded.container".to_string(),
                name: "excluded.container".to_string(),
                content: "[Container]\nImage=quay.io/test:latest".to_string(),
                disposition: FindingKind::excluded(),
                locked: false,
                ..Default::default()
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        assert!(
            !dir.path().join("quadlet/excluded.container").exists(),
            "excluded quadlet must NOT be written to quadlet/"
        );
    }

    #[test]
    fn test_included_quadlet_materialized() {
        // Quadlets with include=true (set by collectors) are materialized.
        let mut snap = InspectionSnapshot::new();
        snap.containers = Some(ContainerSection {
            quadlet_units: vec![QuadletUnit {
                path: "/etc/containers/systemd/app.container".to_string(),
                name: "app.container".to_string(),
                content: "[Container]\nImage=quay.io/test:latest".to_string(),
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        assert!(
            dir.path().join("quadlet/app.container").exists(),
            "included quadlet must be written to quadlet/"
        );
    }

    #[test]
    fn test_excluded_flatpak_not_materialized() {
        // Flatpaks with include=false are not materialized.
        let mut snap = InspectionSnapshot::new();
        snap.aggregate_meta = Some(inspectah_core::types::aggregate::AggregateSnapshotMeta {
            label: "test".into(),
            host_count: 3,
            hostnames: vec![],
            merged_at: String::new(),
            baseline_provisional: false,
            section_host_counts: Default::default(),
        });
        snap.containers = Some(ContainerSection {
            flatpak_apps: vec![FlatpakApp {
                app_id: "org.example.Excluded".to_string(),
                origin: "flathub".to_string(),
                branch: "stable".to_string(),
                disposition: FindingKind::excluded(),
                locked: false,
                aggregate: None,
                remote: "flathub".to_string(),
                remote_url: "https://flathub.org/repo/".to_string(),
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        assert!(
            !dir.path().join("flatpak").exists(),
            "excluded flatpak must NOT produce flatpak/ directory"
        );
    }

    /// A sysctl file is materialized on its own disposition and on nothing
    /// else: it is an ordinary config entry, copied verbatim.
    #[test]
    fn test_sysctl_source_file_follows_its_own_disposition() {
        let snapshot_with = |disposition: FindingKind| {
            let mut snap = InspectionSnapshot::new();
            snap.config = Some(ConfigSection {
                files: vec![ConfigFileEntry {
                    path: "/etc/sysctl.d/99-custom.conf".to_string(),
                    category: ConfigCategory::Sysctl,
                    content: "net.ipv4.ip_forward = 1".to_string(),
                    disposition,
                    ..Default::default()
                }],
            });
            snap
        };

        let dir = TempDir::new().unwrap();
        write_config_tree(&snapshot_with(FindingKind::excluded()), dir.path()).unwrap();
        assert!(
            !dir.path()
                .join("config/etc/sysctl.d/99-custom.conf")
                .exists(),
            "an excluded sysctl file stays out of config/"
        );

        let dir = TempDir::new().unwrap();
        write_config_tree(&snapshot_with(FindingKind::included()), dir.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("config/etc/sysctl.d/99-custom.conf"))
                .expect("an included entry is materialized under config/"),
            "net.ipv4.ip_forward = 1"
        );
    }

    /// § 3.5: a custom tuned profile is a `ConfigCategory::Tuned` entry
    /// copied through `config/etc/` like any other file.
    ///
    /// The four tests this replaces asserted the opposite and are the
    /// defect, not a requirement: `test_tuned_profile_excluded_from_config_tree`
    /// asserted that a decided `config.files` entry under `/etc/tuned/`
    /// must NOT be materialized, and the other three asserted the
    /// `tuned/` promoted root that `TUNED_PREFIX` skip existed to feed.
    /// A file the export drops on a path rule has no decision.
    #[test]
    fn test_tuned_profile_copied_as_plain_config() {
        let mut snap = InspectionSnapshot::new();
        snap.config = Some(ConfigSection {
            files: vec![
                ConfigFileEntry {
                    path: "/etc/tuned/my-profile/tuned.conf".to_string(),
                    category: ConfigCategory::Tuned,
                    content: "[main]\nsummary=test".to_string(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                },
                ConfigFileEntry {
                    path: "/etc/tuned/other-profile/tuned.conf".to_string(),
                    category: ConfigCategory::Tuned,
                    content: "[main]\nsummary=other".to_string(),
                    disposition: FindingKind::excluded(),
                    ..Default::default()
                },
            ],
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        assert_eq!(
            std::fs::read_to_string(dir.path().join("config/etc/tuned/my-profile/tuned.conf"))
                .expect("an included tuned profile is materialized under config/"),
            "[main]\nsummary=test"
        );
        assert!(
            !dir.path()
                .join("config/etc/tuned/other-profile/tuned.conf")
                .exists(),
            "an excluded tuned profile stays out of the export"
        );
        assert!(
            !dir.path().join("tuned").exists(),
            "the tuned/ promoted root is gone"
        );
    }

    #[test]
    fn test_subscription_dir_staged() {
        use base64::Engine;
        use inspectah_core::types::subscription::{SubscriptionFile, SubscriptionSection};

        let mut snap = InspectionSnapshot::new();
        snap.subscription = Some(SubscriptionSection {
            entitlement_certs: vec![
                SubscriptionFile {
                    path: "/etc/pki/entitlement/123.pem".into(),
                    content: base64::engine::general_purpose::STANDARD.encode("cert-data"),
                    size_bytes: 9,
                    cert_expiry: None,
                },
                SubscriptionFile {
                    path: "/etc/pki/entitlement/123-key.pem".into(),
                    content: base64::engine::general_purpose::STANDARD.encode("key-data"),
                    size_bytes: 8,
                    cert_expiry: None,
                },
            ],
            ca_certs: vec![SubscriptionFile {
                path: "/etc/rhsm/ca/redhat-uep.pem".into(),
                content: base64::engine::general_purpose::STANDARD.encode("ca-data"),
                size_bytes: 7,
                cert_expiry: None,
            }],
            config_files: vec![
                SubscriptionFile {
                    path: "/etc/rhsm/rhsm.conf".into(),
                    content: base64::engine::general_purpose::STANDARD.encode("[rhsm]"),
                    size_bytes: 6,
                    cert_expiry: None,
                },
                SubscriptionFile {
                    path: "/etc/yum.repos.d/redhat.repo".into(),
                    content: base64::engine::general_purpose::STANDARD.encode("[rhel]"),
                    size_bytes: 6,
                    cert_expiry: None,
                },
            ],
            ..Default::default()
        });
        snap.preserved_subscription = true;

        let dir = tempfile::TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        assert!(dir.path().join("subscription/entitlement/123.pem").exists());
        assert!(
            dir.path()
                .join("subscription/entitlement/123-key.pem")
                .exists()
        );
        assert!(
            dir.path()
                .join("subscription/rhsm/ca/redhat-uep.pem")
                .exists()
        );
        assert!(dir.path().join("subscription/rhsm/rhsm.conf").exists());
        assert!(dir.path().join("subscription/redhat.repo").exists());
    }

    #[test]
    fn test_no_subscription_no_dir() {
        let snap = InspectionSnapshot::new();
        let dir = tempfile::TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(!dir.path().join("subscription").exists());
    }
}
