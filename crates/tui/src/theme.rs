use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorTier {
    Mono,
    Ansi16,
    TrueColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    TriageInvestigate,
    TriageSite,
    TriageBaseline,
    TextPrimary,
    TextMuted,
    DiffAdded,
    DiffRemoved,
    StatusIncluded,
    StatusExcluded,
    StatusLocked,
    FocusBorder,
    FocusUnfocused,
    FocusSelected,
    SearchMatch,
    Warning,
    Error,
}

impl Token {
    pub fn style(self, tier: ColorTier) -> Style {
        match tier {
            ColorTier::Mono => self.mono_style(),
            ColorTier::Ansi16 => self.ansi16_style(),
            ColorTier::TrueColor => self.truecolor_style(),
        }
    }

    fn mono_style(self) -> Style {
        match self {
            Self::TriageInvestigate => Style::default().add_modifier(Modifier::BOLD),
            Self::TriageSite => Style::default(),
            Self::TriageBaseline => Style::default().add_modifier(Modifier::DIM),
            Self::TextPrimary => Style::default(),
            Self::TextMuted => Style::default().add_modifier(Modifier::DIM),
            Self::DiffAdded => Style::default(),
            Self::DiffRemoved => Style::default(),
            Self::StatusIncluded => Style::default(),
            Self::StatusExcluded => Style::default().add_modifier(Modifier::DIM),
            Self::StatusLocked => Style::default().add_modifier(Modifier::DIM),
            Self::FocusBorder => Style::default().add_modifier(Modifier::BOLD),
            Self::FocusUnfocused => Style::default(),
            Self::FocusSelected => {
                Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
            }
            Self::SearchMatch => Style::default().add_modifier(Modifier::REVERSED),
            Self::Warning => Style::default().add_modifier(Modifier::BOLD),
            Self::Error => Style::default().add_modifier(Modifier::BOLD),
        }
    }

    fn ansi16_style(self) -> Style {
        match self {
            Self::TriageInvestigate => Style::default().fg(Color::Red),
            Self::TriageSite => Style::default().fg(Color::Yellow),
            Self::TriageBaseline => Style::default().fg(Color::Green),
            Self::TextPrimary => Style::default(),
            Self::TextMuted => Style::default().fg(Color::DarkGray),
            Self::DiffAdded => Style::default().fg(Color::Green),
            Self::DiffRemoved => Style::default().fg(Color::Red),
            Self::StatusIncluded => Style::default().fg(Color::Green),
            Self::StatusExcluded => Style::default().add_modifier(Modifier::DIM),
            Self::StatusLocked => Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
            Self::FocusBorder => Style::default().fg(Color::Cyan),
            Self::FocusUnfocused => Style::default().add_modifier(Modifier::DIM),
            Self::FocusSelected => Style::default().add_modifier(Modifier::REVERSED),
            Self::SearchMatch => Style::default().add_modifier(Modifier::REVERSED),
            Self::Warning => Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            Self::Error => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        }
    }

    fn truecolor_style(self) -> Style {
        // TrueColor uses same palette as Ansi16 for v1.
        self.ansi16_style()
    }
}

/// Detect color support from environment. Separated from env access for testing.
pub fn resolve_tier_from_env(no_color: &str, colorterm: Option<&str>) -> ColorTier {
    if !no_color.is_empty() {
        return ColorTier::Mono;
    }
    match colorterm {
        Some(ct) if ct.eq_ignore_ascii_case("truecolor") || ct.eq_ignore_ascii_case("24bit") => {
            ColorTier::TrueColor
        }
        _ => ColorTier::Ansi16,
    }
}

/// Detect color support from the current environment.
pub fn detect_color_tier() -> ColorTier {
    let no_color = std::env::var("NO_COLOR").unwrap_or_default();
    let colorterm = std::env::var("COLORTERM").ok();
    resolve_tier_from_env(&no_color, colorterm.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_color_env_forces_mono() {
        let tier = resolve_tier_from_env("1", None);
        assert_eq!(tier, ColorTier::Mono);
    }

    #[test]
    fn colorterm_truecolor_detected() {
        let tier = resolve_tier_from_env("", Some("truecolor"));
        assert_eq!(tier, ColorTier::TrueColor);
    }

    #[test]
    fn colorterm_24bit_detected() {
        let tier = resolve_tier_from_env("", Some("24bit"));
        assert_eq!(tier, ColorTier::TrueColor);
    }

    #[test]
    fn no_colorterm_defaults_to_ansi16() {
        let tier = resolve_tier_from_env("", None);
        assert_eq!(tier, ColorTier::Ansi16);
    }
}
