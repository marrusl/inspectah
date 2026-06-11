# Group-Aware Rendering & Refine UI — Pre-Spec

## Status

Pre-spec. Ready for brainstorm and design in a separate session.

## Origin

Split from the Anaconda gap classifier spec (R5) after review panel
found the group rendering and refine UI features needed their own
design pass. The classifier spec ships the four-tier classification
model and group-install collection (`installed_groups` snapshot field).
This spec picks up where that left off.

## What We Have After the Classifier Ships

- `installed_groups: Option<Vec<InstalledGroup>>` on the RPM snapshot
  section, populated by `dnf group list --installed` + `dnf group info`
- Package-to-group membership mapping available at rendering and
  display time
- Packages classified normally (Baseline/Site/Investigate) regardless
  of group membership — group is not a classification signal

## What This Spec Needs to Design

### Containerfile Rendering

Render group-member packages as `dnf group install "Group Name"`
instead of individual `dnf install` lines. Key design questions from
the review panel:

1. **Partial groups.** If a user excludes some members of a group in
   refine, `dnf group install` will reinstall them (it replays comps
   metadata). Options: `dnf group install --excludepkgs=...`, fall
   back to individual `dnf install`, or require all-or-nothing group
   toggle.
2. **Overlapping groups.** A package may belong to multiple groups.
   How to handle: first-group-wins, deduplicate, or render the package
   under the group with the most members retained?
3. **Optional members.** `dnf group info` distinguishes mandatory,
   default, and optional packages. Which members count?

### Refine UI (Web)

Group rows replace member packages in the list. Design direction
from brainstorming:

1. **Group row (collapsed):** chevron, group name (semibold), "12
   packages" suffix, single include/exclude toggle. Sorted
   alphabetically alongside individual packages.
2. **Expanded view:** Direct member packages indented (name +
   version). No individual toggles — group is the unit of decision.
3. **Ungroup action:** Icon button dissolves the group into individual
   package rows with their own toggles. One-way door. Containerfile
   switches from `dnf group install` to individual `dnf install`.
4. **Search:** "podman" surfaces the Container Management group with
   "contains: podman" subtitle.
5. **Summary line:** "4 groups, 47 individual packages" at top.

### Open Design Questions from R3 Review

These were the blockers that prompted the split:

- **Homogeneous state rule:** What if hidden members have different
  include/locked states (e.g., one is platform plumbing, others are
  site)? Should groups only form when all members share the same
  state? Auto-ungroup on divergence?
- **Session/view contract:** Where does grouped/ungrouped state live?
  How does it persist across `/api/view`, preview, export, refresh,
  and undo? The `installed_groups` field alone doesn't cover this.
- **Keyboard/a11y:** Disclosure, toggle, ungroup, search-driven
  expansion, and toast feedback all need keyboard/focus/ARIA contracts.
- **Partial-member rendering fidelity:** Does `dnf group install`
  faithfully reproduce the user's retained subset, or does it replay
  the full comps definition?

## Dependencies

- Anaconda gap classifier spec (must ship first — provides the
  `installed_groups` snapshot data)
- Fern (interaction design lead for this spec)
- Tang (session/view contract, typed state model)
- Kit (web UI implementation)
- Thorn (behavioral testing)

## How to Start

Brainstorm in a fresh session:

> "Let's brainstorm the group-aware rendering spec for inspectah.
> The pre-spec is at `process-docs/specs/proposed/2026-06-11-group-rendering-pre-spec.md`."
