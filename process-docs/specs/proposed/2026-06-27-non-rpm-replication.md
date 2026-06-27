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

## Tier 1: Language Package Replication

### Activation

Always on. No flag needed. Language packages are reproducible application
dependencies that belong in the migration output alongside RPM packages
and config files.

### Detection (existing)

The non-RPM inspector already detects pip, npm, and gem packages by
scanning `/opt`, `/srv`, `/usr/local`:

- **pip:** `pip list --format json`, dist-info directory fallback, venv detection
- **npm:** `package-lock.json` parsing (lockfileVersion 1/2/3)
- **gem:** `Gemfile.lock` parsing

### Containerfile Rendering

Each ecosystem has its own rendering logic. Fidelity comments indicate
how the package list was detected.

**pip:**

Venvs are recreated faithfully. The source host chose a venv for a
reason (dep isolation between apps, system Python protection) — that
reason doesn't go away in the image.

- Venv with `requirements.txt`:
  ```dockerfile
  # pip packages: /opt/myapp/venv (from requirements.txt)
  COPY requirements.txt /tmp/myapp-requirements.txt
  RUN python3 -m venv /opt/myapp/venv \
      && /opt/myapp/venv/bin/pip install -r /tmp/myapp-requirements.txt \
      && rm /tmp/myapp-requirements.txt
  ```
- Venv without `requirements.txt`:
  ```dockerfile
  # pip packages: /opt/myapp/venv (detected via dist-info — transitive deps may differ)
  RUN python3 -m venv /opt/myapp/venv \
      && /opt/myapp/venv/bin/pip install flask==2.3.3 requests==2.31.0
  ```
- System-level pip (no venv):
  ```dockerfile
  # pip packages: system (detected via pip list)
  RUN pip install flask==2.3.3 requests==2.31.0
  ```

Inline pinned versions are preferred over lockfile-copy for pip because
binary wheels are platform-specific. `pip install pkg==ver` lets pip
fetch the correct wheel for the target architecture.

**npm:**

- `package-lock.json` found:
  ```dockerfile
  # npm packages: /opt/myapp (from package-lock.json)
  COPY package.json package-lock.json /opt/myapp/
  RUN cd /opt/myapp && npm ci --production
  ```
- No lockfile:
  ```dockerfile
  # npm packages: /opt/myapp (detected via node_modules — lockfile not found)
  RUN npm install -g express@4.18.2 lodash@4.17.21
  ```

**gem:**

- `Gemfile.lock` found:
  ```dockerfile
  # gem packages: /opt/myapp (from Gemfile.lock)
  COPY Gemfile Gemfile.lock /opt/myapp/
  RUN cd /opt/myapp && bundle install --deployment
  ```
- No lockfile:
  ```dockerfile
  # gem packages: /opt/myapp (detected via Gemfile.lock parse)
  RUN gem install rack -v 3.0.8 && gem install sinatra -v 3.1.0
  ```

### Implementation Notes

**npm/gem grouping:** The non-RPM inspector stores npm and gem packages as
individual `NonRpmItem` entries (one per package). The renderer must group
items by parent path to emit one install command per project, not one
`RUN` per package. Branch on `item.method` (`"npm lockfile"` vs other) to
decide lockfile-copy vs. inline.

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

New "Language Packages" decision section in the aggregate and single-host
sidebar. Include toggles per environment (whole venv or npm project as a
unit, not per individual package). Standard toggle behavior matching
packages and configs.

## Tier 2: Unmanaged Files

### Activation

Opt-in via `--include-unmanaged` scan flag. Covers ELF binaries, JARs,
scripts, and anything else in `/opt`, `/srv`, `/usr/local` that isn't
claimed by a language package manager or RPM.

### Scan Behavior

1. Catalog all unmanaged files with: path, size, type (ELF/JAR/script/other),
   last modified
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
- Each item shows: path, size, type
- Running size rollup in section header with denominator for context:
  "4 of 12 items included, ~340 MB of ~1.2 GB" — updates real-time as
  toggles change
- "Include None" bulk action (default state is all-included; "Reset to All"
  link handles the reverse)
- Items toggled off are excluded from the export tarball

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

### Containerfile Rendering

- Cached RPM available:
  ```dockerfile
  COPY repoless-packages/custom-tool-1.2.3.x86_64.rpm /tmp/
  RUN rpm -i /tmp/custom-tool-1.2.3.x86_64.rpm && rm /tmp/custom-tool-1.2.3.x86_64.rpm
  ```
- No cached RPM:
  ```dockerfile
  # MANUAL: custom-tool (no repo source, RPM not in cache)
  # Provide the RPM or add the repo, then uncomment:
  # RUN dnf install custom-tool
  ```

### Refine UI

These packages appear in the normal Packages section (they're RPMs).
New triage annotations:

- "No repo source — cached RPM bundled"
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

After successful upload: label turns green "RPM provided" with checkmark.
Row checkbox becomes fully functional.

**Batch upload:**

When multiple packages need RPMs, a toolbar-level "Upload RPMs" button
appears. Opens a `MultipleFileUpload` modal where users drop multiple
`.rpm` files. inspectah auto-matches each file to the correct package by
parsing the RPM filename. Shows match table (matched/unmatched/conflicts)
before confirming.

**Accessibility:**

- `aria-live="polite"` on row status region for screen reader announcements
- Keyboard-native file picker activation in modal
- Checkbox visible but with warning tooltip pre-upload: "RPM file required
  before this package can be included"

### Export

Cached RPMs and uploaded RPMs are included in the export tarball under
`repoless-packages/`.

## Compose Stacks

### Approach

Reference-only — no Containerfile replication. Compose workloads are
applications running on top of the OS, not OS image content. Baking
compose stacks into an immutable bootc image conflates the OS and the
workload and breaks the immutable update model.

### Scan Behavior

No change — compose files are already detected by the containers inspector.

### Tarball

Original compose files preserved verbatim under `compose/` in the tarball,
with directory structure mirrored (e.g., `compose/opt/myapp/docker-compose.yml`).

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

The existing `NonRpmItem` struct may need:
- `lockfile_path: Option<String>` — path to lockfile when detected via
  lockfile parse (enables lockfile-copy rendering)
- `environment_path: String` — parent path grouping key for npm/gem
  rendering (may be derivable from existing `path` field)

### New Snapshot Fields

- `repoless_packages: Option<Vec<RepolessPackage>>` — packages with no
  repo source, with optional cached RPM path. May be derivable from
  existing `PackageEntry` fields without a new type.

### Tarball Layout

New directories in the export tarball:
- `repoless-packages/` — cached and uploaded RPM files
- `compose/` — preserved compose files (directory structure mirrored)
- `unmanaged/` — copied unmanaged files (when `--include-unmanaged` used)

## Open Questions

None — all design questions resolved during brainstorm.

## Future Work

- **Compose → Quadlet auto-conversion:** Convert docker-compose.yml to
  `.container` quadlet units. Simple stacks (1-3 services) auto-convert;
  complex stacks get best-effort with manual review flag.
- **Java artifact detection:** `find *.jar *.war *.ear` probe for
  deployed Java artifacts in runtime directories.
- **Maven dependency scanning:** `pom.xml` / `.m2/repository` detection
  for Java build-time dependencies (lower priority — build-time concern).
