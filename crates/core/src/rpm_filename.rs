//! RPM filename parsing and the staged filename allowlist.
//!
//! Four places used to parse or sanitize RPM filenames independently and
//! disagreed with each other. This module is the one place a filename is
//! proved safe, and it does that with two deliberately separate functions:
//!
//! - [`RpmFilename::parse_untrusted`] matches filename structure to recover
//!   a package's name, arch, and version-release for lookups. It is
//!   matching, not a safety check: it accepts untrusted strings and a
//!   `Some` result is not proof the original string is safe to use as a
//!   path or a shell argument. The name says so at the call site, not just
//!   in a doc comment someone has to open the file to read.
//! - [`validate_staged_name`] is the safety check: an allowlist a basename
//!   must pass before it reaches the filesystem, a `COPY`, or a `RUN`.
//!   Rejection is always rejection, never stripping -- a name that fails
//!   this check is refused outright, never sanitized into an accepted one.
//!
//! Callers that stage a filename for export must run `validate_staged_name`
//! before trusting anything `RpmFilename::parse_untrusted` reports about it.
//! [`StagedName`] is a name that has passed that check, and the staging
//! boundary takes it rather than a `&str` so the check cannot be skipped.
//!
//! This lives in `core` rather than next to either of its users, and that
//! is forced rather than chosen: the two boundaries that must agree are
//! the staging rename in `refine` and the Containerfile `COPY`/`RUN` in
//! `pipeline`, and `refine` depends on `pipeline`. A type that carries an
//! invariant across that edge has to sit below both, or each crate gets
//! its own allowlist -- which is the exact defect this module was created
//! to end.
//!
//! Three properties of [`StagedName`] are load-bearing and easy to undo
//! by accident. Its field is private, so the defining module is the only
//! place one can be made. Its derive list has no `Deserialize`, so a
//! snapshot cannot hand the renderer a `StagedName` and make the whole
//! structure decorative. And it has no `Default` and no `From<String>`,
//! so there is no constructor that skips [`validate_staged_name`].

use serde::Serialize;

/// Known RPM architectures for filename parsing.
const KNOWN_ARCHES: &[&str] = &[
    "x86_64", "aarch64", "ppc64le", "s390x", "i686", "i386", "armv7hl", "noarch", "src",
];

/// Longest basename the upload path accepts; real RPM names stay under 150.
pub const MAX_UPLOAD_FILENAME_LEN: usize = 200;

/// An RPM filename decomposed into name, architecture, and optional
/// version-release.
///
/// Produced only by [`RpmFilename::parse_untrusted`], which matches filename
/// structure -- this is not proof the filename is safe to use as a path or
/// shell argument. Call [`validate_staged_name`] first for anything that
/// reaches the filesystem or a rendered Containerfile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RpmFilename {
    pub name: String,
    pub arch: String,
    pub version_release: Option<String>,
}

impl RpmFilename {
    /// Parse an RPM filename into name, arch, and optional version-release.
    ///
    /// Tries two strategies:
    /// 1. Standard NEVRA: `name-version-release.arch.rpm` — split on the arch
    ///    segment and extract version-release from between name and arch.
    /// 2. Non-standard: scan for a known arch before `.rpm` and treat everything
    ///    before the version-start (`-\d`) as the package name.
    ///
    /// The `.rpm` suffix is matched case-insensitively. Returns `None` if the
    /// filename has no recognizable arch or name.
    ///
    /// Parsing is matching, not a safety check: a `Some` result says nothing
    /// about whether `filename` is safe to use as a path or shell argument.
    /// `_untrusted` in the name is deliberate: call [`validate_staged_name`]
    /// before using anything this returns for a path or shell argument.
    pub fn parse_untrusted(filename: &str) -> Option<Self> {
        let stem = strip_rpm_suffix(filename)?;

        // Find the arch: last dot- or underscore-separated segment that is a
        // known arch.
        // E.g. "epel-release-latest-10.noarch" → arch = "noarch"
        // E.g. "google-chrome-stable_current_x86_64" → need underscore check too
        let arch = KNOWN_ARCHES.iter().find(|&&a| {
            // Check `.arch` suffix (standard)
            stem.ends_with(&format!(".{a}"))
                // Check `_arch` suffix (vendor convention like google-chrome)
                || stem.ends_with(&format!("_{a}"))
        })?;

        // Strip the arch suffix (and its separator) to get name-version-release
        // (or just name).
        let before_arch = &stem[..stem.len() - arch.len() - 1];

        if before_arch.is_empty() {
            return None;
        }

        // Extract name: everything before the first `-\d` pattern (version start).
        // E.g. "epel-release-latest-10" → name = "epel-release-latest", ver = "10"
        // E.g. "google-chrome-stable_current" → name = "google-chrome-stable_current", ver = None
        let mut name_end = None;
        let bytes = before_arch.as_bytes();
        for i in 0..bytes.len().saturating_sub(1) {
            if bytes[i] == b'-' && bytes[i + 1].is_ascii_digit() {
                name_end = Some(i);
                break;
            }
        }

        match name_end {
            Some(pos) => {
                let name = &before_arch[..pos];
                let version = &before_arch[pos + 1..]; // skip the leading '-'
                if name.is_empty() {
                    None
                } else {
                    Some(RpmFilename {
                        name: name.to_string(),
                        arch: arch.to_string(),
                        version_release: Some(version.to_string()),
                    })
                }
            }
            None => {
                // No version detected — use the whole thing as the name
                Some(RpmFilename {
                    name: before_arch.to_string(),
                    arch: arch.to_string(),
                    version_release: None,
                })
            }
        }
    }
}

/// Returns the bytes before a case-insensitive `.rpm` suffix, or `None` if
/// `s` does not end with one.
///
/// Byte-indexed and panic-safe: `str::get` returns `None` instead of
/// panicking when the split point falls inside a multi-byte character,
/// which a raw slice index would not.
fn strip_rpm_suffix(s: &str) -> Option<&str> {
    let split_at = s.len().checked_sub(4)?;
    let suffix = s.get(split_at..)?;
    suffix.eq_ignore_ascii_case(".rpm").then(|| &s[..split_at])
}

/// A byte allowed anywhere in a staged RPM filename: `[A-Za-z0-9._+~^-]`.
fn is_allowed_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'~' | b'^' | b'-')
}

/// Why a staged filename failed [`validate_staged_name`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    #[error("staged filename is empty")]
    Empty,
    #[error("staged filename exceeds the maximum staged name length")]
    TooLong,
    #[error("staged filename must start with a letter or digit")]
    BadFirstByte,
    #[error("staged filename has a disallowed byte at position {at}")]
    BadByte { at: usize },
    #[error("staged filename does not end in .rpm")]
    NotRpm,
}

/// The allowlist a staged RPM basename must pass before it reaches the
/// filesystem, a `COPY`, or a `RUN`.
///
/// Accepts `name` when, and only when: every byte is in
/// `[A-Za-z0-9._+~^-]`; the first byte is `[A-Za-z0-9]` (no leading dot, no
/// leading `-`, so no hidden file and no option injection); it ends in
/// `.rpm` case-insensitively; and it is no more than
/// [`MAX_UPLOAD_FILENAME_LEN`] bytes. There is no explicit minimum-length
/// check: the first-byte and `.rpm`-suffix rules together make 5 bytes
/// (one alphanumeric byte plus `.rpm`) the shortest name that can pass, so
/// the floor is a consequence of those two rules, not a check of its own --
/// relaxing either one changes it without this doc comment knowing.
///
/// Rejection is always rejection, never stripping: a name that fails this
/// check is refused outright. Nothing here sanitizes `name` into an
/// accepted form, so `../x.rpm` is rejected, never turned into `x.rpm`.
pub fn validate_staged_name(name: &str) -> Result<(), NameError> {
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name.len() > MAX_UPLOAD_FILENAME_LEN {
        return Err(NameError::TooLong);
    }
    if strip_rpm_suffix(name).is_none() {
        return Err(NameError::NotRpm);
    }
    // `name` is non-empty, so there is always a byte 0 to check.
    if !name.as_bytes()[0].is_ascii_alphanumeric() {
        return Err(NameError::BadFirstByte);
    }
    // Byte 0 already passed the (stricter) alphanumeric check above, so it
    // cannot also fail the general allowlist; skip it here.
    for (i, b) in name.bytes().enumerate().skip(1) {
        if !is_allowed_name_byte(b) {
            return Err(NameError::BadByte { at: i });
        }
    }
    Ok(())
}

/// A basename that has passed [`validate_staged_name`].
///
/// [`StagedName::parse`] is the only constructor, so a function taking a
/// `&StagedName` cannot be reached with a name the allowlist rejected. The
/// staging boundary -- the rename that puts a file in the upload directory
/// under its real name -- takes this type rather than a `&str`, which is
/// what keeps "validate before you write" from being a convention a new
/// call site can forget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedName(String);

impl StagedName {
    /// Validate `name` against [`validate_staged_name`] and keep it.
    pub fn parse(name: &str) -> Result<Self, NameError> {
        validate_staged_name(name)?;
        Ok(Self(name.to_string()))
    }

    /// The validated basename.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpm_filename_parse_table() {
        // § 8.6, the executed matcher table, plus two cases the table does
        // not cover that the pre-move tests in session.rs exercised
        // directly: the dotted-arch standard NEVRA shape (KNOWN_ARCHES'
        // first entry, x86_64, was otherwise only reached through the
        // underscore branch below) and the underscore-arch branch itself
        // (vendor filenames like google-chrome), which the upload
        // matcher's own doc comment relies on as its justifying example.
        type Parsed<'a> = Option<(&'a str, &'a str, Option<&'a str>)>;
        let cases: &[(&str, Parsed)] = &[
            (
                "httpd-2.4.57-5.el9.x86_64.rpm",
                Some(("httpd", "x86_64", Some("2.4.57-5.el9"))),
            ),
            (
                "epel-release-latest-9.noarch.rpm",
                Some(("epel-release-latest", "noarch", Some("9"))),
            ),
            (
                "epel-release-9-11.el9.noarch.rpm",
                Some(("epel-release", "noarch", Some("9-11.el9"))),
            ),
            (
                "epel-release-9-10.el9.noarch.rpm",
                Some(("epel-release", "noarch", Some("9-10.el9"))),
            ),
            (
                "epel-release-latest-10.noarch.rpm",
                Some(("epel-release-latest", "noarch", Some("10"))),
            ),
            (
                "epel-release.noarch.rpm",
                Some(("epel-release", "noarch", None)),
            ),
            (
                "epel-release-latest-9.noarch.RPM",
                Some(("epel-release-latest", "noarch", Some("9"))),
            ),
            ("epel-release-latest-9.noarch (1).rpm", None),
            ("epel-release-latest-9.noarch-1.rpm", None),
            ("epel-release-latest-9.rpm", None),
            (
                "google-chrome-stable_current_x86_64.rpm",
                Some(("google-chrome-stable_current", "x86_64", None)),
            ),
        ];
        for (input, want) in cases {
            let got = RpmFilename::parse_untrusted(input);
            let got = got.as_ref().map(|r| {
                (
                    r.name.as_str(),
                    r.arch.as_str(),
                    r.version_release.as_deref(),
                )
            });
            assert_eq!(got, *want, "{input}");
        }
    }

    #[test]
    fn validate_staged_name_table() {
        // § 8.2, the same table the handler test posts.
        let accepted = [
            "epel-release-9-12.el9.noarch.rpm",
            "epel-release-latest-9.noarch.rpm",
            "google-chrome-stable_current_x86_64.rpm",
            "libstdc++-devel-11-1.el9.x86_64.rpm",
            "foo-1.0~rc1-1.fc40.x86_64.rpm",
            "foo-1.2^20240101git1-1.fc40.x86_64.rpm",
            "X-1-1.noarch.RPM",
            "a.rpm",                             // Review Focus 5: the 5-byte minimum
            &format!("{}.rpm", "a".repeat(196)), // Review Focus 5: exactly 200 bytes
        ];
        for n in accepted {
            assert_eq!(validate_staged_name(n), Ok(()), "{n}");
        }

        // Rejected, paired with the specific NameError each one must produce:
        // `NameError` has five variants, and a test that only checks `is_err()`
        // cannot tell "rejects everything" from "rejects for the right reason".
        let rejected: &[(&str, NameError)] = &[
            (
                "epel-release-9$(id)-1.noarch.rpm",
                NameError::BadByte { at: 14 },
            ),
            (
                "epel-release-9-1\nRUN id.noarch.rpm",
                NameError::BadByte { at: 16 },
            ),
            (
                "epel-release-9-1\r.noarch.rpm",
                NameError::BadByte { at: 16 },
            ),
            ("epel-release 9-1.noarch.rpm", NameError::BadByte { at: 12 }),
            (
                "epel-release-9-1.noarch (1).rpm",
                NameError::BadByte { at: 23 },
            ),
            ("../x.rpm", NameError::BadFirstByte),
            ("..\\x.rpm", NameError::BadFirstByte),
            ("/etc/x.rpm", NameError::BadFirstByte),
            (".epel-release-9-1.noarch.rpm", NameError::BadFirstByte),
            ("-rf.rpm", NameError::BadFirstByte),
            ("foo.txt", NameError::NotRpm),
            ("", NameError::Empty),
            (".rpm", NameError::BadFirstByte),
            // Not in the brief's table: `char::is_alphanumeric` admits Unicode
            // ('π' is alphabetic), so these only fail if the check is byte-wise
            // (`u8::is_ascii_alphanumeric`) as the brief requires. Without a
            // case like this, that requirement has no test backing it.
            ("π-1.rpm", NameError::BadFirstByte),
            ("epel-π-9-1.noarch.rpm", NameError::BadByte { at: 5 }),
            // "a€bc" is 6 bytes ('€' is U+20AC, 3 bytes: 0xE2 0x82 0xAC), so
            // `strip_rpm_suffix`'s split point (len - 4 = 2) lands on the
            // second byte of '€' -- not a UTF-8 char boundary. `str::get`
            // must return `None` there instead of panicking the way a raw
            // `&s[2..]` slice would. This also happens to be the right
            // answer on its own terms: the string does not end in `.rpm`.
            ("a\u{20AC}bc", NameError::NotRpm),
            // Checked against the real function, not assumed: 'π' is caught
            // by the general byte scan at its own byte offset (1) before the
            // space is ever reached, because any non-ASCII byte -- not just
            // the ones `is_allowed_name_byte` is built to reject -- fails
            // that check immediately. Byte offset and char count coincide
            // here (nothing before 'π' is multi-byte), so this string alone
            // cannot distinguish a byte-indexed scan from a char-indexed one;
            // that distinction is already pinned by the two 'π' cases above,
            // since a char-indexed scan that still correctly rejects the
            // first non-ASCII byte it meets reports the same number a
            // byte-indexed one does. Kept as a second, differently-shaped
            // probe of the same general-scan path.
            ("aπb c.rpm", NameError::BadByte { at: 1 }),
        ];
        for (n, want) in rejected {
            assert_eq!(validate_staged_name(n), Err(*want), "{n}");
        }

        // 201 bytes: over MAX_UPLOAD_FILENAME_LEN.
        let too_long = format!("{}.rpm", "a".repeat(197));
        assert_eq!(validate_staged_name(&too_long), Err(NameError::TooLong));

        // "../x.rpm" is rejected, never stripped to "x.rpm".
        assert_eq!(
            validate_staged_name("../x.rpm"),
            Err(NameError::BadFirstByte)
        );
    }
}
