//! Language package Containerfile rendering — pip/npm/gem sections.
//!
//! Replaces advisory stubs with executable COPY/RUN instructions for
//! language environment items. High-confidence items render as active
//! instructions; medium-confidence renders commented-out.

use std::path::Path;

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::RenderError;
use inspectah_core::types::nonrpm::NonRpmItem;
use inspectah_core::types::redaction::RedactionState;
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
///
/// The result only ever lands in commented-out `RUN` lines, so it is
/// sanitized for a comment context rather than rejected: a newline in a
/// package name would otherwise close the comment and turn the rest of
/// the name into an active instruction. pip names come from `.dist-info`
/// directory names, which an app-owned venv's service account can write.
fn pinned_package_list(item: &NonRpmItem) -> String {
    let joined = item
        .packages
        .iter()
        .map(|p| {
            if p.version.is_empty() {
                p.name.clone()
            } else {
                format!("{}=={}", p.name, p.version)
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    sanitize_for_comment(&joined)
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
pub(super) fn sanitize_for_comment(s: &str) -> String {
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
        // The directory name is interpolated into `/tmp/<name>-requirements.txt`,
        // which is a separate token from the venv path and gets no quoting
        // from it. Every site below single-quotes it: unquoted, a plain
        // space in the name gives `COPY` three arguments, and any shell
        // metacharacter reaching the `RUN` lines would execute. Quoting it
        // is also what makes narrowing the path predicate safe — do not
        // narrow `UNSAFE_SHELL_CHARS` while these sites are bare.
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
            "COPY language-packages/pip/{hash}/requirements.txt '/tmp/{venv_name}-requirements.txt'"
        ));
        let venv_flags = if item.system_site_packages {
            "--system-site-packages "
        } else {
            ""
        };
        lines.push(format!("RUN python3 -m venv {venv_flags}'{abs_path}' \\"));
        lines.push(format!(
            "    && '{abs_path}'/bin/pip install -r '/tmp/{venv_name}-requirements.txt' \\"
        ));
        lines.push(format!("    && rm '/tmp/{venv_name}-requirements.txt'"));
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
    // The prefix is never part of a command, only of the comment that
    // labels the section — so it is sanitized rather than rejected. Left
    // raw, a newline in it closes the comment and emits an active `RUN`.
    let prefix = sanitize_for_comment(&absolute_path(&item.path));

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

// ---------------------------------------------------------------------------
// Manifest staging
//
// The writer lives beside the renderer that emits its COPY lines. Both
// export paths call it, because both export paths render those lines:
// keeping the writer in one of them is what let the CLI scan tarball
// emit `COPY language-packages/...` for a directory it never wrote.
// ---------------------------------------------------------------------------

/// Materialize collected manifest files into the export directory under
/// `language-packages/<ecosystem>/<hash>/`. Only included items with
/// non-empty manifest_files are materialized — excluded items use
/// commented-out inline installs in the Containerfile, not COPY paths.
pub fn write_language_package_manifests(
    snap: &InspectionSnapshot,
    out: &Path,
) -> Result<(), RenderError> {
    let nrs = match &snap.non_rpm_software {
        Some(n) => n,
        None => return Ok(()),
    };

    let redact = is_redaction_active(snap);

    for item in &nrs.items {
        if !item.disposition.is_included() || item.manifest_files.is_empty() {
            continue;
        }

        // Route to ecosystem subdirectory based on the method string —
        // the canonical detection-method key used throughout the pipeline.
        let ecosystem = if item.method == METHOD_PYTHON_VENV || item.method == METHOD_PIP_DIST_INFO
        {
            "pip"
        } else if item.method == METHOD_NPM_LOCKFILE {
            "npm"
        } else if item.method == METHOD_GEM_LOCKFILE {
            "gem"
        } else {
            continue;
        };

        let hash = env_hash(&item.path);
        let dir = out.join("language-packages").join(ecosystem).join(&hash);
        std::fs::create_dir_all(&dir)
            .map_err(|e| RenderError::Failed(format!("mkdir {}: {e}", dir.display())))?;

        for (filename, content) in &item.manifest_files {
            let output = if redact {
                scrub_manifest_auth(filename, content)
            } else {
                content.clone()
            };
            let file_path = dir.join(filename);
            std::fs::write(&file_path, output)
                .map_err(|e| RenderError::Failed(format!("write {}: {e}", file_path.display())))?;
        }
    }
    Ok(())
}

/// Returns `true` when the snapshot has an active redaction state that
/// warrants scrubbing secrets from exported content.
pub fn is_redaction_active(snap: &InspectionSnapshot) -> bool {
    matches!(
        &snap.redaction_state,
        Some(
            RedactionState::FullyRedacted { .. }
                | RedactionState::PartiallyRedacted { .. }
                | RedactionState::SensitiveRetained { .. }
        )
    )
}

/// Characters that end the authority component of a URL, per RFC 3986:
/// the path, query and fragment delimiters. An `@` after any of these
/// belongs to the path or query, not to the userinfo.
const AUTHORITY_TERMINATORS: [char; 3] = ['/', '?', '#'];

/// Scrub embedded auth credentials from a URL string.
/// Replaces `://user:pass@host` with `://REDACTED@host` and
/// `://token@host` with `://REDACTED@host`.
fn scrub_url_auth(url: &str) -> String {
    // Find the authority section: everything between :// and the end of
    // the authority. If there's no ://, or no @ in the authority, the URL
    // has no embedded auth.
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let authority_start = scheme_end + 3;
    let rest = &url[authority_start..];

    // RFC 3986: userinfo runs to the LAST `@` before the authority ends.
    // A password may legitimately contain an `@`, and real clients read it
    // that way (pip masks `deploy:p@s3cret` whole). Taking the first `@`
    // instead left the tail of such a password in the output. Bounding the
    // search by the terminator first is what makes `rfind` safe: an `@`
    // in a query string must not be mistaken for the end of userinfo.
    let authority_end = rest.find(AUTHORITY_TERMINATORS).unwrap_or(rest.len());
    let Some(at_pos) = rest[..authority_end].rfind('@') else {
        return url.to_string();
    };

    format!("{}://REDACTED@{}", &url[..scheme_end], &rest[at_pos + 1..])
}

/// Scrub auth tokens from manifest file content based on the filename.
///
/// - `requirements.txt`: `--index-url` / `--extra-index-url` lines with auth
/// - `package.json`: `"registry"` URLs with embedded auth
/// - `package-lock.json`: `"resolved"` URLs with embedded auth
/// - `Gemfile`: `source` URLs with embedded auth
/// - `Gemfile.lock`: any `https://user:pass@host` URLs in raw text
pub fn scrub_manifest_auth(filename: &str, content: &str) -> String {
    match filename {
        "requirements.txt" => scrub_requirements_txt(content),
        "package.json" => scrub_package_json(content),
        "package-lock.json" => scrub_package_lock_json(content),
        "Gemfile" => scrub_gemfile(content),
        "Gemfile.lock" => scrub_gemfile_lock(content),
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
    // Both slices already carry their quote: the prefix ends with the opening
    // quote and the suffix starts with the closing one. Re-adding either here
    // emits `""url""` and makes the manifest unparseable.
    format!(
        "{}{}{}",
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

/// Scrub `"resolved"` URLs with embedded auth in package-lock.json.
fn scrub_package_lock_json(content: &str) -> String {
    content
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.contains("\"resolved\"") {
                scrub_json_url_value(line)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Scrub any `https://user:pass@host` URLs found in Gemfile.lock.
fn scrub_gemfile_lock(content: &str) -> String {
    content
        .lines()
        .map(|line| {
            if line.contains("://") && line.contains('@') {
                let mut result = line.to_string();
                if let Some(scheme_pos) = result.find("://") {
                    let authority_start = scheme_pos + 3;
                    let rest = &result[authority_start..];
                    if let Some(at_pos) = rest.find('@') {
                        let slash_pos = rest.find('/').unwrap_or(rest.len());
                        if at_pos < slash_pos {
                            let url_start = result[..scheme_pos]
                                .rfind(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                                .map(|p| p + 1)
                                .unwrap_or(0);
                            let url_end = result[authority_start + at_pos + 1..]
                                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                                .map(|p| authority_start + at_pos + 1 + p)
                                .unwrap_or(result.len());
                            let url = &result[url_start..url_end];
                            let scrubbed = scrub_url_auth(url);
                            result = format!(
                                "{}{}{}",
                                &result[..url_start],
                                scrubbed,
                                &result[url_end..]
                            );
                        }
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

    /// A redacted export must still write manifests the package manager can
    /// read. Assert on the staged file parsing as JSON, not on a substring:
    /// the quote-doubling defect shipped past a suite that only checked that
    /// the credential was gone and `REDACTED@host` was present.
    #[test]
    fn redacted_json_manifests_stage_as_parseable_json() {
        let lockfile = r#"{
  "name": "webapp",
  "lockfileVersion": 3,
  "packages": {
    "node_modules/private": {
      "version": "1.2.3",
      "resolved": "https://deploy:s3cret@npm.corp.example.com/private/-/private-1.2.3.tgz",
      "integrity": "sha512-abc123"
    },
    "node_modules/public": {
      "version": "4.5.6",
      "resolved": "https://registry.npmjs.org/public/-/public-4.5.6.tgz",
      "integrity": "sha512-def456"
    }
  }
}"#;
        let package_json = r#"{
  "name": "webapp",
  "publishConfig": {
    "registry": "https://deploy:s3cret@npm.corp.example.com/"
  }
}"#;

        let mut manifest_files = HashMap::new();
        manifest_files.insert("package-lock.json".to_string(), lockfile.to_string());
        manifest_files.insert("package.json".to_string(), package_json.to_string());
        let item = NonRpmItem {
            path: "/opt/webapp".into(),
            name: "webapp".into(),
            method: METHOD_NPM_LOCKFILE.into(),
            confidence: HIGH_CONFIDENCE.into(),
            disposition: FindingKind::included(),
            manifest_files,
            ..Default::default()
        };

        let mut snap = test_snap(vec![item], &[]);
        snap.redaction_state = Some(RedactionState::FullyRedacted {
            redacted_by: "test".into(),
            config_hash: "deadbeef".into(),
        });

        let tmp = tempfile::tempdir().expect("tempdir");
        write_language_package_manifests(&snap, tmp.path()).expect("manifests must be written");

        let dir = tmp
            .path()
            .join("language-packages")
            .join("npm")
            .join(env_hash("/opt/webapp"));
        for filename in ["package-lock.json", "package.json"] {
            let staged = std::fs::read_to_string(dir.join(filename))
                .unwrap_or_else(|e| panic!("{filename} must be staged: {e}"));
            serde_json::from_str::<serde_json::Value>(&staged).unwrap_or_else(|e| {
                panic!("staged {filename} must be parseable JSON: {e}\n{staged}")
            });
            assert!(
                !staged.contains("s3cret"),
                "credential must be scrubbed from {filename}:\n{staged}"
            );
            assert!(
                staged.contains("REDACTED@npm.corp.example.com"),
                "scrubbed URL must keep its host in {filename}:\n{staged}"
            );
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

    /// The venv directory name lands in `/tmp/<name>-requirements.txt`,
    /// which sits outside the quotes the path itself gets. Unquoted, a
    /// plain space gives `COPY` three arguments and hands `pip install -r`
    /// a stray token: a broken build from a legal path. The existing
    /// `pip_path_with_spaces_renders_with_quotes` misses this because its
    /// space is in a parent directory, so the name comes out clean.
    ///
    /// The negative assertion is also the guard for the deferred narrowing
    /// of `UNSAFE_SHELL_CHARS` to `'` and `\n`. Names carrying `$` or `;`
    /// are rejected by today's path predicate, so nothing renders for them
    /// and the assertion holds trivially; once the predicate narrows they
    /// render, and the assertion starts proving that `$(id)` cannot reach
    /// a `RUN` line unquoted. Quoting has to land before the narrowing,
    /// never after.
    #[test]
    fn venv_directory_name_is_quoted_at_its_own_interpolation_sites() {
        for (name, renders_under_current_predicate) in [
            ("my venv", true),
            ("venv(x)", true),
            ("venv>out", true),
            ("venv*", true),
            ("ve$(id)nv", false),
            ("venv;id", false),
        ] {
            let path = format!("/opt/app/{name}");
            let snap = test_snap(
                vec![pip_venv_item(
                    &path,
                    HIGH_CONFIDENCE,
                    true,
                    vec![("flask", "2.3.3")],
                )],
                &["python3"],
            );
            let output = language_package_lines(&snap).join("\n");

            assert!(
                !output.contains(&format!(" /tmp/{name}")),
                "{name}: the temp path must never be interpolated unquoted: {output}"
            );

            if renders_under_current_predicate {
                for site in [
                    format!("requirements.txt '/tmp/{name}-requirements.txt'"),
                    format!("pip install -r '/tmp/{name}-requirements.txt'"),
                    format!("rm '/tmp/{name}-requirements.txt'"),
                ] {
                    assert!(
                        output.contains(&site),
                        "{name}: expected quoted interpolation {site:?}: {output}"
                    );
                }
            }
        }
    }

    // ---------------------------------------------------------------------------
    // Comment-escape tests
    //
    // Values that only ever land in comments still reach the build if they
    // carry a newline: the newline closes the comment and the remainder
    // becomes an active instruction. These tests assert on the rendered
    // output, because the output is the contract.
    // ---------------------------------------------------------------------------

    /// The instructions a build would actually execute: everything that is
    /// neither a comment nor a blank separator.
    fn active_lines(output: &str) -> Vec<&str> {
        output
            .lines()
            .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
            .collect()
    }

    /// The npm-global prefix is interpolated into a comment line and was
    /// guarded by nothing at all — not the path predicate the other
    /// renderers apply, not the token guard applied to the package list
    /// beside it. A newline in it emits an arbitrary active `RUN`.
    #[test]
    fn npm_global_prefix_newline_cannot_emit_an_active_instruction() {
        const INJECTED: &str = "RUN echo pwned-via-prefix";
        for confidence in [HIGH_CONFIDENCE, MEDIUM_CONFIDENCE] {
            let snap = test_snap(
                vec![npm_global_item(
                    &format!("/usr/local/lib/node_modules\n{INJECTED}"),
                    confidence,
                    vec![("pm2", "5.3.0", false)],
                )],
                &["nodejs"],
            );
            let output = language_package_lines(&snap).join("\n");

            for line in active_lines(&output) {
                assert!(
                    line.starts_with("RUN npm install -g "),
                    "{confidence}: the npm install is the only instruction this \
                     renderer may emit, got {line:?}: {output}"
                );
            }
            assert!(
                output.contains(&format!(r"\n{INJECTED}")),
                "{confidence}: the newline must be escaped into the comment: {output}"
            );
        }
    }

    /// `pinned_package_list` joins raw package names into commented-out
    /// `RUN` lines. pip package names come from `.dist-info` directory
    /// names, and app-owned venvs under `/opt` are routinely writable by a
    /// non-root service account, so the value is attacker-influenced.
    #[test]
    fn pinned_package_newline_cannot_emit_an_active_instruction() {
        const INJECTED: &str = "RUN echo pwned-via-package-name";
        let evil_name = format!("flask\n{INJECTED}");

        // The system-level branch (`# RUN pip install ...`) and the
        // medium-confidence venv branch (`#     && .../bin/pip install ...`)
        // share the helper, so both sites need covering.
        let system_pip = NonRpmItem {
            path: "/usr/lib/python3.9/site-packages".into(),
            name: "system-pip".into(),
            method: METHOD_PIP_DIST_INFO.into(),
            confidence: MEDIUM_CONFIDENCE.into(),
            disposition: FindingKind::excluded(),
            packages: vec![LanguagePackage {
                name: evil_name.clone(),
                version: "2.3.3".into(),
                pinned: false,
            }],
            rpm_filtered: true,
            ..Default::default()
        };
        let venv = pip_venv_item(
            "/opt/app/venv",
            MEDIUM_CONFIDENCE,
            false,
            vec![(&evil_name, "2.3.3")],
        );

        for item in [system_pip, venv] {
            let method = item.method.clone();
            let snap = test_snap(vec![item], &["python3"]);
            let output = language_package_lines(&snap).join("\n");

            assert!(
                active_lines(&output).is_empty(),
                "{method}: a commented-out block must emit no active instruction: {output}"
            );
            assert!(
                output.contains(&format!(r"\n{INJECTED}")),
                "{method}: the newline must be escaped into the comment: {output}"
            );
        }
    }
}
