# Fleet Refine Lifecycle

**Date:** 2026-03-16
**Status:** Proposed
**Part of:** Fleet Refine (Spec 2 of 3)
**Depends on:** Spec 1 (Fleet Merge Completeness)

## Problem

Fleet reports from `inspectah-fleet aggregate` are currently read-only. Users can
see prevalence data (which items are on how many hosts) but cannot act on it —
no toggles, no threshold adjustment, no re-render. The refine workflow that
exists for single-host snapshots (`inspectah-refine`) does not activate fleet-
specific controls when given a fleet snapshot.

This spec enables full refine mode on fleet snapshots: toggles, reset,
re-render, a prevalence slider for threshold adjustment, and radio-button
variant selection for content-bearing items.

## Context: Fleet Refine Decomposition

1. **Fleet Merge Completeness** (Spec 1, code complete) — merge for selinux
   and non_rpm_software, storage suppression
2. **Fleet Refine Lifecycle** (this spec) — toggles, reset, re-render,
   prevalence slider, variant radio groups
3. **Fleet Config Editor with Variant Awareness** — variant comparison, pick
   canonical variant, diff view

## Decisions

### Fleet Detection & Refine Mode Entry

`inspectah-refine` (the HTTP wrapper) does not change. It remains a dumb server
that extracts tarballs, invokes inspectah for re-renders, and serves the result.

inspectah itself detects fleet mode by checking `snapshot.meta.get("fleet")`.
When fleet metadata is present AND `--refine-mode` is set, the template
renders fleet-specific refine controls in addition to all existing refine
controls (toggles, reset, re-render, editor).

No new CLI flags. Fleet refine is an automatic consequence of refining a
fleet snapshot.

### Prevalence Slider

**Placement:** In the sticky refine toolbar, separated from the Re-render and
Reset buttons by a vertical divider. Visible only when `fleet_meta` is
present and `refine_mode` is true.

**Components:**
- Range input (1-100)
- Current value label (updates live during drag)
- Preview count ("would include X / exclude Y", updates during drag)
- Apply button (appears when slider value differs from current threshold)

**Interaction model:**

1. User drags slider — value label and preview counts update in real-time.
   This is a client-side calculation against all items'
   `fleet.count / fleet.total`. No state changes yet.
2. An "Apply" button appears next to the slider when the value differs from
   the current threshold.
3. User clicks Apply — all items recalculate `include` based on the new
   threshold. Toggles and excluded-row CSS update instantly. Sidebar triage
   counts also update client-side (new work — existing triage counts are
   server-rendered, so a client-side recalculation function must be built).
   No re-render needed.
4. For content variant groups: after threshold recalculation, the radio
   constraint applies — only the most prevalent variant per path gets
   `include: true` (see Variant Radio Groups below).

**Initial value:** `snapshot.meta.fleet.min_prevalence` — the threshold used
during `inspectah-fleet aggregate`.

**State tracking:** The slider value is tracked client-side only. It is not
persisted in the snapshot. The snapshot records the resulting `include` values,
which is what matters for Containerfile generation. When re-render is
triggered, the modified snapshot (with updated includes) is sent to inspectah.

### Reset Interaction

The Reset button restores the entire snapshot to its original state, including:
- All `include` values back to original
- Slider position back to original `min_prevalence` value
- All manual toggle overrides cleared
- Editor dirty state cleared (existing behavior — note that `resetToOriginal()`
  restores `include` and `strategy` fields but does not revert edited file
  content; content restoration is an existing limitation)

Reset is the "nuclear option." The slider is the softer re-threshold tool.

### Threshold Comparison

The slider uses the same comparison as the merge engine's
`_prevalence_include()`: `(count * 100) >= (min_prevalence * total)`. An item
at exactly the threshold percentage is included. This must be replicated in
the client-side JavaScript for consistency.

### Content Variant Radio Groups

Fleet snapshots can have multiple content variants per path — e.g., 3
different versions of `/etc/httpd/conf/httpd.conf` from different hosts. The
Containerfile needs exactly one version of each file.

**Radio-button behavior:** Within a variant group (same path, different
content), toggles behave as radio buttons. Enabling one auto-disables the
others. This applies to content-variant item types that have variant grouping
in the renderer: config files, quadlet units, and systemd drop-ins. Compose
files use a different variant key (service/image tuple) and are not currently
grouped by the renderer — compose variant grouping is out of scope.

**Default selection:** The most prevalent variant (highest `fleet.count`). If
the most prevalent variant is below the threshold, all variants start excluded
and the user must manually pick one.

**Slider interaction:** When the slider recalculates include/exclude, the
radio constraint applies after threshold calculation. Only the most prevalent
variant per path that meets the threshold gets `include: true`. Others in the
group stay excluded.

**All-excluded:** If the user excludes all variants of a path, that file is
omitted from the Containerfile entirely.

**Template changes:** Variant group rows (already rendered with
`fleet-variant-children` CSS class) need a `data-variant-group` attribute so
the toggle handler can identify siblings. The toggle handler adds
sibling-deselect logic when the item belongs to a variant group. Affected
template partials: `_config.html.j2`, `_containers.html.j2` (quadlets only),
`_services.html.j2` (drop-ins).

### Existing Controls

All existing refine controls work as-is on fleet snapshots:
- **Toggles** — already rendered but hidden via CSS unless `refine_mode` is
  true. Enabling refine mode on a fleet snapshot shows them. No changes needed
  beyond the variant radio-group behavior above.
- **Re-render** — same POST to `/api/re-render` with modified snapshot. inspectah
  renders the Containerfile from included items.
- **Editor** — works on individual items. Content variant awareness is Spec 3.

## Testing

### Unit Tests (client-side logic via snapshot manipulation)
- Slider recalculation: items above new threshold get `include: true`, below
  get `include: false`
- Variant radio constraint: after recalculation, only most prevalent variant
  per path is included
- Variant toggle: enabling variant B auto-disables variant A within same path
  group
- Variant all-excluded: excluding all variants of a path produces no file in
  Containerfile
- Reset restores original snapshot state including slider value
- Slider Apply vs drag: preview counts update during drag, actual state only
  changes on Apply

### Integration Tests (refine lifecycle)
- Fleet tarball via `inspectah-refine` loads fleet-aware refine UI (slider
  present, prevalence bars visible, toggles enabled)
- Slider change, Apply, then re-render produces correct Containerfile with
  new threshold's included items
- Reset after slider change plus manual toggles restores everything to
  original state

### Not Tested (covered elsewhere)
- `inspectah-refine` wrapper — no changes
- Fleet merge engine — covered by Spec 1
- Basic toggle/reset/re-render mechanics — already tested in single-host
  refine

## Out of Scope

- Variant comparison/diff UI (Spec 3)
- Config editor variant awareness (Spec 3)
- `inspectah-refine` wrapper changes (none needed)
- New CLI flags (none needed)
- Slider value persistence in snapshot (UI-only state)
- Per-section prevalence thresholds (global slider only)
- Target image override (invalidates baseline)
