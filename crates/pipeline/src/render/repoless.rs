use inspectah_core::rpm_filename::StagedName;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::group_render::RenderContext;
use inspectah_core::types::rpm::PackageEntry;
use inspectah_core::types::verified_upload::{VerifiedUpload, VerifiedUploads};

use super::language_packages::sanitize_for_comment;

/// Whether an install block is a build instruction or advice.
///
/// A commented block is inert text; an active one runs `dnf localinstall`
/// as root at image build time. Both forms interpolate the same operand,
/// so both take it from the same place.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Emission {
    Active,
    Commented,
}

/// The `COPY`/`RUN` pair that installs one RPM from `repoless-packages/`.
///
/// This is the only function in this module that writes an RPM filename
/// into a `COPY` or a `RUN`, and it takes a [`StagedName`] rather than a
/// `&str`. That is deliberate and it is the fix for a real defect: the
/// previous form built `RUN dnf localinstall -y /tmp/{name}-{version}-\
/// {release}.{arch}.rpm` straight from four snapshot fields, so a scanned
/// host reporting a package named `x;c6_exec` got that string executed as
/// root at image build time. A name cannot reach this function without
/// having passed the allowlist, because `StagedName` has no other
/// constructor -- so the guarantee is the type's, not a check a new call
/// site can forget.
fn install_lines(name: &StagedName, emission: Emission) -> Vec<String> {
    let rpm = name.as_str();
    let prefix = match emission {
        Emission::Active => "",
        Emission::Commented => "# ",
    };
    vec![
        format!("{prefix}COPY repoless-packages/{rpm} /tmp/"),
        format!("{prefix}RUN dnf localinstall -y /tmp/{rpm} \\"),
        match emission {
            Emission::Active => format!("    && rm /tmp/{rpm}"),
            Emission::Commented => format!("#     && rm /tmp/{rpm}"),
        },
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

/// Render Containerfile lines for repo-less RPM packages.
///
/// Four outcomes per package, in the order they are decided:
///
/// 1. `render_ctx` holds a [`VerifiedUpload`] for it: an active install
///    block naming the basename the export proved and will write.
/// 2. It claims an upload (`uploaded`) and is included, but no verified
///    upload reached here: the demotion block. No `COPY`, a FIXME, and the
///    export response carries the matching warning. An entry the snapshot
///    arrived with is `uploaded: true` without anything having been staged,
///    so this is the ordinary path for an untrusted snapshot, not an
///    exception.
/// 3. `repoless_cached`: the host's dnf cache bundled the RPM at scan time,
///    so the filename is its NEVRA. Active when included, commented when
///    pre-excluded, and a FIXME instead when that NEVRA is not a name the
///    allowlist admits.
/// 4. Neither: the MANUAL comment block.
pub fn repoless_rpm_lines(
    snap: &InspectionSnapshot,
    render_ctx: Option<&RenderContext>,
) -> Vec<String> {
    let rpm_section = match &snap.rpm {
        Some(r) => r,
        None => return Vec::new(),
    };

    let repoless: Vec<_> = rpm_section
        .packages_added
        .iter()
        .filter(|p| !p.repoless_annotation.is_empty())
        .collect();

    if repoless.is_empty() {
        return Vec::new();
    }

    let empty = VerifiedUploads::new();
    let verified = render_ctx.map_or(&empty, |c| &c.verified_uploads);

    let mut lines = Vec::new();
    lines.push(String::new());
    lines.push("# === Repo-less RPM packages ===".into());

    for pkg in &repoless {
        let canonical = format!("{}.{}", pkg.name, pkg.arch);
        if let Some(upload) = verified.get(&canonical) {
            lines.extend(uploaded_lines(pkg, upload));
        } else if pkg.uploaded && pkg.disposition.is_included() {
            lines.push(format!(
                "# FIXME: upload the RPM for {} again in refine; the staged file \
                 could not be verified for export",
                comment_id(pkg)
            ));
        } else if pkg.repoless_cached {
            lines.extend(cached_lines(pkg));
        } else {
            // No cached RPM — manual resolution
            lines.push(format!(
                "# MANUAL: {} (no repo source, RPM not in cache)",
                sanitize_for_comment(&pkg.name)
            ));
            lines.push(
                "# Provide the RPM via the refine UI upload, add a repo, or uncomment:".into(),
            );
            // Sanitized for the same reason the line above is: a newline in
            // a scanned package name would end the comment and turn the
            // rest of the name into an active instruction.
            lines.push(format!(
                "# RUN dnf install {}",
                sanitize_for_comment(&pkg.name)
            ));
        }
    }

    lines
}

/// The block for a package whose staged upload the export proved.
fn uploaded_lines(pkg: &PackageEntry, upload: &VerifiedUpload) -> Vec<String> {
    let mut lines = provenance_lines(pkg, "uploaded RPM");
    lines.push("# It was uploaded in refine. Updates must be managed manually.".into());
    if let Some(uploaded_version) = &upload.uploaded_version {
        // `uploaded_version` is re-derived from the proved basename, so it
        // needs no sanitizing; the other three operands come from the
        // snapshot and do.
        lines.push(format!(
            "# uploaded {}-{uploaded_version}.{}; the host had {}",
            sanitize_for_comment(&pkg.name),
            sanitize_for_comment(&pkg.arch),
            sanitize_for_comment(&format!("{}-{}", pkg.version, pkg.release)),
        ));
    }
    lines.extend(install_lines(&upload.basename, Emission::Active));
    lines
}

/// The block for a package the host's dnf cache supplied at scan time.
fn cached_lines(pkg: &PackageEntry) -> Vec<String> {
    let nevra_filename = format!(
        "{}-{}-{}.{}.rpm",
        pkg.name, pkg.version, pkg.release, pkg.arch
    );
    let Ok(name) = StagedName::parse(&nevra_filename) else {
        // Rejection is rejection: there is no sanitized form of this
        // filename that would name the right file, so the block is advice
        // rather than a `COPY` of a name nothing proved.
        return vec![format!(
            "# FIXME: upload the RPM for {} in refine; the cached RPM's filename \
             is not one this can name in a COPY",
            comment_id(pkg)
        )];
    };

    let mut lines = provenance_lines(pkg, "cached RPM");
    if pkg.disposition.is_included() {
        lines.push(
            "# It was found in the local dnf cache. Updates must be managed manually.".into(),
        );
        lines.extend(install_lines(&name, Emission::Active));
    } else {
        lines.push("# Pre-excluded — uncomment after verifying provenance:".into());
        lines.extend(install_lines(&name, Emission::Commented));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::FindingKind;
    use inspectah_core::types::rpm::{PackageEntry, RpmSection};

    fn test_snapshot_with_repoless(packages: Vec<PackageEntry>) -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::default();
        snap.rpm = Some(RpmSection {
            packages_added: packages,
            ..Default::default()
        });
        snap
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
        assert!(lines.iter().any(|l| l.starts_with("RUN dnf localinstall")));
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
        assert!(lines.is_empty());
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

    /// The lines a build actually runs: everything that is not a comment.
    fn active_lines(lines: &[String]) -> Vec<&str> {
        lines
            .iter()
            .map(|l| l.as_str())
            .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
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

    #[test]
    fn dnf_cache_nevra_that_is_not_a_filename_renders_a_fixme_and_no_copy() {
        // § 8.2's dnf-cache path case, and the defect it closes: these
        // four fields come from a scanned host, and the previous form
        // rendered them into `RUN dnf localinstall -y /tmp/<nevra>.rpm`
        // with no check at all, so the first row below executed `id` as
        // root at image build time.
        //
        // Hand-transcribed, not pasted: each name is what the four fields
        // concatenate to, and each must be a name `validate_staged_name`
        // refuses for the stated reason.
        let hostile: &[(&str, &str, &str, &str)] = &[
            ("evil", "1$(id)", "1", "x86_64"),    // command substitution
            ("evil", "1", "1\nRUN id", "x86_64"), // a second instruction
            ("evil", "1", "1; id", "x86_64"),     // a second command
            ("evil", "1", "1`id`", "x86_64"),     // backtick substitution
            ("../evil", "1", "1", "x86_64"),      // traversal, leading dot
            ("-rf", "1", "1", "x86_64"),          // leading dash: an option
        ];
        for (name, version, release, arch) in hostile {
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
                "RUN dnf localinstall -y /tmp/evil-1-1.x86_64.rpm \\",
                "    && rm /tmp/evil-1-1.x86_64.rpm",
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
