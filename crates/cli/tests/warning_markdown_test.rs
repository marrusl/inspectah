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
use inspectah_core::types::kernelboot::KernelBootSection;
use inspectah_core::types::network::{
    FirewallDirectRule, NetworkSection, ProxyEntry, StaticRouteFile,
};
use inspectah_core::types::nonrpm::{UnmanagedFile, UnmanagedFileSection};
use inspectah_core::types::rpm::{InstalledGroup, RpmSection};
use inspectah_core::types::selinux::{
    FcontextFileType, FcontextRule, SelinuxContext, SelinuxSection,
};
use inspectah_core::types::storage::{CredentialRef, StorageSection, VarDirectory};
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

/// The blocks under the heading of `level` whose text is `title`, up to
/// the next heading at that level or above, as entries of code spans: one
/// per list item, paragraph or table body row. Fails on any HTML, link,
/// image or emphasis node in the section, and on a list item a line break
/// split into two paragraphs.
fn section_code(md: &str, level: u8, title: &str) -> Vec<Vec<String>> {
    let arena = Arena::new();
    let mut options = comrak::Options::default();
    options.extension.table = true;
    let root = parse_document(&arena, md, &options);
    let heading = root
        .children()
        .find(|n| {
            matches!(&n.data.borrow().value, NodeValue::Heading(h) if h.level == level)
                && text_of(n) == title
        })
        .unwrap_or_else(|| panic!("a `{title}` heading:\n{md}"));
    let spans = |node: Node<'_>| -> Vec<String> {
        node.descendants()
            .filter_map(|n| match &n.data.borrow().value {
                NodeValue::HtmlBlock(_)
                | NodeValue::HtmlInline(_)
                | NodeValue::Link(_)
                | NodeValue::Image(_)
                | NodeValue::Emph => panic!("markup in `{title}`:\n{md}"),
                NodeValue::Code(c) => Some(c.literal.clone()),
                _ => None,
            })
            .collect()
    };
    let mut entries = Vec::new();
    let mut block = heading.next_sibling();
    while let Some(node) = block {
        match &node.data.borrow().value {
            NodeValue::Heading(h) if h.level <= level => break,
            NodeValue::List(_) => {
                for item in node.children() {
                    assert_eq!(item.children().count(), 1, "one paragraph per item:\n{md}");
                    entries.push(spans(item));
                }
            }
            NodeValue::Table(_) => entries.extend(node.children().skip(1).map(spans)),
            _ => entries.push(spans(node)),
        }
        block = node.next_sibling();
    }
    entries
}

/// Each audit site that prints host text does so as code spans, so a
/// backtick, a pipe, a link or a line break in the value cannot end one,
/// split a table cell or become markup. One row per site: the snapshot
/// that fills it with [`HOSTILE_PATH`], its heading level and title, and
/// the code spans of each entry that holds the value (an entry the value
/// broke out of holds no such span, and fails the comparison).
#[test]
fn audit_host_paths_render_literally() {
    let host = || HOSTILE_PATH.to_string();
    let config = |kind| InspectionSnapshot {
        config: Some(ConfigSection {
            withheld: Vec::new(),
            files: vec![ConfigFileEntry {
                path: host(),
                kind,
                disposition: FindingKind::included(),
                ..Default::default()
            }],
        }),
        ..Default::default()
    };
    let network = |n: NetworkSection| InspectionSnapshot {
        network: Some(n),
        ..Default::default()
    };
    let storage = |s: StorageSection| InspectionSnapshot {
        storage: Some(s),
        ..Default::default()
    };
    let group = |name: &str| InstalledGroup {
        name: name.into(),
        members: vec![host()],
        ..Default::default()
    };
    let lit = literal_text(HOSTILE_PATH);
    let sites = [
        (
            config(ConfigFileKind::Unowned),
            4,
            "Unowned Config Files (1)",
            vec![vec![lit.clone()]],
        ),
        (
            config(ConfigFileKind::RpmOwnedModified),
            4,
            "Modified RPM-Owned Files (1)",
            vec![vec![lit.clone()]],
        ),
        (
            InspectionSnapshot {
                rpm: Some(RpmSection {
                    installed_groups: Some(vec![group("a"), group("b")]),
                    ..Default::default()
                }),
                ..Default::default()
            },
            2,
            "Packages",
            vec![vec![lit.clone()]],
        ),
        (
            InspectionSnapshot {
                kernel_boot: Some(KernelBootSection {
                    cmdline: host(),
                    ..Default::default()
                }),
                ..Default::default()
            },
            4,
            "Kernel Command Line",
            vec![vec![lit.clone()]],
        ),
        (
            InspectionSnapshot {
                // Excluded, so the disposition after the label is fixed
                // text and the label is the entry's only host text.
                selinux: Some(SelinuxSection {
                    fcontext_rules: vec![FcontextRule::Label {
                        spec: host(),
                        file_type: FcontextFileType::All,
                        context: SelinuxContext {
                            user: "system_u".into(),
                            role: "object_r".into(),
                            type_: "etc_t".into(),
                            level: None,
                        },
                        disposition: FindingKind::excluded(),
                        aggregate: None,
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            },
            4,
            "File Context Rules",
            vec![vec![literal_text(&format!("{HOSTILE_PATH} -> etc_t"))]],
        ),
        (
            network(NetworkSection {
                firewall_direct_rules: vec![FirewallDirectRule {
                    args: host(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            4,
            "Firewall Direct Rules (1)",
            vec![vec![lit.clone()]],
        ),
        (
            network(NetworkSection {
                static_routes: vec![StaticRouteFile {
                    path: host(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            4,
            "Static Routes (1)",
            vec![vec![lit.clone()]],
        ),
        (
            network(NetworkSection {
                hosts_additions: vec![host()],
                ..Default::default()
            }),
            4,
            "/etc/hosts Additions (1)",
            vec![vec![lit.clone()]],
        ),
        (
            network(NetworkSection {
                proxy: vec![ProxyEntry {
                    source: host(),
                    line: host(),
                }],
                ..Default::default()
            }),
            4,
            "Proxy Settings (1)",
            vec![vec![lit.clone(), lit.clone()]],
        ),
        (
            storage(StorageSection {
                credential_refs: vec![CredentialRef {
                    mount_point: host(),
                    credential_path: host(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            4,
            "Credential References (1)",
            vec![vec![lit.clone(), lit.clone()]],
        ),
        (
            storage(StorageSection {
                var_directories: vec![VarDirectory {
                    path: host(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            4,
            "/var Directories (1)",
            vec![vec![lit.clone()]],
        ),
        (
            InspectionSnapshot {
                unmanaged_files: Some(UnmanagedFileSection {
                    items: vec![UnmanagedFile {
                        path: format!("/srv/{HOSTILE_PATH}"),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            },
            3,
            "Unmanaged Files under /srv",
            vec![vec![literal_text(&format!("/srv/{HOSTILE_PATH}"))]],
        ),
    ];
    for (snap, level, title, expected) in sites {
        let md = render_audit(&snap);
        let held: Vec<Vec<String>> = section_code(&md, level, title)
            .into_iter()
            .filter(|entry| entry.iter().any(|span| span.contains("a`b|c")))
            .collect();
        assert_eq!(held, expected, "{title}:\n{md}");
    }
}
