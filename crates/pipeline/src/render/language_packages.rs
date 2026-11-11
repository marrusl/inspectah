//! Language package Containerfile rendering — pip/npm/gem sections.
//!
//! Replaces advisory stubs with executable COPY/RUN instructions for
//! language environment items. High-confidence items render as active
//! instructions; medium-confidence renders commented-out.

use std::path::Path;

use inspectah_core::redaction::{ScrubOutcome, UnscrubbedLine, ValueSyntax, value_span_after_key};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::RenderError;
use inspectah_core::types::nonrpm::NonRpmItem;
use inspectah_core::types::redaction::RedactionState;
use inspectah_core::util::{
    METHOD_GEM_LOCKFILE, METHOD_GEM_SYSTEM, METHOD_NPM_GLOBAL, METHOD_NPM_LOCKFILE,
    METHOD_NPM_MANIFEST, METHOD_PIP_DIST_INFO, METHOD_PYTHON_VENV, env_hash,
};

use crate::build::extract::is_single_normal_component;

use super::comment::{LINE_TERMINATORS, escape_line_terminators};

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

/// Returns true if a token is unsafe to interpolate into a rendered `RUN`
/// line. Every shell interpolation in this module is single-quoted (paths,
/// the venv name, and each npm-global token), so only what breaks out of a
/// single-quoted literal or out of the line is refused: the quote itself,
/// and every character in [`LINE_TERMINATORS`]. The newline ends a
/// Containerfile instruction; the others are line breaks to a terminal or
/// editor, and what the build parser makes of them is unverified. Do not
/// interpolate a checked value anywhere unquoted: this check is safe only
/// because every site quotes.
fn contains_unsafe_chars(s: &str) -> bool {
    s.contains('\'') || s.contains(LINE_TERMINATORS)
}

/// Sanitize a value for display in Containerfile comments.
///
/// Doubles single quotes, then applies [`escape_line_terminators`].
pub(super) fn sanitize_for_comment(s: &str) -> String {
    escape_line_terminators(&s.replace('\'', "''"))
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
        // is also what makes the narrow path predicate safe: these sites
        // must stay quoted while `contains_unsafe_chars` admits metacharacters.
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

/// The two files `npm ci` needs. The active npm block copies both.
const NPM_MANIFESTS: [&str; 2] = ["package.json", "package-lock.json"];

/// Whether an npm project renders its active install, which is also when
/// its manifests are staged: included, high confidence, and holding both
/// manifests.
///
/// The last condition is checked here rather than left to the collector,
/// which now marks a project high only when it read both files. A snapshot
/// the previous binary wrote can hold a high-confidence item with the
/// lockfile alone, and nothing about its schema version says so; its
/// active `COPY` of `package.json` would name a file no export writes.
fn npm_item_is_active(item: &NonRpmItem) -> bool {
    item.disposition.is_included()
        && item.confidence == HIGH_CONFIDENCE
        && NPM_MANIFESTS
            .iter()
            .all(|name| item.manifest_files.contains_key(*name))
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

    // An excluded item never renders as active, even at high confidence,
    // and an active one holds both manifests, so both `COPY` lines below
    // have a staged source and `npm ci` has the pair it requires.
    if npm_item_is_active(item) {
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
        // Below high, or excluded: advice only. No export stages these
        // manifests, so the block names no `COPY` an operator could
        // uncomment into a build failure; it says what is missing instead.
        lines.push(format!(
            "# npm packages: {project_path} (detected via package-lock.json)"
        ));
        let missing: Vec<&str> = NPM_MANIFESTS
            .iter()
            .copied()
            .filter(|name| !item.manifest_files.contains_key(*name))
            .collect();
        if !missing.is_empty() {
            lines.push(format!(
                "# Not captured from the source host: {}",
                missing.join(", ")
            ));
        }
        lines.push(
            "# The export does not carry this project's manifests. Put package.json and".into(),
        );
        lines.push(format!(
            "# package-lock.json in '{project_path}' in the image, then uncomment:"
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
            // Shell injection safety: each token is single-quoted on the
            // `RUN` line, so only a token that could break out of the
            // quotes is refused.
            if contains_unsafe_chars(&p.name) || contains_unsafe_chars(&p.version) {
                None
            } else if p.pinned && !p.version.is_empty() {
                Some(format!("'{}@{}'", p.name, p.version))
            } else {
                Some(format!("'{}'", p.name))
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

    // Quoting stops the shell, not npm: a token such as `--prefix=/x`
    // would still parse as an npm option. The `--` in front of the list
    // ends option parsing, so every token is a package spec.
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
        lines.push(format!("RUN npm install -g -- {pkg_list}"));
    } else {
        lines.push(format!("# npm global packages: {prefix} ({method_label})"));
        lines.push(format!("# RUN npm install -g -- {pkg_list}"));
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
            // Stage only what an active `COPY` carries. Otherwise the npm
            // block is commented, so its manifests would ship in the
            // export and never reach the image.
            if !npm_item_is_active(item) {
                continue;
            }
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
            // The name is a snapshot key joined onto `dir`: an absolute
            // key would replace `dir` and `..` would leave it. Every name
            // the Containerfile copies is a plain file name, so a refused
            // one is never referenced and only this warning records it.
            if !is_single_normal_component(filename) {
                eprintln!(
                    "inspectah: warning: skipping language-package manifest for {}: \
                     {} is not a plain file name",
                    item.path.escape_debug(),
                    filename.escape_debug()
                );
                continue;
            }
            let output = if redact {
                scrub_manifest_auth(filename, content).into_content()
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

/// What one line's scrubber decided about the line it was given.
enum LineOutcome {
    /// Not a line this scrubber handles. No key matched.
    NotMine,
    /// Handled. The rewritten line, which may be identical.
    Rewritten(String),
    /// A key matched and routed this line here, and the value that key
    /// names could not be located. The line is emitted unchanged and
    /// the caller has to say so: this is the state that used to pass in
    /// silence under a `FullyRedacted` label.
    Unlocatable(&'static str),
}

/// Run a per-line scrubber over `content`, collecting the lines it was
/// routed to and could not handle.
///
/// Preserves a trailing newline. The previous `lines()` + `join("\n")`
/// dropped one from every manifest it touched, scrubbed or not, which is
/// why the detector in `redact()` cannot compare whole strings.
fn scrub_lines(content: &str, mut scrub: impl FnMut(&str) -> LineOutcome) -> ScrubOutcome {
    let mut out = String::with_capacity(content.len());
    let mut unscrubbed = Vec::new();
    let mut lines = content.lines().enumerate().peekable();
    while let Some((idx, line)) = lines.next() {
        match scrub(line) {
            LineOutcome::Rewritten(new_line) => out.push_str(&new_line),
            LineOutcome::NotMine => out.push_str(line),
            LineOutcome::Unlocatable(key) => {
                unscrubbed.push(UnscrubbedLine {
                    line: idx as u32 + 1,
                    key: key.to_string(),
                });
                out.push_str(line);
            }
        }
        if lines.peek().is_some() {
            out.push('\n');
        }
    }
    if content.ends_with('\n') {
        out.push('\n');
    }
    ScrubOutcome {
        content: out,
        unscrubbed,
    }
}

/// Rebuild `line` with `span` replaced, leaving every other byte alone.
///
/// Every scrubber reconstructs through here rather than through
/// `replace`, which rewrote every occurrence of the matched text on the
/// line instead of the one it had located.
fn replace_span(line: &str, span: std::ops::Range<usize>, with: &str) -> String {
    format!("{}{}{}", &line[..span.start], with, &line[span.end..])
}

/// Scrub auth tokens from manifest file content based on the filename.
///
/// - `requirements.txt`: `--index-url`, `--extra-index-url` and `-i`
/// - `package.json`: `"registry"` URLs with embedded auth
/// - `package-lock.json`: `"resolved"` URLs with embedded auth
/// - `Gemfile`: `source` URLs with embedded auth
/// - `Gemfile.lock`: every `scheme://user:pass@host` URL in raw text
pub fn scrub_manifest_auth(filename: &str, content: &str) -> ScrubOutcome {
    match filename {
        "requirements.txt" => scrub_lines(content, scrub_requirements_line),
        "package.json" => scrub_lines(content, |l| scrub_json_line(l, JSON_REGISTRY_KEY)),
        "package-lock.json" => scrub_lines(content, |l| scrub_json_line(l, JSON_RESOLVED_KEY)),
        "Gemfile" => scrub_lines(content, scrub_gemfile_line),
        "Gemfile.lock" => scrub_lines(content, scrub_gemfile_lock_line),
        _ => ScrubOutcome {
            content: content.to_string(),
            unscrubbed: Vec::new(),
        },
    }
}

/// pip index directives, longest first so a prefix match cannot claim a
/// longer directive's argument.
const PIP_INDEX_DIRECTIVES: [&str; 3] = ["--extra-index-url", "--index-url", "-i"];

/// Scrub the URL argument of a pip index directive.
///
/// `--index-url=URL` and `-i URL` are both valid requirements-file
/// syntax, confirmed against pip. The previous `splitn` on whitespace
/// returned `None` for the `=` form and the prefix test never matched
/// the short form, so each passed through untouched.
fn scrub_requirements_line(line: &str) -> LineOutcome {
    let trimmed = line.trim_start();
    let indent_len = line.len() - trimmed.len();
    let Some(directive) = PIP_INDEX_DIRECTIVES
        .iter()
        .find(|d| trimmed.starts_with(**d))
    else {
        return LineOutcome::NotMine;
    };
    match value_span_after_key(trimmed, directive.len(), ValueSyntax::OptionArgument) {
        Some(span) => {
            let scrubbed = scrub_url_auth(&trimmed[span.clone()]);
            LineOutcome::Rewritten(format!(
                "{}{}",
                &line[..indent_len],
                replace_span(trimmed, span, &scrubbed)
            ))
        }
        None => LineOutcome::Unlocatable(directive),
    }
}

const JSON_REGISTRY_KEY: &str = "\"registry\"";
const JSON_RESOLVED_KEY: &str = "\"resolved\"";

/// Scrub the URL value of a JSON member whose key is `key`.
///
/// The value is located from the end of the matched key, not from the
/// line's first colon. A compact `"publishConfig": { "registry": "…" }`
/// put the key-value colon of a *different* member first on the line,
/// so the old search took `publishConfig`'s colon, read `registry` as
/// the value, found no `://` in it and gave up.
fn scrub_json_line(line: &str, key: &'static str) -> LineOutcome {
    let Some(key_pos) = line.find(key) else {
        return LineOutcome::NotMine;
    };
    match value_span_after_key(line, key_pos + key.len(), ValueSyntax::JsonMember) {
        Some(span) => {
            let scrubbed = scrub_url_auth(&line[span.clone()]);
            LineOutcome::Rewritten(replace_span(line, span, &scrubbed))
        }
        None => LineOutcome::Unlocatable(key),
    }
}

const GEMFILE_SOURCE_KEY: &str = "source";

/// Scrub the URL argument of a Gemfile `source` line.
///
/// The quoted run has to start at the first non-whitespace character
/// after `source`, and it ends at a quote of the *same* kind. The old
/// loop tried `"` before `'` and broke on the first quote character
/// anywhere on the line, so `source 'url' # see "runbook"` extracted
/// `runbook`.
fn scrub_gemfile_line(line: &str) -> LineOutcome {
    let trimmed = line.trim_start();
    let indent_len = line.len() - trimmed.len();
    if !trimmed.starts_with(GEMFILE_SOURCE_KEY) {
        return LineOutcome::NotMine;
    }
    // `source` must be a whole word here, or `sources = [...]` would be
    // routed to a scrubber and then reported as unlocatable.
    let after = &trimmed[GEMFILE_SOURCE_KEY.len()..];
    if !after.starts_with(|c: char| c.is_whitespace() || c == '"' || c == '\'') {
        return LineOutcome::NotMine;
    }
    match value_span_after_key(
        trimmed,
        GEMFILE_SOURCE_KEY.len(),
        ValueSyntax::QuotedArgument,
    ) {
        Some(span) => {
            let scrubbed = scrub_url_auth(&trimmed[span.clone()]);
            LineOutcome::Rewritten(format!(
                "{}{}",
                &line[..indent_len],
                replace_span(trimmed, span, &scrubbed)
            ))
        }
        None => LineOutcome::Unlocatable(GEMFILE_SOURCE_KEY),
    }
}

/// Byte ranges of every whitespace- or quote-delimited token on `line`
/// that carries a URL scheme.
fn url_spans(line: &str) -> Vec<std::ops::Range<usize>> {
    fn is_delimiter(c: char) -> bool {
        c.is_whitespace() || c == '"' || c == '\''
    }
    let mut spans = Vec::new();
    let mut start = 0usize;
    for (i, c) in line.char_indices() {
        if is_delimiter(c) {
            if start < i && line[start..i].contains("://") {
                spans.push(start..i);
            }
            start = i + c.len_utf8();
        }
    }
    if start < line.len() && line[start..].contains("://") {
        spans.push(start..line.len());
    }
    spans
}

/// Scrub every credentialed URL on a Gemfile.lock line.
///
/// The old version did a single `find("://")`, so a line carrying two
/// credentialed URLs kept the second one — and, once the marker
/// detector landed, reported success while doing it. Replacement runs
/// in descending offset order so an earlier rewrite cannot invalidate a
/// later span, the same discipline the regex engine uses.
fn scrub_gemfile_lock_line(line: &str) -> LineOutcome {
    let spans = url_spans(line);
    if spans.is_empty() {
        return LineOutcome::NotMine;
    }
    let mut out = line.to_string();
    for span in spans.into_iter().rev() {
        let scrubbed = scrub_url_auth(&line[span.clone()]);
        out.replace_range(span, &scrubbed);
    }
    LineOutcome::Rewritten(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::FindingKind;
    use inspectah_core::types::nonrpm::{LanguagePackage, NonRpmSoftwareSection};
    use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
    use std::collections::HashMap;

    /// Every line terminator is escaped to visible text, so comment text
    /// stays on one line in the file and in any viewer. Quotes are doubled
    /// only by `sanitize_for_comment`; everything else passes through.
    #[test]
    fn comment_escapers_leave_no_line_terminator() {
        let cases = [
            ("plain", "plain", "plain"),
            ("a\nb", "a\\nb", "a\\nb"),
            ("a\rb", "a\\rb", "a\\rb"),
            ("a\r\nb", "a\\r\\nb", "a\\r\\nb"),
            ("a\u{0B}b", "a\\u{b}b", "a\\u{b}b"),
            ("a\u{0C}b", "a\\u{c}b", "a\\u{c}b"),
            ("a\u{85}b", "a\\u{85}b", "a\\u{85}b"),
            ("a\u{2028}b", "a\\u{2028}b", "a\\u{2028}b"),
            ("a\u{2029}b", "a\\u{2029}b", "a\\u{2029}b"),
            ("it's", "it''s", "it's"),
            ("tab\there", "tab\there", "tab\there"),
            ("caf\u{e9}", "caf\u{e9}", "caf\u{e9}"),
        ];
        for (input, sanitized, escaped) in cases {
            assert_eq!(sanitize_for_comment(input), sanitized, "{input:?}");
            assert_eq!(escape_line_terminators(input), escaped, "{input:?}");
            assert!(
                !sanitized.contains(LINE_TERMINATORS),
                "{input:?}: expected value holds a terminator"
            );
        }
    }

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

    /// A snapshot the previous binary wrote can hold a high-confidence npm
    /// item with one manifest. It renders as below high whatever its
    /// confidence string says, and the writer stages nothing for it, so no
    /// active `COPY` names a file the export lacks.
    #[test]
    fn a_high_confidence_npm_item_missing_a_manifest_renders_below_high() {
        for missing in NPM_MANIFESTS {
            let mut item = npm_item("/opt/legacy", HIGH_CONFIDENCE);
            item.manifest_files.remove(missing);
            let snap = test_snap(vec![item], &["nodejs"]);
            let output = language_package_lines(&snap).join("\n");

            assert!(
                output
                    .lines()
                    .all(|l| l.starts_with('#') || l.trim().is_empty()),
                "{missing} missing: nothing may be active:\n{output}"
            );
            assert!(
                output.contains(&format!("# Not captured from the source host: {missing}")),
                "{missing} missing: the block names it:\n{output}"
            );

            let dir = tempfile::tempdir().expect("tempdir");
            write_language_package_manifests(&snap, dir.path()).expect("write");
            assert!(
                !dir.path().join("language-packages").exists(),
                "{missing} missing: nothing is staged"
            );
        }
    }

    /// The below-high block is advice the operator can follow: it carries
    /// no `COPY` of a manifest, commented or not, because no export stages
    /// one for it.
    #[test]
    fn a_below_high_npm_block_offers_no_copy_of_an_unstaged_manifest() {
        let snap = test_snap(
            vec![npm_item("/opt/webapp", MEDIUM_CONFIDENCE)],
            &["nodejs"],
        );
        let output = language_package_lines(&snap).join("\n");

        assert!(!output.contains("COPY"), "{output}");
        assert!(!output.contains("Not captured"), "both are held:\n{output}");
        assert!(
            output.contains("# RUN cd '/opt/webapp' && npm ci --production"),
            "{output}"
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
            output.contains("RUN npm install -g -- 'pm2' 'typescript'"),
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
            output.contains("RUN npm install -g -- 'pm2@5.3.0' 'typescript@5.4.2'"),
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
            output.contains("RUN npm install -g -- 'pm2@5.3.0' 'typescript'"),
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
            output.contains("RUN npm install -g -- '@angular/cli'"),
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
            output.contains("RUN npm install -g -- 'pm2'"),
            "safe package must still render: {output}"
        );
        assert!(
            !output.contains("evil"),
            "unsafe package must not appear in command: {output}"
        );
    }

    /// The npm-global install line interpolates host-supplied package
    /// names and versions into a `RUN`. A blacklist there is structurally
    /// incomplete (`>`, `<`, `(`, `*`, `{` and a bare space all passed it),
    /// so each token is single-quoted and this asserts on the rendered
    /// output, not on what the guard returns: the only active line is the
    /// install, carrying every token inside quotes. Inverting the old
    /// guard test to match the narrowed predicate, rather than quoting
    /// first, would ship a live command injection with a green suite.
    #[test]
    fn npm_global_tokens_cannot_escape_their_install_command() {
        let metachar_names = [
            "pm2;id",
            "pm2 rm -rf /",
            "pm2>/etc/passwd",
            "pm2<in",
            "pm2$(id)",
            "pm2`id`",
            "pm2|id",
            "pm2&id",
            "pm2*",
            "pm2(x)",
            "pm2{a,b}",
            "pm2#x",
            "pm2\tx",
        ];
        for evil in metachar_names {
            let snap = test_snap(
                vec![npm_global_item(
                    "/usr/local/lib/node_modules",
                    HIGH_CONFIDENCE,
                    vec![("pm2", "5.3.0", true), (evil, "1.0.0", true)],
                )],
                &["nodejs"],
            );
            let output = language_package_lines(&snap).join("\n");
            let expected = format!("RUN npm install -g -- 'pm2@5.3.0' '{evil}@1.0.0'");
            assert_eq!(
                active_lines(&output),
                vec![expected.as_str()],
                "{evil:?}: the quoted install must be the only active line: {output}"
            );
        }

        // The quote and the newline still break out of the literal, so a
        // token carrying either is refused and never reaches the line.
        for evil in ["evil'name", "evil\nname"] {
            let snap = test_snap(
                vec![npm_global_item(
                    "/usr/local/lib/node_modules",
                    HIGH_CONFIDENCE,
                    vec![("pm2", "5.3.0", false), (evil, "1.0.0", false)],
                )],
                &["nodejs"],
            );
            let output = language_package_lines(&snap).join("\n");
            assert!(
                output.contains("WARNING: 1 package(s) skipped due to unsafe characters"),
                "{evil:?}: must warn about the rejected package: {output}"
            );
            assert_eq!(
                active_lines(&output),
                vec!["RUN npm install -g -- 'pm2'"],
                "{evil:?}: {output}"
            );
            assert!(!output.contains("evil"), "{evil:?}: {output}");
        }

        // Ordinary package-name punctuation renders, quoted.
        for name in ["@angular/cli", "node-sass", "socket.io", "left_pad"] {
            let snap = test_snap(
                vec![npm_global_item(
                    "/usr/local/lib/node_modules",
                    HIGH_CONFIDENCE,
                    vec![(name, "1.0.0", false)],
                )],
                &["nodejs"],
            );
            let output = language_package_lines(&snap).join("\n");
            let expected = format!("RUN npm install -g -- '{name}'");
            assert_eq!(active_lines(&output), vec![expected.as_str()], "{output}");
        }
    }

    /// A manifest file name is a snapshot key joined onto the environment's
    /// directory: an absolute key replaces that directory and `..` leaves
    /// it. Only a single ordinary component is written; the legitimate
    /// manifest beside a refused one still is.
    #[test]
    fn manifest_names_cannot_write_outside_the_export() {
        use crate::render::victim_sandbox::VictimSandbox;
        use inspectah_core::util::METHOD_PYTHON_VENV;

        let s = VictimSandbox::new();
        let victim = s.victim().to_string_lossy().into_owned();
        let mut manifest_files = HashMap::new();
        for hostile in [
            victim.as_str(),
            "../../../../victim/authorized_keys",
            "../../../../victim/planted",
            ".",
            "..",
            "sub/requirements.txt",
        ] {
            manifest_files.insert(hostile.to_string(), "PWNED".to_string());
        }
        manifest_files.insert("requirements.txt".to_string(), "flask==3.0\n".to_string());
        let snap = InspectionSnapshot {
            non_rpm_software: Some(NonRpmSoftwareSection {
                items: vec![NonRpmItem {
                    path: "/opt/app/venv".into(),
                    name: "venv".into(),
                    method: METHOD_PYTHON_VENV.into(),
                    confidence: HIGH_CONFIDENCE.into(),
                    disposition: FindingKind::included(),
                    manifest_files,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let before = s.outside();
        let result = write_language_package_manifests(&snap, &s.out());
        s.assert_contained(&before, "manifest names");
        result.expect("a refused name is skipped, not an error");

        let dir = s
            .out()
            .join("language-packages/pip")
            .join(env_hash("/opt/app/venv"));
        assert_eq!(
            std::fs::read_to_string(dir.join("requirements.txt")).expect("kept manifest"),
            "flask==3.0\n"
        );
        let written: Vec<_> = std::fs::read_dir(&dir)
            .expect("read env dir")
            .map(|e| e.expect("entry").file_name())
            .collect();
        assert_eq!(
            written,
            vec!["requirements.txt"],
            "only the plain name is written"
        );
    }

    /// Quoting stops the shell, not npm: a token such as `--prefix=/x`
    /// still parses as an npm option and moves the install. Every token
    /// renders after `--`, which ends npm's option parsing, on the active
    /// line and on the commented one.
    #[test]
    fn npm_global_option_like_tokens_render_after_the_separator() {
        for (confidence, expected) in [
            (HIGH_CONFIDENCE, "RUN npm install -g -- 'pm2' '--prefix=/x'"),
            (
                MEDIUM_CONFIDENCE,
                "# RUN npm install -g -- 'pm2' '--prefix=/x'",
            ),
        ] {
            let mut item = npm_global_item(
                "/usr/local/lib/node_modules",
                confidence,
                vec![("pm2", "5.3.0", false), ("--prefix=/x", "1.0.0", false)],
            );
            item.disposition = FindingKind::included();
            let snap = test_snap(vec![item], &["nodejs"]);
            let output = language_package_lines(&snap).join("\n");
            assert!(
                output.lines().any(|l| l == expected),
                "{confidence}: expected {expected:?} in: {output}"
            );
        }
    }

    /// Paths render inside single quotes, so shell metacharacters in them
    /// are inert and must not cost the user the environment: real paths
    /// such as `/opt/app$canary/venv` and `/srv/a&b/webapp` rendered in
    /// beta.1 and were skipped once the predicate widened. The quote and
    /// the newline break the literal and stay rejected.
    #[test]
    fn quoted_paths_keep_shell_metacharacters_and_still_render() {
        let rendered = [
            ("semicolon", "/opt/ev;il/venv"),
            ("dollar", "/opt/app$canary/venv"),
            ("backtick", "/opt/ev`il`/venv"),
            ("pipe", "/opt/ev|il/venv"),
            ("ampersand", "/srv/a&b/venv"),
            ("redirect", "/opt/ev>il/venv"),
            ("space", "/opt/my app-1.0/venv"),
        ];
        for (label, path) in rendered {
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
                output.contains(&format!("RUN python3 -m venv '{path}' \\")),
                "{label}: metacharacters inside single quotes are inert: {output}"
            );
            assert!(
                !output.contains("path contains unsafe characters"),
                "{label}: path must not be rejected: {output}"
            );
        }

        let rejected = [
            ("single quote", "/opt/ev'il/venv"),
            ("newline", "/opt/ev\nil/venv"),
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
    /// `contains_unsafe_chars` refuses only `'` and line terminators, so
    /// names carrying `$` or `;` render too, and this proves `$(id)` cannot
    /// reach a `RUN` line unquoted. The quoting is what made that narrowing
    /// safe.
    #[test]
    fn venv_directory_name_is_quoted_at_its_own_interpolation_sites() {
        for name in [
            "my venv",
            "venv(x)",
            "venv>out",
            "venv*",
            "ve$(id)nv",
            "venv;id",
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

    /// A path or package name holding any line terminator renders no active
    /// line. Single quotes make every other character inert to the shell,
    /// but not to whatever reads the Containerfile's lines: a carriage
    /// return or U+2028 inside the quotes is still a line break to a
    /// terminal, and the build parser's handling of the non-newline ones
    /// is unverified.
    #[test]
    fn every_line_terminator_refuses_the_active_lines() {
        const INJECTED: &str = "RUN echo pwned";
        for terminator in LINE_TERMINATORS {
            let value = format!("x{terminator}{INJECTED}");
            let path = format!("/opt/app/{value}");
            for (label, item, runtime) in [
                (
                    "pip venv",
                    pip_venv_item(&path, HIGH_CONFIDENCE, true, vec![("flask", "2.3.3")]),
                    "python3",
                ),
                ("npm project", npm_item(&path, HIGH_CONFIDENCE), "nodejs"),
                ("gem project", gem_item(&path, HIGH_CONFIDENCE), "rubygems"),
                (
                    "npm global package",
                    npm_global_item(
                        "/usr/local/lib/node_modules",
                        HIGH_CONFIDENCE,
                        vec![(value.as_str(), "1.0.0", false)],
                    ),
                    "nodejs",
                ),
            ] {
                let output = language_package_lines(&test_snap(vec![item], &[runtime])).join("\n");
                for line in active_lines(&output) {
                    assert!(
                        !line.contains(INJECTED),
                        "{label} {terminator:?}: the value reached an active line {line:?}: \
                         {output}"
                    );
                }
                assert!(
                    output.contains("unsafe characters"),
                    "{label} {terminator:?}: the value must be refused: {output}"
                );
            }
        }
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

    // ----------------------------------------------------------------
    // The value-span rewrite: Findings 2, 3, 5, 6 and 10.
    //
    // Every one was the same defect — right line, wrong span, because
    // the separator was located with a bare `find()` from the start of
    // the line instead of from the end of the key already matched. Each
    // fixture below is the adversarial form measured in the audit, with
    // the well-formed form kept as a control, because the suite that
    // shipped all five asserted only on well-formed input.
    // ----------------------------------------------------------------

    const CRED: &str = "s3cret";

    /// Finding 2. The key-value colon belongs to `publishConfig`, so a
    /// search for the line's first colon read `registry` as the value,
    /// found no `://` in it and returned the line untouched.
    #[test]
    fn json_member_value_is_found_after_its_own_key() {
        let cases = [
            // (label, input, must_still_contain)
            (
                "compact publishConfig",
                "{\"name\":\"app\",\"publishConfig\": { \"registry\": \"https://deploy:s3cret@npm.corp.example.com/\" }}",
                "npm.corp.example.com",
            ),
            (
                "compact resolved",
                "    \"node_modules/x\": { \"resolved\": \"https://deploy:s3cret@npm.corp.example.com/x.tgz\" },",
                "x.tgz",
            ),
            (
                "own-line registry (control)",
                "  \"registry\": \"https://deploy:s3cret@npm.corp.example.com/\"",
                "npm.corp.example.com",
            ),
        ];
        for (label, input, keep) in cases {
            let name = if input.contains("resolved") {
                "package-lock.json"
            } else {
                "package.json"
            };
            let out = scrub_manifest_auth(name, input);
            assert!(
                !out.content.contains(CRED),
                "{label}: credential survived: {}",
                out.content
            );
            assert!(
                out.content.contains("://REDACTED@"),
                "{label}: no redaction marker: {}",
                out.content
            );
            assert!(
                out.content.contains(keep),
                "{label}: host or path was destroyed: {}",
                out.content
            );
            assert!(
                out.unscrubbed.is_empty(),
                "{label}: located fine, must not report a miss"
            );
        }
    }

    /// Finding 3. `--index-url=URL` has no whitespace for `splitn` to
    /// find, and `-i URL` never matched the prefix test at all. Both are
    /// valid requirements-file syntax, confirmed against pip.
    #[test]
    fn pip_index_directives_scrub_in_every_valid_form() {
        let cases = [
            (
                "equals form",
                "--index-url=https://deploy:s3cret@pypi.corp.example.com/simple",
            ),
            (
                "short form",
                "-i https://deploy:s3cret@pypi.corp.example.com/simple",
            ),
            (
                "extra equals form",
                "--extra-index-url=https://deploy:s3cret@pypi.corp.example.com/simple",
            ),
            (
                "space form (control)",
                "--index-url https://deploy:s3cret@pypi.corp.example.com/simple",
            ),
        ];
        for (label, input) in cases {
            let out = scrub_manifest_auth("requirements.txt", input);
            assert!(
                !out.content.contains(CRED),
                "{label}: credential survived: {}",
                out.content
            );
            assert!(
                out.content.contains("pypi.corp.example.com"),
                "{label}: host was destroyed: {}",
                out.content
            );
            assert!(out.unscrubbed.is_empty(), "{label}: must not report a miss");
        }
    }

    /// Finding 5. The old loop tried `"` before `'` and broke on the
    /// first quote character anywhere on the line, so a double-quoted
    /// word in a trailing comment won over the single-quoted source URL.
    #[test]
    fn gemfile_source_takes_its_own_quoted_run() {
        let cases = [
            (
                "single-quoted source, double-quoted comment",
                "source 'https://deploy:s3cret@gems.corp.example.com' # see \"runbook\"",
            ),
            (
                "double-quoted source, single-quoted comment",
                "source \"https://deploy:s3cret@gems.corp.example.com\" # it's here",
            ),
            (
                "single-quoted source (control)",
                "source 'https://deploy:s3cret@gems.corp.example.com'",
            ),
        ];
        for (label, input) in cases {
            let out = scrub_manifest_auth("Gemfile", input);
            assert!(
                !out.content.contains(CRED),
                "{label}: credential survived: {}",
                out.content
            );
            assert!(
                out.content.contains("gems.corp.example.com"),
                "{label}: host was destroyed: {}",
                out.content
            );
        }
    }

    /// Finding 5, second half: the rebuild used `replace`, which
    /// rewrote every occurrence of the located text rather than the one
    /// occurrence it had located.
    #[test]
    fn gemfile_rewrites_only_the_span_it_located() {
        let input = "source 'https://deploy:s3cret@gems.example.com' # mirror of https://deploy:s3cret@gems.example.com";
        let out = scrub_manifest_auth("Gemfile", input);
        assert_eq!(
            out.content.matches("://REDACTED@").count(),
            1,
            "only the source argument is this scrubber's span: {}",
            out.content
        );
    }

    /// Finding 6. A single `find("://")` scrubbed the first URL on the
    /// line and left the second, and once the marker detector landed it
    /// did so while reporting success.
    #[test]
    fn gemfile_lock_scrubs_every_url_on_a_line() {
        let input =
            "  remote: https://deploy:s3cret@a.example.com/ https://deploy:s3cret@b.example.com/";
        let out = scrub_manifest_auth("Gemfile.lock", input);
        assert!(
            !out.content.contains(CRED),
            "both URLs must be scrubbed: {}",
            out.content
        );
        assert_eq!(
            out.content.matches("://REDACTED@").count(),
            2,
            "one marker per credentialed URL: {}",
            out.content
        );
        assert!(out.content.contains("a.example.com"));
        assert!(out.content.contains("b.example.com"));
    }

    /// Finding 10. RFC 3986 ends userinfo at the last `@` before the
    /// authority terminator, and pip reads it that way. Taking the first
    /// left the tail of a password containing an `@` in the file.
    #[test]
    fn url_userinfo_ends_at_the_last_at_in_the_authority() {
        let out = scrub_manifest_auth(
            "requirements.txt",
            "--index-url https://deploy:p@s3cret@pypi.corp.example.com/simple",
        );
        assert!(
            !out.content.contains(CRED),
            "the password tail after the first @ survived: {}",
            out.content
        );
        assert_eq!(
            out.content,
            "--index-url https://REDACTED@pypi.corp.example.com/simple"
        );
    }

    /// An `@` in a query string is not the end of userinfo, which is
    /// what makes searching backwards safe rather than differently
    /// wrong.
    #[test]
    fn an_at_in_the_query_string_is_not_userinfo() {
        let input = "--index-url https://pypi.corp.example.com/simple?contact=ops@example.com";
        let out = scrub_manifest_auth("requirements.txt", input);
        assert_eq!(out.content, input, "nothing here is a credential");
    }

    /// The scrubber must say so when it cannot find the value it was
    /// routed to rewrite. Silence is what let four separate misses ship
    /// under a `FullyRedacted` label with `Secrets redacted | 0`.
    #[test]
    fn a_scrubber_that_cannot_locate_its_value_reports_it() {
        let cases = [
            // (label, filename, line, expected key)
            (
                "registry key with no colon after it",
                "package.json",
                "  \"registry\" \"https://deploy:s3cret@npm.corp.example.com/\"",
                "\"registry\"",
            ),
            (
                "resolved value is not a quoted run",
                "package-lock.json",
                "  \"resolved\": https://deploy:s3cret@npm.corp.example.com/x.tgz,",
                "\"resolved\"",
            ),
            (
                "index directive with no argument",
                "requirements.txt",
                "--index-url",
                "--index-url",
            ),
            (
                "gemfile source with an unquoted argument",
                "Gemfile",
                "source https://deploy:s3cret@gems.corp.example.com",
                "source",
            ),
        ];
        for (label, filename, line, key) in cases {
            let out = scrub_manifest_auth(filename, line);
            assert_eq!(
                out.unscrubbed.len(),
                1,
                "{label}: expected exactly one reported miss, got {:?}",
                out.unscrubbed
            );
            assert_eq!(out.unscrubbed[0].key, key, "{label}: wrong key reported");
            assert_eq!(out.unscrubbed[0].line, 1, "{label}: wrong line reported");
            assert!(
                !out.unscrubbed[0].key.contains(CRED),
                "{label}: the report must not carry the credential it failed to strip"
            );
        }
    }

    /// A line nothing routed to a scrubber is not a miss, and neither is
    /// a line whose value was located and simply had no credential in
    /// it. Without this the signal would fire on every ordinary
    /// manifest and mean nothing.
    #[test]
    fn located_or_unrouted_lines_are_never_reported_as_misses() {
        let cases = [
            (
                "requirements.txt",
                "--index-url https://pypi.org/simple\nflask==3.0.0\n",
            ),
            ("requirements.txt", "flask==3.0.0\n"),
            (
                "package.json",
                "{\n  \"registry\": \"https://registry.npmjs.org/\"\n}\n",
            ),
            ("package.json", "{\n  \"name\": \"app\"\n}\n"),
            ("Gemfile", "source 'https://rubygems.org'\ngem 'rails'\n"),
            ("Gemfile", "sources = ['https://rubygems.org']\n"),
            (
                "Gemfile.lock",
                "GEM\n  remote: https://rubygems.org/\n  specs:\n",
            ),
        ];
        for (filename, content) in cases {
            let out = scrub_manifest_auth(filename, content);
            assert!(
                out.unscrubbed.is_empty(),
                "{filename}: false miss on {content:?}: {:?}",
                out.unscrubbed
            );
            assert_eq!(
                out.content, content,
                "{filename}: credential-free content must come back byte-identical"
            );
        }
    }
}
