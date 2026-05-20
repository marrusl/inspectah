# v0.8.0-alpha.4

## Upgrade Notice

**Schema version 16.** Snapshots from earlier alphas must be
re-scanned — they will not load. Run `inspectah scan` again on your
target hosts to produce a compatible tarball.

## What's New

### Smarter service handling

inspectah no longer carries over every service state from the source
host. Services whose state matches the systemd preset default are
now recognized as stock configuration and suppressed — if RHEL
changes a default in a future release, your image picks up the new
default automatically instead of pinning the old one.

Only services where the operator made a deliberate choice (explicitly
enabled, disabled, or masked a service, or added a drop-in override)
appear in the Containerfile. Services owned by packages that won't
exist in the target image are omitted with a visible comment
explaining why.

The refine UI shows three new supplemental sections under Services:
**Omitted Services** (proven absent from the target image),
**Service Advisories** (emitted but with caveats — e.g., package
excluded or requires manual installation), and **Service Warnings**
(edge cases like linked or unrecognized unit states that need manual
review).

### User and group migration

inspectah now collects user and group data from the source host and
produces migration artifacts. Custom (non-system) users are surfaced
in the refine UI with per-account strategy control: skip the account
or include it via `useradd` in the Containerfile. Password handling
offers three choices: omit, preserve the existing hash, or prompt
for a new password. Output renders as kickstart fragments, blueprint
TOML, and Containerfile `RUN useradd` lines. Custom groups,
supplementary group memberships, sudoers rules, and SSH authorized
key references are also captured.

### Baseline comparison

When a base image is specified, inspectah now shows what changed
between the base image and the scanned host in the audit report and
readme output. The CLI displays pull progress with a live viewport
during base image extraction.

### Unified repo view

The refine UI's Packages section now groups packages by their source
repository. Each repo is a collapsible section with a header showing
package count. The split pane between the package list and detail
view is drag-to-resize. Global search auto-expands matching repo
groups and navigates directly to the matched package.

### Post-leaf quality improvements

Leaf package classification is now baseline-aware — packages present
in the base image are suppressed from the install list regardless of
their added/modified state on the host. The three-way service preset
contract (divergent / matched / unknown) surfaces honest confidence
levels. A new leaf dependency tree modal shows what each leaf package
pulls in. Version drift between host and base image is shown in a
dedicated Version Changes section.

## Breaking Changes

- Schema version 16: re-scan required for all hosts
- The `action` field is removed from service state changes in the
  snapshot JSON (replaced by typed `current_state` enum)
- Service `current_state` and `default_state` are now typed enums,
  not strings — tooling that reads snapshot JSON directly will need
  to handle `"enabled"` / `"disabled"` / `"masked"` for
  `current_state` and `"enable"` / `"disable"` or `null` for
  `default_state`
