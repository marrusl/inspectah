use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use inspectah_refine::session::RefineSession;

use crate::theme::{ColorTier, Token};
use crate::types::SectionId;

/// A single search hit, carrying section attribution and display text.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub section_id: SectionId,
    pub name: String,
    pub match_context: String,
}

/// Cross-section substring search across all searchable fields.
///
/// Returns results ordered by section (packages first, then configs,
/// services, containers, sysctls, users, then reference sections).
pub fn search_all_sections(session: &RefineSession, query: &str) -> Vec<SearchResult> {
    if query.is_empty() {
        return Vec::new();
    }

    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    let view = session.view();
    let decisions = session.decisions();
    let reference = session.reference();

    // Packages: name.arch
    for pkg in &view.packages {
        let label = format!("{}.{}", pkg.entry.name, pkg.entry.arch);
        if label.to_lowercase().contains(&query_lower) {
            results.push(SearchResult {
                section_id: SectionId::Packages,
                name: label,
                match_context: String::new(),
            });
        }
    }

    // Configs: path
    for cfg in &view.config_files {
        if cfg.entry.path.to_lowercase().contains(&query_lower) {
            results.push(SearchResult {
                section_id: SectionId::Configs,
                name: cfg.entry.path.clone(),
                match_context: String::new(),
            });
        }
    }

    // Services (decision): unit
    for svc in &decisions.service_states {
        if svc.entry.unit.to_lowercase().contains(&query_lower) {
            results.push(SearchResult {
                section_id: SectionId::Services,
                name: svc.entry.unit.clone(),
                match_context: String::new(),
            });
        }
    }

    // Service drop-ins (decision): unit
    for dropin in &decisions.service_dropins {
        if dropin.entry.unit.to_lowercase().contains(&query_lower) {
            results.push(SearchResult {
                section_id: SectionId::Services,
                name: dropin.entry.unit.clone(),
                match_context: "drop-in".into(),
            });
        }
    }

    // Containers — decision quadlets: name
    for q in &decisions.quadlets {
        if q.entry.name.to_lowercase().contains(&query_lower) {
            results.push(SearchResult {
                section_id: SectionId::Containers,
                name: q.entry.name.clone(),
                match_context: "quadlet".into(),
            });
        }
    }

    // Containers — decision flatpaks: app_id
    for f in &decisions.flatpaks {
        if f.entry.app_id.to_lowercase().contains(&query_lower) {
            results.push(SearchResult {
                section_id: SectionId::Containers,
                name: f.entry.app_id.clone(),
                match_context: "flatpak".into(),
            });
        }
    }

    // Sysctls: key
    for s in &decisions.sysctls {
        if s.entry.key.to_lowercase().contains(&query_lower) {
            results.push(SearchResult {
                section_id: SectionId::Sysctls,
                name: s.entry.key.clone(),
                match_context: String::new(),
            });
        }
    }

    // Users: name
    for u in &decisions.users_groups {
        if u.name.to_lowercase().contains(&query_lower) {
            results.push(SearchResult {
                section_id: SectionId::Users,
                name: u.name.clone(),
                match_context: String::new(),
            });
        }
    }

    // Reference: scheduled_tasks (GenericRefItem key)
    for item in &reference.scheduled_tasks {
        if item.key.to_lowercase().contains(&query_lower) {
            results.push(SearchResult {
                section_id: SectionId::ScheduledTasks,
                name: item.key.clone(),
                match_context: String::new(),
            });
        }
    }

    // Reference: non_rpm_software (GenericRefItem key)
    for item in &reference.non_rpm_software {
        if item.key.to_lowercase().contains(&query_lower) {
            results.push(SearchResult {
                section_id: SectionId::NonRpmSoftware,
                name: item.key.clone(),
                match_context: String::new(),
            });
        }
    }

    // Reference: selinux (GenericRefItem key)
    for item in &reference.selinux {
        if item.key.to_lowercase().contains(&query_lower) {
            results.push(SearchResult {
                section_id: SectionId::SELinux,
                name: item.key.clone(),
                match_context: String::new(),
            });
        }
    }

    results
}

/// Count results grouped by section for the attribution header.
fn section_counts(results: &[SearchResult]) -> Vec<(SectionId, usize)> {
    use std::collections::BTreeMap;

    // Use section order index as key for deterministic ordering.
    let mut counts: BTreeMap<usize, (SectionId, usize)> = BTreeMap::new();
    for r in results {
        let idx = crate::sections::SECTION_ORDER
            .iter()
            .position(|&s| s == r.section_id)
            .unwrap_or(usize::MAX);
        counts
            .entry(idx)
            .and_modify(|e| e.1 += 1)
            .or_insert((r.section_id, 1));
    }
    counts.into_values().collect()
}

/// Widget that renders the search overlay.
pub struct SearchWidget<'a> {
    query: &'a str,
    results: &'a [SearchResult],
    selected: usize,
    tier: ColorTier,
}

impl<'a> SearchWidget<'a> {
    pub fn new(
        query: &'a str,
        results: &'a [SearchResult],
        selected: usize,
        tier: ColorTier,
    ) -> Self {
        Self {
            query,
            results,
            selected,
            tier,
        }
    }
}

impl Widget for SearchWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 4 || area.width < 20 {
            return;
        }

        // Clear overlay area.
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                buf[(x, y)].reset();
            }
        }

        let w = area.width as usize;
        let mut row = area.y;

        // Line 1: search prompt
        let prompt = format!("/ {}", self.query);
        let prompt_display = if prompt.len() > w {
            &prompt[..w]
        } else {
            &prompt
        };
        buf.set_string(area.x, row, prompt_display, Token::Warning.style(self.tier));

        // Blinking cursor position indicator.
        let cursor_x = area.x + prompt.len().min(w - 1) as u16;
        buf.set_string(cursor_x, row, "_", Token::Warning.style(self.tier));

        row += 1;

        // Line 2: match count with section attribution
        let total = self.results.len();
        if total == 0 && !self.query.is_empty() {
            buf.set_string(
                area.x + 1,
                row,
                "No matches",
                Token::TextMuted.style(self.tier),
            );
        } else if total > 0 {
            let counts = section_counts(self.results);
            let parts: Vec<String> = counts
                .iter()
                .map(|(sid, n)| format!("{} {}", n, sid.label()))
                .collect();
            let summary = format!("{} matches: {}", total, parts.join(", "));
            let summary_display = if summary.len() > w.saturating_sub(1) {
                &summary[..w.saturating_sub(1)]
            } else {
                &summary
            };
            buf.set_string(
                area.x + 1,
                row,
                summary_display,
                Token::TextMuted.style(self.tier),
            );
        }

        row += 1;

        // Separator
        let sep = "\u{2500}".repeat(w);
        buf.set_string(area.x, row, &sep, Token::TextMuted.style(self.tier));
        row += 1;

        // Results list
        for (i, result) in self.results.iter().enumerate() {
            if row >= area.bottom() {
                break;
            }

            let prefix = format!("[{}] ", result.section_id.label());
            let suffix = if result.match_context.is_empty() {
                String::new()
            } else {
                format!(" ({})", result.match_context)
            };
            let line = format!("{}{}{}", prefix, result.name, suffix);

            let max_w = w.saturating_sub(1);
            let display = if line.len() > max_w {
                &line[..max_w]
            } else {
                &line
            };

            let style = if i == self.selected {
                Token::FocusSelected.style(self.tier)
            } else {
                Token::TextPrimary.style(self.tier)
            };

            buf.set_string(area.x + 1, row, display, style);
            row += 1;
        }
    }
}
