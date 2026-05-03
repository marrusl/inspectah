# Editor Section Redesign

**Date:** 2026-05-03
**Revision:** 4 (aligns autosave failure row with shared contract)
**Status:** Proposed
**Scope:** Edit Files section of the inspectah HTML report (Go port, `go-port` branch)
**Depends on:**
- Fern's UX analysis (`docs/specs/proposed/2026-05-03-editor-redesign-ux-analysis.md`)
- HTML report redesign contract (`docs/specs/proposed/2026-04-28-html-report-redesign.md`)

## Problem

The Go port's Edit Files section has four UX problems:

1. **Auto-enters edit mode** — clicking a file immediately opens CodeMirror, no read-only preview
2. **False modification markers** — files marked as "changed" just from viewing them
3. **Flat file list** — configs, drop-ins, and quadlets in one undifferentiated list, making drop-ins and quadlets hard to discover
4. **No unsaved changes protection** — navigating away silently discards edits

## Data Model: Three Baselines

Every editable file tracks three values:

| Baseline | Name | Mutated by | Description |
|----------|------|------------|-------------|
| `originalContent` | Page-load snapshot | Never (immutable for session) | The file content as loaded from the scan tarball |
| `savedContent` | Last saved value | `Save` button, `Ctrl+S`, modal `Save` | The most recent explicitly saved state |
| `bufferContent` | Current editor buffer | User typing in CodeMirror | The live editor content |

At page load: `originalContent === savedContent === bufferContent`.

**Derived states:**

| Condition | Meaning | Indicator |
|-----------|---------|-----------|
| `bufferContent !== savedContent` | **Dirty** — unsaved buffer changes exist | Triggers modal on navigation, enables Save button |
| `savedContent !== originalContent` | **Modified** — file has been changed from original | Blue dot on file item and tab badge |
| `bufferContent === savedContent` | **Clean** — no unsaved changes | No modal on navigation, Save button disabled |

## Design

### Tab Bar

Three tabs inside the file browser panel (not spanning full editor width):

| Tab | Label | Contents |
|-----|-------|----------|
| Config | `Config (N)` | Regular config files under `/etc/` |
| Drop-ins | `Drop-ins (N)` | systemd service drop-in files (e.g., `httpd.service.d/override.conf`) |
| Quadlets | `Quadlets (N)` | Podman quadlet unit files (`.container`, `.network`, `.volume`, etc.) |

**Counts** show total files in that category, not modified count. Counts are static for the session.

**Modified indicator:** A blue dot (info semantic, `var(--pf-t--global--color--status--info--default)`, `#2b9af3` in dark mode, `#0066cc` in light mode) appears on the tab label when any file in that category has been modified (saved with different content from original): `Config (39) ●`. Individual files in the list also show the dot. Blue is chosen because "file has been edited" is informational, not a warning — amber (`#d29922`) is reserved for "needs decision / in progress" in the triage sections.

**Empty tabs:** Always visible with `(0)` count. Panel shows centered message: "No [config files / systemd drop-in files / quadlet unit files] detected."

**Activation model:** Manual — Arrow Left/Right moves focus between tabs, Enter/Space activates. Home/End jump to first/last tab. This avoids triggering the unsaved-changes modal on every Arrow keypress.

**Per-tab persistence:** Each tab remembers its selected file and scroll position. Switching tabs restores the previous state. If the tab has no previous selection, the content pane shows "Select a file to view."

### Content Pane State Machine

Four states:

```
                    [click file]
      Empty ──────────────────────► Read-Only
                                      │
                              [Edit] or [E]
                                      │
                                      ▼
                               Editing (clean)
                                      │
                                  [type]
                                      │
                                      ▼
                               Editing (dirty)
                                   │    │
                             [Save btn] [Ctrl+S]
                                │         │
                                ▼         ▼
                          Read-Only   Editing (clean)

    From Editing (dirty):
      - [Revert] → Read-Only (full reset to originalContent, no modal)
      - [Escape] → modal if dirty → Read-Only or cancel
      - [click file/tab/nav] → modal → proceed or cancel

    From Editing (clean):
      - [Revert] or [Escape] → Read-Only (no modal)
      - [click file/tab/nav] → Read-Only (no modal)
```

| State | Toolbar | Content Pane |
|-------|---------|--------------|
| **Empty** | Hidden | Centered: "Select a file to view" |
| **Read-Only** | File path + `[Edit]` button | `<pre>` block, monospace, no line numbers, no cursor |
| **Editing (clean)** | File path + `[Revert]` + `[Save]` (disabled) | CodeMirror 6, line numbers, active cursor |
| **Editing (dirty)** | File path + "unsaved" badge + `[Revert]` + `[Save]` (enabled) | CodeMirror 6, buffer differs from savedContent |

### Save and Revert Semantics

| Action | What it does | Resulting state |
|--------|-------------|-----------------|
| **`Save` button** | `savedContent = bufferContent`, exit edit mode | Read-Only. If `savedContent !== originalContent`, file shows modified dot. |
| **`Ctrl+S` / `Cmd+S`** | `savedContent = bufferContent`, stay in edit mode | Editing (clean). Checkpoint — user keeps iterating. |
| **`Revert` button** | `savedContent = originalContent`, `bufferContent = originalContent`, exit edit mode | Read-Only. Modified dot removed. All prior saves within the session are undone. |
| **Modal `Save`** | `savedContent = bufferContent`, then proceed with navigation | Target state depends on navigation type (see Focus Management). |
| **Modal `Discard`** | `bufferContent = savedContent` (drop unsaved buffer only, preserve last checkpoint), then proceed | Target state depends on navigation type. Prior `Ctrl+S` saves are preserved. |
| **Modal `Cancel`** | No change, return to editing | Editing (dirty). |

**Key distinction:** Modal `Discard` drops the unsaved buffer back to `savedContent` — it does NOT reset to `originalContent`. Only `Revert` does a full reset. This preserves `Ctrl+S` checkpoints.

### File Path Header

The content pane header shows the full file path with the filename bolded: `/etc/nginx/` **`nginx.conf`**. In edit mode, "unsaved" appears as a text badge next to the path when the buffer is dirty (`bufferContent !== savedContent`). The "modified" dot appears on the file list item (not in the header) when `savedContent !== originalContent`.

### Unsaved Changes Modal

Triggers when the user attempts to navigate away from a **dirty** editor (`bufferContent !== savedContent`):
- Click a different file in the list
- Switch tabs (via Enter on focused tab)
- Click a sidebar navigation link
- Press Escape (after CodeMirror's internal Escape handling — see below)
- Browser `beforeunload` (simplified browser-native dialog, only when dirty)

**Modal content:**
- Title: "Unsaved changes"
- Body: "You have unsaved changes to [filename]. What would you like to do?"
- Three buttons:
  - **Cancel** — return to editing, focus on CodeMirror
  - **Discard** — drop unsaved buffer (`bufferContent = savedContent`), proceed with navigation
  - **Save** — save (`savedContent = bufferContent`), then proceed with navigation
- Initial focus: **Cancel** button (least destructive default)

Focus traps inside the modal. Tab cycles between Cancel, Discard, Save. Escape triggers Cancel (return to editing).

### Escape Key Precedence

CodeMirror uses Escape internally (dismiss search panel, dismiss autocomplete, exit vim mode if enabled). The editor-exit Escape must not compete with these.

**Rule:** The custom `Escape → exit edit mode` keybinding is registered at a **lower precedence** than CodeMirror's built-in keybindings using `keymap.of()` (not `Prec.highest`). This means:
1. If CodeMirror has an active overlay (search, autocomplete), Escape dismisses the overlay. Edit mode is unchanged.
2. If CodeMirror has no active overlay, Escape propagates to the custom handler, which exits edit mode (triggering modal if dirty).

### Keyboard Shortcuts

| Key | Context | Action |
|-----|---------|--------|
| `E` | Read-only view (content pane focused, not in a text input) | Enter edit mode |
| `Escape` | Edit mode (no CM overlay active) | Exit edit (modal if dirty, direct if clean) |
| `Ctrl+S` / `Cmd+S` | Edit mode | Save checkpoint, stay in edit mode |
| `Arrow Left/Right` | Tab bar focused | Move focus between tabs |
| `Home` / `End` | Tab bar focused | First / last tab |
| `Enter` / `Space` | Tab bar focused | Activate focused tab |
| `Arrow Up/Down` | File list focused | Navigate files |
| `Enter` | File list focused | Select focused file |

### Focus Management

| Transition | Focus Lands On |
|------------|----------------|
| Select file (→ Read-Only) | The `[Edit]` button |
| Click `[Edit]` (→ Editing) | CodeMirror editor (cursor at line 1, col 1) |
| Click `[Save]` button (→ Read-Only) | The `[Edit]` button |
| `Ctrl+S` (→ Editing clean) | Stay in CodeMirror (no focus change) |
| Click `[Revert]` (→ Read-Only) | The `[Edit]` button |
| Modal Save/Discard → **file switch** | The newly selected file's `[Edit]` button (or file list item if in read-only) |
| Modal Save/Discard → **tab switch (prior selection)** | The previously selected file's `[Edit]` button in the target tab |
| Modal Save/Discard → **tab switch (no prior selection)** | The first file in the target tab's file list |
| Modal Save/Discard → **tab switch (empty tab, zero files)** | The active tab button itself (`role="tab"`, `aria-selected="true"`) |
| Modal Save/Discard → **sidebar nav away from Editor** | The destination section's heading (per the broader redesign's destination-focus rule) |
| Modal Cancel | CodeMirror editor (return to editing) |

### Screen Reader Announcements

Via `aria-live="polite"` region:

| Event | Announcement |
|-------|--------------|
| Tab switch | "Showing [category] files, [N] files" |
| File selected | "[filename] selected" |
| Enter edit mode | "Editing [filename]" |
| Save (Ctrl+S) | "[filename] saved" |
| Save (button, → Read-Only) | "[filename] saved" |
| Revert | "[filename] reverted to original" |
| Modal Discard | "Changes discarded for [filename]" |

### ARIA Roles

- Tab bar: `role="tablist"` on container, `role="tab"` on each tab, `role="tabpanel"` on file list
- Each tab: `aria-selected="true/false"`, `aria-controls="panel-[id]"`
- Tab panel: `aria-labelledby="tab-[id]"`
- File list: `role="listbox"` with `role="option"` on each item
- Modified files: `aria-label` suffix, e.g., "chrony.conf, modified"
- Modal: `role="dialog"`, `aria-modal="true"`, `aria-labelledby` pointing to title, initial focus on Cancel button

### Static Mode Behavior

When the report is opened in static mode (no refine server), the editor section is visible with file browsing and read-only viewing intact. The `[Edit]` button is **visible but disabled** (`aria-disabled="true"`) with a tooltip: "Editing requires refine mode. Run: inspectah refine <tarball>". This matches the broader HTML report redesign contract where editing controls stay visible-but-disabled with refine-mode guidance.

### Relationship to Global Change Counter

Editor file saves do **not** increment or decrement the global "N changes pending" counter in the Containerfile preview sidebar. That counter is scoped to **decision surfaces** (include/exclude/acknowledge actions in the triage sections). Editor changes are indicated separately via the blue modified dot on files and tabs.

Rebuild is the canonical way to materialize editor changes in generated artifacts. The editor does not claim to know the downstream impact of a file edit without a rebuild.

## Verification Matrix

| Scenario | Expected Behavior |
|----------|-------------------|
| Edit → type → `Ctrl+S` → type more → click file B → `Discard` | Buffer drops to last `Ctrl+S` checkpoint. File A shows modified dot (savedContent ≠ originalContent). File B opens in Read-Only. |
| Edit → type → `Ctrl+S` → type more → click file B → `Save` | Buffer saved. File A shows modified dot. File B opens in Read-Only. |
| Edit → type → click file B → `Discard` | Buffer drops to savedContent (= originalContent since never saved). File A has no modified dot. File B opens in Read-Only. |
| Edit → type → `Revert` | Full reset to originalContent. No modified dot. Returns to Read-Only. |
| Edit → `Ctrl+S` → `Revert` | Full reset to originalContent. The checkpoint save is undone. No modified dot. Returns to Read-Only. |
| Edit (dirty) → switch tab → `Discard` → target tab has no prior selection | Buffer drops to savedContent. Content pane shows "Select a file to view." Focus on first file in target tab's list. |
| Edit (dirty) → click sidebar nav → `Save` | Buffer saved. Editor section scrolls out of view. Focus on destination section heading. |
| Edit (dirty) → browser close/refresh | Browser-native `beforeunload` dialog: "Changes you made may not be saved." |
| Edit (clean, no changes) → browser close/refresh | No `beforeunload` — clean exit. |
| Edit (dirty) → Escape while CM search is open | CM search closes. Editor stays in dirty state. |
| Edit (dirty) → Escape (no CM overlay) | Unsaved changes modal opens. |
| Read-only → Escape | No effect. |
| Edit (dirty) → switch tab to empty `(0)` tab → `Discard` | Buffer drops to savedContent. Content pane shows empty-state message. Focus lands on the active tab button. |
| Static mode → click `[Edit]` (disabled) | No effect. Tooltip visible: "Editing requires refine mode." |
| Autosave PUT returns `409 Conflict` | Silent discard — `409` means newer canonical state already exists. No retry, no warning, no user-visible indication. Per the broader redesign's autosave contract. |
| Autosave PUT fails (`400` / network error) | Uses the shared autosave failure behavior from the broader redesign contract: shared autosave indicator shows failure state, retry once after 2s, recover indicator back to "Saved" on success. No editor-specific warning banner. |

## Technical Constraints

- Single HTML file with embedded CSS and JS (`go:embed`, no build step)
- PatternFly v6 CSS (vendored), dark/light theme toggle
- CodeMirror 6 (vendored, already integrated)
- Vanilla JS state management (no framework)
- File content in JS snapshot object, modifications via PUT to Go HTTP server
- Both dark and light themes must work

## Out of Scope

- **"+ New File" creation** — deferred to follow-up feature. Collins confirmed the need is real (quadlets, bootc-specific configs), but Fern recommends shipping the core view/edit/tab workflow first and adding creation once the base is solid.
- **Vim mode toggle** — removed from toolbar. If needed later, can be added as a settings option.
- **"Save All" button** — removed. Files are saved one at a time.
- **Diff view** — showing what changed vs original. Could be a future enhancement.
