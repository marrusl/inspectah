use inspectah_refine::types::PackageTarget;
use inspectah_refine::types::{RefinementOp, AnnotatedOp, AttentionLevel, AttentionReason, AttentionTag};
use std::path::PathBuf;

#[test]
fn package_target_serde_roundtrip() {
    let target = PackageTarget { name: "httpd".into(), arch: "x86_64".into() };
    let json = serde_json::to_string(&target).unwrap();
    let parsed: PackageTarget = serde_json::from_str(&json).unwrap();
    assert_eq!(target, parsed);
}

#[test]
fn package_target_display() {
    let target = PackageTarget { name: "glibc".into(), arch: "i686".into() };
    assert_eq!(format!("{target}"), "glibc.i686");
}

#[test]
fn refinement_op_exclude_package_json() {
    let op = RefinementOp::ExcludePackage(PackageTarget { name: "httpd".into(), arch: "x86_64".into() });
    let json = serde_json::to_string(&op).unwrap();
    assert!(json.contains(r#""op":"ExcludePackage""#));
    assert!(json.contains(r#""name":"httpd""#));
    assert!(json.contains(r#""arch":"x86_64""#));
    let parsed: RefinementOp = serde_json::from_str(&json).unwrap();
    assert_eq!(op, parsed);
}

#[test]
fn refinement_op_exclude_config_json() {
    let op = RefinementOp::ExcludeConfig { path: PathBuf::from("/etc/httpd/conf/httpd.conf") };
    let json = serde_json::to_string(&op).unwrap();
    let parsed: RefinementOp = serde_json::from_str(&json).unwrap();
    assert_eq!(op, parsed);
}

#[test]
fn annotated_op_json_flattens() {
    let aop = AnnotatedOp {
        op: RefinementOp::ExcludePackage(PackageTarget { name: "vim".into(), arch: "x86_64".into() }),
        active: true,
    };
    let json = serde_json::to_string(&aop).unwrap();
    assert!(json.contains(r#""op":"ExcludePackage""#));
    assert!(json.contains(r#""active":true"#));
}

#[test]
fn attention_tag_serde() {
    let tag = AttentionTag {
        level: AttentionLevel::NeedsReview,
        reason: AttentionReason::ConfigModified,
        detail: Some("RPM-owned config was modified".into()),
    };
    let json = serde_json::to_string(&tag).unwrap();
    let parsed: AttentionTag = serde_json::from_str(&json).unwrap();
    assert_eq!(tag, parsed);
}

#[test]
fn attention_reason_custom_variant() {
    let reason = AttentionReason::Custom("fleet-uncommon".into());
    let json = serde_json::to_string(&reason).unwrap();
    let parsed: AttentionReason = serde_json::from_str(&json).unwrap();
    assert_eq!(reason, parsed);
}
