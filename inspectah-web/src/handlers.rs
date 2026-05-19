use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::completeness::Completeness;
use inspectah_core::types::config::ConfigFileKind;
use inspectah_core::types::rpm::{VersionChange, VersionChangeDirection};
use inspectah_core::types::users::UserContainerfileStrategy;
use inspectah_refine::baseline_summary::BaselineSummary;
use inspectah_refine::repo_index::{DISTRO_REPOS, RepoIndex};
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{RefinedView, RefinementOp, RepoProvenance, UserPasswordOp};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use crate::error::AppError;

/// Shared application state. Wraps the session mutex alongside caches
/// that are immutable for the session lifetime.
pub struct AppState {
    pub session: Arc<Mutex<RefineSession>>,
    pub sections_cache: OnceLock<Vec<ContextSection>>,
}

// -- Context section DTOs (presentation layer only) -----------------------

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ContextSection {
    pub id: String,
    pub display_name: String,
    pub items: Vec<ContextItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty_reason: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ContextItem {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub detail: Option<String>,
    pub searchable_text: String,
}

// -- Repo group + view response DTOs --------------------------------------

#[derive(Serialize, Clone, Debug)]
pub struct RepoGroupInfo {
    pub section_id: String,
    pub provenance: RepoProvenance,
    pub is_distro: bool,
    pub package_count: usize,
    pub enabled: bool,
}

#[derive(Serialize)]
pub struct ViewResponse {
    #[serde(flatten)]
    pub view: RefinedView,
    pub repo_groups: Vec<RepoGroupInfo>,
    pub baseline_summary: Option<BaselineSummary>,
    pub version_changes: Vec<VersionChangeEntry>,
    pub leaf_dep_tree: std::collections::HashMap<String, Vec<String>>,
    pub users_groups_decisions: Vec<serde_json::Value>,
    pub session_is_sensitive: bool,
}

#[derive(Serialize)]
pub struct VersionChangeEntry {
    pub name: String,
    pub arch: String,
    pub host_version: String,
    pub base_version: String,
    pub host_epoch: String,
    pub base_epoch: String,
    pub direction: String,
}

// -- Viewed tracking request body -----------------------------------------

#[derive(Deserialize)]
pub struct ViewedRequest {
    pub id: String,
}

// -- Tarball export request body ------------------------------------------

#[derive(Deserialize)]
pub struct TarballRequest {
    pub generation: u64,
}

// -- User endpoint request bodies -----------------------------------------

#[derive(Deserialize)]
pub struct UserStrategyRequest {
    pub username: String,
    pub strategy: String,
}

#[derive(Deserialize)]
pub struct UserPasswordRequest {
    pub username: String,
    pub choice: String,
    pub hash: Option<String>,
}

// -- Handlers -------------------------------------------------------------

pub async fn health(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let session = state.session.lock().unwrap();
    let snap = session.snapshot();

    let hostname = snap
        .meta
        .get("hostname")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let (os_name, os_version, os_id) = snap
        .os_release
        .as_ref()
        .map(|os| {
            // pretty_name with name fallback
            let name = if !os.pretty_name.is_empty() {
                os.pretty_name.clone()
            } else {
                os.name.clone()
            };
            (name, os.version_id.clone(), os.id.clone())
        })
        .unwrap_or_default();

    let system_type = serde_json::to_value(&snap.system_type).unwrap_or(json!("unknown"));
    let completeness = match &snap.completeness {
        Completeness::Complete => "complete",
        Completeness::Partial { .. } => "partial",
        Completeness::Incomplete { .. } => "incomplete",
    };

    Json(json!({
        "status": "ok",
        "host": {
            "hostname": hostname,
            "os_name": os_name,
            "os_version": os_version,
            "os_id": os_id,
            "system_type": system_type,
            "schema_version": snap.schema_version,
        },
        "completeness": completeness,
        "policy": {
            "distro_repos": DISTRO_REPOS,
        },
    }))
}

pub async fn get_view(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let session = state.session.lock().unwrap();
    let response = build_view_response(&session);
    Json(serde_json::to_value(&response).unwrap())
}

/// Build a complete `ViewResponse` from session state (view + repo groups).
fn build_view_response(session: &RefineSession) -> ViewResponse {
    let view = session.view().clone();
    let repo_groups = build_repo_groups(session);
    let baseline_summary = session.baseline_summary();
    let version_changes: Vec<VersionChangeEntry> = session
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
        .unwrap_or_default();
    let is_fleet = session
        .snapshot()
        .rpm
        .as_ref()
        .map(|rpm| rpm.packages_added.iter().any(|p| p.fleet.is_some()))
        .unwrap_or(false);
    let leaf_dep_tree: std::collections::HashMap<String, Vec<String>> = if is_fleet {
        std::collections::HashMap::new()
    } else {
        serde_json::from_value(session.leaf_dep_tree()).unwrap_or_default()
    };
    let users_groups_decisions = session
        .snapshot_projected()
        .users_groups
        .map(|ug| ug.users)
        .unwrap_or_default();
    let session_is_sensitive = session.is_sensitive();
    ViewResponse {
        view,
        repo_groups,
        baseline_summary,
        version_changes,
        leaf_dep_tree,
        users_groups_decisions,
        session_is_sensitive,
    }
}

/// Build `RepoGroupInfo` entries from the session's repo index and current view.
fn build_repo_groups(session: &RefineSession) -> Vec<RepoGroupInfo> {
    let view = session.view();
    let repo_index = session.repo_index();
    let changes = session.pending_changes();
    let excluded: BTreeSet<&str> = changes.repos_excluded.iter().map(|s| s.as_str()).collect();

    // Count visible packages per source_repo (lowercased for consistency
    // with RepoIndex, which normalizes section IDs to lowercase).
    let mut repo_counts: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for pkg in &view.packages {
        let section = pkg.entry.source_repo.to_lowercase();
        *repo_counts.entry(section).or_insert(0) += 1;
    }

    // Also include repos known to the index but not visible (0-count)
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
            let enabled = !excluded.contains(section_id.as_str());
            RepoGroupInfo {
                section_id,
                provenance,
                is_distro,
                package_count,
                enabled,
            }
        })
        .collect();

    // Sort: distro repos first, then by section_id
    groups.sort_by(|a, b| {
        b.is_distro
            .cmp(&a.is_distro)
            .then_with(|| a.section_id.cmp(&b.section_id))
    });

    groups
}

pub async fn apply_op(
    State(state): State<Arc<AppState>>,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, AppError> {
    let op: RefinementOp = serde_json::from_slice(&body).map_err(|e| {
        AppError(inspectah_refine::types::RefineError::BadRequest(format!(
            "invalid operation: {e}"
        )))
    })?;
    let mut session = state.session.lock().unwrap();
    session.apply(op).map_err(AppError)?;
    Ok(Json(
        serde_json::to_value(&build_view_response(&session)).unwrap(),
    ))
}

pub async fn undo(
    State(state): State<Arc<AppState>>,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, AppError> {
    // Require JSON body to make this a non-simple request (triggers CORS preflight)
    let _: serde_json::Value = serde_json::from_slice(&body).map_err(|_| {
        AppError(inspectah_refine::types::RefineError::BadRequest(
            "request body must be JSON (use {})".into(),
        ))
    })?;
    let mut session = state.session.lock().unwrap();
    session.undo().map_err(AppError)?;
    Ok(Json(
        serde_json::to_value(&build_view_response(&session)).unwrap(),
    ))
}

pub async fn redo(
    State(state): State<Arc<AppState>>,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, AppError> {
    // Require JSON body to make this a non-simple request (triggers CORS preflight)
    let _: serde_json::Value = serde_json::from_slice(&body).map_err(|_| {
        AppError(inspectah_refine::types::RefineError::BadRequest(
            "request body must be JSON (use {})".into(),
        ))
    })?;
    let mut session = state.session.lock().unwrap();
    session.redo().map_err(AppError)?;
    Ok(Json(
        serde_json::to_value(&build_view_response(&session)).unwrap(),
    ))
}

pub async fn get_ops(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let session = state.session.lock().unwrap();
    Json(serde_json::to_value(session.ops_history()).unwrap())
}

pub async fn get_changes(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let session = state.session.lock().unwrap();
    Json(serde_json::to_value(session.pending_changes()).unwrap())
}

pub async fn export_tarball(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<axum::response::Response, AppError> {
    // Parse generation from request body — malformed JSON → 400
    let req: TarballRequest = serde_json::from_slice(&body).map_err(|_| {
        AppError(inspectah_refine::types::RefineError::BadRequest(
            "request body must be JSON with 'generation' field".into(),
        ))
    })?;

    // Snapshot state under the lock, then release before expensive work.
    let (projected, _generation, sensitive) = {
        let session = state.session.lock().unwrap();
        if req.generation != session.generation() {
            return Err(AppError(
                inspectah_refine::types::RefineError::StaleGeneration {
                    expected: req.generation,
                    actual: session.generation(),
                },
            ));
        }
        (
            session.snapshot_projected(),
            session.generation(),
            session.is_sensitive(),
        )
    };
    // Lock is released here.

    // Export gating: require explicit acknowledgment for sensitive sessions.
    if sensitive {
        let ack = headers
            .get("x-acknowledge-sensitive")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if ack != "true" {
            let summary = build_sensitivity_summary(&projected);
            return Ok((
                StatusCode::PRECONDITION_REQUIRED,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                serde_json::to_vec(&summary).unwrap_or_default(),
            )
                .into_response());
        }
    }

    // Expensive render + tar work happens outside the lock via spawn_blocking.
    let bytes = tokio::task::spawn_blocking(
        move || -> Result<Vec<u8>, inspectah_refine::types::RefineError> {
            let tempdir = tempfile::tempdir()?;
            let tarball_path = tempdir.path().join("inspectah-refine-output.tar.gz");
            inspectah_refine::session::render_refine_export(&projected, &tarball_path)?;
            Ok(std::fs::read(&tarball_path)?)
        },
    )
    .await
    .map_err(|e| {
        AppError(inspectah_refine::types::RefineError::TarballError(
            e.to_string(),
        ))
    })?
    .map_err(AppError)?;

    Ok((
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, "application/gzip"),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"inspectah-refine-output.tar.gz\"",
            ),
        ],
        bytes,
    )
        .into_response())
}

// -- User decision endpoints -----------------------------------------------

pub async fn user_strategy(
    State(state): State<Arc<AppState>>,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, AppError> {
    let req: UserStrategyRequest = serde_json::from_slice(&body).map_err(|e| {
        AppError(inspectah_refine::types::RefineError::BadRequest(format!(
            "invalid user strategy request: {e}"
        )))
    })?;
    let strategy = match req.strategy.as_str() {
        "skip" => UserContainerfileStrategy::Skip,
        "useradd" => UserContainerfileStrategy::Useradd,
        other => {
            return Err(AppError(
                inspectah_refine::types::RefineError::BadRequest(format!(
                    "unknown strategy: {other} (expected \"skip\" or \"useradd\")"
                )),
            ));
        }
    };
    let op = RefinementOp::UserStrategy {
        username: req.username,
        strategy,
    };
    let mut session = state.session.lock().unwrap();
    session.apply(op).map_err(AppError)?;
    Ok(Json(
        serde_json::to_value(&build_view_response(&session)).unwrap(),
    ))
}

pub async fn user_password(
    State(state): State<Arc<AppState>>,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, AppError> {
    let req: UserPasswordRequest = serde_json::from_slice(&body).map_err(|e| {
        AppError(inspectah_refine::types::RefineError::BadRequest(format!(
            "invalid user password request: {e}"
        )))
    })?;
    let pw_op = match req.choice.as_str() {
        "none" => UserPasswordOp::None {
            username: req.username,
        },
        "preserve" => {
            // Validate: snapshot must have preserved_credentials and user must
            // have a password_hash — otherwise "preserve" is an impossible state.
            let session = state.session.lock().unwrap();
            let snap = session.snapshot();
            if !snap.preserved_credentials {
                return Err(AppError(
                    inspectah_refine::types::RefineError::BadRequest(
                        "cannot preserve password: snapshot does not contain preserved credentials"
                            .into(),
                    ),
                ));
            }
            let has_hash = snap
                .users_groups
                .as_ref()
                .and_then(|ug| {
                    ug.users.iter().find(|u| {
                        u.get("name").and_then(|v| v.as_str()) == Some(&req.username)
                    })
                })
                .and_then(|u| u.get("password_hash"))
                .and_then(|v| v.as_str())
                .map(|s| !s.is_empty())
                .unwrap_or(false);
            if !has_hash {
                return Err(AppError(
                    inspectah_refine::types::RefineError::BadRequest(format!(
                        "cannot preserve password for \"{}\": user has no password hash",
                        req.username
                    )),
                ));
            }
            drop(session); // release lock before re-acquiring below
            UserPasswordOp::Preserve {
                username: req.username,
            }
        }
        "new" => {
            // Validate: hash must be provided, non-empty, and in a recognized
            // crypt(3) format ($6$, $5$, or $y$).
            let hash = req.hash.as_deref().unwrap_or("");
            if hash.is_empty() {
                return Err(AppError(
                    inspectah_refine::types::RefineError::BadRequest(
                        "password choice \"new\" requires a non-empty \"hash\" field".into(),
                    ),
                ));
            }
            if !hash.starts_with("$6$") && !hash.starts_with("$5$") && !hash.starts_with("$y$") {
                return Err(AppError(
                    inspectah_refine::types::RefineError::BadRequest(
                        "invalid hash format: must start with $6$, $5$, or $y$ (crypt(3) format)"
                            .into(),
                    ),
                ));
            }
            UserPasswordOp::New {
                username: req.username,
                hash: req.hash,
            }
        }
        other => {
            return Err(AppError(
                inspectah_refine::types::RefineError::BadRequest(format!(
                    "unknown password choice: {other} (expected \"none\", \"preserve\", or \"new\")"
                )),
            ));
        }
    };
    let op = RefinementOp::UserPassword(pw_op);
    let mut session = state.session.lock().unwrap();
    session.apply(op).map_err(AppError)?;
    Ok(Json(
        serde_json::to_value(&build_view_response(&session)).unwrap(),
    ))
}

// -- User preview query params --------------------------------------------

#[derive(Deserialize)]
pub struct UserPreviewQuery {
    #[serde(default)]
    pub reveal: Option<bool>,
}

pub async fn user_preview(
    State(state): State<Arc<AppState>>,
    Query(params): Query<UserPreviewQuery>,
) -> impl IntoResponse {
    let session = state.session.lock().unwrap();
    let sensitive = session.is_sensitive();
    let projected = session.snapshot_projected();
    let kickstart = inspectah_pipeline::render::users::render_kickstart(&projected);
    let blueprint_toml = inspectah_pipeline::render::users::render_blueprint_toml(&projected);

    let reveal = params.reveal.unwrap_or(false);
    let (kickstart, blueprint_toml) = if sensitive && !reveal {
        (
            redact_sensitive_content(&kickstart),
            redact_sensitive_content(&blueprint_toml),
        )
    } else {
        (kickstart, blueprint_toml)
    };

    Json(json!({
        "kickstart": kickstart,
        "blueprint_toml": blueprint_toml,
        "sensitive": sensitive,
    }))
}

/// Redact crypt(3) hashes and SSH key content from rendered artifact strings.
fn redact_sensitive_content(content: &str) -> String {
    use regex::Regex;

    // Redact crypt(3) hashes: $6$..., $5$..., $y$... patterns.
    // Matches the full hash from the $ prefix through the hash characters.
    let crypt_re =
        Regex::new(r#"\$(?:6|5|y)\$[^\s'""]+"#).expect("crypt regex");
    let result = crypt_re.replace_all(content, "<REDACTED>");

    // Redact SSH key base64 blobs, keeping the key type prefix.
    // Matches: ssh-rsa AAAA..., ssh-ed25519 AAAA..., ecdsa-sha2-nistp256 AAAA..., etc.
    let ssh_re = Regex::new(
        r#"((?:ssh-(?:rsa|ed25519|dss)|ecdsa-sha2-nistp(?:256|384|521))\s+)\S+"#,
    )
    .expect("ssh regex");
    let result = ssh_re.replace_all(&result, "${1}<REDACTED>");

    result.into_owned()
}

/// Build a summary of why the session is considered sensitive.
fn build_sensitivity_summary(snap: &InspectionSnapshot) -> serde_json::Value {
    let mut reasons = Vec::new();
    if snap.sensitive_snapshot {
        reasons.push("snapshot contains sensitive data".to_string());
    }
    if let Some(ug) = &snap.users_groups {
        for user in &ug.users {
            let name = user
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
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
                reasons.push(format!("user \"{name}\" has a new password hash"));
            }
        }
    }
    json!({
        "error": "session contains sensitive data — set x-acknowledge-sensitive: true to export",
        "sensitivity_summary": reasons,
    })
}

// -- New Phase 4 endpoints ------------------------------------------------

pub async fn get_sections(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let sections = state.sections_cache.get_or_init(|| {
        let session = state.session.lock().unwrap();
        normalize_for_context(session.snapshot())
    });
    Json(sections.clone())
}

pub async fn mark_viewed(
    State(state): State<Arc<AppState>>,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, AppError> {
    let req: ViewedRequest = serde_json::from_slice(&body).map_err(|e| {
        AppError(inspectah_refine::types::RefineError::BadRequest(format!(
            "invalid viewed request: {e}"
        )))
    })?;
    let mut session = state.session.lock().unwrap();
    session.mark_viewed(&req.id).map_err(AppError)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_viewed(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let session = state.session.lock().unwrap();
    let ids: Vec<&str> = session.viewed_ids().iter().map(|s| s.as_str()).collect();
    Json(json!({ "ids": ids }))
}

// -- normalize_for_context ------------------------------------------------

/// Map an `InspectionSnapshot` to presentation-layer `ContextSection`s.
/// Produces 9 sections matching the spec. Sections that are `None` in the
/// snapshot produce a `ContextSection` with an empty `items` vec.
///
/// Users & Groups data is no longer included here — it flows through
/// `ViewResponse.users_groups_decisions` from the projected snapshot.
pub fn normalize_for_context(snap: &InspectionSnapshot) -> Vec<ContextSection> {
    vec![
        normalize_services(snap),
        normalize_version_changes(snap),
        normalize_containers(snap),
        normalize_network(snap),
        normalize_storage(snap),
        normalize_scheduled_tasks(snap),
        normalize_non_rpm_software(snap),
        normalize_kernel_boot(snap),
        normalize_selinux(snap),
    ]
}

/// Format an epoch-version pair for display. Both sides of a version change
/// are formatted together so that epoch prefixes appear only when they carry
/// information (i.e. when at least one side has a non-zero epoch).
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

fn normalize_version_changes(snap: &InspectionSnapshot) -> ContextSection {
    // Three-state empty reason:
    // - "no_baseline"       — rpm section exists, but no baseline data
    // - "zero_drift"        — baseline exists, but no version changes detected
    // - "data_unavailable"  — no rpm section at all
    let rpm = match &snap.rpm {
        None => {
            return ContextSection {
                id: "version_changes".to_string(),
                display_name: "Version Changes".to_string(),
                items: Vec::new(),
                empty_reason: Some("data_unavailable".to_string()),
            };
        }
        Some(rpm) => rpm,
    };

    if rpm.version_changes.is_empty() {
        let reason = if snap.baseline.is_some() {
            "zero_drift"
        } else {
            "no_baseline"
        };
        return ContextSection {
            id: "version_changes".to_string(),
            display_name: "Version Changes".to_string(),
            items: Vec::new(),
            empty_reason: Some(reason.to_string()),
        };
    }

    // Partition into downgrades and upgrades; downgrades sort first.
    let mut downgrades: Vec<&VersionChange> = Vec::new();
    let mut upgrades: Vec<&VersionChange> = Vec::new();
    for vc in &rpm.version_changes {
        match vc.direction {
            VersionChangeDirection::Downgrade => downgrades.push(vc),
            VersionChangeDirection::Upgrade => upgrades.push(vc),
        }
    }

    let mut items = Vec::new();
    for vc in downgrades.iter().chain(upgrades.iter()) {
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
        let subtitle = format!("{} \u{2192} {} ({})", base_fmt, host_fmt, dir_label);
        let searchable = format!("{} {} {}", vc.name, vc.arch, dir_label);

        items.push(ContextItem {
            id: format!("{}.{}", vc.name, vc.arch),
            title,
            subtitle: Some(subtitle),
            detail: None,
            searchable_text: searchable,
        });
    }

    ContextSection {
        id: "version_changes".to_string(),
        display_name: "Version Changes".to_string(),
        items,
        empty_reason: None,
    }
}

fn normalize_services(snap: &InspectionSnapshot) -> ContextSection {
    let mut items = Vec::new();

    if let Some(svc) = &snap.services {
        let matched_set: std::collections::HashSet<&str> = svc
            .preset_matched_units
            .iter()
            .map(|s| s.as_str())
            .collect();
        let divergent_set: std::collections::HashSet<&str> = svc
            .state_changes
            .iter()
            .map(|sc| sc.unit.as_str())
            .collect();

        // Build drop-in lookup: units that are divergent or matched get
        // their drop-ins folded in; everything else is standalone.
        let enabled_set: std::collections::HashSet<&str> =
            svc.enabled_units.iter().map(|s| s.as_str()).collect();
        let disabled_set: std::collections::HashSet<&str> =
            svc.disabled_units.iter().map(|s| s.as_str()).collect();

        let mut dropin_by_unit: std::collections::HashMap<&str, Vec<&str>> =
            std::collections::HashMap::new();
        let mut standalone_dropins = Vec::new();
        for d in &svc.drop_ins {
            if divergent_set.contains(d.unit.as_str())
                || matched_set.contains(d.unit.as_str())
                || enabled_set.contains(d.unit.as_str())
                || disabled_set.contains(d.unit.as_str())
            {
                dropin_by_unit
                    .entry(d.unit.as_str())
                    .or_default()
                    .push(&d.content);
            } else {
                standalone_dropins.push(d);
            }
        }

        // 1. Divergence items (from state_changes)
        for sc in &svc.state_changes {
            let dropin_detail = dropin_by_unit
                .get(sc.unit.as_str())
                .map(|contents| contents.join("\n---\n"));
            let mut search = format!(
                "{} {} {} {}",
                sc.unit, sc.current_state, sc.default_state, sc.action
            );
            if let Some(pkg) = &sc.owning_package {
                search.push(' ');
                search.push_str(pkg);
            }
            items.push(ContextItem {
                id: sc.unit.clone(),
                title: sc.unit.clone(),
                subtitle: Some(format!(
                    "{} (diverges from preset: {})",
                    sc.current_state, sc.default_state
                )),
                detail: dropin_detail,
                searchable_text: search,
            });
        }

        // 2. Preset-matched with drop-in (visible with annotation)
        //    Preset-matched without drop-in are suppressed entirely.
        for unit_name in &svc.preset_matched_units {
            if let Some(dropin_contents) = dropin_by_unit.get(unit_name.as_str()) {
                let state = if enabled_set.contains(unit_name.as_str()) {
                    "enabled"
                } else {
                    "disabled"
                };
                items.push(ContextItem {
                    id: unit_name.clone(),
                    title: unit_name.clone(),
                    subtitle: Some(format!("{} (matches preset, has drop-in override)", state)),
                    detail: Some(dropin_contents.join("\n---\n")),
                    searchable_text: format!("{} {} drop-in override", unit_name, state),
                });
            }
        }

        // 3. Preset-unknown enabled units (not divergent, not matched)
        for unit_name in &svc.enabled_units {
            if !divergent_set.contains(unit_name.as_str())
                && !matched_set.contains(unit_name.as_str())
            {
                let dropin_detail = dropin_by_unit
                    .get(unit_name.as_str())
                    .map(|contents| contents.join("\n---\n"));
                items.push(ContextItem {
                    id: unit_name.clone(),
                    title: unit_name.clone(),
                    subtitle: Some("enabled (no preset rule)".into()),
                    detail: dropin_detail,
                    searchable_text: format!("{} enabled no preset rule", unit_name),
                });
            }
        }

        // 4. Preset-unknown disabled units (not divergent, not matched)
        for unit_name in &svc.disabled_units {
            if !divergent_set.contains(unit_name.as_str())
                && !matched_set.contains(unit_name.as_str())
            {
                items.push(ContextItem {
                    id: unit_name.clone(),
                    title: unit_name.clone(),
                    subtitle: Some("disabled (no preset rule)".into()),
                    detail: None,
                    searchable_text: format!("{} disabled no preset rule", unit_name),
                });
            }
        }

        // 5. Standalone drop-ins (unit not covered by any of the above)
        for d in &standalone_dropins {
            items.push(ContextItem {
                id: format!("dropin-{}", d.unit),
                title: format!("{} (drop-in)", d.unit),
                subtitle: Some("drop-in override".into()),
                detail: Some(d.content.clone()),
                searchable_text: format!("{} drop-in", d.unit),
            });
        }
    }

    ContextSection {
        id: "services".to_string(),
        display_name: "Services".to_string(),
        items,
        empty_reason: None,
    }
}

fn normalize_containers(snap: &InspectionSnapshot) -> ContextSection {
    let mut items = Vec::new();

    if let Some(ctr) = &snap.containers {
        // QuadletUnit
        for q in &ctr.quadlet_units {
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
        for cf in &ctr.compose_files {
            let basename = Path::new(&cf.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| cf.path.clone());
            let service_names: Vec<&str> = cf.images.iter().map(|s| s.service.as_str()).collect();
            let subtitle = service_names.join(", ");
            let mut search = format!("{} {}", cf.path, service_names.join(" "));
            // Append image refs for searchability (spec: ComposeService.image → searchable_text)
            for svc in &cf.images {
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
        for rc in &ctr.running_containers {
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
        for fa in &ctr.flatpak_apps {
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
    }

    ContextSection {
        id: "containers".to_string(),
        display_name: "Containers".to_string(),
        items,
        empty_reason: None,
    }
}

fn normalize_network(snap: &InspectionSnapshot) -> ContextSection {
    let mut items = Vec::new();

    if let Some(net) = &snap.network {
        // NMConnection
        for conn in &net.connections {
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
        for zone in &net.firewall_zones {
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
        for rule in &net.firewall_direct_rules {
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
        for sr in &net.static_routes {
            items.push(ContextItem {
                id: sr.path.clone(),
                title: sr.name.clone(),
                subtitle: Some(sr.path.clone()),
                detail: None,
                searchable_text: format!("{} {}", sr.path, sr.name),
            });
        }

        // ip_routes
        for route in &net.ip_routes {
            items.push(ContextItem {
                id: route.clone(),
                title: route.clone(),
                subtitle: Some("ip route".to_string()),
                detail: None,
                searchable_text: route.clone(),
            });
        }

        // ip_rules
        for rule in &net.ip_rules {
            items.push(ContextItem {
                id: rule.clone(),
                title: rule.clone(),
                subtitle: Some("ip rule".to_string()),
                detail: None,
                searchable_text: rule.clone(),
            });
        }

        // resolv_provenance
        if !net.resolv_provenance.is_empty() {
            items.push(ContextItem {
                id: "resolv_provenance".to_string(),
                title: "DNS resolver".to_string(),
                subtitle: Some(net.resolv_provenance.clone()),
                detail: None,
                searchable_text: net.resolv_provenance.clone(),
            });
        }

        // hosts_additions
        for line in &net.hosts_additions {
            items.push(ContextItem {
                id: line.clone(),
                title: line.clone(),
                subtitle: Some("hosts".to_string()),
                detail: None,
                searchable_text: line.clone(),
            });
        }

        // ProxyEntry
        for proxy in &net.proxy {
            let id = format!("{}:{}", proxy.source, proxy.line);
            items.push(ContextItem {
                id,
                title: proxy.source.clone(),
                subtitle: Some(proxy.line.clone()),
                detail: None,
                searchable_text: format!("{} {}", proxy.source, proxy.line),
            });
        }
    }

    ContextSection {
        id: "network".to_string(),
        display_name: "Network".to_string(),
        items,
        empty_reason: None,
    }
}

fn normalize_storage(snap: &InspectionSnapshot) -> ContextSection {
    let mut items = Vec::new();

    if let Some(stor) = &snap.storage {
        // FstabEntry
        for entry in &stor.fstab_entries {
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
        for mp in &stor.mount_points {
            items.push(ContextItem {
                id: mp.target.clone(),
                title: mp.target.clone(),
                subtitle: Some(format!("{} ({})", mp.source, mp.fstype)),
                detail: Some(mp.options.clone()),
                searchable_text: format!("{} {} {}", mp.target, mp.source, mp.fstype),
            });
        }

        // LvmVolume
        for lv in &stor.lvm_info {
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
        for vd in &stor.var_directories {
            items.push(ContextItem {
                id: vd.path.clone(),
                title: vd.path.clone(),
                subtitle: Some(format!("~{}", vd.size_estimate)),
                detail: Some(vd.recommendation.clone()),
                searchable_text: format!("{} {} {}", vd.path, vd.size_estimate, vd.recommendation),
            });
        }

        // CredentialRef
        for cr in &stor.credential_refs {
            items.push(ContextItem {
                id: cr.credential_path.clone(),
                title: cr.credential_path.clone(),
                subtitle: Some(format!("mount: {}", cr.mount_point)),
                detail: Some(cr.source.clone()),
                searchable_text: format!("{} {} {}", cr.credential_path, cr.mount_point, cr.source),
            });
        }
    }

    ContextSection {
        id: "storage".to_string(),
        display_name: "Storage".to_string(),
        items,
        empty_reason: None,
    }
}

fn normalize_scheduled_tasks(snap: &InspectionSnapshot) -> ContextSection {
    let mut items = Vec::new();

    if let Some(sched) = &snap.scheduled_tasks {
        // CronJob
        for cj in &sched.cron_jobs {
            let basename = Path::new(&cj.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| cj.path.clone());
            items.push(ContextItem {
                id: cj.path.clone(),
                title: basename,
                subtitle: Some(cj.source.clone()),
                detail: Some(cj.source.clone()),
                searchable_text: format!("{} {}", cj.path, cj.source),
            });
        }

        // SystemdTimer
        for st in &sched.systemd_timers {
            let mut detail_parts = Vec::new();
            if !st.description.is_empty() {
                detail_parts.push(st.description.clone());
            }
            if !st.exec_start.is_empty() {
                detail_parts.push(st.exec_start.clone());
            }
            let detail = if detail_parts.is_empty() {
                None
            } else {
                Some(detail_parts.join("\n"))
            };
            items.push(ContextItem {
                id: st.name.clone(),
                title: st.name.clone(),
                subtitle: Some(st.on_calendar.clone()),
                detail,
                searchable_text: format!(
                    "{} {} {} {}",
                    st.name, st.on_calendar, st.exec_start, st.description
                ),
            });
        }

        // AtJob
        for aj in &sched.at_jobs {
            items.push(ContextItem {
                id: aj.file.clone(),
                title: aj.file.clone(),
                subtitle: Some(format!("{}: {}", aj.user, aj.command)),
                detail: Some(aj.working_dir.clone()),
                searchable_text: format!("{} {} {}", aj.file, aj.command, aj.user),
            });
        }

        // GeneratedTimerUnit
        for gtu in &sched.generated_timer_units {
            let mut detail_parts = Vec::new();
            if !gtu.source_path.is_empty() {
                detail_parts.push(gtu.source_path.clone());
            }
            if !gtu.command.is_empty() {
                detail_parts.push(gtu.command.clone());
            }
            let detail = if detail_parts.is_empty() {
                None
            } else {
                Some(detail_parts.join("\n"))
            };
            items.push(ContextItem {
                id: gtu.name.clone(),
                title: gtu.name.clone(),
                subtitle: Some(gtu.cron_expr.clone()),
                detail,
                searchable_text: format!(
                    "{} {} {} {}",
                    gtu.name, gtu.cron_expr, gtu.source_path, gtu.command
                ),
            });
        }
    }

    ContextSection {
        id: "scheduled_tasks".to_string(),
        display_name: "Scheduled Tasks".to_string(),
        items,
        empty_reason: None,
    }
}

fn normalize_non_rpm_software(snap: &InspectionSnapshot) -> ContextSection {
    let mut items = Vec::new();

    if let Some(nrpm) = &snap.non_rpm_software {
        // NonRpmItem
        for item in &nrpm.items {
            let subtitle = format!("{} ({})", item.method, item.confidence);
            // detail always includes path; pip packages appended when present
            let detail = if !item.packages.is_empty() {
                let pkg_list: Vec<String> = item
                    .packages
                    .iter()
                    .map(|p| {
                        if p.version.is_empty() {
                            p.name.clone()
                        } else {
                            format!("{}=={}", p.name, p.version)
                        }
                    })
                    .collect();
                Some(format!("{}\n{}", item.path, pkg_list.join(", ")))
            } else {
                Some(item.path.clone())
            };

            let mut search = format!("{} {} {} {}", item.name, item.path, item.method, item.lang);
            if !item.version.is_empty() {
                search.push(' ');
                search.push_str(&item.version);
            }

            items.push(ContextItem {
                id: item.name.clone(),
                title: item.name.clone(),
                subtitle: Some(subtitle),
                detail,
                searchable_text: search,
            });
        }

        // ConfigFileEntry (env_files)
        for ef in &nrpm.env_files {
            let basename = Path::new(&ef.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| ef.path.clone());
            let kind_str = match ef.kind {
                ConfigFileKind::RpmOwnedDefault => "rpm-default",
                ConfigFileKind::RpmOwnedModified => "rpm-modified",
                ConfigFileKind::Unowned => "unowned",
                ConfigFileKind::Orphaned => "orphaned",
                ConfigFileKind::BaselineMatch => "baseline-match",
            };
            items.push(ContextItem {
                id: ef.path.clone(),
                title: basename,
                subtitle: Some(kind_str.to_string()),
                detail: Some(ef.content.clone()),
                searchable_text: format!("{} {}", ef.path, ef.content),
            });
        }
    }

    ContextSection {
        id: "non_rpm_software".to_string(),
        display_name: "Non-RPM Software".to_string(),
        items,
        empty_reason: None,
    }
}

fn normalize_kernel_boot(snap: &InspectionSnapshot) -> ContextSection {
    let mut items = Vec::new();

    if let Some(kb) = &snap.kernel_boot {
        // cmdline — single item
        if !kb.cmdline.is_empty() {
            let subtitle = if kb.cmdline.len() > 80 {
                Some(format!("{}...", &kb.cmdline[..77]))
            } else {
                Some(kb.cmdline.clone())
            };
            items.push(ContextItem {
                id: "cmdline".to_string(),
                title: "Kernel cmdline".to_string(),
                subtitle,
                detail: Some(kb.cmdline.clone()),
                searchable_text: kb.cmdline.clone(),
            });
        }

        // grub_defaults — single item
        if !kb.grub_defaults.is_empty() {
            items.push(ContextItem {
                id: "grub_defaults".to_string(),
                title: "GRUB defaults".to_string(),
                subtitle: None,
                detail: Some(kb.grub_defaults.clone()),
                searchable_text: kb.grub_defaults.clone(),
            });
        }

        // tuned_active — single item
        if !kb.tuned_active.is_empty() {
            items.push(ContextItem {
                id: "tuned_active".to_string(),
                title: "Active tuned profile".to_string(),
                subtitle: Some(kb.tuned_active.clone()),
                detail: None,
                searchable_text: kb.tuned_active.clone(),
            });
        }

        // locale — single item (optional)
        if let Some(locale) = &kb.locale {
            items.push(ContextItem {
                id: "locale".to_string(),
                title: "Locale".to_string(),
                subtitle: Some(locale.clone()),
                detail: None,
                searchable_text: locale.clone(),
            });
        }

        // timezone — single item (optional)
        if let Some(tz) = &kb.timezone {
            items.push(ContextItem {
                id: "timezone".to_string(),
                title: "Timezone".to_string(),
                subtitle: Some(tz.clone()),
                detail: None,
                searchable_text: tz.clone(),
            });
        }

        // SysctlOverride
        for so in &kb.sysctl_overrides {
            items.push(ContextItem {
                id: so.key.clone(),
                title: so.key.clone(),
                subtitle: Some(format!("\"{}\" (default: \"{}\")", so.runtime, so.default)),
                detail: Some(so.source.clone()),
                searchable_text: format!("{} {} {} {}", so.key, so.runtime, so.default, so.source),
            });
        }

        // KernelModule (non_default_modules only)
        for km in &kb.non_default_modules {
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

        // ConfigSnippet — modules_load_d
        for cs in &kb.modules_load_d {
            let basename = Path::new(&cs.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| cs.path.clone());
            items.push(ContextItem {
                id: cs.path.clone(),
                title: basename,
                subtitle: Some("modules-load.d".to_string()),
                detail: Some(cs.content.clone()),
                searchable_text: format!("{} {}", cs.path, cs.content),
            });
        }

        // ConfigSnippet — modprobe_d
        for cs in &kb.modprobe_d {
            let basename = Path::new(&cs.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| cs.path.clone());
            items.push(ContextItem {
                id: cs.path.clone(),
                title: basename,
                subtitle: Some("modprobe.d".to_string()),
                detail: Some(cs.content.clone()),
                searchable_text: format!("{} {}", cs.path, cs.content),
            });
        }

        // ConfigSnippet — dracut_conf
        for cs in &kb.dracut_conf {
            let basename = Path::new(&cs.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| cs.path.clone());
            items.push(ContextItem {
                id: cs.path.clone(),
                title: basename,
                subtitle: Some("dracut.conf.d".to_string()),
                detail: Some(cs.content.clone()),
                searchable_text: format!("{} {}", cs.path, cs.content),
            });
        }

        // ConfigSnippet — tuned_custom_profiles
        for cs in &kb.tuned_custom_profiles {
            let basename = Path::new(&cs.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| cs.path.clone());
            items.push(ContextItem {
                id: cs.path.clone(),
                title: basename,
                subtitle: Some("tuned profile".to_string()),
                detail: Some(cs.content.clone()),
                searchable_text: format!("{} {}", cs.path, cs.content),
            });
        }

        // AlternativeEntry
        for ae in &kb.alternatives {
            items.push(ContextItem {
                id: ae.name.clone(),
                title: ae.name.clone(),
                subtitle: Some(format!("{} ({})", ae.path, ae.status)),
                detail: None,
                searchable_text: format!("{} {} {}", ae.name, ae.path, ae.status),
            });
        }
    }

    ContextSection {
        id: "kernel_boot".to_string(),
        display_name: "Kernel & Boot".to_string(),
        items,
        empty_reason: None,
    }
}

fn normalize_selinux(snap: &InspectionSnapshot) -> ContextSection {
    let mut items = Vec::new();

    if let Some(se) = &snap.selinux {
        // Mode — single synthetic item
        if !se.mode.is_empty() {
            items.push(ContextItem {
                id: "selinux_mode".to_string(),
                title: "SELinux mode".to_string(),
                subtitle: Some(se.mode.clone()),
                detail: None,
                searchable_text: format!("selinux mode {}", se.mode),
            });
        }

        // FIPS mode — always emitted (spec: show even when disabled)
        {
            let fips_label = if se.fips_mode { "enabled" } else { "disabled" };
            items.push(ContextItem {
                id: "fips_mode".to_string(),
                title: "FIPS mode".to_string(),
                subtitle: Some(fips_label.to_string()),
                detail: None,
                searchable_text: format!("fips mode {}", fips_label),
            });
        }

        // SelinuxPortLabel
        for pl in &se.port_labels {
            let id = format!("{}/{}", pl.protocol, pl.port);
            items.push(ContextItem {
                id: id.clone(),
                title: id,
                subtitle: Some(pl.label_type.clone()),
                detail: None,
                searchable_text: format!("{} {} {}", pl.protocol, pl.port, pl.label_type),
            });
        }

        // boolean_overrides (serde_json::Value)
        for bo in &se.boolean_overrides {
            let name = bo
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let value = bo
                .get("value")
                .or_else(|| bo.get("state"))
                .map(|v| v.to_string())
                .unwrap_or_default();

            items.push(ContextItem {
                id: name.clone(),
                title: name.clone(),
                subtitle: Some(value),
                detail: None,
                searchable_text: name.clone(),
            });
        }

        // custom_modules
        for module in &se.custom_modules {
            items.push(ContextItem {
                id: module.clone(),
                title: module.clone(),
                subtitle: Some("custom module".to_string()),
                detail: None,
                searchable_text: module.clone(),
            });
        }

        // fcontext_rules
        for rule in &se.fcontext_rules {
            items.push(ContextItem {
                id: rule.clone(),
                title: rule.clone(),
                subtitle: Some("fcontext".to_string()),
                detail: None,
                searchable_text: rule.clone(),
            });
        }

        // CarryForwardFile — audit_rules
        for cf in &se.audit_rules {
            let basename = Path::new(&cf.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| cf.path.clone());
            items.push(ContextItem {
                id: cf.path.clone(),
                title: basename,
                subtitle: Some("audit rule".to_string()),
                detail: Some(cf.content.clone()),
                searchable_text: format!("{} {}", cf.path, cf.content),
            });
        }

        // CarryForwardFile — pam_configs
        for cf in &se.pam_configs {
            let basename = Path::new(&cf.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| cf.path.clone());
            items.push(ContextItem {
                id: cf.path.clone(),
                title: basename,
                subtitle: Some("PAM config".to_string()),
                detail: Some(cf.content.clone()),
                searchable_text: format!("{} {}", cf.path, cf.content),
            });
        }
    }

    ContextSection {
        id: "selinux".to_string(),
        display_name: "Security & Access Control".to_string(),
        items,
        empty_reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::baseline::BaselineData;
    use inspectah_core::types::completeness::{Completeness, InspectorId};
    use inspectah_core::types::containers::{
        ComposeFile, ComposeService, ContainerSection, RunningContainer,
    };
    use inspectah_core::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection, PipPackage};
    use inspectah_core::types::rpm::{
        PackageEntry, PackageState, RepoFile, RpmSection, VersionChange, VersionChangeDirection,
    };
    use inspectah_core::types::selinux::SelinuxSection;
    use inspectah_core::types::services::{ServiceSection, ServiceStateChange, SystemdDropIn};

    fn empty_snapshot() -> InspectionSnapshot {
        InspectionSnapshot::new()
    }

    // -- Fix 1: Health completeness wire shape ---------------------------------

    #[test]
    fn health_completeness_complete_is_string() {
        let mut snap = empty_snapshot();
        snap.completeness = Completeness::Complete;
        let val = serde_json::to_value(&snap.completeness).unwrap();
        // The raw serde produces an object, NOT what the wire contract wants.
        // Verify our match-based conversion produces a flat string.
        let wire = match &snap.completeness {
            Completeness::Complete => "complete",
            Completeness::Partial { .. } => "partial",
            Completeness::Incomplete { .. } => "incomplete",
        };
        assert_eq!(wire, "complete");
        // Confirm serde would NOT produce the right shape on its own
        assert!(val.is_object(), "raw serde produces object, not string");
    }

    #[test]
    fn health_completeness_partial_is_string() {
        let snap_completeness = Completeness::Partial {
            degraded_sections: vec![InspectorId::Rpm],
            reason: "timeout".into(),
        };
        let wire = match &snap_completeness {
            Completeness::Complete => "complete",
            Completeness::Partial { .. } => "partial",
            Completeness::Incomplete { .. } => "incomplete",
        };
        assert_eq!(wire, "partial");
    }

    #[test]
    fn health_completeness_incomplete_is_string() {
        let snap_completeness = Completeness::Incomplete {
            failed_sections: vec![InspectorId::Network],
            degraded_sections: vec![],
            reason: "crash".into(),
        };
        let wire = match &snap_completeness {
            Completeness::Complete => "complete",
            Completeness::Partial { .. } => "partial",
            Completeness::Incomplete { .. } => "incomplete",
        };
        assert_eq!(wire, "incomplete");
    }

    // -- Fix 2a: ComposeFile image refs in searchable_text --------------------

    #[test]
    fn compose_searchable_text_includes_image_refs() {
        let mut snap = empty_snapshot();
        snap.containers = Some(ContainerSection {
            compose_files: vec![ComposeFile {
                path: "/opt/app/docker-compose.yml".into(),
                images: vec![
                    ComposeService {
                        service: "web".into(),
                        image: "nginx:1.25".into(),
                    },
                    ComposeService {
                        service: "db".into(),
                        image: "postgres:16".into(),
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        });

        let sections = normalize_for_context(&snap);
        let containers = sections.iter().find(|s| s.id == "containers").unwrap();
        let item = containers
            .items
            .iter()
            .find(|i| i.id == "/opt/app/docker-compose.yml")
            .unwrap();

        assert!(
            item.searchable_text.contains("nginx:1.25"),
            "searchable_text should contain image ref nginx:1.25, got: {}",
            item.searchable_text
        );
        assert!(
            item.searchable_text.contains("postgres:16"),
            "searchable_text should contain image ref postgres:16, got: {}",
            item.searchable_text
        );
        // Also verify service names are still present
        assert!(item.searchable_text.contains("web"));
        assert!(item.searchable_text.contains("db"));
    }

    // -- Fix 2b: RunningContainer restart_policy in searchable_text -----------

    #[test]
    fn container_searchable_text_includes_restart_policy() {
        let mut snap = empty_snapshot();
        snap.containers = Some(ContainerSection {
            running_containers: vec![RunningContainer {
                id: "abc123".into(),
                name: "my-app".into(),
                image: "myapp:latest".into(),
                status: "running".into(),
                restart_policy: "always".into(),
                ..Default::default()
            }],
            ..Default::default()
        });

        let sections = normalize_for_context(&snap);
        let containers = sections.iter().find(|s| s.id == "containers").unwrap();
        let item = containers.items.iter().find(|i| i.id == "abc123").unwrap();

        assert!(
            item.searchable_text.contains("always"),
            "searchable_text should contain restart_policy, got: {}",
            item.searchable_text
        );
    }

    #[test]
    fn container_searchable_text_omits_empty_restart_policy() {
        let mut snap = empty_snapshot();
        snap.containers = Some(ContainerSection {
            running_containers: vec![RunningContainer {
                id: "abc123".into(),
                name: "my-app".into(),
                image: "myapp:latest".into(),
                status: "running".into(),
                restart_policy: String::new(),
                ..Default::default()
            }],
            ..Default::default()
        });

        let sections = normalize_for_context(&snap);
        let containers = sections.iter().find(|s| s.id == "containers").unwrap();
        let item = containers.items.iter().find(|i| i.id == "abc123").unwrap();

        // Should end cleanly without trailing space
        assert_eq!(item.searchable_text, "my-app myapp:latest running");
    }

    // -- Fix 2c: NonRpmItem detail includes both path and pip packages --------

    #[test]
    fn nonrpm_detail_includes_path_and_packages() {
        let mut snap = empty_snapshot();
        snap.non_rpm_software = Some(NonRpmSoftwareSection {
            items: vec![NonRpmItem {
                name: "myenv".into(),
                path: "/opt/venvs/myenv".into(),
                method: "pip".into(),
                confidence: "high".into(),
                packages: vec![
                    PipPackage {
                        name: "requests".into(),
                        version: "2.31".into(),
                    },
                    PipPackage {
                        name: "flask".into(),
                        version: "".into(),
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        });

        let sections = normalize_for_context(&snap);
        let nonrpm = sections
            .iter()
            .find(|s| s.id == "non_rpm_software")
            .unwrap();
        let item = nonrpm.items.iter().find(|i| i.id == "myenv").unwrap();
        let detail = item.detail.as_ref().unwrap();

        assert!(
            detail.contains("/opt/venvs/myenv"),
            "detail should contain path, got: {}",
            detail
        );
        assert!(
            detail.contains("requests==2.31"),
            "detail should contain pip packages, got: {}",
            detail
        );
        assert!(
            detail.contains("flask"),
            "detail should contain flask, got: {}",
            detail
        );
    }

    #[test]
    fn nonrpm_detail_path_only_when_no_packages() {
        let mut snap = empty_snapshot();
        snap.non_rpm_software = Some(NonRpmSoftwareSection {
            items: vec![NonRpmItem {
                name: "mybin".into(),
                path: "/opt/bin/mybin".into(),
                method: "binary".into(),
                confidence: "medium".into(),
                ..Default::default()
            }],
            ..Default::default()
        });

        let sections = normalize_for_context(&snap);
        let nonrpm = sections
            .iter()
            .find(|s| s.id == "non_rpm_software")
            .unwrap();
        let item = nonrpm.items.iter().find(|i| i.id == "mybin").unwrap();

        assert_eq!(item.detail.as_deref(), Some("/opt/bin/mybin"));
    }

    // -- Fix 2d: FIPS mode synthetic row emitted when disabled ----------------

    #[test]
    fn fips_mode_disabled_emits_row() {
        let mut snap = empty_snapshot();
        snap.selinux = Some(SelinuxSection {
            mode: "enforcing".into(),
            fips_mode: false,
            ..Default::default()
        });

        let sections = normalize_for_context(&snap);
        let selinux = sections.iter().find(|s| s.id == "selinux").unwrap();
        let fips = selinux
            .items
            .iter()
            .find(|i| i.id == "fips_mode")
            .expect("FIPS mode item should exist even when disabled");

        assert_eq!(fips.subtitle.as_deref(), Some("disabled"));
        assert!(fips.searchable_text.contains("disabled"));
    }

    #[test]
    fn fips_mode_enabled_emits_row() {
        let mut snap = empty_snapshot();
        snap.selinux = Some(SelinuxSection {
            mode: "enforcing".into(),
            fips_mode: true,
            ..Default::default()
        });

        let sections = normalize_for_context(&snap);
        let selinux = sections.iter().find(|s| s.id == "selinux").unwrap();
        let fips = selinux
            .items
            .iter()
            .find(|i| i.id == "fips_mode")
            .expect("FIPS mode item should exist when enabled");

        assert_eq!(fips.subtitle.as_deref(), Some("enabled"));
        assert!(fips.searchable_text.contains("enabled"));
    }

    // -- Health endpoint includes policy.distro_repos -------------------------

    #[test]
    fn health_includes_policy_distro_repos() {
        let snap = empty_snapshot();
        let session = RefineSession::new(snap);
        let state = Arc::new(AppState {
            session: Arc::new(Mutex::new(session)),
            sections_cache: OnceLock::new(),
        });

        // Call the health handler synchronously via tokio runtime
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            let Json(val) = health(State(state)).await;
            val
        });

        let policy = result.get("policy").expect("response should have policy");
        let distro_repos = policy
            .get("distro_repos")
            .expect("policy should have distro_repos")
            .as_array()
            .expect("distro_repos should be an array");

        assert!(!distro_repos.is_empty(), "distro_repos should not be empty");
        let repo_strs: Vec<&str> = distro_repos.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(repo_strs.contains(&"baseos"));
        assert!(repo_strs.contains(&"appstream"));
    }

    // -- View response includes repo_groups -----------------------------------

    #[test]
    fn view_response_includes_repo_groups() {
        let mut snap = empty_snapshot();
        snap.rpm = Some(RpmSection {
            packages_added: vec![
                PackageEntry {
                    name: "httpd".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    include: true,
                    source_repo: "appstream".into(),
                    ..Default::default()
                },
                PackageEntry {
                    name: "kernel".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    include: true,
                    source_repo: "baseos".into(),
                    ..Default::default()
                },
                PackageEntry {
                    name: "custom-tool".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    include: true,
                    source_repo: "epel".into(),
                    ..Default::default()
                },
            ],
            repo_files: vec![RepoFile {
                path: "/etc/yum.repos.d/redhat.repo".into(),
                content:
                    "[appstream]\nname=AppStream\ngpgcheck=1\n[baseos]\nname=BaseOS\ngpgcheck=1"
                        .into(),
                ..Default::default()
            }],
            ..Default::default()
        });

        let session = RefineSession::new(snap);
        let groups = build_repo_groups(&session);

        // Should have entries for appstream, baseos, and epel
        assert!(
            groups.len() >= 3,
            "expected at least 3 repo groups, got {}",
            groups.len()
        );

        let appstream = groups
            .iter()
            .find(|g| g.section_id == "appstream")
            .expect("should have appstream group");
        assert!(appstream.is_distro);
        assert_eq!(appstream.provenance, RepoProvenance::Verified);
        assert!(appstream.enabled);

        let baseos = groups
            .iter()
            .find(|g| g.section_id == "baseos")
            .expect("should have baseos group");
        assert!(baseos.is_distro);

        let epel = groups
            .iter()
            .find(|g| g.section_id == "epel")
            .expect("should have epel group");
        assert!(!epel.is_distro);
        // epel has packages but no repo file -> Incomplete
        assert_eq!(epel.provenance, RepoProvenance::Incomplete);
        assert!(epel.enabled);
    }

    #[test]
    fn view_response_repo_groups_distro_sorted_first() {
        let mut snap = empty_snapshot();
        snap.rpm = Some(RpmSection {
            packages_added: vec![
                PackageEntry {
                    name: "httpd".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    include: true,
                    source_repo: "appstream".into(),
                    ..Default::default()
                },
                PackageEntry {
                    name: "zsh".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    include: true,
                    source_repo: "epel".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        });

        let session = RefineSession::new(snap);
        let groups = build_repo_groups(&session);

        // Distro repos should come before non-distro
        let first_non_distro = groups.iter().position(|g| !g.is_distro);
        let last_distro = groups.iter().rposition(|g| g.is_distro);
        if let (Some(fnd), Some(ld)) = (first_non_distro, last_distro) {
            assert!(
                ld < fnd,
                "distro repos should be sorted before non-distro repos"
            );
        }
    }

    #[test]
    fn apply_exclude_repo_via_op_endpoint() {
        use inspectah_core::types::rpm::RepoFile;
        use inspectah_refine::types::RefinementOp;

        // Build snapshot with packages from multiple repos including epel
        let mut snap = empty_snapshot();
        snap.rpm = Some(RpmSection {
            packages_added: vec![
                PackageEntry {
                    name: "httpd".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    source_repo: "appstream".into(),
                    include: true,
                    ..Default::default()
                },
                PackageEntry {
                    name: "epel-release".into(),
                    arch: "noarch".into(),
                    state: PackageState::Added,
                    source_repo: "epel".into(),
                    include: true,
                    ..Default::default()
                },
            ],
            repo_files: vec![
                RepoFile {
                    path: "/etc/yum.repos.d/centos.repo".into(),
                    content: "[baseos]\nname=CentOS BaseOS\n\n[appstream]\nname=CentOS AppStream\n"
                        .into(),
                    include: true,
                    ..Default::default()
                },
                RepoFile {
                    path: "/etc/yum.repos.d/epel.repo".into(),
                    content: "[epel]\nname=EPEL 9\n".into(),
                    include: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });

        let mut session = RefineSession::new(snap);

        // Apply ExcludeRepo via the op API
        let op = RefinementOp::ExcludeRepo {
            section_id: "epel".into(),
        };
        session.apply(op).expect("ExcludeRepo should succeed");

        // Verify epel packages are now excluded
        let view = session.view();
        let epel_pkg = view
            .packages
            .iter()
            .find(|p| p.entry.name == "epel-release");
        assert!(epel_pkg.is_some(), "epel-release should still be in view");
        assert!(
            !epel_pkg.unwrap().entry.include,
            "epel-release should be excluded"
        );

        // Verify repo groups reflect the exclusion
        let groups = build_repo_groups(&session);
        let epel_group = groups.iter().find(|g| g.section_id == "epel");
        assert!(epel_group.is_some(), "epel group should exist");
        assert!(
            !epel_group.unwrap().enabled,
            "epel group should be disabled"
        );
    }

    // -- Cross-section isolation: items must not leak between sections -----

    #[test]
    fn cross_section_no_contamination() {
        use inspectah_core::types::services::{ServiceSection, ServiceStateChange};
        use inspectah_core::types::storage::{MountPoint, StorageSection};

        let mut snap = empty_snapshot();

        snap.services = Some(ServiceSection {
            state_changes: vec![ServiceStateChange {
                unit: "NetworkManager-wait-online.service".into(),
                current_state: "enabled".into(),
                default_state: "enable".into(),
                action: "enable".into(),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        });

        snap.storage = Some(StorageSection {
            mount_points: vec![MountPoint {
                target: "/".into(),
                source: "/dev/mapper/cs-root".into(),
                fstype: "xfs".into(),
                options: "rw".into(),
            }],
            ..Default::default()
        });

        // Users & groups no longer in sections — served via ViewResponse.
        let sections = normalize_for_context(&snap);

        for section in &sections {
            match section.id.as_str() {
                "services" => {
                    assert!(
                        section
                            .items
                            .iter()
                            .any(|i| i.id.contains("NetworkManager"))
                    );
                    assert!(
                        !section.items.iter().any(|i| i.id == "/"),
                        "services has storage item leak"
                    );
                }
                "storage" => {
                    assert!(section.items.iter().any(|i| i.id == "/"));
                    assert!(
                        !section
                            .items
                            .iter()
                            .any(|i| i.id.contains("NetworkManager")),
                        "storage has service item leak"
                    );
                }
                _ => {
                    assert!(
                        section.items.is_empty(),
                        "{} should have no items but has: {:?}",
                        section.id,
                        section.items.iter().map(|i| &i.id).collect::<Vec<_>>()
                    );
                }
            }
        }
    }

    // -- Three-way services normalization ------------------------------------

    #[test]
    fn test_normalize_services_three_way_split() {
        let mut snap = empty_snapshot();
        snap.services = Some(ServiceSection {
            state_changes: vec![ServiceStateChange {
                unit: "httpd.service".into(),
                current_state: "enabled".into(),
                default_state: "disable".into(),
                action: "enable".into(),
                include: true,
                owning_package: None,
                fleet: None,
                attention_reason: None,
            }],
            enabled_units: vec![
                "httpd.service".into(),
                "chronyd.service".into(),
                "oddjobd.service".into(),
            ],
            disabled_units: vec!["cups.service".into()],
            drop_ins: Vec::new(),
            preset_matched_units: vec!["chronyd.service".into()],
        });

        let section = normalize_services(&snap);

        assert!(
            !section.items.iter().any(|i| i.id == "chronyd.service"),
            "preset-matched unit should be suppressed"
        );
        assert_eq!(
            section
                .items
                .iter()
                .filter(|i| i.id == "httpd.service")
                .count(),
            1
        );
        let oddjobd = section
            .items
            .iter()
            .find(|i| i.id == "oddjobd.service")
            .unwrap();
        assert!(
            oddjobd
                .subtitle
                .as_ref()
                .unwrap()
                .contains("no preset rule")
        );
        let cups = section
            .items
            .iter()
            .find(|i| i.id == "cups.service")
            .unwrap();
        assert!(cups.subtitle.as_ref().unwrap().contains("no preset rule"));
    }

    #[test]
    fn test_normalize_services_matched_with_dropin_visible() {
        let mut snap = empty_snapshot();
        snap.services = Some(ServiceSection {
            state_changes: Vec::new(),
            enabled_units: vec!["sshd.service".into()],
            disabled_units: Vec::new(),
            drop_ins: vec![SystemdDropIn {
                unit: "sshd.service".into(),
                path: "/etc/systemd/system/sshd.service.d/override.conf".into(),
                content: "[Service]\nTimeoutStartSec=90".into(),
                include: true,
                tie: false,
                tie_winner: false,
                fleet: None,
            }],
            preset_matched_units: vec!["sshd.service".into()],
        });

        let section = normalize_services(&snap);
        let sshd = section.items.iter().find(|i| i.id == "sshd.service");
        assert!(
            sshd.is_some(),
            "matched unit with drop-in should remain visible"
        );
        assert!(
            sshd.unwrap()
                .subtitle
                .as_ref()
                .unwrap()
                .contains("matches preset")
        );
        assert!(sshd.unwrap().subtitle.as_ref().unwrap().contains("drop-in"));
    }

    #[test]
    fn test_normalize_services_legacy_snapshot_no_preset_matched() {
        let mut snap = empty_snapshot();
        snap.services = Some(ServiceSection {
            state_changes: Vec::new(),
            enabled_units: vec!["chronyd.service".into()],
            disabled_units: Vec::new(),
            drop_ins: Vec::new(),
            preset_matched_units: Vec::new(),
        });

        let section = normalize_services(&snap);
        let chronyd = section
            .items
            .iter()
            .find(|i| i.id == "chronyd.service")
            .unwrap();
        assert!(
            chronyd
                .subtitle
                .as_ref()
                .unwrap()
                .contains("no preset rule")
        );
    }

    // -- normalize_version_changes tests --------------------------------------

    #[test]
    fn test_normalize_version_changes_downgrades_first() {
        let mut snap = empty_snapshot();
        let mut rpm = RpmSection::default();
        rpm.version_changes = vec![
            VersionChange {
                name: "openssl".into(),
                arch: "x86_64".into(),
                host_version: "3.1.2-1.el9".into(),
                base_version: "3.1.1-1.el9".into(),
                host_epoch: String::new(),
                base_epoch: String::new(),
                direction: VersionChangeDirection::Downgrade,
            },
            VersionChange {
                name: "curl".into(),
                arch: "x86_64".into(),
                host_version: "8.0.1-1.el9".into(),
                base_version: "8.1.0-1.el9".into(),
                host_epoch: String::new(),
                base_epoch: String::new(),
                direction: VersionChangeDirection::Upgrade,
            },
        ];
        snap.rpm = Some(rpm);
        snap.baseline = Some(BaselineData {
            image_digest: "sha256:test".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });

        let section = normalize_version_changes(&snap);
        assert_eq!(section.items.len(), 2);
        // Downgrade sorts first
        assert!(
            section.items[0].title.starts_with('\u{25BC}'),
            "first item should be downgrade with ▼ prefix"
        );
        assert!(section.items[0].title.contains("openssl"));
        assert!(section.items[1].title.contains("curl"));
        assert!(
            !section.items[1].title.starts_with('\u{25BC}'),
            "upgrade should not have ▼ prefix"
        );
    }

    #[test]
    fn test_normalize_version_changes_epoch_aware_subtitle() {
        let mut snap = empty_snapshot();
        let mut rpm = RpmSection::default();
        rpm.version_changes = vec![VersionChange {
            name: "bash".into(),
            arch: "x86_64".into(),
            host_version: "5.2.26-3.el9".into(),
            base_version: "5.2.26-3.el9".into(),
            host_epoch: "0".into(),
            base_epoch: "1".into(),
            direction: VersionChangeDirection::Upgrade,
        }];
        snap.rpm = Some(rpm);
        snap.baseline = Some(BaselineData {
            image_digest: "sha256:test".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });

        let section = normalize_version_changes(&snap);
        let subtitle = section.items[0].subtitle.as_ref().unwrap();
        assert!(
            subtitle.contains("1:"),
            "subtitle should show epoch prefix '1:' — got: {}",
            subtitle
        );
        assert!(
            subtitle.contains("0:"),
            "subtitle should show epoch prefix '0:' — got: {}",
            subtitle
        );
    }

    #[test]
    fn test_normalize_version_changes_epoch_only_same_evr() {
        // epoch "2" vs "1" with identical version-release
        let mut snap = empty_snapshot();
        let mut rpm = RpmSection::default();
        rpm.version_changes = vec![VersionChange {
            name: "glibc".into(),
            arch: "x86_64".into(),
            host_version: "2.34-100.el9".into(),
            base_version: "2.34-100.el9".into(),
            host_epoch: "2".into(),
            base_epoch: "1".into(),
            direction: VersionChangeDirection::Downgrade,
        }];
        snap.rpm = Some(rpm);
        snap.baseline = Some(BaselineData {
            image_digest: "sha256:test".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });

        let section = normalize_version_changes(&snap);
        let subtitle = section.items[0].subtitle.as_ref().unwrap();
        assert!(
            subtitle.contains("1:2.34-100.el9"),
            "base side should show epoch — got: {}",
            subtitle
        );
        assert!(
            subtitle.contains("2:2.34-100.el9"),
            "host side should show epoch — got: {}",
            subtitle
        );
    }

    #[test]
    fn test_normalize_version_changes_empty_vs_zero_epoch_normalized() {
        // epoch "0" vs "" with different versions — both normalize to "0",
        // so no epoch prefix should appear
        let mut snap = empty_snapshot();
        let mut rpm = RpmSection::default();
        rpm.version_changes = vec![VersionChange {
            name: "zlib".into(),
            arch: "x86_64".into(),
            host_version: "1.2.12-1.el9".into(),
            base_version: "1.2.11-1.el9".into(),
            host_epoch: String::new(),
            base_epoch: "0".into(),
            direction: VersionChangeDirection::Downgrade,
        }];
        snap.rpm = Some(rpm);
        snap.baseline = Some(BaselineData {
            image_digest: "sha256:test".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });

        let section = normalize_version_changes(&snap);
        let subtitle = section.items[0].subtitle.as_ref().unwrap();
        assert!(
            !subtitle.contains(':'),
            "no epoch prefix when both sides normalize to 0 — got: {}",
            subtitle
        );
    }

    #[test]
    fn test_normalize_version_changes_no_baseline() {
        let mut snap = empty_snapshot();
        snap.rpm = Some(RpmSection::default());
        // No baseline set — snap.baseline is None by default

        let section = normalize_version_changes(&snap);
        assert!(section.items.is_empty());
        assert_eq!(
            section.empty_reason.as_deref(),
            Some("no_baseline"),
            "empty rpm with no baseline should give no_baseline reason"
        );
    }

    #[test]
    fn test_normalize_version_changes_zero_drift() {
        let mut snap = empty_snapshot();
        snap.rpm = Some(RpmSection::default());
        snap.baseline = Some(BaselineData {
            image_digest: "sha256:test".into(),
            packages: std::collections::HashMap::new(),
            extracted_at: "2026-01-01T00:00:00Z".into(),
        });

        let section = normalize_version_changes(&snap);
        assert!(section.items.is_empty());
        assert_eq!(
            section.empty_reason.as_deref(),
            Some("zero_drift"),
            "empty version_changes with baseline should give zero_drift reason"
        );
    }

    #[test]
    fn test_normalize_version_changes_no_rpm() {
        let snap = empty_snapshot();
        // snap.rpm is None by default

        let section = normalize_version_changes(&snap);
        assert!(section.items.is_empty());
        assert_eq!(
            section.empty_reason.as_deref(),
            Some("data_unavailable"),
            "no rpm section should give data_unavailable reason"
        );
    }
}
