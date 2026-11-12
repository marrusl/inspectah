//! Literal text in Markdown: a host value rendered as an inline code span
//! that no Markdown reader can reinterpret.
//!
//! Inside a code span, HTML, links, images, emphasis and backslash escapes
//! are all literal (CommonMark § Code spans), so a value only has to be
//! kept from ending the span, the line, or a table cell:
//!
//! - control characters other than tab, and U+2028 and U+2029, become
//!   `\u{..}` escapes, so no value can end a line, a table row or a list
//!   item;
//! - the fence is one backtick longer than the longest backtick run in the
//!   value, so an embedded run cannot close it;
//! - in a table cell, `|` becomes `\|`. GFM splits a row into cells before
//!   inline parsing, honors that escape inside a code span, then removes
//!   the backslash (cmark-gfm `table.c`, `unescape_pipes`). Backslashes are
//!   never doubled: inside a code span one is already literal.
//!
//! An empty value renders as the fixed text `(empty)`.

/// What an empty value renders as.
const EMPTY: &str = "(empty)";

/// Inline code span per spec § 5.5 "Literal text"; `(empty)` for "".
pub fn md_code(value: &str) -> String {
    if value.is_empty() {
        return EMPTY.to_string();
    }
    let escaped = escape_line_breaking(value);
    let longest_run = escaped.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest_run + 1);
    // CommonMark strips one space from each end of a span whose content
    // begins and ends with a space and is not all spaces; padding both
    // ends keeps such a value, and one that begins or ends with a
    // backtick, exactly as it is.
    let all_spaces = escaped.chars().all(|c| c == ' ');
    let pad =
        !all_spaces && (longest_run > 0 || (escaped.starts_with(' ') && escaped.ends_with(' ')));
    if pad {
        format!("{fence} {escaped} {fence}")
    } else {
        format!("{fence}{escaped}{fence}")
    }
}

/// As [`md_code`], with `|` written `\|` for a GFM table cell.
pub fn md_code_cell(value: &str) -> String {
    md_code(value).replace('|', "\\|")
}

/// Escape every character that could end a line: control characters
/// other than tab, and the Unicode line and paragraph separators.
fn escape_line_breaking(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if (c.is_control() && c != '\t') || c == '\u{2028}' || c == '\u{2029}' {
                format!("\\u{{{:x}}}", u32::from(c))
            } else {
                c.to_string()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md_code_renders_values_literally() {
        let cases = [
            ("plain", "vm.swappiness", "`vm.swappiness`"),
            ("empty", "", "(empty)"),
            ("newline", "a\nb", "`a\\u{a}b`"),
            ("line separator", "a\u{2028}b", "`a\\u{2028}b`"),
            ("paragraph separator", "a\u{2029}b", "`a\\u{2029}b`"),
            ("NUL", "a\0b", "`a\\u{0}b`"),
            ("carriage return", "a\rb", "`a\\u{d}b`"),
            ("DEL", "a\u{7f}b", "`a\\u{7f}b`"),
            ("tab kept", "32768\t60999", "`32768\t60999`"),
            ("three backticks", "a```b", "```` a```b ````"),
            ("leading backtick", "`a", "`` `a ``"),
            ("pipe unchanged", "|/usr/lib/x %P", "`|/usr/lib/x %P`"),
            ("backslash not doubled", "a\\b", "`a\\b`"),
            ("html stays literal", "<img src=x>", "`<img src=x>`"),
            ("space at both ends", " x ", "`  x  `"),
            ("all spaces", "  ", "`  `"),
        ];
        for (name, value, expected) in cases {
            assert_eq!(md_code(value), expected, "{name}");
        }
    }

    #[test]
    fn md_code_cell_escapes_only_the_pipe() {
        let cases = [
            ("pipe", "|/usr/lib/x %P", "`\\|/usr/lib/x %P`"),
            ("two pipes", "a|b|c", "`a\\|b\\|c`"),
            ("backslash before pipe", "a\\|b", "`a\\\\|b`"),
            ("backslash not doubled", "a\\b", "`a\\b`"),
            ("empty", "", "(empty)"),
            ("newline", "a\nb", "`a\\u{a}b`"),
        ];
        for (name, value, expected) in cases {
            assert_eq!(md_code_cell(value), expected, "{name}");
        }
    }
}
