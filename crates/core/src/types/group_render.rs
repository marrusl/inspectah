use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::types::verified_upload::VerifiedUploads;

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
    /// No effective member exists (all locked, all base-image, or none
    /// present) -- nothing to render and nothing to decide (§ 3.7).
    Inert,
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

/// Carries render-time state the projected snapshot cannot hold alongside
/// it. Kept separate from InspectionSnapshot to preserve snapshot purity.
#[derive(Debug, Clone, Default)]
pub struct RenderContext {
    /// Map from group name to its computed rendering state.
    pub group_states: HashMap<String, GroupRenderState>,
    /// The staged uploads this render may name. Empty by default, which is
    /// the safe answer: a `uploaded: true` entry the snapshot carries gets
    /// no `COPY` until an export or a preview proves the file.
    pub verified_uploads: VerifiedUploads,
    /// The packages the operator included in refine. Empty by default,
    /// which is the safe answer: a render outside a refine session has no
    /// operator include to honor.
    pub operator_includes: OperatorIncludes,
    /// The repos excluded in refine, by exact section spelling. Empty by
    /// default: a scan has no repo decision, so its Containerfile never
    /// says a shipped file defines an excluded repo.
    pub excluded_repos: std::collections::BTreeSet<String>,
}

/// The `name.arch` ids of the packages the operator included in a refine
/// session: those whose last include op that reaches them includes them,
/// whether it names the package, a repo it is attributed to, or a group
/// it is a member of.
///
/// This is provenance, not disposition. A package can be included because
/// the scan's defaults or refine's open-time defaults left it so; only
/// membership here says an operator chose it. A fresh scan's render has
/// none, so a decision that needs an operator's include never fires there.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OperatorIncludes(HashSet<String>);

impl OperatorIncludes {
    /// The operator includes `ids`, by `name.arch`.
    pub fn new(ids: impl IntoIterator<Item = String>) -> Self {
        Self(ids.into_iter().collect())
    }

    /// Whether the operator included the package `id` (`name.arch`).
    pub fn contains(&self, id: &str) -> bool {
        self.0.contains(id)
    }
}

impl RenderContext {
    /// Returns true if the named group is in the Renderable state. `Inert`
    /// (no effective member) is not renderable, same as every other variant.
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
        ctx.group_states
            .insert("Container Management".into(), GroupRenderState::Excluded);
        assert!(ctx.is_renderable("Dev Tools"));
        assert!(!ctx.is_renderable("Container Management"));
        assert!(!ctx.is_renderable("Nonexistent"));
    }
}
