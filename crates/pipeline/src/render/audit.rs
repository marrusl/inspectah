//! Audit report renderer — produces audit-report.md summarizing changes,
//! risks, and recommendations.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::aggregate::VariantSelection;
use inspectah_core::types::completeness::Completeness;
use inspectah_core::types::config::ConfigFileKind;
use inspectah_core::types::finding::ShadowType;
use inspectah_core::types::selinux::FcontextSource;
use inspectah_core::types::sysctl::{
    RuntimeObservation, SysctlDir, SysctlExplanation, SysctlLineRef, SysctlOutcome, SysctlSetting,
    UnknownInput, VendorReplacement,
};
use inspectah_core::types::users::UserGroupDecision;

use super::baseline_fmt;
use super::md_literal::{md_code, md_code_cell};
use super::not_carried::{NotCarriedPackage, export_not_carried_packages};
use super::selinux;
use super::sysctl_report::{
    AGGREGATE_LINE, AS_WRITTEN_GLOB_ASSIGN_LINE, AS_WRITTEN_GLOB_EXCLUDE_LINE, AS_WRITTEN_LABEL,
    AS_WRITTEN_UNPARSED_LINE, AsWritten, CarriageState, DRIFT_INTRO, DRIFT_LABEL, DRIFT_LINE,
    FILE_NOT_WRITTEN, LINK_NOTE, NOT_CARRIED_ADVISORY_LINE, NOT_CARRIED_ADVISORY_VIA_LINE,
    NOT_CARRIED_EXCLUDED_LINE, NOT_CARRIED_EXCLUDED_VIA_LINE, NOT_CARRIED_LABEL,
    NOT_CARRIED_NO_CONFIG_ENTRY_LINE, NOT_CARRIED_REFUSED_LINE, NOT_CARRIED_REFUSED_VIA_LINE,
    NOT_CARRIED_RUNTIME_DIR_LINE, NOT_CARRIED_USR_LOCAL_LIB_LINE, NotCarried, OUTSIDE_LINK_LINE,
    REDACTED_FILE_LINE, REDACTED_RUNTIME_LINE, RUNTIME_NOT_EXPOSED, RUNTIME_SAME, SysctlReport,
    TRUST_LINE, TUNED_ACTIVE_LINE, UNAVAILABLE_LINE, UNAVAILABLE_TAIL, UNKNOWN_INTRO,
    VENDOR_CARRIED_LINE, VENDOR_LABEL, VENDOR_NO_CONFIG_ENTRY_LINE, VENDOR_NOT_CARRIED_LINE,
    VENDOR_VERB_DISABLES, VENDOR_VERB_REPLACES, VendorLine, fill, line_link_target, sysctl_report,
};
use crate::section_group::SectionGroup;

/// Advisory entry for the Advisories section within a group.
#[derive(Debug, Clone)]
struct Advisory {
    path_or_pattern: String,
    rationale: String,
}

/// Finds packages that appear in multiple groups.
/// Returns a map from package name to the list of group names containing it.
fn find_package_overlaps(
    groups: &[inspectah_core::types::rpm::InstalledGroup],
) -> std::collections::BTreeMap<String, Vec<String>> {
    use std::collections::{BTreeMap, HashMap};

    // Map package name -> list of groups containing it
    let mut pkg_to_groups: HashMap<&str, Vec<&str>> = HashMap::new();

    for group in groups {
        for member in &group.members {
            pkg_to_groups
                .entry(member.as_str())
                .or_default()
                .push(group.name.as_str());
        }
        for optional in &group.optional_installed {
            pkg_to_groups
                .entry(optional.as_str())
                .or_default()
                .push(group.name.as_str());
        }
    }

    // Filter to only packages that appear in 2+ groups, sorted by package name
    let mut overlaps = BTreeMap::new();
    for (pkg, groups_list) in pkg_to_groups {
        if groups_list.len() >= 2 {
            let mut sorted_groups: Vec<String> =
                groups_list.iter().map(|s| s.to_string()).collect();
            sorted_groups.sort();
            overlaps.insert(pkg.to_string(), sorted_groups);
        }
    }

    overlaps
}

/// Collect advisories for a given group from the snapshot.
fn collect_group_advisories(snap: &InspectionSnapshot, group: SectionGroup) -> Vec<Advisory> {
    let mut advisories = Vec::new();

    match group {
        SectionGroup::Storage => {
            // Unbacked /var advisory
            if let Some(storage) = &snap.storage
                && let Some(ref unbacked) = storage.unbacked_var_advisory
                && let inspectah_core::types::FindingKind::Advisory { rationale, .. } =
                    &unbacked.disposition
            {
                for path in &unbacked.paths {
                    advisories.push(Advisory {
                        path_or_pattern: path.clone(),
                        rationale: rationale.clone(),
                    });
                }
            }
        }
        SectionGroup::SystemConfig => {
            // Config entries carry their advisory on the disposition, so the
            // include-filtered tables in this group drop them alongside the
            // entries the user excluded deliberately.
            for advisory in super::advisory::config_advisories(snap) {
                advisories.push(Advisory {
                    path_or_pattern: advisory.path.to_string(),
                    rationale: advisory.rationale.to_string(),
                });
            }
        }
        SectionGroup::Services => {
            // Service drop-ins with FullShadow
            if let Some(services) = &snap.units {
                for drop_in in &services.drop_ins {
                    if drop_in.shadow_type == Some(ShadowType::FullShadow)
                        && let Some(ref rationale) = drop_in.shadow_rationale
                    {
                        advisories.push(Advisory {
                            path_or_pattern: drop_in.path.clone(),
                            rationale: rationale.clone(),
                        });
                    }
                }
            }
        }
        _ => {}
    }

    advisories
}

/// Render the audit report markdown from a snapshot.
pub fn render_audit(snap: &InspectionSnapshot) -> String {
    let mut lines = Vec::new();

    lines.push("# Audit Report".into());
    lines.push(String::new());

    // Aggregate summary section
    if let Some(meta) = &snap.aggregate_meta {
        lines.push("## Aggregate Summary".into());
        lines.push(String::new());

        lines.push(format!("- **Label:** {}", meta.label));
        lines.push(format!("- **Host count:** {}", meta.host_count));
        lines.push(format!(
            "- **Aggregate baseline:** {}",
            if meta.baseline_provisional {
                "Provisional (multiple target images detected)"
            } else {
                "Unanimous (all hosts match)"
            }
        ));

        if !meta.hostnames.is_empty() {
            lines.push(String::new());
            lines.push("### Hosts".into());
            lines.push(String::new());
            for hostname in &meta.hostnames {
                lines.push(format!("- {}", md_code(hostname)));
            }
        }

        if !meta.section_host_counts.is_empty() {
            lines.push(String::new());
            lines.push("### Section Coverage".into());
            lines.push(String::new());
            lines.push("| Section | Hosts |".into());
            lines.push("|---------|-------|".into());
            for (section, count) in &meta.section_host_counts {
                lines.push(format!("| {} | {} |", section, count));
            }
        }

        // Count unique paths with variant conflicts across all sections.
        // A path like /etc/foo.conf may have both a Selected and an Alternative
        // entry — that's 1 conflicted path, not 2 entries.
        let mut conflict_paths: std::collections::HashSet<&str> = std::collections::HashSet::new();

        if let Some(config) = &snap.config {
            for f in &config.files {
                if f.variant_selection == VariantSelection::Selected
                    || f.variant_selection == VariantSelection::Alternative
                {
                    conflict_paths.insert(&f.path);
                }
            }
        }

        if let Some(services) = &snap.units {
            for d in &services.drop_ins {
                if d.variant_selection == VariantSelection::Selected
                    || d.variant_selection == VariantSelection::Alternative
                {
                    conflict_paths.insert(&d.path);
                }
            }
        }

        if let Some(containers) = &snap.containers {
            for q in &containers.quadlet_units {
                if q.variant_selection == VariantSelection::Selected
                    || q.variant_selection == VariantSelection::Alternative
                {
                    conflict_paths.insert(&q.path);
                }
            }
            for c in &containers.compose_files {
                if c.variant_selection == VariantSelection::Selected
                    || c.variant_selection == VariantSelection::Alternative
                {
                    conflict_paths.insert(&c.path);
                }
            }
        }

        let conflict_count = conflict_paths.len();

        if conflict_count > 0 {
            lines.push(String::new());
            lines.push(format!(
                "**Variant conflicts:** {} path(s) with multiple content versions across the aggregate",
                conflict_count
            ));
        }

        lines.push(String::new());
    }

    // Incomplete sections warning — distinguish failed (no data) from degraded (partial data)
    let (failed_ids, degraded_ids) = match &snap.completeness {
        Completeness::Complete => (vec![], vec![]),
        Completeness::Partial {
            degraded_sections, ..
        } => (vec![], degraded_sections.clone()),
        Completeness::Incomplete {
            failed_sections,
            degraded_sections,
            ..
        } => (failed_sections.clone(), degraded_sections.clone()),
    };
    let completeness_reason = match &snap.completeness {
        Completeness::Partial { reason, .. } | Completeness::Incomplete { reason, .. } => {
            reason.as_str()
        }
        Completeness::Complete => "",
    };
    if !failed_ids.is_empty() || !degraded_ids.is_empty() {
        lines.push("## Incomplete Sections".into());
        lines.push(String::new());

        if !failed_ids.is_empty() {
            lines.push("### Failed (no data collected)".into());
            lines.push(String::new());
            for id in &failed_ids {
                lines.push(format!("- `{:?}`", id).to_lowercase());
            }
            lines.push(String::new());
        }

        if !degraded_ids.is_empty() {
            lines.push("### Degraded (partial data collected)".into());
            lines.push(String::new());
            for id in &degraded_ids {
                lines.push(format!("- `{:?}`", id).to_lowercase());
            }
            lines.push(String::new());
        }

        let reason = completeness_reason;
        if !reason.is_empty() {
            lines.push(format!("**Reason:** {reason}"));
            lines.push(String::new());
        }

        lines.push(
            "Artifacts generated from this snapshot may be missing data from these sections."
                .into(),
        );
        lines.push(String::new());
    }

    // Baseline comparison section
    let baseline_lines = baseline_fmt::baseline_section_lines(snap);
    if !baseline_lines.is_empty() {
        lines.extend(baseline_lines);
    }

    // OS info
    if let Some(os) = &snap.os_release {
        let name = if os.pretty_name.is_empty() {
            &os.name
        } else {
            &os.pretty_name
        };
        lines.push(format!("**Source system:** {name}"));
        lines.push(String::new());
    }

    // Grouped sections
    for group in SectionGroup::all_in_order() {
        let mut group_lines = Vec::new();

        // Collect section content for this group
        match group {
            SectionGroup::Packages => {
                if let Some(rpm) = &snap.rpm {
                    group_lines.push("### Packages".into());
                    group_lines.push(String::new());
                    render_packages_section(
                        rpm,
                        &super::repoless::InstallRepos::of(snap),
                        &export_not_carried_packages(snap),
                        &mut group_lines,
                    );
                }
            }
            SectionGroup::SystemConfig => {
                render_system_config_sections(snap, &mut group_lines);
            }
            SectionGroup::Services => {
                render_services_sections(snap, &mut group_lines);
            }
            SectionGroup::Identity => {
                render_identity_section(snap, &mut group_lines);
            }
            SectionGroup::Network => {
                render_network_section(snap, &mut group_lines);
            }
            SectionGroup::Storage => {
                render_storage_section(snap, &mut group_lines);
            }
            SectionGroup::Software => {
                render_software_sections(snap, &mut group_lines);
            }
            SectionGroup::Secrets => {
                render_secrets_section(snap, &mut group_lines);
            }
        }

        // Collect advisories for this group
        let advisories = collect_group_advisories(snap, *group);

        // Only emit group if it has content or advisories
        if !group_lines.is_empty() || !advisories.is_empty() {
            lines.push(format!("## {}", group.label()));
            lines.push(String::new());

            // Section content
            lines.extend(group_lines);

            // Advisories subsection
            if !advisories.is_empty() {
                lines.push("### Advisories".into());
                lines.push(String::new());
                for advisory in advisories {
                    lines.push(format!(
                        "- ℹ **{}** — {}",
                        advisory.path_or_pattern, advisory.rationale
                    ));
                }
                lines.push(String::new());
            }
        }
    }

    // Provisioning steps owed for the machine-state files left out.
    lines.extend(super::machine_state::provisioning_markdown(snap));

    // Redactions
    if !snap.redactions.is_empty() {
        lines.push("## Redactions".into());
        lines.push(String::new());
        lines.push(format!(
            "{} item(s) redacted. See `secrets-review.md` for details.",
            snap.redactions.len()
        ));
        lines.push(String::new());
    }

    // Warnings
    if !snap.warnings.is_empty() {
        lines.push("## Warnings".into());
        lines.push(String::new());
        for w in &snap.warnings {
            if !w.message.is_empty() {
                lines.push(format!("- {}", md_code(&w.message)));
            }
        }
        lines.push(String::new());
    }

    lines.join("\n")
}

/// Render the Packages section content (not including the section heading).
/// `not_carried` is the [`export_not_carried_packages`] list for the same
/// snapshot.
fn render_packages_section(
    rpm: &inspectah_core::types::rpm::RpmSection,
    repos: &super::repoless::InstallRepos,
    not_carried: &[NotCarriedPackage],
    lines: &mut Vec<String>,
) {
    let included: usize = rpm
        .packages_added
        .iter()
        .filter(|p| p.disposition.is_included())
        .count();
    if included > 0 {
        lines.push(format!("### Added Packages ({included})"));
        lines.push(String::new());
        lines.push("| Name | Version | Release | Arch | Repo |".into());
        lines.push("|------|---------|---------|------|------|".into());
        for p in &rpm.packages_added {
            if !p.disposition.is_included() {
                continue;
            }
            lines.push(format!(
                "| {} | {} | {} | {} | {} |",
                p.name, p.version, p.release, p.arch, p.source_repo
            ));
        }
        lines.push(String::new());
    }

    // Packages the image leaves out because nothing it installs needs them,
    // in the list's order. Enabled Units is the bare unit list, or `-`;
    // Source Repo is `-` for a fleet entry, whose repo the merge drops.
    if !not_carried.is_empty() {
        lines.push(format!("### Not carried ({})", not_carried.len()));
        lines.push(String::new());
        lines.push("| Package | Source Repo | Enabled Units | Reason |".into());
        lines.push("|---------|-------------|---------------|--------|".into());
        for p in not_carried {
            let units = if p.enabled_units.is_empty() {
                "-".to_string()
            } else {
                p.enabled_units.join(", ")
            };
            let repo = if p.source_repo.is_empty() {
                "-"
            } else {
                p.source_repo.as_str()
            };
            lines.push(format!(
                "| {} | {repo} | {units} | {} |",
                p.display_name(),
                p.reason(),
            ));
        }
        lines.push(String::new());
    }

    // Repo-enabling packages: the sections the scan proved serve each one,
    // and whether this export still carries a definition for any of them.
    // A "no" is the § 3.3 demotion — the package renders repo-less and
    // wants an upload or a re-included repo.
    let repo_enabling: Vec<_> = rpm
        .packages_added
        .iter()
        .filter(|p| p.disposition.is_included() && !p.repo_enabling_from.is_empty())
        .collect();
    if !repo_enabling.is_empty() {
        lines.push(format!(
            "### Repo-enabling Packages ({})",
            repo_enabling.len()
        ));
        lines.push(String::new());
        lines.push("| Package | Serving Repos | Carried by Export |".into());
        lines.push("|---------|---------------|-------------------|".into());
        for p in &repo_enabling {
            let serving = p
                .repo_enabling_from
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let carried = match super::repo_enabling::repo_enabling_export(rpm, repos, p) {
                super::repo_enabling::RepoEnablingExport::Served => "yes",
                _ => "no",
            };
            lines.push(format!("| {} | {} | {} |", p.name, serving, carried));
        }
        lines.push(String::new());
    }

    // GPG keys the scan left to the base image, once per path.
    let base_shipped: std::collections::BTreeMap<&str, String> = rpm
        .gpg_keys
        .iter()
        .filter(|k| !k.disposition.is_included())
        .filter_map(|k| Some((k.path.as_str(), k.base_image_ships_reason()?)))
        .collect();
    if !base_shipped.is_empty() {
        lines.push(format!("### GPG Keys Not Copied ({})", base_shipped.len()));
        lines.push(String::new());
        lines.push("| Key | Reason |".into());
        lines.push("|-----|--------|".into());
        for (path, reason) in &base_shipped {
            lines.push(format!("| {} | {reason} |", path.replace('|', "\\|")));
        }
        lines.push(String::new());
    }

    // Group overlap annotations
    if let Some(ref groups) = rpm.installed_groups {
        let overlaps = find_package_overlaps(groups);
        if !overlaps.is_empty() {
            for (pkg_name, group_names) in overlaps {
                let group_list = group_names
                    .iter()
                    .map(|g| format!("'{}'", g))
                    .collect::<Vec<_>>()
                    .join(" and ");
                lines.push(format!(
                        "**Note:** {} appears in both {} — DNF handles this correctly, no action needed.",
                        md_code(&pkg_name), group_list
                    ));
            }
            lines.push(String::new());
        }
    }

    // Version changes
    if !rpm.version_changes.is_empty() {
        lines.push(format!(
            "### Version Changes ({})",
            rpm.version_changes.len()
        ));
        lines.push(String::new());
        lines.push("| Package | Host Version | Base Version | Direction |".into());
        lines.push("|---------|--------------|--------------|-----------|".into());
        for vc in &rpm.version_changes {
            let dir = serde_json::to_string(&vc.direction)
                .unwrap_or_default()
                .trim_matches('"')
                .to_string();
            lines.push(format!(
                "| {} | {} | {} | {} |",
                vc.name, vc.host_version, vc.base_version, dir
            ));
        }
        lines.push(String::new());
    }

    // Module streams
    let non_baseline: Vec<_> = rpm
        .module_streams
        .iter()
        .filter(|ms| ms.disposition.is_included() && !ms.baseline_match)
        .collect();
    if !non_baseline.is_empty() {
        lines.push(format!("### Module Streams ({})", non_baseline.len()));
        lines.push(String::new());
        for ms in &non_baseline {
            lines.push(format!("- {}:{}", ms.module_name, ms.stream));
        }
        lines.push(String::new());
    }
}

/// Render System Configuration sections (config, kernel_boot, selinux).
fn render_system_config_sections(snap: &InspectionSnapshot, lines: &mut Vec<String>) {
    // Config files
    if let Some(config) = &snap.config
        && !config.files.is_empty()
    {
        lines.push("### Configuration Files".into());
        lines.push(String::new());

        let modified: usize = config
            .files
            .iter()
            .filter(|f| f.disposition.is_included() && f.kind == ConfigFileKind::RpmOwnedModified)
            .count();
        let unowned: usize = config
            .files
            .iter()
            .filter(|f| f.disposition.is_included() && f.kind == ConfigFileKind::Unowned)
            .count();

        if modified > 0 {
            lines.push(format!("#### Modified RPM-Owned Files ({modified})"));
            lines.push(String::new());
            for f in &config.files {
                if !f.disposition.is_included() || f.kind != ConfigFileKind::RpmOwnedModified {
                    continue;
                }
                lines.push(format!("**{}**", md_code(&f.path)));
                lines.push(String::new());
                if let Some(ref diff) = f.diff_against_rpm
                    && !diff.is_empty()
                {
                    lines.push("```diff".into());
                    lines.push(diff.clone());
                    lines.push("```".into());
                    lines.push(String::new());
                }
            }
        }

        if unowned > 0 {
            lines.push(format!("#### Unowned Config Files ({unowned})"));
            lines.push(String::new());
            for f in &config.files {
                if !f.disposition.is_included() || f.kind != ConfigFileKind::Unowned {
                    continue;
                }
                let category = serde_json::to_string(&f.category)
                    .unwrap_or_default()
                    .trim_matches('"')
                    .to_string();
                lines.push(format!("- {} ({})", md_code(&f.path), category));
            }
            lines.push(String::new());
        }
    }

    // Machine state: every class row, carriable and withheld, and every
    // withheld unmanaged file, whether or not any file is carried.
    let machine_state = super::machine_state::audit_table(snap);
    if !machine_state.is_empty() {
        if snap.config.as_ref().is_none_or(|c| c.files.is_empty()) {
            lines.push("### Configuration Files".into());
            lines.push(String::new());
        }
        lines.extend(machine_state);
    }

    // Kernel & Boot: always present, because the sysctl explanation says
    // why it is missing when it is.
    lines.push("### Kernel & Boot".into());
    lines.push(String::new());
    if let Some(kb) = &snap.kernel_boot
        && !kb.cmdline.is_empty()
    {
        lines.push("#### Kernel Command Line".into());
        lines.push(String::new());
        lines.push(md_code(&kb.cmdline));
        lines.push(String::new());
    }
    render_sysctl_section(snap, lines);

    // SELinux
    if let Some(sel) = &snap.selinux {
        let has_content = !sel.mode.is_empty()
            || !sel.custom_modules.is_empty()
            || !sel.disabled_modules.is_empty()
            || !sel.boolean_overrides.is_empty()
            || !sel.boolean_conflicts.is_empty()
            || !sel.fcontext_rules.is_empty();

        if has_content {
            lines.push("### Security & Access Control".into());
            lines.push(String::new());
            lines.push(format!("- **Mode:** {}", sel.mode));
            if !sel.custom_modules.is_empty() {
                lines.push(format!(
                    "- **Custom modules:** {}",
                    sel.custom_modules.len()
                ));
            }
            if !sel.disabled_modules.is_empty() {
                lines.push(format!(
                    "- **Modules disabled with semodule -d:** {}",
                    sel.disabled_modules.len()
                ));
            }
            let persistent_booleans = sel.boolean_overrides.len();
            if persistent_booleans > 0 {
                lines.push(format!(
                    "- **Booleans set persistently:** {persistent_booleans}"
                ));
            }
            if !sel.boolean_conflicts.is_empty() {
                lines.push(format!(
                    "- **Booleans set differently across hosts (not carried):** {}",
                    sel.boolean_conflicts.len()
                ));
            }
            // § 4: decision-bearing rules only. This will not match the
            // Containerfile's own count and must not be made to: the audit
            // lists dispositions, so it counts the excluded rules too, while
            // the Containerfile introduces the block it actually carries.
            // Neither counts an `Unparsed` line, which nothing decided.
            let decisions = sel
                .fcontext_rules
                .iter()
                .filter(|r| selinux::is_decision_bearing(r))
                .count();
            if decisions > 0 {
                lines.push(format!("- **File context rules:** {decisions}"));
            }
            if !sel.fcontext_rules.is_empty() {
                lines.push(format!(
                    "- **File context source:** {}",
                    fcontext_source_text(sel.fcontext_source)
                ));
            }
            if sel.fips_mode {
                lines.push("- **FIPS mode:** enabled".into());
            }
            lines.push(String::new());
            render_fcontext_rules(snap, lines);
        }
    }
}

/// The `#### Sysctl` block (spec § 5.5): the aggregate line, the
/// unavailable line, or the explanation's items 1 to 6. The copy is
/// `sysctl_report`'s; every interpolated value is a literal code span,
/// and table cells escape `|`.
fn render_sysctl_section(snap: &InspectionSnapshot, lines: &mut Vec<String>) {
    lines.push("#### Sysctl".into());
    lines.push(String::new());
    let report = match sysctl_report(snap) {
        SysctlReport::Aggregate => {
            lines.push(AGGREGATE_LINE.into());
            lines.push(String::new());
            return;
        }
        SysctlReport::Unavailable { reason } => {
            let lead = fill(UNAVAILABLE_LINE, &[("reason", &md_code(&reason))]);
            lines.push(format!("{lead} {UNAVAILABLE_TAIL}"));
            lines.push(String::new());
            return;
        }
        SysctlReport::Host(report) => report,
    };

    // Each item's lines are paragraphs of their own, so each starts a
    // line in every Markdown reader.
    let mut paragraph = |text: String| {
        lines.push(text);
        lines.push(String::new());
    };
    let label = |text: &str| format!("**{text}**");
    paragraph(TRUST_LINE.into());

    match &report.explanation {
        SysctlExplanation::Resolved(settings) => {
            if !settings.is_empty() {
                let mut table = vec![
                    "| Key | File value | Runtime value | Source |".to_string(),
                    "|-----|------------|---------------|--------|".to_string(),
                ];
                table.extend(settings.iter().map(|setting| sysctl_row(snap, setting)));
                paragraph(table.join("\n"));
            }
            if !report.drift.is_empty() {
                paragraph(label(DRIFT_LABEL));
                let intro = match &report.tuned_active {
                    Some(profile) => format!(
                        "{DRIFT_INTRO} {}",
                        fill(TUNED_ACTIVE_LINE, &[("profile", &md_code(profile))])
                    ),
                    None => DRIFT_INTRO.to_string(),
                };
                paragraph(intro);
                for drift in &report.drift {
                    paragraph(fill(
                        DRIFT_LINE,
                        &[
                            ("key", &md_code(drift.key.as_str())),
                            ("runtime", &md_code(&drift.runtime)),
                            ("file_value", &md_code(&drift.file_value)),
                            ("source", &md_code(&line_ref_text(&drift.from))),
                        ],
                    ));
                }
            }
        }
        SysctlExplanation::Unknown(inputs) => {
            paragraph(UNKNOWN_INTRO.into());
            for input in inputs {
                paragraph(match input {
                    UnknownInput::OutsideLink { file, target } => fill(
                        OUTSIDE_LINK_LINE,
                        &[
                            ("path", &md_code(&file.path())),
                            ("target", &md_code(target)),
                        ],
                    ),
                    UnknownInput::RedactedFile { file } => {
                        fill(REDACTED_FILE_LINE, &[("path", &md_code(&file.path()))])
                    }
                    UnknownInput::WithheldFile { file } => fill(
                        super::sysctl_report::WITHHELD_FILE_LINE,
                        &[("path", &md_code(&file.path()))],
                    ),
                    UnknownInput::RedactedRuntime { key } => {
                        fill(REDACTED_RUNTIME_LINE, &[("key", &md_code(key.as_str()))])
                    }
                });
            }
        }
    }

    if !report.vendor_replacements.is_empty() {
        paragraph(label(VENDOR_LABEL));
        for line in &report.vendor_replacements {
            paragraph(vendor_line_text(line));
        }
    }

    if !report.not_carried.is_empty() {
        paragraph(label(NOT_CARRIED_LABEL));
        for file in &report.not_carried {
            paragraph(not_carried_text(file));
        }
    }

    if !report.as_written.is_empty() {
        paragraph(label(AS_WRITTEN_LABEL));
        for line in &report.as_written {
            paragraph(match line {
                AsWritten::GlobAssign { at, pattern, value } => fill(
                    AS_WRITTEN_GLOB_ASSIGN_LINE,
                    &[
                        ("at", &md_code(&line_ref_text(at))),
                        ("pattern", &md_code(pattern)),
                        ("value", &md_code(value)),
                    ],
                ),
                AsWritten::GlobExclude { at, pattern } => fill(
                    AS_WRITTEN_GLOB_EXCLUDE_LINE,
                    &[
                        ("at", &md_code(&line_ref_text(at))),
                        ("pattern", &md_code(pattern)),
                    ],
                ),
                AsWritten::Unparsed { at, text } => fill(
                    AS_WRITTEN_UNPARSED_LINE,
                    &[
                        ("at", &md_code(&line_ref_text(at))),
                        ("text", &md_code(text)),
                    ],
                ),
            });
        }
    }
}

/// `<path>:<line>` of a sysctl line.
fn line_ref_text(at: &SysctlLineRef) -> String {
    format!("{}/{}:{}", at.dir.path(), at.name, at.line_no)
}

/// One row of the sysctl table: key, file value, runtime value, source.
fn sysctl_row(snap: &InspectionSnapshot, setting: &SysctlSetting) -> String {
    let (file_value, runtime, from) = match &setting.outcome {
        SysctlOutcome::Writes {
            value,
            from,
            runtime,
        } => (
            md_code_cell(value),
            match runtime {
                RuntimeObservation::Matches => RUNTIME_SAME.to_string(),
                RuntimeObservation::NotExposed => RUNTIME_NOT_EXPOSED.to_string(),
                RuntimeObservation::Differs { runtime } => md_code_cell(runtime),
            },
            from,
        ),
        SysctlOutcome::Cancelled { by, runtime } => (
            FILE_NOT_WRITTEN.to_string(),
            runtime
                .as_deref()
                .map_or_else(|| RUNTIME_NOT_EXPOSED.to_string(), md_code_cell),
            by,
        ),
    };
    let mut source = md_code_cell(&line_ref_text(from));
    if let Some(target) = line_link_target(snap, from) {
        source.push(' ');
        source.push_str(&fill(LINK_NOTE, &[("target", &md_code_cell(&target))]));
    }
    format!(
        "| {} | {file_value} | {runtime} | {source} |",
        md_code_cell(setting.key.as_str())
    )
}

/// Item 4: an `/etc` file that replaces or masks a vendor file, worded
/// by how it reaches the image.
fn vendor_line_text(line: &VendorLine) -> String {
    let (verb, name) = match &line.action {
        VendorReplacement::Shadows { name } => (VENDOR_VERB_REPLACES, name),
        VendorReplacement::Masks { name } => (VENDOR_VERB_DISABLES, name),
    };
    let template = match line.carriage {
        CarriageState::Carried { .. } => VENDOR_CARRIED_LINE,
        CarriageState::Excluded { .. }
        | CarriageState::Advisory { .. }
        | CarriageState::Refused { .. } => VENDOR_NOT_CARRIED_LINE,
        CarriageState::NoConfigEntry => VENDOR_NO_CONFIG_ENTRY_LINE,
    };
    fill(
        template,
        &[
            (
                "admin",
                &md_code(&format!("{}/{name}", SysctlDir::Etc.path())),
            ),
            ("verb", verb),
            (
                "vendor",
                &md_code(&format!("{}/{name}", SysctlDir::UsrLib.path())),
            ),
        ],
    )
}

/// Item 5: a file the image does not get, with the reason.
fn not_carried_text(file: &NotCarried) -> String {
    let mut reason = None;
    let (template, path, config_path) = match file {
        NotCarried::RuntimeDir { path } => (NOT_CARRIED_RUNTIME_DIR_LINE, path, None),
        NotCarried::UsrLocalLib { path } => (NOT_CARRIED_USR_LOCAL_LIB_LINE, path, None),
        NotCarried::Etc { path, carriage } => match carriage {
            CarriageState::Refused {
                config_path,
                reason: refusal,
            } => {
                reason = Some(refusal.to_string());
                if config_path == path {
                    (NOT_CARRIED_REFUSED_LINE, path, None)
                } else {
                    (NOT_CARRIED_REFUSED_VIA_LINE, path, Some(config_path))
                }
            }
            CarriageState::Excluded { config_path } if config_path == path => {
                (NOT_CARRIED_EXCLUDED_LINE, path, None)
            }
            CarriageState::Excluded { config_path } => {
                (NOT_CARRIED_EXCLUDED_VIA_LINE, path, Some(config_path))
            }
            CarriageState::Advisory { config_path } if config_path == path => {
                (NOT_CARRIED_ADVISORY_LINE, path, None)
            }
            CarriageState::Advisory { config_path } => {
                (NOT_CARRIED_ADVISORY_VIA_LINE, path, Some(config_path))
            }
            // `NotCarried::Etc` never holds `Carried`; the model leaves a
            // carried file out of the list.
            CarriageState::NoConfigEntry | CarriageState::Carried { .. } => {
                (NOT_CARRIED_NO_CONFIG_ENTRY_LINE, path, None)
            }
        },
    };
    let path = md_code(path);
    let config_path = config_path.map(|config_path| md_code(config_path));
    let mut values = vec![("path", path.as_str())];
    if let Some(config_path) = &config_path {
        values.push(("config_path", config_path.as_str()));
    }
    // The refusal is the exporter's own fixed text, not host text.
    if let Some(reason) = &reason {
        values.push(("reason", reason.as_str()));
    }
    fill(template, &values)
}

/// Where the collected rule order came from, and what that costs the reader.
fn fcontext_source_text(source: FcontextSource) -> &'static str {
    match source {
        FcontextSource::PolicyStore => "policy store, in insertion order",
        FcontextSource::SemanageList => {
            "semanage listing, sorted by expression rather than insertion order; where two \
             expressions overlap, the intended winner has to be last"
        }
    }
}

/// § 4's fcontext section: every collected rule and every equivalence the
/// renderer added, each with what the Containerfile's SELinux block does
/// about it.
///
/// The commands are quoted whole rather than summarized. A summary of a
/// guarded command is a claim about behaviour the guard may not perform, and
/// the gap between what the build does and what this report says it does is
/// the only defect this section can have.
fn render_fcontext_rules(snap: &InspectionSnapshot, lines: &mut Vec<String>) {
    let rows = selinux::fcontext_summaries(snap);
    if rows.is_empty() {
        return;
    }

    lines.push("#### File Context Rules".into());
    lines.push(String::new());
    // The per-rule cost is measured, not estimated: on
    // `registry.redhat.io/rhel9/rhel-bootc:9.8` (2026-10-28) three rules
    // added 21.4, 21.0 and 21.0 MB, one layer each. It is inherent to the
    // approach, since each rule is its own `RUN`, so the sentence states it
    // and stops there.
    lines.push(
        "What the Containerfile's SELinux block does with each rule. Every rule it replays \
         runs its own `semanage` command in its own build layer, which costs roughly 21 MB \
         of image size per rule."
            .into(),
    );
    lines.push(String::new());

    for row in &rows {
        lines.push(format!(
            "**{}** ({})",
            md_code(&selinux::summary_label(&row.rule)),
            selinux::summary_disposition(row)
        ));
        lines.push(String::new());
        if let Some(command) = selinux::summary_command(row) {
            lines.push("```sh".into());
            lines.extend(command.lines().map(str::to_string));
            lines.push("```".into());
            lines.push(String::new());
        }
    }
}

/// The audit report's note on the unit commands the Containerfile runs
/// only if their unit files are in the image, followed by the list.
const GUARDED_COMMANDS_NOTE: &str = "**Guarded commands:** dnf may or may not install the \
     packages these units come from, so the Containerfile runs each command only if the unit \
     files it tests are in the image:";

/// Each command the unit plan guards ([`super::unit_plan::PlannedUnit::guard`]),
/// as the unit's name in code and its verb: the default target's
/// `set-default`, then the plan's order.
fn guarded_unit_commands(snap: &InspectionSnapshot) -> Vec<String> {
    let plan = super::unit_plan::plan_units(snap, &super::configtree::staged_writes(snap));
    let target = plan
        .default_target
        .iter()
        .filter(|target| !target.guard().is_empty())
        .map(|target| {
            format!(
                "{} (set-default)",
                md_code(target.name().as_unit_name().as_str())
            )
        });
    let units = plan
        .planned
        .iter()
        .filter(|unit| !unit.guard().is_empty())
        .map(|unit| format!("{} ({})", md_code(unit.name().as_str()), unit.action()));
    target.chain(units).collect()
}

/// Render Services sections (services, scheduled_tasks).
fn render_services_sections(snap: &InspectionSnapshot, lines: &mut Vec<String>) {
    // Unit State Changes
    if let Some(services) = &snap.units
        && !services.state_changes.is_empty()
    {
        lines.push("### Unit State Changes".into());
        lines.push(String::new());
        lines.push("| Unit | Current | Default | Action |".into());
        lines.push("|------|---------|---------|--------|".into());
        for sc in &services.state_changes {
            let state_str = match sc.current_state() {
                inspectah_core::types::units::UnitState::Enabled => "enabled",
                inspectah_core::types::units::UnitState::Disabled => "disabled",
                inspectah_core::types::units::UnitState::Masked => "masked",
            };
            let default_str = match sc.default_state {
                Some(inspectah_core::types::units::PresetDefault::Enable) => "enable",
                Some(inspectah_core::types::units::PresetDefault::Disable) => "disable",
                None => "unknown",
            };
            let action_str = match sc.implied_action() {
                inspectah_core::types::units::UnitAction::Enable => "enable",
                inspectah_core::types::units::UnitAction::Disable => "disable",
                inspectah_core::types::units::UnitAction::Mask => "mask",
            };
            lines.push(format!(
                "| {} | {} | {} | {} |",
                sc.unit().name().as_str(),
                state_str,
                default_str,
                action_str
            ));
        }
        lines.push(String::new());
        let guarded = guarded_unit_commands(snap);
        if !guarded.is_empty() {
            lines.push(format!("{GUARDED_COMMANDS_NOTE} {}.", guarded.join(", ")));
            lines.push(String::new());
        }
    }

    // Scheduled Tasks
    if let Some(st) = &snap.scheduled_tasks {
        let cron_count = st.cron_jobs.len();
        let timer_count = st.systemd_timers.len() + st.generated_timer_units.len();
        let at_count = st.at_jobs.len();

        if cron_count > 0 || timer_count > 0 || at_count > 0 {
            lines.push("### Scheduled Tasks".into());
            lines.push(String::new());
            lines.push(format!("- **Cron jobs:** {cron_count}"));
            lines.push(format!("- **Systemd timers:** {timer_count}"));
            lines.push(format!("- **At jobs:** {at_count}"));

            let reboot_count = st
                .generated_timer_units
                .iter()
                .filter(|g| g.cron_expr == "@reboot")
                .count();
            if reboot_count > 0 {
                lines.push(String::new());
                lines.push(format!(
                    "**Warning:** {} `@reboot` cron job(s) detected. These cannot be converted \
                     to systemd timers and require manual handling \
                     (boot-triggered oneshot service).",
                    reboot_count
                ));
            }
            lines.push(String::new());
        }
    }
}

/// Render Identity section (users_groups).
fn render_identity_section(snap: &InspectionSnapshot, lines: &mut Vec<String>) {
    if let Some(ug) = &snap.users_groups {
        let included: Vec<UserGroupDecision> = ug
            .users
            .iter()
            .filter_map(|v| serde_json::from_value::<UserGroupDecision>(v.clone()).ok())
            .filter(|u| u.disposition.is_included())
            .collect();

        if !included.is_empty() {
            lines.push(format!("### Users & Groups ({})", included.len()));
            lines.push(String::new());
            lines.push("| Name | UID | Shell | Classification | Sudo | SSH Keys |".into());
            lines.push("|------|-----|-------|----------------|------|----------|".into());
            for u in &included {
                let sudo = if u.has_sudo.unwrap_or(false) {
                    "yes"
                } else {
                    "no"
                };
                let ssh = match u.ssh_key_count.unwrap_or(0) {
                    0 => "none".to_string(),
                    1 => "1 key".to_string(),
                    n => format!("{n} keys"),
                };
                lines.push(format!(
                    "| {} | {} | {} | {} | {} | {} |",
                    u.name, u.uid, u.shell, u.classification, sudo, ssh
                ));
            }
            lines.push(String::new());
        }
    }
}

/// Render Storage section.
/// Render Network section — inventory-only items (non-toggleable, never in
/// Containerfile). Includes the ifcfg deprecation note when applicable.
fn render_network_section(snap: &InspectionSnapshot, lines: &mut Vec<String>) {
    let network = match &snap.network {
        Some(n) => n,
        None => return,
    };

    let has_content = !network.connections.is_empty()
        || !network.firewall_zones.is_empty()
        || !network.firewall_direct_rules.is_empty()
        || !network.static_routes.is_empty()
        || !network.hosts_additions.is_empty()
        || !network.proxy.is_empty();

    if !has_content {
        return;
    }

    lines.push("### Inventory".into());
    lines.push(String::new());

    // Contextual note: ifcfg deprecation when source uses legacy network-scripts
    if network.has_ifcfg_connections() {
        let target_is_el9_plus = snap
            .os_release
            .as_ref()
            .map(|os| {
                let major: u32 = os
                    .version_id
                    .split('.')
                    .next()
                    .unwrap_or("0")
                    .parse()
                    .unwrap_or(0);
                major >= 8 && (os.id == "rhel" || os.id == "centos")
            })
            .unwrap_or(false);
        if target_is_el9_plus {
            lines.push(format!(
                "> {}",
                inspectah_core::types::network::IFCFG_DEPRECATION_NOTE
            ));
            lines.push(String::new());
        }
    }

    if !network.connections.is_empty() {
        lines.push(format!(
            "#### NM Connections ({})",
            network.connections.len()
        ));
        lines.push(String::new());
        lines.push("| Name | Type | Method | Path |".into());
        lines.push("|------|------|--------|------|".into());
        for conn in &network.connections {
            lines.push(format!(
                "| {} | {} | {} | {} |",
                conn.name, conn.conn_type, conn.method, conn.path
            ));
        }
        lines.push(String::new());
    }

    if !network.firewall_zones.is_empty() {
        lines.push(format!(
            "#### Firewall Zones ({})",
            network.firewall_zones.len()
        ));
        lines.push(String::new());
        for zone in &network.firewall_zones {
            lines.push(format!(
                "- **{}** — services: {}, ports: {}, rich rules: {}",
                zone.name,
                zone.services.len(),
                zone.ports.len(),
                zone.rich_rules.len()
            ));
        }
        lines.push(String::new());
    }

    if !network.firewall_direct_rules.is_empty() {
        lines.push(format!(
            "#### Firewall Direct Rules ({})",
            network.firewall_direct_rules.len()
        ));
        lines.push(String::new());
        for rule in &network.firewall_direct_rules {
            lines.push(format!(
                "- {} {} chain={} table={}",
                md_code(&rule.args),
                rule.ipv,
                rule.chain,
                rule.table
            ));
        }
        lines.push(String::new());
    }

    if !network.static_routes.is_empty() {
        lines.push(format!(
            "#### Static Routes ({})",
            network.static_routes.len()
        ));
        lines.push(String::new());
        for route in &network.static_routes {
            lines.push(format!("- {}", md_code(&route.path)));
        }
        lines.push(String::new());
    }

    if !network.hosts_additions.is_empty() {
        lines.push(format!(
            "#### /etc/hosts Additions ({})",
            network.hosts_additions.len()
        ));
        lines.push(String::new());
        for entry in &network.hosts_additions {
            lines.push(format!("- {}", md_code(entry)));
        }
        lines.push(String::new());
    }

    if !network.proxy.is_empty() {
        lines.push(format!("#### Proxy Settings ({})", network.proxy.len()));
        lines.push(String::new());
        for proxy in &network.proxy {
            lines.push(format!(
                "- {}: {}",
                md_code(&proxy.source),
                md_code(&proxy.line)
            ));
        }
        lines.push(String::new());
    }
}

fn render_storage_section(snap: &InspectionSnapshot, lines: &mut Vec<String>) {
    if let Some(storage) = &snap.storage {
        let var_dir_rows = var_dir_outcome_rows(snap);
        let has_content = !storage.fstab_entries.is_empty()
            || !storage.lvm_info.is_empty()
            || !storage.credential_refs.is_empty()
            || !var_dir_rows.is_empty();

        if has_content {
            lines.push("### Storage".into());
            lines.push(String::new());

            if !storage.fstab_entries.is_empty() {
                lines.push(format!(
                    "#### Fstab Entries ({})",
                    storage.fstab_entries.len()
                ));
                lines.push(String::new());
                lines.push("| Device | Mount Point | Type | Options |".into());
                lines.push("|--------|-------------|------|---------|".into());
                for entry in &storage.fstab_entries {
                    lines.push(format!(
                        "| {} | {} | {} | {} |",
                        entry.device, entry.mount_point, entry.fstype, entry.options
                    ));
                }
                lines.push(String::new());
            }

            if !storage.lvm_info.is_empty() {
                lines.push(format!("#### LVM Volumes ({})", storage.lvm_info.len()));
                lines.push(String::new());
                lines.push("| LV Name | VG Name | Size |".into());
                lines.push("|---------|---------|------|".into());
                for lv in &storage.lvm_info {
                    lines.push(format!(
                        "| {} | {} | {} |",
                        lv.lv_name, lv.vg_name, lv.lv_size
                    ));
                }
                lines.push(String::new());
            }

            if !storage.credential_refs.is_empty() {
                lines.push(format!(
                    "#### Credential References ({})",
                    storage.credential_refs.len()
                ));
                lines.push(String::new());
                for cr in &storage.credential_refs {
                    lines.push(format!(
                        "- {} references {} (source: {})",
                        md_code(&cr.mount_point),
                        md_code(&cr.credential_path),
                        cr.source
                    ));
                }
                lines.push(String::new());
            }

            if !var_dir_rows.is_empty() {
                lines.push(format!("#### /var Directories ({})", var_dir_rows.len()));
                lines.push(String::new());
                // Two /var lists reach the operator and they answer
                // different questions. Say which this one is, so the
                // difference is visible rather than inferred.
                lines.push(
                    "As the export resolves them. The Storage advisory lists them as found \
                     on the host."
                        .into(),
                );
                lines.push(String::new());
                lines.push("| Directory | Source verdict | Export outcome |".into());
                lines.push("|-----------|----------------|----------------|".into());
                lines.extend(var_dir_rows);
                lines.push(String::new());
            }
        }
    }
}

/// One row per provisioned or unresolved `/var` directory, giving the
/// source host's verdict beside what the export does about it. A
/// `Carried` directory needs no action and gets no row.
fn var_dir_outcome_rows(snap: &InspectionSnapshot) -> Vec<String> {
    use super::var_dirs::{EffectiveBacking, effective_var_backing};
    use inspectah_core::types::storage::{VarDirBacking, declaring_unit_label};

    effective_var_backing(snap)
        .into_iter()
        .filter_map(|o| {
            let verdict = match &o.source {
                None | Some(VarDirBacking::Unbacked) => "unbacked on host".to_string(),
                Some(VarDirBacking::Tmpfiles { conf, .. }) => {
                    format!("created on host by {conf}")
                }
                Some(
                    VarDirBacking::StateDirectory { declared_by }
                    | VarDirBacking::CacheDirectory { declared_by }
                    | VarDirBacking::LogsDirectory { declared_by },
                ) => format!("created on host by {}", declaring_unit_label(declared_by)),
                Some(VarDirBacking::RpmOwned { package }) => {
                    format!("created on host by {package}")
                }
            };
            let outcome = match &o.outcome {
                EffectiveBacking::Carried => return None,
                EffectiveBacking::Provision { lost: None } => "provisioned".to_string(),
                EffectiveBacking::Provision { lost: Some(_) } => {
                    "not carried, provisioned".to_string()
                }
                EffectiveBacking::Unresolved { reason, .. } => {
                    format!("unresolved: {}", reason.reason_text())
                }
            };
            Some(format!(
                "| {} | {verdict} | {outcome} |",
                md_code_cell(&o.path)
            ))
        })
        .collect()
}

/// Render Software sections (non_rpm_software, unmanaged_files).
fn render_software_sections(snap: &InspectionSnapshot, lines: &mut Vec<String>) {
    if let Some(nrs) = &snap.non_rpm_software {
        let item_count = nrs.items.len();
        let env_count = nrs.env_files.len();

        if item_count > 0 || env_count > 0 {
            lines.push("### Non-RPM Software".into());
            lines.push(String::new());

            if item_count > 0 {
                let mut by_method: std::collections::HashMap<String, usize> =
                    std::collections::HashMap::new();
                for item in &nrs.items {
                    *by_method
                        .entry(if item.method.is_empty() {
                            "unknown".to_string()
                        } else {
                            item.method.clone()
                        })
                        .or_insert(0) += 1;
                }
                let mut methods: Vec<_> = by_method.into_iter().collect();
                methods.sort_by_key(|b| std::cmp::Reverse(b.1));
                lines.push(format!("#### Items ({item_count})"));
                lines.push(String::new());
                for (method, count) in &methods {
                    lines.push(format!("- {method}: {count}"));
                }
                lines.push(String::new());
            }

            if env_count > 0 {
                lines.push(format!(
                    "**Warning:** {env_count} `.env` file(s) detected. These are high-probability \
                     secret carriers and require operator review before inclusion."
                ));
                lines.push(String::new());
            }
        }
    }

    let var_srv = super::unmanaged::included_var_srv_paths(snap);
    if !var_srv.is_empty() {
        lines.push("### Unmanaged Files under /srv".into());
        lines.push(String::new());
        lines.push(format!(
            "{} included file(s) under `/srv` have no `COPY` line in the Containerfile. \
             On bootc images `/srv` is a symlink to `/var/srv`, so this content is \
             machine-local data that belongs to the `/var` lifecycle, not image content: \
             a `COPY` would land in `/var/srv` once, on first boot, and no later image \
             update would change it. Carry it with your `/var` strategy, restoring it into \
             `/var/srv` after deployment or giving it its own mount. The files are in the \
             export under `unmanaged/srv/`.",
            var_srv.len()
        ));
        lines.push(String::new());
        lines.extend(var_srv.iter().map(|p| format!("- {}", md_code(p))));
        lines.push(String::new());
    }
}

/// Render Secrets section (secrets, subscription).
fn render_secrets_section(_snap: &InspectionSnapshot, _lines: &mut Vec<String>) {
    // Currently empty - placeholder for future content
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::baseline::{BaselineData, ResolutionStrategy, TargetImageIdentity};
    use inspectah_core::types::FindingKind;
    use inspectah_core::types::rpm::{
        PackageEntry, PackageState, RpmSection, VersionChange, VersionChangeDirection,
    };
    use std::collections::HashMap;

    fn test_target_image() -> TargetImageIdentity {
        TargetImageIdentity {
            image_ref: "quay.io/centos-bootc/centos-bootc:stream9".into(),
            strategy: ResolutionStrategy::OsRelease,
        }
    }

    fn test_baseline() -> BaselineData {
        BaselineData {
            image_digest: "sha256:abc123def456".into(),
            packages: HashMap::new(),
            extracted_at: "2026-05-18T14:32:00Z".into(),
        }
    }

    /// Config-borne advisories are the only advisory source the report has
    /// to dig out of a disposition. Every table in the System Configuration
    /// group filters on `is_included()`, which is false for an advisory, so
    /// without a collector arm the modernization and cross-tree symlink
    /// detectors produce findings that reach no reader.
    #[test]
    fn config_advisories_reach_the_system_configuration_group() {
        use inspectah_core::types::AdvisoryType;
        use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};

        const SYSVINIT_RATIONALE: &str = "sysvinit script — port to a systemd unit";
        const SYMLINK_RATIONALE: &str = "symlink crosses into read-only /usr";

        let mut snap = InspectionSnapshot::new();
        snap.config = Some(ConfigSection {
            withheld: Vec::new(),
            files: vec![
                ConfigFileEntry {
                    path: "/etc/init.d/legacy-app".into(),
                    disposition: FindingKind::advisory(
                        AdvisoryType::Modernization,
                        SYSVINIT_RATIONALE,
                    ),
                    ..Default::default()
                },
                ConfigFileEntry {
                    path: "/etc/alternatives/java".into(),
                    disposition: FindingKind::advisory(
                        AdvisoryType::CrossTreeSymlink,
                        SYMLINK_RATIONALE,
                    ),
                    ..Default::default()
                },
            ],
        });

        let md = render_audit(&snap);

        // Scope to the group: an `### Advisories` subsection elsewhere in
        // the report would satisfy a bare contains().
        let after_heading = md
            .split_once("## System Configuration")
            .map(|(_, rest)| rest)
            .expect("System Configuration group must render");
        let group = after_heading.split("\n## ").next().unwrap_or(after_heading);
        let advisories = group
            .split_once("### Advisories")
            .map(|(_, rest)| rest)
            .expect("System Configuration group must render an Advisories subsection");

        for (path, rationale) in [
            ("/etc/init.d/legacy-app", SYSVINIT_RATIONALE),
            ("/etc/alternatives/java", SYMLINK_RATIONALE),
        ] {
            assert!(
                advisories.contains(path),
                "advisory for {path} must render in the group: {group}"
            );
            assert!(
                advisories.contains(rationale),
                "advisory for {path} must carry its rationale: {group}"
            );
        }
    }

    #[test]
    fn audit_includes_baseline_section() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(test_target_image());
        snap.baseline = Some(test_baseline());
        snap.rpm = Some(RpmSection {
            version_changes: vec![VersionChange {
                name: "glibc".into(),
                direction: VersionChangeDirection::Upgrade,
                ..Default::default()
            }],
            ..Default::default()
        });
        let md = render_audit(&snap);
        assert!(
            md.contains("## Baseline comparison"),
            "audit must have baseline section"
        );
        assert!(md.contains("centos-bootc:stream9"));
        assert!(md.contains("os-release (auto-detected)"));
    }

    #[test]
    fn audit_baseline_absent_when_no_target() {
        let snap = InspectionSnapshot::new();
        let md = render_audit(&snap);
        assert!(!md.contains("Baseline comparison"));
    }

    fn test_snapshot() -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![PackageEntry {
                name: "httpd".into(),
                state: PackageState::Added,
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            ..Default::default()
        });
        snap
    }

    /// The § 4 row: each repo-enabling package, the sections the scan
    /// proved serve it, and whether the export still carries one.
    #[test]
    fn audit_reports_whether_the_export_still_serves_each_repo_enabling_package() {
        use inspectah_core::types::repo::RepoId;
        use inspectah_core::types::rpm::RepoFile;

        let repo_file = |included: bool| RepoFile {
            path: "/etc/yum.repos.d/epel.repo".into(),
            content: "[epel]\nbaseurl=https://example.invalid/epel\nenabled=1\n".into(),
            is_default_repo: false,
            disposition: FindingKind::from_bool(included),
            locked: false,
            aggregate: None,
            gpgkey_references: Default::default(),
            shipped_by_base: None,
        };
        let snapshot_with = |included: bool| {
            let mut snap = InspectionSnapshot::new();
            snap.rpm = Some(RpmSection {
                packages_added: vec![PackageEntry {
                    name: "epel-release".into(),
                    state: PackageState::Added,
                    source_repo: "@commandline".into(),
                    disposition: FindingKind::included(),
                    repo_enabling_from: vec![RepoId::new("epel")],
                    ..Default::default()
                }],
                repo_files: vec![repo_file(included)],
                ..Default::default()
            });
            snap
        };

        for (included, carried) in [(true, "yes"), (false, "no")] {
            let md = render_audit(&snapshot_with(included));
            assert!(
                md.contains("### Repo-enabling Packages (1)"),
                "carried={carried}:\n{md}"
            );
            assert!(
                md.contains(&format!("| epel-release | epel | {carried} |")),
                "carried={carried}:\n{md}"
            );
        }

        // No row at all when nothing is repo-enabling.
        assert!(
            !render_audit(&test_snapshot()).contains("Repo-enabling Packages"),
            "an ordinary package gets no row"
        );
    }

    #[test]
    fn test_audit_report_renders() {
        let snap = test_snapshot();
        let md = render_audit(&snap);
        assert!(md.contains("# Audit Report"));
    }

    #[test]
    fn test_audit_report_packages() {
        let snap = test_snapshot();
        let md = render_audit(&snap);
        assert!(md.contains("## Packages"));
        assert!(md.contains("httpd"));
    }

    #[test]
    fn test_audit_report_empty_snapshot() {
        let snap = InspectionSnapshot::new();
        let md = render_audit(&snap);
        assert!(md.contains("# Audit Report"));
    }

    #[test]
    fn test_audit_report_partial_completeness() {
        use inspectah_core::types::completeness::{Completeness, InspectorId};
        let mut snap = InspectionSnapshot::new();
        snap.completeness = Completeness::Incomplete {
            failed_sections: vec![InspectorId::Config, InspectorId::Units],
            degraded_sections: vec![],
            reason: "timeout during inspection".into(),
        };
        let md = render_audit(&snap);
        assert!(
            md.contains("## Incomplete Sections"),
            "must contain Incomplete Sections heading"
        );
        assert!(md.contains("config"), "must list config section");
        assert!(md.contains("- `units`"), "must list units section");
        assert!(
            md.contains("timeout during inspection"),
            "must include the reason"
        );
    }

    #[test]
    fn test_audit_report_full_completeness_no_section() {
        let mut snap = InspectionSnapshot::new();
        snap.completeness = Completeness::Complete;
        let md = render_audit(&snap);
        assert!(
            !md.contains("Incomplete Sections"),
            "complete status must not produce Incomplete Sections"
        );
    }

    #[test]
    fn test_audit_renders_version_changes_table_when_populated() {
        let snap = InspectionSnapshot {
            rpm: Some(RpmSection {
                version_changes: vec![VersionChange {
                    name: "bash".into(),
                    arch: "x86_64".into(),
                    host_version: "5.2.26-4.el9".into(),
                    base_version: "5.2.26-3.el9".into(),
                    host_epoch: String::new(),
                    base_epoch: String::new(),
                    direction: VersionChangeDirection::Downgrade,
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let report = render_audit(&snap);
        assert!(report.contains("Version Changes"));
        assert!(report.contains("bash"));
    }

    #[test]
    fn test_audit_omits_version_changes_table_when_empty() {
        let snap = InspectionSnapshot {
            rpm: Some(RpmSection::default()),
            ..Default::default()
        };
        let report = render_audit(&snap);
        assert!(!report.contains("Version Changes"));
    }

    #[test]
    fn test_audit_aggregate_summary_section() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use std::collections::BTreeMap;

        let snap = InspectionSnapshot {
            aggregate_meta: Some(AggregateSnapshotMeta {
                label: "web-servers".into(),
                host_count: 3,
                hostnames: vec!["host1".into(), "host2".into(), "host3".into()],
                merged_at: "2026-05-20T12:00:00Z".into(),
                baseline_provisional: true,
                section_host_counts: BTreeMap::from([
                    ("config".into(), 3usize),
                    ("rpm".into(), 3),
                    ("units".into(), 2),
                ]),
            }),
            ..Default::default()
        };

        let report = render_audit(&snap);

        assert!(report.contains("## Aggregate Summary"));
        assert!(report.contains("**Label:** web-servers"));
        assert!(report.contains("**Host count:** 3"));
        assert!(
            report
                .contains("**Aggregate baseline:** Provisional (multiple target images detected)")
        );
        assert!(report.contains("### Hosts"));
        assert!(report.contains("- `host1`"));
        assert!(report.contains("- `host2`"));
        assert!(report.contains("- `host3`"));
        assert!(report.contains("### Section Coverage"));
        assert!(report.contains("| config | 3 |"));
        assert!(report.contains("| rpm | 3 |"));
        assert!(report.contains("| units | 2 |"));
    }

    #[test]
    fn test_audit_aggregate_variant_conflicts() {
        use inspectah_core::types::aggregate::{AggregateSnapshotMeta, VariantSelection};
        use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
        use inspectah_core::types::units::{SystemdDropIn, UnitSection};
        use std::collections::BTreeMap;

        let mut snap = InspectionSnapshot {
            aggregate_meta: Some(AggregateSnapshotMeta {
                label: "test-aggregate".into(),
                host_count: 2,
                hostnames: vec!["host1".into(), "host2".into()],
                merged_at: "2026-05-20T12:00:00Z".into(),
                baseline_provisional: false,
                section_host_counts: BTreeMap::new(),
            }),
            ..Default::default()
        };

        // Add config file with variant conflict
        snap.config = Some(ConfigSection {
            withheld: Vec::new(),
            files: vec![
                ConfigFileEntry {
                    path: "/etc/app.conf".into(),
                    variant_selection: VariantSelection::Selected,
                    ..Default::default()
                },
                ConfigFileEntry {
                    path: "/etc/other.conf".into(),
                    variant_selection: VariantSelection::Only,
                    ..Default::default()
                },
            ],
        });

        // Add service drop-in with variant conflict
        snap.units = Some(UnitSection {
            drop_ins: vec![SystemdDropIn {
                path: "/etc/systemd/system/foo.service.d/override.conf".into(),
                variant_selection: VariantSelection::Alternative,
                ..Default::default()
            }],
            ..Default::default()
        });

        let report = render_audit(&snap);

        assert!(report.contains("**Aggregate baseline:** Unanimous (all hosts match)"));
        assert!(report.contains("**Variant conflicts:** 2 path(s)"));
    }

    #[test]
    fn test_audit_no_aggregate_summary_for_single_host() {
        let snap = InspectionSnapshot::default();
        let report = render_audit(&snap);
        assert!(!report.contains("Aggregate Summary"));
    }

    /// The Containerfile drops `COPY` for `/srv` findings, so the audit
    /// report has to say where they went; an `/opt` finding gets no note.
    #[test]
    fn test_audit_notes_srv_findings_route_to_var() {
        use inspectah_core::types::nonrpm::{UnmanagedFile, UnmanagedFileSection};

        let snap = InspectionSnapshot {
            unmanaged_files: Some(UnmanagedFileSection {
                items: vec![
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
                ],
                ..Default::default()
            }),
            ..Default::default()
        };
        let md = render_audit(&snap);
        assert!(md.contains("### Unmanaged Files under /srv"), "{md}");
        assert!(md.contains("1 included file(s) under `/srv`"), "{md}");
        assert!(md.contains("symlink to `/var/srv`"), "{md}");
        assert!(md.contains("- `/srv/www/index.html`"), "{md}");
        assert!(!md.contains("excluded.html"), "{md}");
        assert!(!md.contains("/opt/app/server"), "{md}");

        let opt_only = InspectionSnapshot {
            unmanaged_files: Some(UnmanagedFileSection {
                items: vec![UnmanagedFile {
                    path: "/opt/app/server".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(!render_audit(&opt_only).contains("under /srv"));
    }

    #[test]
    fn test_audit_users_groups_section() {
        use inspectah_core::types::users::UserGroupSection;

        let mut snap = InspectionSnapshot::new();
        snap.users_groups = Some(UserGroupSection {
            users: vec![serde_json::json!({
                "name": "alice",
                "uid": 1000,
                "gid": 1000,
                "shell": "/bin/bash",
                "home": "/home/alice",
                "disposition": {"kind": "actionable", "include": true},
                "classification": "interactive",
                "containerfile_strategy": "useradd",
                "password_choice": "preserve",
                "has_sudo": true,
                "ssh_key_count": 2
            })],
            ..Default::default()
        });

        let md = render_audit(&snap);
        assert!(
            md.contains("## Users & Groups"),
            "must contain Users & Groups heading"
        );
        assert!(md.contains("alice"), "must contain user name");
        assert!(md.contains("| 1000 |"), "must contain UID");
        assert!(md.contains("| yes |"), "sudo=true must render as yes");
        assert!(
            md.contains("2 keys"),
            "ssh_key_count=2 must render as '2 keys'"
        );
    }

    #[test]
    fn test_audit_users_groups_excludes_password_hash() {
        use inspectah_core::types::users::UserGroupSection;

        let mut snap = InspectionSnapshot::new();
        snap.users_groups = Some(UserGroupSection {
            users: vec![serde_json::json!({
                "name": "bob",
                "uid": 1001,
                "gid": 1001,
                "shell": "/bin/bash",
                "home": "/home/bob",
                "disposition": {"kind": "actionable", "include": true},
                "classification": "interactive",
                "containerfile_strategy": "useradd",
                "password_choice": "preserve",
                "password_hash": "$6$rounds=656000$secret$hash",
                "ssh_keys": ["ssh-ed25519 AAAA_SECRET_KEY_CONTENT"]
            })],
            ..Default::default()
        });

        let md = render_audit(&snap);
        // User name must appear
        assert!(md.contains("bob"), "user name must appear");
        // Sensitive fields must NOT appear
        assert!(
            !md.contains("$6$rounds"),
            "password_hash value must NOT appear in audit"
        );
        assert!(
            !md.contains("AAAA_SECRET_KEY_CONTENT"),
            "ssh_keys content must NOT appear in audit"
        );
    }

    #[test]
    fn test_audit_users_groups_skipped_when_none() {
        let snap = InspectionSnapshot::new();
        let md = render_audit(&snap);
        assert!(
            !md.contains("Users & Groups"),
            "must not render Users & Groups when users_groups is None"
        );
    }

    #[test]
    fn test_audit_users_groups_skipped_when_all_excluded() {
        use inspectah_core::types::users::UserGroupSection;

        let mut snap = InspectionSnapshot::new();
        snap.users_groups = Some(UserGroupSection {
            users: vec![serde_json::json!({
                "name": "excluded_user",
                "uid": 9999,
                "gid": 9999,
                "shell": "/sbin/nologin",
                "home": "/dev/null",
                "disposition": {"kind": "actionable", "include": false},
                "classification": "system",
                "containerfile_strategy": "skip",
                "password_choice": "none"
            })],
            ..Default::default()
        });

        let md = render_audit(&snap);
        assert!(
            !md.contains("Users & Groups"),
            "must not render Users & Groups when all users are excluded"
        );
    }

    #[test]
    fn test_audit_package_overlap_annotations() {
        use inspectah_core::types::rpm::InstalledGroup;

        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![PackageEntry {
                name: "httpd".into(),
                state: PackageState::Added,
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            installed_groups: Some(vec![
                InstalledGroup {
                    name: "Web Server".into(),
                    members: vec!["httpd".into(), "mod_ssl".into()],
                    optional_installed: vec![],
                },
                InstalledGroup {
                    name: "Development Tools".into(),
                    members: vec!["gcc".into(), "httpd".into()],
                    optional_installed: vec![],
                },
                InstalledGroup {
                    name: "Base".into(),
                    members: vec!["bash".into()],
                    optional_installed: vec!["httpd".into()],
                },
            ]),
            ..Default::default()
        });

        let md = render_audit(&snap);

        // httpd appears in all three groups (Web Server members, Development Tools members, Base optional)
        assert!(
            md.contains("`httpd` appears in both"),
            "must annotate httpd overlap"
        );
        assert!(
            md.contains("'Base'")
                && md.contains("'Development Tools'")
                && md.contains("'Web Server'"),
            "must list all groups containing httpd"
        );
        assert!(
            md.contains("DNF handles this correctly, no action needed"),
            "must include reassurance message"
        );

        // mod_ssl and gcc only appear in one group each — no overlap
        assert!(
            !md.contains("`mod_ssl` appears in both"),
            "must not annotate mod_ssl (no overlap)"
        );
        assert!(
            !md.contains("`gcc` appears in both"),
            "must not annotate gcc (no overlap)"
        );
    }

    #[test]
    fn test_audit_no_overlap_annotations_when_no_overlaps() {
        use inspectah_core::types::rpm::InstalledGroup;

        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![PackageEntry {
                name: "httpd".into(),
                state: PackageState::Added,
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            installed_groups: Some(vec![
                InstalledGroup {
                    name: "Web Server".into(),
                    members: vec!["httpd".into()],
                    optional_installed: vec![],
                },
                InstalledGroup {
                    name: "Development Tools".into(),
                    members: vec!["gcc".into()],
                    optional_installed: vec![],
                },
            ]),
            ..Default::default()
        });

        let md = render_audit(&snap);

        // No overlaps — no annotations
        assert!(
            !md.contains("appears in both"),
            "must not show overlap annotations when no overlaps exist"
        );
    }

    #[test]
    fn test_grouped_audit_report_with_advisories() {
        use inspectah_core::types::finding::AdvisoryType;
        use inspectah_core::types::rpm::InstalledGroup;
        use inspectah_core::types::storage::{StorageSection, UnbackedVarAdvisory};
        use inspectah_core::types::units::{SystemdDropIn, UnitSection};

        let mut snap = InspectionSnapshot::new();

        // Packages section
        snap.rpm = Some(RpmSection {
            packages_added: vec![PackageEntry {
                name: "httpd".into(),
                state: PackageState::Added,
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            installed_groups: Some(vec![InstalledGroup {
                name: "Web Server".into(),
                members: vec!["httpd".into()],
                optional_installed: vec![],
            }]),
            ..Default::default()
        });

        // Storage section with unbacked /var advisory
        snap.storage = Some(StorageSection {
            unbacked_var_advisory: Some(UnbackedVarAdvisory {
                disposition: FindingKind::advisory(
                    AdvisoryType::UnbackedVarDir,
                    "No tmpfiles.d backing for ephemeral directories",
                ),
                paths: vec!["/var/lib/foo".into(), "/var/cache/bar".into()],
            }),
            ..Default::default()
        });

        // Services section with FullShadow drop-in
        snap.units = Some(UnitSection {
            drop_ins: vec![SystemdDropIn {
                path: "/etc/systemd/system/httpd.service.d/override.conf".into(),
                disposition: FindingKind::included(),
                shadow_type: Some(ShadowType::FullShadow),
                shadow_rationale: Some(
                    "Full shadow replaces base unit; review for conflicts".into(),
                ),
                ..Default::default()
            }],
            ..Default::default()
        });

        let md = render_audit(&snap);

        // Group headings must be h2
        assert!(
            md.contains("## Packages"),
            "must have Packages group heading"
        );
        assert!(md.contains("## Storage"), "must have Storage group heading");
        assert!(
            md.contains("## Services & Scheduling"),
            "must have Services group heading"
        );

        // Section headings within groups must be h3
        assert!(
            md.contains("### Packages"),
            "must have Packages section heading"
        );

        // Advisories section within groups
        assert!(
            md.contains("### Advisories"),
            "must have Advisories subsection"
        );

        // Unbacked /var advisory entries
        assert!(
            md.contains("- ℹ **/var/lib/foo** — No tmpfiles.d backing for ephemeral directories"),
            "must show unbacked /var advisory for /var/lib/foo"
        );
        assert!(
            md.contains("- ℹ **/var/cache/bar** — No tmpfiles.d backing for ephemeral directories"),
            "must show unbacked /var advisory for /var/cache/bar"
        );

        // FullShadow service advisory
        assert!(
            md.contains("- ℹ **/etc/systemd/system/httpd.service.d/override.conf** — Full shadow replaces base unit; review for conflicts"),
            "must show FullShadow service advisory with rationale"
        );

        insta::assert_snapshot!(md);
    }

    // --- Not carried table -------------------------------------------------

    /// A host with one leaf and five auto packages nothing it needs:
    /// `cronie` owns two enabled units, `firewalld` one, and both arches of
    /// `glibc` are unneeded, so each prints as `name.arch`. Dispositions are
    /// as collected; the not-carried list stays empty until they are set.
    fn not_carried_host() -> InspectionSnapshot {
        use inspectah_core::types::rpm::FileOwnershipEntry;
        use inspectah_core::types::units::{UnitInventoryEntry, UnitSection, UnitState};
        let auto = |name: &str, arch: &str, repo: &str| PackageEntry {
            name: name.into(),
            arch: arch.into(),
            state: PackageState::Added,
            source_repo: repo.into(),
            disposition: FindingKind::included(),
            ..Default::default()
        };
        let owns = |package: &str, units: &[&str]| FileOwnershipEntry {
            package_name: package.into(),
            paths: units
                .iter()
                .map(|unit| format!("/usr/lib/systemd/system/{unit}"))
                .collect(),
        };
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![
                auto("httpd", "x86_64", "appstream"),
                auto("apr", "x86_64", "appstream"),
                auto("dracut-config-rescue", "x86_64", "baseos"),
                auto("firewalld", "noarch", "baseos"),
                auto("cronie", "x86_64", "baseos"),
                auto("glibc", "x86_64", "baseos"),
                auto("glibc", "i686", "baseos"),
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
                owns("firewalld", &["firewalld.service"]),
                owns("cronie", &["crond.service", "crond-backup.timer"]),
            ],
            ..Default::default()
        });
        snap.units = Some(UnitSection {
            inventory: vec![
                UnitInventoryEntry {
                    unit: "firewalld.service"
                        .to_owned()
                        .try_into()
                        .expect("valid unit name"),
                    state: UnitState::Enabled,
                    preset: None,
                },
                UnitInventoryEntry {
                    unit: "crond.service"
                        .to_owned()
                        .try_into()
                        .expect("valid unit name"),
                    state: UnitState::Enabled,
                    preset: None,
                },
                UnitInventoryEntry {
                    unit: "crond-backup.timer"
                        .to_owned()
                        .try_into()
                        .expect("valid unit name"),
                    state: UnitState::Enabled,
                    preset: None,
                },
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

    /// The unit owners of `not_carried_host`, which the scan carries.
    const HOST_UNIT_OWNERS: [&str; 2] = ["cronie.x86_64", "firewalld.noarch"];

    /// The scanned host after an operator excluded both unit owners, so
    /// every not-carried kind is listed.
    fn owners_excluded_host() -> InspectionSnapshot {
        let mut snap = scanned_not_carried_host();
        for pkg in &mut snap
            .rpm
            .as_mut()
            .expect("fixture has an rpm section")
            .packages_added
        {
            if HOST_UNIT_OWNERS.contains(&format!("{}.{}", pkg.name, pkg.arch).as_str()) {
                pkg.disposition = FindingKind::excluded();
            }
        }
        snap
    }

    /// The `### Not carried` table: its heading through the line before the
    /// next blank line after the table. Empty when the report has no heading.
    fn not_carried_table(md: &str) -> Vec<&str> {
        md.lines()
            .skip_while(|l| !l.starts_with("### Not carried"))
            .enumerate()
            .take_while(|(i, l)| *i < 2 || !l.is_empty())
            .map(|(_, l)| l)
            .collect()
    }

    #[test]
    fn not_carried_table_lists_each_package_in_display_order() {
        use super::super::not_carried::NOT_CARRIED_REASON;
        let md = render_audit(&owners_excluded_host());
        let row = |name: &str, units: &str| {
            format!("| {name} | baseos | {units} | {NOT_CARRIED_REASON} |")
        };
        assert_eq!(
            not_carried_table(&md),
            [
                "### Not carried (5)".to_string(),
                String::new(),
                "| Package | Source Repo | Enabled Units | Reason |".into(),
                "|---------|-------------|---------------|--------|".into(),
                row("cronie", "crond-backup.timer, crond.service"),
                row("firewalld", "firewalld.service"),
                row("dracut-config-rescue", "-"),
                row("glibc.i686", "-"),
                row("glibc.x86_64", "-"),
            ],
            "unit owners first, the unit list bare or `-`, display names, the shared reason:\n{md}"
        );

        let packages = md.find("## Packages").expect("Packages group");
        let table = md.find("### Not carried").expect("table is rendered");
        let next_group = md[packages + 1..]
            .find("\n## ")
            .map_or(md.len(), |i| packages + 1 + i);
        assert!(
            packages < table && table < next_group,
            "the table sits in the Packages group:\n{md}"
        );
    }

    #[test]
    fn not_carried_table_is_absent_without_a_list() {
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
            let md = render_audit(&snap);
            assert!(!md.contains("Not carried"), "{why}:\n{md}");
        }
    }

    #[test]
    fn added_packages_no_longer_count_not_carried_packages_on_the_scan_path() {
        let collected = render_audit(&not_carried_host());
        assert!(
            collected.contains("### Added Packages (7)"),
            "as collected, every package counts:\n{collected}"
        );

        let md = render_audit(&scanned_not_carried_host());
        assert!(md.contains("### Added Packages (4)"), "{md}");
        let added: Vec<&str> = md
            .lines()
            .skip_while(|l| !l.starts_with("### Added Packages"))
            .skip(1)
            .take_while(|l| !l.starts_with("###"))
            .collect();
        for (name, carried) in [
            ("cronie", true),
            ("firewalld", true),
            ("dracut-config-rescue", false),
            ("glibc", false),
        ] {
            assert_eq!(
                added.iter().any(|l| l.starts_with(&format!("| {name} |"))),
                carried,
                "{name}: a unit owner is an added package, the rest are not:\n{md}"
            );
        }
    }

    /// Refine's leaf default keeps leaves as decided and excludes every
    /// auto package but the unit owners. Set by hand here, it renders the
    /// same table as the scan defaults; the check against refine's export
    /// lives in the CLI crate.
    #[test]
    fn not_carried_table_matches_under_leaf_default_dispositions() {
        let mut leaf_default = not_carried_host();
        let rpm = leaf_default
            .rpm
            .as_mut()
            .expect("fixture has an rpm section");
        let leaves = rpm.leaf_packages.clone().expect("fixture has leaf data");
        for pkg in &mut rpm.packages_added {
            let id = format!("{}.{}", pkg.name, pkg.arch);
            if !leaves.contains(&id) && !HOST_UNIT_OWNERS.contains(&id.as_str()) {
                pkg.disposition = FindingKind::excluded();
            }
        }

        let scan = render_audit(&scanned_not_carried_host());
        let refine_like = render_audit(&leaf_default);
        assert!(!not_carried_table(&scan).is_empty(), "{scan}");
        assert_eq!(not_carried_table(&refine_like), not_carried_table(&scan));
    }
}
