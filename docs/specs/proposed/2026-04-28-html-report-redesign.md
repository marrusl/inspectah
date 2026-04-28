# HTML Report Redesign — Guided Triage

**Status:** approved  
**Date:** 2026-04-28  
**Context:** Go-native port (go-port branch). Replaces the 442-line stub renderer from Phase 4 with a full-featured interactive report.  
**Review history:** Rounds 1-3 resolved redesign, security, state model, and autosave/reconnect. Round 4 narrowed to revision-token handshake and autosave UX state accuracy. This revision addresses all round-4 findings.

## Summary

Redesign the inspectah HTML report as a self-contained single-page application using PatternFly v6, CodeMirror 6, and a guided triage interaction model. The report is the primary user-facing output of inspectah — it must give sysadmins confidence that their generated Containerfile is correct and complete.

The core UX innovation is a three-tier triage system that surfaces the most critical items first and auto-handles the obvious ones, while preserving full manual control as an escape hatch.

## Architecture

### Single-file SPA with Go data injection

The Go renderer (`html.go`) does one thing: executes an embedded `html/template` that stamps the **redacted** snapshot JSON into a `<script>` tag inside `report.html`. All UI logic lives in client-side JavaScript.

```
Go Renderer (html.go)
  ├── go:embed report.html        (SPA shell, ~3-5K lines)
  ├── go:embed patternfly.min.css (PatternFly v6, ~200KB)
  └── go:embed codemirror.min.js  (CodeMirror 6, ~150KB)
  │
  │  1. Redact snapshot (pipeline already handles this)
  │  2. Safe-embed: JSON.Marshal + </script escape
  │  3. template.Execute(safe_json) → report.html
  │
  ▼
report.html (self-contained)
  ├── <style>  PatternFly v6 CSS        </style>
  ├── <script> CodeMirror 6 JS          </script>
  ├── <script> const SNAPSHOT = {{.}};   </script>
  └── <script> /* triage UI, live CF     */
               /* preview, sections,     */
               /* theme toggle           */
      </script>
```

The Go renderer is intentionally thin. The SPA shell is the complex artifact — it contains all section layouts, triage logic, and interactivity. This means UI iteration does not require recompilation of Go code (only re-embedding the updated template).

### Security invariants

1. **Redact before embed.** The snapshot MUST pass through the redaction engine before being embedded in HTML. The pipeline already enforces this ordering; the renderer receives only post-redaction snapshots. The renderer MUST NOT accept or embed unredacted snapshots.

2. **Redact before persist.** Any snapshot written to disk (working directory, temp dir) MUST be post-redaction. The refine server's working directory contains only redacted artifacts.

3. **Redact before export.** The tarball download MUST contain only redacted artifacts. The uploaded/modified snapshot from the client, temp workspaces, server logs, and any symlinked or path-escaping content MUST NOT be included in the tarball.

4. **Safe JSON embedding.** Snapshot data is embedded via `json.Marshal()` followed by replacement of `</` with `<\/` to prevent `</script>` injection. This is the only embedding path — no raw string interpolation of snapshot values into HTML.

5. **Text-only DOM rendering.** All snapshot-derived content (package names, file paths, config file content, metadata) MUST be rendered via `textContent` or equivalent safe DOM APIs. No `innerHTML` for snapshot-derived values. HTML structure is built from trusted template code only.

6. **No re-redaction on rerender.** When the admin edits the snapshot in refine mode and triggers a rebuild, the server does NOT re-run the redaction engine. The admin's edits are intentional — refine-mode edits are trusted local-operator input, not content the tool promises to keep redacted. The original scan's redaction pass is authoritative for the initial artifact set; refine-mode rerenders use `renderer.RunAll()` only. This trust assumption is appropriate because the refine server is loopback-only and the operator has direct host access to the same data.

7. **Immutable original snapshot.** The server retains the original scan snapshot as an immutable sidecar file (`original-inspection-snapshot.json`) in the working directory, separate from the mutable `inspection-snapshot.json`. This file is never overwritten by rerenders. It serves as the baseline for diff/comparison features and is excluded from the exported tarball (the tarball contains only the current canonical state).

### Static vs refine mode

One HTML file serves both use cases. Mode is detected client-side on page load:

```javascript
if (window.location.protocol === 'file:') {
  enableStaticMode();
} else {
  fetch('/api/health')
    .then(r => r.json())
    .then(data => {
      if (data.status === 'ok' && data.re_render === true) {
        enableRefineMode();
      } else {
        enableStaticMode();
      }
    })
    .catch(() => enableStaticMode());
}
```

Detection uses two gates: (1) `file://` protocol short-circuits to static mode without a network request, (2) the server must explicitly report `re_render: true` in the health response — reachability alone is not sufficient.

- **Static mode (file:// or server without re_render capability):** Uses the embedded `SNAPSHOT` constant from `report.html`. All data renders. Editing controls present but disabled. Global banner at top: "Editing requires refine mode — run `inspectah refine <tarball>` to enable." Additionally, each disabled interaction region (Editor tab, triage sections, toolbar) shows an inline read-only callout repeating the refine requirement. Disabled controls carry `aria-disabled="true"` and `aria-describedby` pointing to the nearest callout for screen-reader users.

- **Refine mode (http:// + server capability confirmed):** On page load, the client fetches `GET /api/snapshot` to obtain the live working-directory snapshot, which may be newer than the snapshot embedded in `report.html` (due to prior autosaves or a resumed session from a refined tarball). The client uses this fetched snapshot as its authoritative state, discarding the embedded `SNAPSHOT` constant. Editing enabled — CodeMirror active, include/exclude toggles functional, rebuild and download available.

### Refine-mode page boot sequence

```
1. Detect file:// → static mode (use embedded SNAPSHOT)
2. Detect http:// → fetch /api/health
3. Health check fails or re_render=false → static mode
4. Health check ok + re_render=true →
   a. Fetch GET /api/snapshot → {snapshot, revision}
   b. Replace embedded SNAPSHOT with fetched snapshot
   c. Set local revision counter = received revision
   d. Render all sections from live snapshot
   e. Enable editing controls
   f. Start autosave listener (first decision increments to revision+1)
```

This ensures that browser reconnects (after crash, tab close, or page refresh) and resumed sessions (refining a previously downloaded `-refined.tar.gz`) both boot from the correct persisted state.

## Section Layout & Navigation

10 destinations in the sidebar, two groups. The sidebar uses WAI-ARIA grouped navigation (`role="navigation"` with `role="group"` per section), not a tablist. Each link uses `aria-current="page"` for the active destination.

### Overview group

| Destination | Content | Interactive? | Review tracked? |
|-------------|---------|-------------|-----------------|
| **Overview** | Summary stats, audit findings, warnings. Landing page. | Read-only dashboard | No |
| **Editor** | File browser + CodeMirror editing for config files, drop-ins, and quadlet files. All file types are editable here regardless of which triage section owns them. Refine mode only. | Full editing | No |
| **Containerfile** | Live preview with syntax highlighting, copy button. Updates continuously as decisions are made. | Read-only (updated by triage decisions) | No |

### Migration Areas group

| Destination | Content | Triage tiers? | Review tracked? |
|-------------|---------|---------------|-----------------|
| **Packages** | RPM packages. Standalone — this section must be excellent. | Full 3-tier | Yes |
| **Config** | Config files and systemd drop-ins. Quadlet files are excluded — they belong to Containers. | Full 3-tier | Yes |
| **Runtime** | Services + scheduled jobs | Full 3-tier | Yes |
| **Containers** | Quadlet-backed container workloads (top) + non-RPM software (bottom). Containers owns all quadlet files — Config excludes them. Quadlet files remain editable and comparable in the Editor tab. Non-RPM is secondary. | Full 3-tier | Yes |
| **Identity** | Users/groups + SELinux policies | Full 3-tier | Yes |
| **System** | Kernel/boot + network + storage | Full 3-tier | Yes |
| **Secrets** | Redaction findings. Primary action surface for secret-flagged items. | Tier 3 only (all items are flagged) | Yes |

### Sidebar behavior

- **Review progress bar** at top: "Review progress: X / 7" — tracks only the 7 migration-area destinations that have triage actions and review state transitions. Overview, Editor, and Containerfile do not participate.
- **Status dots** per migration-area tab: gray (unreviewed) → yellow (in progress) → green (reviewed with ✓). Overview/Editor/Containerfile show no status dot.
- **Triage badges:** red count for flagged items, yellow count for needs-decision items. Badges update live as decisions are made.
- **Keyboard model:** Arrow keys navigate within each group. Enter/Space activates. Focus lands on the active link within the focused group.
- Tab switching is client-side (SPA, no page reload).
- Section footer on every triage destination: stats line + "Mark section reviewed" button.

### Narrow viewport behavior (< 1200px)

On viewports narrower than 1200px:
- The sidebar collapses behind a hamburger menu button in the masthead. Tapping the button slides the sidebar in as an overlay. Focus traps inside the overlay until dismissed.
- The Containerfile preview panel is hidden from the split view. The Containerfile sidebar destination becomes the only way to view the Containerfile. Its content is identical to what would appear in the right panel on wide viewports.

## Guided Triage Interaction Model

### Three tiers, descending urgency

Tiers are displayed in the order: Red → Yellow → Green. The admin handles the most critical items first while they are freshest.

#### Tier 3 — Flagged (red)

Items requiring careful attention. Examples: unknown-provenance binaries (installed outside package management), kernel modules from third-party sources, SELinux policy modifications.

**Card design:**
- Red left-border + warning banner explaining the specific risk
- Expandable details section (file listings, dependency info, provenance details)
- Action buttons: "Acknowledge & include" (blue) / "Exclude" (gray outline)
- The "Acknowledge" language is deliberate — signals the admin has reviewed the risk, not just clicked through

**Secrets section override:** In the Secrets tab, tier-3 cards use different action language: "Keep in image (acknowledged)" / "Exclude from image" with "Exclude" as the visually primary button (blue). The default posture for secrets is exclusion — the opposite of other sections where inclusion is the primary action.

#### Tier 2 — Needs decision (yellow)

Items the tool cannot confidently auto-decide. Examples: third-party repo packages, module stream packages, custom services, non-default configs.

**Card design:**
- Package/item name, version, source/repo, size
- Reasoning line explaining why it needs a decision (e.g., "Third-party repo package. Not in base image. 3 dependencies will also be added.")
- Action buttons: "Include in image" (blue) / "Leave out" (gray outline)
- Decided items collapse to a single line showing the decision label (green "included" or gray "excluded") with an "undo" affordance

#### Tier 1 — Auto-included (green)

Standard packages/configs that match the base image. Pre-checked by the tool.

**Display:**
- Collapsed by default — shows only a count: "147 standard packages auto-included"
- Brief explanation: "Matched the base image and will be included automatically."
- Expandable to review or override individual items (the escape hatch)
- Chevron indicates collapsed state (▶), expanded state (▼)
- **Override visibility:** When an admin overrides a tier-1 item (excludes an auto-included item), the overridden item surfaces as a decided card above the collapsed summary, not hidden inside it. This prevents decisions from disappearing into the collapsed bucket.

### Tier classification logic (single-host)

Items are assigned to tiers based on signals. When multiple signals match, the **highest-severity signal wins** (tier 3 > tier 2 > tier 1). Each item appears in exactly one section — no duplication across tabs.

**Precedence rule:** If an item triggers a secret/credential detection, it appears ONLY in the Secrets tab regardless of what other section it would otherwise belong to. Secrets is the primary action surface for secret-flagged items.

#### Packages

| Signal | Tier |
|--------|------|
| Package in base image | 1 (auto) |
| Package from standard RHEL/Fedora repo, not in base | 2 (decide) |
| Package from third-party repo (EPEL, vendor repos) | 2 (decide) |
| Module stream package | 2 (decide) |
| Package installed locally (no repo) | 3 (flagged) |
| Kernel module from non-standard source | 3 (flagged) |

#### Config

| Signal | Tier |
|--------|------|
| Config file matching base image content | 1 (auto) |
| Config file modified from base | 2 (decide) |
| Config file not in base image | 2 (decide) |
| Systemd drop-in file | 2 (decide) |

Quadlet files are excluded from Config — they are classified under Containers.

#### Runtime (Services + Scheduled Jobs)

| Signal | Tier |
|--------|------|
| Service in default state (matches base) | 1 (auto) |
| Service state changed from default | 2 (decide) |
| Scheduled job (cron/timer) present | 2 (decide) |

#### Containers

| Signal | Tier |
|--------|------|
| Quadlet file with valid unit | 2 (decide) |
| Running container without quadlet | 3 (flagged) |
| Non-RPM binary with unclear provenance | 3 (flagged) |

#### Identity (Users/Groups + SELinux)

| Signal | Tier |
|--------|------|
| System user/group (UID < 1000, matches base) | 1 (auto) |
| User-created account (UID >= 1000) | 2 (decide) |
| SELinux in enforcing mode (default) | 1 (auto) |
| SELinux boolean changed from default | 2 (decide) |
| Custom SELinux policy module | 3 (flagged) |

#### System (Kernel/Boot + Network + Storage)

| Signal | Tier |
|--------|------|
| Default kernel parameters | 1 (auto) |
| Custom kernel parameters (sysctl, cmdline) | 2 (decide) |
| Network config matching base | 1 (auto) |
| Custom firewall rules or network config | 2 (decide) |
| Non-default mount points or storage config | 2 (decide) |

#### Secrets

All items in Secrets are tier 3 (flagged). Items land here when the redaction engine or heuristic detector flags them. Source signals:

| Signal | Source section |
|--------|--------------|
| SSH private key in config file | Config |
| Password hash in shadow | Identity |
| API key/token in config or env var | Config / Containers |
| Certificate in non-standard path | System |
| High-entropy string with credential-adjacent context | Any |

### Continuous Containerfile updates

Every include/exclude decision immediately updates the Containerfile preview panel. The changed line flashes blue briefly (animation: 2s ease-out). No batch "apply" step — the Containerfile always reflects current decisions.

**Editor edits are excluded from the continuous preview.** Changes made in the CodeMirror editor (config file edits, new files) affect the snapshot but do NOT trigger a client-side Containerfile preview update. These require a full server-side rebuild to reflect accurately — they are captured in the next rebuild cycle.

The "Download tarball" button triggers a rebuild then export — not a pure export of client-side state.

### Decided-state resting patterns

When an item is decided (included or excluded), it collapses to a single-line card. The resting pattern varies by tier:

- **Tier 2 decided:** Single line with green "included" or gray "excluded" label + undo link. Standard pattern.
- **Tier 3 decided (non-Secrets):** Single line with green "included (acknowledged)" or gray "excluded" label + undo link. The "acknowledged" qualifier persists so the admin knows they made a deliberate risk-acceptance decision.
- **Tier 3 decided (Secrets):** Single line with red "kept in image (acknowledged)" or green "excluded from image" label + undo link. Color is inverted from other sections — keeping a secret is the risky action (red label), excluding it is the safe action (green label).

### Post-rebuild behavior

After `POST /api/render` returns and the client updates with canonical state:

- **Active destination preserved.** The currently visible section stays open. If the rebuild removes the current section's content entirely (edge case), fall back to Overview.
- **Expansion state preserved.** Tier collapse/expand state within each section is maintained across rebuilds. Decided-card collapse state is also preserved.
- **Scroll position preserved.** The viewport does not jump to the top of the page or the top of the current section.
- **Review state: inventory-aware.** If a rebuild changes a section's actionable inventory (adds/removes items, changes tiers, changes the decision surface), that section moves from "reviewed" back to "in progress." If the rebuild leaves the section's inventory unchanged, "reviewed" is preserved.
- **Focus management.** After a successful rebuild, focus moves to a dedicated status region in the toolbar (the "Done ✓" message). After a failed rebuild, focus moves to the error message on the button. Focus does NOT fall back to `<body>` or top of page.
- **Live-region announcements.** The toolbar status area is an `aria-live="polite"` region. It announces: "Building..." on submit, "Rebuild complete, downloading tarball" on success, "Rebuild failed: <error summary>" on failure.
- **Reduced motion.** Under `prefers-reduced-motion: reduce`, the blue line-flash animation on Containerfile changes is replaced with a static highlight (no animation). The "Building..." spinner uses a non-animated indicator. Tier expand/collapse transitions are instant.

### Section review states

Review progress tracks the **7 migration-area destinations** only. Overview, Editor, and Containerfile do not participate.

Each migration-area tab progresses through three states:

1. **Unreviewed** (gray dot) — default state
2. **In progress** (yellow dot) — admin has visited and made at least one decision OR expanded a tier-1 section
3. **Reviewed** (green dot + ✓) — admin clicked "Mark section reviewed"

**State transitions:**

| Event | Effect |
|-------|--------|
| First decision in section | unreviewed → in progress |
| Expand tier-1 to browse | unreviewed → in progress |
| Click "Mark section reviewed" | in progress → reviewed |
| Undo a decision in a reviewed section | reviewed → in progress |
| Server rebuild: section inventory unchanged | reviewed state preserved |
| Server rebuild: section inventory changed (items added/removed/re-tiered) | reviewed → in progress |
| Section has zero triageable items | Starts as "reviewed" (auto-complete). Progress bar counts it. |

The sidebar progress bar: "Review progress: X / 7"

## Containerfile Preview Panel

Fixed right panel (~380px wide), visible alongside the main content area on viewports wider than 1200px. On narrower viewports, the right panel is hidden and the Containerfile content is accessible only via the Containerfile sidebar destination. Sticky positioning — scrolls independently of the main content.

Features:
- Syntax highlighting: blue for Dockerfile keywords (FROM, RUN, COPY), green for strings, gray for comments
- Changes badge in header: "3 changes" count
- Copy button
- Line highlighting on changes (blue flash animation)
- Sections in the Containerfile correspond to migration areas: Packages, Services, Config, Quadlets

## Authoritative State Model

The refine server maintains a single **working directory** as the authoritative artifact root. All API operations read from and write to this one directory.

### State flow

```
┌─────────────────────────────────────────────────┐
│  Client (browser)                               │
│  ┌────────────────┐    ┌──────────────────────┐ │
│  │ In-memory       │    │ Containerfile preview │ │
│  │ snapshot draft   │───▶│ (client-side render) │ │
│  │ (user edits)    │    └──────────────────────┘ │
│  └───────┬────────┘                              │
│          │ POST /api/render                      │
│          ▼                                       │
│  ┌────────────────┐                              │
│  │ Server response │◀── canonical post-rebuild   │
│  │ replaces client │    state                    │
│  │ draft state     │                              │
│  └────────────────┘                              │
└─────────────────────────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────────────┐
│  Server (refine)                                │
│  ┌────────────────────────────────────────────┐ │
│  │ Working directory (one, authoritative)     │ │
│  │  ├── inspection-snapshot.json   (mutable)  │ │
│  │  ├── original-inspection-snapshot.json     │ │
│  │  │   (immutable sidecar, never overwritten)│ │
│  │  ├── report.html                           │ │
│  │  ├── Containerfile                         │ │
│  │  ├── config/                               │ │
│  │  └── ...                                   │ │
│  └────────────────────────────────────────────┘ │
│                                                  │
│  POST /api/render:                               │
│   1. Write client snapshot to working dir        │
│   2. Run renderer.RunAll() (NOT pipeline.Run())  │
│   3. Read back results from working dir          │
│   4. Return {render_id, html, containerfile,     │
│              snapshot} — this IS the new state    │
│                                                  │
│  GET /api/tarball?render_id=X:                   │
│   1. Verify render_id matches last successful    │
│   2. Package working dir as tarball              │
│   3. Filename: *-refined.tar.gz                  │
│   4. Exclude: uploaded snapshots, temp files,    │
│      server logs, symlinks                       │
└─────────────────────────────────────────────────┘
```

### Persistence model

**Within a session:** include/exclude decisions auto-persist to the working directory via debounced `PUT /api/snapshot` calls. If the browser crashes or closes, the admin can reconnect to the same server URL and resume — the page boot sequence fetches the live snapshot from the server. The promise is best-effort: decisions made in the last ~500ms before a crash (within the debounce window) may be lost. This is an acceptable tradeoff for a local tool.

**Between sessions:** the tarball is the durable artifact. "Download tarball" produces a stamped `*-refined.tar.gz` containing the admin's decisions. To resume later, the admin runs `inspectah refine` on the downloaded tarball — the snapshot inside it carries all previous decisions, so the report opens with prior work intact.

**Revert:** rolls back to the original scan state within the current session by restoring `original-inspection-snapshot.json` as the working snapshot. Discards all decisions made since the initial extraction.

### Autosave UX states

The toolbar contains a dedicated autosave status indicator, visually separate from the rebuild status region. It shows three states:

| State | Display | Trigger |
|-------|---------|---------|
| **Saving** | Subtle spinner + "Saving..." | A decision has been made but not yet confirmed durable. This includes the debounce window AND the PUT request in flight. The indicator transitions to "Saving..." immediately on decision, not when the request fires. This honestly reflects the ~500ms durability gap. |
| **Saved** | "Saved" + timestamp (e.g., "Saved 3s ago") | PUT returns 200. Only now is the decision confirmed durable on disk. |
| **Save failed** | "Save failed — retrying" in warning color | PUT returns 400 or network error. Client retries once after 2s. |

**409 Conflict is NOT a failure.** A stale-revision 409 from `PUT /api/snapshot` means the server already has newer canonical state (from a completed rebuild). The client silently discards the stale write — no UI change, no "Save failed", no screen-reader announcement. This is a non-error path.

The autosave indicator is NOT in the rebuild `aria-live` region. It uses its own `aria-live="polite"` region that announces only actual failures ("Save failed") and recovery after failure ("Saved"). Routine save confirmations and silent 409 discards are NOT announced. This prevents screen-reader noise during normal operation.

### Trusted-operator scope

The no-re-redaction trust assumption (security invariant 6) extends to autosaved snapshots and resumed tarballs. Autosaved `inspection-snapshot.json` files contain the operator's edits and are treated as trusted local-operator input. A resumed `-refined.tar.gz` carries the same trust: the operator who downloaded it is the same operator who made the decisions. The tool does not promise to keep previously-redacted content redacted after the operator edits it.

### Key invariants

1. **One working directory.** The server owns exactly one working directory extracted from the original tarball. All reads and writes go through this directory.

2. **Rerender updates working directory atomically.** `POST /api/render` writes the new snapshot, runs `renderer.RunAll()` (aligning with the existing `nativeReRender()` path in `refine.go`), and updates all artifacts in the working directory before returning. If the render fails, the working directory is not modified.

3. **Server response is canonical.** The `snapshot` and `containerfile` fields in the `/api/render` response become the client's new authoritative state. The client MUST replace its in-memory snapshot and Containerfile preview with these values. The `html` field is returned for export/inspection only — the live SPA rerenders from the canonical `snapshot` and `containerfile`, preserving the current destination, review states, theme, expansion state, and editor UI state. The client does NOT perform a full document swap.

4. **Tarball is bound to a render.** Each successful `/api/render` returns a `render_id` (monotonic counter or timestamp). `GET /api/tarball` requires a `render_id` parameter and will only package the working directory if it matches the last successful render. This prevents downloading a stale artifact.

5. **No re-redaction on rerender.** The rerender path calls `renderer.RunAll()`, not `pipeline.Run()`. The redaction engine is NOT re-run — the admin's edits are trusted local-operator input (see security invariant 6).

6. **Atomic working-directory updates.** Implementation SHOULD use temp-file/rename semantics to ensure the working directory is never left in a partial-write state on failed renders.

## Refine Server API

Five endpoints beyond static file serving. The server binds to `127.0.0.1` only (loopback). No remote access. All responses carry `Cache-Control: no-store` — including the served `report.html`, API responses, and tarball downloads. Both responses can carry sensitive local migration data even after redaction; avoiding browser/proxy caching is cheap hardening.

### GET /api/snapshot

- **Response (200):** JSON with `snapshot` (the current `inspection-snapshot.json` from the working directory) and `revision` (the current server-side revision number). This is the authoritative snapshot state — it reflects all autosaved decisions and any prior rebuilds.
- **Used by:** Refine-mode page boot (step 4a in the boot sequence). The client fetches this on every page load in refine mode to obtain both the latest persisted state AND the current revision token for autosave continuation.
- **Response format:** `{"snapshot": {...}, "revision": <number>}`
- **Headers:** `Cache-Control: no-store`

### PUT /api/snapshot

- **Request:** `Content-Type: application/json`. Body is a transport envelope: `{"snapshot": {...}, "revision": N}`. The `snapshot` field is the current snapshot JSON reflecting the admin's decisions. The `revision` field is transport metadata (monotonic counter) — it is NOT persisted inside `inspection-snapshot.json`. Only the `snapshot` value is written to disk. Maximum body size: 50MB.
- **Action:** Writes the `snapshot` value to `inspection-snapshot.json` in the working directory. This is the auto-persist path — called by the client after each include/exclude decision. Does NOT trigger a re-render or regenerate artifacts.
- **Revision guard:** The server tracks the last-written revision. If a PUT arrives with a `revision` older than the last-written revision, the server returns 409 Conflict and does not overwrite. This prevents a late debounced autosave from overwriting a newer canonical snapshot written by `POST /api/render`.
- **Response (200):** `{"saved":true,"revision":<number>,"timestamp":"<ISO-8601>"}`
- **Error (400):** Malformed JSON. Working directory unchanged.
- **Error (409):** Stale revision. Working directory unchanged. The client should discard this autosave — the server already has newer state.
- **Headers:** `Cache-Control: no-store`
- **Debouncing:** The client SHOULD debounce calls to this endpoint (e.g., 500ms after the last decision) to avoid excessive disk writes during rapid toggling.

### Autosave / rebuild ordering

The client MUST cancel any pending debounced autosave before posting to `POST /api/render`. After the render response is applied (client state updated from canonical response), the client resumes autosave with the new revision counter. This, combined with the server-side revision guard, ensures that a late autosave can never overwrite the canonical post-render snapshot.

```
Page boot (fresh or reconnect):
  1. GET /api/snapshot → {snapshot, revision: 42}
  2. Client sets local revision = 42
  3. First decision → debounce → PUT /api/snapshot (revision: 43)

Steady-state autosave:
  Decision → debounce 500ms → PUT /api/snapshot (revision N)
  Decision → debounce 500ms → PUT /api/snapshot (revision N+1)

Click "Download tarball":
  1. Cancel pending debounced PUT
  2. POST /api/render → server writes canonical snapshot, returns {revision: R, ...}
  3. Client updates state from response, sets local revision = R
  4. Resume autosave (next PUT will use revision R+1)
  5. Any stale PUT arriving with revision < R → 409, silently discarded
```

### POST /api/render

- **Request:** `Content-Type: application/json`. Body is the modified snapshot JSON. Maximum body size: 50MB.
- **Action:** Writes snapshot to working directory, calls `renderer.RunAll()` with `RefineMode: true` and `OriginalSnapshotPath` (pointing to `original-inspection-snapshot.json` — the immutable sidecar retained from the initial scan). Reads back all artifacts.
- **Response (200):** JSON with `render_id` (string, monotonic), `revision` (number — the new server-side revision after this render), `html` (re-rendered report — for export/inspection, not live document swap), `containerfile` (text), `snapshot` (the snapshot as written — canonical state). The client uses `snapshot` and `containerfile` as authoritative; `html` is supplementary. The client MUST adopt the returned `revision` as its local revision counter for subsequent autosaves.
- **Error (400):** Malformed JSON or invalid content-type. `{"error": "<message>"}`. Working directory unchanged.
- **Error (500):** Renderer failure. `{"error": "<message>"}`. Working directory unchanged (atomic write semantics).
- **Headers:** `Cache-Control: no-store`

### GET /api/tarball

- **Parameters:** `render_id` (required query parameter).
- **Validation:** Returns 409 Conflict if `render_id` does not match the last successful render. Returns 400 if `render_id` is missing.
- **Action:** Packages the working directory as a tarball. Excludes: `original-inspection-snapshot.json` (immutable sidecar), any files not produced by the renderer, temp files, server logs, symlinks, and path-traversal content.
- **Response:** `Content-Type: application/gzip`. `Content-Disposition: attachment; filename="inspectah-<host>-<timestamp>-refined.tar.gz"`.
- **Headers:** `Cache-Control: no-store`

### GET /api/health

- **Response:** `{"status":"ok","re_render":true}`. The `re_render` field explicitly signals that editing/rebuild is available. Without it, the client treats the server as static-mode.
- **Headers:** `Cache-Control: no-store`

### Rebuild flow

1. Admin makes decisions throughout the report. Each decision auto-persists to the server's working directory and updates the Containerfile preview continuously
2. Admin clicks "Download tarball" in bottom toolbar (this is a checkpoint + export action — decisions are already saved)
3. Button state changes to "Building..." with spinner. Live-region announces "Building..."
4. Client POSTs current snapshot to `POST /api/render` to regenerate all artifacts (Containerfile, report, etc.) from the canonical snapshot state
5. Server runs `renderer.RunAll()`, updates working directory atomically, returns results with `render_id`
6. Client replaces its in-memory snapshot and Containerfile with the server-returned canonical `snapshot` and `containerfile` (NOT a full document swap — the SPA shell, active destination, review states, expansion state, theme, and scroll position are preserved)
7. Client compares pre-rebuild and post-rebuild section inventories. Any section whose item set changed moves from "reviewed" back to "in progress"
8. Button briefly shows "Done ✓". Focus moves to status region. Live-region announces "Rebuild complete, downloading tarball"
9. Client triggers `GET /api/tarball?render_id=X` download — the tarball is a stamped checkpoint the admin can resume from later via `inspectah refine`
10. If rebuild fails (400 or 500): button shows red error state with server error message. Focus moves to error. Live-region announces "Rebuild failed: <summary>". Working directory and client state unchanged

## Color Palette

### Semantic colors (tier status)

| Role | Token | Hex |
|------|-------|-----|
| Flagged (tier 3) | red | `#f85149` |
| Needs decision (tier 2) | yellow | `#d29922` |
| Auto-included (tier 1) | green | `#3fb950` |

### Action colors

| Role | Token | Hex |
|------|-------|-----|
| Action buttons | blue | `#4493f8` |
| Action button bg | blue-bg | `rgba(68,147,248,0.12)` |
| Primary CTA (Download) | blue-action | `#2b6cb0` |
| Secondary buttons | gray outline | `var(--border)` border, transparent bg |

### Surface colors (dark mode)

| Role | Hex |
|------|-----|
| Page background | `#0d1117` |
| Surface (sidebar, panels) | `#161b22` |
| Surface raised (cards) | `#1c2128` |
| Border | `#30363d` |
| Text | `#e6edf3` |
| Text muted | `#8b949e` |
| Text dim | `#484f58` |

### Light mode

Defers to PatternFly v6 built-in light theme variables. Toggle via `pf-v6-theme-dark` class on `<html>`. Custom dark-mode overrides use CSS specificity scoped to the dark class. Minimal custom light-mode CSS needed.

## Accessibility

### Keyboard navigation

- **Sidebar:** Arrow keys navigate within each group (Overview, Migration Areas). Enter/Space activates a destination. Tab moves between groups.
- **Triage cards:** Tab moves between cards. Enter/Space activates the focused action button. Arrow keys within a card's action button group.
- **Tier sections:** Enter/Space on tier header toggles expand/collapse.
- **Focus management:** When switching destinations, focus moves to the first heading in the new section. When a decision collapses a card, focus moves to the next undecided card.

### Screen reader support

- Sidebar uses `role="navigation"` with `aria-label="Section navigation"`. Groups use `role="group"` with `aria-labelledby`.
- Triage badges use `aria-label` to announce counts (e.g., "2 items flagged for review").
- Progress bar uses `role="progressbar"` with `aria-valuenow`, `aria-valuemin`, `aria-valuemax`, and `aria-label`.
- Disabled controls in static mode use `aria-disabled="true"` with `aria-describedby` pointing to the nearest inline callout explaining the refine requirement.
- Decision card actions use `aria-label` with full context (e.g., "Include postgresql15-server in image").

## Static Assets

| Asset | Embedding | Size | Source |
|-------|-----------|------|--------|
| PatternFly v6 CSS | `go:embed` in renderer package | ~200KB | Vendored minified file |
| CodeMirror 6 JS | `go:embed` in renderer package | ~150KB | Vendored minified bundle (JSON + YAML highlighting) |
| report.html | `go:embed` as `html/template` | ~3-5K lines | `internal/renderer/static/report.html` |

All three assets are compiled into the Go binary. The output report.html is fully self-contained — no external network requests, no CDN dependencies.

## Testing Strategy

### Go unit tests (renderer)

- Template compiles without error
- Rendered HTML contains all 10 section landmarks
- JSON embedding is XSS-safe: snapshot containing `</script>`, `<img onerror=`, and other injection payloads produces safe output
- Snapshot-derived values are text-escaped (no raw HTML injection possible)
- Refined tarball filename includes `-refined` suffix

### Tier classification tests (table-driven)

Test the tier classification logic with fixture data covering:
- Each signal in the classification matrix
- Precedence: item matching both tier-1 and tier-2 signals → tier 2 wins
- Precedence: item matching both tier-2 and tier-3 signals → tier 3 wins
- Secret precedence: item with secret detection → appears in Secrets only, not origin section
- Zero-item sections → auto-complete as reviewed

### Golden-file tests (fragment-level)

Instead of one full-page golden, test normalized semantic fragments:
- Sidebar shell with progress bar, status dots, and badges
- A representative tier section (Packages with items in all 3 tiers)
- Containerfile output for a known snapshot

Fragment goldens are less noisy than full-page goldens when bundled assets or shell structure changes.

### Refine server contract tests

- `POST /api/render` with valid snapshot → 200, response contains `render_id`, `html`, `containerfile`, `snapshot`
- `POST /api/render` with invalid JSON → 400 error, working directory unchanged
- `POST /api/render` failure (renderer error) → 500 error, working directory unchanged
- `GET /api/tarball?render_id=X` with correct render_id → 200, valid tarball
- `GET /api/tarball?render_id=X` with stale render_id → 409 Conflict
- `GET /api/tarball` without render_id → 400
- `GET /api/snapshot` → 200, returns current `inspection-snapshot.json` from working dir
- `GET /api/snapshot` after PUT → returns the PUT'd snapshot (proves persistence)
- `GET /api/snapshot` after POST /api/render → returns the render-canonical snapshot
- `PUT /api/snapshot` with valid snapshot and current revision → 200, `{"saved":true,"revision":N,"timestamp":"..."}`
- `PUT /api/snapshot` with invalid JSON → 400, working directory unchanged
- `PUT /api/snapshot` with stale revision (older than last-written) → 409, working directory unchanged
- `PUT /api/snapshot` after POST /api/render with pre-render revision → 409 (revision guard prevents overwrite)
- Reconnect continuation: `GET /api/snapshot` returns `{revision: N}` → `PUT /api/snapshot` with `revision: N+1` → 200 success (proves client can continue autosaving after reconnect)
- Render continuation: `POST /api/render` returns `{revision: R}` → `PUT /api/snapshot` with `revision: R+1` → 200 success, while concurrent `PUT` with `revision: R-1` → 409 (proves autosave continues correctly after rebuild while stale writes are rejected)
- `GET /api/health` → 200, `{"status":"ok","re_render":true}`
- Tarball contents: no uploaded snapshots, no `original-inspection-snapshot.json`, no temp files, no symlinks
- All responses carry `Cache-Control: no-store`
- Resume semantics: extracting a `-refined.tar.gz` and serving it via refine loads the snapshot with prior decisions intact

### End-to-end equality test

After a successful `POST /api/render`:
1. Assert that the returned `snapshot` and `containerfile` byte-match the artifacts now present in the working directory
2. Assert that `GET /api/tarball?render_id=X` for the same render_id produces a tarball containing those same artifacts
3. This proves the render response, the working directory, and the exported tarball all represent the same render

### Browser smoke tests (manual, dev-browser)

Explicit test matrix for manual verification:

| Scenario | Verify |
|----------|--------|
| Static mode (file://) | Page renders, banner shows, editing controls disabled, no fetch errors in console |
| Refine mode (http://) | Page renders, editing controls enabled, no banner |
| Sidebar navigation | All 10 destinations accessible, section content switches, focus moves to heading |
| Tier rendering | Red → Yellow → Green ordering in all triage sections |
| Include/exclude + undo | Toggle a package → Containerfile updates → undo → Containerfile reverts |
| Editor change + rebuild | Edit a config file → Download tarball → verify tarball contains edited file |
| Mark reviewed then mutate | Mark Packages reviewed → undo a decision → status reverts to in progress |
| Rebuild inventory change | Mark section reviewed → edit related file in Editor → rebuild → section with changed inventory reopens to in progress |
| Successful rebuild | Click Download → spinner → Done ✓ → tarball downloads. Visible Containerfile matches exported artifact |
| Failed rebuild | Corrupt snapshot → Click Download → error message on button → prior state preserved |
| Session resume | Download tarball → stop server → `inspectah refine` the downloaded tarball → prior decisions intact |
| Browser crash recovery | Make decisions → close browser tab → reopen same URL → decisions preserved |
| Theme toggle | Dark → Light → Dark. All sections readable in both. |

## Out of Scope

- **Fleet prevalence-driven tier defaults.** Fleet snapshots render with all data but without smart tier assignment or prevalence slider. Follow-up spec.
- **Automated browser CI tests.** No headless browser in CI pipeline yet.
- **Print stylesheet.** Not needed for v1.
- **Diff view before rebuild.** The continuous Containerfile preview makes pre-rebuild diffs redundant.

## File Layout

```
cmd/inspectah/internal/renderer/
├── static/
│   ├── report.html          # SPA shell (html/template)
│   ├── patternfly.min.css   # PatternFly v6 vendored
│   └── codemirror.min.js    # CodeMirror 6 vendored
├── embed.go                 # go:embed directives
├── html.go                  # RenderHTMLReport() — template execution
├── html_test.go             # Unit + golden-file + tier classification tests
└── ... (other renderers unchanged)

cmd/inspectah/internal/refine/
├── server.go                # Updated: render_id tracking, loopback binding,
│                            #   tarball scoping, /api/health re_render field
├── server_test.go           # Updated: contract tests for all API behaviors
└── ... (tarball helpers unchanged)
```
