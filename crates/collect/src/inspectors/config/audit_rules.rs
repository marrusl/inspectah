//! Whether `/etc/audit/audit.rules` is the administrator's input or a file
//! the host writes again at every audit start.
//!
//! By default `augenrules --load` builds the file from
//! `/etc/audit/rules.d/*.rules` and loads it: `ExecStartPost=` of
//! `auditd.service` on RHEL 9.8 (audit 3.1.5), `ExecStart=` of
//! `audit-rules.service` on RHEL 10.2 (audit-rules 4.0.3); both packaged
//! units are in `testdata/fixtures/config/audit/`. Then the file is
//! generated: the rules that matter are in `rules.d`, which the walk
//! carries, and a copied `audit.rules` is overwritten at the first start.
//!
//! RHEL also supports turning `augenrules` off: copy the unit to
//! `/etc/systemd/system/`, comment out the `augenrules` line and enable
//! the `auditctl -R /etc/audit/audit.rules` line beside it (the RHEL 9
//! security hardening guide, "Disabling augenrules", and the comment in
//! the packaged unit itself). Then `audit.rules` is what the host loads,
//! and dropping it would leave an image that loads no rules at all.
//!
//! `augenrules` itself (read on both hosts, 2026-11-21) always rewrites
//! the file when `rules.d` exists, since its output starts with a header
//! line, and leaves it alone and loads it as it is when `rules.d` is
//! missing.

use super::admit::opens_without_blocking;
use inspectah_core::traits::executor::{Executor, FileKind};
use std::path::Path;

/// The file this module decides about.
pub(super) const AUDIT_RULES_PATH: &str = "/etc/audit/audit.rules";

/// The directory `augenrules` builds [`AUDIT_RULES_PATH`] from.
const RULES_DIR: &str = "/etc/audit/rules.d";

/// The units that load the audit rules: `auditd.service` on RHEL 9,
/// `audit-rules.service` on RHEL 10.
const LOADER_UNITS: &[&str] = &["auditd.service", "audit-rules.service"];

/// Where a unit's own file is looked for, the administrator's first.
const UNIT_DIRS: &[&str] = &["/etc/systemd/system", "/usr/lib/systemd/system"];

/// What a masked unit file links to.
const DEV_NULL: &str = "/dev/null";

/// The drop-in directory for every service, beside each unit's own.
const SERVICE_TYPE_DROP_IN_DIR: &str = "service.d";

/// A drop-in file's suffix.
const DROP_IN_SUFFIX: &str = ".conf";

/// The section the exec settings live in.
const SERVICE_SECTION: &str = "[Service]";

/// The settings whose commands run when the unit starts.
const START_COMMAND_KEYS: &[&str] = &["ExecStartPre", "ExecStart", "ExecStartPost"];

/// The characters systemd allows before an exec command's path (`-` to
/// ignore failure, and the privilege prefixes).
const EXEC_PREFIX_CHARS: &[char] = &['-', '@', ':', '+', '!', '|'];

/// The program that builds and loads the rules from [`RULES_DIR`].
const AUGENRULES: &str = "augenrules";

/// The `augenrules` option that only reports and writes nothing.
const AUGENRULES_CHECK_ONLY: &str = "--check";

/// The program that loads a rules file, and its option for one.
const AUDITCTL: &str = "auditctl";
const AUDITCTL_LOAD_FILE: &str = "-R";

/// How the host comes by [`AUDIT_RULES_PATH`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Source {
    /// A loader unit runs `augenrules` and [`RULES_DIR`] exists, so the
    /// file is written again from `rules.d` at every start: not carried.
    Augenrules,
    /// No loader unit runs `augenrules` and one loads the file with
    /// `auditctl -R`: the administrator's input, carried as config.
    LoadedDirectly,
    /// Neither could be established, for this reason: carried, with a
    /// scan warning.
    Unknown(String),
}

/// How the host comes by `/etc/audit/audit.rules`, from the start
/// commands of its loader units after `/etc` overrides and drop-ins.
pub(super) fn source(exec: &dyn Executor) -> Source {
    let mut commands: Vec<Vec<String>> = Vec::new();
    for unit in LOADER_UNITS {
        match start_commands(exec, unit) {
            Ok(found) => commands.extend(found),
            Err(reason) => return Source::Unknown(reason),
        }
    }
    let regenerates = commands.iter().any(|argv| runs_augenrules(argv));
    let loads_directly = commands.iter().any(|argv| loads_audit_rules(argv));
    if regenerates {
        if matches!(
            exec.file_kind(Path::new(RULES_DIR)),
            Ok(FileKind::Directory)
        ) {
            Source::Augenrules
        } else {
            Source::Unknown(format!(
                "augenrules runs, but {RULES_DIR} is not a directory, so it loads \
                 {AUDIT_RULES_PATH} as it is"
            ))
        }
    } else if loads_directly {
        Source::LoadedDirectly
    } else {
        Source::Unknown(format!(
            "no audit unit runs augenrules or loads {AUDIT_RULES_PATH} with auditctl -R"
        ))
    }
}

/// The start commands of `unit`, each split into words: its own file
/// (the first of [`UNIT_DIRS`] that has it), then its drop-ins and the
/// service-wide ones in file-name order, an `/etc` drop-in hiding a
/// packaged one of the same name. An empty assignment clears the
/// setting, as systemd does. No commands when the host has no such
/// unit or the unit is masked; an error when a file that decides could
/// not be read.
fn start_commands(exec: &dyn Executor, unit: &str) -> Result<Vec<Vec<String>>, String> {
    let Some(unit_file) = UNIT_DIRS
        .iter()
        .map(|dir| format!("{dir}/{unit}"))
        .find(|path| exec.file_kind(Path::new(path)).is_ok())
    else {
        return Ok(Vec::new());
    };
    // A masked unit never runs, and systemd reads none of its drop-ins.
    if exec
        .resolve_final_target(Path::new(&unit_file))
        .is_ok_and(|target| target == Path::new(DEV_NULL))
    {
        return Ok(Vec::new());
    }
    let mut files = vec![unit_file];

    let mut drop_ins: Vec<(String, String)> = Vec::new();
    for dir in UNIT_DIRS {
        for drop_in_dir in [
            format!("{dir}/{unit}.d"),
            format!("{dir}/{SERVICE_TYPE_DROP_IN_DIR}"),
        ] {
            let names = match exec.read_dir(Path::new(&drop_in_dir)) {
                Ok(names) => names,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(format!("{drop_in_dir} could not be listed: {e}")),
            };
            for name in names {
                let hidden = drop_ins.iter().any(|(seen, _)| *seen == name);
                if name.ends_with(DROP_IN_SUFFIX) && !hidden {
                    drop_ins.push((name.clone(), format!("{drop_in_dir}/{name}")));
                }
            }
        }
    }
    drop_ins.sort();
    files.extend(drop_ins.into_iter().map(|(_, path)| path));

    let mut settings: Vec<(&str, Vec<Vec<String>>)> = START_COMMAND_KEYS
        .iter()
        .map(|key| (*key, Vec::new()))
        .collect();
    for path in files {
        if !opens_without_blocking(exec, &path) {
            return Err(format!("{path} is not a regular file"));
        }
        let content = exec
            .read_file(Path::new(&path))
            .map_err(|e| format!("{path} could not be read: {e}"))?;
        apply_unit_file(&content, &mut settings);
    }
    Ok(settings
        .into_iter()
        .flat_map(|(_, commands)| commands)
        .collect())
}

/// Apply the start-command assignments of one unit file or drop-in, in
/// order, to `settings`.
fn apply_unit_file(content: &str, settings: &mut [(&str, Vec<Vec<String>>)]) {
    let mut in_service = false;
    for line in content.lines().map(str::trim) {
        if line.starts_with('[') {
            in_service = line == SERVICE_SECTION;
            continue;
        }
        if !in_service || line.starts_with(['#', ';']) {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some((_, commands)) = settings.iter_mut().find(|(k, _)| *k == key.trim()) else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            commands.clear();
            continue;
        }
        // One value can hold several commands separated by a lone `;`.
        let words: Vec<&str> = value.split_whitespace().collect();
        for command in words.split(|word| *word == ";") {
            if !command.is_empty() {
                commands.push(command.iter().map(|w| (*w).to_string()).collect());
            }
        }
    }
}

/// The program a command runs, by file name, without its prefix
/// characters.
fn program(argv: &[String]) -> &str {
    argv.first()
        .map(|first| first.trim_start_matches(EXEC_PREFIX_CHARS))
        .and_then(|path| path.rsplit('/').next())
        .unwrap_or("")
}

/// Whether the command writes the rules from `rules.d`.
fn runs_augenrules(argv: &[String]) -> bool {
    program(argv) == AUGENRULES && !argv.iter().any(|arg| arg == AUGENRULES_CHECK_ONLY)
}

/// Whether the command loads `/etc/audit/audit.rules` as it is.
fn loads_audit_rules(argv: &[String]) -> bool {
    program(argv) == AUDITCTL
        && argv
            .windows(2)
            .any(|pair| pair[0] == AUDITCTL_LOAD_FILE && pair[1] == AUDIT_RULES_PATH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;

    /// A packaged unit as measured on the test hosts, 2026-11-21.
    macro_rules! unit {
        ($release:literal, $name:literal) => {
            include_str!(concat!(
                "../../../../../testdata/fixtures/config/audit/",
                $release,
                "/",
                $name
            ))
        };
    }
    const EL9_AUDITD: &str = unit!("rhel-9.8", "auditd.service");
    const EL10_AUDITD: &str = unit!("rhel-10.2", "auditd.service");
    const EL10_AUDIT_RULES: &str = unit!("rhel-10.2", "audit-rules.service");

    /// The RHEL 9 unit as the documented procedure leaves it in `/etc`:
    /// the `augenrules` line commented out, the `auditctl` line enabled.
    fn el9_documented_override() -> String {
        EL9_AUDITD
            .replace(
                "ExecStartPost=-/sbin/augenrules --load",
                "#ExecStartPost=-/sbin/augenrules --load",
            )
            .replace(
                "#ExecStartPost=-/sbin/auditctl -R /etc/audit/audit.rules",
                "ExecStartPost=-/sbin/auditctl -R /etc/audit/audit.rules",
            )
    }

    fn el9() -> MockExecutor {
        MockExecutor::new()
            .with_file("/usr/lib/systemd/system/auditd.service", EL9_AUDITD)
            .with_dir(RULES_DIR, vec!["audit.rules"])
    }

    fn el10() -> MockExecutor {
        MockExecutor::new()
            .with_file("/usr/lib/systemd/system/auditd.service", EL10_AUDITD)
            .with_file(
                "/usr/lib/systemd/system/audit-rules.service",
                EL10_AUDIT_RULES,
            )
            .with_dir(RULES_DIR, vec!["audit.rules"])
    }

    #[test]
    fn the_packaged_units_run_augenrules() {
        assert_eq!(source(&el9()), Source::Augenrules);
        assert_eq!(source(&el10()), Source::Augenrules);
    }

    #[test]
    fn the_documented_override_loads_the_file_directly() {
        let exec = el9().with_file(
            "/etc/systemd/system/auditd.service",
            &el9_documented_override(),
        );
        assert_eq!(source(&exec), Source::LoadedDirectly);
    }

    /// The same change as a drop-in: an empty assignment clears the
    /// packaged command, as systemd does.
    #[test]
    fn a_drop_in_that_replaces_augenrules_loads_the_file_directly() {
        let el9_drop_in = el9()
            .with_dir("/etc/systemd/system/auditd.service.d", vec!["10-load.conf"])
            .with_file(
                "/etc/systemd/system/auditd.service.d/10-load.conf",
                "[Service]\nExecStartPost=\nExecStartPost=-/sbin/auditctl -R /etc/audit/audit.rules\n",
            );
        assert_eq!(source(&el9_drop_in), Source::LoadedDirectly);

        let el10_drop_in = el10()
            .with_dir(
                "/etc/systemd/system/audit-rules.service.d",
                vec!["10-load.conf"],
            )
            .with_file(
                "/etc/systemd/system/audit-rules.service.d/10-load.conf",
                "[Service]\nExecStart=\nExecStart=/usr/sbin/auditctl -R /etc/audit/audit.rules\n",
            );
        assert_eq!(source(&el10_drop_in), Source::LoadedDirectly);

        // A drop-in that only adds a command leaves augenrules running.
        let added = el9()
            .with_dir("/etc/systemd/system/auditd.service.d", vec!["10-more.conf"])
            .with_file(
                "/etc/systemd/system/auditd.service.d/10-more.conf",
                "[Service]\nExecStartPost=-/sbin/auditctl -R /etc/audit/audit.rules\n",
            );
        assert_eq!(source(&added), Source::Augenrules);
    }

    /// Whatever the scan cannot settle is `Unknown`, never `Augenrules`.
    #[test]
    fn what_cannot_be_established_is_unknown() {
        let no_rules_d =
            MockExecutor::new().with_file("/usr/lib/systemd/system/auditd.service", EL9_AUDITD);
        let masked = el9().with_link("/etc/systemd/system/auditd.service", "/dev/null");
        let unreadable = el9()
            .with_dir("/etc/systemd/system/auditd.service.d", vec!["10-x.conf"])
            .with_file_error(
                "/etc/systemd/system/auditd.service.d/10-x.conf",
                std::io::ErrorKind::PermissionDenied,
            );
        let fifo = el9()
            .with_dir("/etc/systemd/system/auditd.service.d", vec!["10-x.conf"])
            .with_special_file("/etc/systemd/system/auditd.service.d/10-x.conf");
        let no_unit = MockExecutor::new().with_dir(RULES_DIR, vec![]);
        for (name, exec) in [
            ("no rules.d", no_rules_d),
            ("masked", masked),
            ("unreadable drop-in", unreadable),
            ("FIFO drop-in", fifo),
            ("no unit", no_unit),
        ] {
            assert!(
                matches!(source(&exec), Source::Unknown(_)),
                "{name}: {:?}",
                source(&exec)
            );
        }
    }

    #[test]
    fn commands_by_program_and_arguments() {
        let argv = |line: &str| {
            line.split_whitespace()
                .map(String::from)
                .collect::<Vec<_>>()
        };
        assert!(runs_augenrules(&argv("-/sbin/augenrules --load")));
        assert!(runs_augenrules(&argv("/usr/sbin/augenrules")));
        assert!(!runs_augenrules(&argv("/usr/sbin/augenrules --check")));
        assert!(loads_audit_rules(&argv(
            "-/sbin/auditctl -R /etc/audit/audit.rules"
        )));
        assert!(!loads_audit_rules(&argv(
            "/sbin/auditctl -R /etc/audit/audit-stop.rules"
        )));
        assert!(!loads_audit_rules(&argv("/sbin/auditctl -D")));
    }
}
