use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Json;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::fleet::{FleetPrevalence, PrevalenceZone, VariantSelection};
use inspectah_refine::classify::{
    classify_containers, classify_services, classify_sysctls, classify_tuned,
};
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{
    ContentHash, FleetContext, ItemId, Triage, TriageBucket, TriageReason, TriageTag,
};
use serde::{Deserialize, Serialize};
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
    pub repo_groups: Vec<crate::handlers::RepoGroupInfo>,
    pub repo_conflict_count: usize,
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
pub struct RepoSourceEntryDto {
    pub repo: String,
    pub host_count: usize,
}

#[derive(Clone, Serialize)]
pub struct FleetItem {
    pub item_id: ItemId,
    pub include: bool,
    pub triage: FleetTriageDto,
    pub prevalence: FleetPrevalenceDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variants: Option<FleetVariants>,
    pub source_repo: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_conflict: Option<Vec<RepoSourceEntryDto>>,
}

#[derive(Clone, Serialize)]
pub struct FleetTriageDto {
    pub bucket: String,
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
// DTOs — fleet diff endpoint
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct FleetDiffRequest {
    pub item_id: ItemId,
    pub base: String,
    pub target: String,
}

#[derive(Serialize)]
pub struct FleetDiffResponse {
    pub base_hash: String,
    pub target_hash: String,
    pub base_hosts: Vec<String>,
    pub target_hosts: Vec<String>,
    pub hunks: Vec<FleetDiffHunk>,
    pub stats: FleetDiffStats,
}

#[derive(Serialize)]
pub struct FleetDiffHunk {
    pub base_range: FleetLineRange,
    pub target_range: FleetLineRange,
    pub changes: Vec<FleetDiffChange>,
}

#[derive(Serialize)]
pub struct FleetLineRange {
    pub start: usize,
    pub count: usize,
}

#[derive(Serialize)]
pub struct FleetDiffChange {
    pub kind: String,
    pub content: String,
}

#[derive(Serialize)]
pub struct FleetDiffStats {
    pub total_changes: usize,
    pub insertions: usize,
    pub deletions: usize,
}

// ---------------------------------------------------------------------------
// Handlers
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

pub async fn fleet_diff(
    State(state): State<Arc<AppState>>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let req: FleetDiffRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("invalid request: {e}")})),
            )
                .into_response();
        }
    };

    let path = match &req.item_id {
        ItemId::Config { path } => path.clone(),
        _ => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({"error": "diff is only supported for config items"})),
            )
                .into_response();
        }
    };

    let session = state.session.lock().unwrap();
    let snap = session.snapshot_projected();

    let config = match snap.config.as_ref() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({"error": "no config section in snapshot"})),
            )
                .into_response();
        }
    };

    // Collect all entries for this path.
    let entries: Vec<_> = config.files.iter().filter(|e| e.path == path).collect();
    if entries.is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": format!("unknown config path: {path}")})),
        )
            .into_response();
    }

    // Find entries matching the requested hashes.
    let base_entry = entries
        .iter()
        .find(|e| ContentHash::from_content(e.content.as_bytes()).as_str() == req.base);
    let target_entry = entries
        .iter()
        .find(|e| ContentHash::from_content(e.content.as_bytes()).as_str() == req.target);

    let base_entry = match base_entry {
        Some(e) => e,
        None => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({"error": format!("unknown base hash: {}", req.base)})),
            )
                .into_response();
        }
    };
    let target_entry = match target_entry {
        Some(e) => e,
        None => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({"error": format!("unknown target hash: {}", req.target)})),
            )
                .into_response();
        }
    };

    // Extract host lists from fleet prevalence.
    let base_hosts = base_entry
        .fleet
        .as_ref()
        .map(|f| f.hosts.clone())
        .unwrap_or_default();
    let target_hosts = target_entry
        .fleet
        .as_ref()
        .map(|f| f.hosts.clone())
        .unwrap_or_default();

    // Compute the diff.
    let diff_result = match inspectah_refine::fleet::diff::compute_diff(
        &base_entry.content,
        &target_entry.content,
        3,
    ) {
        Ok(r) => r,
        Err(inspectah_refine::fleet::diff::DiffError::BinaryContent) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({"error": "binary content cannot be diffed"})),
            )
                .into_response();
        }
        Err(inspectah_refine::fleet::diff::DiffError::InputTooLarge) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({"error": "content exceeds size limit for diffing"})),
            )
                .into_response();
        }
    };

    // Map DiffResult to response DTOs.
    let hunks: Vec<FleetDiffHunk> = diff_result
        .hunks
        .into_iter()
        .map(|h| FleetDiffHunk {
            base_range: FleetLineRange {
                start: h.base_range.start,
                count: h.base_range.count,
            },
            target_range: FleetLineRange {
                start: h.target_range.start,
                count: h.target_range.count,
            },
            changes: h
                .changes
                .into_iter()
                .map(|c| {
                    use inspectah_refine::fleet::diff::ChangeKind;
                    FleetDiffChange {
                        kind: match c.kind {
                            ChangeKind::Equal => "equal".to_string(),
                            ChangeKind::Delete => "delete".to_string(),
                            ChangeKind::Insert => "insert".to_string(),
                        },
                        content: c.content,
                    }
                })
                .collect(),
        })
        .collect();

    let response = FleetDiffResponse {
        base_hash: req.base,
        target_hash: req.target,
        base_hosts,
        target_hosts,
        hunks,
        stats: FleetDiffStats {
            total_changes: diff_result.stats.total_changes,
            insertions: diff_result.stats.insertions,
            deletions: diff_result.stats.deletions,
        },
    };

    Json(serde_json::to_value(&response).unwrap()).into_response()
}

// ---------------------------------------------------------------------------
// Response builder
// ---------------------------------------------------------------------------

fn build_fleet_view_response(session: &RefineSession, ctx: &FleetContext) -> FleetViewResponse {
    let view = session.view();
    let snap = session.snapshot_projected();
    let sections = build_fleet_sections(session, &snap, ctx);
    let summary = build_fleet_summary(&snap, ctx, &sections);
    let repo_groups = crate::handlers::build_repo_groups(session);
    let repo_conflict_count = ctx.repo_conflicts.len();

    FleetViewResponse {
        generation: session.generation(),
        can_undo: session.can_undo(),
        can_redo: session.can_redo(),
        containerfile_preview: view.containerfile_preview.clone(),
        session_is_sensitive: session.is_sensitive(),
        summary,
        sections,
        repo_groups,
        repo_conflict_count,
    }
}

// ---------------------------------------------------------------------------
// Summary builder
// ---------------------------------------------------------------------------

fn build_fleet_summary(
    snap: &InspectionSnapshot,
    ctx: &FleetContext,
    sections: &[FleetSection],
) -> FleetSummary {
    let variant_summary = inspectah_refine::fleet::variant_summary(snap, Some(ctx));

    let mut actionable_variant_items = Vec::new();

    if let Some(ref vs) = variant_summary {
        for (path, info) in &vs.variant_distribution {
            // Config variants are actionable (decision section)
            let max_host_spread = info.host_split.iter().copied().max().unwrap_or(0);
            actionable_variant_items.push(ActionableVariantItem {
                item_id: ItemId::Config { path: path.clone() },
                section_id: "configs".to_string(),
                variant_count: info.variant_count,
                max_host_spread,
            });
        }
    }

    // Count non-config items that have variants (informational).
    // This is an item count (how many items have variants), not a variant
    // option count. The frontend displays "N additional items have variants."
    let informational_variant_count = sections
        .iter()
        .filter(|s| !s.is_decision_section)
        .flat_map(section_items)
        .filter(|item| item.variants.is_some())
        .count();

    FleetSummary {
        host_count: ctx.total_hosts,
        actionable_variant_items,
        informational_variant_count,
    }
}

/// Iterate over all items in a section regardless of zone/flat layout.
fn section_items(section: &FleetSection) -> impl Iterator<Item = &FleetItem> {
    let zone_items = section.zones.iter().flat_map(|z| {
        z.consensus
            .items
            .iter()
            .chain(z.near_consensus.items.iter())
            .chain(z.divergent.items.iter())
    });
    let flat_items = section.items.iter().flat_map(|items| items.iter());
    zone_items.chain(flat_items)
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
                    name: pkg.entry.name.clone(),
                    arch: pkg.entry.arch.clone(),
                };
                let fp = pkg.entry.fleet.as_ref();
                let name_arch_key = format!("{}.{}", pkg.entry.name, pkg.entry.arch);
                let repo_conflict = ctx.repo_conflicts.get(&name_arch_key).map(|entries| {
                    entries
                        .iter()
                        .map(|e| RepoSourceEntryDto {
                            repo: e.repo.clone(),
                            host_count: e.host_count,
                        })
                        .collect()
                });
                FleetItem {
                    item_id,
                    include: pkg.entry.include,
                    triage: build_triage_dto(&pkg.triage, fp, ctx),
                    prevalence: fleet_prevalence_dto(fp, ctx),
                    variants: None,
                    source_repo: pkg.entry.source_repo.clone(),
                    repo_conflict,
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
            .filter(|cfg| {
                // Filter out default/unmodified configs to reduce noise.
                // Only show configs that were actually modified by users.
                // A path is kept if ANY variant is user-modified; skipped
                // only when ALL variants are default kinds.
                use inspectah_core::types::config::ConfigFileKind;
                let dominated_by_defaults = entries_by_path
                    .get(cfg.entry.path.as_str())
                    .map(|entries| {
                        entries.iter().all(|e| {
                            matches!(
                                e.kind,
                                ConfigFileKind::RpmOwnedDefault | ConfigFileKind::BaselineMatch
                            )
                        })
                    })
                    .unwrap_or(false);
                !dominated_by_defaults
            })
            .map(|cfg| {
                let item_id = ItemId::Config {
                    path: cfg.entry.path.clone(),
                };
                let fp = cfg.entry.fleet.as_ref();

                // Build variant info if this path has multiple entries.
                let path_entries = entries_by_path.get(cfg.entry.path.as_str());
                let variants = path_entries
                    .filter(|entries| entries.len() >= 2)
                    .map(|entries| build_variants(entries, cfg));

                FleetItem {
                    item_id,
                    include: cfg.entry.include,
                    triage: build_triage_dto(&cfg.triage, fp, ctx),
                    prevalence: fleet_prevalence_dto(fp, ctx),
                    variants,
                    source_repo: String::new(),
                    repo_conflict: None,
                }
            })
            .collect();

        sections.push(build_section(
            "configs",
            "Configuration Files",
            true,
            &items,
            ctx,
        ));
    }

    // Services — classified as decision items with triage tags
    {
        let (states, dropins) = classify_services(snap);

        let mut items: Vec<FleetItem> = states
            .iter()
            .map(|s| {
                let item_id = ItemId::Service {
                    unit: s.entry.unit.clone(),
                };
                let fp = s.entry.fleet.as_ref();
                FleetItem {
                    item_id,
                    include: s.entry.include,
                    triage: build_triage_dto(&s.triage, fp, ctx),
                    prevalence: fleet_prevalence_dto(fp, ctx),
                    variants: None,
                    source_repo: String::new(),
                    repo_conflict: None,
                }
            })
            .collect();

        // Drop-in overrides — group by (unit, path) for variant detection
        let mut dropin_groups: std::collections::BTreeMap<
            (&str, &str),
            Vec<&inspectah_refine::types::RefinedDropIn>,
        > = std::collections::BTreeMap::new();
        for d in &dropins {
            dropin_groups
                .entry((d.entry.unit.as_str(), d.entry.path.as_str()))
                .or_default()
                .push(d);
        }

        for ((_, path), group) in &dropin_groups {
            let representative = group
                .iter()
                .find(|d| {
                    matches!(
                        d.entry.variant_selection,
                        VariantSelection::Selected | VariantSelection::Only
                    )
                })
                .or_else(|| group.first());
            if let Some(d) = representative {
                let item_id = ItemId::DropIn {
                    path: path.to_string(),
                };
                let fp = d.entry.fleet.as_ref();
                let variants = if group.len() >= 2 {
                    Some(build_content_variants(
                        &group
                            .iter()
                            .map(|d| {
                                (
                                    &d.entry.content,
                                    d.entry.variant_selection,
                                    d.entry.fleet.as_ref(),
                                )
                            })
                            .collect::<Vec<_>>(),
                    ))
                } else {
                    None
                };
                items.push(FleetItem {
                    item_id,
                    include: d.entry.include,
                    triage: build_triage_dto(&d.triage, fp, ctx),
                    prevalence: fleet_prevalence_dto(fp, ctx),
                    variants,
                    source_repo: String::new(),
                    repo_conflict: None,
                });
            }
        }

        if !items.is_empty() {
            sections.push(build_section("services", "Services", true, &items, ctx));
        }
    }

    // Containers — quadlets and flatpaks classified as decision items
    {
        let (quadlets, flatpaks) = classify_containers(snap);

        let mut items: Vec<FleetItem> = Vec::new();

        // Quadlets: group by path for variant detection (same pattern as drop-ins)
        let mut quadlet_groups: std::collections::BTreeMap<
            &str,
            Vec<&inspectah_refine::types::RefinedQuadlet>,
        > = std::collections::BTreeMap::new();
        for q in &quadlets {
            quadlet_groups
                .entry(q.entry.path.as_str())
                .or_default()
                .push(q);
        }

        for (path, group) in &quadlet_groups {
            let representative = group
                .iter()
                .find(|q| {
                    matches!(
                        q.entry.variant_selection,
                        VariantSelection::Selected | VariantSelection::Only
                    )
                })
                .or_else(|| group.first());
            if let Some(q) = representative {
                let item_id = ItemId::Quadlet {
                    path: path.to_string(),
                };
                let fp = q.entry.fleet.as_ref();
                let variants = if group.len() >= 2 {
                    Some(build_content_variants(
                        &group
                            .iter()
                            .map(|q| {
                                (
                                    &q.entry.content,
                                    q.entry.variant_selection,
                                    q.entry.fleet.as_ref(),
                                )
                            })
                            .collect::<Vec<_>>(),
                    ))
                } else {
                    None
                };
                items.push(FleetItem {
                    item_id,
                    include: q.entry.include,
                    triage: build_triage_dto(&q.triage, fp, ctx),
                    prevalence: fleet_prevalence_dto(fp, ctx),
                    variants,
                    source_repo: String::new(),
                    repo_conflict: None,
                });
            }
        }

        // Flatpaks: no fleet field, so no variant detection or prevalence.
        // Each flatpak is a standalone decision item with lifecycle badge.
        for f in &flatpaks {
            let item_id = ItemId::Flatpak {
                app_id: f.entry.app_id.clone(),
                remote: f.entry.remote.clone(),
                branch: f.entry.branch.clone(),
            };
            items.push(FleetItem {
                item_id,
                include: f.entry.include,
                triage: build_triage_dto(&f.triage, None, ctx),
                prevalence: fleet_prevalence_dto(None, ctx),
                variants: None,
                source_repo: String::new(),
                repo_conflict: None,
            });
        }

        if !items.is_empty() {
            sections.push(build_section("containers", "Containers", true, &items, ctx));
        }
    }

    // Sysctls — classified as decision items with triage tags
    {
        let sysctls = classify_sysctls(snap);
        let mut items: Vec<FleetItem> = Vec::new();

        // Group sysctl overrides by key to detect fleet variants.
        // Each key may have different runtime values across hosts.
        let mut sysctl_groups: std::collections::BTreeMap<
            &str,
            Vec<&inspectah_refine::types::RefinedSysctl>,
        > = std::collections::BTreeMap::new();
        for s in &sysctls {
            sysctl_groups
                .entry(s.entry.key.as_str())
                .or_default()
                .push(s);
        }

        for (key, group) in &sysctl_groups {
            // Pick the representative: prefer the entry with fleet data
            // showing the highest count (majority value), else first.
            let representative = group
                .iter()
                .max_by_key(|s| s.entry.fleet.as_ref().map(|f| f.count).unwrap_or(0))
                .unwrap_or(&group[0]);

            let item_id = ItemId::Sysctl {
                key: key.to_string(),
            };
            let fp = representative.entry.fleet.as_ref();

            // Build variant info using human-readable values (not content hashes).
            let variants = if group.len() >= 2 {
                Some(build_sysctl_variants(group))
            } else {
                None
            };

            items.push(FleetItem {
                item_id,
                include: representative.entry.include,
                triage: build_triage_dto(&representative.triage, fp, ctx),
                prevalence: fleet_prevalence_dto(fp, ctx),
                variants,
                source_repo: String::new(),
                repo_conflict: None,
            });
        }

        if !items.is_empty() {
            sections.push(build_section(
                "sysctls",
                "Kernel Parameters",
                true,
                &items,
                ctx,
            ));
        }
    }

    // Tuned — classified as decision items with triage tags
    {
        let tuned_selections = classify_tuned(snap);
        // Use the projected tuned_include from the snapshot (respects user ops).
        let tuned_include = snap
            .kernel_boot
            .as_ref()
            .map(|kb| kb.tuned_include)
            .unwrap_or(false);
        // Tuned is a scalar merged via most_prevalent_scalar; no per-item
        // FleetPrevalence exists. Use kernel_boot section host count as the
        // best available prevalence (all hosts with kernel_boot data contributed
        // to the tuned_active selection).
        let kb_host_count = ctx
            .fleet_meta
            .section_host_counts
            .get("kernel_boot")
            .copied()
            .unwrap_or(0) as u32;
        let items: Vec<FleetItem> = tuned_selections
            .iter()
            .map(|t| {
                let item_id = ItemId::TunedSelection {
                    profile: t.active_profile.clone(),
                };
                FleetItem {
                    item_id,
                    include: tuned_include,
                    triage: build_triage_dto(&t.triage, None, ctx),
                    prevalence: FleetPrevalenceDto {
                        count: kb_host_count,
                        total: ctx.total_hosts as u32,
                    },
                    variants: None,
                    source_repo: String::new(),
                    repo_conflict: None,
                }
            })
            .collect();

        if !items.is_empty() {
            sections.push(build_section("tuned", "Tuned Profiles", true, &items, ctx));
        }
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
            match &item.triage.zone {
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
    // NOTE: Services and containers (quadlets + flatpaks) moved to
    // build_fleet_sections() as decision items.

    // Compose files — remain as context items (read-only, no toggles).
    if let Some(ref containers) = snap.containers {
        let mut items: Vec<FleetItem> = Vec::new();

        // Group compose files by path to detect variants.
        let mut compose_groups: std::collections::BTreeMap<
            &str,
            Vec<&inspectah_core::types::containers::ComposeFile>,
        > = std::collections::BTreeMap::new();
        for c in &containers.compose_files {
            compose_groups.entry(c.path.as_str()).or_default().push(c);
        }
        for (path, group) in &compose_groups {
            let representative = group
                .iter()
                .find(|c| {
                    matches!(
                        c.variant_selection,
                        VariantSelection::Selected | VariantSelection::Only
                    )
                })
                .or_else(|| group.first());
            if let Some(c) = representative {
                let item_id = ItemId::Compose {
                    path: path.to_string(),
                };
                let fp = c.fleet.as_ref();
                // Compose files don't have a single `content` field; hash
                // the path + serialized images to produce a stable key.
                let variants = if group.len() >= 2 {
                    Some(build_compose_variants(group))
                } else {
                    None
                };
                items.push(FleetItem {
                    item_id,
                    include: fleet_include_default(fp),
                    triage: default_context_triage(fp, ctx),
                    prevalence: fleet_prevalence_dto(fp, ctx),
                    variants,
                    source_repo: String::new(),
                    repo_conflict: None,
                });
            }
        }

        if !items.is_empty() {
            sections.push(build_section(
                "compose",
                "Compose Files",
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
                include: fleet_include_default(fp),
                triage: default_context_triage(fp, ctx),
                prevalence: fleet_prevalence_dto(fp, ctx),
                variants: None,
                source_repo: String::new(),
                repo_conflict: None,
            });
        }
        for zone in &net.firewall_zones {
            let item_id = ItemId::FirewallZone {
                path: zone.path.clone(),
            };
            let fp = zone.fleet.as_ref();
            items.push(FleetItem {
                item_id,
                include: fleet_include_default(fp),
                triage: default_context_triage(fp, ctx),
                prevalence: fleet_prevalence_dto(fp, ctx),
                variants: None,
                source_repo: String::new(),
                repo_conflict: None,
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
                    include: fleet_include_default(fp),
                    triage: default_context_triage(fp, ctx),
                    prevalence: fleet_prevalence_dto(fp, ctx),
                    variants: None,
                    source_repo: String::new(),
                    repo_conflict: None,
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
                include: fleet_include_default(fp),
                triage: default_context_triage(fp, ctx),
                prevalence: fleet_prevalence_dto(fp, ctx),
                variants: None,
                source_repo: String::new(),
                repo_conflict: None,
            });
        }
        for timer in &sched.systemd_timers {
            let item_id = ItemId::SystemdTimer {
                name: timer.name.clone(),
            };
            let fp = timer.fleet.as_ref();
            items.push(FleetItem {
                item_id,
                include: fleet_include_default(fp),
                triage: default_context_triage(fp, ctx),
                prevalence: fleet_prevalence_dto(fp, ctx),
                variants: None,
                source_repo: String::new(),
                repo_conflict: None,
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
                    include: fleet_include_default(fp),
                    triage: default_context_triage(fp, ctx),
                    prevalence: fleet_prevalence_dto(fp, ctx),
                    variants: None,
                    source_repo: String::new(),
                    repo_conflict: None,
                }
            })
            .collect();
        if !items.is_empty() {
            sections.push(build_section("selinux", "SELinux", false, &items, ctx));
        }
    }

    // Kernel & Boot
    // NOTE: Sysctls moved to build_fleet_sections() as decision items.
    if let Some(ref kb) = snap.kernel_boot {
        let mut items: Vec<FleetItem> = Vec::new();
        for module in &kb.loaded_modules {
            let item_id = ItemId::KernelModule {
                name: module.name.clone(),
            };
            let fp = module.fleet.as_ref();
            items.push(FleetItem {
                item_id,
                include: fleet_include_default(fp),
                triage: default_context_triage(fp, ctx),
                prevalence: fleet_prevalence_dto(fp, ctx),
                variants: None,
                source_repo: String::new(),
                repo_conflict: None,
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
                    include: fleet_include_default(fp),
                    triage: default_context_triage(fp, ctx),
                    prevalence: fleet_prevalence_dto(fp, ctx),
                    variants: None,
                    source_repo: String::new(),
                    repo_conflict: None,
                }
            })
            .collect();
        if !items.is_empty() {
            sections.push(build_section(
                "nonrpm",
                "Non-RPM Software",
                false,
                &items,
                ctx,
            ));
        }
    }

    // NOTE: users_groups is DEFERRED — not included in fleet view
    // NOTE: version_changes comes from rpm section diffs, not a standalone section
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_triage_dto(
    tag: &TriageTag,
    fp: Option<&inspectah_core::types::fleet::FleetPrevalence>,
    ctx: &FleetContext,
) -> FleetTriageDto {
    let bucket = triage_bucket_to_string(tag.bucket());
    let reason = triage_reason_to_string(&tag.primary_reason);
    let zone = match &tag.triage {
        Triage::Fleet(ft) => {
            // Derive zone from fleet bucket for wire compat
            Some(match ft.bucket {
                inspectah_refine::types::FleetBucket::Investigate => PrevalenceZone::Divergent,
                inspectah_refine::types::FleetBucket::Divergent => PrevalenceZone::Divergent,
                inspectah_refine::types::FleetBucket::Partial => PrevalenceZone::NearConsensus,
                inspectah_refine::types::FleetBucket::Universal => PrevalenceZone::Consensus,
            })
        }
        Triage::SingleHost(_) => {
            // Fall back to zone classification from fleet prevalence
            let z = fp.map(inspectah_core::fleet::classify_zone);
            if ctx.zones_active { z } else { None }
        }
    };
    let prevalence = fp.map(|f| f.count.max(0) as u32).unwrap_or(0);
    FleetTriageDto {
        bucket,
        reason,
        zone,
        prevalence,
    }
}

fn default_context_triage(
    fp: Option<&inspectah_core::types::fleet::FleetPrevalence>,
    ctx: &FleetContext,
) -> FleetTriageDto {
    let zone = fp.map(inspectah_core::fleet::classify_zone);
    let zone = if ctx.zones_active { zone } else { None };
    FleetTriageDto {
        bucket: "site".to_string(),
        reason: "context_item".to_string(),
        zone,
        prevalence: fp.map(|f| f.count.max(0) as u32).unwrap_or(0),
    }
}

/// Returns `true` only when the item is present on every host (100% prevalence).
/// Items without fleet prevalence data default to excluded.
fn fleet_include_default(fp: Option<&inspectah_core::types::fleet::FleetPrevalence>) -> bool {
    fp.is_some_and(|f| f.count > 0 && f.count == f.total)
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

fn triage_bucket_to_string(bucket: TriageBucket) -> String {
    match bucket {
        TriageBucket::Baseline => "baseline".to_string(),
        TriageBucket::Site => "site".to_string(),
        TriageBucket::Investigate => "investigate".to_string(),
    }
}

fn triage_reason_to_string(reason: &TriageReason) -> String {
    match reason {
        TriageReason::PackageBaselineMatch => "package_baseline_match".to_string(),
        TriageReason::PackageUserAdded => "package_user_added".to_string(),
        TriageReason::PackageVersionChanged => "package_version_changed".to_string(),
        TriageReason::PackageProvenanceUnavailable => "package_provenance_unavailable".to_string(),
        TriageReason::PackageLocalInstall => "package_local_install".to_string(),
        TriageReason::PackageNoRepoSource => "package_no_repo_source".to_string(),
        TriageReason::PackageConfigCaptured => "package_config_captured".to_string(),
        TriageReason::ConfigDefault => "config_default".to_string(),
        TriageReason::ConfigBaselineMatch => "config_baseline_match".to_string(),
        TriageReason::ConfigModified => "config_modified".to_string(),
        TriageReason::ConfigUnowned => "config_unowned".to_string(),
        TriageReason::ConfigOrphaned => "config_orphaned".to_string(),
        TriageReason::SensitivePath => "sensitive_path".to_string(),
        TriageReason::Custom(s) => s.clone(),
        // All remaining reasons get a snake_case string from the variant name
        other => format!("{:?}", other).to_lowercase(),
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
                || (matches!(e.variant_selection, VariantSelection::Only) && hash == selected_hash);
            FleetVariantOption {
                hash: hash.as_str().to_string(),
                hosts: e
                    .fleet
                    .as_ref()
                    .map(|f| f.hosts.clone())
                    .unwrap_or_default(),
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

/// Build read-only `FleetVariants` for context items that have content
/// (quadlet units, service drop-ins). Each tuple is (content, variant_selection, fleet).
fn build_content_variants(
    entries: &[(&String, VariantSelection, Option<&FleetPrevalence>)],
) -> FleetVariants {
    let selected_entry = entries
        .iter()
        .find(|(_, vs, _)| matches!(vs, VariantSelection::Selected | VariantSelection::Only));
    let selected_hash = selected_entry
        .map(|(content, _, _)| ContentHash::from_content(content.as_bytes()))
        .unwrap_or_else(|| ContentHash::from_content(entries[0].0.as_bytes()));

    let options: Vec<FleetVariantOption> = entries
        .iter()
        .map(|(content, vs, fp)| {
            let hash = ContentHash::from_content(content.as_bytes());
            let is_selected = matches!(vs, VariantSelection::Selected)
                || (matches!(vs, VariantSelection::Only) && hash == selected_hash);
            FleetVariantOption {
                hash: hash.as_str().to_string(),
                hosts: fp.map(|f| f.hosts.clone()).unwrap_or_default(),
                host_count: fp.map(|f| f.count.max(0) as usize).unwrap_or(0),
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

/// Build read-only `FleetVariants` for compose files. Compose files lack a
/// single `content` field, so we hash `path + serialized images` to produce
/// a stable content identity.
fn build_compose_variants(
    entries: &[&inspectah_core::types::containers::ComposeFile],
) -> FleetVariants {
    fn compose_hash(c: &inspectah_core::types::containers::ComposeFile) -> ContentHash {
        let mut key = c.path.clone();
        for svc in &c.images {
            key.push(':');
            key.push_str(&svc.service);
            key.push('=');
            key.push_str(&svc.image);
        }
        ContentHash::from_content(key.as_bytes())
    }

    let selected_entry = entries.iter().find(|c| {
        matches!(
            c.variant_selection,
            VariantSelection::Selected | VariantSelection::Only
        )
    });
    let selected_hash = selected_entry
        .map(|c| compose_hash(c))
        .unwrap_or_else(|| compose_hash(entries[0]));

    let options: Vec<FleetVariantOption> = entries
        .iter()
        .map(|c| {
            let hash = compose_hash(c);
            let is_selected = matches!(c.variant_selection, VariantSelection::Selected)
                || (matches!(c.variant_selection, VariantSelection::Only) && hash == selected_hash);
            FleetVariantOption {
                hash: hash.as_str().to_string(),
                hosts: c
                    .fleet
                    .as_ref()
                    .map(|f| f.hosts.clone())
                    .unwrap_or_default(),
                host_count: c
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

/// Build `FleetVariants` for sysctl overrides using human-readable runtime
/// values as the variant identifier instead of content hashes. Sysctl values
/// are short scalars (e.g. "10", "4096"), so displaying the actual value is
/// more useful than an opaque hash.
fn build_sysctl_variants(entries: &[&inspectah_refine::types::RefinedSysctl]) -> FleetVariants {
    // Use runtime value as the "hash" key so the frontend shows
    // "10 (45 hosts)" vs "60 (5 hosts)" instead of content hashes.
    let selected_entry = entries
        .iter()
        .max_by_key(|s| s.entry.fleet.as_ref().map(|f| f.count).unwrap_or(0));
    let selected_key = selected_entry
        .map(|s| s.entry.runtime.clone())
        .unwrap_or_else(|| entries[0].entry.runtime.clone());

    let options: Vec<FleetVariantOption> = entries
        .iter()
        .map(|s| {
            let is_selected = s.entry.runtime == selected_key;
            FleetVariantOption {
                hash: s.entry.runtime.clone(),
                hosts: s
                    .entry
                    .fleet
                    .as_ref()
                    .map(|f| f.hosts.clone())
                    .unwrap_or_default(),
                host_count: s
                    .entry
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
        selected: selected_key,
        options,
    }
}
