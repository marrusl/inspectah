# Release Notes: v0.8.6-beta.5

## What's new in v0.8.6-beta.5

This release focuses on aggregate (multi-host) refine polish and single-host correctness fixes. beta.4 was tagged but never released on GitHub, so this release includes all changes since beta.3.

### Aggregate refine improvements

The aggregate refine view gets smarter defaults and cleaner UI:

- **Prevalence-based default selections** -- packages and config files in aggregate mode now correctly default to excluded when not present on 100% of hosts. Previously all items defaulted to selected regardless of prevalence, requiring manual deselection of partial-prevalence items.
- **Prevalence badge toggle** -- clicking any prevalence badge toggles between fraction (45/50) and flat percentage (90%) display. Toggle is global across all badges.
- **Sidebar include/total counts** -- decision sections in aggregate sidebar now show "N included / M total" in their badges, matching the per-section counts available in single-host mode.
- **Stats bar simplified** -- removed "N need review" / "All reviewed" labels from the aggregate stats bar header. Host count and total items remain.
- **Sidebar simplified** -- removed "N/M confirmed" ack progress labels from sidebar nav items. Item count badges remain.
- **Keyboard navigation** -- number keys (1-9) now jump to the correct aggregate sections. Previously they sent single-host section IDs, causing partial navigation failures.

### Single-host refine fixes

Several correctness fixes for the single-host refine workflow:

- **RHEL repo classification** -- RHEL-style long repo IDs (e.g. `rhel-9-for-x86_64-baseos-rpms`) are now correctly classified as distro repos. Previously, only short CentOS-style IDs were recognized, causing RHEL base repos to appear as toggleable third-party repos.
- **Ungrouped packages** -- ungrouping a DNF package group now correctly surfaces individual members in the package list and Containerfile. Previously, non-leaf group members were filtered out by the leaf dependency filter after ungrouping.
- **COPR self-exclusion** -- inspectah's own COPR repo definition is now auto-excluded from config files and repo file output. The migration tool should never carry its own repo into the target image.
- **Container row click target** -- entire quadlet row is now clickable to expand/collapse the unit file content, not just the small chevron indicator.
- **False unredact offer** -- User Artifact Preview no longer shows the redact/reveal banner when the displayed content has no redacted material. ContainerfilePanel "Reveal hashes" button also hidden when the Containerfile has no crypt(3) hashes.
- **Group toggle removed** -- removed the non-functional group-level toggle switch. Groups are managed via the ungroup button or per-member actions.

### Parity and polish

- **Dark mode badge contrast** -- prevalence badges now have proper dark-mode color overrides instead of hardcoded light-mode colors.

### Also included (from beta.4, not previously released)

- **Subscription cert expiry display** -- scan output shows entitlement certificate expiration with warnings at <7 days.
- **Anaconda gap classifier** -- installer-default packages auto-excluded from migration scope.
- **Context section overhaul** -- version changes table, networking/kernel subsections, service state display, quadlet content view.
- **Mandatory baseline** -- `--no-baseline` flag removed; baseline extraction is now required.
- **CLI rename** -- `fleet` subcommand renamed to `aggregate`.
- **Pull failure classification** -- five error categories with tailored remediation guidance.
- **RPM performance** -- massive speedup through batched `dnf group info`, `rpm -qR`, and `--whatprovides` calls.
- **Progress output** -- redesigned as append-only streaming receipt with simplified modes.
- **Build metadata** -- `inspectah version` now shows commit hash and build date.

### Binaries

Pre-built binaries for 3 platforms:
- `inspectah-darwin-arm64` -- macOS on Apple Silicon
- `inspectah-linux-arm64-bin` -- Linux on ARM64 (static musl binary)
- `inspectah-linux-amd64` -- Linux on x86_64 (static musl binary)

**Full changelog:** https://github.com/marrusl/inspectah/compare/v0.8.6-beta.3...v0.8.6-beta.5
