// inspectah-web/src/adapter.rs
//
// Builds a `ViewResponse` from a `RefineSession` by reading session state
// and mapping domain types to presentation-layer DTOs. Produces JSON
// identical to the legacy `build_view_response` in handlers.rs.
//
// Per-section web adapters convert typed domain data from
// `ReferenceProjection` into `ReferenceSection`/`ContextItem` for the wire.

use std::collections::HashMap;
use std::path::Path;

use inspectah_core::types::group_render::{DegradationReason, GroupRenderState};
use inspectah_core::types::nonrpm::FileType;
use inspectah_core::types::rpm::VersionChangeDirection;
use inspectah_core::types::services::{PresetDefault, ServiceUnitState};
use inspectah_pipeline::render::service_intent::AdvisoryReason;
use inspectah_refine::projection::{
    GenericRefItem, RefContainers, RefKernelBoot, RefNetwork, RefServices, RefStorage,
    RefVersionChanges, ReferenceProjection,
};
use inspectah_refine::session::RefineSession;

use crate::web_types::{
    ContextItem, ContextSubsection, DropInDecisionDto, FlatpakDecisionDto, GroupInfo,
    GroupMemberInfo, LanguagePackageEnvDto, PackageProvenance, ProvenanceSignalsDto,
    QuadletDecisionDto, ReferenceSection, RepoGroupInfo, ServiceDecisionDto, SysctlDecisionDto,
    TunedDecisionDto, UnmanagedFileGroupDto, UnmanagedFileItemDto, VersionChangeEntry,
    ViewResponse,
};

/// Build a complete [`ViewResponse`] from session state.
///
/// This is a pure mapping layer: it reads the session's view, decisions,
/// and baseline summary, then maps each domain type to its DTO counterpart.
/// The output serializes to JSON identical to the legacy handler path.
pub fn build_web_view(session: &RefineSession) -> ViewResponse {
    let view = session.view().clone();
    let decisions = session.decisions();

    // Map repo groups from projection type to DTO
    let repo_groups: Vec<RepoGroupInfo> = decisions
        .repo_groups
        .iter()
        .map(|rg| RepoGroupInfo {
            section_id: rg.section_id.clone(),
            provenance: rg.provenance,
            is_distro: rg.is_distro,
            tier: rg.tier,
            package_count: rg.package_count,
            enabled: rg.enabled,
        })
        .collect();

    // Map version changes from VersionChange to VersionChangeEntry
    let version_changes: Vec<VersionChangeEntry> = decisions
        .version_changes
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
        .collect();

    // Map service decisions from Refined types to DTOs
    let service_states: Vec<ServiceDecisionDto> = decisions
        .service_states
        .iter()
        .map(|s| ServiceDecisionDto {
            unit: s.entry.unit.clone(),
            triage: s.triage.clone(),
            include: s.entry.include,
            locked: s.entry.locked,
            attention_reason: s.entry.attention_reason.clone(),
            owning_package: s.entry.owning_package.clone(),
            default_state: s.entry.default_state.map(|d| d.to_string()),
            current_state: s.entry.current_state.to_string(),
        })
        .collect();

    let service_dropins: Vec<DropInDecisionDto> = decisions
        .service_dropins
        .iter()
        .map(|d| DropInDecisionDto {
            unit: d.entry.unit.clone(),
            path: d.entry.path.clone(),
            triage: d.triage.clone(),
            include: d.entry.include,
            locked: d.entry.locked,
            attention_reason: d.entry.attention_reason.clone(),
        })
        .collect();

    // Map container decisions
    let quadlets: Vec<QuadletDecisionDto> = decisions
        .quadlets
        .iter()
        .map(|q| {
            let content = if q.entry.content.is_empty() {
                None
            } else {
                Some(q.entry.content.clone())
            };
            QuadletDecisionDto {
                path: q.entry.path.clone(),
                name: q.entry.name.clone(),
                image: q.entry.image.clone(),
                triage: q.triage.clone(),
                include: q.entry.include,
                locked: q.entry.locked,
                content,
            }
        })
        .collect();

    let flatpaks: Vec<FlatpakDecisionDto> = decisions
        .flatpaks
        .iter()
        .map(|f| FlatpakDecisionDto {
            app_id: f.entry.app_id.clone(),
            remote: f.entry.remote.clone(),
            branch: f.entry.branch.clone(),
            triage: f.triage.clone(),
            include: f.entry.include,
            locked: f.entry.locked,
            lifecycle: "first_boot".to_string(),
        })
        .collect();

    // Map sysctl decisions
    let sysctls: Vec<SysctlDecisionDto> = decisions
        .sysctls
        .iter()
        .map(|s| SysctlDecisionDto {
            key: s.entry.key.clone(),
            runtime: s.entry.runtime.clone(),
            default: s.entry.default.clone(),
            source: s.entry.source.clone(),
            triage: s.triage.clone(),
            include: s.entry.include,
            locked: s.entry.locked,
        })
        .collect();

    // Map tuned decisions
    let tuned: Vec<TunedDecisionDto> = decisions
        .tuned
        .iter()
        .map(|t| TunedDecisionDto {
            active_profile: t.active_profile.clone(),
            custom_profiles: t.custom_profiles.clone(),
            triage: t.triage.clone(),
            include: t.include,
            locked: false,
        })
        .collect();

    // -- Package groups and provenance ----------------------------------------

    let render_ctx = session.render_context();
    let installed_groups = session
        .snapshot()
        .rpm
        .as_ref()
        .and_then(|r| r.installed_groups.as_deref())
        .unwrap_or(&[]);

    let package_groups: Vec<GroupInfo> = installed_groups
        .iter()
        .map(|group| {
            let render_state = render_ctx
                .group_states
                .get(&group.name)
                .cloned()
                .unwrap_or(GroupRenderState::Renderable);

            let (state_str, degradation_reason) = match &render_state {
                GroupRenderState::Renderable => ("renderable".to_string(), None),
                GroupRenderState::Excluded => ("excluded".to_string(), None),
                GroupRenderState::Ungrouped => ("ungrouped".to_string(), None),
                GroupRenderState::Degraded { reason } => {
                    let reason_str = match reason {
                        DegradationReason::MemberExcluded => "member_excluded",
                        DegradationReason::MemberOverridden => "member_overridden",
                        DegradationReason::MultilibConflict => "multilib_conflict",
                    };
                    ("degraded".to_string(), Some(reason_str.to_string()))
                }
            };

            // Count locked members by cross-referencing against projected packages
            let projected = session.snapshot_projected();
            let projected_pkgs = projected
                .rpm
                .as_ref()
                .map(|r| &r.packages_added[..])
                .unwrap_or(&[]);

            let locked_count = group
                .members
                .iter()
                .filter(|m| projected_pkgs.iter().any(|p| p.name == **m && p.locked))
                .count();

            // Build member list with overlap detection
            let mut members: Vec<GroupMemberInfo> = group
                .members
                .iter()
                .map(|member_name| {
                    let locked = projected_pkgs
                        .iter()
                        .any(|p| p.name == *member_name && p.locked);
                    let in_base_image = !projected_pkgs.iter().any(|p| p.name == *member_name);
                    let overlap_groups: Vec<String> = installed_groups
                        .iter()
                        .filter(|other| other.name != group.name)
                        .filter(|other| other.members.contains(member_name))
                        .map(|other| other.name.clone())
                        .collect();
                    GroupMemberInfo {
                        name: member_name.clone(),
                        locked,
                        overlap_groups,
                        in_base_image,
                    }
                })
                .collect();

            // Sort members: new (in_base_image=false) first, then base-image, alphabetical within each
            members.sort_by(|a, b| {
                a.in_base_image
                    .cmp(&b.in_base_image)
                    .then(a.name.cmp(&b.name))
            });

            // Compute added_count
            let added_count = members.iter().filter(|m| !m.in_base_image).count();

            GroupInfo {
                name: group.name.clone(),
                member_count: members.len(),
                added_count,
                locked_count,
                optional_spillover_count: group.optional_installed.len(),
                render_state: state_str,
                degradation_reason,
                members,
            }
        })
        .collect();

    // Build per-package provenance map. For each package in the view,
    // check if it belongs to any group in a non-renderable state.
    let mut package_provenances: HashMap<String, PackageProvenance> = HashMap::new();

    for pkg in &view.packages {
        let pkg_name = &pkg.entry.name;
        let pkg_key = format!("{}.{}", pkg.entry.name, pkg.entry.arch);

        // Check optional_installed membership (any group, any render state)
        if let Some(group) = installed_groups
            .iter()
            .find(|g| g.optional_installed.contains(pkg_name))
        {
            package_provenances.insert(
                pkg_key,
                PackageProvenance {
                    kind: "optional_spillover".to_string(),
                    group_name: group.name.clone(),
                },
            );
            continue;
        }

        // Check ungrouped/degraded membership
        if let Some(group) = installed_groups.iter().find(|g| {
            g.members.contains(pkg_name)
                && matches!(
                    render_ctx.group_states.get(&g.name),
                    Some(GroupRenderState::Ungrouped) | Some(GroupRenderState::Degraded { .. })
                )
        }) {
            let kind = if render_ctx.is_ungrouped(&group.name) {
                "ungrouped_member"
            } else {
                "degraded_member"
            };
            package_provenances.insert(
                pkg_key,
                PackageProvenance {
                    kind: kind.to_string(),
                    group_name: group.name.clone(),
                },
            );
        }
    }

    // -- Language packages (from non_rpm_software items with lang field) ------

    let snap = session.snapshot();

    let language_packages: Vec<LanguagePackageEnvDto> = snap
        .non_rpm_software
        .as_ref()
        .map(|nrs| {
            nrs.items
                .iter()
                .filter(|item| !item.lang.is_empty())
                .map(|item| {
                    let manifest_basis = item
                        .manifest_files
                        .keys()
                        .next()
                        .cloned()
                        .unwrap_or_default();
                    LanguagePackageEnvDto {
                        ecosystem: item.lang.clone(),
                        path: item.path.clone(),
                        method: item.method.clone(),
                        packages: item.packages.iter().map(|p| p.name.clone()).collect(),
                        confidence: item.confidence.clone(),
                        manifest_basis,
                        include: item.include,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    // -- Unmanaged files (grouped by parent directory) -----------------------

    let has_unmanaged_scan = snap.unmanaged_files.is_some();

    let unmanaged_files: Vec<UnmanagedFileGroupDto> = snap
        .unmanaged_files
        .as_ref()
        .map(|ufs| {
            // Group files by parent directory
            let mut groups: HashMap<String, Vec<UnmanagedFileItemDto>> = HashMap::new();
            for file in &ufs.items {
                let dir = Path::new(&file.path)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| "/".to_string());
                groups.entry(dir).or_default().push(UnmanagedFileItemDto {
                    path: file.path.clone(),
                    size: file.size,
                    is_var_path: file.under_var,
                    include: file.include,
                    provenance: ProvenanceSignalsDto {
                        file_type: file_type_str(&file.file_type),
                        last_modified: file.provenance.last_modified,
                        uid: file.provenance.uid,
                        gid: file.provenance.gid,
                        permissions: file.provenance.permissions.clone(),
                        mutability: file.provenance.mutable,
                        writable_mount: file.provenance.writable_mount,
                        service_working_dir: file.provenance.service_working_dir,
                    },
                });
            }
            let mut result: Vec<UnmanagedFileGroupDto> = groups
                .into_iter()
                .map(|(directory, items)| UnmanagedFileGroupDto { directory, items })
                .collect();
            result.sort_by(|a, b| a.directory.cmp(&b.directory));
            result
        })
        .unwrap_or_default();

    ViewResponse {
        view,
        repo_groups,
        baseline_summary: decisions.baseline_summary.clone(),
        version_changes,
        service_states,
        service_dropins,
        quadlets,
        flatpaks,
        sysctls,
        tuned,
        users_groups_decisions: decisions.users_groups.clone(),
        package_groups,
        package_provenances,
        session_is_sensitive: decisions.is_sensitive,
        language_packages,
        unmanaged_files,
        has_unmanaged_scan,
    }
}

// =========================================================================
// Reference section adapters
//
// Each adapter takes typed domain data from ReferenceProjection and
// produces ReferenceSection/ContextItem for the wire.  Presentation logic
// (subtitles, searchable_text, display_name, empty_reason strings) is
// ported from each normalize_* function in handlers.rs.
// =========================================================================

// -- FileType helper -------------------------------------------------------

/// Map a `FileType` enum variant to the snake_case string the frontend expects.
fn file_type_str(ft: &FileType) -> String {
    match ft {
        FileType::ElfBinary => "elf_binary".to_string(),
        FileType::Jar => "jar".to_string(),
        FileType::Script => "script".to_string(),
        FileType::DataFile => "data_file".to_string(),
        FileType::Config => "config".to_string(),
        FileType::Symlink => "symlink".to_string(),
        FileType::Other => "other".to_string(),
    }
}

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
                subtitle: Some(format!("package '{}' not in target image", o.package)),
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
        if e.is_empty() { "0" } else { e }
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
    let mut subsections = Vec::new();

    // Customizations subsection
    let mut custom_items = Vec::new();

    // tuned_active → customizations
    if let Some(tuned) = &data.tuned_active {
        custom_items.push(ContextItem {
            id: "tuned_active".to_string(),
            title: "Active tuned profile".to_string(),
            subtitle: Some(tuned.clone()),
            detail: None,
            searchable_text: tuned.clone(),
        });
    }

    // sysctl_overrides → customizations
    for so in &data.sysctl_overrides {
        custom_items.push(ContextItem {
            id: so.key.clone(),
            title: so.key.clone(),
            subtitle: Some(format!("\"{}\" (default: \"{}\")", so.runtime, so.default)),
            detail: Some(so.source.clone()),
            searchable_text: format!("{} {} {} {}", so.key, so.runtime, so.default, so.source),
        });
    }

    // non_default_modules → customizations
    for km in &data.non_default_modules {
        custom_items.push(ContextItem {
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
    let snippet_items = |snippets: &[inspectah_refine::projection::RefConfigSnippet],
                         label: &str|
     -> Vec<ContextItem> {
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

    custom_items.extend(snippet_items(&data.modules_load_d, "modules-load.d"));
    custom_items.extend(snippet_items(&data.modprobe_d, "modprobe.d"));
    custom_items.extend(snippet_items(&data.dracut_conf, "dracut.conf.d"));
    custom_items.extend(snippet_items(&data.custom_tuned_profiles, "tuned profile"));

    if !custom_items.is_empty() {
        subsections.push(ContextSubsection {
            id: "customizations".to_string(),
            display_name: "Customizations".to_string(),
            items: custom_items,
        });
    }

    // Defaults / Context subsection
    let mut default_items = Vec::new();

    // cmdline → defaults_context
    if let Some(cmdline) = &data.cmdline {
        let subtitle = if cmdline.len() > 80 {
            Some(format!("{}...", &cmdline[..77]))
        } else {
            Some(cmdline.clone())
        };
        default_items.push(ContextItem {
            id: "cmdline".to_string(),
            title: "Kernel cmdline".to_string(),
            subtitle,
            detail: Some(cmdline.clone()),
            searchable_text: cmdline.clone(),
        });
    }

    // grub_defaults → defaults_context
    if let Some(grub) = &data.grub_defaults {
        default_items.push(ContextItem {
            id: "grub_defaults".to_string(),
            title: "GRUB defaults".to_string(),
            subtitle: None,
            detail: Some(grub.clone()),
            searchable_text: grub.clone(),
        });
    }

    // locale → defaults_context
    if let Some(locale) = &data.locale {
        default_items.push(ContextItem {
            id: "locale".to_string(),
            title: "Locale".to_string(),
            subtitle: Some(locale.clone()),
            detail: None,
            searchable_text: locale.clone(),
        });
    }

    // timezone → defaults_context
    if let Some(tz) = &data.timezone {
        default_items.push(ContextItem {
            id: "timezone".to_string(),
            title: "Timezone".to_string(),
            subtitle: Some(tz.clone()),
            detail: None,
            searchable_text: tz.clone(),
        });
    }

    // alternatives → defaults_context
    for ae in &data.alternatives {
        default_items.push(ContextItem {
            id: ae.name.clone(),
            title: ae.name.clone(),
            subtitle: Some(format!("{} ({})", ae.path, ae.status)),
            detail: None,
            searchable_text: format!("{} {} {}", ae.name, ae.path, ae.status),
        });
    }

    if !default_items.is_empty() {
        subsections.push(ContextSubsection {
            id: "defaults_context".to_string(),
            display_name: "Defaults / Context".to_string(),
            items: default_items,
        });
    }

    ReferenceSection {
        id: "kernel_boot".to_string(),
        display_name: "Kernel & Boot".to_string(),
        items: Vec::new(),
        subsections,
        empty_reason: None,
    }
}

// -- Network ---------------------------------------------------------------

pub fn web_network_section(data: &RefNetwork) -> ReferenceSection {
    let mut subsections = Vec::new();

    // 1. Connections subsection
    let mut connections_items = Vec::new();
    for conn in &data.connections {
        connections_items.push(ContextItem {
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
    if !connections_items.is_empty() {
        subsections.push(ContextSubsection {
            id: "connections".to_string(),
            display_name: "Connections".to_string(),
            items: connections_items,
        });
    }

    // 2. Firewall subsection (zones + direct rules)
    let mut firewall_items = Vec::new();
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
        firewall_items.push(ContextItem {
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
    for rule in &data.firewall_direct_rules {
        let id = format!("{}:{}:{}", rule.ipv, rule.chain, rule.priority);
        firewall_items.push(ContextItem {
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
    if !firewall_items.is_empty() {
        subsections.push(ContextSubsection {
            id: "firewall".to_string(),
            display_name: "Firewall".to_string(),
            items: firewall_items,
        });
    }

    // 3. Routes & Rules subsection (static routes + ip routes + ip rules)
    let mut routes_rules_items = Vec::new();
    for sr in &data.static_routes {
        routes_rules_items.push(ContextItem {
            id: sr.path.clone(),
            title: sr.name.clone(),
            subtitle: Some(sr.path.clone()),
            detail: None,
            searchable_text: format!("{} {}", sr.path, sr.name),
        });
    }
    for route in &data.ip_routes {
        routes_rules_items.push(ContextItem {
            id: route.clone(),
            title: route.clone(),
            subtitle: Some("ip route".to_string()),
            detail: None,
            searchable_text: route.clone(),
        });
    }
    for rule in &data.ip_rules {
        routes_rules_items.push(ContextItem {
            id: rule.clone(),
            title: rule.clone(),
            subtitle: Some("ip rule".to_string()),
            detail: None,
            searchable_text: rule.clone(),
        });
    }
    if !routes_rules_items.is_empty() {
        subsections.push(ContextSubsection {
            id: "routes_rules".to_string(),
            display_name: "Routes & Rules".to_string(),
            items: routes_rules_items,
        });
    }

    // 4. DNS & Hosts subsection (resolv_provenance + hosts_additions)
    let mut dns_hosts_items = Vec::new();
    if !data.resolv_provenance.is_empty() {
        dns_hosts_items.push(ContextItem {
            id: "resolv_provenance".to_string(),
            title: "DNS resolver".to_string(),
            subtitle: Some(data.resolv_provenance.clone()),
            detail: None,
            searchable_text: data.resolv_provenance.clone(),
        });
    }
    for line in &data.hosts_additions {
        dns_hosts_items.push(ContextItem {
            id: line.clone(),
            title: line.clone(),
            subtitle: Some("hosts".to_string()),
            detail: None,
            searchable_text: line.clone(),
        });
    }
    if !dns_hosts_items.is_empty() {
        subsections.push(ContextSubsection {
            id: "dns_hosts".to_string(),
            display_name: "DNS & Hosts".to_string(),
            items: dns_hosts_items,
        });
    }

    // 5. Proxy subsection
    let mut proxy_items = Vec::new();
    for proxy in &data.proxy_env {
        let id = format!("{}:{}", proxy.source, proxy.line);
        proxy_items.push(ContextItem {
            id,
            title: proxy.source.clone(),
            subtitle: Some(proxy.line.clone()),
            detail: None,
            searchable_text: format!("{} {}", proxy.source, proxy.line),
        });
    }
    if !proxy_items.is_empty() {
        subsections.push(ContextSubsection {
            id: "proxy".to_string(),
            display_name: "Proxy".to_string(),
            items: proxy_items,
        });
    }

    ReferenceSection {
        id: "network".to_string(),
        display_name: "Network".to_string(),
        items: Vec::new(),
        subsections,
        empty_reason: None,
    }
}

// -- Storage ---------------------------------------------------------------

pub fn web_storage_section(data: &RefStorage) -> ReferenceSection {
    let mut items = Vec::new();

    // FstabEntry
    for entry in &data.fstab_entries {
        let detail = if let Some(reason) = &entry.attention_reason {
            format!("{} [{}]", entry.options, reason)
        } else {
            entry.options.clone()
        };
        items.push(ContextItem {
            id: entry.mount_point.clone(),
            title: entry.mount_point.clone(),
            subtitle: Some(format!("{} ({})", entry.device, entry.fstype)),
            detail: Some(detail),
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
                if let Some(c) = &item.content {
                    search.push(' ');
                    search.push_str(c);
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
        web_generic_section(
            "scheduled_tasks",
            "Scheduled Tasks",
            &ref_proj.scheduled_tasks,
        ),
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

    #[test]
    fn build_web_view_empty_snapshot() {
        let snap = InspectionSnapshot::new();
        let session = RefineSession::new(snap);
        let response = build_web_view(&session);
        let json = serde_json::to_value(&response).expect("serialize");
        assert!(json.get("generation").is_some());
        assert!(json.get("service_states").is_some());
        assert!(json.get("repo_groups").is_some());
        // package_groups present even with no groups
        let groups = json["package_groups"]
            .as_array()
            .expect("package_groups array");
        assert!(groups.is_empty(), "empty snapshot has no groups");
    }

    #[test]
    fn build_web_view_with_rpm() {
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
                locked: false,
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
        let json = serde_json::to_value(build_web_view(&session)).expect("serialize");
        let vc = json["version_changes"].as_array().unwrap();
        assert_eq!(vc.len(), 1);
        assert_eq!(vc[0]["direction"], "upgrade");
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
        assert_eq!(
            actual_ids, expected_ids,
            "section ids must match canonical order"
        );
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

    #[test]
    fn build_web_view_with_groups() {
        use inspectah_core::types::rpm::{InstalledGroup, PackageEntry, PackageState, RpmSection};

        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![
                PackageEntry {
                    name: "gcc".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    include: true,
                    locked: false,
                    source_repo: "appstream".into(),
                    ..Default::default()
                },
                PackageEntry {
                    name: "make".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    include: true,
                    locked: false,
                    source_repo: "appstream".into(),
                    ..Default::default()
                },
            ],
            installed_groups: Some(vec![InstalledGroup {
                name: "Development Tools".into(),
                members: vec!["gcc".into(), "make".into()],
                optional_installed: vec!["valgrind".into()],
            }]),
            ..Default::default()
        });
        let session = RefineSession::new(snap);
        let response = build_web_view(&session);
        let json = serde_json::to_value(&response).expect("serialize");

        // Verify group data is populated correctly
        let groups = json["package_groups"].as_array().expect("package_groups");
        assert_eq!(groups.len(), 1, "one group");
        assert_eq!(groups[0]["name"], "Development Tools");
        assert_eq!(groups[0]["member_count"], 2);
        assert_eq!(groups[0]["optional_spillover_count"], 1);

        // Render state reflects session classification (members excluded by
        // default triage → degraded). The exact state depends on the
        // classifier; we verify it is a valid string.
        let state = groups[0]["render_state"].as_str().unwrap();
        assert!(
            ["renderable", "excluded", "ungrouped", "degraded"].contains(&state),
            "render_state must be one of the known states, got: {}",
            state
        );

        let members = groups[0]["members"].as_array().expect("members");
        assert_eq!(members.len(), 2);
        // Verify member overlap_groups is an array
        assert!(members[0]["overlap_groups"].is_array());
    }

    #[test]
    fn group_info_includes_in_base_image_and_added_count() {
        use inspectah_core::types::rpm::{InstalledGroup, PackageEntry, PackageState, RpmSection};

        // Build snapshot with packages_added: ["httpd", "mod_ssl"]
        // Build InstalledGroup with members: ["httpd", "mod_ssl", "apr", "apr-util"]
        // (apr and apr-util are installed but from base — not in packages_added)
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![
                PackageEntry {
                    name: "httpd".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    include: true,
                    locked: false,
                    source_repo: "appstream".into(),
                    ..Default::default()
                },
                PackageEntry {
                    name: "mod_ssl".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    include: true,
                    locked: false,
                    source_repo: "appstream".into(),
                    ..Default::default()
                },
            ],
            installed_groups: Some(vec![InstalledGroup {
                name: "Web Server".into(),
                members: vec![
                    "httpd".into(),
                    "mod_ssl".into(),
                    "apr".into(),
                    "apr-util".into(),
                ],
                optional_installed: vec![],
            }]),
            ..Default::default()
        });

        let session = RefineSession::new(snap);
        let response = build_web_view(&session);
        let json = serde_json::to_value(&response).expect("serialize");

        let groups = json["package_groups"].as_array().expect("package_groups");
        assert_eq!(groups.len(), 1);

        // Assert: GroupInfo.added_count == 2
        assert_eq!(groups[0]["added_count"], 2, "added_count should be 2");

        // Assert: GroupInfo.member_count == 4
        assert_eq!(groups[0]["member_count"], 4, "member_count should be 4");

        let members = groups[0]["members"].as_array().expect("members");
        assert_eq!(members.len(), 4);

        // Assert: members sorted new first (httpd, mod_ssl), then base (apr, apr-util)
        assert_eq!(members[0]["name"], "httpd");
        assert_eq!(
            members[0]["in_base_image"], false,
            "httpd should not be in_base_image"
        );

        assert_eq!(members[1]["name"], "mod_ssl");
        assert_eq!(
            members[1]["in_base_image"], false,
            "mod_ssl should not be in_base_image"
        );

        assert_eq!(members[2]["name"], "apr");
        assert_eq!(
            members[2]["in_base_image"], true,
            "apr should be in_base_image"
        );

        assert_eq!(members[3]["name"], "apr-util");
        assert_eq!(
            members[3]["in_base_image"], true,
            "apr-util should be in_base_image"
        );
    }

    #[test]
    fn web_network_section_groups_into_subsections() {
        use inspectah_refine::projection::{
            RefFirewallDirectRule, RefFirewallZone, RefNMConnection, RefNetwork, RefProxyEnv,
            RefStaticRoute,
        };

        let data = RefNetwork {
            connections: vec![RefNMConnection {
                name: "eth0".into(),
                conn_type: "ethernet".into(),
                method: "auto".into(),
                path: "/etc/NetworkManager/system-connections/eth0.nmconnection".into(),
            }],
            firewall_zones: vec![RefFirewallZone {
                name: "public".into(),
                path: "/etc/firewalld/zones/public.xml".into(),
                content: "<zone>...</zone>".into(),
                services: vec!["ssh".into()],
                ports: vec!["8080/tcp".into()],
                rich_rules: vec![],
            }],
            firewall_direct_rules: vec![RefFirewallDirectRule {
                ipv: "ipv4".into(),
                table: "filter".into(),
                chain: "INPUT".into(),
                priority: "0".into(),
                args: "-p tcp --dport 443 -j ACCEPT".into(),
            }],
            static_routes: vec![RefStaticRoute {
                path: "/etc/sysconfig/network-scripts/route-eth0".into(),
                name: "route-eth0".into(),
            }],
            ip_routes: vec!["10.0.0.0/8 via 10.0.0.1".into()],
            ip_rules: vec!["from 10.0.0.0/8 lookup 100".into()],
            resolv_provenance: "NetworkManager".into(),
            hosts_additions: vec!["10.0.0.5 db.local".into()],
            proxy_env: vec![RefProxyEnv {
                source: "/etc/environment".into(),
                line: "http_proxy=http://proxy:3128".into(),
            }],
        };
        let section = web_network_section(&data);
        assert!(section.items.is_empty(), "top-level items must be empty");
        assert_eq!(section.subsections.len(), 5);

        assert_eq!(section.subsections[0].id, "connections");
        assert_eq!(section.subsections[0].display_name, "Connections");
        assert_eq!(section.subsections[0].items.len(), 1);
        assert_eq!(section.subsections[0].items[0].title, "eth0");
        assert!(
            section.subsections[0].items[0]
                .subtitle
                .as_deref()
                .unwrap()
                .contains("ethernet")
        );

        assert_eq!(section.subsections[1].id, "firewall");
        assert_eq!(section.subsections[1].items.len(), 2);
        assert!(section.subsections[1].items[0].detail.is_some());

        assert_eq!(section.subsections[2].id, "routes_rules");
        assert_eq!(section.subsections[2].display_name, "Routes & Rules");
        assert_eq!(section.subsections[2].items.len(), 3);
        assert!(
            section.subsections[2]
                .items
                .iter()
                .any(|i| i.subtitle.as_deref() == Some("ip route"))
        );

        assert_eq!(section.subsections[3].id, "dns_hosts");
        assert_eq!(section.subsections[3].items.len(), 2);

        assert_eq!(section.subsections[4].id, "proxy");
        assert_eq!(section.subsections[4].items.len(), 1);
    }

    #[test]
    fn build_web_view_includes_language_packages() {
        use inspectah_core::types::nonrpm::{LanguagePackage, NonRpmItem, NonRpmSoftwareSection};

        let mut snap = InspectionSnapshot::new();
        let mut manifest_files = std::collections::HashMap::new();
        manifest_files.insert("requirements.txt".to_string(), "flask==2.0".to_string());
        snap.non_rpm_software = Some(NonRpmSoftwareSection {
            items: vec![NonRpmItem {
                path: "/opt/app/venv".into(),
                name: "app-venv".into(),
                method: "pip-freeze".into(),
                confidence: "high".into(),
                include: true,
                lang: "pip".into(),
                packages: vec![LanguagePackage {
                    name: "flask".into(),
                    version: "2.0".into(),
                }],
                manifest_files,
                ..Default::default()
            }],
            ..Default::default()
        });
        let session = RefineSession::new(snap);
        let json = serde_json::to_value(build_web_view(&session)).expect("serialize");

        let lang = json["language_packages"].as_array().unwrap();
        assert_eq!(lang.len(), 1);
        assert_eq!(lang[0]["ecosystem"], "pip");
        assert_eq!(lang[0]["path"], "/opt/app/venv");
        assert_eq!(lang[0]["packages"][0], "flask");
        assert_eq!(lang[0]["manifest_basis"], "requirements.txt");
        assert_eq!(lang[0]["include"], true);
        assert_eq!(json["has_unmanaged_scan"], false);
    }

    #[test]
    fn build_web_view_includes_unmanaged_files() {
        use inspectah_core::types::nonrpm::{
            FileType, ProvenanceSignals, UnmanagedFile, UnmanagedFileSection,
        };

        let mut snap = InspectionSnapshot::new();
        snap.unmanaged_files = Some(UnmanagedFileSection {
            items: vec![
                UnmanagedFile {
                    path: "/opt/splunk/bin/splunkd".into(),
                    size: 52428800,
                    file_type: FileType::ElfBinary,
                    provenance: ProvenanceSignals {
                        file_type: FileType::ElfBinary,
                        last_modified: 1700000000,
                        uid: 0,
                        gid: 0,
                        permissions: "0755".into(),
                        mutable: false,
                        writable_mount: false,
                        service_working_dir: false,
                    },
                    include: true,
                    under_var: false,
                    ..Default::default()
                },
                UnmanagedFile {
                    path: "/opt/splunk/etc/config.ini".into(),
                    size: 1024,
                    file_type: FileType::Config,
                    provenance: ProvenanceSignals {
                        file_type: FileType::Config,
                        last_modified: 1700000000,
                        uid: 0,
                        gid: 0,
                        permissions: "0644".into(),
                        mutable: true,
                        writable_mount: false,
                        service_working_dir: false,
                    },
                    include: false,
                    under_var: false,
                    ..Default::default()
                },
            ],
            total_size: 52429824,
            total_count: 2,
        });
        let session = RefineSession::new(snap);
        let json = serde_json::to_value(build_web_view(&session)).expect("serialize");

        assert_eq!(json["has_unmanaged_scan"], true);
        let groups = json["unmanaged_files"].as_array().unwrap();
        // Both files under /opt/splunk — but in different subdirectories
        // /opt/splunk/bin and /opt/splunk/etc
        assert_eq!(groups.len(), 2);

        // Find the bin group
        let bin_group = groups
            .iter()
            .find(|g| g["directory"].as_str() == Some("/opt/splunk/bin"))
            .expect("bin group");
        let bin_items = bin_group["items"].as_array().unwrap();
        assert_eq!(bin_items.len(), 1);
        assert_eq!(bin_items[0]["path"], "/opt/splunk/bin/splunkd");
        assert_eq!(bin_items[0]["provenance"]["file_type"], "elf_binary");
        assert_eq!(bin_items[0]["include"], true);

        // Find the etc group
        let etc_group = groups
            .iter()
            .find(|g| g["directory"].as_str() == Some("/opt/splunk/etc"))
            .expect("etc group");
        let etc_items = etc_group["items"].as_array().unwrap();
        assert_eq!(etc_items.len(), 1);
        assert_eq!(etc_items[0]["include"], false);
        assert_eq!(etc_items[0]["provenance"]["mutability"], true);
    }

    #[test]
    fn web_network_section_omits_empty_subsections() {
        use inspectah_refine::projection::{RefNMConnection, RefNetwork};

        let data = RefNetwork {
            connections: vec![RefNMConnection {
                name: "eth0".into(),
                conn_type: "ethernet".into(),
                method: "auto".into(),
                path: "/path".into(),
            }],
            firewall_zones: vec![],
            firewall_direct_rules: vec![],
            static_routes: vec![],
            ip_routes: vec![],
            ip_rules: vec![],
            resolv_provenance: String::new(),
            hosts_additions: vec![],
            proxy_env: vec![],
        };
        let section = web_network_section(&data);
        assert_eq!(section.subsections.len(), 1);
        assert_eq!(section.subsections[0].id, "connections");
    }

    #[test]
    fn web_kernel_boot_section_splits_customizations_and_defaults() {
        use inspectah_refine::projection::{RefKernelBoot, RefSysctlOverride};

        let data = RefKernelBoot {
            tuned_active: Some("throughput-performance".into()),
            sysctl_overrides: vec![RefSysctlOverride {
                key: "vm.swappiness".into(),
                runtime: "10".into(),
                default: "60".into(),
                source: "/etc/sysctl.d/99-custom.conf".into(),
            }],
            cmdline: Some("BOOT_IMAGE=/vmlinuz-5.14.0 root=/dev/mapper/rhel-root".into()),
            locale: Some("en_US.UTF-8".into()),
            ..Default::default()
        };
        let section = web_kernel_boot_section(&data);

        assert!(section.items.is_empty(), "top-level items must be empty");
        assert_eq!(section.subsections.len(), 2);
        assert_eq!(section.subsections[0].id, "customizations");
        assert_eq!(section.subsections[0].display_name, "Customizations");
        assert!(
            section.subsections[0]
                .items
                .iter()
                .any(|i| i.title == "Active tuned profile")
        );
        assert!(
            section.subsections[0]
                .items
                .iter()
                .any(|i| i.title == "vm.swappiness")
        );

        assert_eq!(section.subsections[1].id, "defaults_context");
        assert_eq!(section.subsections[1].display_name, "Defaults / Context");
        assert!(
            section.subsections[1]
                .items
                .iter()
                .any(|i| i.title == "Kernel cmdline")
        );
        assert!(
            section.subsections[1]
                .items
                .iter()
                .any(|i| i.title == "Locale")
        );
    }

    #[test]
    fn web_kernel_boot_section_omits_empty_customizations() {
        use inspectah_refine::projection::RefKernelBoot;

        let data = RefKernelBoot {
            cmdline: Some("BOOT_IMAGE=...".into()),
            locale: Some("en_US.UTF-8".into()),
            ..Default::default()
        };
        let section = web_kernel_boot_section(&data);
        assert_eq!(section.subsections.len(), 1, "only non-empty subsections");
        assert_eq!(section.subsections[0].id, "defaults_context");
    }
}
