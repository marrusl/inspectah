# Non-RPM Software & Containers Triage — Design Spec

**Status:** Revision 4 (addressing round-3 review blockers)
**Date:** 2026-05-04
**Participants:** Mark Russell, Collins (architecture), Fern (UX), Ember (strategy), Seal (container tooling)
**Revision 2 notes:** Addresses four round-1 review blockers: Non-RPM review-state persistence contract, Containers section current-state overclaim, Non-RPM scaffolding build-context truth, and Flatpak scope/lifecycle. Mark's decision: flatpak is migration-assist only, not ongoing desired state.
**Revision 3 notes:** Addresses three round-2 blockers: Non-RPM interaction/persistence truth (autosave overclaim, control pattern, draft behavior, a11y), non-RPM export/build-context truth (no live non-rpm/ tree), Flatpak remote auto-config boundary (best-effort for public remotes). Also fixes compose current-state overclaim and specifies Quadlet draft sink.
**Revision 4 notes:** Fixes save-seam mismatch: spec now correctly references `PUT /api/snapshot` for durability and `POST /api/render` for rebuild. Compose current-state tightened to `ComposeFile.Images` ([]ComposeService) not generic service count.

---

## Summary

Split the current overloaded "Containers" triage section into two first-class sections: **Containers** (deployment workloads) and **Non-RPM Software** (filesystem artifacts). Fix false-positive pip detection at the inspector level. Add data-driven Containerfile output for non-RPM items.

## Design Principles

1. **Triage + decision with structured discovery.** The tool categorizes and presents; the operator decides what matters.
2. **Honest about automation boundaries.** Toggle switches mean "the system will act." Review-status badges mean "your responsibility." Never mix the signals.
3. **Trust through transparency.** Containerfile stubs are opt-in scaffolding with detection metadata, not implied guidance. Make uncertainty the feature.
4. **Data-driven confidence.** The stub-vs-comment decision uses existing `readelf`-derived signals (`static` field, `shared_libs` list) and pip `.so` detection, not guesswork.
5. **Current-state honesty.** This spec distinguishes what inspectah can do today from what requires new inspector, schema, or renderer work. Sections marked **(NEW WORK)** require implementation before the described behavior exists.

---

## Section 1: Containers (new first-class section)

Containers become their own triage section with four subsections tiered by actionability. Visual hierarchy runs from full interactive treatment to read-only inventory.

### 1.1 Quadlet Units

**Treatment:** Toggle switch pattern (same as config/services).

Include/exclude directly affects Containerfile output — included units get `COPY` for unit files + `systemctl enable`.

**Current state:** The inspector detects quadlet units and stores name, path, content, and image (extracted from `Image=` directive). The triage classifier creates items with toggles. The Containerfile renderer handles `COPY` for included unit files.

**(NEW WORK)** Show ports and volumes extracted from the `.container` file — this requires parsing additional quadlet directives (`PublishPort=`, `Volume=`) that the inspector currently captures in raw `content` but does not extract as structured fields. `.network` and `.volume` units are peers in the quadlet model, not children of `.container` units. The triage section should group them visually near related containers (matched by name convention) but not imply a strict parent-child hierarchy. **(NEW WORK)** — name-based association heuristic for visual grouping.

### 1.2 Flatpak Apps

**Treatment:** Toggle switch with persistent inline annotation.

The annotation reads *"Installed on first boot (not baked into image)"* and is always visible — not a tooltip, not a footnote. This is Fern's recommendation: the deployment mechanism is surprising, so the explanation must be unavoidable.

**Scope: migration-assist only.** Flatpak output is a one-time provisioning aid. inspectah generates a manifest and a reference first-boot service. After initial installation, flatpak lifecycle (updates, removal, adding new apps) is the operator's responsibility. inspectah does not provide ongoing desired-state reconciliation across image upgrades.

**System vs. user:** Only system-level flatpak installations appear in triage. User-level flatpaks are personal preference, not machine state.

**(NEW WORK — inspector):** The current detector runs `flatpak list --app` without `--system`/`--user` filtering. The inspector must be updated to pass `--system` and exclude user-level installations. This is a detector change, not a schema change — the `FlatpakApp` struct already carries the needed fields.

**(NEW WORK — triage + renderer):** Flatpak apps are not currently first-class triage items. The classifier needs a new `classifyFlatpakApps` function, and the Containerfile renderer needs a new flatpak output section (the manifest file + oneshot service reference, not inline `flatpak install` commands).

**Output:** A declarative JSON manifest listing selected flatpaks (app ID, remote, branch) + a reference systemd oneshot service. The oneshot uses a sentinel file (`ConditionPathExists=!/var/lib/.flatpak-provisioned`) to run once. This follows the uBlue/Fedora Atomic pattern.

**Remote configuration (best-effort for conventional remotes):** The generated oneshot configures flatpak remotes before installing apps using `flatpak remote-add --if-not-exists <name> <url>`. This works reliably for public remotes (Flathub, Fedora, GNOME Nightly) where name + URL is sufficient.

**Operator responsibility for non-conventional remotes:** Custom remotes that use GPG trust material, authenticator plugins, filter configuration, or non-standard options cannot be fully reconstructed from name + URL alone. The generated service includes a comment listing any remotes it could not fully reconstruct, with guidance: "This remote may require additional configuration. See `flatpak remote-modify --help`."

**(NEW WORK — inspector):** Capture remote name per app via `flatpak list --columns=application,origin` and remote URLs via `flatpak remotes --columns=name,url`. These two data points are sufficient for the best-effort `remote-add` story. The inspector does NOT attempt to capture GPG keys, authenticator config, or filter settings — those are operator-managed.

**Caveats the triage section must surface:**
- Flatpak installation requires network access at first boot
- Remote configuration is best-effort: public remotes (Flathub etc.) are auto-configured; custom/enterprise remotes may require manual setup
- The generated service is best-effort: if network is unavailable at first boot, flatpaks will not be installed. The service should include retry logic, but the operator should not assume guaranteed installation.

### 1.3 Running Containers

**Treatment:** Informational + action suggestion.

Running containers (from `podman ps`) cannot be included as-is — they're runtime state, not image state. Each running container gets a card with:
- Container name, image, ports, volumes, status
- **"Generate Quadlet Draft"** secondary action button

**(NEW WORK):** The draft is generated from `podman inspect` data. Image, ports, volumes, environment, and networks map to quadlet `[Container]` directives. No upstream `podman generate quadlet` exists yet — inspectah builds the mapping itself.

**Mapping limitations (per Seal's review):** Restart policy is NOT a `[Container]` directive — it belongs in the `[Service]` section as systemd restart semantics (`Restart=`, `RestartSec=`). The draft must handle this correctly or omit it with a comment. Other gaps: healthcheck translation, dependency ordering between containers, and user namespace mapping are not straightforward and should be omitted from v1 drafts with TODO comments.

**(NEW WORK — inspector):** Running container data requires `--query-podman` flag at scan time. The current inspector collects this optionally. The triage section should handle the case where no running container data exists (empty state: "No running containers detected. Run inspectah with --query-podman to inspect running workloads.").

The button label says "Draft" explicitly — the generated `.container` file needs operator review. Lighter visual weight than quadlet toggles.

**Post-click behavior for "Generate Quadlet Draft":**
- First click: generates the draft `.container` file content and adds it to `snap.containers.quadlet_units` as a new entry with `Include: false` (operator must review and toggle on). The card updates to show a "Draft generated — see Quadlet Units above" message with a link/scroll to the new quadlet entry.
- Repeated clicks: no-op if draft already exists. Button shows "Draft generated" in disabled state.
- Error state: if `podman inspect` data is insufficient (missing image field), button shows inline error: "Cannot generate draft — container image unknown."
- **Durable sink:** The generated draft is stored in `snap.containers.quadlet_units` — the same typed array as inspector-detected quadlets. This means it persists via the normal snapshot autosave, appears in the Quadlet Units subsection with a toggle, and is editable via the existing file editor. **(NEW WORK)** — the draft needs a `generated: true` flag so the UI can distinguish inspector-detected quadlets from generated drafts.
- **Keyboard:** button is focusable (`tabindex="0"`), activates on Enter/Space. After generation, focus moves to the new quadlet entry in the Quadlet Units subsection.

### 1.4 Compose Files

**Treatment:** Informational only.

Compose files cannot be safely auto-migrated. Show a service inventory with key metadata per service.

**Current state:** The Go schema stores `ComposeFile.Path` and `ComposeFile.Images` (a `[]ComposeService`, each carrying a service name + image reference). Raw YAML content is not stored. This is sufficient for v1: show the file path and the captured image-backed service pairs (service name + image per entry).

**(NEW WORK):** Extracting ports, volumes, and other per-service metadata requires additional compose YAML parsing. Expand-to-YAML disclosure requires storing or re-reading the raw file content. Both are follow-up work beyond v1.

Muted card styling, no action affordances beyond inspect.

### Visual Hierarchy

Top to bottom: Quadlets (full toggles) → Flatpaks (toggles + annotation) → Running containers (secondary action button) → Compose (read-only). The progression communicates: actionable → actionable-with-caveat → suggestive → informational.

---

## Section 2: Non-RPM Software (restructured)

### Purpose

Migration planning worksheet, not a decision board. The operator tracks what they've reviewed and records their migration plan. The tool provides detection data and optional Containerfile scaffolding.

### Grouping: By Type with Complexity Signal

Items are grouped by what the operator recognizes, with migration complexity as an inline signal per item.

| Group | Examples | Complexity signal |
|-------|---------|-------------------|
| Compiled Binaries | Go, C/C++, Rust in /usr/local | `(static — direct copy)` or `(dynamic — verify deps)` per item |
| Python Environments | venvs in /opt, pip packages | `(has requirements.txt)` or `(rebuild needed)` |
| Node.js Apps | npm/yarn apps with lockfiles | `(verify native modules)` |
| Shell Scripts | .sh files in /usr/local/bin | `(direct copy)` at group level |
| Other / Mixed | Directories, unknown binaries | Per-item assessment |

**De-noise rule (Fern):** If every item in a group has the same complexity (all shell scripts = "direct copy"), show it once at the group level. Only break out per-item when there's variance.

### Interaction Pattern: Review Status

Non-RPM items use a review-status indicator, not toggle switches. The toggle implies "I flipped this, the system will act." For non-RPM items, the system has no agency — the operator is the actor.

**Three states per item:**
- `Not reviewed` (default, neutral badge)
- `Reviewed` (acknowledged, no action planned)
- `Migration planned` (operator intends to carry this forward)

**Freeform notes field** per item — operator records their migration approach ("will rebuild from source," "COPY binary is fine," "moving to a container sidecar").

**Visual distinction:** Cards must look categorically different from the toggle sections (config, services, containers). Different card styling — background tint, distinct icon language — so the operator immediately grasps "this section is my responsibility, not the tool's."

### Persistence Contract (NEW WORK)

Review status and notes are output-affecting state (they control Containerfile scaffolding). They must persist across refine sessions.

**Typed home:** Add two new fields to `NonRpmItem` in `schema/types.go`:
```go
ReviewStatus string `json:"review_status,omitempty"` // "not_reviewed" | "reviewed" | "migration_planned"
Notes        string `json:"notes,omitempty"`
```

These fields live on the snapshot's `non_rpm_software.items[]` entries, alongside existing fields like `path` and `method`.

**Save mechanism:** The existing refine SPA uses two API endpoints: `PUT /api/snapshot` for routine snapshot durability (writes the snapshot JSON to disk without re-rendering), and `POST /api/render` for full rebuild/re-render (writes snapshot, runs the renderer pipeline, returns updated HTML + Containerfile + manifest). There is no per-keystroke autosave. **(NEW WORK)** Review-status changes should save via `PUT /api/snapshot` (durable but no re-render needed for status-only changes). When the operator clicks "Re-render," `POST /api/render` picks up the latest snapshot including review status and produces Containerfile scaffolding for `migration_planned` items. Notes field changes save on blur via `PUT /api/snapshot`.

**Lifecycle:**
- Initial scan: all items start as `review_status: "not_reviewed"`, `notes: ""`
- Refine session: operator changes status and adds notes via the SPA
- Save: snapshot JSON updated on status change and notes blur via `PUT /api/snapshot` (routine durability, no re-render)
- Re-render: only items with `review_status: "migration_planned"` produce Containerfile scaffolding
- Export: the final tarball includes the snapshot with review status and notes, so they survive the refine → build handoff

**Primary control pattern: segmented button group.**

The review-status control is a three-segment button group (not a cycling click, not a dropdown):

```
[ Not reviewed | Reviewed | Migration planned ]
```

- Rendered as `role="radiogroup"` with three `role="radio"` buttons
- Only one segment active at a time (mutually exclusive)
- Active segment is visually filled; inactive segments are outlined
- Left/Right arrow keys move selection (true radio-group keyboard model, same as version-changes filter)
- Tab enters the group on the active segment; Tab exits
- `aria-label="Review status for <item name>"`
- Each segment has `aria-checked="true"` / `"false"`

This avoids the ambiguity of a cycling click (operator can't tell which state is next) and is more compact than a dropdown.

**Notes field contract:**
- Visible when card is expanded (below the status control, above the detection metadata)
- `<textarea>` with `aria-label="Migration notes for <item name>"`, placeholder "Add migration notes..."
- Saves on blur via the lightweight save endpoint
- Collapsed card shows first line of notes (truncated) as a preview if non-empty
- Empty notes field: no preview in collapsed state

**Section chrome and progress:**
- The Non-RPM section does NOT participate in the progress bar or sidebar completion dots (it's a planning worksheet, not a decision checklist)
- Sidebar badge shows count of `not_reviewed` items (e.g., "8") as a neutral indicator, not a "needs attention" warning
- Section header shows: "Non-RPM Software (3 of 11 reviewed)" as a progress summary

**Empty state:** Section shows "No non-RPM software detected" when the snapshot has zero items after false-positive filtering. If the inspector was not run with non-RPM scanning enabled, show: "Non-RPM scanning was not performed. Re-run inspectah to detect non-package software."

**Keyboard/accessibility contract:**
- Each card row: two tab stops (segmented status control + expand chevron)
- Expanded card: additional tab stops for notes textarea and detection metadata links (e.g., "View contents" for items with captured files)
- Focus after status change: stays on the active segment within the radio group
- Focus after expand/collapse: stays on the chevron
- Screen reader: status change announces new state via `aria-live="polite"` on the status group

### Containerfile Output: Data-Driven Stubs

Items marked "Migration planned" produce output in a fenced block at the bottom of the Containerfile. Items marked "Reviewed" or "Not reviewed" produce nothing.

**Decision rule (Collins):** If the item is self-contained (no dynamic library dependencies, no compiled-against-specific-system artifacts), generate a template stub. If it has system-level entanglement, generate a comment-only warning.

| Type | Output | Rationale |
|------|--------|-----------|
| Shell scripts | **Stub:** `# COPY deploy.sh /usr/local/bin/` | No hidden deps |
| Go binary (static) | **Stub:** `# COPY foo /usr/local/bin/` | Self-contained per readelf `static: true` |
| Go binary (CGO/dynamic) | **Comment only** with `shared_libs` list | Shared lib graph fragile |
| C/C++ dynamic binary | **Comment only** with `shared_libs` list | Same — dependency analysis needed |
| Python with requirements.txt | **Stub:** `# COPY requirements.txt` + `# RUN pip install -r` | Rebuild is correct, not venv COPY (Venvs embed absolute paths) |
| npm with node_modules | **Comment only** | Native modules may break across base images |

**The decision is automatic:** inspectah uses `readelf`-derived signals already on the `NonRpmItem` schema: the `static` boolean (true = statically linked, safe to COPY) and the `shared_libs` list (non-empty = dynamically linked, needs review). For Python, the `has_c_extensions` field (derived from `.so` file scan in dist-info) indicates native module risk. Node.js native module detection is not currently implemented — **(NEW WORK)** to add `.so` scanning in `node_modules/` if lockfile-detected apps are included.

**Build-context reality (NEW WORK — export):** The Go-port export path does NOT currently create a `non-rpm/` payload tree in the output tarball. Non-RPM items are detected and stored in the snapshot as metadata (path, type, signals), but the actual files are not captured.

**v1 approach:** Containerfile stubs use source-host absolute paths as documentation of where the file was found, NOT as executable `COPY` sources. The operator must manually copy the files from the source host into their build context. Stubs clearly state this:

**Fenced block format:**
```dockerfile
# === Non-RPM Software (operator review required) ===
# Items below were identified on the source system and marked for migration.
# These are NOT automatically included in the build context.
# Copy the source files from the original host into your build directory,
# then uncomment and adjust the instructions below.

# DETECTED: /usr/local/bin/driftify-probe (Go binary, statically linked)
# Source host path: /usr/local/bin/driftify-probe
# COPY driftify-probe /usr/local/bin/

# DETECTED: /opt/myapp (Python venv, has requirements.txt)
# Source host path: /opt/myapp/requirements.txt
# COPY requirements.txt /opt/myapp/
# RUN pip install -r /opt/myapp/requirements.txt

# WARNING: /usr/local/bin/mystery-tool (C/C++ binary, dynamically linked)
# Source host path: /usr/local/bin/mystery-tool
# Requires manual dependency analysis — shared library graph may differ on target image.
# Shared libs: libssl.so.3, libcrypto.so.3 (from readelf)
```

**Future improvement:** Add a `non-rpm/` payload export step that captures selected files into the output tarball, making the stubs directly executable. This requires new plumbing in the export pipeline and may significantly increase tarball size.

Each stub carries detection metadata (path, type, linking status) as annotation. The `size` field is not currently on the `NonRpmItem` schema — stubs omit file size until the schema is extended. The `requirements.txt` association for Python venvs uses the `files` field on `NonRpmItem` if populated, but this association is not guaranteed for all venvs — stubs note the source host path and let the operator verify.

---

## Section 3: Inspector Fix (false-positive pip filtering)

### Problem

`scanPip()` in `nonrpm.go` finds `.dist-info` directories without checking RPM ownership. On a typical Fedora system, 13+ RPM-installed packages (dnf, setools, selinux, distro, etc.) are falsely detected as pip-installed software.

### Fix

After detecting a `.dist-info` directory, cross-reference the path against `rpm -qf <dist-info-path>`. If RPM owns it, skip it entirely. Don't add it to the snapshot — a pip package that's actually an RPM is not a finding.

### Scope

Only applies on package-mode systems where `rpm` is available. On ostree/bootc systems, the scanner already limits to `/usr/local/` and is unaffected.

### Schema Change

None. Filtering at detection, not adding fields.

---

## Open Items for Implementation Plan

### Resolved in revision 2
- ~~Review-status persistence~~ — defined: `ReviewStatus` and `Notes` fields on `NonRpmItem`, snapshot-backed
- ~~Flatpak lifecycle~~ — decided: migration-assist only, not desired-state
- ~~System vs. user flatpak detection~~ — specified: inspector must add `--system` flag

### Resolved in revision 3
- ~~Non-RPM autosave truth~~ — corrected: routine save via `PUT /api/snapshot`, rebuild via `POST /api/render`. No per-keystroke autosave.
- ~~Review-status control pattern~~ — defined: segmented radio-group with three states
- ~~Generate Quadlet Draft behavior~~ — defined: post-click flow, durable sink in `snap.containers.quadlet_units`, error/repeat states
- ~~Non-RPM section chrome/progress~~ — defined: does not participate in progress bar, sidebar shows `not_reviewed` count, header shows review progress
- ~~Accessibility contract~~ — defined: keyboard model, focus management, screen reader announcements
- ~~Non-RPM export truth~~ — corrected: no `non-rpm/` tree exists. v1 stubs document source paths, operator copies files manually. Future: payload export step.
- ~~Flatpak remote boundary~~ — narrowed: best-effort for public remotes (name + URL). Custom trust/auth is operator responsibility.
- ~~Compose data truth~~ — corrected: Go schema stores path + parsed service/image pairs, not raw YAML.
- ~~Quadlet .network/.volume~~ — corrected: peers not children, visual grouping by name convention.

### Remaining
1. **Quadlet draft generation mapping:** Define `podman inspect` field → quadlet `[Container]` + `[Service]` directive mapping. Restart → `[Service]`. Healthcheck, dependency ordering, user namespace deferred from v1.
2. **Flatpak manifest format:** JSON vs. YAML. Follow uBlue format. Include: app ID, remote name, branch.
3. **Non-RPM card styling:** Fern to spec visual treatment distinguishing review-status cards from toggle cards.
4. **Node.js native module detection:** `.so` scanning in `node_modules/`. Not currently implemented.
5. **Non-RPM payload export (future):** Add `non-rpm/` tree to output tarball for directly executable stubs. Significant tarball size impact — design separately.
6. **Compose v2 features:** Per-service ports/volumes parsing, expand-to-YAML. Beyond v1.
7. **Flatpak remote capture:** Inspector collects remote URLs via `flatpak remotes --columns=name,url`.
8. ~~Notes-only save endpoint~~ — resolved: `PUT /api/snapshot` already provides lightweight save without re-render.

---

## Team Input Summary

| Expert | Key contribution |
|--------|-----------------|
| Collins | Filesystem zone classification, stub-vs-comment decision rule (self-contained vs. entangled), flatpak first-boot architecture, Python venv COPY anti-pattern |
| Fern | Review-status pattern over toggles (system has no agency), visual distinction for operator-responsibility sections, first-boot annotation must be unavoidable, fenced block with detection metadata |
| Ember | Cloud migration tool analogy (AWS MGN/Azure Migrate), "make uncertainty the feature," flatpak as desktop migration differentiator, stubs as opt-in scaffolding |
| Seal | Running container → quadlet mapping via podman inspect, no upstream `podman generate quadlet` yet, uBlue flatpak pattern is ecosystem consensus |
