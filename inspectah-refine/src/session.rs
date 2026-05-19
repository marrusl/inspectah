use std::collections::{HashMap, HashSet};
use std::path::Path;

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::ConfigFileKind;
use inspectah_core::types::redaction::RedactionState;
use inspectah_pipeline::render::containerfile::render_containerfile;

use crate::attention::{compute_config_attention, compute_package_attention};
use crate::baseline_summary::{BaselineSummary, derive_baseline_summary};
use crate::normalize::{normalize_config_defaults, normalize_package_defaults};
use crate::repo_index::RepoIndex;
use crate::types::{
    AnnotatedOp, AttentionLevel, ChangesSummary, PackageTarget, RefineError, RefineStats,
    RefinedView, RefinementOp, RepoProvenance, UserPasswordOp,
};

pub struct RefineSession {
    original: InspectionSnapshot,
    repo_index: RepoIndex,
    baseline_available: bool,
    ops: Vec<RefinementOp>,
    cursor: usize,
    cached_view: Option<RefinedView>,
    generation: u64,
    /// Tracks which context items the user has viewed in the UI.
    /// Format: "section:item_id" (e.g., "packages:httpd.x86_64").
    /// Non-serialized — excluded from tarball export.
    viewed: HashSet<String>,
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
        let pkgs = compute_package_attention(&snapshot);
        let configs = compute_config_attention(&snapshot);
        normalize_package_defaults(&mut snapshot, &pkgs);
        normalize_config_defaults(&mut snapshot, &configs);

        let mut session = Self {
            original: snapshot,
            repo_index,
            baseline_available,
            ops: Vec::new(),
            cursor: 0,
            cached_view: None,
            generation: 0,
            viewed: HashSet::new(),
        };
        session.recompute_view();
        session
    }

    pub fn repo_index(&self) -> &RepoIndex {
        &self.repo_index
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
        self.recompute_view();
        Ok(())
    }

    pub fn undo(&mut self) -> Result<(), RefineError> {
        if self.cursor == 0 {
            return Err(RefineError::NothingToUndo);
        }
        self.cursor -= 1;
        self.generation += 1;
        self.cached_view = None;
        self.recompute_view();
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), RefineError> {
        if self.cursor >= self.ops.len() {
            return Err(RefineError::NothingToRedo);
        }
        self.cursor += 1;
        self.generation += 1;
        self.cached_view = None;
        self.recompute_view();
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
        let mut packages_included = Vec::new();
        let mut packages_excluded = Vec::new();
        let mut configs_included = Vec::new();
        let mut configs_excluded = Vec::new();

        if let (Some(orig_rpm), Some(proj_rpm)) = (&self.original.rpm, &projected.rpm) {
            for (orig, proj) in orig_rpm.packages_added.iter().zip(&proj_rpm.packages_added) {
                let target = PackageTarget {
                    name: orig.name.clone(),
                    arch: orig.arch.clone(),
                };
                if orig.include != proj.include {
                    if proj.include {
                        packages_included.push(target);
                    } else {
                        packages_excluded.push(target);
                    }
                }
            }
        }

        if let (Some(orig_cfg), Some(proj_cfg)) = (&self.original.config, &projected.config) {
            for (orig, proj) in orig_cfg.files.iter().zip(&proj_cfg.files) {
                if orig.include != proj.include {
                    if proj.include {
                        configs_included.push(orig.path.clone());
                    } else {
                        configs_excluded.push(orig.path.clone());
                    }
                }
            }
        }

        let repos_excluded: Vec<String> =
            self.excluded_sections_at(&projected).into_iter().collect();

        let is_dirty = !packages_included.is_empty()
            || !packages_excluded.is_empty()
            || !configs_included.is_empty()
            || !configs_excluded.is_empty()
            || !repos_excluded.is_empty();

        ChangesSummary {
            packages_included,
            packages_excluded,
            configs_included,
            configs_excluded,
            repos_excluded,
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
        render_refine_export(&projected, path)
    }

    // --- Private helpers ---

    fn validate_target(&self, op: &RefinementOp) -> Result<(), RefineError> {
        match op {
            RefinementOp::ExcludePackage(target) | RefinementOp::IncludePackage(target) => {
                let found = self
                    .original
                    .rpm
                    .as_ref()
                    .map(|r| r.packages_added.iter().any(|e| target.matches(e)))
                    .unwrap_or(false);
                if !found {
                    return Err(RefineError::UnknownTarget(target.to_string()));
                }
            }
            RefinementOp::ExcludeConfig { path } | RefinementOp::IncludeConfig { path } => {
                let found = self
                    .original
                    .config
                    .as_ref()
                    .map(|c| c.files.iter().any(|e| e.path == path.to_string_lossy()))
                    .unwrap_or(false);
                if !found {
                    return Err(RefineError::UnknownTarget(
                        path.to_string_lossy().to_string(),
                    ));
                }
            }
            RefinementOp::ExcludeRepo { section_id } => {
                if RepoIndex::is_distro_repo(section_id) {
                    return Err(RefineError::BadRequest(format!(
                        "cannot exclude distro repo: {section_id}"
                    )));
                }
                match self.repo_index.provenance(section_id) {
                    RepoProvenance::Verified => {}
                    prov => {
                        return Err(RefineError::BadRequest(format!(
                            "cannot exclude repo '{section_id}': provenance is {prov:?}"
                        )));
                    }
                }
            }
            RefinementOp::IncludeRepo { section_id } => {
                if RepoIndex::is_distro_repo(section_id) {
                    return Err(RefineError::BadRequest(format!(
                        "cannot toggle distro repo: {section_id}"
                    )));
                }
                let prov = self.repo_index.provenance(section_id);
                if !matches!(prov, RepoProvenance::Verified) {
                    return Err(RefineError::BadRequest(format!(
                        "cannot include repo '{section_id}': provenance is {prov:?}"
                    )));
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

    fn is_op_noop(&self, op: &RefinementOp) -> bool {
        let projected = self.project_snapshot();
        match op {
            RefinementOp::ExcludePackage(target) => {
                projected
                    .rpm
                    .as_ref()
                    .and_then(|r| r.packages_added.iter().find(|e| target.matches(e)))
                    .map(|e| !e.include) // already excluded = noop
                    .unwrap_or(false)
            }
            RefinementOp::IncludePackage(target) => {
                projected
                    .rpm
                    .as_ref()
                    .and_then(|r| r.packages_added.iter().find(|e| target.matches(e)))
                    .map(|e| e.include) // already included = noop
                    .unwrap_or(false)
            }
            RefinementOp::ExcludeConfig { path } => projected
                .config
                .as_ref()
                .and_then(|c| c.files.iter().find(|e| e.path == path.to_string_lossy()))
                .map(|e| !e.include)
                .unwrap_or(false),
            RefinementOp::IncludeConfig { path } => projected
                .config
                .as_ref()
                .and_then(|c| c.files.iter().find(|e| e.path == path.to_string_lossy()))
                .map(|e| e.include)
                .unwrap_or(false),
            RefinementOp::ExcludeRepo { section_id } => {
                // Noop if the section is already in the excluded set
                let excluded = self.excluded_sections_at(&projected);
                excluded.contains(section_id)
            }
            RefinementOp::IncludeRepo { section_id } => {
                // Noop if the section is NOT in the excluded set
                let excluded = self.excluded_sections_at(&projected);
                !excluded.contains(section_id)
            }
            // User ops are never noop — always replay to ensure correctness
            RefinementOp::UserStrategy { .. } | RefinementOp::UserPassword(_) => false,
        }
    }

    fn project_snapshot(&self) -> InspectionSnapshot {
        let mut snap = self.original.clone();

        for op in &self.ops[..self.cursor] {
            match op {
                RefinementOp::ExcludePackage(target) => {
                    if let Some(ref mut rpm) = snap.rpm {
                        if let Some(pkg) = rpm.packages_added.iter_mut().find(|e| target.matches(e))
                        {
                            pkg.include = false;
                        }
                    }
                }
                RefinementOp::IncludePackage(target) => {
                    if let Some(ref mut rpm) = snap.rpm {
                        if let Some(pkg) = rpm.packages_added.iter_mut().find(|e| target.matches(e))
                        {
                            pkg.include = true;
                        }
                    }
                }
                RefinementOp::ExcludeConfig { path } => {
                    if let Some(ref mut config) = snap.config {
                        if let Some(entry) = config
                            .files
                            .iter_mut()
                            .find(|e| e.path == path.to_string_lossy())
                        {
                            entry.include = false;
                        }
                    }
                }
                RefinementOp::IncludeConfig { path } => {
                    if let Some(ref mut config) = snap.config {
                        if let Some(entry) = config
                            .files
                            .iter_mut()
                            .find(|e| e.path == path.to_string_lossy())
                        {
                            entry.include = true;
                        }
                    }
                }
                RefinementOp::ExcludeRepo { section_id } => {
                    // Compute excluded sections BEFORE mutably borrowing snap
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
                                if all_sections_excluded {
                                    if let Some(rf) =
                                        rpm.repo_files.iter_mut().find(|r| r.path == *file_path)
                                    {
                                        rf.include = false;
                                    }
                                }
                            }
                        }

                        // 3. For GPG keys: exclude only if ALL sections
                        // that reference this key are excluded
                        if let Some(key_paths) = self.repo_index.gpg_keys_by_section.get(section_id)
                        {
                            for key_path in key_paths {
                                if let Some(referencing_sections) =
                                    self.repo_index.sections_by_gpg_key.get(key_path)
                                {
                                    let all_excluded = referencing_sections
                                        .iter()
                                        .all(|sid| excluded_sections.contains(sid));
                                    if all_excluded {
                                        if let Some(k) =
                                            rpm.gpg_keys.iter_mut().find(|g| g.path == *key_path)
                                        {
                                            k.include = false;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                RefinementOp::IncludeRepo { section_id } => {
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
                                if let Some(rf) =
                                    rpm.repo_files.iter_mut().find(|r| r.path == *file_path)
                                {
                                    rf.include = true;
                                }
                            }
                        }

                        // 3. Re-enable GPG keys for this section
                        if let Some(key_paths) = self.repo_index.gpg_keys_by_section.get(section_id)
                        {
                            for key_path in key_paths {
                                if let Some(k) =
                                    rpm.gpg_keys.iter_mut().find(|g| g.path == *key_path)
                                {
                                    k.include = true;
                                }
                            }
                        }
                    }
                }
                RefinementOp::UserStrategy { username, strategy } => {
                    if let Some(ref mut ug) = snap.users_groups {
                        if let Some(user) = ug.users.iter_mut().find(|u| {
                            u.get("name").and_then(|v| v.as_str()) == Some(username)
                        }) {
                            user.as_object_mut().map(|m| {
                                m.insert(
                                    "containerfile_strategy".to_string(),
                                    serde_json::to_value(strategy).unwrap(),
                                );
                            });
                        }
                    }
                }
                RefinementOp::UserPassword(pw_op) => {
                    match pw_op {
                        UserPasswordOp::New { username, hash } => {
                            if let Some(ref mut ug) = snap.users_groups {
                                if let Some(user) = ug.users.iter_mut().find(|u| {
                                    u.get("name").and_then(|v| v.as_str()) == Some(username)
                                }) {
                                    if let Some(m) = user.as_object_mut() {
                                        m.insert(
                                            "password_choice".to_string(),
                                            serde_json::json!("new"),
                                        );
                                        if let Some(h) = hash {
                                            m.insert(
                                                "password_hash".to_string(),
                                                serde_json::json!(h),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        UserPasswordOp::None { username } => {
                            if let Some(ref mut ug) = snap.users_groups {
                                if let Some(user) = ug.users.iter_mut().find(|u| {
                                    u.get("name").and_then(|v| v.as_str()) == Some(username)
                                }) {
                                    if let Some(m) = user.as_object_mut() {
                                        m.insert(
                                            "password_choice".to_string(),
                                            serde_json::json!("none"),
                                        );
                                        // CLEAR password_hash
                                        m.remove("password_hash");
                                    }
                                }
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

                            if let Some(ref mut ug) = snap.users_groups {
                                if let Some(user) = ug.users.iter_mut().find(|u| {
                                    u.get("name").and_then(|v| v.as_str()) == Some(username)
                                }) {
                                    if let Some(m) = user.as_object_mut() {
                                        m.insert(
                                            "password_choice".to_string(),
                                            serde_json::json!("preserve"),
                                        );
                                        match original_hash {
                                            Some(h) => {
                                                m.insert(
                                                    "password_hash".to_string(),
                                                    serde_json::json!(h),
                                                );
                                            }
                                            None => {
                                                m.remove("password_hash");
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

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
    /// active op stack. An ExcludeRepo adds to the set, an IncludeRepo removes.
    fn excluded_sections_at(&self, _snap: &InspectionSnapshot) -> HashSet<String> {
        let mut excluded = HashSet::new();
        for op in &self.ops[..self.cursor] {
            match op {
                RefinementOp::ExcludeRepo { section_id } => {
                    excluded.insert(section_id.clone());
                }
                RefinementOp::IncludeRepo { section_id } => {
                    excluded.remove(section_id);
                }
                _ => {}
            }
        }
        excluded
    }

    fn recompute_view(&mut self) {
        let projected = self.project_snapshot();
        let all_packages = compute_package_attention(&projected);
        let config_files = compute_config_attention(&projected);

        // Build a set of packages that were normalized to include=false at
        // construction time (non-leaf Tier 2 dependencies). These are hidden
        // from the triage view because dnf resolves them automatically.
        // Packages the user explicitly excluded via ops remain visible so
        // the user can undo the exclusion.
        let hidden_deps: HashSet<(&str, &str)> = self
            .original
            .rpm
            .as_ref()
            .map(|r| {
                r.packages_added
                    .iter()
                    .filter(|p| !p.include)
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
                {
                    let primary = p.attention.first().map(|t| t.level);
                    if !matches!(primary, Some(AttentionLevel::NeedsReview)) {
                        return false;
                    }
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

                        let primary_level = pkg.attention.first().map(|t| t.level);
                        let original_include = original_package_includes
                            .get(&(pkg.entry.name.as_str(), pkg.entry.arch.as_str()))
                            .copied()
                            .unwrap_or(pkg.entry.include);

                        leaf_set.contains(package_id.as_str())
                            || matches!(primary_level, Some(AttentionLevel::NeedsReview))
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
        let containerfile_preview = render_containerfile(&projected, Some(&materialized_roots));
        drop(preview_dir);

        let stats = RefineStats {
            total_packages: packages.len(),
            included_packages: packages.iter().filter(|p| p.entry.include).count(),
            excluded_packages: packages.iter().filter(|p| !p.entry.include).count(),
            total_configs: config_files.len(),
            included_configs: config_files.iter().filter(|c| c.entry.include).count(),
            excluded_configs: config_files.iter().filter(|c| !c.entry.include).count(),
            needs_review_count: packages
                .iter()
                .filter(|p| {
                    p.attention
                        .iter()
                        .any(|t| t.level == AttentionLevel::NeedsReview)
                })
                .count()
                + config_files
                    .iter()
                    .filter(|c| {
                        c.attention
                            .iter()
                            .any(|t| t.level == AttentionLevel::NeedsReview)
                    })
                    .count(),
            ops_applied: self.cursor,
            can_undo: self.can_undo(),
            can_redo: self.can_redo(),
            package_managed_configs: projected
                .config
                .as_ref()
                .map(|c| {
                    c.files
                        .iter()
                        .filter(|f| {
                            !f.include
                                && matches!(
                                    f.kind,
                                    ConfigFileKind::RpmOwnedDefault | ConfigFileKind::BaselineMatch
                                )
                        })
                        .count()
                })
                .unwrap_or(0),
            baseline_available: self.baseline_available,
        };

        self.cached_view = Some(RefinedView {
            packages,
            config_files,
            containerfile_preview,
            stats,
            generation: self.generation,
        });
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
    // write_config_tree() can emit drop-ins/, quadlet/, flatpak/ at root.
    let allowed_top_level: std::collections::HashSet<&str> = [
        "config",
        "env-files",
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
    let containerfile = render_containerfile(snap, Some(&materialized_roots));
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
            schema_version: 15,
            rpm: Some(RpmSection::default()),
            ..Default::default()
        }
    }

    #[test]
    fn view_filters_to_canonical_leaf_packages_when_available() {
        let mut snap = test_snapshot();
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
        assert_eq!(view.stats.total_packages, 2);
    }

    #[test]
    fn containerfile_preview_only_includes_leaf_packages() {
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
            view.stats.total_packages, 1,
            "total_packages should be leaf count"
        );
        assert_eq!(
            view.stats.included_packages, 1,
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
            schema_version: 15,
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
}
