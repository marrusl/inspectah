# Containerfile Change Highlights

## Summary

When a user makes refinement decisions (toggling packages, services, configs
in or out), the containerfile preview should highlight what changed — drawing
attention to added and removed lines with temporary visual feedback, automatic
scrolling, and appropriate behavior when the panel is collapsed.

## Trigger Model

The system does not track which refinement operation caused a change. Instead,
`ContainerfilePanel` diffs the previous `containerfilePreview` string against
the new one on every render. Any text change, from any cause, produces
highlights. This keeps the feature decoupled from the refinement operation
types and automatically covers future operations.

## Diffing Strategy

Line-by-line diff of the raw containerfile string using the `diff` npm package
(`diffLines`). The containerfile's `# === Section ===` headers are used as
scroll targets but not as diff boundaries — line-level diffing handles all
cases including mid-section edits and entire section additions/removals.

### State Management

A custom hook (`useContainerfileDiff`) inside `ContainerfilePanel`:

- Stores the previous containerfile string via a `usePrevious` pattern.
- Runs the diff on each update and produces a list of added/removed line
  indices.
- Manages highlight lifecycle: each highlight has an expiry timer. Removed
  lines are pruned from state after the collapse animation's `transitionend`
  fires.
- Exposes a `clearHighlights` callback for the collapsed-panel expand flow.

State stays inside `ContainerfilePanel` — no other component needs it.

## Visual Treatment

### Additions

- **Appearance:** Instant — line appears at full height immediately.
- **Highlight:** Green-tinted background (`rgba(74,222,128,0.15)` dark /
  `rgba(34,197,94,0.12)` light) with a solid green left border (3px,
  `#4ade80` dark / `#22c55e` light).
- **Timing:** 0.5s at full intensity, then fade out over 1–1.5s.
- **Implementation:** CSS class with `@keyframes` fade triggered after a
  0.5s `animation-delay`.

### Removals

- **Appearance:** Line stays in the DOM with a "departing" class. This is
  required — removing the element immediately would skip the animation.
- **Phase 1 — Glow:** Amber/warm background tint (`rgba(251,191,36,0.15)`)
  with amber left border. Duration: ~0.3s.
- **Phase 2 — Collapse:** Height collapses to zero via `max-height`
  transition (~0.5–0.7s). Set explicit `max-height` from `scrollHeight`
  before transitioning to 0.
- **Cleanup:** On `transitionend`, remove the element from state/DOM.
- **Total:** ~0.8–1s.

### Design Rationale for Asymmetry

Additions appear instantly because the new text is immediately readable —
the highlight alone signals "this is new." Removals animate out because the
user needs to register what's leaving before it disappears (a spatial cue
that aids comprehension). This asymmetry is intentional and was validated
by the team.

### Theme Colors

| Element | Dark Theme | Light Theme |
|---------|-----------|-------------|
| Addition background | `rgba(74,222,128,0.15)` | `rgba(34,197,94,0.12)` |
| Addition border | `#4ade80` | `#22c55e` |
| Removal glow background | `rgba(251,191,36,0.15)` | `rgba(251,191,36,0.12)` |
| Removal glow border | `#f59e0b` | `#f59e0b` |

These should be defined as CSS custom properties on the panel, not
hardcoded per-line. The existing theme mechanism (PatternFly v6 dark class
toggle) should drive selection.

## Scroll Behavior

### Single-item toggle

Auto-scroll to the changed line using `scrollIntoView` with
`behavior: 'smooth'` (300–400ms ease-out). Skip the scroll if the
changed line is already within the visible area of the panel's scroll
container (check via `getBoundingClientRect` against the panel bounds).

### Bulk operations (multiple lines change)

Auto-scroll to the first affected line (topmost in document order).
Highlight all changed lines simultaneously, but only scroll to the
first one. The user can scroll down to see the rest.

### Post-scroll settle buffer

After scroll completes, wait 100–150ms before starting the highlight
animation. This gives the eye time to land on the scroll destination.

### Scroll interruption

If a scroll animation is in-flight and the user triggers another toggle,
cancel the current scroll and scroll to the new target. Highlights from
both toggles run independently with their own timers.

## Collapsed Panel Behavior

When the `ContainerfilePanel` is collapsed and the containerfile changes:

1. **Show a dot indicator** on the collapsed panel tab — simple
   presence/absence, no count or magnitude. A CSS pseudo-element or a
   small `<span>` is sufficient.
2. **On expand**, run the diff between the containerfile as it was when
   the panel was last open (or last expanded) and the current version.
   Apply the normal highlight animations to the cumulative diff.
3. **Clear the dot** once the panel is expanded and highlights have been
   shown.

The "last seen" containerfile string is captured when the panel
transitions from open to collapsed. This is the baseline for the
cumulative diff on re-expand.

## Accessibility

### Reduced Motion

When `prefers-reduced-motion: reduce` is active:

- All animation durations set to 0.
- Additions: line appears with the green background color, holds for 2s,
  then disappears instantly (no fade).
- Removals: line disappears immediately, no glow or collapse animation.
- Scroll: `behavior: 'auto'` (instant jump, no smooth scroll).
- Implementation: a single `@media (prefers-reduced-motion: reduce)` block
  that zeroes all transition and animation durations.

### Screen Reader

- Each refinement toggle produces an `aria-live="polite"` announcement.
  Format: `"[Item name] added to containerfile"` or `"[Item name] removed
  from containerfile"`.
- The live region is near the toggle control (in `DecisionList`), not in
  the preview panel.
- Do NOT announce scroll position changes.
- The containerfile panel already has `aria-label="Containerfile preview"`.

### Focus Management

Focus stays on the toggle control after every interaction. The preview
panel updates are peripheral feedback. Moving focus to the preview on
every toggle would be disorienting for keyboard and screen reader users.

## Edge Cases

### Rapid successive toggles

- Highlights fire immediately on every toggle — no debouncing of visual
  feedback.
- Scroll debounces with a ~150ms window. If multiple toggles fire in quick
  succession targeting different locations, only the last one drives the
  scroll.
- Each highlight has its own independent fade timer.

### Undo (toggling back)

No special treatment. If the user adds a package and immediately removes
it, the removal gets the standard amber glow + collapse. The containerfile
is the source of truth, not the animation history.

### Empty diff

If the containerfile string is identical before and after an API response
(possible in edge cases), do nothing — no highlights, no scroll, no dot.

### Panel auto-collapse on resize

If the browser is resized below 1280px and the panel auto-collapses while
highlights are active, cancel any running highlight animations and capture
the current containerfile as the "last seen" baseline. If changes happen
while collapsed at narrow width, the dot appears and cumulative highlights
play on re-expand (same as deliberate collapse).

## Implementation Approach

### Dependencies

- `diff` npm package (for `diffLines`) — lightweight, well-maintained,
  no transitive dependencies.

### Files touched

- `ContainerfilePanel.tsx` — main changes: render individual lines instead
  of a raw text block, apply highlight/departing classes, scroll logic,
  dot indicator on collapsed tab.
- `App.css` — new CSS: highlight keyframes, collapse transitions, dot
  indicator, reduced-motion overrides.
- New: `useContainerfileDiff.ts` hook — diff logic, highlight state
  management, previous-value tracking.
- `DecisionList.tsx` — add `aria-live` region for toggle announcements
  (if not already present).

### Key implementation detail

The containerfile preview currently renders as a single text block.
This feature requires rendering individual lines as separate elements
(e.g., `<div>` per line) so that each line can carry highlight/departing
classes and participate in CSS transitions. This is the primary structural
change to `ContainerfilePanel`.

## Out of Scope

- "Show changes" toggle overlay (diff mode) — potential future enhancement
  if users want to see cumulative changes, but not in this iteration.
- Change count or magnitude indicator on collapsed tab — ship the simple
  dot first, add smarts if requested.
- Transient "N lines changed" indicator near the toggle control — noted
  by Fern as a possible enhancement, deferred.
- Highlight replay for collapsed-panel changes — on expand, show the
  cumulative diff, not a step-by-step replay of intermediate states.
