use inspectah_refine::types::PackageTarget;
use inspectah_refine::types::{AnnotatedOp, ItemId, RefinementOp};

#[test]
fn package_target_serde_roundtrip() {
    let target = PackageTarget {
        name: "httpd".into(),
        arch: "x86_64".into(),
    };
    let json = serde_json::to_string(&target).unwrap();
    let parsed: PackageTarget = serde_json::from_str(&json).unwrap();
    assert_eq!(target, parsed);
}

#[test]
fn package_target_display() {
    let target = PackageTarget {
        name: "glibc".into(),
        arch: "i686".into(),
    };
    assert_eq!(format!("{target}"), "glibc.i686");
}

#[test]
fn set_include_exclude_package_json() {
    let op = RefinementOp::SetInclude {
        item_id: ItemId::Package {
            name: "httpd".into(),
            arch: "x86_64".into(),
        },
        include: false,
    };
    let json = serde_json::to_string(&op).unwrap();
    assert!(json.contains(r#""op":"SetInclude""#));
    assert!(json.contains(r#""name":"httpd""#));
    assert!(json.contains(r#""arch":"x86_64""#));
    let parsed: RefinementOp = serde_json::from_str(&json).unwrap();
    assert_eq!(op, parsed);
}

#[test]
fn set_include_exclude_config_json() {
    let op = RefinementOp::SetInclude {
        item_id: ItemId::Config {
            path: "/etc/httpd/conf/httpd.conf".into(),
        },
        include: false,
    };
    let json = serde_json::to_string(&op).unwrap();
    let parsed: RefinementOp = serde_json::from_str(&json).unwrap();
    assert_eq!(op, parsed);
}

#[test]
fn annotated_op_json_flattens() {
    let aop = AnnotatedOp {
        op: RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "vim".into(),
                arch: "x86_64".into(),
            },
            include: false,
        },
        active: true,
    };
    let json = serde_json::to_string(&aop).unwrap();
    assert!(json.contains(r#""op":"SetInclude""#));
    assert!(json.contains(r#""active":true"#));
}

#[test]
fn triage_reason_custom_variant() {
    use inspectah_refine::types::TriageReason;
    let reason = TriageReason::Custom("aggregate-uncommon".into());
    let json = serde_json::to_string(&reason).unwrap();
    let parsed: TriageReason = serde_json::from_str(&json).unwrap();
    assert_eq!(reason, parsed);
}
