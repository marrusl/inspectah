//! Redaction coverage tests for Slice 2b inspector surfaces.
//!
//! Tests planted secrets in proxy URLs, container env, sudoers rules, and
//! shadow entries. Verifies the redaction engine detects, masks, or flags
//! each surface correctly. Compose env is NOT scanned (hints-only).

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::containers::{ContainerSection, RunningContainer};
use inspectah_core::types::network::{NetworkSection, ProxyEntry};
use inspectah_core::types::redaction::{Confidence, FindingKind, RedactionState};
use inspectah_core::types::users::UserGroupSection;
use inspectah_pipeline::redaction::engine::{redact, RedactOptions};

// ---------------------------------------------------------------------------
// Test 1: Proxy URL with password masked inline
// ---------------------------------------------------------------------------

#[test]
fn proxy_url_with_password_masked_inline() {
    let mut snapshot = InspectionSnapshot::new();
    snapshot.network = Some(NetworkSection {
        proxy: vec![ProxyEntry {
            source: "etc/environment".into(),
            line: "http_proxy=http://admin:secret123@proxy.corp.com:8080".into(),
        }],
        ..Default::default()
    });

    redact(&mut snapshot, &RedactOptions::default());

    let net = snapshot.network.as_ref().unwrap();
    let masked_line = &net.proxy[0].line;

    // Password segment must be replaced with [REDACTED]
    assert!(
        !masked_line.contains("secret123"),
        "password must be masked, got: {masked_line}"
    );
    assert!(
        masked_line.contains("[REDACTED]"),
        "masked line must contain [REDACTED], got: {masked_line}"
    );

    // Full URL structure must be preserved
    assert!(
        masked_line.contains("http_proxy=http://admin:[REDACTED]@proxy.corp.com:8080"),
        "URL structure must be preserved, got: {masked_line}"
    );

    // Finding must be recorded
    assert!(
        snapshot.redactions.iter().any(
            |f| f.finding_kind == Some(FindingKind::Password) && f.source == "proxy_credential"
        ),
        "proxy credential finding must be recorded"
    );
}

// ---------------------------------------------------------------------------
// Test 2: Podman env with secret redacted
// ---------------------------------------------------------------------------

#[test]
fn podman_env_with_secret_redacted() {
    let mut snapshot = InspectionSnapshot::new();
    snapshot.containers = Some(ContainerSection {
        running_containers: vec![RunningContainer {
            name: "mydb".into(),
            env: vec!["DB_PASSWORD=hunter2".into(), "PORT=5432".into()],
            ..Default::default()
        }],
        ..Default::default()
    });

    redact(&mut snapshot, &RedactOptions::default());

    let containers = snapshot.containers.as_ref().unwrap();
    let env = &containers.running_containers[0].env;

    // DB_PASSWORD value must be redacted
    assert!(
        !env.iter().any(|e| e.contains("hunter2")),
        "secret env value must be redacted, got: {env:?}"
    );

    // Finding must be recorded
    assert!(
        snapshot.redactions.iter().any(|f| f.path.contains("mydb")),
        "finding must reference container name"
    );
}

// ---------------------------------------------------------------------------
// Test 3: Sudoers with embedded password redacted
// ---------------------------------------------------------------------------

#[test]
fn sudoers_with_embedded_password_redacted() {
    let mut snapshot = InspectionSnapshot::new();
    snapshot.users_groups = Some(UserGroupSection {
        sudoers_rules: vec![
            "deploy ALL=(ALL) NOPASSWD:ALL".into(),
            "# password=supersecret for automated login".into(),
        ],
        ..Default::default()
    });

    redact(&mut snapshot, &RedactOptions::default());

    let users = snapshot.users_groups.as_ref().unwrap();

    // The rule containing password= must be redacted
    assert!(
        !users
            .sudoers_rules
            .iter()
            .any(|r| r.contains("supersecret")),
        "password in sudoers rule must be redacted, got: {:?}",
        users.sudoers_rules
    );

    // Finding must be recorded
    assert!(
        snapshot.redactions.iter().any(|f| f.path == "/etc/sudoers"),
        "finding must reference /etc/sudoers"
    );
}

// ---------------------------------------------------------------------------
// Test 4: Shadow hash detected
// ---------------------------------------------------------------------------

#[test]
fn shadow_hash_detected() {
    let mut snapshot = InspectionSnapshot::new();
    snapshot.users_groups = Some(UserGroupSection {
        shadow_entries: vec![
            "admin:$6$rounds=65536$saltsalt$hashhashhash:19000:0:99999:7:::".into(),
        ],
        ..Default::default()
    });

    redact(&mut snapshot, &RedactOptions::default());

    // Shadow hash finding must be recorded
    assert!(
        snapshot
            .redactions
            .iter()
            .any(|f| f.finding_kind == Some(FindingKind::ShadowHash)
                && f.confidence == Some(Confidence::High)),
        "shadow hash must produce a High-confidence ShadowHash finding"
    );

    // Hash must be redacted inline
    let users = snapshot.users_groups.as_ref().unwrap();
    assert!(
        !users.shadow_entries[0].contains("$6$"),
        "shadow hash must be redacted inline, got: {}",
        users.shadow_entries[0]
    );
}

// ---------------------------------------------------------------------------
// Test 5: Shadow locked — no finding
// ---------------------------------------------------------------------------

#[test]
fn shadow_locked_no_finding() {
    let mut snapshot = InspectionSnapshot::new();
    snapshot.users_groups = Some(UserGroupSection {
        shadow_entries: vec!["root:!!:19000:0:99999:7:::".into()],
        ..Default::default()
    });

    redact(&mut snapshot, &RedactOptions::default());

    // Locked account must not produce a finding
    assert!(
        snapshot.redactions.is_empty(),
        "locked shadow account must not produce findings, got: {:?}",
        snapshot.redactions
    );

    // State should be FullyRedacted (clean)
    match &snapshot.redaction_state {
        Some(RedactionState::FullyRedacted { .. }) => {}
        other => panic!("expected FullyRedacted for locked shadow, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Test 6: Clean proxy — no finding
// ---------------------------------------------------------------------------

#[test]
fn clean_proxy_no_finding() {
    let mut snapshot = InspectionSnapshot::new();
    snapshot.network = Some(NetworkSection {
        proxy: vec![ProxyEntry {
            source: "etc/environment".into(),
            line: "http_proxy=http://proxy:8080".into(),
        }],
        ..Default::default()
    });

    redact(&mut snapshot, &RedactOptions::default());

    // No credentials = no finding
    assert!(
        snapshot
            .redactions
            .iter()
            .all(|f| f.source != "proxy_credential"),
        "clean proxy URL must not produce a proxy_credential finding"
    );

    // Line must be unchanged
    let net = snapshot.network.as_ref().unwrap();
    assert_eq!(
        net.proxy[0].line, "http_proxy=http://proxy:8080",
        "clean proxy line must not be modified"
    );
}

// ---------------------------------------------------------------------------
// Test 7: Clean env — no finding
// ---------------------------------------------------------------------------

#[test]
fn clean_env_no_finding() {
    let mut snapshot = InspectionSnapshot::new();
    snapshot.containers = Some(ContainerSection {
        running_containers: vec![RunningContainer {
            name: "webserver".into(),
            env: vec!["PORT=8080".into(), "NODE_ENV=production".into()],
            ..Default::default()
        }],
        ..Default::default()
    });

    redact(&mut snapshot, &RedactOptions::default());

    // No secrets in env = no finding from container scan
    assert!(
        !snapshot
            .redactions
            .iter()
            .any(|f| f.path.contains("webserver")),
        "clean env vars must not produce container findings"
    );

    // Env values must be unchanged
    let containers = snapshot.containers.as_ref().unwrap();
    let env = &containers.running_containers[0].env;
    assert!(
        env.contains(&"PORT=8080".to_string()),
        "clean env must be preserved"
    );
    assert!(
        env.contains(&"NODE_ENV=production".to_string()),
        "clean env must be preserved"
    );
}
