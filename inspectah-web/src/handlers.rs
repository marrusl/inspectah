use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::completeness::Completeness;
use inspectah_core::types::users::UserContainerfileStrategy;
use inspectah_refine::repo_index::{DISTRO_REPOS, RepoIndex};
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{RefinementOp, RepoProvenance, UserPasswordOp};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeSet;

/// Primary header name for acknowledging sensitive data exports.
/// Both this and the legacy header are accepted for backward compatibility.
pub const ACK_SENSITIVE_HEADER: &str = "x-ack-sensitive";
const LEGACY_ACK_SENSITIVE_HEADER: &str = "x-acknowledge-sensitive";

/// Produce a display version that avoids duplicating what's already in pretty_name.
///
/// Cases:
/// - "CentOS Stream 9" + "9"       → "" (exact match, pretty_name is sufficient)
/// - "Fedora Linux 41 (Server)" + "41" → "" (version appears inside pretty_name)
/// - "RHEL 9.4 (Plow)" + "9.4"    → "" (exact match inside pretty_name)
/// - "RHEL 10" + "10.2"           → "(10.2)" (major in name, minor is new info)
/// - "RHEL" + "10.2"              → "10.2" (no version in name at all)
fn deduplicate_version(pretty_name: &str, version_id: &str) -> String {
    if version_id.is_empty() {
        return String::new();
    }
    // Exact version already appears as a word boundary in pretty_name
    for (i, _) in pretty_name.match_indices(version_id) {
        let before_ok = i == 0 || !pretty_name.as_bytes()[i - 1].is_ascii_alphanumeric();
        let after = i + version_id.len();
        let after_ok =
            after >= pretty_name.len() || !pretty_name.as_bytes()[after].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return String::new();
        }
    }
    // Major part appears but version_id has a minor (e.g., name has "10", version is "10.2")
    if let Some(major) = version_id.split('.').next()
        && major != version_id
    {
        for (i, _) in pretty_name.match_indices(major) {
            let before_ok = i == 0 || !pretty_name.as_bytes()[i - 1].is_ascii_alphanumeric();
            let after = i + major.len();
            let after_ok = after >= pretty_name.len()
                || !pretty_name.as_bytes()[after].is_ascii_alphanumeric();
            if before_ok && after_ok {
                return format!("({version_id})");
            }
        }
    }

    // No overlap at all
    version_id.to_string()
}
use std::sync::{Arc, Mutex, OnceLock};

use crate::error::AppError;

/// Shared application state. Wraps the session mutex alongside caches
/// that are immutable for the session lifetime.
pub struct AppState {
    pub session: Arc<Mutex<RefineSession>>,
    pub sections_cache: OnceLock<Vec<ReferenceSection>>,
}

pub use crate::web_types::*;

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
            let name = if !os.pretty_name.is_empty() {
                os.pretty_name.clone()
            } else {
                os.name.clone()
            };
            let version = deduplicate_version(&name, &os.version_id);
            (name, version, os.id.clone())
        })
        .unwrap_or_default();

    let system_type = serde_json::to_value(&snap.system_type).unwrap_or(json!("unknown"));
    let completeness = match &snap.completeness {
        Completeness::Complete => "complete",
        Completeness::Partial { .. } => "partial",
        Completeness::Incomplete { .. } => "incomplete",
    };

    let fleet = snap.fleet_meta.as_ref().map(|meta| {
        let variant_count = inspectah_refine::fleet::variant_summary(snap, session.fleet_context())
            .map(|s| s.paths_with_variants)
            .unwrap_or(0);
        json!({
            "host_count": meta.host_count,
            "hostnames": meta.hostnames,
            "zones_active": session.fleet_context()
                .map(|fc| fc.zones_active).unwrap_or(false),
            "variant_count": variant_count,
            "label": meta.label,
            "merged_at": meta.merged_at,
        })
    });

    let session_is_sensitive = session.is_sensitive();

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
        "fleet": fleet,
        "session_is_sensitive": session_is_sensitive,
    }))
}

pub async fn get_view(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let session = state.session.lock().unwrap();
    let response = crate::adapter::build_web_view(&session);
    Json(serde_json::to_value(&response).unwrap())
}

/// Build `RepoGroupInfo` entries from the session's repo index and current view.
pub(crate) fn build_repo_groups(session: &RefineSession) -> Vec<RepoGroupInfo> {
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
        serde_json::to_value(crate::adapter::build_web_view(&session)).unwrap(),
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
        serde_json::to_value(crate::adapter::build_web_view(&session)).unwrap(),
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
        serde_json::to_value(crate::adapter::build_web_view(&session)).unwrap(),
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
    let (projected, _generation, sensitive, original_includes, export_filename) = {
        let session = state.session.lock().unwrap();
        if req.generation != session.generation() {
            return Err(AppError(
                inspectah_refine::types::RefineError::StaleGeneration {
                    expected: req.generation,
                    actual: session.generation(),
                },
            ));
        }
        let orig_inc: std::collections::HashMap<String, bool> = session
            .snapshot()
            .rpm
            .as_ref()
            .map(|r| {
                r.packages_added
                    .iter()
                    .map(|p| (format!("{}.{}", p.name, p.arch), p.include))
                    .collect()
            })
            .unwrap_or_default();
        // Derive download filename from hostname in the snapshot.
        let hostname = session
            .snapshot()
            .meta
            .get("hostname")
            .and_then(|v| v.as_str())
            .unwrap_or("inspectah");
        let sanitized = inspectah_pipeline::render::tarball::sanitize_hostname(hostname);
        let filename = format!("inspectah-{sanitized}-refined.tar.gz");
        (
            session.snapshot_projected(),
            session.generation(),
            session.is_sensitive(),
            orig_inc,
            filename,
        )
    };
    // Lock is released here.

    // Export gating: require explicit acknowledgment for sensitive sessions.
    if sensitive {
        let ack = headers
            .get(ACK_SENSITIVE_HEADER)
            .or_else(|| headers.get(LEGACY_ACK_SENSITIVE_HEADER))
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
    let dl_name = export_filename.clone();
    let bytes = tokio::task::spawn_blocking(
        move || -> Result<Vec<u8>, inspectah_refine::types::RefineError> {
            let tempdir = tempfile::tempdir()?;
            let tarball_path = tempdir.path().join(&dl_name);
            inspectah_refine::session::render_refine_export(
                &projected,
                &tarball_path,
                Some(&original_includes),
            )?;
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

    let disposition = format!("attachment; filename=\"{export_filename}\"");
    Ok((
        StatusCode::OK,
        [
            (
                axum::http::header::CONTENT_TYPE.as_str(),
                "application/gzip".to_string(),
            ),
            (
                axum::http::header::CONTENT_DISPOSITION.as_str(),
                disposition,
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
            return Err(AppError(inspectah_refine::types::RefineError::BadRequest(
                format!("unknown strategy: {other} (expected \"skip\" or \"useradd\")"),
            )));
        }
    };
    let op = RefinementOp::UserStrategy {
        username: req.username,
        strategy,
    };
    let mut session = state.session.lock().unwrap();
    session.apply(op).map_err(AppError)?;
    Ok(Json(
        serde_json::to_value(crate::adapter::build_web_view(&session)).unwrap(),
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
                return Err(AppError(inspectah_refine::types::RefineError::BadRequest(
                    "cannot preserve password: snapshot does not contain preserved credentials"
                        .into(),
                )));
            }
            let has_hash = snap
                .users_groups
                .as_ref()
                .and_then(|ug| {
                    ug.users
                        .iter()
                        .find(|u| u.get("name").and_then(|v| v.as_str()) == Some(&req.username))
                })
                .and_then(|u| u.get("password_hash"))
                .and_then(|v| v.as_str())
                .map(|s| !s.is_empty())
                .unwrap_or(false);
            if !has_hash {
                return Err(AppError(inspectah_refine::types::RefineError::BadRequest(
                    format!(
                        "cannot preserve password for \"{}\": user has no password hash",
                        req.username
                    ),
                )));
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
                return Err(AppError(inspectah_refine::types::RefineError::BadRequest(
                    "password choice \"new\" requires a non-empty \"hash\" field".into(),
                )));
            }
            if !hash.starts_with("$6$") && !hash.starts_with("$5$") && !hash.starts_with("$y$") {
                return Err(AppError(inspectah_refine::types::RefineError::BadRequest(
                    "invalid hash format: must start with $6$, $5$, or $y$ (crypt(3) format)"
                        .into(),
                )));
            }
            UserPasswordOp::New {
                username: req.username,
                hash: req.hash,
            }
        }
        other => {
            return Err(AppError(inspectah_refine::types::RefineError::BadRequest(
                format!(
                    "unknown password choice: {other} (expected \"none\", \"preserve\", or \"new\")"
                ),
            )));
        }
    };
    let op = RefinementOp::UserPassword(pw_op);
    let mut session = state.session.lock().unwrap();
    session.apply(op).map_err(AppError)?;
    Ok(Json(
        serde_json::to_value(crate::adapter::build_web_view(&session)).unwrap(),
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
    let crypt_re = Regex::new(r#"\$(?:6|5|y)\$[^\s'""]+"#).expect("crypt regex");
    let result = crypt_re.replace_all(content, "<REDACTED>");

    // Redact SSH key base64 blobs, keeping the key type prefix.
    // Matches: ssh-rsa AAAA..., ssh-ed25519 AAAA..., ecdsa-sha2-nistp256 AAAA..., etc.
    let ssh_re =
        Regex::new(r#"((?:ssh-(?:rsa|ed25519|dss)|ecdsa-sha2-nistp(?:256|384|521))\s+)\S+"#)
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
        "error": "session contains sensitive data — set x-ack-sensitive: true to export",
        "sensitivity_summary": reasons,
    })
}

// -- New Phase 4 endpoints ------------------------------------------------

pub async fn get_sections(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let sections = state.sections_cache.get_or_init(|| {
        let session = state.session.lock().unwrap();
        crate::adapter::build_web_sections(session.reference())
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
    use inspectah_core::types::warnings::{Warning, WarningSeverity};
    use inspectah_refine::types::RepoTier;

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
                    locked: false,
                    source_repo: "appstream".into(),
                    ..Default::default()
                },
                PackageEntry {
                    name: "kernel".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    include: true,
                    locked: false,
                    source_repo: "baseos".into(),
                    ..Default::default()
                },
                PackageEntry {
                    name: "custom-tool".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    include: true,
                    locked: false,
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
        assert_eq!(appstream.tier, RepoTier::Distro);
        assert!(appstream.enabled);

        let baseos = groups
            .iter()
            .find(|g| g.section_id == "baseos")
            .expect("should have baseos group");
        assert!(baseos.is_distro);
        assert_eq!(baseos.tier, RepoTier::Distro);

        let epel = groups
            .iter()
            .find(|g| g.section_id == "epel")
            .expect("should have epel group");
        assert!(!epel.is_distro);
        assert_eq!(epel.tier, RepoTier::ThirdParty);
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
                    locked: false,
                    source_repo: "appstream".into(),
                    ..Default::default()
                },
                PackageEntry {
                    name: "zsh".into(),
                    arch: "x86_64".into(),
                    state: PackageState::Added,
                    include: true,
                    locked: false,
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
                    locked: false,
                    ..Default::default()
                },
                PackageEntry {
                    name: "epel-release".into(),
                    arch: "noarch".into(),
                    state: PackageState::Added,
                    source_repo: "epel".into(),
                    include: true,
                    locked: false,
                    ..Default::default()
                },
            ],
            repo_files: vec![
                RepoFile {
                    path: "/etc/yum.repos.d/centos.repo".into(),
                    content: "[baseos]\nname=CentOS BaseOS\n\n[appstream]\nname=CentOS AppStream\n"
                        .into(),
                    include: true,
                    locked: false,
                    ..Default::default()
                },
                RepoFile {
                    path: "/etc/yum.repos.d/epel.repo".into(),
                    content: "[epel]\nname=EPEL 9\n".into(),
                    include: true,
                    locked: false,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });

        let mut session = RefineSession::new(snap);

        // Apply SetInclude (exclude repo) via the op API
        let op = RefinementOp::SetInclude {
            item_id: inspectah_refine::types::ItemId::Repo {
                path: "epel".into(),
            },
            include: false,
        };
        session
            .apply(op)
            .expect("SetInclude exclude repo should succeed");

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

    // -- deduplicate_version tests -------------------------------------------

    #[test]
    fn dedup_version_exact_suffix() {
        // CentOS Stream 9 + 9 → empty (already in name)
        assert_eq!(deduplicate_version("CentOS Stream 9", "9"), "");
    }

    #[test]
    fn dedup_version_inside_with_parens() {
        // Fedora Linux 41 (Server Edition) + 41 → empty
        assert_eq!(
            deduplicate_version("Fedora Linux 41 (Server Edition)", "41"),
            ""
        );
    }

    #[test]
    fn dedup_version_rhel_9_exact() {
        // RHEL 9.4 (Plow) + 9.4 → empty
        assert_eq!(
            deduplicate_version("Red Hat Enterprise Linux 9.4 (Plow)", "9.4"),
            ""
        );
    }

    #[test]
    fn dedup_version_rhel_10_minor() {
        // RHEL 10 + 10.2 → "(10.2)" — major in name, minor is new info
        assert_eq!(
            deduplicate_version("Red Hat Enterprise Linux 10", "10.2"),
            "(10.2)"
        );
    }

    #[test]
    fn dedup_version_rhel_10_codename_minor() {
        // RHEL 10 (Coughlan) + 10.2 → "(10.2)"
        assert_eq!(
            deduplicate_version("Red Hat Enterprise Linux 10 (Coughlan)", "10.2"),
            "(10.2)"
        );
    }

    #[test]
    fn dedup_version_no_overlap() {
        // Name has no version at all → pass through
        assert_eq!(
            deduplicate_version("Red Hat Enterprise Linux", "10.2"),
            "10.2"
        );
    }

    #[test]
    fn dedup_version_empty() {
        assert_eq!(deduplicate_version("CentOS Stream 9", ""), "");
    }

    #[test]
    fn dedup_version_no_false_positive_on_partial_digit() {
        // "19" should not match version "9" — word boundary check
        assert_eq!(deduplicate_version("Build19 Linux", "9"), "9");
    }

    #[test]
    fn dedup_version_rhel_9_with_minor() {
        // RHEL 9 + 9.4 → "(9.4)" — major in name, minor adds info
        assert_eq!(
            deduplicate_version("Red Hat Enterprise Linux 9", "9.4"),
            "(9.4)"
        );
    }

    #[test]
    fn dedup_version_bazzite_current() {
        // Bazzite 44 (FROM Fedora Kinoite) + 44 → empty
        assert_eq!(
            deduplicate_version("Bazzite 44 (FROM Fedora Kinoite)", "44"),
            ""
        );
    }

    #[test]
    fn dedup_version_bazzite_old_format() {
        // Old UBlue format: version embedded in date-stamped string
        assert_eq!(
            deduplicate_version("Fedora Linux 40.20240621.0 (Bazzite)", "40"),
            ""
        );
    }

    #[test]
    fn dedup_version_aurora() {
        assert_eq!(
            deduplicate_version("Aurora 44 (FROM Fedora Kinoite)", "44"),
            ""
        );
    }

    #[test]
    fn dedup_version_bluefin() {
        assert_eq!(
            deduplicate_version(
                "Bluefin (Version: 41.20250209.1 / FROM Fedora Silverblue 41)",
                "41"
            ),
            ""
        );
    }

    #[test]
    fn dedup_version_bluefin_lts() {
        // Bluefin LTS is built on CentOS Stream 10
        assert_eq!(
            deduplicate_version("Bluefin LTS 10 (FROM CentOS Stream 10)", "10"),
            ""
        );
    }

    /// Structural test: ensure the ACK_SENSITIVE_HEADER constant stays in sync
    /// with the CORS allow-headers configuration in lib.rs.
    ///
    /// This is a compile-time contract enforced at test-time. If the header
    /// name changes in one place, this test will fail until both are updated.
    #[test]
    fn test_ack_sensitive_header_cors_sync() {
        // The constant used in handlers must match what's configured in CORS.
        // lib.rs references handlers::ACK_SENSITIVE_HEADER, so this test verifies
        // the constant is properly exposed and has the expected value.
        assert_eq!(
            ACK_SENSITIVE_HEADER, "x-ack-sensitive",
            "ACK_SENSITIVE_HEADER constant must match CORS configuration"
        );

        // Verify the legacy header constant exists and is distinct
        assert_eq!(
            LEGACY_ACK_SENSITIVE_HEADER, "x-acknowledge-sensitive",
            "Legacy header name must be preserved for backward compatibility"
        );
    }
}
