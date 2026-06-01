# TUI Refine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `inspectah-tui`, a terminal interface for the refine workflow — keyboard-driven, works over SSH at 80×24, full single-host triage parity with the web UI.

**Architecture:** Three-layer design — `app.rs` (event loop + state bridge) → `screen/` (layout composition) → `widget/` (stateless renderers). Events flow through a crossterm polling thread to an mpsc channel. Keys map to an `Action` enum via `keys.rs`. The app applies actions to `RefineSession` or `TuiState`, then passes cached views to the active screen for rendering. Widgets receive data slices and render to `ratatui::Frame` — they hold no state.

**Tech Stack:** Rust, ratatui (terminal UI), crossterm (backend), color-eyre (panic safety), tui-input (text input), signal-hook (SIGTSTP/SIGCONT), insta (snapshot testing)

**Spec:** `docs/specs/proposed/2026-05-30-tui-refine-design.md` — single source of truth. Read it fresh before implementation.

**Thorn Checkpoints:** After Tasks 6, 10, 14, 18, 22. Code review at each.

---

## File Structure

```
inspectah-tui/
  Cargo.toml
  src/
    lib.rs                # public API: run_tui(session, config)
    app.rs                # event loop, terminal lifecycle, state bridge
    action.rs             # Action enum
    event.rs              # crossterm event polling thread + mpsc channel
    types.rs              # FocusTarget, DetailMode, InputMode, TuiState, SectionEntry, SectionItem
    theme.rs              # ColorTier, Token, style resolution, NO_COLOR
    keys.rs               # event → Action mapping, modal dispatch
    sections.rs           # build sidebar entries from session data
    screen/
      mod.rs              # Screen enum (SingleHost, later Fleet)
      single_host.rs      # two-panel layout, focus management
    widget/
      mod.rs              # re-exports
      section_nav.rs      # sidebar with section counts
      triage_list.rs      # grouped item list with collapse/expand
      info_bar.rs         # compact 2-row detail for metadata items
      detail_view.rs      # fullscreen detail for content items
      containerfile.rs    # containerfile preview panel
      status_bar.rs       # stats + delta hint + key hints
      help_screen.rs      # ? keybinding reference
      search.rs           # / fuzzy filter overlay
      command_line.rs     # : command mode
      user_strategy.rs    # users section interactive view
    test_helpers.rs       # buffer_to_string, test session builders
```

**Modifications to existing crates:**

- `Cargo.toml` (workspace root) — add `inspectah-tui` to members
- `inspectah-cli/Cargo.toml` — add `inspectah-tui` dependency
- `inspectah-cli/src/main.rs` — add `Tui` variant to `Commands` enum
- `inspectah-cli/src/commands/mod.rs` — add `pub mod tui;`
- `inspectah-cli/src/commands/tui.rs` — new subcommand (modeled on `refine.rs`)

---

## Phase 1: Foundation

### Task 1: Crate Scaffolding

**Files:**
- Create: `inspectah-tui/Cargo.toml`
- Create: `inspectah-tui/src/lib.rs`
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Create crate directory**

Run: `mkdir -p inspectah-tui/src`

- [ ] **Step 2: Write Cargo.toml**

```toml
[package]
name = "inspectah-tui"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
inspectah-refine = { path = "../inspectah-refine" }
inspectah-core = { path = "../inspectah-core" }
ratatui = "0.29"
crossterm = "0.28"
color-eyre = "0.6"
tui-input = "0.11"
signal-hook = "0.3"
libc = "0.2"

[dev-dependencies]
insta = { workspace = true }
tempfile = { workspace = true }
inspectah-refine = { path = "../inspectah-refine" }
```

- [ ] **Step 3: Write lib.rs stub**

```rust
use inspectah_refine::session::RefineSession;

pub fn run_tui(_session: RefineSession) -> color_eyre::Result<()> {
    Ok(())
}
```

- [ ] **Step 4: Add to workspace**

In `Cargo.toml` (workspace root), add `"inspectah-tui"` to the `members` array:

```toml
members = [
    "inspectah-core",
    "inspectah-collect",
    "inspectah-pipeline",
    "inspectah-cli",
    "inspectah-web",
    "inspectah-refine",
    "inspectah-tui",
]
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check -p inspectah-tui`
Expected: compiles with no errors.

- [ ] **Step 6: Commit**

```
feat(tui): scaffold inspectah-tui crate

Add new workspace member with ratatui, crossterm, color-eyre
dependencies. Stub lib.rs with run_tui entry point.
```

---

### Task 2: Core Types

**Files:**
- Create: `inspectah-tui/src/types.rs`
- Create: `inspectah-tui/src/action.rs`
- Modify: `inspectah-tui/src/lib.rs`

- [ ] **Step 1: Write types.rs**

```rust
use std::collections::HashSet;
use std::time::Instant;

use inspectah_refine::projection::types::ReferenceProjection;
use inspectah_refine::types::{
    ItemId, RefinedConfig, RefinedPackage, SectionKind, TriageTag,
};

/// Which panel has keyboard focus.
/// Tab cycles: Sidebar → ItemList → DetailPane (when open) → Sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusTarget {
    Sidebar,
    ItemList,
    /// Active when a detail view (info bar or fullscreen) is open.
    DetailPane,
}

/// What the detail pane is showing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailMode {
    /// No detail pane visible.
    None,
    /// Compact 2-row info bar at bottom of item list.
    InfoBar,
    /// Fullscreen detail replacing the item list.
    Fullscreen,
}

/// Current input mode — determines how keys are interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Search,
    Command,
    /// Export confirmation prompt (y/N).
    Confirm,
    /// Help screen overlay.
    Help,
}

/// A flash message shown in the status bar for a limited duration.
#[derive(Debug, Clone)]
pub struct FlashMessage {
    pub text: String,
    pub expires: Instant,
}

impl FlashMessage {
    pub fn new(text: impl Into<String>, duration_secs: u64) -> Self {
        Self {
            text: text.into(),
            expires: Instant::now() + std::time::Duration::from_secs(duration_secs),
        }
    }

    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.expires
    }
}

/// All TUI-specific state (not session state).
#[derive(Debug)]
pub struct TuiState {
    pub focus: FocusTarget,
    pub active_section: usize,
    pub cursor: usize,
    pub detail_mode: DetailMode,
    pub input_mode: InputMode,
    pub search_query: String,
    pub command_input: String,
    pub show_containerfile: bool,
    pub flash: Option<FlashMessage>,
    /// Which triage groups are collapsed, keyed by (section_index, group_index).
    pub collapsed_groups: HashSet<(usize, usize)>,
    /// Saved cursor position per section, restored on section switch.
    pub section_cursors: Vec<usize>,
    /// Sidebar scroll offset (for overflow).
    pub sidebar_scroll: usize,
}

impl TuiState {
    pub fn new(section_count: usize) -> Self {
        let mut collapsed = HashSet::new();
        // Baseline group (index 2) is collapsed by default in every section.
        for i in 0..section_count {
            collapsed.insert((i, 2));
        }
        Self {
            focus: FocusTarget::Sidebar,
            active_section: 0,
            cursor: 0,
            detail_mode: DetailMode::None,
            input_mode: InputMode::Normal,
            search_query: String::new(),
            command_input: String::new(),
            show_containerfile: false,
            flash: None,
            collapsed_groups: collapsed,
            section_cursors: vec![0; section_count],
            sidebar_scroll: 0,
        }
    }
}

/// Identifies a sidebar section.
///
/// Section model (from spec rev3):
/// - 7 decision/composite above the separator: Packages (with embedded
///   repo bar), Configs, Services (composite: decision states/drop-ins +
///   reference divergent/advisories/warnings/omitted), Containers
///   (composite: decision quadlets/flatpaks + reference running/compose),
///   Sysctls, Tuned, Users
/// - 7 reference-only below the separator: VerChanges, KernelBoot,
///   Network, Storage, ScheduledTasks, NonRpmSoftware, SELinux
///
/// Repos are NOT a standalone sidebar entry — they are embedded in the
/// Packages section via a repo bar (matching the web UI's RepoBar.tsx).
/// Services and Containers are composite: they contain both decision items
/// (togglable via Space) and reference items (read-only, Space is no-op).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectionId {
    // Decision / composite sections (above sidebar separator)
    Packages,
    Configs,
    Services,    // composite: decision service_states/dropins + ref divergent/advisories/warnings/omitted
    Containers,  // composite: decision quadlets/flatpaks + ref running_containers/compose_files
    Sysctls,
    Tuned,
    Users,
    // Reference-only sections (below sidebar separator)
    VerChanges,
    KernelBoot,
    Network,
    Storage,
    ScheduledTasks,
    NonRpmSoftware,
    SELinux,
}

impl SectionId {
    /// True for sections that contain togglable (decision) items.
    /// Composite sections (Services, Containers) return true — they
    /// contain BOTH decision and reference items. The triage list
    /// renders decision items with Space toggle and reference items
    /// as read-only within the same section.
    pub fn is_decision(&self) -> bool {
        matches!(
            self,
            Self::Packages
                | Self::Configs
                | Self::Services
                | Self::Containers
                | Self::Sysctls
                | Self::Tuned
                | Self::Users
        )
    }

    /// True for composite sections that mix decision + reference items.
    pub fn is_composite(&self) -> bool {
        matches!(self, Self::Services | Self::Containers)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Packages => "Packages",
            Self::Configs => "Configs",
            Self::Services => "Services",
            Self::Containers => "Containers",
            Self::Sysctls => "Sysctls",
            Self::Tuned => "Tuned",
            Self::Users => "Users",
            Self::VerChanges => "Ver.Chg",
            Self::KernelBoot => "Kernel",
            Self::Network => "Network",
            Self::Storage => "Storage",
            Self::ScheduledTasks => "Sched.",
            Self::NonRpmSoftware => "Non-RPM",
            Self::SELinux => "SELinux",
        }
    }

    /// The `mark_viewed()` prefix for this section, if reviewed tracking
    /// is supported. Returns `None` for sections whose items cannot be
    /// independently marked as viewed (repos are embedded in packages,
    /// sysctls/tuned have no VALID_SECTIONS prefix).
    ///
    /// VALID_SECTIONS in RefineSession: packages, configs, services,
    /// containers, users_groups, network, storage, scheduled_tasks,
    /// non_rpm_software, kernel_boot, selinux
    pub fn viewed_prefix(&self) -> Option<&'static str> {
        match self {
            Self::Packages => Some("packages"),
            Self::Configs => Some("configs"),
            Self::Services => Some("services"),
            Self::Containers => Some("containers"),
            Self::Users => Some("users_groups"),
            Self::VerChanges => None, // not in VALID_SECTIONS
            Self::KernelBoot => Some("kernel_boot"),
            Self::Network => Some("network"),
            Self::Storage => Some("storage"),
            Self::ScheduledTasks => Some("scheduled_tasks"),
            Self::NonRpmSoftware => Some("non_rpm_software"),
            Self::SELinux => Some("selinux"),
            Self::Sysctls => None, // not in VALID_SECTIONS
            Self::Tuned => None,   // not in VALID_SECTIONS
        }
    }
}

/// A sidebar entry with section metadata.
#[derive(Debug, Clone)]
pub struct SectionEntry {
    pub id: SectionId,
    pub count: usize,
    pub included: usize,
    pub excluded: usize,
}
```

- [ ] **Step 2: Write action.rs**

```rust
use std::path::PathBuf;

/// All actions the TUI can perform, produced by key mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,
    // Navigation
    CursorUp,
    CursorDown,
    CursorTop,
    CursorBottom,
    FocusSidebar,
    FocusItems,
    CycleFocus,
    JumpToSection(usize),
    NextGroup,
    PrevGroup,
    // Item interaction
    ToggleItem,
    OpenDetail,
    CloseDetail,
    PromoteDetail,
    DetailNext,
    DetailPrev,
    // Session
    Undo,
    Redo,
    Refresh,
    // Overlays
    EnterSearch,
    EnterCommand,
    ShowHelp,
    ToggleContainerfile,
    // Input mode
    SubmitInput,
    CancelInput,
    InputChar(char),
    InputBackspace,
    InputDelete,
    InputLeft,
    InputRight,
    InputHome,
    InputEnd,
    // Tab completion in command mode
    TabComplete,
    // Export confirmation
    ConfirmYes,
    ConfirmNo,
    // No-op (unbound key)
    Noop,
}
```

- [ ] **Step 3: Update lib.rs with module declarations**

```rust
pub mod action;
pub mod types;

use inspectah_refine::session::RefineSession;

pub fn run_tui(_session: RefineSession) -> color_eyre::Result<()> {
    Ok(())
}
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check -p inspectah-tui`
Expected: compiles with no errors.

- [ ] **Step 5: Commit**

```
feat(tui): add core types and action enum

TuiState, FocusTarget, DetailMode, InputMode, SectionId, SectionEntry,
FlashMessage, and full Action enum.
```

---

### Task 3: Theme System

**Files:**
- Create: `inspectah-tui/src/theme.rs`
- Modify: `inspectah-tui/src/lib.rs`

- [ ] **Step 1: Write the test for color tier detection**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_color_env_forces_mono() {
        // NO_COLOR env is checked by detect_color_tier
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-tui -- theme`
Expected: FAIL — `ColorTier` not defined.

- [ ] **Step 3: Write theme.rs**

```rust
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
            Self::FocusBorder => Style::default().add_modifier(Modifier::BOLD),
            Self::FocusUnfocused => Style::default(),
            Self::FocusSelected => Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
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
            Self::FocusBorder => Style::default().fg(Color::Cyan),
            Self::FocusUnfocused => Style::default().add_modifier(Modifier::DIM),
            Self::FocusSelected => Style::default().add_modifier(Modifier::REVERSED),
            Self::SearchMatch => Style::default().add_modifier(Modifier::REVERSED),
            Self::Warning => Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            Self::Error => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        }
    }

    fn truecolor_style(self) -> Style {
        // TrueColor uses same palette as Ansi16 for v1.
        // Extend with RGB values in v2 if needed.
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
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-tui -- theme`
Expected: all 4 tests pass.

- [ ] **Step 5: Add module to lib.rs**

Add `pub mod theme;` to `lib.rs`.

- [ ] **Step 6: Commit**

```
feat(tui): add theme system with ColorTier and semantic tokens

15 semantic tokens (TriageInvestigate, StatusIncluded, etc.) with
Mono/Ansi16/TrueColor resolution. Respects NO_COLOR env.
```

---

### Task 4: Event System

**Files:**
- Create: `inspectah-tui/src/event.rs`
- Modify: `inspectah-tui/src/lib.rs`

- [ ] **Step 1: Write event.rs**

```rust
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event as CrosstermEvent, KeyEvent};

/// Events the TUI processes.
#[derive(Debug)]
pub enum Event {
    Key(KeyEvent),
    Resize(u16, u16),
    Tick,
}

/// Polls crossterm events in a background thread, sends to main loop.
pub struct EventReader {
    rx: mpsc::Receiver<Event>,
    _handle: thread::JoinHandle<()>,
}

impl EventReader {
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::channel();

        let handle = thread::spawn(move || loop {
            if event::poll(tick_rate).unwrap_or(false) {
                match event::read() {
                    Ok(CrosstermEvent::Key(key)) => {
                        if tx.send(Event::Key(key)).is_err() {
                            break;
                        }
                    }
                    Ok(CrosstermEvent::Resize(w, h)) => {
                        if tx.send(Event::Resize(w, h)).is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            } else {
                // Tick on timeout — drives flash message expiry.
                if tx.send(Event::Tick).is_err() {
                    break;
                }
            }
        });

        Self {
            rx,
            _handle: handle,
        }
    }

    /// Blocking receive. Returns None when sender is dropped.
    pub fn next(&self) -> Option<Event> {
        self.rx.recv().ok()
    }
}
```

- [ ] **Step 2: Add module to lib.rs**

Add `pub mod event;` to `lib.rs`.

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p inspectah-tui`
Expected: compiles with no errors.

- [ ] **Step 4: Commit**

```
feat(tui): add event polling thread

Crossterm event reader in a background thread with mpsc channel.
250ms tick rate for flash message expiry.
```

---

### Task 5: Key Mapping

**Files:**
- Create: `inspectah-tui/src/keys.rs`
- Modify: `inspectah-tui/src/lib.rs`

- [ ] **Step 1: Write the test for normal-mode key mapping**

```rust
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
        assert_eq!(map_key(key(KeyCode::Char('j')), InputMode::Normal), Action::CursorDown);
        assert_eq!(map_key(key(KeyCode::Char('k')), InputMode::Normal), Action::CursorUp);
        assert_eq!(map_key(key(KeyCode::Down), InputMode::Normal), Action::CursorDown);
        assert_eq!(map_key(key(KeyCode::Up), InputMode::Normal), Action::CursorUp);
        assert_eq!(map_key(key(KeyCode::Char('h')), InputMode::Normal), Action::FocusSidebar);
        assert_eq!(map_key(key(KeyCode::Char('l')), InputMode::Normal), Action::FocusItems);
        assert_eq!(map_key(key(KeyCode::Tab), InputMode::Normal), Action::CycleFocus);
        assert_eq!(map_key(key(KeyCode::Char('g')), InputMode::Normal), Action::CursorTop);
        assert_eq!(map_key(key(KeyCode::Char('G')), InputMode::Normal), Action::CursorBottom);
    }

    #[test]
    fn normal_mode_actions() {
        assert_eq!(map_key(key(KeyCode::Char(' ')), InputMode::Normal), Action::ToggleItem);
        assert_eq!(map_key(key(KeyCode::Enter), InputMode::Normal), Action::OpenDetail);
        assert_eq!(map_key(key(KeyCode::Esc), InputMode::Normal), Action::CloseDetail);
        assert_eq!(map_key(key(KeyCode::Char('u')), InputMode::Normal), Action::Undo);
        assert_eq!(map_key(ctrl(KeyCode::Char('r')), InputMode::Normal), Action::Redo);
        assert_eq!(map_key(key(KeyCode::Char('q')), InputMode::Normal), Action::Quit);
    }

    #[test]
    fn normal_mode_overlays() {
        assert_eq!(map_key(key(KeyCode::Char('/')), InputMode::Normal), Action::EnterSearch);
        assert_eq!(map_key(key(KeyCode::Char(':')), InputMode::Normal), Action::EnterCommand);
        assert_eq!(map_key(key(KeyCode::Char('?')), InputMode::Normal), Action::ShowHelp);
        assert_eq!(map_key(key(KeyCode::Char('c')), InputMode::Normal), Action::ToggleContainerfile);
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
        assert_eq!(map_key(key(KeyCode::Esc), InputMode::Search), Action::CancelInput);
        assert_eq!(map_key(key(KeyCode::Enter), InputMode::Search), Action::SubmitInput);
        assert_eq!(
            map_key(key(KeyCode::Char('a')), InputMode::Search),
            Action::InputChar('a')
        );
        assert_eq!(map_key(key(KeyCode::Backspace), InputMode::Search), Action::InputBackspace);
    }

    #[test]
    fn command_mode_tab_complete() {
        assert_eq!(map_key(key(KeyCode::Tab), InputMode::Command), Action::TabComplete);
    }

    #[test]
    fn confirm_mode() {
        assert_eq!(map_key(key(KeyCode::Char('y')), InputMode::Confirm), Action::ConfirmYes);
        assert_eq!(map_key(key(KeyCode::Char('n')), InputMode::Confirm), Action::ConfirmNo);
        assert_eq!(map_key(key(KeyCode::Esc), InputMode::Confirm), Action::ConfirmNo);
        assert_eq!(map_key(key(KeyCode::Enter), InputMode::Confirm), Action::ConfirmNo);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-tui -- keys`
Expected: FAIL — `map_key` not defined.

- [ ] **Step 3: Write keys.rs**

```rust
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
    // Ctrl combinations first
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
            Action::JumpToSection(ch.to_digit(10).unwrap() as usize - 1)
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
        // j/k navigate results in search mode
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
```

- [ ] **Step 4: Add module to lib.rs**

Add `pub mod keys;` to `lib.rs`.

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-tui -- keys`
Expected: all tests pass.

- [ ] **Step 6: Commit**

```
feat(tui): add key mapping with modal dispatch

Maps crossterm KeyEvent to Action enum based on InputMode.
Normal, Search, Command, Confirm, and Help modes.
```

---

### Task 6: App Shell + Terminal Lifecycle

**Files:**
- Create: `inspectah-tui/src/app.rs`
- Modify: `inspectah-tui/src/lib.rs`

- [ ] **Step 1: Write app.rs with terminal guard and main loop**

```rust
use std::io::{self, Stdout};
use std::time::Duration;

use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, execute};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use inspectah_refine::session::RefineSession;

use crate::action::Action;
use crate::event::{Event, EventReader};
use crate::keys::map_key;
use crate::theme::{detect_color_tier, ColorTier};
use crate::types::{DetailMode, FocusTarget, InputMode, TuiState};

/// RAII guard — restores terminal on drop (including panics).
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, cursor::Show);
    }
}

pub struct App {
    session: RefineSession,
    state: TuiState,
    tier: ColorTier,
    should_quit: bool,
}

impl App {
    pub fn new(session: RefineSession) -> Self {
        // Count sections to initialize TuiState
        let section_count = 14; // 7 decision/composite + 7 reference
        Self {
            session,
            state: TuiState::new(section_count),
            tier: detect_color_tier(),
            should_quit: false,
        }
    }

    pub fn run(mut self) -> color_eyre::Result<()> {
        // 1. Install color-eyre BEFORE alt screen
        color_eyre::install()?;

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
        //    crossterm only handles SIGWINCH — SIGTSTP/SIGCONT need signal-hook.
        let mut signals = signal_hook::iterator::Signals::new(&[
            signal_hook::consts::SIGTSTP,
            signal_hook::consts::SIGCONT,
        ])?;

        // Spawn a thread that forwards signals as Events
        let signal_tx = {
            // Share the event channel with the signal thread
            // (requires adding a signal variant to Event enum)
            // See event.rs — add Event::Signal(i32) variant
            std::thread::spawn(move || {
                for sig in signals.forever() {
                    match sig {
                        signal_hook::consts::SIGTSTP => {
                            // Leave alt screen, disable raw mode before suspending
                            let _ = terminal::disable_raw_mode();
                            let _ = execute!(io::stdout(), LeaveAlternateScreen, cursor::Show);
                            // Re-raise SIGTSTP with default handler to actually suspend
                            unsafe {
                                libc::signal(libc::SIGTSTP, libc::SIG_DFL);
                                libc::raise(libc::SIGTSTP);
                            }
                        }
                        signal_hook::consts::SIGCONT => {
                            // Restore terminal state after resume
                            let _ = terminal::enable_raw_mode();
                            let _ = execute!(io::stdout(), EnterAlternateScreen, cursor::Hide);
                            // Re-register our SIGTSTP handler
                            unsafe {
                                libc::signal(libc::SIGTSTP, libc::SIG_DFL);
                            }
                        }
                        _ => {}
                    }
                }
            })
        };

        // 7. Event reader thread (250ms tick)
        let events = EventReader::new(Duration::from_millis(250));

        // 8. Main event loop
        while !self.should_quit {
            // Render
            terminal.draw(|frame| {
                self.render(frame);
            })?;

            // Wait for event
            match events.next() {
                Some(Event::Key(key)) => {
                    let action = map_key(key, self.state.input_mode);
                    self.handle_action(action);
                }
                Some(Event::Resize(_, _)) => {
                    // Terminal handles resize automatically
                }
                Some(Event::Tick) => {
                    // Clear expired flash messages
                    if let Some(ref flash) = self.state.flash {
                        if flash.is_expired() {
                            self.state.flash = None;
                        }
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
            // Navigation and interaction wired in Tasks 11-12
            _ => {}
        }
    }

    fn render(&self, frame: &mut ratatui::Frame) {
        let area = frame.area();

        // Minimum terminal size check
        if area.width < 80 || area.height < 24 {
            let msg = ratatui::widgets::Paragraph::new(format!(
                "Terminal too small ({}×{}). Minimum: 80×24.",
                area.width, area.height
            ));
            frame.render_widget(msg, area);
            return;
        }

        // Placeholder — wired to SingleHost screen in Task 10
        let msg = ratatui::widgets::Paragraph::new("inspectah tui — press q to quit");
        frame.render_widget(msg, area);
    }
}
```

- [ ] **Step 2: Update lib.rs to wire run_tui to App::run**

```rust
pub mod action;
pub mod app;
pub mod event;
pub mod keys;
pub mod theme;
pub mod types;

use inspectah_refine::session::RefineSession;

pub fn run_tui(session: RefineSession) -> color_eyre::Result<()> {
    app::App::new(session).run()
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p inspectah-tui`
Expected: compiles with no errors.

- [ ] **Step 4: Smoke test manually**

Create a quick test binary or add a temporary `main.rs` that loads a fixture tarball and calls `run_tui()`. Verify: app launches in alt screen, shows placeholder text, `q` quits cleanly, terminal restores. Then remove the temporary binary.

This is a manual verification step — automated testing of terminal lifecycle requires integration test infrastructure added in later tasks.

- [ ] **Step 5: Commit**

```
feat(tui): add app shell with terminal lifecycle

RAII guard for terminal restore on exit/panic. color-eyre installs
before alt screen. SIGTSTP/SIGCONT handling for Ctrl+Z suspend/resume.
Main event loop with key → action dispatch. Minimum 80×24 terminal
size check.
```

---

> **THORN CHECKPOINT 1:** Terminal lifecycle safety. **Verification:**
> 1. Normal exit (`q`) — terminal restores, cursor visible, raw mode off
> 2. `Ctrl+C` — same as above
> 3. Forced panic (`unwrap()` on None in a test build) — color-eyre handler fires, terminal restores
> 4. `Ctrl+Z` suspend — terminal leaves alt screen, shell prompt visible, `fg` re-enters alt screen and redraws
> 5. Terminal resize during session — no crash, redraws correctly
> 6. Terminal < 80×24 — shows "too small" message, no crash
>
> Request Thorn review before proceeding.

---

## Phase 2: Rendering

### Task 7: Section Nav Widget

**Files:**
- Create: `inspectah-tui/src/widget/mod.rs`
- Create: `inspectah-tui/src/widget/section_nav.rs`
- Create: `inspectah-tui/src/test_helpers.rs`
- Modify: `inspectah-tui/src/lib.rs`

- [ ] **Step 1: Write test_helpers.rs**

```rust
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

/// Convert a ratatui buffer to a plain text string for snapshot testing.
/// Strips trailing whitespace per line.
pub fn buffer_to_string(buf: &Buffer) -> String {
    let mut result = String::new();
    for y in buf.area.y..buf.area.bottom() {
        let mut line = String::new();
        for x in buf.area.x..buf.area.right() {
            let cell = &buf[(x, y)];
            line.push_str(cell.symbol());
        }
        result.push_str(line.trim_end());
        result.push('\n');
    }
    result
}
```

- [ ] **Step 2: Write section_nav.rs with snapshot test**

```rust
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::theme::{ColorTier, Token};
use crate::types::SectionEntry;

pub struct SectionNavWidget<'a> {
    sections: &'a [SectionEntry],
    active: usize,
    focused: bool,
    tier: ColorTier,
    scroll_offset: usize,
}

impl<'a> SectionNavWidget<'a> {
    pub fn new(
        sections: &'a [SectionEntry],
        active: usize,
        focused: bool,
        tier: ColorTier,
        scroll_offset: usize,
    ) -> Self {
        Self {
            sections,
            active,
            focused,
            tier,
            scroll_offset,
        }
    }
}

impl Widget for SectionNavWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let visible_height = area.height as usize;
        let total = self.sections.len();
        let scroll = self.scroll_offset.min(total.saturating_sub(visible_height));

        let border_style = if self.focused {
            Token::FocusBorder.style(self.tier)
        } else {
            Token::FocusUnfocused.style(self.tier)
        };

        // Render header
        let header = if self.focused { "─ Sections ─" } else { " Sections " };
        let header_line = format!("{:width$}", header, width = area.width as usize);
        buf.set_string(area.x, area.y, &header_line, border_style);

        // Render section rows
        for (i, entry) in self.sections.iter().enumerate().skip(scroll) {
            let row_y = area.y + 1 + (i - scroll) as u16;
            if row_y >= area.bottom() {
                break;
            }

            let is_active = i == self.active;
            let style = if is_active {
                Token::FocusSelected.style(self.tier)
            } else if entry.id.is_decision() {
                Token::TextPrimary.style(self.tier)
            } else {
                Token::TextMuted.style(self.tier)
            };

            // Number prefix for first 9 sections
            let num = if i < 9 {
                format!("{} ", i + 1)
            } else {
                "  ".to_string()
            };

            // Label + count
            let count_str = format!("{}", entry.count);
            let label_width = area.width as usize - num.len() - count_str.len() - 1;
            let label = entry.id.label();
            let truncated = if label.len() > label_width {
                &label[..label_width]
            } else {
                label
            };

            let line = format!(
                "{}{:<width$}{}",
                num,
                truncated,
                count_str,
                width = label_width,
            );
            buf.set_string(area.x, row_y, &line, style);
        }

        // Scroll indicators
        if scroll > 0 {
            buf.set_string(area.right() - 1, area.y + 1, "▲", border_style);
        }
        if scroll + visible_height < total + 1 {
            buf.set_string(area.right() - 1, area.bottom() - 1, "▼", border_style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::buffer_to_string;
    use crate::types::SectionId;

    fn test_sections() -> Vec<SectionEntry> {
        vec![
            SectionEntry { id: SectionId::Packages, count: 142, included: 130, excluded: 12 },
            SectionEntry { id: SectionId::Configs, count: 47, included: 40, excluded: 7 },
            SectionEntry { id: SectionId::Services, count: 23, included: 20, excluded: 3 },
        ]
    }

    #[test]
    fn renders_sections_with_counts() {
        let sections = test_sections();
        let widget = SectionNavWidget::new(&sections, 0, true, ColorTier::Mono, 0);
        let area = Rect::new(0, 0, 18, 5);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        insta::assert_snapshot!(buffer_to_string(&buf));
    }

    #[test]
    fn highlights_active_section() {
        let sections = test_sections();
        let widget = SectionNavWidget::new(&sections, 1, true, ColorTier::Mono, 0);
        let area = Rect::new(0, 0, 18, 5);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        // Active section (Configs) should have reversed style
        let configs_y = area.y + 2; // header + Packages + Configs
        let cell = &buf[(area.x, configs_y)];
        assert!(
            cell.style().add_modifier.contains(ratatui::style::Modifier::REVERSED)
                || cell.style().add_modifier.contains(ratatui::style::Modifier::BOLD),
            "Active section should be highlighted"
        );
    }
}
```

- [ ] **Step 3: Write widget/mod.rs**

```rust
pub mod section_nav;
```

- [ ] **Step 4: Update lib.rs**

Add `pub mod test_helpers;` and `pub mod widget;` to `lib.rs`.

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-tui -- section_nav`
Expected: tests pass, insta snapshots created.

- [ ] **Step 6: Review snapshots**

Run: `cargo insta review -p inspectah-tui`
Accept the snapshots after verifying the rendered output matches expected sidebar layout.

- [ ] **Step 7: Commit**

```
feat(tui): add section nav sidebar widget

Renders numbered section list with counts, active highlight,
scroll indicators. Snapshot tested via insta.
```

---

### Task 8: Triage List Widget

**Files:**
- Create: `inspectah-tui/src/widget/triage_list.rs`
- Create: `inspectah-tui/src/sections.rs`
- Modify: `inspectah-tui/src/widget/mod.rs`
- Modify: `inspectah-tui/src/lib.rs`

- [ ] **Step 1: Write sections.rs — build sidebar entries from session data**

This module reads session data and constructs `SectionEntry` and item lists. It bridges the `RefineSession` API to the TUI's type model.

```rust
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::SectionKind;

use crate::types::{SectionEntry, SectionId};

/// Ordered list of all sidebar sections: 7 decision/composite + 7 reference.
/// Matches the spec's sidebar layout (ASCII art in spec § Layout & Panels).
pub const SECTION_ORDER: &[SectionId] = &[
    // Decision / composite (above separator, numbered 1-7)
    SectionId::Packages,    // includes embedded repo bar from decisions().repo_groups
    SectionId::Configs,
    SectionId::Services,    // composite: decision service_states/dropins + ref context
    SectionId::Containers,  // composite: decision quadlets/flatpaks + ref running/compose
    SectionId::Sysctls,
    SectionId::Tuned,
    SectionId::Users,
    // Reference-only (below separator, numbered 8-9 then unnumbered)
    SectionId::VerChanges,
    SectionId::KernelBoot,
    SectionId::Network,
    SectionId::Storage,
    SectionId::ScheduledTasks,
    SectionId::NonRpmSoftware,
    SectionId::SELinux,
];

/// Build sidebar entries from session data.
pub fn build_section_entries(session: &RefineSession) -> Vec<SectionEntry> {
    let view = session.view();
    let decisions = session.decisions();
    let reference = session.reference();

    SECTION_ORDER
        .iter()
        .map(|id| {
            // IMPORTANT: RefineStats only produces SectionStats for Package,
            // Config, and Repo. All other sections must count items directly
            // from decisions() and reference(). Calling view().stats.section()
            // for SectionKind::Service, Sysctl, etc. returns zeros.
            let (count, included, excluded) = match id {
                SectionId::Packages => {
                    let s = view.stats.section(SectionKind::Package);
                    (s.total, s.included, s.excluded)
                }
                SectionId::Configs => {
                    let s = view.stats.section(SectionKind::Config);
                    (s.total, s.included, s.excluded)
                }
                SectionId::Services => {
                    // Composite: decision items counted directly + all 8
                    // reference sub-collections from reference().services.
                    let decision_count = decisions.service_states.len()
                        + decisions.service_dropins.len();
                    let decision_included = decisions.service_states.iter()
                        .filter(|s| s.entry.include).count()
                        + decisions.service_dropins.iter()
                        .filter(|d| d.entry.include).count();
                    let ref_svc = &reference.services;
                    let ref_count = ref_svc.divergent.len()
                        + ref_svc.preset_matched_with_dropins.len()
                        + ref_svc.preset_unknown_enabled.len()
                        + ref_svc.preset_unknown_disabled.len()
                        + ref_svc.standalone_dropins.len()
                        + ref_svc.omitted.len()
                        + ref_svc.advisories.len()
                        + ref_svc.warnings.len();
                    (decision_count + ref_count, decision_included, decision_count - decision_included)
                }
                SectionId::Containers => {
                    // Composite: decision quadlets/flatpaks + reference
                    // running_containers/compose_files.
                    let q_total = decisions.quadlets.len();
                    let q_incl = decisions.quadlets.iter().filter(|q| q.entry.include).count();
                    let f_total = decisions.flatpaks.len();
                    let f_incl = decisions.flatpaks.iter().filter(|f| f.entry.include).count();
                    let ref_count = reference.containers.running_containers.len()
                        + reference.containers.compose_files.len();
                    (q_total + f_total + ref_count, q_incl + f_incl, q_total + f_total - q_incl - f_incl)
                }
                SectionId::Sysctls => {
                    let total = decisions.sysctls.len();
                    let incl = decisions.sysctls.iter().filter(|s| s.entry.include).count();
                    (total, incl, total - incl)
                }
                SectionId::Tuned => {
                    let total = decisions.tuned.len();
                    let incl = decisions.tuned.iter().filter(|t| t.include).count();
                    (total, incl, total - incl)
                }
                SectionId::Users => {
                    let total = decisions.users_groups.len();
                    (total, 0, 0) // users don't have include/exclude — they have strategies
                }
                // Reference-only sections — count only, no included/excluded
                SectionId::VerChanges => {
                    let vc = &reference.version_changes;
                    (vc.downgrades.len() + vc.upgrades.len(), 0, 0)
                }
                SectionId::KernelBoot => {
                    let kb = &reference.kernel_boot;
                    (kb.total_items(), 0, 0)
                }
                SectionId::Network => {
                    let net = &reference.network;
                    (net.total_items(), 0, 0)
                }
                SectionId::Storage => {
                    let st = &reference.storage;
                    (st.total_items(), 0, 0)
                }
                SectionId::ScheduledTasks => (reference.scheduled_tasks.len(), 0, 0),
                SectionId::NonRpmSoftware => (reference.non_rpm_software.len(), 0, 0),
                SectionId::SELinux => (reference.selinux.len(), 0, 0),
            };

            SectionEntry {
                id: *id,
                count,
                included,
                excluded,
            }
        })
        .collect()
}
```

**Notes:**
- `RefKernelBoot`, `RefNetwork`, and `RefStorage` may not have a `total_items()` method. If not, count their sub-fields directly. Check `inspectah-refine/src/projection/types.rs`.
- **`RefineStats` only has 3 `SectionStats` entries**: Package, Config, Repo. All other section counts MUST be derived by counting items directly from `decisions()` and `reference()`. Calling `view().stats.section(SectionKind::Service)` (or Sysctl, Tuned, Quadlet, Flatpak, User) returns zeros — those `SectionKind` variants exist in the enum but `recompute_view()` never populates stats for them.
- Services count: `decisions().service_states.len() + decisions().service_dropins.len()` (decision) + all 8 `reference().services` sub-collections (reference).
- Containers count: `decisions().quadlets.len() + decisions().flatpaks.len()` (decision) + `reference().containers.running_containers.len() + reference().containers.compose_files.len()` (reference).
- Sysctls/Tuned/Users: count directly from `decisions().sysctls`, `decisions().tuned`, `decisions().users_groups`.

- [ ] **Step 2: Write triage_list.rs**

```rust
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use inspectah_refine::types::TriageBucket;

use crate::theme::{ColorTier, Token};
use crate::types::SectionId;

/// A displayable item in the triage list.
pub struct ListItem {
    pub name: String,
    pub detail: String,
    pub triage: TriageBucket,
    pub included: Option<bool>,
    pub is_group_header: bool,
    pub group_index: usize,
    pub is_collapsed: bool,
    pub group_count: usize,
}

pub struct TriageListWidget<'a> {
    items: &'a [ListItem],
    cursor: usize,
    focused: bool,
    tier: ColorTier,
    section_id: SectionId,
    scroll_offset: usize,
}

impl<'a> TriageListWidget<'a> {
    pub fn new(
        items: &'a [ListItem],
        cursor: usize,
        focused: bool,
        tier: ColorTier,
        section_id: SectionId,
    ) -> Self {
        Self {
            items,
            cursor,
            focused,
            tier,
            section_id,
            scroll_offset: 0,
        }
    }

    pub fn with_scroll(mut self, offset: usize) -> Self {
        self.scroll_offset = offset;
        self
    }
}

impl Widget for TriageListWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let is_pure_reference = !self.section_id.is_decision();
        let is_composite = self.section_id.is_composite();

        // Section header
        let header = if is_pure_reference {
            format!(" {} — Reference (read-only context)", self.section_id.label())
        } else if is_composite {
            format!(" {} (decision + reference)", self.section_id.label())
        } else {
            format!(" {}", self.section_id.label())
        };
        buf.set_string(
            area.x,
            area.y,
            &truncate(&header, area.width as usize),
            Token::TextMuted.style(self.tier),
        );

        let list_area_y = area.y + 1;
        let list_height = area.height.saturating_sub(1) as usize;

        // Ensure cursor is visible
        let scroll = compute_scroll(self.cursor, self.scroll_offset, list_height);

        for (i, item) in self.items.iter().enumerate().skip(scroll) {
            let row_y = list_area_y + (i - scroll) as u16;
            if row_y >= area.bottom() {
                break;
            }

            let is_cursor = i == self.cursor;

            if item.is_group_header {
                render_group_header(buf, area.x, row_y, area.width, item, is_cursor, self.tier);
            } else {
                render_item_row(buf, area.x, row_y, area.width, item, is_cursor, self.focused, is_reference, self.tier);
            }
        }
    }
}

fn render_group_header(buf: &mut Buffer, x: u16, y: u16, width: u16, item: &ListItem, is_cursor: bool, tier: ColorTier) {
    let arrow = if item.is_collapsed { "▸" } else { "▾" };
    let label = match item.triage {
        TriageBucket::Investigate => "Investigate",
        TriageBucket::Site => "Site",
        TriageBucket::Baseline => "already in base image",
    };
    let header = format!(" {} {} ({})", arrow, label, item.group_count);

    let style = if is_cursor {
        Token::FocusSelected.style(tier)
    } else {
        match item.triage {
            TriageBucket::Investigate => Token::TriageInvestigate.style(tier),
            TriageBucket::Site => Token::TriageSite.style(tier),
            TriageBucket::Baseline => Token::TriageBaseline.style(tier),
        }
    };

    buf.set_string(x, y, &truncate(&header, width as usize), style);
}

fn render_item_row(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    item: &ListItem,
    is_cursor: bool,
    focused: bool,
    is_reference: bool,
    tier: ColorTier,
) {
    let w = width as usize;

    // Include/exclude indicator.
    // For composite sections, each item knows its own mutability via
    // included: Some(_) = decision item, None = reference item.
    let is_ref_item = is_reference || item.included.is_none();
    let indicator = if is_ref_item {
        "  "
    } else {
        match item.included {
            Some(true) => "● ",
            Some(false) => "○ ",
            None => "  ",
        }
    };

    let indicator_style = match item.included {
        Some(true) => Token::StatusIncluded.style(tier),
        _ => Token::StatusExcluded.style(tier),
    };

    // Name and detail
    let detail_width = w.saturating_sub(indicator.len() + item.name.len() + 2);
    let detail = truncate(&item.detail, detail_width);

    let row_style = if is_cursor && focused {
        Token::FocusSelected.style(tier)
    } else {
        Token::TextPrimary.style(tier)
    };

    buf.set_string(x, y, indicator, indicator_style);
    let name_x = x + indicator.len() as u16;
    buf.set_string(name_x, y, &truncate(&item.name, w - indicator.len()), row_style);

    if !detail.is_empty() {
        let detail_x = name_x + item.name.len() as u16 + 1;
        if (detail_x as usize) < x as usize + w {
            buf.set_string(detail_x, y, &detail, Token::TextMuted.style(tier));
        }
    }
}

fn compute_scroll(cursor: usize, current_scroll: usize, height: usize) -> usize {
    if height == 0 {
        return 0;
    }
    if cursor < current_scroll {
        cursor
    } else if cursor >= current_scroll + height {
        cursor - height + 1
    } else {
        current_scroll
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max > 1 {
        format!("{}…", &s[..max - 1])
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::buffer_to_string;

    fn test_items() -> Vec<ListItem> {
        vec![
            ListItem {
                name: String::new(),
                detail: String::new(),
                triage: TriageBucket::Investigate,
                included: None,
                is_group_header: true,
                group_index: 0,
                is_collapsed: false,
                group_count: 2,
            },
            ListItem {
                name: "mystery-pkg".into(),
                detail: "1.0.0  (none)".into(),
                triage: TriageBucket::Investigate,
                included: Some(true),
                is_group_header: false,
                group_index: 0,
                is_collapsed: false,
                group_count: 0,
            },
            ListItem {
                name: "unknown-lib".into(),
                detail: "2.3.1".into(),
                triage: TriageBucket::Investigate,
                included: Some(true),
                is_group_header: false,
                group_index: 0,
                is_collapsed: false,
                group_count: 0,
            },
            ListItem {
                name: String::new(),
                detail: String::new(),
                triage: TriageBucket::Site,
                included: None,
                is_group_header: true,
                group_index: 1,
                is_collapsed: false,
                group_count: 1,
            },
            ListItem {
                name: "httpd".into(),
                detail: "2.4.62  rhel-9-appstream".into(),
                triage: TriageBucket::Site,
                included: Some(true),
                is_group_header: false,
                group_index: 1,
                is_collapsed: false,
                group_count: 0,
            },
        ]
    }

    #[test]
    fn renders_grouped_items() {
        let items = test_items();
        let widget = TriageListWidget::new(&items, 0, true, ColorTier::Mono, SectionId::Packages);
        let area = Rect::new(0, 0, 50, 8);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        insta::assert_snapshot!(buffer_to_string(&buf));
    }

    #[test]
    fn reference_section_shows_no_indicators() {
        let items = vec![ListItem {
            name: "eth0".into(),
            detail: "connected".into(),
            triage: TriageBucket::Site,
            included: None,
            is_group_header: false,
            group_index: 0,
            is_collapsed: false,
            group_count: 0,
        }];
        let widget = TriageListWidget::new(&items, 0, true, ColorTier::Mono, SectionId::Network);
        let area = Rect::new(0, 0, 40, 3);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        let output = buffer_to_string(&buf);
        assert!(output.contains("Reference"), "Should show Reference label");
    }
}
```

- [ ] **Step 3: Update widget/mod.rs**

```rust
pub mod section_nav;
pub mod triage_list;
```

- [ ] **Step 4: Add sections module to lib.rs**

Add `pub mod sections;` to `lib.rs`.

- [ ] **Step 5: Run tests and review snapshots**

Run: `cargo test -p inspectah-tui -- triage_list && cargo insta review -p inspectah-tui`
Expected: tests pass, snapshots show grouped items with indicators.

- [ ] **Step 6: Commit**

```
feat(tui): add triage list widget and section builder

Grouped item list with Investigate/Site/Baseline headers,
include/exclude indicators, column truncation. Sections
module bridges RefineSession API to TUI types.
```

---

### Task 9: Status Bar Widget

**Files:**
- Create: `inspectah-tui/src/widget/status_bar.rs`
- Modify: `inspectah-tui/src/widget/mod.rs`

- [ ] **Step 1: Write status_bar.rs**

```rust
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::theme::{ColorTier, Token};
use crate::types::FlashMessage;

pub struct StatusBarWidget<'a> {
    included: usize,
    excluded: usize,
    review_count: usize,
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
            review_count: 0,
            containerfile_delta: 0,
            reviewed: 0,
            total_reviewable: 0,
            flash: None,
            tier,
            is_decision_section: true,
        }
    }

    pub fn stats(mut self, included: usize, excluded: usize, review: usize) -> Self {
        self.included = included;
        self.excluded = excluded;
        self.review_count = review;
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
        if let Some(flash) = self.flash {
            if !flash.is_expired() {
                buf.set_string(area.x + 1, area.y, &flash.text, Token::Warning.style(self.tier));
                return;
            }
        }

        let mut parts: Vec<String> = Vec::new();

        if self.is_decision_section {
            parts.push(format!("{} incl", self.included));
            parts.push(format!("{} excl", self.excluded));
            if self.review_count > 0 {
                parts.push(format!("{} review", self.review_count));
            }
        }

        if self.containerfile_delta > 0 {
            parts.push(format!("Containerfile: {}Δ", self.containerfile_delta));
        }

        if self.total_reviewable > 0 {
            parts.push(format!("{}/{} reviewed", self.reviewed, self.total_reviewable));
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
            .stats(142, 176, 12)
            .containerfile_delta(3);
        let area = Rect::new(0, 0, 80, 1);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        let output = buffer_to_string(&buf);
        assert!(output.contains("142 incl"));
        assert!(output.contains("176 excl"));
        assert!(output.contains("12 review"));
        assert!(output.contains("Containerfile: 3Δ"));
    }

    #[test]
    fn flash_overrides_stats() {
        let flash = FlashMessage::new("Resumed session (5 ops)", 3);
        let widget = StatusBarWidget::new(ColorTier::Mono)
            .stats(100, 50, 0)
            .flash(Some(&flash));
        let area = Rect::new(0, 0, 60, 1);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        let output = buffer_to_string(&buf);
        assert!(output.contains("Resumed session (5 ops)"));
        assert!(!output.contains("100 incl"));
    }
}
```

- [ ] **Step 2: Update widget/mod.rs**

Add `pub mod status_bar;` to `widget/mod.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-tui -- status_bar`
Expected: all tests pass.

- [ ] **Step 4: Commit**

```
feat(tui): add status bar widget

Shows included/excluded/review counts, containerfile delta,
reviewed progress. Flash messages override stats temporarily.
Key hints on the right.
```

---

### Task 10: SingleHost Screen + Assembly

**Files:**
- Create: `inspectah-tui/src/screen/mod.rs`
- Create: `inspectah-tui/src/screen/single_host.rs`
- Modify: `inspectah-tui/src/app.rs`
- Modify: `inspectah-tui/src/lib.rs`

- [ ] **Step 1: Write screen/mod.rs**

```rust
pub mod single_host;

use ratatui::Frame;

use inspectah_refine::session::RefineSession;

use crate::theme::ColorTier;
use crate::types::TuiState;

pub enum Screen {
    SingleHost(single_host::SingleHostScreen),
}

impl Screen {
    pub fn render(&self, frame: &mut Frame, session: &RefineSession, state: &TuiState, tier: ColorTier) {
        match self {
            Screen::SingleHost(screen) => screen.render(frame, session, state, tier),
        }
    }
}
```

- [ ] **Step 2: Write screen/single_host.rs**

```rust
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

use inspectah_refine::session::RefineSession;

use crate::sections::build_section_entries;
use crate::theme::ColorTier;
use crate::types::{FocusTarget, TuiState};
use crate::widget::section_nav::SectionNavWidget;
use crate::widget::status_bar::StatusBarWidget;
use crate::widget::triage_list::{self, ListItem, TriageListWidget};

const SIDEBAR_WIDTH: u16 = 18;

pub struct SingleHostScreen;

impl SingleHostScreen {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, frame: &mut Frame, session: &RefineSession, state: &TuiState, tier: ColorTier) {
        let area = frame.area();

        // Top-level vertical split: main area + status bar (1 row)
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(area);

        let main_area = vertical[0];
        let status_area = vertical[1];

        // Main area: sidebar (fixed) + item list (remaining)
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(SIDEBAR_WIDTH),
                Constraint::Min(40),
            ])
            .split(main_area);

        let sidebar_area = horizontal[0];
        let list_area = horizontal[1];

        // Build section data
        let sections = build_section_entries(session);

        // Render sidebar
        let sidebar = SectionNavWidget::new(
            &sections,
            state.active_section,
            state.focus == FocusTarget::Sidebar,
            tier,
            state.sidebar_scroll,
        );
        frame.render_widget(sidebar, sidebar_area);

        // Build list items for active section
        let items = build_list_items(session, state, &sections);

        // Render triage list
        let active_section_id = sections
            .get(state.active_section)
            .map(|s| s.id)
            .unwrap_or(crate::types::SectionId::Packages);
        let list = TriageListWidget::new(
            &items,
            state.cursor,
            state.focus == FocusTarget::ItemList,
            tier,
            active_section_id,
        );
        frame.render_widget(list, list_area);

        // Render status bar
        let active_entry = sections.get(state.active_section);
        let view = session.view();
        let status = StatusBarWidget::new(tier)
            .stats(
                active_entry.map(|e| e.included).unwrap_or(0),
                active_entry.map(|e| e.excluded).unwrap_or(0),
                view.stats.needs_review_count,
            )
            .decision_section(active_entry.map(|e| e.id.is_decision()).unwrap_or(true))
            .flash(state.flash.as_ref());
        frame.render_widget(status, status_area);
    }
}

/// Build displayable list items for the active section from session data.
/// Groups items by triage bucket (decision sections) or flat list (reference).
fn build_list_items(
    session: &RefineSession,
    state: &TuiState,
    sections: &[crate::types::SectionEntry],
) -> Vec<ListItem> {
    use inspectah_refine::types::TriageBucket;

    let active = match sections.get(state.active_section) {
        Some(s) => s,
        None => return Vec::new(),
    };

    // Reference sections: flat list of GenericRefItems or typed items.
    // Decision sections: grouped by triage bucket.
    //
    // This is a large match on SectionId. Each arm extracts the right
    // data from session.view(), session.decisions(), or session.reference(),
    // converts to ListItem, and groups by triage bucket.
    //
    // Full implementation requires mapping each section's domain types
    // to ListItem. The pattern is identical for each — extract name,
    // detail string, triage tag, and include state.

    // Placeholder — Task 11 fills in the full match once navigation is wired
    let view = session.view();
    let decisions = session.decisions();

    match active.id {
        crate::types::SectionId::Packages => {
            build_grouped_items(
                view.packages.iter().map(|p| {
                    let name = format!("{}.{}", p.entry.name, p.entry.arch);
                    let detail = format!(
                        "{}  {}",
                        p.entry.version,
                        p.entry.source_repo.as_deref().unwrap_or("(none)"),
                    );
                    (name, detail, p.triage.bucket, Some(p.entry.include))
                }),
                state,
            )
        }
        crate::types::SectionId::Configs => {
            build_grouped_items(
                view.config_files.iter().map(|c| {
                    let name = c.entry.path.to_string_lossy().to_string();
                    let detail = c.entry.change_type.clone().unwrap_or_default();
                    (name, detail, c.triage.bucket, Some(c.entry.include))
                }),
                state,
            )
        }
        // Services (composite): decision items grouped by triage, then reference
        // context (divergent, advisories, warnings, omitted) as a flat read-only
        // section below. Decision items from decisions().service_states and
        // decisions().service_dropins. Reference items from reference().services.
        //
        // Containers (composite): decision quadlets/flatpaks grouped by triage,
        // then reference running_containers/compose_files as read-only below.
        // Decision items from decisions().quadlets and decisions().flatpaks.
        // Reference items from reference().containers.
        //
        // Sysctls, Tuned, Users: pure decision sections from decisions().
        //
        // VerChanges, KernelBoot, Network, Storage, ScheduledTasks,
        // NonRpmSoftware, SELinux: pure reference, flat list of GenericRefItem
        // or typed items from reference(). No triage grouping, no toggling.
        //
        // The implementer must add an arm for every SectionId variant.
        // Each arm follows the same pattern shown above for Packages/Configs.
        _ => Vec::new(),
    }
}

/// Build a grouped list from an iterator of (name, detail, bucket, include) tuples.
fn build_grouped_items(
    items: impl Iterator<Item = (String, String, inspectah_refine::types::TriageBucket, Option<bool>)>,
    state: &TuiState,
) -> Vec<ListItem> {
    use inspectah_refine::types::TriageBucket;
    use std::collections::BTreeMap;

    let buckets = [TriageBucket::Investigate, TriageBucket::Site, TriageBucket::Baseline];
    let mut grouped: BTreeMap<usize, Vec<(String, String, Option<bool>)>> = BTreeMap::new();

    for (name, detail, bucket, include) in items {
        let idx = match bucket {
            TriageBucket::Investigate => 0,
            TriageBucket::Site => 1,
            TriageBucket::Baseline => 2,
        };
        grouped.entry(idx).or_default().push((name, detail, include));
    }

    let mut result = Vec::new();
    for (group_idx, bucket) in buckets.iter().enumerate() {
        let items_in_group = grouped.get(&group_idx).map(|v| v.len()).unwrap_or(0);
        if items_in_group == 0 {
            continue;
        }

        let is_collapsed = state
            .collapsed_groups
            .contains(&(state.active_section, group_idx));

        // Group header
        result.push(ListItem {
            name: String::new(),
            detail: String::new(),
            triage: *bucket,
            included: None,
            is_group_header: true,
            group_index: group_idx,
            is_collapsed,
            group_count: items_in_group,
        });

        if !is_collapsed {
            if let Some(group_items) = grouped.get(&group_idx) {
                for (name, detail, include) in group_items {
                    result.push(ListItem {
                        name: name.clone(),
                        detail: detail.clone(),
                        triage: *bucket,
                        included: *include,
                        is_group_header: false,
                        group_index: group_idx,
                        is_collapsed: false,
                        group_count: 0,
                    });
                }
            }
        }
    }

    result
}
```

- [ ] **Step 3: Wire screen into app.rs render method**

Replace the placeholder `render` method in `app.rs`:

```rust
fn render(&self, frame: &mut ratatui::Frame) {
    let area = frame.area();

    if area.width < 80 || area.height < 24 {
        let msg = ratatui::widgets::Paragraph::new(format!(
            "Terminal too small ({}×{}). Minimum: 80×24.",
            area.width, area.height
        ));
        frame.render_widget(msg, area);
        return;
    }

    self.screen.render(frame, &self.session, &self.state, self.tier);
}
```

Add a `screen` field to `App`:

```rust
pub struct App {
    session: RefineSession,
    state: TuiState,
    tier: ColorTier,
    should_quit: bool,
    screen: crate::screen::Screen,
    /// Tarball path for :fresh reload and export default path.
    tarball_path: Option<std::path::PathBuf>,
    pending_export_path: Option<std::path::PathBuf>,
}
```

Initialize it in `App::new`:

```rust
screen: crate::screen::Screen::SingleHost(
    crate::screen::single_host::SingleHostScreen::new(),
),
```

- [ ] **Step 4: Update lib.rs**

Add `pub mod screen;` and `pub mod sections;` (if not already added).

- [ ] **Step 5: Verify compilation**

Run: `cargo check -p inspectah-tui`
Expected: compiles. Some sections may show empty lists — that's expected until all section arms are filled in Task 11.

- [ ] **Step 6: Commit**

```
feat(tui): add SingleHost screen with two-panel layout

Sidebar (18 chars) + item list + status bar. Renders real session
data for Packages and Configs sections. Other sections return
empty lists until wired.
```

---

> **THORN CHECKPOINT 2:** Data rendering correctness. **Verification:**
> 1. Sidebar shows 14 sections (7+7 with separator) — counts match web UI for the same tarball
> 2. Packages section shows repo bar with repo names and package counts
> 3. Services section shows both decision items (service_states, dropins) and reference context (divergent, advisories, etc.)
> 4. Containers section shows both decision items (quadlets, flatpaks) and reference items (running, compose)
> 5. Reference-only sections (VerChanges, KernelBoot, etc.) show flat lists with "Reference" header
> 6. Empty sections show count 0 (not hidden — sidebar always shows all 14 entries)
>
> Request Thorn review before proceeding.

---

## Phase 3: Interaction

### Task 11: Navigation + Focus Management

**Files:**
- Modify: `inspectah-tui/src/app.rs`

- [ ] **Step 1: Implement navigation actions in handle_action**

```rust
fn handle_action(&mut self, action: Action) {
    match action {
        Action::Quit => self.should_quit = true,

        // Navigation
        Action::CursorDown => {
            let max = self.visible_item_count().saturating_sub(1);
            if self.state.cursor < max {
                self.state.cursor += 1;
            }
        }
        Action::CursorUp => {
            if self.state.cursor > 0 {
                self.state.cursor -= 1;
            }
        }
        Action::CursorTop => {
            self.state.cursor = 0;
        }
        Action::CursorBottom => {
            self.state.cursor = self.visible_item_count().saturating_sub(1);
        }

        // Focus
        Action::FocusSidebar => {
            self.state.focus = FocusTarget::Sidebar;
        }
        Action::FocusItems => {
            self.state.focus = FocusTarget::ItemList;
        }
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

        // Section jump
        Action::JumpToSection(idx) => {
            let sections = sections::build_section_entries(&self.session);
            if idx < sections.len() {
                // Save current cursor
                self.state.section_cursors[self.state.active_section] = self.state.cursor;
                self.state.active_section = idx;
                // Restore saved cursor for new section
                self.state.cursor = self.state.section_cursors[idx];
                self.state.focus = FocusTarget::ItemList;
            }
        }

        // Group navigation
        Action::NextGroup => {
            let items = self.current_items();
            for i in (self.state.cursor + 1)..items.len() {
                if items[i].is_group_header {
                    self.state.cursor = i;
                    break;
                }
            }
        }
        Action::PrevGroup => {
            let items = self.current_items();
            for i in (0..self.state.cursor).rev() {
                if items[i].is_group_header {
                    self.state.cursor = i;
                    break;
                }
            }
        }

        _ => {}
    }
}

fn visible_item_count(&self) -> usize {
    self.current_items().len()
}

fn current_items(&self) -> Vec<crate::widget::triage_list::ListItem> {
    let sections = sections::build_section_entries(&self.session);
    crate::screen::single_host::build_list_items(&self.session, &self.state, &sections)
}
```

**Note:** `build_list_items` needs to be made `pub` in `single_host.rs`. Also, cursor movement in sidebar focus mode should navigate sections (j/k = section up/down), not item list. Add focus-aware dispatch:

```rust
// In handle_action, wrap navigation in focus check:
Action::CursorDown => {
    match self.state.focus {
        FocusTarget::Sidebar => {
            let sections = sections::build_section_entries(&self.session);
            if self.state.active_section < sections.len() - 1 {
                self.state.section_cursors[self.state.active_section] = self.state.cursor;
                self.state.active_section += 1;
                self.state.cursor = self.state.section_cursors[self.state.active_section];
            }
        }
        FocusTarget::ItemList => {
            let max = self.visible_item_count().saturating_sub(1);
            if self.state.cursor < max {
                self.state.cursor += 1;
            }
        }
    }
}
// Same pattern for CursorUp
```

- [ ] **Step 2: Add group collapse/expand on Enter when cursor is on group header**

When the cursor is on a group header and Enter is pressed, toggle collapse:

```rust
Action::OpenDetail => {
    let items = self.current_items();
    if let Some(item) = items.get(self.state.cursor) {
        if item.is_group_header {
            let key = (self.state.active_section, item.group_index);
            if self.state.collapsed_groups.contains(&key) {
                self.state.collapsed_groups.remove(&key);
            } else {
                self.state.collapsed_groups.insert(key);
            }
        } else {
            // Open detail view — Task 13
            self.state.detail_mode = DetailMode::InfoBar;
        }
    }
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p inspectah-tui`
Expected: compiles with no errors.

- [ ] **Step 4: Manual smoke test**

Wire up a test tarball, launch the TUI, verify:
- j/k moves cursor in sidebar (switches sections) and item list
- h/l switches focus between sidebar and items
- Tab cycles focus
- 1-9 jumps to sections
- g/G goes to top/bottom
- {/} jumps between group headers
- Enter on a group header toggles collapse

- [ ] **Step 5: Commit**

```
feat(tui): wire navigation and focus management

j/k cursor movement (focus-aware), h/l focus switch, Tab cycle,
1-9 section jump, g/G top/bottom, {/} group jump, Enter
toggles group collapse.
```

---

### Task 12: Item Toggling + Undo/Redo

**Files:**
- Modify: `inspectah-tui/src/app.rs`

- [ ] **Step 1: Implement toggle action**

The TUI needs to construct a `RefinementOp::SetInclude` from the current cursor item. This requires mapping the `ListItem` back to an `ItemId`. Add an `item_id` field to `ListItem`:

In `triage_list.rs`, add to `ListItem`:
```rust
pub item_id: Option<inspectah_refine::types::ItemId>,
```

In `build_list_items`, populate `item_id` from the domain item. For packages:
```rust
item_id: Some(ItemId::Package {
    name: p.entry.name.clone(),
    arch: p.entry.arch.clone(),
}),
```

Then in `app.rs`:

```rust
Action::ToggleItem => {
    let items = self.current_items();
    if let Some(item) = items.get(self.state.cursor) {
        if let Some(ref item_id) = item.item_id {
            let new_include = !item.included.unwrap_or(true);
            let op = inspectah_refine::types::RefinementOp::SetInclude {
                item_id: item_id.clone(),
                include: new_include,
            };
            if let Err(e) = self.session.apply(op) {
                self.state.flash = Some(FlashMessage::new(
                    format!("Toggle failed: {e}"),
                    3,
                ));
            }
        }
    }
}
```

- [ ] **Step 2: Implement undo/redo**

```rust
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
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p inspectah-tui`
Expected: compiles with no errors.

- [ ] **Step 4: Manual smoke test**

With a test tarball:
- Navigate to a package, press Space — indicator should toggle
- Press u — undo, indicator reverts
- Press Ctrl+r — redo, indicator toggles again
- Reference section items: Space should be a no-op (no item_id)

- [ ] **Step 5: Commit**

```
feat(tui): wire item toggling and undo/redo

Space toggles include/exclude via RefinementOp::SetInclude.
u/Ctrl+r for undo/redo. Flash messages on errors.
```

---

### Task 13: Info Bar Widget (Compact Detail)

**Files:**
- Create: `inspectah-tui/src/widget/info_bar.rs`
- Modify: `inspectah-tui/src/widget/mod.rs`
- Modify: `inspectah-tui/src/screen/single_host.rs`
- Modify: `inspectah-tui/src/app.rs`

- [ ] **Step 1: Write info_bar.rs**

```rust
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::theme::{ColorTier, Token};

/// Data for the compact info bar (2-3 rows at bottom of item list).
pub struct InfoBarData {
    pub name: String,
    pub fields: Vec<(String, String)>,
}

pub struct InfoBarWidget<'a> {
    data: &'a InfoBarData,
    tier: ColorTier,
}

impl<'a> InfoBarWidget<'a> {
    pub fn new(data: &'a InfoBarData, tier: ColorTier) -> Self {
        Self { data, tier }
    }
}

impl Widget for InfoBarWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 2 || area.width < 20 {
            return;
        }

        // Separator line
        let sep = "─".repeat(area.width as usize);
        buf.set_string(area.x, area.y, &sep, Token::TextMuted.style(self.tier));

        // Item name on first data row
        buf.set_string(
            area.x + 1,
            area.y + 1,
            &self.data.name,
            Token::TextPrimary.style(self.tier),
        );

        // Key-value fields on subsequent rows
        for (i, (key, value)) in self.data.fields.iter().enumerate() {
            let row_y = area.y + 2 + i as u16;
            if row_y >= area.bottom() {
                break;
            }
            let label = format!("  {}: ", key);
            buf.set_string(area.x, row_y, &label, Token::TextMuted.style(self.tier));
            buf.set_string(
                area.x + label.len() as u16,
                row_y,
                value,
                Token::TextPrimary.style(self.tier),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::buffer_to_string;

    #[test]
    fn renders_package_info() {
        let data = InfoBarData {
            name: "httpd.x86_64".into(),
            fields: vec![
                ("Version".into(), "2.4.62".into()),
                ("Repo".into(), "rhel-9-appstream".into()),
                ("Reason".into(), "User-added package".into()),
            ],
        };
        let widget = InfoBarWidget::new(&data, ColorTier::Mono);
        let area = Rect::new(0, 0, 50, 5);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        insta::assert_snapshot!(buffer_to_string(&buf));
    }
}
```

- [ ] **Step 2: Wire info bar into SingleHost screen**

When `state.detail_mode == DetailMode::InfoBar`, split the list area vertically — main list gets most of the height, info bar gets 4 rows at the bottom.

In `single_host.rs`, after the main layout split:

```rust
let (list_area, info_area) = if state.detail_mode == DetailMode::InfoBar {
    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(4)])
        .split(list_area);
    (split[0], Some(split[1]))
} else {
    (list_area, None)
};
```

Render the info bar when present:

```rust
if let Some(info_area) = info_area {
    if let Some(data) = build_info_bar_data(session, state, &items) {
        let info = InfoBarWidget::new(&data, tier);
        frame.render_widget(info, info_area);
    }
}
```

The `build_info_bar_data` function extracts key-value fields from the current cursor item based on its type (package, config, service, etc.).

- [ ] **Step 3: Wire Enter/Esc in app.rs**

Enter opens info bar (already partially done in Task 11). Esc closes it:

```rust
Action::CloseDetail => {
    if self.state.detail_mode != DetailMode::None {
        self.state.detail_mode = DetailMode::None;
    }
}
```

- [ ] **Step 4: Update widget/mod.rs**

Add `pub mod info_bar;`.

- [ ] **Step 5: Run tests and review snapshots**

Run: `cargo test -p inspectah-tui -- info_bar && cargo insta review -p inspectah-tui`

- [ ] **Step 6: Commit**

```
feat(tui): add compact info bar for metadata items

2-3 row detail at bottom of item list. Shows key-value fields
(version, repo, reason for packages; state, owner for services).
Enter opens, Esc closes.
```

---

### Task 14: Detail View Widget (Fullscreen)

**Files:**
- Create: `inspectah-tui/src/widget/detail_view.rs`
- Modify: `inspectah-tui/src/widget/mod.rs`
- Modify: `inspectah-tui/src/screen/single_host.rs`
- Modify: `inspectah-tui/src/app.rs`

- [ ] **Step 1: Write detail_view.rs**

```rust
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{Paragraph, Widget, Wrap};

use crate::theme::{ColorTier, Token};

/// Content for fullscreen detail view.
pub struct DetailData {
    pub title: String,
    pub content: String,
    pub content_type: DetailContentType,
    pub include: Option<bool>,
    pub position: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailContentType {
    Diff,
    UnitFile,
    YamlContent,
    PlainText,
}

pub struct DetailViewWidget<'a> {
    data: &'a DetailData,
    scroll: u16,
    tier: ColorTier,
}

impl<'a> DetailViewWidget<'a> {
    pub fn new(data: &'a DetailData, scroll: u16, tier: ColorTier) -> Self {
        Self { data, scroll, tier }
    }
}

impl Widget for DetailViewWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 3 || area.width < 20 {
            return;
        }

        // Header: title + include state + position
        let include_indicator = match self.data.include {
            Some(true) => "● include",
            Some(false) => "○ exclude",
            None => "",
        };

        let header = format!(" {} {} {}", self.data.title, include_indicator, self.data.position);
        buf.set_string(area.x, area.y, &header, Token::TextPrimary.style(self.tier));

        // Separator
        let sep = "─".repeat(area.width as usize);
        buf.set_string(area.x, area.y + 1, &sep, Token::TextMuted.style(self.tier));

        // Content area
        let content_area = Rect {
            x: area.x,
            y: area.y + 2,
            width: area.width,
            height: area.height.saturating_sub(3),
        };

        // Render content lines with syntax-aware styling
        let lines: Vec<&str> = self.data.content.lines().collect();
        for (i, line) in lines.iter().enumerate().skip(self.scroll as usize) {
            let row_y = content_area.y + (i - self.scroll as usize) as u16;
            if row_y >= content_area.bottom() {
                break;
            }

            let style = match self.data.content_type {
                DetailContentType::Diff => {
                    if line.starts_with('+') {
                        Token::DiffAdded.style(self.tier)
                    } else if line.starts_with('-') {
                        Token::DiffRemoved.style(self.tier)
                    } else {
                        Token::TextPrimary.style(self.tier)
                    }
                }
                _ => Token::TextPrimary.style(self.tier),
            };

            let truncated = if line.len() > area.width as usize {
                &line[..area.width as usize]
            } else {
                line
            };
            buf.set_string(area.x, row_y, truncated, style);
        }

        // Footer: key hints
        let footer_y = area.bottom() - 1;
        let hints = "Esc:close  Space:toggle  n/p:next/prev  f:fullscreen";
        buf.set_string(area.x + 1, footer_y, hints, Token::TextMuted.style(self.tier));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::buffer_to_string;

    #[test]
    fn renders_diff_content() {
        let data = DetailData {
            title: "/etc/httpd/conf/httpd.conf".into(),
            content: " ServerRoot \"/etc/httpd\"\n-Listen 80\n+Listen 8080\n ServerName localhost".into(),
            content_type: DetailContentType::Diff,
            include: Some(true),
            position: "[3/47]".into(),
        };
        let widget = DetailViewWidget::new(&data, 0, ColorTier::Mono);
        let area = Rect::new(0, 0, 60, 8);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        insta::assert_snapshot!(buffer_to_string(&buf));
    }
}
```

- [ ] **Step 2: Wire fullscreen detail into SingleHost screen**

When `state.detail_mode == DetailMode::Fullscreen`, render the detail view instead of the item list (takes over the entire list area):

```rust
if state.detail_mode == DetailMode::Fullscreen {
    if let Some(data) = build_detail_data(session, state, &items) {
        let detail = DetailViewWidget::new(&data, state.detail_scroll, tier);
        frame.render_widget(detail, list_area);
    }
    return; // Skip item list rendering
}
```

Add `detail_scroll: u16` field to `TuiState` (default 0).

- [ ] **Step 3: Wire detail navigation in app.rs**

```rust
Action::PromoteDetail => {
    if self.state.detail_mode == DetailMode::InfoBar {
        self.state.detail_mode = DetailMode::Fullscreen;
        self.state.detail_scroll = 0;
    }
}
Action::DetailNext => {
    if self.state.detail_mode == DetailMode::Fullscreen {
        let max = self.visible_item_count().saturating_sub(1);
        if self.state.cursor < max {
            self.state.cursor += 1;
            self.state.detail_scroll = 0;
        }
    }
}
Action::DetailPrev => {
    if self.state.detail_mode == DetailMode::Fullscreen {
        if self.state.cursor > 0 {
            self.state.cursor -= 1;
            self.state.detail_scroll = 0;
        }
    }
}
```

- [ ] **Step 4: Classify items as compact or fullscreen on Enter**

Items with text content (configs, quadlets, compose files) open fullscreen. Metadata items (packages, services) open info bar. Add classification logic:

```rust
// In handle_action, Action::OpenDetail:
fn item_has_content(item: &ListItem) -> bool {
    // Items with displayable text content get fullscreen
    // This is determined by the section type + whether content is available
    item.has_content
}
```

Add a `has_content: bool` field to `ListItem`, set by `build_list_items` based on item type.

- [ ] **Step 5: Update widget/mod.rs**

Add `pub mod detail_view;`.

- [ ] **Step 6: Run tests and review snapshots**

Run: `cargo test -p inspectah-tui -- detail_view && cargo insta review -p inspectah-tui`

- [ ] **Step 7: Commit**

```
feat(tui): add fullscreen detail view with diff highlighting

Fullscreen detail for content items (configs, quadlets, compose).
Diff syntax highlighting (green/red for +/-). n/p navigates
items in fullscreen, f promotes info bar to fullscreen.
```

---

> **THORN CHECKPOINT 3:** Core triage workflow. **Verification:**
> 1. Space on a decision item toggles include/exclude (indicator changes, containerfile preview updates)
> 2. Space on a reference item in a composite section (e.g., running container in Containers) — no-op, no crash
> 3. Space on a reference-only section item (e.g., VerChanges) — no-op
> 4. Undo after toggle — item reverts. Redo — toggles again
> 5. Multiple undos followed by a new toggle — redo tail is discarded (standard undo tree)
> 6. Enter on a package — compact info bar shows version/repo/reason
> 7. Enter on a config — fullscreen detail shows diff with +/- highlighting
> 8. n/p in fullscreen — navigates items, detail updates. Space from detail — toggles
>
> Request Thorn review before proceeding.

---

## Phase 4: Overlays

### Task 15: Help Screen

**Files:**
- Create: `inspectah-tui/src/widget/help_screen.rs`
- Modify: `inspectah-tui/src/widget/mod.rs`
- Modify: `inspectah-tui/src/app.rs`

- [ ] **Step 1: Write help_screen.rs**

```rust
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::theme::{ColorTier, Token};

const HELP_ENTRIES: &[(&str, &str)] = &[
    ("j/k ↑/↓", "Move cursor"),
    ("h/l ←/→", "Sidebar ↔ items"),
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
];

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
        // Clear area
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                buf[(x, y)].reset();
            }
        }

        // Title
        let title = " Keybindings — press ? or Esc to close ";
        buf.set_string(area.x + 1, area.y, title, Token::TextPrimary.style(self.tier));

        // Separator
        let sep = "─".repeat(area.width as usize);
        buf.set_string(area.x, area.y + 1, &sep, Token::TextMuted.style(self.tier));

        // Entries
        for (i, (key, desc)) in HELP_ENTRIES.iter().enumerate() {
            let row_y = area.y + 2 + i as u16;
            if row_y >= area.bottom().saturating_sub(1) {
                break;
            }

            if key.is_empty() {
                continue;
            }

            if desc.is_empty() {
                // Section header
                buf.set_string(area.x + 2, row_y, key, Token::TextPrimary.style(self.tier));
            } else {
                let key_width = 18;
                buf.set_string(area.x + 2, row_y, key, Token::Warning.style(self.tier));
                buf.set_string(
                    area.x + 2 + key_width,
                    row_y,
                    desc,
                    Token::TextPrimary.style(self.tier),
                );
            }
        }
    }
}
```

- [ ] **Step 2: Wire help toggle in app.rs**

```rust
Action::ShowHelp => {
    self.state.input_mode = if self.state.input_mode == InputMode::Help {
        InputMode::Normal
    } else {
        InputMode::Help
    };
}
```

In the render method, overlay the help screen when `input_mode == Help`:

```rust
if self.state.input_mode == InputMode::Help {
    let help = HelpScreenWidget::new(self.tier);
    frame.render_widget(help, frame.area());
}
```

- [ ] **Step 3: Update widget/mod.rs, run tests**

Add `pub mod help_screen;`.

Run: `cargo check -p inspectah-tui`

- [ ] **Step 4: Commit**

```
feat(tui): add help screen overlay

? toggles keybinding reference. Lists all keys, section jumps,
and : commands. Esc or ? closes.
```

---

### Task 16: Search Overlay

**Files:**
- Create: `inspectah-tui/src/widget/search.rs`
- Modify: `inspectah-tui/src/widget/mod.rs`
- Modify: `inspectah-tui/src/app.rs`

- [ ] **Step 1: Write search.rs**

```rust
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::theme::{ColorTier, Token};
use crate::types::SectionId;

/// A search match with section attribution.
pub struct SearchResult {
    pub section_id: SectionId,
    pub name: String,
    pub match_context: String,
}

pub struct SearchWidget<'a> {
    query: &'a str,
    results: &'a [SearchResult],
    selected: usize,
    tier: ColorTier,
}

impl<'a> SearchWidget<'a> {
    pub fn new(query: &'a str, results: &'a [SearchResult], selected: usize, tier: ColorTier) -> Self {
        Self { query, results, selected, tier }
    }
}

impl Widget for SearchWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 3 || area.width < 20 {
            return;
        }

        // Search input line
        let prompt = format!("/ {}", self.query);
        buf.set_string(area.x, area.y, &prompt, Token::TextPrimary.style(self.tier));

        // Match count + section attribution
        if !self.results.is_empty() {
            let mut section_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
            for r in self.results {
                *section_counts.entry(r.section_id.label()).or_default() += 1;
            }
            let attribution: Vec<String> = section_counts
                .iter()
                .map(|(name, count)| format!("{}({})", name, count))
                .collect();
            let count_line = format!(
                "{} matches — {}",
                self.results.len(),
                attribution.join(", ")
            );
            let count_x = area.right().saturating_sub(count_line.len() as u16 + 1);
            buf.set_string(count_x, area.y, &count_line, Token::TextMuted.style(self.tier));
        }

        // Separator
        let sep = "─".repeat(area.width as usize);
        buf.set_string(area.x, area.y + 1, &sep, Token::TextMuted.style(self.tier));

        // Results
        for (i, result) in self.results.iter().enumerate() {
            let row_y = area.y + 2 + i as u16;
            if row_y >= area.bottom() {
                break;
            }

            let is_selected = i == self.selected;
            let style = if is_selected {
                Token::FocusSelected.style(self.tier)
            } else {
                Token::TextPrimary.style(self.tier)
            };

            let section_label = format!("[{}] ", result.section_id.label());
            buf.set_string(area.x + 1, row_y, &section_label, Token::TextMuted.style(self.tier));
            buf.set_string(
                area.x + 1 + section_label.len() as u16,
                row_y,
                &result.name,
                style,
            );
        }
    }
}

/// Perform cross-section search across all items.
pub fn search_all_sections(
    session: &inspectah_refine::session::RefineSession,
    query: &str,
) -> Vec<SearchResult> {
    if query.is_empty() {
        return Vec::new();
    }

    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    let view = session.view();
    let decisions = session.decisions();

    // Search packages
    for pkg in &view.packages {
        let name = format!("{}.{}", pkg.entry.name, pkg.entry.arch);
        if name.to_lowercase().contains(&query_lower) {
            results.push(SearchResult {
                section_id: SectionId::Packages,
                name,
                match_context: pkg.entry.version.clone(),
            });
        }
    }

    // Search configs
    for cfg in &view.config_files {
        let path = cfg.entry.path.to_string_lossy().to_string();
        if path.to_lowercase().contains(&query_lower) {
            results.push(SearchResult {
                section_id: SectionId::Configs,
                name: path,
                match_context: String::new(),
            });
        }
    }

    // Search services (composite — decision + reference)
    for svc in &decisions.service_states {
        if svc.entry.unit.to_lowercase().contains(&query_lower) {
            results.push(SearchResult {
                section_id: SectionId::Services,
                name: svc.entry.unit.clone(),
                match_context: String::new(),
            });
        }
    }

    // Search containers (composite — decision quadlets/flatpaks + reference)
    for q in &decisions.quadlets {
        let name = q.entry.name.clone();
        if name.to_lowercase().contains(&query_lower) {
            results.push(SearchResult {
                section_id: SectionId::Containers,
                name,
                match_context: "quadlet".into(),
            });
        }
    }
    for f in &decisions.flatpaks {
        if f.entry.app_id.to_lowercase().contains(&query_lower) {
            results.push(SearchResult {
                section_id: SectionId::Containers,
                name: f.entry.app_id.clone(),
                match_context: "flatpak".into(),
            });
        }
    }

    // Search sysctls
    for s in &decisions.sysctls {
        let key = s.entry.key.clone();
        if key.to_lowercase().contains(&query_lower) {
            results.push(SearchResult {
                section_id: SectionId::Sysctls,
                name: key,
                match_context: String::new(),
            });
        }
    }

    // Search users
    for u in &decisions.users_groups {
        let username = u.username.clone();
        if username.to_lowercase().contains(&query_lower) {
            results.push(SearchResult {
                section_id: SectionId::Users,
                name: username,
                match_context: String::new(),
            });
        }
    }

    // Search reference sections: version changes, network, storage, etc.
    // Each uses GenericRefItem fields (id, key) or typed fields.
    // See Implementation Notes § Cross-section search fields for the
    // complete field list per section. Pattern is identical: iterate
    // items, match name/key against query, push SearchResult.

    let reference = session.reference();

    for item in &reference.scheduled_tasks {
        if item.key.to_lowercase().contains(&query_lower) {
            results.push(SearchResult {
                section_id: SectionId::ScheduledTasks,
                name: item.key.clone(),
                match_context: item.summary.clone().unwrap_or_default(),
            });
        }
    }
    // Repeat for: non_rpm_software, selinux (GenericRefItem),
    // version_changes (name field), kernel_boot, network, storage
    // (typed fields — see search fields table in Implementation Notes).

    results
}
```

- [ ] **Step 2: Wire search mode in app.rs**

```rust
Action::EnterSearch => {
    self.state.input_mode = InputMode::Search;
    self.state.search_query.clear();
    self.search_results.clear();
    self.search_selected = 0;
}
```

Add `search_results: Vec<SearchResult>` and `search_selected: usize` to `App`.

On `InputChar` in search mode, update search results:
```rust
// In handle_action, when input_mode is Search:
Action::InputChar(ch) => {
    if self.state.input_mode == InputMode::Search {
        self.state.search_query.push(ch);
        self.search_results = search_all_sections(&self.session, &self.state.search_query);
        self.search_selected = 0;
    }
    // ... similar for Command mode
}
```

On Enter in search mode, navigate to selected result:
```rust
Action::SubmitInput if self.state.input_mode == InputMode::Search => {
    if let Some(result) = self.search_results.get(self.search_selected) {
        // Find section index for result.section_id
        // Set active_section and cursor
        self.state.input_mode = InputMode::Normal;
    }
}
```

On Esc, cancel search:
```rust
Action::CancelInput if self.state.input_mode == InputMode::Search => {
    self.state.input_mode = InputMode::Normal;
    self.state.search_query.clear();
}
```

- [ ] **Step 3: Update widget/mod.rs**

Add `pub mod search;`.

- [ ] **Step 4: Run tests**

Run: `cargo check -p inspectah-tui`

- [ ] **Step 5: Commit**

```
feat(tui): add cross-section search overlay

/ opens search. Real-time fuzzy filter across all sections with
section attribution counts. Enter navigates to matched item.
Esc cancels.
```

---

### Task 17: Command Line

**Files:**
- Create: `inspectah-tui/src/widget/command_line.rs`
- Modify: `inspectah-tui/src/widget/mod.rs`
- Modify: `inspectah-tui/src/app.rs`

- [ ] **Step 1: Write command_line.rs**

```rust
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::theme::{ColorTier, Token};

pub struct CommandLineWidget<'a> {
    input: &'a str,
    completion: Option<&'a str>,
    tier: ColorTier,
}

impl<'a> CommandLineWidget<'a> {
    pub fn new(input: &'a str, completion: Option<&'a str>, tier: ColorTier) -> Self {
        Self { input, completion, tier }
    }
}

impl Widget for CommandLineWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }
        let prompt = format!(":{}", self.input);
        buf.set_string(area.x, area.y, &prompt, Token::TextPrimary.style(self.tier));

        if let Some(comp) = self.completion {
            let comp_x = area.x + prompt.len() as u16;
            buf.set_string(comp_x, area.y, comp, Token::TextMuted.style(self.tier));
        }
    }
}

/// Available commands with tab-completable names.
pub const COMMANDS: &[&str] = &["export", "fresh", "redo", "section", "stats", "undo"];

/// Parse and return the command name and arguments from input.
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

/// Find tab completion for the current input.
pub fn complete(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    // If input has a space, complete the argument (section names)
    if let Some((_cmd, partial)) = trimmed.split_once(' ') {
        // Section name completion handled by caller
        return None;
    }

    // Command name completion
    let matches: Vec<&&str> = COMMANDS.iter().filter(|c| c.starts_with(trimmed)).collect();
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
    fn parse_export_with_path() {
        let (cmd, args) = parse_command("export /tmp/out.tar.gz").unwrap();
        assert_eq!(cmd, "export");
        assert_eq!(args, "/tmp/out.tar.gz");
    }

    #[test]
    fn parse_simple_command() {
        let (cmd, args) = parse_command("stats").unwrap();
        assert_eq!(cmd, "stats");
        assert_eq!(args, "");
    }

    #[test]
    fn complete_partial() {
        assert_eq!(complete("ex"), Some("export".into()));
        assert_eq!(complete("st"), Some("stats".into()));
    }

    #[test]
    fn complete_ambiguous_returns_none() {
        // "s" matches "section" and "stats"
        assert_eq!(complete("s"), None);
    }
}
```

- [ ] **Step 2: Wire command dispatch in app.rs**

```rust
Action::EnterCommand => {
    self.state.input_mode = InputMode::Command;
    self.state.command_input.clear();
}

// On SubmitInput in Command mode:
Action::SubmitInput if self.state.input_mode == InputMode::Command => {
    self.execute_command(&self.state.command_input.clone());
    self.state.input_mode = InputMode::Normal;
    self.state.command_input.clear();
}
```

```rust
fn execute_command(&mut self, input: &str) {
    use crate::widget::command_line::parse_command;

    let Some((cmd, args)) = parse_command(input) else {
        return;
    };

    match cmd {
        "export" => {
            // Handled in Task 20 (Export Safety)
            self.state.flash = Some(FlashMessage::new("Export: not yet implemented", 3));
        }
        "section" => {
            let sections = sections::build_section_entries(&self.session);
            let target = args.trim().to_lowercase();
            if let Some(idx) = sections.iter().position(|s| {
                s.id.label().to_lowercase().starts_with(&target)
            }) {
                self.state.section_cursors[self.state.active_section] = self.state.cursor;
                self.state.active_section = idx;
                self.state.cursor = self.state.section_cursors[idx];
                self.state.focus = FocusTarget::ItemList;
            }
        }
        "stats" => {
            let view = self.session.view();
            let msg = format!(
                "Ops: {} | Undo: {} | Redo: {} | Review: {}",
                view.stats.ops_applied,
                view.stats.can_undo,
                view.stats.can_redo,
                view.stats.needs_review_count,
            );
            self.state.flash = Some(FlashMessage::new(msg, 5));
        }
        "undo" => {
            let _ = self.session.undo();
        }
        "redo" => {
            let _ = self.session.redo();
        }
        "fresh" => {
            // Discard current session, delete sidecar, reload from tarball.
            // Matches CLI's fresh-start pattern (refine.rs line 63/75):
            //   let _ = std::fs::remove_file(&session_path);
            // This ensures the old session cannot be resumed on next launch.
            if let Some(ref tarball_path) = self.tarball_path {
                // 1. Delete sidecar file so it can't be resumed
                let session_path = inspectah_refine::autosave::session_file_path(tarball_path);
                let _ = std::fs::remove_file(&session_path);

                // 2. Reload session from tarball
                match inspectah_refine::tarball::from_tarball(tarball_path) {
                    Ok(mut new_session) => {
                        new_session.set_tarball_path(tarball_path.clone());
                        self.session = new_session;
                        self.state = TuiState::new(crate::sections::SECTION_ORDER.len());
                        self.state.flash = Some(FlashMessage::new(
                            "Session discarded — starting fresh", 3,
                        ));
                    }
                    Err(e) => {
                        self.state.flash = Some(FlashMessage::new(
                            format!("Fresh failed: {e}"), 5,
                        ));
                    }
                }
            } else {
                self.state.flash = Some(FlashMessage::new(
                    "No tarball path — cannot restart", 3,
                ));
            }
        }
        _ => {
            self.state.flash = Some(FlashMessage::new(format!("Unknown command: {}", cmd), 3));
        }
    }
}
```

- [ ] **Step 3: Tab completion wiring**

```rust
Action::TabComplete => {
    if let Some(completed) = command_line::complete(&self.state.command_input) {
        self.state.command_input = completed;
    }
}
```

- [ ] **Step 4: Update widget/mod.rs**

Add `pub mod command_line;`.

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-tui -- command_line`
Expected: parse and completion tests pass.

- [ ] **Step 6: Commit**

```
feat(tui): add command line with tab completion

: opens command mode. Supports :export, :section, :stats,
:undo, :redo, :fresh. Tab completes command names.
```

---

### Task 18: Containerfile Toggle

**Files:**
- Create: `inspectah-tui/src/widget/containerfile.rs`
- Modify: `inspectah-tui/src/widget/mod.rs`
- Modify: `inspectah-tui/src/screen/single_host.rs`
- Modify: `inspectah-tui/src/app.rs`

- [ ] **Step 1: Write containerfile.rs**

```rust
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::theme::{ColorTier, Token};

pub struct ContainerfileWidget<'a> {
    content: &'a str,
    tier: ColorTier,
}

impl<'a> ContainerfileWidget<'a> {
    pub fn new(content: &'a str, tier: ColorTier) -> Self {
        Self { content, tier }
    }
}

impl Widget for ContainerfileWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 2 || area.width < 10 {
            return;
        }

        // Header
        let header = " Containerfile Preview ";
        buf.set_string(area.x, area.y, header, Token::TextPrimary.style(self.tier));

        // Separator
        let sep = "─".repeat(area.width as usize);
        buf.set_string(area.x, area.y + 1, &sep, Token::TextMuted.style(self.tier));

        // Content with line numbers
        for (i, line) in self.content.lines().enumerate() {
            let row_y = area.y + 2 + i as u16;
            if row_y >= area.bottom() {
                break;
            }

            let line_num = format!("{:>3} ", i + 1);
            buf.set_string(area.x, row_y, &line_num, Token::TextMuted.style(self.tier));

            let max_width = area.width.saturating_sub(4) as usize;
            let display_line = if line.len() > max_width {
                &line[..max_width]
            } else {
                line
            };

            // Dockerfile-style highlighting
            let style = if line.starts_with("FROM") || line.starts_with("RUN") || line.starts_with("COPY") || line.starts_with("ADD") || line.starts_with("ENV") {
                Token::DiffAdded.style(self.tier)
            } else if line.starts_with('#') {
                Token::TextMuted.style(self.tier)
            } else {
                Token::TextPrimary.style(self.tier)
            };

            buf.set_string(area.x + 4, row_y, display_line, style);
        }
    }
}
```

- [ ] **Step 2: Wire containerfile toggle in screen layout**

In `single_host.rs`, when `state.show_containerfile` is true, use a three-way horizontal split: items left, containerfile right, sidebar hidden.

```rust
if state.show_containerfile {
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_area);

    // Render item list in left half
    // ... (same as normal, but wider)

    // Render containerfile in right half
    let cf_content = &session.view().containerfile_preview;
    let cf_widget = ContainerfileWidget::new(cf_content, tier);
    frame.render_widget(cf_widget, horizontal[1]);
} else {
    // Normal two-panel layout (sidebar + items)
    // ... existing code
}
```

- [ ] **Step 3: Wire toggle action in app.rs**

```rust
Action::ToggleContainerfile => {
    self.state.show_containerfile = !self.state.show_containerfile;
}
```

- [ ] **Step 4: Update widget/mod.rs**

Add `pub mod containerfile;`.

- [ ] **Step 5: Run cargo check**

Run: `cargo check -p inspectah-tui`

- [ ] **Step 6: Commit**

```
feat(tui): add containerfile toggle panel

c toggles side-by-side view: items left, containerfile right.
Sidebar hides to give both panels room. Dockerfile keyword
highlighting. c or Esc returns to default view.
```

---

> **THORN CHECKPOINT 4:** Overlays and commands. **Verification:**
> 1. `/httpd` — search finds packages and services containing "httpd", section attribution shown
> 2. Enter on a search result — navigates to the item in its section, cursor on the right row
> 3. Esc in search — restores previous section and cursor position
> 4. `:export` on a non-sensitive session — exports immediately, shows path
> 5. `:export` on a sensitive session — shows y/N prompt. `n` cancels. `y` exports. Enter alone cancels (default N)
> 6. `:fresh` — deletes sidecar file, reloads session, all ops cleared. Next quit + relaunch starts fresh (no resume prompt)
> 7. `:section containers` — jumps to Containers section. Tab-completion works for both command names and section names
> 8. `c` — containerfile preview panel appears, sidebar hides. `c` again returns to normal view
> 9. `?` — help screen shows all keybindings. `?` again or Esc closes
>
> Request Thorn review before proceeding.

---

## Phase 5: Features

### Task 19: User Strategy View

**Files:**
- Create: `inspectah-tui/src/widget/user_strategy.rs`
- Modify: `inspectah-tui/src/widget/mod.rs`
- Modify: `inspectah-tui/src/screen/single_host.rs`

- [ ] **Step 1: Write user_strategy.rs**

```rust
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::theme::{ColorTier, Token};

pub struct UserEntry {
    pub username: String,
    pub uid: u32,
    pub strategy: String,
    pub has_password: bool,
}

pub struct UserStrategyWidget<'a> {
    users: &'a [UserEntry],
    cursor: usize,
    tier: ColorTier,
}

impl<'a> UserStrategyWidget<'a> {
    pub fn new(users: &'a [UserEntry], cursor: usize, tier: ColorTier) -> Self {
        Self { users, cursor, tier }
    }
}

impl Widget for UserStrategyWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 3 {
            return;
        }

        // Header
        let header = " Users — Space: cycle strategy, Enter: password options ";
        buf.set_string(area.x, area.y, header, Token::TextPrimary.style(self.tier));

        let sep = "─".repeat(area.width as usize);
        buf.set_string(area.x, area.y + 1, &sep, Token::TextMuted.style(self.tier));

        // Column headers
        let col_header = format!(
            "  {:<20} {:<6} {:<12} {}",
            "Username", "UID", "Strategy", "Password"
        );
        buf.set_string(area.x, area.y + 2, &col_header, Token::TextMuted.style(self.tier));

        // User rows
        for (i, user) in self.users.iter().enumerate() {
            let row_y = area.y + 3 + i as u16;
            if row_y >= area.bottom() {
                break;
            }

            let is_cursor = i == self.cursor;
            let style = if is_cursor {
                Token::FocusSelected.style(self.tier)
            } else {
                Token::TextPrimary.style(self.tier)
            };

            let pwd = if user.has_password { "●" } else { "—" };
            let row = format!(
                "  {:<20} {:<6} {:<12} {}",
                user.username, user.uid, user.strategy, pwd
            );
            buf.set_string(area.x, row_y, &row, style);
        }
    }
}
```

- [ ] **Step 2: Wire user strategy into SingleHost screen**

When the active section is `SectionId::Users`, render the user strategy widget instead of the normal triage list.

```rust
// In SingleHostScreen::render, after selecting the active section:
if active_section_id == SectionId::Users {
    let users = build_user_entries(session);
    let user_widget = UserStrategyWidget::new(&users, state.cursor, tier);
    frame.render_widget(user_widget, list_area);
    return;
}
```

- [ ] **Step 3: Wire Space to cycle user strategy**

In `app.rs`, when the active section is Users and Space is pressed, cycle through strategies (skip → useradd):

```rust
// In handle_action, ToggleItem when section is Users:
let sections = sections::build_section_entries(&self.session);
if sections[self.state.active_section].id == SectionId::Users {
    let users = build_user_entries(&self.session);
    if let Some(user) = users.get(self.state.cursor) {
        let new_strategy = match user.strategy.as_str() {
            "skip" => UserContainerfileStrategy::Useradd,
            "useradd" => UserContainerfileStrategy::Skip,
            _ => UserContainerfileStrategy::Skip,
        };
        let op = RefinementOp::UserStrategy {
            username: user.username.clone(),
            strategy: new_strategy,
        };
        let _ = self.session.apply(op);
    }
}
```

- [ ] **Step 4: Wire Enter for password management**

When the active section is Users and Enter is pressed on a user, open a password sub-menu. The password operations map to `RefinementOp::UserPassword`:

```rust
// Three password choices cycle with repeated Enter presses,
// or render a sub-menu:
// - "none"     → UserPasswordOp::None { username }     — clear password
// - "preserve" → UserPasswordOp::Preserve { username } — keep original hash
// - "new"      → UserPasswordOp::New { username, hash } — set new hash
//
// For v1: Enter cycles None → Preserve → New (with empty hash).
// Full hash input (prompting for a password string) is deferred.

let pwd_op = match user.password_choice.as_str() {
    "none" => RefinementOp::UserPassword(UserPasswordOp::Preserve {
        username: user.username.clone(),
    }),
    "preserve" => RefinementOp::UserPassword(UserPasswordOp::New {
        username: user.username.clone(),
        hash: None, // deferred: prompt for hash value
    }),
    _ => RefinementOp::UserPassword(UserPasswordOp::None {
        username: user.username.clone(),
    }),
};
let _ = self.session.apply(pwd_op);
```

Add `password_choice: String` to `UserEntry` (populated from the projected user's `password_choice` field).

- [ ] **Step 5: Update widget/mod.rs**

Add `pub mod user_strategy;`.

- [ ] **Step 5: Commit**

```
feat(tui): add user strategy interactive view

Fullscreen per-user strategy selection (skip/useradd) with
password indicator. Space cycles strategy, Enter opens
password options.
```

---

### Task 20: Export Safety

**Files:**
- Modify: `inspectah-tui/src/app.rs`

- [ ] **Step 1: Implement export with sensitive confirmation**

```rust
fn execute_command(&mut self, input: &str) {
    // ... existing command dispatch ...
    match cmd {
        "export" => {
            if self.session.is_sensitive() {
                // Enter confirmation mode
                self.state.input_mode = InputMode::Confirm;
                self.pending_export_path = Some(
                    args.trim()
                        .parse::<std::path::PathBuf>()
                        .unwrap_or_else(|_| {
                            std::path::PathBuf::from("./inspectah-export.tar.gz")
                        }),
                );
                // Flash shows the confirmation prompt
                // (Rendered inline by the confirm overlay)
            } else {
                self.do_export(args);
            }
        }
        // ...
    }
}
```

- [ ] **Step 2: Handle confirmation response**

```rust
Action::ConfirmYes => {
    if let Some(path) = self.pending_export_path.take() {
        self.do_export(&path.to_string_lossy());
    }
    self.state.input_mode = InputMode::Normal;
}
Action::ConfirmNo => {
    self.pending_export_path = None;
    self.state.input_mode = InputMode::Normal;
    self.state.flash = Some(FlashMessage::new("Export cancelled.", 3));
}
```

- [ ] **Step 3: Implement do_export**

```rust
fn do_export(&mut self, path_arg: &str) {
    let path = if path_arg.is_empty() {
        std::path::PathBuf::from("./inspectah-export.tar.gz")
    } else {
        std::path::PathBuf::from(path_arg)
    };

    let generation = self.session.view().generation;
    match self.session.export_tarball(&path, generation) {
        Ok(()) => {
            self.state.flash = Some(FlashMessage::new(
                format!("Exported to {}", path.display()),
                5,
            ));
        }
        Err(e) => {
            self.state.flash = Some(FlashMessage::new(
                format!("Export failed: {e}"),
                5,
            ));
        }
    }
}
```

Add `pending_export_path: Option<std::path::PathBuf>` to `App`.

- [ ] **Step 4: Render confirmation prompt**

In the render method, when `input_mode == Confirm`:

```rust
if self.state.input_mode == InputMode::Confirm {
    // 3-row confirmation block at bottom
    let confirm_area = Rect {
        x: area.x + 2,
        y: area.bottom().saturating_sub(4),
        width: area.width.saturating_sub(4),
        height: 3,
    };

    // Clear area
    for y in confirm_area.y..confirm_area.bottom() {
        for x in confirm_area.x..confirm_area.right() {
            buf[(x, y)].reset();
        }
    }

    let warning = Token::Warning.style(self.tier);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("⚠ This session contains sensitive data.", warning)),
            Line::from(Span::styled("  Exported artifacts will include this data in plain text.", warning)),
            Line::from(Span::styled("  Proceed? [y/N]", warning)),
        ]),
        confirm_area,
    );
}
```

- [ ] **Step 5: Commit**

```
feat(tui): add export with sensitive data confirmation

:export [path] exports tarball. When is_sensitive() is true,
shows y/N confirmation prompt. Only y/Y proceeds. Default
path: ./inspectah-export.tar.gz.
```

---

### Task 21: Reviewed Progress

**Files:**
- Modify: `inspectah-tui/src/app.rs`
- Modify: `inspectah-tui/src/widget/status_bar.rs`
- Modify: `inspectah-tui/src/widget/section_nav.rs`

- [ ] **Step 1: Call mark_viewed on detail open**

In `app.rs`, when `OpenDetail` opens a detail view, call `mark_viewed` using the section's `viewed_prefix()`. This respects the `VALID_SECTIONS` contract — only sections with a valid prefix get reviewed tracking. Sections without a prefix (Sysctls, Tuned, VerChanges) silently skip the call.

**Critical contract:** `mark_viewed()` validates against `VALID_SECTIONS` = `[packages, configs, services, containers, users_groups, network, storage, scheduled_tasks, non_rpm_software, kernel_boot, selinux]`. Items in sections without a VALID_SECTIONS prefix (sysctls, tuned) cannot be marked viewed. Quadlet/flatpak items use the `containers:` prefix, not `quadlets:` or `flatpaks:`. Repo items are embedded in packages — there is no `repos:` prefix.

```rust
Action::OpenDetail => {
    let items = self.current_items();
    if let Some(item) = items.get(self.state.cursor) {
        if item.is_group_header {
            // Toggle group collapse (existing)
        } else {
            // Mark as viewed — only for sections with a valid viewed prefix
            let sections = sections::build_section_entries(&self.session);
            if let Some(section) = sections.get(self.state.active_section) {
                if let Some(prefix) = section.id.viewed_prefix() {
                    if let Some(ref item_id) = item.item_id {
                        let viewed_key = format!(
                            "{}:{}",
                            prefix,
                            item_id_to_viewed_key(item_id),
                        );
                        let _ = self.session.mark_viewed(&viewed_key);
                    }
                }
            }

            // Open appropriate detail mode
            if item.has_content {
                self.state.detail_mode = DetailMode::Fullscreen;
            } else {
                self.state.detail_mode = DetailMode::InfoBar;
            }
        }
    }
}
```

Helper function for the item key portion (after the `:`):
```rust
fn item_id_to_viewed_key(item_id: &ItemId) -> String {
    match item_id {
        ItemId::Package { name, arch } => format!("{}.{}", name, arch),
        ItemId::Config { path } => path.clone(),
        ItemId::Service { unit } => unit.clone(),
        ItemId::DropIn { path } => path.clone(),
        ItemId::Quadlet { path } => path.clone(),
        ItemId::Flatpak { app_id, .. } => app_id.clone(),
        ItemId::Compose { path } => path.clone(),
        _ => String::new(),
    }
}
```

Note: `viewed_prefix()` returns `None` for `SectionId::Sysctls` and `SectionId::Tuned` because `"sysctls"` and `"tuned"` are not in `VALID_SECTIONS`. Items in those sections silently skip reviewed tracking. This matches the code contract in `RefineSession::validate_viewed_id()`.

- [ ] **Step 2: Show reviewed progress in status bar**

Count viewed items per-section using `viewed_prefix()`:

```rust
let active_section = sections.get(state.active_section);
let active_reviewed = if let Some(section) = active_section {
    if let Some(prefix) = section.id.viewed_prefix() {
        let prefix_colon = format!("{}:", prefix);
        session.viewed_ids().iter().filter(|k| k.starts_with(&prefix_colon)).count()
    } else {
        0 // Sections without a viewed prefix don't track reviewed state
    }
} else {
    0
};

// Total reviewable = count of decision items in sections that support reviewed tracking
let total_reviewable: usize = sections
    .iter()
    .filter(|s| s.id.viewed_prefix().is_some() && s.id.is_decision())
    .map(|s| s.count)
    .sum();

let status = StatusBarWidget::new(tier)
    .stats(...)
    .reviewed_progress(active_reviewed, active_section.map(|s| s.count).unwrap_or(0));
```

- [ ] **Step 3: Commit**

```
feat(tui): add reviewed progress tracking

Enter marks item as viewed via mark_viewed() using the section's
VALID_SECTIONS prefix. Sections without a valid prefix (sysctls,
tuned) silently skip. Status bar shows per-section reviewed/total.
```

---

## Phase 6: Integration

### Task 22: CLI Integration + Startup Flow

**Files:**
- Create: `inspectah-cli/src/commands/tui.rs`
- Modify: `inspectah-cli/src/commands/mod.rs`
- Modify: `inspectah-cli/src/main.rs`
- Modify: `inspectah-cli/Cargo.toml`

- [ ] **Step 1: Add inspectah-tui dependency to CLI Cargo.toml**

In `inspectah-cli/Cargo.toml`, add:
```toml
inspectah-tui = { path = "../inspectah-tui" }
```

- [ ] **Step 2: Write commands/tui.rs**

```rust
use std::path::PathBuf;

use clap::Args;
use inspectah_refine::session::RefineSession;

use crate::commands::refine::{ResolveResult, SessionChoice};

#[derive(Args)]
pub struct TuiArgs {
    /// Path to an inspectah snapshot tarball
    pub tarball: PathBuf,

    /// Start fresh, discarding any saved session
    #[arg(long)]
    pub fresh: bool,
}

pub fn run_tui(args: &TuiArgs) -> anyhow::Result<()> {
    eprintln!("Loading snapshot...");

    let session = resolve_tui_session(&args.tarball, args.fresh)?;

    inspectah_tui::run_tui(session)?;

    Ok(())
}

fn resolve_tui_session(tarball: &std::path::Path, fresh: bool) -> anyhow::Result<RefineSession> {
    if fresh {
        let mut session = inspectah_refine::tarball::from_tarball(tarball)?;
        session.set_tarball_path(tarball.to_path_buf());
        return Ok(session);
    }

    match RefineSession::resume_from(tarball) {
        Ok(Some(session)) => {
            // Branch 2: Resume
            eprintln!("Resumed session ({} ops)", session.cursor());
            Ok(session)
        }
        Ok(None) => {
            // Branch 1: Fresh (no sidecar)
            let mut session = inspectah_refine::tarball::from_tarball(tarball)?;
            session.set_tarball_path(tarball.to_path_buf());
            Ok(session)
        }
        Err(inspectah_refine::types::RefineError::StaleTarball { .. }) => {
            // Branch 3: Stale sidecar
            eprintln!("Stale session discarded — tarball has changed");
            let mut session = inspectah_refine::tarball::from_tarball(tarball)?;
            session.set_tarball_path(tarball.to_path_buf());
            Ok(session)
        }
        Err(e) => {
            // Branch 4: Corrupt/unloadable
            anyhow::bail!("Failed to load session: {e}");
        }
    }
}
```

**Note:** The `resolve_tui_session` function is separate from `refine.rs`'s `resolve_session` because the TUI doesn't need the interactive choice prompt (Resume/Fresh/Quit) — it uses `--fresh` flag or auto-resumes. Flash messages for resume/stale are shown in the TUI status bar via `FlashMessage`, not printed to stderr. Wire the flash message into `run_tui`:

Update `inspectah_tui::run_tui` to accept a startup message:

```rust
// In lib.rs:
pub fn run_tui(session: RefineSession) -> color_eyre::Result<()> {
    app::App::new(session).run()
}
```

Pass resume flash from the CLI:
```rust
// In tui.rs, after creating session:
let flash = match &resume_status {
    ResumeStatus::Resumed(n) => Some(format!("Resumed session ({n} ops)")),
    ResumeStatus::Stale => Some("Stale session discarded — tarball has changed".into()),
    ResumeStatus::Fresh => None,
};
// Pass flash to run_tui or set on TuiState after App::new
```

- [ ] **Step 3: Add Tui variant to Commands enum in main.rs**

```rust
#[derive(Subcommand)]
enum Commands {
    Scan(commands::scan::ScanArgs),
    Refine(commands::refine::RefineArgs),
    /// Terminal UI for interactive refine workflow
    Tui(commands::tui::TuiArgs),
    Fleet(commands::fleet::FleetArgs),
    Build(commands::build::BuildArgs),
    Version,
}
```

Add the dispatch arm:
```rust
Commands::Tui(args) => match commands::tui::run_tui(&args) {
    Ok(()) => {}
    Err(e) => {
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}
```

- [ ] **Step 4: Update commands/mod.rs**

Add `pub mod tui;`.

- [ ] **Step 5: Verify full build**

Run: `cargo build -p inspectah-cli`
Expected: compiles. The `inspectah tui` subcommand is available.

- [ ] **Step 6: End-to-end smoke test**

```bash
cargo run -p inspectah-cli -- tui testdata/single-host-e2e.tar.gz
```

Verify:
- TUI launches with real data
- Sidebar shows sections with counts
- Navigation works (j/k, 1-9)
- Space toggles items
- u/Ctrl+r undo/redo
- Enter opens detail (info bar for packages, fullscreen for configs)
- / search works
- : command mode works
- :export produces a tarball
- q quits cleanly
- Terminal always restores

```bash
# Test --fresh flag
cargo run -p inspectah-cli -- tui --fresh testdata/single-host-e2e.tar.gz
```

```bash
# Test resume (run twice)
cargo run -p inspectah-cli -- tui testdata/single-host-e2e.tar.gz
# Toggle some items, quit
cargo run -p inspectah-cli -- tui testdata/single-host-e2e.tar.gz
# Should show "Resumed session (N ops)" flash
```

- [ ] **Step 7: Commit**

```
feat(tui): add CLI subcommand with session resume

`inspectah tui <tarball>` launches the TUI. Handles all
resume_from branches: fresh, resume, stale, corrupt.
--fresh flag to discard saved session.
```

---

> **THORN CHECKPOINT 5:** Feature-complete. **Verification:**
> 1. `inspectah tui <tarball>` — fresh launch, session loads, triage works end-to-end
> 2. Quit + relaunch same tarball — "Resumed session (N ops)" flash, all ops restored
> 3. Re-scan tarball (modify content), relaunch — "Stale session discarded" flash, starts fresh
> 4. Corrupt the sidecar JSON, relaunch — error message, exits cleanly (no crash, no alt-screen leak)
> 5. `--fresh` flag — sidecar deleted, fresh session even when valid sidecar exists
> 6. Users section — Space cycles strategy (skip/useradd), Enter cycles password (none/preserve/new)
> 7. Reviewed progress — Enter on item shows "N/M reviewed" counter. Counter only increments for sections with VALID_SECTIONS prefix (not sysctls/tuned)
> 8. Full e2e: navigate all 14 sections, toggle items, search, `:export`, `:fresh`, undo/redo, suspend/resume
>
> Request final Thorn review.

---

## Implementation Notes

### Section model (rev3)

The plan uses the spec's rev3 section model:
- **7 sidebar entries above separator** (decision/composite): Packages (with embedded repo bar), Configs, Services (composite), Containers (composite), Sysctls, Tuned, Users
- **7 sidebar entries below separator** (reference-only): VerChanges, KernelBoot, Network, Storage, ScheduledTasks, NonRpmSoftware, SELinux
- **Repos are NOT a standalone section** — they are rendered as a repo bar at the top of the Packages section (matching the web UI's `RepoBar.tsx`)
- **Services is composite** — decision items (service_states, dropins from `decisions()`) + reference context (divergent, advisories, warnings, omitted from `reference().services`). Decision items support Space toggle; reference items are read-only.
- **Containers is composite** — decision items (quadlets, flatpaks from `decisions()`) + reference items (running_containers, compose_files from `reference().containers`). Quadlet/flatpak items use `containers:` prefix for `mark_viewed()`, not standalone prefixes.

### Composite section rendering

For composite sections (Services, Containers), the triage list renders two sub-regions within the same sidebar section:
1. **Decision items** — grouped by triage bucket (Investigate/Site/Baseline), Space toggles include/exclude
2. **Reference items** — flat list below a separator header reading "Reference — read-only context", Space is no-op

The `build_list_items` function must produce `ListItem` entries for both sub-regions. Decision items get `item_id: Some(...)` and `included: Some(bool)`. Reference items get `item_id: None` and `included: None`.

**Services composite — all 8 reference sub-collections (from `reference().services`):**

| Sub-collection | Items | Display |
|---|---|---|
| `divergent` | Services with state divergent from preset | Unit name + state + implied action |
| `preset_matched_with_dropins` | Preset-matched but has drop-in override | Unit name + "matches preset, has drop-in" |
| `preset_unknown_enabled` | Enabled with no preset rule | Unit name + "enabled (no preset rule)" |
| `preset_unknown_disabled` | Disabled with no preset rule | Unit name + "disabled (no preset rule)" |
| `standalone_dropins` | Drop-in files without a divergent service | Unit name + "(drop-in)" + content |
| `omitted` | Package-proven absent services | Unit name + package + reason |
| `advisories` | Service-specific advisories | Unit name + owning package + reasons |
| `warnings` | Service warnings | Unit name + message |

The web adapter (`web_services_section` in `adapter.rs`) renders all 8. The TUI must render all 8 as reference items in the Services section.

**Containers composite — all 4 sub-collections (from `reference().containers`):**

| Sub-collection | Items | Display |
|---|---|---|
| `quadlets` | Quadlet unit files | Name + image + path (detail: content) |
| `compose_files` | Docker Compose files | Filename + service count (detail: YAML) |
| `running_containers` | Running OCI containers | Name + image + status |
| `flatpaks` | Flatpak apps | App ID + origin + branch |

The web adapter (`web_containers_section`) renders all 4. Quadlets and flatpaks from `decisions()` are decision items (togglable). The same-named items from `reference().containers` are reference context (read-only). The TUI must render both sets.

### Repo bar in Packages section

When the Packages section is active, the item list renders a repo bar at the top (above the triage groups). The repo bar shows repo names with package counts from `decisions().repo_groups`. This matches the web UI's `RepoBar.tsx` component pattern.

**Data source:** `decisions().repo_groups: Vec<RepoGroup>` — each has `section_id`, `provenance`, `is_distro`, `tier`, `package_count`, `enabled`.

**Rendering:** Single row at top of item list area. Repo names as horizontal chips: `baseos 88  appstream 54  epel 12`. Repos render as toggleable via Space — excluding a repo hides its packages (uses `RefinementOp::SetInclude` with `ItemId::Repo { path: section_id }`). This is wired in Task 10's `build_list_items` for the Packages arm.

**Task 10 must include:** A `RepoBar` rendering step in the Packages section layout. The repo bar consumes 2 rows of the item list area (border + content). The remaining height goes to the triage-grouped package list below.

### Sections not fully wired in Task 8

The `build_list_items` function in Task 8 only implements Packages and Configs. The remaining sections follow the same pattern. Complete all match arms during Task 11. Each arm:

1. Iterates items from the appropriate session accessor
2. Maps each domain type to `(name, detail, triage_bucket, include)` tuple
3. Calls `build_grouped_items` (pure decision) or builds mixed decision+reference lists (composite)

Consult the spec's "Section type mapping" table for exact data sources per section.

### Reviewed progress contract

`VALID_SECTIONS` in `RefineSession` = `[packages, configs, services, containers, users_groups, network, storage, scheduled_tasks, non_rpm_software, kernel_boot, selinux]`. The `SectionId::viewed_prefix()` method maps each sidebar section to its VALID_SECTIONS prefix (or `None` if the section has no valid prefix).

**NOT in VALID_SECTIONS:** `repos`, `quadlets`, `flatpaks`, `sysctls`, `tuned`, `compose`, `version_changes`. Items in these categories either use a parent section prefix (`quadlets` → `containers:`, `flatpaks` → `containers:`) or have no reviewed tracking (`sysctls`, `tuned`).

### Types that may need adjustment

- `ItemId` — the full enum has variants beyond what the plan shows. Check `inspectah-refine/src/types.rs` for all variants.
- `RefKernelBoot`, `RefNetwork`, `RefStorage` — may not have a `total_items()` method. Count their sub-fields directly.
- `PackageEntry`, `ConfigFileEntry`, `ServiceStateChange` — inner types of `RefinedPackage`, `RefinedConfig`, `RefinedServiceState`. Check field names in `inspectah-core` and `inspectah-refine` types.

### Cross-section search fields

The `search_all_sections` function must search these fields per section:

| Section | Searchable fields |
|---------|------------------|
| Packages | `name.arch`, version, source_repo |
| Configs | file path |
| Services (decision) | unit name, owning package |
| Services (reference) | unit name, advisory reason |
| Containers (decision) | quadlet name/path, flatpak app_id |
| Containers (reference) | container name/image, compose path |
| Sysctls | sysctl key |
| Tuned | profile name |
| Users | username |
| VerChanges | package name |
| KernelBoot | module name, sysctl key |
| Network | connection name, interface, zone name |
| Storage | mount point, device |
| ScheduledTasks | id, key |
| NonRpmSoftware | id, key |
| SELinux | id, key |

### Testing with fixture tarballs

Integration tests should use `testdata/single-host-e2e.tar.gz` or `tests/e2e/fixtures/single-host.tar.gz`. These create a `RefineSession` with real-world section data (packages, configs, services, etc.).

---

## Known Gaps (Pre-Flight for Tang)

These items surfaced in 3 review rounds and are documented here rather than embedded in task bodies. Tang must verify each against the spec and code during implementation — `cargo check` and the Thorn checkpoints will catch mismatches.

### 1. Composite section task wording vs. code contract

The `build_list_items` arms for `SectionId::Services` and `SectionId::Containers` (Task 10) show the pattern for Packages and Configs but leave other sections as comments. When implementing the Services arm:

- **Decision items:** `decisions().service_states` (Vec<RefinedServiceState>) + `decisions().service_dropins` (Vec<RefinedDropIn>) — both togglable via Space.
- **Reference items:** ALL 8 sub-collections from `reference().services` (see Implementation Notes § Services composite table) — read-only. The web adapter `web_services_section()` in `inspectah-web/src/adapter.rs:207` is the rendering truth baseline. Read it before implementing the TUI equivalent.

When implementing the Containers arm:
- **Decision items:** `decisions().quadlets` + `decisions().flatpaks` — togglable. Use `containers:` prefix for `mark_viewed()`, NOT `quadlets:` or `flatpaks:`.
- **Reference items:** `reference().containers.running_containers` + `reference().containers.compose_files` — read-only. The web adapter `web_containers_section()` at `adapter.rs:481` is the truth baseline.

Verify: the TUI's item count for each composite section matches the web UI's count for the same tarball.

### 2. CLI `--fresh` sidecar deletion

Task 22's `resolve_tui_session` handles `--fresh` by calling `from_tarball()` without deleting the existing sidecar. The CLI's `refine.rs` does `std::fs::remove_file(&session_path)` at lines 63 and 75 before loading a fresh session. Add the same `remove_file` call to Task 22's fresh branch:

```rust
if fresh {
    let session_path = inspectah_refine::autosave::session_file_path(tarball);
    let _ = std::fs::remove_file(&session_path);
    let mut session = inspectah_refine::tarball::from_tarball(tarball)?;
    session.set_tarball_path(tarball.to_path_buf());
    return Ok(session);
}
```

Without this, a `--fresh` launch leaves the old sidecar intact and the next normal launch resumes from it.

### 3. Checkpoint timing vs. feature readiness

Checkpoint 2 claims "data rendering correctness" but the composite section arms may not be fully wired yet (they depend on Task 11 filling in the match arms). Checkpoint 4 claims export verification but export is in Task 20 (Phase 5). Tang should:

- At checkpoint 2: verify at least Packages (with repo bar), Configs, and one composite section (Services or Containers) render correctly with real data. Other sections may show empty lists — that's acceptable at checkpoint 2 if documented.
- At checkpoint 4: verify `:export` works because Task 20 (export safety) should be complete before checkpoint 4's overlays review. If not, note the gap for Thorn.

### 4. Repo bar, search identity, and user row identity

**Repo bar:** The plan documents the repo bar in Implementation Notes but Task 10's `build_list_items` Packages arm doesn't include it. Tang should add a repo bar rendering step at the top of the Packages item list, consuming `decisions().repo_groups`. The web's `RepoBar.tsx` shows repo names as horizontal chips with package counts. Repo toggles use `RefinementOp::SetInclude { item_id: ItemId::Repo { path: section_id }, include }`.

**Search result identity:** When the user presses Enter on a search result, the TUI must navigate to the correct section AND cursor position. This requires the search result to carry enough identity to locate the item in the section's `ListItem` list. The `SearchResult.name` field must match the `ListItem.name` produced by `build_list_items` for that section.

**User row identity:** The `UserEntry` struct uses `username` as the identity key for `RefinementOp::UserStrategy` and `UserPasswordOp`. The implementer must verify that `username` matches the field name in the projected user JSON (`"name"` field in `UserGroupSection.users`). Check `test_snapshot_with_user()` in `session.rs` for the canonical field name.

### 5. Reviewed-progress denominator for composites

For composite sections (Services, Containers), the reviewed-progress denominator should count only decision items (togglable), not reference items (read-only). The status bar shows `"N/M reviewed"` where M is the count of items the operator is expected to review — reference context items are informational and don't need explicit review.

In `build_section_entries`, the `count` field includes both decision and reference items (for the sidebar total). A separate `decision_count` field (or computed from `included + excluded`) should be used for the reviewed-progress denominator. Tang should add this field to `SectionEntry` if needed.
