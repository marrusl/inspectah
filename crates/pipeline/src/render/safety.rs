//! Shell and HTML safety functions for renderer output.

use regex::Regex;
use std::ops::Range;
use std::sync::LazyLock;

use super::comment::LINE_TERMINATORS;

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

pub use inspectah_core::types::destination::has_line_break_or_control;

/// [`has_line_break_or_control`] with tab allowed, for a value whose
/// format separates fields with tabs as well as spaces (an `/etc/hosts`
/// line, a sysctl value read from `/proc/sys`).
pub fn has_line_break_or_non_tab_control(s: &str) -> bool {
    s.chars()
        .any(|c| c != '\t' && (c.is_control() || LINE_TERMINATORS.contains(&c)))
}

/// Why a path failed [`CopyToken::parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CopyTokenError {
    #[error("it is empty")]
    Empty,
    #[error("it holds a line break or control character")]
    LineBreakOrControl,
    #[error("it holds whitespace, which would split the COPY arguments")]
    Whitespace,
    #[error(
        "it holds `\\` or `$`, which the Containerfile parser treats as escape or variable syntax"
    )]
    ParserSyntax,
    #[error("it holds `*`, `?` or `[`, which a COPY source treats as a wildcard")]
    Wildcard,
}

/// A path the export may write as a bare argument of a Containerfile
/// `COPY` line.
///
/// `COPY` arguments are not shell words and have no quoting short of the
/// JSON form, so the value is proved instead: no line break or control
/// character, which would end the instruction and start one the snapshot
/// chose; no whitespace, which separates arguments, so `/opt/x /etc/`
/// would copy into `/etc`; and no `\` or `$`, which the Containerfile
/// parser reads as an escape (a trailing `\` continues the line) and as
/// variable expansion. Nor `*`, `?` or `[`: the builder expands those as
/// wildcards in a `COPY` source, so the line would copy every file the
/// pattern matches rather than the one the snapshot named.
///
/// [`CopyToken::parse`] is the only constructor, so a renderer that takes
/// a `&CopyToken` cannot be handed an unproved path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyToken(String);

impl CopyToken {
    /// Validate `value` and keep it.
    pub fn parse(value: &str) -> Result<Self, CopyTokenError> {
        if value.is_empty() {
            return Err(CopyTokenError::Empty);
        }
        if has_line_break_or_control(value) {
            return Err(CopyTokenError::LineBreakOrControl);
        }
        if value.contains(char::is_whitespace) {
            return Err(CopyTokenError::Whitespace);
        }
        if value.contains(['\\', '$']) {
            return Err(CopyTokenError::ParserSyntax);
        }
        if value.contains(['*', '?', '[']) {
            return Err(CopyTokenError::Wildcard);
        }
        Ok(Self(value.to_string()))
    }

    /// The validated path.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CopyToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why `value` cannot stand as an operand of a generated command, if it
/// cannot: a package, module, group, unit or SELinux name that a `RUN`
/// line passes to `dnf`, `systemctl`, `setsebool` or `semanage`.
///
/// This is separate from [`sanitize_shell_value`], which is a line and
/// shell-word guard and says nothing about where a word sits. In an
/// operand position a word that starts with `-` (`--nogpgcheck`) or `#`
/// could stand in for an option or a comment, so inspectah does not write
/// one there. A value after `=` or inside another word can start with
/// either legitimately, so the check belongs at the operand sites only.
pub fn operand_refusal(value: &str) -> Option<&'static str> {
    if value.starts_with('-') {
        Some("starts with `-`, which inspectah does not write as an operand")
    } else if value.starts_with('#') {
        Some("starts with `#`, which inspectah does not write as an operand")
    } else {
        None
    }
}

/// Why `value` cannot be written bare as an operand of a `RUN` command,
/// if it cannot: [`sanitize_shell_value`] refuses it, or it is in an
/// operand position and [`operand_refusal`] refuses it there.
pub fn shell_operand_refusal(value: &str) -> Option<&'static str> {
    if sanitize_shell_value(value).is_none() {
        Some("holds a character that is unsafe in a shell word")
    } else {
        operand_refusal(value)
    }
}

/// Whether [`shell_operand_refusal`] accepts `value`.
pub fn is_shell_operand(value: &str) -> bool {
    shell_operand_refusal(value).is_none()
}

/// `s` as one single-quoted POSIX shell word: every `'` becomes `'\''`.
///
/// Inside single quotes the shell substitutes nothing and splits nothing,
/// so this is safe for any value that holds no line break. It is not safe
/// for one that does: the shell keeps the newline, but a Containerfile or
/// kickstart parser reading the file line by line ends the line there.
/// Refuse those with [`has_line_break_or_control`] first.
pub fn single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// A byte a POSIX shell reads as itself in an unquoted word, in any
/// position: `[A-Za-z0-9@%+=:,./_-]`. It holds no metacharacter, no glob
/// character, no quote, no `\`, no `$`, no `~` and no `#`.
fn is_bare_shell_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'@' | b'%' | b'+' | b'=' | b':' | b',' | b'.' | b'/' | b'_' | b'-'
        )
}

/// `s` as one POSIX shell word, quoted only when it needs it: bare when it
/// is non-empty and every byte is in `[A-Za-z0-9@%+=:,./_-]`, otherwise
/// [`single_quote`]d. Generated commands should read as a person would
/// write them, and quoting a word that does not need it suggests it does
/// (Mark Russell, 2026-11-18).
///
/// Total over any input: the decision is an allowlist, not a list of
/// characters known to be dangerous, so a value set that later widens
/// still renders correctly. A leading `-` is not handled here: put `--`
/// before the operands. The line-break caveat of [`single_quote`] applies.
pub fn shell_word(s: &str) -> String {
    if !s.is_empty() && s.bytes().all(is_bare_shell_byte) {
        s.to_owned()
    } else {
        single_quote(s)
    }
}

/// Returns true if the given string is a valid tuned profile name.
pub fn is_valid_tuned_profile(name: &str) -> bool {
    TUNED_PROFILE_RE.is_match(name)
}

pub use inspectah_core::types::destination::MAX_EXPORT_PATH_LEN;

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

/// Longest non-path field the export will write into a build
/// instruction. A mode is four bytes and Linux caps an account name at
/// 32, so this is generous by two orders of magnitude and exists only to
/// bound the string.
pub const MAX_EXPORT_TOKEN_LEN: usize = 256;

/// A byte allowed in a non-path field: `[A-Za-z0-9._-]`.
///
/// The POSIX portable filename set, which is what an account name and a
/// mode are both drawn from. It excludes whitespace, which separates
/// fields on a tmpfiles.d line, and the newline, which separates
/// records; `#`, which starts a tmpfiles.d comment, and `%`, which
/// starts a specifier; `\`, which tmpfiles.d consumes as C-style
/// escaping; and every shell metacharacter, because these fields also
/// reach Containerfile comments that a newline could promote to an
/// instruction.
fn is_allowed_token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')
}

/// Why a field failed [`ExportToken::parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TokenError {
    #[error("exported field is empty")]
    Empty,
    #[error("exported field exceeds the maximum exported field length")]
    TooLong,
    #[error("exported field has a disallowed byte at position {at}")]
    BadByte { at: usize },
}

/// A non-path field the export may write into a generated build
/// instruction: a mode, an account name, a group name.
///
/// The sibling of [`ExportPath`], and it exists for the same reason one
/// step on. Proving the path of
/// `d {path} {mode} {owner} {group} -` left four fields on that line
/// unproved, and `VarDirectory`'s `mode`, `owner_name` and `group_name`
/// are unrestricted `Option<String>` that snapshot loading does not
/// check. The strongest of the three is not even a crafted-tarball
/// vector: `discover_var_directories` parses `stat -c "%04a %u %g %U %G"`
/// with `splitn(5, ' ')`, so the last field took the whole remainder of
/// the output, newlines included.
///
/// Accepts `value` when, and only when: it is not empty; every byte
/// passes [`is_allowed_token_byte`]; and it is no more than
/// [`MAX_EXPORT_TOKEN_LEN`] bytes.
///
/// What a refusal costs differs by field, and the caller decides. An
/// account name degrades to the numeric id the renderer already holds,
/// which provisions the same ownership. A mode has no safe stand-in that
/// is still the host's mode, so a refused mode refuses the whole line
/// rather than quietly substituting a default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportToken(String);

impl ExportToken {
    /// Validate `value` against the allowlist above and keep it.
    pub fn parse(value: &str) -> Result<Self, TokenError> {
        if value.is_empty() {
            return Err(TokenError::Empty);
        }
        if value.len() > MAX_EXPORT_TOKEN_LEN {
            return Err(TokenError::TooLong);
        }
        for (i, b) in value.bytes().enumerate() {
            if !is_allowed_token_byte(b) {
                return Err(TokenError::BadByte { at: i });
            }
        }
        Ok(Self(value.to_string()))
    }

    /// The validated field.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why a value failed [`BareWord::parse_name`] or [`BareWord::parse_url`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BareWordError {
    #[error(transparent)]
    Token(#[from] TokenError),
    #[error("starts with `-`, which inspectah does not write as an operand")]
    LeadingDash,
    #[error("does not start with a URL scheme letter")]
    NoScheme,
    #[error("does not start with a letter or digit")]
    BadStart,
}

/// A byte allowed in a [`BareWord`] URL: `[A-Za-z0-9._~:/?&=+,@-]`.
///
/// It leaves out whitespace, which separates `Exec*=` arguments; the line
/// terminators, which end the directive; `%`, which starts a systemd
/// specifier; `$`, which systemd expands as a variable; `\`, `'` and `"`,
/// which systemd reads as escaping and quoting; and `;`.
fn is_allowed_url_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'.' | b'_' | b'~' | b':' | b'/' | b'?' | b'&' | b'=' | b'+' | b',' | b'@' | b'-'
        )
}

/// One argument the export may write bare, unquoted, into a generated
/// command or directive line, such as a systemd `ExecStart=` line: a
/// flatpak remote name, application id, branch, or remote URL.
///
/// The value is proved rather than quoted for systemd's command-line
/// syntax, so no claim rests on how systemd unquotes. A name is an
/// [`ExportToken`] (`[A-Za-z0-9._-]`) that does not start with `-`; a URL
/// passes [`is_allowed_url_byte`] and starts with a letter, as a scheme
/// does. Neither can hold a line break, so neither can add a directive to
/// a unit that runs as root at boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BareWord(String);

impl BareWord {
    /// Validate a remote name, application id or branch.
    pub fn parse_name(value: &str) -> Result<Self, BareWordError> {
        ExportToken::parse(value)?;
        if value.starts_with('-') {
            return Err(BareWordError::LeadingDash);
        }
        Ok(Self(value.to_string()))
    }

    /// Validate a remote URL.
    pub fn parse_url(value: &str) -> Result<Self, BareWordError> {
        if value.is_empty() {
            return Err(TokenError::Empty.into());
        }
        if value.len() > MAX_EXPORT_PATH_LEN {
            return Err(TokenError::TooLong.into());
        }
        if let Some(at) = value.bytes().position(|b| !is_allowed_url_byte(b)) {
            return Err(TokenError::BadByte { at }.into());
        }
        if !value.as_bytes()[0].is_ascii_alphabetic() {
            return Err(BareWordError::NoScheme);
        }
        Ok(Self(value.to_string()))
    }
}

/// A byte allowed in a [`BareWord`] image reference:
/// `[A-Za-z0-9._:/@-]`, which covers registry, port, repository path, tag
/// and digest, and nothing a Containerfile parser treats as syntax.
fn is_allowed_image_ref_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'/' | b'@' | b'-')
}

impl BareWord {
    /// Validate a container image reference for a `FROM` line: no
    /// whitespace (which would add `AS <name>` or another argument), no
    /// line break (which would add an instruction), no `$` (which the
    /// parser expands), and it starts with a letter or digit.
    pub fn parse_image_ref(value: &str) -> Result<Self, BareWordError> {
        if value.is_empty() {
            return Err(TokenError::Empty.into());
        }
        if value.len() > MAX_EXPORT_PATH_LEN {
            return Err(TokenError::TooLong.into());
        }
        if let Some(at) = value.bytes().position(|b| !is_allowed_image_ref_byte(b)) {
            return Err(TokenError::BadByte { at }.into());
        }
        if !value.as_bytes()[0].is_ascii_alphanumeric() {
            return Err(BareWordError::BadStart);
        }
        Ok(Self(value.to_string()))
    }
}

/// Why a package group name cannot sit inside the double quotes of a
/// `dnf group install` line, if it cannot. Group names hold spaces
/// legitimately, so the name is quoted, and only what ends or acts inside
/// double quotes is refused: `"`, `\`, `$` and the backquote, every line
/// terminator, which would end the instruction, and every other control.
pub fn double_quoted_word_refusal(value: &str) -> Option<&'static str> {
    if value.is_empty() {
        Some("is empty")
    } else if has_line_break_or_control(value) {
        Some("holds a line break or control character")
    } else if value.contains(['"', '\\', '$', '`']) {
        Some("holds a quote, backslash, `$` or backquote")
    } else {
        None
    }
}

impl std::fmt::Display for BareWord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a name failed [`AccountName::parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AccountNameError {
    #[error(transparent)]
    Token(#[from] TokenError),
    #[error("account name starts with `-`")]
    LeadingDash,
    #[error("account name is `.` or `..`")]
    Dots,
    #[error("account name is all digits")]
    Numeric,
}

/// A user or group name the export may write into a generated build
/// instruction (`useradd`, `groupadd`, `chpasswd`, `install -o`).
///
/// [`ExportToken`]'s byte set, which is shadow-utils' portable name set,
/// with the refusals shadow-utils makes on top of it: a leading `-`;
/// `.` and `..`; and an all-digit name. A trailing `$` (Samba machine
/// accounts) is refused too, because it is not in the token set: such a
/// user gets a FIXME rather than a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountName(ExportToken);

impl AccountName {
    /// Validate `value` as an account or group name and keep it.
    pub fn parse(value: &str) -> Result<Self, AccountNameError> {
        let token = ExportToken::parse(value)?;
        if value.starts_with('-') {
            return Err(AccountNameError::LeadingDash);
        }
        if value == "." || value == ".." {
            return Err(AccountNameError::Dots);
        }
        if value.bytes().all(|b| b.is_ascii_digit()) {
            return Err(AccountNameError::Numeric);
        }
        Ok(Self(token))
    }

    /// The validated name.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Shortest legacy (DES) crypt(3) hash; bigcrypt is longer.
const MIN_LEGACY_CRYPT_LEN: usize = 13;

/// A byte allowed in a crypt(3) hash: the crypt base-64 alphabet
/// `[./0-9A-Za-z]`, the `$` that separates a modular hash's fields, and
/// `=`, `,`, `+` and `-`, which parameter fields use (`rounds=5000`,
/// `m=65536,t=2,p=1`).
///
/// It excludes whitespace and the newline, which end a `chpasswd` record;
/// the `:` that separates its fields; and every quote and shell
/// metacharacter, so a hash that passes is safe inside single quotes.
fn is_allowed_crypt_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'/' | b'$' | b'=' | b',' | b'+' | b'-')
}

/// Why a hash failed [`CryptHash::parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CryptHashError {
    #[error("password hash is empty")]
    Empty,
    #[error("password hash exceeds the maximum exported field length")]
    TooLong,
    #[error("password hash has a disallowed byte at position {at}")]
    BadByte { at: usize },
    #[error("password hash is not a modular `$id$...` or legacy crypt string")]
    Format,
}

/// A password hash as `/etc/shadow` stores it, which the export may
/// write into `chpasswd -e` input.
///
/// Accepts a modular hash, `$id$` followed by at least one more field
/// (`$6$salt$hash`, `$y$j9T$salt$hash`, `$2b$10$...`), or a legacy DES or
/// bigcrypt string of [`MIN_LEGACY_CRYPT_LEN`] or more crypt base-64
/// characters. Every byte passes [`is_allowed_crypt_byte`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CryptHash(String);

impl CryptHash {
    /// Validate `value` as a crypt(3) hash and keep it.
    pub fn parse(value: &str) -> Result<Self, CryptHashError> {
        if value.is_empty() {
            return Err(CryptHashError::Empty);
        }
        if value.len() > MAX_EXPORT_TOKEN_LEN {
            return Err(CryptHashError::TooLong);
        }
        if let Some(at) = value.bytes().position(|b| !is_allowed_crypt_byte(b)) {
            return Err(CryptHashError::BadByte { at });
        }
        let well_formed = match value.strip_prefix('$') {
            Some(rest) => {
                let mut fields = rest.split('$');
                fields.next().is_some_and(|id| !id.is_empty()) && fields.next().is_some()
            }
            None => {
                value.len() >= MIN_LEGACY_CRYPT_LEN
                    && value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'/')
            }
        };
        if !well_formed {
            return Err(CryptHashError::Format);
        }
        Ok(Self(value.to_string()))
    }

    /// The validated hash.
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

/// Kernel parameter names the boot loader, installer or base image owns.
/// An argument with one of these names is never carried, whatever its
/// value or quoting.
const KARGS_BOOTLOADER_NAMES: &[&str] = &[
    "ro",
    "rhgb",
    "quiet",
    "splash",
    "nosplash",
    "noplymouth",
    "root",
    "rd.lvm.lv",
    "rd.luks.uuid",
    "resume",
    "BOOT_IMAGE",
    "initrd",
    "LANG",
    "console",
    "crashkernel",
];

/// One argument of a kernel command line, read the way the kernel's
/// parameter parser reads it (`next_arg` in the kernel's
/// `lib/cmdline.c`).
///
/// An argument ends at whitespace outside double quotes. A double quote
/// opening the whole argument (`"name=a b"`) and one opening the value
/// (`name="a b"`) are both quoting: neither is part of the name or the
/// value, and one closing quote at the argument's end is dropped with
/// them. The name runs to the first `=`; the value is everything after
/// it. Every span points into the command line the argument was parsed
/// from, so a caller can replace exactly the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelArg<'a> {
    cmdline: &'a str,
    raw: Range<usize>,
    name: Range<usize>,
    value: Option<Range<usize>>,
}

impl<'a> KernelArg<'a> {
    /// The argument as written, quotes included.
    pub fn raw(&self) -> &'a str {
        &self.cmdline[self.raw.clone()]
    }

    /// Where [`Self::raw`] sits in the command line.
    pub fn raw_span(&self) -> Range<usize> {
        self.raw.clone()
    }

    /// The parameter name the kernel reads.
    pub fn name(&self) -> &'a str {
        &self.cmdline[self.name.clone()]
    }

    /// Where [`Self::name`] sits in the command line.
    pub fn name_span(&self) -> Range<usize> {
        self.name.clone()
    }

    /// The value the kernel reads, or `None` for an argument with no `=`.
    pub fn value(&self) -> Option<&'a str> {
        self.value.clone().map(|v| &self.cmdline[v])
    }

    /// Where [`Self::value`] sits in the command line.
    pub fn value_span(&self) -> Option<Range<usize>> {
        self.value.clone()
    }
}

/// The ASCII whitespace the kernel's `isspace` counts, which ends an
/// unquoted argument. Unlike `u8::is_ascii_whitespace` it includes the
/// vertical tab. The kernel also counts one non-ASCII byte
/// ([`KERNEL_LATIN1_SPACE`]), which [`operator_kargs`] refuses instead.
fn is_kernel_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

/// The kernel's parameter name comparison (`parameq`): `-` and `_` are
/// the same character.
pub(crate) fn kernel_name_eq(a: &str, b: &str) -> bool {
    let norm = |c: u8| if c == b'-' { b'_' } else { c };
    a.len() == b.len() && a.bytes().zip(b.bytes()).all(|(x, y)| norm(x) == norm(y))
}

/// Splits a kernel command line into its arguments the way the kernel's
/// `next_arg` does. Quotes and whitespace are ASCII, so every span is on
/// a character boundary.
pub fn kernel_args(cmdline: &str) -> Vec<KernelArg<'_>> {
    let bytes = cmdline.as_bytes();
    let mut args = Vec::new();
    let mut pos = 0;
    loop {
        while pos < bytes.len() && is_kernel_space(bytes[pos]) {
            pos += 1;
        }
        if pos == bytes.len() {
            return args;
        }
        let start = pos;
        let quoted = bytes[pos] == b'"';
        let body = if quoted { pos + 1 } else { pos };
        let mut in_quote = quoted;
        // The kernel takes the first `=` past the argument's first
        // character, quoted or not.
        let mut equals = None;
        let mut end = body;
        while end < bytes.len() && (in_quote || !is_kernel_space(bytes[end])) {
            if equals.is_none() && end != body && bytes[end] == b'=' {
                equals = Some(end);
            }
            if bytes[end] == b'"' {
                in_quote = !in_quote;
            }
            end += 1;
        }
        let closing_quote = end > body && bytes[end - 1] == b'"';
        let (name, value) = match equals {
            None => {
                let name_end = if quoted && closing_quote {
                    end - 1
                } else {
                    end
                };
                (body..name_end, None)
            }
            Some(eq) => {
                let mut value_start = eq + 1;
                let value_quoted = value_start < end && bytes[value_start] == b'"';
                if value_quoted {
                    value_start += 1;
                }
                let value_end = if (value_quoted || quoted) && closing_quote {
                    (end - 1).max(value_start)
                } else {
                    end
                };
                (body..eq, Some(value_start..value_end))
            }
        };
        args.push(KernelArg {
            cmdline,
            raw: start..end,
            name,
            value,
        });
        pos = end;
    }
}

/// Returns true if `arg` names a parameter the boot loader, installer or
/// base image owns. The parsed name decides, so quoting cannot hide one.
pub fn is_bootloader_karg(arg: &KernelArg<'_>) -> bool {
    KARGS_BOOTLOADER_NAMES
        .iter()
        .any(|name| kernel_name_eq(arg.name(), name))
}

/// The byte the kernel's `isspace` also counts as whitespace (Latin-1
/// no-break space). In UTF-8 it is a continuation byte (`à` is `C3 A0`),
/// so the kernel splits an argument holding one inside a character, where
/// [`kernel_args`] cannot.
const KERNEL_LATIN1_SPACE: u8 = 0xa0;

/// Extracts operator-defined kargs from a cmdline string, filtering out
/// bootloader-managed ones, any holding a line break or other control
/// character, and any the kernel would split where [`kernel_args`] does
/// not ([`KERNEL_LATIN1_SPACE`]). The arguments go into a TOML string
/// array, which escapes quotes and backslashes itself, so no shell
/// character is refused.
pub fn operator_kargs(cmdline: &str) -> Vec<KernelArg<'_>> {
    kernel_args(cmdline)
        .into_iter()
        .filter(|arg| !is_bootloader_karg(arg))
        .filter(|arg| !has_line_break_or_control(arg.raw()))
        .filter(|arg| !arg.raw().as_bytes().contains(&KERNEL_LATIN1_SPACE))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A word made only of the bare set stays bare; anything else, the
    /// empty word included, is single-quoted. A leading `-` is not the
    /// quoting's job: the caller puts `--` first.
    #[test]
    fn shell_word_quotes_only_what_needs_it() {
        let cases: &[(&str, &str)] = &[
            ("", "''"),
            ("httpd.service", "httpd.service"),
            ("getty@tty2.service", "getty@tty2.service"),
            ("a%b", "a%b"),
            ("a+b", "a+b"),
            ("a=b", "a=b"),
            ("a:b", "a:b"),
            ("a,b", "a,b"),
            ("a.b", "a.b"),
            ("a/b", "a/b"),
            ("a_b", "a_b"),
            ("a-b", "a-b"),
            ("-.slice", "-.slice"),
            (r"backup\x2djob.service", r"'backup\x2djob.service'"),
            ("a b", "'a b'"),
            ("a*", "'a*'"),
            ("a?", "'a?'"),
            ("a[b]", "'a[b]'"),
            ("it's", r"'it'\''s'"),
            ("$HOME", "'$HOME'"),
            ("`id`", "'`id`'"),
        ];
        for (word, expected) in cases {
            assert_eq!(shell_word(word), *expected, "{word:?}");
        }
    }

    #[test]
    fn test_sanitize_shell_value_safe() {
        assert_eq!(sanitize_shell_value("normal-pkg"), Some("normal-pkg"));
        assert_eq!(sanitize_shell_value("httpd"), Some("httpd"));
        assert_eq!(sanitize_shell_value("vim-enhanced"), Some("vim-enhanced"));
    }

    /// Every Containerfile site guarded by `sanitize_shell_value` relies on
    /// it refusing each line terminator (its `\s` is Unicode-aware), so a
    /// guarded value cannot end the instruction.
    #[test]
    fn sanitize_shell_value_refuses_every_line_terminator() {
        for t in LINE_TERMINATORS {
            let value = format!("a{t}RUN id");
            assert_eq!(sanitize_shell_value(&value), None, "{t:?}");
        }
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
    fn operand_refusal_table() {
        for ok in [
            "httpd",
            "vim-enhanced",
            "nginx:1.24",
            "a-b",
            "a#b",
            "x86_64",
        ] {
            assert_eq!(operand_refusal(ok), None, "{ok}");
        }
        for refused in ["--nogpgcheck", "-oalice", "-", "#foo", "#"] {
            assert!(operand_refusal(refused).is_some(), "{refused}");
        }
    }

    #[test]
    fn copy_token_refuses_wildcards() {
        for (value, want) in [
            ("opt/*", CopyTokenError::Wildcard),
            ("opt/a?", CopyTokenError::Wildcard),
            ("opt/[ab]", CopyTokenError::Wildcard),
            ("opt/a b", CopyTokenError::Whitespace),
            ("opt/$x", CopyTokenError::ParserSyntax),
        ] {
            assert_eq!(CopyToken::parse(value), Err(want), "{value}");
        }
        assert!(CopyToken::parse("opt/app-1.2/bin").is_ok());
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
    fn export_token_accepts_real_fields() {
        for value in [
            "0755",
            "1777",
            "-",
            "root",
            "appone",
            "1001",
            "systemd-resolve",
            "nfsnobody",
            "my.user_1",
        ] {
            assert!(ExportToken::parse(value).is_ok(), "{value}");
        }
    }

    #[test]
    fn export_token_refuses_table() {
        let cases: &[(&str, TokenError)] = &[
            ("", TokenError::Empty),
            ("appone appone", TokenError::BadByte { at: 6 }),
            (
                "appone -\nd /etc/shadow 0777 root root",
                TokenError::BadByte { at: 6 },
            ),
            ("0755\n", TokenError::BadByte { at: 4 }),
            ("root#", TokenError::BadByte { at: 4 }),
            ("root%t", TokenError::BadByte { at: 4 }),
            ("root\\n", TokenError::BadByte { at: 4 }),
            ("root$(id)", TokenError::BadByte { at: 4 }),
            ("root/etc", TokenError::BadByte { at: 4 }),
        ];
        for (value, want) in cases {
            assert_eq!(ExportToken::parse(value), Err(*want), "{value}");
        }
        let too_long = "a".repeat(MAX_EXPORT_TOKEN_LEN + 1);
        assert_eq!(ExportToken::parse(&too_long), Err(TokenError::TooLong));
    }

    #[test]
    fn account_name_table() {
        for ok in ["alice", "john.doe", "svc_app-1", "_apt", "Admin"] {
            assert!(AccountName::parse(ok).is_ok(), "{ok}");
        }
        let refused: &[(&str, AccountNameError)] = &[
            ("-oalice", AccountNameError::LeadingDash),
            (".", AccountNameError::Dots),
            ("..", AccountNameError::Dots),
            ("1000", AccountNameError::Numeric),
            (
                "mall*ry",
                AccountNameError::Token(TokenError::BadByte { at: 4 }),
            ),
            (
                "host$",
                AccountNameError::Token(TokenError::BadByte { at: 4 }),
            ),
            ("", AccountNameError::Token(TokenError::Empty)),
        ];
        for (value, want) in refused {
            assert_eq!(AccountName::parse(value), Err(*want), "{value}");
        }
    }

    #[test]
    fn crypt_hash_table() {
        for ok in [
            "$6$rounds=65536$salt$hash",
            "$y$j9T$abcdefghijklmnop$ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789./abcd",
            "$2b$10$abcdefghijklmnopqrstuuABCDEFGHIJKLMNOPQRSTUVWXYZ01234",
            "$argon2id$v=19$m=65536,t=2,p=1$c2FsdA$aGFzaA",
            "abJnggxhB/yWI",
        ] {
            assert!(CryptHash::parse(ok).is_ok(), "{ok}");
        }
        let refused: &[(&str, CryptHashError)] = &[
            ("", CryptHashError::Empty),
            (
                "$6$salt$hash'; id; echo '",
                CryptHashError::BadByte { at: 12 },
            ),
            ("$6$salt$hash\nRUN id", CryptHashError::BadByte { at: 12 }),
            ("$6$salt$ha:sh", CryptHashError::BadByte { at: 10 }),
            ("$6$ salt", CryptHashError::BadByte { at: 3 }),
            ("$6", CryptHashError::Format),
            ("$$salt", CryptHashError::Format),
            ("short", CryptHashError::Format),
            ("legacy-but-not=crypt", CryptHashError::Format),
        ];
        for (value, want) in refused {
            assert_eq!(CryptHash::parse(value), Err(*want), "{value}");
        }
    }

    /// The raw text of each operator argument on `cmdline`.
    fn operator_raws(cmdline: &str) -> Vec<&str> {
        operator_kargs(cmdline).iter().map(KernelArg::raw).collect()
    }

    #[test]
    fn test_operator_kargs() {
        assert_eq!(
            operator_raws("quiet crashkernel=auto nosmt=force"),
            vec!["nosmt=force"]
        );
    }

    #[test]
    fn test_operator_kargs_filters_bootloader() {
        let result = operator_raws("ro rhgb quiet root=/dev/sda1 BOOT_IMAGE=/vmlinuz nosmt=force");
        assert_eq!(result, vec!["nosmt=force"]);
    }

    /// A quoted value with spaces is one argument, as the kernel reads it;
    /// characters a shell would treat specially are kept, and an argument
    /// with a control character, or a byte the kernel splits on inside a
    /// UTF-8 character, is refused.
    #[test]
    fn operator_kargs_split_like_the_kernel() {
        let cases: &[(&str, &[&str])] = &[
            ("", &[]),
            ("  quiet  ", &[]),
            ("a=1\tb=2", &["a=1", "b=2"]),
            ("a=1\u{b}b=2", &["a=1", "b=2"]),
            (
                "quiet dyndbg=\"file x.c +p\" nosmt",
                &["dyndbg=\"file x.c +p\"", "nosmt"],
            ),
            ("\"a b\"=c d", &["\"a b\"=c", "d"]),
            ("x=$y;z back\\slash", &["x=$y;z", "back\\slash"]),
            ("unclosed=\"a b", &["unclosed=\"a b"]),
            ("bell=\u{7} ok=1", &["ok=1"]),
            ("x\u{e0}root=UUID=old ok=1", &["ok=1"]),
        ];
        for (cmdline, want) in cases {
            assert_eq!(operator_raws(cmdline), *want, "{cmdline:?}");
        }
    }

    /// Each argument's raw text, name and value, as the kernel's
    /// `next_arg` reads them.
    #[test]
    fn kernel_args_read_name_and_value_like_next_arg() {
        type Case<'a> = (&'a str, &'a [(&'a str, &'a str, Option<&'a str>)]);
        let cases: &[Case] = &[
            ("nosmt", &[("nosmt", "nosmt", None)]),
            ("a=b=c", &[("a=b=c", "a", Some("b=c"))]),
            ("a=", &[("a=", "a", Some(""))]),
            ("=a=b", &[("=a=b", "=a", Some("b"))]),
            ("\"quiet\"", &[("\"quiet\"", "quiet", None)]),
            ("\"quiet", &[("\"quiet", "quiet", None)]),
            ("\"", &[("\"", "", None)]),
            (
                "ro \"root=UUID=old-host\" nosmt=force",
                &[
                    ("ro", "ro", None),
                    ("\"root=UUID=old-host\"", "root", Some("UUID=old-host")),
                    ("nosmt=force", "nosmt", Some("force")),
                ],
            ),
            (
                "\"rd.iscsi.password=alpha bravo\"",
                &[(
                    "\"rd.iscsi.password=alpha bravo\"",
                    "rd.iscsi.password",
                    Some("alpha bravo"),
                )],
            ),
            (
                "rd.iscsi.password=\"alpha bravo\"",
                &[(
                    "rd.iscsi.password=\"alpha bravo\"",
                    "rd.iscsi.password",
                    Some("alpha bravo"),
                )],
            ),
            ("x=\"", &[("x=\"", "x", Some(""))]),
            ("\"x=\"", &[("\"x=\"", "x", Some(""))]),
            ("\"a=\"b\"\"", &[("\"a=\"b\"\"", "a", Some("b\""))]),
            ("\"a b\"=c", &[("\"a b\"=c", "a b\"", Some("c"))]),
            (
                "x=\"a b\"c d",
                &[("x=\"a b\"c", "x", Some("a b\"c")), ("d", "d", None)],
            ),
        ];
        for (cmdline, want) in cases {
            let got: Vec<(&str, &str, Option<&str>)> = kernel_args(cmdline)
                .iter()
                .map(|a| (a.raw(), a.name(), a.value()))
                .collect();
            assert_eq!(got, *want, "{cmdline:?}");
        }
    }

    /// A boot loader parameter is dropped by its parsed name, however it
    /// is quoted, and `-` matches `_` as the kernel compares names.
    #[test]
    fn bootloader_kargs_drop_by_parsed_name() {
        for cmdline in [
            "root=UUID=old-host",
            "\"root=UUID=old-host\"",
            "root=\"UUID=old-host\"",
            "\"rd.lvm.lv=rhel/root\"",
            "rd.lvm.lv=\"rhel/root\"",
            "\"rd.luks.uuid=luks-1\"",
            "\"crashkernel=1G-4G:192M\"",
            "\"BOOT-IMAGE=/vmlinuz\"",
            "\"quiet\"",
            "root",
        ] {
            assert_eq!(operator_raws(cmdline), Vec::<&str>::new(), "{cmdline:?}");
        }
    }
}
