//! Containerfile renderer — produces a Containerfile from an InspectionSnapshot.
//!
//! Section order:
//! 1. FROM + repos + GPG + modules + packages (dnf install -y)
//! 2. Services (enable/disable)
//! 3. Firewall zones
//! 4. Scheduled tasks (timer COPYs)
//! 5. Config files (COPY per top-level dir)
//! 6. Non-RPM software
//! 7. Containers (quadlet COPYs)
//! 8. Users
//! 9. Kernel/boot (kargs.d, sysctl, tuned activation)
//! 10. Security & Access Control (SELinux, FIPS, PAM, audit)
//! 11. Network (routes, hosts, proxy)
//! 12. Secrets comments
//!     12b. /var directory provisioning (tmpfiles.d + mkdir fallback)
//! 13. Epilogue (tmpfiles, RUN bootc container lint)

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::completeness::{Completeness, InspectorId};
use inspectah_core::types::config::SYNTHESIZED_SYSCTL_FILE;
use inspectah_core::types::group_render::RenderContext;
use inspectah_core::types::os::SystemType;
use inspectah_core::types::redaction::RedactionKind;
use inspectah_core::types::repo::{RepoId, parse_repo_sections};
use inspectah_core::types::rpm::{PackageEntry, RpmSection};

use inspectah_core::types::group_render::{DegradationReason, GroupRenderState};

use super::configtree::{StagedFileKind, staged_unit_file, unstaged_files, validate_path};
use super::language_packages;
use super::not_carried::{NOT_CARRIED_REASON, not_carried_packages};
use super::repoless::PackageRoute;
use super::safety::{
    BareWord, CopyToken, ExportPath, ExportToken, double_quoted_word_refusal,
    is_valid_tuned_profile, operator_kargs, sanitize_shell_value,
};
use super::service_intent::{is_package_installable, manual_follow_up_line, render_service_intent};
use super::var_dirs::{EffectiveBacking, UnresolvedReason, effective_var_backing};

/// Human-readable label for a `DegradationReason`.
fn degradation_reason_label(reason: &DegradationReason) -> &'static str {
    match reason {
        DegradationReason::MemberExcluded => "member excluded",
        DegradationReason::MemberOverridden => "member overridden",
        DegradationReason::MultilibConflict => "multilib conflict",
    }
}

/// Emit a section header + body only when the body is non-empty.
///
/// This is the **sole** place that produces `# === ... ===` banners.
/// Individual section functions compute their body lines and pass them
/// through here — they never format the banner themselves.  This makes
/// it structurally impossible to emit an empty section header.
///
/// A trailing blank line is appended automatically after the body.
pub(crate) fn section(header: &str, body: Vec<String>) -> Vec<String> {
    if body.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![format!("# === {} ===", header)];
    lines.extend(body);
    lines.push(String::new());
    lines
}

/// Render the Containerfile content from a snapshot.
///
/// When `materialized_roots` is provided, COPY lines are derived from the
/// directories the config tree actually wrote — guaranteeing the
/// Containerfile and config tree describe the same system. When `None`,
/// roots are computed from the snapshot (standalone rendering without
/// prior materialization).
pub fn render_containerfile(
    snap: &InspectionSnapshot,
    materialized_roots: Option<&[String]>,
    render_ctx: Option<&RenderContext>,
) -> String {
    render_containerfile_inner(snap, materialized_roots, None, render_ctx)
}

pub fn render_containerfile_with_originals(
    snap: &InspectionSnapshot,
    materialized_roots: Option<&[String]>,
    original_includes: &std::collections::HashMap<String, bool>,
    render_ctx: Option<&RenderContext>,
) -> String {
    render_containerfile_inner(
        snap,
        materialized_roots,
        Some(original_includes),
        render_ctx,
    )
}

fn render_containerfile_inner(
    snap: &InspectionSnapshot,
    materialized_roots: Option<&[String]>,
    original_includes: Option<&std::collections::HashMap<String, bool>>,
    render_ctx: Option<&RenderContext>,
) -> String {
    let base = base_image_from_snapshot(snap);
    let base_str = base.as_deref().unwrap_or("");
    let mut lines: Vec<String> = Vec::new();

    // Subscription mount instructions — show before any other build content
    if snap.preserved_subscription {
        lines.push("# === RHEL Subscription ===".into());
        lines.push("# This build requires RHEL entitlement certificates for repo access.".into());
        lines.push("# Build with:".into());
        lines.push("#   podman build \\".into());
        lines.push(
            "#     -v ./subscription/entitlement:/run/secrets/etc-pki-entitlement:z \\".into(),
        );
        lines.push("#     -v ./subscription/rhsm:/run/secrets/rhsm:z \\".into());
        lines.push("#     -v ./subscription/redhat.repo:/run/secrets/redhat.repo:z \\".into());
        lines.push("#     -f Containerfile .".into());
        lines.push("#".into());
        lines.push("# Or use: inspectah build <tarball> -t <name:tag>".into());
        lines.push(String::new());
    }

    // Completeness warning — surface before any build instructions
    let affected_ids: Vec<_> = match &snap.completeness {
        Completeness::Partial {
            degraded_sections, ..
        } => degraded_sections.clone(),
        Completeness::Incomplete {
            failed_sections,
            degraded_sections,
            ..
        } => {
            let mut ids = failed_sections.clone();
            ids.extend(degraded_sections.iter().copied());
            ids
        }
        Completeness::Complete => vec![],
    };
    if !affected_ids.is_empty() {
        let section_names: Vec<String> = affected_ids
            .iter()
            .map(|id| format!("{:?}", id).to_lowercase())
            .collect();
        lines.push(
            "# WARNING: This Containerfile was generated from an incomplete inspection.".into(),
        );
        lines.push(format!(
            "# The following inspector sections may be missing or degraded: {}",
            section_names.join(", ")
        ));
        lines.push("# Review the audit report for details before building.".into());
        lines.push(String::new());
    }

    // 1. Packages section (FROM + repos + GPG + modules + groups + packages)
    lines.extend(packages_section_lines(
        snap,
        base.as_deref(),
        original_includes,
        render_ctx,
    ));

    // bootc label for ostree-desktops base images
    if matches!(snap.system_type, SystemType::RpmOstree | SystemType::Bootc)
        && base_str.contains("fedora-ostree-desktops")
    {
        lines.push("# ostree-desktops images may need bootc label for compatibility".into());
        lines.push("LABEL containers.bootc 1".into());
        lines.push(String::new());
    }

    // 2. Services
    if is_degraded(&snap.completeness, InspectorId::Services) {
        lines.push("# FIXME: services data may be incomplete (inspector returned degraded)".into());
    }
    lines.extend(services_section_lines(snap));
    lines.extend(drop_ins_section_lines(snap));

    // 3. Firewall zones
    if is_degraded(&snap.completeness, InspectorId::Network) {
        lines.push("# FIXME: network data may be incomplete (inspector returned degraded)".into());
    }
    lines.extend(network_section_lines(snap, true));

    // 4. Scheduled tasks
    if is_degraded(&snap.completeness, InspectorId::ScheduledTasks) {
        lines.push(
            "# FIXME: scheduled_tasks data may be incomplete (inspector returned degraded)".into(),
        );
    }
    lines.extend(scheduled_tasks_section_lines(snap));

    // 5. Config files
    if is_degraded(&snap.completeness, InspectorId::Config) {
        lines.push("# FIXME: config data may be incomplete (inspector returned degraded)".into());
    }
    lines.extend(config_section_lines(snap, materialized_roots));

    // 6. Non-RPM software
    if is_degraded(&snap.completeness, InspectorId::NonRpmSoftware) {
        lines.push(
            "# FIXME: non_rpm_software data may be incomplete (inspector returned degraded)".into(),
        );
    }
    lines.extend(non_rpm_section_lines(snap));

    // 7. Containers
    if is_degraded(&snap.completeness, InspectorId::Containers) {
        lines.push(
            "# FIXME: containers data may be incomplete (inspector returned degraded)".into(),
        );
    }
    lines.extend(containers_section_lines(snap));

    // 7b. Compose stacks (reference-only comment block)
    lines.extend(compose_comment_lines(snap));

    // 8. Users
    if is_degraded(&snap.completeness, InspectorId::UsersGroups) {
        lines.push(
            "# FIXME: users_groups data may be incomplete (inspector returned degraded)".into(),
        );
    }
    lines.extend(super::users::render_containerfile_users(snap));

    // 9. Kernel/boot
    if is_degraded(&snap.completeness, InspectorId::KernelBoot) {
        lines.push(
            "# FIXME: kernel_boot data may be incomplete (inspector returned degraded)".into(),
        );
    }
    lines.extend(kernel_boot_section_lines(snap));

    // 10. Security & Access Control
    if is_degraded(&snap.completeness, InspectorId::Selinux) {
        lines.push("# FIXME: selinux data may be incomplete (inspector returned degraded)".into());
    }
    lines.extend(selinux_section_lines(snap));

    // 11. Network (non-firewall)
    lines.extend(network_section_lines(snap, false));

    // 12. Secrets comments
    lines.extend(secrets_comment_lines(snap));

    // 12b. /var directory provisioning (tmpfiles.d + mkdir fallback)
    let tmpfiles_staged = render_tmpfiles_conf(snap).is_some();
    lines.extend(var_dir_section_lines(snap, tmpfiles_staged));

    // 13. Epilogue
    lines.extend(tmpfiles_lines());
    lines.extend(validate_lines());

    super::comment::join_lines(&lines)
}

/// Check whether a specific inspector section is degraded (not failed).
fn is_degraded(completeness: &Completeness, id: InspectorId) -> bool {
    match completeness {
        Completeness::Partial {
            degraded_sections, ..
        } => degraded_sections.contains(&id),
        Completeness::Incomplete {
            degraded_sections, ..
        } => degraded_sections.contains(&id),
        Completeness::Complete => false,
    }
}

/// Returns the base image reference from the snapshot, if determinable.
///
/// Uses `target_image.image_ref` (normalized ref from resolution).
/// Returns `None` when unavailable — callers render a comment.
pub fn base_image_from_snapshot(snap: &InspectionSnapshot) -> Option<String> {
    snap.target_image.as_ref().map(|ti| ti.image_ref.clone())
}

// --- Packages section ---

fn canonical_package_id(name: &str, arch: &str) -> String {
    format!("{name}.{arch}")
}

fn install_name_for_package(
    pkg: &PackageEntry,
    duplicate_name_counts: &std::collections::HashMap<String, usize>,
) -> String {
    if duplicate_name_counts
        .get(&pkg.name)
        .copied()
        .unwrap_or_default()
        > 1
        && !pkg.arch.is_empty()
    {
        canonical_package_id(&pkg.name, &pkg.arch)
    } else {
        pkg.name.clone()
    }
}

/// Section id prefix of the CodeReady Builder repository, lowercased.
const CRB_ID_PREFIX: &str = "codeready-builder-";

/// Infixes that mark a CRB section as a debuginfo or source mirror rather
/// than the binary repo EPEL packages resolve against.
const CRB_ID_EXCLUDED_INFIXES: [&str; 2] = ["-debug-", "-source-"];

/// Why the CRB flags are there, said once per Containerfile.
const CRB_BANNER: &str = "# CRB was enabled on the source host; EPEL packages may depend on it.";

/// The CRB section ids this export carries enabled, as the host wrote them.
///
/// `--enablerepo` takes an id verbatim: nothing here reconstructs one from
/// a prefix. That is also why no `$(arch)` can reach a `RUN` line -- an id
/// containing it fails `sanitize_shell_value`, which rejects `$`, `(` and
/// `)`, so the property is the filter's rather than a separate check's.
fn crb_ids(rpm: &RpmSection) -> Vec<RepoId> {
    let mut ids: std::collections::BTreeSet<RepoId> = std::collections::BTreeSet::new();
    for file in rpm
        .repo_files
        .iter()
        .filter(|f| f.disposition.is_included())
    {
        for sec in parse_repo_sections(&file.content) {
            let lower = sec.id.as_str().to_ascii_lowercase();
            if sec.enabled
                && lower.starts_with(CRB_ID_PREFIX)
                && !CRB_ID_EXCLUDED_INFIXES.iter().any(|m| lower.contains(m))
                && sanitize_shell_value(sec.id.as_str()).is_some()
            {
                ids.insert(sec.id);
            }
        }
    }
    ids.into_iter().collect()
}

/// The `--enablerepo` flags and one-time banner that CRB adds to the
/// package, group, module-stream, repo-enabling and disabled-repo
/// transactions.
///
/// Those five ask for it by name (the disabled-repo one through
/// [`CrbFlags::with_repos`]). § 3.1's `RUN dnf install -y
/// policycoreutils-python-utils` at the head of the SELinux block does
/// not, and cannot: it is a `const` in `selinux.rs` that nothing here
/// rewrites. Scoping by call site rather than by sweeping `dnf install`
/// lines is what keeps that exclusion from regressing when a later
/// transaction is added.
struct CrbFlags {
    /// Appended straight after the `dnf` subcommand; empty when no CRB
    /// section is carried.
    flags: String,
    banner_pending: bool,
}

impl CrbFlags {
    fn new(rpm: &RpmSection) -> CrbFlags {
        let ids = crb_ids(rpm);
        CrbFlags {
            flags: ids
                .iter()
                .map(|id| format!(" --enablerepo={id}"))
                .collect::<String>(),
            banner_pending: !ids.is_empty(),
        }
    }

    /// These flags plus `--enablerepo=<id>` for each of `ids`, for a
    /// transaction that also needs repos the source host kept disabled.
    /// The banner stays with `self`. Every id must already have passed
    /// `sanitize_shell_value`.
    fn with_repos<'a>(&self, ids: impl IntoIterator<Item = &'a RepoId>) -> CrbFlags {
        CrbFlags {
            flags: ids.into_iter().fold(self.flags.clone(), |flags, id| {
                format!("{flags} --enablerepo={id}")
            }),
            banner_pending: false,
        }
    }

    /// The banner, for the first transaction that asks.
    fn banner(&mut self) -> Option<String> {
        if !self.banner_pending {
            return None;
        }
        self.banner_pending = false;
        Some(CRB_BANNER.to_string())
    }
}

/// A `RUN dnf install -y` transaction over `names`, with the cleanup tail
/// every install block in this file carries.
fn dnf_install_lines(names: &[&str], crb: &CrbFlags) -> Vec<String> {
    dnf_install_with_local_lines(names, &[], crb)
}

/// [`dnf_install_lines`] that also installs RPM files the build copied to
/// `/tmp/`, in the same transaction, and removes them after.
fn dnf_install_with_local_lines(
    names: &[&str],
    local: &[&inspectah_core::rpm_filename::StagedName],
    crb: &CrbFlags,
) -> Vec<String> {
    let mut body = vec![format!("RUN dnf install -y{} \\", crb.flags)];
    for name in names {
        body.push(format!("    {} \\", name));
    }
    for rpm in local {
        body.push(format!("    /tmp/{} \\", rpm.as_str()));
    }
    if !local.is_empty() {
        let paths: Vec<String> = local
            .iter()
            .map(|rpm| format!("/tmp/{}", rpm.as_str()))
            .collect();
        body.push(format!("    && rm {} \\", paths.join(" ")));
    }
    body.push("    && dnf clean all \\".into());
    body.push("    && rm -rf \\".into());
    body.push("        /var/cache/dnf \\".into());
    body.push("        /var/lib/dnf/history* \\".into());
    body.push("        /var/log/dnf* \\".into());
    body.push("        /var/log/hawkey.log \\".into());
    body.push("        /var/log/rhsm".into());
    body
}

/// Upstream "latest" RPM URLs for release packages whose project
/// publishes one. Advice on a `#` line only: it reaches the network,
/// which installing from the copied definition does not have to.
const RELEASE_PERMALINKS: &[(&str, &str)] = &[(
    "epel-release",
    "https://dl.fedoraproject.org/pub/epel/epel-release-latest-{major}.noarch.rpm",
)];

/// What a permalink substitutes the host's OS major version into.
const OS_MAJOR_PLACEHOLDER: &str = "{major}";

/// The permalink for `pkg` and the host it points at, when the project
/// publishes one and the snapshot recorded an OS version to fill in.
fn release_permalink(snap: &InspectionSnapshot, pkg: &PackageEntry) -> Option<(String, String)> {
    let (_, template) = RELEASE_PERMALINKS
        .iter()
        .find(|(name, _)| *name == pkg.name)?;
    let version_id = &snap.os_release.as_ref()?.version_id;
    let major = sanitize_shell_value(version_id.split('.').next()?).filter(|m| !m.is_empty())?;
    let url = template.replace(OS_MAJOR_PLACEHOLDER, major);
    let host = url.split_once("://")?.1.split('/').next()?.to_string();
    Some((url, host))
}

/// Longest line the repo-enabling narration wraps to, counting the
/// leading `# `. § 5.2's block text is this wrap over its sentences.
const NARRATION_WIDTH: usize = 80;

/// Greedy word wrap into `# `-prefixed comment lines.
fn comment_paragraph(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::from("#");
    for word in text.split_whitespace() {
        if current.len() > 1 && current.len() + 1 + word.len() > NARRATION_WIDTH {
            lines.push(std::mem::replace(&mut current, String::from("#")));
        }
        current.push(' ');
        current.push_str(word);
    }
    if current.len() > 1 {
        lines.push(current);
    }
    lines
}

/// § 5.2's narration for one repo-enabling package: why it installs by
/// name from the copied definition, and what the build needs for that to
/// work.
fn repo_enabling_narration(
    snap: &InspectionSnapshot,
    rpm: &RpmSection,
    pkg: &PackageEntry,
) -> Vec<String> {
    let mut text = format!(
        "{} was installed outside any repo on the source host; the host's enabled repos \
         served it, and its repo files and key are copied above, so it installs from the \
         copied definition. The build needs those repo endpoints reachable with any \
         credentials and CA trust they require.",
        super::language_packages::sanitize_for_comment(&pkg.name)
    );
    if let Some(file) = super::repo_enabling::owned_repo_file_name(rpm, pkg) {
        text.push_str(&format!(
            " Installing after the COPY may leave a cosmetic {}.rpmnew.",
            super::language_packages::sanitize_for_comment(file)
        ));
    }
    let permalink = release_permalink(snap, pkg);
    if let Some((_, host)) = &permalink {
        text.push_str(&format!(" Alternative, when {host} is reachable:"));
    }
    let mut lines = comment_paragraph(&text);
    if let Some((url, _)) = &permalink {
        lines.push(format!("# RUN dnf install -y {url}"));
    }
    lines
}

/// § 3.3's demotion line, printed in place of the active install when no
/// serving repo definition survives the export.
fn demotion_fixme(pkg: &PackageEntry, loss: &super::repo_enabling::ServingLoss) -> String {
    let name = super::language_packages::sanitize_for_comment(&pkg.name);
    let id = super::language_packages::sanitize_for_comment(loss.id.as_str());
    let tail =
        format!("{name} installs from that repo definition, which this export does not carry");
    match &loss.file {
        Some(file) => format!(
            "# FIXME: include {} under Configuration Files, or re-include repo {id}, or \
             upload the RPM; {tail}",
            super::language_packages::sanitize_for_comment(file)
        ),
        // Nothing captured defines the section, so there is no file to
        // name; the advice that would name one is dropped, not invented.
        None => format!("# FIXME: re-include repo {id}, or upload the RPM; {tail}"),
    }
}

fn packages_section_lines(
    snap: &InspectionSnapshot,
    base: Option<&str>,
    original_includes: Option<&std::collections::HashMap<String, bool>>,
    render_ctx: Option<&RenderContext>,
) -> Vec<String> {
    let mut lines = Vec::new();

    match base {
        // The reference is a bare `FROM` argument, so it is proved.
        Some(b) => match BareWord::parse_image_ref(b) {
            Ok(image) => lines.push(format!("FROM {image}")),
            Err(e) => {
                lines.push(format!(
                    "# FIXME: target image {} refused ({e}); FROM line omitted. Use --base-image to specify.",
                    language_packages::sanitize_for_comment(b)
                ));
            }
        },
        None => lines.push("# FROM line omitted \u{2014} target image could not be determined. Use --base-image to specify.".to_string()),
    }
    lines.push(String::new());

    let rpm = match &snap.rpm {
        Some(rpm) => rpm,
        None => return lines,
    };

    // Partial leaf authority indicator for aggregate snapshots
    if let (Some(auth), Some(total)) = (rpm.leaf_authority_hosts, rpm.leaf_total_hosts)
        && auth < total
    {
        lines.push(format!(
            "# Leaf classification: {}/{} hosts (partial authority)",
            auth, total
        ));
        lines.push(String::new());
    }

    // Repo files
    let included_repos: usize = rpm
        .repo_files
        .iter()
        .filter(|r| r.disposition.is_included() && !r.is_default_repo)
        .count();
    if included_repos > 0 {
        let body = vec!["COPY config/etc/yum.repos.d/ /etc/yum.repos.d/".into()];
        lines.extend(section(
            &format!("Custom Repositories ({included_repos})"),
            body,
        ));
    }

    // GPG keys — batch standard-dir keys, per-key import for non-standard
    let included_gpg: Vec<_> = rpm
        .gpg_keys
        .iter()
        .filter(|k| k.disposition.is_included())
        .collect();
    if !included_gpg.is_empty() {
        let mut gpg_body: Vec<String> = Vec::new();

        const STANDARD_GPG_DIR: &str = "etc/pki/rpm-gpg";

        // Classify keys: safe vs unsafe, standard-dir vs non-standard vs root
        let mut standard_keys: Vec<&inspectah_core::types::rpm::RepoFile> = Vec::new();
        let mut nonstandard_keys: Vec<&inspectah_core::types::rpm::RepoFile> = Vec::new();
        let mut root_keys: Vec<&inspectah_core::types::rpm::RepoFile> = Vec::new();
        let mut nonstandard_dirs: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();

        for key in &included_gpg {
            // Host paths are absolute — check for traversal, NUL, and whitespace
            if key.path.contains("..") || key.path.contains('\0') {
                gpg_body.push(format!(
                    "# FIXME: copy this GPG key in by hand, or exclude it, then rebuild; \
                     its path contains unsafe characters: {}",
                    super::safety::html_escape(&key.path)
                ));
                continue;
            }
            // The config tree stager does not write a key whose path it
            // refuses, so nothing below may name one.
            if let Err(e) = validate_path(key.path.trim_start_matches('/')) {
                gpg_body.push(format!(
                    "# FIXME: GPG key {} not copied: {e}",
                    language_packages::sanitize_for_comment(&key.path)
                ));
                continue;
            }
            if super::safety::sanitize_shell_value(&key.path).is_none() {
                gpg_body.push(format!(
                    "# FIXME: copy this GPG key in by hand, or exclude it, then rebuild; \
                     its path is unsafe for the shell: {}",
                    super::safety::html_escape(&key.path)
                ));
                continue;
            }
            let rel = key.path.trim_start_matches('/');
            match rel.rsplit_once('/') {
                Some((dir, _)) if !dir.is_empty() => {
                    if dir == STANDARD_GPG_DIR {
                        standard_keys.push(key);
                    } else {
                        nonstandard_dirs.insert(dir.to_string());
                        nonstandard_keys.push(key);
                    }
                }
                _ => {
                    // Root-level key (e.g., /good-key) — stage the file directly
                    root_keys.push(key);
                    nonstandard_keys.push(key);
                }
            }
        }

        // Standard-dir keys: single directory COPY, no rpm --import needed
        // (RPM automatically picks up keys in /etc/pki/rpm-gpg/)
        if !standard_keys.is_empty() {
            gpg_body.push(format!(
                "COPY config/{STANDARD_GPG_DIR}/ /{STANDARD_GPG_DIR}/"
            ));
        }

        // Non-standard directory keys: COPY parent dir + per-key rpm --import
        for dir in &nonstandard_dirs {
            match super::safety::sanitize_shell_value(dir) {
                Some(safe) => gpg_body.push(format!("COPY config/{safe}/ /{safe}/")),
                None => gpg_body.push(format!(
                    "# FIXME: GPG directory path contains unsafe characters: {}",
                    super::safety::html_escape(dir)
                )),
            }
        }

        // COPY root-level keys directly (no parent directory)
        for key in &root_keys {
            let rel = key.path.trim_start_matches('/');
            gpg_body.push(format!("COPY config/{rel} {}", key.path));
        }

        // rpm --import only for non-standard keys (standard-dir keys are auto-imported)
        for key in &nonstandard_keys {
            gpg_body.push(format!("RUN rpm --import {}", key.path));
        }
        lines.extend(section(
            &format!("GPG Keys ({})", included_gpg.len()),
            gpg_body,
        ));
    }

    let mut crb = CrbFlags::new(rpm);

    // Module streams
    let enabled_modules: Vec<_> = rpm
        .module_streams
        .iter()
        .filter(|ms| ms.disposition.is_included() && !ms.baseline_match)
        .collect();
    if !enabled_modules.is_empty() {
        let mut mod_body: Vec<String> = Vec::new();
        for ms in &enabled_modules {
            // Sanitize all host-derived values before shell interpolation
            if sanitize_shell_value(&ms.module_name).is_none()
                || sanitize_shell_value(&ms.stream).is_none()
            {
                mod_body.push(format!(
                    "# FIXME: module stream contains unsafe characters, skipped: {:?}:{:?}",
                    ms.module_name, ms.stream
                ));
                continue;
            }
            let profiles = if ms.profiles.is_empty() {
                String::new()
            } else {
                // Sanitize each profile name
                let safe_profiles: Vec<&str> = ms
                    .profiles
                    .iter()
                    .filter_map(|p| sanitize_shell_value(p))
                    .collect();
                if safe_profiles.len() != ms.profiles.len() {
                    mod_body.push(format!(
                        "# FIXME: module profile contains unsafe characters, skipped: {:?}",
                        ms.profiles
                    ));
                    continue;
                }
                format!("/{}", safe_profiles.join(","))
            };
            if let Some(banner) = crb.banner() {
                mod_body.push(banner);
            }
            mod_body.push(format!(
                "RUN dnf module enable -y{} {}:{}{}",
                crb.flags, ms.module_name, ms.stream, profiles
            ));
        }
        lines.extend(section("Module Streams", mod_body));
    }

    // Package Groups: the renderable groups and their member names, so the
    // members can be suppressed from the individual packages section. The
    // `dnf group install` itself is emitted further down, after the
    // repo-enabling block and the combined local and disabled-repo
    // install.
    let mut refused_groups: Vec<String> = Vec::new();
    let (renderable_groups, group_member_suppressed): (
        Vec<&inspectah_core::types::rpm::InstalledGroup>,
        std::collections::HashSet<String>,
    ) = {
        let mut suppressed = std::collections::HashSet::new();
        let mut renderable: Vec<&inspectah_core::types::rpm::InstalledGroup> = Vec::new();

        if let Some(ctx) = render_ctx
            && let Some(groups) = &rpm.installed_groups
        {
            // Collect renderable groups sorted alphabetically by name
            // A group whose name cannot sit inside the double quotes of
            // `dnf group install` is not rendered as a group: it gets a
            // FIXME below, and its members install on their own.
            renderable = groups
                .iter()
                .filter(|g| ctx.is_renderable(&g.name))
                .filter(|g| {
                    let refusal = double_quoted_word_refusal(&g.name);
                    if let Some(why) = refusal {
                        refused_groups.push(format!(
                            "# FIXME: package group {} not installed as a group: the name {why}",
                            language_packages::sanitize_for_comment(&g.name)
                        ));
                    }
                    refusal.is_none()
                })
                .collect();
            renderable.sort_by(|a, b| a.name.cmp(&b.name));

            // Build the member suppression set from all renderable groups
            for g in &renderable {
                for member in &g.members {
                    suppressed.insert(member.clone());
                }
                // NOTE: optional_installed are NOT suppressed here.
                // `dnf group install` does not install optional members,
                // so they must appear in the individual packages section.
            }
        }

        (renderable, suppressed)
    };

    // Provenance comments — explain why group members appear individually.
    // Provenance comments for groups whose members appear individually.
    // Ungrouped groups emit NO comment — their members silently join the
    // individual packages section (per design: ungrouping = treat as leaf).
    // Degraded groups get an explanatory comment. Optional spillover is
    // always noted regardless of group state. Emitted after the group
    // install, just before the individual packages they explain.
    let mut provenance_lines: Vec<String> = Vec::new();
    if let Some(ctx) = render_ctx
        && let Some(groups) = &rpm.installed_groups
    {
        // Collect by group, sorted alphabetically
        let mut sorted_groups: Vec<&inspectah_core::types::rpm::InstalledGroup> =
            groups.iter().collect();
        sorted_groups.sort_by(|a, b| a.name.cmp(&b.name));

        for g in &sorted_groups {
            if let Some(GroupRenderState::Degraded { reason }) = ctx.group_states.get(&g.name) {
                provenance_lines.push(format!(
                    "# \"{}\" degraded ({}): members rendered individually",
                    g.name,
                    degradation_reason_label(reason),
                ));
            }
            // Optional spillover — always emitted regardless of group state
            if !g.optional_installed.is_empty() {
                provenance_lines.push(format!("# Optional members from \"{}\"", g.name));
            }
        }
    }

    // Packages
    let mut install_names = Vec::new();
    let mut todo_lines = Vec::new();

    for pkg in &rpm.packages_added {
        if let Some(line) = manual_follow_up_line(pkg) {
            todo_lines.push(line);
        }
    }

    let leaf_filter: Option<std::collections::HashSet<String>> = rpm
        .leaf_packages
        .as_ref()
        .map(|leaf_packages| leaf_packages.iter().cloned().collect());

    let baseline_suppressed_set: std::collections::HashSet<String> = rpm
        .baseline_suppressed
        .as_ref()
        .map(|v| v.iter().cloned().collect())
        .unwrap_or_default();

    let no_uploads = inspectah_core::types::verified_upload::VerifiedUploads::new();
    let verified = render_ctx.map_or(&no_uploads, |c| &c.verified_uploads);
    let pulled_in = super::service_intent::pulled_in_by_included(rpm);

    let installable_packages: Vec<&PackageEntry> = rpm
        .packages_added
        .iter()
        .filter(|pkg| is_package_installable(pkg))
        .filter(|pkg| {
            !baseline_suppressed_set.contains(&canonical_package_id(&pkg.name, &pkg.arch))
        })
        .filter(|pkg| {
            // A package from a disabled repo that an included package
            // depends on is named in the combined install, whatever its
            // own disposition and leaf status. A plain or group
            // transaction does not enable that repo, so dnf cannot pull it
            // in there.
            // The disposition is left as it is: refine's non-leaf default
            // and an operator's exclusion both still read as excluded.
            if pulled_in.contains(&canonical_package_id(&pkg.name, &pkg.arch))
                && matches!(
                    super::repoless::route_for(rpm, pkg, verified),
                    PackageRoute::DisabledRepo(_)
                )
            {
                return true;
            }
            if !pkg.disposition.is_included() {
                return false;
            }
            // Locked packages are already decided — skip leaf filter
            if pkg.locked {
                return true;
            }
            // Anaconda-classified packages with include=true had their
            // include state set by the anaconda classifier, not by
            // leaf classification. Bypass the leaf filter.
            if pkg.source_repo == "anaconda" && pkg.disposition.is_included() {
                return true;
            }
            let id = canonical_package_id(&pkg.name, &pkg.arch);
            if let Some(orig) = original_includes {
                let was_included = orig
                    .get(&id)
                    .copied()
                    .unwrap_or(pkg.disposition.is_included());
                if was_included != pkg.disposition.is_included() {
                    return true;
                }
            }
            leaf_filter
                .as_ref()
                .is_none_or(|leaf_ids| leaf_ids.contains(&id))
        })
        .collect();

    let duplicate_name_counts: std::collections::HashMap<String, usize> = installable_packages
        .iter()
        .fold(std::collections::HashMap::new(), |mut counts, pkg| {
            *counts.entry(pkg.name.clone()).or_insert(0) += 1;
            counts
        });

    // Repo-enabling packages (e.g. epel-release) install in an earlier
    // transaction so their GPG keys and repo configs are present when DNF
    // verifies signatures of packages from those repos. Which packages
    // those are is `repo_enabling_from` and nothing else: deciding it by a
    // name ending in `-release` rendered a package the host's repos never
    // served as an active install *and* as a MANUAL block.
    let mut repo_enabling: Vec<(&PackageEntry, String)> = Vec::new();
    let mut demoted: Vec<(&PackageEntry, super::repo_enabling::ServingLoss)> = Vec::new();
    // Packages installed from a repo the source host has disabled, by repo.
    // They come from the same selection as the plain list, so a non-leaf
    // dependency is left to dnf here as it is there, and naming and
    // multiarch handling match.
    let mut disabled_repo_installs: std::collections::BTreeMap<RepoId, Vec<String>> =
        std::collections::BTreeMap::new();

    for pkg in &installable_packages {
        let route = super::repoless::route_for(rpm, pkg, verified);
        // Suppress packages whose name is covered by a renderable group,
        // except one from a disabled repo: the group transaction does not
        // enable that repo, so it cannot install the member.
        if !matches!(route, PackageRoute::DisabledRepo(_))
            && group_member_suppressed.contains(&pkg.name)
        {
            continue;
        }
        let install_name = install_name_for_package(pkg, &duplicate_name_counts);
        if sanitize_shell_value(&install_name).is_none() {
            continue;
        }
        // The repo-enabling match comes first: a package that lost its
        // proof carries the repo-less annotation `demote_unserved_repo_enabling`
        // gave it, and still owes the operator the FIXME naming what to
        // re-include.
        match super::repo_enabling::repo_enabling_export(rpm, pkg) {
            super::repo_enabling::RepoEnablingExport::NotEnabling => match route {
                PackageRoute::ByName => install_names.push(install_name),
                PackageRoute::DisabledRepo(id) => disabled_repo_installs
                    .entry(id)
                    .or_default()
                    .push(install_name),
                // No enabled repo on the source host serves a repo-less
                // package, so a plain `dnf install` of it fails with
                // `Unable to find a match`. It renders in the repo-less
                // block, from the upload when there is one.
                PackageRoute::Upload(_) | PackageRoute::Repoless => {}
            },
            super::repo_enabling::RepoEnablingExport::Served => {
                repo_enabling.push((pkg, install_name))
            }
            // A demoted package the operator uploaded an RPM for installs
            // from it in the repo-less block, which settles the FIXME.
            super::repo_enabling::RepoEnablingExport::Lost(loss) => {
                if !matches!(route, PackageRoute::Upload(_)) {
                    demoted.push((pkg, loss));
                }
            }
        }
    }

    if !repo_enabling.is_empty() || !demoted.is_empty() {
        let mut repo_body: Vec<String> = Vec::new();
        for (pkg, _) in &repo_enabling {
            repo_body.extend(repo_enabling_narration(snap, rpm, pkg));
        }
        for (pkg, loss) in &demoted {
            repo_body.push(demotion_fixme(pkg, loss));
        }
        if !repo_enabling.is_empty() {
            let mut names: Vec<&str> = repo_enabling.iter().map(|(_, n)| n.as_str()).collect();
            names.sort();
            if let Some(banner) = crb.banner() {
                repo_body.push(banner);
            }
            repo_body.extend(dnf_install_lines(&names, &crb));
        }
        lines.extend(section(
            &format!(
                "Repo-enabling packages ({})",
                repo_enabling.len() + demoted.len()
            ),
            repo_body,
        ));
    }

    // Packages installed from repos the source host has disabled, and the
    // RPMs the export installs from file (verified uploads and included
    // cached RPMs), in one transaction. That transaction enables every
    // one of the disabled repos and names both sets, so dnf resolves them
    // together whichever depends on which, across several hops and
    // through cycles; separate steps had to guess an order and could
    // guess wrong in either direction. Only these packages are named, and
    // no other transaction carries the disabled-repo flags, so enabling
    // those repos cannot change what a group or a plain-list package
    // resolves to. It runs after the repo-enabling block, whose repos it
    // may resolve against, and before the group and plain installs and
    // every service action, any of which can need it. Installing a
    // package early is harmless when nothing needs it.
    let repoless = super::repoless::repoless_plan(snap, render_ctx);
    let mut names: Vec<&str> = disabled_repo_installs
        .values()
        .flatten()
        .map(String::as_str)
        .collect();
    names.sort();
    names.dedup();
    if !names.is_empty() || !repoless.local.is_empty() {
        let mut body: Vec<String> = Vec::new();
        if let Some(banner) = crb.banner() {
            body.push(banner);
        }
        for id in disabled_repo_installs.keys() {
            body.push(format!(
                "# '{id}' is disabled on the source host (enabled=0); enabled for this install only."
            ));
        }
        for rpm in &repoless.local {
            body.extend(rpm.notes.iter().cloned());
            body.push(format!(
                "COPY repoless-packages/{} /tmp/",
                rpm.name.as_str()
            ));
        }
        let local: Vec<&inspectah_core::rpm_filename::StagedName> =
            repoless.local.iter().map(|rpm| &rpm.name).collect();
        body.extend(dnf_install_with_local_lines(
            &names,
            &local,
            &crb.with_repos(disabled_repo_installs.keys()),
        ));
        let header = match (names.len(), local.len()) {
            (n, 0) => format!("Packages from repos disabled on the source host ({n})"),
            (0, k) => format!("Repo-less RPM installs ({k})"),
            (n, k) => format!(
                "Repo-less RPMs and packages from repos disabled on the source host ({})",
                n + k
            ),
        };
        lines.extend(section(&header, body));
    }

    // Advice for the repo-less packages the export cannot install: the
    // MANUAL block, the FIXMEs, and a pre-excluded cached RPM's commented
    // block, kept together beside the install above.
    if !repoless.advice.is_empty() {
        lines.push(String::new());
        lines.push("# === Repo-less RPM packages ===".into());
        lines.extend(repoless.advice);
    }

    if !refused_groups.is_empty() {
        lines.push(String::new());
        lines.extend(refused_groups);
    }

    if !renderable_groups.is_empty() {
        let mut grp_body: Vec<String> = Vec::new();
        if let Some(banner) = crb.banner() {
            grp_body.push(banner);
        }
        grp_body.push(format!("RUN dnf group install -y{} \\", crb.flags));
        for g in &renderable_groups {
            grp_body.push(format!("    \"{}\" \\", g.name));
        }
        grp_body.push("    && dnf clean all \\".into());
        grp_body.push("    && rm -rf \\".into());
        grp_body.push("        /var/cache/dnf \\".into());
        grp_body.push("        /var/lib/dnf/history* \\".into());
        grp_body.push("        /var/log/dnf* \\".into());
        grp_body.push("        /var/log/hawkey.log \\".into());
        grp_body.push("        /var/log/rhsm".into());
        lines.extend(section(
            &format!("Package Groups ({})", renderable_groups.len()),
            grp_body,
        ));
    }

    if !provenance_lines.is_empty() {
        lines.extend(provenance_lines);
        lines.push(String::new());
    }

    if !install_names.is_empty() {
        install_names.sort();
        let names: Vec<&str> = install_names.iter().map(|n| n.as_str()).collect();
        let mut pkg_body: Vec<String> = Vec::new();
        if let Some(banner) = crb.banner() {
            pkg_body.push(banner);
        }
        pkg_body.extend(dnf_install_lines(&names, &crb));
        lines.extend(section(
            &format!("Packages ({})", install_names.len()),
            pkg_body,
        ));
    }

    lines.extend(not_carried_lines(snap));

    if !todo_lines.is_empty() {
        lines.push(String::new());
        lines.extend(section("Manual Follow-up Required", todo_lines));
    }

    // Version locks
    let included_locks: Vec<_> = rpm
        .version_locks
        .iter()
        .filter(|vl| vl.disposition.is_included())
        .collect();
    if !included_locks.is_empty() {
        let mut safe_locks = Vec::new();
        let mut unsafe_locks = Vec::new();
        for vl in &included_locks {
            if sanitize_shell_value(&vl.raw_pattern).is_some() {
                safe_locks.push(vl);
            } else {
                unsafe_locks.push(vl);
            }
        }
        if !safe_locks.is_empty() {
            let mut vl_body: Vec<String> = Vec::new();
            vl_body.push("RUN dnf install -y python3-dnf-plugin-versionlock && \\".into());
            for vl in &safe_locks {
                vl_body.push(format!("    dnf versionlock add {} && \\", vl.raw_pattern));
            }
            vl_body.push("    dnf clean all".into());
            lines.extend(section("Version Locks", vl_body));
        }
        for vl in &unsafe_locks {
            lines.push(format!(
                "# FIXME: version lock pattern contains unsafe characters, skipped: {:?}",
                vl.raw_pattern
            ));
        }
        if !unsafe_locks.is_empty() {
            lines.push(String::new());
        }
    }

    lines
}

/// The `# Not carried:` block: a header with the reason and count, then
/// one line per [`not_carried_packages`] entry, in its order, naming any
/// units the package had enabled. Comments only, so nothing in it is
/// installed. Empty, with no header, when the list is.
///
/// The name and the units come from the snapshot, which an imported one
/// can fill with anything, so each is sanitized: a newline in either
/// would end the comment and turn the rest into an active instruction.
/// Exclusion keeps these packages away from the install-token checks, so
/// nothing upstream has screened them.
fn not_carried_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let list = not_carried_packages(snap);
    if list.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![format!(
        "# Not carried: {NOT_CARRIED_REASON} ({})",
        list.len()
    )];
    for pkg in &list {
        let name = language_packages::sanitize_for_comment(&pkg.display_name());
        let units: Vec<String> = pkg
            .enabled_units
            .iter()
            .map(|unit| language_packages::sanitize_for_comment(unit))
            .collect();
        lines.push(match units.as_slice() {
            [] => format!("#   {name}"),
            [unit] => format!("#   {name} ({unit} was enabled)"),
            units => format!("#   {name} ({} were enabled)", units.join(", ")),
        });
    }
    lines.push(String::new());
    lines
}

// --- Services section (delegated to service_intent authority) ---

fn services_section_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let plan = render_service_intent(snap);
    section("Service Enablement", plan.lines)
}

// --- Service Drop-ins section ---

fn drop_ins_section_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let services = match &snap.services {
        Some(s) => s,
        None => return Vec::new(),
    };

    let included_count = services
        .drop_ins
        .iter()
        .filter(|d| d.disposition.is_included())
        .count();
    if included_count == 0 {
        return Vec::new();
    }

    let mut body = Vec::new();
    body.push(format!(
        "# {} service drop-in(s) — override snippets for systemd units",
        included_count
    ));
    body.push("COPY drop-ins/etc/systemd/system/ /etc/systemd/system/".into());

    section("Service Drop-ins", body)
}

// --- Network section ---

fn network_section_lines(snap: &InspectionSnapshot, firewall_only: bool) -> Vec<String> {
    let mut lines = Vec::new();
    let network = match &snap.network {
        Some(n) => n,
        None => return lines,
    };

    if firewall_only {
        // Only count actionable (non-inventory, non-advisory) items.
        // Network findings are inventory — they display data but never
        // produce Containerfile output.
        let included_zones: usize = network
            .firewall_zones
            .iter()
            .filter(|z| z.disposition.is_included())
            .count();
        let included_direct_rules: usize = network
            .firewall_direct_rules
            .iter()
            .filter(|r| r.disposition.is_included())
            .count();
        if included_zones > 0 || included_direct_rules > 0 {
            let mut fw_body: Vec<String> = Vec::new();
            if included_zones > 0 {
                fw_body.push(format!(
                    "# {} custom firewall zone(s) — included in COPY config/etc/ below",
                    included_zones
                ));
            }
            lines.extend(section("Firewall Configuration", fw_body));
        }
        return lines;
    }

    // Non-firewall network config
    {
        let body: Vec<String> = network
            .static_routes
            .iter()
            .map(|r| format!("# Static route file: {}", r.path))
            .collect();
        lines.extend(section("Static Routes", body));
    }

    {
        let mut body: Vec<String> = Vec::new();
        if !network.hosts_additions.is_empty() {
            body.push("# FIXME: These /etc/hosts entries need to be added to the image:".into());
            for h in &network.hosts_additions {
                body.push(format!("#   {}", h));
            }
        }
        lines.extend(section("/etc/hosts Additions", body));
    }

    {
        let body: Vec<String> = network
            .proxy
            .iter()
            .map(|p| format!("# {}: {}", p.source, p.line))
            .collect();
        lines.extend(section("Proxy Configuration", body));
    }

    lines
}

// --- Scheduled Tasks section ---

fn scheduled_tasks_section_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let st = match &snap.scheduled_tasks {
        Some(s) => s,
        None => return Vec::new(),
    };

    let local_timers: Vec<_> = st
        .systemd_timers
        .iter()
        .filter(|t| t.source == "local" && t.disposition.is_included())
        .collect();

    let included_timers: Vec<_> = st
        .generated_timer_units
        .iter()
        .filter(|u| u.disposition.is_included())
        .collect();

    let has_content =
        !local_timers.is_empty() || !included_timers.is_empty() || !st.at_jobs.is_empty();
    if !has_content {
        return Vec::new();
    }

    let mut body = Vec::new();

    if !local_timers.is_empty() || !included_timers.is_empty() {
        body.push("COPY config/etc/systemd/system/ /etc/systemd/system/".into());
    }

    // Timer names are snapshot strings: every one that reaches a comment
    // is escaped, or a newline in it would end the comment.
    if !local_timers.is_empty() {
        let names: Vec<String> = local_timers
            .iter()
            .map(|t| language_packages::sanitize_for_comment(&format!("{}.timer", t.name)))
            .collect();
        body.push(format!(
            "# Existing local timers ({}): {}",
            local_timers.len(),
            names.join(", ")
        ));
    }

    if !included_timers.is_empty() {
        let names: Vec<String> = included_timers
            .iter()
            .filter(|u| !u.name.is_empty())
            .map(|u| language_packages::sanitize_for_comment(&u.name))
            .collect();
        body.push(format!(
            "# Converted from cron: {} timer(s): {}",
            included_timers.len(),
            names.join(", ")
        ));
    }

    // Consolidate timer enables. A unit is enabled only if the config
    // tree staged it under the same name and the name is safe in a shell
    // word; otherwise the Containerfile says so in a FIXME.
    let mut timer_names = Vec::new();
    for t in &local_timers {
        if let Some(unit) = enableable_unit(&t.name, "timer", &mut body) {
            timer_names.push(unit);
        }
    }
    let mut reboot_service_names = Vec::new();
    for u in &included_timers {
        if !u.name.is_empty() {
            // @reboot entries have no timer — enable the service instead
            if u.cron_expr == "@reboot" {
                if let Some(unit) = enableable_unit(&u.name, "service", &mut body) {
                    reboot_service_names.push(unit);
                }
            } else if let Some(unit) = enableable_unit(&u.name, "timer", &mut body) {
                timer_names.push(unit);
            }
        }
    }
    if !timer_names.is_empty() {
        body.push(format!("RUN systemctl enable {}", timer_names.join(" ")));
    }
    if !reboot_service_names.is_empty() {
        body.push("# @reboot cron job(s) — boot-triggered oneshot service(s):".to_string());
        body.push(format!(
            "RUN systemctl enable {}",
            reboot_service_names.join(" ")
        ));
    }

    if !st.at_jobs.is_empty() {
        body.push(format!(
            "# FIXME: {} at job(s) found — convert to systemd timers or cron",
            st.at_jobs.len()
        ));
        for a in &st.at_jobs {
            body.push(format!(
                "#   at job: {}",
                language_packages::sanitize_for_comment(&a.command)
            ));
        }
    }

    section("Scheduled Tasks", body)
}

/// The unit file `name.suffix` if it may be enabled: the config tree
/// staged it ([`staged_unit_file`]) and it is safe as a shell word.
/// Otherwise push a FIXME, with the name escaped as comment text, and
/// return `None`.
fn enableable_unit(name: &str, suffix: &str, body: &mut Vec<String>) -> Option<String> {
    let shown = language_packages::sanitize_for_comment(name);
    let Some(unit) = staged_unit_file(name, suffix) else {
        body.push(format!(
            "# FIXME: timer {shown} not staged: the name is not a single path component"
        ));
        return None;
    };
    if sanitize_shell_value(&unit).is_none() {
        body.push(format!(
            "# FIXME: Timer unit name contains unsafe characters: {shown}"
        ));
        return None;
    }
    Some(unit)
}

// --- Config section ---

fn config_section_lines(
    snap: &InspectionSnapshot,
    materialized_roots: Option<&[String]>,
) -> Vec<String> {
    let mut lines = Vec::new();

    // COPY per top-level dir — use materialized roots when available
    // (single source of truth from write_config_tree), fall back to
    // snapshot-derived roots for standalone rendering.
    let config_roots: Vec<String> = match materialized_roots {
        Some(roots) => roots.to_vec(),
        None => config_copy_roots_from_snapshot(snap),
    };

    // Every file the config tree stager refuses by path, from the
    // stager's own list and check. GPG keys are left to the GPG section,
    // which also keeps a refused key off its `rpm --import` line.
    let unstaged: Vec<_> = unstaged_files(snap)
        .into_iter()
        .filter(|(src, _)| src.kind != StagedFileKind::GpgKey)
        .collect();

    let has_config_content = !config_roots.is_empty()
        || !unstaged.is_empty()
        || snap
            .config
            .as_ref()
            .is_some_and(|c| c.files.iter().any(|f| f.disposition.is_included()));

    if has_config_content {
        let mut cfg_body: Vec<String> = Vec::new();

        // Config inventory comment
        if let Some(config) = &snap.config {
            let total = config
                .files
                .iter()
                .filter(|f| f.disposition.is_included())
                .count();
            if total > 0 {
                cfg_body.push(format!("# {} config file(s) captured", total));
            }

            let has_diffs = config.files.iter().any(|f| f.diff_against_rpm.is_some());
            if has_diffs {
                cfg_body.push(
                    "# Config diffs (--config-diffs): see audit-report.md and audit-report.html for per-file diffs."
                        .into(),
                );
            }
        }
        if !cfg_body.is_empty() {
            cfg_body.push(String::new());
        }

        // A file the config tree stager refuses is never written, so
        // say so here.
        for (src, e) in &unstaged {
            cfg_body.push(format!(
                "# FIXME: {} {} not copied: {e}",
                src.kind.label(),
                language_packages::sanitize_for_comment(src.path)
            ));
        }

        // A root is a bare COPY argument, so it is a proved `CopyToken`.
        for root in &config_roots {
            match CopyToken::parse(root) {
                Ok(root) => cfg_body.push(format!("COPY config/{root}/ /{root}/")),
                Err(e) => cfg_body.push(format!(
                    "# FIXME: config directory /{} not copied: {e}",
                    language_packages::sanitize_for_comment(root)
                )),
            }
        }
        lines.extend(section("Configuration Files", cfg_body));
    }

    // CA trust anchors
    if let Some(config) = &snap.config {
        let has_ca = config.files.iter().any(|f| {
            f.disposition.is_included()
                && f.path
                    .trim_start_matches('/')
                    .starts_with("etc/pki/ca-trust/source/anchors/")
        });
        if has_ca {
            let ca_body = vec![
                "# Custom CA certificates detected in /etc/pki/ca-trust/source/anchors/".into(),
                "RUN update-ca-trust".into(),
            ];
            lines.extend(section("CA Trust Store", ca_body));
        }
    }

    // Crypto policy
    lines.extend(crypto_policy_lines(snap));

    lines
}

/// Compute top-level directory roots from included config files in the snapshot.
/// Used as a fallback when materialized roots aren't available (standalone rendering).
fn config_copy_roots_from_snapshot(snap: &InspectionSnapshot) -> Vec<String> {
    let config = match &snap.config {
        Some(c) => c,
        None => return Vec::new(),
    };

    let mut roots: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for f in &config.files {
        if !f.disposition.is_included() {
            continue;
        }
        let rel = f.path.trim_start_matches('/');
        // The stager writes nothing for a refused path, so it adds no root.
        if rel.is_empty() || validate_path(rel).is_err() {
            continue;
        }
        if let Some(top) = rel.split('/').next() {
            roots.insert(top.to_string());
        }
    }
    roots.into_iter().collect()
}

fn crypto_policy_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let config = match &snap.config {
        Some(c) => c,
        None => return Vec::new(),
    };

    for f in &config.files {
        if f.path == "/etc/crypto-policies/config" && f.disposition.is_included() {
            let policy = f
                .content
                .lines()
                .next()
                .unwrap_or("")
                .split('#')
                .next()
                .unwrap_or("")
                .trim();
            if policy.is_empty() || policy == "DEFAULT" {
                return Vec::new();
            }
            if !is_valid_tuned_profile(policy) {
                return vec![
                    format!(
                        "# WARNING: crypto policy name contains unexpected characters, skipped: {:?}",
                        policy
                    ),
                    String::new(),
                ];
            }
            return vec![
                format!("# System crypto policy: {policy}"),
                format!("RUN update-crypto-policies --set {policy}"),
                String::new(),
            ];
        }
    }
    Vec::new()
}

// --- Containers section ---

fn containers_section_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let mut lines = Vec::new();
    let containers = match &snap.containers {
        Some(c) => c,
        None => return lines,
    };

    let included_quadlets: usize = containers
        .quadlet_units
        .iter()
        .filter(|u| u.disposition.is_included())
        .count();
    let included_flatpaks: usize = containers
        .flatpak_apps
        .iter()
        .filter(|a| a.disposition.is_included())
        .count();

    if included_quadlets == 0 && included_flatpaks == 0 {
        return lines;
    }

    let mut body: Vec<String> = Vec::new();
    if included_quadlets > 0 {
        body.push("COPY quadlet/ /etc/containers/systemd/".into());
    }
    if included_flatpaks > 0 {
        body.push("# Flatpak applications — installed on first boot via oneshot service".into());
        body.push("# Manifest: flatpak/flatpak-install.json".into());
        body.push("COPY flatpak/ /usr/share/inspectah/flatpak/".into());
        body.push(
            "COPY flatpak/flatpak-provision.service /etc/systemd/system/flatpak-provision.service"
                .into(),
        );
        body.push("RUN systemctl enable flatpak-provision.service".into());
    }
    let total_workloads = included_quadlets + included_flatpaks;
    lines.extend(section(
        &format!("Container Workloads ({total_workloads})"),
        body,
    ));
    lines
}

// --- Compose comment block (reference-only) ---

/// Generates a Containerfile comment block for detected compose stacks.
///
/// No COPY or RUN directives — compose is reference-only. The comment
/// lists detected stacks and nudges toward Quadlet migration.
fn compose_comment_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let mut lines = Vec::new();
    let containers = match &snap.containers {
        Some(c) => c,
        None => return lines,
    };

    let compose_files: Vec<_> = containers
        .compose_files
        .iter()
        .filter(|c| c.disposition.is_included())
        .collect();

    if compose_files.is_empty() {
        return lines;
    }

    let mut body: Vec<String> = vec![
        "# === Compose stacks detected ===".into(),
        "# The following compose stacks were running on the source host.".into(),
        "# These are application workloads, not OS configuration.".into(),
        "# See compose/ in the build context for the original files.".into(),
        "#".into(),
        "# Consider converting to Quadlet units \u{2014} .container files under".into(),
        "# /etc/containers/systemd/ that let systemd manage your container".into(),
        "# workloads natively.".into(),
        "#   man quadlet(5)".into(),
        "#   https://docs.podman.io/en/latest/markdown/podman-systemd.unit.5.html".into(),
        "#   https://docs.redhat.com/en/documentation/red_hat_enterprise_linux/10/html/building_running_and_managing_containers/porting-containers-to-systemd-using-podman".into(),
        "#".into(),
    ];

    for cf in &compose_files {
        let svc_count = cf.images.len();
        let svc_label = if svc_count == 1 {
            "1 service".to_string()
        } else {
            format!("{svc_count} services")
        };
        // Sanitize path for comment rendering: replace control characters
        // and newlines to prevent comment injection.
        let safe_path: String = cf
            .path
            .chars()
            .map(|c| if c.is_control() { '?' } else { c })
            .collect();
        body.push(format!("#   - /{safe_path} ({svc_label})"));
    }

    lines.extend(section("Compose Stacks (reference only)", body));
    lines
}

// --- Non-RPM Software section ---

fn non_rpm_section_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let mut lines = Vec::new();

    // Language package sections (executable, not advisory).
    let lang_lines = language_packages::language_package_lines(snap);
    if !lang_lines.is_empty() {
        lines.extend(section("Language Packages", lang_lines));
    }

    // Unmanaged files section (opt-in via --include-unmanaged)
    lines.extend(super::unmanaged::unmanaged_file_lines(snap));

    // Remaining non-RPM items that aren't language packages
    // (ELF binaries, .env files, git repos — still advisory).
    let nrs = match &snap.non_rpm_software {
        Some(n) => n,
        None => return lines,
    };

    let migration_items: Vec<_> = nrs
        .items
        .iter()
        .filter(|item| {
            item.review_status == "migration_planned" && !language_packages::is_language_env(item)
        })
        .collect();

    if migration_items.is_empty() {
        return lines;
    }

    let mut body: Vec<String> = Vec::new();
    body.push(
        "# WARNING: These stubs are advisory — source files are NOT in the build context.".into(),
    );
    body.push("# You must manually stage each referenced file/package before building.".into());
    body.push("#".into());

    for item in &migration_items {
        let note = if item.notes.is_empty() {
            String::new()
        } else {
            format!(" — {}", item.notes)
        };

        if (item.lang == "go" || item.method == "go binary") && item.r#static {
            let dest = std::path::Path::new(&item.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            body.push(format!(
                "# COPY {} /usr/local/bin/{}{}",
                item.path, dest, note
            ));
        } else if item.lang == "shell" || item.path.ends_with(".sh") {
            let dest = std::path::Path::new(&item.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            body.push(format!(
                "# COPY {} /usr/local/bin/{}{}",
                item.path, dest, note
            ));
        } else if !item.shared_libs.is_empty() {
            body.push(format!(
                "# {} — dynamic binary, shared libs: {}{}",
                item.path,
                item.shared_libs.join(", "),
                note
            ));
            body.push("# Dependency analysis required before COPY".into());
        } else if item.r#static {
            let dest = std::path::Path::new(&item.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            body.push(format!(
                "# COPY {} /usr/local/bin/{}{}",
                item.path, dest, note
            ));
        } else {
            body.push(format!(
                "# {} ({}) — review required for migration{}",
                item.path, item.method, note
            ));
        }
    }
    lines.extend(section("Non-RPM Software (migration planned)", body));
    lines
}

// Users section — delegated to render::users::render_containerfile_users()

// --- Kernel/Boot section ---

/// The `config.files` path of `profile`'s own `tuned.conf`, when that
/// entry exists and the export is not carrying it.
///
/// `None` covers both the stock-profile case (no entry under
/// `/etc/tuned/<profile>/`, because nothing there was collected) and the
/// included case, which are the two ways activation is consistent.
fn excluded_tuned_profile_file(snap: &InspectionSnapshot, profile: &str) -> Option<String> {
    let path = format!("/etc/tuned/{profile}/tuned.conf");
    let entry = snap
        .config
        .as_ref()?
        .files
        .iter()
        .find(|f| f.path == path)?;
    (!entry.disposition.is_included()).then(|| entry.path.clone())
}

fn kernel_boot_section_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let kb = match &snap.kernel_boot {
        Some(k) => k,
        None => return Vec::new(),
    };

    // Compute all sub-section content first, then only emit the
    // outer header if there's actual content to show.
    let mut body = Vec::new();

    // Kernel arguments
    let safe_kargs = operator_kargs(&kb.cmdline);
    if !safe_kargs.is_empty() {
        body.push("# Kernel Arguments (bootc-native kargs.d)".into());
        body.push(
            "# These are applied at install and honored across image upgrades. See bootc documentation:"
                .into(),
        );
        body.push("# https://containers.github.io/bootc/building/kernel-arguments.html".into());
        body.push("RUN mkdir -p /usr/lib/bootc/kargs.d".into());
        body.push(
            "COPY config/usr/lib/bootc/kargs.d/inspectah-migrated.toml /usr/lib/bootc/kargs.d/"
                .into(),
        );
    }

    // Sysctl overrides — synthesized into a single drop-in
    let included_sysctl: usize = kb
        .sysctl_overrides
        .iter()
        .filter(|s| s.disposition.is_included())
        .count();
    if included_sysctl > 0 {
        body.push(format!(
            "# {} sysctl override(s) — merged into single drop-in",
            included_sysctl
        ));
        body.push(format!(
            "COPY sysctl/etc/sysctl.d/{SYNTHESIZED_SYSCTL_FILE} /etc/sysctl.d/"
        ));
    }

    // Tuned — gated on include flag (set by collectors / aggregate merge).
    // The profile's own file is a `config.files` entry with its own
    // decision, so activation can outlive the file: when the operator
    // excluded it, naming the profile would point the image at a file it
    // does not carry.
    let tuned_included = kb.tuned_disposition.is_included();
    if tuned_included && !kb.tuned_active.is_empty() {
        if !is_valid_tuned_profile(&kb.tuned_active) {
            body.push(format!(
                "# FIXME: tuned profile name contains unsafe characters: {:?}",
                kb.tuned_active
            ));
        } else if let Some(excluded) = excluded_tuned_profile_file(snap, &kb.tuned_active) {
            body.push(format!(
                "# FIXME: include {excluded} under Configuration Files or activate a stock \
                 profile; the active profile is custom and its file is excluded"
            ));
        } else {
            body.push(format!("# Tuned profile: {}", kb.tuned_active));
            body.push(format!(
                "RUN echo \"{}\" > /etc/tuned/active_profile",
                kb.tuned_active
            ));
            body.push("RUN echo \"manual\" > /etc/tuned/profile_mode".into());
            body.push("RUN systemctl enable tuned.service".into());
        }
    }

    section("Kernel and Boot Configuration", body)
}

// --- Security & Access Control section ---

/// Whether a port label renders as a `semanage port` command rather than a
/// FIXME. The install precondition and the port loop read the same answer, so
/// the block never installs `semanage` for a command it will not emit.
fn port_label_renders(pl: &inspectah_core::types::selinux::SelinuxPortLabel) -> bool {
    sanitize_shell_value(&pl.protocol).is_some()
        && sanitize_shell_value(&pl.port).is_some()
        && sanitize_shell_value(&pl.label_type).is_some()
}

fn selinux_section_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let sel = match &snap.selinux {
        Some(s) => s,
        None => return Vec::new(),
    };

    // Compute all sub-section content first, then only emit the
    // outer header if there's actual content to show.
    let mut body = Vec::new();

    if !sel.custom_modules.is_empty() {
        body.push(format!(
            "# FIXME: {} custom policy module(s) detected — \
             export .pp files to config/selinux/ and uncomment the COPY + semodule lines below",
            sel.custom_modules.len()
        ));
        body.push("# COPY config/selinux/ /tmp/selinux/".into());
        body.push("# RUN semodule -i /tmp/selinux/*.pp && rm -rf /tmp/selinux/".into());
    }

    // Non-default booleans
    let non_default: Vec<_> = sel
        .boolean_overrides
        .iter()
        .filter(|b| {
            let inc = b.get("include").and_then(|v| v.as_bool()).unwrap_or(true);
            let nd = b
                .get("non_default")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            inc && nd
        })
        .collect();

    if !non_default.is_empty() {
        body.push(format!(
            "# FIXME: {} non-default boolean(s) detected — verify each is still needed",
            non_default.len()
        ));
        for b in &non_default {
            let bname = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let bval = b
                .get("current_value")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if bname.is_empty() {
                continue;
            }
            if sanitize_shell_value(bname).is_some() && sanitize_shell_value(bval).is_some() {
                body.push(format!("RUN setsebool -P {} {}", bname, bval));
            } else {
                body.push(format!(
                    "# FIXME: boolean name/value contains unsafe characters, skipped: {:?}={:?}",
                    bname, bval
                ));
            }
        }
    }

    // The fcontext rules, and the precondition that installs the tool their
    // commands need. The precondition sits here rather than beside the port
    // labels because it has to precede whichever `semanage` command the block
    // emits first (spec § 3.1).
    let fcontext = super::selinux::render_fcontext_rules(snap);
    let ports_need_semanage = sel.port_labels.iter().any(port_label_renders);
    body.extend(fcontext.preamble);
    if fcontext.emits_command || ports_need_semanage {
        body.extend(
            super::selinux::INSTALL_PRECONDITION
                .iter()
                .map(|l| (*l).to_string()),
        );
    }
    body.extend(fcontext.rules);

    if !sel.port_labels.is_empty() {
        body.push(format!(
            "# {} custom SELinux port label(s) detected",
            sel.port_labels.len()
        ));
        for pl in &sel.port_labels {
            if port_label_renders(pl) {
                body.push(format!(
                    "RUN semanage port -a -t {} -p {} {}",
                    pl.label_type, pl.protocol, pl.port
                ));
            } else {
                body.push(format!(
                    "# FIXME: port label contains unsafe characters, skipped: {:?} {:?} {:?}",
                    pl.label_type, pl.protocol, pl.port
                ));
            }
        }
    }

    if sel.fips_mode {
        body.push(
            "# FIXME: host has FIPS mode enabled — enable FIPS in the bootc image via fips-mode-setup"
                .into(),
        );
    }

    section("Security & Access Control", body)
}

// --- Secrets comment lines ---

fn secrets_comment_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let mut excluded = Vec::new();
    let mut flagged = Vec::new();

    for finding in &snap.redactions {
        if finding.source != "file" {
            continue;
        }
        match finding.kind {
            RedactionKind::Excluded => excluded.push(finding),
            RedactionKind::Flagged => flagged.push(finding),
            RedactionKind::Inline => {}
        }
    }

    if excluded.is_empty() && flagged.is_empty() {
        return Vec::new();
    }

    let mut lines = Vec::new();
    if !excluded.is_empty() {
        let mut body = vec![format!(
            "# {} file(s) excluded from the image for security:",
            excluded.len()
        )];
        for f in &excluded {
            body.push(format!("#   {} ({})", f.path, f.remediation));
        }
        body.push("# See secrets-review.md for details and remediation steps.".into());
        lines.extend(section("Secrets: Excluded Files", body));
    }
    if !flagged.is_empty() {
        let mut body = vec![format!(
            "# {} file(s) flagged for manual review:",
            flagged.len()
        )];
        for f in &flagged {
            body.push(format!("#   {}", f.path));
        }
        body.push("# See secrets-review.md for details.".into());
        lines.extend(section("Secrets: Flagged for Review", body));
    }
    lines
}

// --- /var directory provisioning ---

/// Collect usernames being materialized via useradd in this Containerfile.
///
/// These are users with `containerfile_strategy == "useradd"` and `include == true`.
/// Their accounts are guaranteed to exist at image build time, so tmpfiles.d entries
/// can safely reference them by name.
fn collect_materialized_usernames(snap: &InspectionSnapshot) -> std::collections::HashSet<String> {
    let ug = match &snap.users_groups {
        Some(u) => u,
        None => return std::collections::HashSet::new(),
    };
    ug.users
        .iter()
        .filter(|u| {
            let strategy = u
                .get("containerfile_strategy")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let include = u.get("include").and_then(|v| v.as_bool()).unwrap_or(true);
            strategy == "useradd" && include
        })
        .filter_map(|u| u.get("name").and_then(|v| v.as_str()).map(String::from))
        .collect()
}

/// Well-known RPM packages and the system accounts their scriptlets create.
///
/// Many RPM packages create system accounts whose names differ from the package
/// name (e.g., `postgresql-server` creates `postgres`). This table maps package
/// names to the accounts they provide so tmpfiles.d entries can use symbolic
/// names instead of falling through to numeric UID:GID.
///
/// The table is intentionally conservative — only widely-deployed packages with
/// stable account names are listed. Unknown packages fall through to the numeric
/// path safely.
const KNOWN_PACKAGE_ACCOUNTS: &[(&str, &[&str])] = &[
    ("postgresql-server", &["postgres"]),
    ("bind", &["named"]),
    ("mysql-server", &["mysql"]),
    ("mariadb-server", &["mysql"]),
    ("httpd", &["apache"]),
    ("nginx", &["nginx"]),
    ("redis", &["redis"]),
    ("mongodb-server", &["mongod"]),
    ("rabbitmq-server", &["rabbitmq"]),
    ("elasticsearch", &["elasticsearch"]),
    ("grafana", &["grafana"]),
    ("prometheus", &["prometheus"]),
    ("tomcat", &["tomcat"]),
    ("dovecot", &["dovecot"]),
    ("postfix", &["postfix"]),
    ("chrony", &["chrony"]),
    ("openssh-server", &["sshd"]),
    ("rpcbind", &["rpc"]),
    ("nfs-utils", &["nfsnobody"]),
    ("haproxy", &["haproxy"]),
    ("squid", &["squid"]),
    ("memcached", &["memcached"]),
];

/// Collect system account names provided by replicated RPMs.
///
/// Builds a map from account name to providing package name. Uses two sources:
/// 1. `KNOWN_PACKAGE_ACCOUNTS` — static mapping for packages whose account
///    names differ from the package name.
/// 2. Direct identity — if the package name itself is an account name (e.g.,
///    `redis` creates `redis`), the direct match in `resolve_tmpfiles_ownership`
///    still works via the original `replicated_rpms` set.
///
/// Returns `(rpm_accounts, replicated_rpms)`:
/// - `rpm_accounts`: account name -> package name (from the static mapping)
/// - `replicated_rpms`: set of package names (for direct-match fallback)
fn collect_rpm_provided_accounts(
    snap: &InspectionSnapshot,
) -> (
    std::collections::HashMap<String, String>,
    std::collections::HashSet<String>,
) {
    let replicated: std::collections::HashSet<String> = snap
        .rpm
        .as_ref()
        .map(|rpm| {
            rpm.packages_added
                .iter()
                .filter(|p| p.disposition.is_included())
                .map(|p| p.name.clone())
                .collect()
        })
        .unwrap_or_default();

    let mut accounts = std::collections::HashMap::new();
    for &(pkg, accts) in KNOWN_PACKAGE_ACCOUNTS {
        if replicated.contains(pkg) {
            for &acct in accts {
                accounts.insert(acct.to_string(), pkg.to_string());
            }
        }
    }

    (accounts, replicated)
}

/// Resolve ownership fields for a tmpfiles.d entry.
///
/// Returns `(owner, group, comment)`. Comments go on a separate line above the
/// entry; never inline per tmpfiles.d best practice.
///
/// Resolution order (first match wins):
/// 1. Root (UID 0) -> `root root`
/// 2. Account materialized via useradd -> name
/// 3. Account created by a replicated RPM -> name + package comment
///    (uses KNOWN_PACKAGE_ACCOUNTS mapping for package!=account cases,
///    plus direct package-name==account-name match as fallback)
/// 4. Name available but not guaranteed -> numeric UID:GID + warning comment
/// 5. Name unavailable -> numeric UID:GID
fn resolve_tmpfiles_ownership(
    d: &inspectah_core::types::storage::VarDirectory,
    materialized_users: &std::collections::HashSet<String>,
    rpm_accounts: &std::collections::HashMap<String, String>,
    replicated_rpms: &std::collections::HashSet<String>,
) -> (ExportToken, ExportToken, String) {
    let uid = d.owner_uid.unwrap_or(0);
    let gid = d.owner_gid.unwrap_or(0);

    // Case 1: root
    if uid == 0 {
        return (
            known_token(DEFAULT_VAR_DIR_OWNER),
            known_token(DEFAULT_VAR_DIR_OWNER),
            String::new(),
        );
    }

    // The two names come off the scanned host and reach both a
    // tmpfiles.d field and a comment, and nothing upstream checks
    // either. A name the allowlist refuses is treated as a name the
    // scan never captured, which routes it to the numeric fallback the
    // later cases already use: the same ownership, written the way this
    // function already writes it when `stat` reported no name.
    let owner_name = proved_account_name(d.owner_name.as_deref());
    let group_name = proved_account_name(d.group_name.as_deref());

    // Case 2: materialized user (being created by useradd in this Containerfile)
    if !owner_name.is_empty() && materialized_users.contains(owner_name) {
        let group = if !group_name.is_empty() {
            known_token(group_name)
        } else {
            numeric_token(gid)
        };
        return (known_token(owner_name), group, String::new());
    }

    // Case 3: RPM-provided user (package scriptlets create the account)
    // Check the package-account mapping first (handles postgres->postgresql-server etc.),
    // then fall back to direct package-name==account-name match (handles redis->redis etc.)
    if !owner_name.is_empty() {
        let providing_package = rpm_accounts
            .get(owner_name)
            .map(|s| s.as_str())
            .or_else(|| {
                if replicated_rpms.contains(owner_name) {
                    Some(owner_name)
                } else {
                    None
                }
            });

        if let Some(pkg) = providing_package {
            let group = if !group_name.is_empty() {
                known_token(group_name)
            } else {
                known_token(owner_name)
            };
            return (
                known_token(owner_name),
                group,
                format!("created by package: {pkg}"),
            );
        }
    }

    // Case 4: name known but not guaranteed in the image
    if !owner_name.is_empty() {
        return (
            numeric_token(uid),
            numeric_token(gid),
            format!("account '{owner_name}' not guaranteed in image"),
        );
    }

    // Case 5: no name available
    (numeric_token(uid), numeric_token(gid), String::new())
}

/// An account name the export may write, or `""` when the scan captured
/// none and when the one it captured is not a name this will write.
///
/// A refusal is deliberately indistinguishable from an absence here:
/// both mean the renderer has no usable name and both route to the same
/// numeric fallback, so neither needs a branch of its own.
fn proved_account_name(name: Option<&str>) -> &str {
    match name {
        Some(name) if ExportToken::parse(name).is_ok() => name,
        _ => "",
    }
}

/// A field this module produced rather than read off a host.
///
/// Module constants and decimal ids are allowlist-clean by construction,
/// so this is the one place an `ExportToken` is built without a caller
/// handling a refusal.
fn known_token(value: &str) -> ExportToken {
    ExportToken::parse(value).expect("a module constant or a proved name passes the allowlist")
}

/// A numeric uid or gid as a tmpfiles.d field.
fn numeric_token(id: u32) -> ExportToken {
    ExportToken::parse(&id.to_string()).expect("decimal digits pass the allowlist")
}

/// Mode and ownership for the commented `#d` line of an `Unresolved`
/// directory whose own mode the scan did not capture. The line is inert
/// until the user uncomments it, and these are tmpfiles.d's own
/// defaults for a directory.
const DEFAULT_VAR_DIR_MODE: &str = "0755";
const DEFAULT_VAR_DIR_OWNER: &str = "root";

/// The comment that precedes a provisioned directory whose source-host
/// creator this export does not carry.
/// `creator` is host-sourced in every case the caller can reach: a
/// tmpfiles.d fragment path, a unit filename, or a package name, each an
/// unrestricted `String` inside `VarDirBacking`. This comment goes into
/// both `inspectah-var.conf` and the Containerfile, and on both surfaces
/// a newline ends the comment and promotes the rest to a directive, so
/// it is escaped rather than refused: a comment that only labels cannot
/// execute once its newline is gone, and refusing would cost the
/// operator the reason for a line they are being asked to act on.
fn lost_creator_comment(path: &ExportPath, creator: &str) -> String {
    let path = path.as_str();
    let creator = super::language_packages::sanitize_for_comment(creator);
    format!("# {path}: created on the host by {creator}, which this export does not carry")
}

/// The `RUN` that makes a provisioned directory the scan captured no mode
/// for.
///
/// This is a shell-form `RUN`: `/bin/sh -c` runs it as root at image
/// build time and nothing quotes the operand. It takes an [`ExportPath`]
/// rather than a `&str` so a path that did not pass the allowlist cannot
/// reach it, which is the same guarantee `StagedName` gives the repo-less
/// `COPY`/`RUN` pair.
fn mkdir_line(path: &ExportPath) -> String {
    format!("RUN mkdir -p {}", path.as_str())
}

/// The active tmpfiles.d `d` entry for a provisioned directory.
///
/// Every operand is a proved type for the same reason: tmpfiles.d
/// records are newline-separated and their fields whitespace-separated,
/// so an unproved value in *any* of the four ends the record and starts
/// a directive of its own that systemd applies at boot. Proving only the
/// path would have left the other three carrying the defect the path was
/// fixed for.
fn tmpfiles_dir_line(
    path: &ExportPath,
    mode: &ExportToken,
    owner: &ExportToken,
    group: &ExportToken,
) -> String {
    format!(
        "d {} {} {} {} -",
        path.as_str(),
        mode.as_str(),
        owner.as_str(),
        group.as_str()
    )
}

/// Why a directory got an instruction instead of a build step.
const REFUSED_PATH_REASON: &str =
    "its path has characters this export will not write into a build instruction";
const REFUSED_MODE_REASON: &str =
    "its recorded mode is not one this export will write into a tmpfiles.d entry";

/// The line a directory renders in place of its `RUN` or its `d` entry
/// when one of its own fields is refused.
///
/// Rejection is rejection: there is no sanitized form of a path that
/// names the directory the host had, and no stand-in for a mode that is
/// still the host's mode, so the export hands the operator an
/// instruction instead of writing a build step it cannot vouch for. The
/// path is escaped rather than refused here because this line is a
/// comment, and naming the directory is the whole point of it.
fn refused_field_comment(path: &str, reason: &str) -> String {
    format!(
        "# FIXME: create {} by hand; {reason}",
        super::language_packages::sanitize_for_comment(path)
    )
}

/// The two-line FIXME an `Unresolved` directory renders: an imperative
/// instruction naming the cause (§ 5.6), then the `d` line to uncomment.
///
/// Both `inspectah-var.conf` and the Containerfile's Storage block carry
/// it, so the wording is produced in one place.
///
/// `path` and the three fields beside it are proved, not escaped: the
/// commented `d` line exists to be uncommented, so it has to be a line
/// that is safe once the `#` comes off. A refused mode takes the same
/// answer it takes on an active line, an instruction rather than a
/// substituted default, because a default mode here would be a guess
/// the operator could not tell from a reading. `d` is still needed for
/// the mode and ownership the line carries.
fn unresolved_fixme_lines(
    d: &inspectah_core::types::storage::VarDirectory,
    path: &ExportPath,
    creator: &str,
    reason: UnresolvedReason,
    materialized_users: &std::collections::HashSet<String>,
    rpm_accounts: &std::collections::HashMap<String, String>,
    replicated_rpms: &std::collections::HashSet<String>,
) -> Vec<String> {
    let (mode, owner, group) = match &d.mode {
        Some(mode) => {
            let Ok(mode) = ExportToken::parse(mode) else {
                return vec![refused_field_comment(&d.path, REFUSED_MODE_REASON)];
            };
            let (owner, group, _) =
                resolve_tmpfiles_ownership(d, materialized_users, rpm_accounts, replicated_rpms);
            (mode, owner, group)
        }
        None => (
            known_token(DEFAULT_VAR_DIR_MODE),
            known_token(DEFAULT_VAR_DIR_OWNER),
            known_token(DEFAULT_VAR_DIR_OWNER),
        ),
    };
    vec![
        format!(
            "# FIXME: confirm the base image creates {}; on the host {} created it and {}; \
             if the base does not, uncomment the next line",
            path.as_str(),
            super::language_packages::sanitize_for_comment(creator),
            reason.reason_text()
        ),
        format!("#{}", tmpfiles_dir_line(path, &mode, &owner, &group)),
    ]
}

/// Generate tmpfiles.d conf content for the /var directories this export
/// has to provision or cannot resolve.
///
/// `Provision` with mode data gets an active `d` line, preceded by the
/// lost-creator comment when the verdict named a creator the export
/// drops. `Unresolved` gets the two-line FIXME and no active line.
/// `Carried` gets nothing: a creator the export carries already makes
/// the directory, and a second `d` line would duplicate it.
///
/// Returns `None` when no directory produces a line. `Provision`
/// directories without mode data are skipped here and handled by
/// `var_dir_section_lines()` as `RUN mkdir -p` fallback, so a single
/// snapshot can produce BOTH a tmpfiles.d conf AND `RUN mkdir` lines.
pub(crate) fn render_tmpfiles_conf(snap: &InspectionSnapshot) -> Option<String> {
    let storage = snap.storage.as_ref()?;
    let materialized_users = collect_materialized_usernames(snap);
    let (rpm_accounts, replicated_rpms) = collect_rpm_provided_accounts(snap);
    let mut lines = Vec::new();

    for (d, outcome) in storage
        .var_directories
        .iter()
        .zip(effective_var_backing(snap))
    {
        debug_assert_eq!(d.path, outcome.path);
        match &outcome.outcome {
            EffectiveBacking::Carried => {}
            EffectiveBacking::Provision { lost } => {
                // Per-directory: skip directories missing mode data.
                // These get RUN mkdir fallback in var_dir_section_lines().
                let Some(mode) = d.mode.as_deref() else {
                    continue;
                };
                let Ok(path) = ExportPath::parse(&d.path) else {
                    lines.push(refused_field_comment(&d.path, REFUSED_PATH_REASON));
                    continue;
                };
                let Ok(mode) = ExportToken::parse(mode) else {
                    lines.push(refused_field_comment(&d.path, REFUSED_MODE_REASON));
                    continue;
                };
                if let Some(creator) = lost {
                    lines.push(lost_creator_comment(&path, creator));
                }
                let (owner, group, comment) = resolve_tmpfiles_ownership(
                    d,
                    &materialized_users,
                    &rpm_accounts,
                    &replicated_rpms,
                );
                if !comment.is_empty() {
                    lines.push(format!("# {comment}"));
                }
                lines.push(tmpfiles_dir_line(&path, &mode, &owner, &group));
            }
            EffectiveBacking::Unresolved { creator, reason } => {
                let Ok(path) = ExportPath::parse(&d.path) else {
                    lines.push(refused_field_comment(&d.path, REFUSED_PATH_REASON));
                    continue;
                };
                lines.extend(unresolved_fixme_lines(
                    d,
                    &path,
                    creator,
                    *reason,
                    &materialized_users,
                    &rpm_accounts,
                    &replicated_rpms,
                ));
            }
        }
    }

    if lines.is_empty() {
        return None;
    }

    Some(super::comment::join_lines(&lines) + "\n")
}

fn var_dir_section_lines(snap: &InspectionSnapshot, tmpfiles_staged: bool) -> Vec<String> {
    let storage = match &snap.storage {
        Some(s) => s,
        None => return Vec::new(),
    };

    let materialized_users = collect_materialized_usernames(snap);
    let (rpm_accounts, replicated_rpms) = collect_rpm_provided_accounts(snap);
    let mut body = Vec::new();

    if tmpfiles_staged {
        body.push("# /var directories with known ownership/mode provisioned via tmpfiles.d".into());
        body.push(
            "COPY config/usr/lib/tmpfiles.d/inspectah-var.conf /usr/lib/tmpfiles.d/inspectah-var.conf"
                .into(),
        );
    }

    for (d, outcome) in storage
        .var_directories
        .iter()
        .zip(effective_var_backing(snap))
    {
        debug_assert_eq!(d.path, outcome.path);
        match &outcome.outcome {
            EffectiveBacking::Carried => {}
            // Fallback: RUN mkdir for provisioned dirs WITHOUT
            // ownership/mode data. The rest are in the tmpfiles.d conf.
            EffectiveBacking::Provision { lost } if d.mode.is_none() => {
                let Ok(path) = ExportPath::parse(&d.path) else {
                    body.push(refused_field_comment(&d.path, REFUSED_PATH_REASON));
                    continue;
                };
                if let Some(creator) = lost {
                    body.push(lost_creator_comment(&path, creator));
                }
                body.push(mkdir_line(&path));
            }
            EffectiveBacking::Provision { .. } => {}
            EffectiveBacking::Unresolved { creator, reason } => {
                let Ok(path) = ExportPath::parse(&d.path) else {
                    body.push(refused_field_comment(&d.path, REFUSED_PATH_REASON));
                    continue;
                };
                body.extend(unresolved_fixme_lines(
                    d,
                    &path,
                    creator,
                    *reason,
                    &materialized_users,
                    &rpm_accounts,
                    &replicated_rpms,
                ));
            }
        }
    }

    section("/var directory provisioning", body)
}

// --- Epilogue ---

fn tmpfiles_lines() -> Vec<String> {
    // Placeholder — no content to emit yet. When systemd-tmpfiles
    // directives are captured by the inspector, content will be
    // rendered here conditionally. Until then, emit nothing.
    Vec::new()
}

fn validate_lines() -> Vec<String> {
    section(
        "Validate bootc compatibility",
        vec!["RUN bootc container lint".into()],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::FindingKind;
    use inspectah_core::types::completeness::{Completeness, InspectorId};
    use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};

    fn snapshot_with_packages(names: &[&str]) -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: names
                .iter()
                .map(|n| PackageEntry {
                    name: n.to_string(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    source_repo: "appstream".into(),
                    disposition: FindingKind::included(),
                    locked: false,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        });
        snap
    }

    // --- Repo-enabling fixtures (Task 17) -------------------------------

    const EPEL_REPO_PATH: &str = "/etc/yum.repos.d/epel.repo";
    const EPEL_KEY_PATH: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9";
    const EPEL_REPO_CONTENT: &str = "[epel]\n\
         name=EPEL\n\
         baseurl=https://example.invalid/epel\n\
         enabled=1\n\
         gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9\n";

    fn repo_file(
        path: &str,
        content: &str,
        included: bool,
    ) -> inspectah_core::types::rpm::RepoFile {
        inspectah_core::types::rpm::RepoFile {
            path: path.into(),
            content: content.into(),
            is_default_repo: false,
            disposition: FindingKind::from_bool(included),
            locked: false,
            aggregate: None,
        }
    }

    fn owns(package: &str, paths: &[&str]) -> inspectah_core::types::rpm::FileOwnershipEntry {
        inspectah_core::types::rpm::FileOwnershipEntry {
            package_name: package.into(),
            paths: paths.iter().map(|p| (*p).to_string()).collect(),
        }
    }

    /// Make `name` repo-enabling the way a scan records it: installed
    /// `@commandline`, served by `[epel]`, owning the `epel.repo` and key
    /// this export is carrying.
    fn make_epel_repo_enabling(snap: &mut InspectionSnapshot, name: &str) {
        let rpm = snap.rpm.as_mut().expect("fixture has an rpm section");
        for pkg in rpm.packages_added.iter_mut().filter(|p| p.name == name) {
            pkg.source_repo = "@commandline".into();
            pkg.repo_enabling_from = vec![inspectah_core::types::repo::RepoId::new("epel")];
        }
        rpm.repo_files
            .push(repo_file(EPEL_REPO_PATH, EPEL_REPO_CONTENT, true));
        rpm.gpg_keys.push(repo_file(EPEL_KEY_PATH, "", true));
        rpm.file_ownership
            .push(owns(name, &[EPEL_REPO_PATH, EPEL_KEY_PATH]));
    }

    /// Refine renders the projected snapshot, where `epel-release` has
    /// already been demoted and annotated. The annotation used to send it
    /// past the repo-enabling match, so the FIXME naming the file to
    /// re-include never rendered and only the MANUAL block remained.
    #[test]
    fn a_demoted_repo_enabling_package_keeps_its_fixme_after_projection() {
        let mut snap = snapshot_with_packages(&["epel-release", "htop"]);
        make_epel_repo_enabling(&mut snap, "epel-release");
        repo_file_mut(&mut snap, EPEL_REPO_PATH).disposition = FindingKind::excluded();
        super::super::repo_enabling::demote_unserved_repo_enabling(&mut snap);
        assert!(
            !snap
                .rpm
                .as_ref()
                .expect("fixture has an rpm section")
                .packages_added[0]
                .repoless_annotation
                .is_empty(),
            "the fixture must be the projected, annotated state"
        );

        let output = render_containerfile(&snap, None, None);

        assert!(
            output.contains(&format!(
                "# FIXME: include {EPEL_REPO_PATH} under Configuration Files, or re-include \
                 repo epel, or upload the RPM; epel-release installs from that repo \
                 definition, which this export does not carry"
            )),
            "{output}"
        );
        assert!(output.contains("# MANUAL: epel-release"), "{output}");
        assert!(!output.contains("    epel-release \\"), "{output}");
    }

    /// A demoted repo-enabling leaf the operator uploaded installs from the
    /// upload, so dnf resolves its dependencies: an excluded dependency
    /// from a host-disabled repo is still named in that repo's
    /// transaction and keeps its `systemctl enable`. Without the upload
    /// the leaf is a FIXME only, which is the control: the dependency is
    /// not named, is not carried, and its unit is omitted as such.
    #[test]
    fn an_uploaded_demoted_leaf_still_pulls_in_its_dependencies() {
        const UPLOAD: &str = "epel-release-9-1.el9.x86_64.rpm";
        const ENABLEREPO: &str = "RUN dnf install -y --enablerepo=epel-cisco-openh264 \\";

        let host = |uploaded: bool| {
            let mut snap = snapshot_with_packages(&["epel-release", "openh264"]);
            make_epel_repo_enabling(&mut snap, "epel-release");
            repo_file_mut(&mut snap, EPEL_REPO_PATH).disposition = FindingKind::excluded();
            {
                let rpm = snap.rpm.as_mut().expect("fixture has an rpm section");
                rpm.repo_files
                    .push(repo_file(DISABLED_REPO_PATH, DISABLED_REPO_CONTENT, true));
                rpm.packages_added[0].uploaded = uploaded;
                rpm.packages_added[1].disposition = FindingKind::excluded();
                rpm.baseline_package_names = Some(vec!["bash".into()]);
                rpm.leaf_packages = Some(vec!["epel-release.x86_64".into()]);
                rpm.auto_packages = Some(vec!["openh264.x86_64".into()]);
                rpm.leaf_dep_tree =
                    serde_json::json!({ "epel-release.x86_64": ["openh264.x86_64"] });
            }
            make_disabled_repo_package(&mut snap, "openh264", DISABLED_REPO_ID);
            super::super::repo_enabling::demote_unserved_repo_enabling(&mut snap);
            snap.services = Some(inspectah_core::types::services::ServiceSection {
                state_changes: vec![inspectah_core::types::services::ServiceStateChange {
                    unit: "openh264.service".into(),
                    current_state: inspectah_core::types::services::ServiceUnitState::Enabled,
                    default_state: Some(inspectah_core::types::services::PresetDefault::Disable),
                    disposition: FindingKind::included(),
                    locked: false,
                    owning_package: Some("openh264".into()),
                    aggregate: None,
                    attention_reason: None,
                    shadow_type: None,
                    shadow_rationale: None,
                }],
                enabled_units: vec![],
                disabled_units: vec![],
                drop_ins: vec![],
                preset_matched_units: vec![],
            });
            snap
        };

        // Uploaded: the leaf installs from the upload.
        let mut ctx = RenderContext::default();
        ctx.verified_uploads.insert(
            "epel-release.x86_64".into(),
            inspectah_core::types::verified_upload::VerifiedUpload {
                canonical: "epel-release.x86_64".into(),
                basename: inspectah_core::rpm_filename::StagedName::parse(UPLOAD)
                    .expect("a valid basename"),
                uploaded_version: None,
            },
        );
        let output = render_containerfile(&host(true), None, Some(&ctx));
        assert!(
            output.contains(&format!("    /tmp/{UPLOAD} \\")),
            "the leaf installs from its upload:\n{output}"
        );
        assert!(output.contains(ENABLEREPO), "{output}");
        assert!(
            output.lines().any(|l| l == "    openh264 \\"),
            "the dependency is named in its repo's transaction:\n{output}"
        );
        assert!(
            output
                .lines()
                .any(|l| !l.starts_with('#') && l.contains("openh264.service")),
            "the dependency keeps its unit:\n{output}"
        );
        assert!(!output.contains("# Omitted: openh264.service"), "{output}");

        // The control: no upload, so the leaf is a FIXME only.
        let output = render_containerfile(&host(false), None, Some(&RenderContext::default()));
        assert!(output.contains("# FIXME: include"), "{output}");
        assert!(!output.contains(ENABLEREPO), "{output}");
        assert!(!output.lines().any(|l| l == "    openh264 \\"), "{output}");
        assert!(
            output.contains("# Omitted: openh264.service (package 'openh264' is not carried"),
            "{output}"
        );
    }

    /// The other half of the double render: a `@commandline` package the
    /// scan left repo-less got an active `dnf install` from `source_repo`
    /// alone, next to its own MANUAL block. `@commandline` is not a repo,
    /// and nothing proves one serves this package.
    #[test]
    fn a_repoless_commandline_package_is_never_an_active_install() {
        let mut snap = snapshot_with_packages(&["vendor-release", "htop"]);
        for pkg in snap
            .rpm
            .as_mut()
            .expect("fixture has an rpm section")
            .packages_added
            .iter_mut()
            .filter(|p| p.name == "vendor-release")
        {
            pkg.source_repo = "@commandline".into();
            pkg.repoless_annotation = "No repo source \u{2014} manual resolution needed".into();
        }
        let output = render_containerfile(&snap, None, None);

        assert!(
            output.contains("# MANUAL: vendor-release"),
            "the repo-less block is where it belongs:\n{output}"
        );
        assert!(
            !output.contains("    vendor-release \\"),
            "and it must not also be installed by name:\n{output}"
        );
        assert!(
            output.contains("    htop \\"),
            "the control: an ordinary package still installs:\n{output}"
        );
    }

    const DISABLED_REPO_ID: &str = "epel-cisco-openh264";
    const DISABLED_REPO_PATH: &str = "/etc/yum.repos.d/fedora-cisco-openh264.repo";
    const DISABLED_REPO_CONTENT: &str = "[epel-cisco-openh264]\n\
         name=openh264\n\
         metalink=https://example.invalid/metalink\n\
         enabled=0\n";

    /// Mark `name` as the scan records a package from a repo the host
    /// keeps disabled: repo-less, with that repo as its `source_repo`.
    fn make_disabled_repo_package(snap: &mut InspectionSnapshot, name: &str, source_repo: &str) {
        let rpm = snap.rpm.as_mut().expect("fixture has an rpm section");
        for pkg in rpm.packages_added.iter_mut().filter(|p| p.name == name) {
            pkg.source_repo = source_repo.into();
            pkg.repoless_annotation = format!(
                "No repo source: repo '{source_repo}' not in enabled repos, manual resolution needed"
            );
        }
    }

    /// A package installed from a repo the source host has disabled
    /// renders once: from that repo with `--enablerepo` while the export
    /// carries the repo's definition, and in the repo-less block
    /// otherwise. Never in the plain package list, where it fails the
    /// build with `Unable to find a match`. It is selected the way the
    /// plain list selects, except that a dependency of an included package
    /// is named whatever its disposition, since the dependent's transaction
    /// cannot reach the repo. Group membership does not suppress it.
    #[test]
    fn a_disabled_repo_package_renders_once() {
        const CRB_CONTENT: &str = "[codeready-builder-for-rhel-9-aarch64-rpms]\n\
             baseurl=https://example.invalid/crb\nenabled=1\n";
        const ENABLEREPO: &str = "--enablerepo=epel-cisco-openh264";
        const INSTALL_LINE: &str = "    openh264 \\";
        const GROUP: &str = "Multimedia";
        const KEY_PATH: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-cisco";
        const KEYED: &str = "[epel-cisco-openh264]\n\
             metalink=https://example.invalid/metalink\n\
             enabled=0\n\
             gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-cisco\n";

        #[derive(Clone, Copy)]
        enum Want {
            /// One install, carrying the flag; no repo-less block.
            FromRepo,
            /// The repo-less block; no flag, no install by name. `todo`
            /// says whether the manual follow-up names it too.
            Repoless { todo: bool },
            /// Nowhere: no install, no flag, no repo-less block.
            Nothing,
        }
        #[derive(Clone, Copy)]
        enum Leaf {
            /// The snapshot carries no leaf data.
            Unknown,
            /// A package nothing else depends on.
            TopLevel,
            /// A dependency of the included leaf `htop`.
            Dependency,
            /// A dependency of an excluded leaf only.
            DependencyOfExcluded,
        }
        struct Case {
            name: &'static str,
            source_repo: &'static str,
            repo_file: Option<(&'static str, bool)>,
            crb: bool,
            included: bool,
            state: PackageState,
            leaf: Leaf,
            group_member: bool,
            /// The section names a `gpgkey=file://` key: `Some(included)`.
            key: Option<bool>,
            want: Want,
        }
        const BASE: Case = Case {
            name: "disabled repo shipped, package included",
            source_repo: DISABLED_REPO_ID,
            repo_file: Some((DISABLED_REPO_CONTENT, true)),
            crb: false,
            included: true,
            state: PackageState::Added,
            leaf: Leaf::Unknown,
            group_member: false,
            key: None,
            want: Want::FromRepo,
        };
        let cases = [
            BASE,
            Case {
                name: "disabled repo shipped alongside CRB",
                crb: true,
                ..BASE
            },
            Case {
                name: "disabled repo not captured",
                repo_file: None,
                want: Want::Repoless { todo: false },
                ..BASE
            },
            Case {
                name: "disabled repo excluded in refine",
                repo_file: Some((DISABLED_REPO_CONTENT, false)),
                want: Want::Repoless { todo: false },
                ..BASE
            },
            Case {
                name: "package excluded in refine",
                included: false,
                want: Want::Nothing,
                ..BASE
            },
            Case {
                name: "@commandline package is unchanged",
                source_repo: "@commandline",
                want: Want::Repoless { todo: true },
                ..BASE
            },
            Case {
                name: "a no-repo state is not installable by name, whatever its repo",
                state: PackageState::NoRepo,
                want: Want::Repoless { todo: true },
                ..BASE
            },
            Case {
                name: "a leaf installs from its repo",
                leaf: Leaf::TopLevel,
                ..BASE
            },
            Case {
                name: "a non-leaf dependency of an included package installs from its repo",
                leaf: Leaf::Dependency,
                ..BASE
            },
            Case {
                name: "the same dependency, excluded, still installs: it is pulled in",
                leaf: Leaf::Dependency,
                included: false,
                ..BASE
            },
            Case {
                name: "a dependency of an excluded leaf only is not named",
                leaf: Leaf::DependencyOfExcluded,
                want: Want::Nothing,
                ..BASE
            },
            Case {
                name: "a group member still installs: the group transaction lacks the repo",
                group_member: true,
                ..BASE
            },
            Case {
                name: "the section's GPG key ships",
                repo_file: Some((KEYED, true)),
                key: Some(true),
                ..BASE
            },
            Case {
                name: "the section's GPG key is excluded",
                repo_file: Some((KEYED, true)),
                key: Some(false),
                want: Want::Repoless { todo: false },
                ..BASE
            },
            Case {
                name: "the section's GPG key was never captured",
                repo_file: Some((KEYED, true)),
                want: Want::Repoless { todo: false },
                ..BASE
            },
        ];

        for case in &cases {
            let mut snap = snapshot_with_packages(&["openh264", "htop"]);
            make_disabled_repo_package(&mut snap, "openh264", case.source_repo);
            let mut ctx = RenderContext::default();
            {
                let rpm = snap.rpm.as_mut().expect("fixture has an rpm section");
                let pkg = rpm
                    .packages_added
                    .iter_mut()
                    .find(|p| p.name == "openh264")
                    .expect("fixture has openh264");
                pkg.disposition = FindingKind::from_bool(case.included);
                pkg.state = case.state.clone();
                if let Some((content, included)) = case.repo_file {
                    rpm.repo_files
                        .push(repo_file(DISABLED_REPO_PATH, content, included));
                }
                if case.crb {
                    rpm.repo_files
                        .push(repo_file("/etc/yum.repos.d/crb.repo", CRB_CONTENT, true));
                }
                if let Some(included) = case.key {
                    rpm.gpg_keys.push(repo_file(KEY_PATH, "", included));
                }
                match case.leaf {
                    Leaf::Unknown => {}
                    Leaf::TopLevel => {
                        rpm.leaf_packages =
                            Some(vec!["openh264.x86_64".into(), "htop.x86_64".into()]);
                    }
                    Leaf::Dependency => {
                        rpm.leaf_packages = Some(vec!["htop.x86_64".into()]);
                        rpm.leaf_dep_tree =
                            serde_json::json!({ "htop.x86_64": ["openh264.x86_64"] });
                    }
                    Leaf::DependencyOfExcluded => {
                        let mut parent = rpm.packages_added[1].clone();
                        parent.name = "vlc".into();
                        parent.disposition = FindingKind::excluded();
                        rpm.packages_added.push(parent);
                        rpm.leaf_packages = Some(vec!["htop.x86_64".into(), "vlc.x86_64".into()]);
                        rpm.leaf_dep_tree =
                            serde_json::json!({ "vlc.x86_64": ["openh264.x86_64"] });
                    }
                }
                if case.group_member {
                    rpm.installed_groups = Some(vec![inspectah_core::types::rpm::InstalledGroup {
                        name: GROUP.into(),
                        members: vec!["openh264".into()],
                        optional_installed: vec![],
                    }]);
                    ctx.group_states
                        .insert(GROUP.into(), GroupRenderState::Renderable);
                }
            }
            let output = render_containerfile(&snap, None, Some(&ctx));
            let installs = output.lines().filter(|l| *l == INSTALL_LINE).count();
            let todo = output.contains("# TODO: 'openh264");

            match case.want {
                Want::FromRepo => {
                    assert_eq!(installs, 1, "{}: one install:\n{output}", case.name);
                    let flagged: Vec<&str> = output
                        .lines()
                        .filter(|l| l.starts_with("RUN dnf install") && l.contains(ENABLEREPO))
                        .collect();
                    assert_eq!(
                        flagged.len(),
                        1,
                        "{}: one flagged RUN:\n{output}",
                        case.name
                    );
                    assert_eq!(
                        flagged[0].contains("--enablerepo=codeready-builder-"),
                        case.crb,
                        "{}: CRB flags ride along exactly when CRB ships:\n{output}",
                        case.name
                    );
                    assert!(
                        !output.contains("# MANUAL: openh264")
                            && !output.contains("Repo-less RPM packages"),
                        "{}: not in the repo-less block:\n{output}",
                        case.name
                    );
                    assert!(!todo, "{}: no manual follow-up:\n{output}", case.name);
                }
                Want::Repoless { todo: want_todo } => {
                    assert_eq!(installs, 0, "{}: no install by name:\n{output}", case.name);
                    assert!(
                        !output.contains("--enablerepo"),
                        "{}: no flag:\n{output}",
                        case.name
                    );
                    assert!(
                        output.contains("# MANUAL: openh264"),
                        "{}: the repo-less block keeps it:\n{output}",
                        case.name
                    );
                    assert_eq!(
                        todo, want_todo,
                        "{}: manual follow-up:\n{output}",
                        case.name
                    );
                }
                Want::Nothing => {
                    assert_eq!(installs, 0, "{}: no install:\n{output}", case.name);
                    assert!(
                        !output.contains("--enablerepo"),
                        "{}: no flag:\n{output}",
                        case.name
                    );
                    assert!(
                        !output.contains("openh264"),
                        "{}: named nowhere:\n{output}",
                        case.name
                    );
                }
            }
            assert!(
                output.contains("    htop \\"),
                "{}: the control, an ordinary package, still installs:\n{output}",
                case.name
            );
        }
    }

    /// The package transactions run repo-enabling first, then the combined
    /// install of the disabled-repo packages and local RPMs, then the group
    /// install, then the plain list. A group member or plain package may
    /// depend on a disabled-repo package, and neither of those
    /// transactions enables its repo.
    #[test]
    fn disabled_repo_transactions_run_before_the_group_and_plain_ones() {
        const GROUP: &str = "Multimedia";
        let mut snap = snapshot_with_packages(&["epel-release", "openh264", "htop"]);
        make_epel_repo_enabling(&mut snap, "epel-release");
        make_disabled_repo_package(&mut snap, "openh264", DISABLED_REPO_ID);
        {
            let rpm = snap.rpm.as_mut().expect("fixture has an rpm section");
            rpm.repo_files
                .push(repo_file(DISABLED_REPO_PATH, DISABLED_REPO_CONTENT, true));
            rpm.installed_groups = Some(vec![inspectah_core::types::rpm::InstalledGroup {
                name: GROUP.into(),
                members: vec!["gstreamer1".into()],
                optional_installed: vec![],
            }]);
        }
        let mut ctx = RenderContext::default();
        ctx.group_states
            .insert(GROUP.into(), GroupRenderState::Renderable);

        let output = render_containerfile(&snap, None, Some(&ctx));
        let at = |needle: &str| {
            output
                .find(needle)
                .unwrap_or_else(|| panic!("missing {needle:?}:\n{output}"))
        };
        let order = [
            at("# === Repo-enabling packages (1) ==="),
            at("# === Packages from repos disabled on the source host (1) ==="),
            at("# === Package Groups (1) ==="),
            at("# === Packages (1) ==="),
        ];
        assert!(
            order.windows(2).all(|w| w[0] < w[1]),
            "transaction order {order:?}:\n{output}"
        );
        assert_eq!(
            output
                .lines()
                .filter(|l| l.starts_with("RUN dnf install") && l.contains("--enablerepo="))
                .count(),
            1,
            "one transaction for the disabled repos:\n{output}"
        );
    }

    /// A leaf from one disabled repo depends on a package from another,
    /// and the repos sort the opposite way round. A transaction per repo in
    /// id order installed the leaf first, with its dependency's repo still
    /// disabled. One transaction enabling both resolves them together.
    #[test]
    fn disabled_repos_install_together_whatever_their_dependency_order() {
        const APP_REPO: &str = "/etc/yum.repos.d/a-app.repo";
        const LIB_REPO: &str = "/etc/yum.repos.d/z-lib.repo";
        let mut snap = snapshot_with_packages(&["vendor-app", "vendor-lib", "htop"]);
        make_disabled_repo_package(&mut snap, "vendor-app", "a-app");
        make_disabled_repo_package(&mut snap, "vendor-lib", "z-lib");
        {
            let rpm = snap.rpm.as_mut().expect("fixture has an rpm section");
            rpm.repo_files.push(repo_file(
                APP_REPO,
                "[a-app]\nbaseurl=https://example.invalid/app\nenabled=0\n",
                true,
            ));
            rpm.repo_files.push(repo_file(
                LIB_REPO,
                "[z-lib]\nbaseurl=https://example.invalid/lib\nenabled=0\n",
                true,
            ));
            rpm.leaf_packages = Some(vec!["vendor-app.x86_64".into(), "htop.x86_64".into()]);
            rpm.leaf_dep_tree = serde_json::json!({ "vendor-app.x86_64": ["vendor-lib.x86_64"] });
        }

        let output = render_containerfile(&snap, None, None);
        let flagged: Vec<&str> = output
            .lines()
            .filter(|l| l.starts_with("RUN dnf install") && l.contains("--enablerepo="))
            .collect();
        assert_eq!(
            flagged,
            vec!["RUN dnf install -y --enablerepo=a-app --enablerepo=z-lib \\"],
            "one transaction enabling both repos:\n{output}"
        );
        let body = &output[output.find(flagged[0]).expect("the flagged transaction")..];
        let body = &body[..body.find("&& dnf clean all").expect("the tail")];
        assert!(
            body.contains("    vendor-app \\") && body.contains("    vendor-lib \\"),
            "both packages in it:\n{output}"
        );
        assert!(
            output.contains("Packages from repos disabled on the source host (2)"),
            "{output}"
        );
    }

    /// Two disabled repos and one upload, in each dependency direction.
    /// `vendor-app` comes from `a-app` and depends on `vendor-lib` from
    /// `z-lib`; one of the two is uploaded. Whichever it is, the upload and
    /// the disabled-repo package install in the same transaction, so dnf
    /// resolves the dependency from the file or from the enabled repo.
    #[test]
    fn an_upload_and_a_disabled_repo_package_install_together_either_way() {
        struct Case {
            uploaded: &'static str,
            from_repo: &'static str,
            repo: &'static str,
        }
        let cases = [
            Case {
                uploaded: "vendor-lib",
                from_repo: "vendor-app",
                repo: "a-app",
            },
            Case {
                uploaded: "vendor-app",
                from_repo: "vendor-lib",
                repo: "z-lib",
            },
        ];
        for case in &cases {
            let mut snap = snapshot_with_packages(&["vendor-app", "vendor-lib", "htop"]);
            make_disabled_repo_package(&mut snap, "vendor-app", "a-app");
            make_disabled_repo_package(&mut snap, "vendor-lib", "z-lib");
            {
                let rpm = snap.rpm.as_mut().expect("fixture has an rpm section");
                rpm.repo_files.push(repo_file(
                    "/etc/yum.repos.d/a-app.repo",
                    "[a-app]\nbaseurl=https://example.invalid/app\nenabled=0\n",
                    true,
                ));
                rpm.repo_files.push(repo_file(
                    "/etc/yum.repos.d/z-lib.repo",
                    "[z-lib]\nbaseurl=https://example.invalid/lib\nenabled=0\n",
                    true,
                ));
                rpm.leaf_packages = Some(vec!["vendor-app.x86_64".into(), "htop.x86_64".into()]);
                rpm.leaf_dep_tree =
                    serde_json::json!({ "vendor-app.x86_64": ["vendor-lib.x86_64"] });
            }
            let upload = format!("{}-2-1.el9.x86_64.rpm", case.uploaded);
            let mut ctx = RenderContext::default();
            ctx.verified_uploads.insert(
                format!("{}.x86_64", case.uploaded),
                inspectah_core::types::verified_upload::VerifiedUpload {
                    canonical: format!("{}.x86_64", case.uploaded),
                    basename: inspectah_core::rpm_filename::StagedName::parse(&upload)
                        .expect("a valid basename"),
                    uploaded_version: None,
                },
            );

            let output = render_containerfile(&snap, None, Some(&ctx));
            let runs: Vec<&str> = output
                .lines()
                .filter(|l| {
                    l.starts_with("RUN dnf install") || l.starts_with("RUN dnf localinstall")
                })
                .collect();
            let expected_run = format!("RUN dnf install -y --enablerepo={} \\", case.repo);
            assert_eq!(
                runs,
                vec![expected_run.as_str(), "RUN dnf install -y \\"],
                "{}: one combined transaction, then the plain list:\n{output}",
                case.uploaded
            );
            let start = output.find(&expected_run).expect("checked above");
            let body = &output[start..];
            let body = &body[..body.find("&& dnf clean all").expect("the tail")];
            for operand in [
                format!("    {} \\", case.from_repo),
                format!("    /tmp/{upload} \\"),
            ] {
                assert!(
                    body.lines().any(|l| l == operand),
                    "{}: {operand:?} in the combined transaction:\n{output}",
                    case.uploaded
                );
            }
            assert_eq!(
                output
                    .matches(&format!("COPY repoless-packages/{upload} /tmp/"))
                    .count(),
                1,
                "{}: one COPY:\n{output}",
                case.uploaded
            );
            assert!(
                output.find(&format!("COPY repoless-packages/{upload}")) < Some(start),
                "{}: the COPY precedes the install:\n{output}",
                case.uploaded
            );
        }
    }

    /// A cached repo-less RPM installs in the package phase, before the
    /// `systemctl enable` of a unit it owns.
    #[test]
    fn a_cached_repoless_rpm_installs_before_its_unit_is_enabled() {
        let mut snap = snapshot_with_packages(&["custom-tool", "htop"]);
        {
            let pkg = &mut snap.rpm.as_mut().expect("rpm").packages_added[0];
            pkg.source_repo = "@commandline".into();
            pkg.version = "1.0".into();
            pkg.release = "1.el9".into();
            pkg.repoless_cached = true;
            pkg.repoless_annotation = "No repo source \u{2014} cached RPM bundled".into();
        }
        snap.services = Some(inspectah_core::types::services::ServiceSection {
            state_changes: vec![inspectah_core::types::services::ServiceStateChange {
                unit: "custom-tool.service".into(),
                current_state: inspectah_core::types::services::ServiceUnitState::Enabled,
                default_state: Some(inspectah_core::types::services::PresetDefault::Disable),
                disposition: FindingKind::included(),
                locked: false,
                owning_package: Some("custom-tool".into()),
                aggregate: None,
                attention_reason: None,
                shadow_type: None,
                shadow_rationale: None,
            }],
            enabled_units: vec![],
            disabled_units: vec![],
            drop_ins: vec![],
            preset_matched_units: vec![],
        });

        let output = render_containerfile(&snap, None, None);
        let at = |needle: &str| {
            output
                .lines()
                .position(|l| l == needle)
                .unwrap_or_else(|| panic!("missing {needle:?}:\n{output}"))
        };
        let install = at("    /tmp/custom-tool-1.0-1.el9.x86_64.rpm \\");
        let enable = at("    custom-tool.service");
        assert!(install < enable, "{output}");
        assert_eq!(
            output
                .matches("COPY repoless-packages/custom-tool-1.0-1.el9.x86_64.rpm")
                .count(),
            1
        );
    }

    /// The case the multiarch naming fix was for: both arches are non-leaf
    /// disabled-repo dependencies of an included leaf. Naming came from
    /// counts over the plain list's selection, which leaves non-leaf
    /// packages out, so the pair fell back to a bare name and dedup
    /// collapsed it to one operand.
    #[test]
    fn a_non_leaf_multiarch_pair_from_a_disabled_repo_keeps_both_arches() {
        let mut snap = snapshot_with_packages(&["openh264", "htop"]);
        {
            let rpm = snap.rpm.as_mut().expect("fixture has an rpm section");
            let mut i686 = rpm.packages_added[0].clone();
            i686.arch = "i686".into();
            rpm.packages_added.push(i686);
            rpm.repo_files
                .push(repo_file(DISABLED_REPO_PATH, DISABLED_REPO_CONTENT, true));
            rpm.leaf_packages = Some(vec!["htop.x86_64".into()]);
            rpm.leaf_dep_tree =
                serde_json::json!({ "htop.x86_64": ["openh264.x86_64", "openh264.i686"] });
        }
        make_disabled_repo_package(&mut snap, "openh264", DISABLED_REPO_ID);

        let output = render_containerfile(&snap, None, None);

        assert!(
            output.contains("Packages from repos disabled on the source host (2)"),
            "{output}"
        );
        assert!(output.contains("    openh264.i686 \\"), "{output}");
        assert!(output.contains("    openh264.x86_64 \\"), "{output}");
        assert!(!output.contains("    openh264 \\"), "{output}");
    }

    /// An operator's upload wins over the disabled repo: the package renders
    /// once, from the uploaded RPM in the repo-less block, with no
    /// `--enablerepo` transaction. Without the upload the same snapshot
    /// installs from the repo, which is the control.
    #[test]
    fn a_verified_upload_wins_over_a_disabled_repo() {
        const UPLOAD: &str = "openh264-2.4.2-1.el9.x86_64.rpm";
        let mut snap = snapshot_with_packages(&["openh264", "htop"]);
        snap.rpm
            .as_mut()
            .expect("fixture has an rpm section")
            .repo_files
            .push(repo_file(DISABLED_REPO_PATH, DISABLED_REPO_CONTENT, true));
        make_disabled_repo_package(&mut snap, "openh264", DISABLED_REPO_ID);

        let without = render_containerfile(&snap, None, Some(&RenderContext::default()));
        assert!(
            without.contains("--enablerepo=epel-cisco-openh264"),
            "{without}"
        );

        let mut ctx = RenderContext::default();
        ctx.verified_uploads.insert(
            "openh264.x86_64".into(),
            inspectah_core::types::verified_upload::VerifiedUpload {
                canonical: "openh264.x86_64".into(),
                basename: inspectah_core::rpm_filename::StagedName::parse(UPLOAD)
                    .expect("a valid basename"),
                uploaded_version: None,
            },
        );
        let with = render_containerfile(&snap, None, Some(&ctx));

        assert!(!with.contains("--enablerepo"), "{with}");
        assert!(
            with.contains(&format!("COPY repoless-packages/{UPLOAD} /tmp/")),
            "{with}"
        );
        assert!(!with.contains("    openh264 \\"), "{with}");
    }

    /// Both arches of a multiarch pair from a disabled repo install, each
    /// by `name.arch` as the plain list names them, and the header counts
    /// what the transaction installs.
    #[test]
    fn a_disabled_repo_multiarch_pair_installs_both_arches() {
        let mut snap = snapshot_with_packages(&["openh264", "htop"]);
        {
            let rpm = snap.rpm.as_mut().expect("fixture has an rpm section");
            let mut i686 = rpm.packages_added[0].clone();
            i686.arch = "i686".into();
            rpm.packages_added.push(i686);
            rpm.repo_files
                .push(repo_file(DISABLED_REPO_PATH, DISABLED_REPO_CONTENT, true));
        }
        make_disabled_repo_package(&mut snap, "openh264", DISABLED_REPO_ID);

        let output = render_containerfile(&snap, None, None);

        assert!(
            output.contains("Packages from repos disabled on the source host (2)"),
            "{output}"
        );
        assert!(output.contains("    openh264.i686 \\"), "{output}");
        assert!(output.contains("    openh264.x86_64 \\"), "{output}");
        assert!(!output.contains("    openh264 \\"), "{output}");
    }

    #[test]
    fn repo_enabling_block_replaces_the_duplicate_manual_block() {
        let mut snap = snapshot_with_packages(&["epel-release", "htop"]);
        make_epel_repo_enabling(&mut snap, "epel-release");
        snap.os_release = Some(inspectah_core::types::os::OsRelease {
            version_id: "9.6".into(),
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);

        // § 5.2's block text, hand-transcribed, through the active install.
        let expected = r"# === Repo-enabling packages (1) ===
# epel-release was installed outside any repo on the source host; the host's
# enabled repos served it, and its repo files and key are copied above, so it
# installs from the copied definition. The build needs those repo endpoints
# reachable with any credentials and CA trust they require. Installing after the
# COPY may leave a cosmetic epel.repo.rpmnew. Alternative, when
# dl.fedoraproject.org is reachable:
# RUN dnf install -y https://dl.fedoraproject.org/pub/epel/epel-release-latest-9.noarch.rpm
RUN dnf install -y \
    epel-release \";
        assert!(
            output.contains(expected),
            "§ 5.2 block text:\nexpected:\n{expected}\ngot:\n{output}"
        );

        // § 8.3's active lines, in order.
        let repos_at = output
            .find("COPY config/etc/yum.repos.d/ /etc/yum.repos.d/")
            .expect("the repo files are copied");
        let keys_at = output
            .find("COPY config/etc/pki/rpm-gpg/ /etc/pki/rpm-gpg/")
            .expect("the GPG keys are copied");
        let install_at = output.find(expected).expect("checked above");
        assert!(
            repos_at < keys_at && keys_at < install_at,
            "the COPYs must precede the install:\n{output}"
        );

        // The permalink is advice, never an instruction.
        for line in output
            .lines()
            .filter(|l| l.contains("dl.fedoraproject.org"))
        {
            assert!(
                line.starts_with('#'),
                "the permalink reached an active line: {line}"
            );
        }
        assert!(
            !output.contains("# MANUAL: epel-release"),
            "the package that used to render twice still does:\n{output}"
        );
        assert!(
            !output.contains("# TODO: 'epel-release"),
            "a repo-enabling package needs no manual follow-up:\n{output}"
        );
    }

    /// `vendor-release` owns `vendor.repo` but is served by `[bootstrap]`
    /// from a file it does not own -- the two-repository case, where
    /// losing either half is a demotion.
    fn two_repository_snapshot() -> InspectionSnapshot {
        let mut snap = snapshot_with_packages(&["vendor-release", "htop"]);
        let rpm = snap.rpm.as_mut().expect("fixture has an rpm section");
        for pkg in rpm
            .packages_added
            .iter_mut()
            .filter(|p| p.name == "vendor-release")
        {
            pkg.source_repo = "@commandline".into();
            pkg.repo_enabling_from = vec![inspectah_core::types::repo::RepoId::new("bootstrap")];
        }
        rpm.repo_files.push(repo_file(
            "/etc/yum.repos.d/vendor.repo",
            "[vendor]\nbaseurl=https://example.invalid/vendor\nenabled=1\n\
             gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-VENDOR\n",
            true,
        ));
        rpm.repo_files.push(repo_file(
            "/etc/yum.repos.d/bootstrap.repo",
            "[bootstrap]\nbaseurl=https://example.invalid/bootstrap\nenabled=1\n\
             gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-BOOTSTRAP\n",
            true,
        ));
        rpm.gpg_keys
            .push(repo_file("/etc/pki/rpm-gpg/RPM-GPG-KEY-VENDOR", "", true));
        rpm.gpg_keys.push(repo_file(
            "/etc/pki/rpm-gpg/RPM-GPG-KEY-BOOTSTRAP",
            "",
            true,
        ));
        rpm.file_ownership.push(owns(
            "vendor-release",
            &[
                "/etc/yum.repos.d/vendor.repo",
                "/etc/pki/rpm-gpg/RPM-GPG-KEY-VENDOR",
            ],
        ));
        snap
    }

    fn repo_file_mut<'a>(
        snap: &'a mut InspectionSnapshot,
        path: &str,
    ) -> &'a mut inspectah_core::types::rpm::RepoFile {
        snap.rpm
            .as_mut()
            .expect("fixture has an rpm section")
            .repo_files
            .iter_mut()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("fixture carries {path}"))
    }

    /// One way the export can lose a repo-enabling package's proof, and
    /// the section and file the FIXME must then name.
    struct DemotionCase {
        label: &'static str,
        mutate: fn(&mut InspectionSnapshot),
        id: &'static str,
        file: &'static str,
    }

    #[test]
    fn losing_the_serving_repo_demotes_the_package() {
        const ACTIVE_INSTALL: &str = "    vendor-release \\";
        const FIXME_TAIL: &str = "vendor-release installs from that repo definition, \
                                  which this export does not carry";

        // The control: everything carried, so the demotion half below can
        // fail. Re-including bootstrap.repo is this same state.
        let served = render_containerfile(&two_repository_snapshot(), None, None);
        assert!(
            served.contains(ACTIVE_INSTALL),
            "the carried case must install by name, or every case below is vacuous:\n{served}"
        );
        assert!(
            !served.contains("# FIXME:"),
            "the carried case must not warn:\n{served}"
        );

        // Each way the export can lose the proof, and the file the FIXME
        // names for it.
        let cases = [
            DemotionCase {
                label: "bootstrap.repo excluded",
                mutate: |s| {
                    repo_file_mut(s, "/etc/yum.repos.d/bootstrap.repo").disposition =
                        FindingKind::excluded();
                },
                id: "bootstrap",
                file: "/etc/yum.repos.d/bootstrap.repo",
            },
            DemotionCase {
                label: "[bootstrap] carried with enabled=0",
                mutate: |s| {
                    repo_file_mut(s, "/etc/yum.repos.d/bootstrap.repo").content =
                        "[bootstrap]\nbaseurl=https://example.invalid/bootstrap\nenabled=0\n"
                            .into();
                },
                id: "bootstrap",
                file: "/etc/yum.repos.d/bootstrap.repo",
            },
            DemotionCase {
                label: "[bootstrap]'s gpgkey excluded",
                mutate: |s| {
                    let rpm = s.rpm.as_mut().expect("fixture has an rpm section");
                    for key in rpm
                        .gpg_keys
                        .iter_mut()
                        .filter(|k| k.path.ends_with("RPM-GPG-KEY-BOOTSTRAP"))
                    {
                        key.disposition = FindingKind::excluded();
                    }
                },
                id: "bootstrap",
                file: "/etc/yum.repos.d/bootstrap.repo",
            },
            DemotionCase {
                label: "the vendor.repo the package owns is excluded",
                mutate: |s| {
                    repo_file_mut(s, "/etc/yum.repos.d/vendor.repo").disposition =
                        FindingKind::excluded();
                },
                id: "bootstrap",
                file: "/etc/yum.repos.d/vendor.repo",
            },
        ];

        for DemotionCase {
            label,
            mutate,
            id,
            file,
        } in cases
        {
            let mut snap = two_repository_snapshot();
            mutate(&mut snap);
            let output = render_containerfile(&snap, None, None);
            let expected = format!(
                "# FIXME: include {file} under Configuration Files, or re-include repo {id}, \
                 or upload the RPM; {FIXME_TAIL}"
            );
            assert!(
                output.contains(&expected),
                "{label}:\nexpected:\n{expected}\ngot:\n{output}"
            );
            assert!(
                !output.contains(ACTIVE_INSTALL),
                "{label}: the renderer kept an active install:\n{output}"
            );

            // The projection half: the annotation is what returns the
            // upload control, and what makes the repo-less block render.
            super::super::repo_enabling::demote_unserved_repo_enabling(&mut snap);
            let demoted = snap
                .rpm
                .as_ref()
                .expect("fixture has an rpm section")
                .packages_added
                .iter()
                .find(|p| p.name == "vendor-release")
                .expect("fixture has vendor-release");
            assert_eq!(
                demoted.repoless_annotation,
                format!(
                    "No repo source: the repo that served vendor-release ({id}) \
                     is not carried by this export"
                ),
                "{label}"
            );
            let after = render_containerfile(&snap, None, None);
            assert!(
                after.contains("# MANUAL: vendor-release"),
                "{label}: the demoted package must reach the repo-less block:\n{after}"
            );
            assert!(
                after.contains(&expected),
                "{label}: the annotation must not cost the FIXME:\n{after}"
            );
            assert!(
                !after.contains(ACTIVE_INSTALL),
                "{label}: still no active install after demotion:\n{after}"
            );
        }

        // A serving id no captured file defines: there is no file to name,
        // so the advice that names one is dropped rather than invented.
        let mut snap = two_repository_snapshot();
        for pkg in snap
            .rpm
            .as_mut()
            .expect("fixture has an rpm section")
            .packages_added
            .iter_mut()
            .filter(|p| p.name == "vendor-release")
        {
            pkg.repo_enabling_from = vec![inspectah_core::types::repo::RepoId::new("mirror")];
        }
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains(&format!(
                "# FIXME: re-include repo mirror, or upload the RPM; {FIXME_TAIL}"
            )),
            "an undefined serving id:\n{output}"
        );
        assert!(
            !output.contains("under Configuration Files"),
            "no file exists to name:\n{output}"
        );
    }

    #[test]
    fn crb_flags_every_dnf_transaction() {
        use inspectah_core::types::selinux::{SelinuxPortLabel, SelinuxSection};

        const CRB_MATCHED: &str = "codeready-builder-for-rhel-9-aarch64-rpms";
        const CRB_VERBATIM: &str = "codeready-builder-rhel9-internal";
        const CRB_REPO_PATH: &str = "/etc/yum.repos.d/crb.repo";
        // The last section is the input the `$(arch)` assertion needs: an
        // id carrying a command substitution reaches the filter and has
        // to be refused there, or "no line contains $(arch)" is satisfied
        // by the fixture rather than by the code.
        const CRB_CONTENT: &str = "[codeready-builder-for-rhel-9-aarch64-rpms]\nenabled=1\n\
             [codeready-builder-rhel9-internal]\nenabled=1\n\
             [crb-mirror-rhel9]\nenabled=1\n\
             [codeready-builder-for-rhel-9-aarch64-debug-rpms]\nenabled=1\n\
             [codeready-builder-for-rhel-9-aarch64-source-rpms]\nenabled=1\n\
             [codeready-builder-$(arch)-rpms]\nenabled=1\n";

        let mut snap = snapshot_with_packages(&["epel-release", "htop"]);
        make_epel_repo_enabling(&mut snap, "epel-release");
        {
            let rpm = snap.rpm.as_mut().expect("fixture has an rpm section");
            rpm.repo_files
                .push(repo_file(CRB_REPO_PATH, CRB_CONTENT, true));
            rpm.module_streams
                .push(inspectah_core::types::rpm::EnabledModuleStream {
                    module_name: "nodejs".into(),
                    stream: "18".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                });
            rpm.installed_groups = Some(vec![InstalledGroup {
                name: "Development Tools".into(),
                members: vec!["make".into()],
                ..Default::default()
            }]);
        }
        // An SELinux block, so § 3.1's precondition is in the output and
        // the exclusion below is reachable rather than vacuous.
        snap.selinux = Some(SelinuxSection {
            port_labels: vec![SelinuxPortLabel {
                protocol: "tcp".into(),
                port: "8443".into(),
                label_type: "http_port_t".into(),
                disposition: FindingKind::included(),
                locked: false,
                aggregate: None,
            }],
            ..Default::default()
        });
        let mut ctx = RenderContext::default();
        ctx.group_states
            .insert("Development Tools".into(), GroupRenderState::Renderable);
        let output = render_containerfile(&snap, None, Some(&ctx));

        // Identify the precondition by the package it installs, not by
        // equality with the const: a line that gained a flag stops being
        // equal to the const, which would take an equality-based filter
        // or guard out of the picture exactly when it should fire.
        const PRECONDITION_PACKAGE: &str = "policycoreutils-python-utils";
        let transactions: Vec<&str> = output
            .lines()
            .filter(|l| l.starts_with("RUN dnf ") && !l.contains(PRECONDITION_PACKAGE))
            .collect();
        assert_eq!(
            transactions.len(),
            4,
            "bootstrap, module stream, group and package list:\n{output}"
        );
        for line in &transactions {
            assert!(
                line.contains(&format!("--enablerepo={CRB_MATCHED}"))
                    && line.contains(&format!("--enablerepo={CRB_VERBATIM}")),
                "transaction missing a CRB flag: {line}"
            );
        }
        assert_eq!(
            output
                .matches("# CRB was enabled on the source host; EPEL packages may depend on it.")
                .count(),
            1,
            "the comment appears once:\n{output}"
        );
        // § 3.1's exclusion, in one assertion: the precondition renders,
        // and it renders verbatim -- so any flag it acquired shows up as
        // the difference rather than as an unsatisfiable predicate that
        // passes.
        let precondition_line = output
            .lines()
            .find(|l| l.starts_with("RUN dnf ") && l.contains(PRECONDITION_PACKAGE))
            .unwrap_or_else(|| {
                panic!("the precondition must render, or the exclusion is vacuous:\n{output}")
            });
        assert_eq!(
            precondition_line,
            super::super::selinux::INSTALL_PRECONDITION[2],
            "§ 3.1's precondition must carry no --enablerepo"
        );
        assert!(
            !output.contains("crb-mirror-rhel9"),
            "a name that merely contains crb is not a CRB section:\n{output}"
        );
        assert!(
            !output.contains("-debug-rpms") && !output.contains("-source-rpms"),
            "debug and source mirrors are not enabled:\n{output}"
        );
        assert!(
            !output.contains("$(arch)"),
            "no id is reconstructed:\n{output}"
        );

        // The debug and source sections alone emit nothing.
        let mut only_excluded = snapshot_with_packages(&["htop"]);
        only_excluded
            .rpm
            .as_mut()
            .expect("fixture has an rpm section")
            .repo_files
            .push(repo_file(
                CRB_REPO_PATH,
                "[codeready-builder-for-rhel-9-aarch64-debug-rpms]\nenabled=1\n\
                 [codeready-builder-for-rhel-9-aarch64-source-rpms]\nenabled=1\n",
                true,
            ));
        let output = render_containerfile(&only_excluded, None, None);
        assert!(
            !output.contains("--enablerepo") && !output.contains("# CRB was enabled"),
            "debug and source sections alone emit nothing:\n{output}"
        );

        // A section the export is not carrying does not reach a RUN line.
        let mut dropped = snapshot_with_packages(&["htop"]);
        dropped
            .rpm
            .as_mut()
            .expect("fixture has an rpm section")
            .repo_files
            .push(repo_file(CRB_REPO_PATH, CRB_CONTENT, false));
        let output = render_containerfile(&dropped, None, None);
        assert!(
            !output.contains("--enablerepo"),
            "an excluded repo file enables nothing:\n{output}"
        );
    }

    #[test]
    fn test_containerfile_package_based() {
        let snap = snapshot_with_packages(&["httpd", "vim-enhanced"]);
        let output = render_containerfile(&snap, None, None);
        // No target_image or rpm.base_image → FROM omitted
        assert!(
            output.contains("# FROM line omitted"),
            "must omit FROM when no base image source"
        );
        assert!(
            output.contains("RUN dnf install -y \\"),
            "must contain dnf install with continuation"
        );
        assert!(output.contains("httpd"), "must contain httpd");
        assert!(output.contains("vim-enhanced"), "must contain vim-enhanced");
        assert!(
            output.contains("&& dnf clean all"),
            "must contain cleanup commands"
        );
    }

    #[test]
    fn test_containerfile_leaf_packages_use_canonical_ids_but_render_package_names() {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![
                PackageEntry {
                    name: "httpd".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    source_repo: "appstream".into(),
                    disposition: FindingKind::included(),
                    locked: false,
                    ..Default::default()
                },
                PackageEntry {
                    name: "httpd".into(),
                    arch: "i686".into(),
                    state: PackageState::Added,
                    source_repo: "appstream".into(),
                    disposition: FindingKind::excluded(),
                    locked: false,
                    ..Default::default()
                },
            ],
            leaf_packages: Some(vec!["httpd.x86_64".into()]),
            auto_packages: Some(vec!["httpd.i686".into()]),
            ..Default::default()
        });

        let output = render_containerfile(&snap, None, None);

        // Packages are now one-per-line; check the indented package lines
        assert!(
            output.lines().any(|line| line.trim().starts_with("httpd")),
            "must render httpd as a package line"
        );
        assert!(
            !output.contains("httpd.x86_64"),
            "must not leak canonical leaf identity"
        );
        assert!(
            !output.contains("httpd.i686"),
            "must not install the non-leaf arch"
        );
    }

    #[test]
    fn test_containerfile_non_leaf_manual_follow_up_survives_leaf_filter() {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![
                PackageEntry {
                    name: "httpd".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    source_repo: "appstream".into(),
                    disposition: FindingKind::included(),
                    locked: false,
                    ..Default::default()
                },
                PackageEntry {
                    name: "local-tool".into(),
                    arch: "x86_64".into(),
                    state: PackageState::LocalInstall,
                    source_repo: String::new(),
                    disposition: FindingKind::excluded(),
                    locked: false,
                    ..Default::default()
                },
                PackageEntry {
                    name: "orphan-pkg".into(),
                    arch: "x86_64".into(),
                    state: PackageState::NoRepo,
                    source_repo: String::new(),
                    disposition: FindingKind::excluded(),
                    locked: false,
                    ..Default::default()
                },
                PackageEntry {
                    name: "mystery".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    source_repo: String::new(),
                    disposition: FindingKind::excluded(),
                    locked: false,
                    ..Default::default()
                },
            ],
            leaf_packages: Some(vec!["httpd.x86_64".into()]),
            auto_packages: Some(vec![
                "local-tool.x86_64".into(),
                "orphan-pkg.x86_64".into(),
                "mystery.x86_64".into(),
            ]),
            ..Default::default()
        });

        let output = render_containerfile(&snap, None, None);

        // Packages are now one-per-line; collect indented package lines
        let pkg_lines: Vec<&str> = output
            .lines()
            .filter(|l| l.starts_with("    ") && !l.contains("&&") && !l.starts_with("    /"))
            .collect();
        let pkg_text = pkg_lines.join(" ");

        assert!(
            pkg_text.contains("httpd"),
            "leaf package must appear in install block"
        );
        assert!(
            !pkg_text.contains("local-tool")
                && !pkg_text.contains("orphan-pkg")
                && !pkg_text.contains("mystery"),
            "non-leaf unresolved packages must stay off the install block"
        );
        assert!(
            output.contains("# === Manual Follow-up Required ==="),
            "manual follow-up section must be present, got:\n{output}"
        );
        for package in ["local-tool", "orphan-pkg", "mystery"] {
            assert!(
                output.contains(package),
                "manual follow-up must mention {package}, got:\n{output}"
            );
        }
    }

    #[test]
    fn test_containerfile_section_ordering() {
        use inspectah_core::types::services::{
            PresetDefault, ServiceStateChange, ServiceUnitState,
        };
        // Build a snapshot with data in multiple sections to verify ordering
        let mut snap = snapshot_with_packages(&["httpd"]);
        snap.services = Some(inspectah_core::types::services::ServiceSection {
            state_changes: vec![ServiceStateChange {
                unit: "httpd.service".into(),
                current_state: ServiceUnitState::Enabled,
                default_state: Some(PresetDefault::Disable),
                disposition: FindingKind::included(),
                locked: false,
                owning_package: None,
                aggregate: None,
                attention_reason: None,
                shadow_type: None,
                shadow_rationale: None,
            }],
            ..Default::default()
        });
        snap.selinux = Some(inspectah_core::types::selinux::SelinuxSection {
            fips_mode: true,
            ..Default::default()
        });

        let output = render_containerfile(&snap, None, None);

        // Verify section order: packages before services before selinux before epilogue
        let packages_pos = output.find("dnf install -y").unwrap();
        let services_pos = output.find("Service Enablement").unwrap();
        let selinux_pos = output.find("Security & Access Control").unwrap();
        let epilogue_pos = output.find("bootc container lint").unwrap();

        assert!(
            packages_pos < services_pos,
            "packages must come before services"
        );
        assert!(
            services_pos < selinux_pos,
            "services must come before selinux"
        );
        assert!(
            selinux_pos < epilogue_pos,
            "selinux must come before epilogue"
        );
    }

    #[test]
    fn test_containerfile_empty_snapshot() {
        let snap = InspectionSnapshot::new();
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("# FROM line omitted"),
            "empty snapshot must omit FROM with comment"
        );
        assert!(
            !output.starts_with("FROM "),
            "empty snapshot must not have a FROM directive"
        );
        assert!(
            output.contains("RUN bootc container lint"),
            "must contain lint epilogue"
        );
    }

    #[test]
    fn test_containerfile_custom_base_image() {
        use inspectah_core::baseline::{ResolutionStrategy, TargetImageIdentity};
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(TargetImageIdentity {
            image_ref: "quay.io/custom/image:latest".into(),
            strategy: ResolutionStrategy::CliOverride,
        });
        let output = render_containerfile(&snap, None, None);
        assert!(output.contains("FROM quay.io/custom/image:latest"));
    }

    #[test]
    fn test_containerfile_services() {
        use inspectah_core::types::services::{
            PresetDefault, ServiceStateChange, ServiceUnitState,
        };
        let mut snap = InspectionSnapshot::new();
        snap.services = Some(inspectah_core::types::services::ServiceSection {
            state_changes: vec![
                ServiceStateChange {
                    unit: "httpd.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    disposition: FindingKind::included(),
                    locked: false,
                    owning_package: None,
                    aggregate: None,
                    attention_reason: None,
                    shadow_type: None,
                    shadow_rationale: None,
                },
                ServiceStateChange {
                    unit: "sshd.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    disposition: FindingKind::included(),
                    locked: false,
                    owning_package: None,
                    aggregate: None,
                    attention_reason: None,
                    shadow_type: None,
                    shadow_rationale: None,
                },
                ServiceStateChange {
                    unit: "cups.service".into(),
                    current_state: ServiceUnitState::Disabled,
                    default_state: Some(PresetDefault::Enable),
                    disposition: FindingKind::included(),
                    locked: false,
                    owning_package: None,
                    aggregate: None,
                    attention_reason: None,
                    shadow_type: None,
                    shadow_rationale: None,
                },
            ],
            // enabled_units/disabled_units are full inventory — not used by renderer
            enabled_units: vec![
                "httpd.service".into(),
                "sshd.service".into(),
                "chronyd.service".into(),
            ],
            disabled_units: vec!["cups.service".into(), "NetworkManager.service".into()],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("systemctl enable \\\n    httpd.service \\\n    sshd.service"),
            "enable must use multi-line continuation"
        );
        assert!(
            output.contains("systemctl disable \\\n    cups.service"),
            "disable must use multi-line continuation"
        );
        // Preset-matching units from enabled_units/disabled_units must NOT appear
        assert!(
            !output.contains("chronyd"),
            "preset-matching enabled unit must not appear"
        );
        assert!(
            !output.contains("NetworkManager"),
            "preset-matching disabled unit must not appear"
        );
    }

    #[test]
    fn test_containerfile_unsafe_package_skipped() {
        let snap = snapshot_with_packages(&["safe-pkg", "bad;pkg"]);
        let output = render_containerfile(&snap, None, None);
        assert!(output.contains("safe-pkg"));
        // The unsafe package should not appear anywhere in the install block
        assert!(!output.contains("bad;pkg"));
    }

    #[test]
    fn test_containerfile_shell_metachar_package_rejected() {
        // Package name with shell command injection
        let snap = snapshot_with_packages(&["legit-pkg", "pkg$(whoami)", "pkg`id`"]);
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("legit-pkg"),
            "safe package must be included"
        );
        // Unsafe packages must not appear in any RUN line
        for line in output.lines() {
            if line.starts_with("RUN ") {
                assert!(
                    !line.contains("$(whoami)"),
                    "RUN line must not contain shell substitution: {line}"
                );
                assert!(
                    !line.contains("`id`"),
                    "RUN line must not contain backtick substitution: {line}"
                );
            }
        }
    }

    #[test]
    fn test_containerfile_unsafe_module_stream_skipped() {
        use inspectah_core::types::rpm::EnabledModuleStream;
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            module_streams: vec![
                EnabledModuleStream {
                    module_name: "safe-module".into(),
                    stream: "1.0".into(),
                    profiles: vec![],
                    disposition: FindingKind::included(),
                    locked: false,
                    baseline_match: false,
                    ..Default::default()
                },
                EnabledModuleStream {
                    module_name: "evil$(whoami)".into(),
                    stream: "2.0".into(),
                    profiles: vec![],
                    disposition: FindingKind::included(),
                    locked: false,
                    baseline_match: false,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("RUN dnf module enable -y safe-module:1.0"),
            "safe module must be rendered"
        );
        // The unsafe module must not appear in any RUN line
        for line in output.lines() {
            if line.starts_with("RUN ") {
                assert!(
                    !line.contains("$(whoami)"),
                    "RUN line must not contain shell metacharacters: {line}"
                );
            }
        }
        assert!(
            output.contains("FIXME: module stream contains unsafe characters"),
            "unsafe module must produce a FIXME comment"
        );
    }

    #[test]
    fn test_containerfile_unsafe_version_lock_skipped() {
        use inspectah_core::types::rpm::VersionLockEntry;
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            version_locks: vec![
                VersionLockEntry {
                    raw_pattern: "httpd-0:2.4.57-5.el9.*".into(),
                    disposition: FindingKind::included(),
                    locked: false,
                    ..Default::default()
                },
                VersionLockEntry {
                    raw_pattern: "pkg;rm -rf /".into(),
                    disposition: FindingKind::included(),
                    locked: false,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("dnf versionlock add httpd"),
            "safe version lock must be rendered"
        );
        // The unsafe pattern must not appear in any RUN line
        for line in output.lines() {
            if line.starts_with("RUN ") || line.starts_with("    dnf ") {
                assert!(
                    !line.contains("rm -rf"),
                    "RUN line must not contain unsafe version lock: {line}"
                );
            }
        }
        assert!(
            output.contains("FIXME: version lock pattern contains unsafe characters"),
            "unsafe version lock must produce a FIXME comment"
        );
    }

    #[test]
    fn test_containerfile_partial_completeness_warning() {
        let mut snap = InspectionSnapshot::new();
        snap.completeness = Completeness::Incomplete {
            failed_sections: vec![InspectorId::Config],
            degraded_sections: vec![],
            reason: "config inspector timed out".into(),
        };
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains(
                "WARNING: This Containerfile was generated from an incomplete inspection"
            ),
            "must contain completeness warning"
        );
        assert!(
            output.contains("config"),
            "must list the incomplete section"
        );
        // Warning must appear before the FROM/omitted comment
        let warning_pos = output.find("WARNING").unwrap();
        let from_pos = output.find("# FROM line omitted").unwrap();
        assert!(
            warning_pos < from_pos,
            "completeness warning must appear before FROM comment"
        );
    }

    #[test]
    fn test_containerfile_full_completeness_no_warning() {
        let mut snap = InspectionSnapshot::new();
        snap.completeness = Completeness::Complete;
        let output = render_containerfile(&snap, None, None);
        assert!(
            !output.contains(
                "WARNING: This Containerfile was generated from an incomplete inspection"
            ),
            "complete status must not produce warning"
        );
    }

    #[test]
    fn test_containerfile_unsafe_timer_fixme() {
        use inspectah_core::types::scheduled::{ScheduledTaskSection, SystemdTimer};
        let mut snap = InspectionSnapshot::new();
        snap.scheduled_tasks = Some(ScheduledTaskSection {
            systemd_timers: vec![SystemdTimer {
                name: "evil$(whoami)".into(),
                source: "local".into(),
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            }],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("FIXME: Timer unit name contains unsafe characters"),
            "unsafe timer must produce FIXME comment, got:\n{output}"
        );
        assert!(
            output.contains("evil$(whoami)"),
            "FIXME must include original unsafe name"
        );
        // Must NOT appear in any RUN line
        for line in output.lines() {
            if line.starts_with("RUN ") {
                assert!(
                    !line.contains("$(whoami)"),
                    "RUN line must not contain unsafe timer name: {line}"
                );
            }
        }
    }

    #[test]
    fn test_containerfile_gpg_keys_actual_paths() {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            gpg_keys: vec![
                inspectah_core::types::rpm::RepoFile {
                    path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9".into(),
                    content: "key-data".into(),
                    disposition: FindingKind::included(),
                    locked: false,
                    ..Default::default()
                },
                inspectah_core::types::rpm::RepoFile {
                    path: "/opt/custom/keys/signing-key.asc".into(),
                    content: "key-data".into(),
                    disposition: FindingKind::included(),
                    locked: false,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        // Standard-dir key should NOT have rpm --import (auto-imported)
        assert!(
            !output.contains("rpm --import /etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9"),
            "standard-dir key must NOT have rpm --import (auto-imported)"
        );
        // Standard-dir key should have directory COPY
        assert!(
            output.contains("COPY config/etc/pki/rpm-gpg/ /etc/pki/rpm-gpg/"),
            "standard-dir key must have directory COPY"
        );
        assert!(
            output.contains("rpm --import /opt/custom/keys/signing-key.asc"),
            "must have rpm --import for non-standard path key"
        );
    }

    #[test]
    fn test_containerfile_gpg_unsafe_path_fixme() {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            gpg_keys: vec![
                inspectah_core::types::rpm::RepoFile {
                    path: "/etc/pki/rpm-gpg/GOOD-KEY".into(),
                    content: "key-data".into(),
                    disposition: FindingKind::included(),
                    locked: false,
                    ..Default::default()
                },
                inspectah_core::types::rpm::RepoFile {
                    path: "../../etc/shadow".into(),
                    content: "bad".into(),
                    disposition: FindingKind::included(),
                    locked: false,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        assert!(output.contains("FIXME"), "unsafe path must produce FIXME");
        assert!(
            !output.contains("rpm --import ../../etc/shadow"),
            "traversal path must NOT reach rpm --import"
        );
        // GOOD-KEY is in the standard GPG dir — gets directory COPY, no rpm --import
        assert!(
            output.contains("COPY config/etc/pki/rpm-gpg/ /etc/pki/rpm-gpg/"),
            "safe standard-dir key must get directory COPY"
        );
        assert!(
            !output.contains("rpm --import /etc/pki/rpm-gpg/GOOD-KEY"),
            "standard-dir key must NOT have rpm --import (auto-imported)"
        );
    }

    #[test]
    fn test_containerfile_gpg_whitespace_path_fixme() {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            gpg_keys: vec![inspectah_core::types::rpm::RepoFile {
                path: "/opt/custom keys/signing-key.asc".into(),
                content: "key-data".into(),
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            }],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("FIXME"),
            "whitespace path must produce FIXME"
        );
        assert!(
            !output.contains("rpm --import /opt/custom keys"),
            "whitespace path must NOT reach rpm --import"
        );
    }

    #[test]
    fn test_gpg_standard_dir_single_copy() {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            gpg_keys: vec![
                inspectah_core::types::rpm::RepoFile {
                    path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9".into(),
                    content: "key1".into(),
                    disposition: FindingKind::included(),
                    locked: false,
                    ..Default::default()
                },
                inspectah_core::types::rpm::RepoFile {
                    path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial".into(),
                    content: "key2".into(),
                    disposition: FindingKind::included(),
                    locked: false,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        let copy_lines: Vec<_> = output
            .lines()
            .filter(|l| l.contains("COPY") && l.contains("rpm-gpg"))
            .collect();
        assert_eq!(
            copy_lines.len(),
            1,
            "standard dir keys should be single COPY, got: {:?}",
            copy_lines
        );
        assert!(
            !output.contains("rpm --import"),
            "standard dir keys should not have explicit imports"
        );
    }

    #[test]
    fn test_gpg_mixed_paths() {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            gpg_keys: vec![
                inspectah_core::types::rpm::RepoFile {
                    path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9".into(),
                    content: "key1".into(),
                    disposition: FindingKind::included(),
                    locked: false,
                    ..Default::default()
                },
                inspectah_core::types::rpm::RepoFile {
                    path: "/opt/custom/keys/signing-key.asc".into(),
                    content: "key2".into(),
                    disposition: FindingKind::included(),
                    locked: false,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        // Standard dir key gets directory COPY
        assert!(
            output.contains("COPY config/etc/pki/rpm-gpg/"),
            "standard dir keys should get directory COPY"
        );
        // Non-standard key gets per-key COPY + import
        assert!(
            output.contains("rpm --import /opt/custom/keys/signing-key.asc"),
            "non-standard key should get rpm --import"
        );
    }

    #[test]
    fn test_service_backslash_continuation_over_3() {
        use inspectah_core::types::services::{
            PresetDefault, ServiceStateChange, ServiceUnitState,
        };
        let mut snap = InspectionSnapshot::new();
        snap.services = Some(inspectah_core::types::services::ServiceSection {
            state_changes: vec![
                ServiceStateChange {
                    unit: "httpd.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    disposition: FindingKind::included(),
                    locked: false,
                    owning_package: None,
                    aggregate: None,
                    attention_reason: None,
                    shadow_type: None,
                    shadow_rationale: None,
                },
                ServiceStateChange {
                    unit: "sshd.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    disposition: FindingKind::included(),
                    locked: false,
                    owning_package: None,
                    aggregate: None,
                    attention_reason: None,
                    shadow_type: None,
                    shadow_rationale: None,
                },
                ServiceStateChange {
                    unit: "chronyd.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    disposition: FindingKind::included(),
                    locked: false,
                    owning_package: None,
                    aggregate: None,
                    attention_reason: None,
                    shadow_type: None,
                    shadow_rationale: None,
                },
                ServiceStateChange {
                    unit: "firewalld.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    disposition: FindingKind::included(),
                    locked: false,
                    owning_package: None,
                    aggregate: None,
                    attention_reason: None,
                    shadow_type: None,
                    shadow_rationale: None,
                },
            ],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("systemctl enable \\"),
            "4+ services should use continuation, got:\n{}",
            output
        );
        assert!(
            output.contains("    httpd.service \\"),
            "services on indented lines with continuation"
        );
    }

    #[test]
    fn test_service_multi_line_even_under_4() {
        use inspectah_core::types::services::{
            PresetDefault, ServiceStateChange, ServiceUnitState,
        };
        let mut snap = InspectionSnapshot::new();
        snap.services = Some(inspectah_core::types::services::ServiceSection {
            state_changes: vec![
                ServiceStateChange {
                    unit: "httpd.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    disposition: FindingKind::included(),
                    locked: false,
                    owning_package: None,
                    aggregate: None,
                    attention_reason: None,
                    shadow_type: None,
                    shadow_rationale: None,
                },
                ServiceStateChange {
                    unit: "sshd.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    disposition: FindingKind::included(),
                    locked: false,
                    owning_package: None,
                    aggregate: None,
                    attention_reason: None,
                    shadow_type: None,
                    shadow_rationale: None,
                },
            ],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("systemctl enable \\\n    httpd.service \\\n    sshd.service"),
            "2 services must use multi-line continuation"
        );
    }

    #[test]
    fn test_containerfile_masked_services() {
        use inspectah_core::types::services::{
            PresetDefault, ServiceStateChange, ServiceUnitState,
        };
        let mut snap = InspectionSnapshot::new();
        snap.services = Some(inspectah_core::types::services::ServiceSection {
            state_changes: vec![
                ServiceStateChange {
                    unit: "cups.service".into(),
                    current_state: ServiceUnitState::Masked,
                    default_state: Some(PresetDefault::Enable),
                    disposition: FindingKind::included(),
                    locked: false,
                    owning_package: None,
                    aggregate: None,
                    attention_reason: None,
                    shadow_type: None,
                    shadow_rationale: None,
                },
                ServiceStateChange {
                    unit: "httpd.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    disposition: FindingKind::included(),
                    locked: false,
                    owning_package: None,
                    aggregate: None,
                    attention_reason: None,
                    shadow_type: None,
                    shadow_rationale: None,
                },
            ],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("systemctl mask \\\n    cups.service"),
            "masked service must produce systemctl mask with continuation, got:\n{}",
            output
        );
        assert!(
            output.contains("systemctl enable \\\n    httpd.service"),
            "enabled service must still work alongside masked"
        );
    }

    #[test]
    fn test_service_disable_backslash_continuation_over_3() {
        use inspectah_core::types::services::{
            PresetDefault, ServiceStateChange, ServiceUnitState,
        };
        let mut snap = InspectionSnapshot::new();
        snap.services = Some(inspectah_core::types::services::ServiceSection {
            state_changes: vec![
                ServiceStateChange {
                    unit: "cups.service".into(),
                    current_state: ServiceUnitState::Disabled,
                    default_state: Some(PresetDefault::Enable),
                    disposition: FindingKind::included(),
                    locked: false,
                    owning_package: None,
                    aggregate: None,
                    attention_reason: None,
                    shadow_type: None,
                    shadow_rationale: None,
                },
                ServiceStateChange {
                    unit: "avahi-daemon.service".into(),
                    current_state: ServiceUnitState::Disabled,
                    default_state: Some(PresetDefault::Enable),
                    disposition: FindingKind::included(),
                    locked: false,
                    owning_package: None,
                    aggregate: None,
                    attention_reason: None,
                    shadow_type: None,
                    shadow_rationale: None,
                },
                ServiceStateChange {
                    unit: "bluetooth.service".into(),
                    current_state: ServiceUnitState::Disabled,
                    default_state: Some(PresetDefault::Enable),
                    disposition: FindingKind::included(),
                    locked: false,
                    owning_package: None,
                    aggregate: None,
                    attention_reason: None,
                    shadow_type: None,
                    shadow_rationale: None,
                },
                ServiceStateChange {
                    unit: "ModemManager.service".into(),
                    current_state: ServiceUnitState::Disabled,
                    default_state: Some(PresetDefault::Enable),
                    disposition: FindingKind::included(),
                    locked: false,
                    owning_package: None,
                    aggregate: None,
                    attention_reason: None,
                    shadow_type: None,
                    shadow_rationale: None,
                },
            ],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("systemctl disable \\"),
            "4+ disabled services should use continuation"
        );
    }

    #[test]
    fn test_containerfile_gpg_root_path_staged() {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            gpg_keys: vec![inspectah_core::types::rpm::RepoFile {
                path: "/good-key".into(),
                content: "key-data".into(),
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            }],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("COPY config/good-key /good-key"),
            "root-level key must have direct COPY"
        );
        assert!(
            output.contains("rpm --import /good-key"),
            "root-level key must have rpm --import after staging"
        );
    }

    // -- Phase 6: Dynamic FROM tests ------------------------------------------

    #[test]
    fn test_from_uses_target_image() {
        use inspectah_core::baseline::{ResolutionStrategy, TargetImageIdentity};
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(TargetImageIdentity {
            image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.6".into(),
            strategy: ResolutionStrategy::OsRelease,
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("FROM registry.redhat.io/rhel9/rhel-bootc:9.6"),
            "must use target_image.image_ref for FROM, got:\n{output}"
        );
    }

    #[test]
    fn test_from_omitted_when_no_target_image() {
        let snap = InspectionSnapshot::new();
        let result = base_image_from_snapshot(&snap);
        assert!(result.is_none(), "no target_image must return None");
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("# FROM line omitted"),
            "must contain omission comment"
        );
        assert!(
            !output.lines().any(|l| l.starts_with("FROM ")),
            "must not contain a FROM directive"
        );
    }

    #[test]
    fn test_from_no_target_image_returns_none() {
        let snap = InspectionSnapshot::new();
        let result = base_image_from_snapshot(&snap);
        assert!(result.is_none(), "no target_image must return None");
    }

    #[test]
    fn test_from_target_image_returns_ref() {
        use inspectah_core::baseline::{ResolutionStrategy, TargetImageIdentity};
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(TargetImageIdentity {
            image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.6".into(),
            strategy: ResolutionStrategy::OsRelease,
        });
        let result = base_image_from_snapshot(&snap);
        assert_eq!(
            result.unwrap(),
            "registry.redhat.io/rhel9/rhel-bootc:9.6",
            "target_image.image_ref must be returned"
        );
    }

    #[test]
    fn test_excluded_quadlet_generates_no_containerfile_output() {
        use inspectah_core::types::containers::{ContainerSection, QuadletUnit};
        let mut snap = InspectionSnapshot::new();
        // Aggregate context: explicit include=false is honored (prevalence-based).
        // Single-host snapshots override include=false for quadlets.
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
                name: "excluded.container".into(),
                content: "[Container]\nImage=quay.io/test:latest".into(),
                disposition: FindingKind::excluded(),
                locked: false,
                ..Default::default()
            }],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            !output.contains("COPY quadlet/"),
            "excluded quadlet must NOT produce COPY quadlet/ line"
        );
        assert!(
            !output.contains("Container Workloads"),
            "excluded quadlet must NOT produce Container Workloads section"
        );
    }

    #[test]
    fn test_included_quadlet_renders_containerfile_output() {
        use inspectah_core::types::containers::{ContainerSection, QuadletUnit};
        let mut snap = InspectionSnapshot::new();
        // Quadlets with include=true (set by collectors) produce output.
        snap.containers = Some(ContainerSection {
            quadlet_units: vec![QuadletUnit {
                name: "app.container".into(),
                content: "[Container]\nImage=quay.io/test:latest".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("COPY quadlet/"),
            "included quadlet must produce COPY quadlet/ line"
        );
        assert!(
            output.contains("Container Workloads"),
            "included quadlet must produce Container Workloads section"
        );
    }

    #[test]
    fn test_included_tuned_renders_containerfile_output() {
        use inspectah_core::types::kernelboot::KernelBootSection;
        let mut snap = InspectionSnapshot::new();
        // Tuned with include=true (set by collectors) produces output.
        snap.kernel_boot = Some(KernelBootSection {
            tuned_active: "virtual-guest".into(),
            tuned_disposition: FindingKind::included(),
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("tuned"),
            "included tuned must produce tuned output"
        );
        assert!(
            output.contains("RUN systemctl enable tuned.service"),
            "included tuned must enable tuned.service"
        );
    }

    #[test]
    fn test_excluded_flatpak_generates_no_containerfile_output() {
        use inspectah_core::types::containers::{ContainerSection, FlatpakApp};
        let mut snap = InspectionSnapshot::new();
        // Flatpaks with include=false produce no output.
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
                app_id: "org.example.Excluded".into(),
                origin: "flathub".into(),
                branch: "stable".into(),
                disposition: FindingKind::excluded(),
                locked: false,
                aggregate: None,
                remote: "flathub".into(),
                remote_url: "https://flathub.org/repo/".into(),
            }],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            !output.contains("flatpak"),
            "excluded flatpak must NOT produce any flatpak output"
        );
        assert!(
            !output.contains("Container Workloads"),
            "excluded flatpak must NOT produce Container Workloads section"
        );
    }

    #[test]
    fn test_containerfile_excludes_baseline_suppressed_packages() {
        use inspectah_core::snapshot::InspectionSnapshot;
        use inspectah_core::types::rpm::{PackageEntry, RpmSection};

        // Build an RpmSection with baseline_suppressed packages
        // and verify they don't appear in the rendered containerfile
        let snap = InspectionSnapshot {
            rpm: Some(RpmSection {
                packages_added: vec![
                    PackageEntry {
                        name: "httpd".into(),
                        arch: "x86_64".into(),
                        disposition: FindingKind::included(),
                        locked: false,
                        source_repo: "appstream".into(),
                        ..Default::default()
                    },
                    PackageEntry {
                        name: "kernel".into(),
                        arch: "x86_64".into(),
                        disposition: FindingKind::included(),
                        locked: false,
                        source_repo: "baseos".into(),
                        ..Default::default()
                    },
                ],
                leaf_packages: Some(vec!["httpd.x86_64".into(), "kernel.x86_64".into()]),
                baseline_suppressed: Some(vec!["kernel.x86_64".into()]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("httpd"),
            "non-suppressed package should be in containerfile"
        );
        assert!(
            !output.contains("kernel"),
            "baseline-suppressed package must not be in containerfile"
        );
    }

    #[test]
    fn test_containerfile_excluded_service_generates_no_systemctl() {
        use inspectah_core::types::services::{
            PresetDefault, ServiceStateChange, ServiceUnitState,
        };
        let mut snap = InspectionSnapshot::new();
        snap.services = Some(inspectah_core::types::services::ServiceSection {
            state_changes: vec![
                ServiceStateChange {
                    unit: "httpd.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    disposition: FindingKind::included(),
                    locked: false,
                    owning_package: None,
                    aggregate: None,
                    attention_reason: None,
                    shadow_type: None,
                    shadow_rationale: None,
                },
                ServiceStateChange {
                    unit: "excluded.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    disposition: FindingKind::excluded(), // excluded by triage
                    locked: false,
                    owning_package: None,
                    aggregate: None,
                    attention_reason: None,
                    shadow_type: None,
                    shadow_rationale: None,
                },
            ],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("httpd.service"),
            "included service must produce systemctl line"
        );
        assert!(
            !output.contains("excluded.service"),
            "service with include=false must NOT appear in containerfile"
        );
    }

    /// End-to-end: containerfile service section uses implied_action via the
    /// service_intent authority — enabled/disabled/masked services produce
    /// the correct systemctl verbs.
    #[test]
    fn test_containerfile_services_use_implied_action() {
        use inspectah_core::types::services::{
            PresetDefault, ServiceStateChange, ServiceUnitState,
        };
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            baseline_package_names: Some(vec!["httpd".into(), "cups".into(), "avahi".into()]),
            packages_added: vec![],
            ..Default::default()
        });
        snap.services = Some(inspectah_core::types::services::ServiceSection {
            state_changes: vec![
                ServiceStateChange {
                    unit: "httpd.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    disposition: FindingKind::included(),
                    locked: false,
                    owning_package: Some("httpd".into()),
                    aggregate: None,
                    attention_reason: None,
                    shadow_type: None,
                    shadow_rationale: None,
                },
                ServiceStateChange {
                    unit: "cups.service".into(),
                    current_state: ServiceUnitState::Masked,
                    default_state: Some(PresetDefault::Enable),
                    disposition: FindingKind::included(),
                    locked: false,
                    owning_package: Some("cups".into()),
                    aggregate: None,
                    attention_reason: None,
                    shadow_type: None,
                    shadow_rationale: None,
                },
                ServiceStateChange {
                    unit: "avahi-daemon.service".into(),
                    current_state: ServiceUnitState::Disabled,
                    default_state: Some(PresetDefault::Enable),
                    disposition: FindingKind::included(),
                    locked: false,
                    owning_package: Some("avahi".into()),
                    aggregate: None,
                    attention_reason: None,
                    shadow_type: None,
                    shadow_rationale: None,
                },
            ],
            ..Default::default()
        });

        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("systemctl enable \\\n    httpd.service"),
            "enabled service must produce systemctl enable with continuation"
        );
        assert!(
            output.contains("systemctl mask \\\n    cups.service"),
            "masked service must produce systemctl mask with continuation"
        );
        assert!(
            output.contains("systemctl disable \\\n    avahi-daemon.service"),
            "disabled service must produce systemctl disable with continuation"
        );
    }

    #[test]
    fn test_containerfile_sysctl_copy_synthesized_file() {
        use inspectah_core::types::kernelboot::{KernelBootSection, SysctlOverride};
        let mut snap = InspectionSnapshot::new();
        snap.kernel_boot = Some(KernelBootSection {
            sysctl_overrides: vec![
                SysctlOverride {
                    key: "net.ipv4.ip_forward".into(),
                    runtime: "1".into(),
                    source: "/etc/sysctl.d/99-custom.conf".into(),
                    disposition: FindingKind::included(),
                    locked: false,
                    ..Default::default()
                },
                SysctlOverride {
                    key: "vm.swappiness".into(),
                    runtime: "10".into(),
                    source: "/etc/sysctl.d/99-custom.conf".into(),
                    disposition: FindingKind::excluded(),
                    locked: false,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("COPY sysctl/etc/sysctl.d/99-inspectah-migrated.conf /etc/sysctl.d/"),
            "included sysctls must produce COPY for synthesized file, got:\n{output}"
        );
        assert!(
            output.contains("1 sysctl override(s)"),
            "count must reflect only included overrides"
        );
    }

    #[test]
    fn test_containerfile_sysctl_excluded_no_copy() {
        use inspectah_core::types::kernelboot::{KernelBootSection, SysctlOverride};
        let mut snap = InspectionSnapshot::new();
        snap.kernel_boot = Some(KernelBootSection {
            sysctl_overrides: vec![SysctlOverride {
                key: "vm.swappiness".into(),
                runtime: "10".into(),
                source: "/etc/sysctl.d/99-custom.conf".into(),
                disposition: FindingKind::excluded(),
                locked: false,
                ..Default::default()
            }],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            !output.contains("sysctl"),
            "all-excluded sysctls must produce no sysctl output, got:\n{output}"
        );
    }

    #[test]
    fn test_containerfile_tuned_excluded_no_output() {
        use inspectah_core::types::kernelboot::KernelBootSection;
        let mut snap = InspectionSnapshot::new();
        // Aggregate context: explicit tuned_include=false is honored.
        // Single-host snapshots override to included when tuned_active is set.
        snap.aggregate_meta = Some(inspectah_core::types::aggregate::AggregateSnapshotMeta {
            label: "test".into(),
            host_count: 3,
            hostnames: vec![],
            merged_at: String::new(),
            baseline_provisional: false,
            section_host_counts: Default::default(),
        });
        snap.kernel_boot = Some(KernelBootSection {
            tuned_active: "virtual-guest".into(),
            tuned_disposition: FindingKind::excluded(),
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            !output.contains("tuned"),
            "excluded tuned must produce no tuned output, got:\n{output}"
        );
    }

    /// § 3.5: the custom profile travels under `config/etc/tuned/` with
    /// the rest of the config tree, so the Containerfile's tuned block is
    /// the activation decision and nothing else.
    ///
    /// The earlier form of this test asserted the
    /// `COPY tuned/etc/tuned/ /etc/tuned/` line, which came from the
    /// `tuned/` promoted root: a second copy path for a file the config
    /// tree already carries.
    #[test]
    fn test_containerfile_tuned_activation_without_a_promoted_root() {
        use inspectah_core::types::config::{ConfigCategory, ConfigFileEntry, ConfigSection};
        use inspectah_core::types::kernelboot::KernelBootSection;
        let mut snap = InspectionSnapshot::new();
        snap.kernel_boot = Some(KernelBootSection {
            tuned_active: "my-profile".into(),
            tuned_disposition: FindingKind::included(),
            ..Default::default()
        });
        snap.config = Some(ConfigSection {
            files: vec![ConfigFileEntry {
                path: "/etc/tuned/my-profile/tuned.conf".into(),
                category: ConfigCategory::Tuned,
                content: "[main]\nsummary=Custom".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            !output.contains("COPY tuned/"),
            "the tuned/ promoted root is gone, got:\n{output}"
        );
        assert!(
            output.contains("RUN echo \"my-profile\" > /etc/tuned/active_profile"),
            "must set active_profile"
        );
        assert!(
            output.contains("RUN systemctl enable tuned.service"),
            "must enable tuned.service"
        );
    }

    /// § 3.5: activation and the profile file are two decisions on two
    /// entities, so they can disagree. Activating a profile whose file the
    /// operator excluded would bake a reference to a file the image does
    /// not carry, so the renderer names the contradiction instead.
    #[test]
    fn test_containerfile_tuned_activation_without_the_profile_file() {
        use inspectah_core::types::config::{ConfigCategory, ConfigFileEntry, ConfigSection};
        use inspectah_core::types::kernelboot::KernelBootSection;
        let mut snap = InspectionSnapshot::new();
        snap.kernel_boot = Some(KernelBootSection {
            tuned_active: "my-profile".into(),
            tuned_disposition: FindingKind::included(),
            ..Default::default()
        });
        snap.config = Some(ConfigSection {
            files: vec![ConfigFileEntry {
                path: "/etc/tuned/my-profile/tuned.conf".into(),
                category: ConfigCategory::Tuned,
                content: "[main]\nsummary=Custom".into(),
                disposition: FindingKind::excluded(),
                ..Default::default()
            }],
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains(
                "# FIXME: include /etc/tuned/my-profile/tuned.conf under Configuration Files \
                 or activate a stock profile; the active profile is custom and its file is \
                 excluded"
            ),
            "an excluded custom profile with activation included must render the FIXME, got:\n{output}"
        );
        assert!(
            !output.contains("active_profile"),
            "the activation lines must not render alongside the FIXME, got:\n{output}"
        );
        assert!(
            !output.contains("COPY tuned/"),
            "the tuned/ promoted root is gone, got:\n{output}"
        );
    }

    #[test]
    fn test_included_drop_in_generates_copy_line() {
        use inspectah_core::types::services::{ServiceSection, SystemdDropIn};
        let mut snap = InspectionSnapshot::new();
        snap.services = Some(ServiceSection {
            drop_ins: vec![SystemdDropIn {
                unit: "httpd.service".into(),
                path: "etc/systemd/system/httpd.service.d/limits.conf".into(),
                content: "[Service]\nLimitNOFILE=65535".into(),
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            }],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("COPY drop-ins/etc/systemd/system/ /etc/systemd/system/"),
            "included drop-in must produce COPY drop-ins/ line, got:\n{output}"
        );
        assert!(
            output.contains("Service Drop-ins"),
            "included drop-in must produce Service Drop-ins section header"
        );
    }

    #[test]
    fn test_partial_leaf_authority_comment_appears() {
        let mut snap = snapshot_with_packages(&["httpd"]);
        snap.aggregate_meta = Some(inspectah_core::types::aggregate::AggregateSnapshotMeta {
            label: "test".into(),
            host_count: 3,
            hostnames: vec!["a".into(), "b".into(), "c".into()],
            merged_at: "2026-06-09T00:00:00Z".into(),
            baseline_provisional: false,
            section_host_counts: Default::default(),
        });
        snap.rpm.as_mut().unwrap().leaf_authority_hosts = Some(2);
        snap.rpm.as_mut().unwrap().leaf_total_hosts = Some(3);

        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("# Leaf classification: 2/3 hosts (partial authority)"),
            "partial authority comment must appear when auth < total, got:\n{output}"
        );
    }

    #[test]
    fn test_full_leaf_authority_no_comment() {
        let mut snap = snapshot_with_packages(&["httpd"]);
        snap.aggregate_meta = Some(inspectah_core::types::aggregate::AggregateSnapshotMeta {
            label: "test".into(),
            host_count: 2,
            hostnames: vec!["a".into(), "b".into()],
            merged_at: "2026-06-09T00:00:00Z".into(),
            baseline_provisional: false,
            section_host_counts: Default::default(),
        });
        snap.rpm.as_mut().unwrap().leaf_authority_hosts = Some(2);
        snap.rpm.as_mut().unwrap().leaf_total_hosts = Some(2);

        let output = render_containerfile(&snap, None, None);
        assert!(
            !output.contains("partial authority"),
            "full authority must not produce partial authority comment"
        );
    }

    #[test]
    fn test_excluded_drop_in_generates_no_copy_line() {
        use inspectah_core::types::services::{ServiceSection, SystemdDropIn};
        let mut snap = InspectionSnapshot::new();
        snap.services = Some(ServiceSection {
            drop_ins: vec![SystemdDropIn {
                unit: "httpd.service".into(),
                path: "etc/systemd/system/httpd.service.d/limits.conf".into(),
                content: "[Service]\nLimitNOFILE=65535".into(),
                disposition: FindingKind::excluded(),
                locked: false,
                ..Default::default()
            }],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        assert!(
            !output.contains("COPY drop-ins/"),
            "excluded drop-in must NOT produce COPY drop-ins/ line"
        );
        assert!(
            !output.contains("Service Drop-ins"),
            "excluded drop-in must NOT produce Service Drop-ins section"
        );
    }

    #[test]
    fn test_aggregate_containerfile_leaf_only_packages() {
        // Aggregate snapshot with pre-filtered leaf-only packages_added.
        // Simulates what merge_rpm_sections produces: only leaf packages
        // survive into packages_added, transitive deps are stripped.
        let mut snap = InspectionSnapshot::new();
        snap.aggregate_meta = Some(inspectah_core::types::aggregate::AggregateSnapshotMeta {
            label: "web-tier".into(),
            host_count: 3,
            hostnames: vec!["web-01".into(), "web-02".into(), "web-03".into()],
            merged_at: "2026-06-09T00:00:00Z".into(),
            baseline_provisional: false,
            section_host_counts: Default::default(),
        });
        snap.rpm = Some(RpmSection {
            // Only leaf package — merge already filtered out perl-libs
            packages_added: vec![PackageEntry {
                name: "git".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                disposition: FindingKind::included(),
                locked: false,
                ..Default::default()
            }],
            leaf_packages: Some(vec!["git.x86_64".into()]),
            auto_packages: Some(Vec::new()),
            ..Default::default()
        });

        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("git"),
            "leaf package 'git' must appear in containerfile install line"
        );
        assert!(
            !output.contains("perl-libs"),
            "transitive dep 'perl-libs' must NOT appear (pre-filtered by merge)"
        );
    }

    #[test]
    fn test_containerfile_repo_enabling_packages_split() {
        // The basis is `repo_enabling_from`, not the name: this fixture
        // used to pass on the `-release` suffix alone, and would pass that
        // way again if the suffix came back.
        let mut snap = snapshot_with_packages(&["bat", "epel-release", "htop"]);
        make_epel_repo_enabling(&mut snap, "epel-release");
        let output = render_containerfile(&snap, None, None);

        // epel-release must appear in its own install step before the other packages
        let repo_pos = output
            .find("Repo-enabling packages")
            .expect("must have a repo-enabling section");
        let pkg_pos = output
            .find("Packages (2)")
            .expect("must have a packages section with the remaining 2");
        assert!(
            repo_pos < pkg_pos,
            "repo-enabling section must come before regular packages section"
        );

        // The repo-enabling section should contain epel-release
        let repo_section = &output[repo_pos..pkg_pos];
        assert!(
            repo_section.contains("epel-release"),
            "repo-enabling section must contain epel-release"
        );

        // The regular packages section should NOT contain epel-release
        let pkg_section = &output[pkg_pos..];
        assert!(
            !pkg_section.contains("epel-release"),
            "regular packages section must not contain epel-release"
        );
        assert!(pkg_section.contains("bat"), "must contain bat");
        assert!(pkg_section.contains("htop"), "must contain htop");
    }

    #[test]
    fn test_containerfile_no_repo_packages_single_block() {
        // `vendor-release` is the whole point of this fixture: its name
        // ends in `-release` and nothing records a serving repo for it, so
        // a single block is correct *because* no package carries
        // `repo_enabling_from`. Before, the fixture simply had no
        // `-release` name in it and this assertion was true for a reason
        // that no longer exists.
        let snap = snapshot_with_packages(&["httpd", "vim-enhanced", "git", "vendor-release"]);
        let output = render_containerfile(&snap, None, None);

        // No repo-enabling section should exist
        assert!(
            !output.contains("Repo-enabling packages"),
            "a name ending in -release is not the basis; only repo_enabling_from is:\n{output}"
        );
        // Single packages section with all 4
        assert!(
            output.contains("Packages (4)"),
            "must have a single packages section with all 4 packages:\n{output}"
        );
        assert!(output.contains("httpd"), "must contain httpd");
        assert!(output.contains("vim-enhanced"), "must contain vim-enhanced");
        assert!(output.contains("git"), "must contain git");
        assert!(
            output.contains("vendor-release"),
            "must contain vendor-release"
        );
    }

    // --- Group install rendering tests ---

    use inspectah_core::types::group_render::{GroupRenderState, RenderContext};
    use inspectah_core::types::rpm::InstalledGroup;

    /// Build a snapshot with packages AND installed groups.
    fn snapshot_with_packages_and_groups(
        pkg_names: &[&str],
        groups: Vec<InstalledGroup>,
    ) -> InspectionSnapshot {
        let mut snap = snapshot_with_packages(pkg_names);
        if let Some(ref mut rpm) = snap.rpm {
            rpm.installed_groups = Some(groups);
        }
        snap
    }

    #[test]
    fn test_containerfile_renders_group_install_section() {
        let snap = snapshot_with_packages_and_groups(
            &["httpd", "gcc", "make", "vim-enhanced"],
            vec![
                InstalledGroup {
                    name: "Development Tools".into(),
                    members: vec!["gcc".into(), "make".into()],
                    optional_installed: vec![],
                },
                InstalledGroup {
                    name: "Container Management".into(),
                    members: vec!["podman".into()],
                    optional_installed: vec![],
                },
            ],
        );

        let mut ctx = RenderContext::default();
        ctx.group_states
            .insert("Development Tools".into(), GroupRenderState::Renderable);
        ctx.group_states
            .insert("Container Management".into(), GroupRenderState::Renderable);

        let output = render_containerfile(&snap, None, Some(&ctx));

        // Group section present with correct count
        assert!(
            output.contains("Package Groups (2)"),
            "must have Package Groups (2) section header"
        );
        // dnf group install command
        assert!(
            output.contains("RUN dnf group install -y"),
            "must contain dnf group install command"
        );
        // Group names quoted and sorted alphabetically
        assert!(
            output.contains("\"Container Management\""),
            "must contain quoted group name Container Management"
        );
        assert!(
            output.contains("\"Development Tools\""),
            "must contain quoted group name Development Tools"
        );
        // Container Management sorts before Development Tools
        let cm_pos = output.find("\"Container Management\"").unwrap();
        let dt_pos = output.find("\"Development Tools\"").unwrap();
        assert!(
            cm_pos < dt_pos,
            "Container Management must sort before Development Tools"
        );

        // Group members suppressed from individual packages
        assert!(
            !output.contains("    gcc "),
            "gcc must be suppressed from individual packages (member of renderable group)"
        );
        assert!(
            !output.contains("    make "),
            "make must be suppressed from individual packages (member of renderable group)"
        );

        // Non-member packages still present
        assert!(
            output.contains("httpd"),
            "httpd must remain in individual packages"
        );
        assert!(
            output.contains("vim-enhanced"),
            "vim-enhanced must remain in individual packages"
        );

        // Group section comes before individual packages section
        let group_pos = output.find("Package Groups").unwrap();
        let pkg_pos = output.find("Packages (").unwrap();
        assert!(
            group_pos < pkg_pos,
            "group section must come before individual packages section"
        );
    }

    #[test]
    fn test_containerfile_no_group_section_without_render_ctx() {
        let snap = snapshot_with_packages_and_groups(
            &["httpd", "gcc", "make"],
            vec![InstalledGroup {
                name: "Development Tools".into(),
                members: vec!["gcc".into(), "make".into()],
                optional_installed: vec![],
            }],
        );

        // render_ctx is None — all packages render individually
        let output = render_containerfile(&snap, None, None);

        assert!(
            !output.contains("Package Groups"),
            "must not have group section when render_ctx is None"
        );
        assert!(
            !output.contains("dnf group install"),
            "must not have dnf group install when render_ctx is None"
        );
        // All packages present individually
        assert!(output.contains("gcc"), "gcc must be in individual packages");
        assert!(
            output.contains("make"),
            "make must be in individual packages"
        );
        assert!(
            output.contains("httpd"),
            "httpd must be in individual packages"
        );
        assert!(
            output.contains("Packages (3)"),
            "must show all 3 packages individually"
        );
    }

    #[test]
    fn test_containerfile_excluded_groups_not_rendered() {
        let snap = snapshot_with_packages_and_groups(
            &["httpd", "gcc", "make"],
            vec![InstalledGroup {
                name: "Development Tools".into(),
                members: vec!["gcc".into(), "make".into()],
                optional_installed: vec![],
            }],
        );

        let mut ctx = RenderContext::default();
        ctx.group_states
            .insert("Development Tools".into(), GroupRenderState::Excluded);

        let output = render_containerfile(&snap, None, Some(&ctx));

        assert!(
            !output.contains("Package Groups"),
            "excluded group must not produce a group section"
        );
        assert!(
            !output.contains("dnf group install"),
            "excluded group must not produce dnf group install"
        );
        // Members render individually since group is excluded
        assert!(
            output.contains("gcc"),
            "gcc must be in individual packages when group is excluded"
        );
        assert!(
            output.contains("make"),
            "make must be in individual packages when group is excluded"
        );
    }

    #[test]
    fn test_containerfile_optional_installed_not_suppressed_by_renderable_group() {
        // optional_installed members MUST appear individually even when the
        // parent group is renderable — `dnf group install` does not install
        // optional members, so suppressing them loses them entirely.
        let snap = snapshot_with_packages_and_groups(
            &["httpd", "gcc", "gdb"],
            vec![InstalledGroup {
                name: "Development Tools".into(),
                members: vec!["gcc".into()],
                optional_installed: vec!["gdb".into()],
            }],
        );

        let mut ctx = RenderContext::default();
        ctx.group_states
            .insert("Development Tools".into(), GroupRenderState::Renderable);

        let output = render_containerfile(&snap, None, Some(&ctx));

        assert!(
            output.contains("Package Groups (1)"),
            "must have Package Groups (1) section"
        );
        // Mandatory member suppressed — reproduced by dnf group install
        assert!(
            !output.contains("    gcc "),
            "gcc must be suppressed (mandatory member reproduced by group install)"
        );
        // Optional member NOT suppressed — must appear individually
        assert!(
            output.contains("gdb"),
            "gdb must appear individually (optional_installed, not installed by group install)"
        );
        assert!(
            output.contains("httpd"),
            "httpd must remain in individual packages"
        );
        // Provenance comment for optional members
        assert!(
            output.contains("# Optional members from \"Development Tools\""),
            "must have optional provenance comment"
        );
    }

    #[test]
    fn test_containerfile_mixed_renderable_and_degraded_groups() {
        use inspectah_core::types::group_render::DegradationReason;

        let snap = snapshot_with_packages_and_groups(
            &["httpd", "gcc", "make", "podman", "skopeo"],
            vec![
                InstalledGroup {
                    name: "Development Tools".into(),
                    members: vec!["gcc".into(), "make".into()],
                    optional_installed: vec![],
                },
                InstalledGroup {
                    name: "Container Management".into(),
                    members: vec!["podman".into(), "skopeo".into()],
                    optional_installed: vec![],
                },
            ],
        );

        let mut ctx = RenderContext::default();
        ctx.group_states
            .insert("Development Tools".into(), GroupRenderState::Renderable);
        ctx.group_states.insert(
            "Container Management".into(),
            GroupRenderState::Degraded {
                reason: DegradationReason::MemberExcluded,
            },
        );

        let output = render_containerfile(&snap, None, Some(&ctx));

        // Only the renderable group appears in group section
        assert!(
            output.contains("Package Groups (1)"),
            "must have Package Groups (1) — only Development Tools is renderable"
        );
        assert!(
            output.contains("\"Development Tools\""),
            "must contain Development Tools in group install"
        );
        // Container Management must not appear in the dnf group install block
        let group_install_block = output
            .find("dnf group install")
            .map(|pos| {
                &output[pos..output[pos..]
                    .find("dnf clean all")
                    .map_or(output.len(), |end| pos + end)]
            })
            .unwrap_or("");
        assert!(
            !group_install_block.contains("Container Management"),
            "Container Management is degraded, must not appear in dnf group install block"
        );
        // But it SHOULD appear in the degraded provenance comment
        assert!(
            output.contains("# \"Container Management\" degraded (member excluded): members rendered individually"),
            "degraded group must have provenance comment"
        );

        // Renderable group members suppressed
        assert!(
            !output.contains("    gcc "),
            "gcc suppressed (renderable group member)"
        );
        assert!(
            !output.contains("    make "),
            "make suppressed (renderable group member)"
        );

        // Degraded group members render individually
        assert!(
            output.contains("podman"),
            "podman must render individually (degraded group)"
        );
        assert!(
            output.contains("skopeo"),
            "skopeo must render individually (degraded group)"
        );
    }

    // --- Provenance comment annotation tests (Task 15) ---

    #[test]
    fn test_containerfile_ungrouped_no_comment() {
        let snap = snapshot_with_packages_and_groups(
            &["httpd", "gcc", "make"],
            vec![InstalledGroup {
                name: "Dev Tools".into(),
                members: vec!["gcc".into(), "make".into()],
                optional_installed: vec![],
            }],
        );

        let mut ctx = RenderContext::default();
        ctx.group_states
            .insert("Dev Tools".into(), GroupRenderState::Ungrouped);

        let output = render_containerfile(&snap, None, Some(&ctx));

        // Ungrouped groups emit NO provenance comment — members silently
        // join the individual packages section.
        assert!(
            !output.contains("Ungrouped"),
            "ungrouped group must not produce a provenance comment, got:\n{output}"
        );
        // No dnf group install for this group
        assert!(
            !output.contains("dnf group install"),
            "ungrouped group must not produce dnf group install"
        );
        // Members render individually
        assert!(output.contains("gcc"), "gcc must render individually");
        assert!(output.contains("make"), "make must render individually");
    }

    #[test]
    fn test_containerfile_degraded_comment() {
        use inspectah_core::types::group_render::DegradationReason;

        let snap = snapshot_with_packages_and_groups(
            &["httpd", "gcc", "make"],
            vec![InstalledGroup {
                name: "Dev Tools".into(),
                members: vec!["gcc".into(), "make".into()],
                optional_installed: vec![],
            }],
        );

        let mut ctx = RenderContext::default();
        ctx.group_states.insert(
            "Dev Tools".into(),
            GroupRenderState::Degraded {
                reason: DegradationReason::MemberExcluded,
            },
        );

        let output = render_containerfile(&snap, None, Some(&ctx));

        // Degraded comment must appear
        assert!(
            output.contains(
                "# \"Dev Tools\" degraded (member excluded): members rendered individually"
            ),
            "must contain degraded provenance comment, got:\n{output}"
        );
        // Members render individually
        assert!(output.contains("gcc"), "gcc must render individually");
        assert!(output.contains("make"), "make must render individually");
    }

    #[test]
    fn test_containerfile_optional_provenance_comment() {
        // Optional members get a provenance comment AND appear individually
        // in the packages section — they are never suppressed because
        // `dnf group install` does not install optional members.
        let snap = snapshot_with_packages_and_groups(
            &["httpd", "gcc", "gdb", "valgrind"],
            vec![InstalledGroup {
                name: "Dev Tools".into(),
                members: vec!["gcc".into()],
                optional_installed: vec!["gdb".into(), "valgrind".into()],
            }],
        );

        let mut ctx = RenderContext::default();
        ctx.group_states
            .insert("Dev Tools".into(), GroupRenderState::Renderable);

        let output = render_containerfile(&snap, None, Some(&ctx));

        // Optional members comment must appear
        assert!(
            output.contains("# Optional members from \"Dev Tools\""),
            "must contain optional provenance comment, got:\n{output}"
        );
        // Optional members MUST appear in individual packages
        assert!(
            output.contains("gdb"),
            "gdb must appear individually (optional member not installed by group install)"
        );
        assert!(
            output.contains("valgrind"),
            "valgrind must appear individually (optional member not installed by group install)"
        );
    }

    #[test]
    fn test_containerfile_optional_provenance_from_excluded_group() {
        // Optional members are ALWAYS independent — even when the parent
        // group is excluded, optional members that are installed should
        // get a provenance comment.
        let snap = snapshot_with_packages_and_groups(
            &["httpd", "gcc", "gdb"],
            vec![InstalledGroup {
                name: "Dev Tools".into(),
                members: vec!["gcc".into()],
                optional_installed: vec!["gdb".into()],
            }],
        );

        let mut ctx = RenderContext::default();
        ctx.group_states
            .insert("Dev Tools".into(), GroupRenderState::Excluded);

        let output = render_containerfile(&snap, None, Some(&ctx));

        // Optional provenance comment must still appear even for excluded groups
        assert!(
            output.contains("# Optional members from \"Dev Tools\""),
            "optional provenance comment must appear even for excluded groups, got:\n{output}"
        );
        // gdb renders individually (not suppressed — group is excluded)
        assert!(
            output.contains("gdb"),
            "gdb must render individually when parent group is excluded"
        );
    }

    #[test]
    fn test_containerfile_optional_installed_survive_excluded_parent() {
        // Optional members are independent of parent group state per spec.
        // Even when the parent group is excluded, optional members that are
        // installed must appear individually with provenance.
        let snap = snapshot_with_packages_and_groups(
            &["httpd", "gcc", "gdb", "strace"],
            vec![InstalledGroup {
                name: "Debug Tools".into(),
                members: vec!["gcc".into()],
                optional_installed: vec!["gdb".into(), "strace".into()],
            }],
        );

        let mut ctx = RenderContext::default();
        ctx.group_states
            .insert("Debug Tools".into(), GroupRenderState::Excluded);

        let output = render_containerfile(&snap, None, Some(&ctx));

        // No group install section — group is excluded
        assert!(
            !output.contains("dnf group install"),
            "excluded group must not produce a dnf group install block"
        );
        // Optional members appear individually
        assert!(
            output.contains("gdb"),
            "gdb must appear individually (optional member, parent excluded)"
        );
        assert!(
            output.contains("strace"),
            "strace must appear individually (optional member, parent excluded)"
        );
        // Provenance comment present
        assert!(
            output.contains("# Optional members from \"Debug Tools\""),
            "must have optional provenance comment for excluded group"
        );
        // httpd also appears (non-group package)
        assert!(
            output.contains("httpd"),
            "httpd must remain in individual packages"
        );
    }

    #[test]
    fn test_containerfile_degraded_reason_member_overridden() {
        use inspectah_core::types::group_render::DegradationReason;

        let snap = snapshot_with_packages_and_groups(
            &["gcc", "make"],
            vec![InstalledGroup {
                name: "Dev Tools".into(),
                members: vec!["gcc".into(), "make".into()],
                optional_installed: vec![],
            }],
        );

        let mut ctx = RenderContext::default();
        ctx.group_states.insert(
            "Dev Tools".into(),
            GroupRenderState::Degraded {
                reason: DegradationReason::MemberOverridden,
            },
        );

        let output = render_containerfile(&snap, None, Some(&ctx));
        assert!(
            output.contains(
                "# \"Dev Tools\" degraded (member overridden): members rendered individually"
            ),
            "must show member_overridden reason, got:\n{output}"
        );
    }

    #[test]
    fn test_containerfile_degraded_reason_multilib() {
        use inspectah_core::types::group_render::DegradationReason;

        let snap = snapshot_with_packages_and_groups(
            &["gcc"],
            vec![InstalledGroup {
                name: "Dev Tools".into(),
                members: vec!["gcc".into()],
                optional_installed: vec![],
            }],
        );

        let mut ctx = RenderContext::default();
        ctx.group_states.insert(
            "Dev Tools".into(),
            GroupRenderState::Degraded {
                reason: DegradationReason::MultilibConflict,
            },
        );

        let output = render_containerfile(&snap, None, Some(&ctx));
        assert!(
            output.contains(
                "# \"Dev Tools\" degraded (multilib conflict): members rendered individually"
            ),
            "must show multilib_conflict reason, got:\n{output}"
        );
    }

    // ── Task 16: Excluded group with excluded members emits nothing ──

    #[test]
    fn test_containerfile_excluded_group_emits_nothing() {
        // Snapshot where group members have include: false (truly excluded)
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![
                PackageEntry {
                    name: "httpd".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    source_repo: "appstream".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                },
                PackageEntry {
                    name: "gcc".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    source_repo: "appstream".into(),
                    disposition: FindingKind::excluded(), // member excluded
                    ..Default::default()
                },
                PackageEntry {
                    name: "make".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    source_repo: "appstream".into(),
                    disposition: FindingKind::excluded(), // member excluded
                    ..Default::default()
                },
            ],
            installed_groups: Some(vec![InstalledGroup {
                name: "Development Tools".into(),
                members: vec!["gcc".into(), "make".into()],
                optional_installed: vec![],
            }]),
            ..Default::default()
        });

        let mut ctx = RenderContext::default();
        ctx.group_states
            .insert("Development Tools".into(), GroupRenderState::Excluded);

        let output = render_containerfile(&snap, None, Some(&ctx));

        // No group section at all
        assert!(
            !output.contains("Package Groups"),
            "excluded group must not produce a group section"
        );
        assert!(
            !output.contains("dnf group install"),
            "excluded group must not produce dnf group install"
        );
        // Group name must not appear anywhere in group install context
        assert!(
            !output.contains("\"Development Tools\""),
            "excluded group name must not appear in dnf group install output"
        );
        // Members with include:false must not appear in individual packages
        assert!(
            !output.contains("    gcc"),
            "gcc (include:false) must not appear in individual packages"
        );
        assert!(
            !output.contains("    make"),
            "make (include:false) must not appear in individual packages"
        );
        // Non-member package still renders
        assert!(
            output.contains("httpd"),
            "httpd (non-member, include:true) must still render"
        );
    }

    // ── Task 16b: Overlap-precedence proof matrix ──

    #[test]
    fn test_containerfile_no_duplicate_across_group_and_individual_sections() {
        // A package covered by a renderable group must NOT appear in the
        // individual packages section — even when it would otherwise qualify.
        let snap = snapshot_with_packages_and_groups(
            &["httpd", "gcc", "make"],
            vec![InstalledGroup {
                name: "Development Tools".into(),
                members: vec!["gcc".into(), "make".into()],
                optional_installed: vec![],
            }],
        );

        let mut ctx = RenderContext::default();
        ctx.group_states
            .insert("Development Tools".into(), GroupRenderState::Renderable);

        let output = render_containerfile(&snap, None, Some(&ctx));

        // Group section must exist
        assert!(
            output.contains("Package Groups (1)"),
            "must have Package Groups section"
        );
        assert!(
            output.contains("\"Development Tools\""),
            "must contain group name in dnf group install"
        );
        // Members suppressed from individual packages section
        assert!(
            !output.contains("    gcc "),
            "gcc must not appear in individual packages (covered by renderable group)"
        );
        assert!(
            !output.contains("    make "),
            "make must not appear in individual packages (covered by renderable group)"
        );
        // Non-member still present individually
        assert!(
            output.contains("httpd"),
            "httpd must remain in individual packages (not a group member)"
        );

        // Count: gcc/make must appear exactly once each in the entire output
        // (only inside the group install command, not in the individual section)
        let gcc_count = output.matches("gcc").count();
        assert!(
            gcc_count <= 1,
            "gcc must appear at most once (in group install suppression set), found {gcc_count} occurrences"
        );
    }

    #[test]
    fn test_containerfile_precedence_renderable_suppresses_individual() {
        use inspectah_core::types::group_render::DegradationReason;

        // Package "gcc" belongs to two groups:
        //   - "Development Tools" (Renderable)
        //   - "Build Essentials" (Degraded)
        // The renderable group should suppress gcc from individual packages,
        // even though the degraded group would normally spill its members.
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![
                PackageEntry {
                    name: "httpd".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    source_repo: "appstream".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                },
                PackageEntry {
                    name: "gcc".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    source_repo: "appstream".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                },
                PackageEntry {
                    name: "make".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    source_repo: "appstream".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                },
                PackageEntry {
                    name: "cmake".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    source_repo: "appstream".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                },
            ],
            installed_groups: Some(vec![
                InstalledGroup {
                    name: "Development Tools".into(),
                    members: vec!["gcc".into(), "make".into()],
                    optional_installed: vec![],
                },
                InstalledGroup {
                    name: "Build Essentials".into(),
                    members: vec!["gcc".into(), "cmake".into()],
                    optional_installed: vec![],
                },
            ]),
            ..Default::default()
        });

        let mut ctx = RenderContext::default();
        ctx.group_states
            .insert("Development Tools".into(), GroupRenderState::Renderable);
        ctx.group_states.insert(
            "Build Essentials".into(),
            GroupRenderState::Degraded {
                reason: DegradationReason::MemberExcluded,
            },
        );

        let output = render_containerfile(&snap, None, Some(&ctx));

        // Only Development Tools in group install section
        assert!(
            output.contains("Package Groups (1)"),
            "must have exactly 1 renderable group"
        );
        assert!(
            output.contains("\"Development Tools\""),
            "Development Tools must be in group install"
        );
        // Scope the check to the dnf group install block — the provenance
        // comment legitimately contains "Build Essentials" in quotes.
        let group_install_block = output
            .find("dnf group install")
            .map(|pos| {
                &output[pos..output[pos..]
                    .find("dnf clean all")
                    .map_or(output.len(), |end| pos + end)]
            })
            .unwrap_or("");
        assert!(
            !group_install_block.contains("Build Essentials"),
            "Build Essentials (degraded) must not be in dnf group install block"
        );

        // gcc is a member of the renderable group — must be suppressed
        // from individual packages even though it's also in the degraded group
        assert!(
            !output.contains("    gcc "),
            "gcc must be suppressed: renderable group membership takes precedence"
        );

        // make is only in the renderable group — also suppressed
        assert!(
            !output.contains("    make "),
            "make must be suppressed (renderable group member)"
        );

        // cmake is only in the degraded group — it renders individually
        // (degraded groups spill their members to individual packages)
        assert!(
            output.contains("cmake"),
            "cmake must render individually (only in degraded group, not suppressed)"
        );

        // httpd is not in any group — renders individually
        assert!(
            output.contains("httpd"),
            "httpd must render individually (not a group member)"
        );

        // Degraded provenance comment must appear
        assert!(
            output.contains("# \"Build Essentials\" degraded"),
            "degraded provenance comment must appear for Build Essentials"
        );
    }

    // --- Compose comment block tests ---

    #[test]
    fn compose_comment_block_emitted() {
        use inspectah_core::types::containers::{ComposeFile, ComposeService, ContainerSection};

        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            containers: Some(ContainerSection {
                compose_files: vec![
                    ComposeFile {
                        path: "opt/myapp/docker-compose.yml".to_string(),
                        images: vec![
                            ComposeService {
                                service: "web".to_string(),
                                image: "nginx:latest".to_string(),
                            },
                            ComposeService {
                                service: "db".to_string(),
                                image: "postgres:16".to_string(),
                            },
                        ],
                        disposition: FindingKind::included(),
                        ..Default::default()
                    },
                    ComposeFile {
                        path: "srv/monitoring/compose.yml".to_string(),
                        images: vec![ComposeService {
                            service: "prometheus".to_string(),
                            image: "prom/prometheus".to_string(),
                        }],
                        disposition: FindingKind::included(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            ..Default::default()
        };

        let output = render_containerfile(&snap, None, None);
        assert!(output.contains("Compose stacks detected"));
        assert!(output.contains("quadlet(5)"));
        assert!(output.contains("podman-systemd.unit.5.html"));
        assert!(output.contains("porting-containers-to-systemd-using-podman"));
        assert!(output.contains("opt/myapp/docker-compose.yml (2 services)"));
        assert!(output.contains("srv/monitoring/compose.yml (1 service)"));
        // No COPY or RUN for compose
        assert!(!output.contains("COPY compose/"));
        assert!(!output.contains("RUN compose"));
    }

    #[test]
    fn compose_comment_block_absent_when_no_compose() {
        use inspectah_core::types::containers::ContainerSection;

        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            containers: Some(ContainerSection::default()),
            ..Default::default()
        };

        let output = render_containerfile(&snap, None, None);
        assert!(!output.contains("Compose stacks detected"));
    }

    #[test]
    fn compose_comment_excludes_non_included() {
        use inspectah_core::types::containers::{ComposeFile, ContainerSection};

        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            containers: Some(ContainerSection {
                compose_files: vec![ComposeFile {
                    path: "opt/excluded/docker-compose.yml".to_string(),
                    disposition: FindingKind::excluded(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let output = render_containerfile(&snap, None, None);
        assert!(!output.contains("Compose stacks detected"));
    }

    // --- tmpfiles.d /var directory provisioning tests ---

    use inspectah_core::types::storage::{StorageSection, VarDirBacking, VarDirectory};

    fn var_dir(path: &str, backing: VarDirBacking) -> VarDirectory {
        VarDirectory {
            path: path.into(),
            backing: Some(backing),
            ..Default::default()
        }
    }

    fn var_dir_with_mode(
        path: &str,
        mode: &str,
        uid: u32,
        gid: u32,
        owner: Option<&str>,
        group: Option<&str>,
    ) -> VarDirectory {
        VarDirectory {
            path: path.into(),
            backing: Some(VarDirBacking::Unbacked),
            mode: Some(mode.into()),
            owner_uid: Some(uid),
            owner_gid: Some(gid),
            owner_name: owner.map(String::from),
            group_name: group.map(String::from),
        }
    }

    #[test]
    fn tmpfiles_conf_none_when_no_storage() {
        let snap = InspectionSnapshot::new();
        assert!(render_tmpfiles_conf(&snap).is_none());
    }

    /// A backed directory produces no conf only when the export can
    /// prove it carries the creator. The same directory with nothing to
    /// prove it by is unresolved, not carried: an earlier version of
    /// this test asserted the absence outright, which is the defect
    /// `effective_var_backing` exists to fix.
    #[test]
    fn tmpfiles_conf_none_when_the_owning_package_is_in_the_base() {
        let mut snap = InspectionSnapshot::new();
        snap.storage = Some(StorageSection {
            var_directories: vec![var_dir(
                "/var/log",
                VarDirBacking::RpmOwned {
                    package: "filesystem".into(),
                },
            )],
            ..Default::default()
        });

        // No rpm section: the owner can be neither placed nor ruled out.
        let content = render_tmpfiles_conf(&snap).expect("unresolved produces a FIXME");
        assert!(content.contains("# FIXME: confirm the base image creates /var/log;"));
        assert!(content.contains("#d /var/log 0755 root root -"));
        assert!(!content.lines().any(|l| l.starts_with("d /var/log")));

        // A baseline naming the owner proves the base creates it.
        snap.rpm = Some(RpmSection {
            baseline_package_names: Some(vec!["filesystem".into()]),
            ..Default::default()
        });
        assert!(render_tmpfiles_conf(&snap).is_none());
    }

    #[test]
    fn tmpfiles_conf_none_when_no_mode_data() {
        let mut snap = InspectionSnapshot::new();
        snap.storage = Some(StorageSection {
            var_directories: vec![var_dir("/var/lib/myapp", VarDirBacking::Unbacked)],
            ..Default::default()
        });
        assert!(render_tmpfiles_conf(&snap).is_none());
    }

    #[test]
    fn tmpfiles_conf_root_ownership() {
        let mut snap = InspectionSnapshot::new();
        snap.storage = Some(StorageSection {
            var_directories: vec![var_dir_with_mode(
                "/var/lib/myapp",
                "0755",
                0,
                0,
                Some("root"),
                Some("root"),
            )],
            ..Default::default()
        });
        let content = render_tmpfiles_conf(&snap).expect("should produce content");
        assert_eq!(content, "d /var/lib/myapp 0755 root root -\n");
    }

    #[test]
    fn tmpfiles_conf_materialized_user() {
        use inspectah_core::types::users::UserGroupSection;

        let mut snap = InspectionSnapshot::new();
        snap.storage = Some(StorageSection {
            var_directories: vec![var_dir_with_mode(
                "/var/lib/myapp",
                "0750",
                1001,
                1001,
                Some("myapp"),
                Some("myapp"),
            )],
            ..Default::default()
        });
        snap.users_groups = Some(UserGroupSection {
            users: vec![serde_json::json!({
                "name": "myapp",
                "uid": 1001,
                "gid": 1001,
                "include": true,
                "containerfile_strategy": "useradd"
            })],
            ..Default::default()
        });

        let content = render_tmpfiles_conf(&snap).expect("should produce content");
        assert_eq!(content, "d /var/lib/myapp 0750 myapp myapp -\n");
        // No comment for materialized users — the name is guaranteed
        assert!(!content.contains('#'));
    }

    #[test]
    fn tmpfiles_conf_rpm_provided_user() {
        let mut snap = InspectionSnapshot::new();
        snap.storage = Some(StorageSection {
            var_directories: vec![var_dir_with_mode(
                "/var/lib/nginx",
                "0700",
                996,
                996,
                Some("nginx"),
                Some("nginx"),
            )],
            ..Default::default()
        });
        snap.rpm = Some(RpmSection {
            packages_added: vec![PackageEntry {
                name: "nginx".into(),
                state: PackageState::Added,
                disposition: FindingKind::included(),
                source_repo: "appstream".into(),
                ..Default::default()
            }],
            ..Default::default()
        });

        let content = render_tmpfiles_conf(&snap).expect("should produce content");
        assert!(content.contains("# created by package: nginx"));
        assert!(content.contains("d /var/lib/nginx 0700 nginx nginx -"));
    }

    #[test]
    fn tmpfiles_conf_unguaranteed_name_uses_numeric() {
        let mut snap = InspectionSnapshot::new();
        snap.storage = Some(StorageSection {
            var_directories: vec![var_dir_with_mode(
                "/var/lib/someapp",
                "0750",
                500,
                500,
                Some("someapp"),
                Some("someapp"),
            )],
            ..Default::default()
        });
        // No RPMs replicated, no user materialization
        let content = render_tmpfiles_conf(&snap).expect("should produce content");
        assert!(
            content.contains("# account 'someapp' not guaranteed in image"),
            "must have warning comment: {content}"
        );
        assert!(
            content.contains("d /var/lib/someapp 0750 500 500 -"),
            "must use numeric UID:GID: {content}"
        );
    }

    #[test]
    fn tmpfiles_conf_no_name_uses_numeric_no_comment() {
        let mut snap = InspectionSnapshot::new();
        snap.storage = Some(StorageSection {
            var_directories: vec![VarDirectory {
                path: "/var/lib/thing".into(),
                backing: Some(VarDirBacking::Unbacked),
                mode: Some("0755".into()),
                owner_uid: Some(999),
                owner_gid: Some(999),
                owner_name: None,
                group_name: None,
            }],
            ..Default::default()
        });

        let content = render_tmpfiles_conf(&snap).expect("should produce content");
        assert_eq!(content, "d /var/lib/thing 0755 999 999 -\n");
        assert!(!content.contains('#'), "no comment when no name available");
    }

    #[test]
    fn tmpfiles_conf_non_default_modes() {
        let mut snap = InspectionSnapshot::new();
        snap.storage = Some(StorageSection {
            var_directories: vec![
                var_dir_with_mode("/var/lib/sticky", "1777", 0, 0, Some("root"), Some("root")),
                var_dir_with_mode("/var/lib/sgid", "2770", 0, 0, Some("root"), Some("root")),
                var_dir_with_mode("/var/lib/tight", "0700", 0, 0, Some("root"), Some("root")),
            ],
            ..Default::default()
        });

        let content = render_tmpfiles_conf(&snap).expect("should produce content");
        assert!(content.contains("d /var/lib/sticky 1777 root root -"));
        assert!(content.contains("d /var/lib/sgid 2770 root root -"));
        assert!(content.contains("d /var/lib/tight 0700 root root -"));
    }

    #[test]
    fn tmpfiles_conf_comments_on_separate_lines() {
        let mut snap = InspectionSnapshot::new();
        snap.storage = Some(StorageSection {
            var_directories: vec![var_dir_with_mode(
                "/var/lib/orphan",
                "0750",
                1234,
                1234,
                Some("orphan"),
                Some("orphan"),
            )],
            ..Default::default()
        });

        let content = render_tmpfiles_conf(&snap).expect("should produce content");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2, "comment + entry = 2 lines");
        assert!(
            lines[0].starts_with('#'),
            "first line must be a comment: {}",
            lines[0]
        );
        assert!(
            lines[1].starts_with("d "),
            "second line must be the entry: {}",
            lines[1]
        );
        // No inline comments
        let entry_line = lines[1];
        assert!(
            !entry_line.contains('#'),
            "no inline comments in tmpfiles.d entries"
        );
    }

    #[test]
    fn tmpfiles_mixed_output_three_dirs() {
        // Critical mixed-data test from the brief:
        // 3 unbacked dirs:
        //   1. full metadata (mode + owner) -> tmpfiles entry
        //   2. mode but no owner name -> tmpfiles with numeric fallback
        //   3. no mode at all -> RUN mkdir fallback
        let mut snap = InspectionSnapshot::new();
        snap.storage = Some(StorageSection {
            var_directories: vec![
                // Dir 1: full metadata, root-owned
                var_dir_with_mode("/var/lib/full", "0755", 0, 0, Some("root"), Some("root")),
                // Dir 2: mode present, no owner name -> numeric
                VarDirectory {
                    path: "/var/lib/numeric".into(),
                    backing: Some(VarDirBacking::Unbacked),
                    mode: Some("0700".into()),
                    owner_uid: Some(500),
                    owner_gid: Some(500),
                    owner_name: None,
                    group_name: None,
                },
                // Dir 3: no mode -> mkdir fallback
                var_dir("/var/lib/nomode", VarDirBacking::Unbacked),
            ],
            ..Default::default()
        });

        // Verify tmpfiles conf has entries for dir 1 and dir 2
        let content = render_tmpfiles_conf(&snap).expect("should produce content");
        assert!(
            content.contains("d /var/lib/full 0755 root root -"),
            "dir 1 must be in tmpfiles"
        );
        assert!(
            content.contains("d /var/lib/numeric 0700 500 500 -"),
            "dir 2 must be in tmpfiles with numeric"
        );
        assert!(
            !content.contains("/var/lib/nomode"),
            "dir 3 must NOT be in tmpfiles"
        );

        // Verify the Containerfile renders 1 RUN mkdir for dir 3 and a COPY for tmpfiles
        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains(
                "COPY config/usr/lib/tmpfiles.d/inspectah-var.conf /usr/lib/tmpfiles.d/inspectah-var.conf"
            ),
            "must have COPY for tmpfiles.d: {output}"
        );
        assert!(
            output.contains("RUN mkdir -p /var/lib/nomode"),
            "dir 3 must get RUN mkdir fallback"
        );
        assert!(
            !output.contains("RUN mkdir -p /var/lib/full"),
            "dir 1 must NOT get RUN mkdir (handled by tmpfiles)"
        );
        assert!(
            !output.contains("RUN mkdir -p /var/lib/numeric"),
            "dir 2 must NOT get RUN mkdir (handled by tmpfiles)"
        );
    }

    #[test]
    fn var_dir_all_have_mode_no_mkdir() {
        let mut snap = InspectionSnapshot::new();
        snap.storage = Some(StorageSection {
            var_directories: vec![var_dir_with_mode(
                "/var/lib/app",
                "0755",
                0,
                0,
                Some("root"),
                Some("root"),
            )],
            ..Default::default()
        });

        let output = render_containerfile(&snap, None, None);
        assert!(
            output.contains("COPY config/usr/lib/tmpfiles.d/inspectah-var.conf"),
            "must emit COPY"
        );
        assert!(
            !output.contains("RUN mkdir"),
            "no mkdir when all dirs have mode"
        );
    }

    #[test]
    fn var_dir_none_have_mode_no_tmpfiles() {
        let mut snap = InspectionSnapshot::new();
        snap.storage = Some(StorageSection {
            var_directories: vec![
                var_dir("/var/lib/a", VarDirBacking::Unbacked),
                var_dir("/var/lib/b", VarDirBacking::Unbacked),
            ],
            ..Default::default()
        });

        let output = render_containerfile(&snap, None, None);
        assert!(
            !output.contains("tmpfiles"),
            "no tmpfiles references when no mode data"
        );
        assert!(output.contains("RUN mkdir -p /var/lib/a"));
        assert!(output.contains("RUN mkdir -p /var/lib/b"));
    }

    /// A `/var` path the export has to provision reaches two emissions
    /// nothing validated: a shell-form `RUN mkdir -p` when the scan
    /// captured no mode, and a tmpfiles.d `d` line when it did. The
    /// first is the dangerous one -- `/bin/sh -c` reads `;` as a command
    /// separator at image build time, as root -- and the second lets a
    /// newline add a directive of its own. Neither emission may carry a
    /// path the allowlist refuses.
    #[test]
    fn var_dir_refused_path_reaches_neither_emission() {
        const SHELL_PATH: &str = "/var/lib/app;id";
        const CONF_PATH: &str = "/var/lib/two\nd /etc/shadow 0777 root root -";

        let mut snap = InspectionSnapshot::new();
        snap.storage = Some(StorageSection {
            var_directories: vec![
                var_dir(SHELL_PATH, VarDirBacking::Unbacked),
                var_dir_with_mode(CONF_PATH, "0755", 0, 0, Some("root"), Some("root")),
            ],
            ..Default::default()
        });

        let output = render_containerfile(&snap, None, None);
        assert!(
            !output
                .lines()
                .any(|l| l.starts_with("RUN ") && l.contains("app;id")),
            "a path with a command separator must not reach a RUN: {output}"
        );
        assert!(
            output.contains("# FIXME: create /var/lib/app;id by hand"),
            "the refusal must name the directory it skipped: {output}"
        );

        let conf = render_tmpfiles_conf(&snap).expect("the refusal lines are content");
        assert!(
            !conf.lines().any(|l| l.starts_with("d /etc/shadow")),
            "a newline in a path must not add a directive: {conf}"
        );
        assert!(
            conf.contains("# FIXME: create /var/lib/two"),
            "the refusal must name the directory it skipped: {conf}"
        );
    }

    /// The active (non-comment, non-blank) lines of an artifact are the
    /// instructions it executes. Asserting on that set, rather than on
    /// the absence of one bad string, is the only assertion that holds
    /// when the injection arrives through a field nobody thought of.
    fn active_lines(output: &str) -> Vec<&str> {
        output
            .lines()
            .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
            .collect()
    }

    /// Timer names reach the Containerfile in inventory comments, FIXMEs
    /// and `systemctl enable`. A newline in one closes its comment, and a
    /// name the config-tree writer refuses names a unit that was never
    /// staged. Neither may add an active line; the ordinary timers beside
    /// them are still enabled.
    #[test]
    fn refused_timer_names_add_no_active_instruction() {
        use inspectah_core::types::scheduled::{
            AtJob, GeneratedTimerUnit, ScheduledTaskSection, SystemdTimer,
        };
        let local = |name: &str| SystemdTimer {
            name: name.into(),
            source: "local".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        };
        let generated = |name: &str, cron_expr: &str| GeneratedTimerUnit {
            name: name.into(),
            cron_expr: cron_expr.into(),
            timer_content: "[Timer]".into(),
            service_content: "[Service]".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        };
        let mut snap = InspectionSnapshot::new();
        snap.scheduled_tasks = Some(ScheduledTaskSection {
            systemd_timers: vec![
                local("ok\nRUN touch /pwned-local\n#"),
                local("cr\rRUN touch /pwned-cr"),
                local("/etc/abs"),
                local("good"),
            ],
            generated_timer_units: vec![
                generated(
                    "../../victim/planted\nRUN touch /pwned-cron\n#",
                    "0 2 * * *",
                ),
                generated("../../../../../victim/planted", "0 2 * * *"),
                generated("../../victim/boot", "@reboot"),
                generated("backup", "0 3 * * *"),
            ],
            at_jobs: vec![AtJob {
                command: "echo hi\nRUN touch /pwned-at".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None, None);
        let active = active_lines(&output);
        for bad in ["pwned", "victim", "/etc/abs"] {
            assert!(
                !active.iter().any(|l| l.contains(bad)),
                "{bad} reached an active line: {active:#?}"
            );
        }
        assert!(
            !output
                .lines()
                .any(|l| l.starts_with('\r') || l.contains("\rRUN")),
            "a carriage return reached the output: {output}"
        );
        assert!(
            active.contains(&"RUN systemctl enable good.timer backup.timer"),
            "the ordinary timers are not enabled: {active:#?}"
        );
        for refused in [
            "/etc/abs",
            "../../../../../victim/planted",
            "../../victim/boot",
        ] {
            assert!(
                output.contains(&format!("# FIXME: timer {refused} not staged")),
                "no FIXME for {refused}: {output}"
            );
        }
    }

    /// `d {path} {mode} {owner} {group} -` has four host-sourced fields
    /// and a newline in any of them ends the record and starts a
    /// directive of its own. Each directory below poisons a different
    /// one: the mode, the owner, the group, the creator a `Provision`
    /// names, and the creator an `Unresolved` names. The assertion is
    /// the whole active-line set of each artifact, because that is the
    /// only form that covers the field nobody listed.
    ///
    /// The owner and the group need separate directories, which is the
    /// correction to the first version of this test. Poisoning both at
    /// once proves less than it looks: the refused owner falls through
    /// to the numeric branch, which emits a numeric group too, and the
    /// group's own check is never reached. `/var/lib/bad-group` keeps a
    /// guaranteed owner so that the group is the only field in doubt.
    #[test]
    fn var_dir_hostile_fields_add_no_directive() {
        // The age field the format string appends lands on the injected
        // line, so this payload leaves no dangling token behind it.
        const PAYLOAD: &str = "\nd /etc/shadow 0777 root root";

        let unbacked =
            |path: &str, mode: String, uid: u32, owner: &str, group: &str| VarDirectory {
                path: path.into(),
                backing: Some(VarDirBacking::Unbacked),
                mode: Some(mode),
                owner_uid: Some(uid),
                owner_gid: Some(uid),
                owner_name: Some(owner.into()),
                group_name: Some(group.into()),
            };

        use inspectah_core::types::users::UserGroupSection;

        let mut snap = InspectionSnapshot::new();
        // `appone` reaches case 2 of `resolve_tmpfiles_ownership`, which
        // is the only path on which the group field is read at all.
        snap.users_groups = Some(UserGroupSection {
            users: vec![serde_json::json!({
                "name": "appone",
                "uid": 1001,
                "gid": 1001,
                "include": true,
                "containerfile_strategy": "useradd"
            })],
            ..Default::default()
        });
        snap.storage = Some(StorageSection {
            var_directories: vec![
                unbacked(
                    "/var/lib/bad-mode",
                    format!("0755{PAYLOAD}"),
                    0,
                    "root",
                    "root",
                ),
                unbacked(
                    "/var/lib/bad-owner",
                    "0750".into(),
                    1001,
                    &format!("appone -{PAYLOAD}"),
                    "appone",
                ),
                unbacked(
                    "/var/lib/bad-group",
                    "0751".into(),
                    1001,
                    "appone",
                    &format!("appone -{PAYLOAD}"),
                ),
                VarDirectory {
                    path: "/var/lib/bad-creator".into(),
                    backing: Some(VarDirBacking::Tmpfiles {
                        conf: format!("/etc/tmpfiles.d/appone.conf{PAYLOAD}"),
                        line: "d /var/lib/bad-creator 0700 root root -".into(),
                    }),
                    mode: Some("0700".into()),
                    owner_uid: Some(0),
                    owner_gid: Some(0),
                    owner_name: Some("root".into()),
                    group_name: Some("root".into()),
                },
                VarDirectory {
                    path: "/var/lib/bad-unit".into(),
                    backing: Some(VarDirBacking::StateDirectory {
                        declared_by: vec![format!(
                            "/usr/lib/systemd/system/appone.service{PAYLOAD}"
                        )],
                    }),
                    mode: Some("0755".into()),
                    owner_uid: Some(0),
                    owner_gid: Some(0),
                    owner_name: Some("root".into()),
                    group_name: Some("root".into()),
                },
            ],
            ..Default::default()
        });

        let conf = render_tmpfiles_conf(&snap).expect("four of the five produce lines");
        assert_eq!(
            active_lines(&conf),
            vec![
                // The refused mode takes the whole line with it; a
                // substituted default would be a guess the operator
                // could not tell from a reading.
                //
                // A refused owner degrades both fields, because the
                // numeric branch is what the renderer already uses when
                // it has no name. A refused group degrades only itself,
                // and that is the case the first version of this test
                // could not reach.
                "d /var/lib/bad-owner 0750 1001 1001 -",
                "d /var/lib/bad-group 0751 appone 1001 -",
                "d /var/lib/bad-creator 0700 root root -",
                // The payload puts a `/` in the declaring path, so the
                // parent is no longer a system unit directory and the
                // declarer is not a unit the manager would load. The
                // export provisions the directory itself, and every
                // field of the line it writes comes from the directory
                // record rather than from the creator.
                "d /var/lib/bad-unit 0755 root root -",
            ],
            "the conf executes exactly the four directives it intends: {conf}"
        );

        let output = render_containerfile(&snap, None, None);
        let active = active_lines(&output);
        assert!(
            !active.iter().any(|l| l.starts_with("d ")),
            "a tmpfiles directive is never an active Containerfile line: {output}"
        );
        assert!(
            !active.iter().any(|l| l.contains("shadow")),
            "no payload reaches an active line: {output}"
        );
    }

    /// A unit-backed directory is excluded from active provisioning in
    /// both artifacts, but only when the export carries the unit. With
    /// no evidence either way it renders a commented line and a FIXME —
    /// never an active one. An earlier version of this test asserted the
    /// unconditional exclusion, which is the defect
    /// `effective_var_backing` exists to fix.
    #[test]
    fn var_dir_unit_backed_dirs_are_never_actively_provisioned() {
        use inspectah_core::types::config::{
            ConfigCategory, ConfigFileEntry, ConfigFileKind, ConfigSection,
        };

        let mut snap = InspectionSnapshot::new();
        snap.storage = Some(StorageSection {
            var_directories: vec![
                var_dir(
                    "/var/log",
                    VarDirBacking::LogsDirectory {
                        declared_by: vec![
                            "/etc/systemd/system/rsyslog.service".into(),
                            "/usr/lib/systemd/system/rsyslog.service".into(),
                        ],
                    },
                ),
                var_dir(
                    "/var/cache",
                    VarDirBacking::CacheDirectory {
                        declared_by: vec![
                            "/etc/systemd/system/dnf-makecache.service".into(),
                            "/usr/lib/systemd/system/dnf-makecache.service".into(),
                        ],
                    },
                ),
                var_dir_with_mode("/var/lib/app", "0755", 0, 0, Some("root"), Some("root")),
            ],
            ..Default::default()
        });

        // Neither unit is a config entry or RPM-owned: unresolved.
        let content = render_tmpfiles_conf(&snap).expect("should produce content");
        for path in ["/var/log", "/var/cache"] {
            assert!(content.contains(&format!("#d {path} 0755 root root -")));
            assert!(
                !content
                    .lines()
                    .any(|l| l.starts_with(&format!("d {path} ")))
            );
        }
        let output = render_containerfile(&snap, None, None);
        assert!(!output.contains("RUN mkdir -p /var/log"));
        assert!(!output.contains("RUN mkdir -p /var/cache"));

        // Both units carried as included config entries: neither
        // artifact mentions the directories at all.
        snap.config = Some(ConfigSection {
            files: ["rsyslog.service", "dnf-makecache.service"]
                .iter()
                .map(|unit| ConfigFileEntry {
                    path: format!("/etc/systemd/system/{unit}"),
                    kind: ConfigFileKind::Unowned,
                    category: ConfigCategory::Other,
                    content: "[Unit]\n".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                })
                .collect(),
        });
        let content = render_tmpfiles_conf(&snap).expect("should produce content");
        assert!(!content.contains("/var/log"), "carried dir excluded");
        assert!(!content.contains("/var/cache"), "carried dir excluded");
    }

    // --- tmpfiles ownership resolution tests ---

    /// Helper: build a snapshot with specific packages and a var directory owned
    /// by the given user, then render tmpfiles.d and return the output.
    fn tmpfiles_for_owner_and_packages(
        owner_name: &str,
        group_name: &str,
        uid: u32,
        gid: u32,
        packages: &[&str],
    ) -> Option<String> {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: packages
                .iter()
                .map(|n| PackageEntry {
                    name: n.to_string(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    source_repo: "appstream".into(),
                    disposition: FindingKind::included(),
                    locked: false,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        });
        snap.storage = Some(StorageSection {
            var_directories: vec![var_dir_with_mode(
                "/var/lib/testdir",
                "0755",
                uid,
                gid,
                Some(owner_name),
                Some(group_name),
            )],
            ..Default::default()
        });
        render_tmpfiles_conf(&snap)
    }

    #[test]
    fn tmpfiles_ownership_mapped_account_postgres() {
        // postgres account is created by postgresql-server, not a package called "postgres"
        let content =
            tmpfiles_for_owner_and_packages("postgres", "postgres", 26, 26, &["postgresql-server"])
                .expect("should produce tmpfiles content");
        assert!(
            content.contains("postgres postgres"),
            "must use symbolic name 'postgres', got: {content}"
        );
        assert!(
            content.contains("created by package: postgresql-server"),
            "comment must name the providing package, got: {content}"
        );
    }

    #[test]
    fn tmpfiles_ownership_mapped_account_named() {
        // named account is created by bind
        let content = tmpfiles_for_owner_and_packages("named", "named", 25, 25, &["bind"])
            .expect("should produce tmpfiles content");
        assert!(
            content.contains("named named"),
            "must use symbolic name 'named', got: {content}"
        );
        assert!(
            content.contains("created by package: bind"),
            "comment must name the providing package, got: {content}"
        );
    }

    #[test]
    fn tmpfiles_ownership_mapped_account_mysql_via_mariadb() {
        // mysql account can come from mariadb-server
        let content =
            tmpfiles_for_owner_and_packages("mysql", "mysql", 27, 27, &["mariadb-server"])
                .expect("should produce tmpfiles content");
        assert!(
            content.contains("mysql mysql"),
            "must use symbolic name 'mysql', got: {content}"
        );
        assert!(
            content.contains("created by package: mariadb-server"),
            "comment must name mariadb-server, got: {content}"
        );
    }

    #[test]
    fn tmpfiles_ownership_direct_match_still_works() {
        // nginx package creates nginx account — direct name==package match
        let content = tmpfiles_for_owner_and_packages("nginx", "nginx", 997, 995, &["nginx"])
            .expect("should produce tmpfiles content");
        assert!(
            content.contains("nginx nginx"),
            "direct match must use symbolic name, got: {content}"
        );
        assert!(
            content.contains("created by package: nginx"),
            "comment must name the package, got: {content}"
        );
    }

    #[test]
    fn tmpfiles_ownership_unknown_account_falls_through() {
        // myapp is not in any mapping and not a package name — must use numeric UID:GID
        let content = tmpfiles_for_owner_and_packages("myapp", "myapp", 5000, 5000, &["httpd"])
            .expect("should produce tmpfiles content");
        assert!(
            content.contains("5000 5000"),
            "unknown account must fall through to numeric UID:GID, got: {content}"
        );
        assert!(
            content.contains("not guaranteed"),
            "must include warning comment, got: {content}"
        );
    }

    #[test]
    fn tmpfiles_ownership_package_present_account_not_mapped() {
        // Package "custom-daemon" is replicated but not in the mapping, and the
        // owner "customd" doesn't match the package name — safe fallthrough
        let content =
            tmpfiles_for_owner_and_packages("customd", "customd", 4999, 4999, &["custom-daemon"])
                .expect("should produce tmpfiles content");
        assert!(
            content.contains("4999 4999"),
            "unmapped account must fall through to numeric, got: {content}"
        );
        assert!(
            content.contains("not guaranteed"),
            "must include warning comment, got: {content}"
        );
    }

    // --- Not carried block -------------------------------------------------

    /// A host with one leaf and five auto packages nothing it needs:
    /// `cronie` owns two enabled units, `firewalld` one, and both arches of
    /// `glibc` are unneeded, so each prints as `name.arch`. Dispositions are
    /// as collected; the not-carried list stays empty until they are set.
    fn not_carried_host() -> InspectionSnapshot {
        use inspectah_core::types::services::ServiceSection;
        let auto = |name: &str, arch: &str| PackageEntry {
            name: name.into(),
            arch: arch.into(),
            state: PackageState::Added,
            source_repo: "baseos".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        };
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![
                auto("httpd", "x86_64"),
                auto("apr", "x86_64"),
                auto("dracut-config-rescue", "x86_64"),
                auto("firewalld", "noarch"),
                auto("cronie", "x86_64"),
                auto("glibc", "x86_64"),
                auto("glibc", "i686"),
            ],
            leaf_packages: Some(vec!["httpd.x86_64".into()]),
            auto_packages: Some(
                [
                    "apr.x86_64",
                    "dracut-config-rescue.x86_64",
                    "firewalld.noarch",
                    "cronie.x86_64",
                    "glibc.x86_64",
                    "glibc.i686",
                ]
                .iter()
                .map(|id| (*id).to_string())
                .collect(),
            ),
            leaf_dep_tree: serde_json::json!({ "httpd.x86_64": ["apr.x86_64"] }),
            file_ownership: vec![
                owns("firewalld", &["/usr/lib/systemd/system/firewalld.service"]),
                owns(
                    "cronie",
                    &[
                        "/usr/lib/systemd/system/crond.service",
                        "/usr/lib/systemd/system/crond-backup.timer",
                    ],
                ),
            ],
            ..Default::default()
        });
        snap.services = Some(ServiceSection {
            enabled_units: vec![
                "firewalld.service".into(),
                "crond.service".into(),
                "crond-backup.timer".into(),
            ],
            ..Default::default()
        });
        snap
    }

    fn scanned_not_carried_host() -> InspectionSnapshot {
        let mut snap = not_carried_host();
        super::super::not_carried::apply_scan_package_defaults(&mut snap);
        snap
    }

    /// The `# Not carried:` block: its header through the line before the
    /// next blank line. Empty when the output has no header.
    fn not_carried_block(output: &str) -> Vec<&str> {
        output
            .lines()
            .skip_while(|l| !l.starts_with("# Not carried:"))
            .take_while(|l| !l.is_empty())
            .collect()
    }

    #[test]
    fn not_carried_block_lists_each_package_after_the_install_lines() {
        let output = render_containerfile(&scanned_not_carried_host(), None, None);

        assert_eq!(
            not_carried_block(&output),
            [
                format!("# Not carried: {NOT_CARRIED_REASON} (5)").as_str(),
                "#   cronie (crond-backup.timer, crond.service were enabled)",
                "#   firewalld (firewalld.service was enabled)",
                "#   dracut-config-rescue",
                "#   glibc.i686",
                "#   glibc.x86_64",
            ],
            "{output}"
        );

        let header = output.find("# Not carried:").expect("block is rendered");
        let install = output
            .find("# === Packages (")
            .expect("the leaf is installed");
        assert!(
            install < header,
            "the block follows the install lines:\n{output}"
        );
        insta::assert_snapshot!(output);
    }

    #[test]
    fn not_carried_block_is_absent_without_a_list() {
        let cases: Vec<(&str, InspectionSnapshot)> = vec![
            (
                "dispositions as collected leave the list empty",
                not_carried_host(),
            ),
            ("no leaf data, so no classification", {
                let mut snap = not_carried_host();
                let rpm = snap.rpm.as_mut().expect("fixture has an rpm section");
                rpm.leaf_packages = None;
                rpm.auto_packages = None;
                super::super::not_carried::apply_scan_package_defaults(&mut snap);
                snap
            }),
        ];
        for (why, snap) in cases {
            let output = render_containerfile(&snap, None, None);
            assert!(!output.contains("Not carried"), "{why}:\n{output}");
        }
    }

    #[test]
    fn not_carried_block_is_comments_only() {
        let snap = scanned_not_carried_host();
        let output = render_containerfile(&snap, None, None);
        let block = not_carried_block(&output);
        assert!(!block.is_empty(), "the fixture lists packages");
        assert!(block.iter().all(|l| l.starts_with('#')), "{output}");

        let listed = super::super::not_carried::not_carried_packages(&snap);
        let installed: Vec<&str> = output
            .lines()
            .filter(|l| !l.starts_with('#'))
            .flat_map(str::split_whitespace)
            .collect();
        for pkg in &listed {
            for name in [pkg.name.clone(), pkg.display_name()] {
                assert!(
                    !installed.contains(&name.as_str()),
                    "{name} is installed:\n{output}"
                );
            }
        }
    }

    /// Refine's leaf default keeps leaves as decided and excludes every
    /// auto package. Set by hand here, it renders the same block as the
    /// scan defaults; the cross-crate check against refine's export lives
    /// in the CLI crate.
    #[test]
    fn not_carried_block_matches_under_leaf_default_dispositions() {
        let mut leaf_default = not_carried_host();
        let rpm = leaf_default
            .rpm
            .as_mut()
            .expect("fixture has an rpm section");
        let leaves = rpm.leaf_packages.clone().expect("fixture has leaf data");
        for pkg in &mut rpm.packages_added {
            if !leaves.contains(&canonical_package_id(&pkg.name, &pkg.arch)) {
                pkg.disposition = FindingKind::excluded();
            }
        }

        let scan = render_containerfile(&scanned_not_carried_host(), None, None);
        let refine_like = render_containerfile(&leaf_default, None, None);
        assert!(!not_carried_block(&scan).is_empty(), "{scan}");
        assert_eq!(not_carried_block(&refine_like), not_carried_block(&scan));
    }

    /// A snapshot field that reaches the block carrying a newline cannot
    /// end the comment: each hostile field still renders as one `#` line,
    /// and no line outside a comment carries the payload.
    #[test]
    fn not_carried_block_keeps_hostile_fields_inside_the_comment() {
        const PAYLOAD: &str = "\nRUN touch /tmp/injected\n#";

        /// Add an auto package nothing needs, so it is in the class.
        fn add_unneeded(snap: &mut InspectionSnapshot, name: &str, arch: &str) {
            let rpm = snap.rpm.as_mut().expect("fixture has an rpm section");
            rpm.packages_added.push(PackageEntry {
                name: name.into(),
                arch: arch.into(),
                state: PackageState::Added,
                source_repo: "baseos".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            });
            rpm.auto_packages
                .as_mut()
                .expect("fixture has auto data")
                .push(format!("{name}.{arch}"));
        }

        let cases: Vec<(&str, InspectionSnapshot)> = vec![
            ("package name", {
                let mut snap = not_carried_host();
                add_unneeded(&mut snap, &format!("unused{PAYLOAD}"), "x86_64");
                snap
            }),
            ("arch of an arch-qualified display name", {
                let mut snap = not_carried_host();
                add_unneeded(&mut snap, "twoarch", "x86_64");
                add_unneeded(&mut snap, "twoarch", &format!("i686{PAYLOAD}"));
                snap
            }),
            ("enabled unit name", {
                let mut snap = not_carried_host();
                let unit = format!("evil{PAYLOAD}.service");
                snap.rpm
                    .as_mut()
                    .expect("fixture has an rpm section")
                    .file_ownership
                    .push(owns(
                        "firewalld",
                        &[format!("/usr/lib/systemd/system/{unit}").as_str()],
                    ));
                snap.services
                    .as_mut()
                    .expect("fixture has a services section")
                    .enabled_units
                    .push(unit);
                snap
            }),
        ];

        for (field, mut snap) in cases {
            super::super::not_carried::apply_scan_package_defaults(&mut snap);
            let listed = super::super::not_carried::not_carried_packages(&snap).len();
            let output = render_containerfile(&snap, None, None);
            let block = not_carried_block(&output);

            assert_eq!(
                block.len(),
                listed + 1,
                "{field}: one header and one line per package:\n{output}"
            );
            assert!(
                block.iter().all(|l| l.starts_with('#')),
                "{field}:\n{output}"
            );
            assert!(
                block.iter().any(|l| l.contains("RUN touch /tmp/injected")),
                "{field}: the payload is listed, escaped:\n{output}"
            );
            assert!(
                output
                    .lines()
                    .filter(|l| l.contains("injected"))
                    .all(|l| l.starts_with('#')),
                "{field}: the payload stays in a comment:\n{output}"
            );
        }
    }
}
