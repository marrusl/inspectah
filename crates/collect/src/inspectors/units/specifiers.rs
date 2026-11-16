//! The unit specifiers inspectah expands when it reads a unit file. The
//! `[Install]` reader expands `%i`, `%p`, `%N` and `%%` in
//! `DefaultInstance=` and in the names its keys assign (§ 7.2 of the units
//! spec); the instance reader (§ 7.5) and the scheduled inspector, for a
//! timer's `Unit=` (§ 8), expand those, `%I` and the system manager's
//! directory specifiers ([`SYSTEM_DIRECTORY_SPECIFIERS`]). Any other
//! specifier is not expanded, and the caller declines the value.

/// The specifiers one kind of read expands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpecifierSet {
    /// A unit setting systemd reads when it loads the unit, such as an
    /// instance's `ExecStart=` or a timer's `Unit=`: `%i`, `%I`, `%p`, `%N`,
    /// `%%` and [`SYSTEM_DIRECTORY_SPECIFIERS`].
    Runtime,
    /// `[Install]`, as `systemctl enable` expands it: `%i`, `%p`, `%N` and
    /// `%%`. systemd's install expansion table has no `%I` and no directory
    /// specifier (252 and 257, `install-printf.c`: `%i`, `%j`, `%n`, `%N`,
    /// `%p` and the system and credential ones), so a value holding one is
    /// not expanded.
    Install,
}

/// The directory specifiers whose value is fixed for the system manager,
/// and that value, in `unit_full_printf_full`'s table (systemd 252 and 257,
/// `src/core/unit-printf.c`):
///
/// - `%t`, `%S`, `%C`, `%L` and `%E` are `specifier_special_directory`,
///   the manager's `prefix[]` for `EXEC_DIRECTORY_RUNTIME`, `_STATE`,
///   `_CACHE`, `_LOGS` and `_CONFIGURATION`. `manager_setup_prefix`
///   (`src/core/manager.c`) fills it for the system manager from
///   `paths_system`: `SD_PATH_SYSTEM_RUNTIME`, `_STATE_PRIVATE`,
///   `_STATE_CACHE`, `_STATE_LOGS` and `_CONFIGURATION`, which
///   `sd_path_lookup` (`src/libsystemd/sd-path/sd-path.c`) gives as `/run`,
///   `/var/lib`, `/var/cache`, `/var/log` and `/etc`. Only the user
///   manager reads `$XDG_*` here.
/// - `%T` and `%V` are `specifier_tmp_dir` and `specifier_var_tmp_dir`
///   (`src/shared/specifier.c`), `tmp_dir` and `var_tmp_dir`
///   (`src/basic/fs-util.c`): the manager's own `$TMPDIR`, `$TEMP` or
///   `$TMP` when one names a directory, otherwise `/tmp` and `/var/tmp`. A
///   stock host gives PID 1 none of them; one set through the kernel
///   command line or `ManagerEnvironment=` is not read, and the scan then
///   expands these two differently from the host.
///
/// `%D` (`/usr/share`, 257 only), `%h`, `%d` and the rest are not here: they
/// depend on the release, the unit or the running user.
const SYSTEM_DIRECTORY_SPECIFIERS: [(char, &str); 7] = [
    ('t', "/run"),
    ('S', "/var/lib"),
    ('C', "/var/cache"),
    ('L', "/var/log"),
    ('E', "/etc"),
    ('T', "/tmp"),
    ('V', "/var/tmp"),
];

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
            other if set == SpecifierSet::Runtime => out.push_str(
                SYSTEM_DIRECTORY_SPECIFIERS
                    .iter()
                    .find(|(specifier, _)| *specifier == other)
                    .map(|(_, dir)| *dir)?,
            ),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The system manager's fixed directory specifiers expand in the
    /// runtime set, beside an instance specifier, and stay unexpanded in
    /// `[Install]`, whose table has none of them (252 and 257,
    /// `install-printf.c`). A specifier in neither set stays unexpanded.
    #[test]
    fn directory_specifiers_expand_at_runtime_only() {
        for (value, runtime) in [
            (
                "%t/openvpn-server/status-%i.log",
                Some("/run/openvpn-server/status-a.log"),
            ),
            ("%S/w/%i", Some("/var/lib/w/a")),
            ("%C/w/%i", Some("/var/cache/w/a")),
            ("%L/w/%i", Some("/var/log/w/a")),
            ("%E/w/%i.conf", Some("/etc/w/a.conf")),
            ("%T/w-%i", Some("/tmp/w-a")),
            ("%V/w-%i", Some("/var/tmp/w-a")),
            ("%h/w/%i", None),
            ("%H-%i", None),
            ("%D/w", None),
        ] {
            assert_eq!(
                expand_unit_specifiers(value, "w@a", SpecifierSet::Runtime).as_deref(),
                runtime,
                "{value}"
            );
            assert_eq!(
                expand_unit_specifiers(value, "w@a", SpecifierSet::Install),
                None,
                "{value} in [Install]"
            );
        }
    }
}
