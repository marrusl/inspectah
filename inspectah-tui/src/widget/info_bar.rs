//! Compact info bar widget -- 2-3 rows at the bottom of the item list.
//!
//! Shows key-value metadata for the currently selected item without
//! replacing the list view. Activated by Enter on a non-header item,
//! dismissed by Esc.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::theme::{ColorTier, Token};

/// Data for the compact info bar (2-3 rows at bottom of item list).
pub struct InfoBarData {
    pub name: String,
    pub fields: Vec<(String, String)>,
}

pub struct InfoBarWidget<'a> {
    data: &'a InfoBarData,
    tier: ColorTier,
}

impl<'a> InfoBarWidget<'a> {
    pub fn new(data: &'a InfoBarData, tier: ColorTier) -> Self {
        Self { data, tier }
    }
}

impl Widget for InfoBarWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 2 || area.width < 20 {
            return;
        }

        // Separator line.
        let max_chars = area.width as usize;
        let sep = "─".repeat(max_chars);
        buf.set_string(area.x, area.y, &sep, Token::TextMuted.style(self.tier));

        // Item name on first data row.
        let name = if self.data.name.len() > max_chars.saturating_sub(1) {
            &self.data.name[..max_chars.saturating_sub(1)]
        } else {
            &self.data.name
        };
        buf.set_string(
            area.x + 1,
            area.y + 1,
            name,
            Token::TextPrimary.style(self.tier),
        );

        // Key-value fields on subsequent rows.
        for (i, (key, value)) in self.data.fields.iter().enumerate() {
            let row_y = area.y + 2 + i as u16;
            if row_y >= area.bottom() {
                break;
            }
            let label = format!("  {}: ", key);
            buf.set_string(area.x, row_y, &label, Token::TextMuted.style(self.tier));
            buf.set_string(
                area.x + label.len() as u16,
                row_y,
                value,
                Token::TextPrimary.style(self.tier),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::buffer_to_string;

    #[test]
    fn renders_package_info() {
        let data = InfoBarData {
            name: "httpd.x86_64".into(),
            fields: vec![
                ("Version".into(), "2.4.62".into()),
                ("Repo".into(), "rhel-9-appstream".into()),
                ("Reason".into(), "User-added package".into()),
            ],
        };
        let widget = InfoBarWidget::new(&data, ColorTier::Mono);
        let area = Rect::new(0, 0, 50, 5);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        insta::assert_snapshot!(buffer_to_string(&buf));
    }

    #[test]
    fn renders_config_info() {
        let data = InfoBarData {
            name: "/etc/httpd/conf/httpd.conf".into(),
            fields: vec![
                ("Kind".into(), "Modified".into()),
                ("Category".into(), "Application".into()),
            ],
        };
        let widget = InfoBarWidget::new(&data, ColorTier::Mono);
        let area = Rect::new(0, 0, 50, 4);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        insta::assert_snapshot!(buffer_to_string(&buf));
    }

    #[test]
    fn skips_render_when_too_small() {
        let data = InfoBarData {
            name: "tiny".into(),
            fields: vec![],
        };
        let widget = InfoBarWidget::new(&data, ColorTier::Mono);
        let area = Rect::new(0, 0, 10, 1); // too short
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        // Buffer should be empty (all spaces).
        let text = buffer_to_string(&buf);
        assert!(text.trim().is_empty());
    }

    #[test]
    fn truncates_fields_to_available_rows() {
        let data = InfoBarData {
            name: "test-item".into(),
            fields: vec![
                ("A".into(), "1".into()),
                ("B".into(), "2".into()),
                ("C".into(), "3".into()),
                ("D".into(), "4".into()),
            ],
        };
        let widget = InfoBarWidget::new(&data, ColorTier::Mono);
        // Only 4 rows: separator + name + 2 fields. "C" and "D" won't fit.
        let area = Rect::new(0, 0, 40, 4);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        let text = buffer_to_string(&buf);
        assert!(text.contains("A:"));
        assert!(text.contains("B:"));
        assert!(!text.contains("C:"));
    }
}
