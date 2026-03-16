# Future Visual Improvements

UI/UX improvements for the yoinkc HTML report, organized by effort.

## Quick Wins

### Sticky Sidebar
The navigation sidebar scrolls away on long reports. Make it `position: sticky; top: 0; height: 100vh; overflow-y: auto;` so it stays visible while scrolling content. PF6 supports this pattern.

### Section Collapse
Large sections (packages, config, services) can be overwhelming on systems with hundreds of items. Wrap each section's content in a PF6 expandable section (`pf-v6-c-expandable-section`) so operators can collapse sections they've already reviewed. Default: expanded.

## Medium Effort

### Toggle Switches + Greyed Cards
Replace all include/exclude checkboxes with PF6 toggle switches. Excluded items get reduced opacity (~0.6) instead of strikethrough. More PF6-idiomatic, communicates "excluded from this migration" rather than "deleted," and clearly signals reversibility. Touches every tab — needs a mini-brainstorm for the JS handler changes.

### Triage Progress Indicator
The summary banner shows counts but doesn't show progress. Add a "7 of 12 items reviewed" indicator or progress bar to help operators track their work through the triage process. Needs to define what "reviewed" means (toggled at least once? viewed?).

### Hover Preview on Config Cards
Hovering over a config file card shows a small content preview tooltip, reducing the need to click into the editor for quick checks. Use PF6 tooltip or popover component.

## Larger Features

### Diff View
When a file has been edited, show a side-by-side or inline diff against the original. CodeMirror 6 has `@codemirror/merge` extension. Requires rebuilding the CM6 bundle and adding a toggle in the editor toolbar (e.g., "View diff" button that switches between edit mode and diff mode).

### Search Across Files
Ctrl+Shift+F to search content across all files in the editor tree. Search against snapshot content fields, highlight matches in the tree, show results in a panel. New UI component.
