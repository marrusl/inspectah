//! Warning messages carry host text (repo file names, key paths, unit
//! names), so the Warnings list in `README.md` and `audit-report.md` must
//! print each one literally, and so must the audit's host-path lists.
//! Checked by parsing the reports with a GFM-conformant parser, not by
//! string matching.

use comrak::nodes::{Node, NodeValue};
use comrak::{Arena, parse_document};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::warnings::Warning;
use inspectah_pipeline::render::audit::render_audit;
use inspectah_pipeline::render::md_literal::literal_text;
use inspectah_pipeline::render::readme::render_readme;

/// The inspector every fixture warning names.
const INSPECTOR: &str = "rpm";

/// A warning built the way the rpm collector builds its unread-key one,
/// with a repo file name and key path full of Markdown metacharacters, an
/// HTML tag, an ESC and a newline.
const HOSTILE: &str = "gpgkey in /etc/yum.repos.d/a`b|c*d*_e_.repo: \
     [link](http://example.invalid) <img/src=x/onerror=alert(1)> &amp; \
     /etc/pki/rpm-gpg/k\u{1b}[31m\nsecond line";

/// A warning with nothing to escape, so the list has a neighbour the
/// hostile one could break into.
const PLAIN: &str = "unit foo.service has state 'bad'";

/// An inspector name as an imported snapshot can carry it: the scan sets
/// the field from code, but a snapshot file is arbitrary text.
const HOSTILE_INSPECTOR: &str = "rpm** [x](http://example.invalid) `a|b`\n# heading";

/// A host path with a backtick, a pipe, Markdown and a line break.
const HOSTILE_PATH: &str = "/etc/a`b|c*d*_e_[x](http://example.invalid)\nsecond line";

fn snapshot() -> InspectionSnapshot {
    snapshot_with(INSPECTOR)
}

fn snapshot_with(inspector: &str) -> InspectionSnapshot {
    InspectionSnapshot {
        warnings: [HOSTILE, PLAIN]
            .into_iter()
            .map(|message| Warning {
                inspector: inspector.into(),
                message: message.into(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

/// The text of `node` and everything under it.
fn text_of(node: Node<'_>) -> String {
    node.descendants()
        .filter_map(|n| match &n.data.borrow().value {
            NodeValue::Text(text) => Some(text.to_string()),
            _ => None,
        })
        .collect()
}

/// The list right after the heading of `level` whose text is `title`.
fn list_after<'a>(root: Node<'a>, level: u8, title: &str) -> Node<'a> {
    let heading = root
        .children()
        .find(|n| {
            matches!(&n.data.borrow().value, NodeValue::Heading(h) if h.level == level)
                && text_of(n) == title
        })
        .unwrap_or_else(|| panic!("a `{title}` heading"));
    let list = heading.next_sibling().expect("something after the heading");
    assert!(
        matches!(list.data.borrow().value, NodeValue::List(_)),
        "the `{title}` heading is followed by a list"
    );
    list
}

/// Each item of the list under the heading, as its code spans in order.
/// Fails on any HTML, link, image or emphasis node, or a heading or a
/// second paragraph a line break would make.
fn code_spans(md: &str, level: u8, title: &str) -> Vec<Vec<String>> {
    let arena = Arena::new();
    let mut options = comrak::Options::default();
    options.extension.table = true;
    let root = parse_document(&arena, md, &options);
    let list = list_after(root, level, title);
    list.children()
        .map(|item| {
            assert_eq!(item.children().count(), 1, "one paragraph per item:\n{md}");
            let mut code = Vec::new();
            for node in item.descendants() {
                match &node.data.borrow().value {
                    NodeValue::HtmlBlock(_)
                    | NodeValue::HtmlInline(_)
                    | NodeValue::Link(_)
                    | NodeValue::Image(_)
                    | NodeValue::Heading(_)
                    | NodeValue::Emph => panic!("markup in a `{title}` item:\n{md}"),
                    NodeValue::Code(c) => code.push(c.literal.clone()),
                    _ => {}
                }
            }
            code
        })
        .collect()
}

#[test]
fn warnings_render_literally_in_markdown_reports() {
    let snap = snapshot();
    let audit = [vec![literal_text(HOSTILE)], vec![literal_text(PLAIN)]];
    assert_eq!(
        code_spans(&render_audit(&snap), 2, "Warnings"),
        audit,
        "audit-report.md"
    );
    let readme = [
        vec![literal_text(INSPECTOR), literal_text(HOSTILE)],
        vec![literal_text(INSPECTOR), literal_text(PLAIN)],
    ];
    assert_eq!(
        code_spans(&render_readme(&snap), 2, "Warnings"),
        readme,
        "README.md"
    );
}

/// README.md names each warning's inspector in bold. The field is host
/// text in an imported snapshot, so it is a code span too, inside the bold.
#[test]
fn a_hostile_inspector_name_renders_literally_in_the_readme() {
    let md = render_readme(&snapshot_with(HOSTILE_INSPECTOR));
    let expected = [
        vec![literal_text(HOSTILE_INSPECTOR), literal_text(HOSTILE)],
        vec![literal_text(HOSTILE_INSPECTOR), literal_text(PLAIN)],
    ];
    assert_eq!(code_spans(&md, 2, "Warnings"), expected, "{md}");
}

/// Each audit list that prints a host path does so as one code span, so a
/// backtick, a pipe or a line break in the path cannot end it. One row per
/// site: the snapshot that fills it, its heading level and title, and the
/// code spans each item holds.
#[test]
fn audit_host_paths_render_literally() {
    let unowned = InspectionSnapshot {
        config: Some(ConfigSection {
            files: vec![ConfigFileEntry {
                path: HOSTILE_PATH.into(),
                kind: ConfigFileKind::Unowned,
                disposition: FindingKind::included(),
                ..Default::default()
            }],
        }),
        ..Default::default()
    };
    let sites = [(
        unowned,
        4,
        "Unowned Config Files (1)",
        vec![vec![literal_text(HOSTILE_PATH)]],
    )];
    for (snap, level, title, expected) in sites {
        let md = render_audit(&snap);
        assert_eq!(code_spans(&md, level, title), expected, "{title}:\n{md}");
    }
}
