# Non-RPM Replication Plan 4: Compose Raw Content + Aggregate Support

<!-- agentic: tang | model: opus | sdd: true | thorn-checkpoint: after T3, T5a, T8, T9a -->
<!-- depends-on: plan1 (data model, shared contracts), plan2 (UnmanagedFile types, ItemId::UnmanagedFile), plan3 (LanguagePackageEnv UI types) -->

## Goal

Wire compose raw YAML through collection, redaction, export, and
Containerfile rendering. Then add aggregate-mode sections for Language
Packages and Unmanaged Files, using the identity models established in
Plans 1-3.

**Success criteria:**
1. `ComposeFile.raw_content` populated during collection, redacted when
   snapshot is in redacted state, exported under `compose/` in the tarball
2. Containerfile contains a compose comment block with Quadlet nudge and
   correct doc links (no COPY/RUN)
3. `compose/` added to the export allowlist, export contract test passes
4. `docs/reference/output-artifacts.md` documents the `compose/` root
5. Aggregate view returns `language_packages` and `unmanaged_files`
   sections with zone-based layout
6. Aggregate merge uses correct identity keys: ecosystem+path for language
   envs, file path for unmanaged files
7. Prevalence-based defaults: 100% = include, <100% = exclude — verified
   by explicit merge/handler tests per section
8. Variant handling: package-list diff for language envs, content-hash
   for unmanaged files — backed by declared backend payloads
9. All existing tests pass, new tests cover each task
10. Clippy clean, `cargo fmt`, no warnings

## Architecture

```
Plan 4 has three tracks:

Track A: Compose raw content (Tasks 1-4a)
  containers.rs → types/containers.rs → session.rs → containerfile.rs → docs
  collector        data model            export        rendering        artifacts

Track B: Aggregate sections (Tasks 5-9a)
  nonrpm.rs → merge.rs → aggregate_handlers.rs → aggregate TS types → API/DTO contract
  agg fields   identity    section builder         frontend DTOs        response shapes

Track C: Aggregate UI (Tasks 10-14)
  AggregateItemRow → ItemDetailPane → VariantView → Search → Sidebar
  row metadata       detail content    variant diff   index    wiring
```

Track A modifies the existing compose pipeline end-to-end. Track B adds
new aggregate sections following the exact pattern used by packages,
configs, services, sysctls, and containers. Track C wires Plan 3's
single-host components into aggregate mode using the API contract from
Task 9a.

## Tech Stack

- Rust (inspectah-core, inspectah-collect, inspectah-refine,
  inspectah-pipeline, inspectah-web)
- TypeScript (React, PatternFly — aggregate view types only)
- serde, sha2, regex

## Spec Reference

`process-docs/specs/proposed/2026-06-27-non-rpm-replication.md`

Sections consumed by this plan:
- "Compose Stacks" — collector changes, sensitivity gating, Containerfile
  rendering, tarball layout
- "Aggregate Mode" — identity model, prevalence defaults, variant handling
- "Data Model Changes" — ComposeFile extension, tarball layout

## Global Constraints

1. Clippy clean (`cargo clippy -- -D warnings`), `cargo fmt --check`
2. No team names in code, comments, or commit messages
3. Conventional commits: `type(scope): description`
4. Attribution: `Assisted-by: Claude Code (Opus 4.6)`
5. Consume Plan 1's shared contracts exactly — do not redefine ItemId
   variants, method strings, or confidence gates
6. Consume Plan 2's `UnmanagedFileSection` contract exactly —
   `items: Vec<UnmanagedFile>`, `total_size: u64`, `total_count: usize`.
   Do not rename fields or omit section-level totals.
7. Compose stays reference-only: no include toggles, no Containerfile
   COPY/RUN directives
8. Aggregate sections use zone-based layout via `build_section()` — same
   pattern as packages, configs, services, sysctls
9. Compose secret scrubber lives in `inspectah-core` (shared utility) —
   do not defer crate placement to implementation time

## Shared Contracts Consumed from Plan 1

### ItemId Variants Used

| Plan 4 Context | ItemId Variant | Identity Key |
|----------------|---------------|--------------|
| Compose export | `ItemId::Compose { path }` | Compose file path (existing) |
| Language Packages aggregate | `ItemId::LanguageEnv { ecosystem, path }` | `"pip:/opt/myapp/venv"` |
| Unmanaged Files aggregate | `ItemId::UnmanagedFile { path }` | Absolute file path |

### Export Allowlist Addition

| Root | Gate |
|------|------|
| `compose` | When compose files detected |

(Plan 2 adds `unmanaged` and `repoless-packages`.)

### NonRpmItem Fields Used for Aggregate

| Field | Aggregate Use |
|-------|---------------|
| `manifest_files` | Variant diff key for language envs |
| `packages` | Unified package list (`Vec<LanguagePackage>`) for all ecosystems |

### UnmanagedFile Contract from Plan 2

Plan 2 Task 1 defines the shared contract. Plan 4 consumes it as-is:

| Struct | Field | Type | Notes |
|--------|-------|------|-------|
| `UnmanagedFile` | `path` | `String` | Identity key |
| | `size` | `u64` | File size in bytes |
| | `file_type` | `FileType` | Detected file type |
| | `provenance` | `ProvenanceSignals` | Raw metadata + derived signals |
| | `include` | `bool` | Default true |
| | `locked` | `bool` | |
| | `acknowledged` | `bool` | |
| | `under_var` | `bool` | `/var` persistence warning |
| | `aggregate` | `Option<AggregatePrevalence>` | Populated in aggregate mode |
| `UnmanagedFileSection` | `items` | `Vec<UnmanagedFile>` | **Not `files`** |
| | `total_size` | `u64` | Sum of all file sizes |
| | `total_count` | `usize` | Number of cataloged files |

**Plan 4 additions** (Task 5a): `content_hash` and `variant_selection`
are aggregate-only fields not present in Plan 2. Task 5a adds them with
`serde(default)` for backward compatibility before any aggregate task
uses them.

## File Map

### Track A: Compose Raw Content

| Task | Files | Creates/Modifies |
|------|-------|------------------|
| T1 | `crates/core/src/types/containers.rs` | Modify: add `raw_content` field |
| T2 | `crates/collect/src/inspectors/containers.rs`, `crates/core/src/redaction.rs` | Modify: retain raw YAML; Create: scrubber in core |
| T3 | `crates/refine/src/session.rs`, `crates/refine/tests/export_contract_test.rs` | Modify: compose export + allowlist |
| T4 | `crates/pipeline/src/render/containerfile.rs` | Modify: compose comment block |
| T4a | `docs/reference/output-artifacts.md` | Modify: document `compose/` root |

### Track B: Aggregate Support

| Task | Files | Creates/Modifies |
|------|-------|------------------|
| T5 | `crates/core/src/aggregate/merge.rs` | Modify: NonRpmItem identity key |
| T5a | `crates/core/src/types/nonrpm.rs` | Modify: add aggregate-only fields to UnmanagedFile |
| T6 | `crates/core/src/aggregate/merge.rs` | Modify: add UnmanagedFile merge |
| T7 | `crates/web/src/aggregate_handlers.rs` | Modify: language_packages section |
| T8 | `crates/web/src/aggregate_handlers.rs` | Modify: unmanaged_files section |
| T9 | `crates/web/ui/src/api/types.ts` | Modify: aggregate TS DTOs |
| T9a | `crates/web/src/aggregate_handlers.rs`, `crates/web/ui/src/api/types.ts` | Modify: per-section metadata + variant payloads in response |

### Track C: Aggregate UI

| Task | Files | Creates/Modifies |
|------|-------|------------------|
| T10 | `crates/web/ui/src/components/aggregate/AggregateItemRow.tsx` | Modify: section-aware row metadata |
| T11 | `crates/web/ui/src/components/aggregate/ItemDetailPane.tsx` | Modify: detail pane content |
| T11a | `crates/web/src/aggregate_handlers.rs` | Modify: variant diff payloads for T12 |
| T12 | `crates/web/ui/src/components/aggregate/VariantView.tsx` | Modify: variant comparison UI |
| T13 | `crates/web/ui/src/components/aggregate/AggregateApp.tsx` | Modify: search scope |
| T14 | `crates/web/ui/src/components/aggregate/AggregateSidebar.tsx` | Modify: sidebar wiring |

---

## Task 1: ComposeFile — Add `raw_content` Field

**Files:**
- Modify: `crates/core/src/types/containers.rs`
- Test: existing roundtrip test in `containers.rs`

**Interfaces:**
- Produces: `ComposeFile.raw_content: Option<String>`
- Consumed by: Tasks 2, 3, 4

- [ ] **Step 1: Add `raw_content` field to `ComposeFile`**

In `crates/core/src/types/containers.rs`, add after the `aggregate` field
on `ComposeFile`:

```rust
    /// Raw compose YAML content, retained for verbatim export.
    /// Subject to redaction rules when snapshot is in redacted state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_content: Option<String>,
```

The struct currently has: `path`, `images`, `include`, `locked`,
`variant_selection`, `aggregate`. The new field goes after `aggregate`.

- [ ] **Step 2: Update roundtrip test**

Update the existing `test_container_section_roundtrip` test to include
`raw_content`:

```rust
    compose_files: vec![ComposeFile {
        path: "opt/myapp/docker-compose.yml".to_string(),
        images: vec![],
        include: true,
        raw_content: Some("version: '3'\nservices:\n  web:\n    image: nginx\n".to_string()),
        ..Default::default()
    }],
```

- [ ] **Step 3: Add raw_content serde test**

```rust
    #[test]
    fn compose_raw_content_roundtrip() {
        let cf = ComposeFile {
            path: "opt/app/docker-compose.yml".to_string(),
            raw_content: Some("services:\n  db:\n    image: postgres:16\n".to_string()),
            ..Default::default()
        };
        let json = serde_json::to_string(&cf).unwrap();
        assert!(json.contains("raw_content"));
        let parsed: ComposeFile = serde_json::from_str(&json).unwrap();
        assert_eq!(cf.raw_content, parsed.raw_content);
    }

    #[test]
    fn compose_raw_content_none_omitted() {
        let cf = ComposeFile {
            path: "opt/app/compose.yml".to_string(),
            ..Default::default()
        };
        let json = serde_json::to_string(&cf).unwrap();
        assert!(!json.contains("raw_content"));
    }
```

- [ ] **Step 4: Verify tests pass, commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
cargo test -p inspectah-core -- containers
git add crates/core/src/types/containers.rs
git commit -m "feat(core): add raw_content field to ComposeFile

Retains raw compose YAML for verbatim export. Subject to redaction
rules via sensitivity gating in the collector.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 2: Containers Inspector — Retain Raw YAML + Redaction Scrub

**Files:**
- Modify: `crates/collect/src/inspectors/containers.rs`
- Create: `crates/core/src/redaction.rs` (scrubber utility)
- Test: new unit tests in both files

**Interfaces:**
- Depends on: Task 1 (`ComposeFile.raw_content`)
- Produces: populated `raw_content` during collection
- Consumed by: Tasks 3, 4

**Crate home decision:** The `scrub_compose_secrets` function lives in
`inspectah-core` under `crates/core/src/redaction.rs`. Both
`inspectah-collect` (which already depends on `inspectah-core`) and
`inspectah-refine` (which already depends on `inspectah-core`) can reach
it without adding new cross-crate dependencies. If `redaction.rs` already
exists in core, add the function there. If not, create the module and add
`pub mod redaction;` to `crates/core/src/lib.rs`.

- [ ] **Step 1: Retain raw YAML in `find_compose_files`**

In `crates/collect/src/inspectors/containers.rs`, in the
`find_compose_files` function, the `content` variable already holds the
raw file content (line ~327). Currently it is only used for image
extraction and secret scanning. Retain it:

Change the `ComposeFile` construction at line ~363 from:

```rust
        files.push(ComposeFile {
            path: rel_path,
            images: parse_result.services,
            include: true,
            ..Default::default()
        });
```

To:

```rust
        files.push(ComposeFile {
            path: rel_path,
            images: parse_result.services,
            include: true,
            raw_content: Some(content.clone()),
            ..Default::default()
        });
```

Note: `content` is already a `String` from `exec.read_file()`. The clone
is necessary because `content` is borrowed by `scan_compose_env_secrets`
earlier, but that call completes before this point so the borrow is not
active. If the borrow checker disagrees, move the clone before the scan
call.

- [ ] **Step 2: Add `scrub_compose_secrets` in `inspectah-core`**

Create or extend `crates/core/src/redaction.rs`:

```rust
/// Scrubs secret-like values from compose YAML content.
///
/// Replaces values of environment variables whose names match
/// secret patterns with `<REDACTED>`. Handles both `KEY=VALUE` and
/// `KEY: value` patterns within `environment:` blocks.
///
/// Lives in inspectah-core so both inspectah-collect and
/// inspectah-refine can use it without cross-crate dependency issues.
pub fn scrub_compose_secrets(content: &str) -> String {
    const SECRET_PATTERNS: &[&str] = &[
        "PASSWORD", "PASSWD", "SECRET", "TOKEN", "API_KEY",
        "PRIVATE_KEY", "AUTH", "CREDENTIAL",
    ];

    let mut result = String::with_capacity(content.len());
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            result.push_str(line);
            result.push('\n');
            continue;
        }
        let upper = trimmed.to_uppercase();
        let is_secret = SECRET_PATTERNS
            .iter()
            .any(|pat| upper.contains(pat) && (trimmed.contains('=') || trimmed.contains(':')));
        if is_secret {
            // Replace the value portion while preserving indentation and key.
            if let Some(eq_pos) = trimmed.find('=') {
                let indent = &line[..line.len() - line.trim_start().len()];
                let key = &trimmed[..eq_pos + 1];
                result.push_str(indent);
                result.push_str(key);
                result.push_str("<REDACTED>");
                result.push('\n');
            } else if let Some(colon_pos) = trimmed.find(':') {
                let indent = &line[..line.len() - line.trim_start().len()];
                let key = &trimmed[..colon_pos + 1];
                result.push_str(indent);
                result.push_str(key);
                result.push_str(" <REDACTED>");
                result.push('\n');
            } else {
                result.push_str(line);
                result.push('\n');
            }
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }
    // Remove trailing newline added by the loop if original didn't have one.
    if !content.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }
    result
}
```

- [ ] **Step 3: Add unit tests for `scrub_compose_secrets`**

In `crates/core/src/redaction.rs` (or a test module):

```rust
    #[test]
    fn scrub_compose_secrets_redacts_eq_style() {
        let input = "services:\n  web:\n    environment:\n      DB_PASSWORD=hunter2\n      APP_PORT=8080\n";
        let scrubbed = scrub_compose_secrets(input);
        assert!(scrubbed.contains("DB_PASSWORD=<REDACTED>"));
        assert!(scrubbed.contains("APP_PORT=8080")); // not secret
        assert!(!scrubbed.contains("hunter2"));
    }

    #[test]
    fn scrub_compose_secrets_redacts_colon_style() {
        let input = "services:\n  db:\n    environment:\n      SECRET_KEY: my-secret\n      LANG: en_US\n";
        let scrubbed = scrub_compose_secrets(input);
        assert!(scrubbed.contains("SECRET_KEY: <REDACTED>"));
        assert!(scrubbed.contains("LANG: en_US")); // not secret
        assert!(!scrubbed.contains("my-secret"));
    }

    #[test]
    fn scrub_compose_secrets_preserves_comments_and_blanks() {
        let input = "# A comment\n\nservices:\n  app:\n    image: nginx\n";
        let scrubbed = scrub_compose_secrets(input);
        assert_eq!(input, scrubbed);
    }

    #[test]
    fn scrub_compose_secrets_handles_api_token() {
        let input = "    API_TOKEN=abc123\n";
        let scrubbed = scrub_compose_secrets(input);
        assert!(scrubbed.contains("API_TOKEN=<REDACTED>"));
    }
```

- [ ] **Step 4: Verify tests pass, commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
cargo test -p inspectah-core -- redaction
cargo test -p inspectah-collect -- containers
git add crates/core/src/redaction.rs crates/core/src/lib.rs \
       crates/collect/src/inspectors/containers.rs
git commit -m "feat(collect): retain raw compose YAML and add secret scrubber

find_compose_files now populates ComposeFile.raw_content with the
raw YAML. scrub_compose_secrets lives in inspectah-core::redaction
so both collect and refine can use it without cross-crate issues.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 3: Compose Export — Tarball + Allowlist

**Files:**
- Modify: `crates/refine/src/session.rs`
- Modify: `crates/refine/tests/export_contract_test.rs`
- Test: export contract test

**Interfaces:**
- Depends on: Tasks 1-2 (`ComposeFile.raw_content` populated,
  `inspectah_core::redaction::scrub_compose_secrets` available)
- Produces: `compose/` directory in tarball
- Consumed by: Task 4 (Containerfile references compose/), Task 4a (docs)

- [ ] **Step 1: Add `compose` to the export allowlist**

In `crates/refine/src/session.rs`, in `render_refine_export()`, add
`"compose"` to the `allowed_top_level` HashSet (line ~2471):

```rust
    let allowed_top_level: std::collections::HashSet<&str> = [
        "config",
        "drop-ins",
        "flatpak",
        "sysctl",
        "tuned",
        "env-files",
        "aggregate",
        "schema",
        "users",
        "compose",  // <-- add
        "inspection-snapshot.json",
        "Containerfile",
        "audit-report.md",
        "inspectah-users.ks",
        "inspectah-users.toml",
    ]
    .iter()
    .copied()
    .collect();
```

- [ ] **Step 2: Write compose files to the tarball staging directory**

In `render_refine_export()`, after the env-files materialization block
(step 2 in the function, around line ~2446) and before the
allowed_top_level cleanup, add compose file writing:

```rust
    // 2b. Materialize compose files (conditional)
    if let Some(ref containers) = snap.containers {
        let compose_files: Vec<_> = containers
            .compose_files
            .iter()
            .filter(|c| c.raw_content.is_some())
            .collect();
        if !compose_files.is_empty() {
            let compose_root = out.join("compose");
            let is_redacted = snap
                .redaction
                .as_ref()
                .map(|r| {
                    matches!(
                        r.state,
                        inspectah_core::types::redaction::RedactionState::Redacted
                    )
                })
                .unwrap_or(false);

            for cf in &compose_files {
                // Mirror directory structure: compose/opt/myapp/docker-compose.yml
                let dest = compose_root.join(&cf.path);
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| RefineError::TarballError(e.to_string()))?;
                }
                let content = if is_redacted {
                    inspectah_core::redaction::scrub_compose_secrets(
                        cf.raw_content.as_deref().unwrap_or(""),
                    )
                } else {
                    cf.raw_content.clone().unwrap_or_default()
                };
                std::fs::write(&dest, content)
                    .map_err(|e| RefineError::TarballError(e.to_string()))?;
            }
        }
    }
```

- [ ] **Step 3: Access redaction state**

The `snap.redaction` field provides `RedactionState`. Check the existing
import path — `inspectah_core::types::redaction::RedactionState` should
already be available. If not, add the import.

The check pattern:
```rust
use inspectah_core::types::redaction::RedactionState;

let is_redacted = snap
    .redaction
    .as_ref()
    .map(|r| matches!(r.state, RedactionState::Redacted))
    .unwrap_or(false);
```

- [ ] **Step 4: Update export contract test**

In `crates/refine/tests/export_contract_test.rs`, add a test that
verifies compose files appear under `compose/` in the tarball:

```rust
#[test]
fn export_includes_compose_files() {
    use inspectah_core::types::containers::{ComposeFile, ContainerSection};

    let snap = InspectionSnapshot {
        schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
        containers: Some(ContainerSection {
            compose_files: vec![ComposeFile {
                path: "opt/myapp/docker-compose.yml".to_string(),
                raw_content: Some("services:\n  web:\n    image: nginx\n".to_string()),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    };

    let dir = tempfile::tempdir().unwrap();
    let tarball_path = dir.path().join("export.tar.gz");
    render_refine_export(&snap, &tarball_path, None, None).unwrap();

    let f = std::fs::File::open(&tarball_path).unwrap();
    let gz = flate2::read::GzDecoder::new(f);
    let mut ar = tar::Archive::new(gz);
    let entries: Vec<String> = ar
        .entries()
        .unwrap()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.path().ok().map(|p| p.to_string_lossy().to_string()))
        .collect();

    assert!(
        entries.iter().any(|e| e.contains("compose/opt/myapp/docker-compose.yml")),
        "compose file not found in tarball: {entries:?}"
    );
}
```

- [ ] **Step 5: Add redaction test for compose export**

```rust
#[test]
fn export_redacts_compose_secrets() {
    use inspectah_core::types::containers::{ComposeFile, ContainerSection};
    use inspectah_core::types::redaction::{RedactionInfo, RedactionState};

    let yaml = "services:\n  db:\n    environment:\n      DB_PASSWORD=hunter2\n      PORT=5432\n";
    let snap = InspectionSnapshot {
        schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
        containers: Some(ContainerSection {
            compose_files: vec![ComposeFile {
                path: "opt/db/docker-compose.yml".to_string(),
                raw_content: Some(yaml.to_string()),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        }),
        redaction: Some(RedactionInfo {
            state: RedactionState::Redacted,
            ..Default::default()
        }),
        ..Default::default()
    };

    let dir = tempfile::tempdir().unwrap();
    let tarball_path = dir.path().join("export.tar.gz");
    render_refine_export(&snap, &tarball_path, None, None).unwrap();

    // Read back the compose file content from the tarball.
    let f = std::fs::File::open(&tarball_path).unwrap();
    let gz = flate2::read::GzDecoder::new(f);
    let mut ar = tar::Archive::new(gz);
    let mut compose_content = String::new();
    for entry in ar.entries().unwrap() {
        let mut entry = entry.unwrap();
        let path = entry.path().unwrap().to_string_lossy().to_string();
        if path.contains("compose/opt/db/docker-compose.yml") {
            use std::io::Read;
            entry.read_to_string(&mut compose_content).unwrap();
            break;
        }
    }

    assert!(compose_content.contains("<REDACTED>"), "secrets not redacted");
    assert!(!compose_content.contains("hunter2"), "secret value leaked");
    assert!(compose_content.contains("PORT=5432"), "non-secret was redacted");
}
```

- [ ] **Step 6: Verify tests pass, commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
cargo test -p inspectah-refine -- export
git add crates/refine/src/session.rs crates/refine/tests/export_contract_test.rs
git commit -m "feat(refine): export compose files to tarball with redaction

Writes compose files under compose/ in the export tarball, mirroring
the source directory structure. Secret-like env var values are scrubbed
via inspectah_core::redaction::scrub_compose_secrets when the snapshot
is in redacted state. Adds compose to the export allowlist.

Assisted-by: Claude Code (Opus 4.6)"
```

**Thorn checkpoint: after T1-T3 complete (covers data model + collector + export).**

---

## Task 4: Containerfile — Compose Comment Block

**Files:**
- Modify: `crates/pipeline/src/render/containerfile.rs`
- Test: new unit test in same file

**Interfaces:**
- Depends on: Tasks 1-3 (compose data available, compose/ in tarball)
- Produces: compose comment block in Containerfile output

- [ ] **Step 1: Add `compose_comment_lines` function**

In `crates/pipeline/src/render/containerfile.rs`, add a new function
after `containers_section_lines`:

```rust
/// Generates a Containerfile comment block for detected compose stacks.
///
/// No COPY or RUN directives — compose is reference-only. The comment
/// lists detected stacks and nudges toward Quadlet migration.
fn compose_comment_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let mut lines = Vec::new();
    let containers = match &snap.containers {
        Some(c) => c,
        None => return lines,
    };

    let compose_files: Vec<_> = containers
        .compose_files
        .iter()
        .filter(|c| c.include)
        .collect();

    if compose_files.is_empty() {
        return lines;
    }

    let mut body: Vec<String> = Vec::new();
    body.push("# === Compose stacks detected ===".into());
    body.push(
        "# The following compose stacks were running on the source host.".into(),
    );
    body.push("# These are application workloads, not OS configuration.".into());
    body.push("# See compose/ in the build context for the original files.".into());
    body.push("#".into());
    body.push(
        "# Consider converting to Quadlet units — .container files under".into(),
    );
    body.push(
        "# /etc/containers/systemd/ that let systemd manage your container".into(),
    );
    body.push("# workloads natively.".into());
    body.push("#   man quadlet(5)".into());
    body.push(
        "#   https://docs.podman.io/en/latest/markdown/podman-systemd.unit.5.html".into(),
    );
    body.push(
        "#   https://docs.redhat.com/en/documentation/red_hat_enterprise_linux/10/html/building_running_and_managing_containers/porting-containers-to-systemd-using-podman".into(),
    );
    body.push("#".into());

    for cf in &compose_files {
        let svc_count = cf.images.len();
        let svc_label = if svc_count == 1 {
            "1 service".to_string()
        } else {
            format!("{svc_count} services")
        };
        body.push(format!("#   - /{} ({svc_label})", cf.path));
    }

    lines.extend(section("Compose Stacks (reference only)", body));
    lines
}
```

- [ ] **Step 2: Wire `compose_comment_lines` into the main render function**

In `render_containerfile()` / `render_containerfile_inner()`, add the
compose comment block call. Place it after the containers section
(step 7) and before users (step 8). The exact insertion point is after
line ~203 (`lines.extend(containers_section_lines(snap));`):

```rust
    // 7b. Compose stacks (reference-only comment block)
    lines.extend(compose_comment_lines(snap));
```

- [ ] **Step 3: Add unit tests**

```rust
    #[test]
    fn compose_comment_block_emitted() {
        use inspectah_core::types::containers::{
            ComposeFile, ComposeService, ContainerSection,
        };

        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            containers: Some(ContainerSection {
                compose_files: vec![
                    ComposeFile {
                        path: "opt/myapp/docker-compose.yml".to_string(),
                        images: vec![
                            ComposeService {
                                service: "web".to_string(),
                                image: "nginx:latest".to_string(),
                            },
                            ComposeService {
                                service: "db".to_string(),
                                image: "postgres:16".to_string(),
                            },
                        ],
                        include: true,
                        ..Default::default()
                    },
                    ComposeFile {
                        path: "srv/monitoring/compose.yml".to_string(),
                        images: vec![ComposeService {
                            service: "prometheus".to_string(),
                            image: "prom/prometheus".to_string(),
                        }],
                        include: true,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            ..Default::default()
        };

        let output = render_containerfile(&snap, None, None);
        assert!(output.contains("Compose stacks detected"));
        assert!(output.contains("quadlet(5)"));
        assert!(output.contains("podman-systemd.unit.5.html"));
        assert!(output.contains("porting-containers-to-systemd-using-podman"));
        assert!(output.contains("opt/myapp/docker-compose.yml (2 services)"));
        assert!(output.contains("srv/monitoring/compose.yml (1 service)"));
        // No COPY or RUN for compose
        assert!(!output.contains("COPY compose/"));
        assert!(!output.contains("RUN compose"));
    }

    #[test]
    fn compose_comment_block_absent_when_no_compose() {
        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            containers: Some(ContainerSection::default()),
            ..Default::default()
        };

        let output = render_containerfile(&snap, None, None);
        assert!(!output.contains("Compose stacks detected"));
    }

    #[test]
    fn compose_comment_excludes_non_included() {
        use inspectah_core::types::containers::{ComposeFile, ContainerSection};

        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            containers: Some(ContainerSection {
                compose_files: vec![ComposeFile {
                    path: "opt/excluded/docker-compose.yml".to_string(),
                    include: false,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let output = render_containerfile(&snap, None, None);
        assert!(!output.contains("Compose stacks detected"));
    }
```

- [ ] **Step 4: Verify tests pass, commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
cargo test -p inspectah-pipeline -- containerfile
git add crates/pipeline/src/render/containerfile.rs
git commit -m "feat(pipeline): add compose comment block to Containerfile

Reference-only comment listing detected compose stacks with service
counts and a Quadlet migration nudge. Links to man quadlet(5), Podman
docs, and RHEL 10 container guide. No COPY/RUN directives.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 4a: Compose Docs + Sensitivity Handoff

**Files:**
- Modify: `docs/reference/output-artifacts.md`

**Interfaces:**
- Depends on: Task 3 (compose/ export exists)
- Produces: documented `compose/` root in output-artifacts reference

This task closes two compose-side gaps flagged in review:

1. **Output-artifacts documentation:** Add the `compose/` root to the
   documented export contract in `docs/reference/output-artifacts.md`.

2. **Compose sensitivity indicator handoff:** The spec requires the
   refine UI to show a sensitivity indicator on compose entries when
   secret-like patterns were detected (spec: "The refine UI shows a
   sensitivity indicator on compose entries when secret-like patterns
   were detected"). This is a **Plan 3 UI concern** — the compose sidebar
   destination and its visual treatment are Plan 3 scope. **Named handoff
   to Plan 3:** Plan 3 Task 8 (Sidebar Updates) should add a sensitivity
   badge to the compose sidebar entry when `RedactionHint` entries exist
   for compose files. If Plan 3 is already approved without this, file a
   follow-up issue for a patch plan.

- [ ] **Step 1: Add compose/ to output-artifacts.md**

In `docs/reference/output-artifacts.md`, add a row to the export roots
table:

```markdown
| `compose/` | Compose YAML files | Automatic when compose files detected | Raw YAML mirroring source directory structure. Subject to redaction: secret-like env var values replaced with `<REDACTED>` when snapshot is in redacted state. |
```

- [ ] **Step 2: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add docs/reference/output-artifacts.md
git commit -m "docs(reference): add compose/ root to output-artifacts

Documents the compose/ export root added in Task 3. Subject to
redaction rules when snapshot is in redacted state.

Assisted-by: Claude Code (Opus 4.6)"
```

**Thorn checkpoint: after T4a complete (full compose pipeline end-to-end + docs).**

---

## Task 5: Aggregate Merge — NonRpmItem Identity Key Fix

**Files:**
- Modify: `crates/core/src/aggregate/merge.rs`
- Test: new unit test in same file

**Interfaces:**
- Depends on: Plan 1 Task 1 (`NonRpmItem` field extensions)
- Produces: composite identity key for language env aggregate merge
- Consumed by: Task 7

The current `NonRpmItem::identity_key()` returns `&self.name`, which
collides when the same package name appears in different environments
(e.g., `requests` in two different venvs). The spec requires
ecosystem + environment path as the identity key.

- [ ] **Step 1: Change NonRpmItem identity_key**

In `crates/core/src/aggregate/merge.rs`, change the `AggregateMergeable`
impl for `NonRpmItem` (line ~340):

From:
```rust
impl AggregateMergeable for NonRpmItem {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.name)
    }
    // ...
}
```

To:
```rust
impl AggregateMergeable for NonRpmItem {
    fn identity_key(&self) -> Cow<'_, str> {
        // Composite key: method determines the ecosystem, path provides
        // the environment scope. Falls back to name for legacy items
        // that lack a path (binary detection, etc.).
        if self.path.is_empty() {
            Cow::Borrowed(&self.name)
        } else {
            let ecosystem = match self.method.as_str() {
                "pip list" | "pip dist-info" | "venv" => "pip",
                "npm lockfile" => "npm",
                "gem lockfile" => "gem",
                _ => "other",
            };
            Cow::Owned(format!("{}:{}", ecosystem, self.path))
        }
    }
    // ... rest unchanged
}
```

- [ ] **Step 2: Add content_variant_key for package-list diffing**

Add `content_variant_key` to the `AggregateMergeable` impl to support
variant detection when different hosts have different package lists
in the same environment:

```rust
    fn content_variant_key(&self) -> Option<Cow<'_, str>> {
        use sha2::{Digest, Sha256};
        // Hash the unified package list to detect divergence across hosts.
        // All ecosystems use packages: Vec<LanguagePackage> (Plan 1 Task 1
        // renames PipPackage -> LanguagePackage and reuses the same field).
        let mut hasher = Sha256::new();
        hasher.update(self.method.as_bytes());
        hasher.update(b"\n");
        for pkg in &self.packages {
            hasher.update(format!("{}={}\n", pkg.name, pkg.version).as_bytes());
        }
        Some(Cow::Owned(format!("{:x}", hasher.finalize())))
    }
```

- [ ] **Step 3: Add unit tests for composite identity key and prevalence defaults**

```rust
    #[test]
    fn nonrpm_aggregate_identity_key_composite() {
        let item = NonRpmItem {
            name: "requests".to_string(),
            path: "/opt/myapp/venv".to_string(),
            method: "venv".to_string(),
            ..Default::default()
        };
        assert_eq!(item.identity_key().as_ref(), "pip:/opt/myapp/venv");
    }

    #[test]
    fn nonrpm_aggregate_identity_key_npm() {
        let item = NonRpmItem {
            name: "express".to_string(),
            path: "/srv/webapp".to_string(),
            method: "npm lockfile".to_string(),
            ..Default::default()
        };
        assert_eq!(item.identity_key().as_ref(), "npm:/srv/webapp");
    }

    #[test]
    fn nonrpm_aggregate_identity_key_fallback() {
        let item = NonRpmItem {
            name: "custom-binary".to_string(),
            path: String::new(),
            method: "binary".to_string(),
            ..Default::default()
        };
        assert_eq!(item.identity_key().as_ref(), "custom-binary");
    }

    #[test]
    fn nonrpm_merge_100_pct_prevalence_includes() {
        // Two hosts, same environment on both → 100% prevalence → include: true
        let section_a = Some(NonRpmSoftwareSection {
            items: vec![NonRpmItem {
                name: "myapp-venv".to_string(),
                path: "/opt/myapp/venv".to_string(),
                method: "venv".to_string(),
                include: true,
                ..Default::default()
            }],
            env_files: vec![],
        });
        let section_b = Some(NonRpmSoftwareSection {
            items: vec![NonRpmItem {
                name: "myapp-venv".to_string(),
                path: "/opt/myapp/venv".to_string(),
                method: "venv".to_string(),
                include: true,
                ..Default::default()
            }],
            env_files: vec![],
        });

        let merged = merge_nonrpm_sections(
            vec![section_a, section_b],
            2,
            &["host-a".into(), "host-b".into()],
        )
        .unwrap();

        let item = &merged.items[0];
        let agg = item.aggregate.as_ref().unwrap();
        assert_eq!(agg.count, 2);
        assert_eq!(agg.total, 2);
        assert!(item.include, "100% prevalence should default to include: true");
    }

    #[test]
    fn nonrpm_merge_partial_prevalence_excludes() {
        // Two hosts, environment on only one → 50% prevalence → include: false
        let section_a = Some(NonRpmSoftwareSection {
            items: vec![NonRpmItem {
                name: "myapp-venv".to_string(),
                path: "/opt/myapp/venv".to_string(),
                method: "venv".to_string(),
                include: true,
                ..Default::default()
            }],
            env_files: vec![],
        });
        let section_b = Some(NonRpmSoftwareSection {
            items: vec![],
            env_files: vec![],
        });

        let merged = merge_nonrpm_sections(
            vec![section_a, section_b],
            2,
            &["host-a".into(), "host-b".into()],
        )
        .unwrap();

        let item = &merged.items[0];
        let agg = item.aggregate.as_ref().unwrap();
        assert_eq!(agg.count, 1);
        assert_eq!(agg.total, 2);
        assert!(!item.include, "partial prevalence should default to include: false");
    }
```

- [ ] **Step 4: Verify tests pass, commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
cargo test -p inspectah-core -- merge
git add crates/core/src/aggregate/merge.rs
git commit -m "fix(core): use composite identity key for NonRpmItem aggregate merge

Changes NonRpmItem identity from name-only to ecosystem:path format
(e.g., pip:/opt/myapp/venv) to prevent collisions when the same
package appears in multiple environments. Falls back to name for
legacy items without a path. Adds prevalence-default tests proving
100% = include, <100% = exclude.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 5a: Data Model — Aggregate-Only Fields for UnmanagedFile

**Files:**
- Modify: `crates/core/src/types/nonrpm.rs`
- Test: serde roundtrip tests

**Interfaces:**
- Depends on: Plan 2 Task 1 (`UnmanagedFile` struct exists)
- Produces: `content_hash` and `variant_selection` fields on `UnmanagedFile`
- Consumed by: Tasks 6, 8, 9a, 11a

Plan 2 defines `UnmanagedFile` without `content_hash` or
`variant_selection` because variant selection is a merge-layer output.
`content_hash` is a SHA-256 of the actual file content, computed by the
collector during `--include-unmanaged` scan (the file is read for
bundling anyway, so hashing is negligible). In single-host mode it
remains empty. This task adds both fields with `serde(default)` so
existing single-host snapshots deserialize without breakage.

**Note:** The `content_hash` is populated by Plan 2's unmanaged file
collector (the scan reads file content for tarball bundling — hash it
at the same time). If Plan 2 does not yet compute it, add a step to
Plan 2's cataloging task that hashes file content during bundling.

- [ ] **Step 1: Add `content_hash` field to `UnmanagedFile`**

In `crates/core/src/types/nonrpm.rs`, add after the `aggregate` field
on `UnmanagedFile`:

```rust
    /// SHA-256 hash of the actual file content, for aggregate variant
    /// detection. Empty in single-host mode — populated by the collector
    /// during scan (the file content is read for bundling anyway when
    /// --include-unmanaged is used, so hashing adds negligible cost).
    /// Same file content across hosts produces the same hash; different
    /// content at the same path produces different hashes.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content_hash: String,
```

- [ ] **Step 2: Add `variant_selection` field to `UnmanagedFile`**

```rust
    /// Variant selection state for aggregate mode.
    /// Only meaningful when multiple hosts have the same file path but
    /// different content hashes.
    #[serde(default)]
    pub variant_selection: VariantSelection,
```

Import `VariantSelection` from `crate::types::aggregate::VariantSelection`
(or wherever it is defined — check the existing `VariantSelection` usage
in `ComposeFile`, `ConfigFileEntry`, etc. for the canonical import path).

- [ ] **Step 3: Add serde roundtrip tests**

```rust
    #[test]
    fn unmanaged_file_aggregate_fields_roundtrip() {
        use crate::types::aggregate::VariantSelection;

        let uf = UnmanagedFile {
            path: "/opt/splunk/bin/splunkd".to_string(),
            size: 52_000_000,
            content_hash: "abc123def456".to_string(),
            variant_selection: VariantSelection::Selected,
            ..Default::default()
        };
        let json = serde_json::to_string(&uf).unwrap();
        assert!(json.contains("content_hash"));
        assert!(json.contains("variant_selection"));
        let parsed: UnmanagedFile = serde_json::from_str(&json).unwrap();
        assert_eq!(uf.content_hash, parsed.content_hash);
        assert_eq!(uf.variant_selection, parsed.variant_selection);
    }

    #[test]
    fn unmanaged_file_aggregate_fields_default_from_plan2_json() {
        // Simulates deserializing a Plan 2 snapshot that lacks
        // content_hash and variant_selection.
        let json = r#"{"path":"/opt/app/bin","size":1000,"file_type":"elf_binary","include":true}"#;
        let parsed: UnmanagedFile = serde_json::from_str(json).unwrap();
        assert!(parsed.content_hash.is_empty());
        assert_eq!(parsed.variant_selection, VariantSelection::default());
    }

    #[test]
    fn unmanaged_file_empty_content_hash_omitted_in_json() {
        let uf = UnmanagedFile {
            path: "/opt/app/bin".to_string(),
            content_hash: String::new(),
            ..Default::default()
        };
        let json = serde_json::to_string(&uf).unwrap();
        assert!(!json.contains("content_hash"), "empty content_hash should be omitted");
    }

    #[test]
    fn same_content_produces_same_hash() {
        // content_hash is a SHA-256 of the actual file bytes.
        // Two hosts with identical file content at the same path
        // must produce the same variant key.
        use sha2::{Digest, Sha256};
        let content = b"#!/bin/bash\necho hello\n";
        let hash1 = format!("{:x}", Sha256::digest(content));
        let hash2 = format!("{:x}", Sha256::digest(content));
        assert_eq!(hash1, hash2, "identical content must produce identical hash");
    }

    #[test]
    fn different_content_produces_different_hash() {
        use sha2::{Digest, Sha256};
        let hash1 = format!("{:x}", Sha256::digest(b"version 1.0"));
        let hash2 = format!("{:x}", Sha256::digest(b"version 2.0"));
        assert_ne!(hash1, hash2, "different content must produce different hash");
    }
```

- [ ] **Step 4: Verify tests pass, commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
cargo test -p inspectah-core -- nonrpm
git add crates/core/src/types/nonrpm.rs
git commit -m "feat(core): add aggregate-only fields to UnmanagedFile

Adds content_hash and variant_selection to UnmanagedFile for
aggregate variant detection. Both use serde(default) for backward
compatibility with Plan 2 single-host snapshots. The merge layer
populates content_hash; variant_selection is a merge output.

Assisted-by: Claude Code (Opus 4.6)"
```

**Thorn checkpoint: after T5a complete (aggregate data model for both section types is finalized).**

---

## Task 6: Aggregate Merge — UnmanagedFile Support

**Files:**
- Modify: `crates/core/src/aggregate/merge.rs`
- Test: new unit test in same file

**Interfaces:**
- Depends on: Plan 2 Task 1 (`UnmanagedFile`, `UnmanagedFileSection` types),
  Task 5a (`content_hash`, `variant_selection` fields)
- Produces: `AggregateMergeable` impl for `UnmanagedFile`, merge function
- Consumed by: Task 8

- [ ] **Step 1: Implement `AggregateMergeable` for `UnmanagedFile`**

In `crates/core/src/aggregate/merge.rs`, add after the `NonRpmItem` impl:

```rust
use crate::types::nonrpm::{UnmanagedFile, UnmanagedFileSection};

impl AggregateMergeable for UnmanagedFile {
    fn identity_key(&self) -> Cow<'_, str> {
        // File path is the stable identity for unmanaged files.
        Cow::Borrowed(&self.path)
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.include = val;
    }

    fn content_variant_key(&self) -> Option<Cow<'_, str>> {
        // Use the content hash to detect divergent file content across hosts.
        // content_hash is added by Task 5a — empty in single-host mode.
        if self.content_hash.is_empty() {
            None
        } else {
            Some(Cow::Borrowed(&self.content_hash))
        }
    }

    fn variant_selection_mut(&mut self) -> Option<&mut VariantSelection> {
        Some(&mut self.variant_selection)
    }
}
```

- [ ] **Step 2: Add `merge_unmanaged_file_sections` function**

```rust
/// Merge unmanaged file sections from multiple hosts.
///
/// Uses Plan 2's UnmanagedFileSection contract: `items` (not `files`),
/// with `total_size` and `total_count` recomputed from merged items.
pub fn merge_unmanaged_file_sections(
    sections: Vec<Option<UnmanagedFileSection>>,
    total_hosts: usize,
    hostnames: &[String],
) -> Option<UnmanagedFileSection> {
    if sections.iter().all(|s| s.is_none()) {
        return None;
    }

    let items = merge_items(
        collect_items(&sections, |s| &s.items),
        total_hosts,
        hostnames,
    );

    // Recompute totals from merged items.
    let total_size: u64 = items.iter().map(|f| f.size).sum();
    let total_count = items.len();

    Some(UnmanagedFileSection {
        items,
        total_size,
        total_count,
    })
}
```

- [ ] **Step 3: Wire into the top-level merge function**

Find the function that calls `merge_nonrpm_sections` and
`merge_container_sections` (the top-level snapshot merge). Add
`merge_unmanaged_file_sections` in the same pattern, assigning the
result to `merged_snap.unmanaged_files`.

- [ ] **Step 4: Add unit tests including prevalence defaults**

```rust
    #[test]
    fn unmanaged_file_merge_by_path() {
        let section_a = Some(UnmanagedFileSection {
            items: vec![UnmanagedFile {
                path: "/opt/splunk/bin/splunkd".to_string(),
                size: 52_000_000,
                include: true,
                content_hash: "aaa111".to_string(),
                ..Default::default()
            }],
            total_size: 52_000_000,
            total_count: 1,
        });
        let section_b = Some(UnmanagedFileSection {
            items: vec![UnmanagedFile {
                path: "/opt/splunk/bin/splunkd".to_string(),
                size: 52_000_000,
                include: true,
                content_hash: "bbb222".to_string(),
                ..Default::default()
            }],
            total_size: 52_000_000,
            total_count: 1,
        });

        let merged = merge_unmanaged_file_sections(
            vec![section_a, section_b],
            2,
            &["host-a".into(), "host-b".into()],
        )
        .unwrap();

        // Same path -> merged into one item with aggregate prevalence.
        assert_eq!(merged.items.len(), 1);
        let file = &merged.items[0];
        assert_eq!(file.path, "/opt/splunk/bin/splunkd");
        let agg = file.aggregate.as_ref().unwrap();
        assert_eq!(agg.total, 2);
        // Totals recomputed
        assert_eq!(merged.total_count, 1);
        assert_eq!(merged.total_size, 52_000_000);
    }

    #[test]
    fn unmanaged_file_merge_100_pct_prevalence_includes() {
        // File present on all hosts → 100% → include: true
        let section_a = Some(UnmanagedFileSection {
            items: vec![UnmanagedFile {
                path: "/opt/app/server".to_string(),
                size: 10_000,
                include: true,
                ..Default::default()
            }],
            total_size: 10_000,
            total_count: 1,
        });
        let section_b = Some(UnmanagedFileSection {
            items: vec![UnmanagedFile {
                path: "/opt/app/server".to_string(),
                size: 10_000,
                include: true,
                ..Default::default()
            }],
            total_size: 10_000,
            total_count: 1,
        });

        let merged = merge_unmanaged_file_sections(
            vec![section_a, section_b],
            2,
            &["host-a".into(), "host-b".into()],
        )
        .unwrap();

        let file = &merged.items[0];
        let agg = file.aggregate.as_ref().unwrap();
        assert_eq!(agg.count, 2);
        assert_eq!(agg.total, 2);
        assert!(file.include, "100% prevalence should default to include: true");
    }

    #[test]
    fn unmanaged_file_merge_partial_prevalence_excludes() {
        // File on 1 of 2 hosts → 50% → include: false
        let section_a = Some(UnmanagedFileSection {
            items: vec![UnmanagedFile {
                path: "/opt/app/server".to_string(),
                size: 10_000,
                include: true,
                ..Default::default()
            }],
            total_size: 10_000,
            total_count: 1,
        });
        let section_b = Some(UnmanagedFileSection {
            items: vec![],
            total_size: 0,
            total_count: 0,
        });

        let merged = merge_unmanaged_file_sections(
            vec![section_a, section_b],
            2,
            &["host-a".into(), "host-b".into()],
        )
        .unwrap();

        let file = &merged.items[0];
        let agg = file.aggregate.as_ref().unwrap();
        assert_eq!(agg.count, 1);
        assert_eq!(agg.total, 2);
        assert!(!file.include, "partial prevalence should default to include: false");
    }
```

- [ ] **Step 5: Verify tests pass, commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
cargo test -p inspectah-core -- merge
git add crates/core/src/aggregate/merge.rs
git commit -m "feat(core): add UnmanagedFile aggregate merge support

Implements AggregateMergeable for UnmanagedFile using file path as
identity key and content hash for variant detection. Uses Plan 2's
exact contract: UnmanagedFileSection.items with total_size and
total_count recomputed from merged results. Prevalence tests verify
100% = include, <100% = exclude.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 7: Aggregate Handler — Language Packages Section

**Files:**
- Modify: `crates/web/src/aggregate_handlers.rs`
- Test: new test in same file

**Interfaces:**
- Depends on: Task 5 (NonRpmItem composite identity key), Plan 1 Task 1
  (`NonRpmItem` field extensions), Plan 3 Task 2 (UI types)
- Produces: `language_packages` section in aggregate view response
- Consumed by: Task 9 (TS types), Task 9a (API/DTO contract)

- [ ] **Step 1: Add language package classification helper**

In `crates/web/src/aggregate_handlers.rs`, add a helper to classify
NonRpmItem entries as language environments:

```rust
/// Classifies non-RPM items as language package environments for the
/// aggregate view. Groups by ecosystem + path (matching the aggregate
/// merge identity key).
fn classify_language_envs(
    snap: &InspectionSnapshot,
) -> Vec<(&NonRpmItem, ItemId)> {
    let nrs = match &snap.non_rpm_software {
        Some(n) => n,
        None => return Vec::new(),
    };

    nrs.items
        .iter()
        .filter(|item| {
            matches!(
                item.method.as_str(),
                "pip list" | "pip dist-info" | "venv" | "npm lockfile" | "gem lockfile"
            )
        })
        .map(|item| {
            let ecosystem = match item.method.as_str() {
                "pip list" | "pip dist-info" | "venv" => "pip",
                "npm lockfile" => "npm",
                "gem lockfile" => "gem",
                _ => "other",
            };
            let item_id = ItemId::LanguageEnv {
                ecosystem: ecosystem.to_string(),
                path: item.path.clone(),
            };
            (item, item_id)
        })
        .collect()
}
```

- [ ] **Step 2: Build `language_packages` section in `build_aggregate_sections`**

After the containers section block in `build_aggregate_sections`, add:

```rust
    // Language Packages — decision items with aggregate prevalence
    {
        let lang_envs = classify_language_envs(snap);
        if !lang_envs.is_empty() {
            let mut items: Vec<AggregateItem> = Vec::new();
            for (item, item_id) in &lang_envs {
                let fp = item.aggregate.as_ref();

                items.push(AggregateItem {
                    item_id: item_id.clone(),
                    include: item.include,
                    locked: item.locked,
                    attention_reason: None,
                    triage: build_triage_dto(
                        &Triage {
                            bucket: if item.include {
                                TriageBucket::Keep
                            } else {
                                TriageBucket::Exclude
                            },
                            reason: TriageReason::UserDecision,
                            tags: vec![],
                            zone: fp.map(|a| {
                                if a.count == a.total {
                                    PrevalenceZone::Consensus
                                } else if a.count as f64 / a.total.max(1) as f64 >= 0.5 {
                                    PrevalenceZone::NearConsensus
                                } else {
                                    PrevalenceZone::Divergent
                                }
                            }),
                        },
                        fp,
                        ctx,
                    ),
                    prevalence: aggregate_prevalence_dto(fp, ctx),
                    variants: None,
                    variant_payload: None, // Populated by T11a for divergent items
                    source_repo: String::new(),
                    repo_conflict: None,
                });
            }

            if !items.is_empty() {
                sections.push(build_section(
                    "language_packages",
                    "Language Packages",
                    true,
                    &items,
                    ctx,
                ));
            }
        }
    }
```

**Note on variant payloads:** This task produces the section structure
and zone-based layout. The variant payloads (package-list diffs) are
added by Task 9a (API/DTO contract) and Task 11a (variant diff data),
which extend the response shape. Setting `variants: None` here is
intentional — it avoids coupling the section builder to variant data
that has not been defined yet.

- [ ] **Step 3: Add necessary imports**

Ensure the following are imported at the top of `aggregate_handlers.rs`:

```rust
use inspectah_core::types::nonrpm::NonRpmItem;
```

If `ItemId::LanguageEnv` is not yet defined (depends on Plan 1 Task 1),
this task is blocked until Plan 1 lands.

- [ ] **Step 4: Add unit test with prevalence-default verification**

```rust
    #[test]
    fn aggregate_language_packages_section_emitted() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use inspectah_core::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection};

        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            non_rpm_software: Some(NonRpmSoftwareSection {
                items: vec![NonRpmItem {
                    name: "myapp-venv".to_string(),
                    path: "/opt/myapp/venv".to_string(),
                    method: "venv".to_string(),
                    confidence: "high".to_string(),
                    include: true,
                    aggregate: Some(AggregatePrevalence {
                        count: 3,
                        total: 3,
                        hosts: vec!["a".into(), "b".into(), "c".into()],
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                env_files: vec![],
            }),
            ..Default::default()
        };

        let ctx = AggregateContext {
            aggregate_meta: AggregateSnapshotMeta {
                label: "test".to_string(),
                host_count: 3,
                hostnames: vec!["a".into(), "b".into(), "c".into()],
                merged_at: "2026-01-01T00:00:00Z".to_string(),
                baseline_provisional: false,
                section_host_counts: BTreeMap::new(),
            },
            zones_active: true,
            total_hosts: 3,
            repo_conflicts: BTreeMap::new(),
        };

        let session = RefineSession::new(snap.clone());
        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let lang_section = sections
            .iter()
            .find(|s| s.id == "language_packages");

        assert!(
            lang_section.is_some(),
            "language_packages section should be present"
        );
        let section = lang_section.unwrap();
        assert!(section.is_decision_section);
    }

    #[test]
    fn aggregate_language_packages_100_pct_includes() {
        // 3/3 hosts → consensus zone, include: true
        let snap = make_lang_pkg_snap(3, 3); // helper
        let ctx = make_aggregate_ctx(3);
        let session = RefineSession::new(snap.clone());
        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let section = sections.iter().find(|s| s.id == "language_packages").unwrap();
        let items = all_section_items(section);
        assert!(items[0].include, "100% prevalence should be included");
    }

    #[test]
    fn aggregate_language_packages_partial_excludes() {
        // 1/3 hosts → divergent zone, include: false
        let snap = make_lang_pkg_snap(1, 3); // helper
        let ctx = make_aggregate_ctx(3);
        let session = RefineSession::new(snap.clone());
        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let section = sections.iter().find(|s| s.id == "language_packages").unwrap();
        let items = all_section_items(section);
        assert!(!items[0].include, "partial prevalence should be excluded");
    }
```

**Note:** The `make_lang_pkg_snap` and `make_aggregate_ctx` helpers build
test fixtures. If similar helpers already exist in the test module, reuse
them. If not, add minimal constructors that take `count` and `total`.

- [ ] **Step 5: Verify tests pass, commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
cargo test -p inspectah-web -- aggregate
git add crates/web/src/aggregate_handlers.rs
git commit -m "feat(web): add language_packages section to aggregate view

Groups non-RPM language environments by ecosystem:path identity key,
applies zone-based layout. Prevalence tests verify 100% = include,
<100% = exclude. Variant payloads deferred to Task 9a.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 8: Aggregate Handler — Unmanaged Files Section

**Files:**
- Modify: `crates/web/src/aggregate_handlers.rs`
- Test: new test in same file

**Interfaces:**
- Depends on: Task 6 (UnmanagedFile merge), Task 5a
  (`content_hash`, `variant_selection` fields), Plan 2 Task 1
  (`UnmanagedFile`, `UnmanagedFileSection`, `ItemId::UnmanagedFile`)
- Produces: `unmanaged_files` section in aggregate view response
- Consumed by: Task 9 (TS types), Task 9a (API/DTO contract)

- [ ] **Step 1: Add unmanaged file section builder**

After the language packages section block in `build_aggregate_sections`,
add:

```rust
    // Unmanaged Files — decision items with aggregate prevalence
    if let Some(ref unmanaged) = snap.unmanaged_files {
        let mut items: Vec<AggregateItem> = Vec::new();
        for f in &unmanaged.items {
            let item_id = ItemId::UnmanagedFile {
                path: f.path.clone(),
            };
            let fp = f.aggregate.as_ref();

            items.push(AggregateItem {
                item_id,
                include: f.include,
                locked: f.locked,
                attention_reason: None,
                triage: build_triage_dto(
                    &Triage {
                        bucket: if f.include {
                            TriageBucket::Keep
                        } else {
                            TriageBucket::Exclude
                        },
                        reason: TriageReason::UserDecision,
                        tags: vec![],
                        zone: fp.map(|a| {
                            if a.count == a.total {
                                PrevalenceZone::Consensus
                            } else if a.count as f64 / a.total.max(1) as f64 >= 0.5 {
                                PrevalenceZone::NearConsensus
                            } else {
                                PrevalenceZone::Divergent
                            }
                        }),
                    },
                    fp,
                    ctx,
                ),
                prevalence: aggregate_prevalence_dto(fp, ctx),
                variants: None,
                // variant_payload populated by Task 11a after merge
                // identifies divergent items. Initially None; T11a sets
                // it to Some(UnmanagedFileVariantPayload) for items
                // with different content_hash across hosts.
                variant_payload: None,
                source_repo: String::new(),
                repo_conflict: None,
            });
        }

        if !items.is_empty() {
            sections.push(build_section(
                "unmanaged_files",
                "Unmanaged Files",
                true,
                &items,
                ctx,
            ));
        }
    }
```

**Note:** This uses `unmanaged.items` (not `.files`) matching Plan 2's
contract. Variant payloads (content-hash metadata comparison) are added
by Task 9a, not here.

- [ ] **Step 2: Add necessary imports**

```rust
use inspectah_core::types::nonrpm::{UnmanagedFile, UnmanagedFileSection};
```

If `snap.unmanaged_files` field or `UnmanagedFileSection` doesn't exist
yet (depends on Plan 2 Task 1), this task is blocked.

- [ ] **Step 3: Add unit test with prevalence-default verification**

```rust
    #[test]
    fn aggregate_unmanaged_files_section_emitted() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use inspectah_core::types::nonrpm::{UnmanagedFile, UnmanagedFileSection};

        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            unmanaged_files: Some(UnmanagedFileSection {
                items: vec![UnmanagedFile {
                    path: "/opt/splunk/bin/splunkd".to_string(),
                    size: 52_000_000,
                    include: true,
                    content_hash: "abc123".to_string(),
                    aggregate: Some(AggregatePrevalence {
                        count: 2,
                        total: 3,
                        hosts: vec!["a".into(), "b".into()],
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                total_size: 52_000_000,
                total_count: 1,
            }),
            ..Default::default()
        };

        let ctx = AggregateContext {
            aggregate_meta: AggregateSnapshotMeta {
                label: "test".to_string(),
                host_count: 3,
                hostnames: vec!["a".into(), "b".into(), "c".into()],
                merged_at: "2026-01-01T00:00:00Z".to_string(),
                baseline_provisional: false,
                section_host_counts: BTreeMap::new(),
            },
            zones_active: true,
            total_hosts: 3,
            repo_conflicts: BTreeMap::new(),
        };

        let session = RefineSession::new(snap.clone());
        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let unmanaged_section = sections
            .iter()
            .find(|s| s.id == "unmanaged_files");

        assert!(
            unmanaged_section.is_some(),
            "unmanaged_files section should be present"
        );
        let section = unmanaged_section.unwrap();
        assert!(section.is_decision_section);
    }

    #[test]
    fn aggregate_unmanaged_files_100_pct_includes() {
        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            unmanaged_files: Some(UnmanagedFileSection {
                items: vec![UnmanagedFile {
                    path: "/opt/app/server".to_string(),
                    size: 10_000,
                    include: true,
                    aggregate: Some(AggregatePrevalence {
                        count: 2,
                        total: 2,
                        hosts: vec!["a".into(), "b".into()],
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                total_size: 10_000,
                total_count: 1,
            }),
            ..Default::default()
        };

        let ctx = make_aggregate_ctx(2);
        let session = RefineSession::new(snap.clone());
        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let section = sections.iter().find(|s| s.id == "unmanaged_files").unwrap();
        let items = all_section_items(section);
        assert!(items[0].include, "100% prevalence should be included");
    }

    #[test]
    fn aggregate_unmanaged_files_partial_excludes() {
        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            unmanaged_files: Some(UnmanagedFileSection {
                items: vec![UnmanagedFile {
                    path: "/opt/app/server".to_string(),
                    size: 10_000,
                    include: false,
                    aggregate: Some(AggregatePrevalence {
                        count: 1,
                        total: 3,
                        hosts: vec!["a".into()],
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                total_size: 10_000,
                total_count: 1,
            }),
            ..Default::default()
        };

        let ctx = make_aggregate_ctx(3);
        let session = RefineSession::new(snap.clone());
        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let section = sections.iter().find(|s| s.id == "unmanaged_files").unwrap();
        let items = all_section_items(section);
        assert!(!items[0].include, "partial prevalence should be excluded");
    }
```

- [ ] **Step 4: Verify tests pass, commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
cargo test -p inspectah-web -- aggregate
git add crates/web/src/aggregate_handlers.rs
git commit -m "feat(web): add unmanaged_files section to aggregate view

Iterates UnmanagedFileSection.items (Plan 2 contract) with zone-based
layout. Prevalence tests verify 100% = include, <100% = exclude.
Variant payloads deferred to Task 9a.

Assisted-by: Claude Code (Opus 4.6)"
```

**Thorn checkpoint: after T8 complete (aggregate sections for both types built).**

---

## Task 9: TypeScript — Aggregate DTO Updates

**Files:**
- Modify: `crates/web/ui/src/api/types.ts`
- Test: type-level only (TypeScript compiler)

**Interfaces:**
- Depends on: Tasks 7-8 (new aggregate sections)
- Produces: TS types for frontend consumption
- Consumed by: Task 9a (extended DTOs), Track C UI components

- [ ] **Step 1: Verify `ItemId` union type includes new variants**

Plan 1 Task 1 should have added `ItemIdLanguageEnv` and Plan 2 Task 1
should have added `ItemIdUnmanagedFile` to the TypeScript `ItemId` union.
Verify they exist:

```typescript
// Should already exist from Plan 1/2:
export interface ItemIdLanguageEnv {
  kind: "LanguageEnv";
  key: { ecosystem: string; path: string };
}

export interface ItemIdUnmanagedFile {
  kind: "UnmanagedFile";
  key: { path: string };
}
```

If missing, add them and include in the `ItemId` union type.

- [ ] **Step 2: Verify TypeScript compiles**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/crates/web/ui
npx tsc --noEmit
```

- [ ] **Step 3: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add crates/web/ui/src/api/types.ts
git commit -m "feat(web): verify aggregate DTO types for language packages and unmanaged files

Confirms TypeScript ItemId union includes LanguageEnv and
UnmanagedFile variants from Plans 1/2.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 9a: API/DTO Contract — Per-Section Metadata + Variant Payloads

**Files:**
- Modify: `crates/web/src/aggregate_handlers.rs`
- Modify: `crates/web/ui/src/api/types.ts`
- Test: Rust unit tests + TypeScript compiler

**Interfaces:**
- Depends on: Tasks 7-8 (sections exist), Task 9 (base TS types)
- Produces: extended aggregate response with section-specific metadata
  and variant payload structures
- Consumed by: Tasks 10-14 (Track C UI)

This task bridges Track B (backend sections) and Track C (UI). It defines
the EXACT JSON response shapes the UI will consume, ensuring Track C tasks
have declared backend data sources instead of implied future data.

- [ ] **Step 1: Define per-item metadata DTOs (Rust)**

In `crates/web/src/aggregate_handlers.rs`, add metadata structures:

```rust
/// Per-item metadata for language package aggregate rows.
/// Carried in AggregateItem.section_metadata as a serde_json::Value.
#[derive(Serialize)]
pub struct LanguagePackageMetadata {
    /// Ecosystem identifier (pip, npm, gem)
    pub ecosystem: String,
    /// Confidence level (high, medium, low)
    pub confidence: String,
    /// Number of packages in this environment
    pub package_count: usize,
    /// Manifest file basis (e.g., "requirements.txt", "package-lock.json")
    pub manifest_basis: Option<String>,
    /// Full package list for detail pane rendering
    pub packages: Vec<LanguagePackageDto>,
}

#[derive(Serialize)]
pub struct LanguagePackageDto {
    pub name: String,
    pub version: String,
}

/// Per-item metadata for unmanaged file aggregate rows.
#[derive(Serialize)]
pub struct UnmanagedFileMetadata {
    /// Detected file type (elf_binary, jar, script, etc.)
    pub file_type: String,
    /// File size in bytes
    pub size: u64,
    /// True if path is under /var (persistence warning)
    pub under_var: bool,
    /// Provenance detail for the detail pane
    pub provenance: UnmanagedFileProvenanceDto,
}

#[derive(Serialize)]
pub struct UnmanagedFileProvenanceDto {
    pub last_modified: u64,
    pub uid: u32,
    pub gid: u32,
    pub permissions: String,
    pub writable_mount: bool,
    pub mutability: bool,
    pub service_working_dir: bool,
}
```

- [ ] **Step 2: Add `section_metadata` field to `AggregateItem`**

Extend `AggregateItem` with an optional metadata field:

```rust
#[derive(Serialize, Clone)]
pub struct AggregateItem {
    // ... existing fields ...

    /// Section-specific per-item metadata, serialized as JSON.
    /// Language packages: LanguagePackageMetadata
    /// Unmanaged files: UnmanagedFileMetadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section_metadata: Option<serde_json::Value>,

    /// Section-specific variant payload, serialized as JSON.
    /// Only populated when the item has variants (multiple hosts
    /// with different content at the same identity key).
    /// Language packages: LanguagePackageVariantPayload
    /// Unmanaged files: UnmanagedFileVariantPayload
    /// Track C (T12) reads this field to render variant diff views.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant_payload: Option<serde_json::Value>,
}
```

- [ ] **Step 3: Populate metadata in T7/T8 section builders**

Update the language packages section builder (T7) to populate
`section_metadata`:

```rust
    section_metadata: Some(serde_json::to_value(LanguagePackageMetadata {
        ecosystem: ecosystem.to_string(),
        confidence: item.confidence.clone(),
        package_count: item.packages.len(),
        // Deterministic manifest_basis from Plan 1's manifest_files: HashMap<String, String>.
        // Priority order: requirements.txt > package-lock.json > Gemfile.lock > first key.
        // This matches the rendering priority in Plan 1 Task 6.
        manifest_basis: ["requirements.txt", "package-lock.json", "Gemfile.lock"]
            .iter()
            .find(|k| item.manifest_files.contains_key(**k))
            .map(|k| k.to_string())
            .or_else(|| {
                // HashMap iteration is non-deterministic — sort keys
                // for a stable fallback when no known manifest name matches.
                let mut keys: Vec<&String> = item.manifest_files.keys().collect();
                keys.sort();
                keys.first().map(|k| k.to_string())
            }),
        packages: item.packages.iter().map(|p| LanguagePackageDto {
            name: p.name.clone(),
            version: p.version.clone(),
        }).collect(),
    }).ok()),
```

Update the unmanaged files section builder (T8) to populate
`section_metadata`:

```rust
    section_metadata: Some(serde_json::to_value(UnmanagedFileMetadata {
        file_type: format!("{:?}", f.file_type),
        size: f.size,
        under_var: f.under_var,
        provenance: UnmanagedFileProvenanceDto {
            last_modified: f.provenance.last_modified,
            uid: f.provenance.uid,
            gid: f.provenance.gid,
            permissions: f.provenance.permissions.clone(),
            writable_mount: f.provenance.writable_mount,
            mutability: f.provenance.mutable,
            service_working_dir: f.provenance.service_working_dir,
        },
    }).ok()),
```

- [ ] **Step 4: Define variant payload DTOs (Rust)**

```rust
/// Variant payload for language packages — package-list diff inputs.
#[derive(Serialize)]
pub struct LanguagePackageVariantPayload {
    /// Per-variant package lists for diff rendering
    pub variant_packages: Vec<VariantPackageList>,
}

#[derive(Serialize)]
pub struct VariantPackageList {
    pub content_hash: String,
    pub hosts: Vec<String>,
    pub host_count: usize,
    pub selected: bool,
    pub packages: Vec<LanguagePackageDto>,
}

/// Variant payload for unmanaged files — metadata comparison inputs.
#[derive(Serialize)]
pub struct UnmanagedFileVariantPayload {
    /// Per-variant metadata for comparison rendering
    pub variant_metadata: Vec<VariantFileMetadata>,
}

#[derive(Serialize)]
pub struct VariantFileMetadata {
    pub content_hash: String,
    pub hosts: Vec<String>,
    pub host_count: usize,
    pub selected: bool,
    pub size: u64,
    pub last_modified: u64,
}
```

- [ ] **Step 5: Add TypeScript DTOs**

In `crates/web/ui/src/api/types.ts`:

```typescript
/** Section-specific metadata — language packages */
export interface LanguagePackageMetadata {
  ecosystem: string;
  confidence: string;
  package_count: number;
  manifest_basis: string | null;
  packages: LanguagePackageDto[];
}

export interface LanguagePackageDto {
  name: string;
  version: string;
}

/** Section-specific metadata — unmanaged files */
export interface UnmanagedFileMetadata {
  file_type: string;
  size: number;
  under_var: boolean;
  provenance: UnmanagedFileProvenanceDto;
}

export interface UnmanagedFileProvenanceDto {
  last_modified: number;
  uid: number;
  gid: number;
  permissions: string;
  writable_mount: boolean;
  mutability: boolean;
  service_working_dir: boolean;
}

/** Variant payload — language packages */
export interface LanguagePackageVariantPayload {
  variant_packages: VariantPackageList[];
}

export interface VariantPackageList {
  content_hash: string;
  hosts: string[];
  host_count: number;
  selected: boolean;
  packages: LanguagePackageDto[];
}

/** Variant payload — unmanaged files */
export interface UnmanagedFileVariantPayload {
  variant_metadata: VariantFileMetadata[];
}

export interface VariantFileMetadata {
  content_hash: string;
  hosts: string[];
  host_count: number;
  selected: boolean;
  size: number;
  last_modified: number;
}
```

Add `section_metadata` to the `AggregateItem` interface:

```typescript
export interface AggregateItem {
  // ... existing fields ...
  section_metadata?: Record<string, unknown>;
  /** Variant payload — only present when item has variants across hosts.
   *  Language packages: LanguagePackageVariantPayload
   *  Unmanaged files: UnmanagedFileVariantPayload
   *  Track C T12 reads this to render variant diff views. */
  variant_payload?: Record<string, unknown>;
}
```

- [ ] **Step 6: Add tests**

```rust
    #[test]
    fn language_packages_section_metadata_populated() {
        // Build aggregate with a pip environment that has 3 packages
        // Assert: section_metadata is Some and contains ecosystem, confidence,
        //   package_count, manifest_basis, and packages array
    }

    #[test]
    fn unmanaged_files_section_metadata_populated() {
        // Build aggregate with an ELF binary under /var
        // Assert: section_metadata contains file_type, size, under_var: true,
        //   and provenance with last_modified, uid, gid, permissions
    }
```

- [ ] **Step 7: Verify tests + TS compile, commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
cargo test -p inspectah-web -- aggregate
cd crates/web/ui && npx tsc --noEmit
cd /Users/mrussell/Work/bootc-migration/inspectah
git add crates/web/src/aggregate_handlers.rs crates/web/ui/src/api/types.ts
git commit -m "feat(web): add per-section metadata and variant payload DTOs

Extends AggregateItem with section_metadata carrying per-item
context (ecosystem/confidence/packages for lang pkgs, file type/
size/provenance for unmanaged files). Adds variant payload types
for package-list diffs and metadata comparison. TypeScript DTOs
mirror the Rust structures.

Assisted-by: Claude Code (Opus 4.6)"
```

**Thorn checkpoint: after T9a complete (full backend API contract for Track C).**

---

## TDD Protocol

Each task follows red-green-refactor:

1. **Red:** Write the test first (or alongside the code where the test
   structure is trivial). The test must fail before the implementation.
2. **Green:** Implement the minimum code to make the test pass.
3. **Refactor:** Clean up, ensure Clippy/fmt compliance.

For Tasks 1-4a (compose pipeline), tests verify:
- Serde roundtrip for `raw_content` (T1)
- Secret scrubbing correctness (T2)
- Tarball contains `compose/` entries, redaction applied (T3)
- Containerfile output contains comment block, no COPY/RUN (T4)
- Docs updated (T4a — manual verification)

For Tasks 5-9a (aggregate), tests verify:
- Identity key produces `ecosystem:path` format (T5)
- Prevalence defaults: 100% = include, <100% = exclude (T5, T6)
- Aggregate-only fields deserialize from Plan 2 JSON (T5a)
- UnmanagedFile merges by path with prevalence + totals (T6)
- Language packages section emitted with correct zone/prevalence (T7)
- Unmanaged files section emitted with correct zone/prevalence (T8)
- TypeScript compiles without errors (T9)
- Section metadata populated, variant payload types defined (T9a)

---

## Track C: Aggregate UI (Tasks 10-14)

These tasks implement the aggregate UI work deferred from Plan 3. Plan 3
builds the single-host components and interaction contracts; this track
wires them into aggregate mode with the required decision-support metadata.

**Dependency:** Tasks 10-14 depend on Task 9a (API/DTO contract defining
section_metadata and variant payload shapes) and Plan 3 (single-host
components exist). Task 9a is the explicit data source — Track C does not
assume metadata from thin air.

### Task 10: AggregateItemRow — Section-Aware Metadata Rendering

**Files:**
- Modify: `crates/web/ui/src/components/aggregate/AggregateItemRow.tsx`
- Test: `crates/web/ui/src/components/aggregate/__tests__/AggregateItemRow.test.tsx`

**Interfaces:**
- Consumes: `AggregateItem.section_metadata` from Task 9a
  (`LanguagePackageMetadata` or `UnmanagedFileMetadata`)
- Produces: Section-specific row metadata rendering

Language Packages rows display: ecosystem icon/label, confidence badge
(green=high, orange=medium), package count badge, manifest basis subtitle.

Unmanaged Files rows display: file type icon/label, size badge,
`/var` warning icon when path is under `/var`.

- [ ] **Step 1: Write failing test for language package row metadata**

```typescript
test("renders ecosystem label and confidence badge for language package items", () => {
  // Render AggregateItemRow with sectionId="language_packages"
  // and item with section_metadata: { ecosystem: "pip", confidence: "high",
  //   package_count: 12, manifest_basis: "requirements.txt", packages: [...] }
  // Assert: ecosystem label, green confidence badge, "12 packages" badge
});
```

- [ ] **Step 2: Implement section-aware rendering**

In `AggregateItemRow`, branch on `sectionId` to extract and render
section-specific metadata from `item.section_metadata`:

```typescript
if (sectionId === "language_packages" && item.section_metadata) {
  const meta = item.section_metadata as LanguagePackageMetadata;
  // Render: ecosystem label, confidence badge, package count
}
if (sectionId === "unmanaged_files" && item.section_metadata) {
  const meta = item.section_metadata as UnmanagedFileMetadata;
  // Render: file type label, size badge, /var warning
}
```

- [ ] **Step 3: Write test for unmanaged file row metadata**

```typescript
test("renders file type and size for unmanaged file items", () => {
  // item with section_metadata: { file_type: "elf_binary", size: 2400000,
  //   under_var: true, provenance: {...} }
  // Assert: "ELF" label, "2.3 MB" badge, /var warning icon
});
```

- [ ] **Step 4: Run tests and verify**

```bash
cd crates/web/ui && npx jest AggregateItemRow --no-coverage
```

- [ ] **Step 5: Commit**

```
feat(web): add section-aware aggregate row metadata for new sections
```

### Task 11: Aggregate Detail Pane — Language Packages & Unmanaged Files

**Files:**
- Modify: `crates/web/ui/src/components/aggregate/ItemDetailPane.tsx`
- Test: `crates/web/ui/src/components/aggregate/__tests__/ItemDetailPane.test.tsx`

**Interfaces:**
- Consumes: `AggregateItem.section_metadata` from Task 9a
  (`LanguagePackageMetadata.packages` for full list,
  `UnmanagedFileMetadata.provenance` for detail signals)
- Produces: Detail pane content for language envs and unmanaged files

- [ ] **Step 1: Write failing test for language package detail pane**

```typescript
test("renders full package list in detail pane for language_packages section", () => {
  // Detail item with section_metadata.packages:
  //   [{name: "flask", version: "2.3.3"}, {name: "requests", version: "2.31.0"}]
  // Assert: each package name and version rendered in table
  // Assert: confidence level shown
  // Assert: manifest basis ("from requirements.txt") shown
});
```

- [ ] **Step 2: Implement detail pane branches**

Add `language_packages` and `unmanaged_files` section handlers to
`ItemDetailPane`. Language packages show full package list table
from `section_metadata.packages`. Unmanaged files show provenance
signals from `section_metadata.provenance`.

- [ ] **Step 3: Write test for unmanaged file detail pane**

- [ ] **Step 4: Run tests and commit**

```
feat(web): add aggregate detail pane for language packages and unmanaged files
```

### Task 11a: Variant Diff Payloads — Backend Data for T12

**Files:**
- Modify: `crates/web/src/aggregate_handlers.rs`
- Test: Rust unit tests

**Interfaces:**
- Depends on: Task 9a (variant payload DTOs defined),
  Task 5a (`content_hash` on UnmanagedFile)
- Produces: populated variant payloads in aggregate response
- Consumed by: Task 12 (variant comparison UI)

This task produces the actual variant data structures that Task 12's UI
consumes. Without this, T12 has no declared backend data source.

- [ ] **Step 1: Populate language package variant payloads**

When the merge layer detects variants (different package lists for the
same ecosystem:path), build `LanguagePackageVariantPayload` with
per-variant package lists:

```rust
    // In the language packages section builder, when variants are detected:
    let variant_payload = if has_variants {
        Some(serde_json::to_value(LanguagePackageVariantPayload {
            variant_packages: variant_items.iter().map(|vi| {
                VariantPackageList {
                    content_hash: vi.content_hash.clone(),
                    hosts: vi.hosts.clone(),
                    host_count: vi.host_count,
                    selected: vi.selected,
                    packages: vi.packages.iter().map(|p| LanguagePackageDto {
                        name: p.name.clone(),
                        version: p.version.clone(),
                    }).collect(),
                }
            }).collect(),
        }).ok())
    } else {
        None
    };
```

- [ ] **Step 2: Populate unmanaged file variant payloads**

When the merge layer detects variants (same path, different content
hash), build `UnmanagedFileVariantPayload` with per-variant metadata:

```rust
    let variant_payload = if has_variants {
        Some(serde_json::to_value(UnmanagedFileVariantPayload {
            variant_metadata: variant_files.iter().map(|vf| {
                VariantFileMetadata {
                    content_hash: vf.content_hash.clone(),
                    hosts: vf.hosts.clone(),
                    host_count: vf.host_count,
                    selected: vf.selected,
                    size: vf.size,
                    last_modified: vf.provenance.last_modified,
                }
            }).collect(),
        }).ok())
    } else {
        None
    };
```

- [ ] **Step 3: Add tests**

```rust
    #[test]
    fn language_package_variant_payload_populated() {
        // Two hosts with same pip:/opt/app/venv but different package lists
        // Assert: variant_packages has 2 entries with distinct content_hash,
        //   each containing the full package list for that variant
    }

    #[test]
    fn unmanaged_file_variant_payload_populated() {
        // Two hosts with /opt/app/server, different content_hash
        // Assert: variant_metadata has 2 entries with size and last_modified
    }
```

- [ ] **Step 4: Verify tests pass, commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
cargo test -p inspectah-web -- aggregate
git add crates/web/src/aggregate_handlers.rs
git commit -m "feat(web): populate variant diff payloads for aggregate sections

Language package variants carry per-variant package lists for
package-list diff rendering. Unmanaged file variants carry size and
last-modified for metadata comparison. Provides the backend data
source Task 12's variant UI consumes.

Assisted-by: Claude Code (Opus 4.6)"
```

### Task 12: Aggregate Variant Views

**Files:**
- Modify: `crates/web/ui/src/components/aggregate/VariantView.tsx` (or new component)
- Test: matching test file

**Interfaces:**
- Consumes: Variant payloads from Task 11a
  (`LanguagePackageVariantPayload` and `UnmanagedFileVariantPayload`)
- Produces: Variant comparison UI per spec's aggregate decision-support contract

**Language Packages variant view:** When hosts diverge on the same
environment path, show a structured diff: added packages, removed
packages, version differences. NOT a text diff — a package-list diff
with columns for each variant. Data source: `variant_packages` array
from `LanguagePackageVariantPayload`.

**Unmanaged Files variant view:** When hosts have the same path but
different content hash, show metadata comparison: file size per variant,
last-modified per variant, "content differs" indicator. No binary diff.
Data source: `variant_metadata` array from
`UnmanagedFileVariantPayload`.

- [ ] **Step 1: Write failing test for package-list variant diff**

```typescript
test("renders package-list diff between variants for language packages", () => {
  // Two variants from LanguagePackageVariantPayload.variant_packages:
  //   Variant A: [flask 2.3.3, requests 2.31.0]
  //   Variant B: [flask 2.3.3, requests 2.32.0, newpkg 1.0.0]
  // Assert: "requests" shown as version change (2.31.0 -> 2.32.0)
  // Assert: "newpkg" shown as added in variant B
});
```

- [ ] **Step 2: Implement variant views**

- [ ] **Step 3: Write test for unmanaged file variant metadata**

```typescript
test("renders metadata comparison for unmanaged file variants", () => {
  // Two variants from UnmanagedFileVariantPayload.variant_metadata:
  //   Variant A: size=52MB, last_modified=1719500000
  //   Variant B: size=53MB, last_modified=1719600000
  // Assert: size and last-modified shown for each variant
  // Assert: "content differs" indicator present
});
```

- [ ] **Step 4: Run tests and commit**

```
feat(web): add variant comparison views for aggregate language packages and unmanaged files
```

### Task 13: Aggregate Search — New Section Coverage

**Files:**
- Modify: `crates/web/ui/src/components/aggregate/AggregateApp.tsx` (search scope)
- Modify: `crates/web/ui/src/components/GlobalSearch.tsx` (if aggregate search is shared)
- Test: matching test files

**Interfaces:**
- Consumes: Aggregate section items + `section_metadata` from Task 9a
- Produces: Searchable aggregate items matching spec's searchable fields

Per spec's aggregate decision-support contract:

| Section | Searchable fields |
|---------|-------------------|
| Language Packages | ecosystem, environment path, package names, manifest basis ("lockfile", "dist-info") |
| Unmanaged Files | file path, file type |

Search indexing uses `section_metadata` fields from Task 9a for
section-specific searchable content.

- [ ] **Step 1: Write failing test**

```typescript
test("aggregate global search includes language package items", () => {
  // Search for "flask" — should match a language package env containing flask
  // via section_metadata.packages[].name
});
```

- [ ] **Step 2: Extend search indexing for new sections**

- [ ] **Step 3: Write test for unmanaged file search**

- [ ] **Step 4: Run tests and commit**

```
feat(web): extend aggregate search to language packages and unmanaged files
```

### Task 14: Aggregate Sidebar — New Section Wiring

**Files:**
- Modify: `crates/web/ui/src/components/aggregate/AggregateSidebar.tsx`
- Modify: `crates/web/ui/src/components/aggregate/AggregateApp.tsx`
- Test: matching test files

**Interfaces:**
- Consumes: Aggregate view response with language_packages and unmanaged_files sections
- Produces: Sidebar entries with zone-based counts (consensus/near-consensus/divergent),
  include/total counts matching single-host pattern

`AggregateSidebar` is data-driven from the sections array — new sections
may appear automatically if the backend returns them with `is_decision_section: true`.
Verify this works and add explicit tests. If not automatic, wire the new
section IDs into the sidebar rendering.

**Scope note:** This task covers **aggregate** sidebar only
(`AggregateSidebar.tsx`). Single-host `Sidebar.tsx` is Plan 3 scope —
Plan 3 Task 8 handles adding Language Packages and Unmanaged Files to
the single-host sidebar. Do not modify `Sidebar.tsx` in this task.

- [ ] **Step 1: Write test**

```typescript
test("aggregate sidebar shows language_packages and unmanaged_files sections", () => {
  // Mock aggregate view with both sections (is_decision_section: true)
  // Assert: both appear in Review group with zone-based counts
});
```

- [ ] **Step 2: Verify or wire sidebar rendering**

- [ ] **Step 3: Run tests and commit**

```
feat(web): wire language packages and unmanaged files into aggregate sidebar
```

**Thorn checkpoint: review Tasks 10-14 before marking Plan 4 complete.**

---

## Aggregate Parity Note

These tasks implement aggregate-mode support using the same components
Plan 3 built for single-host mode. Shared components (LanguagePackageList,
UnmanagedFileList, RpmUploadModal) should not need aggregate-specific
forks — they receive data through props. The aggregate-specific work is
in the metadata rendering (AggregateItemRow), detail pane, variant views,
search scope, and sidebar wiring.

If visual or interaction drift is detected between single-host and
aggregate during implementation, file a comms thread before proceeding.

---

## Execution Notes

**Parallelism:** Tracks A, B, and C have the following dependencies:
- Track A (compose, T1-T4a): independent, can start immediately
- Track B (aggregate backend, T5-T9a): depends on Plans 1+2 landing
- Track C (aggregate UI, T10-T14): depends on Task 9a AND Plan 3

**Dependency ordering within tracks:**
- Track A: T1 -> T2 -> T3 -> T4 -> T4a (strictly sequential)
- Track B: T5 + T5a can run in parallel, then T6 -> T8, T7 depends
  on T5, T9 depends on T7+T8, T9a depends on T9
- Track C: T10 -> T11 -> T11a -> T12 -> T13 -> T14

**Plan dependencies:** Tasks 5-8 depend on Plan 1 and Plan 2 landing
first (they need the extended NonRpmItem fields, UnmanagedFile types,
and ItemId variants). Tasks 10-14 depend on Plan 3 (single-host
components exist) AND Task 9a (API/DTO contract). Tasks 1-4a can
proceed independently — ComposeFile is in a different type module.

**Cross-crate visibility:** Task 2 places `scrub_compose_secrets` in
`inspectah-core::redaction` (decision made, not deferred). Both
`inspectah-collect` and `inspectah-refine` already depend on
`inspectah-core`, so no new cross-crate dependency is introduced.

## Compose Sensitivity Handoff

The spec requires the refine UI to show a sensitivity indicator on
compose entries when secret-like patterns were detected. This is a
**Plan 3 UI concern** — the compose sidebar destination and its visual
treatment are Plan 3 scope. Task 4a documents the handoff explicitly.
If Plan 3 was approved without covering this, a follow-up patch plan
is needed to add the sensitivity badge to the compose sidebar entry.

## Revision History

- **R1 (2026-06-28):** Revised per review checklist. Changes:
  - Fixed `UnmanagedFileSection` contract drift: `items` (not `files`),
    preserved `total_size` and `total_count` throughout
  - Added Task 5a: aggregate-only fields (`content_hash`,
    `variant_selection`) on `UnmanagedFile` with `serde(default)`
  - Added Task 9a: API/DTO contract task defining `section_metadata`,
    variant payload DTOs, and TypeScript mirrors — bridges Track B/C
  - Added Task 11a: variant diff payload population — backend data
    source for Task 12's variant comparison UI
  - Added prevalence-default tests to T5, T6, T7, T8 proving
    `100% = include`, `<100% = exclude` per section
  - Removed T9 Step 2 (single-host `Sidebar.tsx`) — Plan 3 scope
  - Added Task 4a: `docs/reference/output-artifacts.md` update +
    named compose sensitivity handoff to Plan 3
  - Pinned compose scrubber to `inspectah-core::redaction`
  - Updated Track C dependency to reference Task 9a explicitly
  - Aligned all code examples with Plan 2's actual field names
