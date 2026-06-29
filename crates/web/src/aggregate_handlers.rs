use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Json;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::aggregate::{AggregatePrevalence, PrevalenceZone, VariantSelection};
use inspectah_refine::classify::{
    classify_containers, classify_services, classify_sysctls, classify_tuned,
};
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{
    AggregateContext, ContentHash, ItemId, Triage, TriageBucket, TriageReason, TriageTag,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

use crate::handlers::AppState;

// ---------------------------------------------------------------------------
// DTOs — presentation-layer types for the aggregate view JSON response
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct AggregateViewResponse {
    pub generation: u64,
    pub can_undo: bool,
    pub can_redo: bool,
    pub containerfile_preview: String,
    pub session_is_sensitive: bool,
    pub summary: AggregateSummary,
    pub sections: Vec<AggregateSection>,
    pub repo_groups: Vec<crate::handlers::RepoGroupInfo>,
    pub repo_conflict_count: usize,
}

#[derive(Serialize)]
pub struct AggregateSummary {
    pub host_count: usize,
    pub actionable_variant_items: Vec<ActionableVariantItem>,
    pub informational_variant_count: usize,
    pub leaf_authority_hosts: Option<u32>,
    pub leaf_total_hosts: Option<u32>,
}

#[derive(Serialize)]
pub struct ActionableVariantItem {
    pub item_id: ItemId,
    pub section_id: String,
    pub variant_count: usize,
    pub max_host_spread: usize,
}

#[derive(Serialize)]
pub struct AggregateSection {
    pub id: String,
    pub display_name: String,
    pub is_decision_section: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zones: Option<AggregateZones>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<AggregateItem>>,
}

#[derive(Serialize)]
pub struct AggregateZones {
    pub consensus: AggregateZoneGroup,
    pub near_consensus: AggregateZoneGroup,
    pub divergent: AggregateZoneGroup,
}

#[derive(Serialize)]
pub struct AggregateZoneGroup {
    pub items: Vec<AggregateItem>,
    pub count: usize,
}

#[derive(Clone, Serialize)]
pub struct RepoSourceEntryDto {
    pub repo: String,
    pub host_count: usize,
}

#[derive(Clone, Serialize)]
pub struct AggregateItem {
    pub item_id: ItemId,
    pub include: bool,
    pub locked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attention_reason: Option<String>,
    pub triage: AggregateTriageDto,
    pub prevalence: AggregatePrevalenceDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variants: Option<AggregateVariants>,
    pub source_repo: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_conflict: Option<Vec<RepoSourceEntryDto>>,
    /// Section-specific per-item metadata, serialized as JSON.
    /// Language packages: `LanguagePackageMetadata`.
    /// Unmanaged files: `UnmanagedFileMetadata`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section_metadata: Option<serde_json::Value>,
    /// Section-specific variant payload, serialized as JSON.
    /// Only populated when the item has variants (multiple hosts
    /// with different content at the same identity key).
    /// Track C (T12) reads this field to render variant diff views.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant_payload: Option<serde_json::Value>,
}

#[derive(Clone, Serialize)]
pub struct AggregateTriageDto {
    pub bucket: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<PrevalenceZone>,
    pub prevalence: u32,
}

#[derive(Clone, Serialize)]
pub struct AggregatePrevalenceDto {
    pub count: u32,
    pub total: u32,
}

#[derive(Clone, Serialize)]
pub struct AggregateVariants {
    pub count: usize,
    pub selected: String,
    pub options: Vec<AggregateVariantOption>,
}

#[derive(Clone, Serialize)]
pub struct AggregateVariantOption {
    pub hash: String,
    pub hosts: Vec<String>,
    pub host_count: usize,
    pub selected: bool,
}

// ---------------------------------------------------------------------------
// DTOs — per-section metadata (serialized into AggregateItem.section_metadata)
// ---------------------------------------------------------------------------

/// Per-item metadata for language package aggregate rows.
/// Carried in `AggregateItem.section_metadata` as a `serde_json::Value`.
#[derive(Clone, Serialize)]
pub struct LanguagePackageMetadata {
    /// Ecosystem identifier (pip, npm, gem).
    pub ecosystem: String,
    /// Confidence level (high, medium, low).
    pub confidence: String,
    /// Number of packages in this environment.
    pub package_count: usize,
    /// Manifest file basis (e.g., "requirements.txt", "package-lock.json").
    /// Deterministic: priority order requirements.txt > package-lock.json >
    /// Gemfile.lock > first key (sorted for stability).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_basis: Option<String>,
    /// Full package list for detail pane rendering.
    pub packages: Vec<LanguagePackageDto>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct LanguagePackageDto {
    pub name: String,
    pub version: String,
}

/// Per-item metadata for unmanaged file aggregate rows.
/// Carried in `AggregateItem.section_metadata` as a `serde_json::Value`.
#[derive(Clone, Serialize)]
pub struct UnmanagedFileMetadata {
    /// Detected file type (elf_binary, jar, script, etc.).
    pub file_type: String,
    /// File size in bytes.
    pub size: u64,
    /// True if path is under /var (persistence warning).
    pub under_var: bool,
    /// Provenance detail for the detail pane.
    pub provenance: UnmanagedFileProvenanceDto,
}

#[derive(Clone, Serialize)]
pub struct UnmanagedFileProvenanceDto {
    pub last_modified: u64,
    pub uid: u32,
    pub gid: u32,
    pub permissions: String,
    pub writable_mount: bool,
    pub mutability: bool,
    pub service_working_dir: bool,
}

// ---------------------------------------------------------------------------
// DTOs — variant payloads (serialized into AggregateItem.variant_payload)
// ---------------------------------------------------------------------------

/// Variant payload for language packages — package-list diff inputs.
#[derive(Clone, Serialize, Deserialize)]
pub struct LanguagePackageVariantPayload {
    /// Per-variant package lists for diff rendering.
    pub variant_packages: Vec<VariantPackageList>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct VariantPackageList {
    pub content_hash: String,
    pub hosts: Vec<String>,
    pub host_count: usize,
    pub selected: bool,
    pub packages: Vec<LanguagePackageDto>,
}

/// Variant payload for unmanaged files — metadata comparison inputs.
#[derive(Clone, Serialize, Deserialize)]
pub struct UnmanagedFileVariantPayload {
    /// Per-variant metadata for comparison rendering.
    pub variant_metadata: Vec<VariantFileMetadata>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct VariantFileMetadata {
    pub content_hash: String,
    pub hosts: Vec<String>,
    pub host_count: usize,
    pub selected: bool,
    pub size: u64,
    pub last_modified: u64,
}

// ---------------------------------------------------------------------------
// DTOs — aggregate diff endpoint
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct AggregateDiffRequest {
    pub item_id: ItemId,
    pub base: String,
    pub target: String,
}

#[derive(Serialize)]
pub struct AggregateDiffResponse {
    pub base_hash: String,
    pub target_hash: String,
    pub base_hosts: Vec<String>,
    pub target_hosts: Vec<String>,
    pub hunks: Vec<AggregateDiffHunk>,
    pub stats: AggregateDiffStats,
}

#[derive(Serialize)]
pub struct AggregateDiffHunk {
    pub base_range: AggregateLineRange,
    pub target_range: AggregateLineRange,
    pub changes: Vec<AggregateDiffChange>,
}

#[derive(Serialize)]
pub struct AggregateLineRange {
    pub start: usize,
    pub count: usize,
}

#[derive(Serialize)]
pub struct AggregateDiffChange {
    pub kind: String,
    pub content: String,
}

#[derive(Serialize)]
pub struct AggregateDiffStats {
    pub total_changes: usize,
    pub insertions: usize,
    pub deletions: usize,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn aggregate_view(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let session = state.session.lock().unwrap();
    match session.aggregate_context() {
        Some(ctx) => {
            let response = build_aggregate_view_response(&session, ctx);
            Json(serde_json::to_value(&response).unwrap()).into_response()
        }
        None => Json(json!({"error": "not an aggregate session"})).into_response(),
    }
}

pub async fn aggregate_diff(
    State(state): State<Arc<AppState>>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let req: AggregateDiffRequest = match serde_json::from_slice(&body) {
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

    // Extract host lists from aggregate prevalence.
    let base_hosts = base_entry
        .aggregate
        .as_ref()
        .map(|f| f.hosts.clone())
        .unwrap_or_default();
    let target_hosts = target_entry
        .aggregate
        .as_ref()
        .map(|f| f.hosts.clone())
        .unwrap_or_default();

    // Compute the diff.
    let diff_result = match inspectah_refine::aggregate::diff::compute_diff(
        &base_entry.content,
        &target_entry.content,
        3,
    ) {
        Ok(r) => r,
        Err(inspectah_refine::aggregate::diff::DiffError::BinaryContent) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({"error": "binary content cannot be diffed"})),
            )
                .into_response();
        }
        Err(inspectah_refine::aggregate::diff::DiffError::InputTooLarge) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({"error": "content exceeds size limit for diffing"})),
            )
                .into_response();
        }
    };

    // Map DiffResult to response DTOs.
    let hunks: Vec<AggregateDiffHunk> = diff_result
        .hunks
        .into_iter()
        .map(|h| AggregateDiffHunk {
            base_range: AggregateLineRange {
                start: h.base_range.start,
                count: h.base_range.count,
            },
            target_range: AggregateLineRange {
                start: h.target_range.start,
                count: h.target_range.count,
            },
            changes: h
                .changes
                .into_iter()
                .map(|c| {
                    use inspectah_refine::aggregate::diff::ChangeKind;
                    AggregateDiffChange {
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

    let response = AggregateDiffResponse {
        base_hash: req.base,
        target_hash: req.target,
        base_hosts,
        target_hosts,
        hunks,
        stats: AggregateDiffStats {
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

fn build_aggregate_view_response(
    session: &RefineSession,
    ctx: &AggregateContext,
) -> AggregateViewResponse {
    let view = session.view();
    let snap = session.snapshot_projected();
    let sections = build_aggregate_sections(session, &snap, ctx);
    let summary = build_aggregate_summary(&snap, ctx, &sections);
    let repo_groups = crate::handlers::build_repo_groups(session);
    let repo_conflict_count = ctx.repo_conflicts.len();

    AggregateViewResponse {
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

fn build_aggregate_summary(
    snap: &InspectionSnapshot,
    ctx: &AggregateContext,
    sections: &[AggregateSection],
) -> AggregateSummary {
    let variant_summary = inspectah_refine::aggregate::variant_summary(snap, Some(ctx));

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

    let (leaf_authority_hosts, leaf_total_hosts) = snap
        .rpm
        .as_ref()
        .map(|r| (r.leaf_authority_hosts, r.leaf_total_hosts))
        .unwrap_or((None, None));

    AggregateSummary {
        host_count: ctx.total_hosts,
        actionable_variant_items,
        informational_variant_count,
        leaf_authority_hosts,
        leaf_total_hosts,
    }
}

/// Iterate over all items in a section regardless of zone/flat layout.
fn section_items(section: &AggregateSection) -> impl Iterator<Item = &AggregateItem> {
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

fn build_aggregate_sections(
    session: &RefineSession,
    snap: &InspectionSnapshot,
    ctx: &AggregateContext,
) -> Vec<AggregateSection> {
    let view = session.view();
    let mut sections = Vec::new();

    // --- Decision sections: packages, configs ---
    // Packages
    if snap.rpm.is_some() {
        let items: Vec<AggregateItem> = view
            .packages
            .iter()
            .map(|pkg| {
                let item_id = ItemId::Package {
                    name: pkg.entry.name.clone(),
                    arch: pkg.entry.arch.clone(),
                };
                let fp = pkg.entry.aggregate.as_ref();
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
                AggregateItem {
                    item_id,
                    include: pkg.entry.include,
                    locked: pkg.entry.locked,
                    attention_reason: None,
                    triage: build_triage_dto(&pkg.triage, fp, ctx),
                    prevalence: aggregate_prevalence_dto(fp, ctx),
                    variants: None,
                    source_repo: pkg.entry.source_repo.clone(),
                    repo_conflict,
                    section_metadata: None,
                    variant_payload: None,
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

        let items: Vec<AggregateItem> = view
            .config_files
            .iter()
            .filter(|cfg| {
                // In aggregate mode, only emit the Selected variant (or Only) for
                // each path. Alternative variants are folded into `variants`.
                use inspectah_core::types::aggregate::VariantSelection;
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
                let fp = cfg.entry.aggregate.as_ref();

                // Build variant info if this path has multiple entries.
                let path_entries = entries_by_path.get(cfg.entry.path.as_str());
                let variants = path_entries
                    .filter(|entries| entries.len() >= 2)
                    .map(|entries| build_variants(entries, cfg));

                AggregateItem {
                    item_id,
                    include: cfg.entry.include,
                    locked: cfg.entry.locked,
                    attention_reason: cfg.entry.attention_reason.clone(),
                    triage: build_triage_dto(&cfg.triage, fp, ctx),
                    prevalence: aggregate_prevalence_dto(fp, ctx),
                    variants,
                    source_repo: String::new(),
                    repo_conflict: None,
                    section_metadata: None,
                    variant_payload: None,
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

        let mut items: Vec<AggregateItem> = states
            .iter()
            .map(|s| {
                let item_id = ItemId::Service {
                    unit: s.entry.unit.clone(),
                };
                let fp = s.entry.aggregate.as_ref();
                AggregateItem {
                    item_id,
                    include: s.entry.include,
                    locked: s.entry.locked,
                    attention_reason: s.entry.attention_reason.clone(),
                    triage: build_triage_dto(&s.triage, fp, ctx),
                    prevalence: aggregate_prevalence_dto(fp, ctx),
                    variants: None,
                    source_repo: String::new(),
                    repo_conflict: None,
                    section_metadata: None,
                    variant_payload: None,
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
                let fp = d.entry.aggregate.as_ref();
                let variants = if group.len() >= 2 {
                    Some(build_content_variants(
                        &group
                            .iter()
                            .map(|d| {
                                (
                                    &d.entry.content,
                                    d.entry.variant_selection,
                                    d.entry.aggregate.as_ref(),
                                )
                            })
                            .collect::<Vec<_>>(),
                    ))
                } else {
                    None
                };
                items.push(AggregateItem {
                    item_id,
                    include: d.entry.include,
                    locked: d.entry.locked,
                    attention_reason: d.entry.attention_reason.clone(),
                    triage: build_triage_dto(&d.triage, fp, ctx),
                    prevalence: aggregate_prevalence_dto(fp, ctx),
                    variants,
                    source_repo: String::new(),
                    repo_conflict: None,
                    section_metadata: None,
                    variant_payload: None,
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

        let mut items: Vec<AggregateItem> = Vec::new();

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
                let fp = q.entry.aggregate.as_ref();
                let variants = if group.len() >= 2 {
                    Some(build_content_variants(
                        &group
                            .iter()
                            .map(|q| {
                                (
                                    &q.entry.content,
                                    q.entry.variant_selection,
                                    q.entry.aggregate.as_ref(),
                                )
                            })
                            .collect::<Vec<_>>(),
                    ))
                } else {
                    None
                };
                items.push(AggregateItem {
                    item_id,
                    include: q.entry.include,
                    locked: q.entry.locked,
                    attention_reason: None,
                    triage: build_triage_dto(&q.triage, fp, ctx),
                    prevalence: aggregate_prevalence_dto(fp, ctx),
                    variants,
                    source_repo: String::new(),
                    repo_conflict: None,
                    section_metadata: None,
                    variant_payload: None,
                });
            }
        }

        // Flatpaks: standalone decision items with aggregate prevalence from
        // the merge layer's per-entry AggregatePrevalence.
        for f in &flatpaks {
            let item_id = ItemId::Flatpak {
                app_id: f.entry.app_id.clone(),
                remote: f.entry.remote.clone(),
                branch: f.entry.branch.clone(),
            };
            items.push(AggregateItem {
                item_id,
                include: f.entry.include,
                locked: f.entry.locked,
                attention_reason: None,
                triage: build_triage_dto(&f.triage, None, ctx),
                prevalence: aggregate_prevalence_dto(f.entry.aggregate.as_ref(), ctx),
                variants: None,
                source_repo: String::new(),
                repo_conflict: None,
                section_metadata: None,
                variant_payload: None,
            });
        }

        if !items.is_empty() {
            sections.push(build_section("containers", "Containers", true, &items, ctx));
        }
    }

    // Sysctls — classified as decision items with triage tags
    {
        let sysctls = classify_sysctls(snap);
        let mut items: Vec<AggregateItem> = Vec::new();

        // Group sysctl overrides by key to detect aggregate variants.
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
            // Pick the representative: prefer the entry with aggregate data
            // showing the highest count (majority value), else first.
            let representative = group
                .iter()
                .max_by_key(|s| s.entry.aggregate.as_ref().map(|f| f.count).unwrap_or(0))
                .unwrap_or(&group[0]);

            let item_id = ItemId::Sysctl {
                key: key.to_string(),
            };
            let fp = representative.entry.aggregate.as_ref();

            // Build variant info using human-readable values (not content hashes).
            let variants = if group.len() >= 2 {
                Some(build_sysctl_variants(group))
            } else {
                None
            };

            items.push(AggregateItem {
                item_id,
                include: representative.entry.include,
                locked: representative.entry.locked,
                attention_reason: None,
                triage: build_triage_dto(&representative.triage, fp, ctx),
                prevalence: aggregate_prevalence_dto(fp, ctx),
                variants,
                source_repo: String::new(),
                repo_conflict: None,
                section_metadata: None,
                variant_payload: None,
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
        // AggregatePrevalence exists. Derive prevalence from tuned_include:
        // - tuned_include=true means the merge layer proved universality
        //   (is_scalar_universal), so count == total_hosts.
        // - tuned_include=false means the profile is NOT universal (or is
        //   a stock profile). We lack the exact winner count from the merge
        //   layer, so show 0 to avoid the false "N/N hosts" display.
        let total = ctx.total_hosts as u32;
        let tuned_prevalence_count = if tuned_include { total } else { 0 };
        let items: Vec<AggregateItem> = tuned_selections
            .iter()
            .map(|t| {
                let item_id = ItemId::TunedSelection {
                    profile: t.active_profile.clone(),
                };
                AggregateItem {
                    item_id,
                    include: tuned_include,
                    locked: false,
                    attention_reason: None,
                    triage: build_triage_dto(&t.triage, None, ctx),
                    prevalence: AggregatePrevalenceDto {
                        count: tuned_prevalence_count,
                        total,
                    },
                    variants: None,
                    source_repo: String::new(),
                    repo_conflict: None,
                    section_metadata: None,
                    variant_payload: None,
                }
            })
            .collect();

        if !items.is_empty() {
            sections.push(build_section("tuned", "Tuned Profiles", true, &items, ctx));
        }
    }

    // --- Reference sections (read-only, no toggles) ---
    // These sections come from the snapshot, not from RefinedView.
    // Items have aggregate prevalence but no include/exclude toggle.
    build_reference_sections(&mut sections, snap, ctx);

    sections
}

/// Build a `AggregateSection` from items, zone-grouping when `zones_active`.
fn build_section(
    id: &str,
    display_name: &str,
    is_decision_section: bool,
    items: &[AggregateItem],
    ctx: &AggregateContext,
) -> AggregateSection {
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

        AggregateSection {
            id: id.to_string(),
            display_name: display_name.to_string(),
            is_decision_section,
            zones: Some(AggregateZones {
                consensus: AggregateZoneGroup {
                    count: consensus.len(),
                    items: consensus.into_iter().cloned().collect(),
                },
                near_consensus: AggregateZoneGroup {
                    count: near_consensus.len(),
                    items: near_consensus.into_iter().cloned().collect(),
                },
                divergent: AggregateZoneGroup {
                    count: divergent.len(),
                    items: divergent.into_iter().cloned().collect(),
                },
            }),
            items: None,
        }
    } else {
        // Aggregate-of-2: flat list, no zone grouping
        AggregateSection {
            id: id.to_string(),
            display_name: display_name.to_string(),
            is_decision_section,
            zones: None,
            items: Some(items.to_vec()),
        }
    }
}

// ---------------------------------------------------------------------------
// Reference section builders (services, containers, etc.)
// ---------------------------------------------------------------------------

fn build_reference_sections(
    sections: &mut Vec<AggregateSection>,
    snap: &InspectionSnapshot,
    ctx: &AggregateContext,
) {
    // NOTE: Services and containers (quadlets + flatpaks) moved to
    // build_aggregate_sections() as decision items.

    // Compose files — remain as context items (read-only, no toggles).
    if let Some(ref containers) = snap.containers {
        let mut items: Vec<AggregateItem> = Vec::new();

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
                let fp = c.aggregate.as_ref();
                // Compose files don't have a single `content` field; hash
                // the path + serialized images to produce a stable key.
                let variants = if group.len() >= 2 {
                    Some(build_compose_variants(group))
                } else {
                    None
                };
                items.push(AggregateItem {
                    item_id,
                    include: c.include,
                    locked: c.locked,
                    attention_reason: None,
                    triage: default_context_triage(fp, ctx),
                    prevalence: aggregate_prevalence_dto(fp, ctx),
                    variants,
                    source_repo: String::new(),
                    repo_conflict: None,
                    section_metadata: None,
                    variant_payload: None,
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
        let mut items: Vec<AggregateItem> = Vec::new();
        for conn in &net.connections {
            let item_id = ItemId::NMConnection {
                path: conn.path.clone(),
            };
            let fp = conn.aggregate.as_ref();
            items.push(AggregateItem {
                item_id,
                include: conn.include,
                locked: conn.locked,
                attention_reason: None,
                triage: default_context_triage(fp, ctx),
                prevalence: aggregate_prevalence_dto(fp, ctx),
                variants: None,
                source_repo: String::new(),
                repo_conflict: None,
                section_metadata: None,
                variant_payload: None,
            });
        }
        for zone in &net.firewall_zones {
            let item_id = ItemId::FirewallZone {
                path: zone.path.clone(),
            };
            let fp = zone.aggregate.as_ref();
            items.push(AggregateItem {
                item_id,
                include: zone.include,
                locked: zone.locked,
                attention_reason: None,
                triage: default_context_triage(fp, ctx),
                prevalence: aggregate_prevalence_dto(fp, ctx),
                variants: None,
                source_repo: String::new(),
                repo_conflict: None,
                section_metadata: None,
                variant_payload: None,
            });
        }
        if !items.is_empty() {
            sections.push(build_section("network", "Network", false, &items, ctx));
        }
    }

    // Storage
    if let Some(ref storage) = snap.storage {
        let items: Vec<AggregateItem> = storage
            .fstab_entries
            .iter()
            .map(|entry| {
                let item_id = ItemId::Fstab {
                    mount_point: entry.mount_point.clone(),
                };
                let fp = entry.aggregate.as_ref();
                AggregateItem {
                    item_id,
                    include: entry.include,
                    locked: entry.locked,
                    attention_reason: entry.attention_reason.clone(),
                    triage: default_context_triage(fp, ctx),
                    prevalence: aggregate_prevalence_dto(fp, ctx),
                    variants: None,
                    source_repo: String::new(),
                    repo_conflict: None,
                    section_metadata: None,
                    variant_payload: None,
                }
            })
            .collect();
        if !items.is_empty() {
            sections.push(build_section("storage", "Storage", false, &items, ctx));
        }
    }

    // Scheduled tasks
    if let Some(ref sched) = snap.scheduled_tasks {
        let mut items: Vec<AggregateItem> = Vec::new();
        for cron in &sched.cron_jobs {
            let item_id = ItemId::CronJob {
                path: cron.path.clone(),
            };
            let fp = cron.aggregate.as_ref();
            items.push(AggregateItem {
                item_id,
                include: cron.include,
                locked: cron.locked,
                attention_reason: None,
                triage: default_context_triage(fp, ctx),
                prevalence: aggregate_prevalence_dto(fp, ctx),
                variants: None,
                source_repo: String::new(),
                repo_conflict: None,
                section_metadata: None,
                variant_payload: None,
            });
        }
        for timer in &sched.systemd_timers {
            let item_id = ItemId::SystemdTimer {
                name: timer.name.clone(),
            };
            let fp = timer.aggregate.as_ref();
            items.push(AggregateItem {
                item_id,
                include: timer.include,
                locked: timer.locked,
                attention_reason: None,
                triage: default_context_triage(fp, ctx),
                prevalence: aggregate_prevalence_dto(fp, ctx),
                variants: None,
                source_repo: String::new(),
                repo_conflict: None,
                section_metadata: None,
                variant_payload: None,
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
        let items: Vec<AggregateItem> = selinux
            .port_labels
            .iter()
            .map(|port| {
                let item_id = ItemId::SelinuxPort {
                    protocol_port: format!("{}:{}", port.protocol, port.port),
                };
                let fp = port.aggregate.as_ref();
                AggregateItem {
                    item_id,
                    include: port.include,
                    locked: port.locked,
                    attention_reason: None,
                    triage: default_context_triage(fp, ctx),
                    prevalence: aggregate_prevalence_dto(fp, ctx),
                    variants: None,
                    source_repo: String::new(),
                    repo_conflict: None,
                    section_metadata: None,
                    variant_payload: None,
                }
            })
            .collect();
        if !items.is_empty() {
            sections.push(build_section("selinux", "SELinux", false, &items, ctx));
        }
    }

    // Kernel & Boot
    // NOTE: Sysctls moved to build_aggregate_sections() as decision items.
    if let Some(ref kb) = snap.kernel_boot {
        let mut items: Vec<AggregateItem> = Vec::new();
        for module in &kb.loaded_modules {
            let item_id = ItemId::KernelModule {
                name: module.name.clone(),
            };
            let fp = module.aggregate.as_ref();
            items.push(AggregateItem {
                item_id,
                include: module.include,
                locked: module.locked,
                attention_reason: None,
                triage: default_context_triage(fp, ctx),
                prevalence: aggregate_prevalence_dto(fp, ctx),
                variants: None,
                source_repo: String::new(),
                repo_conflict: None,
                section_metadata: None,
                variant_payload: None,
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
        let items: Vec<AggregateItem> = nonrpm
            .items
            .iter()
            .map(|entry| {
                let item_id = ItemId::NonRpm {
                    name: entry.name.clone(),
                };
                let fp = entry.aggregate.as_ref();
                AggregateItem {
                    item_id,
                    include: entry.include,
                    locked: entry.locked,
                    attention_reason: None,
                    triage: default_context_triage(fp, ctx),
                    prevalence: aggregate_prevalence_dto(fp, ctx),
                    variants: None,
                    source_repo: String::new(),
                    repo_conflict: None,
                    section_metadata: None,
                    variant_payload: None,
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

    // Language Packages — decision items grouped by ecosystem:path
    {
        let lang_envs = classify_language_envs(snap);
        if !lang_envs.is_empty() {
            // Group entries by identity key (ecosystem:path from ItemId)
            // to detect variants — multiple content variants at the same key.
            let mut lang_by_key: HashMap<String, Vec<usize>> = HashMap::new();
            for (idx, (_, item_id)) in lang_envs.iter().enumerate() {
                let key = lang_env_group_key(item_id);
                lang_by_key.entry(key).or_default().push(idx);
            }

            let items: Vec<AggregateItem> = lang_envs
                .iter()
                .enumerate()
                .map(|(idx, (entry, item_id))| {
                    let fp = entry.aggregate.as_ref();

                    // Check if this identity key has multiple content variants.
                    let key = lang_env_group_key(item_id);
                    let (variants, variant_payload) =
                        if let Some(sibs) = lang_by_key.get(&key).filter(|s| s.len() >= 2) {
                            (
                                Some(build_language_package_variants(sibs, &lang_envs, idx)),
                                build_language_package_variant_payload(sibs, &lang_envs),
                            )
                        } else {
                            (None, None)
                        };

                    AggregateItem {
                        item_id: item_id.clone(),
                        include: entry.include,
                        locked: entry.locked,
                        attention_reason: None,
                        triage: default_context_triage(fp, ctx),
                        prevalence: aggregate_prevalence_dto(fp, ctx),
                        variants,
                        source_repo: String::new(),
                        repo_conflict: None,
                        section_metadata: build_language_package_metadata(entry),
                        variant_payload,
                    }
                })
                .collect();

            if !items.is_empty() {
                sections.push(build_section(
                    "language_packages",
                    "Language Packages",
                    true,
                    &items,
                    ctx,
                ));
            }
        }
    }

    // Unmanaged Files — decision items with aggregate prevalence
    if let Some(ref unmanaged) = snap.unmanaged_files {
        // Group by path to detect variants (same path, different content hash).
        let mut unmanaged_by_path: HashMap<&str, Vec<usize>> = HashMap::new();
        for (idx, f) in unmanaged.items.iter().enumerate() {
            unmanaged_by_path
                .entry(f.path.as_str())
                .or_default()
                .push(idx);
        }

        let items: Vec<AggregateItem> = unmanaged
            .items
            .iter()
            .enumerate()
            .map(|(idx, f)| {
                let item_id = ItemId::UnmanagedFile {
                    path: f.path.clone(),
                };
                let fp = f.aggregate.as_ref();

                let (variants, variant_payload) = if let Some(sibs) = unmanaged_by_path
                    .get(f.path.as_str())
                    .filter(|s| s.len() >= 2)
                {
                    (
                        Some(build_unmanaged_file_variants(sibs, &unmanaged.items, idx)),
                        build_unmanaged_file_variant_payload(sibs, &unmanaged.items),
                    )
                } else {
                    (None, None)
                };

                AggregateItem {
                    item_id,
                    include: f.include,
                    locked: f.locked,
                    attention_reason: None,
                    triage: default_context_triage(fp, ctx),
                    prevalence: aggregate_prevalence_dto(fp, ctx),
                    variants,
                    source_repo: String::new(),
                    repo_conflict: None,
                    section_metadata: build_unmanaged_file_metadata(f),
                    variant_payload,
                }
            })
            .collect();

        if !items.is_empty() {
            sections.push(build_section(
                "unmanaged_files",
                "Unmanaged Files",
                true,
                &items,
                ctx,
            ));
        }
    }

    // NOTE: users_groups is DEFERRED — not included in aggregate view
    // NOTE: version_changes comes from rpm section diffs, not a standalone section
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Classifies non-RPM items as language package environments for the
/// aggregate view. Groups by ecosystem + path (matching the aggregate
/// merge identity key).
fn classify_language_envs(
    snap: &InspectionSnapshot,
) -> Vec<(&inspectah_core::types::nonrpm::NonRpmItem, ItemId)> {
    let nrs = match &snap.non_rpm_software {
        Some(n) => n,
        None => return Vec::new(),
    };

    nrs.items
        .iter()
        .filter(|item| {
            matches!(
                item.method.as_str(),
                "pip list" | "pip dist-info" | "venv" | "npm lockfile" | "gem lockfile"
            )
        })
        .map(|item| {
            let ecosystem = match item.method.as_str() {
                "pip list" | "pip dist-info" | "venv" => "pip",
                "npm lockfile" => "npm",
                "gem lockfile" => "gem",
                _ => "other",
            };
            let item_id = ItemId::LanguageEnv {
                ecosystem: ecosystem.to_string(),
                path: item.path.clone(),
            };
            (item, item_id)
        })
        .collect()
}

/// Extract the grouping key from a language environment `ItemId`.
/// Must match the identity key used by the merge layer.
fn lang_env_group_key(item_id: &ItemId) -> String {
    match item_id {
        ItemId::LanguageEnv { ecosystem, path } => format!("{ecosystem}:{path}"),
        other => format!("{other:?}"),
    }
}

/// Build section metadata for a language package aggregate item.
/// Returns `Some(serde_json::Value)` with ecosystem, confidence, package
/// count, manifest basis, and full package list for the detail pane.
fn build_language_package_metadata(
    item: &inspectah_core::types::nonrpm::NonRpmItem,
) -> Option<serde_json::Value> {
    let ecosystem = match item.method.as_str() {
        "pip list" | "pip dist-info" | "venv" => "pip",
        "npm lockfile" => "npm",
        "gem lockfile" => "gem",
        _ => "other",
    };

    // Deterministic manifest_basis: priority order for well-known names,
    // then sorted-key fallback for HashMap stability, then method-based
    // fallback for detection methods like "pip dist-info" that may not
    // populate manifest_files.
    let manifest_basis = ["requirements.txt", "package-lock.json", "Gemfile.lock"]
        .iter()
        .find(|k| item.manifest_files.contains_key(**k))
        .map(|k| k.to_string())
        .or_else(|| {
            let mut keys: Vec<&String> = item.manifest_files.keys().collect();
            keys.sort();
            keys.first().map(|k| k.to_string())
        })
        .or_else(|| match item.method.as_str() {
            "pip dist-info" => Some("dist-info".to_string()),
            _ => None,
        });

    let packages: Vec<LanguagePackageDto> = item
        .packages
        .iter()
        .map(|p| LanguagePackageDto {
            name: p.name.clone(),
            version: p.version.clone(),
        })
        .collect();

    serde_json::to_value(LanguagePackageMetadata {
        ecosystem: ecosystem.to_string(),
        confidence: item.confidence.clone(),
        package_count: item.packages.len(),
        manifest_basis,
        packages,
    })
    .ok()
}

/// Build section metadata for an unmanaged file aggregate item.
/// Returns `Some(serde_json::Value)` with file type, size, under_var flag,
/// and full provenance signals for the detail pane.
fn build_unmanaged_file_metadata(
    f: &inspectah_core::types::nonrpm::UnmanagedFile,
) -> Option<serde_json::Value> {
    // Use serde serialization for file_type to get snake_case wire format
    // (e.g., "elf_binary") instead of Debug format ("ElfBinary").
    let file_type_wire = serde_json::to_value(&f.file_type)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "other".to_string());
    serde_json::to_value(UnmanagedFileMetadata {
        file_type: file_type_wire,
        size: f.size,
        under_var: f.under_var,
        provenance: UnmanagedFileProvenanceDto {
            last_modified: f.provenance.last_modified,
            uid: f.provenance.uid,
            gid: f.provenance.gid,
            permissions: f.provenance.permissions.clone(),
            writable_mount: f.provenance.writable_mount,
            mutability: f.provenance.mutable,
            service_working_dir: f.provenance.service_working_dir,
        },
    })
    .ok()
}

/// Compute a content hash for a language package entry that mirrors the
/// `AggregateMergeable::content_variant_key` implementation for `NonRpmItem`.
fn lang_package_content_hash(entry: &inspectah_core::types::nonrpm::NonRpmItem) -> String {
    use inspectah_refine::types::ContentHash;
    let mut key = String::new();
    key.push_str(&entry.method);
    key.push('\n');
    // Sort by (name, version) to match merge layer's content_variant_key.
    let mut sorted: Vec<_> = entry.packages.iter().collect();
    sorted.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
    for pkg in &sorted {
        key.push_str(&format!("{}={}\n", pkg.name, pkg.version));
    }
    ContentHash::from_content(key.as_bytes())
        .as_str()
        .to_string()
}

/// Build `AggregateVariants` for language package items that share an identity
/// key (ecosystem:path) but have different package lists.
fn build_language_package_variants(
    sibling_indices: &[usize],
    lang_envs: &[(&inspectah_core::types::nonrpm::NonRpmItem, ItemId)],
    _current_idx: usize,
) -> AggregateVariants {
    let options: Vec<AggregateVariantOption> = sibling_indices
        .iter()
        .map(|&idx| {
            let (entry, _) = &lang_envs[idx];
            let hash = lang_package_content_hash(entry);
            let fp = entry.aggregate.as_ref();
            AggregateVariantOption {
                hash,
                hosts: fp.map(|f| f.hosts.clone()).unwrap_or_default(),
                host_count: fp.map(|f| f.count.max(0) as usize).unwrap_or(0),
                selected: entry.include,
            }
        })
        .collect();

    let selected_hash = options
        .iter()
        .find(|o| o.selected)
        .map(|o| o.hash.clone())
        .unwrap_or_default();

    AggregateVariants {
        count: options.len(),
        selected: selected_hash,
        options,
    }
}

/// Build `LanguagePackageVariantPayload` for language package items that
/// share an identity key but have divergent package lists.
fn build_language_package_variant_payload(
    sibling_indices: &[usize],
    lang_envs: &[(&inspectah_core::types::nonrpm::NonRpmItem, ItemId)],
) -> Option<serde_json::Value> {
    let variant_packages: Vec<VariantPackageList> = sibling_indices
        .iter()
        .map(|&idx| {
            let (entry, _) = &lang_envs[idx];
            let content_hash = lang_package_content_hash(entry);
            let fp = entry.aggregate.as_ref();
            VariantPackageList {
                content_hash,
                hosts: fp.map(|f| f.hosts.clone()).unwrap_or_default(),
                host_count: fp.map(|f| f.count.max(0) as usize).unwrap_or(0),
                selected: entry.include,
                packages: entry
                    .packages
                    .iter()
                    .map(|p| LanguagePackageDto {
                        name: p.name.clone(),
                        version: p.version.clone(),
                    })
                    .collect(),
            }
        })
        .collect();

    serde_json::to_value(LanguagePackageVariantPayload { variant_packages }).ok()
}

/// Build `AggregateVariants` for unmanaged files that share a path but have
/// different content hashes.
fn build_unmanaged_file_variants(
    sibling_indices: &[usize],
    items: &[inspectah_core::types::nonrpm::UnmanagedFile],
    _current_idx: usize,
) -> AggregateVariants {
    let options: Vec<AggregateVariantOption> = sibling_indices
        .iter()
        .map(|&idx| {
            let f = &items[idx];
            let fp = f.aggregate.as_ref();
            AggregateVariantOption {
                hash: f.content_hash.clone(),
                hosts: fp.map(|a| a.hosts.clone()).unwrap_or_default(),
                host_count: fp.map(|a| a.count.max(0) as usize).unwrap_or(0),
                selected: matches!(
                    f.variant_selection,
                    VariantSelection::Selected | VariantSelection::Only
                ),
            }
        })
        .collect();

    let selected_hash = options
        .iter()
        .find(|o| o.selected)
        .map(|o| o.hash.clone())
        .unwrap_or_default();

    AggregateVariants {
        count: options.len(),
        selected: selected_hash,
        options,
    }
}

/// Build `UnmanagedFileVariantPayload` for unmanaged files that share a path
/// but have divergent content.
fn build_unmanaged_file_variant_payload(
    sibling_indices: &[usize],
    items: &[inspectah_core::types::nonrpm::UnmanagedFile],
) -> Option<serde_json::Value> {
    let variant_metadata: Vec<VariantFileMetadata> = sibling_indices
        .iter()
        .map(|&idx| {
            let f = &items[idx];
            let fp = f.aggregate.as_ref();
            VariantFileMetadata {
                content_hash: f.content_hash.clone(),
                hosts: fp.map(|a| a.hosts.clone()).unwrap_or_default(),
                host_count: fp.map(|a| a.count.max(0) as usize).unwrap_or(0),
                selected: matches!(
                    f.variant_selection,
                    VariantSelection::Selected | VariantSelection::Only
                ),
                size: f.size,
                last_modified: f.provenance.last_modified,
            }
        })
        .collect();

    serde_json::to_value(UnmanagedFileVariantPayload { variant_metadata }).ok()
}

fn build_triage_dto(
    tag: &TriageTag,
    fp: Option<&inspectah_core::types::aggregate::AggregatePrevalence>,
    ctx: &AggregateContext,
) -> AggregateTriageDto {
    let bucket = triage_bucket_to_string(tag.bucket());
    let reason = triage_reason_to_string(&tag.primary_reason);
    let zone = match &tag.triage {
        Triage::Aggregate(ft) => {
            // Derive zone from aggregate bucket for wire compat
            Some(match ft.bucket {
                inspectah_refine::types::AggregateBucket::Investigate => PrevalenceZone::Divergent,
                inspectah_refine::types::AggregateBucket::Divergent => PrevalenceZone::Divergent,
                inspectah_refine::types::AggregateBucket::Partial => PrevalenceZone::NearConsensus,
                inspectah_refine::types::AggregateBucket::Universal => PrevalenceZone::Consensus,
            })
        }
        Triage::SingleHost(_) => {
            // Fall back to zone classification from aggregate prevalence
            let z = fp.map(inspectah_core::aggregate::classify_zone);
            if ctx.zones_active { z } else { None }
        }
    };
    let prevalence = fp.map(|f| f.count.max(0) as u32).unwrap_or(0);
    AggregateTriageDto {
        bucket,
        reason,
        zone,
        prevalence,
    }
}

fn default_context_triage(
    fp: Option<&inspectah_core::types::aggregate::AggregatePrevalence>,
    ctx: &AggregateContext,
) -> AggregateTriageDto {
    let zone = fp.map(inspectah_core::aggregate::classify_zone);
    let zone = if ctx.zones_active { zone } else { None };
    AggregateTriageDto {
        bucket: "site".to_string(),
        reason: "context_item".to_string(),
        zone,
        prevalence: fp.map(|f| f.count.max(0) as u32).unwrap_or(0),
    }
}

fn aggregate_prevalence_dto(
    fp: Option<&inspectah_core::types::aggregate::AggregatePrevalence>,
    ctx: &AggregateContext,
) -> AggregatePrevalenceDto {
    AggregatePrevalenceDto {
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
        TriageReason::PackagePlatformPlumbing => "package_platform_plumbing".to_string(),
        TriageReason::PackageInstallerDefault => "package_installer_default".to_string(),
        TriageReason::PackageInstallerPromotedService => {
            "package_installer_promoted_service".to_string()
        }
        TriageReason::PackageInstallerPromotedConfig => {
            "package_installer_promoted_config".to_string()
        }
        TriageReason::PackageInstallerAmbiguous => "package_installer_ambiguous".to_string(),
        TriageReason::PackageInstallerEvidenceUnavailable => {
            "package_installer_evidence_unavailable".to_string()
        }
        TriageReason::Custom(s) => s.clone(),
        // All remaining reasons get a snake_case string from the variant name
        other => format!("{:?}", other).to_lowercase(),
    }
}

fn build_variants(
    entries: &[&inspectah_core::types::config::ConfigFileEntry],
    selected_cfg: &inspectah_refine::types::RefinedConfig,
) -> AggregateVariants {
    use inspectah_core::types::aggregate::VariantSelection;
    use inspectah_refine::types::ContentHash;

    let selected_hash = ContentHash::from_content(selected_cfg.entry.content.as_bytes());

    let options: Vec<AggregateVariantOption> = entries
        .iter()
        .map(|e| {
            let hash = ContentHash::from_content(e.content.as_bytes());
            let is_selected = matches!(e.variant_selection, VariantSelection::Selected)
                || (matches!(e.variant_selection, VariantSelection::Only) && hash == selected_hash);
            AggregateVariantOption {
                hash: hash.as_str().to_string(),
                hosts: e
                    .aggregate
                    .as_ref()
                    .map(|f| f.hosts.clone())
                    .unwrap_or_default(),
                host_count: e
                    .aggregate
                    .as_ref()
                    .map(|f| f.count.max(0) as usize)
                    .unwrap_or(0),
                selected: is_selected,
            }
        })
        .collect();

    AggregateVariants {
        count: entries.len(),
        selected: selected_hash.as_str().to_string(),
        options,
    }
}

/// Build read-only `AggregateVariants` for context items that have content
/// (quadlet units, service drop-ins). Each tuple is (content, variant_selection, aggregate).
fn build_content_variants(
    entries: &[(&String, VariantSelection, Option<&AggregatePrevalence>)],
) -> AggregateVariants {
    let selected_entry = entries
        .iter()
        .find(|(_, vs, _)| matches!(vs, VariantSelection::Selected | VariantSelection::Only));
    let selected_hash = selected_entry
        .map(|(content, _, _)| ContentHash::from_content(content.as_bytes()))
        .unwrap_or_else(|| ContentHash::from_content(entries[0].0.as_bytes()));

    let options: Vec<AggregateVariantOption> = entries
        .iter()
        .map(|(content, vs, fp)| {
            let hash = ContentHash::from_content(content.as_bytes());
            let is_selected = matches!(vs, VariantSelection::Selected)
                || (matches!(vs, VariantSelection::Only) && hash == selected_hash);
            AggregateVariantOption {
                hash: hash.as_str().to_string(),
                hosts: fp.map(|f| f.hosts.clone()).unwrap_or_default(),
                host_count: fp.map(|f| f.count.max(0) as usize).unwrap_or(0),
                selected: is_selected,
            }
        })
        .collect();

    AggregateVariants {
        count: entries.len(),
        selected: selected_hash.as_str().to_string(),
        options,
    }
}

/// Build read-only `AggregateVariants` for compose files. Compose files lack a
/// single `content` field, so we hash `path + serialized images` to produce
/// a stable content identity.
fn build_compose_variants(
    entries: &[&inspectah_core::types::containers::ComposeFile],
) -> AggregateVariants {
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

    let options: Vec<AggregateVariantOption> = entries
        .iter()
        .map(|c| {
            let hash = compose_hash(c);
            let is_selected = matches!(c.variant_selection, VariantSelection::Selected)
                || (matches!(c.variant_selection, VariantSelection::Only) && hash == selected_hash);
            AggregateVariantOption {
                hash: hash.as_str().to_string(),
                hosts: c
                    .aggregate
                    .as_ref()
                    .map(|f| f.hosts.clone())
                    .unwrap_or_default(),
                host_count: c
                    .aggregate
                    .as_ref()
                    .map(|f| f.count.max(0) as usize)
                    .unwrap_or(0),
                selected: is_selected,
            }
        })
        .collect();

    AggregateVariants {
        count: entries.len(),
        selected: selected_hash.as_str().to_string(),
        options,
    }
}

/// Build `AggregateVariants` for sysctl overrides using human-readable runtime
/// values as the variant identifier instead of content hashes. Sysctl values
/// are short scalars (e.g. "10", "4096"), so displaying the actual value is
/// more useful than an opaque hash.
fn build_sysctl_variants(entries: &[&inspectah_refine::types::RefinedSysctl]) -> AggregateVariants {
    // Use runtime value as the "hash" key so the frontend shows
    // "10 (45 hosts)" vs "60 (5 hosts)" instead of content hashes.
    let selected_entry = entries
        .iter()
        .max_by_key(|s| s.entry.aggregate.as_ref().map(|f| f.count).unwrap_or(0));
    let selected_key = selected_entry
        .map(|s| s.entry.runtime.clone())
        .unwrap_or_else(|| entries[0].entry.runtime.clone());

    let options: Vec<AggregateVariantOption> = entries
        .iter()
        .map(|s| {
            let is_selected = s.entry.runtime == selected_key;
            AggregateVariantOption {
                hash: s.entry.runtime.clone(),
                hosts: s
                    .entry
                    .aggregate
                    .as_ref()
                    .map(|f| f.hosts.clone())
                    .unwrap_or_default(),
                host_count: s
                    .entry
                    .aggregate
                    .as_ref()
                    .map(|f| f.count.max(0) as usize)
                    .unwrap_or(0),
                selected: is_selected,
            }
        })
        .collect();

    AggregateVariants {
        count: entries.len(),
        selected: selected_key,
        options,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashMap};

    /// Aggregate handlers must read the stored `include` value from snapshot entries,
    /// NOT recompute it from prevalence.  This test builds an NMConnection with
    /// `include: true` but non-universal prevalence (2 of 5 hosts).  The deleted
    /// `aggregate_include_default` function would have returned `false` for this
    /// prevalence — if the handler still recomputes, this test catches it.
    #[test]
    fn aggregate_handlers_use_stored_include_not_recomputed() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use inspectah_core::types::network::{NMConnection, NetworkSection};

        // Build a snapshot with one NMConnection: include=true, prevalence=2/5.
        let conn = NMConnection {
            path: "/etc/NetworkManager/test.nmconnection".to_string(),
            include: true, // stored value — should pass through
            aggregate: Some(AggregatePrevalence {
                count: 2,
                total: 5,
                hosts: vec!["host-a".into(), "host-b".into()],
                aggregate_count: None,
                aggregate_hosts: None,
            }),
            ..Default::default()
        };

        let snap = InspectionSnapshot {
            schema_version: 1,
            network: Some(NetworkSection {
                connections: vec![conn],
                ..Default::default()
            }),
            ..Default::default()
        };

        let ctx = AggregateContext {
            aggregate_meta: AggregateSnapshotMeta {
                label: "test-aggregate".to_string(),
                host_count: 5,
                hostnames: vec![
                    "host-a".into(),
                    "host-b".into(),
                    "host-c".into(),
                    "host-d".into(),
                    "host-e".into(),
                ],
                merged_at: "2026-01-01T00:00:00Z".to_string(),
                baseline_provisional: false,
                section_host_counts: BTreeMap::new(),
            },
            zones: HashMap::new(),
            total_hosts: 5,
            zones_active: false, // flat list mode for simpler assertion
            repo_conflicts: HashMap::new(),
        };

        let mut sections = Vec::new();
        build_reference_sections(&mut sections, &snap, &ctx);

        // Find the network section.
        let net_section = sections
            .iter()
            .find(|s| s.id == "network")
            .expect("network section must exist");

        // Get the first item — our NMConnection (flat list when zones_active=false).
        let items = net_section
            .items
            .as_ref()
            .expect("zones_active=false produces flat items list");
        assert_eq!(items.len(), 1);

        // The critical assertion: include must be true (stored value),
        // not false (which aggregate_include_default would have returned for 2/5).
        assert!(
            items[0].include,
            "aggregate handler must use stored include value (true), \
             not recompute from prevalence (which would be false for 2/5 hosts)"
        );
    }

    #[test]
    fn aggregate_flatpak_prevalence_plumbed() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use inspectah_core::types::containers::{ContainerSection, FlatpakApp};
        use inspectah_refine::session::RefineSession;

        let app = FlatpakApp {
            app_id: "org.gnome.Calculator".into(),
            origin: "flathub".into(),
            branch: "stable".into(),
            include: true,
            remote: "flathub".into(),
            aggregate: Some(AggregatePrevalence {
                count: 2,
                total: 3,
                hosts: vec!["host-a".into(), "host-b".into()],
                ..Default::default()
            }),
            ..Default::default()
        };

        let snap = InspectionSnapshot {
            schema_version: 1,
            containers: Some(ContainerSection {
                flatpak_apps: vec![app],
                ..Default::default()
            }),
            ..Default::default()
        };

        let session = RefineSession::new(snap.clone());

        let ctx = AggregateContext {
            aggregate_meta: AggregateSnapshotMeta {
                label: "test-aggregate".to_string(),
                host_count: 3,
                hostnames: vec!["host-a".into(), "host-b".into(), "host-c".into()],
                merged_at: "2026-01-01T00:00:00Z".to_string(),
                baseline_provisional: false,
                section_host_counts: BTreeMap::new(),
            },
            zones: HashMap::new(),
            total_hosts: 3,
            zones_active: false,
            repo_conflicts: HashMap::new(),
        };

        let sections = build_aggregate_sections(&session, &snap, &ctx);

        let container_section = sections
            .iter()
            .find(|s| s.id == "containers")
            .expect("containers section must exist");

        let items = container_section
            .items
            .as_ref()
            .expect("zones_active=false produces flat items list");

        let flatpak_item = items
            .iter()
            .find(|i| matches!(&i.item_id, ItemId::Flatpak { app_id, .. } if app_id == "org.gnome.Calculator"))
            .expect("flatpak item must exist");

        assert_eq!(
            flatpak_item.prevalence.count, 2,
            "flatpak prevalence count must come from aggregate data, not be zero"
        );
        assert_eq!(flatpak_item.prevalence.total, 3);
    }

    #[test]
    fn aggregate_tuned_prevalence_reflects_universality() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use inspectah_core::types::kernelboot::KernelBootSection;
        use inspectah_refine::session::RefineSession;

        // Tuned profile is universal (tuned_include=true) across 3 hosts.
        let snap = InspectionSnapshot {
            schema_version: 1,
            kernel_boot: Some(KernelBootSection {
                tuned_active: "my-custom-profile".into(),
                tuned_include: true,
                ..Default::default()
            }),
            ..Default::default()
        };

        let session = RefineSession::new(snap.clone());

        let ctx = AggregateContext {
            aggregate_meta: AggregateSnapshotMeta {
                label: "test-aggregate".to_string(),
                host_count: 3,
                hostnames: vec!["host-a".into(), "host-b".into(), "host-c".into()],
                merged_at: "2026-01-01T00:00:00Z".to_string(),
                baseline_provisional: false,
                section_host_counts: BTreeMap::from([("kernel_boot".into(), 3)]),
            },
            zones: HashMap::new(),
            total_hosts: 3,
            zones_active: false,
            repo_conflicts: HashMap::new(),
        };

        let sections = build_aggregate_sections(&session, &snap, &ctx);

        let tuned_section = sections
            .iter()
            .find(|s| s.id == "tuned")
            .expect("tuned section must exist");

        let items = tuned_section
            .items
            .as_ref()
            .expect("zones_active=false produces flat items list");

        assert_eq!(items.len(), 1);
        // Universal profile: count must equal total.
        assert_eq!(items[0].prevalence.count, 3);
        assert_eq!(items[0].prevalence.total, 3);

        // Now test non-universal: tuned_include=false means profile is NOT
        // on all hosts. The section_host_counts["kernel_boot"] = 3 but the
        // profile is only on 2 hosts (not universal).
        let snap_partial = InspectionSnapshot {
            schema_version: 1,
            kernel_boot: Some(KernelBootSection {
                tuned_active: "my-custom-profile".into(),
                tuned_include: false,
                ..Default::default()
            }),
            ..Default::default()
        };

        let session_partial = RefineSession::new(snap_partial.clone());

        let sections_partial = build_aggregate_sections(&session_partial, &snap_partial, &ctx);

        let tuned_section_partial = sections_partial
            .iter()
            .find(|s| s.id == "tuned")
            .expect("tuned section must exist");

        let items_partial = tuned_section_partial
            .items
            .as_ref()
            .expect("zones_active=false produces flat items list");

        assert_eq!(items_partial.len(), 1);
        // Non-universal: count must NOT equal total.
        assert!(
            items_partial[0].prevalence.count < items_partial[0].prevalence.total,
            "non-universal tuned profile must show count < total, got {}/{}",
            items_partial[0].prevalence.count,
            items_partial[0].prevalence.total,
        );
    }

    #[test]
    fn aggregate_language_packages_section_emitted() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use inspectah_core::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection};
        use inspectah_refine::session::RefineSession;

        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            non_rpm_software: Some(NonRpmSoftwareSection {
                items: vec![NonRpmItem {
                    name: "myapp-venv".to_string(),
                    path: "/opt/myapp/venv".to_string(),
                    method: "venv".to_string(),
                    confidence: "high".to_string(),
                    include: true,
                    aggregate: Some(AggregatePrevalence {
                        count: 3,
                        total: 3,
                        hosts: vec!["a".into(), "b".into(), "c".into()],
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                env_files: vec![],
            }),
            ..Default::default()
        };

        let ctx = AggregateContext {
            aggregate_meta: AggregateSnapshotMeta {
                label: "test".to_string(),
                host_count: 3,
                hostnames: vec!["a".into(), "b".into(), "c".into()],
                merged_at: "2026-01-01T00:00:00Z".to_string(),
                baseline_provisional: false,
                section_host_counts: BTreeMap::new(),
            },
            zones: HashMap::new(),
            total_hosts: 3,
            zones_active: false,
            repo_conflicts: HashMap::new(),
        };

        let session = RefineSession::new(snap.clone());
        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let lang_section = sections.iter().find(|s| s.id == "language_packages");

        assert!(
            lang_section.is_some(),
            "language_packages section should be present"
        );
        let section = lang_section.unwrap();
        assert!(section.is_decision_section);
    }

    #[test]
    fn aggregate_language_packages_prevalence_plumbed() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use inspectah_core::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection};
        use inspectah_refine::session::RefineSession;

        // 3/3 hosts — universal, include: true
        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            non_rpm_software: Some(NonRpmSoftwareSection {
                items: vec![NonRpmItem {
                    name: "myapp-venv".to_string(),
                    path: "/opt/myapp/venv".to_string(),
                    method: "venv".to_string(),
                    confidence: "high".to_string(),
                    include: true,
                    aggregate: Some(AggregatePrevalence {
                        count: 3,
                        total: 3,
                        hosts: vec!["a".into(), "b".into(), "c".into()],
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                env_files: vec![],
            }),
            ..Default::default()
        };

        let ctx = AggregateContext {
            aggregate_meta: AggregateSnapshotMeta {
                label: "test".to_string(),
                host_count: 3,
                hostnames: vec!["a".into(), "b".into(), "c".into()],
                merged_at: "2026-01-01T00:00:00Z".to_string(),
                baseline_provisional: false,
                section_host_counts: BTreeMap::new(),
            },
            zones: HashMap::new(),
            total_hosts: 3,
            zones_active: false,
            repo_conflicts: HashMap::new(),
        };

        let session = RefineSession::new(snap.clone());
        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let section = sections
            .iter()
            .find(|s| s.id == "language_packages")
            .expect("language_packages section must exist");
        let items = section
            .items
            .as_ref()
            .expect("zones_active=false produces flat items list");
        assert_eq!(items[0].prevalence.count, 3);
        assert_eq!(items[0].prevalence.total, 3);
        assert!(
            items[0].include,
            "100% prevalence should preserve stored include=true"
        );
    }

    #[test]
    fn aggregate_language_packages_partial_prevalence() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use inspectah_core::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection};
        use inspectah_refine::session::RefineSession;

        // 1/3 hosts — divergent, include: false (stored value)
        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            non_rpm_software: Some(NonRpmSoftwareSection {
                items: vec![NonRpmItem {
                    name: "stale-venv".to_string(),
                    path: "/opt/stale/venv".to_string(),
                    method: "pip list".to_string(),
                    confidence: "high".to_string(),
                    include: false,
                    aggregate: Some(AggregatePrevalence {
                        count: 1,
                        total: 3,
                        hosts: vec!["a".into()],
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                env_files: vec![],
            }),
            ..Default::default()
        };

        let ctx = AggregateContext {
            aggregate_meta: AggregateSnapshotMeta {
                label: "test".to_string(),
                host_count: 3,
                hostnames: vec!["a".into(), "b".into(), "c".into()],
                merged_at: "2026-01-01T00:00:00Z".to_string(),
                baseline_provisional: false,
                section_host_counts: BTreeMap::new(),
            },
            zones: HashMap::new(),
            total_hosts: 3,
            zones_active: false,
            repo_conflicts: HashMap::new(),
        };

        let session = RefineSession::new(snap.clone());
        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let section = sections
            .iter()
            .find(|s| s.id == "language_packages")
            .expect("language_packages section must exist");
        let items = section
            .items
            .as_ref()
            .expect("zones_active=false produces flat items list");
        assert_eq!(items[0].prevalence.count, 1);
        assert_eq!(items[0].prevalence.total, 3);
        assert!(
            !items[0].include,
            "partial prevalence should preserve stored include=false"
        );
    }

    #[test]
    fn aggregate_language_packages_filters_non_lang_methods() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use inspectah_core::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection};
        use inspectah_refine::session::RefineSession;

        // A binary-detection item should NOT appear in language_packages
        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            non_rpm_software: Some(NonRpmSoftwareSection {
                items: vec![NonRpmItem {
                    name: "custom-binary".to_string(),
                    path: "/usr/local/bin/custom".to_string(),
                    method: "binary".to_string(),
                    confidence: "medium".to_string(),
                    include: true,
                    aggregate: Some(AggregatePrevalence {
                        count: 3,
                        total: 3,
                        hosts: vec!["a".into(), "b".into(), "c".into()],
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                env_files: vec![],
            }),
            ..Default::default()
        };

        let ctx = AggregateContext {
            aggregate_meta: AggregateSnapshotMeta {
                label: "test".to_string(),
                host_count: 3,
                hostnames: vec!["a".into(), "b".into(), "c".into()],
                merged_at: "2026-01-01T00:00:00Z".to_string(),
                baseline_provisional: false,
                section_host_counts: BTreeMap::new(),
            },
            zones: HashMap::new(),
            total_hosts: 3,
            zones_active: false,
            repo_conflicts: HashMap::new(),
        };

        let session = RefineSession::new(snap.clone());
        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let lang_section = sections.iter().find(|s| s.id == "language_packages");

        assert!(
            lang_section.is_none(),
            "binary-detection items should not produce a language_packages section"
        );
    }

    #[test]
    fn aggregate_unmanaged_files_section_emitted() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use inspectah_core::types::nonrpm::{UnmanagedFile, UnmanagedFileSection};

        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            unmanaged_files: Some(UnmanagedFileSection {
                items: vec![UnmanagedFile {
                    path: "/opt/splunk/bin/splunkd".to_string(),
                    size: 52_000_000,
                    include: true,
                    aggregate: Some(AggregatePrevalence {
                        count: 2,
                        total: 3,
                        hosts: vec!["a".into(), "b".into()],
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                total_size: 52_000_000,
                total_count: 1,
            }),
            ..Default::default()
        };

        let ctx = AggregateContext {
            aggregate_meta: AggregateSnapshotMeta {
                label: "test".to_string(),
                host_count: 3,
                hostnames: vec!["a".into(), "b".into(), "c".into()],
                merged_at: "2026-01-01T00:00:00Z".to_string(),
                baseline_provisional: false,
                section_host_counts: BTreeMap::new(),
            },
            zones: HashMap::new(),
            zones_active: true,
            total_hosts: 3,
            repo_conflicts: HashMap::new(),
        };

        let session = RefineSession::new(snap.clone());
        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let unmanaged_section = sections.iter().find(|s| s.id == "unmanaged_files");

        assert!(
            unmanaged_section.is_some(),
            "unmanaged_files section should be present"
        );
        let section = unmanaged_section.unwrap();
        assert!(section.is_decision_section);
    }

    #[test]
    fn aggregate_unmanaged_files_100_pct_includes() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use inspectah_core::types::nonrpm::{UnmanagedFile, UnmanagedFileSection};

        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            unmanaged_files: Some(UnmanagedFileSection {
                items: vec![UnmanagedFile {
                    path: "/opt/app/server".to_string(),
                    size: 10_000,
                    include: true,
                    aggregate: Some(AggregatePrevalence {
                        count: 2,
                        total: 2,
                        hosts: vec!["a".into(), "b".into()],
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                total_size: 10_000,
                total_count: 1,
            }),
            ..Default::default()
        };

        let ctx = AggregateContext {
            aggregate_meta: AggregateSnapshotMeta {
                label: "test".to_string(),
                host_count: 2,
                hostnames: vec!["a".into(), "b".into()],
                merged_at: "2026-01-01T00:00:00Z".to_string(),
                baseline_provisional: false,
                section_host_counts: BTreeMap::new(),
            },
            zones: HashMap::new(),
            zones_active: true,
            total_hosts: 2,
            repo_conflicts: HashMap::new(),
        };

        let session = RefineSession::new(snap.clone());
        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let section = sections
            .iter()
            .find(|s| s.id == "unmanaged_files")
            .expect("unmanaged_files section should exist");

        // Items are zone-sorted; collect all items from zones
        let items: Vec<&AggregateItem> = if let Some(ref zones) = section.zones {
            zones
                .consensus
                .items
                .iter()
                .chain(zones.near_consensus.items.iter())
                .chain(zones.divergent.items.iter())
                .collect()
        } else {
            section
                .items
                .as_ref()
                .map_or(vec![], |v| v.iter().collect())
        };

        assert!(!items.is_empty(), "should have at least one item");
        assert!(items[0].include, "100% prevalence should be included");
    }

    #[test]
    fn aggregate_unmanaged_files_partial_excludes() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use inspectah_core::types::nonrpm::{UnmanagedFile, UnmanagedFileSection};

        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            unmanaged_files: Some(UnmanagedFileSection {
                items: vec![UnmanagedFile {
                    path: "/opt/app/server".to_string(),
                    size: 10_000,
                    include: false,
                    aggregate: Some(AggregatePrevalence {
                        count: 1,
                        total: 3,
                        hosts: vec!["a".into()],
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                total_size: 10_000,
                total_count: 1,
            }),
            ..Default::default()
        };

        let ctx = AggregateContext {
            aggregate_meta: AggregateSnapshotMeta {
                label: "test".to_string(),
                host_count: 3,
                hostnames: vec!["a".into(), "b".into(), "c".into()],
                merged_at: "2026-01-01T00:00:00Z".to_string(),
                baseline_provisional: false,
                section_host_counts: BTreeMap::new(),
            },
            zones: HashMap::new(),
            zones_active: true,
            total_hosts: 3,
            repo_conflicts: HashMap::new(),
        };

        let session = RefineSession::new(snap.clone());
        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let section = sections
            .iter()
            .find(|s| s.id == "unmanaged_files")
            .expect("unmanaged_files section should exist");

        // Items are zone-sorted; collect all items from zones
        let items: Vec<&AggregateItem> = if let Some(ref zones) = section.zones {
            zones
                .consensus
                .items
                .iter()
                .chain(zones.near_consensus.items.iter())
                .chain(zones.divergent.items.iter())
                .collect()
        } else {
            section
                .items
                .as_ref()
                .map_or(vec![], |v| v.iter().collect())
        };

        assert!(!items.is_empty(), "should have at least one item");
        assert!(!items[0].include, "partial prevalence should be excluded");
    }

    #[test]
    fn language_packages_section_metadata_populated() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use inspectah_core::types::nonrpm::{LanguagePackage, NonRpmItem, NonRpmSoftwareSection};
        use inspectah_refine::session::RefineSession;

        let mut manifest_files = std::collections::HashMap::new();
        manifest_files.insert("requirements.txt".to_string(), String::new());

        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            non_rpm_software: Some(NonRpmSoftwareSection {
                items: vec![NonRpmItem {
                    name: "myapp-venv".to_string(),
                    path: "/opt/myapp/venv".to_string(),
                    method: "venv".to_string(),
                    confidence: "high".to_string(),
                    include: true,
                    packages: vec![
                        LanguagePackage {
                            name: "flask".to_string(),
                            version: "2.3.0".to_string(),
                        },
                        LanguagePackage {
                            name: "requests".to_string(),
                            version: "2.31.0".to_string(),
                        },
                    ],
                    manifest_files,
                    aggregate: Some(AggregatePrevalence {
                        count: 3,
                        total: 3,
                        hosts: vec!["a".into(), "b".into(), "c".into()],
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                env_files: vec![],
            }),
            ..Default::default()
        };

        let ctx = AggregateContext {
            aggregate_meta: AggregateSnapshotMeta {
                label: "test".to_string(),
                host_count: 3,
                hostnames: vec!["a".into(), "b".into(), "c".into()],
                merged_at: "2026-01-01T00:00:00Z".to_string(),
                baseline_provisional: false,
                section_host_counts: BTreeMap::new(),
            },
            zones: HashMap::new(),
            total_hosts: 3,
            zones_active: false,
            repo_conflicts: HashMap::new(),
        };

        let session = RefineSession::new(snap.clone());
        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let section = sections
            .iter()
            .find(|s| s.id == "language_packages")
            .expect("language_packages section must exist");
        let items = section
            .items
            .as_ref()
            .expect("zones_active=false produces flat items list");

        let meta = items[0]
            .section_metadata
            .as_ref()
            .expect("section_metadata should be populated for language packages");

        assert_eq!(meta["ecosystem"], "pip");
        assert_eq!(meta["confidence"], "high");
        assert_eq!(meta["package_count"], 2);
        assert_eq!(meta["manifest_basis"], "requirements.txt");

        let packages = meta["packages"]
            .as_array()
            .expect("packages should be array");
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0]["name"], "flask");
        assert_eq!(packages[0]["version"], "2.3.0");
    }

    #[test]
    fn unmanaged_files_section_metadata_populated() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use inspectah_core::types::nonrpm::{
            FileType, ProvenanceSignals, UnmanagedFile, UnmanagedFileSection,
        };
        use inspectah_refine::session::RefineSession;

        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            unmanaged_files: Some(UnmanagedFileSection {
                items: vec![UnmanagedFile {
                    path: "/var/lib/splunk/data".to_string(),
                    size: 52_000_000,
                    file_type: FileType::ElfBinary,
                    under_var: true,
                    provenance: ProvenanceSignals {
                        file_type: FileType::ElfBinary,
                        last_modified: 1700000000,
                        uid: 1001,
                        gid: 1001,
                        permissions: "0755".to_string(),
                        mutable: true,
                        writable_mount: true,
                        service_working_dir: false,
                    },
                    include: true,
                    aggregate: Some(AggregatePrevalence {
                        count: 3,
                        total: 3,
                        hosts: vec!["a".into(), "b".into(), "c".into()],
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let ctx = AggregateContext {
            aggregate_meta: AggregateSnapshotMeta {
                label: "test".to_string(),
                host_count: 3,
                hostnames: vec!["a".into(), "b".into(), "c".into()],
                merged_at: "2026-01-01T00:00:00Z".to_string(),
                baseline_provisional: false,
                section_host_counts: BTreeMap::new(),
            },
            zones: HashMap::new(),
            total_hosts: 3,
            zones_active: true,
            repo_conflicts: HashMap::new(),
        };

        let session = RefineSession::new(snap.clone());
        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let section = sections
            .iter()
            .find(|s| s.id == "unmanaged_files")
            .expect("unmanaged_files section must exist");

        // zones_active=true, so items are in zone groups
        let items: Vec<&AggregateItem> = if let Some(ref zones) = section.zones {
            zones
                .consensus
                .items
                .iter()
                .chain(zones.near_consensus.items.iter())
                .chain(zones.divergent.items.iter())
                .collect()
        } else {
            section
                .items
                .as_ref()
                .map_or(vec![], |v| v.iter().collect())
        };

        let meta = items[0]
            .section_metadata
            .as_ref()
            .expect("section_metadata should be populated for unmanaged files");

        assert_eq!(meta["file_type"], "elf_binary");
        assert_eq!(meta["size"], 52_000_000);
        assert_eq!(meta["under_var"], true);

        let prov = &meta["provenance"];
        assert_eq!(prov["last_modified"], 1700000000_u64);
        assert_eq!(prov["uid"], 1001);
        assert_eq!(prov["gid"], 1001);
        assert_eq!(prov["permissions"], "0755");
        assert_eq!(prov["writable_mount"], true);
        assert_eq!(prov["mutability"], true);
        assert_eq!(prov["service_working_dir"], false);
    }

    #[test]
    fn non_metadata_sections_have_no_section_metadata() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use inspectah_core::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection};
        use inspectah_refine::session::RefineSession;

        // A binary-detection item goes to the nonrpm section, not language_packages
        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            non_rpm_software: Some(NonRpmSoftwareSection {
                items: vec![NonRpmItem {
                    name: "custom-tool".to_string(),
                    path: "/opt/tool".to_string(),
                    method: "binary-detection".to_string(),
                    confidence: "medium".to_string(),
                    include: true,
                    aggregate: Some(AggregatePrevalence {
                        count: 2,
                        total: 2,
                        hosts: vec!["a".into(), "b".into()],
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                env_files: vec![],
            }),
            ..Default::default()
        };

        let ctx = AggregateContext {
            aggregate_meta: AggregateSnapshotMeta {
                label: "test".to_string(),
                host_count: 2,
                hostnames: vec!["a".into(), "b".into()],
                merged_at: "2026-01-01T00:00:00Z".to_string(),
                baseline_provisional: false,
                section_host_counts: BTreeMap::new(),
            },
            zones: HashMap::new(),
            total_hosts: 2,
            zones_active: false,
            repo_conflicts: HashMap::new(),
        };

        let session = RefineSession::new(snap.clone());
        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let nonrpm_section = sections
            .iter()
            .find(|s| s.id == "nonrpm")
            .expect("nonrpm section must exist");
        let items = nonrpm_section
            .items
            .as_ref()
            .expect("zones_active=false produces flat items list");

        assert!(
            items[0].section_metadata.is_none(),
            "non-RPM software section should not have section_metadata"
        );
        assert!(
            items[0].variant_payload.is_none(),
            "non-RPM software section should not have variant_payload"
        );
    }

    #[test]
    fn language_package_variant_payload_populated() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use inspectah_core::types::nonrpm::{LanguagePackage, NonRpmItem, NonRpmSoftwareSection};

        // Two hosts with the same pip:/opt/app/venv but different package lists.
        // The merge layer produces two NonRpmItem entries with the same
        // identity key but different content — simulated here directly.
        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            non_rpm_software: Some(NonRpmSoftwareSection {
                items: vec![
                    NonRpmItem {
                        name: "app-venv".to_string(),
                        path: "/opt/app/venv".to_string(),
                        method: "venv".to_string(),
                        confidence: "high".to_string(),
                        include: true,
                        packages: vec![
                            LanguagePackage {
                                name: "flask".to_string(),
                                version: "2.3.0".to_string(),
                            },
                            LanguagePackage {
                                name: "requests".to_string(),
                                version: "2.31.0".to_string(),
                            },
                        ],
                        aggregate: Some(AggregatePrevalence {
                            count: 2,
                            total: 3,
                            hosts: vec!["host-a".into(), "host-b".into()],
                            aggregate_count: Some(3),
                            aggregate_hosts: Some(vec![
                                "host-a".into(),
                                "host-b".into(),
                                "host-c".into(),
                            ]),
                        }),
                        ..Default::default()
                    },
                    NonRpmItem {
                        name: "app-venv".to_string(),
                        path: "/opt/app/venv".to_string(),
                        method: "venv".to_string(),
                        confidence: "high".to_string(),
                        include: true,
                        packages: vec![
                            LanguagePackage {
                                name: "flask".to_string(),
                                version: "2.2.5".to_string(),
                            },
                            LanguagePackage {
                                name: "requests".to_string(),
                                version: "2.28.0".to_string(),
                            },
                        ],
                        aggregate: Some(AggregatePrevalence {
                            count: 1,
                            total: 3,
                            hosts: vec!["host-c".into()],
                            aggregate_count: Some(3),
                            aggregate_hosts: Some(vec![
                                "host-a".into(),
                                "host-b".into(),
                                "host-c".into(),
                            ]),
                        }),
                        ..Default::default()
                    },
                ],
                env_files: vec![],
            }),
            ..Default::default()
        };

        let ctx = AggregateContext {
            aggregate_meta: AggregateSnapshotMeta {
                label: "test".to_string(),
                host_count: 3,
                hostnames: vec!["host-a".into(), "host-b".into(), "host-c".into()],
                merged_at: "2026-01-01T00:00:00Z".to_string(),
                baseline_provisional: false,
                section_host_counts: BTreeMap::new(),
            },
            zones: HashMap::new(),
            total_hosts: 3,
            zones_active: false,
            repo_conflicts: HashMap::new(),
        };

        let session = RefineSession::new(snap.clone());
        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let lang_section = sections
            .iter()
            .find(|s| s.id == "language_packages")
            .expect("language_packages section must exist");

        let items = lang_section
            .items
            .as_ref()
            .expect("zones_active=false produces flat items list");

        // Both items should have variant_payload populated.
        for item in items {
            assert!(
                item.variant_payload.is_some(),
                "variant_payload should be populated for divergent language package items"
            );
            assert!(
                item.variants.is_some(),
                "variants should be populated for divergent language package items"
            );
        }

        // Verify the payload structure: variant_packages with 2 entries.
        let payload: LanguagePackageVariantPayload =
            serde_json::from_value(items[0].variant_payload.clone().unwrap())
                .expect("variant_payload should deserialize to LanguagePackageVariantPayload");
        assert_eq!(
            payload.variant_packages.len(),
            2,
            "should have 2 variant package lists"
        );

        // Each variant should have a distinct content_hash.
        let hashes: Vec<&str> = payload
            .variant_packages
            .iter()
            .map(|v| v.content_hash.as_str())
            .collect();
        assert_ne!(
            hashes[0], hashes[1],
            "variant package lists with different packages must have different content hashes"
        );

        // Verify host counts are plumbed through.
        assert_eq!(payload.variant_packages[0].host_count, 2);
        assert_eq!(payload.variant_packages[1].host_count, 1);
        assert_eq!(payload.variant_packages[0].packages.len(), 2);
        assert_eq!(payload.variant_packages[1].packages.len(), 2);
    }

    #[test]
    fn language_package_no_variant_payload_when_single() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use inspectah_core::types::nonrpm::{LanguagePackage, NonRpmItem, NonRpmSoftwareSection};

        // Single entry — no variants, variant_payload should be None.
        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            non_rpm_software: Some(NonRpmSoftwareSection {
                items: vec![NonRpmItem {
                    name: "app-venv".to_string(),
                    path: "/opt/app/venv".to_string(),
                    method: "venv".to_string(),
                    confidence: "high".to_string(),
                    include: true,
                    packages: vec![LanguagePackage {
                        name: "flask".to_string(),
                        version: "2.3.0".to_string(),
                    }],
                    aggregate: Some(AggregatePrevalence {
                        count: 3,
                        total: 3,
                        hosts: vec!["a".into(), "b".into(), "c".into()],
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                env_files: vec![],
            }),
            ..Default::default()
        };

        let ctx = AggregateContext {
            aggregate_meta: AggregateSnapshotMeta {
                label: "test".to_string(),
                host_count: 3,
                hostnames: vec!["a".into(), "b".into(), "c".into()],
                merged_at: "2026-01-01T00:00:00Z".to_string(),
                baseline_provisional: false,
                section_host_counts: BTreeMap::new(),
            },
            zones: HashMap::new(),
            total_hosts: 3,
            zones_active: false,
            repo_conflicts: HashMap::new(),
        };

        let session = RefineSession::new(snap.clone());
        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let lang_section = sections
            .iter()
            .find(|s| s.id == "language_packages")
            .expect("language_packages section must exist");
        let items = lang_section.items.as_ref().expect("flat items list");

        assert!(
            items[0].variant_payload.is_none(),
            "single language package should not have variant_payload"
        );
        assert!(
            items[0].variants.is_none(),
            "single language package should not have variants"
        );
    }

    #[test]
    fn unmanaged_file_variant_payload_populated() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use inspectah_core::types::nonrpm::{UnmanagedFile, UnmanagedFileSection};

        // Two hosts with /opt/app/server at the same path but different content.
        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            unmanaged_files: Some(UnmanagedFileSection {
                items: vec![
                    UnmanagedFile {
                        path: "/opt/app/server".to_string(),
                        size: 1_000_000,
                        include: true,
                        content_hash: "aaa111".to_string(),
                        aggregate: Some(AggregatePrevalence {
                            count: 2,
                            total: 3,
                            hosts: vec!["host-a".into(), "host-b".into()],
                            aggregate_count: Some(3),
                            aggregate_hosts: Some(vec![
                                "host-a".into(),
                                "host-b".into(),
                                "host-c".into(),
                            ]),
                        }),
                        ..Default::default()
                    },
                    UnmanagedFile {
                        path: "/opt/app/server".to_string(),
                        size: 1_200_000,
                        include: true,
                        content_hash: "bbb222".to_string(),
                        aggregate: Some(AggregatePrevalence {
                            count: 1,
                            total: 3,
                            hosts: vec!["host-c".into()],
                            aggregate_count: Some(3),
                            aggregate_hosts: Some(vec![
                                "host-a".into(),
                                "host-b".into(),
                                "host-c".into(),
                            ]),
                        }),
                        ..Default::default()
                    },
                ],
                total_size: 2_200_000,
                total_count: 2,
            }),
            ..Default::default()
        };

        let ctx = AggregateContext {
            aggregate_meta: AggregateSnapshotMeta {
                label: "test".to_string(),
                host_count: 3,
                hostnames: vec!["host-a".into(), "host-b".into(), "host-c".into()],
                merged_at: "2026-01-01T00:00:00Z".to_string(),
                baseline_provisional: false,
                section_host_counts: BTreeMap::new(),
            },
            zones: HashMap::new(),
            total_hosts: 3,
            zones_active: false,
            repo_conflicts: HashMap::new(),
        };

        let session = RefineSession::new(snap.clone());
        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let unmanaged_section = sections
            .iter()
            .find(|s| s.id == "unmanaged_files")
            .expect("unmanaged_files section must exist");

        let items = unmanaged_section
            .items
            .as_ref()
            .expect("zones_active=false produces flat items list");

        // Both items should have variant_payload populated.
        for item in items {
            assert!(
                item.variant_payload.is_some(),
                "variant_payload should be populated for divergent unmanaged file items"
            );
            assert!(
                item.variants.is_some(),
                "variants should be populated for divergent unmanaged file items"
            );
        }

        // Verify the payload structure: variant_metadata with 2 entries.
        let payload: UnmanagedFileVariantPayload =
            serde_json::from_value(items[0].variant_payload.clone().unwrap())
                .expect("variant_payload should deserialize to UnmanagedFileVariantPayload");
        assert_eq!(
            payload.variant_metadata.len(),
            2,
            "should have 2 variant metadata entries"
        );

        // Check distinct content hashes.
        assert_eq!(payload.variant_metadata[0].content_hash, "aaa111");
        assert_eq!(payload.variant_metadata[1].content_hash, "bbb222");

        // Check size and last_modified are carried through.
        assert_eq!(payload.variant_metadata[0].size, 1_000_000);
        assert_eq!(payload.variant_metadata[1].size, 1_200_000);

        // Check host counts.
        assert_eq!(payload.variant_metadata[0].host_count, 2);
        assert_eq!(payload.variant_metadata[1].host_count, 1);
    }

    #[test]
    fn unmanaged_file_no_variant_payload_when_single() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use inspectah_core::types::nonrpm::{UnmanagedFile, UnmanagedFileSection};

        // Single file — no variants.
        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            unmanaged_files: Some(UnmanagedFileSection {
                items: vec![UnmanagedFile {
                    path: "/opt/app/server".to_string(),
                    size: 1_000_000,
                    include: true,
                    content_hash: "aaa111".to_string(),
                    aggregate: Some(AggregatePrevalence {
                        count: 3,
                        total: 3,
                        hosts: vec!["a".into(), "b".into(), "c".into()],
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                total_size: 1_000_000,
                total_count: 1,
            }),
            ..Default::default()
        };

        let ctx = AggregateContext {
            aggregate_meta: AggregateSnapshotMeta {
                label: "test".to_string(),
                host_count: 3,
                hostnames: vec!["a".into(), "b".into(), "c".into()],
                merged_at: "2026-01-01T00:00:00Z".to_string(),
                baseline_provisional: false,
                section_host_counts: BTreeMap::new(),
            },
            zones: HashMap::new(),
            total_hosts: 3,
            zones_active: false,
            repo_conflicts: HashMap::new(),
        };

        let session = RefineSession::new(snap.clone());
        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let unmanaged_section = sections
            .iter()
            .find(|s| s.id == "unmanaged_files")
            .expect("unmanaged_files section must exist");
        let items = unmanaged_section.items.as_ref().expect("flat items list");

        assert!(
            items[0].variant_payload.is_none(),
            "single unmanaged file should not have variant_payload"
        );
        assert!(
            items[0].variants.is_none(),
            "single unmanaged file should not have variants"
        );
    }

    #[test]
    fn pip_dist_info_manifest_basis_synthesized() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use inspectah_core::types::nonrpm::{LanguagePackage, NonRpmItem, NonRpmSoftwareSection};
        use inspectah_refine::session::RefineSession;

        // pip dist-info entry with NO manifest_files — manifest_basis
        // should fall back to "dist-info" from the method name.
        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            non_rpm_software: Some(NonRpmSoftwareSection {
                items: vec![NonRpmItem {
                    name: "system-pip".to_string(),
                    path: "/usr/lib/python3.9/site-packages".to_string(),
                    method: "pip dist-info".to_string(),
                    confidence: "medium".to_string(),
                    include: true,
                    packages: vec![LanguagePackage {
                        name: "setuptools".to_string(),
                        version: "53.0.0".to_string(),
                    }],
                    manifest_files: std::collections::HashMap::new(),
                    aggregate: Some(AggregatePrevalence {
                        count: 3,
                        total: 3,
                        hosts: vec!["a".into(), "b".into(), "c".into()],
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                env_files: vec![],
            }),
            ..Default::default()
        };

        let ctx = AggregateContext {
            aggregate_meta: AggregateSnapshotMeta {
                label: "test".to_string(),
                host_count: 3,
                hostnames: vec!["a".into(), "b".into(), "c".into()],
                merged_at: "2026-01-01T00:00:00Z".to_string(),
                baseline_provisional: false,
                section_host_counts: BTreeMap::new(),
            },
            zones: HashMap::new(),
            total_hosts: 3,
            zones_active: false,
            repo_conflicts: HashMap::new(),
        };

        let session = RefineSession::new(snap.clone());
        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let section = sections
            .iter()
            .find(|s| s.id == "language_packages")
            .expect("language_packages section must exist");
        let items = section
            .items
            .as_ref()
            .expect("zones_active=false produces flat items list");

        let meta = items[0]
            .section_metadata
            .as_ref()
            .expect("section_metadata should be populated");

        assert_eq!(
            meta["manifest_basis"], "dist-info",
            "pip dist-info items with empty manifest_files should get manifest_basis='dist-info'"
        );
    }
}
