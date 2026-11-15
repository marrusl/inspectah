//! The unit specifiers inspectah expands when it reads a unit file. The
//! `[Install]` reader expands `%i`, `%p`, `%N` and `%%` in
//! `DefaultInstance=` and in the names its keys assign (§ 7.2 of the units
//! spec); the scheduled inspector expands those and `%I` in a timer's
//! `Unit=` (§ 8). Any other specifier is not expanded, and the caller
//! declines the value.

/// The specifiers one kind of read expands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpecifierSet {
    /// A unit setting systemd reads when it loads the unit, such as a
    /// timer's `Unit=`: `%i`, `%I`, `%p`, `%N` and `%%`.
    Runtime,
    /// `[Install]`, as `systemctl enable` expands it: `%i`, `%p`, `%N` and
    /// `%%`. systemd's install expansion table has no `%I` (252 and 257,
    /// `install-printf.c`), so a value holding one is not expanded.
    Install,
}

/// `value` with the specifiers of `set` expanded for the unit `name`,
/// given without its type suffix (`worker@a` for `worker@a.service`), or
/// `None` for any other specifier.
pub(crate) fn expand_unit_specifiers(value: &str, name: &str, set: SpecifierSet) -> Option<String> {
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
            'I' if set == SpecifierSet::Runtime => out.push_str(&unescape_unit_text(instance)?),
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
