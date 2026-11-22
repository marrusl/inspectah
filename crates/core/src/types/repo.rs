use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

/// Repo section IDs considered part of the base distribution.
pub const DISTRO_REPOS: &[&str] = &[
    "baseos",
    "appstream",
    "fedora",
    "updates",
    "updates-testing",
    "extras",
    "anaconda",
];

/// Red Hat repos that are official but user-toggleable (not part of the base image).
pub const OFFICIAL_OPTIONAL_REPOS: &[&str] = &["crb", "codeready-builder", "rhel-extensions"];

/// Distro repo keywords that may appear as segments in longer RHEL repo IDs.
///
/// RHEL systems use repo IDs like `rhel-9-for-x86_64-baseos-rpms` rather
/// than the short `baseos` used by CentOS Stream. This list is checked
/// with segment-boundary matching (preceded by `-` or start-of-string)
/// to avoid false positives like a hypothetical `myappstream-extras`.
const DISTRO_REPO_KEYWORDS: &[&str] = &["baseos", "appstream"];

/// Keywords for official-optional repos that may appear as segments
/// in longer RHEL repo IDs (e.g. `codeready-builder-for-rhel-9-x86_64-rpms`).
const OFFICIAL_OPTIONAL_KEYWORDS: &[&str] = &["codeready-builder", "crb"];

/// Classification tier for a repo section ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoTier {
    Distro,
    OfficialOptional,
    ThirdParty,
    None,
}

/// Check whether `id` contains `keyword` as a delimited segment.
///
/// A segment match requires the keyword to be preceded by `-` or
/// start-of-string and followed by `-` or end-of-string.
fn contains_keyword_segment(id: &str, keyword: &str) -> bool {
    if let Some(pos) = id.find(keyword) {
        let before_ok = pos == 0 || id.as_bytes()[pos - 1] == b'-';
        let after = pos + keyword.len();
        let after_ok = after == id.len() || id.as_bytes()[after] == b'-';
        before_ok && after_ok
    } else {
        false
    }
}

/// Classify a repo section ID into its tier.
///
/// Empty, missing, or `@commandline` section IDs return `None` --
/// "no repo identity" is distinct from "known third-party repo."
///
/// Matches both short CentOS-style IDs (`baseos`) and long RHEL-style
/// IDs (`rhel-9-for-x86_64-baseos-rpms`) via segment-boundary matching.
pub fn repo_tier(section_id: &str) -> RepoTier {
    if section_id.is_empty() {
        return RepoTier::None;
    }
    let lower = section_id.to_lowercase();
    let id = lower.as_str();
    if id == "@commandline" {
        return RepoTier::None;
    }
    // Exact match first (CentOS Stream, Fedora, etc.)
    if DISTRO_REPOS.contains(&id) {
        return RepoTier::Distro;
    }
    if OFFICIAL_OPTIONAL_REPOS.contains(&id) {
        return RepoTier::OfficialOptional;
    }
    // Segment match for RHEL-style long IDs (e.g. rhel-9-for-x86_64-baseos-rpms)
    if DISTRO_REPO_KEYWORDS
        .iter()
        .any(|kw| contains_keyword_segment(id, kw))
    {
        return RepoTier::Distro;
    }
    if OFFICIAL_OPTIONAL_KEYWORDS
        .iter()
        .any(|kw| contains_keyword_segment(id, kw))
    {
        return RepoTier::OfficialOptional;
    }
    RepoTier::ThirdParty
}

/// A yum/dnf repo section id (a `.repo` file `[section]` header or an
/// install-time `%{from_repo}` value).
///
/// Equality, hashing, and ordering are ASCII case-insensitive and fold
/// nothing outside the ASCII range, so `RepoId`s that compare equal always
/// hash equal -- see Review Focus 1 in the phase 0 plan. `Display` keeps
/// the id exactly as the host wrote it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct RepoId(String);

impl RepoId {
    /// Build a `RepoId` from the host's spelling of the id.
    pub fn new(id: impl Into<String>) -> RepoId {
        RepoId(id.into())
    }

    /// The id exactly as the host wrote it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq for RepoId {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl Eq for RepoId {}

impl Hash for RepoId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_ascii_lowercase().hash(state);
    }
}

impl PartialOrd for RepoId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RepoId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .to_ascii_lowercase()
            .cmp(&other.0.to_ascii_lowercase())
    }
}

impl fmt::Display for RepoId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A repo identity dnf reports that is not a real `.repo` section.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PseudoRepo {
    /// `@commandline`: a package installed by local RPM path, not a repo.
    Commandline,
    /// `anaconda`: the installer recorded this as the source at install time.
    Anaconda,
    /// `@System`: dnf's marker for a package with no repo provenance.
    System,
}

/// How a package's recorded repo value relates to the host's `.repo`
/// sections.
///
/// `source_repo` on `PackageEntry` keeps its historical meaning (the
/// resolved id for `Exact`/`Resolved`, the recorded value otherwise); this
/// enum is the structured fact `source_repo` summarizes, and every variant
/// carries or implies the originally recorded string (see [`Self::recorded`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RepoAttribution {
    /// The recorded value names a section on the host, whether or not that
    /// section is enabled: the one spelled exactly as recorded, or else the
    /// one case variant of it ([`section_spelling`]).
    Exact { id: RepoId },
    /// The recorded short name matched exactly one enabled section by
    /// substring.
    Resolved { recorded: String, id: RepoId },
    /// The recorded short name matched more than one enabled section by
    /// substring; `source_repo` is left as recorded.
    Ambiguous {
        recorded: String,
        candidates: Vec<RepoId>,
    },
    /// No section on the host matches; the repo was likely removed or
    /// renamed since install.
    Stale { recorded: String },
    /// The recorded value is a pseudo repo (`@commandline`, `anaconda`,
    /// `@System`), not a real repo identity.
    Pseudo { which: PseudoRepo, recorded: String },
    /// No repo value was recorded for this package.
    #[default]
    Unrecorded,
}

impl RepoAttribution {
    /// The repo identity as the host originally recorded it.
    ///
    /// For `Exact`, this returns the matched section's spelling, which
    /// differs from the host's recorded value in ASCII case only when no
    /// section is spelled as recorded and exactly one case variant is; the
    /// originally recorded spelling is not retained. `Unrecorded` has nothing to report, so this returns `""`.
    pub fn recorded(&self) -> &str {
        match self {
            RepoAttribution::Exact { id } => id.as_str(),
            RepoAttribution::Resolved { recorded, .. }
            | RepoAttribution::Ambiguous { recorded, .. }
            | RepoAttribution::Stale { recorded }
            | RepoAttribution::Pseudo { recorded, .. } => recorded,
            RepoAttribution::Unrecorded => "",
        }
    }
}

/// A `[section]` header exactly as the host wrote it.
///
/// Equality, hashing and ordering are the string's own, case included:
/// the identity of a repo decision and of an owner. dnf keys repos
/// case-sensitively, so `[site]` and `[SITE]` in one file are two repos.
/// [`RepoId`] keeps its case-folding semantics for package attribution;
/// code that matches a section against a package's recorded value builds
/// `RepoId::new(id.as_str())` at that comparison. No `From` conversion
/// joins the two types.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct SectionId(String);

impl SectionId {
    /// The section id as the header spells it.
    pub fn new(id: impl Into<String>) -> SectionId {
        SectionId(id.into())
    }

    /// The id exactly as the host wrote it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One reference in a section's final `gpgkey=` value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpgKeyRef {
    /// `file:///path`, or a value with no `://`, which the scan reads as a
    /// path. The path as written, `file://` removed, before dnf variables
    /// expand.
    Local(String),
    /// Any other scheme. dnf fetches it; the scan does not.
    Remote(String),
}

impl GpgKeyRef {
    /// Read one whitespace- or comma-separated reference. `None` for a
    /// `file://` reference with no path.
    fn parse(reference: &str) -> Option<GpgKeyRef> {
        if let Some(path) = reference.strip_prefix("file://") {
            return (!path.is_empty()).then(|| GpgKeyRef::Local(path.to_owned()));
        }
        if reference.contains("://") {
            Some(GpgKeyRef::Remote(reference.to_owned()))
        } else {
            Some(GpgKeyRef::Local(reference.to_owned()))
        }
    }
}

/// One parsed `[section]` of a `.repo` file.
///
/// The one reading of `.repo` content: collection, ownership, provenance
/// and the export all read it, so none can disagree on continuation,
/// replacement, bare paths or stray lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSectionDef {
    pub id: SectionId,
    /// The section's `enabled=` value; `true` when the key is absent,
    /// matching dnf's own default.
    pub enabled: bool,
    /// Whether the section defines `baseurl`, `metalink`, or `mirrorlist`.
    pub has_source: bool,
    /// Every reference in the final `gpgkey=` value, split on whitespace
    /// and commas, in order.
    pub gpg_keys: Vec<GpgKeyRef>,
}

impl RepoSectionDef {
    /// The local key paths the section names, as written
    /// ([`GpgKeyRef::Local`]). Every consumer that asks which keys a
    /// section names reads this.
    pub fn local_keys(&self) -> impl Iterator<Item = &str> {
        self.gpg_keys.iter().filter_map(|r| match r {
            GpgKeyRef::Local(path) => Some(path.as_str()),
            GpgKeyRef::Remote(_) => None,
        })
    }
}

/// Trim a trailing `#` or `;` comment from an INI value.
fn strip_trailing_comment(value: &str) -> &str {
    match value.find(['#', ';']) {
        Some(idx) => &value[..idx],
        None => value,
    }
}

/// dnf's `enabled=` vocabulary: `1`, `true`, `yes`, `on` (case-insensitive)
/// are enabled; everything else, including malformed values, is disabled.
fn is_enabled_value(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// An option a [`RepoSectionDef`] is derived from.
#[derive(Clone, Copy)]
enum RepoOption {
    Enabled,
    Baseurl,
    Metalink,
    Mirrorlist,
    Gpgkey,
}

impl RepoOption {
    fn of(key: &str) -> Option<RepoOption> {
        Some(match key {
            "enabled" => RepoOption::Enabled,
            "baseurl" => RepoOption::Baseurl,
            "metalink" => RepoOption::Metalink,
            "mirrorlist" => RepoOption::Mirrorlist,
            "gpgkey" => RepoOption::Gpgkey,
            _ => return None,
        })
    }
}

/// The last value each option a [`RepoSectionDef`] is derived from was
/// assigned, across every block of one section.
#[derive(Default)]
struct SectionValues {
    enabled: Option<String>,
    baseurl: Option<String>,
    metalink: Option<String>,
    mirrorlist: Option<String>,
    gpgkey: Option<String>,
}

impl SectionValues {
    fn slot(&mut self, option: RepoOption) -> &mut Option<String> {
        match option {
            RepoOption::Enabled => &mut self.enabled,
            RepoOption::Baseurl => &mut self.baseurl,
            RepoOption::Metalink => &mut self.metalink,
            RepoOption::Mirrorlist => &mut self.mirrorlist,
            RepoOption::Gpgkey => &mut self.gpgkey,
        }
    }

    /// Derive the section dnf ends up with from the final option values.
    fn into_def(self, id: SectionId) -> RepoSectionDef {
        let defined = |v: &Option<String>| v.as_deref().is_some_and(|v| !v.trim().is_empty());
        RepoSectionDef {
            id,
            enabled: self
                .enabled
                .as_deref()
                .is_none_or(|v| is_enabled_value(v.trim())),
            has_source: defined(&self.baseurl)
                || defined(&self.metalink)
                || defined(&self.mirrorlist),
            gpg_keys: self
                .gpgkey
                .unwrap_or_default()
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|part| !part.is_empty())
                .filter_map(GpgKeyRef::parse)
                .collect(),
        }
    }
}

/// Parse `.repo`-style INI content into one [`RepoSectionDef`] per
/// `[section]` header.
///
/// Read the way dnf's ini reader reads it:
///
/// - **Continuation.** An indented line that follows a `key=value` line
///   continues that value. A comment line (`#` or `;` in column 0) or an
///   empty line ends the value; a line of only spaces or tabs stays inside
///   it. An indented line that follows no `key=value` line is read as a
///   line of its own.
/// - **Replacement.** A header repeated in the same content reopens its
///   section rather than starting another, as libdnf's `ConfigParser`
///   does, and the section keeps its first position. Each option keeps the
///   last value assigned to it in any block, and the section is derived
///   from those final values: `baseurl`, `metalink` and `mirrorlist` are
///   separate options, so clearing one leaves the others, and an empty
///   final value is no source; `gpg_keys` comes from the final `gpgkey=`
///   alone.
/// - **Stray lines.** Lines before the first section header, and lines
///   without a `key=value` form, are dropped rather than rejected:
///   malformed INI should degrade, not fail the scan.
/// - **Trailing comments.** A `#` or `;` and what follows it is cut from
///   each line of a value.
///
/// The header match is on the exact spelling, since dnf keys repos
/// case-sensitively; [`RepoId`]'s folded equality is not used here.
pub fn parse_repo_sections(content: &str) -> Vec<RepoSectionDef> {
    let mut sections: Vec<(SectionId, SectionValues)> = Vec::new();
    let mut current: Option<usize> = None;
    // The option the previous line assigned, when it is one this parser
    // keeps, so an indented line continues it. `Some(None)` is a
    // `key=value` line for an option it does not keep.
    let mut in_item: Option<Option<RepoOption>> = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            in_item = None;
            continue;
        }
        if trimmed.is_empty() {
            // A blank but not empty line stays inside the value.
            continue;
        }
        let indented = line.starts_with([' ', '\t']);
        if indented
            && !trimmed.starts_with('[')
            && let Some(item) = in_item
        {
            if let (Some(option), Some(idx)) = (item, current) {
                let piece = strip_trailing_comment(trimmed).trim();
                let slot = sections[idx].1.slot(option);
                let value = slot.get_or_insert_with(String::new);
                value.push('\n');
                value.push_str(piece);
            }
            continue;
        }
        in_item = None;

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let header = &trimmed[1..trimmed.len() - 1];
            current = Some(
                match sections.iter().position(|(id, _)| id.as_str() == header) {
                    Some(idx) => idx,
                    None => {
                        sections.push((SectionId::new(header), SectionValues::default()));
                        sections.len() - 1
                    }
                },
            );
            continue;
        }

        let Some(idx) = current else {
            continue; // line outside any section
        };
        let Some((key, value)) = trimmed.split_once('=') else {
            continue; // malformed: no key=value
        };
        let option = RepoOption::of(key.trim());
        in_item = Some(option);
        if let Some(option) = option {
            let value = strip_trailing_comment(value).trim();
            *sections[idx].1.slot(option) = Some(value.to_owned());
        }
    }

    sections
        .into_iter()
        .map(|(id, values)| values.into_def(id))
        .collect()
}

/// The key dnf 4 sorts `.repo` files by: the path without its `.repo`
/// suffix, split into components (`dnf.util.split_path`). Without the cut,
/// `x-y.repo` would sort before `x.repo`; dnf reads `x` first.
pub fn dnf_read_order(path: &str) -> Vec<&str> {
    path.strip_suffix(".repo")
        .unwrap_or(path)
        .split('/')
        .collect()
}

/// Which of the section ids `defined` a recorded or requested `wanted`
/// names: the one spelled exactly as `wanted`, or, when none is, the one
/// case variant of it there is. Two or more case variants and no exact
/// spelling is ambiguous, and names none.
///
/// dnf keys repos by their exact spelling, so `[r]` and `[R]` are two
/// repos and `--enablerepo` takes the one spelled as given. [`RepoId`]'s
/// folded equality is right for attribution, where a recorded short name
/// is matched loosely, and wrong for choosing which section that is; this
/// is the choice. Every place that turns a wanted id into one section asks
/// it, so they cannot pick different case variants.
pub fn section_spelling<'a>(
    defined: impl IntoIterator<Item = &'a str>,
    wanted: &str,
) -> Option<&'a str> {
    let mut folded: Option<&'a str> = None;
    let mut ambiguous = false;
    for spelling in defined {
        if spelling == wanted {
            return Some(spelling);
        }
        if spelling.eq_ignore_ascii_case(wanted) {
            match folded {
                Some(seen) if seen != spelling => ambiguous = true,
                _ => folded = Some(spelling),
            }
        }
    }
    folded.filter(|_| !ambiguous)
}

/// The definition of `id` dnf uses, given every `.repo` file it reads as
/// `(path, content)`.
///
/// dnf 4 merges the file lists of every `reposdir`, sorts them by
/// [`dnf_read_order`] (`dnf/conf/read.py`, `RepoReader`), and the first
/// file to define an id wins: `RepoDict.add` refuses a later one with
/// "Repository ... is listed more than once in the configuration" and dnf
/// goes on without it. Repeated headers inside one file are merged by
/// [`parse_repo_sections`]. The order of `files` does not enter into it;
/// two files with the same path, which a host cannot have, fall back to
/// their content so the answer is still the same in any order.
///
/// Ids are case-sensitive to dnf, so the section is the one spelled as
/// `id` is, in any file, and a case variant only when it is the one case
/// variant any file defines ([`section_spelling`]). The returned section's
/// id is that spelling.
///
/// Which files to pass is the caller's question, and the whole of it: the
/// files the host's dnf reads, or the ones an image build will have. The
/// answer is only as right as that set.
pub fn effective_definition<'a>(
    files: impl IntoIterator<Item = (&'a str, &'a str)>,
    id: &RepoId,
) -> Option<RepoSectionDef> {
    effective_definition_in(files, id).map(|(_, sec)| sec)
}

/// [`effective_definition`], with the path of the file that holds it.
pub fn effective_definition_in<'a>(
    files: impl IntoIterator<Item = (&'a str, &'a str)>,
    id: &RepoId,
) -> Option<(&'a str, RepoSectionDef)> {
    let defs: Vec<(Vec<&str>, &str, &str, RepoSectionDef)> = files
        .into_iter()
        .flat_map(|(path, content)| {
            parse_repo_sections(content)
                .into_iter()
                .filter(|sec| RepoId::new(sec.id.as_str()) == *id)
                .map(move |sec| (dnf_read_order(path), content, path, sec))
        })
        .collect();
    let spelling = section_spelling(
        defs.iter().map(|(_, _, _, sec)| sec.id.as_str()),
        id.as_str(),
    )?
    .to_owned();
    defs.into_iter()
        .filter(|(_, _, _, sec)| sec.id.as_str() == spelling)
        .min_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)))
        .map(|(_, _, path, sec)| (path, sec))
}

/// Whether refine's repo toggle cascades to `repo`'s members: not a
/// distro repo, and the definition the source host's dnf uses for it
/// among `files` (`(path, content)`, every `.repo` file the snapshot
/// holds) is enabled. A repo that is not toggleable can still have a
/// carriage-only switch for the files and keys it owns.
pub fn toggleable<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>, repo: &str) -> bool {
    !matches!(repo_tier(repo), RepoTier::Distro)
        && effective_definition(files, &RepoId::new(repo)).is_some_and(|def| def.enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_tier_distro() {
        assert_eq!(repo_tier("baseos"), RepoTier::Distro);
        assert_eq!(repo_tier("appstream"), RepoTier::Distro);
        assert_eq!(repo_tier("AppStream"), RepoTier::Distro);
        assert_eq!(repo_tier("anaconda"), RepoTier::Distro);
        assert_eq!(repo_tier("updates-testing"), RepoTier::Distro);
        assert_eq!(repo_tier("extras"), RepoTier::Distro);
    }

    #[test]
    fn test_repo_tier_distro_rhel_long_ids() {
        // RHEL systems use long repo IDs like rhel-9-for-x86_64-baseos-rpms
        assert_eq!(repo_tier("rhel-9-for-x86_64-baseos-rpms"), RepoTier::Distro);
        assert_eq!(
            repo_tier("rhel-9-for-x86_64-appstream-rpms"),
            RepoTier::Distro
        );
        assert_eq!(
            repo_tier("rhel-9-for-aarch64-baseos-rpms"),
            RepoTier::Distro
        );
        // Case-insensitive
        assert_eq!(repo_tier("RHEL-9-for-x86_64-BaseOS-rpms"), RepoTier::Distro);
    }

    #[test]
    fn test_repo_tier_official_optional() {
        assert_eq!(repo_tier("crb"), RepoTier::OfficialOptional);
        assert_eq!(repo_tier("CRB"), RepoTier::OfficialOptional);
        assert_eq!(repo_tier("rhel-extensions"), RepoTier::OfficialOptional);
    }

    #[test]
    fn test_repo_tier_official_optional_rhel_long_ids() {
        assert_eq!(
            repo_tier("codeready-builder-for-rhel-9-x86_64-rpms"),
            RepoTier::OfficialOptional
        );
    }

    #[test]
    fn test_repo_tier_third_party() {
        assert_eq!(repo_tier("epel"), RepoTier::ThirdParty);
        assert_eq!(repo_tier("copr:mytools"), RepoTier::ThirdParty);
    }

    #[test]
    fn test_repo_tier_no_false_segment_match() {
        // "myappstream-extras" should NOT match "appstream" because
        // the keyword is not at a segment boundary (preceded by 'y', not '-')
        assert_eq!(repo_tier("myappstream-extras"), RepoTier::ThirdParty);
    }

    #[test]
    fn test_repo_tier_none() {
        assert_eq!(repo_tier(""), RepoTier::None);
        assert_eq!(repo_tier("@commandline"), RepoTier::None);
        assert_eq!(repo_tier("@CommandLine"), RepoTier::None);
    }

    #[test]
    fn repo_id_folds_ascii_case_only() {
        assert_eq!(
            RepoId::new("EPEL_10_Everything_x86_64"),
            RepoId::new("epel_10_everything_x86_64")
        );
        // Review Focus 1: Eq, Hash and Ord must agree, and must not fold non-ASCII.
        assert_ne!(RepoId::new("repo-Ü"), RepoId::new("repo-ü"));
        let mut set = std::collections::HashSet::new();
        set.insert(RepoId::new("EPEL"));
        assert!(set.contains(&RepoId::new("epel")));
        assert!(!set.contains(&RepoId::new("repo-Ü")) || set.len() == 1);
        assert_eq!(RepoId::new("EPEL").to_string(), "EPEL"); // Display keeps the host's spelling
    }

    #[test]
    fn parse_repo_sections_reads_dnf_enabled_vocabulary() {
        // Review Focus 2.
        let content = "\
stray = line outside any section
[a]
baseurl=https://example.invalid/a
[b]
enabled=0
metalink=https://example.invalid/b
[c]
enabled = True   # trailing comment
mirrorlist=https://example.invalid/c
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-C
[d]
enabled=yes
";
        let got = parse_repo_sections(content);
        let by = |n: &str| got.iter().find(|s| s.id.as_str() == n).unwrap();
        assert_eq!(got.len(), 4);
        assert!(by("a").enabled && by("a").has_source); // absent enabled= is true
        assert!(!by("b").enabled && by("b").has_source);
        assert!(by("c").enabled);
        assert_eq!(
            by("c").local_keys().collect::<Vec<_>>(),
            vec!["/etc/pki/rpm-gpg/RPM-GPG-KEY-C"]
        );
        assert!(by("d").enabled);
    }

    #[test]
    fn parse_repo_sections_merges_a_repeated_header_as_dnf_does() {
        // libdnf's ConfigParser keeps one section per exact header: a
        // repeated `[r]` reopens it, and a later key overwrites an earlier
        // one. A header differing only in case is a different repo to dnf.
        let content = "\
[r]
baseurl=https://example.invalid/r
gpgkey=file:///etc/pki/rpm-gpg/KEY-1
[other]
enabled=0
[r]
enabled=0
gpgkey=file:///etc/pki/rpm-gpg/KEY-2
[R]
enabled=1
";
        let got = parse_repo_sections(content);
        let spellings: Vec<&str> = got.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(spellings, vec!["r", "other", "R"]);
        let r = &got[0];
        assert!(!r.enabled, "the second block's enabled=0 applies");
        assert!(r.has_source, "the first block's baseurl still applies");
        assert_eq!(
            r.local_keys().collect::<Vec<_>>(),
            vec!["/etc/pki/rpm-gpg/KEY-2"],
            "the last gpgkey= replaces the first, as dnf keeps the last value"
        );
    }

    #[test]
    fn parse_repo_sections_keeps_the_last_value_of_each_key() {
        // libdnf's ConfigParser assigns each option once per key: a key
        // repeated across reopened blocks keeps its last value, and the
        // three source keys are separate options.
        struct Case {
            name: &'static str,
            content: &'static str,
            enabled: bool,
            has_source: bool,
            keys: &'static [&'static str],
        }
        let cases = [
            Case {
                name: "source cleared and repo disabled",
                content: "[r]\nbaseurl=https://example.invalid/r\nenabled=1\n[r]\nbaseurl=\nenabled=0\n",
                enabled: false,
                has_source: false,
                keys: &[],
            },
            Case {
                name: "metalink survives a cleared baseurl",
                content: "[r]\nbaseurl=https://example.invalid/r\nmetalink=https://example.invalid/m\n[r]\nbaseurl=\n",
                enabled: true,
                has_source: true,
                keys: &[],
            },
            Case {
                name: "a cleared mirrorlist leaves no source",
                content: "[r]\nmirrorlist=https://example.invalid/m\nmirrorlist=   # gone\n",
                enabled: true,
                has_source: false,
                keys: &[],
            },
            Case {
                name: "an old key replaced by a new one",
                content: "[r]\nbaseurl=https://example.invalid/r\ngpgkey=file:///etc/pki/rpm-gpg/OLD\n[r]\ngpgkey=file:///etc/pki/rpm-gpg/NEW\n",
                enabled: true,
                has_source: true,
                keys: &["/etc/pki/rpm-gpg/NEW"],
            },
            Case {
                name: "a cleared gpgkey names no key",
                content: "[r]\nbaseurl=https://example.invalid/r\ngpgkey=file:///etc/pki/rpm-gpg/OLD\ngpgkey=\n",
                enabled: true,
                has_source: true,
                keys: &[],
            },
        ];
        for case in cases {
            let got = parse_repo_sections(case.content);
            assert_eq!(got.len(), 1, "{}", case.name);
            let r = &got[0];
            assert_eq!(r.enabled, case.enabled, "{}: enabled", case.name);
            assert_eq!(r.has_source, case.has_source, "{}: has_source", case.name);
            assert_eq!(
                r.local_keys().collect::<Vec<_>>(),
                case.keys,
                "{}: gpg_keys",
                case.name
            );
        }
    }

    #[test]
    fn effective_definition_prefers_the_exact_spelling_of_a_case_variant_id() {
        // dnf keys repos by exact spelling: `[ab]` and `[AB]` are two repos,
        // in one file or in two. The wanted spelling picks its own section;
        // a case variant stands in only when it is the only one, and two
        // case variants with no exact spelling name none. Every case runs
        // with the files in both orders.
        const LOWER: &str = "[ab]\nbaseurl=https://example.invalid/lower\nenabled=0\n";
        const UPPER: &str = "[AB]\nbaseurl=https://example.invalid/upper\nenabled=1\n";
        const MIXED: &str = "[AB]\nenabled=0\n";
        // `(what, files as (path, content), wanted, expected id and enabled)`.
        type Case<'a> = (
            &'a str,
            &'a [(&'a str, &'a str)],
            &'a str,
            Option<(&'a str, bool)>,
        );
        const BOTH: &str = "[ab]\nbaseurl=https://example.invalid/lower\nenabled=0\n\
                            [AB]\nbaseurl=https://example.invalid/upper\nenabled=1\n";
        let same_file: &[(&str, &str)] = &[("/etc/yum.repos.d/a.repo", BOTH)];
        let cases: &[Case] = &[
            ("same file, wanted ab", same_file, "ab", Some(("ab", false))),
            ("same file, wanted AB", same_file, "AB", Some(("AB", true))),
            ("same file, wanted Ab: two variants", same_file, "Ab", None),
            (
                // `A.repo` sorts before `b.repo`, so a folded match that
                // ignored the spelling would take `[AB]` for `ab`.
                "cross file, wanted ab",
                &[
                    ("/etc/yum.repos.d/A.repo", UPPER),
                    ("/etc/yum.repos.d/b.repo", LOWER),
                ],
                "ab",
                Some(("ab", false)),
            ),
            (
                "cross file, wanted AB",
                &[
                    ("/etc/yum.repos.d/A.repo", UPPER),
                    ("/etc/yum.repos.d/b.repo", LOWER),
                ],
                "AB",
                Some(("AB", true)),
            ),
            (
                "cross file, wanted Ab: two variants",
                &[
                    ("/etc/yum.repos.d/A.repo", UPPER),
                    ("/etc/yum.repos.d/b.repo", LOWER),
                ],
                "Ab",
                None,
            ),
            (
                "one case variant stands in",
                &[("/etc/yum.repos.d/b.repo", UPPER)],
                "ab",
                Some(("AB", true)),
            ),
            (
                "the one variant, in two files: dnf's first file",
                &[
                    ("/etc/yum.repos.d/b.repo", MIXED),
                    ("/etc/yum.repos.d/a.repo", UPPER),
                ],
                "ab",
                Some(("AB", true)),
            ),
        ];
        for (what, files, wanted, expected) in cases {
            let mut reversed = files.to_vec();
            reversed.reverse();
            for (order, set) in [("in order", files.to_vec()), ("reversed", reversed)] {
                let got = effective_definition(set, &RepoId::new(*wanted));
                assert_eq!(
                    got.as_ref().map(|sec| (sec.id.as_str(), sec.enabled)),
                    *expected,
                    "{what}, {order}"
                );
            }
        }
    }

    #[test]
    fn parse_repo_sections_reads_gpgkey_values_as_dnf_does() {
        // One parser for collection and ownership (§ 3.1 of the repo
        // ownership spec): continuation, replacement, bare paths, remote
        // references and stray lines, each as dnf's ini reader reads them.
        struct Case {
            name: &'static str,
            content: &'static str,
            keys: &'static [&'static str],
            remote: &'static [&'static str],
        }
        let cases = [
            Case {
                name: "an indented line continues the value",
                content: "[r]\ngpgkey=file:///etc/pki/rpm-gpg/A\n  file:///etc/pki/rpm-gpg/B\n",
                keys: &["/etc/pki/rpm-gpg/A", "/etc/pki/rpm-gpg/B"],
                remote: &[],
            },
            Case {
                name: "a line of only spaces stays inside the value",
                content: "[r]\ngpgkey=file:///etc/pki/rpm-gpg/A\n   \n\tfile:///etc/pki/rpm-gpg/B\n",
                keys: &["/etc/pki/rpm-gpg/A", "/etc/pki/rpm-gpg/B"],
                remote: &[],
            },
            Case {
                name: "an empty line ends the value",
                content: "[r]\ngpgkey=file:///etc/pki/rpm-gpg/A\n\n  file:///etc/pki/rpm-gpg/B\n",
                keys: &["/etc/pki/rpm-gpg/A"],
                remote: &[],
            },
            Case {
                name: "a comment line ends the value",
                content: "[r]\ngpgkey=file:///etc/pki/rpm-gpg/A\n# note\n  file:///etc/pki/rpm-gpg/B\n",
                keys: &["/etc/pki/rpm-gpg/A"],
                remote: &[],
            },
            Case {
                name: "a later assignment replaces the earlier one",
                content: "[r]\ngpgkey=file:///etc/pki/rpm-gpg/OLD\ngpgkey=file:///etc/pki/rpm-gpg/NEW\n",
                keys: &["/etc/pki/rpm-gpg/NEW"],
                remote: &[],
            },
            Case {
                name: "a bare path is a local key",
                content: "[r]\ngpgkey=/etc/pki/rpm-gpg/BARE\n",
                keys: &["/etc/pki/rpm-gpg/BARE"],
                remote: &[],
            },
            Case {
                name: "a remote key is kept apart",
                content: "[r]\ngpgkey=https://example.invalid/KEY, file:///etc/pki/rpm-gpg/L\n",
                keys: &["/etc/pki/rpm-gpg/L"],
                remote: &["https://example.invalid/KEY"],
            },
            Case {
                name: "a gpgkey= before the first header names nothing",
                content: "gpgkey=file:///etc/pki/rpm-gpg/PRE\n[r]\nbaseurl=https://example.invalid/r\n",
                keys: &[],
                remote: &[],
            },
        ];
        for case in cases {
            let got = parse_repo_sections(case.content);
            assert_eq!(got.len(), 1, "{}", case.name);
            assert_eq!(
                got[0].local_keys().collect::<Vec<_>>(),
                case.keys,
                "{}",
                case.name
            );
            let remote: Vec<&str> = got[0]
                .gpg_keys
                .iter()
                .filter_map(|r| match r {
                    GpgKeyRef::Remote(url) => Some(url.as_str()),
                    GpgKeyRef::Local(_) => None,
                })
                .collect();
            assert_eq!(remote, case.remote, "{}", case.name);
        }
        assert!(parse_repo_sections("# comments only\n; and more\n").is_empty());
    }

    #[test]
    fn section_ids_keep_their_case() {
        let got = parse_repo_sections("[site]\n[SITE]\n");
        let ids: std::collections::BTreeSet<&SectionId> = got.iter().map(|s| &s.id).collect();
        assert_eq!(ids.len(), 2, "[site] and [SITE] are two repos");
    }
}
