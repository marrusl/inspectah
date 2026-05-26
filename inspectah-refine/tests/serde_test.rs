use inspectah_refine::types::{AttentionReason, ItemId, RefinementOp, RepoProvenance};

#[test]
fn attention_reason_custom_serialization() {
    let reason = AttentionReason::Custom("detail".to_string());
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
fn test_new_attention_reasons_roundtrip() {
    let reasons = vec![
        AttentionReason::PackageBaselineMatch,
        AttentionReason::PackageUserAdded,
        AttentionReason::PackageVersionChanged,
        AttentionReason::PackageProvenanceUnavailable,
        AttentionReason::PackageNoRepoSource,
        AttentionReason::ConfigDefault,
        AttentionReason::ConfigBaselineMatch,
    ];
    for reason in &reasons {
        let json = serde_json::to_string(reason).unwrap();
        let parsed: AttentionReason = serde_json::from_str(&json).unwrap();
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
