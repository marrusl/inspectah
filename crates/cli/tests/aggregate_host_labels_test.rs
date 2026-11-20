//! Host labels in an aggregate, from collection to the rendered audit.
//!
//! Hosts run through the real scan path ([`SysctlHost`], `collect()`,
//! `validate`, `redact`), are written and loaded again, then merged. An
//! unnamed host's label is in angle brackets, which Markdown reads as
//! HTML when written bare, so the audit must render every host label as
//! literal text.

#[path = "support/sysctl_hosts.rs"]
mod sysctl_hosts;

use std::collections::BTreeMap;

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Arena, Options, parse_document};
use inspectah_core::aggregate::merge_snapshots;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::aggregate::AggregateSnapshotMeta;
use inspectah_core::types::config::ConfigFileEntry;
use inspectah_pipeline::render::audit::render_audit;
use sysctl_hosts::{SysctlHost, reload, scan};

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

/// A file every host carries with the same content.
const SHARED_FILE: &str = "/etc/fleet.conf";

/// The scan of `host`, given a file only it carries at `own_file`,
/// written and loaded again.
fn scanned(host: SysctlHost, own_file: &str) -> InspectionSnapshot {
    reload(&scan(
        host.file(SHARED_FILE, "shared = 1\n")
            .file(own_file, &format!("owner = {own_file}\n")),
    ))
}

/// The hosts the merge attributes the config file at `path` to, and its
/// prevalence.
fn config_hosts(merged: &InspectionSnapshot, path: &str) -> (Vec<String>, i32, i32) {
    let entries: Vec<&ConfigFileEntry> = merged
        .config
        .as_ref()
        .expect("a config section")
        .files
        .iter()
        .filter(|e| e.path == path)
        .collect();
    let [entry] = entries.as_slice() else {
        panic!("one entry at {path}, got {}", entries.len());
    };
    let agg = entry.aggregate.as_ref().expect("aggregate prevalence");
    (agg.hosts.clone(), agg.count, agg.total)
}

/// Hosts with an empty static name, one named by its transient name and
/// two that nothing named, beside a host with a static name, collected,
/// reloaded and merged in both input orders. The two unnamed hosts are
/// given one scan timestamp, so only their content tells them apart.
/// Every input is its own host, shared items count every host, and each
/// host-specific item is attributed to its own host whichever order the
/// inputs come in.
#[test]
fn collected_hosts_aggregate_with_distinct_labels() {
    const TIED: &str = "2026-11-19T10:00:00.000000000+00:00";
    let transient = scanned(
        SysctlHost::new()
            .file("/etc/hostname", "")
            .file("/proc/sys/kernel/hostname", "rhel9-a\n")
            .hostnamectl("rhel9-a", "transient"),
        "/etc/own-transient.conf",
    );
    let named = scanned(
        SysctlHost::new().file("/etc/hostname", "web-01\n"),
        "/etc/own-static.conf",
    );
    let unnamed = |own_file: &str| {
        let mut snap = scanned(
            SysctlHost::new()
                .file("/etc/hostname", "")
                .file("/proc/sys/kernel/hostname", "localhost\n")
                .hostnamectl("localhost", "default"),
            own_file,
        );
        snap.meta
            .insert("timestamp".into(), serde_json::json!(TIED));
        snap
    };
    let unnamed_x = unnamed("/etc/own-x.conf");
    let unnamed_y = unnamed("/etc/own-y.conf");

    let source = |snap: &InspectionSnapshot| {
        (
            snap.meta
                .get("hostname")
                .and_then(|v| v.as_str())
                .map(String::from),
            snap.meta["hostname_source"].as_str().map(String::from),
        )
    };
    assert_eq!(
        source(&transient),
        (
            Some("rhel9-a".into()),
            Some("/proc/sys/kernel/hostname".into())
        )
    );
    assert_eq!(
        source(&named),
        (Some("web-01".into()), Some("/etc/hostname".into()))
    );
    assert_eq!(source(&unnamed_x), (None, Some("none".into())));
    assert_eq!(source(&unnamed_y), (None, Some("none".into())));

    let inputs = vec![transient, named, unnamed_x, unnamed_y];
    let mut reversed = inputs.clone();
    reversed.reverse();

    let mut attributions = Vec::new();
    for inputs in [inputs, reversed] {
        let (merged, _) = merge_snapshots(inputs, None, None).expect("the hosts merge");
        let merged = reload(&merged);
        let labels = merged
            .aggregate_meta
            .as_ref()
            .expect("aggregate meta")
            .hostnames
            .clone();
        assert_eq!(
            labels,
            [
                "rhel9-a".to_string(),
                "web-01".into(),
                format!("<unnamed 1, scanned {TIED}>"),
                format!("<unnamed 2, scanned {TIED}>"),
            ]
        );

        // Prevalence lists its hosts sorted by label.
        let mut sorted_labels = labels.clone();
        sorted_labels.sort();
        assert_eq!(config_hosts(&merged, SHARED_FILE), (sorted_labels, 4, 4));
        let bash = merged
            .rpm
            .as_ref()
            .expect("an rpm section")
            .packages_added
            .iter()
            .find(|p| p.name == "bash")
            .and_then(|p| p.aggregate.as_ref())
            .expect("bash with prevalence");
        assert_eq!((bash.count, bash.total), (4, 4), "bash is on every host");

        let own = |path: &str| {
            let (hosts, count, total) = config_hosts(&merged, path);
            assert_eq!((count, total), (1, 4), "{path} is on one host");
            hosts
        };
        assert_eq!(own("/etc/own-transient.conf"), ["rhel9-a"]);
        assert_eq!(own("/etc/own-static.conf"), ["web-01"]);
        let (x, y) = (own("/etc/own-x.conf"), own("/etc/own-y.conf"));
        assert_ne!(x, y, "each unnamed host keeps its own file");

        assert_eq!(parsed_host_list(&render_audit(&merged)), labels);
        attributions.push((x, y));
    }
    assert_eq!(
        attributions[0], attributions[1],
        "attribution follows content, not input order"
    );
}
