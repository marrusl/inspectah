//! Comment text in rendered files.
//!
//! Characters that some reader of a rendered file treats as ending a line,
//! and the escaper that keeps them out of comment text.

/// Characters that some reader of a Containerfile treats as ending a line:
/// the Unicode mandatory line breaks (UAX #14 classes BK, CR, LF and NL).
pub const LINE_TERMINATORS: &[char] = &[
    '\n', '\r', '\u{0B}', '\u{0C}', '\u{85}', '\u{2028}', '\u{2029}',
];

/// Escape every line terminator in comment text, leaving all else as is.
///
/// The newline ends a Containerfile comment, so the rest of the value
/// would become an instruction. The other terminators in
/// [`LINE_TERMINATORS`] are escaped whatever the build parser does with
/// them, because a terminal or editor may break the line there and show a
/// reviewer a line the file does not have: a carriage return returns to
/// column 0 and overprints the comment, and editors display U+2028 and
/// U+2029 as line breaks. A newline prints as the two characters `\n`, a
/// carriage return as `\r`, and the rest in `\u{2028}` form.
///
/// Use this on a whole comment line whose fixed wording contains quotes,
/// such as `OmissionReason::describe` output, where
/// `sanitize_for_comment`'s quote doubling would alter the wording.
pub(crate) fn escape_line_terminators(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if LINE_TERMINATORS.contains(&c) => out.extend(c.escape_unicode()),
            c => out.push(c),
        }
    }
    out
}
