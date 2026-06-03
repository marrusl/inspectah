use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::theme::{ColorTier, Token};

/// (key, description) pairs for the help screen.
/// Empty key = blank line separator. Empty description = section header.
const HELP_ENTRIES: &[(&str, &str)] = &[
    ("j/k \u{2191}/\u{2193}", "Move cursor"),
    ("h/l \u{2190}/\u{2192}", "Sidebar \u{2194} items"),
    ("Tab", "Cycle focus"),
    ("1-9", "Jump to section"),
    ("{/}", "Prev/next group"),
    ("g/G", "Top/bottom"),
    ("Space", "Toggle include/exclude"),
    ("Enter", "Open detail"),
    ("Esc", "Close / back"),
    ("f", "Fullscreen detail"),
    ("n/p", "Next/prev in detail"),
    ("u", "Undo"),
    ("Ctrl+r", "Redo"),
    ("c", "Containerfile toggle"),
    ("/", "Search"),
    (":", "Command mode"),
    ("r", "Refresh"),
    ("q", "Quit"),
    ("", ""),
    ("Commands", ""),
    (":export [path]", "Export tarball"),
    (":section <name>", "Jump to section"),
    (":stats", "Session statistics"),
    (":undo / :redo", "Undo / redo"),
    (":fresh", "Discard and restart"),
    ("", ""),
    ("Indicators", ""),
    ("[+]", "Included"),
    ("[-]", "Excluded"),
    ("[-L]", "Locked (cannot toggle)"),
];

/// Width of the key column in characters.
const KEY_COL_WIDTH: u16 = 18;

/// Fullscreen overlay showing keybinding reference.
pub struct HelpScreenWidget {
    tier: ColorTier,
}

impl HelpScreenWidget {
    pub fn new(tier: ColorTier) -> Self {
        Self { tier }
    }
}

impl Widget for HelpScreenWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Clear the entire area.
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)].reset();
            }
        }

        let key_style = Token::Warning.style(self.tier);
        let desc_style = Token::TextPrimary.style(self.tier);
        let header_style = Token::Warning.style(self.tier);

        // Title
        let title = " Keybindings ";
        let title_x = area.x + 2;
        let mut y = area.y + 1;

        buf.set_string(title_x, y, title, header_style);
        y += 1;

        // Separator line
        let sep_width = (area.width.saturating_sub(4)) as usize;
        let separator: String = "\u{2500}".repeat(sep_width);
        buf.set_string(title_x, y, &separator, Token::TextMuted.style(self.tier));
        y += 1;

        // Entries
        for &(key, desc) in HELP_ENTRIES {
            if y >= area.bottom().saturating_sub(1) {
                break;
            }

            if key.is_empty() && desc.is_empty() {
                // Blank line separator.
                y += 1;
                continue;
            }

            if !key.is_empty() && desc.is_empty() {
                // Section header.
                buf.set_string(title_x, y, key, header_style);
                y += 1;
                continue;
            }

            // Two-column entry: key (highlighted) + description (plain).
            let key_x = title_x;
            let desc_x = title_x + KEY_COL_WIDTH;

            buf.set_string(key_x, y, key, key_style);
            if desc_x < area.right() {
                buf.set_string(desc_x, y, desc, desc_style);
            }
            y += 1;
        }

        // Footer hint
        let footer = "Press ? or Esc to close";
        let footer_y = area.bottom().saturating_sub(1);
        if footer_y > y {
            buf.set_string(title_x, footer_y, footer, Token::TextMuted.style(self.tier));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::buffer_to_string;

    #[test]
    fn renders_title_and_entries() {
        let widget = HelpScreenWidget::new(ColorTier::Mono);
        let area = Rect::new(0, 0, 60, 30);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        let output = buffer_to_string(&buf);
        assert!(output.contains("Keybindings"));
        assert!(output.contains("Move cursor"));
        assert!(output.contains("Quit"));
        assert!(output.contains("Commands"));
        assert!(output.contains("Export tarball"));
    }

    #[test]
    fn renders_footer_hint() {
        let widget = HelpScreenWidget::new(ColorTier::Mono);
        let area = Rect::new(0, 0, 60, 35);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        let output = buffer_to_string(&buf);
        assert!(output.contains("Press ? or Esc to close"));
    }

    #[test]
    fn clips_at_area_boundary() {
        // Very short area should not panic.
        let widget = HelpScreenWidget::new(ColorTier::Ansi16);
        let area = Rect::new(0, 0, 40, 5);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        // No panic = success.
    }
}
