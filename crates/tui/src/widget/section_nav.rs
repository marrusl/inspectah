use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::widgets::Widget;

use crate::sections::NavRow;
use crate::theme::{ColorTier, Token};
use crate::types::SectionId;

pub struct SectionNavWidget<'a> {
    rows: &'a [NavRow],
    /// The currently active `SectionId` (determines which row is highlighted).
    active_section: SectionId,
    focused: bool,
    tier: ColorTier,
    scroll_offset: usize,
}

impl<'a> SectionNavWidget<'a> {
    pub fn new(
        rows: &'a [NavRow],
        active_section: SectionId,
        focused: bool,
        tier: ColorTier,
        scroll_offset: usize,
    ) -> Self {
        Self {
            rows,
            active_section,
            focused,
            tier,
            scroll_offset,
        }
    }
}

impl Widget for SectionNavWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let visible_height = area.height as usize;
        let total = self.rows.len();
        let scroll = self.scroll_offset.min(total.saturating_sub(visible_height));

        let border_style = if self.focused {
            Token::FocusBorder.style(self.tier)
        } else {
            Token::FocusUnfocused.style(self.tier)
        };

        // Render header
        let header = if self.focused {
            "\u{2500} Sections \u{2500}"
        } else {
            " Sections "
        };
        let header_line = format!("{:width$}", header, width = area.width as usize);
        buf.set_string(area.x, area.y, &header_line, border_style);

        // Track section number for 1-9 jumps (only sections, not group headers).
        let mut section_num: usize = 0;

        // Render nav rows
        for (i, row) in self.rows.iter().enumerate().skip(scroll) {
            let row_y = area.y + 1 + (i - scroll) as u16;
            if row_y >= area.bottom() {
                break;
            }

            match row {
                NavRow::Group {
                    group,
                    collapsed,
                    actionable,
                    advisories,
                } => {
                    let arrow = if *collapsed { "\u{25b8}" } else { "\u{25be}" };
                    let label = group.short_label();
                    let text = if *collapsed {
                        format!("{} {} [{}, {} adv]", arrow, label, actionable, advisories)
                    } else {
                        format!("{} {}", arrow, label)
                    };
                    let padded = format!(
                        "{:<width$}",
                        &text[..text.len().min(area.width as usize)],
                        width = area.width as usize
                    );
                    let style = Token::FocusBorder
                        .style(self.tier)
                        .add_modifier(Modifier::DIM);
                    buf.set_string(area.x, row_y, &padded, style);
                }
                NavRow::Section(entry) => {
                    section_num += 1;
                    let is_active = entry.id == self.active_section;
                    let style = if is_active {
                        Token::FocusSelected.style(self.tier)
                    } else if entry.id.is_decision() {
                        Token::TextPrimary.style(self.tier)
                    } else {
                        Token::TextMuted.style(self.tier)
                    };

                    // Number prefix for first 9 sections
                    let num = if section_num <= 9 {
                        format!("{} ", section_num)
                    } else {
                        "  ".to_string()
                    };

                    // Indent to show hierarchy under group header.
                    let indent = " ";

                    // Label + count
                    let count_str = format!("{}", entry.count);
                    let label_width = (area.width as usize)
                        .saturating_sub(indent.len())
                        .saturating_sub(num.len())
                        .saturating_sub(count_str.len())
                        .saturating_sub(1);
                    let label = entry.id.label();
                    let truncated = if label.len() > label_width {
                        &label[..label_width]
                    } else {
                        label
                    };

                    let line = format!(
                        "{}{}{:<width$}{}",
                        indent,
                        num,
                        truncated,
                        count_str,
                        width = label_width,
                    );
                    buf.set_string(area.x, row_y, &line, style);
                }
            }
        }

        // Scroll indicators
        if scroll > 0 {
            buf.set_string(area.right() - 1, area.y + 1, "\u{25b2}", border_style);
        }
        if scroll + visible_height < total + 1 {
            buf.set_string(
                area.right() - 1,
                area.bottom() - 1,
                "\u{25bc}",
                border_style,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sections::build_nav_rows;
    use crate::test_helpers::buffer_to_string;
    use crate::types::{NavGroup, SectionEntry};
    use std::collections::HashSet;

    fn test_entries() -> Vec<SectionEntry> {
        vec![
            SectionEntry {
                id: SectionId::Packages,
                count: 142,
                included: 130,
                excluded: 12,
                advisory: 0,
            },
            SectionEntry {
                id: SectionId::Configs,
                count: 47,
                included: 40,
                excluded: 7,
                advisory: 0,
            },
            SectionEntry {
                id: SectionId::Services,
                count: 23,
                included: 20,
                excluded: 3,
                advisory: 0,
            },
        ]
    }

    #[test]
    fn renders_nav_tree_with_groups() {
        let entries = test_entries();
        let rows = build_nav_rows(&entries, &HashSet::new());
        let widget = SectionNavWidget::new(&rows, SectionId::Packages, true, ColorTier::Mono, 0);
        let area = Rect::new(0, 0, 22, 10);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        let text = buffer_to_string(&buf);
        // Group headers should be present.
        assert!(text.contains("Packages"), "should contain Packages group");
    }

    #[test]
    fn collapsed_group_shows_summary() {
        let entries = vec![
            SectionEntry {
                id: SectionId::Configs,
                count: 47,
                included: 40,
                excluded: 7,
                advisory: 0,
            },
            SectionEntry {
                id: SectionId::Sysctls,
                count: 8,
                included: 5,
                excluded: 3,
                advisory: 0,
            },
            SectionEntry {
                id: SectionId::Tuned,
                count: 3,
                included: 2,
                excluded: 1,
                advisory: 0,
            },
            SectionEntry {
                id: SectionId::KernelBoot,
                count: 10,
                included: 0,
                excluded: 0,
                advisory: 0,
            },
            SectionEntry {
                id: SectionId::SELinux,
                count: 2,
                included: 0,
                excluded: 0,
                advisory: 0,
            },
        ];
        let mut collapsed = HashSet::new();
        collapsed.insert(NavGroup::SystemConfig);
        let rows = build_nav_rows(&entries, &collapsed);
        let widget = SectionNavWidget::new(&rows, SectionId::Packages, true, ColorTier::Mono, 0);
        let area = Rect::new(0, 0, 30, 5);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        let text = buffer_to_string(&buf);
        // Collapsed summary should show counts.
        assert!(
            text.contains("adv"),
            "collapsed group should show advisory count"
        );
    }

    #[test]
    fn empty_area_renders_nothing() {
        let rows = build_nav_rows(&test_entries(), &HashSet::new());
        let widget = SectionNavWidget::new(&rows, SectionId::Packages, true, ColorTier::Mono, 0);
        let area = Rect::new(0, 0, 0, 0);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        assert_eq!(buffer_to_string(&buf), "");
    }
}
