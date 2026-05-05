# Non-RPM Software & Containers Triage — Design Spec

**Status:** Approved (brainstorm)
**Date:** 2026-05-04
**Participants:** Mark Russell, Collins (architecture), Fern (UX), Ember (strategy), Seal (container tooling)

---

## Summary

Split the current overloaded "Containers" triage section into two first-class sections: **Containers** (deployment workloads) and **Non-RPM Software** (filesystem artifacts). Fix false-positive pip detection at the inspector level. Add data-driven Containerfile output for non-RPM items.

## Design Principles

1. **Triage + decision with structured discovery.** The tool categorizes and presents; the operator decides what matters.
2. **Honest about automation boundaries.** Toggle switches mean "the system will act." Review-status badges mean "your responsibility." Never mix the signals.
3. **Trust through transparency.** Containerfile stubs are opt-in scaffolding with detection metadata, not implied guidance. Make uncertainty the feature.
4. **Data-driven confidence.** The stub-vs-comment decision uses existing ldd data and .so detection, not guesswork.

---

## Section 1: Containers (new first-class section)

Containers become their own triage section with four subsections tiered by actionability. Visual hierarchy runs from full interactive treatment to read-only inventory.

### 1.1 Quadlet Units

**Treatment:** Toggle switch pattern (same as config/services).

Include/exclude directly affects Containerfile output — included units get `COPY` for unit files + `systemctl enable`. Show image reference, ports, and volumes extracted from the `.container` file. `.network` and `.volume` units render as supporting items under their parent container.

### 1.2 Flatpak Apps

**Treatment:** Toggle switch with persistent inline annotation.

The annotation reads *"Installed on first boot (not baked into image)"* and is always visible — not a tooltip, not a footnote. This is Fern's recommendation: the deployment mechanism is surprising, so the explanation must be unavoidable.

**System vs. user:** Only system-level flatpak installations (`flatpak list --system`) appear in triage. User-level flatpaks are personal preference, not machine state. The inspector must distinguish and filter.

**Output:** A declarative JSON manifest listing selected flatpaks + a reference systemd oneshot service that installs them on first boot. The oneshot uses a sentinel file (`ConditionPathExists=!/var/lib/.flatpak-provisioned`) to run once. This follows the uBlue/Fedora Atomic pattern — the only community-proven approach.

**Caveat:** The triage section notes that flatpak installation requires network access at first boot. The generated service should include retry logic.

### 1.3 Running Containers

**Treatment:** Informational + action suggestion.

Running containers (from `podman ps`) cannot be included as-is — they're runtime state, not image state. Each running container gets a card with:
- Container name, image, ports, volumes, status
- **"Generate Quadlet Draft"** secondary action button

The draft is generated from `podman inspect` data. Image, ports, volumes, environment, networks, and restart policy map nearly 1:1 to quadlet `[Container]` directives (per Seal's analysis — no `podman generate quadlet` exists upstream yet, but `podman inspect` JSON provides all needed fields).

The button label says "Draft" explicitly — the generated `.container` file needs operator review. Lighter visual weight than quadlet toggles.

### 1.4 Compose Files

**Treatment:** Informational only.

Compose files cannot be safely auto-migrated. Show a service inventory: service name, image, ports, volumes per service. Expand-to-YAML available as secondary disclosure for full inspection. Muted card styling, no action affordances beyond inspect.

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

**The decision is automatic:** inspectah uses existing `ldd` data to determine static vs. dynamic linking for binaries, and `.so` file detection in site-packages/node_modules for Python/Node.

**Fenced block format:**
```dockerfile
# === Non-RPM Software (operator review required) ===
# Items below were identified on the source system and marked for migration.
# Review and uncomment/adjust before building.

# DETECTED: /usr/local/bin/driftify-probe (Go binary, 14MB, statically linked)
# COPY /usr/local/bin/driftify-probe /usr/local/bin/

# DETECTED: /opt/myapp (Python venv, has requirements.txt)
# COPY /opt/myapp/requirements.txt /opt/myapp/
# RUN pip install -r /opt/myapp/requirements.txt

# WARNING: /usr/local/bin/mystery-tool (C/C++ binary, dynamically linked)
# Requires manual dependency analysis — shared library graph may differ on target image.
# See migration worksheet for ldd output.
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

1. **Quadlet draft generation:** Define the `podman inspect` field → quadlet directive mapping. Seal confirmed the mapping is nearly 1:1.
2. **Flatpak manifest format:** Decide JSON vs. YAML. Follow uBlue's format for ecosystem compatibility.
3. **System vs. user flatpak detection:** Verify inspector passes `--system` flag. If not, add it.
4. **Review-status persistence:** How does the operator's review state (not reviewed / reviewed / planned) persist across refine sessions? Likely stored in the snapshot JSON alongside existing include/decision state.
5. **Notes field persistence:** Same question for freeform notes. New field on snapshot items or a sidecar file?
6. **Non-RPM card styling:** Fern to spec the visual treatment that distinguishes review-status cards from toggle cards.
7. **Complexity signal data:** Verify inspectah captures enough metadata to generate the inline complexity signal (ldd output, requirements.txt presence, .so detection in node_modules).

---

## Team Input Summary

| Expert | Key contribution |
|--------|-----------------|
| Collins | Filesystem zone classification, stub-vs-comment decision rule (self-contained vs. entangled), flatpak first-boot architecture, Python venv COPY anti-pattern |
| Fern | Review-status pattern over toggles (system has no agency), visual distinction for operator-responsibility sections, first-boot annotation must be unavoidable, fenced block with detection metadata |
| Ember | Cloud migration tool analogy (AWS MGN/Azure Migrate), "make uncertainty the feature," flatpak as desktop migration differentiator, stubs as opt-in scaffolding |
| Seal | Running container → quadlet mapping via podman inspect, no upstream `podman generate quadlet` yet, uBlue flatpak pattern is ecosystem consensus |
