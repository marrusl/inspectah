use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::config::{ConfigCategory, ConfigFileEntry, ConfigFileKind};
use inspectah_core::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection, PipPackage};
use inspectah_core::types::redaction::{Confidence, RedactionHint};
use inspectah_core::types::system::SourceSystem;
use inspectah_core::types::warnings::Warning;
use std::collections::HashMap;
use std::path::Path;

/// Directories to prune during recursive walks (build artifacts, caches).
const PRUNE_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "__pycache__",
    ".tox",
    ".venv",
    "venv",
    ".cache",
    ".npm",
    "target",
    "build",
    "dist",
    ".eggs",
    "vendor",
];

/// .env file names to collect.
const ENV_FILE_NAMES: &[&str] = &[
    ".env",
    ".env.local",
    ".env.production",
    ".env.staging",
    ".env.development",
];

/// Secret-like environment variable name fragments that trigger redaction hints.
const SECRET_PATTERNS: &[&str] = &[
    "PASSWORD",
    "SECRET",
    "TOKEN",
    "KEY",
    "CREDENTIAL",
    "API_KEY",
    "ACCESS_KEY",
    "DATABASE_URL",
    "REDIS_URL",
];

/// Scan roots for non-RPM software.
const SCAN_ROOTS: &[&str] = &["/opt", "/srv", "/usr/local"];

/// Ostree-internal /var paths to filter out.
const OSTREE_VAR_INTERNALS: &[&str] = &[
    "var/lib/ostree",
    "var/lib/rpm-ostree",
    "var/lib/flatpak",
];

/// Inspects non-RPM software: ELF binaries in /opt, /srv, /usr/local,
/// Python venvs, pip/npm/gem packages, .env files, and git repos.
pub struct NonRpmInspector;

impl NonRpmInspector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NonRpmInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl Inspector for NonRpmInspector {
    fn id(&self) -> InspectorId {
        InspectorId::NonRpmSoftware
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(&self, ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError> {
        // Wave 2 ordering contract: require rpm_state presence.
        // None means RPM inspector failed entirely.
        // Some(_) means proceed — but we deliberately do NOT call any rpm_state methods.
        if ctx.rpm_state.is_none() {
            return Err(InspectorError::Failed {
                reason: "RPM state unavailable — non-RPM inspector requires RPM phase to complete"
                    .to_string(),
            });
        }

        let exec = ctx.executor;
        let mut warnings = Vec::new();
        let mut redaction_hints = Vec::new();
        let mut section = NonRpmSoftwareSection {
            items: Vec::new(),
            env_files: Vec::new(),
        };

        let is_ostree = matches!(
            ctx.source_system,
            SourceSystem::RpmOstree { .. } | SourceSystem::Bootc { .. }
        );

        // Probe for readelf availability.
        let has_readelf = probe_command(exec, "readelf");
        let has_file = if has_readelf {
            probe_command(exec, "file")
        } else {
            false
        };

        if !has_readelf {
            warnings.push(Warning {
                inspector: "non_rpm_software".to_string(),
                message: "readelf not available (rc=127) — ELF binary classification skipped. \
                          Install binutils in the inspectah container image."
                    .to_string(),
                ..Default::default()
            });
        }

        // Scan directories for ELF binaries.
        scan_dirs(exec, &mut section, has_readelf, has_file);

        // Scan Python venvs.
        scan_python_venvs(exec, &mut section, &mut warnings);

        // Scan pip packages (system-level dist-info).
        scan_pip_packages(exec, &mut section, is_ostree);

        // Scan npm packages (package-lock.json).
        scan_npm_packages(exec, &mut section, is_ostree);

        // Scan gem packages (Gemfile.lock).
        scan_gem_packages(exec, &mut section, is_ostree);

        // Collect .env files.
        collect_env_files(exec, &mut section, &mut redaction_hints);

        // Collect git repos.
        collect_git_repos(exec, &mut section, &mut redaction_hints);

        // Filter ostree-internal /var paths.
        if is_ostree {
            filter_ostree_var_paths(&mut section);
        }

        // Deduplicate items by path, keeping highest confidence.
        deduplicate_items(&mut section);

        // Return Degraded if readelf was unavailable but we still got partial data.
        if !has_readelf
            && (!section.items.is_empty() || !section.env_files.is_empty())
        {
            return Err(InspectorError::Degraded {
                reason: "readelf unavailable — ELF binary classification skipped".to_string(),
                partial: Box::new(InspectorOutput {
                    section: SectionData::NonRpmSoftware(section),
                    warnings,
                    redaction_hints,
                }),
            });
        }

        Ok(InspectorOutput {
            section: SectionData::NonRpmSoftware(section),
            warnings,
            redaction_hints,
        })
    }
}

// ---------------------------------------------------------------------------
// Tool availability
// ---------------------------------------------------------------------------

/// Check if a command is available (exit code != 127).
fn probe_command(exec: &dyn Executor, name: &str) -> bool {
    let result = exec.run(name, &["--version"]);
    result.exit_code != 127
}

// ---------------------------------------------------------------------------
// ELF binary scanning
// ---------------------------------------------------------------------------

/// Result of classifying a binary via readelf.
#[derive(Debug)]
struct BinaryClassification {
    lang: String,
    is_static: bool,
    shared_libs: Vec<String>,
}

/// Classify an ELF binary using readelf section headers and dynamic info.
fn classify_binary(exec: &dyn Executor, path: &str) -> Option<BinaryClassification> {
    let sections = exec.run("readelf", &["-S", path]);
    if sections.exit_code != 0 {
        return None;
    }

    let is_go = sections.stdout.contains(".note.go.buildid")
        || sections.stdout.contains(".gopclntab");
    let is_rust = sections.stdout.contains(".rustc");

    let dynamic = exec.run("readelf", &["-d", path]);
    let dynamic_output = if dynamic.exit_code == 0 {
        &dynamic.stdout
    } else {
        ""
    };

    let is_static = dynamic_output.contains("no dynamic section")
        || dynamic_output.is_empty()
        || dynamic.exit_code != 0;

    let mut shared_libs = Vec::new();
    for line in dynamic_output.lines() {
        if line.contains("(NEEDED)") {
            if let Some(start) = line.find('[') {
                if let Some(end) = line[start..].find(']') {
                    shared_libs.push(line[start + 1..start + end].to_string());
                }
            }
        }
    }

    let lang = if is_go {
        "go".to_string()
    } else if is_rust {
        "rust".to_string()
    } else {
        "c/c++".to_string()
    };

    Some(BinaryClassification {
        lang,
        is_static,
        shared_libs,
    })
}

/// Extract version from binary using `strings` output.
fn extract_version(exec: &dyn Executor, path: &str) -> String {
    let result = exec.run("strings", &[path]);
    if result.exit_code != 0 {
        return String::new();
    }
    extract_version_from_text(&result.stdout)
}

/// Parse version patterns from text (strings output).
fn extract_version_from_text(text: &str) -> String {
    for line in text.lines() {
        // Try version=1.2.3 or version: 1.2.3 pattern first.
        if line.to_lowercase().contains("version") {
            if let Some(v) = extract_semver(line) {
                return v;
            }
        }
        // Try go1.21.5 pattern.
        if let Some(pos) = line.find("go") {
            if let Some(v) = extract_semver(&line[pos + 2..]) {
                return v;
            }
        }
    }
    String::new()
}

/// Extract a semver-like version (X.Y.Z or X.Y) from text.
fn extract_semver(text: &str) -> Option<String> {
    let mut start = None;
    let mut dots = 0;

    for (i, ch) in text.char_indices() {
        match ch {
            '0'..='9' => {
                if start.is_none() {
                    start = Some(i);
                }
            }
            '.' if start.is_some() && dots < 2 => {
                dots += 1;
            }
            _ => {
                if let Some(s) = start {
                    if dots >= 1 {
                        let candidate = &text[s..i];
                        let candidate = candidate.trim_end_matches('.');
                        if candidate.contains('.') {
                            return Some(candidate.to_string());
                        }
                    }
                }
                start = None;
                dots = 0;
            }
        }
    }
    // Check tail.
    if let Some(s) = start {
        if dots >= 1 {
            let candidate = &text[s..];
            let candidate = candidate.trim_end_matches('.');
            if candidate.contains('.') {
                return Some(candidate.to_string());
            }
        }
    }
    None
}

/// Check if a file is an ELF binary using the `file` command.
fn is_elf_binary(exec: &dyn Executor, path: &str) -> bool {
    let result = exec.run("file", &["-b", path]);
    result.exit_code == 0 && result.stdout.contains("ELF")
}

/// Scan /opt, /srv, /usr/local for ELF binaries and classify them.
fn scan_dirs(
    exec: &dyn Executor,
    section: &mut NonRpmSoftwareSection,
    has_readelf: bool,
    has_file: bool,
) {
    for root in SCAN_ROOTS {
        if exec.read_dir(Path::new(root)).is_err() {
            continue; // silent skip if dir doesn't exist
        }
        walk_for_elf_binaries(exec, root, section, has_readelf, has_file);
    }
}

/// Recursively walk a directory tree looking for ELF binaries.
fn walk_for_elf_binaries(
    exec: &dyn Executor,
    dir: &str,
    section: &mut NonRpmSoftwareSection,
    has_readelf: bool,
    has_file: bool,
) {
    let entries = match exec.read_dir(Path::new(dir)) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in &entries {
        let child = format!("{}/{}", dir, entry);

        // Check if this is a directory.
        if exec.read_dir(Path::new(&child)).is_ok() {
            if PRUNE_DIRS.contains(&entry.as_str()) {
                continue;
            }
            walk_for_elf_binaries(exec, &child, section, has_readelf, has_file);
            continue;
        }

        // Try to classify as ELF binary.
        let rel_path = child.trim_start_matches('/').to_string();

        if has_readelf {
            if let Some(bc) = classify_binary(exec, &child) {
                let version = extract_version(exec, &child);
                section.items.push(NonRpmItem {
                    path: rel_path,
                    name: entry.clone(),
                    method: format!("readelf ({})", bc.lang),
                    confidence: "high".to_string(),
                    lang: bc.lang,
                    r#static: bc.is_static,
                    shared_libs: bc.shared_libs,
                    version,
                    ..Default::default()
                });
                continue;
            }
        }

        // Fall back to `file` command for ELF detection.
        if has_file && is_elf_binary(exec, &child) {
            let version = extract_version(exec, &child);
            section.items.push(NonRpmItem {
                path: rel_path,
                name: entry.clone(),
                method: "file scan".to_string(),
                confidence: "low".to_string(),
                version,
                ..Default::default()
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Python venv scanning
// ---------------------------------------------------------------------------

/// Scan for Python virtual environments (pyvenv.cfg) under scan roots.
fn scan_python_venvs(
    exec: &dyn Executor,
    section: &mut NonRpmSoftwareSection,
    warnings: &mut Vec<Warning>,
) {
    let mut pip_fail_count = 0;

    for root in SCAN_ROOTS {
        let venvs = find_venvs(exec, root);
        for venv in &venvs {
            let packages = scan_venv_packages(exec, &venv.path);
            let pip_packages = if packages.is_empty() {
                // Try dist-info fallback.
                let fallback = scan_dist_info(exec, &venv.path);
                if fallback.is_empty() {
                    pip_fail_count += 1;
                }
                fallback
            } else {
                packages
            };

            let rel_path = venv.path.trim_start_matches('/').to_string();
            section.items.push(NonRpmItem {
                path: rel_path,
                name: Path::new(&venv.path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                method: "python venv".to_string(),
                confidence: "high".to_string(),
                system_site_packages: venv.system_site_packages,
                packages: pip_packages,
                ..Default::default()
            });
        }
    }

    if pip_fail_count > 0 {
        warnings.push(Warning {
            inspector: "non_rpm_software".to_string(),
            message: "pip list --path failed for venv(s) — package inventory may be \
                      incomplete (dist-info scan used as fallback)."
                .to_string(),
            ..Default::default()
        });
    }
}

struct VenvInfo {
    path: String,
    system_site_packages: bool,
}

/// Find pyvenv.cfg files under a root directory.
fn find_venvs(exec: &dyn Executor, root: &str) -> Vec<VenvInfo> {
    let mut results = Vec::new();
    find_venvs_walk(exec, root, &mut results);
    results
}

fn find_venvs_walk(exec: &dyn Executor, dir: &str, results: &mut Vec<VenvInfo>) {
    let entries = match exec.read_dir(Path::new(dir)) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in &entries {
        let child = format!("{}/{}", dir, entry);
        if entry == "pyvenv.cfg" {
            // Found a venv — parse its config.
            let system_sp = match exec.read_file(Path::new(&child)) {
                Ok(content) => content
                    .lines()
                    .any(|l| l.trim().eq_ignore_ascii_case("include-system-site-packages = true")),
                Err(_) => false,
            };
            results.push(VenvInfo {
                path: dir.to_string(),
                system_site_packages: system_sp,
            });
            return; // Don't recurse into venv dirs.
        }

        // Recurse into subdirs, but prune build artifacts.
        if exec.read_dir(Path::new(&child)).is_ok() && !PRUNE_DIRS.contains(&entry.as_str()) {
            find_venvs_walk(exec, &child, results);
        }
    }
}

/// Try `pip list --path` for a venv's site-packages.
fn scan_venv_packages(exec: &dyn Executor, venv_path: &str) -> Vec<PipPackage> {
    let sp_path = find_site_packages_path(exec, venv_path);
    if sp_path.is_empty() {
        return Vec::new();
    }

    let result = exec.run("pip", &["list", "--path", &sp_path, "--format", "json"]);
    if result.exit_code != 0 || result.stdout.trim().is_empty() {
        return Vec::new();
    }

    parse_pip_json(&result.stdout)
}

/// Find a site-packages directory under a venv root.
fn find_site_packages_path(exec: &dyn Executor, root: &str) -> String {
    let mut result = String::new();
    find_site_packages_walk(exec, root, &mut result);
    result
}

fn find_site_packages_walk(exec: &dyn Executor, dir: &str, result: &mut String) {
    if !result.is_empty() {
        return;
    }
    let entries = match exec.read_dir(Path::new(dir)) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in &entries {
        let child = format!("{}/{}", dir, entry);
        if exec.read_dir(Path::new(&child)).is_ok() {
            if entry == "site-packages" {
                *result = child;
                return;
            }
            find_site_packages_walk(exec, &child, result);
        }
    }
}

/// Parse pip list JSON output.
fn parse_pip_json(json_str: &str) -> Vec<PipPackage> {
    #[derive(serde::Deserialize)]
    struct PipEntry {
        name: String,
        version: String,
    }

    match serde_json::from_str::<Vec<PipEntry>>(json_str) {
        Ok(entries) => entries
            .into_iter()
            .map(|e| PipPackage {
                name: e.name,
                version: e.version,
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Scan dist-info directories inside a venv for package metadata.
fn scan_dist_info(exec: &dyn Executor, venv_path: &str) -> Vec<PipPackage> {
    let mut packages = Vec::new();
    scan_dist_info_walk(exec, venv_path, &mut packages);
    packages
}

fn scan_dist_info_walk(exec: &dyn Executor, dir: &str, packages: &mut Vec<PipPackage>) {
    let entries = match exec.read_dir(Path::new(dir)) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in &entries {
        let child = format!("{}/{}", dir, entry);
        if exec.read_dir(Path::new(&child)).is_ok() {
            if entry == "site-packages" {
                // Scan this site-packages for .dist-info dirs.
                if let Ok(sp_entries) = exec.read_dir(Path::new(&child)) {
                    for sp_entry in &sp_entries {
                        if sp_entry.ends_with(".dist-info") {
                            let (name, version) =
                                parse_dist_info_name(sp_entry.trim_end_matches(".dist-info"));
                            packages.push(PipPackage { name, version });
                        }
                    }
                }
            } else {
                scan_dist_info_walk(exec, &child, packages);
            }
        }
    }
}

/// Split "name-version" into (name, version).
fn parse_dist_info_name(s: &str) -> (String, String) {
    match s.rfind('-') {
        Some(idx) => (s[..idx].to_string(), s[idx + 1..].to_string()),
        None => (s.to_string(), String::new()),
    }
}

// ---------------------------------------------------------------------------
// pip system-level scanning
// ---------------------------------------------------------------------------

/// Scan system-level pip packages via dist-info directories.
fn scan_pip_packages(exec: &dyn Executor, section: &mut NonRpmSoftwareSection, is_ostree: bool) {
    let search_roots = &[
        "/usr/lib/python3",
        "/usr/lib64/python3",
        "/usr/local/lib/python3",
    ];

    for search_root in search_roots {
        let parent_dir = Path::new(search_root)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let prefix = Path::new(search_root)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let entries = match exec.read_dir(Path::new(&parent_dir)) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in &entries {
            if !entry.starts_with(&prefix) {
                continue;
            }
            let python_dir = format!("{}/{}", parent_dir, entry);
            let sp_dir = format!("{}/site-packages", python_dir);
            let sp_entries = match exec.read_dir(Path::new(&sp_dir)) {
                Ok(e) => e,
                Err(_) => continue,
            };

            for sp_entry in &sp_entries {
                if sp_entry.ends_with(".dist-info") {
                    let (name, version) =
                        parse_dist_info_name(sp_entry.trim_end_matches(".dist-info"));
                    let rel_path = sp_dir.trim_start_matches('/').to_string();

                    if is_ostree && rel_path.starts_with("var/") {
                        continue;
                    }

                    section.items.push(NonRpmItem {
                        path: rel_path.clone(),
                        name: name.clone(),
                        method: "pip dist-info".to_string(),
                        confidence: "medium".to_string(),
                        packages: vec![PipPackage { name, version }],
                        ..Default::default()
                    });
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// npm scanning
// ---------------------------------------------------------------------------

/// Scan for npm packages via package-lock.json files.
fn scan_npm_packages(exec: &dyn Executor, section: &mut NonRpmSoftwareSection, is_ostree: bool) {
    for root in SCAN_ROOTS {
        find_files_matching(exec, root, "package-lock.json", &mut |path| {
            let rel_path = path.trim_start_matches('/').to_string();
            if is_ostree && rel_path.starts_with("var/") {
                return;
            }

            if let Ok(content) = exec.read_file(Path::new(path)) {
                let packages = parse_package_lock(&content);
                for pkg in packages {
                    section.items.push(NonRpmItem {
                        path: rel_path.clone(),
                        name: pkg.name,
                        method: "npm lockfile".to_string(),
                        confidence: "high".to_string(),
                        version: pkg.version,
                        ..Default::default()
                    });
                }
            }
        });
    }
}

struct NpmPackage {
    name: String,
    version: String,
}

/// Parse package-lock.json to extract dependency names and versions.
fn parse_package_lock(content: &str) -> Vec<NpmPackage> {
    let mut packages = Vec::new();

    let parsed: serde_json::Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => return packages,
    };

    // lockfileVersion 3: packages map with "node_modules/..." keys.
    if let Some(pkgs) = parsed.get("packages").and_then(|p| p.as_object()) {
        for (key, value) in pkgs {
            if key.is_empty() {
                continue; // Skip root entry.
            }
            let name = key
                .strip_prefix("node_modules/")
                .unwrap_or(key)
                .to_string();
            let version = value
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            packages.push(NpmPackage { name, version });
        }
    }
    // lockfileVersion 1/2: dependencies map.
    else if let Some(deps) = parsed.get("dependencies").and_then(|d| d.as_object()) {
        for (name, value) in deps {
            let version = value
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            packages.push(NpmPackage {
                name: name.clone(),
                version,
            });
        }
    }

    packages
}

// ---------------------------------------------------------------------------
// gem scanning
// ---------------------------------------------------------------------------

/// Scan for gem packages via Gemfile.lock files.
fn scan_gem_packages(exec: &dyn Executor, section: &mut NonRpmSoftwareSection, is_ostree: bool) {
    for root in SCAN_ROOTS {
        find_files_matching(exec, root, "Gemfile.lock", &mut |path| {
            let rel_path = path.trim_start_matches('/').to_string();
            if is_ostree && rel_path.starts_with("var/") {
                return;
            }

            if let Ok(content) = exec.read_file(Path::new(path)) {
                let gems = parse_gemfile_lock(&content);
                for gem in gems {
                    section.items.push(NonRpmItem {
                        path: rel_path.clone(),
                        name: gem.name,
                        method: "gem lockfile".to_string(),
                        confidence: "high".to_string(),
                        version: gem.version,
                        ..Default::default()
                    });
                }
            }
        });
    }
}

struct GemPackage {
    name: String,
    version: String,
}

/// Parse Gemfile.lock to extract gem names and versions from the specs section.
fn parse_gemfile_lock(content: &str) -> Vec<GemPackage> {
    let mut gems = Vec::new();
    let mut in_specs = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "specs:" {
            in_specs = true;
            continue;
        }
        if in_specs {
            // Specs entries are indented with 4 spaces (top-level gems).
            // Sub-dependencies are indented with 6 spaces — skip them.
            if !line.starts_with("    ") || line.starts_with("      ") {
                if !trimmed.is_empty() && !line.starts_with(' ') {
                    in_specs = false; // Left the specs block.
                }
                continue;
            }
            // Parse "    name (version)"
            let parts: Vec<&str> = trimmed.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let name = parts[0].to_string();
                let version = parts[1]
                    .trim_start_matches('(')
                    .trim_end_matches(')')
                    .to_string();
                gems.push(GemPackage { name, version });
            }
        }
    }

    gems
}

// ---------------------------------------------------------------------------
// .env file collection
// ---------------------------------------------------------------------------

/// Collect .env files from scan roots.
fn collect_env_files(
    exec: &dyn Executor,
    section: &mut NonRpmSoftwareSection,
    redaction_hints: &mut Vec<RedactionHint>,
) {
    for root in SCAN_ROOTS {
        for env_name in ENV_FILE_NAMES {
            find_files_matching(exec, root, env_name, &mut |path| {
                let content = exec.read_file(Path::new(path)).unwrap_or_default();
                let rel_path = path.trim_start_matches('/').to_string();

                // Flag env file content for redaction.
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') {
                        continue;
                    }
                    if let Some(var_name) = trimmed.split('=').next() {
                        let upper = var_name.to_uppercase();
                        if SECRET_PATTERNS.iter().any(|p| upper.contains(p)) {
                            redaction_hints.push(RedactionHint {
                                path: rel_path.clone(),
                                reason: format!(
                                    "environment variable '{}' in .env file may contain a secret",
                                    var_name.trim()
                                ),
                                confidence: Some(Confidence::High),
                            });
                        }
                    }
                }

                section.env_files.push(ConfigFileEntry {
                    path: rel_path,
                    kind: ConfigFileKind::Unowned,
                    category: ConfigCategory::Environment,
                    content,
                    ..Default::default()
                });
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Git repo collection
// ---------------------------------------------------------------------------

/// Collect git repos by finding .git/config files.
fn collect_git_repos(
    exec: &dyn Executor,
    section: &mut NonRpmSoftwareSection,
    redaction_hints: &mut Vec<RedactionHint>,
) {
    for root in SCAN_ROOTS {
        find_git_configs(exec, root, section, redaction_hints);
    }
}

fn find_git_configs(
    exec: &dyn Executor,
    dir: &str,
    section: &mut NonRpmSoftwareSection,
    redaction_hints: &mut Vec<RedactionHint>,
) {
    let entries = match exec.read_dir(Path::new(dir)) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in &entries {
        let child = format!("{}/{}", dir, entry);
        if entry == ".git" {
            // Found a git repo — extract remote URL from config.
            let config_path = format!("{}/.git/config", dir);
            let remote_url = match exec.read_file(Path::new(&config_path)) {
                Ok(content) => extract_git_remote_url(&content),
                Err(_) => String::new(),
            };

            let rel_path = dir.trim_start_matches('/').to_string();

            // Flag if remote URL contains embedded credentials.
            if remote_url.contains('@') && remote_url.contains("://") {
                // Pattern: https://user:pass@host/...
                if let Some(proto_rest) = remote_url.split("://").nth(1) {
                    if proto_rest.contains('@') && proto_rest.contains(':') {
                        redaction_hints.push(RedactionHint {
                            path: format!("{}/config", rel_path),
                            reason: "git remote URL may contain embedded credentials".to_string(),
                            confidence: Some(Confidence::Medium),
                        });
                    }
                }
            }

            section.items.push(NonRpmItem {
                path: rel_path,
                name: Path::new(dir)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                method: "git repo".to_string(),
                confidence: "high".to_string(),
                git_remote: remote_url,
                ..Default::default()
            });
            continue; // Don't recurse into .git.
        }

        // Recurse into subdirs.
        if exec.read_dir(Path::new(&child)).is_ok() && !PRUNE_DIRS.contains(&entry.as_str()) {
            find_git_configs(exec, &child, section, redaction_hints);
        }
    }
}

/// Extract the remote "origin" URL from a git config file.
fn extract_git_remote_url(content: &str) -> String {
    let mut in_origin = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[remote \"origin\"]" {
            in_origin = true;
            continue;
        }
        if in_origin {
            if trimmed.starts_with('[') {
                break; // New section.
            }
            if let Some(url) = trimmed.strip_prefix("url = ") {
                return url.trim().to_string();
            }
        }
    }
    String::new()
}

// ---------------------------------------------------------------------------
// Post-processing
// ---------------------------------------------------------------------------

/// Filter ostree-internal /var paths from items.
fn filter_ostree_var_paths(section: &mut NonRpmSoftwareSection) {
    section.items.retain(|item| {
        !OSTREE_VAR_INTERNALS
            .iter()
            .any(|internal| item.path.starts_with(internal))
    });
}

/// Deduplicate items by path, keeping the highest-confidence entry.
fn deduplicate_items(section: &mut NonRpmSoftwareSection) {
    let confidence_rank = |c: &str| -> i32 {
        match c {
            "high" => 2,
            "medium" => 1,
            _ => 0,
        }
    };

    let mut seen: HashMap<String, (usize, i32)> = HashMap::new();
    let mut order = Vec::new();

    for (i, item) in section.items.iter().enumerate() {
        let rank = confidence_rank(&item.confidence);
        match seen.get(&item.path) {
            None => {
                seen.insert(item.path.clone(), (i, rank));
                order.push(item.path.clone());
            }
            Some(&(_, existing_rank)) => {
                if rank > existing_rank {
                    seen.insert(item.path.clone(), (i, rank));
                }
            }
        }
    }

    let items = std::mem::take(&mut section.items);
    section.items = order
        .iter()
        .filter_map(|path| {
            seen.get(path)
                .map(|&(idx, _)| items[idx].clone())
        })
        .collect();
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Recursively find files matching a specific filename under a root.
fn find_files_matching(
    exec: &dyn Executor,
    root: &str,
    filename: &str,
    handler: &mut impl FnMut(&str),
) {
    let entries = match exec.read_dir(Path::new(root)) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in &entries {
        let child = format!("{}/{}", root, entry);
        if entry == filename {
            handler(&child);
            continue;
        }
        if exec.read_dir(Path::new(&child)).is_ok() {
            if PRUNE_DIRS.contains(&entry.as_str()) {
                continue;
            }
            find_files_matching(exec, &child, filename, handler);
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;
    use inspectah_core::traits::executor::ExecResult;
    use inspectah_core::traits::inspector::{InspectionContext, Inspector, RpmState};
    use inspectah_core::types::os::OsRelease;
    use inspectah_core::types::system::SourceSystem;

    fn test_os_release() -> OsRelease {
        OsRelease {
            id: "rhel".to_string(),
            version_id: "9.4".to_string(),
            ..Default::default()
        }
    }

    fn test_source_system() -> SourceSystem {
        SourceSystem::PackageBased {
            os_release: test_os_release(),
        }
    }

    fn empty_rpm_state() -> RpmState {
        RpmState::default()
    }

    fn readelf_go_fixture() -> &'static str {
        include_str!("../../../testdata/fixtures/nonrpm/readelf-sections-go.txt")
    }

    fn readelf_rust_fixture() -> &'static str {
        include_str!("../../../testdata/fixtures/nonrpm/readelf-sections-rust.txt")
    }

    fn readelf_c_fixture() -> &'static str {
        include_str!("../../../testdata/fixtures/nonrpm/readelf-sections-c.txt")
    }

    fn readelf_dynamic_linked_fixture() -> &'static str {
        include_str!("../../../testdata/fixtures/nonrpm/readelf-dynamic-linked.txt")
    }

    fn readelf_dynamic_static_fixture() -> &'static str {
        include_str!("../../../testdata/fixtures/nonrpm/readelf-dynamic-static.txt")
    }

    fn strings_version_fixture() -> &'static str {
        include_str!("../../../testdata/fixtures/nonrpm/strings-version.txt")
    }

    fn pip_list_fixture() -> &'static str {
        include_str!("../../../testdata/fixtures/nonrpm/pip-list-output.txt")
    }

    fn package_lock_fixture() -> &'static str {
        include_str!("../../../testdata/fixtures/nonrpm/package-lock.json")
    }

    fn gemfile_lock_fixture() -> &'static str {
        include_str!("../../../testdata/fixtures/nonrpm/gemfile.lock")
    }

    fn env_file_fixture() -> &'static str {
        include_str!("../../../testdata/fixtures/nonrpm/env-file.txt")
    }

    fn git_config_fixture() -> &'static str {
        include_str!("../../../testdata/fixtures/nonrpm/git-config")
    }

    fn pyvenv_cfg_fixture() -> &'static str {
        include_str!("../../../testdata/fixtures/nonrpm/pyvenv.cfg")
    }

    // ---- Test 1: test_classify_binary_go ----

    #[test]
    fn test_classify_binary_go() {
        let exec = MockExecutor::new()
            .with_command(
                "readelf -S /opt/app/myapp",
                ExecResult {
                    stdout: readelf_go_fixture().to_string(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "readelf -d /opt/app/myapp",
                ExecResult {
                    stdout: readelf_dynamic_static_fixture().to_string(),
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let bc = classify_binary(&exec, "/opt/app/myapp").unwrap();
        assert_eq!(bc.lang, "go");
        assert!(bc.is_static, "Go binary should be static");
        assert!(bc.shared_libs.is_empty());
    }

    // ---- Test 2: test_classify_binary_rust ----

    #[test]
    fn test_classify_binary_rust() {
        let exec = MockExecutor::new()
            .with_command(
                "readelf -S /opt/app/myapp",
                ExecResult {
                    stdout: readelf_rust_fixture().to_string(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "readelf -d /opt/app/myapp",
                ExecResult {
                    stdout: readelf_dynamic_linked_fixture().to_string(),
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let bc = classify_binary(&exec, "/opt/app/myapp").unwrap();
        assert_eq!(bc.lang, "rust");
        assert!(!bc.is_static, "Rust binary with NEEDED entries should be dynamic");
        assert!(
            bc.shared_libs.contains(&"libc.so.6".to_string()),
            "shared_libs should contain libc.so.6, got: {:?}",
            bc.shared_libs
        );
    }

    // ---- Test 3: test_classify_binary_c_dynamic ----

    #[test]
    fn test_classify_binary_c_dynamic() {
        let exec = MockExecutor::new()
            .with_command(
                "readelf -S /opt/app/myapp",
                ExecResult {
                    stdout: readelf_c_fixture().to_string(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "readelf -d /opt/app/myapp",
                ExecResult {
                    stdout: readelf_dynamic_linked_fixture().to_string(),
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let bc = classify_binary(&exec, "/opt/app/myapp").unwrap();
        assert_eq!(bc.lang, "c/c++");
        assert!(!bc.is_static, "C binary with NEEDED entries should be dynamic");
        assert!(!bc.shared_libs.is_empty());
    }

    // ---- Test 4: test_classify_binary_c_static ----

    #[test]
    fn test_classify_binary_c_static() {
        let exec = MockExecutor::new()
            .with_command(
                "readelf -S /opt/app/myapp",
                ExecResult {
                    stdout: readelf_c_fixture().to_string(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "readelf -d /opt/app/myapp",
                ExecResult {
                    stdout: readelf_dynamic_static_fixture().to_string(),
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let bc = classify_binary(&exec, "/opt/app/myapp").unwrap();
        assert_eq!(bc.lang, "c/c++");
        assert!(bc.is_static, "C binary with no dynamic section should be static");
        assert!(bc.shared_libs.is_empty());
    }

    // ---- Test 5: test_classify_binary_readelf_unavailable ----

    #[test]
    fn test_classify_binary_readelf_unavailable() {
        let exec = MockExecutor::new().with_command(
            "readelf -S /opt/app/myapp",
            ExecResult {
                exit_code: 127,
                stderr: "command not found: readelf".to_string(),
                ..Default::default()
            },
        );

        let bc = classify_binary(&exec, "/opt/app/myapp");
        assert!(bc.is_none(), "should return None when readelf fails");
    }

    // ---- Test 6: test_strings_version_extraction ----

    #[test]
    fn test_strings_version_extraction() {
        let version = extract_version_from_text(strings_version_fixture());
        assert_eq!(version, "1.2.3", "should extract version from 'version=1.2.3'");
    }

    // ---- Test 7: test_strings_version_go_pattern ----

    #[test]
    fn test_strings_version_go_pattern() {
        let text = "go1.21.5\nother stuff\n";
        let version = extract_version_from_text(text);
        assert_eq!(version, "1.21.5", "should extract Go version");
    }

    // ---- Test 8: test_strings_version_no_match ----

    #[test]
    fn test_strings_version_no_match() {
        let text = "no version info here\njust random text\n";
        let version = extract_version_from_text(text);
        assert!(version.is_empty(), "should return empty when no match");
    }

    // ---- Test 9: test_scan_pip_venv ----

    #[test]
    fn test_scan_pip_venv() {
        let exec = MockExecutor::new()
            .with_dir("/opt", vec!["myapp"])
            .with_dir("/opt/myapp", vec!["pyvenv.cfg", "lib"])
            .with_file("/opt/myapp/pyvenv.cfg", pyvenv_cfg_fixture())
            .with_dir("/opt/myapp/lib", vec!["python3.9"])
            .with_dir("/opt/myapp/lib/python3.9", vec!["site-packages"])
            .with_dir(
                "/opt/myapp/lib/python3.9/site-packages",
                vec!["flask-2.3.3.dist-info", "requests-2.31.0.dist-info"],
            )
            .with_command(
                "pip list --path /opt/myapp/lib/python3.9/site-packages --format json",
                ExecResult {
                    stdout: pip_list_fixture().to_string(),
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let mut section = NonRpmSoftwareSection::default();
        let mut warnings = Vec::new();

        // Scan venvs under /opt.
        let venvs = find_venvs(&exec, "/opt");
        assert_eq!(venvs.len(), 1, "should find one venv");
        assert!(!venvs[0].system_site_packages);

        let packages = scan_venv_packages(&exec, &venvs[0].path);
        assert!(!packages.is_empty(), "should list venv packages");
        assert!(
            packages.iter().any(|p| p.name == "flask"),
            "should find flask package"
        );

        scan_python_venvs(&exec, &mut section, &mut warnings);
        assert_eq!(section.items.len(), 1);
        assert_eq!(section.items[0].method, "python venv");
    }

    // ---- Test 10: test_scan_pip_dist_info ----

    #[test]
    fn test_scan_pip_dist_info() {
        let exec = MockExecutor::new()
            .with_dir("/opt", vec!["venv"])
            .with_dir("/opt/venv", vec!["pyvenv.cfg", "lib"])
            .with_file("/opt/venv/pyvenv.cfg", "home = /usr/bin\nversion = 3.9.18\n")
            .with_dir("/opt/venv/lib", vec!["python3.9"])
            .with_dir("/opt/venv/lib/python3.9", vec!["site-packages"])
            .with_dir(
                "/opt/venv/lib/python3.9/site-packages",
                vec!["flask-2.3.3.dist-info", "requests-2.31.0.dist-info"],
            )
            // pip list fails.
            .with_command(
                "pip list --path /opt/venv/lib/python3.9/site-packages --format json",
                ExecResult {
                    exit_code: 1,
                    ..Default::default()
                },
            );

        let packages = scan_dist_info(&exec, "/opt/venv");
        assert_eq!(packages.len(), 2, "should find 2 dist-info packages");
        assert!(packages.iter().any(|p| p.name == "flask" && p.version == "2.3.3"));
        assert!(packages.iter().any(|p| p.name == "requests" && p.version == "2.31.0"));
    }

    // ---- Test 11: test_scan_npm_packages ----

    #[test]
    fn test_scan_npm_packages() {
        let packages = parse_package_lock(package_lock_fixture());
        assert_eq!(packages.len(), 2, "should find 2 npm packages");
        assert!(
            packages.iter().any(|p| p.name == "express" && p.version == "4.18.2"),
            "should find express package"
        );
        assert!(
            packages.iter().any(|p| p.name == "lodash" && p.version == "4.17.21"),
            "should find lodash package"
        );
    }

    // ---- Test 12: test_scan_gem_packages ----

    #[test]
    fn test_scan_gem_packages() {
        let gems = parse_gemfile_lock(gemfile_lock_fixture());
        assert!(
            !gems.is_empty(),
            "should find gems in Gemfile.lock"
        );
        assert!(
            gems.iter().any(|g| g.name == "rack" && g.version == "3.0.8"),
            "should find rack gem, got: {:?}",
            gems.iter().map(|g| format!("{}={}", g.name, g.version)).collect::<Vec<_>>()
        );
        assert!(
            gems.iter().any(|g| g.name == "sinatra" && g.version == "3.1.0"),
            "should find sinatra gem"
        );
    }

    // ---- Test 13: test_collect_env_files ----

    #[test]
    fn test_collect_env_files() {
        let exec = MockExecutor::new()
            .with_dir("/opt", vec!["myapp"])
            .with_dir("/opt/myapp", vec![".env"])
            .with_file("/opt/myapp/.env", env_file_fixture())
            .with_dir("/srv", vec![])
            .with_dir("/usr/local", vec![]);

        let mut section = NonRpmSoftwareSection::default();
        let mut hints = Vec::new();
        collect_env_files(&exec, &mut section, &mut hints);

        assert_eq!(section.env_files.len(), 1, "should collect one .env file");
        assert_eq!(section.env_files[0].path, "opt/myapp/.env");
        assert!(
            section.env_files[0].content.contains("DATABASE_URL"),
            "should preserve .env content"
        );
    }

    // ---- Test 14: test_collect_git_repos ----

    #[test]
    fn test_collect_git_repos() {
        let exec = MockExecutor::new()
            .with_dir("/opt", vec!["myapp"])
            .with_dir("/opt/myapp", vec![".git", "src"])
            .with_dir("/opt/myapp/.git", vec!["config"])
            .with_file("/opt/myapp/.git/config", git_config_fixture())
            .with_dir("/srv", vec![])
            .with_dir("/usr/local", vec![]);

        let mut section = NonRpmSoftwareSection::default();
        let mut hints = Vec::new();
        collect_git_repos(&exec, &mut section, &mut hints);

        assert_eq!(section.items.len(), 1, "should find one git repo");
        assert_eq!(section.items[0].method, "git repo");
        assert_eq!(
            section.items[0].git_remote,
            "https://github.com/example/myapp.git"
        );
    }

    // ---- Test 15: test_env_file_redaction_surface ----

    #[test]
    fn test_env_file_redaction_surface() {
        let exec = MockExecutor::new()
            .with_dir("/opt", vec!["app"])
            .with_dir("/opt/app", vec![".env"])
            .with_file("/opt/app/.env", env_file_fixture())
            .with_dir("/srv", vec![])
            .with_dir("/usr/local", vec![]);

        let mut section = NonRpmSoftwareSection::default();
        let mut hints = Vec::new();
        collect_env_files(&exec, &mut section, &mut hints);

        assert!(
            !hints.is_empty(),
            "should flag .env content for redaction"
        );
        assert!(
            hints.iter().any(|h| h.reason.contains("DATABASE_URL")),
            "should flag DATABASE_URL as potential secret, got: {:?}",
            hints.iter().map(|h| &h.reason).collect::<Vec<_>>()
        );
        assert!(
            hints.iter().any(|h| h.reason.contains("SECRET_KEY")),
            "should flag SECRET_KEY as potential secret"
        );
        assert!(
            hints.iter().any(|h| h.reason.contains("API_KEY")),
            "should flag API_KEY as potential secret"
        );
    }

    // ---- Test 16: test_git_remote_url_redaction ----

    #[test]
    fn test_git_remote_url_redaction() {
        let exec = MockExecutor::new()
            .with_dir("/opt", vec!["app"])
            .with_dir("/opt/app", vec![".git"])
            .with_dir("/opt/app/.git", vec!["config"])
            .with_file(
                "/opt/app/.git/config",
                "[remote \"origin\"]\n\turl = https://user:password@github.com/private/repo.git\n",
            )
            .with_dir("/srv", vec![])
            .with_dir("/usr/local", vec![]);

        let mut section = NonRpmSoftwareSection::default();
        let mut hints = Vec::new();
        collect_git_repos(&exec, &mut section, &mut hints);

        assert!(
            !hints.is_empty(),
            "should flag git remote URL with embedded credentials"
        );
        assert!(
            hints
                .iter()
                .any(|h| h.reason.contains("embedded credentials")),
            "hint should mention embedded credentials"
        );
    }

    // ---- Test 17: test_nonrpm_empty_system ----

    #[test]
    fn test_nonrpm_empty_system() {
        // No /opt, /srv, /usr/local — all return NotFound.
        let exec = MockExecutor::new()
            .with_command(
                "readelf --version",
                ExecResult {
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "file --version",
                ExecResult {
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = NonRpmInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
        };

        let result = inspector.inspect(&ctx);
        let output = result.expect("should succeed on empty system");
        if let SectionData::NonRpmSoftware(section) = &output.section {
            assert!(section.items.is_empty(), "should have no items");
            assert!(section.env_files.is_empty(), "should have no env files");
        } else {
            panic!("expected NonRpmSoftware section");
        }
    }

    // ---- Test 18: test_nonrpm_degraded_no_readelf ----

    #[test]
    fn test_nonrpm_degraded_no_readelf() {
        let exec = MockExecutor::new()
            .with_command(
                "readelf --version",
                ExecResult {
                    exit_code: 127,
                    stderr: "command not found".to_string(),
                    ..Default::default()
                },
            )
            // Provide env file so we get partial data (triggers Degraded, not empty Ok).
            .with_dir("/opt", vec!["app"])
            .with_dir("/opt/app", vec![".env"])
            .with_file("/opt/app/.env", "APP_NAME=test\n")
            .with_dir("/srv", vec![])
            .with_dir("/usr/local", vec![]);

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = NonRpmInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
        };

        let result = inspector.inspect(&ctx);
        match result {
            Err(InspectorError::Degraded { reason, partial }) => {
                assert!(
                    reason.contains("readelf"),
                    "degraded reason should mention readelf: {reason}"
                );
                assert!(
                    !partial.warnings.is_empty(),
                    "should have warning about readelf"
                );
            }
            other => panic!("expected Degraded, got {other:?}"),
        }
    }

    // ---- Test 19: test_nonrpm_ostree_var_filtering ----

    #[test]
    fn test_nonrpm_ostree_var_filtering() {
        let mut section = NonRpmSoftwareSection {
            items: vec![
                NonRpmItem {
                    path: "opt/app/bin/myapp".to_string(),
                    name: "myapp".to_string(),
                    ..Default::default()
                },
                NonRpmItem {
                    path: "var/lib/ostree/deploy/config".to_string(),
                    name: "config".to_string(),
                    ..Default::default()
                },
                NonRpmItem {
                    path: "var/lib/rpm-ostree/something".to_string(),
                    name: "something".to_string(),
                    ..Default::default()
                },
                NonRpmItem {
                    path: "var/lib/flatpak/app".to_string(),
                    name: "app".to_string(),
                    ..Default::default()
                },
            ],
            env_files: Vec::new(),
        };

        filter_ostree_var_paths(&mut section);

        assert_eq!(section.items.len(), 1, "should filter ostree /var paths");
        assert_eq!(section.items[0].path, "opt/app/bin/myapp");
    }

    // ---- Test 20: test_nonrpm_deduplication ----

    #[test]
    fn test_nonrpm_deduplication() {
        let mut section = NonRpmSoftwareSection {
            items: vec![
                NonRpmItem {
                    path: "opt/app/bin/myapp".to_string(),
                    name: "myapp".to_string(),
                    confidence: "low".to_string(),
                    method: "file scan".to_string(),
                    ..Default::default()
                },
                NonRpmItem {
                    path: "opt/app/bin/myapp".to_string(),
                    name: "myapp".to_string(),
                    confidence: "high".to_string(),
                    method: "readelf (go)".to_string(),
                    lang: "go".to_string(),
                    ..Default::default()
                },
                NonRpmItem {
                    path: "opt/other/bin/tool".to_string(),
                    name: "tool".to_string(),
                    confidence: "medium".to_string(),
                    ..Default::default()
                },
            ],
            env_files: Vec::new(),
        };

        deduplicate_items(&mut section);

        assert_eq!(section.items.len(), 2, "should deduplicate same-path items");
        let myapp = section.items.iter().find(|i| i.name == "myapp").unwrap();
        assert_eq!(
            myapp.confidence, "high",
            "should keep highest-confidence entry"
        );
        assert_eq!(myapp.method, "readelf (go)");
    }
}
