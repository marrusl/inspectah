# User/Group Materialization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Generate kickstart, blueprint TOML, and Containerfile artifacts for migrating local human user accounts from package-mode RHEL to image-mode.

**Architecture:** Layered additions across the workspace: core types define the data model, the collector enriches scan-time data, the refine layer adds user decision ops, the pipeline renders output artifacts, and the web layer exposes the decision UI. Each layer builds on the previous — implement in order.

**Tech Stack:** Rust workspace (inspectah-core, inspectah-collect, inspectah-refine, inspectah-pipeline, inspectah-web), Axum web framework, React + TypeScript UI.

**Spec:** `docs/specs/proposed/2026-05-18-user-group-materialization-design.md`

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

In `inspectah-core/src/types/users.rs`, add a new test:

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
    fn default() -> Self {
        Self::Skip
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserPasswordChoice {
    None,
    Preserve,
    New,
}

impl Default for UserPasswordChoice {
    fn default() -> Self {
        Self::None
    }
}
```

- [ ] **Step 7: Run tests to verify both pass**

Run: `cargo test -p inspectah-core -- users`
Expected: PASS for new enum tests and existing `UserGroupSection` roundtrip

- [ ] **Step 8: Add sensitive snapshot metadata fields to snapshot**

In `inspectah-core/src/snapshot.rs`, add to `InspectionSnapshot`:

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
Expected: all tests PASS (including parity gate — new fields have `#[serde(default)]`)

- [ ] **Step 10: Commit**

```bash
git add inspectah-core/src/types/redaction.rs inspectah-core/src/types/users.rs inspectah-core/src/snapshot.rs
git commit -m "feat(core): add SensitiveRetained state and user decision enums"
```

---

### Task 2: Collector — Classification, Preserve Flags, Group Source

**Files:**
- Modify: `inspectah-collect/src/inspectors/users.rs`
- Test: `inspectah-collect/src/inspectors/users.rs` (inline tests)
- Test: `inspectah-collect/tests/users_test.rs`

- [ ] **Step 1: Write failing test for classification rationale**

In the inline test module of `inspectah-collect/src/inspectors/users.rs`:

```rust
#[test]
fn classification_rationale_interactive() {
    let user = serde_json::json!({
        "name": "alice",
        "uid": 1000,
        "gid": 1000,
        "shell": "/bin/bash",
        "home": "/home/alice"
    });
    let rationale = build_classification_rationale(&user);
    assert!(rationale.contains("bash shell"));
    assert!(rationale.contains("/home/alice"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-collect classification_rationale`
Expected: FAIL — `build_classification_rationale` not defined

- [ ] **Step 3: Implement `build_classification_rationale`**

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

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p inspectah-collect classification_rationale`
Expected: PASS

- [ ] **Step 5: Wire classification and rationale into `RunUsersGroups`**

After the existing `classify_user` call in the main `inspect` method, add:

```rust
u["classification_rationale"] = serde_json::Value::String(
    build_classification_rationale(&u)
);
```

Also add to each user entry during `parse_passwd`:
```rust
"has_sudo": false,
"has_subuid": false,
"supplementary_groups": [],
```

After `parseSudoers`, set `has_sudo` on matching users. After `parse_subid_file` for subuid, set `has_subuid`. After group parsing, populate `supplementary_groups`.

- [ ] **Step 6: Write failing test for `--preserve-password-hashes`**

```rust
#[test]
fn preserve_password_hashes_stores_hash() {
    let exec = MockExecutor::new()
        .with_file("/etc/passwd", "alice:x:1000:1000:Alice:/home/alice:/bin/bash\n")
        .with_file("/etc/shadow", "alice:$6$rounds=5000$salt$hash123:19700:0:99999:7:::\n")
        .with_file("/etc/group", "alice:x:1000:\n");

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };
    let inspector = UsersGroupsInspector::with_options(UserGroupOptions {
        strategy_override: None,
        preserve_password_hashes: true,
        preserve_ssh_keys: false,
    });
    let output = inspector.inspect(&ctx).unwrap();

    if let SectionData::UsersGroups(section) = &output.section {
        let user = &section.users[0];
        assert_eq!(user["password_hash"], "$6$rounds=5000$salt$hash123");
        assert_eq!(user["password_status"], "password_set");
    } else {
        panic!("expected UsersGroups");
    }
}
```

- [ ] **Step 7: Add `preserve_password_hashes` and `preserve_ssh_keys` to `UserGroupOptions`**

```rust
#[derive(Debug, Clone, Default)]
pub struct UserGroupOptions {
    pub strategy_override: Option<String>,
    pub preserve_password_hashes: bool,
    pub preserve_ssh_keys: bool,
}
```

- [ ] **Step 8: Implement password hash preservation in `parse_shadow`**

Add a `preserve_hashes: bool` parameter to `parse_shadow`. When true, store the raw hash in a `password_hash` field on matching user entries (looked up by username from `section.users`). The existing status field (`locked`/`disabled`/`password_set`/`no_password`) is always computed regardless.

After calling `parse_shadow`, if `preserve_hashes` is true, add a `RedactionHint` for each user with `password_status == "password_set"`.

- [ ] **Step 9: Run preserve test to verify it passes**

Run: `cargo test -p inspectah-collect preserve_password_hashes`
Expected: PASS

- [ ] **Step 10: Write failing test for `--preserve-ssh-keys`**

```rust
#[test]
fn preserve_ssh_keys_stores_content() {
    let exec = MockExecutor::new()
        .with_file("/etc/passwd", "alice:x:1000:1000:Alice:/home/alice:/bin/bash\n")
        .with_file("/etc/group", "alice:x:1000:\n")
        .with_file(
            "/home/alice/.ssh/authorized_keys",
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5 alice@work\nssh-rsa AAAAB3NzaC1yc2 alice@laptop\n",
        );

    let inspector = UsersGroupsInspector::with_options(UserGroupOptions {
        preserve_ssh_keys: true,
        ..Default::default()
    });
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };
    let output = inspector.inspect(&ctx).unwrap();

    if let SectionData::UsersGroups(section) = &output.section {
        let user = &section.users[0];
        assert_eq!(user["ssh_key_count"], 2);
        let keys = user["ssh_keys"].as_array().unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys[0].as_str().unwrap().starts_with("ssh-ed25519"));
    } else {
        panic!("expected UsersGroups");
    }
}
```

- [ ] **Step 11: Implement SSH key content preservation**

Modify `collect_ssh_keys` to accept `preserve_keys: bool`. When true, read the full content of `authorized_keys`, parse into individual key lines (skip blank lines and comments), store as `ssh_keys` array on the user entry. Always compute `ssh_key_count` from the stored keys or line count.

- [ ] **Step 12: Run SSH key test to verify it passes**

Run: `cargo test -p inspectah-collect preserve_ssh_keys`
Expected: PASS

- [ ] **Step 13: Add `source` field to group entries**

In `parse_group`, add to each group entry:
```rust
"source": if gid >= 1000 { "custom" } else { "system" }
```

Write a test:
```rust
#[test]
fn group_source_custom_vs_system() {
    let text = "wheel:x:10:\nalice:x:1000:alice\n";
    let mut section = UserGroupSection::default();
    let mut non_system = HashMap::new();
    parse_group(text, &mut section, &mut non_system);
    // wheel is GID 10 = system, filtered out by GID range
    // alice is GID 1000 = custom
    assert_eq!(section.groups[0]["source"], "custom");
}
```

- [ ] **Step 14: Run all collector tests**

Run: `cargo test -p inspectah-collect`
Expected: all PASS

- [ ] **Step 15: Commit**

```bash
git add inspectah-collect/src/inspectors/users.rs
git commit -m "feat(collect): add classification rationale, preserve flags, group source"
```

---

### Task 3: CLI Flags — Scan Command

**Files:**
- Modify: `inspectah-cli/src/commands/scan.rs`

- [ ] **Step 1: Add `--preserve-password-hashes` and `--preserve-ssh-keys` args**

In the `ScanArgs` struct:

```rust
#[arg(long)]
preserve_password_hashes: bool,

#[arg(long)]
preserve_ssh_keys: bool,

#[arg(long)]
acknowledge_sensitive: bool,
```

- [ ] **Step 2: Wire flags into the inspector options**

In `run_scan`, pass the flags through to `UserGroupOptions` when constructing the inspector. Also set `snapshot.sensitive_snapshot`, `snapshot.preserved_credentials`, and `snapshot.preserved_ssh_keys` based on the flags.

- [ ] **Step 3: Add export gating**

After rendering the snapshot, before writing the tarball, check if `sensitive_snapshot` is true. If so and `acknowledge_sensitive` is false, print a warning and prompt for confirmation (or error in non-interactive mode).

- [ ] **Step 4: Run `cargo build -p inspectah-cli` to verify compilation**

Expected: successful build

- [ ] **Step 5: Commit**

```bash
git add inspectah-cli/src/commands/scan.rs
git commit -m "feat(cli): add --preserve-password-hashes and --preserve-ssh-keys flags"
```

---

### Task 4: Refine — User Decision Ops and Sensitivity

**Files:**
- Modify: `inspectah-refine/src/types.rs`
- Modify: `inspectah-refine/src/session.rs`
- Test: `inspectah-refine/src/session.rs` (inline tests)

- [ ] **Step 1: Write failing test for `UserStrategy` op**

In the test module of `inspectah-refine/src/session.rs`:

```rust
#[test]
fn user_strategy_op_sets_useradd() {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({
            "name": "alice", "uid": 1000, "gid": 1000,
            "shell": "/bin/bash", "home": "/home/alice",
            "classification": "interactive"
        })],
        ..Default::default()
    });
    let mut session = RefineSession::new(snap);
    session
        .apply(RefinementOp::UserStrategy {
            username: "alice".into(),
            strategy: UserContainerfileStrategy::Useradd,
        })
        .unwrap();

    let projected = session.snapshot_projected();
    let user = &projected.users_groups.unwrap().users[0];
    assert_eq!(user["containerfile_strategy"], "useradd");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-refine user_strategy_op`
Expected: FAIL — `UserStrategy` variant doesn't exist

- [ ] **Step 3: Add `UserStrategy` and `UserPassword` to `RefinementOp`**

In `inspectah-refine/src/types.rs`:

```rust
use inspectah_core::types::users::{UserContainerfileStrategy, UserPasswordChoice};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", content = "target")]
pub enum RefinementOp {
    ExcludePackage(PackageTarget),
    IncludePackage(PackageTarget),
    ExcludeConfig { path: PathBuf },
    IncludeConfig { path: PathBuf },
    ExcludeRepo { section_id: String },
    IncludeRepo { section_id: String },
    UserStrategy { username: String, strategy: UserContainerfileStrategy },
    UserPassword { username: String, password: UserPasswordChoice, hash: Option<String> },
}
```

- [ ] **Step 4: Handle new ops in `validate_target`**

In `session.rs`, add match arms:

```rust
RefinementOp::UserStrategy { username, .. }
| RefinementOp::UserPassword { username, .. } => {
    let found = self
        .original
        .users_groups
        .as_ref()
        .map(|ug| ug.users.iter().any(|u| {
            u.get("name").and_then(|n| n.as_str()) == Some(username)
        }))
        .unwrap_or(false);
    if !found {
        return Err(RefineError::UnknownTarget(username.clone()));
    }
}
```

- [ ] **Step 5: Handle new ops in `project_snapshot`**

In `project_snapshot`, add projection for user ops:

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
                if let Some(h) = hash {
                    user["password_hash"] = serde_json::Value::String(h.clone());
                }
            }
        }
    }
}
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test -p inspectah-refine user_strategy_op`
Expected: PASS

- [ ] **Step 7: Write test for `session_is_sensitive` with `NewPassword`**

```rust
#[test]
fn new_password_triggers_sensitive_session() {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({
            "name": "alice", "uid": 1000, "shell": "/bin/bash"
        })],
        ..Default::default()
    });
    let mut session = RefineSession::new(snap);

    assert!(!session.is_sensitive());

    session
        .apply(RefinementOp::UserPassword {
            username: "alice".into(),
            password: UserPasswordChoice::New,
            hash: Some("$6$rounds=5000$salt$hash".into()),
        })
        .unwrap();

    assert!(session.is_sensitive());
}
```

- [ ] **Step 8: Implement `is_sensitive` on `RefineSession`**

```rust
pub fn is_sensitive(&self) -> bool {
    if self.original.sensitive_snapshot {
        return true;
    }
    self.ops[..self.cursor].iter().any(|op| {
        matches!(op, RefinementOp::UserPassword {
            password: UserPasswordChoice::New, ..
        })
    })
}
```

- [ ] **Step 9: Run test to verify it passes**

Run: `cargo test -p inspectah-refine new_password_triggers`
Expected: PASS

- [ ] **Step 10: Run all refine tests**

Run: `cargo test -p inspectah-refine`
Expected: all PASS

- [ ] **Step 11: Commit**

```bash
git add inspectah-refine/src/types.rs inspectah-refine/src/session.rs
git commit -m "feat(refine): add UserStrategy and UserPassword ops with sensitivity tracking"
```

---

### Task 5: Pipeline — User Artifact Renderers

**Files:**
- Create: `inspectah-pipeline/src/render/users.rs`
- Modify: `inspectah-pipeline/src/render/mod.rs`
- Modify: `inspectah-pipeline/src/render/containerfile.rs`
- Test: `inspectah-pipeline/tests/users_render_test.rs`

- [ ] **Step 1: Write failing test for kickstart renderer**

Create `inspectah-pipeline/tests/users_render_test.rs`:

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::users::UserGroupSection;

#[test]
fn kickstart_renders_group_before_user() {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({
            "name": "alice",
            "uid": 1000,
            "gid": 1000,
            "shell": "/bin/bash",
            "home": "/home/alice",
            "supplementary_groups": ["wheel", "docker"]
        })],
        groups: vec![serde_json::json!({
            "name": "alice",
            "gid": 1000,
            "source": "custom"
        })],
        ..Default::default()
    });

    let ks = inspectah_pipeline::render::users::render_kickstart(&snap);
    let group_pos = ks.find("group --name=alice").unwrap();
    let user_pos = ks.find("user --name=alice").unwrap();
    assert!(group_pos < user_pos, "group must precede user in kickstart");
    assert!(ks.contains("--uid=1000"));
    assert!(ks.contains("--gid=1000"));
    assert!(ks.contains("--groups=wheel,docker"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-pipeline kickstart_renders_group`
Expected: FAIL — module `users` not found

- [ ] **Step 3: Create `inspectah-pipeline/src/render/users.rs` with kickstart renderer**

```rust
use inspectah_core::snapshot::InspectionSnapshot;

pub fn render_kickstart(snap: &InspectionSnapshot) -> String {
    let mut lines = Vec::new();
    let ug = match &snap.users_groups {
        Some(ug) => ug,
        None => return String::new(),
    };

    // Groups first (custom only, GID 1000+)
    let custom_groups: Vec<&serde_json::Value> = ug
        .groups
        .iter()
        .filter(|g| g.get("source").and_then(|s| s.as_str()) == Some("custom"))
        .collect();

    if !custom_groups.is_empty() {
        lines.push("# Groups -- generated by inspectah".to_string());
        for group in &custom_groups {
            let name = group["name"].as_str().unwrap_or("");
            let gid = group["gid"].as_u64().unwrap_or(0);
            lines.push(format!("group --name={name} --gid={gid}"));
        }
        lines.push(String::new());
    }

    // Users
    lines.push("# Users -- generated by inspectah".to_string());
    for user in &ug.users {
        let name = user["name"].as_str().unwrap_or("");
        let uid = user["uid"].as_u64().unwrap_or(0);
        let gid = user["gid"].as_u64().unwrap_or(0);
        let home = user["home"].as_str().unwrap_or("");
        let shell = user["shell"].as_str().unwrap_or("");

        let mut parts = vec![format!(
            "user --name={name} --uid={uid} --gid={gid} --homedir={home} --shell={shell}"
        )];

        if let Some(groups) = user.get("supplementary_groups").and_then(|v| v.as_array()) {
            let names: Vec<&str> = groups.iter().filter_map(|g| g.as_str()).collect();
            if !names.is_empty() {
                parts.push(format!("--groups={}", names.join(",")));
            }
        }

        if let Some(hash) = user.get("password_hash").and_then(|v| v.as_str()) {
            if !hash.is_empty() {
                parts.push(format!("--iscrypted --password={hash}"));
            }
        }

        lines.push(parts.join(" "));
    }

    // SSH keys
    let has_keys = ug.users.iter().any(|u| {
        u.get("ssh_keys")
            .and_then(|k| k.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false)
    });

    if has_keys {
        lines.push(String::new());
        lines.push("# SSH keys".to_string());
        for user in &ug.users {
            let name = user["name"].as_str().unwrap_or("");
            if let Some(keys) = user.get("ssh_keys").and_then(|k| k.as_array()) {
                for key in keys {
                    if let Some(k) = key.as_str() {
                        lines.push(format!("sshkey --username={name} \"{k}\""));
                    }
                }
            }
        }
    }

    lines.join("\n") + "\n"
}
```

- [ ] **Step 4: Register the module in `inspectah-pipeline/src/render/mod.rs`**

Add: `pub mod users;`

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p inspectah-pipeline kickstart_renders_group`
Expected: PASS

- [ ] **Step 6: Write failing test for Blueprint TOML renderer**

In `inspectah-pipeline/tests/users_render_test.rs`:

```rust
#[test]
fn toml_renders_group_and_user_blocks() {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({
            "name": "alice",
            "uid": 1000,
            "gid": 1000,
            "shell": "/bin/bash",
            "home": "/home/alice",
            "supplementary_groups": ["wheel"],
            "password_hash": "$6$rounds=5000$salt$hash"
        })],
        groups: vec![serde_json::json!({
            "name": "alice", "gid": 1000, "source": "custom"
        })],
        ..Default::default()
    });

    let toml = inspectah_pipeline::render::users::render_blueprint_toml(&snap);
    assert!(toml.contains("[[customizations.group]]"));
    assert!(toml.contains("[[customizations.user]]"));
    assert!(toml.contains("gid = 1000"));
    assert!(toml.contains("password = \"$6$rounds=5000$salt$hash\""));
}
```

- [ ] **Step 7: Implement `render_blueprint_toml`**

```rust
pub fn render_blueprint_toml(snap: &InspectionSnapshot) -> String {
    let mut lines = Vec::new();
    let ug = match &snap.users_groups {
        Some(ug) => ug,
        None => return String::new(),
    };

    // Groups
    for group in &ug.groups {
        if group.get("source").and_then(|s| s.as_str()) != Some("custom") {
            continue;
        }
        let name = group["name"].as_str().unwrap_or("");
        let gid = group["gid"].as_u64().unwrap_or(0);
        lines.push("[[customizations.group]]".to_string());
        lines.push(format!("name = \"{name}\""));
        lines.push(format!("gid = {gid}"));
        lines.push(String::new());
    }

    // Users
    for user in &ug.users {
        let name = user["name"].as_str().unwrap_or("");
        let uid = user["uid"].as_u64().unwrap_or(0);
        let gid = user["gid"].as_u64().unwrap_or(0);
        let home = user["home"].as_str().unwrap_or("");
        let shell = user["shell"].as_str().unwrap_or("");

        lines.push("[[customizations.user]]".to_string());
        lines.push(format!("name = \"{name}\""));
        lines.push(format!("uid = {uid}"));
        lines.push(format!("gid = {gid}"));

        if let Some(groups) = user.get("supplementary_groups").and_then(|v| v.as_array()) {
            let names: Vec<String> = groups
                .iter()
                .filter_map(|g| g.as_str().map(|s| format!("\"{s}\"")))
                .collect();
            if !names.is_empty() {
                lines.push(format!("groups = [{}]", names.join(", ")));
            }
        }

        lines.push(format!("home = \"{home}\""));
        lines.push(format!("shell = \"{shell}\""));

        if let Some(hash) = user.get("password_hash").and_then(|v| v.as_str()) {
            if !hash.is_empty() {
                lines.push(format!("password = \"{hash}\""));
            }
        }

        if let Some(keys) = user.get("ssh_keys").and_then(|k| k.as_array()) {
            if let Some(first_key) = keys.first().and_then(|k| k.as_str()) {
                lines.push(format!("key = \"{first_key}\""));
                if keys.len() > 1 {
                    lines.push(format!(
                        "# Additional keys in inspectah-users.ks and users/home/{name}/.ssh/authorized_keys"
                    ));
                }
            }
        }

        lines.push(String::new());
    }

    lines.join("\n")
}
```

- [ ] **Step 8: Run test to verify it passes**

Run: `cargo test -p inspectah-pipeline toml_renders_group`
Expected: PASS

- [ ] **Step 9: Write failing test for Containerfile useradd renderer**

```rust
#[test]
fn containerfile_useradd_with_groups_and_ssh() {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({
            "name": "alice",
            "uid": 1000,
            "gid": 1000,
            "shell": "/bin/bash",
            "home": "/home/alice",
            "containerfile_strategy": "useradd",
            "supplementary_groups": ["wheel"],
            "password_hash": "$6$rounds=5000$salt$hash",
            "ssh_keys": ["ssh-ed25519 AAAAC3NzaC1lZDI1NTE5 alice@work"]
        })],
        groups: vec![serde_json::json!({
            "name": "alice", "gid": 1000, "source": "custom"
        })],
        ..Default::default()
    });

    let cf = inspectah_pipeline::render::users::render_containerfile_users(&snap);
    assert!(cf.contains("groupadd -g 1000 alice"));
    assert!(cf.contains("useradd -u 1000"));
    assert!(cf.contains("chpasswd -e"));
    assert!(cf.contains("COPY users/home/alice/.ssh/authorized_keys"));
    assert!(cf.contains("install -d -m 700"));
}
```

- [ ] **Step 10: Implement `render_containerfile_users`**

```rust
pub fn render_containerfile_users(snap: &InspectionSnapshot) -> String {
    let mut lines = Vec::new();
    let ug = match &snap.users_groups {
        Some(ug) => ug,
        None => return String::new(),
    };

    let useradd_users: Vec<&serde_json::Value> = ug
        .users
        .iter()
        .filter(|u| {
            u.get("containerfile_strategy")
                .and_then(|s| s.as_str())
                == Some("useradd")
        })
        .collect();

    if useradd_users.is_empty() {
        return String::new();
    }

    // Collect needed custom groups
    let needed_gids: std::collections::HashSet<u64> = useradd_users
        .iter()
        .filter_map(|u| u.get("gid").and_then(|g| g.as_u64()))
        .collect();

    let custom_groups: Vec<&serde_json::Value> = ug
        .groups
        .iter()
        .filter(|g| {
            g.get("source").and_then(|s| s.as_str()) == Some("custom")
                && needed_gids.contains(&g.get("gid").and_then(|v| v.as_u64()).unwrap_or(0))
        })
        .collect();

    if !custom_groups.is_empty() {
        lines.push("# Groups (custom, GID 1000+)".to_string());
        for group in &custom_groups {
            let name = group["name"].as_str().unwrap_or("");
            let gid = group["gid"].as_u64().unwrap_or(0);
            lines.push(format!("RUN groupadd -g {gid} {name}"));
        }
        lines.push(String::new());
    }

    for user in &useradd_users {
        let name = user["name"].as_str().unwrap_or("");
        let uid = user["uid"].as_u64().unwrap_or(0);
        let gid = user["gid"].as_u64().unwrap_or(0);
        let home = user["home"].as_str().unwrap_or("");
        let shell = user["shell"].as_str().unwrap_or("");

        let mut useradd_args = format!(
            "RUN useradd -u {uid} -g {gid}"
        );

        if let Some(groups) = user.get("supplementary_groups").and_then(|v| v.as_array()) {
            let names: Vec<&str> = groups.iter().filter_map(|g| g.as_str()).collect();
            if !names.is_empty() {
                useradd_args.push_str(&format!(" -G {}", names.join(",")));
            }
        }

        useradd_args.push_str(&format!(" -d {home} -s {shell} -m {name}"));
        lines.push(format!("# User: {name} (useradd -- install-time seeding)"));
        lines.push(useradd_args);

        // Password
        if let Some(hash) = user.get("password_hash").and_then(|v| v.as_str()) {
            if !hash.is_empty() {
                lines.push(
                    "# WARNING: Password hash in image layer -- inspectable by anyone with image access"
                        .to_string(),
                );
                lines.push(format!("RUN echo '{name}:{hash}' | chpasswd -e"));
            }
        }

        // SSH keys
        if let Some(keys) = user.get("ssh_keys").and_then(|k| k.as_array()) {
            if !keys.is_empty() {
                lines.push(format!(
                    "RUN install -d -m 700 -o {name} -g {name} {home}/.ssh"
                ));
                lines.push(format!(
                    "COPY users/home/{name}/.ssh/authorized_keys {home}/.ssh/authorized_keys"
                ));
                lines.push(format!(
                    "RUN chown {name}:{name} {home}/.ssh/authorized_keys && \\\n    chmod 600 {home}/.ssh/authorized_keys"
                ));
            }
        }

        lines.push(String::new());
    }

    lines.join("\n")
}
```

- [ ] **Step 11: Write function to stage SSH authorized_keys files**

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
            .join("\n")
            + "\n";

        std::fs::write(ssh_dir.join("authorized_keys"), content)?;
    }

    Ok(())
}
```

- [ ] **Step 12: Run all pipeline tests**

Run: `cargo test -p inspectah-pipeline`
Expected: all PASS

- [ ] **Step 13: Commit**

```bash
git add inspectah-pipeline/src/render/users.rs inspectah-pipeline/src/render/mod.rs inspectah-pipeline/tests/users_render_test.rs
git commit -m "feat(pipeline): add kickstart, blueprint TOML, and containerfile user renderers"
```

---

### Task 6: Refine Export — Wire User Artifacts

**Files:**
- Modify: `inspectah-refine/src/session.rs` (in `render_refine_export`)
- Modify: `inspectah-refine/tests/export_contract_test.rs`

- [ ] **Step 1: Write failing test for user artifacts in export**

In `inspectah-refine/tests/export_contract_test.rs`, add a new test:

```rust
#[test]
fn export_includes_user_artifacts() {
    let mut snap = test_snapshot();
    snap.users_groups = Some(inspectah_core::types::users::UserGroupSection {
        users: vec![serde_json::json!({
            "name": "alice", "uid": 1000, "gid": 1000,
            "shell": "/bin/bash", "home": "/home/alice",
            "classification": "interactive",
            "supplementary_groups": []
        })],
        groups: vec![serde_json::json!({
            "name": "alice", "gid": 1000, "source": "custom"
        })],
        ..Default::default()
    });
    let session = RefineSession::new(snap);

    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");
    session
        .export_tarball(&tarball_path, session.generation())
        .unwrap();

    let files = tarball_file_set(&tarball_path);
    assert!(
        files.contains("inspectah-users.ks"),
        "missing inspectah-users.ks in {files:?}"
    );
    assert!(
        files.contains("inspectah-users.toml"),
        "missing inspectah-users.toml in {files:?}"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-refine export_includes_user`
Expected: FAIL — files not in tarball

- [ ] **Step 3: Add user artifacts to `render_refine_export`**

In `render_refine_export`, after the Containerfile write and before the tarball packing:

```rust
// 3b. User provisioning artifacts (always generated when users_groups is present)
let users_ks = inspectah_pipeline::render::users::render_kickstart(snap);
if !users_ks.trim().is_empty() {
    std::fs::write(out.join("inspectah-users.ks"), &users_ks)?;
}
let users_toml = inspectah_pipeline::render::users::render_blueprint_toml(snap);
if !users_toml.trim().is_empty() {
    std::fs::write(out.join("inspectah-users.toml"), &users_toml)?;
}

// 3c. Stage SSH authorized_keys files
inspectah_pipeline::render::users::stage_ssh_keys(snap, out)
    .map_err(|e| RefineError::RenderFailed(e.to_string()))?;
```

Also add `"inspectah-users.ks"`, `"inspectah-users.toml"`, and `"users"` to the `allowed_top_level` set.

- [ ] **Step 4: Integrate user Containerfile directives into main Containerfile**

In the Containerfile rendering (either in `render_containerfile` or as an append), add the user materialization block after the existing content:

```rust
let user_cf = inspectah_pipeline::render::users::render_containerfile_users(snap);
if !user_cf.is_empty() {
    containerfile.push_str("\n# --- User accounts (install-time seeding) ---\n");
    containerfile.push_str(&user_cf);
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p inspectah-refine export_includes_user`
Expected: PASS

- [ ] **Step 6: Update the existing `export_exact_file_set` test**

Add `"inspectah-users.ks"` and `"inspectah-users.toml"` to the expected file set in the existing test (the test needs `users_groups` data in the test snapshot).

- [ ] **Step 7: Write test for SSH key staging in export**

```rust
#[test]
fn export_stages_ssh_keys() {
    let mut snap = test_snapshot();
    snap.users_groups = Some(inspectah_core::types::users::UserGroupSection {
        users: vec![serde_json::json!({
            "name": "alice", "uid": 1000, "gid": 1000,
            "shell": "/bin/bash", "home": "/home/alice",
            "ssh_keys": ["ssh-ed25519 AAAAC3NzaC1lZDI1NTE5 alice@work"],
            "containerfile_strategy": "useradd",
            "supplementary_groups": []
        })],
        ..Default::default()
    });
    snap.preserved_ssh_keys = true;
    snap.sensitive_snapshot = true;

    let session = RefineSession::new(snap);
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");
    session.export_tarball(&tarball_path, session.generation()).unwrap();

    let files = tarball_file_set(&tarball_path);
    assert!(files.contains("users/home/alice/.ssh/authorized_keys"));
}
```

- [ ] **Step 8: Run all refine and export tests**

Run: `cargo test -p inspectah-refine`
Expected: all PASS

- [ ] **Step 9: Commit**

```bash
git add inspectah-refine/src/session.rs inspectah-refine/tests/export_contract_test.rs
git commit -m "feat(refine): wire user artifacts into export tarball and file set"
```

---

### Task 7: Web API — User Decision Endpoints

**Files:**
- Modify: `inspectah-web/src/handlers.rs`
- Modify: `inspectah-web/src/lib.rs`

- [ ] **Step 1: Add request types**

In `inspectah-web/src/handlers.rs`:

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

```rust
pub async fn user_strategy(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UserStrategyRequest>,
) -> impl IntoResponse {
    let strategy = match req.strategy.as_str() {
        "skip" => UserContainerfileStrategy::Skip,
        "useradd" => UserContainerfileStrategy::Useradd,
        other => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": format!("unknown strategy: {other}")}))).into_response(),
    };
    let mut session = state.session.lock().unwrap();
    match session.apply(RefinementOp::UserStrategy {
        username: req.username,
        strategy,
    }) {
        Ok(()) => Json(build_view_response(&session)).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}
```

- [ ] **Step 3: Implement `user_password` handler**

```rust
pub async fn user_password(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UserPasswordRequest>,
) -> impl IntoResponse {
    let (password, hash) = match req.choice.as_str() {
        "none" => (UserPasswordChoice::None, None),
        "preserve" => (UserPasswordChoice::Preserve, None),
        "new" => match req.hash {
            Some(h) => (UserPasswordChoice::New, Some(h)),
            None => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "hash required for new password"}))).into_response(),
        },
        other => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": format!("unknown choice: {other}")}))).into_response(),
    };
    let mut session = state.session.lock().unwrap();
    match session.apply(RefinementOp::UserPassword {
        username: req.username,
        password,
        hash,
    }) {
        Ok(()) => Json(build_view_response(&session)).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}
```

- [ ] **Step 4: Implement `user_preview` handler**

```rust
pub async fn user_preview(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let session = state.session.lock().unwrap();
    let projected = session.snapshot_projected();
    let ks = inspectah_pipeline::render::users::render_kickstart(&projected);
    let toml = inspectah_pipeline::render::users::render_blueprint_toml(&projected);
    Json(serde_json::json!({
        "kickstart": ks,
        "blueprint_toml": toml,
    }))
}
```

- [ ] **Step 5: Add `users_groups_decisions` to `ViewResponse`**

Add a field to `ViewResponse`:

```rust
pub users_groups_decisions: Vec<serde_json::Value>,
```

Populate in `build_view_response` by reading user entries from the projected snapshot:

```rust
let users_groups_decisions = session
    .snapshot_projected()
    .users_groups
    .map(|ug| ug.users)
    .unwrap_or_default();
```

- [ ] **Step 6: Add export gating to tarball handler**

In the existing `export_tarball` handler, add sensitivity check before proceeding:

```rust
if session.is_sensitive() {
    let ack = headers.get("x-acknowledge-sensitive")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("false");
    if ack != "true" {
        return (StatusCode::PRECONDITION_REQUIRED, Json(serde_json::json!({
            "error": "sensitive_export",
            "sensitivity_summary": {
                "preserved_credentials": session.snapshot().preserved_credentials,
                "preserved_ssh_keys": session.snapshot().preserved_ssh_keys,
                "new_passwords": session.is_sensitive() && !session.snapshot().sensitive_snapshot,
            }
        }))).into_response();
    }
}
```

- [ ] **Step 7: Register routes**

In `inspectah-web/src/lib.rs`, add:

```rust
.route("/api/user-strategy", post(handlers::user_strategy))
.route("/api/user-password", post(handlers::user_password))
.route("/api/user-preview", get(handlers::user_preview))
```

- [ ] **Step 8: Move `users_groups` from cached sections to view path**

In `get_sections`, remove `normalize_users_groups(snap)` from the `sections_cache` initialization. The data now comes through `ViewResponse.users_groups_decisions`.

- [ ] **Step 9: Build and verify compilation**

Run: `cargo build -p inspectah-web`
Expected: successful build

- [ ] **Step 10: Commit**

```bash
git add inspectah-web/src/handlers.rs inspectah-web/src/lib.rs
git commit -m "feat(web): add user strategy, password, and preview API endpoints"
```

---

### Task 8: Web UI — Users & Groups Decision Section

**Files:**
- Create: `inspectah-web/ui/src/components/UsersGroupsSection.tsx`
- Modify: `inspectah-web/ui/src/components/Sidebar.tsx`
- Modify: `inspectah-web/ui/src/components/MainContent.tsx`
- Modify: `inspectah-web/ui/src/api/client.ts`
- Modify: `inspectah-web/ui/src/hooks/useView.ts`

This task creates the React components for the Users & Groups decision section. The detailed keyboard/focus/a11y behaviors are deferred to the UX hardening backlog — this task delivers the functional UI.

- [ ] **Step 1: Add API client methods**

In `inspectah-web/ui/src/api/client.ts`:

```typescript
export async function setUserStrategy(username: string, strategy: string) {
  const res = await fetch('/api/user-strategy', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ username, strategy }),
  });
  return res.json();
}

export async function setUserPassword(username: string, choice: string, hash?: string) {
  const res = await fetch('/api/user-password', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ username, choice, hash }),
  });
  return res.json();
}

export async function fetchUserPreview() {
  const res = await fetch('/api/user-preview');
  return res.json();
}
```

- [ ] **Step 2: Create `UsersGroupsSection.tsx` with user cards**

Create the component with:
- Section header with banner text and "Preview Artifacts" button
- User cards rendering name, UID, shell, home, supplementary groups
- Classification rationale display
- Badges: sudo (amber), SSH (blue), subuid (teal)
- Strategy radio group (Skip / useradd)
- Collapsible password options section
- Collapsible SSH key detail section
- Non-interactive card visual distinction
- Empty state message
- Sensitivity banner when `sensitive_snapshot` is true

The component reads user data from `ViewResponse.users_groups_decisions` via the existing view hook.

- [ ] **Step 3: Create artifact preview modal**

Add a modal/drawer component that shows the rendered `inspectah-users.ks` and `inspectah-users.toml` content from the `/api/user-preview` endpoint. Tabbed view to switch between formats. Read-only, monospace display. Sensitive values show redaction placeholder with per-value reveal toggle.

- [ ] **Step 4: Move `users_groups` to Decisions group in Sidebar**

In `Sidebar.tsx`, move `users_groups` from the Context section list to the Decisions section list. Update any section-group constants or arrays.

- [ ] **Step 5: Render `UsersGroupsSection` in MainContent**

In `MainContent.tsx`, add the `UsersGroupsSection` component in the Decisions area, similar to how packages and configs are rendered.

- [ ] **Step 6: Add export gating UI**

In the export/download flow, check if the session is sensitive. If so, show a sensitivity banner with acknowledgment checkbox before allowing tarball download. Pass `X-Acknowledge-Sensitive: true` header when the user confirms.

- [ ] **Step 7: Manual testing in browser**

Start the dev server: `cargo run -p inspectah-cli -- refine <test-tarball>`
Open the UI and verify:
- Users & Groups appears in the Decisions group
- User cards render with correct data
- Strategy selector works (Skip/useradd)
- Password options expand/collapse
- SSH key detail expands/collapse
- Badges display correctly
- Artifact preview shows KS and TOML
- Sensitivity banner appears when applicable

- [ ] **Step 8: Commit**

```bash
git add inspectah-web/ui/src/
git commit -m "feat(web): add Users & Groups decision section with user cards and artifact preview"
```

---

### Task 9: Integration — End-to-End Smoke Test

**Files:**
- Modify: `inspectah-pipeline/tests/smoke_render_2b.rs` (or create new)
- Test: `inspectah-cli/tests/refine_e2e_test.rs`

- [ ] **Step 1: Write e2e test for full pipeline**

Create a test that builds a snapshot with users/groups, runs it through the refine session, applies a `UserStrategy::Useradd` op, exports, and verifies the tarball contains:
- `inspectah-users.ks` with correct group + user lines
- `inspectah-users.toml` with correct TOML blocks
- `Containerfile` with useradd directives
- `users/home/<user>/.ssh/authorized_keys` when SSH keys are preserved

```rust
#[test]
fn full_pipeline_users_groups_materialization() {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({
            "name": "alice",
            "uid": 1000,
            "gid": 1000,
            "shell": "/bin/bash",
            "home": "/home/alice",
            "classification": "interactive",
            "classification_rationale": "bash shell, home at /home/alice",
            "password_status": "password_set",
            "password_hash": "$6$rounds=5000$testsalt$testhash",
            "ssh_key_count": 1,
            "ssh_keys": ["ssh-ed25519 AAAAC3NzaC1lZDI1NTE5 alice@work"],
            "has_sudo": true,
            "has_subuid": false,
            "supplementary_groups": ["wheel"]
        })],
        groups: vec![serde_json::json!({
            "name": "alice",
            "gid": 1000,
            "source": "custom"
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

    let mut session = RefineSession::new(snap);
    session
        .apply(RefinementOp::UserStrategy {
            username: "alice".into(),
            strategy: UserContainerfileStrategy::Useradd,
        })
        .unwrap();

    assert!(session.is_sensitive());

    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");
    session
        .export_tarball(&tarball_path, session.generation())
        .unwrap();

    let files = tarball_file_set(&tarball_path);
    assert!(files.contains("inspectah-users.ks"));
    assert!(files.contains("inspectah-users.toml"));
    assert!(files.contains("users/home/alice/.ssh/authorized_keys"));
    assert!(files.contains("Containerfile"));

    // Verify Containerfile contains useradd
    let cf = read_tarball_file(&tarball_path, "Containerfile");
    assert!(cf.contains("useradd"), "Containerfile should contain useradd for alice");
    assert!(cf.contains("groupadd"), "Containerfile should contain groupadd for alice's group");
}
```

- [ ] **Step 2: Run the integration test**

Run: `cargo test -p inspectah-refine full_pipeline_users`
Expected: PASS

- [ ] **Step 3: Run the full workspace test suite**

Run: `cargo test --workspace`
Expected: all PASS

- [ ] **Step 4: Commit**

```bash
git add inspectah-refine/tests/ inspectah-pipeline/tests/
git commit -m "test: add end-to-end integration test for user/group materialization"
```
