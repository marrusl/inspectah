//! The unit specifiers inspectah expands when it reads a unit file: `%i`,
//! `%I`, `%p`, `%N` and `%%`. The `[Install]` reader expands them in
//! `DefaultInstance=` and in the names its keys assign (§ 7.2 of the units
//! spec), and the scheduled inspector in a timer's `Unit=` (§ 8). Any other
//! specifier is not expanded, and the caller declines the value.

/// `value` with `%i`, `%I`, `%p`, `%N` and `%%` expanded for the unit
/// `name`, given without its type suffix (`worker@a` for
/// `worker@a.service`), or `None` for any other specifier.
pub(crate) fn expand_unit_specifiers(value: &str, name: &str) -> Option<String> {
    let (prefix, instance) = match name.split_once('@') {
        Some((prefix, instance)) => (prefix, instance),
        None => (name, ""),
    };
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'i' => out.push_str(instance),
            'I' => out.push_str(&unescape_unit_text(instance)?),
            'p' => out.push_str(prefix),
            'N' => out.push_str(name),
            '%' => out.push('%'),
            _ => return None,
        }
    }
    Some(out)
}

/// systemd's unescaping of a unit name part: `-` is `/` and `\xNN` is
/// that byte. `None` when an escape is malformed or the result is not
/// UTF-8.
fn unescape_unit_text(text: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&b, tail)) = rest.split_first() {
        match b {
            b'-' => {
                bytes.push(b'/');
                rest = tail;
            }
            b'\\' => {
                let hex = tail.strip_prefix(b"x")?.get(..2)?;
                bytes.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
                rest = &tail[3..];
            }
            other => {
                bytes.push(other);
                rest = tail;
            }
        }
    }
    String::from_utf8(bytes).ok()
}
