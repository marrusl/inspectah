# Non-RPM Replication Plan 4: Compose Raw Content + Aggregate Support

<!-- agentic: tang | model: opus | sdd: true | thorn-checkpoint: after T3, T6, T9 -->
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
4. Aggregate view returns `language_packages` and `unmanaged_files`
   sections with zone-based layout
5. Aggregate merge uses correct identity keys: ecosystem+path for language
   envs, file path for unmanaged files
6. Prevalence-based defaults: 100% = include, <100% = exclude
7. Variant handling: package-list diff for language envs, content-hash
   for unmanaged files
8. All existing tests pass, new tests cover each task
9. Clippy clean, `cargo fmt`, no warnings

## Architecture

```
Plan 4 has two independent tracks that share no code:

Track A: Compose raw content (Tasks 1-4)
  containers.rs → types/containers.rs → session.rs → containerfile.rs
  collector        data model            export        rendering

Track B: Aggregate sections (Tasks 5-9)
  merge.rs → aggregate_handlers.rs → aggregate TS types
  identity    section builder         frontend DTOs
```

Track A modifies the existing compose pipeline end-to-end. Track B adds
new aggregate sections following the exact pattern used by packages,
configs, services, sysctls, and containers.

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
6. Compose stays reference-only: no include toggles, no Containerfile
   COPY/RUN directives
7. Aggregate sections use zone-based layout via `build_section()` — same
   pattern as packages, configs, services, sysctls

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

## File Map

### Track A: Compose Raw Content

| Task | Files | Creates/Modifies |
|------|-------|------------------|
| T1 | `crates/core/src/types/containers.rs` | Modify: add `raw_content` field |
| T2 | `crates/collect/src/inspectors/containers.rs` | Modify: retain raw YAML, add redaction scrub |
| T3 | `crates/refine/src/session.rs`, `crates/refine/tests/export_contract_test.rs` | Modify: compose export + allowlist |
| T4 | `crates/pipeline/src/render/containerfile.rs` | Modify: compose comment block |

### Track B: Aggregate Support

| Task | Files | Creates/Modifies |
|------|-------|------------------|
| T5 | `crates/core/src/aggregate/merge.rs` | Modify: NonRpmItem identity key |
| T6 | `crates/core/src/aggregate/merge.rs` | Modify: add UnmanagedFile merge |
| T7 | `crates/web/src/aggregate_handlers.rs` | Modify: language_packages section |
| T8 | `crates/web/src/aggregate_handlers.rs` | Modify: unmanaged_files section |
| T9 | `crates/web/ui/src/api/types.ts` | Modify: aggregate TS DTOs |

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
- Test: new unit tests in same file

**Interfaces:**
- Depends on: Task 1 (`ComposeFile.raw_content`)
- Produces: populated `raw_content` during collection
- Consumed by: Tasks 3, 4

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

- [ ] **Step 2: Add `scrub_compose_secrets` utility function**

Add a new function after `scan_compose_env_secrets`:

```rust
/// Scrubs secret-like values from compose YAML content.
///
/// Replaces values of environment variables whose names match
/// SECRET_PATTERNS with `<REDACTED>`. Handles both `KEY=VALUE` and
/// `KEY: value` patterns within `environment:` blocks.
fn scrub_compose_secrets(content: &str) -> String {
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
cargo test -p inspectah-collect -- containers
git add crates/collect/src/inspectors/containers.rs
git commit -m "feat(collect): retain raw compose YAML and add secret scrubber

find_compose_files now populates ComposeFile.raw_content with the
raw YAML. scrub_compose_secrets replaces secret-like env var values
with <REDACTED> for use during redacted export.

Assisted-by: Claude Code (Opus 4.6)"
```

**Thorn checkpoint: after T1-T3 complete (covers data model + collector + export).**

---

## Task 3: Compose Export — Tarball + Allowlist

**Files:**
- Modify: `crates/refine/src/session.rs`
- Modify: `crates/refine/tests/export_contract_test.rs`
- Test: export contract test

**Interfaces:**
- Depends on: Tasks 1-2 (`ComposeFile.raw_content` populated)
- Produces: `compose/` directory in tarball
- Consumed by: Task 4 (Containerfile references compose/)

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
                    inspectah_collect::inspectors::containers::scrub_compose_secrets(
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

**Note on `scrub_compose_secrets` visibility:** The function is currently
private in `containers.rs`. Make it `pub(crate)` — or better, since it is
called cross-crate from `inspectah-refine`, make it `pub` and re-export
from `inspectah_collect::inspectors::containers`. If `inspectah-refine`
does not already depend on `inspectah-collect`, an alternative approach:
move `scrub_compose_secrets` to a utility module in `inspectah-core`
(e.g., `crates/core/src/redaction.rs`) where both crates can reach it.
Evaluate the dependency graph at implementation time and pick the path
that avoids circular dependencies.

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
# Also add scrub_compose_secrets visibility change if applicable
git commit -m "feat(refine): export compose files to tarball with redaction

Writes compose files under compose/ in the export tarball, mirroring
the source directory structure. Secret-like env var values are scrubbed
when the snapshot is in redacted state. Adds compose to the export
allowlist.

Assisted-by: Claude Code (Opus 4.6)"
```

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

**Thorn checkpoint: after T4 complete (full compose pipeline end-to-end).**

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
        // renames PipPackage → LanguagePackage and reuses the same field).
        let mut hasher = Sha256::new();
        hasher.update(self.method.as_bytes());
        hasher.update(b"\n");
        for pkg in &self.packages {
            hasher.update(format!("{}={}\n", pkg.name, pkg.version).as_bytes());
        }
        Some(Cow::Owned(format!("{:x}", hasher.finalize())))
    }
```

- [ ] **Step 3: Add unit test for composite identity key**

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
legacy items without a path.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 6: Aggregate Merge — UnmanagedFile Support

**Files:**
- Modify: `crates/core/src/aggregate/merge.rs`
- Test: new unit test in same file

**Interfaces:**
- Depends on: Plan 2 Task 1 (`UnmanagedFile`, `UnmanagedFileSection` types)
- Produces: `AggregateMergeable` impl for `UnmanagedFile`, merge function
- Consumed by: Task 8

- [ ] **Step 1: Implement `AggregateMergeable` for `UnmanagedFile`**

In `crates/core/src/aggregate/merge.rs`, add after the `NonRpmItem` impl:

```rust
use crate::types::nonrpm::UnmanagedFile;

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

**Note:** This depends on Plan 2 Task 1 having added `UnmanagedFile` with
fields `path`, `aggregate`, `include`, `content_hash`, and
`variant_selection`. If the struct differs at implementation time, adjust
field names to match.

- [ ] **Step 2: Add `merge_unmanaged_file_sections` function**

```rust
/// Merge unmanaged file sections from multiple hosts.
pub fn merge_unmanaged_file_sections(
    sections: Vec<Option<UnmanagedFileSection>>,
    total_hosts: usize,
    hostnames: &[String],
) -> Option<UnmanagedFileSection> {
    if sections.iter().all(|s| s.is_none()) {
        return None;
    }

    let files = merge_items(
        collect_items(&sections, |s| &s.files),
        total_hosts,
        hostnames,
    );

    Some(UnmanagedFileSection { files })
}
```

Add the corresponding import for `UnmanagedFileSection` at the top of the
nonrpm import block.

- [ ] **Step 3: Wire into the top-level merge function**

Find the function that calls `merge_nonrpm_sections` and
`merge_container_sections` (the top-level snapshot merge). Add
`merge_unmanaged_file_sections` in the same pattern, assigning the
result to `merged_snap.unmanaged_files`.

**Note:** The `InspectionSnapshot` field for unmanaged files is added by
Plan 2 Task 1 (likely `unmanaged_files: Option<UnmanagedFileSection>`).
Follow the same pattern as `non_rpm_software`.

- [ ] **Step 4: Add unit test**

```rust
    #[test]
    fn unmanaged_file_merge_by_path() {
        let section_a = Some(UnmanagedFileSection {
            files: vec![UnmanagedFile {
                path: "/opt/splunk/bin/splunkd".to_string(),
                include: true,
                content_hash: "aaa111".to_string(),
                ..Default::default()
            }],
        });
        let section_b = Some(UnmanagedFileSection {
            files: vec![UnmanagedFile {
                path: "/opt/splunk/bin/splunkd".to_string(),
                include: true,
                content_hash: "bbb222".to_string(),
                ..Default::default()
            }],
        });

        let merged = merge_unmanaged_file_sections(
            vec![section_a, section_b],
            2,
            &["host-a".into(), "host-b".into()],
        )
        .unwrap();

        // Same path → merged into one item with aggregate prevalence.
        assert_eq!(merged.files.len(), 1);
        let file = &merged.files[0];
        assert_eq!(file.path, "/opt/splunk/bin/splunkd");
        let agg = file.aggregate.as_ref().unwrap();
        assert_eq!(agg.total, 2);
    }
```

- [ ] **Step 5: Verify tests pass, commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
cargo test -p inspectah-core -- merge
git add crates/core/src/aggregate/merge.rs
git commit -m "feat(core): add UnmanagedFile aggregate merge support

Implements AggregateMergeable for UnmanagedFile using file path as
identity key and content hash for variant detection. Adds
merge_unmanaged_file_sections following the same pattern as other
section merges.

Assisted-by: Claude Code (Opus 4.6)"
```

**Thorn checkpoint: after T5-T6 complete (aggregate merge layer for both new sections).**

---

## Task 7: Aggregate Handler — Language Packages Section

**Files:**
- Modify: `crates/web/src/aggregate_handlers.rs`
- Test: new test in same file

**Interfaces:**
- Depends on: Task 5 (NonRpmItem composite identity key), Plan 1 Task 1
  (`NonRpmItem` field extensions), Plan 3 Task 2 (UI types)
- Produces: `language_packages` section in aggregate view response
- Consumed by: Task 9 (TS types)

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
            // Group by identity key for variant detection.
            let mut env_groups: std::collections::BTreeMap<
                String,
                Vec<&NonRpmItem>,
            > = std::collections::BTreeMap::new();
            for (item, item_id) in &lang_envs {
                let key = match item_id {
                    ItemId::LanguageEnv { ecosystem, path } => {
                        format!("{ecosystem}:{path}")
                    }
                    _ => continue,
                };
                env_groups.entry(key).or_default().push(item);
            }

            let mut items: Vec<AggregateItem> = Vec::new();
            for (key, group) in &env_groups {
                // Pick representative: highest prevalence count, else first.
                let representative = group
                    .iter()
                    .max_by_key(|item| {
                        item.aggregate.as_ref().map(|a| a.count).unwrap_or(0)
                    })
                    .unwrap_or(&&group[0]);

                let ecosystem = key.split(':').next().unwrap_or("other");
                let env_path = key.split(':').skip(1).collect::<Vec<_>>().join(":");
                let item_id = ItemId::LanguageEnv {
                    ecosystem: ecosystem.to_string(),
                    path: env_path,
                };
                let fp = representative.aggregate.as_ref();

                // Variant detection: different package lists across hosts.
                let variants = if group.len() >= 2 {
                    // Use content_variant_key from the merge layer's
                    // VariantSelection. Build variants from the aggregate
                    // prevalence data.
                    Some(build_content_variants(
                        &group
                            .iter()
                            .map(|item| {
                                (
                                    &item.content,
                                    item.variant_selection,
                                    item.aggregate.as_ref(),
                                )
                            })
                            .collect::<Vec<_>>(),
                    ))
                } else {
                    None
                };

                items.push(AggregateItem {
                    item_id,
                    include: representative.include,
                    locked: representative.locked,
                    attention_reason: None,
                    triage: build_triage_dto(
                        &Triage {
                            bucket: if representative.include {
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
                    variants,
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

**Note on `VariantSelection` and `content` fields:** NonRpmItem currently
has a `content` field (String) and Plan 1 may add `variant_selection`.
If `variant_selection` is not on NonRpmItem at implementation time, use
`VariantSelection::Only` as default for all items (no variant detection
until the merge layer populates it). Adjust to match Plan 1's actual
implementation.

- [ ] **Step 3: Add necessary imports**

Ensure the following are imported at the top of `aggregate_handlers.rs`:

```rust
use inspectah_core::types::nonrpm::NonRpmItem;
```

If `ItemId::LanguageEnv` is not yet defined (depends on Plan 1 Task 1),
this task is blocked until Plan 1 lands.

- [ ] **Step 4: Add unit test**

```rust
    #[test]
    fn aggregate_language_packages_section_emitted() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use inspectah_core::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection};
        use inspectah_refine::session::RefineSession;

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

        let session = RefineSession::new(snap.clone());
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

        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let lang_section = sections
            .iter()
            .find(|s| s.id == "language_packages");

        assert!(
            lang_section.is_some(),
            "language_packages section should be present"
        );
    }
```

- [ ] **Step 5: Verify tests pass, commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
cargo test -p inspectah-web -- aggregate
git add crates/web/src/aggregate_handlers.rs
git commit -m "feat(web): add language_packages section to aggregate view

Groups non-RPM language environments by ecosystem:path identity key,
applies zone-based layout, and supports variant detection via package
list hashing. Prevalence-based defaults: 100% include, <100% exclude.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 8: Aggregate Handler — Unmanaged Files Section

**Files:**
- Modify: `crates/web/src/aggregate_handlers.rs`
- Test: new test in same file

**Interfaces:**
- Depends on: Task 6 (UnmanagedFile merge), Plan 2 Task 1
  (`UnmanagedFile`, `UnmanagedFileSection`, `ItemId::UnmanagedFile`)
- Produces: `unmanaged_files` section in aggregate view response
- Consumed by: Task 9 (TS types)

- [ ] **Step 1: Add unmanaged file section builder**

After the language packages section block in `build_aggregate_sections`,
add:

```rust
    // Unmanaged Files — decision items with aggregate prevalence
    if let Some(ref unmanaged) = snap.unmanaged_files {
        // Group by path for variant detection (different content hashes
        // across hosts for the same file path).
        let mut file_groups: std::collections::BTreeMap<
            &str,
            Vec<&UnmanagedFile>,
        > = std::collections::BTreeMap::new();
        for f in &unmanaged.files {
            file_groups.entry(f.path.as_str()).or_default().push(f);
        }

        let mut items: Vec<AggregateItem> = Vec::new();
        for (path, group) in &file_groups {
            let representative = group
                .iter()
                .find(|f| {
                    matches!(
                        f.variant_selection,
                        VariantSelection::Selected | VariantSelection::Only
                    )
                })
                .or_else(|| group.first());

            if let Some(f) = representative {
                let item_id = ItemId::UnmanagedFile {
                    path: path.to_string(),
                };
                let fp = f.aggregate.as_ref();

                let variants = if group.len() >= 2 {
                    Some(build_content_variants(
                        &group
                            .iter()
                            .map(|f| {
                                (
                                    &f.content_hash as &str,
                                    f.variant_selection,
                                    f.aggregate.as_ref(),
                                )
                            })
                            .collect::<Vec<_>>(),
                    ))
                } else {
                    None
                };

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
                    variants,
                    source_repo: String::new(),
                    repo_conflict: None,
                });
            }
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

- [ ] **Step 2: Add necessary imports**

```rust
use inspectah_core::types::nonrpm::UnmanagedFile;
```

If `snap.unmanaged_files` field or `UnmanagedFileSection` doesn't exist
yet (depends on Plan 2 Task 1), this task is blocked.

- [ ] **Step 3: Handle `build_content_variants` type compatibility**

The `build_content_variants` helper expects `(&str, VariantSelection,
Option<&AggregatePrevalence>)` tuples. For unmanaged files, the
"content" for variant comparison is the `content_hash` field (a String),
not a full file body. Verify the type of `content_hash` — if it's
`String`, use `f.content_hash.as_str()` in the map closure. If the
`build_content_variants` function hashes its input internally, using
the pre-hashed content_hash is fine (it produces a hash-of-hash, which
is still a stable differentiator).

If `build_content_variants` doesn't fit cleanly (it expects full content
for re-hashing), write a simpler variant builder that uses the content
hash directly:

```rust
fn build_hash_variants(
    entries: &[(&str, VariantSelection, Option<&AggregatePrevalence>)],
) -> Vec<AggregateVariant> {
    entries
        .iter()
        .map(|(hash, selection, prevalence)| AggregateVariant {
            content_hash: ContentHash(hash.to_string()),
            variant_selection: *selection,
            host_count: prevalence.map(|p| p.count as usize).unwrap_or(0),
            hosts: prevalence
                .map(|p| p.hosts.clone())
                .unwrap_or_default(),
        })
        .collect()
}
```

- [ ] **Step 4: Add unit test**

```rust
    #[test]
    fn aggregate_unmanaged_files_section_emitted() {
        use inspectah_core::types::aggregate::AggregateSnapshotMeta;
        use inspectah_core::types::nonrpm::{UnmanagedFile, UnmanagedFileSection};
        use inspectah_refine::session::RefineSession;

        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            unmanaged_files: Some(UnmanagedFileSection {
                files: vec![UnmanagedFile {
                    path: "/opt/splunk/bin/splunkd".to_string(),
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
            }),
            ..Default::default()
        };

        let session = RefineSession::new(snap.clone());
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

        let sections = build_aggregate_sections(&session, &snap, &ctx);
        let unmanaged_section = sections
            .iter()
            .find(|s| s.id == "unmanaged_files");

        assert!(
            unmanaged_section.is_some(),
            "unmanaged_files section should be present"
        );
    }
```

- [ ] **Step 5: Verify tests pass, commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
cargo test -p inspectah-web -- aggregate
git add crates/web/src/aggregate_handlers.rs
git commit -m "feat(web): add unmanaged_files section to aggregate view

Groups unmanaged files by path with content-hash variant detection.
Uses zone-based layout consistent with other aggregate sections.
Prevalence-based defaults: 100% include, <100% exclude.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 9: TypeScript — Aggregate DTO Updates

**Files:**
- Modify: `crates/web/ui/src/api/types.ts`
- Test: type-level only (TypeScript compiler)

**Interfaces:**
- Depends on: Tasks 7-8 (new aggregate sections)
- Produces: TS types for frontend consumption
- Consumed by: Plan 3 aggregate UI components (if not already covered)

- [ ] **Step 1: Verify `ItemId` union type includes new variants**

Plan 1 Task 1 should have added `ItemIdLanguageEnv` and Plan 2 Task 1
should have added `ItemIdUnmanagedFile` to the TypeScript `ItemId` union.
Verify they exist:

```typescript
// Should already exist from Plan 1/2:
export interface ItemIdLanguageEnv {
  type: "LanguageEnv";
  key: { ecosystem: string; path: string };
}

export interface ItemIdUnmanagedFile {
  type: "UnmanagedFile";
  key: { path: string };
}
```

If missing, add them and include in the `ItemId` union type.

- [ ] **Step 2: Add section IDs to aggregate sidebar**

In the sidebar component or wherever aggregate section IDs are listed for
navigation, ensure `"language_packages"` and `"unmanaged_files"` are
included. Check `crates/web/ui/src/components/Sidebar.tsx` — the
`REVIEW_SECTIONS` array likely needs these additions:

```typescript
  { id: "language_packages", label: "Language Packages" },
  { id: "unmanaged_files", label: "Unmanaged Files" },
```

These should be added as decision sections (they have include/exclude
toggles), not reference sections.

- [ ] **Step 3: Verify TypeScript compiles**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/crates/web/ui
npx tsc --noEmit
```

- [ ] **Step 4: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add crates/web/ui/src/api/types.ts crates/web/ui/src/components/Sidebar.tsx
git commit -m "feat(web): add aggregate DTO types for language packages and unmanaged files

Ensures TypeScript types and sidebar navigation include the new
aggregate sections added by the backend.

Assisted-by: Claude Code (Opus 4.6)"
```

**Thorn checkpoint: after T9 complete (full aggregate pipeline end-to-end).**

---

## TDD Protocol

Each task follows red-green-refactor:

1. **Red:** Write the test first (or alongside the code where the test
   structure is trivial). The test must fail before the implementation.
2. **Green:** Implement the minimum code to make the test pass.
3. **Refactor:** Clean up, ensure Clippy/fmt compliance.

For Tasks 1-4 (compose pipeline), tests verify:
- Serde roundtrip for `raw_content` (T1)
- Secret scrubbing correctness (T2)
- Tarball contains `compose/` entries, redaction applied (T3)
- Containerfile output contains comment block, no COPY/RUN (T4)

For Tasks 5-9 (aggregate), tests verify:
- Identity key produces `ecosystem:path` format (T5)
- UnmanagedFile merges by path with prevalence (T6)
- Aggregate view response contains `language_packages` section (T7)
- Aggregate view response contains `unmanaged_files` section (T8)
- TypeScript compiles without errors (T9)

## Execution Notes

**Parallelism:** Tracks A and B are independent. Tasks 1-4 (compose) and
Tasks 5-6 (aggregate merge) can be implemented in parallel if two agents
are available. Tasks 7-8 depend on Tasks 5-6, and Task 9 depends on
Tasks 7-8.

**Dependency ordering within tracks:**
- Track A: T1 → T2 → T3 → T4 (strictly sequential)
- Track B: T5 → T7, T6 → T8, T9 depends on T7+T8

**Plan dependencies:** Tasks 5-8 depend on Plan 1 and Plan 2 landing
first (they need the extended NonRpmItem fields, UnmanagedFile types,
and ItemId variants). Task 1-4 can proceed independently — ComposeFile
is in a different type module.

**Cross-crate visibility:** Task 3 needs `scrub_compose_secrets` from
`inspectah-collect`, but `inspectah-refine` may not depend on
`inspectah-collect`. Resolution options (decide at implementation time):
1. Move scrub function to `inspectah-core` (cleanest)
2. Add `inspectah-collect` as a dev/build dependency of `inspectah-refine`
3. Duplicate the small function (acceptable given its simplicity)
