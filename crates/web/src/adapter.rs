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
use inspectah_core::types::network::IFCFG_DEPRECATION_NOTE;
use inspectah_core::types::nonrpm::FileType;
use inspectah_core::types::repo::RepoId;
use inspectah_core::types::rpm::VersionChangeDirection;
use inspectah_core::types::services::{PresetDefault, ServiceUnitState};
use inspectah_core::types::storage::{VarDirBacking, declaring_unit_label};
use inspectah_core::types::{AdvisoryType, FindingKind};
use inspectah_pipeline::render::advisory::config_advisories;
use inspectah_pipeline::render::service_intent::AdvisoryReason;
use inspectah_pipeline::render::var_dirs::EffectiveBacking;
use inspectah_refine::group_state;
use inspectah_refine::projection::{
    GenericRefItem, RefContainers, RefKernelBoot, RefNetwork, RefServices, RefStorage,
    RefVarDirectory, RefVersionChanges, ReferenceProjection, config_ref_summary,
};
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::ItemId;

use crate::web_types::{
    ContextAdvisory, ContextDecision, ContextItem, ContextSubsection, DropInDecisionDto,
    FlatpakDecisionDto, GroupInfo, GroupMemberInfo, LanguagePackageDto, LanguagePackageEnvDto,
    PackageProvenance, ProvenanceSignalsDto, QuadletDecisionDto, ReferenceSection, RepoGroupInfo,
    ServiceDecisionDto, SysctlDecisionDto, TunedDecisionDto, UnmanagedFileGroupDto,
    UnmanagedFileItemDto, VersionChangeEntry, ViewResponse,
};

/// Build a complete [`ViewResponse`] from session state.
///
/// This is a pure mapping layer: it reads the session's view, decisions,
/// and baseline summary, then maps each domain type to its DTO counterpart.
/// The output serializes to JSON identical to the legacy handler path.
/// Restore the advisory disposition on every config entry the collector
/// flagged, overwriting whatever the projection left there.
///
/// The view's config entries are classified from the *projected* snapshot,
/// and `project_snapshot` rewrites a config disposition with
/// `FindingKind::from_bool` for any `SetInclude` in the timeline. A session
/// autosaved before advisory toggles were refused can still replay one, and
/// `resume_from` restores that timeline without per-op validation — so the
/// projection can report an advisory as an ordinary included file.
///
/// The original snapshot is authoritative here: a modernization or
/// cross-tree symlink finding is a fact about the host, not a decision the
/// user owns. Taking both the set of advisory paths and their content from a
/// single `config_advisories` call is what keeps the row half and the
/// content half from disagreeing about what a config advisory is.
fn restore_config_advisories(
    config_files: &mut [inspectah_refine::types::RefinedConfig],
    original: &inspectah_core::snapshot::InspectionSnapshot,
) {
    for advisory in config_advisories(original) {
        if let Some(refined) = config_files
            .iter_mut()
            .find(|c| c.entry.path == advisory.path)
        {
            refined.entry.disposition =
                FindingKind::advisory(advisory.advisory_type.clone(), advisory.rationale);
        }
    }
}

pub fn build_web_view(session: &RefineSession) -> ViewResponse {
    let mut view = session.view().clone();
    restore_config_advisories(&mut view.config_files, session.snapshot());
    let decisions = session.decisions();

    // Map repo groups from projection type to DTO. `RepoGroup` (the
    // projection type `decisions.repo_groups` holds) has no effective-member
    // concept of its own, so the three counts below come from
    // `session.repo_effective` directly -- the same § 3.2 source
    // `handlers::build_repo_groups` reads for the aggregate view, which
    // keeps those three fields from diverging between the two surfaces even
    // though `package_count` itself is computed twice (once here, ported
    // into `decisions::build_repo_groups`, and once in `handlers.rs`).
    let repo_groups: Vec<RepoGroupInfo> = decisions
        .repo_groups
        .iter()
        .map(|rg| {
            let effective = session.repo_effective(&RepoId::new(rg.section_id.as_str()));
            RepoGroupInfo {
                section_id: rg.section_id.clone(),
                provenance: rg.provenance,
                is_distro: rg.is_distro,
                tier: rg.tier,
                package_count: rg.package_count,
                effective_member_count: effective.member_count,
                effective_included: effective.included,
                effective_unlisted: effective.unlisted,
                enabled: rg.enabled,
            }
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

    // Map service decisions from Refined types to DTOs.
    // Shadow fields flow from the service entry itself.
    let service_states: Vec<ServiceDecisionDto> = decisions
        .service_states
        .iter()
        .map(|s| ServiceDecisionDto {
            unit: s.entry.unit.clone(),
            triage: s.triage.clone(),
            include: s.entry.disposition.is_included(),
            locked: s.entry.locked,
            attention_reason: s.entry.attention_reason.clone(),
            owning_package: s.entry.owning_package.clone(),
            default_state: s.entry.default_state.map(|d| d.to_string()),
            current_state: s.entry.current_state.to_string(),
            shadow_type: s
                .entry
                .shadow_type
                .as_ref()
                .and_then(|st| serde_json::to_value(st).ok())
                .and_then(|v| v.as_str().map(|s| s.to_string())),
            shadow_rationale: s.entry.shadow_rationale.clone(),
        })
        .collect();

    let service_dropins: Vec<DropInDecisionDto> = decisions
        .service_dropins
        .iter()
        .map(|d| DropInDecisionDto {
            unit: d.entry.unit.clone(),
            path: d.entry.path.clone(),
            triage: d.triage.clone(),
            include: d.entry.disposition.is_included(),
            locked: d.entry.locked,
            attention_reason: d.entry.attention_reason.clone(),
            shadow_type: d
                .entry
                .shadow_type
                .as_ref()
                .and_then(|st| serde_json::to_value(st).ok())
                .and_then(|v| v.as_str().map(|s| s.to_string())),
            shadow_rationale: d.entry.shadow_rationale.clone(),
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
                include: q.entry.disposition.is_included(),
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
            include: f.entry.disposition.is_included(),
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
            include: s.entry.disposition.is_included(),
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
                GroupRenderState::Inert => ("inert".to_string(), None),
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

            // The effective set this group's op acts on: present, actionable,
            // not locked (§ 3.7) — the same definition the no-op refusal and
            // group render-state evaluation use.
            let effective = group_state::effective_members(projected_pkgs, &group.members);
            let effective_member_count = effective.len();
            let effective_included = effective
                .iter()
                .filter(|p| p.disposition.is_included())
                .count();

            // A member is in the base image if it's recorded as baseline-
            // suppressed or base-image-only, not merely absent from
            // `packages_added` (position 5). `baseline_suppressed` entries
            // are canonical `name.arch` ids; split on the LAST dot, since
            // package names can contain dots themselves (`java-1.8.0-openjdk`,
            // `python3.11`) while arch values never do.
            let baseline_suppressed_names: std::collections::HashSet<&str> = projected
                .rpm
                .as_ref()
                .and_then(|r| r.baseline_suppressed.as_ref())
                .map(|v| {
                    v.iter()
                        .map(|id| {
                            id.rsplit_once('.')
                                .map_or(id.as_str(), |(name, _arch)| name)
                        })
                        .collect()
                })
                .unwrap_or_default();
            let base_image_only_names: std::collections::HashSet<&str> = projected
                .rpm
                .as_ref()
                .map(|r| r.base_image_only.iter().map(|p| p.name.as_str()).collect())
                .unwrap_or_default();

            // Build member list with overlap detection
            let mut members: Vec<GroupMemberInfo> = group
                .members
                .iter()
                .map(|member_name| {
                    let locked = projected_pkgs
                        .iter()
                        .any(|p| p.name == *member_name && p.locked);
                    let in_base_image = baseline_suppressed_names.contains(member_name.as_str())
                        || base_image_only_names.contains(member_name.as_str());
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
                effective_member_count,
                effective_included,
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
    let projected = session.snapshot_projected();

    let language_packages: Vec<LanguagePackageEnvDto> = projected
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
                        packages: item
                            .packages
                            .iter()
                            .map(|p| LanguagePackageDto {
                                name: p.name.clone(),
                                detected_version: p.version.clone(),
                                pinned: p.pinned,
                            })
                            .collect(),
                        confidence: item.confidence.clone(),
                        manifest_basis,
                        include: item.disposition.is_included(),
                        has_c_extensions: item.has_c_extensions,
                        system_site_packages: item.system_site_packages,
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
                    include: file.disposition.is_included(),
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

// -- Decision helper -------------------------------------------------------

/// The decision a reference row carries, or `None` when the row is not
/// where the decision is made.
///
/// Only an `Actionable` finding is decided on its row. `Advisory` and
/// `Inventory` findings are non-toggleable (§ 3.5), and a row that
/// rendered a decision for one would offer a control the server refuses.
fn context_decision(
    item_id: ItemId,
    disposition: &FindingKind,
    locked: bool,
) -> Option<ContextDecision> {
    match disposition {
        FindingKind::Actionable { include } => Some(ContextDecision {
            item_id,
            include: *include,
            locked,
        }),
        FindingKind::Advisory { .. } | FindingKind::Inventory => None,
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
        });
    }

    // -- Subsections --

    // Omitted services (owning package excluded, or proven absent)
    if !data.omitted.is_empty() {
        let omission_items: Vec<ContextItem> = data
            .omitted
            .iter()
            .map(|o| ContextItem {
                id: format!("omitted-{}", o.unit),
                title: o.unit.clone(),
                subtitle: Some(o.reason.describe(&o.package)),
                detail: None,
                searchable_text: format!("{} omitted {}", o.unit, o.package),
                ..Default::default()
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
                    ..Default::default()
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
                ..Default::default()
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
        has_ifcfg: false,
        ifcfg_note: None,
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
        let reason = data.empty_reason.as_ref().map(|r| {
            match r {
                EmptyReason::DataUnavailable => EMPTY_DATA_UNAVAILABLE,
                EmptyReason::ZeroDrift => EMPTY_ZERO_DRIFT,
            }
            .to_string()
        });
        return ReferenceSection {
            id: "version_changes".to_string(),
            display_name: "Version Changes".to_string(),
            items: Vec::new(),
            subsections: Vec::new(),
            empty_reason: reason,
            has_ifcfg: false,
            ifcfg_note: None,
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
            ..Default::default()
        });
    }

    ReferenceSection {
        id: "version_changes".to_string(),
        display_name: "Version Changes".to_string(),
        items,
        subsections: Vec::new(),
        empty_reason: None,
        has_ifcfg: false,
        ifcfg_note: None,
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            decision: context_decision(
                ItemId::TunedSelection {
                    profile: tuned.clone(),
                },
                &data.tuned_disposition,
                false,
            ),
            ..Default::default()
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
            decision: context_decision(
                ItemId::Sysctl {
                    key: so.key.clone(),
                },
                &so.disposition,
                so.locked,
            ),
            ..Default::default()
        });
    }

    // The kernel and boot config files live in `config.files`. These
    // rows point at the Configuration Files row that decides each one:
    // the title is the whole path, because two profiles named
    // `tuned.conf` read alike as basenames, and the content is not
    // repeated here because the row that can be decided holds it.
    for entry in &data.customizations_from_config {
        custom_items.push(ContextItem {
            id: entry.path.clone(),
            title: entry.path.clone(),
            subtitle: Some(config_ref_summary(entry)),
            detail: None,
            searchable_text: entry.path.clone(),
            // A shadowed entry is `Inventory` (§ 3.5): its content
            // reaches the image through the keys that absorbed it, so
            // the row explains where the decision went and offers none.
            decision: context_decision(
                ItemId::Config {
                    path: entry.path.clone(),
                },
                &entry.disposition,
                entry.locked,
            ),
            ..Default::default()
        });
    }

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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
        has_ifcfg: false,
        ifcfg_note: None,
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
        });
    }
    for route in &data.ip_routes {
        routes_rules_items.push(ContextItem {
            id: route.clone(),
            title: route.clone(),
            subtitle: Some("ip route".to_string()),
            detail: None,
            searchable_text: route.clone(),
            ..Default::default()
        });
    }
    for rule in &data.ip_rules {
        routes_rules_items.push(ContextItem {
            id: rule.clone(),
            title: rule.clone(),
            subtitle: Some("ip rule".to_string()),
            detail: None,
            searchable_text: rule.clone(),
            ..Default::default()
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
            ..Default::default()
        });
    }
    for line in &data.hosts_additions {
        dns_hosts_items.push(ContextItem {
            id: line.clone(),
            title: line.clone(),
            subtitle: Some("hosts".to_string()),
            detail: None,
            searchable_text: line.clone(),
            ..Default::default()
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
            ..Default::default()
        });
    }
    if !proxy_items.is_empty() {
        subsections.push(ContextSubsection {
            id: "proxy".to_string(),
            display_name: "Proxy".to_string(),
            items: proxy_items,
        });
    }

    let ifcfg_note = if data.has_ifcfg {
        Some(IFCFG_DEPRECATION_NOTE.to_string())
    } else {
        None
    };

    ReferenceSection {
        id: "network".to_string(),
        display_name: "Network".to_string(),
        items: Vec::new(),
        subsections,
        empty_reason: None,
        has_ifcfg: data.has_ifcfg,
        ifcfg_note,
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
        });
    }

    items.extend(var_directory_rows(&data.var_directories));

    // CredentialRef
    for cr in &data.credential_refs {
        items.push(ContextItem {
            id: cr.credential_path.clone(),
            title: cr.credential_path.clone(),
            subtitle: Some(format!("mount: {}", cr.mount_point)),
            detail: Some(cr.source.clone()),
            searchable_text: format!("{} {} {}", cr.credential_path, cr.mount_point, cr.source),
            ..Default::default()
        });
    }

    crate::web_types::reference_section("storage", "Storage", items)
}

/// Id of the collapsed `/var` summary row (§ 6.6).
pub const VAR_SUMMARY_ID: &str = "var_directories";

/// Ownership a carried directory has to depart from to earn a row of
/// its own (§ 6.6); anything else collapses into the summary.
const DEFAULT_VAR_DIR_MODE: &str = "0755";
const DEFAULT_VAR_DIR_OWNER: &str = "root";

/// Rationale on a `Provision` directory nothing on the host declares.
/// `Provision { lost: Some(_) }` names the creator it lost instead.
///
/// The directory exists on the host, so something made it. What is
/// absent is a declarative creator, and saying "no creator" instead
/// reads to an admin as a tool error.
const NO_DECLARED_CREATOR_RATIONALE: &str = "nothing on the host declares this directory: no tmpfiles.d entry, unit directive, \
     or package owns it. This export provisions it.";

/// State text on a `Provision` row whose ownership the scan could not
/// read. The export cannot write a tmpfiles.d line without a mode, so
/// it falls back to `RUN mkdir -p`, which lands the directory in the
/// image rather than re-asserting it on every boot.
const MKDIR_FALLBACK_STATE: &str =
    "ownership not recorded: created in the image, not by tmpfiles.d";

/// State text on an `Unresolved` row. The advisory badge reads
/// "Unbacked /var directory", which is a stronger claim than this
/// outcome supports, so the row says what is actually known.
const UNCONFIRMED_STATE: &str = "backing unconfirmed";

/// What the summary's third column says when the scan recorded no
/// mode, owner, or group. Not tmpfiles.d's `-`, which means "leave the
/// default" in the Containerfile the same operator is reading.
const OWNERSHIP_NOT_RECORDED: &str = "ownership not recorded";

/// The `/var` rows of the Storage section (§ 6.6): one collapsed
/// summary over every directory, one advisory row per directory the
/// export must provision or cannot place, and an inventory row for a
/// carried directory whose ownership or mode departs from the default.
///
/// The rows follow `EffectiveBacking`, not the collected verdict: a
/// creator the export drops is not a backing, however the source host
/// made the directory (§ 3.4).
fn var_directory_rows(dirs: &[RefVarDirectory]) -> Vec<ContextItem> {
    if dirs.is_empty() {
        return Vec::new();
    }

    let mut advisory_rows = Vec::new();
    let mut inventory_rows = Vec::new();
    let mut attention_lines = Vec::new();
    let mut carried_lines = Vec::new();
    let mut provisioned = 0usize;
    let mut unconfirmed = 0usize;

    for vd in dirs {
        let line = summary_line(vd);
        match &vd.outcome {
            EffectiveBacking::Carried => {
                carried_lines.push(line);
                if departs_from_default_ownership(vd) {
                    inventory_rows.push(ContextItem {
                        id: vd.path.clone(),
                        title: vd.path.clone(),
                        subtitle: None,
                        detail: ownership_detail(vd),
                        searchable_text: match ownership_detail(vd) {
                            Some(d) => format!("{} {}", vd.path, d),
                            None => vd.path.clone(),
                        },
                        ..Default::default()
                    });
                }
            }
            EffectiveBacking::Provision { lost } => {
                attention_lines.push(line);
                provisioned += 1;
                let rationale = match lost {
                    Some(creator) => format!(
                        "created on the host by {creator}, which this export does not carry"
                    ),
                    None => NO_DECLARED_CREATOR_RATIONALE.to_string(),
                };
                // No mode means no tmpfiles.d line: the export falls
                // back to `RUN mkdir -p`, which is image content rather
                // than a per-boot creator, and the row has to say so.
                let state = ownership_detail(vd)
                    .is_none()
                    .then(|| MKDIR_FALLBACK_STATE.to_string());
                advisory_rows.push(unbacked_var_row(vd, rationale, state));
            }
            EffectiveBacking::Unresolved { creator, reason } => {
                attention_lines.push(line);
                unconfirmed += 1;
                let rationale = format!(
                    "created on the host by {creator}; this export cannot tell whether the \
                     base image carries it ({})",
                    reason.reason_text()
                );
                advisory_rows.push(unbacked_var_row(
                    vd,
                    rationale,
                    Some(UNCONFIRMED_STATE.to_string()),
                ));
            }
        }
    }

    // `Provision` and `Unresolved` are counted apart. One says the
    // export makes the directory; the other says the export does not
    // know whether anything will. Calling both "unbacked" asserts of
    // the second exactly what its own rationale says is unknown.
    let state_text = match (provisioned, unconfirmed) {
        (0, 0) => "all backed by tmpfiles.d, unit directives, or package ownership".to_string(),
        (u, 0) => format!("{u} unbacked"),
        (0, n) => format!("{n} unconfirmed"),
        (u, n) => format!("{u} unbacked, {n} unconfirmed"),
    };
    // The directories needing attention list first: they are what the
    // reader opened the summary for (§ 6.6).
    attention_lines.extend(carried_lines);

    let mut rows = vec![ContextItem {
        id: VAR_SUMMARY_ID.to_string(),
        title: format!("{} /var directories", dirs.len()),
        subtitle: Some(state_text),
        detail: Some(attention_lines.join("\n")),
        searchable_text: format!("/var directories {}", attention_lines.join(" ")),
        ..Default::default()
    }];
    rows.append(&mut advisory_rows);
    rows.append(&mut inventory_rows);
    rows
}

/// One line of the summary's directory list:
/// `<path>  <mode> <owner>:<group>  <creator>  <outcome>` (§ 6.6).
///
/// The fourth column is the point. § 6.6 wrote this list for backed
/// directories only, and once the unbacked ones join it the creator
/// column alone reads as the explanation of the summary's count, which
/// it is not: the count comes from the outcome and the creator is a
/// fact about the host.
fn summary_line(vd: &RefVarDirectory) -> String {
    let ownership = ownership_detail(vd).unwrap_or_else(|| OWNERSHIP_NOT_RECORDED.to_string());
    format!(
        "{}  {}  {}  {}",
        vd.path,
        ownership,
        host_creator(vd),
        export_outcome(vd)
    )
}

/// What made the directory on the source host, named so the operator
/// can act on it: the answer to "which fragment do I keep including".
fn host_creator(vd: &RefVarDirectory) -> String {
    match &vd.source {
        Some(VarDirBacking::Tmpfiles { conf, .. }) => format!("tmpfiles.d ({conf})"),
        Some(VarDirBacking::StateDirectory { declared_by }) => {
            format!("StateDirectory= ({})", declaring_unit_label(declared_by))
        }
        Some(VarDirBacking::CacheDirectory { declared_by }) => {
            format!("CacheDirectory= ({})", declaring_unit_label(declared_by))
        }
        Some(VarDirBacking::LogsDirectory { declared_by }) => {
            format!("LogsDirectory= ({})", declaring_unit_label(declared_by))
        }
        Some(VarDirBacking::RpmOwned { package }) => format!("package-owned ({package})"),
        // Not "unbacked": that word names an outcome, and this column
        // names a mechanism. The two do not denote the same set.
        Some(VarDirBacking::Unbacked) | None => "nothing declares it".to_string(),
    }
}

/// What the export does about that creator (§ 3.4), which is where the
/// summary's count comes from.
///
/// Each value leads with the word the summary counts it under, so the
/// reader can match a row to the count above it: `backed`, `unbacked`,
/// or `unconfirmed`. Leading with the consequence instead left the
/// `Provision` row reading reassuringly ("provisioned by this export")
/// under a count that reads as a problem ("1 unbacked"), with no shared
/// token to connect them.
fn export_outcome(vd: &RefVarDirectory) -> &'static str {
    match &vd.outcome {
        EffectiveBacking::Carried => "backed: creator carried by this export",
        EffectiveBacking::Provision { .. } => "unbacked: provisioned by this export",
        EffectiveBacking::Unresolved { .. } => "backing unconfirmed",
    }
}

/// `<mode> <owner>:<group>`, or `None` when the collector recorded none
/// of the three.
fn ownership_detail(vd: &RefVarDirectory) -> Option<String> {
    match (&vd.mode, &vd.owner_name, &vd.group_name) {
        (Some(mode), Some(owner), Some(group)) => Some(format!("{mode} {owner}:{group}")),
        _ => None,
    }
}

/// Whether a carried directory's ownership or mode is worth a row of
/// its own: the image will not reproduce it from the default (§ 6.6).
fn departs_from_default_ownership(vd: &RefVarDirectory) -> bool {
    let non_default_mode = vd
        .mode
        .as_deref()
        .is_some_and(|m| m != DEFAULT_VAR_DIR_MODE);
    let non_root_owner = vd
        .owner_name
        .as_deref()
        .is_some_and(|o| o != DEFAULT_VAR_DIR_OWNER)
        || vd
            .group_name
            .as_deref()
            .is_some_and(|g| g != DEFAULT_VAR_DIR_OWNER);
    non_default_mode || non_root_owner
}

/// An advisory row for a directory the export cannot report as backed.
fn unbacked_var_row(vd: &RefVarDirectory, rationale: String, state: Option<String>) -> ContextItem {
    ContextItem {
        id: vd.path.clone(),
        title: vd.path.clone(),
        subtitle: state,
        detail: ownership_detail(vd),
        searchable_text: format!("{} {}", vd.path, rationale),
        advisory: Some(ContextAdvisory {
            advisory_type: AdvisoryType::UnbackedVarDir,
            rationale,
        }),
        ..Default::default()
    }
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
            ..Default::default()
        })
        .collect();
    crate::web_types::reference_section(id, display_name, context_items)
}

// -- Orchestrator ----------------------------------------------------------

/// Build all 8 reference sections in canonical order.
///
/// Order MUST match `normalize_for_reference` in handlers.rs:
/// services, containers, network, storage,
/// scheduled_tasks, non_rpm_software, kernel_boot, selinux.
pub fn build_web_sections(ref_proj: &ReferenceProjection) -> Vec<ReferenceSection> {
    let collected = &ref_proj.collected;
    vec![
        with_empty_reason(web_services_section(&ref_proj.services), collected.services),
        with_empty_reason(
            web_containers_section(&ref_proj.containers),
            collected.containers,
        ),
        with_empty_reason(web_network_section(&ref_proj.network), collected.network),
        with_empty_reason(web_storage_section(&ref_proj.storage), collected.storage),
        with_empty_reason(
            web_generic_section(
                "scheduled_tasks",
                "Scheduled Tasks",
                &ref_proj.scheduled_tasks,
            ),
            collected.scheduled_tasks,
        ),
        with_empty_reason(
            web_generic_section(
                "non_rpm_software",
                "Non-RPM Software",
                &ref_proj.non_rpm_software,
            ),
            collected.non_rpm_software,
        ),
        with_empty_reason(
            web_kernel_boot_section(&ref_proj.kernel_boot),
            collected.kernel_boot,
        ),
        with_empty_reason(
            web_generic_section("selinux", "Security & Access Control", &ref_proj.selinux),
            collected.selinux,
        ),
    ]
}

/// An empty section says which of the two emptinesses it is (§ 3.8).
///
/// The distinction is not available from the projection: a section the
/// inspector never collected and one it collected and found nothing in
/// both project to no rows, and only the snapshot separates them.
fn with_empty_reason(mut section: ReferenceSection, collected: bool) -> ReferenceSection {
    if section.items.is_empty() && section.subsections.is_empty() {
        section.empty_reason = Some(
            if collected {
                EMPTY_ZERO_DRIFT
            } else {
                EMPTY_DATA_UNAVAILABLE
            }
            .to_string(),
        );
    }
    section
}

/// The inspector collected the section and it projects to no rows.
const EMPTY_ZERO_DRIFT: &str = "zero_drift";
/// The inspector collected no section at all.
const EMPTY_DATA_UNAVAILABLE: &str = "data_unavailable";

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::snapshot::InspectionSnapshot;
    use inspectah_core::types::FindingKind;

    mod var_directory_rows {
        use super::*;
        use inspectah_refine::projection::RefVarDirectory;

        fn storage_with(dirs: Vec<RefVarDirectory>) -> RefStorage {
            RefStorage {
                fstab_entries: Vec::new(),
                mount_points: Vec::new(),
                lvm_volumes: Vec::new(),
                var_directories: dirs,
                credential_refs: Vec::new(),
                unbacked_var_paths: Vec::new(),
            }
        }

        fn carried(path: &str, mode: Option<&str>, owner: Option<&str>) -> RefVarDirectory {
            RefVarDirectory {
                path: path.into(),
                mode: mode.map(Into::into),
                owner_name: owner.map(Into::into),
                group_name: owner.map(Into::into),
                source: Some(VarDirBacking::RpmOwned {
                    package: "appone".into(),
                }),
                outcome: EffectiveBacking::Carried,
            }
        }

        /// A carried directory whose ownership departs from the default
        /// keeps a row of its own, carrying ownership and mode and
        /// nothing else. The row used to lead with a size estimate and
        /// a recommendation that the collector never filled, so the
        /// subtitle read `~` and the detail read ` (0755 root:root)`.
        #[test]
        fn a_non_default_directory_shows_only_ownership() {
            let section = web_storage_section(&storage_with(vec![carried(
                "/var/lib/appone/cache",
                Some("0750"),
                Some("appone"),
            )]));
            let item = section
                .items
                .iter()
                .find(|i| i.id == "/var/lib/appone/cache")
                .expect("the directory should produce a row");
            assert_eq!(item.subtitle, None);
            assert_eq!(item.detail.as_deref(), Some("0750 appone:appone"));
            assert_eq!(
                item.searchable_text,
                "/var/lib/appone/cache 0750 appone:appone"
            );
        }

        /// A carried directory the image reproduces from the default
        /// has nothing to say on its own and collapses into the summary
        /// (§ 6.6). The row that used to exist for it is gone.
        #[test]
        fn a_default_directory_collapses_into_the_summary() {
            let section = web_storage_section(&storage_with(vec![
                carried("/var/lib/missing", None, None),
                carried("/var/log/plain", Some("0755"), Some("root")),
            ]));
            assert!(
                !section
                    .items
                    .iter()
                    .any(|i| i.id == "/var/lib/missing" || i.id == "/var/log/plain"),
                "neither directory earns a row: {:?}",
                section.items.iter().map(|i| &i.id).collect::<Vec<_>>()
            );
            let summary = section
                .items
                .iter()
                .find(|i| i.id == VAR_SUMMARY_ID)
                .expect("the summary row");
            assert_eq!(summary.title, "2 /var directories");
            assert_eq!(
                summary.detail.as_deref(),
                Some(
                    "/var/lib/missing  ownership not recorded  \
                     package-owned (appone)  backed: creator carried by this export\n\
                     /var/log/plain  0755 root:root  package-owned (appone)  \
                     backed: creator carried by this export"
                )
            );
        }
    }

    mod config_advisory {
        use super::*;
        use inspectah_core::types::AdvisoryType;
        use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
        use inspectah_refine::types::{
            RefinedConfig, Triage, TriageBucket, TriageReason, TriageTag,
        };

        const ADVISORY_PATH: &str = "/etc/init.d/legacy-app";
        const RATIONALE: &str = "sysvinit script — port to a systemd unit";
        const SYMLINK_PATH: &str = "/etc/alternatives/java";
        const PLAIN_PATH: &str = "/etc/httpd/conf/httpd.conf";

        fn snapshot_with_advisories() -> InspectionSnapshot {
            let entry = |path: &str, disposition: FindingKind| ConfigFileEntry {
                path: path.into(),
                disposition,
                ..Default::default()
            };
            let mut snap = InspectionSnapshot::new();
            snap.config = Some(ConfigSection {
                files: vec![
                    entry(
                        ADVISORY_PATH,
                        FindingKind::advisory(AdvisoryType::Modernization, RATIONALE),
                    ),
                    entry(
                        SYMLINK_PATH,
                        FindingKind::advisory(
                            AdvisoryType::CrossTreeSymlink,
                            "symlink crosses into read-only /usr",
                        ),
                    ),
                    entry(PLAIN_PATH, FindingKind::included()),
                ],
            });
            snap
        }

        /// The wire contract has to carry the advisory type and rationale, not
        /// just the fact that the entry is not included — an advisory rendered
        /// as an excluded config file states the opposite of what it means.
        #[test]
        fn view_carries_advisory_type_and_rationale() {
            let session = RefineSession::new(snapshot_with_advisories());
            let json = serde_json::to_value(build_web_view(&session)).expect("serialize");
            let files = json["config_files"].as_array().expect("config_files array");

            let advisory = files
                .iter()
                .find(|f| f["entry"]["path"] == ADVISORY_PATH)
                .expect("the advisory entry reaches the client");
            assert_eq!(advisory["entry"]["disposition"]["kind"], "advisory");
            assert_eq!(
                advisory["entry"]["disposition"]["advisory_type"],
                "modernization"
            );
            assert_eq!(advisory["entry"]["disposition"]["rationale"], RATIONALE);

            let symlink = files
                .iter()
                .find(|f| f["entry"]["path"] == SYMLINK_PATH)
                .expect("every advisory variant reaches the client");
            assert_eq!(
                symlink["entry"]["disposition"]["advisory_type"],
                "cross_tree_symlink"
            );

            let plain = files
                .iter()
                .find(|f| f["entry"]["path"] == PLAIN_PATH)
                .expect("actionable entries are unaffected");
            assert_eq!(plain["entry"]["disposition"]["kind"], "actionable");
            assert_eq!(plain["entry"]["disposition"]["include"], true);
        }

        #[test]
        fn every_advisory_path_appears_exactly_once() {
            let session = RefineSession::new(snapshot_with_advisories());
            let json = serde_json::to_value(build_web_view(&session)).expect("serialize");
            let files = json["config_files"].as_array().expect("config_files array");
            for path in [ADVISORY_PATH, SYMLINK_PATH] {
                let count = files.iter().filter(|f| f["entry"]["path"] == path).count();
                assert_eq!(count, 1, "{path} must render once, not as two rows");
            }
        }

        /// A session autosaved before the advisory guard shipped can replay a
        /// `SetInclude` on an advisory path — `resume_from` restores the
        /// timeline without per-op validation, and `project_snapshot` then
        /// overwrites the disposition with `FindingKind::from_bool`. The
        /// collector's finding is a fact about the host, not a decision the
        /// user is entitled to overwrite, so the original snapshot wins.
        #[test]
        fn stale_projection_does_not_downgrade_an_advisory() {
            let original = snapshot_with_advisories();
            let mut projected: Vec<RefinedConfig> = original
                .config
                .as_ref()
                .unwrap()
                .files
                .iter()
                .map(|entry| RefinedConfig {
                    entry: entry.clone(),
                    triage: TriageTag {
                        triage: Triage::SingleHost(TriageBucket::Investigate),
                        primary_reason: TriageReason::ConfigUnowned,
                        annotations: Vec::new(),
                    },
                })
                .collect();
            // What a replayed stale SetInclude leaves behind.
            projected[0].entry.disposition = FindingKind::included();

            restore_config_advisories(&mut projected, &original);

            assert_eq!(
                projected[0].entry.disposition,
                FindingKind::advisory(AdvisoryType::Modernization, RATIONALE),
                "a stale op must not turn an advisory into an included file"
            );
            assert_eq!(
                projected[2].entry.disposition,
                FindingKind::included(),
                "actionable entries keep whatever the projection decided"
            );
        }
    }

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
                disposition: FindingKind::included(),
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

    /// Verify `build_web_sections` returns 8 sections with correct ids in
    /// canonical order (matching `normalize_for_reference`).
    #[test]
    fn build_web_sections_returns_8_sections_in_order() {
        use inspectah_refine::projection::project_reference;

        let snap = InspectionSnapshot::new();
        let ref_proj = project_reference(&snap);
        let sections = build_web_sections(&ref_proj);

        assert_eq!(sections.len(), 8, "must return exactly 8 sections");

        let expected_ids = [
            "services",
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
                    disposition: FindingKind::included(),
                    locked: false,
                    source_repo: "appstream".into(),
                    ..Default::default()
                },
                PackageEntry {
                    name: "make".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    disposition: FindingKind::included(),
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
                    disposition: FindingKind::included(),
                    locked: false,
                    source_repo: "appstream".into(),
                    ..Default::default()
                },
                PackageEntry {
                    name: "mod_ssl".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    disposition: FindingKind::included(),
                    locked: false,
                    source_repo: "appstream".into(),
                    ..Default::default()
                },
            ],
            // apr/apr-util are declared group members that ship with the base
            // image rather than being added by this host: `in_base_image` is
            // computed from this membership, not from absence in
            // `packages_added` (§ 3.7 position 5).
            baseline_suppressed: Some(vec!["apr.x86_64".into(), "apr-util.x86_64".into()]),
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

    /// Builds the § 3.7 host example: "Container Management" declares three
    /// members. `buildah` and `containernetworking-plugins` are present and
    /// non-leaf (empty baseline + empty `leaf_packages` normalizes both to
    /// excluded at construction); `podman` is declared but baseline-suppressed
    /// and absent from `packages_added`.
    fn group_info_for_host_fixture(name: &str) -> GroupInfo {
        use inspectah_core::baseline::BaselineData;
        use inspectah_core::types::rpm::{InstalledGroup, PackageEntry, PackageState, RpmSection};

        let mut snap = InspectionSnapshot::new();
        snap.baseline = Some(BaselineData {
            image_digest: "sha256:test".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });
        snap.rpm = Some(RpmSection {
            packages_added: vec![
                PackageEntry {
                    name: "buildah".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    disposition: FindingKind::included(),
                    locked: false,
                    source_repo: "appstream".into(),
                    ..Default::default()
                },
                PackageEntry {
                    name: "containernetworking-plugins".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    disposition: FindingKind::included(),
                    locked: false,
                    source_repo: "appstream".into(),
                    ..Default::default()
                },
            ],
            leaf_packages: Some(vec![]),
            baseline_suppressed: Some(vec!["podman.x86_64".into()]),
            installed_groups: Some(vec![InstalledGroup {
                name: name.to_string(),
                members: vec![
                    "buildah".into(),
                    "containernetworking-plugins".into(),
                    "podman".into(),
                ],
                optional_installed: vec![],
            }]),
            ..Default::default()
        });

        let session = RefineSession::new(snap);
        let response = build_web_view(&session);
        response
            .package_groups
            .into_iter()
            .find(|g| g.name == name)
            .expect("group present in view")
    }

    /// `effective_member_count`/`effective_included` exclude `podman` (absent
    /// from `packages_added`); `member_count` still counts all three declared
    /// members, and `podman` reports `in_base_image` via baseline-suppressed
    /// membership rather than absence (§ 3.7).
    #[test]
    fn group_info_reports_declared_and_effective_counts() {
        let gi = group_info_for_host_fixture("Container Management");
        assert_eq!(
            (
                gi.member_count,
                gi.effective_member_count,
                gi.effective_included
            ),
            (3, 2, 0)
        );
        assert!(
            gi.members
                .iter()
                .find(|m| m.name == "podman")
                .unwrap()
                .in_base_image
        );
    }

    /// Discriminates position 5's rule change from the old absence-based
    /// one, and covers finding 1 (dotted canonical ids) in the same fixture.
    ///
    /// `java-1.8.0-openjdk` is absent from `packages_added` but genuinely
    /// baseline-suppressed, recorded as the canonical id
    /// `java-1.8.0-openjdk.x86_64` — a package name containing dots. Parsing
    /// that id by splitting on the FIRST dot (the pre-fix bug) yields
    /// `"java-1"`, which never matches the member name, so `in_base_image`
    /// would wrongly read `false`. Splitting on the LAST dot (arch never
    /// contains one) yields the correct name.
    ///
    /// `orphan-base-pkg` is also absent from `packages_added`, but recorded
    /// in neither `baseline_suppressed` nor `base_image_only`. Under the old
    /// absence-based rule this member would read `in_base_image: true`
    /// (nothing in `packages_added` named it) and not count toward
    /// `added_count`; under § 3.7 position 5 it reads `false` instead, since
    /// no record attests it's actually a base-image package, and it does
    /// count.
    #[test]
    fn in_base_image_reflects_membership_not_absence() {
        use inspectah_core::types::rpm::{InstalledGroup, PackageEntry, PackageState, RpmSection};

        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![PackageEntry {
                name: "gcc".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                disposition: FindingKind::included(),
                locked: false,
                source_repo: "appstream".into(),
                ..Default::default()
            }],
            baseline_suppressed: Some(vec!["java-1.8.0-openjdk.x86_64".into()]),
            installed_groups: Some(vec![InstalledGroup {
                name: "Dev Tools".into(),
                members: vec![
                    "gcc".into(),
                    "java-1.8.0-openjdk".into(),
                    "orphan-base-pkg".into(),
                ],
                optional_installed: vec![],
            }]),
            ..Default::default()
        });

        let session = RefineSession::new(snap);
        let response = build_web_view(&session);
        let gi = response
            .package_groups
            .into_iter()
            .find(|g| g.name == "Dev Tools")
            .expect("group present in view");

        let member = |name: &str| gi.members.iter().find(|m| m.name == name).unwrap();
        assert!(
            member("java-1.8.0-openjdk").in_base_image,
            "a dotted canonical id must be parsed by its last dot (arch), not its first"
        );
        assert!(
            !member("orphan-base-pkg").in_base_image,
            "absence from packages_added alone must not imply in_base_image \
             without a baseline_suppressed or base_image_only record"
        );
        // added_count = members where !in_base_image: gcc (present, not
        // base-image) and orphan-base-pkg (absent but unrecorded, so it
        // counts as added under the new rule); java-1.8.0-openjdk is
        // base-image and does not count.
        assert_eq!(gi.added_count, 2);
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
            has_ifcfg: false,
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
                disposition: FindingKind::included(),
                lang: "pip".into(),
                packages: vec![LanguagePackage {
                    name: "flask".into(),
                    version: "2.0".into(),
                    pinned: false,
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
        assert_eq!(lang[0]["packages"][0]["name"], "flask");
        assert_eq!(lang[0]["packages"][0]["detected_version"], "2.0");
        assert_eq!(lang[0]["packages"][0]["pinned"], false);
        assert_eq!(lang[0]["manifest_basis"], "requirements.txt");
        assert_eq!(lang[0]["include"], true);
        assert_eq!(lang[0]["has_c_extensions"], false);
        assert_eq!(lang[0]["system_site_packages"], false);
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
                    disposition: FindingKind::included(),
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
                    disposition: FindingKind::excluded(),
                    under_var: false,
                    ..Default::default()
                },
            ],
            usr_entries: Vec::new(),
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
            has_ifcfg: false,
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
                disposition: FindingKind::included(),
                locked: false,
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

    /// A sysctl row offers a control only where there is a decision to
    /// make: `context_decision` refuses `Advisory` and `Inventory`
    /// (§ 3.5), because a row that rendered one would offer a toggle the
    /// server rejects.
    ///
    /// This is the only fixture in the workspace that puts a
    /// non-`Actionable` disposition on a sysctl. Every other
    /// `SysctlOverride` literal is `included()` or `excluded()`, so
    /// without this case the sysctl call site could construct a
    /// `ContextDecision` directly and no test would notice.
    #[test]
    fn a_non_actionable_sysctl_row_offers_no_control() {
        use inspectah_refine::projection::{RefKernelBoot, RefSysctlOverride};

        let sysctl = |key: &str, disposition: FindingKind| RefSysctlOverride {
            key: key.into(),
            runtime: "1".into(),
            default: "0".into(),
            source: "/etc/sysctl.d/99-custom.conf".into(),
            disposition,
            locked: false,
        };
        let data = RefKernelBoot {
            sysctl_overrides: vec![
                sysctl("net.ipv4.ip_forward", FindingKind::included()),
                sysctl("vm.swappiness", FindingKind::inventory()),
            ],
            ..Default::default()
        };

        let section = web_kernel_boot_section(&data);
        let items = &section
            .subsections
            .iter()
            .find(|s| s.id == "customizations")
            .expect("the customizations subsection")
            .items;
        let decision_of = |key: &str| {
            items
                .iter()
                .find(|i| i.id == key)
                .unwrap_or_else(|| panic!("row {key} is present"))
                .decision
                .as_ref()
        };

        // Both halves: the actionable row must still decide, or "no
        // decision" would be satisfied by a section that renders none.
        assert_eq!(
            decision_of("net.ipv4.ip_forward").map(|d| d.include),
            Some(true),
            "an actionable sysctl is decided on its own row"
        );
        assert!(
            decision_of("vm.swappiness").is_none(),
            "a non-actionable sysctl row must offer no toggle, got {:?}",
            decision_of("vm.swappiness")
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

    #[test]
    fn web_network_section_ifcfg_fields() {
        use inspectah_refine::projection::{RefNMConnection, RefNetwork};

        // Network with ifcfg connections
        let data = RefNetwork {
            connections: vec![RefNMConnection {
                name: "eth0".into(),
                conn_type: "ethernet".into(),
                method: "auto".into(),
                path: "/etc/sysconfig/network-scripts/ifcfg-eth0".into(),
            }],
            has_ifcfg: true,
            ..Default::default()
        };
        let section = web_network_section(&data);
        assert!(section.has_ifcfg, "has_ifcfg must be true");
        assert!(section.ifcfg_note.is_some(), "ifcfg_note must be populated");
        assert!(
            section
                .ifcfg_note
                .as_deref()
                .unwrap()
                .contains("ifcfg network scripts"),
            "note must reference ifcfg"
        );

        // Network without ifcfg connections
        let data_no_ifcfg = RefNetwork {
            connections: vec![RefNMConnection {
                name: "eth0".into(),
                conn_type: "ethernet".into(),
                method: "auto".into(),
                path: "/etc/NetworkManager/system-connections/eth0.nmconnection".into(),
            }],
            has_ifcfg: false,
            ..Default::default()
        };
        let section_no = web_network_section(&data_no_ifcfg);
        assert!(!section_no.has_ifcfg, "has_ifcfg must be false");
        assert!(section_no.ifcfg_note.is_none(), "no note when no ifcfg");
    }

    #[test]
    fn web_network_section_ifcfg_skipped_in_json_when_false() {
        use inspectah_refine::projection::RefNetwork;

        let data = RefNetwork::default();
        let section = web_network_section(&data);
        let json = serde_json::to_value(&section).unwrap();
        assert!(
            json.get("has_ifcfg").is_none(),
            "has_ifcfg false must be skipped in serialization"
        );
        assert!(
            json.get("ifcfg_note").is_none(),
            "ifcfg_note None must be skipped in serialization"
        );
    }

    #[test]
    fn ref_service_item_carries_shadow_fields() {
        use inspectah_core::types::ShadowType;
        use inspectah_refine::projection::RefServiceItem;

        let item = RefServiceItem {
            unit: "httpd.service".into(),
            current_state: ServiceUnitState::Enabled,
            default_state: Some(PresetDefault::Disable),
            owning_package: Some("httpd".into()),
            dropin_contents: vec![],
            shadow_type: Some(ShadowType::FullShadow),
            shadow_rationale: Some("full override by custom unit".into()),
        };
        assert!(item.shadow_type.is_some());
        assert_eq!(
            item.shadow_rationale.as_deref(),
            Some("full override by custom unit")
        );
    }

    #[test]
    fn service_decision_dto_wires_shadow_fields() {
        use inspectah_core::types::services::{ServiceSection, ServiceStateChange};
        use inspectah_core::types::{FindingKind, ShadowType};

        let mut snap = InspectionSnapshot::new();
        snap.services = Some(ServiceSection {
            state_changes: vec![ServiceStateChange {
                unit: "httpd.service".into(),
                current_state: ServiceUnitState::Enabled,
                default_state: Some(PresetDefault::Disable),
                owning_package: Some("httpd".into()),
                disposition: FindingKind::included(),
                locked: false,
                aggregate: None,
                attention_reason: None,
                shadow_type: Some(ShadowType::FullShadow),
                shadow_rationale: Some("Custom unit file overrides package default".into()),
            }],
            ..Default::default()
        });

        let session = RefineSession::new(snap);
        let response = build_web_view(&session);
        let json = serde_json::to_value(&response).expect("serialize");

        let states = json["service_states"].as_array().unwrap();
        assert_eq!(states.len(), 1);
        assert_eq!(states[0]["unit"], "httpd.service");
        assert_eq!(states[0]["shadow_type"], "full_shadow");
        assert_eq!(
            states[0]["shadow_rationale"],
            "Custom unit file overrides package default"
        );
    }

    // -- Task 7: `RepoGroupInfo`'s effective-member counts (§ 3.2) ----------

    /// § 8.8 "Hidden member": a `Verified` "vendor" repo with one included
    /// visible leaf ("a") and one actionable, unlocked, non-leaf dependency
    /// ("b") the display filter hides -- originally excluded at
    /// normalization, not `Investigate`. A baseline (even empty) is required
    /// so "a" normalizes to Site -> leaf -> included; "b" is left off
    /// `leaf_packages` so it normalizes to Site -> non-leaf -> excluded and
    /// the hidden-dependency filter drops it from `view.packages`.
    fn repo_group_info_for(section_id: &str) -> RepoGroupInfo {
        use inspectah_core::baseline::BaselineData;
        use inspectah_core::types::rpm::{PackageEntry, PackageState, RepoFile, RpmSection};

        let mut snap = InspectionSnapshot::new();
        snap.baseline = Some(BaselineData {
            image_digest: "sha256:test".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });
        snap.rpm = Some(RpmSection {
            packages_added: vec![
                PackageEntry {
                    name: "a".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    source_repo: "vendor".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                },
                PackageEntry {
                    name: "b".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    source_repo: "vendor".into(),
                    disposition: FindingKind::excluded(),
                    ..Default::default()
                },
            ],
            leaf_packages: Some(vec!["a.x86_64".into()]),
            repo_files: vec![RepoFile {
                path: "/etc/yum.repos.d/vendor.repo".into(),
                content: "[vendor]\nname=Vendor\nbaseurl=http://example.invalid/vendor\n".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            ..Default::default()
        });

        let session = RefineSession::new(snap);
        let response = build_web_view(&session);
        response
            .repo_groups
            .into_iter()
            .find(|rg| rg.section_id == section_id)
            .expect("repo present in view")
    }

    /// Pre-task, `RepoGroupInfo` had no `effective_*` fields at all, so this
    /// fails to compile against pre-task code -- the strongest form of
    /// "fails before, passes after." What the old `package_count`-only
    /// computation reports for "vendor" is 1 (the visible leaf only); the
    /// effective set the control actually acts on is 2, with 1 included and
    /// 1 unlisted, which `package_count` alone cannot distinguish.
    #[test]
    fn repo_group_info_counts_the_effective_set_not_the_visible_one() {
        let rg = repo_group_info_for("vendor");
        assert_eq!(
            (
                rg.effective_member_count,
                rg.effective_included,
                rg.effective_unlisted,
                rg.package_count
            ),
            (2, 1, 1, 1)
        );
    }

    /// A `Verified` "vendor" repo with two actionable, unlocked members, used
    /// by `state_a`/`state_b` below. Mirrors
    /// `inspectah_refine`'s own `verified_repo_session` test fixture
    /// (`crates/refine/tests/session_test.rs`), rebuilt here because
    /// integration test fixtures aren't shared across crates.
    fn two_member_vendor_session() -> RefineSession {
        use inspectah_core::baseline::BaselineData;
        use inspectah_core::types::rpm::{PackageEntry, PackageState, RepoFile, RpmSection};

        let mut snap = InspectionSnapshot::new();
        snap.baseline = Some(BaselineData {
            image_digest: "sha256:test".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });
        snap.rpm = Some(RpmSection {
            packages_added: vec![
                PackageEntry {
                    name: "a".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    source_repo: "vendor".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                },
                PackageEntry {
                    name: "b".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    source_repo: "vendor".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                },
            ],
            repo_files: vec![RepoFile {
                path: "/etc/yum.repos.d/vendor.repo".into(),
                content: "[vendor]\nname=Vendor\nbaseurl=http://example.invalid/vendor\n".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            ..Default::default()
        });
        RefineSession::new(snap)
    }

    /// `(enabled, effective_member_count, effective_included, effective_unlisted)`
    /// for the "vendor" repo, read the same way the § 6.3 Switch does.
    fn repo_disagreement_state(session: &RefineSession) -> (bool, usize, usize, usize) {
        let rg = build_web_view(session)
            .repo_groups
            .into_iter()
            .find(|rg| rg.section_id == "vendor")
            .expect("vendor repo present in view");
        (
            rg.enabled,
            rg.effective_member_count,
            rg.effective_included,
            rg.effective_unlisted,
        )
    }

    /// State (a), § 8.8: enabled history (no repo op), both members
    /// individually excluded.
    fn state_a() -> (bool, usize, usize, usize) {
        use inspectah_refine::types::{ItemId, RefinementOp};

        let mut s = two_member_vendor_session();
        for name in ["a", "b"] {
            s.apply(RefinementOp::SetInclude {
                item_id: ItemId::Package {
                    name: name.into(),
                    arch: "x86_64".into(),
                },
                include: false,
            })
            .unwrap();
        }
        repo_disagreement_state(&s)
    }

    /// State (b), § 8.8: excluded history (a recorded repo exclude), both
    /// members individually included.
    fn state_b() -> (bool, usize, usize, usize) {
        use inspectah_refine::types::{ItemId, RefinementOp};

        let mut s = two_member_vendor_session();
        s.apply(RefinementOp::SetInclude {
            item_id: ItemId::Repo {
                path: "vendor".into(),
            },
            include: false,
        })
        .unwrap();
        for name in ["a", "b"] {
            s.apply(RefinementOp::SetInclude {
                item_id: ItemId::Package {
                    name: name.into(),
                    arch: "x86_64".into(),
                },
                include: true,
            })
            .unwrap();
        }
        repo_disagreement_state(&s)
    }

    /// § 8.8: the exact inputs the § 6.3 Switch reads, which § 8.12's
    /// frontend fixtures copy verbatim. Pre-task, `RepoGroupInfo` had no
    /// `effective_*` fields, so this fails to compile against pre-task code.
    #[test]
    fn build_repo_groups_reports_the_two_disagreement_states() {
        assert_eq!(state_a(), (true, 2, 0, 0)); // enabled, M=2, N=0, K=0
        assert_eq!(state_b(), (false, 2, 2, 0)); // excluded history, M=2, N=2, K=0
    }
}
