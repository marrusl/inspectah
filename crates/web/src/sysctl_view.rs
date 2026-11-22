//! The read-only sysctl panel, projected from the sysctl report model.
//!
//! [`project_sysctl`] maps [`sysctl_report`] one-to-one onto the panel's
//! DTOs, so the panel states exactly what the audit and the HTML report
//! state. Nothing here is a decision: each `/etc` sysctl file is decided
//! by its Configuration Files row, which the DTOs name by path, and the
//! carriage they report is read from those rows in the snapshot passed in.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::sysctl::{
    RuntimeObservation, SysctlDir, SysctlExplanation, SysctlFileRef, SysctlLineRef, SysctlOutcome,
    SysctlSetting, UnknownInput, VendorReplacement,
};
use inspectah_pipeline::render::sysctl_report::{
    AsWritten, CarriageState, FileRow, FileRowBody, NotCarried, SysctlDrift, SysctlHostReport,
    SysctlReport, VendorLine, line_link_target, sysctl_report,
};

use crate::web_types::{
    AsWrittenLineDto, CarriageDto, NotCarriedDto, NotCarriedReason, ReplacementAction, RuntimeDto,
    SysctlDriftDto, SysctlExplanationDto, SysctlFileBodyDto, SysctlFileDto, SysctlHostView,
    SysctlLineRefDto, SysctlOutcomeDto, SysctlSettingDto, SysctlViewDto, UnknownInputDto,
    VendorReplacementDto,
};

/// The sysctl panel for `snap`.
///
/// `Aggregate` on a fleet aggregate, `Unavailable` when the kernel/boot
/// section or its sysctl section is absent, `Host` otherwise. Pass the
/// session's projected snapshot so carriage reflects the current
/// Configuration Files decisions.
pub fn project_sysctl(snap: &InspectionSnapshot) -> SysctlViewDto {
    match sysctl_report(snap) {
        SysctlReport::Aggregate => SysctlViewDto::Aggregate,
        SysctlReport::Unavailable { reason } => SysctlViewDto::Unavailable { reason },
        SysctlReport::Host(host) => SysctlViewDto::Host(host_view(snap, host)),
    }
}

fn host_view(snap: &InspectionSnapshot, host: SysctlHostReport) -> SysctlHostView {
    let line = |at: &SysctlLineRef| line_ref(snap, at);
    let explanation = match host.explanation {
        SysctlExplanation::Resolved(settings) => SysctlExplanationDto::Resolved {
            settings: settings.iter().map(|s| setting(s, &line)).collect(),
            drift: host.drift.iter().map(|d| drift(d, &line)).collect(),
        },
        SysctlExplanation::Unknown(inputs) => SysctlExplanationDto::Unknown {
            inputs: inputs.into_iter().map(unknown_input).collect(),
        },
    };
    SysctlHostView {
        explanation,
        vendor_replacements: host
            .vendor_replacements
            .into_iter()
            .map(vendor_replacement)
            .collect(),
        not_carried: host
            .not_carried
            .into_iter()
            .filter_map(not_carried)
            .collect(),
        as_written: host
            .as_written
            .into_iter()
            .map(|a| as_written(a, &line))
            .collect(),
        files: host.files.into_iter().map(file).collect(),
        tuned_active: host.tuned_active,
    }
}

fn line_ref(snap: &InspectionSnapshot, at: &SysctlLineRef) -> SysctlLineRefDto {
    SysctlLineRefDto {
        path: file_path(at.dir, &at.name),
        line: at.line_no,
        link_target: line_link_target(snap, at),
    }
}

fn file_path(dir: SysctlDir, name: &str) -> String {
    SysctlFileRef {
        dir,
        name: name.to_owned(),
    }
    .path()
}

fn setting(
    s: &SysctlSetting,
    line: &impl Fn(&SysctlLineRef) -> SysctlLineRefDto,
) -> SysctlSettingDto {
    let outcome = match &s.outcome {
        SysctlOutcome::Writes {
            value,
            from,
            runtime,
        } => SysctlOutcomeDto::Writes {
            value: value.clone(),
            from: line(from),
            runtime: match runtime {
                RuntimeObservation::Matches => RuntimeDto::Matches,
                RuntimeObservation::Differs { runtime } => RuntimeDto::Differs {
                    runtime: runtime.clone(),
                },
                RuntimeObservation::NotExposed => RuntimeDto::NotExposed,
            },
        },
        SysctlOutcome::Cancelled { by, runtime } => SysctlOutcomeDto::Cancelled {
            by: line(by),
            runtime: runtime.clone(),
        },
    };
    SysctlSettingDto {
        key: s.key.as_str().to_owned(),
        outcome,
        overwritten: s.overwritten.iter().map(line).collect(),
    }
}

fn drift(d: &SysctlDrift, line: &impl Fn(&SysctlLineRef) -> SysctlLineRefDto) -> SysctlDriftDto {
    SysctlDriftDto {
        key: d.key.as_str().to_owned(),
        file_value: d.file_value.clone(),
        runtime: d.runtime.clone(),
        from: line(&d.from),
    }
}

fn unknown_input(input: UnknownInput) -> UnknownInputDto {
    match input {
        UnknownInput::OutsideLink { file, target } => UnknownInputDto::OutsideLink {
            path: file.path(),
            target,
        },
        UnknownInput::RedactedFile { file } => UnknownInputDto::RedactedFile { path: file.path() },
        UnknownInput::WithheldFile { file } => UnknownInputDto::WithheldFile { path: file.path() },
        UnknownInput::RedactedRuntime { key } => UnknownInputDto::RedactedRuntime {
            key: key.as_str().to_owned(),
        },
    }
}

fn vendor_replacement(v: VendorLine) -> VendorReplacementDto {
    let (action, name) = match v.action {
        VendorReplacement::Shadows { name } => (ReplacementAction::Shadows, name),
        VendorReplacement::Masks { name } => (ReplacementAction::Masks, name),
    };
    VendorReplacementDto {
        action,
        path: file_path(SysctlDir::Etc, &name),
        vendor_path: file_path(SysctlDir::UsrLib, &name),
        carriage: match v.carriage {
            CarriageState::Carried { config_path } => CarriageDto::Carried { config_path },
            CarriageState::Refused {
                config_path,
                reason,
            } => CarriageDto::Refused {
                config_path,
                reason: reason.to_string(),
            },
            CarriageState::Excluded { config_path } => CarriageDto::Excluded { config_path },
            CarriageState::Advisory { config_path } => CarriageDto::Advisory { config_path },
            CarriageState::NoConfigEntry => CarriageDto::NoConfigEntry,
        },
    }
}

/// `None` for an `/etc` file whose carriage is `Carried`: the report
/// never lists one, and a carried file is not "not carried".
fn not_carried(n: NotCarried) -> Option<NotCarriedDto> {
    let (path, reason) = match n {
        NotCarried::RuntimeDir { path } => (path, NotCarriedReason::RuntimeDir),
        NotCarried::UsrLocalLib { path } => (path, NotCarriedReason::UsrLocalLib),
        NotCarried::Etc { path, carriage } => {
            let reason = match carriage {
                CarriageState::Carried { .. } => return None,
                CarriageState::Refused {
                    config_path,
                    reason,
                } => NotCarriedReason::Refused {
                    config_path,
                    reason: reason.to_string(),
                },
                CarriageState::Excluded { config_path } => {
                    NotCarriedReason::Excluded { config_path }
                }
                CarriageState::Advisory { config_path } => {
                    NotCarriedReason::Advisory { config_path }
                }
                CarriageState::NoConfigEntry => NotCarriedReason::NoConfigEntry,
            };
            (path, reason)
        }
    };
    Some(NotCarriedDto { path, reason })
}

fn as_written(
    a: AsWritten,
    line: &impl Fn(&SysctlLineRef) -> SysctlLineRefDto,
) -> AsWrittenLineDto {
    match a {
        AsWritten::GlobAssign { at, pattern, value } => AsWrittenLineDto::GlobAssign {
            at: line(&at),
            pattern,
            value,
        },
        AsWritten::GlobExclude { at, pattern } => AsWrittenLineDto::GlobExclude {
            at: line(&at),
            pattern,
        },
        AsWritten::Unparsed { at, text } => AsWrittenLineDto::Unparsed {
            at: line(&at),
            text,
        },
    }
}

fn file(row: FileRow) -> SysctlFileDto {
    SysctlFileDto {
        path: row.file.path(),
        dir: row.file.dir,
        body: match row.body {
            FileRowBody::Read { link_target } => SysctlFileBodyDto::Read { link_target },
            FileRowBody::Mask => SysctlFileBodyDto::Mask,
            FileRowBody::OutsideLink { target } => SysctlFileBodyDto::OutsideLink { target },
            FileRowBody::Withheld => SysctlFileBodyDto::Withheld,
        },
        config_path: row.config_path,
    }
}
