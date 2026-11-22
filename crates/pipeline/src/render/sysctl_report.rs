//! The sysctl report model: what every surface says about a host's sysctl
//! search path, derived once.
//!
//! The audit report, the HTML report and the refine view state the same
//! facts: the explanation, the drift, the vendor files replaced, what is
//! not carried and what is reported as written. Each derives them from
//! [`sysctl_report`] and only formats, so the three cannot disagree.
//! Nothing here is a decision: carriage is read from the config entries,
//! which stay the one decision.
//!
//! The spec's fixed copy is held here as constants, verbatim.

use super::configtree::{PathError, validate_path};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::ConfigFileEntry;
use inspectah_core::types::sysctl::{
    RuntimeObservation, SysctlCarriage, SysctlDir, SysctlExplanation, SysctlFileBody,
    SysctlFileRef, SysctlKey, SysctlLine, SysctlLineRef, SysctlName, SysctlOutcome,
    VendorReplacement,
};

/// Item 1 of the explanation.
pub const TRUST_LINE: &str = "These values are trusted host configuration. The files that set them are carried as they were, not rewritten.";

/// Stands in for the table and the drift lines when the explanation is
/// `Unknown`.
pub const UNKNOWN_INTRO: &str = "Values and drift are not stated for this host, because inspectah does not know every input below as the host had it. One unknown line can set, override or cancel any key.";

/// Shown once above the drift lines.
pub const DRIFT_INTRO: &str = "These keys run at a value other than the one the files request. A tuned profile, another service, a hand-run `sysctl -w`, or a module that created the key after systemd-sysctl ran at early boot can each cause this, and some of them reapply at every boot, so the value after a reboot cannot be read from this scan. The image gets the files, not the running values.";

/// The whole sysctl explanation on a fleet aggregate.
pub const AGGREGATE_LINE: &str = "Sysctl explanation is per host and is not available across a fleet yet; each host's own scan has it. Sysctl files are carried as Configuration Files, with a variant per differing content.";

/// Ends the line shown when the explanation is unavailable.
pub const UNAVAILABLE_TAIL: &str = "Sysctl files are still carried as Configuration Files.";

/// Precedes [`UNAVAILABLE_TAIL`] when the explanation is unavailable.
pub const UNAVAILABLE_LINE: &str = "Sysctl explanation unavailable: {reason}.";

// The sentences below hold named `{placeholder}` holes; [`fill`] puts a
// surface's already-encoded values in them.

/// Item 2, `Unknown`: a surviving outside link.
pub const OUTSIDE_LINK_LINE: &str = "{path} links to {target}, outside `/etc` and the sysctl directories, and inspectah did not read it.";
/// Item 2, `Unknown`: a surviving file redaction changed.
pub const REDACTED_FILE_LINE: &str =
    "{path} held text that redaction replaced, so what it sets is not known.";
/// Item 2, `Unknown`: a surviving file the read gate withheld.
pub const WITHHELD_FILE_LINE: &str = "{path} is a machine-state file whose contents were not captured, so what it sets is not known.";
/// Item 2, `Unknown`: a runtime value redaction removed.
pub const REDACTED_RUNTIME_LINE: &str =
    "The running value of {key} held text that redaction removed.";

/// Item 2 table: Runtime value when the kernel runs the value written.
pub const RUNTIME_SAME: &str = "same";
/// Item 2 table: Runtime value when `sysctl -a` did not list the key.
pub const RUNTIME_NOT_EXPOSED: &str = "not exposed";
/// Item 2 table: File value when the last entry for the key is an
/// exclusion.
pub const FILE_NOT_WRITTEN: &str = "not written";
/// Item 2 table: follows the Source when the file was read through a link.
pub const LINK_NOTE: &str = "(link to {target})";

/// Item 3's label.
pub const DRIFT_LABEL: &str = "Runtime drift";
/// Item 3: one drifted key.
pub const DRIFT_LINE: &str = "{key} runs at {runtime}; the files request {file_value} ({source}).";
/// Item 3: added to [`DRIFT_INTRO`] when a tuned profile is active.
pub const TUNED_ACTIVE_LINE: &str = "tuned profile {profile} is active on this host.";

/// Item 4's label.
pub const VENDOR_LABEL: &str = "Vendor files replaced";
/// Item 4's verb for a file that shadows a vendor file.
pub const VENDOR_VERB_REPLACES: &str = "replaces";
/// Item 4's verb for a mask.
pub const VENDOR_VERB_DISABLES: &str = "disables";
/// Item 4, the admin file is carried.
pub const VENDOR_CARRIED_LINE: &str = "{admin} {verb} {vendor} entirely, on the target as well, so keys the target's version of that vendor file sets will not apply from it. Compare it with the target image's copy.";
/// Item 4, the admin file's config entry is excluded or advisory, or is
/// included at a path the export refuses.
pub const VENDOR_NOT_CARRIED_LINE: &str = "{admin} {verb} {vendor} on this host. It is not carried, so on the target the vendor file applies.";
/// Item 4, the admin file has no config entry.
pub const VENDOR_NO_CONFIG_ENTRY_LINE: &str = "{admin} {verb} {vendor} on this host. It has no Configuration Files row, so it reaches the target only if a package there ships it.";

/// Item 5's label.
pub const NOT_CARRIED_LABEL: &str = "Not carried";
/// Item 5, a file in `/run/sysctl.d`.
pub const NOT_CARRIED_RUNTIME_DIR_LINE: &str = "{path}: generated at runtime.";
/// Item 5, a file in `/usr/local/lib/sysctl.d`.
pub const NOT_CARRIED_USR_LOCAL_LIB_LINE: &str =
    "{path}: carried only as unmanaged files under `--include-unmanaged`.";
/// Item 5, an `/etc` file whose own config entry is excluded.
pub const NOT_CARRIED_EXCLUDED_LINE: &str = "{path}: its Configuration Files row is excluded.";
/// Item 5, an `/etc` file carried through another path's entry, which is
/// excluded.
pub const NOT_CARRIED_EXCLUDED_VIA_LINE: &str =
    "{path}: the Configuration Files row that carries it, {config_path}, is excluded.";
/// Item 5, an `/etc` file whose own config entry is an advisory.
pub const NOT_CARRIED_ADVISORY_LINE: &str = "{path}: its Configuration Files row is an advisory.";
/// Item 5, an `/etc` file carried through another path's entry, which is
/// an advisory.
pub const NOT_CARRIED_ADVISORY_VIA_LINE: &str =
    "{path}: the Configuration Files row that carries it, {config_path}, is an advisory.";
/// Item 5, an `/etc` file whose own config entry is included at a path
/// the export refuses; `{reason}` is the refusal.
pub const NOT_CARRIED_REFUSED_LINE: &str = "{path}: its Configuration Files row is included, but the export refuses the path ({reason}) and does not write the file.";
/// Item 5, an `/etc` file carried through another path's entry, which is
/// included at a path the export refuses.
pub const NOT_CARRIED_REFUSED_VIA_LINE: &str = "{path}: the Configuration Files row that carries it, {config_path}, is included, but the export refuses that path ({reason}) and does not write the file.";
/// Item 5, an `/etc` file with no config entry.
pub const NOT_CARRIED_NO_CONFIG_ENTRY_LINE: &str = "{path}: it has no Configuration Files row.";

/// Item 6's label.
pub const AS_WRITTEN_LABEL: &str = "As written";
/// Item 6, a glob assignment.
pub const AS_WRITTEN_GLOB_ASSIGN_LINE: &str = "{at}: glob {pattern} set to {value}, not expanded.";
/// Item 6, a glob exclusion.
pub const AS_WRITTEN_GLOB_EXCLUDE_LINE: &str = "{at}: glob exclusion {pattern}, not expanded.";
/// Item 6, a line the parser could not read.
pub const AS_WRITTEN_UNPARSED_LINE: &str = "{at}: unparsed line {text}.";

/// Put `values` in the `{name}` holes of a copy template, in one pass, so
/// a value that itself holds `{name}` text is never substituted again.
/// Each surface passes values already encoded for it (a Markdown code
/// span, HTML-escaped text).
///
/// # Panics
///
/// When the template names a hole `values` does not fill, or leaves one
/// unclosed: the templates are this module's constants, so either is a
/// programming error the tests catch.
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut filled = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        filled.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let close = after
            .find('}')
            .expect("every hole in a sysctl copy template is closed");
        let name = &after[..close];
        let value = values
            .iter()
            .find(|(hole, _)| *hole == name)
            .map(|(_, value)| *value)
            .unwrap_or_else(|| panic!("sysctl copy template hole `{name}` has no value"));
        filled.push_str(value);
        rest = &after[close + 1..];
    }
    filled.push_str(rest);
    filled
}

/// The warnings' inspector name for the kernel/boot inspector, whose
/// degradation is the reason the explanation is unavailable.
const KERNEL_BOOT_INSPECTOR: &str = "KernelBoot";

/// What the surfaces say about sysctl for one snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SysctlReport {
    /// A fleet aggregate: the explanation is per host.
    Aggregate,
    /// A single host whose sysctl section was not collected.
    Unavailable {
        /// The kernel/boot degradation messages, joined `"; "`.
        reason: String,
    },
    /// A single host with a collected sysctl section.
    Host(SysctlHostReport),
}

/// One host's sysctl explanation and the facts derived from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SysctlHostReport {
    pub explanation: SysctlExplanation,
    /// The `Differs` rows of a `Resolved` explanation, in key order.
    pub drift: Vec<SysctlDrift>,
    pub vendor_replacements: Vec<VendorLine>,
    pub not_carried: Vec<NotCarried>,
    pub as_written: Vec<AsWritten>,
    /// Every collected file, in (name, dir) order.
    pub files: Vec<FileRow>,
    /// The active tuned profile, when the snapshot records one.
    pub tuned_active: Option<String>,
}

/// A key the kernel runs at a value other than the one the files request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SysctlDrift {
    pub key: SysctlKey,
    pub file_value: String,
    pub runtime: String,
    pub from: SysctlLineRef,
}

/// How an `/etc` sysctl file reaches the image, as the surfaces word it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CarriageState {
    /// The config entry at `config_path` is included and the export
    /// writes it.
    Carried { config_path: String },
    /// The config entry at `config_path` is included, but the exporter
    /// refuses its path (`configtree::validate_path`), so the export does
    /// not hold the file.
    Refused {
        config_path: String,
        reason: PathError,
    },
    /// The config entry at `config_path` is excluded.
    Excluded { config_path: String },
    /// The config entry at `config_path` is an advisory.
    Advisory { config_path: String },
    /// No config entry carries the file.
    NoConfigEntry,
}

/// A vendor file replaced by an `/etc` file, with that file's carriage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VendorLine {
    pub action: VendorReplacement,
    pub carriage: CarriageState,
}

/// A sysctl file the image does not get.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotCarried {
    /// A file in `/run/sysctl.d`, generated at runtime.
    RuntimeDir { path: String },
    /// A file in `/usr/local/lib/sysctl.d`.
    UsrLocalLib { path: String },
    /// An `/etc` file whose carriage is not `Carried`.
    Etc {
        path: String,
        carriage: CarriageState,
    },
}

/// A line reported as written: a glob, which is not expanded, or a line
/// the parser could not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsWritten {
    GlobAssign {
        at: SysctlLineRef,
        pattern: String,
        value: String,
    },
    GlobExclude {
        at: SysctlLineRef,
        pattern: String,
    },
    Unparsed {
        at: SysctlLineRef,
        text: String,
    },
}

/// One collected file, for a file list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRow {
    pub file: SysctlFileRef,
    pub body: FileRowBody,
    /// The config entry's path, when `carriage()` names one.
    pub config_path: Option<String>,
}

/// What a collected file is, without its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileRowBody {
    Read {
        link_target: Option<String>,
    },
    Mask,
    OutsideLink {
        target: String,
    },
    /// A machine-state file the scan did not read.
    Withheld,
}

/// The sysctl report of `snap`.
///
/// `Aggregate` on a fleet aggregate; `Unavailable` when the kernel/boot
/// section or its sysctl section is absent; `Host` otherwise.
pub fn sysctl_report(snap: &InspectionSnapshot) -> SysctlReport {
    if snap.aggregate_meta.is_some() {
        return SysctlReport::Aggregate;
    }
    let Some((kernel_boot, section)) = snap
        .kernel_boot
        .as_ref()
        .and_then(|kb| kb.sysctl.as_ref().map(|section| (kb, section)))
    else {
        let reason = snap
            .warnings
            .iter()
            .filter(|w| w.inspector == KERNEL_BOOT_INSPECTOR)
            .map(|w| w.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        return SysctlReport::Unavailable { reason };
    };

    let explanation = section.effective();
    let drift = match &explanation {
        SysctlExplanation::Resolved(settings) => settings
            .iter()
            .filter_map(|setting| match &setting.outcome {
                SysctlOutcome::Writes {
                    value,
                    from,
                    runtime: RuntimeObservation::Differs { runtime },
                } => Some(SysctlDrift {
                    key: setting.key.clone(),
                    file_value: value.clone(),
                    runtime: runtime.clone(),
                    from: from.clone(),
                }),
                _ => None,
            })
            .collect(),
        SysctlExplanation::Unknown(_) => Vec::new(),
    };

    let carriage = section.carriage(snap.config.as_ref());
    let carrier_of = |file: &SysctlFileRef| -> Option<&ConfigFileEntry> {
        carriage
            .iter()
            .find(|(carried, _)| carried == file)
            .and_then(|(_, carriage)| match carriage {
                SysctlCarriage::Config(entry) => Some(*entry),
                SysctlCarriage::NoConfigEntry => None,
            })
    };
    let state_of = |file: &SysctlFileRef| carriage_state(carrier_of(file));

    let vendor_replacements = section
        .vendor_replacements()
        .into_iter()
        .map(|action| {
            let name = match &action {
                VendorReplacement::Shadows { name } | VendorReplacement::Masks { name } => name,
            };
            let carriage = state_of(&SysctlFileRef {
                dir: SysctlDir::Etc,
                name: name.clone(),
            });
            VendorLine { action, carriage }
        })
        .collect();

    let not_carried = section
        .files
        .iter()
        .filter_map(|file| {
            let path = file.path();
            match file.dir {
                SysctlDir::Run => Some(NotCarried::RuntimeDir { path }),
                SysctlDir::UsrLocalLib => Some(NotCarried::UsrLocalLib { path }),
                SysctlDir::Etc => match state_of(&file.file_ref()) {
                    CarriageState::Carried { .. } => None,
                    carriage => Some(NotCarried::Etc { path, carriage }),
                },
                SysctlDir::UsrLib => None,
            }
        })
        .collect();

    let as_written = section
        .files
        .iter()
        .filter(|file| {
            matches!(
                file.body,
                SysctlFileBody::Read {
                    redacted: false,
                    ..
                }
            )
        })
        .flat_map(|file| {
            let at = |line_no: u32| SysctlLineRef {
                dir: file.dir,
                name: file.name.clone(),
                line_no,
            };
            file.lines()
                .into_iter()
                .filter_map(move |line| match line {
                    SysctlLine::Assign {
                        line_no,
                        name: SysctlName::Glob(pattern),
                        value,
                        ..
                    } => Some(AsWritten::GlobAssign {
                        at: at(line_no),
                        pattern,
                        value,
                    }),
                    SysctlLine::Exclude {
                        line_no,
                        name: SysctlName::Glob(pattern),
                    } => Some(AsWritten::GlobExclude {
                        at: at(line_no),
                        pattern,
                    }),
                    SysctlLine::Unparsed { line_no, text } => Some(AsWritten::Unparsed {
                        at: at(line_no),
                        text,
                    }),
                    SysctlLine::Assign { .. } | SysctlLine::Exclude { .. } => None,
                })
                .collect::<Vec<_>>()
        })
        .collect();

    let files = section
        .files
        .iter()
        .map(|file| {
            let file_ref = file.file_ref();
            let config_path = carrier_of(&file_ref).map(|entry| entry.path.clone());
            let body = match &file.body {
                SysctlFileBody::Read { link_target, .. } => FileRowBody::Read {
                    link_target: link_target.clone(),
                },
                SysctlFileBody::Mask => FileRowBody::Mask,
                SysctlFileBody::OutsideLink { target } => FileRowBody::OutsideLink {
                    target: target.clone(),
                },
                SysctlFileBody::Withheld { .. } => FileRowBody::Withheld,
            };
            FileRow {
                file: file_ref,
                body,
                config_path,
            }
        })
        .collect();

    let tuned_active = Some(kernel_boot.tuned_active.clone()).filter(|t| !t.is_empty());

    SysctlReport::Host(SysctlHostReport {
        explanation,
        drift,
        vendor_replacements,
        not_carried,
        as_written,
        files,
        tuned_active,
    })
}

/// `link_target` of the file a line ref names, for "(link to ..)" notes.
pub fn line_link_target(snap: &InspectionSnapshot, at: &SysctlLineRef) -> Option<String> {
    snap.kernel_boot
        .as_ref()?
        .sysctl
        .as_ref()?
        .files
        .iter()
        .find(|file| file.dir == at.dir && file.name == at.name)
        .and_then(|file| match &file.body {
            SysctlFileBody::Read { link_target, .. } => link_target.clone(),
            SysctlFileBody::Mask
            | SysctlFileBody::OutsideLink { .. }
            | SysctlFileBody::Withheld { .. } => None,
        })
}

/// The carriage state a config entry, or its absence, gives a file:
/// included is `Carried`, or `Refused` when the exporter's own path check
/// refuses the entry's path; an advisory is `Advisory`; anything else is
/// `Excluded`.
fn carriage_state(entry: Option<&ConfigFileEntry>) -> CarriageState {
    let Some(entry) = entry else {
        return CarriageState::NoConfigEntry;
    };
    let config_path = entry.path.clone();
    if entry.disposition.is_included() {
        // The stager's check, on the path as the stager passes it.
        match validate_path(entry.path.trim_start_matches('/')) {
            Ok(()) => CarriageState::Carried { config_path },
            Err(reason) => CarriageState::Refused {
                config_path,
                reason,
            },
        }
    } else if entry.disposition.is_advisory() {
        CarriageState::Advisory { config_path }
    } else {
        CarriageState::Excluded { config_path }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_puts_each_value_in_its_hole_once() {
        let cases = [
            (
                "plain",
                "{a} and {b}.",
                vec![("a", "1"), ("b", "2")],
                "1 and 2.",
            ),
            ("repeated hole", "{a}{a}", vec![("a", "x")], "xx"),
            (
                "value holding a hole name",
                "{a} then {b}",
                vec![("a", "{b}"), ("b", "B")],
                "{b} then B",
            ),
            (
                "value holding braces",
                "{a}.",
                vec![("a", "`\\u{a}`")],
                "`\\u{a}`.",
            ),
            ("no holes", DRIFT_INTRO, vec![], DRIFT_INTRO),
        ];
        for (name, template, values, expected) in cases {
            assert_eq!(fill(template, &values), expected, "{name}");
        }
    }

    #[test]
    #[should_panic(expected = "hole `b` has no value")]
    fn fill_refuses_an_unfilled_hole() {
        fill("{a} {b}", &[("a", "1")]);
    }
}
