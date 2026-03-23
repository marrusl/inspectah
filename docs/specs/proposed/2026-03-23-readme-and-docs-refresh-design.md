# README Restructure and Docs Freshness Pass

**Date:** 2026-03-23
**Status:** Proposed

## Problem

The README front-loads architecture and inspector internals before the reader understands what yoinkc does or what they get. The four-step user journey (inspect → refine → build → fleet) is scattered across the document — refine at line 256, build at line 286, fleet at line 309. A sysadmin, enthusiast, or technical architect evaluating bootc migration has to wade through ~100 lines of inspector detail before reaching the workflow sections.

Separately, several docs have fallen out of step with the code: `design.md` references the deleted `run-yoinkc-fleet.sh` script, the Interactive Refinement section predates auto-open browser and several refine fixes, the template partial count is wrong (29, not 23), and the visual consistency spec is partially implemented but still marked "Proposed."

## Scope

Two deliverables:

1. **README restructure** — journey-first reorganization, no user-facing topics/commands/tables removed
2. **Docs freshness pass** — targeted updates to design.md, visual consistency spec, and nits doc

---

## Part 1: README Restructure

### New Section Order

```
1.  What is yoinkc (value prop, 3-4 sentences)
2.  Workflow overview (inspect → refine → build → fleet diagram)
3.  Inspect (current Quick Start, lightly tightened)
4.  Refine (pulled up from current line 256)
5.  Build (pulled up from current line 286)
6.  Fleet (pulled up from current line 309)
7.  Output Artifacts (current content, unchanged)
    --- below the fold ---
8.  CLI Reference (consolidated: inspect + refine + fleet flags)
9.  How It Works (architecture, inspectors, layer ordering, baseline generation)
10. Advanced Usage (running directly, development)
11. License
```

### Section 1: What is yoinkc

New section. 3-4 sentences covering:

- What it does: inspects a running RHEL/CentOS Stream/Fedora host, produces everything needed to rebuild it as a bootc image
- How it works at a high level: figures out what you added to the base OS and generates only the delta
- What you get: Containerfile, config tree, audit report, interactive HTML dashboard
- Who it's for: put a real migration case against bootc, not a toy example

### Section 2: Workflow Overview

New section. A text diagram showing the four steps with one-liner descriptions and link to each section:

```
┌──────────────┐    ┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│  1. Inspect  │───▸│  2. Refine   │───▸│  3. Build    │───▸│  4. Fleet    │
└──────────────┘    └──────────────┘    └──────────────┘    └──────────────┘
 Scan a host,        Edit findings       Build the bootc     Merge multiple
 get a tarball       in the browser      image               hosts into one

 run-yoinkc.sh       run-yoinkc.sh       yoinkc-build        run-yoinkc.sh
                     refine *.tar.gz     *.tar.gz tag        fleet dir/ -p 80
```

Columns are on a 20-char pitch (16-char box + 4-char gap). Each step links to its detailed section below. Goal: within 15 lines the reader knows the tool's shape. The exact formatting is flexible — the key constraint is that all four steps are visible at a glance with their one-liner command.

### Section 3: Inspect

Current Quick Start content with minor tightening:

- The `curl | sudo sh` one-liner
- "That's it" paragraph (tarball contents summary)
- `sudo` caveat and RHEL auth note as callouts
- Env vars table (`YOINKC_IMAGE`, `YOINKC_HOSTNAME`, `YOINKC_DEBUG`)
- "Download the script first" variant for passing flags

No new content; this is a relabel and light edit of the existing Quick Start.

### Section 4: Refine

Pulled up and tightened from current Interactive Refinement section. Keep:

- Two commands: `scp` the tarball, `./run-yoinkc.sh refine hostname-*.tar.gz`
- One sentence: browser auto-opens (updated from current "Open http://localhost:8642" to reflect auto-open behavior from 282370d)
- What you can do: toggle packages/configs/services, click Re-render, download updated tarball
- Direct usage commands (`yoinkc refine --no-browser --port 9000`)

Move to "How It Works" section: detailed UI feature list (toolbar states, standalone mode detection, strategy dropdowns, dirty/clean/standalone state descriptions, leaf package cascade behavior).

### Section 5: Build

Pulled up and tightened from current Building the Image section. Keep:

- One command: `./yoinkc-build hostname-*.tar.gz my-bootc-image:latest`
- RHEL cert handling in one sentence (auto-detected, bind-mounted)
- Push variant: `--push registry.example.com/...`
- Requirements: Python 3.9+, podman or docker

Move to "How It Works": cert search order, `openssl x509 -checkend` validation, non-RHEL host behavior.

### Section 6: Fleet

Pulled up from current Fleet Analysis section. Keep:

- Two-step workflow (run yoinkc per host, collect tarballs, run fleet)
- Prevalence threshold in one sentence
- Container wrapper note (no Python needed)
- Env vars table and flags table (both compact)
- Direct usage commands

Move to "How It Works": fleet HTML report feature details (prevalence bars, popovers, variant groups, fraction/percentage toggle).

### Section 7: Output Artifacts

Current content, unchanged. Positioned after the four journey steps as the natural "what did I just get?" reference.

### Section 8: CLI Reference (consolidated)

Three subsections:

- **`yoinkc` (inspect)** — current Core Options, Target Image, Inspection Options, and Output Options tables (already in README, just relocated)
- **`yoinkc refine`** — flags table: `--no-browser`, `--port` (currently buried in prose)
- **`yoinkc fleet`** — current fleet flags table (already in README, just relocated)

`yoinkc-build` is a standalone companion script, not a subcommand. Its usage is covered in the Build section (Section 5) rather than this CLI Reference — it has only two positional args and two flags, which fit naturally in the prose.

### Section 9: How It Works

All deep material relocated here, in this order:

1. **Architecture** — current text (inspectors/renderers, baseline subtraction)
2. **Inspectors** — all 11 current subsections, unchanged
3. **Containerfile Layer Ordering** — current text, unchanged
4. **Baseline Generation** — current text, unchanged

### Section 10: Advanced Usage

- **Running directly** — current "Running directly (advanced)" section (podman run flags table)
- **Development** — current Development section (`pip install -e .`, pytest)

### Section 11: License

Unchanged.

### What Changes

- **New content:** Value prop paragraph (~4 sentences), workflow diagram (~10 lines)
- **Updated content:** Refine section mentions auto-open browser; CLI Reference consolidated from scattered flag tables
- **Relocated content:** Inspectors, layer ordering, baseline generation move below the fold; detailed UI/cert/fleet descriptions move to How It Works
- **Deleted content:** No user-facing topics, commands, or tables removed. Prose may be tightened (shortened) during relocation, but all substantive information is preserved.

---

## Part 2: Docs Freshness Pass

### 2A: design.md Updates

**Sweep all `run-yoinkc-fleet.sh` and standalone `yoinkc-fleet` references.** The deleted script and old entry point are referenced in multiple locations. All must be updated to reflect the consolidated `run-yoinkc.sh` with subcommand routing:

- **Section 3, Wrapper Scripts** — remove the `run-yoinkc-fleet.sh` subsection entirely. Update the `run-yoinkc.sh` subsection to mention subcommand routing (`run-yoinkc.sh fleet`, `run-yoinkc.sh refine`).
- **Section 3, CLI Reference > `yoinkc-fleet` heading** — rename to `yoinkc fleet` (subcommand, not separate entry point). Update the command syntax from `yoinkc-fleet aggregate <input_dir>` to `yoinkc fleet <input_dir>`.
- **Section 7, Fleet Analysis > Container Wrapper** — heading currently says "Container Wrapper (`run-yoinkc-fleet.sh`)". Change to reference `run-yoinkc.sh fleet`.
- **Section 8, Wrapper Scripts** — currently describes two shell scripts. Rewrite to describe `run-yoinkc.sh` as the single entry point with subcommand detection (`fleet|refine` case statement).
- **Section 8, Image Build** — currently says "For fleet operations, run-yoinkc-fleet.sh overrides the entry point to yoinkc-fleet." Remove or rewrite to reflect that fleet is now `run-yoinkc.sh fleet` routing to `yoinkc fleet`.

**Section 8, Interactive Refinement — add missing behaviors:**
- Auto-open browser on startup (282370d)
- Health endpoint polling instead of bare TCP for server readiness (4650b60)
- `Cache-Control: no-cache, no-store, must-revalidate` headers (067152a)
- Re-render button shows combined edit+toggle counts, e.g. "Re-render (2 edits, 3 toggles)" (b9c1413)

**Update all stale partial counts.** The claim "23 partials" appears twice in design.md:
- **Section 6 > HTML Report Renderer** (currently around line 615) — update count from 23 to 29 and update the list of partials. The 29 includes both content partials (e.g. `_packages.html.j2`) and infrastructure partials (`_css.html.j2`, `_js.html.j2`, `_macros.html.j2`, `_toolbar.html.j2`). New partials since the doc was written: `_banner.html.j2`, `_compare_modal.html.j2`, `_editor.html.j2`, `_editor_js.html.j2`, `_module_streams.html.j2`, `_new_file_modal.html.j2`, `_toolbar.html.j2`, `_version_locks.html.j2`. (Verify exact delta — some may have been splits rather than additions.)
- **Section 10 > Tech Debt** (currently around line 993) — same stale "23 include partials" claim. Update to match.

**Section 10, Future Work & Backlog — audit for implemented items.** Check each item against recent commits and move implemented items out:
- Variant auto-selection: implemented (a71f0d3)
- Companion tools consolidation: implemented (2b2c533, 2da18b9, 3c511d5)
- Any others that landed since the section was last updated

### 2B: Visual Consistency Spec

The spec at `docs/specs/proposed/2026-03-22-visual-consistency-pass-design.md` is partially implemented across three commits:

- `b428ac2` — style(report): apply initial visual consistency pass
- `6f6c78a` — feat(report): streamline config triage and editor actions
- `f530c15` — feat(report): merge repo controls into package dependency tree

Add a status annotation at the top of each Part:

- **Part A (Global Table Spacing):** verify implementation status, annotate
- **Part B (Config Files Tab Cleanup):** B1–B4 implemented via 6f6c78a. B5 (variant row structure) needs verification.
- **Part C (Packages Tab Restructure):** implemented via f530c15
- **Part D (Column Consistency):** verify via b428ac2, annotate
- **Part E (Inline Style Cleanup):** verify via b428ac2, annotate

If all parts are implemented, change top-level status from "Proposed" to "Implemented" and move the file to `docs/specs/implemented/`. The sibling scaffolding files (`2026-03-22-visual-consistency-pass-plan.md` and `2026-03-22-visual-consistency-pass-prompts.md`) should be deleted — they are implementation scaffolding, not durable artifacts, and would otherwise be orphaned in `proposed/` pointing at a file that moved.

If parts remain, change status to "Partially Implemented" and clearly mark which parts are done vs. pending. In that case the sibling files stay in `proposed/` alongside the spec.

### 2C: Nits Doc

`docs/nits-2026-03-16.md` has items resolved by recent commits. Annotate resolved items with the fixing commit:

- **#1 (duplicate re-render buttons):** resolved by b9c1413 (combined edit+toggle counts on single button)
- **#4b (footer height/alignment):** resolved by 6ef98e1 (content-footer gap fix)
- **#6 (column spacing):** likely resolved by b428ac2 (visual consistency pass)
- **#7 (Kernel/Boot card spacing):** likely resolved by b428ac2
- **#8 (packages layout):** resolved by f530c15 (repo controls merged into dependency tree)

Items still requiring verification should be tested before marking resolved.

---

## Out of Scope

- Content changes to the inspector descriptions (they're accurate, just relocated)
- New features or behavior changes
- `future-visual-improvements.md` and `future-inspection-coverage.md` updates (tracking docs, not claiming to describe current behavior)
- Cross-stream targeting spec (paused brainstorming)
- driftify design.md updates (separate project)

## Testing

### Part 1 (README)

- All internal anchor links resolve correctly after section reordering (grep for `](#` and verify each target exists)
- No content from the current README is missing in the new version (diff review — changes should be moves and additions, not deletions)
- The "What is yoinkc" section exists and is non-empty
- The workflow diagram exists and shows all four steps
- The first 40 lines cover: what it is, workflow diagram, and the start of the Inspect section
- CLI Reference tables are complete (all flags from current README present, including refine and fleet flags now consolidated)
- Refine section mentions auto-open browser (not "open http://localhost:8642 in your browser")

### Part 2 (Docs Freshness)

**Stale script/entry-point references (exact commands):**

```bash
# Must return 0 matches:
grep -c 'run-yoinkc-fleet\.sh' design.md

# Must return 0 matches (standalone entry point references).
# Allowed exception: the historical note "were previously separate
# entry points (`yoinkc-fleet`, `yoinkc-refine`)" in Section 3.
grep -n 'yoinkc-fleet' design.md | grep -v 'previously separate entry points'
```

**Partial count (exact command):**

```bash
# The count reported in design.md Section 6 > HTML Report Renderer
# and Section 10 > Tech Debt must both equal this number:
ls src/yoinkc/templates/report/ | wc -l
```

The count includes all partials — content (`_packages.html.j2`) and infrastructure (`_css.html.j2`, `_js.html.j2`, `_macros.html.j2`).

**Other checks:**
- `design.md`: Interactive Refinement section mentions auto-open browser, health polling, Cache-Control, combined re-render counts
- Visual consistency spec: each Part has a status annotation; if moved to `implemented/`, sibling plan/prompts files are deleted
- Nits doc: resolved items annotated with commit hashes
