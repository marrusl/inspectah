//! Audit report renderer — produces audit-report.md summarizing changes,
//! risks, and recommendations.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::aggregate::VariantSelection;
use inspectah_core::types::completeness::Completeness;
use inspectah_core::types::config::ConfigFileKind;
use inspectah_core::types::finding::ShadowType;
use inspectah_core::types::users::UserGroupDecision;

use super::baseline_fmt;
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
            if let Some(services) = &snap.services {
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
                lines.push(format!("- {}", hostname));
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

        if let Some(services) = &snap.services {
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
                    render_packages_section(rpm, &mut group_lines);
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
                lines.push(format!("- {}", w.message));
            }
        }
        lines.push(String::new());
    }

    lines.join("\n")
}

/// Render the Packages section content (not including the section heading).
fn render_packages_section(rpm: &inspectah_core::types::rpm::RpmSection, lines: &mut Vec<String>) {
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
                        "**Note:** `{}` appears in both {} — DNF handles this correctly, no action needed.",
                        pkg_name, group_list
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
                lines.push(format!("**`{}`**", f.path));
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
                lines.push(format!("- `{}` ({})", f.path, category));
            }
            lines.push(String::new());
        }
    }

    // Kernel & Boot
    if let Some(kb) = &snap.kernel_boot {
        let has_content = !kb.cmdline.is_empty()
            || !kb.sysctl_overrides.is_empty()
            || !kb.modules_load_d.is_empty()
            || !kb.modprobe_d.is_empty()
            || !kb.dracut_conf.is_empty()
            || !kb.non_default_modules.is_empty();

        if has_content {
            lines.push("### Kernel & Boot".into());
            lines.push(String::new());

            if !kb.cmdline.is_empty() {
                lines.push("#### Kernel Command Line".into());
                lines.push(String::new());
                lines.push(format!("`{}`", kb.cmdline));
                lines.push(String::new());
            }

            let included_overrides: Vec<_> = kb
                .sysctl_overrides
                .iter()
                .filter(|o| o.disposition.is_included())
                .collect();
            if !included_overrides.is_empty() {
                lines.push(format!(
                    "#### Sysctl Overrides ({})",
                    included_overrides.len()
                ));
                lines.push(String::new());
                lines.push("| Key | Runtime Value | Default Value | Source |".into());
                lines.push("|-----|---------------|---------------|--------|".into());
                for o in &included_overrides {
                    lines.push(format!(
                        "| {} | {} | {} | {} |",
                        o.key, o.runtime, o.default, o.source
                    ));
                }
                lines.push(String::new());
            }

            if !kb.modules_load_d.is_empty() {
                lines.push(format!(
                    "#### Loaded Module Configs ({})",
                    kb.modules_load_d.len()
                ));
                lines.push(String::new());
                for m in &kb.modules_load_d {
                    lines.push(format!("- `{}`", m.path));
                }
                lines.push(String::new());
            }

            if !kb.modprobe_d.is_empty() {
                lines.push(format!("#### Modprobe Configs ({})", kb.modprobe_d.len()));
                lines.push(String::new());
                for m in &kb.modprobe_d {
                    lines.push(format!("- `{}`", m.path));
                }
                lines.push(String::new());
            }

            if !kb.dracut_conf.is_empty() {
                lines.push(format!("#### Dracut Configs ({})", kb.dracut_conf.len()));
                lines.push(String::new());
                for d in &kb.dracut_conf {
                    lines.push(format!("- `{}`", d.path));
                }
                lines.push(String::new());
            }

            if !kb.non_default_modules.is_empty() {
                lines.push(format!(
                    "#### Non-Default Kernel Modules ({})",
                    kb.non_default_modules.len()
                ));
                lines.push(String::new());
                lines.push("| Module | Size | Used By |".into());
                lines.push("|--------|------|---------|".into());
                for m in &kb.non_default_modules {
                    lines.push(format!("| {} | {} | {} |", m.name, m.size, m.used_by));
                }
                lines.push(String::new());
            }
        }
    }

    // SELinux
    if let Some(sel) = &snap.selinux {
        let has_content = !sel.mode.is_empty()
            || !sel.custom_modules.is_empty()
            || !sel.boolean_overrides.is_empty()
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
            let non_default_booleans = sel.boolean_overrides.len();
            if non_default_booleans > 0 {
                lines.push(format!(
                    "- **Non-default booleans:** {non_default_booleans}"
                ));
            }
            if !sel.fcontext_rules.is_empty() {
                lines.push(format!(
                    "- **File context rules:** {}",
                    sel.fcontext_rules.len()
                ));
            }
            if sel.fips_mode {
                lines.push("- **FIPS mode:** enabled".into());
            }
            lines.push(String::new());
        }
    }
}

/// Render Services sections (services, scheduled_tasks).
fn render_services_sections(snap: &InspectionSnapshot, lines: &mut Vec<String>) {
    // Service State Changes
    if let Some(services) = &snap.services
        && !services.state_changes.is_empty()
    {
        lines.push("### Service State Changes".into());
        lines.push(String::new());
        lines.push("| Unit | Current | Default | Action |".into());
        lines.push("|------|---------|---------|--------|".into());
        for sc in &services.state_changes {
            let state_str = match sc.current_state {
                inspectah_core::types::services::ServiceUnitState::Enabled => "enabled",
                inspectah_core::types::services::ServiceUnitState::Disabled => "disabled",
                inspectah_core::types::services::ServiceUnitState::Masked => "masked",
            };
            let default_str = match sc.default_state {
                Some(inspectah_core::types::services::PresetDefault::Enable) => "enable",
                Some(inspectah_core::types::services::PresetDefault::Disable) => "disable",
                None => "unknown",
            };
            let action_str = match sc.implied_action() {
                inspectah_core::types::services::ServiceAction::Enable => "enable",
                inspectah_core::types::services::ServiceAction::Disable => "disable",
                inspectah_core::types::services::ServiceAction::Mask => "mask",
            };
            lines.push(format!(
                "| {} | {} | {} | {} |",
                sc.unit, state_str, default_str, action_str
            ));
        }
        lines.push(String::new());
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
                "- `{}` {} chain={} table={}",
                rule.args, rule.ipv, rule.chain, rule.table
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
            lines.push(format!("- `{}`", route.path));
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
            lines.push(format!("- `{}`", entry));
        }
        lines.push(String::new());
    }

    if !network.proxy.is_empty() {
        lines.push(format!("#### Proxy Settings ({})", network.proxy.len()));
        lines.push(String::new());
        for proxy in &network.proxy {
            lines.push(format!("- `{}`: `{}`", proxy.source, proxy.line));
        }
        lines.push(String::new());
    }
}

fn render_storage_section(snap: &InspectionSnapshot, lines: &mut Vec<String>) {
    if let Some(storage) = &snap.storage {
        let has_content = !storage.fstab_entries.is_empty()
            || !storage.lvm_info.is_empty()
            || !storage.credential_refs.is_empty();

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
                        "- `{}` references `{}` (source: {})",
                        cr.mount_point, cr.credential_path, cr.source
                    ));
                }
                lines.push(String::new());
            }
        }
    }
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
            failed_sections: vec![InspectorId::Config, InspectorId::Services],
            degraded_sections: vec![],
            reason: "timeout during inspection".into(),
        };
        let md = render_audit(&snap);
        assert!(
            md.contains("## Incomplete Sections"),
            "must contain Incomplete Sections heading"
        );
        assert!(md.contains("config"), "must list config section");
        assert!(md.contains("services"), "must list services section");
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
                    ("services".into(), 2),
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
        assert!(report.contains("- host1"));
        assert!(report.contains("- host2"));
        assert!(report.contains("- host3"));
        assert!(report.contains("### Section Coverage"));
        assert!(report.contains("| config | 3 |"));
        assert!(report.contains("| rpm | 3 |"));
        assert!(report.contains("| services | 2 |"));
    }

    #[test]
    fn test_audit_aggregate_variant_conflicts() {
        use inspectah_core::types::aggregate::{AggregateSnapshotMeta, VariantSelection};
        use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
        use inspectah_core::types::services::{ServiceSection, SystemdDropIn};
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
        snap.services = Some(ServiceSection {
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
        use inspectah_core::types::services::{ServiceSection, SystemdDropIn};
        use inspectah_core::types::storage::{StorageSection, UnbackedVarAdvisory};

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
        snap.services = Some(ServiceSection {
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
}
