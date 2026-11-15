//! The `[Install]` section `systemctl enable` acts on for one unit (§ 7.2
//! of the units spec, step 4): read from the unit file and the drop-ins
//! that apply to it, in systemd's load order, with the reset systemd
//! performs.
//!
//! The reader is a pure function over text. The caller lists the
//! directories [`drop_in_dirs`] names under both unit directories and
//! reads the files; nothing here touches the host.

// The scan calls the reader once install files and install state are
// collected; until then only the tests below do.
#![cfg_attr(not(test), allow(dead_code))]

use super::{UNIT_FILE_WHITESPACE, logical_lines};
use crate::inspectors::scheduled::expand_timer_specifiers;
use inspectah_core::types::units::{InstallUnknown, UnitName};
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

/// The section header `enable` reads.
const INSTALL_SECTION: &str = "Install";

/// The suffix of a unit's drop-in directory.
const DROP_IN_DIR_SUFFIX: &str = ".d";

/// The unit directory a drop-in sits under. The order is the priority:
/// for one file name in one directory, an `/etc` file replaces a vendor
/// one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DropInRoot {
    /// `/etc/systemd/system`.
    Etc,
    /// `/usr/lib/systemd/system`.
    Vendor,
}

/// Where one drop-in was read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DropInSource {
    /// The unit directory it sits under.
    pub(crate) root: DropInRoot,
    /// Its directory's name, one of [`drop_in_dirs`]. A drop-in from any
    /// other directory is not read for `[Install]`.
    pub(crate) dir: String,
    /// Its file name, a `.conf` name.
    pub(crate) file_name: String,
}

/// The `[Install]` keys `enable` acts on, after every drop-in that applies.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct InstallSection {
    pub(crate) wanted_by: Vec<UnitName>,
    pub(crate) required_by: Vec<UnitName>,
    pub(crate) alias: Vec<UnitName>,
    pub(crate) also: Vec<UnitName>,
    pub(crate) default_instance: Option<String>,
}

/// What a unit's `[Install]` section is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InstallRead {
    /// The unit has no `[Install]` section.
    Absent,
    Known(InstallSection),
    /// The section cannot be relied on. `assigns_also` says whether any
    /// line assigns `Also=`, an empty assignment included.
    Undetermined {
        reason: InstallUnknown,
        assigns_also: bool,
    },
}

/// The `.d` directory names `enable` reads `[Install]` from for `name`:
/// its own and, for an instance, its template's. The type-wide directory
/// (`service.d`) and the dash-prefix directories (`a-.service.d` for
/// `a-b.service`) are never among them. The caller looks for each under
/// both unit directories.
pub(crate) fn drop_in_dirs(name: &UnitName) -> Vec<String> {
    let mut dirs = vec![format!("{}{DROP_IN_DIR_SUFFIX}", name.as_str())];
    if let Some(parts) = name.instance() {
        dirs.push(format!("{}{DROP_IN_DIR_SUFFIX}", parts.template().as_str()));
    }
    dirs
}

/// The effective `[Install]` section of `name`, whose unit file reads
/// `fragment`, with `%i`, `%I`, `%p` and `%N` expanded.
///
/// Load order: the unit file first, then one drop-in per file name, in
/// file-name order across every directory of [`drop_in_dirs`] and both
/// roots, not one root at a time. Of the drop-ins that share a file name,
/// the one under `/etc` is read over a vendor one, and within one root the
/// unit's own `.d` over its template's. The second half is assumed, not
/// measured: one file name is one key across the two directories.
///
/// An empty `WantedBy=` or `Alias=` clears every value of that key before
/// it; an empty `Also=` clears nothing. An empty `RequiredBy=` after a
/// `RequiredBy=` value, any `UpheldBy=`, a specifier the reader does not
/// expand and a value that is not a [`UnitName`] each make the section
/// [`InstallRead::Undetermined`], with the first such reason in load order.
pub(crate) fn read_install(
    name: &UnitName,
    fragment: &str,
    drop_ins: &[(DropInSource, &str)],
) -> InstallRead {
    let dirs = drop_in_dirs(name);
    let mut selected: BTreeMap<&str, ((DropInRoot, usize), &str)> = BTreeMap::new();
    for (source, text) in drop_ins {
        let Some(dir_rank) = dirs.iter().position(|dir| *dir == source.dir) else {
            continue;
        };
        let rank = (source.root, dir_rank);
        match selected.entry(source.file_name.as_str()) {
            Entry::Vacant(slot) => {
                slot.insert((rank, text));
            }
            Entry::Occupied(mut slot) => {
                if rank < slot.get().0 {
                    slot.insert((rank, text));
                }
            }
        }
    }

    let mut reader = Reader::new(name);
    reader.read(fragment);
    for (_, text) in selected.into_values() {
        reader.read(text);
    }
    reader.finish()
}

/// The `[Install]` assignments read so far, in load order.
struct Reader<'a> {
    /// The unit name without its type suffix, which the specifiers expand
    /// from.
    stem: &'a str,
    /// Whether any file read so far has an `[Install]` header.
    has_section: bool,
    section: InstallSection,
    /// The first reason the section cannot be relied on.
    unknown: Option<InstallUnknown>,
    assigns_also: bool,
}

impl<'a> Reader<'a> {
    fn new(name: &'a UnitName) -> Self {
        let (stem, _) = name
            .as_str()
            .rsplit_once('.')
            .expect("a unit name ends in its type suffix");
        Self {
            stem,
            has_section: false,
            section: InstallSection::default(),
            unknown: None,
            assigns_also: false,
        }
    }

    /// Apply the `[Install]` assignments of one file, by logical line.
    fn read(&mut self, text: &str) {
        let mut in_install = false;
        for line in logical_lines(text) {
            let line = line.trim_matches(UNIT_FILE_WHITESPACE);
            if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                in_install = header == INSTALL_SECTION;
                self.has_section |= in_install;
                continue;
            }
            if !in_install {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                self.assign(
                    key.trim_matches(UNIT_FILE_WHITESPACE),
                    value.trim_matches(UNIT_FILE_WHITESPACE),
                );
            }
        }
    }

    fn assign(&mut self, key: &str, value: &str) {
        match key {
            "WantedBy" if value.is_empty() => self.section.wanted_by.clear(),
            "WantedBy" => self.extend(value, |s| &mut s.wanted_by),
            "Alias" if value.is_empty() => self.section.alias.clear(),
            "Alias" => self.extend(value, |s| &mut s.alias),
            "Also" => {
                self.assigns_also = true;
                self.extend(value, |s| &mut s.also);
            }
            "RequiredBy" if value.is_empty() => {
                if !self.section.required_by.is_empty() {
                    self.decline(InstallUnknown::RequiredByReset);
                }
            }
            "RequiredBy" => self.extend(value, |s| &mut s.required_by),
            "UpheldBy" => self.decline(InstallUnknown::UpheldBy),
            "DefaultInstance" if value.is_empty() => self.section.default_instance = None,
            "DefaultInstance" => match expand_timer_specifiers(value, self.stem) {
                Some(instance) => self.section.default_instance = Some(instance),
                None => self.decline(InstallUnknown::Specifier),
            },
            _ => {}
        }
    }

    /// Add each whitespace-separated unit name of `value` to the list
    /// `list` picks, once.
    fn extend(&mut self, value: &str, list: fn(&mut InstallSection) -> &mut Vec<UnitName>) {
        for word in value.split(UNIT_FILE_WHITESPACE).filter(|w| !w.is_empty()) {
            let Some(expanded) = expand_timer_specifiers(word, self.stem) else {
                self.decline(InstallUnknown::Specifier);
                continue;
            };
            match UnitName::try_from(expanded) {
                Ok(unit) => {
                    let values = list(&mut self.section);
                    if !values.contains(&unit) {
                        values.push(unit);
                    }
                }
                Err(_) => self.decline(InstallUnknown::ForeignName),
            }
        }
    }

    fn decline(&mut self, reason: InstallUnknown) {
        self.unknown.get_or_insert(reason);
    }

    fn finish(self) -> InstallRead {
        match (self.unknown, self.has_section) {
            (Some(reason), _) => InstallRead::Undetermined {
                reason,
                assigns_also: self.assigns_also,
            },
            (None, false) => InstallRead::Absent,
            (None, true) => InstallRead::Known(self.section),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(name: &str) -> UnitName {
        UnitName::try_from(name.to_owned()).expect("a valid unit name")
    }

    fn units(names: &[&str]) -> Vec<UnitName> {
        names.iter().map(|n| unit(n)).collect()
    }

    fn source(root: DropInRoot, dir: &str, file_name: &str) -> DropInSource {
        DropInSource {
            root,
            dir: dir.to_owned(),
            file_name: file_name.to_owned(),
        }
    }

    fn wanted(names: &[&str]) -> InstallRead {
        InstallRead::Known(InstallSection {
            wanted_by: units(names),
            ..InstallSection::default()
        })
    }

    fn undetermined(reason: InstallUnknown, assigns_also: bool) -> InstallRead {
        InstallRead::Undetermined {
            reason,
            assigns_also,
        }
    }

    use DropInRoot::{Etc, Vendor};

    /// A unit file whose section wants `multi-user.target`.
    const WANTS_MULTI_USER: &str = "[Unit]\nDescription=x\n\n[Service]\nExecStart=/bin/true\n\n[Install]\nWantedBy=multi-user.target\n";

    /// A unit file with no `[Install]` section.
    const NO_INSTALL: &str = "[Unit]\nDescription=x\n\n[Service]\nExecStart=/bin/true\n";

    /// A drop-in that changes no `[Install]` key.
    const NO_INSTALL_DROP_IN: &str = "[Service]\nNice=5\n";

    /// Every reader case of § 7.2 step 4, the load order, the reset rule
    /// and the undetermined reasons: the unit, its file, its drop-ins as
    /// (root, directory, file name, text), and the section `enable` acts on.
    #[test]
    fn the_install_reader_follows_systemds_load_order_and_resets() {
        type DropIns<'a> = &'a [(DropInRoot, &'a str, &'a str, &'a str)];
        let cases: &[(&str, &str, &str, DropIns, InstallRead)] = &[
            (
                "WantedBy= from the unit file",
                "a.service",
                WANTS_MULTI_USER,
                &[],
                wanted(&["multi-user.target"]),
            ),
            (
                "WantedBy= from an /etc drop-in only",
                "a.service",
                NO_INSTALL,
                &[(
                    Etc,
                    "a.service.d",
                    "10-a.conf",
                    "[Install]\nWantedBy=extra.target\n",
                )],
                wanted(&["extra.target"]),
            ),
            (
                "WantedBy= from a vendor drop-in only",
                "a.service",
                NO_INSTALL,
                &[(
                    Vendor,
                    "a.service.d",
                    "10-a.conf",
                    "[Install]\nWantedBy=extra.target\n",
                )],
                wanted(&["extra.target"]),
            ),
            (
                "WantedBy= from the template's .d for an instance",
                "t@x.service",
                NO_INSTALL,
                &[(
                    Vendor,
                    "t@.service.d",
                    "10-a.conf",
                    "[Install]\nWantedBy=extra.target\n",
                )],
                wanted(&["extra.target"]),
            ),
            (
                "a later drop-in adds to the unit file's list",
                "a.service",
                WANTS_MULTI_USER,
                &[(
                    Etc,
                    "a.service.d",
                    "10-a.conf",
                    "[Install]\nWantedBy=extra.target\n",
                )],
                wanted(&["multi-user.target", "extra.target"]),
            ),
            (
                "file-name order across both roots, not root by root",
                "a.service",
                WANTS_MULTI_USER,
                &[
                    (Etc, "a.service.d", "20-b.conf", "[Install]\nWantedBy=\n"),
                    (
                        Vendor,
                        "a.service.d",
                        "10-a.conf",
                        "[Install]\nWantedBy=extra.target\n",
                    ),
                ],
                wanted(&[]),
            ),
            (
                "file-name order across both roots, reversed",
                "a.service",
                WANTS_MULTI_USER,
                &[
                    (Etc, "a.service.d", "10-a.conf", "[Install]\nWantedBy=\n"),
                    (
                        Vendor,
                        "a.service.d",
                        "20-b.conf",
                        "[Install]\nWantedBy=extra.target\n",
                    ),
                ],
                wanted(&["extra.target"]),
            ),
            (
                "file-name order across the own and the template's .d",
                "t@x.service",
                WANTS_MULTI_USER,
                &[
                    (Etc, "t@x.service.d", "20-b.conf", "[Install]\nWantedBy=\n"),
                    (
                        Vendor,
                        "t@.service.d",
                        "10-a.conf",
                        "[Install]\nWantedBy=extra.target\n",
                    ),
                ],
                wanted(&[]),
            ),
            (
                "an /etc file replaces a vendor file of its name in the same directory",
                "a.service",
                WANTS_MULTI_USER,
                &[
                    (
                        Vendor,
                        "a.service.d",
                        "10-a.conf",
                        "[Install]\nWantedBy=extra.target\n",
                    ),
                    (Etc, "a.service.d", "10-a.conf", NO_INSTALL_DROP_IN),
                ],
                wanted(&["multi-user.target"]),
            ),
            (
                "an /etc file in the instance's .d replaces a vendor one in the template's",
                "t@x.service",
                WANTS_MULTI_USER,
                &[
                    (
                        Vendor,
                        "t@.service.d",
                        "10-a.conf",
                        "[Install]\nWantedBy=extra.target\n",
                    ),
                    (Etc, "t@x.service.d", "10-a.conf", NO_INSTALL_DROP_IN),
                ],
                wanted(&["multi-user.target"]),
            ),
            (
                "an /etc file in the template's .d replaces a vendor one in the instance's",
                "t@x.service",
                WANTS_MULTI_USER,
                &[
                    (
                        Vendor,
                        "t@x.service.d",
                        "10-a.conf",
                        "[Install]\nWantedBy=own.target\n",
                    ),
                    (
                        Etc,
                        "t@.service.d",
                        "10-a.conf",
                        "[Install]\nWantedBy=etc.target\n",
                    ),
                ],
                wanted(&["multi-user.target", "etc.target"]),
            ),
            (
                "of two vendor files of one name, the instance's own .d is read",
                "t@x.service",
                WANTS_MULTI_USER,
                &[
                    (
                        Vendor,
                        "t@.service.d",
                        "10-a.conf",
                        "[Install]\nWantedBy=template.target\n",
                    ),
                    (
                        Vendor,
                        "t@x.service.d",
                        "10-a.conf",
                        "[Install]\nWantedBy=own.target\n",
                    ),
                ],
                wanted(&["multi-user.target", "own.target"]),
            ),
            (
                "the type-wide and dash-prefix directories are not read",
                "a-b.service",
                WANTS_MULTI_USER,
                &[
                    (
                        Vendor,
                        "service.d",
                        "10-a.conf",
                        "[Install]\nWantedBy=typewide.target\n",
                    ),
                    (
                        Etc,
                        "a-.service.d",
                        "10-b.conf",
                        "[Install]\nWantedBy=dash.target\n",
                    ),
                ],
                wanted(&["multi-user.target"]),
            ),
            (
                "a drop-in holding only an empty WantedBy= expects nothing",
                "a.service",
                WANTS_MULTI_USER,
                &[(Etc, "a.service.d", "10-a.conf", "[Install]\nWantedBy=\n")],
                wanted(&[]),
            ),
            (
                "a reset then a value in one drop-in keeps only the value",
                "a.service",
                WANTS_MULTI_USER,
                &[(
                    Etc,
                    "a.service.d",
                    "10-a.conf",
                    "[Install]\nWantedBy=\nWantedBy=graphical.target\n",
                )],
                wanted(&["graphical.target"]),
            ),
            (
                "a reset in 10-a.conf and a value in 20-b.conf keeps the value",
                "a.service",
                WANTS_MULTI_USER,
                &[
                    (Etc, "a.service.d", "10-a.conf", "[Install]\nWantedBy=\n"),
                    (
                        Etc,
                        "a.service.d",
                        "20-b.conf",
                        "[Install]\nWantedBy=extra.target\n",
                    ),
                ],
                wanted(&["extra.target"]),
            ),
            (
                "a value in 10-a.conf and a reset in 20-b.conf expects nothing",
                "a.service",
                WANTS_MULTI_USER,
                &[
                    (
                        Etc,
                        "a.service.d",
                        "10-a.conf",
                        "[Install]\nWantedBy=extra.target\n",
                    ),
                    (Etc, "a.service.d", "20-b.conf", "[Install]\nWantedBy=\n"),
                ],
                wanted(&[]),
            ),
            (
                "an empty Alias= clears the unit file's alias",
                "a.service",
                "[Install]\nWantedBy=multi-user.target\nAlias=b.service\n",
                &[(Etc, "a.service.d", "10-a.conf", "[Install]\nAlias=\n")],
                wanted(&["multi-user.target"]),
            ),
            (
                "an empty Also= keeps the unit file's member",
                "a.service",
                "[Install]\nWantedBy=multi-user.target\nAlso=a.socket\n",
                &[(Etc, "a.service.d", "10-a.conf", "[Install]\nAlso=\n")],
                InstallRead::Known(InstallSection {
                    wanted_by: units(&["multi-user.target"]),
                    also: units(&["a.socket"]),
                    ..InstallSection::default()
                }),
            ),
            (
                "an empty RequiredBy= after a value is undetermined",
                "a.service",
                "[Install]\nWantedBy=multi-user.target\nRequiredBy=a.target\n",
                &[(Etc, "a.service.d", "10-a.conf", "[Install]\nRequiredBy=\n")],
                undetermined(InstallUnknown::RequiredByReset, false),
            ),
            (
                "an empty RequiredBy= with no value before it is ignored",
                "a.service",
                WANTS_MULTI_USER,
                &[(Etc, "a.service.d", "10-a.conf", "[Install]\nRequiredBy=\n")],
                wanted(&["multi-user.target"]),
            ),
            (
                "UpheldBy= is undetermined",
                "a.service",
                "[Install]\nWantedBy=multi-user.target\nUpheldBy=multi-user.target\n",
                &[],
                undetermined(InstallUnknown::UpheldBy, false),
            ),
            (
                "an empty UpheldBy= is undetermined",
                "a.service",
                WANTS_MULTI_USER,
                &[(Etc, "a.service.d", "10-a.conf", "[Install]\nUpheldBy=\n")],
                undetermined(InstallUnknown::UpheldBy, false),
            ),
            (
                "%H is undetermined, and the Also= line is recorded",
                "a.service",
                "[Install]\nWantedBy=%H.target\nAlso=a.socket\n",
                &[],
                undetermined(InstallUnknown::Specifier, true),
            ),
            (
                "%i expands for an instance",
                "t@x.service",
                "[Install]\nWantedBy=multi-user.target\nAlias=alias-%i.service\n",
                &[],
                InstallRead::Known(InstallSection {
                    wanted_by: units(&["multi-user.target"]),
                    alias: units(&["alias-x.service"]),
                    ..InstallSection::default()
                }),
            ),
            (
                "a .device value is a foreign name",
                "a.service",
                "[Install]\nWantedBy=dev-sda.device\n",
                &[],
                undetermined(InstallUnknown::ForeignName, false),
            ),
            (
                "no section is absent",
                "a.service",
                NO_INSTALL,
                &[],
                InstallRead::Absent,
            ),
            (
                "an [Install] header with CRLF and a value with trailing CR read as plain",
                "a.service",
                "[Unit]\r\nDescription=x\r\n[Install]\r\nWantedBy=multi-user.target \r\n",
                &[],
                wanted(&["multi-user.target"]),
            ),
        ];
        for (label, name, fragment, drop_ins, expected) in cases {
            let drop_ins: Vec<(DropInSource, &str)> = drop_ins
                .iter()
                .map(|&(root, dir, file, text)| (source(root, dir, file), text))
                .collect();
            assert_eq!(
                &read_install(&unit(name), fragment, &drop_ins),
                expected,
                "{label}"
            );
        }
    }

    /// `enable` reads `[Install]` from the unit's own `.d` and, for an
    /// instance, its template's, and from no type-wide or dash-prefix
    /// directory.
    #[test]
    fn drop_in_dirs_are_the_own_and_the_templates() {
        for (name, expected) in [
            ("a-b-c.service", vec!["a-b-c.service.d"]),
            ("t@x.service", vec!["t@x.service.d", "t@.service.d"]),
        ] {
            let dirs = drop_in_dirs(&unit(name));
            assert_eq!(dirs, expected, "{name}");
            for refused in ["service.d", "a-.service.d", "a-b-.service.d", "t@.d"] {
                assert!(!dirs.iter().any(|d| d == refused), "{name} lists {refused}");
            }
        }
    }
}
