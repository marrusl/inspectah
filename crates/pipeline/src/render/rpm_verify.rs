//! What the export says when `rpm -Va` failed on the scanned host.
//!
//! The rpm collector records [`RPM_VA_FAILED_WARNING`] in the snapshot's
//! warnings, which the audit report and the HTML report list. This module
//! finds that warning by its code and repeats it as an export warning, so a
//! scan prints it on stderr and a refine export shows it in its warnings,
//! from one text.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::{RPM_VA_FAILED_WARNING, is_rpm_va_failed_warning};

use crate::export_warning::ExportWarning;

/// The export warning for a snapshot whose `rpm -Va` failed, or `None`.
///
/// A [`ExportWarning::failure`]: modified RPM-owned configuration files
/// the operator expects to carry may be missing from the export.
pub fn rpm_va_failed_notice(snap: &InspectionSnapshot) -> Option<ExportWarning> {
    snap.warnings
        .iter()
        .any(is_rpm_va_failed_warning)
        .then(|| ExportWarning::failure(RPM_VA_FAILED_WARNING))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export_warning::ExportWarningKind;
    use inspectah_core::types::rpm::rpm_va_failed_warning;
    use inspectah_core::types::warnings::Warning;

    fn snap_with(warnings: Vec<Warning>) -> InspectionSnapshot {
        InspectionSnapshot {
            warnings,
            ..Default::default()
        }
    }

    #[test]
    fn a_failed_rpm_va_gives_one_failure_with_the_collector_text() {
        let snap = snap_with(vec![
            Warning {
                inspector: "rpm".into(),
                message: "GPG key x named in /etc/yum.repos.d/a.repo was not collected".into(),
                ..Default::default()
            },
            rpm_va_failed_warning(),
        ]);
        let notice = rpm_va_failed_notice(&snap).expect("rpm -Va failed");
        assert_eq!(notice.kind(), ExportWarningKind::Failure);
        assert_eq!(notice.text(), RPM_VA_FAILED_WARNING);
    }

    /// The code, not the prose, identifies the warning: a reworded
    /// message still matches, and the same text without the code does not.
    #[test]
    fn the_notice_matches_on_the_code_not_the_text() {
        let reworded = Warning {
            message: "rpm -Va failed (reworded)".into(),
            ..rpm_va_failed_warning()
        };
        assert!(rpm_va_failed_notice(&snap_with(vec![reworded])).is_some());

        assert!(rpm_va_failed_notice(&snap_with(Vec::new())).is_none());
        let prose_only = Warning {
            inspector: "rpm".into(),
            message: RPM_VA_FAILED_WARNING.into(),
            ..Default::default()
        };
        assert!(rpm_va_failed_notice(&snap_with(vec![prose_only])).is_none());
    }

    /// The code is part of the snapshot JSON, so it survives a tarball
    /// round trip into refine.
    #[test]
    fn the_code_survives_a_json_round_trip() {
        let json = serde_json::to_value(rpm_va_failed_warning()).unwrap();
        assert_eq!(json["code"], "rpm_va_failed");
        let back: Warning = serde_json::from_value(json).unwrap();
        assert!(is_rpm_va_failed_warning(&back));
    }
}
