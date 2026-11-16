use inspectah_refine::types::{ItemId, RefinementOp, RepoProvenance, TriageReason};

#[test]
fn triage_reason_custom_serialization() {
    let reason = TriageReason::Custom("detail".to_string());
    let json = serde_json::to_string(&reason).unwrap();
    println!("Custom variant JSON: {}", json);
    assert!(json.contains("custom"));
}

#[test]
fn test_set_include_repo_op_roundtrip() {
    let op = RefinementOp::SetInclude {
        item_id: ItemId::Repo {
            path: "epel".into(),
        },
        include: false,
    };
    let json = serde_json::to_string(&op).unwrap();
    let parsed: RefinementOp = serde_json::from_str(&json).unwrap();
    assert_eq!(op, parsed);
}

#[test]
fn test_triage_reasons_roundtrip() {
    let reasons = vec![
        TriageReason::PackageBaselineMatch,
        TriageReason::PackageUserAdded,
        TriageReason::PackageVersionChanged,
        TriageReason::PackageProvenanceUnavailable,
        TriageReason::PackageNoRepoSource,
        TriageReason::ConfigDefault,
        TriageReason::ConfigBaselineMatch,
    ];
    for reason in &reasons {
        let json = serde_json::to_string(reason).unwrap();
        let parsed: TriageReason = serde_json::from_str(&json).unwrap();
        assert_eq!(*reason, parsed);
    }
}

#[test]
fn test_repo_provenance_roundtrip() {
    for prov in &[
        RepoProvenance::Verified,
        RepoProvenance::Incomplete,
        RepoProvenance::Unknown,
    ] {
        let json = serde_json::to_string(prov).unwrap();
        let parsed: RepoProvenance = serde_json::from_str(&json).unwrap();
        assert_eq!(*prov, parsed);
    }
}

/// A unit row's id is `{"kind":"Unit","key":{"unit":...}}` and survives a
/// round trip unchanged.
#[test]
fn item_id_unit_round_trips() {
    let wire = serde_json::json!({"kind": "Unit", "key": {"unit": "sshd.service"}});
    let id: ItemId = serde_json::from_value(wire.clone()).expect("a `Unit` id must deserialize");
    assert_eq!(serde_json::to_value(&id).unwrap(), wire);

    let op = RefinementOp::SetInclude {
        item_id: id,
        include: false,
    };
    let back: RefinementOp = serde_json::from_str(&serde_json::to_string(&op).unwrap()).unwrap();
    assert_eq!(op, back);
}

/// The old `Service` kind is refused with no alias, and the refusal names
/// it. The same key under the `Unit` kind is the positive control.
#[test]
fn item_id_service_kind_is_refused() {
    let err = serde_json::from_value::<ItemId>(
        serde_json::json!({"kind": "Service", "key": {"unit": "sshd.service"}}),
    )
    .expect_err("the `Service` kind must be refused");
    assert!(
        err.to_string().contains("`Service`"),
        "the refusal must name the refused kind, got: {err}"
    );

    serde_json::from_value::<ItemId>(
        serde_json::json!({"kind": "Unit", "key": {"unit": "sshd.service"}}),
    )
    .expect("the same key under the `Unit` kind must be accepted");
}

/// A unit name the unit type refuses is refused in an op too.
#[test]
fn item_id_unit_refuses_an_invalid_unit_name() {
    serde_json::from_value::<ItemId>(
        serde_json::json!({"kind": "Unit", "key": {"unit": "sshd.service"}}),
    )
    .expect("positive control: a valid unit name is accepted");
    let err = serde_json::from_value::<ItemId>(
        serde_json::json!({"kind": "Unit", "key": {"unit": "sshd.device"}}),
    )
    .expect_err("a unit name with no known type suffix must be refused");
    assert!(
        err.to_string().contains("sshd.device"),
        "the refusal must name the refused unit, got: {err}"
    );
}

/// § 11.1: `ItemId::SystemdTimer` is removed. A timer's description row
/// controls nothing, so a request or a saved op naming that kind is
/// refused when it is read, and the refusal names the kind. The same key
/// under the `GeneratedTimer` kind, a timer row that still has a control,
/// is the positive control.
#[test]
fn item_id_systemd_timer_kind_is_refused() {
    let err = serde_json::from_value::<RefinementOp>(serde_json::json!({
        "op": "SetInclude",
        "target": {
            "item_id": {"kind": "SystemdTimer", "key": {"name": "backup"}},
            "include": false
        }
    }))
    .expect_err("an op naming the `SystemdTimer` kind must be refused");
    assert!(
        err.to_string().contains("`SystemdTimer`"),
        "the refusal must name the refused kind, got: {err}"
    );

    serde_json::from_value::<RefinementOp>(serde_json::json!({
        "op": "SetInclude",
        "target": {
            "item_id": {"kind": "GeneratedTimer", "key": {"name": "backup"}},
            "include": false
        }
    }))
    .expect("the same key under the `GeneratedTimer` kind must be accepted");
}
