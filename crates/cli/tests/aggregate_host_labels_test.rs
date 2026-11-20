//! Host labels in an aggregate, from collection to the rendered audit.
//!
//! An unnamed host's label is in angle brackets, which Markdown reads as
//! HTML when written bare, so the audit must render every host label as
//! literal text.

use std::collections::BTreeMap;

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Arena, Options, parse_document};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::aggregate::AggregateSnapshotMeta;
use inspectah_pipeline::render::audit::render_audit;

/// A node's text, with its markup dropped.
fn text_of<'a>(node: &'a AstNode<'a>) -> String {
    node.descendants()
        .filter_map(|n| match &n.data.borrow().value {
            NodeValue::Text(text) => Some(text.to_string()),
            _ => None,
        })
        .collect()
}

/// The audit's `### Hosts` list as a CommonMark parser reads it: each
/// item's code span, as its literal. Panics if an item holds anything
/// but one code span: plain text, which Markdown may reinterpret, or
/// HTML, which a reader never sees.
fn parsed_host_list(md: &str) -> Vec<String> {
    let arena = Arena::new();
    let root = parse_document(&arena, md, &Options::default());
    let heading = root
        .children()
        .find(|n| {
            matches!(&n.data.borrow().value, NodeValue::Heading(h) if h.level == 3)
                && text_of(n) == "Hosts"
        })
        .expect("a `Hosts` heading");
    let list = heading.next_sibling().expect("a list after the heading");
    assert!(
        matches!(list.data.borrow().value, NodeValue::List(_)),
        "the hosts are a list"
    );
    list.children()
        .map(|item| {
            let inlines: Vec<NodeValue> = item
                .descendants()
                .skip(1)
                .map(|n| n.data.borrow().value.clone())
                .filter(|v| !matches!(v, NodeValue::Paragraph))
                .collect();
            match inlines.as_slice() {
                [NodeValue::Code(code)] => code.literal.clone(),
                other => panic!("a host item is one code span, not {other:?}"),
            }
        })
        .collect()
}

/// Every host label reaches the audit as literal text, angle brackets
/// and all. Written bare, `<unnamed>` is an HTML tag and disappears.
#[test]
fn audit_renders_host_labels_literally() {
    let hostnames = vec![
        "<unnamed>".to_string(),
        "web-01".to_string(),
        "<unnamed 1>".to_string(),
        "<unnamed 2, scanned 2026-11-19T10:00:00+00:00>".to_string(),
    ];
    let snap = InspectionSnapshot {
        aggregate_meta: Some(AggregateSnapshotMeta {
            label: "fleet".into(),
            host_count: hostnames.len(),
            hostnames: hostnames.clone(),
            merged_at: "2026-11-19T12:00:00Z".into(),
            baseline_provisional: false,
            section_host_counts: BTreeMap::new(),
        }),
        ..Default::default()
    };

    assert_eq!(parsed_host_list(&render_audit(&snap)), hostnames);
}
