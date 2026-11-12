//! The one `sysctl.d(5)` line parser, and the value comparison the
//! resolution uses.

use super::{SysctlKey, SysctlLine, SysctlName};

/// Characters that make a name a glob (`sysctl.d(5)`).
const GLOB_CHARS: [char; 3] = ['*', '?', '['];

/// Parse a sysctl file's content into its lines (`sysctl.d(5)`;
/// systemd's `sysctl.c`, `parse_line`).
///
/// Lines are trimmed first, so a CRLF file or trailing whitespace parses
/// like the LF file. Blank lines and lines starting with `#` or `;` are
/// skipped. A line holding `=` is an assignment: it splits on the first
/// `=`, both sides are trimmed, and a leading `-` on the name sets
/// `ignore_failure`. A line without `=` that starts with `-` excludes the
/// name after the `-`. A name holding `*`, `?` or `[` is a glob, any other
/// a key, normalized. Every other line, including one whose name is empty
/// or is a non-glob name holding whitespace (no `/proc/sys` entry can
/// match it), is unparsed.
/// Line numbers are 1-based physical lines.
pub fn parse_sysctl_lines(content: &str) -> Vec<SysctlLine> {
    content
        .lines()
        .zip(1u32..)
        .filter_map(|(raw, line_no)| parse_line(raw.trim(), line_no))
        .collect()
}

fn parse_line(line: &str, line_no: u32) -> Option<SysctlLine> {
    if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
        return None;
    }
    let unparsed = || SysctlLine::Unparsed {
        line_no,
        text: line.to_string(),
    };
    let parsed = match line.split_once('=') {
        Some((name, value)) => {
            let name = name.trim();
            let (name, ignore_failure) = match name.strip_prefix('-') {
                Some(rest) => (rest.trim_start(), true),
                None => (name, false),
            };
            parse_name(name).map(|name| SysctlLine::Assign {
                line_no,
                name,
                value: value.trim().to_string(),
                ignore_failure,
            })
        }
        None => line
            .strip_prefix('-')
            .and_then(|name| parse_name(name.trim_start()))
            .map(|name| SysctlLine::Exclude { line_no, name }),
    };
    Some(parsed.unwrap_or_else(unparsed))
}

/// Classify a trimmed name, or `None` when it cannot name a sysctl.
///
/// Globs are classified before the whitespace check: a bracket expression
/// such as `[! ]` may hold a space and is still a valid glob.
fn parse_name(name: &str) -> Option<SysctlName> {
    if name.is_empty() {
        return None;
    }
    if name.contains(GLOB_CHARS) {
        return Some(SysctlName::Glob(name.to_string()));
    }
    if name.contains(char::is_whitespace) {
        return None;
    }
    Some(SysctlName::Key(SysctlKey::normalize(name)))
}

/// Compare a sysctl.d file value with a runtime value as token sequences.
///
/// The kernel reports multi-value sysctls (`net.ipv4.ip_local_port_range`,
/// `net.ipv4.tcp_rmem`, `kernel.printk`) tab-separated, while config files
/// write them space-separated. Comparing raw strings would report every such
/// key as drift even when the kernel runs exactly the configured value, so
/// whitespace of any kind and amount counts as a single separator here.
///
/// This applies to every key, including string-valued ones such as
/// `kernel.core_pattern` where internal spacing is part of the value, so a
/// whitespace-only difference there also compares equal. Key-aware comparison
/// was left out deliberately to keep a single rule for all keys.
pub fn sysctl_values_equal(file_val: &str, runtime_val: &str) -> bool {
    file_val
        .split_whitespace()
        .eq(runtime_val.split_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::sysctl::{SysctlKey, SysctlLine, SysctlName};

    fn key(name: &str) -> SysctlName {
        SysctlName::Key(SysctlKey::normalize(name))
    }

    #[test]
    fn normalize_spellings() {
        let cases = [
            ("net/ipv4/ip_forward", "net.ipv4.ip_forward"),
            ("net.ipv4.ip_forward", "net.ipv4.ip_forward"),
            (
                "net/ipv4/conf/enp3s0.200/forwarding",
                "net.ipv4.conf.enp3s0/200.forwarding",
            ),
            (
                "net.ipv4.conf.enp3s0/200.forwarding",
                "net.ipv4.conf.enp3s0/200.forwarding",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(
                SysctlKey::normalize(input).as_str(),
                expected,
                "normalizing `{input}`"
            );
        }
    }

    #[test]
    fn parse_lines() {
        let cases: Vec<(&str, Vec<SysctlLine>)> = vec![
            ("", vec![]),
            ("\n   \n", vec![]),
            ("# comment = 1\n; also = 2\n", vec![]),
            (
                "vm.swappiness = 10",
                vec![SysctlLine::Assign {
                    line_no: 1,
                    name: key("vm.swappiness"),
                    value: "10".into(),
                    ignore_failure: false,
                }],
            ),
            (
                "-net.ipv6.conf.all.disable_ipv6 = 1",
                vec![SysctlLine::Assign {
                    line_no: 1,
                    name: key("net.ipv6.conf.all.disable_ipv6"),
                    value: "1".into(),
                    ignore_failure: true,
                }],
            ),
            (
                "-vm.swappiness",
                vec![SysctlLine::Exclude {
                    line_no: 1,
                    name: key("vm.swappiness"),
                }],
            ),
            (
                "net.ipv4.conf.*.rp_filter = 2",
                vec![SysctlLine::Assign {
                    line_no: 1,
                    name: SysctlName::Glob("net.ipv4.conf.*.rp_filter".into()),
                    value: "2".into(),
                    ignore_failure: false,
                }],
            ),
            (
                "-net.*.x",
                vec![SysctlLine::Exclude {
                    line_no: 1,
                    name: SysctlName::Glob("net.*.x".into()),
                }],
            ),
            (
                "net.ipv4.conf.[! ]*.rp_filter = 2",
                vec![SysctlLine::Assign {
                    line_no: 1,
                    name: SysctlName::Glob("net.ipv4.conf.[! ]*.rp_filter".into()),
                    value: "2".into(),
                    ignore_failure: false,
                }],
            ),
            (
                "-net.ipv4.conf.[! ]*.rp_filter",
                vec![SysctlLine::Exclude {
                    line_no: 1,
                    name: SysctlName::Glob("net.ipv4.conf.[! ]*.rp_filter".into()),
                }],
            ),
            (
                "<img src=x>",
                vec![SysctlLine::Unparsed {
                    line_no: 1,
                    text: "<img src=x>".into(),
                }],
            ),
            (
                "[link](http://example.com)",
                vec![SysctlLine::Unparsed {
                    line_no: 1,
                    text: "[link](http://example.com)".into(),
                }],
            ),
            (
                "= 1",
                vec![SysctlLine::Unparsed {
                    line_no: 1,
                    text: "= 1".into(),
                }],
            ),
            (
                "-",
                vec![SysctlLine::Unparsed {
                    line_no: 1,
                    text: "-".into(),
                }],
            ),
            (
                "net/ipv4/ip_forward = 1",
                vec![SysctlLine::Assign {
                    line_no: 1,
                    name: key("net.ipv4.ip_forward"),
                    value: "1".into(),
                    ignore_failure: false,
                }],
            ),
            (
                "kernel.core_pattern = |/x a=b",
                vec![SysctlLine::Assign {
                    line_no: 1,
                    name: key("kernel.core_pattern"),
                    value: "|/x a=b".into(),
                    ignore_failure: false,
                }],
            ),
            (
                "# header\n\nvm.swappiness = 10\n; note\n-kernel.sysrq\n",
                vec![
                    SysctlLine::Assign {
                        line_no: 3,
                        name: key("vm.swappiness"),
                        value: "10".into(),
                        ignore_failure: false,
                    },
                    SysctlLine::Exclude {
                        line_no: 5,
                        name: key("kernel.sysrq"),
                    },
                ],
            ),
        ];
        for (content, expected) in cases {
            assert_eq!(parse_sysctl_lines(content), expected, "parsing {content:?}");
        }
    }

    #[test]
    fn crlf_lines_parse_like_lf() {
        let lf = parse_sysctl_lines("vm.swappiness = 10\n");
        assert_eq!(lf.len(), 1);
        for variant in ["vm.swappiness = 10\r\n", "vm.swappiness = 10   \n"] {
            assert_eq!(parse_sysctl_lines(variant), lf, "parsing {variant:?}");
        }
    }

    #[test]
    fn empty_value_is_an_assignment() {
        assert_eq!(
            parse_sysctl_lines("kernel.foo ="),
            vec![SysctlLine::Assign {
                line_no: 1,
                name: key("kernel.foo"),
                value: String::new(),
                ignore_failure: false,
            }]
        );
        assert!(sysctl_values_equal("", ""));
    }

    #[test]
    fn values_equal_ignores_whitespace_runs() {
        assert!(sysctl_values_equal("32768 60999", "32768\t60999"));
        assert!(!sysctl_values_equal("32768 60999", "32768 61000"));
    }
}
