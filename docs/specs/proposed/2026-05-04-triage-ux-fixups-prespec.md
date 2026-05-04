# Triage UX Fixups — Pre-Spec

**Status:** Pre-spec (pending brainstorm)
**Date:** 2026-05-04
**Context:** Mark's live testing feedback after the single-machine triage redesign + UX fixes landed on `go-port`. These must be addressed before moving to fleet verification (cutover plan item 1).

---

## Core Problem

The triage redesign shipped the right components (accordions, notification cards, display-only surfaces, dep drill-down) but the defaults, wording, and visual hierarchy don't match the product intent. Single-machine mode should assume inclusion — the user is migrating their system and expects everything carried forward unless they explicitly remove it. The current UI treats too many items as needing decisions when they should be green and included.

## Feedback by Section

### Packages

1. **"Needs decision" is wrong wording and wrong color.** Tier 2 items are included packages from known repos. They should be green (like tier 1), not amber/warning. The distinction between tier 1 (base image packages) and tier 2 (user-installed from repos) is real, but both are included by default. Need better tier labels — something like "Included" and "Base image" instead of "Needs decision" and "Auto-included."

2. **`fedora` repo should be non-disableable.** On Fedora, the `fedora` repo is equivalent to BaseOS on RHEL — it's the distro's core repo. The accordion should get `alwaysIncluded: true` treatment (no toggle, "always included" label). The heuristic needs to recognize `fedora` as a standard repo alongside `baseos`, `appstream`, etc.

3. **Version changes should be a separate section, not embedded in packages tier 1.** It should show ALL packages with version deltas (both leaf and auto-dependency packages), not just the ones that happen to land in tier 1. This is a reference view — "here's what changed between your host and the base image."

### Configuration

4. **All config files should default to included (green).** The current tier 2 "needs decision" treatment is wrong for single-machine mode. Users may want to un-check configs, but the default is inclusion. Cards should be green.

### Runtime

5. **`dnf-makecache.service` (and similar image-mode incompatible services) should default to EXCLUDED.** These are correctly flagged as tier 3, but the current treatment lets users acknowledge and include them. They should default to excluded since they can't work in image mode.

6. **Services-changed and cron jobs should be included by default.** The current excluded-by-default state is wrong. In single-machine mode, the user's runtime configuration should be carried forward. They can exclude items they don't want.

### Containers (and Non-RPM)

7. **Container section is confusing.** Unclear what's included, what's acknowledged, and what the undo button does. The undo button may be broken.

8. **Quadlets should be on top, defaulted to included (green).** Not "needs decision." Single-machine mode assumes the user wants their quadlet definitions in the image.

### System & Security

9. **Mount point acknowledgment is out of scope.** Fstab entries shouldn't require user acknowledgment for image building purposes. They're informational at best.

10. **Firewall rules should be included by default.**

11. **Kernel modules: only surface user-configured modules.** Most modules autoload by default and don't need user attention. Only modules that the user explicitly configured (not auto-loaded) should be surfaced as items needing awareness.

12. **Sysctls should be included by default (green).**

13. **Group all info-only sections together.** Display-only surfaces (network, fstab, running containers) should be visually separated from actionable items, perhaps in their own collapsible "Informational" group at the bottom of each section.

### Secrets

14. **Button consequences are unclear.** "Keep in image" — does this include the real secret, or is the file still redacted? The copy needs to make the consequence explicit: the file is redacted in the output regardless, but keeping it means the Containerfile will COPY the redacted placeholder.

### General / Visual

15. **Auto-included tier header not visually expandable.** In both dark and light mode, the tier 1 header doesn't look clickable/expandable. Needs a visual affordance (chevron, hover state, or similar).

16. **Light mode is too bright.** The overall brightness needs toning down.

17. **Dark mode theme toggle button is invisible.** Can't see the button to switch themes.

18. **Containerfile preview pane should be adjustable and hideable.** Users should be able to resize the preview pane width and collapse/hide it entirely.

## Thematic Summary

Most of the feedback falls into three themes:

**A. Default state is wrong.** Single-machine mode should assume inclusion. Tier 2 items across all sections should be green and included, not amber "needs decision." The exception is image-mode incompatible items (dnf-makecache, packagekit) which should default to excluded.

**B. Wording and visual hierarchy need revision.** Tier labels, button copy (especially secrets), and visual affordances (expandable headers, theme toggle) don't communicate the right intent.

**C. Information architecture needs refinement.** Version changes should be their own section. Info-only items should be grouped together. The containerfile preview needs layout controls.
