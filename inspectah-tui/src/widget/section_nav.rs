use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::theme::{ColorTier, Token};
use crate::types::SectionEntry;

pub struct SectionNavWidget<'a> {
    sections: &'a [SectionEntry],
    active: usize,
    focused: bool,
    tier: ColorTier,
    scroll_offset: usize,
}

impl<'a> SectionNavWidget<'a> {
    pub fn new(
        sections: &'a [SectionEntry],
        active: usize,
        focused: bool,
        tier: ColorTier,
        scroll_offset: usize,
    ) -> Self {
        Self {
            sections,
            active,
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
        let total = self.sections.len();
        let scroll = self.scroll_offset.min(total.saturating_sub(visible_height));

        let border_style = if self.focused {
            Token::FocusBorder.style(self.tier)
        } else {
            Token::FocusUnfocused.style(self.tier)
        };

        // Render header
        let header = if self.focused {
            "─ Sections ─"
        } else {
            " Sections "
        };
        let header_line = format!("{:width$}", header, width = area.width as usize);
        buf.set_string(area.x, area.y, &header_line, border_style);

        // Render section rows
        for (i, entry) in self.sections.iter().enumerate().skip(scroll) {
            let row_y = area.y + 1 + (i - scroll) as u16;
            if row_y >= area.bottom() {
                break;
            }

            let is_active = i == self.active;
            let style = if is_active {
                Token::FocusSelected.style(self.tier)
            } else if entry.id.is_decision() {
                Token::TextPrimary.style(self.tier)
            } else {
                Token::TextMuted.style(self.tier)
            };

            // Number prefix for first 9 sections
            let num = if i < 9 {
                format!("{} ", i + 1)
            } else {
                "  ".to_string()
            };

            // Label + count
            let count_str = format!("{}", entry.count);
            let label_width = (area.width as usize)
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
                "{}{:<width$}{}",
                num,
                truncated,
                count_str,
                width = label_width,
            );
            buf.set_string(area.x, row_y, &line, style);
        }

        // Scroll indicators
        if scroll > 0 {
            buf.set_string(area.right() - 1, area.y + 1, "▲", border_style);
        }
        if scroll + visible_height < total + 1 {
            buf.set_string(area.right() - 1, area.bottom() - 1, "▼", border_style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::buffer_to_string;
    use crate::types::SectionId;

    fn test_sections() -> Vec<SectionEntry> {
        vec![
            SectionEntry {
                id: SectionId::Packages,
                count: 142,
                included: 130,
                excluded: 12,
            },
            SectionEntry {
                id: SectionId::Configs,
                count: 47,
                included: 40,
                excluded: 7,
            },
            SectionEntry {
                id: SectionId::Services,
                count: 23,
                included: 20,
                excluded: 3,
            },
        ]
    }

    #[test]
    fn renders_sections_with_counts() {
        let sections = test_sections();
        let widget = SectionNavWidget::new(&sections, 0, true, ColorTier::Mono, 0);
        let area = Rect::new(0, 0, 18, 5);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        insta::assert_snapshot!(buffer_to_string(&buf));
    }

    #[test]
    fn highlights_active_section() {
        let sections = test_sections();
        let widget = SectionNavWidget::new(&sections, 1, true, ColorTier::Mono, 0);
        let area = Rect::new(0, 0, 18, 5);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        // Active section (Configs) should have reversed style
        let configs_y = area.y + 2; // header + Packages + Configs
        let cell = &buf[(area.x, configs_y)];
        assert!(
            cell.style()
                .add_modifier
                .contains(ratatui::style::Modifier::REVERSED)
                || cell
                    .style()
                    .add_modifier
                    .contains(ratatui::style::Modifier::BOLD),
            "Active section should be highlighted"
        );
    }

    #[test]
    fn empty_area_renders_nothing() {
        let sections = test_sections();
        let widget = SectionNavWidget::new(&sections, 0, true, ColorTier::Mono, 0);
        let area = Rect::new(0, 0, 0, 0);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        assert_eq!(buffer_to_string(&buf), "");
    }

    #[test]
    fn unfocused_uses_different_header() {
        let sections = test_sections();
        let widget = SectionNavWidget::new(&sections, 0, false, ColorTier::Mono, 0);
        let area = Rect::new(0, 0, 18, 5);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        insta::assert_snapshot!(buffer_to_string(&buf));
    }
}
