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

use super::specifiers::{SpecifierSet, expand_unit_specifiers};
use inspectah_core::traits::executor::Executor;
use inspectah_core::types::units::{
    UNIT_FILE_WHITESPACE, UnitName, is_instance_config_path, is_instance_data_path, logical_lines,
};
use std::collections::BTreeSet;
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

/// The characters a word is split on to find the paths inside it, as in
/// `--config=/etc/x.conf` or a `BindPaths=` value `src:dst`.
const WORD_SEPARATORS: &[char] = &['=', ':', ','];

/// The characters systemd allows before a path to change how it reads it:
/// the `Exec*=` prefixes, `-` for an optional path, and `|` for a
/// triggering condition. `!` negates a condition.
const PATH_PREFIXES: &[char] = &['-', '@', '+', ':', '|', '!'];

/// The prefix that negates a condition: the path must not exist, so it is
/// not something the instance needs.
const NEGATION_PREFIX: char = '!';

/// The quote characters a unit file value may wrap a word in.
const QUOTES: &[char] = &['"', '\''];

/// What one enabled template instance needs, sorted and without repeats.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct InstanceNeeds {
    /// Paths under `/etc`, kept whether or not anything exists there.
    pub(crate) required_config: Vec<String>,
    /// Paths under `/var` outside `/var/run` and `/var/lock` that exist on
    /// the source host.
    pub(crate) data_paths: Vec<String>,
}

/// What the instance `name` needs, read from `unit_texts`, its effective
/// unit file and the drop-ins that apply in load order, and the fixed
/// tables for its template. A data path is kept only when `exec` finds
/// something there. A unit that is not an instance needs nothing.
pub(crate) fn instance_needs(
    exec: &dyn Executor,
    name: &UnitName,
    unit_texts: &[String],
) -> InstanceNeeds {
    let Some(instance) = name.instance() else {
        return InstanceNeeds::default();
    };
    let template = instance.template();
    let (stem, _) = name
        .as_str()
        .rsplit_once('.')
        .expect("a unit name ends in its type suffix");

    // The unexpanded values: a path the file names is configuration or
    // data by where it expands to; a table row and a `StateDirectory=`
    // value are one or the other by their source.
    let mut named = Vec::new();
    let mut state_directories = Vec::new();
    for text in unit_texts {
        let found = named_paths(text);
        named.extend(found.paths);
        state_directories.extend(found.state_directories);
    }
    let expand = |value: &str| {
        expand_unit_specifiers(value, stem, SpecifierSet::Runtime).map(|path| simplify_path(&path))
    };
    let named: Vec<String> = named.iter().filter_map(|value| expand(value)).collect();

    let required_config: BTreeSet<String> = named
        .iter()
        .cloned()
        .chain(table_rows(&INSTANCE_CONFIG_TABLE, &template).filter_map(expand))
        .filter(|path| is_instance_config_path(path))
        .collect();
    let data_paths: BTreeSet<String> = named
        .into_iter()
        .chain(table_rows(&INSTANCE_DATA_TABLE, &template).filter_map(expand))
        .chain(
            state_directories
                .iter()
                .filter_map(|value| expand(value))
                .filter(|dir| !dir.starts_with('/'))
                .map(|dir| format!("{STATE_DIRECTORY_ROOT}/{dir}")),
        )
        .filter(|path| is_instance_data_path(path) && exec.file_exists(Path::new(path)))
        .collect();
    InstanceNeeds {
        required_config: required_config.into_iter().collect(),
        data_paths: data_paths.into_iter().collect(),
    }
}

/// `path` with each run of `/` made one and a trailing `/` dropped, as
/// systemd simplifies a path it loads, so `/var/lib/x/%i/` names the
/// directory it reads as and is not refused by the form check. Only the
/// collector simplifies: the snapshot loader keeps the strict form.
fn simplify_path(path: &str) -> String {
    let mut simple = String::with_capacity(path.len());
    for c in path.chars() {
        if !(c == '/' && simple.ends_with('/')) {
            simple.push(c);
        }
    }
    if simple.len() > 1 && simple.ends_with('/') {
        simple.pop();
    }
    simple
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

/// The unexpanded values one unit file text names that hold `%i` or `%I`.
#[derive(Debug, Default, PartialEq, Eq)]
struct NamedPaths {
    /// Absolute paths, from any key outside `[Install]`. An optional
    /// `EnvironmentFile=` and a negated condition are left out.
    paths: Vec<String>,
    /// `StateDirectory=` values, relative to [`STATE_DIRECTORY_ROOT`].
    state_directories: Vec<String>,
}

/// The values `text` names that hold `%i` or `%I`, by logical line.
fn named_paths(text: &str) -> NamedPaths {
    let mut named = NamedPaths::default();
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
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim_matches(UNIT_FILE_WHITESPACE);
        match key.trim_matches(UNIT_FILE_WHITESPACE) {
            ENVIRONMENT_FILE_KEY => {
                if !value.starts_with(OPTIONAL_FILE_PREFIX) && holds_instance_specifier(value) {
                    named.paths.push(value.to_owned());
                }
            }
            STATE_DIRECTORY_KEY => named.state_directories.extend(
                words(value)
                    .filter_map(|word| word.split(STATE_DIRECTORY_LINK_SEPARATOR).next())
                    .filter(|dir| holds_instance_specifier(dir))
                    .map(str::to_owned),
            ),
            _ => named.paths.extend(
                words(value)
                    .flat_map(|word| word.split(WORD_SEPARATORS))
                    .filter_map(path_in)
                    .filter(|path| holds_instance_specifier(path))
                    .map(str::to_owned),
            ),
        }
    }
    named
}

/// The whitespace-separated words of `value`, each without its quotes.
fn words(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(UNIT_FILE_WHITESPACE)
        .map(|word| word.trim_matches(QUOTES))
        .filter(|word| !word.is_empty())
}

/// The absolute path `piece` gives, after systemd's prefixes, or `None`
/// for a piece that is no path or a negated one.
fn path_in(piece: &str) -> Option<&str> {
    let path = piece.trim_start_matches(PATH_PREFIXES);
    let prefixes = &piece[..piece.len() - path.len()];
    (path.starts_with('/') && !prefixes.contains(NEGATION_PREFIX)).then_some(path)
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

    #[test]
    fn named_paths_reads_each_kind_of_value() {
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
                "[Service]\nExecStart=-/usr/bin/x --config=/etc/x/%I.conf \"/var/lib/x/%i\"\n",
                vec!["/etc/x/%I.conf", "/var/lib/x/%i"],
                vec![],
            ),
            (
                "[Service]\nBindPaths=/var/lib/x/%i:/data\n",
                vec!["/var/lib/x/%i"],
                vec![],
            ),
            ("[Unit]\nConditionPathExists=!/etc/x/%i\n", vec![], vec![]),
            (
                "[Unit]\nConditionPathExists=|/etc/x/%i\n",
                vec!["/etc/x/%i"],
                vec![],
            ),
            ("[Service]\nExecStart=/usr/bin/x %i\n", vec![], vec![]),
            (
                "[Service]\nExecStart=/usr/bin/x /etc/x/%%i\n",
                vec![],
                vec![],
            ),
            (
                "[Service]\nStateDirectory=x/%i y z/%I:link\n",
                vec![],
                vec!["x/%i", "z/%I"],
            ),
            ("[Install]\nAlias=/etc/x/%i\n", vec![], vec![]),
            (
                "[Service]\nExecStart=/usr/bin/x \\\n  /etc/x/%i.conf\n",
                vec!["/etc/x/%i.conf"],
                vec![],
            ),
            (
                "[Service]\nReadWritePaths=/var/lib/x/%i/ /etc//x/%i\n",
                vec!["/var/lib/x/%i/", "/etc//x/%i"],
                vec![],
            ),
        ] {
            let named = named_paths(text);
            assert_eq!(named.paths, paths, "{text:?}");
            assert_eq!(named.state_directories, state, "{text:?}");
        }
    }

    /// A path written with a trailing or doubled `/` is kept, simplified as
    /// systemd loads it, and not dropped by the form check: dropping a
    /// `required_config` path would let the enable go unchecked.
    #[test]
    fn a_path_with_a_trailing_or_doubled_slash_is_kept_simplified() {
        let exec = crate::executor::mock::MockExecutor::new()
            .with_dir("/var/lib/x/a", vec![])
            .with_dir("/var/lib/y/a", vec![]);
        let name = UnitName::try_from("x@a.service".to_owned()).expect("a unit name");
        let needs = instance_needs(
            &exec,
            &name,
            &["[Unit]\nConditionPathExists=/etc/x//%i/\n\
               [Service]\nReadWritePaths=/var/lib/x/%i/\nStateDirectory=y/%i/\n"
                .to_owned()],
        );
        assert_eq!(needs.required_config, ["/etc/x/a"]);
        assert_eq!(needs.data_paths, ["/var/lib/x/a", "/var/lib/y/a"]);
        for (path, simple) in [
            ("/etc/x/a/", "/etc/x/a"),
            ("/etc//x///a", "/etc/x/a"),
            ("/", "/"),
            ("/etc/x/a", "/etc/x/a"),
        ] {
            assert_eq!(simplify_path(path), simple, "{path}");
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
