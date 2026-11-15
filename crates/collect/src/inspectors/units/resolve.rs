//! The unit file systemd would load for a name, read from the two unit
//! directories this collector reads (§ 7.2 of the units spec, steps 2 and
//! 3).
//!
//! The answer is a [`Resolved`] with every owning package unknown: the
//! owner lookup fills it in afterwards, before the unit is classified.

use super::install::{DropInRoot, drop_in_dirs};
use inspectah_core::traits::executor::Executor;
use inspectah_core::types::units::{
    UnitFragment, UnitName, holds_install_section, systemd_reads_drop_in,
};
use std::io;
use std::path::Path;

/// The administrator's unit directory.
pub(super) const ETC_UNIT_DIR: &str = "/etc/systemd/system";

/// The directory packages install unit files into.
pub(super) const VENDOR_UNIT_DIR: &str = "/usr/lib/systemd/system";

/// The directories on systemd's unit search path besides the two this
/// collector reads, in the order `systemd.unit(5)` gives them, each with
/// the highest of the two it outranks: [`DropInRoot::Etc`] for the four
/// above `/etc/systemd/system`, which outrank both, and
/// [`DropInRoot::Vendor`] for the five between it and
/// `/usr/lib/systemd/system`. `/run/systemd/generator.late` comes after
/// the vendor directory, outranks nothing read here, and is not listed.
pub(super) const UNREAD_SEARCH_PATH: [(&str, DropInRoot); 9] = [
    ("/etc/systemd/system.control", DropInRoot::Etc),
    ("/run/systemd/system.control", DropInRoot::Etc),
    ("/run/systemd/transient", DropInRoot::Etc),
    ("/run/systemd/generator.early", DropInRoot::Etc),
    ("/etc/systemd/system.attached", DropInRoot::Vendor),
    ("/run/systemd/system", DropInRoot::Vendor),
    ("/run/systemd/system.attached", DropInRoot::Vendor),
    ("/run/systemd/generator", DropInRoot::Vendor),
    ("/usr/local/lib/systemd/system", DropInRoot::Vendor),
];

/// A link to this path masks the unit.
const DEV_NULL: &str = "/dev/null";

/// What systemd has for a name on the source host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Resolved {
    /// A unit file loads.
    File { fragment: UnitFragment },
    /// A zero-length regular file, or a link that ends at `/dev/null` or at
    /// a zero-length file, is at the unit's path. `hidden` is the unit file
    /// under the mask: `Vendor` or `Absent`.
    Masked { hidden: UnitFragment },
    /// A link that ends at another unit's non-empty file in one of the two
    /// unit directories.
    AliasOf(UnitName),
    /// No file in the two directories this collector reads.
    NotFound,
    /// A file on the search path that this collector does not read
    /// changes what loads for the name or what `enable` does (§ 7.1).
    Unsupported { path: String },
}

/// A name the collector cannot resolve to a unit it models.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ResolveError {
    /// A link that ends at a non-empty file outside both unit directories,
    /// or at nothing: the unit is `linked`, which is excluded.
    Linked { path: String, target: String },
    /// A path that exists and could not be read.
    Read { path: String, error: String },
}

/// What sits at one path.
pub(super) enum Entry {
    Missing,
    Link(String),
    /// A regular file, and whether it is exactly zero bytes long.
    File {
        empty: bool,
    },
}

/// Resolve `name`: its own file under `/etc`, then under the vendor
/// directory, then, for an instance, its template's in the same order. A
/// link is tested before anything else, and a regular file is a mask only
/// when it is exactly zero bytes long. A file on the rest of the search
/// path that changes the answer makes the name [`Resolved::Unsupported`]
/// ([`search_path_change`]).
pub(super) fn resolve(exec: &dyn Executor, name: &UnitName) -> Result<Resolved, ResolveError> {
    let (resolved, chosen) = effective_file(exec, name)?;
    match search_path_change(exec, name, chosen.as_ref())? {
        Some(path) => Ok(Resolved::Unsupported { path }),
        None => Ok(resolved),
    }
}

/// The file the collector chose for a name: the unit's own or its
/// template's, and the directory it sits in.
struct Chosen {
    file: UnitName,
    root: DropInRoot,
}

/// [`resolve`] over the two directories this collector reads, with the
/// file it chose; `None` when there is none.
fn effective_file(
    exec: &dyn Executor,
    name: &UnitName,
) -> Result<(Resolved, Option<Chosen>), ResolveError> {
    for file in candidates(name) {
        let etc = format!("{ETC_UNIT_DIR}/{}", file.as_str());
        let chosen = Chosen {
            file: file.clone(),
            root: DropInRoot::Etc,
        };
        match entry(exec, &etc)? {
            Entry::Missing => {}
            Entry::Link(target) => {
                return Ok((link_form(exec, &etc, target, &file, true)?, Some(chosen)));
            }
            Entry::File { empty: true } => {
                let hidden = hidden_fragment(exec, &file)?;
                return Ok((Resolved::Masked { hidden }, Some(chosen)));
            }
            Entry::File { empty: false } => {
                let vendor = format!("{VENDOR_UNIT_DIR}/{}", file.as_str());
                let fragment = match entry(exec, &vendor)? {
                    Entry::Missing => UnitFragment::Local { file },
                    Entry::Link(_) | Entry::File { .. } => UnitFragment::Shadowed {
                        file,
                        owning_package: None,
                    },
                };
                return Ok((Resolved::File { fragment }, Some(chosen)));
            }
        }

        let vendor = format!("{VENDOR_UNIT_DIR}/{}", file.as_str());
        let chosen = Chosen {
            file: file.clone(),
            root: DropInRoot::Vendor,
        };
        match entry(exec, &vendor)? {
            Entry::Missing => {}
            Entry::Link(target) => {
                return Ok((
                    link_form(exec, &vendor, target, &file, false)?,
                    Some(chosen),
                ));
            }
            // A zero-length vendor file is a mask the package ships, with
            // no file under it.
            Entry::File { empty: true } => {
                let hidden = UnitFragment::Absent;
                return Ok((Resolved::Masked { hidden }, Some(chosen)));
            }
            Entry::File { empty: false } => {
                let fragment = UnitFragment::Vendor {
                    file,
                    owning_package: None,
                };
                return Ok((Resolved::File { fragment }, Some(chosen)));
            }
        }
    }
    Ok((Resolved::NotFound, None))
}

/// The first path on [`UNREAD_SEARCH_PATH`] that changes what the
/// collector read for `name`, which chose `chosen` (§ 7.1 of the units
/// spec, "The search path"):
///
/// - a unit file systemd loads in place of the chosen one: the chosen
///   file's name in a directory that outranks it, and, when the template
///   was chosen for an instance, the instance's own file in any directory
///   on the list, since a concrete instance file loads over its template
///   wherever each sits. With nothing chosen, any file for the name or its
///   template;
/// - a `.conf` drop-in in one of [`drop_in_dirs`] in any directory on the
///   list that holds an `[Install]` section or has the file name of a
///   vendor drop-in in those directories. Type-wide and dash-prefix
///   directories are not read for `[Install]`, so their drop-ins change
///   nothing here.
///
/// A drop-in whose text cannot be decoded is counted as changing the
/// answer, since its `[Install]` cannot be ruled out.
fn search_path_change(
    exec: &dyn Executor,
    name: &UnitName,
    chosen: Option<&Chosen>,
) -> Result<Option<String>, ResolveError> {
    for (dir, rank) in UNREAD_SEARCH_PATH {
        let files = match chosen {
            None => candidates(name),
            Some(chosen) => {
                let mut files = Vec::new();
                if rank <= chosen.root {
                    files.push(chosen.file.clone());
                }
                if chosen.file != *name {
                    files.push(name.clone());
                }
                files
            }
        };
        for file in files {
            let path = format!("{dir}/{}", file.as_str());
            if !matches!(entry(exec, &path)?, Entry::Missing) {
                return Ok(Some(path));
            }
        }
    }

    let dirs = drop_in_dirs(name);
    let mut vendor_names: Option<Vec<String>> = None;
    for (dir, _) in UNREAD_SEARCH_PATH {
        for drop_in_dir in &dirs {
            for file in conf_files(exec, &format!("{dir}/{drop_in_dir}"))? {
                let path = format!("{dir}/{drop_in_dir}/{file}");
                let vendor_names = match &vendor_names {
                    Some(names) => names,
                    None => vendor_names.insert(vendor_drop_in_names(exec, &dirs)?),
                };
                if vendor_names.contains(&file) {
                    return Ok(Some(path));
                }
                match exec.read_file(Path::new(&path)) {
                    Ok(text) if !holds_install_section(&text) => {}
                    Ok(_) => return Ok(Some(path)),
                    Err(e) if e.kind() == io::ErrorKind::InvalidData => return Ok(Some(path)),
                    Err(e) => {
                        return Err(ResolveError::Read {
                            path,
                            error: e.to_string(),
                        });
                    }
                }
            }
        }
    }
    Ok(None)
}

/// The `.conf` file names in the directory `path` that systemd reads
/// ([`systemd_reads_drop_in`]: a hidden name is skipped); none when it
/// does not exist.
pub(super) fn conf_files(exec: &dyn Executor, path: &str) -> Result<Vec<String>, ResolveError> {
    match exec.read_dir(Path::new(path)) {
        Ok(entries) => Ok(entries
            .into_iter()
            .filter(|entry| systemd_reads_drop_in(entry))
            .collect()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(ResolveError::Read {
            path: path.to_owned(),
            error: e.to_string(),
        }),
    }
}

/// The `.conf` file names a package ships in `dirs` under the vendor
/// directory.
fn vendor_drop_in_names(exec: &dyn Executor, dirs: &[String]) -> Result<Vec<String>, ResolveError> {
    let mut names = Vec::new();
    for dir in dirs {
        names.extend(conf_files(exec, &format!("{VENDOR_UNIT_DIR}/{dir}"))?);
    }
    Ok(names)
}

/// The names whose file can load for `name`: its own, then its template's
/// when it is an instance.
fn candidates(name: &UnitName) -> Vec<UnitName> {
    let mut names = vec![name.clone()];
    if let Some(parts) = name.instance() {
        names.push(parts.template());
    }
    names
}

/// The vendor file a mask on `file` hides: `file` itself, or for an
/// instance its template, under the vendor directory; `Absent` when there
/// is none.
fn hidden_fragment(exec: &dyn Executor, file: &UnitName) -> Result<UnitFragment, ResolveError> {
    for candidate in candidates(file) {
        let vendor = format!("{VENDOR_UNIT_DIR}/{}", candidate.as_str());
        if !matches!(entry(exec, &vendor)?, Entry::Missing) {
            return Ok(UnitFragment::Vendor {
                file: candidate,
                owning_package: None,
            });
        }
    }
    Ok(UnitFragment::Absent)
}

/// What a link at `path` makes the unit, from [`link_end`]: a mask, an
/// alias of another unit, or linked.
fn link_form(
    exec: &dyn Executor,
    path: &str,
    target: String,
    file: &UnitName,
    under_etc: bool,
) -> Result<Resolved, ResolveError> {
    match link_end(exec, path)? {
        LinkEnd::Mask => {
            let hidden = if under_etc {
                hidden_fragment(exec, file)?
            } else {
                UnitFragment::Absent
            };
            Ok(Resolved::Masked { hidden })
        }
        LinkEnd::UnitFile(other) if other != *file => Ok(Resolved::AliasOf(other)),
        LinkEnd::UnitFile(_) | LinkEnd::Elsewhere => Err(ResolveError::Linked {
            path: path.to_owned(),
            target,
        }),
    }
}

/// Where the whole chain of the link at `path` ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LinkEnd {
    /// `/dev/null` or a zero-length regular file, wherever it is: a mask.
    Mask,
    /// A file with content directly in one of the two unit directories,
    /// whose name is this unit name.
    UnitFile(UnitName),
    /// Anything else: a file elsewhere or with a name that is not a unit
    /// name, or a chain that does not resolve (dangling, through a path
    /// that is not a directory, or a loop).
    Elsewhere,
}

/// Follow the link at `path` to the end of its chain and decide its form
/// before anything else, as systemd's `unit_file_load_or_readlink` does: a
/// link that ends at `/dev/null` or at a zero-length regular file is a
/// mask, wherever that file is, so a link to a link to `/dev/null` is a
/// mask and so is a link to an empty unit file. Only a link that ends at a
/// file with content can name a unit. The resolver and the link reader
/// both read a link through this, so they agree on which unit it names.
///
/// A chain that does not resolve ends [`LinkEnd::Elsewhere`]. One the
/// collector could not follow, because a permission or I/O failure stopped
/// it on the way, is a [`ResolveError::Read`] naming the link at `path`:
/// where it ends is not known, so nothing may be read from it.
pub(super) fn link_end(exec: &dyn Executor, path: &str) -> Result<LinkEnd, ResolveError> {
    let resolved = match exec.resolve_final_target(Path::new(path)) {
        Ok(resolved) => resolved,
        Err(e) if chain_does_not_resolve(&e) => return Ok(LinkEnd::Elsewhere),
        Err(e) => {
            return Err(ResolveError::Read {
                path: path.to_owned(),
                error: e.to_string(),
            });
        }
    };
    // A target that cannot be read is a read failure, as systemd fails on a
    // target it cannot stat: either form could hide a mask.
    let masked = resolved == Path::new(DEV_NULL)
        || matches!(
            entry(exec, &resolved.to_string_lossy())?,
            Entry::File { empty: true }
        );
    if masked {
        return Ok(LinkEnd::Mask);
    }
    let in_unit_dir = resolved
        .parent()
        .is_some_and(|dir| dir == Path::new(ETC_UNIT_DIR) || dir == Path::new(VENDOR_UNIT_DIR));
    let unit = resolved
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| UnitName::try_from(n.to_owned()).ok());
    Ok(match unit {
        Some(unit) if in_unit_dir => LinkEnd::UnitFile(unit),
        _ => LinkEnd::Elsewhere,
    })
}

/// Whether `error`, from following a chain of links, says the chain ends
/// at nothing: a missing path (dangling), a path that is not a directory
/// where the chain needs one, or a loop (`ELOOP`).
fn chain_does_not_resolve(error: &io::Error) -> bool {
    // `io::ErrorKind::FilesystemLoop` is unstable
    // (https://github.com/rust-lang/rust/issues/86442), so a loop is read
    // from the OS error number.
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    ) || error.raw_os_error() == Some(libc::ELOOP)
}

/// Read what sits at `path` without following a link.
pub(super) fn entry(exec: &dyn Executor, path: &str) -> Result<Entry, ResolveError> {
    let read_error = |error: io::Error| ResolveError::Read {
        path: path.to_owned(),
        error: error.to_string(),
    };
    match exec.read_link(Path::new(path)) {
        Ok(target) => Ok(Entry::Link(target)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Entry::Missing),
        // `EINVAL`: the path exists and is not a link.
        Err(e) if e.kind() == io::ErrorKind::InvalidInput => {
            match exec.read_file(Path::new(path)) {
                Ok(content) => Ok(Entry::File {
                    empty: content.is_empty(),
                }),
                // Only zero length matters here, and `InvalidData` proves the
                // file has bytes: the executor refuses a file over its size cap,
                // or content that is not UTF-8, with that kind. systemd loads a
                // unit whose comment or `Description=` holds Latin-1 bytes, so
                // such a file is a unit, not a mask and not a read failure.
                Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                    Ok(Entry::File { empty: false })
                }
                Err(e) => Err(read_error(e)),
            }
        }
        Err(e) => Err(read_error(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;

    fn name(n: &str) -> UnitName {
        UnitName::try_from(n.to_owned()).expect("test unit names are valid")
    }

    fn vendor(file: &str) -> UnitFragment {
        UnitFragment::Vendor {
            file: name(file),
            owning_package: None,
        }
    }

    const UNIT: &str = "[Unit]\nDescription=test\n";

    #[test]
    fn resolve_table() {
        let exec = MockExecutor::new()
            // Vendor only, /etc only, both.
            .with_file("/usr/lib/systemd/system/vendor.service", UNIT)
            .with_file("/etc/systemd/system/local.service", UNIT)
            .with_file("/usr/lib/systemd/system/both.service", UNIT)
            .with_file("/etc/systemd/system/both.service", UNIT)
            // Masks over a vendor unit: a link to /dev/null, a zero-length
            // file, and a link to a link to /dev/null.
            .with_file("/usr/lib/systemd/system/nulled.service", UNIT)
            .with_link("/etc/systemd/system/nulled.service", "/dev/null")
            .with_file("/usr/lib/systemd/system/empty.service", UNIT)
            .with_file("/etc/systemd/system/empty.service", "")
            .with_file("/usr/lib/systemd/system/hop.service", UNIT)
            .with_link("/etc/systemd/system/hop.service", "/etc/alternatives/hop")
            .with_link("/etc/alternatives/hop", "/dev/null")
            // A mask with no vendor file under it.
            .with_link("/etc/systemd/system/gone.service", "/dev/null")
            // A comment-only file over a vendor unit loads as a unit.
            .with_file("/usr/lib/systemd/system/comment.service", UNIT)
            .with_file("/etc/systemd/system/comment.service", "# nothing\n\n")
            // An alias to another unit's file.
            .with_file("/usr/lib/systemd/system/real.service", UNIT)
            .with_link(
                "/etc/systemd/system/alias.service",
                "/usr/lib/systemd/system/real.service",
            )
            // Instances: template fallback, and a concrete /etc file that
            // wins over a vendor template.
            .with_file("/usr/lib/systemd/system/getty@.service", UNIT)
            .with_file("/usr/lib/systemd/system/worker@.service", UNIT)
            .with_file("/etc/systemd/system/worker@site.service", UNIT);

        let cases: Vec<(&str, Resolved)> = vec![
            (
                "vendor.service",
                Resolved::File {
                    fragment: vendor("vendor.service"),
                },
            ),
            (
                "local.service",
                Resolved::File {
                    fragment: UnitFragment::Local {
                        file: name("local.service"),
                    },
                },
            ),
            (
                "both.service",
                Resolved::File {
                    fragment: UnitFragment::Shadowed {
                        file: name("both.service"),
                        owning_package: None,
                    },
                },
            ),
            (
                "nulled.service",
                Resolved::Masked {
                    hidden: vendor("nulled.service"),
                },
            ),
            (
                "empty.service",
                Resolved::Masked {
                    hidden: vendor("empty.service"),
                },
            ),
            (
                "hop.service",
                Resolved::Masked {
                    hidden: vendor("hop.service"),
                },
            ),
            (
                "gone.service",
                Resolved::Masked {
                    hidden: UnitFragment::Absent,
                },
            ),
            (
                "comment.service",
                Resolved::File {
                    fragment: UnitFragment::Shadowed {
                        file: name("comment.service"),
                        owning_package: None,
                    },
                },
            ),
            ("alias.service", Resolved::AliasOf(name("real.service"))),
            (
                "getty@tty1.service",
                Resolved::File {
                    fragment: vendor("getty@.service"),
                },
            ),
            (
                "worker@site.service",
                Resolved::File {
                    fragment: UnitFragment::Local {
                        file: name("worker@site.service"),
                    },
                },
            ),
            ("missing.service", Resolved::NotFound),
        ];
        for (unit, expected) in cases {
            assert_eq!(resolve(&exec, &name(unit)), Ok(expected), "{unit}");
        }
    }

    /// T24, the concrete instance and the search path for instances (the
    /// scan records no instance yet, so these are asserted here): a file of
    /// the instance's own name is the fragment over its template wherever
    /// each sits on the two directories read; an instance file anywhere on
    /// the search path refuses the instance when the template was chosen,
    /// and so does a template file that outranks the chosen template; a
    /// template file beside a chosen instance file changes nothing.
    #[test]
    fn the_effective_file_of_an_instance_and_the_search_path() {
        let vendor_template =
            || MockExecutor::new().with_file("/usr/lib/systemd/system/worker@.service", UNIT);
        let local_template =
            || MockExecutor::new().with_file("/etc/systemd/system/worker@.service", UNIT);
        let unsupported = |path: &str| Resolved::Unsupported { path: path.into() };
        let cases: Vec<(&str, MockExecutor, Resolved)> = vec![
            (
                "an /etc instance over a vendor template",
                vendor_template().with_file("/etc/systemd/system/worker@site.service", UNIT),
                Resolved::File {
                    fragment: UnitFragment::Local {
                        file: name("worker@site.service"),
                    },
                },
            ),
            (
                "a vendor instance over an /etc template",
                local_template().with_file("/usr/lib/systemd/system/worker@site.service", UNIT),
                Resolved::File {
                    fragment: vendor("worker@site.service"),
                },
            ),
            (
                "no instance file: the template",
                vendor_template(),
                Resolved::File {
                    fragment: vendor("worker@.service"),
                },
            ),
            (
                "an instance file under /run, the template chosen",
                vendor_template().with_file("/run/systemd/system/worker@site.service", UNIT),
                unsupported("/run/systemd/system/worker@site.service"),
            ),
            (
                "an instance file under /usr/local/lib, the /etc template chosen",
                local_template()
                    .with_file("/usr/local/lib/systemd/system/worker@site.service", UNIT),
                unsupported("/usr/local/lib/systemd/system/worker@site.service"),
            ),
            (
                "a template under /run outranks the chosen vendor template",
                vendor_template().with_file("/run/systemd/system/worker@.service", UNIT),
                unsupported("/run/systemd/system/worker@.service"),
            ),
            (
                "a template under /run does not outrank the chosen /etc template",
                local_template().with_file("/run/systemd/system/worker@.service", UNIT),
                Resolved::File {
                    fragment: UnitFragment::Local {
                        file: name("worker@.service"),
                    },
                },
            ),
            (
                "a template under /run beside a chosen vendor instance",
                MockExecutor::new()
                    .with_file("/usr/lib/systemd/system/worker@site.service", UNIT)
                    .with_file("/run/systemd/system/worker@.service", UNIT),
                Resolved::File {
                    fragment: vendor("worker@site.service"),
                },
            ),
            (
                "a drop-in with [Install] in the template's .d under /run",
                vendor_template()
                    .with_dir("/run/systemd/system/worker@.service.d", vec!["50-x.conf"])
                    .with_file(
                        "/run/systemd/system/worker@.service.d/50-x.conf",
                        "[Install]\nWantedBy=multi-user.target\n",
                    ),
                unsupported("/run/systemd/system/worker@.service.d/50-x.conf"),
            ),
            (
                "a drop-in with no [Install] in the template's .d under /run",
                vendor_template()
                    .with_dir("/run/systemd/system/worker@.service.d", vec!["50-x.conf"])
                    .with_file(
                        "/run/systemd/system/worker@.service.d/50-x.conf",
                        "[Service]\nNice=5\n",
                    ),
                Resolved::File {
                    fragment: vendor("worker@.service"),
                },
            ),
        ];
        for (label, exec, expected) in cases {
            assert_eq!(
                resolve(&exec, &name("worker@site.service")),
                Ok(expected),
                "{label}"
            );
        }
    }

    /// The constant holds the directories `systemd.unit(5)` lists besides
    /// the two this collector reads, in its order, and not
    /// `generator.late`.
    #[test]
    fn the_search_path_constant_is_systemds_order() {
        let dirs: Vec<&str> = UNREAD_SEARCH_PATH.iter().map(|(dir, _)| *dir).collect();
        assert_eq!(
            dirs,
            [
                "/etc/systemd/system.control",
                "/run/systemd/system.control",
                "/run/systemd/transient",
                "/run/systemd/generator.early",
                "/etc/systemd/system.attached",
                "/run/systemd/system",
                "/run/systemd/system.attached",
                "/run/systemd/generator",
                "/usr/local/lib/systemd/system",
            ]
        );
        let above_etc = UNREAD_SEARCH_PATH
            .iter()
            .take_while(|(_, rank)| *rank == DropInRoot::Etc)
            .count();
        assert_eq!(above_etc, 4, "the first four outrank /etc/systemd/system");
    }

    /// A link whose final target is a zero-length file is a mask, as one to
    /// `/dev/null` is: systemd tests the final target for null or empty
    /// before it calls a link an alias or linked. The hidden fragment is the
    /// original name's vendor file. Control: the same links ending at a
    /// non-empty file are an alias and linked.
    #[test]
    fn a_link_ending_at_an_empty_file_is_a_mask() {
        let links = |end: &str| {
            MockExecutor::new()
                .with_file("/usr/lib/systemd/system/a.socket", UNIT)
                .with_file("/usr/lib/systemd/system/c.socket", UNIT)
                .with_file("/usr/lib/systemd/system/o.socket", UNIT)
                .with_file("/etc/systemd/system/b.socket", end)
                .with_file("/opt/masks/off", end)
                .with_file("/usr/lib/systemd/system/e.socket", end)
                // Direct, inside the unit directories.
                .with_link("/etc/systemd/system/a.socket", "b.socket")
                // Chained through a link outside them.
                .with_link("/etc/systemd/system/c.socket", "/etc/alternatives/c")
                .with_link("/etc/alternatives/c", "/etc/systemd/system/b.socket")
                // Direct, to a file outside both directories.
                .with_link("/etc/systemd/system/o.socket", "/opt/masks/off")
                // A vendor link, with no file under it.
                .with_link("/usr/lib/systemd/system/v.socket", "e.socket")
        };

        let empty = links("");
        let masked = |unit: &str, hidden: UnitFragment| {
            assert_eq!(
                resolve(&empty, &name(unit)),
                Ok(Resolved::Masked { hidden }),
                "{unit}"
            );
        };
        masked("a.socket", vendor("a.socket"));
        masked("c.socket", vendor("c.socket"));
        masked("o.socket", vendor("o.socket"));
        masked("v.socket", UnitFragment::Absent);

        let full = links(UNIT);
        assert_eq!(
            resolve(&full, &name("a.socket")),
            Ok(Resolved::AliasOf(name("b.socket")))
        );
        assert_eq!(
            resolve(&full, &name("c.socket")),
            Ok(Resolved::AliasOf(name("b.socket")))
        );
        assert_eq!(
            resolve(&full, &name("o.socket")),
            Err(ResolveError::Linked {
                path: "/etc/systemd/system/o.socket".into(),
                target: "/opt/masks/off".into(),
            })
        );
        assert_eq!(
            resolve(&full, &name("v.socket")),
            Ok(Resolved::AliasOf(name("e.socket")))
        );
    }

    #[test]
    fn link_outside_the_unit_directories_is_linked() {
        let exec = MockExecutor::new()
            .with_file("/opt/app/app.service", UNIT)
            .with_link("/etc/systemd/system/app.service", "/opt/app/app.service")
            .with_dangling_link("/etc/systemd/system/broken.service", "/opt/gone.service");
        // Positive control: a link into the vendor directory is an alias.
        let alias = MockExecutor::new()
            .with_file("/usr/lib/systemd/system/real.service", UNIT)
            .with_link(
                "/etc/systemd/system/app.service",
                "/usr/lib/systemd/system/real.service",
            );
        assert_eq!(
            resolve(&alias, &name("app.service")),
            Ok(Resolved::AliasOf(name("real.service")))
        );

        assert_eq!(
            resolve(&exec, &name("app.service")),
            Err(ResolveError::Linked {
                path: "/etc/systemd/system/app.service".into(),
                target: "/opt/app/app.service".into(),
            })
        );
        assert_eq!(
            resolve(&exec, &name("broken.service")),
            Err(ResolveError::Linked {
                path: "/etc/systemd/system/broken.service".into(),
                target: "/opt/gone.service".into(),
            })
        );
    }

    /// A chain that does not resolve, dangling, through a path that is
    /// not a directory, or looping, ends nowhere: [`LinkEnd::Elsewhere`].
    /// A chain the collector could not follow, because a link on the way
    /// could not be read, is a read failure naming the link, never
    /// `Elsewhere`: what it ends at is not known. Control: the readable
    /// chain ends at the vendor unit.
    #[test]
    fn a_chain_that_cannot_be_followed_is_a_read_failure() {
        let public = format!("{ETC_UNIT_DIR}/public.service");
        let relay = "/opt/units/relay.service";
        let real = format!("{VENDOR_UNIT_DIR}/real.service");
        let chain = || {
            MockExecutor::new()
                .with_file(&real, UNIT)
                .with_link(&public, relay)
                .with_link(relay, &real)
        };
        let readable = chain();
        assert_eq!(
            link_end(&readable, &public),
            Ok(LinkEnd::UnitFile(name("real.service")))
        );

        let looping = MockExecutor::new()
            .with_link(&public, relay)
            .with_link(relay, &public);
        let ends_nowhere = [
            (
                "dangling",
                chain().with_dangling_link(relay, "/opt/units/gone.service"),
            ),
            (
                "through a file",
                chain().with_link_error(relay, io::ErrorKind::NotADirectory),
            ),
            ("a loop", looping),
        ];
        for (case, exec) in ends_nowhere {
            assert_eq!(link_end(&exec, &public), Ok(LinkEnd::Elsewhere), "{case}");
        }

        for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::Other] {
            let exec = chain().with_link_error(relay, kind);
            match link_end(&exec, &public) {
                Err(ResolveError::Read { path, error }) => {
                    assert_eq!(path, public, "{kind:?}");
                    assert!(error.contains(relay), "{kind:?}: {error}");
                }
                other => panic!("{kind:?}: expected a read failure, got {other:?}"),
            }
            assert!(
                matches!(
                    resolve(&exec, &name("public.service")),
                    Err(ResolveError::Read { path, .. }) if path == public
                ),
                "{kind:?}: the resolver reads the same failure"
            );
        }
    }

    #[test]
    fn unreadable_unit_file_is_a_read_error() {
        let exec = MockExecutor::new().with_file_error(
            "/etc/systemd/system/secret.service",
            io::ErrorKind::PermissionDenied,
        );
        // Positive control: the same path readable resolves.
        let readable = MockExecutor::new().with_file("/etc/systemd/system/secret.service", UNIT);
        assert!(matches!(
            resolve(&readable, &name("secret.service")),
            Ok(Resolved::File { .. })
        ));
        assert!(matches!(
            resolve(&exec, &name("secret.service")),
            Err(ResolveError::Read { path, .. }) if path == "/etc/systemd/system/secret.service"
        ));
    }
}
