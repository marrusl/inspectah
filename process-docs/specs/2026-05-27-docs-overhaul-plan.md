# inspectah Documentation Overhaul — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver a complete, external-facing documentation site for inspectah using Diataxis framework, with 6 interactive D3 diagrams, before the Rust branch merges to main.

**Architecture:** Jekyll + GitHub Pages site published from `docs/`. Internal engineering artifacts (plans, designs, specs, backlog) moved to project root. D3 diagrams are standalone HTML pages with shared JS utilities, embedded via iframe in doc pages with click-to-fullscreen. All content derived from the current Rust codebase — no Go/Python references.

**Tech Stack:** Jekyll (just-the-docs theme), D3.js v7, GitHub Pages, Markdown, HTML/CSS/JS.

**Design Spec:** `specs/2026-05-27-docs-overhaul-design.md` (approved — 3-round review)

**Skills:** Diataxis for all documentation writing. `ui-ux-pro-max` for D3 diagram implementation.

**Image-Mode Framing Rules (apply to ALL tasks):**
- Baseline = "already in base image → no action needed" (subtracted, not included)
- Redaction = "sensitive data handling / review boundary" (never "trust signal")
- Architect = future-direction only (not in Rust CLI yet — dashed/dimmed in diagrams)
- Build = not in main flow (Rust CLI: scan, refine, fleet, version only)
- Layers = "build-time image composition" (not runtime boundaries)

---

## Phase 1: Structure + Core Funnel

### Task 1: Move internal artifacts out of `docs/`

**Files:**
- Move: `docs/plans/` → `plans/`
- Move: `docs/designs/` → `designs/`
- Move: `docs/backlog/` → `backlog/`
- Move: `docs/specs/` → merge into root `specs/` (includes `specs/proposed/`, `specs/implemented/`, `specs/reviews/`, `specs/plans/`, and loose files: `2026-05-11-phase2-inspector-parity-design.md`, `2026-04-26-build-subcommand-design.md`)
- Delete: `docs/superpowers/` (brainstorm artifacts — ephemeral, `.superpowers/` at root is gitignored)
- Move: `docs/ROADMAP.md` → `ROADMAP.md`
- Move: `docs/nit-list.md` → `nit-list.md`
- Move: `docs/nits-2026-03-16.md` → `nits-2026-03-16.md`
- Move: `docs/future-inspection-coverage.md` → `future-inspection-coverage.md`
- Move: `docs/future-visual-improvements.md` → `future-visual-improvements.md`
- Move: `docs/RELEASE-v0.8.2-alpha.1.md` → `RELEASE-v0.8.2-alpha.1.md`
- Move: `docs/RELEASE-v0.8.2-alpha.2.md` → `RELEASE-v0.8.2-alpha.2.md`

- [ ] **Step 1: Create root directories for moved content**

```bash
mkdir -p plans designs backlog
```

- [ ] **Step 2: Move all directories and files**

```bash
# Directories
git mv docs/plans/* plans/
git mv docs/designs/* designs/
git mv docs/backlog/* backlog/

# Merge docs/specs into root specs/ (root specs/ already exists)
git mv docs/specs/proposed specs/proposed
git mv docs/specs/implemented specs/implemented
git mv docs/specs/reviews specs/reviews
git mv docs/specs/plans specs/plans
# Loose spec files at docs/specs/ root
git mv docs/specs/2026-05-11-phase2-inspector-parity-design.md specs/
git mv docs/specs/2026-04-26-build-subcommand-design.md specs/

# Internal brainstorm artifacts (ephemeral, delete)
git rm -r docs/superpowers/

# Loose files
git mv docs/ROADMAP.md ROADMAP.md
git mv docs/nit-list.md nit-list.md
git mv docs/nits-2026-03-16.md nits-2026-03-16.md
git mv docs/future-inspection-coverage.md future-inspection-coverage.md
git mv docs/future-visual-improvements.md future-visual-improvements.md
git mv docs/RELEASE-v0.8.2-alpha.1.md RELEASE-v0.8.2-alpha.1.md
git mv docs/RELEASE-v0.8.2-alpha.2.md RELEASE-v0.8.2-alpha.2.md
```

- [ ] **Step 3: Remove now-empty directories**

```bash
rmdir docs/plans docs/designs docs/backlog docs/specs docs/superpowers 2>/dev/null || true
```

- [ ] **Step 4: Verify `docs/` is clean**

```bash
ls docs/
```

Expected: only user-facing content remains — `_config.yml`, `explanation/`, `how-to/`, `reference/`, `images/`, `diagrams/`. No internal dirs (`plans/`, `designs/`, `backlog/`, `specs/`, `superpowers/`). Run `find docs/ -type d` and verify every directory is either a Diataxis category, `diagrams/`, `images/`, or a Jekyll system dir.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "docs: move internal artifacts out of docs/ to project root

Cleans docs/ as the exclusive GitHub Pages publish surface.
Plans, designs, specs, backlog, release notes, and internal
tracking files move to project root directories."
```

### Task 2: Delete Go-era docs

**Files:**
- Delete: `docs/reference/architecture-diagram.md`
- Delete: `docs/reference/cli.md`
- Delete: `docs/reference/design.md`
- Delete: `docs/reference/implementation-plan.md`
- Delete: `docs/reference/quick-start-developer.md`
- Delete: `docs/reference/readme-developer.md`

- [ ] **Step 1: Delete all Go/Python-era reference docs**

```bash
git rm docs/reference/architecture-diagram.md
git rm docs/reference/cli.md
git rm docs/reference/design.md
git rm docs/reference/implementation-plan.md
git rm docs/reference/quick-start-developer.md
git rm docs/reference/readme-developer.md
```

- [ ] **Step 2: Verify directory is empty**

```bash
ls docs/reference/
```

Expected: empty (directory can remain for new reference docs).

- [ ] **Step 3: Commit**

```bash
git commit -m "docs: delete Go-era reference docs

Git history preserves them. Replaced by Rust-specific
documentation in the new Diataxis structure."
```

### Task 3: Delete old D3 diagrams

**Files:**
- Delete: `docs/diagrams/architecture.html`
- Delete: `docs/diagrams/conceptual.html`

- [ ] **Step 1: Delete existing diagrams**

```bash
git rm docs/diagrams/architecture.html
git rm docs/diagrams/conceptual.html
```

- [ ] **Step 2: Commit**

```bash
git commit -m "docs: remove old D3 diagrams

Will be replaced with unified-visual-language diagrams
in Phase 3."
```

### Task 4: Set up Jekyll + GitHub Pages

**Files:**
- Modify: `docs/_config.yml`
- Create: `docs/Gemfile`
- Modify: `.gitignore` (add Jekyll build artifacts)

- [ ] **Step 1: Write Jekyll config**

Create `docs/_config.yml`. Use `remote_theme` (GitHub Pages native, no local gem install needed for deployment):

```yaml
title: inspectah
description: Migration analysis tool for package-mode to image-mode RHEL
remote_theme: just-the-docs/just-the-docs@v0.10.0

url: ""
baseurl: ""

permalink: pretty

# Search
search_enabled: true
search:
  heading_level: 3

# Color scheme
color_scheme: dark

# Footer
gh_edit_link: true
gh_edit_link_text: "Edit this page on GitHub"
gh_edit_repository: "https://github.com/mrussell/inspectah"
gh_edit_branch: "main"
gh_edit_source: docs
gh_edit_view_mode: "tree"

# Aux links (top right)
aux_links:
  "GitHub":
    - "https://github.com/mrussell/inspectah"

# Plugins (required for remote_theme on GitHub Pages)
plugins:
  - jekyll-remote-theme
```

**Navigation groups in just-the-docs:** Top-level nav categories are created by giving pages a `nav_order` in their front matter. Pages with `has_children: true` become parent categories. Child pages use `parent: "<parent title>"`. For example:

```yaml
# docs/how-to/review-and-refine.md
---
title: Review and Refine
parent: How-To Guides
nav_order: 1
---
```

```yaml
# docs/how-to-index.md (creates the "How-To Guides" nav group)
---
title: How-To Guides
nav_order: 4
has_children: true
---
```

Each Diataxis category needs a nav-group index page:
- `docs/tutorials-index.md` → "Tutorials" (nav_order: 3)
- `docs/how-to-index.md` → "How-To Guides" (nav_order: 4)
- `docs/reference-index.md` → "Reference" (nav_order: 5)
- `docs/explanation-index.md` → "Explanation" (nav_order: 6)
- `docs/contributing-index.md` → "Contributing" (nav_order: 7)

These are minimal files — just front matter with `has_children: true` and a one-line description. The actual child pages populate the nav automatically.

**Diagrams in nav:** Diagrams are standalone HTML files, not Jekyll-rendered markdown. They do NOT appear in the just-the-docs nav tree. Users discover diagrams via embed iframes in doc pages and the "Open interactive diagram" buttons. No separate "Interactive Diagrams" nav destination — diagrams are accessed in context, not as a standalone section.

- [ ] **Step 2: Create Gemfile for local testing**

Create `docs/Gemfile`:

```ruby
source "https://rubygems.org"

gem "jekyll", "~> 4.3"
gem "just-the-docs", "~> 0.10"
gem "jekyll-remote-theme"
gem "webrick"
```

- [ ] **Step 3: Create nav-group index pages**

Create 5 minimal index pages for each Diataxis category. Example (`docs/how-to-index.md`):

```yaml
---
title: How-To Guides
nav_order: 4
has_children: true
---

Task-oriented guides for common inspectah workflows.
```

Create: `tutorials-index.md`, `how-to-index.md`, `reference-index.md`, `explanation-index.md`, `contributing-index.md`.

- [ ] **Step 4: Update .gitignore**

Append to `.gitignore`:

```
# Jekyll
docs/_site/
docs/.jekyll-cache/
docs/.jekyll-metadata
docs/Gemfile.lock
```

- [ ] **Step 5: Test Jekyll builds locally**

```bash
cd docs && bundle install && bundle exec jekyll serve
```

Open `http://localhost:4000`. Verify:
- Just-the-docs dark theme loads
- Left nav shows: Home, Getting Started, Tutorials, How-To Guides, Reference, Explanation, Contributing
- Search works (type a query, results appear)
- No diagram HTML files appear in nav
- No internal engineering material visible

- [ ] **Step 6: Commit**

```bash
git add docs/_config.yml docs/Gemfile docs/*-index.md .gitignore
git commit -m "docs: set up Jekyll with just-the-docs theme

Configures GitHub Pages site with remote_theme, dark mode,
search, nav groups for each Diataxis category."
```

### Task 5: Create docs landing page

**Files:**
- Create: `docs/index.md`

- [ ] **Step 1: Write the landing page**

Create `docs/index.md` with front matter for just-the-docs:

```yaml
---
title: Home
layout: home
nav_order: 1
---
```

Content structure:
- **Opening paragraph:** What inspectah does (1-2 sentences). Migration analysis tool that scans package-mode RHEL/CentOS/Fedora hosts and generates image-mode migration artifacts.
- **Quick links section:** Getting Started, CLI Reference
- **Documentation sections:** Tutorials, How-To Guides, Reference, Explanation, Contributing — each with a one-line description and link
- **No standalone diagrams link.** Diagrams are discovered via embeds in their parent pages, not as a top-level destination.
- **Current CLI surface:** `scan`, `refine`, `fleet` (init, aggregate), `version`

Sources to consult:
- `README.md` for current value proposition
- `inspectah-cli/src/main.rs` for actual subcommand list
- Design spec § 3.3 for design principles

- [ ] **Step 2: Verify it renders**

```bash
cd docs && bundle exec jekyll serve
```

Open `http://localhost:4000` — landing page should show with just-the-docs theme, dark mode, working nav.

- [ ] **Step 3: Commit**

```bash
git add docs/index.md
git commit -m "docs: add landing page for docs site"
```

### Task 6: Rewrite README.md

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Read current README for content to preserve**

The current README (~10KB) has good content buried under too much technical depth. Preserve:
- Value proposition (first paragraph of "What is inspectah?")
- Installation methods (RPM, Homebrew, source)
- Basic scan workflow
- Output artifacts tree
- License

Remove:
- All Go/Python/container-image references
- Inspector details (moved to docs site)
- Baseline generation internals
- Layer ordering details
- Shell script (legacy) section

- [ ] **Step 2: Write the new README**

Target: under 250 lines. Structure:

```markdown
# inspectah

One-paragraph value proposition.

## Quick Start

3-step: install, scan, view output. 60 seconds.

## Installation

### RPM (Fedora / RHEL / CentOS Stream)
### Homebrew (macOS)
### From source

## What It Does

scan → inspect → triage → generate migration artifacts.
NOT a build step — inspectah generates Containerfiles and reports.
Build the image yourself with `podman build`.

## Output

Tarball tree showing key files.

## Commands

Brief table: scan, refine, fleet (init, aggregate), version.
Link to full CLI reference on docs site.

## Documentation

Link to GitHub Pages docs site.

## License
```

Sources:
- Current `README.md` for installation instructions
- `inspectah-cli/src/main.rs` for subcommand list
- `cargo run -q -p inspectah-cli -- scan --help` for scan flags
- Design spec § 5 for requirements

- [ ] **Step 3: Verify no Go/Python references remain**

```bash
grep -i -n 'python\|\.py\|container.*image\|docker\|go run\|go build\|main\.go' README.md
```

Expected: no matches.

- [ ] **Step 4: Verify line count**

```bash
wc -l README.md
```

Expected: under 250 lines.

- [ ] **Step 5: Commit**

```bash
git add README.md
git commit -m "docs: rewrite README for Rust CLI

Under 250 lines. Value prop, install, quick start,
output overview. Links to docs site for depth.
No Go/Python references."
```

### Task 7: Generate CLI reference

**Files:**
- Create: `docs/reference/cli.md`

- [ ] **Step 1: Generate markdown from CLI**

```bash
cargo run -q -p inspectah-cli -- --markdown-help > docs/reference/cli.md
```

- [ ] **Step 2: Add just-the-docs front matter**

Prepend to `docs/reference/cli.md`:

```yaml
---
title: CLI Reference
parent: Reference
nav_order: 1
---
```

- [ ] **Step 3: Review generated output**

Open the file. Verify it covers: `scan`, `refine`, `fleet` (with `init`, `aggregate` subcommands), `version`. Verify no stale or undocumented flags.

- [ ] **Step 4: Commit**

```bash
git add docs/reference/cli.md
git commit -m "docs: generate CLI reference from --markdown-help"
```

### Task 8: Write getting-started tutorial

**Files:**
- Create: `docs/getting-started.md`

Invoke the **Diataxis** skill for this task. This is a **tutorial** — learning-oriented, follows a specific path, results in a working outcome.

- [ ] **Step 1: Write the getting-started tutorial**

Create `docs/getting-started.md` with front matter:

```yaml
---
title: Getting Started
nav_order: 2
---
```

Content structure — the cold-start funnel:

1. **Prerequisites** — RHEL/CentOS/Fedora host, podman, root access, registry auth for RHEL
2. **Install inspectah** — RPM, Homebrew, or cargo install (brief, link to README for details)
3. **Scan your first host** — `sudo inspectah scan`, what to expect (progress output, timing)
4. **Understand the output** — what's in the tarball, key files (Containerfile, audit-report.md, report.html, secrets-review.md). Link to `reference/output-artifacts.md`
5. **Open the refine UI** — `inspectah refine <tarball>`, open browser, what you see
6. **Understand triage classifications** — Baseline/Site/Investigate at a glance. Link to `reference/triage-classification.md`
7. **Next steps** — fleet aggregation, baseline subtraction, full docs site

Sources to consult:
- `inspectah-cli/src/commands/scan.rs` for actual scan behavior
- `inspectah-cli/src/commands/refine.rs` for refine behavior
- `inspectah-pipeline/src/render/mod.rs` for what gets rendered
- Existing README "Getting Started" section for install/scan flow
- Design spec § 4.4 for framing rules

- [ ] **Step 2: Verify all CLI commands shown actually work**

Run every command from the tutorial on a test host or check against `--help` output. Verify no flags are wrong.

- [ ] **Step 3: Verify no `build` in the main flow**

```bash
grep -n 'inspectah build\|build subcommand\|build the image' docs/getting-started.md
```

Expected: only a note pointing to manual `podman build` / how-to guide, not as part of the main tutorial path.

- [ ] **Step 4: Commit**

```bash
git add docs/getting-started.md
git commit -m "docs: add getting-started tutorial

Cold-start funnel: install → scan → understand output →
refine. Covers the complete first-run path for a sysadmin."
```

### Task 9: Write output artifacts reference

**Files:**
- Create: `docs/reference/output-artifacts.md`

Invoke the **Diataxis** skill. This is a **reference** page — information-oriented, structured for lookup.

- [ ] **Step 1: Map the per-command artifact contracts from source**

The Rust codebase has at least two materially different output surfaces. Inspect both fully — do not truncate with `head`.

```bash
# Scan/fleet aggregate: read the full render module to find ALL renderers
cat inspectah-pipeline/src/render/mod.rs

# Identify every renderer function — these map to output files
grep -rn 'pub fn render\|fn render_' inspectah-pipeline/src/render/

# Refine export: read the full export function
grep -n 'render_refine_export\|export' inspectah-refine/src/session.rs

# Refine export contract test — this is the authoritative spec for what refine exports
cat inspectah-refine/tests/export_contract_test.rs

# Identify conditional vs always-written artifacts
grep -rn 'if \|Option\|Some\|None' inspectah-pipeline/src/render/mod.rs | grep -i 'render\|write\|create'
```

Build a complete matrix before writing: which files does `scan` always produce? Which are conditional (e.g., secrets-review only when secrets found)? What does `fleet aggregate` produce that `scan` doesn't? What is the exact narrower set that `refine` exports?

- [ ] **Step 2: Write the reference page**

Create `docs/reference/output-artifacts.md` with front matter:

```yaml
---
title: Output Artifacts
parent: Reference
nav_order: 4
---
```

Content structure — per-command matrix:

1. **`inspectah scan` output** — full tarball tree. List every file with a one-line description. Distinguish always-written vs. conditional artifacts.
2. **`inspectah fleet aggregate` output** — what the fleet aggregate command produces (fleet-level reports, consensus data).
3. **`inspectah refine` export** — narrower contract. What gets exported when the user saves from the refine UI. Reference the export contract test for accuracy.
4. **Tarball structure** — visual tree diagram of a scan tarball (preserve the good tree from the current README).

Sources:
- `inspectah-pipeline/src/render/mod.rs` for the full renderer list
- `inspectah-refine/src/session.rs` for refine export
- `inspectah-refine/tests/export_contract_test.rs` for the export contract
- Current README "Output Artifacts" section for the tree diagram
- Design spec § 4.4 for framing rules (no build step in the flow)

- [ ] **Step 3: Commit**

```bash
git add docs/reference/output-artifacts.md
git commit -m "docs: add output artifacts reference

Per-command artifact contracts: scan, fleet aggregate,
refine export. File-by-file reference with always-written
vs conditional distinction."
```

### Task 10: Rewrite build-bootc-image as manual podman build guide

**Files:**
- Rewrite: `docs/how-to/build-bootc-image.md`

The existing page was written for the Go CLI's `inspectah build` subcommand, which does not exist in the Rust CLI. This is a **full rewrite**, not a banner patch — the legacy content references a different tool surface.

- [ ] **Step 1: Read the existing page to identify salvageable content**

The RHEL subscription cert handling section may still be accurate. The `podman build` concepts are reusable. Everything referencing `inspectah build` is stale.

- [ ] **Step 2: Rewrite the page**

```yaml
---
title: Build a bootc Image from inspectah Output
parent: How-To Guides
nav_order: 6
---
```

Content structure:

```markdown
> **Note:** inspectah generates migration artifacts (Containerfile, configs, reports)
> but does not currently build images. Use `podman build` to create the image
> from the generated Containerfile.

## Prerequisites
- Completed `inspectah scan` with output tarball
- podman installed
- For RHEL: valid subscription certs (see below)

## Extract the tarball
tar xzf <hostname>-<timestamp>.tar.gz
cd <hostname>-<timestamp>/

## Build the image
podman build -t <your-image-name> .

## RHEL Subscription Cert Handling
[Rewrite from existing page — the cert detection, mounting,
and expiry check content is still valid. Reframe around
manual podman build, not inspectah build.]

## Cross-architecture builds
podman build --platform linux/arm64 ...

## Push to a registry
podman push <your-image-name> <registry>/<repo>:<tag>

## Troubleshooting
[Rewrite from existing — keep the dnf/registry auth scenarios,
remove inspectah build-specific errors]
```

- [ ] **Step 3: Verify no `inspectah build` references remain**

```bash
grep -n 'inspectah build\|inspectah.*build' docs/how-to/build-bootc-image.md
```

Expected: no matches.

- [ ] **Step 4: Commit**

```bash
git add docs/how-to/build-bootc-image.md
git commit -m "docs: rewrite build-bootc-image for manual podman build

Full rewrite from legacy inspectah build guide.
Covers podman build from scan tarball output,
RHEL cert handling, cross-arch, push to registry."
```

## Phase 2: Essential User Docs

### Task 11: Write refine UI how-to

**Files:**
- Create: `docs/how-to/review-and-refine.md`

Invoke **Diataxis** skill. This is a **how-to guide** — task-oriented, assumes the reader already has a scan tarball.

- [ ] **Step 1: Research refine UI behavior**

```bash
# Refine command and flags
cargo run -q -p inspectah-cli -- refine --help

# Web handler routes (what pages/features exist)
grep -n 'fn \|route\|handler\|get\|post' inspectah-web/src/lib.rs | head -30

# Refine session management
grep -n 'pub fn\|pub struct' inspectah-refine/src/session.rs | head -20

# Fleet refine handlers (if any)
grep -rn 'fleet' inspectah-web/src/ | head -10
```

- [ ] **Step 2: Write the how-to guide**

Create `docs/how-to/review-and-refine.md`:

```yaml
---
title: Review and Refine Findings
parent: How-To Guides
nav_order: 1
---
```

Content structure:
1. **Start the refine server** — `inspectah refine <tarball>`, browser opens
2. **Navigate the dashboard** — what each section shows (packages, configs, services, etc.)
3. **Understand triage indicators** — Baseline (green), Site (amber), Investigate (red)
4. **Promote/demote items** — how to move items between classifications
5. **Review sensitive data** — secrets-review section, redaction indicators
6. **Export refined results** — save updated Containerfile and reports
7. **Fleet mode** — if using fleet data, how the cross-host view differs

Sources:
- `inspectah-web/` for actual UI structure
- `inspectah-refine/` for session/triage logic
- Design spec § 4.4 for redaction framing

- [ ] **Step 3: Commit**

```bash
git add docs/how-to/review-and-refine.md
git commit -m "docs: add refine UI how-to guide

First documentation of the flagship refine web UI.
Covers navigation, triage, promotion, export."
```

### Task 12: Write triage classification reference

**Files:**
- Create: `docs/reference/triage-classification.md`

Invoke **Diataxis** skill. This is a **reference** page.

- [ ] **Step 1: Research triage model in source**

```bash
# Triage types and classification logic
grep -rn 'Baseline\|Site\|Investigate\|Universal\|Partial\|Divergent' inspectah-core/src/types/ | head -30

# Section promotion logic
grep -rn 'promot\|classif' inspectah-refine/src/ | head -20
```

- [ ] **Step 2: Write the reference page**

Create `docs/reference/triage-classification.md`:

```yaml
---
title: Triage Classification
parent: Reference
nav_order: 2
---
```

Content structure:
1. **Single-host classifications:**
   - Baseline — "Already in the base image → no action needed" (subtracted from scope)
   - Site — "User-installed or configured → add to Containerfile"
   - Investigate — "Unclear, needs review → human decision"
2. **Fleet consensus classifications:**
   - Universal — found on all hosts (version differences are normal)
   - Partial — present on some hosts, not others (role-based)
   - Divergent — same item everywhere but configured differently (config file variants)
   - Investigate — unclear, needs review
3. **The Partial vs. Divergent distinction** — call this out explicitly:
   - Partial = **presence** varies
   - Divergent = **configuration** differs
   - Concrete examples for each
4. **Section promotion** — how items move between classifications in the refine UI
5. **How classifications map to Containerfile actions**

- [ ] **Step 3: Commit**

```bash
git add docs/reference/triage-classification.md
git commit -m "docs: add triage classification reference

Baseline/Site/Investigate for single-host.
Universal/Partial/Divergent/Investigate for fleet.
Clear Partial vs Divergent distinction with examples."
```

### Task 13: Write fleet aggregation how-to

**Files:**
- Create: `docs/how-to/fleet-aggregation.md`

Invoke **Diataxis** skill. **How-to guide.**

- [ ] **Step 1: Research fleet commands**

```bash
cargo run -q -p inspectah-cli -- fleet --help
cargo run -q -p inspectah-cli -- fleet init --help
cargo run -q -p inspectah-cli -- fleet aggregate --help
```

- [ ] **Step 2: Write the how-to**

Create `docs/how-to/fleet-aggregation.md`:

```yaml
---
title: Fleet Aggregation
parent: How-To Guides
nav_order: 2
---
```

Content structure:
1. **Scan multiple hosts** — run `inspectah scan` on each host, collect tarballs
2. **Initialize fleet** — `inspectah fleet init`, what the TOML manifest looks like
3. **Aggregate** — `inspectah fleet aggregate`, what it produces
4. **Refine fleet data** — `inspectah refine` with fleet data, cross-host view
5. **Understand consensus** — link to triage-classification reference

Note: Architect is NOT part of this guide (not yet in Rust). Mention it as "planned future capability" in a note at the end if appropriate.

- [ ] **Step 3: Commit**

```bash
git add docs/how-to/fleet-aggregation.md
git commit -m "docs: add fleet aggregation how-to

Scan N hosts, fleet init, aggregate, refine.
No Architect (not yet in Rust CLI)."
```

### Task 14: Write migration model explanation

**Files:**
- Create: `docs/explanation/migration-model.md`

Invoke **Diataxis** skill. This is an **explanation** — understanding-oriented, discusses the "why."

- [ ] **Step 1: Write the explanation**

Create `docs/explanation/migration-model.md`:

```yaml
---
title: Migration Model
parent: Explanation
nav_order: 2
---
```

Content structure:
1. **Package mode vs. image mode** — what they are, why organizations migrate
2. **What inspectah does in this picture** — scans the current state, classifies everything, generates migration artifacts
3. **The inspection→triage→artifact pipeline** — conceptual overview
4. **What inspectah does NOT do** — does not build images, does not deploy, does not manage runtime. It generates analysis and artifacts.
5. **The role of the sysadmin** — inspectah assists, human decides

Collins consult recommended before publishing. Follow § 4.4 framing rules strictly.

- [ ] **Step 2: Commit**

```bash
git add docs/explanation/migration-model.md
git commit -m "docs: add migration model explanation

Why package-mode to image-mode. What inspectah does
and does not do in the migration picture."
```

### Task 15: Write triage philosophy explanation

**Files:**
- Create: `docs/explanation/triage-philosophy.md`

Invoke **Diataxis** skill. **Explanation.**

- [ ] **Step 1: Write the explanation**

Create `docs/explanation/triage-philosophy.md`:

```yaml
---
title: Triage Philosophy
parent: Explanation
nav_order: 3
---
```

Content structure:
1. **Why triage?** — a host has hundreds of packages, configs, services. Not all matter for migration.
2. **The classification approach** — baseline subtraction, site-specific identification, investigation flags
3. **Why three categories, not two** — the value of "Investigate" as an explicit uncertainty marker
4. **Fleet consensus** — why fleet-level analysis adds a second classification axis
5. **Design choices** — why version differences are "universal" not "divergent," why config differences are the real signal

Sources:
- `inspectah-core/src/types/` for classification types
- Design spec § 4.4 and § 4.5 Diagram 3 for framing

- [ ] **Step 2: Commit**

```bash
git add docs/explanation/triage-philosophy.md
git commit -m "docs: add triage philosophy explanation

Design rationale behind the classification system.
Why three categories, why fleet consensus."
```

## Phase 3: Diagrams

### Task 16: Build shared D3 utilities

**Files:**
- Create: `docs/diagrams/shared/theme.js`
- Create: `docs/diagrams/shared/interactions.js`
- Create: `docs/diagrams/shared/accessibility.js`
- Create: `docs/diagrams/shared/embed.js`

Invoke **ui-ux-pro-max** skill for this and all diagram tasks.

- [ ] **Step 1: Write theme.js**

Shared color system and CSS variables. Must match inspectah refine UI palette.

```javascript
// Color system from design spec § 4.2
export const colors = {
  green:  { fill: 'rgba(34,197,94,0.12)',  stroke: '#22c55e',  text: '#86efac' },
  teal:   { fill: 'rgba(45,212,191,0.12)', stroke: '#2dd4bf',  text: '#5eead4' },
  blue:   { fill: 'rgba(96,165,250,0.12)', stroke: '#60a5fa',  text: '#93c5fd' },
  purple: { fill: 'rgba(192,132,252,0.12)',stroke: '#c084fc',  text: '#d8b4fe' },
  amber:  { fill: 'rgba(245,158,11,0.12)', stroke: '#f59e0b',  text: '#fde68a' },
  rose:   { fill: 'rgba(244,114,182,0.12)',stroke: '#f472b6',  text: '#f9a8d4' },
  red:    { fill: 'rgba(239,68,68,0.15)',  stroke: '#ef4444',  text: '#fca5a5' },
  orange: { fill: 'rgba(249,115,22,0.12)', stroke: '#f97316',  text: '#fdba74' },
};

export const bg = '#0f1729';
export const surface = '#182038';
export const border = '#2a3a5c';
export const text = '#e0e6f0';
export const textDim = '#8899bb';
```

Also export: CSS injection function for standalone pages, shared SVG filter definitions (glow, arrowheads).

- [ ] **Step 2: Write interactions.js**

Shared zoom/pan setup, tooltip behavior, click-to-expand/collapse with animated transitions.

Key exports:
- `setupZoom(svg, g)` — D3 zoom behavior with scale extent [0.3, 3]
- `setupTooltip(container)` — creates tooltip div, returns show/hide/move functions
- `toggleExpand(nodeId, expanded, renderFn)` — manages expand state and re-renders
- `centerView(svg, zoom, positions, nodeW, nodeH)` — auto-center on content

- [ ] **Step 3: Write accessibility.js**

Keyboard navigation and ARIA support per design spec § 4.3.

Key exports:
- `setupKeyboardNav(nodes)` — Tab between nodes, Enter/Space to expand, Escape to close, Arrow keys for spatial nav
- `setupFocusManagement(container)` — focus trap in expanded content, focus restoration on collapse
- `checkReducedMotion()` — returns boolean, all animations should check this
- `ariaAttributes(node, isExpandable, isExpanded)` — returns attribute object

- [ ] **Step 4: Write embed.js**

Iframe detection and fullscreen behavior per design spec § 4.1.

Key exports:
- `isEmbedded()` — `window.self !== window.top`
- `setupFullscreen(buttonSelector, fallbackUrl)` — Fullscreen API with window.open fallback
- `setupPreviewMode(renderFn)` — simplified rendering for iframe context
- `notifyParent(event)` — postMessage to parent page for focus restoration

- [ ] **Step 5: Build the standalone diagram shell**

Every diagram uses a common HTML shell for standalone/fullscreen mode (per design spec § 4.2). Create a shell template or helper function in `embed.js` that renders:

1. **Title overlay** (top-left): `<h1>inspectah</h1>` + `<span>` with diagram subtitle
2. **Legend** (top-right): color-coded dots with labels matching the diagram's color groups
3. **Back-to-docs link** (top-left, below title): `← Back to docs` linking to the parent doc page. Each diagram defines its own `backUrl`.
4. **Exit fullscreen button** (top-right corner): visible labeled button "Exit fullscreen (Esc)", keyboard-accessible, fires `document.exitFullscreen()`
5. **Hint bar** (bottom-center): "Click a node to expand details. Scroll to zoom. Drag to pan."

The shell also injects:
- The shared CSS variables from `theme.js` (background, surface, border, text colors)
- The SVG filter definitions (glow, arrowhead markers)
- The `prefers-reduced-motion` media query listener

Export a function like `createDiagramShell({ title, subtitle, backUrl, legendItems })` that sets up all of this and returns the `<svg>` element and zoom-enabled `<g>` group for the diagram to render into.

All 6 diagram tasks (17-22) **must** use this shell. Do not duplicate the title/legend/back-link/exit markup per diagram.

- [ ] **Step 6: Test shared utilities load correctly**

Create a minimal test HTML page that imports all four modules:

```html
<!DOCTYPE html>
<html><head><script type="module">
import { colors } from './shared/theme.js';
import { setupZoom } from './shared/interactions.js';
import { checkReducedMotion } from './shared/accessibility.js';
import { isEmbedded } from './shared/embed.js';
console.log('All imports OK', { colors, isEmbedded: isEmbedded() });
document.body.textContent = 'Shared utilities loaded successfully';
</script></head><body>Loading...</body></html>
```

Open in browser. Console should show "All imports OK" with no errors.

- [ ] **Step 6: Commit**

```bash
git add docs/diagrams/shared/
git commit -m "docs: add shared D3 diagram utilities

Theme, interactions, accessibility, embed detection.
Unified visual language for all 6 diagrams."
```

### Task 17: Build conceptual pipeline diagram

**Files:**
- Create: `docs/diagrams/conceptual-pipeline.html`

Invoke **ui-ux-pro-max** skill.

- [ ] **Step 1: Build the diagram**

Standalone HTML page using D3 v7 (CDN) + shared utilities. **Must use the diagram shell from Task 16 Step 5** — call `createDiagramShell()` with this diagram's title, subtitle, back URL, and legend items. Do not duplicate title/legend/back-link/exit markup. This applies to all diagram tasks (17-22).

Stages (derive labels from source):
- Host Input (green) — RHEL/CentOS/Fedora host
- Preflight (teal) — expands: podman, root, registry checks
- Inspectors (teal) — expands: list all registered inspectors from `inspectah-cli/src/commands/scan.rs`
- Snapshot (blue, glow) — expands: JSON schema version, section names from `inspectah-core/src/snapshot.rs`
- Redaction (red, glow) — "Sensitive Data Handling." Expands: what gets masked, operator review, `secrets-review.md`
- Renderers (blue) — expands: renderer list from `inspectah-pipeline/src/render/mod.rs`
- Tarball (orange) — output artifact

**No build step.** Pipeline ends at tarball.

Interactions: click-to-expand nodes, hover tooltips, animated flow dots between stages, zoom/pan in standalone.

- [ ] **Step 2: Test in browser — functional and accessibility**

Open `docs/diagrams/conceptual-pipeline.html` directly. Verify:
- All stages render with correct colors per theme.js
- Click-to-expand works on nodes with children
- Tooltips show on hover
- Zoom/pan works

**Accessibility acceptance checklist (apply to ALL diagram tasks 17-22):**
- [ ] Tab moves focus between nodes in reading order; visible 2px focus ring on each
- [ ] Enter/Space expands/collapses focused node
- [ ] Escape closes expanded content or exits fullscreen
- [ ] `prefers-reduced-motion: reduce` → no flow dots, instant transitions, no glow pulse
- [ ] Every expandable node has `role="button"` and `aria-expanded="true/false"`
- [ ] Tooltip content available via `aria-describedby`, not hover-only
- [ ] All SVG groups have `<title>` and `<desc>` tags
- [ ] Standalone page has `<title>` matching diagram name

- [ ] **Step 3: Commit**

```bash
git add docs/diagrams/conceptual-pipeline.html
git commit -m "docs: add conceptual pipeline D3 diagram

Interactive pipeline: host → inspect → redact → render → tarball.
Click-to-expand detail at every stage."
```

### Task 18: Build user flow diagram

**Files:**
- Create: `docs/diagrams/user-flow.html`

- [ ] **Step 1: Build the diagram**

User journey stages (amber):
- Discover → Install → First Scan → Understand Output → Refine

Branch point:
- Single Host Path: Refine → Migration Artifacts (ends here, note about manual build)
- Fleet Path: Fleet init → Aggregate → Fleet Refine (iterate loop)
- **Future direction (dashed, dimmed):** Architect → Image composition plan. Labeled "planned — not yet in the CLI."

Each stage expands with: CLI command, what it produces, common issues, link to doc page.

- [ ] **Step 2: Test in browser — verify Architect is visually distinct**

Architect node must have: dashed border, ~40% opacity, "planned" label. Must NOT look like a current feature.

- [ ] **Step 3: Commit**

```bash
git add docs/diagrams/user-flow.html
git commit -m "docs: add user flow D3 diagram

Discovery to daily use journey. Architect shown as
future-direction (dashed/dimmed)."
```

### Task 19: Build triage decision tree diagram

**Files:**
- Create: `docs/diagrams/triage-decision-tree.html`

- [ ] **Step 1: Build the diagram**

Two-layer layout:

Top layer — single-host classifications:
- Found Item → Baseline (green) / Site (amber) / Investigate (red)
- Each expands with criteria, examples, Containerfile action

Bottom layer — fleet consensus:
- Universal (green) / Partial (blue) / Divergent (orange) / Investigate (red)
- Each expands with definition and concrete example

**Critical:** Partial vs. Divergent must be visually unambiguous:
- Partial: icon/visual showing items present on some hosts, absent on others
- Divergent: icon/visual showing same item with different config content

- [ ] **Step 2: Test Partial/Divergent clarity**

Show to a colleague or review yourself: can you tell the difference between Partial and Divergent from the visual alone, without reading labels? If not, iterate.

- [ ] **Step 3: Commit**

```bash
git add docs/diagrams/triage-decision-tree.html
git commit -m "docs: add triage decision tree D3 diagram

Single-host and fleet consensus classifications.
Clear Partial vs Divergent visual distinction."
```

### Task 20: Build fleet topology diagram

**Files:**
- Create: `docs/diagrams/fleet-topology.html`

- [ ] **Step 1: Build the diagram**

Layout:
- Host nodes (blue) at top — 3-4 example hosts with OS version labels
- Fleet Aggregate (rose) — expands: consensus matrix
- Fleet Refine (rose) — iterate loop (animated arc)
- Fleet artifacts (orange) — per-host Containerfiles, fleet reports
- **Future direction (dashed, dimmed):** Architect → Base/App/Role layers. Labeled "planned — build-time image composition, not runtime layers."

- [ ] **Step 2: Verify Architect framing**

Architect node: dashed border, dimmed, "planned" label, explicit "build-time image composition — not runtime layer boundaries" in tooltip.

- [ ] **Step 3: Commit**

```bash
git add docs/diagrams/fleet-topology.html
git commit -m "docs: add fleet topology D3 diagram

Host scan → aggregate → refine → artifacts.
Architect shown as future-direction."
```

### Task 21: Build software architecture diagram

**Files:**
- Create: `docs/diagrams/software-architecture.html`

- [ ] **Step 1: Derive crate structure from source**

```bash
# Get crate names and dependencies
cat Cargo.toml
for crate in inspectah-cli inspectah-web inspectah-pipeline inspectah-refine inspectah-collect inspectah-core; do
  echo "=== $crate ==="
  grep -A 20 '\[dependencies\]' $crate/Cargo.toml | head -25
done
```

- [ ] **Step 2: Build the diagram**

Crate dependency graph. Tiers derived from actual Cargo.toml:
- Entry points (binary/app crates)
- Orchestration (pipeline, refine)
- Collection + foundation (collect, core — core gets glow)

Expansion at `crate/module/contract` level — three layers:
1. **Crate** — name, one-line purpose, which tier it occupies
2. **Module** — name, responsibility description
3. **Contract** — key public trait or responsibility boundary (e.g., "Inspector trait — each inspector implements this to produce a typed snapshot section")

Each crate node expands to show:
- Module list with one-line descriptions (from `src/` directory)
- Key modules expand one more level to show their public contract/ownership boundary
- Core `types/` → domain type module names with ownership notes (e.g., "redaction — RedactionHint, redaction policy types")
- Collect `inspectors/` → inspector module names with which snapshot section each populates
- Pipeline → redaction step in pipeline flow, renderer chain, render→tarball contract
- Dependency arrows highlight on hover

**Not in v1:** individual struct field listings, function signatures, internal implementation details. Those require generation tooling or will rot.

- [ ] **Step 3: Commit**

```bash
git add docs/diagrams/software-architecture.html
git commit -m "docs: add software architecture D3 diagram

Crate dependency graph with module-level expansion.
Derived from Cargo.toml and source structure."
```

### Task 22: Build data flow diagram

**Files:**
- Create: `docs/diagrams/data-flow.html`

- [ ] **Step 1: Build the diagram**

Flow:
- Host Filesystems (green) — expands: categories of data read
- Inspectors (teal) — expands: per-inspector summary
- InspectionSnapshot (blue, glow) — expands: schema version, section names, purpose
- Redact (red, glow) — "Sensitive Data Handling." Expands: masking rules, review boundary
- Renderers (purple) — expands: per-renderer summary
- Tarball (orange) — expands: output directory tree, always-written vs conditional

All content derived from source. Distinguish `scan`/`fleet aggregate` broad artifact set from `refine` export narrower contract.

- [ ] **Step 2: Commit**

```bash
git add docs/diagrams/data-flow.html
git commit -m "docs: add data flow D3 diagram

Snapshot data through the pipeline. Per-command artifact
contracts distinguished."
```

### Task 23: Embed diagrams in parent doc pages

**Files:**
- Modify: `docs/getting-started.md` (conceptual-pipeline, user-flow)
- Modify: `docs/index.md` (user-flow)
- Modify: `docs/reference/triage-classification.md` (triage-decision-tree)
- Modify: `docs/explanation/migration-model.md` (conceptual-pipeline)
- Modify: `docs/explanation/triage-philosophy.md` (triage-decision-tree)
- Modify: `docs/how-to/fleet-aggregation.md` (fleet-topology)
- Modify: `docs/explanation/fleet-consensus.md` (fleet-topology) — if written by this point
- Modify: `docs/explanation/architecture.md` (software-architecture, data-flow) — if written

- [ ] **Step 1: Add embed markup to each parent page**

For each page, add the iframe + button + text summary pattern. **Important:** pages with multiple diagrams need per-iframe bindings, not `document.querySelector('iframe')`.

Each diagram embed block uses a unique `id` on both the iframe and its button. The button's `onclick` stores itself as the focus-return target, enters fullscreen, and a `fullscreenchange` listener restores focus when fullscreen exits:

```html
<div class="diagram-embed" style="margin: 2em 0;">
  <iframe id="diagram-conceptual-pipeline"
          src="../diagrams/conceptual-pipeline.html"
          title="Conceptual Pipeline — interactive preview"
          width="100%" height="450" frameborder="0"
          loading="lazy" tabindex="0"></iframe>
  <div style="margin-top: 0.5em;">
    <button id="btn-diagram-conceptual-pipeline"
            onclick="(function(btn){
      var iframe = document.getElementById('diagram-conceptual-pipeline');
      if (iframe.requestFullscreen) {
        iframe.requestFullscreen();
        // Store the triggering button for focus restoration
        iframe._triggerBtn = btn;
        // One-shot listener: restore focus to THIS button when fullscreen exits
        document.addEventListener('fullscreenchange', function handler() {
          if (!document.fullscreenElement) {
            document.removeEventListener('fullscreenchange', handler);
            if (iframe._triggerBtn) {
              iframe._triggerBtn.focus();
              iframe._triggerBtn = null;
            }
          }
        });
      } else {
        window.open(iframe.src, '_blank');
      }
    })(this)"
            aria-label="Open conceptual pipeline diagram in fullscreen">
      Open interactive diagram
    </button>
  </div>
  <p><em>This diagram shows the inspectah pipeline from host scan through artifact generation. Click "Open interactive diagram" for zoom, pan, and click-to-expand detail.</em></p>
</div>
```

**Parent-side focus contract:** Each button passes `this` into its handler, which stores it on the iframe element. A one-shot `fullscreenchange` listener fires when fullscreen exits and calls `.focus()` on the stored button reference. This works correctly on pages with multiple diagram embeds because each button/iframe pair maintains its own reference — no shared global state. The `window.open` fallback path does not need focus restoration (new tab, user navigates back manually).

**Full approved embed map** (from design spec):

| Parent Page | Diagram |
|-------------|---------|
| getting-started.md | conceptual-pipeline, user-flow |
| index.md | user-flow |
| reference/triage-classification.md | triage-decision-tree |
| explanation/migration-model.md | conceptual-pipeline |
| explanation/triage-philosophy.md | triage-decision-tree |
| how-to/fleet-aggregation.md | fleet-topology |
| explanation/fleet-consensus.md | fleet-topology |
| explanation/architecture.md | software-architecture, data-flow |
| contributing/developer-guide.md | software-architecture |
| reference/snapshot-schema.md | data-flow |

- [ ] **Step 2: Test all embeds render correctly**

```bash
cd docs && bundle exec jekyll serve
```

Open each page. Verify:
- Iframe shows diagram preview at correct height
- "Open interactive diagram" button works (fullscreen or new tab)
- Text summary is readable below each embed

- [ ] **Step 3: Commit**

```bash
git add docs/
git commit -m "docs: embed D3 diagrams in parent doc pages

Iframe preview + fullscreen button + text summary
for each diagram/page pair."
```

## Phase 4: Depth + Contributing

### Task 24: Write fleet consensus explanation

**Files:**
- Create: `docs/explanation/fleet-consensus.md`

Invoke **Diataxis** skill. **Explanation.**

- [ ] **Step 1: Write the explanation**

```yaml
---
title: Fleet Consensus
parent: Explanation
nav_order: 4
---
```

Content: how fleet aggregation works conceptually. Why consensus matters. How individual host classifications combine into fleet-level categories. The meaning of Universal/Partial/Divergent in practice. Why config differences are the real signal (not package version differences).

**Embed:** Include `fleet-topology.html` diagram iframe with fullscreen button and text summary.

Sources: `inspectah-refine/src/`, `inspectah-core/src/types/fleet.rs` or equivalent.

- [ ] **Step 2: Commit**

```bash
git add docs/explanation/fleet-consensus.md
git commit -m "docs: add fleet consensus explanation"
```

### Task 25: Write architecture explanation

**Files:**
- Create: `docs/explanation/architecture.md`

Invoke **Diataxis** skill. **Explanation.** Embed software-architecture and data-flow diagrams.

- [ ] **Step 1: Write the explanation**

```yaml
---
title: Architecture
parent: Explanation
nav_order: 1
---
```

Content: Rust workspace structure, crate responsibilities, data flow from scan to artifact. Embed the software-architecture and data-flow diagrams. Written narrative explains the "why" of the architecture; diagrams show the "what."

Sources: `Cargo.toml`, each crate's `src/lib.rs` or `src/main.rs`, existing `docs/explanation/architecture.md` for conceptual content (adapt, don't copy — it references Go/Python internals).

- [ ] **Step 2: Commit**

```bash
git add docs/explanation/architecture.md
git commit -m "docs: add architecture explanation with embedded diagrams"
```

### Task 26: Write remaining reference pages

**Files:**
- Create: `docs/reference/snapshot-schema.md`
- Create: `docs/reference/inspector-coverage.md`
- Create: `docs/reference/fleet-manifest.md`
- Create: `docs/reference/configuration.md`

Invoke **Diataxis** skill. All **reference** pages.

- [ ] **Step 1: Write snapshot-schema.md**

JSON schema reference. Derive from `inspectah-core/src/snapshot.rs`. Document each section, its type, and what populates it. **Embed:** Include `data-flow.html` diagram iframe with fullscreen button and text summary.

- [ ] **Step 2: Write inspector-coverage.md**

What each inspector scans. Derive inspector list from `inspectah-cli/src/commands/scan.rs` and implementation files in `inspectah-collect/src/inspectors/`. For each: what data sources it reads, what section it populates, what it reports.

- [ ] **Step 3: Write fleet-manifest.md**

TOML manifest format. Derive from `inspectah fleet init` output and manifest parsing code.

- [ ] **Step 4: Write configuration.md**

First check what config surface actually exists in the Rust CLI:

```bash
grep -rn 'env\|config\|INSPECTAH' inspectah-cli/src/ | grep -v test | grep -v target
```

If the Rust branch only has environment variables and defaults (no user config file system), say so directly and scope the page to what exists: env vars, default values, CLI flag overrides. Do not document a config file format that doesn't exist. If there is no meaningful config surface beyond CLI flags, this page may reduce to a short section in the CLI reference rather than a standalone page.

- [ ] **Step 5: Add front matter to all four pages**

Each page needs `parent: Reference` and appropriate `nav_order`.

- [ ] **Step 6: Commit**

```bash
git add docs/reference/
git commit -m "docs: add remaining reference pages

Snapshot schema, inspector coverage, fleet manifest,
configuration. All derived from Rust source."
```

### Task 27: Write remaining how-to guides

**Files:**
- Create: `docs/how-to/baseline-subtraction.md`
- Create: `docs/how-to/customize-output.md`
- Create: `docs/how-to/ci-integration.md`

Invoke **Diataxis** skill. All **how-to guides.**

- [ ] **Step 1: Write baseline-subtraction.md**

Using `--base-image` to filter known-good content. Derive from `inspectah-core/src/baseline.rs` and `scan --help`.

- [ ] **Step 2: Write customize-output.md**

Progress modes (`--progress rich|plain|flat`), verbosity (`--verbose`, `--quiet`), redaction flags (`--preserve-password-hashes`, `--preserve-ssh-keys`, `--acknowledge-sensitive`). Frame redaction as "sensitive data handling" per § 4.4.

- [ ] **Step 3: Write ci-integration.md**

Running inspectah in CI pipelines. Non-interactive mode, machine-readable output, exit codes. Derive from CLI help and source.

- [ ] **Step 4: Add front matter to all three pages**

Each page needs `parent: How-To Guides` and appropriate `nav_order`.

- [ ] **Step 5: Commit**

```bash
git add docs/how-to/
git commit -m "docs: add remaining how-to guides

Baseline subtraction, output customization, CI integration."
```

### Task 28: Write contributing docs

**Files:**
- Create: `docs/contributing/developer-guide.md`
- Create: `docs/contributing/adding-an-inspector.md`

Invoke **Diataxis** skill. These are **how-to guides** for contributors.

- [ ] **Step 1: Write developer-guide.md**

```yaml
---
title: Developer Guide
parent: Contributing
nav_order: 1
---
```

Content: building from source (`cargo build`), running tests (`cargo test`), workspace structure, PR process. Derive from actual build/test commands and Cargo workspace config. **Embed:** Include `software-architecture.html` diagram iframe with fullscreen button and text summary.

- [ ] **Step 2: Write adding-an-inspector.md**

```yaml
---
title: Adding an Inspector
parent: Contributing
nav_order: 2
---
```

Content: step-by-step for adding a new inspector module. Where to create the file, the Inspector trait to implement, how to register it, how to add types, how to test it. Derive from existing inspector implementations in `inspectah-collect/src/inspectors/`.

- [ ] **Step 3: Commit**

```bash
git add docs/contributing/
git commit -m "docs: add contributing docs

Developer guide and adding-an-inspector walkthrough.
Covers Rust workspace build, test, and contribution flow."
```

### Task 29: Write first-migration tutorial

**Files:**
- Create: `docs/tutorials/first-migration.md`

Invoke **Diataxis** skill. This is a **tutorial** — end-to-end guided walkthrough.

- [ ] **Step 1: Write the tutorial**

```yaml
---
title: Your First Migration
parent: Tutorials
nav_order: 1
---
```

Content: complete end-to-end single-host migration walkthrough. Starts from a package-mode host, ends with a generated Containerfile ready for `podman build`. Covers: scan, understand output, open refine, triage decisions, export. Does NOT include build step (not in Rust). Links to `how-to/build-bootc-image.md` for that.

- [ ] **Step 2: Commit**

```bash
git add docs/tutorials/first-migration.md
git commit -m "docs: add first-migration tutorial

End-to-end walkthrough: scan → understand → refine → export."
```

### Task 30: Final validation

- [ ] **Step 1: Full Jekyll build**

```bash
cd docs && bundle exec jekyll build
```

Expected: no errors, no warnings about broken links.

- [ ] **Step 2: Local serve and manual smoke test**

```bash
cd docs && bundle exec jekyll serve
```

Check:
- Landing page loads with working navigation
- All nav categories populated (Getting Started, Tutorials, How-To, Reference, Explanation, Contributing)
- All diagram iframes render previews
- Fullscreen buttons work on all diagrams
- Search works (type "triage" → finds classification pages)
- Dark theme renders correctly
- No internal engineering artifacts visible in nav or search

- [ ] **Step 3: Link audit**

```bash
# Check for broken internal links (portable — no GNU grep -oP)
find docs -name '*.md' -not -path 'docs/_site/*' | while read mdfile; do
  grep -on '\]([^)]*\.md[^)]*)' "$mdfile" | while IFS=: read lineno match; do
    link=$(echo "$match" | sed 's/.*](\(.*\))/\1/' | sed 's/#.*//')
    dir=$(dirname "$mdfile")
    target="$dir/$link"
    if [ ! -f "$target" ]; then
      echo "BROKEN: $mdfile:$lineno → $link"
    fi
  done
done
```

- [ ] **Step 4: Verify no Go/Python references in user-facing docs**

```bash
grep -rn -i 'python\|\.py\|go run\|go build\|main\.go\|container.*image.*pull' docs/ \
  --include='*.md' | grep -v '_site/' | grep -v 'contributing/'
```

Expected: no matches (contributing/ may mention language tooling, that's fine).

- [ ] **Step 5: Semantic leak checks**

Scan for stray references to deferred or incorrectly framed features outside their approved contexts:

```bash
echo "=== inspectah build outside approved contexts ==="
grep -rn 'inspectah build\|inspectah.*build subcommand' docs/ --include='*.md' \
  | grep -v '_site/' | grep -v 'build-bootc-image.md'

echo "=== inspectah architect outside future-state framing ==="
grep -rn -i 'inspectah architect\|architect subcommand' docs/ --include='*.md' \
  | grep -v '_site/'

echo "=== runtime layer/strata phrasing (should be build-time composition) ==="
grep -rn -i 'runtime layer\|runtime strat\|runtime boundar\|managed.*layer' docs/ \
  --include='*.md' | grep -v '_site/'

echo "=== trust signal phrasing (should be sensitive data handling) ==="
grep -rn -i 'trust signal\|trust seal\|security trust' docs/ \
  --include='*.md' --include='*.html' | grep -v '_site/'
```

Expected: no matches in any category. If matches found, verify they are in approved future-state or manual-build contexts.

- [ ] **Step 6: Diagram accessibility QA**

Open each of the 6 diagrams in a browser and verify the accessibility acceptance checklist:

| Check | How to verify |
|-------|---------------|
| Keyboard navigation | Tab through all nodes; verify visible focus ring (2px, high contrast) on each |
| Expand/collapse | Focus a node, press Enter or Space; content expands. Press Escape; it collapses. |
| Fullscreen exit | Press Escape in fullscreen; verify it exits and focus returns to the embed button |
| Reduced motion | Set `prefers-reduced-motion: reduce` in browser dev tools; reload. No flow dots, no animated transitions, no glow pulse. |
| ARIA attributes | In dev tools, inspect expandable nodes: `role="button"`, `aria-expanded="true/false"` present |
| SVG accessibility | Inspect SVG groups: `<title>` and `<desc>` tags present on all major node groups |
| Screen reader test | (If available) Navigate with VoiceOver/NVDA; node names and states announced |
| Diagram shell | Title overlay, legend, "← Back to docs" link, "Exit fullscreen (Esc)" button, hint bar all present and functional |
| Back link | Click "← Back to docs" — navigates to the correct parent doc page for each diagram |

All 6 diagrams must pass all checks before the plan is complete.

- [ ] **Step 7: Publish surface audit**

```bash
echo "=== Directories in docs/ ==="
find docs/ -type d -not -path 'docs/_site/*' -not -path 'docs/.jekyll*' | sort

echo "=== Non-user-facing files that should not be published ==="
find docs/ -type f -not -path 'docs/_site/*' -not -path 'docs/.jekyll*' \
  | grep -i 'roadmap\|nit-list\|nits-\|future-\|RELEASE-\|backlog\|specs/\|plans/\|designs/\|superpowers/' \
  || echo "CLEAN — no internal artifacts in docs/"
```

Expected: "CLEAN" — no internal engineering artifacts remain in the publish surface.

- [ ] **Step 8: Commit any fixups**

```bash
git add -A
git commit -m "docs: final validation fixups"
```
