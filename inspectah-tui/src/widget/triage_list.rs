//! Triage list widget — the main item list in the content area.
//!
//! Renders grouped items within a section. Each section's items are
//! organized by triage group (Investigate, Site, Baseline) with
//! collapsible group headers and include/exclude indicators.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::widgets::Widget;

use crate::theme::{ColorTier, Token};
use crate::types::SectionId;

/// Which triage group an item belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TriageGroup {
    Investigate,
    Site,
    Baseline,
}

impl TriageGroup {
    pub fn label(self) -> &'static str {
        match self {
            Self::Investigate => "Investigate",
            Self::Site => "Site",
            Self::Baseline => "Baseline",
        }
    }
}

/// A single item in the triage list.
#[derive(Debug, Clone)]
pub struct ListItem {
    /// Primary display name (package name, config path, service unit, etc.).
    pub name: String,
    /// Secondary detail (version, category, state, etc.).
    pub detail: String,
    /// Which triage group this item belongs to.
    pub group: TriageGroup,
    /// Include/exclude state. `None` for reference-only items.
    pub included: Option<bool>,
    /// True if this entry is a group header row, not an item.
    pub is_group_header: bool,
    /// Index within the group (for alternating row styles).
    pub group_index: usize,
    /// Whether this group header is collapsed (children hidden).
    pub is_collapsed: bool,
    /// Number of items in this group (shown on header).
    pub group_count: usize,
}

impl ListItem {
    /// Create a regular item row.
    pub fn item(
        name: impl Into<String>,
        detail: impl Into<String>,
        group: TriageGroup,
        included: Option<bool>,
        group_index: usize,
    ) -> Self {
        Self {
            name: name.into(),
            detail: detail.into(),
            group,
            included,
            is_group_header: false,
            group_index,
            is_collapsed: false,
            group_count: 0,
        }
    }

    /// Create a group header row.
    pub fn header(group: TriageGroup, count: usize, collapsed: bool) -> Self {
        Self {
            name: String::new(),
            detail: String::new(),
            group,
            included: None,
            is_group_header: true,
            group_index: 0,
            is_collapsed: collapsed,
            group_count: count,
        }
    }
}

/// Main triage list widget rendering grouped items.
pub struct TriageListWidget<'a> {
    items: &'a [ListItem],
    /// Index into `items` of the cursor row.
    cursor: usize,
    /// Current section being displayed.
    section: SectionId,
    focused: bool,
    tier: ColorTier,
    /// Scroll offset (number of rows scrolled past the top).
    scroll_offset: usize,
}

impl<'a> TriageListWidget<'a> {
    pub fn new(
        items: &'a [ListItem],
        cursor: usize,
        section: SectionId,
        focused: bool,
        tier: ColorTier,
        scroll_offset: usize,
    ) -> Self {
        Self {
            items,
            cursor,
            section,
            focused,
            tier,
            scroll_offset,
        }
    }
}

impl Widget for TriageListWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let width = area.width as usize;

        // Row 0: section header.
        let header_style = if self.focused {
            Token::FocusBorder.style(self.tier)
        } else {
            Token::FocusUnfocused.style(self.tier)
        };
        let header_text = if self.focused {
            format!("-- {} --", self.section.label())
        } else {
            format!("  {}  ", self.section.label())
        };
        let padded = truncate(&header_text, width);
        let padded = format!("{:<width$}", padded, width = width);
        buf.set_string(area.x, area.y, &padded, header_style);

        // Remaining rows for items.
        let content_height = (area.height as usize).saturating_sub(1);
        if content_height == 0 {
            return;
        }

        let scroll = compute_scroll(self.scroll_offset, self.items.len(), content_height);

        let is_pure_reference = !self.section.is_decision();

        for (row_idx, item) in self
            .items
            .iter()
            .enumerate()
            .skip(scroll)
            .take(content_height)
        {
            let y = area.y + 1 + (row_idx - scroll) as u16;
            if y >= area.bottom() {
                break;
            }

            let is_cursor = row_idx == self.cursor && self.focused;
            let mut ctx = RowCtx {
                width,
                x: area.x,
                y,
                buf,
                tier: self.tier,
            };

            if item.is_group_header {
                render_group_header(item, is_cursor, &mut ctx);
            } else {
                render_item_row(item, is_cursor, is_pure_reference, &mut ctx);
            }
        }
    }
}

/// Shared rendering context for a single row.
struct RowCtx<'b> {
    width: usize,
    x: u16,
    y: u16,
    buf: &'b mut Buffer,
    tier: ColorTier,
}

/// Render a group header row (e.g., "▸ Investigate (12)").
fn render_group_header(item: &ListItem, is_cursor: bool, ctx: &mut RowCtx<'_>) {
    let arrow = if item.is_collapsed {
        "\u{25b8}"
    } else {
        "\u{25be}"
    };
    let text = format!("{} {} ({})", arrow, item.group.label(), item.group_count);
    let padded = format!("{:<width$}", truncate(&text, ctx.width), width = ctx.width);

    let group_style = match item.group {
        TriageGroup::Investigate => Token::TriageInvestigate.style(ctx.tier),
        TriageGroup::Site => Token::TriageSite.style(ctx.tier),
        TriageGroup::Baseline => Token::TriageBaseline.style(ctx.tier),
    };

    let style = if is_cursor {
        group_style.add_modifier(Modifier::REVERSED)
    } else {
        group_style
    };

    ctx.buf.set_string(ctx.x, ctx.y, &padded, style);
}

/// Render a regular item row with include/exclude indicator.
fn render_item_row(
    item: &ListItem,
    is_cursor: bool,
    is_pure_reference: bool,
    ctx: &mut RowCtx<'_>,
) {
    // Include/exclude indicator column (2 chars).
    let indicator = if is_pure_reference {
        "  "
    } else {
        match item.included {
            Some(true) => "\u{25cf} ",  // ● included
            Some(false) => "\u{25cb} ", // ○ excluded
            None => "  ",               // reference item in composite section
        }
    };

    let indicator_style = match item.included {
        Some(true) => Token::StatusIncluded.style(ctx.tier),
        Some(false) => Token::StatusExcluded.style(ctx.tier),
        _ => Token::TextMuted.style(ctx.tier),
    };

    // Name and detail share the remaining width.
    let remaining = ctx.width.saturating_sub(2);
    let (name_width, detail_width) = if remaining > 20 {
        let detail_w = remaining / 3;
        (remaining - detail_w, detail_w)
    } else {
        (remaining, 0)
    };

    let name_str = truncate(&item.name, name_width);
    let detail_str = if detail_width > 0 {
        truncate(&item.detail, detail_width)
    } else {
        String::new()
    };

    let name_style = if is_cursor {
        Token::FocusSelected.style(ctx.tier)
    } else {
        Token::TextPrimary.style(ctx.tier)
    };

    let detail_style = if is_cursor {
        Token::FocusSelected.style(ctx.tier)
    } else {
        Token::TextMuted.style(ctx.tier)
    };

    // Write indicator.
    ctx.buf.set_string(ctx.x, ctx.y, indicator, indicator_style);

    // Write name, padded to its column.
    let name_padded = format!("{:<width$}", name_str, width = name_width);
    ctx.buf
        .set_string(ctx.x + 2, ctx.y, &name_padded, name_style);

    // Write detail.
    if detail_width > 0 {
        let detail_padded = format!("{:<width$}", detail_str, width = detail_width);
        ctx.buf.set_string(
            ctx.x + 2 + name_width as u16,
            ctx.y,
            &detail_padded,
            detail_style,
        );
    }
}

/// Compute the scroll offset to keep the viewport sensible.
///
/// Returns the effective scroll, clamped so that:
/// - scroll never exceeds the maximum that would leave blank rows
/// - at least one row is always visible
pub fn compute_scroll(scroll_offset: usize, total_items: usize, viewport_height: usize) -> usize {
    if total_items <= viewport_height {
        0
    } else {
        scroll_offset.min(total_items - viewport_height)
    }
}

/// Truncate a string to fit within `max_width` columns.
/// Uses byte-level truncation (safe for ASCII; multi-byte chars may
/// produce a slightly shorter result but never overflow the width).
pub fn truncate(s: &str, max_width: usize) -> String {
    if s.len() <= max_width {
        s.to_string()
    } else if max_width <= 1 {
        s.chars().take(max_width).collect()
    } else {
        let mut result = String::with_capacity(max_width);
        for ch in s.chars() {
            if result.len() + ch.len_utf8() + 1 > max_width {
                break;
            }
            result.push(ch);
        }
        result.push('\u{2026}'); // ellipsis
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::buffer_to_string;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    fn test_items() -> Vec<ListItem> {
        vec![
            ListItem::header(TriageGroup::Investigate, 2, false),
            ListItem::item(
                "httpd",
                "2.4.57-2.el9",
                TriageGroup::Investigate,
                Some(true),
                0,
            ),
            ListItem::item(
                "nginx",
                "1.24.0-1.el9",
                TriageGroup::Investigate,
                Some(false),
                1,
            ),
            ListItem::header(TriageGroup::Baseline, 1, false),
            ListItem::item("bash", "5.2.15-5.el9", TriageGroup::Baseline, Some(true), 0),
        ]
    }

    #[test]
    fn renders_triage_list_with_groups() {
        let items = test_items();
        let widget = TriageListWidget::new(
            &items,
            1, // cursor on httpd
            SectionId::Packages,
            true,
            ColorTier::Mono,
            0,
        );
        let area = Rect::new(0, 0, 40, 7);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        insta::assert_snapshot!(buffer_to_string(&buf));
    }

    #[test]
    fn renders_reference_section_no_indicators() {
        let items = vec![
            ListItem::header(TriageGroup::Investigate, 1, false),
            ListItem::item(
                "kernel-5.14.0",
                "downgrade",
                TriageGroup::Investigate,
                None,
                0,
            ),
        ];
        let widget = TriageListWidget::new(
            &items,
            0, // cursor on header
            SectionId::VerChanges,
            true,
            ColorTier::Mono,
            0,
        );
        let area = Rect::new(0, 0, 30, 4);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        insta::assert_snapshot!(buffer_to_string(&buf));
    }

    #[test]
    fn compute_scroll_clamps_to_max() {
        assert_eq!(compute_scroll(0, 10, 5), 0);
        assert_eq!(compute_scroll(3, 10, 5), 3);
        assert_eq!(compute_scroll(10, 10, 5), 5);
        assert_eq!(compute_scroll(100, 10, 5), 5);
    }

    #[test]
    fn compute_scroll_no_scroll_when_fits() {
        assert_eq!(compute_scroll(5, 3, 5), 0);
        assert_eq!(compute_scroll(0, 5, 5), 0);
    }

    #[test]
    fn truncate_short_string_unchanged() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_long_string_adds_ellipsis() {
        let result = truncate("very-long-package-name", 10);
        assert!(result.len() <= 12); // utf8 ellipsis is 3 bytes
        assert!(result.ends_with('\u{2026}'));
    }

    #[test]
    fn empty_area_renders_nothing() {
        let items = test_items();
        let widget =
            TriageListWidget::new(&items, 0, SectionId::Packages, true, ColorTier::Mono, 0);
        let area = Rect::new(0, 0, 0, 0);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        assert_eq!(buffer_to_string(&buf), "");
    }
}
