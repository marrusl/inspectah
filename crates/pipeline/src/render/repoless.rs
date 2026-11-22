use std::collections::BTreeSet;

use inspectah_core::rpm_filename::StagedName;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::group_render::RenderContext;
use inspectah_core::types::repo::{
    RepoAttribution, RepoId, RepoSectionDef, SectionId, effective_definition, effective_section_in,
    parse_repo_sections, section_spelling,
};
use inspectah_core::types::rpm::{PackageEntry, RpmSection};
use inspectah_core::types::verified_upload::{VerifiedUpload, VerifiedUploads};

use super::configtree::{StagedWrites, is_staged, staged_writes};
use super::language_packages::sanitize_for_comment;
use super::repo_enabling::{keys_included, serves};
use super::safety::sanitize_shell_value;
use super::unit_plan::is_package_installable;

/// How one `packages_added` entry reaches the image.
///
/// This is the one routing decision for a package's install, and every
/// party that acts on it asks [`route_for`]: the Containerfile's package
/// sections, the repo-less block, refine's trust gate, the `/var` evidence's
/// install set, and every writer of `repoless-packages/` (the scan
/// tarball's [`write_cached_rpms`], refine's payload extraction, and
/// refine's upload copy). A writer stages a file only for a route whose
/// lines `COPY` it, so no export carries an RPM that nothing installs.
///
/// The route says how a package would install, not whether it does: the
/// package's disposition is applied by each consumer, as it always was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageRoute<'a> {
    /// Not repo-less: installs by name from a repo the host enables, in the
    /// plain list or the repo-enabling block.
    ByName,
    /// Installs from a repo the source host keeps disabled, in the package
    /// phase's combined install, which carries `--enablerepo=<id>` for
    /// every such repo. Nothing is staged for it.
    DisabledRepo(RepoId),
    /// Not repo-less on the host, but the section it installed from there,
    /// by exact spelling, is one the build cannot install it from
    /// ([`InstallRepos::lost_route`]): the export drops its `.repo` file,
    /// the file's definition is not the one dnf reads, or a key dnf needs
    /// for it is not in the image, and the base image does not supply it.
    /// It renders the demotion FIXME naming what restores it, and takes an
    /// upload, until the repo's files and keys are back.
    LostRepo(SectionId),
    /// Repo-less, installed from the operator's upload, which the export
    /// proved and writes, by path in the same combined install. An upload is a decision the operator made, so it
    /// wins over a disabled repo the export could install from.
    Upload(&'a VerifiedUpload),
    /// Repo-less otherwise: the cached RPM the scan bundled, the FIXME for
    /// an upload nothing proved, or the MANUAL block.
    Repoless,
}

/// The route for `pkg`, given the uploads this export proved.
///
/// [`PackageRoute::Repoless`] covers both an included cached RPM, which
/// also installs by path in the combined install, and the advice-only
/// outcomes; [`repoless_plan`] tells them apart.
///
/// In order: no repo-less annotation and a route the export still carries
/// is [`PackageRoute::ByName`]; a verified upload is
/// [`PackageRoute::Upload`]; a route the export no longer carries
/// ([`InstallRepos::lost_route`]) is [`PackageRoute::LostRepo`]; a disabled
/// repo the export can install from ([`disabled_repo_install`]) is
/// [`PackageRoute::DisabledRepo`]; anything else is
/// [`PackageRoute::Repoless`]. The lost route is read from the snapshot's
/// repo files and this export's staging, never from the annotation, so
/// refine's demotion annotation does not change it. A cached RPM is not an operator decision, so
/// it does not outrank the disabled repo. A path with no uploads, the scan
/// tarball or refine before any upload, passes an empty map. `repos` is
/// the export's [`InstallRepos`], built once from the same snapshot.
pub fn route_for<'a>(
    rpm: &RpmSection,
    repos: &InstallRepos,
    pkg: &PackageEntry,
    verified: &'a VerifiedUploads,
) -> PackageRoute<'a> {
    let lost = repos.lost_route(pkg);
    if pkg.repoless_annotation.is_empty() && lost.is_none() {
        return PackageRoute::ByName;
    }
    if let Some(upload) = verified.get(&format!("{}.{}", pkg.name, pkg.arch)) {
        return PackageRoute::Upload(upload);
    }
    if let Some(id) = lost {
        return PackageRoute::LostRepo(id);
    }
    match disabled_repo_install(rpm, repos, pkg) {
        Some(id) => PackageRoute::DisabledRepo(id),
        None => PackageRoute::Repoless,
    }
}

/// Copy each cached repo-less RPM into `repoless-packages/` under
/// `render_dir`, for the scan tarball.
///
/// Only a package routed [`PackageRoute::Repoless`] is staged: a package
/// that installs from its disabled repo has no `COPY` naming its cached
/// RPM, and staging it anyway left an export carrying a file nothing
/// installs. The file is named by the package's NEVRA, the name the
/// repo-less block copies, and only once [`StagedName::parse`] admits it:
/// the four NEVRA fields come from the scanned host, and a name such as
/// `../evil` would otherwise be copied outside `repoless-packages/`.
///
/// Returns one warning line per package whose NEVRA the allowlist
/// refused. Such a package is not staged, and the Containerfile carries a
/// FIXME for it in place of a `COPY`; the caller prints the lines so the
/// operator knows the cached RPM did not make it into the tarball.
pub fn write_cached_rpms(
    snap: &InspectionSnapshot,
    render_dir: &std::path::Path,
) -> std::io::Result<Vec<String>> {
    let mut refused = Vec::new();
    let Some(rpm) = &snap.rpm else {
        return Ok(refused);
    };
    let repos = InstallRepos::of(snap);
    let no_uploads = VerifiedUploads::new();
    let dest_dir = render_dir.join("repoless-packages");
    for pkg in &rpm.packages_added {
        let Some(cache_path) = &pkg.cache_path else {
            continue;
        };
        if route_for(rpm, &repos, pkg, &no_uploads) != PackageRoute::Repoless {
            continue;
        }
        let nevra_filename = format!(
            "{}-{}-{}.{}.rpm",
            pkg.name, pkg.version, pkg.release, pkg.arch
        );
        let Ok(filename) = StagedName::parse(&nevra_filename) else {
            // Escaped for the terminal: these fields are host-supplied.
            refused.push(format!(
                "cached RPM for {} not bundled: its filename {} is not one \
                 the build can name; upload the RPM in refine",
                format!("{}.{}", pkg.name, pkg.arch).escape_debug(),
                nevra_filename.escape_debug(),
            ));
            continue;
        };
        std::fs::create_dir_all(&dest_dir)?;
        std::fs::copy(cache_path, dest_dir.join(filename.as_str())).map_err(|e| {
            std::io::Error::new(e.kind(), format!("copy cached RPM {cache_path}: {e}"))
        })?;
    }
    Ok(refused)
}

/// The repo a repo-less package installs from with `--enablerepo`, when
/// the source host has that repo disabled and this export carries it.
///
/// The scan marks a package repo-less when its `source_repo` is not in
/// `dnf repolist --enabled` and no enabled repo serves it by name. When
/// that repo is merely disabled (`enabled=0`) in a `.repo` file the export
/// ships, the package was installed from it on purpose, so the build
/// installs it from the same repo instead of asking for an upload. This
/// is decided at render, not at scan, because whether the `.repo` file
/// ships is a refine decision the scan cannot see. Callers reach it only
/// through [`route_for`], which refine's `normalize_package_defaults` asks
/// too to exempt such a package from its repo-less trust gate, so the two
/// cannot disagree.
///
/// Returns `None`, leaving the package repo-less, unless all of these
/// hold: it carries a repo-less annotation, it is installable by name
/// ([`is_package_installable`]: not a local install or no-repo state, and
/// its `source_repo` is a real repo id rather than `@commandline`), its
/// the scan did not attribute it to a pseudo repo
/// ([`RepoAttribution::Pseudo`]: `@System`, `anaconda`), the definition dnf
/// would use among the repo files the build has when it installs
/// ([`InstallRepos::effective`]) is disabled and defines a source, every
/// key dnf needs for that section ([`RepoSectionDef::required_keys`]) is
/// in the image (dnf cannot verify the package without it), and the id
/// and the package's name and arch pass `sanitize_shell_value`. The id
/// is returned as the `.repo` file spells it, since `--enablerepo` takes
/// it verbatim.
///
/// The package's own disposition is not one of the conditions. This says
/// how the package installs, not whether it does: an excluded one renders
/// nowhere, like any other excluded package from a repo, and a non-leaf
/// dependency the plain list's selection leaves to dnf is not offered in
/// the repo-less block either. That is what lets a scan tarball, where
/// such a dependency is included, and a refine export, where its default
/// is excluded, render it the same way.
fn disabled_repo_install(
    rpm: &RpmSection,
    repos: &InstallRepos,
    pkg: &PackageEntry,
) -> Option<RepoId> {
    if !is_package_installable(pkg)
        || pkg.repoless_annotation.is_empty()
        || pkg.source_repo.is_empty()
        || matches!(pkg.repo_attribution, RepoAttribution::Pseudo { .. })
        || sanitize_shell_value(&pkg.name).is_none()
        || sanitize_shell_value(&pkg.arch).is_none()
    {
        return None;
    }
    repos
        .effective(&RepoId::new(pkg.source_repo.as_str()))
        .filter(|sec| {
            !sec.enabled
                && sec.has_source
                && keys_included(rpm, repos, sec.required_keys())
                && sanitize_shell_value(sec.id.as_str()).is_some()
        })
        .map(|sec| RepoId::new(sec.id.as_str()))
}

/// Where the package phase's early `COPY` puts the export's repo files.
const YUM_REPOS_DIR: &str = "/etc/yum.repos.d";

/// The `.repo` files dnf reads when the Containerfile's package phase
/// installs, with the content the export stages for each: the one set the
/// disabled-repo route resolves a repo id against.
///
/// The package phase copies `config/etc/yum.repos.d/` into the image
/// before it installs anything, and only when the Custom Repositories
/// section renders, which it does when at least one included repo file
/// that is not a default repo file is staged ([`Self::custom_count`]).
/// That `COPY` carries the whole directory, so when it runs the image has
/// every `.repo` file staged there: a default repo file the operator
/// included too, and, where a config entry at the same path replaced a
/// repo file's content, the config entry's content. A repo file the
/// stager withholds or refuses is not there, and when the section does not
/// render nothing is: the rest of `config/etc/` is copied after the
/// packages are installed.
///
/// The files the base image itself ships in `/etc/yum.repos.d` are not
/// known to the export and are not part of the set.
///
/// It also holds the GPG keys the GPG Keys section copies, which it does
/// right after that `COPY` and before anything installs: an included key
/// the stager writes at an absolute, shell-safe path. A key the stager
/// withholds or refuses is not copied, however it is disposed.
#[derive(Debug, Clone, Default)]
pub struct InstallRepos {
    /// How many included, non-default repo files are staged.
    custom: usize,
    /// `(destination, staged content)` of each `.repo` file staged in the
    /// directory: what dnf reads at install time when the `COPY` renders
    /// (`custom > 0`), and nothing otherwise.
    files: Vec<(String, String)>,
    /// The paths of the `gpg_keys` entries the GPG Keys section copies.
    keys: BTreeSet<String>,
    /// Every section spelling the snapshot's repo files define, exactly as
    /// written: what a package's recorded repo resolves against, exact
    /// spelling first ([`section_spelling`]).
    spellings: BTreeSet<String>,
    /// The sections, by exact spelling, a package the source host
    /// installed by name from cannot be installed from in this export
    /// ([`route_lost`]).
    lost: BTreeSet<SectionId>,
}

impl InstallRepos {
    /// The set for `snap`, from the stager's own decision.
    pub fn of(snap: &InspectionSnapshot) -> Self {
        Self::from_staged(snap, &staged_writes(snap))
    }

    /// The set for `snap`, given its [`staged_writes`].
    pub fn from_staged(snap: &InspectionSnapshot, staged: &StagedWrites<'_>) -> Self {
        let custom = snap.rpm.as_ref().map_or(0, |rpm| {
            rpm.repo_files
                .iter()
                .filter(|r| r.disposition.is_included() && !r.is_default_repo)
                .filter(|r| is_staged(staged, &r.path))
                .count()
        });
        // dnf reads `<reposdir>/*.repo`: directly in the directory, and a
        // glob, so not a name starting with a dot.
        let files = staged
            .writes
            .iter()
            .filter_map(|(dest, write)| {
                let (dir, name) = dest.as_str().rsplit_once('/')?;
                (dir == YUM_REPOS_DIR && name.ends_with(".repo") && !name.starts_with('.'))
                    .then(|| (dest.as_str().to_owned(), write.content.to_string()))
            })
            .collect();
        // The same conditions the GPG Keys section applies before it names
        // a key in a `COPY`: included, staged, absolute and shell-safe.
        let keys = snap.rpm.as_ref().map_or_else(BTreeSet::new, |rpm| {
            rpm.gpg_keys
                .iter()
                .filter(|k| k.disposition.is_included() && is_staged(staged, &k.path))
                .filter(|k| k.path.starts_with('/') && sanitize_shell_value(&k.path).is_some())
                .map(|k| k.path.clone())
                .collect()
        });
        let spellings = snap.rpm.as_ref().map_or_else(BTreeSet::new, |rpm| {
            rpm.repo_files
                .iter()
                .flat_map(|f| parse_repo_sections(&f.content))
                .map(|sec| sec.id.as_str().to_owned())
                .collect()
        });
        let mut repos = Self {
            custom,
            files,
            keys,
            spellings,
            lost: BTreeSet::new(),
        };
        if let Some(rpm) = snap.rpm.as_ref() {
            let mut candidates: BTreeSet<SectionId> =
                repos.spellings.iter().map(SectionId::new).collect();
            candidates.extend(
                rpm.packages_added
                    .iter()
                    .filter_map(|p| repos.route_section(p)),
            );
            repos.lost = candidates
                .into_iter()
                .filter(|section| route_lost(rpm, &repos, section))
                .collect();
        }
        repos
    }

    /// Whether the build cannot install a package from `section`, by exact
    /// spelling, that the source host installed by name from it
    /// ([`route_lost`]). The one answer package routing, the repo-less
    /// advice and unit-owner holds read.
    pub(crate) fn is_lost(&self, section: &SectionId) -> bool {
        self.lost.contains(section)
    }

    /// The section `pkg` installed from on the source host, by exact
    /// spelling: the section spelled as its recorded repo, or else its one
    /// case variant, among the repo files the snapshot holds
    /// ([`section_spelling`]). When none is, and the collector attributed
    /// the package to a section a captured file defined
    /// ([`RepoAttribution::Exact`] or [`RepoAttribution::Resolved`], read
    /// from the files before redaction ran), that section: its definition
    /// is no longer in any file the snapshot holds, which is the evidence
    /// that no file the export copies can define it.
    fn route_section(&self, pkg: &PackageEntry) -> Option<SectionId> {
        if pkg.source_repo.is_empty() {
            return None;
        }
        if let Some(spelling) =
            section_spelling(self.spellings.iter().map(String::as_str), &pkg.source_repo)
        {
            return Some(SectionId::new(spelling));
        }
        match &pkg.repo_attribution {
            RepoAttribution::Exact { id } | RepoAttribution::Resolved { id, .. } => {
                Some(SectionId::new(id.as_str()))
            }
            _ => None,
        }
    }

    /// The section `pkg` installed from on the source host, when the build
    /// cannot install it from there: [`PackageRoute::LostRepo`].
    ///
    /// Only for a package installed by name (installable, not
    /// repo-enabling, which has its own proof, and not attributed to a
    /// pseudo repo), resolved to its section by [`Self::route_section`]
    /// and judged by [`route_lost`]. The package's disposition plays no
    /// part, as for every route.
    pub fn lost_route(&self, pkg: &PackageEntry) -> Option<SectionId> {
        if self.lost.is_empty()
            || !pkg.repo_enabling_from.is_empty()
            || !is_package_installable(pkg)
            || matches!(pkg.repo_attribution, RepoAttribution::Pseudo { .. })
        {
            return None;
        }
        self.route_section(pkg)
            .filter(|section| self.lost.contains(section))
    }

    /// Whether the GPG Keys section copies the captured key at `path`
    /// before the package phase installs.
    pub(crate) fn copies_key(&self, path: &str) -> bool {
        self.keys.contains(path)
    }

    /// How many included repo files that are not default repo files the
    /// export stages: the Custom Repositories count, and whether that
    /// section's `COPY` renders at all.
    pub fn custom_count(&self) -> usize {
        self.custom
    }

    /// The definition of `id` dnf uses when the package phase installs
    /// ([`effective_definition`] over this set).
    pub fn effective(&self, id: &RepoId) -> Option<RepoSectionDef> {
        if self.custom == 0 {
            return None;
        }
        effective_definition(self.staged_files(), id)
    }

    /// The definition of the section spelled exactly `section` dnf uses
    /// when the package phase installs ([`effective_section_in`] over this
    /// set): no case variant stands in for it.
    fn effective_section(&self, section: &SectionId) -> Option<RepoSectionDef> {
        if self.custom == 0 {
            return None;
        }
        effective_section_in(self.staged_files(), section).map(|(_, sec)| sec)
    }

    fn staged_files(&self) -> impl Iterator<Item = (&str, &str)> {
        self.files
            .iter()
            .map(|(path, content)| (path.as_str(), content.as_str()))
    }
}

/// The route-loss decision: whether the build cannot install, from a
/// repo the image will have, a package the source host's dnf installed by
/// name from `section`, by exact spelling. Package routing
/// ([`InstallRepos::lost_route`]), the advice that names a restoring
/// switch (`repo_enabling::lost_route_loss`) and unit-owner holds
/// (`not_carried`) all read this one answer, through
/// [`InstallRepos::is_lost`].
///
/// Lost exactly when every one of these holds:
///
/// 1. **The host installed by name from it.** The definition the host's
///    dnf used, among every `.repo` file the snapshot holds
///    ([`effective_section_in`]), is enabled and has a source. A section
///    no stored file defines counts too: the collector attributed a
///    package to it from the files as captured, before redaction, and
///    nothing the export copies can define it now.
/// 2. **The base image does not supply it** ([`base_supplies`]): the
///    host's definition is not in a default repo file, nor in a file a
///    base image package owns unmodified, such as Fedora's `fedora.repo`.
///    A distro-named section alone proves nothing: a `company.repo`
///    defining `[baseos]` for an internal mirror is what the host
///    installed from, and the base image's own `baseos`, where it has
///    one, need not serve the same packages.
/// 3. **The export does not serve it.** The definition of that exact
///    section among the files the build has when it installs
///    ([`InstallRepos::effective_section`]) is missing, disabled, without
///    a source, or names a key dnf needs that the image lacks
///    ([`serves`](super::repo_enabling::serves)). A key a section names
///    with signature checking off is not needed
///    ([`RepoSectionDef::required_keys`]).
fn route_lost(rpm: &RpmSection, repos: &InstallRepos, section: &SectionId) -> bool {
    let host_files = rpm
        .repo_files
        .iter()
        .map(|f| (f.path.as_str(), f.content.as_str()));
    let Some((path, host)) = effective_section_in(host_files, section) else {
        return true;
    };
    if !(host.enabled && host.has_source) || base_supplies(rpm, path) {
        return false;
    }
    !repos
        .effective_section(section)
        .is_some_and(|export| serves(rpm, repos, &export))
}

/// Whether the base image has its own copy of the repo file at `path`,
/// defining what the host's copy defines, so a section the host's dnf read
/// from it is the base image's to provide whatever the export does with
/// the host's copy:
///
/// - a default repo file (`is_default_repo`, `redhat.repo` among them),
///   which the subscription flow mounts at build time; or
/// - a file a package the base image has (`baseline_package_names`) owns
///   on the host (`file_ownership`) that `rpm -Va` did not report
///   modified (`rpm_va`): Fedora's `fedora-repos` and its `fedora.repo`
///   and `fedora-updates.repo`, CentOS Stream's `centos-stream-repos`.
///   This is the evidence the scan reads to leave out a key the base image
///   ships (`leave_out_keys_the_base_ships`), read from the snapshot. A
///   host copy someone changed, by enabling a section or adding one, is
///   reported by `rpm -Va` and is the host's own.
///
/// Without a baseline nothing is known about the base image's packages,
/// and only a default repo file counts.
fn base_supplies(rpm: &RpmSection, path: &str) -> bool {
    let Some(file) = rpm.repo_files.iter().find(|f| f.path == path) else {
        return false;
    };
    if file.is_default_repo {
        return true;
    }
    let Some(base) = rpm.baseline_package_names.as_ref() else {
        return false;
    };
    !rpm.rpm_va.iter().any(|e| e.path == path)
        && rpm
            .file_ownership
            .iter()
            .any(|e| base.contains(&e.package_name) && e.paths.iter().any(|p| p == path))
}

/// The commented `COPY`/`RUN` pair a pre-excluded cached RPM offers.
///
/// This and [`LocalRpm`] are the only places in this module an RPM
/// filename reaches a `COPY` or a `RUN`, and both take a [`StagedName`]
/// rather than a `&str`. That is deliberate and it is the fix for a real
/// defect: an earlier form built `RUN dnf localinstall -y /tmp/{name}-\
/// {version}-{release}.{arch}.rpm` straight from four snapshot fields, so a
/// scanned host reporting a package named `x;c6_exec` got that string
/// executed as root at image build time. A name cannot reach either
/// without having passed the allowlist, because `StagedName` has no other
/// constructor -- so the guarantee is the type's, not a check a new call
/// site can forget.
fn commented_install_lines(name: &StagedName) -> Vec<String> {
    let rpm = name.as_str();
    vec![
        format!("# COPY repoless-packages/{rpm} /tmp/"),
        format!("# RUN dnf localinstall -y /tmp/{rpm} \\"),
        format!("#     && rm /tmp/{rpm}"),
    ]
}

/// The two-line provenance warning every repo-less install block carries.
fn provenance_lines(pkg: &PackageEntry, source: &str) -> Vec<String> {
    vec![
        format!(
            "# Repo-less package: {} ({source}, no repository provenance)",
            sanitize_for_comment(&pkg.name)
        ),
        "# WARNING: This package has no upstream repo and no GPG verification.".into(),
    ]
}

/// The `name.arch` this package is keyed by, sanitized for a comment.
fn comment_id(pkg: &PackageEntry) -> String {
    sanitize_for_comment(&format!("{}.{}", pkg.name, pkg.arch))
}

/// One RPM the export stages under `repoless-packages/` and installs from
/// that file: a verified upload, or an included cached RPM.
pub(crate) struct LocalRpm {
    /// The staged basename, the only name its `COPY` and install carry.
    pub name: StagedName,
    /// The comments that say where the RPM came from.
    pub notes: Vec<String>,
}

/// What the repo-less packages contribute to the Containerfile.
///
/// The RPMs to install from file go into the package phase's combined
/// install, with the disabled-repo packages, so dnf resolves the two sets
/// together whichever depends on which. Everything else is advice: the
/// MANUAL block, the FIXMEs, and the commented block of a pre-excluded
/// cached RPM.
pub(crate) struct RepolessPlan {
    pub local: Vec<LocalRpm>,
    pub advice: Vec<String>,
}

/// Plan the repo-less packages.
///
/// Four outcomes per package, in the order they are decided:
///
/// 1. `render_ctx` holds a [`VerifiedUpload`] for it: a [`LocalRpm`]
///    naming the basename the export proved and will write.
/// 2. It claims an upload (`uploaded`) and is included, but no verified
///    upload reached here: the demotion FIXME. No `COPY`, and the export
///    response carries the matching warning. An entry the snapshot
///    arrived with is `uploaded: true` without anything having been staged,
///    so this is the ordinary path for an untrusted snapshot, not an
///    exception.
/// 3. `repoless_cached`: the host's dnf cache bundled the RPM at scan time,
///    so the filename is its NEVRA. A [`LocalRpm`] when included, a
///    commented block when pre-excluded, and a FIXME instead when that
///    NEVRA is not a name the allowlist admits.
/// 4. Neither: the MANUAL comment block, except for a package in
///    `advised`, the demoted repo-enabling packages whose demotion FIXME
///    the packages section already printed. That FIXME names the controls
///    that restore the package; a MANUAL block beside it said to add a
///    repo or uncomment an install that cannot work, so one package got
///    two contradicting answers.
pub(crate) fn repoless_plan(
    snap: &InspectionSnapshot,
    render_ctx: Option<&RenderContext>,
    advised: &std::collections::BTreeSet<(&str, &str)>,
) -> RepolessPlan {
    let mut plan = RepolessPlan {
        local: Vec::new(),
        advice: Vec::new(),
    };
    let Some(rpm_section) = &snap.rpm else {
        return plan;
    };

    let empty = VerifiedUploads::new();
    let verified = render_ctx.map_or(&empty, |c| &c.verified_uploads);
    let repos = InstallRepos::of(snap);

    // A package that installs by name, or from its host-disabled repo,
    // renders in the packages section instead, so it is not offered here.
    for pkg in &rpm_section.packages_added {
        match route_for(rpm_section, &repos, pkg, verified) {
            PackageRoute::Upload(upload) => plan.local.push(uploaded_rpm(pkg, upload)),
            PackageRoute::Repoless => plan_repoless(pkg, advised, &mut plan),
            // A lost route's FIXME is printed by the packages section, for
            // a package it would have named.
            PackageRoute::ByName | PackageRoute::DisabledRepo(_) | PackageRoute::LostRepo(_) => {}
        }
    }
    plan
}

/// Outcomes 2 to 4 of [`repoless_plan`] for one package.
fn plan_repoless(
    pkg: &PackageEntry,
    advised: &std::collections::BTreeSet<(&str, &str)>,
    plan: &mut RepolessPlan,
) {
    if pkg.uploaded && pkg.disposition.is_included() {
        plan.advice.push(format!(
            "# FIXME: upload the RPM for {} again in refine; the staged file \
             could not be verified for export",
            comment_id(pkg)
        ));
    } else if pkg.repoless_cached {
        plan_cached(pkg, plan);
    } else if advised.contains(&(pkg.name.as_str(), pkg.arch.as_str())) {
        // Its demotion FIXME is its advice.
    } else {
        // No cached RPM — manual resolution
        plan.advice.push(format!(
            "# MANUAL: {} (no repo source, RPM not in cache)",
            sanitize_for_comment(&pkg.name)
        ));
        plan.advice
            .push("# Provide the RPM via the refine UI upload, add a repo, or uncomment:".into());
        // Sanitized for the same reason the line above is: a newline in
        // a scanned package name would end the comment and turn the
        // rest of the name into an active instruction.
        plan.advice.push(format!(
            "# RUN dnf install {}",
            sanitize_for_comment(&pkg.name)
        ));
    }
}

/// The local RPM for a package whose staged upload the export proved.
fn uploaded_rpm(pkg: &PackageEntry, upload: &VerifiedUpload) -> LocalRpm {
    let mut notes = provenance_lines(pkg, "uploaded RPM");
    notes.push("# It was uploaded in refine. Updates must be managed manually.".into());
    if let Some(uploaded_version) = &upload.uploaded_version {
        // `uploaded_version` is re-derived from the proved basename, so it
        // needs no sanitizing; the other three operands come from the
        // snapshot and do.
        notes.push(format!(
            "# uploaded {}-{uploaded_version}.{}; the host had {}",
            sanitize_for_comment(&pkg.name),
            sanitize_for_comment(&pkg.arch),
            sanitize_for_comment(&format!("{}-{}", pkg.version, pkg.release)),
        ));
    }
    LocalRpm {
        name: upload.basename.clone(),
        notes,
    }
}

/// A package the host's dnf cache supplied at scan time.
fn plan_cached(pkg: &PackageEntry, plan: &mut RepolessPlan) {
    let nevra_filename = format!(
        "{}-{}-{}.{}.rpm",
        pkg.name, pkg.version, pkg.release, pkg.arch
    );
    let Ok(name) = StagedName::parse(&nevra_filename) else {
        // Rejection is rejection: there is no sanitized form of this
        // filename that would name the right file, so the block is advice
        // rather than a `COPY` of a name nothing proved.
        plan.advice.push(format!(
            "# FIXME: upload the RPM for {} in refine; the cached RPM's filename \
             is not one this can name in a COPY",
            comment_id(pkg)
        ));
        return;
    };

    let mut notes = provenance_lines(pkg, "cached RPM");
    if pkg.disposition.is_included() {
        notes.push(
            "# It was found in the local dnf cache. Updates must be managed manually.".into(),
        );
        plan.local.push(LocalRpm { name, notes });
    } else {
        plan.advice.extend(notes);
        plan.advice
            .push("# Pre-excluded — uncomment after verifying provenance:".into());
        plan.advice.extend(commented_install_lines(&name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::FindingKind;
    use inspectah_core::types::repo::PseudoRepo;
    use inspectah_core::types::rpm::{PackageEntry, PackageState, RepoFile, RpmSection};

    /// The rendered Containerfile, line by line: repo-less packages are
    /// planned here and rendered in the package phase's combined install,
    /// so the whole render is what a test reads.
    fn repoless_rpm_lines(
        snap: &InspectionSnapshot,
        render_ctx: Option<&RenderContext>,
    ) -> Vec<String> {
        super::super::containerfile::render_containerfile(snap, None, render_ctx)
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn test_snapshot_with_repoless(packages: Vec<PackageEntry>) -> InspectionSnapshot {
        InspectionSnapshot {
            rpm: Some(RpmSection {
                packages_added: packages,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn cached_rpm_pre_excluded_renders_commented() {
        let snap = test_snapshot_with_repoless(vec![PackageEntry {
            name: "custom-tool".into(),
            version: "1.2.3".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            source_repo: String::new(),
            disposition: FindingKind::excluded(),
            repoless_cached: true,
            repoless_annotation: "No repo source — cached RPM bundled".into(),
            ..Default::default()
        }]);
        let lines = repoless_rpm_lines(&snap, None);
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("# COPY repoless-packages/"))
        );
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("# RUN dnf localinstall"))
        );
    }

    #[test]
    fn cached_rpm_user_included_renders_active() {
        let snap = test_snapshot_with_repoless(vec![PackageEntry {
            name: "custom-tool".into(),
            version: "1.2.3".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            source_repo: String::new(),
            disposition: FindingKind::included(),
            repoless_cached: true,
            repoless_annotation: "No repo source — cached RPM bundled".into(),
            ..Default::default()
        }]);
        let lines = repoless_rpm_lines(&snap, None);
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("COPY repoless-packages/"))
        );
        assert!(
            lines
                .iter()
                .any(|l| l == "    /tmp/custom-tool-1.2.3-1.el9.x86_64.rpm \\")
        );
    }

    #[test]
    fn missing_rpm_renders_manual_block() {
        let snap = test_snapshot_with_repoless(vec![PackageEntry {
            name: "custom-tool".into(),
            version: "1.2.3".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            source_repo: String::new(),
            disposition: FindingKind::excluded(),
            repoless_cached: false,
            repoless_annotation: "No repo source — manual resolution needed".into(),
            ..Default::default()
        }]);
        let lines = repoless_rpm_lines(&snap, None);
        assert!(lines.iter().any(|l| l.contains("MANUAL: custom-tool")));
        assert!(lines.iter().any(|l| l.contains("dnf install custom-tool")));
    }

    #[test]
    fn packages_with_repo_not_rendered() {
        let snap = test_snapshot_with_repoless(vec![PackageEntry {
            name: "httpd".into(),
            source_repo: "appstream".into(),
            ..Default::default()
        }]);
        let lines = repoless_rpm_lines(&snap, None);
        assert!(
            !lines
                .iter()
                .any(|l| l.contains("Repo-less") || l.contains("repoless-packages")),
            "{lines:?}"
        );
    }

    #[test]
    fn disabled_repo_package_renders_as_repoless() {
        let snap = test_snapshot_with_repoless(vec![PackageEntry {
            name: "internal-tool".into(),
            version: "2.0".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            source_repo: "internal-tools".into(), // non-empty but disabled
            disposition: FindingKind::excluded(),
            repoless_cached: true,
            repoless_annotation:
                "No repo source — repo 'internal-tools' not in enabled repos — cached RPM bundled"
                    .into(),
            ..Default::default()
        }]);
        let lines = repoless_rpm_lines(&snap, None);
        assert!(lines.iter().any(|l| l.contains("Repo-less package")));
    }

    /// The lines a build actually runs that concern a repo-less RPM file:
    /// not comments, and naming `repoless-packages/` or `/tmp/`.
    fn active_lines(lines: &[String]) -> Vec<&str> {
        lines
            .iter()
            .map(|l| l.as_str())
            .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
            .filter(|l| l.contains("repoless-packages/") || l.contains("/tmp/"))
            .collect()
    }

    /// One repo-less entry whose four NEVRA fields are taken verbatim.
    fn nevra_entry(name: &str, version: &str, release: &str, arch: &str) -> PackageEntry {
        PackageEntry {
            name: name.into(),
            version: version.into(),
            release: release.into(),
            arch: arch.into(),
            source_repo: String::new(),
            disposition: FindingKind::included(),
            repoless_cached: true,
            repoless_annotation: "No repo source — cached RPM bundled".into(),
            ..Default::default()
        }
    }

    /// NEVRAs whose concatenated filename the allowlist refuses. These
    /// four fields come from a scanned host's rpmdb.
    ///
    /// Hand-transcribed, not pasted: each name is what the four fields
    /// concatenate to, and each must be a name `validate_staged_name`
    /// refuses for the stated reason.
    const HOSTILE_NEVRAS: &[(&str, &str, &str, &str)] = &[
        ("evil", "1$(id)", "1", "x86_64"),    // command substitution
        ("evil", "1", "1\nRUN id", "x86_64"), // a second instruction
        ("evil", "1", "1; id", "x86_64"),     // a second command
        ("evil", "1", "1`id`", "x86_64"),     // backtick substitution
        ("../evil", "1", "1", "x86_64"),      // traversal, leading dot
        ("-rf", "1", "1", "x86_64"),          // leading dash: an option
    ];

    #[test]
    fn dnf_cache_nevra_that_is_not_a_filename_renders_a_fixme_and_no_copy() {
        // § 8.2's dnf-cache path case, and the defect it closes: the
        // previous form rendered these into `RUN dnf localinstall -y
        // /tmp/<nevra>.rpm` with no check at all, so the first row of
        // `HOSTILE_NEVRAS` executed `id` as root at image build time.
        for (name, version, release, arch) in HOSTILE_NEVRAS {
            let snap = test_snapshot_with_repoless(vec![nevra_entry(name, version, release, arch)]);
            let lines = repoless_rpm_lines(&snap, None);
            let rendered = lines.join("\n");

            assert_eq!(
                active_lines(&lines),
                Vec::<&str>::new(),
                "{name}-{version}-{release}.{arch}: nothing may be active"
            );
            assert!(!rendered.contains("COPY repoless-packages/"), "{rendered}");
            assert!(
                lines
                    .iter()
                    .any(|l| l.starts_with("# FIXME: upload the RPM for")),
                "{rendered}"
            );
        }
    }

    /// Every file under `dir`, recursively.
    fn files_under(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(dir).expect("read_dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                found.extend(files_under(&path));
            } else {
                found.push(path);
            }
        }
        found
    }

    #[test]
    fn write_cached_rpms_refuses_a_nevra_that_is_not_a_filename_and_writes_nothing() {
        // The scan tarball's writer builds the same filename the renderer
        // refuses above, and joined it onto `repoless-packages/` as given:
        // `../evil` copied the cached RPM out of that directory, and as root
        // when the scan runs as root. A refused package is skipped and
        // reported, never silently dropped. The assertion is about the
        // filesystem, not the allowlist's verdict: the only file anywhere
        // under the root afterwards is the cached RPM the scan started with.
        for (name, version, release, arch) in HOSTILE_NEVRAS {
            let root = tempfile::tempdir().expect("tempdir");
            let cache = root.path().join("cached.rpm");
            std::fs::write(&cache, b"rpm").expect("write cached RPM");
            let render_dir = root.path().join("render");
            std::fs::create_dir(&render_dir).expect("create render dir");

            let mut pkg = nevra_entry(name, version, release, arch);
            pkg.cache_path = Some(cache.to_string_lossy().into_owned());
            let snap = test_snapshot_with_repoless(vec![pkg]);
            let warnings = write_cached_rpms(&snap, &render_dir).expect("write_cached_rpms");

            assert_eq!(
                files_under(root.path()),
                vec![cache.clone()],
                "{name}-{version}-{release}.{arch}: nothing may be written"
            );
            assert_eq!(warnings.len(), 1, "{name}: {warnings:?}");
            assert!(warnings[0].contains("not bundled"), "{name}: {warnings:?}");
        }

        // Control: an admitted NEVRA is still staged, with no warning.
        let root = tempfile::tempdir().expect("tempdir");
        let cache = root.path().join("cached.rpm");
        std::fs::write(&cache, b"rpm").expect("write cached RPM");
        let mut pkg = nevra_entry("evil", "1", "1", "x86_64");
        pkg.cache_path = Some(cache.to_string_lossy().into_owned());
        let snap = test_snapshot_with_repoless(vec![pkg]);
        let warnings = write_cached_rpms(&snap, root.path()).expect("write_cached_rpms");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(
            root.path()
                .join("repoless-packages/evil-1-1.x86_64.rpm")
                .is_file()
        );
    }

    #[test]
    fn a_dnf_cache_nevra_the_allowlist_admits_still_renders_its_install() {
        // The control the test above needs: without it, "renders a FIXME
        // and no COPY" would also pass against a renderer that emits
        // nothing for any cached RPM at all.
        let snap = test_snapshot_with_repoless(vec![nevra_entry("evil", "1", "1", "x86_64")]);
        let lines = repoless_rpm_lines(&snap, None);
        assert_eq!(
            active_lines(&lines),
            vec![
                "COPY repoless-packages/evil-1-1.x86_64.rpm /tmp/",
                "    /tmp/evil-1-1.x86_64.rpm \\",
                "    && rm /tmp/evil-1-1.x86_64.rpm \\",
            ]
        );
    }

    /// A `[r]` section, disabled, with a source: one dnf can install from
    /// with `--enablerepo=r`.
    const R_DISABLED: &str = "[r]\nbaseurl=https://example.invalid/r\nenabled=0\n";
    /// A `[r]` section dnf keeps enabled.
    const R_ENABLED: &str = "[r]\nbaseurl=https://example.invalid/r\nenabled=1\n";

    /// An included `.repo` file at `path`.
    fn repo_file(path: &str, content: &str, is_default_repo: bool) -> RepoFile {
        RepoFile {
            path: path.into(),
            content: content.into(),
            is_default_repo,
            disposition: FindingKind::included(),
            ..Default::default()
        }
    }

    /// A repo-less package the host installed from `source_repo`.
    fn from_repo(source_repo: &str, attribution: RepoAttribution) -> PackageEntry {
        PackageEntry {
            name: "tool".into(),
            version: "1".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: source_repo.into(),
            repo_attribution: attribution,
            disposition: FindingKind::included(),
            repoless_annotation: format!(
                "No repo source: repo '{source_repo}' not in enabled repos"
            ),
            ..Default::default()
        }
    }

    fn exact(id: &str) -> RepoAttribution {
        RepoAttribution::Exact {
            id: RepoId::new(id),
        }
    }

    /// A snapshot holding only `pkg` and `files`.
    fn snapshot_with(pkg: &PackageEntry, files: Vec<RepoFile>) -> InspectionSnapshot {
        InspectionSnapshot {
            rpm: Some(RpmSection {
                packages_added: vec![pkg.clone()],
                repo_files: files,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// Whether the snapshot's only package routes to its disabled repo, and
    /// whether the rendered Containerfile installs it with
    /// `--enablerepo=r`. The two are asked separately and must agree.
    fn routes_and_renders_enablerepo(snap: &InspectionSnapshot) -> bool {
        let rpm = snap.rpm.as_ref().expect("an rpm section");
        let routed = matches!(
            route_for(
                rpm,
                &InstallRepos::of(snap),
                &rpm.packages_added[0],
                &VerifiedUploads::new()
            ),
            PackageRoute::DisabledRepo(_)
        );
        let rendered = super::super::containerfile::render_containerfile(snap, None, None)
            .lines()
            .any(|l| l.starts_with("RUN dnf install") && l.contains("--enablerepo=r "));
        assert_eq!(routed, rendered, "route and Containerfile disagree");
        routed
    }

    /// Whether `pkg` routes to its disabled repo, given `files`.
    fn installs_from_disabled_repo(pkg: &PackageEntry, files: Vec<RepoFile>) -> bool {
        routes_and_renders_enablerepo(&snapshot_with(pkg, files))
    }

    /// `(file name, content)` pairs under `/etc/yum.repos.d`.
    type RepoFileNames = &'static [(&'static str, &'static str)];

    #[test]
    fn a_duplicate_repo_id_resolves_to_dnfs_definition_in_either_file_order() {
        // dnf 4 reads `.repo` files sorted by path with `.repo` cut off, and
        // the first file to define an id wins; a later one is dropped with
        // "listed more than once". Inside one file, a repeated header
        // reopens the section and later keys overwrite earlier ones. Each
        // case runs with the files in both orders: the answer is dnf's,
        // never `read_dir`'s.
        let d = "/etc/yum.repos.d";
        let cases: &[(&str, RepoFileNames, bool)] = &[
            (
                "first file disabled, second enabled",
                &[("a.repo", R_DISABLED), ("b.repo", R_ENABLED)],
                true,
            ),
            (
                "first file enabled, second disabled",
                &[("a.repo", R_ENABLED), ("b.repo", R_DISABLED)],
                false,
            ),
            (
                // `x` sorts before `x-y`, though `x-y.repo` sorts before
                // `x.repo`: dnf compares the names without `.repo`.
                "dnf compares names without .repo",
                &[("x.repo", R_DISABLED), ("x-y.repo", R_ENABLED)],
                true,
            ),
            (
                "one file, a later block disables it",
                &[(
                    "a.repo",
                    "[r]\nbaseurl=https://example.invalid/r\n[r]\nenabled=0\n",
                )],
                true,
            ),
            (
                "one file, a later block enables it",
                &[(
                    "a.repo",
                    "[r]\nbaseurl=https://example.invalid/r\nenabled=0\n[r]\nenabled=1\n",
                )],
                false,
            ),
            (
                // The final baseurl= is empty: dnf has no source to
                // install from, whatever the first block said.
                "one file, a later block clears the source and disables it",
                &[(
                    "a.repo",
                    "[r]\nbaseurl=https://example.invalid/r\nenabled=1\n[r]\nbaseurl=\nenabled=0\n",
                )],
                false,
            ),
        ];
        for (what, files, expected) in cases {
            let pkg = from_repo("r", exact("r"));
            let forward: Vec<RepoFile> = files
                .iter()
                .map(|(name, content)| repo_file(&format!("{d}/{name}"), content, false))
                .collect();
            let mut reverse = forward.clone();
            reverse.reverse();
            assert_eq!(
                installs_from_disabled_repo(&pkg, forward),
                *expected,
                "{what}, listed in order"
            );
            assert_eq!(
                installs_from_disabled_repo(&pkg, reverse),
                *expected,
                "{what}, listed reversed"
            );
        }
    }

    #[test]
    fn a_case_variant_repo_id_routes_to_its_own_spelling() {
        // dnf keys repos by exact spelling and `--enablerepo` takes one
        // verbatim: `[ab]` and `[AB]` are two repos, in one file or in two,
        // and a package from `ab` installs with `--enablerepo=ab`, never
        // `--enablerepo=AB`. Both are disabled with a source, so either
        // would route; only the id tells them apart. Each case runs with
        // the files in both orders.
        const LOWER: &str = "[ab]\nbaseurl=https://example.invalid/lower\nenabled=0\n";
        const UPPER: &str = "[AB]\nbaseurl=https://example.invalid/upper\nenabled=0\n";
        let d = "/etc/yum.repos.d";
        let same_file = format!("{LOWER}{UPPER}");
        let layouts: [(&str, Vec<(String, &str)>); 2] = [
            (
                "same file",
                vec![(format!("{d}/a.repo"), same_file.as_str())],
            ),
            (
                // `A.repo` sorts first, so a folded match would take `[AB]`.
                "cross file",
                vec![
                    (format!("{d}/A.repo"), UPPER),
                    (format!("{d}/b.repo"), LOWER),
                ],
            ),
        ];
        for (layout, files) in &layouts {
            for wanted in ["ab", "AB"] {
                let forward: Vec<RepoFile> = files
                    .iter()
                    .map(|(path, content)| repo_file(path, content, false))
                    .collect();
                let mut reverse = forward.clone();
                reverse.reverse();
                for (order, files) in [("in order", forward), ("reversed", reverse)] {
                    let snap = snapshot_with(&from_repo(wanted, exact(wanted)), files);
                    let rpm = snap.rpm.as_ref().expect("an rpm section");
                    let what = format!("{layout}, {wanted}, {order}");
                    match route_for(
                        rpm,
                        &InstallRepos::of(&snap),
                        &rpm.packages_added[0],
                        &VerifiedUploads::new(),
                    ) {
                        PackageRoute::DisabledRepo(id) => {
                            assert_eq!(id.as_str(), wanted, "{what}: routed id")
                        }
                        other => panic!("{what}: {other:?}"),
                    }
                    let flags: Vec<String> =
                        super::super::containerfile::render_containerfile(&snap, None, None)
                            .split_whitespace()
                            .filter(|w| w.starts_with("--enablerepo="))
                            .map(str::to_string)
                            .collect();
                    assert_eq!(flags, vec![format!("--enablerepo={wanted}")], "{what}");
                }
            }
        }
    }

    #[test]
    fn packages_from_case_variant_repos_install_together_with_both_flags() {
        // Routing gives each package its own spelling; the combined
        // install must keep them apart too. Grouped by a folded id, a
        // package from `ab` and one from `AB` shared one bucket and the
        // transaction enabled only one of the two repos, so whichever
        // package lives only in the other one could not be found. Each
        // case runs in both package orders and both file orders.
        const LOWER: &str = "[ab]\nbaseurl=https://example.invalid/lower\nenabled=0\n";
        const UPPER: &str = "[AB]\nbaseurl=https://example.invalid/upper\nenabled=0\n";
        let d = "/etc/yum.repos.d";
        let same_file = format!("{LOWER}{UPPER}");
        let layouts: [(&str, Vec<(String, &str)>); 2] = [
            (
                "same file",
                vec![(format!("{d}/a.repo"), same_file.as_str())],
            ),
            (
                "cross file",
                vec![
                    (format!("{d}/A.repo"), UPPER),
                    (format!("{d}/b.repo"), LOWER),
                ],
            ),
        ];
        let package = |name: &str, repo: &str| PackageEntry {
            name: name.into(),
            ..from_repo(repo, exact(repo))
        };
        let packages = vec![package("tool-lower", "ab"), package("tool-upper", "AB")];
        for (layout, files) in &layouts {
            let files: Vec<RepoFile> = files
                .iter()
                .map(|(path, content)| repo_file(path, content, false))
                .collect();
            for reverse_packages in [false, true] {
                for reverse_files in [false, true] {
                    let mut snap = snapshot_with(&packages[0], files.clone());
                    let rpm = snap.rpm.as_mut().expect("an rpm section");
                    rpm.packages_added = packages.clone();
                    if reverse_packages {
                        rpm.packages_added.reverse();
                    }
                    if reverse_files {
                        rpm.repo_files.reverse();
                    }
                    let what = format!(
                        "{layout}, packages reversed {reverse_packages}, \
                         files reversed {reverse_files}"
                    );
                    let output =
                        super::super::containerfile::render_containerfile(&snap, None, None);
                    let mut flags: Vec<&str> = output
                        .split_whitespace()
                        .filter(|w| w.starts_with("--enablerepo="))
                        .collect();
                    flags.sort();
                    assert_eq!(
                        flags,
                        ["--enablerepo=AB", "--enablerepo=ab"],
                        "{what}:\n{output}"
                    );
                    let operands: Vec<&str> = output
                        .lines()
                        .map(|l| l.trim_end_matches(" \\").trim())
                        .filter(|l| l.starts_with("tool-"))
                        .collect();
                    assert_eq!(operands, ["tool-lower", "tool-upper"], "{what}:\n{output}");
                }
            }
        }
    }

    #[test]
    fn a_default_repo_file_or_a_pseudo_repo_never_routes_to_a_disabled_repo() {
        // The Custom Repositories `COPY` carries the whole staged directory
        // before the install, default repo files included, and renders only
        // when a custom file is staged; so an included default file is one
        // dnf reads when a custom file brings the `COPY`, and none is read
        // when no custom file does. An excluded default file is not staged.
        // And a pseudo repo is not a repo, whatever a `.repo` file calls a
        // section: the scan's typed attribution says so, not a spelling.
        let mut excluded_default = repo_file("/etc/yum.repos.d/a-default.repo", R_ENABLED, true);
        excluded_default.disposition = FindingKind::excluded();
        let cases: &[(&str, PackageEntry, Vec<RepoFile>, bool)] = &[
            (
                "control: a custom file",
                from_repo("r", exact("r")),
                vec![repo_file("/etc/yum.repos.d/custom.repo", R_DISABLED, false)],
                true,
            ),
            (
                // No custom file is staged, so the `COPY` that would put it
                // in the image before the install does not render.
                "only a default file is staged",
                from_repo("r", exact("r")),
                vec![repo_file("/etc/yum.repos.d/redhat.repo", R_DISABLED, true)],
                false,
            ),
            (
                "a default file defines it, and a custom file brings the COPY",
                from_repo("r", exact("r")),
                vec![
                    repo_file("/etc/yum.repos.d/redhat.repo", R_DISABLED, true),
                    repo_file(
                        "/etc/yum.repos.d/z-custom.repo",
                        "[other]\nbaseurl=https://example.invalid/o\n",
                        false,
                    ),
                ],
                true,
            ),
            (
                // dnf reads redhat.repo first and keeps its definition: the
                // image gets this copy of it too.
                "an included default file sorts first and enables it",
                from_repo("r", exact("r")),
                vec![
                    repo_file("/etc/yum.repos.d/redhat.repo", R_ENABLED, true),
                    repo_file("/etc/yum.repos.d/z-custom.repo", R_DISABLED, false),
                ],
                false,
            ),
            (
                "an excluded default file sorts first and enables it",
                from_repo("r", exact("r")),
                vec![
                    excluded_default,
                    repo_file("/etc/yum.repos.d/z-custom.repo", R_DISABLED, false),
                ],
                true,
            ),
            (
                "anaconda, attributed as a pseudo repo",
                from_repo(
                    "anaconda",
                    RepoAttribution::Pseudo {
                        which: PseudoRepo::Anaconda,
                        recorded: "anaconda".into(),
                    },
                ),
                vec![repo_file(
                    "/etc/yum.repos.d/custom.repo",
                    "[anaconda]\nbaseurl=https://example.invalid/a\nenabled=0\n",
                    false,
                )],
                false,
            ),
        ];
        for (what, pkg, files, expected) in cases {
            assert_eq!(
                installs_from_disabled_repo(pkg, files.clone()),
                *expected,
                "{what}"
            );
        }
    }

    /// An included config entry at `path`.
    fn config_entry(path: &str, content: &str) -> inspectah_core::types::config::ConfigFileEntry {
        inspectah_core::types::config::ConfigFileEntry {
            path: path.into(),
            content: content.into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }
    }

    #[test]
    fn the_route_reads_the_repo_files_the_export_stages() {
        // The route judges the files the build has when it installs: what
        // the stager writes under `/etc/yum.repos.d`, with the content it
        // writes. An included repo file the stager withholds or refuses is
        // not one of them. A config entry at the repo file's own path has
        // no vote: the repo ownership pass makes it inventory, so dnf reads
        // the repo file's content whatever the config entry holds.
        const CUSTOM: &str = "/etc/yum.repos.d/custom.repo";
        let other = || {
            repo_file(
                "/etc/yum.repos.d/other.repo",
                "[other]\nbaseurl=https://example.invalid/o\n",
                false,
            )
        };
        // `(what, repo files, config entries as (path, content), routes)`.
        type Case = (
            &'static str,
            Vec<RepoFile>,
            Vec<(&'static str, &'static str)>,
            bool,
        );
        let cases: Vec<Case> = vec![
            (
                "control: the repo file is staged",
                vec![repo_file(CUSTOM, R_DISABLED, false), other()],
                vec![],
                true,
            ),
            (
                // A config entry under `custom.repo/` needs it as a
                // directory: the stager withholds both as a conflict.
                "the repo file is withheld",
                vec![repo_file(CUSTOM, R_DISABLED, false), other()],
                vec![("/etc/yum.repos.d/custom.repo/stray", "x\n")],
                false,
            ),
            (
                "the repo file's path is refused",
                vec![
                    repo_file("/etc/yum.repos.d/cus\ttom.repo", R_DISABLED, false),
                    other(),
                ],
                vec![],
                false,
            ),
            (
                "a config twin that enables the repo does not change the file",
                vec![repo_file(CUSTOM, R_DISABLED, false)],
                vec![(CUSTOM, R_ENABLED)],
                true,
            ),
            (
                "a config twin that disables the repo does not change the file",
                vec![repo_file(CUSTOM, R_ENABLED, false)],
                vec![(CUSTOM, R_DISABLED)],
                false,
            ),
        ];
        for (what, files, config, expected) in cases {
            let pkg = from_repo("r", exact("r"));
            let mut snap = snapshot_with(&pkg, files);
            snap.config = Some(inspectah_core::types::config::ConfigSection {
                withheld: Vec::new(),
                files: config
                    .into_iter()
                    .map(|(path, content)| config_entry(path, content))
                    .collect(),
            });
            inspectah_core::repo_ownership::settle_repo_ownership(&mut snap);
            assert_eq!(routes_and_renders_enablerepo(&snap), expected, "{what}");
        }
    }

    #[test]
    fn a_key_the_export_does_not_copy_keeps_the_package_repo_less() {
        // dnf verifies a package against the keys its section names, so the
        // route needs each one in the image when the package phase
        // installs: copied by the GPG Keys section, which copies only what
        // the stager writes, or shipped by the base image. An included key
        // the stager withholds (a config entry under `KEY-r/` needs that
        // path as a directory) is not copied, and dnf fails on it.
        const KEY: &str = "/etc/pki/rpm-gpg/KEY-r";
        const KEYED: &str = "[r]\nbaseurl=https://example.invalid/r\nenabled=0\n\
                             gpgkey=file:///etc/pki/rpm-gpg/KEY-r\n";
        let key = |shipped_by_base: Option<&str>| RepoFile {
            path: KEY.into(),
            content: "key-data".into(),
            disposition: FindingKind::included(),
            shipped_by_base: shipped_by_base.map(String::from),
            ..Default::default()
        };
        // `(what, key, config entries as (path, content), routes)`.
        type Case = (
            &'static str,
            RepoFile,
            Vec<(&'static str, &'static str)>,
            bool,
        );
        let cases: Vec<Case> = vec![
            ("control: the key is copied", key(None), vec![], true),
            (
                "the key is withheld",
                key(None),
                vec![("/etc/pki/rpm-gpg/KEY-r/stray", "x\n")],
                false,
            ),
            (
                "the key is withheld, but the base image ships it",
                key(Some("vendor-keys")),
                vec![("/etc/pki/rpm-gpg/KEY-r/stray", "x\n")],
                true,
            ),
        ];
        for (what, key, config, expected) in cases {
            let pkg = from_repo("r", exact("r"));
            let mut snap = snapshot_with(
                &pkg,
                vec![repo_file("/etc/yum.repos.d/custom.repo", KEYED, false)],
            );
            snap.rpm.as_mut().expect("an rpm section").gpg_keys = vec![key];
            snap.config = Some(inspectah_core::types::config::ConfigSection {
                withheld: Vec::new(),
                files: config
                    .into_iter()
                    .map(|(path, content)| config_entry(path, content))
                    .collect(),
            });
            inspectah_core::repo_ownership::settle_repo_ownership(&mut snap);
            assert_eq!(routes_and_renders_enablerepo(&snap), expected, "{what}");
            let rpm = snap.rpm.as_ref().expect("an rpm section");
            let uploads = VerifiedUploads::new();
            let route = route_for(
                rpm,
                &InstallRepos::of(&snap),
                &rpm.packages_added[0],
                &uploads,
            );
            assert_eq!(
                matches!(route, PackageRoute::Repoless),
                !expected,
                "{what}: {route:?}"
            );
        }
    }

    #[test]
    fn an_anaconda_package_never_routes_to_a_section_named_anaconda() {
        // The scan's own path, not a hand-built attribution: normalization
        // classifies the recorded `anaconda`, repo-less detection flags the
        // package, and the route reads what the two left behind. A disabled
        // `[anaconda]` section with a source is exactly what the route
        // would otherwise accept.
        use inspectah_collect::executor::mock::MockExecutor;
        use inspectah_collect::inspectors::rpm::repoless::scan_dnf_cache_for_repoless;
        use inspectah_collect::inspectors::rpm::source_repos::normalize_source_repos;
        use inspectah_core::traits::executor::ExecResult;

        let ok = |stdout: &str| ExecResult {
            stdout: stdout.into(),
            exit_code: 0,
            ..Default::default()
        };
        let exec = MockExecutor::new()
            .with_command(
                "dnf repolist --enabled -q",
                ok("repo id  repo name\nbaseos  BaseOS\n"),
            )
            .with_command(
                "dnf repoquery --available --queryformat %{name} %{reponame}\n -q",
                ok("bash baseos\n"),
            )
            .with_command("find /var/cache/dnf -name *.rpm -type f", ok(""));

        let mut pkg = from_repo("anaconda", RepoAttribution::Unrecorded);
        pkg.repoless_annotation.clear();
        let mut rpm = RpmSection {
            packages_added: vec![pkg],
            repo_files: vec![repo_file(
                "/etc/yum.repos.d/custom.repo",
                "[anaconda]\nbaseurl=https://example.invalid/a\nenabled=0\n",
                false,
            )],
            ..Default::default()
        };
        normalize_source_repos(&mut rpm.packages_added, &rpm.repo_files);
        scan_dnf_cache_for_repoless(&exec, &mut rpm);

        let pkg = &rpm.packages_added[0];
        assert!(
            !pkg.repoless_annotation.is_empty(),
            "the scan leaves the package repo-less"
        );
        let snap = InspectionSnapshot {
            rpm: Some(rpm.clone()),
            ..Default::default()
        };
        assert_eq!(
            route_for(&rpm, &InstallRepos::of(&snap), pkg, &VerifiedUploads::new()),
            PackageRoute::Repoless
        );
    }

    #[test]
    fn an_uploaded_package_with_no_verified_upload_is_demoted() {
        // `uploaded` and `cache_path` are `#[serde(default)]` and arrive
        // from the scanned host. With no verified map this entry claims an
        // upload nothing proved, and `set_uploaded` would have left
        // `repoless_cached` true as well -- so the demotion has to be
        // decided before the cached branch, or this renders the NEVRA
        // `COPY` on a snapshot's say-so.
        let mut pkg = nevra_entry("epel-release", "9", "11.el9", "noarch");
        pkg.uploaded = true;
        pkg.cache_path = Some("/etc/shadow".into());
        let snap = test_snapshot_with_repoless(vec![pkg]);
        let lines = repoless_rpm_lines(&snap, None);

        assert_eq!(active_lines(&lines), Vec::<&str>::new());
        assert!(lines.iter().any(|l| l
            == "# FIXME: upload the RPM for epel-release.noarch again in refine; \
                the staged file could not be verified for export"));
    }

    /// The route-loss decision over the class of inputs that decide it,
    /// each case judged for a package installed by name from `site` with
    /// the scan's attribution. `lost` is what the build can do: install it
    /// from a repo the image will have, or not.
    #[test]
    fn the_route_is_lost_only_when_the_build_cannot_install_from_a_repo_it_has() {
        use inspectah_core::types::rpm::{FileOwnershipEntry, RpmVaEntry};
        const KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-site";
        let site = |extra: &str| {
            format!(
                "[site]\nbaseurl=https://site.example/\nenabled=1\ngpgkey=file://{KEY}\n{extra}"
            )
        };
        let file = |name: &str, content: &str, include: bool| RepoFile {
            path: format!("/etc/yum.repos.d/{name}"),
            content: content.into(),
            disposition: FindingKind::from_bool(include),
            ..Default::default()
        };
        let key = |include: bool| RepoFile {
            path: KEY.into(),
            content: "-----BEGIN PGP PUBLIC KEY BLOCK-----\n".into(),
            disposition: FindingKind::from_bool(include),
            ..Default::default()
        };
        /// What the base image and `rpm -Va` say about the repo files.
        #[derive(Clone, Copy)]
        enum Base {
            /// No baseline: nothing is known about the base image.
            Unknown,
            /// `site-repos` owns `site.repo` and is in the base image.
            Owns,
            /// As `Owns`, and `rpm -Va` reports `site.repo` modified.
            OwnsModified,
        }
        struct Case {
            name: &'static str,
            files: Vec<RepoFile>,
            key: Option<RepoFile>,
            base: Base,
            source_repo: &'static str,
            lost: bool,
        }
        let case = |name, files, key, base, lost| Case {
            name,
            files,
            key,
            base,
            source_repo: "site",
            lost,
        };
        let cases = vec![
            case(
                "carried",
                vec![file("site.repo", &site(""), true)],
                Some(key(true)),
                Base::Unknown,
                false,
            ),
            case(
                "file dropped",
                vec![file("site.repo", &site(""), false)],
                Some(key(true)),
                Base::Unknown,
                true,
            ),
            case(
                "file dropped beside another carried file",
                vec![
                    file("site.repo", &site(""), false),
                    file("other.repo", "[other]\nbaseurl=https://o.example/\n", true),
                ],
                Some(key(true)),
                Base::Unknown,
                true,
            ),
            case(
                "another section keeps the file",
                vec![file(
                    "site.repo",
                    &format!("{}[other]\nbaseurl=https://o.example/\n", site("")),
                    true,
                )],
                Some(key(true)),
                Base::Unknown,
                false,
            ),
            case(
                "host disabled it",
                vec![file("site.repo", &site("enabled=0\n"), false)],
                Some(key(true)),
                Base::Unknown,
                false,
            ),
            case(
                "key dropped, gpgcheck unset",
                vec![file("site.repo", &site(""), true)],
                Some(key(false)),
                Base::Unknown,
                true,
            ),
            case(
                "key never captured, gpgcheck=1",
                vec![file("site.repo", &site("gpgcheck=1\n"), true)],
                None,
                Base::Unknown,
                true,
            ),
            case(
                "key never captured, gpgcheck=0 and repo_gpgcheck=0",
                vec![file(
                    "site.repo",
                    &site("gpgcheck=0\nrepo_gpgcheck=0\n"),
                    true,
                )],
                None,
                Base::Unknown,
                false,
            ),
            case(
                "key never captured, dnf 5's pkg_gpgcheck=0",
                vec![file("site.repo", &site("pkg_gpgcheck=0\n"), true)],
                None,
                Base::Unknown,
                false,
            ),
            case(
                "key never captured, gpgcheck=0 but repo_gpgcheck=1",
                vec![file(
                    "site.repo",
                    &site("gpgcheck=0\nrepo_gpgcheck=1\n"),
                    true,
                )],
                None,
                Base::Unknown,
                true,
            ),
            case(
                "dropped, a base image package owns it unmodified",
                vec![file("site.repo", &site(""), false)],
                Some(key(true)),
                Base::Owns,
                false,
            ),
            case(
                "dropped, a base image package owns it, modified on the host",
                vec![file("site.repo", &site(""), false)],
                Some(key(true)),
                Base::OwnsModified,
                true,
            ),
            Case {
                source_repo: "baseos",
                ..case(
                    "dropped, a distro name in a custom file",
                    vec![file(
                        "site.repo",
                        &site("").replace("[site]", "[baseos]"),
                        false,
                    )],
                    Some(key(true)),
                    Base::Unknown,
                    true,
                )
            },
            case(
                "a default repo file defines it",
                vec![RepoFile {
                    is_default_repo: true,
                    ..file("redhat.repo", &site(""), false)
                }],
                Some(key(true)),
                Base::Unknown,
                false,
            ),
            case(
                "no file the snapshot holds defines the attributed section",
                vec![file(
                    "site.repo",
                    "[other]\nbaseurl=https://o.example/\n",
                    true,
                )],
                Some(key(true)),
                Base::Unknown,
                true,
            ),
        ];
        for c in cases {
            let mut pkg = from_repo(c.source_repo, exact(c.source_repo));
            pkg.repoless_annotation.clear();
            let mut snap = snapshot_with(&pkg, c.files);
            let rpm = snap.rpm.as_mut().unwrap();
            rpm.gpg_keys.extend(c.key);
            if !matches!(c.base, Base::Unknown) {
                rpm.baseline_package_names = Some(vec!["site-repos".into()]);
                rpm.file_ownership = vec![FileOwnershipEntry {
                    package_name: "site-repos".into(),
                    paths: vec!["/etc/yum.repos.d/site.repo".into()],
                }];
            }
            if matches!(c.base, Base::OwnsModified) {
                rpm.rpm_va = vec![RpmVaEntry {
                    path: "/etc/yum.repos.d/site.repo".into(),
                    ..Default::default()
                }];
            }
            let repos = InstallRepos::of(&snap);
            let rpm = snap.rpm.as_ref().unwrap();
            let none = VerifiedUploads::new();
            let route = route_for(rpm, &repos, &rpm.packages_added[0], &none);
            assert_eq!(
                matches!(route, PackageRoute::LostRepo(_)),
                c.lost,
                "{} ({}): {route:?}",
                c.name,
                c.source_repo
            );
            if !c.lost {
                assert_eq!(route, PackageRoute::ByName, "{}", c.name);
            }
        }
    }

    /// `[site]` and `[SITE]` in separate files are two repos: only the one
    /// that lost its key loses its route, in either file order, and each
    /// package resolves to its own spelling.
    #[test]
    fn a_case_variant_section_keeps_its_own_route() {
        const KEY: &str = "/etc/pki/rpm-gpg/missing";
        let lower = format!("[site]\nbaseurl=https://l.example/\ngpgkey=file://{KEY}\n");
        let upper = "[SITE]\nbaseurl=https://u.example/\ngpgkey=https://u.example/KEY\n";
        for (first, second) in [("a.repo", "b.repo"), ("b.repo", "a.repo")] {
            for (lost_in, kept_in) in [(first, second), (second, first)] {
                let files = vec![
                    repo_file(&format!("/etc/yum.repos.d/{lost_in}"), &lower, false),
                    repo_file(&format!("/etc/yum.repos.d/{kept_in}"), upper, false),
                ];
                let mut pkg_lower = from_repo("site", exact("site"));
                pkg_lower.repoless_annotation.clear();
                let mut pkg_upper = from_repo("SITE", exact("SITE"));
                pkg_upper.repoless_annotation.clear();
                pkg_upper.name = "upper-tool".into();
                let mut snap = snapshot_with(&pkg_lower, files);
                snap.rpm.as_mut().unwrap().packages_added.push(pkg_upper);
                let repos = InstallRepos::of(&snap);
                let rpm = snap.rpm.as_ref().unwrap();
                let none = VerifiedUploads::new();
                assert_eq!(
                    route_for(rpm, &repos, &rpm.packages_added[0], &none),
                    PackageRoute::LostRepo(SectionId::new("site")),
                    "{lost_in} then {kept_in}"
                );
                assert_eq!(
                    route_for(rpm, &repos, &rpm.packages_added[1], &none),
                    PackageRoute::ByName,
                    "{lost_in} then {kept_in}"
                );
            }
        }
    }
}
