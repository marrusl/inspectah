//! Containerfile renderer — produces a Containerfile from an InspectionSnapshot.
//!
//! Section order matches Go exactly:
//! 1. FROM + repos + GPG + modules + packages (dnf install -y)
//! 2. Services (enable/disable)
//! 3. Firewall zones
//! 4. Scheduled tasks (timer COPYs)
//! 5. Config files (COPY per top-level dir)
//! 6. Non-RPM software
//! 7. Containers (quadlet COPYs)
//! 8. Users
//! 9. Kernel/boot (kargs.d, sysctl, modules)
//! 10. Security & Access Control (SELinux, FIPS, PAM, audit)
//! 11. Network (routes, hosts, proxy)
//! 12. Secrets comments
//! 13. Epilogue (tmpfiles, RUN bootc container lint)

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::completeness::{Completeness, InspectorId};
use inspectah_core::types::os::SystemType;
use inspectah_core::types::redaction::RedactionKind;
use inspectah_core::types::rpm::PackageEntry;

use super::safety::{is_valid_tuned_profile, operator_kargs, sanitize_shell_value};

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
) -> String {
    let base = base_image_from_snapshot(snap);
    let base_str = base.as_deref().unwrap_or("");
    let mut lines: Vec<String> = Vec::new();

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

    // 1. Packages section (FROM + repos + GPG + modules + packages)
    lines.extend(packages_section_lines(snap, base.as_deref()));

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

    // 8. Users
    if is_degraded(&snap.completeness, InspectorId::UsersGroups) {
        lines.push(
            "# FIXME: users_groups data may be incomplete (inspector returned degraded)".into(),
        );
    }
    lines.extend(users_section_lines(snap));

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

    // 13. Epilogue
    lines.extend(tmpfiles_lines());
    lines.extend(validate_lines());

    lines.join("\n")
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
/// Phase 6: prefers `target_image.image_ref` (normalized ref from resolution).
/// Falls back to legacy `rpm.base_image` for Go-generated snapshots.
/// Returns `None` when neither source is available — callers render a comment.
pub fn base_image_from_snapshot(snap: &InspectionSnapshot) -> Option<String> {
    // Phase 6: prefer top-level target_image (stores normalized ref)
    if let Some(ref ti) = snap.target_image {
        return Some(ti.image_ref.clone());
    }
    // Backward compat: Go-generated snapshots with rpm.base_image
    if let Some(rpm) = &snap.rpm {
        if let Some(ref base) = rpm.base_image {
            if !base.is_empty() {
                return Some(base.clone());
            }
        }
    }
    None
}

// --- Packages section ---

fn canonical_package_id(name: &str, arch: &str) -> String {
    format!("{name}.{arch}")
}

fn package_display_name(pkg: &PackageEntry) -> String {
    if pkg.arch.is_empty() {
        pkg.name.clone()
    } else {
        canonical_package_id(&pkg.name, &pkg.arch)
    }
}

fn package_state_label(pkg: &PackageEntry) -> String {
    serde_json::to_string(&pkg.state)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string()
}

fn manual_follow_up_line(pkg: &PackageEntry) -> Option<String> {
    match pkg.state {
        inspectah_core::types::rpm::PackageState::LocalInstall => Some(format!(
            "# TODO: '{}' was installed locally (state: {}) — provide a .rpm or custom repo.",
            package_display_name(pkg),
            package_state_label(pkg)
        )),
        inspectah_core::types::rpm::PackageState::NoRepo => Some(format!(
            "# TODO: '{}' has no repository source (state: {}) — provide a .rpm or custom repo.",
            package_display_name(pkg),
            package_state_label(pkg)
        )),
        _ if pkg.source_repo.is_empty() => Some(format!(
            "# TODO: '{}' has no recorded repository source — verify how to reinstall it in the image.",
            package_display_name(pkg)
        )),
        _ => None,
    }
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

fn packages_section_lines(snap: &InspectionSnapshot, base: Option<&str>) -> Vec<String> {
    let mut lines = Vec::new();

    match base {
        Some(b) => lines.push(format!("FROM {b}")),
        None => lines.push("# FROM line omitted \u{2014} target image could not be determined. Use --base-image to specify.".to_string()),
    }
    lines.push(String::new());

    let rpm = match &snap.rpm {
        Some(rpm) => rpm,
        None => return lines,
    };

    // Repo files
    let included_repos: usize = rpm
        .repo_files
        .iter()
        .filter(|r| r.include && !r.is_default_repo)
        .count();
    if included_repos > 0 {
        lines.push(format!("# === Custom Repositories ({included_repos}) ==="));
        lines.push("COPY config/etc/yum.repos.d/ /etc/yum.repos.d/".into());
        lines.push(String::new());
    }

    // GPG keys — batch standard-dir keys, per-key import for non-standard
    let included_gpg: Vec<_> = rpm.gpg_keys.iter().filter(|k| k.include).collect();
    if !included_gpg.is_empty() {
        lines.push(format!("# === GPG Keys ({}) ===", included_gpg.len()));

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
                lines.push(format!(
                    "# FIXME: GPG key path contains unsafe characters: {}",
                    super::safety::html_escape(&key.path)
                ));
                continue;
            }
            if super::safety::sanitize_shell_value(&key.path).is_none() {
                lines.push(format!(
                    "# FIXME: GPG key path unsafe for shell: {}",
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
            lines.push(format!(
                "COPY config/{STANDARD_GPG_DIR}/ /{STANDARD_GPG_DIR}/"
            ));
        }

        // Non-standard directory keys: COPY parent dir + per-key rpm --import
        for dir in &nonstandard_dirs {
            match super::safety::sanitize_shell_value(dir) {
                Some(safe) => lines.push(format!("COPY config/{safe}/ /{safe}/")),
                None => lines.push(format!(
                    "# FIXME: GPG directory path contains unsafe characters: {}",
                    super::safety::html_escape(dir)
                )),
            }
        }

        // COPY root-level keys directly (no parent directory)
        for key in &root_keys {
            let rel = key.path.trim_start_matches('/');
            lines.push(format!("COPY config/{rel} {}", key.path));
        }

        // rpm --import only for non-standard keys (standard-dir keys are auto-imported)
        for key in &nonstandard_keys {
            lines.push(format!("RUN rpm --import {}", key.path));
        }
        lines.push(String::new());
    }

    // Module streams
    let enabled_modules: Vec<_> = rpm
        .module_streams
        .iter()
        .filter(|ms| ms.include && !ms.baseline_match)
        .collect();
    if !enabled_modules.is_empty() {
        lines.push("# === Module Streams ===".into());
        for ms in &enabled_modules {
            // Sanitize all host-derived values before shell interpolation
            if sanitize_shell_value(&ms.module_name).is_none()
                || sanitize_shell_value(&ms.stream).is_none()
            {
                lines.push(format!(
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
                    lines.push(format!(
                        "# FIXME: module profile contains unsafe characters, skipped: {:?}",
                        ms.profiles
                    ));
                    continue;
                }
                format!("/{}", safe_profiles.join(","))
            };
            lines.push(format!(
                "RUN dnf module enable -y {}:{}{}",
                ms.module_name, ms.stream, profiles
            ));
        }
        lines.push(String::new());
    }

    // Packages
    let mut install_names = Vec::new();
    let mut todo_lines = Vec::new();

    for pkg in &rpm.packages_added {
        if let Some(line) = manual_follow_up_line(pkg) {
            todo_lines.push(line);
        }
    }

    let is_fleet_snapshot = rpm.packages_added.iter().any(|pkg| pkg.fleet.is_some());
    let leaf_filter: Option<std::collections::HashSet<String>> = rpm
        .leaf_packages
        .as_ref()
        .filter(|_| !is_fleet_snapshot)
        .map(|leaf_packages| leaf_packages.iter().cloned().collect());

    let baseline_suppressed_set: std::collections::HashSet<String> = rpm
        .baseline_suppressed
        .as_ref()
        .map(|v| v.iter().cloned().collect())
        .unwrap_or_default();

    let installable_packages: Vec<&PackageEntry> = rpm
        .packages_added
        .iter()
        .filter(|pkg| pkg.include)
        .filter(|pkg| manual_follow_up_line(pkg).is_none())
        .filter(|pkg| {
            // Baseline-suppressed packages never go into RUN dnf install
            !baseline_suppressed_set.contains(&canonical_package_id(&pkg.name, &pkg.arch))
        })
        .filter(|pkg| {
            leaf_filter.as_ref().is_none_or(|leaf_ids| {
                leaf_ids.contains(&canonical_package_id(&pkg.name, &pkg.arch))
            })
        })
        .collect();

    let duplicate_name_counts: std::collections::HashMap<String, usize> = installable_packages
        .iter()
        .fold(std::collections::HashMap::new(), |mut counts, pkg| {
            *counts.entry(pkg.name.clone()).or_insert(0) += 1;
            counts
        });

    for pkg in installable_packages {
        let install_name = install_name_for_package(pkg, &duplicate_name_counts);
        if sanitize_shell_value(&install_name).is_some() {
            install_names.push(install_name);
        }
    }

    if !install_names.is_empty() {
        install_names.sort();
        lines.push(format!("# === Packages ({}) ===", install_names.len()));
        let dnf_suffix = " && dnf clean all && rm -rf /var/cache/dnf /var/lib/dnf/history* /var/log/dnf* /var/log/hawkey.log /var/log/rhsm";
        if install_names.len() <= 10 {
            lines.push(format!(
                "RUN dnf install -y {}{}",
                install_names.join(" "),
                dnf_suffix
            ));
        } else {
            lines.push("RUN dnf install -y \\".into());
            for name in &install_names {
                lines.push(format!("    {} \\", name));
            }
            lines.push(format!(
                "    {}",
                dnf_suffix.trim_start_matches(" && ").replace("&& ", "")
            ));
        }
        lines.push(String::new());
    }

    if !todo_lines.is_empty() {
        lines.push(String::new());
        lines.push("# === Manual Follow-up Required ===".into());
        lines.extend(todo_lines);
        lines.push(String::new());
    }

    // Version locks
    let included_locks: Vec<_> = rpm.version_locks.iter().filter(|vl| vl.include).collect();
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
            lines.push("# === Version Locks ===".into());
            lines.push("RUN dnf install -y python3-dnf-plugin-versionlock && \\".into());
            for vl in &safe_locks {
                lines.push(format!("    dnf versionlock add {} && \\", vl.raw_pattern));
            }
            lines.push("    dnf clean all".into());
            lines.push(String::new());
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

// --- Services section ---

/// Format a `RUN systemctl enable/disable` block. When the unit count
/// exceeds 3, use backslash line-continuation for readability.
fn systemctl_lines(verb: &str, units: &[String]) -> Vec<String> {
    if units.len() <= 3 {
        vec![format!("RUN systemctl {} {}", verb, units.join(" "))]
    } else {
        let mut lines = vec![format!("RUN systemctl {} \\", verb)];
        for (i, u) in units.iter().enumerate() {
            if i < units.len() - 1 {
                lines.push(format!("    {} \\", u));
            } else {
                lines.push(format!("    {}", u));
            }
        }
        lines
    }
}

fn services_section_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let mut lines = Vec::new();

    // Build set of config-tree timer units to exclude from service enables
    let mut config_tree_units: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Some(st) = &snap.scheduled_tasks {
        for t in &st.systemd_timers {
            if t.source == "local" && !t.name.is_empty() {
                config_tree_units.insert(format!("{}.timer", t.name));
                config_tree_units.insert(format!("{}.service", t.name));
            }
        }
        for u in &st.generated_timer_units {
            if u.include && !u.name.is_empty() {
                // @reboot entries have empty timer_content — only the service
                // file is materialized in the config tree.
                if !u.timer_content.is_empty() {
                    config_tree_units.insert(format!("{}.timer", u.name));
                }
                if !u.service_content.is_empty() {
                    config_tree_units.insert(format!("{}.service", u.name));
                }
            }
        }
    }

    let services = match &snap.services {
        Some(s) => s,
        None => return lines,
    };

    // Derive enable/disable lists from state_changes (preset-divergent only),
    // not from enabled_units/disabled_units (full inventory).
    let included_changes: Vec<_> = services
        .state_changes
        .iter()
        .filter(|sc| sc.include)
        .collect();

    if included_changes.is_empty() {
        return lines;
    }

    lines.push("# === Service Enablement ===".into());

    let mut safe_enabled = Vec::new();
    let mut safe_disabled = Vec::new();
    let mut safe_masked = Vec::new();
    let mut deferred = Vec::new();

    for sc in &included_changes {
        let u = &sc.unit;
        if sanitize_shell_value(u).is_none() {
            continue;
        }
        match sc.action.as_str() {
            "enable" => {
                if config_tree_units.contains(u.as_str()) {
                    deferred.push(u.clone());
                } else {
                    safe_enabled.push(u.clone());
                }
            }
            "disable" => {
                safe_disabled.push(u.clone());
            }
            "mask" => {
                safe_masked.push(u.clone());
            }
            _ => {}
        }
    }

    if !safe_enabled.is_empty() {
        lines.extend(systemctl_lines("enable", &safe_enabled));
    }
    if !safe_disabled.is_empty() {
        lines.extend(systemctl_lines("disable", &safe_disabled));
    }
    if !safe_masked.is_empty() {
        lines.extend(systemctl_lines("mask", &safe_masked));
    }
    if !deferred.is_empty() {
        lines.push(format!(
            "# {} unit(s) deferred to Scheduled Tasks section: {}",
            deferred.len(),
            deferred.join(", ")
        ));
    }

    lines.push(String::new());
    lines
}

// --- Network section ---

fn network_section_lines(snap: &InspectionSnapshot, firewall_only: bool) -> Vec<String> {
    let mut lines = Vec::new();
    let network = match &snap.network {
        Some(n) => n,
        None => return lines,
    };

    if firewall_only {
        let included_zones: usize = network.firewall_zones.iter().filter(|z| z.include).count();
        if included_zones > 0 || !network.firewall_direct_rules.is_empty() {
            lines.push("# === Firewall Configuration ===".into());
            if included_zones > 0 {
                lines.push(format!(
                    "# {} custom firewall zone(s) — included in COPY config/etc/ below",
                    included_zones
                ));
            }
            lines.push(String::new());
        }
        return lines;
    }

    // Non-firewall network config
    if !network.static_routes.is_empty() {
        lines.push("# === Static Routes ===".into());
        for r in &network.static_routes {
            lines.push(format!("# Static route file: {}", r.path));
        }
        lines.push(String::new());
    }

    if !network.hosts_additions.is_empty() {
        lines.push("# === /etc/hosts Additions ===".into());
        lines.push("# FIXME: These /etc/hosts entries need to be added to the image:".into());
        for h in &network.hosts_additions {
            lines.push(format!("#   {}", h));
        }
        lines.push(String::new());
    }

    if !network.proxy.is_empty() {
        lines.push("# === Proxy Configuration ===".into());
        for p in &network.proxy {
            lines.push(format!("# {}: {}", p.source, p.line));
        }
        lines.push(String::new());
    }

    lines
}

// --- Scheduled Tasks section ---

fn scheduled_tasks_section_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let mut lines = Vec::new();

    let st = match &snap.scheduled_tasks {
        Some(s) => s,
        None => return lines,
    };

    let has_content = !st.generated_timer_units.is_empty()
        || !st.systemd_timers.is_empty()
        || !st.cron_jobs.is_empty()
        || !st.at_jobs.is_empty();
    if !has_content {
        return lines;
    }

    lines.push("# === Scheduled Tasks ===".into());

    let local_timers: Vec<_> = st
        .systemd_timers
        .iter()
        .filter(|t| t.source == "local" && t.include == Some(true))
        .collect();

    let included_timers: Vec<_> = st
        .generated_timer_units
        .iter()
        .filter(|u| u.include)
        .collect();

    if !local_timers.is_empty() || !included_timers.is_empty() {
        lines.push("COPY config/etc/systemd/system/ /etc/systemd/system/".into());
    }

    if !local_timers.is_empty() {
        let names: Vec<String> = local_timers
            .iter()
            .map(|t| format!("{}.timer", t.name))
            .collect();
        lines.push(format!(
            "# Existing local timers ({}): {}",
            local_timers.len(),
            names.join(", ")
        ));
    }

    if !included_timers.is_empty() {
        let names: Vec<String> = included_timers
            .iter()
            .filter(|u| !u.name.is_empty())
            .map(|u| u.name.clone())
            .collect();
        lines.push(format!(
            "# Converted from cron: {} timer(s): {}",
            included_timers.len(),
            names.join(", ")
        ));
    }

    // Consolidate timer enables — sanitize names before shell interpolation
    let mut timer_names = Vec::new();
    for t in &local_timers {
        let unit = format!("{}.timer", t.name);
        if sanitize_shell_value(&unit).is_some() {
            timer_names.push(unit);
        } else {
            lines.push(format!(
                "# FIXME: Timer unit name contains unsafe characters: {}",
                t.name
            ));
        }
    }
    let mut reboot_service_names = Vec::new();
    for u in &included_timers {
        if !u.name.is_empty() {
            // @reboot entries have no timer — enable the service instead
            if u.cron_expr == "@reboot" {
                let unit = format!("{}.service", u.name);
                if sanitize_shell_value(&unit).is_some() {
                    reboot_service_names.push(unit);
                }
            } else {
                let unit = format!("{}.timer", u.name);
                if sanitize_shell_value(&unit).is_some() {
                    timer_names.push(unit);
                } else {
                    lines.push(format!(
                        "# FIXME: Timer unit name contains unsafe characters: {}",
                        u.name
                    ));
                }
            }
        }
    }
    if !timer_names.is_empty() {
        lines.push(format!("RUN systemctl enable {}", timer_names.join(" ")));
    }
    if !reboot_service_names.is_empty() {
        lines.push("# @reboot cron job(s) — boot-triggered oneshot service(s):".to_string());
        lines.push(format!(
            "RUN systemctl enable {}",
            reboot_service_names.join(" ")
        ));
    }

    if !st.at_jobs.is_empty() {
        lines.push(format!(
            "# FIXME: {} at job(s) found — convert to systemd timers or cron",
            st.at_jobs.len()
        ));
        for a in &st.at_jobs {
            lines.push(format!("#   at job: {}", a.command));
        }
    }

    lines.push(String::new());
    lines
}

// --- Config section ---

fn config_section_lines(
    snap: &InspectionSnapshot,
    materialized_roots: Option<&[String]>,
) -> Vec<String> {
    let mut lines = Vec::new();

    lines.push("# === Configuration Files ===".into());

    // Config inventory comment
    if let Some(config) = &snap.config {
        let total = config.files.iter().filter(|f| f.include).count();
        if total > 0 {
            lines.push(format!("# {} config file(s) captured", total));
        }

        let has_diffs = config.files.iter().any(|f| f.diff_against_rpm.is_some());
        if has_diffs {
            lines.push(
                "# Config diffs (--config-diffs): see audit-report.md and report.html for per-file diffs."
                    .into(),
            );
        }
    }
    lines.push(String::new());

    // COPY per top-level dir — use materialized roots when available
    // (single source of truth from write_config_tree), fall back to
    // snapshot-derived roots for standalone rendering.
    let config_roots: Vec<String> = match materialized_roots {
        Some(roots) => roots.to_vec(),
        None => config_copy_roots_from_snapshot(snap),
    };
    if config_roots.is_empty() {
        lines.push("# (no config files captured)".into());
    } else {
        for root in &config_roots {
            lines.push(format!("COPY config/{root}/ /{root}/"));
        }
    }
    lines.push(String::new());

    // CA trust anchors
    if let Some(config) = &snap.config {
        let has_ca = config.files.iter().any(|f| {
            f.include
                && f.path
                    .trim_start_matches('/')
                    .starts_with("etc/pki/ca-trust/source/anchors/")
        });
        if has_ca {
            lines.push("# === CA Trust Store ===".into());
            lines.push(
                "# Custom CA certificates detected in /etc/pki/ca-trust/source/anchors/".into(),
            );
            lines.push("RUN update-ca-trust".into());
            lines.push(String::new());
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
        if !f.include {
            continue;
        }
        let rel = f.path.trim_start_matches('/');
        if rel.is_empty() {
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
        if f.path == "/etc/crypto-policies/config" && f.include {
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
        .filter(|u| u.include)
        .count();
    let included_flatpaks: usize = containers.flatpak_apps.iter().filter(|a| a.include).count();

    if included_quadlets == 0 && included_flatpaks == 0 {
        return lines;
    }

    lines.push("# === Container Workloads ===".into());
    if included_quadlets > 0 {
        lines.push("COPY quadlet/ /etc/containers/systemd/".into());
    }
    if included_flatpaks > 0 {
        lines.push("# Flatpak applications — installed on first boot via oneshot service".into());
        lines.push("# Manifest: flatpak/flatpak-install.json".into());
        lines.push("COPY flatpak/ /usr/share/inspectah/flatpak/".into());
        lines.push(
            "COPY flatpak/flatpak-provision.service /etc/systemd/system/flatpak-provision.service"
                .into(),
        );
        lines.push("RUN systemctl enable flatpak-provision.service".into());
    }
    lines.push(String::new());
    lines
}

// --- Non-RPM Software section ---

fn non_rpm_section_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let mut lines = Vec::new();
    let nrs = match &snap.non_rpm_software {
        Some(n) => n,
        None => return lines,
    };

    let migration_items: Vec<_> = nrs
        .items
        .iter()
        .filter(|item| item.review_status == "migration_planned")
        .collect();

    if migration_items.is_empty() {
        return lines;
    }

    lines.push("# === Non-RPM Software (migration planned) ===".into());
    lines.push(
        "# WARNING: These stubs are advisory — source files are NOT in the build context.".into(),
    );
    lines.push("# You must manually stage each referenced file/package before building.".into());
    lines.push("#".into());

    for item in &migration_items {
        let note = if item.notes.is_empty() {
            String::new()
        } else {
            format!(" — {}", item.notes)
        };

        if item.method == "pip dist-info" && item.has_c_extensions {
            lines.push(format!(
                "# {}=={} — pip package with native extensions, rebuild required{}",
                item.name, item.version, note
            ));
        } else if item.method == "pip dist-info" {
            lines.push(format!(
                "# {}=={} — pip package{}",
                item.name, item.version, note
            ));
            lines.push(format!("# RUN pip install {}=={}", item.name, item.version));
        } else if (item.lang == "go" || item.method == "go binary") && item.r#static {
            let dest = std::path::Path::new(&item.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            lines.push(format!(
                "# COPY {} /usr/local/bin/{}{}",
                item.path, dest, note
            ));
        } else if item.lang == "shell" || item.path.ends_with(".sh") {
            let dest = std::path::Path::new(&item.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            lines.push(format!(
                "# COPY {} /usr/local/bin/{}{}",
                item.path, dest, note
            ));
        } else if !item.shared_libs.is_empty() {
            lines.push(format!(
                "# {} — dynamic binary, shared libs: {}{}",
                item.path,
                item.shared_libs.join(", "),
                note
            ));
            lines.push("# Dependency analysis required before COPY".into());
        } else if item.r#static {
            let dest = std::path::Path::new(&item.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            lines.push(format!(
                "# COPY {} /usr/local/bin/{}{}",
                item.path, dest, note
            ));
        } else {
            lines.push(format!(
                "# {} ({}) — review required for migration{}",
                item.path, item.method, note
            ));
        }
    }
    lines.push(String::new());
    lines
}

// --- Users section ---

fn users_section_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let mut lines = Vec::new();
    let ug = match &snap.users_groups {
        Some(u) => u,
        None => return lines,
    };

    let included_users: Vec<_> = ug
        .users
        .iter()
        .filter(|u| u.get("include").and_then(|v| v.as_bool()).unwrap_or(true))
        .collect();

    if included_users.is_empty() {
        return lines;
    }

    lines.push("# === Users and Groups ===".into());

    let mut sysusers_count = 0;
    let mut useradd_users = Vec::new();
    let mut blueprint_count = 0;
    let mut kickstart_count = 0;

    for u in &included_users {
        let strategy = u
            .get("strategy")
            .and_then(|v| v.as_str())
            .unwrap_or("useradd");
        match strategy {
            "sysusers" => sysusers_count += 1,
            "blueprint" => blueprint_count += 1,
            "kickstart" => kickstart_count += 1,
            _ => useradd_users.push(*u),
        }
    }

    if sysusers_count > 0 {
        lines.push(format!("# systemd-sysusers entries ({sysusers_count}):"));
        lines.push("# These are system users created via sysusers.d drop-ins in config/.".into());
    }

    if !useradd_users.is_empty() {
        lines.push(format!("# useradd users ({}):", useradd_users.len()));
        for u in &useradd_users {
            let name = u.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let uid = u.get("uid").and_then(|v| v.as_f64()).unwrap_or(0.0);
            if !name.is_empty() && sanitize_shell_value(name).is_some() {
                if uid > 0.0 {
                    lines.push(format!("RUN useradd -u {} {}", uid as u32, name));
                } else {
                    lines.push(format!("RUN useradd {}", name));
                }
            }
        }
    }

    if blueprint_count > 0 {
        lines.push(format!(
            "# FIXME: {} user(s) with blueprint strategy — provision via image builder blueprint",
            blueprint_count
        ));
    }
    if kickstart_count > 0 {
        lines.push(format!(
            "# FIXME: {} user(s) with kickstart strategy — see kickstart.ks",
            kickstart_count
        ));
    }

    lines.push(String::new());
    lines
}

// --- Kernel/Boot section ---

fn kernel_boot_section_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let mut lines = Vec::new();

    let kb = match &snap.kernel_boot {
        Some(k) => k,
        None => return lines,
    };

    let has_content = !kb.cmdline.is_empty()
        || !kb.modules_load_d.is_empty()
        || !kb.modprobe_d.is_empty()
        || !kb.dracut_conf.is_empty()
        || !kb.sysctl_overrides.is_empty()
        || !kb.non_default_modules.is_empty()
        || !kb.tuned_active.is_empty()
        || !kb.tuned_custom_profiles.is_empty();

    if !has_content {
        return lines;
    }

    lines.push("# === Kernel and Boot Configuration ===".into());

    // Kernel arguments
    let safe_kargs = operator_kargs(&kb.cmdline);
    if !safe_kargs.is_empty() {
        lines.push("# === Kernel Arguments (bootc-native kargs.d) ===".into());
        lines.push(
            "# These are applied at install and honored across image upgrades. See bootc documentation:"
                .into(),
        );
        lines.push("# https://containers.github.io/bootc/building/kernel-arguments.html".into());
        lines.push("RUN mkdir -p /usr/lib/bootc/kargs.d".into());
        lines.push(
            "COPY config/usr/lib/bootc/kargs.d/inspectah-migrated.toml /usr/lib/bootc/kargs.d/"
                .into(),
        );
    }

    // Non-default modules
    let included_mods: usize = kb.non_default_modules.iter().filter(|m| m.include).count();
    if included_mods > 0 {
        lines.push(format!(
            "# {} non-default kernel module(s) — config files in COPY config/etc/ above",
            included_mods
        ));
    }

    // Sysctl overrides
    let included_sysctl: usize = kb.sysctl_overrides.iter().filter(|s| s.include).count();
    if included_sysctl > 0 {
        lines.push(format!(
            "# {} sysctl override(s) — config files in COPY config/etc/ above",
            included_sysctl
        ));
    }

    // Tuned
    if !kb.tuned_active.is_empty() {
        if is_valid_tuned_profile(&kb.tuned_active) {
            lines.push(format!("# Tuned profile: {}", kb.tuned_active));
            lines.push(format!(
                "RUN echo \"{}\" > /etc/tuned/active_profile",
                kb.tuned_active
            ));
            lines.push("RUN echo \"manual\" > /etc/tuned/profile_mode".into());
            lines.push("RUN systemctl enable tuned.service".into());
        } else {
            lines.push(format!(
                "# FIXME: tuned profile name contains unsafe characters: {:?}",
                kb.tuned_active
            ));
        }
    }

    lines.push(String::new());
    lines
}

// --- Security & Access Control section ---

fn selinux_section_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let mut lines = Vec::new();
    let sel = match &snap.selinux {
        Some(s) => s,
        None => return lines,
    };

    let has_content = !sel.custom_modules.is_empty()
        || !sel.boolean_overrides.is_empty()
        || !sel.fcontext_rules.is_empty()
        || !sel.audit_rules.is_empty()
        || sel.fips_mode
        || !sel.port_labels.is_empty();

    if !has_content {
        return lines;
    }

    lines.push("# === Security & Access Control ===".into());

    if !sel.custom_modules.is_empty() {
        lines.push(format!(
            "# FIXME: {} custom policy module(s) detected — \
             export .pp files to config/selinux/ and uncomment the COPY + semodule lines below",
            sel.custom_modules.len()
        ));
        lines.push("# COPY config/selinux/ /tmp/selinux/".into());
        lines.push("# RUN semodule -i /tmp/selinux/*.pp && rm -rf /tmp/selinux/".into());
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
        lines.push(format!(
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
                lines.push(format!("RUN setsebool -P {} {}", bname, bval));
            } else {
                lines.push(format!(
                    "# FIXME: boolean name/value contains unsafe characters, skipped: {:?}={:?}",
                    bname, bval
                ));
            }
        }
    }

    if !sel.fcontext_rules.is_empty() {
        lines.push(format!(
            "# FIXME: {} custom fcontext rule(s) detected — apply in image",
            sel.fcontext_rules.len()
        ));
        let limit = sel.fcontext_rules.len().min(10);
        for fc in &sel.fcontext_rules[..limit] {
            if sanitize_shell_value(fc).is_some() {
                lines.push(format!("# RUN semanage fcontext -a {}", fc));
            } else {
                lines.push(format!(
                    "# FIXME: fcontext rule contains unsafe characters: {:?}",
                    fc
                ));
            }
        }
        lines.push("# RUN restorecon -Rv /  # apply fcontext changes after all COPYs".into());
    }

    if !sel.audit_rules.is_empty() {
        lines.push(format!(
            "# {} custom audit rule file(s) materialized under config/etc/audit/rules.d/",
            sel.audit_rules.len()
        ));
    }

    if !sel.pam_configs.is_empty() {
        lines.push(format!(
            "# {} custom PAM config file(s) materialized under config/etc/pam.d/",
            sel.pam_configs.len()
        ));
    }

    if !sel.port_labels.is_empty() {
        lines.push(format!(
            "# {} custom SELinux port label(s) detected",
            sel.port_labels.len()
        ));
        for pl in &sel.port_labels {
            if sanitize_shell_value(&pl.protocol).is_some()
                && sanitize_shell_value(&pl.port).is_some()
                && sanitize_shell_value(&pl.label_type).is_some()
            {
                lines.push(format!(
                    "RUN semanage port -a -t {} -p {} {}",
                    pl.label_type, pl.protocol, pl.port
                ));
            } else {
                lines.push(format!(
                    "# FIXME: port label contains unsafe characters, skipped: {:?} {:?} {:?}",
                    pl.label_type, pl.protocol, pl.port
                ));
            }
        }
    }

    if sel.fips_mode {
        lines.push(
            "# FIXME: host has FIPS mode enabled — enable FIPS in the bootc image via fips-mode-setup"
                .into(),
        );
    }

    lines.push(String::new());
    lines
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
        lines.push("# === Secrets: Excluded Files ===".into());
        lines.push(format!(
            "# {} file(s) excluded from the image for security:",
            excluded.len()
        ));
        for f in &excluded {
            lines.push(format!("#   {} ({})", f.path, f.remediation));
        }
        lines.push("# See secrets-review.md for details and remediation steps.".into());
        lines.push(String::new());
    }
    if !flagged.is_empty() {
        lines.push("# === Secrets: Flagged for Review ===".into());
        lines.push(format!(
            "# {} file(s) flagged for manual review:",
            flagged.len()
        ));
        for f in &flagged {
            lines.push(format!("#   {}", f.path));
        }
        lines.push("# See secrets-review.md for details.".into());
        lines.push(String::new());
    }
    lines
}

// --- Epilogue ---

fn tmpfiles_lines() -> Vec<String> {
    vec![
        "# === Finalize: systemd-tmpfiles for /tmp, /run, /var, /etc/ above".into(),
        String::new(),
    ]
}

fn validate_lines() -> Vec<String> {
    vec![
        "# === Validate bootc compatibility ===".into(),
        "RUN bootc container lint".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
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
                    include: true,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        });
        snap
    }

    #[test]
    fn test_containerfile_package_based() {
        let snap = snapshot_with_packages(&["httpd", "vim-enhanced"]);
        let output = render_containerfile(&snap, None);
        // No target_image or rpm.base_image → FROM omitted
        assert!(
            output.contains("# FROM line omitted"),
            "must omit FROM when no base image source"
        );
        assert!(
            output.contains("RUN dnf install -y"),
            "must contain dnf install"
        );
        assert!(output.contains("httpd"), "must contain httpd");
        assert!(output.contains("vim-enhanced"), "must contain vim-enhanced");
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
                    include: true,
                    ..Default::default()
                },
                PackageEntry {
                    name: "httpd".into(),
                    arch: "i686".into(),
                    state: PackageState::Added,
                    source_repo: "appstream".into(),
                    include: false,
                    ..Default::default()
                },
            ],
            leaf_packages: Some(vec!["httpd.x86_64".into()]),
            auto_packages: Some(vec!["httpd.i686".into()]),
            ..Default::default()
        });

        let output = render_containerfile(&snap, None);
        let install_line = output
            .lines()
            .find(|line| line.starts_with("RUN dnf install -y"))
            .expect("install line must be present");

        assert!(
            install_line
                .split_whitespace()
                .any(|token| token == "httpd"),
            "install line must render package names, got: {install_line}"
        );
        assert!(
            !install_line.contains("httpd.x86_64"),
            "install line must not leak canonical leaf identity, got: {install_line}"
        );
        assert!(
            !install_line.contains("httpd.i686"),
            "install line must not install the non-leaf arch, got: {install_line}"
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
                    include: true,
                    ..Default::default()
                },
                PackageEntry {
                    name: "local-tool".into(),
                    arch: "x86_64".into(),
                    state: PackageState::LocalInstall,
                    source_repo: String::new(),
                    include: false,
                    ..Default::default()
                },
                PackageEntry {
                    name: "orphan-pkg".into(),
                    arch: "x86_64".into(),
                    state: PackageState::NoRepo,
                    source_repo: String::new(),
                    include: false,
                    ..Default::default()
                },
                PackageEntry {
                    name: "mystery".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    source_repo: String::new(),
                    include: false,
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

        let output = render_containerfile(&snap, None);
        let install_line = output
            .lines()
            .find(|line| line.starts_with("RUN dnf install -y"))
            .expect("install line must be present");

        assert!(
            install_line.contains("httpd"),
            "leaf package must stay on the install line, got: {install_line}"
        );
        assert!(
            !install_line.contains("local-tool")
                && !install_line.contains("orphan-pkg")
                && !install_line.contains("mystery"),
            "non-leaf unresolved packages must stay off the install line, got: {install_line}"
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
        use inspectah_core::types::services::ServiceStateChange;
        // Build a snapshot with data in multiple sections to verify ordering
        let mut snap = snapshot_with_packages(&["httpd"]);
        snap.services = Some(inspectah_core::types::services::ServiceSection {
            state_changes: vec![ServiceStateChange {
                unit: "httpd.service".into(),
                action: "enable".into(),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        snap.selinux = Some(inspectah_core::types::selinux::SelinuxSection {
            fips_mode: true,
            ..Default::default()
        });

        let output = render_containerfile(&snap, None);

        // Verify section order: packages before services before selinux before epilogue
        let packages_pos = output.find("dnf install").unwrap();
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
        let output = render_containerfile(&snap, None);
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
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            base_image: Some("quay.io/custom/image:latest".into()),
            ..Default::default()
        });
        let output = render_containerfile(&snap, None);
        assert!(output.contains("FROM quay.io/custom/image:latest"));
    }

    #[test]
    fn test_containerfile_services() {
        use inspectah_core::types::services::ServiceStateChange;
        let mut snap = InspectionSnapshot::new();
        snap.services = Some(inspectah_core::types::services::ServiceSection {
            state_changes: vec![
                ServiceStateChange {
                    unit: "httpd.service".into(),
                    current_state: "enabled".into(),
                    default_state: "disabled".into(),
                    action: "enable".into(),
                    include: true,
                    ..Default::default()
                },
                ServiceStateChange {
                    unit: "sshd.service".into(),
                    current_state: "enabled".into(),
                    default_state: "disabled".into(),
                    action: "enable".into(),
                    include: true,
                    ..Default::default()
                },
                ServiceStateChange {
                    unit: "cups.service".into(),
                    current_state: "disabled".into(),
                    default_state: "enabled".into(),
                    action: "disable".into(),
                    include: true,
                    ..Default::default()
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
        let output = render_containerfile(&snap, None);
        assert!(output.contains("systemctl enable httpd.service sshd.service"));
        assert!(output.contains("systemctl disable cups.service"));
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
        let output = render_containerfile(&snap, None);
        assert!(output.contains("safe-pkg"));
        // The unsafe package should not appear in a RUN command
        assert!(!output.contains("RUN dnf install -y bad;pkg"));
    }

    #[test]
    fn test_containerfile_shell_metachar_package_rejected() {
        // Package name with shell command injection
        let snap = snapshot_with_packages(&["legit-pkg", "pkg$(whoami)", "pkg`id`"]);
        let output = render_containerfile(&snap, None);
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
                    include: true,
                    baseline_match: false,
                    ..Default::default()
                },
                EnabledModuleStream {
                    module_name: "evil$(whoami)".into(),
                    stream: "2.0".into(),
                    profiles: vec![],
                    include: true,
                    baseline_match: false,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None);
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
                    include: true,
                    ..Default::default()
                },
                VersionLockEntry {
                    raw_pattern: "pkg;rm -rf /".into(),
                    include: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None);
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
        let output = render_containerfile(&snap, None);
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
        let output = render_containerfile(&snap, None);
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
                include: Some(true),
                ..Default::default()
            }],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None);
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
                    include: true,
                    ..Default::default()
                },
                inspectah_core::types::rpm::RepoFile {
                    path: "/opt/custom/keys/signing-key.asc".into(),
                    content: "key-data".into(),
                    include: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None);
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
                    include: true,
                    ..Default::default()
                },
                inspectah_core::types::rpm::RepoFile {
                    path: "../../etc/shadow".into(),
                    content: "bad".into(),
                    include: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None);
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
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None);
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
                    include: true,
                    ..Default::default()
                },
                inspectah_core::types::rpm::RepoFile {
                    path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial".into(),
                    content: "key2".into(),
                    include: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None);
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
                    include: true,
                    ..Default::default()
                },
                inspectah_core::types::rpm::RepoFile {
                    path: "/opt/custom/keys/signing-key.asc".into(),
                    content: "key2".into(),
                    include: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None);
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
        use inspectah_core::types::services::ServiceStateChange;
        let mut snap = InspectionSnapshot::new();
        snap.services = Some(inspectah_core::types::services::ServiceSection {
            state_changes: vec![
                ServiceStateChange {
                    unit: "httpd.service".into(),
                    action: "enable".into(),
                    include: true,
                    ..Default::default()
                },
                ServiceStateChange {
                    unit: "sshd.service".into(),
                    action: "enable".into(),
                    include: true,
                    ..Default::default()
                },
                ServiceStateChange {
                    unit: "chronyd.service".into(),
                    action: "enable".into(),
                    include: true,
                    ..Default::default()
                },
                ServiceStateChange {
                    unit: "firewalld.service".into(),
                    action: "enable".into(),
                    include: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None);
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
    fn test_service_single_line_under_4() {
        use inspectah_core::types::services::ServiceStateChange;
        let mut snap = InspectionSnapshot::new();
        snap.services = Some(inspectah_core::types::services::ServiceSection {
            state_changes: vec![
                ServiceStateChange {
                    unit: "httpd.service".into(),
                    action: "enable".into(),
                    include: true,
                    ..Default::default()
                },
                ServiceStateChange {
                    unit: "sshd.service".into(),
                    action: "enable".into(),
                    include: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None);
        assert!(
            output.contains("systemctl enable httpd.service sshd.service"),
            "2 services should be single line"
        );
        assert!(
            !output.contains("\\"),
            "2 services should not use backslash continuation"
        );
    }

    #[test]
    fn test_containerfile_masked_services() {
        use inspectah_core::types::services::ServiceStateChange;
        let mut snap = InspectionSnapshot::new();
        snap.services = Some(inspectah_core::types::services::ServiceSection {
            state_changes: vec![
                ServiceStateChange {
                    unit: "cups.service".into(),
                    current_state: "masked".into(),
                    default_state: "enabled".into(),
                    action: "mask".into(),
                    include: true,
                    ..Default::default()
                },
                ServiceStateChange {
                    unit: "httpd.service".into(),
                    action: "enable".into(),
                    include: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None);
        assert!(
            output.contains("systemctl mask cups.service"),
            "masked service must produce systemctl mask, got:\n{}",
            output
        );
        assert!(
            output.contains("systemctl enable httpd.service"),
            "enabled service must still work alongside masked"
        );
    }

    #[test]
    fn test_service_disable_backslash_continuation_over_3() {
        use inspectah_core::types::services::ServiceStateChange;
        let mut snap = InspectionSnapshot::new();
        snap.services = Some(inspectah_core::types::services::ServiceSection {
            state_changes: vec![
                ServiceStateChange {
                    unit: "cups.service".into(),
                    action: "disable".into(),
                    include: true,
                    ..Default::default()
                },
                ServiceStateChange {
                    unit: "avahi-daemon.service".into(),
                    action: "disable".into(),
                    include: true,
                    ..Default::default()
                },
                ServiceStateChange {
                    unit: "bluetooth.service".into(),
                    action: "disable".into(),
                    include: true,
                    ..Default::default()
                },
                ServiceStateChange {
                    unit: "ModemManager.service".into(),
                    action: "disable".into(),
                    include: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None);
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
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None);
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
        let output = render_containerfile(&snap, None);
        assert!(
            output.contains("FROM registry.redhat.io/rhel9/rhel-bootc:9.6"),
            "must use target_image.image_ref for FROM, got:\n{output}"
        );
    }

    #[test]
    fn test_from_target_image_with_no_baseline_degraded() {
        use inspectah_core::baseline::{ResolutionStrategy, TargetImageIdentity};
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(TargetImageIdentity {
            image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.6".into(),
            strategy: ResolutionStrategy::OsRelease,
        });
        snap.no_baseline = true;
        let output = render_containerfile(&snap, None);
        assert!(
            output.contains("FROM registry.redhat.io/rhel9/rhel-bootc:9.6"),
            "degraded (no_baseline=true) must still use target_image for FROM"
        );
    }

    #[test]
    fn test_from_omitted_when_no_target_image() {
        let snap = InspectionSnapshot::new();
        let result = base_image_from_snapshot(&snap);
        assert!(
            result.is_none(),
            "no target_image or rpm.base_image must return None"
        );
        let output = render_containerfile(&snap, None);
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
    fn test_from_legacy_go_snapshot_uses_rpm_base_image() {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            base_image: Some("quay.io/legacy/image:1.0".into()),
            ..Default::default()
        });
        let output = render_containerfile(&snap, None);
        assert!(
            output.contains("FROM quay.io/legacy/image:1.0"),
            "legacy Go snapshot must use rpm.base_image"
        );
    }

    #[test]
    fn test_from_no_fallback_without_any_source() {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            base_image: None,
            ..Default::default()
        });
        let result = base_image_from_snapshot(&snap);
        assert!(
            result.is_none(),
            "no hardcoded fallback when rpm.base_image is None"
        );
    }

    #[test]
    fn test_from_target_image_takes_priority_over_rpm_base_image() {
        use inspectah_core::baseline::{ResolutionStrategy, TargetImageIdentity};
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(TargetImageIdentity {
            image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.6".into(),
            strategy: ResolutionStrategy::OsRelease,
        });
        snap.rpm = Some(RpmSection {
            base_image: Some("quay.io/old/image:1.0".into()),
            ..Default::default()
        });
        let result = base_image_from_snapshot(&snap);
        assert_eq!(
            result.unwrap(),
            "registry.redhat.io/rhel9/rhel-bootc:9.6",
            "target_image must take priority over rpm.base_image"
        );
    }

    #[test]
    fn test_containerfile_excludes_baseline_suppressed_packages() {
        use inspectah_core::snapshot::InspectionSnapshot;
        use inspectah_core::types::rpm::{PackageEntry, RpmSection};

        // Build an RpmSection with baseline_suppressed packages
        // and verify they don't appear in the rendered containerfile
        let mut rpm = RpmSection::default();
        rpm.packages_added = vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                include: true,
                source_repo: "appstream".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "kernel".into(),
                arch: "x86_64".into(),
                include: true,
                source_repo: "baseos".into(),
                ..Default::default()
            },
        ];
        rpm.leaf_packages = Some(vec!["httpd.x86_64".into(), "kernel.x86_64".into()]);
        rpm.baseline_suppressed = Some(vec!["kernel.x86_64".into()]);

        let mut snap = InspectionSnapshot::default();
        snap.rpm = Some(rpm);

        let output = render_containerfile(&snap, None);
        assert!(
            output.contains("httpd"),
            "non-suppressed package should be in containerfile"
        );
        assert!(
            !output.contains("kernel"),
            "baseline-suppressed package must not be in containerfile"
        );
    }
}
