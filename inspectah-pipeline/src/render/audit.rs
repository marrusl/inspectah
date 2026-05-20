//! Audit report renderer — produces audit-report.md summarizing changes,
//! risks, and recommendations.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::completeness::Completeness;
use inspectah_core::types::config::ConfigFileKind;
use inspectah_core::types::fleet::VariantSelection;

use super::baseline_fmt;

/// Render the audit report markdown from a snapshot.
pub fn render_audit(snap: &InspectionSnapshot) -> String {
    let mut lines = Vec::new();

    lines.push("# Audit Report".into());
    lines.push(String::new());

    // Fleet aggregate summary section
    if let Some(meta) = &snap.fleet_meta {
        lines.push("## Fleet Aggregate Summary".into());
        lines.push(String::new());

        lines.push(format!("- **Label:** {}", meta.label));
        lines.push(format!("- **Host count:** {}", meta.host_count));
        lines.push(format!(
            "- **Fleet baseline:** {}",
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
                "**Variant conflicts:** {} path(s) with multiple content versions across the fleet",
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

    // Packages
    if let Some(rpm) = &snap.rpm {
        lines.push("## Packages".into());
        lines.push(String::new());

        let included: usize = rpm.packages_added.iter().filter(|p| p.include).count();
        if included > 0 {
            lines.push(format!("### Added Packages ({included})"));
            lines.push(String::new());
            lines.push("| Name | Version | Release | Arch | Repo |".into());
            lines.push("|------|---------|---------|------|------|".into());
            for p in &rpm.packages_added {
                if !p.include {
                    continue;
                }
                lines.push(format!(
                    "| {} | {} | {} | {} | {} |",
                    p.name, p.version, p.release, p.arch, p.source_repo
                ));
            }
            lines.push(String::new());
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
            .filter(|ms| ms.include && !ms.baseline_match)
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

    // Config files
    if let Some(config) = &snap.config
        && !config.files.is_empty()
    {
        lines.push("## Configuration Files".into());
        lines.push(String::new());

        let modified: usize = config
            .files
            .iter()
            .filter(|f| f.include && f.kind == ConfigFileKind::RpmOwnedModified)
            .count();
        let unowned: usize = config
            .files
            .iter()
            .filter(|f| f.include && f.kind == ConfigFileKind::Unowned)
            .count();

        if modified > 0 {
            lines.push(format!("### Modified RPM-Owned Files ({modified})"));
            lines.push(String::new());
            for f in &config.files {
                if !f.include || f.kind != ConfigFileKind::RpmOwnedModified {
                    continue;
                }
                lines.push(format!("#### `{}`", f.path));
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
            lines.push(format!("### Unowned Config Files ({unowned})"));
            lines.push(String::new());
            for f in &config.files {
                if !f.include || f.kind != ConfigFileKind::Unowned {
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

    // Services
    if let Some(services) = &snap.services
        && !services.state_changes.is_empty()
    {
        lines.push("## Service State Changes".into());
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

    // Storage
    if let Some(storage) = &snap.storage {
        let has_content = !storage.fstab_entries.is_empty()
            || !storage.lvm_info.is_empty()
            || !storage.credential_refs.is_empty();

        if has_content {
            lines.push("## Storage".into());
            lines.push(String::new());

            if !storage.fstab_entries.is_empty() {
                lines.push(format!(
                    "### Fstab Entries ({})",
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
                lines.push(format!("### LVM Volumes ({})", storage.lvm_info.len()));
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
                    "### Credential References ({})",
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

    // Kernel & Boot
    if let Some(kb) = &snap.kernel_boot {
        let has_content = !kb.cmdline.is_empty()
            || !kb.sysctl_overrides.is_empty()
            || !kb.modules_load_d.is_empty()
            || !kb.modprobe_d.is_empty()
            || !kb.dracut_conf.is_empty()
            || !kb.non_default_modules.is_empty();

        if has_content {
            lines.push("## Kernel & Boot".into());
            lines.push(String::new());

            if !kb.cmdline.is_empty() {
                lines.push("### Kernel Command Line".into());
                lines.push(String::new());
                lines.push(format!("`{}`", kb.cmdline));
                lines.push(String::new());
            }

            let included_overrides: Vec<_> =
                kb.sysctl_overrides.iter().filter(|o| o.include).collect();
            if !included_overrides.is_empty() {
                lines.push(format!(
                    "### Sysctl Overrides ({})",
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
                    "### Loaded Module Configs ({})",
                    kb.modules_load_d.len()
                ));
                lines.push(String::new());
                for m in &kb.modules_load_d {
                    lines.push(format!("- `{}`", m.path));
                }
                lines.push(String::new());
            }

            if !kb.modprobe_d.is_empty() {
                lines.push(format!("### Modprobe Configs ({})", kb.modprobe_d.len()));
                lines.push(String::new());
                for m in &kb.modprobe_d {
                    lines.push(format!("- `{}`", m.path));
                }
                lines.push(String::new());
            }

            if !kb.dracut_conf.is_empty() {
                lines.push(format!("### Dracut Configs ({})", kb.dracut_conf.len()));
                lines.push(String::new());
                for d in &kb.dracut_conf {
                    lines.push(format!("- `{}`", d.path));
                }
                lines.push(String::new());
            }

            if !kb.non_default_modules.is_empty() {
                lines.push(format!(
                    "### Non-Default Kernel Modules ({})",
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

    // Scheduled Tasks
    if let Some(st) = &snap.scheduled_tasks {
        let cron_count = st.cron_jobs.len();
        let timer_count = st.systemd_timers.len() + st.generated_timer_units.len();
        let at_count = st.at_jobs.len();

        if cron_count > 0 || timer_count > 0 || at_count > 0 {
            lines.push("## Scheduled Tasks".into());
            lines.push(String::new());
            lines.push(format!("- **Cron jobs:** {cron_count}"));
            lines.push(format!("- **Systemd timers:** {timer_count}"));
            lines.push(format!("- **At jobs:** {at_count}"));

            // Detect @reboot entries from generated timer units where the
            // real cron expression is stored, not from CronJob.source (which
            // holds the collector source label like "cron.d" or "crontab").
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

    // SELinux
    if let Some(sel) = &snap.selinux {
        let has_content = !sel.mode.is_empty()
            || !sel.custom_modules.is_empty()
            || !sel.boolean_overrides.is_empty()
            || !sel.fcontext_rules.is_empty();

        if has_content {
            lines.push("## Security & Access Control".into());
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

    // Non-RPM Software
    if let Some(nrs) = &snap.non_rpm_software {
        let item_count = nrs.items.len();
        let env_count = nrs.env_files.len();

        if item_count > 0 || env_count > 0 {
            lines.push("## Non-RPM Software".into());
            lines.push(String::new());

            if item_count > 0 {
                // Count by method
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
                lines.push(format!("### Items ({item_count})"));
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

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::baseline::{BaselineData, ResolutionStrategy, TargetImageIdentity};
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
                include: true,
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
        let mut snap = InspectionSnapshot::default();
        let mut rpm = RpmSection::default();
        rpm.version_changes = vec![VersionChange {
            name: "bash".into(),
            arch: "x86_64".into(),
            host_version: "5.2.26-4.el9".into(),
            base_version: "5.2.26-3.el9".into(),
            host_epoch: String::new(),
            base_epoch: String::new(),
            direction: VersionChangeDirection::Downgrade,
        }];
        snap.rpm = Some(rpm);
        let report = render_audit(&snap);
        assert!(report.contains("Version Changes"));
        assert!(report.contains("bash"));
    }

    #[test]
    fn test_audit_omits_version_changes_table_when_empty() {
        let mut snap = InspectionSnapshot::default();
        snap.rpm = Some(RpmSection::default());
        let report = render_audit(&snap);
        assert!(!report.contains("Version Changes"));
    }

    #[test]
    fn test_audit_fleet_summary_section() {
        use inspectah_core::types::fleet::FleetSnapshotMeta;
        use std::collections::BTreeMap;

        let mut snap = InspectionSnapshot::default();
        snap.fleet_meta = Some(FleetSnapshotMeta {
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
        });

        let report = render_audit(&snap);

        assert!(report.contains("## Fleet Aggregate Summary"));
        assert!(report.contains("**Label:** web-servers"));
        assert!(report.contains("**Host count:** 3"));
        assert!(
            report.contains("**Fleet baseline:** Provisional (multiple target images detected)")
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
    fn test_audit_fleet_variant_conflicts() {
        use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
        use inspectah_core::types::fleet::{FleetSnapshotMeta, VariantSelection};
        use inspectah_core::types::services::{ServiceSection, SystemdDropIn};
        use std::collections::BTreeMap;

        let mut snap = InspectionSnapshot::default();
        snap.fleet_meta = Some(FleetSnapshotMeta {
            label: "test-fleet".into(),
            host_count: 2,
            hostnames: vec!["host1".into(), "host2".into()],
            merged_at: "2026-05-20T12:00:00Z".into(),
            baseline_provisional: false,
            section_host_counts: BTreeMap::new(),
        });

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
            ..Default::default()
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

        assert!(report.contains("**Fleet baseline:** Unanimous (all hosts match)"));
        assert!(report.contains("**Variant conflicts:** 2 path(s)"));
    }

    #[test]
    fn test_audit_no_fleet_summary_for_single_host() {
        let snap = InspectionSnapshot::default();
        let report = render_audit(&snap);
        assert!(!report.contains("Fleet Aggregate Summary"));
    }
}
