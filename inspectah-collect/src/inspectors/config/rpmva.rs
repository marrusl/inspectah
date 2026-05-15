/// Parsed `rpm -Va` output entry with individual flag decomposition.
///
/// Each line of `rpm -Va` output looks like:
/// ```text
/// S.5....T.  c /etc/httpd/conf/httpd.conf
/// ```
///
/// The 9-character flag field indicates which attributes differ:
/// S=size, M=mode, 5=md5/sha256, D=device, L=link, U=user, G=group, T=mtime, P=caps.
///
/// The attribute character (c, d, g, l, r) indicates the file type.
/// The keyword `missing` indicates the file has been deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpmVaLine {
    /// Raw 9-character flag string (e.g., "S.5....T.")
    pub flags: String,
    /// Attribute type: c=config, d=doc, g=ghost, l=license, r=readme
    pub attribute: Option<char>,
    /// Absolute path to the file
    pub path: String,
    /// Whether the file is missing (deleted)
    pub missing: bool,
    /// Individual flag decomposition
    pub size_changed: bool,
    pub mode_changed: bool,
    pub digest_changed: bool,
    pub device_changed: bool,
    pub link_changed: bool,
    pub user_changed: bool,
    pub group_changed: bool,
    pub mtime_changed: bool,
    pub caps_changed: bool,
}

/// Parses a single line of `rpm -Va` output.
///
/// Returns `None` for blank or unparseable lines.
///
/// Expected formats:
/// - `S.5....T.  c /etc/httpd/conf/httpd.conf` — normal modified file
/// - `missing    c /etc/deleted.conf` — deleted file
/// - `SM5DLUGTP c /path` — all flags set
pub fn parse_rpm_va_line(line: &str) -> Option<RpmVaLine> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Handle "missing" keyword
    if let Some(stripped) = trimmed.strip_prefix("missing") {
        let rest = stripped.trim_start();
        let (attribute, path) = parse_attr_and_path(rest)?;
        return Some(RpmVaLine {
            flags: "missing".into(),
            attribute,
            path,
            missing: true,
            size_changed: false,
            mode_changed: false,
            digest_changed: false,
            device_changed: false,
            link_changed: false,
            user_changed: false,
            group_changed: false,
            mtime_changed: false,
            caps_changed: false,
        });
    }

    // Normal flag format: 9 characters + whitespace + optional attribute + path
    if trimmed.len() < 9 {
        return None;
    }

    let flag_str = &trimmed[..9];
    let rest = trimmed[9..].trim_start();
    let (attribute, path) = parse_attr_and_path(rest)?;

    let chars: Vec<char> = flag_str.chars().collect();
    if chars.len() != 9 {
        return None;
    }

    Some(RpmVaLine {
        flags: flag_str.to_string(),
        attribute,
        path,
        missing: false,
        size_changed: chars[0] == 'S',
        mode_changed: chars[1] == 'M',
        digest_changed: chars[2] == '5',
        device_changed: chars[3] == 'D',
        link_changed: chars[4] == 'L',
        user_changed: chars[5] == 'U',
        group_changed: chars[6] == 'G',
        mtime_changed: chars[7] == 'T',
        caps_changed: chars[8] == 'P',
    })
}

/// Parses the attribute character and path from the remainder after flags.
///
/// The attribute is a single character (c, d, g, l, r) followed by whitespace
/// and then the path. If no attribute character, the path starts immediately.
fn parse_attr_and_path(rest: &str) -> Option<(Option<char>, String)> {
    if rest.is_empty() {
        return None;
    }

    // Check if the first non-whitespace char is a known attribute
    let trimmed = rest.trim_start();
    if trimmed.is_empty() {
        return None;
    }

    let first = trimmed.chars().next()?;
    if matches!(first, 'c' | 'd' | 'g' | 'l' | 'r') {
        // Must be followed by whitespace and then a path
        let after_attr = &trimmed[1..];
        let path = after_attr.trim_start();
        if path.is_empty() {
            return None;
        }
        Some((Some(first), path.to_string()))
    } else if first == '/' {
        // No attribute, path starts directly
        Some((None, trimmed.to_string()))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Test 7: test_parse_rpm_va_line ----

    #[test]
    fn test_parse_rpm_va_line() {
        let entry =
            parse_rpm_va_line("S.5....T.  c /etc/httpd/conf/httpd.conf").expect("should parse");
        assert_eq!(entry.flags, "S.5....T.");
        assert_eq!(entry.attribute, Some('c'));
        assert_eq!(entry.path, "/etc/httpd/conf/httpd.conf");
        assert!(!entry.missing);
        assert!(entry.size_changed);
        assert!(!entry.mode_changed);
        assert!(entry.digest_changed);
        assert!(!entry.device_changed);
        assert!(!entry.link_changed);
        assert!(!entry.user_changed);
        assert!(!entry.group_changed);
        assert!(entry.mtime_changed);
        assert!(!entry.caps_changed);
    }

    // ---- Test 8: test_parse_rpm_va_missing ----

    #[test]
    fn test_parse_rpm_va_missing() {
        let entry = parse_rpm_va_line("missing    c /etc/deleted.conf").expect("should parse");
        assert_eq!(entry.flags, "missing");
        assert_eq!(entry.attribute, Some('c'));
        assert_eq!(entry.path, "/etc/deleted.conf");
        assert!(entry.missing);
        assert!(!entry.size_changed);
    }

    // ---- Test 9: test_parse_rpm_va_all_flags ----

    #[test]
    fn test_parse_rpm_va_all_flags() {
        let entry = parse_rpm_va_line("SM5DLUGTP c /path").expect("should parse");
        assert_eq!(entry.flags, "SM5DLUGTP");
        assert!(entry.size_changed);
        assert!(entry.mode_changed);
        assert!(entry.digest_changed);
        assert!(entry.device_changed);
        assert!(entry.link_changed);
        assert!(entry.user_changed);
        assert!(entry.group_changed);
        assert!(entry.mtime_changed);
        assert!(entry.caps_changed);
    }
}
