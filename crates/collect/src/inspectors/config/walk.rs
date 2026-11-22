use inspectah_core::traits::executor::Executor;
use inspectah_core::types::units::UNIT_TYPE_SUFFIXES;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Dev-artifact filtering
// ---------------------------------------------------------------------------

/// VCS directory names whose presence prunes the entire subtree.
const PRUNE_MARKERS: &[&str] = &[".git", ".svn", ".hg"];

/// Directory names always skipped during recursive walks.
const SKIP_DIR_NAMES: &[&str] = &[
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    ".tox",
    ".nox",
    "node_modules",
    ".eggs",
    ".vscode",
    ".idea",
    ".cursor",
];

/// Returns `true` if any path component is a dev/build directory.
pub fn is_dev_artifact(path: &str) -> bool {
    for part in path.split('/') {
        if part.is_empty() {
            continue;
        }
        if PRUNE_MARKERS.contains(&part) || SKIP_DIR_NAMES.contains(&part) {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// System-generated exclusion lists
// ---------------------------------------------------------------------------

/// Exact paths that are system-generated and should not appear as "unowned".
///
/// Files that identify one machine are not here: they are members of the
/// machine-state class (`inspectah_core::machine_state::RULES`), which the
/// collector records visibly with a provisioning notice. This list is
/// noise only.
pub(crate) const UNOWNED_EXCLUDE_EXACT: &[&str] = &[
    "/etc/adjtime",
    "/etc/localtime",
    // useradd/groupadd backups
    "/etc/.pwd.lock",
    "/etc/passwd-",
    "/etc/shadow-",
    "/etc/group-",
    "/etc/gshadow-",
    "/etc/subuid-",
    "/etc/subgid-",
    // systemd runtime state
    "/etc/.updated",
    "/etc/machine-info",
    // standard systemd unit symlinks
    "/etc/systemd/system/default.target",
    "/etc/systemd/system/dbus.service",
    "/etc/systemd/user/dbus.service",
    // Network / DNS
    "/etc/resolv.conf",
    "/etc/NetworkManager/NetworkManager-intern.conf",
    // ld.so / system library state
    "/etc/ld.so.cache",
    "/etc/ld.so.conf",
    "/etc/mtab",
    "/etc/rpc",
    // Package manager state
    "/etc/dnf/dnf.conf",
    "/etc/yum.conf",
    "/etc/npmrc",
    // Anaconda / installer artifacts
    "/etc/sysconfig/anaconda",
    "/etc/sysconfig/kernel",
    "/etc/sysconfig/network",
    "/etc/sysconfig/selinux",
    "/etc/sysconfig/network-scripts/readme-ifcfg-rh.txt",
    // Bootloader / kernel
    "/etc/kernel/cmdline",
    // systemd standard targets
    "/etc/systemd/system/ctrl-alt-del.target",
    // OpenSSL configs (not RPM-owned on RHEL 10)
    "/etc/pki/tls/ct_log_list.cnf",
    "/etc/pki/tls/fips_local.cnf",
    "/etc/pki/tls/openssl.cnf",
    // SELinux policy store
    "/etc/selinux/targeted/setrans.conf",
    "/etc/selinux/targeted/seusers",
    "/etc/selinux/targeted/.policy.sha512",
    "/etc/selinux/targeted/booleans.subs_dist",
    // udisks2
    "/etc/udisks2/udisks2.conf",
    "/etc/udisks2/mount_options.conf.example",
    // PAM base configs
    "/etc/pam.d/chfn",
    "/etc/pam.d/chsh",
    "/etc/pam.d/login",
    "/etc/pam.d/remote",
    "/etc/pam.d/runuser",
    "/etc/pam.d/runuser-l",
    "/etc/pam.d/su",
    "/etc/pam.d/su-l",
    // tuned runtime state
    "/etc/tuned/active_profile",
    "/etc/tuned/profile_mode",
    "/etc/tuned/bootcmdline",
    // NOTE: CA bundle symlinks (/etc/ssl/certs/ca-bundle.crt etc.) were
    // previously excluded here because RPM owns the canonical path
    // (/etc/pki/tls/certs/) not the symlink path. These are now handled
    // by the symlink-aware ownership check in is_rpm_owned_via_symlink().
];

/// Glob patterns for system-generated files (fnmatch-style).
pub(crate) const UNOWNED_EXCLUDE_GLOBS: &[&str] = &[
    "/etc/pki/product-default/*.pem",
    "/etc/alternatives/*",
    "/etc/X11/fontpath.d/*",
    "/etc/selinux/*/policy/policy.*",
    "/etc/selinux/*/contexts/*",
    "/etc/selinux/*/contexts/files/*",
    "/etc/selinux/*/contexts/users/*",
    "/etc/udev/hwdb.bin",
    "/etc/pki/ca-trust/extracted/*",
    "/etc/crypto-policies/back-ends/*",
    "/etc/pki/java/cacerts",
    "/etc/pki/tls/cert.pem",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/pki/tls/certs/ca-bundle.trust.crt",
    "/etc/depmod.d/*-dist.conf",
    // NOTE: `/etc/modprobe.d/*-blacklist.conf` was here, "for the distro
    // files". This list is consulted only in the unowned branch of the
    // walk, after is_rpm_owned() and is_rpm_owned_via_symlink(), and not
    // at all in the rpm -Va branch — so a distro-shipped blacklist never
    // reached it and the only files it could drop were ones the admin
    // wrote. Dropping a module blacklist silently changes which driver
    // binds in the migrated image, so the pattern is gone.
    "/etc/dconf/db/distro.d/*",
    "/etc/dconf/db/distro.d/locks/*",
    "/etc/dnf/protected.d/*",
    "/etc/profile.d/gnupg2.*",
    "/etc/logrotate.d/kvm_stat",
    "/etc/systemd/system/*.wants/*",
    "/etc/systemd/system/*.requires/*",
    // An `.upholds` link is a unit-state finding, never configuration: the
    // walk would follow it and stage the linked unit's content at the
    // link's path (§ 7.7 of the units spec).
    "/etc/systemd/system/*.upholds/*",
    "/etc/systemd/user/*.wants/*",
    "/etc/systemd/user/*.requires/*",
    // NOTE: Drop-in overrides (*.service.d/*.conf, *.timer.d/*.conf,
    // *.socket.d/*.conf) are intentionally NOT excluded — they are
    // user-created config that is migration-relevant.
    "/etc/systemd/sleep.conf.d/*",
    "/etc/lvm/archive/*",
    "/etc/lvm/backup/*",
    "/etc/firewalld/zones/*.xml.old",
    "/etc/firewalld/*.xml.old",
    "/etc/NetworkManager/system-connections/*.nmconnection.bak",
    "/etc/sysconfig/network-scripts/readme-*",
    "/etc/pm/sleep.d/*",
];

/// Subtree prefixes for system-generated noise.
/// These are package-manager internals, desktop/session infrastructure, and
/// other runtime-generated trees that are never migration-relevant.
pub(crate) const UNOWNED_EXCLUDE_PREFIX: &[&str] = &[
    // Package manager internals — dnf/yum configs from RPM scriptlets,
    // irrelevant on image-mode systems where dnf doesn't run at runtime
    "/etc/yum/",
    "/etc/rpm/",
    // Desktop/interactive login session systemd units created by
    // systemd-rpm-macros running `systemctl --global enable`
    "/etc/xdg/systemd/",
    // Product certificates subscription-manager installs at registration:
    // unowned, generated, and the source host's subscription identity,
    // not configuration. The trailing slash keeps the match on a path
    // boundary.
    "/etc/pki/product/",
];

/// Basename suffixes of the copies rpm leaves beside a config file it
/// did not overwrite: the old file on removal or upgrade (`.rpmsave`),
/// the new default it did not install (`.rpmnew`), the file it replaced
/// (`.rpmorig`). The live file is the config; these are leftovers no
/// application reads, at any depth.
///
/// A discovery filter only, so not part of [`is_excluded_unowned_path`]:
/// `classify_ghost` reads that list to decide whether a link may be read
/// through, and a live name an admin linked to one of these copies is the
/// config the application reads ([`is_rpm_backup_copy`]).
pub(crate) const UNOWNED_EXCLUDE_SUFFIX: &[&str] = &[".rpmsave", ".rpmnew", ".rpmorig"];

/// Returns `true` if the path should be excluded from unowned file detection.
///
/// Checks: exact match, glob match, subtree prefix exclusion, rpm's
/// backup copies, and the systemd unit symlink test, which needs `exec`
/// to read link metadata.
pub fn is_excluded_unowned(exec: &dyn Executor, path: &str) -> bool {
    // Systemd unit links and masks directly under /etc/systemd/system/:
    // state the units inspector records.
    is_excluded_unowned_path(path)
        || is_rpm_backup_copy(path)
        || is_systemd_unit_symlink(exec, path)
}

/// Whether `path` is a copy rpm left beside a config file
/// ([`UNOWNED_EXCLUDE_SUFFIX`]): never a row of its own, but readable
/// through a live link that names it.
fn is_rpm_backup_copy(path: &str) -> bool {
    UNOWNED_EXCLUDE_SUFFIX
        .iter()
        .any(|suffix| path.ends_with(suffix))
}

/// The part of [`is_excluded_unowned`] decided by the path alone that
/// also decides what a link may be read through: the exact, glob and
/// subtree exclusion lists.
pub(super) fn is_excluded_unowned_path(path: &str) -> bool {
    UNOWNED_EXCLUDE_EXACT.contains(&path)
        || UNOWNED_EXCLUDE_GLOBS
            .iter()
            .any(|pattern| match_unowned_glob(pattern, path))
        // Subtree prefix exclusion (system-generated noise)
        || UNOWNED_EXCLUDE_PREFIX
            .iter()
            .any(|prefix| path.starts_with(prefix))
}

/// Returns `true` if the path is unit state rather than a unit file: a
/// **symlink**, or a zero-length regular file with a unit type suffix,
/// whose name sits directly under `/etc/systemd/system/`.
///
/// **For a link, the whole test is the file type. The link target is not read, and
/// this function does not distinguish the two kinds of link that live
/// here.** Both are systemd's own state and the services inspector
/// records both independently: alias and enablement links from
/// `systemctl enable` (e.g. `dbus-org.fedoraproject.FirewallD1.service`),
/// and mask links to `/dev/null` from `systemctl mask`. Neither carries
/// content of its own, so copying one into the export would turn a link
/// into a file. Naming only aliases here would repeat the defect this
/// replaced, where the name promised more than the code checked.
///
/// A **zero-length regular file** with a unit type suffix there is a mask
/// too: systemd reads it as one, and the units inspector records it as a
/// `Masked` state change. A mask has one representation, so it is no
/// config entry either (§ 7.7 of the units spec).
///
/// A **non-empty regular** file in the same directory is the opposite case:
/// an admin-written unit, either a full shadow of a vendor unit or a wholly
/// new one, and migration-relevant by definition. It must reach
/// `config.files`.
///
/// Examples:
///   `/etc/systemd/system/display-manager.service` → true when it is a symlink
///   `/etc/systemd/system/kdump.service` → true when it is a zero-length file (mask)
///   `/etc/systemd/system/sshd.service` → false when it is a regular file (full shadow)
///   `/etc/systemd/system/sshd.service.d/override.conf` → false (drop-in)
///   `/etc/systemd/system/multi-user.target.wants/sshd.service` → false (handled by glob)
fn is_systemd_unit_symlink(exec: &dyn Executor, path: &str) -> bool {
    const PREFIX: &str = "/etc/systemd/system/";
    let Some(rest) = path.strip_prefix(PREFIX) else {
        return false;
    };
    // Scope the test to bare names directly under the prefix. `.wants/`
    // and `.requires/` links are already glob-excluded above, and a
    // symlink inside a `.d/` drop-in directory is admin-made config.
    if rest.is_empty() || rest.contains('/') {
        return false;
    }
    if exec.read_link(Path::new(path)).is_ok() {
        return true;
    }
    let has_unit_suffix = rest
        .rsplit_once('.')
        .is_some_and(|(_, suffix)| UNIT_TYPE_SUFFIXES.contains(&suffix));
    // Any read error leaves the file to the config walk, which records why
    // it could not capture the content.
    has_unit_suffix
        && super::admit::may_read_to_decide(exec, path)
        && exec
            .read_file(Path::new(path))
            .is_ok_and(|content| content.is_empty())
}

/// Matches a path against a glob pattern: the machine-state class's
/// matcher, so the silent lists and the class rules match alike
/// ([`inspectah_core::machine_state::match_glob`]).
pub(super) fn match_unowned_glob(pattern: &str, path: &str) -> bool {
    inspectah_core::machine_state::match_glob(pattern, path)
}

// ---------------------------------------------------------------------------
// Recursive /etc walk
// ---------------------------------------------------------------------------

/// Recursively walks a directory tree via the Executor, collecting file paths.
///
/// Returns relative paths (relative to `root`) of all regular files found.
/// Prunes dev-artifact directories and VCS trees. Returns an error if the
/// walk found nothing at all, which is what a PermissionDenied on `root`
/// itself looks like.
///
/// A PermissionDenied on a directory *below* `root` is appended to
/// `degraded_reasons` and the walk continues. Those files are absent from
/// the result and therefore from the export, so the caller must report the
/// section degraded rather than complete: the walk earlier discarded these
/// reasons unless it found no files at all, which reported a partial scan
/// of `/etc` as a whole one.
pub fn walk_etc_with_reasons(
    exec: &dyn Executor,
    root: &str,
    degraded_reasons: &mut Vec<String>,
) -> Result<Vec<String>, std::io::Error> {
    let mut files = Vec::new();
    let before = degraded_reasons.len();
    walk_recursive_inner(
        exec,
        root,
        "",
        &mut Vec::new(),
        &mut files,
        degraded_reasons,
    );

    if files.is_empty() && degraded_reasons.len() > before {
        // No files and a permission error: the error was on `root` itself,
        // so it is the whole result rather than a partial-scan note.
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            degraded_reasons.split_off(before).join("; "),
        ));
    }

    Ok(files)
}

/// [`walk_etc_with_reasons`] for the secondary scans (orphan detection,
/// `/usr/etc`, the ostree branch), which do not feed the section's
/// completeness and so have nowhere to put a partial-scan note.
pub fn walk_etc_recursive(exec: &dyn Executor, root: &str) -> Result<Vec<String>, std::io::Error> {
    walk_etc_with_reasons(exec, root, &mut Vec::new())
}

/// `entered` holds each directory link the walk is inside, by where the
/// link itself is ([`link_location`]).
fn walk_recursive_inner(
    exec: &dyn Executor,
    root: &str,
    rel: &str,
    entered: &mut Vec<PathBuf>,
    files: &mut Vec<String>,
    degraded_reasons: &mut Vec<String>,
) {
    let dir = if rel.is_empty() {
        root.to_string()
    } else {
        format!("{root}/{rel}")
    };

    let entries = match exec.read_dir(Path::new(&dir)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            degraded_reasons.push(format!("permission denied: {dir}"));
            return;
        }
        Err(_) => return,
    };

    // Check for VCS prune markers — if any marker is present, skip entire subtree
    for marker in PRUNE_MARKERS {
        if entries.iter().any(|e| e == marker) {
            return;
        }
    }

    for name in &entries {
        let child_rel = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };

        let child_path = format!("{root}/{child_rel}");

        // Check if this is a directory by trying to read it
        match exec.read_dir(Path::new(&child_path)) {
            Ok(_) => {
                // It's a directory — recurse if not a skip dir
                if !SKIP_DIR_NAMES.contains(&name.as_str()) {
                    // Skip directory symlinks that resolve outside root.
                    // Example: /etc/httpd/modules -> /usr/lib64/httpd/modules
                    // Files under these are RPM-owned binaries accessed via
                    // a compat symlink — not user-modified config.
                    if is_symlink_outside_root(exec, &child_path, root) {
                        continue;
                    }
                    // A directory link is entered, and its files listed
                    // under the name the walk reached them by: an
                    // application reads them there. Inside it, the same
                    // link is not entered again, so a link to a directory
                    // above it (`conf.d -> .`) is walked once instead of
                    // without end.
                    let link = link_location(exec, &dir, name);
                    if let Some(link) = &link {
                        if entered.contains(link) {
                            continue;
                        }
                        entered.push(link.clone());
                    }
                    walk_recursive_inner(exec, root, &child_rel, entered, files, degraded_reasons);
                    if link.is_some() {
                        entered.pop();
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                degraded_reasons.push(format!("permission denied: {child_path}"));
            }
            Err(_) => {
                // Not a directory (NotFound from read_dir) — it's a file
                files.push(child_rel);
            }
        }
    }
}

/// Returns `true` if `path` is a symlink whose resolved target is outside `root`.
///
/// Public for testing; primary consumer is `walk_recursive_inner`.
///
/// Used to skip directory symlinks like `/etc/httpd/modules` ->
/// `/usr/lib64/httpd/modules` where the entire subtree consists of
/// RPM-owned binaries, not user-modifiable config files.
pub fn is_symlink_outside_root(exec: &dyn Executor, path: &str, root: &str) -> bool {
    // Only check paths that are actually symlinks.
    if exec.read_link(Path::new(path)).is_err() {
        return false;
    }
    // Resolve the full chain to get the final target.
    if let Ok(resolved) = exec.resolve_final_target(Path::new(path)) {
        let resolved_str = resolved.to_string_lossy();
        !resolved_str.starts_with(root)
    } else {
        // Dangling or broken symlink — skip it (nothing useful to collect).
        true
    }
}

/// Where the link `name` in `dir` is, with `dir` resolved, when it is a
/// link: the same for every spelling the walk reaches it by, so
/// `/etc/httpd/conf.d/conf.d` under `conf.d -> .` is `/etc/httpd/conf.d`.
/// `None` for anything that is not a link. A directory that does not
/// resolve keeps its spelling.
fn link_location(exec: &dyn Executor, dir: &str, name: &str) -> Option<PathBuf> {
    let path = Path::new(dir).join(name);
    exec.read_link(&path).ok()?;
    let here = exec
        .resolve_final_target(Path::new(dir))
        .unwrap_or_else(|_| PathBuf::from(dir));
    Some(here.join(name))
}

/// Returns DHCP-managed NetworkManager connection file paths.
///
/// Scans `/etc/NetworkManager/system-connections/` for `.nmconnection` files
/// whose content indicates DHCP (method=auto). These are excluded from
/// config collection because they are generated at runtime.
pub fn dhcp_connection_paths(exec: &dyn Executor) -> Vec<String> {
    let nm_dir = "/etc/NetworkManager/system-connections";
    let entries = match exec.read_dir(Path::new(nm_dir)) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let mut paths = Vec::new();
    for name in &entries {
        if !name.ends_with(".nmconnection") {
            continue;
        }
        let file_path = format!("{nm_dir}/{name}");
        // Never a FIFO, which blocks the scan, nor a file behind a link
        // the section withholds.
        if !super::admit::may_read_to_decide(exec, &file_path) {
            continue;
        }
        if let Ok(content) = exec.read_file(Path::new(&file_path)) {
            // DHCP connections have method=auto in the [ipv4] section
            if content.contains("method=auto") {
                paths.push(file_path);
            }
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Collect case 15: the silent lists hold noise only. No exact entry
    /// is a class member, and no class rule's path is one a retained glob
    /// or prefix would hide from the walk. `BasenameSuffix` rules are
    /// exempt by design: a `.keytab` in a silent directory reaches the
    /// collector through the orphan branch.
    #[test]
    fn the_silent_lists_hold_no_machine_state_member() {
        use inspectah_core::machine_state::{PathMatch, RULES, classify_path};
        for path in UNOWNED_EXCLUDE_EXACT {
            assert_eq!(classify_path(path), None, "{path} is a class member");
        }
        for (rule, _) in RULES {
            let path = match *rule {
                PathMatch::Exact(p) | PathMatch::Prefix(p) => p.to_string(),
                PathMatch::Glob(g) => g.replace('*', "x"),
                PathMatch::BasenameSuffix(_) => continue,
            };
            assert!(
                !is_excluded_unowned_path(&path),
                "{path}: a silent list hides a class rule's path"
            );
        }
    }
    use crate::executor::mock::MockExecutor;

    // ---- Test 10: test_is_excluded_unowned_exact ----

    #[test]
    fn test_is_excluded_unowned_exact() {
        let exec = MockExecutor::new();
        // Identity files are machine-state class members, recorded
        // visibly, not silent exclusions.
        assert!(!is_excluded_unowned(&exec, "/etc/machine-id"));
        assert!(!is_excluded_unowned(&exec, "/etc/hostname"));
        assert!(is_excluded_unowned(&exec, "/etc/localtime"));
        assert!(is_excluded_unowned(&exec, "/etc/adjtime"));
        assert!(is_excluded_unowned(&exec, "/etc/passwd-"));
        assert!(is_excluded_unowned(&exec, "/etc/shadow-"));
    }

    // ---- Test 11: test_is_excluded_unowned_glob ----

    #[test]
    fn test_is_excluded_unowned_glob() {
        let exec = MockExecutor::new();
        assert!(is_excluded_unowned(
            &exec,
            "/etc/selinux/targeted/contexts/foo"
        ));
        assert!(is_excluded_unowned(
            &exec,
            "/etc/selinux/targeted/contexts/files/bar"
        ));
        assert!(is_excluded_unowned(
            &exec,
            "/etc/NetworkManager/system-connections/eth0.nmconnection.bak"
        ));
        assert!(is_excluded_unowned(&exec, "/etc/alternatives/java"));
        assert!(!is_excluded_unowned(
            &exec,
            "/etc/lvm/devices/system.devices"
        ));
    }

    // ---- Test 12: test_is_excluded_unowned_not_excluded ----

    #[test]
    fn test_is_excluded_unowned_not_excluded() {
        let exec = MockExecutor::new();
        assert!(!is_excluded_unowned(&exec, "/etc/httpd/conf/httpd.conf"));
        assert!(!is_excluded_unowned(&exec, "/etc/custom-app/config.yaml"));
        assert!(!is_excluded_unowned(&exec, "/etc/sshd_config"));
    }

    // ---- Test 13: test_walk_etc_skips_vcs ----

    #[test]
    fn test_walk_etc_skips_vcs() {
        let exec = MockExecutor::new()
            .with_dir("/etc", vec!["httpd", "myapp"])
            // httpd is a regular dir with files
            .with_dir("/etc/httpd", vec!["httpd.conf"])
            // myapp has a .git marker — entire subtree should be pruned
            .with_dir("/etc/myapp", vec![".git", "config.yaml"]);

        let files = walk_etc_recursive(&exec, "/etc").expect("should succeed");
        assert!(
            files.iter().any(|f| f == "httpd/httpd.conf"),
            "should find httpd.conf"
        );
        assert!(
            !files.iter().any(|f| f.contains("myapp")),
            "should skip myapp (has .git marker)"
        );
    }

    // ---- Test 14: test_is_dev_artifact ----

    #[test]
    fn test_is_dev_artifact() {
        assert!(is_dev_artifact("/some/path/.git/config"));
        assert!(is_dev_artifact("/some/path/.svn/entries"));
        assert!(is_dev_artifact("/some/path/node_modules/pkg"));
        assert!(is_dev_artifact("/some/path/__pycache__/mod.pyc"));
        assert!(is_dev_artifact("/some/.tox/env/bin"));
        assert!(!is_dev_artifact("/etc/httpd/conf/httpd.conf"));
        assert!(!is_dev_artifact("/etc/sshd_config"));
    }

    // ---- Cross-inspector boundary tests ----

    /// The walker owns `/etc/audit/rules.d/`, `/etc/pam.d/` and the custom
    /// tuned profiles (§ 3.6).
    ///
    /// Tests 26 and 27 asserted the opposite — that `is_excluded_unowned`
    /// returns true for `/etc/audit/rules.d/custom.rules`,
    /// `/etc/pam.d/sshd` and `/etc/pam.d/system-auth` — and a third rule
    /// excluded `/etc/tuned/*/tuned.conf`. That exclusion was item 1.8's
    /// defect, not a requirement: it handed the three trees to collectors
    /// that emitted files nobody could decide about.
    ///
    /// The three tuned runtime-state files are a different case and stay
    /// excluded, because `tuned_disposition` renders them.
    #[test]
    fn test_config_walker_owns_pam_audit_and_tuned_profiles() {
        let exec = MockExecutor::new();
        assert!(!is_excluded_unowned(
            &exec,
            "/etc/audit/rules.d/custom.rules"
        ));
        assert!(!is_excluded_unowned(&exec, "/etc/pam.d/sshd"));
        assert!(!is_excluded_unowned(&exec, "/etc/pam.d/system-auth"));
        assert!(!is_excluded_unowned(&exec, "/etc/tuned/myapp/tuned.conf"));

        assert!(is_excluded_unowned(&exec, "/etc/tuned/active_profile"));
        assert!(is_excluded_unowned(&exec, "/etc/tuned/profile_mode"));
        assert!(is_excluded_unowned(&exec, "/etc/tuned/bootcmdline"));
        // The eight base PAM configs keep their exact-match exclusion.
        assert!(is_excluded_unowned(&exec, "/etc/pam.d/su-l"));
        assert!(is_excluded_unowned(&exec, "/etc/pam.d/runuser-l"));
    }

    // ---- DHCP filtering ----

    // ---- Noise filter tests ----

    #[test]
    fn test_yum_subtree_excluded() {
        let exec = MockExecutor::new();
        assert!(is_excluded_unowned(
            &exec,
            "/etc/yum/protected.d/setup.conf"
        ));
        assert!(is_excluded_unowned(&exec, "/etc/yum/vars/releasever"));
        assert!(is_excluded_unowned(&exec, "/etc/rpm/macros.dist"));
    }

    #[test]
    fn test_xdg_systemd_excluded() {
        let exec = MockExecutor::new();
        assert!(is_excluded_unowned(
            &exec,
            "/etc/xdg/systemd/user/dbus.service"
        ));
        assert!(is_excluded_unowned(
            &exec,
            "/etc/xdg/systemd/user/sockets.target.wants/dbus.socket"
        ));
    }

    /// `/etc/systemd/system` splits by file type, not by nesting depth.
    ///
    /// **Provenance: the layout is measured, not constructed.** Every
    /// path below was present on the `rhel9` test VM after `driftify
    /// --profile standard`, 2026-10-31, with the link targets it
    /// reported: the full shadow at `sshd.service` is a regular file,
    /// `kdump.service` and `bluetooth.service` are mask links to
    /// `/dev/null`, and the firewalld alias is an enablement link into
    /// the vendor unit directory.
    ///
    /// The predicate used to strip the prefix and exclude anything with
    /// no `/` left, which discarded the admin-written full shadow — the
    /// one file in the directory a migration must carry.
    #[test]
    fn test_systemd_unit_symlinks_excluded_regular_files_kept() {
        let exec = MockExecutor::new()
            .with_link("/etc/systemd/system/kdump.service", "/dev/null")
            .with_link("/etc/systemd/system/bluetooth.service", "/dev/null")
            .with_link(
                "/etc/systemd/system/dbus-org.fedoraproject.FirewallD1.service",
                "/usr/lib/systemd/system/firewalld.service",
            )
            .with_link(
                "/etc/systemd/system/multi-user.target.wants/sshd.service",
                "/usr/lib/systemd/system/sshd.service",
            );

        let cases: &[(&str, bool, &str)] = &[
            (
                "/etc/systemd/system/sshd.service",
                false,
                "a regular file is an admin-written full shadow of the vendor unit",
            ),
            (
                "/etc/systemd/system/driftify-backup.timer",
                false,
                "a regular file with no vendor counterpart is a wholly new admin unit",
            ),
            (
                "/etc/systemd/system/dbus-org.fedoraproject.FirewallD1.service",
                true,
                "an alias link into the vendor unit directory is systemd's own state",
            ),
            (
                "/etc/systemd/system/kdump.service",
                true,
                "a mask link to /dev/null is recorded by the services inspector instead",
            ),
            (
                "/etc/systemd/system/bluetooth.service",
                true,
                "the same, for a unit masked rather than shadowed",
            ),
            (
                "/etc/systemd/system/httpd.service.d/limits.conf",
                false,
                "a drop-in is admin config and was never in scope for this rule",
            ),
            (
                "/etc/systemd/system/multi-user.target.wants/sshd.service",
                true,
                "an enablement link, excluded by the .wants/ glob rather than by this \
                 rule — the depth test was never load-bearing for it",
            ),
        ];

        for (path, excluded, why) in cases {
            assert_eq!(is_excluded_unowned(&exec, path), *excluded, "{path}: {why}");
        }
    }

    #[test]
    fn test_systemd_dropin_not_excluded() {
        // Drop-in overrides inside .d/ directories are user config — keep them
        let exec = MockExecutor::new();
        assert!(!is_excluded_unowned(
            &exec,
            "/etc/systemd/system/sshd.service.d/override.conf"
        ));
        assert!(!is_excluded_unowned(
            &exec,
            "/etc/systemd/system/docker.service.d/http-proxy.conf"
        ));
    }

    #[test]
    fn test_ca_bundle_symlinks_not_statically_excluded() {
        // CA bundle symlinks are no longer statically excluded — they are
        // handled by the symlink-aware ownership check in the config
        // inspector (is_rpm_owned_via_symlink). The canonical paths under
        // /etc/pki/tls/certs/ are still glob-excluded.
        let exec = MockExecutor::new();
        assert!(!is_excluded_unowned(&exec, "/etc/ssl/certs/ca-bundle.crt"));
        assert!(!is_excluded_unowned(
            &exec,
            "/etc/ssl/certs/ca-bundle.trust.crt"
        ));
        // But the canonical paths ARE excluded via the pki glob
        assert!(is_excluded_unowned(
            &exec,
            "/etc/pki/tls/certs/ca-bundle.crt"
        ));
        assert!(is_excluded_unowned(
            &exec,
            "/etc/pki/tls/certs/ca-bundle.trust.crt"
        ));
    }

    #[test]
    fn test_dhcp_connection_paths() {
        let exec = MockExecutor::new()
            .with_dir(
                "/etc/NetworkManager/system-connections",
                vec!["eth0.nmconnection", "static-vpn.nmconnection"],
            )
            .with_file(
                "/etc/NetworkManager/system-connections/eth0.nmconnection",
                "[connection]\nid=eth0\n[ipv4]\nmethod=auto\n",
            )
            .with_file(
                "/etc/NetworkManager/system-connections/static-vpn.nmconnection",
                "[connection]\nid=vpn\n[ipv4]\nmethod=manual\naddress1=10.0.0.1/24\n",
            );

        let paths = dhcp_connection_paths(&exec);
        assert_eq!(paths.len(), 1);
        assert!(paths[0].contains("eth0.nmconnection"));
    }

    /// A FIFO named like a connection, or a link to one, is never opened:
    /// `with_special_file` paths panic when read.
    #[test]
    fn dhcp_detection_never_opens_a_special_file() {
        let nm_dir = "/etc/NetworkManager/system-connections";
        let exec = MockExecutor::new()
            .with_dir(
                nm_dir,
                vec![
                    "control.nmconnection",
                    "link.nmconnection",
                    "eth0.nmconnection",
                ],
            )
            .with_special_file(&format!("{nm_dir}/control.nmconnection"))
            .with_link(
                &format!("{nm_dir}/link.nmconnection"),
                "control.nmconnection",
            )
            .with_file(
                &format!("{nm_dir}/eth0.nmconnection"),
                "[ipv4]\nmethod=auto\n",
            );
        assert_eq!(
            dhcp_connection_paths(&exec),
            [format!("{nm_dir}/eth0.nmconnection")]
        );
    }

    // ---- Symlink-aware directory skipping tests ----

    #[test]
    fn test_is_symlink_outside_root_true() {
        // /etc/httpd/modules -> /usr/lib64/httpd/modules (outside /etc)
        let exec = MockExecutor::new().with_link("/etc/httpd/modules", "/usr/lib64/httpd/modules");
        assert!(is_symlink_outside_root(&exec, "/etc/httpd/modules", "/etc"));
    }

    #[test]
    fn test_is_symlink_outside_root_false_within_etc() {
        // /etc/foo/link -> /etc/foo/real (stays within /etc)
        let exec = MockExecutor::new().with_link("/etc/foo/link", "/etc/foo/real");
        assert!(!is_symlink_outside_root(&exec, "/etc/foo/link", "/etc"));
    }

    #[test]
    fn test_is_symlink_outside_root_not_a_symlink() {
        // Regular directory, not a symlink
        let exec = MockExecutor::new().with_dir("/etc/httpd/conf", vec!["httpd.conf"]);
        assert!(!is_symlink_outside_root(&exec, "/etc/httpd/conf", "/etc"));
    }

    #[test]
    fn test_is_symlink_outside_root_dangling() {
        // Dangling symlink — target doesn't exist, resolve fails
        let exec = MockExecutor::new().with_link("/etc/broken", "/nonexistent/path");
        // Dangling link should be treated as outside (skipped)
        assert!(is_symlink_outside_root(&exec, "/etc/broken", "/etc"));
    }

    /// A link to a directory above it is entered once, so each file
    /// under it is listed under that name as well, and the link is not
    /// entered again inside itself; a link to a sibling directory is
    /// entered, inside the ancestor link as well. The mock lists each
    /// spelling a real `readdir` would follow.
    #[test]
    fn walk_enters_a_link_to_an_ancestor_once_and_one_to_a_sibling() {
        let etc = vec!["app", "httpd", "httpd-live"];
        let exec = MockExecutor::new()
            .with_dir("/etc", etc.clone())
            .with_dir("/etc/app", vec!["app.conf", "state"])
            .with_dir("/etc/app/state", etc.clone())
            .with_dir("/etc/app/state/app", vec!["app.conf", "state"])
            .with_dir("/etc/app/state/app/state", etc)
            .with_dir("/etc/app/state/httpd", vec!["conf.d"])
            .with_dir("/etc/app/state/httpd/conf.d", vec!["site.conf"])
            .with_dir("/etc/app/state/httpd-live", vec!["site.conf"])
            .with_link("/etc/app/state", "/etc")
            .with_dir("/etc/httpd", vec!["conf.d"])
            .with_dir("/etc/httpd/conf.d", vec!["site.conf"])
            .with_link("/etc/httpd/conf.d", "../httpd-live")
            .with_dir("/etc/httpd-live", vec!["site.conf"]);
        let mut files = walk_etc_recursive(&exec, "/etc").expect("the walk succeeds");
        files.sort();
        assert_eq!(
            files,
            [
                "app/app.conf",
                "app/state/app/app.conf",
                "app/state/httpd-live/site.conf",
                "app/state/httpd/conf.d/site.conf",
                "httpd-live/site.conf",
                "httpd/conf.d/site.conf",
            ]
        );
    }

    #[test]
    fn test_walk_etc_skips_directory_symlink_outside_root() {
        // /etc/httpd/modules is a symlink to /usr/lib64/httpd/modules.
        // The walker should skip the entire subtree.
        let exec = MockExecutor::new()
            .with_dir("/etc", vec!["httpd"])
            .with_dir("/etc/httpd", vec!["conf", "modules"])
            .with_dir("/etc/httpd/conf", vec!["httpd.conf"])
            // modules is a directory (read_dir succeeds) but also a symlink
            .with_dir("/etc/httpd/modules", vec!["mod_ssl.so", "mod_proxy.so"])
            .with_link("/etc/httpd/modules", "/usr/lib64/httpd/modules");

        let files = walk_etc_recursive(&exec, "/etc").unwrap();

        // httpd.conf should be found (regular file in regular dir)
        assert!(
            files.iter().any(|f| f == "httpd/conf/httpd.conf"),
            "should find httpd.conf"
        );
        // .so files under the symlinked dir should NOT be found
        assert!(
            !files.iter().any(|f| f.contains("mod_ssl.so")),
            "should skip mod_ssl.so under symlinked modules dir"
        );
        assert!(
            !files.iter().any(|f| f.contains("mod_proxy.so")),
            "should skip mod_proxy.so under symlinked modules dir"
        );
    }

    #[test]
    fn test_walk_etc_follows_directory_symlink_within_root() {
        // /etc/foo/link -> /etc/foo/real (stays within /etc)
        // Should still recurse into it.
        let exec = MockExecutor::new()
            .with_dir("/etc", vec!["foo"])
            .with_dir("/etc/foo", vec!["link"])
            .with_dir("/etc/foo/link", vec!["config.conf"])
            .with_link("/etc/foo/link", "/etc/foo/real");

        let files = walk_etc_recursive(&exec, "/etc").unwrap();
        assert!(
            files.iter().any(|f| f == "foo/link/config.conf"),
            "should find config.conf under within-root symlinked dir"
        );
    }

    #[test]
    fn test_walk_etc_skips_multiple_external_symlinks() {
        // Common RHEL symlinks: modules, logs, run all point outside /etc
        let exec = MockExecutor::new()
            .with_dir("/etc", vec!["httpd"])
            .with_dir("/etc/httpd", vec!["conf", "modules", "logs", "run"])
            .with_dir("/etc/httpd/conf", vec!["httpd.conf"])
            .with_dir("/etc/httpd/modules", vec!["mod_ssl.so"])
            .with_dir("/etc/httpd/logs", vec!["access_log"])
            .with_dir("/etc/httpd/run", vec!["httpd.pid"])
            .with_link("/etc/httpd/modules", "/usr/lib64/httpd/modules")
            .with_link("/etc/httpd/logs", "/var/log/httpd")
            .with_link("/etc/httpd/run", "/run/httpd");

        let files = walk_etc_recursive(&exec, "/etc").unwrap();

        assert!(
            files.iter().any(|f| f == "httpd/conf/httpd.conf"),
            "should find httpd.conf"
        );
        assert!(
            !files.iter().any(|f| f.contains("mod_ssl")),
            "should skip files under modules symlink"
        );
        assert!(
            !files.iter().any(|f| f.contains("access_log")),
            "should skip files under logs symlink"
        );
        assert!(
            !files.iter().any(|f| f.contains("httpd.pid")),
            "should skip files under run symlink"
        );
    }

    /// A link to a directory above it is entered once and not again
    /// inside itself. The mock lists each spelling a real `readdir`
    /// would follow, three passes deep, so a walk that went through the
    /// link again would find `site.conf` under a longer name.
    #[test]
    fn a_link_to_a_directory_above_it_is_walked_once() {
        let entries = vec!["conf.d", "site.conf"];
        let self_link = MockExecutor::new()
            .with_dir("/etc", vec!["httpd"])
            .with_dir("/etc/httpd", entries.clone())
            .with_dir("/etc/httpd/conf.d", entries.clone())
            .with_dir("/etc/httpd/conf.d/conf.d", entries.clone())
            .with_dir("/etc/httpd/conf.d/conf.d/conf.d", entries)
            .with_link("/etc/httpd/conf.d", ".");
        let mut files = walk_etc_recursive(&self_link, "/etc").unwrap();
        files.sort();
        assert_eq!(files, ["httpd/conf.d/site.conf", "httpd/site.conf"]);
    }
}
