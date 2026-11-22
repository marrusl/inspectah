//! Whether `/etc/audit/audit.rules` is the administrator's input or a file
//! the host writes again at every audit start.
//!
//! With the packaged units, `augenrules --load` builds the file from
//! `/etc/audit/rules.d/*.rules` and loads it: `ExecStartPost=` of
//! `auditd.service` on RHEL 9.8 (audit 3.1.5), `ExecStart=` of
//! `audit-rules.service` on RHEL 10.2 (audit-rules 4.0.3); both packaged
//! units are in `testdata/fixtures/config/audit/`. `augenrules` itself
//! (read on both hosts, 2026-11-21) always rewrites the file when
//! `rules.d` exists, since its output starts with a header line. Then the
//! file is generated: the rules that matter are in `rules.d`, which the
//! walk carries, and a copied `audit.rules` is overwritten at the first
//! start.
//!
//! Anything else may load the file as it is. RHEL documents turning
//! `augenrules` off by copying the unit to `/etc/systemd/system/` and
//! enabling the `auditctl -R /etc/audit/audit.rules` line beside it, and a
//! drop-in can do the same in syntax as varied as systemd accepts (dash
//! prefixes, type-wide directories, quoting, continued lines, `/run`).
//! Dropping such a file leaves an image that loads no rules. So the units
//! are not read at all: the file is generated only when the loader units
//! are exactly as packaged, which the scan decides by what is present,
//! and in every other case it is carried with a scan warning.

use inspectah_core::traits::executor::{Executor, FileKind};
use inspectah_core::types::rpm::RpmVaEntry;
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

/// The file this module decides about.
pub(super) const AUDIT_RULES_PATH: &str = "/etc/audit/audit.rules";

/// The directory `augenrules` builds [`AUDIT_RULES_PATH`] from.
const RULES_DIR: &str = "/etc/audit/rules.d";

/// The units that load the audit rules: `auditd.service` on RHEL 9,
/// `audit-rules.service` on RHEL 10.
const LOADER_UNITS: &[&str] = &["auditd.service", "audit-rules.service"];

/// Where packages install unit files.
const PACKAGED_UNIT_DIR: &str = "/usr/lib/systemd/system";

/// Every directory systemd reads system units and drop-ins from
/// (`systemd.unit(5)`, "System Unit Search Path"), the packaged one
/// included.
const UNIT_SEARCH_PATH: &[&str] = &[
    "/etc/systemd/system.control",
    "/run/systemd/system.control",
    "/run/systemd/transient",
    "/run/systemd/generator.early",
    "/etc/systemd/system",
    "/etc/systemd/system.attached",
    "/run/systemd/system",
    "/run/systemd/system.attached",
    "/run/systemd/generator",
    "/usr/local/lib/systemd/system",
    PACKAGED_UNIT_DIR,
    "/run/systemd/generator.late",
];

/// The suffix of a drop-in directory's name.
const DROP_IN_DIR_SUFFIX: &str = ".d";

/// How the scan treats [`AUDIT_RULES_PATH`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Source {
    /// The loader units are exactly as packaged and [`RULES_DIR`] is a
    /// directory, so `augenrules` writes the file again at every start:
    /// not carried.
    Augenrules,
    /// Anything else: carried, with a scan warning giving the reason.
    Carried(Reason),
}

/// What made the scan carry [`AUDIT_RULES_PATH`]. Every reason names
/// paths only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Reason {
    /// [`RULES_DIR`] is not a directory, so `augenrules` would load the
    /// file as it is.
    NoRulesDir,
    /// No loader unit is installed by a package.
    NoPackagedLoader,
    /// `rpm -Va` reports this packaged unit changed.
    ChangedPackagedUnit(String),
    /// `rpm -Va` did not run to the end, so nothing says this packaged
    /// unit is unchanged.
    UnverifiedPackagedUnit(String),
    /// A unit file in the packaged directory no package lists.
    UnpackagedUnit(String),
    /// A unit file, mask or drop-in directory for a loader unit, at this
    /// path outside the package.
    Override(String),
    /// A link naming a loader unit under another name, whose drop-ins
    /// systemd would read too.
    Alias(String),
    /// This path could not be checked, for this reason.
    Unchecked(String, String),
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRulesDir => write!(
                f,
                "{RULES_DIR} is not a directory, so augenrules loads {AUDIT_RULES_PATH} as it is"
            ),
            Self::NoPackagedLoader => write!(
                f,
                "no package installs an audit loader unit ({}) in {PACKAGED_UNIT_DIR}",
                LOADER_UNITS.join(", ")
            ),
            Self::ChangedPackagedUnit(path) => write!(f, "rpm reports {path} changed"),
            Self::UnverifiedPackagedUnit(path) => write!(
                f,
                "rpm -Va did not complete, so {path} is not known to be as packaged"
            ),
            Self::UnpackagedUnit(path) => write!(f, "no package installs {path}"),
            Self::Override(path) => write!(f, "{path} overrides or extends an audit loader unit"),
            Self::Alias(path) => write!(f, "{path} is another name for an audit loader unit"),
            Self::Unchecked(path, error) => write!(f, "{path} could not be checked ({error})"),
        }
    }
}

/// How the scan treats `/etc/audit/audit.rules`. `packaged` is every path
/// under [`PACKAGED_UNIT_DIR`] some package lists, and `verified` the
/// `rpm -Va` rows, `None` when `rpm -Va` did not run to the end. Only what
/// is present is looked at; no unit is read.
pub(super) fn source(
    exec: &dyn Executor,
    packaged: &BTreeSet<String>,
    verified: Option<&[RpmVaEntry]>,
) -> Source {
    match exactly_as_packaged(exec, packaged, verified) {
        Ok(()) => Source::Augenrules,
        Err(reason) => Source::Carried(reason),
    }
}

/// `Ok` when at least one loader unit is installed by a package and
/// verified unchanged, nothing else anywhere in [`UNIT_SEARCH_PATH`]
/// names, masks, aliases or extends a loader unit, and [`RULES_DIR`] is a
/// directory. A failed verification is never "unchanged".
fn exactly_as_packaged(
    exec: &dyn Executor,
    packaged: &BTreeSet<String>,
    verified: Option<&[RpmVaEntry]>,
) -> Result<(), Reason> {
    let mut any_loader = false;
    for unit in LOADER_UNITS {
        for dir in UNIT_SEARCH_PATH {
            let unit_file = format!("{dir}/{unit}");
            match (*dir == PACKAGED_UNIT_DIR, present(exec, &unit_file)?) {
                (_, None) => {}
                (false, Some(_)) => return Err(Reason::Override(unit_file)),
                (true, Some(kind)) => {
                    if kind != FileKind::Regular || !packaged.contains(&unit_file) {
                        return Err(Reason::UnpackagedUnit(unit_file));
                    }
                    let Some(verified) = verified else {
                        return Err(Reason::UnverifiedPackagedUnit(unit_file));
                    };
                    if verified.iter().any(|row| row.path == unit_file) {
                        return Err(Reason::ChangedPackagedUnit(unit_file));
                    }
                    any_loader = true;
                }
            }
            for name in drop_in_dirs(unit) {
                let drop_in_dir = format!("{dir}/{name}");
                if present(exec, &drop_in_dir)?.is_some() {
                    return Err(Reason::Override(drop_in_dir));
                }
            }
        }
    }
    for dir in UNIT_SEARCH_PATH {
        if let Some(alias) = alias_of_a_loader(exec, dir)? {
            return Err(Reason::Alias(alias));
        }
    }
    if !any_loader {
        return Err(Reason::NoPackagedLoader);
    }
    match exec.file_kind(Path::new(RULES_DIR)) {
        Ok(FileKind::Directory) => Ok(()),
        _ => Err(Reason::NoRulesDir),
    }
}

/// What is at `path`, by `lstat`; `None` when nothing is.
fn present(exec: &dyn Executor, path: &str) -> Result<Option<FileKind>, Reason> {
    match exec.file_kind(Path::new(path)) {
        Ok(kind) => Ok(Some(kind)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Reason::Unchecked(path.to_string(), e.to_string())),
    }
}

/// The drop-in directory names systemd reads for `unit`: its own, one for
/// each dash prefix of its name (`audit-.service.d` for
/// `audit-rules.service`), and the type-wide one (`service.d`).
fn drop_in_dirs(unit: &str) -> Vec<String> {
    let (stem, unit_type) = unit
        .rsplit_once('.')
        .expect("a loader unit name has a type suffix");
    let mut names = vec![format!("{unit}{DROP_IN_DIR_SUFFIX}")];
    names.extend(
        stem.match_indices('-')
            .map(|(dash, _)| format!("{}.{unit_type}{DROP_IN_DIR_SUFFIX}", &stem[..=dash])),
    );
    names.push(format!("{unit_type}{DROP_IN_DIR_SUFFIX}"));
    names
}

/// The first entry of `dir` that is a link to a loader unit under another
/// name.
fn alias_of_a_loader(exec: &dyn Executor, dir: &str) -> Result<Option<String>, Reason> {
    let names = match exec.read_dir(Path::new(dir)) {
        Ok(names) => names,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Reason::Unchecked(dir.to_string(), e.to_string())),
    };
    Ok(names
        .into_iter()
        .filter(|name| !LOADER_UNITS.contains(&name.as_str()))
        .map(|name| format!("{dir}/{name}"))
        .find(|path| {
            exec.read_link(Path::new(path)).is_ok_and(|target| {
                Path::new(&target)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| LOADER_UNITS.contains(&name))
            })
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;

    const EL9_AUDITD: &str = "/usr/lib/systemd/system/auditd.service";
    const EL10_AUDIT_RULES: &str = "/usr/lib/systemd/system/audit-rules.service";

    /// RHEL 9 as packaged: `auditd.service` listed by `audit`.
    fn el9() -> (MockExecutor, BTreeSet<String>) {
        let exec = MockExecutor::new()
            .with_file(EL9_AUDITD, "[Service]\n")
            .with_dir(RULES_DIR, vec!["audit.rules"]);
        (exec, BTreeSet::from([EL9_AUDITD.to_string()]))
    }

    /// RHEL 10 as packaged: both units listed by their packages.
    fn el10() -> (MockExecutor, BTreeSet<String>) {
        let exec = MockExecutor::new()
            .with_file(EL9_AUDITD, "[Service]\n")
            .with_file(EL10_AUDIT_RULES, "[Service]\n")
            .with_dir(RULES_DIR, vec!["audit.rules"]);
        (
            exec,
            BTreeSet::from([EL9_AUDITD.to_string(), EL10_AUDIT_RULES.to_string()]),
        )
    }

    #[test]
    fn the_packaged_units_are_augenrules() {
        for (exec, packaged) in [el9(), el10()] {
            assert_eq!(source(&exec, &packaged, Some(&[])), Source::Augenrules);
        }
    }

    #[test]
    fn drop_in_directory_names_follow_systemd() {
        assert_eq!(
            drop_in_dirs("audit-rules.service"),
            ["audit-rules.service.d", "audit-.service.d", "service.d"]
        );
        assert_eq!(
            drop_in_dirs("auditd.service"),
            ["auditd.service.d", "service.d"]
        );
    }

    /// Anything present beside the packaged units, or a verification that
    /// did not complete, carries the file, with the path that was found;
    /// nothing is read.
    #[test]
    fn anything_beside_the_packaged_units_is_carried() {
        let changed = |path: &str| RpmVaEntry {
            path: path.into(),
            flags: "S.5....T.".into(),
            package: None,
        };
        let override_at = |p: &str| Source::Carried(Reason::Override(p.into()));
        let cases: Vec<(&str, MockExecutor, Option<Vec<RpmVaEntry>>, Source)> = vec![
            (
                "an /etc unit",
                el9().0.with_file("/etc/systemd/system/auditd.service", "x"),
                Some(vec![]),
                override_at("/etc/systemd/system/auditd.service"),
            ),
            (
                "a /dev/null mask",
                el9()
                    .0
                    .with_link("/etc/systemd/system/auditd.service", "/dev/null"),
                Some(vec![]),
                override_at("/etc/systemd/system/auditd.service"),
            ),
            (
                "a dash-prefix drop-in in /run",
                el10()
                    .0
                    .with_dir("/run/systemd/system/audit-.service.d", vec![".x.conf"]),
                Some(vec![]),
                override_at("/run/systemd/system/audit-.service.d"),
            ),
            (
                "a type-wide drop-in in the packaged directory",
                el9()
                    .0
                    .with_dir("/usr/lib/systemd/system/service.d", vec!["10-x.conf"]),
                Some(vec![]),
                override_at("/usr/lib/systemd/system/service.d"),
            ),
            (
                "an alias",
                el9()
                    .0
                    .with_link("/etc/systemd/system/audit.service", EL9_AUDITD)
                    .with_dir("/etc/systemd/system", vec!["audit.service"]),
                Some(vec![]),
                Source::Carried(Reason::Alias("/etc/systemd/system/audit.service".into())),
            ),
            (
                "a changed packaged unit",
                el9().0,
                Some(vec![changed(EL9_AUDITD)]),
                Source::Carried(Reason::ChangedPackagedUnit(EL9_AUDITD.into())),
            ),
            (
                "verification that did not complete",
                el9().0,
                None,
                Source::Carried(Reason::UnverifiedPackagedUnit(EL9_AUDITD.into())),
            ),
            (
                "an unlisted unit in the packaged directory",
                el9().0.with_file(EL10_AUDIT_RULES, "x"),
                Some(vec![]),
                Source::Carried(Reason::UnpackagedUnit(EL10_AUDIT_RULES.into())),
            ),
            (
                "no packaged loader",
                MockExecutor::new().with_dir(RULES_DIR, vec![]),
                Some(vec![]),
                Source::Carried(Reason::NoPackagedLoader),
            ),
            (
                "no rules.d",
                MockExecutor::new().with_file(EL9_AUDITD, "x"),
                Some(vec![]),
                Source::Carried(Reason::NoRulesDir),
            ),
            (
                "an unlistable search directory",
                el9()
                    .0
                    .with_dir_error("/etc/systemd/system", std::io::ErrorKind::PermissionDenied),
                Some(vec![]),
                Source::Carried(Reason::Unchecked(
                    "/etc/systemd/system".into(),
                    "/etc/systemd/system".into(),
                )),
            ),
        ];
        let packaged = el9().1;
        for (case, exec, verified, expected) in cases {
            assert_eq!(
                source(&exec, &packaged, verified.as_deref()),
                expected,
                "{case}"
            );
        }
    }
}
