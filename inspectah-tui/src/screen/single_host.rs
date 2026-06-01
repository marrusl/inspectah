//! SingleHost screen -- two-panel layout with sidebar + triage list + status bar.
//!
//! Composes section nav, triage list, and status bar widgets into the
//! main single-host inspection view.

use std::collections::HashMap;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};

use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, TriageBucket};

use crate::sections::{SECTION_ORDER, build_section_entries};
use crate::theme::ColorTier;
use crate::types::{DetailMode, FocusTarget, SectionId, TuiState};
use crate::widget::containerfile::ContainerfileWidget;
use crate::widget::detail_view::{DetailContentType, DetailData, DetailViewWidget};
use crate::widget::info_bar::{InfoBarData, InfoBarWidget};
use crate::widget::section_nav::SectionNavWidget;
use crate::widget::status_bar::StatusBarWidget;
use crate::widget::triage_list::{ListItem, TriageGroup, TriageListWidget};
use crate::widget::user_strategy::{UserEntry, UserStrategyWidget};

const SIDEBAR_WIDTH: u16 = 18;

/// A raw item tuple: (name, detail, triage_group, include_state, item_id, has_content).
type RawItem = (
    String,
    String,
    TriageGroup,
    Option<bool>,
    Option<ItemId>,
    bool,
);

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

        // --- Build section data (needed for both layouts and status bar) ---
        let entries = build_section_entries(session);
        let active_section_id = SECTION_ORDER
            .get(state.active_section)
            .copied()
            .unwrap_or(SectionId::Packages);
        let items = build_list_items(session, active_section_id, state);

        // --- Containerfile toggle: hide sidebar, split 50/50 ---
        if state.show_containerfile {
            let halves = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(main_area);

            let list_area = halves[0];
            let cf_area = halves[1];

            // Left: triage list.
            let list_focused = state.focus == FocusTarget::ItemList;
            let list_widget = TriageListWidget::new(
                &items,
                state.cursor,
                active_section_id,
                list_focused,
                tier,
                0,
            );
            frame.render_widget(list_widget, list_area);

            // Right: containerfile preview.
            let view = session.view();
            let cf_widget = ContainerfileWidget::new(&view.containerfile_preview, tier);
            frame.render_widget(cf_widget, cf_area);
        } else {
            // --- Default layout: sidebar + item list ---
            let horizontal = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(20)])
                .split(main_area);

            let sidebar_area = horizontal[0];
            let list_area = horizontal[1];

            // --- Sidebar ---
            let sidebar_focused = state.focus == FocusTarget::Sidebar;
            let sidebar = SectionNavWidget::new(
                &entries,
                state.active_section,
                sidebar_focused,
                tier,
                state.sidebar_scroll,
            );
            frame.render_widget(sidebar, sidebar_area);

            // --- Users section: specialized strategy view ---
            if active_section_id == SectionId::Users {
                let user_entries = build_user_entries(session);
                let list_focused = state.focus == FocusTarget::ItemList;
                let user_widget =
                    UserStrategyWidget::new(&user_entries, state.cursor, list_focused, tier);
                frame.render_widget(user_widget, list_area);
            // --- Triage list / Detail view ---
            // Fullscreen detail replaces the item list entirely.
            } else if state.detail_mode == DetailMode::Fullscreen {
                if let Some(data) = build_detail_data(session, state, &items) {
                    frame.render_widget(
                        DetailViewWidget::new(&data, state.detail_scroll, tier),
                        list_area,
                    );
                }
            } else {
                // Split list area for info bar when active.
                let (list_render_area, info_area) = if state.detail_mode == DetailMode::InfoBar {
                    let split = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Min(5), Constraint::Length(4)])
                        .split(list_area);
                    (split[0], Some(split[1]))
                } else {
                    (list_area, None)
                };

                let list_focused = state.focus == FocusTarget::ItemList;
                let list_widget = TriageListWidget::new(
                    &items,
                    state.cursor,
                    active_section_id,
                    list_focused,
                    tier,
                    0, // scroll_offset -- wired in Task 11
                );
                frame.render_widget(list_widget, list_render_area);

                // --- Info bar ---
                if let Some(info_area) = info_area
                    && let Some(data) = build_info_bar_data(session, state, &items)
                {
                    frame.render_widget(InfoBarWidget::new(&data, tier), info_area);
                }
            }
        }

        // --- Status bar ---
        let view = session.view();
        let stats = &view.stats;

        // Find current section's entry for included/excluded counts.
        let section_entry = entries.iter().find(|e| e.id == active_section_id);

        let (included, excluded) = section_entry
            .map(|e| (e.included, e.excluded))
            .unwrap_or((0, 0));

        // Count reviewed items for this section (items whose viewed key
        // starts with the section's prefix).
        let (reviewed, total_reviewable) = if let Some(prefix) = active_section_id.viewed_prefix() {
            let prefix_colon = format!("{prefix}:");
            let reviewed = session
                .viewed_ids()
                .iter()
                .filter(|id| id.starts_with(&prefix_colon))
                .count();
            let total = items.iter().filter(|i| !i.is_group_header).count();
            (reviewed, total)
        } else {
            (0, 0)
        };

        let status = StatusBarWidget::new(tier)
            .stats(included, excluded, stats.needs_review_count)
            .reviewed_progress(reviewed, total_reviewable)
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
/// empty lists until remaining section data is wired.
pub fn build_list_items(
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
                    let id = ItemId::Package {
                        name: pkg.entry.name.clone(),
                        arch: pkg.entry.arch.clone(),
                    };
                    (
                        name,
                        detail,
                        group,
                        Some(pkg.entry.include),
                        Some(id),
                        false,
                    )
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
                    let id = ItemId::Config {
                        path: cfg.entry.path.clone(),
                    };
                    // Configs have diff content available for fullscreen detail.
                    let has_content =
                        cfg.entry.diff_against_rpm.is_some() || !cfg.entry.content.is_empty();
                    (
                        name,
                        detail,
                        group,
                        Some(cfg.entry.include),
                        Some(id),
                        has_content,
                    )
                })
                .collect()
        }
        // Other sections -- empty until wired in Task 11.
        _ => Vec::new(),
    };

    build_grouped_items(&raw_items, state, section)
}

/// Build info bar data for the currently selected item.
///
/// Extracts key-value fields based on section type. Packages show version,
/// repo, and triage group. Configs show kind, category. Other sections
/// fall back to name + detail.
fn build_info_bar_data(
    session: &RefineSession,
    state: &TuiState,
    items: &[ListItem],
) -> Option<InfoBarData> {
    let item = items.get(state.cursor)?;
    if item.is_group_header {
        return None;
    }

    let active_section_id = SECTION_ORDER
        .get(state.active_section)
        .copied()
        .unwrap_or(SectionId::Packages);

    let fields = match (active_section_id, &item.item_id) {
        (SectionId::Packages, Some(ItemId::Package { name, arch })) => {
            let view = session.view();
            if let Some(pkg) = view
                .packages
                .iter()
                .find(|p| p.entry.name == *name && p.entry.arch == *arch)
            {
                vec![
                    ("Version".into(), pkg.entry.version.clone()),
                    (
                        "Repo".into(),
                        if pkg.entry.source_repo.is_empty() {
                            "unknown".into()
                        } else {
                            pkg.entry.source_repo.clone()
                        },
                    ),
                    ("Triage".into(), format!("{:?}", pkg.triage.bucket())),
                ]
            } else {
                vec![("Detail".into(), item.detail.clone())]
            }
        }
        (SectionId::Configs, Some(ItemId::Config { path })) => {
            let view = session.view();
            if let Some(cfg) = view.config_files.iter().find(|c| c.entry.path == *path) {
                vec![
                    ("Kind".into(), format!("{:?}", cfg.entry.kind)),
                    ("Category".into(), format!("{:?}", cfg.entry.category)),
                ]
            } else {
                vec![("Detail".into(), item.detail.clone())]
            }
        }
        _ => {
            // Fallback for unwired sections.
            if item.detail.is_empty() {
                vec![]
            } else {
                vec![("Detail".into(), item.detail.clone())]
            }
        }
    };

    Some(InfoBarData {
        name: item.name.clone(),
        fields,
    })
}

/// Build fullscreen detail data for the currently selected item.
///
/// For configs: shows diff content (or raw content if no diff available).
/// For packages: shows a key-value plain text summary.
/// Other sections fall back to a plain text summary.
fn build_detail_data(
    session: &RefineSession,
    state: &TuiState,
    items: &[ListItem],
) -> Option<DetailData> {
    let item = items.get(state.cursor)?;
    if item.is_group_header {
        return None;
    }

    let active_section_id = SECTION_ORDER
        .get(state.active_section)
        .copied()
        .unwrap_or(SectionId::Packages);

    // Position string (1-indexed, excluding headers).
    let item_index = items
        .iter()
        .take(state.cursor + 1)
        .filter(|i| !i.is_group_header)
        .count();
    let total_items = items.iter().filter(|i| !i.is_group_header).count();
    let position = format!("{} of {}", item_index, total_items);

    match (active_section_id, &item.item_id) {
        (SectionId::Configs, Some(ItemId::Config { path })) => {
            let view = session.view();
            if let Some(cfg) = view.config_files.iter().find(|c| c.entry.path == *path) {
                let (content, content_type) = if let Some(ref diff) = cfg.entry.diff_against_rpm {
                    (diff.clone(), DetailContentType::Diff)
                } else if !cfg.entry.content.is_empty() {
                    (cfg.entry.content.clone(), DetailContentType::PlainText)
                } else {
                    (
                        "(no content available)".into(),
                        DetailContentType::PlainText,
                    )
                };
                Some(DetailData {
                    title: cfg.entry.path.clone(),
                    content,
                    content_type,
                    include: Some(cfg.entry.include),
                    position,
                })
            } else {
                None
            }
        }
        (SectionId::Packages, Some(ItemId::Package { name, arch })) => {
            let view = session.view();
            if let Some(pkg) = view
                .packages
                .iter()
                .find(|p| p.entry.name == *name && p.entry.arch == *arch)
            {
                let content = format!(
                    "Name: {}\nVersion: {}\nArch: {}\nRepo: {}\nTriage: {:?}\nInclude: {}",
                    pkg.entry.name,
                    pkg.entry.version,
                    pkg.entry.arch,
                    if pkg.entry.source_repo.is_empty() {
                        "unknown"
                    } else {
                        &pkg.entry.source_repo
                    },
                    pkg.triage.bucket(),
                    pkg.entry.include,
                );
                Some(DetailData {
                    title: item.name.clone(),
                    content,
                    content_type: DetailContentType::PlainText,
                    include: Some(pkg.entry.include),
                    position,
                })
            } else {
                None
            }
        }
        _ => {
            // Fallback plain text for other sections.
            if item.detail.is_empty() {
                Some(DetailData {
                    title: item.name.clone(),
                    content: "(no detail available)".into(),
                    content_type: DetailContentType::PlainText,
                    include: item.included,
                    position,
                })
            } else {
                Some(DetailData {
                    title: item.name.clone(),
                    content: format!("Detail: {}", item.detail),
                    content_type: DetailContentType::PlainText,
                    include: item.included,
                    position,
                })
            }
        }
    }
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
                if item.5 {
                    result.push(ListItem::item_with_content(
                        item.0.clone(),
                        item.1.clone(),
                        bucket,
                        item.3,
                        idx,
                        item.4.clone(),
                    ));
                } else {
                    result.push(ListItem::item(
                        item.0.clone(),
                        item.1.clone(),
                        bucket,
                        item.3,
                        idx,
                        item.4.clone(),
                    ));
                }
            }
        }
    }

    result
}

/// Build user entries from session decisions for the UserStrategyWidget.
pub fn build_user_entries(session: &RefineSession) -> Vec<UserEntry> {
    session
        .decisions()
        .users_groups
        .iter()
        .map(|u| {
            let has_password = u
                .password_status
                .as_deref()
                .map(|s| s == "password_set")
                .unwrap_or(false);
            UserEntry {
                username: u.name.clone(),
                uid: u.uid,
                strategy: u.containerfile_strategy.clone(),
                has_password,
                password_choice: u.password_choice.clone(),
            }
        })
        .collect()
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
        let items: Vec<RawItem> = Vec::new();
        let state = TuiState::new(14);
        let result = build_grouped_items(&items, &state, SectionId::Packages);
        assert!(result.is_empty());
    }

    #[test]
    fn grouped_items_have_headers_then_items() {
        let items: Vec<RawItem> = vec![
            (
                "httpd".to_string(),
                "2.4".to_string(),
                TriageGroup::Investigate,
                Some(true),
                None,
                false,
            ),
            (
                "nginx".to_string(),
                "1.24".to_string(),
                TriageGroup::Investigate,
                Some(false),
                None,
                false,
            ),
            (
                "bash".to_string(),
                "5.2".to_string(),
                TriageGroup::Baseline,
                Some(true),
                None,
                false,
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
