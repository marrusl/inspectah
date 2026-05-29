// inspectah-web/tests/fixture_structure_test.rs
//
// Snapshot test: parses each e2e fixture as serde_json::Value and snapshots it.
// Any fixture change (field added, removed, renamed, retyped) breaks the snapshot
// and requires explicit `cargo insta review` acceptance.
//
// This does NOT validate against Rust response types (they lack Deserialize).
// Rust-type compatibility is proven by Task 8's real-server smoke tests.
//
// Run with: INSPECTAH_SKIP_UI=1 cargo test -p inspectah-web --test fixture_structure_test

fn fixture_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/e2e/fixtures")
}

fn snapshot_fixture(name: &str, relative_path: &str) {
    let path = fixture_dir().join(relative_path);
    let json_str = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Cannot read fixture {}: {}", path.display(), e));
    let value: serde_json::Value = serde_json::from_str(&json_str)
        .unwrap_or_else(|e| panic!("Fixture {} is not valid JSON: {}", relative_path, e));
    insta::assert_json_snapshot!(name, value);
}

// --- Body fixtures: single-host (GET presets) ---

#[test]
fn fixture_single_host_health() {
    snapshot_fixture("single_host_health", "single-host/health.json");
}

#[test]
fn fixture_single_host_view() {
    snapshot_fixture("single_host_view", "single-host/view.json");
}

#[test]
fn fixture_single_host_sections() {
    snapshot_fixture("single_host_sections", "single-host/sections.json");
}

#[test]
fn fixture_single_host_ops() {
    snapshot_fixture("single_host_ops", "single-host/ops-empty.json");
}

#[test]
fn fixture_single_host_changes() {
    snapshot_fixture("single_host_changes", "single-host/changes-empty.json");
}

#[test]
fn fixture_single_host_viewed() {
    snapshot_fixture("single_host_viewed", "single-host/viewed-empty.json");
}

#[test]
fn fixture_single_host_user_preview() {
    snapshot_fixture("single_host_user_preview", "single-host/user-preview.json");
}

#[test]
fn fixture_single_host_user_preview_redacted() {
    snapshot_fixture(
        "single_host_user_preview_redacted",
        "single-host/user-preview-redacted.json",
    );
}

// --- Body fixtures: fleet (GET presets) ---

#[test]
fn fixture_fleet_view() {
    snapshot_fixture("fleet_view", "fleet/fleet-view.json");
}

#[test]
fn fixture_fleet_health() {
    snapshot_fixture("fleet_health", "fleet/health.json");
}

#[test]
fn fixture_fleet_sections() {
    snapshot_fixture("fleet_sections", "fleet/sections.json");
}

// --- Sequence fixtures ---

#[test]
fn fixture_sequence_after_exclude() {
    snapshot_fixture(
        "seq_after_exclude",
        "sequences/exclude-undo-redo/01-after-exclude.json",
    );
}

#[test]
fn fixture_sequence_after_undo() {
    snapshot_fixture(
        "seq_after_undo",
        "sequences/exclude-undo-redo/02-after-undo.json",
    );
}

#[test]
fn fixture_sequence_after_redo() {
    snapshot_fixture(
        "seq_after_redo",
        "sequences/exclude-undo-redo/03-after-redo.json",
    );
}

// --- Error fixtures ---

#[test]
fn fixture_error_server_500() {
    snapshot_fixture("error_server_500", "errors/server-500.json");
}

// --- Manifest ---

#[test]
fn fixture_manifest() {
    snapshot_fixture("manifest", "manifest.json");
}

// --- POST response wrappers (strip _status, snapshot body) ---

fn snapshot_post_fixture(name: &str, relative_path: &str) {
    let path = fixture_dir().join(relative_path);
    let json_str = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Cannot read POST fixture {}: {}", path.display(), e));
    let mut value: serde_json::Value = serde_json::from_str(&json_str)
        .unwrap_or_else(|e| panic!("POST fixture {} is not valid JSON: {}", relative_path, e));
    // Strip _status transport metadata before snapshotting the body
    if let Some(obj) = value.as_object_mut() {
        obj.remove("_status");
    }
    insta::assert_json_snapshot!(name, value);
}

#[test]
fn fixture_post_op_success() {
    snapshot_post_fixture("post_op_success", "post-responses/op/success.json");
}

#[test]
fn fixture_post_undo_success() {
    snapshot_post_fixture("post_undo_success", "post-responses/undo/success.json");
}

#[test]
fn fixture_post_undo_nothing() {
    snapshot_post_fixture(
        "post_undo_nothing",
        "post-responses/undo/nothing-to-undo.json",
    );
}

#[test]
fn fixture_post_redo_success() {
    snapshot_post_fixture("post_redo_success", "post-responses/redo/success.json");
}

#[test]
fn fixture_post_fleet_diff_success() {
    snapshot_post_fixture(
        "post_fleet_diff_success",
        "post-responses/fleet-diff/success.json",
    );
}

#[test]
fn fixture_post_tarball_sensitive_required() {
    snapshot_post_fixture(
        "post_tarball_sensitive_required",
        "post-responses/tarball/sensitive-required.json",
    );
}

#[test]
fn fixture_post_tarball_stale() {
    snapshot_post_fixture("post_tarball_stale", "post-responses/tarball/stale.json");
}

#[test]
fn fixture_post_user_password_success() {
    snapshot_post_fixture(
        "post_user_password_success",
        "post-responses/user-password/success.json",
    );
}

#[test]
fn fixture_post_user_password_invalid() {
    snapshot_post_fixture(
        "post_user_password_invalid",
        "post-responses/user-password/invalid.json",
    );
}

#[test]
fn fixture_post_user_strategy_success() {
    snapshot_post_fixture(
        "post_user_strategy_success",
        "post-responses/user-strategy/success.json",
    );
}

#[test]
fn fixture_post_viewed_success() {
    snapshot_post_fixture("post_viewed_success", "post-responses/viewed/success.json");
}
