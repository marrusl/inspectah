# Live Containerfile Preview

**Date:** 2026-03-31
**Status:** Proposed
**Author:** Kit (via brainstorm with Mark)

## Goal

Eliminate the re-render cycle for previewing Containerfile changes. When the user toggles any include/exclude, the Containerfile tab updates instantly via client-side JS. The server-side pipeline is repositioned as an export action ("Rebuild & Download").

## Context

Currently, every include/exclude toggle requires clicking Re-render, which spawns a full subprocess (`inspectah inspect --from-snapshot`), rebuilds all 6 output artifacts, replaces the entire page via `document.write()`, and takes 1-3 seconds. This breaks flow for iterative decisions.

The snapshot object is already in `window.snapshot` as a JS object. The Containerfile is displayed in a plain `<pre id="containerfile-pre">` block with no syntax highlighting. All the data needed to generate a preview is already client-side.

**Trust model:** The tab shows a **live preview** for fast iteration (“think”). It may intentionally differ from the Python renderer (see simplifications below). **Rebuild & Download** is the only supported path that produces the authoritative Containerfile and tarball together—users should treat that output as the artifact. We do not add UI that implies the preview text is copy-paste authority.

## Design

### Client-Side Preview Generator

A new function `generateContainerfilePreview()` that reads `window.snapshot` and produces Containerfile text. It runs:
- On page load (replaces the server-rendered Containerfile with the live version)
- On every include/exclude toggle
- On every variant selection change
- On config editor save
- On prevalence slider change (if the threshold change affects which items are included)

**Implementation notes:**
- Write preview output to `#containerfile-pre` using `textContent`, not `innerHTML`, to prevent injection.
- Deep clone the snapshot for `window.originalSnapshot` via `JSON.parse(JSON.stringify(snapshot))`. A reference copy would let Discard silently corrupt state.
- **Dirty state lifecycle:** Dirty state is set on any include toggle, variant selection, config edit, or prevalence slider change. Dirty state is cleared when Discard is confirmed or Rebuild & Download succeeds. The Discard confirmation dialog only appears when dirty state is set.
- **Baseline refresh on rebuild:** After a successful Rebuild & Download, `window.originalSnapshot` is updated to a deep clone of the current `window.snapshot` (`JSON.parse(JSON.stringify(window.snapshot))`). This means Discard after a rebuild returns to the post-rebuild state, not the original page-load state. Rationale: once the user has committed via Rebuild & Download, that output is the new authoritative baseline — discarding further back would be confusing and inconsistent with the trust model.

**Sections to generate (matching the Python renderer `_core.py` order):**

1. **Packages** — `FROM` line using `snapshot.rpm.base_image` (or resolved via `_base_image_from_snapshot` fallback chain), followed by `RUN dnf install -y` with all `snapshot.rpm.packages_added` where `include === true`, sorted alphabetically. Line-wrapped with backslash continuation for readability.
2. **Services** — `RUN systemctl enable/disable` from `snapshot.services.state_changes` where `include === true`, grouped by action
3. **Network (firewall)** — firewall zone configuration from `snapshot.network` where `include === true`
4. **Scheduled tasks** — `COPY` for timer units and cron files where `include === true`
5. **Config files** — `COPY` lines from `snapshot.config.files` where `include === true`, mapping paths from the config tree
6. **Non-RPM software** — `RUN pip install` for Python packages from `snapshot.non_rpm_software` where `include === true`
7. **Containers** — `COPY` for quadlet units (`snapshot.containers.quadlet_units`) and compose files (`snapshot.containers.compose_files`) where `include === true`. These are separate sub-lists on `ContainerSection`, not a flat iterable.
8. **Users/Groups** — `RUN useradd/groupadd` from `snapshot.users_groups` where `include === true`
9. **Kernel boot** — kargs.d drop-in where `include === true`
10. **SELinux** — `RUN semanage port` for port labels where `include === true`
11. **Epilogue** — `RUN bootc container lint` (always present)

The second `network` call in `_core.py` (`firewall_only=False`) produces NM connection informational comments, not actionable Containerfile instructions. It is omitted from the preview, consistent with the "no detailed comments" simplification.

**Simplifications vs Python renderer:**
- No FIXME annotations (those are triage-level, not preview-level)
- No multistage build logic for C-extension pip packages (preview shows `RUN pip install`, export handles the complexity)
- No DHCP connection filtering (include all network items that pass the `include` check)
- No detailed comments or section headers (clean output, not annotated)
- No tmpfiles epilogue (comment block in the Python renderer; omitted consistent with "no detailed comments" simplification)
- Sections with zero included items are omitted entirely

**These simplifications are acceptable** because the preview shows intent ("these packages will be installed") accurately. The export via "Rebuild & Download" produces the full Python-rendered Containerfile with all annotations and edge case handling.

### UX Changes

**Toolbar restructuring:**

| Current | New | ID |
|---------|-----|-----|
| Re-render (triggers full pipeline) | **Rebuild & Download** (triggers full pipeline + tarball download) | `#btn-re-render` (reused, relabeled) |
| Reset (restores original state) | **Discard** (with confirmation dialog) | `#btn-reset` |
| Download Tarball | Remove (merged into Rebuild & Download) | `#btn-tarball` (removed) |
| Download Snapshot | Keep as-is | `#btn-download-snapshot` |

**Rebuild & Download behavior (strict ordering):**
1. Refresh `window.snapshot` from the server response
2. Update `#containerfile-pre` with the authoritative Python-rendered Containerfile via `textContent`
3. Deep clone `window.snapshot` into `window.originalSnapshot` (new Discard baseline)
4. Call `recalcTriageCounts()` (audit summary reflects post-rebuild state)
5. Clear dirty state
6. Trigger tarball download via `/api/tarball`
7. Show success toast

Note: tarball download is step 6 (after state is fully consistent) so that if the download fails, all client state is already correct.

**Rebuild & Download client contract:**
- **Re-render success:** The server returns the full page HTML (same `/api/re-render` endpoint as today). However, instead of replacing the page via `document.write()`, the client extracts the Python-rendered Containerfile from the response and updates `#containerfile-pre` via `textContent`. The client also refreshes `window.snapshot` from the response's embedded snapshot object. This is a change from the current behavior where `document.write()` replaces the entire page — this spec requires in-place update so that client-side state (dirty tracking, event listeners, original snapshot) is preserved.
- **Re-render success + tarball download failure:** The Containerfile text and `window.snapshot` are still updated (the re-render succeeded). Dirty state clears and `window.originalSnapshot` is refreshed (deep clone), because the authoritative rebuild succeeded — the failure is artifact delivery, not state generation; the user's state is consistent and they just need to retry the download. Toast shows a warning about the download failure. The user can retry via the existing `/api/tarball` endpoint directly (e.g., a "Retry Download" link in the toast) or re-run Rebuild & Download.
- **Re-render failure:** Nothing changes. Toast shows the error. Preview stays as-is. `window.snapshot` and `window.originalSnapshot` are untouched.

**Discard with confirmation:**
```
"Discard all edits?"
This will revert the Containerfile to its last built state.
[Discard] [Cancel]
```
(If no rebuild has occurred this session, "last built state" means the original page-load state — one string covers both cases.)
On confirm: restore `window.snapshot` from `window.originalSnapshot`, re-run `generateContainerfilePreview()`, update all summary counts and badges. Dirty state is cleared.

**Containerfile tab — preview cue:** A single helper line sits directly above `#containerfile-pre` (subtitle-style, low visual weight). **Exact copy:**

> Live preview — updates as you edit. Rebuild & Download produces the Containerfile in your tarball.

The words **Rebuild & Download** match the toolbar button label (same casing); in the UI they may be styled emphasized (e.g. `<strong>`) so the path to the artifact is scannable. No other chrome or redesign of the tab.

### Containerfile Tab

The static server-rendered Containerfile content in `#containerfile-pre` is replaced on page load by the client-side preview generator output. The tab title stays **Containerfile**; treat it mentally as a **live preview** (fast feedback while editing includes/variants), not as a published artifact.

**Preview cue (in-product):** Same helper line as in UX Changes — placed immediately above `#containerfile-pre`. **Exact copy:**

> Live preview — updates as you edit. Rebuild & Download produces the Containerfile in your tarball.

**No Copy button** on this tab: we do not facilitate one-click copy from preview, because that implied the clipboard matched export when the preview can differ. Users may still select text in `#containerfile-pre` manually if they wish.

After **Rebuild & Download** succeeds, `#containerfile-pre` shows the same Python-rendered Containerfile that ships in the tarball—the artifact-producing path.

### Hooking Into Existing Events

The preview generator hooks into existing event paths:

| Event | Current behavior | New behavior (added) |
|-------|-----------------|---------------------|
| Include toggle click | Updates dirty state, enables Rebuild & Download | + calls `generateContainerfilePreview()` and `recalcTriageCounts()` |
| Variant selection change | Updates dirty state | + calls `generateContainerfilePreview()` and `recalcTriageCounts()` |
| Config editor save | Marks dirty | + calls `generateContainerfilePreview()` and `recalcTriageCounts()` |
| Prevalence slider input | Updates summary cards | + calls `generateContainerfilePreview()` and `recalcTriageCounts()` only when the threshold change affects which items are included (not on every pixel of slider movement) |
| Discard click | Restores original snapshot | + confirmation dialog, + calls `generateContainerfilePreview()` and `recalcTriageCounts()` |
| Page load | Renders server-side Containerfile | + calls `generateContainerfilePreview()` to replace with live version; `recalcTriageCounts()` sets initial counts |

### Audit Report

The audit report's **detail tables** are NOT live-previewed — they stay server-rendered. However, the **executive summary counts** (packages added, config files, containers, triage breakdown by automatic/review/manual) update live on every include/exclude toggle, variant selection, and prevalence slider threshold change via `recalcTriageCounts()`.

**Audit tab — preview cue:** A single helper line sits at the top of the audit report (subtitle-style, low visual weight, same pattern as the Containerfile cue). **Exact copy:**

> Summary counts update as you edit. Detail tables refresh on Rebuild & Download.

- Reuses the existing `recalcTriageCounts()` function (~30 lines of lightweight JS)
- Updates section item counts and triage category totals in the summary cards
- Detail tables and per-item rows remain server-rendered (no full audit rebuild on toggle)
- Performance: <1ms per update

## Scope

**In scope:**
- client-side preview generator
- Toolbar restructuring (Rebuild & Download, Discard confirmation)
- Hook into all include/exclude/variant/editor events
- One-line preview helper above `#containerfile-pre` (exact copy specified in UX / Containerfile Tab sections)
- Remove Containerfile tab Copy button and its clipboard handler (avoid preview-as-authority UX)
- Live audit executive summary counts — on every include/exclude toggle, variant selection, and prevalence slider threshold change, update the audit report's executive summary counts (packages added, config files, containers, triage breakdown) via `recalcTriageCounts()` and lightweight JS (~30 lines). Detail tables remain server-rendered.
- E2E tests for live preview behavior, preview helper visibility, absence of Copy affordance on the Containerfile tab, and audit count updates

**Out of scope:**
- Full live audit report preview (detail tables, per-item rows)
- Multistage build preview for C-extension pip packages
- FIXME annotations in preview
- DHCP connection filtering in preview
- "Refresh audit" button (follow-on)
- Preview-vs-export copy labeling or switching what Copy does (superseded by removing Copy entirely)

## Files to Modify

- `src/inspectah/templates/report/_js.html.j2` — add `generateContainerfilePreview()`, hook into events; remove Copy-to-clipboard logic for the Containerfile tab
- `src/inspectah/templates/report/_containerfile.html.j2` — add preview helper line above `#containerfile-pre` (exact copy per spec); remove Copy button (`#btn-copy-cf`)
- `src/inspectah/templates/report/_toolbar.html.j2` — rename Re-render, remove Download Tarball; drop `.btn-copy-cf` styling if it becomes unused
- `src/inspectah/templates/report/_css.html.j2` — confirmation dialog styling (if needed)
- `tests/e2e/tests/re-render-cycle.spec.ts` — update for new button labels and behavior
- New or extended E2E tests for live preview behavior and no Copy control on Containerfile tab

## Testing

### E2E Tests

| Test | Assertion |
|------|-----------|
| Containerfile updates on package toggle | Toggle a package, verify `#containerfile-pre` text changes immediately (no button click needed) |
| Containerfile updates on variant selection | Select a config variant, verify COPY line appears/changes in preview |
| Containerfile updates on prevalence slider | Set prevalence slider to a value that crosses a known inclusion threshold in the fixture data (e.g., from 60% to 40% where at least one package's prevalence falls between), verify `#containerfile-pre` package list gains or loses the affected package |
| Discard shows confirmation | Click Discard with pending changes, verify dialog appears |
| Discard confirmed restores original | Confirm Discard, verify Containerfile matches original |
| Rebuild & Download triggers full pipeline | Click Rebuild & Download, verify tarball download starts |
| Preview helper visible | Open Containerfile tab, assert the helper line contains the exact preview cue copy (live preview vs Rebuild & Download) |
| No Copy on Containerfile tab | Open Containerfile tab, assert no Copy / `#btn-copy-cf` (preview is not a one-click export surface) |
| After Rebuild & Download, preview matches export | Toggle to dirty preview, run Rebuild & Download, assert `#containerfile-pre` matches server-rendered content (e.g. stable marker or snapshot from response) |
| Audit counts update on package toggle | Toggle a package include off, verify audit executive summary triage count decrements by one (requires fixture with known item counts) |
| Audit counts update on prevalence slider | Move prevalence slider across a known threshold, verify executive summary section counts reflect the changed inclusions (requires fixture with known item counts) |

### Python Tests

No new Python tests needed — the server-side pipeline is unchanged. The JS generator is tested via E2E.
