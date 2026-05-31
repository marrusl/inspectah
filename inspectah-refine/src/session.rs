use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use inspectah_core::fleet::classify_zone;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::fleet::PrevalenceZone;
use inspectah_core::types::redaction::RedactionState;
use inspectah_pipeline::render::containerfile::{
    render_containerfile, render_containerfile_with_originals,
};

use crate::baseline_summary::{BaselineSummary, derive_baseline_summary};
use crate::classify::{classify_configs, classify_packages};
use crate::fleet::variant_ops::{self, VariantProjectionState};
use crate::normalize::{normalize_config_defaults, normalize_package_defaults};
use crate::repo_index::RepoIndex;
use crate::types::{
    AnnotatedOp, ChangesSummary, ContentHash, FleetContext, ItemId, RefineError, RefineMode,
    RefineStats, RefinedView, RefinementOp, RepoProvenance, SectionChangeSummary, SectionKind,
    SectionStats, TriageBucket, UserPasswordOp,
};

pub struct RefineSession {
    original: InspectionSnapshot,
    repo_index: RepoIndex,
    baseline_available: bool,
    refine_mode: RefineMode,
    ops: Vec<RefinementOp>,
    cursor: usize,
    cached_view: Option<RefinedView>,
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

impl RefineSession {
    pub fn new(mut snapshot: InspectionSnapshot) -> Self {
        let repo_index = RepoIndex::build(&snapshot);
        let baseline_available = snapshot
            .rpm
            .as_ref()
            .and_then(|r| r.baseline_package_names.as_ref())
            .is_some();

        // Classify then normalize — materializes tier-aware defaults
        // into the snapshot BEFORE the op stack begins.
        let pkgs = classify_packages(&snapshot);
        let configs = classify_configs(&snapshot);
        normalize_package_defaults(&mut snapshot, &pkgs);
        normalize_config_defaults(&mut snapshot, &configs);

        // Detect fleet mode from snapshot metadata.
        let refine_mode = if let Some(ref fleet_meta) = snapshot.fleet_meta {
            let zones_active = fleet_meta.host_count >= 3;
            let mut zones = HashMap::new();

            // Classify multi-variant config paths by most-divergent variant.
            // When a path has 2+ variants (e.g., 3 hosts have version A, 2 have
            // version B), the path-level zone should reflect the divergence, not
            // hide it. We classify each variant individually and take the min
            // (Divergent < NearConsensus < Consensus).
            if let Some(ref cfg) = snapshot.config {
                let mut path_zones: HashMap<&str, PrevalenceZone> = HashMap::new();
                for entry in &cfg.files {
                    if let Some(ref prevalence) = entry.fleet {
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
                    if let Some(ref prevalence) = entry.fleet {
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
                    if let Some(ref prevalence) = entry.fleet {
                        dropin_sum
                            .entry(entry.path.as_str())
                            .and_modify(|(sum, _)| {
                                *sum += prevalence.count;
                            })
                            .or_insert((prevalence.count, prevalence.total));
                    }
                }
                for (path, (count, total)) in &dropin_sum {
                    let item_prev = inspectah_core::types::fleet::FleetPrevalence {
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
                    if let Some(ref prevalence) = entry.fleet {
                        quadlet_sum
                            .entry(entry.path.as_str())
                            .and_modify(|(sum, _)| {
                                *sum += prevalence.count;
                            })
                            .or_insert((prevalence.count, prevalence.total));
                    }
                }
                for (path, (count, total)) in &quadlet_sum {
                    let item_prev = inspectah_core::types::fleet::FleetPrevalence {
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

            RefineMode::Fleet(FleetContext {
                fleet_meta: fleet_meta.clone(),
                zones,
                total_hosts: fleet_meta.host_count,
                zones_active,
                repo_conflicts: snapshot.rpm_repo_conflicts.clone(),
            })
        } else {
            RefineMode::SingleHost
        };

        // Fleet prevalence gate: in fleet mode, items below full prevalence
        // default to excluded (strict intersection). This overrides the
        // single-host triage defaults set by normalize_package_defaults.
        // Applies to ALL section types with per-item fleet prevalence data.
        if matches!(refine_mode, RefineMode::Fleet(_)) {
            // Packages
            if let Some(ref mut rpm) = snapshot.rpm {
                for pkg in &mut rpm.packages_added {
                    if let Some(ref fp) = pkg.fleet
                        && fp.count < fp.total
                    {
                        pkg.include = false;
                    }
                }
            }

            // Config files
            if let Some(ref mut config) = snapshot.config {
                for entry in &mut config.files {
                    if let Some(ref fp) = entry.fleet
                        && fp.count < fp.total
                    {
                        entry.include = false;
                    }
                }
            }

            // Services
            if let Some(ref mut services) = snapshot.services {
                for svc in &mut services.state_changes {
                    if let Some(ref fp) = svc.fleet
                        && fp.count < fp.total
                    {
                        svc.include = false;
                    }
                }
                for dropin in &mut services.drop_ins {
                    if let Some(ref fp) = dropin.fleet
                        && fp.count < fp.total
                    {
                        dropin.include = false;
                    }
                }
            }

            // Containers (quadlets, compose)
            if let Some(ref mut containers) = snapshot.containers {
                for quadlet in &mut containers.quadlet_units {
                    if let Some(ref fp) = quadlet.fleet
                        && fp.count < fp.total
                    {
                        quadlet.include = false;
                    }
                }
                for compose in &mut containers.compose_files {
                    if let Some(ref fp) = compose.fleet
                        && fp.count < fp.total
                    {
                        compose.include = false;
                    }
                }
            }

            // Kernel/boot: sysctl overrides, loaded modules
            if let Some(ref mut kb) = snapshot.kernel_boot {
                for sysctl in &mut kb.sysctl_overrides {
                    if let Some(ref fp) = sysctl.fleet
                        && fp.count < fp.total
                    {
                        sysctl.include = false;
                    }
                }
                for module in &mut kb.loaded_modules {
                    if let Some(ref fp) = module.fleet
                        && fp.count < fp.total
                    {
                        module.include = false;
                    }
                }
            }

            // Scheduled tasks
            if let Some(ref mut sched) = snapshot.scheduled_tasks {
                for cron in &mut sched.cron_jobs {
                    if let Some(ref fp) = cron.fleet
                        && fp.count < fp.total
                    {
                        cron.include = false;
                    }
                }
                for timer in &mut sched.generated_timer_units {
                    if let Some(ref fp) = timer.fleet
                        && fp.count < fp.total
                    {
                        timer.include = false;
                    }
                }
            }

            // SELinux port labels
            if let Some(ref mut selinux) = snapshot.selinux {
                for port in &mut selinux.port_labels {
                    if let Some(ref fp) = port.fleet
                        && fp.count < fp.total
                    {
                        port.include = false;
                    }
                }
            }

            // Network: connections, firewall zones
            if let Some(ref mut network) = snapshot.network {
                for conn in &mut network.connections {
                    if let Some(ref fp) = conn.fleet
                        && fp.count < fp.total
                    {
                        conn.include = Some(false);
                    }
                }
                for zone in &mut network.firewall_zones {
                    if let Some(ref fp) = zone.fleet
                        && fp.count < fp.total
                    {
                        zone.include = false;
                    }
                }
            }

            // Non-RPM software
            if let Some(ref mut nonrpm) = snapshot.non_rpm_software {
                for item in &mut nonrpm.items {
                    if let Some(ref fp) = item.fleet
                        && fp.count < fp.total
                    {
                        item.include = false;
                    }
                }
            }

            // Storage: fstab entries
            if let Some(ref mut storage) = snapshot.storage {
                for entry in &mut storage.fstab_entries {
                    if let Some(ref fp) = entry.fleet
                        && fp.count < fp.total
                    {
                        entry.include = Some(false);
                    }
                }
            }
        }

        let mut session = Self {
            original: snapshot,
            repo_index,
            baseline_available,
            refine_mode,
            ops: Vec::new(),
            cursor: 0,
            cached_view: None,
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
            schema_version: 2,
            tarball_path: tarball.clone(),
            tarball_hash,
            ops: self.ops.clone(),
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

        // Direct restore: set ops and cursor atomically, skip per-op validation.
        // Safe because: (a) ops were validated on original apply, (b) tarball
        // hash match guarantees identical snapshot baseline. This preserves
        // the full redo tail because we bypass apply() which truncates.
        session.ops = saved.ops;
        session.cursor = saved.cursor;
        session.cached_view = None;
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

    /// Returns the fleet context if this session was created from a fleet snapshot.
    /// Returns `None` for single-host snapshots.
    pub fn fleet_context(&self) -> Option<&FleetContext> {
        match &self.refine_mode {
            RefineMode::Fleet(ctx) => Some(ctx),
            RefineMode::SingleHost => None,
        }
    }

    pub fn view(&self) -> &RefinedView {
        self.cached_view
            .as_ref()
            .expect("view is always computed after new() or mutation")
    }

    pub fn apply(&mut self, op: RefinementOp) -> Result<(), RefineError> {
        // Validate target exists
        self.validate_target(&op)?;

        // Check idempotency
        if self.is_op_noop(&op) {
            return Ok(());
        }

        // Truncate redo history at cursor
        self.ops.truncate(self.cursor);
        self.ops.push(op);
        self.cursor += 1;
        self.generation += 1;
        self.cached_view = None;
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
        self.cached_decisions = None;
        self.recompute_view();
        self.try_autosave();
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), RefineError> {
        if self.cursor >= self.ops.len() {
            return Err(RefineError::NothingToRedo);
        }
        self.cursor += 1;
        self.generation += 1;
        self.cached_view = None;
        self.cached_decisions = None;
        self.recompute_view();
        self.try_autosave();
        Ok(())
    }

    pub fn ops_history(&self) -> Vec<AnnotatedOp> {
        self.ops
            .iter()
            .enumerate()
            .map(|(i, op)| AnnotatedOp {
                op: op.clone(),
                active: i < self.cursor,
            })
            .collect()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_redo(&self) -> bool {
        self.cursor < self.ops.len()
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
            use inspectah_core::types::fleet::VariantSelection;
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

        let is_dirty = sections
            .iter()
            .any(|s| !s.included.is_empty() || !s.excluded.is_empty())
            || variants_changed > 0;

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
        render_refine_export(&projected, path, Some(&orig_inc))
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
                    | ItemId::ModuleStream { .. }
                    | ItemId::VersionLock { .. } => {}
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
            // Fleet variant ops: validate using projection state
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
        for op in &self.ops[..self.cursor] {
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
                // Other item kinds: never noop for now
                _ => false,
            },
            // User ops are never noop — always replay to ensure correctness
            RefinementOp::UserStrategy { .. } | RefinementOp::UserPassword(_) => false,
            // Fleet ops are never noop — projection-derived state makes idempotency detection fragile
            RefinementOp::SelectVariant { .. }
            | RefinementOp::EditVariant { .. }
            | RefinementOp::DiscardVariant { .. } => false,
        }
    }

    fn project_snapshot(&self) -> InspectionSnapshot {
        let mut snap = self.original.clone();
        let mut variant_state = VariantProjectionState::default();

        for op in &self.ops[..self.cursor] {
            match op {
                RefinementOp::SetInclude { item_id, include } => {
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
                // Fleet variant ops: accumulate into projection state
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

        snap
    }

    /// Compute the set of section IDs that are currently excluded based on the
    /// active op stack. A SetInclude(Repo, false) adds to the set, SetInclude(Repo, true) removes.
    fn excluded_sections_at(&self, _snap: &InspectionSnapshot) -> HashSet<String> {
        let mut excluded = HashSet::new();
        for op in &self.ops[..self.cursor] {
            if let RefinementOp::SetInclude {
                item_id: ItemId::Repo { path: section_id },
                include,
            } = op
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

        // Fleet triage scoring (when in fleet mode).
        if let RefineMode::Fleet(ref ctx) = self.refine_mode {
            for pkg in &mut all_packages {
                let item_id = ItemId::Package {
                    name: pkg.entry.name.clone(),
                    arch: pkg.entry.arch.clone(),
                };
                let prevalence_count = pkg
                    .entry
                    .fleet
                    .as_ref()
                    .map(|f| f.count.max(0) as u32)
                    .unwrap_or(0);
                let prevalence_total = pkg
                    .entry
                    .fleet
                    .as_ref()
                    .map(|f| f.total.max(0) as u32)
                    .unwrap_or(ctx.total_hosts as u32);
                let fleet_tag = crate::fleet::classify::classify_fleet_bucket(
                    ctx,
                    &item_id,
                    pkg.triage.bucket(),
                    pkg.triage.primary_reason.clone(),
                    prevalence_count,
                    prevalence_total,
                );
                pkg.triage.triage = fleet_tag.triage;
            }
            for cfg in &mut config_files {
                let item_id = ItemId::Config {
                    path: cfg.entry.path.clone(),
                };
                let prevalence_count = cfg
                    .entry
                    .fleet
                    .as_ref()
                    .map(|f| f.count.max(0) as u32)
                    .unwrap_or(0);
                let prevalence_total = cfg
                    .entry
                    .fleet
                    .as_ref()
                    .map(|f| f.total.max(0) as u32)
                    .unwrap_or(ctx.total_hosts as u32);
                let fleet_tag = crate::fleet::classify::classify_fleet_bucket(
                    ctx,
                    &item_id,
                    cfg.triage.bucket(),
                    cfg.triage.primary_reason.clone(),
                    prevalence_count,
                    prevalence_total,
                );
                cfg.triage.triage = fleet_tag.triage;
            }
        }

        // Build a set of packages that were normalized to include=false at
        // construction time (non-leaf Tier 2 dependencies). These are hidden
        // from the triage view because dnf resolves them automatically.
        // Packages the user explicitly excluded via ops remain visible so
        // the user can undo the exclusion.
        //
        // Fleet gate exclusion: In fleet mode, the prevalence gate also sets
        // include=false for non-universal items. Those must NOT be hidden —
        // they should be visible but unchecked. We distinguish them by
        // checking fleet prevalence: if count < total, the package was
        // excluded by the fleet gate, not by normalization.
        let is_fleet = matches!(self.refine_mode, RefineMode::Fleet(_));
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
                        // In fleet mode, skip packages excluded by the fleet
                        // prevalence gate — those should remain visible.
                        if is_fleet
                            && let Some(ref fp) = p.fleet
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

        let packages: Vec<_> = all_packages
            .into_iter()
            .filter(|p| {
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

            // Step 2: THEN apply leaf filter if available
            let is_fleet_snapshot = rpm.packages_added.iter().any(|pkg| pkg.fleet.is_some());
            if let Some(leaf_names) = rpm.leaf_packages.as_ref().filter(|_| !is_fleet_snapshot) {
                let leaf_set: HashSet<&str> = leaf_names.iter().map(|s| s.as_str()).collect();
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
                    })
                    .collect()
            } else {
                packages
            }
        } else {
            packages
        };

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
pub fn render_refine_export(
    snap: &InspectionSnapshot,
    tarball_path: &Path,
    original_includes: Option<&std::collections::HashMap<String, bool>>,
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

    // 2c. Remove any top-level artifacts outside the approved export contract.
    let allowed_top_level: std::collections::HashSet<&str> = [
        "config",
        "drop-ins",
        "quadlet",
        "flatpak",
        "sysctl",
        "tuned",
        "env-files",
        "fleet",
        "schema",
        "users",
        "inspection-snapshot.json",
        "Containerfile",
        "audit-report.md",
        "inspectah-users.ks",
        "inspectah-users.toml",
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
        render_containerfile_with_originals(snap, Some(&materialized_roots), orig)
    } else {
        render_containerfile(snap, Some(&materialized_roots))
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

    // 7. Create flat tarball (no prefix subdirectory)
    create_flat_tarball(out, tarball_path)?;

    Ok(())
}

/// Create a flat tarball (no prefix directory) from a source directory.
fn create_flat_tarball(source_dir: &Path, tarball_path: &Path) -> Result<(), RefineError> {
    let f = std::fs::File::create(tarball_path)?;
    let gz = flate2::write::GzEncoder::new(f, flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);

    let mut paths: Vec<_> = walkdir::WalkDir::new(source_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path() != source_dir)
        .map(|e| e.into_path())
        .collect();
    paths.sort();

    for path in &paths {
        let rel = path
            .strip_prefix(source_dir)
            .map_err(|e| RefineError::TarballError(e.to_string()))?;
        if path.is_dir() {
            tar.append_dir(rel, path)
                .map_err(|e| RefineError::TarballError(e.to_string()))?;
        } else {
            tar.append_path_with_name(path, rel)
                .map_err(|e| RefineError::TarballError(e.to_string()))?;
        }
    }

    tar.finish()
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
                source_repo: "baseos".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "glibc".into(),
                arch: "i686".into(),
                include: true,
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
                source_repo: "appstream".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "glibc".into(),
                arch: "x86_64".into(),
                include: true,
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
                source_repo: "appstream".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "glibc".into(),
                arch: "x86_64".into(),
                include: true,
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
                source_repo: "baseos".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "glibc".into(),
                arch: "i686".into(),
                include: true,
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
                source_repo: "appstream".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "kernel".into(),
                arch: "x86_64".into(),
                include: true,
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
                source_repo: "appstream".into(),
                state: PackageState::LocalInstall,
                ..Default::default()
            },
            PackageEntry {
                name: "kernel".into(),
                arch: "x86_64".into(),
                include: true,
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
                source_repo: "appstream".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "kernel".into(),
                arch: "x86_64".into(),
                include: true,
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
                        owning_package: Some("httpd".into()),
                        fleet: None,
                        attention_reason: None,
                    },
                    ServiceStateChange {
                        unit: "sshd.service".into(),
                        current_state: ServiceUnitState::Enabled,
                        default_state: Some(PresetDefault::Enable),
                        include: true,
                        owning_package: Some("openssh-server".into()),
                        fleet: None,
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
                        ..Default::default()
                    },
                    SystemdDropIn {
                        unit: "httpd.service".into(),
                        path: "/etc/systemd/system/httpd.service.d/timeout.conf".into(),
                        content: "[Service]\nTimeoutStartSec=120".into(),
                        include: true,
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
                        ..Default::default()
                    },
                    QuadletUnit {
                        path: "/etc/containers/systemd/db.container".into(),
                        name: "db.container".into(),
                        image: "docker.io/library/postgres:16".into(),
                        include: true,
                        ..Default::default()
                    },
                ],
                flatpak_apps: vec![
                    FlatpakApp {
                        app_id: "org.mozilla.firefox".into(),
                        remote: "flathub".into(),
                        branch: "stable".into(),
                        include: true,
                        ..Default::default()
                    },
                    FlatpakApp {
                        app_id: "org.gimp.GIMP".into(),
                        remote: "flathub".into(),
                        branch: "stable".into(),
                        include: true,
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
                        fleet: None,
                    },
                    SysctlOverride {
                        key: "vm.swappiness".into(),
                        runtime: "10".into(),
                        default: "60".into(),
                        source: "/etc/sysctl.d/99-custom.conf".into(),
                        include: true,
                        fleet: None,
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
                ..Default::default()
            }],
            flatpak_apps: vec![FlatpakApp {
                app_id: "org.example.App".into(),
                remote: "flathub".into(),
                branch: "stable".into(),
                include: true,
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
        render_refine_export(&snap, &tarball_path, None).unwrap();

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

        // Promoted roots must appear in the tarball
        assert!(
            entries.iter().any(|e| e.starts_with("drop-ins/")),
            "tarball must contain drop-ins/ root. entries: {entries:?}"
        );
        assert!(
            entries
                .iter()
                .any(|e| e == "quadlet/myapp.container" || e == "quadlet/myapp.container/"),
            "tarball must contain quadlet/myapp.container. entries: {entries:?}"
        );
        assert!(
            entries.iter().any(|e| e.starts_with("flatpak/")),
            "tarball must contain flatpak/ root. entries: {entries:?}"
        );
        assert!(
            entries.iter().any(|e| e.starts_with("sysctl/")),
            "tarball must contain sysctl/ root. entries: {entries:?}"
        );
        assert!(
            entries.iter().any(|e| e.starts_with("tuned/")),
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
                .any(|e| e.starts_with("config/etc/systemd/system/httpd.service.d/")),
            "drop-ins must NOT be under config/. entries: {entries:?}"
        );
        assert!(
            !entries
                .iter()
                .any(|e| e.starts_with("config/etc/containers/systemd/")),
            "quadlets must NOT be under config/. entries: {entries:?}"
        );
        assert!(
            !entries.iter().any(|e| e.starts_with("config/etc/tuned/")),
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
                    owning_package: Some("httpd".into()),
                    fleet: None,
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
                    owning_package: Some("httpd".into()),
                    fleet: None,
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
}
