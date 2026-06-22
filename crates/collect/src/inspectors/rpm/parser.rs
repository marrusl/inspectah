use inspectah_core::types::rpm::PackageEntry;

/// Parse "epoch:name-version-release.arch" format from `rpm -qa --queryformat`.
/// Format: `%{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}`
pub fn parse_nevra(line: &str) -> Option<PackageEntry> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    // Split epoch:rest
    let (epoch_str, rest) = line.split_once(':')?;
    let epoch = if epoch_str == "(none)" {
        "0"
    } else {
        epoch_str
    };

    // Split rest into name-version-release.arch
    // Find the last '.' → arch separator
    let dot_pos = rest.rfind('.')?;
    let arch = &rest[dot_pos + 1..];
    let name_ver_rel = &rest[..dot_pos];

    // Find the second-to-last '-' → version-release separator
    let rel_dash = name_ver_rel.rfind('-')?;
    let release = &name_ver_rel[rel_dash + 1..];
    let name_ver = &name_ver_rel[..rel_dash];

    // Find the last '-' in name_ver → name-version separator
    let ver_dash = name_ver.rfind('-')?;
    let version = &name_ver[ver_dash + 1..];
    let name = &name_ver[..ver_dash];

    Some(PackageEntry {
        name: name.into(),
        epoch: epoch.into(),
        version: version.into(),
        release: release.into(),
        arch: arch.into(),
        ..Default::default()
    })
}

/// RPM version comparison algorithm (rpmvercmp).
/// Implements the same algorithm as librpm's C rpmvercmp.
pub fn rpmvercmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    if a == b {
        return Ordering::Equal;
    }

    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();

    loop {
        // Skip non-alphanumeric, non-tilde, non-caret characters
        while ai
            .peek()
            .is_some_and(|c| !c.is_alphanumeric() && *c != '~' && *c != '^')
        {
            ai.next();
        }
        while bi
            .peek()
            .is_some_and(|c| !c.is_alphanumeric() && *c != '~' && *c != '^')
        {
            bi.next();
        }

        // Handle tilde (sorts before everything)
        match (ai.peek(), bi.peek()) {
            (Some('~'), Some('~')) => {
                ai.next();
                bi.next();
                continue;
            }
            (Some('~'), _) => return Ordering::Less,
            (_, Some('~')) => return Ordering::Greater,
            _ => {}
        }

        // Handle caret (sorts after empty, before other characters)
        match (ai.peek(), bi.peek()) {
            (Some('^'), Some('^')) => {
                ai.next();
                bi.next();
                continue;
            }
            (Some('^'), None) => return Ordering::Greater,
            (None, Some('^')) => return Ordering::Less,
            (Some('^'), _) => return Ordering::Less,
            (_, Some('^')) => return Ordering::Greater,
            _ => {}
        }

        // End of both strings
        if ai.peek().is_none() && bi.peek().is_none() {
            return Ordering::Equal;
        }

        // One string ended before the other
        if ai.peek().is_none() {
            return Ordering::Less;
        }
        if bi.peek().is_none() {
            return Ordering::Greater;
        }

        // Determine segment types independently for each side
        let a_is_digit = ai.peek().unwrap().is_ascii_digit();
        let b_is_digit = bi.peek().unwrap().is_ascii_digit();

        let seg_a: String = if a_is_digit {
            collect_while(&mut ai, |c| c.is_ascii_digit())
        } else {
            collect_while(&mut ai, |c| c.is_alphabetic())
        };

        let seg_b: String = if b_is_digit {
            collect_while(&mut bi, |c| c.is_ascii_digit())
        } else {
            collect_while(&mut bi, |c| c.is_alphabetic())
        };

        // If segment types differ, digits always win (per librpm C algorithm)
        match (a_is_digit, b_is_digit) {
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            (true, true) => {
                let na: u64 = seg_a.parse().unwrap_or(0);
                let nb: u64 = seg_b.parse().unwrap_or(0);
                let cmp = na.cmp(&nb);
                if cmp != Ordering::Equal {
                    return cmp;
                }
            }
            (false, false) => {
                let cmp = seg_a.cmp(&seg_b);
                if cmp != Ordering::Equal {
                    return cmp;
                }
            }
        }
    }
}

fn collect_while(
    iter: &mut std::iter::Peekable<std::str::Chars>,
    pred: impl Fn(char) -> bool,
) -> String {
    let mut s = String::new();
    while iter.peek().is_some_and(|c| pred(*c)) {
        s.push(iter.next().unwrap());
    }
    s
}

/// Package names that are always excluded from scan output.
/// inspectah is the scanning tool itself — including it in migration
/// output would be nonsensical.
const SELF_EXCLUDE_PACKAGES: &[&str] = &["inspectah"];

/// Parse the output of `rpm -qa --queryformat` into PackageEntry list.
/// Filters gpg-pubkey virtual packages and the inspectah package itself.
pub fn parse_rpm_qa(output: &str) -> Vec<PackageEntry> {
    output
        .lines()
        .filter_map(|line| {
            let entry = parse_nevra(line)?;
            if entry.name == "gpg-pubkey" {
                return None;
            }
            if SELF_EXCLUDE_PACKAGES.contains(&entry.name.as_str()) {
                return None;
            }
            Some(entry)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_nevra_standard() {
        let entry = parse_nevra("0:bash-5.2.26-3.el9.x86_64").unwrap();
        assert_eq!(entry.epoch, "0");
        assert_eq!(entry.name, "bash");
        assert_eq!(entry.version, "5.2.26");
        assert_eq!(entry.release, "3.el9");
        assert_eq!(entry.arch, "x86_64");
    }

    #[test]
    fn test_parse_nevra_no_epoch() {
        let entry = parse_nevra("(none):httpd-2.4.57-5.el9.x86_64").unwrap();
        assert_eq!(entry.epoch, "0");
        assert_eq!(entry.name, "httpd");
    }

    #[test]
    fn test_parse_nevra_noarch() {
        let entry = parse_nevra("0:tzdata-2024a-1.el9.noarch").unwrap();
        assert_eq!(entry.arch, "noarch");
    }

    #[test]
    fn test_rpmvercmp_numeric() {
        assert_eq!(rpmvercmp("1.2.3", "1.2.3"), std::cmp::Ordering::Equal);
        assert_eq!(rpmvercmp("1.2.4", "1.2.3"), std::cmp::Ordering::Greater);
        assert_eq!(rpmvercmp("1.2.3", "1.2.4"), std::cmp::Ordering::Less);
    }

    #[test]
    fn test_rpmvercmp_tilde() {
        // Tilde sorts before anything, even empty
        assert_eq!(rpmvercmp("1.0~rc1", "1.0"), std::cmp::Ordering::Less);
        assert_eq!(rpmvercmp("1.0", "1.0~rc1"), std::cmp::Ordering::Greater);
    }

    #[test]
    fn test_rpmvercmp_caret() {
        // Caret sorts after empty but before any other character
        assert_eq!(rpmvercmp("1.0^git1", "1.0"), std::cmp::Ordering::Greater);
        assert_eq!(rpmvercmp("1.0^git1", "1.0.1"), std::cmp::Ordering::Less);
    }

    #[test]
    fn test_rpmvercmp_mixed_alpha_numeric_antisymmetry() {
        // Digits always win over alpha — must be anti-symmetric
        assert_eq!(rpmvercmp("1", "a"), std::cmp::Ordering::Greater);
        assert_eq!(rpmvercmp("a", "1"), std::cmp::Ordering::Less);
    }

    #[test]
    fn test_rpmvercmp_leading_zeros() {
        assert_eq!(rpmvercmp("001", "1"), std::cmp::Ordering::Equal);
        assert_eq!(rpmvercmp("01.0", "1.0"), std::cmp::Ordering::Equal);
    }

    #[test]
    fn test_rpmvercmp_large_numeric() {
        assert_eq!(
            rpmvercmp("99999999999", "99999999998"),
            std::cmp::Ordering::Greater
        );
    }

    // --- Self-exclusion tests ---

    #[test]
    fn test_parse_rpm_qa_excludes_inspectah() {
        let output = "\
0:bash-5.2.26-3.el9.x86_64
0:inspectah-0.8.0-1.el9.x86_64
0:httpd-2.4.57-5.el9.x86_64
0:gpg-pubkey-fd431d51-4ae0493b.x86_64
";
        let packages = parse_rpm_qa(output);
        let names: Vec<&str> = packages.iter().map(|p| p.name.as_str()).collect();

        assert!(names.contains(&"bash"), "bash must be included");
        assert!(names.contains(&"httpd"), "httpd must be included");
        assert!(
            !names.contains(&"inspectah"),
            "inspectah must be excluded from scan output"
        );
        assert!(
            !names.contains(&"gpg-pubkey"),
            "gpg-pubkey must still be excluded"
        );
        assert_eq!(packages.len(), 2, "only bash and httpd should remain");
    }

    #[test]
    fn test_parse_rpm_qa_without_inspectah_unaffected() {
        let output = "\
0:bash-5.2.26-3.el9.x86_64
0:httpd-2.4.57-5.el9.x86_64
";
        let packages = parse_rpm_qa(output);
        assert_eq!(packages.len(), 2, "normal packages should not be filtered");
    }
}
