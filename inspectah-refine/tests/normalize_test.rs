#[test]
fn omitted_include_defaults_to_true() {
    let json = r#"{"schema_version": 14, "rpm": {"packages_added": [{"name": "httpd", "arch": "x86_64", "state": "added"}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        snap.rpm.as_ref().unwrap().packages_added[0].include,
        "omitted include must default to true"
    );
}

#[test]
fn explicit_false_preserved() {
    let json = r#"{"schema_version": 14, "rpm": {"packages_added": [{"name": "httpd", "arch": "x86_64", "state": "added", "include": false}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        !snap.rpm.as_ref().unwrap().packages_added[0].include,
        "explicit include: false must be preserved"
    );
}

#[test]
fn explicit_true_preserved() {
    let json = r#"{"schema_version": 14, "rpm": {"packages_added": [{"name": "httpd", "arch": "x86_64", "state": "added", "include": true}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(snap.rpm.as_ref().unwrap().packages_added[0].include);
}

#[test]
fn omitted_config_include_defaults_to_true() {
    let json = r#"{"schema_version": 14, "config": {"files": [{"path": "/etc/httpd/conf/httpd.conf", "kind": "rpm_owned_modified", "category": "other"}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        snap.config.as_ref().unwrap().files[0].include,
        "omitted config include must default to true"
    );
}

#[test]
fn explicit_config_false_preserved() {
    let json = r#"{"schema_version": 14, "config": {"files": [{"path": "/etc/httpd/conf/httpd.conf", "kind": "rpm_owned_modified", "category": "other", "include": false}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        !snap.config.as_ref().unwrap().files[0].include,
        "explicit config include: false must be preserved"
    );
}

#[test]
fn base_image_only_include_false_preserved() {
    let json = r#"{"schema_version": 14, "rpm": {"packages_added": [], "base_image_only": [{"name": "kernel", "arch": "x86_64", "state": "base_image_only", "include": false}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(!snap.rpm.as_ref().unwrap().base_image_only[0].include);
}

#[test]
fn base_image_only_omitted_include_defaults_true() {
    let json = r#"{"schema_version": 14, "rpm": {"packages_added": [], "base_image_only": [{"name": "kernel", "arch": "x86_64", "state": "base_image_only"}]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(
        snap.rpm.as_ref().unwrap().base_image_only[0].include,
        "omitted base_image_only include must default to true"
    );
}

#[test]
fn mixed_present_and_absent_includes() {
    let json = r#"{"schema_version": 14, "rpm": {"packages_added": [
        {"name": "httpd", "arch": "x86_64", "state": "added", "include": false},
        {"name": "vim", "arch": "x86_64", "state": "added", "include": true},
        {"name": "curl", "arch": "x86_64", "state": "added"}
    ]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    let pkgs = &snap.rpm.as_ref().unwrap().packages_added;
    assert!(!pkgs[0].include, "httpd: explicit false preserved");
    assert!(pkgs[1].include, "vim: explicit true preserved");
    assert!(pkgs[2].include, "curl: omitted defaulted to true");
}

#[test]
fn empty_snapshot_loads() {
    let json = r#"{"schema_version": 14}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    assert!(snap.rpm.is_none());
    assert!(snap.config.is_none());
}

#[test]
fn go_emitted_snapshot_roundtrip() {
    let json = r#"{"schema_version": 14, "rpm": {"packages_added": [
        {"name": "httpd", "arch": "x86_64", "state": "added", "include": true},
        {"name": "vim", "arch": "x86_64", "state": "added", "include": true}
    ], "base_image_only": [
        {"name": "kernel", "arch": "x86_64", "state": "base_image_only", "include": false}
    ]}, "config": {"files": [
        {"path": "/etc/httpd/conf/httpd.conf", "kind": "rpm_owned_modified", "category": "other", "include": true}
    ]}}"#;
    let snap = inspectah_refine::normalize::load_for_refine(json).unwrap();
    let rpm = snap.rpm.as_ref().unwrap();
    assert!(rpm.packages_added[0].include);
    assert!(rpm.packages_added[1].include);
    assert!(!rpm.base_image_only[0].include);
    assert!(snap.config.as_ref().unwrap().files[0].include);
}
