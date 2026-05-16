use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::ConfigFileKind;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::RefinementOp;
use serde::{Deserialize, Serialize};
use serde_json::json;
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
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ContextItem {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub detail: Option<String>,
    pub searchable_text: String,
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
    let completeness = serde_json::to_value(&snap.completeness).unwrap_or(json!("unknown"));

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
    }))
}

pub async fn get_view(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let session = state.session.lock().unwrap();
    Json(serde_json::to_value(session.view()).unwrap())
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
    Ok(Json(serde_json::to_value(session.view()).unwrap()))
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
    Ok(Json(serde_json::to_value(session.view()).unwrap()))
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
    Ok(Json(serde_json::to_value(session.view()).unwrap()))
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
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, AppError> {
    // Parse generation from request body — malformed JSON → 400
    let req: TarballRequest = serde_json::from_slice(&body).map_err(|_| {
        AppError(inspectah_refine::types::RefineError::BadRequest(
            "request body must be JSON with 'generation' field".into(),
        ))
    })?;

    // Snapshot state under the lock, then release before expensive work.
    let (projected, _generation) = {
        let session = state.session.lock().unwrap();
        if req.generation != session.generation() {
            return Err(AppError(
                inspectah_refine::types::RefineError::StaleGeneration {
                    expected: req.generation,
                    actual: session.generation(),
                },
            ));
        }
        (session.snapshot_projected(), session.generation())
    };
    // Lock is released here.

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
    ))
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
    session.mark_viewed(&req.id);
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
pub fn normalize_for_context(snap: &InspectionSnapshot) -> Vec<ContextSection> {
    vec![
        normalize_services(snap),
        normalize_containers(snap),
        normalize_users_groups(snap),
        normalize_network(snap),
        normalize_storage(snap),
        normalize_scheduled_tasks(snap),
        normalize_non_rpm_software(snap),
        normalize_kernel_boot(snap),
        normalize_selinux(snap),
    ]
}

fn normalize_services(snap: &InspectionSnapshot) -> ContextSection {
    let mut items = Vec::new();

    if let Some(svc) = &snap.services {
        // Collect drop-in units for lookup (matching state_change units get folded)
        let mut dropin_by_unit: std::collections::HashMap<&str, Vec<&str>> =
            std::collections::HashMap::new();
        let mut standalone_dropins = Vec::new();
        let state_change_units: std::collections::HashSet<&str> =
            svc.state_changes.iter().map(|sc| sc.unit.as_str()).collect();

        for d in &svc.drop_ins {
            if state_change_units.contains(d.unit.as_str()) {
                dropin_by_unit
                    .entry(d.unit.as_str())
                    .or_default()
                    .push(&d.content);
            } else {
                standalone_dropins.push(d);
            }
        }

        // ServiceStateChange items
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
                subtitle: Some(format!("{} \u{2192} {}", sc.current_state, sc.action)),
                detail: dropin_detail,
                searchable_text: search,
            });
        }

        // Standalone drop-ins (no matching state_change)
        for d in &standalone_dropins {
            items.push(ContextItem {
                id: d.path.clone(),
                title: d.unit.clone(),
                subtitle: Some("drop-in".to_string()),
                detail: Some(d.content.clone()),
                searchable_text: format!("{} {} {}", d.unit, d.path, d.content),
            });
        }

        // enabled_units
        for unit in &svc.enabled_units {
            items.push(ContextItem {
                id: unit.clone(),
                title: unit.clone(),
                subtitle: Some("enabled".to_string()),
                detail: None,
                searchable_text: unit.clone(),
            });
        }

        // disabled_units
        for unit in &svc.disabled_units {
            items.push(ContextItem {
                id: unit.clone(),
                title: unit.clone(),
                subtitle: Some("disabled".to_string()),
                detail: None,
                searchable_text: unit.clone(),
            });
        }
    }

    ContextSection {
        id: "services".to_string(),
        display_name: "Services".to_string(),
        items,
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
            let service_names: Vec<&str> =
                cf.images.iter().map(|s| s.service.as_str()).collect();
            let subtitle = service_names.join(", ");
            let search = format!("{} {}", cf.path, service_names.join(" "));
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
            items.push(ContextItem {
                id: rc.id.clone(),
                title: rc.name.clone(),
                subtitle: Some(subtitle),
                detail,
                searchable_text: format!("{} {} {}", rc.name, rc.image, rc.status),
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
    }
}

fn normalize_users_groups(snap: &InspectionSnapshot) -> ContextSection {
    let mut items = Vec::new();

    if let Some(ug) = &snap.users_groups {
        // Users (serde_json::Value)
        for user in &ug.users {
            let name = user
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let uid = user
                .get("uid")
                .map(|v| v.to_string())
                .unwrap_or_default();

            // Build detail from sudoers + SSH keys
            let mut detail_parts = Vec::new();

            // Match sudoers rules referencing this user
            let user_sudoers: Vec<&str> = ug
                .sudoers_rules
                .iter()
                .filter(|r| r.contains(&name))
                .map(|r| r.as_str())
                .collect();
            if !user_sudoers.is_empty() {
                detail_parts.push(format!("sudoers: {}", user_sudoers.join("; ")));
            }

            // SSH key refs for this user
            let user_keys: Vec<String> = ug
                .ssh_authorized_keys_refs
                .iter()
                .filter(|k| {
                    k.get("user")
                        .and_then(|v| v.as_str())
                        .map(|u| u == name)
                        .unwrap_or(false)
                })
                .filter_map(|k| {
                    k.get("path")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
                .collect();
            if !user_keys.is_empty() {
                detail_parts.push(format!("ssh_keys: {}", user_keys.join(", ")));
            }

            let detail = if detail_parts.is_empty() {
                None
            } else {
                Some(detail_parts.join("\n"))
            };

            items.push(ContextItem {
                id: name.clone(),
                title: name.clone(),
                subtitle: Some(format!("uid:{uid}")),
                detail,
                searchable_text: format!("{} {}", name, uid),
            });
        }

        // Groups (serde_json::Value)
        for group in &ug.groups {
            let name = group
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let gid = group
                .get("gid")
                .map(|v| v.to_string())
                .unwrap_or_default();

            items.push(ContextItem {
                id: name.clone(),
                title: name.clone(),
                subtitle: Some(format!("gid:{gid}")),
                detail: None,
                searchable_text: format!("{} {}", name, gid),
            });
        }
    }

    ContextSection {
        id: "users_groups".to_string(),
        display_name: "Users & Groups".to_string(),
        items,
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
                searchable_text: format!(
                    "{} {} {}",
                    vd.path, vd.size_estimate, vd.recommendation
                ),
            });
        }

        // CredentialRef
        for cr in &stor.credential_refs {
            items.push(ContextItem {
                id: cr.credential_path.clone(),
                title: cr.credential_path.clone(),
                subtitle: Some(format!("mount: {}", cr.mount_point)),
                detail: Some(cr.source.clone()),
                searchable_text: format!(
                    "{} {} {}",
                    cr.credential_path, cr.mount_point, cr.source
                ),
            });
        }
    }

    ContextSection {
        id: "storage".to_string(),
        display_name: "Storage".to_string(),
        items,
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
    }
}

fn normalize_non_rpm_software(snap: &InspectionSnapshot) -> ContextSection {
    let mut items = Vec::new();

    if let Some(nrpm) = &snap.non_rpm_software {
        // NonRpmItem
        for item in &nrpm.items {
            let subtitle = format!("{} ({})", item.method, item.confidence);
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
                Some(pkg_list.join(", "))
            } else {
                Some(item.path.clone())
            };

            let mut search = format!(
                "{} {} {} {}",
                item.name, item.path, item.method, item.lang
            );
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
                subtitle: Some(format!(
                    "\"{}\" (default: \"{}\")",
                    so.runtime, so.default
                )),
                detail: Some(so.source.clone()),
                searchable_text: format!(
                    "{} {} {} {}",
                    so.key, so.runtime, so.default, so.source
                ),
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

        // FIPS mode — single synthetic item
        if se.fips_mode {
            items.push(ContextItem {
                id: "fips_mode".to_string(),
                title: "FIPS mode".to_string(),
                subtitle: Some("enabled".to_string()),
                detail: None,
                searchable_text: "fips mode enabled".to_string(),
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
        display_name: "SELinux".to_string(),
        items,
    }
}
