//! Fullscreen detail view widget -- replaces the item list area.
//!
//! Shows scrollable content for the selected item: diff output for
//! configs (with +/- syntax highlighting), key-value summaries for
//! packages, plain text for other content types.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::widgets::Widget;

use crate::theme::{ColorTier, Token};
use crate::widget::triage_list::truncate;

/// The type of content being displayed, determines rendering behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailContentType {
    /// Unified diff output -- lines starting with +/- get colored.
    Diff,
    /// Systemd unit file content.
    UnitFile,
    /// YAML/structured content.
    YamlContent,
    /// Plain text (key-value summaries, etc.).
    PlainText,
}

/// Pre-built data for the fullscreen detail view.
#[derive(Debug, Clone)]
pub struct DetailData {
    /// Item title (config path, package name, etc.).
    pub title: String,
    /// The content body to display.
    pub content: String,
    /// How to render the content.
    pub content_type: DetailContentType,
    /// Include/exclude state of the item (`None` for reference-only).
    pub include: Option<bool>,
    /// Position string (e.g., "3 of 12").
    pub position: String,
}

/// Fullscreen detail view widget.
pub struct DetailViewWidget<'a> {
    data: &'a DetailData,
    scroll: u16,
    tier: ColorTier,
}

impl<'a> DetailViewWidget<'a> {
    pub fn new(data: &'a DetailData, scroll: u16, tier: ColorTier) -> Self {
        Self { data, scroll, tier }
    }
}

impl Widget for DetailViewWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 4 || area.width < 20 {
            return;
        }

        let width = area.width as usize;

        // Row 0: header -- title + include indicator + position.
        self.render_header(area, buf, width);

        // Row 1: separator.
        let sep: String = "\u{2500}".repeat(width);
        buf.set_string(area.x, area.y + 1, &sep, Token::TextMuted.style(self.tier));

        // Rows 2..n-1: scrollable content.
        let content_top = area.y + 2;
        let footer_row = area.bottom().saturating_sub(1);
        let content_height = footer_row.saturating_sub(content_top) as usize;

        if content_height > 0 {
            self.render_content(area.x, content_top, width, content_height, buf);
        }

        // Last row: footer with key hints.
        self.render_footer(area.x, footer_row, width, buf);
    }
}

impl DetailViewWidget<'_> {
    fn render_header(&self, area: Rect, buf: &mut Buffer, width: usize) {
        let include_indicator = match self.data.include {
            Some(true) => "\u{25cf} ",  // ● included
            Some(false) => "\u{25cb} ", // ○ excluded
            None => "",
        };

        let include_style = match self.data.include {
            Some(true) => Token::StatusIncluded.style(self.tier),
            Some(false) => Token::StatusExcluded.style(self.tier),
            None => Token::TextMuted.style(self.tier),
        };

        // Position string right-aligned.
        let pos_str = &self.data.position;
        let pos_width = pos_str.len();

        // Write include indicator.
        let mut x = area.x;
        if !include_indicator.is_empty() {
            buf.set_string(x, area.y, include_indicator, include_style);
            x += include_indicator.len() as u16;
        }

        // Write title (truncated to fit before position).
        let title_max = width
            .saturating_sub(include_indicator.len())
            .saturating_sub(pos_width + 1);
        let title = truncate(&self.data.title, title_max);
        buf.set_string(
            x,
            area.y,
            &title,
            Token::TextPrimary
                .style(self.tier)
                .add_modifier(Modifier::BOLD),
        );

        // Write position right-aligned.
        if pos_width < width {
            let pos_x = area.x + (width - pos_width) as u16;
            buf.set_string(pos_x, area.y, pos_str, Token::TextMuted.style(self.tier));
        }
    }

    fn render_content(&self, x: u16, top_y: u16, width: usize, height: usize, buf: &mut Buffer) {
        let lines: Vec<&str> = self.data.content.lines().collect();
        let scroll = self.scroll as usize;

        for (row, line) in lines.iter().skip(scroll).take(height).enumerate() {
            let y = top_y + row as u16;
            let display = truncate(line, width);

            let style = if self.data.content_type == DetailContentType::Diff {
                if line.starts_with('+') && !line.starts_with("+++") {
                    Token::DiffAdded.style(self.tier)
                } else if line.starts_with('-') && !line.starts_with("---") {
                    Token::DiffRemoved.style(self.tier)
                } else {
                    Token::TextPrimary.style(self.tier)
                }
            } else {
                Token::TextPrimary.style(self.tier)
            };

            buf.set_string(x, y, &display, style);
        }
    }

    fn render_footer(&self, x: u16, y: u16, width: usize, buf: &mut Buffer) {
        let hints = "n:next  p:prev  Esc:close  j/k:scroll";
        let display = truncate(hints, width);
        buf.set_string(x, y, &display, Token::TextMuted.style(self.tier));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::buffer_to_string;

    fn diff_content() -> String {
        [
            "--- a/etc/httpd.conf",
            "+++ b/etc/httpd.conf",
            "@@ -10,3 +10,3 @@",
            " ServerRoot \"/etc/httpd\"",
            "-Listen 80",
            "+Listen 8080",
            " ServerAdmin root@localhost",
        ]
        .join("\n")
    }

    #[test]
    fn renders_diff_detail() {
        let data = DetailData {
            title: "/etc/httpd/conf/httpd.conf".into(),
            content: diff_content(),
            content_type: DetailContentType::Diff,
            include: Some(true),
            position: "3 of 12".into(),
        };
        let widget = DetailViewWidget::new(&data, 0, ColorTier::Mono);
        let area = Rect::new(0, 0, 50, 12);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        insta::assert_snapshot!(buffer_to_string(&buf));
    }

    #[test]
    fn renders_plain_text_detail() {
        let data = DetailData {
            title: "httpd.x86_64".into(),
            content: "Version: 2.4.62\nRepo: rhel-9-appstream\nArch: x86_64".into(),
            content_type: DetailContentType::PlainText,
            include: Some(false),
            position: "1 of 5".into(),
        };
        let widget = DetailViewWidget::new(&data, 0, ColorTier::Mono);
        let area = Rect::new(0, 0, 50, 8);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        insta::assert_snapshot!(buffer_to_string(&buf));
    }

    #[test]
    fn scrolls_content() {
        let content = (0..20)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let data = DetailData {
            title: "scrolltest".into(),
            content,
            content_type: DetailContentType::PlainText,
            include: None,
            position: "1 of 1".into(),
        };
        // Scroll past first 5 lines.
        let widget = DetailViewWidget::new(&data, 5, ColorTier::Mono);
        let area = Rect::new(0, 0, 30, 8);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        let text = buffer_to_string(&buf);
        // First visible content line should be "line 5".
        assert!(text.contains("line 5"));
        assert!(!text.contains("line 0"));
    }

    #[test]
    fn skips_render_when_too_small() {
        let data = DetailData {
            title: "tiny".into(),
            content: "content".into(),
            content_type: DetailContentType::PlainText,
            include: None,
            position: "1 of 1".into(),
        };
        let widget = DetailViewWidget::new(&data, 0, ColorTier::Mono);
        let area = Rect::new(0, 0, 10, 2); // too small
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        let text = buffer_to_string(&buf);
        assert!(text.trim().is_empty());
    }
}
