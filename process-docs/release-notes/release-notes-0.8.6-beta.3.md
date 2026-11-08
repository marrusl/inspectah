# Release Notes: v0.8.6-beta.3

## What's new in v0.8.6-beta.3

### Mandatory baseline + pull failure classification

The scan now **requires a baseline image** — the `--no-baseline` flag has been removed. When inspectah cannot pull the target image, it exits with code 3 and provides remediation guidance based on the failure type. Five error categories are recognized (registry unreachable, auth required, image not found, TLS/cert error, unknown), each with specific steps including disconnected-environment workarounds via `podman save`/`podman load`.

The `--baseline` flag has been renamed to `--target-image` for clarity (the old flag still works as an alias).

### Anaconda gap classifier

Packages installed by the RHEL installer (Anaconda) are now classified as platform plumbing and **auto-excluded from migration scope**. This dramatically reduces migration noise — installer-default packages no longer clutter the refine view or Containerfile output. User refinement operations are preserved across reclassification.

### Refine UX overhaul

Major improvements to the refine web interface:

- **Version changes table** — context section now renders version changes as a clean grouped table with upgrade/downgrade categories and proper EVR formatting
- **Networking subsections** — networking section reorganized into clear subsections: Connections, Firewall, Routes & Rules, DNS & Hosts, Proxy
- **Kernel & boot subsections** — kernel & boot section split into Customizations vs Defaults/Context
- **Package group dependencies** — group member lists now indicate which packages are already in the base image vs newly added. Summary labels distinguish "3 new members" from "5 members (3 new, 2 in base)". Long member lists use progressive disclosure instead of fixed truncation.
- **Service states** — services now display both current (host) state and preset default side-by-side
- **Quadlet content view** — quadlet unit file content is now viewable inline via expand/collapse in the container section
- **Config content** — 500-character truncation removed; full content is now visible
- **UI cleanup** — removed noisy triage count badges and subuid badges from user cards

### RPM performance improvements

Scan performance significantly improved through batching:
- `dnf group info` calls batched across all groups
- `rpm -qR` dependency queries batched into single invocation
- `--whatprovides` lookups batched globally

These changes dramatically reduce scan time on hosts with many packages and groups.

### Progress output improvements

Scan progress output redesigned as an append-only streaming receipt. Progress modes simplified from three (rich/plain/flat) to two (pretty/flat). Sub-step detail moved behind `--verbose` flag, which now works with both modes.

### Other improvements

- Tuned profiles detected during scan are now auto-enabled by default
- Sidebar section counts correctly sum subsection items for subsection-only sections
- Accessibility: ContextList subsections use semantic headings and ARIA region landmarks
- Schema version bumped to 19

### Binaries

Pre-built binaries for 3 platforms:
- `inspectah-darwin-arm64` — macOS on Apple Silicon
- `inspectah-linux-arm64` — Linux on ARM64 (static musl binary)
- `inspectah-linux-amd64` — Linux on x86_64 (static musl binary)

**Full changelog:** https://github.com/marrusl/inspectah/compare/v0.8.6-beta.2...v0.8.6-beta.3
