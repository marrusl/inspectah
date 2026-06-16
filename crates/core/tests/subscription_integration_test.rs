//! Integration tests for subscription data across aggregate merge boundaries.
//!
//! Validates typed timestamp comparison, hostname tiebreak, mixed-presence
//! handling, and the sensitive_snapshot / preserved_subscription flags.

use inspectah_core::aggregate::merge_snapshots;
use inspectah_core::snapshot::{InspectionSnapshot, SCHEMA_VERSION};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::subscription::{SubscriptionFile, SubscriptionSection};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a minimal snapshot that passes aggregate validation.
fn valid_snap(hostname: &str) -> InspectionSnapshot {
    let mut s = InspectionSnapshot::new();
    s.schema_version = SCHEMA_VERSION;
    s.meta
        .insert("hostname".into(), serde_json::json!(hostname));
    s.os_release = Some(OsRelease {
        version_id: "9.4".into(),
        ..Default::default()
    });
    s
}

/// Attach subscription data with a specific expiry timestamp.
fn with_subscription(
    snap: &mut InspectionSnapshot,
    hostname: &str,
    serial: &str,
    expiry_unix: i64,
) {
    let expiry = time::OffsetDateTime::from_unix_timestamp(expiry_unix)
        .expect("valid unix timestamp for test");
    snap.preserved_subscription = true;
    snap.sensitive_snapshot = true;
    snap.subscription = Some(SubscriptionSection {
        entitlement_certs: vec![
            SubscriptionFile {
                path: format!("/etc/pki/entitlement/{serial}.pem"),
                content: "cert-data".into(),
                size_bytes: 100,
                cert_expiry: Some(expiry),
            },
            SubscriptionFile {
                path: format!("/etc/pki/entitlement/{serial}-key.pem"),
                content: "key-data".into(),
                size_bytes: 80,
                cert_expiry: None,
            },
        ],
        ca_certs: vec![SubscriptionFile {
            path: "/etc/rhsm/ca/redhat-uep.pem".into(),
            content: "ca-data".into(),
            size_bytes: 50,
            cert_expiry: None,
        }],
        config_files: vec![
            SubscriptionFile {
                path: "/etc/rhsm/rhsm.conf".into(),
                content: "[rhsm]\nbaseurl = https://cdn.redhat.com".into(),
                size_bytes: 40,
                cert_expiry: None,
            },
            SubscriptionFile {
                path: "/etc/yum.repos.d/redhat.repo".into(),
                content: "[rhel-base]\nenabled=1".into(),
                size_bytes: 25,
                cert_expiry: None,
            },
        ],
        earliest_expiry: Some(expiry),
        incomplete: false,
        org_id: Some("12345".into()),
        system_uuid: Some(format!("uuid-{hostname}")),
        rhsm_server: Some("subscription.rhsm.redhat.com".into()),
        source_hostname: Some(hostname.into()),
    });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Aggregate merge picks the subscription with the latest expiry date.
/// Uses typed OffsetDateTime comparison, not string comparison.
#[test]
fn aggregate_merge_picks_latest_expiry() {
    let early_ts = 1_719_792_000; // 2024-07-01
    let late_ts = 1_735_689_600; // 2025-01-01

    let mut snap_a = valid_snap("host-a");
    with_subscription(&mut snap_a, "host-a", "111", early_ts);

    let mut snap_b = valid_snap("host-b");
    with_subscription(&mut snap_b, "host-b", "222", late_ts);

    let (merged, warnings) =
        merge_snapshots(vec![snap_a, snap_b], None).expect("merge should succeed");

    assert!(
        warnings.is_empty()
            || warnings.iter().all(|w| !matches!(
                w,
                inspectah_core::aggregate::validate::AggregateWarning::BaselineConflict { .. }
            ))
    );
    assert!(merged.preserved_subscription);
    assert!(merged.sensitive_snapshot);

    let sub = merged.subscription.expect("should have subscription");
    assert_eq!(sub.source_hostname.as_deref(), Some("host-b"));
    assert_eq!(
        sub.earliest_expiry,
        Some(time::OffsetDateTime::from_unix_timestamp(late_ts).unwrap())
    );
}

/// When two snapshots have identical expiry timestamps, the merge picks
/// the one with the lexicographically first hostname.
#[test]
fn aggregate_merge_subscription_hostname_tiebreak_on_equal_expiry() {
    let same_ts = 1_725_148_800;

    let mut snap_z = valid_snap("host-zebra");
    with_subscription(&mut snap_z, "host-zebra", "999", same_ts);

    let mut snap_a = valid_snap("host-alpha");
    with_subscription(&mut snap_a, "host-alpha", "888", same_ts);

    let (merged, _) = merge_snapshots(vec![snap_z, snap_a], None).expect("merge should succeed");

    let sub = merged.subscription.expect("should have subscription");
    assert_eq!(
        sub.source_hostname.as_deref(),
        Some("host-alpha"),
        "alphabetically first hostname wins on equal expiry"
    );
}

/// When only some hosts have subscription data, the merge includes it
/// from whichever host has it and ORs the boolean flags.
#[test]
fn aggregate_merge_subscription_mixed_presence() {
    let ts = 1_725_148_800;

    // host-a: no subscription
    let snap_a = valid_snap("host-a");

    // host-b: has subscription
    let mut snap_b = valid_snap("host-b");
    with_subscription(&mut snap_b, "host-b", "333", ts);

    // host-c: no subscription
    let snap_c = valid_snap("host-c");

    let (merged, _) =
        merge_snapshots(vec![snap_a, snap_b, snap_c], None).expect("merge should succeed");

    assert!(merged.preserved_subscription, "OR across hosts");
    assert!(merged.sensitive_snapshot, "OR across hosts");
    assert!(merged.subscription.is_some());
    let sub = merged.subscription.unwrap();
    assert_eq!(sub.source_hostname.as_deref(), Some("host-b"));
}

/// Incomplete subscription sections are excluded from winner selection.
/// Only complete (non-incomplete) sections participate.
#[test]
fn aggregate_merge_subscription_skips_incomplete() {
    let early_ts = 1_719_792_000;
    let late_ts = 1_735_689_600;

    // host-a: late expiry but incomplete
    let mut snap_a = valid_snap("host-a");
    with_subscription(&mut snap_a, "host-a", "111", late_ts);
    snap_a.subscription.as_mut().unwrap().incomplete = true;

    // host-b: earlier expiry but complete
    let mut snap_b = valid_snap("host-b");
    with_subscription(&mut snap_b, "host-b", "222", early_ts);

    let (merged, _) = merge_snapshots(vec![snap_a, snap_b], None).expect("merge should succeed");

    let sub = merged.subscription.expect("should have subscription");
    // host-b wins because host-a is incomplete
    assert_eq!(sub.source_hostname.as_deref(), Some("host-b"));
    assert_eq!(
        sub.earliest_expiry,
        Some(time::OffsetDateTime::from_unix_timestamp(early_ts).unwrap())
    );
}

/// Subscription data includes serial-matched entitlement pairs from the
/// winning host's section.
#[test]
fn aggregate_merge_subscription_carries_entitlement_pairs() {
    let ts = 1_725_148_800;

    let mut snap = valid_snap("host-a");
    with_subscription(&mut snap, "host-a", "555", ts);

    // Add a second host with no subscription to trigger aggregate merge
    let snap_b = valid_snap("host-b");

    let (merged, _) = merge_snapshots(vec![snap, snap_b], None).expect("merge should succeed");

    let sub = merged.subscription.expect("should have subscription");
    assert_eq!(sub.entitlement_certs.len(), 2, "cert + key");
    assert!(
        sub.entitlement_certs
            .iter()
            .any(|f| f.path.contains("555.pem"))
    );
    assert!(
        sub.entitlement_certs
            .iter()
            .any(|f| f.path.contains("555-key.pem"))
    );

    // Verify serial matching works on the merged data
    let (pairs, orphans) =
        inspectah_core::types::subscription::match_entitlement_pairs(&sub.entitlement_certs);
    assert_eq!(pairs.len(), 1, "one matched pair");
    assert!(orphans.is_empty());
    assert_eq!(pairs[0].serial, "555");
    assert!(pairs[0].is_complete());
}

/// Snapshot roundtrip: subscription data survives serialize -> deserialize.
#[test]
fn subscription_snapshot_roundtrip() {
    let ts = 1_725_148_800;
    let mut snap = valid_snap("host-a");
    with_subscription(&mut snap, "host-a", "777", ts);

    let json = serde_json::to_string_pretty(&snap).expect("serialize");
    let parsed = InspectionSnapshot::load(&json).expect("deserialize");

    assert!(parsed.subscription.is_some());
    assert!(parsed.preserved_subscription);
    assert!(parsed.sensitive_snapshot);

    let sub = parsed.subscription.unwrap();
    assert_eq!(sub.source_hostname.as_deref(), Some("host-a"));
    assert_eq!(
        sub.earliest_expiry,
        Some(time::OffsetDateTime::from_unix_timestamp(ts).unwrap())
    );
    assert_eq!(sub.org_id.as_deref(), Some("12345"));
    assert_eq!(sub.entitlement_certs.len(), 2);
    assert_eq!(sub.ca_certs.len(), 1);
    assert_eq!(sub.config_files.len(), 2);
}

/// Three hosts with distinct expiry dates: merge picks the latest.
#[test]
fn aggregate_merge_three_hosts_picks_latest() {
    let ts1 = 1_719_792_000; // 2024-07-01
    let ts2 = 1_725_148_800; // 2024-09-01
    let ts3 = 1_735_689_600; // 2025-01-01

    let mut snap_a = valid_snap("host-a");
    with_subscription(&mut snap_a, "host-a", "111", ts1);

    let mut snap_b = valid_snap("host-b");
    with_subscription(&mut snap_b, "host-b", "222", ts2);

    let mut snap_c = valid_snap("host-c");
    with_subscription(&mut snap_c, "host-c", "333", ts3);

    let (merged, _) =
        merge_snapshots(vec![snap_a, snap_b, snap_c], None).expect("merge should succeed");

    let sub = merged.subscription.expect("should have subscription");
    assert_eq!(sub.source_hostname.as_deref(), Some("host-c"));
    assert_eq!(
        sub.earliest_expiry,
        Some(time::OffsetDateTime::from_unix_timestamp(ts3).unwrap())
    );
}
