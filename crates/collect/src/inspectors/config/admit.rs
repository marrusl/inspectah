//! Where a candidate config path physically is, and whether its content
//! may be read: the checks every branch of the config inspector makes
//! before it opens a file under `/etc`.
//!
//! A path under `/etc` can name a file somewhere else in two ways, and
//! every exclusion the inspector has (credentials, machine identity, the
//! account databases, files another section owns) matches a path by how
//! it is spelled.
//!
//! - **A directory above it is a symlink.** With `/etc/app/state -> /etc`,
//!   `/etc/app/state/chrony.keys` opens `/etc/chrony.keys`, because the
//!   kernel follows every ancestor link, `lstat` included, while no rule
//!   matches the requested spelling. [`locate`] refuses such a path
//!   outright. Measured on RHEL 9.8 and 10.2 (2026-11-21, after a drift
//!   run): no packaged path lies under a directory link in `/etc`, and
//!   every directory link there points either out of `/etc`, which the
//!   walk already skips, or at a directory the walk reaches under its own
//!   name (`/etc/ssl/certs`, `/etc/yum/vars`), so refusing loses no file
//!   and only drops a second name for one.
//! - **The path itself is a symlink.** A read returns its target's
//!   content. [`readable`] reads through a link only to a regular file
//!   under `/etc` that the section would carry at its own path.
//!
//! Both also refuse what must never be opened: a FIFO, socket or device,
//! directly or at the end of a link chain, since opening a FIFO with no
//! writer blocks and the subprocess timeout does not cover file reads.
//!
//! What remains is the race between these checks and the read: a file
//! swapped after it was checked is read as whatever it became. Closing it
//! needs the executor to open without blocking and check through the
//! descriptor.

use super::ghost::{GhostClass, classify_ghost};
use inspectah_core::traits::executor::{Executor, FileKind};
use std::path::Path;

/// The directory the config inspector covers, with its trailing slash.
const ETC_PREFIX: &str = "/etc/";

/// Where a masking link points, and the one link target outside `/etc`
/// read as content: it reads as nothing.
const DEV_NULL: &str = "/dev/null";

/// Where `authselect create-profile` puts a custom profile.
const AUTHSELECT_CUSTOM_PREFIX: &str = "/etc/authselect/custom/";

/// Where authselect's shipped profiles live, which a custom profile's
/// links point into.
const AUTHSELECT_SHIPPED_PREFIX: &str = "/usr/share/authselect/";

/// Why [`locate`] refused a candidate path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Refusal {
    /// Not an absolute path under `/etc` once `.` and repeated separators
    /// are removed, or it holds a `..` component.
    NotUnderEtc,
    /// A directory above the file is a symlink: the path is another name
    /// for a file elsewhere.
    AliasedAncestor,
    /// The directory holding the file could not be resolved: a link in it
    /// is dangling or loops, or a component is unreadable.
    UnresolvedAncestor,
}

/// `path` with `.` components and repeated separators removed, when it
/// is an absolute path under `/etc` with no `..` component.
pub(super) fn normalize_etc_path(path: &str) -> Option<String> {
    if !path.starts_with('/') {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            name => parts.push(name),
        }
    }
    let normalized = format!("/{}", parts.join("/"));
    (normalized.starts_with(ETC_PREFIX)).then_some(normalized)
}

/// The candidate `path` as the inspector checks and classifies it: its
/// normalized spelling, when every directory above it is a real
/// directory. A path the caller reports keeps the spelling it was found
/// under; this one is what the exclusions match.
pub(super) fn locate(exec: &dyn Executor, path: &str) -> Result<String, Refusal> {
    let normalized = normalize_etc_path(path).ok_or(Refusal::NotUnderEtc)?;
    let (parent, _) = normalized
        .rsplit_once('/')
        .expect("a normalized path under /etc has a separator");
    let resolved = exec
        .resolve_final_target(Path::new(parent))
        .map_err(|_| Refusal::UnresolvedAncestor)?;
    if resolved == Path::new(parent) {
        Ok(normalized)
    } else {
        Err(Refusal::AliasedAncestor)
    }
}

/// What reading a located path returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Readable {
    /// A regular file at the path itself.
    File,
    /// A link whose chain ends at `/dev/null`: a mask, whose content is
    /// empty and is never opened.
    DevNullMask,
    /// A link whose chain ends at this regular file.
    ThroughLink(String),
}

/// Why a located path's content is not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Unreadable {
    /// A FIFO, socket or device, directly or at the end of its links.
    Special,
    /// Absent, a directory, or a link that does not resolve.
    NotAFile,
    /// A link that resolves outside `/etc`, to this path.
    LinkLeavesEtc(String),
    /// A link to a file under `/etc` the section never carries at its own
    /// path ([`classify_ghost`] is not `Config` there): key material,
    /// machine identity, another section's file, or a generated file.
    LinkToWithheld(String),
}

/// Whether, and through what, the content of `path` may be read. Every
/// content capture asks this first ([`super::capture_config`]).
///
/// A link leaving `/etc` is read in one case: a file of a custom
/// authselect profile linked into the shipped profiles, which the image
/// needs as a file (`is_authselect_profile_link` in `mod.rs`).
pub(super) fn readable(exec: &dyn Executor, path: &str) -> Result<Readable, Unreadable> {
    let on_host = Path::new(path);
    match exec.file_kind(on_host) {
        Ok(FileKind::Regular) => Ok(Readable::File),
        Ok(FileKind::Special) => Err(Unreadable::Special),
        Ok(FileKind::Directory) | Err(_) => Err(Unreadable::NotAFile),
        Ok(FileKind::Symlink) => {
            let target = exec
                .resolve_final_target(on_host)
                .map_err(|_| Unreadable::NotAFile)?;
            if target == Path::new(DEV_NULL) {
                return Ok(Readable::DevNullMask);
            }
            match exec.file_kind(&target) {
                Ok(FileKind::Regular) => {}
                Ok(FileKind::Special) => return Err(Unreadable::Special),
                _ => return Err(Unreadable::NotAFile),
            }
            let target = target.to_string_lossy().into_owned();
            if !target.starts_with(ETC_PREFIX) {
                return if is_authselect_profile_link(path, &target) {
                    Ok(Readable::ThroughLink(target))
                } else {
                    Err(Unreadable::LinkLeavesEtc(target))
                };
            }
            if classify_ghost(&target) == GhostClass::Config {
                Ok(Readable::ThroughLink(target))
            } else {
                Err(Unreadable::LinkToWithheld(target))
            }
        }
    }
}

/// Whether opening `path` cannot block: a regular file, or a link whose
/// chain ends at one or at `/dev/null`. For the reads that only decide
/// something (a DHCP connection, the crypto policy, a unit file), whose
/// content is not kept.
pub(crate) fn opens_without_blocking(exec: &dyn Executor, path: &str) -> bool {
    let on_host = Path::new(path);
    match exec.file_kind(on_host) {
        Ok(FileKind::Regular) => true,
        Ok(FileKind::Symlink) => exec.resolve_final_target(on_host).is_ok_and(|target| {
            target == Path::new(DEV_NULL)
                || matches!(exec.file_kind(&target), Ok(FileKind::Regular))
        }),
        _ => false,
    }
}

/// A file of a custom authselect profile that is a link into the shipped
/// profiles, as `authselect create-profile <name> -b <base>
/// --symlink-meta --symlink-pam` makes them (`README`, `REQUIREMENTS`
/// and the PAM templates link to `/usr/share/authselect/vendor/<base>/`,
/// measured on RHEL 9.8 and 10.2).
///
/// The walk would make such a link a cross-tree advisory and carry
/// nothing, but the export selects the profile, and `authselect select`
/// refuses a profile with no `README` ("Unable to find profile"), and
/// writes an empty PAM stack for a missing template: both measured in
/// rhel-bootc 9.8 and 10.2. So the link is collected as a file holding
/// what it resolves to, and the image's profile is complete. A build from
/// such a context selected the profile and passed `authselect check` on
/// both releases. The profile's files are then copies of the host's
/// shipped templates; they no longer follow the image's own.
pub(super) fn is_authselect_profile_link(path: &str, resolved: &str) -> bool {
    path.strip_prefix(AUTHSELECT_CUSTOM_PREFIX)
        .and_then(|rest| rest.split_once('/'))
        .is_some_and(|(profile, file)| {
            !profile.is_empty() && !file.is_empty() && !file.contains('/')
        })
        && resolved.starts_with(AUTHSELECT_SHIPPED_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;

    #[test]
    fn normalizes_paths_under_etc_only() {
        for (path, normalized) in [
            ("/etc/sssd/sssd.conf", Some("/etc/sssd/sssd.conf")),
            ("/etc//sssd/./sssd.conf", Some("/etc/sssd/sssd.conf")),
            ("//etc/x/", Some("/etc/x")),
            ("/etc/a/../shadow", None),
            ("/etc/..", None),
            ("/etc", None),
            ("/etc/", None),
            ("/etcetera/x", None),
            ("etc/x", None),
            ("/var/lib/x", None),
        ] {
            assert_eq!(normalize_etc_path(path).as_deref(), normalized, "{path}");
        }
    }

    /// A path whose directory is a link is refused, whatever the link
    /// points at; a final link is left to [`readable`].
    #[test]
    fn locate_refuses_an_aliased_ancestor() {
        let exec = MockExecutor::new()
            .with_link("/etc/app/state", "/etc")
            .with_link("/etc/app/data", "/var/lib/app")
            .with_link("/etc/repo-alias", "yum.repos.d")
            .with_dangling_link("/etc/gone", "/nowhere")
            .with_link("/etc/final-link", "/etc/shadow");
        for (path, located) in [
            ("/etc/sssd/sssd.conf", Ok("/etc/sssd/sssd.conf".to_string())),
            (
                "/etc/./sssd//sssd.conf",
                Ok("/etc/sssd/sssd.conf".to_string()),
            ),
            ("/etc/final-link", Ok("/etc/final-link".to_string())),
            ("/etc/app/state/chrony.keys", Err(Refusal::AliasedAncestor)),
            ("/etc/app/state/sub/x", Err(Refusal::AliasedAncestor)),
            ("/etc/app/data/token", Err(Refusal::AliasedAncestor)),
            ("/etc/repo-alias/redhat.repo", Err(Refusal::AliasedAncestor)),
            ("/etc/gone/x", Err(Refusal::UnresolvedAncestor)),
            ("/etc/../root/x", Err(Refusal::NotUnderEtc)),
        ] {
            assert_eq!(locate(&exec, path), located, "{path}");
        }
    }

    #[test]
    fn readable_follows_a_link_only_to_carried_config_under_etc() {
        let exec = MockExecutor::new()
            .with_file("/etc/app.conf", "a=1\n")
            .with_file("/etc/shadow", "root:x:\n")
            .with_file("/etc/chrony.keys", "1 SHA1 HEX:00\n")
            .with_file("/etc/ssh/ssh_host_ed25519_key", "key\n")
            .with_file("/root/secret", "s\n")
            .with_file("/usr/share/authselect/vendor/sssd/README", "r\n")
            .with_special_file("/etc/app.fifo")
            .with_dir("/etc/dir", vec![])
            .with_link("/etc/to-app", "app.conf")
            .with_link("/etc/to-chain", "to-app")
            .with_link("/etc/to-shadow", "/etc/shadow")
            .with_link("/etc/to-keys", "chrony.keys")
            .with_link("/etc/to-hostkey", "ssh/ssh_host_ed25519_key")
            .with_link("/etc/to-root", "/root/secret")
            .with_link("/etc/to-fifo", "app.fifo")
            .with_link("/etc/to-dir", "dir")
            .with_link("/etc/masked", "/dev/null")
            .with_dangling_link("/etc/broken", "/nowhere")
            .with_link(
                "/etc/authselect/custom/corp/README",
                "/usr/share/authselect/vendor/sssd/README",
            );
        let through = |t: &str| Ok(Readable::ThroughLink(t.into()));
        for (path, expected) in [
            ("/etc/app.conf", Ok(Readable::File)),
            ("/etc/to-app", through("/etc/app.conf")),
            ("/etc/to-chain", through("/etc/app.conf")),
            ("/etc/masked", Ok(Readable::DevNullMask)),
            (
                "/etc/authselect/custom/corp/README",
                through("/usr/share/authselect/vendor/sssd/README"),
            ),
            ("/etc/app.fifo", Err(Unreadable::Special)),
            ("/etc/to-fifo", Err(Unreadable::Special)),
            ("/etc/dir", Err(Unreadable::NotAFile)),
            ("/etc/to-dir", Err(Unreadable::NotAFile)),
            ("/etc/broken", Err(Unreadable::NotAFile)),
            ("/etc/absent", Err(Unreadable::NotAFile)),
            (
                "/etc/to-root",
                Err(Unreadable::LinkLeavesEtc("/root/secret".into())),
            ),
            (
                "/etc/to-shadow",
                Err(Unreadable::LinkToWithheld("/etc/shadow".into())),
            ),
            (
                "/etc/to-keys",
                Err(Unreadable::LinkToWithheld("/etc/chrony.keys".into())),
            ),
            (
                "/etc/to-hostkey",
                Err(Unreadable::LinkToWithheld(
                    "/etc/ssh/ssh_host_ed25519_key".into(),
                )),
            ),
        ] {
            assert_eq!(readable(&exec, path), expected, "{path}");
        }
        for (path, opens) in [
            ("/etc/app.conf", true),
            ("/etc/to-shadow", true),
            ("/etc/masked", true),
            ("/etc/app.fifo", false),
            ("/etc/to-fifo", false),
            ("/etc/dir", false),
            ("/etc/absent", false),
        ] {
            assert_eq!(opens_without_blocking(&exec, path), opens, "{path}");
        }
    }

    #[test]
    fn authselect_profile_links() {
        for (path, resolved, carried) in [
            (
                "/etc/authselect/custom/corp/README",
                "/usr/share/authselect/vendor/sssd/README",
                true,
            ),
            (
                "/etc/authselect/custom/corp/system-auth",
                "/usr/share/authselect/default/sssd/system-auth",
                true,
            ),
            (
                "/etc/authselect/custom/corp/README",
                "/usr/share/doc/README",
                false,
            ),
            (
                "/etc/authselect/custom/README",
                "/usr/share/authselect/vendor/sssd/README",
                false,
            ),
            (
                "/etc/authselect/custom/corp/sub/README",
                "/usr/share/authselect/vendor/sssd/README",
                false,
            ),
            (
                "/etc/pam.d/system-auth",
                "/usr/share/authselect/vendor/sssd/system-auth",
                false,
            ),
        ] {
            assert_eq!(
                is_authselect_profile_link(path, resolved),
                carried,
                "{path} -> {resolved}"
            );
        }
    }
}
