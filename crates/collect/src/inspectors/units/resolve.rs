//! The unit file systemd would load for a name, read from the two unit
//! directories this collector reads (§ 7.2 of the units spec, steps 2 and
//! 3).
//!
//! The answer is a [`Resolved`] with every owning package unknown: the
//! owner lookup fills it in afterwards, before the unit is classified.

use inspectah_core::traits::executor::Executor;
use inspectah_core::types::units::{UnitFragment, UnitName};
use std::io;
use std::path::Path;

/// The administrator's unit directory.
pub(super) const ETC_UNIT_DIR: &str = "/etc/systemd/system";

/// The directory packages install unit files into.
pub(super) const VENDOR_UNIT_DIR: &str = "/usr/lib/systemd/system";

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
/// when it is exactly zero bytes long.
pub(super) fn resolve(exec: &dyn Executor, name: &UnitName) -> Result<Resolved, ResolveError> {
    for file in candidates(name) {
        let etc = format!("{ETC_UNIT_DIR}/{}", file.as_str());
        match entry(exec, &etc)? {
            Entry::Missing => {}
            Entry::Link(target) => return link_form(exec, &etc, target, &file, true),
            Entry::File { empty: true } => {
                return Ok(Resolved::Masked {
                    hidden: hidden_fragment(exec, &file)?,
                });
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
                return Ok(Resolved::File { fragment });
            }
        }

        let vendor = format!("{VENDOR_UNIT_DIR}/{}", file.as_str());
        match entry(exec, &vendor)? {
            Entry::Missing => {}
            Entry::Link(target) => return link_form(exec, &vendor, target, &file, false),
            // A zero-length vendor file is a mask the package ships, with
            // no file under it.
            Entry::File { empty: true } => {
                return Ok(Resolved::Masked {
                    hidden: UnitFragment::Absent,
                });
            }
            Entry::File { empty: false } => {
                return Ok(Resolved::File {
                    fragment: UnitFragment::Vendor {
                        file,
                        owning_package: None,
                    },
                });
            }
        }
    }
    Ok(Resolved::NotFound)
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

/// What a link at `path` makes the unit. The whole chain is followed, and
/// the final target's form is decided before anything else, as systemd's
/// `unit_file_load_or_readlink` does: a link that ends at `/dev/null` or at
/// a zero-length regular file is a mask, wherever that file is, so a link
/// to a link to `/dev/null` is a mask and so is a link to an empty unit
/// file. Only a link that ends at a file with content is an alias or
/// linked.
fn link_form(
    exec: &dyn Executor,
    path: &str,
    target: String,
    file: &UnitName,
    under_etc: bool,
) -> Result<Resolved, ResolveError> {
    let linked = || ResolveError::Linked {
        path: path.to_owned(),
        target: target.clone(),
    };
    let Ok(resolved) = exec.resolve_final_target(Path::new(path)) else {
        return Err(linked());
    };
    // A target that cannot be read is a read failure, as systemd fails on a
    // target it cannot stat: either form could hide a mask.
    let masked = resolved == Path::new(DEV_NULL)
        || matches!(
            entry(exec, &resolved.to_string_lossy())?,
            Entry::File { empty: true }
        );
    if masked {
        let hidden = if under_etc {
            hidden_fragment(exec, file)?
        } else {
            UnitFragment::Absent
        };
        return Ok(Resolved::Masked { hidden });
    }
    let in_unit_dir = resolved
        .parent()
        .is_some_and(|dir| dir == Path::new(ETC_UNIT_DIR) || dir == Path::new(VENDOR_UNIT_DIR));
    let other_unit = resolved
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| UnitName::try_from(n.to_owned()).ok())
        .filter(|other| other != file);
    match other_unit {
        Some(other) if in_unit_dir => Ok(Resolved::AliasOf(other)),
        _ => Err(linked()),
    }
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
