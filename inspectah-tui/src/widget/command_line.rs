use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::theme::{ColorTier, Token};

/// Widget that renders a `:` command prompt with input and optional ghost
/// text for tab completion.
pub struct CommandLineWidget<'a> {
    input: &'a str,
    completion: Option<&'a str>,
    tier: ColorTier,
}

impl<'a> CommandLineWidget<'a> {
    pub fn new(input: &'a str, completion: Option<&'a str>, tier: ColorTier) -> Self {
        Self {
            input,
            completion,
            tier,
        }
    }
}

impl Widget for CommandLineWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 3 || area.height < 1 {
            return;
        }

        // Clear the status bar area so the command line replaces it.
        for x in area.x..area.right() {
            buf[(x, area.y)].reset();
        }

        let w = area.width as usize;

        // Render `:` prompt + input text.
        let prompt = format!(":{}", self.input);
        let prompt_display = if prompt.len() > w {
            &prompt[..w]
        } else {
            &prompt
        };
        buf.set_string(
            area.x,
            area.y,
            prompt_display,
            Token::Warning.style(self.tier),
        );

        // Ghost text for tab completion (dimmed, after the typed input).
        if let Some(completion) = self.completion {
            let typed_len = prompt.len();
            if typed_len < w && completion.len() > self.input.len() {
                let ghost = &completion[self.input.len()..];
                let avail = w - typed_len;
                let ghost_display = if ghost.len() > avail {
                    &ghost[..avail]
                } else {
                    ghost
                };
                buf.set_string(
                    area.x + typed_len as u16,
                    area.y,
                    ghost_display,
                    Token::TextMuted.style(self.tier),
                );
            }
        }
    }
}

/// Available commands with tab-completable names.
pub const COMMANDS: &[&str] = &[
    "export", "fresh", "quit", "redo", "save", "search", "section", "stats", "undo",
];

/// Parse command input into (command_name, args).
///
/// Returns `None` for empty input.
pub fn parse_command(input: &str) -> Option<(&str, &str)> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut parts = trimmed.splitn(2, ' ');
    let cmd = parts.next()?;
    let args = parts.next().unwrap_or("");
    Some((cmd, args))
}

/// Find a unique tab completion match for the given partial input.
///
/// Returns `Some(full_command)` when exactly one command matches the prefix,
/// `None` when zero or multiple commands match (ambiguous).
pub fn complete(input: &str) -> Option<String> {
    if input.is_empty() {
        return None;
    }
    let lower = input.to_lowercase();
    let matches: Vec<&&str> = COMMANDS.iter().filter(|c| c.starts_with(&lower)).collect();
    if matches.len() == 1 {
        Some(matches[0].to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_command_with_args() {
        let (cmd, args) = parse_command("section packages").unwrap();
        assert_eq!(cmd, "section");
        assert_eq!(args, "packages");
    }

    #[test]
    fn parse_command_simple() {
        let (cmd, args) = parse_command("quit").unwrap();
        assert_eq!(cmd, "quit");
        assert_eq!(args, "");
    }

    #[test]
    fn complete_partial_unique() {
        assert_eq!(complete("st"), Some("stats".into()));
    }

    #[test]
    fn complete_ambiguous_returns_none() {
        // "s" matches "save", "search", "section", "stats" -- ambiguous.
        assert_eq!(complete("s"), None);
    }
}
