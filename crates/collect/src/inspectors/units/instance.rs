//! What an enabled template instance reads and keeps (§ 7.5 of the units
//! spec, "What an instance needs"): the paths under `/etc` it cannot start
//! without (`required_config`) and the paths under `/var` that hold its
//! persistent data (`data_paths`).
//!
//! Each list has two sources: the paths the effective unit names that hold
//! `%i` or `%I`, expanded for the instance, and a fixed table for the
//! templates whose unit file does not name what they read. Values are
//! expanded with [`SpecifierSet::Runtime`], since systemd expands them
//! when it loads the unit; `[Install]` is not read here. Only what the
//! host's files say is recorded: the plan decides from it.
//!
//! Each setting is read with its own syntax, as systemd's parser for it
//! reads it: a command line is split into words with quotes and C escapes
//! removed before any path is looked for, an `EnvironmentFile=` or a
//! condition is one path, and so on ([`Syntax`]). A value that names a
//! path for the instance and cannot be read that way makes the instance's
//! needs unknown ([`NeedsUnknown`]): its enable is not carried, rather than
//! carried with the path dropped.

use super::install::DROP_IN_DIR_SUFFIX;
use super::resolve::unit_search_path;
use super::specifiers::{SpecifierSet, expand_unit_specifiers};
use inspectah_core::traits::executor::Executor;
use inspectah_core::types::units::{
    InstanceNeedsUnknown, UNIT_FILE_WHITESPACE, UnitName, is_instance_config_path,
    is_instance_data_path, numbered_logical_lines, systemd_reads_drop_in,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io::ErrorKind;
use std::path::Path;

/// Templates whose instance reads a configuration file its unit file does
/// not name, and that file's path.
/// `wg-quick@.service` runs `wg-quick up %i`; the OpenVPN templates pass
/// `--config %i.conf` from a `WorkingDirectory=`.
const INSTANCE_CONFIG_TABLE: [(&str, &str); 3] = [
    ("wg-quick@.service", "/etc/wireguard/%i.conf"),
    ("openvpn-client@.service", "/etc/openvpn/client/%i.conf"),
    ("openvpn-server@.service", "/etc/openvpn/server/%i.conf"),
];

/// Templates whose instance keeps data at a path its unit file does not
/// name, and that path.
const INSTANCE_DATA_TABLE: [(&str, &str); 1] = [("dirsrv@.service", "/var/lib/dirsrv/slapd-%i")];

/// Whether `template` is on one of the three fixed lists of § 7.5: the
/// configuration and data tables here, and the templates tied to a host's
/// devices.
pub(crate) fn on_a_list(template: &UnitName) -> bool {
    INSTANCE_CONFIG_TABLE
        .iter()
        .chain(&INSTANCE_DATA_TABLE)
        .any(|(name, _)| *name == template.as_str())
        || super::links::HOST_DEVICE_TEMPLATES.contains(&template.as_str())
}

/// The key whose value is a file of environment assignments systemd reads.
const ENVIRONMENT_FILE_KEY: &str = "EnvironmentFile";

/// The prefix that makes an `EnvironmentFile=` optional to systemd.
const OPTIONAL_FILE_PREFIX: char = '-';

/// The key whose values name directories systemd keeps under `/var/lib`.
const STATE_DIRECTORY_KEY: &str = "StateDirectory";

/// Where systemd places a system unit's `StateDirectory=`.
const STATE_DIRECTORY_ROOT: &str = "/var/lib";

/// The separator of a `StateDirectory=` value's optional second part, a
/// symlink name systemd also creates; the first part is the directory.
const STATE_DIRECTORY_LINK_SEPARATOR: char = ':';

/// The section of a unit file that names units, not paths.
const INSTALL_SECTION: &str = "Install";

/// The section systemd reads conditions and assertions from.
const UNIT_SECTION: &str = "Unit";

/// The conditions and assertions systemd knows, each read in
/// [`UNIT_SECTION`] (systemd 252 and 257, `load-fragment-gperf.gperf.in`,
/// the same list in both). An empty one resets every condition, or every
/// assertion; a name not on the list is ignored, so it resets nothing.
const CONDITION_KEYS: [&str; 65] = [
    "ConditionPathExists",
    "ConditionPathExistsGlob",
    "ConditionPathIsDirectory",
    "ConditionPathIsSymbolicLink",
    "ConditionPathIsMountPoint",
    "ConditionPathIsReadWrite",
    "ConditionPathIsEncrypted",
    "ConditionDirectoryNotEmpty",
    "ConditionFileNotEmpty",
    "ConditionFileIsExecutable",
    "ConditionNeedsUpdate",
    "ConditionFirstBoot",
    "ConditionArchitecture",
    "ConditionFirmware",
    "ConditionVirtualization",
    "ConditionHost",
    "ConditionKernelCommandLine",
    "ConditionKernelVersion",
    "ConditionCredential",
    "ConditionSecurity",
    "ConditionCapability",
    "ConditionACPower",
    "ConditionMemory",
    "ConditionCPUFeature",
    "ConditionCPUs",
    "ConditionEnvironment",
    "ConditionUser",
    "ConditionGroup",
    "ConditionControlGroupController",
    "ConditionOSRelease",
    "ConditionMemoryPressure",
    "ConditionCPUPressure",
    "ConditionIOPressure",
    "AssertPathExists",
    "AssertPathExistsGlob",
    "AssertPathIsDirectory",
    "AssertPathIsSymbolicLink",
    "AssertPathIsMountPoint",
    "AssertPathIsReadWrite",
    "AssertPathIsEncrypted",
    "AssertDirectoryNotEmpty",
    "AssertFileNotEmpty",
    "AssertFileIsExecutable",
    "AssertNeedsUpdate",
    "AssertFirstBoot",
    "AssertArchitecture",
    "AssertVirtualization",
    "AssertHost",
    "AssertKernelCommandLine",
    "AssertKernelVersion",
    "AssertCredential",
    "AssertSecurity",
    "AssertCapability",
    "AssertACPower",
    "AssertMemory",
    "AssertCPUFeature",
    "AssertCPUs",
    "AssertEnvironment",
    "AssertUser",
    "AssertGroup",
    "AssertControlGroupController",
    "AssertOSRelease",
    "AssertMemoryPressure",
    "AssertCPUPressure",
    "AssertIOPressure",
];

/// The type suffix of a service unit.
const SERVICE_SUFFIX: &str = "service";

/// The type suffix of a socket unit.
const SOCKET_SUFFIX: &str = "socket";

/// The command keys a service takes in `[Service]` (systemd 252 and 257,
/// `load-fragment-gperf.gperf.in`).
const SERVICE_COMMAND_KEYS: [&str; 7] = [
    "ExecCondition",
    "ExecStartPre",
    "ExecStart",
    "ExecStartPost",
    "ExecReload",
    "ExecStop",
    "ExecStopPost",
];

/// The command keys a socket takes in `[Socket]` (systemd 252 and 257).
const SOCKET_COMMAND_KEYS: [&str; 4] = [
    "ExecStartPre",
    "ExecStartPost",
    "ExecStopPre",
    "ExecStopPost",
];

/// The unit types whose own section takes the execution settings
/// (`EXEC_CONTEXT_CONFIG_ITEMS`: `EnvironmentFile=`, the path lists, the
/// bind mounts, `StateDirectory=`, `WorkingDirectory=`, `RootDirectory=`
/// and `RootImage=`), with that section (systemd 252 and 257).
const EXEC_SECTIONS: [(&str, &str); 4] = [
    (SERVICE_SUFFIX, "Service"),
    (SOCKET_SUFFIX, "Socket"),
    ("mount", "Mount"),
    ("swap", "Swap"),
];

/// The key only a service takes, in `[Service]`, whose relative value is
/// made absolute under [`PID_FILE_RUNTIME_DIR`].
const PID_FILE_KEY: &str = "PIDFile";

/// Where systemd places a relative `PIDFile=`.
const PID_FILE_RUNTIME_DIR: &str = "/run";

/// The key whose value `~` means the user's home directory.
const WORKING_DIRECTORY_KEY: &str = "WorkingDirectory";

/// `WorkingDirectory=`'s value for the user's home directory.
const HOME_DIRECTORY: &str = "~";

/// The API file systems systemd 257 refuses a `PIDFile=` under
/// (`PATH_CHECK_NON_API_VFS`); 252 takes them, and treating them as
/// refused there keeps an earlier path, toward omitting.
const API_VFS_ROOTS: [&str; 3] = ["/proc", "/sys", "/dev"];

/// The longest path systemd takes, its terminating NUL excluded
/// (`PATH_MAX` less one): the most `unit_path_printf` expands a value to,
/// and the most `path_is_valid` takes once it is simplified.
const PATH_MAX_LENGTH: usize = 4095;

/// The longest component systemd takes (`NAME_MAX`, `path_is_valid`).
const NAME_MAX_LENGTH: usize = 255;

/// The root an instance's configuration sits under.
const CONFIG_ROOT: &str = "/etc";

/// The keys whose value is a command line, read by `config_parse_exec`.
const COMMAND_KEYS: [&str; 8] = [
    "ExecCondition",
    "ExecStartPre",
    "ExecStart",
    "ExecStartPost",
    "ExecReload",
    "ExecStop",
    "ExecStopPre",
    "ExecStopPost",
];

/// The prefixes systemd allows before a command's executable.
const COMMAND_PREFIXES: &[char] = &['-', '@', ':', '+', '!'];

/// A lone word that separates two commands on one command line.
const COMMAND_SEPARATOR: char = ';';

/// The keys whose value is a list of paths, read by
/// `config_parse_namespace_path_strv`.
const PATH_LIST_KEYS: [&str; 5] = [
    "ReadWritePaths",
    "ReadOnlyPaths",
    "InaccessiblePaths",
    "ExecPaths",
    "NoExecPaths",
];

/// The prefixes a path-list entry may carry: `-` for an optional path and
/// `+` for one under the unit's root directory.
const PATH_LIST_PREFIXES: &[char] = &['-', '+'];

/// The keys whose value is a list of `source[:destination[:options]]` bind
/// mounts, read by `config_parse_bind_paths`.
const BIND_KEYS: [&str; 2] = ["BindPaths", "BindReadOnlyPaths"];

/// The prefix that makes a bind mount's source optional.
const BIND_OPTIONAL_PREFIX: char = '-';

/// The separator of a bind mount's parts.
const BIND_PART_SEPARATOR: char = ':';

/// The keys whose whole value is one path, quotes and spaces included.
const SINGLE_PATH_KEYS: [&str; 4] = ["WorkingDirectory", "RootDirectory", "RootImage", "PIDFile"];

/// The prefix that makes a single-path setting's path optional.
const SINGLE_PATH_OPTIONAL_PREFIX: char = '-';

/// The key prefixes of the conditions and assertions. A path condition's
/// whole value is one path after an optional `|` and `!`
/// (`config_parse_unit_condition_path`).
const CONDITION_KEY_PREFIXES: [&str; 2] = ["Condition", "Assert"];

/// The prefix of a triggering condition: one of several alternatives.
const TRIGGER_PREFIX: char = '|';

/// The prefix that negates a condition: the path must not exist, so it is
/// not something the instance needs.
const NEGATION_PREFIX: char = '!';

/// The characters inside a word of a setting whose syntax inspectah does
/// not model after which a path operand can start, as in
/// `--config=/etc/x.conf` or `src:/etc/x`. A word that starts with a path
/// is that path whole, `:` and `,` included.
const OPERAND_SEPARATORS: &[char] = &['=', ':', ','];

/// The characters systemd separates words with.
const WORD_SEPARATORS: &[char] = UNIT_FILE_WHITESPACE;

/// The quote characters systemd removes from a word.
const QUOTES: [char; 2] = ['"', '\''];

/// What one enabled template instance needs, sorted and without repeats.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct InstanceNeeds {
    /// Paths under `/etc`, kept whether or not anything exists there.
    pub(crate) required_config: Vec<String>,
    /// Paths under `/var` outside `/var/run` and `/var/lock` that exist on
    /// the source host.
    pub(crate) data_paths: Vec<String>,
    /// Paths of the runtime drop-ins whose reset, replacement or hiding of
    /// a drop-in of their file name removed or changed a path the two lists
    /// would otherwise hold. Without one of them the image reads that path
    /// again, unchecked, so each is evidence the export has to stage (§ 10.2
    /// rule 5), never a requirement.
    pub(crate) overriding_drop_ins: Vec<String>,
}

/// Why what an instance needs is unknown. Its enable is not carried: a
/// path it may read would go unchecked by rule 6.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NeedsUnknown {
    /// The value of the setting `key`, on line `line` of the unit file or
    /// drop-in at `path`, names a path for the instance and cannot be read
    /// as systemd reads it: an unbalanced quote, an escape systemd refuses,
    /// a specifier inspectah does not expand, or a configuration path
    /// holding a control character. The value itself is not kept: it can
    /// hold a credential, and a warning is never redacted.
    Uninterpretable {
        key: String,
        path: String,
        line: usize,
    },
    /// A runtime drop-in directory or drop-in at `path` could not be read
    /// in full, or a drop-in there is not text: one of its settings may
    /// name a path.
    Unreadable { path: String, error: String },
}

impl NeedsUnknown {
    /// What the state change records: the same fields, the read error
    /// left to the warning and the scan's degraded reason.
    pub(crate) fn record(&self) -> InstanceNeedsUnknown {
        match self {
            Self::Uninterpretable { key, path, line } => InstanceNeedsUnknown::Uninterpretable {
                key: key.clone(),
                path: path.clone(),
                line: *line,
            },
            Self::Unreadable { path, .. } => {
                InstanceNeedsUnknown::Unreadable { path: path.clone() }
            }
        }
    }
}

/// What the instance `name` needs, read from `fragment`, the path and text
/// of its effective unit file, and the runtime drop-ins systemd applies to it
/// ([`runtime_drop_ins`]), in load order, and the fixed tables for its
/// template. A data path is kept only when `exec` finds something there. A
/// unit that is not an instance needs nothing.
///
/// A drop-in whose reset or replacement removed an assignment that named a
/// path the lists would otherwise hold, or that hides a drop-in of its file
/// name that names one, is recorded in
/// [`InstanceNeeds::overriding_drop_ins`]: the lists hold only while it is
/// there. A removed or hidden assignment that cannot be interpreted, and a
/// hidden drop-in that cannot be read, count, since what they name is
/// unknown. A reset in the unit file itself records nothing: the unit file
/// is rule 5's own evidence.
pub(crate) fn instance_needs(
    exec: &dyn Executor,
    name: &UnitName,
    fragment: Option<(&str, &str)>,
) -> Result<InstanceNeeds, NeedsUnknown> {
    let Some(instance) = name.instance() else {
        return Ok(InstanceNeeds::default());
    };
    let drop_ins = runtime_drop_ins(exec, name)?;
    let template = instance.template();
    let (stem, suffix) = name
        .as_str()
        .rsplit_once('.')
        .expect("a unit name ends in its type suffix");
    let raw_expand = |value: &str| expand_unit_specifiers(value, stem, SpecifierSet::Runtime);
    let expand = |value: &str| raw_expand(value).map(|path| simplify_path(&path));
    let keeps_data = |path: &str| is_instance_data_path(path) && exec.file_exists(Path::new(path));

    let effective = effective_assignments(
        fragment.map(|(_, text)| text),
        &drop_ins.chosen,
        suffix,
        &raw_expand,
    );
    let mut required_config = BTreeSet::new();
    let mut data = BTreeSet::new();
    for assignment in &effective.kept {
        let named = named_paths(&assignment.key, &assignment.value, &expand).map_err(|_| {
            let path = match assignment.source {
                Some(index) => drop_ins.chosen[index].0.as_str(),
                None => fragment.map_or("", |(path, _)| path),
            };
            NeedsUnknown::Uninterpretable {
                key: assignment.key.clone(),
                path: path.to_owned(),
                line: assignment.line,
            }
        })?;
        required_config.extend(named.config);
        data.extend(named.data);
    }
    required_config.extend(
        table_rows(&INSTANCE_CONFIG_TABLE, &template)
            .filter_map(expand)
            .filter(|path| is_instance_config_path(path)),
    );
    data.extend(table_rows(&INSTANCE_DATA_TABLE, &template).filter_map(expand));
    let data_paths: BTreeSet<String> = data.into_iter().filter(|p| keeps_data(p)).collect();

    // Whether an assignment the host's files no longer apply would add to
    // the lists, were it applied again.
    let changes_lists = |key: &str, value: &str| match named_paths(key, value, &expand) {
        Err(Uninterpretable) => true,
        Ok(named) => {
            named.config.iter().any(|p| !required_config.contains(p))
                || named
                    .data
                    .iter()
                    .any(|p| keeps_data(p) && !data_paths.contains(p))
        }
    };
    let mut overriding = BTreeSet::new();
    for (remover, removed) in &effective.removed {
        if changes_lists(&removed.key, &removed.value) {
            overriding.insert(drop_ins.chosen[*remover].0.clone());
        }
    }
    for (hider, hidden) in &drop_ins.hidden {
        let changes = match exec.read_file(Path::new(hidden)) {
            Ok(text) => assignments(&text)
                .iter()
                .any(|a| changes_lists(&a.key, &a.value)),
            Err(_) => true,
        };
        if changes {
            overriding.insert(hider.clone());
        }
    }
    Ok(InstanceNeeds {
        required_config: required_config.into_iter().collect(),
        data_paths: data_paths.into_iter().collect(),
        overriding_drop_ins: overriding.into_iter().collect(),
    })
}

/// The paths for the instance one assignment names, expanded: the
/// configuration paths under `/etc`, and the other paths, data candidates
/// for [`instance_needs`] to keep when something is there.
#[derive(Debug, Default)]
struct NamedPaths {
    config: Vec<String>,
    data: Vec<String>,
}

/// The paths for the instance the assignment `key=value` names, read with
/// the setting's [`Syntax`] and expanded with `expand`. A value systemd's
/// parser refuses names nothing unless it holds `%i` or `%I`.
/// [`Uninterpretable`] for such a value, an operand `expand` cannot expand,
/// and a configuration path the form check refuses (a control character):
/// a configuration path is never dropped.
fn named_paths(
    key: &str,
    value: &str,
    expand: &dyn Fn(&str) -> Option<String>,
) -> Result<NamedPaths, Uninterpretable> {
    let mut named = NamedPaths::default();
    let operands = match operands(key, value) {
        Ok(operands) => operands,
        Err(Uninterpretable) if holds_instance_specifier(value) => return Err(Uninterpretable),
        Err(Uninterpretable) => return Ok(named),
    };
    for operand in operands {
        let (text, state_directory) = match &operand {
            Operand::Path(text) => (text, false),
            Operand::StateDirectory(text) => (text, true),
        };
        if !holds_instance_specifier(text) {
            continue;
        }
        let expanded = expand(text).ok_or(Uninterpretable)?;
        if state_directory {
            if !expanded.starts_with('/') {
                named
                    .data
                    .push(format!("{STATE_DIRECTORY_ROOT}/{expanded}"));
            }
            continue;
        }
        if expanded.starts_with(&format!("{CONFIG_ROOT}/")) {
            if !is_instance_config_path(&expanded) {
                return Err(Uninterpretable);
            }
            named.config.push(expanded);
        } else {
            named.data.push(expanded);
        }
    }
    Ok(named)
}

/// The runtime drop-ins of a unit ([`runtime_drop_ins`]).
#[derive(Debug, Default)]
struct RuntimeDropIns {
    /// The path and text of each drop-in systemd applies, in load order.
    chosen: Vec<(String, String)>,
    /// Each drop-in systemd does not read because a chosen one has its file
    /// name: the chosen one's path, then the hidden one's.
    hidden: Vec<(String, String)>,
}

/// The texts of the drop-ins systemd applies to `name` when it loads the
/// unit, in load order, as `unit_file_find_dropin_paths` finds them
/// (systemd 252 and 257, `src/shared/dropin.c`, identical in both): for
/// each directory of the search path ([`unit_search_path`]), the `.d`
/// directories of the unit's names ([`named_drop_in_dirs`]); then, in each
/// directory of the search path, the type-wide `<type>.d`. Of the drop-ins
/// that share a file name, the first in that list of directories is read
/// (`conf_files_list_strv`), and they are applied in file-name order. A
/// hidden name is skipped ([`systemd_reads_drop_in`]).
///
/// This is the runtime selection, separate from the `[Install]` reader's
/// (`install::selected_drop_ins`), which reads only the own and template
/// `.d` in its own order: `enable` does not read `[Install]` from a
/// type-wide or dash-prefix directory. A unit's aliases' directories are
/// not read: an alias of a template instance is not modeled.
///
/// A directory listing that fails in any way but `NotFound` (an iteration
/// error included, through `read_dir_strict`), and a chosen drop-in that
/// cannot be read as text, are [`NeedsUnknown::Unreadable`]: systemd reads
/// what the collector cannot. A hidden drop-in is listed, not read.
fn runtime_drop_ins(exec: &dyn Executor, name: &UnitName) -> Result<RuntimeDropIns, NeedsUnknown> {
    let unreadable = |path: &str, error: String| NeedsUnknown::Unreadable {
        path: path.to_owned(),
        error,
    };
    let mut named = Vec::new();
    named_drop_in_dirs(name.as_str(), &mut named);
    let (_, suffix) = name
        .as_str()
        .rsplit_once('.')
        .expect("a unit name ends in its type suffix");
    let type_wide = format!("{suffix}{DROP_IN_DIR_SUFFIX}");
    let dirs: Vec<String> = unit_search_path()
        .flat_map(|root| named.iter().map(move |dir| format!("{root}/{dir}")))
        .chain(unit_search_path().map(|root| format!("{root}/{type_wide}")))
        .collect();

    let mut chosen: BTreeMap<String, String> = BTreeMap::new();
    let mut hidden = Vec::new();
    for dir in &dirs {
        let entries = match exec.read_dir_strict(Path::new(dir)) {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => return Err(unreadable(dir, e.to_string())),
        };
        for entry in entries.into_iter().filter(|e| systemd_reads_drop_in(e)) {
            let path = format!("{dir}/{entry}");
            match chosen.get(&entry) {
                Some(hider) => hidden.push((hider.clone(), path)),
                None => {
                    chosen.insert(entry, path);
                }
            }
        }
    }
    let chosen = chosen
        .into_values()
        .map(|path| match exec.read_file(Path::new(&path)) {
            Ok(text) => Ok((path, text)),
            Err(e) => Err(unreadable(&path, e.to_string())),
        })
        .collect::<Result<_, _>>()?;
    Ok(RuntimeDropIns { chosen, hidden })
}

/// Add the `.d` directory names systemd reads for the unit `name` to
/// `dirs`, as `unit_file_find_dirs` does (systemd 252 and 257,
/// `src/shared/dropin.c`): its own; for an instance, its template's and
/// what that gives; then, for a name with a `-` in its prefix, the name cut
/// after its last `-` (`a-b-c.service` gives `a-b-.service`, which gives
/// `a-.service`), keeping the instance (`a-b@x.service` gives
/// `a-@x.service`), and what that gives in turn.
fn named_drop_in_dirs(name: &str, dirs: &mut Vec<String>) {
    let dir = format!("{name}{DROP_IN_DIR_SUFFIX}");
    if dirs.contains(&dir) {
        return;
    }
    dirs.push(dir);
    let Some((stem, suffix)) = name.rsplit_once('.') else {
        return;
    };
    let (prefix, instance) = match stem.split_once('@') {
        Some((prefix, instance)) => (prefix, Some(instance).filter(|i| !i.is_empty())),
        None => (stem, None),
    };
    if instance.is_some() {
        named_drop_in_dirs(&format!("{prefix}@.{suffix}"), dirs);
    }
    // A trailing `-` is cut once before the next one is looked for, so
    // `a-.service` gives `a`, which has none.
    let mut prefix = prefix.to_owned();
    let mut chopped = false;
    loop {
        let Some(dash) = prefix.rfind('-') else {
            return;
        };
        if dash == 0 {
            return;
        }
        if dash + 1 < prefix.len() || chopped {
            prefix.truncate(dash + 1);
            break;
        }
        prefix.truncate(dash);
        chopped = true;
    }
    let cut = match instance {
        Some(instance) => format!("{prefix}@{instance}.{suffix}"),
        None => format!("{prefix}.{suffix}"),
    };
    named_drop_in_dirs(&cut, dirs);
}

/// `path` with each run of `/` made one, each `.` component dropped and a
/// trailing `/` dropped, as systemd simplifies a path it loads, so
/// `/var/lib/x/%i/` and `/etc/x/./%i` name what they read as and are not
/// refused by the form check. A `..` component is kept, as systemd keeps
/// it: the form check refuses it in a data path, and keeps it in a
/// configuration path so rule 6 declines the enable naming it. Only the
/// collector simplifies: the snapshot loader keeps the strict form.
fn simplify_path(path: &str) -> String {
    let absolute = path.starts_with('/');
    let parts: Vec<&str> = path
        .split('/')
        .filter(|part| !matches!(*part, "" | "."))
        .collect();
    let joined = parts.join("/");
    if absolute {
        format!("/{joined}")
    } else {
        joined
    }
}

/// The unexpanded paths `table` gives `template`.
fn table_rows<'t>(
    table: &'t [(&str, &'static str)],
    template: &'t UnitName,
) -> impl Iterator<Item = &'static str> + 't {
    table
        .iter()
        .filter(move |(name, _)| *name == template.as_str())
        .map(|(_, path)| *path)
}

/// The assignments of one unit file text outside `[Install]`, by logical
/// line, key and value trimmed, each with the section it is in and the
/// line it starts on. Each source is `None`, for the caller to set.
fn assignments(text: &str) -> Vec<Assignment> {
    let mut found = Vec::new();
    let mut section: Option<String> = None;
    for (number, line) in numbered_logical_lines(text) {
        let line = line.trim_matches(UNIT_FILE_WHITESPACE);
        if line.starts_with('[') && line.ends_with(']') {
            section = Some(line[1..line.len() - 1].to_owned());
            continue;
        }
        if section.as_deref() == Some(INSTALL_SECTION) {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            found.push(Assignment {
                source: None,
                line: number,
                section: section.clone(),
                key: key.trim_matches(UNIT_FILE_WHITESPACE).to_owned(),
                value: value.trim_matches(UNIT_FILE_WHITESPACE).to_owned(),
            });
        }
    }
    found
}

/// One assignment outside `[Install]`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Assignment {
    /// The index of the drop-in it is in, in the list
    /// [`effective_assignments`] was given; `None` for the unit file.
    source: Option<usize>,
    /// The line of its file it starts on, counted from 1.
    line: usize,
    /// The section it is in; `None` before the first section header.
    section: Option<String>,
    key: String,
    value: String,
}

/// What [`effective_assignments`] finds.
#[derive(Debug, Default)]
struct Effective {
    /// The assignments in effect, in load order.
    kept: Vec<Assignment>,
    /// Each assignment a drop-in's reset or replacement removed, with the
    /// index of that drop-in. One the unit file removed is not listed.
    removed: Vec<(usize, Assignment)>,
}

/// The assignments that are in effect once `fragment`, the unit file's
/// text, then each of `drop_ins` (path, text) in load order is applied
/// with systemd's resets, in order, for a unit of the type `suffix`
/// (`service`) whose values `expand` expands. An empty value resets the
/// values before it ([`Syntax::resets`]) and is not kept; a single-path
/// setting keeps only its last value. A reset or a replacement removes
/// anything only when systemd applies the assignment ([`applies`]): one
/// in a section that does not take its key, or a replacement systemd
/// refuses, leaves the earlier values in place, as systemd does. A setting
/// whose syntax is not modeled keeps every value, an empty one dropped: if
/// systemd resets it, a path is required that the unit no longer reads,
/// which omits an enable rather than carry one. So does an assignment
/// systemd ignores that adds a value.
fn effective_assignments(
    fragment: Option<&str>,
    drop_ins: &[(String, String)],
    suffix: &str,
    expand: &dyn Fn(&str) -> Option<String>,
) -> Effective {
    let texts = fragment.map(|text| (None, text)).into_iter().chain(
        drop_ins
            .iter()
            .enumerate()
            .map(|(index, (_, text))| (Some(index), text.as_str())),
    );
    let mut effective = Effective::default();
    for (source, text) in texts {
        for mut assignment in assignments(text) {
            assignment.source = source;
            let syntax = Syntax::of(&assignment.key);
            let replaces = syntax == Syntax::SinglePath;
            if (assignment.value.is_empty() || replaces) && applies(&assignment, suffix, expand) {
                let (removed, kept) = std::mem::take(&mut effective.kept)
                    .into_iter()
                    .partition(|earlier: &Assignment| syntax.resets(&assignment.key, &earlier.key));
                effective.kept = kept;
                if let Some(remover) = source {
                    effective
                        .removed
                        .extend(removed.into_iter().map(|earlier| (remover, earlier)));
                }
            }
            if !assignment.value.is_empty() {
                effective.kept.push(assignment);
            }
        }
    }
    effective
}

/// Whether systemd applies `assignment`, a reset or a replacement in a
/// unit of the type `suffix`, so that it removes the values before it.
/// systemd looks a setting up by its section and key together and ignores
/// one its table does not hold (`conf-parser.c`, 252 and 257): so the
/// assignment's section must take its key ([`section_takes`]). A
/// replacement must then be one its parser accepts: each single-path
/// parser checks the new value before it frees the old one
/// (`load-fragment.c`, 252 and 257: `config_parse_working_directory`,
/// `config_parse_unit_path_printf` with `config_parse_path`,
/// `config_parse_pid_file`), and on a refusal keeps the old value, or
/// fails the unit's load ([`single_path_accepted`]). Where acceptance
/// cannot be told (a specifier `expand` does not expand) it is not
/// applied, so the earlier path stays required.
fn applies(assignment: &Assignment, suffix: &str, expand: &dyn Fn(&str) -> Option<String>) -> bool {
    let Some(section) = assignment.section.as_deref() else {
        return false;
    };
    if !section_takes(section, &assignment.key, suffix) {
        return false;
    }
    assignment.value.is_empty() || single_path_accepted(&assignment.key, &assignment.value, expand)
}

/// Whether `section` of a unit of the type `suffix` takes the modeled
/// setting `key` (systemd 252 and 257, `load-fragment-gperf.gperf.in`). A
/// setting whose syntax is not modeled is taken nowhere: it resets nothing
/// here.
fn section_takes(section: &str, key: &str, suffix: &str) -> bool {
    let own_section = EXEC_SECTIONS
        .iter()
        .any(|(kind, own)| *kind == suffix && *own == section);
    match Syntax::of(key) {
        Syntax::Condition => section == UNIT_SECTION && CONDITION_KEYS.contains(&key),
        Syntax::Command => {
            own_section
                && match suffix {
                    SERVICE_SUFFIX => SERVICE_COMMAND_KEYS.contains(&key),
                    SOCKET_SUFFIX => SOCKET_COMMAND_KEYS.contains(&key),
                    _ => false,
                }
        }
        Syntax::SinglePath if key == PID_FILE_KEY => own_section && suffix == SERVICE_SUFFIX,
        Syntax::EnvironmentFile
        | Syntax::PathList
        | Syntax::Bind
        | Syntax::StateDirectory
        | Syntax::SinglePath => own_section,
        Syntax::Other => false,
    }
}

/// Whether the parser of the single-path setting `key` accepts the
/// non-empty `value`, so that it replaces the earlier value (systemd 252
/// and 257): `WorkingDirectory=` takes `~`, or, after an optional `-`, a
/// path that expands to an absolute, valid and normalized one;
/// `RootDirectory=` and `RootImage=` take an absolute, valid and
/// normalized path, with no `-`; `PIDFile=` takes any path, a relative one
/// made absolute under `/run`, that is valid and normalized and, in 257,
/// not under an API file system. Each first expands the value with
/// [`unit_path_printf`], whose length limit is checked before anything
/// simplifies or prefixes it, so `expand` must not simplify. A key that is
/// no single-path setting is not checked here: only its empty value resets
/// anything.
fn single_path_accepted(key: &str, value: &str, expand: &dyn Fn(&str) -> Option<String>) -> bool {
    if Syntax::of(key) != Syntax::SinglePath {
        return true;
    }
    if key == PID_FILE_KEY {
        return unit_path_printf(value, expand).is_some_and(|path| {
            let path = if path.starts_with('/') {
                path
            } else {
                format!("{PID_FILE_RUNTIME_DIR}/{path}")
            };
            systemd_accepts_path(&path)
                && !API_VFS_ROOTS
                    .iter()
                    .any(|root| path_is_at_or_under(&simplify_path(&path), root))
        });
    }
    let value = if key == WORKING_DIRECTORY_KEY {
        let value = value
            .strip_prefix(SINGLE_PATH_OPTIONAL_PREFIX)
            .unwrap_or(value);
        if value == HOME_DIRECTORY {
            return true;
        }
        value
    } else {
        value
    };
    unit_path_printf(value, expand).is_some_and(|path| systemd_accepts_path(&path))
}

/// `value` expanded with `expand` as systemd's `unit_path_printf` expands a
/// single-path setting (252 and 257, `src/core/unit-printf.h`: the
/// expansion limited to `PATH_MAX - 1` bytes, which `specifier_printf` in
/// `src/shared/specifier.c` refuses with `ENAMETOOLONG`). `None` when
/// `expand` cannot expand it or the expansion is longer than
/// [`PATH_MAX_LENGTH`]. The limit applies to the expansion as written,
/// before the parser simplifies it or makes a relative `PIDFile=` absolute,
/// so `/` followed by 2048 `./` and `srv` is refused although it simplifies
/// to `/srv`. On that refusal the parser keeps the earlier value (an
/// optional `WorkingDirectory=`, `PIDFile=`) or fails the unit's load
/// (`load-fragment.c`); either way the earlier assignment stays required.
fn unit_path_printf(value: &str, expand: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    expand(value).filter(|path| path.len() <= PATH_MAX_LENGTH)
}

/// Whether systemd's `path_simplify_and_warn` with `PATH_CHECK_ABSOLUTE`
/// accepts the expanded `path`, which has passed [`unit_path_printf`]
/// already (systemd 252 `src/shared/parse-helpers.c`,
/// 257 the same file): absolute before it is simplified, then, simplified,
/// no longer than `PATH_MAX` with no component longer than `NAME_MAX`
/// (`path_is_valid`) and with no `..` component (`path_is_normalized`,
/// which simplifying cannot satisfy otherwise).
fn systemd_accepts_path(path: &str) -> bool {
    if !path.starts_with('/') {
        return false;
    }
    let simple = simplify_path(path);
    simple.len() <= PATH_MAX_LENGTH
        && simple
            .split('/')
            .all(|part| part.len() <= NAME_MAX_LENGTH && part != "..")
}

/// Whether the absolute, simplified `path` is `root` or under it, by whole
/// components.
fn path_is_at_or_under(path: &str, root: &str) -> bool {
    path.strip_prefix(root)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// How systemd reads one setting's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Syntax {
    /// A command line ([`COMMAND_KEYS`]): words, quotes and C escapes
    /// removed; the first word of each command is the executable after its
    /// prefixes.
    Command,
    /// `EnvironmentFile=`: the whole value is one path; `-` makes it
    /// optional.
    EnvironmentFile,
    /// A condition or assertion: the whole value, after `|` and `!`.
    Condition,
    /// A list of paths ([`PATH_LIST_KEYS`]): words with quotes removed.
    PathList,
    /// A list of bind mounts ([`BIND_KEYS`]).
    Bind,
    /// `StateDirectory=`: words, each a directory and an optional link.
    StateDirectory,
    /// A setting whose whole value is one path ([`SINGLE_PATH_KEYS`]).
    SinglePath,
    /// Any other setting: words read leniently, and a path operand looked
    /// for in each ([`operand_in_word`]). Its value never declines the
    /// enable for being unreadable, since its syntax is not known.
    Other,
}

impl Syntax {
    /// Whether an empty assignment of `key`, a setting of this syntax,
    /// clears the earlier assignment of `earlier` (systemd 252 and 257,
    /// `load-fragment.c`, each parser's `isempty(rvalue)` branch): a
    /// command, `EnvironmentFile=`, a path list, `StateDirectory=` and a
    /// single-path setting clear their own key; an empty `BindPaths=` or
    /// `BindReadOnlyPaths=` clears both, which share one list; an empty
    /// condition clears every condition, and an empty assertion every
    /// assertion. A setting whose syntax is not modeled clears nothing.
    fn resets(self, key: &str, earlier: &str) -> bool {
        match self {
            Self::Command
            | Self::EnvironmentFile
            | Self::PathList
            | Self::StateDirectory
            | Self::SinglePath => earlier == key,
            Self::Bind => BIND_KEYS.contains(&earlier),
            Self::Condition => CONDITION_KEY_PREFIXES
                .iter()
                .any(|prefix| key.starts_with(prefix) && earlier.starts_with(prefix)),
            Self::Other => false,
        }
    }

    /// The syntax of the setting `key`.
    fn of(key: &str) -> Self {
        if COMMAND_KEYS.contains(&key) {
            Self::Command
        } else if key == ENVIRONMENT_FILE_KEY {
            Self::EnvironmentFile
        } else if CONDITION_KEY_PREFIXES
            .iter()
            .any(|prefix| key.starts_with(prefix))
        {
            Self::Condition
        } else if PATH_LIST_KEYS.contains(&key) {
            Self::PathList
        } else if BIND_KEYS.contains(&key) {
            Self::Bind
        } else if key == STATE_DIRECTORY_KEY {
            Self::StateDirectory
        } else if SINGLE_PATH_KEYS.contains(&key) {
            Self::SinglePath
        } else {
            Self::Other
        }
    }
}

/// A value one assignment names that may be a path for the instance,
/// unexpanded.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Operand {
    /// An absolute path once expanded, from any key outside `[Install]`.
    Path(String),
    /// A `StateDirectory=` value, relative to [`STATE_DIRECTORY_ROOT`].
    StateDirectory(String),
}

/// A value systemd's parser for its setting would refuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Uninterpretable;

/// The operands the assignment `key=value` names, read with the setting's
/// [`Syntax`]. An optional `EnvironmentFile=` and a negated condition give
/// none.
fn operands(key: &str, value: &str) -> Result<Vec<Operand>, Uninterpretable> {
    let path = |text: &str| Operand::Path(text.to_owned());
    Ok(match Syntax::of(key) {
        Syntax::EnvironmentFile => {
            if value.starts_with(OPTIONAL_FILE_PREFIX) {
                Vec::new()
            } else {
                vec![path(value)]
            }
        }
        Syntax::Condition => {
            let value = match value.strip_prefix(TRIGGER_PREFIX) {
                Some(rest) => rest.trim_start_matches(WORD_SEPARATORS),
                None => value,
            };
            if value.starts_with(NEGATION_PREFIX) {
                Vec::new()
            } else {
                vec![path(value)]
            }
        }
        Syntax::SinglePath => vec![path(
            value
                .strip_prefix(SINGLE_PATH_OPTIONAL_PREFIX)
                .unwrap_or(value),
        )],
        Syntax::Command => command_operands(value)?,
        Syntax::PathList => words(value, Extract::default())?
            .iter()
            .map(|word| {
                let word = word.strip_prefix(PATH_LIST_PREFIXES[0]).unwrap_or(word);
                path(word.strip_prefix(PATH_LIST_PREFIXES[1]).unwrap_or(word))
            })
            .collect(),
        Syntax::Bind => bind_sources(value)?.iter().map(|s| path(s)).collect(),
        Syntax::StateDirectory => {
            let mut directories = Vec::new();
            for tuple in words(
                value,
                Extract {
                    retain_escape: true,
                    ..Extract::default()
                },
            )? {
                let mut rest = tuple.as_str();
                let source = extract_first_word(
                    &mut rest,
                    &[STATE_DIRECTORY_LINK_SEPARATOR],
                    Extract {
                        cunescape: true,
                        unescape_separators: true,
                        ..Extract::default()
                    },
                )?;
                if let Some((source, _)) = source {
                    directories.push(Operand::StateDirectory(source));
                }
            }
            directories
        }
        Syntax::Other => words(
            value,
            Extract {
                cunescape: true,
                relax: true,
                ..Extract::default()
            },
        )?
        .iter()
        .filter_map(|word| operand_in_word(word).map(path))
        .collect(),
    })
}

/// The operands of a command line, read as `config_parse_exec` reads it
/// (systemd 252 and 257, `src/core/load-fragment.c`): words with quotes
/// and C escapes removed before specifiers are expanded, so
/// `--config="/etc/a b/%i.conf"` is one word holding one path. A lone `;`
/// separates commands and `\;` is a literal `;` argument. The executable of
/// each command is an operand after its prefixes; each argument gives the
/// operand [`operand_in_word`] finds.
fn command_operands(value: &str) -> Result<Vec<Operand>, Uninterpretable> {
    let flags = Extract {
        cunescape: true,
        ..Extract::default()
    };
    let separator_word = |rest: &str, word: &str| {
        rest.strip_prefix(word)
            .is_some_and(|after| after.is_empty() || after.starts_with(WORD_SEPARATORS))
    };
    let separator = COMMAND_SEPARATOR.to_string();
    let escaped_separator = format!("\\{COMMAND_SEPARATOR}");
    let mut operands = Vec::new();
    let mut rest = value;
    let mut executable = true;
    loop {
        rest = rest.trim_start_matches(WORD_SEPARATORS);
        if separator_word(rest, &separator) {
            rest = &rest[separator.len()..];
            executable = true;
            continue;
        }
        if !executable && separator_word(rest, &escaped_separator) {
            rest = &rest[escaped_separator.len()..];
            continue;
        }
        let Some((word, _)) = extract_first_word(&mut rest, WORD_SEPARATORS, flags)? else {
            break;
        };
        if executable {
            operands.push(Operand::Path(
                word.trim_start_matches(COMMAND_PREFIXES).to_owned(),
            ));
            executable = false;
        } else if let Some(operand) = operand_in_word(&word) {
            operands.push(Operand::Path(operand.to_owned()));
        }
    }
    Ok(operands)
}

/// The sources of a bind-mount list, read as `config_parse_bind_paths`
/// reads it: each entry's parts end at `:` or whitespace, quotes removed,
/// and an optional source loses its `-`.
fn bind_sources(value: &str) -> Result<Vec<String>, Uninterpretable> {
    let separators: Vec<char> = std::iter::once(BIND_PART_SEPARATOR)
        .chain(WORD_SEPARATORS.iter().copied())
        .collect();
    let flags = Extract {
        dont_coalesce: true,
        ..Extract::default()
    };
    let mut sources = Vec::new();
    let mut rest = value;
    while let Some((source, end)) = extract_first_word(&mut rest, &separators, flags)? {
        if !source.is_empty() {
            let source = source.strip_prefix(BIND_OPTIONAL_PREFIX).unwrap_or(&source);
            sources.push(source.to_owned());
        }
        // The destination, then the options, belong to this entry.
        if end == Some(BIND_PART_SEPARATOR)
            && let Some((_, Some(BIND_PART_SEPARATOR))) =
                extract_first_word(&mut rest, &separators, flags)?
        {
            extract_first_word(&mut rest, WORD_SEPARATORS, Extract::default())?;
        }
    }
    Ok(sources)
}

/// The words of `value`, separated by whitespace, read with `flags`.
fn words(value: &str, flags: Extract) -> Result<Vec<String>, Uninterpretable> {
    let mut rest = value;
    let mut words = Vec::new();
    while let Some((word, _)) = extract_first_word(&mut rest, WORD_SEPARATORS, flags)? {
        words.push(word);
    }
    Ok(words)
}

/// The path operand inside one word of a command or of a setting whose
/// syntax is not modeled: the whole word when it starts with a path, else
/// the rest of the word after the first `=`, `:` or `,` that is followed by
/// a path. A path is never split: `/etc/a:b/%i.conf` is one operand.
fn operand_in_word(word: &str) -> Option<&str> {
    if starts_a_path(word) {
        return Some(word);
    }
    word.char_indices()
        .filter(|(_, c)| OPERAND_SEPARATORS.contains(c))
        .map(|(at, c)| &word[at + c.len_utf8()..])
        .find(|rest| starts_a_path(rest))
}

/// Whether `text` starts with an absolute path: `/`, or a specifier for a
/// directory followed by `/` (`%E/x`, `%h/x`), which systemd expands to an
/// absolute path.
fn starts_a_path(text: &str) -> bool {
    let mut chars = text.chars();
    match (chars.next(), chars.next(), chars.next()) {
        (Some('/'), _, _) => true,
        (Some('%'), Some(specifier), Some('/')) => specifier.is_ascii_alphabetic(),
        _ => false,
    }
}

/// The flags of systemd's `extract_first_word` a setting's parser passes,
/// besides `EXTRACT_UNQUOTE`, which every caller here passes.
#[derive(Debug, Clone, Copy, Default)]
struct Extract {
    /// `EXTRACT_CUNESCAPE`: a backslash starts a C escape
    /// ([`cunescape_one`]); without it, a backslash keeps the character
    /// after it as written.
    cunescape: bool,
    /// `EXTRACT_RETAIN_ESCAPE`: a backslash is an ordinary character.
    retain_escape: bool,
    /// `EXTRACT_DONT_COALESCE_SEPARATORS`: each separator ends one word, so
    /// two in a row give an empty word.
    dont_coalesce: bool,
    /// `EXTRACT_UNESCAPE_SEPARATORS`: a backslash before a separator or a
    /// backslash keeps that character where `cunescape` refuses it.
    unescape_separators: bool,
    /// Read what systemd would refuse as written instead of failing: for a
    /// setting whose syntax is not modeled ([`Syntax::Other`]).
    relax: bool,
}

/// The first word of `rest` and the separator that ended it (`None` at the
/// end), as systemd's `extract_first_word` with `EXTRACT_UNQUOTE` returns
/// it (systemd 252 and 257, `src/basic/extract-word.c`, identical in both):
/// leading separators skipped, a quote opened anywhere in a word and
/// removed, separators inside quotes kept, and a backslash read by
/// `flags`. `rest` is left after the word. `None` when no word is left;
/// [`Uninterpretable`] for an unbalanced quote, a trailing backslash or an
/// escape systemd refuses, unless `flags.relax`.
fn extract_first_word(
    rest: &mut &str,
    separators: &[char],
    flags: Extract,
) -> Result<Option<(String, Option<char>)>, Uninterpretable> {
    let input = *rest;
    let mut at = 0;
    // Leading separators.
    while let Some(c) = input[at..].chars().next() {
        if !separators.contains(&c) {
            break;
        }
        at += c.len_utf8();
        if flags.dont_coalesce {
            *rest = &input[at..];
            return Ok(Some((String::new(), Some(c))));
        }
    }
    if at == input.len() {
        *rest = "";
        return Ok(None);
    }
    let mut word = String::new();
    let mut quote: Option<char> = None;
    while let Some(c) = input[at..].chars().next() {
        at += c.len_utf8();
        if c == '\\' && !flags.retain_escape {
            let escaped = &input[at..];
            let Some(next) = escaped.chars().next() else {
                if flags.relax {
                    word.push(c);
                    break;
                }
                return Err(Uninterpretable);
            };
            if !flags.cunescape {
                word.push(next);
                at += next.len_utf8();
            } else if let Some((unescaped, length)) = cunescape_one(escaped) {
                word.push(unescaped);
                at += length;
            } else if flags.unescape_separators && (separators.contains(&next) || next == '\\') {
                word.push(next);
                at += next.len_utf8();
            } else if flags.relax {
                word.push(c);
                word.push(next);
                at += next.len_utf8();
            } else {
                return Err(Uninterpretable);
            }
            continue;
        }
        match quote {
            Some(open) if c == open => quote = None,
            Some(_) => word.push(c),
            None if QUOTES.contains(&c) => quote = Some(c),
            None if separators.contains(&c) => {
                if !flags.dont_coalesce {
                    at += input[at..].len() - input[at..].trim_start_matches(separators).len();
                }
                *rest = &input[at..];
                return Ok(Some((word, Some(c))));
            }
            None => word.push(c),
        }
    }
    if quote.is_some() && !flags.relax {
        return Err(Uninterpretable);
    }
    *rest = "";
    Ok(Some((word, None)))
}

/// The C escape `escaped` starts with (the text after a backslash) and its
/// length in bytes, as systemd's `cunescape_one` reads it (252 and 257,
/// `src/basic/escape.c`): the single-letter escapes, `\s` for a space,
/// `\xNN`, three octal digits, `\uNNNN` and `\UNNNNNNNN`, none of them
/// zero. `None` for anything else, and for a byte escape above ASCII, which
/// systemd copies as a raw byte that is not UTF-8 text.
fn cunescape_one(escaped: &str) -> Option<(char, usize)> {
    let digits = |count: usize, radix: u32| -> Option<u32> {
        let text = escaped.get(1..=count)?;
        if !text.chars().all(|d| d.is_digit(radix)) {
            return None;
        }
        u32::from_str_radix(text, radix).ok().filter(|v| *v != 0)
    };
    let byte = |value: u32| char::from_u32(value).filter(char::is_ascii);
    let single = match escaped.chars().next()? {
        'a' => '\u{07}',
        'b' => '\u{08}',
        'f' => '\u{0c}',
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        'v' => '\u{0b}',
        '\\' => '\\',
        '"' => '"',
        '\'' => '\'',
        's' => ' ',
        'x' => return Some((byte(digits(2, 16)?)?, 3)),
        'u' => return Some((char::from_u32(digits(4, 16)?)?, 5)),
        'U' => return Some((char::from_u32(digits(8, 16)?)?, 9)),
        '0'..='7' => {
            let text = escaped.get(..3)?;
            if !text.chars().all(|d| d.is_digit(8)) {
                return None;
            }
            let value = u32::from_str_radix(text, 8).ok().filter(|v| *v != 0)?;
            return Some((byte(value)?, 3));
        }
        _ => return None,
    };
    Some((single, 1))
}

/// Whether `value` holds the specifier `%i` or `%I`; `%%` is a literal
/// percent sign and starts none.
fn holds_instance_specifier(value: &str) -> bool {
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('i' | 'I') => return true,
                _ => continue,
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The unexpanded paths and state directories one text names that
    /// hold `%i` or `%I`, in order.
    fn named(text: &str) -> (Vec<String>, Vec<String>) {
        let mut paths = Vec::new();
        let mut state = Vec::new();
        for Assignment { key, value, .. } in assignments(text) {
            for operand in operands(&key, &value).expect("an interpretable value") {
                match operand {
                    Operand::Path(p) if holds_instance_specifier(&p) => paths.push(p),
                    Operand::StateDirectory(d) if holds_instance_specifier(&d) => state.push(d),
                    _ => {}
                }
            }
        }
        (paths, state)
    }

    #[test]
    fn each_kind_of_value_is_read_with_its_syntax() {
        for (text, paths, state) in [
            (
                "[Service]\nEnvironmentFile=/etc/x/%i.env\n",
                vec!["/etc/x/%i.env"],
                vec![],
            ),
            (
                "[Service]\nEnvironmentFile=-/etc/x/%i.env\n",
                vec![],
                vec![],
            ),
            (
                "[Service]\nEnvironmentFile=/etc/sysconfig/x\n",
                vec![],
                vec![],
            ),
            (
                "[Service]\nEnvironmentFile=/etc/a b/%i.env\n",
                vec!["/etc/a b/%i.env"],
                vec![],
            ),
            (
                "[Service]\nExecStart=-/usr/bin/x --config=/etc/x/%I.conf \"/var/lib/x/%i\"\n",
                vec!["/etc/x/%I.conf", "/var/lib/x/%i"],
                vec![],
            ),
            (
                "[Service]\nExecStart=/usr/bin/x --config=\"/etc/a b/%i.conf\" '/etc/c d/%i'\n",
                vec!["/etc/a b/%i.conf", "/etc/c d/%i"],
                vec![],
            ),
            (
                "[Service]\nExecStart=/usr/bin/x /etc/a:b/%i.conf x:/etc/c,d/%i\n",
                vec!["/etc/a:b/%i.conf", "/etc/c,d/%i"],
                vec![],
            ),
            (
                "[Service]\nExecStart=/usr/bin/x /etc/a\\x20b/%i ; /etc/%i/run \\; /etc/c/%i\n",
                vec!["/etc/a b/%i", "/etc/%i/run", "/etc/c/%i"],
                vec![],
            ),
            (
                "[Service]\nExecStart=/usr/bin/x %E/x/%i.conf --dir=%S/x/%i\n",
                vec!["%E/x/%i.conf", "%S/x/%i"],
                vec![],
            ),
            (
                "[Service]\nBindPaths=/var/lib/x/%i:/data -/etc/y/%i:/y:rbind\n",
                vec!["/var/lib/x/%i", "/etc/y/%i"],
                vec![],
            ),
            (
                "[Service]\nBindReadOnlyPaths=\"/etc/a:b/%i\":/z\n",
                vec!["/etc/a:b/%i"],
                vec![],
            ),
            ("[Unit]\nConditionPathExists=!/etc/x/%i\n", vec![], vec![]),
            (
                "[Unit]\nConditionPathExists=|/etc/x/%i\n",
                vec!["/etc/x/%i"],
                vec![],
            ),
            (
                "[Unit]\nAssertPathExists=| !/etc/x/%i\nConditionPathExists=/etc/a b/%i\n",
                vec!["/etc/a b/%i"],
                vec![],
            ),
            ("[Service]\nExecStart=/usr/bin/x %i\n", vec![], vec![]),
            (
                "[Service]\nExecStart=/usr/bin/x /etc/x/%%i\n",
                vec![],
                vec![],
            ),
            (
                "[Service]\nStateDirectory=x/%i y z/%I:link \"w v/%i\"\n",
                vec![],
                vec!["x/%i", "z/%I", "w v/%i"],
            ),
            ("[Install]\nAlias=/etc/x/%i\n", vec![], vec![]),
            (
                "[Service]\nExecStart=/usr/bin/x \\\n  /etc/x/%i.conf\n",
                vec!["/etc/x/%i.conf"],
                vec![],
            ),
            (
                "[Service]\nReadWritePaths=/var/lib/x/%i/ -/etc//x/%i +\"/etc/y z/%i\"\n",
                vec!["/var/lib/x/%i/", "/etc//x/%i", "/etc/y z/%i"],
                vec![],
            ),
            (
                "[Service]\nWorkingDirectory=-/etc/w d/%i\n",
                vec!["/etc/w d/%i"],
                vec![],
            ),
            ("[Unit]\nDescription=%I's tunnel on %H\n", vec![], vec![]),
        ] {
            let (found_paths, found_state) = named(text);
            assert_eq!(found_paths, paths, "{text:?}");
            assert_eq!(found_state, state, "{text:?}");
        }
    }

    /// The values systemd's parser refuses are uninterpretable; a setting
    /// whose syntax is not modeled is read leniently and never is.
    #[test]
    fn a_value_systemd_refuses_is_uninterpretable() {
        for (key, value) in [
            ("ExecStart", "/usr/bin/x \"/etc/x/%i"),
            ("ExecStart", "/usr/bin/x /etc/x\\q/%i"),
            ("ExecStart", "/usr/bin/x /etc/x/%i\\"),
            ("ExecStart", "/usr/bin/x /etc/\\x00/%i"),
            ("ExecStart", "/usr/bin/x /etc/\\xff/%i"),
            ("ReadOnlyPaths", "'/etc/x/%i"),
            ("BindPaths", "\"/etc/x/%i:/y"),
            ("StateDirectory", "\"x/%i"),
        ] {
            assert_eq!(operands(key, value), Err(Uninterpretable), "{key}={value}");
        }
        for (key, value) in [("Description", "it's \"%i"), ("Environment", "A=\\q")] {
            assert!(operands(key, value).is_ok(), "{key}={value}");
        }
    }

    #[test]
    fn c_escapes_are_read_as_systemd_reads_them() {
        for (escaped, expected) in [
            ("n", Some(('\n', 1))),
            ("s", Some((' ', 1))),
            ("\\", Some(('\\', 1))),
            ("x41", Some(('A', 3))),
            ("101", Some(('A', 3))),
            ("u00e9", Some(('\u{e9}', 5))),
            ("U0001F600", Some(('\u{1F600}', 9))),
            ("x00", None),
            ("000", None),
            ("x8f", None),
            ("400", None),
            ("x4", None),
            ("q", None),
        ] {
            assert_eq!(cunescape_one(escaped), expected, "\\{escaped}");
        }
    }

    /// A path written with a trailing or doubled `/` is kept, simplified as
    /// systemd loads it, and not dropped by the form check: dropping a
    /// `required_config` path would let the enable go unchecked. A
    /// configuration path with a `..` component is kept as written.
    #[test]
    fn a_path_with_a_trailing_or_doubled_slash_is_kept_simplified() {
        let exec = crate::executor::mock::MockExecutor::new()
            .with_dir("/var/lib/x/a", vec![])
            .with_dir("/var/lib/y/a", vec![]);
        let name = UnitName::try_from("x@a.service".to_owned()).expect("a unit name");
        let needs = instance_needs(
            &exec,
            &name,
            Some((
                "/usr/lib/systemd/system/x@.service",
                "[Unit]\nConditionPathExists=/etc/x//%i/\nConditionPathExists=/etc/./z/%i\n\
               ConditionPathExists=/etc/w/../%i.conf\n\
               [Service]\nReadWritePaths=/var/lib/x/%i/\nStateDirectory=./y/%i/\n",
            )),
        )
        .expect("interpretable needs");
        assert_eq!(
            needs.required_config,
            ["/etc/w/../a.conf", "/etc/x/a", "/etc/z/a"]
        );
        assert_eq!(needs.data_paths, ["/var/lib/x/a", "/var/lib/y/a"]);
        for (path, simple) in [
            ("/etc/x/a/", "/etc/x/a"),
            ("/etc//x///a", "/etc/x/a"),
            ("/", "/"),
            ("/etc/x/a", "/etc/x/a"),
            ("/etc/x/./a", "/etc/x/a"),
            ("/./etc/./x/.", "/etc/x"),
            ("/etc/x/../a", "/etc/x/../a"),
            ("./y/a/", "y/a"),
        ] {
            assert_eq!(simplify_path(path), simple, "{path}");
        }
    }

    /// `unit_file_find_dirs`'s order (systemd 252 and 257): own, template
    /// and the template's prefixes, then the instance's prefixes.
    #[test]
    fn the_named_drop_in_dirs_are_systemds() {
        for (name, expected) in [
            ("plain.service", vec!["plain.service.d"]),
            (
                "a-b-c.service",
                vec!["a-b-c.service.d", "a-b-.service.d", "a-.service.d"],
            ),
            (
                "worker-job@site.service",
                vec![
                    "worker-job@site.service.d",
                    "worker-job@.service.d",
                    "worker-.service.d",
                    "worker-@site.service.d",
                    "worker-@.service.d",
                ],
            ),
            ("t@x.timer", vec!["t@x.timer.d", "t@.timer.d"]),
            ("-a.service", vec!["-a.service.d"]),
        ] {
            let mut dirs = Vec::new();
            named_drop_in_dirs(name, &mut dirs);
            assert_eq!(dirs, expected, "{name}");
        }
    }

    /// Which sections take which settings, and which replacements the
    /// single-path parsers accept (systemd 252 and 257).
    #[test]
    fn a_reset_applies_only_where_systemd_takes_it() {
        let expand = |value: &str| expand_unit_specifiers(value, "w@a", SpecifierSet::Runtime);
        for (section, key, suffix, takes) in [
            ("Service", "EnvironmentFile", "service", true),
            ("Unit", "EnvironmentFile", "service", false),
            ("Socket", "EnvironmentFile", "socket", true),
            ("Service", "EnvironmentFile", "socket", false),
            ("Service", "EnvironmentFile", "timer", false),
            ("Unit", "ConditionPathExists", "service", true),
            ("Unit", "AssertFileNotEmpty", "timer", true),
            ("Service", "ConditionPathExists", "service", false),
            ("Unit", "ConditionPathExist", "service", false),
            ("Service", "ExecStart", "service", true),
            ("Service", "ExecStopPre", "service", false),
            ("Socket", "ExecStopPre", "socket", true),
            ("Socket", "ExecStart", "socket", false),
            ("Service", "PIDFile", "service", true),
            ("Socket", "PIDFile", "socket", false),
            ("Mount", "WorkingDirectory", "mount", true),
            ("Service", "Description", "service", false),
        ] {
            assert_eq!(
                section_takes(section, key, suffix),
                takes,
                "[{section}] {key}= in a {suffix}"
            );
        }
        for (key, value, accepted) in [
            ("WorkingDirectory", "/srv/w", true),
            ("WorkingDirectory", "-/srv/w", true),
            ("WorkingDirectory", "~", true),
            ("WorkingDirectory", "-~", true),
            ("WorkingDirectory", "/srv//./w/", true),
            ("WorkingDirectory", "/srv/%i", true),
            ("WorkingDirectory", "-relative", false),
            ("WorkingDirectory", "relative", false),
            ("WorkingDirectory", "/srv/../w", false),
            ("WorkingDirectory", "-%h/w", false),
            ("RootDirectory", "/srv/w", true),
            ("RootDirectory", "-/srv/w", false),
            ("RootImage", "relative.img", false),
            ("PIDFile", "w.pid", true),
            ("PIDFile", "/run/w/%i.pid", true),
            ("PIDFile", "/proc/w.pid", false),
            ("PIDFile", "/devices/w.pid", true),
            ("PIDFile", "../w.pid", false),
        ] {
            assert_eq!(
                single_path_accepted(key, value, &expand),
                accepted,
                "{key}={value}"
            );
        }
        let long_component = format!("/srv/{}", "x".repeat(NAME_MAX_LENGTH + 1));
        assert!(!systemd_accepts_path(&long_component));
        let long_path = format!("/{}", ["x"; PATH_MAX_LENGTH / 2 + 1].join("/"));
        assert!(!systemd_accepts_path(&long_path));
    }

    /// `unit_path_printf` refuses an expansion longer than 4095 bytes before
    /// anything simplifies it (systemd 252 and 257,
    /// `src/core/unit-printf.h`), so a replacement whose expansion is over
    /// the limit replaces nothing, however short it simplifies. The length
    /// is the expansion's (`%i` gives `a`), after `WorkingDirectory=`'s `-`
    /// and before `PIDFile=`'s `/run` prefix.
    #[test]
    fn a_replacement_expanding_past_the_limit_is_refused_before_simplifying() {
        let expand = |value: &str| expand_unit_specifiers(value, "w@a", SpecifierSet::Runtime);
        // An absolute value of `length` bytes that simplifies to `/srv`.
        let collapsing = |length: usize| format!("{}srv", "/".repeat(length - 3));
        // A relative value of `length` bytes that simplifies to `w.pid`.
        let relative = |length: usize| format!(".{}w.pid", "/".repeat(length - 6));
        let at = PATH_MAX_LENGTH;
        for (key, value, accepted) in [
            ("WorkingDirectory", collapsing(at), true),
            ("WorkingDirectory", collapsing(at + 1), false),
            ("WorkingDirectory", format!("-{}", collapsing(at)), true),
            (
                "WorkingDirectory",
                format!("-{}", collapsing(at + 1)),
                false,
            ),
            (
                "WorkingDirectory",
                format!("-/{}", "./".repeat(2048) + "srv"),
                false,
            ),
            (
                "WorkingDirectory",
                format!("{}%i", collapsing(at - 1)),
                true,
            ),
            ("WorkingDirectory", format!("{}%i", collapsing(at)), false),
            ("RootDirectory", collapsing(at), true),
            ("RootDirectory", collapsing(at + 1), false),
            ("RootImage", collapsing(at), true),
            ("RootImage", collapsing(at + 1), false),
            ("PIDFile", collapsing(at), true),
            ("PIDFile", collapsing(at + 1), false),
            ("PIDFile", relative(at), true),
            ("PIDFile", relative(at + 1), false),
        ] {
            assert_eq!(
                single_path_accepted(key, &value, &expand),
                accepted,
                "{key}= of {} bytes",
                value.len()
            );
        }
    }

    #[test]
    fn the_tables_name_only_templates() {
        for (template, path) in INSTANCE_CONFIG_TABLE.iter().chain(&INSTANCE_DATA_TABLE) {
            let unit = UnitName::try_from((*template).to_owned()).expect("a unit name");
            assert!(unit.is_template(), "{template}");
            assert!(holds_instance_specifier(path), "{path}");
        }
    }
}
