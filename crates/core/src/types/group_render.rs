use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// The rendering state of an installed DNF group in the refine session.
/// Computed from the session timeline and the effective projected snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state")]
pub enum GroupRenderState {
    /// All non-locked members included -- render as `dnf group install`.
    Renderable,
    /// Group explicitly excluded via group-level SetInclude(false).
    Excluded,
    /// Group dissolved by UngroupGroup directive -- members render individually.
    Ungrouped,
    /// Group cannot be rendered atomically -- individual member rendering required.
    Degraded { reason: DegradationReason },
}

/// Why a group cannot be rendered as a single `dnf group install` command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DegradationReason {
    /// A non-locked member has been excluded individually.
    MemberExcluded,
    /// An individual SetInclude op on a member diverges from the most recent
    /// group-level op for this group.
    MemberOverridden,
    /// A member appears in multiple architectures on the effective surface.
    MultilibConflict,
}

/// Carries group rendering state alongside the projected snapshot.
/// Kept separate from InspectionSnapshot to preserve snapshot purity.
#[derive(Debug, Clone, Default)]
pub struct RenderContext {
    /// Map from group name to its computed rendering state.
    pub group_states: HashMap<String, GroupRenderState>,
}

impl RenderContext {
    /// Returns true if the named group is in the Renderable state.
    pub fn is_renderable(&self, group_name: &str) -> bool {
        matches!(
            self.group_states.get(group_name),
            Some(GroupRenderState::Renderable)
        )
    }

    /// Returns true if the named group has been explicitly excluded.
    pub fn is_excluded(&self, group_name: &str) -> bool {
        matches!(
            self.group_states.get(group_name),
            Some(GroupRenderState::Excluded)
        )
    }

    /// Returns true if the named group has been ungrouped (dissolved).
    pub fn is_ungrouped(&self, group_name: &str) -> bool {
        matches!(
            self.group_states.get(group_name),
            Some(GroupRenderState::Ungrouped)
        )
    }

    /// Returns true if the named group is degraded (cannot render atomically).
    pub fn is_degraded(&self, group_name: &str) -> bool {
        matches!(
            self.group_states.get(group_name),
            Some(GroupRenderState::Degraded { .. })
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_context_default_is_empty() {
        let ctx = RenderContext::default();
        assert!(ctx.group_states.is_empty());
    }

    #[test]
    fn group_render_state_serde_round_trip() {
        let state = GroupRenderState::Degraded {
            reason: DegradationReason::MultilibConflict,
        };
        let json = serde_json::to_string(&state).unwrap();
        let back: GroupRenderState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, back);
    }

    #[test]
    fn render_context_is_renderable_helper() {
        let mut ctx = RenderContext::default();
        ctx.group_states
            .insert("Dev Tools".into(), GroupRenderState::Renderable);
        ctx.group_states.insert(
            "Container Management".into(),
            GroupRenderState::Excluded,
        );
        assert!(ctx.is_renderable("Dev Tools"));
        assert!(!ctx.is_renderable("Container Management"));
        assert!(!ctx.is_renderable("Nonexistent"));
    }
}
