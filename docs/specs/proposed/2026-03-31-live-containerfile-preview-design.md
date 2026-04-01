# Live Containerfile Preview

**Date:** 2026-03-31
**Status:** Proposed
**Author:** Kit (via brainstorm with Mark)

## Goal

Eliminate the re-render cycle for previewing Containerfile changes. When the user toggles any include/exclude, the Containerfile tab updates instantly via client-side JS. The server-side pipeline is repositioned as an export action ("Rebuild & Download").

## Context

Currently, every include/exclude toggle requires clicking Re-render, which spawns a full subprocess (`yoinkc inspect --from-snapshot`), rebuilds all 6 output artifacts, replaces the entire page via `document.write()`, and takes 1-3 seconds. This breaks flow for iterative decisions.

The snapshot object is already in `window.snapshot` as a JS object. The Containerfile is displayed in a plain `<pre id="containerfile-pre">` block with no syntax highlighting. All the data needed to generate a preview is already client-side.

**Trust model:** The tab shows a **live preview** for fast iteration (“think”). It may intentionally differ from the Python renderer (see simplifications below). **Rebuild & Download** is the only supported path that produces the authoritative Containerfile and tarball together—users should treat that output as the artifact. We do not add UI that implies the preview text is copy-paste authority.

## Design

### JS Containerfile Preview Generator

A new function `generateContainerfilePreview()` that reads `window.snapshot` and produces Containerfile text. It runs:
- On page load (replaces the server-rendered Containerfile with the live version)
- On every include/exclude toggle
- On every variant selection change
- On config editor save
- On prevalence slider change (if it affects included items)

**Sections to generate (matching the Python renderer order):**

1. **FROM** — `snapshot.meta.base_image` or fallback
2. **Packages** — `RUN dnf install -y` with all `snapshot.rpm.packages_added` where `include === true`, sorted alphabetically. Line-wrapped with backslash continuation for readability.
3. **Services** — `RUN systemctl enable/disable` from `snapshot.services.state_changes` where `include === true`, grouped by action
4. **Config files** — `COPY` lines from `snapshot.config.files` where `include === true`, mapping paths from the config tree
5. **Containers** — `COPY` for quadlet units and compose files from `snapshot.containers` where `include === true`
6. **Non-RPM software** — `RUN pip install` for Python packages from `snapshot.non_rpm_software` where `include === true`
7. **Users/Groups** — `RUN useradd/groupadd` from `snapshot.users_groups` where `include === true`
8. **Scheduled tasks** — `COPY` for timer units and cron files where `include === true`
9. **Network** — firewall zone configuration where `include === true`
10. **SELinux** — `RUN semanage port` for port labels where `include === true`
11. **Kernel boot** — kargs.d drop-in where `include === true`
12. **Epilogue** — `RUN bootc container lint` (always present)

**Simplifications vs Python renderer:**
- No FIXME annotations (those are triage-level, not preview-level)
- No multistage build logic for C-extension pip packages (preview shows `RUN pip install`, export handles the complexity)
- No DHCP connection filtering (include all network items that pass the `include` check)
- No detailed comments or section headers (clean output, not annotated)
- Sections with zero included items are omitted entirely

**These simplifications are acceptable** because the preview shows intent ("these packages will be installed") accurately. The export via "Rebuild & Download" produces the full Python-rendered Containerfile with all annotations and edge case handling.

### UX Changes

**Toolbar restructuring:**

| Current | New | ID |
|---------|-----|-----|
| Re-render (triggers full pipeline) | **Rebuild & Download** (triggers full pipeline + tarball download) | `#btn-re-render` (reused, relabeled) |
| Reset (restores original state) | **Reset** (with confirmation dialog) | `#btn-reset` |
| Download Tarball | Remove (merged into Rebuild & Download) | `#btn-tarball` (removed) |
| Download Snapshot | Keep as-is | `#btn-download-snapshot` |

**Rebuild & Download behavior:**
1. Runs the full `run_all()` pipeline via `/api/re-render` (same server endpoint)
2. On success, automatically triggers tarball download via `/api/tarball`
3. Updates `#containerfile-pre` with the authoritative Python-rendered version (preview and export stay aligned after a successful rebuild)
4. Shows success toast

**Reset with confirmation:**
```
"You have unsaved changes. Reset to original state?"
[Reset] [Cancel]
```
On confirm: restore `window.snapshot` from `window.originalSnapshot`, re-run `generateContainerfilePreview()`, update all summary counts and badges.

### Containerfile Tab

The static server-rendered Containerfile content in `#containerfile-pre` is replaced on page load by the JS preview output. The tab title stays **Containerfile**; treat it mentally as a **live preview** (fast feedback while editing includes/variants), not as a published artifact.

**No Copy button** on this tab: we do not facilitate one-click copy from preview, because that implied the clipboard matched export when the preview can differ. Users may still select text in `#containerfile-pre` manually if they wish.

After **Rebuild & Download** succeeds, `#containerfile-pre` shows the same Python-rendered Containerfile that ships in the tarball—the artifact-producing path.

### Hooking Into Existing Events

The preview generator hooks into existing event paths:

| Event | Current behavior | New behavior (added) |
|-------|-----------------|---------------------|
| Include toggle click | Updates dirty state, enables Rebuild & Download | + calls `generateContainerfilePreview()` |
| Variant selection change | Updates dirty state | + calls `generateContainerfilePreview()` |
| Config editor save | Marks dirty | + calls `generateContainerfilePreview()` |
| Prevalence slider input | Updates summary cards | + calls `generateContainerfilePreview()` |
| Reset click | Restores original snapshot | + confirmation dialog, + calls `generateContainerfilePreview()` |
| Page load | Renders server-side Containerfile | + calls `generateContainerfilePreview()` to replace with live version |

### Audit Report

The audit report is NOT live-previewed. It stays server-rendered. A manual "Refresh audit" button can be added as a follow-on if needed, but it's not in scope for this spec. The audit tab shows the last-rendered version.

## Scope

**In scope:**
- JS Containerfile preview generator
- Toolbar restructuring (Rebuild & Download, Reset confirmation)
- Hook into all include/exclude/variant/editor events
- Remove Containerfile tab Copy button and its clipboard handler (avoid preview-as-authority UX)
- E2E tests for live preview behavior and absence of Copy affordance on the Containerfile tab

**Out of scope:**
- Live audit report preview
- Multistage build preview for C-extension pip packages
- FIXME annotations in preview
- DHCP connection filtering in preview
- "Refresh audit" button (follow-on)
- Preview-vs-export copy labeling or switching what Copy does (superseded by removing Copy entirely)

## Files to Modify

- `src/yoinkc/templates/report/_js.html.j2` — add `generateContainerfilePreview()`, hook into events; remove Copy-to-clipboard logic for the Containerfile tab
- `src/yoinkc/templates/report/_containerfile.html.j2` — remove Copy button (`#btn-copy-cf`)
- `src/yoinkc/templates/report/_toolbar.html.j2` — rename Re-render, remove Download Tarball; drop `.btn-copy-cf` styling if it becomes unused
- `src/yoinkc/templates/report/_css.html.j2` — confirmation dialog styling (if needed)
- `tests/e2e/tests/re-render-cycle.spec.ts` — update for new button labels and behavior
- New or extended E2E tests for live preview behavior and no Copy control on Containerfile tab

## Testing

### E2E Tests

| Test | Assertion |
|------|-----------|
| Containerfile updates on package toggle | Toggle a package, verify `#containerfile-pre` text changes immediately (no button click needed) |
| Containerfile updates on variant selection | Select a config variant, verify COPY line appears/changes in preview |
| Containerfile updates on prevalence slider | Move slider, verify package list changes in preview |
| Reset shows confirmation | Click Reset with pending changes, verify dialog appears |
| Reset confirmed restores original | Confirm Reset, verify Containerfile matches original |
| Rebuild & Download triggers full pipeline | Click Rebuild & Download, verify tarball download starts |
| No Copy on Containerfile tab | Open Containerfile tab, assert no Copy / `#btn-copy-cf` (preview is not a one-click export surface) |
| After Rebuild & Download, preview matches export | Toggle to dirty preview, run Rebuild & Download, assert `#containerfile-pre` matches server-rendered content (e.g. stable marker or snapshot from response) |

### Python Tests

No new Python tests needed — the server-side pipeline is unchanged. The JS generator is tested via E2E.
