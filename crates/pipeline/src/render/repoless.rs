use inspectah_core::rpm_filename::StagedName;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::group_render::RenderContext;
use inspectah_core::types::repo::{RepoId, parse_repo_sections};
use inspectah_core::types::rpm::{PackageEntry, RpmSection};
use inspectah_core::types::verified_upload::{VerifiedUpload, VerifiedUploads};

use super::language_packages::sanitize_for_comment;
use super::repo_enabling::keys_included;
use super::safety::sanitize_shell_value;
use super::service_intent::is_package_installable;

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
/// In order: no repo-less annotation is [`PackageRoute::ByName`]; a
/// verified upload is [`PackageRoute::Upload`]; a disabled repo the export
/// can install from ([`disabled_repo_install`]) is
/// [`PackageRoute::DisabledRepo`]; anything else is
/// [`PackageRoute::Repoless`]. A cached RPM is not an operator decision, so
/// it does not outrank the disabled repo. A path with no uploads, the scan
/// tarball or refine before any upload, passes an empty map.
pub fn route_for<'a>(
    rpm: &RpmSection,
    pkg: &PackageEntry,
    verified: &'a VerifiedUploads,
) -> PackageRoute<'a> {
    if pkg.repoless_annotation.is_empty() {
        return PackageRoute::ByName;
    }
    if let Some(upload) = verified.get(&format!("{}.{}", pkg.name, pkg.arch)) {
        return PackageRoute::Upload(upload);
    }
    match disabled_repo_install(rpm, pkg) {
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
    rpm: &RpmSection,
    render_dir: &std::path::Path,
) -> std::io::Result<Vec<String>> {
    let mut refused = Vec::new();
    let no_uploads = VerifiedUploads::new();
    let dest_dir = render_dir.join("repoless-packages");
    for pkg in &rpm.packages_added {
        let Some(cache_path) = &pkg.cache_path else {
            continue;
        };
        if route_for(rpm, pkg, &no_uploads) != PackageRoute::Repoless {
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
/// `source_repo` is not another `@` pseudo-repo, an included repo file has
/// a section with that id that is disabled and defines a source, every
/// `gpgkey=file://` key that section names is an included key (dnf cannot
/// verify the package without it), and the id and the package's name and arch pass `sanitize_shell_value`. The id
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
fn disabled_repo_install(rpm: &RpmSection, pkg: &PackageEntry) -> Option<RepoId> {
    if !is_package_installable(pkg)
        || pkg.repoless_annotation.is_empty()
        || pkg.source_repo.is_empty()
        || pkg.source_repo.starts_with('@')
        || sanitize_shell_value(&pkg.name).is_none()
        || sanitize_shell_value(&pkg.arch).is_none()
    {
        return None;
    }
    rpm.repo_files
        .iter()
        .filter(|f| f.disposition.is_included())
        .flat_map(|f| parse_repo_sections(&f.content))
        .find(|sec| sec.id.as_str().eq_ignore_ascii_case(&pkg.source_repo))
        .filter(|sec| {
            !sec.enabled
                && sec.has_source
                && keys_included(rpm, &sec.gpg_key_paths)
                && sanitize_shell_value(sec.id.as_str()).is_some()
        })
        .map(|sec| sec.id)
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
/// 4. Neither: the MANUAL comment block.
pub(crate) fn repoless_plan(
    snap: &InspectionSnapshot,
    render_ctx: Option<&RenderContext>,
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

    // A package that installs by name, or from its host-disabled repo,
    // renders in the packages section instead, so it is not offered here.
    for pkg in &rpm_section.packages_added {
        match route_for(rpm_section, pkg, verified) {
            PackageRoute::Upload(upload) => plan.local.push(uploaded_rpm(pkg, upload)),
            PackageRoute::Repoless => plan_repoless(pkg, &mut plan),
            PackageRoute::ByName | PackageRoute::DisabledRepo(_) => {}
        }
    }
    plan
}

/// Outcomes 2 to 4 of [`repoless_plan`] for one package.
fn plan_repoless(pkg: &PackageEntry, plan: &mut RepolessPlan) {
    if pkg.uploaded && pkg.disposition.is_included() {
        plan.advice.push(format!(
            "# FIXME: upload the RPM for {} again in refine; the staged file \
             could not be verified for export",
            comment_id(pkg)
        ));
    } else if pkg.repoless_cached {
        plan_cached(pkg, plan);
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
    use inspectah_core::types::rpm::{PackageEntry, RpmSection};

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
            let rpm = RpmSection {
                packages_added: vec![pkg],
                ..Default::default()
            };
            let warnings = write_cached_rpms(&rpm, &render_dir).expect("write_cached_rpms");

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
        let rpm = RpmSection {
            packages_added: vec![pkg],
            ..Default::default()
        };
        let warnings = write_cached_rpms(&rpm, root.path()).expect("write_cached_rpms");
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
}
