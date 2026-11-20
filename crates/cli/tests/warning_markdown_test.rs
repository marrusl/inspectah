//! Warning messages carry host text (repo file names, key paths, unit
//! names), so the Warnings list in `README.md` and `audit-report.md` must
//! print each one literally. Checked by parsing both reports with a
//! GFM-conformant parser, not by string matching.

use comrak::nodes::{Node, NodeValue};
use comrak::{Arena, parse_document};
use inspectah_core::snapshot::InspectionSnapshot;
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

fn snapshot() -> InspectionSnapshot {
    InspectionSnapshot {
        warnings: [HOSTILE, PLAIN]
            .into_iter()
            .map(|message| Warning {
                inspector: INSPECTOR.into(),
                message: message.into(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

/// The list under the `## Warnings` heading.
fn warnings_list<'a>(root: Node<'a>) -> Node<'a> {
    let heading_text = |node: Node<'_>| -> String {
        node.descendants()
            .filter_map(|n| match &n.data.borrow().value {
                NodeValue::Text(text) => Some(text.to_string()),
                _ => None,
            })
            .collect()
    };
    let heading = root
        .children()
        .find(|n| {
            matches!(&n.data.borrow().value, NodeValue::Heading(h) if h.level == 2)
                && heading_text(*n) == "Warnings"
        })
        .expect("a `Warnings` heading");
    let list = heading.next_sibling().expect("something after the heading");
    assert!(
        matches!(list.data.borrow().value, NodeValue::List(_)),
        "the heading is followed by a list"
    );
    list
}

/// Each list item's message, read back as the code span the renderer
/// wrote it in. Fails on any HTML, link, image or emphasis node.
fn messages(md: &str) -> Vec<String> {
    let arena = Arena::new();
    let mut options = comrak::Options::default();
    options.extension.table = true;
    let root = parse_document(&arena, md, &options);
    let list = warnings_list(root);
    list.children()
        .map(|item| {
            let mut code = Vec::new();
            for node in item.descendants() {
                match &node.data.borrow().value {
                    NodeValue::HtmlBlock(_)
                    | NodeValue::HtmlInline(_)
                    | NodeValue::Link(_)
                    | NodeValue::Image(_)
                    | NodeValue::Emph => panic!("markup in a warning item:\n{md}"),
                    NodeValue::Code(c) => code.push(c.literal.clone()),
                    _ => {}
                }
            }
            assert_eq!(code.len(), 1, "one code span per warning:\n{md}");
            code.remove(0)
        })
        .collect()
}

#[test]
fn warnings_render_literally_in_markdown_reports() {
    let snap = snapshot();
    let expected = [literal_text(HOSTILE), literal_text(PLAIN)];
    for (name, md) in [
        ("README.md", render_readme(&snap)),
        ("audit-report.md", render_audit(&snap)),
    ] {
        assert_eq!(messages(&md), expected, "{name}:\n{md}");
    }
}
