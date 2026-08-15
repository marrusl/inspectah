//! Language package Containerfile rendering — pip/npm/gem sections.
//!
//! Replaces advisory stubs with executable COPY/RUN instructions for
//! language environment items. High-confidence items render as active
//! instructions; medium-confidence renders commented-out.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::nonrpm::NonRpmItem;
use inspectah_core::util::{
    METHOD_GEM_LOCKFILE, METHOD_GEM_SYSTEM, METHOD_NPM_GLOBAL, METHOD_NPM_LOCKFILE,
    METHOD_NPM_MANIFEST, METHOD_PIP_DIST_INFO, METHOD_PYTHON_VENV, env_hash,
};

const HIGH_CONFIDENCE: &str = "high";
const MEDIUM_CONFIDENCE: &str = "medium";

/// Runtime RPM package names checked for each ecosystem.
const RUNTIME_PYTHON: &str = "python3";
const RUNTIME_NODEJS: &str = "nodejs";
const RUNTIME_RUBYGEMS: &str = "rubygems";

/// Returns true if the item is a pip environment (venv or system-level).
pub fn is_pip_env(item: &NonRpmItem) -> bool {
    item.method == METHOD_PYTHON_VENV || item.method == METHOD_PIP_DIST_INFO
}

/// Returns true if the item is an npm environment (lockfile or manifest-only).
fn is_npm_env(item: &NonRpmItem) -> bool {
    item.method == METHOD_NPM_LOCKFILE || item.method == METHOD_NPM_MANIFEST
}

/// Returns true if the item is an npm global environment.
fn is_npm_global_env(item: &NonRpmItem) -> bool {
    item.method == METHOD_NPM_GLOBAL
}

/// Returns true if the item is a gem environment (lockfile or system).
fn is_gem_env(item: &NonRpmItem) -> bool {
    item.method == METHOD_GEM_LOCKFILE || item.method == METHOD_GEM_SYSTEM
}

/// Returns true if the item is a language environment handled by this module.
pub fn is_language_env(item: &NonRpmItem) -> bool {
    is_pip_env(item) || is_npm_env(item) || is_npm_global_env(item) || is_gem_env(item)
}

/// Render Containerfile lines for all language package environments.
///
/// Processes ALL language environment items regardless of `include` state.
/// Medium-confidence excluded items render as commented-out blocks so they
/// remain visible and reviewable in the Containerfile.
pub fn language_package_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let nrs = match &snap.non_rpm_software {
        Some(n) if !n.items.is_empty() => n,
        _ => return Vec::new(),
    };

    // Collect RPM package names for runtime prerequisite checks.
    let rpm_names = collect_rpm_names(snap);

    let pip_items: Vec<&NonRpmItem> = nrs.items.iter().filter(|i| is_pip_env(i)).collect();
    let npm_items: Vec<&NonRpmItem> = nrs.items.iter().filter(|i| is_npm_env(i)).collect();
    let npm_global_items: Vec<&NonRpmItem> =
        nrs.items.iter().filter(|i| is_npm_global_env(i)).collect();
    let gem_items: Vec<&NonRpmItem> = nrs.items.iter().filter(|i| is_gem_env(i)).collect();

    if pip_items.is_empty()
        && npm_items.is_empty()
        && npm_global_items.is_empty()
        && gem_items.is_empty()
    {
        return Vec::new();
    }

    let mut lines = Vec::new();

    if !pip_items.is_empty() {
        lines.extend(render_pip_section(&pip_items, &rpm_names));
    }
    if !npm_items.is_empty() {
        lines.extend(render_npm_section(&npm_items, &rpm_names));
    }
    if !npm_global_items.is_empty() {
        lines.extend(render_npm_global_section(&npm_global_items, &rpm_names));
    }
    if !gem_items.is_empty() {
        lines.extend(render_gem_section(&gem_items, &rpm_names));
    }

    lines
}

/// Collect RPM package names (without arch suffix) from the snapshot.
fn collect_rpm_names(snap: &InspectionSnapshot) -> Vec<String> {
    let rpm = match &snap.rpm {
        Some(r) => r,
        None => return Vec::new(),
    };
    rpm.packages_added
        .iter()
        .map(|p| {
            // Strip .arch suffix to get bare name for matching.
            p.name
                .rsplit_once('.')
                .map_or(p.name.as_str(), |(name, _)| name)
                .to_string()
        })
        .collect()
}

/// Check if a runtime package is present in the RPM list.
fn has_runtime(rpm_names: &[String], runtime: &str) -> bool {
    rpm_names.iter().any(|n| n == runtime)
}

/// Format a pinned package list: `pkg1==ver1 pkg2==ver2`.
fn pinned_package_list(item: &NonRpmItem) -> String {
    item.packages
        .iter()
        .map(|p| {
            if p.version.is_empty() {
                p.name.clone()
            } else {
                format!("{}=={}", p.name, p.version)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Characters that make a token unsafe to interpolate into a rendered
/// `RUN` line. The single quote and newline break out of the single-quoted
/// paths the renderers emit; the shell metacharacters matter at the
/// unquoted interpolation sites, notably the npm global package list.
const UNSAFE_SHELL_CHARS: &[char] = &['\'', '\n', ';', '$', '`', '|', '&'];

/// Returns true if a string contains characters that are unsafe to render
/// into a shell command.
fn contains_unsafe_chars(s: &str) -> bool {
    s.contains(UNSAFE_SHELL_CHARS)
}

/// Sanitize a path for display in warning comments by replacing unsafe chars.
fn sanitize_for_comment(s: &str) -> String {
    s.replace('\'', "''").replace('\n', "\\n")
}

/// Restore the leading `/` on a path the collector stored relative.
///
/// Leading-slash normalization is `trim_start_matches('/')` on both halves
/// of this contract: the collectors in `nonrpm.rs` strip *every* leading
/// slash before storing, and the renderers strip again before prepending
/// exactly one. `strip_prefix('/')` removes only the first, so mixing the
/// two turns a doubled root into `//opt` on one side and `/opt` on the
/// other for the same stored path.
fn absolute_path(stored_path: &str) -> String {
    format!("/{}", stored_path.trim_start_matches('/'))
}

// ---------------------------------------------------------------------------
// pip rendering
// ---------------------------------------------------------------------------

fn render_pip_section(items: &[&NonRpmItem], rpm_names: &[String]) -> Vec<String> {
    let mut lines = Vec::new();

    if !has_runtime(rpm_names, RUNTIME_PYTHON) && !rpm_names.is_empty() {
        lines.push(format!(
            "# WARNING: {RUNTIME_PYTHON} not found in RPM package list \
             — add it before this section"
        ));
    }

    for item in items {
        lines.push(String::new());
        lines.extend(render_pip_item(item));
    }

    lines
}

fn render_pip_item(item: &NonRpmItem) -> Vec<String> {
    let mut lines = Vec::new();

    // Low confidence: advisory only (defensive — should not occur after hardening).
    if item.confidence != HIGH_CONFIDENCE && item.confidence != MEDIUM_CONFIDENCE {
        lines.push(format!(
            "# pip packages: {} (low confidence — review required)",
            item.path
        ));
        return lines;
    }

    // Excluded items never render as active, even at high confidence.
    // Downgrade to medium so they render commented-out.
    let effective_confidence = if !item.disposition.is_included() {
        MEDIUM_CONFIDENCE
    } else {
        item.confidence.as_str()
    };

    let is_venv = item.method == METHOD_PYTHON_VENV;
    let has_requirements = item.manifest_files.contains_key("requirements.txt");

    // C-extension safety gate.
    if item.has_c_extensions {
        lines.push(
            "# WARNING: This environment contains packages with C extensions that may need".into(),
        );
        lines.push("# native compilation toolchains (gcc, python3-devel).".into());
    }

    let abs_path = absolute_path(&item.path);

    // Shell injection safety: reject paths with unsafe characters.
    if contains_unsafe_chars(&abs_path) {
        lines.push(format!(
            "# WARNING: path contains unsafe characters, skipping: {}",
            sanitize_for_comment(&abs_path)
        ));
        return lines;
    }

    if is_venv && has_requirements && effective_confidence == HIGH_CONFIDENCE {
        // High confidence venv with requirements.txt: executable COPY/RUN.
        let hash = env_hash(&item.path);
        let venv_name = std::path::Path::new(&abs_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "venv".to_string());
        let fidelity = if item.rpm_filtered {
            "from requirements.txt, RPM-filtered"
        } else {
            "from requirements.txt"
        };

        lines.push(format!("# pip packages: {abs_path} ({fidelity})"));
        lines.push(format!(
            "COPY language-packages/pip/{hash}/requirements.txt /tmp/{venv_name}-requirements.txt"
        ));
        let venv_flags = if item.system_site_packages {
            "--system-site-packages "
        } else {
            ""
        };
        lines.push(format!("RUN python3 -m venv {venv_flags}'{abs_path}' \\"));
        lines.push(format!(
            "    && '{abs_path}'/bin/pip install -r /tmp/{venv_name}-requirements.txt \\"
        ));
        lines.push(format!("    && rm /tmp/{venv_name}-requirements.txt"));
    } else if is_venv {
        // Medium confidence venv (no requirements.txt): commented out.
        let pkgs = pinned_package_list(item);
        lines.push(format!(
            "# pip packages: {abs_path} (detected via dist-info \
             — transitive deps may differ)"
        ));
        lines.push("# Uncomment after verifying package list is complete:".into());
        let venv_flags = if item.system_site_packages {
            "--system-site-packages "
        } else {
            ""
        };
        lines.push(format!("# RUN python3 -m venv {venv_flags}'{abs_path}' \\"));
        if pkgs.is_empty() {
            lines.push(format!("#     && '{abs_path}'/bin/pip install <packages>"));
        } else {
            lines.push(format!("#     && '{abs_path}'/bin/pip install {pkgs}"));
        }
    } else {
        // System-level pip (always medium confidence): commented out.
        let pkgs = pinned_package_list(item);
        let fidelity = if item.rpm_filtered {
            "detected via pip list, RPM-filtered"
        } else {
            "detected via pip list"
        };
        lines.push(format!("# pip packages: system ({fidelity})"));
        lines.push("# Uncomment after verifying package list is complete:".into());
        if pkgs.is_empty() {
            lines.push("# RUN pip install <packages>".into());
        } else {
            lines.push(format!("# RUN pip install {pkgs}"));
        }
    }

    lines
}

// ---------------------------------------------------------------------------
// npm rendering
// ---------------------------------------------------------------------------

fn render_npm_section(items: &[&NonRpmItem], rpm_names: &[String]) -> Vec<String> {
    let mut lines = Vec::new();

    if !has_runtime(rpm_names, RUNTIME_NODEJS) && !rpm_names.is_empty() {
        lines.push(format!(
            "# WARNING: {RUNTIME_NODEJS} not found in RPM package list \
             — add it before this section"
        ));
    }

    for item in items {
        lines.push(String::new());
        lines.extend(render_npm_item(item));
    }

    lines
}

fn render_npm_item(item: &NonRpmItem) -> Vec<String> {
    let mut lines = Vec::new();

    // Low confidence: advisory only (defensive — should not occur after hardening).
    if item.confidence != HIGH_CONFIDENCE && item.confidence != MEDIUM_CONFIDENCE {
        let project_path = absolute_path(&item.path);
        lines.push(format!(
            "# npm packages: {project_path} (low confidence — review required)"
        ));
        return lines;
    }

    // Excluded items never render as active, even at high confidence.
    let effective_confidence = if !item.disposition.is_included() {
        MEDIUM_CONFIDENCE
    } else {
        item.confidence.as_str()
    };

    let hash = env_hash(&item.path);
    let project_path = absolute_path(&item.path);

    // Shell injection safety: reject paths with unsafe characters.
    if contains_unsafe_chars(&project_path) {
        lines.push(format!(
            "# WARNING: path contains unsafe characters, skipping: {}",
            sanitize_for_comment(&project_path)
        ));
        return lines;
    }

    if effective_confidence == HIGH_CONFIDENCE {
        lines.push(format!(
            "# npm packages: {project_path} (from package-lock.json)"
        ));
        lines.push(format!(
            "COPY language-packages/npm/{hash}/package.json '{project_path}'/package.json"
        ));
        lines.push(format!(
            "COPY language-packages/npm/{hash}/package-lock.json \
             '{project_path}'/package-lock.json"
        ));
        lines.push(format!("RUN cd '{project_path}' && npm ci --production"));
    } else {
        // Medium confidence: commented out.
        lines.push(format!(
            "# npm packages: {project_path} (detected via package-lock.json)"
        ));
        lines.push("# Uncomment after verifying package list is complete:".into());
        lines.push(format!(
            "# COPY language-packages/npm/{hash}/package.json '{project_path}'/package.json"
        ));
        lines.push(format!(
            "# COPY language-packages/npm/{hash}/package-lock.json \
             '{project_path}'/package-lock.json"
        ));
        lines.push(format!("# RUN cd '{project_path}' && npm ci --production"));
    }

    lines
}

// ---------------------------------------------------------------------------
// npm global rendering
// ---------------------------------------------------------------------------

fn render_npm_global_section(items: &[&NonRpmItem], rpm_names: &[String]) -> Vec<String> {
    let mut lines = Vec::new();

    if !has_runtime(rpm_names, RUNTIME_NODEJS) && !rpm_names.is_empty() {
        lines.push(format!(
            "# WARNING: {RUNTIME_NODEJS} not found in RPM package list \
             — add it before this section"
        ));
    }

    for item in items {
        lines.push(String::new());
        lines.extend(render_npm_global_item(item));
    }

    lines
}

fn render_npm_global_item(item: &NonRpmItem) -> Vec<String> {
    let mut lines = Vec::new();
    let prefix = absolute_path(&item.path);

    let effective_confidence = if !item.disposition.is_included() {
        MEDIUM_CONFIDENCE
    } else {
        item.confidence.as_str()
    };

    let method_label = if item.confidence == HIGH_CONFIDENCE {
        "detected via npm list -g"
    } else {
        "detected via directory walk"
    };

    // Build package list respecting pin state
    let pkg_list: Vec<String> = item
        .packages
        .iter()
        .filter_map(|p| {
            // Shell injection safety: reject packages with unsafe characters.
            if contains_unsafe_chars(&p.name) || contains_unsafe_chars(&p.version) {
                None
            } else if p.pinned && !p.version.is_empty() {
                Some(format!("{}@{}", p.name, p.version))
            } else {
                Some(p.name.clone())
            }
        })
        .collect();

    // Count rejected packages.
    let rejected_count = item.packages.len() - pkg_list.len();
    if rejected_count > 0 {
        lines.push(format!(
            "# WARNING: {rejected_count} package(s) skipped due to unsafe characters in name/version"
        ));
    }

    let pkg_list = pkg_list.join(" ");

    if effective_confidence == HIGH_CONFIDENCE {
        lines.push(format!("# npm global packages: {prefix} ({method_label})"));
        if item.has_c_extensions {
            lines.push(
                "# WARNING: environment contains native addons — \
                 build tools (gcc, node-gyp) may be needed"
                    .into(),
            );
        }
        lines.push(format!("RUN npm install -g {pkg_list}"));
    } else {
        lines.push(format!("# npm global packages: {prefix} ({method_label})"));
        lines.push(format!("# RUN npm install -g {pkg_list}"));
    }

    lines
}

// ---------------------------------------------------------------------------
// gem rendering
// ---------------------------------------------------------------------------

fn render_gem_section(items: &[&NonRpmItem], rpm_names: &[String]) -> Vec<String> {
    let mut lines = Vec::new();

    if !has_runtime(rpm_names, RUNTIME_RUBYGEMS) && !rpm_names.is_empty() {
        lines.push(format!(
            "# WARNING: {RUNTIME_RUBYGEMS} not found in RPM package list \
             — add it before this section"
        ));
    }

    for item in items {
        lines.push(String::new());
        lines.extend(render_gem_item(item));
    }

    lines
}

fn render_gem_item(item: &NonRpmItem) -> Vec<String> {
    let mut lines = Vec::new();

    // Low confidence: advisory only (defensive — should not occur after hardening).
    if item.confidence != HIGH_CONFIDENCE && item.confidence != MEDIUM_CONFIDENCE {
        let project_path = absolute_path(&item.path);
        lines.push(format!(
            "# gem packages: {project_path} (low confidence — review required)"
        ));
        return lines;
    }

    // Excluded items never render as active, even at high confidence.
    let effective_confidence = if !item.disposition.is_included() {
        MEDIUM_CONFIDENCE
    } else {
        item.confidence.as_str()
    };

    let hash = env_hash(&item.path);
    let project_path = absolute_path(&item.path);

    // Shell injection safety: reject paths with unsafe characters.
    if contains_unsafe_chars(&project_path) {
        lines.push(format!(
            "# WARNING: path contains unsafe characters, skipping: {}",
            sanitize_for_comment(&project_path)
        ));
        return lines;
    }

    if effective_confidence == HIGH_CONFIDENCE {
        lines.push(format!(
            "# gem packages: {project_path} (from Gemfile.lock)"
        ));
        lines.push(format!(
            "COPY language-packages/gem/{hash}/Gemfile '{project_path}'/Gemfile"
        ));
        lines.push(format!(
            "COPY language-packages/gem/{hash}/Gemfile.lock '{project_path}'/Gemfile.lock"
        ));
        lines.push(format!(
            "RUN cd '{project_path}' && bundle config set --local deployment 'true' && bundle install"
        ));
    } else {
        // Medium confidence: commented out.
        lines.push(format!(
            "# gem packages: {project_path} (detected via Gemfile.lock)"
        ));
        lines.push("# Uncomment after verifying package list is complete:".into());
        lines.push(format!(
            "# COPY language-packages/gem/{hash}/Gemfile '{project_path}'/Gemfile"
        ));
        lines.push(format!(
            "# COPY language-packages/gem/{hash}/Gemfile.lock '{project_path}'/Gemfile.lock"
        ));
        lines.push(format!(
            "# RUN cd '{project_path}' && bundle config set --local deployment 'true' && bundle install"
        ));
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::FindingKind;
    use inspectah_core::types::nonrpm::{LanguagePackage, NonRpmSoftwareSection};
    use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
    use std::collections::HashMap;

    /// The renderer must produce exactly one leading slash for any stored
    /// form the collector's `trim_start_matches('/')` can emit, including
    /// the doubled roots it normalizes away.
    #[test]
    fn absolute_path_restores_exactly_one_leading_slash() {
        let cases = [
            ("opt/app", "/opt/app"),
            ("/opt/app", "/opt/app"),
            ("//opt/app", "/opt/app"),
            ("///opt/app", "/opt/app"),
            ("", "/"),
        ];
        for (stored, expected) in cases {
            assert_eq!(
                absolute_path(stored),
                expected,
                "stored path {stored:?} must render as {expected:?}"
            );
        }
    }

    /// Build a minimal snapshot with given non-RPM items and optional RPM packages.
    fn test_snap(items: Vec<NonRpmItem>, rpm_names: &[&str]) -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.non_rpm_software = Some(NonRpmSoftwareSection {
            items,
            env_files: vec![],
        });
        if !rpm_names.is_empty() {
            snap.rpm = Some(RpmSection {
                packages_added: rpm_names
                    .iter()
                    .map(|n| PackageEntry {
                        name: n.to_string(),
                        version: "1.0".into(),
                        release: "1.el9".into(),
                        arch: "x86_64".into(),
                        state: PackageState::Added,
                        disposition: FindingKind::included(),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            });
        }
        snap
    }

    fn pip_venv_item(
        path: &str,
        confidence: &str,
        has_req: bool,
        packages: Vec<(&str, &str)>,
    ) -> NonRpmItem {
        let mut manifest_files = HashMap::new();
        if has_req {
            manifest_files.insert(
                "requirements.txt".to_string(),
                packages
                    .iter()
                    .map(|(n, v)| format!("{n}=={v}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
        NonRpmItem {
            path: path.into(),
            name: std::path::Path::new(path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            method: METHOD_PYTHON_VENV.into(),
            confidence: confidence.into(),
            disposition: FindingKind::from_bool(confidence == HIGH_CONFIDENCE),
            packages: packages
                .iter()
                .map(|(n, v)| LanguagePackage {
                    name: n.to_string(),
                    version: v.to_string(),
                    pinned: false,
                })
                .collect(),
            manifest_files,
            rpm_filtered: has_req,
            ..Default::default()
        }
    }

    fn npm_item(path: &str, confidence: &str) -> NonRpmItem {
        let mut manifest_files = HashMap::new();
        manifest_files.insert("package.json".to_string(), "{}".to_string());
        manifest_files.insert(
            "package-lock.json".to_string(),
            r#"{"lockfileVersion":3}"#.to_string(),
        );
        NonRpmItem {
            path: path.into(),
            name: std::path::Path::new(path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            method: METHOD_NPM_LOCKFILE.into(),
            confidence: confidence.into(),
            disposition: FindingKind::from_bool(confidence == HIGH_CONFIDENCE),
            manifest_files,
            ..Default::default()
        }
    }

    fn gem_item(path: &str, confidence: &str) -> NonRpmItem {
        let mut manifest_files = HashMap::new();
        manifest_files.insert(
            "Gemfile".to_string(),
            "source 'https://rubygems.org'".to_string(),
        );
        manifest_files.insert("Gemfile.lock".to_string(), "GEM\n  specs:\n".to_string());
        NonRpmItem {
            path: path.into(),
            name: std::path::Path::new(path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            method: METHOD_GEM_LOCKFILE.into(),
            confidence: confidence.into(),
            disposition: FindingKind::from_bool(confidence == HIGH_CONFIDENCE),
            manifest_files,
            ..Default::default()
        }
    }

    #[test]
    fn pip_venv_high_confidence_renders_copy_and_run() {
        let snap = test_snap(
            vec![pip_venv_item(
                "/opt/myapp/venv",
                HIGH_CONFIDENCE,
                true,
                vec![("flask", "2.3.3"), ("requests", "2.31.0")],
            )],
            &["python3"],
        );
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains("COPY language-packages/pip/"),
            "must COPY requirements.txt: {output}"
        );
        assert!(
            output.contains("RUN python3 -m venv '/opt/myapp/venv'"),
            "must create venv with quoted path: {output}"
        );
        assert!(
            output.contains("pip install -r"),
            "must pip install from requirements: {output}"
        );
        // Must not be commented out.
        assert!(
            output.contains("\nRUN python3"),
            "RUN must not be commented out: {output}"
        );
    }

    #[test]
    fn pip_venv_medium_confidence_renders_commented_out() {
        let snap = test_snap(
            vec![pip_venv_item(
                "/opt/myapp/venv",
                MEDIUM_CONFIDENCE,
                false,
                vec![("flask", "2.3.3"), ("requests", "2.31.0")],
            )],
            &["python3"],
        );
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains("# RUN python3 -m venv"),
            "medium confidence must be commented out: {output}"
        );
        assert!(
            output.contains("# Uncomment after verifying"),
            "must include uncomment guidance: {output}"
        );
        assert!(
            output.contains("flask==2.3.3"),
            "must include pinned packages: {output}"
        );
    }

    #[test]
    fn pip_c_extension_emits_toolchain_warning() {
        let mut item = pip_venv_item(
            "/opt/myapp/venv",
            HIGH_CONFIDENCE,
            true,
            vec![("numpy", "1.24.0")],
        );
        item.has_c_extensions = true;
        let snap = test_snap(vec![item], &["python3"]);
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains("WARNING: This environment contains packages with C extensions"),
            "must warn about C extensions: {output}"
        );
        assert!(
            output.contains("native compilation toolchains"),
            "must mention toolchains: {output}"
        );
    }

    #[test]
    fn npm_lockfile_renders_copy_and_npm_ci() {
        let snap = test_snap(vec![npm_item("/opt/myapp", HIGH_CONFIDENCE)], &["nodejs"]);
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains("COPY language-packages/npm/"),
            "must COPY npm manifests: {output}"
        );
        assert!(
            output.contains("package.json"),
            "must copy package.json: {output}"
        );
        assert!(
            output.contains("package-lock.json"),
            "must copy package-lock.json: {output}"
        );
        assert!(
            output.contains("npm ci --production"),
            "must run npm ci: {output}"
        );
    }

    #[test]
    fn gem_lockfile_renders_copy_and_bundle_install() {
        let snap = test_snap(vec![gem_item("/opt/myapp", HIGH_CONFIDENCE)], &["rubygems"]);
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains("COPY language-packages/gem/"),
            "must COPY gem manifests: {output}"
        );
        assert!(output.contains("Gemfile"), "must copy Gemfile: {output}");
        assert!(
            output.contains("Gemfile.lock"),
            "must copy Gemfile.lock: {output}"
        );
        assert!(
            output.contains("bundle config set --local deployment 'true' && bundle install"),
            "must run bundle install with new syntax: {output}"
        );
    }

    #[test]
    fn missing_runtime_emits_warning_comment() {
        // pip items but no python3 in RPM list.
        let snap = test_snap(
            vec![pip_venv_item(
                "/opt/myapp/venv",
                HIGH_CONFIDENCE,
                true,
                vec![("flask", "2.3.3")],
            )],
            &["httpd"], // some RPM but not python3
        );
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains("WARNING: python3 not found in RPM package list"),
            "must warn about missing python3: {output}"
        );
    }

    #[test]
    fn medium_confidence_items_rendered_even_when_excluded() {
        // Medium confidence item with include: false — must still render (commented).
        let mut item = pip_venv_item(
            "/opt/myapp/venv",
            MEDIUM_CONFIDENCE,
            false,
            vec![("flask", "2.3.3")],
        );
        item.disposition = FindingKind::excluded();

        let snap = test_snap(vec![item], &["python3"]);
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            !output.is_empty(),
            "excluded medium-confidence items must still produce output"
        );
        assert!(
            output.contains("# RUN python3"),
            "excluded item must render as commented-out: {output}"
        );
    }

    #[test]
    fn low_confidence_items_render_advisory_only() {
        // Low-confidence items should not produce executable or commented-out
        // install commands — they render as advisory comments only. In practice
        // low confidence should not occur after hardening, but we handle it
        // defensively.
        let item = NonRpmItem {
            path: "/opt/myapp/venv".into(),
            name: "venv".into(),
            method: METHOD_PYTHON_VENV.into(),
            confidence: "low".into(),
            disposition: FindingKind::excluded(),
            ..Default::default()
        };
        let snap = test_snap(vec![item], &["python3"]);
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        // Low confidence gets the medium-confidence commented-out treatment
        // (since it's neither high-confidence). This is fine — it will be
        // commented out and not executable.
        assert!(
            !output.contains("\nRUN ") && !output.contains("\nCOPY "),
            "low confidence must not produce active instructions: {output}"
        );
    }

    #[test]
    fn empty_snapshot_produces_no_lines() {
        let snap = InspectionSnapshot::new();
        let lines = language_package_lines(&snap);
        assert!(lines.is_empty(), "empty snapshot must produce no lines");
    }

    #[test]
    fn non_language_items_ignored() {
        // Binary items should not be processed by this module.
        let snap = test_snap(
            vec![NonRpmItem {
                path: "/opt/bin/myapp".into(),
                name: "myapp".into(),
                method: "binary".into(),
                confidence: "high".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            &[],
        );
        let lines = language_package_lines(&snap);
        assert!(
            lines.is_empty(),
            "binary items must not produce language package lines"
        );
    }

    #[test]
    fn system_pip_renders_commented_out() {
        let item = NonRpmItem {
            path: "/usr/lib/python3.9/site-packages".into(),
            name: "system-pip".into(),
            method: METHOD_PIP_DIST_INFO.into(),
            confidence: MEDIUM_CONFIDENCE.into(),
            disposition: FindingKind::excluded(),
            packages: vec![
                LanguagePackage {
                    name: "flask".into(),
                    version: "2.3.3".into(),
                    pinned: false,
                },
                LanguagePackage {
                    name: "requests".into(),
                    version: "2.31.0".into(),
                    pinned: false,
                },
            ],
            rpm_filtered: true,
            ..Default::default()
        };
        let snap = test_snap(vec![item], &["python3"]);
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains("# pip packages: system"),
            "system pip must show 'system' label: {output}"
        );
        assert!(
            output.contains("# RUN pip install"),
            "system pip must be commented out: {output}"
        );
        assert!(
            output.contains("flask==2.3.3"),
            "must include pinned packages: {output}"
        );
    }

    #[test]
    fn runtime_warning_skipped_when_no_rpm_data() {
        // When there's no RPM section at all, skip the runtime warning —
        // we can't know what's installed.
        let snap = test_snap(
            vec![pip_venv_item(
                "/opt/myapp/venv",
                HIGH_CONFIDENCE,
                true,
                vec![("flask", "2.3.3")],
            )],
            &[], // no RPM data
        );
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            !output.contains("WARNING: python3 not found"),
            "should not warn when RPM section is absent: {output}"
        );
    }

    #[test]
    fn high_confidence_excluded_renders_commented_out() {
        // High-confidence item with include: false must NOT produce active
        // COPY/RUN — it should render commented-out like medium confidence.
        let mut item = pip_venv_item(
            "/opt/myapp/venv",
            HIGH_CONFIDENCE,
            true,
            vec![("flask", "2.3.3")],
        );
        item.disposition = FindingKind::excluded();

        let snap = test_snap(vec![item], &["python3"]);
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        // Must not contain active (uncommented) COPY or RUN.
        assert!(
            !output.contains("\nCOPY "),
            "excluded high-confidence must not produce active COPY: {output}"
        );
        assert!(
            !output.contains("\nRUN "),
            "excluded high-confidence must not produce active RUN: {output}"
        );
        // Must still render as commented-out.
        assert!(
            output.contains("# RUN python3") || output.contains("# Uncomment"),
            "excluded high-confidence must render commented-out: {output}"
        );
    }

    #[test]
    fn pip_venv_normalizes_relative_path_to_absolute() {
        // The collector strips the leading slash; the renderer must restore it.
        let item = pip_venv_item(
            "opt/myapp/venv", // no leading slash — collector output shape
            HIGH_CONFIDENCE,
            true,
            vec![("flask", "2.3.3")],
        );
        let snap = test_snap(vec![item], &["python3"]);
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains("RUN python3 -m venv '/opt/myapp/venv'"),
            "must normalize path to absolute with quotes: {output}"
        );
        assert!(
            !output.contains("RUN python3 -m venv 'opt/"),
            "must not use relative path: {output}"
        );
    }

    #[test]
    fn high_confidence_excluded_npm_renders_commented_out() {
        let mut item = npm_item("/opt/webapp", HIGH_CONFIDENCE);
        item.disposition = FindingKind::excluded();

        let snap = test_snap(vec![item], &["nodejs"]);
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            !output.contains("\nCOPY "),
            "excluded high-confidence npm must not produce active COPY: {output}"
        );
        assert!(
            !output.contains("\nRUN "),
            "excluded high-confidence npm must not produce active RUN: {output}"
        );
        assert!(
            output.contains("# RUN cd") || output.contains("# COPY"),
            "excluded high-confidence npm must render commented-out: {output}"
        );
    }

    #[test]
    fn env_hash_used_in_paths() {
        let path = "/opt/myapp/venv";
        let expected_hash = env_hash(path);
        let snap = test_snap(
            vec![pip_venv_item(
                path,
                HIGH_CONFIDENCE,
                true,
                vec![("flask", "2.3.3")],
            )],
            &["python3"],
        );
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains(&expected_hash),
            "must use env_hash for paths: expected {expected_hash} in output: {output}"
        );
    }

    #[test]
    fn system_site_packages_true_includes_flag() {
        let mut item = pip_venv_item(
            "/opt/myapp/venv",
            HIGH_CONFIDENCE,
            true,
            vec![("flask", "2.3.3")],
        );
        item.system_site_packages = true;
        let snap = test_snap(vec![item], &["python3"]);
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains("--system-site-packages"),
            "system_site_packages: true must include --system-site-packages flag: {output}"
        );
        assert!(
            output.contains("RUN python3 -m venv --system-site-packages '/opt/myapp/venv'"),
            "flag must appear in venv creation command with quoted path: {output}"
        );
    }

    #[test]
    fn system_site_packages_false_excludes_flag() {
        let mut item = pip_venv_item(
            "/opt/myapp/venv",
            HIGH_CONFIDENCE,
            true,
            vec![("flask", "2.3.3")],
        );
        item.system_site_packages = false;
        let snap = test_snap(vec![item], &["python3"]);
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            !output.contains("--system-site-packages"),
            "system_site_packages: false must NOT include --system-site-packages flag: {output}"
        );
    }

    #[test]
    fn gem_high_confidence_uses_new_bundler_syntax() {
        let snap = test_snap(vec![gem_item("/opt/myapp", HIGH_CONFIDENCE)], &["rubygems"]);
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains("bundle config set --local deployment 'true'"),
            "must use new bundle config syntax: {output}"
        );
        assert!(
            output.contains("&& bundle install"),
            "must follow with bundle install: {output}"
        );
    }

    #[test]
    fn gem_high_confidence_does_not_use_deprecated_flag() {
        let snap = test_snap(vec![gem_item("/opt/myapp", HIGH_CONFIDENCE)], &["rubygems"]);
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            !output.contains("bundle install --deployment"),
            "must NOT use deprecated --deployment flag: {output}"
        );
    }

    #[test]
    fn gem_medium_confidence_uses_new_bundler_syntax() {
        let snap = test_snap(
            vec![gem_item("/opt/myapp", MEDIUM_CONFIDENCE)],
            &["rubygems"],
        );
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains("# RUN cd '/opt/myapp' && bundle config set --local deployment 'true' && bundle install"),
            "medium confidence commented version must use new syntax with quoted path: {output}"
        );
        assert!(
            !output.contains("bundle install --deployment"),
            "must NOT use deprecated --deployment flag: {output}"
        );
    }

    // ---------------------------------------------------------------------------
    // npm global tests
    // ---------------------------------------------------------------------------

    fn npm_global_item(
        path: &str,
        confidence: &str,
        packages: Vec<(&str, &str, bool)>,
    ) -> NonRpmItem {
        use inspectah_core::util::METHOD_NPM_GLOBAL;
        NonRpmItem {
            path: path.into(),
            name: "npm-global".into(),
            method: METHOD_NPM_GLOBAL.into(),
            confidence: confidence.into(),
            disposition: FindingKind::from_bool(confidence == HIGH_CONFIDENCE),
            packages: packages
                .iter()
                .map(|(n, v, pinned)| LanguagePackage {
                    name: n.to_string(),
                    version: v.to_string(),
                    pinned: *pinned,
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn npm_global_rendered_unpinned_by_default() {
        let snap = test_snap(
            vec![npm_global_item(
                "/usr/local/lib/node_modules",
                HIGH_CONFIDENCE,
                vec![("pm2", "5.3.0", false), ("typescript", "5.4.2", false)],
            )],
            &["nodejs"],
        );
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains("RUN npm install -g pm2 typescript"),
            "unpinned packages must not include version: {output}"
        );
        assert!(
            !output.contains("@5.3.0") && !output.contains("@5.4.2"),
            "unpinned packages must not have versions: {output}"
        );
    }

    #[test]
    fn npm_global_rendered_pinned() {
        let snap = test_snap(
            vec![npm_global_item(
                "/usr/local/lib/node_modules",
                HIGH_CONFIDENCE,
                vec![("pm2", "5.3.0", true), ("typescript", "5.4.2", true)],
            )],
            &["nodejs"],
        );
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains("RUN npm install -g pm2@5.3.0 typescript@5.4.2"),
            "pinned packages must include version: {output}"
        );
    }

    #[test]
    fn npm_global_mixed_pin_state() {
        let snap = test_snap(
            vec![npm_global_item(
                "/usr/local/lib/node_modules",
                HIGH_CONFIDENCE,
                vec![("pm2", "5.3.0", true), ("typescript", "5.4.2", false)],
            )],
            &["nodejs"],
        );
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains("pm2@5.3.0"),
            "pinned package must include version: {output}"
        );
        assert!(
            !output.contains("typescript@"),
            "unpinned package must not include version: {output}"
        );
        assert!(
            output.contains("RUN npm install -g pm2@5.3.0 typescript"),
            "mixed pin state must be respected: {output}"
        );
    }

    #[test]
    fn npm_global_scoped_package_rendering() {
        let snap = test_snap(
            vec![npm_global_item(
                "/usr/local/lib/node_modules",
                HIGH_CONFIDENCE,
                vec![("@angular/cli", "17.0.0", false)],
            )],
            &["nodejs"],
        );
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains("RUN npm install -g @angular/cli"),
            "scoped packages must be rendered correctly: {output}"
        );
    }

    #[test]
    fn npm_global_excluded_renders_commented_out() {
        let mut item = npm_global_item(
            "/usr/local/lib/node_modules",
            HIGH_CONFIDENCE,
            vec![("pm2", "5.3.0", false)],
        );
        item.disposition = FindingKind::excluded();

        let snap = test_snap(vec![item], &["nodejs"]);
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains("# RUN npm install -g"),
            "excluded npm globals must be commented out: {output}"
        );
        assert!(
            !output.contains("\nRUN npm install -g"),
            "excluded npm globals must not produce active RUN: {output}"
        );
    }

    #[test]
    fn npm_global_c_extension_warning() {
        let mut item = npm_global_item(
            "/usr/local/lib/node_modules",
            HIGH_CONFIDENCE,
            vec![("node-sass", "8.0.0", false)],
        );
        item.has_c_extensions = true;

        let snap = test_snap(vec![item], &["nodejs"]);
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains("WARNING: environment contains native addons"),
            "must warn about native addons: {output}"
        );
        assert!(
            output.contains("build tools (gcc, node-gyp) may be needed"),
            "must mention build tools: {output}"
        );
    }

    #[test]
    fn npm_global_pinned_field_does_not_affect_pip_gem() {
        // Verify that existing pip/gem rendering is unchanged by the pinned field.
        // Use medium-confidence pip (dist-info) which renders package list inline.
        let pip_item = NonRpmItem {
            path: "/usr/lib/python3.9/site-packages".into(),
            name: "system-pip".into(),
            method: METHOD_PIP_DIST_INFO.into(),
            confidence: MEDIUM_CONFIDENCE.into(),
            disposition: FindingKind::excluded(),
            packages: vec![
                LanguagePackage {
                    name: "flask".into(),
                    version: "2.3.3".into(),
                    pinned: false,
                },
                LanguagePackage {
                    name: "requests".into(),
                    version: "2.31.0".into(),
                    pinned: false,
                },
            ],
            rpm_filtered: true,
            ..Default::default()
        };

        let snap = test_snap(
            vec![pip_item, gem_item("/opt/gemapp", HIGH_CONFIDENCE)],
            &["python3", "rubygems"],
        );
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        // pip should still use == syntax, not @
        assert!(
            output.contains("flask==2.3.3"),
            "pip must use == syntax: {output}"
        );
        assert!(
            !output.contains("flask@2.3.3"),
            "pip must not use @ syntax: {output}"
        );

        // gem should still use bundle install, not version pins
        assert!(
            output.contains("bundle install"),
            "gem must use bundle install: {output}"
        );
    }

    // ---------------------------------------------------------------------------
    // Shell injection safety tests
    // ---------------------------------------------------------------------------

    #[test]
    fn pip_path_with_spaces_renders_with_quotes() {
        let snap = test_snap(
            vec![pip_venv_item(
                "/opt/my app/venv",
                HIGH_CONFIDENCE,
                true,
                vec![("flask", "2.3.3")],
            )],
            &["python3"],
        );
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains("RUN python3 -m venv '/opt/my app/venv'"),
            "path with spaces must be single-quoted: {output}"
        );
        assert!(
            output.contains("'/opt/my app/venv'/bin/pip install"),
            "path in pip install must be quoted: {output}"
        );
    }

    #[test]
    fn pip_path_with_single_quote_is_rejected() {
        let snap = test_snap(
            vec![pip_venv_item(
                "/opt/user's/venv",
                HIGH_CONFIDENCE,
                true,
                vec![("flask", "2.3.3")],
            )],
            &["python3"],
        );
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains("WARNING: path contains unsafe characters, skipping"),
            "path with single quote must be rejected: {output}"
        );
        assert!(
            !output.contains("RUN python3 -m venv"),
            "rejected path must not render RUN: {output}"
        );
        assert!(
            output.contains("/opt/user''s/venv"),
            "sanitized path must escape single quotes: {output}"
        );
    }

    #[test]
    fn npm_path_with_newline_is_rejected() {
        let snap = test_snap(
            vec![npm_item("/opt/app\ndir", HIGH_CONFIDENCE)],
            &["nodejs"],
        );
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains("WARNING: path contains unsafe characters, skipping"),
            "path with newline must be rejected: {output}"
        );
        assert!(
            !output.contains("RUN cd"),
            "rejected path must not render RUN: {output}"
        );
        assert!(
            output.contains(r"\n"),
            "sanitized path must escape newlines: {output}"
        );
    }

    #[test]
    fn npm_global_package_with_shell_metachar_is_rejected() {
        let snap = test_snap(
            vec![npm_global_item(
                "/usr/local/lib/node_modules",
                HIGH_CONFIDENCE,
                vec![("pm2", "5.3.0", false), ("evil';rm -rf /", "1.0.0", false)],
            )],
            &["nodejs"],
        );
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains("WARNING: 1 package(s) skipped due to unsafe characters"),
            "must warn about rejected packages: {output}"
        );
        assert!(
            output.contains("RUN npm install -g pm2"),
            "safe package must still render: {output}"
        );
        assert!(
            !output.contains("evil"),
            "unsafe package must not appear in command: {output}"
        );
    }

    /// Package names land in `RUN npm install -g <list>` unquoted, so a
    /// metacharacter there is a command-injection vector, not a cosmetic
    /// problem. Every character in `UNSAFE_SHELL_CHARS` must be rejected,
    /// and ordinary package-name punctuation must survive.
    #[test]
    fn npm_global_rejects_every_unsafe_shell_char_in_package_names() {
        let rejected = [
            ("single quote", "evil'name"),
            ("newline", "evil\nname"),
            ("semicolon", "evil;rm -rf /"),
            ("dollar", "evil$(id)"),
            ("backtick", "evil`id`"),
            ("pipe", "evil|id"),
            ("ampersand", "evil&id"),
        ];
        for (label, name) in rejected {
            let snap = test_snap(
                vec![npm_global_item(
                    "/usr/local/lib/node_modules",
                    HIGH_CONFIDENCE,
                    vec![("pm2", "5.3.0", false), (name, "1.0.0", false)],
                )],
                &["nodejs"],
            );
            let output = language_package_lines(&snap).join("\n");

            assert!(
                output.contains("WARNING: 1 package(s) skipped due to unsafe characters"),
                "{label}: must warn about the rejected package: {output}"
            );
            assert!(
                !output.contains("evil"),
                "{label}: rejected package must not reach the RUN line: {output}"
            );
            assert!(
                output.contains("RUN npm install -g pm2"),
                "{label}: the safe package must still render: {output}"
            );
        }

        // Ordinary package-name punctuation must not trip the guard.
        let accepted = ["@angular/cli", "node-sass", "socket.io", "left_pad"];
        for name in accepted {
            let snap = test_snap(
                vec![npm_global_item(
                    "/usr/local/lib/node_modules",
                    HIGH_CONFIDENCE,
                    vec![(name, "1.0.0", false)],
                )],
                &["nodejs"],
            );
            let output = language_package_lines(&snap).join("\n");
            assert!(
                output.contains(&format!("RUN npm install -g {name}")),
                "{name}: safe package must render: {output}"
            );
            assert!(
                !output.contains("skipped due to unsafe characters"),
                "{name}: safe package must not be rejected: {output}"
            );
        }
    }

    #[test]
    fn pip_rejects_every_unsafe_shell_char_in_paths() {
        let rejected = [
            ("single quote", "/opt/ev'il/venv"),
            ("newline", "/opt/ev\nil/venv"),
            ("semicolon", "/opt/ev;il/venv"),
            ("dollar", "/opt/ev$il/venv"),
            ("backtick", "/opt/ev`il/venv"),
            ("pipe", "/opt/ev|il/venv"),
            ("ampersand", "/opt/ev&il/venv"),
        ];
        for (label, path) in rejected {
            let snap = test_snap(
                vec![pip_venv_item(
                    path,
                    HIGH_CONFIDENCE,
                    true,
                    vec![("flask", "2.3.3")],
                )],
                &["python3"],
            );
            let output = language_package_lines(&snap).join("\n");

            assert!(
                output.contains("WARNING: path contains unsafe characters, skipping"),
                "{label}: path must be rejected: {output}"
            );
            assert!(
                !output.contains("RUN python3 -m venv"),
                "{label}: rejected path must not render a RUN line: {output}"
            );
        }

        // Spaces and other benign path characters stay renderable.
        let snap = test_snap(
            vec![pip_venv_item(
                "/opt/my app-1.0/venv",
                HIGH_CONFIDENCE,
                true,
                vec![("flask", "2.3.3")],
            )],
            &["python3"],
        );
        let output = language_package_lines(&snap).join("\n");
        assert!(
            output.contains("RUN python3 -m venv '/opt/my app-1.0/venv'"),
            "benign path must still render: {output}"
        );
    }

    #[test]
    fn gem_path_with_spaces_renders_with_quotes() {
        let snap = test_snap(
            vec![gem_item("/opt/my project", HIGH_CONFIDENCE)],
            &["rubygems"],
        );
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        assert!(
            output.contains("COPY language-packages/gem/"),
            "must COPY gem files: {output}"
        );
        assert!(
            output.contains("'/opt/my project'/Gemfile"),
            "path with spaces must be quoted in COPY: {output}"
        );
        assert!(
            output.contains("RUN cd '/opt/my project'"),
            "path with spaces must be quoted in RUN: {output}"
        );
    }

    #[test]
    fn normal_paths_still_render_correctly() {
        let snap = test_snap(
            vec![
                pip_venv_item(
                    "/opt/app/venv",
                    HIGH_CONFIDENCE,
                    true,
                    vec![("flask", "2.3.3")],
                ),
                npm_item("/opt/webapp", HIGH_CONFIDENCE),
                gem_item("/opt/gemapp", HIGH_CONFIDENCE),
            ],
            &["python3", "nodejs", "rubygems"],
        );
        let lines = language_package_lines(&snap);
        let output = lines.join("\n");

        // All should render without warnings.
        assert!(
            !output.contains("WARNING: path contains unsafe"),
            "normal paths must not trigger warnings: {output}"
        );
        // All should render with quotes (defensive).
        assert!(
            output.contains("RUN python3 -m venv '/opt/app/venv'"),
            "pip path must be quoted: {output}"
        );
        assert!(
            output.contains("RUN cd '/opt/webapp'"),
            "npm path must be quoted: {output}"
        );
        assert!(
            output.contains("RUN cd '/opt/gemapp'"),
            "gem path must be quoted: {output}"
        );
    }

    #[test]
    fn npm_global_runtime_check() {
        // Test 1: nodejs absent from RPM list → warning emitted
        let snap_no_nodejs = test_snap(
            vec![npm_global_item(
                "/usr/local/lib/node_modules",
                HIGH_CONFIDENCE,
                vec![("pm2", "5.3.0", false)],
            )],
            &["httpd"], // some RPM but not nodejs
        );
        let lines = language_package_lines(&snap_no_nodejs);
        let output = lines.join("\n");

        assert!(
            output.contains("WARNING: nodejs not found in RPM package list"),
            "must warn when nodejs absent: {output}"
        );

        // Test 2: nodejs present → no warning
        let snap_with_nodejs = test_snap(
            vec![npm_global_item(
                "/usr/local/lib/node_modules",
                HIGH_CONFIDENCE,
                vec![("pm2", "5.3.0", false)],
            )],
            &["nodejs"],
        );
        let lines_with_nodejs = language_package_lines(&snap_with_nodejs);
        let output_with_nodejs = lines_with_nodejs.join("\n");

        assert!(
            !output_with_nodejs.contains("WARNING: nodejs not found"),
            "must NOT warn when nodejs present: {output_with_nodejs}"
        );

        // Test 3: no RPM data at all → no warning
        let snap_no_rpm = test_snap(
            vec![npm_global_item(
                "/usr/local/lib/node_modules",
                HIGH_CONFIDENCE,
                vec![("pm2", "5.3.0", false)],
            )],
            &[], // no RPM data
        );
        let lines_no_rpm = language_package_lines(&snap_no_rpm);
        let output_no_rpm = lines_no_rpm.join("\n");

        assert!(
            !output_no_rpm.contains("WARNING: nodejs not found"),
            "must NOT warn when no RPM data: {output_no_rpm}"
        );
    }
}
