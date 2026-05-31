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
4. On each frame, `app.rs` reads `session.view()` (recomputed after
   mutation), `session.decisions()` (recomputed if dirty), and
   `session.reference()` (OnceLock, computed once). Passes all three +
   `TuiState` to the active screen. The item list renders from `view()`;
   decision-specific state (sensitivity, baseline summary) comes from
   `decisions()`; reference sections render from `reference()`.
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
    fn render(&self, frame: &mut Frame,
              view: &RefinedView,
              decisions: &DecisionProjection,
              reference: &ReferenceProjection,
              state: &TuiState) { ... }
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

### Session data model

`RefineSession` exposes three cache layers with different invalidation
strategies. The TUI reads all three — it does NOT maintain its own view
cache or dirty flag.

- **`session.view()`** returns `&RefinedView`. The primary data model:
  classified packages, configs, containerfile preview, and `RefineStats`
  (per-section counts, review count, undo/redo state). Internally cached
  via `Option<RefinedView>`; invalidated on every mutation (apply/undo/redo)
  and recomputed immediately via `recompute_view()`. The TUI renders the
  item list from `view()`.
- **`session.decisions()`** returns `&DecisionProjection`. Structured
  decision data: service states, drop-ins, quadlets, flatpaks, sysctls,
  tuned profiles, repos, version changes, users/groups, `is_sensitive`,
  and `baseline_summary`. Internally cached via `Option<DecisionProjection>`;
  invalidated on every mutation and recomputed on next access. The TUI uses
  `decisions()` for mutation-specific state that `view()` does not carry
  (e.g., `is_sensitive` for export gating, `baseline_summary` for the
  baseline header).
- **`session.reference()`** returns `&ReferenceProjection`. Immutable
  reference context: services (divergent, advisories, warnings, omitted),
  version changes (downgrades, upgrades), containers (quadlets, compose,
  running, flatpaks), kernel/boot, network, storage, scheduled tasks,
  non-RPM software, SELinux. Backed by `OnceLock` — computed once on first
  access, never invalidated. The TUI uses `reference()` for read-only
  context sections.

```rust
struct App {
    session: RefineSession,
    state: TuiState,
}

// Item list data — packages, configs, stats, containerfile preview
fn view(&self) -> &RefinedView {
    self.session.view()
}

// Decision-specific data — sensitivity, baseline summary, typed decision items
fn decisions(&self) -> &DecisionProjection {
    self.session.decisions()
}

// Reference data — immutable context sections, computed once
fn reference(&self) -> &ReferenceProjection {
    self.session.reference()
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
 142 incl · 176 excl · 12 review · 47/142 reviewed · Containerfile: 3Δ
```

- **Sidebar** (fixed 18 chars): section names with item counts, numbered 1-9.
  Active section highlighted. Decision sections show triage counts; reference
  sections show item counts with a `ref` badge. Stats summary below.
- **Item list** (remaining width): content depends on section type:
  - **Decision sections** — grouped by triage bucket (investigate → site →
    baseline). Baseline collapsed by default with header "already in base
    image." Columns adapt to width — version and source truncate first.
    Space toggles include/exclude.
  - **Reference sections** — flat or sub-grouped list of read-only context
    items. No triage buckets, no include/exclude toggling. Items display
    their typed data (e.g., service state, connection type, zone rules).
    Space is a no-op. The section header reads "Reference — read-only context"
    to set operator expectations.
- **Status bar** (bottom row): included/excluded/review counts (decision
  sections only), containerfile delta hint ("Containerfile: 3Δ"), active
  search filter, reviewed progress ("47/142 reviewed"), autosave status
  (warning indicator when degraded).
- **Footer hints** (bottom row, shared with status bar): 4-5 keybinding
  hints. Hints adapt to section type — reference sections suppress
  Space/toggle hints. Full list behind `?`.

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

### Section type mapping

The shipped projection model splits sections into two categories. The TUI
must respect this boundary — it is not a UI choice, it is a data contract.

**Decision sections** (from `DecisionProjection` — mutable, togglable):
- Packages (version_changes + baseline_summary)
- Repos (repo_groups)
- Configs (service_dropins + quadlets)
- Services (service_states)
- Sysctls (sysctls)
- Tuned profiles (tuned)
- Users/groups (users_groups)

**Reference sections** (from `ReferenceProjection` — immutable, read-only):
- Services context (services: divergent, advisories, warnings, omitted)
- Version changes context (version_changes: downgrades, upgrades)
- Kernel/boot (kernel_boot: cmdline, modules, dracut, alternatives)
- Network (network: connections, firewall, routes, proxy)
- Storage (storage)
- Scheduled tasks (scheduled_tasks)
- Non-RPM software (non_rpm_software)
- SELinux (selinux)

**Composite section — Containers:** Container items span both projections:
- **Decision items** from `DecisionProjection`: `quadlets`
  (toggleable quadlet units) and `flatpaks` (toggleable flatpak apps)
- **Reference items** from `ReferenceProjection`: `containers.quadlets`
  (read-only quadlet context), `containers.compose_files`,
  `containers.running_containers`, `containers.flatpaks` (read-only
  flatpak context)

The sidebar shows a single "Containers" entry. When the operator
navigates to it, the item list renders both decision items (with
include/exclude toggling via Space) and reference items (read-only, Space
is a no-op) in a grouped layout. Decision items appear first under a
"Triage" subheader; reference items follow under a "Context" subheader.
This prevents the discoverability problem of scattering container-related
items across distant sidebar entries.

The sidebar groups sections visually. Decision sections appear first
(numbered 1-N), followed by a separator and reference sections.
Containers appears as the last decision section (since it contains
toggleable items) with its reference context inline. This matches the
operator's mental model: "things I need to decide" above "things I need
to understand."

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
items in fullscreen. In decision sections, `Space` toggles include/exclude
from within any detail view. In reference sections, `Space` is a no-op
(detail views are read-only context). `Esc` returns to the list at the same
cursor position.

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
| `Space` | Toggle include/exclude (decision sections only; no-op in reference) |
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

Overlay at top of item list. Cross-section fuzzy search across all sections
(both decision and reference). Results narrow in real time with count and
section attribution shown (`3 matches — Packages(2), Network(1)`). Each
result row shows the section name, item name, and match context. `j/k`
navigates the result list. `Enter` navigates to the matched item in its
section (switching the active section if needed). `Esc` clears the search
and restores the previous section and cursor position.

This preserves the web UI's global discovery model — the operator can find
an item without knowing which section it belongs to.

### Identity contracts

The TUI's progress tracking, search, and sidebar counts are grounded in
concrete session APIs. This section pins those contracts.

**Reviewed item identity:** The session tracks reviewed items in a
`HashSet<String>` (`viewed` field on `RefineSession`). The key format is
`"section:item_id"` — e.g., `"packages:httpd.x86_64"`,
`"configs:/etc/sysctl.d/99-custom.conf"`. The TUI calls
`session.mark_viewed(key)` when the operator opens detail view on an
item, and reads `session.viewed_ids()` to get the full set. The reviewed
count for a section is `viewed_ids().iter().filter(|k| k.starts_with(section_prefix)).count()`.

**Search fields:** Cross-section search iterates the typed fields of
each projection. For decision items: package name + arch, repo path,
config path, service unit name, quadlet name, flatpak app ID, sysctl
key, tuned profile name, username. For reference items: the typed struct
fields (service unit, connection name, firewall zone name, compose path,
container name/image, module name, etc.). The TUI builds a searchable
index by extracting string fields from the projection structs — no
intermediate `searchable_text` field.

**Sidebar counts:** Section counts in the sidebar come from
`view().stats` (`RefineStats`). `RefineStats.sections` is a
`Vec<SectionStats>`, where each `SectionStats` has `kind: SectionKind`,
`total`, `included`, and `excluded`. The sidebar renders `total` as the
count for each decision section. Reference section counts come from the
length of the corresponding `Vec` in `ReferenceProjection` (e.g.,
`reference().containers.running_containers.len()`). The status bar's
`needs_review_count` comes from `view().stats.needs_review_count`.

### Command mode (`:`)

Command line at bottom. Available commands:

- `:export [path]` — export tarball. If `session.is_sensitive()` returns
  true, the TUI presents an interactive confirmation prompt before
  proceeding (see Export safety below). Prints path on completion
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

Inherits `inspectah-refine`'s existing autosave. `try_autosave()` is called
on every mutation (apply, undo, redo). It computes the tarball hash via
`compute_tarball_hash()` and writes the session sidecar via
`save_session()`. Session resumes where it left off if TUI is restarted
against the same snapshot.

#### Autosave degradation

`try_autosave()` handles two failure modes. The TUI must surface both in
the status bar — stderr-only logging (the web server's approach) is not
sufficient for an interactive terminal.

**Transient failure** (e.g., temporary disk full, NFS hiccup): the session
logs `"autosave: transient failure — {e}"` to stderr and retries on the
next mutation. The status bar shows a brief warning flash:
`⚠ autosave: retry pending`. The warning clears on the next successful
save. No operator action required.

**Permanent degradation** (EROFS, EACCES — read-only filesystem or
permission denied): the session sets `durability_degraded = true` and
stops attempting further saves. The status bar shows a persistent warning:
`⚠ autosave: disabled (read-only)`. This indicator remains for the rest
of the session.

**Export while degraded:** Export is always available regardless of
autosave state. The operator can still run `:export` to produce a tarball.
Autosave durability affects session resume, not export. The export safety
prompt (sensitive data acknowledgment) applies independently.

**Recovery:** There is no in-TUI recovery action for permanent
degradation. The operator must fix the underlying filesystem issue
(remount read-write, fix permissions) and restart the TUI. On restart,
`resume_from()` will attempt to load the last successful sidecar. If
autosave degraded before any save succeeded, the session starts fresh.

### Startup and session resume

`inspectah tui <tarball>` enters the refine workflow. The TUI must handle
all branches of `RefineSession::resume_from()` and
`RefineSession::new_with_tarball()`.

**Branch 1 — Fresh session (no sidecar exists):**
`resume_from()` returns `Ok(None)`. The TUI calls
`new_with_tarball(snapshot, tarball)` to create a fresh session. The
status bar shows the section summary immediately. No message to the
operator — this is the default path.

**Branch 2 — Resume (sidecar exists, tarball hash matches):**
`resume_from()` returns `Ok(Some(session))` with all saved ops restored
and the redo tail preserved. The status bar shows a brief flash:
`Resumed session (N ops)` for 3 seconds, where N is `session.cursor()`.
The operator picks up where they left off.

**Branch 3 — Stale sidecar (sidecar exists, tarball hash mismatch):**
`resume_from()` returns `Err(RefineError::StaleTarball { saved_hash,
current_hash })`. The tarball has been re-scanned since the last session.
The TUI discards the stale sidecar (it is not automatically deleted — the
next autosave will overwrite it), creates a fresh session via
`new_with_tarball()`, and shows a status bar flash: `Stale session
discarded — tarball has changed`. The operator starts fresh.

**Branch 4 — Corrupt or unloadable sidecar:**
`resume_from()` returns `Err(RefineError::SnapshotLoad(...))`. The
session file exists but is malformed, has an unknown schema version, or
the tarball itself cannot be loaded. The TUI prints a user-facing error
message to the terminal (not alt screen), restores terminal state, and
exits with a non-zero status code. The error message includes the
specific failure reason from the `RefineError` variant.

**Branch 5 — Tarball load failure (no session file involved):**
If the tarball path does not exist or cannot be parsed as a valid
snapshot, `InspectionSnapshot::from_tarball()` fails before
`resume_from()` is ever called. Same behavior as Branch 4: error message,
clean terminal restore, non-zero exit.

### Export safety

The web handler gates export behind an `x-ack-sensitive` HTTP header when
`session.is_sensitive()` is true. The TUI must enforce the same contract
through an interactive prompt.

When the operator runs `:export` and the session is sensitive:

1. The command line area expands to a 3-row confirmation block:
   ```
   ⚠ This session contains sensitive data (passwords, keys, or secrets).
     Exported artifacts will include this data in plain text.
     Proceed? [y/N]
   ```
2. Only `y` or `Y` proceeds. Any other key (including Enter alone) cancels
   with "Export cancelled."
3. Non-sensitive sessions export immediately with no prompt.

The confirmation text mirrors `build_sensitivity_summary()` from the web
handler — it explains *why* the session is sensitive, not just that it is.
If the session has multiple sensitivity reasons (e.g., snapshot contains
sensitive data AND user passwords detected), the prompt lists all reasons.

This is the TUI equivalent of the web's `x-ack-sensitive` header. The
contract is: no sensitive data leaves the session without explicit operator
acknowledgment.

### Reviewed progress

The TUI tracks which decision items the operator has actually reviewed.
This gives progress feedback during triage without requiring an explicit
"mark as reviewed" action.

**Tracking rule:** An item is marked "reviewed" when the operator opens
its detail view (`Enter`). Cursor-scrolling past an item does NOT count
as reviewed — the operator must open the item to inspect it. This
matches the session's existing `viewed` tracking (`viewed: HashSet<String>`
with format `"section:item_id"`, e.g., `"packages:httpd.x86_64"`), which
records items the UI explicitly marks via `mark_viewed()`.

**Display:** The status bar shows a progress counter for the active
decision section: `47/142 reviewed`. The sidebar shows a progress
indicator next to each decision section — a filled bar or fraction.
Reference sections do not track reviewed state.

**Persistence:** Reviewed state is stored in the session's `viewed`
HashSet (in-memory). It resets on session restart. This is intentional —
reviewed state is a convenience for the current triage pass, not a
durable record. The autosave covers mutation state (include/exclude
decisions), not review progress.

**Scope:** Reviewed tracking applies only to decision sections. Reference
sections are informational context — there is no "you should look at all
of these" expectation.

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
- **Minimal UI-thread blocking**: mutations (apply/undo/redo) are
  synchronous and trigger `try_autosave()`, which computes a SHA-256 hash
  of the tarball and writes the session sidecar file. On local SSDs this
  is sub-millisecond and imperceptible. On network-mounted filesystems it
  may be slower, but autosave degrades gracefully (see Autosave
  degradation above). Export is the only potentially slow operation; it
  writes to disk but is user-initiated and blocking is acceptable for the
  duration.

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
  not re-verify that `DecisionProjection` rollback is correct (that's
  `inspectah-refine`'s job).
- Search: simulate `/httpd`, verify cross-section results with section
  attribution, Enter navigates to correct section+item, Esc restores
  previous section and cursor.
- Export safety: simulate `:export` with sensitive session, verify
  confirmation prompt appears. Verify `y` proceeds, `n`/Enter/Esc cancels.
  Verify non-sensitive session exports immediately.
- Reviewed tracking: open detail (Enter) on items, verify reviewed count
  increments. Verify cursor-scrolling past an item does NOT increment the
  reviewed count. Verify reference sections do not track reviewed state.
- Reference sections: verify Space is a no-op in reference sections.
  Verify reference items render without triage bucket grouping.
  Verify keybinding hints suppress toggle in reference context.

### Detail mode classification

The compact-vs-fullscreen decision is a pure function of section type. Test
the `section_type → DetailMode` mapping as a standalone unit, separate from
rendering. Then snapshot each `DetailMode` variant once.

### Section coverage

Each section type gets at least one test verifying its list renders
correctly and Enter opens the right detail mode. Decision sections
(packages, repos, configs, services, sysctls, tuned, users) must verify
triage bucket grouping and Space toggling. Reference sections
(kernel/boot, network, storage, SELinux, scheduled tasks, non-RPM
software) must verify read-only rendering and Space no-op. The
Containers composite section must verify that decision items (quadlets,
flatpaks) support Space toggling while reference items (running
containers, compose files) are read-only.

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
  reference sections, Space in fullscreen detail, `/` during active search)
  are handled gracefully — either no-op or sensible behavior, never crashes
  or silent state corruption.

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

7. **Session-managed three-layer caching.** `session.view()` returns
   `RefinedView` (packages, configs, stats, containerfile preview),
   invalidated on mutation. `session.decisions()` returns
   `DecisionProjection` (typed decision data, sensitivity, baseline
   summary), invalidated on mutation. `session.reference()` returns
   `ReferenceProjection` via OnceLock, computed once. TUI does not
   maintain its own view cache. (Tang, rev2)

8. **color-eyre before alt screen.** Panic handler must be installed before
   terminal state changes, or a panic between alt screen entry and handler
   registration bricks the terminal. (Tang)

9. **CVD-safe truecolor palette.** Shift investigate toward orange-red and
   baseline toward blue-green for deuteranopia safety. 16-color tier relies
   on text tags and symbols. (Fern)

10. **signal-hook for SIGTSTP.** crossterm handles SIGWINCH but not suspend/
    resume signals. (Tang)

11. **Decision vs reference section split.** The TUI respects the
    `DecisionProjection` / `ReferenceProjection` boundary from the shipped
    projection model. Decision sections offer include/exclude toggling;
    reference sections are read-only context. Space is a no-op in reference
    sections. This prevents the "everything is toggleable" confusion flagged
    in review. (Tang, rev1)

12. **Interactive export safety prompt.** Sensitive sessions require an
    explicit `y` confirmation before export, mirroring the web handler's
    `x-ack-sensitive` header contract. The TUI prompt explains *why* the
    session is sensitive. Default is cancel (N). (Tang, rev1)

13. **Cross-section search.** `/` searches all sections (decision and
    reference), showing section attribution per result. This preserves the
    web UI's global discovery model instead of narrowing to section-only
    filtering. (Tang, rev1)

14. **Reviewed progress tracking.** Decision items are marked "reviewed"
    only when the operator opens detail view (Enter). Cursor-scrolling
    does not count. Progress shown in status bar and sidebar. Backed by
    `session.viewed_ids()` HashSet. In-memory only, resets on restart.
    (Tang, rev2 — tightened from rev1 cursor-contact semantics)

15. **Three-layer session model.** `view()` returns `RefinedView` for the
    item list (packages, configs, stats). `decisions()` returns
    `DecisionProjection` for mutation-specific state (sensitivity,
    baseline summary, typed decision items). `reference()` returns
    `ReferenceProjection` for immutable context sections. All three are
    session-managed caches with different invalidation strategies.
    (Tang, rev2)

16. **Autosave degradation surfaces in status bar.** Transient failures
    show a brief warning flash; permanent degradation (EROFS, EACCES)
    shows a persistent indicator. Export remains available regardless.
    No in-TUI recovery — operator fixes the filesystem and restarts.
    (Tang, rev2)

17. **Containers as composite sidebar section.** Decision items
    (quadlets, flatpaks) and reference items (running containers,
    compose files) appear under a single "Containers" sidebar entry
    with "Triage" and "Context" subheaders. Prevents scattering
    container-related items across distant sections. (Tang, rev2)

18. **Explicit startup branches.** Five TUI entry paths mapped to
    `resume_from()` outcomes: fresh (no sidecar), resume (hash match),
    stale (hash mismatch, discard and start fresh), corrupt sidecar
    (error + exit), tarball load failure (error + exit). (Tang, rev2)

## Finding Traceability

Mapping of review findings to their resolutions in this revision.

| # | Finding | Severity | Round | Reviewer(s) | Resolution |
|---|---------|----------|-------|-------------|------------|
| 1 | Export safety missing | High | R1 | Thorn | Added "Export safety" subsection under Interaction Model. Interactive `y/N` confirmation prompt when `session.is_sensitive()` is true. Mirrors web handler's `x-ack-sensitive` contract. Decision 12. |
| 2 | Reviewed-state regression | High | R1 | Fern | Added "Viewed/reviewed progress" subsection. Cursor-contact and detail-open tracking with status bar counter (`47/142 viewed`). In-memory only, decision section scope. Decision 14. |
| 3 | Decision vs reference blur | High | R1 | Fern, Collins | Added "Section type mapping" subsection listing all decision and reference sections. Updated sidebar, item list, keybindings, and detail view descriptions to distinguish behavior. Space no-op in reference sections. Decision 11. |
| 4 | Search regression | Medium | R1 | Fern | Rewrote "Search (`/`)" subsection. Cross-section search with section attribution per result. Enter navigates to matched section+item. Decision 13. |
| 5 | Update projection references | — | R1 | — | Replaced all `RefinedView`/`project()` references with `DecisionProjection`/`ReferenceProjection`/`decisions()`/`reference()`. Rewrote "Projection access" subsection (was "View caching"). Updated `Screen::render` signature. Decision 7 updated. |
| H1 | Core projection contract misstated | High | R2 | Tang, Collins | Restored `RefinedView` as primary data model. Rewrote "Session data model" (was "Projection access") to document all three cache layers: `view()`, `decisions()`, `reference()`. Updated "Data flow" step 4 and `Screen::render` signature. Decision 7 updated to "Three-layer caching." Decision 15. |
| H2 | Silent autosave degradation | High | R2 | Thorn | Added "Autosave degradation" subsection under Autosave. Transient: status bar flash + retry. Permanent: persistent indicator + degraded flag. Export unaffected. No in-TUI recovery. Decision 16. |
| H3 | Viewed progress overstates review | High | R2 | Fern | Changed "viewed" to "reviewed" semantics throughout. Detail-open only, not cursor-scroll. Renamed section to "Reviewed progress." Pinned to `session.viewed_ids()` HashSet. Decision 14 updated. |
| H4 | Container taxonomy splits discoverability | High | R2 | Fern | Created "Containers" composite sidebar section combining decision items (quadlets, flatpaks) and reference items (running containers, compose files) under one entry with Triage/Context subheaders. Decision 17. |
| M1 | Mutation path not non-blocking | Medium | R2 | Tang | Replaced "no I/O" claim in Terminal hygiene with accurate description of autosave I/O (SHA-256 hash + sidecar write, sub-ms on SSDs). Cross-referenced autosave degradation. |
| M2 | Identity contracts loose | Medium | R2 | Collins | Added "Identity contracts" subsection. Pinned reviewed item ID format (`"section:item_id"`), search fields (typed struct fields from projections), sidebar counts (`RefineStats.sections` + projection Vec lengths). |
| M3 | Resume/fresh branches underspecified | Medium | R2 | Thorn | Added "Startup and session resume" subsection with five explicit branches: fresh, resume, stale, corrupt sidecar, tarball load failure. Each mapped to `resume_from()` return variant and operator-visible behavior. Decision 18. |
