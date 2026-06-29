# Release Notes: v0.8.7-beta.1

## What's new in v0.8.7-beta.1

This is the biggest inspectah release yet. Non-RPM software replication lands as a complete tier system, covering language packages (pip/npm/gem), unmanaged files, and repo-less RPMs. Detection accuracy improvements fix several real-world false positives discovered during hands-on testing.

### Non-RPM software replication

The headline feature: inspectah now detects and replicates software that lives outside the RPM ecosystem.

**Tier 1 -- Language packages (pip, npm, gem):**
- Pip virtual environments are detected and recreated faithfully in the Containerfile. npm and gem projects use lockfile-copy.
- Confidence-based rendering: high-confidence environments get active `COPY`/`RUN` lines, medium are commented-out, low are advisory-only.
- Manifest files (requirements.txt, package.json, Gemfile) are exported alongside the tarball.
- New refine UI section with per-environment toggles, confidence badges, package counts, and manifest basis labels.

**Tier 2 -- Unmanaged files (`--include-unmanaged`):**
- Files from /opt, /srv, /usr/local that aren't owned by RPM or Tier 1 language packages are cataloged with provenance signals (mutability, writable mount, service working directory).
- Size confirmation prompt prevents accidentally bundling large directories (suppressible with `-y`/`--yes`).
- New refine UI section with directory grouping, per-item toggles, provenance badges, and running size rollup.
- Symlinks detected without following, preserved as tar entries, rendered as `RUN ln -sf` directives.

**Tier 3 -- Repo-less RPMs:**
- Packages with no repo source or a disabled/removed repo are detected automatically.
- Cached RPMs from `/var/cache/dnf/` are bundled; missing RPMs get a `MANUAL` annotation.
- Refine UI upload endpoint (`POST /api/upload-rpm`) for manual RPM provision with single-file and batch upload modals.
- Five row states: cached_excluded, cached_included, needs_upload, uploaded_excluded, uploaded_included.

**Compose reference:**
- Detected compose files listed as a reference-only comment block in the Containerfile with Quadlet migration guidance.
- Raw YAML exported under `compose/` in the tarball, subject to secret redaction.

### Detection accuracy

Fixes found during hands-on testing of real RHEL systems:

- **RPM repo-less false positives** -- repo-less detection now uses case-insensitive substring matching between install-time short names (`AppStream`, `baseos`) and full repo IDs (`rhel-9-for-aarch64-appstream-rpms`). Previously ~50% of packages were falsely flagged on real RHEL systems.
- **Python venv underdetection** -- removed `venv` from PRUNE_DIRS so the venv walker can discover environments at the most common path (`/opt/myapp/venv/`).
- **npm underdetection** -- added package.json manifest fallback scan for npm projects without `package-lock.json`.
- **Ruby gem underdetection** -- added system gem detection via `gem list --local` with RPM ownership filtering.
- **Duplicate repo display** -- package tables now use the same repo identifier as the config tree. Source repo short names normalized to full repo IDs using `.repo` file section headers.
- **RPM ownership check** -- pip RPM filtering now uses `rpm -qf` path ownership proof instead of `python3-<name>` heuristic, preventing false suppression of user-managed packages.
- **Deduplication uses ecosystem+path** -- language environment dedup key includes ecosystem, preventing same-path npm+gem projects from collapsing.

### RPM upload flow

Improved UX for providing repo-less RPMs:

- **Name+arch matching** -- uploaded RPMs match repo-less packages by name and architecture instead of requiring exact NEVRA filenames. Supports vendor downloads, COPR, and manual builds.
- **Upload feedback** -- modals show whether the RPM matched a repo-less package with inline warnings for unmatched files, version-mismatch info display, and an export confirmation gate for unmatched uploads.
- **Batch upload** -- shows list of packages needing RPMs with live match progress, collapsible checklist with green/grey labels.

### Refine UI polish

- **Unconditional sections** -- Language Packages and Unmanaged Files sections now appear in the sidebar unconditionally with explicit empty states ("None detected" / "Not scanned") instead of being hidden when empty.
- **Discoverability hint** -- sidebar hint when scan was run without `--include-unmanaged`, guiding users to re-run for coverage.
- **Global search** -- language packages searchable by environment path, package name, and ecosystem. Unmanaged files searchable by path.
- **Aggregate support** -- both Language Packages and Unmanaged Files sections available in aggregate mode with zone-based layout, prevalence badges, and variant comparison.

### Documentation

Comprehensive documentation update shipped alongside the feature work:

- New how-to guides for non-RPM software replication and repo-less RPMs.
- Updated CLI reference covering new flags (`--include-unmanaged`, `--exclude-path`, `-y`/`--yes`).
- Updated inspector coverage and output artifacts documentation.

### Other improvements

- **`-y`/`--yes` global CLI flag** -- suppresses interactive prompts for CI/automation use.
- **`--exclude-path` scan flag** -- repeatable path exclusion for unmanaged file collection.
- **Manifest redaction** -- redacted exports now scrub auth-bearing URLs from `requirements.txt`, `package.json`, `package-lock.json`, `Gemfile`, and `Gemfile.lock`.
- **Pip venv paths normalized** -- venv renderer now produces absolute paths instead of relative.

### Binaries

Pre-built binaries for 3 platforms:
- `inspectah-darwin-arm64` -- macOS on Apple Silicon
- `inspectah-linux-arm64-bin` -- Linux on ARM64 (static musl binary)
- `inspectah-linux-amd64` -- Linux on x86_64 (static musl binary)

**Full changelog:** https://github.com/marrusl/inspectah/compare/v0.8.6-beta.5...v0.8.7-beta.1
