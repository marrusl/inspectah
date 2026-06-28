use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use inspectah_core::aggregate::classify_zone;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::aggregate::PrevalenceZone;
use inspectah_core::types::redaction::RedactionState;
use inspectah_pipeline::render::containerfile::{
    render_containerfile, render_containerfile_with_originals,
};

use crate::aggregate::variant_ops::{self, VariantProjectionState};
use crate::baseline_summary::{BaselineSummary, derive_baseline_summary};
use crate::classify::{classify_configs, classify_packages};
use crate::group_state::{GroupEvalContext, derive_group_state};
use crate::normalize::{
    normalize_config_defaults, normalize_inspectah_repo_files, normalize_package_defaults,
};
use crate::repo_index::RepoIndex;
use crate::types::{
    AggregateContext, AnnotatedOp, AnnotatedTimelineEntry, ChangesSummary, ContentHash, ItemId,
    RefineError, RefineMode, RefineStats, RefinedView, RefinementOp, RepoProvenance,
    SectionChangeSummary, SectionKind, SectionStats, TimelineEntry, TriageBucket, TriageReason,
    UserPasswordOp, ViewDirective,
};
use inspectah_core::types::group_render::RenderContext;
use inspectah_core::types::rpm::PackageEntry;
use inspectah_core::util::env_hash;

pub struct RefineSession {
    original: InspectionSnapshot,
    repo_index: RepoIndex,
    baseline_available: bool,
    refine_mode: RefineMode,
    timeline: Vec<TimelineEntry>,
    cursor: usize,
    cached_view: Option<RefinedView>,
    cached_render_context: Option<RenderContext>,
    cached_decisions: Option<crate::projection::DecisionProjection>,
    cached_reference: std::sync::OnceLock<crate::projection::ReferenceProjection>,
    generation: u64,
    /// Tracks which context items the user has viewed in the UI.
    /// Format: "section:item_id" (e.g., "packages:httpd.x86_64").
    /// Non-serialized — excluded from tarball export.
    viewed: HashSet<String>,
    /// Path to the source tarball. When set, auto-save writes a session
    /// sidecar file after every cursor-changing mutation.
    tarball_path: Option<PathBuf>,
    /// Set to true when auto-save encounters a permanent I/O failure
    /// (EROFS, EACCES). Suppresses further save attempts for this session.
    durability_degraded: bool,
}

fn canonical_package_id(name: &str, arch: &str) -> String {
    format!("{name}.{arch}")
}

/// Check whether an item is locked in the original (normalized) snapshot.
/// Locked items cannot be re-included via SetInclude ops.
fn is_item_locked(snapshot: &InspectionSnapshot, item_id: &ItemId) -> bool {
    match item_id {
        ItemId::Package { name, arch } => snapshot
            .rpm
            .as_ref()
            .and_then(|r| {
                r.packages_added
                    .iter()
                    .find(|e| e.name == *name && e.arch == *arch)
            })
            .map(|e| e.locked)
            .unwrap_or(false),
        ItemId::Config { path } => snapshot
            .config
            .as_ref()
            .and_then(|c| c.files.iter().find(|e| e.path == *path))
            .map(|e| e.locked)
            .unwrap_or(false),
        ItemId::Service { unit } => snapshot
            .services
            .as_ref()
            .and_then(|s| s.state_changes.iter().find(|sc| sc.unit == *unit))
            .map(|sc| sc.locked)
            .unwrap_or(false),
        ItemId::DropIn { path } => snapshot
            .services
            .as_ref()
            .and_then(|s| s.drop_ins.iter().find(|d| d.path == *path))
            .map(|d| d.locked)
            .unwrap_or(false),
        ItemId::Quadlet { path } => snapshot
            .containers
            .as_ref()
            .and_then(|c| c.quadlet_units.iter().find(|q| q.path == *path))
            .map(|q| q.locked)
            .unwrap_or(false),
        ItemId::Flatpak {
            app_id,
            remote,
            branch,
        } => snapshot
            .containers
            .as_ref()
            .and_then(|c| {
                c.flatpak_apps
                    .iter()
                    .find(|f| f.app_id == *app_id && f.remote == *remote && f.branch == *branch)
            })
            .map(|f| f.locked)
            .unwrap_or(false),
        ItemId::Fstab { mount_point } => snapshot
            .storage
            .as_ref()
            .and_then(|s| {
                s.fstab_entries
                    .iter()
                    .find(|e| e.mount_point == *mount_point)
            })
            .map(|e| e.locked)
            .unwrap_or(false),
        ItemId::Sysctl { key } => snapshot
            .kernel_boot
            .as_ref()
            .and_then(|kb| kb.sysctl_overrides.iter().find(|s| s.key == *key))
            .map(|s| s.locked)
            .unwrap_or(false),
        // Item kinds without locked field or not yet handled: not lockable
        _ => false,
    }
}

/// Defense-in-depth: clamp all locked items to include=false in the
/// projected snapshot. Ensures renderers and exporters never see a
/// locked item with include=true, regardless of op-stack state.
fn clamp_locked_items(snapshot: &mut InspectionSnapshot) {
    if let Some(ref mut rpm) = snapshot.rpm {
        for pkg in &mut rpm.packages_added {
            if pkg.locked {
                pkg.include = false;
            }
        }
        for ms in &mut rpm.module_streams {
            if ms.locked {
                ms.include = false;
            }
        }
        for vl in &mut rpm.version_locks {
            if vl.locked {
                vl.include = false;
            }
        }
        for rf in &mut rpm.repo_files {
            if rf.locked {
                rf.include = false;
            }
        }
    }
    if let Some(ref mut config) = snapshot.config {
        for f in &mut config.files {
            if f.locked {
                f.include = false;
            }
        }
    }
    if let Some(ref mut services) = snapshot.services {
        for sc in &mut services.state_changes {
            if sc.locked {
                sc.include = false;
            }
        }
        for di in &mut services.drop_ins {
            if di.locked {
                di.include = false;
            }
        }
    }
    if let Some(ref mut containers) = snapshot.containers {
        for q in &mut containers.quadlet_units {
            if q.locked {
                q.include = false;
            }
        }
        for cf in &mut containers.compose_files {
            if cf.locked {
                cf.include = false;
            }
        }
        for rc in &mut containers.running_containers {
            if rc.locked {
                rc.include = false;
            }
        }
        for f in &mut containers.flatpak_apps {
            if f.locked {
                f.include = false;
            }
        }
    }
    if let Some(ref mut storage) = snapshot.storage {
        for e in &mut storage.fstab_entries {
            if e.locked {
                e.include = false;
            }
        }
    }
    if let Some(ref mut kb) = snapshot.kernel_boot {
        for s in &mut kb.sysctl_overrides {
            if s.locked {
                s.include = false;
            }
        }
        for m in &mut kb.loaded_modules {
            if m.locked {
                m.include = false;
            }
        }
        for m in &mut kb.non_default_modules {
            if m.locked {
                m.include = false;
            }
        }
    }
    if let Some(ref mut net) = snapshot.network {
        for c in &mut net.connections {
            if c.locked {
                c.include = false;
            }
        }
        for fz in &mut net.firewall_zones {
            if fz.locked {
                fz.include = false;
            }
        }
        for fdr in &mut net.firewall_direct_rules {
            if fdr.locked {
                fdr.include = false;
            }
        }
    }
    if let Some(ref mut sched) = snapshot.scheduled_tasks {
        for cj in &mut sched.cron_jobs {
            if cj.locked {
                cj.include = false;
            }
        }
        for st in &mut sched.systemd_timers {
            if st.locked {
                st.include = false;
            }
        }
        for aj in &mut sched.at_jobs {
            if aj.locked {
                aj.include = false;
            }
        }
        for gtu in &mut sched.generated_timer_units {
            if gtu.locked {
                gtu.include = false;
            }
        }
    }
    if let Some(ref mut sel) = snapshot.selinux {
        for pl in &mut sel.port_labels {
            if pl.locked {
                pl.include = false;
            }
        }
    }
    if let Some(ref mut nonrpm) = snapshot.non_rpm_software {
        for item in &mut nonrpm.items {
            if item.locked {
                item.include = false;
            }
        }
    }
}

impl RefineSession {
    pub fn new(mut snapshot: InspectionSnapshot) -> Self {
        let repo_index = RepoIndex::build(&snapshot);
        let baseline_available = snapshot
            .rpm
            .as_ref()
            .and_then(|r| r.baseline_package_names.as_ref())
            .is_some();

        // Aggregate snapshots use prevalence-based intersection, not leaf
        // filtering. Clear leaf_packages before normalization so
        // normalize_package_defaults does not exclude non-leaf packages.
        let is_aggregate_snapshot = snapshot.aggregate_meta.is_some();
        if is_aggregate_snapshot && let Some(ref mut rpm) = snapshot.rpm {
            rpm.leaf_packages = None;
        }

        // Classify then normalize — materializes tier-aware defaults
        // into the snapshot BEFORE the op stack begins.
        let pkgs = classify_packages(&snapshot);
        let configs = classify_configs(&snapshot);
        normalize_package_defaults(&mut snapshot, &pkgs);
        normalize_config_defaults(&mut snapshot, &configs);
        normalize_inspectah_repo_files(&mut snapshot);

        // Detect aggregate mode from snapshot metadata.
        let refine_mode = if let Some(ref aggregate_meta) = snapshot.aggregate_meta {
            let zones_active = aggregate_meta.host_count >= 3;
            let mut zones = HashMap::new();

            // Classify multi-variant config paths by most-divergent variant.
            // When a path has 2+ variants (e.g., 3 hosts have version A, 2 have
            // version B), the path-level zone should reflect the divergence, not
            // hide it. We classify each variant individually and take the min
            // (Divergent < NearConsensus < Consensus).
            if let Some(ref cfg) = snapshot.config {
                let mut path_zones: HashMap<&str, PrevalenceZone> = HashMap::new();
                for entry in &cfg.files {
                    if let Some(ref prevalence) = entry.aggregate {
                        let variant_zone = classify_zone(prevalence);
                        path_zones
                            .entry(entry.path.as_str())
                            .and_modify(|current| {
                                *current = (*current).min(variant_zone);
                            })
                            .or_insert(variant_zone);
                    }
                }
                for (path, zone) in &path_zones {
                    zones.insert(
                        ItemId::Config {
                            path: path.to_string(),
                        },
                        *zone,
                    );
                }
            }

            // Populate zone map from packages.
            if let Some(ref rpm) = snapshot.rpm {
                for entry in &rpm.packages_added {
                    if let Some(ref prevalence) = entry.aggregate {
                        let zone = classify_zone(prevalence);
                        let item_id = ItemId::Package {
                            name: entry.name.clone(),
                            arch: entry.arch.clone(),
                        };
                        zones.insert(item_id, zone);
                    }
                }
            }

            // Zone classification for drop-ins (services section).
            if let Some(ref svc) = snapshot.services {
                let mut dropin_sum: HashMap<&str, (i32, i32)> = HashMap::new();
                for entry in &svc.drop_ins {
                    if let Some(ref prevalence) = entry.aggregate {
                        dropin_sum
                            .entry(entry.path.as_str())
                            .and_modify(|(sum, _)| {
                                *sum += prevalence.count;
                            })
                            .or_insert((prevalence.count, prevalence.total));
                    }
                }
                for (path, (count, total)) in &dropin_sum {
                    let item_prev = inspectah_core::types::aggregate::AggregatePrevalence {
                        count: *count,
                        total: *total,
                        hosts: vec![],
                        ..Default::default()
                    };
                    zones.insert(
                        ItemId::DropIn {
                            path: path.to_string(),
                        },
                        classify_zone(&item_prev),
                    );
                }
            }

            // Zone classification for quadlets (containers section).
            if let Some(ref containers) = snapshot.containers {
                let mut quadlet_sum: HashMap<&str, (i32, i32)> = HashMap::new();
                for entry in &containers.quadlet_units {
                    if let Some(ref prevalence) = entry.aggregate {
                        quadlet_sum
                            .entry(entry.path.as_str())
                            .and_modify(|(sum, _)| {
                                *sum += prevalence.count;
                            })
                            .or_insert((prevalence.count, prevalence.total));
                    }
                }
                for (path, (count, total)) in &quadlet_sum {
                    let item_prev = inspectah_core::types::aggregate::AggregatePrevalence {
                        count: *count,
                        total: *total,
                        hosts: vec![],
                        ..Default::default()
                    };
                    zones.insert(
                        ItemId::Quadlet {
                            path: path.to_string(),
                        },
                        classify_zone(&item_prev),
                    );
                }
            }

            RefineMode::Aggregate(AggregateContext {
                aggregate_meta: aggregate_meta.clone(),
                zones,
                total_hosts: aggregate_meta.host_count,
                zones_active,
                repo_conflicts: snapshot.rpm_repo_conflicts.clone(),
            })
        } else {
            RefineMode::SingleHost
        };

        // Aggregate prevalence narrowing is handled in the aggregate merge layer
        // (merge.rs). Items below full prevalence arrive with include=false
        // already set before the session sees them.

        let mut session = Self {
            original: snapshot,
            repo_index,
            baseline_available,
            refine_mode,
            timeline: Vec::new(),
            cursor: 0,
            cached_view: None,
            cached_render_context: None,
            cached_decisions: None,
            cached_reference: std::sync::OnceLock::new(),
            generation: 0,
            viewed: HashSet::new(),
            tarball_path: None,
            durability_degraded: false,
        };
        session.recompute_view();
        session
    }

    /// Create a session from a snapshot with a known tarball path.
    /// Enables auto-save: a session sidecar file is written after every
    /// cursor-changing mutation (apply, undo, redo).
    pub fn new_with_tarball(snapshot: InspectionSnapshot, tarball: PathBuf) -> Self {
        let mut session = Self::new(snapshot);
        session.tarball_path = Some(tarball);
        session
    }

    /// Persist current session state to the sidecar file.
    ///
    /// No-op when `tarball_path` is `None` or durability has been degraded
    /// by a prior permanent I/O error. Transient failures are logged but
    /// do not degrade durability.
    fn try_autosave(&mut self) {
        let tarball = match &self.tarball_path {
            Some(p) if !self.durability_degraded => p.clone(),
            _ => return,
        };

        let tarball_hash = match crate::autosave::compute_tarball_hash(&tarball) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("autosave: failed to hash tarball: {e}");
                return;
            }
        };

        let state = crate::autosave::SessionState {
            schema_version: 3,
            tarball_path: tarball.clone(),
            tarball_hash,
            timeline: self.timeline.clone(),
            cursor: self.cursor,
            saved_at: {
                let dur = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                format!("{}s", dur.as_secs())
            },
        };

        if let Err(e) = crate::autosave::save_session(&state, &tarball) {
            let is_permanent = matches!(
                e.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
            );
            if is_permanent {
                eprintln!("autosave: permanently degraded — {e}");
                self.durability_degraded = true;
            } else {
                eprintln!("autosave: transient failure — {e}");
            }
        }
    }

    /// Attempt to resume a previous refine session from the sidecar file
    /// next to the given tarball.
    ///
    /// Returns `Ok(None)` if no session file exists. Returns an error if
    /// the session file is corrupt, the tarball has been modified since the
    /// session was saved (stale), or the tarball cannot be loaded.
    ///
    /// On success, the returned session has all saved ops restored with the
    /// full redo tail preserved, and auto-save enabled.
    pub fn resume_from(tarball: &Path) -> Result<Option<Self>, RefineError> {
        let saved = match crate::autosave::load_session(tarball) {
            Ok(Some(s)) => s,
            Ok(None) => return Ok(None),
            Err(e) => {
                return Err(RefineError::SnapshotLoad(format!(
                    "failed to load session file: {e}"
                )));
            }
        };

        // Stale tarball detection: reject resume if the tarball has changed
        // since the session was saved.
        let current_hash = crate::autosave::compute_tarball_hash(tarball)
            .map_err(|e| RefineError::SnapshotLoad(format!("hash computation failed: {e}")))?;
        if current_hash != saved.tarball_hash {
            return Err(RefineError::StaleTarball {
                saved_hash: saved.tarball_hash.as_str().to_string(),
                current_hash: current_hash.as_str().to_string(),
            });
        }

        // Load a fresh session from the tarball (extract, validate, normalize).
        let fresh = crate::tarball::from_tarball(tarball)?;
        let snapshot = fresh.snapshot().clone();

        // Reconstruct with tarball path for auto-save
        let mut session = Self::new_with_tarball(snapshot, tarball.to_path_buf());

        // Direct restore: set timeline and cursor atomically, skip per-op validation.
        // Safe because: (a) ops were validated on original apply, (b) tarball
        // hash match guarantees identical snapshot baseline. This preserves
        // the full redo tail because we bypass apply() which truncates.
        // v3 autosave stores Vec<TimelineEntry> directly; v2 files are
        // migrated to v3 on load (see autosave::load_session).
        session.timeline = saved.timeline;
        session.cursor = saved.cursor.min(session.timeline.len()); // clamp to valid range
        session.cached_view = None;
        session.cached_render_context = None;
        session.cached_decisions = None;
        session.recompute_view();

        // Single autosave to confirm restored state
        session.try_autosave();

        Ok(Some(session))
    }

    /// Enable auto-save for an existing session by setting the tarball path.
    /// Called by the CLI after `from_tarball()` to wire up persistence.
    pub fn set_tarball_path(&mut self, path: PathBuf) {
        self.tarball_path = Some(path);
    }

    pub fn repo_index(&self) -> &RepoIndex {
        &self.repo_index
    }

    /// Returns the aggregate context if this session was created from an aggregate snapshot.
    /// Returns `None` for single-host snapshots.
    pub fn aggregate_context(&self) -> Option<&AggregateContext> {
        match &self.refine_mode {
            RefineMode::Aggregate(ctx) => Some(ctx),
            RefineMode::SingleHost => None,
        }
    }

    pub fn view(&self) -> &RefinedView {
        self.cached_view
            .as_ref()
            .expect("view is always computed after new() or mutation")
    }

    /// Returns the render context computed alongside the view.
    /// Contains per-group rendering state derived from the session timeline.
    pub fn render_context(&self) -> &RenderContext {
        self.cached_render_context
            .as_ref()
            .expect("render context is always computed after new() or mutation")
    }

    pub fn apply(&mut self, op: RefinementOp) -> Result<(), RefineError> {
        // Validate target exists
        self.validate_target(&op)?;

        // Short-circuit: locked items cannot be re-included.
        // Must happen before the op is recorded to prevent stale ops
        // from persisting in autosaved history.
        if let RefinementOp::SetInclude {
            ref item_id,
            include: true,
        } = op
            && is_item_locked(&self.original, item_id)
        {
            return Ok(()); // silent no-op — op never recorded
        }

        // Check idempotency
        if self.is_op_noop(&op) {
            return Ok(());
        }

        // Truncate redo history at cursor
        self.timeline.truncate(self.cursor);
        self.timeline.push(TimelineEntry::Op(op));
        self.cursor += 1;
        self.generation += 1;
        self.cached_view = None;
        self.cached_render_context = None;
        self.cached_decisions = None;
        self.recompute_view();
        self.try_autosave();
        Ok(())
    }

    /// Apply a view-plane directive (e.g., ungroup a package group).
    /// View directives do not mutate the projected snapshot — they control
    /// how data is displayed. They share the timeline with refinement ops
    /// so undo/redo works uniformly.
    pub fn apply_directive(&mut self, directive: ViewDirective) -> Result<(), RefineError> {
        // Validate: group must exist in installed_groups
        match &directive {
            ViewDirective::UngroupGroup { group_name } => {
                let groups = self.installed_groups();
                if !groups.iter().any(|g| g.name == *group_name) {
                    return Err(RefineError::BadRequest(format!(
                        "unknown group: {group_name}"
                    )));
                }
            }
        }

        // Check idempotency — skip if an identical directive is already active
        if self.is_directive_noop(&directive) {
            return Ok(());
        }

        // Truncate redo history at cursor
        self.timeline.truncate(self.cursor);
        self.timeline.push(TimelineEntry::View(directive));
        self.cursor += 1;
        self.generation += 1;
        self.cached_view = None;
        self.cached_render_context = None;
        self.cached_decisions = None;
        self.recompute_view();
        self.try_autosave();
        Ok(())
    }

    pub fn undo(&mut self) -> Result<(), RefineError> {
        if self.cursor == 0 {
            return Err(RefineError::NothingToUndo);
        }
        self.cursor -= 1;
        self.generation += 1;
        self.cached_view = None;
        self.cached_render_context = None;
        self.cached_decisions = None;
        self.recompute_view();
        self.try_autosave();
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), RefineError> {
        if self.cursor >= self.timeline.len() {
            return Err(RefineError::NothingToRedo);
        }
        self.cursor += 1;
        self.generation += 1;
        self.cached_view = None;
        self.cached_render_context = None;
        self.cached_decisions = None;
        self.recompute_view();
        self.try_autosave();
        Ok(())
    }

    pub fn ops_history(&self) -> Vec<AnnotatedOp> {
        self.timeline
            .iter()
            .enumerate()
            .filter_map(|(i, entry)| match entry {
                TimelineEntry::Op(op) => Some(AnnotatedOp {
                    op: op.clone(),
                    active: i < self.cursor,
                }),
                TimelineEntry::View(_) => None,
            })
            .collect()
    }

    /// Returns all timeline entries (both Op and View) with active flags.
    pub fn timeline_history(&self) -> Vec<AnnotatedTimelineEntry> {
        self.timeline
            .iter()
            .enumerate()
            .map(|(i, entry)| AnnotatedTimelineEntry {
                entry: entry.clone(),
                active: i < self.cursor,
            })
            .collect()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Returns the number of entries in the timeline (including redo tail).
    pub fn timeline_len(&self) -> usize {
        self.timeline.len()
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_redo(&self) -> bool {
        self.cursor < self.timeline.len()
    }

    pub fn pending_changes(&self) -> ChangesSummary {
        let projected = self.project_snapshot();

        // --- Package changes ---
        let mut pkg_included = Vec::new();
        let mut pkg_excluded = Vec::new();
        if let (Some(orig_rpm), Some(proj_rpm)) = (&self.original.rpm, &projected.rpm) {
            for (orig, proj) in orig_rpm.packages_added.iter().zip(&proj_rpm.packages_added) {
                if orig.include != proj.include {
                    let id = ItemId::Package {
                        name: orig.name.clone(),
                        arch: orig.arch.clone(),
                    };
                    if proj.include {
                        pkg_included.push(id);
                    } else {
                        pkg_excluded.push(id);
                    }
                }
            }
        }

        // --- Config changes ---
        let mut cfg_included = Vec::new();
        let mut cfg_excluded = Vec::new();
        if let (Some(orig_cfg), Some(proj_cfg)) = (&self.original.config, &projected.config) {
            for (orig, proj) in orig_cfg.files.iter().zip(&proj_cfg.files) {
                if orig.include != proj.include {
                    let id = ItemId::Config {
                        path: orig.path.clone(),
                    };
                    if proj.include {
                        cfg_included.push(id);
                    } else {
                        cfg_excluded.push(id);
                    }
                }
            }
        }

        // --- Repo changes ---
        let repos_excluded_set = self.excluded_sections_at(&projected);
        let repo_excluded: Vec<ItemId> = repos_excluded_set
            .into_iter()
            .map(|s| ItemId::Repo { path: s })
            .collect();

        // --- Build sections vec (only non-empty sections) ---
        let mut sections = Vec::new();
        if !pkg_included.is_empty() || !pkg_excluded.is_empty() {
            sections.push(SectionChangeSummary {
                kind: SectionKind::Package,
                included: pkg_included,
                excluded: pkg_excluded,
            });
        }
        if !cfg_included.is_empty() || !cfg_excluded.is_empty() {
            sections.push(SectionChangeSummary {
                kind: SectionKind::Config,
                included: cfg_included,
                excluded: cfg_excluded,
            });
        }
        if !repo_excluded.is_empty() {
            sections.push(SectionChangeSummary {
                kind: SectionKind::Repo,
                included: Vec::new(),
                excluded: repo_excluded,
            });
        }

        // Projection-based variant dirty check: compare projected variant_selection
        // values against originals. A variant op followed by its reverse
        // (e.g., select A->B then B->A) correctly reports variants_changed == 0.
        let variants_changed = {
            use inspectah_core::types::aggregate::VariantSelection;
            let mut count = 0usize;

            // Config variants
            if let (Some(orig_cfg), Some(proj_cfg)) = (&self.original.config, &projected.config) {
                for orig_entry in &orig_cfg.files {
                    if let Some(proj_entry) = proj_cfg.files.iter().find(|e| {
                        e.path == orig_entry.path
                            && ContentHash::from_content(e.content.as_bytes())
                                == ContentHash::from_content(orig_entry.content.as_bytes())
                    }) {
                        if proj_entry.variant_selection != orig_entry.variant_selection {
                            count += 1;
                        }
                    } else {
                        count += 1;
                    }
                }
                for proj_entry in &proj_cfg.files {
                    let in_original = orig_cfg.files.iter().any(|e| {
                        e.path == proj_entry.path
                            && ContentHash::from_content(e.content.as_bytes())
                                == ContentHash::from_content(proj_entry.content.as_bytes())
                    });
                    if !in_original && proj_entry.variant_selection != VariantSelection::Only {
                        count += 1;
                    }
                }
            }

            // Drop-in variants
            if let (Some(orig_svc), Some(proj_svc)) = (&self.original.services, &projected.services)
            {
                for orig_entry in &orig_svc.drop_ins {
                    if let Some(proj_entry) = proj_svc.drop_ins.iter().find(|e| {
                        e.path == orig_entry.path
                            && ContentHash::from_content(e.content.as_bytes())
                                == ContentHash::from_content(orig_entry.content.as_bytes())
                    }) {
                        if proj_entry.variant_selection != orig_entry.variant_selection {
                            count += 1;
                        }
                    } else {
                        count += 1;
                    }
                }
                for proj_entry in &proj_svc.drop_ins {
                    let in_original = orig_svc.drop_ins.iter().any(|e| {
                        e.path == proj_entry.path
                            && ContentHash::from_content(e.content.as_bytes())
                                == ContentHash::from_content(proj_entry.content.as_bytes())
                    });
                    if !in_original && proj_entry.variant_selection != VariantSelection::Only {
                        count += 1;
                    }
                }
            }

            // Quadlet variants
            if let (Some(orig_ctr), Some(proj_ctr)) =
                (&self.original.containers, &projected.containers)
            {
                for orig_entry in &orig_ctr.quadlet_units {
                    if let Some(proj_entry) = proj_ctr.quadlet_units.iter().find(|e| {
                        e.path == orig_entry.path
                            && ContentHash::from_content(e.content.as_bytes())
                                == ContentHash::from_content(orig_entry.content.as_bytes())
                    }) {
                        if proj_entry.variant_selection != orig_entry.variant_selection {
                            count += 1;
                        }
                    } else {
                        count += 1;
                    }
                }
                for proj_entry in &proj_ctr.quadlet_units {
                    let in_original = orig_ctr.quadlet_units.iter().any(|e| {
                        e.path == proj_entry.path
                            && ContentHash::from_content(e.content.as_bytes())
                                == ContentHash::from_content(proj_entry.content.as_bytes())
                    });
                    if !in_original && proj_entry.variant_selection != VariantSelection::Only {
                        count += 1;
                    }
                }
            }

            // Compose variant selection changes
            if let (Some(orig_cont), Some(proj_cont)) =
                (&self.original.containers, &projected.containers)
            {
                for orig_entry in &orig_cont.compose_files {
                    let orig_hash = ContentHash::from_content(
                        serde_json::to_string(&orig_entry.images)
                            .unwrap_or_default()
                            .as_bytes(),
                    );
                    if let Some(proj_entry) = proj_cont.compose_files.iter().find(|e| {
                        e.path == orig_entry.path
                            && ContentHash::from_content(
                                serde_json::to_string(&e.images)
                                    .unwrap_or_default()
                                    .as_bytes(),
                            ) == orig_hash
                    }) {
                        if proj_entry.variant_selection != orig_entry.variant_selection {
                            count += 1;
                        }
                    } else {
                        count += 1;
                    }
                }
            }

            count
        };

        // View directives (e.g. UngroupGroup) are not captured by
        // section-include or variant-selection diffs, so we check the
        // active timeline for any View entries.  Using `cursor > 0`
        // would incorrectly mark net-zero variant round-trips as dirty.
        let has_active_directives = self.timeline[..self.cursor]
            .iter()
            .any(|e| matches!(e, TimelineEntry::View(_)));

        let is_dirty = sections
            .iter()
            .any(|s| !s.included.is_empty() || !s.excluded.is_empty())
            || variants_changed > 0
            || has_active_directives;

        ChangesSummary {
            sections,
            variants_changed,
            is_dirty,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.pending_changes().is_dirty
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Returns a reference to the original (unmodified) snapshot.
    pub fn snapshot(&self) -> &InspectionSnapshot {
        &self.original
    }

    /// Snapshot the current projected state. Returns an owned clone.
    /// Used by the HTTP layer to snapshot under the lock and then
    /// release the lock before doing expensive export work.
    pub fn snapshot_projected(&self) -> InspectionSnapshot {
        self.project_snapshot()
    }

    /// Derive baseline summary from the current view's classified packages.
    ///
    /// Returns `None` when the snapshot has no `target_image` or `baseline`.
    /// Counts reflect classification state, not triage state — they are
    /// stable across user include/exclude operations.
    pub fn baseline_summary(&self) -> Option<BaselineSummary> {
        derive_baseline_summary(&self.original, &self.view().packages)
    }

    /// Return the leaf dependency tree from the snapshot's RPM section.
    /// Returns an empty JSON object when RPM data is unavailable.
    pub fn leaf_dep_tree(&self) -> serde_json::Value {
        self.snapshot()
            .rpm
            .as_ref()
            .map(|rpm| rpm.leaf_dep_tree.clone())
            .unwrap_or(serde_json::json!({}))
    }

    /// Valid section prefixes for viewed IDs.
    const VALID_SECTIONS: &'static [&'static str] = &[
        "packages",
        "configs",
        "services",
        "containers",
        "users_groups",
        "network",
        "storage",
        "scheduled_tasks",
        "non_rpm_software",
        "kernel_boot",
        "selinux",
    ];

    /// Validate that a viewed ID matches the `section:item_id` format.
    fn validate_viewed_id(id: &str) -> Result<(), RefineError> {
        let Some((section, item_id)) = id.split_once(':') else {
            return Err(RefineError::BadRequest(format!(
                "invalid viewed ID format: expected 'section:item_id', got '{id}'"
            )));
        };
        if item_id.is_empty() {
            return Err(RefineError::BadRequest(format!(
                "invalid viewed ID: item_id is empty in '{id}'"
            )));
        }
        if !Self::VALID_SECTIONS.contains(&section) {
            return Err(RefineError::BadRequest(format!(
                "invalid viewed ID section '{section}': must be one of {:?}",
                Self::VALID_SECTIONS
            )));
        }
        Ok(())
    }

    /// Mark a context item as viewed by the user.
    /// `id` format: "section:item_id" (e.g., "packages:httpd.x86_64").
    /// Returns an error if the ID format is invalid.
    pub fn mark_viewed(&mut self, id: &str) -> Result<(), RefineError> {
        Self::validate_viewed_id(id)?;
        self.viewed.insert(id.to_string());
        Ok(())
    }

    /// Check whether a context item has been viewed.
    pub fn is_viewed(&self, id: &str) -> bool {
        self.viewed.contains(id)
    }

    /// Returns the full set of viewed item IDs.
    pub fn viewed_ids(&self) -> &HashSet<String> {
        &self.viewed
    }

    /// Returns true if the PROJECTED state contains sensitive material.
    ///
    /// Sensitive when `sensitive_snapshot` is true OR any user has
    /// `password_choice == "new"` with a non-empty `password_hash`.
    /// Based on projected state, not op history.
    pub fn is_sensitive(&self) -> bool {
        let projected = self.project_snapshot();
        if projected.sensitive_snapshot {
            return true;
        }
        if let Some(ug) = &projected.users_groups {
            for user in &ug.users {
                let choice = user
                    .get("password_choice")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let has_hash = user
                    .get("password_hash")
                    .and_then(|v| v.as_str())
                    .map(|s| !s.is_empty())
                    .unwrap_or(false);
                if choice == "new" && has_hash {
                    return true;
                }
            }
        }
        false
    }

    pub fn export_tarball(&self, path: &Path, expected_generation: u64) -> Result<(), RefineError> {
        if expected_generation != self.generation {
            return Err(RefineError::StaleGeneration {
                expected: expected_generation,
                actual: self.generation,
            });
        }

        let projected = self.project_snapshot();
        let orig_inc: std::collections::HashMap<String, bool> = self
            .original
            .rpm
            .as_ref()
            .map(|r| {
                r.packages_added
                    .iter()
                    .map(|p| (canonical_package_id(&p.name, &p.arch), p.include))
                    .collect()
            })
            .unwrap_or_default();
        render_refine_export(
            &projected,
            path,
            Some(&orig_inc),
            self.cached_render_context.as_ref(),
        )
    }

    // --- Private helpers ---

    fn validate_target(&self, op: &RefinementOp) -> Result<(), RefineError> {
        match op {
            RefinementOp::SetInclude { item_id, include } => {
                match item_id {
                    ItemId::Package { name, arch } => {
                        let found = self
                            .original
                            .rpm
                            .as_ref()
                            .map(|r| {
                                r.packages_added
                                    .iter()
                                    .any(|e| e.name == *name && e.arch == *arch)
                            })
                            .unwrap_or(false);
                        if !found {
                            return Err(RefineError::UnknownTarget(format!("{name}.{arch}")));
                        }
                    }
                    ItemId::Config { path } => {
                        let found = self
                            .original
                            .config
                            .as_ref()
                            .map(|c| c.files.iter().any(|e| e.path == *path))
                            .unwrap_or(false);
                        if !found {
                            return Err(RefineError::UnknownTarget(path.clone()));
                        }
                    }
                    ItemId::Repo { path: section_id } => {
                        if RepoIndex::is_distro_repo(section_id) {
                            return Err(RefineError::BadRequest(format!(
                                "cannot toggle distro repo: {section_id}"
                            )));
                        }
                        let prov = self.repo_index.provenance(section_id);
                        if !matches!(prov, RepoProvenance::Verified) {
                            let verb = if *include { "include" } else { "exclude" };
                            return Err(RefineError::BadRequest(format!(
                                "cannot {verb} repo '{section_id}': provenance is {prov:?}"
                            )));
                        }
                    }
                    ItemId::Service { unit } => {
                        let found = self
                            .original
                            .services
                            .as_ref()
                            .map(|s| s.state_changes.iter().any(|sc| sc.unit == *unit))
                            .unwrap_or(false);
                        if !found {
                            return Err(RefineError::UnknownTarget(unit.clone()));
                        }
                    }
                    ItemId::DropIn { path } => {
                        let found = self
                            .original
                            .services
                            .as_ref()
                            .map(|s| s.drop_ins.iter().any(|d| d.path == *path))
                            .unwrap_or(false);
                        if !found {
                            return Err(RefineError::UnknownTarget(path.clone()));
                        }
                        // Cannot include a drop-in when its parent service is excluded.
                        if *include {
                            let projected = self.project_snapshot();
                            if let Some(ref svc_section) = projected.services
                                && let Some(dropin) =
                                    svc_section.drop_ins.iter().find(|d| d.path == *path)
                            {
                                let parent_included = svc_section
                                    .state_changes
                                    .iter()
                                    .any(|s| s.unit == dropin.unit && s.include);
                                if !parent_included {
                                    return Err(RefineError::BadRequest(
                                        "cannot include drop-in when parent service is excluded"
                                            .into(),
                                    ));
                                }
                            }
                        }
                    }
                    ItemId::Quadlet { path } => {
                        let found = self
                            .original
                            .containers
                            .as_ref()
                            .map(|c| c.quadlet_units.iter().any(|q| q.path == *path))
                            .unwrap_or(false);
                        if !found {
                            return Err(RefineError::UnknownTarget(path.clone()));
                        }
                    }
                    ItemId::Flatpak {
                        app_id,
                        remote,
                        branch,
                    } => {
                        let found = self
                            .original
                            .containers
                            .as_ref()
                            .map(|c| {
                                c.flatpak_apps.iter().any(|f| {
                                    f.app_id == *app_id
                                        && f.remote == *remote
                                        && f.branch == *branch
                                })
                            })
                            .unwrap_or(false);
                        if !found {
                            return Err(RefineError::UnknownTarget(format!(
                                "{app_id} ({remote}/{branch})"
                            )));
                        }
                    }
                    ItemId::Sysctl { key } => {
                        let found = self
                            .original
                            .kernel_boot
                            .as_ref()
                            .map(|kb| kb.sysctl_overrides.iter().any(|s| s.key == *key))
                            .unwrap_or(false);
                        if !found {
                            return Err(RefineError::UnknownTarget(key.clone()));
                        }
                    }
                    ItemId::TunedSelection { profile } => {
                        let found = self
                            .original
                            .kernel_boot
                            .as_ref()
                            .map(|kb| kb.tuned_active == *profile)
                            .unwrap_or(false);
                        if !found {
                            return Err(RefineError::UnknownTarget(profile.clone()));
                        }
                    }
                    // Phase 2-3 item kinds: not yet handled, accept without validation
                    ItemId::Compose { .. }
                    | ItemId::NMConnection { .. }
                    | ItemId::FirewallZone { .. }
                    | ItemId::KernelModule { .. }
                    | ItemId::CronJob { .. }
                    | ItemId::SystemdTimer { .. }
                    | ItemId::AtJob { .. }
                    | ItemId::GeneratedTimer { .. }
                    | ItemId::SelinuxPort { .. }
                    | ItemId::Fstab { .. }
                    | ItemId::NonRpm { .. }
                    | ItemId::LanguageEnv { .. }
                    | ItemId::ModuleStream { .. }
                    | ItemId::VersionLock { .. }
                    | ItemId::Group { .. } => {
                        if let ItemId::Group { name } = item_id {
                            let found = self.installed_groups().iter().any(|g| g.name == *name);
                            if !found {
                                return Err(RefineError::UnknownTarget(name.clone()));
                            }
                        }
                    }
                }
            }
            RefinementOp::UserStrategy { username, .. } => {
                if !self.user_exists(username) {
                    return Err(RefineError::UnknownTarget(username.clone()));
                }
            }
            RefinementOp::UserPassword(pw_op) => {
                let uname = match pw_op {
                    UserPasswordOp::New { username, .. } => username,
                    UserPasswordOp::None { username } => username,
                    UserPasswordOp::Preserve { username } => username,
                };
                if !self.user_exists(uname) {
                    return Err(RefineError::UnknownTarget(uname.clone()));
                }
            }
            // Aggregate variant ops: validate using projection state
            RefinementOp::SelectVariant { item_id, target } => {
                let state = self.build_variant_state();
                variant_ops::validate_select(&self.original, &state, item_id, target)?;
            }
            RefinementOp::EditVariant {
                item_id,
                content: _,
                based_on,
            } => {
                match item_id {
                    ItemId::Config { path } => {
                        let found = self
                            .original
                            .config
                            .as_ref()
                            .map(|c| c.files.iter().any(|e| e.path == *path))
                            .unwrap_or(false);
                        if !found {
                            return Err(RefineError::UnknownTarget(path.clone()));
                        }
                    }
                    ItemId::DropIn { path } => {
                        let found = self
                            .original
                            .services
                            .as_ref()
                            .map(|s| s.drop_ins.iter().any(|e| e.path == *path))
                            .unwrap_or(false);
                        if !found {
                            return Err(RefineError::UnknownTarget(path.clone()));
                        }
                    }
                    ItemId::Quadlet { path } => {
                        let found = self
                            .original
                            .containers
                            .as_ref()
                            .map(|c| c.quadlet_units.iter().any(|e| e.path == *path))
                            .unwrap_or(false);
                        if !found {
                            return Err(RefineError::UnknownTarget(path.clone()));
                        }
                    }
                    ItemId::Compose { .. } => {
                        return Err(RefineError::BadRequest(
                            "EditVariant not supported for Compose items (structured carrier)"
                                .into(),
                        ));
                    }
                    _ => {
                        return Err(RefineError::BadRequest(format!(
                            "EditVariant not supported for {:?}",
                            item_id
                        )));
                    }
                }
                // Validate based_on if provided — scoped to the target item's path.
                if let Some(hash) = based_on {
                    let state = self.build_variant_state();
                    let path = variant_ops::item_path(item_id);
                    let in_user = path
                        .and_then(|p| state.user_variants.get(p).map(|m| m.contains_key(hash)))
                        .unwrap_or(false);
                    let in_host = self.hash_in_variant_section_for_item(item_id, hash);
                    if !in_user && !in_host {
                        return Err(RefineError::BadRequest(format!(
                            "based_on hash {} not found in variant pool",
                            hash.as_str()
                        )));
                    }
                }
            }
            RefinementOp::DiscardVariant { item_id, variant } => {
                let state = self.build_variant_state();
                variant_ops::validate_discard(&self.original, &state, item_id, variant)?;
            }
        }
        Ok(())
    }

    /// Check whether a username exists in the snapshot's users_groups.users.
    fn user_exists(&self, username: &str) -> bool {
        self.original
            .users_groups
            .as_ref()
            .map(|ug| {
                ug.users.iter().any(|u| {
                    u.get("name")
                        .and_then(|v| v.as_str())
                        .map(|n| n == username)
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    }

    /// Check whether a content hash exists in the host-sourced variant entries
    /// for the target item's path in the appropriate snapshot section.
    /// Scoped to entries matching the target path — prevents cross-item leakage.
    fn hash_in_variant_section_for_item(
        &self,
        item_id: &ItemId,
        hash: &crate::types::ContentHash,
    ) -> bool {
        match item_id {
            ItemId::Config { path } => self
                .original
                .config
                .as_ref()
                .map(|c| {
                    c.files.iter().any(|e| {
                        e.path == *path
                            && crate::types::ContentHash::from_content(e.content.as_bytes())
                                == *hash
                    })
                })
                .unwrap_or(false),
            ItemId::DropIn { path } => self
                .original
                .services
                .as_ref()
                .map(|s| {
                    s.drop_ins.iter().any(|e| {
                        e.path == *path
                            && crate::types::ContentHash::from_content(e.content.as_bytes())
                                == *hash
                    })
                })
                .unwrap_or(false),
            ItemId::Quadlet { path } => self
                .original
                .containers
                .as_ref()
                .map(|c| {
                    c.quadlet_units.iter().any(|e| {
                        e.path == *path
                            && crate::types::ContentHash::from_content(e.content.as_bytes())
                                == *hash
                    })
                })
                .unwrap_or(false),
            _ => false,
        }
    }

    /// Build the variant projection state by replaying ops up to the current cursor.
    /// Used by validate_target to check variant state at the point of validation.
    fn build_variant_state(&self) -> VariantProjectionState {
        let mut state = VariantProjectionState::default();
        for entry in &self.timeline[..self.cursor] {
            if let TimelineEntry::Op(op) = entry {
                match op {
                    RefinementOp::SelectVariant { item_id, target } => {
                        variant_ops::apply_select(&mut state, item_id, target);
                    }
                    RefinementOp::EditVariant {
                        item_id, content, ..
                    } => {
                        variant_ops::apply_edit(&mut state, item_id, content, &self.original);
                    }
                    RefinementOp::DiscardVariant { item_id, variant } => {
                        variant_ops::apply_discard(&mut state, item_id, variant);
                    }
                    _ => {}
                }
            }
        }
        state
    }

    fn is_op_noop(&self, op: &RefinementOp) -> bool {
        let projected = self.project_snapshot();
        match op {
            RefinementOp::SetInclude { item_id, include } => match item_id {
                ItemId::Package { name, arch } => projected
                    .rpm
                    .as_ref()
                    .and_then(|r| {
                        r.packages_added
                            .iter()
                            .find(|e| e.name == *name && e.arch == *arch)
                    })
                    .map(|e| e.include == *include)
                    .unwrap_or(false),
                ItemId::Config { path } => projected
                    .config
                    .as_ref()
                    .and_then(|c| c.files.iter().find(|e| e.path == *path))
                    .map(|e| e.include == *include)
                    .unwrap_or(false),
                ItemId::Repo { path: section_id } => {
                    let excluded = self.excluded_sections_at(&projected);
                    if *include {
                        // Noop if the section is NOT in the excluded set
                        !excluded.contains(section_id)
                    } else {
                        // Noop if the section is already in the excluded set
                        excluded.contains(section_id)
                    }
                }
                ItemId::Group { name } => {
                    // Noop if ALL non-locked members already have the
                    // requested include state.
                    let member_names: Vec<String> = self
                        .installed_groups()
                        .iter()
                        .find(|g| g.name == *name)
                        .map(|g| g.members.clone())
                        .unwrap_or_default();
                    projected
                        .rpm
                        .as_ref()
                        .map(|r| {
                            r.packages_added
                                .iter()
                                .filter(|e| member_names.contains(&e.name))
                                .filter(|e| !(*include && e.locked))
                                .all(|e| e.include == *include)
                        })
                        .unwrap_or(true)
                }
                // Other item kinds: never noop for now
                _ => false,
            },
            // User ops are never noop — always replay to ensure correctness
            RefinementOp::UserStrategy { .. } | RefinementOp::UserPassword(_) => false,
            // Aggregate ops are never noop — projection-derived state makes idempotency detection fragile
            RefinementOp::SelectVariant { .. }
            | RefinementOp::EditVariant { .. }
            | RefinementOp::DiscardVariant { .. } => false,
        }
    }

    /// Check whether a view directive is already active in the timeline
    /// (up to cursor). If so, applying it again is a no-op.
    fn is_directive_noop(&self, directive: &ViewDirective) -> bool {
        match directive {
            ViewDirective::UngroupGroup { group_name } => {
                self.timeline[..self.cursor].iter().any(|entry| {
                    matches!(
                        entry,
                        TimelineEntry::View(ViewDirective::UngroupGroup { group_name: name })
                            if name == group_name
                    )
                })
            }
        }
    }

    /// Returns the installed groups from the snapshot's RPM section,
    /// or an empty slice if none are present.
    fn installed_groups(&self) -> &[inspectah_core::types::rpm::InstalledGroup] {
        self.original
            .rpm
            .as_ref()
            .and_then(|r| r.installed_groups.as_deref())
            .unwrap_or(&[])
    }

    fn project_snapshot(&self) -> InspectionSnapshot {
        let mut snap = self.original.clone();
        let mut variant_state = VariantProjectionState::default();

        for entry in &self.timeline[..self.cursor] {
            let op = match entry {
                TimelineEntry::Op(op) => op,
                TimelineEntry::View(_) => continue,
            };
            match op {
                RefinementOp::SetInclude { item_id, include } => {
                    // Defense-in-depth: skip stale SetInclude(true) ops
                    // from pre-locked autosaved sessions.
                    if *include && is_item_locked(&self.original, item_id) {
                        continue;
                    }
                    match item_id {
                        ItemId::Package { name, arch } => {
                            if let Some(ref mut rpm) = snap.rpm
                                && let Some(pkg) = rpm
                                    .packages_added
                                    .iter_mut()
                                    .find(|e| e.name == *name && e.arch == *arch)
                            {
                                pkg.include = *include;
                            }
                        }
                        ItemId::Config { path } => {
                            if let Some(ref mut config) = snap.config
                                && let Some(entry) =
                                    config.files.iter_mut().find(|e| e.path == *path)
                            {
                                entry.include = *include;
                            }
                        }
                        ItemId::Repo { path: section_id } => {
                            if !*include {
                                // Exclude repo: cascade to packages, repo files, GPG keys
                                let excluded_sections = self.excluded_sections_at(&snap);

                                if let Some(ref mut rpm) = snap.rpm {
                                    // 1. Exclude all packages from this repo (case-insensitive)
                                    for pkg in &mut rpm.packages_added {
                                        if pkg.source_repo.eq_ignore_ascii_case(section_id) {
                                            pkg.include = false;
                                        }
                                    }

                                    // 2. For repo files: exclude only if ALL sections
                                    // defined in that file are now excluded
                                    if let Some(file_paths) =
                                        self.repo_index.repo_file_by_section.get(section_id)
                                    {
                                        for file_path in file_paths {
                                            let all_sections_excluded = self
                                                .repo_index
                                                .repo_file_by_section
                                                .iter()
                                                .filter(|(_, paths)| paths.contains(file_path))
                                                .all(|(sid, _)| excluded_sections.contains(sid));
                                            if all_sections_excluded
                                                && let Some(rf) = rpm
                                                    .repo_files
                                                    .iter_mut()
                                                    .find(|r| r.path == *file_path)
                                            {
                                                rf.include = false;
                                            }
                                        }
                                    }

                                    // 3. For GPG keys: exclude only if ALL sections
                                    // that reference this key are excluded
                                    if let Some(key_paths) =
                                        self.repo_index.gpg_keys_by_section.get(section_id)
                                    {
                                        for key_path in key_paths {
                                            if let Some(referencing_sections) =
                                                self.repo_index.sections_by_gpg_key.get(key_path)
                                            {
                                                let all_excluded = referencing_sections
                                                    .iter()
                                                    .all(|sid| excluded_sections.contains(sid));
                                                if all_excluded
                                                    && let Some(k) = rpm
                                                        .gpg_keys
                                                        .iter_mut()
                                                        .find(|g| g.path == *key_path)
                                                {
                                                    k.include = false;
                                                }
                                            }
                                        }
                                    }
                                }
                            } else {
                                // Include repo: re-enable packages, repo files, GPG keys
                                if let Some(ref mut rpm) = snap.rpm {
                                    // 1. Include all packages from this repo (case-insensitive)
                                    for pkg in &mut rpm.packages_added {
                                        if pkg.source_repo.eq_ignore_ascii_case(section_id) {
                                            pkg.include = true;
                                        }
                                    }

                                    // 2. Re-enable repo files for this section
                                    if let Some(file_paths) =
                                        self.repo_index.repo_file_by_section.get(section_id)
                                    {
                                        for file_path in file_paths {
                                            if let Some(rf) = rpm
                                                .repo_files
                                                .iter_mut()
                                                .find(|r| r.path == *file_path)
                                            {
                                                rf.include = true;
                                            }
                                        }
                                    }

                                    // 3. Re-enable GPG keys for this section
                                    if let Some(key_paths) =
                                        self.repo_index.gpg_keys_by_section.get(section_id)
                                    {
                                        for key_path in key_paths {
                                            if let Some(k) = rpm
                                                .gpg_keys
                                                .iter_mut()
                                                .find(|g| g.path == *key_path)
                                            {
                                                k.include = true;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        ItemId::Service { unit } => {
                            if let Some(ref mut services) = snap.services {
                                if let Some(svc) =
                                    services.state_changes.iter_mut().find(|s| s.unit == *unit)
                                {
                                    svc.include = *include;
                                }
                                // Symmetric cascade: toggling a service cascades
                                // to all drop-ins for that unit.
                                for dropin in
                                    services.drop_ins.iter_mut().filter(|d| d.unit == *unit)
                                {
                                    dropin.include = *include;
                                }
                            }
                        }
                        ItemId::DropIn { path } => {
                            if let Some(ref mut services) = snap.services
                                && let Some(dropin) =
                                    services.drop_ins.iter_mut().find(|d| d.path == *path)
                            {
                                dropin.include = *include;
                            }
                        }
                        ItemId::Quadlet { path } => {
                            if let Some(ref mut containers) = snap.containers
                                && let Some(quadlet) = containers
                                    .quadlet_units
                                    .iter_mut()
                                    .find(|q| q.path == *path)
                            {
                                quadlet.include = *include;
                            }
                        }
                        ItemId::Flatpak {
                            app_id,
                            remote,
                            branch,
                        } => {
                            if let Some(ref mut containers) = snap.containers
                                && let Some(flatpak) =
                                    containers.flatpak_apps.iter_mut().find(|f| {
                                        f.app_id == *app_id
                                            && f.remote == *remote
                                            && f.branch == *branch
                                    })
                            {
                                flatpak.include = *include;
                            }
                        }
                        ItemId::Sysctl { key } => {
                            if let Some(ref mut kb) = snap.kernel_boot
                                && let Some(sysctl) =
                                    kb.sysctl_overrides.iter_mut().find(|s| s.key == *key)
                            {
                                sysctl.include = *include;
                            }
                        }
                        ItemId::TunedSelection { profile } => {
                            if let Some(ref mut kb) = snap.kernel_boot
                                && kb.tuned_active == *profile
                            {
                                kb.tuned_include = *include;
                            }
                        }
                        ItemId::Group { name } => {
                            // Fan out: apply include/exclude to ALL package
                            // members of the named group (all arches).
                            let member_names: Vec<String> = self
                                .installed_groups()
                                .iter()
                                .find(|g| g.name == *name)
                                .map(|g| g.members.clone())
                                .unwrap_or_default();
                            if let Some(ref mut rpm) = snap.rpm {
                                for pkg in &mut rpm.packages_added {
                                    if member_names.contains(&pkg.name) {
                                        if *include && pkg.locked {
                                            // Locked members stay excluded
                                            continue;
                                        }
                                        pkg.include = *include;
                                    }
                                }
                            }
                        }
                        // Phase 2-3 item kinds: not yet handled
                        _ => {}
                    }
                }
                RefinementOp::UserStrategy { username, strategy } => {
                    if let Some(ref mut ug) = snap.users_groups
                        && let Some(user) = ug
                            .users
                            .iter_mut()
                            .find(|u| u.get("name").and_then(|v| v.as_str()) == Some(username))
                        && let Some(m) = user.as_object_mut()
                    {
                        m.insert(
                            "containerfile_strategy".to_string(),
                            serde_json::to_value(strategy).unwrap(),
                        );
                    }
                }
                RefinementOp::UserPassword(pw_op) => {
                    match pw_op {
                        UserPasswordOp::New { username, hash } => {
                            if let Some(ref mut ug) = snap.users_groups
                                && let Some(user) = ug.users.iter_mut().find(|u| {
                                    u.get("name").and_then(|v| v.as_str()) == Some(username)
                                })
                                && let Some(m) = user.as_object_mut()
                            {
                                m.insert("password_choice".to_string(), serde_json::json!("new"));
                                if let Some(h) = hash {
                                    m.insert("password_hash".to_string(), serde_json::json!(h));
                                }
                            }
                        }
                        UserPasswordOp::None { username } => {
                            if let Some(ref mut ug) = snap.users_groups
                                && let Some(user) = ug.users.iter_mut().find(|u| {
                                    u.get("name").and_then(|v| v.as_str()) == Some(username)
                                })
                                && let Some(m) = user.as_object_mut()
                            {
                                m.insert("password_choice".to_string(), serde_json::json!("none"));
                                // CLEAR password_hash
                                m.remove("password_hash");
                            }
                        }
                        UserPasswordOp::Preserve { username } => {
                            // CRITICAL: Restore the ORIGINAL hash from self.original,
                            // not the projected state. This handles New -> Preserve correctly.
                            let original_hash = self
                                .original
                                .users_groups
                                .as_ref()
                                .and_then(|ug| {
                                    ug.users.iter().find(|u| {
                                        u.get("name").and_then(|v| v.as_str()) == Some(username)
                                    })
                                })
                                .and_then(|u| u.get("password_hash"))
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());

                            if let Some(ref mut ug) = snap.users_groups
                                && let Some(user) = ug.users.iter_mut().find(|u| {
                                    u.get("name").and_then(|v| v.as_str()) == Some(username)
                                })
                                && let Some(m) = user.as_object_mut()
                            {
                                m.insert(
                                    "password_choice".to_string(),
                                    serde_json::json!("preserve"),
                                );
                                match original_hash {
                                    Some(h) => {
                                        m.insert("password_hash".to_string(), serde_json::json!(h));
                                    }
                                    None => {
                                        m.remove("password_hash");
                                    }
                                }
                            }
                        }
                    }
                }
                // Aggregate variant ops: accumulate into projection state
                RefinementOp::SelectVariant { item_id, target } => {
                    variant_ops::apply_select(&mut variant_state, item_id, target);
                }
                RefinementOp::EditVariant {
                    item_id, content, ..
                } => {
                    variant_ops::apply_edit(&mut variant_state, item_id, content, &self.original);
                }
                RefinementOp::DiscardVariant { item_id, variant } => {
                    variant_ops::apply_discard(&mut variant_state, item_id, variant);
                }
            }
        }

        // Materialize variant projection state into the snapshot
        variant_ops::materialize_variants(&mut snap, &variant_state);

        // If refine-time ops introduced sensitivity (e.g. NewPassword),
        // upgrade the snapshot's redaction_state and sensitive_snapshot flag.
        if !self.original.sensitive_snapshot {
            let has_new_password = snap
                .users_groups
                .as_ref()
                .map(|ug| {
                    ug.users.iter().any(|u| {
                        let choice = u
                            .get("password_choice")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let has_hash = u
                            .get("password_hash")
                            .and_then(|v| v.as_str())
                            .map(|s| !s.is_empty())
                            .unwrap_or(false);
                        choice == "new" && has_hash
                    })
                })
                .unwrap_or(false);

            if has_new_password {
                snap.sensitive_snapshot = true;

                match &snap.redaction_state {
                    Some(RedactionState::FullyRedacted {
                        redacted_by,
                        config_hash,
                    }) => {
                        snap.redaction_state = Some(RedactionState::SensitiveRetained {
                            redacted_by: redacted_by.clone(),
                            config_hash: config_hash.clone(),
                            unresolved_count: 0,
                            unresolved_hints: Vec::new(),
                        });
                    }
                    Some(RedactionState::PartiallyRedacted {
                        redacted_by,
                        config_hash,
                        unresolved_count,
                        unresolved_hints,
                    }) => {
                        snap.redaction_state = Some(RedactionState::SensitiveRetained {
                            redacted_by: redacted_by.clone(),
                            config_hash: config_hash.clone(),
                            unresolved_count: *unresolved_count,
                            unresolved_hints: unresolved_hints.clone(),
                        });
                    }
                    _ => {} // Already SensitiveRetained or other state
                }
            }
        }

        // Defense-in-depth: clamp locked items to include=false regardless
        // of op-stack state. Ensures renderers and exporters never see a
        // locked item with include=true.
        clamp_locked_items(&mut snap);

        snap
    }

    /// Compute the set of section IDs that are currently excluded based on the
    /// active op stack. A SetInclude(Repo, false) adds to the set, SetInclude(Repo, true) removes.
    fn excluded_sections_at(&self, _snap: &InspectionSnapshot) -> HashSet<String> {
        let mut excluded = HashSet::new();
        for entry in &self.timeline[..self.cursor] {
            if let TimelineEntry::Op(RefinementOp::SetInclude {
                item_id: ItemId::Repo { path: section_id },
                include,
            }) = entry
            {
                if *include {
                    excluded.remove(section_id);
                } else {
                    excluded.insert(section_id.clone());
                }
            }
        }
        excluded
    }

    fn recompute_view(&mut self) {
        let projected = self.project_snapshot();
        let mut all_packages = classify_packages(&projected);
        let mut config_files = classify_configs(&projected);

        // The classifier re-derives include/locked from package metadata,
        // which overwrites user ops (e.g., anaconda Tier 4 always sets
        // include=true). Restore the projected snapshot's include/locked
        // values so user SetInclude ops are respected.
        if let Some(ref rpm) = projected.rpm {
            for pkg in &mut all_packages {
                if let Some(entry) = rpm
                    .packages_added
                    .iter()
                    .find(|e| e.name == pkg.entry.name && e.arch == pkg.entry.arch)
                {
                    pkg.entry.include = entry.include;
                    pkg.entry.locked = entry.locked;
                }
            }
        }

        // Aggregate triage scoring (when in aggregate mode).
        if let RefineMode::Aggregate(ref ctx) = self.refine_mode {
            for pkg in &mut all_packages {
                let item_id = ItemId::Package {
                    name: pkg.entry.name.clone(),
                    arch: pkg.entry.arch.clone(),
                };
                let prevalence_count = pkg
                    .entry
                    .aggregate
                    .as_ref()
                    .map(|f| f.count.max(0) as u32)
                    .unwrap_or(0);
                let prevalence_total = pkg
                    .entry
                    .aggregate
                    .as_ref()
                    .map(|f| f.total.max(0) as u32)
                    .unwrap_or(ctx.total_hosts as u32);
                let aggregate_tag = crate::aggregate::classify::classify_aggregate_bucket(
                    ctx,
                    &item_id,
                    pkg.triage.bucket(),
                    pkg.triage.primary_reason.clone(),
                    prevalence_count,
                    prevalence_total,
                );
                pkg.triage.triage = aggregate_tag.triage;
            }
            for cfg in &mut config_files {
                let item_id = ItemId::Config {
                    path: cfg.entry.path.clone(),
                };
                let prevalence_count = cfg
                    .entry
                    .aggregate
                    .as_ref()
                    .map(|f| f.count.max(0) as u32)
                    .unwrap_or(0);
                let prevalence_total = cfg
                    .entry
                    .aggregate
                    .as_ref()
                    .map(|f| f.total.max(0) as u32)
                    .unwrap_or(ctx.total_hosts as u32);
                let aggregate_tag = crate::aggregate::classify::classify_aggregate_bucket(
                    ctx,
                    &item_id,
                    cfg.triage.bucket(),
                    cfg.triage.primary_reason.clone(),
                    prevalence_count,
                    prevalence_total,
                );
                cfg.triage.triage = aggregate_tag.triage;
            }
        }

        // Build a set of packages that were normalized to include=false at
        // construction time (non-leaf Tier 2 dependencies). These are hidden
        // from the triage view because dnf resolves them automatically.
        // Packages the user explicitly excluded via ops remain visible so
        // the user can undo the exclusion.
        //
        // In aggregate mode, non-universal items arrive with include=false from
        // the aggregate merge layer — those are NOT hidden here because they
        // should remain visible but unchecked. The merge layer tags them with
        // aggregate prevalence data, so we can distinguish them from normalized deps.
        let is_aggregate = matches!(self.refine_mode, RefineMode::Aggregate(_));
        let hidden_deps: HashSet<(&str, &str)> = self
            .original
            .rpm
            .as_ref()
            .map(|r| {
                r.packages_added
                    .iter()
                    .filter(|p| {
                        if p.include {
                            return false;
                        }
                        // In aggregate mode, skip packages excluded by the merge
                        // layer's prevalence narrowing — those should remain
                        // visible but unchecked.
                        if is_aggregate
                            && let Some(ref fp) = p.aggregate
                            && fp.count < fp.total
                        {
                            return false;
                        }
                        true
                    })
                    .map(|p| (p.name.as_str(), p.arch.as_str()))
                    .collect()
            })
            .unwrap_or_default();

        let original_package_includes: HashMap<(&str, &str), bool> = self
            .original
            .rpm
            .as_ref()
            .map(|r| {
                r.packages_added
                    .iter()
                    .map(|pkg| ((pkg.name.as_str(), pkg.arch.as_str()), pkg.include))
                    .collect()
            })
            .unwrap_or_default();

        // Snapshot classified include/locked state before all_packages is
        // consumed by into_iter(). Used below to sync the projected snapshot
        // for Containerfile preview rendering.
        let classified_state: Vec<(String, bool, bool)> = all_packages
            .iter()
            .map(|p| {
                (
                    canonical_package_id(&p.entry.name, &p.entry.arch),
                    p.entry.include,
                    p.entry.locked,
                )
            })
            .collect();

        let packages: Vec<_> = all_packages
            .into_iter()
            .filter(|p| {
                // Platform plumbing packages (grub2-*, shim-*, etc.) are
                // unconditionally excluded, locked, and provide no user
                // signal. Hide them from the refine view entirely.
                if p.triage.primary_reason == TriageReason::PackagePlatformPlumbing {
                    return false;
                }

                // Only filter out packages that were normalized to include=false
                // at construction AND are still false after ops AND are not
                // NeedsReview. These are non-leaf Tier 2 dependencies the
                // operator never needs to see. User-excluded packages (include
                // was true originally) stay visible. Tier 3 (NeedsReview)
                // items stay visible even though they default to include=false.
                if !p.entry.include
                    && hidden_deps.contains(&(p.entry.name.as_str(), p.entry.arch.as_str()))
                    && p.triage.bucket() != TriageBucket::Investigate
                {
                    return false;
                }
                true
            })
            .collect();

        // Filter to leaf packages when authoritative leaf data is available for
        // a single-host snapshot. Preserve NeedsReview packages and any package
        // whose include state the operator explicitly changed so the view/stats
        // stay honest.
        let packages = if let Some(rpm) = projected.rpm.as_ref() {
            // Step 1: ALWAYS filter baseline-suppressed (independent of leaf data)
            let baseline_suppressed_set: HashSet<&str> = rpm
                .baseline_suppressed
                .as_ref()
                .map(|v| v.iter().map(|s| s.as_str()).collect())
                .unwrap_or_default();

            let packages: Vec<_> = if !baseline_suppressed_set.is_empty() {
                packages
                    .into_iter()
                    .filter(|pkg| {
                        let package_id =
                            canonical_package_id(pkg.entry.name.as_str(), pkg.entry.arch.as_str());
                        !baseline_suppressed_set.contains(package_id.as_str())
                    })
                    .collect()
            } else {
                packages
            };

            // Step 2: THEN apply leaf filter if available (single-host only —
            // aggregate mode uses prevalence-based intersection instead).
            if !is_aggregate && let Some(leaf_names) = rpm.leaf_packages.as_ref() {
                let leaf_set: HashSet<&str> = leaf_names.iter().map(|s| s.as_str()).collect();

                // Ungrouped group members must bypass the leaf filter so they
                // appear as individual packages after ungrouping. Without this,
                // non-leaf group members vanish when their group is dissolved.
                let ungrouped_member_names: HashSet<&str> = self
                    .installed_groups()
                    .iter()
                    .filter(|g| {
                        self.timeline[..self.cursor].iter().any(|entry| {
                            matches!(
                                entry,
                                TimelineEntry::View(ViewDirective::UngroupGroup { group_name })
                                    if *group_name == g.name
                            )
                        })
                    })
                    .flat_map(|g| g.members.iter().map(|m| m.as_str()))
                    .collect();

                packages
                    .into_iter()
                    .filter(|pkg| {
                        let package_id =
                            canonical_package_id(pkg.entry.name.as_str(), pkg.entry.arch.as_str());

                        let original_include = original_package_includes
                            .get(&(pkg.entry.name.as_str(), pkg.entry.arch.as_str()))
                            .copied()
                            .unwrap_or(pkg.entry.include);

                        leaf_set.contains(package_id.as_str())
                            || pkg.triage.bucket() == TriageBucket::Investigate
                            || pkg.entry.include != original_include
                            || ungrouped_member_names.contains(pkg.entry.name.as_str())
                    })
                    .collect()
            } else {
                packages
            }
        } else {
            packages
        };

        // Apply the classified include/locked state to the projected snapshot
        // so the Containerfile preview matches the view.
        let mut projected = projected;
        if let Some(rpm) = &mut projected.rpm {
            let state_map: std::collections::HashMap<&str, (bool, bool)> = classified_state
                .iter()
                .map(|(key, inc, locked)| (key.as_str(), (*inc, *locked)))
                .collect();
            for pkg in &mut rpm.packages_added {
                let key = canonical_package_id(&pkg.name, &pkg.arch);
                if let Some(&(include, locked)) = state_map.get(key.as_str()) {
                    pkg.include = include;
                    pkg.locked = locked;
                }
            }
        }

        // Build RenderContext from the *filtered* package set (baseline-
        // suppressed + leaf-filtered) so group state reflects the actual
        // render surface. A group member that was filtered out must not
        // make the group look Renderable.
        let effective_packages: Vec<PackageEntry> =
            packages.iter().map(|p| p.entry.clone()).collect();
        let mut group_states = HashMap::new();

        for group in self.installed_groups() {
            // 1. Check if this group was ungrouped via a ViewDirective.
            let ungrouped = self.timeline[..self.cursor].iter().any(|entry| {
                matches!(
                    entry,
                    TimelineEntry::View(ViewDirective::UngroupGroup { group_name })
                        if *group_name == group.name
                )
            });

            // 2. Find the most recent group-level SetInclude op for this group.
            //    Scan backwards from cursor for efficiency.
            let mut group_excluded = false;
            let mut last_group_op_index: Option<usize> = None;
            let mut last_group_op_include = true;

            for (i, entry) in self.timeline[..self.cursor].iter().enumerate().rev() {
                if let TimelineEntry::Op(RefinementOp::SetInclude {
                    item_id: ItemId::Group { name },
                    include,
                }) = entry
                    && *name == group.name
                {
                    last_group_op_index = Some(i);
                    last_group_op_include = *include;
                    group_excluded = !*include;
                    break;
                }
            }

            // 3. Build divergent overrides: individual package ops AFTER the
            //    most recent group op whose include value DIVERGES from
            //    the group op's direction. Per-group, not global.
            let mut divergent_overrides: HashSet<String> = HashSet::new();

            if let Some(group_op_idx) = last_group_op_index {
                for entry in &self.timeline[group_op_idx + 1..self.cursor] {
                    if let TimelineEntry::Op(RefinementOp::SetInclude {
                        item_id: ItemId::Package { name, .. },
                        include,
                    }) = entry
                        && group.members.contains(name)
                        && *include != last_group_op_include
                    {
                        divergent_overrides.insert(name.clone());
                    }
                }
            }

            let ctx = GroupEvalContext {
                group,
                effective_packages: &effective_packages,
                ungrouped,
                group_excluded,
                divergent_overrides: &divergent_overrides,
            };

            group_states.insert(group.name.clone(), derive_group_state(&ctx));
        }

        self.cached_render_context = Some(RenderContext { group_states });

        // Preview must use the SAME root derivation as export to guarantee
        // byte-identical Containerfile output. The config tree materializer
        // computes the actual directory structure (which includes repo files,
        // GPG keys, firewall zones, etc. beyond config.files). We materialize
        // to a tempdir, read the roots, render the Containerfile, then drop
        // the tempdir.
        let preview_dir = tempfile::tempdir().expect("tempdir for preview");
        let materialized_roots = inspectah_pipeline::render::configtree::write_config_tree(
            &projected,
            preview_dir.path(),
        )
        .unwrap_or_default();
        let containerfile_preview = render_containerfile_with_originals(
            &projected,
            Some(&materialized_roots),
            &original_package_includes
                .iter()
                .map(|((n, a), &inc)| (canonical_package_id(n, a), inc))
                .collect(),
            self.cached_render_context.as_ref(),
        );
        drop(preview_dir);

        let pkg_total = packages.len();
        let pkg_included = packages.iter().filter(|p| p.entry.include).count();
        let pkg_excluded = packages.iter().filter(|p| !p.entry.include).count();
        let cfg_total = config_files.len();
        let cfg_included = config_files.iter().filter(|c| c.entry.include).count();
        let cfg_excluded = config_files.iter().filter(|c| !c.entry.include).count();

        let stats = RefineStats {
            sections: vec![
                SectionStats {
                    kind: SectionKind::Package,
                    total: pkg_total,
                    included: pkg_included,
                    excluded: pkg_excluded,
                },
                SectionStats {
                    kind: SectionKind::Config,
                    total: cfg_total,
                    included: cfg_included,
                    excluded: cfg_excluded,
                },
            ],
            needs_review_count: packages
                .iter()
                .filter(|p| p.triage.bucket() == TriageBucket::Investigate)
                .count()
                + config_files
                    .iter()
                    .filter(|c| c.triage.bucket() == TriageBucket::Investigate)
                    .count(),
            ops_applied: self.cursor,
            can_undo: self.can_undo(),
            can_redo: self.can_redo(),
            baseline_available: self.baseline_available,
        };

        self.cached_view = Some(RefinedView {
            packages,
            config_files,
            containerfile_preview,
            stats,
            generation: self.generation,
        });

        self.cached_decisions = Some(crate::projection::project_decisions(self));
    }

    /// Returns the decision projection for the current session state.
    /// Panics if called before `recompute_view()` has run.
    pub fn decisions(&self) -> &crate::projection::DecisionProjection {
        self.cached_decisions
            .as_ref()
            .expect("decisions projection is always computed after new() or mutation")
    }

    /// Returns the reference projection, computing it lazily on first access.
    /// The reference projection is derived solely from the original snapshot
    /// and is immutable across mutations.
    pub fn reference(&self) -> &crate::projection::ReferenceProjection {
        self.cached_reference
            .get_or_init(|| crate::projection::project_reference(&self.original))
    }
}

/// Render exactly the approved refine export file set to a tarball.
///
/// This is NOT `render_all()`. The pipeline's `render_all()` writes 8
/// artifacts (including report.html, README.md, secrets-review.md,
/// kickstart-suggestion.ks) that are outside the approved refine export
/// contract. This function materializes only the contracted set:
///
/// Required: inspection-snapshot.json, Containerfile, audit-report.md,
///           schema/snapshot.schema.json
/// Conditional: config/ (when snapshot has included config files),
///              env-files/ (when snapshot has env-file data)
/// Excluded: report.html, README.md, secrets-review.md,
///           kickstart-suggestion.ks, original-inspection-snapshot.json
///
/// Preview/export fidelity: both paths use `render_containerfile(snap,
/// Some(&materialized_roots))` with the same materialized root set, so
/// the exported Containerfile is byte-identical to what the preview shows.
/// Write `aggregate/variants/<path>/<hash>.content` files for every Alternative
/// config entry, drop-in, and quadlet unit in an aggregate snapshot.  Selected and
/// Only variants are materialized in the normal config tree; alternatives are
/// preserved here so the export tarball captures all variant content.
fn write_aggregate_variants(snap: &InspectionSnapshot, out: &Path) -> Result<(), RefineError> {
    use inspectah_core::types::aggregate::VariantSelection;
    use sha2::{Digest, Sha256};

    let mut wrote_any = false;

    // Config file alternatives
    if let Some(ref cfg) = snap.config {
        for entry in &cfg.files {
            if entry.variant_selection == VariantSelection::Alternative {
                let hash = format!("{:x}", Sha256::digest(entry.content.as_bytes()));
                let short = &hash[..12];
                let rel = entry.path.strip_prefix('/').unwrap_or(&entry.path);
                let dir = out.join("aggregate/variants").join(rel);
                std::fs::create_dir_all(&dir)?;
                std::fs::write(dir.join(format!("{short}.content")), &entry.content)?;
                wrote_any = true;
            }
        }
    }

    // Drop-in alternatives
    if let Some(ref svc) = snap.services {
        for dropin in &svc.drop_ins {
            if dropin.variant_selection == VariantSelection::Alternative {
                let hash = format!("{:x}", Sha256::digest(dropin.content.as_bytes()));
                let short = &hash[..12];
                let rel = dropin.path.strip_prefix('/').unwrap_or(&dropin.path);
                let dir = out.join("aggregate/variants").join(rel);
                std::fs::create_dir_all(&dir)?;
                std::fs::write(dir.join(format!("{short}.content")), &dropin.content)?;
                wrote_any = true;
            }
        }
    }

    // Quadlet alternatives
    if let Some(ref ctr) = snap.containers {
        for quadlet in &ctr.quadlet_units {
            if quadlet.variant_selection == VariantSelection::Alternative {
                let hash = format!("{:x}", Sha256::digest(quadlet.content.as_bytes()));
                let short = &hash[..12];
                let rel = quadlet.path.strip_prefix('/').unwrap_or(&quadlet.path);
                let dir = out.join("aggregate/variants").join(rel);
                std::fs::create_dir_all(&dir)?;
                std::fs::write(dir.join(format!("{short}.content")), &quadlet.content)?;
                wrote_any = true;
            }
        }
    }

    let _ = wrote_any; // suppress unused warning when no variants exist
    Ok(())
}

/// Materialize collected manifest files into the export directory under
/// `language-packages/<ecosystem>/<hash>/`. Only included items with
/// non-empty manifest_files are materialized — excluded items use
/// commented-out inline installs in the Containerfile, not COPY paths.
/// Returns `true` when the snapshot has an active redaction state that
/// warrants scrubbing secrets from exported content.
fn is_redaction_active(snap: &InspectionSnapshot) -> bool {
    matches!(
        &snap.redaction_state,
        Some(
            RedactionState::FullyRedacted { .. }
                | RedactionState::PartiallyRedacted { .. }
                | RedactionState::SensitiveRetained { .. }
        )
    )
}

/// Scrub embedded auth credentials from a URL string.
/// Replaces `://user:pass@host` with `://REDACTED@host` and
/// `://token@host` with `://REDACTED@host`.
fn scrub_url_auth(url: &str) -> String {
    // Find the authority section: everything between :// and the next @.
    // If there's no ://, or no @ after it, the URL has no embedded auth.
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let authority_start = scheme_end + 3;
    let rest = &url[authority_start..];

    let Some(at_pos) = rest.find('@') else {
        return url.to_string();
    };

    // Verify this @ is in the authority (before any /), not in a path or query.
    let slash_pos = rest.find('/').unwrap_or(rest.len());
    if at_pos > slash_pos {
        return url.to_string();
    }

    format!("{}://REDACTED@{}", &url[..scheme_end], &rest[at_pos + 1..])
}

/// Scrub auth tokens from manifest file content based on the filename.
///
/// - `requirements.txt`: `--index-url` / `--extra-index-url` lines with auth
/// - `package.json`: `"registry"` URLs with embedded auth
/// - `Gemfile`: `source` URLs with embedded auth
fn scrub_manifest_auth(filename: &str, content: &str) -> String {
    match filename {
        "requirements.txt" => scrub_requirements_txt(content),
        "package.json" => scrub_package_json(content),
        "Gemfile" => scrub_gemfile(content),
        _ => content.to_string(),
    }
}

/// Scrub `--index-url` and `--extra-index-url` lines in requirements.txt.
fn scrub_requirements_txt(content: &str) -> String {
    content
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("--index-url") || trimmed.starts_with("--extra-index-url") {
                // Split on whitespace: directive + URL
                let mut parts = trimmed.splitn(2, char::is_whitespace);
                let directive = parts.next().unwrap_or(trimmed);
                match parts.next() {
                    Some(url) => format!("{} {}", directive, scrub_url_auth(url.trim())),
                    None => line.to_string(),
                }
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Scrub `"registry"` URLs with embedded auth in package.json.
/// Operates on raw text to avoid pulling in a JSON library dependency
/// for a simple find-and-replace in URL values.
fn scrub_package_json(content: &str) -> String {
    content
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            // Match lines like: "registry": "https://user:pass@host/..."
            if trimmed.contains("\"registry\"") {
                scrub_json_url_value(line)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Scrub the URL value in a JSON key-value line.
fn scrub_json_url_value(line: &str) -> String {
    // Find the value portion after the colon that follows "registry".
    // The value is a quoted string containing a URL.
    let Some(colon_pos) = line.find(':') else {
        return line.to_string();
    };
    let after_colon = &line[colon_pos + 1..];

    // Find the opening and closing quotes of the value.
    let Some(open_quote) = after_colon.find('"') else {
        return line.to_string();
    };
    let value_start = colon_pos + 1 + open_quote + 1;
    let value_slice = &line[value_start..];
    let Some(close_quote) = value_slice.find('"') else {
        return line.to_string();
    };

    let url = &value_slice[..close_quote];
    let scrubbed = scrub_url_auth(url);
    format!(
        "{}\"{}\"{}",
        &line[..value_start],
        scrubbed,
        &line[value_start + close_quote..]
    )
}

/// Scrub `source` URLs with embedded auth in Gemfile.
fn scrub_gemfile(content: &str) -> String {
    content
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("source") {
                // source "https://user:pass@gems.example.com"
                // source 'https://user:pass@gems.example.com'
                let quote_chars = ['"', '\''];
                let mut result = line.to_string();
                for q in &quote_chars {
                    if let Some(open) = trimmed.find(*q)
                        && let Some(close) = trimmed[open + 1..].find(*q)
                    {
                        let url = &trimmed[open + 1..open + 1 + close];
                        let scrubbed = scrub_url_auth(url);
                        // Reconstruct preserving original indentation.
                        let indent = &line[..line.len() - trimmed.len()];
                        result = format!("{}{}", indent, trimmed.replace(url, &scrubbed));
                        break;
                    }
                }
                result
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn write_language_package_manifests(
    snap: &InspectionSnapshot,
    out: &Path,
) -> Result<(), RefineError> {
    let nrs = match &snap.non_rpm_software {
        Some(n) => n,
        None => return Ok(()),
    };

    let redact = is_redaction_active(snap);

    for item in &nrs.items {
        if !item.include || item.manifest_files.is_empty() {
            continue;
        }

        // Route to ecosystem subdirectory based on the method string —
        // the canonical detection-method key used throughout the pipeline.
        let ecosystem = if item.method.contains("pip") || item.method == "venv" {
            "pip"
        } else if item.method == "npm lockfile" {
            "npm"
        } else if item.method == "gem lockfile" {
            "gem"
        } else {
            continue;
        };

        let hash = env_hash(&item.path);
        let dir = out.join("language-packages").join(ecosystem).join(&hash);
        std::fs::create_dir_all(&dir)
            .map_err(|e| RefineError::RenderFailed(format!("mkdir {}: {e}", dir.display())))?;

        for (filename, content) in &item.manifest_files {
            let output = if redact {
                scrub_manifest_auth(filename, content)
            } else {
                content.clone()
            };
            let file_path = dir.join(filename);
            std::fs::write(&file_path, output).map_err(|e| {
                RefineError::RenderFailed(format!("write {}: {e}", file_path.display()))
            })?;
        }
    }
    Ok(())
}

pub fn render_refine_export(
    snap: &InspectionSnapshot,
    tarball_path: &Path,
    original_includes: Option<&std::collections::HashMap<String, bool>>,
    render_ctx: Option<&RenderContext>,
) -> Result<(), RefineError> {
    let tempdir = tempfile::tempdir().map_err(|e| RefineError::TarballError(e.to_string()))?;
    let out = tempdir.path();

    // 1. Materialize config tree FIRST -- gives us materialized_roots,
    //    the renderer's single source of truth for COPY lines.
    let materialized_roots = inspectah_pipeline::render::configtree::write_config_tree(snap, out)
        .map_err(|e| RefineError::RenderFailed(e.to_string()))?;

    // 2. Materialize env-files (conditional)
    inspectah_pipeline::render::configtree::write_env_files(snap, out)
        .map_err(|e| RefineError::RenderFailed(e.to_string()))?;

    // 2b. User artifacts (conditional — only when users_groups has data)
    let users_ks = inspectah_pipeline::render::users::render_kickstart(snap);
    if !users_ks.is_empty() {
        std::fs::write(out.join("inspectah-users.ks"), users_ks)?;
    }
    let users_toml = inspectah_pipeline::render::users::render_blueprint_toml(snap);
    if !users_toml.is_empty() {
        std::fs::write(out.join("inspectah-users.toml"), users_toml)?;
    }
    inspectah_pipeline::render::users::stage_ssh_keys(snap, out)
        .map_err(|e| RefineError::RenderFailed(format!("stage SSH keys: {e}")))?;

    // 2c. Materialize aggregate variant files (conditional — only for aggregate snapshots
    //     with Alternative config entries, drop-ins, or quadlet units).
    if snap.aggregate_meta.is_some() {
        write_aggregate_variants(snap, out)?;
    }

    // 2d. Materialize language-package manifests (conditional — only when
    //     non-RPM items with include=true have collected manifest files).
    write_language_package_manifests(snap, out)?;

    // 2e. Remove any top-level artifacts outside the approved export contract.
    //     "quadlet" is intentionally excluded — quadlet units are written by
    //     write_config_tree as a side effect but are NOT part of the refine
    //     export contract. The Containerfile references them via config/ paths.
    let allowed_top_level: std::collections::HashSet<&str> = [
        "config",
        "drop-ins",
        "flatpak",
        "sysctl",
        "tuned",
        "env-files",
        "aggregate",
        "schema",
        "users",
        "inspection-snapshot.json",
        "Containerfile",
        "audit-report.md",
        "inspectah-users.ks",
        "inspectah-users.toml",
        "language-packages",
    ]
    .iter()
    .copied()
    .collect();

    for entry in std::fs::read_dir(out)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !allowed_top_level.contains(name_str.as_ref()) {
            if entry.file_type()?.is_dir() {
                std::fs::remove_dir_all(entry.path())?;
            } else {
                std::fs::remove_file(entry.path())?;
            }
        }
    }

    // 3. Containerfile -- uses materialized_roots from the SAME config
    //    tree write that populated the tarball's config/ directory.
    //    Preview also materializes to a tempdir for the same roots,
    //    guaranteeing byte-identical output.
    let containerfile = if let Some(orig) = original_includes {
        render_containerfile_with_originals(snap, Some(&materialized_roots), orig, render_ctx)
    } else {
        render_containerfile(snap, Some(&materialized_roots), render_ctx)
    };
    std::fs::write(out.join("Containerfile"), containerfile)?;

    // 4. audit-report.md
    let audit = inspectah_pipeline::render::audit::render_audit(snap);
    std::fs::write(out.join("audit-report.md"), audit)?;

    // 5. inspection-snapshot.json (projected)
    let snap_json =
        serde_json::to_string_pretty(snap).map_err(|e| RefineError::TarballError(e.to_string()))?;
    std::fs::write(out.join("inspection-snapshot.json"), snap_json)?;

    // 6. schema/snapshot.schema.json (placeholder -- same as scan.rs)
    let schema_dir = out.join("schema");
    std::fs::create_dir_all(&schema_dir)?;
    std::fs::write(
        schema_dir.join("snapshot.schema.json"),
        r#"{"$schema":"http://json-schema.org/draft-07/schema#","title":"InspectionSnapshot","description":"Phase 7 placeholder","type":"object"}"#,
    )?;

    // 7. Create tarball with a top-level directory matching the output stem.
    //    e.g. "foo-refined.tar.gz" → prefix "foo-refined", so extraction
    //    produces foo-refined/Containerfile, foo-refined/config/, etc.
    let stem = tarball_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let prefix = stem
        .strip_suffix(".tar.gz")
        .or_else(|| stem.strip_suffix(".tgz"))
        .unwrap_or(&stem);
    inspectah_pipeline::render::tarball::create_tarball(out, tarball_path, prefix)
        .map_err(|e| RefineError::TarballError(e.to_string()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::UserPasswordOp;
    use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
    use inspectah_core::types::users::UserGroupSection;

    /// Build a minimal snapshot suitable for RefineSession tests.
    fn test_snapshot() -> InspectionSnapshot {
        InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection::default()),
            ..Default::default()
        }
    }

    #[test]
    fn session_timeline_migration_preserves_existing_ops() {
        let mut snap = test_snapshot();
        // Need baseline so the package gets Site bucket and stays include=true
        // after normalization, making SetInclude(false) a real change.
        snap.baseline = Some(inspectah_core::baseline::BaselineData {
            image_digest: "sha256:test".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });
        let rpm = snap.rpm.as_mut().unwrap();
        rpm.packages_added = vec![PackageEntry {
            name: "httpd".into(),
            arch: "x86_64".into(),
            include: true,
            locked: false,
            source_repo: "appstream".into(),
            ..Default::default()
        }];

        let mut session = RefineSession::new(snap);
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Package {
                    name: "httpd".into(),
                    arch: "x86_64".into(),
                },
                include: false,
            })
            .unwrap();
        assert_eq!(session.timeline_len(), 1);
        assert!(session.can_undo());

        // Undo should work and reduce cursor, timeline_len stays the same
        session.undo().unwrap();
        assert_eq!(session.timeline_len(), 1);
        assert!(!session.can_undo());
        assert!(session.can_redo());

        // Redo should work
        session.redo().unwrap();
        assert_eq!(session.timeline_len(), 1);
        assert!(session.can_undo());
        assert!(!session.can_redo());

        // ops_history should extract the op correctly
        let history = session.ops_history();
        assert_eq!(history.len(), 1);
        assert!(history[0].active);
    }

    #[test]
    fn view_filters_to_canonical_leaf_packages_when_available() {
        let mut snap = test_snapshot();
        // Need baseline so packages get Site bucket (user-added), not Investigate.
        // Leaf filtering only applies to Site, not Investigate.
        snap.baseline = Some(inspectah_core::baseline::BaselineData {
            image_digest: "sha256:test".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });
        let rpm = snap.rpm.as_mut().unwrap();
        rpm.packages_added = vec![
            PackageEntry {
                name: "glibc".into(),
                arch: "x86_64".into(),
                include: true,
                locked: false,
                source_repo: "baseos".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "glibc".into(),
                arch: "i686".into(),
                include: true,
                locked: false,
                source_repo: "baseos".into(),
                ..Default::default()
            },
        ];
        rpm.leaf_packages = Some(vec!["glibc.x86_64".into()]);
        rpm.auto_packages = Some(vec!["glibc.i686".into()]);

        let session = RefineSession::new(snap);
        let view = session.view();

        // View should only contain the canonical leaf package.
        assert_eq!(view.packages.len(), 1);
        assert_eq!(view.packages[0].entry.name, "glibc");
        assert_eq!(view.packages[0].entry.arch, "x86_64");
    }

    #[test]
    fn view_shows_all_packages_when_leaf_data_unavailable() {
        let mut snap = test_snapshot();
        let rpm = snap.rpm.as_mut().unwrap();
        rpm.packages_added = vec![
            PackageEntry {
                name: "vim".into(),
                arch: "x86_64".into(),
                include: true,
                locked: false,
                source_repo: "appstream".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "glibc".into(),
                arch: "x86_64".into(),
                include: true,
                locked: false,
                source_repo: "baseos".into(),
                ..Default::default()
            },
        ];
        rpm.leaf_packages = None; // No leaf data

        let session = RefineSession::new(snap);
        let view = session.view();

        // All packages visible (degraded mode)
        assert_eq!(view.packages.len(), 2);
        assert_eq!(view.stats.total_packages(), 2);
    }

    #[test]
    fn containerfile_preview_only_includes_leaf_packages() {
        let mut snap = test_snapshot();
        snap.baseline = Some(inspectah_core::baseline::BaselineData {
            image_digest: "sha256:test".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });
        let rpm = snap.rpm.as_mut().unwrap();
        rpm.packages_added = vec![
            PackageEntry {
                name: "vim".into(),
                arch: "x86_64".into(),
                include: true,
                locked: false,
                source_repo: "appstream".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "glibc".into(),
                arch: "x86_64".into(),
                include: true,
                locked: false,
                source_repo: "baseos".into(),
                ..Default::default()
            },
        ];
        rpm.leaf_packages = Some(vec!["vim.x86_64".into()]);
        rpm.auto_packages = Some(vec!["glibc.x86_64".into()]);

        let session = RefineSession::new(snap);
        let view = session.view();

        // Containerfile should contain vim but not glibc
        assert!(
            view.containerfile_preview.contains("vim"),
            "containerfile should contain leaf package 'vim'"
        );
        assert!(
            !view.containerfile_preview.contains("glibc"),
            "containerfile should NOT contain auto package 'glibc'"
        );
    }

    #[test]
    fn view_stats_respect_canonical_leaf_identity() {
        let mut snap = test_snapshot();
        snap.baseline = Some(inspectah_core::baseline::BaselineData {
            image_digest: "sha256:test".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });
        let rpm = snap.rpm.as_mut().unwrap();
        rpm.packages_added = vec![
            PackageEntry {
                name: "glibc".into(),
                arch: "x86_64".into(),
                include: true,
                locked: false,
                source_repo: "baseos".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "glibc".into(),
                arch: "i686".into(),
                include: true,
                locked: false,
                source_repo: "baseos".into(),
                ..Default::default()
            },
        ];
        rpm.leaf_packages = Some(vec!["glibc.x86_64".into()]);
        rpm.auto_packages = Some(vec!["glibc.i686".into()]);

        let session = RefineSession::new(snap);
        let view = session.view();

        // Stats should reflect only the matching canonical package identity.
        assert_eq!(
            view.stats.total_packages(),
            1,
            "total_packages should be leaf count"
        );
        assert_eq!(
            view.stats.included_packages(),
            1,
            "included_packages should be leaf count"
        );
    }

    #[test]
    fn baseline_suppressed_excluded_from_view_even_if_needs_review() {
        let mut snap = test_snapshot();
        let rpm = snap.rpm.as_mut().unwrap();
        rpm.packages_added = vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                include: true,
                locked: false,
                source_repo: "appstream".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "kernel".into(),
                arch: "x86_64".into(),
                include: true,
                locked: false,
                source_repo: "baseos".into(),
                state: PackageState::Modified,
                ..Default::default()
            },
        ];
        rpm.leaf_packages = Some(vec!["httpd.x86_64".into()]);
        rpm.auto_packages = Some(Vec::new());
        rpm.baseline_suppressed = Some(vec!["kernel.x86_64".into()]);

        let session = RefineSession::new(snap);
        let view = session.view();

        assert_eq!(view.packages.len(), 1);
        assert_eq!(view.packages[0].entry.name, "httpd");
        assert!(
            !view.packages.iter().any(|p| p.entry.name == "kernel"),
            "baseline-suppressed package must not appear in view"
        );
    }

    #[test]
    fn needs_review_count_stable_with_baseline_suppression() {
        let mut snap = test_snapshot();
        let rpm = snap.rpm.as_mut().unwrap();
        rpm.packages_added = vec![
            PackageEntry {
                name: "vim".into(),
                arch: "x86_64".into(),
                include: true,
                locked: false,
                source_repo: "appstream".into(),
                state: PackageState::LocalInstall,
                ..Default::default()
            },
            PackageEntry {
                name: "kernel".into(),
                arch: "x86_64".into(),
                include: true,
                locked: false,
                source_repo: "baseos".into(),
                state: PackageState::Modified,
                ..Default::default()
            },
        ];
        rpm.leaf_packages = Some(vec!["vim.x86_64".into()]);
        rpm.auto_packages = Some(Vec::new());
        rpm.baseline_suppressed = Some(vec!["kernel.x86_64".into()]);

        let session = RefineSession::new(snap);
        let view = session.view();

        // Only vim (LocalInstall) should be counted, not kernel (Modified but suppressed)
        assert_eq!(
            view.stats.needs_review_count, 1,
            "needs_review_count should exclude baseline-suppressed packages"
        );
    }

    #[test]
    fn containerfile_excludes_baseline_suppressed_packages() {
        let mut snap = test_snapshot();
        snap.baseline = Some(inspectah_core::baseline::BaselineData {
            image_digest: "sha256:test".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });
        let rpm = snap.rpm.as_mut().unwrap();
        rpm.packages_added = vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                include: true,
                locked: false,
                source_repo: "appstream".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "kernel".into(),
                arch: "x86_64".into(),
                include: true,
                locked: false,
                source_repo: "baseos".into(),
                state: PackageState::Modified,
                ..Default::default()
            },
        ];
        rpm.leaf_packages = Some(vec!["httpd.x86_64".into()]);
        rpm.auto_packages = Some(Vec::new());
        rpm.baseline_suppressed = Some(vec!["kernel.x86_64".into()]);

        let session = RefineSession::new(snap);
        let view = session.view();

        assert!(
            view.containerfile_preview.contains("httpd"),
            "containerfile should contain leaf package 'httpd'"
        );
        assert!(
            !view.containerfile_preview.contains("kernel"),
            "containerfile should NOT contain baseline-suppressed package 'kernel'"
        );
    }

    #[test]
    fn test_baseline_suppressed_excluded_even_when_leaf_packages_unavailable() {
        let mut snap = test_snapshot();
        let rpm = snap.rpm.as_mut().unwrap();
        rpm.packages_added = vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                include: true,
                locked: false,
                source_repo: "appstream".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "kernel".into(),
                arch: "x86_64".into(),
                include: true,
                locked: false,
                source_repo: "baseos".into(),
                ..Default::default()
            },
        ];
        // Degraded mode: no leaf data
        rpm.leaf_packages = None;
        rpm.auto_packages = None;
        // But baseline suppression IS available
        rpm.baseline_suppressed = Some(vec!["kernel.x86_64".into()]);

        let session = RefineSession::new(snap);
        let view = session.view();

        // kernel should be excluded even though leaf filter is disabled
        assert!(
            !view.packages.iter().any(|p| p.entry.name == "kernel"),
            "baseline-suppressed package must not appear even in degraded mode"
        );
        assert!(view.packages.iter().any(|p| p.entry.name == "httpd"));
    }

    /// Build a snapshot with a users_groups section containing one user.
    fn test_snapshot_with_user() -> InspectionSnapshot {
        InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection::default()),
            users_groups: Some(UserGroupSection {
                users: vec![serde_json::json!({
                    "name": "alice",
                    "uid": 1001,
                    "gid": 1001,
                    "include": true,
                    "containerfile_strategy": "skip",
                    "password_choice": "none",
                    "password_hash": "$6$original_hash",
                    "home": "/home/alice",
                    "shell": "/bin/bash"
                })],
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn user_strategy_op_projects_useradd() {
        let snap = test_snapshot_with_user();
        let mut session = RefineSession::new(snap);

        session
            .apply(RefinementOp::UserStrategy {
                username: "alice".into(),
                strategy: inspectah_core::types::users::UserContainerfileStrategy::Useradd,
            })
            .unwrap();

        let projected = session.snapshot_projected();
        let user = projected
            .users_groups
            .as_ref()
            .unwrap()
            .users
            .iter()
            .find(|u| u.get("name").and_then(|v| v.as_str()) == Some("alice"))
            .unwrap();

        assert_eq!(
            user.get("containerfile_strategy").and_then(|v| v.as_str()),
            Some("useradd"),
            "UserStrategy op must set containerfile_strategy to useradd"
        );
    }

    #[test]
    fn user_password_none_clears_hash() {
        let snap = test_snapshot_with_user();
        let mut session = RefineSession::new(snap);

        session
            .apply(RefinementOp::UserPassword(UserPasswordOp::None {
                username: "alice".into(),
            }))
            .unwrap();

        let projected = session.snapshot_projected();
        let user = projected
            .users_groups
            .as_ref()
            .unwrap()
            .users
            .iter()
            .find(|u| u.get("name").and_then(|v| v.as_str()) == Some("alice"))
            .unwrap();

        assert_eq!(
            user.get("password_choice").and_then(|v| v.as_str()),
            Some("none"),
            "password_choice must be 'none'"
        );
        assert!(
            user.get("password_hash").is_none(),
            "password_hash must be cleared when password_choice is 'none'"
        );
    }

    #[test]
    fn preserve_after_new_restores_original_hash() {
        let snap = test_snapshot_with_user();
        let mut session = RefineSession::new(snap);

        // Step 1: Set a NEW password hash
        session
            .apply(RefinementOp::UserPassword(UserPasswordOp::New {
                username: "alice".into(),
                hash: Some("$6$new_hash_value".into()),
            }))
            .unwrap();

        // Verify new hash is in place
        let projected = session.snapshot_projected();
        let user = projected
            .users_groups
            .as_ref()
            .unwrap()
            .users
            .iter()
            .find(|u| u.get("name").and_then(|v| v.as_str()) == Some("alice"))
            .unwrap();
        assert_eq!(
            user.get("password_hash").and_then(|v| v.as_str()),
            Some("$6$new_hash_value"),
            "after New op, hash must be the new value"
        );

        // Step 2: Preserve — must restore the ORIGINAL hash, not the new one
        session
            .apply(RefinementOp::UserPassword(UserPasswordOp::Preserve {
                username: "alice".into(),
            }))
            .unwrap();

        let projected = session.snapshot_projected();
        let user = projected
            .users_groups
            .as_ref()
            .unwrap()
            .users
            .iter()
            .find(|u| u.get("name").and_then(|v| v.as_str()) == Some("alice"))
            .unwrap();

        assert_eq!(
            user.get("password_choice").and_then(|v| v.as_str()),
            Some("preserve"),
            "password_choice must be 'preserve'"
        );
        assert_eq!(
            user.get("password_hash").and_then(|v| v.as_str()),
            Some("$6$original_hash"),
            "Preserve must restore the ORIGINAL scan-time hash, not the projected (new) hash"
        );
    }

    #[test]
    fn new_password_triggers_sensitive_on_projected_state() {
        let snap = test_snapshot_with_user();
        let mut session = RefineSession::new(snap);

        // Before any password ops, not sensitive
        assert!(
            !session.is_sensitive(),
            "session must not be sensitive before any password ops"
        );

        // Set a new password hash
        session
            .apply(RefinementOp::UserPassword(UserPasswordOp::New {
                username: "alice".into(),
                hash: Some("$6$new_secret".into()),
            }))
            .unwrap();

        assert!(
            session.is_sensitive(),
            "session must be sensitive after setting a new password hash"
        );

        // Switch to None — clears hash, no longer sensitive
        session
            .apply(RefinementOp::UserPassword(UserPasswordOp::None {
                username: "alice".into(),
            }))
            .unwrap();

        assert!(
            !session.is_sensitive(),
            "session must not be sensitive after clearing password"
        );
    }

    #[test]
    fn partially_redacted_upgrades_to_sensitive_retained() {
        use inspectah_core::types::redaction::{Confidence, RedactionHint};

        let mut snap = test_snapshot_with_user();
        snap.redaction_state = Some(RedactionState::PartiallyRedacted {
            redacted_by: "inspectah 0.8.0".into(),
            config_hash: "abc".into(),
            unresolved_count: 1,
            unresolved_hints: vec![RedactionHint {
                path: "/etc/foo".into(),
                reason: "test hint".into(),
                confidence: Some(Confidence::High),
            }],
        });

        let mut session = RefineSession::new(snap);

        session
            .apply(RefinementOp::UserPassword(UserPasswordOp::New {
                username: "alice".into(),
                hash: Some("$6$salt$hash".into()),
            }))
            .unwrap();

        let projected = session.snapshot_projected();
        assert!(
            projected.sensitive_snapshot,
            "snapshot must be sensitive after new password"
        );
        match &projected.redaction_state {
            Some(RedactionState::SensitiveRetained {
                unresolved_count,
                unresolved_hints,
                ..
            }) => {
                assert_eq!(
                    *unresolved_count, 1,
                    "must carry forward unresolved count from PartiallyRedacted"
                );
                assert_eq!(
                    unresolved_hints.len(),
                    1,
                    "must carry forward unresolved hints from PartiallyRedacted"
                );
                assert_eq!(unresolved_hints[0].path, "/etc/foo");
            }
            other => panic!("expected SensitiveRetained, got {other:?}"),
        }
    }

    // --- Service refinement tests ---

    use inspectah_core::types::services::{
        PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState, SystemdDropIn,
    };

    /// Build a snapshot with services and drop-ins for SetInclude tests.
    fn test_snapshot_with_services() -> InspectionSnapshot {
        InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection::default()),
            services: Some(ServiceSection {
                state_changes: vec![
                    ServiceStateChange {
                        unit: "httpd.service".into(),
                        current_state: ServiceUnitState::Enabled,
                        default_state: Some(PresetDefault::Disable),
                        include: true,
                        locked: false,
                        owning_package: Some("httpd".into()),
                        aggregate: None,
                        attention_reason: None,
                    },
                    ServiceStateChange {
                        unit: "sshd.service".into(),
                        current_state: ServiceUnitState::Enabled,
                        default_state: Some(PresetDefault::Enable),
                        include: true,
                        locked: false,
                        owning_package: Some("openssh-server".into()),
                        aggregate: None,
                        attention_reason: None,
                    },
                ],
                enabled_units: vec!["httpd.service".into(), "sshd.service".into()],
                disabled_units: vec![],
                drop_ins: vec![
                    SystemdDropIn {
                        unit: "httpd.service".into(),
                        path: "/etc/systemd/system/httpd.service.d/limits.conf".into(),
                        content: "[Service]\nLimitNOFILE=65536".into(),
                        include: true,
                        locked: false,
                        ..Default::default()
                    },
                    SystemdDropIn {
                        unit: "httpd.service".into(),
                        path: "/etc/systemd/system/httpd.service.d/timeout.conf".into(),
                        content: "[Service]\nTimeoutStartSec=120".into(),
                        include: true,
                        locked: false,
                        ..Default::default()
                    },
                ],
                preset_matched_units: vec![],
            }),
            ..Default::default()
        }
    }

    #[test]
    fn exclude_service_cascades_to_drop_ins() {
        let snap = test_snapshot_with_services();
        let mut session = RefineSession::new(snap);

        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Service {
                    unit: "httpd.service".into(),
                },
                include: false,
            })
            .unwrap();

        let projected = session.snapshot_projected();
        let svc = projected.services.as_ref().unwrap();

        // Service itself must be excluded.
        let httpd = svc
            .state_changes
            .iter()
            .find(|s| s.unit == "httpd.service")
            .unwrap();
        assert!(!httpd.include, "httpd.service must be excluded");

        // Both drop-ins for httpd must also be excluded.
        for dropin in svc.drop_ins.iter().filter(|d| d.unit == "httpd.service") {
            assert!(
                !dropin.include,
                "drop-in {} must be excluded when parent service is excluded",
                dropin.path
            );
        }

        // sshd.service must remain unaffected.
        let sshd = svc
            .state_changes
            .iter()
            .find(|s| s.unit == "sshd.service")
            .unwrap();
        assert!(sshd.include, "sshd.service must remain included");
    }

    #[test]
    fn re_include_service_re_includes_drop_ins() {
        let snap = test_snapshot_with_services();
        let mut session = RefineSession::new(snap);

        // Exclude first.
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Service {
                    unit: "httpd.service".into(),
                },
                include: false,
            })
            .unwrap();

        // Re-include.
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Service {
                    unit: "httpd.service".into(),
                },
                include: true,
            })
            .unwrap();

        let projected = session.snapshot_projected();
        let svc = projected.services.as_ref().unwrap();

        let httpd = svc
            .state_changes
            .iter()
            .find(|s| s.unit == "httpd.service")
            .unwrap();
        assert!(httpd.include, "httpd.service must be re-included");

        for dropin in svc.drop_ins.iter().filter(|d| d.unit == "httpd.service") {
            assert!(
                dropin.include,
                "drop-in {} must be re-included when parent service is re-included",
                dropin.path
            );
        }
    }

    #[test]
    fn exclude_individual_drop_in_while_parent_included() {
        let snap = test_snapshot_with_services();
        let mut session = RefineSession::new(snap);

        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::DropIn {
                    path: "/etc/systemd/system/httpd.service.d/limits.conf".into(),
                },
                include: false,
            })
            .unwrap();

        let projected = session.snapshot_projected();
        let svc = projected.services.as_ref().unwrap();

        // Parent service stays included.
        let httpd = svc
            .state_changes
            .iter()
            .find(|s| s.unit == "httpd.service")
            .unwrap();
        assert!(httpd.include, "parent service must remain included");

        // Only the targeted drop-in is excluded.
        let limits = svc
            .drop_ins
            .iter()
            .find(|d| d.path.contains("limits.conf"))
            .unwrap();
        assert!(!limits.include, "limits.conf drop-in must be excluded");

        let timeout = svc
            .drop_ins
            .iter()
            .find(|d| d.path.contains("timeout.conf"))
            .unwrap();
        assert!(timeout.include, "timeout.conf drop-in must remain included");
    }

    #[test]
    fn include_drop_in_when_parent_excluded_is_error() {
        let snap = test_snapshot_with_services();
        let mut session = RefineSession::new(snap);

        // Exclude the parent service (cascades to drop-ins).
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Service {
                    unit: "httpd.service".into(),
                },
                include: false,
            })
            .unwrap();

        // Attempt to include a drop-in while parent is excluded.
        let result = session.apply(RefinementOp::SetInclude {
            item_id: ItemId::DropIn {
                path: "/etc/systemd/system/httpd.service.d/limits.conf".into(),
            },
            include: true,
        });

        assert!(
            result.is_err(),
            "including drop-in with excluded parent must fail"
        );
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("parent service is excluded"),
            "error must mention parent service, got: {err}"
        );
    }

    #[test]
    fn unknown_service_unit_is_error() {
        let snap = test_snapshot_with_services();
        let mut session = RefineSession::new(snap);

        let result = session.apply(RefinementOp::SetInclude {
            item_id: ItemId::Service {
                unit: "nonexistent.service".into(),
            },
            include: false,
        });

        assert!(result.is_err(), "unknown service unit must be rejected");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("nonexistent.service"),
            "error must name the unknown target, got: {err}"
        );
    }

    #[test]
    fn unknown_drop_in_path_is_error() {
        let snap = test_snapshot_with_services();
        let mut session = RefineSession::new(snap);

        let result = session.apply(RefinementOp::SetInclude {
            item_id: ItemId::DropIn {
                path: "/etc/systemd/system/ghost.service.d/nope.conf".into(),
            },
            include: false,
        });

        assert!(result.is_err(), "unknown drop-in path must be rejected");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("nope.conf"),
            "error must name the unknown target, got: {err}"
        );
    }

    // ── Container (Quadlet / Flatpak) SetInclude tests ──────────────

    use inspectah_core::types::containers::{ContainerSection, FlatpakApp, QuadletUnit};

    /// Build a snapshot with quadlets and flatpaks for SetInclude tests.
    fn test_snapshot_with_containers() -> InspectionSnapshot {
        InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection::default()),
            containers: Some(ContainerSection {
                quadlet_units: vec![
                    QuadletUnit {
                        path: "/etc/containers/systemd/myapp.container".into(),
                        name: "myapp.container".into(),
                        image: "quay.io/myorg/myapp:latest".into(),
                        include: true,
                        locked: false,
                        ..Default::default()
                    },
                    QuadletUnit {
                        path: "/etc/containers/systemd/db.container".into(),
                        name: "db.container".into(),
                        image: "docker.io/library/postgres:16".into(),
                        include: true,
                        locked: false,
                        ..Default::default()
                    },
                ],
                flatpak_apps: vec![
                    FlatpakApp {
                        app_id: "org.mozilla.firefox".into(),
                        remote: "flathub".into(),
                        branch: "stable".into(),
                        include: true,
                        locked: false,
                        ..Default::default()
                    },
                    FlatpakApp {
                        app_id: "org.gimp.GIMP".into(),
                        remote: "flathub".into(),
                        branch: "stable".into(),
                        include: true,
                        locked: false,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn exclude_quadlet_sets_include_false() {
        let snap = test_snapshot_with_containers();
        let mut session = RefineSession::new(snap);

        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Quadlet {
                    path: "/etc/containers/systemd/myapp.container".into(),
                },
                include: false,
            })
            .unwrap();

        let projected = session.snapshot_projected();
        let containers = projected.containers.as_ref().unwrap();

        let myapp = containers
            .quadlet_units
            .iter()
            .find(|q| q.path.contains("myapp"))
            .unwrap();
        assert!(!myapp.include, "myapp quadlet must be excluded");

        // Other quadlet must be unaffected.
        let db = containers
            .quadlet_units
            .iter()
            .find(|q| q.path.contains("db"))
            .unwrap();
        assert!(db.include, "db quadlet must remain included");
    }

    #[test]
    fn exclude_flatpak_sets_include_false() {
        let snap = test_snapshot_with_containers();
        let mut session = RefineSession::new(snap);

        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Flatpak {
                    app_id: "org.mozilla.firefox".into(),
                    remote: "flathub".into(),
                    branch: "stable".into(),
                },
                include: false,
            })
            .unwrap();

        let projected = session.snapshot_projected();
        let containers = projected.containers.as_ref().unwrap();

        let firefox = containers
            .flatpak_apps
            .iter()
            .find(|f| f.app_id == "org.mozilla.firefox")
            .unwrap();
        assert!(!firefox.include, "firefox flatpak must be excluded");

        // Other flatpak must be unaffected.
        let gimp = containers
            .flatpak_apps
            .iter()
            .find(|f| f.app_id == "org.gimp.GIMP")
            .unwrap();
        assert!(gimp.include, "GIMP flatpak must remain included");
    }

    #[test]
    fn unknown_quadlet_path_is_error() {
        let snap = test_snapshot_with_containers();
        let mut session = RefineSession::new(snap);

        let result = session.apply(RefinementOp::SetInclude {
            item_id: ItemId::Quadlet {
                path: "/etc/containers/systemd/ghost.container".into(),
            },
            include: false,
        });

        assert!(result.is_err(), "unknown quadlet path must be rejected");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("ghost.container"),
            "error must name the unknown target, got: {err}"
        );
    }

    #[test]
    fn unknown_flatpak_is_error() {
        let snap = test_snapshot_with_containers();
        let mut session = RefineSession::new(snap);

        let result = session.apply(RefinementOp::SetInclude {
            item_id: ItemId::Flatpak {
                app_id: "org.ghost.App".into(),
                remote: "flathub".into(),
                branch: "stable".into(),
            },
            include: false,
        });

        assert!(result.is_err(), "unknown flatpak must be rejected");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("org.ghost.App"),
            "error must name the unknown target, got: {err}"
        );
    }

    // ── Sysctl / Tuned SetInclude tests ───────────────────────────────

    use inspectah_core::types::kernelboot::{KernelBootSection, SysctlOverride};

    /// Build a snapshot with sysctl overrides and tuned profile for SetInclude tests.
    fn test_snapshot_with_kernel_boot() -> InspectionSnapshot {
        InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection::default()),
            kernel_boot: Some(KernelBootSection {
                sysctl_overrides: vec![
                    SysctlOverride {
                        key: "net.ipv4.ip_forward".into(),
                        runtime: "1".into(),
                        default: "0".into(),
                        source: "/etc/sysctl.d/99-custom.conf".into(),
                        include: true,
                        locked: false,
                        aggregate: None,
                    },
                    SysctlOverride {
                        key: "vm.swappiness".into(),
                        runtime: "10".into(),
                        default: "60".into(),
                        source: "/etc/sysctl.d/99-custom.conf".into(),
                        include: true,
                        locked: false,
                        aggregate: None,
                    },
                ],
                tuned_active: "throughput-performance".into(),
                tuned_include: true,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn exclude_sysctl_sets_include_false() {
        let snap = test_snapshot_with_kernel_boot();
        let mut session = RefineSession::new(snap);

        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Sysctl {
                    key: "net.ipv4.ip_forward".into(),
                },
                include: false,
            })
            .unwrap();

        let projected = session.snapshot_projected();
        let kb = projected.kernel_boot.as_ref().unwrap();

        let ip_fwd = kb
            .sysctl_overrides
            .iter()
            .find(|s| s.key == "net.ipv4.ip_forward")
            .unwrap();
        assert!(!ip_fwd.include, "ip_forward sysctl must be excluded");

        // Other sysctl must be unaffected.
        let swappiness = kb
            .sysctl_overrides
            .iter()
            .find(|s| s.key == "vm.swappiness")
            .unwrap();
        assert!(swappiness.include, "vm.swappiness must remain included");
    }

    #[test]
    fn exclude_tuned_sets_include_false() {
        let snap = test_snapshot_with_kernel_boot();
        let mut session = RefineSession::new(snap);

        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::TunedSelection {
                    profile: "throughput-performance".into(),
                },
                include: false,
            })
            .unwrap();

        let projected = session.snapshot_projected();
        let kb = projected.kernel_boot.as_ref().unwrap();

        assert!(
            !kb.tuned_include,
            "tuned_include must be false after excluding tuned profile"
        );
        assert_eq!(
            kb.tuned_active, "throughput-performance",
            "tuned_active profile name must be preserved"
        );
    }

    #[test]
    fn unknown_sysctl_key_is_error() {
        let snap = test_snapshot_with_kernel_boot();
        let mut session = RefineSession::new(snap);

        let result = session.apply(RefinementOp::SetInclude {
            item_id: ItemId::Sysctl {
                key: "kernel.nonexistent".into(),
            },
            include: false,
        });

        assert!(result.is_err(), "unknown sysctl key must be rejected");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("kernel.nonexistent"),
            "error must name the unknown target, got: {err}"
        );
    }

    #[test]
    fn unknown_tuned_profile_is_error() {
        let snap = test_snapshot_with_kernel_boot();
        let mut session = RefineSession::new(snap);

        let result = session.apply(RefinementOp::SetInclude {
            item_id: ItemId::TunedSelection {
                profile: "balanced".into(),
            },
            include: false,
        });

        assert!(result.is_err(), "unknown tuned profile must be rejected");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("balanced"),
            "error must name the unknown target, got: {err}"
        );
    }

    #[test]
    fn export_tarball_includes_promoted_roots() {
        use inspectah_core::types::containers::{ContainerSection, FlatpakApp, QuadletUnit};
        use inspectah_core::types::kernelboot::{ConfigSnippet, KernelBootSection, SysctlOverride};
        use inspectah_core::types::services::{ServiceSection, SystemdDropIn};

        let mut snap = test_snapshot();

        // Service drop-in → drop-ins/ root
        snap.services = Some(ServiceSection {
            drop_ins: vec![SystemdDropIn {
                unit: "httpd.service".into(),
                path: "etc/systemd/system/httpd.service.d/limits.conf".into(),
                content: "[Service]\nLimitNOFILE=65535".into(),
                include: true,
                locked: false,
                ..Default::default()
            }],
            ..Default::default()
        });

        // Quadlet → quadlet/ root
        snap.containers = Some(ContainerSection {
            quadlet_units: vec![QuadletUnit {
                path: "/etc/containers/systemd/myapp.container".into(),
                name: "myapp.container".into(),
                content: "[Container]\nImage=quay.io/test:latest".into(),
                include: true,
                locked: false,
                ..Default::default()
            }],
            flatpak_apps: vec![FlatpakApp {
                app_id: "org.example.App".into(),
                remote: "flathub".into(),
                branch: "stable".into(),
                include: true,
                locked: false,
                remote_url: "https://flathub.org/repo/".into(),
                ..Default::default()
            }],
            ..Default::default()
        });

        // Sysctl → sysctl/ root
        // Tuned → tuned/ root
        snap.kernel_boot = Some(KernelBootSection {
            sysctl_overrides: vec![SysctlOverride {
                key: "net.ipv4.ip_forward".into(),
                runtime: "1".into(),
                source: "/etc/sysctl.d/99-custom.conf".into(),
                include: true,
                locked: false,
                ..Default::default()
            }],
            tuned_include: true,
            tuned_active: "my-profile".into(),
            tuned_custom_profiles: vec![ConfigSnippet {
                path: "etc/tuned/my-profile/tuned.conf".into(),
                content: "[main]\nsummary=Custom".into(),
            }],
            ..Default::default()
        });

        let tmpdir = tempfile::tempdir().unwrap();
        let tarball_path = tmpdir.path().join("export.tar.gz");
        render_refine_export(&snap, &tarball_path, None, None).unwrap();

        // Read tarball entries
        let f = std::fs::File::open(&tarball_path).unwrap();
        let gz = flate2::read::GzDecoder::new(f);
        let mut ar = tar::Archive::new(gz);
        let entries: Vec<String> = ar
            .entries()
            .unwrap()
            .filter_map(|e| e.ok())
            .filter_map(|e| e.path().ok().map(|p| p.to_string_lossy().to_string()))
            .collect();

        // Tarball uses "export" as the top-level prefix (from "export.tar.gz").
        // Promoted roots must appear in the tarball under that prefix.
        assert!(
            entries.iter().any(|e| e.starts_with("export/drop-ins/")),
            "tarball must contain drop-ins/ root. entries: {entries:?}"
        );
        // quadlet/ is intentionally excluded from the refine export
        // (see export_excludes_extra_config_tree_artifacts contract test).
        assert!(
            !entries.iter().any(|e| e.contains("quadlet/")),
            "tarball must NOT contain quadlet/. entries: {entries:?}"
        );
        assert!(
            entries.iter().any(|e| e.starts_with("export/flatpak/")),
            "tarball must contain flatpak/ root. entries: {entries:?}"
        );
        assert!(
            entries.iter().any(|e| e.starts_with("export/sysctl/")),
            "tarball must contain sysctl/ root. entries: {entries:?}"
        );
        assert!(
            entries.iter().any(|e| e.starts_with("export/tuned/")),
            "tarball must contain tuned/ root. entries: {entries:?}"
        );

        // Verify specific files within promoted roots
        assert!(
            entries
                .iter()
                .any(|e| e.contains("httpd.service.d/limits.conf")),
            "drop-in content must be present. entries: {entries:?}"
        );
        assert!(
            entries
                .iter()
                .any(|e| e.contains("99-inspectah-migrated.conf")),
            "synthesized sysctl file must be present. entries: {entries:?}"
        );
        assert!(
            entries.iter().any(|e| e.contains("flatpak-install.json")),
            "flatpak manifest must be present. entries: {entries:?}"
        );
        assert!(
            entries
                .iter()
                .any(|e| e.contains("flatpak-provision.service")),
            "flatpak provisioning service must be present. entries: {entries:?}"
        );
        assert!(
            entries
                .iter()
                .any(|e| e.contains("tuned/etc/tuned/my-profile/tuned.conf")),
            "tuned profile must be present. entries: {entries:?}"
        );

        // Promoted artifacts must NOT appear under config/
        assert!(
            !entries
                .iter()
                .any(|e| e.starts_with("export/config/etc/systemd/system/httpd.service.d/")),
            "drop-ins must NOT be under config/. entries: {entries:?}"
        );
        assert!(
            !entries
                .iter()
                .any(|e| e.starts_with("export/config/etc/containers/systemd/")),
            "quadlets must NOT be under config/. entries: {entries:?}"
        );
        assert!(
            !entries
                .iter()
                .any(|e| e.starts_with("export/config/etc/tuned/")),
            "tuned profiles must NOT be under config/. entries: {entries:?}"
        );
    }

    // ── Projection cache tests ──────────────────────────────────────

    #[test]
    fn session_exposes_decisions_and_reference() {
        use inspectah_core::types::services::{
            PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState,
        };

        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection::default()),
            services: Some(ServiceSection {
                state_changes: vec![ServiceStateChange {
                    unit: "httpd.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    include: true,
                    locked: false,
                    owning_package: Some("httpd".into()),
                    aggregate: None,
                    attention_reason: None,
                }],
                enabled_units: vec!["httpd.service".into()],
                disabled_units: vec![],
                drop_ins: vec![],
                preset_matched_units: vec![],
            }),
            ..Default::default()
        };
        let session = RefineSession::new(snap);

        // decisions() should return without panic and contain the service
        let dec = session.decisions();
        assert_eq!(dec.service_states.len(), 1);
        assert_eq!(dec.service_states[0].entry.unit, "httpd.service");

        // reference() should return without panic
        let _ref_proj = session.reference();
    }

    #[test]
    fn decisions_invalidated_on_mutation() {
        use inspectah_core::types::rpm::VersionChange;
        use inspectah_core::types::rpm::VersionChangeDirection;
        use inspectah_core::types::services::{
            PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState,
        };

        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection {
                packages_added: vec![PackageEntry {
                    name: "nginx".into(),
                    arch: "x86_64".into(),
                    version: "1.24.0".into(),
                    release: "1.el9".into(),
                    include: true,
                    locked: false,
                    state: PackageState::Added,
                    source_repo: "appstream".into(),
                    ..Default::default()
                }],
                version_changes: vec![
                    VersionChange {
                        name: "openssl".into(),
                        arch: "x86_64".into(),
                        host_version: "3.0.7".into(),
                        base_version: "3.0.8".into(),
                        host_epoch: "1".into(),
                        base_epoch: "1".into(),
                        direction: VersionChangeDirection::Downgrade,
                    },
                    VersionChange {
                        name: "curl".into(),
                        arch: "x86_64".into(),
                        host_version: "8.1.0".into(),
                        base_version: "8.0.0".into(),
                        host_epoch: "0".into(),
                        base_epoch: "0".into(),
                        direction: VersionChangeDirection::Upgrade,
                    },
                ],
                ..Default::default()
            }),
            services: Some(ServiceSection {
                state_changes: vec![ServiceStateChange {
                    unit: "httpd.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    include: false,
                    locked: false,
                    owning_package: Some("httpd".into()),
                    aggregate: None,
                    attention_reason: None,
                }],
                enabled_units: vec!["httpd.service".into()],
                disabled_units: vec![],
                drop_ins: vec![],
                preset_matched_units: vec![],
            }),
            ..Default::default()
        };

        let mut session = RefineSession::new(snap);

        // Capture pre-mutation decisions
        let dec_before = session.decisions().clone();
        // httpd.service starts with include=false
        assert!(
            dec_before
                .service_states
                .iter()
                .any(|s| s.entry.unit == "httpd.service" && !s.entry.include),
            "httpd.service should start excluded"
        );

        // Capture reference pointer before mutation
        let ref_before = session.reference() as *const crate::projection::ReferenceProjection;

        // Mutate: flip httpd.service include false -> true
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Service {
                    unit: "httpd.service".into(),
                },
                include: true,
            })
            .unwrap();

        // Decisions should have changed
        let dec_after = session.decisions();
        assert!(
            dec_after
                .service_states
                .iter()
                .any(|s| s.entry.unit == "httpd.service" && s.entry.include),
            "httpd.service should now be included"
        );

        // Also mutate a package to change projected RPM state
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Package {
                    name: "nginx".into(),
                    arch: "x86_64".into(),
                },
                include: false,
            })
            .unwrap();

        // Reference should be pointer-identical (OnceLock, never recomputed)
        let ref_after = session.reference() as *const crate::projection::ReferenceProjection;
        assert!(
            std::ptr::eq(ref_before as *const (), ref_after as *const ()),
            "reference projection pointer must be stable across mutations"
        );

        // Verify reference field stability: version_changes should still
        // have the same counts regardless of mutations
        let ref_proj = session.reference();
        assert_eq!(ref_proj.version_changes.downgrades.len(), 1);
        assert_eq!(ref_proj.version_changes.upgrades.len(), 1);
        assert_eq!(ref_proj.version_changes.downgrades[0].name, "openssl");
        assert_eq!(ref_proj.version_changes.upgrades[0].name, "curl");
    }

    // ── Locked field enforcement tests ────────────────────────────────

    #[test]
    fn set_include_on_locked_item_is_rejected() {
        let mut snap = test_snapshot_with_services();
        // Lock httpd.service to simulate a semantic exclusion
        let services = snap.services.as_mut().unwrap();
        services.state_changes[0].include = false;
        services.state_changes[0].locked = true;

        let mut session = RefineSession::new(snap);

        // Attempting to re-include a locked service must be a silent no-op
        let result = session.apply(RefinementOp::SetInclude {
            item_id: ItemId::Service {
                unit: "httpd.service".into(),
            },
            include: true,
        });
        assert!(result.is_ok(), "locked set-include must not error");

        // The service must remain excluded
        let projected = session.snapshot_projected();
        let svc = projected.services.as_ref().unwrap();
        let httpd = svc
            .state_changes
            .iter()
            .find(|s| s.unit == "httpd.service")
            .unwrap();
        assert!(
            !httpd.include,
            "locked service must stay excluded after set-include attempt"
        );

        // The op must NOT be recorded in history
        assert!(
            session.ops_history().is_empty(),
            "locked set-include must not record an op"
        );
    }

    #[test]
    fn recompute_view_skips_locked_set_include_ops() {
        let mut snap = test_snapshot_with_services();
        let services = snap.services.as_mut().unwrap();
        // Start unlocked and excluded
        services.state_changes[0].include = false;
        services.state_changes[0].locked = false;

        let mut session = RefineSession::new(snap);

        // Apply a valid include op while unlocked
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Service {
                    unit: "httpd.service".into(),
                },
                include: true,
            })
            .unwrap();

        // Verify the op was recorded and the service is included
        assert_eq!(session.ops_history().len(), 1);
        let projected = session.snapshot_projected();
        assert!(
            projected
                .services
                .as_ref()
                .unwrap()
                .state_changes
                .iter()
                .find(|s| s.unit == "httpd.service")
                .unwrap()
                .include,
            "service must be included after unlocked set-include"
        );

        // Now simulate what happens when the item becomes locked in a
        // newer snapshot but stale ops exist from the autosaved session.
        // We do this by directly locking the original snapshot's item and
        // forcing a recompute. This models resume_from() with a snapshot
        // where the normalize layer now locks this item.
        session.original.services.as_mut().unwrap().state_changes[0].locked = true;
        session.original.services.as_mut().unwrap().state_changes[0].include = false;
        session.cached_view = None;
        session.cached_decisions = None;
        session.recompute_view();

        // The stale SetInclude(true) op must be skipped during replay
        let projected = session.snapshot_projected();
        let httpd = projected
            .services
            .as_ref()
            .unwrap()
            .state_changes
            .iter()
            .find(|s| s.unit == "httpd.service")
            .unwrap();
        assert!(
            !httpd.include,
            "locked service must stay excluded even with stale include op in history"
        );
    }

    #[test]
    fn export_clamp_forces_locked_items_excluded() {
        // Verify clamp_locked_items forces include=false on locked items
        // even if the working snapshot somehow has include=true.
        let mut snap = test_snapshot_with_services();
        let services = snap.services.as_mut().unwrap();
        // Simulate a locked item that somehow has include=true
        services.state_changes[0].include = true;
        services.state_changes[0].locked = true;

        clamp_locked_items(&mut snap);

        let httpd = snap
            .services
            .as_ref()
            .unwrap()
            .state_changes
            .iter()
            .find(|s| s.unit == "httpd.service")
            .unwrap();
        assert!(
            !httpd.include,
            "clamp must force locked item to include=false"
        );

        // Verify unlocked items are not affected
        let sshd = snap
            .services
            .as_ref()
            .unwrap()
            .state_changes
            .iter()
            .find(|s| s.unit == "sshd.service")
            .unwrap();
        assert!(sshd.include, "clamp must not affect unlocked items");
    }

    // ── Aggregate pre-filtered packages regression tests ──────────────────

    #[test]
    fn aggregate_pre_filtered_packages_drive_refine_view() {
        // Aggregate snapshot where merge already filtered packages_added to
        // leaf-only. The refine view must show only those pre-filtered
        // packages, not re-expand to include transitive deps.
        let mut snap = test_snapshot();
        snap.aggregate_meta = Some(inspectah_core::types::aggregate::AggregateSnapshotMeta {
            label: "web-tier".into(),
            host_count: 3,
            hostnames: vec!["web-01".into(), "web-02".into(), "web-03".into()],
            merged_at: "2026-06-09T00:00:00Z".into(),
            baseline_provisional: false,
            section_host_counts: Default::default(),
        });
        // Baseline so packages land in Site bucket (leaf filtering applies)
        snap.baseline = Some(inspectah_core::baseline::BaselineData {
            image_digest: "sha256:test".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });
        let rpm = snap.rpm.as_mut().unwrap();
        rpm.packages_added = vec![PackageEntry {
            name: "git".into(),
            arch: "x86_64".into(),
            include: true,
            locked: false,
            source_repo: "appstream".into(),
            aggregate: Some(Default::default()),
            ..Default::default()
        }];
        rpm.leaf_packages = Some(vec!["git.x86_64".into()]);
        rpm.auto_packages = Some(Vec::new());

        let session = RefineSession::new(snap);
        let view = session.view();

        // Only the pre-filtered leaf package should appear
        assert_eq!(
            view.packages.len(),
            1,
            "view must show only the pre-filtered leaf package"
        );
        assert_eq!(
            view.packages[0].entry.name, "git",
            "the single visible package must be 'git'"
        );
    }

    #[test]
    fn aggregate_leaf_authority_metadata_on_snapshot() {
        // Partial authority metadata set during merge must be accessible
        // on the snapshot through the refine session.
        let mut snap = test_snapshot();
        snap.aggregate_meta = Some(inspectah_core::types::aggregate::AggregateSnapshotMeta {
            label: "web-tier".into(),
            host_count: 3,
            hostnames: vec!["web-01".into(), "web-02".into(), "web-03".into()],
            merged_at: "2026-06-09T00:00:00Z".into(),
            baseline_provisional: false,
            section_host_counts: Default::default(),
        });
        let rpm = snap.rpm.as_mut().unwrap();
        rpm.leaf_authority_hosts = Some(2);
        rpm.leaf_total_hosts = Some(3);

        let session = RefineSession::new(snap);
        let snap_ref = session.snapshot();
        let rpm_ref = snap_ref.rpm.as_ref().unwrap();

        assert_eq!(
            rpm_ref.leaf_authority_hosts,
            Some(2),
            "leaf_authority_hosts must be preserved through RefineSession"
        );
        assert_eq!(
            rpm_ref.leaf_total_hosts,
            Some(3),
            "leaf_total_hosts must be preserved through RefineSession"
        );
    }

    // ── Locked-item contract tests ────────────────────────────────────

    #[test]
    fn locked_platform_plumbing_package_rejects_set_include() {
        // End-to-end: the package enters as a normal anaconda-sourced
        // PackageEntry (include=true, locked=false). The anaconda
        // classifier in RefineSession::new() must classify it as Tier 1
        // platform plumbing and set include=false, locked=true. Then
        // SetInclude(true) must be a silent no-op.
        use inspectah_core::types::config::ConfigSection;
        use inspectah_core::types::services::ServiceSection;
        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection {
                packages_added: vec![PackageEntry {
                    name: "grub2-efi-aa64-cdboot".into(),
                    arch: "aarch64".into(),
                    state: PackageState::Added,
                    source_repo: "anaconda".into(),
                    include: true,
                    locked: false,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            services: Some(ServiceSection {
                state_changes: vec![],
                enabled_units: vec![],
                disabled_units: vec![],
                drop_ins: vec![],
                preset_matched_units: vec![],
            }),
            config: Some(ConfigSection { files: vec![] }),
            ..Default::default()
        };

        let mut session = RefineSession::new(snap);

        // Attempt to include a locked package — must succeed (Ok) but be a no-op
        let result = session.apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "grub2-efi-aa64-cdboot".into(),
                arch: "aarch64".into(),
            },
            include: true,
        });
        assert!(result.is_ok(), "apply on locked item must return Ok");

        // View: the package must still be excluded
        let view = session.view();
        let view_pkg = view
            .packages
            .iter()
            .find(|p| p.entry.name == "grub2-efi-aa64-cdboot");
        if let Some(pkg) = view_pkg {
            assert!(
                !pkg.entry.include,
                "view: locked package must stay excluded"
            );
            assert!(pkg.entry.locked, "view: locked flag must be preserved");
        }

        // Projected snapshot: the package must still be excluded
        let projected = session.snapshot_projected();
        let proj_pkg = projected
            .rpm
            .as_ref()
            .unwrap()
            .packages_added
            .iter()
            .find(|p| p.name == "grub2-efi-aa64-cdboot")
            .unwrap();
        assert!(
            !proj_pkg.include,
            "projected: locked package must stay excluded"
        );
        assert!(proj_pkg.locked, "projected: locked flag must be preserved");
    }

    // ── UngroupGroup / apply_directive tests ──────────────────────────

    use crate::types::ViewDirective;
    use inspectah_core::types::rpm::InstalledGroup;

    /// Build a snapshot with installed groups for apply_directive tests.
    fn test_snapshot_with_groups() -> InspectionSnapshot {
        let mut snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection {
                packages_added: vec![
                    PackageEntry {
                        name: "podman".into(),
                        arch: "x86_64".into(),
                        include: true,
                        locked: false,
                        source_repo: "appstream".into(),
                        ..Default::default()
                    },
                    PackageEntry {
                        name: "buildah".into(),
                        arch: "x86_64".into(),
                        include: true,
                        locked: false,
                        source_repo: "appstream".into(),
                        ..Default::default()
                    },
                    PackageEntry {
                        name: "skopeo".into(),
                        arch: "x86_64".into(),
                        include: true,
                        locked: false,
                        source_repo: "appstream".into(),
                        ..Default::default()
                    },
                ],
                installed_groups: Some(vec![InstalledGroup {
                    name: "Container Management".into(),
                    members: vec!["podman".into(), "buildah".into(), "skopeo".into()],
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };
        // Baseline needed so packages get Site bucket and include=true survives normalization.
        snap.baseline = Some(inspectah_core::baseline::BaselineData {
            image_digest: "sha256:test".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });
        snap
    }

    #[test]
    fn ungroup_adds_view_directive_to_timeline() {
        let snap = test_snapshot_with_groups();
        let mut session = RefineSession::new(snap);
        session
            .apply_directive(ViewDirective::UngroupGroup {
                group_name: "Container Management".into(),
            })
            .unwrap();
        assert_eq!(session.timeline_len(), 1);
        assert!(session.can_undo());
    }

    #[test]
    fn ungroup_idempotent_on_already_ungrouped() {
        let snap = test_snapshot_with_groups();
        let mut session = RefineSession::new(snap);
        session
            .apply_directive(ViewDirective::UngroupGroup {
                group_name: "Container Management".into(),
            })
            .unwrap();
        let len_after_first = session.timeline_len();
        session
            .apply_directive(ViewDirective::UngroupGroup {
                group_name: "Container Management".into(),
            })
            .unwrap();
        assert_eq!(session.timeline_len(), len_after_first, "idempotent");
    }

    #[test]
    fn ungroup_unknown_group_returns_error() {
        let snap = test_snapshot_with_groups();
        let mut session = RefineSession::new(snap);
        let result = session.apply_directive(ViewDirective::UngroupGroup {
            group_name: "Nonexistent Group".into(),
        });
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("Nonexistent Group"),
            "error must name the unknown group, got: {err}"
        );
    }

    #[test]
    fn undo_ungroup_regroups() {
        let snap = test_snapshot_with_groups();
        let mut session = RefineSession::new(snap);
        session
            .apply_directive(ViewDirective::UngroupGroup {
                group_name: "Container Management".into(),
            })
            .unwrap();
        session.undo().unwrap();
        assert_eq!(session.timeline_len(), 1);
        assert_eq!(session.cursor(), 0);
    }

    #[test]
    fn ungroup_sets_dirty_state() {
        let snap = test_snapshot_with_groups();
        let mut session = RefineSession::new(snap);
        assert!(!session.pending_changes().is_dirty);
        session
            .apply_directive(ViewDirective::UngroupGroup {
                group_name: "Container Management".into(),
            })
            .unwrap();
        assert!(session.pending_changes().is_dirty);
    }

    // ── Group-level SetInclude fan-out tests ──────────────────────────

    /// Build a snapshot with installed groups that includes a locked member.
    fn test_snapshot_with_groups_and_locked() -> InspectionSnapshot {
        let mut snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection {
                packages_added: vec![
                    PackageEntry {
                        name: "podman".into(),
                        arch: "x86_64".into(),
                        include: true,
                        locked: false,
                        source_repo: "appstream".into(),
                        ..Default::default()
                    },
                    PackageEntry {
                        name: "buildah".into(),
                        arch: "x86_64".into(),
                        include: true,
                        locked: true,
                        source_repo: "appstream".into(),
                        ..Default::default()
                    },
                    PackageEntry {
                        name: "skopeo".into(),
                        arch: "x86_64".into(),
                        include: true,
                        locked: false,
                        source_repo: "appstream".into(),
                        ..Default::default()
                    },
                ],
                installed_groups: Some(vec![InstalledGroup {
                    name: "Container Management".into(),
                    members: vec!["podman".into(), "buildah".into(), "skopeo".into()],
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };
        snap.baseline = Some(inspectah_core::baseline::BaselineData {
            image_digest: "sha256:test".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });
        snap
    }

    /// Build a snapshot with a group containing multi-arch members.
    fn test_snapshot_with_groups_multiarch() -> InspectionSnapshot {
        let mut snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection {
                packages_added: vec![
                    PackageEntry {
                        name: "podman".into(),
                        arch: "x86_64".into(),
                        include: true,
                        locked: false,
                        source_repo: "appstream".into(),
                        ..Default::default()
                    },
                    PackageEntry {
                        name: "podman".into(),
                        arch: "i686".into(),
                        include: true,
                        locked: false,
                        source_repo: "appstream".into(),
                        ..Default::default()
                    },
                    PackageEntry {
                        name: "buildah".into(),
                        arch: "x86_64".into(),
                        include: true,
                        locked: false,
                        source_repo: "appstream".into(),
                        ..Default::default()
                    },
                ],
                installed_groups: Some(vec![InstalledGroup {
                    name: "Container Management".into(),
                    members: vec!["podman".into(), "buildah".into()],
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };
        snap.baseline = Some(inspectah_core::baseline::BaselineData {
            image_digest: "sha256:test".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });
        snap
    }

    #[test]
    fn set_include_group_false_excludes_all_members() {
        let snap = test_snapshot_with_groups();
        let mut session = RefineSession::new(snap);
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Group {
                    name: "Container Management".into(),
                },
                include: false,
            })
            .unwrap();
        let view = session.view();
        for pkg in &view.packages {
            if ["podman", "buildah", "skopeo"].contains(&pkg.entry.name.as_str()) {
                assert!(!pkg.entry.include, "{} should be excluded", pkg.entry.name);
            }
        }
    }

    #[test]
    fn set_include_group_true_includes_non_locked_members() {
        let snap = test_snapshot_with_groups_and_locked();
        let mut session = RefineSession::new(snap);
        // First exclude the group
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Group {
                    name: "Container Management".into(),
                },
                include: false,
            })
            .unwrap();
        // Then re-include
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Group {
                    name: "Container Management".into(),
                },
                include: true,
            })
            .unwrap();
        let view = session.view();
        for pkg in &view.packages {
            match pkg.entry.name.as_str() {
                // buildah is locked — should stay excluded
                "buildah" => assert!(!pkg.entry.include, "locked buildah should stay excluded"),
                // podman and skopeo are not locked — should be re-included
                "podman" | "skopeo" => assert!(
                    pkg.entry.include,
                    "{} should be re-included",
                    pkg.entry.name
                ),
                _ => {}
            }
        }
    }

    #[test]
    fn undo_group_exclude_restores_all_members() {
        let snap = test_snapshot_with_groups();
        let mut session = RefineSession::new(snap);
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Group {
                    name: "Container Management".into(),
                },
                include: false,
            })
            .unwrap();
        session.undo().unwrap();
        let view = session.view();
        for pkg in &view.packages {
            if ["podman", "buildah", "skopeo"].contains(&pkg.entry.name.as_str()) {
                assert!(pkg.entry.include, "{} should be restored", pkg.entry.name);
            }
        }
    }

    #[test]
    fn set_include_group_multi_arch_toggles_all_arches() {
        let snap = test_snapshot_with_groups_multiarch();
        let mut session = RefineSession::new(snap);
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Group {
                    name: "Container Management".into(),
                },
                include: false,
            })
            .unwrap();
        let view = session.view();
        for pkg in &view.packages {
            if ["podman", "buildah"].contains(&pkg.entry.name.as_str()) {
                assert!(
                    !pkg.entry.include,
                    "{}.{} should be excluded",
                    pkg.entry.name, pkg.entry.arch
                );
            }
        }
    }

    #[test]
    fn set_include_group_unknown_is_error() {
        let snap = test_snapshot_with_groups();
        let mut session = RefineSession::new(snap);
        let result = session.apply(RefinementOp::SetInclude {
            item_id: ItemId::Group {
                name: "Nonexistent Group".into(),
            },
            include: false,
        });
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("Nonexistent Group"),
            "error must name the unknown group, got: {err}"
        );
    }

    #[test]
    fn set_include_group_noop_when_all_members_already_match() {
        let snap = test_snapshot_with_groups();
        let mut session = RefineSession::new(snap);
        // All members start included — setting include=true should be noop
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Group {
                    name: "Container Management".into(),
                },
                include: true,
            })
            .unwrap();
        assert_eq!(session.timeline_len(), 0, "noop should not add to timeline");
    }

    #[test]
    fn individual_op_after_group_op_takes_precedence() {
        let snap = test_snapshot_with_groups();
        let mut session = RefineSession::new(snap);
        // Exclude group (which contains podman)
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Group {
                    name: "Container Management".into(),
                },
                include: false,
            })
            .unwrap();
        // Re-include podman individually
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Package {
                    name: "podman".into(),
                    arch: "x86_64".into(),
                },
                include: true,
            })
            .unwrap();
        let view = session.view();
        let podman = view
            .packages
            .iter()
            .find(|p| p.entry.name == "podman")
            .unwrap();
        assert!(podman.entry.include, "individual op wins");
    }

    // ── Optional-installed independence tests ─────────────────────────

    /// Build a snapshot with a group that has optional_installed members.
    fn test_snapshot_with_optional_members() -> InspectionSnapshot {
        let mut snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection {
                packages_added: vec![
                    PackageEntry {
                        name: "podman".into(),
                        arch: "x86_64".into(),
                        include: true,
                        locked: false,
                        source_repo: "appstream".into(),
                        ..Default::default()
                    },
                    PackageEntry {
                        name: "buildah".into(),
                        arch: "x86_64".into(),
                        include: true,
                        locked: false,
                        source_repo: "appstream".into(),
                        ..Default::default()
                    },
                    PackageEntry {
                        name: "skopeo".into(),
                        arch: "x86_64".into(),
                        include: true,
                        locked: false,
                        source_repo: "appstream".into(),
                        ..Default::default()
                    },
                    PackageEntry {
                        name: "python3-podman".into(),
                        arch: "noarch".into(),
                        include: true,
                        locked: false,
                        source_repo: "appstream".into(),
                        ..Default::default()
                    },
                ],
                installed_groups: Some(vec![InstalledGroup {
                    name: "Container Management".into(),
                    members: vec!["podman".into(), "buildah".into(), "skopeo".into()],
                    optional_installed: vec!["python3-podman".into()],
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };
        snap.baseline = Some(inspectah_core::baseline::BaselineData {
            image_digest: "sha256:test".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });
        snap
    }

    #[test]
    fn optional_installed_not_affected_by_group_exclude() {
        let snap = test_snapshot_with_optional_members();
        let mut session = RefineSession::new(snap);
        // Record the optional package's include state
        let opt_before = session
            .view()
            .packages
            .iter()
            .find(|p| p.entry.name == "python3-podman")
            .unwrap()
            .entry
            .include;
        // Exclude the group
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Group {
                    name: "Container Management".into(),
                },
                include: false,
            })
            .unwrap();
        let opt_after = session
            .view()
            .packages
            .iter()
            .find(|p| p.entry.name == "python3-podman")
            .unwrap()
            .entry
            .include;
        assert_eq!(opt_before, opt_after, "optional member unchanged");
        // Verify regular members were excluded
        for pkg in &session.view().packages {
            if ["podman", "buildah", "skopeo"].contains(&pkg.entry.name.as_str()) {
                assert!(!pkg.entry.include, "{} should be excluded", pkg.entry.name);
            }
        }
    }

    // ── RenderContext integration tests ──────────────────────────────

    #[test]
    fn render_context_built_during_view_computation() {
        let snap = test_snapshot_with_groups();
        let session = RefineSession::new(snap);
        let ctx = session.render_context();
        assert!(
            ctx.is_renderable("Container Management"),
            "fresh session: all members included => Renderable"
        );
    }

    #[test]
    fn render_context_reflects_ungroup() {
        let snap = test_snapshot_with_groups();
        let mut session = RefineSession::new(snap);
        session
            .apply_directive(ViewDirective::UngroupGroup {
                group_name: "Container Management".into(),
            })
            .unwrap();
        let ctx = session.render_context();
        assert!(
            ctx.is_ungrouped("Container Management"),
            "after UngroupGroup => Ungrouped"
        );
    }

    #[test]
    fn render_context_reflects_degradation() {
        let snap = test_snapshot_with_groups();
        let mut session = RefineSession::new(snap);
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Package {
                    name: "podman".into(),
                    arch: "x86_64".into(),
                },
                include: false,
            })
            .unwrap();
        let ctx = session.render_context();
        assert!(
            ctx.is_degraded("Container Management"),
            "excluding one member without group op => Degraded"
        );
    }

    #[test]
    fn render_context_auto_upgrades_after_undo() {
        let snap = test_snapshot_with_groups();
        let mut session = RefineSession::new(snap);
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Package {
                    name: "podman".into(),
                    arch: "x86_64".into(),
                },
                include: false,
            })
            .unwrap();
        assert!(
            session.render_context().is_degraded("Container Management"),
            "after member exclude => Degraded"
        );
        session.undo().unwrap();
        assert!(
            session
                .render_context()
                .is_renderable("Container Management"),
            "after undo => back to Renderable"
        );
    }

    #[test]
    fn render_context_group_exclude_then_member_override_is_degraded() {
        let snap = test_snapshot_with_groups();
        let mut session = RefineSession::new(snap);
        // Exclude the whole group
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Group {
                    name: "Container Management".into(),
                },
                include: false,
            })
            .unwrap();
        assert!(
            session.render_context().is_excluded("Container Management"),
            "group exclude => Excluded"
        );
        // Re-include one member individually — diverges from group op
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Package {
                    name: "podman".into(),
                    arch: "x86_64".into(),
                },
                include: true,
            })
            .unwrap();
        assert!(
            session.render_context().is_degraded("Container Management"),
            "member override after group exclude => Degraded (MemberOverridden)"
        );
    }

    #[test]
    fn render_context_no_groups_produces_empty() {
        let snap = test_snapshot();
        let session = RefineSession::new(snap);
        let ctx = session.render_context();
        assert!(
            ctx.group_states.is_empty(),
            "snapshot without groups => empty RenderContext"
        );
    }

    #[test]
    fn export_snapshot_does_not_contain_render_context() {
        use crate::types::ViewDirective;

        let snap = test_snapshot_with_groups();
        let mut session = RefineSession::new(snap);

        // Apply a view directive that creates RenderContext state
        session
            .apply_directive(ViewDirective::UngroupGroup {
                group_name: "Container Management".into(),
            })
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let export_path = dir.path().join("export.tar.gz");
        session
            .export_tarball(&export_path, session.view().generation)
            .unwrap();

        // Read the exported snapshot JSON from the tarball
        let file = std::fs::File::open(&export_path).unwrap();
        let gz = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(gz);

        let mut snapshot_json = None;
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let path = entry.path().unwrap();
            if path.to_string_lossy().ends_with("inspection-snapshot.json") {
                let mut content = String::new();
                std::io::Read::read_to_string(&mut entry, &mut content).unwrap();
                snapshot_json = Some(content);
                break;
            }
        }

        let snapshot_json = snapshot_json.expect("inspection-snapshot.json must exist in tarball");

        // Verify RenderContext fields are NOT present in the exported JSON
        assert!(
            !snapshot_json.contains("ungrouped"),
            "exported snapshot must not contain 'ungrouped' field from RenderContext"
        );
        assert!(
            !snapshot_json.contains("group_states"),
            "exported snapshot must not contain 'group_states' field from RenderContext"
        );
        assert!(
            !snapshot_json.contains("render_context"),
            "exported snapshot must not contain 'render_context' field"
        );
    }

    // ── Truth-boundary proof tests ──────────────────────────────────
    //
    // These verify that group state derivation uses the *filtered* render
    // surface (baseline-suppressed + leaf-filtered), not the raw projected
    // snapshot. Before the fix, effective_packages came from
    // projected.rpm.packages_added, which could include packages the view
    // never shows.

    #[test]
    fn baseline_suppressed_member_excluded_from_group_eval() {
        // Setup: group "Web Server" has members httpd and mod_ssl.
        // httpd is baseline-suppressed (present in target image).
        // mod_ssl has include=false (user excluded it).
        //
        // OLD behavior (bug): effective_packages includes httpd (from raw
        //   projection). httpd has include=true, mod_ssl has include=false.
        //   derive_group_state sees one included + one excluded non-locked
        //   member => Degraded(MemberExcluded). This is WRONG because httpd
        //   isn't on the render surface at all.
        //
        // NEW behavior (fix): effective_packages comes from filtered packages.
        //   httpd is baseline-suppressed and absent. Only mod_ssl remains,
        //   with include=false => Degraded(MemberExcluded). The group state
        //   now matches what the view actually shows.
        //
        // The key assertion: the group state reflects the filtered surface.
        // We verify by checking that only 1 package appears in the view
        // (mod_ssl) and the group IS Degraded — but for the right reason
        // (the visible member is excluded, not a phantom baseline member).
        let mut snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection {
                packages_added: vec![
                    PackageEntry {
                        name: "httpd".into(),
                        arch: "x86_64".into(),
                        include: true,
                        locked: false,
                        source_repo: "appstream".into(),
                        ..Default::default()
                    },
                    PackageEntry {
                        name: "mod_ssl".into(),
                        arch: "x86_64".into(),
                        include: true,
                        locked: false,
                        source_repo: "appstream".into(),
                        ..Default::default()
                    },
                ],
                baseline_suppressed: Some(vec!["httpd.x86_64".into()]),
                installed_groups: Some(vec![InstalledGroup {
                    name: "Web Server".into(),
                    members: vec!["httpd".into(), "mod_ssl".into()],
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };
        snap.baseline = Some(inspectah_core::baseline::BaselineData {
            image_digest: "sha256:test".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });

        let mut session = RefineSession::new(snap);

        // Exclude mod_ssl so we have a visible excluded member.
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Package {
                    name: "mod_ssl".into(),
                    arch: "x86_64".into(),
                },
                include: false,
            })
            .unwrap();

        let view = session.view();

        // httpd should be baseline-suppressed and absent from view.
        assert!(
            !view.packages.iter().any(|p| p.entry.name == "httpd"),
            "httpd must be baseline-suppressed from view"
        );

        // mod_ssl should be the only visible package.
        assert_eq!(
            view.packages
                .iter()
                .filter(|p| p.entry.name == "mod_ssl")
                .count(),
            1,
            "mod_ssl must be visible in view"
        );

        // Group state must reflect the filtered surface: mod_ssl excluded
        // means Degraded(MemberExcluded). Before the fix, httpd would have
        // polluted effective_packages from the raw projection.
        let ctx = session.render_context();
        assert!(
            ctx.is_degraded("Web Server"),
            "group with baseline-suppressed member must derive state from filtered surface"
        );
    }

    #[test]
    fn leaf_filtered_member_excluded_from_group_eval() {
        // Setup: group "Core Libs" has members vim and glibc.
        // glibc is a transitive dep (not a leaf), so leaf filtering removes it.
        // vim is a leaf and remains visible with include=true.
        //
        // OLD behavior (bug): effective_packages includes glibc (from raw
        //   projection). Both have include=true => Renderable. But the view
        //   only shows vim. If the user exports, `dnf group install` would
        //   try to install glibc explicitly, which is wasteful — it's a dep.
        //
        // NEW behavior (fix): effective_packages comes from filtered packages.
        //   glibc is absent (leaf-filtered). Only vim remains, include=true.
        //   derive_group_state sees 1 member, included => Renderable. The
        //   result is the same in this case, but the derivation is honest:
        //   it's Renderable because the *visible* members are all included.
        let mut snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection {
                packages_added: vec![
                    PackageEntry {
                        name: "vim".into(),
                        arch: "x86_64".into(),
                        include: true,
                        locked: false,
                        source_repo: "appstream".into(),
                        ..Default::default()
                    },
                    PackageEntry {
                        name: "glibc".into(),
                        arch: "x86_64".into(),
                        include: true,
                        locked: false,
                        source_repo: "baseos".into(),
                        ..Default::default()
                    },
                ],
                leaf_packages: Some(vec!["vim.x86_64".into()]),
                auto_packages: Some(vec!["glibc.x86_64".into()]),
                installed_groups: Some(vec![InstalledGroup {
                    name: "Core Libs".into(),
                    members: vec!["vim".into(), "glibc".into()],
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };
        snap.baseline = Some(inspectah_core::baseline::BaselineData {
            image_digest: "sha256:test".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });

        let session = RefineSession::new(snap);
        let view = session.view();

        // glibc should be leaf-filtered and absent from view.
        assert!(
            !view.packages.iter().any(|p| p.entry.name == "glibc"),
            "glibc must be leaf-filtered from view"
        );

        // vim should be the only visible package.
        assert_eq!(
            view.packages
                .iter()
                .filter(|p| p.entry.name == "vim")
                .count(),
            1,
            "vim must be visible in view"
        );

        // Group state should reflect only the visible member (vim, included).
        // With the fix, glibc is absent from effective_packages, so the
        // group state is based solely on vim. Before the fix, glibc would
        // have been in effective_packages from the raw projection.
        let ctx = session.render_context();
        assert!(
            ctx.is_renderable("Core Libs"),
            "group with only visible members included should be Renderable"
        );

        // Now exclude vim to prove the filtered surface drives group state.
        let mut session2 = RefineSession::new({
            let mut s = InspectionSnapshot {
                schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
                rpm: Some(RpmSection {
                    packages_added: vec![
                        PackageEntry {
                            name: "vim".into(),
                            arch: "x86_64".into(),
                            include: true,
                            locked: false,
                            source_repo: "appstream".into(),
                            ..Default::default()
                        },
                        PackageEntry {
                            name: "glibc".into(),
                            arch: "x86_64".into(),
                            include: true,
                            locked: false,
                            source_repo: "baseos".into(),
                            ..Default::default()
                        },
                    ],
                    leaf_packages: Some(vec!["vim.x86_64".into()]),
                    auto_packages: Some(vec!["glibc.x86_64".into()]),
                    installed_groups: Some(vec![InstalledGroup {
                        name: "Core Libs".into(),
                        members: vec!["vim".into(), "glibc".into()],
                        ..Default::default()
                    }]),
                    ..Default::default()
                }),
                ..Default::default()
            };
            s.baseline = Some(inspectah_core::baseline::BaselineData {
                image_digest: "sha256:test".into(),
                packages: std::collections::HashMap::new(),
                extracted_at: "2026-01-01T00:00:00Z".into(),
            });
            s
        });

        session2
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Package {
                    name: "vim".into(),
                    arch: "x86_64".into(),
                },
                include: false,
            })
            .unwrap();

        let ctx2 = session2.render_context();
        assert!(
            ctx2.is_degraded("Core Libs"),
            "excluding the only visible member must degrade the group"
        );
    }

    #[test]
    fn view_hides_platform_plumbing_packages() {
        use inspectah_core::types::config::ConfigSection;
        use inspectah_core::types::rpm::FileOwnershipEntry;
        use inspectah_core::types::services::{
            PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState,
        };

        let mut snap = test_snapshot();
        let rpm = snap.rpm.as_mut().unwrap();
        rpm.packages_added = vec![
            // Platform plumbing — should be hidden
            PackageEntry {
                name: "grub2-tools".into(),
                arch: "x86_64".into(),
                include: true,
                source_repo: "anaconda".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "shim-x64".into(),
                arch: "x86_64".into(),
                include: true,
                source_repo: "anaconda".into(),
                ..Default::default()
            },
            // Normal anaconda package — should be visible
            PackageEntry {
                name: "harfbuzz".into(),
                arch: "x86_64".into(),
                include: true,
                source_repo: "anaconda".into(),
                ..Default::default()
            },
            // Non-anaconda package — should be visible
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                include: true,
                source_repo: "appstream".into(),
                ..Default::default()
            },
        ];
        rpm.file_ownership = vec![FileOwnershipEntry {
            package_name: "dummy".into(),
            paths: vec!["/etc/dummy".into()],
        }];

        // Provide services + config evidence so anaconda classifier runs
        snap.services = Some(ServiceSection {
            state_changes: vec![ServiceStateChange {
                unit: "sshd.service".into(),
                current_state: ServiceUnitState::Enabled,
                default_state: Some(PresetDefault::Enable),
                include: true,
                locked: false,
                owning_package: Some("openssh-server".into()),
                aggregate: None,
                attention_reason: None,
            }],
            ..Default::default()
        });
        snap.config = Some(ConfigSection { files: vec![] });

        let session = RefineSession::new(snap);
        let view = session.view();

        let view_names: Vec<&str> = view
            .packages
            .iter()
            .map(|p| p.entry.name.as_str())
            .collect();

        // Platform plumbing packages must not appear in the view
        assert!(
            !view_names.contains(&"grub2-tools"),
            "grub2-tools (platform plumbing) should be hidden from view"
        );
        assert!(
            !view_names.contains(&"shim-x64"),
            "shim-x64 (platform plumbing) should be hidden from view"
        );

        // Non-plumbing packages must remain visible
        assert!(
            view_names.contains(&"harfbuzz"),
            "harfbuzz should be visible in view"
        );
        assert!(
            view_names.contains(&"httpd"),
            "httpd should be visible in view"
        );
    }

    #[test]
    fn view_plumbing_filter_updates_package_count() {
        use inspectah_core::types::config::ConfigSection;
        use inspectah_core::types::rpm::FileOwnershipEntry;
        use inspectah_core::types::services::{
            PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState,
        };

        let mut snap = test_snapshot();
        let rpm = snap.rpm.as_mut().unwrap();
        rpm.packages_added = vec![
            PackageEntry {
                name: "grub2-tools".into(),
                arch: "x86_64".into(),
                include: true,
                source_repo: "anaconda".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "cronie".into(),
                arch: "x86_64".into(),
                include: true,
                source_repo: "anaconda".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                include: true,
                source_repo: "appstream".into(),
                ..Default::default()
            },
        ];
        rpm.file_ownership = vec![FileOwnershipEntry {
            package_name: "dummy".into(),
            paths: vec!["/etc/dummy".into()],
        }];

        snap.services = Some(ServiceSection {
            state_changes: vec![ServiceStateChange {
                unit: "sshd.service".into(),
                current_state: ServiceUnitState::Enabled,
                default_state: Some(PresetDefault::Enable),
                include: true,
                locked: false,
                owning_package: Some("openssh-server".into()),
                aggregate: None,
                attention_reason: None,
            }],
            ..Default::default()
        });
        snap.config = Some(ConfigSection { files: vec![] });

        let session = RefineSession::new(snap);
        let view = session.view();

        // Only 2 packages should be visible (grub2-tools hidden)
        assert_eq!(
            view.packages.len(),
            2,
            "view should contain 2 packages (grub2-tools filtered)"
        );
        assert_eq!(
            view.stats.total_packages(),
            2,
            "stats.total_packages should reflect filtered count"
        );
    }

    #[test]
    fn user_can_exclude_anaconda_tier4_package() {
        use inspectah_core::types::config::ConfigSection;
        use inspectah_core::types::rpm::FileOwnershipEntry;
        use inspectah_core::types::services::{
            PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState,
        };

        let mut snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection {
                packages_added: vec![PackageEntry {
                    name: "harfbuzz".into(),
                    arch: "aarch64".into(),
                    include: true,
                    source_repo: "anaconda".into(),
                    ..Default::default()
                }],
                file_ownership: vec![FileOwnershipEntry {
                    package_name: "dummy".into(),
                    paths: vec!["/etc/dummy".into()],
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        snap.services = Some(ServiceSection {
            state_changes: vec![ServiceStateChange {
                unit: "sshd.service".into(),
                current_state: ServiceUnitState::Enabled,
                default_state: Some(PresetDefault::Enable),
                include: true,
                locked: false,
                owning_package: Some("openssh-server".into()),
                aggregate: None,
                attention_reason: None,
            }],
            ..Default::default()
        });
        snap.config = Some(ConfigSection { files: vec![] });

        let mut session = RefineSession::new(snap);

        // harfbuzz starts as Tier 4 ambiguous: include=true, locked=false
        let view = session.view();
        let pkg = view
            .packages
            .iter()
            .find(|p| p.entry.name == "harfbuzz")
            .unwrap();
        assert!(pkg.entry.include, "harfbuzz should start included");
        assert!(!pkg.entry.locked, "harfbuzz should not be locked");

        // User toggles harfbuzz to exclude
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Package {
                    name: "harfbuzz".into(),
                    arch: "aarch64".into(),
                },
                include: false,
            })
            .unwrap();

        // After toggle, harfbuzz must be excluded — classifier must not
        // override the user's op
        let view = session.view();
        let pkg = view
            .packages
            .iter()
            .find(|p| p.entry.name == "harfbuzz")
            .unwrap();
        assert!(
            !pkg.entry.include,
            "harfbuzz must be excluded after user toggle"
        );
    }
}
