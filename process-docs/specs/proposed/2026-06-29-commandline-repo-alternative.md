# @commandline Package Repo Alternative

> **Status:** Pre-spec
> **Priority:** Low — corner case enhancement
> **Related:** RPM repo-less detection, refine UI package details

## Problem

Packages installed via `rpm -i` (`@commandline`) are flagged as repo-less
and require the user to upload the RPM manually. In some cases, the same
package (same name, possibly same version) is available in an enabled
repository. The user currently has no visibility into this.

## Observation

This is a corner case. If someone ran `rpm -i`, they likely had a reason:
a patched build, a specific version, a package not yet in repos. The
current behavior (flag as repo-less, require RPM upload) is correct as the
default. This enhancement adds information, not a new default.

## Proposed Enhancement

### Backend

When processing `@commandline` packages in the repo-less flow, also query
`dnf repoquery --available <name>` to check if the package exists in an
enabled repo. If found, store:

- `repo_alternative`: the repo ID that provides it (e.g., `rhel-10-for-aarch64-baseos-rpms`)
- `repo_alternative_version`: the version available in that repo

These are informational fields — they do NOT change the package's
repo-less status or the default behavior.

### Frontend (Refine UI)

In the package detail pane for `@commandline` repo-less packages with a
repo alternative:

- Show an info line: "Also available in **rhel-10-baseos-rpms** as version **X.Y.Z-R.el10**"
- If the repo version matches the installed version exactly: "Same version available in **rhel-10-baseos-rpms** — consider using repo source instead"
- Offer a toggle: "Use repo version" which switches the package from
  upload-required to repo-sourced (changes the Containerfile from
  `MANUAL` annotation to a normal `dnf install` line)

### Containerfile Impact

When the user opts to use the repo version:
- Package renders as a normal `dnf install` line (active, not commented)
- The `MANUAL` annotation and upload requirement are removed
- If the repo version differs from the installed version, add a comment
  noting the version difference

## Open Questions

1. Should we batch the `dnf repoquery --available` call for all
   `@commandline` packages, or is per-package acceptable? (Batch is
   better for performance, but the number of @commandline packages is
   typically very small.)
2. Should this extend to other repo-less packages (truly missing from
   repos), or only `@commandline`? A package from a disabled/removed
   repo that happens to exist in another enabled repo is a different
   situation.
3. Version comparison: if the repo version is newer, should we
   recommend upgrading? Or is "same name, different version" a warning?

## Scope

- Small backend change (extend repo-less scan)
- Small frontend change (detail pane info line + toggle)
- No new CLI flags or scan behavior changes
- Does NOT change the default: @commandline packages remain repo-less
  unless the user explicitly opts to use the repo version
