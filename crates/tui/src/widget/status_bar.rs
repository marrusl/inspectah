use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::theme::{ColorTier, Token};
use crate::types::FlashMessage;

pub struct StatusBarWidget<'a> {
    included: usize,
    excluded: usize,
    containerfile_delta: usize,
    reviewed: usize,
    total_reviewable: usize,
    flash: Option<&'a FlashMessage>,
    tier: ColorTier,
    is_decision_section: bool,
}

impl<'a> StatusBarWidget<'a> {
    pub fn new(tier: ColorTier) -> Self {
        Self {
            included: 0,
            excluded: 0,
            containerfile_delta: 0,
            reviewed: 0,
            total_reviewable: 0,
            flash: None,
            tier,
            is_decision_section: true,
        }
    }

    pub fn stats(mut self, included: usize, excluded: usize) -> Self {
        self.included = included;
        self.excluded = excluded;
        self
    }

    pub fn containerfile_delta(mut self, delta: usize) -> Self {
        self.containerfile_delta = delta;
        self
    }

    pub fn reviewed_progress(mut self, reviewed: usize, total: usize) -> Self {
        self.reviewed = reviewed;
        self.total_reviewable = total;
        self
    }

    pub fn flash(mut self, flash: Option<&'a FlashMessage>) -> Self {
        self.flash = flash;
        self
    }

    pub fn decision_section(mut self, is_decision: bool) -> Self {
        self.is_decision_section = is_decision;
        self
    }
}

impl Widget for StatusBarWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }

        // Flash message takes priority
        if let Some(flash) = self.flash
            && !flash.is_expired()
        {
            buf.set_string(
                area.x + 1,
                area.y,
                &flash.text,
                Token::Warning.style(self.tier),
            );
            return;
        }

        let mut parts: Vec<String> = Vec::new();

        if self.is_decision_section {
            parts.push(format!("{} incl", self.included));
            parts.push(format!("{} excl", self.excluded));
        }

        if self.containerfile_delta > 0 {
            parts.push(format!("Containerfile: {}Δ", self.containerfile_delta));
        }

        if self.total_reviewable > 0 {
            parts.push(format!(
                "{}/{} reviewed",
                self.reviewed, self.total_reviewable
            ));
        }

        let status = format!(" {}", parts.join(" · "));

        // Key hints on the right
        let hints = "q:quit  ?:help  /:search  ::cmd";
        let hints_x = area.right().saturating_sub(hints.len() as u16 + 1);

        buf.set_string(area.x, area.y, &status, Token::TextMuted.style(self.tier));

        if hints_x > area.x + status.len() as u16 + 2 {
            buf.set_string(hints_x, area.y, hints, Token::TextMuted.style(self.tier));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::buffer_to_string;

    #[test]
    fn renders_stats_line() {
        let widget = StatusBarWidget::new(ColorTier::Mono)
            .stats(142, 176)
            .containerfile_delta(3);
        let area = Rect::new(0, 0, 80, 1);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        let output = buffer_to_string(&buf);
        assert!(output.contains("142 incl"));
        assert!(output.contains("176 excl"));
        assert!(output.contains("Containerfile: 3Δ"));
    }

    #[test]
    fn flash_overrides_stats() {
        let flash = FlashMessage::new("Resumed session (5 ops)", 3);
        let widget = StatusBarWidget::new(ColorTier::Mono)
            .stats(100, 50)
            .flash(Some(&flash));
        let area = Rect::new(0, 0, 60, 1);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        let output = buffer_to_string(&buf);
        assert!(output.contains("Resumed session (5 ops)"));
        assert!(!output.contains("100 incl"));
    }
}
