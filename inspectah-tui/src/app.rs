use std::io;
use std::time::Duration;

use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, execute};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use inspectah_refine::session::RefineSession;

use crate::action::Action;
use crate::event::{Event, EventReader};
use crate::keys::map_key;
use crate::screen::Screen;
use crate::screen::single_host::SingleHostScreen;
use crate::theme::{ColorTier, detect_color_tier};
use crate::types::TuiState;

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
        if action == Action::Quit {
            self.should_quit = true;
        }
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
