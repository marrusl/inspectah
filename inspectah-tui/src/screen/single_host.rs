//! SingleHost screen -- two-panel layout with sidebar + triage list + status bar.
//!
//! Composes section nav, triage list, and status bar widgets into the
//! main single-host inspection view.

use std::collections::HashMap;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};

use inspectah_refine::session::RefineSession;
use inspectah_refine::types::TriageBucket;

use crate::sections::{SECTION_ORDER, build_section_entries};
use crate::theme::ColorTier;
use crate::types::{FocusTarget, SectionId, TuiState};
use crate::widget::section_nav::SectionNavWidget;
use crate::widget::status_bar::StatusBarWidget;
use crate::widget::triage_list::{ListItem, TriageGroup, TriageListWidget};

const SIDEBAR_WIDTH: u16 = 18;

/// A raw item tuple: (name, detail, triage_group, include_state).
type RawItem = (String, String, TriageGroup, Option<bool>);

pub struct SingleHostScreen;

impl Default for SingleHostScreen {
    fn default() -> Self {
        Self
    }
}

impl SingleHostScreen {
    pub fn new() -> Self {
        Self
    }

    pub fn render(
        &self,
        frame: &mut Frame,
        session: &RefineSession,
        state: &TuiState,
        tier: ColorTier,
    ) {
        let area = frame.area();

        // Top-level vertical split: main area + status bar (1 row).
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(area);

        let main_area = vertical[0];
        let status_area = vertical[1];

        // Horizontal split: sidebar (fixed width) + item list (remaining).
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(20)])
            .split(main_area);

        let sidebar_area = horizontal[0];
        let list_area = horizontal[1];

        // --- Sidebar ---
        let entries = build_section_entries(session);
        let sidebar_focused = state.focus == FocusTarget::Sidebar;
        let sidebar = SectionNavWidget::new(
            &entries,
            state.active_section,
            sidebar_focused,
            tier,
            state.sidebar_scroll,
        );
        frame.render_widget(sidebar, sidebar_area);

        // --- Triage list ---
        let active_section_id = SECTION_ORDER
            .get(state.active_section)
            .copied()
            .unwrap_or(SectionId::Packages);
        let items = build_list_items(session, active_section_id, state);
        let list_focused = state.focus == FocusTarget::ItemList;
        let list_widget = TriageListWidget::new(
            &items,
            state.cursor,
            active_section_id,
            list_focused,
            tier,
            0, // scroll_offset -- wired in Task 11
        );
        frame.render_widget(list_widget, list_area);

        // --- Status bar ---
        let view = session.view();
        let stats = &view.stats;

        // Find current section's entry for included/excluded counts.
        let section_entry = entries.iter().find(|e| e.id == active_section_id);

        let (included, excluded) = section_entry
            .map(|e| (e.included, e.excluded))
            .unwrap_or((0, 0));

        let status = StatusBarWidget::new(tier)
            .stats(included, excluded, stats.needs_review_count)
            .decision_section(active_section_id.is_decision())
            .flash(state.flash.as_ref());
        frame.render_widget(status, status_area);
    }
}

/// Map a `TriageBucket` from session data to the TUI's `TriageGroup`.
fn bucket_to_group(bucket: TriageBucket) -> TriageGroup {
    match bucket {
        TriageBucket::Investigate => TriageGroup::Investigate,
        TriageBucket::Site => TriageGroup::Site,
        TriageBucket::Baseline => TriageGroup::Baseline,
    }
}

/// Build `ListItem`s for the active section from session data.
///
/// Only Packages and Configs are fully wired. Other sections return
/// empty lists until Task 11 wires remaining section data.
fn build_list_items(
    session: &RefineSession,
    section: SectionId,
    state: &TuiState,
) -> Vec<ListItem> {
    let raw_items: Vec<RawItem> = match section {
        SectionId::Packages => {
            let view = session.view();
            view.packages
                .iter()
                .map(|pkg| {
                    let name = if pkg.entry.arch.is_empty() || pkg.entry.arch == "noarch" {
                        pkg.entry.name.clone()
                    } else {
                        format!("{}.{}", pkg.entry.name, pkg.entry.arch)
                    };
                    let detail = if pkg.entry.source_repo.is_empty() {
                        pkg.entry.version.clone()
                    } else {
                        format!("{} ({})", pkg.entry.version, pkg.entry.source_repo)
                    };
                    let group = bucket_to_group(pkg.triage.bucket());
                    (name, detail, group, Some(pkg.entry.include))
                })
                .collect()
        }
        SectionId::Configs => {
            let view = session.view();
            view.config_files
                .iter()
                .map(|cfg| {
                    let name = cfg.entry.path.clone();
                    let detail = format!("{:?}", cfg.entry.category);
                    let group = bucket_to_group(cfg.triage.bucket());
                    (name, detail, group, Some(cfg.entry.include))
                })
                .collect()
        }
        // Other sections -- empty until wired in Task 11.
        _ => Vec::new(),
    };

    build_grouped_items(&raw_items, state, section)
}

/// Group raw items by triage bucket with collapsible headers.
///
/// The canonical bucket order is Investigate, Site, Baseline.
fn build_grouped_items(items: &[RawItem], state: &TuiState, _section: SectionId) -> Vec<ListItem> {
    const BUCKET_ORDER: &[TriageGroup] = &[
        TriageGroup::Investigate,
        TriageGroup::Site,
        TriageGroup::Baseline,
    ];

    // Group items by triage group.
    let mut grouped: HashMap<TriageGroup, Vec<&RawItem>> = HashMap::new();
    for item in items {
        grouped.entry(item.2).or_default().push(item);
    }

    let mut result = Vec::new();

    for (group_idx, &bucket) in BUCKET_ORDER.iter().enumerate() {
        let group_items = match grouped.get(&bucket) {
            Some(items) if !items.is_empty() => items,
            _ => continue,
        };

        let is_collapsed = state
            .collapsed_groups
            .contains(&(state.active_section, group_idx));

        // Group header.
        result.push(ListItem::header(bucket, group_items.len(), is_collapsed));

        if !is_collapsed {
            for (idx, item) in group_items.iter().enumerate() {
                result.push(ListItem::item(
                    item.0.clone(),
                    item.1.clone(),
                    bucket,
                    item.3,
                    idx,
                ));
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_to_group_maps_all_variants() {
        assert_eq!(
            bucket_to_group(TriageBucket::Investigate),
            TriageGroup::Investigate
        );
        assert_eq!(bucket_to_group(TriageBucket::Site), TriageGroup::Site);
        assert_eq!(
            bucket_to_group(TriageBucket::Baseline),
            TriageGroup::Baseline
        );
    }

    #[test]
    fn empty_items_produce_empty_grouped() {
        let items: Vec<(String, String, TriageGroup, Option<bool>)> = Vec::new();
        let state = TuiState::new(14);
        let result = build_grouped_items(&items, &state, SectionId::Packages);
        assert!(result.is_empty());
    }

    #[test]
    fn grouped_items_have_headers_then_items() {
        let items = vec![
            (
                "httpd".to_string(),
                "2.4".to_string(),
                TriageGroup::Investigate,
                Some(true),
            ),
            (
                "nginx".to_string(),
                "1.24".to_string(),
                TriageGroup::Investigate,
                Some(false),
            ),
            (
                "bash".to_string(),
                "5.2".to_string(),
                TriageGroup::Baseline,
                Some(true),
            ),
        ];
        // Baseline (group_idx 2) is collapsed by default in TuiState::new.
        let state = TuiState::new(14);
        let result = build_grouped_items(&items, &state, SectionId::Packages);

        // Investigate header + 2 items + Baseline header (collapsed, no items).
        assert_eq!(result.len(), 4);
        assert!(result[0].is_group_header);
        assert_eq!(result[0].group, TriageGroup::Investigate);
        assert_eq!(result[0].group_count, 2);
        assert!(!result[1].is_group_header);
        assert_eq!(result[1].name, "httpd");
        assert!(!result[2].is_group_header);
        assert_eq!(result[2].name, "nginx");
        assert!(result[3].is_group_header);
        assert_eq!(result[3].group, TriageGroup::Baseline);
        assert!(result[3].is_collapsed);
    }
}
