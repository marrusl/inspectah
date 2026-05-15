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
//! 10. SELinux
//! 11. Network (routes, hosts, proxy)
//! 12. Secrets comments
//! 13. Epilogue (tmpfiles, RUN bootc container lint)

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::completeness::{Completeness, InspectorId};
use inspectah_core::types::os::SystemType;
use inspectah_core::types::redaction::RedactionKind;

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
    lines.extend(packages_section_lines(snap, &base));

    // bootc label for ostree-desktops base images
    if matches!(snap.system_type, SystemType::RpmOstree | SystemType::Bootc)
        && base.contains("fedora-ostree-desktops")
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

    // 10. SELinux
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

/// Returns the base image reference from the snapshot.
pub fn base_image_from_snapshot(snap: &InspectionSnapshot) -> String {
    if let Some(rpm) = &snap.rpm {
        if let Some(ref base) = rpm.base_image {
            if !base.is_empty() {
                return base.clone();
            }
        }
    }
    "registry.redhat.io/rhel9/rhel-bootc:9.4".to_string()
}

// --- Packages section ---

fn packages_section_lines(snap: &InspectionSnapshot, base: &str) -> Vec<String> {
    let mut lines = Vec::new();

    lines.push(format!("FROM {base}"));
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

    // GPG keys — generate per-key rpm --import using actual paths
    let included_gpg: Vec<_> = rpm.gpg_keys.iter().filter(|k| k.include).collect();
    if !included_gpg.is_empty() {
        lines.push(format!("# === GPG Keys ({}) ===", included_gpg.len()));

        // COPY each unique parent directory containing GPG keys
        let mut gpg_dirs: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut safe_keys: Vec<&inspectah_core::types::rpm::RepoFile> = Vec::new();
        let mut root_keys: Vec<&inspectah_core::types::rpm::RepoFile> = Vec::new();
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
                    gpg_dirs.insert(dir.to_string());
                }
                _ => {
                    // Root-level key (e.g., /good-key) — stage the file directly
                    root_keys.push(key);
                }
            }
            safe_keys.push(key);
        }
        for dir in &gpg_dirs {
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

        // Per-key rpm --import only for keys that passed validation AND have staging
        for key in &safe_keys {
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

    // Build set of excluded package names
    let excluded_pkgs: std::collections::HashSet<&str> = rpm
        .packages_added
        .iter()
        .filter(|p| !p.include)
        .map(|p| p.name.as_str())
        .collect();

    let unreachable: std::collections::HashSet<&str> = rpm
        .packages_added
        .iter()
        .filter(|p| {
            matches!(
                p.state,
                inspectah_core::types::rpm::PackageState::LocalInstall
                    | inspectah_core::types::rpm::PackageState::NoRepo
            )
        })
        .map(|p| p.name.as_str())
        .collect();

    if let Some(ref leaf_packages) = rpm.leaf_packages {
        for name in leaf_packages {
            if excluded_pkgs.contains(name.as_str()) {
                continue;
            }
            if sanitize_shell_value(name).is_some() {
                if unreachable.contains(name.as_str()) {
                    let state = unreachable_state(snap, name);
                    todo_lines.push(format!(
                        "# TODO: '{}' was installed locally (state: {}) \
                         — no repository source. Provide a .rpm or custom repo.",
                        name, state
                    ));
                    continue;
                }
                install_names.push(name.clone());
            }
        }
    } else {
        for pkg in &rpm.packages_added {
            if pkg.include && sanitize_shell_value(&pkg.name).is_some() {
                if matches!(
                    pkg.state,
                    inspectah_core::types::rpm::PackageState::LocalInstall
                        | inspectah_core::types::rpm::PackageState::NoRepo
                ) {
                    let state = serde_json::to_string(&pkg.state)
                        .unwrap_or_default()
                        .trim_matches('"')
                        .to_string();
                    todo_lines.push(format!(
                        "# TODO: '{}' was installed locally (state: {}) \
                         — no repository source. Provide a .rpm or custom repo.",
                        pkg.name, state
                    ));
                    continue;
                }
                install_names.push(pkg.name.clone());
            }
        }
    }

    if !install_names.is_empty() {
        install_names.sort();
        lines.push(format!("# === Packages ({}) ===", install_names.len()));
        let dnf_suffix =
            " && dnf clean all && rm -rf /var/cache/dnf /var/lib/dnf/history* /var/log/dnf* /var/log/hawkey.log /var/log/rhsm";
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

fn unreachable_state(snap: &InspectionSnapshot, name: &str) -> String {
    if let Some(rpm) = &snap.rpm {
        for pkg in &rpm.packages_added {
            if pkg.name == name {
                return serde_json::to_string(&pkg.state)
                    .unwrap_or_default()
                    .trim_matches('"')
                    .to_string();
            }
        }
    }
    "unknown".to_string()
}

// --- Services section ---

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
                config_tree_units.insert(format!("{}.timer", u.name));
                config_tree_units.insert(format!("{}.service", u.name));
            }
        }
    }

    let services = match &snap.services {
        Some(s) => s,
        None => return lines,
    };

    if services.enabled_units.is_empty() && services.disabled_units.is_empty() {
        return lines;
    }

    lines.push("# === Service Enablement ===".into());

    let mut safe_enabled = Vec::new();
    let mut safe_disabled = Vec::new();
    let mut deferred = Vec::new();

    for u in &services.enabled_units {
        if sanitize_shell_value(u).is_none() {
            continue;
        }
        if config_tree_units.contains(u) {
            deferred.push(u.clone());
            continue;
        }
        safe_enabled.push(u.clone());
    }
    for u in &services.disabled_units {
        if sanitize_shell_value(u).is_none() {
            continue;
        }
        safe_disabled.push(u.clone());
    }

    if !safe_enabled.is_empty() {
        lines.push(format!("RUN systemctl enable {}", safe_enabled.join(" ")));
    }
    if !safe_disabled.is_empty() {
        lines.push(format!("RUN systemctl disable {}", safe_disabled.join(" ")));
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
    for u in &included_timers {
        if !u.name.is_empty() {
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
    if !timer_names.is_empty() {
        lines.push(format!("RUN systemctl enable {}", timer_names.join(" ")));
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

// --- SELinux section ---

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

    lines.push("# === SELinux Customizations ===".into());

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
                    state: PackageState::Added,
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
        assert!(output.contains("FROM"), "must contain FROM line");
        assert!(
            output.contains("RUN dnf install -y"),
            "must contain dnf install"
        );
        assert!(output.contains("httpd"), "must contain httpd");
        assert!(output.contains("vim-enhanced"), "must contain vim-enhanced");
    }

    #[test]
    fn test_containerfile_section_ordering() {
        // Build a snapshot with data in multiple sections to verify ordering
        let mut snap = snapshot_with_packages(&["httpd"]);
        snap.services = Some(inspectah_core::types::services::ServiceSection {
            enabled_units: vec!["httpd.service".into()],
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
        let selinux_pos = output.find("SELinux").unwrap();
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
        assert!(output.contains("FROM"), "must contain FROM even if empty");
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
        let mut snap = InspectionSnapshot::new();
        snap.services = Some(inspectah_core::types::services::ServiceSection {
            enabled_units: vec!["httpd.service".into(), "sshd.service".into()],
            disabled_units: vec!["cups.service".into()],
            ..Default::default()
        });
        let output = render_containerfile(&snap, None);
        assert!(output.contains("systemctl enable httpd.service sshd.service"));
        assert!(output.contains("systemctl disable cups.service"));
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
        // Warning must appear before the FROM line
        let warning_pos = output.find("WARNING").unwrap();
        let from_pos = output.find("FROM").unwrap();
        assert!(
            warning_pos < from_pos,
            "completeness warning must appear before FROM line"
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
        assert!(
            output.contains("rpm --import /etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9"),
            "must have rpm --import for standard path key"
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
        assert!(
            output.contains("rpm --import /etc/pki/rpm-gpg/GOOD-KEY"),
            "safe path must still work"
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
}
