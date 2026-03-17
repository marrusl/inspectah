# Future Visual Improvements

UI/UX improvements for the yoinkc HTML report, organized by effort.

Last audited: 2026-03-17

---

## Larger Features

### Search Across Files
Ctrl+Shift+F to search content across all files in the editor tree. Search against snapshot content fields, highlight matches in the tree, show results in a panel. New UI component.

### Keyboard Navigation Phase 2 — Screen Reader
ARIA live regions for dynamic content (toast notifications, badge updates, re-render results). Role descriptions for custom components.

### Keyboard Navigation Phase 3 — Shortcuts
Power-user keyboard shortcuts: `1-9` for tabs, `j/k` for next/previous item, `/` for search, `r` for mark reviewed.

---

## Specced (pending implementation)

- **Triage progress indicator** — per-section "Mark reviewed" button, sidebar progress bar, checkmarks. Spec: `docs/specs/proposed/2026-03-17-visual-improvements-design.md` Part A
- **Section collapse** — per-card PF6 expandable with chevron toggle. Spec: Part B
- **Keyboard nav Phase 1** — tab order, skip link, modal focus trap, ARIA landmarks. Spec: Part C
- **Animation/transitions polish** — tab fade, card expand/collapse, badge pulse, modal scale-up, reduced-motion guard. Spec: Part D

## Already Implemented (removed from backlog 2026-03-17)

- **Sticky sidebar** — `position: sticky` applied via PF6 classes
- **Toggle switches** — `pf-v6-c-switch` component used across all tabs (replaced checkboxes)
- **Diff view** — custom line-based diff via `lineDiff()` in fleet variant comparison modal (not CM6 merge extension, but functional)
- **Hover preview** — fleet popover functionality with host lists on bar click; config file hover not specifically implemented but editor link (pencil icon) provides quick access
