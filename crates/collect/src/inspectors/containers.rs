use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::traits::progress::ProgressSink;
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::containers::{
    ComposeFile, ComposeService, ContainerMount, ContainerSection, FlatpakApp, QuadletUnit,
    RunningContainer,
};
use inspectah_core::types::redaction::RedactionHint;
use inspectah_core::types::warnings::Warning;
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const QUADLET_EXTENSIONS: &[&str] = &[
    ".container",
    ".volume",
    ".network",
    ".kube",
    ".pod",
    ".image",
    ".build",
];

const COMPOSE_PATTERNS: &[&str] = &[
    "docker-compose*.yml",
    "docker-compose*.yaml",
    "compose*.yml",
    "compose*.yaml",
];

const COMPOSE_SEARCH_DIRS: &[&str] = &["opt", "srv", "etc"];

const NON_SYSTEM_UID_MIN: u32 = 1000;
const NON_SYSTEM_UID_MAX: u32 = 60000;

/// VCS directory names whose presence causes the entire subtree to be skipped.
const PRUNE_MARKERS: &[&str] = &[".git", ".svn", ".hg"];

/// Directory names always skipped during recursive walks.
const SKIP_DIR_NAMES: &[&str] = &[
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    ".tox",
    ".nox",
    "node_modules",
    ".eggs",
    ".vscode",
    ".idea",
    ".cursor",
];

/// Env var name patterns that suggest sensitive content.
const SECRET_PATTERNS: &[&str] = &["PASSWORD", "SECRET", "TOKEN", "KEY", "CREDENTIAL"];

// ---------------------------------------------------------------------------
// Inspector
// ---------------------------------------------------------------------------

/// Inspects container workloads: Quadlet unit files, compose YAML files,
/// running containers (via podman), and installed Flatpak applications.
pub struct ContainersInspector;

impl ContainersInspector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ContainersInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl Inspector for ContainersInspector {
    fn id(&self) -> InspectorId {
        InspectorId::Containers
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(
        &self,
        ctx: &InspectionContext<'_>,
        progress: &dyn ProgressSink,
    ) -> Result<InspectorOutput, InspectorError> {
        let exec = ctx.executor;
        let mut warnings: Vec<Warning> = Vec::new();
        let mut hints: Vec<RedactionHint> = Vec::new();
        let mut degraded_reasons: Vec<String> = Vec::new();

        let mut section = ContainerSection {
            quadlet_units: Vec::new(),
            compose_files: Vec::new(),
            running_containers: Vec::new(),
            flatpak_apps: Vec::new(),
        };

        // --- Quadlet units ---
        let mut quadlet_dirs = vec![
            "/etc/containers/systemd".to_string(),
            "/usr/share/containers/systemd".to_string(),
            "/etc/systemd/system".to_string(),
        ];
        quadlet_dirs.extend(user_quadlet_dirs(exec));

        for dir in &quadlet_dirs {
            let units = scan_quadlet_dir(exec, dir, &mut degraded_reasons);
            section.quadlet_units.extend(units);
        }

        // --- Compose files ---
        for search_dir in COMPOSE_SEARCH_DIRS {
            let root = format!("/{search_dir}");
            let files = find_compose_files(exec, &root, &mut hints, &mut degraded_reasons);
            section.compose_files.extend(files);
        }

        // --- Running containers (podman) ---
        let (containers, podman_warnings) =
            query_podman_containers(exec, &mut hints, &mut degraded_reasons);
        section.running_containers = containers;
        warnings.extend(podman_warnings);

        // --- Flatpak apps ---
        section.flatpak_apps = detect_flatpak_apps(exec);

        // Emit metric for progress rendering
        progress.emit(inspectah_core::types::progress::ProgressEvent::Metric {
            inspector: InspectorId::Containers,
            kind: inspectah_core::types::progress::MetricKind::ContainersFound,
            value: section.running_containers.len(),
        });

        let output = InspectorOutput {
            section: SectionData::Containers(section),
            warnings,
            redaction_hints: hints,
        };

        if degraded_reasons.is_empty() {
            Ok(output)
        } else {
            Err(InspectorError::Degraded {
                partial: Box::new(output),
                reason: degraded_reasons.join("; "),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Quadlet scanning
// ---------------------------------------------------------------------------

/// Scans a directory for Quadlet unit files (.container, .volume, etc.).
fn scan_quadlet_dir(
    exec: &dyn Executor,
    dir: &str,
    degraded_reasons: &mut Vec<String>,
) -> Vec<QuadletUnit> {
    let dir_path = Path::new(dir);

    let entries = match exec.read_dir(dir_path) {
        Ok(e) => e,
        Err(ref err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Vec::new();
        }
        Err(ref err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            degraded_reasons.push(format!(
                "Permission denied reading {dir} -- quadlet units may be incomplete"
            ));
            return Vec::new();
        }
        Err(_) => return Vec::new(),
    };

    let mut sorted = entries;
    sorted.sort();

    let mut units = Vec::new();
    for name in &sorted {
        let ext = match name.rfind('.') {
            Some(pos) => &name[pos..],
            None => continue,
        };
        if !QUADLET_EXTENSIONS.contains(&ext) {
            continue;
        }

        let path = format!("{dir}/{name}");
        let content = match exec.read_file(Path::new(&path)) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let image = if ext == ".container" {
            extract_quadlet_image(&content)
        } else {
            String::new()
        };

        let (ports, volumes) = extract_quadlet_ports_and_volumes(&content);

        let rel_path = path
            .strip_prefix(exec.host_root().to_str().unwrap_or("/"))
            .unwrap_or(&path);
        let rel_path = rel_path.strip_prefix('/').unwrap_or(rel_path).to_string();

        units.push(QuadletUnit {
            path: rel_path,
            name: name.clone(),
            content,
            image,
            ports,
            volumes,
            include: true,
            ..Default::default()
        });
    }
    units
}

/// Parses `Image=` from a .container quadlet file content.
fn extract_quadlet_image(content: &str) -> String {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(val) = trimmed.strip_prefix("Image=") {
            let val = val.trim();
            if !val.is_empty() {
                return val.to_string();
            }
        }
    }
    String::new()
}

/// Parses `PublishPort=` and `Volume=` directives from quadlet content.
fn extract_quadlet_ports_and_volumes(content: &str) -> (Vec<String>, Vec<String>) {
    let mut ports = Vec::new();
    let mut volumes = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();

        if lower.starts_with("publishport") && trimmed.contains('=') {
            let val = trimmed[trimmed.find('=').unwrap_or(0) + 1..].trim();
            if !val.is_empty() {
                ports.push(val.to_string());
            }
        } else if lower.starts_with("volume")
            && !lower.starts_with("volumedriver")
            && trimmed.contains('=')
        {
            let val = trimmed[trimmed.find('=').unwrap_or(0) + 1..].trim();
            if !val.is_empty() {
                volumes.push(val.to_string());
            }
        }
    }

    (ports, volumes)
}

/// Discovers per-user quadlet directories by parsing /etc/passwd for
/// non-system UIDs (1000-59999).
fn user_quadlet_dirs(exec: &dyn Executor) -> Vec<String> {
    let passwd = match exec.read_file(Path::new("/etc/passwd")) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };

    let mut dirs = Vec::new();
    for line in passwd.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 7 {
            continue;
        }
        let uid: u32 = match parts[2].parse() {
            Ok(u) => u,
            Err(_) => continue,
        };
        if !(NON_SYSTEM_UID_MIN..NON_SYSTEM_UID_MAX).contains(&uid) {
            continue;
        }
        let home = parts[5].strip_prefix('/').unwrap_or(parts[5]);
        dirs.push(format!("/{home}/.config/containers/systemd"));
    }
    dirs
}

// ---------------------------------------------------------------------------
// Compose file discovery
// ---------------------------------------------------------------------------

/// Recursively searches a directory for compose files, pruning VCS
/// checkouts and dev-artifact directories.
fn find_compose_files(
    exec: &dyn Executor,
    root: &str,
    hints: &mut Vec<RedactionHint>,
    degraded_reasons: &mut Vec<String>,
) -> Vec<ComposeFile> {
    let mut matches = Vec::new();
    filtered_walk(exec, root, &mut |path: &str, name: &str| {
        for pattern in COMPOSE_PATTERNS {
            if match_glob(pattern, name) {
                matches.push(path.to_string());
            }
        }
    });
    matches.sort();

    let mut files = Vec::new();
    for path in &matches {
        let content = match exec.read_file(Path::new(path)) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Check for YAML anchors/aliases which our simple parser cannot handle.
        if content.contains("<<: *") || content.contains("*default") {
            // Detect anchor reference patterns.
            let has_anchor = content.lines().any(|l| {
                let t = l.trim();
                t.contains("&") && !t.starts_with('#')
            });
            if has_anchor {
                degraded_reasons.push(format!(
                    "Compose file {path} uses YAML anchors/aliases \
                     -- image extraction may be incomplete"
                ));
            }
        }

        // Scan for secret-like env vars and emit redaction hints.
        scan_compose_env_secrets(&content, path, hints);

        let parse_result = extract_compose_images(&content);
        if parse_result.had_anomalies {
            degraded_reasons.push(format!(
                "Compose file {path} has structural YAML issues \
                 -- image extraction may be incomplete"
            ));
        }

        let rel_path = path
            .strip_prefix(exec.host_root().to_str().unwrap_or("/"))
            .unwrap_or(path);
        let rel_path = rel_path.strip_prefix('/').unwrap_or(rel_path).to_string();

        files.push(ComposeFile {
            path: rel_path,
            images: parse_result.services,
            include: true,
            ..Default::default()
        });
    }
    files
}

/// Recursive directory traversal that prunes VCS checkouts and dev-artifact
/// directories. Calls `cb(full_path, base_name)` for each file visited.
fn filtered_walk(exec: &dyn Executor, dir: &str, cb: &mut dyn FnMut(&str, &str)) {
    let entries = match exec.read_dir(Path::new(dir)) {
        Ok(e) => e,
        Err(_) => return,
    };

    // Check for prune markers among children.
    for name in &entries {
        if PRUNE_MARKERS.contains(&name.as_str()) {
            return;
        }
    }

    let mut sorted = entries;
    sorted.sort();

    for name in &sorted {
        let full_path = format!("{dir}/{name}");

        // Check if this looks like a directory by trying to read_dir.
        // MockExecutor returns entries for registered dirs, NotFound otherwise.
        if exec.read_dir(Path::new(&full_path)).is_ok() || SKIP_DIR_NAMES.contains(&name.as_str()) {
            if !SKIP_DIR_NAMES.contains(&name.as_str()) {
                filtered_walk(exec, &full_path, cb);
            }
            continue;
        }

        cb(&full_path, name);
    }
}

/// Simple glob matching supporting `*` wildcards.
/// Only supports patterns like `prefix*suffix`.
fn match_glob(pattern: &str, name: &str) -> bool {
    if let Some(star_pos) = pattern.find('*') {
        let prefix = &pattern[..star_pos];
        let suffix = &pattern[star_pos + 1..];
        name.starts_with(prefix)
            && name.ends_with(suffix)
            && name.len() >= prefix.len() + suffix.len()
    } else {
        pattern == name
    }
}

/// Known compose service sub-keys that should never appear as service names.
/// If our parser sees one at service-indent level, the YAML structure is
/// broken (indentation error) and we flag it as a parse anomaly.
const COMPOSE_SUB_KEYS: &[&str] = &[
    "ports",
    "volumes",
    "environment",
    "env_file",
    "networks",
    "depends_on",
    "build",
    "command",
    "entrypoint",
    "restart",
    "deploy",
    "labels",
    "healthcheck",
    "logging",
    "secrets",
    "configs",
    "expose",
    "extra_hosts",
];

/// Result of compose image extraction.
struct ComposeParseResult {
    services: Vec<ComposeService>,
    /// True if the parser encountered structural anomalies (broken
    /// indentation, sub-keys misaligned to service level).
    had_anomalies: bool,
}

/// Parses `image:` fields from a compose YAML without requiring a YAML
/// library. Detects service-level indent dynamically so 2-space, 4-space,
/// and tab-indented files all work.
fn extract_compose_images(content: &str) -> ComposeParseResult {
    let mut results = Vec::new();
    let mut had_anomalies = false;

    let mut current_service = String::new();
    let mut service_indent: i32 = -1;
    let mut in_services = false;

    let image_re = match Regex::new(r"^image:\s*(.+)") {
        Ok(r) => r,
        Err(_) => {
            return ComposeParseResult {
                services: results,
                had_anomalies: true,
            };
        }
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let indent = line.len() - line.trim_start_matches([' ', '\t']).len();

        // Detect "services:" block.
        if trimmed == "services:" || trimmed.starts_with("services:") {
            in_services = true;
            service_indent = -1;
            current_service.clear();
            continue;
        }

        if !in_services {
            continue;
        }

        // Top-level key other than services -- stop.
        if indent == 0 && !trimmed.starts_with('#') {
            in_services = false;
            current_service.clear();
            service_indent = -1;
            continue;
        }

        // Calibrate service indent from the first indented key.
        if service_indent < 0 && indent > 0 {
            service_indent = indent as i32;
        }

        // Service-level key (e.g. "web:", "db:").
        if service_indent > 0 && indent == service_indent as usize && trimmed.ends_with(':') {
            let key_name = trimmed.trim_end_matches(':');
            // Detect compose sub-keys appearing at service level —
            // this signals broken indentation in the YAML.
            if COMPOSE_SUB_KEYS.contains(&key_name.to_lowercase().as_str()) {
                had_anomalies = true;
            } else {
                current_service = key_name.to_string();
            }
            continue;
        }

        // Inside a service -- look for image:.
        if !current_service.is_empty()
            && indent > service_indent as usize
            && let Some(caps) = image_re.captures(trimmed)
            && let Some(img_match) = caps.get(1)
        {
            let img = img_match
                .as_str()
                .trim()
                .trim_matches(|c| c == '\'' || c == '"');
            results.push(ComposeService {
                service: current_service.clone(),
                image: img.to_string(),
            });
        }
    }
    ComposeParseResult {
        services: results,
        had_anomalies,
    }
}

/// Scans compose file content for environment blocks with secret-like
/// variable names and emits redaction hints. Does NOT persist env content.
fn scan_compose_env_secrets(content: &str, path: &str, hints: &mut Vec<RedactionHint>) {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Look for KEY=VALUE or KEY: value patterns under environment blocks.
        let upper = trimmed.to_uppercase();
        for pattern in SECRET_PATTERNS {
            if upper.contains(pattern) && (trimmed.contains('=') || trimmed.contains(':')) {
                let rel_path = path.strip_prefix('/').unwrap_or(path);
                hints.push(RedactionHint {
                    path: rel_path.to_string(),
                    reason: format!("compose env var matches secret pattern '{pattern}'"),
                    confidence: None,
                });
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Podman container query
// ---------------------------------------------------------------------------

/// Runs `podman ps --format json` + `podman inspect` and returns parsed
/// container data, plus any warnings.
fn query_podman_containers(
    exec: &dyn Executor,
    hints: &mut Vec<RedactionHint>,
    degraded_reasons: &mut Vec<String>,
) -> (Vec<RunningContainer>, Vec<Warning>) {
    let mut warnings = Vec::new();

    // Check if podman is installed before attempting ps.
    let which = exec.run("which", &["podman"]);
    if which.exit_code != 0 {
        // Podman not installed -- skip silently (warning only).
        warnings.push(Warning {
            inspector: "containers".into(),
            message: "podman not installed -- live container data unavailable.".into(),
            ..Default::default()
        });
        return (Vec::new(), warnings);
    }

    let result = exec.run("podman", &["ps", "--format", "json"]);
    if result.exit_code != 0 {
        // Podman is installed but ps failed -- real failure, Degraded.
        degraded_reasons.push("podman ps failed -- live container data lost".to_string());
        warnings.push(Warning {
            inspector: "containers".into(),
            message: "podman ps failed -- live container data unavailable.".into(),
            ..Default::default()
        });
        return (Vec::new(), warnings);
    }

    let stdout = result.stdout.trim();
    if stdout.is_empty() {
        return (Vec::new(), warnings);
    }

    let ps_data: Vec<serde_json::Value> = match serde_json::from_str(stdout) {
        Ok(d) => d,
        Err(e) => {
            degraded_reasons.push(format!("podman ps JSON parse error: {e}"));
            return (Vec::new(), warnings);
        }
    };

    // Collect container IDs for podman inspect.
    let ids: Vec<String> = ps_data
        .iter()
        .filter_map(|c| {
            c.get("Id")
                .or_else(|| c.get("ID"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        })
        .collect();

    // Try podman inspect for rich data.
    if !ids.is_empty() {
        let mut args: Vec<&str> = vec!["inspect"];
        for id in &ids {
            args.push(id);
        }
        let ir = exec.run("podman", &args);
        if ir.exit_code == 0 && !ir.stdout.trim().is_empty() {
            match serde_json::from_str::<Vec<serde_json::Value>>(ir.stdout.trim()) {
                Ok(inspect_data) => {
                    let containers = parse_podman_inspect(&inspect_data, hints);
                    return (containers, warnings);
                }
                Err(e) => {
                    degraded_reasons.push(format!("podman inspect JSON parse error: {e}"));
                }
            }
        }
    }

    // Fallback to ps-only data.
    let containers = parse_podman_ps(&ps_data);
    (containers, warnings)
}

/// Extracts container details from `podman inspect` JSON output.
fn parse_podman_inspect(
    data: &[serde_json::Value],
    hints: &mut Vec<RedactionHint>,
) -> Vec<RunningContainer> {
    let mut containers = Vec::new();

    for c in data {
        let id = string_field(c, &["Id", "ID"]);
        let name = string_field(c, &["Name"]);
        let image_name = string_field(c, &["ImageName"]);
        let image_id = string_field(c, &["Image"]);

        // Status from State.Status.
        let status = c
            .get("State")
            .and_then(|s| s.get("Status"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let mounts = parse_mounts(c.get("Mounts"));

        let network_settings = c.get("NetworkSettings");
        let networks = network_settings
            .and_then(|ns| ns.get("Networks"))
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let ports = network_settings
            .and_then(|ns| ns.get("Ports"))
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        let config = c.get("Config");
        let env: Vec<String> = config
            .and_then(|cfg| cfg.get("Env"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|e| e.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        // Emit redaction hints for secret-like env vars.
        for entry in &env {
            let var_name = entry.split('=').next().unwrap_or("").to_uppercase();
            for pattern in SECRET_PATTERNS {
                if var_name.contains(pattern) {
                    hints.push(RedactionHint {
                        path: format!("container:{name}"),
                        reason: format!(
                            "container env var '{var_name}' matches secret pattern '{pattern}'"
                        ),
                        confidence: None,
                    });
                    break;
                }
            }
        }

        let restart_policy = extract_restart_policy(c);

        containers.push(RunningContainer {
            id,
            name,
            image: image_name,
            image_id,
            status,
            restart_policy,
            mounts,
            networks,
            ports,
            env,
            inspect_data: true,
            include: true,
            ..Default::default()
        });
    }
    containers
}

/// Fallback parser when podman inspect is unavailable. Extracts basic
/// fields from podman ps JSON.
fn parse_podman_ps(data: &[serde_json::Value]) -> Vec<RunningContainer> {
    let mut containers = Vec::new();

    for c in data {
        let id = string_field(c, &["ID", "Id"]);

        let name = match c.get("Names") {
            Some(serde_json::Value::Array(arr)) => arr
                .first()
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            Some(serde_json::Value::String(s)) => s.clone(),
            _ => String::new(),
        };

        let status = c
            .get("State")
            .and_then(|v| v.as_str())
            .or_else(|| c.get("Status").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string();

        containers.push(RunningContainer {
            id,
            name,
            image: string_field(c, &["Image"]),
            status,
            include: true,
            ..Default::default()
        });
    }
    containers
}

/// Extracts the first non-empty string value from a JSON object by trying
/// each key in order.
fn string_field(val: &serde_json::Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(s) = val.get(key).and_then(|v| v.as_str())
            && !s.is_empty()
        {
            return s.to_string();
        }
    }
    String::new()
}

/// Converts a Mounts JSON array into ContainerMount slices.
fn parse_mounts(val: Option<&serde_json::Value>) -> Vec<ContainerMount> {
    let arr = match val.and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return Vec::new(),
    };

    let mut mounts = Vec::new();
    for item in arr {
        let rw = item.get("RW").and_then(|v| v.as_bool()).unwrap_or(true);

        mounts.push(ContainerMount {
            mount_type: string_field(item, &["Type"]),
            source: string_field(item, &["Source"]),
            destination: string_field(item, &["Destination"]),
            mode: string_field(item, &["Mode"]),
            rw,
        });
    }
    mounts
}

/// Extracts the restart policy name from HostConfig.RestartPolicy.Name.
fn extract_restart_policy(c: &serde_json::Value) -> String {
    c.get("HostConfig")
        .and_then(|hc| hc.get("RestartPolicy"))
        .and_then(|rp| rp.get("Name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

// ---------------------------------------------------------------------------
// Flatpak detection
// ---------------------------------------------------------------------------

/// Lists installed system-level Flatpak applications and resolves remote
/// URLs for each origin.
fn detect_flatpak_apps(exec: &dyn Executor) -> Vec<FlatpakApp> {
    // Check if flatpak is installed.
    let which = exec.run("which", &["flatpak"]);
    if which.exit_code != 0 {
        return Vec::new();
    }

    let result = exec.run(
        "flatpak",
        &[
            "list",
            "--app",
            "--system",
            "--columns=application,origin,branch",
        ],
    );
    if result.exit_code != 0 {
        return Vec::new();
    }

    // Build remote name -> URL map (best-effort).
    let mut remote_urls: HashMap<String, String> = HashMap::new();
    let remotes_result = exec.run(
        "flatpak",
        &["remote-list", "--system", "--columns=name,url"],
    );
    if remotes_result.exit_code == 0 {
        for line in remotes_result.stdout.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                remote_urls.insert(parts[0].trim().to_string(), parts[1].trim().to_string());
            }
        }
    }

    let mut apps = Vec::new();
    for line in result.stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parts: Vec<&str> = trimmed.split('\t').collect();
        if parts.is_empty() {
            continue;
        }

        let app_id = parts[0].trim().to_string();
        let origin = parts
            .get(1)
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let branch = parts
            .get(2)
            .map(|s| s.trim().to_string())
            .unwrap_or_default();

        let remote = origin.clone();
        let remote_url = remote_urls.get(&origin).cloned().unwrap_or_default();

        apps.push(FlatpakApp {
            app_id,
            origin,
            branch,
            remote,
            remote_url,
            include: true,
            ..Default::default()
        });
    }
    apps
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;
    use inspectah_core::traits::executor::ExecResult;
    use inspectah_core::traits::progress::NullProgress;
    use inspectah_core::types::os::OsRelease;
    use inspectah_core::types::system::SourceSystem;

    fn fixture(name: &str) -> String {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let workspace_root = std::path::Path::new(manifest_dir)
            .parent()
            .and_then(|p| p.parent())
            .unwrap_or(std::path::Path::new(manifest_dir));
        let path = workspace_root
            .join("testdata/fixtures/containers")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()))
    }

    fn test_source_system() -> SourceSystem {
        SourceSystem::PackageBased {
            os_release: OsRelease {
                id: "rhel".to_string(),
                version_id: "9.4".to_string(),
                ..Default::default()
            },
        }
    }

    // -----------------------------------------------------------------------
    // Quadlet tests
    // -----------------------------------------------------------------------

    #[test]
    fn quadlet_container_unit() {
        let content = fixture("webapp.container");
        let image = extract_quadlet_image(&content);
        assert_eq!(image, "registry.example.com/webapp:latest");

        let (ports, volumes) = extract_quadlet_ports_and_volumes(&content);
        assert_eq!(ports, vec!["8080:80"]);
        assert_eq!(volumes, vec!["/data:/app/data:Z"]);
    }

    #[test]
    fn quadlet_volume_unit() {
        let exec = MockExecutor::new()
            .with_dir("/etc/containers/systemd", vec!["webapp-data.volume"])
            .with_file(
                "/etc/containers/systemd/webapp-data.volume",
                &fixture("webapp-data.volume"),
            );

        let mut degraded = Vec::new();
        let units = scan_quadlet_dir(&exec, "/etc/containers/systemd", &mut degraded);

        assert_eq!(units.len(), 1);
        assert_eq!(units[0].name, "webapp-data.volume");
        assert!(degraded.is_empty());
    }

    #[test]
    fn quadlet_network_unit() {
        let network_content = "[Network]\nSubnet=10.89.0.0/24\nGateway=10.89.0.1\n";
        let exec = MockExecutor::new()
            .with_dir("/etc/containers/systemd", vec!["app.network"])
            .with_file("/etc/containers/systemd/app.network", network_content);

        let mut degraded = Vec::new();
        let units = scan_quadlet_dir(&exec, "/etc/containers/systemd", &mut degraded);

        assert_eq!(units.len(), 1);
        assert_eq!(units[0].name, "app.network");
        assert!(degraded.is_empty());
    }

    #[test]
    fn quadlet_all_extensions() {
        let exec = MockExecutor::new()
            .with_dir(
                "/etc/containers/systemd",
                vec![
                    "app.container",
                    "data.volume",
                    "net.network",
                    "deploy.kube",
                    "group.pod",
                    "base.image",
                    "build.build",
                ],
            )
            .with_file(
                "/etc/containers/systemd/app.container",
                "[Container]\nImage=registry.example.com/app:v1\n",
            )
            .with_file("/etc/containers/systemd/data.volume", "[Volume]\n")
            .with_file("/etc/containers/systemd/net.network", "[Network]\n")
            .with_file("/etc/containers/systemd/deploy.kube", "[Kube]\n")
            .with_file("/etc/containers/systemd/group.pod", "[Pod]\n")
            .with_file("/etc/containers/systemd/base.image", "[Image]\n")
            .with_file("/etc/containers/systemd/build.build", "[Build]\n");

        let mut degraded = Vec::new();
        let units = scan_quadlet_dir(&exec, "/etc/containers/systemd", &mut degraded);

        assert_eq!(units.len(), 7, "all 7 extensions should be recognized");
        assert!(degraded.is_empty());
    }

    #[test]
    fn quadlet_image_extraction() {
        // Various Image= formats.
        assert_eq!(
            extract_quadlet_image("Image=quay.io/org/img:latest"),
            "quay.io/org/img:latest"
        );
        assert_eq!(
            extract_quadlet_image("[Container]\nImage=docker.io/library/nginx\n[Service]\n"),
            "docker.io/library/nginx"
        );
        assert_eq!(
            extract_quadlet_image("[Container]\nEnvironment=FOO=bar\n"),
            ""
        );
        assert_eq!(
            extract_quadlet_image("Image= "),
            "",
            "whitespace-only value should be empty"
        );
    }

    #[test]
    fn quadlet_system_dir_not_found() {
        let exec = MockExecutor::new();
        // /etc/containers/systemd not registered -> NotFound -> silent skip.
        let mut degraded = Vec::new();
        let units = scan_quadlet_dir(&exec, "/etc/containers/systemd", &mut degraded);

        assert!(units.is_empty());
        assert!(degraded.is_empty(), "NotFound should be a silent skip");
    }

    #[test]
    fn quadlet_system_dir_permission_denied() {
        let exec = MockExecutor::new().with_dir_error(
            "/etc/containers/systemd",
            std::io::ErrorKind::PermissionDenied,
        );

        let mut degraded = Vec::new();
        let units = scan_quadlet_dir(&exec, "/etc/containers/systemd", &mut degraded);

        assert!(units.is_empty());
        assert!(
            !degraded.is_empty(),
            "PermissionDenied should produce a Degraded reason"
        );
        assert!(degraded[0].contains("Permission denied"));
    }

    #[test]
    fn user_quadlet_dirs_discovery() {
        let passwd = "\
root:x:0:0:root:/root:/bin/bash
daemon:x:1:1:daemon:/usr/sbin:/usr/sbin/nologin
alice:x:1000:1000:Alice:/home/alice:/bin/bash
bob:x:1001:1001:Bob:/home/bob:/bin/zsh
nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin
";
        let exec = MockExecutor::new().with_file("/etc/passwd", passwd);

        let dirs = user_quadlet_dirs(&exec);

        assert_eq!(dirs.len(), 2);
        assert_eq!(dirs[0], "/home/alice/.config/containers/systemd");
        assert_eq!(dirs[1], "/home/bob/.config/containers/systemd");
    }

    // -----------------------------------------------------------------------
    // Compose tests
    // -----------------------------------------------------------------------

    #[test]
    fn compose_image_extraction_2space() {
        let yaml = "\
version: '3.8'

services:
  web:
    image: nginx:1.25
  db:
    image: postgres:16
";
        let result = extract_compose_images(yaml);
        assert_eq!(result.services.len(), 2);
        assert_eq!(result.services[0].service, "web");
        assert_eq!(result.services[0].image, "nginx:1.25");
        assert_eq!(result.services[1].service, "db");
        assert_eq!(result.services[1].image, "postgres:16");
        assert!(
            !result.had_anomalies,
            "valid YAML should not have anomalies"
        );
    }

    #[test]
    fn compose_image_extraction_4space() {
        let yaml = "\
services:
    web:
        image: nginx:alpine
    api:
        image: node:20
";
        let result = extract_compose_images(yaml);
        assert_eq!(result.services.len(), 2);
        assert_eq!(result.services[0].image, "nginx:alpine");
        assert_eq!(result.services[1].image, "node:20");
        assert!(!result.had_anomalies);
    }

    #[test]
    fn compose_image_extraction_tab() {
        let yaml = "services:\n\tweb:\n\t\timage: httpd:2.4\n\tdb:\n\t\timage: mariadb:11\n";
        let result = extract_compose_images(yaml);
        assert_eq!(result.services.len(), 2);
        assert_eq!(result.services[0].image, "httpd:2.4");
        assert_eq!(result.services[1].image, "mariadb:11");
        assert!(!result.had_anomalies);
    }

    #[test]
    fn compose_no_services_block() {
        let yaml = "\
version: '3.8'
networks:
  default:
    driver: bridge
";
        let result = extract_compose_images(yaml);
        assert!(
            result.services.is_empty(),
            "no services block should yield empty"
        );
        assert!(!result.had_anomalies);
    }

    #[test]
    fn compose_malformed_yaml_degraded() {
        // Malformed YAML from fixture -- indentation errors cause compose
        // sub-keys (ports, volumes) to appear at service-indent level.
        // The parser does best-effort extraction AND signals the anomaly
        // so the caller can report Degraded status.
        let content = fixture("compose-malformed.yaml");
        let exec = MockExecutor::new()
            .with_dir("/opt", vec!["compose-malformed.yaml"])
            .with_file("/opt/compose-malformed.yaml", &content);

        let mut hints = Vec::new();
        let mut degraded = Vec::new();
        let files = find_compose_files(&exec, "/opt", &mut hints, &mut degraded);

        // File should be discovered even when malformed.
        assert_eq!(files.len(), 1, "malformed YAML must still be discovered");
        // Best-effort image extraction: the "web" service with "image: nginx:latest"
        // is structurally valid within the malformed file, so the extractor should
        // find it despite the broken "ports"/"volumes" indentation below.
        assert!(
            files[0].images.iter().any(|s| s.image == "nginx:latest"),
            "best-effort extraction should find nginx:latest, got: {:?}",
            files[0].images
        );
        // Structural anomalies (sub-keys at service level) produce a
        // degraded reason so the inspector reports Degraded status.
        assert!(
            !degraded.is_empty(),
            "malformed YAML must produce a Degraded reason"
        );
        assert!(
            degraded
                .iter()
                .any(|d| d.contains("structural YAML issues")),
            "degraded reason should mention structural issues, got: {:?}",
            degraded
        );
    }

    #[test]
    fn compose_valid_but_unsupported_yaml_degraded() {
        // YAML with anchors/aliases that our simple parser cannot handle.
        let content = fixture("compose-anchors.yaml");
        let exec = MockExecutor::new()
            .with_dir("/opt", vec!["compose-anchors.yaml"])
            .with_file("/opt/compose-anchors.yaml", &content);

        let mut hints = Vec::new();
        let mut degraded = Vec::new();
        let files = find_compose_files(&exec, "/opt", &mut hints, &mut degraded);

        assert_eq!(files.len(), 1);
        assert!(
            !degraded.is_empty(),
            "YAML anchors/aliases should produce a Degraded reason"
        );
        assert!(degraded[0].contains("anchors"));
    }

    #[test]
    fn compose_file_discovery() {
        let exec = MockExecutor::new()
            .with_dir("/opt", vec!["myapp"])
            .with_dir("/opt/myapp", vec!["docker-compose.yml"])
            .with_file(
                "/opt/myapp/docker-compose.yml",
                "services:\n  web:\n    image: nginx\n",
            )
            .with_dir("/srv", vec!["deploy"])
            .with_dir("/srv/deploy", vec!["compose.yaml"])
            .with_file(
                "/srv/deploy/compose.yaml",
                "services:\n  api:\n    image: node:20\n",
            );

        let mut hints = Vec::new();
        let mut degraded = Vec::new();

        let mut all_files = Vec::new();
        for search_dir in COMPOSE_SEARCH_DIRS {
            let root = format!("/{search_dir}");
            let files = find_compose_files(&exec, &root, &mut hints, &mut degraded);
            all_files.extend(files);
        }

        assert_eq!(
            all_files.len(),
            2,
            "should find compose files in /opt and /srv"
        );
    }

    #[test]
    fn compose_env_secret_redaction_hint() {
        let yaml = "\
services:
  db:
    image: postgres:16
    environment:
      POSTGRES_PASSWORD: hunter2
      POSTGRES_USER: webapp
";
        let mut hints = Vec::new();
        scan_compose_env_secrets(yaml, "/opt/compose.yaml", &mut hints);

        assert!(
            !hints.is_empty(),
            "PASSWORD env var should produce a RedactionHint"
        );
        assert!(hints[0].reason.contains("PASSWORD"));
    }

    // -----------------------------------------------------------------------
    // Podman tests
    // -----------------------------------------------------------------------

    #[test]
    fn podman_ps_and_inspect() {
        let ps_json = fixture("podman-ps.json");
        let inspect_json = fixture("podman-inspect.json");

        let exec = MockExecutor::new()
            .with_command(
                "which podman",
                ExecResult {
                    stdout: "/usr/bin/podman".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "podman ps --format json",
                ExecResult {
                    stdout: ps_json,
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "podman inspect abc123def456",
                ExecResult {
                    stdout: inspect_json,
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let mut hints = Vec::new();
        let mut degraded = Vec::new();
        let (containers, warnings) = query_podman_containers(&exec, &mut hints, &mut degraded);

        assert!(warnings.is_empty());
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].id, "abc123def456");
        assert_eq!(containers[0].name, "webapp");
        assert_eq!(containers[0].image, "docker.io/library/nginx:latest");
        assert_eq!(containers[0].status, "running");
        assert!(containers[0].inspect_data);
    }

    #[test]
    fn podman_ps_only_fallback() {
        let ps_json = fixture("podman-ps.json");

        let exec = MockExecutor::new()
            .with_command(
                "which podman",
                ExecResult {
                    stdout: "/usr/bin/podman".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "podman ps --format json",
                ExecResult {
                    stdout: ps_json,
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "podman inspect abc123def456",
                ExecResult {
                    exit_code: 1,
                    stderr: "Error: no such container".into(),
                    ..Default::default()
                },
            );

        let mut hints = Vec::new();
        let mut degraded = Vec::new();
        let (containers, warnings) = query_podman_containers(&exec, &mut hints, &mut degraded);

        assert!(warnings.is_empty());
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].id, "abc123def456");
        // ps-only: inspect_data should be false (default).
        assert!(!containers[0].inspect_data);
    }

    #[test]
    fn podman_not_installed_skips_silently() {
        // Podman not installed (which podman fails) — warning-only, not
        // Degraded. The inspector succeeds with quadlet/compose/flatpak
        // data even when podman is absent.
        let exec = MockExecutor::new().with_command(
            "which podman",
            ExecResult {
                exit_code: 1,
                stderr: "podman not found".into(),
                ..Default::default()
            },
        );

        let mut hints = Vec::new();
        let mut degraded = Vec::new();
        let (containers, warnings) = query_podman_containers(&exec, &mut hints, &mut degraded);

        assert!(containers.is_empty());
        assert_eq!(warnings.len(), 1, "should warn when podman not installed");
        assert!(warnings[0].message.contains("podman not installed"));
        assert!(
            degraded.is_empty(),
            "podman not installed is warning-only, not Degraded"
        );
    }

    #[test]
    fn podman_installed_but_ps_fails_is_degraded() {
        // Podman IS installed (which podman succeeds) but podman ps fails
        // — this is a real failure and should produce Degraded.
        let exec = MockExecutor::new()
            .with_command(
                "which podman",
                ExecResult {
                    stdout: "/usr/bin/podman".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "podman ps --format json",
                ExecResult {
                    exit_code: 1,
                    stderr: "Error: cannot connect to Podman".into(),
                    ..Default::default()
                },
            );

        let mut hints = Vec::new();
        let mut degraded = Vec::new();
        let (containers, warnings) = query_podman_containers(&exec, &mut hints, &mut degraded);

        assert!(containers.is_empty());
        assert_eq!(warnings.len(), 1, "should warn when podman ps fails");
        assert!(warnings[0].message.contains("podman ps failed"));
        assert!(
            !degraded.is_empty(),
            "podman installed but ps failing must produce Degraded"
        );
        assert!(degraded[0].contains("podman ps failed"));
    }

    #[test]
    fn podman_inspect_mounts() {
        let json_str = r#"[
          {
            "Type": "bind",
            "Source": "/data",
            "Destination": "/app/data",
            "Mode": "rw",
            "RW": true
          },
          {
            "Type": "volume",
            "Source": "db-vol",
            "Destination": "/var/lib/db",
            "Mode": "ro",
            "RW": false
          }
        ]"#;

        let val: serde_json::Value = serde_json::from_str(json_str).expect("valid JSON");
        let mounts = parse_mounts(Some(&val));

        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0].mount_type, "bind");
        assert_eq!(mounts[0].source, "/data");
        assert_eq!(mounts[0].destination, "/app/data");
        assert!(mounts[0].rw);
        assert_eq!(mounts[1].mount_type, "volume");
        assert!(!mounts[1].rw);
    }

    #[test]
    fn podman_inspect_restart_policy() {
        let json_str = r#"{
          "HostConfig": {
            "RestartPolicy": {
              "Name": "on-failure",
              "MaximumRetryCount": 3
            }
          }
        }"#;

        let val: serde_json::Value = serde_json::from_str(json_str).expect("valid JSON");
        let policy = extract_restart_policy(&val);
        assert_eq!(policy, "on-failure");
    }

    #[test]
    fn podman_json_parse_error() {
        let exec = MockExecutor::new()
            .with_command(
                "which podman",
                ExecResult {
                    stdout: "/usr/bin/podman".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "podman ps --format json",
                ExecResult {
                    stdout: "not valid json{{{".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let mut hints = Vec::new();
        let mut degraded = Vec::new();
        let (containers, _warnings) = query_podman_containers(&exec, &mut hints, &mut degraded);

        assert!(containers.is_empty());
        assert!(
            !degraded.is_empty(),
            "malformed JSON should produce a Degraded reason"
        );
        assert!(degraded[0].contains("JSON parse error"));
    }

    #[test]
    fn podman_env_secret_redaction_hint() {
        let inspect_json = r#"[{
          "Id": "abc123",
          "Name": "test",
          "ImageName": "nginx:latest",
          "State": {"Status": "running"},
          "Config": {
            "Env": [
              "PATH=/usr/bin",
              "API_TOKEN=secret123"
            ]
          },
          "Mounts": [],
          "NetworkSettings": {"Networks": {}, "Ports": {}},
          "HostConfig": {"RestartPolicy": {"Name": ""}}
        }]"#;

        let data: Vec<serde_json::Value> = serde_json::from_str(inspect_json).expect("valid JSON");
        let mut hints = Vec::new();
        let _containers = parse_podman_inspect(&data, &mut hints);

        assert!(
            !hints.is_empty(),
            "API_TOKEN should produce a RedactionHint"
        );
        assert!(hints[0].reason.contains("TOKEN"));
    }

    // -----------------------------------------------------------------------
    // Flatpak tests
    // -----------------------------------------------------------------------

    #[test]
    fn flatpak_apps_detected() {
        let exec = MockExecutor::new()
            .with_command(
                "which flatpak",
                ExecResult {
                    stdout: "/usr/bin/flatpak".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "flatpak list --app --system --columns=application,origin,branch",
                ExecResult {
                    stdout: "\
org.mozilla.firefox\tflathub\tstable
org.gnome.Calculator\tfedora\tstable
com.visualstudio.code\tflathub\tstable
"
                    .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "flatpak remote-list --system --columns=name,url",
                ExecResult {
                    stdout: "flathub\thttps://dl.flathub.org/repo/\nfedora\thttps://flatpaks.fedoraproject.org/\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let apps = detect_flatpak_apps(&exec);

        assert_eq!(apps.len(), 3);
        assert_eq!(apps[0].app_id, "org.mozilla.firefox");
        assert_eq!(apps[0].origin, "flathub");
        assert_eq!(apps[0].branch, "stable");
        assert_eq!(apps[0].remote, "flathub");
        assert_eq!(apps[0].remote_url, "https://dl.flathub.org/repo/");
        assert_eq!(apps[1].app_id, "org.gnome.Calculator");
        assert_eq!(apps[1].origin, "fedora");
        assert_eq!(apps[1].remote_url, "https://flatpaks.fedoraproject.org/");
    }

    #[test]
    fn flatpak_not_installed() {
        let exec = MockExecutor::new().with_command(
            "which flatpak",
            ExecResult {
                exit_code: 1,
                stderr: "flatpak not found".into(),
                ..Default::default()
            },
        );

        let apps = detect_flatpak_apps(&exec);
        assert!(apps.is_empty(), "no flatpak should yield empty");
    }

    #[test]
    fn flatpak_remotes() {
        let exec = MockExecutor::new()
            .with_command(
                "which flatpak",
                ExecResult {
                    stdout: "/usr/bin/flatpak".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "flatpak list --app --system --columns=application,origin,branch",
                ExecResult {
                    stdout: "org.example.App\tcustom-remote\tstable\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "flatpak remote-list --system --columns=name,url",
                ExecResult {
                    stdout: "custom-remote\thttps://example.org/repo/\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let apps = detect_flatpak_apps(&exec);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].remote, "custom-remote");
        assert_eq!(apps[0].remote_url, "https://example.org/repo/");
    }

    // -----------------------------------------------------------------------
    // Integration / full-inspector tests
    // -----------------------------------------------------------------------

    #[test]
    fn empty_system_no_containers() {
        // All empty -> Complete, not Degraded.
        let exec = MockExecutor::new()
            .with_command(
                "which podman",
                ExecResult {
                    stdout: "/usr/bin/podman".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "podman ps --format json",
                ExecResult {
                    stdout: "[]".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "which flatpak",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            );

        let inspector = ContainersInspector::new();
        let source = test_source_system();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        assert!(
            result.is_ok(),
            "empty system should be Complete, not Degraded"
        );

        if let Ok(output) = result {
            if let SectionData::Containers(section) = output.section {
                assert!(section.quadlet_units.is_empty());
                assert!(section.compose_files.is_empty());
                assert!(section.running_containers.is_empty());
                assert!(section.flatpak_apps.is_empty());
            } else {
                panic!("expected Containers section");
            }
        }
    }

    // -----------------------------------------------------------------------
    // Glob matching
    // -----------------------------------------------------------------------

    #[test]
    fn glob_matching() {
        assert!(match_glob("docker-compose*.yml", "docker-compose.yml"));
        assert!(match_glob("docker-compose*.yml", "docker-compose-prod.yml"));
        assert!(!match_glob("docker-compose*.yml", "docker-compose.yaml"));
        assert!(match_glob("compose*.yaml", "compose.yaml"));
        assert!(match_glob("compose*.yaml", "compose-dev.yaml"));
        assert!(!match_glob("compose*.yaml", "docker-compose.yaml"));
        assert!(match_glob("*.container", "webapp.container"));
        assert!(!match_glob("*.container", "webapp.volume"));
    }

    // -----------------------------------------------------------------------
    // include: true collector defaults
    // -----------------------------------------------------------------------

    #[test]
    fn collected_quadlets_have_include_true() {
        let exec = MockExecutor::new()
            .with_dir("/etc/containers/systemd", vec!["app.container"])
            .with_file(
                "/etc/containers/systemd/app.container",
                "[Container]\nImage=quay.io/org/app:v1\n",
            );

        let mut degraded = Vec::new();
        let units = scan_quadlet_dir(&exec, "/etc/containers/systemd", &mut degraded);
        assert_eq!(units.len(), 1);
        assert!(
            units[0].include,
            "collected QuadletUnit should have include: true"
        );
    }

    #[test]
    fn collected_compose_files_have_include_true() {
        let exec = MockExecutor::new()
            .with_dir("/opt", vec!["docker-compose.yml"])
            .with_file(
                "/opt/docker-compose.yml",
                "services:\n  web:\n    image: nginx\n",
            );

        let mut hints = Vec::new();
        let mut degraded = Vec::new();
        let files = find_compose_files(&exec, "/opt", &mut hints, &mut degraded);
        assert_eq!(files.len(), 1);
        assert!(
            files[0].include,
            "collected ComposeFile should have include: true"
        );
    }

    #[test]
    fn collected_flatpak_apps_have_include_true() {
        let exec = MockExecutor::new()
            .with_command(
                "which flatpak",
                ExecResult {
                    stdout: "/usr/bin/flatpak".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "flatpak list --app --system --columns=application,origin,branch",
                ExecResult {
                    stdout: "org.mozilla.firefox\tflathub\tstable\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "flatpak remote-list --system --columns=name,url",
                ExecResult {
                    stdout: "flathub\thttps://dl.flathub.org/repo/\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let apps = detect_flatpak_apps(&exec);
        assert_eq!(apps.len(), 1);
        assert!(
            apps[0].include,
            "collected FlatpakApp should have include: true"
        );
    }

    #[test]
    fn collected_running_containers_have_include_true() {
        let ps_json = fixture("podman-ps.json");
        let inspect_json = fixture("podman-inspect.json");

        let exec = MockExecutor::new()
            .with_command(
                "which podman",
                ExecResult {
                    stdout: "/usr/bin/podman".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "podman ps --format json",
                ExecResult {
                    stdout: ps_json.clone(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "podman inspect abc123def456",
                ExecResult {
                    stdout: inspect_json,
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let mut hints = Vec::new();
        let mut degraded = Vec::new();
        let (containers, _) = query_podman_containers(&exec, &mut hints, &mut degraded);
        assert!(!containers.is_empty());
        for c in &containers {
            assert!(
                c.include,
                "collected RunningContainer '{}' should have include: true",
                c.name
            );
        }

        // Also verify ps-only fallback path sets include: true
        let exec2 = MockExecutor::new()
            .with_command(
                "which podman",
                ExecResult {
                    stdout: "/usr/bin/podman".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "podman ps --format json",
                ExecResult {
                    stdout: ps_json,
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "podman inspect abc123def456",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            );

        let mut hints2 = Vec::new();
        let mut degraded2 = Vec::new();
        let (containers2, _) = query_podman_containers(&exec2, &mut hints2, &mut degraded2);
        assert!(!containers2.is_empty());
        for c in &containers2 {
            assert!(
                c.include,
                "ps-only RunningContainer '{}' should have include: true",
                c.name
            );
        }
    }
}
