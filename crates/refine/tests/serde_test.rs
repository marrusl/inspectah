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
