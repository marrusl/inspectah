# Language Package Detection v2 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Improve language package detection fidelity: add npm global package detection with per-package version pinning, C-extension detection, expanded scan roots (`--scan-home`, `--scan-path`, `/var/www`), and fix `system_site_packages` rendering and bundler deprecation.

**Architecture:** Inspector additions (npm globals, C-extension, scan expansion) feed through the existing NonRpmItem pipeline into the language package renderer, refine session, and web UI. Per-package pin state adds a new refine op type (`SetPackagePin`) and structured DTO (`LanguagePackageDto`) for the frontend.

**Tech Stack:** Rust (core, collect, pipeline, refine, web, cli), React/TypeScript/PatternFly (web UI)

**Spec:** `process-docs/specs/implemented/2026-07-08-language-package-detection-v2.md`

## Global Constraints

- Schema version bumps from 21 → 22 (new `LanguagePackage.pinned` field and `scan_roots` meta)
- `cargo clippy -- -W clippy::all` must pass with zero warnings on every commit
- `cargo fmt --check` must pass on every commit
- `METHOD_NPM_GLOBAL` constant value is `"npm global"`
- `LanguagePackage.pinned` is consulted ONLY for `METHOD_NPM_GLOBAL` — existing pip/gem rendering unchanged
- Scan-root messages go to stderr/progress, never stdout (preserves `--inspect-only` contract)
- All new serde fields use `#[serde(default)]` for backward compatibility with older snapshots
- **Kit frontend tasks:** Invoke `/ui-ux-pro-max` skill before implementing UI components
- RPM filtering for npm globals uses `rpm -qf <prefix>/<pkg>` — same pattern as system pip

---

### Task 1: Core type additions — METHOD_NPM_GLOBAL, LanguagePackage.pinned, schema bump

**Files:**
- Modify: `crates/core/src/util.rs`
- Modify: `crates/core/src/types/nonrpm.rs`
- Modify: `crates/core/src/snapshot.rs`
- Modify: all `LanguagePackage` constructor sites

**Interfaces:**
- Produces: `METHOD_NPM_GLOBAL` constant; `LanguagePackage.pinned: bool` field; `SCHEMA_VERSION = 22`

**blocked_by:** none

- [ ] **Step 1: Add METHOD_NPM_GLOBAL constant**

```rust
// In crates/core/src/util.rs, after METHOD_GEM_SYSTEM:
pub const METHOD_NPM_GLOBAL: &str = "npm global";
```

- [ ] **Step 2: Add pinned field to LanguagePackage**

```rust
// In crates/core/src/types/nonrpm.rs:
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguagePackage {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub pinned: bool,
}
```

- [ ] **Step 3: Bump schema version**

```rust
// In crates/core/src/snapshot.rs:
pub const SCHEMA_VERSION: u32 = 22;
```

Update `MIN_SCHEMA` if it gates on exact match — the existing `MIN_SCHEMA` check uses `SCHEMA_VERSION` as both floor and ceiling, so bumping `SCHEMA_VERSION` automatically updates the range. Verify old v21 snapshots still load by ensuring `MIN_SCHEMA` stays at 21 or below:

```rust
const MIN_SCHEMA: u32 = 16; // or whatever the current floor is
```

- [ ] **Step 4: Fix constructor sites**

Search for `LanguagePackage {` constructors and add `pinned: false` (or use `..Default::default()`). Key sites: `nonrpm.rs` tests, `language_packages.rs` test fixtures, aggregate merge.

- [ ] **Step 5: Add backward-compat deserialization test**

```rust
#[test]
fn language_package_without_pinned_deserializes() {
    let json = r#"{"name":"pm2","version":"5.3.0"}"#;
    let pkg: LanguagePackage = serde_json::from_str(json).unwrap();
    assert_eq!(pkg.name, "pm2");
    assert_eq!(pkg.version, "5.3.0");
    assert!(!pkg.pinned, "pinned defaults to false");
}
```

- [ ] **Step 6: Run tests, commit**

Run: `cargo test --workspace && cargo clippy -- -W clippy::all && cargo fmt --check`

```
feat(core): add METHOD_NPM_GLOBAL, LanguagePackage.pinned, bump schema to v22
```

---

### Task 2: npm global package detection

**Files:**
- Modify: `crates/collect/src/inspectors/nonrpm.rs`

**Interfaces:**
- Consumes: `METHOD_NPM_GLOBAL` from Task 1
- Produces: `NonRpmItem` entries with `method == METHOD_NPM_GLOBAL`, populated `packages: Vec<LanguagePackage>`, ecosystem `"npm"`, path = resolved global prefix

**blocked_by:** Task 1

- [ ] **Step 1: Add npm global detection function**

Add `scan_npm_global_packages()` to `nonrpm.rs`. This function:

1. Runs `npm root -g` to discover the actual global prefix. Falls back to well-known paths `/usr/lib/node_modules` and `/usr/local/lib/node_modules` if `npm` is not on PATH.
2. Runs `npm list -g --json` and parses the `dependencies` object for package names + versions (high confidence).
3. Walks each discovered prefix directory for `<prefix>/<pkg>/package.json` (unscoped) and `<prefix>/@scope/<pkg>/package.json` (scoped) — medium confidence.
4. Merges per-package: `npm list` entry preferred over directory walk entry for each package name.
5. RPM filters each package via `rpm -qf <prefix>/<pkg>`.
6. Produces one `NonRpmItem` per prefix with packages.

```rust
fn scan_npm_global_packages(
    exec: &dyn Executor,
    section: &mut NonRpmSoftwareSection,
    rpm_state: &RpmState,
    is_ostree: bool,
) {
    let prefixes = discover_npm_global_prefixes(exec);
    if prefixes.is_empty() {
        return;
    }

    let npm_list_packages = parse_npm_list_global(exec);

    for prefix in &prefixes {
        let dir_walk_packages = walk_npm_global_prefix(exec, prefix);
        let merged = merge_npm_global_packages(&npm_list_packages, &dir_walk_packages, prefix);
        let filtered = rpm_filter_npm_globals(exec, &merged, prefix);

        if filtered.is_empty() {
            continue;
        }

        let confidence = if npm_list_packages.is_some() {
            "high"
        } else {
            "medium"
        };

        section.items.push(NonRpmItem {
            path: prefix.clone(),
            name: format!("npm-globals-{}", env_hash(prefix)),
            method: METHOD_NPM_GLOBAL.into(),
            confidence: confidence.into(),
            lang: "npm".into(),
            packages: filtered,
            disposition: FindingKind::included(),
            ..Default::default()
        });
    }
}
```

- [ ] **Step 2: Implement helper functions**

`discover_npm_global_prefixes(exec)` — runs `npm root -g`, adds well-known fallbacks, deduplicates:

```rust
fn discover_npm_global_prefixes(exec: &dyn Executor) -> Vec<String> {
    let mut prefixes = Vec::new();

    let result = exec.run("npm", &["root", "-g"]);
    if result.exit_code == 0 {
        let path = result.stdout.trim().to_string();
        if !path.is_empty() {
            prefixes.push(path);
        }
    }

    for fallback in &["/usr/lib/node_modules", "/usr/local/lib/node_modules"] {
        if !prefixes.contains(&fallback.to_string()) {
            let check = exec.run("test", &["-d", fallback]);
            if check.exit_code == 0 {
                prefixes.push(fallback.to_string());
            }
        }
    }

    prefixes
}
```

`parse_npm_list_global(exec)` — runs `npm list -g --json`, returns `Option<HashMap<String, String>>` (name → version):

```rust
fn parse_npm_list_global(exec: &dyn Executor) -> Option<HashMap<String, String>> {
    let result = exec.run("npm", &["list", "-g", "--json"]);
    if result.exit_code != 0 {
        return None;
    }
    let parsed: serde_json::Value = serde_json::from_str(&result.stdout).ok()?;
    let deps = parsed.get("dependencies")?.as_object()?;
    let mut packages = HashMap::new();
    for (name, info) in deps {
        if let Some(version) = info.get("version").and_then(|v| v.as_str()) {
            packages.insert(name.clone(), version.to_string());
        }
    }
    Some(packages)
}
```

`walk_npm_global_prefix(exec, prefix)` — directory walk for unscoped + scoped packages:

```rust
fn walk_npm_global_prefix(exec: &dyn Executor, prefix: &str) -> Vec<LanguagePackage> {
    let mut packages = Vec::new();
    let result = exec.run("ls", &["-1", prefix]);
    if result.exit_code != 0 {
        return packages;
    }

    for entry in result.stdout.lines() {
        let entry = entry.trim();
        if entry.is_empty() || entry.starts_with('.') {
            continue;
        }
        if entry.starts_with('@') {
            // Scoped package: walk @scope/ for sub-packages
            let scope_path = format!("{}/{}", prefix, entry);
            let scope_result = exec.run("ls", &["-1", &scope_path]);
            if scope_result.exit_code == 0 {
                for sub in scope_result.stdout.lines() {
                    let sub = sub.trim();
                    if sub.is_empty() || sub.starts_with('.') {
                        continue;
                    }
                    let pkg_name = format!("{}/{}", entry, sub);
                    if let Some(pkg) = read_npm_package_json(exec, prefix, &pkg_name) {
                        packages.push(pkg);
                    }
                }
            }
        } else {
            if let Some(pkg) = read_npm_package_json(exec, prefix, entry) {
                packages.push(pkg);
            }
        }
    }

    packages
}
```

`read_npm_package_json(exec, prefix, pkg_name)` — reads `package.json` for name and version:

```rust
fn read_npm_package_json(
    exec: &dyn Executor,
    prefix: &str,
    pkg_name: &str,
) -> Option<LanguagePackage> {
    let pkg_json_path = format!("{}/{}/package.json", prefix, pkg_name);
    let content = exec.read_file(std::path::Path::new(&pkg_json_path)).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    let name = parsed.get("name")?.as_str()?.to_string();
    let version = parsed
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some(LanguagePackage {
        name,
        version,
        pinned: false,
    })
}
```

`merge_npm_global_packages()` — npm list entry wins per package name:

```rust
fn merge_npm_global_packages(
    npm_list: &Option<HashMap<String, String>>,
    dir_walk: &[LanguagePackage],
    prefix: &str,
) -> Vec<LanguagePackage> {
    let mut merged: HashMap<String, LanguagePackage> = HashMap::new();

    // Directory walk entries first (lower priority)
    for pkg in dir_walk {
        merged.insert(pkg.name.clone(), pkg.clone());
    }

    // npm list entries override (higher priority)
    if let Some(npm_list) = npm_list {
        for (name, version) in npm_list {
            merged.insert(
                name.clone(),
                LanguagePackage {
                    name: name.clone(),
                    version: version.clone(),
                    pinned: false,
                },
            );
        }
    }

    let mut result: Vec<LanguagePackage> = merged.into_values().collect();
    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}
```

`rpm_filter_npm_globals()` — filter RPM-owned packages:

```rust
fn rpm_filter_npm_globals(
    exec: &dyn Executor,
    packages: &[LanguagePackage],
    prefix: &str,
) -> Vec<LanguagePackage> {
    packages
        .iter()
        .filter(|pkg| {
            let pkg_path = format!("{}/{}", prefix, pkg.name);
            let result = exec.run("rpm", &["-qf", &pkg_path]);
            result.exit_code != 0 // keep if NOT owned by an RPM
        })
        .cloned()
        .collect()
}
```

- [ ] **Step 3: Wire into inspector**

Call `scan_npm_global_packages()` from the main `inspect()` function, alongside existing `scan_npm_packages()`.

- [ ] **Step 4: Add unit tests with mock executor**

Test cases:
1. `npm list -g` + directory walk both available → merge, prefer npm list versions
2. `npm` not on PATH → directory walk fallback only (medium confidence)
3. Scoped packages (`@angular/cli`, `@types/node`) discovered
4. RPM-owned packages filtered out
5. Multiple prefixes produce separate `NonRpmItem` entries
6. Empty prefix (no packages) → no item produced

- [ ] **Step 5: Run tests, commit**

Run: `cargo test -p inspectah-collect -- npm_global`

```
feat(collect): detect npm global packages via npm list and directory walk
```

---

### Task 3: C-extension detection for pip environments

**Files:**
- Modify: `crates/collect/src/inspectors/nonrpm.rs`

**Interfaces:**
- Produces: `NonRpmItem.has_c_extensions = true` when `.so` files found in site-packages

**blocked_by:** none (independent)

- [ ] **Step 1: Add C-extension scanning**

After inventorying a pip environment, scan the `site-packages/` directory tree for `.so` files:

```rust
fn detect_c_extensions(exec: &dyn Executor, site_packages_path: &str) -> bool {
    let result = exec.run(
        "find",
        &[site_packages_path, "-name", "*.so", "-type", "f", "-print", "-quit"],
    );
    result.exit_code == 0 && !result.stdout.trim().is_empty()
}
```

Call this from the pip scanning functions (both `scan_pip_packages` for venvs and the system-pip path) and set `item.has_c_extensions = true` when detected.

- [ ] **Step 2: Add tests**

Test cases:
1. Package subdirectory `.so` (e.g., `site-packages/numpy/core/_multiarray.so`) → `has_c_extensions: true`
2. Top-level `.so` (e.g., `site-packages/ujson.cpython-311-x86_64-linux-gnu.so`) → `has_c_extensions: true`
3. No `.so` files → `has_c_extensions: false`

- [ ] **Step 3: Run tests, commit**

```
feat(collect): detect C-extension shared objects in pip environments
```

---

### Task 4: Scan expansion — --scan-home, --scan-path, /var/www default, scope persistence

**Files:**
- Modify: `crates/collect/src/inspectors/nonrpm.rs` (SCAN_ROOTS, root resolution)
- Modify: `crates/cli/src/commands/scan.rs` (CLI flags, scan root assembly)
- Modify: `crates/core/src/snapshot.rs` (meta keys)

**Interfaces:**
- Produces: `--scan-home` and `--scan-path` CLI flags; `/var/www` in default roots; `scan_roots`, `scan_home_users`, `scan_extra_paths` in snapshot meta

**blocked_by:** none (independent)

- [ ] **Step 1: Add /var/www to SCAN_ROOTS**

```rust
// In crates/collect/src/inspectors/nonrpm.rs:
const SCAN_ROOTS: &[&str] = &["/opt", "/srv", "/usr/local", "/var/www"];
```

- [ ] **Step 2: Add CLI flags to scan.rs**

```rust
// In ScanArgs struct:
#[arg(long, value_name = "all|USER,...")]
pub scan_home: Option<String>,

#[arg(long, value_name = "PATH", action = ArgAction::Append)]
pub scan_path: Vec<String>,
```

- [ ] **Step 3: Implement scan root assembly**

In the scan command, build the effective root list from defaults + flags:

```rust
fn build_scan_roots(
    args: &ScanArgs,
    exec: &dyn Executor,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut roots: Vec<String> = SCAN_ROOTS.iter().map(|s| s.to_string()).collect();
    let mut home_users = Vec::new();
    let mut extra_paths = Vec::new();

    // --scan-home
    if let Some(ref home_arg) = args.scan_home {
        if home_arg.is_empty() {
            eprintln!("Error: --scan-home requires 'all' or a comma-separated user list");
            std::process::exit(1);
        }
        let users = resolve_home_users(exec, home_arg);
        home_users = users.iter().map(|(u, _)| u.clone()).collect();
        for (_, home_dir) in &users {
            if !roots.iter().any(|r| home_dir.starts_with(r)) {
                roots.push(home_dir.clone());
            }
        }
    }

    // --scan-path
    for path in &args.scan_path {
        extra_paths.push(path.clone());
        if !roots.iter().any(|r| path.starts_with(r)) {
            roots.push(path.clone());
        }
    }

    (roots, home_users, extra_paths)
}
```

- [ ] **Step 4: Implement resolve_home_users()**

```rust
fn resolve_home_users(
    exec: &dyn Executor,
    spec: &str,
) -> Vec<(String, String)> {
    let mut users = Vec::new();
    if spec == "all" {
        let result = exec.run("getent", &["passwd"]);
        if result.exit_code == 0 {
            for line in result.stdout.lines() {
                let fields: Vec<&str> = line.split(':').collect();
                if fields.len() >= 6 {
                    let uid: u32 = fields[2].parse().unwrap_or(0);
                    if uid >= 1000 {
                        users.push((fields[0].to_string(), fields[5].to_string()));
                    }
                }
            }
        }
    } else {
        for username in spec.split(',') {
            let username = username.trim();
            let result = exec.run("getent", &["passwd", username]);
            if result.exit_code == 0 {
                let fields: Vec<&str> = result.stdout.trim().split(':').collect();
                if fields.len() >= 6 {
                    users.push((fields[0].to_string(), fields[5].to_string()));
                }
            } else {
                eprintln!("Warning: user '{}' not found, skipping", username);
            }
        }
    }
    users
}
```

- [ ] **Step 5: Validate --scan-path entries**

Check each `--scan-path` exists. Warn on broad paths (fewer than 2 components). Skip missing paths with stderr warning.

- [ ] **Step 6: Persist scan scope in snapshot meta**

After building roots, store in `InspectionSnapshot.meta`:

```rust
meta.insert("scan_roots".into(), serde_json::to_value(&roots).unwrap());
meta.insert("scan_home_users".into(), serde_json::to_value(&home_users).unwrap());
meta.insert("scan_extra_paths".into(), serde_json::to_value(&extra_paths).unwrap());
```

- [ ] **Step 7: Pass effective roots to inspector**

The nonrpm inspector currently uses the hardcoded `SCAN_ROOTS` constant. Change the scan functions to accept `&[String]` roots parameter instead of using the constant directly. Thread the effective roots from the CLI through `InspectionContext` or a new field.

- [ ] **Step 8: Add CLI tests**

Test cases per spec:
1. `--scan-home` bare flag (no argument) → error with help text
2. `--scan-home all` discovers users with UID >= 1000
3. `--scan-home nonexistent` warns and continues
4. `--scan-home nginx` (system user, UID < 1000) included when explicitly named
5. `--scan-path /nonexistent` warns and continues
6. Broad `--scan-path /` produces warning
7. Duplicate suppression (home dir under existing root)
8. `--inspect-only` stdout remains parseable JSON with `--scan-home`/`--scan-path` active (spec §6 CLI test + §3 Output Channel Contract): scan-root headers and warnings go to stderr only, so `serde_json::from_str::<Value>(stdout)` succeeds — verifies the new stderr writes never leak to stdout

- [ ] **Step 9: Run tests, commit**

```
feat(cli,collect): add --scan-home, --scan-path flags and /var/www default root
```

---

### Task 5: Renderer fixes — system_site_packages + bundler deprecation

**Files:**
- Modify: `crates/pipeline/src/render/language_packages.rs`

**Interfaces:**
- Produces: `--system-site-packages` flag in venv creation; updated bundler syntax

**blocked_by:** none (independent)

- [ ] **Step 1: Add --system-site-packages to venv creation**

In `render_pip_item()`, when building the `python3 -m venv` command, check `item.system_site_packages`:

```rust
// High confidence venv with requirements.txt:
let venv_flags = if item.system_site_packages {
    "--system-site-packages "
} else {
    ""
};
lines.push(format!("RUN python3 -m venv {venv_flags}{abs_path} \\"));
```

Apply to ALL venv creation branches (high-confidence with requirements.txt, medium-confidence with dist-info, and the commented-out versions).

- [ ] **Step 2: Fix bundler deprecation**

Replace `bundle install --deployment` (lines 360 and 375) with the new syntax:

```rust
// High confidence (active):
lines.push(format!(
    "RUN cd {project_path} && bundle config set --local deployment 'true' && bundle install"
));

// Medium confidence (commented):
lines.push(format!(
    "# RUN cd {project_path} && bundle config set --local deployment 'true' && bundle install"
));
```

- [ ] **Step 3: Add tests**

Test cases:
1. `system_site_packages: true` → output contains `--system-site-packages`
2. `system_site_packages: false` → output does NOT contain `--system-site-packages`
3. Gem high confidence → output contains `bundle config set --local deployment`
4. Gem high confidence → output does NOT contain `bundle install --deployment`
5. Gem medium confidence → commented version uses new syntax

- [ ] **Step 4: Update existing tests that assert old bundler syntax**

The test at line 625 asserts `output.contains("bundle install --deployment")` — update to assert the new syntax.

- [ ] **Step 5: Run tests, commit**

```
fix(pipeline): add --system-site-packages to venv creation and fix bundler deprecation
```

---

### Task 6: npm global renderer — unpinned and pinned rendering

**Files:**
- Modify: `crates/pipeline/src/render/language_packages.rs`

**Interfaces:**
- Consumes: `METHOD_NPM_GLOBAL` from Task 1, npm global `NonRpmItem` entries from Task 2
- Produces: Containerfile lines for npm global packages, respecting `LanguagePackage.pinned`

**blocked_by:** Task 1, Task 2

- [ ] **Step 1: Add npm global environment check**

```rust
fn is_npm_global_env(item: &NonRpmItem) -> bool {
    item.method == METHOD_NPM_GLOBAL
}
```

Update `is_language_env()` to include npm globals:

```rust
pub fn is_language_env(item: &NonRpmItem) -> bool {
    is_pip_env(item) || is_npm_env(item) || is_npm_global_env(item) || is_gem_env(item)
}
```

- [ ] **Step 2: Add npm global renderer**

```rust
fn render_npm_global_item(item: &NonRpmItem) -> Vec<String> {
    let mut lines = Vec::new();
    let prefix = format!("/{}", item.path.trim_start_matches('/'));

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
    let pkg_list: String = item
        .packages
        .iter()
        .map(|p| {
            if p.pinned && !p.version.is_empty() {
                format!("{}@{}", p.name, p.version)
            } else {
                p.name.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    if effective_confidence == HIGH_CONFIDENCE {
        lines.push(format!(
            "# npm global packages: {prefix} ({method_label})"
        ));
        if item.has_c_extensions {
            lines.push(
                "# WARNING: environment contains native addons — \
                 build tools (gcc, node-gyp) may be needed"
                    .into(),
            );
        }
        lines.push(format!("RUN npm install -g {pkg_list}"));
    } else {
        lines.push(format!(
            "# npm global packages: {prefix} ({method_label})"
        ));
        lines.push(format!("# RUN npm install -g {pkg_list}"));
    }

    lines
}
```

- [ ] **Step 3: Wire into language_package_lines() with runtime check**

Add a `render_npm_global_section()` wrapper that emits the `nodejs` runtime
warning (spec §1 Runtime Check — same behavior as `render_npm_section`, which
the new npm-global render path does NOT inherit automatically) before
rendering each item:

```rust
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
```

Then add npm global items to the filter and rendering in `language_package_lines()`:

```rust
let npm_global_items: Vec<&NonRpmItem> = nrs.items.iter()
    .filter(|i| is_npm_global_env(i))
    .collect();

// After existing npm section rendering:
if !npm_global_items.is_empty() {
    lines.extend(render_npm_global_section(&npm_global_items, &rpm_names));
}
```

- [ ] **Step 4: Add tests**

Test cases:
1. npm globals rendered unpinned by default (`RUN npm install -g pm2 typescript`)
2. npm globals rendered pinned (`RUN npm install -g pm2@5.3.0 typescript@5.4.2`)
3. Mixed pin state (`RUN npm install -g pm2@5.3.0 typescript`)
4. Scoped package rendering (`RUN npm install -g @angular/cli`)
5. Excluded npm globals → commented out
6. C-extension warning rendered when `has_c_extensions: true`
7. Existing pip/gem rendering unchanged by `pinned` field
8. Runtime check (spec §1): `nodejs` absent from RPM list → `nodejs not found in RPM package list` warning emitted for the npm-global section; warning absent when `nodejs` is present or when there is no RPM data

- [ ] **Step 5: Run tests, commit**

```
feat(pipeline): render npm global packages with per-package version pinning
```

---

### Task 7: Pin state refine contract — ItemId, ops, session persistence

**Files:**
- Modify: `crates/refine/src/types.rs`
- Modify: `crates/refine/src/session.rs`

**Interfaces:**
- Consumes: `LanguagePackage.pinned` from Task 1
- Produces: `ItemId::LanguagePackage`, `RefinementOp::SetPackagePin`, `RefinementOp::SetBulkPackagePin`, session apply/validate/project logic

**blocked_by:** Task 1

- [ ] **Step 1: Add ItemId::LanguagePackage variant**

```rust
// In crates/refine/src/types.rs, add to ItemId enum:
LanguagePackage {
    ecosystem: String,
    env_path: String,
    package: String,
},
```

- [ ] **Step 2: Add RefinementOp variants**

```rust
// In crates/refine/src/types.rs, add to RefinementOp enum:
SetPackagePin {
    item_id: ItemId,
    pinned: bool,
},
SetBulkPackagePin {
    ecosystem: String,
    env_path: String,
    pinned: bool,
},
```

- [ ] **Step 3: Implement session apply logic**

In `session.rs`, add handling for the new ops in `apply()`:

```rust
RefinementOp::SetPackagePin { ref item_id, pinned } => {
    if let ItemId::LanguagePackage { ecosystem, env_path, package } = item_id {
        self.set_package_pin(ecosystem, env_path, package, *pinned)?;
    }
}
RefinementOp::SetBulkPackagePin { ref ecosystem, ref env_path, pinned } => {
    self.set_bulk_package_pin(ecosystem, env_path, *pinned)?;
}
```

Implement `set_package_pin()` — finds the `NonRpmItem` by ecosystem+path, then finds the `LanguagePackage` by name and sets `pinned`:

```rust
fn set_package_pin(
    &mut self,
    ecosystem: &str,
    env_path: &str,
    package: &str,
    pinned: bool,
) -> Result<(), RefineError> {
    let nrs = self.original.non_rpm_software.as_mut()
        .ok_or_else(|| RefineError::UnknownTarget(format!("{}:{}", ecosystem, env_path)))?;
    let item = nrs.items.iter_mut()
        .find(|i| i.method.contains(ecosystem) && i.path == env_path)
        .ok_or_else(|| RefineError::UnknownTarget(format!("{}:{}", ecosystem, env_path)))?;
    let pkg = item.packages.iter_mut()
        .find(|p| p.name == package)
        .ok_or_else(|| RefineError::UnknownTarget(package.to_string()))?;
    pkg.pinned = pinned;
    self.mark_dirty();
    Ok(())
}
```

Implement `set_bulk_package_pin()` similarly — sets `pinned` on ALL packages in the environment.

- [ ] **Step 4: Implement validate_target()**

Add validation for the new op variants in `validate_target()`:

```rust
RefinementOp::SetPackagePin { ref item_id, .. } => {
    if let ItemId::LanguagePackage { ecosystem, env_path, package } = item_id {
        // Validate environment exists and package exists within it
    }
}
```

- [ ] **Step 5: Add serde round-trip tests**

```rust
#[test]
fn set_package_pin_serde_roundtrip() {
    let op = RefinementOp::SetPackagePin {
        item_id: ItemId::LanguagePackage {
            ecosystem: "npm".into(),
            env_path: "/usr/lib/node_modules".into(),
            package: "pm2".into(),
        },
        pinned: true,
    };
    let json = serde_json::to_string(&op).unwrap();
    let deserialized: RefinementOp = serde_json::from_str(&json).unwrap();
    // verify round-trip
}
```

- [ ] **Step 6: Add session persistence tests**

Test cases:
1. `SetPackagePin` op round-trips through autosave/reload
2. `SetBulkPackagePin` sets all packages in target environment
3. Pin state survives session reload
4. Exported Containerfile matches pinned/unpinned state

- [ ] **Step 7: Run tests, commit**

```
feat(refine): add SetPackagePin and SetBulkPackagePin ops with session persistence
```

---

### Task 8: Pin state web adapter + DTO

**Files:**
- Modify: `crates/web/src/web_types.rs`
- Modify: `crates/web/src/adapter.rs`
- Modify: `crates/web/src/handlers.rs`

**Interfaces:**
- Consumes: `LanguagePackage.pinned` from Task 1, `SetPackagePin` / `SetBulkPackagePin` ops from Task 7
- Produces: `LanguagePackageDto`, updated `LanguagePackageEnvDto`, API endpoints for pin state changes

**blocked_by:** Task 1, Task 7

- [ ] **Step 1: Add LanguagePackageDto**

```rust
// In crates/web/src/web_types.rs:
#[derive(Serialize, Clone, Debug)]
pub struct LanguagePackageDto {
    pub name: String,
    pub detected_version: String,
    pub pinned: bool,
}
```

- [ ] **Step 2: Update LanguagePackageEnvDto**

Replace `packages: Vec<String>` with structured package list and add new fields:

```rust
pub struct LanguagePackageEnvDto {
    pub ecosystem: String,
    pub path: String,
    pub method: String,
    pub packages: Vec<LanguagePackageDto>,  // was Vec<String>
    pub confidence: String,
    pub manifest_basis: String,
    pub include: bool,
    pub has_c_extensions: bool,
    pub system_site_packages: bool,
}
```

- [ ] **Step 3: Update adapter conversion**

In `adapter.rs`, where `LanguagePackageEnvDto` is built from `NonRpmItem`, convert packages to structured DTOs:

```rust
packages: item.packages.iter().map(|p| LanguagePackageDto {
    name: p.name.clone(),
    detected_version: p.version.clone(),
    pinned: p.pinned,
}).collect(),
has_c_extensions: item.has_c_extensions,
system_site_packages: item.system_site_packages,
```

- [ ] **Step 4: Add API endpoints for pin state**

Add `POST /api/set-package-pin` and `POST /api/set-bulk-package-pin` handlers:

```rust
pub async fn set_package_pin(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SetPackagePinRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut session = state.session.lock().unwrap();
    session.apply(RefinementOp::SetPackagePin {
        item_id: ItemId::LanguagePackage {
            ecosystem: payload.ecosystem,
            env_path: payload.env_path,
            package: payload.package,
        },
        pinned: payload.pinned,
    })?;
    let view = crate::adapter::build_web_view(&session);
    Ok(Json(serde_json::to_value(&view).unwrap()))
}
```

- [ ] **Step 5: Add tests**

Test: DTO conversion produces correct structure with pinned/unpinned packages. API endpoint round-trips pin state.

- [ ] **Step 6: Run tests, commit**

```
feat(web): add LanguagePackageDto, structured package list, and pin state API
```

---

### Task 9: LanguagePackageList structural overhaul (Kit)

**Files:**
- Modify: `crates/web/ui/src/components/LanguagePackageList.tsx`
- Modify: `crates/web/ui/src/api/types.ts`
- Modify: `crates/web/ui/src/api/client.ts`

**Interfaces:**
- Consumes: `LanguagePackageDto`, `LanguagePackageEnvDto` from Task 8
- Produces: Expandable package sublists, C-extension badge, system_site_packages badge

**blocked_by:** Task 8

- [ ] **Step 1: Update TypeScript types**

```typescript
// In api/types.ts:
export interface LanguagePackageDto {
  name: string;
  detected_version: string;
  pinned: boolean;
}

// Update LanguagePackageEnv:
export interface LanguagePackageEnv {
  ecosystem: string;
  path: string;
  method: string;
  packages: LanguagePackageDto[];  // was string[]
  confidence: string;
  manifest_basis: string;
  include: boolean;
  has_c_extensions: boolean;
  system_site_packages: boolean;
}
```

- [ ] **Step 2: Add API client methods**

```typescript
export async function setPackagePin(
  ecosystem: string, envPath: string, pkg: string, pinned: boolean
): Promise<ViewResponse> {
  return postJson("/api/set-package-pin", { ecosystem, env_path: envPath, package: pkg, pinned });
}

export async function setBulkPackagePin(
  ecosystem: string, envPath: string, pinned: boolean
): Promise<ViewResponse> {
  return postJson("/api/set-bulk-package-pin", { ecosystem, env_path: envPath, pinned });
}
```

- [ ] **Step 3: Add expandable package sublist**

For npm global environments (`method === "npm global"`), render as expandable rows. Clicking the environment row toggles expand/collapse revealing the package sublist. Each package shows name, version, and pin toggle.

- [ ] **Step 4: Add C-extension and system_site_packages badges**

```tsx
{env.has_c_extensions && (
  <span role="status" aria-label="This environment contains C extensions">
    <Label color="orange" isCompact>C extensions</Label>
  </span>
)}
{env.system_site_packages && (
  <span role="status" aria-label="This environment uses system site-packages">
    <Label color="blue" isCompact>system site-packages</Label>
  </span>
)}
```

- [ ] **Step 5: Add component tests**

Test: expandable rows render for npm globals, badges appear when flags set, badges absent when flags false, package list shows structured data.

- [ ] **Step 6: Run tests, commit**

```
feat(web-ui): add expandable package sublists and environment badges
```

---

### Task 10: Pin interaction UI (Kit)

**Files:**
- Modify: `crates/web/ui/src/components/LanguagePackageList.tsx`
- Modify: `crates/web/ui/src/hooks/useKeyboard.ts`

**Interfaces:**
- Consumes: `setPackagePin`, `setBulkPackagePin` client methods from Task 9
- Produces: Pin toggle checkboxes, bulk pin/unpin button, keyboard navigation, search integration, accessibility

**blocked_by:** Task 9

- [ ] **Step 1: Add pin toggle checkboxes**

Each package row in the expanded sublist gets a pin checkbox:

```tsx
<input
  type="checkbox"
  checked={pkg.pinned}
  onChange={() => onPinToggle(env.ecosystem, env.path, pkg.name, !pkg.pinned)}
  aria-label={`Pin ${pkg.name} to version ${pkg.detected_version}`}
/>
```

- [ ] **Step 2: Add bulk pin/unpin button**

At the bottom of each expanded package sublist:

```tsx
<Button
  variant="link"
  onClick={() => onBulkPin(env.ecosystem, env.path, !allPinned)}
  aria-label={`${allPinned ? "Unpin" : "Pin"} all packages in ${env.ecosystem} globals ${env.path}`}
>
  {allPinned ? "Unpin all" : "Pin all"}
</Button>
```

Button label always describes its next action, not current state.

- [ ] **Step 3: Implement keyboard navigation**

Per spec accessibility contract:
- `Space` on pin checkbox toggles it
- `Enter` on environment row toggles expand/collapse
- `ArrowDown`/`ArrowUp` move between package rows
- `Escape` within sublist collapses parent, focus returns to environment row

- [ ] **Step 4: Add aria-live announcements**

After bulk pin/unpin: announce `"Pinned N packages in npm globals"` / `"Unpinned N packages"`.

- [ ] **Step 5: Add search integration**

If search matches a package name (e.g., `pm2`), auto-expand the matching environment row and highlight the matching package row.

- [ ] **Step 6: Add component tests**

Test: pin toggle calls API, bulk pin sets all, bulk unpin clears all, keyboard navigation works, search auto-expands, aria announcements fire.

- [ ] **Step 7: Run tests, commit**

```
feat(web-ui): add per-package version pinning with keyboard navigation and search
```

---

### Task 11: Driftify fixtures

**Files:**
- Modify: `src/profiles/nonrpm.rs` (in driftify repo at `/Users/mrussell/Work/bootc-migration/driftify/`)

**Interfaces:**
- Produces: Test fixture data for npm globals, C-extensions, scan-home paths, /var/www deployments, system_site_packages

**blocked_by:** Task 2, Task 3

- [ ] **Step 1: Add npm global fixtures**

Add mock data for:
- Unscoped packages: `pm2/package.json`, `typescript/package.json`
- Scoped packages: `@angular/cli/package.json`, `@types/node/package.json`
- Multi-prefix: packages in both `/usr/lib/node_modules/` and `/usr/local/lib/node_modules/`

- [ ] **Step 2: Add C-extension fixtures**

Add mock `.so` files:
- Package subdir: `site-packages/numpy/core/_multiarray.so`
- Top-level: `site-packages/ujson.cpython-311-x86_64-linux-gnu.so`

- [ ] **Step 3: Add scan-home and /var/www fixtures**

Add language environments under simulated user home directories and `/var/www/`.

- [ ] **Step 4: Add system_site_packages fixture**

Add a venv with `include-system-site-packages = true` in `pyvenv.cfg`.

- [ ] **Step 5: Run driftify tests, commit**

```
feat(driftify): add npm global, C-extension, scan-home, and system_site_packages fixtures
```

---

## Task Dependency Graph

```
T1 (core types) ──┬──→ T2 (npm global detection) ──→ T6 (npm global renderer)
                   ├──→ T7 (pin state refine) ──→ T8 (pin state adapter) ──→ T9 (UI structure) ──→ T10 (pin interaction)
                   └──→ T8 (pin state adapter)
T3 (C-extension) ──→ T9 (badges)
T4 (scan expansion) — independent
T5 (renderer fixes) ──→ T9 (system_site_packages badge)
T2, T3 ──→ T11 (driftify)
```

**Parallelism:** T1, T3, T4, T5 can start simultaneously. T2 and T7 can parallel after T1. T6 needs T1+T2. T8 needs T1+T7. Frontend track (T9-T10) starts once T8 lands. T11 runs after T2+T3.
