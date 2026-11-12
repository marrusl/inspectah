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
    /// The config entry at `config_path` is included.
    Carried { config_path: String },
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
    Read { link_target: Option<String> },
    Mask,
    OutsideLink { target: String },
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
            SysctlFileBody::Mask | SysctlFileBody::OutsideLink { .. } => None,
        })
}

/// The carriage state a config entry, or its absence, gives a file:
/// included is `Carried`, an advisory is `Advisory`, anything else is
/// `Excluded`.
fn carriage_state(entry: Option<&ConfigFileEntry>) -> CarriageState {
    let Some(entry) = entry else {
        return CarriageState::NoConfigEntry;
    };
    let config_path = entry.path.clone();
    if entry.disposition.is_included() {
        CarriageState::Carried { config_path }
    } else if entry.disposition.is_advisory() {
        CarriageState::Advisory { config_path }
    } else {
        CarriageState::Excluded { config_path }
    }
}
