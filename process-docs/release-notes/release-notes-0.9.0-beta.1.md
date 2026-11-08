# Release Notes: v0.9.0-beta.1

## What's new in v0.9.0-beta.1

This release rolls up two work streams: an extended-findings overhaul that changes how inspectah represents non-toggleable information (network data, advisories), and a refine/TUI convergence pass that brings the web sidebar and keyboard navigation in line across single-host and aggregate modes. It also includes language package detection v2 for npm and pip, a large correctness fix stream, and a fix to the default target base image resolution.

### Extended findings

Findings now carry explicit semantics instead of a single include/exclude boolean:

- **Three finding kinds** -- `Actionable` findings keep the familiar include/exclude toggle. `Advisory` findings (cross-tree symlinks, modernization notes, unbacked `/var` directories) show rationale but aren't toggleable. `Inventory` findings (network connections, routes, firewall rules) are informational-only everywhere they appear.
- **Network data as inventory** -- network connections, firewall zones, static routes, IP rules, resolv.conf provenance, `/etc/hosts` additions, and proxy entries now render as a dedicated inventory section in the HTML report and as non-toggleable rows in the TUI. Previously only NM connections were shown, and network items were (incorrectly) toggleable in the TUI.
- **ifcfg deprecation notice** -- HTML report and TUI both flag legacy `network-scripts` connections with a deprecation advisory.
- **`/var` directory discovery** -- the storage inspector now scans `/var/lib`, `/var/log`, and `/var/cache` for directories not owned by RPM and classifies how each is backed (tmpfiles.d, systemd `StateDirectory`/`CacheDirectory`/`LogsDirectory`, or genuinely unbacked). Unbacked directories show ownership and mode in the HTML report, TUI, and refine web storage sections, and the Containerfile emits `tmpfiles.d` provisioning (or a fallback `RUN mkdir -p`) so they exist on first boot.
- **Full-shadow service detection** -- services with a complete unit-file override at `/etc/systemd/system/` are now detected in every case, including services that already match their preset default and services with no `.service.d/` drop-in directory. They show up as actionable findings with rationale about base-image update implications, an amber "Shadow override" badge in the refine UI, and a shadow count on the section header.
- **Merge preserves finding semantics** -- aggregate and fleet merges no longer collapse Advisory/Inventory findings back to a boolean. A new `with_include()` method on `FindingKind` keeps the distinction through the merge.

### Refine and TUI convergence

The refine web sidebar and its keyboard/batch-action handling were rebuilt for consistency across sections:

- **8-group sidebar** -- the sidebar is now data-driven from a `/api/groups` endpoint, organized into Packages, System Config, Services & Scheduling, Users & Identity, Network, Storage, Software, and Secrets. Collapsed state persists across sections, and number keys 1-8 jump between groups.
- **Sidebar badges** -- triage sections show blue badges with decision counts, reference sections show grey badges, and a section that's fully excluded shows a "0" badge with a screen-reader announcement.
- **Batch toggle, now per-section** -- "Include all" / "Exclude all" (and the Ctrl+Shift+A/X shortcuts) are anchored to individual triage sections rather than group headings, plus a separate group-level batch toggle on triage group headings (with a confirmation dialog before excluding an entire Packages group). Batch-toggle undo is now a single atomic step instead of one undo per item.
- **Reset to defaults** -- a new button next to undo/redo reverts all include/exclude changes back to the initial analysis state, gated behind a confirmation dialog.
- **Auto-exclude reason labels** -- pre-excluded, unlocked packages show a grey badge explaining why: installer default, unclear provenance, no repo source, or dependency.
- **Empty states** -- a fully-excluded config section shows an "All configuration files excluded" message instead of an empty list, and a repo with `incomplete` provenance (e.g. EPEL) now gets its toggle switch back -- it was previously impossible to deselect.

### Language package detection v2

- **npm global packages** -- globally-installed npm packages are detected via `npm list -g --json` (high confidence) and a directory walk fallback (medium confidence), with per-prefix binding, RPM filtering, and scoped package support (`@angular/cli`). They support per-package pin/unpin in refine (rendered as `name@version` in the generated `npm install -g` command) and expand in the refine UI to show individual packages with keyboard navigation and search-driven auto-expansion.
- **C-extension detection** -- pip environments with native `.so` files in site-packages are flagged and shown with an orange "C extensions" badge.
- **system_site_packages badge** -- pip venvs created with `--system-site-packages` are labeled in the refine UI, and the flag is now correctly included when inspectah regenerates the `python3 -m venv` command.
- **Scan expansion** -- `--scan-home all|user,...` and `--scan-path /path` extend non-RPM scanning beyond the default roots; `/var/www` is now a default scan root. Scan scope is persisted in the snapshot.
- **Tier 1/Tier 2 overlap fixed** -- system gems and npm manifest-detected projects were being duplicated as unmanaged-file `COPY` lines in the Containerfile. All language detection methods now feed the scan exclusion filter correctly.

### Target base image resolution

Default target resolution stays **version-pinned** (a floor tag like `9.6`, not `:latest`) for every target. RHEL 8 and CentOS Stream 8 hosts, which have no bootc base image of their own, now resolve to the EL9 floor tag by default (`registry.redhat.io/rhel9/rhel-bootc:9.6`, `quay.io/centos-bootc/centos-bootc:stream9`), and an EL8-to-EL9 migration is classified as a major upgrade rather than same-stream.

If you want to track `:latest` instead, pass it explicitly with `--base-image`, which accepts any tag or digest as an override.

### Other fixes

- **Renderer shell safety** -- paths and package names in generated Containerfile `RUN` lines are now single-quoted; tokens containing single quotes or newlines are rejected with a warning comment instead of being emitted unescaped.
- **Symlink containment** -- all recursive directory walkers now enforce scan-root containment and cycle detection, closing a path-escape route through directory symlinks in user-controlled trees.
- **Scan-root deduplication** -- `--scan-home`/`--scan-path` dedup now compares path components instead of string prefixes, so `/var/www2` is no longer incorrectly suppressed by `/var/www`.
- **Bundler deprecation** -- Containerfile rendering now uses `bundle config set --local deployment 'true' && bundle install` instead of the deprecated `bundle install --deployment`.
- **Unreadable shadow files** -- a bare shadow file under `/etc/systemd/system/` that can't be read now triggers inspector degradation instead of silently producing an empty entry.
- **`/var` fixes** -- directory discovery now goes to depth 2 (catching cases like `/var/lib/pgsql/data`) and dedupes parent/child pairs; the advisory wording no longer claims unbacked `/var` directories are lost on reboot (they aren't -- the actual issue is the lack of declarative lifecycle management).

### Schema version

Schema version bumped to 22 (from 20 in v0.8.7-beta.1). Tarballs from older schema versions are no longer loadable.

### Binaries

Pre-built binaries for 3 platforms:
- `inspectah-darwin-arm64` -- macOS on Apple Silicon
- `inspectah-linux-arm64-bin` -- Linux on ARM64 (static musl binary)
- `inspectah-linux-amd64` -- Linux on x86_64 (static musl binary)

**Full changelog:** https://github.com/marrusl/inspectah/compare/v0.8.7-beta.1...v0.9.0-beta.1
