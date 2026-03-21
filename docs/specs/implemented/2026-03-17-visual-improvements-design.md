# Visual Improvements Batch

**Date:** 2026-03-17
**Status:** Implemented
**Implementation note:** Parts A-D are independently implementable. Part D's collapsible animation depends on Part B's DOM structure but can be implemented in the same pass.

## Problem

Four visual/interaction gaps in the HTML report:

1. **No triage progress tracking** — operators review 12 inspector/system tabs but have no way to track which sections they've completed. The summary banner shows aggregate counts but not review progress.
2. **No section collapse** — tabs with many cards (Packages has 4+, Kernel/Boot has 6) create long scroll areas. Operators can't collapse sections they've finished reviewing.
3. **No keyboard navigation** — custom elements (fleet bars, copy buttons, pencil icons, toggles) lack focus management. No skip link, no ARIA landmarks, modals don't trap focus.
4. **No transition polish** — tab switches, toggle changes, and badge updates happen instantly with no visual continuity. State changes feel jarring.

## Part A: Triage Progress Indicator

### Scope

The 12 inspector/system tabs that contain triageable content:

| Section | Tab ID |
|---------|--------|
| Packages | `packages` |
| Config | `config` |
| Services | `services` |
| Users/Groups | `users_groups` |
| Containers | `containers` |
| Non-RPM | `non_rpm` |
| Scheduled | `scheduled_tasks` |
| Kernel/Boot | `kernel_boot` |
| SELinux | `selinux` |
| Network | `network` |
| Storage | `storage` |
| Secrets | `secrets` |

The 5 overview tabs (Summary, Audit, Warnings, Editor/File Browser, Containerfile) are excluded — they don't contain triage decisions.

### Sidebar progress bar

Add a compact progress bar at the top of the sidebar nav, above the "Overview" section header:

- PF6 progress component (`pf-v6-c-progress pf-m-sm`) with `pf-m-inside` variant for inline percentage text
- Label: "**N of 12 reviewed**" above the bar
- Bar fills proportionally as sections are marked reviewed
- Color: default PF6 blue (`--pf-v6-c-progress__bar--BackgroundColor`)
- At 12/12: bar turns green (`pf-m-success`) and label changes to "**All sections reviewed**"

### Per-tab "Mark reviewed" button

Each of the 12 triageable tabs gets a "Mark as reviewed" button in its tab content header area:

- Position: right-aligned in a thin toolbar row at the top of the tab content, below the tab title
- PF6 button: `pf-v6-c-button pf-m-secondary pf-m-small`
- Label: "Mark as reviewed"
- On click: toggles to a checked state — button becomes `pf-m-plain` with a checkmark icon and "Reviewed" label (green text, `--pf-v6-global--success-color--100`)
- Clicking again (on a reviewed section) un-marks it — returns to the secondary button state
- Each click updates the sidebar progress bar count and checkmark

### Sidebar checkmarks

Reviewed tabs show a small green checkmark icon after the tab label in the sidebar nav:

- Icon: PF6 `pf-v6-pficon-check` or Unicode ✓ in green (`--pf-v6-global--success-color--100`)
- Position: after the tab label text, before the triage count badge (if present)
- Appears on mark, disappears on un-mark

### State management

Review state is stored in a JS object (`reviewedSections = {}`) keyed by tab ID. This is purely client-side — it does not persist to the snapshot JSON, does not survive page reload, and does not affect the Containerfile or any export.

**Reset interaction:** The existing "Reset to original inspection" button clears all review marks (sets all to un-reviewed) along with its other reset behavior.

**Re-render interaction:** A re-render replaces the page content, so review state is naturally cleared — consistent with re-render clearing other client-side state.

## Part B: Section Collapse (Per-Card)

### Design

Wrap each card's content body in a PF6 expandable section (`pf-v6-c-expandable-section`). The card header becomes the toggle.

### Structure per card

```html
<div class="pf-v6-c-card">
  <div class="pf-v6-c-card__header">
    <button class="pf-v6-c-expandable-section__toggle" aria-expanded="true">
      <span class="pf-v6-c-expandable-section__toggle-icon">
        <!-- chevron icon, rotates 90deg when collapsed -->
      </span>
      <span class="pf-v6-c-expandable-section__toggle-text">
        Card Title
      </span>
    </button>
  </div>
  <div class="pf-v6-c-expandable-section__content">
    <!-- existing card body content -->
  </div>
</div>
```

### Toggle behavior

- Click the card header chevron or title text to toggle expand/collapse
- `aria-expanded` attribute toggles between `true`/`false`
- Collapsed state hides `.pf-v6-c-expandable-section__content` via `display: none` (overridden by Part D's animation when transitions are enabled)
- Chevron rotates: down (expanded) to right (collapsed)

### Default state

All cards start expanded. No persistence of collapse state across re-render or page reload.

### Cards inventory

Cards that get the collapsible treatment (cards with substantive content — tables, code blocks, lists):

| Tab | Cards |
|-----|-------|
| Packages | Repositories, Packages Added (dep tree), Module Streams, Version Locks, Duplicate Packages |
| Config | Configuration Files |
| Services | Service State Changes, Drop-in Overrides |
| Users/Groups | Users, Groups |
| Containers | Quadlet Units, Compose Files |
| Non-RPM | Compiled Binaries, Python Virtual Environments, Node.js/npm, Ruby gems |
| Scheduled | Systemd Timers, Cron-Converted Timers, Cron Jobs, At Jobs |
| Kernel/Boot | Kernel Command Line, Sysctl Overrides, Module/Dracut Config, Tuned Profiles, Locale/Timezone, Alternatives |
| SELinux | Overview, Custom Policy Modules, Non-Default Booleans, Port Labels, File Contexts, Audit Rules, PAM Configs |
| Network | Connections, Firewall Zones, IP Routes, IP Rules, DNS/Resolver |
| Storage | Fstab Entries |
| Secrets | Secrets table |
| Audit | Each audit section (already has expandable behavior — verify no conflict) |

**Excluded:** Summary tab cards (small, always-visible dashboard), Containerfile tab (single code block, no benefit), Warnings tab (flat alert list with dismiss), Editor tab (custom layout).

### Interaction with Part A

Collapsing a card does **not** mark the section as reviewed. These are independent: collapse is "hide what I've seen," reviewed is "I've made my decisions here." An operator might collapse the RPM table but leave the tab un-reviewed because they haven't checked Version Locks yet.

## Part C: Keyboard Navigation / Accessibility (Phase 1)

This part establishes the structural foundation. Screen reader enhancements (ARIA live regions, role descriptions) and keyboard shortcuts (power-user hotkeys) are deferred to future specs.

### Tab order

Logical tab order through the report:

1. Skip link (see below)
2. Toolbar (top — re-render button, prevalence slider if fleet)
3. Sidebar navigation links (top to bottom)
4. Active tab content (top to bottom within the tab):
   - "Mark as reviewed" button
   - Card collapse toggles
   - Table rows: include toggles, strategy dropdowns, action buttons
   - Inline controls: fleet variant radios, compare buttons, copy buttons
5. Bottom toolbar (if present)

### Skip link

Hidden "Skip to main content" link as the first focusable element in the document:

```html
<a class="pf-v6-c-skip-to-content" href="#main-content">Skip to main content</a>
```

- Visually hidden until focused (PF6 provides `.pf-v6-c-skip-to-content` with `:focus` visibility)
- Target: `id="main-content"` on the tab content container

### Focus trap in modals

All modals (compare modal, new file modal, copy format modal, host list popover) must trap focus:

- On open: focus moves to the first focusable element inside the modal
- Tab/Shift+Tab cycles within the modal's focusable elements
- Escape closes the modal
- On close: focus returns to the element that triggered the modal

Implementation: a lightweight `trapFocus(modalElement)` JS function that:
1. Finds all focusable elements within the modal (`a, button, input, select, textarea, [tabindex]:not([tabindex="-1"])`)
2. On keydown, if Tab on last element then focus first; if Shift+Tab on first then focus last
3. On Escape: close handler
4. Returns a cleanup function to remove the event listener

### Focus return

After any action that moves focus away from the trigger (modal close, popover dismiss, dropdown close), focus returns to the triggering element. This requires storing `document.activeElement` before opening.

### Visible focus indicators

Audit all custom interactive elements for `:focus-visible` styles. PF6 components get this for free; custom elements that need explicit focus styles:

- Fleet prevalence color bars (if clickable)
- Copy button format dropdown items
- Pencil icon edit links
- Editor file list items
- Editor variant accordion rows
- Card collapse toggle buttons (Part B)

Add `outline: 2px solid var(--pf-v6-global--primary-color--100); outline-offset: 2px;` to any custom interactive element missing a focus indicator.

### ARIA landmarks

| Element | Attribute |
|---------|-----------|
| Sidebar `<nav>` | `role="navigation"` + `aria-label="Section navigation"` |
| Tab content container | `role="main"` + `id="main-content"` |
| Bottom toolbar | `role="toolbar"` + `aria-label="Actions"` |
| Top toolbar (if present) | `role="toolbar"` + `aria-label="Report controls"` |
| Active sidebar link | `aria-current="page"` (updated on tab switch) |
| Expandable cards (Part B) | `aria-expanded` on toggle button, `aria-controls` pointing to content ID |

### Tab switch focus management

When switching tabs (sidebar click or browser back/forward), move focus to the tab content heading or the "Mark as reviewed" button. This prevents focus from being lost in the now-hidden previous tab's content.

## Part D: Animation/Transitions Polish

All transitions use CSS only (no JS animation libraries). All respect `prefers-reduced-motion: reduce`.

### Reduced motion

```css
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    animation-duration: 0.01ms !important;
    transition-duration: 0.01ms !important;
  }
}
```

### Tab switching

Fade transition on tab content panels:

```css
.tab-content-panel {
  opacity: 0;
  transition: opacity 150ms ease-in-out;
}
.tab-content-panel.pf-m-current {
  opacity: 1;
}
```

The `show(id)` JS function adds/removes `pf-m-current` — the CSS transition handles the visual.

### Collapsible cards (Part B)

Smooth expand/collapse using `max-height` + `opacity`:

```css
.pf-v6-c-expandable-section__content {
  overflow: hidden;
  max-height: 5000px; /* large enough for any content */
  opacity: 1;
  transition: max-height 200ms ease-out, opacity 150ms ease-out;
}
.pf-v6-c-expandable-section.pf-m-collapsed .pf-v6-c-expandable-section__content {
  max-height: 0;
  opacity: 0;
  padding-top: 0;
  padding-bottom: 0;
}
```

Chevron rotation:

```css
.pf-v6-c-expandable-section__toggle-icon {
  transition: transform 200ms ease-out;
}
.pf-v6-c-expandable-section.pf-m-collapsed .pf-v6-c-expandable-section__toggle-icon {
  transform: rotate(-90deg);
}
```

### Toggle feedback

PF6 switch components already have built-in transitions. Audit custom toggles (fleet prevalence slider Apply/Cancel buttons) for consistent timing. No additional work expected.

### Sidebar badge pulse

Brief scale pulse when triage count badges update:

```css
@keyframes badge-pulse {
  0% { transform: scale(1); }
  50% { transform: scale(1.15); }
  100% { transform: scale(1); }
}
.sidebar-badge-updated {
  animation: badge-pulse 200ms ease-out;
}
```

JS adds `sidebar-badge-updated` class on badge value change, removes it after animation completes (200ms timeout or `animationend` event).

### Modal transitions

Fade-in backdrop + scale-up modal:

```css
.pf-v6-c-backdrop {
  opacity: 0;
  transition: opacity 150ms ease-out;
}
.pf-v6-c-backdrop.pf-m-open {
  opacity: 1;
}

.pf-v6-c-modal-box {
  transform: scale(0.95);
  opacity: 0;
  transition: transform 150ms ease-out, opacity 150ms ease-out;
}
.pf-v6-c-backdrop.pf-m-open .pf-v6-c-modal-box {
  transform: scale(1);
  opacity: 1;
}
```

### "Mark reviewed" checkmark

Subtle check-appear animation:

```css
@keyframes check-appear {
  0% { transform: scale(0); opacity: 0; }
  60% { transform: scale(1.2); }
  100% { transform: scale(1); opacity: 1; }
}
.reviewed-checkmark {
  animation: check-appear 250ms ease-out;
}
```

### Progress bar

PF6 progress bar already transitions its width via CSS. No additional work needed — the bar animates smoothly as the width percentage changes.

## Out of Scope

- Cross-file search (separate spec — larger feature with new UI component)
- Screen reader enhancements beyond ARIA landmarks (Phase 2)
- Keyboard shortcuts / power-user hotkeys (Phase 3)
- Persistence of review state or collapse state across page reload
- Review state in snapshot JSON or Containerfile output
- Hover preview / tooltip enhancements
- Diff view improvements

## Implementation Order

Recommended sequence (each part is independently implementable):

1. **Part B (Section Collapse)** — introduces the DOM structure that Part D animates
2. **Part D (Animations)** — builds on Part B's expandable sections, adds transitions everywhere
3. **Part A (Progress Indicator)** — independent feature, benefits from Part D's animations being in place
4. **Part C (Keyboard Nav)** — structural pass that audits all interactive elements including those from Parts A and B

## Testing

- **Part A:** Verify progress bar updates on mark/un-mark. Verify checkmarks appear/disappear in sidebar. Verify reset clears all review marks. Verify re-render clears review state. Verify count never exceeds 12 or goes below 0. Verify 12/12 shows success state.
- **Part B:** Verify all inventoried cards have collapse toggle. Verify expand/collapse toggles `aria-expanded`. Verify collapsed content is hidden. Verify default state is expanded. Verify collapse does not affect "mark reviewed" state. Verify audit tab expandable sections don't conflict.
- **Part C:** Verify skip link appears on focus and jumps to main content. Verify tab order follows the specified sequence. Verify modal focus trap (Tab, Shift+Tab, Escape). Verify focus returns to trigger after modal close. Verify all custom interactive elements have visible focus indicators. Verify `aria-current="page"` updates on tab switch. Verify ARIA landmarks are present.
- **Part D:** Verify tab fade transition on switch. Verify card expand/collapse animation. Verify badge pulse on count change. Verify modal fade-in/scale-up. Verify checkmark appear animation. Verify `prefers-reduced-motion: reduce` disables all animations. Verify no animation exceeds 250ms.
