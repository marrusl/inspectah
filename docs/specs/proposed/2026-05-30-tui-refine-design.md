# inspectah-tui: Terminal Refine Interface

## Purpose

inspectah-tui is a terminal interface for the refine workflow — triage
packages, configs, services, and other system artifacts for RHEL image mode
migration. It provides full functional parity with the single-host web UI
through a keyboard-driven interface that works over SSH without port
forwarding.

The primary use case is direct-on-host triage: SSH into a scanned system,
run `inspectah tui`, and triage without setting up port forwarding for the
web UI. It must work at 80×24 (minimal SSH session) but take advantage of
wider terminals when available.

## Scope

### In scope (v1)

- Single-host triage: all section types (packages, repos, configs, services,
  containers, users/groups, kernel/boot, network, SELinux, storage, scheduled
  tasks)
- Include/exclude toggling with undo/redo
- Containerfile preview (toggle view)
- Tarball export via `:export`
- Fuzzy search and filter (`/`)
- Command mode (`:`)
- Session autosave (reuses `inspectah-refine` autosave)
- Users section with strategy/password management
- Type-aware item detail (compact info bar for metadata, fullscreen for
  content items)
- Single dark theme with NO_COLOR/16-color/truecolor tiers

### Out of scope (v2+)

- Fleet mode (multi-host comparison, prevalence zones, variant diffs).
  The architecture uses enum dispatch with a Screen variant per mode —
  adding a Fleet variant reuses the widget library without rework, but
  fleet UI is a separate design problem.
- Theming configuration / custom palettes
- Mouse interaction (keyboard-only for v1)
- Keybinding customization

## Architecture

### Crate structure

New `inspectah-tui` crate added to the workspace. No HTTP layer — the TUI
consumes `inspectah-refine` directly.

**Dependencies:**
- `inspectah-refine` — session state, undo/redo, classify, export
- `inspectah-core` — shared types
- `ratatui` — terminal UI framework
- `crossterm` — terminal backend
- `color-eyre` — panic safety and terminal restoration
- `tui-input` — text input widget for search/command line
- `ratatui-macros` — `line![]`/`span![]` layout helpers
- `signal-hook` — SIGTSTP/SIGCONT handling (crossterm only handles SIGWINCH)

**Entry point:** `inspectah tui` subcommand added to `inspectah-cli`. The CLI
loads the snapshot tarball, constructs a `RefineSession`, and calls
`run_tui(session)`.

### Three-layer architecture

```
inspectah-tui/
  src/
    lib.rs                # public API: run_tui(session: RefineSession)
    app.rs                # event loop, terminal lifecycle, state bridge
    action.rs             # Action enum (Quit, ToggleItem, Navigate, etc.)
    event.rs              # crossterm event polling thread + mpsc channel
    screen/
      mod.rs              # Screen enum (SingleHost, later Fleet)
      single_host.rs      # two-panel layout, focus management
    widget/
      mod.rs
      triage_list.rs      # grouped item list with collapse/expand
      section_nav.rs      # sidebar with section counts
      info_bar.rs         # compact 2-row detail for metadata items
      detail_view.rs      # fullscreen detail for content items
      containerfile.rs    # containerfile preview panel
      status_bar.rs       # stats + delta hint
      help_screen.rs      # ? keybinding reference
      search.rs           # / fuzzy filter overlay
      command_line.rs     # : command mode
      user_strategy.rs    # users section interactive view
    theme.rs              # semantic color tokens, NO_COLOR detection
    keys.rs               # keymap definition, event → action mapping
    types.rs              # TUI-specific types (FocusTarget, DetailMode, etc.)
```

### Data flow

1. `app.rs` owns `RefineSession` and `TuiState` (focus target, active
   section, detail mode, search query, cursor position).
2. `event.rs` runs crossterm event polling in a dedicated thread, sends
   events to `app.rs` via `mpsc::channel`. Uses
   `crossterm::event::poll(Duration::from_millis(250))` then `read()` on hit.
3. On each event, `app.rs` maps key → `Action` via `keys.rs`, then either
   mutates `TuiState` or calls `RefineSession` methods (apply op, undo, redo,
   export). Mutations set a dirty flag.
4. On each frame, `app.rs` calls `RefineSession::project()` only if dirty,
   caching the `RefinedView`. Passes cached view + `TuiState` to the active
   screen.
5. Screen composes widgets. Each widget receives the data slice it needs and
   renders to a `ratatui::Frame`. Widgets are stateless renderers.

### Screen enum (fleet v2 seam)

Enum dispatch, not trait objects. Two variants known at compile time — `dyn`
dispatch buys nothing and costs object safety constraints.

```rust
enum Screen {
    SingleHost(SingleHostScreen),
    // Fleet(FleetScreen), // v2
}

impl Screen {
    fn handle_key(&mut self, key: KeyEvent, session: &mut RefineSession,
                  state: &mut TuiState) -> Action { ... }
    fn render(&self, frame: &mut Frame, view: &RefinedView, state: &TuiState) { ... }
}
```

Note: `handle_key` receives `&mut TuiState` so the screen can read focus
state when deciding how to handle a key.

### Action enum

All mutations flow through a centralized `Action` enum. `handle_key` returns
an `Action`; `app.rs` applies it. This keeps mutation logic in one place.

```rust
enum Action {
    Quit,
    NavigateSection(usize),
    ToggleItem(ItemId),
    Undo,
    Redo,
    OpenDetail,
    CloseDetail,
    FullscreenDetail,
    ToggleContainerfile,
    Search(String),
    ClearSearch,
    Export(Option<PathBuf>),
    Refresh,
    Resize(u16, u16),
    None,
}
```

### View caching

`RefineSession::project()` walks the full session state. Calling it every
frame wastes CPU, especially during resize/scroll events that don't change
data. Cache the `RefinedView` and reproject only when the dirty flag is set
(after any `RefineSession` mutation).

```rust
struct App {
    session: RefineSession,
    state: TuiState,
    view_cache: Option<RefinedView>,
    dirty: bool,
}

fn view(&mut self) -> &RefinedView {
    if self.dirty || self.view_cache.is_none() {
        self.view_cache = Some(self.session.project());
        self.dirty = false;
    }
    self.view_cache.as_ref().unwrap()
}
```

### Terminal initialization order

1. `color_eyre::install()` — must happen BEFORE entering alt screen. If a
   panic occurs after alt screen but before the cleanup hook, the terminal
   would be bricked.
2. Create a terminal guard struct with `Drop` that restores raw mode, leaves
   alt screen, and shows cursor.
3. `crossterm::terminal::enable_raw_mode()`
4. Enter alternate screen, hide cursor.
5. Register `signal-hook` handlers for SIGTSTP/SIGCONT.

## Layout & Panels

### Default view: two-panel

```
┌─ Section Nav ───┬─ Item List ──────────────────────────────────┐
│ 1 Packages  142 │ ▼ Investigate (12)                           │
│ 2 Repos       8 │   ▸ mystery-pkg    1.0.0  (none)  [inv]     │
│ 3 Configs    47 │   ...                                        │
│ 4 Services   23 │ ▼ Site (130)                                 │
│ 5 Containers  3 │   ● httpd          2.4.62 baseos  [site]    │
│ 6 Users       5 │   ...                                        │
│ 7 Kernel      2 │ ▶ Baseline (176) ── already in base image   │
│ 8 Network     4 │                                              │
│ 9 SELinux     1 │                                              │
│   Storage     2 │                                              │
│   Scheduled   3 │                                              │
│                 │                                              │
├─ Stats ─────────┤                                              │
│ 142 incl        │                                              │
│ 176 excl        │                                              │
│ 12 review       │                                              │
└─────────────────┴──────────────────────────────────────────────┘
 142 incl · 176 excl · 12 review · Containerfile: 3Δ
```

- **Sidebar** (fixed 18 chars): section names with item counts, numbered 1-9.
  Active section highlighted. Stats summary below.
- **Item list** (remaining width): grouped by triage bucket (investigate →
  site → baseline). Baseline collapsed by default with header "already in
  base image." Columns adapt to width — version and source truncate first.
- **Status bar** (bottom row): included/excluded/review counts, containerfile
  delta hint ("Containerfile: 3Δ"), active search filter.
- **Footer hints** (bottom row, shared with status bar): 4-5 keybinding
  hints. Full list behind `?`.

### Containerfile toggle (`c`)

Replaces two-panel view with a side-by-side split: items left, containerfile
right. Sidebar hides to give both panels room. `c` or `Esc` returns to
default view.

### Item grouping

Items within each section are grouped by triage bucket:

1. **Investigate** (expanded) — items needing human review
2. **Site** (expanded) — items classified as site-specific additions
3. **Baseline** (collapsed) — items matching the base image, header reads
   "already in base image"

`{`/`}` jumps between group headers. `Enter` on a group header
expands/collapses it.

### Detail views (type-aware)

`Enter` behaves differently based on item type:

**Compact info bar** (2-3 rows at bottom, list barely shrinks) for metadata
items:
- Packages — triage reason, repo, version (already visible in row)
- Repos — provenance, tier, package count
- Services — state change, owning package
- Flatpaks — app ID, remote
- Kernel modules, tuned profiles, SELinux port labels, NM connections,
  fstab entries

**Fullscreen detail** (takes over entire screen) for content items:
- Configs — diff against RPM default
- Quadlets — unit file content
- Compose files — compose YAML
- Drop-ins — override content
- Firewall zones — zone XML (custom zones have rich rules)
- Sysctl overrides — value comparison
- Cron jobs — when script content is present

The classification rule: if the item has inspectable text content (file body,
diff, unit definition), fullscreen. If it's metadata, compact info bar.

`f` from a compact info bar promotes to fullscreen. `n`/`p` steps through
items in fullscreen. `Space` toggles include/exclude from within any detail
view. `Esc` returns to the list at the same cursor position.

### Users section

Fullscreen interactive view with per-user strategy selection (skip/useradd)
and password handling. Replaces the standard triage list when the Users
section is active.

### Minimum terminal size

80×24. Below that, render a clear "terminal too small" message. The sidebar
is designed for 18 chars; the item list gets the remainder (~60 chars at 80
cols, enough for name + version + repo + triage tag).

## Interaction Model

### Philosophy

Hybrid modeless. No vim-style mode switching for basic operations. The triage
loop (scan → toggle → move on) is zero-friction. Search and commands are
overlays that dismiss on completion.

### Keybindings

| Key | Action |
|---|---|
| `j/k` or `↑/↓` | Move cursor in list |
| `h/l` or `←/→` | Switch sidebar ↔ items focus |
| `Space` | Toggle include/exclude on focused item |
| `Enter` | Open detail (compact or fullscreen, type-aware) |
| `Esc` | Close detail / cancel search / back |
| `n/p` | Next/prev item (in fullscreen detail) |
| `f` | Promote compact info bar to fullscreen |
| `u` | Undo |
| `Ctrl+r` | Redo |
| `c` | Toggle containerfile preview |
| `/` | Fuzzy search/filter |
| `:` | Command mode |
| `?` | Help screen |
| `Tab` | Cycle focus (sidebar → items → detail pane) |
| `1-9` | Jump to section by number |
| `{/}` | Jump to prev/next triage group |
| `g/G` | Top/bottom of list |
| `r` | Refresh data |
| `q` | Quit |

### Search (`/`)

Overlay at top of item list. Fuzzy filter within the active section. Results
narrow in real time with count shown (`3/142 matches`). `Enter` jumps to
selected match. `Esc` clears filter and restores full list.

### Command mode (`:`)

Command line at bottom. Available commands:

- `:export [path]` — export tarball, prints path on completion
- `:section <name>` — jump to section by name
- `:stats` — show session statistics (per-section counts, review items,
  operations applied, baseline status, session metadata)
- `:undo` / `:redo` — alternative to `u` / `Ctrl+r`

Tab-completion on command names and section names.

### Undo/redo

`u` undoes last operation, `Ctrl+r` redoes. Maps directly to
`RefineSession`'s undo/redo stack. Status bar briefly flashes the
undone/redone operation description.

### Focus management

`Tab` cycles: sidebar → item list → detail pane (when open). `h/l` switches
directly between sidebar and item list. Active focus indicated by border
color change.

### Autosave

Inherits `inspectah-refine`'s existing autosave. Operations persisted
automatically. Session resumes where it left off if TUI is restarted against
the same snapshot.

## Color & Terminal Compatibility

### Semantic color tokens

Tokens defined as an enum. Each token resolves to a `ratatui::Style` based
on the detected color tier. Exhaustive matching catches missing tokens at
compile time.

```rust
enum Token {
    TriageInvestigate, TriageSite, TriageBaseline,
    TextPrimary, TextMuted,
    DiffAdded, DiffRemoved,
    StatusIncluded, StatusExcluded,
    FocusBorder, FocusUnfocused, FocusSelected,
    SearchMatch,
    Warning, Error,
}

enum ColorTier { Mono, Ansi16, TrueColor }

impl Token {
    fn style(self, tier: ColorTier) -> Style { ... }
}
```

| Token | Purpose | 16-color | Mono fallback |
|---|---|---|---|
| `TriageInvestigate` | Investigate items | Red | Bold |
| `TriageSite` | Site items | Yellow | Normal |
| `TriageBaseline` | Baseline items | Green | Dim |
| `TextPrimary` | Default text | Default fg | Default |
| `TextMuted` | Metadata, secondary | Dark gray | Dim |
| `DiffAdded` | Diff insertions | Green | `+` prefix |
| `DiffRemoved` | Diff deletions | Red | `-` prefix |
| `StatusIncluded` | Include indicator | Green | `●` |
| `StatusExcluded` | Exclude indicator | Dim | `○` |
| `FocusBorder` | Focused panel border | Cyan | Bold border |
| `FocusUnfocused` | Unfocused border | Dim | Normal border |
| `FocusSelected` | Cursor / selection row | Reverse | Reverse + bold |
| `SearchMatch` | Search highlight | Reverse | Reverse |
| `Warning` | Sensitive paths, flags | Yellow bold | `⚠` + bold |
| `Error` | Hard failures | Red bold | `✗` + bold |

### Color tiers

1. **`NO_COLOR` / monochrome** — bold, dim, reverse video carry all meaning.
   Fully usable. If a terminal renders dim poorly, baseline items use
   underline as a fallback differentiator from `TextMuted`.
2. **16 ANSI** (default) — respects user's terminal theme.
3. **256/truecolor** — detected via `$COLORTERM`. v1 ships one dark palette.
   For truecolor, investigate shifts toward orange-red and baseline toward
   blue-green to improve CVD (color vision deficiency) accessibility.

Detection is manual (~15 lines):

```rust
fn detect_tier() -> ColorTier {
    if std::env::var_os("NO_COLOR").is_some() { return ColorTier::Mono; }
    match std::env::var("COLORTERM").as_deref() {
        Ok("truecolor" | "24bit") => ColorTier::TrueColor,
        _ => ColorTier::Ansi16,
    }
}
```

### Non-color signals

Every semantic meaning is paired with a non-color signal:

- Triage bucket: `▸` investigate, `●` site, `○` baseline + text tags
  (`[inv]`, `[site]`, `[base]`)
- Include/exclude: `●` / `○`
- Content available: `▸` on items with inspectable content
- Collapsed/expanded: `▶` / `▼`
- Diff: `+` / `-` prefixes
- Warning: `⚠` symbol
- Error: `✗` symbol

### Terminal hygiene

- **Alternate screen** for the full TUI. Shell scrollback untouched on exit.
- **Panic handler** via `color-eyre`: restores terminal state (raw mode off,
  alt screen exit, cursor visible) before printing trace. Installed before
  terminal init.
- **Terminal guard** struct with `Drop` impl for cleanup on all exit paths.
- **SIGWINCH**: handled by crossterm. Re-layout on resize, debounced. Below
  80×24: "terminal too small" message.
- **SIGTSTP** (`Ctrl+Z`): via `signal-hook`. Leave alt screen, disable raw
  mode, suspend. On `SIGCONT`: re-enter alt screen, force full redraw.
- **Event polling**: dedicated thread with `mpsc::channel`. Render only on
  events — no fixed timer burning CPU at idle.
- **No UI-thread blocking**: all operations against `RefineSession` are
  synchronous and fast (in-memory data, no I/O). Export is the only
  potentially slow operation; it writes to disk but is user-initiated and
  blocking is acceptable for the duration.

## Testing Strategy

### Unit tests (widget rendering)

Each widget gets snapshot tests via `insta`. Render to `ratatui::Buffer` with
a fixed size, snapshot the output.

- Test at multiple widths: 80, 120, 200 columns. Use
  `insta::Settings::set_snapshot_suffix` for width-parameterized tests to
  avoid filename collisions.
- Theme tier tests: verify monochrome mode produces usable output (symbols
  and text tags present, no color-only signals).
- Snapshot review policy: snapshots reviewed in CI. Large snapshot update PRs
  get manual review to distinguish intentional changes from regressions.

### Integration tests (key → state → render)

- Construct `RefineSession` from test fixture tarballs (reuse
  `inspectah-refine`'s existing fixtures).
- Simulate key sequences: navigate to item, Space to toggle, verify
  `TuiState` reflects the change (cursor position, detail mode), verify
  widget renders the new state.
- Undo/redo: apply ops, undo, verify TUI state (cursor, visible items) —
  not re-verify that `RefinedView` rollback is correct (that's
  `inspectah-refine`'s job).
- Search: simulate `/httpd`, verify filter narrows list, Enter selects,
  Esc clears.

### Detail mode classification

The compact-vs-fullscreen decision is a pure function of section type. Test
the `section_type → DetailMode` mapping as a standalone unit, separate from
rendering. Then snapshot each `DetailMode` variant once.

### Section coverage

Each section type (packages, configs, services, containers, users, kernel,
network, SELinux, storage, scheduled) gets at least one test verifying its
triage list renders correctly and Enter opens the right detail mode.

### Focus and resize

- **Focus persistence across resize**: select item N, trigger resize, assert
  item N still focused and selected.
- **Minimum size enforcement**: resize below 80×24, verify "terminal too
  small" message renders.

### Error paths

- Verify user-facing error display when `RefineSession` construction fails
  (corrupt tarball, missing manifest, permission error). Verify terminal
  state is properly restored.

### Key conflict / unhandled input

- Verify keys valid in one context but meaningless in another (Space in
  fullscreen detail, `/` during active search) are handled gracefully —
  either no-op or sensible behavior, never crashes or silent state
  corruption.

### Mouse events

- Verify mouse events do not panic or corrupt state, even though mouse
  interaction is not supported in v1.

### Test boundary

The TUI crate tests rendering, dispatch, and state transitions. It does NOT
test triage logic, classification correctness, undo/redo data integrity,
containerfile rendering, or export format — those are `inspectah-refine`'s
responsibility and are covered by its 300+ test suite.

## Decisions Log

Key decisions made during brainstorming, with rationale:

1. **Single-host v1, fleet v2.** The TUI's killer use case ("SSH in, triage
   on the box") is single-host by definition. Fleet is an analytical workflow
   that reshapes the information architecture. Ship single-host fast, let
   usage signal whether fleet-in-terminal is needed. (Tang, Ember)

2. **Hybrid modeless interaction.** The triage loop is fast and repetitive —
   modal switching adds friction. Search and commands are overlays. (Kiwi)

3. **Layout B: two-panel + containerfile toggle.** Items need maximum width
   for the decision context. The containerfile is a verification artifact,
   not continuous reference — one keypress away via `c`. Adaptive panel count
   (layout C) violates the Bloomberg principle of spatial stability. (Fern,
   Ember)

4. **Grouped by triage bucket, baseline collapsed.** On real RHEL hosts,
   baseline packages are 80%+ of items. Collapsing them front-loads the items
   that need decisions. Header reads "already in base image" (not "excluded,"
   which confuses the user perspective). (Mark)

5. **Type-aware detail depth.** Enter on metadata items shows a compact info
   bar; Enter on content items goes fullscreen. One interaction, type-driven
   depth. Packages get a tooltip, configs get a workspace. (Ember)

6. **Enum dispatch over trait objects.** Two screen variants known at compile
   time. Exhaustive matching, no dynamic dispatch overhead. (Tang)

7. **View caching with dirty flag.** `project()` is expensive. Cache the
   `RefinedView`, reproject only after mutations. (Tang)

8. **color-eyre before alt screen.** Panic handler must be installed before
   terminal state changes, or a panic between alt screen entry and handler
   registration bricks the terminal. (Tang)

9. **CVD-safe truecolor palette.** Shift investigate toward orange-red and
   baseline toward blue-green for deuteranopia safety. 16-color tier relies
   on text tags and symbols. (Fern)

10. **signal-hook for SIGTSTP.** crossterm handles SIGWINCH but not suspend/
    resume signals. (Tang)
