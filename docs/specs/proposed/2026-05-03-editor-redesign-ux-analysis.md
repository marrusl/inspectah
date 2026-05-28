# Editor Redesign UX Analysis

**Type:** Pre-spec / UX analysis  
**Date:** 2026-05-03  
**Author:** Fern (UX Specialist)  
**Scope:** Edit Files section of the inspectah HTML report (Go port)  
**Status:** Draft for brainstorm

---

## Context

The Edit Files section lets sysadmins view and modify config files, systemd drop-ins, and quadlet units that will be included in a migration Containerfile. The Go port currently has four UX problems: auto-entering edit mode on file selection, false "changed" markers from viewing, a flat undifferentiated file list, and no unsaved-changes protection.

This analysis covers the redesigned interaction model: tabbed file organization, read-only-first viewing, explicit edit mode, and unsaved-changes protection. It is structured as recommendations with open questions where tradeoffs exist.

---

## 1. Tab Design

### 1.1 Structure

Three tabs replace the current flat list with group headers:

| Tab | Label | Source data |
|-----|-------|-------------|
| Config | `Config` | `snapshot.config.files[]` |
| Drop-ins | `Drop-ins` | `snapshot.services.drop_ins[]` |
| Quadlets | `Quadlets` | `snapshot.containers.quadlet_units[]` |

**Rationale for tabs over group headers:** The current Go port uses `file-browser-group` divs with `file-browser-group-label` headers inside a single scrollable list. This buries drop-ins and quadlets below potentially dozens of config files. Tabs give each category equal visual weight and let users jump directly to drop-ins or quadlets without scrolling past 30+ config files.

### 1.2 Tab bar placement

The tab bar sits at the top of the file browser panel (the left drawer panel), above the file list. It does NOT span the full editor width -- it controls which file list is shown, not which content pane is shown. The content/editor pane on the right is shared across all tabs.

```
+---------------------------+-------------------------------+
| [Config] [Drop-ins] [Quad]|  /etc/ssh/sshd_config         |
|---------------------------|  [Edit]                       |
| /etc/ssh/sshd_config    * |-------------------------------|
| /etc/chrony.conf          |  # sshd config file           |
| /etc/fstab                |  PermitRootLogin no           |
| /etc/resolv.conf          |  ...                          |
|                           |                               |
+---------------------------+-------------------------------+
```

### 1.3 Labels and counts

**Recommendation:** Show file counts in tab labels: `Config (39)`, `Drop-ins (3)`, `Quadlets (4)`.

Counts serve two purposes:
1. **Discovery** -- an admin immediately sees that drop-ins and quadlets exist without clicking each tab.
2. **Progress tracking** -- when combined with modified indicators (see 1.5), the count provides at-a-glance status.

**Count source:** Total files in that category, not modified count. The count is static for the session (files are not added/removed in the redesigned editor -- that is the "+ New File" flow, which is a separate feature).

### 1.4 Empty states

When a tab has zero files, show a centered message in the file list area:

- Config (0): `No config files detected.`
- Drop-ins (0): `No systemd drop-in files detected.`
- Quadlets (0): `No quadlet unit files detected.`

The tab itself remains visible but shows count `(0)`. Do not hide empty tabs -- their absence would confuse users who expect the category to exist. An empty tab with `(0)` is informative; a missing tab is ambiguous.

### 1.5 Modified-file indicators

**Recommendation:** Show a small dot or asterisk on file list items that have been modified (content differs from `originalSnapshot`). Additionally, show a badge on the tab label when any file in that category has been modified: `Config (39) *` or a colored dot.

This answers "which files have I touched?" without requiring the user to click through every file.

**Open question:** Should the tab badge show the count of modified files (e.g., `Config (39) [2 modified]`) or just a binary indicator (dot)? A count is more informative but adds visual noise. For the brainstorm: consider that sysadmins working through 39 config files will want to know "how many have I actually changed?" A count serves that need; a dot does not.

### 1.6 Tab persistence and keyboard navigation

**ARIA pattern:** Use `role="tablist"` on the tab bar, `role="tab"` on each tab button, `role="tabpanel"` on the file list container. Follow WAI-ARIA Tabs pattern:

- `Arrow Left` / `Arrow Right` moves focus between tabs.
- `Enter` or `Space` activates the focused tab (if using manual activation -- recommended for this case since tab switching may trigger unsaved-changes checks).
- `Home` / `End` jump to first/last tab.
- The active tab has `aria-selected="true"`, inactive tabs have `aria-selected="false"`.
- Each tab's `aria-controls` points to the tabpanel ID.
- The tabpanel has `aria-labelledby` pointing back to the tab.

**Manual vs. automatic activation:** Use **manual activation** (Arrow moves focus, Enter activates). Automatic activation (Arrow immediately switches tabs) is the simpler pattern, but tab switching here may trigger an unsaved-changes modal. Automatic activation that sometimes triggers a modal is jarring -- the user pressed Arrow expecting a lightweight focus move, not a blocking dialog.

**Open question:** An alternative is automatic activation with no modal on tab switch (treat it like switching files within a tab -- see Section 3 for the unsaved-changes policy). This is simpler but means Arrow Left/Right could silently discard unsaved work if the policy is "save on navigate." Worth discussing in brainstorm.

---

## 2. Read-Only / Edit / Save / Revert Flow

### 2.1 State machine

The editor content pane has four states:

```
                    +----------+
                    |  Empty   |  (no file selected)
                    +----+-----+
                         |
                    select file
                         |
                         v
                    +----------+
              +---->| Read-Only|<-----------+
              |     +----+-----+            |
              |          |                  |
              |     click [Edit]        [Revert] or
              |          |              [Save] (returns
              |          v               to read-only)
              |     +----------+            |
              |     |  Editing |------------+
              |     +----+-----+
              |          |
              |     type changes
              |          |
              |          v
              |     +----------+
              +-----| Dirty    |
               nav  +----------+
              away     |    |
             (modal)   |    |
                  [Save]  [Revert]
                       |    |
                       v    v
                   Read-Only
```

**States:**

| State | Description | Toolbar | Content pane |
|-------|-------------|---------|--------------|
| **Empty** | No file selected | Hidden | Centered placeholder: "Select a file to view" |
| **Read-Only** | File selected, viewing content | File path + `[Edit]` button | `<pre>` block with file content, no line numbers, no cursor |
| **Editing (clean)** | Edit mode, no changes made yet | File path + `[Revert]` `[Save]` (Save disabled) | CodeMirror 6 editor with line numbers, active cursor |
| **Editing (dirty)** | Edit mode, unsaved changes exist | File path + `[Revert]` `[Save]` (Save enabled) | CodeMirror 6 editor, content differs from snapshot |

### 2.2 State transitions

| From | Trigger | To | Side effects |
|------|---------|----|----|
| Empty | Select file | Read-Only | Populate path header and `<pre>` content |
| Read-Only | Click `[Edit]` | Editing (clean) | Mount CodeMirror, hide `<pre>`, store content as `editBaseline` |
| Read-Only | Select different file | Read-Only | Update path header and `<pre>` content |
| Editing (clean) | Type in editor | Editing (dirty) | Compare buffer to `editBaseline`, enable Save if different |
| Editing (clean) | Click `[Revert]` | Read-Only | Tear down CodeMirror, show `<pre>` |
| Editing (clean) | Select different file | *See Section 3* | No unsaved changes, safe to navigate |
| Editing (dirty) | Click `[Save]` | Read-Only | Write buffer to `snapshot`, mark file as modified, tear down CodeMirror, show updated `<pre>`, schedule autosave |
| Editing (dirty) | Click `[Revert]` | Read-Only | Restore content from `originalSnapshot`, tear down CodeMirror, show original `<pre>`, remove from modified set |
| Editing (dirty) | Select different file | Modal | See Section 3 |
| Editing (dirty) | Switch tab | Modal | See Section 3 |
| Editing (dirty) | Navigate away (sidebar) | Modal | See Section 3 |

### 2.3 What "Save" means

Save writes the editor buffer content to the in-memory `snapshot` object (e.g., `snapshot.config.files[i].content = buffer`). This triggers the autosave debounce (PUT /api/snapshot) if in refine mode. Save does NOT trigger a re-render/rebuild -- that is a separate action ("Download tarball" in the bottom toolbar).

After Save:
- The file is marked as "modified" in the file list (dot indicator).
- The content pane returns to read-only view showing the saved content.
- The "N changes pending" counter in the bottom toolbar increments.

### 2.4 What "Revert" means

Revert restores the file content to its value in `originalSnapshot` (the snapshot state at page load). This undoes ALL edits -- both unsaved buffer changes AND previously saved changes. This matches the Python version's behavior and the editor-behavior-fixes spec.

After Revert:
- The file is removed from the "modified" set.
- The content pane returns to read-only view showing the original content.
- If the file was previously saved (modified), the "N changes pending" counter decrements.
- The snapshot is updated with the original content and autosave fires.

**Open question:** Should Revert undo just the current editing session (back to last save) or all changes since page load (back to original)? The Python version and the editor-behavior-fixes spec define Revert as "back to original." This is stronger but also more destructive. A two-level undo (Revert to last save vs. Reset to original) adds complexity. Recommendation: keep single-level Revert-to-original for v1, consistent with prior behavior. Users who want to undo just the current session can click Revert and then re-enter Edit with the original content.

### 2.5 Focus management

Focus management prevents keyboard users from getting lost after state transitions:

| Transition | Focus lands on |
|------------|---------------|
| Select file (to Read-Only) | The `[Edit]` button |
| Click `[Edit]` (to Editing) | The CodeMirror editor (cursor at line 1, column 1) |
| Click `[Save]` (to Read-Only) | The `[Edit]` button |
| Click `[Revert]` (to Read-Only) | The `[Edit]` button |
| Modal Save/Discard (to Read-Only) | The newly selected file's `[Edit]` button |
| Modal Cancel | The CodeMirror editor (return to editing) |

**Screen reader announcements** (via `aria-live="polite"` region):
- Entering edit mode: "Editing [filename]"
- Save: "Saved [filename]"
- Revert: "Reverted [filename] to original"
- Modal open: Handled by modal's `aria-modal` and focus trap

### 2.6 Toolbar layout

The toolbar sits between the tab bar area and the content pane:

```
Read-only state:
+--------------------------------------------------+
| /etc/ssh/sshd_config                      [Edit] |
+--------------------------------------------------+

Editing state:
+--------------------------------------------------+
| /etc/ssh/sshd_config          [Revert]    [Save] |
+--------------------------------------------------+
```

- File path is left-aligned, truncated with ellipsis if needed, full path in `title` attribute.
- Buttons are right-aligned.
- In read-only state: only `[Edit]` visible.
- In editing state: `[Revert]` (secondary/link style) and `[Save]` (primary style). Save is disabled until content differs from `editBaseline`.

**Removed from Python version:** The Vim toggle. This was a niche feature that added toolbar complexity. If Vim mode is wanted later, it can be a keyboard shortcut (Ctrl+Shift+V) rather than a toolbar button.

**Open question:** Should the toolbar show a "modified" indicator for the current file? E.g., a dot or "(modified)" label next to the file path when the file's saved content differs from `originalSnapshot`. This is distinct from the "dirty" (unsaved buffer changes) state -- it means "this file has been saved with changes." The file list already shows this via the dot indicator, so it may be redundant. But it reinforces the state without requiring the user to glance at the file list.

---

## 3. Unsaved Changes Protection

### 3.1 When the modal appears

The unsaved-changes modal appears when the user attempts to navigate away from a dirty editor (Editing state with unsaved changes). Triggers:

| Trigger | Modal? | Rationale |
|---------|--------|-----------|
| Select different file (same tab) | Yes | Would lose unsaved buffer content |
| Switch to different tab | Yes | Would lose unsaved buffer content |
| Click sidebar nav link (leave Editor) | Yes | Would lose unsaved buffer content |
| Browser back/forward | Yes (via `beforeunload`) | Would lose unsaved buffer content |
| Close browser tab | Yes (via `beforeunload`) | Would lose unsaved buffer content |
| Click `[Revert]` | No | Revert is an explicit "discard" action |
| Click `[Save]` | No | Save is an explicit "keep" action |

**Not triggered in clean editing state.** If the user is in Editing mode but has not typed anything (buffer === editBaseline), navigation proceeds without a modal. The user entered edit mode but did not change anything -- no data to lose.

### 3.2 Modal design

```
+-----------------------------------------------+
|  Unsaved changes                               |
|------------------------------------------------|
|                                                |
|  You have unsaved changes to                   |
|  /etc/ssh/sshd_config.                         |
|                                                |
|  [Cancel]          [Discard]          [Save]   |
+------------------------------------------------+
```

**Buttons (left to right):**

| Button | Style | Action |
|--------|-------|--------|
| Cancel | Link (`pf-m-link`) | Close modal, return to editing. Focus returns to CodeMirror. |
| Discard | Secondary (`pf-m-secondary`) | Discard unsaved changes, proceed with navigation. File content reverts to last saved state (NOT originalSnapshot -- the user may have previously saved changes they want to keep). |
| Save | Primary (`pf-m-primary`) | Save current buffer to snapshot, proceed with navigation. |

**Important distinction:** Discard in the modal reverts to the last **saved** state, not to `originalSnapshot`. This is different from the `[Revert]` button, which goes back to `originalSnapshot`. The modal is about unsaved buffer changes; `[Revert]` is about all changes since page load. This distinction matters when a user has saved once, continued editing, and then tries to navigate away -- Discard keeps their first save, Revert would undo it.

### 3.3 Multiple unsaved files

The redesign does not support multiple simultaneously open editors. Only one file is in edit mode at a time. Therefore, the unsaved-changes modal only ever applies to a single file. This simplifies the interaction significantly compared to a multi-tab editor paradigm.

If a user saves File A, then edits File B, File A's changes are already persisted to the snapshot. Only File B's unsaved buffer is at risk. The modal only fires for File B.

### 3.4 `beforeunload` handling

Register a `beforeunload` handler that fires when the editor is in a dirty state:

```javascript
window.addEventListener('beforeunload', function(e) {
  if (editorIsDirty()) {
    e.preventDefault();
    // Modern browsers show their own generic message
  }
});
```

This covers browser tab close, browser navigation, and page refresh. The browser shows its own native dialog -- we cannot customize it, and we should not try. The custom modal (Section 3.2) handles in-app navigation; `beforeunload` handles browser-level navigation.

### 3.5 Sidebar navigation interception

When the user clicks a sidebar nav link while in a dirty editing state, the click handler must:

1. Prevent default navigation.
2. Store the intended destination.
3. Show the unsaved-changes modal.
4. On Save: save, then navigate to the stored destination.
5. On Discard: revert buffer to last save, then navigate.
6. On Cancel: return to editing, clear stored destination.

This requires wrapping the existing `show(section)` function with a dirty-state guard.

---

## 4. File List Behavior Within Tabs

### 4.1 Per-tab selection state

Each tab maintains its own selected file. When the user:

1. Selects `sshd_config` in Config tab.
2. Switches to Drop-ins tab.
3. Switches back to Config tab.

`sshd_config` is still selected and its content is displayed in the content pane.

**Implementation:** Store `selectedFile[tabId]` as a per-tab property. On tab switch, restore the selected file for the target tab and update the content pane.

### 4.2 Content pane on tab switch

When switching tabs:

- **If the target tab has a previously selected file:** Show that file in read-only view (even if it was in edit mode before -- edit mode is not preserved across tab switches, which is why the unsaved-changes modal fires first if needed).
- **If the target tab has no previously selected file:** Show the Empty state ("Select a file to view").

**Rationale for not preserving edit mode across tab switches:** Edit mode involves a mounted CodeMirror instance with cursor position, undo history, and scroll state. Preserving this across tab switches would require maintaining multiple CodeMirror instances in the DOM (hidden but alive). This adds complexity for an edge case -- most users will not switch tabs mid-edit. The modal provides a clean checkpoint.

### 4.3 File list sorting

Within each tab, files are sorted alphabetically by full path. This matches the behavior specified in the editor-behavior-fixes spec. The Python version also sorts alphabetically within categories.

### 4.4 File path display

Show the full path in the file list item. Use CSS to dim the directory portion and bold the filename:

```
/etc/ssh/sshd_config
^^^^^^^^^            <- dimmed (opacity: 0.6)
         ^^^^^^^^^^^  <- bold
```

This matches the Python version's path display behavior from the editor-behavior-fixes spec.

### 4.5 File list scrolling

Each tab's file list scrolls independently. When switching tabs, the scroll position for the target tab is restored. This prevents the user from losing their place in a long config file list when checking a drop-in and returning.

---

## 5. File Count Badges

### 5.1 Recommendation

Show file counts in tab labels. This is low-cost (the data is already computed from `collectEditorFiles()`) and high-value for discovery.

Format: `Config (39)` -- plain parenthetical count, not a PF6 badge component. A badge component adds visual weight that competes with more important indicators. Parenthetical counts are the standard pattern for tab counts in dense UIs (Gmail, Jira, VS Code).

### 5.2 Modified-file badge

When files in a tab category have been modified, add a visual indicator to the tab label. Two options:

**Option A: Dot indicator**
```
Config (39) *    Drop-ins (3)    Quadlets (4) *
```
A colored dot (or asterisk) appears when any file in the category has been modified. Simple, binary signal. Used by VS Code for unsaved files.

**Option B: Modified count**
```
Config (39) [2 edited]    Drop-ins (3)    Quadlets (4) [1 edited]
```
Shows how many files have been modified. More informative but noisier.

**Recommendation:** Option A (dot indicator) for the tab. The exact count of modified files is available in the file list itself (each modified file has a dot). The tab-level indicator just answers "is there anything modified in this category?" -- a binary question.

### 5.3 Modified indicator in file list

Each file list item shows a small dot (filled circle, 6px) to the left of the filename when the file has been modified (saved content differs from `originalSnapshot`). Color: use the info/blue semantic color (`--pf-t--global--color--status--info--default`) to distinguish from error/warning indicators used elsewhere in the report.

```
  /etc/ssh/sshd_config            <- unmodified
* /etc/chrony.conf                <- modified (blue dot)
  /etc/fstab                      <- unmodified
```

---

## 6. Accessibility

### 6.1 Tab panel ARIA roles

The file category tabs use the WAI-ARIA Tabs pattern:

```html
<div role="tablist" aria-label="File categories">
  <button role="tab" id="tab-config" aria-selected="true"
          aria-controls="panel-config" tabindex="0">
    Config (39)
  </button>
  <button role="tab" id="tab-dropins" aria-selected="false"
          aria-controls="panel-dropins" tabindex="-1">
    Drop-ins (3)
  </button>
  <button role="tab" id="tab-quadlets" aria-selected="false"
          aria-controls="panel-quadlets" tabindex="-1">
    Quadlets (4)
  </button>
</div>

<div role="tabpanel" id="panel-config" aria-labelledby="tab-config">
  <!-- file list -->
</div>
<!-- other panels hidden with display:none -->
```

### 6.2 File list ARIA

The file list within each tab panel uses `role="listbox"` with `role="option"` on each item (matching the current Go port pattern). Selected file has `aria-selected="true"`. Modified files include `aria-label` suffix: "sshd_config, modified".

### 6.3 Keyboard shortcuts

| Context | Key | Action |
|---------|-----|--------|
| Tab bar | Arrow Left/Right | Move focus between tabs |
| Tab bar | Enter/Space | Activate focused tab |
| Tab bar | Home/End | First/last tab |
| File list | Arrow Up/Down | Move focus between files |
| File list | Enter | Select focused file |
| Read-only view | E | Enter edit mode (mnemonic shortcut) |
| Edit mode | Escape | Exit edit mode (triggers modal if dirty, otherwise returns to read-only) |
| Edit mode | Ctrl+S / Cmd+S | Save |
| Modal | Escape | Cancel (return to editing) |
| Modal | Tab | Cycle between Cancel, Discard, Save buttons |

**Escape in edit mode (open question):** Should Escape exit edit mode? In CodeMirror, Escape is used to blur the editor or exit certain modes (like search). If we capture Escape for "exit edit mode," it conflicts with CodeMirror's internal Escape handling. Options:

1. **Escape exits edit mode** (with modal if dirty). Simple, discoverable. But conflicts with CodeMirror search-panel dismiss.
2. **Escape only exits when focus is on the toolbar**, not when CodeMirror has focus. More nuanced but avoids conflicts.
3. **No Escape shortcut.** Users click `[Revert]` or `[Save]` to exit. Least surprising but less efficient.

Recommendation: Option 2 -- Escape exits edit mode only when the toolbar or content area outside CodeMirror has focus. When CodeMirror has focus, Escape behaves as CodeMirror expects (dismiss search, etc.). Users can Tab out of CodeMirror to the toolbar, then Escape.

### 6.4 Focus trapping in edit mode

Edit mode does NOT trap focus. The user can Tab through: CodeMirror editor -> Revert button -> Save button -> (cycle). This is not a modal -- it is a mode within the page. Focus trapping is reserved for actual modals (the unsaved-changes dialog).

### 6.5 Screen reader announcements

Use an `aria-live="polite"` region for state change announcements:

| Event | Announcement |
|-------|-------------|
| Tab switch | "Showing [Config/Drop-ins/Quadlets] files, [N] files" |
| File selected | "[filename] selected" |
| Enter edit mode | "Editing [filename]" |
| Save | "[filename] saved" |
| Revert | "[filename] reverted to original" |
| File modified indicator appears | No announcement (visual-only, info available via aria-label) |

### 6.6 Color contrast

All indicators must meet WCAG 2.1 AA contrast ratios (4.5:1 for text, 3:1 for UI components) in both light and dark themes. The modified-file dot uses the info semantic color, which PatternFly v6 already ensures meets contrast requirements in both themes.

---

## 7. Anti-Patterns to Avoid

### 7.1 Auto-entering edit mode on file selection

This is the primary bug in the Go port. Clicking a file should show a read-only view. Edit mode is an explicit user action. Auto-entering edit mode causes:
- False "modified" markers (the Go port writes content back on file switch even if unchanged, because CodeMirror's `state.doc.toString()` may normalize whitespace).
- Cognitive overhead -- the user wanted to read, not edit.
- Accessibility issues -- CodeMirror steals focus and changes the interaction model unexpectedly.

### 7.2 Treating "viewed" as "modified"

Never mark a file as modified unless its content has actually changed. Compare against `originalSnapshot` content, not against "was an editor mounted." The Go port's `openFileInEditor` saves the current editor content back to the snapshot on every file switch (`currentEditorFile.ref.content = editorInstance.state.doc.toString()`), which can create false modifications via whitespace normalization or encoding differences.

### 7.3 Silent data loss

Never discard unsaved changes without user confirmation. The Go port currently has no unsaved-changes protection -- switching files silently writes the buffer back (which is a different kind of problem: silent auto-save without user intent).

### 7.4 Destroying CodeMirror undo history on save

When the user saves but continues editing (not the case in our design -- save returns to read-only), destroying undo history prevents Ctrl+Z from working. In our design, save exits edit mode, so this is moot. But if the design changes to allow continued editing after save, preserve undo history.

### 7.5 Using automatic tab activation with modal interrupts

As discussed in 1.6, automatic tab activation (Arrow key immediately switches tabs) combined with a blocking modal creates a jarring experience. Either use manual activation (our recommendation) or eliminate the modal on tab switch (by auto-saving or discarding).

### 7.6 Hiding empty tabs

Hiding tabs when their category has zero files creates a shifting layout and makes users wonder where the tab went. Always show all three tabs, with a `(0)` count and an empty-state message in the panel.

### 7.7 Overloading the editor toolbar

The Python version had Edit, Vim toggle, Revert, Save All, and Save in the toolbar. This is five actions in a context where only two are relevant at a time. The redesign simplifies to: Edit (in read-only) or Revert + Save (in edit mode). No Vim toggle, no Save All (only one file is edited at a time).

### 7.8 Confusing "Revert" semantics

Revert must have one clear meaning. In the Python version, Revert was ambiguous -- it restored content but left the user in edit mode, which led to the "revert trap" bug. In the redesign, Revert always means: "undo all my changes and return to read-only view." It is a full reset, not a partial undo.

---

## 8. Open Questions for Brainstorm

These are genuine design decisions where reasonable people could disagree. They should be resolved during the brainstorm before moving to a full spec.

### 8.1 Tab activation model

**Manual activation** (Arrow moves focus, Enter activates) vs. **Automatic activation** (Arrow immediately switches tab).

- Manual is safer with the unsaved-changes modal but less fluid for browsing.
- Automatic is more natural for keyboard users but creates a jarring modal-on-Arrow-key experience.
- A third option: automatic activation with auto-save on tab switch (no modal). This is the most fluid but removes user control over when saves happen.

### 8.2 Revert scope

**Revert to original** (page-load state) vs. **Revert to last save** (discard just unsaved buffer).

- Revert-to-original is simpler (one concept) and matches the Python behavior.
- Revert-to-last-save is less destructive and matches expectations from desktop editors.
- Could offer both: `[Undo changes]` (to last save) and `[Reset to original]` (to page load). But two undo levels add complexity.

### 8.3 Escape key behavior in edit mode

See Section 6.3 for the three options. This affects both keyboard efficiency and CodeMirror compatibility.

### 8.4 Modified-file badge style

Dot indicator (Option A) vs. modified count (Option B) on tab labels. See Section 5.2.

### 8.5 "Save" button label

"Save" is the obvious choice, but in the context of this tool, "save" writes to an in-memory snapshot, not to disk. The actual disk write happens via autosave (debounced PUT). Should the button say "Save" (familiar but technically imprecise), "Apply" (accurate but unfamiliar in editor contexts), or "Save" with a tooltip explaining the behavior?

Recommendation: "Save" is the right label. Users understand "save" as "commit my changes." The autosave mechanism is an implementation detail they should not need to think about. "Apply" sounds like a settings dialog, not a text editor.

### 8.6 Ctrl+S / Cmd+S behavior

Should Ctrl+S/Cmd+S in edit mode save AND exit to read-only (matching the `[Save]` button), or save and stay in edit mode (matching desktop editor expectations)?

Arguments for save-and-exit: Consistent with the `[Save]` button. One behavior, one mental model.

Arguments for save-and-stay: Desktop muscle memory. Sysadmins are used to Vim/nano where save keeps them in the editor.

Recommendation: Save-and-exit for v1, matching the button. If users request save-and-stay, it can be added as an option later.

### 8.7 Path display: full path vs. filename with tooltip

The file list currently shows full paths (Go port) or filename-only with dimmed prefix (Python version spec). For tabs with many files sharing deep paths (e.g., `/etc/systemd/system/`), full paths create long, hard-to-scan list items. Options:

- Full path with dimmed directory, bold filename (Python spec approach).
- Filename only, full path in tooltip and in the toolbar path header.
- Relative path within category (e.g., `ssh/sshd_config` under Config, stripping `/etc/`).

Recommendation: Full path with dimmed directory. It provides maximum information without requiring hover, and the dimming reduces visual noise. The toolbar header also shows the full path, so the file list display is reinforcement, not the only source.

---

## 9. Interaction with the Broader Redesign

### 9.1 Relationship to the HTML Report Redesign spec

The HTML Report Redesign spec (2026-04-28) describes the editor as one of 10 sidebar destinations. It specifies "File browser + CodeMirror editing for config files, drop-ins, and quadlet files" and marks it as "Full editing, refine mode only." This UX analysis is consistent with that spec and provides the detailed interaction design that the broader spec defers.

### 9.2 Static mode behavior

In static mode (file:// protocol or server without re_render capability), the editor section should show files in read-only view with tabs. The `[Edit]` button is not shown. The section header shows: "Editing requires refine mode -- run `inspectah refine <tarball>` to enable." This matches the broader redesign spec's treatment of disabled controls in static mode.

### 9.3 Autosave integration

File saves in the editor write to the in-memory snapshot. The existing autosave mechanism (debounced PUT /api/snapshot) handles persistence. No additional autosave logic is needed in the editor -- it piggybacks on the existing system. The editor just needs to call `scheduleAutosave()` after updating the snapshot.

### 9.4 Bottom toolbar

The editor section does not need its own bottom toolbar. The global bottom toolbar (with "Download tarball" / rebuild functionality and change counter) applies to the whole report. Editor saves increment the pending-changes counter in that global toolbar. The Python version's per-editor bottom bar ("Discard all", pending count, "Rebuild & Download") is superseded by the global toolbar in the redesigned report.

---

## 10. Implementation Notes for Kit

These are not part of the UX spec but are observations from the codebase that will help implementation.

### 10.1 Data model mapping

The three tabs map to these snapshot paths:

| Tab | Snapshot path | Content field |
|-----|--------------|---------------|
| Config | `snapshot.config.files[i]` | `.content` |
| Drop-ins | `snapshot.services.drop_ins[i]` | `.content` |
| Quadlets | `snapshot.containers.quadlet_units[i]` | `.content` (may need `.name` for path fallback) |

The existing `collectEditorFiles()` function in the Go port already groups files by family. The tab redesign replaces the group-header rendering with actual tabs.

### 10.2 State to track per file

```javascript
{
  path: string,           // file path
  family: string,         // 'config' | 'drop-in' | 'quadlet'
  ref: object,            // reference to snapshot object
  originalContent: string, // content at page load (from originalSnapshot)
  isModified: boolean,    // content !== originalContent
}
```

### 10.3 State to track globally

```javascript
{
  activeTab: string,           // 'config' | 'drop-in' | 'quadlet'
  selectedFile: {              // per-tab selection
    config: string | null,     // file path
    'drop-in': string | null,
    quadlet: string | null,
  },
  editorMode: string,          // 'empty' | 'read-only' | 'editing'
  editBaseline: string | null, // content when Edit was clicked
  isDirty: boolean,            // buffer !== editBaseline
  pendingNavigation: object | null, // stored destination during modal
}
```

### 10.4 Content comparison for "modified" detection

Use strict string equality (`===`) to compare current content with `originalContent`. Do NOT use CodeMirror's `state.doc.toString()` for comparison during read-only mode -- only compare when the user explicitly saves. This prevents the false-modification bug in the current Go port.
