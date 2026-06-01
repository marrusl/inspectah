//! Containerfile preview widget -- side panel showing the generated Containerfile.
//!
//! Renders line-numbered Containerfile content with Dockerfile keyword
//! highlighting: directive keywords (FROM, RUN, COPY, ADD, ENV) use
//! DiffAdded style; comments use TextMuted.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::widgets::Widget;

use crate::theme::{ColorTier, Token};
use crate::widget::triage_list::truncate;

/// Containerfile preview panel widget.
pub struct ContainerfileWidget<'a> {
    content: &'a str,
    tier: ColorTier,
}

impl<'a> ContainerfileWidget<'a> {
    pub fn new(content: &'a str, tier: ColorTier) -> Self {
        Self { content, tier }
    }
}

/// Dockerfile directive keywords that get DiffAdded highlighting.
const DOCKERFILE_KEYWORDS: &[&str] = &[
    "FROM",
    "RUN",
    "COPY",
    "ADD",
    "ENV",
    "ARG",
    "LABEL",
    "EXPOSE",
    "WORKDIR",
    "USER",
    "ENTRYPOINT",
    "CMD",
    "VOLUME",
    "STOPSIGNAL",
    "HEALTHCHECK",
    "SHELL",
    "ONBUILD",
    "MAINTAINER",
];

/// Check if a line starts with a Dockerfile keyword.
fn is_keyword_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    DOCKERFILE_KEYWORDS.iter().any(|kw| trimmed.starts_with(kw))
}

impl Widget for ContainerfileWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 3 || area.width < 15 {
            return;
        }

        let width = area.width as usize;

        // Row 0: header.
        let header = "Containerfile Preview";
        let display = truncate(header, width);
        buf.set_string(
            area.x,
            area.y,
            &display,
            Token::TextPrimary
                .style(self.tier)
                .add_modifier(Modifier::BOLD),
        );

        // Row 1: separator.
        let sep: String = "\u{2500}".repeat(width);
        buf.set_string(area.x, area.y + 1, &sep, Token::TextMuted.style(self.tier));

        // Rows 2..end: line-numbered content.
        let content_top = area.y + 2;
        let content_height = area.height.saturating_sub(2) as usize;
        let lines: Vec<&str> = self.content.lines().collect();

        // Line number gutter: 3 chars right-aligned + space separator.
        let gutter_width: usize = 4; // "NNN " format
        let text_width = width.saturating_sub(gutter_width);

        for (row, line) in lines.iter().take(content_height).enumerate() {
            let y = content_top + row as u16;
            let line_num = row + 1;

            // Render line number (3-char right-aligned).
            let num_str = format!("{line_num:>3} ");
            buf.set_string(area.x, y, &num_str, Token::TextMuted.style(self.tier));

            // Render content with syntax highlighting.
            let display = truncate(line, text_width);
            let style = if line.trim_start().starts_with('#') {
                Token::TextMuted.style(self.tier)
            } else if is_keyword_line(line) {
                Token::DiffAdded.style(self.tier)
            } else {
                Token::TextPrimary.style(self.tier)
            };

            buf.set_string(area.x + gutter_width as u16, y, &display, style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::buffer_to_string;

    #[test]
    fn renders_containerfile_with_line_numbers() {
        let content = "FROM registry.redhat.io/rhel9:latest\nRUN dnf install -y httpd\nCOPY config /etc/httpd/\n# Start the server\nENV PORT=8080";
        let widget = ContainerfileWidget::new(content, ColorTier::Mono);
        let area = Rect::new(0, 0, 50, 10);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        let text = buffer_to_string(&buf);
        assert!(text.contains("Containerfile Preview"));
        assert!(text.contains("FROM"));
        assert!(text.contains("RUN"));
    }

    #[test]
    fn skips_render_when_too_small() {
        let widget = ContainerfileWidget::new("FROM scratch", ColorTier::Mono);
        let area = Rect::new(0, 0, 10, 2);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        let text = buffer_to_string(&buf);
        // Header should not appear -- area too small.
        assert!(!text.contains("Containerfile"));
    }

    #[test]
    fn keyword_detection() {
        assert!(is_keyword_line("FROM scratch"));
        assert!(is_keyword_line("RUN dnf install -y httpd"));
        assert!(is_keyword_line("COPY . /app"));
        assert!(is_keyword_line("ADD archive.tar.gz /opt"));
        assert!(is_keyword_line("ENV PORT=8080"));
        assert!(is_keyword_line("  WORKDIR /app")); // leading whitespace
        assert!(!is_keyword_line("echo hello"));
        assert!(!is_keyword_line("# comment"));
    }

    #[test]
    fn truncates_long_lines() {
        let long_line = format!("RUN {}", "x".repeat(200));
        let widget = ContainerfileWidget::new(&long_line, ColorTier::Mono);
        let area = Rect::new(0, 0, 30, 5);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        // Should not panic; content is truncated to area width.
        let text = buffer_to_string(&buf);
        assert!(text.contains("RUN"));
    }
}
