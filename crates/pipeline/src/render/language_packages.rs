//! Language package Containerfile rendering — pip/npm/gem sections.
//!
//! Replaces advisory stubs with executable COPY/RUN instructions for
//! language environment items. High-confidence items render as active
//! instructions; medium-confidence renders commented-out.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::nonrpm::NonRpmItem;
use inspectah_core::util::{
    METHOD_GEM_LOCKFILE, METHOD_GEM_SYSTEM, METHOD_NPM_LOCKFILE, METHOD_NPM_MANIFEST,
    METHOD_PIP_DIST_INFO, METHOD_PYTHON_VENV, env_hash,
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

/// Returns true if the item is a gem environment (lockfile or system).
fn is_gem_env(item: &NonRpmItem) -> bool {
    item.method == METHOD_GEM_LOCKFILE || item.method == METHOD_GEM_SYSTEM
}

/// Returns true if the item is a language environment handled by this module.
pub fn is_language_env(item: &NonRpmItem) -> bool {
    is_pip_env(item) || is_npm_env(item) || is_gem_env(item)
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
    let gem_items: Vec<&NonRpmItem> = nrs.items.iter().filter(|i| is_gem_env(i)).collect();

    if pip_items.is_empty() && npm_items.is_empty() && gem_items.is_empty() {
        return Vec::new();
    }

    let mut lines = Vec::new();

    if !pip_items.is_empty() {
        lines.extend(render_pip_section(&pip_items, &rpm_names));
    }
    if !npm_items.is_empty() {
        lines.extend(render_npm_section(&npm_items, &rpm_names));
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
    let effective_confidence = if !item.include {
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

    // Normalize path to absolute — the collector strips the leading slash
    // before storing. npm/gem renderers already do this; pip must too.
    let abs_path = format!("/{}", item.path.trim_start_matches('/'));

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
        lines.push(format!("RUN python3 -m venv {abs_path} \\"));
        lines.push(format!(
            "    && {abs_path}/bin/pip install -r /tmp/{venv_name}-requirements.txt \\"
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
        lines.push(format!("# RUN python3 -m venv {abs_path} \\"));
        if pkgs.is_empty() {
            lines.push(format!("#     && {abs_path}/bin/pip install <packages>"));
        } else {
            lines.push(format!("#     && {abs_path}/bin/pip install {pkgs}"));
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
        let project_path = format!("/{}", item.path.trim_start_matches('/'));
        lines.push(format!(
            "# npm packages: {project_path} (low confidence — review required)"
        ));
        return lines;
    }

    // Excluded items never render as active, even at high confidence.
    let effective_confidence = if !item.include {
        MEDIUM_CONFIDENCE
    } else {
        item.confidence.as_str()
    };

    let hash = env_hash(&item.path);
    let project_path = format!("/{}", item.path.trim_start_matches('/'));

    if effective_confidence == HIGH_CONFIDENCE {
        lines.push(format!(
            "# npm packages: {project_path} (from package-lock.json)"
        ));
        lines.push(format!(
            "COPY language-packages/npm/{hash}/package.json {project_path}/package.json"
        ));
        lines.push(format!(
            "COPY language-packages/npm/{hash}/package-lock.json \
             {project_path}/package-lock.json"
        ));
        lines.push(format!("RUN cd {project_path} && npm ci --production"));
    } else {
        // Medium confidence: commented out.
        lines.push(format!(
            "# npm packages: {project_path} (detected via package-lock.json)"
        ));
        lines.push("# Uncomment after verifying package list is complete:".into());
        lines.push(format!(
            "# COPY language-packages/npm/{hash}/package.json {project_path}/package.json"
        ));
        lines.push(format!(
            "# COPY language-packages/npm/{hash}/package-lock.json \
             {project_path}/package-lock.json"
        ));
        lines.push(format!("# RUN cd {project_path} && npm ci --production"));
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
        let project_path = format!("/{}", item.path.trim_start_matches('/'));
        lines.push(format!(
            "# gem packages: {project_path} (low confidence — review required)"
        ));
        return lines;
    }

    // Excluded items never render as active, even at high confidence.
    let effective_confidence = if !item.include {
        MEDIUM_CONFIDENCE
    } else {
        item.confidence.as_str()
    };

    let hash = env_hash(&item.path);
    let project_path = format!("/{}", item.path.trim_start_matches('/'));

    if effective_confidence == HIGH_CONFIDENCE {
        lines.push(format!(
            "# gem packages: {project_path} (from Gemfile.lock)"
        ));
        lines.push(format!(
            "COPY language-packages/gem/{hash}/Gemfile {project_path}/Gemfile"
        ));
        lines.push(format!(
            "COPY language-packages/gem/{hash}/Gemfile.lock {project_path}/Gemfile.lock"
        ));
        lines.push(format!(
            "RUN cd {project_path} && bundle install --deployment"
        ));
    } else {
        // Medium confidence: commented out.
        lines.push(format!(
            "# gem packages: {project_path} (detected via Gemfile.lock)"
        ));
        lines.push("# Uncomment after verifying package list is complete:".into());
        lines.push(format!(
            "# COPY language-packages/gem/{hash}/Gemfile {project_path}/Gemfile"
        ));
        lines.push(format!(
            "# COPY language-packages/gem/{hash}/Gemfile.lock {project_path}/Gemfile.lock"
        ));
        lines.push(format!(
            "# RUN cd {project_path} && bundle install --deployment"
        ));
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::nonrpm::{LanguagePackage, NonRpmSoftwareSection};
    use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
    use std::collections::HashMap;

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
                        include: true,
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
            include: confidence == HIGH_CONFIDENCE,
            packages: packages
                .iter()
                .map(|(n, v)| LanguagePackage {
                    name: n.to_string(),
                    version: v.to_string(),
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
            include: confidence == HIGH_CONFIDENCE,
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
            include: confidence == HIGH_CONFIDENCE,
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
            output.contains("RUN python3 -m venv /opt/myapp/venv"),
            "must create venv: {output}"
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
            output.contains("bundle install --deployment"),
            "must run bundle install: {output}"
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
        item.include = false;

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
            include: false,
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
                include: true,
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
            include: false,
            packages: vec![
                LanguagePackage {
                    name: "flask".into(),
                    version: "2.3.3".into(),
                },
                LanguagePackage {
                    name: "requests".into(),
                    version: "2.31.0".into(),
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
        item.include = false;

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
            output.contains("RUN python3 -m venv /opt/myapp/venv"),
            "must normalize path to absolute: {output}"
        );
        assert!(
            !output.contains("RUN python3 -m venv opt/"),
            "must not use relative path: {output}"
        );
    }

    #[test]
    fn high_confidence_excluded_npm_renders_commented_out() {
        let mut item = npm_item("/opt/webapp", HIGH_CONFIDENCE);
        item.include = false;

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
}
