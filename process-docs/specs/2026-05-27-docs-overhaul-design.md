# inspectah Documentation Overhaul — Design Spec

**Date:** 2026-05-27
**Author:** Mango (Technical Documentation Writer), with Kiwi orchestration
**Status:** Approved — 3-round review complete (Fern R2, Collins R2, Kit R3, Tang R3)

## 1. Goals

- **Trigger:** Rust branch promotion to main. The full docs cycle completes before merge — the repo launches with a complete, accurate documentation surface.
- **Quality bar:** External-facing. Docs must work for a sysadmin who has never heard of inspectah landing on the repo cold — conference attendees, Red Hat colleagues, potential contributors.
- **Framework:** Diataxis (tutorials, how-to guides, reference, explanation).
- **Deployment:** GitHub Pages site with Jekyll, hosted from `docs/`.

## 2. Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Go-era docs | Delete entirely | Git history preserves them; no archive clutter |
| `build` subcommand | Remove from user-facing flows | Not yet in Rust; keep `how-to/build-bootc-image.md` with "not yet available" banner but exclude from journey diagrams and getting-started funnel |
| D3 diagrams | Start fresh, unified visual language | Consistent identity across all 6 diagrams |
| Diagram audience | Both sysadmins and developers | Separate tiers: 4 user-facing, 2 developer-facing |
| Diagram interactivity | Rich interactive (click-to-expand, zoom/pan, tooltips, animated flow) | Match existing diagram quality bar |
| Diagram embedding | Inline iframe preview + click-to-fullscreen | Contextual preview without sacrificing interactivity |
| Diagram detail level | Deep click-to-expand; dev diagrams capped at crate/module/contract for v1 | Deeper type/trait/field expansion deferred until codebase stabilizes or generation tooling exists |
| Architect in user flows | Future-direction treatment (dashed/dimmed) | Not yet in Rust CLI; same principle as `build` — only shipped features in the primary journey |
| Output artifacts | Per-command contract boundaries | `scan`/`fleet aggregate` produce the broad artifact set; `refine` export has a narrower contract. Single reference page, matrix format |
| `docs/` boundary | Clean publish surface only | Internal engineering artifacts (plans, designs, backlog, etc.) move to project root |
| CLI reference source | Generated from `--markdown-help` | Rust CLI has built-in markdown generation; hand-written docs will drift |
| Redaction framing | "Sensitive data handling" | Not "trust signal" — "trust" has specific bootc/image-mode meaning (provenance, signatures, boot integrity) |
| Architect/layer framing | Build-time image composition | Not runtime layer boundaries — bootc deployments are image references, not managed strata |

## 3. Documentation Architecture

### 3.1 Publishing Boundary

`docs/` is the **exclusive GitHub Pages publish source**. No internal engineering artifacts live here. Internal content that currently resides in `docs/` moves to the project root:

**Moved out of `docs/` to project root:**
- `docs/plans/` → `plans/`
- `docs/designs/` → `designs/`
- `docs/backlog/` → `backlog/`
- `docs/specs/` → merged with existing root `specs/`
- `docs/ROADMAP.md` → `ROADMAP.md`
- `docs/nit-list.md` → `nit-list.md`
- `docs/nits-2026-03-16.md` → `nits-2026-03-16.md`
- `docs/future-inspection-coverage.md` → `future-inspection-coverage.md`
- `docs/future-visual-improvements.md` → `future-visual-improvements.md`
- Release notes (`RELEASE-*.md`) → project root

After this move, everything in `docs/` is user-facing and publishable. No exclude list needed in `_config.yml` beyond standard Jekyll excludes.

### 3.2 File Tree

```
docs/
├── index.md                                  # Docs landing page (GitHub Pages home)
├── getting-started.md                        # Tutorial: install → scan → refine → understand
├── tutorials/
│   └── first-migration.md                    # Tutorial: end-to-end single-host migration
├── how-to/
│   ├── review-and-refine.md                  # Flagship refine web UI workflow
│   ├── fleet-aggregation.md                  # Scan N hosts → fleet init → aggregate → refine
│   ├── baseline-subtraction.md               # Using --base-image to filter known-good
│   ├── customize-output.md                   # Progress modes, verbosity, redaction flags
│   ├── ci-integration.md                     # Running inspectah in CI pipelines
│   └── build-bootc-image.md                  # [BANNER: build not yet in Rust — manual podman build from tarball]
├── reference/
│   ├── cli.md                                # Generated from `inspectah --markdown-help`
│   ├── triage-classification.md              # Baseline/Site/Investigate + fleet consensus
│   ├── output-artifacts.md                   # Per-command artifact contracts (scan, fleet aggregate, refine export)
│   ├── snapshot-schema.md                    # JSON schema reference
│   ├── inspector-coverage.md                 # What each inspector scans
│   ├── fleet-manifest.md                     # TOML manifest format reference
│   └── configuration.md                      # Config file, env vars, defaults
├── explanation/
│   ├── architecture.md                       # Rust workspace architecture (embeds D3)
│   ├── migration-model.md                    # Why package-mode → image-mode
│   ├── triage-philosophy.md                  # Design rationale behind classification
│   └── fleet-consensus.md                    # How fleet aggregation works conceptually
├── contributing/
│   ├── developer-guide.md                    # Build, test, contribute (Rust workspace)
│   └── adding-an-inspector.md                # Step-by-step: add a new inspector
├── diagrams/                                 # D3 interactive diagrams (HTML)
│   ├── shared/                              # Shared D3 utilities (published as static assets)
│   │   ├── theme.js                          # Color system, CSS variables
│   │   ├── interactions.js                   # Zoom, pan, tooltip, expand/collapse
│   │   ├── accessibility.js                  # Keyboard nav, focus management, ARIA
│   │   └── embed.js                          # Iframe detection, fullscreen, preview mode
│   ├── conceptual-pipeline.html              # Scan → inspect → redact → render (user-facing)
│   ├── user-flow.html                        # User journey: discovery → daily use (user-facing)
│   ├── triage-decision-tree.html             # Classification logic with examples (user-facing)
│   ├── fleet-topology.html                   # Single → fleet → architect (user-facing)
│   ├── software-architecture.html            # Crate dependency graph (developer-facing)
│   └── data-flow.html                        # Snapshot through pipeline (developer-facing)
├── images/                                   # Static assets (screenshots, PNGs)
└── _config.yml                               # Jekyll config (just-the-docs theme)
```

### 3.3 Design Principles

1. **Audience-first.** Three personas: sysadmin evaluating, sysadmin using daily, contributor. First two are priority.
2. **Diataxis framework.** Tutorials (learning), how-to (task), reference (information), explanation (understanding).
3. **Getting-started funnel.** README → docs landing → first scan → understanding output → refining → fleet. Each step links to the next. Only features that exist in the Rust CLI appear in this funnel.
4. **CLI is source of truth.** CLI reference generated from `inspectah --markdown-help`. Narrative docs link to it. Manual validation should be unnecessary except as a CI check.
5. **No hard-coded implementation counts.** Inspector counts, type module counts, snapshot section counts, and crate topology are derived from code at implementation time, not pinned in the spec. Descriptions use stable conceptual language ("each inspector," "all type modules") rather than exact numbers.
6. **Image-mode accuracy.** Baseline, redaction, and layer terminology must match bootc/image-mode semantics. See § 4.4 for framing rules.

### 3.4 Page Count

| Category | Pages |
|----------|-------|
| Tutorials | 2 |
| How-to guides | 6 (1 deferred) |
| Reference | 7 |
| Explanation | 4 |
| Contributing | 2 |
| **Written total** | **21** |
| D3 diagrams | 6 |
| **Grand total** | **27** |

## 4. D3 Interactive Diagram Suite

### 4.1 Dual-Mode Rendering

Every diagram supports two modes via a shared embed library (`diagrams/shared/embed.js`):

**Embedded preview (iframe):**
- Height: 400-500px within parent doc page
- Shows high-level structure with hover tooltips (mouse) or focus tooltips (keyboard)
- Click-to-expand disabled in preview; interacting with nodes shows a brief label only
- Prominent labeled button **outside the iframe** in the parent page: "Open interactive diagram" (not "click the diagram itself")
- Button behavior: `requestFullscreen()` on the iframe element, with fallback to `window.open()` for the standalone URL
- `<iframe>` attributes: `title="[diagram name] — interactive preview"`, `tabindex="0"`, `loading="lazy"`

**Fullscreen / standalone mode:**
- Full zoom/pan via D3 zoom behavior
- Click-to-expand nodes with animated transitions
- Animated flow dots on connection lines
- Keyboard navigation (see § 4.3)
- Visible "Exit fullscreen" button with keyboard shortcut label
- Focus returns to the expand button in the parent page when fullscreen closes

Detection: `window.self !== window.top` for iframe context. Standalone pages also check for Fullscreen API state.

### 4.2 Unified Visual Language

All 6 diagrams share:

- **Dark theme** — `#0f1729` background matching the inspectah refine UI palette
- **Color system:**
  - Green (`#22c55e`) — input / foundation
  - Teal (`#2dd4bf`) — collection / processing
  - Blue (`#60a5fa`) — data structures / snapshots
  - Purple (`#c084fc`) — decisions / orchestration
  - Amber (`#f59e0b`) — user journey stages
  - Rose (`#f472b6`) — fleet / architect
  - Red (`#ef4444`) with glow — sensitive data handling (redaction)
  - Orange (`#f97316`) — output / artifacts
- **Interaction patterns:**
  - Click-to-expand nodes with animated transitions
  - Hover tooltips with descriptions
  - Zoom/pan via D3 zoom behavior (fullscreen/standalone only)
  - Animated flow dots on connection lines (respects `prefers-reduced-motion`)
  - Glow filter on critical nodes (redaction, snapshot)
- **Navigation:** Each standalone diagram includes a back-to-docs link, title overlay, and legend
- **Shared D3 utilities:** `diagrams/shared/` — theme, interactions, accessibility, embed detection

### 4.3 Accessibility Contract

All diagrams must satisfy the following for public docs:

**Keyboard navigation (fullscreen/standalone):**
- Tab moves focus between nodes in reading order
- Enter/Space expands/collapses focused node
- Escape closes expanded content or exits fullscreen
- Arrow keys for spatial navigation between adjacent nodes

**Focus management:**
- Visible focus indicator on all interactive nodes (2px outline, high contrast)
- Focus trapped inside expanded node content until Escape
- Focus restored to parent node on collapse
- Focus restored to expand button in parent page on fullscreen exit

**Motion:**
- All animated flow indicators and transitions respect `prefers-reduced-motion: reduce`
- When reduced motion is active: no flow dots, instant expand/collapse, no glow pulse

**Screen readers:**
- All nodes have `role="button"` and `aria-expanded="true/false"` where expandable
- Tooltip content available via `aria-describedby`, not hover-only
- SVG elements have `<title>` and `<desc>` tags
- Iframe has descriptive `title` attribute

**Text equivalents:**
- Each embedded diagram has a short text summary (2-3 sentences) directly below the iframe in the parent markdown page, describing what the diagram shows. The diagram enhances the explanation; the explanation does not depend solely on the diagram.

### 4.4 Image-Mode Framing Rules

These rules apply to all diagrams and written docs. They address conceptual accuracy concerns raised by Collins in round 1 review.

**Baseline semantics:**
- Baseline means "content that is already present in the target base image and does not need to be added to the Containerfile."
- Do NOT describe it as "in base image already → auto-included" — that implies inspectah adds it. Baseline items are *subtracted* from the migration scope, not included.
- Correct framing: "Already in the base image → no action needed."

**Build subcommand:**
- `build` is not yet in Rust. Do not include it in the primary user journey or conceptual pipeline diagrams.
- The single-host path ends at "generated migration artifacts" (Containerfile + tarball), not at a build step.
- `how-to/build-bootc-image.md` exists with a banner explaining manual `podman build` from the tarball. It is not linked from the getting-started funnel.

**Redaction:**
- Frame as "sensitive data handling" or "redaction and review," never "trust signal" or "security trust."
- In bootc/image-mode, "trust" has specific meaning: provenance, signature policy, UKI/dm-verity boot integrity chain.
- Redaction is artifact hygiene — inspectah redacts what it can and hands off `secrets-review.md` for manual review. The output may still require operator attention.
- The redaction node gets a glow and distinct color to draw attention, but the framing is "review boundary" not "trust seal."

**Architect / layer decomposition:**
- Label layers as **build-time image composition** artifacts, not runtime boundaries.
- "Base / App / Role" are derived-image build relationships (Containerfile `FROM` chains), not bootc runtime strata that sysadmins manage independently.
- Add explicit caveat: layer decomposition does not imply every captured artifact splits cleanly across layers.

### 4.5 Diagram Specifications

Implementation note: all inspector counts, type module counts, snapshot section counts, and crate module lists are **derived from the code at implementation time**. The spec describes conceptual content; the implementer reads the actual source to populate node labels and expansion content.

#### Diagram 1: Conceptual Pipeline (user-facing)
**Embeds in:** getting-started.md, migration-model.md

Stages: Host Input → Preflight → Inspectors → Snapshot → **Redaction** → Renderers → Tarball

- Preflight expands: podman check, root check, registry auth check
- Inspectors expand: all registered inspectors listed individually (derived from `scan.rs` at implementation time)
- Snapshot expands: JSON schema version, section list (derived from `snapshot.rs`)
- **Redaction** has glow (attention signal — see § 4.4). Expands to show: what gets masked (passwords, keys, tokens), what the operator reviews (`secrets-review.md`), that output may still require manual review
- Renderers expand: list derived from pipeline source (Containerfile, audit-report, report.html, secrets-review, etc.)
- **No build step.** Pipeline ends at tarball output.

#### Diagram 2: User Flow / Journey (user-facing)
**Embeds in:** index.md, getting-started.md

Stages: Discover → Install → First Scan → Understand Output → Refine

Branch point after Refine:
- Single Host Path: Refine → Migration Artifacts (Containerfile + tarball). Ends here. A note indicates "build the image manually with `podman build`; see how-to guide."
- Fleet Path: Fleet init → Aggregate → Fleet Refine (iterate). Ends at refined fleet-level migration artifacts.
- Future direction (visually distinct, dashed/dimmed): Architect → Build-time image composition plan. Labeled "planned — not yet in the CLI."

Each stage expands with:
- CLI command (exact invocation with common flags)
- What you'll see (output description, progress indicators)
- What it produces (files, artifacts)
- Common issues (auth errors, permission problems)
- Link to relevant doc page

#### Diagram 3: Triage Decision Tree (user-facing)
**Embeds in:** triage-classification.md, triage-philosophy.md

**Single-host layer:**
Found Item → Baseline / Site / Investigate

- Baseline: "Already in the base image → no action needed" (subtracted from scope, not included)
- Site: "User-installed or configured → add to Containerfile"
- Investigate: "Unclear, needs review → human decision"

Each classification expands with: criteria, concrete examples, Containerfile action, section promotion behavior in refine UI.

**Fleet consensus layer:**
- **Universal:** Found on all hosts. Package version differences are normal — still universal.
- **Partial:** Present on some hosts, not others. Role-based presence variation. Example: "httpd installed on web-01, web-02 but not db-01" — different roles, expected.
- **Divergent:** Same item everywhere but configured differently. Config file variants across hosts that should be consistent. Example: "/etc/httpd/conf.d/ssl.conf has different cipher suites on web-01 vs web-02."
- **Investigate:** Unclear, needs human review.

The Partial/Divergent distinction is the most common source of confusion. The diagram must make it visually unambiguous:
- Partial = **presence** varies (some have it, some don't)
- Divergent = **configuration** differs (all have it, but it's set up differently)

#### Diagram 4: Fleet Topology (user-facing)
**Embeds in:** fleet-aggregation.md, fleet-consensus.md

Layout: Multiple host nodes at top → Fleet Aggregate → Fleet Refine (with iterate loop) → Refined fleet artifacts

- Host nodes show hostname, OS version; expand to inspector summary, package count, config diff count
- Fleet Aggregate expands: consensus matrix showing universal/partial/divergent/investigate distribution
- Fleet Refine shows iterate loop (animated arc back to refine)
- Fleet artifacts: per-host Containerfiles, fleet-level reports, consensus summary
- **Future direction (visually distinct — dashed border, dimmed opacity):** Architect → Image composition output (Base / App / Role). Labeled "planned — not yet in the CLI. Build-time image composition, not runtime layer boundaries."

#### Diagram 5: Software Architecture (developer-facing)
**Embeds in:** architecture.md, developer-guide.md

Crate dependency graph. Layout and tier grouping derived from actual `Cargo.toml` dependency declarations at implementation time. The existing conceptual tiers are:
- Entry points (binary/app crates)
- Orchestration (pipeline, refine)
- Collection and foundation (collect, core)

Expansion capped at crate/module/contract level for v1 (deeper type/trait/field detail deferred until codebase stabilizes or generation tooling exists):
- Each crate → module list with one-line descriptions
- Key modules expand one more level to show their responsibility and public contract (e.g., "inspectors/ — implements the Inspector trait for each domain")
- Core types/ → domain type module names (not individual struct fields)
- Collect inspectors/ → registered inspector module names
- Pipeline → shows redaction step in pipeline flow, renderer chain
- Dependency arrows highlighted on hover

#### Diagram 6: Data Flow (developer-facing)
**Embeds in:** architecture.md, snapshot-schema.md

Flow: Host Filesystems → Inspectors → InspectionSnapshot → **Redact** → Renderers → Tarball

- Host Filesystems expands: categories of data read (filesystem paths, RPM database, systemd units, /proc)
- Inspectors expands: per-inspector summary (what domain it covers, key data sources — not internal struct field mappings)
- InspectionSnapshot has glow; expands to show schema version, section names, and purpose of each section
- **Redact** has glow; expands to show field masking rules, hash status classification (locked/disabled/password_set/no_password — never the actual hash), SSH key counting contract. Framed as "review boundary" per § 4.4.
- Renderers expands: per-renderer what it reads from snapshot and what it produces
- Tarball expands: full directory tree of output artifact
- All content derived from source at implementation time; always-written vs. conditional artifacts distinguished

## 5. README Rewrite

The current README is ~10KB and front-loads technical internals. The rewrite:

- **Under 250 lines.** Value proposition → install → 60-second quickstart → output overview → links to docs site.
- Remove all Go/Python/container-image references.
- Document the current Rust CLI workflows: `scan`, `refine`, `fleet`.
- `build` not mentioned in the main flow. A note under output says "build the image manually with `podman build` from the generated Containerfile; see docs for details."
- No inspector details, no baseline generation internals, no layer ordering — all in the docs site.
- "See Also" section links to the GitHub Pages docs site for everything deeper.

## 6. GitHub Pages Setup

- **Theme:** `just-the-docs` — clean navigation, built-in search, dark mode support.
- **Publish source:** `docs/` directory on `main` branch. `docs/` contains **only** user-facing content (see § 3.1).
- **Navigation structure:** Getting Started, Tutorials, How-To Guides, Reference, Explanation, Contributing. Diagrams are discovered in-context via embeds in their parent pages — no standalone "Interactive Diagrams" nav entry.
- **D3 diagram pages:** Served as standalone HTML alongside Jekyll-rendered markdown. Jekyll passes through `.html` files in `diagrams/` without processing. The `shared/` subdirectory contains JS utilities published as normal static assets and referenced by each diagram's `<script>` tags.
- **`_config.yml`:** Theme, nav order, title, description. No complex exclude list needed since `docs/` is a clean surface.

## 7. Delivery Plan

Since the full docs cycle completes before the Rust→main merge, phases represent work ordering rather than hard merge gates. The ordering prioritizes the getting-started funnel first, then fills depth and diagrams.

### Phase 1: Structure + Core Funnel

| # | Task | Owner |
|---|------|-------|
| 1 | Move internal artifacts out of `docs/` to project root (plans, designs, backlog, etc.) | Mango |
| 2 | Delete Go-era docs from `docs/reference/` | Mango |
| 3 | Set up Jekyll + GitHub Pages (`_config.yml`, theme, nav) | Mango |
| 4 | Rewrite README.md (under 250 lines, Rust-only, no build in main flow) | Mango |
| 5 | Generate `reference/cli.md` from `inspectah --markdown-help` | Mango |
| 6 | Create `index.md` docs landing page | Mango |
| 7 | Write `getting-started.md` tutorial (the cold-start funnel) | Mango |
| 8 | Write `reference/output-artifacts.md` (per-command artifact contracts: scan, fleet aggregate, refine export) | Mango |
| 9 | Banner `build-bootc-image.md` ("not yet available in Rust; manual podman build") | Mango |

### Phase 2: Essential User Docs

| # | Task | Owner |
|---|------|-------|
| 10 | Write `how-to/review-and-refine.md` (refine UI — zero docs today) | Mango |
| 11 | Write `reference/triage-classification.md` (with Partial/Divergent clarity) | Mango |
| 12 | Write `how-to/fleet-aggregation.md` | Mango |
| 13 | Write `explanation/migration-model.md` | Mango |
| 14 | Write `explanation/triage-philosophy.md` | Mango |

### Phase 3: Diagrams

| # | Task | Owner |
|---|------|-------|
| 15 | Build shared D3 utilities (`diagrams/shared/`) | Mango (ui-ux-pro-max) |
| 16 | Build conceptual-pipeline.html | Mango (ui-ux-pro-max) |
| 17 | Build user-flow.html | Mango (ui-ux-pro-max) |
| 18 | Build triage-decision-tree.html | Mango (ui-ux-pro-max) |
| 19 | Build fleet-topology.html | Mango (ui-ux-pro-max) |
| 20 | Build software-architecture.html | Mango (ui-ux-pro-max) |
| 21 | Build data-flow.html | Mango (ui-ux-pro-max) |
| 22 | Embed all diagrams into parent doc pages with iframe + fullscreen button | Mango |

### Phase 4: Depth + Contributing

| # | Task | Owner |
|---|------|-------|
| 23 | Write `explanation/fleet-consensus.md` | Mango |
| 24 | Write `explanation/architecture.md` | Mango |
| 25 | Write remaining reference pages (snapshot-schema, inspector-coverage, fleet-manifest, configuration) | Mango |
| 26 | Write remaining how-to guides (baseline-subtraction, customize-output, ci-integration) | Mango |
| 27 | Write `contributing/developer-guide.md` (Rust workspace) | Mango |
| 28 | Write `contributing/adding-an-inspector.md` | Mango |

## 8. Skills and Tools

- **Mango:** Primary owner of the entire overhaul. Diataxis skill for written documentation. Owns docs architecture, content, GitHub Pages structure, and diagram implementation.
- **ui-ux-pro-max:** Invoked for D3 diagram visual design, interactivity, accessibility, and dual-mode rendering.
- **Collins consult:** For image-mode framing review on triage-classification, migration-model, and fleet-consensus docs before they publish.
- **Tang consult:** For Rust implementation accuracy checks — inspector lists, snapshot schema fields, crate topology — before diagrams or reference pages go live.

## 9. Out of Scope

- `inspectah build` subcommand as a current workflow (not yet in Rust)
- Release notes automation
- SEO optimization
- Deprecation/removal policy documentation
- driftify documentation (separate repo, separate effort)
- Code-generated diagram data (aspirational; v1 diagrams are hand-authored with implementation-time source review)

## 10. Round 1 Review Response

| Reviewer | Key Finding | Resolution |
|----------|-------------|------------|
| Fern | Phase 1 doesn't deliver cold-start funnel | Moved getting-started.md + output-artifacts.md into Phase 1; full cycle pre-merge |
| Fern | Missing accessibility contract for diagrams | Added § 4.3 with keyboard, focus, motion, screen reader, and text equivalent requirements |
| Fern | `build` inconsistency in user journey | Removed from primary flow; ends at migration artifacts |
| Kit | `docs/` boundary too loose | Added § 3.1: internal artifacts move to project root; `docs/` is clean publish surface |
| Kit | D3 suite over-scoped | Extracted shared utilities to `shared/`; each diagram builds on common code, reducing per-diagram scope |
| Kit | Embed/fullscreen contract vague | Detailed in § 4.1: iframe attributes, button placement, fullscreen API, focus restoration |
| Collins | Baseline semantics inverted | Fixed in § 4.4: "already in base image → no action needed" (subtracted, not included) |
| Collins | `build` in user journey | Removed from diagrams; single-host path ends at migration artifacts |
| Collins | Redaction framed as "trust signal" | Reframed to "sensitive data handling / review boundary" throughout (§ 4.4) |
| Collins | Architect layers imply runtime boundaries | Labeled as "build-time image composition" with explicit caveat (§ 4.4) |
| Tang | Hard-coded implementation counts stale | Removed all pinned numbers; § 3.3 principle: derive from code at implementation time |
| Tang | CLI ref should use `--markdown-help` | Changed to generated source (§ 3.2, § 7 task 5) |
| Tang | Dev diagram detail creates maintenance debt | Acknowledged in § 9 (code-generated data is aspirational); v1 diagrams hand-authored with source review at implementation time |

## 11. Round 2 Review Response

Fern and Collins approved in round 2 (dropped per approve-and-drop).

| Reviewer | Key Finding | Resolution |
|----------|-------------|------------|
| Kit | `diagrams/_shared/` not published by Jekyll (underscore prefix) | Renamed to `diagrams/shared/` — normal static asset path, no Jekyll special handling needed |
| Kit | Dev diagrams promise more source-coupled detail than hand-authored docs can maintain | Capped v1 dev diagram expansion at crate/module/contract level; deeper type/trait/field detail deferred until codebase stabilizes or generation tooling exists (§ 4.5 Diagrams 5, 6) |
| Tang | Architect still in shipped user journey despite not being in Rust CLI | Moved to future-direction treatment in Diagrams 2 and 4 — dashed border, dimmed opacity, "planned — not yet in the CLI" label. Same principle as `build` removal. |
| Tang | `output-artifacts.md` flattens distinct per-command contracts | Page now explicitly structured as per-command matrix: `scan`/`fleet aggregate` broad artifact set vs. `refine` export narrower contract (§ 3.2, § 7 task 8) |
