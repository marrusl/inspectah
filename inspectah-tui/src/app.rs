use std::io;
use std::time::Duration;

use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, execute};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use inspectah_refine::session::RefineSession;
use inspectah_refine::types::RefinementOp;

use crate::action::Action;
use crate::event::{Event, EventReader};
use crate::keys::map_key;
use crate::screen::Screen;
use crate::screen::single_host::SingleHostScreen;
use crate::sections::{self, SECTION_ORDER};
use crate::theme::{ColorTier, detect_color_tier};
use crate::types::{DetailMode, FlashMessage, FocusTarget, TuiState};

use crate::widget::triage_list::TriageGroup;

/// Map a `TriageGroup` to its index in the canonical bucket order
/// (Investigate=0, Site=1, Baseline=2), matching `collapsed_groups` keys.
fn group_to_bucket_index(group: TriageGroup) -> usize {
    match group {
        TriageGroup::Investigate => 0,
        TriageGroup::Site => 1,
        TriageGroup::Baseline => 2,
    }
}

/// RAII guard -- restores terminal on drop (including panics).
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, cursor::Show);
    }
}

// Fields `tarball_path` and `pending_export_path` are wired in later phases (export).
#[allow(dead_code)]
pub struct App {
    session: RefineSession,
    state: TuiState,
    tier: ColorTier,
    should_quit: bool,
    screen: Screen,
    /// Tarball path for :fresh reload and export default path.
    tarball_path: Option<std::path::PathBuf>,
    /// Pending export path set by :export command.
    pending_export_path: Option<std::path::PathBuf>,
}

impl App {
    pub fn new(session: RefineSession) -> Self {
        let section_count = 14; // 7 decision/composite + 7 reference
        Self {
            session,
            state: TuiState::new(section_count),
            tier: detect_color_tier(),
            should_quit: false,
            screen: Screen::SingleHost(SingleHostScreen::new()),
            tarball_path: None,
            pending_export_path: None,
        }
    }

    pub fn run(mut self) -> color_eyre::Result<()> {
        // 1. Install color-eyre BEFORE alt screen (ignore if already installed)
        let _ = color_eyre::install();

        // 2. Terminal guard (Drop restores)
        let _guard = TerminalGuard;

        // 3. Enable raw mode
        terminal::enable_raw_mode()?;

        // 4. Enter alternate screen, hide cursor
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, cursor::Hide)?;

        // 5. Create ratatui terminal
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // 6. Register signal handlers for Ctrl+Z suspend/resume
        let mut signals = signal_hook::iterator::Signals::new([
            signal_hook::consts::SIGTSTP,
            signal_hook::consts::SIGCONT,
        ])?;

        // Spawn thread that handles SIGTSTP/SIGCONT
        let _signal_thread = std::thread::spawn(move || {
            for sig in signals.forever() {
                match sig {
                    signal_hook::consts::SIGTSTP => {
                        let _ = terminal::disable_raw_mode();
                        let _ = execute!(io::stdout(), LeaveAlternateScreen, cursor::Show);
                        // SAFETY: Re-raising SIGTSTP with the default handler to actually
                        // suspend the process. `libc::signal` sets the disposition to
                        // SIG_DFL, then `libc::raise` sends the signal to the calling
                        // thread. Both calls are well-defined for SIGTSTP on POSIX.
                        unsafe {
                            libc::signal(libc::SIGTSTP, libc::SIG_DFL);
                            libc::raise(libc::SIGTSTP);
                        }
                    }
                    signal_hook::consts::SIGCONT => {
                        let _ = terminal::enable_raw_mode();
                        let _ = execute!(io::stdout(), EnterAlternateScreen, cursor::Hide);
                        // SAFETY: Restoring the default SIGTSTP handler so a subsequent
                        // Ctrl+Z will suspend again. `libc::signal` is safe for this
                        // well-defined signal number on all POSIX systems.
                        unsafe {
                            libc::signal(libc::SIGTSTP, libc::SIG_DFL);
                        }
                    }
                    _ => {}
                }
            }
        });

        // 7. Event reader thread (250ms tick)
        let events = EventReader::new(Duration::from_millis(250));

        // 8. Main event loop
        while !self.should_quit {
            terminal.draw(|frame| {
                self.render(frame);
            })?;

            match events.next() {
                Some(Event::Key(key)) => {
                    let action = map_key(key, self.state.input_mode);
                    self.handle_action(action);
                }
                Some(Event::Resize(_, _)) => {
                    // Terminal handles resize automatically
                }
                Some(Event::Tick) => {
                    if let Some(ref flash) = self.state.flash
                        && flash.is_expired()
                    {
                        self.state.flash = None;
                    }
                }
                None => break,
            }
        }

        Ok(())
    }

    fn handle_action(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,

            // Navigation — focus-aware
            Action::CursorDown => match self.state.focus {
                FocusTarget::Sidebar => {
                    let sections = sections::build_section_entries(&self.session);
                    if self.state.active_section < sections.len() - 1 {
                        self.state.section_cursors[self.state.active_section] = self.state.cursor;
                        self.state.active_section += 1;
                        self.state.cursor = self.state.section_cursors[self.state.active_section];
                    }
                }
                FocusTarget::ItemList | FocusTarget::DetailPane => {
                    let max = self.visible_item_count().saturating_sub(1);
                    if self.state.cursor < max {
                        self.state.cursor += 1;
                    }
                }
            },
            Action::CursorUp => match self.state.focus {
                FocusTarget::Sidebar => {
                    if self.state.active_section > 0 {
                        self.state.section_cursors[self.state.active_section] = self.state.cursor;
                        self.state.active_section -= 1;
                        self.state.cursor = self.state.section_cursors[self.state.active_section];
                    }
                }
                FocusTarget::ItemList | FocusTarget::DetailPane => {
                    if self.state.cursor > 0 {
                        self.state.cursor -= 1;
                    }
                }
            },
            Action::CursorTop => self.state.cursor = 0,
            Action::CursorBottom => {
                self.state.cursor = self.visible_item_count().saturating_sub(1);
            }

            // Focus
            Action::FocusSidebar => self.state.focus = FocusTarget::Sidebar,
            Action::FocusItems => self.state.focus = FocusTarget::ItemList,
            Action::CycleFocus => {
                self.state.focus = match self.state.focus {
                    FocusTarget::Sidebar => FocusTarget::ItemList,
                    FocusTarget::ItemList => {
                        if self.state.detail_mode != DetailMode::None {
                            FocusTarget::DetailPane
                        } else {
                            FocusTarget::Sidebar
                        }
                    }
                    FocusTarget::DetailPane => FocusTarget::Sidebar,
                };
            }

            // Section jump (1-9)
            Action::JumpToSection(idx) => {
                let sections = sections::build_section_entries(&self.session);
                if idx < sections.len() {
                    self.state.section_cursors[self.state.active_section] = self.state.cursor;
                    self.state.active_section = idx;
                    self.state.cursor = self.state.section_cursors[idx];
                    self.state.focus = FocusTarget::ItemList;
                }
            }

            // Group navigation
            Action::NextGroup => {
                let items = self.current_items();
                if let Some((i, _)) = items
                    .iter()
                    .enumerate()
                    .skip(self.state.cursor + 1)
                    .find(|(_, item)| item.is_group_header)
                {
                    self.state.cursor = i;
                }
            }
            Action::PrevGroup => {
                let items = self.current_items();
                if let Some((i, _)) = items
                    .iter()
                    .enumerate()
                    .take(self.state.cursor)
                    .rev()
                    .find(|(_, item)| item.is_group_header)
                {
                    self.state.cursor = i;
                }
            }

            // Group collapse/expand on Enter when on group header
            Action::OpenDetail => {
                let items = self.current_items();
                if let Some(item) = items.get(self.state.cursor) {
                    if item.is_group_header {
                        let group_idx = group_to_bucket_index(item.group);
                        let key = (self.state.active_section, group_idx);
                        if self.state.collapsed_groups.contains(&key) {
                            self.state.collapsed_groups.remove(&key);
                        } else {
                            self.state.collapsed_groups.insert(key);
                        }
                    } else {
                        self.state.detail_mode = DetailMode::InfoBar;
                    }
                }
            }

            // Item toggling
            Action::ToggleItem => {
                let items = self.current_items();
                if let Some(item) = items.get(self.state.cursor)
                    && let Some(ref item_id) = item.item_id
                {
                    let new_include = !item.included.unwrap_or(true);
                    let op = RefinementOp::SetInclude {
                        item_id: item_id.clone(),
                        include: new_include,
                    };
                    if let Err(e) = self.session.apply(op) {
                        self.state.flash =
                            Some(FlashMessage::new(format!("Toggle failed: {e}"), 3));
                    }
                }
            }
            Action::Undo => {
                if let Err(e) = self.session.undo() {
                    self.state.flash = Some(FlashMessage::new(format!("Undo: {e}"), 3));
                }
            }
            Action::Redo => {
                if let Err(e) = self.session.redo() {
                    self.state.flash = Some(FlashMessage::new(format!("Redo: {e}"), 3));
                }
            }

            _ => {}
        }
    }

    /// Count of visible items in the current section (respecting collapsed groups).
    fn visible_item_count(&self) -> usize {
        self.current_items().len()
    }

    /// Build the flat list of items for the currently active section.
    fn current_items(&self) -> Vec<crate::widget::triage_list::ListItem> {
        let active_section_id = SECTION_ORDER
            .get(self.state.active_section)
            .copied()
            .unwrap_or(crate::types::SectionId::Packages);
        crate::screen::single_host::build_list_items(&self.session, active_section_id, &self.state)
    }

    fn render(&self, frame: &mut ratatui::Frame) {
        let area = frame.area();

        if area.width < 80 || area.height < 24 {
            let msg = ratatui::widgets::Paragraph::new(format!(
                "Terminal too small ({}x{}). Minimum: 80x24.",
                area.width, area.height
            ));
            frame.render_widget(msg, area);
            return;
        }

        self.screen
            .render(frame, &self.session, &self.state, self.tier);
    }
}
