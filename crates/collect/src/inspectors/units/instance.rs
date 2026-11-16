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
    UNIT_FILE_WHITESPACE, UnitName, is_instance_config_path, is_instance_data_path, logical_lines,
    systemd_reads_drop_in,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io::ErrorKind;
use std::path::Path;

/// Templates whose instance reads a configuration file its unit file does
/// not name, and that file's path (Mark Russell, 2026-11-13).
/// `wg-quick@.service` runs `wg-quick up %i`; the OpenVPN templates pass
/// `--config %i.conf` from a `WorkingDirectory=`.
const INSTANCE_CONFIG_TABLE: [(&str, &str); 3] = [
    ("wg-quick@.service", "/etc/wireguard/%i.conf"),
    ("openvpn-client@.service", "/etc/openvpn/client/%i.conf"),
    ("openvpn-server@.service", "/etc/openvpn/server/%i.conf"),
];

/// Templates whose instance keeps data at a path its unit file does not
/// name, and that path (Mark Russell, 2026-11-13).
const INSTANCE_DATA_TABLE: [(&str, &str); 1] = [("dirsrv@.service", "/var/lib/dirsrv/slapd-%i")];

/// The attention note of an enabled instance of a template on no list
/// whose unit file names nothing it reads or keeps (§ 7.5, Mark Russell,
/// 2026-11-13).
pub(crate) const INSTANCE_READS_UNKNOWN: &str = "inspectah does not know what this instance \
     reads: its template is on no list, and its unit file names no path under /etc or /var \
     for it. It is carried on its unit file alone.";

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
const INSTALL_SECTION_HEADER: &str = "[Install]";

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
}

/// Why what an instance needs is unknown. Its enable is not carried: a
/// path it may read would go unchecked by rule 6.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NeedsUnknown {
    /// The value `value` of the setting `key` names a path for the
    /// instance and cannot be read as systemd reads it: an unbalanced
    /// quote, an escape systemd refuses, a specifier inspectah does not
    /// expand, or a configuration path holding a control character.
    Uninterpretable { key: String, value: String },
    /// A runtime drop-in directory or drop-in at `path` could not be read
    /// in full, or a drop-in there is not text: one of its settings may
    /// name a path.
    Unreadable { path: String, error: String },
}

/// What the instance `name` needs, read from `fragment`, the text of its
/// effective unit file, and the runtime drop-ins systemd applies to it
/// ([`runtime_drop_ins`]), in load order, and the fixed tables for its
/// template. A data path is kept only when `exec` finds something there. A
/// unit that is not an instance needs nothing.
pub(crate) fn instance_needs(
    exec: &dyn Executor,
    name: &UnitName,
    fragment: Option<&str>,
) -> Result<InstanceNeeds, NeedsUnknown> {
    let Some(instance) = name.instance() else {
        return Ok(InstanceNeeds::default());
    };
    let unit_texts: Vec<String> = fragment
        .map(str::to_owned)
        .into_iter()
        .chain(runtime_drop_ins(exec, name)?)
        .collect();
    let template = instance.template();
    let (stem, _) = name
        .as_str()
        .rsplit_once('.')
        .expect("a unit name ends in its type suffix");
    let expand = |value: &str| {
        expand_unit_specifiers(value, stem, SpecifierSet::Runtime).map(|path| simplify_path(&path))
    };

    let mut required_config = BTreeSet::new();
    let mut data = BTreeSet::new();
    for (key, value) in effective_assignments(&unit_texts) {
        let uninterpretable = || NeedsUnknown::Uninterpretable {
            key: key.clone(),
            value: value.clone(),
        };
        let operands = match operands(&key, &value) {
            Ok(operands) => operands,
            Err(Uninterpretable) if holds_instance_specifier(&value) => {
                return Err(uninterpretable());
            }
            Err(Uninterpretable) => continue,
        };
        for operand in operands {
            let (text, state_directory) = match &operand {
                Operand::Path(text) => (text, false),
                Operand::StateDirectory(text) => (text, true),
            };
            if !holds_instance_specifier(text) {
                continue;
            }
            let expanded = expand(text).ok_or_else(uninterpretable)?;
            if state_directory {
                if !expanded.starts_with('/') {
                    data.insert(format!("{STATE_DIRECTORY_ROOT}/{expanded}"));
                }
                continue;
            }
            if expanded.starts_with(&format!("{CONFIG_ROOT}/")) {
                // A configuration path is never dropped: one the form check
                // refuses (a control character) declines the enable.
                if !is_instance_config_path(&expanded) {
                    return Err(uninterpretable());
                }
                required_config.insert(expanded);
            } else {
                data.insert(expanded);
            }
        }
    }
    required_config.extend(
        table_rows(&INSTANCE_CONFIG_TABLE, &template)
            .filter_map(expand)
            .filter(|path| is_instance_config_path(path)),
    );
    data.extend(table_rows(&INSTANCE_DATA_TABLE, &template).filter_map(expand));
    let data_paths = data
        .into_iter()
        .filter(|path| is_instance_data_path(path) && exec.file_exists(Path::new(path)))
        .collect();
    Ok(InstanceNeeds {
        required_config: required_config.into_iter().collect(),
        data_paths,
    })
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
/// error included, through `read_dir_strict`), and a drop-in that cannot be
/// read as text, are [`NeedsUnknown::Unreadable`]: systemd reads what the
/// collector cannot.
fn runtime_drop_ins(exec: &dyn Executor, name: &UnitName) -> Result<Vec<String>, NeedsUnknown> {
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
    for dir in &dirs {
        let entries = match exec.read_dir_strict(Path::new(dir)) {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => return Err(unreadable(dir, e.to_string())),
        };
        for entry in entries.into_iter().filter(|e| systemd_reads_drop_in(e)) {
            let path = format!("{dir}/{entry}");
            chosen.entry(entry).or_insert(path);
        }
    }
    chosen
        .values()
        .map(|path| {
            exec.read_file(Path::new(path))
                .map_err(|e| unreadable(path, e.to_string()))
        })
        .collect()
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

/// The assignments of one unit file text outside `[Install]`, as
/// `(key, value)` by logical line, both trimmed.
fn assignments(text: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut in_install = false;
    for line in logical_lines(text) {
        let line = line.trim_matches(UNIT_FILE_WHITESPACE);
        if line.starts_with('[') && line.ends_with(']') {
            in_install = line == INSTALL_SECTION_HEADER;
            continue;
        }
        if in_install {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            found.push((
                key.trim_matches(UNIT_FILE_WHITESPACE).to_owned(),
                value.trim_matches(UNIT_FILE_WHITESPACE).to_owned(),
            ));
        }
    }
    found
}

/// The assignments that are in effect once every text of `unit_texts`
/// (the unit file, then its drop-ins in load order) is applied with
/// systemd's resets, in order. An empty value resets the values before it
/// ([`Syntax::resets`]) and is not kept; a single-path setting keeps only
/// its last value. A setting whose syntax is not modeled keeps every value,
/// an empty one dropped: if systemd resets it, a path is required that the
/// unit no longer reads, which omits an enable rather than carry one.
fn effective_assignments(unit_texts: &[String]) -> Vec<(String, String)> {
    let mut kept: Vec<(String, String)> = Vec::new();
    for (key, value) in unit_texts.iter().flat_map(|text| assignments(text)) {
        let syntax = Syntax::of(&key);
        let replaces = syntax == Syntax::SinglePath;
        if value.is_empty() || replaces {
            kept.retain(|(earlier, _)| !syntax.resets(&key, earlier));
        }
        if !value.is_empty() {
            kept.push((key, value));
        }
    }
    kept
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
        for (key, value) in assignments(text) {
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
            Some(
                "[Unit]\nConditionPathExists=/etc/x//%i/\nConditionPathExists=/etc/./z/%i\n\
               ConditionPathExists=/etc/w/../%i.conf\n\
               [Service]\nReadWritePaths=/var/lib/x/%i/\nStateDirectory=./y/%i/\n",
            ),
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

    #[test]
    fn the_tables_name_only_templates() {
        for (template, path) in INSTANCE_CONFIG_TABLE.iter().chain(&INSTANCE_DATA_TABLE) {
            let unit = UnitName::try_from((*template).to_owned()).expect("a unit name");
            assert!(unit.is_template(), "{template}");
            assert!(holds_instance_specifier(path), "{path}");
        }
    }
}
