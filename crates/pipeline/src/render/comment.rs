//! Comment lines in rendered files.
//!
//! **Every renderer that writes a file where `#` starts a comment joins its
//! lines with [`join_lines`].** That covers the Containerfile, the `/var`
//! tmpfiles.d fragment, the kickstart suggestion, and the users kickstart
//! and blueprint fragments. The join escapes [`escape_comment_text`]'s
//! characters in every comment line, so a snapshot value interpolated into
//! a comment cannot end it early, wherever the line was built and whether
//! or not its site remembered to escape. A site that escapes its own value
//! first loses nothing: the escaped text holds none of these characters,
//! so the join leaves it as it is.
//!
//! The guard sits at the join rather than at each `format!("# ...")`
//! because the renderers emit a few hundred comment lines from plain
//! `String`s, and a new one can only be guarded by the place every line
//! passes through. It covers comment lines only. A value reaching an
//! instruction is refused or proved before it gets there (see
//! `process-docs/skills/containerfile-value-quoting.md`): escaping it at
//! the join would silently change the command.
//!
//! A file assembled some other way does not get this guard, so build any
//! new one as a `Vec<String>` of lines and join it here.

/// Characters that some reader of a rendered file treats as ending a line:
/// the Unicode mandatory line breaks (UAX #14 classes BK, CR, LF and NL).
pub const LINE_TERMINATORS: &[char] = &[
    '\n', '\r', '\u{0B}', '\u{0C}', '\u{85}', '\u{2028}', '\u{2029}',
];

/// Escape every character in comment text that a reader could act on,
/// leaving all else as is.
///
/// That is every line terminator in [`LINE_TERMINATORS`], and every other
/// control character except tab: the C0 controls, DEL and the C1
/// controls. The newline ends a Containerfile comment, so the rest of the
/// value would become an instruction. The other terminators are escaped
/// whatever the build parser does with them, because a terminal or editor
/// may break the line there and show a reviewer a line the file does not
/// have: a carriage return returns to column 0 and overprints the comment,
/// and editors display U+2028 and U+2029 as line breaks. The remaining
/// controls cannot break a line, but ESC and the C1 CSI start terminal
/// escape sequences, which can move the cursor, erase or recolor what was
/// printed, so a `cat` of the file can show text it does not hold.
///
/// A newline prints as the two characters `\n`, a carriage return as
/// `\r`, and the rest in `\u{1b}` form.
///
/// Use this on a whole comment line whose fixed wording contains quotes,
/// such as `OmissionReason::describe` output, where `sanitize_for_comment`'s
/// quote doubling would alter the wording.
pub(crate) fn escape_comment_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push(c),
            c if c.is_control() || LINE_TERMINATORS.contains(&c) => out.extend(c.escape_unicode()),
            c => out.push(c),
        }
    }
    out
}

/// Whether a rendered line is a comment: `#` after any leading spaces or
/// tabs, which is what the Containerfile, tmpfiles.d, kickstart and TOML
/// parsers read as one.
///
/// Only spaces and tabs are skipped, not `char::is_whitespace`, which
/// includes the line terminators: a line opened by a terminator is not a
/// comment, and treating it as one would escape an instruction.
fn is_comment_line(line: &str) -> bool {
    line.trim_start_matches([' ', '\t']).starts_with('#')
}

/// Join rendered lines into file text, one line per element, with every
/// comment line passed through [`escape_comment_text`].
///
/// Each element is one line of output. A comment element that held a
/// terminator would otherwise come out as two lines, the second one an
/// instruction; escaped, it stays one.
pub(crate) fn join_lines<S: AsRef<str>>(lines: &[S]) -> String {
    lines
        .iter()
        .map(|line| {
            let line = line.as_ref();
            if is_comment_line(line) {
                escape_comment_text(line)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every line terminator and every control but tab is escaped to
    /// visible text, so comment text stays on one line in the file and
    /// prints as itself in a terminal.
    #[test]
    fn comment_text_keeps_no_line_terminator_or_control() {
        let cases = [
            ("plain", "plain"),
            ("a\nb", "a\\nb"),
            ("a\rb", "a\\rb"),
            ("a\r\nb", "a\\r\\nb"),
            ("a\u{0B}b", "a\\u{b}b"),
            ("a\u{0C}b", "a\\u{c}b"),
            ("a\u{85}b", "a\\u{85}b"),
            ("a\u{2028}b", "a\\u{2028}b"),
            ("a\u{2029}b", "a\\u{2029}b"),
            ("a\u{1b}[2Jb", "a\\u{1b}[2Jb"),
            ("a\u{0}b", "a\\u{0}b"),
            ("a\u{7f}b", "a\\u{7f}b"),
            ("a\u{9b}b", "a\\u{9b}b"),
            ("it's", "it's"),
            ("tab\there", "tab\there"),
            ("caf\u{e9} \u{2014}", "caf\u{e9} \u{2014}"),
        ];
        for (input, escaped) in cases {
            let out = escape_comment_text(input);
            assert_eq!(out, escaped, "{input:?}");
            assert!(
                !out.contains(LINE_TERMINATORS),
                "{input:?}: escaped text holds a terminator"
            );
            assert!(
                !out.chars().any(|c| c.is_control() && c != '\t'),
                "{input:?}: escaped text holds a control"
            );
        }
    }

    /// The join escapes comment lines, indented ones included, and leaves
    /// every other line as it was given, so it never rewrites an
    /// instruction. Escaped text passes through unchanged a second time.
    #[test]
    fn join_escapes_comment_lines_only() {
        let cases: &[(&[&str], &str)] = &[
            (&["# a\nRUN x"], "# a\\nRUN x"),
            (&["  # a\u{2028}RUN x"], "  # a\\u{2028}RUN x"),
            (&["\t#a\u{1b}]0;t\u{7}"], "\t#a\\u{1b}]0;t\\u{7}"),
            (&["# a\\nb"], "# a\\nb"),
            (&["RUN a \\", "    && b"], "RUN a \\\n    && b"),
            (&["RUN echo '\u{1b}'"], "RUN echo '\u{1b}'"),
            (&["# one", "", "FROM x"], "# one\n\nFROM x"),
            (&[], ""),
        ];
        for (lines, joined) in cases {
            assert_eq!(join_lines(lines), *joined, "{lines:?}");
        }
    }
}
