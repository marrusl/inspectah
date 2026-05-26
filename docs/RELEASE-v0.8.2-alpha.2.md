# v0.8.2-alpha.2 Release Notes

**Date:** 2026-05-26
**Commits since v0.8.2-alpha.1:** 21

## Critical Fix

- **User-toggled packages now appear in Containerfile.** The leaf filter
  silently dropped non-leaf packages even when the user explicitly included
  them. User intent now overrides leaf status — if you check it on, it's
  treated as a leaf.

## Bug Fixes

- **Version change display order:** Swapped from `base → host` to
  `host → base` so arrows match labels (upgrade/downgrade).
- **Fleet triage:** Non-universal divergent items demoted to informational.
  Only items on every host with variants require review.
- **@commandline repo label:** Shows "not included" instead of misleading
  "always included." Hidden entirely when count is zero.
- **Third-party repo color:** Swapped from fill-weight yellow (unreadable
  on white) to text-weight warning color with proper contrast.
- **Export dialog warning:** Updated for section promotion — services,
  containers, sysctls, and tuned are now toggleable, not fixed.
- **Empty env files:** No longer emitted to tarball (ghost directories).
- **REVIEW/REFERENCE naming:** Frontend constants aligned with UI labels.

## Cleanup

- Removed placeholder `schema/snapshot.schema.json` from tarball.
- Removed redundant `fleet/variants/` file tree from tarball. Variant
  content lives in `snapshot.json` only; the refine UI reads from memory.
