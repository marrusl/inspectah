//! What systemd-sysctl would do with the collected search path, computed
//! the way systemd-sysctl does it (`sysctl.d(5)`; systemd's `sysctl.c`).
//!
//! Pure functions over a [`SysctlSection`]: the collector, the audit,
//! refine and tests all derive the explanation here, so they cannot
//! disagree. Nothing is stored; nothing here is a decision.

use super::{
    SysctlDir, SysctlFile, SysctlFileBody, SysctlFileRef, SysctlKey, SysctlLine, SysctlLineRef,
    SysctlName, SysctlSection, sysctl_values_equal,
};
use crate::types::config::{ConfigFileEntry, ConfigSection};
use std::collections::BTreeMap;

/// The systemd package's link that applies `/etc/sysctl.conf` at its
/// position in the search path.
pub const SYSCTL_CONF_LINK: &str = "/etc/sysctl.d/99-sysctl.conf";

/// The legacy single-file configuration [`SYSCTL_CONF_LINK`] points at.
pub const SYSCTL_CONF: &str = "/etc/sysctl.conf";

/// What systemd-sysctl would do on the host, or why it cannot be stated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SysctlExplanation {
    /// Every surviving file's lines are known. One row per concrete key
    /// that some surviving file names, sorted by key.
    Resolved(Vec<SysctlSetting>),
    /// Some input is not known as the host had it: a surviving file
    /// inspectah did not read or that redaction changed, or a runtime value
    /// redaction removed. No value, winner or drift is stated for any key.
    Unknown(Vec<UnknownInput>),
}

/// One input that makes the explanation [`SysctlExplanation::Unknown`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnknownInput {
    /// A surviving `OutsideLink`: systemd-sysctl reads it, inspectah did not.
    OutsideLink { file: SysctlFileRef, target: String },
    /// A surviving file whose content redaction changed.
    RedactedFile { file: SysctlFileRef },
    /// A surviving file the read gate withheld as a machine-state member:
    /// systemd-sysctl reads it, inspectah did not.
    WithheldFile { file: SysctlFileRef },
    /// A key in `redacted_runtime`.
    RedactedRuntime { key: SysctlKey },
}

/// The outcome for one concrete key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SysctlSetting {
    pub key: SysctlKey,
    pub outcome: SysctlOutcome,
    /// Earlier entries for this key, in apply order.
    pub overwritten: Vec<SysctlLineRef>,
}

/// What the last entry for a key does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SysctlOutcome {
    /// systemd-sysctl writes `value`, from the line `from`.
    Writes {
        value: String,
        from: SysctlLineRef,
        runtime: RuntimeObservation,
    },
    /// The last entry for the key is an exclusion: nothing is written.
    Cancelled {
        by: SysctlLineRef,
        runtime: Option<String>,
    },
}

/// How the kernel's value compares with the value written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeObservation {
    Matches,
    Differs {
        runtime: String,
    },
    /// The key was not in `sysctl -a` output at scan time.
    NotExposed,
}

/// An admin file in `/etc/sysctl.d` that replaces a same-named file in
/// `/usr/lib/sysctl.d`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VendorReplacement {
    /// `/etc/sysctl.d/<name>` over `/usr/lib/sysctl.d/<name>`.
    Shadows { name: String },
    /// `/etc/sysctl.d/<name>` is a `/dev/null` link over
    /// `/usr/lib/sysctl.d/<name>`.
    Masks { name: String },
}

/// How an `/etc` sysctl file reaches the image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SysctlCarriage<'a> {
    /// The config entry that carries the file: the entry at its own path,
    /// or, for the package's `99-sysctl.conf` link to `/etc/sysctl.conf`,
    /// the `/etc/sysctl.conf` entry. Carried when the entry is included.
    Config(&'a ConfigFileEntry),
    /// No config entry: true of a package file left as shipped, and of a
    /// link the config walker does not collect.
    NoConfigEntry,
}

/// The current last entry for a key while lines are applied.
enum Entry {
    Assign { value: String, from: SysctlLineRef },
    Exclude { by: SysctlLineRef },
}

impl Entry {
    fn line(self) -> SysctlLineRef {
        match self {
            Entry::Assign { from, .. } => from,
            Entry::Exclude { by } => by,
        }
    }
}

impl SysctlSection {
    /// What systemd-sysctl would do on this host, or why it cannot be
    /// stated.
    ///
    /// A file is dropped when a file of the same name sits in a
    /// higher-priority directory; masks and outside links shadow like any
    /// file. If any survivor was not read or was redacted, or any runtime
    /// value was redacted, the result is `Unknown`: systemd keeps the last
    /// entry for a key, so one unknown line can change the outcome of any
    /// key. Otherwise survivors apply in name order, lines in file order,
    /// the last entry for a key winning. Globs take no part.
    pub fn effective(&self) -> SysctlExplanation {
        let survivors = self.survivors();

        let unknown: Vec<UnknownInput> = survivors
            .iter()
            .filter_map(|file| match &file.body {
                SysctlFileBody::OutsideLink { target } => Some(UnknownInput::OutsideLink {
                    file: file.file_ref(),
                    target: target.clone(),
                }),
                SysctlFileBody::Read { redacted: true, .. } => Some(UnknownInput::RedactedFile {
                    file: file.file_ref(),
                }),
                SysctlFileBody::Withheld { .. } => Some(UnknownInput::WithheldFile {
                    file: file.file_ref(),
                }),
                SysctlFileBody::Read { .. } | SysctlFileBody::Mask => None,
            })
            .chain(
                self.redacted_runtime
                    .iter()
                    .map(|key| UnknownInput::RedactedRuntime { key: key.clone() }),
            )
            .collect();
        if !unknown.is_empty() {
            return SysctlExplanation::Unknown(unknown);
        }

        let mut entries: BTreeMap<SysctlKey, (Entry, Vec<SysctlLineRef>)> = BTreeMap::new();
        for file in survivors {
            for parsed in file.lines() {
                let (key, entry) = match parsed {
                    SysctlLine::Assign {
                        line_no,
                        name: SysctlName::Key(key),
                        value,
                        ..
                    } => (
                        key,
                        Entry::Assign {
                            value,
                            from: line_ref(file, line_no),
                        },
                    ),
                    SysctlLine::Exclude {
                        line_no,
                        name: SysctlName::Key(key),
                    } => (
                        key,
                        Entry::Exclude {
                            by: line_ref(file, line_no),
                        },
                    ),
                    SysctlLine::Assign { .. }
                    | SysctlLine::Exclude { .. }
                    | SysctlLine::Unparsed { .. } => continue,
                };
                match entries.get_mut(&key) {
                    Some((current, overwritten)) => {
                        let replaced = std::mem::replace(current, entry);
                        overwritten.push(replaced.line());
                    }
                    None => {
                        entries.insert(key, (entry, Vec::new()));
                    }
                }
            }
        }

        let settings = entries
            .into_iter()
            .map(|(key, (entry, overwritten))| {
                let runtime = self.runtime.get(&key);
                let outcome = match entry {
                    Entry::Assign { value, from } => {
                        let runtime = match runtime {
                            None => RuntimeObservation::NotExposed,
                            Some(seen) if sysctl_values_equal(&value, seen) => {
                                RuntimeObservation::Matches
                            }
                            Some(seen) => RuntimeObservation::Differs {
                                runtime: seen.clone(),
                            },
                        };
                        SysctlOutcome::Writes {
                            value,
                            from,
                            runtime,
                        }
                    }
                    Entry::Exclude { by } => SysctlOutcome::Cancelled {
                        by,
                        runtime: runtime.cloned(),
                    },
                };
                SysctlSetting {
                    key,
                    outcome,
                    overwritten,
                }
            })
            .collect();
        SysctlExplanation::Resolved(settings)
    }

    /// Admin files in /etc that replace or mask a host vendor file, sorted
    /// by name: each `/etc/sysctl.d` file with a same-named
    /// `/usr/lib/sysctl.d` file.
    pub fn vendor_replacements(&self) -> Vec<VendorReplacement> {
        let mut replacements: Vec<VendorReplacement> = self
            .files_in(SysctlDir::Etc)
            .filter(|etc| {
                self.files_in(SysctlDir::UsrLib)
                    .any(|vendor| vendor.name == etc.name)
            })
            .map(|etc| {
                let name = etc.name.clone();
                match etc.body {
                    SysctlFileBody::Mask => VendorReplacement::Masks { name },
                    SysctlFileBody::OutsideLink { .. }
                    | SysctlFileBody::Withheld { .. }
                    | SysctlFileBody::Read { .. } => VendorReplacement::Shadows { name },
                }
            })
            .collect();
        replacements.sort_by(|a, b| replacement_name(a).cmp(replacement_name(b)));
        replacements
    }

    /// How each /etc file reaches the image, read from the current config
    /// decisions, in (name, dir) order. Display only: the config entry
    /// stays the one decision.
    ///
    /// The carrier is the entry at the file's own path; failing that, for
    /// [`SYSCTL_CONF_LINK`] linked to [`SYSCTL_CONF`], the [`SYSCTL_CONF`]
    /// entry, since the target image ships the same link. No other link is
    /// followed.
    pub fn carriage<'a>(
        &self,
        config: Option<&'a ConfigSection>,
    ) -> Vec<(SysctlFileRef, SysctlCarriage<'a>)> {
        let entry_at = |path: &str| {
            config.and_then(|config| config.files.iter().find(|entry| entry.path == path))
        };
        let mut etc_files: Vec<&SysctlFile> = self.files_in(SysctlDir::Etc).collect();
        etc_files.sort_by(|a, b| a.name.cmp(&b.name));
        etc_files
            .into_iter()
            .map(|file| {
                let path = file.path();
                let carrier = entry_at(&path).or_else(|| {
                    let through_link = path == SYSCTL_CONF_LINK
                        && matches!(
                            &file.body,
                            SysctlFileBody::Read { link_target: Some(target), .. }
                                if target == SYSCTL_CONF
                        );
                    through_link.then(|| entry_at(SYSCTL_CONF)).flatten()
                });
                let carriage =
                    carrier.map_or(SysctlCarriage::NoConfigEntry, SysctlCarriage::Config);
                (file.file_ref(), carriage)
            })
            .collect()
    }

    /// The files systemd-sysctl reads after same-name shadowing, sorted by
    /// name: for each name, the file in the smallest [`SysctlDir`].
    fn survivors(&self) -> Vec<&SysctlFile> {
        let mut by_name: BTreeMap<&str, &SysctlFile> = BTreeMap::new();
        for file in &self.files {
            by_name
                .entry(file.name.as_str())
                .and_modify(|kept| {
                    if file.dir < kept.dir {
                        *kept = file;
                    }
                })
                .or_insert(file);
        }
        by_name.into_values().collect()
    }

    fn files_in(&self, dir: SysctlDir) -> impl Iterator<Item = &SysctlFile> {
        self.files.iter().filter(move |file| file.dir == dir)
    }
}

fn line_ref(file: &SysctlFile, line_no: u32) -> SysctlLineRef {
    SysctlLineRef {
        dir: file.dir,
        name: file.name.clone(),
        line_no,
    }
}

fn replacement_name(replacement: &VendorReplacement) -> &str {
    match replacement {
        VendorReplacement::Shadows { name } | VendorReplacement::Masks { name } => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::config::{ConfigFileEntry, ConfigSection};
    use crate::types::sysctl::{
        SysctlDir, SysctlFile, SysctlFileBody, SysctlFileRef, SysctlKey, SysctlLineRef,
        SysctlSection,
    };
    use std::collections::BTreeMap;

    const SWAPPINESS: &str = "vm.swappiness";

    fn read(dir: SysctlDir, name: &str, content: &str) -> SysctlFile {
        SysctlFile {
            dir,
            name: name.into(),
            body: SysctlFileBody::Read {
                link_target: None,
                content: content.into(),
                redacted: false,
            },
        }
    }

    fn redacted(dir: SysctlDir, name: &str, content: &str) -> SysctlFile {
        SysctlFile {
            dir,
            name: name.into(),
            body: SysctlFileBody::Read {
                link_target: None,
                content: content.into(),
                redacted: true,
            },
        }
    }

    fn linked(dir: SysctlDir, name: &str, target: &str, content: &str) -> SysctlFile {
        SysctlFile {
            dir,
            name: name.into(),
            body: SysctlFileBody::Read {
                link_target: Some(target.into()),
                content: content.into(),
                redacted: false,
            },
        }
    }

    fn mask(dir: SysctlDir, name: &str) -> SysctlFile {
        SysctlFile {
            dir,
            name: name.into(),
            body: SysctlFileBody::Mask,
        }
    }

    fn outside(dir: SysctlDir, name: &str, target: &str) -> SysctlFile {
        SysctlFile {
            dir,
            name: name.into(),
            body: SysctlFileBody::OutsideLink {
                target: target.into(),
            },
        }
    }

    fn section(mut files: Vec<SysctlFile>, runtime: &[(&str, &str)]) -> SysctlSection {
        files.sort_by(|a, b| (&a.name, a.dir).cmp(&(&b.name, b.dir)));
        SysctlSection {
            files,
            runtime: runtime
                .iter()
                .map(|(k, v)| (SysctlKey::normalize(k), v.to_string()))
                .collect::<BTreeMap<_, _>>(),
            redacted_runtime: Vec::new(),
        }
    }

    fn key(name: &str) -> SysctlKey {
        SysctlKey::normalize(name)
    }

    fn file_ref(dir: SysctlDir, name: &str) -> SysctlFileRef {
        SysctlFileRef {
            dir,
            name: name.into(),
        }
    }

    fn line(dir: SysctlDir, name: &str, line_no: u32) -> SysctlLineRef {
        SysctlLineRef {
            dir,
            name: name.into(),
            line_no,
        }
    }

    fn writes(
        key_name: &str,
        value: &str,
        from: SysctlLineRef,
        runtime: RuntimeObservation,
        overwritten: Vec<SysctlLineRef>,
    ) -> SysctlSetting {
        SysctlSetting {
            key: key(key_name),
            outcome: SysctlOutcome::Writes {
                value: value.into(),
                from,
                runtime,
            },
            overwritten,
        }
    }

    fn entry(path: &str) -> ConfigFileEntry {
        ConfigFileEntry {
            path: path.into(),
            ..Default::default()
        }
    }

    /// Each directory contributes its own key, and a key the kernel does
    /// not list is `NotExposed`. Twin: `case_01_each_directory_is_read`
    /// (with its unlisted-key layout).
    #[test]
    fn each_directory_resolves_and_unlisted_key_is_not_exposed() {
        let s = section(
            vec![
                read(SysctlDir::Etc, "10-etc.conf", "kernel.a = 1\n"),
                read(SysctlDir::Run, "20-run.conf", "kernel.b = 2\n"),
                read(SysctlDir::UsrLocalLib, "30-local.conf", "kernel.c = 3\n"),
                read(SysctlDir::UsrLib, "40-lib.conf", "kernel.d = 4\n"),
            ],
            &[("kernel.a", "1"), ("kernel.c", "3"), ("kernel.d", "4")],
        );
        assert_eq!(
            s.effective(),
            SysctlExplanation::Resolved(vec![
                writes(
                    "kernel.a",
                    "1",
                    line(SysctlDir::Etc, "10-etc.conf", 1),
                    RuntimeObservation::Matches,
                    vec![],
                ),
                writes(
                    "kernel.b",
                    "2",
                    line(SysctlDir::Run, "20-run.conf", 1),
                    RuntimeObservation::NotExposed,
                    vec![],
                ),
                writes(
                    "kernel.c",
                    "3",
                    line(SysctlDir::UsrLocalLib, "30-local.conf", 1),
                    RuntimeObservation::Matches,
                    vec![],
                ),
                writes(
                    "kernel.d",
                    "4",
                    line(SysctlDir::UsrLib, "40-lib.conf", 1),
                    RuntimeObservation::Matches,
                    vec![],
                ),
            ])
        );
    }

    /// Files apply in name order whatever their directory, so the last
    /// name wins and the others are `overwritten` in apply order. Twin:
    /// `case_02_priority_by_name_across_directories` (both layouts).
    #[test]
    fn priority_is_by_name_across_directories() {
        let four = section(
            vec![
                read(SysctlDir::UsrLib, "50-default.conf", "vm.swappiness = 1\n"),
                read(SysctlDir::Etc, "90-local.conf", "vm.swappiness = 2\n"),
                read(SysctlDir::Run, "95-run.conf", "vm.swappiness = 3\n"),
                read(
                    SysctlDir::UsrLocalLib,
                    "97-local.conf",
                    "vm.swappiness = 4\n",
                ),
            ],
            &[(SWAPPINESS, "4")],
        );
        assert_eq!(
            four.effective(),
            SysctlExplanation::Resolved(vec![writes(
                SWAPPINESS,
                "4",
                line(SysctlDir::UsrLocalLib, "97-local.conf", 1),
                RuntimeObservation::Matches,
                vec![
                    line(SysctlDir::UsrLib, "50-default.conf", 1),
                    line(SysctlDir::Etc, "90-local.conf", 1),
                    line(SysctlDir::Run, "95-run.conf", 1),
                ],
            )])
        );

        let vendor_later = section(
            vec![
                read(SysctlDir::Etc, "40-local.conf", "vm.swappiness = 2\n"),
                read(SysctlDir::UsrLib, "50-default.conf", "vm.swappiness = 1\n"),
            ],
            &[(SWAPPINESS, "1")],
        );
        assert_eq!(
            vendor_later.effective(),
            SysctlExplanation::Resolved(vec![writes(
                SWAPPINESS,
                "1",
                line(SysctlDir::UsrLib, "50-default.conf", 1),
                RuntimeObservation::Matches,
                vec![line(SysctlDir::Etc, "40-local.conf", 1)],
            )])
        );
    }

    /// A same-named file in a higher-priority directory replaces the
    /// lower one whole: its keys, including one the higher file does not
    /// set, take no part. Twin: `case_03_same_name_shadow_each_adjacent_pair`
    /// (every layout).
    #[test]
    fn same_name_file_shadows_lower_directory() {
        let pairs = [
            (SysctlDir::Etc, SysctlDir::Run),
            (SysctlDir::Run, SysctlDir::UsrLocalLib),
            (SysctlDir::UsrLocalLib, SysctlDir::UsrLib),
            (SysctlDir::Etc, SysctlDir::UsrLib),
            (SysctlDir::Run, SysctlDir::UsrLib),
        ];
        for (higher, lower) in pairs {
            let s = section(
                vec![
                    read(higher, "50-x.conf", "kernel.a = 1\n"),
                    read(lower, "50-x.conf", "kernel.a = 2\nkernel.b = 3\n"),
                ],
                &[("kernel.a", "1"), ("kernel.b", "3")],
            );
            assert_eq!(
                s.effective(),
                SysctlExplanation::Resolved(vec![writes(
                    "kernel.a",
                    "1",
                    line(higher, "50-x.conf", 1),
                    RuntimeObservation::Matches,
                    vec![],
                )]),
                "{higher:?} over {lower:?}"
            );
        }
    }

    /// The last line for a key in one file wins; the earlier one is
    /// `overwritten`. Twin: `case_08_repeated_assignment_in_one_file`.
    #[test]
    fn repeated_assignment_in_one_file() {
        let s = section(
            vec![read(
                SysctlDir::Etc,
                "50-x.conf",
                "# header\nkernel.k = 1\n\n\nkernel.k = 2\n",
            )],
            &[("kernel.k", "2")],
        );
        assert_eq!(
            s.effective(),
            SysctlExplanation::Resolved(vec![writes(
                "kernel.k",
                "2",
                line(SysctlDir::Etc, "50-x.conf", 5),
                RuntimeObservation::Matches,
                vec![line(SysctlDir::Etc, "50-x.conf", 2)],
            )])
        );
    }

    /// An exclusion after an assignment cancels it, carrying the runtime
    /// value or `None`; an assignment after an exclusion writes and is
    /// compared with runtime. Twin: `case_10_key_exclusion_both_orders`
    /// (both orders and the unlisted-key layout).
    #[test]
    fn exclusion_both_orders() {
        let cancel_files = || {
            vec![
                read(SysctlDir::Etc, "10-a.conf", "vm.swappiness = 10\n"),
                read(SysctlDir::UsrLib, "20-b.conf", "-vm.swappiness\n"),
            ]
        };
        let cancelled = |runtime: Option<&str>| {
            SysctlExplanation::Resolved(vec![SysctlSetting {
                key: key(SWAPPINESS),
                outcome: SysctlOutcome::Cancelled {
                    by: line(SysctlDir::UsrLib, "20-b.conf", 1),
                    runtime: runtime.map(str::to_string),
                },
                overwritten: vec![line(SysctlDir::Etc, "10-a.conf", 1)],
            }])
        };
        assert_eq!(
            section(cancel_files(), &[(SWAPPINESS, "60")]).effective(),
            cancelled(Some("60"))
        );
        assert_eq!(section(cancel_files(), &[]).effective(), cancelled(None));

        let reversed = section(
            vec![
                read(SysctlDir::Etc, "10-a.conf", "-vm.swappiness\n"),
                read(SysctlDir::UsrLib, "20-b.conf", "vm.swappiness = 10\n"),
            ],
            &[(SWAPPINESS, "60")],
        );
        assert_eq!(
            reversed.effective(),
            SysctlExplanation::Resolved(vec![writes(
                SWAPPINESS,
                "10",
                line(SysctlDir::UsrLib, "20-b.conf", 1),
                RuntimeObservation::Differs {
                    runtime: "60".into(),
                },
                vec![line(SysctlDir::Etc, "10-a.conf", 1)],
            )])
        );
    }

    /// A mask in `/etc` shadows the same-named vendor file, so none of
    /// its keys resolve. Twin: `case_04_unowned_mask`.
    #[test]
    fn mask_shadows_vendor_file() {
        let s = section(
            vec![
                read(
                    SysctlDir::UsrLib,
                    "50-coredump.conf",
                    "kernel.core_pattern = |x\n",
                ),
                mask(SysctlDir::Etc, "50-coredump.conf"),
            ],
            &[("kernel.core_pattern", "|x")],
        );
        assert_eq!(s.effective(), SysctlExplanation::Resolved(vec![]));
    }

    /// A surviving outside link makes the section `Unknown` whatever it
    /// would set or cancel; a shadowed one is never read and changes
    /// nothing. Twin: `case_17_outside_links_snapshot`.
    #[test]
    fn outside_link_survives_or_is_shadowed() {
        let target = "/usr/share/site/tuning.conf";
        let survives = section(
            vec![
                read(SysctlDir::UsrLib, "50-default.conf", "vm.swappiness = 10\n"),
                outside(SysctlDir::Etc, "90-site.conf", target),
            ],
            &[(SWAPPINESS, "60")],
        );
        assert_eq!(
            survives.effective(),
            SysctlExplanation::Unknown(vec![UnknownInput::OutsideLink {
                file: file_ref(SysctlDir::Etc, "90-site.conf"),
                target: target.into(),
            }])
        );

        let shadowed = section(
            vec![
                outside(SysctlDir::Run, "90-site.conf", target),
                read(SysctlDir::Etc, "90-site.conf", "vm.swappiness = 60\n"),
            ],
            &[(SWAPPINESS, "60")],
        );
        assert_eq!(
            shadowed.effective(),
            SysctlExplanation::Resolved(vec![writes(
                SWAPPINESS,
                "60",
                line(SysctlDir::Etc, "90-site.conf", 1),
                RuntimeObservation::Matches,
                vec![],
            )])
        );
    }

    /// A surviving redacted file makes the section `Unknown`; a shadowed
    /// one does not. Every unknown survivor is listed in (name, dir)
    /// order, then every redacted runtime key. Twin:
    /// `case_18_redaction_makes_the_explanation_unknown_snapshot`.
    #[test]
    fn redacted_file_survives_or_is_shadowed() {
        let survives = section(
            vec![
                redacted(
                    SysctlDir::Etc,
                    "80-app.conf",
                    "vm.swappiness = 10\ndb_password = REDACTED_PASSWORD_1\n",
                ),
                read(SysctlDir::UsrLib, "10-vendor.conf", "vm.dirty_ratio = 20\n"),
            ],
            &[(SWAPPINESS, "60"), ("vm.dirty_ratio", "30")],
        );
        assert_eq!(
            survives.effective(),
            SysctlExplanation::Unknown(vec![UnknownInput::RedactedFile {
                file: file_ref(SysctlDir::Etc, "80-app.conf"),
            }])
        );

        let shadowed = section(
            vec![
                redacted(
                    SysctlDir::UsrLib,
                    "50-x.conf",
                    "db_password = REDACTED_PASSWORD_1\n",
                ),
                read(SysctlDir::Etc, "50-x.conf", "vm.swappiness = 60\n"),
            ],
            &[(SWAPPINESS, "60")],
        );
        assert_eq!(
            shadowed.effective(),
            SysctlExplanation::Resolved(vec![writes(
                SWAPPINESS,
                "60",
                line(SysctlDir::Etc, "50-x.conf", 1),
                RuntimeObservation::Matches,
                vec![],
            )])
        );

        let mut several = section(
            vec![
                outside(SysctlDir::Etc, "20-b.conf", "/opt/b.conf"),
                redacted(SysctlDir::Run, "10-a.conf", "x = REDACTED_PASSWORD_1\n"),
                read(SysctlDir::UsrLib, "30-c.conf", "vm.swappiness = 10\n"),
            ],
            &[],
        );
        several.redacted_runtime = vec![key("kernel.core_pattern")];
        assert_eq!(
            several.effective(),
            SysctlExplanation::Unknown(vec![
                UnknownInput::RedactedFile {
                    file: file_ref(SysctlDir::Run, "10-a.conf"),
                },
                UnknownInput::OutsideLink {
                    file: file_ref(SysctlDir::Etc, "20-b.conf"),
                    target: "/opt/b.conf".into(),
                },
                UnknownInput::RedactedRuntime {
                    key: key("kernel.core_pattern"),
                },
            ])
        );
    }

    /// A runtime value redaction removed makes the section `Unknown` on
    /// its own, with no `RedactedFile`. Twin:
    /// `case_18_redaction_makes_the_explanation_unknown_snapshot`
    /// (secret only at runtime).
    #[test]
    fn redacted_runtime_key_is_unknown() {
        let mut s = section(
            vec![read(
                SysctlDir::Etc,
                "85-core.conf",
                "kernel.core_pattern = REDACTED_PASSWORD_1\n",
            )],
            &[],
        );
        s.redacted_runtime = vec![key("kernel.core_pattern")];
        assert_eq!(
            s.effective(),
            SysctlExplanation::Unknown(vec![UnknownInput::RedactedRuntime {
                key: key("kernel.core_pattern"),
            }])
        );
    }

    /// Glob assignments and glob exclusions add no row. Twin:
    /// `case_14_glob_reported_not_expanded`.
    #[test]
    fn glob_adds_no_row() {
        let s = section(
            vec![read(
                SysctlDir::Etc,
                "50-x.conf",
                "net.ipv4.conf.*.rp_filter = 2\n-net.ipv4.conf.*.rp_filter\n",
            )],
            &[],
        );
        assert_eq!(s.effective(), SysctlExplanation::Resolved(vec![]));
    }

    /// `/etc` files replacing or masking a same-named `/usr/lib` file,
    /// sorted by name; nothing else pairs. Twins:
    /// `case_03_same_name_shadow_each_adjacent_pair` (`/etc` over
    /// `/usr/lib` and `/run` over `/usr/lib`), `case_04_unowned_mask`,
    /// `case_05_rpm_owned_mask`.
    #[test]
    fn vendor_replacements_pair_etc_with_usr_lib() {
        let cases: Vec<(&str, Vec<SysctlFile>, Vec<VendorReplacement>)> = vec![
            (
                "etc over usr/lib",
                vec![
                    read(SysctlDir::Etc, "50-x.conf", "kernel.a = 1\n"),
                    read(SysctlDir::UsrLib, "50-x.conf", "kernel.a = 2\n"),
                ],
                vec![VendorReplacement::Shadows {
                    name: "50-x.conf".into(),
                }],
            ),
            (
                "mask",
                vec![
                    mask(SysctlDir::Etc, "50-coredump.conf"),
                    read(SysctlDir::UsrLib, "50-coredump.conf", "kernel.a = 2\n"),
                ],
                vec![VendorReplacement::Masks {
                    name: "50-coredump.conf".into(),
                }],
            ),
            (
                "etc without a vendor twin",
                vec![
                    mask(SysctlDir::Etc, "99-sysctl.conf"),
                    read(SysctlDir::Etc, "50-x.conf", "kernel.a = 1\n"),
                ],
                vec![],
            ),
            (
                "run over usr/lib",
                vec![
                    read(SysctlDir::Run, "50-x.conf", "kernel.a = 1\n"),
                    read(SysctlDir::UsrLib, "50-x.conf", "kernel.a = 2\n"),
                ],
                vec![],
            ),
            (
                "several, sorted by name",
                vec![
                    read(SysctlDir::Etc, "60-b.conf", "kernel.a = 1\n"),
                    read(SysctlDir::UsrLib, "60-b.conf", "kernel.a = 2\n"),
                    mask(SysctlDir::Etc, "10-a.conf"),
                    read(SysctlDir::Run, "10-a.conf", ""),
                    read(SysctlDir::UsrLib, "10-a.conf", "kernel.a = 3\n"),
                ],
                vec![
                    VendorReplacement::Masks {
                        name: "10-a.conf".into(),
                    },
                    VendorReplacement::Shadows {
                        name: "60-b.conf".into(),
                    },
                ],
            ),
        ];
        for (label, files, expected) in cases {
            assert_eq!(
                section(files, &[]).vendor_replacements(),
                expected,
                "{label}"
            );
        }
    }

    /// An `/etc` file is carried by the config entry at its own path,
    /// which wins even for `99-sysctl.conf`. Twins:
    /// `case_12_one_drifted_key_does_not_drop_the_file`,
    /// `case_05_rpm_owned_mask`.
    #[test]
    fn carriage_by_own_path() {
        let s = section(
            vec![
                read(SysctlDir::Etc, "90-tuning.conf", "vm.swappiness = 10\n"),
                mask(SysctlDir::Etc, "99-sysctl.conf"),
            ],
            &[],
        );
        let config = ConfigSection {
            files: vec![
                entry("/etc/sysctl.conf"),
                entry("/etc/sysctl.d/90-tuning.conf"),
                entry(SYSCTL_CONF_LINK),
            ],
            withheld: Vec::new(),
        };
        assert_eq!(
            s.carriage(Some(&config)),
            vec![
                (
                    file_ref(SysctlDir::Etc, "90-tuning.conf"),
                    SysctlCarriage::Config(&config.files[1]),
                ),
                (
                    file_ref(SysctlDir::Etc, "99-sysctl.conf"),
                    SysctlCarriage::Config(&config.files[2]),
                ),
            ]
        );
    }

    /// The package's `99-sysctl.conf` link to `/etc/sysctl.conf` is
    /// carried by the `/etc/sysctl.conf` entry. Twin:
    /// `case_06_sysctl_conf_through_the_symlink`.
    #[test]
    fn carriage_through_the_sysctl_conf_link() {
        let s = section(
            vec![linked(
                SysctlDir::Etc,
                "99-sysctl.conf",
                SYSCTL_CONF,
                "vm.swappiness = 10\n",
            )],
            &[],
        );
        let config = ConfigSection {
            files: vec![entry("/etc/hosts"), entry(SYSCTL_CONF)],
            withheld: Vec::new(),
        };
        assert_eq!(
            s.carriage(Some(&config)),
            vec![(
                file_ref(SysctlDir::Etc, "99-sysctl.conf"),
                SysctlCarriage::Config(&config.files[1]),
            )]
        );
    }

    /// No other link is followed: `99-sysctl.conf` pointing at another
    /// `/etc` file, or another name pointing at `/etc/sysctl.conf`, has no
    /// carrier. Twin: `case_05_rpm_owned_mask` (its link-elsewhere twin).
    #[test]
    fn carriage_follows_no_other_link() {
        let s = section(
            vec![
                linked(
                    SysctlDir::Etc,
                    "99-sysctl.conf",
                    "/etc/sysctl-site.conf",
                    "vm.swappiness = 10\n",
                ),
                linked(SysctlDir::Etc, "50-x.conf", SYSCTL_CONF, "kernel.a = 1\n"),
            ],
            &[],
        );
        let config = ConfigSection {
            files: vec![entry("/etc/sysctl-site.conf"), entry(SYSCTL_CONF)],
            withheld: Vec::new(),
        };
        assert_eq!(
            s.carriage(Some(&config)),
            vec![
                (
                    file_ref(SysctlDir::Etc, "50-x.conf"),
                    SysctlCarriage::NoConfigEntry,
                ),
                (
                    file_ref(SysctlDir::Etc, "99-sysctl.conf"),
                    SysctlCarriage::NoConfigEntry,
                ),
            ]
        );
    }

    /// Only `/etc` files appear; a file in another directory is never
    /// carried by the config path, even with a matching entry. Twin:
    /// `case_01_each_directory_is_read`.
    #[test]
    fn carriage_covers_etc_only() {
        let s = section(
            vec![
                read(SysctlDir::Etc, "10-etc.conf", "kernel.a = 1\n"),
                read(SysctlDir::Run, "20-run.conf", "kernel.b = 2\n"),
                read(SysctlDir::UsrLocalLib, "30-local.conf", "kernel.c = 3\n"),
                read(SysctlDir::UsrLib, "40-lib.conf", "kernel.d = 4\n"),
            ],
            &[],
        );
        let config = ConfigSection {
            files: vec![entry("/run/sysctl.d/20-run.conf")],
            withheld: Vec::new(),
        };
        assert_eq!(
            s.carriage(Some(&config)),
            vec![(
                file_ref(SysctlDir::Etc, "10-etc.conf"),
                SysctlCarriage::NoConfigEntry,
            )]
        );
    }

    /// With no config section every `/etc` file has no carrier. Twin:
    /// `config_section_absent_from_a_scan`.
    #[test]
    fn carriage_without_config_section() {
        let s = section(
            vec![
                read(SysctlDir::Etc, "90-tuning.conf", "vm.swappiness = 10\n"),
                linked(SysctlDir::Etc, "99-sysctl.conf", SYSCTL_CONF, ""),
            ],
            &[],
        );
        assert_eq!(
            s.carriage(None),
            vec![
                (
                    file_ref(SysctlDir::Etc, "90-tuning.conf"),
                    SysctlCarriage::NoConfigEntry,
                ),
                (
                    file_ref(SysctlDir::Etc, "99-sysctl.conf"),
                    SysctlCarriage::NoConfigEntry,
                ),
            ]
        );
    }
}
