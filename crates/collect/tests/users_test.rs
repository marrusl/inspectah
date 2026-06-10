//! Integration tests for UsersGroupsInspector.
//!
//! Runs the actual Rust inspector on fixture data via MockExecutor and
//! verifies output is structurally correct. Follows the same pattern as
//! parity_test.rs (Slice 2a inspectors).
//!
//! Classifies users by login shell:
//!   - Valid login shell → `interactive`
//!   - No valid login shell → `non-interactive`

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::users::UsersGroupsInspector;
use inspectah_core::traits::inspector::{InspectionContext, Inspector, InspectorError};
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::types::completeness::SectionData;
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;
use inspectah_core::types::users::UserGroupSection;

// ── Shared helpers ──────────────────────────────────────────────────

fn pkg_source() -> SourceSystem {
    SourceSystem::PackageBased {
        os_release: OsRelease {
            name: "Red Hat Enterprise Linux".into(),
            version_id: "9.4".into(),
            id: "rhel".into(),
            ..Default::default()
        },
    }
}

// ── Fixtures ────────────────────────────────────────────────────────

const PASSWD_FIXTURE: &str = include_str!("../../../testdata/fixtures/users/passwd");
const SHADOW_FIXTURE: &str = include_str!("../../../testdata/fixtures/users/shadow");
const GROUP_FIXTURE: &str = include_str!("../../../testdata/fixtures/users/group");
const GSHADOW_FIXTURE: &str = include_str!("../../../testdata/fixtures/users/gshadow");
const SUBUID_FIXTURE: &str = include_str!("../../../testdata/fixtures/users/subuid");
const SUBGID_FIXTURE: &str = include_str!("../../../testdata/fixtures/users/subgid");
const SUDOERS_FIXTURE: &str = include_str!("../../../testdata/fixtures/users/sudoers");
const SUDOERSD_FIXTURE: &str = include_str!("../../../testdata/fixtures/users/sudoers.d-webapp");
const SSH_KEYS_FIXTURE: &str = include_str!("../../../testdata/fixtures/users/authorized_keys");

// ── Mock builder ────────────────────────────────────────────────────

fn users_happy_mock() -> MockExecutor {
    MockExecutor::new()
        .with_file("/etc/passwd", PASSWD_FIXTURE)
        .with_file("/etc/shadow", SHADOW_FIXTURE)
        .with_file("/etc/group", GROUP_FIXTURE)
        .with_file("/etc/gshadow", GSHADOW_FIXTURE)
        .with_file("/etc/subuid", SUBUID_FIXTURE)
        .with_file("/etc/subgid", SUBGID_FIXTURE)
        .with_file("/etc/sudoers", SUDOERS_FIXTURE)
        .with_dir("/etc/sudoers.d", vec!["webapp"])
        .with_file("/etc/sudoers.d/webapp", SUDOERSD_FIXTURE)
        // SSH keys for alice (the only user with a login shell /bin/bash)
        .with_file("/home/alice/.ssh/authorized_keys", SSH_KEYS_FIXTURE)
}

/// Extract UserGroupSection from an inspector result, handling both Ok and Degraded.
fn extract_section(
    result: Result<inspectah_core::traits::inspector::InspectorOutput, InspectorError>,
) -> (
    inspectah_core::types::users::UserGroupSection,
    bool, // true if degraded
) {
    let (output, degraded) = match result {
        Ok(o) => (o, false),
        Err(InspectorError::Degraded { partial, .. }) => (*partial, true),
        Err(e) => panic!("unexpected error: {e}"),
    };

    match output.section {
        SectionData::UsersGroups(s) => (s, degraded),
        other => panic!("expected SectionData::UsersGroups, got {:?}", other),
    }
}

// ── Tests ───────────────────────────────────────────────────────────

/// Happy path: full population with both auto-detect strategies.
#[test]
fn test_users_inspector_happy_path() {
    let exec = users_happy_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let result = UsersGroupsInspector::new().inspect(&ctx, &NullProgress);

    let (section, _degraded) = extract_section(result);

    // passwd has 5 users: alice (1000), bob (1001), charlie (1002),
    // daemon (1003), webapp (1004) — all in non-system UID range.
    assert!(
        !section.users.is_empty(),
        "inspector must find users from passwd fixture"
    );
    assert_eq!(section.users.len(), 5, "fixture has 5 non-system users");

    // Groups
    assert!(
        !section.groups.is_empty(),
        "inspector must find groups from group fixture"
    );

    // Shadow entries
    assert!(
        !section.shadow_entries.is_empty(),
        "inspector must extract shadow entries"
    );

    // Gshadow entries (only non-system groups with actual admin/member data)
    // wheel and docker have members/admins, the rest have empty fields.
    assert!(
        !section.gshadow_entries.is_empty(),
        "inspector must extract gshadow entries for groups with members"
    );

    // subuid/subgid
    assert!(
        !section.subuid_entries.is_empty(),
        "inspector must extract subuid entries"
    );
    assert!(
        !section.subgid_entries.is_empty(),
        "inspector must extract subgid entries"
    );

    // Sudoers rules
    assert!(
        !section.sudoers_rules.is_empty(),
        "inspector must extract sudoers rules"
    );

    // SSH key refs
    assert!(
        !section.ssh_authorized_keys_refs.is_empty(),
        "inspector must find SSH key refs for alice"
    );
    let alice_ref = &section.ssh_authorized_keys_refs[0];
    assert_eq!(alice_ref["user"], "alice");
    // Must have key count, not key content.
    assert!(
        alice_ref.get("key_count").is_some(),
        "SSH ref must include key_count"
    );
    let json = serde_json::to_string(alice_ref).unwrap();
    assert!(
        !json.contains("AAAAB3"),
        "SSH ref must not contain key material"
    );
}

/// Shadow entries contain no hash patterns ($6$, $y$, etc.).
#[test]
fn test_users_inspector_shadow_strips_hashes() {
    let exec = users_happy_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let result = UsersGroupsInspector::new().inspect(&ctx, &NullProgress);
    let (section, _) = extract_section(result);

    // Verify no shadow entry contains a raw hash.
    let all_shadow = section.shadow_entries.join("\n");
    assert!(
        !all_shadow.contains("$6$"),
        "shadow entries must not contain $6$ (SHA-512 hash)"
    );
    assert!(
        !all_shadow.contains("$y$"),
        "shadow entries must not contain $y$ (yescrypt hash)"
    );
    assert!(
        !all_shadow.contains("$5$"),
        "shadow entries must not contain $5$ (SHA-256 hash)"
    );

    // Also verify individual entries have a status string, not a hash field.
    for entry in &section.shadow_entries {
        let parts: Vec<&str> = entry.split(':').collect();
        assert!(
            parts.len() >= 2,
            "shadow entry must have at least username:status"
        );
        let status = parts[1];
        // Status should be one of: locked, disabled, set, none — not a hash.
        assert!(
            !status.starts_with('$'),
            "shadow status field must not be a hash: {entry}"
        );
    }
}

/// Gshadow entries contain no password hash patterns.
#[test]
fn test_users_inspector_gshadow_strips_passwords() {
    let exec = users_happy_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let result = UsersGroupsInspector::new().inspect(&ctx, &NullProgress);
    let (section, _) = extract_section(result);

    let all_gshadow = section.gshadow_entries.join("\n");
    assert!(
        !all_gshadow.contains("$6$"),
        "gshadow entries must not contain $6$ (SHA-512 hash)"
    );
    assert!(
        !all_gshadow.contains("$y$"),
        "gshadow entries must not contain $y$ (yescrypt hash)"
    );
    assert!(
        !all_gshadow.contains("$5$"),
        "gshadow entries must not contain $5$ (SHA-256 hash)"
    );
}

/// Classification: valid login shell -> interactive, nologin/false -> non-interactive.
/// Also verifies default containerfile_strategy and password_choice fields.
#[test]
fn test_users_inspector_classification() {
    let exec = MockExecutor::new().with_file(
        "/etc/passwd",
        "alice:x:1000:1000:Alice:/home/alice:/bin/bash\n\
         bob:x:1001:1001:Bob:/home/bob:/usr/sbin/nologin\n\
         charlie:x:1002:1002:Charlie:/home/charlie:/bin/zsh\n\
         daemon:x:1003:1003:Daemon:/srv/daemon:/bin/false\n\
         custom:x:1004:1004:Custom:/home/custom:/usr/local/bin/custom\n",
    );

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let result = UsersGroupsInspector::new().inspect(&ctx, &NullProgress);
    let (section, _) = extract_section(result);

    // Build a name -> classification map.
    let classifications: std::collections::HashMap<String, String> = section
        .users
        .iter()
        .filter_map(|u| {
            let name = u.get("name")?.as_str()?.to_string();
            let classification = u.get("classification")?.as_str()?.to_string();
            Some((name, classification))
        })
        .collect();

    // Valid login shells -> interactive
    assert_eq!(
        classifications.get("alice").map(|s| s.as_str()),
        Some("interactive"),
        "alice (/bin/bash) should be interactive"
    );
    assert_eq!(
        classifications.get("charlie").map(|s| s.as_str()),
        Some("interactive"),
        "charlie (/bin/zsh) should be interactive"
    );

    // Nologin, false, unknown shells -> non-interactive
    assert_eq!(
        classifications.get("bob").map(|s| s.as_str()),
        Some("non-interactive"),
        "bob (/usr/sbin/nologin) should be non-interactive"
    );
    assert_eq!(
        classifications.get("daemon").map(|s| s.as_str()),
        Some("non-interactive"),
        "daemon (/bin/false) should be non-interactive"
    );
    assert_eq!(
        classifications.get("custom").map(|s| s.as_str()),
        Some("non-interactive"),
        "custom (/usr/local/bin/custom) should be non-interactive (unknown shell)"
    );

    // All users should have default containerfile_strategy and password_choice.
    for user in &section.users {
        let name = user.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        assert_eq!(
            user.get("containerfile_strategy").and_then(|v| v.as_str()),
            Some("skip"),
            "{name} should have containerfile_strategy=skip"
        );
        assert_eq!(
            user.get("password_choice").and_then(|v| v.as_str()),
            Some("none"),
            "{name} should have password_choice=none"
        );
    }
}

/// Groups no longer carry a strategy field.
#[test]
fn test_users_inspector_groups_no_strategy() {
    let exec = MockExecutor::new()
        .with_file(
            "/etc/passwd",
            "alice:x:1000:1000:Alice:/home/alice:/bin/bash\n\
             daemon:x:1001:1001:Daemon:/srv/daemon:/sbin/nologin\n",
        )
        .with_file(
            "/etc/group",
            "alice:x:1000:\n\
             daemon:x:1001:\n",
        );

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let result = UsersGroupsInspector::new().inspect(&ctx, &NullProgress);
    let (section, _) = extract_section(result);

    for group in &section.groups {
        let name = group.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        assert!(
            group.get("strategy").is_none(),
            "{name} group should not have a strategy field"
        );
    }
}

/// PermissionDenied on /etc/shadow -> Degraded output.
#[test]
fn test_users_inspector_degraded_shadow() {
    let exec = MockExecutor::new()
        .with_file(
            "/etc/passwd",
            "alice:x:1000:1000:Alice:/home/alice:/bin/bash\n",
        )
        .with_file_error("/etc/shadow", std::io::ErrorKind::PermissionDenied)
        .with_file("/etc/group", "alice:x:1000:\n");

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let result = UsersGroupsInspector::new().inspect(&ctx, &NullProgress);

    match result {
        Err(InspectorError::Degraded { partial, reason }) => {
            assert!(
                reason.contains("permission denied"),
                "degraded reason should mention permission denied, got: {reason}"
            );
            assert!(
                matches!(partial.section, SectionData::UsersGroups(_)),
                "partial output should still contain a UserGroupSection"
            );
            // Even when degraded, users should be populated from passwd.
            if let SectionData::UsersGroups(section) = &partial.section {
                assert!(
                    !section.users.is_empty(),
                    "degraded output should still contain users from passwd"
                );
            }
        }
        Ok(_) => panic!("expected Degraded error when /etc/shadow is PermissionDenied"),
        Err(e) => panic!("expected Degraded, got: {e}"),
    }
}

/// Output round-trips through UserGroupSection type.
#[test]
fn test_users_inspector_json_roundtrip() {
    let exec = users_happy_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let result = UsersGroupsInspector::new().inspect(&ctx, &NullProgress);

    let (section, _) = extract_section(result);

    let rust_json = serde_json::to_string_pretty(&section).unwrap();
    let roundtrip: UserGroupSection =
        serde_json::from_str(&rust_json).expect("inspector output must be valid JSON");
    let roundtrip_json = serde_json::to_string_pretty(&roundtrip).unwrap();
    assert_eq!(
        rust_json, roundtrip_json,
        "inspector output must round-trip faithfully through UserGroupSection"
    );
}
