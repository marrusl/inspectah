use inspectah_core::types::completeness::{Completeness, InspectorId};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionState {
    Normal,
    Degraded,
    Failed,
}

pub fn section_state(id: InspectorId, completeness: &Completeness) -> SectionState {
    match completeness {
        Completeness::Complete => SectionState::Normal,
        Completeness::Partial {
            degraded_sections, ..
        } => {
            if degraded_sections.contains(&id) {
                SectionState::Degraded
            } else {
                SectionState::Normal
            }
        }
        Completeness::Incomplete {
            failed_sections,
            degraded_sections,
            ..
        } => {
            if failed_sections.contains(&id) {
                SectionState::Failed
            } else if degraded_sections.contains(&id) {
                SectionState::Degraded
            } else {
                SectionState::Normal
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::completeness::{Completeness, InspectorId};

    #[test]
    fn section_state_normal_when_complete() {
        let c = Completeness::Complete;
        assert_eq!(section_state(InspectorId::Rpm, &c), SectionState::Normal);
    }

    #[test]
    fn section_state_degraded_when_in_degraded_list() {
        let c = Completeness::Partial {
            degraded_sections: vec![InspectorId::Config],
            reason: "test".into(),
        };
        assert_eq!(
            section_state(InspectorId::Config, &c),
            SectionState::Degraded
        );
        assert_eq!(section_state(InspectorId::Rpm, &c), SectionState::Normal);
    }

    #[test]
    fn section_state_failed_when_in_failed_list() {
        let c = Completeness::Incomplete {
            failed_sections: vec![InspectorId::Storage],
            degraded_sections: vec![],
            reason: "timeout".into(),
        };
        assert_eq!(
            section_state(InspectorId::Storage, &c),
            SectionState::Failed
        );
        assert_eq!(section_state(InspectorId::Rpm, &c), SectionState::Normal);
    }
}
