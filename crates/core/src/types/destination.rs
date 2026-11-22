//! Where a file lands in the image, and the path rules every writer of
//! one shares. Moved here from the pipeline's config tree so the repo
//! ownership pass in core and the stager compare paths the same way.

/// Characters that some reader of a rendered file treats as ending a line:
/// the Unicode mandatory line breaks (UAX #14 classes BK, CR, LF and NL).
pub const LINE_TERMINATORS: &[char] = &[
    '\n', '\r', '\u{0B}', '\u{0C}', '\u{85}', '\u{2028}', '\u{2029}',
];

/// Longest path the export will write into a build instruction. Linux's
/// own `PATH_MAX`, so no path a real filesystem can hold is refused for
/// length alone.
pub const MAX_EXPORT_PATH_LEN: usize = 4096;

/// Whether `s` holds a character that ends a line for some reader (every
/// character in [`LINE_TERMINATORS`]) or any other control character,
/// tab included.
///
/// No value the export writes into an instruction, directive or config
/// key line may hold one: a terminator would start a new line, and the
/// line it starts is an instruction the snapshot chose.
pub fn has_line_break_or_control(s: &str) -> bool {
    s.chars()
        .any(|c| c.is_control() || LINE_TERMINATORS.contains(&c))
}

/// The longest path component a Linux filesystem holds (`NAME_MAX`), in
/// bytes. A path's whole length is held to [`MAX_EXPORT_PATH_LEN`],
/// Linux's `PATH_MAX`, the export's existing path-length policy.
const MAX_PATH_COMPONENT_LEN: usize = 255;

/// Where a staged file lands in the image: a normalized absolute path.
///
/// The one constructor, [`Destination::new`], drops empty and `.`
/// components, so `/etc/systemd/./system//x.timer`, a trailing separator
/// and a path given without its leading `/` are all one destination. It
/// refuses `..`, a NUL byte and a control character, a path that names no
/// file, and a path or component longer than a Linux filesystem holds
/// (checked first, so a pathological imported path costs no more than
/// one pass). The reading is lexical: the staging directory is created
/// empty by the export and only the stager writes into it, so no symlink
/// sits on a path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Destination(String);

impl Destination {
    /// The normalized form of `path`, or the rule it breaks.
    pub fn new(path: &str) -> Result<Self, PathError> {
        if path.len() > MAX_EXPORT_PATH_LEN {
            return Err(PathError::TooLong);
        }
        if path.contains('\0') {
            return Err(PathError::NulByte);
        }
        if has_line_break_or_control(path) {
            return Err(PathError::LineBreakOrControl);
        }
        let mut normal = String::with_capacity(path.len() + 1);
        for component in path.split('/') {
            match component {
                "" | "." => {}
                ".." => return Err(PathError::Traversal),
                name if name.len() > MAX_PATH_COMPONENT_LEN => {
                    return Err(PathError::ComponentTooLong);
                }
                name => {
                    normal.push('/');
                    normal.push_str(name);
                }
            }
        }
        if normal.is_empty() {
            return Err(PathError::NoFileName);
        }
        Ok(Self(normal))
    }

    /// The absolute path, `/` first.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The path below `/`, for joining under a staging directory.
    pub fn relative(&self) -> &str {
        &self.0[1..]
    }

    /// The directory this path is in, `None` for a top-level name. A
    /// prefix of a normal path ending before a separator is normal too.
    pub fn parent(&self) -> Option<Destination> {
        let (dir, _) = self.0.rsplit_once('/')?;
        (!dir.is_empty()).then(|| Destination(dir.to_owned()))
    }
}

impl std::fmt::Display for Destination {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Path safety errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    #[error("path contains NUL byte")]
    NulByte,
    #[error("path contains a line break or control character")]
    LineBreakOrControl,
    #[error("absolute path not allowed")]
    AbsolutePath,
    #[error("path traversal detected")]
    Traversal,
    #[error("symlink escapes root")]
    SymlinkEscape,
    #[error("path names no file")]
    NoFileName,
    #[error("path is longer than {MAX_EXPORT_PATH_LEN} bytes")]
    TooLong,
    #[error("a path component is longer than {MAX_PATH_COMPONENT_LEN} bytes")]
    ComponentTooLong,
}

impl PathError {
    /// The variant's name, as refine's view names the rule a refused
    /// offer broke. The one place it is spelled.
    pub fn name(self) -> &'static str {
        match self {
            Self::NulByte => "NulByte",
            Self::LineBreakOrControl => "LineBreakOrControl",
            Self::AbsolutePath => "AbsolutePath",
            Self::Traversal => "Traversal",
            Self::SymlinkEscape => "SymlinkEscape",
            Self::NoFileName => "NoFileName",
            Self::TooLong => "TooLong",
            Self::ComponentTooLong => "ComponentTooLong",
        }
    }
}
