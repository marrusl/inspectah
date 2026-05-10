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
const KARGS_BOOTLOADER_EXACT: &[&str] = &[
    "ro", "rhgb", "quiet", "splash", "nosplash", "noplymouth",
];

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
        assert_eq!(html_escape("<script>alert(1)</script>"), "&lt;script&gt;alert(1)&lt;/script&gt;");
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
