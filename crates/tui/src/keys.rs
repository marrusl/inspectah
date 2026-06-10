use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::Action;
use crate::types::InputMode;

/// Map a crossterm key event to a TUI action based on current input mode.
pub fn map_key(key: KeyEvent, mode: InputMode) -> Action {
    match mode {
        InputMode::Normal => map_normal(key),
        InputMode::Search => map_text_input(key),
        InputMode::Command => map_command_input(key),
        InputMode::Confirm => map_confirm(key),
        InputMode::Help => map_help(key),
    }
}

fn map_normal(key: KeyEvent) -> Action {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('r') => Action::Redo,
            KeyCode::Char('c') => Action::Quit,
            _ => Action::Noop,
        };
    }

    match key.code {
        // Navigation
        KeyCode::Char('j') | KeyCode::Down => Action::CursorDown,
        KeyCode::Char('k') | KeyCode::Up => Action::CursorUp,
        KeyCode::Char('h') | KeyCode::Left => Action::FocusSidebar,
        KeyCode::Char('l') | KeyCode::Right => Action::FocusItems,
        KeyCode::Tab => Action::CycleFocus,
        KeyCode::Char('g') => Action::CursorTop,
        KeyCode::Char('G') => Action::CursorBottom,
        KeyCode::Char('{') => Action::PrevGroup,
        KeyCode::Char('}') => Action::NextGroup,

        // Section jumps (1-9)
        KeyCode::Char(ch @ '1'..='9') => {
            Action::JumpToSection(ch.to_digit(10).expect("matched '1'..='9'") as usize - 1)
        }

        // Item interaction
        KeyCode::Char(' ') => Action::ToggleItem,
        KeyCode::Enter => Action::OpenDetail,
        KeyCode::Esc => Action::CloseDetail,
        KeyCode::Char('n') => Action::DetailNext,
        KeyCode::Char('p') => Action::DetailPrev,
        KeyCode::Char('f') => Action::PromoteDetail,

        // Session
        KeyCode::Char('u') => Action::Undo,
        KeyCode::Char('r') => Action::Refresh,

        // Overlays
        KeyCode::Char('/') => Action::EnterSearch,
        KeyCode::Char(':') => Action::EnterCommand,
        KeyCode::Char('?') => Action::ShowHelp,
        KeyCode::Char('c') => Action::ToggleContainerfile,

        // Quit
        KeyCode::Char('q') => Action::Quit,

        _ => Action::Noop,
    }
}

fn map_text_input(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => Action::CancelInput,
        KeyCode::Enter => Action::SubmitInput,
        KeyCode::Backspace => Action::InputBackspace,
        KeyCode::Delete => Action::InputDelete,
        KeyCode::Left => Action::InputLeft,
        KeyCode::Right => Action::InputRight,
        KeyCode::Home => Action::InputHome,
        KeyCode::End => Action::InputEnd,
        KeyCode::Char(ch) => Action::InputChar(ch),
        _ => Action::Noop,
    }
}

fn map_command_input(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Tab => Action::TabComplete,
        KeyCode::Esc => Action::CancelInput,
        KeyCode::Enter => Action::SubmitInput,
        KeyCode::Backspace => Action::InputBackspace,
        KeyCode::Delete => Action::InputDelete,
        KeyCode::Left => Action::InputLeft,
        KeyCode::Right => Action::InputRight,
        KeyCode::Home => Action::InputHome,
        KeyCode::End => Action::InputEnd,
        KeyCode::Char(ch) => Action::InputChar(ch),
        _ => Action::Noop,
    }
}

fn map_confirm(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => Action::ConfirmYes,
        _ => Action::ConfirmNo,
    }
}

fn map_help(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => Action::CloseDetail,
        _ => Action::Noop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    #[test]
    fn normal_mode_navigation() {
        assert_eq!(
            map_key(key(KeyCode::Char('j')), InputMode::Normal),
            Action::CursorDown
        );
        assert_eq!(
            map_key(key(KeyCode::Char('k')), InputMode::Normal),
            Action::CursorUp
        );
        assert_eq!(
            map_key(key(KeyCode::Down), InputMode::Normal),
            Action::CursorDown
        );
        assert_eq!(
            map_key(key(KeyCode::Up), InputMode::Normal),
            Action::CursorUp
        );
        assert_eq!(
            map_key(key(KeyCode::Char('h')), InputMode::Normal),
            Action::FocusSidebar
        );
        assert_eq!(
            map_key(key(KeyCode::Char('l')), InputMode::Normal),
            Action::FocusItems
        );
        assert_eq!(
            map_key(key(KeyCode::Tab), InputMode::Normal),
            Action::CycleFocus
        );
        assert_eq!(
            map_key(key(KeyCode::Char('g')), InputMode::Normal),
            Action::CursorTop
        );
        assert_eq!(
            map_key(key(KeyCode::Char('G')), InputMode::Normal),
            Action::CursorBottom
        );
    }

    #[test]
    fn normal_mode_actions() {
        assert_eq!(
            map_key(key(KeyCode::Char(' ')), InputMode::Normal),
            Action::ToggleItem
        );
        assert_eq!(
            map_key(key(KeyCode::Enter), InputMode::Normal),
            Action::OpenDetail
        );
        assert_eq!(
            map_key(key(KeyCode::Esc), InputMode::Normal),
            Action::CloseDetail
        );
        assert_eq!(
            map_key(key(KeyCode::Char('u')), InputMode::Normal),
            Action::Undo
        );
        assert_eq!(
            map_key(ctrl(KeyCode::Char('r')), InputMode::Normal),
            Action::Redo
        );
        assert_eq!(
            map_key(key(KeyCode::Char('q')), InputMode::Normal),
            Action::Quit
        );
    }

    #[test]
    fn normal_mode_overlays() {
        assert_eq!(
            map_key(key(KeyCode::Char('/')), InputMode::Normal),
            Action::EnterSearch
        );
        assert_eq!(
            map_key(key(KeyCode::Char(':')), InputMode::Normal),
            Action::EnterCommand
        );
        assert_eq!(
            map_key(key(KeyCode::Char('?')), InputMode::Normal),
            Action::ShowHelp
        );
        assert_eq!(
            map_key(key(KeyCode::Char('c')), InputMode::Normal),
            Action::ToggleContainerfile
        );
    }

    #[test]
    fn section_number_jumps() {
        for n in 1..=9u8 {
            let ch = (b'0' + n) as char;
            assert_eq!(
                map_key(key(KeyCode::Char(ch)), InputMode::Normal),
                Action::JumpToSection(n as usize - 1)
            );
        }
    }

    #[test]
    fn search_mode_keys() {
        assert_eq!(
            map_key(key(KeyCode::Esc), InputMode::Search),
            Action::CancelInput
        );
        assert_eq!(
            map_key(key(KeyCode::Enter), InputMode::Search),
            Action::SubmitInput
        );
        assert_eq!(
            map_key(key(KeyCode::Char('a')), InputMode::Search),
            Action::InputChar('a')
        );
        assert_eq!(
            map_key(key(KeyCode::Backspace), InputMode::Search),
            Action::InputBackspace
        );
    }

    #[test]
    fn command_mode_tab_complete() {
        assert_eq!(
            map_key(key(KeyCode::Tab), InputMode::Command),
            Action::TabComplete
        );
    }

    #[test]
    fn confirm_mode() {
        assert_eq!(
            map_key(key(KeyCode::Char('y')), InputMode::Confirm),
            Action::ConfirmYes
        );
        assert_eq!(
            map_key(key(KeyCode::Char('n')), InputMode::Confirm),
            Action::ConfirmNo
        );
        assert_eq!(
            map_key(key(KeyCode::Esc), InputMode::Confirm),
            Action::ConfirmNo
        );
        assert_eq!(
            map_key(key(KeyCode::Enter), InputMode::Confirm),
            Action::ConfirmNo
        );
    }
}
