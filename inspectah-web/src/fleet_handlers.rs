use axum::extract::State;
use axum::response::IntoResponse;
use axum::response::Json;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::fleet::PrevalenceZone;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{
    AttentionLevel, AttentionReason, AttentionTag, FleetAttention, FleetContext, ItemId,
};
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;

use crate::handlers::AppState;

// ---------------------------------------------------------------------------
// DTOs — presentation-layer types for the fleet view JSON response
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct FleetViewResponse {
    pub generation: u64,
    pub can_undo: bool,
    pub can_redo: bool,
    pub containerfile_preview: String,
    pub session_is_sensitive: bool,
    pub summary: FleetSummary,
    pub sections: Vec<FleetSection>,
}

#[derive(Serialize)]
pub struct FleetSummary {
    pub host_count: usize,
    pub actionable_variant_items: Vec<ActionableVariantItem>,
    pub informational_variant_count: usize,
}

#[derive(Serialize)]
pub struct ActionableVariantItem {
    pub item_id: ItemId,
    pub section_id: String,
    pub variant_count: usize,
    pub max_host_spread: usize,
}

#[derive(Serialize)]
pub struct FleetSection {
    pub id: String,
    pub display_name: String,
    pub is_decision_section: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zones: Option<FleetZones>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<FleetItem>>,
}

#[derive(Serialize)]
pub struct FleetZones {
    pub consensus: FleetZoneGroup,
    pub near_consensus: FleetZoneGroup,
    pub divergent: FleetZoneGroup,
}

#[derive(Serialize)]
pub struct FleetZoneGroup {
    pub items: Vec<FleetItem>,
    pub count: usize,
}

#[derive(Clone, Serialize)]
pub struct FleetItem {
    pub item_id: ItemId,
    pub include: bool,
    pub attention: FleetAttentionDto,
    pub prevalence: FleetPrevalenceDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variants: Option<FleetVariants>,
}

#[derive(Clone, Serialize)]
pub struct FleetAttentionDto {
    pub level: AttentionLevel,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<PrevalenceZone>,
    pub prevalence: u32,
}

#[derive(Clone, Serialize)]
pub struct FleetPrevalenceDto {
    pub count: u32,
    pub total: u32,
}

#[derive(Clone, Serialize)]
pub struct FleetVariants {
    pub count: usize,
    pub selected: String,
    pub options: Vec<FleetVariantOption>,
}

#[derive(Clone, Serialize)]
pub struct FleetVariantOption {
    pub hash: String,
    pub hosts: Vec<String>,
    pub host_count: usize,
    pub selected: bool,
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

pub async fn fleet_view(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let session = state.session.lock().unwrap();
    match session.fleet_context() {
        Some(ctx) => {
            let response = build_fleet_view_response(&session, ctx);
            Json(serde_json::to_value(&response).unwrap()).into_response()
        }
        None => Json(json!({"error": "not a fleet session"})).into_response(),
    }
}

// ---------------------------------------------------------------------------
// Response builder
// ---------------------------------------------------------------------------

fn build_fleet_view_response(
    session: &RefineSession,
    ctx: &FleetContext,
) -> FleetViewResponse {
    let view = session.view();
    let snap = session.snapshot_projected();

    FleetViewResponse {
        generation: session.generation(),
        can_undo: session.can_undo(),
        can_redo: session.can_redo(),
        containerfile_preview: view.containerfile_preview.clone(),
        session_is_sensitive: session.is_sensitive(),
        summary: build_fleet_summary(&snap, ctx),
        sections: build_fleet_sections(session, &snap, ctx),
    }
}

// ---------------------------------------------------------------------------
// Summary builder
// ---------------------------------------------------------------------------

fn build_fleet_summary(
    snap: &InspectionSnapshot,
    ctx: &FleetContext,
) -> FleetSummary {
    let variant_summary =
        inspectah_refine::fleet::variant_summary(snap, Some(ctx));

    let mut actionable_variant_items = Vec::new();
    let mut informational_variant_count: usize = 0;

    if let Some(ref vs) = variant_summary {
        for (path, info) in &vs.variant_distribution {
            // Config variants are actionable (decision section)
            let max_host_spread = info.host_split.iter().copied().max().unwrap_or(0);
            actionable_variant_items.push(ActionableVariantItem {
                item_id: ItemId::Config {
                    path: path.clone(),
                },
                section_id: "configs".to_string(),
                variant_count: info.variant_count,
                max_host_spread,
            });
        }
    }

    // Count non-config variants (informational) from context sections.
    // Currently config is the only section with variant tracking, so this
    // stays 0 until other sections gain fleet_prevalence variant support.
    _ = &mut informational_variant_count;

    FleetSummary {
        host_count: ctx.total_hosts,
        actionable_variant_items,
        informational_variant_count,
    }
}

// ---------------------------------------------------------------------------
// Section builders
// ---------------------------------------------------------------------------

fn build_fleet_sections(
    session: &RefineSession,
    snap: &InspectionSnapshot,
    ctx: &FleetContext,
) -> Vec<FleetSection> {
    let view = session.view();
    let mut sections = Vec::new();

    // --- Decision sections: packages, configs ---
    // Packages
    if snap.rpm.is_some() {
        let items: Vec<FleetItem> = view
            .packages
            .iter()
            .map(|pkg| {
                let item_id = ItemId::Package {
                    name_arch: format!("{}.{}", pkg.entry.name, pkg.entry.arch),
                };
                let fa = pkg.fleet_attention;
                let fp = pkg.entry.fleet.as_ref();
                FleetItem {
                    item_id,
                    include: pkg.entry.include,
                    attention: build_attention_dto(&pkg.attention, fa),
                    prevalence: fleet_prevalence_dto(fp, ctx),
                    variants: None,
                }
            })
            .collect();

        sections.push(build_section("packages", "Packages", true, &items, ctx));
    }

    // Configs
    if let Some(ref config) = snap.config {
        // Group config entries by path to identify variants.
        let mut entries_by_path: std::collections::BTreeMap<
            &str,
            Vec<&inspectah_core::types::config::ConfigFileEntry>,
        > = std::collections::BTreeMap::new();
        for entry in &config.files {
            entries_by_path
                .entry(entry.path.as_str())
                .or_default()
                .push(entry);
        }

        let items: Vec<FleetItem> = view
            .config_files
            .iter()
            .filter(|cfg| {
                // In fleet mode, only emit the Selected variant (or Only) for
                // each path. Alternative variants are folded into `variants`.
                use inspectah_core::types::fleet::VariantSelection;
                matches!(
                    cfg.entry.variant_selection,
                    VariantSelection::Selected | VariantSelection::Only
                )
            })
            .map(|cfg| {
                let item_id = ItemId::Config {
                    path: cfg.entry.path.clone(),
                };
                let fa = cfg.fleet_attention;
                let fp = cfg.entry.fleet.as_ref();

                // Build variant info if this path has multiple entries.
                let path_entries = entries_by_path.get(cfg.entry.path.as_str());
                let variants = path_entries
                    .filter(|entries| entries.len() >= 2)
                    .map(|entries| build_variants(entries, cfg));

                FleetItem {
                    item_id,
                    include: cfg.entry.include,
                    attention: build_attention_dto(&cfg.attention, fa),
                    prevalence: fleet_prevalence_dto(fp, ctx),
                    variants,
                }
            })
            .collect();

        sections.push(build_section("configs", "Configuration Files", true, &items, ctx));
    }

    // --- Context sections (read-only, no toggles) ---
    // These sections come from the snapshot, not from RefinedView.
    // Items have fleet prevalence but no include/exclude toggle.
    build_context_sections(&mut sections, snap, ctx);

    sections
}

/// Build a `FleetSection` from items, zone-grouping when `zones_active`.
fn build_section(
    id: &str,
    display_name: &str,
    is_decision_section: bool,
    items: &[FleetItem],
    ctx: &FleetContext,
) -> FleetSection {
    if ctx.zones_active {
        // Group items by zone
        let mut consensus = Vec::new();
        let mut near_consensus = Vec::new();
        let mut divergent = Vec::new();

        for item in items {
            match item.attention.zone {
                Some(PrevalenceZone::Consensus) => consensus.push(item),
                Some(PrevalenceZone::NearConsensus) => near_consensus.push(item),
                Some(PrevalenceZone::Divergent) => divergent.push(item),
                None => consensus.push(item), // unclassified → consensus bucket
            }
        }

        FleetSection {
            id: id.to_string(),
            display_name: display_name.to_string(),
            is_decision_section,
            zones: Some(FleetZones {
                consensus: FleetZoneGroup {
                    count: consensus.len(),
                    items: consensus.into_iter().cloned().collect(),
                },
                near_consensus: FleetZoneGroup {
                    count: near_consensus.len(),
                    items: near_consensus.into_iter().cloned().collect(),
                },
                divergent: FleetZoneGroup {
                    count: divergent.len(),
                    items: divergent.into_iter().cloned().collect(),
                },
            }),
            items: None,
        }
    } else {
        // Fleet-of-2: flat list, no zone grouping
        FleetSection {
            id: id.to_string(),
            display_name: display_name.to_string(),
            is_decision_section,
            zones: None,
            items: Some(items.to_vec()),
        }
    }
}

// ---------------------------------------------------------------------------
// Context section builders (services, containers, etc.)
// ---------------------------------------------------------------------------

fn build_context_sections(
    sections: &mut Vec<FleetSection>,
    snap: &InspectionSnapshot,
    ctx: &FleetContext,
) {
    // Services
    if let Some(ref svc) = snap.services {
        let items: Vec<FleetItem> = svc
            .state_changes
            .iter()
            .map(|unit| {
                let item_id = ItemId::Service {
                    unit: unit.unit.clone(),
                };
                let fp = unit.fleet.as_ref();
                FleetItem {
                    item_id,
                    include: true,
                    attention: default_context_attention(fp, ctx),
                    prevalence: fleet_prevalence_dto(fp, ctx),
                    variants: None,
                }
            })
            .collect();
        if !items.is_empty() {
            sections.push(build_section("services", "Services", false, &items, ctx));
        }
    }

    // Containers (quadlets + compose)
    if let Some(ref containers) = snap.containers {
        let mut items: Vec<FleetItem> = Vec::new();
        for q in &containers.quadlet_units {
            let item_id = ItemId::Quadlet {
                path: q.path.clone(),
            };
            let fp = q.fleet.as_ref();
            items.push(FleetItem {
                item_id,
                include: true,
                attention: default_context_attention(fp, ctx),
                prevalence: fleet_prevalence_dto(fp, ctx),
                variants: None,
            });
        }
        for c in &containers.compose_files {
            let item_id = ItemId::Compose {
                path: c.path.clone(),
            };
            let fp = c.fleet.as_ref();
            items.push(FleetItem {
                item_id,
                include: true,
                attention: default_context_attention(fp, ctx),
                prevalence: fleet_prevalence_dto(fp, ctx),
                variants: None,
            });
        }
        if !items.is_empty() {
            sections.push(build_section(
                "containers",
                "Containers",
                false,
                &items,
                ctx,
            ));
        }
    }

    // Network
    if let Some(ref net) = snap.network {
        let mut items: Vec<FleetItem> = Vec::new();
        for conn in &net.connections {
            let item_id = ItemId::NMConnection {
                path: conn.path.clone(),
            };
            let fp = conn.fleet.as_ref();
            items.push(FleetItem {
                item_id,
                include: true,
                attention: default_context_attention(fp, ctx),
                prevalence: fleet_prevalence_dto(fp, ctx),
                variants: None,
            });
        }
        for zone in &net.firewall_zones {
            let item_id = ItemId::FirewallZone {
                path: zone.path.clone(),
            };
            let fp = zone.fleet.as_ref();
            items.push(FleetItem {
                item_id,
                include: true,
                attention: default_context_attention(fp, ctx),
                prevalence: fleet_prevalence_dto(fp, ctx),
                variants: None,
            });
        }
        if !items.is_empty() {
            sections.push(build_section("network", "Network", false, &items, ctx));
        }
    }

    // Storage
    if let Some(ref storage) = snap.storage {
        let items: Vec<FleetItem> = storage
            .fstab_entries
            .iter()
            .map(|entry| {
                let item_id = ItemId::Fstab {
                    mount_point: entry.mount_point.clone(),
                };
                let fp = entry.fleet.as_ref();
                FleetItem {
                    item_id,
                    include: true,
                    attention: default_context_attention(fp, ctx),
                    prevalence: fleet_prevalence_dto(fp, ctx),
                    variants: None,
                }
            })
            .collect();
        if !items.is_empty() {
            sections.push(build_section("storage", "Storage", false, &items, ctx));
        }
    }

    // Scheduled tasks
    if let Some(ref sched) = snap.scheduled_tasks {
        let mut items: Vec<FleetItem> = Vec::new();
        for cron in &sched.cron_jobs {
            let item_id = ItemId::CronJob {
                path: cron.path.clone(),
            };
            let fp = cron.fleet.as_ref();
            items.push(FleetItem {
                item_id,
                include: true,
                attention: default_context_attention(fp, ctx),
                prevalence: fleet_prevalence_dto(fp, ctx),
                variants: None,
            });
        }
        for timer in &sched.systemd_timers {
            let item_id = ItemId::SystemdTimer {
                name: timer.name.clone(),
            };
            let fp = timer.fleet.as_ref();
            items.push(FleetItem {
                item_id,
                include: true,
                attention: default_context_attention(fp, ctx),
                prevalence: fleet_prevalence_dto(fp, ctx),
                variants: None,
            });
        }
        if !items.is_empty() {
            sections.push(build_section(
                "scheduled",
                "Scheduled Tasks",
                false,
                &items,
                ctx,
            ));
        }
    }

    // SELinux
    if let Some(ref selinux) = snap.selinux {
        let items: Vec<FleetItem> = selinux
            .port_labels
            .iter()
            .map(|port| {
                let item_id = ItemId::SelinuxPort {
                    protocol_port: format!("{}:{}", port.protocol, port.port),
                };
                let fp = port.fleet.as_ref();
                FleetItem {
                    item_id,
                    include: true,
                    attention: default_context_attention(fp, ctx),
                    prevalence: fleet_prevalence_dto(fp, ctx),
                    variants: None,
                }
            })
            .collect();
        if !items.is_empty() {
            sections.push(build_section("selinux", "SELinux", false, &items, ctx));
        }
    }

    // Kernel & Boot
    if let Some(ref kb) = snap.kernel_boot {
        let mut items: Vec<FleetItem> = Vec::new();
        for module in &kb.loaded_modules {
            let item_id = ItemId::KernelModule {
                name: module.name.clone(),
            };
            let fp = module.fleet.as_ref();
            items.push(FleetItem {
                item_id,
                include: true,
                attention: default_context_attention(fp, ctx),
                prevalence: fleet_prevalence_dto(fp, ctx),
                variants: None,
            });
        }
        for sysctl in &kb.sysctl_overrides {
            let item_id = ItemId::Sysctl {
                key: sysctl.key.clone(),
            };
            let fp = sysctl.fleet.as_ref();
            items.push(FleetItem {
                item_id,
                include: true,
                attention: default_context_attention(fp, ctx),
                prevalence: fleet_prevalence_dto(fp, ctx),
                variants: None,
            });
        }
        if !items.is_empty() {
            sections.push(build_section(
                "kernel_boot",
                "Kernel & Boot",
                false,
                &items,
                ctx,
            ));
        }
    }

    // Non-RPM Software
    if let Some(ref nonrpm) = snap.non_rpm_software {
        let items: Vec<FleetItem> = nonrpm
            .items
            .iter()
            .map(|entry| {
                let item_id = ItemId::NonRpm {
                    name: entry.name.clone(),
                };
                let fp = entry.fleet.as_ref();
                FleetItem {
                    item_id,
                    include: true,
                    attention: default_context_attention(fp, ctx),
                    prevalence: fleet_prevalence_dto(fp, ctx),
                    variants: None,
                }
            })
            .collect();
        if !items.is_empty() {
            sections.push(build_section("nonrpm", "Non-RPM Software", false, &items, ctx));
        }
    }

    // NOTE: users_groups is DEFERRED — not included in fleet view
    // NOTE: version_changes comes from rpm section diffs, not a standalone section
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_attention_dto(
    tags: &[AttentionTag],
    fleet_attention: Option<FleetAttention>,
) -> FleetAttentionDto {
    let level = tags
        .first()
        .map(|t| t.level)
        .unwrap_or(AttentionLevel::Routine);
    let reason = tags
        .first()
        .map(|t| attention_reason_to_string(&t.reason))
        .unwrap_or_else(|| "routine".to_string());
    let (zone, prevalence) = match fleet_attention {
        Some(fa) => (fa.zone, fa.prevalence),
        None => (None, 0),
    };
    FleetAttentionDto {
        level,
        reason,
        zone,
        prevalence,
    }
}

fn default_context_attention(
    fp: Option<&inspectah_core::types::fleet::FleetPrevalence>,
    ctx: &FleetContext,
) -> FleetAttentionDto {
    let zone = fp.map(|f| {
        inspectah_core::fleet::classify_zone(f)
    });
    let zone = if ctx.zones_active { zone } else { None };
    FleetAttentionDto {
        level: AttentionLevel::Informational,
        reason: "context_item".to_string(),
        zone,
        prevalence: fp.map(|f| f.count.max(0) as u32).unwrap_or(0),
    }
}

fn fleet_prevalence_dto(
    fp: Option<&inspectah_core::types::fleet::FleetPrevalence>,
    ctx: &FleetContext,
) -> FleetPrevalenceDto {
    FleetPrevalenceDto {
        count: fp.map(|f| f.count.max(0) as u32).unwrap_or(0),
        total: fp
            .map(|f| f.total.max(0) as u32)
            .unwrap_or(ctx.total_hosts as u32),
    }
}

fn attention_reason_to_string(reason: &AttentionReason) -> String {
    match reason {
        AttentionReason::PackageBaselineMatch => "package_baseline_match".to_string(),
        AttentionReason::PackageUserAdded => "package_user_added".to_string(),
        AttentionReason::PackageVersionChanged => "package_version_changed".to_string(),
        AttentionReason::PackageProvenanceUnavailable => {
            "package_provenance_unavailable".to_string()
        }
        AttentionReason::PackageLocalInstall => "package_local_install".to_string(),
        AttentionReason::PackageNoRepoSource => "package_no_repo_source".to_string(),
        AttentionReason::ConfigDefault => "config_default".to_string(),
        AttentionReason::ConfigBaselineMatch => "config_baseline_match".to_string(),
        AttentionReason::ConfigModified => "config_modified".to_string(),
        AttentionReason::ConfigUnowned => "config_unowned".to_string(),
        AttentionReason::ConfigOrphaned => "config_orphaned".to_string(),
        AttentionReason::SensitivePath => "sensitive_path".to_string(),
        AttentionReason::ServiceImageModeIncompatible => {
            "service_image_mode_incompatible".to_string()
        }
        AttentionReason::Custom(s) => s.clone(),
    }
}

fn build_variants(
    entries: &[&inspectah_core::types::config::ConfigFileEntry],
    selected_cfg: &inspectah_refine::types::RefinedConfig,
) -> FleetVariants {
    use inspectah_core::types::fleet::VariantSelection;
    use inspectah_refine::types::ContentHash;

    let selected_hash = ContentHash::from_content(selected_cfg.entry.content.as_bytes());

    let options: Vec<FleetVariantOption> = entries
        .iter()
        .map(|e| {
            let hash = ContentHash::from_content(e.content.as_bytes());
            let is_selected = matches!(e.variant_selection, VariantSelection::Selected)
                || (matches!(e.variant_selection, VariantSelection::Only)
                    && hash == selected_hash);
            FleetVariantOption {
                hash: hash.as_str().to_string(),
                hosts: e.fleet.as_ref().map(|f| f.hosts.clone()).unwrap_or_default(),
                host_count: e
                    .fleet
                    .as_ref()
                    .map(|f| f.count.max(0) as usize)
                    .unwrap_or(0),
                selected: is_selected,
            }
        })
        .collect();

    FleetVariants {
        count: entries.len(),
        selected: selected_hash.as_str().to_string(),
        options,
    }
}
