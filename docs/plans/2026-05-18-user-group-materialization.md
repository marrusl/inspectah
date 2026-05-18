# User/Group Materialization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Generate kickstart, blueprint TOML, and Containerfile artifacts for migrating local human user accounts from package-mode RHEL to image-mode.

**Architecture:** Layered additions across the workspace: core types define the data model, the collector enriches scan-time data, the pipeline renders output artifacts (replacing the existing `users_section_lines` renderer), the refine layer adds user decision ops, and the web layer exposes the decision UI. Each layer builds on the previous — implement in order.

**Tech Stack:** Rust workspace (inspectah-core, inspectah-collect, inspectah-refine, inspectah-pipeline, inspectah-web), Axum web framework, React + TypeScript UI.

**Spec:** `docs/specs/proposed/2026-05-18-user-group-materialization-design.md`

**Key repo seams this plan touches:**
- `inspectah-pipeline/src/render/containerfile.rs:1071` — existing `users_section_lines()` (REPLACED, not layered beside)
- `inspectah-pipeline/src/render/mod.rs:34` — `render_all()` (scan-time artifacts added here)
- `inspectah-pipeline/src/redaction/engine.rs:367` — `redact()` (allowlist for preserved fields)
- `inspectah-refine/src/session.rs:758` — `render_refine_export()` (new artifacts added)
- `inspectah-refine/src/tarball.rs:141` — `from_tarball()` redaction_state acceptance
- `inspectah-refine/src/attention.rs:287` — unresolved hints attention handling
- `inspectah-web/src/handlers.rs:780` — `normalize_users_groups()` (removed from sections_cache)

---

### Task 1: Core Types — Redaction State and User Decision Enums

**Files:**
- Modify: `inspectah-core/src/types/redaction.rs`
- Modify: `inspectah-core/src/types/users.rs`
- Modify: `inspectah-core/src/snapshot.rs`
- Test: `inspectah-core/src/types/redaction.rs` (inline tests)
- Test: `inspectah-core/src/types/users.rs` (inline tests)

- [ ] **Step 1: Write failing test for `SensitiveRetained` variant**

In `inspectah-core/src/types/redaction.rs`, add to the existing `#[cfg(test)]` module:

```rust
#[test]
fn sensitive_retained_roundtrip() {
    let state = RedactionState::SensitiveRetained {
        redacted_by: "inspectah 0.8.0".into(),
        config_hash: "abc123".into(),
        unresolved_count: 2,
        unresolved_hints: vec![],
    };
    let json = serde_json::to_string(&state).unwrap();
    let parsed: RedactionState = serde_json::from_str(&json).unwrap();
    assert_eq!(state, parsed);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-core sensitive_retained_roundtrip`
Expected: FAIL — no `SensitiveRetained` variant

- [ ] **Step 3: Add `SensitiveRetained` variant to `RedactionState`**

In `inspectah-core/src/types/redaction.rs`, add to the `RedactionState` enum:

```rust
#[serde(rename = "sensitive_retained")]
SensitiveRetained {
    redacted_by: String,
    config_hash: String,
    unresolved_count: u32,
    #[serde(default)]
    unresolved_hints: Vec<RedactionHint>,
},
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p inspectah-core sensitive_retained_roundtrip`
Expected: PASS

- [ ] **Step 5: Write failing test for `UserContainerfileStrategy` enum**

In `inspectah-core/src/types/users.rs`:

```rust
#[test]
fn user_containerfile_strategy_roundtrip() {
    let skip: UserContainerfileStrategy = serde_json::from_str("\"skip\"").unwrap();
    assert_eq!(skip, UserContainerfileStrategy::Skip);
    let useradd: UserContainerfileStrategy = serde_json::from_str("\"useradd\"").unwrap();
    assert_eq!(useradd, UserContainerfileStrategy::Useradd);
}
```

- [ ] **Step 6: Add user decision enums**

In `inspectah-core/src/types/users.rs`, above the `UserGroupSection` struct:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserContainerfileStrategy {
    Skip,
    Useradd,
}

impl Default for UserContainerfileStrategy {
    fn default() -> Self { Self::Skip }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserPasswordChoice {
    None,
    Preserve,
    New,
}

impl Default for UserPasswordChoice {
    fn default() -> Self { Self::None }
}
```

- [ ] **Step 7: Run to verify enums pass**

Run: `cargo test -p inspectah-core -- users`
Expected: PASS

- [ ] **Step 8: Add sensitive snapshot metadata fields to `InspectionSnapshot`**

In `inspectah-core/src/snapshot.rs`:

```rust
#[serde(default)]
pub sensitive_snapshot: bool,
#[serde(default)]
pub preserved_credentials: bool,
#[serde(default)]
pub preserved_ssh_keys: bool,
```

- [ ] **Step 9: Run full core tests**

Run: `cargo test -p inspectah-core`
Expected: all PASS

- [ ] **Step 10: Commit**

```bash
git add inspectah-core/
git commit -m "feat(core): add SensitiveRetained state and user decision enums"
```

---

### Task 2: Trust Path — Redaction Engine, Attention, and Tarball Import

Wire `SensitiveRetained` into the three live producer/consumer seams that currently only handle `FullyRedacted` and `PartiallyRedacted`.

**Files:**
- Modify: `inspectah-pipeline/src/redaction/engine.rs`
- Modify: `inspectah-refine/src/attention.rs`
- Modify: `inspectah-refine/src/tarball.rs`
- Test: `inspectah-pipeline/src/redaction/engine.rs` (inline tests)
- Test: `inspectah-refine/src/attention.rs` (inline tests)
- Test: `inspectah-refine/src/tarball.rs` (inline tests)

- [ ] **Step 1: Write failing test — redaction engine skips preserved `password_hash` field**

In `inspectah-pipeline/src/redaction/engine.rs` test module:

```rust
#[test]
fn redact_preserves_password_hash_field_when_sensitive() {
    let mut snap = InspectionSnapshot::new();
    snap.sensitive_snapshot = true;
    snap.preserved_credentials = true;
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({
            "name": "alice",
            "uid": 1000,
            "password_hash": "$6$rounds=5000$salt$hash123"
        })],
        ..Default::default()
    });
    redact(&mut snap, &RedactOptions::default());
    let hash = snap.users_groups.unwrap().users[0]["password_hash"]
        .as_str().unwrap();
    assert_eq!(hash, "$6$rounds=5000$salt$hash123",
        "preserved password_hash must survive redaction when sensitive_snapshot is true");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-pipeline redact_preserves_password_hash`
Expected: FAIL — redaction engine strips the hash

- [ ] **Step 3: Implement redaction allowlist for sensitive snapshots**

In the `redact()` function (`inspectah-pipeline/src/redaction/engine.rs`), after scanning shadow entries (~line 955), add a guard that skips redaction of `password_hash` fields on user entries when `snapshot.preserved_credentials` is true. Similarly, skip redaction of `ssh_keys` content when `snapshot.preserved_ssh_keys` is true.

The existing shadow-entry scanning in the redaction engine (~line 955-984) operates on `shadow_entries` (raw colon-separated lines). The new `password_hash` field lives on the user JSON object, not in `shadow_entries`. The guard prevents the general content scanner from stripping the hash if it appears in user JSON during string-level scanning.

- [ ] **Step 4: Set `SensitiveRetained` state at end of redaction**

In `redact()`, after the existing redaction-state assignment (~line 1162), add:

```rust
if snapshot.sensitive_snapshot {
    snapshot.redaction_state = Some(RedactionState::SensitiveRetained {
        redacted_by: format!("inspectah {}", env!("CARGO_PKG_VERSION")),
        config_hash: config_hash.clone(),
        unresolved_count,
        unresolved_hints: unresolved_hints.clone(),
    });
}
```

This replaces the `FullyRedacted` or `PartiallyRedacted` state with `SensitiveRetained` when the snapshot was scanned with preserve flags, carrying forward any unresolved hints.

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p inspectah-pipeline redact_preserves_password_hash`
Expected: PASS

- [ ] **Step 6: Write failing test — attention handles `SensitiveRetained` unresolved hints**

In `inspectah-refine/src/attention.rs` test module:

```rust
#[test]
fn sensitive_retained_surfaces_unresolved_hints() {
    let mut snap = InspectionSnapshot::new();
    snap.redaction_state = Some(RedactionState::SensitiveRetained {
        redacted_by: "inspectah 0.8.0".into(),
        config_hash: "abc".into(),
        unresolved_count: 1,
        unresolved_hints: vec![RedactionHint {
            path: "/etc/httpd/conf/httpd.conf".into(),
            reason: "possible credential".into(),
            confidence: None,
        }],
    });
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/httpd/conf/httpd.conf".into(),
            include: true,
            ..Default::default()
        }],
    });

    let result = compute_attention(&snap);
    let config_attention = &result.config_files[0].attention;
    assert!(config_attention.iter().any(|a|
        a.level == AttentionLevel::NeedsReview
        && matches!(a.reason, AttentionReason::Custom(_))
    ), "SensitiveRetained with unresolved hints must surface NeedsReview");
}
```

- [ ] **Step 7: Handle `SensitiveRetained` in attention computation**

In `inspectah-refine/src/attention.rs`, where the existing code matches `PartiallyRedacted` (~line 287-296), extend the match arm to also handle `SensitiveRetained`:

```rust
if let Some(RedactionState::PartiallyRedacted { ref unresolved_hints, .. })
| Some(RedactionState::SensitiveRetained { ref unresolved_hints, .. })
    = snap.redaction_state
{
    // existing unresolved hint surfacing logic
}
```

- [ ] **Step 8: Run attention test to verify it passes**

Run: `cargo test -p inspectah-refine sensitive_retained_surfaces`
Expected: PASS

- [ ] **Step 9: Write failing test — tarball import accepts `SensitiveRetained`**

In `inspectah-refine/src/tarball.rs` test module (or `inspectah-refine/tests/`):

```rust
#[test]
fn from_tarball_accepts_sensitive_retained() {
    // Create a tarball with SensitiveRetained state
    let mut snap = test_snapshot();
    snap.redaction_state = Some(RedactionState::SensitiveRetained {
        redacted_by: "inspectah 0.8.0".into(),
        config_hash: "abc".into(),
        unresolved_count: 0,
        unresolved_hints: vec![],
    });
    snap.sensitive_snapshot = true;

    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("test.tar.gz");
    render_refine_export(&snap, &path).unwrap();

    let session = from_tarball(&path).unwrap();
    assert!(session.snapshot().sensitive_snapshot);
}
```

- [ ] **Step 10: Update `from_tarball` acceptance to include `SensitiveRetained`**

In `inspectah-refine/src/tarball.rs` (~line 141), add `SensitiveRetained` to the accepted states:

```rust
match &snap.redaction_state {
    Some(RedactionState::FullyRedacted { .. })
    | Some(RedactionState::PartiallyRedacted { .. })
    | Some(RedactionState::SensitiveRetained { .. }) => Ok(()),
    // ...reject others
}
```

- [ ] **Step 11: Run tarball import test**

Run: `cargo test -p inspectah-refine from_tarball_accepts_sensitive`
Expected: PASS

- [ ] **Step 12: Run all pipeline and refine tests**

Run: `cargo test -p inspectah-pipeline && cargo test -p inspectah-refine`
Expected: all PASS

- [ ] **Step 13: Commit**

```bash
git add inspectah-pipeline/src/redaction/ inspectah-refine/src/attention.rs inspectah-refine/src/tarball.rs
git commit -m "feat(trust): wire SensitiveRetained into redaction engine, attention, and tarball import"
```

---

### Task 3: Collector — Classification, Preserve Flags, Group Source

Enrichment order matters: collect all user metadata (sudo, SSH, groups) BEFORE computing `classification_rationale`, so the rationale includes all signals.

**Files:**
- Modify: `inspectah-collect/src/inspectors/users.rs`
- Test: `inspectah-collect/src/inspectors/users.rs` (inline tests)

- [ ] **Step 1: Add `preserve_password_hashes` and `preserve_ssh_keys` to `UserGroupOptions`**

```rust
#[derive(Debug, Clone, Default)]
pub struct UserGroupOptions {
    pub strategy_override: Option<String>,
    pub preserve_password_hashes: bool,
    pub preserve_ssh_keys: bool,
}
```

- [ ] **Step 2: Add new per-user fields during `parse_passwd`**

Initialize placeholder fields on each user entry in `parse_passwd`:

```rust
"has_sudo": false,
"has_subuid": false,
"supplementary_groups": serde_json::Value::Array(vec![]),
"classification_rationale": "",
```

- [ ] **Step 3: Wire enrichment fields AFTER all collectors run**

In the main `inspect` method, after all parsing is complete (passwd, shadow, group, gshadow, subuid, subgid, sudoers, SSH keys), run enrichment in this order:

1. After `parseSudoers`: iterate users, set `has_sudo: true` for any user matching a sudoers rule username.
2. After `parse_subid_file` for subuid: iterate users, set `has_subuid: true` for matching usernames.
3. After `parse_group`: iterate users, populate `supplementary_groups` array from group memberships (groups where the user appears in the members list, excluding primary group).
4. LAST — after ALL enrichments: compute `classification_rationale` for each user.

- [ ] **Step 4: Implement `build_classification_rationale`**

```rust
fn build_classification_rationale(user: &serde_json::Value) -> String {
    let mut parts = Vec::new();

    let shell = user.get("shell").and_then(|v| v.as_str()).unwrap_or("");
    if VALID_LOGIN_SHELLS.contains(&shell) {
        if let Some(name) = std::path::Path::new(shell).file_name() {
            parts.push(format!("{} shell", name.to_string_lossy()));
        }
    } else {
        parts.push(format!("{} (non-interactive)", shell));
    }

    if let Some(home) = user.get("home").and_then(|v| v.as_str()) {
        parts.push(format!("home at {}", home));
    }

    if let Some(status) = user.get("password_status").and_then(|v| v.as_str()) {
        parts.push(match status {
            "password_set" => "password set".into(),
            "locked" => "password locked".into(),
            "disabled" => "account disabled".into(),
            "no_password" => "no password".into(),
            other => format!("password {}", other),
        });
    }

    if user.get("has_sudo").and_then(|v| v.as_bool()).unwrap_or(false) {
        parts.push("has sudo".into());
    }

    if let Some(count) = user.get("ssh_key_count").and_then(|v| v.as_u64()) {
        if count > 0 {
            parts.push(format!("{} SSH key{}", count, if count == 1 { "" } else { "s" }));
        }
    }

    if let Some(groups) = user.get("supplementary_groups").and_then(|v| v.as_array()) {
        if !groups.is_empty() {
            let names: Vec<&str> = groups.iter().filter_map(|g| g.as_str()).collect();
            parts.push(format!("member of {}", names.join(", ")));
        }
    }

    parts.join(", ")
}
```

- [ ] **Step 5: Write test proving rationale includes ALL enrichment signals**

```rust
#[test]
fn classification_rationale_includes_all_enrichments() {
    let exec = MockExecutor::new()
        .with_file("/etc/passwd", "alice:x:1000:1000:Alice:/home/alice:/bin/bash\n")
        .with_file("/etc/shadow", "alice:$6$salt$hash:19700:0:99999:7:::\n")
        .with_file("/etc/group", "alice:x:1000:\nwheel:x:10:alice\n")
        .with_file("/etc/subuid", "alice:100000:65536\n")
        .with_file("/etc/subgid", "alice:100000:65536\n")
        .with_file("/etc/sudoers", "alice ALL=(ALL) NOPASSWD:ALL\n")
        .with_file("/home/alice/.ssh/authorized_keys", "ssh-ed25519 AAAA alice@work\n");

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };
    let result = UsersGroupsInspector::new().inspect(&ctx).unwrap();

    if let SectionData::UsersGroups(section) = &result.section {
        let rationale = section.users[0]["classification_rationale"].as_str().unwrap();
        assert!(rationale.contains("bash shell"), "missing shell: {rationale}");
        assert!(rationale.contains("/home/alice"), "missing home: {rationale}");
        assert!(rationale.contains("password set"), "missing password: {rationale}");
        assert!(rationale.contains("has sudo"), "missing sudo: {rationale}");
        assert!(rationale.contains("SSH key"), "missing SSH: {rationale}");
        assert!(rationale.contains("member of wheel"), "missing groups: {rationale}");
    } else {
        panic!("expected UsersGroups");
    }
}
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test -p inspectah-collect classification_rationale_includes_all`
Expected: PASS

- [ ] **Step 7: Implement password hash preservation in `parse_shadow`**

Add `preserve_hashes: bool` parameter to `parse_shadow`. When true AND status is `password_set`, store the raw hash in a `password_hash` field on the matching user entry (look up by username from `section.users`). The status field is always computed.

Write test:

```rust
#[test]
fn preserve_password_hashes_stores_hash() {
    let exec = MockExecutor::new()
        .with_file("/etc/passwd", "alice:x:1000:1000:Alice:/home/alice:/bin/bash\n")
        .with_file("/etc/shadow", "alice:$6$rounds=5000$salt$hash123:19700:0:99999:7:::\n")
        .with_file("/etc/group", "alice:x:1000:\n");

    let inspector = UsersGroupsInspector::with_options(UserGroupOptions {
        preserve_password_hashes: true,
        ..Default::default()
    });
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source, executor: &exec,
        rpm_state: None, baseline_data: None,
    };
    let output = inspector.inspect(&ctx).unwrap();
    if let SectionData::UsersGroups(section) = &output.section {
        assert_eq!(section.users[0]["password_hash"], "$6$rounds=5000$salt$hash123");
        assert_eq!(section.users[0]["password_status"], "password_set");
    } else { panic!("expected UsersGroups"); }
}

#[test]
fn no_preserve_flag_omits_hash() {
    let exec = MockExecutor::new()
        .with_file("/etc/passwd", "alice:x:1000:1000:Alice:/home/alice:/bin/bash\n")
        .with_file("/etc/shadow", "alice:$6$rounds=5000$salt$hash123:19700:0:99999:7:::\n")
        .with_file("/etc/group", "alice:x:1000:\n");

    let inspector = UsersGroupsInspector::new();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source, executor: &exec,
        rpm_state: None, baseline_data: None,
    };
    let output = inspector.inspect(&ctx).unwrap();
    if let SectionData::UsersGroups(section) = &output.section {
        assert!(section.users[0].get("password_hash").is_none(),
            "password_hash must not be present without preserve flag");
    } else { panic!("expected UsersGroups"); }
}
```

- [ ] **Step 8: Implement SSH key content preservation**

Add `preserve_keys: bool` to `collect_ssh_keys`. When true, read full content, parse into individual key lines (skip blank/comment), store as `ssh_keys` array. Always compute `ssh_key_count`.

Write test:

```rust
#[test]
fn preserve_ssh_keys_stores_content() {
    let exec = MockExecutor::new()
        .with_file("/etc/passwd", "alice:x:1000:1000:Alice:/home/alice:/bin/bash\n")
        .with_file("/etc/group", "alice:x:1000:\n")
        .with_file("/home/alice/.ssh/authorized_keys",
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5 alice@work\nssh-rsa AAAAB3NzaC1yc2 alice@laptop\n");

    let inspector = UsersGroupsInspector::with_options(UserGroupOptions {
        preserve_ssh_keys: true,
        ..Default::default()
    });
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source, executor: &exec,
        rpm_state: None, baseline_data: None,
    };
    let output = inspector.inspect(&ctx).unwrap();
    if let SectionData::UsersGroups(section) = &output.section {
        assert_eq!(section.users[0]["ssh_key_count"], 2);
        let keys = section.users[0]["ssh_keys"].as_array().unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys[0].as_str().unwrap().starts_with("ssh-ed25519"));
    } else { panic!("expected UsersGroups"); }
}
```

- [ ] **Step 9: Add `source` field to group entries**

In `parse_group`, add `"source": if gid >= 1000 { "custom" } else { "system" }`.

Write test proving system supplementary groups are captured in user entries:

```rust
#[test]
fn system_supplementary_groups_captured_on_user() {
    let exec = MockExecutor::new()
        .with_file("/etc/passwd", "alice:x:1000:1000:Alice:/home/alice:/bin/bash\n")
        .with_file("/etc/group", "alice:x:1000:\nwheel:x:10:alice\ndocker:x:990:alice\n");

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source, executor: &exec,
        rpm_state: None, baseline_data: None,
    };
    let output = UsersGroupsInspector::new().inspect(&ctx).unwrap();
    if let SectionData::UsersGroups(section) = &output.section {
        let groups = section.users[0]["supplementary_groups"].as_array().unwrap();
        let names: Vec<&str> = groups.iter().filter_map(|g| g.as_str()).collect();
        assert!(names.contains(&"wheel"), "must capture system supplementary group 'wheel'");
        assert!(names.contains(&"docker"), "must capture system supplementary group 'docker'");
    } else { panic!("expected UsersGroups"); }
}
```

- [ ] **Step 10: Run all collector tests**

Run: `cargo test -p inspectah-collect`
Expected: all PASS

- [ ] **Step 11: Commit**

```bash
git add inspectah-collect/
git commit -m "feat(collect): add classification rationale, preserve flags, group source"
```

---

### Task 4: CLI Flags — Scan Command

**Files:**
- Modify: `inspectah-cli/src/commands/scan.rs`

- [ ] **Step 1: Add CLI args**

```rust
#[arg(long)]
preserve_password_hashes: bool,

#[arg(long)]
preserve_ssh_keys: bool,

#[arg(long)]
acknowledge_sensitive: bool,
```

- [ ] **Step 2: Wire flags into inspector options and snapshot metadata**

Pass flags to `UserGroupOptions`. After inspection, set:

```rust
snapshot.sensitive_snapshot = args.preserve_password_hashes || args.preserve_ssh_keys;
snapshot.preserved_credentials = args.preserve_password_hashes;
snapshot.preserved_ssh_keys = args.preserve_ssh_keys;
```

- [ ] **Step 3: Add scan-time export gating**

After `redact()` runs, before writing the tarball: if `snapshot.sensitive_snapshot` is true and `args.acknowledge_sensitive` is false, print a warning to stderr and return an error in non-interactive mode, or prompt for confirmation in interactive mode.

- [ ] **Step 4: Verify compilation**

Run: `cargo build -p inspectah-cli`
Expected: successful build

- [ ] **Step 5: Commit**

```bash
git add inspectah-cli/
git commit -m "feat(cli): add --preserve-password-hashes and --preserve-ssh-keys flags"
```

---

### Task 5: Pipeline — Replace Existing Users Renderer and Add Artifact Renderers

This task replaces the existing `users_section_lines()` function in `containerfile.rs` with the new spec-compliant renderer, and adds standalone KS/TOML renderers. The replacement ensures ONE canonical Containerfile rendering path for preview/export parity.

**Files:**
- Create: `inspectah-pipeline/src/render/users.rs`
- Modify: `inspectah-pipeline/src/render/mod.rs`
- Modify: `inspectah-pipeline/src/render/containerfile.rs`
- Test: `inspectah-pipeline/tests/users_render_test.rs`

- [ ] **Step 1: Write failing test for kickstart renderer with groups before users**

Create `inspectah-pipeline/tests/users_render_test.rs`:

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::users::UserGroupSection;

#[test]
fn kickstart_renders_group_before_user() {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({
            "name": "alice", "uid": 1000, "gid": 1000,
            "shell": "/bin/bash", "home": "/home/alice",
            "supplementary_groups": ["wheel", "docker"]
        })],
        groups: vec![serde_json::json!({
            "name": "alice", "gid": 1000, "source": "custom"
        })],
        ..Default::default()
    });

    let ks = inspectah_pipeline::render::users::render_kickstart(&snap);
    let group_pos = ks.find("group --name=alice").unwrap();
    let user_pos = ks.find("user --name=alice").unwrap();
    assert!(group_pos < user_pos, "group must precede user");
    assert!(ks.contains("--uid=1000"));
    assert!(ks.contains("--gid=1000"));
    assert!(ks.contains("--groups=wheel,docker"));
}
```

- [ ] **Step 2: Create `inspectah-pipeline/src/render/users.rs` with `render_kickstart`**

Implement the kickstart renderer per spec: groups first (custom only), then users with `--uid`, `--gid`, `--groups`, `--iscrypted --password=` (conditional), then `sshkey` lines (conditional). See spec section "Always generated (in tarball)" for exact format.

Register module in `inspectah-pipeline/src/render/mod.rs`: `pub mod users;`

- [ ] **Step 3: Run test**

Run: `cargo test -p inspectah-pipeline kickstart_renders_group`
Expected: PASS

- [ ] **Step 4: Write and implement `render_blueprint_toml`**

Test:
```rust
#[test]
fn toml_renders_group_and_user_blocks() {
    // ... test with groups, users, password, single SSH key
    let toml = inspectah_pipeline::render::users::render_blueprint_toml(&snap);
    assert!(toml.contains("[[customizations.group]]"));
    assert!(toml.contains("[[customizations.user]]"));
}

#[test]
fn toml_multi_key_uses_first_with_comment() {
    // ... user with 2 SSH keys
    let toml = inspectah_pipeline::render::users::render_blueprint_toml(&snap);
    assert!(toml.contains("key = \"ssh-ed25519"));
    assert!(toml.contains("# Additional keys"));
}
```

Implement per spec: `[[customizations.group]]` blocks, then `[[customizations.user]]` blocks. Single `key` field with comment for multi-key users.

- [ ] **Step 5: Write and implement `render_containerfile_users`**

This function produces ONLY the user-materialization portion of the Containerfile. It will be called from the existing `render_containerfile()` in place of the old `users_section_lines()`.

Test:
```rust
#[test]
fn containerfile_useradd_with_groups_and_ssh() {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({
            "name": "alice", "uid": 1000, "gid": 1000,
            "shell": "/bin/bash", "home": "/home/alice",
            "containerfile_strategy": "useradd",
            "supplementary_groups": ["wheel", "devops"],
            "password_hash": "$6$rounds=5000$salt$hash",
            "ssh_keys": ["ssh-ed25519 AAAA alice@work"]
        })],
        groups: vec![
            serde_json::json!({"name": "alice", "gid": 1000, "source": "custom"}),
            serde_json::json!({"name": "devops", "gid": 1005, "source": "custom"}),
        ],
        ..Default::default()
    });

    let cf = inspectah_pipeline::render::users::render_containerfile_users(&snap);
    // Primary group
    assert!(cf.contains("groupadd -g 1000 alice"));
    // Supplementary custom group
    assert!(cf.contains("groupadd -g 1005 devops"));
    assert!(cf.contains("useradd -u 1000"));
    assert!(cf.contains("-G wheel,devops"));
    assert!(cf.contains("chpasswd -e"));
    assert!(cf.contains("install -d -m 700"));
    assert!(cf.contains("COPY users/home/alice/.ssh/authorized_keys"));
}

#[test]
fn containerfile_skip_users_produces_empty() {
    // All users with strategy "skip" → no Containerfile output
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({
            "name": "alice", "uid": 1000, "classification": "interactive"
        })],
        ..Default::default()
    });
    let cf = inspectah_pipeline::render::users::render_containerfile_users(&snap);
    assert!(cf.is_empty(), "skip strategy should produce no output");
}
```

Implement per spec. Key details:
- Materialize ALL custom groups needed by useradd users (primary AND supplementary where `source: "custom"`), not just primary groups
- Use actual GID for `.ssh` ownership: `chown {name}:{gid}` not `chown {name}:{name}` (the user's primary group may have a different name)
- `install -d -m 700 -o {name} -g {gid} {home}/.ssh`

- [ ] **Step 6: Replace `users_section_lines` in `containerfile.rs`**

In `inspectah-pipeline/src/render/containerfile.rs`:
- Remove the existing `users_section_lines()` function (~line 1071)
- Replace the call at ~line 133 (`lines.extend(users_section_lines(snap))`) with:

```rust
let user_cf = users::render_containerfile_users(snap);
if !user_cf.is_empty() {
    lines.push("# --- User accounts (install-time seeding) ---".into());
    lines.extend(user_cf.lines().map(|l| l.to_string()));
}
```

This ensures ONE canonical Containerfile rendering path. The `render_containerfile()` function remains the single entry point for both preview and export, preserving preview/export parity.

- [ ] **Step 7: Implement `stage_ssh_keys`**

```rust
pub fn stage_ssh_keys(
    snap: &InspectionSnapshot,
    output_dir: &std::path::Path,
) -> std::io::Result<()> {
    let ug = match &snap.users_groups {
        Some(ug) => ug,
        None => return Ok(()),
    };

    for user in &ug.users {
        let name = match user.get("name").and_then(|n| n.as_str()) {
            Some(n) => n,
            None => continue,
        };
        let keys = match user.get("ssh_keys").and_then(|k| k.as_array()) {
            Some(k) if !k.is_empty() => k,
            _ => continue,
        };

        let ssh_dir = output_dir.join(format!("users/home/{name}/.ssh"));
        std::fs::create_dir_all(&ssh_dir)?;

        let content: String = keys
            .iter()
            .filter_map(|k| k.as_str())
            .collect::<Vec<&str>>()
            .join("\n") + "\n";

        std::fs::write(ssh_dir.join("authorized_keys"), content)?;
    }

    Ok(())
}
```

- [ ] **Step 8: Wire user artifacts into scan-time `render_all`**

In `inspectah-pipeline/src/render/mod.rs`, in `render_all()`, after the existing kickstart write (~step 6):

```rust
// 6b. inspectah-users.ks
let users_ks = users::render_kickstart(snap);
if !users_ks.trim().is_empty() {
    std::fs::write(output_dir.join("inspectah-users.ks"), &users_ks)?;
}

// 6c. inspectah-users.toml
let users_toml = users::render_blueprint_toml(snap);
if !users_toml.trim().is_empty() {
    std::fs::write(output_dir.join("inspectah-users.toml"), &users_toml)?;
}

// 6d. Stage SSH authorized_keys files
users::stage_ssh_keys(snap, output_dir)
    .map_err(|e| RenderError::Failed(format!("stage SSH keys: {e}")))?;
```

This ensures user artifacts are generated at SCAN time (in the main tarball), not just at refine export time.

- [ ] **Step 9: Run all pipeline tests**

Run: `cargo test -p inspectah-pipeline`
Expected: all PASS

- [ ] **Step 10: Commit**

```bash
git add inspectah-pipeline/
git commit -m "feat(pipeline): replace users_section_lines with spec-compliant renderers"
```

---

### Task 6: Refine — User Decision Ops, Sensitivity, and Export Wiring

**Files:**
- Modify: `inspectah-refine/src/types.rs`
- Modify: `inspectah-refine/src/session.rs`
- Modify: `inspectah-refine/tests/export_contract_test.rs`

- [ ] **Step 1: Add `UserStrategy` and `UserPassword` to `RefinementOp`**

In `inspectah-refine/src/types.rs`:

```rust
use inspectah_core::types::users::{UserContainerfileStrategy, UserPasswordChoice};

// Add to enum RefinementOp:
UserStrategy { username: String, strategy: UserContainerfileStrategy },
UserPassword { username: String, password: UserPasswordChoice, hash: Option<String> },
```

- [ ] **Step 2: Handle new ops in `validate_target`**

```rust
RefinementOp::UserStrategy { username, .. }
| RefinementOp::UserPassword { username, .. } => {
    let found = self.original.users_groups.as_ref()
        .map(|ug| ug.users.iter().any(|u|
            u.get("name").and_then(|n| n.as_str()) == Some(username)
        ))
        .unwrap_or(false);
    if !found {
        return Err(RefineError::UnknownTarget(username.clone()));
    }
}
```

- [ ] **Step 3: Handle new ops in `project_snapshot`**

Project user decisions into snapshot fields. For `UserPassword` with `password: None`, explicitly clear `password_hash` from the user entry:

```rust
RefinementOp::UserStrategy { username, strategy } => {
    if let Some(ug) = &mut projected.users_groups {
        for user in &mut ug.users {
            if user.get("name").and_then(|n| n.as_str()) == Some(username) {
                user["containerfile_strategy"] = serde_json::to_value(strategy).unwrap();
            }
        }
    }
}
RefinementOp::UserPassword { username, password, hash } => {
    if let Some(ug) = &mut projected.users_groups {
        for user in &mut ug.users {
            if user.get("name").and_then(|n| n.as_str()) == Some(username) {
                user["password_choice"] = serde_json::to_value(password).unwrap();
                match password {
                    UserPasswordChoice::New => {
                        if let Some(h) = hash {
                            user["password_hash"] = serde_json::Value::String(h.clone());
                        }
                    }
                    UserPasswordChoice::None => {
                        user.as_object_mut().map(|m| m.remove("password_hash"));
                    }
                    UserPasswordChoice::Preserve => {
                        // Restore the ORIGINAL scan-time hash from self.original,
                        // not whatever is currently in the projected state.
                        // This handles the New -> Preserve sequence correctly:
                        // New sets a new hash, Preserve must restore the original.
                        let original_hash = self.original.users_groups.as_ref()
                            .and_then(|ug| ug.users.iter().find(|u|
                                u.get("name").and_then(|n| n.as_str()) == Some(username)
                            ))
                            .and_then(|u| u.get("password_hash"))
                            .cloned();
                        match original_hash {
                            Some(h) => { user["password_hash"] = h; }
                            None => {
                                // No preserved hash exists — this is a no-op.
                                // The user entry keeps whatever password_status it had.
                            }
                        }
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 3b: Write test for New -> Preserve restoring original hash**

```rust
#[test]
fn preserve_after_new_restores_original_hash() {
    let mut snap = InspectionSnapshot::new();
    snap.sensitive_snapshot = true;
    snap.preserved_credentials = true;
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({
            "name": "alice", "uid": 1000,
            "password_hash": "$6$original$hash",
            "password_status": "password_set"
        })],
        ..Default::default()
    });
    let mut session = RefineSession::new(snap);

    // Set a new password
    session.apply(RefinementOp::UserPassword {
        username: "alice".into(),
        password: UserPasswordChoice::New,
        hash: Some("$6$new$hash".into()),
    }).unwrap();
    let proj = session.snapshot_projected();
    assert_eq!(proj.users_groups.as_ref().unwrap().users[0]["password_hash"], "$6$new$hash");

    // Switch back to preserve — must restore original, not keep the new one
    session.apply(RefinementOp::UserPassword {
        username: "alice".into(),
        password: UserPasswordChoice::Preserve,
        hash: None,
    }).unwrap();
    let proj = session.snapshot_projected();
    assert_eq!(proj.users_groups.as_ref().unwrap().users[0]["password_hash"], "$6$original$hash",
        "Preserve must restore the original scan-time hash, not keep the New hash");
}
```

- [ ] **Step 4: Implement `is_sensitive` based on projected state**

Sensitivity is determined by the PROJECTED snapshot state, not op history:

```rust
pub fn is_sensitive(&self) -> bool {
    let projected = self.project_snapshot();
    if projected.sensitive_snapshot {
        return true;
    }
    // Check if any user has a password_hash from a NewPassword decision
    projected.users_groups.as_ref()
        .map(|ug| ug.users.iter().any(|u| {
            u.get("password_choice").and_then(|c| c.as_str()) == Some("new")
                && u.get("password_hash").and_then(|h| h.as_str()).is_some()
        }))
        .unwrap_or(false)
}
```

- [ ] **Step 5: Write tests for user ops**

```rust
#[test]
fn user_strategy_op_projects_useradd() {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({"name": "alice", "uid": 1000})],
        ..Default::default()
    });
    let mut session = RefineSession::new(snap);
    session.apply(RefinementOp::UserStrategy {
        username: "alice".into(),
        strategy: UserContainerfileStrategy::Useradd,
    }).unwrap();
    let proj = session.snapshot_projected();
    assert_eq!(proj.users_groups.unwrap().users[0]["containerfile_strategy"], "useradd");
}

#[test]
fn user_password_none_clears_hash() {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({
            "name": "alice", "uid": 1000,
            "password_hash": "$6$existing$hash"
        })],
        ..Default::default()
    });
    let mut session = RefineSession::new(snap);
    session.apply(RefinementOp::UserPassword {
        username: "alice".into(),
        password: UserPasswordChoice::None,
        hash: None,
    }).unwrap();
    let proj = session.snapshot_projected();
    assert!(proj.users_groups.unwrap().users[0].get("password_hash").is_none(),
        "None choice must clear password_hash");
}

#[test]
fn new_password_triggers_sensitive_on_projected_state() {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({"name": "alice", "uid": 1000})],
        ..Default::default()
    });
    let mut session = RefineSession::new(snap);
    assert!(!session.is_sensitive());
    session.apply(RefinementOp::UserPassword {
        username: "alice".into(),
        password: UserPasswordChoice::New,
        hash: Some("$6$salt$hash".into()),
    }).unwrap();
    assert!(session.is_sensitive());
}
```

- [ ] **Step 6: Wire user artifacts into `render_refine_export`**

In `render_refine_export`, add user artifacts after Containerfile (which now includes user materialization via the replaced `users_section_lines`):

```rust
// User provisioning artifacts
let users_ks = inspectah_pipeline::render::users::render_kickstart(snap);
if !users_ks.trim().is_empty() {
    std::fs::write(out.join("inspectah-users.ks"), &users_ks)?;
}
let users_toml = inspectah_pipeline::render::users::render_blueprint_toml(snap);
if !users_toml.trim().is_empty() {
    std::fs::write(out.join("inspectah-users.toml"), &users_toml)?;
}
inspectah_pipeline::render::users::stage_ssh_keys(snap, out)
    .map_err(|e| RefineError::RenderFailed(e.to_string()))?;
```

Add `"inspectah-users.ks"`, `"inspectah-users.toml"`, and `"users"` to `allowed_top_level`.

- [ ] **Step 7: Update export contract test**

Add test verifying new files appear in the tarball file set:

```rust
#[test]
fn export_includes_user_artifacts() {
    let mut snap = test_snapshot();
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({
            "name": "alice", "uid": 1000, "gid": 1000,
            "shell": "/bin/bash", "home": "/home/alice",
            "supplementary_groups": []
        })],
        groups: vec![serde_json::json!({"name": "alice", "gid": 1000, "source": "custom"})],
        ..Default::default()
    });
    let session = RefineSession::new(snap);
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("out.tar.gz");
    session.export_tarball(&path, session.generation()).unwrap();
    let files = tarball_file_set(&path);
    assert!(files.contains("inspectah-users.ks"));
    assert!(files.contains("inspectah-users.toml"));
}
```

- [ ] **Step 8: Run all refine tests**

Run: `cargo test -p inspectah-refine`
Expected: all PASS

- [ ] **Step 9: Commit**

```bash
git add inspectah-refine/
git commit -m "feat(refine): add UserStrategy/UserPassword ops with projected-state sensitivity"
```

---

### Task 7: Web API — User Endpoints and Export Gating

**Files:**
- Modify: `inspectah-web/src/handlers.rs`
- Modify: `inspectah-web/src/lib.rs`

- [ ] **Step 1: Add request types**

```rust
#[derive(Deserialize)]
pub struct UserStrategyRequest {
    pub username: String,
    pub strategy: String,
}

#[derive(Deserialize)]
pub struct UserPasswordRequest {
    pub username: String,
    pub choice: String,
    pub hash: Option<String>,
}
```

- [ ] **Step 2: Implement `user_strategy` handler**

Parse strategy string to `UserContainerfileStrategy`, apply op, return `ViewResponse`.

- [ ] **Step 3: Implement `user_password` handler**

Parse choice string to `UserPasswordChoice`, apply op, return `ViewResponse`.

- [ ] **Step 4: Implement `user_preview` handler**

Return rendered KS and TOML from projected snapshot.

- [ ] **Step 5: Add `users_groups_decisions` to `ViewResponse`**

```rust
pub users_groups_decisions: Vec<serde_json::Value>,
```

Populate from `session.snapshot_projected().users_groups.map(|ug| ug.users)`.

- [ ] **Step 6: Add export gating to `export_tarball` handler**

Extract `x-acknowledge-sensitive` header. If `session.is_sensitive()` and header is not `"true"`, return HTTP 428 with sensitivity summary.

- [ ] **Step 7: Remove `normalize_users_groups` from `sections_cache`**

In `get_sections`, remove `normalize_users_groups(snap)` from the `sections_cache.get_or_init()` closure (~line 411). The data now comes through `ViewResponse.users_groups_decisions`.

- [ ] **Step 8: Register routes**

In `inspectah-web/src/lib.rs`:

```rust
.route("/api/user-strategy", post(handlers::user_strategy))
.route("/api/user-password", post(handlers::user_password))
.route("/api/user-preview", get(handlers::user_preview))
```

- [ ] **Step 9: Verify compilation**

Run: `cargo build -p inspectah-web`
Expected: successful build

- [ ] **Step 10: Commit**

```bash
git add inspectah-web/src/
git commit -m "feat(web): add user strategy/password/preview endpoints and export gating"
```

---

### Task 8: Web UI — Users & Groups Decision Section

This task creates the React components. The real file owners after `users_groups` leaves the cached context path are listed explicitly.

**Files:**
- Create: `inspectah-web/ui/src/components/UsersGroupsSection.tsx`
- Create: `inspectah-web/ui/src/components/UserCard.tsx`
- Create: `inspectah-web/ui/src/components/UserArtifactPreview.tsx`
- Modify: `inspectah-web/ui/src/App.tsx` — active section routing for `users_groups`
- Modify: `inspectah-web/ui/src/components/Sidebar.tsx` — move to Decisions group
- Modify: `inspectah-web/ui/src/components/MainContent.tsx` — render decision section
- Modify: `inspectah-web/ui/src/components/GlobalSearch.tsx` — include user items in search
- Modify: `inspectah-web/ui/src/components/ExportDialog.tsx` — sensitivity gating with acknowledgment
- Modify: `inspectah-web/ui/src/components/ContainerfilePanel.tsx` — default-redaction for sensitive values in live preview
- Modify: `inspectah-web/ui/src/hooks/useKeyboard.ts` — register shortcut for Users section
- Modify: `inspectah-web/ui/src/api/client.ts` — API methods
- Modify: `inspectah-web/ui/src/api/types.ts` — type definitions for user decisions

- [ ] **Step 1: Add TypeScript types in `api/types.ts`**

```typescript
export interface UserDecision {
  name: string;
  uid: number;
  gid: number;
  shell: string;
  home: string;
  classification: string;
  classification_rationale: string;
  password_status: string;
  password_hash?: string;
  password_choice?: string;
  containerfile_strategy?: string;
  ssh_key_count: number;
  ssh_keys?: string[];
  has_sudo: boolean;
  has_subuid: boolean;
  supplementary_groups: string[];
}
```

Add `users_groups_decisions: UserDecision[]` and `session_is_sensitive: boolean` to the `ViewResponse` type.

- [ ] **Step 2: Add API client methods**

In `api/client.ts`:

```typescript
export async function setUserStrategy(username: string, strategy: string) { ... }
export async function setUserPassword(username: string, choice: string, hash?: string) { ... }
export async function fetchUserPreview(): Promise<{ kickstart: string; blueprint_toml: string }> { ... }
```

- [ ] **Step 3: Create `UserCard.tsx` component**

Renders a single user card with:
- Name + UID header
- Badges: sudo (amber bg), SSH (blue/gray with count + captured/detected), subuid (teal)
- Details line: shell, home, supplementary groups
- Classification rationale text
- Strategy radio group (Skip / useradd) — onChange calls `setUserStrategy`
- Collapsible password options
- Collapsible SSH key detail (fingerprint display, reveal toggle for full key)
- Non-interactive cards get muted style + "Review recommended" tag
- Sensitive values (password_hash, full SSH key text) redacted by default with per-value reveal toggles

- [ ] **Step 4: Create `UsersGroupsSection.tsx`**

Section wrapper with:
- Header: "Users & Groups" title
- Banner text about KS/TOML always generated
- "Preview Artifacts" button
- Sensitivity banner when `session_is_sensitive` is true
- Maps `users_groups_decisions` to `UserCard` components
- Empty state when no users

- [ ] **Step 5: Create `UserArtifactPreview.tsx`**

Modal/drawer showing rendered KS and TOML from `/api/user-preview`. Tabbed display. Read-only monospace. Sensitive values redacted by default with reveal toggles matching the card behavior.

- [ ] **Step 6: Update `Sidebar.tsx`**

Move `users_groups` from the Context section list to the Decisions section list. Verify section ID and display name.

- [ ] **Step 7: Update `MainContent.tsx`**

Add `UsersGroupsSection` rendering when `activeSection === "users_groups"`.

- [ ] **Step 8: Update `App.tsx`**

Ensure `users_groups` is handled in the active section routing. The section should be navigable from sidebar click and keyboard shortcut.

- [ ] **Step 9: Update `GlobalSearch.tsx`**

Include user entries from `users_groups_decisions` in search results. Match on username, UID, shell, groups.

- [ ] **Step 10: Update `useKeyboard.ts`**

Register a keyboard shortcut to navigate to the Users & Groups section (following the pattern used by other decision sections).

- [ ] **Step 11: Update `ExportDialog.tsx`**

Check if session is sensitive. If so:
- Show sensitivity banner listing what sensitive content exists
- Require acknowledgment checkbox before enabling export button
- Pass `X-Acknowledge-Sensitive: true` header on export request
- Handle HTTP 428 response from the tarball endpoint

- [ ] **Step 12: Update `ContainerfilePanel.tsx` — default-redaction for sensitive values**

The existing Containerfile preview (`containerfile_preview` in `RefinedView`) is rendered by `render_containerfile()` in `inspectah-refine/src/session.rs::recompute_view()`. When a useradd user has a preserved or new password hash, the preview will contain the raw `chpasswd -e` line with the hash visible.

**Session-level sensitivity signal:** The redaction trigger must be `session_is_sensitive` (the computed flag from Task 6's `is_sensitive()`), NOT `sensitive_snapshot` (which is scan-time only). A non-sensitive scan that becomes sensitive because the user sets a new password in refine must still trigger redaction. The signal flows through the mutable view/API path:

1. Add `session_is_sensitive: bool` to `ViewResponse` (in `inspectah-web/src/handlers.rs`), populated from `session.is_sensitive()` on every view response.
2. `App.tsx` reads `session_is_sensitive` from the view hook and passes it to `ContainerfilePanel`, `UsersGroupsSection` (for the sensitivity banner), and `ExportDialog`.
3. `ContainerfilePanel.tsx` uses `session_is_sensitive` (not `sensitive_snapshot`) to activate redaction.

The Containerfile panel applies default-redaction when `session_is_sensitive` is true:
- Scan the rendered `containerfile_preview` string for `crypt(3)` hash patterns (`$6$...`, `$y$...`, `$5$...`) in `chpasswd -e` lines
- Replace with a redacted placeholder (e.g., `$6$<REDACTED>`) by default
- Add a reveal toggle (consistent with user card reveal pattern) that shows the actual hash
- When `session_is_sensitive` is false, the panel renders unchanged

This is a UI-only transform — the underlying `containerfile_preview` string from the API is unchanged. The redaction happens in the React component at render time.

- [ ] **Step 13: Manual testing in browser**

Start dev server: `cargo run -p inspectah-cli -- refine <test-tarball>`
Verify:
- Users & Groups appears in Decisions sidebar group
- User cards render with correct data, badges, rationale
- Strategy selector works
- Password/SSH expandable sections work
- Artifact preview modal shows KS and TOML
- Sensitivity banner appears for sensitive snapshots
- Containerfile preview redacts password hashes by default, reveal toggle works
- Export dialog shows acknowledgment for sensitive sessions
- GlobalSearch finds users
- Keyboard shortcut navigates to section

- [ ] **Step 14: Commit**

```bash
git add inspectah-web/ui/src/
git commit -m "feat(web): add Users & Groups decision section with cards, preview, and export gating"
```

---

### Task 9: Integration — End-to-End Smoke Test

**Files:**
- Create or modify: `inspectah-refine/tests/users_integration_test.rs`

- [ ] **Step 1: Write e2e test for full pipeline**

```rust
#[test]
fn full_pipeline_users_groups_materialization() {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({
            "name": "alice", "uid": 1000, "gid": 1000,
            "shell": "/bin/bash", "home": "/home/alice",
            "classification": "interactive",
            "classification_rationale": "bash shell, home at /home/alice",
            "password_status": "password_set",
            "password_hash": "$6$rounds=5000$testsalt$testhash",
            "ssh_key_count": 1,
            "ssh_keys": ["ssh-ed25519 AAAAC3NzaC1lZDI1NTE5 alice@work"],
            "has_sudo": true, "has_subuid": false,
            "supplementary_groups": ["wheel"]
        })],
        groups: vec![serde_json::json!({
            "name": "alice", "gid": 1000, "source": "custom"
        })],
        ..Default::default()
    });
    snap.sensitive_snapshot = true;
    snap.preserved_credentials = true;
    snap.preserved_ssh_keys = true;
    snap.redaction_state = Some(RedactionState::SensitiveRetained {
        redacted_by: "inspectah 0.8.0".into(),
        config_hash: "abc".into(),
        unresolved_count: 0,
        unresolved_hints: vec![],
    });

    // Apply useradd strategy
    let mut session = RefineSession::new(snap);
    session.apply(RefinementOp::UserStrategy {
        username: "alice".into(),
        strategy: UserContainerfileStrategy::Useradd,
    }).unwrap();

    assert!(session.is_sensitive());

    // Export
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("output.tar.gz");
    session.export_tarball(&path, session.generation()).unwrap();

    // Verify tarball contents
    let files = tarball_file_set(&path);
    assert!(files.contains("inspectah-users.ks"));
    assert!(files.contains("inspectah-users.toml"));
    assert!(files.contains("users/home/alice/.ssh/authorized_keys"));
    assert!(files.contains("Containerfile"));

    // Verify Containerfile contains user materialization
    let cf = read_tarball_file(&path, "Containerfile");
    assert!(cf.contains("groupadd -g 1000 alice"));
    assert!(cf.contains("useradd -u 1000"));
    assert!(cf.contains("chpasswd -e"));
    assert!(cf.contains("COPY users/home/alice/.ssh/authorized_keys"));
    assert!(cf.contains("install -d -m 700"));

    // Verify KS contains groups before users
    let ks = read_tarball_file(&path, "inspectah-users.ks");
    assert!(ks.find("group --name=alice").unwrap() < ks.find("user --name=alice").unwrap());

    // Verify re-import preserves projected state
    let reimported = from_tarball(&path).unwrap();
    let proj_user = &reimported.snapshot().users_groups.as_ref().unwrap().users[0];
    assert_eq!(proj_user["containerfile_strategy"], "useradd");
}
```

- [ ] **Step 2: Run the integration test**

Run: `cargo test -p inspectah-refine full_pipeline_users`
Expected: PASS

- [ ] **Step 3: Write preview/export parity proof**

Verify that preview API output matches exported tarball content for all user artifacts:

```rust
#[test]
fn preview_export_parity_for_user_artifacts() {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({
            "name": "alice", "uid": 1000, "gid": 1000,
            "shell": "/bin/bash", "home": "/home/alice",
            "classification": "interactive",
            "containerfile_strategy": "useradd",
            "password_hash": "$6$rounds=5000$salt$hash",
            "supplementary_groups": ["wheel"],
            "ssh_keys": ["ssh-ed25519 AAAA alice@work"]
        })],
        groups: vec![serde_json::json!({"name": "alice", "gid": 1000, "source": "custom"})],
        ..Default::default()
    });
    snap.sensitive_snapshot = true;
    snap.preserved_credentials = true;
    snap.preserved_ssh_keys = true;
    snap.redaction_state = Some(RedactionState::SensitiveRetained {
        redacted_by: "inspectah 0.8.0".into(),
        config_hash: "abc".into(),
        unresolved_count: 0, unresolved_hints: vec![],
    });

    let mut session = RefineSession::new(snap);
    session.apply(RefinementOp::UserStrategy {
        username: "alice".into(),
        strategy: UserContainerfileStrategy::Useradd,
    }).unwrap();

    // Preview output — read from the LIVE refine view, the same path
    // the web handler reads. This is the actual preview seam, not a
    // renderer shortcut.
    let view = session.view();
    let preview_cf = view.containerfile_preview.clone();

    // User artifact preview — rendered from the projected snapshot,
    // same as the /api/user-preview handler.
    let projected = session.snapshot_projected();
    let preview_ks = inspectah_pipeline::render::users::render_kickstart(&projected);
    let preview_toml = inspectah_pipeline::render::users::render_blueprint_toml(&projected);

    // Export output (what goes in the tarball)
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("out.tar.gz");
    session.export_tarball(&path, session.generation()).unwrap();
    let export_ks = read_tarball_file(&path, "inspectah-users.ks");
    let export_toml = read_tarball_file(&path, "inspectah-users.toml");
    let export_cf = read_tarball_file(&path, "Containerfile");

    assert_eq!(preview_ks, export_ks,
        "inspectah-users.ks preview must match export");
    assert_eq!(preview_toml, export_toml,
        "inspectah-users.toml preview must match export");
    assert_eq!(preview_cf, export_cf,
        "Containerfile live preview must match export");
}
```

- [ ] **Step 4: Run full workspace test suite**

Run: `cargo test --workspace`
Expected: all PASS

- [ ] **Step 5: Commit**

```bash
git add inspectah-refine/tests/
git commit -m "test: add end-to-end integration test for user/group materialization"
```
