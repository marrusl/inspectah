# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Extended finding kinds** — findings now carry Advisory, Inventory, or Actionable semantics. Advisory findings (cross-tree symlinks, modernization, unbacked /var) display rationale but are non-toggleable. Inventory findings (network connections) are informational-only across all surfaces.
- **Network section in HTML report** — network connections now render as an inventory section in the HTML report, with ifcfg deprecation advisory when legacy network-scripts are detected.
- **`/var` directory discovery** — storage inspector scans /var/lib, /var/log, /var/cache for non-trivial directories and classifies their backing mechanism (tmpfiles.d, StateDirectory, CacheDirectory, LogsDirectory, RPM-owned, unbacked).
- **Full-shadow detection for all services** — full unit-file shadows at /etc/systemd/system/ are detected even when no .service.d/ drop-in directory exists, with rationale about base-image update implications.
- **8-group sidebar** — refine web sidebar replaced with data-driven NavExpandable groups (Packages, System Config, Services & Scheduling, Users & Identity, Network, Storage, Software, Secrets) sourced from a `/api/groups` endpoint. Collapsed state persists across sections.
- **Sidebar badges** — triage sections show blue badges with decision counts; reference sections show grey badges. Cleared-state "0" badge with screen-reader announcement when all items in a section are excluded.
- **Sidebar keyboard navigation** — number keys 1-8 jump to groups, aria-current stays on active section when parent is collapsed, focus restores to group heading on collapse.
- **Group-level batch toggle** — kebab menu on triage group headings with "Include all" / "Exclude all". Packages group has a confirmation dialog for "Exclude all". Ctrl+Shift+A/X shortcuts.
- **Full-shadow service rendering** — services with full unit-file shadows show a warning amber border, "Shadow override" gold badge, and shadow rationale linked via aria-describedby. Section header shows shadow count.
- **ifcfg deprecation banner** — network section shows a PatternFly info Alert when legacy network-scripts connections are detected, with note text from the backend constant.
- **tmpfiles.d provisioning** — Containerfile emits `/usr/lib/tmpfiles.d/inspectah-var.conf` for unbacked /var directories with known ownership and mode. Directories without ownership data fall back to `RUN mkdir -p`. Ownership resolution follows a 5-step priority (root → materialized user → RPM-packaged → numeric with comment → numeric).
- **/var ownership display** — unbacked /var directories show ownership and mode (e.g., `0750 postgres:postgres`) in the HTML report, TUI, and refine web storage sections.
- **Orphan full-shadow synthesis** — services with `/etc/systemd/system/` shadows but no state divergence entry now get synthetic ServiceStateChange entries, making them real toggle targets through the full refine lifecycle (session, toggle, autosave, reload, export).

### Changed
- **All RHEL and Fedora targets use :latest** — RHEL 9.x, RHEL 10.x, and Fedora targets now all resolve to `:latest` instead of pinning to a version floor tag. The bootc ecosystem tracks the latest available minor within a major, making version pinning unnecessary.

### Fixed
- **`/var` directory depth-2 discovery** — `discover_var_directories()` now scans to `-maxdepth 2`, catching spec acceptance cases like `/var/lib/pgsql/data`. Deduplicates by keeping only leaf directories when both parent and child are discovered.
- **`/var` advisory wording corrected** — unbacked /var directories are not ephemeral on reboot (as previously stated); the actual problem is lack of declarative lifecycle management. Wording corrected across HTML report, TUI, and projection types.
- **Unreadable shadow file degradation** — bare shadow files in `/etc/systemd/system/` that cannot be read now trigger inspector degradation instead of silently producing empty content entries.
- **Unbacked /var paths in web adapter** — `web_storage_section()` now includes `unbacked_var_paths` as advisory items, making them visible in the React frontend.
- **Orphan full-shadow drop-ins in HTML report** — preset-matched services with full shadows (no `state_changes` entry) now appear as actionable service findings with rationale helper text in the HTML services table.
- **ifcfg deprecation note in TUI** — the `IFCFG_DEPRECATION_NOTE` advisory is now shown in the TUI Network section when connections use legacy network-scripts paths.
- **Full-shadow detection for preset-matched services** — services matching their preset (e.g., sshd enabled with preset=enable) were invisible to full-shadow detection because they went into `preset_matched_units`, not `state_changes`. An independent scan of `/etc/systemd/system/` now catches shadows regardless of which bucket the unit landed in.
- **Unbacked /var directories in Containerfile** — unbacked /var directories now emit `RUN mkdir -p` lines in the Containerfile output. Previously collected but only surfaced in the audit report.
- **Unbacked /var advisory in HTML report, refine, and TUI** — the unbacked /var advisory now renders in the HTML report storage section, is projected into RefStorage for the refine view, and appears as an advisory item in the TUI.
- **Complete network data in HTML report** — the HTML report now renders firewall zones, firewall direct rules, static routes, IP routes, IP rules, resolv.conf provenance, /etc/hosts additions, and proxy entries. Previously only NM connections were shown. The network item count now reflects all network data types.
- **Merge preserves finding semantics** — aggregate and fleet merge operations no longer collapse Advisory/Inventory findings back to boolean include/exclude. The `with_include()` method on `FindingKind` preserves non-actionable variants through merge.
- **TUI network items non-toggleable** — network connections, firewall zones, routes, and other network inventory items are now rendered as non-toggleable inventory rows in the TUI. Toggle guard and session-level validation reject inventory ItemId variants.
- **Batch-toggle slug routing** — `POST /api/batch-toggle/:group_slug` now uses `SectionGroup` slugs instead of ad-hoc group names. Reference-only groups are rejected.
- **Language package / unmanaged file double-counting** — system gems and npm manifest projects are no longer duplicated as unmanaged COPY lines in the Containerfile. All six language detection methods now feed the scan exclusion filter, and system gems use the actual gem directory path for prefix matching.

## [0.8.7-beta.1] - 2026-06-29

### Added
- **Language package replication (Tier 1)** — pip, npm, and gem environments are detected, rendered as executable Containerfile output, and exported with manifest files. Pip venvs are recreated faithfully; npm/gem use lockfile-copy. Confidence-based rendering: high=active, medium=commented-out, low=advisory.
- **Unmanaged file collection (Tier 2)** — `--include-unmanaged` catalogs files from /opt, /srv, /usr/local not owned by RPM or Tier 1 language packages. Includes provenance signals (mutability, writable mount, service working directory), size confirmation prompt (suppressible with `-y`/`--yes`), and per-file toggles in refine.
- **Repo-less RPM handling (Tier 3)** — packages with no repo source or a disabled/removed repo are detected automatically. Cached RPMs from `/var/cache/dnf/` are bundled; missing RPMs get a `MANUAL` annotation. Refine UI upload endpoint (`POST /api/upload-rpm`) allows manual RPM provision.
- **`-y`/`--yes` global CLI flag** — suppresses interactive prompts for CI/automation use.
- **`--exclude-path` scan flag** — repeatable path exclusion for unmanaged file collection.
- **Symlink-safe unmanaged scanning** — symlinks are detected without following, preserved as tar symlink entries, and rendered as `RUN ln -sf` directives in the Containerfile.
- **Refine UI: Language Packages section** — new decision section showing pip/npm/gem environments with per-environment toggles, confidence badges, package counts, and manifest basis labels. Keyboard shortcut 6.
- **Refine UI: Unmanaged Files section** — new decision section with directory grouping, per-item and per-group toggles, provenance signal badges (mutability, writable mount, service workdir), running size rollup, /var path warnings, and ArrowLeft/Right keyboard expand/collapse. Keyboard shortcut 7.
- **Refine UI: RPM upload modals** — single-file upload with NEVRA validation and batch upload with auto-matching, conflict detection, and matched/unmatched/conflicts view. Focus trap and focus return on close.
- **Refine UI: repo-less RPM row states** — packages without repo sources show upload icon instead of checkbox. Five states: cached_excluded, cached_included, needs_upload, uploaded_excluded, uploaded_included. Row-level aria-live announcements for state transitions.
- **Refine UI: `--include-unmanaged` discoverability** — sidebar hint when scan was run without `--include-unmanaged`, guiding users to re-run for unmanaged file coverage.
- **Global search for new sections** — language packages searchable by environment path, package name, and ecosystem. Unmanaged files searchable by path. Results navigate to correct section with reveal highlighting.
- **Compose stack reference** — detected compose files are listed as a reference-only comment block in the Containerfile with Quadlet migration guidance. Raw YAML exported under `compose/` in the tarball, subject to secret redaction.
- **Aggregate: language packages section** — aggregate mode now includes a Language Packages section with zone-based layout, prevalence badges, ecosystem/confidence metadata, package-list variant diffs, and searchable package names.
- **Aggregate: unmanaged files section** — aggregate mode now includes an Unmanaged Files section with zone-based layout, file type/size metadata, provenance signals in detail pane, content-hash variant comparison, and searchable file paths.

### Changed
- **Refine sidebar** — Language Packages and Unmanaged Files sections now appear unconditionally with explicit empty states instead of being hidden when empty
- **RPM upload feedback** — upload modals now show whether the RPM matched a repo-less package, with inline warnings for unmatched files, version-mismatch info display, and an export confirmation gate for unmatched uploads
- **RPM batch upload modal** — shows list of packages needing RPMs with live match progress before the drop zone, using a collapsible checklist with green/grey labels
- **Manifest redaction coverage** — redacted exports now scrub auth-bearing URLs from `requirements.txt`, `package.json`, `package-lock.json`, `Gemfile`, and `Gemfile.lock` in both sidecar files and `inspection-snapshot.json`.
- **RPM ownership check** — pip RPM filtering now uses `rpm -qf` path ownership proof instead of `python3-<name>` heuristic, preventing false suppression of user-managed packages.
- **Deduplication uses ecosystem+path** — language environment dedup key includes ecosystem, preventing same-path npm+gem projects from collapsing. System pip now collects all packages into a single environment entry.

### Fixed
- **RPM upload matching** — uploaded RPMs now match repo-less packages by name and architecture instead of requiring exact NEVRA filenames. Supports non-standard filenames from vendor downloads, COPR, and manual builds. Upload endpoint now returns match status (`matched`/`unmatched`) with canonical `name.arch`.
- **RPM repo-less false positives** — repo-less detection now uses case-insensitive substring matching between install-time short names (`AppStream`, `baseos`) and full repo IDs (`rhel-9-for-aarch64-appstream-rpms`). Previously ~50% of packages were falsely flagged on real RHEL systems.
- **Python venv detection for "venv" directories** — removed `venv` from PRUNE_DIRS so the venv walker can discover environments at the most common path (`/opt/myapp/venv/`).
- **npm detection for projects without lockfile** — added package.json manifest fallback scan for npm projects without `package-lock.json`. Detected with method `npm manifest` and confidence `low`.
- **Ruby gem detection for system-installed gems** — added system gem detection via `gem list --local` with RPM ownership filtering. Detected with method `gem system` and confidence `medium`/`low`.
- **Duplicate repo display** — package tables now use the same repo identifier as the config tree. Source repo short names are normalized to full repo IDs using `.repo` file section headers.
- **Pip venv paths normalized** — venv renderer now produces absolute paths (`/opt/myapp/venv`) instead of relative (`opt/myapp/venv`).
- **Refine exclusion honored** — high-confidence language environments with `include: false` no longer emit active `COPY`/`RUN` lines.
- **Export contract test method strings** — test fixtures updated to use canonical method constants.

## [0.8.6-beta.5] - 2026-06-27

### Added
- **Prevalence badge display toggle** — clicking any prevalence badge in aggregate mode toggles between fraction (45/50) and flat percentage (90%) display. Toggle is global — one click changes all badges.
- **Aggregate sidebar include/total counts** — decision sections in aggregate sidebar now show "N included / M total" in their badges, matching the per-section counts available in single-host mode.

### Changed
- **Aggregate stats bar simplified** — removed "N need review" / "All reviewed" labels from the aggregate stats bar header. Host count and total items remain.
- **Aggregate sidebar simplified** — removed "N/M confirmed" ack progress labels from sidebar nav items. Item count badges remain.

### Fixed
- **Dark mode prevalence badge contrast** — prevalence badges now have proper dark-mode color overrides instead of hardcoded light-mode colors.
- **Keyboard navigation in aggregate mode** — number keys (1-9) now jump to the correct aggregate sections. Previously they sent single-host section IDs, causing partial navigation failures.
- **Aggregate default selections** — packages and config files in aggregate mode now correctly default to excluded when not present on 100% of hosts. Previously all items defaulted to selected regardless of prevalence, requiring manual deselection of partial-prevalence items.
- **False "unredact hashes" offer** — User Artifact Preview no longer shows the redact/reveal banner when the displayed content has no redacted material. ContainerfilePanel "Reveal hashes" button also hidden when the Containerfile has no crypt(3) hashes.
- **Container row click target** — entire quadlet row is now clickable to expand/collapse the unit file content, not just the small chevron indicator. Follows the same pattern as package decision rows.
- **RHEL repo classification** — RHEL-style long repo IDs (e.g. `rhel-9-for-x86_64-baseos-rpms`) are now correctly classified as distro repos. Previously, only short CentOS-style IDs (`baseos`, `appstream`) were recognized, causing RHEL base repos to appear as toggleable third-party repos instead of always-on.
- **Ungrouped packages disappearing** — ungrouping a DNF package group now correctly surfaces individual members in the package list and Containerfile. Previously, non-leaf group members were filtered out by the leaf dependency filter after ungrouping. Removed the empty `# Ungrouped from "..."` Containerfile comment.
- **Inspectah COPR repo in config files** — inspectah's own COPR repo definition is now auto-excluded from config files and repo file output. The migration tool should never carry its own repo into the target image.
- **Group toggle removed** — removed the non-functional group-level toggle switch. Groups are managed via the ungroup button (dissolves into individual packages) or per-member actions.

## [0.8.6-beta.4] - 2026-06-22

### Added
- **Subscription cert expiry display** — scan output now shows entitlement certificate expiration date when `--preserve subscription` is used. Warns at <7 days remaining, errors if already expired. Also included in the generated README.
- **Anaconda gap classifier** — packages installed by the RHEL installer (Anaconda) are now classified as platform plumbing and auto-excluded from migration scope. Dramatically reduces migration noise by hiding installer-default packages.
- **Package group dependency visibility** — group members now show whether they're already in the base image. Summary labels distinguish "new" members from base-image members. Progressive disclosure replaces fixed truncation for long group member lists.
- **Version changes table** — context section now renders version changes as a grouped table (upgrades/downgrades with EVR formatting) instead of simple list.
- **Networking subsections** — context section networking split into clear subsections: Connections, Firewall, Routes & Rules, DNS & Hosts, Proxy.
- **Kernel & boot subsections** — context section kernel & boot split into Customizations vs Defaults/Context for clearer organization.
- **Service state display** — refine UI now shows both current (host) state and preset default alongside each service.
- **Container section quadlet content** — quadlet unit file content now viewable inline via expand/collapse in container section.
- **Sidebar subsection counts** — sidebar section counts correctly sum subsection items for subsection-only sections.
- **Accessibility improvements** — ContextList subsections use semantic headings and ARIA region landmarks.
- **Pull failure classification** — five error categories (registry unreachable, auth required, image not found, TLS/cert error, unknown) with tailored remediation guidance including disconnected-environment workarounds.
- Build metadata in version output — `inspectah version` and `--version` now show commit hash and build date
- Compile-time build script (`build.rs`) captures git revision and date

### Changed
- **CLI command rename** — `fleet` subcommand renamed to `aggregate` — all CLI commands, types, modules, and documentation updated
- **Mandatory baseline** — `--no-baseline` flag removed; baseline extraction is now required. Scans that cannot pull the target image exit with code 3 with remediation guidance. Use `--base-image` to override auto-resolution.
- **CLI flag rename** — `--baseline` flag renamed to `--target-image` (old flag still accepted as alias for compatibility).
- **Exit codes** — pull failures now exit with code 3 (previously the scan would continue with degraded output).
- **Scan progress output** redesigned as append-only streaming receipt.
- **Progress modes** simplified from three (rich/plain/flat) to two (pretty/flat).
- **Sub-step detail** moved behind `--verbose` flag.
- **Verbose mode** now works with both pretty and flat modes; flat mode respects `--verbose` (previously always showed sub-steps).
- **Tuned profiles** auto-enabled by default (previously required manual inclusion).
- **Schema version** bumped to 19. Tarballs from older schema versions are no longer loadable.

### Fixed
- **RPM performance** — massive speedup through batching: `dnf group info`, `rpm -qR`, and `--whatprovides` calls now batched into single invocations. Dramatically reduces scan time on hosts with many packages/groups.
- **Platform plumbing packages** hidden from refine view (installer-default packages no longer clutter migration scope).
- **User refinement operations** preserved after anaconda reclassification.
- **Config content truncation** removed (previously capped at 500 characters).
- **InstalledGroup members** filtered to installed-only packages (previously included uninstalled metadata).
- **Triage count badge** removed from UI (was noisy, not useful).
- **Subuid badge** removed from user cards.

### Removed
- **`--no-baseline` flag** — baseline is now mandatory.
- **`--progress rich` and `--progress plain` modes** (use `--progress pretty`).

### Known Issues
- RHEL-subscribed builds (`--preserve-subscription`) do not work when inspectah runs on non-RHEL hosts. The subscription material is host-specific and cannot be transferred across distributions.

## [0.8.5-beta.2] - 2026-06-05

### Added
- Unified include-default model for all 25 toggleable item types
- Locked items with reason badges in both web UI and TUI
- Shell completions auto-generated via clap_complete
- Experimental TUI mentioned in README
- CHANGELOG.md

### Changed
- Include flag is now authoritative for render overrides
- Fleet prevalence gate removed (now handled by aggregate merge)
- Single-host normalization moved to collectors
- Fleet handlers consume stored include values directly

### Fixed
- Validate `--ack-sensitive` before scanning instead of after full scan
- Progress display race condition causing duplicate spinner lines
- Triage diagram text clipping in expanded detail panels
- Triage diagram icon/text overlap in fleet category nodes
- Correct COPR username in README and docs
- Add missing kickstart file to README output tree
- Repair broken Getting Started link in docs
- MongoDB URL redaction preserves connection string structure
- Symlink resolution during /etc ownership classification
- FlatpakApp missing fleet field in test initializers

## [0.8.5-beta.1] - 2026-06-02

### Added
- PasswordHash pattern for secret detection
- PEM full-block matching for certificate detection
- False-positive value filtering for NSS/PAM tokens
- Comment-line filtering to pattern matching
- Documentation landing page for GitHub Pages site
- Experimental TUI for refine mentioned in README

### Changed
- Removed Homebrew install section from README (Rust CLI only)
- README rewritten for Rust CLI
- Removed shipped and Go-era specs and plans

### Fixed
- Clear fleet redaction_state properly
- Comment-line secret detection accuracy
- ExportDialog and App.routing test failures
- Serialized env-var tests to eliminate flaky race
- Null-safety and double-toggle bugs in ContextItem
- Test fixtures that produced noop operations

### Removed
- Pre-promotion compatibility shims

## [0.8.5-alpha.1] - 2026-06-01

### Added
- Project reference extraction system for cross-section analysis
- Network and storage reference extractors
- Container and kernel/boot reference extractors
- Service reference extractor
- Version change reference extractor
- Include field to RefinedTunedSelection
- Projection types module for reference-based refinement

## [0.8.4-alpha.1] - 2026-05-30

### Changed
- RPM-based dependency classification is now the primary path (replaced DNF-based resolution)

### Fixed
- Massive performance improvement: baseline filter now runs before DNF dependency resolution (reducing analysis time from 711 seconds to seconds)

## [0.8.3-alpha.2] - 2026-05-29

### Added
- Timing instrumentation to RPM inspector phases

### Fixed
- Baseline filter ordering bug - packages now filtered before DNF dependency resolution
- Build output now streams in real time instead of buffering

## [0.8.3-alpha.1] - 2026-05-29

### Added
- `inspectah build` command for building bootable container images
- `--preserve-subscription` flag to capture RHSM subscription material
- `--ack-sensitive` flag (renamed from `--acknowledge-sensitive`)
- Subscription fields to snapshot schema (v18)
- SubscriptionFile, SubscriptionSection, and EntitlementPair types
- SubscriptionInspector for RHSM material collection
- Integration tests for preserve-subscription feature
- Comprehensive documentation site using Jekyll and just-the-docs theme
- Diataxis-structured documentation (tutorials, how-to guides, reference, explanations)
- Six D3 diagrams embedded in documentation
- First-migration tutorial
- Contributing documentation
- CLI reference from help output
- Getting started tutorial
- Build and subscription documentation

### Changed
- Reframed project as distro-neutral FOSS tool
- Subscription files staged in tarball output
- Documentation moved to GitHub Pages

### Fixed
- Hardlink rejection in tarball extractor (security)
- Full symlink chain resolution in subscription inspector
- Fail fast when `--keep-context` target is non-empty
- Deterministic ambient/fallback proof tests
- Diagram centering on fullscreen enter/exit

## [0.8.2-alpha.2] - 2026-05-26

### Changed
- Variant file tree removed from tarball output
- Empty env files skipped in tarball output
- Schema placeholder file removed from tarball
- Non-universal divergent items demoted to informational

### Fixed
- User-toggled packages bypass leaf filter
- Leaf filter skipped for fleet snapshots in Containerfile
- Export dialog warning updated for promoted sections
- Version change display swapped to host → base
- Third-party repos use warning color
- Empty non-toggleable repos hidden from RepoBar
- `@commandline` repo shows 'not included' in RepoBar

## [0.8.2-alpha.1] - 2026-05-26

### Added
- System Tuning section (merged sysctls and tuned)
- Triage bucket type system
- `--verbose` and `--quiet` flags to scan command
- Consistent section headings to all content panes
- Top-level checkbox to UserCard header

### Changed
- Sysctls and tuned merged into unified System Tuning section
- Section promotion complete (all phases)
- Triage classification system implemented
- DECISION/CONTEXT_SECTIONS renamed to REVIEW/REFERENCE
- Default to strict intersection for package include in fleet

### Fixed
- Prevalence badge contrast improved
- Stock default tuned profiles suppressed from Containerfile
- Intersection default applied to all section types
- Cross-section state bleed and search collisions prevented
- Tuned profile include and prevalence in fleet view
- Projected include used for decision section toggles
- Fleet banner contrast for dark theme
- Expand chevron hidden for empty/whitespace-only detail
- `@commandline` repo made non-toggleable with friendly label
- Entire row clickable for expand/collapse
- Config noise - system-generated files filtered from unowned file detection
- RPM-owned file filtering uses sentinel format

## [0.8.1-alpha.2] - 2026-05-24

### Added
- Unified package/repository management across single and fleet views
- Accessibility contract: ARIA live regions, grid headers, focus management
- Fleet conflict popover and excluded zone states
- Containerfile change highlights with auto-scroll
- Reduced motion support for animations
- OS theme auto-detection with manual override
- Hostname popover to fleet StatsBar

### Changed
- Multi-line format for systemctl enable/disable/mask
- Leaf-package filter applied to fleet snapshots in Containerfile

### Fixed
- Focus handoff and keyboard navigation
- Attention badges removed from fleet item rows
- Banner text uses neutral dark color
- Variant selection decoupled from auto-review
- Info attention badge text contrast
- Main container stretches full viewport width
- Fleet content fills full viewport width
- Variant view renders inline below item row
- Search result selection highlight softened

## [0.8.0-alpha.4] - 2026-05-19

### Added
- Service intent inference with typed contract
- Service context subsections in UI
- Service omissions and advisories surfacing
- Owning package resolution during service collection
- Display implementations for service types
- `inspectah fleet` and `fleet init` commands
- Fleet aggregate Phase 1 functionality
- SSH tunnel hint when starting refine server
- Refine command shown after scan completes

### Changed
- Centralized service omission decisions
- Strict service deserialization enforced
- Masked service distinction in data model
- Preset unknown handling improved

### Fixed
- Omitted-row duplication and DOM identity conflicts
- Omission comments emitted correctly
- Duplicate package handling
- Baseline comparison uses plain package names
- Owning package guard rejects spaced output
- Service package truth helpers shared

## [0.7.0-go-final] - 2026-06-02

Final release of the Go implementation before the Rust rewrite.

---

[Unreleased]: https://github.com/marrusl/inspectah/compare/v0.8.7-beta.1...HEAD
[0.8.7-beta.1]: https://github.com/marrusl/inspectah/compare/v0.8.6-beta.5...v0.8.7-beta.1
[0.8.6-beta.5]: https://github.com/marrusl/inspectah/compare/v0.8.6-beta.4...v0.8.6-beta.5
[0.8.6-beta.4]: https://github.com/marrusl/inspectah/compare/v0.8.5-beta.2...v0.8.6-beta.4
[0.8.5-beta.2]: https://github.com/marrusl/inspectah/compare/v0.8.5-beta.1...v0.8.5-beta.2
[0.8.5-beta.1]: https://github.com/marrusl/inspectah/compare/v0.8.5-alpha.1...v0.8.5-beta.1
[0.8.5-alpha.1]: https://github.com/marrusl/inspectah/compare/v0.8.4-alpha.1...v0.8.5-alpha.1
[0.8.4-alpha.1]: https://github.com/marrusl/inspectah/compare/v0.8.3-alpha.2...v0.8.4-alpha.1
[0.8.3-alpha.2]: https://github.com/marrusl/inspectah/compare/v0.8.3-alpha.1...v0.8.3-alpha.2
[0.8.3-alpha.1]: https://github.com/marrusl/inspectah/compare/v0.8.2-alpha.2...v0.8.3-alpha.1
[0.8.2-alpha.2]: https://github.com/marrusl/inspectah/compare/v0.8.2-alpha.1...v0.8.2-alpha.2
[0.8.2-alpha.1]: https://github.com/marrusl/inspectah/compare/v0.8.1-alpha.2...v0.8.2-alpha.1
[0.8.1-alpha.2]: https://github.com/marrusl/inspectah/compare/v0.8.0-alpha.4...v0.8.1-alpha.2
[0.8.0-alpha.4]: https://github.com/marrusl/inspectah/compare/v0.7.0-go-final...v0.8.0-alpha.4
[0.7.0-go-final]: https://github.com/marrusl/inspectah/releases/tag/v0.7.0-go-final
