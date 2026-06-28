# Non-RPM Software Replication

## Summary

Extend inspectah's Containerfile generation to replicate non-RPM software
found on the source host. Three tiers of replication, distinguished by
reproducibility and risk:

1. **Language packages** (pip/npm/gem) — always on, reproducible installs
2. **Unmanaged files** — opt-in, raw file copies with explicit risk signaling
3. **Repo-less RPM packages** — automatic, cached RPM bundling with upload fallback

Compose stacks are preserved as reference material with a Quadlet migration nudge.

**Design principle:** inspectah's job is accurate migration, not opinionated
restructuring. Replicate what was there.

**Product contract change:** This feature upgrades non-RPM handling from
advisory/reference output to executable Containerfile output. Today the
renderer explicitly warns that non-RPM source files are not in the build
context and emits advisory stubs. This spec replaces that with real
`COPY`/`RUN` instructions backed by collected artifacts. This is a
deliberate product-model shift: inspectah moves from "here's what we
found, you figure it out" to "here's a buildable Containerfile for the
full host state." Items remain reviewable and toggleable in refine — the
shift is from advisory to pre-included-but-reviewable.

## Tier 1: Language Package Replication

### Activation

Always on. No flag needed. Language packages are reproducible application
dependencies that belong in the migration output alongside RPM packages
and config files.

### Collector Hardening (prerequisite work)

The current non-RPM inspector has known provenance gaps that must be
fixed before Tier 1 output can be trusted as executable:

**1. RPM ownership filtering for pip packages.**
`scan_pip_packages()` walks system Python `dist-info` directories under
`/usr/lib/python3*`, `/usr/lib64/python3*`, and `/usr/local/lib/python3*`
and records everything as pip content. This produces false positives:
RPM-managed Python packages (e.g., `python3-requests` installed via
`dnf`) appear as pip packages.

Fix: cross-reference detected pip packages against `rpm_state`. Any
package whose `dist-info` path is owned by an RPM (`rpm -qf <path>`)
is excluded from the pip inventory. The `NonRpmInspector` already
requires `rpm_state` but does not use it for filtering — wire it up.

**2. Project-level artifact collection for npm/gem.**
The current inspector emits one `NonRpmItem` per package from lockfile
parsing. This loses the project context: which packages belong to the
same `package-lock.json`, which directory the project lives in, and
critically, the lockfile and manifest files themselves are not retained
in the snapshot.

Fix: restructure npm/gem collection to emit project-level entries:
- One `NonRpmItem` per project directory (not per package)
- `method` indicates lockfile presence: `"npm lockfile"`, `"gem lockfile"`
- New `manifest_files` field captures the raw content of collected
  manifests (see Data Model Changes)
- Individual package details stored in a `packages` vec on the project
  item (analogous to pip's existing `packages: Vec<PipPackage>`)

**3. Venv artifact collection for pip.**
When a venv is detected, if a `requirements.txt` exists in or adjacent
to the venv root, collect its content into the snapshot. If no
requirements file exists, the inline pinned-version fallback is used.

**4. Provenance confidence labeling.**
Each language environment item gets an explicit confidence level:
- `high`: lockfile or requirements.txt collected, RPM-filtered
- `medium`: dist-info/pip-list detection, RPM-filtered, no lockfile
- `low`: detection without RPM filtering (should not occur after
  hardening, but defensive)

The Containerfile renderer uses confidence to gate rendering:
- `high`: executable output (`COPY`/`RUN`)
- `medium`: commented-out executable output with fidelity warning,
  pre-excluded in refine (user must explicitly include). This prevents
  medium-confidence items from silently becoming executable installs.
- `low`: advisory-only, not renderable (defensive — should not occur
  after hardening)

### Detection

After collector hardening, the detection pipeline becomes:

- **pip:** `pip list --format json` + dist-info fallback + venv detection.
  RPM-owned packages filtered via `rpm -qf`. Requirements.txt collected
  when found. Venv structure preserved.
- **npm:** `package-lock.json` parsing. Project-level grouping. Manifest
  files (`package.json`, `package-lock.json`) collected into snapshot.
- **gem:** `Gemfile.lock` parsing. Project-level grouping. Manifest
  files (`Gemfile`, `Gemfile.lock`) collected into snapshot.

v1 scope: npm and gem detection is lockfile-only. The current collector
does not discover npm/gem projects without lockfiles, and adding that
detection path (e.g., scanning `node_modules` or gem directories)
requires new provenance rules. Lockfile-backed detection is high
confidence and sufficient for v1.

### Build Context / Export Contract

This is the critical new contract. The spec's Containerfile examples
reference files that must exist in the exported build context.

**New export artifact roots:**

| Root | Contents | Gate |
|------|----------|------|
| `language-packages/` | Manifest files per project | Always (Tier 1 is always on) |

**Per-ecosystem export layout:**

- pip with requirements.txt:
  `language-packages/pip/<env-hash>/requirements.txt`
- pip without requirements.txt: no file export (inline rendering)
- npm: `language-packages/npm/<project-hash>/package.json`,
  `language-packages/npm/<project-hash>/package-lock.json`
- gem: `language-packages/gem/<project-hash>/Gemfile`,
  `language-packages/gem/<project-hash>/Gemfile.lock`

The `<env-hash>` / `<project-hash>` is derived from the environment or
project path to ensure uniqueness without embedding full host paths.

**Export contract changes:**
- `render_refine_export()` allowlist in `crates/refine/src/session.rs`
  must be extended to include `language-packages/`
- `crates/refine/tests/export_contract_test.rs` must be updated
- `docs/reference/output-artifacts.md` must document the new roots
- Preview/export parity: the Containerfile preview in the refine UI
  must reference the same paths the export materializes

### Containerfile Rendering

Each ecosystem has its own rendering logic. Fidelity comments indicate
how the package list was detected.

**pip:**

Venvs are recreated faithfully. The source host chose a venv for a
reason (dep isolation between apps, system Python protection) — that
reason doesn't go away in the image.

- Venv with `requirements.txt` (high confidence):
  ```dockerfile
  # pip packages: /opt/myapp/venv (from requirements.txt, RPM-filtered)
  COPY language-packages/pip/a1b2c3/requirements.txt /tmp/myapp-requirements.txt
  RUN python3 -m venv /opt/myapp/venv \
      && /opt/myapp/venv/bin/pip install -r /tmp/myapp-requirements.txt \
      && rm /tmp/myapp-requirements.txt
  ```
- Venv without `requirements.txt` (medium confidence — commented out,
  user must explicitly include):
  ```dockerfile
  # pip packages: /opt/myapp/venv (detected via dist-info — transitive deps may differ)
  # Uncomment after verifying package list is complete:
  # RUN python3 -m venv /opt/myapp/venv \
  #     && /opt/myapp/venv/bin/pip install flask==2.3.3 requests==2.31.0
  ```
- System-level pip (medium confidence — commented out):
  ```dockerfile
  # pip packages: system (detected via pip list, RPM-filtered)
  # Uncomment after verifying package list is complete:
  # RUN pip install flask==2.3.3 requests==2.31.0
  ```

Inline pinned versions are preferred over lockfile-copy for pip because
binary wheels are platform-specific. `pip install pkg==ver` lets pip
fetch the correct wheel for the target architecture.

**npm (lockfile-only in v1):**

- `package-lock.json` found (high confidence):
  ```dockerfile
  # npm packages: /opt/myapp (from package-lock.json)
  COPY language-packages/npm/d4e5f6/package.json /opt/myapp/package.json
  COPY language-packages/npm/d4e5f6/package-lock.json /opt/myapp/package-lock.json
  RUN cd /opt/myapp && npm ci --production
  ```

**gem (lockfile-only in v1):**

- `Gemfile.lock` found (high confidence):
  ```dockerfile
  # gem packages: /opt/myapp (from Gemfile.lock)
  COPY language-packages/gem/g7h8i9/Gemfile /opt/myapp/Gemfile
  COPY language-packages/gem/g7h8i9/Gemfile.lock /opt/myapp/Gemfile.lock
  RUN cd /opt/myapp && bundle install --deployment
  ```

### Implementation Notes

**Runtime prerequisite validation:** Before rendering a language package
section, verify that the corresponding runtime (`python3`, `nodejs`,
`rubygems`) appears in the RPM package list. If missing, emit a warning
comment: `# WARNING: python3 not found in RPM package list — add it
before this section`.

**C extension handling:** pip items with C extensions already get a
"rebuild required" warning in the existing inspector. Preserve this —
don't blindly emit `pip install` for packages that may need native
compilation toolchains.

### Refine UI

New "Language Packages" decision section. Include toggles per environment
(whole venv or npm/gem project as a unit, not per individual package).
Standard toggle behavior matching packages and configs.

## Tier 2: Unmanaged Files

### Activation

Opt-in via `--include-unmanaged` scan flag. Covers ELF binaries, JARs,
scripts, and anything else in `/opt`, `/srv`, `/usr/local` that isn't
claimed by a language package manager or RPM.

### Provenance Signals

Not all files in `/opt` are equal. The scan catalogs provenance signals
to help users distinguish executable payload from mutable host state:

- **File type:** ELF binary, JAR, script (shebang), data file, config,
  symlink, other
- **Mutability indicators:** last-modified timestamp relative to system
  install date, presence in a writable mount, file permissions
- **Ownership:** filesystem uid/gid, whether the path is under a
  service's working directory

These signals are surfaced in the refine UI to inform include/exclude
decisions. The Containerfile warning block applies regardless of signals
— all unmanaged files are "you own maintenance" territory.

### Scan Behavior

1. Catalog all unmanaged files with: path, size, type, last modified,
   ownership, provenance signals
2. Exclude files already captured by Tier 1 (language package environments)
   — no double-counting
3. Apply `--exclude-path` filters (repeatable flag, processed before catalog)
4. Display total count and size, prompt for confirmation before bundling:
   ```
   Found 47 unmanaged files in /opt, /srv (2.3 GB total)
   Include in tarball? [Y/n]
   ```
5. `--yes` suppresses the prompt and bundles everything
6. Bundled files are included in the scan tarball

Bundling must happen at scan time because the tarball may be transferred
to a different machine for refine — the original files won't be available
later.

### Build Context / Export Contract

| Root | Contents | Gate |
|------|----------|------|
| `unmanaged/` | Copied files, directory structure preserved | `--include-unmanaged` |

Export contract: `render_refine_export()` allowlist extended. Only files
with `include: true` after refine are materialized in the export tarball.

### Containerfile Rendering

Separate section with clear warning block:

```dockerfile
# === Unmanaged files (no package manager provenance) ===
# These files were copied directly from the source host. They have
# no upstream update path and must be manually maintained.
COPY unmanaged/opt/splunk/ /opt/splunk/
COPY unmanaged/opt/datadog/ /opt/datadog/
```

Files grouped by source directory for readability.

### Refine UI

- "Unmanaged Files" decision section, visible only when `--include-unmanaged`
  was used at scan time
- When section is absent, show discoverability hint: "Unmanaged files not
  scanned. Re-run with `--include-unmanaged` to review."
- Items grouped by parent directory with group-level toggle. Per-item
  toggles within each group for fine-grained control.
- Each item shows: path, size, type, provenance signals
- Running size rollup in section header with denominator for context:
  "4 of 12 items included, ~340 MB of ~1.2 GB" — updates real-time as
  toggles change
- "Include None" bulk action (default state is all-included; "Reset to All"
  link handles the reverse)
- Items toggled off are excluded from the export tarball

### `/var` Path Guidance

Unmanaged files under `/var` require special handling in bootc images.
`/var` is persistent and mutable — it survives image updates via ostree
3-way merge. Files `COPY`'d into `/var` in a Containerfile become the
initial state but can drift from the image definition at runtime.

When an unmanaged file's path is under `/var` (e.g., `/var/lib/myapp/data`),
the refine UI shows an additional warning: "This path is under /var
(persistent, mutable). Changes at runtime will not be reset by image
updates." The Containerfile comment for `/var` items includes:
`# NOTE: /var is persistent — this file can drift from the image after boot.`

### Messaging

Frame as a "lift and shift" capability, not a recommendation. The
Containerfile warning says "you own maintenance" — direct but not
apocalyptic. Nudge toward proper packaging where possible.

## Tier 3: Repo-less RPM Packages

### Activation

Automatic — no flag needed. Applies to RPM packages where `source_repo`
is empty or points to a repo that's no longer configured.

### Scan Behavior

For each package with no repo source:
- Check `/var/cache/dnf/` for the cached `.rpm` file
- If found: bundle into the tarball under `repoless-packages/`
- If not found: record as "manual resolution needed"

### Build Context / Export Contract

| Root | Contents | Gate |
|------|----------|------|
| `repoless-packages/` | Cached and uploaded RPM files | Automatic |

Export contract: `render_refine_export()` allowlist extended.

### Containerfile Rendering

Use `dnf localinstall` instead of `rpm -i` to preserve dependency
resolution. However, repo-less RPMs bypass the normal trust chain
(no repo GPG key verification, no upstream provenance). The Containerfile
must make this explicit.

**Provenance gating:** Repo-less RPMs are pre-excluded by default in
refine. The user must explicitly include them — this is the trust gate.
The triage annotation and Containerfile warning make the provenance gap
visible so the decision is informed, not silent.

- Cached RPM available (pre-excluded, user must explicitly include):
  ```dockerfile
  # Repo-less package: custom-tool (cached RPM, no repository provenance)
  # WARNING: This package has no upstream repo and no GPG verification.
  # It was found in the local dnf cache. Updates must be managed manually.
  COPY repoless-packages/custom-tool-1.2.3.x86_64.rpm /tmp/
  RUN dnf localinstall -y /tmp/custom-tool-1.2.3.x86_64.rpm \
      && rm /tmp/custom-tool-1.2.3.x86_64.rpm
  ```
- No cached RPM:
  ```dockerfile
  # MANUAL: custom-tool (no repo source, RPM not in cache)
  # Provide the RPM via the refine UI upload, add a repo, or uncomment:
  # RUN dnf install custom-tool
  ```

### Refine UI

These packages appear in the normal Packages section (they're RPMs).
New triage annotations:

- "No repo source — cached RPM bundled (pre-excluded, no GPG verification)"
- "No repo source — manual resolution needed"

**RPM upload interaction:**

For "manual resolution" packages, the package row shows an orange
"RPM needed" label in the badge slot. Clicking it opens a modal dialog:

- PatternFly `FileUpload` with drag-and-drop and file picker
- Displays expected NEVRA (name-version-release.arch.rpm) for confirmation
- Client-side validation: filename must match expected package name and
  architecture, file must end in `.rpm`
- Validation feedback via `FileUploadHelperText` — green for match,
  red with explanation for mismatch
- Modal focus trap: focus moves to the file upload area on open, returns
  to the trigger button on close
- Screen reader: `aria-live="polite"` announces "RPM provided for
  [package-name]" on successful upload

After successful upload: label turns green "RPM provided" with checkmark.
Row checkbox becomes fully functional (enabled, toggleable).

Pre-upload checkbox state: visible but disabled with warning tooltip
"RPM file required before this package can be included." The row is not
hidden — users need to see it exists and understand what action is needed.

**Batch upload:**

When multiple packages need RPMs, a toolbar-level "Upload RPMs" button
appears. Opens a `MultipleFileUpload` modal where users drop multiple
`.rpm` files. inspectah auto-matches each file to the correct package by
parsing the RPM filename. Shows match table (matched/unmatched/conflicts)
before confirming. Screen reader announces summary: "4 of 6 RPMs matched
successfully."

### Export

Cached RPMs and uploaded RPMs are included in the export tarball under
`repoless-packages/`.

## Compose Stacks

### Approach

Reference-only — no Containerfile replication. Compose workloads are
applications running on top of the OS, not OS image content. Baking
compose stacks into an immutable bootc image conflates the OS and the
workload and breaks the immutable update model.

### Collector Changes

The current `ComposeFile` type in `crates/core/src/types/containers.rs`
stores `path` and parsed `images` but does not retain raw compose YAML.
The inspector drops the body after parsing for image extraction.

Fix: add a `raw_content: Option<String>` field to `ComposeFile`. The
containers inspector retains the raw YAML when constructing compose
entries. This enables verbatim export.

**Sensitivity gating:** Compose files commonly contain environment
variables with secrets (database passwords, API keys). The existing
inspector already scans for secret-like patterns and emits redaction
hints. Apply the same sensitivity rules to compose export:

- If the snapshot has `redaction_state == Some(Redacted)`, compose files
  in the export have secret-like values replaced with `<REDACTED>`
- If unredacted, compose files export verbatim (operator has already
  acknowledged the sensitivity)
- The refine UI shows a sensitivity indicator on compose entries when
  secret-like patterns were detected

### Scan Behavior

No change to scan trigger — compose files are already detected by the
containers inspector. The change is retaining raw content.

### Tarball

Compose files preserved under `compose/` in the tarball, with directory
structure mirrored (e.g., `compose/opt/myapp/docker-compose.yml`).
Subject to redaction rules above.

### Build Context / Export Contract

| Root | Contents | Gate |
|------|----------|------|
| `compose/` | Compose files (possibly redacted) | Automatic when compose detected |

Export contract: `render_refine_export()` allowlist extended.

### Containerfile Rendering

No `RUN` or `COPY` directives. Comment block listing detected stacks with
a Quadlet migration nudge:

```dockerfile
# === Compose stacks detected ===
# The following compose stacks were running on the source host.
# These are application workloads, not OS configuration.
# See compose/ in the build context for the original files.
#
# Consider converting to Quadlet units — .container files under
# /etc/containers/systemd/ that let systemd manage your container
# workloads natively.
#   man quadlet(5)
#   https://docs.podman.io/en/latest/markdown/podman-systemd.unit.5.html
#   https://docs.redhat.com/en/documentation/red_hat_enterprise_linux/10/html/building_running_and_managing_containers/porting-containers-to-systemd-using-podman
#
#   - /opt/myapp/docker-compose.yml (3 services)
#   - /srv/monitoring/docker-compose.yml (2 services)
```

### Refine UI

Compose stacks appear in the Containers reference section (existing).
Read-only — no include toggles. Expandable to view services and images.
Subtle banner: "Compose stacks should be deployed as container workloads
on the running system. Consider Quadlet units."

### Roadmap

Compose → Quadlet auto-conversion is a future feature. When implemented,
compose stacks would move from reference to a decision section with
toggleable quadlet generation.

## Refine UI: Section Topology

### Single-Host Sidebar

The final sidebar section list with new sections integrated:

**Review (decision sections):**
1. Packages *(existing)*
2. Config Files *(existing)*
3. Users & Groups *(existing)*
4. Services *(existing)*
5. Containers *(existing — quadlets, flatpaks)*
6. System Tuning *(existing)*
7. **Language Packages** *(new)*
8. **Unmanaged Files** *(new, flag-gated)*

**Reference (read-only sections):**
- Non-RPM Software *(existing — retains ELF/binary inventory not
  covered by Language Packages or Unmanaged Files)*
- Kernel & Boot *(existing)*
- Security & Access Control *(existing)*
- Scheduled Tasks *(existing)*

Language Packages replaces the pip/npm/gem portion of the existing
Non-RPM Software reference section. Non-RPM Software retains any items
not claimed by Tier 1 or Tier 2 (e.g., .env files, git repos, binaries
when `--include-unmanaged` is not used).

### Aggregate Mode

Language Packages and Unmanaged Files ship in aggregate mode from day one.
Deferring aggregate creates feature drift between modes — the same parity
problem this spec should prevent.

**Aggregate identity model:**

The new sections follow the same aggregate pattern as existing sections:
identity key determines grouping, prevalence shows host coverage, variant
selection handles divergence.

| Section | Identity Key | Prevalence | Variant Trigger |
|---------|-------------|------------|-----------------|
| Language Packages | ecosystem + environment path (e.g., `pip:/opt/myapp/venv`) | How many hosts have this environment | Different package lists across hosts for the same env path |
| Unmanaged Files | file path (e.g., `/opt/splunk/bin/splunkd`) | How many hosts have this file | Different file content (hash) across hosts |

This is the same model packages use (`name.arch` → prevalence → version
variants) and configs use (`path` → prevalence → content variants).

**Prevalence-based defaults:**
- Language environments present on 100% of hosts: `include: true`
- Language environments on <100% of hosts: `include: false`
- Same rule for unmanaged files (consistent with the Tier 2 aggregate
  default selection fix shipped in beta.5)

**Variant handling:**
- When hosts diverge on package versions within the same environment
  path, use the existing variant selection model: majority variant
  selected by default, user can switch via the variant picker
- For unmanaged files, content-hash comparison surfaces divergent files
  with the same variant picker UX

**Aggregate sidebar sections:**
Language Packages and Unmanaged Files appear in the aggregate sidebar
Review group with the same zone-based layout (consensus / near-consensus /
divergent) as other aggregate decision sections.

### Item Identity Contract

The refine plumbing uses `ItemId` for toggle/undo/redo operations and
DTO projection. New item types must define canonical identities that
are unique, stable across undo/redo, and serializable.

| Item Type | `ItemId` Variant | Identity Key | Example |
|-----------|-----------------|--------------|---------|
| Language Package env | `ItemId::LanguageEnv { ecosystem, path }` | ecosystem + environment path | `pip:/opt/myapp/venv` |
| Unmanaged File | `ItemId::UnmanagedFile { path }` | absolute file path | `/opt/splunk/bin/splunkd` |

These extend the existing `ItemId` enum in `crates/refine/src/types.rs`.
The `RefinementOp::SetInclude` operation already accepts any `ItemId`
variant — no new op types needed.

**DTO contract:** The web API response for language package and unmanaged
file sections must include per-item `id` fields matching the `ItemId`
serialization format. The React UI uses these IDs for toggle callbacks,
keyboard focus tracking, and search result targeting.

### Compose Sidebar Reconciliation

Compose stacks currently live in the Containers reference section in the
sidebar. This spec does not move them. The compose comment block in the
Containerfile and the `compose/` export root are new outputs, but compose
remains a reference surface in the sidebar — no include toggles, no
decision semantics.

**Global search:** Compose entries are searchable via global search
(matching on compose file path and service names), consistent with other
reference section items. Search results for compose items navigate to
the Containers reference section.

**Keyboard shortcut:** Compose is part of the Containers section (key 5).
No separate shortcut needed.

### RPM Upload Row Contract

Repo-less RPM packages appear in the existing Packages section alongside
normal RPMs. The upload interaction adds new row states that must be
truthful on the current Packages surface:

**Row states:**

| State | Checkbox | Badge | Primary Action | Containerfile |
|-------|----------|-------|----------------|---------------|
| Cached RPM, pre-excluded | Enabled, unchecked | Orange "No repo" | Toggle include | Commented `dnf localinstall` |
| Cached RPM, user-included | Enabled, checked | Orange "No repo" | Toggle include | Active `dnf localinstall` |
| No RPM, needs upload | Disabled | Orange "RPM needed" (clickable) | Click → upload modal | Commented `# MANUAL` |
| RPM uploaded, pre-excluded | Enabled, unchecked | Green "RPM provided" | Toggle include | Commented `dnf localinstall` |
| RPM uploaded, user-included | Enabled, checked | Green "RPM provided" | Toggle include | Active `dnf localinstall` |

**State transitions:**
- "RPM needed" → upload modal → success → "RPM provided" (checkbox
  enables, item remains excluded until user toggles)
- "RPM provided" → click "x" on badge → "RPM needed" (file removed,
  checkbox disables)

**ARIA:** Badge state changes announced via `aria-live="polite"` on the
row. Modal open/close announced via standard PatternFly modal semantics.

### Unmanaged Files Grouped Interaction

Items are grouped by parent directory (e.g., all files under `/opt/splunk/`
form one group). Groups are collapsible.

**Keyboard behavior:**
- Arrow keys navigate between groups (collapsed) or items (expanded)
- Enter/Space on a group header toggles expand/collapse
- Enter/Space on a group toggle changes include state for all children
- Tab moves between group toggle → first child toggle → next group

**Search behavior:**
- Section search matches on file path and file type
- When search matches items inside a collapsed group, the group
  auto-expands to show matches (same pattern as package group search)
- Search match count in section header reflects individual items, not
  groups

**Live announcements:**
- Group toggle: `aria-live="polite"` announces "Included 12 files in
  /opt/splunk" or "Excluded 12 files in /opt/splunk"
- Individual toggle: announces "Included /opt/splunk/bin/splunkd" or
  "Excluded /opt/splunk/bin/splunkd"
- Size rollup update announced after a debounce: "340 MB of 1.2 GB
  included"

### Section IDs and Keyboard Navigation

New section IDs for `useKeyboard.ts`:

| Section | ID | Key |
|---------|-----|-----|
| Packages | `packages` | 1 |
| Config Files | `configs` | 2 |
| Users & Groups | `users_groups` | 3 |
| Services | `services` | 4 |
| Containers | `containers` | 5 |
| System Tuning | `system_tuning` | 6 |
| Language Packages | `language_packages` | 7 |
| Unmanaged Files | `unmanaged_files` | 8 |

Key 8 (`unmanaged_files`) is a no-op when the section is not visible
(flag not used). Keys 9+ remain unassigned.

### Search Behavior

Global search (`/` or Ctrl+K) includes Language Packages and Unmanaged
Files in its scope. Section-level search within Language Packages
matches on: environment path, package names, ecosystem type. Section-level
search within Unmanaged Files matches on: file path, file type.

### Focus Management

Language Packages and Unmanaged Files inherit the same focus contract as
existing decision sections:
- Focus moves to the first item when the section is selected via
  sidebar click or keyboard shortcut
- Tab order: section header → items → toggles
- Escape returns focus to the sidebar

RPM upload modal focus:
- On open: focus moves to the file upload drop zone
- On close (success or cancel): focus returns to the "RPM needed" trigger
  label on the package row
- Tab order within modal: file upload area → file picker button →
  cancel → confirm

## CLI Changes

### New Flags

| Flag | Scope | Description |
|------|-------|-------------|
| `-y` / `--yes` | Global | Assume yes to all interactive prompts. For CI/automation. |
| `--include-unmanaged` | `scan` | Catalog and bundle unmanaged files from /opt, /srv, /usr/local. Prompts with total size before bundling (suppressed by `--yes`). |
| `--exclude-path <path>` | `scan` | Exclude specific paths from unmanaged file collection. Repeatable. |

### No New Flags For

- Language packages (pip/npm/gem) — always on
- Repo-less RPM handling — automatic
- Compose stacks — already collected, reference-only

### Behavior Changes

- Containerfile gains new sections: language packages, unmanaged files
  (when opted in), repo-less RPM handling, compose comment block
- Scan prompts for confirmation when `--include-unmanaged` finds files
- Refine UI gains: Language Packages decision section, Unmanaged Files
  decision section (flag-gated with discoverability hint), RPM upload
  modal with batch support, compose reference with Quadlet nudge

## Data Model Changes

### NonRpmItem Extensions

```rust
pub struct NonRpmItem {
    // ... existing fields ...

    /// Collected manifest file contents for this project/environment.
    /// Key: filename (e.g., "requirements.txt", "package-lock.json")
    /// Value: raw file content
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub manifest_files: HashMap<String, String>,

    /// Provenance confidence: "high", "medium", "low"
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub confidence: String,  // already exists, semantics tightened

    /// Whether this item was RPM-ownership-filtered (pip only)
    #[serde(default)]
    pub rpm_filtered: bool,
}
```

### ComposeFile Extension

```rust
pub struct ComposeFile {
    // ... existing fields ...

    /// Raw compose YAML content, retained for verbatim export.
    /// Subject to redaction rules when snapshot is in redacted state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_content: Option<String>,
}
```

### Tarball Layout

New directories in the export tarball:

| Root | Purpose | Gate |
|------|---------|------|
| `language-packages/` | Manifest files for pip/npm/gem | Always |
| `repoless-packages/` | Cached and uploaded RPM files | Automatic |
| `compose/` | Preserved compose files | When compose detected |
| `unmanaged/` | Copied unmanaged files | `--include-unmanaged` |

All four roots must be added to `render_refine_export()` allowlist in
`crates/refine/src/session.rs`. Export contract tests and
`docs/reference/output-artifacts.md` must be updated to match.

## Schema Version

These changes add new fields and artifact roots. Schema version must be
bumped. The new fields use `#[serde(default)]` so older tarballs remain
loadable (new fields default to empty/None). Newer tarballs with these
fields are not loadable by older inspectah versions (forward-incompatible),
which is the existing schema versioning policy.

## Future Work

- **Compose → Quadlet auto-conversion:** Convert docker-compose.yml to
  `.container` quadlet units. Simple stacks (1-3 services) auto-convert;
  complex stacks get best-effort with manual review flag.
- **Non-lockfile npm/gem detection:** Scan `node_modules` or gem
  directories when no lockfile exists. Requires new provenance rules
  and confidence labeling. Lower priority — lockfile-backed detection
  covers the high-confidence cases.
- **Java artifact detection:** `find *.jar *.war *.ear` probe for
  deployed Java artifacts in runtime directories.
- **Maven dependency scanning:** `pom.xml` / `.m2/repository` detection
  for Java build-time dependencies (lower priority — build-time concern).
