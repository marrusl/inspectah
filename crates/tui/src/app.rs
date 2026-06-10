use std::io;
use std::time::Duration;

use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, execute};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use inspectah_core::types::users::UserPasswordChoice;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, RefinementOp, UserPasswordOp};

use crate::action::Action;
use crate::event::{Event, EventReader};
use crate::keys::map_key;
use crate::screen::Screen;
use crate::screen::single_host::{SingleHostScreen, build_user_entries};
use crate::sections::{self, SECTION_ORDER};
use crate::theme::{ColorTier, detect_color_tier};
use crate::types::{DetailMode, FlashMessage, FocusTarget, InputMode, SectionId, TuiState};

use crate::widget::command_line::{self, CommandLineWidget};
use crate::widget::help_screen::HelpScreenWidget;
use crate::widget::search::{self, SearchResult, SearchWidget};
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

/// Derive the item-level key portion for `mark_viewed()`.
///
/// The full viewed ID is `"{prefix}:{item_key}"` where prefix comes from
/// `SectionId::viewed_prefix()` and item_key is this function's output.
fn item_id_to_viewed_key(item_id: &ItemId) -> String {
    match item_id {
        ItemId::Package { name, arch } => format!("{name}.{arch}"),
        ItemId::Repo { path } => path.clone(),
        ItemId::ModuleStream { module_stream } => module_stream.clone(),
        ItemId::VersionLock { name_arch } => name_arch.clone(),
        ItemId::Config { path } => path.clone(),
        ItemId::Service { unit } => unit.clone(),
        ItemId::DropIn { path } => path.clone(),
        ItemId::Quadlet { path } => path.clone(),
        ItemId::Compose { path } => path.clone(),
        ItemId::Flatpak {
            app_id,
            remote,
            branch,
        } => format!("{app_id}/{remote}/{branch}"),
        ItemId::NMConnection { path } => path.clone(),
        ItemId::FirewallZone { path } => path.clone(),
        ItemId::KernelModule { name } => name.clone(),
        ItemId::Sysctl { key } => key.clone(),
        ItemId::TunedSelection { profile } => profile.clone(),
        ItemId::CronJob { path } => path.clone(),
        ItemId::SystemdTimer { name } => name.clone(),
        ItemId::AtJob { file } => file.clone(),
        ItemId::GeneratedTimer { name } => name.clone(),
        ItemId::SelinuxPort { protocol_port } => protocol_port.clone(),
        ItemId::Fstab { mount_point } => mount_point.clone(),
        ItemId::NonRpm { name } => name.clone(),
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
    /// Current search results (populated in real-time as user types).
    search_results: Vec<SearchResult>,
    /// Currently selected index in search results.
    search_selected: usize,
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
            search_results: Vec::new(),
            search_selected: 0,
        }
    }

    pub fn set_tarball_path(&mut self, path: std::path::PathBuf) {
        self.tarball_path = Some(path);
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
                        // signal-hook's sigaction-based SIGTSTP handler persists
                        // across suspend/resume -- no need to re-register it here.
                        // The SIGTSTP handler resets to SIG_DFL before raise() (so
                        // the process actually suspends), and signal-hook restores
                        // our handler automatically on return from the signal.
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

            // Search-mode cursor movement (must precede unconditional arms).
            Action::CursorDown
                if self.state.input_mode == InputMode::Search
                    && !self.search_results.is_empty() =>
            {
                self.search_selected =
                    (self.search_selected + 1).min(self.search_results.len() - 1);
            }
            Action::CursorUp if self.state.input_mode == InputMode::Search => {
                self.search_selected = self.search_selected.saturating_sub(1);
            }

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
                FocusTarget::DetailPane if self.state.detail_mode == DetailMode::Fullscreen => {
                    // Scroll detail content down.
                    self.state.detail_scroll = self.state.detail_scroll.saturating_add(1);
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
                FocusTarget::DetailPane if self.state.detail_mode == DetailMode::Fullscreen => {
                    // Scroll detail content up.
                    self.state.detail_scroll = self.state.detail_scroll.saturating_sub(1);
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

            // Group collapse/expand on Enter when on group header,
            // or open detail view for items.
            // Users section: Enter cycles password choice instead.
            Action::OpenDetail => {
                let active_section_id = SECTION_ORDER
                    .get(self.state.active_section)
                    .copied()
                    .unwrap_or(SectionId::Packages);
                if active_section_id == SectionId::Users {
                    let entries = build_user_entries(&self.session);
                    if let Some(entry) = entries.get(self.state.cursor) {
                        let next = entry.next_password_choice();
                        let op = RefinementOp::UserPassword(match next {
                            UserPasswordChoice::None => UserPasswordOp::None {
                                username: entry.username.clone(),
                            },
                            UserPasswordChoice::Preserve => UserPasswordOp::Preserve {
                                username: entry.username.clone(),
                            },
                            // New is excluded from TUI cycling (requires hash input),
                            // but handle the variant exhaustively in case it's set
                            // externally (e.g., session loaded from web UI).
                            UserPasswordChoice::New => UserPasswordOp::None {
                                username: entry.username.clone(),
                            },
                        });
                        if let Err(e) = self.session.apply(op) {
                            self.state.flash =
                                Some(FlashMessage::new(format!("Password toggle: {e}"), 3));
                        }
                    }
                } else {
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
                        } else if item.has_content {
                            // Items with rich content open in fullscreen.
                            self.state.detail_mode = DetailMode::Fullscreen;
                            self.state.detail_scroll = 0;
                        } else {
                            // Items without content get the info bar.
                            self.state.detail_mode = DetailMode::InfoBar;
                        }
                        // Mark item as viewed if the section supports tracking.
                        if let Some(ref item_id) = item.item_id
                            && let Some(prefix) = active_section_id.viewed_prefix()
                        {
                            let key = item_id_to_viewed_key(item_id);
                            if !key.is_empty() {
                                let viewed_id = format!("{prefix}:{key}");
                                let _ = self.session.mark_viewed(&viewed_id);
                            }
                        }
                    }
                }
            }

            // Promote info bar to fullscreen detail.
            Action::PromoteDetail if self.state.detail_mode == DetailMode::InfoBar => {
                self.state.detail_mode = DetailMode::Fullscreen;
                self.state.detail_scroll = 0;
            }

            // Navigate to next item while in detail mode.
            Action::DetailNext if self.state.detail_mode != DetailMode::None => {
                let max = self.visible_item_count().saturating_sub(1);
                if self.state.cursor < max {
                    self.state.cursor += 1;
                    self.state.detail_scroll = 0;
                }
            }

            // Navigate to previous item while in detail mode.
            Action::DetailPrev
                if self.state.detail_mode != DetailMode::None && self.state.cursor > 0 =>
            {
                self.state.cursor -= 1;
                self.state.detail_scroll = 0;
            }

            // Close help overlay
            Action::CloseDetail if self.state.input_mode == InputMode::Help => {
                self.state.input_mode = InputMode::Normal;
            }

            // Close detail view
            Action::CloseDetail if self.state.detail_mode != DetailMode::None => {
                self.state.detail_mode = DetailMode::None;
                self.state.detail_scroll = 0;
                if self.state.focus == FocusTarget::DetailPane {
                    self.state.focus = FocusTarget::ItemList;
                }
            }

            // Help overlay
            Action::ShowHelp => {
                self.state.input_mode = if self.state.input_mode == InputMode::Help {
                    InputMode::Normal
                } else {
                    InputMode::Help
                };
            }

            // Search overlay
            Action::EnterSearch => {
                self.state.input_mode = InputMode::Search;
                self.state.search_query.clear();
                self.search_results.clear();
                self.search_selected = 0;
            }
            Action::InputChar(ch) if self.state.input_mode == InputMode::Search => {
                self.state.search_query.push(ch);
                self.search_results =
                    search::search_all_sections(&self.session, &self.state.search_query);
                self.search_selected = 0;
            }
            Action::InputBackspace if self.state.input_mode == InputMode::Search => {
                self.state.search_query.pop();
                self.search_results =
                    search::search_all_sections(&self.session, &self.state.search_query);
                self.search_selected = 0;
            }
            Action::SubmitInput if self.state.input_mode == InputMode::Search => {
                if let Some(result) = self.search_results.get(self.search_selected) {
                    // Find the section index for the result's section_id.
                    if let Some(idx) = SECTION_ORDER.iter().position(|&s| s == result.section_id) {
                        self.state.section_cursors[self.state.active_section] = self.state.cursor;
                        self.state.active_section = idx;
                        self.state.focus = FocusTarget::ItemList;

                        // Navigate to the specific matching item within the section.
                        let target_name = result.name.clone();
                        let section_items = crate::screen::single_host::build_list_items(
                            &self.session,
                            result.section_id,
                            &self.state,
                        );
                        let item_cursor = section_items
                            .iter()
                            .position(|item| !item.is_group_header && item.name == target_name)
                            .unwrap_or(0);
                        self.state.cursor = item_cursor;
                    }
                }
                self.state.input_mode = InputMode::Normal;
                self.state.search_query.clear();
                self.search_results.clear();
            }
            Action::CancelInput if self.state.input_mode == InputMode::Search => {
                self.state.input_mode = InputMode::Normal;
                self.state.search_query.clear();
                self.search_results.clear();
            }

            // Command mode
            Action::EnterCommand => {
                self.state.input_mode = InputMode::Command;
                self.state.command_input.clear();
            }
            Action::InputChar(ch) if self.state.input_mode == InputMode::Command => {
                self.state.command_input.push(ch);
            }
            Action::InputBackspace if self.state.input_mode == InputMode::Command => {
                self.state.command_input.pop();
            }
            Action::TabComplete if self.state.input_mode == InputMode::Command => {
                if let Some(completed) = command_line::complete(&self.state.command_input) {
                    self.state.command_input = completed;
                }
            }
            Action::SubmitInput if self.state.input_mode == InputMode::Command => {
                self.execute_command();
                // Some commands (e.g. :search) switch to a different input mode;
                // only reset to Normal if still in Command mode.
                if self.state.input_mode == InputMode::Command {
                    self.state.input_mode = InputMode::Normal;
                }
                self.state.command_input.clear();
            }
            Action::CancelInput if self.state.input_mode == InputMode::Command => {
                self.state.input_mode = InputMode::Normal;
                self.state.command_input.clear();
            }

            // Item toggling -- Users section cycles strategy instead of include/exclude.
            // Locked items cannot be toggled (no-op with flash message).
            Action::ToggleItem => {
                let active_section_id = SECTION_ORDER
                    .get(self.state.active_section)
                    .copied()
                    .unwrap_or(SectionId::Packages);
                if active_section_id == SectionId::Users {
                    let entries = build_user_entries(&self.session);
                    if let Some(entry) = entries.get(self.state.cursor) {
                        let op = RefinementOp::UserStrategy {
                            username: entry.username.clone(),
                            strategy: entry.next_strategy(),
                        };
                        if let Err(e) = self.session.apply(op) {
                            self.state.flash =
                                Some(FlashMessage::new(format!("Strategy toggle: {e}"), 3));
                        }
                    }
                } else {
                    let items = self.current_items();
                    if let Some(item) = items.get(self.state.cursor) {
                        if item.locked {
                            let reason = item.lock_reason.as_deref().unwrap_or("item is locked");
                            self.state.flash =
                                Some(FlashMessage::new(format!("Locked: {reason}"), 3));
                        } else if let Some(ref item_id) = item.item_id {
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

            Action::ToggleContainerfile => {
                self.state.show_containerfile = !self.state.show_containerfile;
            }

            // Export confirmation (y/N)
            Action::ConfirmYes if self.state.input_mode == InputMode::Confirm => {
                if let Some(path) = self.pending_export_path.take() {
                    self.do_export(&path);
                }
                self.state.input_mode = InputMode::Normal;
            }
            Action::ConfirmNo if self.state.input_mode == InputMode::Confirm => {
                self.pending_export_path = None;
                self.state.input_mode = InputMode::Normal;
                self.state.flash = Some(FlashMessage::new("Export cancelled.", 3));
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

    /// Execute a parsed command from command mode.
    fn execute_command(&mut self) {
        let input = self.state.command_input.clone();
        let Some((cmd, args)) = command_line::parse_command(&input) else {
            return;
        };

        match cmd {
            "export" | "save" => {
                let path = if args.trim().is_empty() {
                    // Derive export name from input tarball:
                    //   input "foo.tar.gz" → "foo-refined.tar.gz"
                    // Falls back to a generic name when no input path is set.
                    if let Some(ref tp) = self.tarball_path {
                        let stem = tp.file_name().unwrap_or_default().to_string_lossy();
                        let base = stem
                            .strip_suffix(".tar.gz")
                            .or_else(|| stem.strip_suffix(".tgz"))
                            .unwrap_or(&stem);
                        std::path::PathBuf::from(format!("./{base}-refined.tar.gz"))
                    } else {
                        std::path::PathBuf::from("./inspectah-refined.tar.gz")
                    }
                } else {
                    std::path::PathBuf::from(args.trim())
                };
                if self.session.is_sensitive() {
                    self.pending_export_path = Some(path);
                    self.state.input_mode = InputMode::Confirm;
                } else {
                    self.do_export(&path);
                }
            }
            "search" => {
                // Switch to search mode with the args as initial query.
                self.state.input_mode = InputMode::Search;
                self.state.search_query = args.to_string();
                self.search_results =
                    search::search_all_sections(&self.session, &self.state.search_query);
                self.search_selected = 0;
            }
            "section" => {
                let target = args.trim().to_lowercase();
                if target.is_empty() {
                    self.state.flash = Some(FlashMessage::new("Usage: :section <name>", 3));
                } else {
                    // Find section by prefix match on label.
                    let sections = sections::build_section_entries(&self.session);
                    if let Some(idx) = SECTION_ORDER
                        .iter()
                        .position(|s| s.label().to_lowercase().starts_with(&target))
                    {
                        if idx < sections.len() {
                            self.state.section_cursors[self.state.active_section] =
                                self.state.cursor;
                            self.state.active_section = idx;
                            self.state.cursor = self.state.section_cursors[idx];
                            self.state.focus = FocusTarget::ItemList;
                        }
                    } else {
                        self.state.flash = Some(FlashMessage::new(
                            format!("Unknown section: {}", args.trim()),
                            3,
                        ));
                    }
                }
            }
            "stats" => {
                let view = self.session.view();
                let s = &view.stats;
                let msg = format!(
                    "ops: {} | undo: {} | redo: {} | review: {}",
                    s.ops_applied,
                    if s.can_undo { "yes" } else { "no" },
                    if s.can_redo { "yes" } else { "no" },
                    s.needs_review_count,
                );
                self.state.flash = Some(FlashMessage::new(msg, 5));
            }
            "undo" => {
                if let Err(e) = self.session.undo() {
                    self.state.flash = Some(FlashMessage::new(format!("Undo: {e}"), 3));
                }
            }
            "redo" => {
                if let Err(e) = self.session.redo() {
                    self.state.flash = Some(FlashMessage::new(format!("Redo: {e}"), 3));
                }
            }
            "fresh" => {
                if let Some(ref tarball) = self.tarball_path {
                    // Delete the sidecar session file and reload from tarball.
                    let sidecar = inspectah_refine::autosave::session_file_path(tarball);
                    let _ = std::fs::remove_file(&sidecar);
                    match inspectah_refine::tarball::from_tarball(tarball) {
                        Ok(fresh) => {
                            self.session = fresh;
                            // Restore tarball path on the new session so autosave keeps working.
                            if let Some(ref path) = self.tarball_path {
                                self.session.set_tarball_path(path.clone());
                            }
                            self.state = TuiState::new(14);
                            self.state.flash =
                                Some(FlashMessage::new("Session reset from tarball", 3));
                        }
                        Err(e) => {
                            self.state.flash =
                                Some(FlashMessage::new(format!("Fresh reload failed: {e}"), 5));
                        }
                    }
                } else {
                    self.state.flash =
                        Some(FlashMessage::new("No tarball path -- cannot reload", 3));
                }
            }
            "quit" => {
                self.should_quit = true;
            }
            _ => {
                self.state.flash = Some(FlashMessage::new(format!("Unknown command: {cmd}"), 3));
            }
        }
    }

    /// Perform the actual tarball export to `path`.
    fn do_export(&mut self, path: &std::path::Path) {
        let generation = self.session.generation();
        match self.session.export_tarball(path, generation) {
            Ok(()) => {
                self.state.flash = Some(FlashMessage::new(
                    format!("Exported: {}", path.display()),
                    5,
                ));
            }
            Err(e) => {
                self.state.flash = Some(FlashMessage::new(format!("Export failed: {e}"), 5));
            }
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

        // Overlay: help screen
        if self.state.input_mode == InputMode::Help {
            frame.render_widget(HelpScreenWidget::new(self.tier), area);
        }

        // Overlay: search
        if self.state.input_mode == InputMode::Search {
            frame.render_widget(
                SearchWidget::new(
                    &self.state.search_query,
                    &self.search_results,
                    self.search_selected,
                    self.tier,
                ),
                area,
            );
        }

        // Overlay: command line (renders in the status bar row)
        if self.state.input_mode == InputMode::Command {
            let status_area = ratatui::layout::Rect {
                x: area.x,
                y: area.bottom().saturating_sub(1),
                width: area.width,
                height: 1,
            };
            let comp = command_line::complete(&self.state.command_input);
            frame.render_widget(
                CommandLineWidget::new(&self.state.command_input, comp.as_deref(), self.tier),
                status_area,
            );
        }

        // Overlay: export confirmation prompt (3-row warning block at bottom)
        if self.state.input_mode == InputMode::Confirm {
            use ratatui::style::{Color, Style};
            use ratatui::text::{Line, Span};
            use ratatui::widgets::{Block, Borders, Paragraph};

            let confirm_height = 5u16; // 3 text lines + 2 border rows
            let confirm_area = ratatui::layout::Rect {
                x: area.x,
                y: area.bottom().saturating_sub(confirm_height),
                width: area.width,
                height: confirm_height.min(area.height),
            };
            let warning_style = Style::default().fg(Color::Yellow);
            let text = vec![
                Line::from(Span::styled(
                    " This session contains sensitive data.",
                    warning_style,
                )),
                Line::from(Span::styled(
                    " Exported artifacts will include this data in plain text.",
                    warning_style,
                )),
                Line::from(Span::styled(
                    " Proceed? [y/N]",
                    Style::default().fg(Color::White),
                )),
            ];
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(" Export Warning ");
            let paragraph = Paragraph::new(text).block(block);
            frame.render_widget(paragraph, confirm_area);
        }
    }
}
