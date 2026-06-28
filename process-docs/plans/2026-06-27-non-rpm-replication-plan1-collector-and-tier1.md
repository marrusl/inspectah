# Non-RPM Replication Plan 1: Collector Hardening & Tier 1 Rendering

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Harden the non-RPM collector to produce trustworthy, project-level language environment data, then render executable pip/npm/gem sections in the Containerfile backed by collected manifest artifacts.

**Architecture:** Three layers change in sequence: (1) core types gain new fields and ItemId variants, (2) the collector emits project-level entries with RPM ownership filtering and manifest capture, (3) the pipeline renderer emits real COPY/RUN instructions instead of advisory stubs, backed by a new `language-packages/` export root.

**Tech Stack:** Rust (2024 edition), serde, insta (snapshot testing), inspectah-core types, inspectah-refine, inspectah-pipeline, inspectah-collect.

**Spec:** `process-docs/specs/proposed/2026-06-27-non-rpm-replication.md` — read fresh before implementation. This plan covers Tier 1 and the shared contracts Plans 2-4 depend on.

**Thorn Checkpoints:** After Tasks 3, 6, 9.

## Global Constraints

- Clippy clean: `cargo clippy -- -W clippy::all` with zero warnings.
- Format: `cargo fmt --check` must pass.
- No team member names in code or commits.
- Commit format: `type(scope): description`. Attribution: `Assisted-by: Claude Code (Opus 4.6)`.
- All new `#[serde]` fields use `#[serde(default)]` for backward-compatible deserialization.
- Schema version bumps from 19 to 20 at the end (Task 9), not incrementally.
- Existing tests must keep passing throughout. Run `cargo test` after each task.

## File Map

### Modified files

| File | Change |
|------|--------|
| `crates/core/src/types/nonrpm.rs` | Add `manifest_files`, `rpm_filtered` to `NonRpmItem`; add `NpmPackage`, `GemPackage` structs |
| `crates/core/src/snapshot.rs` | Bump `SCHEMA_VERSION` from 19 to 20 (Task 9 only) |
| `crates/refine/src/types.rs` | Add `ItemId::LanguageEnv` variant |
| `crates/collect/src/inspectors/nonrpm.rs` | RPM ownership filtering in `scan_pip_packages()`, project-level restructuring in `scan_npm_packages()` and `scan_gem_packages()`, requirements.txt collection in `scan_python_venvs()` |
| `crates/pipeline/src/render/containerfile.rs` | Replace advisory stubs in `non_rpm_section_lines()` with executable COPY/RUN |
| `crates/refine/src/session.rs` | Add `language-packages` to export allowlist, add materialization logic |
| `crates/refine/tests/export_contract_test.rs` | Add contract test for `language-packages/` root |

### New files

| File | Responsibility |
|------|---------------|
| `crates/pipeline/src/render/language_packages.rs` | Containerfile rendering for pip/npm/gem sections |
| `crates/collect/tests/nonrpm_rpm_filter_test.rs` | Integration tests for RPM ownership filtering |

---

### Task 1: Data Model Extensions

**Files:**
- Modify: `crates/core/src/types/nonrpm.rs`
- Modify: `crates/refine/src/types.rs`
- Test: existing roundtrip test in `nonrpm.rs`, new test in `types.rs`

**Interfaces:**
- Produces: `NonRpmItem.manifest_files`, `NonRpmItem.rpm_filtered`, `NpmPackage`, `GemPackage`, `ItemId::LanguageEnv`
- Consumed by: Tasks 2-9

- [ ] **Step 1: Add NpmPackage and GemPackage structs**

In `crates/core/src/types/nonrpm.rs`, add after `PipPackage`:

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NpmPackage {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GemPackage {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
}
```

- [ ] **Step 2: Extend NonRpmItem with new fields**

Add these fields to `NonRpmItem` (after existing `git_remote` field):

```rust
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub manifest_files: std::collections::HashMap<String, String>,

    #[serde(default)]
    pub rpm_filtered: bool,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub npm_packages: Vec<NpmPackage>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gem_packages: Vec<GemPackage>,
```

Add `use std::collections::HashMap;` to the file imports.

- [ ] **Step 3: Add ItemId::LanguageEnv variant**

In `crates/refine/src/types.rs`, add to the `ItemId` enum:

```rust
    // Language packages section
    LanguageEnv {
        ecosystem: String,
        path: String,
    },
```

- [ ] **Step 4: Update roundtrip test**

In the existing `test_nonrpm_section_roundtrip` test in `nonrpm.rs`,
add `manifest_files`, `rpm_filtered`, `npm_packages`, and `gem_packages`
to the test fixture. Verify serde roundtrip preserves all fields.

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-core -p inspectah-refine`
Expected: all pass, zero clippy warnings.

- [ ] **Step 6: Commit**

```
feat(core): add language package data model extensions

Add NpmPackage, GemPackage structs and manifest_files, rpm_filtered,
npm_packages, gem_packages fields to NonRpmItem. Add
ItemId::LanguageEnv variant. All new fields use serde(default)
for backward compatibility.
```

---

### Task 2: RPM Ownership Filtering for pip

**Files:**
- Modify: `crates/collect/src/inspectors/nonrpm.rs` (functions: `scan_pip_packages`, `scan_dist_info`)
- Create: `crates/collect/tests/nonrpm_rpm_filter_test.rs`

**Interfaces:**
- Consumes: `RpmState.owned_paths: HashSet<PathBuf>` from `crates/core/src/traits/inspector.rs`
- Produces: pip `NonRpmItem` entries with `rpm_filtered: true` and RPM-owned packages excluded

- [ ] **Step 1: Write failing test for RPM filtering**

Create `crates/collect/tests/nonrpm_rpm_filter_test.rs`:

```rust
use inspectah_collect::inspectors::nonrpm::NonRpmInspector;
use inspectah_core::traits::inspector::*;
use inspectah_core::traits::progress::NullProgress;
// ... test setup with MockExecutor

#[test]
fn pip_rpm_owned_packages_excluded() {
    // Set up a MockExecutor with dist-info for both "requests" (RPM-owned)
    // and "flask" (user-installed pip).
    // RpmState.owned_paths includes the requests dist-info path.
    // Assert: only flask appears in the output, with rpm_filtered: true.
}

#[test]
fn pip_all_rpm_owned_produces_empty() {
    // All detected pip packages are RPM-owned.
    // Assert: no pip NonRpmItem entries emitted.
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-collect --test nonrpm_rpm_filter_test`
Expected: FAIL — filtering not implemented.

- [ ] **Step 3: Implement RPM ownership filtering**

In `crates/collect/src/inspectors/nonrpm.rs`:

1. Change `scan_pip_packages` signature to accept `rpm_state: Option<&RpmState>`:
   ```rust
   fn scan_pip_packages(
       exec: &dyn Executor,
       section: &mut NonRpmSoftwareSection,
       is_ostree: bool,
       rpm_state: Option<&RpmState>,
   )
   ```

2. Pass `ctx.rpm_state` from the `inspect()` method call site.

3. After detecting a pip package, check ownership:
   ```rust
   let dist_info_path = PathBuf::from(&rel_path);
   let is_rpm_owned = rpm_state
       .map(|rs| rs.owned_paths.contains(&dist_info_path))
       .unwrap_or(false);
   if is_rpm_owned {
       continue;
   }
   ```

4. Set `rpm_filtered: true` on all emitted pip items when `rpm_state.is_some()`.

5. Apply the same filtering in `scan_dist_info` (the fallback path).

- [ ] **Step 4: Update confidence labeling**

Set `confidence` on pip items based on detection quality:
- `"high"` when requirements.txt was collected AND rpm_filtered
- `"medium"` when dist-info/pip-list detection AND rpm_filtered
- `"low"` when no RPM filtering available (defensive)

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-collect`
Expected: all pass including new filter tests. Existing tests may need
`rpm_state` parameter added to `scan_pip_packages` calls.

- [ ] **Step 6: Commit**

```
feat(collect): filter RPM-owned packages from pip inventory

Cross-reference pip dist-info paths against RpmState.owned_paths.
RPM-managed Python packages (e.g., python3-requests via dnf) are
excluded from the pip inventory. Sets rpm_filtered: true and
confidence levels on all pip items.
```

---

### Task 3: Venv Requirements.txt Collection

**Files:**
- Modify: `crates/collect/src/inspectors/nonrpm.rs` (function: `scan_python_venvs`)

**Interfaces:**
- Consumes: `Executor.read_file()` for requirements.txt content
- Produces: `NonRpmItem.manifest_files["requirements.txt"]` populated when file exists

- [ ] **Step 1: Write failing test**

Add to `nonrpm_rpm_filter_test.rs` (or existing nonrpm test module):

```rust
#[test]
fn venv_with_requirements_txt_captures_manifest() {
    // MockExecutor with /opt/myapp/venv/pyvenv.cfg and
    // /opt/myapp/requirements.txt containing "flask==2.3.3\nrequests==2.31.0\n"
    // Assert: the venv NonRpmItem has manifest_files["requirements.txt"]
    // containing the file content.
}

#[test]
fn venv_without_requirements_txt_has_empty_manifests() {
    // MockExecutor with /opt/myapp/venv/ but no requirements.txt.
    // Assert: manifest_files is empty, confidence is "medium".
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-collect --test nonrpm_rpm_filter_test`
Expected: FAIL

- [ ] **Step 3: Implement requirements.txt collection**

In `scan_python_venvs`, after detecting a venv at a path like `/opt/myapp/venv`:

```rust
let venv_parent = Path::new(venv_path).parent().unwrap_or(Path::new("/"));
let candidates = [
    venv_parent.join("requirements.txt"),
    Path::new(venv_path).join("requirements.txt"),
];
let mut manifest_files = HashMap::new();
for candidate in &candidates {
    if let Ok(content) = exec.read_file(candidate) {
        manifest_files.insert("requirements.txt".to_string(), content);
        break;
    }
}
```

Set on the emitted `NonRpmItem`:
- `manifest_files` from above
- `confidence: "high"` if requirements.txt found, `"medium"` otherwise

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-collect`
Expected: all pass.

- [ ] **Step 5: Commit**

```
feat(collect): capture requirements.txt for venv environments

When a Python venv is detected, look for requirements.txt in the
venv parent directory or venv root. Captured content stored in
manifest_files for Containerfile rendering. Sets confidence to
"high" when found, "medium" otherwise.
```

**Thorn checkpoint: review Tasks 1-3 before proceeding.**

---

### Task 4: npm Project-Level Restructuring

**Files:**
- Modify: `crates/collect/src/inspectors/nonrpm.rs` (function: `scan_npm_packages`)

**Interfaces:**
- Consumes: `Executor.read_file()` for package.json and package-lock.json
- Produces: One `NonRpmItem` per project directory (not per package) with
  `npm_packages: Vec<NpmPackage>`, `manifest_files` containing raw lockfile/manifest content,
  `method: "npm lockfile"`, `confidence: "high"`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn npm_emits_one_item_per_project() {
    // MockExecutor with /opt/myapp/package-lock.json containing 3 packages.
    // Assert: one NonRpmItem emitted (not 3).
    // Assert: npm_packages has 3 entries.
    // Assert: manifest_files contains "package.json" and "package-lock.json".
    // Assert: method == "npm lockfile", confidence == "high".
}
```

- [ ] **Step 2: Run test to verify it fails**

Expected: FAIL — current code emits one item per package.

- [ ] **Step 3: Restructure scan_npm_packages**

Replace the per-package emission loop with project-level emission:

```rust
fn scan_npm_packages(exec: &dyn Executor, section: &mut NonRpmSoftwareSection, is_ostree: bool) {
    for root in SCAN_ROOTS {
        find_files_matching(exec, root, "package-lock.json", &mut |lockfile_path| {
            let project_dir = Path::new(lockfile_path).parent().unwrap_or(Path::new("/"));
            let rel_path = project_dir.to_string_lossy().trim_start_matches('/').to_string();
            if is_ostree && rel_path.starts_with("var/") {
                return;
            }

            let mut manifest_files = HashMap::new();
            let mut npm_packages = Vec::new();

            // Collect lockfile content and parse packages
            if let Ok(content) = exec.read_file(Path::new(lockfile_path)) {
                manifest_files.insert("package-lock.json".to_string(), content.clone());
                npm_packages = parse_package_lock(&content)
                    .into_iter()
                    .map(|p| NpmPackage { name: p.name, version: p.version })
                    .collect();
            }

            // Collect package.json
            let pkg_json_path = project_dir.join("package.json");
            if let Ok(content) = exec.read_file(&pkg_json_path) {
                manifest_files.insert("package.json".to_string(), content);
            }

            section.items.push(NonRpmItem {
                path: rel_path,
                name: project_dir.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                method: "npm lockfile".to_string(),
                confidence: "high".to_string(),
                include: true,
                npm_packages,
                manifest_files,
                ..Default::default()
            });
        });
    }
}
```

- [ ] **Step 4: Update existing npm tests**

Existing `test_scan_npm_packages` asserts per-package items. Update to
assert one project item with `npm_packages` vec.

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-collect`
Expected: all pass.

- [ ] **Step 6: Commit**

```
feat(collect): restructure npm to project-level entries

Emit one NonRpmItem per project directory instead of one per package.
Package details stored in npm_packages vec. Lockfile and package.json
captured in manifest_files for Containerfile rendering.
```

---

### Task 5: gem Project-Level Restructuring

**Files:**
- Modify: `crates/collect/src/inspectors/nonrpm.rs` (function: `scan_gem_packages`)

**Interfaces:**
- Same pattern as Task 4 but for gem: one item per project with
  `gem_packages: Vec<GemPackage>`, `manifest_files` with Gemfile/Gemfile.lock

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn gem_emits_one_item_per_project() {
    // MockExecutor with /opt/myapp/Gemfile.lock containing 2 gems.
    // Assert: one NonRpmItem emitted.
    // Assert: gem_packages has 2 entries.
    // Assert: manifest_files contains "Gemfile" and "Gemfile.lock".
}
```

- [ ] **Step 2: Run test to verify it fails**

- [ ] **Step 3: Restructure scan_gem_packages**

Same pattern as Task 4: project-level emission, collect Gemfile and
Gemfile.lock content, parse gems into `gem_packages` vec.

```rust
fn scan_gem_packages(exec: &dyn Executor, section: &mut NonRpmSoftwareSection, is_ostree: bool) {
    for root in SCAN_ROOTS {
        find_files_matching(exec, root, "Gemfile.lock", &mut |lockfile_path| {
            let project_dir = Path::new(lockfile_path).parent().unwrap_or(Path::new("/"));
            let rel_path = project_dir.to_string_lossy().trim_start_matches('/').to_string();
            if is_ostree && rel_path.starts_with("var/") {
                return;
            }

            let mut manifest_files = HashMap::new();
            let mut gem_packages = Vec::new();

            if let Ok(content) = exec.read_file(Path::new(lockfile_path)) {
                manifest_files.insert("Gemfile.lock".to_string(), content.clone());
                gem_packages = parse_gemfile_lock(&content)
                    .into_iter()
                    .map(|g| GemPackage { name: g.name, version: g.version })
                    .collect();
            }

            let gemfile_path = project_dir.join("Gemfile");
            if let Ok(content) = exec.read_file(&gemfile_path) {
                manifest_files.insert("Gemfile".to_string(), content);
            }

            section.items.push(NonRpmItem {
                path: rel_path,
                name: project_dir.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                method: "gem lockfile".to_string(),
                confidence: "high".to_string(),
                include: true,
                gem_packages,
                manifest_files,
                ..Default::default()
            });
        });
    }
}
```

- [ ] **Step 4: Update existing gem tests**

Update `test_scan_gem_packages` to assert one project item.

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-collect`

- [ ] **Step 6: Commit**

```
feat(collect): restructure gem to project-level entries

Same pattern as npm: one NonRpmItem per project, gem details in
gem_packages vec, Gemfile and Gemfile.lock in manifest_files.
```

---

### Task 6: Containerfile Renderer — Language Packages

**Files:**
- Create: `crates/pipeline/src/render/language_packages.rs`
- Modify: `crates/pipeline/src/render/mod.rs` (add module)
- Modify: `crates/pipeline/src/render/containerfile.rs` (replace advisory stubs)

**Interfaces:**
- Consumes: `InspectionSnapshot.non_rpm_software` with hardened NonRpmItem entries
- Produces: Vec<String> of Containerfile lines for pip/npm/gem sections

- [ ] **Step 1: Create the renderer module**

Create `crates/pipeline/src/render/language_packages.rs`:

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::nonrpm::NonRpmItem;
use sha2::{Digest, Sha256};

const HIGH_CONFIDENCE: &str = "high";
const MEDIUM_CONFIDENCE: &str = "medium";

fn env_hash(path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..6])
}

pub fn language_package_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let nrs = match &snap.non_rpm_software {
        Some(n) if !n.items.is_empty() => n,
        _ => return Vec::new(),
    };

    let mut lines = Vec::new();

    // Partition items by ecosystem
    let pip_items: Vec<&NonRpmItem> = nrs.items.iter()
        .filter(|i| i.include && is_pip_env(i))
        .collect();
    let npm_items: Vec<&NonRpmItem> = nrs.items.iter()
        .filter(|i| i.include && i.method == "npm lockfile")
        .collect();
    let gem_items: Vec<&NonRpmItem> = nrs.items.iter()
        .filter(|i| i.include && i.method == "gem lockfile")
        .collect();

    if !pip_items.is_empty() {
        lines.extend(render_pip_section(&pip_items));
    }
    if !npm_items.is_empty() {
        lines.extend(render_npm_section(&npm_items));
    }
    if !gem_items.is_empty() {
        lines.extend(render_gem_section(&gem_items));
    }

    lines
}
```

Then implement `render_pip_section`, `render_npm_section`, `render_gem_section`
per the spec's Containerfile Rendering section. Key rules:
- pip venv: `RUN python3 -m venv <path> && <path>/bin/pip install ...`
- pip system: `RUN pip install ...`
- pip with requirements.txt (high confidence): `COPY language-packages/pip/<hash>/requirements.txt ...`
- pip without (medium confidence): commented-out inline install
- npm: `COPY language-packages/npm/<hash>/package.json + package-lock.json ... && npm ci`
- gem: `COPY language-packages/gem/<hash>/Gemfile + Gemfile.lock ... && bundle install`
- Runtime prerequisite check: warn if python3/nodejs/rubygems not in RPM list

- [ ] **Step 2: Add module to mod.rs**

In `crates/pipeline/src/render/mod.rs`, add:
```rust
pub mod language_packages;
```

- [ ] **Step 3: Replace advisory stubs in containerfile.rs**

In `crates/pipeline/src/render/containerfile.rs`, find `non_rpm_section_lines()`
(line ~1105). Replace the advisory stub logic with a call to the new renderer:

```rust
fn non_rpm_section_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let mut lines = Vec::new();

    // Language package sections (executable, not advisory)
    lines.extend(language_packages::language_package_lines(snap));

    // Remaining non-RPM items that aren't language packages
    // (ELF binaries, .env files, git repos — still advisory)
    let nrs = match &snap.non_rpm_software {
        Some(n) => n,
        None => return lines,
    };
    let remaining: Vec<&NonRpmItem> = nrs.items.iter()
        .filter(|i| i.include && !is_language_env(i))
        .collect();
    if !remaining.is_empty() {
        lines.push(String::new());
        lines.push("# WARNING: These stubs are advisory — source files are NOT in the build context.".into());
        // ... existing advisory rendering for non-language items
    }

    lines
}
```

- [ ] **Step 4: Write renderer tests**

Add tests in `language_packages.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pip_venv_with_requirements_renders_copy_and_run() { ... }

    #[test]
    fn pip_venv_medium_confidence_renders_commented_out() { ... }

    #[test]
    fn npm_lockfile_renders_copy_and_npm_ci() { ... }

    #[test]
    fn gem_lockfile_renders_copy_and_bundle_install() { ... }

    #[test]
    fn missing_runtime_emits_warning_comment() { ... }

    #[test]
    fn excluded_items_not_rendered() { ... }
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-pipeline`
Expected: all pass, including existing containerfile tests.

- [ ] **Step 6: Commit**

```
feat(pipeline): add language package Containerfile rendering

Replace advisory non-RPM stubs with executable COPY/RUN for
pip/npm/gem. High-confidence items render as active instructions;
medium-confidence renders commented-out. Runtime prerequisite
warnings emitted when python3/nodejs/rubygems missing from RPM list.
```

**Thorn checkpoint: review Tasks 4-6 before proceeding.**

---

### Task 7: Export Contract — language-packages/ Root

**Files:**
- Modify: `crates/refine/src/session.rs` (export allowlist + materialization)
- Modify: `crates/refine/tests/export_contract_test.rs`

**Interfaces:**
- Consumes: `NonRpmItem.manifest_files` from projected snapshot
- Produces: `language-packages/{pip,npm,gem}/<hash>/` directories in export tarball

- [ ] **Step 1: Add language-packages to export allowlist**

In `crates/refine/src/session.rs`, find the `allowed_top_level` HashSet
(currently contains "config", "drop-ins", "flatpak", etc.). Add:

```rust
"language-packages",
```

- [ ] **Step 2: Add manifest materialization to export**

In the export function (near `write_config_tree` and similar calls),
add a new function call:

```rust
write_language_package_manifests(snap, out)?;
```

Implement `write_language_package_manifests`:

```rust
fn write_language_package_manifests(
    snap: &InspectionSnapshot,
    out: &Path,
) -> Result<(), RefineError> {
    let nrs = match &snap.non_rpm_software {
        Some(n) => n,
        None => return Ok(()),
    };

    for item in &nrs.items {
        if !item.include || item.manifest_files.is_empty() {
            continue;
        }

        let ecosystem = if !item.packages.is_empty() || item.method.contains("pip") {
            "pip"
        } else if item.method == "npm lockfile" {
            "npm"
        } else if item.method == "gem lockfile" {
            "gem"
        } else {
            continue;
        };

        let hash = env_hash(&item.path);
        let dir = out.join("language-packages").join(ecosystem).join(&hash);
        std::fs::create_dir_all(&dir)
            .map_err(|e| RefineError::ExportFailed(format!("mkdir {}: {e}", dir.display())))?;

        for (filename, content) in &item.manifest_files {
            let file_path = dir.join(filename);
            std::fs::write(&file_path, content)
                .map_err(|e| RefineError::ExportFailed(format!("write {}: {e}", file_path.display())))?;
        }
    }
    Ok(())
}
```

- [ ] **Step 3: Write export contract test**

In `crates/refine/tests/export_contract_test.rs`, add:

```rust
#[test]
fn export_includes_language_packages_root() {
    // Build a snapshot with an npm NonRpmItem that has manifest_files.
    // Run export.
    // Assert: tarball contains language-packages/npm/<hash>/package.json
    // Assert: tarball contains language-packages/npm/<hash>/package-lock.json
}

#[test]
fn export_excludes_language_packages_when_none_included() {
    // Snapshot with language packages but all include: false.
    // Assert: no language-packages/ root in tarball.
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-refine`
Expected: all pass.

- [ ] **Step 5: Commit**

```
feat(refine): add language-packages/ export root

Materialize collected manifest files (requirements.txt,
package.json, package-lock.json, Gemfile, Gemfile.lock) into
the export tarball under language-packages/<ecosystem>/<hash>/.
Export contract test verifies presence and exclusion rules.
```

---

### Task 8: Preview/Export Parity

**Files:**
- Modify: `crates/pipeline/src/render/containerfile.rs`
- Modify: `crates/pipeline/src/render/language_packages.rs`

**Interfaces:**
- Consumes: same manifest data as Tasks 6-7
- Produces: Containerfile preview that references the same paths the export materializes

- [ ] **Step 1: Verify parity**

The Containerfile preview (shown in the refine UI) and the exported
Containerfile must reference identical `COPY` source paths. Verify
that `language_package_lines()` uses the same `env_hash()` function
and path format as `write_language_package_manifests()`.

If they're in different modules, extract `env_hash()` into a shared
location (e.g., `crates/refine/src/types.rs` or a new small utility).

- [ ] **Step 2: Write parity test**

```rust
#[test]
fn containerfile_copy_paths_match_export_layout() {
    // Build a snapshot with pip+npm items.
    // Render Containerfile lines.
    // Run export.
    // Extract all COPY source paths from the Containerfile.
    // Assert: every COPY source path exists in the export tarball.
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-pipeline -p inspectah-refine`

- [ ] **Step 4: Commit**

```
test(pipeline): add preview/export parity test for language packages

Verifies that Containerfile COPY source paths match the export
tarball layout exactly.
```

---

### Task 9: Schema Version Bump + Docs

**Files:**
- Modify: `crates/core/src/snapshot.rs` (SCHEMA_VERSION 19 → 20)
- Modify: `docs/reference/output-artifacts.md` (document new root)

**Interfaces:**
- This is the final task — no downstream dependencies within this plan.

- [ ] **Step 1: Bump schema version**

In `crates/core/src/snapshot.rs`, change:
```rust
pub const SCHEMA_VERSION: u32 = 20;
```

- [ ] **Step 2: Update output artifacts docs**

In `docs/reference/output-artifacts.md`, add `language-packages/` to
the artifact root table with description: "Manifest files for
pip/npm/gem language package environments."

- [ ] **Step 3: Run full test suite**

Run: `cargo test`
Expected: all tests pass. Some snapshot tests may need updating due to
schema version change — update insta snapshots with `cargo insta review`.

- [ ] **Step 4: Commit**

```
chore(core): bump schema version to 20 for language package support

New NonRpmItem fields (manifest_files, rpm_filtered, npm_packages,
gem_packages) and new export root (language-packages/) constitute
a schema change. Older tarballs remain loadable via serde(default).
```

**Thorn checkpoint: review Tasks 7-9 before proceeding to Plan 2.**

---

## Shared Contracts for Plans 2-4

Plans 2, 3, and 4 depend on the following contracts established by this plan:

### ItemId Contracts

| Plan | ItemId Variant | Identity Key |
|------|---------------|--------------|
| Plan 2 | `ItemId::UnmanagedFile { path }` | Absolute file path |
| Plan 2 | (uses existing `ItemId::Package`) | For repo-less RPMs |
| Plan 3 | `ItemId::LanguageEnv` (from Task 1) | ecosystem + env path |
| Plan 4 | Same as Plans 2-3 | Aggregate wraps same IDs |

### Export Allowlist

Plans 2 and 4 must add their roots to the same `allowed_top_level`
HashSet in `crates/refine/src/session.rs`:

| Plan | Root | Gate |
|------|------|------|
| Plan 2 | `unmanaged` | `--include-unmanaged` |
| Plan 2 | `repoless-packages` | Automatic |
| Plan 4 | `compose` | When compose detected |

### NonRpmItem Method Strings

The `method` field is the branch key for rendering and UI routing:

| Method | Ecosystem | Source |
|--------|-----------|--------|
| `"pip list"` | pip | System-level pip |
| `"pip dist-info"` | pip | dist-info directory scan |
| `"venv"` | pip | Python venv |
| `"npm lockfile"` | npm | package-lock.json parse |
| `"gem lockfile"` | gem | Gemfile.lock parse |
| `"binary"` | — | ELF binary (Plan 2) |

### Confidence Rendering Gate

| Confidence | Containerfile | Refine Default |
|------------|--------------|----------------|
| `"high"` | Active COPY/RUN | `include: true` |
| `"medium"` | Commented-out | `include: false` |
| `"low"` | Advisory only | `include: false` |
