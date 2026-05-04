# Triage UX Fixups — Design Spec

**Status:** Approved (revision 5)
**Date:** 2026-05-04
**Context:** Mark's live testing feedback after the single-machine triage redesign + UX fixes landed on `go-port`. These must be addressed before moving to fleet verification (cutover plan item 1).
**Inputs:** Pre-spec (`2026-05-04-triage-ux-fixups-prespec.md`), Ember strategic input, Fern interaction design input.
**Revision 2 notes:** Addresses round 1 review blockers: default-inclusion state contract (snapshot normalization, not classifier-only), version-changes scoped to current data model (upgrade/downgrade only) with full interaction contract, secrets split by redaction kind with symmetric decision paths, preview-pane accessible splitter + localStorage persistence, kernel-module heuristic narrowed to `modules-load.d` only.
**Revision 3 notes:** Addresses round 2 blockers: service/cron toggle-to-renderer synchronization contract (rebuild `EnabledUnits`/`DisabledUnits` from `StateChanges` on re-render), secrets expanded to true three-state model (`excluded`/`inline`/`flagged`), version-changes filter uses true radio-group keyboard model, secret card post-action focus rule defined, kernel modules relabeled from output-affecting to informational (comment-only on current branch).
**Revision 4 notes:** Addresses round 3 blockers: service sync uses live action enum (`enable`/`disable`/`mask`/`unchanged`), cron sync propagates to all GeneratedTimerUnits sharing a SourcePath, sync pinned at `nativeReRender()` seam (after unmarshal, before re-serialization), secrets join key changed from path to index (`secret-<n>` → `redactions[n]`), secret override artifact reconciliation defined (`reconcileSecretOverrides`), stale "two kinds" copy fixed, modprobe.d Config-section excludability overclaim removed.
**Revision 5 notes:** Addresses round 4 blockers: masked services collapsed into `DisabledUnits` as lossy approximation (no separate render path on current branch), stale Item 5 renderer claim fixed, secrets reconciliation rewritten as canonical-preserving derived view (`reconciledRedactions`) covering all three override directions (`excluded→included`, `inline→excluded`, `flagged→excluded`) and all artifact consumers (`secretsCommentLines`, `WriteRedactedDir`, `RenderSecretsReview`, audit/README counts).

---

## Design Principles

**Product posture: recommendation engine with overrides.** Inspectah generates a recommended migration plan. The user reviews and adjusts, not decides from scratch. Every default reflects inspectah's opinion about what belongs in the image.

**Default inclusion model:** In single-machine mode, everything is included by default unless inspectah has a specific reason to exclude it (image-mode incompatibility). The interesting signal is what to *exclude*, not what to include.

**Migration preview framing:** Version changes use prospective framing ("what will change when you move to the target image") rather than retrospective framing ("what drifted on your host"). This positions inspectah as a migration planning tool.

**Scope:** Single-machine triage mode only. Fleet mode is unaffected.

---

## Items In Scope

16 items from the pre-spec, organized into 4 implementation tiers.

### Out of Scope
- **Item 7 (container section confusion):** Deferred to separate investigation. Current state is unclear — "confusing" and "undo may be broken" needs Kit to investigate before scoping.

### Scope Changed
- **Item 9 (fstab acknowledgment):** Simplified from "remove acknowledgment" to "passive reference in the Informational group." Fstab entries are not relevant to image creation — they are a deploy-time concern. All acknowledgment interaction is removed; entries become read-only display items inside the Informational wrapper (see Item 13, Tier 3).

---

## Tier 1: Default Inclusion Sweep

*High impact, low cost. Biggest UX win — eliminates false "needs decision" friction across the entire tool.*

All items in this tier share the same root cause: the triage UI treats included items as needing decisions. The fix is the same pattern for each: flip the default to included, change the color from amber to green, update the label.

### Items 1, 4, 6, 8, 10, 12 — Flip to Included-by-Default

| Item | Section | Surface | Snapshot backing structure |
|------|---------|---------|--------------------------|
| 1 | Packages | Tier 2 packages from repos | `snap.Rpm.PackagesAdded[].Include` |
| 4 | Config | All config files | `snap.ConfigFiles[].Include` |
| 6 | Runtime | Services with state changes, cron jobs | `snap.Services.StateChanges[].Include`, `snap.CronJobs[].Include` |
| 8 | Containers | Quadlet units | `snap.Containers.QuadletUnits[].Include` |
| 10 | System | Firewall rules | `snap.Firewall.Zones[].Include` |
| 12 | System | Sysctl overrides | `snap.Sysctl.Overrides[].Include` |

#### State contract: snapshot normalization

The authoritative source of inclusion state is the snapshot's backing structures, not the classifier manifest. The classifier's `DefaultInclude` field tells the SPA what the original default was (for decided-state tracking), but the snapshot's `Include` field is what the renderer reads when generating artifacts.

**Normalization function:** A new `normalizeIncludeDefaults(snap, isFleet)` function runs at the same point as the existing `normalizeLeafDefaults` — BEFORE the immutable sidecar snapshot is created. This ensures both the working snapshot and the sidecar agree on defaults.

For each affected surface, when `!isFleet`:
- Set `Include = true` on all items that currently default to `Include = false` (or nil/unset)
- The classifier then reads these values via `DefaultInclude: item.Include` in `classifyAll`, so the manifest and snapshot agree

This follows the same pattern as `normalizeLeafDefaults` for packages: normalize the snapshot, then let the classifier read from it. The classifier does not independently set defaults.

**Rendering contract:** Items with `Include: true` render green ("included"). The amber "needs decision" state is reserved for items where the snapshot's `Include` is false/nil and the user has not yet acted. After normalization, no single-machine tier 2 items will be in that state.

**Autosave roundtrip:** When the user toggles an item off, `updateSnapshotInclude()` sets `Include = false` on the snapshot structure, which flows through autosave → re-render → Containerfile. When toggled back on, `Include = true` is restored. The normalization step is idempotent — re-normalizing an already-normalized snapshot is a no-op.

#### Service and cron toggle-to-renderer synchronization

The SPA writes service decisions to `snap.services.state_changes[].include` and cron decisions to `snap.scheduled_tasks.cron_jobs[].include`. But the Containerfile renderer reads service enable/disable from `snap.Services.EnabledUnits` / `snap.Services.DisabledUnits`, and cron-derived output from `snap.ScheduledTasks.GeneratedTimerUnits[].Include`. These are separate structures — toggling `StateChanges[].Include` alone does not change the renderer's output.

**Synchronization strategy: rebuild on re-render.** A new `syncServiceDecisions(snap)` function rebuilds `EnabledUnits` and `DisabledUnits` from `StateChanges`. This uses the live `ServiceStateChange.Action` enum values from `inspector/services.go`:

```
For each entry in StateChanges:
  If entry.Include == false:
    Remove entry.Unit from EnabledUnits (if present)
    Remove entry.Unit from DisabledUnits (if present)
  Else:
    If entry.Action == "enable":
      Add entry.Unit to EnabledUnits (if not present)
    If entry.Action == "disable":
      Add entry.Unit to DisabledUnits (if not present)
    If entry.Action == "mask":
      // No separate masked-unit render path exists on the current branch.
      // Collapse mask into DisabledUnits as a lossy-but-safe approximation:
      // the renderer emits `systemctl disable` which is weaker than `mask`
      // but directionally correct. A real `systemctl mask` output path is
      // deferred to a future spec.
      Remove entry.Unit from EnabledUnits (if present)
      Add entry.Unit to DisabledUnits (if not present)
    If entry.Action == "unchanged":
      // No-op — unit stays in whatever list it was in
```

**Cron synchronization:** A `syncCronDecisions(snap)` function propagates `CronJobs[].Include` to `GeneratedTimerUnits`. The inspector (`inspector/scheduled.go:parseCronEntries()`) can emit multiple `GeneratedTimerUnit` entries for a single cron file (one per cron line). The sync rule: **all `GeneratedTimerUnit` entries whose `SourcePath` matches the `CronJob.Path` inherit that cron file's `Include` value.**

```
For each CronJob:
  For each GeneratedTimerUnit where unit.SourcePath == cronJob.Path:
    Set unit.Include = cronJob.Include
```

**Placement in the re-render pipeline:** Both sync functions run inside `nativeReRender()` in `cli/refine.go`, immediately after JSON unmarshal of the incoming snapshot and **before** both rendering and re-serialization. This ensures:
1. The Containerfile renderer sees the synchronized lists
2. The returned `ReRenderResult.Snapshot` carries the synchronized state
3. The exported `inspection-snapshot.json` written to disk also reflects the sync

The SPA continues to write only `StateChanges[].Include` and `CronJobs[].Include` — the server handles all downstream synchronization.

**Test obligations:**
- Toggle service off → autosave → re-render → unit absent from `EnabledUnits`/`DisabledUnits` AND absent from Containerfile `systemctl` lines
- Toggle cron off → autosave → re-render → all matching `GeneratedTimerUnits` have `Include: false` AND generated timer absent from Containerfile
- Multi-line cron file: toggling one cron file off excludes all generated timer units from that file
- Returned/exported snapshot agrees with rendered Containerfile on all service and cron decisions

**Accessibility:** ARIA labels change from "needs decision" to "included" for affected items. Screen reader announcements must reflect the new state.

### Item 5 — Incompatible Services Excluded-by-Default

Services on the incompatible list (`dnf-makecache`, `packagekit`, and future additions) default to **excluded** rather than flagged-and-includable. These cannot work in image mode — inclusion is provably wrong.

**Snapshot normalization:** `normalizeIncludeDefaults` sets `Include = false` for services matching the incompatible list when `!isFleet`. The classifier reads this as `DefaultInclude: false`, so the manifest correctly shows these items as excluded-by-default.

**Render suppression:** The Containerfile renderer emits `systemctl enable/disable` from `snap.Services.EnabledUnits` / `snap.Services.DisabledUnits` — it does NOT read `StateChanges[].Include` directly. The normalization must ensure that incompatible services with `Include = false` are removed from `EnabledUnits` (and `DisabledUnits` if applicable). The `syncServiceDecisions()` function then maintains this invariant on every subsequent re-render after user interactions.

**Rendering change:** These render with a muted/excluded treatment and a brief explanation: "This service assumes package management at runtime, which is unavailable in image mode." The user can still override to include, but the default is exclusion.

### Item 2 — `fedora` Repo Always-Included

Extend the `alwaysIncluded` heuristic to recognize the `fedora` repo alongside `baseos`, `appstream`, `crb`, etc. On Fedora systems, the `fedora` repo is the distro's core repo — equivalent to BaseOS on RHEL.

**Classifier change:** Add `fedora` to the set of repo IDs that receive `alwaysIncluded: true`. The heuristic matches on the repo ID string.

**Rendering change:** Non-toggleable accordion with "always included" label, same as BaseOS. No toggle switch.

### Tier Labels

The tier distinction between base image packages and user-installed packages is real (provenance differs), but both are included by default. The labels must communicate provenance, not status.

- **Group header for base image packages:** "Base image"
- **Group header for user-installed repo packages:** "Your repos"
- Individual repo accordions retain their repo names as headers (e.g., "fedora — 142 packages", "epel — 23 packages")
- Both groups render green. The visual distinction is the group header text, not the color.

### Batch Override

The accordion-level toggle already handles group-level include/exclude control. Verify that it works correctly when the default is flipped to included — toggling the accordion OFF should exclude all items in the group, toggling ON should re-include them.

---

## Tier 2: Labeling and Affordance

*Medium impact, low-to-medium cost. CSS/component fixes that remove discoverability problems.*

### Item 15 — Expandable Header Affordance

The tier 1 ("Base image") accordion header doesn't look clickable in either theme.

**Fix:**
- Add a chevron icon (right-aligned, `▸` collapsed / `▾` expanded)
- Add hover state: subtle background highlight
- Set `cursor: pointer` on the header element
- ARIA: `aria-expanded="true|false"` on the header (may already be present — verify)

### Item 17 — Dark Mode Toggle Visibility

The theme toggle button is invisible in dark mode.

**Fix:** Ensure the toggle has sufficient contrast against the dark background. Likely a missing or low-contrast `color`, `border`, or `background` on the toggle element in the dark theme CSS. Minimum contrast ratio: 3:1 against the background (WCAG 2.2 AA for UI components).

### Item 16 — Light Mode Brightness

Overall brightness needs toning down.

**Fix:** Reduce the main background luminance. Shift from pure white (`#ffffff`) to a warm off-white (e.g., `#f8f7f5` or `#fafaf8`). This is a global CSS change affecting the body/main background.

**Note:** This is subjective and may need iteration. The spec provides a starting point; Mark validates in-browser.

---

## Tier 3: Structural Changes

*Medium-to-high impact, higher cost. New sections, new components, new data flows.*

### Item 3 — Version Changes Section (Hybrid Placement)

Version changes get two surfaces: a summary block in the Overview and a full detail section accessible from the sidebar. Migration preview framing (framing B): "Your host vs. target base image."

#### Data contract

The current `snap.Rpm.VersionChanges` schema supports exactly two states:

- `VersionChangeUpgrade` — package exists on both host and base image, host version < base version
- `VersionChangeDowngrade` — package exists on both host and base image, host version > base version

Each `VersionChange` entry carries: `Name`, `Arch`, `HostVersion`, `BaseVersion`, `Direction`.

The inspector (`rpm.go`) only emits entries when a package exists in BOTH the host package list and the baseline, with a different EVR. It does **not** emit "removed" (host-only) or "new in base" (base-only) entries. Those concepts exist in other snapshot fields (`PackagesAdded`, `BaseImageOnly`) but are not part of `VersionChanges`.

**This spec scopes the version changes section to upgrades and downgrades only.** "Removed" and "new in base" views are deferred to a future spec that explicitly sources them from the correct snapshot fields and defines their interaction model.

#### Overview Block

New subsection in the executive summary, positioned between Migration Scope and Attention Items. Rendered client-side in `renderOverview()` from `App.snapshot.rpm.version_changes` (consistent with how the overview currently renders from `App.snapshot`).

**Content:**
- Two stat cards: upgrades (green), downgrades (amber)
- If downgrades > 0: notable items callout naming the downgraded packages inline with an amber highlight (e.g., "3 packages will be downgraded — `python3`, `nodejs`, `glibc-headers`")
- If zero version changes: no block rendered (section omitted, not shown empty)
- "View all N →" link navigating to the detail section

**Counts:** Computed client-side from `App.snapshot.rpm.version_changes` by counting entries where `direction === "upgrade"` vs. `direction === "downgrade"`. No Go-side summary struct needed — the overview is client-rendered.

#### Detail Section

New top-level section in the sidebar, positioned below the triage sections with a visual separator.

**Sidebar treatment:**
- Separated from triage sections by a divider line
- "info" badge (not a progress indicator)
- Does NOT participate in section progress tracking, sidebar dots, or the progress bar

**Content:**
- Header: "Package Version Changes" with total count and subtitle "Your host vs. target base image"
- Filter controls: All / Upgrades / Downgrades
- Table columns: Package (monospace), Your Host (version, muted), → arrow, Target Image (version), Change (colored label: ↑ upgrade / ↓ downgrade)
- Downgrade rows get a subtle amber-tinted background for visual prominence
- Footer: "N packages · purely informational — does not affect Containerfile output"

#### Interaction contract

**"View all N →" navigation:**
- Clicking the link in the overview block sets `App.activeSection = 'version-changes'` and scrolls the main content area to the detail section
- Focus lands on the section heading (`<h2>`) which has `tabindex="-1"` for programmatic focus
- The sidebar highlights the Version Changes entry
- Screen reader announcement: section heading text is read on focus

**Filter controls:**
- Semantic model: radio group (`role="radiogroup"`) with three radio buttons (All / Upgrades / Downgrades)
- Keyboard (true radio-group model): Left/Right arrow keys both move focus AND change the selected filter in a single action. No separate Enter/Space activation step — selection follows focus. This is the standard WAI-ARIA radio-group keyboard pattern (roving tabindex).
- Tab enters the group on the currently selected radio; Tab again exits the group. Only one radio is in the tab order at a time.
- Each filter button has `role="radio"`, `aria-checked="true|false"`. The selected radio has `tabindex="0"`; unselected radios have `tabindex="-1"`.
- Active filter announcement on selection change: "Showing [all/upgrades/downgrades], N packages"
- Table updates immediately on selection change (client-side filtering, no re-render)

**Empty states:**
- Zero version changes total: section omitted from sidebar entirely. Overview block not rendered.
- Zero matches for active filter (e.g., no downgrades): table body shows a single row spanning all columns: "No [downgrades/upgrades] detected." Filter controls remain visible.

**Critical constraint:** This section MUST NOT trigger Containerfile updates. No output pipeline wiring. The section is purely informational — it renders from `VersionChanges` data but has no write path to the Containerfile renderer.

**Data source:** Existing `snap.Rpm.VersionChanges` data. The current `classifyVersionChanges` function in the classifier creates display-only triage items in the packages section — this moves them to their own section. The classifier function changes its `Section` assignment from `"packages"` to `"version-changes"` (new section ID).

### Item 13 — Info-Only Grouping (Heavy)

Each triage section gets a collapsible "Informational" wrapper at the bottom, containing all display-only items.

**Render change — two-pass in `renderTriageSection`:**
1. First pass: render output-affecting items (accordions with toggles, decision cards, notification cards)
2. Second pass: render display-only items inside the collapsible wrapper

**Wrapper component:**
- Header: "Informational — N items" with chevron
- Default state: collapsed
- Expand reveals display-only accordions and cards inside
- Visual treatment: muted left border to distinguish from actionable content
- **Empty state:** When N = 0, the wrapper is omitted entirely (not rendered as an empty collapsible)
- **Persistence:** Wrapper open/closed state resets to collapsed on page reload (does not persist to autosave). This is consistent with tier expander behavior and avoids adding UI chrome to the snapshot.

**Items that move into the wrapper (by section):**
- **Runtime:** (none currently — services/cron/timers are all output-affecting)
- **Containers:** Running containers without quadlet backing
- **System & Security:** Network connections, fstab entries (passive read-only, no buttons)
- Other sections: any future display-only surfaces

**Accounting exclusion:** All items inside the wrapper are excluded from decided/undecided counts, sidebar badges, and progress tracking. Extends the existing `isPassiveItem` pattern to all display-only items regardless of key prefix.

**Fstab simplification (item 9):** Fstab entries move into the Informational wrapper as pure read-only items. Remove the Acknowledge/Skip interaction entirely. No buttons, no state tracking. The entries touching bootc-managed paths (`/`, `/boot`, `/var`, `/sysroot`, `/usr`, `/etc`) retain their guidance text ("this mount interacts with bootc's filesystem model — handle at deploy time") but as static informational text, not a card requiring action.

### Item 14 — Secrets Consequence Preview

Secret cards show what will happen *before* any button press, split by the three redaction kinds that inspectah actually implements (`excluded`, `inline`, `flagged`). Trust through transparency.

#### Current secrets contract

Inspectah's secrets pipeline (`pipeline/redact.go`) produces `RedactionFinding` entries with three distinct kinds for file-backed secrets:

1. **`excluded`** — the file is removed from the config tree entirely. A `.REDACTED` placeholder file is written to `outputDir/redacted/<path>.REDACTED` with remediation guidance (either `regenerate` or `provision`). The file does NOT appear in the image. The Containerfile renderer emits a comment block via `secretsCommentLines()`.

2. **`inline`** — sensitive values were actually replaced inline in the file content. The redacted file remains in the config tree and IS included in the image via the directory-level `COPY config/etc/ /etc/` instruction. The file structure is preserved but secret values are replaced with placeholder text.

3. **`flagged`** — advisory/heuristic finding. The file is flagged for manual review but content may be untouched (unless strict-mode heuristics upgrade the finding to `inline`). This is NOT the same safety story as `inline` — a flagged file may still contain the original sensitive values.

The secrets renderer (`renderer/secrets.go`) already splits findings into these three lists: `excluded`, `inlineRedacted`, and `flagged`.

The Containerfile does NOT contain per-file `COPY` instructions for individual secret files. Config files are copied via directory-level COPY instructions. Secret-related Containerfile output is limited to comment blocks generated by `secretsCommentLines()`.

#### Card layout — excluded secrets (`kind: "excluded"`)

**Pre-decision state:**
1. **Header:** "Secret Detected" badge (red) + file path (monospace)
2. **Explanation:** "This file contains sensitive data ([detection reason]). It has been excluded from the image. A placeholder with remediation guidance is in `redacted/`."
3. **Consequence preview** (secondary disclosure, collapsed by default): Labeled "What appears in the Containerfile:" followed by a code block showing the comment block from `secretsCommentLines()`:
   ```
   # SECRET EXCLUDED: /etc/myapp/config.conf
   # Action: provision this file on the target system from your secrets management process
   # See redacted/etc/myapp/config.conf.REDACTED for details
   ```
4. **Buttons:** "Keep excluded" (green, default) / "Include in image" (neutral, with warning)

**After "keep excluded" (collapsed state):**
- File path with "excluded" badge (muted)
- "File excluded — provision at deploy time"
- Undo button
- **Focus:** moves to the Undo button on the collapsed card
- Screen reader: "Secret [path] excluded from image. Undo available."

**After "include in image" (collapsed state):**
- File path with amber warning badge
- "File will be included — original content was excluded, verify before building"
- Undo button
- **Focus:** moves to the Undo button on the collapsed card
- Screen reader: "Warning: secret [path] will be included. Undo available."

**Undo from either state:** Returns to pre-decision state. Focus moves to the first action button. Screen reader: "Decision undone for [path]."

#### Card layout — inline-redacted secrets (`kind: "inline"`)

**Pre-decision state:**
1. **Header:** "Secret Redacted" badge (amber) + file path (monospace)
2. **Explanation:** "Sensitive values in this file have been redacted inline. The file structure is preserved but secret content has been replaced with placeholders. The redacted file will be included in the image via the config tree."
3. **Consequence preview** (secondary disclosure, collapsed by default): Labeled "What happens:" followed by explanatory text: "This file is part of the config tree and is included via the directory-level `COPY config/etc/ /etc/` instruction. The redacted version (with placeholder values) is what appears in the image. Replace placeholders with real values at build time or deploy time."
4. **Buttons:** "Keep redacted version" (green, default) / "Exclude entire file" (neutral)

**After "keep redacted version" (collapsed state):**
- File path with green checkmark
- "Included with redacted values — replace placeholders at build or deploy time"
- Undo button
- **Focus:** moves to the Undo button on the collapsed card
- Screen reader: "Secret [path] included with redacted values. Undo available."

**After "exclude entire file" (collapsed state):**
- File path with "excluded" badge (muted)
- "File will be excluded from the config tree"
- Undo button
- **Focus:** moves to the Undo button on the collapsed card
- Screen reader: "Secret [path] excluded from config tree. Undo available."

**Undo from either state:** Returns to pre-decision state. Focus moves to the first action button. Screen reader: "Decision undone for [path]."

#### Card layout — flagged secrets (`kind: "flagged"`)

**Pre-decision state:**
1. **Header:** "Review Required" badge (yellow) + file path (monospace)
2. **Explanation:** "This file may contain sensitive data ([detection reason]). The file content has NOT been modified — it requires manual review to determine if redaction or exclusion is needed."
3. **Consequence preview** (secondary disclosure, collapsed by default): "This file is currently included in the config tree as-is. If it contains real secrets, they will be present in the image. Review the file and decide whether to exclude it."
4. **Buttons:** "Keep as-is" (neutral) / "Exclude from image" (amber, recommended)

**After "keep as-is" (collapsed state):**
- File path with yellow "reviewed" badge
- "Kept as-is after review — verify no sensitive data before building"
- Undo button
- **Focus:** moves to the Undo button on the collapsed card
- Screen reader: "Secret [path] kept as-is after review. Undo available."

**After "exclude from image" (collapsed state):**
- File path with "excluded" badge (muted)
- "File will be excluded from the config tree"
- Undo button
- **Focus:** moves to the Undo button on the collapsed card
- Screen reader: "Secret [path] excluded from config tree. Undo available."

**Undo from either state:** Returns to pre-decision state. Focus moves to the first action button. Screen reader: "Decision undone for [path]."

#### Post-action focus rule (all secret card types)

After any decision action (include, exclude, keep, undo) that collapses or rerenders a secret card, focus moves to the **Undo button** on the collapsed card. This is consistent across all three card types and all four action paths. Undo returns focus to the first action button on the restored pre-decision card.

#### Data source for the SPA

The SPA reads secret data from `App.snapshot.redactions` directly. Each entry is a `RedactionFinding` with fields: `path`, `source`, `kind`, `pattern`, `remediation`, `detection_method`, `confidence`. The `kind` field drives the three-way card rendering split.

**Join key: index, not path.** The current `classifySecretItems()` emits one triage item per redaction entry with key `secret-<index>` where `<index>` is the array position in `snap.Redactions`. A single file can produce multiple redaction entries (one per pattern match, plus heuristic findings), so a path-based join is not stable. The SPA maps `secret-<n>` to `App.snapshot.redactions[n]` by index to get the full finding metadata for that specific card.

This keeps `TriageItem` lean (only `is_secret`, `source_path`, and the existing fields) while giving each card access to the exact `kind`, `pattern`, `remediation`, and `confidence` for its specific finding.

**Test obligation:** Include a test case where one file path produces multiple redaction findings (e.g., one `inline` pattern match and one `flagged` heuristic finding on the same file). Verify that each `secret-<n>` card renders with the correct `kind` and consequence text for its specific finding, not the first finding for that path.

#### Override artifact reconciliation

Secret-card actions persist by toggling the backing config file's include state through `source_path` — they do NOT mutate `snap.Redactions`. But multiple redaction-derived artifact generators read directly from `snap.Redactions`:
- `secretsCommentLines()` — Containerfile comment blocks for `excluded` and `flagged` findings
- `WriteRedactedDir()` — `.REDACTED` placeholder files for `excluded` findings
- `RenderSecretsReview()` — `secrets-review.md` with redaction details
- Audit report and README redaction counts

This creates a drift risk: if a user overrides a secret decision, config include state changes but redaction-derived artifacts still reflect the original `Kind`.

**Reconciliation contract:** During the re-render pipeline (same `nativeReRender()` seam as service/cron sync), `reconcileSecretOverrides(snap)` produces one authoritative post-override redaction state. This function:

1. **Does NOT modify the canonical `snap.Redactions` slice or its ordering.** The `secret-<n>` → `redactions[n]` index binding must remain stable across autosave/re-render cycles. The SPA and triage manifest continue to read the canonical slice.

2. **Produces a derived `reconciledRedactions` view** by copying `snap.Redactions` and adjusting `Kind` values based on config file include state. This derived view is passed to ALL artifact generators instead of the canonical slice.

3. **Override rules for all three directions:**

```
For each finding in reconciledRedactions where Source == "file":
  Look up the config file at finding.Path via source_path

  Case: finding.Kind == "excluded" AND config.Include == true
    // User overrode exclusion → file is re-included
    Set finding.Kind = "overridden"
    // Effect: suppressed from comment blocks, no .REDACTED placeholder written,
    // secrets-review.md notes the override, counts reflect the override

  Case: finding.Kind == "inline" AND config.Include == false
    // User excluded a file that had inline redaction
    Set finding.Kind = "excluded"
    // Effect: comment block emitted, .REDACTED placeholder written,
    // file removed from config tree on next render

  Case: finding.Kind == "flagged" AND config.Include == false
    // User explicitly excluded a flagged file
    Set finding.Kind = "excluded"
    // Effect: comment block emitted, .REDACTED placeholder written,
    // file removed from config tree on next render

  All other combinations: no change (finding.Kind preserved as-is)
```

4. **All artifact generators receive `reconciledRedactions`:**
   - `secretsCommentLines(reconciledRedactions)` — skips `overridden`, emits for `excluded`/`flagged`
   - `WriteRedactedDir(reconciledRedactions)` — writes placeholders for `excluded` only
   - `RenderSecretsReview(reconciledRedactions)` — reflects overrides in the review doc
   - Audit/README counts — computed from `reconciledRedactions`

5. **The returned `ReRenderResult.Snapshot` carries the canonical `snap.Redactions`** (unchanged ordering, unchanged Kinds) so the SPA's index-based binding remains stable. The reconciled view is render-time only — it does not persist.

This ensures all artifacts agree with the triage UI after user overrides, while preserving the index stability that the SPA depends on.

#### Multi-secret density

When multiple secrets are present, each gets its own card. The consequence preview block is a secondary disclosure (collapsed by default) to manage density. The explanation text (item 2) is always visible, giving the user enough context to decide without expanding the preview. This keeps the section scannable when 5+ secrets are present.

### Item 18 — Resizable/Hideable Containerfile Preview Pane

#### Accessible splitter pattern

The resize control uses the WAI-ARIA window splitter pattern:

**Splitter element:**
- `role="separator"` with `aria-orientation="vertical"`
- `tabindex="0"` (keyboard-focusable)
- `aria-valuenow`: current width in pixels
- `aria-valuemin`: 200 (minimum width, enough for COPY instructions)
- `aria-valuemax`: 60% of viewport width
- `aria-label`: "Resize Containerfile preview"

**Pointer interaction:** Drag the splitter left/right to resize. Standard cursor change (`col-resize`) on hover.

**Keyboard interaction:**
- Left/Right arrow: resize in 10px increments
- Home: collapse to minimum width
- End: expand to maximum width
- Enter: toggle between collapsed and last-used width

**Collapse/expand:**
- Toggle button in the preview pane header: "Hide preview" / "Show preview"
- When collapsed, the triage content area expands to full width
- The toggle button remains visible (pinned to the right edge of the header area)
- Keyboard: Enter on the splitter also toggles collapse
- Focus after collapse: moves to the "Show preview" toggle button
- Focus after expand: moves to the splitter element

**Screen reader:**
- Splitter focus announcement: "Resize Containerfile preview, separator, current width [N] pixels"
- After resize: "Containerfile preview, [N] pixels wide"
- After collapse: "Containerfile preview hidden"
- After expand: "Containerfile preview shown, [N] pixels wide"

#### State persistence

Preview pane width and collapsed/expanded state are **UI chrome**, not migration data. They MUST NOT be stored in `inspection-snapshot.json`, which is a canonical migration artifact exported in tarballs.

**Persistence home:** `localStorage`, keyed by a stable identifier derived from the report context (e.g., `inspectah-preview-pane-${snapshotHash}` or a simpler `inspectah-preview-pane` if per-report persistence is unnecessary).

**Stored values:**
```json
{
  "width": 400,
  "collapsed": false
}
```

**Fallback:** If no stored state exists, default to width ~35% of viewport, expanded. If `localStorage` is unavailable (e.g., private browsing), use in-memory defaults with no persistence — the pane still works, it just resets on reload.

---

## Tier 4: Investigation Items

### Item 11 — Kernel Module Filtering

Only surface modules that the user explicitly configured for loading, not modules auto-loaded by the kernel or hardware.

**Definition of "user-configured":** A module is user-configured if it appears in a configuration file under `/etc/modules-load.d/`. This directory contains explicit load directives — "load this module at boot."

**`modprobe.d` is excluded from this heuristic.** Files in `/etc/modprobe.d/` express policy (blacklists, aliases, options), not explicit load intent. Treating any `modprobe.d` mention as load intent would over-surface modules and blur runtime observation with configuration policy. `modprobe.d` config files are already captured in the config tree and will be included via the directory-level COPY if the user includes their config — that is sufficient for carrying policy into the image.

**Classifier heuristic:** Cross-reference detected kernel modules against the contents of files in `/etc/modules-load.d/` from the inspection snapshot. Modules named in those files are classified as user-configured and surfaced in the triage. Modules not named in `modules-load.d` are auto-loaded and excluded from triage entirely.

**Surface type: informational, not output-affecting.** On the current branch, the kernel module toggle changes `snap.KernelBoot.NonDefaultModules[].Include`, but the Containerfile renderer only emits a comment counting included modules — it does not produce actionable `modules-load.d` content from the toggle. The actual config snippets (`ModulesLoadD`, `ModprobeD`) are copied unconditionally via the config tree. Until a real write path is implemented that connects the triage toggle to the copied `modules-load.d` content, these items are display-only.

**Rendering:** User-configured modules render as **display-only accordion items** inside the System & Security section (not inside the Informational wrapper — they are important enough to be visible alongside actionable items, but they do not have an output-affecting toggle). Each row shows the module name and the `modules-load.d` file that references it. No toggle switch. The accordion header notes: "Module load configuration is carried via the config tree — these are shown for awareness."

**Future:** If a write path is added that lets the triage toggle control whether specific `modules-load.d` entries are included in the copied config, these items should be upgraded to output-affecting with a toggle. That is out of scope for this spec.

**Data requirement:** The inspection snapshot must capture the contents of `/etc/modules-load.d/`. If this data is not currently collected, the inspector needs a new collection step. The snapshot already captures `/etc/modprobe.d/` as part of the config tree — no additional collection needed for that directory.

#### Modprobe policy awareness (informational)

`modprobe.d` config files are carried forward via the config tree's directory-level COPY. On the current branch, these files are copied from `KernelBoot.ModprobeD` config snippets and do not have per-file include state — they travel with the config tree unconditionally. Users migrating to different hardware may not realize they're carrying hardware-specific module policy (GPU driver options, NIC tuning, hardware blacklists) into an image intended for different targets.

**Surface:** Informational accordion inside the System & Security Informational wrapper. Not output-affecting — the files are controlled in the Config section.

**Header:** "Module policy files — N files"

**Subheader text:** "These modprobe configuration files will be included in the image via the config tree. Review if migrating to different hardware."

**Drill-down rows:** Each row shows the file path and a brief summary of what it does, derived from the first directive in the file:
- `/etc/modprobe.d/nvidia.conf` — options (driver parameters)
- `/etc/modprobe.d/blacklist-nouveau.conf` — blacklist
- `/etc/modprobe.d/custom-nic.conf` — options (hardware tuning)

**Directive classification:** Parse the first non-comment line of each `modprobe.d` config file to determine the summary label: `blacklist` → "blacklist", `options` → "options (driver parameters)" or "options (hardware tuning)", `install`/`remove` → "custom load/unload script", `alias` → "module alias". This is best-effort — if parsing fails, show the file path with no summary.

**Empty state:** If no `modprobe.d` config files exist (or all are RPM-owned defaults without user modifications), omit this accordion.

### Item 7 — Container Section (Deferred)

Excluded from this spec. The container section has unclear UX ("confusing") and a potentially broken undo button. This needs investigation before a fix can be designed.

**Next step:** Kit investigates the current container section behavior, documents what's broken, and a follow-up spec addresses the fix.

---

## Test Strategy

### Snapshot Normalization Tests (Go)
- `normalizeIncludeDefaults` sets `Include: true` for all tier 2 surfaces when `!isFleet`
- `normalizeIncludeDefaults` sets `Include: false` for incompatible services when `!isFleet`
- Incompatible services with `Include: false` are removed from `EnabledUnits` if present
- Normalization is idempotent — running twice produces the same result
- Fleet snapshots are untouched by normalization
- Sidecar created AFTER normalization agrees with working snapshot on all `Include` values

### Service/Cron Synchronization Tests (Go)
- `syncServiceDecisions`: `Action: "enable"` with `Include: true` → unit in `EnabledUnits`; with `Include: false` → unit removed
- `syncServiceDecisions`: `Action: "disable"` with `Include: true` → unit in `DisabledUnits`; with `Include: false` → unit removed
- `syncServiceDecisions`: `Action: "mask"` with `Include: true` → unit removed from `EnabledUnits`, added to `DisabledUnits` (lossy approximation)
- `syncServiceDecisions`: `Action: "unchanged"` → no change to `EnabledUnits`/`DisabledUnits`
- `syncServiceDecisions`: incompatible services excluded during normalization stay out of `EnabledUnits` after sync
- `syncCronDecisions`: toggling `CronJobs[].Include = false` sets ALL `GeneratedTimerUnits` with matching `SourcePath` to `Include: false`
- `syncCronDecisions`: multi-line cron file → multiple generated timer units → all inherit the file's Include
- Sync runs in `nativeReRender()` after unmarshal, before re-serialization — returned snapshot agrees with rendered Containerfile
- **Artifact roundtrip (services):** toggle service OFF → autosave → re-render → unit absent from `EnabledUnits`/`DisabledUnits` AND absent from Containerfile `systemctl` lines
- **Artifact roundtrip (cron):** toggle cron OFF → autosave → re-render → generated timer units absent from Containerfile
- Sync functions are idempotent

### Secrets Reconciliation Tests (Go)
- `reconcileSecretOverrides` produces a derived view; canonical `snap.Redactions` ordering is unchanged
- `excluded` + `Include: true` → `reconciledRedactions` entry has `Kind: "overridden"` → suppressed from all artifact generators
- `inline` + `Include: false` → `reconciledRedactions` entry has `Kind: "excluded"` → comment block emitted, placeholder written
- `flagged` + `Include: false` → `reconciledRedactions` entry has `Kind: "excluded"` → comment block emitted, placeholder written
- `flagged` + `Include: true` (kept as-is) → `reconciledRedactions` entry unchanged → flagged comment block still emitted
- Multiple findings on same path: each `secret-<n>` card maps to the correct `redactions[n]` entry; verify with 2+ findings on one file
- All artifact consumers receive `reconciledRedactions`: `secretsCommentLines`, `WriteRedactedDir`, `RenderSecretsReview`, audit/README counts
- `ReRenderResult.Snapshot` carries canonical (unmodified) `Redactions` — index binding stable across rebuild
- **Artifact roundtrip (excluded→include):** flip excluded secret to "include" → re-render → comment block absent from Containerfile, `.REDACTED` placeholder not written, `secrets-review.md` notes override
- **Artifact roundtrip (flagged→exclude):** flip flagged secret to "exclude" → re-render → comment block present, placeholder written, file excluded from config tree

### Classifier Tests (Go)
- `DefaultInclude` values match snapshot `Include` values after normalization
- `fedora` repo: `alwaysIncluded: true` alongside `baseos`, `appstream`
- Version changes: classified in `version-changes` section (not `packages`), tier 1, display-only, upgrade/downgrade only
- Kernel module filtering: modules in `modules-load.d` → surfaced as display-only; `modprobe.d`-only and auto-loaded → excluded
- Fleet gate: none of the above changes apply when `isFleet` is true

### Rendered Output Tests (Go)
- Golden HTML: tier labels "Base image" / "Your repos" present
- Version changes section renders separately from packages
- Informational wrapper renders after actionable items in each section
- Fstab entries render without buttons inside Informational wrapper
- Kernel module items render as display-only (no toggle), not output-affecting
- Secrets: three card types render distinctly for `excluded`, `inline`, and `flagged` kinds
- **Artifact roundtrip (full):** after normalization + sync + autosave + re-render, Containerfile excludes incompatible services and includes all other tier 2 items

### Browser Contract Tests (Manual Verification)
- [ ] All tier 2 items render green with "included" state across all sections
- [ ] Toggling a service OFF → autosave → re-render → service absent from Containerfile `systemctl` lines
- [ ] Toggling a cron job OFF → autosave → re-render → generated timer absent from Containerfile
- [ ] Incompatible services render as excluded-by-default with override option
- [ ] `fedora` repo accordion is non-toggleable with "always included" label
- [ ] Tier 1 header has chevron, hover state, and `cursor: pointer`
- [ ] Dark mode toggle button is visible and meets 3:1 contrast ratio
- [ ] Light mode background uses off-white (not pure white)
- [ ] Overview panel shows version changes summary with upgrade/downgrade counts only
- [ ] Overview block is omitted when zero version changes exist
- [ ] "View all →" link navigates to version changes detail section; focus lands on section heading
- [ ] Version changes filter: Left/Right arrow moves focus AND selects (true radio-group, no Enter needed)
- [ ] Version changes filter: Tab enters on selected radio, Tab exits group
- [ ] Version changes empty filter state shows "No [type] detected" row
- [ ] Version changes section does NOT affect Containerfile output
- [ ] Informational wrapper is collapsible, defaults to collapsed, omitted when empty
- [ ] Fstab entries inside wrapper have no buttons
- [ ] Excluded-kind secret cards show comment-block consequence preview
- [ ] Inline-kind secret cards show directory-level COPY explanation with redaction note
- [ ] Flagged-kind secret cards show "Review Required" badge and warn content may be untouched
- [ ] All three secret card types have symmetric include/exclude/undo paths
- [ ] After any secret card action, focus moves to the collapsed card's Undo button
- [ ] After undo, focus returns to the first action button on the restored card
- [ ] Secret consequence preview is a secondary disclosure (collapsed by default)
- [ ] Preview pane splitter is keyboard-accessible (arrow keys resize, Enter toggles collapse)
- [ ] Preview pane splitter announces width changes to screen reader
- [ ] Preview pane state persists via localStorage across page reload
- [ ] Preview pane works without localStorage (in-memory fallback, no errors)
- [ ] Accordion-level toggle works correctly when default is "included" (toggle OFF excludes group)
- [ ] ARIA labels say "included" (not "needs decision") for green items
- [ ] Kernel modules: only modules named in `modules-load.d` appear, rendered as display-only (no toggle)
- [ ] Modprobe policy files appear as informational accordion in System & Security
- [ ] Version changes sidebar entry is below triage sections with separator and "info" badge

---

## Implementation Tiers Summary

| Tier | Items | Cost | Impact | Dependencies |
|------|-------|------|--------|-------------|
| 1 | 1, 2, 4, 5, 6, 8, 10, 12 + tier labels | Low | Highest | None |
| 2 | 15, 16, 17 | Low | Medium | None |
| 3 | 3, 9, 13, 14, 18 | Medium-High | Medium-High | Tier 1 (for correct default state) |
| 4 | 11 | Medium | Medium | May need inspector change for `modules-load.d` collection |

Tiers 1 and 2 can be implemented in parallel. Tier 3 depends on tier 1 for correct default state rendering. Tier 4 is independent but may require inspector-level changes.
