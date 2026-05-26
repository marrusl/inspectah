use inspectah_refine::types::{ContentHash, ItemId, RefinementOp};

#[test]
fn content_hash_valid_64_hex() {
    let hash = ContentHash::new("a".repeat(64)).unwrap();
    assert_eq!(hash.as_str(), "a".repeat(64));
}

#[test]
fn content_hash_rejects_63_chars() {
    assert!(ContentHash::new("a".repeat(63)).is_err());
}

#[test]
fn content_hash_rejects_non_hex() {
    assert!(ContentHash::new("z".repeat(64)).is_err());
}

#[test]
fn content_hash_from_content_produces_valid_hash() {
    let hash = ContentHash::from_content(b"hello world");
    assert_eq!(hash.as_str().len(), 64);
    assert!(hash.as_str().chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn content_hash_serde_roundtrip() {
    let hash = ContentHash::from_content(b"test");
    let json = serde_json::to_string(&hash).unwrap();
    let parsed: ContentHash = serde_json::from_str(&json).unwrap();
    assert_eq!(hash, parsed);
}

#[test]
fn content_hash_ord_for_btreemap() {
    use std::collections::BTreeMap;
    let mut map = BTreeMap::new();
    let h1 = ContentHash::from_content(b"aaa");
    let h2 = ContentHash::from_content(b"bbb");
    map.insert(h1.clone(), "first");
    map.insert(h2.clone(), "second");
    assert_eq!(map.len(), 2);
}

#[test]
fn item_id_config_serde_roundtrip() {
    let id = ItemId::Config {
        path: "/etc/nginx/nginx.conf".into(),
    };
    let json = serde_json::to_string(&id).unwrap();
    assert!(json.contains("Config"));
    let parsed: ItemId = serde_json::from_str(&json).unwrap();
    assert_eq!(id, parsed);
}

#[test]
fn item_id_package_serde_roundtrip() {
    let id = ItemId::Package {
        name: "httpd".into(),
        arch: "x86_64".into(),
    };
    let json = serde_json::to_string(&id).unwrap();
    let parsed: ItemId = serde_json::from_str(&json).unwrap();
    assert_eq!(id, parsed);
}

#[test]
fn select_variant_op_serde() {
    let hash = ContentHash::from_content(b"variant content");
    let op = RefinementOp::SelectVariant {
        item_id: ItemId::Config {
            path: "/etc/test.conf".into(),
        },
        target: hash,
    };
    let json = serde_json::to_string(&op).unwrap();
    let parsed: RefinementOp = serde_json::from_str(&json).unwrap();
    assert_eq!(op, parsed);
}

#[test]
fn edit_variant_op_serde() {
    let op = RefinementOp::EditVariant {
        item_id: ItemId::DropIn {
            path: "/etc/systemd/system/httpd.service.d/override.conf".into(),
        },
        content: "new content".into(),
        based_on: None,
    };
    let json = serde_json::to_string(&op).unwrap();
    let parsed: RefinementOp = serde_json::from_str(&json).unwrap();
    assert_eq!(op, parsed);
}

#[test]
fn discard_variant_op_serde() {
    let hash = ContentHash::from_content(b"discard me");
    let op = RefinementOp::DiscardVariant {
        item_id: ItemId::Config {
            path: "/etc/test.conf".into(),
        },
        variant: hash,
    };
    let json = serde_json::to_string(&op).unwrap();
    let parsed: RefinementOp = serde_json::from_str(&json).unwrap();
    assert_eq!(op, parsed);
}
