# Non-RPM Software & Containers Triage — Design Spec

**Status:** Revision 2 (addressing round-1 review blockers)
**Date:** 2026-05-04
**Participants:** Mark Russell, Collins (architecture), Fern (UX), Ember (strategy), Seal (container tooling)
**Revision 2 notes:** Addresses four round-1 review blockers: Non-RPM review-state persistence contract, Containers section current-state overclaim, Non-RPM scaffolding build-context truth, and Flatpak scope/lifecycle. Mark's decision: flatpak is migration-assist only, not ongoing desired state.

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

**(NEW WORK)** Show ports and volumes extracted from the `.container` file — this requires parsing additional quadlet directives (`PublishPort=`, `Volume=`) that the inspector currently captures in raw `content` but does not extract as structured fields. `.network` and `.volume` units rendering as supporting items under their parent container requires a new parent-child association that does not exist in the current schema.

### 1.2 Flatpak Apps

**Treatment:** Toggle switch with persistent inline annotation.

The annotation reads *"Installed on first boot (not baked into image)"* and is always visible — not a tooltip, not a footnote. This is Fern's recommendation: the deployment mechanism is surprising, so the explanation must be unavoidable.

**Scope: migration-assist only.** Flatpak output is a one-time provisioning aid. inspectah generates a manifest and a reference first-boot service. After initial installation, flatpak lifecycle (updates, removal, adding new apps) is the operator's responsibility. inspectah does not provide ongoing desired-state reconciliation across image upgrades.

**System vs. user:** Only system-level flatpak installations appear in triage. User-level flatpaks are personal preference, not machine state.

**(NEW WORK — inspector):** The current detector runs `flatpak list --app` without `--system`/`--user` filtering. The inspector must be updated to pass `--system` and exclude user-level installations. This is a detector change, not a schema change — the `FlatpakApp` struct already carries the needed fields.

**(NEW WORK — triage + renderer):** Flatpak apps are not currently first-class triage items. The classifier needs a new `classifyFlatpakApps` function, and the Containerfile renderer needs a new flatpak output section (the manifest file + oneshot service reference, not inline `flatpak install` commands).

**Output:** A declarative JSON manifest listing selected flatpaks (app ID, remote, branch) + a reference systemd oneshot service. The oneshot uses a sentinel file (`ConditionPathExists=!/var/lib/.flatpak-provisioned`) to run once. This follows the uBlue/Fedora Atomic pattern.

**Caveats the triage section must surface:**
- Flatpak installation requires network access at first boot
- The generated service does not manage flatpak remotes or trust material — the operator must ensure the target system has the correct remotes configured (e.g., Flathub)
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

### 1.4 Compose Files

**Treatment:** Informational only.

Compose files cannot be safely auto-migrated. Show a service inventory with key metadata per service.

**(NEW WORK):** The current inspector stores compose file path and raw content. Parsing service-level metadata (service name, image, ports, volumes) from the YAML requires new parsing logic in the inspector or classifier. For v1, showing the file path + service count (extractable from top-level YAML keys) is sufficient. Full service inventory and expand-to-YAML disclosure is a follow-up.

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

These fields live on the snapshot's `non_rpm_software.items[]` entries, alongside existing fields like `include` and `path`. They are saved via the existing autosave mechanism (snapshot JSON written to disk on every state change).

**Lifecycle:**
- Initial scan: all items start as `review_status: "not_reviewed"`, `notes: ""`
- Refine session: operator changes status and adds notes via the SPA
- Autosave: snapshot JSON updated, persists across page reloads and re-opens
- Re-render: only items with `review_status: "migration_planned"` produce Containerfile scaffolding
- Export: the final tarball includes the snapshot with review status and notes, so they survive the refine → build handoff

**Interaction contract:**
- Status change: click cycles `Not reviewed` → `Reviewed` → `Migration planned` (or direct selection via dropdown/radio)
- Notes field: inline text input, visible when card is expanded, auto-saves on blur
- Focus: after status change, focus stays on the status control (no reorder, no jump)
- Empty state: section shows "No non-RPM software detected" when the snapshot has zero items after false-positive filtering

### Containerfile Output: Data-Driven Stubs

Items marked "Migration planned" produce output in a fenced block at the bottom of the Containerfile. Items marked "Reviewed" or "Not reviewed" produce nothing.

**Decision rule (Collins):** If the item is self-contained (no dynamic library dependencies, no compiled-against-specific-system artifacts), generate a template stub. If it has system-level entanglement, generate a comment-only warning.

| Type | Output | Rationale |
|------|--------|-----------|
| Shell scripts | **Stub:** `# COPY /usr/local/bin/deploy.sh /usr/local/bin/` | No hidden deps |
| Go binary (static) | **Stub:** `# COPY /usr/local/bin/foo /usr/local/bin/` | Self-contained per ldd |
| Go binary (CGO/dynamic) | **Comment only** with ldd warning | Shared lib graph fragile |
| C/C++ dynamic binary | **Comment only** with ldd warning | Same — dependency analysis needed |
| Python with requirements.txt | **Stub:** `# COPY requirements.txt` + `# RUN pip install -r` | Rebuild is correct, not venv COPY (Venvs embed absolute paths) |
| npm with node_modules | **Comment only** | Native modules may break across base images |

**The decision is automatic:** inspectah uses `readelf`-derived signals already on the `NonRpmItem` schema: the `static` boolean (true = statically linked, safe to COPY) and the `shared_libs` list (non-empty = dynamically linked, needs review). For Python, the `has_c_extensions` field (derived from `.so` file scan in dist-info) indicates native module risk. Node.js native module detection is not currently implemented — **(NEW WORK)** to add `.so` scanning in `node_modules/` if lockfile-detected apps are included.

**Build-context reality:** The `COPY` stubs below use source-host absolute paths (e.g., `/usr/local/bin/foo`). These paths are valid because inspectah's export tarball already captures non-RPM artifacts in a `non-rpm/` directory within the output. **(VERIFY)** Confirm the Go-port export path includes non-RPM payloads in the tarball. If it does not, the stubs must reference the tarball-relative path or the spec must add an export step. Until verified, stubs should note: `# Source: captured in output tarball at non-rpm/usr/local/bin/foo`.

**Fenced block format:**
```dockerfile
# === Non-RPM Software (operator review required) ===
# Items below were identified on the source system and marked for migration.
# Review and uncomment/adjust before building.
# Source files are captured in the output tarball under non-rpm/.

# DETECTED: /usr/local/bin/driftify-probe (Go binary, 14MB, statically linked)
# COPY non-rpm/usr/local/bin/driftify-probe /usr/local/bin/

# DETECTED: /opt/myapp (Python venv, has requirements.txt)
# COPY non-rpm/opt/myapp/requirements.txt /opt/myapp/
# RUN pip install -r /opt/myapp/requirements.txt

# WARNING: /usr/local/bin/mystery-tool (C/C++ binary, dynamically linked)
# Requires manual dependency analysis — shared library graph may differ on target image.
# Shared libs: libssl.so.3, libcrypto.so.3 (from readelf)
```

Each stub carries detection metadata (path, type, size, linking status) as annotation above the instruction. This is what makes it a useful starting point rather than a misleading one.

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
- ~~Review-status persistence~~ — defined: `ReviewStatus` and `Notes` fields on `NonRpmItem`, snapshot-backed, autosave
- ~~Flatpak lifecycle~~ — decided: migration-assist only, not desired-state
- ~~System vs. user flatpak detection~~ — specified: inspector must add `--system` flag

### Remaining
1. **Quadlet draft generation:** Define the `podman inspect` field → quadlet `[Container]` directive mapping. Note: restart policy maps to `[Service]` not `[Container]`. Healthcheck, dependency ordering, and user namespace mapping deferred from v1 drafts.
2. **Flatpak manifest format:** Decide JSON vs. YAML. Follow uBlue's format for ecosystem compatibility. Include: app ID, remote name, branch.
3. **Non-RPM card styling:** Fern to spec the visual treatment that distinguishes review-status cards from toggle cards.
4. **Node.js native module detection:** Add `.so` scanning in `node_modules/` for lockfile-detected apps. Currently not implemented.
5. **Non-RPM export verification:** Confirm the Go-port export tarball includes non-RPM payloads. If not, add an export step or adjust stub paths.
6. **Compose service parsing:** Extracting per-service metadata (image, ports, volumes) from compose YAML requires new parsing. v1 can show file path + service count only.
7. **Flatpak remote/trust material:** The generated oneshot assumes remotes are pre-configured. Document this assumption; do not attempt to auto-configure remotes.

---

## Team Input Summary

| Expert | Key contribution |
|--------|-----------------|
| Collins | Filesystem zone classification, stub-vs-comment decision rule (self-contained vs. entangled), flatpak first-boot architecture, Python venv COPY anti-pattern |
| Fern | Review-status pattern over toggles (system has no agency), visual distinction for operator-responsibility sections, first-boot annotation must be unavoidable, fenced block with detection metadata |
| Ember | Cloud migration tool analogy (AWS MGN/Azure Migrate), "make uncertainty the feature," flatpak as desktop migration differentiator, stubs as opt-in scaffolding |
| Seal | Running container → quadlet mapping via podman inspect, no upstream `podman generate quadlet` yet, uBlue flatpak pattern is ecosystem consensus |
