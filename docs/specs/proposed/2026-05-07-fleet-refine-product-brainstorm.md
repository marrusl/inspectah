# Fleet Refine Product Strategy Assessment

**Author:** Ember (Shadow PM)
**Date:** 2026-05-07
**Status:** Brainstorm — Mark decisions captured 2026-05-07

---

## Executive Summary

Mark's instincts here are largely right. The prevalence heuristic simplification is the correct call. The config variant problem is the hardest UX challenge in this entire product. The file editor consolidation is directionally right but needs a more nuanced approach than a straight modal swap. And the competitive framing should lean hard into "fleet-level migration intelligence" — because nobody else is even attempting this.

Here's my assessment of each idea, with pushback where I disagree.

---

## 1. Prevalence Heuristic Simplification

**Mark's proposal:** Drop the user-facing threshold slider. Replace with: everything selected by default, items with <100% prevalence flagged as "review needed."

**My position: Strongly agree, with one refinement.**

The slider is a vestige of thinking about prevalence as a filter. But that's not how enterprise sysadmins think. They think: "What's standard across my fleet, and where does it diverge?" That's a sort/group operation, not a filter.

The current slider creates two problems:
1. **Analysis paralysis.** What should I set it to? 66%? 80%? The "right" answer depends on the specific item, not a global threshold. A package at 48/50 hosts is obviously standard. A config file at 25/50 hosts might be a regional variant — equally valid but for different servers.
2. **Hidden state.** Items below the threshold disappear from view. In a migration tool, hiding data is dangerous. The sysadmin needs to see everything and make explicit decisions.

**The refinement:** Don't just bucket into "included" vs. "review needed." Create three visual tiers:

| Tier | Criteria | Default State | Visual Treatment |
|------|----------|---------------|------------------|
| **Universal** | 100% prevalence | Included, collapsed | Clean/green — no action needed |
| **Dominant** | >50% but <100% | Included, visible | Subtle attention marker — "on 47/50 hosts" with prevalence bar |
| **Minority** | <=50% | Included but flagged | Review-needed badge — "on 3/50 hosts — is this intentional?" |

Everything starts selected. Nothing is hidden. The tiers are a visual hierarchy that tells the sysadmin where to focus their attention without requiring them to configure anything.

**Why this is strategically right:** The slider was a power-user feature that actually made the tool harder for everyone. Enterprise migration tools need opinionated defaults with escape hatches, not neutral toolboxes. Nobody building a golden image from 50 servers wants to fiddle with a global percentage — they want the tool to say "here's the standard set, and here are the things you need to decide about."

**What to keep from the current prevalence data:** The `FleetPrevalence` struct (`Count`, `Total`, `Hosts`) is excellent. The merge logic in `mergeIdentityItems` and `mergeContentItems` already does the right thing — tracking which hosts have each item and variant. The only change is how the UI presents this data: from "filter by threshold" to "sort and group by prevalence tier."

**Backward compatibility:** The `--min-prevalence` CLI flag can stay for programmatic use. The refine UI just stops exposing it as a slider. Default it to 0 (everything included) in fleet merge, and let the UI tier rendering handle the visual prioritization.

---

## 2. Competitive Positioning

**No one else does this.** That's not hyperbole — I've tracked this space extensively. The competitive landscape breaks down as:

| Tool | What it does | Fleet capability |
|------|-------------|-----------------|
| **Forklift (RH)** | VM-level migration | None — lifts whole VMs |
| **Migration Toolkit for Apps** | Java/container app migration | None — app-level only |
| **AWS Migration Hub** | Portfolio-level tracking | Discovery, not analysis |
| **SUSE distro-migration** | Distro upgrades | Single-system only |
| **Various CM tools** (Ansible, Puppet) | Config management | They define state, they don't analyze divergence |

inspectah fleet mode occupies a genuinely novel position: **fleet-level migration analysis with item-level granularity.** The closest analogy isn't a migration tool — it's what Ansible's `ansible-cmdb` or Puppet's `puppet-report` do for understanding fleet state, but applied to the specific problem of "what should go into my golden container image?"

**How to frame it:**

> "inspectah doesn't just tell you what's on your servers — it tells you what your servers agree on, where they diverge, and helps you make explicit decisions about what goes into your golden image."

The key differentiator phrases:
- **"Prevalence-aware migration"** — the tool understands fleet consensus
- **"Divergence visibility"** — conflicts aren't hidden, they're surfaced with context
- **"Golden image synthesis"** — the output is a production Containerfile, not a report

**Strategic implication:** Fleet mode is the enterprise sell. Single-machine mode is the POC that gets the sysadmin hooked. The competitive moat is in the fleet experience — no one else can even compare to an MVP here because they're not collecting the right data at the right granularity.

---

## 3. The "Why" Transparency

**Mark's proposal:** Make preselection reasoning transparent — explain WHY something is defaulted on or off.

**My position: Yes, but calibrate for the audience.**

The triage system already has `Reason` fields — `"Service state changed (enabled -> disabled)."`, `"System user (UID < 1000), matches base."`, etc. These are good. They're terse, factual, and tell the sysadmin exactly what the tool detected.

**What sysadmins actually want:**

1. **Data, not rationale.** Don't explain *why* the tool thinks something matters. Show the data that led to the classification. "On 3/50 hosts (web-prod-01, web-prod-02, web-staging-01)" is more useful than "This item has low prevalence and may represent a host-specific customization."

2. **Contextual hints, not docs.** For fleet mode specifically, the most useful transparency is:
   - Which hosts have this item (already in `FleetPrevalence.Hosts`)
   - Whether it's a package, config, service, etc. (already in `Section`)
   - Whether the item has variants across hosts (already tracked by `mergeContentItems`)
   - Whether it conflicts with base image content (already in `State` for packages)

3. **Progressive disclosure.** Default view shows prevalence count and tier badge. Click/expand shows host list. For config variants, expanding shows the variant count and compare action. No one needs a paragraph of explanation on first contact.

**Where I'd push back:** Don't add explanatory text like "This package is included because it appears on all hosts." Sysadmins can read "50/50 hosts" and draw that conclusion instantly. Over-explaining insults the user's intelligence. The triage `Reason` field should stay factual and terse.

**One exception:** For items the tool *excludes* by default (secrets, base-image-only packages), a brief reason is appropriate because the user didn't make that choice. "Excluded: detected as credential material" or "Display only: package exists in base image" are the right level.

---

## 4. Config Variant Handling

**Mark's proposal:** Compare file variants across hosts and pick which one, without going to File Editor. Group conflicting config files.

**My position: This is the hardest UX problem in the product, and the current approach is 80% right.**

Let me break down what actually happens when a sysadmin faces config variants:

### The Decision Matrix

When `/etc/nginx/nginx.conf` has 3 variants across 50 hosts, the sysadmin's actual decision is one of:

| Scenario | Action | Frequency |
|----------|--------|-----------|
| One variant is clearly "right" (e.g., latest version, most hosts) | Pick it | ~60% of cases |
| Variants differ in a few lines (e.g., different upstream servers) | Pick one, edit to generalize | ~25% of cases |
| Variants are fundamentally different (e.g., different architectures) | Write new config for image mode | ~10% of cases |
| Config is host-specific and shouldn't be in the image at all | Exclude the file entirely | ~5% of cases |

### What Already Works

The variant auto-selection spec (proposed `2026-03-22`) already handles the 60% case correctly: auto-select the most prevalent variant, surface ties. The fleet variant comparison spec (implemented `2026-03-16`) provides inline diff for comparing variants. These are good.

### What's Missing

**The 25% case — "pick one and tweak."** This is where Mark's instinct about the File Editor matters. The current flow is:

1. See variants on config tab
2. Compare them (modal diff)
3. Pick one (radio selection)
4. Navigate to File Editor tab
5. Find the file in the editor tree
6. Edit it

Steps 4-6 are friction. The user already has context from the comparison — they know what they want to change. Making them context-switch to a separate tab and re-find the file breaks their flow.

**The 10% case — "write a new config."** This is genuinely hard and probably out of scope for the near term. But the tool should at least acknowledge it: when a user is staring at 3 fundamentally different variants of a config file, the right UX might be "none of the above — I'll provide a new file" as an explicit option.

### My Recommendation

**Surface variant conflicts directly in the config section, not just in the editor.** The config tab should show:

1. **Uniform configs** (1 variant) — standard row, just include/exclude
2. **Auto-resolved configs** (1 dominant variant) — standard row with subtle "(selected: variant from web-prod-*)" annotation
3. **Conflicting configs** (tied or near-tied variants) — expanded group showing all variants with:
   - Inline prevalence bars for each variant
   - One-click compare between any two variants
   - "Edit selected" action that opens the editor *for that specific file* (not the editor tab)

The compare-and-decide workflow should live on the config tab. The editor is for the follow-up edit, not for the initial triage.

---

## 5. File Editor Consolidation

**Mark's proposal:** Rethink File Editor as inline modal instead of a separate tab.

**My position: Directionally right, but a modal is the wrong pattern. Use a slide-over drawer.**

### Why the separate tab is problematic

The editor redesign UX analysis (2026-05-03) already catalogs this well. The separate tab creates two navigation problems:

1. **Context loss.** User sees a config variant conflict on the config tab, decides to edit, switches to editor tab, loses sight of the variant context.
2. **Discovery gap.** Users who don't explore the editor tab may not realize they can edit file content at all.

### Why a modal is also wrong

A full-screen or large modal for file editing has its own problems:

1. **No ambient context.** When editing `/etc/nginx/nginx.conf`, you often want to glance at the variant comparison or the prevalence data. A modal blocks that.
2. **Multi-file workflows.** Sometimes the sysadmin needs to edit 3 related configs (nginx.conf, nginx-site.conf, ssl.conf). A modal forces open-close-open-close. A tab or drawer lets them work through a batch.
3. **CodeMirror + modals = accessibility pain.** Focus trapping, keyboard shortcuts, escape key conflicts. The UX analysis already flags this.

### My recommendation: Slide-over drawer

The variant auto-selection spec already proposes a PF6 resizable drawer for the editor tree pane. Extend this pattern:

- **From any section tab,** clicking "Edit" on a config item opens a slide-over drawer (right side, ~50% width) with the CodeMirror editor.
- **The section tab stays visible** behind/beside the drawer. User can see the variant context while editing.
- **The drawer persists** across file selections. Click edit on another file, the drawer updates. No open-close friction.
- **The editor tab becomes optional** — it's still there as a "browse all editable files" view, but most editing happens in-context via the drawer.

This gives you the "inline editing without tab switching" that Mark wants, without the limitations of a modal.

**Caveat:** This is a significant UX change. I'd recommend implementing the drawer for config variant editing first (where the context-loss problem is most acute) and seeing how users respond before migrating all editing to this pattern.

---

## 6. Phasing and MVP

Here's how I'd sequence this work, organized by impact and dependency:

### Phase 1: Foundation (Highest Impact, Lowest Risk)
**Goal: Make fleet refine self-explanatory without requiring any user configuration.**

1. **Prevalence heuristic switch** — Remove slider from UI, default `--min-prevalence` to 0, add three-tier visual hierarchy (Universal/Dominant/Minority). This is primarily a UI/template change; the fleet merge backend already computes everything needed.

2. **Prevalence data surfacing** — Show "X/Y hosts" with inline prevalence bar on every fleet item. Host list on expand/tooltip. This data is already in `FleetPrevalence` — it's a rendering change.

3. **Preselection reasoning** — Add terse reason annotations to triage items. Mostly leveraging existing `Reason` field; fleet items get prevalence-tier context.

**Why this first:** These three changes transform the fleet experience from "configure a threshold then review" to "review what the tool already figured out." It's the biggest UX improvement with the least structural risk. No new components, no architecture changes.

### Phase 2: Variant Decision Flow (Highest Complexity)
**Goal: Make config variant conflicts resolvable without leaving the config section.**

4. **Variant conflict grouping** — Visual hierarchy on config tab: uniform > auto-resolved > conflicting. Tier badges (reuse gold-badge pattern from variant auto-selection spec).

5. **Inline variant compare** — Compare action on config tab (not just editor tab). This exists for the editor already; it's about making it accessible from the config section.

6. **Edit-in-context drawer** — Slide-over drawer for editing a specific file from any section tab. Start with config variants only.

**Why this second:** This is where the real product value emerges — the sysadmin can triage, compare, decide, and edit config variants in a single continuous flow. But it requires the Phase 1 foundation (prevalence tiers, data surfacing) to provide the context that makes variant decisions informed.

### Phase 3: Polish and Consolidation
**Goal: Clean up redundant navigation and edge cases.**

7. **Editor tab role refinement** — The editor tab becomes "browse all editable files" rather than the primary editing surface. Most editing now happens via the drawer from section tabs.

8. **"None of the above" variant option** — For configs where no existing variant is right, allow the user to provide a new file (or mark the file as "will provide separately").

9. **Variant merge hints** — For the 25% case (pick one and tweak), show a simple diff summary: "Variants differ in 3 lines" or "Variants are structurally different." This helps the user decide whether to pick-and-edit or write-from-scratch.

### What Can Wait (Post-MVP)

- **Variant auto-merge suggestions.** The tool could theoretically suggest merged configs for simple cases (e.g., concatenating different upstream server blocks). This is technically interesting but UX-dangerous — auto-merging system configs is high-risk, and sysadmins would rightfully distrust it.

- **Template/parameterization detection.** Identifying that config variants differ only in hostname or IP and suggesting a template approach. Valuable, but a research problem more than a product feature right now.

- **Cross-section variant correlation.** "These 3 hosts have a different nginx.conf AND a different firewall rule AND different SELinux booleans" — likely a coherent server role, not random drift. This is the eventual "migration intelligence" play but requires more data science than UX work.

---

## Strategic "So What"

The fleet refine experience is inspectah's competitive moat and enterprise value proposition. Every improvement here widens the gap with tools that only understand single-system migration.

The prevalence heuristic simplification isn't just a UX improvement — it's a product positioning decision. By removing the threshold slider, we're saying: "This tool is smart enough to classify your fleet data for you. Your job is to review the decisions, not configure the analysis." That's a meaningfully different product than "here's a slider to filter your data."

The config variant problem is the one that will determine whether fleet mode is a novelty or a production workflow. If resolving config conflicts requires 15 clicks across 3 tabs, sysadmins will do it once for a demo and then go back to manually writing their Containerfile. If it's a 3-click flow (see conflict, compare, pick or edit), they'll use it for every migration.

The phasing prioritizes the changes that have the highest impact on first-time fleet experience (Phase 1) before tackling the deeper workflow improvements (Phase 2-3). An impressive MVP is: "load your fleet data, see a clear picture of what's standard and what diverges, resolve the divergences, export your Containerfile." That's Phase 1 + Phase 2.

---

## Mark's Decisions (2026-05-07)

1. **Prevalence threshold:** Default to 100%. Items at 100% prevalence (strict intersection) are "Included" — no review needed. Items below 100% fall into the "Review" tier. An interactive threshold control in the UI lets users lower it and watch items reclassify from "Review" to "Included" in real time. This replaces the old `-p` CLI flag with a more intuitive in-UI control.
2. **Diff implementation:** The Python version's self-contained LCS `lineDiff()` is a proven reference (no external deps, 5000-line cap). Port or rewrite — not bound to it.
3. **Phasing:** Agreed with incremental delivery (Phase 1: prevalence visibility first).
