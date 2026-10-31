//! Shell and HTML safety functions for renderer output.

use regex::Regex;
use std::sync::LazyLock;

/// Regex matching characters that would change shell semantics if injected
/// into a RUN command. The data comes from RPM databases / systemd on an
/// operator-controlled host, so this is a safety net against corrupted
/// snapshots, not a security boundary.
static SHELL_UNSAFE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"[;&|$`"'\\<>(){}\n\r\s]"#).unwrap());

/// Regex matching valid tuned profile names: alphanumeric, hyphens,
/// underscores only. Stricter than `sanitize_shell_value` because the
/// name is interpolated directly into an echo redirect.
static TUNED_PROFILE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_-]+$").unwrap());

/// Returns `None` if `val` contains shell-unsafe characters.
/// Returns `Some(val)` if safe for shell interpolation.
pub fn sanitize_shell_value(val: &str) -> Option<&str> {
    if SHELL_UNSAFE_RE.is_match(val) {
        None
    } else {
        Some(val)
    }
}

/// Returns true if the given string is a valid tuned profile name.
pub fn is_valid_tuned_profile(name: &str) -> bool {
    TUNED_PROFILE_RE.is_match(name)
}

/// Longest path the export will write into a build instruction. Linux's
/// own `PATH_MAX`, so no path a real filesystem can hold is refused for
/// length alone.
pub const MAX_EXPORT_PATH_LEN: usize = 4096;

/// A byte allowed anywhere in an exported path: `[A-Za-z0-9._+~^@:,-]`
/// plus the component separator.
///
/// Every byte outside this set is left out on purpose. It excludes the
/// shell metacharacters `; & | $ ` ( ) { } < > ' " \` and whitespace, the
/// glob characters `* ? [ ]`, the `#` that starts a tmpfiles.d comment,
/// and the `%` that starts a tmpfiles.d specifier, so a path that passes
/// needs no quoting on either surface it reaches.
fn is_allowed_path_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'.' | b'_' | b'+' | b'~' | b'^' | b'@' | b':' | b',' | b'-' | b'/'
        )
}

/// Why a path failed [`ExportPath::parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    #[error("exported path is empty")]
    Empty,
    #[error("exported path exceeds the maximum exported path length")]
    TooLong,
    #[error("exported path is not absolute")]
    NotAbsolute,
    #[error("exported path has a disallowed byte at position {at}")]
    BadByte { at: usize },
    #[error("exported path has a `.` or `..` component")]
    RelativeComponent,
}

/// A filesystem path the export may write into a generated build
/// instruction.
///
/// [`ExportPath::parse`] is the only constructor, so a function taking an
/// `&ExportPath` cannot be reached with a path the allowlist refused.
/// That is the guarantee `inspectah_core::rpm_filename::StagedName`
/// already gives a staged RPM basename, and a `/var` directory path needs
/// the same one: it comes from a scanned host, nothing between the
/// snapshot and the renderer checks it, and it reaches a shell-form `RUN`
/// that `/bin/sh -c` runs as root at image build time as well as a
/// tmpfiles.d `d` line systemd applies at boot. Neither surface quotes
/// it.
///
/// Accepts `path` when, and only when: it starts with `/`; every byte
/// passes [`is_allowed_path_byte`]; no component is `.` or `..`, so no
/// accepted path names a directory outside the subtree its leading
/// components say; and it is no more than [`MAX_EXPORT_PATH_LEN`] bytes.
///
/// Rejection is always rejection, never stripping. A path that fails this
/// check is refused outright and the export says so in the artifact,
/// rather than being rewritten into one that passes and names a different
/// directory than the host had.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportPath(String);

impl ExportPath {
    /// Validate `path` against the allowlist above and keep it.
    pub fn parse(path: &str) -> Result<Self, PathError> {
        if path.is_empty() {
            return Err(PathError::Empty);
        }
        if path.len() > MAX_EXPORT_PATH_LEN {
            return Err(PathError::TooLong);
        }
        if !path.starts_with('/') {
            return Err(PathError::NotAbsolute);
        }
        for (i, b) in path.bytes().enumerate() {
            if !is_allowed_path_byte(b) {
                return Err(PathError::BadByte { at: i });
            }
        }
        if path.split('/').any(|c| c == "." || c == "..") {
            return Err(PathError::RelativeComponent);
        }
        Ok(Self(path.to_string()))
    }

    /// The validated path.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Escape HTML special characters to prevent XSS in report output.
pub fn html_escape(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#x27;"),
            _ => output.push(ch),
        }
    }
    output
}

/// Exact bare-word kernel parameters managed by bootloader/base image.
const KARGS_BOOTLOADER_EXACT: &[&str] =
    &["ro", "rhgb", "quiet", "splash", "nosplash", "noplymouth"];

/// Prefixes whose matching kargs are bootloader/installer-owned.
const KARGS_BOOTLOADER_PREFIXES: &[&str] = &[
    "root=",
    "rd.lvm.lv=",
    "rd.luks.uuid=",
    "resume=",
    "BOOT_IMAGE=",
    "initrd=",
    "LANG=",
    "console=",
    "crashkernel=",
];

/// Returns true if karg is a bootloader-managed parameter.
pub fn is_bootloader_karg(karg: &str) -> bool {
    if KARGS_BOOTLOADER_EXACT.contains(&karg) {
        return true;
    }
    KARGS_BOOTLOADER_PREFIXES
        .iter()
        .any(|prefix| karg.starts_with(prefix))
}

/// Extracts operator-defined kargs from a cmdline string, filtering out
/// bootloader-managed ones and those with unsafe characters.
pub fn operator_kargs(cmdline: &str) -> Vec<String> {
    cmdline
        .split_whitespace()
        .filter(|karg| !is_bootloader_karg(karg))
        .filter(|karg| sanitize_shell_value(karg).is_some())
        .map(|s| s.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_shell_value_safe() {
        assert_eq!(sanitize_shell_value("normal-pkg"), Some("normal-pkg"));
        assert_eq!(sanitize_shell_value("httpd"), Some("httpd"));
        assert_eq!(sanitize_shell_value("vim-enhanced"), Some("vim-enhanced"));
    }

    #[test]
    fn test_sanitize_shell_value_unsafe() {
        assert_eq!(sanitize_shell_value("pkg; rm -rf /"), None);
        assert_eq!(sanitize_shell_value("pkg$(whoami)"), None);
        assert_eq!(sanitize_shell_value("pkg`id`"), None);
        assert_eq!(sanitize_shell_value("pkg|cat"), None);
        assert_eq!(sanitize_shell_value("pkg&bg"), None);
        assert_eq!(sanitize_shell_value("pkg>file"), None);
        assert_eq!(sanitize_shell_value("pkg<file"), None);
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("normal text"), "normal text");
        assert_eq!(
            html_escape("<script>alert(1)</script>"),
            "&lt;script&gt;alert(1)&lt;/script&gt;"
        );
        assert_eq!(html_escape("a & b"), "a &amp; b");
        assert_eq!(html_escape("\"quoted\""), "&quot;quoted&quot;");
        assert_eq!(html_escape("it's"), "it&#x27;s");
    }

    #[test]
    fn test_html_escape_combined() {
        assert_eq!(
            html_escape("<img src=\"x\" onerror='alert(1)'>"),
            "&lt;img src=&quot;x&quot; onerror=&#x27;alert(1)&#x27;&gt;"
        );
    }

    #[test]
    fn test_is_valid_tuned_profile() {
        assert!(is_valid_tuned_profile("virtual-guest"));
        assert!(is_valid_tuned_profile("throughput-performance"));
        assert!(is_valid_tuned_profile("my_profile"));
        assert!(!is_valid_tuned_profile("profile; rm -rf /"));
        assert!(!is_valid_tuned_profile(""));
    }

    #[test]
    fn export_path_accepts_real_var_paths() {
        for path in [
            "/var",
            "/var/lib/pgsql",
            "/var/lib/appone/cache",
            "/var/lib/private/getty@tty1",
            "/var/log/anaconda",
            "/var/lib/a.b_c+d~e^f:g,h-i",
        ] {
            assert!(ExportPath::parse(path).is_ok(), "{path}");
        }
    }

    #[test]
    fn export_path_refuses_table() {
        let cases: &[(&str, PathError)] = &[
            ("", PathError::Empty),
            ("var/lib/app", PathError::NotAbsolute),
            ("/var/lib/app;id", PathError::BadByte { at: 12 }),
            ("/var/lib/app$(id)", PathError::BadByte { at: 12 }),
            ("/var/lib/app`id`", PathError::BadByte { at: 12 }),
            ("/var/lib/app id", PathError::BadByte { at: 12 }),
            ("/var/lib/app\nd /etc/shadow", PathError::BadByte { at: 12 }),
            ("/var/lib/*", PathError::BadByte { at: 9 }),
            ("/var/lib/#app", PathError::BadByte { at: 9 }),
            ("/var/lib/%t", PathError::BadByte { at: 9 }),
            ("/var/lib/../../etc/app", PathError::RelativeComponent),
            ("/var/./app", PathError::RelativeComponent),
        ];
        for (path, want) in cases {
            assert_eq!(ExportPath::parse(path), Err(*want), "{path}");
        }
        let too_long = format!("/var/{}", "a".repeat(MAX_EXPORT_PATH_LEN));
        assert_eq!(ExportPath::parse(&too_long), Err(PathError::TooLong));
    }

    #[test]
    fn test_operator_kargs() {
        let result = operator_kargs("quiet crashkernel=auto nosmt=force");
        assert!(!result.contains(&"quiet".to_string()));
        assert!(!result.contains(&"crashkernel=auto".to_string()));
        assert!(result.contains(&"nosmt=force".to_string()));
    }

    #[test]
    fn test_operator_kargs_filters_bootloader() {
        let result = operator_kargs("ro rhgb quiet root=/dev/sda1 BOOT_IMAGE=/vmlinuz nosmt=force");
        assert_eq!(result, vec!["nosmt=force"]);
    }
}
