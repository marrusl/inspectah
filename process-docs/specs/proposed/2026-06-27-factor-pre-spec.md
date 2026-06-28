# Factor — Pre-Spec

## Metadata

| Field        | Value                                                |
|--------------|------------------------------------------------------|
| Date         | 2026-06-27                                           |
| Status       | Pre-spec                                             |
| Contributors | Mark Russell (PM), image mode specialist, UX lead, competitive analyst |
| Renamed from | `inspectah architect` (2026-06-27)                   |

## Naming

"Factor" replaces "architect." The name describes what the tool does —
decompose into constituent parts — not a role it assumes. The
mathematical metaphor is precise: factoring IS decomposition along
natural boundaries, and the factors can be recombined. Works cleanly as
a CLI verb (`inspectah factor`), in documentation, in conference talks,
and in help strings.

---

## 1. Problem Statement

A team migrating a heterogeneous RHEL fleet to image mode faces an
architecture question they currently answer by guesswork: how many
images do we need, what goes in each layer, and how do we structure
Containerfiles that actually reflect fleet reality?

Today this is manual work. Someone exports a fleet's package and config
data, eyeballs it in a spreadsheet, and draws layer boundaries based on
intuition. This process is slow, error-prone, and disconnected from the
actual fleet data.

Factor solves this by taking curated fleet data and decomposing it into
role-separated output archives with an auto-proposed layer structure.
The proposal is prevalence-driven — packages on 340/400 machines are
base, packages on 12 are extensions — and users refine it rather than
building from scratch.

The strategic frame: factor turns inspectah from a fleet analysis tool
into a fleet standardization engine. It doesn't just tell you what your
fleet looks like; it tells you what your image architecture should be.

## 2. Input Model

Factor does NOT consume a single machine tarball.

Factor consumes a **directory of refined aggregates** — the output of
`inspectah refine`. Each aggregate is a fleet-representative artifact
produced by the inspect-many → aggregate → refine (interactive triage)
pipeline. By the time data reaches factor, it has already been:

- Collected across multiple machines
- Aggregated into fleet-level views
- Triaged through refine (noise filtered, decisions made)

This means factor works with fleet-representative data, not
single-system snapshots. Every artifact carries fleet prevalence
metadata (how many machines, what variance). This is a fundamental
design constraint: factor's proposals are only as good as the refine
output that feeds them.

**Implication:** Factor never needs to handle raw machine tarballs,
single-host edge cases, or unrefined data. The refine stage is the
quality gate.

## 3. Design Principles

These principles are binding on the detailed spec. They emerged from
converged agreement across PM, image mode specialist, and UX lead.

### 3.1 Propose, then refine

Factor proposes an auto-decomposition. Users refine it. Never a blank
canvas. The proposal must be good enough that a quick-mode user can
accept and export in under 10 minutes. Power users get full interactive
refinement.

### 3.2 Prevalence drives structure

Every factoring decision is grounded in fleet data. A package on 398/400
machines is base layer. A package on 12 is an extension or role. This is
not configurable in the abstract — it flows from the data.

### 3.3 Roles, not layers

The UI vocabulary is "role" (web-server, hardened-base, monitoring), not
"layer" (Layer 1, Layer 3). Layers are an implementation detail of
container image builds. Roles are meaningful to the humans who own and
maintain them.

### 3.4 Tied changes follow automatically

Moving a package between roles should automatically move its associated
configs, services, firewall rules, and other dependent artifacts. These
are "tied changes." The user sees a preview of what will follow before
committing the move.

### 3.5 Archives first, Containerfiles derived

The primary output is a set of self-contained role archives. The
Containerfile is a derived artifact rendered from the archive set, not
the other way around. This separation unlocks team ownership,
composable assembly, and per-role drift detection.

### 3.6 Bootc-aware from the ground up

Factor understands the bootc filesystem model. It knows `/usr` is
immutable, `/etc` uses 3-way merge, and `/var` is persistent state. This
knowledge shapes where configs land, how variance is surfaced, and what
structural warnings get emitted.

### 3.7 Self-scoring proposals

Factor scores its own decomposition: "87% clear affinity, 4 artifacts
ambiguous — review these." Users know immediately where to focus
attention and where they can trust the auto-proposal.

## 4. Proposed Workflow

### Phase 1: Load and auto-decompose

User points factor at a directory of refined aggregates. Factor reads
fleet prevalence data, artifact types (packages, configs, services,
quadlets, firewall rules, sysctls, users/groups), and tied-change
relationships. It proposes a decomposition into base + N roles using
prevalence thresholds and artifact affinity.

### Phase 2: Review the proposal

Factor presents the proposed decomposition in the canvas UI: a roles ×
artifact types matrix. Each artifact carries its fleet context
(prevalence, variance). The proposal includes a confidence score and
flags ambiguous artifacts for human review.

### Phase 3: Refine interactively

Two modes:

- **Quick mode:** Review flagged ambiguities, accept the rest, export.
  Target: under 10 minutes for a well-curated fleet.
- **Full refinement:** Drag artifacts between roles, split/merge roles,
  resolve tied-change decisions, handle outlier packages. Full
  interactive canvas with ghost previews and connector lines.

### Phase 4: Export

Generate the output archive set. Optionally render Containerfiles from
the archives. Preview the output tree before committing.

### Phase 5: Coverage report

Factor reports what percentage of the fleet the proposed architecture
covers: "5-role stack covers 94% of fleet, remaining 6% are long-tail
across 3-8 machines." This tells the user whether their architecture is
good enough or needs more roles.

## 5. Output Model

### Primary output: role archives

Factor produces a set of directories or tarballs:

```
output/
  base/
    packages.list
    configs/
    services/
    ...
  web-server/
    packages.list
    configs/
    services/
    ...
  hardened/
    packages.list
    configs/
    firewall/
    sysctls/
    ...
```

Each role is a self-contained archive. Artifact types within each role:
packages, configs, services, quadlets, firewall rules, sysctls,
users/groups. The archive set is the source of truth.

### Derived output: Containerfiles

`inspectah factor --containerfile` renders a multi-stage or
multi-Containerfile build from the archive set. Possible structures:

- Single `Containerfile` with labeled stages
- Multiple files: `Containerfile.base`, `Containerfile.web-server`, etc.

Users must be able to set the `FROM` line for each role — factor cannot
know image registry paths or tags in advance.

### What the separation unlocks

- **Team ownership:** Security team owns `hardened/`, app team owns
  `web-server/`. Each role can have its own review and release cycle.
- **Composable assembly:** Pick base + 3 of 8 roles for a given
  deployment target. The archive set is a menu, not a monolith.
- **Per-role drift detection:** driftify can monitor drift at role
  granularity, not just whole-image.
- **Replayable patterns:** Save a decomposition, reapply next quarter,
  see role-level drift over time.

## 6. UX Vision

### Canvas, not tabs

The primary interaction surface is a roles × artifact types matrix. All
roles visible simultaneously. Each cell shows the artifacts assigned to
that role in that category. This gives spatial awareness of the full
decomposition that tabs or accordions destroy.

### Prevalence is always visible

Every artifact carries fleet context. Prevalence bars show how many
machines have this artifact. Variance indicators flag configs that differ
across hosts. Nothing is presented without its fleet dimension.

### Tied changes: ghost preview + commit

When a user drags an artifact to a different role, its tied artifacts
appear as ghosts with connector lines in the destination role — visible
but not yet committed. The user reviews the cascade, then commits or
cancels. This makes tied changes discoverable without making them
surprising.

### Tied changes with fleet dimension

Not all ties are equal. Fleet-uniform ties (httpd config identical on all
398 machines that have httpd) follow silently. High-variance ties (config
differs across hosts) surface a decision point: "This config varies
across 12 machines — review before moving?"

### Outlier surfacing

Factor flags artifacts with unusual prevalence patterns: 12 machines
have a package nobody else does. Is that intentional specialization or
drift? Factor surfaces the question; the user decides.

### Export preview

Before committing an export, users see a tree view of each output
directory. This is the "you are about to produce this" confirmation —
catches errors before they become bad archives.

### Platform

Factor stays web-based. Fleet data visualization (prevalence bars,
matrix canvas, ghost previews, connector lines) needs the rendering
capability of a browser canvas. The TUI handles simpler interactions.

## 7. Bootc Integration

Factor should understand the bootc filesystem model and use it to make
smarter decomposition decisions.

### Filesystem awareness

| Path    | bootc behavior          | Factor implication                          |
|---------|-------------------------|---------------------------------------------|
| `/usr`  | Immutable in image      | Default target for packages and base configs |
| `/etc`  | 3-way merge at boot     | Variance is the key signal here             |
| `/var`  | Persistent, not in image| Divergent content = persistent state         |

### Config variance as signal

Config variance is the most actionable signal for factoring:

- **398/400 identical:** Default config. Emit to `/usr/etc/` (image
  baked). No parameterization needed.
- **12 differ:** Consider parameterizing. Surface as a decision point:
  hardcode vs. template vs. runtime config.
- **All different:** Runtime state. Probably belongs in `/var` or is
  managed by a config management tool. Flag, don't image-bake.

### Layer ordering for cache optimization

When rendering Containerfiles, factor should order layers for build cache
efficiency:

1. Base packages (changes least often)
2. System configs
3. Application packages
4. Application configs
5. Services and enablement (changes most often)

### `/var` handling

Uniform `/var` content across the fleet might be image-worthy (e.g.,
directory structure). Divergent `/var` content is persistent state — emit
`tmpfiles.d` entries to create directory structures, but don't bake data.

### Pre-build structural linting

Factor should lint the proposed output before export:

- No raw `/var` writes in the Containerfile
- Base role is bootc-compatible (no conflicts with immutable `/usr`)
- Config placement respects the `/usr/etc` vs `/etc` model
- Services are enabled via `systemctl preset`, not manual symlinks

## 8. Ecosystem Fit

Factor occupies a specific position in the inspectah pipeline:

```
inspect (many machines)
  → aggregate (fleet-level views)
    → refine (interactive triage, curate fleet truth)
      → factor (decompose into role architecture)
        → BIB (build images from Containerfiles)
          → driftify (monitor drift against role baselines)
```

### Relationship to refine

Refine curates what's true about the fleet. Factor decides how to
structure it. Refine answers "what do we have?" Factor answers "how
should we build it?" The boundary is clean: refine's output is factor's
input, and refine never makes architectural decisions.

### Relationship to driftify

Factor's role archives become driftify's baseline targets. Instead of
monitoring drift against a monolithic image, driftify can track drift at
role granularity: "The web-server role has drifted on 3 config files
since last quarter."

### Fleet-aware templates

A decomposition pattern (base + web + hardened + monitoring) can be
saved and reapplied. Point it at next quarter's refined fleet data and
see what changed at the role level. This makes factor a repeatable
quarterly standardization step, not a one-time migration exercise.

### Coverage as a metric

Factor's coverage report ("94% of fleet covered by 5 roles") becomes a
trackable metric. Quarter-over-quarter improvement in coverage means the
image architecture is converging on fleet reality.

## 9. Competitive Positioning

No tool in the container migration or fleet management space does
multi-artifact layer decomposition from fleet-representative data.

Existing migration tools (Konveyor, containerization advisors) work
with single applications or single hosts. They produce a single
Containerfile for a single workload. They don't factor.

Factor's competitive moats:

1. **Fleet input:** Working from aggregated, refined fleet data — not
   single machines — makes factor a standardization engine. "Look at
   400 machines and tell me what my image architecture should be."
2. **Role-separated output:** Independently ownable, versionable,
   composable archive sets. No competitor produces this.
3. **Prevalence-driven proposals:** Auto-decomposition grounded in
   actual fleet data, not templates or guesswork.
4. **Bootc-native:** Understanding the immutable filesystem model means
   factor's output is structurally correct for image mode, not just
   syntactically valid.
5. **Tied changes:** Automatic dependency tracking across artifact types.
   Move a package, its world follows.

The positioning line: "The only tool that can look at 400 machines and
tell you what your image architecture should be."

## 10. Open Questions

These must be resolved in the detailed spec:

1. **Prevalence thresholds.** What percentage constitutes "base" vs.
   "extension"? Is this configurable, auto-detected, or both? What
   about the middle ground (60% prevalence)?
2. **Tied-change discovery.** How does factor discover ties? Package →
   config ties are detectable (rpm owns the file). Service → package
   ties are detectable (systemd unit names). What about cross-artifact
   ties that aren't rpm-tracked?
3. **Role naming.** Does factor auto-name roles (from dominant package
   groups?) or require user naming? What's the default?
4. **Merge semantics.** When two roles are merged, how are conflicts
   resolved? What about tied changes that span both?
5. **FROM line management.** How does the user specify the base image
   for each role's Containerfile? Interactive prompt? Config file?
   CLI flags?
6. **Archive format.** Tarballs? Directories? Both? What metadata
   accompanies each archive (provenance, fleet coverage, timestamp)?
7. **Incremental re-factor.** Can a user load a previous decomposition
   and re-factor with updated fleet data? What's preserved, what's
   recomputed?
8. **Role dependency ordering.** If role B depends on role A (e.g.,
   web-server needs base), how is this expressed and enforced?
9. **Outlier policy.** What happens to artifacts that don't fit any
   role? Separate "unassigned" bucket? Force assignment? Exclude?
10. **TUI scope.** What subset of factor's workflow is available in the
    TUI vs. web-only? Is quick mode TUI-feasible?

## 11. Glossary

| Term             | Definition                                                              |
|------------------|-------------------------------------------------------------------------|
| **Factor**       | The act of decomposing fleet data into role-separated output archives.  |
| **Role**         | A named, purpose-driven grouping of artifacts (e.g., "web-server", "hardened-base"). Replaces "layer" in UI vocabulary. |
| **Tied change**  | An artifact that should follow another when moved between roles (e.g., httpd's config files follow httpd). |
| **Archive**      | A self-contained output directory or tarball for a single role, containing all artifact types assigned to that role. |
| **Prevalence**   | The fraction of fleet machines that have a given artifact. The primary signal for auto-decomposition. |
| **Variance**     | The degree to which an artifact differs across fleet machines. High-variance configs need different handling than uniform ones. |
| **Coverage**     | The percentage of fleet machines fully represented by the proposed role architecture. |
| **Refined aggregate** | The output of `inspectah refine` — fleet-representative data that has been triaged and curated. Factor's input. |
| **Ghost preview**| A UI pattern where tied artifacts appear as translucent previews in the destination role before the user commits a move. |
| **Quick mode**   | A streamlined factor workflow: review flagged ambiguities, accept auto-proposal, export. Target: under 10 minutes. |
