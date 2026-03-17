# Future Visual Improvements

UI/UX improvements for the yoinkc HTML report, organized by effort.

Last audited: 2026-03-17

---

## Medium Effort

### Triage Progress Indicator
The summary banner shows counts but doesn't show progress. Add a "7 of 12 items reviewed" indicator or progress bar to help operators track their work through the triage process. Needs to define what "reviewed" means (toggled at least once? viewed?).

### Section Collapse
Large sections (packages, config, services) can be overwhelming on systems with hundreds of items. Wrap each section's content in a PF6 expandable section (`pf-v6-c-expandable-section`) so operators can collapse sections they've already reviewed. Default: expanded.

## Larger Features

### Search Across Files
Ctrl+Shift+F to search content across all files in the editor tree. Search against snapshot content fields, highlight matches in the tree, show results in a panel. New UI component.

---

## Already Implemented (removed from backlog 2026-03-17)

- **Sticky sidebar** — `position: sticky` applied via PF6 classes
- **Toggle switches** — `pf-v6-c-switch` component used across all tabs (replaced checkboxes)
- **Diff view** — custom line-based diff via `lineDiff()` in fleet variant comparison modal (not CM6 merge extension, but functional)
- **Hover preview** — fleet popover functionality with host lists on bar click; config file hover not specifically implemented but editor link (pencil icon) provides quick access
