//! What the export says when `rpm -Va` failed on the scanned host.
//!
//! The rpm collector records [`RPM_VA_FAILED_WARNING`] in the snapshot's
//! warnings, which the audit report and the HTML report list. This module
//! repeats that same warning as an export warning, so a scan prints it on
//! stderr and a refine export shows it in its warnings, from one text.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::RPM_VA_FAILED_WARNING;

use crate::export_warning::ExportWarning;

/// The export warning for a snapshot whose `rpm -Va` failed, or `None`.
///
/// A [`ExportWarning::failure`]: modified RPM-owned configuration files
/// the operator expects to carry may be missing from the export.
pub fn rpm_va_failed_notice(snap: &InspectionSnapshot) -> Option<ExportWarning> {
    snap.warnings
        .iter()
        .any(|w| w.inspector == "rpm" && w.message == RPM_VA_FAILED_WARNING)
        .then(|| ExportWarning::failure(RPM_VA_FAILED_WARNING))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export_warning::ExportWarningKind;
    use inspectah_core::types::warnings::Warning;

    fn snap_with(warnings: Vec<Warning>) -> InspectionSnapshot {
        InspectionSnapshot {
            warnings,
            ..Default::default()
        }
    }

    fn rpm_warning(message: &str) -> Warning {
        Warning {
            inspector: "rpm".into(),
            message: message.into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_failed_rpm_va_gives_one_failure_with_the_collector_text() {
        let snap = snap_with(vec![
            rpm_warning("GPG key x named in /etc/yum.repos.d/a.repo was not collected"),
            rpm_warning(RPM_VA_FAILED_WARNING),
        ]);
        let notice = rpm_va_failed_notice(&snap).expect("rpm -Va failed");
        assert_eq!(notice.kind(), ExportWarningKind::Failure);
        assert_eq!(notice.text(), RPM_VA_FAILED_WARNING);
    }

    #[test]
    fn no_notice_without_the_warning() {
        assert!(rpm_va_failed_notice(&snap_with(Vec::new())).is_none());
        // Another inspector's warning with the same text is not the
        // collector's.
        let other = Warning {
            inspector: "config".into(),
            message: RPM_VA_FAILED_WARNING.into(),
            ..Default::default()
        };
        assert!(rpm_va_failed_notice(&snap_with(vec![other])).is_none());
    }
}
