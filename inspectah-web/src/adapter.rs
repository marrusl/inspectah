// inspectah-web/src/adapter.rs
//
// Builds a `ViewResponse` from a `RefineSession` by reading session state
// and mapping domain types to presentation-layer DTOs. Produces JSON
// identical to the legacy `build_view_response` in handlers.rs.
//
// Per-section web adapters convert typed domain data from
// `ReferenceProjection` into `ReferenceSection`/`ContextItem` for the wire.

use std::collections::BTreeSet;
use std::path::Path;

use inspectah_core::types::rpm::VersionChangeDirection;
use inspectah_core::types::services::{PresetDefault, ServiceUnitState};
use inspectah_core::types::users::UserGroupDecision;
use inspectah_pipeline::render::service_intent::AdvisoryReason;
use inspectah_refine::classify::{
    classify_containers, classify_services, classify_sysctls, classify_tuned,
};
use inspectah_refine::projection::{
    GenericRefItem, RefContainers, RefKernelBoot, RefNetwork, RefServices, RefStorage,
    RefVersionChanges, ReferenceProjection,
};
use inspectah_refine::repo_index::RepoIndex;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::RepoProvenance;

use crate::web_types::{
    ContextItem, ContextSubsection, DropInDecisionDto, FlatpakDecisionDto, QuadletDecisionDto,
    RepoGroupInfo, ReferenceSection, ServiceDecisionDto, SysctlDecisionDto, TunedDecisionDto,
    VersionChangeEntry, ViewResponse,
};

/// Build a complete [`ViewResponse`] from session state.
///
/// This is a pure mapping layer: it reads the session's view, decisions,
/// and baseline summary, then maps each domain type to its DTO counterpart.
/// The output serializes to JSON identical to the legacy handler path.
pub fn build_web_view(session: &RefineSession) -> ViewResponse {
    let view = session.view().clone();
    let repo_groups = build_repo_groups(session);
    let baseline_summary = session.baseline_summary();
    let version_changes = build_version_changes(session);
    let (service_states, service_dropins) = build_service_decisions(session);
    let (quadlets, flatpaks) = build_container_decisions(session);
    let sysctls = build_sysctl_decisions(session);
    let tuned = build_tuned_decisions(session);
    let users_groups_decisions = build_users_groups_decisions(session);
    let session_is_sensitive = session.is_sensitive();

    ViewResponse {
        view,
        repo_groups,
        baseline_summary,
        version_changes,
        service_states,
        service_dropins,
        quadlets,
        flatpaks,
        sysctls,
        tuned,
        users_groups_decisions,
        session_is_sensitive,
    }
}

// -- Version changes -------------------------------------------------------

fn build_version_changes(session: &RefineSession) -> Vec<VersionChangeEntry> {
    session
        .snapshot()
        .rpm
        .as_ref()
        .map(|rpm| {
            rpm.version_changes
                .iter()
                .map(|vc| {
                    let dir = match vc.direction {
                        VersionChangeDirection::Upgrade => "upgrade",
                        VersionChangeDirection::Downgrade => "downgrade",
                    };
                    VersionChangeEntry {
                        name: vc.name.clone(),
                        arch: vc.arch.clone(),
                        host_version: vc.host_version.clone(),
                        base_version: vc.base_version.clone(),
                        host_epoch: vc.host_epoch.clone(),
                        base_epoch: vc.base_epoch.clone(),
                        direction: dir.to_string(),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

// -- Repo groups -----------------------------------------------------------

fn build_repo_groups(session: &RefineSession) -> Vec<RepoGroupInfo> {
    let view = session.view();
    let repo_index = session.repo_index();
    let changes = session.pending_changes();
    let repos_excluded = changes.repos_excluded();
    let excluded: BTreeSet<&str> = repos_excluded.iter().map(|s| s.as_str()).collect();

    // Count visible packages per source_repo (lowercased for consistency
    // with RepoIndex, which normalizes section IDs to lowercase).
    let mut repo_counts: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for pkg in &view.packages {
        let section = pkg.entry.source_repo.to_lowercase();
        *repo_counts.entry(section).or_insert(0) += 1;
    }

    // Also include repos known to the index but not visible (0-count).
    for section_id in repo_index.packages_by_repo.keys() {
        repo_counts.entry(section_id.clone()).or_insert(0);
    }

    let mut groups: Vec<RepoGroupInfo> = repo_counts
        .into_iter()
        .map(|(section_id, package_count)| {
            let provenance = if section_id.is_empty() {
                RepoProvenance::Unknown
            } else {
                repo_index.provenance(&section_id)
            };
            let is_distro = if section_id.is_empty() {
                false
            } else {
                RepoIndex::is_distro_repo(&section_id)
            };
            let tier = RepoIndex::repo_tier(&section_id);
            let enabled = !excluded.contains(section_id.as_str());
            RepoGroupInfo {
                section_id,
                provenance,
                is_distro,
                tier,
                package_count,
                enabled,
            }
        })
        .collect();

    // Sort: distro repos first, then by section_id.
    groups.sort_by(|a, b| {
        b.is_distro
            .cmp(&a.is_distro)
            .then_with(|| a.section_id.cmp(&b.section_id))
    });

    groups
}

// -- Service decisions -----------------------------------------------------

fn build_service_decisions(
    session: &RefineSession,
) -> (Vec<ServiceDecisionDto>, Vec<DropInDecisionDto>) {
    let snap = session.snapshot_projected();
    let (states, dropins) = classify_services(&snap);

    let state_dtos: Vec<ServiceDecisionDto> = states
        .into_iter()
        .map(|s| ServiceDecisionDto {
            unit: s.entry.unit.clone(),
            triage: s.triage,
            include: s.entry.include,
            owning_package: s.entry.owning_package.clone(),
        })
        .collect();

    let dropin_dtos: Vec<DropInDecisionDto> = dropins
        .into_iter()
        .map(|d| DropInDecisionDto {
            unit: d.entry.unit.clone(),
            path: d.entry.path.clone(),
            triage: d.triage,
            include: d.entry.include,
        })
        .collect();

    (state_dtos, dropin_dtos)
}

// -- Container decisions ---------------------------------------------------

fn build_container_decisions(
    session: &RefineSession,
) -> (Vec<QuadletDecisionDto>, Vec<FlatpakDecisionDto>) {
    let snap = session.snapshot_projected();
    let (quadlets, flatpaks) = classify_containers(&snap);

    let quadlet_dtos: Vec<QuadletDecisionDto> = quadlets
        .into_iter()
        .map(|q| QuadletDecisionDto {
            path: q.entry.path.clone(),
            name: q.entry.name.clone(),
            image: q.entry.image.clone(),
            triage: q.triage,
            include: q.entry.include,
        })
        .collect();

    let flatpak_dtos: Vec<FlatpakDecisionDto> = flatpaks
        .into_iter()
        .map(|f| FlatpakDecisionDto {
            app_id: f.entry.app_id.clone(),
            remote: f.entry.remote.clone(),
            branch: f.entry.branch.clone(),
            triage: f.triage,
            include: f.entry.include,
            lifecycle: "first_boot".to_string(),
        })
        .collect();

    (quadlet_dtos, flatpak_dtos)
}

// -- Sysctl decisions ------------------------------------------------------

fn build_sysctl_decisions(session: &RefineSession) -> Vec<SysctlDecisionDto> {
    let snap = session.snapshot_projected();
    classify_sysctls(&snap)
        .into_iter()
        .map(|s| SysctlDecisionDto {
            key: s.entry.key.clone(),
            runtime: s.entry.runtime.clone(),
            default: s.entry.default.clone(),
            source: s.entry.source.clone(),
            triage: s.triage,
            include: s.entry.include,
        })
        .collect()
}

// -- Tuned decisions -------------------------------------------------------

fn build_tuned_decisions(session: &RefineSession) -> Vec<TunedDecisionDto> {
    let snap = session.snapshot_projected();
    let tuned_include = snap.kernel_boot.as_ref().is_none_or(|kb| kb.tuned_include);
    classify_tuned(&snap)
        .into_iter()
        .map(|t| TunedDecisionDto {
            active_profile: t.active_profile.clone(),
            custom_profiles: t.custom_profiles.clone(),
            triage: t.triage,
            include: tuned_include,
        })
        .collect()
}

// -- Users/groups decisions ------------------------------------------------

fn build_users_groups_decisions(session: &RefineSession) -> Vec<UserGroupDecision> {
    session
        .snapshot_projected()
        .users_groups
        .map(|ug| ug.users)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect()
}

// =========================================================================
// Reference section adapters
//
// Each adapter takes typed domain data from ReferenceProjection and
// produces ReferenceSection/ContextItem for the wire.  Presentation logic
// (subtitles, searchable_text, display_name, empty_reason strings) is
// ported from each normalize_* function in handlers.rs.
// =========================================================================

// -- Services --------------------------------------------------------------

/// Map a (current_state, default_state) pair to a human-readable subtitle.
///
/// Ported from `typed_service_subtitle` in handlers.rs.
fn typed_service_subtitle(current: ServiceUnitState, default: Option<PresetDefault>) -> String {
    match (current, default) {
        (ServiceUnitState::Enabled, Some(PresetDefault::Disable)) => {
            "enabled (diverges from preset: disable)".to_string()
        }
        (ServiceUnitState::Disabled, Some(PresetDefault::Enable)) => {
            "disabled (diverges from preset: enable)".to_string()
        }
        (ServiceUnitState::Masked, Some(PresetDefault::Enable)) => {
            "masked (preset default: enable)".to_string()
        }
        (ServiceUnitState::Masked, Some(PresetDefault::Disable)) => {
            "masked (preset default: disable)".to_string()
        }
        (ServiceUnitState::Masked, None) => "masked (no preset rule)".to_string(),
        (state, Some(d)) => format!("{} (diverges from preset: {})", state, d),
        (state, None) => format!("{} (no preset rule)", state),
    }
}

/// Derive the implied action string from a ServiceUnitState.
///
/// Mirrors `ServiceStateChange::implied_action().to_string()`.
fn implied_action_str(state: ServiceUnitState) -> &'static str {
    match state {
        ServiceUnitState::Enabled => "enable",
        ServiceUnitState::Disabled => "disable",
        ServiceUnitState::Masked => "mask",
    }
}

pub fn web_services_section(data: &RefServices) -> ReferenceSection {
    let mut items = Vec::new();
    let mut subsections = Vec::new();

    // 1. Divergent items — typed subtitles
    for svc in &data.divergent {
        let subtitle = typed_service_subtitle(svc.current_state, svc.default_state);
        let dropin_detail = if svc.dropin_contents.is_empty() {
            None
        } else {
            Some(svc.dropin_contents.join("\n---\n"))
        };
        let state_str = svc.current_state.to_string();
        let action_str = implied_action_str(svc.current_state);
        let default_str = svc
            .default_state
            .map(|d| d.to_string())
            .unwrap_or_else(|| "none".to_string());
        let mut search = format!("{} {} {} {}", svc.unit, state_str, default_str, action_str);
        if let Some(pkg) = &svc.owning_package {
            search.push(' ');
            search.push_str(pkg);
        }
        items.push(ContextItem {
            id: svc.unit.clone(),
            title: svc.unit.clone(),
            subtitle: Some(subtitle),
            detail: dropin_detail,
            searchable_text: search,
        });
    }

    // 2. Preset-matched with drop-in (visible with annotation)
    for svc in &data.preset_matched_with_dropins {
        let state = svc.current_state.to_string();
        let detail = if svc.dropin_contents.is_empty() {
            None
        } else {
            Some(svc.dropin_contents.join("\n---\n"))
        };
        items.push(ContextItem {
            id: svc.unit.clone(),
            title: svc.unit.clone(),
            subtitle: Some(format!("{} (matches preset, has drop-in override)", state)),
            detail,
            searchable_text: format!("{} {} drop-in override", svc.unit, state),
        });
    }

    // 3. Preset-unknown enabled units
    for svc in &data.preset_unknown_enabled {
        let dropin_detail = if svc.dropin_contents.is_empty() {
            None
        } else {
            Some(svc.dropin_contents.join("\n---\n"))
        };
        items.push(ContextItem {
            id: svc.unit.clone(),
            title: svc.unit.clone(),
            subtitle: Some("enabled (no preset rule)".into()),
            detail: dropin_detail,
            searchable_text: format!("{} enabled no preset rule", svc.unit),
        });
    }

    // 4. Preset-unknown disabled units
    for svc in &data.preset_unknown_disabled {
        items.push(ContextItem {
            id: svc.unit.clone(),
            title: svc.unit.clone(),
            subtitle: Some("disabled (no preset rule)".into()),
            detail: None,
            searchable_text: format!("{} disabled no preset rule", svc.unit),
        });
    }

    // 5. Standalone drop-ins
    for d in &data.standalone_dropins {
        items.push(ContextItem {
            id: format!("dropin-{}", d.unit),
            title: format!("{} (drop-in)", d.unit),
            subtitle: Some("drop-in override".into()),
            detail: Some(d.content.clone()),
            searchable_text: format!("{} drop-in", d.unit),
        });
    }

    // -- Subsections --

    // Omitted services (package proven absent)
    if !data.omitted.is_empty() {
        let omission_items: Vec<ContextItem> = data
            .omitted
            .iter()
            .map(|o| ContextItem {
                id: format!("omitted-{}", o.unit),
                title: o.unit.clone(),
                subtitle: Some(format!(
                    "package '{}' not in target image",
                    o.package
                )),
                detail: None,
                searchable_text: format!("{} omitted {}", o.unit, o.package),
            })
            .collect();
        subsections.push(ContextSubsection {
            id: "omitted_services".to_string(),
            display_name: "Omitted Services".to_string(),
            items: omission_items,
        });
    }

    // Service advisories (presence uncertain)
    if !data.advisories.is_empty() {
        let advisory_items: Vec<ContextItem> = data
            .advisories
            .iter()
            .map(|a| {
                let reasons_str: Vec<&str> = a
                    .reasons
                    .iter()
                    .map(|r| match r {
                        AdvisoryReason::PackageExcluded => "package excluded",
                        AdvisoryReason::PackageUnreachable => "package unreachable",
                        AdvisoryReason::BaselineUnavailable => "baseline unavailable",
                    })
                    .collect();
                ContextItem {
                    id: format!("advisory-{}", a.unit),
                    title: a.unit.clone(),
                    subtitle: Some(format!(
                        "package '{}': {}",
                        a.owning_package,
                        reasons_str.join("; ")
                    )),
                    detail: None,
                    searchable_text: format!(
                        "{} advisory {} {}",
                        a.unit,
                        a.owning_package,
                        reasons_str.join(" ")
                    ),
                }
            })
            .collect();
        subsections.push(ContextSubsection {
            id: "service_advisories".to_string(),
            display_name: "Service Advisories".to_string(),
            items: advisory_items,
        });
    }

    // Service warnings (from collector)
    if !data.warnings.is_empty() {
        let warning_items: Vec<ContextItem> = data
            .warnings
            .iter()
            .map(|w| ContextItem {
                id: format!("warning-{}", w.unit),
                title: w.unit.clone(),
                subtitle: Some(w.message.clone()),
                detail: None,
                searchable_text: format!("warning {}", w.message),
            })
            .collect();
        subsections.push(ContextSubsection {
            id: "service_warnings".to_string(),
            display_name: "Service Warnings".to_string(),
            items: warning_items,
        });
    }

    ReferenceSection {
        id: "services".to_string(),
        display_name: "Services".to_string(),
        items,
        subsections,
        empty_reason: None,
    }
}

// -- Version changes -------------------------------------------------------

/// Format an epoch+version pair, showing epochs only when they differ.
///
/// Ported from `format_evr_pair` in handlers.rs.
fn format_evr_pair(
    base_epoch: &str,
    base_version: &str,
    host_epoch: &str,
    host_version: &str,
) -> (String, String) {
    fn norm(e: &str) -> &str {
        if e.is_empty() {
            "0"
        } else {
            e
        }
    }
    let base_norm = norm(base_epoch);
    let host_norm = norm(host_epoch);
    let show_epoch = base_norm != host_norm || base_norm != "0";

    let fmt = |epoch: &str, version: &str| -> String {
        if show_epoch {
            let e = if epoch.is_empty() { "0" } else { epoch };
            format!("{}:{}", e, version)
        } else {
            version.to_string()
        }
    };

    (fmt(base_epoch, base_version), fmt(host_epoch, host_version))
}

pub fn web_version_changes_section(data: &RefVersionChanges) -> ReferenceSection {
    use inspectah_refine::projection::EmptyReason;

    // Three-state empty reason
    if data.downgrades.is_empty() && data.upgrades.is_empty() {
        let reason = data.empty_reason.as_ref().map(|r| match r {
            EmptyReason::DataUnavailable => "data_unavailable".to_string(),
            EmptyReason::NoBaseline => "no_baseline".to_string(),
            EmptyReason::ZeroDrift => "zero_drift".to_string(),
        });
        return ReferenceSection {
            id: "version_changes".to_string(),
            display_name: "Version Changes".to_string(),
            items: Vec::new(),
            subsections: Vec::new(),
            empty_reason: reason,
        };
    }

    let mut items = Vec::new();
    for vc in data.downgrades.iter().chain(data.upgrades.iter()) {
        let (base_fmt, host_fmt) = format_evr_pair(
            &vc.base_epoch,
            &vc.base_version,
            &vc.host_epoch,
            &vc.host_version,
        );
        let dir_label = match vc.direction {
            VersionChangeDirection::Downgrade => "downgrade",
            VersionChangeDirection::Upgrade => "upgrade",
        };
        let prefix = match vc.direction {
            VersionChangeDirection::Downgrade => "\u{25BC} ",
            VersionChangeDirection::Upgrade => "",
        };
        let title = format!("{}{}.{}", prefix, vc.name, vc.arch);
        let subtitle = format!("{} \u{2192} {} ({})", host_fmt, base_fmt, dir_label);
        let searchable = format!("{} {} {}", vc.name, vc.arch, dir_label);

        items.push(ContextItem {
            id: format!("{}.{}", vc.name, vc.arch),
            title,
            subtitle: Some(subtitle),
            detail: None,
            searchable_text: searchable,
        });
    }

    ReferenceSection {
        id: "version_changes".to_string(),
        display_name: "Version Changes".to_string(),
        items,
        subsections: Vec::new(),
        empty_reason: None,
    }
}

// -- Containers ------------------------------------------------------------

pub fn web_containers_section(data: &RefContainers) -> ReferenceSection {
    let mut items = Vec::new();

    // QuadletUnit
    for q in &data.quadlets {
        let mut search = format!("{} {} {}", q.name, q.image, q.path);
        if !q.ports.is_empty() {
            search.push(' ');
            search.push_str(&q.ports.join(" "));
        }
        if !q.volumes.is_empty() {
            search.push(' ');
            search.push_str(&q.volumes.join(" "));
        }
        items.push(ContextItem {
            id: q.name.clone(),
            title: q.name.clone(),
            subtitle: Some(q.image.clone()),
            detail: Some(q.content.clone()),
            searchable_text: search,
        });
    }

    // ComposeFile
    for cf in &data.compose_files {
        let basename = Path::new(&cf.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| cf.path.clone());
        let service_names: Vec<&str> = cf.services.iter().map(|s| s.service.as_str()).collect();
        let subtitle = service_names.join(", ");
        let mut search = format!("{} {}", cf.path, service_names.join(" "));
        // Append image refs for searchability
        for svc in &cf.services {
            if !svc.image.is_empty() {
                search.push(' ');
                search.push_str(&svc.image);
            }
        }
        items.push(ContextItem {
            id: cf.path.clone(),
            title: basename,
            subtitle: Some(subtitle),
            detail: None,
            searchable_text: search,
        });
    }

    // RunningContainer
    for rc in &data.running_containers {
        let subtitle = format!("{} ({})", rc.image, rc.status);
        let mut detail_parts = Vec::new();
        if !rc.env.is_empty() {
            detail_parts.push(format!("env: {}", rc.env.join(", ")));
        }
        if !rc.mounts.is_empty() {
            let mount_strs: Vec<String> = rc
                .mounts
                .iter()
                .map(|m| format!("{} {}:{}", m.mount_type, m.source, m.destination))
                .collect();
            detail_parts.push(format!("mounts: {}", mount_strs.join("; ")));
        }
        let detail = if detail_parts.is_empty() {
            None
        } else {
            Some(detail_parts.join("\n"))
        };
        let mut search = format!("{} {} {}", rc.name, rc.image, rc.status);
        if !rc.restart_policy.is_empty() {
            search.push(' ');
            search.push_str(&rc.restart_policy);
        }
        items.push(ContextItem {
            id: rc.id.clone(),
            title: rc.name.clone(),
            subtitle: Some(subtitle),
            detail,
            searchable_text: search,
        });
    }

    // FlatpakApp
    for fa in &data.flatpaks {
        let mut search = fa.app_id.clone();
        search.push(' ');
        search.push_str(&fa.origin);
        search.push(' ');
        search.push_str(&fa.branch);
        if !fa.remote.is_empty() {
            search.push(' ');
            search.push_str(&fa.remote);
        }
        if !fa.remote_url.is_empty() {
            search.push(' ');
            search.push_str(&fa.remote_url);
        }
        items.push(ContextItem {
            id: fa.app_id.clone(),
            title: fa.app_id.clone(),
            subtitle: Some(format!("{}/{}", fa.origin, fa.branch)),
            detail: None,
            searchable_text: search,
        });
    }

    crate::web_types::reference_section("containers", "Containers", items)
}

// -- Kernel & Boot ---------------------------------------------------------

pub fn web_kernel_boot_section(data: &RefKernelBoot) -> ReferenceSection {
    let mut items = Vec::new();

    // cmdline — single item
    if let Some(cmdline) = &data.cmdline {
        let subtitle = if cmdline.len() > 80 {
            Some(format!("{}...", &cmdline[..77]))
        } else {
            Some(cmdline.clone())
        };
        items.push(ContextItem {
            id: "cmdline".to_string(),
            title: "Kernel cmdline".to_string(),
            subtitle,
            detail: Some(cmdline.clone()),
            searchable_text: cmdline.clone(),
        });
    }

    // grub_defaults — single item
    if let Some(grub) = &data.grub_defaults {
        items.push(ContextItem {
            id: "grub_defaults".to_string(),
            title: "GRUB defaults".to_string(),
            subtitle: None,
            detail: Some(grub.clone()),
            searchable_text: grub.clone(),
        });
    }

    // tuned_active — single item
    if let Some(tuned) = &data.tuned_active {
        items.push(ContextItem {
            id: "tuned_active".to_string(),
            title: "Active tuned profile".to_string(),
            subtitle: Some(tuned.clone()),
            detail: None,
            searchable_text: tuned.clone(),
        });
    }

    // locale — single item (optional)
    if let Some(locale) = &data.locale {
        items.push(ContextItem {
            id: "locale".to_string(),
            title: "Locale".to_string(),
            subtitle: Some(locale.clone()),
            detail: None,
            searchable_text: locale.clone(),
        });
    }

    // timezone — single item (optional)
    if let Some(tz) = &data.timezone {
        items.push(ContextItem {
            id: "timezone".to_string(),
            title: "Timezone".to_string(),
            subtitle: Some(tz.clone()),
            detail: None,
            searchable_text: tz.clone(),
        });
    }

    // SysctlOverride
    for so in &data.sysctl_overrides {
        items.push(ContextItem {
            id: so.key.clone(),
            title: so.key.clone(),
            subtitle: Some(format!("\"{}\" (default: \"{}\")", so.runtime, so.default)),
            detail: Some(so.source.clone()),
            searchable_text: format!("{} {} {} {}", so.key, so.runtime, so.default, so.source),
        });
    }

    // KernelModule (non_default_modules only)
    for km in &data.non_default_modules {
        items.push(ContextItem {
            id: km.name.clone(),
            title: km.name.clone(),
            subtitle: Some(format!("size: {}", km.size)),
            detail: if km.used_by.is_empty() {
                None
            } else {
                Some(km.used_by.clone())
            },
            searchable_text: format!("{} {} {}", km.name, km.size, km.used_by),
        });
    }

    // Helper for config snippet items
    let snippet_items =
        |snippets: &[inspectah_refine::projection::RefConfigSnippet], label: &str| -> Vec<ContextItem> {
            snippets
                .iter()
                .map(|cs| {
                    let basename = Path::new(&cs.path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| cs.path.clone());
                    ContextItem {
                        id: cs.path.clone(),
                        title: basename,
                        subtitle: Some(label.to_string()),
                        detail: Some(cs.content.clone()),
                        searchable_text: format!("{} {}", cs.path, cs.content),
                    }
                })
                .collect()
        };

    items.extend(snippet_items(&data.modules_load_d, "modules-load.d"));
    items.extend(snippet_items(&data.modprobe_d, "modprobe.d"));
    items.extend(snippet_items(&data.dracut_conf, "dracut.conf.d"));
    items.extend(snippet_items(&data.custom_tuned_profiles, "tuned profile"));

    // AlternativeEntry
    for ae in &data.alternatives {
        items.push(ContextItem {
            id: ae.name.clone(),
            title: ae.name.clone(),
            subtitle: Some(format!("{} ({})", ae.path, ae.status)),
            detail: None,
            searchable_text: format!("{} {} {}", ae.name, ae.path, ae.status),
        });
    }

    crate::web_types::reference_section("kernel_boot", "Kernel & Boot", items)
}

// -- Network ---------------------------------------------------------------

pub fn web_network_section(data: &RefNetwork) -> ReferenceSection {
    let mut items = Vec::new();

    // NMConnection
    for conn in &data.connections {
        items.push(ContextItem {
            id: conn.name.clone(),
            title: conn.name.clone(),
            subtitle: Some(format!("{} ({})", conn.conn_type, conn.method)),
            detail: None,
            searchable_text: format!(
                "{} {} {} {}",
                conn.name, conn.conn_type, conn.method, conn.path
            ),
        });
    }

    // FirewallZone
    for zone in &data.firewall_zones {
        let mut summary_parts = Vec::new();
        if !zone.services.is_empty() {
            summary_parts.push(format!("services: {}", zone.services.join(", ")));
        }
        if !zone.ports.is_empty() {
            summary_parts.push(format!("ports: {}", zone.ports.join(", ")));
        }
        let subtitle = if summary_parts.is_empty() {
            None
        } else {
            Some(summary_parts.join("; "))
        };
        items.push(ContextItem {
            id: zone.name.clone(),
            title: zone.name.clone(),
            subtitle,
            detail: Some(zone.content.clone()),
            searchable_text: format!(
                "{} {} {} {}",
                zone.name,
                zone.services.join(" "),
                zone.ports.join(" "),
                zone.rich_rules.join(" ")
            ),
        });
    }

    // FirewallDirectRule
    for rule in &data.firewall_direct_rules {
        let id = format!("{}:{}:{}", rule.ipv, rule.chain, rule.priority);
        items.push(ContextItem {
            id,
            title: rule.chain.clone(),
            subtitle: Some(format!("{} {}", rule.ipv, rule.table)),
            detail: Some(rule.args.clone()),
            searchable_text: format!(
                "{} {} {} {} {}",
                rule.ipv, rule.table, rule.chain, rule.priority, rule.args
            ),
        });
    }

    // StaticRouteFile
    for sr in &data.static_routes {
        items.push(ContextItem {
            id: sr.path.clone(),
            title: sr.name.clone(),
            subtitle: Some(sr.path.clone()),
            detail: None,
            searchable_text: format!("{} {}", sr.path, sr.name),
        });
    }

    // ip_routes
    for route in &data.ip_routes {
        items.push(ContextItem {
            id: route.clone(),
            title: route.clone(),
            subtitle: Some("ip route".to_string()),
            detail: None,
            searchable_text: route.clone(),
        });
    }

    // ip_rules
    for rule in &data.ip_rules {
        items.push(ContextItem {
            id: rule.clone(),
            title: rule.clone(),
            subtitle: Some("ip rule".to_string()),
            detail: None,
            searchable_text: rule.clone(),
        });
    }

    // resolv_provenance
    if !data.resolv_provenance.is_empty() {
        items.push(ContextItem {
            id: "resolv_provenance".to_string(),
            title: "DNS resolver".to_string(),
            subtitle: Some(data.resolv_provenance.clone()),
            detail: None,
            searchable_text: data.resolv_provenance.clone(),
        });
    }

    // hosts_additions
    for line in &data.hosts_additions {
        items.push(ContextItem {
            id: line.clone(),
            title: line.clone(),
            subtitle: Some("hosts".to_string()),
            detail: None,
            searchable_text: line.clone(),
        });
    }

    // ProxyEntry
    for proxy in &data.proxy_env {
        let id = format!("{}:{}", proxy.source, proxy.line);
        items.push(ContextItem {
            id,
            title: proxy.source.clone(),
            subtitle: Some(proxy.line.clone()),
            detail: None,
            searchable_text: format!("{} {}", proxy.source, proxy.line),
        });
    }

    crate::web_types::reference_section("network", "Network", items)
}

// -- Storage ---------------------------------------------------------------

pub fn web_storage_section(data: &RefStorage) -> ReferenceSection {
    let mut items = Vec::new();

    // FstabEntry
    for entry in &data.fstab_entries {
        items.push(ContextItem {
            id: entry.mount_point.clone(),
            title: entry.mount_point.clone(),
            subtitle: Some(format!("{} ({})", entry.device, entry.fstype)),
            detail: Some(entry.options.clone()),
            searchable_text: format!(
                "{} {} {} {}",
                entry.device, entry.mount_point, entry.fstype, entry.options
            ),
        });
    }

    // MountPoint
    for mp in &data.mount_points {
        items.push(ContextItem {
            id: mp.target.clone(),
            title: mp.target.clone(),
            subtitle: Some(format!("{} ({})", mp.source, mp.fstype)),
            detail: Some(mp.options.clone()),
            searchable_text: format!("{} {} {}", mp.target, mp.source, mp.fstype),
        });
    }

    // LvmVolume
    for lv in &data.lvm_volumes {
        let id = format!("{}/{}", lv.vg_name, lv.lv_name);
        items.push(ContextItem {
            id,
            title: lv.lv_name.clone(),
            subtitle: Some(format!("VG: {}, size: {}", lv.vg_name, lv.lv_size)),
            detail: None,
            searchable_text: format!("{} {} {}", lv.lv_name, lv.vg_name, lv.lv_size),
        });
    }

    // VarDirectory
    for vd in &data.var_directories {
        items.push(ContextItem {
            id: vd.path.clone(),
            title: vd.path.clone(),
            subtitle: Some(format!("~{}", vd.size_estimate)),
            detail: Some(vd.recommendation.clone()),
            searchable_text: format!("{} {} {}", vd.path, vd.size_estimate, vd.recommendation),
        });
    }

    // CredentialRef
    for cr in &data.credential_refs {
        items.push(ContextItem {
            id: cr.credential_path.clone(),
            title: cr.credential_path.clone(),
            subtitle: Some(format!("mount: {}", cr.mount_point)),
            detail: Some(cr.source.clone()),
            searchable_text: format!("{} {} {}", cr.credential_path, cr.mount_point, cr.source),
        });
    }

    crate::web_types::reference_section("storage", "Storage", items)
}

// -- Generic sections (scheduled_tasks, non_rpm_software, selinux) ---------

pub fn web_generic_section(
    id: &str,
    display_name: &str,
    items: &[GenericRefItem],
) -> ReferenceSection {
    let context_items: Vec<ContextItem> = items
        .iter()
        .map(|item| ContextItem {
            id: item.id.clone(),
            title: item.key.clone(),
            subtitle: item.summary.clone(),
            detail: item.content.clone(),
            searchable_text: {
                let mut search = item.key.clone();
                if let Some(s) = &item.summary {
                    search.push(' ');
                    search.push_str(s);
                }
                if !item.tags.is_empty() {
                    search.push(' ');
                    search.push_str(&item.tags.join(" "));
                }
                search
            },
        })
        .collect();
    crate::web_types::reference_section(id, display_name, context_items)
}

// -- Orchestrator ----------------------------------------------------------

/// Build all 9 reference sections in canonical order.
///
/// Order MUST match `normalize_for_reference` in handlers.rs:
/// services, version_changes, containers, network, storage,
/// scheduled_tasks, non_rpm_software, kernel_boot, selinux.
pub fn build_web_sections(ref_proj: &ReferenceProjection) -> Vec<ReferenceSection> {
    vec![
        web_services_section(&ref_proj.services),
        web_version_changes_section(&ref_proj.version_changes),
        web_containers_section(&ref_proj.containers),
        web_network_section(&ref_proj.network),
        web_storage_section(&ref_proj.storage),
        web_generic_section("scheduled_tasks", "Scheduled Tasks", &ref_proj.scheduled_tasks),
        web_generic_section(
            "non_rpm_software",
            "Non-RPM Software",
            &ref_proj.non_rpm_software,
        ),
        web_kernel_boot_section(&ref_proj.kernel_boot),
        web_generic_section("selinux", "Security & Access Control", &ref_proj.selinux),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::snapshot::InspectionSnapshot;

    /// Verify that `build_web_view` produces the same JSON as the legacy
    /// `build_view_response` path in handlers.rs on an empty snapshot.
    #[test]
    fn adapter_matches_legacy_empty() {
        let snap = InspectionSnapshot::new();
        let session = RefineSession::new(snap);

        let adapter_response = build_web_view(&session);
        let legacy_response = crate::handlers::build_view_response_for_test(&session);

        let adapter_json =
            serde_json::to_value(&adapter_response).expect("serialize adapter response");
        let legacy_json =
            serde_json::to_value(&legacy_response).expect("serialize legacy response");

        assert_eq!(adapter_json, legacy_json, "adapter and legacy JSON must match");
    }

    /// Verify equivalence on a snapshot with RPM data (version changes, packages).
    #[test]
    fn adapter_matches_legacy_with_rpm() {
        use inspectah_core::types::rpm::{
            PackageEntry, PackageState, RpmSection, VersionChange, VersionChangeDirection,
        };

        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            }],
            version_changes: vec![VersionChange {
                name: "curl".into(),
                arch: "x86_64".into(),
                host_version: "8.6.0".into(),
                base_version: "8.5.0".into(),
                host_epoch: "0".into(),
                base_epoch: "0".into(),
                direction: VersionChangeDirection::Upgrade,
            }],
            ..Default::default()
        });
        let session = RefineSession::new(snap);

        let adapter_json =
            serde_json::to_value(build_web_view(&session)).expect("serialize adapter");
        let legacy_json =
            serde_json::to_value(crate::handlers::build_view_response_for_test(&session))
                .expect("serialize legacy");

        assert_eq!(adapter_json, legacy_json);
    }

    /// Verify `build_web_sections` returns 9 sections with correct ids in
    /// canonical order (matching `normalize_for_reference`).
    #[test]
    fn build_web_sections_returns_9_sections_in_order() {
        use inspectah_refine::projection::project_reference;

        let snap = InspectionSnapshot::new();
        let ref_proj = project_reference(&snap);
        let sections = build_web_sections(&ref_proj);

        assert_eq!(sections.len(), 9, "must return exactly 9 sections");

        let expected_ids = [
            "services",
            "version_changes",
            "containers",
            "network",
            "storage",
            "scheduled_tasks",
            "non_rpm_software",
            "kernel_boot",
            "selinux",
        ];
        let actual_ids: Vec<&str> = sections.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(actual_ids, expected_ids, "section ids must match canonical order");
    }

    /// Verify `build_web_sections` returns correct display_name for each section.
    #[test]
    fn build_web_sections_display_names() {
        use inspectah_refine::projection::project_reference;

        let snap = InspectionSnapshot::new();
        let ref_proj = project_reference(&snap);
        let sections = build_web_sections(&ref_proj);

        let expected_names = [
            "Services",
            "Version Changes",
            "Containers",
            "Network",
            "Storage",
            "Scheduled Tasks",
            "Non-RPM Software",
            "Kernel & Boot",
            "Security & Access Control",
        ];
        let actual_names: Vec<&str> = sections.iter().map(|s| s.display_name.as_str()).collect();
        assert_eq!(actual_names, expected_names);
    }
}
