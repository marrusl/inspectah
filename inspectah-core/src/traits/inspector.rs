use crate::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use crate::types::redaction::RedactionHint;
use crate::types::system::SourceSystem;
use crate::types::warnings::Warning;
use std::fmt;

pub trait Inspector: Send + Sync {
    fn id(&self) -> InspectorId;
    fn applicable_to(&self) -> &[SourceSystemKind];
    fn inspect(&self, ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError>;
}

/// Borrowed references into executor + source system state.
/// Enables scoped-thread execution where multiple InspectionContext
/// values share one executor.
pub struct InspectionContext<'a> {
    pub source: &'a SourceSystem,
    pub executor: &'a dyn crate::traits::executor::Executor,
    pub rpm_state: Option<&'a RpmState>,
}

/// Read-only RPM state provided to non-RPM inspectors during two-phase collection.
#[derive(Debug, Clone, Default)]
pub struct RpmState {
    pub installed_packages: std::collections::HashSet<String>,
    pub owned_paths: std::collections::HashSet<String>,
}

/// Typed section output — the compiler proves inspectors emit valid section shapes.
#[derive(Debug, Clone)]
pub struct InspectorOutput {
    pub section: SectionData,
    pub warnings: Vec<Warning>,
    pub redaction_hints: Vec<RedactionHint>,
}

#[derive(Debug, Clone)]
pub enum InspectorError {
    Skipped {
        reason: String,
    },
    Degraded {
        partial: Box<InspectorOutput>,
        reason: String,
    },
    Failed {
        reason: String,
    },
}

impl fmt::Display for InspectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Skipped { reason } => write!(f, "skipped: {reason}"),
            Self::Degraded { reason, .. } => write!(f, "degraded: {reason}"),
            Self::Failed { reason } => write!(f, "failed: {reason}"),
        }
    }
}

impl std::error::Error for InspectorError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inspector_error_display() {
        let err = InspectorError::Skipped {
            reason: "not applicable".into(),
        };
        assert!(format!("{err}").contains("not applicable"));

        let err = InspectorError::Failed {
            reason: "rpm db corrupt".into(),
        };
        assert!(format!("{err}").contains("rpm db corrupt"));
    }

    #[test]
    fn test_degraded_carries_partial_output() {
        use crate::types::completeness::SectionData;
        use crate::types::rpm::RpmSection;
        let output = InspectorOutput {
            section: SectionData::Rpm(RpmSection::default()),
            warnings: vec![],
            redaction_hints: vec![],
        };
        let err = InspectorError::Degraded {
            partial: Box::new(output.clone()),
            reason: "partial rpm db".into(),
        };
        if let InspectorError::Degraded { partial, .. } = err {
            assert_eq!(partial.warnings.len(), 0);
        }
    }
}
