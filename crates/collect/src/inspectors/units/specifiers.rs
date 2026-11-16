//! The unit specifiers inspectah expands when it reads a unit file. The
//! `[Install]` reader expands `%i`, `%p`, `%N` and `%%` in
//! `DefaultInstance=` and in the names its keys assign (§ 7.2 of the units
//! spec); the instance reader (§ 7.5) expands those, `%I` and the system
//! manager's directory specifiers ([`SYSTEM_DIRECTORY_SPECIFIERS`]); the
//! scheduled inspector, for a timer's `Unit=` (§ 8), expands `%i`, `%p`,
//! `%N` and `%%` and ignores a value systemd ignores
//! ([`trigger_unit_ignored`]). Any other specifier is not expanded, and the
//! caller declines the value.

/// The specifiers one kind of read expands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpecifierSet {
    /// A unit setting systemd reads with `unit_full_printf` when it loads
    /// the unit, such as an instance's `ExecStart=`: `%i`, `%I`, `%p`,
    /// `%N`, `%%` and [`SYSTEM_DIRECTORY_SPECIFIERS`].
    Runtime,
    /// A timer's `Unit=`, which systemd expands with `unit_name_printf`
    /// (252 and 257, `src/core/unit-printf.c`): `%i`, `%p`, `%N` and `%%`.
    /// That table also has `%j`, `%n` and the system and credential
    /// specifiers, which inspectah does not expand. It has no `%I` and no
    /// directory specifier, and systemd ignores a value holding one
    /// ([`trigger_unit_ignored`]).
    TriggerUnit,
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

/// Every specifier systemd's `unit_name_printf` resolves (252 and 257,
/// `src/core/unit-printf.c`): its own `%i`, `%j`, `%n`, `%N` and `%p`, then
/// `COMMON_SYSTEM_SPECIFIERS` and `COMMON_CREDS_SPECIFIERS`
/// (`src/shared/specifier.h`).
const UNIT_NAME_PRINTF_SPECIFIERS: &str = "ijnNpaAbBHlqmMovwWgGuU";

/// Whether systemd ignores the timer `Unit=` value `value` for its
/// specifiers: `specifier_printf` (`src/shared/specifier.c`, 252 and 257)
/// fails with `EBADSLT` ("Invalid slot") on a letter or digit after `%`
/// that is not in [`UNIT_NAME_PRINTF_SPECIFIERS`], such as `%I` or a
/// directory specifier, and `config_parse_trigger_unit`
/// (`src/core/load-fragment.c`) then ignores the assignment as if it were
/// absent. `%%` is a literal `%`, and any other character after `%` is
/// kept as written.
pub(crate) fn trigger_unit_ignored(value: &str) -> bool {
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '%'
            && chars.next().is_some_and(|specifier| {
                specifier.is_ascii_alphanumeric()
                    && !UNIT_NAME_PRINTF_SPECIFIERS.contains(specifier)
            })
        {
            return true;
        }
    }
    false
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
    /// `install-printf.c`), and in a timer's `Unit=`, whose table has none
    /// either (`unit-printf.c`). A specifier in neither set stays
    /// unexpanded.
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
            for set in [SpecifierSet::Install, SpecifierSet::TriggerUnit] {
                assert_eq!(
                    expand_unit_specifiers(value, "w@a", set),
                    None,
                    "{value} in {set:?}"
                );
            }
        }
    }

    /// A timer's `Unit=` is ignored for a letter or digit after `%` that
    /// `unit_name_printf`'s table lacks, wherever it sits, and not for one
    /// the table has, for `%%`, or for punctuation after `%`.
    #[test]
    fn a_trigger_unit_is_ignored_for_a_specifier_outside_systemds_table() {
        for (value, ignored) in [
            ("x@%I.service", true),
            ("%t.service", true),
            ("x-%h.service", true),
            ("%H-%I.service", true),
            ("x-%0.service", true),
            ("x@%i.service", false),
            ("%p-%N.service", false),
            ("%H.service", false),
            ("%n-%j-%u.service", false),
            ("100%%I.service", false),
            ("x%-.service", false),
            ("x%", false),
            ("x.service", false),
        ] {
            assert_eq!(trigger_unit_ignored(value), ignored, "{value}");
        }
    }
}
