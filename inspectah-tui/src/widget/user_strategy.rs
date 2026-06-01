//! User strategy widget -- specialized view for the Users section.
//!
//! Replaces the generic triage list when the Users section is active.
//! Displays a per-user table with username, UID, strategy (skip/useradd),
//! and password choice columns. Space cycles strategy, Enter cycles
//! password choice.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::widgets::Widget;

use inspectah_core::types::users::{UserContainerfileStrategy, UserPasswordChoice};

use crate::theme::{ColorTier, Token};

/// A single user entry for the strategy view.
#[derive(Debug, Clone)]
pub struct UserEntry {
    pub username: String,
    pub uid: u64,
    pub strategy: UserContainerfileStrategy,
    pub has_password: bool,
    pub password_choice: UserPasswordChoice,
}

impl UserEntry {
    /// Display label for the containerfile strategy.
    pub fn strategy_label(&self) -> &'static str {
        match self.strategy {
            UserContainerfileStrategy::Skip => "skip",
            UserContainerfileStrategy::Useradd => "useradd",
        }
    }

    /// Display label for the password choice.
    pub fn password_label(&self) -> &'static str {
        match self.password_choice {
            UserPasswordChoice::None => "none",
            UserPasswordChoice::Preserve => "preserve",
            UserPasswordChoice::New => "new",
        }
    }

    /// Cycle to the next strategy variant.
    pub fn next_strategy(&self) -> UserContainerfileStrategy {
        match self.strategy {
            UserContainerfileStrategy::Skip => UserContainerfileStrategy::Useradd,
            UserContainerfileStrategy::Useradd => UserContainerfileStrategy::Skip,
        }
    }

    /// Cycle to the next password choice variant.
    ///
    /// Only cycles between `None` and `Preserve`. `New` requires a
    /// password hash that the TUI cannot prompt for, so it is excluded
    /// from the cycle. If the current choice is `New` (set externally),
    /// cycle back to `None`.
    pub fn next_password_choice(&self) -> UserPasswordChoice {
        match self.password_choice {
            UserPasswordChoice::None => UserPasswordChoice::Preserve,
            UserPasswordChoice::Preserve | UserPasswordChoice::New => UserPasswordChoice::None,
        }
    }
}

/// Widget that renders the user strategy table.
pub struct UserStrategyWidget<'a> {
    entries: &'a [UserEntry],
    cursor: usize,
    focused: bool,
    tier: ColorTier,
}

impl<'a> UserStrategyWidget<'a> {
    pub fn new(entries: &'a [UserEntry], cursor: usize, focused: bool, tier: ColorTier) -> Self {
        Self {
            entries,
            cursor,
            focused,
            tier,
        }
    }
}

/// Truncate a string to at most `max_width` characters, appending an
/// ellipsis if truncated.
fn truncate(s: &str, max_width: usize) -> String {
    if s.len() <= max_width {
        s.to_string()
    } else if max_width <= 1 {
        "\u{2026}".to_string()
    } else {
        let mut result: String = s.chars().take(max_width - 1).collect();
        result.push('\u{2026}');
        result
    }
}

impl Widget for UserStrategyWidget<'_> {
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
        let title = format!("{:<width$}", " Users", width = width);
        buf.set_string(area.x, area.y, &title, header_style);

        if area.height < 3 {
            return;
        }

        // Row 1: column headers.
        let col_header = format_columns("Username", "UID", "Strategy", "Password", width);
        let col_style = Token::TextMuted.style(self.tier);
        buf.set_string(area.x, area.y + 1, &col_header, col_style);

        // Rows 2+: user entries.
        let content_height = (area.height as usize).saturating_sub(2);
        let scroll = compute_scroll(self.cursor, self.entries.len(), content_height);

        for (row_idx, entry) in self
            .entries
            .iter()
            .enumerate()
            .skip(scroll)
            .take(content_height)
        {
            let y = area.y + 2 + (row_idx - scroll) as u16;
            if y >= area.bottom() {
                break;
            }

            let is_cursor = row_idx == self.cursor && self.focused;
            let uid_str = entry.uid.to_string();
            let line = format_columns(
                &entry.username,
                &uid_str,
                entry.strategy_label(),
                entry.password_label(),
                width,
            );

            let style = if is_cursor {
                Token::FocusSelected.style(self.tier)
            } else {
                Token::TextPrimary.style(self.tier)
            };

            // Highlight strategy column with color when not on cursor row.
            if is_cursor {
                buf.set_string(area.x, y, &line, style);
            } else {
                // Render each column segment with appropriate styling.
                let (name_w, uid_w, strat_w, _pw_w) = column_widths(width);
                let mut x = area.x;

                // Username column.
                let name_str = truncate(&entry.username, name_w);
                let name_padded = format!("{:<width$}", name_str, width = name_w);
                buf.set_string(x, y, &name_padded, style);
                x += name_w as u16 + 1; // +1 for separator space

                // UID column.
                let uid_display = truncate(&uid_str, uid_w);
                let uid_padded = format!("{:<width$}", uid_display, width = uid_w);
                buf.set_string(x, y, &uid_padded, Token::TextMuted.style(self.tier));
                x += uid_w as u16 + 1;

                // Strategy column -- colored by value.
                let strat_str = truncate(entry.strategy_label(), strat_w);
                let strat_padded = format!("{:<width$}", strat_str, width = strat_w);
                let strat_style = match entry.strategy {
                    UserContainerfileStrategy::Skip => Token::TextMuted.style(self.tier),
                    UserContainerfileStrategy::Useradd => Token::StatusIncluded
                        .style(self.tier)
                        .add_modifier(Modifier::BOLD),
                };
                buf.set_string(x, y, &strat_padded, strat_style);
                x += strat_w as u16 + 1;

                // Password column.
                let pw_str = truncate(
                    entry.password_label(),
                    width.saturating_sub(x as usize - area.x as usize),
                );
                buf.set_string(x, y, &pw_str, Token::TextMuted.style(self.tier));
            }
        }
    }
}

/// Compute column widths for a given total width.
///
/// Layout: Username (40%) | UID (10%) | Strategy (20%) | Password (remaining).
fn column_widths(total: usize) -> (usize, usize, usize, usize) {
    let name_w = (total * 40 / 100).max(8);
    let uid_w = (total * 10 / 100).max(5);
    let strat_w = (total * 20 / 100).max(8);
    let pw_w = total.saturating_sub(name_w + uid_w + strat_w + 3); // 3 separators
    (name_w, uid_w, strat_w, pw_w)
}

/// Format a row with the four columns.
fn format_columns(name: &str, uid: &str, strategy: &str, password: &str, total: usize) -> String {
    let (name_w, uid_w, strat_w, pw_w) = column_widths(total);
    let line = format!(
        "{:<nw$} {:<uw$} {:<sw$} {:<pw$}",
        truncate(name, name_w),
        truncate(uid, uid_w),
        truncate(strategy, strat_w),
        truncate(password, pw_w),
        nw = name_w,
        uw = uid_w,
        sw = strat_w,
        pw = pw_w,
    );
    // Ensure exact width (pad or truncate).
    if line.len() < total {
        format!("{:<width$}", line, width = total)
    } else {
        line[..total].to_string()
    }
}

/// Compute scroll offset to keep the cursor visible within the viewport.
fn compute_scroll(cursor: usize, item_count: usize, viewport: usize) -> usize {
    if item_count <= viewport {
        return 0;
    }
    let max_scroll = item_count.saturating_sub(viewport);
    cursor.min(max_scroll)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_entries() -> Vec<UserEntry> {
        vec![
            UserEntry {
                username: "root".to_string(),
                uid: 0,
                strategy: UserContainerfileStrategy::Useradd,
                has_password: true,
                password_choice: UserPasswordChoice::Preserve,
            },
            UserEntry {
                username: "alice".to_string(),
                uid: 1000,
                strategy: UserContainerfileStrategy::Useradd,
                has_password: true,
                password_choice: UserPasswordChoice::New,
            },
            UserEntry {
                username: "nobody".to_string(),
                uid: 65534,
                strategy: UserContainerfileStrategy::Skip,
                has_password: false,
                password_choice: UserPasswordChoice::None,
            },
        ]
    }

    #[test]
    fn strategy_label_maps_variants() {
        let entry = UserEntry {
            username: "test".to_string(),
            uid: 1,
            strategy: UserContainerfileStrategy::Skip,
            has_password: false,
            password_choice: UserPasswordChoice::None,
        };
        assert_eq!(entry.strategy_label(), "skip");

        let entry2 = UserEntry {
            username: "test".to_string(),
            uid: 1,
            strategy: UserContainerfileStrategy::Useradd,
            has_password: false,
            password_choice: UserPasswordChoice::None,
        };
        assert_eq!(entry2.strategy_label(), "useradd");
    }

    #[test]
    fn password_label_maps_variants() {
        let entry = UserEntry {
            username: "t".to_string(),
            uid: 1,
            strategy: UserContainerfileStrategy::Skip,
            has_password: false,
            password_choice: UserPasswordChoice::None,
        };
        assert_eq!(entry.password_label(), "none");

        let entry2 = UserEntry {
            username: "t".to_string(),
            uid: 1,
            strategy: UserContainerfileStrategy::Skip,
            has_password: true,
            password_choice: UserPasswordChoice::Preserve,
        };
        assert_eq!(entry2.password_label(), "preserve");

        let entry3 = UserEntry {
            username: "t".to_string(),
            uid: 1,
            strategy: UserContainerfileStrategy::Skip,
            has_password: false,
            password_choice: UserPasswordChoice::New,
        };
        assert_eq!(entry3.password_label(), "new");
    }

    #[test]
    fn next_strategy_cycles() {
        let entry = UserEntry {
            username: "t".to_string(),
            uid: 1,
            strategy: UserContainerfileStrategy::Skip,
            has_password: false,
            password_choice: UserPasswordChoice::None,
        };
        assert_eq!(entry.next_strategy(), UserContainerfileStrategy::Useradd);

        let entry2 = UserEntry {
            username: "t".to_string(),
            uid: 1,
            strategy: UserContainerfileStrategy::Useradd,
            has_password: false,
            password_choice: UserPasswordChoice::None,
        };
        assert_eq!(entry2.next_strategy(), UserContainerfileStrategy::Skip);
    }

    #[test]
    fn next_password_choice_cycles() {
        // None -> Preserve -> None (New excluded from TUI cycle).
        let entry = UserEntry {
            username: "t".to_string(),
            uid: 1,
            strategy: UserContainerfileStrategy::Skip,
            has_password: false,
            password_choice: UserPasswordChoice::None,
        };
        assert_eq!(entry.next_password_choice(), UserPasswordChoice::Preserve);

        let entry2 = UserEntry {
            username: "t".to_string(),
            uid: 1,
            strategy: UserContainerfileStrategy::Skip,
            has_password: true,
            password_choice: UserPasswordChoice::Preserve,
        };
        assert_eq!(entry2.next_password_choice(), UserPasswordChoice::None);

        // New (set externally) cycles back to None.
        let entry3 = UserEntry {
            username: "t".to_string(),
            uid: 1,
            strategy: UserContainerfileStrategy::Skip,
            has_password: false,
            password_choice: UserPasswordChoice::New,
        };
        assert_eq!(entry3.next_password_choice(), UserPasswordChoice::None);
    }

    #[test]
    fn renders_empty_entries() {
        let entries: Vec<UserEntry> = Vec::new();
        let widget = UserStrategyWidget::new(&entries, 0, true, ColorTier::Mono);
        let area = Rect::new(0, 0, 60, 5);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        // Should render header + column headers without panic.
    }

    #[test]
    fn renders_with_entries() {
        let entries = test_entries();
        let widget = UserStrategyWidget::new(&entries, 1, true, ColorTier::Mono);
        let area = Rect::new(0, 0, 60, 6);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        // Should render all 3 entries with cursor on alice.
    }

    #[test]
    fn zero_area_renders_nothing() {
        let entries = test_entries();
        let widget = UserStrategyWidget::new(&entries, 0, true, ColorTier::Mono);
        let area = Rect::new(0, 0, 0, 0);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
    }

    #[test]
    fn column_widths_sums_correctly() {
        let (name_w, uid_w, strat_w, pw_w) = column_widths(80);
        // 3 separator spaces between 4 columns.
        assert_eq!(name_w + uid_w + strat_w + pw_w + 3, 80);
    }

    #[test]
    fn compute_scroll_keeps_cursor_visible() {
        assert_eq!(compute_scroll(0, 10, 5), 0);
        assert_eq!(compute_scroll(3, 10, 5), 3);
        assert_eq!(compute_scroll(6, 10, 5), 5);
    }

    #[test]
    fn compute_scroll_no_scroll_when_fits() {
        assert_eq!(compute_scroll(2, 3, 5), 0);
    }

    #[test]
    fn truncate_short_unchanged() {
        assert_eq!(truncate("hi", 10), "hi");
    }

    #[test]
    fn truncate_long_adds_ellipsis() {
        let result = truncate("very-long-username", 8);
        assert!(result.len() <= 10); // utf8 ellipsis is 3 bytes
        assert!(result.ends_with('\u{2026}'));
    }
}
