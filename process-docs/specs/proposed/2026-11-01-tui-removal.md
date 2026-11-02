# TUI Removal — Documentation Pre-Spec

## Metadata

| Field | Value |
|---|---|
| Date | 2026-11-01 |
| Status | Pre-spec |
| Author | Mango (Technical Documentation Writer) |
| Scope | The nine published documentation surfaces only |
| Backlog item | `inspectah-remove-the-tui` (FKA `workflow/backlog/inspectah/open/`) |
| Knowledge record | `topics/inspectah-tui.md` (FKA) |

## Purpose

The backlog item scopes the removal and gives it an 11-step checklist. Step 8
reads: "Docs: the nine published pages... This is Mango's lane, not the
remover's." This document is that lane. It does not redo the decisions the
backlog item already made — the TUI goes, `--tui` vanishes outright, no
deprecation cycle, the two design documents in `process-docs/` stay. It exists
because removing a section from nine published pages, two of them interactive
diagrams, is a documentation design question with several points where more
than one reasonable answer exists. This pre-spec enumerates those points for
Mark to decide. **No documentation has been edited to produce this file.**

Three of Mango's own "ask Mark first" triggers apply across this set:
removing a documentation page's content outright in a way that changes what a
page is about, restructuring a page's information architecture, and changing
the README's structure. That is why this is a pre-spec rather than a round of
edits.

## 1. Already decided, not reopened here

- The TUI is removed. No current use case; restart later if one appears
  (Mark, 2026-11-01).
- `--tui` vanishes outright: no retained flag, no deprecation message,
  no "removed in <version>" hint. `inspectah refine --tui` becomes an
  unrecognised argument.
- No deprecation cycle. The feature shipped marked experimental
  (`README.md:166`), so no migration path is owed.
- The clean break applies to user surfaces only: the CLI, `README.md`,
  everything under `docs/`, and user-visible strings.
  `process-docs/`, git history, and the CHANGELOG's released sections are
  explicitly out of scope and keep their mentions.
- The two TUI design documents in `process-docs/` stay in place, with a
  pointer added to `topics/inspectah-tui.md`. That edit belongs to the code
  removal, not to this pre-spec.

## 2. The code removal is thin, and that is correct, not incomplete

22 files, one workspace member line, two call sites outside the crate
(`crates/cli/Cargo.toml:13` and `crates/cli/src/commands/refine.rs:162`),
four dependencies that drop with it. No shared types move, nothing else in
the workspace references the crate. The backlog item's checklist (steps 1-7,
9-11) already fully specifies this work: delete the crate, drop the
workspace member and the dependency, remove the flag and its branch,
regenerate the lock file, sweep `process-docs/`, fix the one changelog
clause at line 36 that mentions the TUI mid-sentence in an unrelated entry,
add the removal's own changelog entry, check the release workflow and
packaging for TUI-conditional steps. None of that has an open design
question, so nothing further is said about it here.

## 3. The documentation surface, confirmed

A repo-wide search confirms the nine pages named in the backlog item are the
complete set — nothing under `docs/` or in `README.md` mentions `tui`,
`terminal ui`, `ratatui`, or `crossterm` outside them, and all nine mentions
are real, not noise. None of the nine turned out to be a false positive that
can drop off this list.

| Page | What's there now | Disposition |
|---|---|---|
| `README.md` | One subsection | Delete |
| `docs/reference/configuration.md` | One table row | Delete |
| `docs/reference/triage-classification.md` | One parenthetical | Delete (smallest edit in the set) |
| `docs/reference/cli.md` | One table row, one example | Delete |
| `docs/explanation/architecture.md` | A dedicated crate section, a table row, a crate count, one cross-reference sentence | Rewrite |
| `docs/contributing/developer-guide.md` | A table row, a crate count, one dependency-flow sentence | Rewrite |
| `docs/how-to/review-and-refine.md` | A page section structured around two interfaces, two comparison sentences | Rewrite (the big one) |
| `docs/diagrams/software-architecture.html` | A full node in the dependency graph's data model | Redraw (data edit, auto-layout) |
| `docs/diagrams/user-flow.html` | One bullet and two phrases inside an existing node | Redraw (lighter data edit) |

**Ratio: 3 clean deletions, 4 prose rewrites, 2 diagram data edits.** Nothing
in the nine is pure noise — the smallest is a three-word parenthetical in
`triage-classification.md`; the largest is `review-and-refine.md`, which is
flagged separately below.

## 4. Page by page

### `README.md` — delete, nothing replaces it

Lines 166-174, nested under `### Refine`:

> #### Terminal UI (experimental)
>
> For terminal-based editing, use the `--tui` flag...
> The TUI provides keyboard-driven navigation and inline item toggling
> without leaving the terminal. This is an experimental feature — the
> browser-based workflow is recommended for most users.

**Decision 1 — does anything take this subsection's place?** The `### Refine`
section already describes the web dashboard completely without it; nothing
upstream or downstream of this subsection depends on its presence.

**Recommendation: nothing takes its place.** Adding a sentence acknowledging
the removal (e.g. "a terminal interface was removed...") would itself be the
residue the clean-break standard rules out — a reader who never knew the TUI
existed has no reason to be told one used to. Plain deletion.

### `docs/reference/configuration.md` — delete, mechanical

One row in "CLI flags (refine)":

```
| `--tui` | Use terminal UI instead of web browser. |
```

No decision needed. Delete the row.

### `docs/reference/triage-classification.md` — delete, mechanical

Line 245: "Some items carry a `locked: true` flag that prevents toggling in
the refine UI (both web and TUI)."

Becomes: "Some items carry a `locked: true` flag that prevents toggling in
the refine UI." No decision needed — the parenthetical is the only thing
that moves.

### `docs/reference/cli.md` — delete, two spots, one check run

Per your flag: `--tui` disappearing silently means this reference would
otherwise document an argument that now errors. Checked the rest of the page
for other assumptions that the TUI exists — there are none. The `refine`
command's synopsis (`inspectah refine [OPTIONS] <TARBALL>`), its argument
table, and its other three options (`--port`, `--open`, `--fresh`) make no
reference to an interface choice.

Two edits:
1. Drop the `--tui` row from the `refine` Options table.
2. Drop the "Use the terminal UI instead of a web browser" example block
   (the `inspectah refine --tui ...` command).

No decision needed — the flag vanishing outright (already decided) means the
reference simply stops documenting it. Mechanical.

### `docs/explanation/architecture.md` — rewrite

This page states a crate count three times in prose ("workspace of seven
crates") and carries a dedicated ~30-line section, `## inspectah-tui: the
terminal interface`, plus a table row.

**Decision 2 — the crate count.** "Seven crates" becomes "six crates"
throughout. Mechanical, no real choice here, flagging only because it's easy
to miss one instance.

**Decision 3 — the dedicated section.** `## inspectah-tui: the terminal
interface` (the section describing TUI's module layout and its dependency
rationale) is deleted in full. No replacement content — nothing else needs
to say what a removed crate's modules were.

**Decision 4 — one cross-reference sentence needs a call.** Under
`## inspectah-web: serving the interface`:

> **Why web is separate from refine.** ... This separation means the refine
> engine can be driven by other frontends -- and it is: the TUI crate
> provides a terminal-based alternative (see below).

The claim ("and it is") stops being true. Two ways to close this:

- **Option A — trim the clause, keep the architectural point:** "This
  separation means the refine engine can be driven by more than one
  frontend." States a design property of the architecture without pointing
  at a frontend that no longer exists.
- **Option B — drop the sentence's second half entirely:** end the
  paragraph at "...serves the resulting HTML." The architectural claim
  about multiple frontends becomes speculative once there is exactly one,
  and speculative claims about extensibility are easy to plant and hard to
  keep honest.

**Recommendation: Option B.** The point this sentence is making (why web is
its own crate rather than folded into refine) stands on its own without
invoking a second frontend that no longer exists — Mark decides.

### `docs/contributing/developer-guide.md` — rewrite

Smaller than architecture.md, same shape of edit.

- "The workspace compiles seven crates" -> six.
- "a Cargo workspace with seven member crates" -> six.
- Drop the `inspectah-tui` table row.
- Drop the sentence "`inspectah-tui` depends on `inspectah-refine` and
  `inspectah-core`." from the dependency-flow paragraph, leaving the other
  three dependency sentences intact.

No open decision — this page doesn't carry the cross-reference sentence
architecture.md does, so it's mechanical once the crate count and table are
fixed.

### `docs/how-to/review-and-refine.md` — rewrite, the largest item

**This is the page your brief called out, and it earns it.** The page is
structured around a choice between two interfaces from its second section
onward, and that framing stops making sense once there is only one option.
Three spots need a decision:

**Decision 5 — the "Choose your interface" section (lines 17-54).**
Currently:

```
## Choose your interface

inspectah refine offers two interfaces: a **web UI** (default) and a
**terminal UI**. ...

### Web UI (default)
[web launch commands]

### Terminal UI
[tui launch commands and description]
```

Two options:

- **Option A — fold the web content up, drop the "choose" framing
  entirely.** Replace the whole section with a plain `## Launch the refine
  UI` heading, carrying only the web launch commands, with no "(default)"
  qualifier since there's nothing left to default against:

  > ## Launch the refine UI
  >
  > ```bash
  > inspectah refine ./scan-output.tar.gz
  > ```
  >
  > The server starts on port 8642 and opens your browser automatically.
  > To use a different port:
  >
  > ```bash
  > inspectah refine --port 9000 ./scan-output.tar.gz
  > ```
  >
  > To suppress the browser launch:
  >
  > ```bash
  > inspectah refine --open false ./scan-output.tar.gz
  > ```

- **Option B — keep a heading structure, rename it, keep the content
  under one subsection anyway.** Lower-effort, but leaves a vestigial
  `##`/`###` nesting built for two things holding one.

**Recommendation: Option A.** The "choose your interface" framing is the
single clearest piece of residue in the whole set — a page opening with
"choose between two things" when one of them no longer exists reads exactly
like the half-removed state the clean-break standard rules out. This is the
one place in the nine pages where I'd say the rewrite is not optional if the
clean-break standard is taken seriously.

**Decision 6 — the locked-items comparison (lines 119-129).**

> In the web UI, locked items appear with a lock icon and a reason badge.
> In the TUI, locked items are marked with a lock indicator and cannot be
> selected for toggling.

Proposed replacement, merged into the paragraph above it:

> Some items display as locked: their include/exclude state is visible but
> the toggle is disabled, marked with a lock icon and a reason badge.
> Locked items carry a reason explaining why they cannot be changed (e.g.,
> "Baseline package" or "Required dependency"). These represent
> non-negotiable decisions where toggling would produce an invalid
> Containerfile.

No real choice here beyond confirming the merge reads fine — flagging
because it depends on Decision 5's outcome for section structure.

**Decision 7 — the export fidelity comparison (lines 176-183).**

> When you export from the TUI, the generated Containerfile is
> byte-identical to what the preview pane showed before you exported.
>
> When you export from the web UI's Export dialog, one difference exists
> today: package groups render as their individual members instead of a
> single `dnf group install` line. The image that results installs the
> same packages either way; only the Containerfile's wording differs.

The first sentence is pure TUI content and is deleted outright — no
decision there. The second sentence is a real, standalone caveat about the
web export path that happens to have been phrased as a comparison against
the TUI. Two options:

- **Option A — keep the caveat, reworded without the TUI benchmark:**
  "One difference exists today between the preview pane and the Export
  dialog: package groups render as their individual members in the
  exported Containerfile instead of a single `dnf group install` line.
  The image that results installs the same packages either way; only the
  Containerfile's wording differs."
- **Option B — drop it entirely**, on the reasoning that a caveat framed
  as "here's how this differs from the TUI" has lost its reason for being
  singled out once the TUI is gone, even though the underlying fact
  (preview pane vs. Export dialog wording) is still true and still a real
  gotcha.

**Recommendation: Option A.** The fact is independently true and useful —
it would confuse a user who compares the preview pane's output to what the
Export dialog actually produces, regardless of whether a TUI ever existed.
Losing it would be dropping real information to tidy up an edit, which
isn't what the clean-break standard asks for.

### `docs/diagrams/software-architecture.html` — redraw, confirmed from source

Both diagrams are D3-driven SVGs built from inline JavaScript data arrays
(`docs/diagrams/shared/embed.js` provides the shared rendering helper), not
static images and not generated from an external design tool. "Redrawing"
means editing the data, not re-exporting from somewhere else.

The TUI is a full top-level node in the `crates` array (`id: 'tui'`, ~54
lines, lines 129-181), referenced in two other places:

- `cli`'s `deps` array (line 46) lists `'tui'`.
- The `entry` tier's `crateIds` (line 472) lists `'tui'` alongside `'cli'`
  and `'web'`.

Confirmed from the rendering code: dependency arrows (`depEdges`) are built
entirely by iterating `crates` and each crate's `deps` array at init time —
there's no separately maintained edge list to clean up. Tier width and node
centering are computed from `tier.crateIds.length`. **So the fix is three
edits to the data — delete the `tui` node object, drop `'tui'` from `cli`'s
deps, drop `'tui'` from the entry tier's `crateIds` — and the layout,
arrows, and centering all recompute automatically**, no hand-adjusted SVG
coordinates. No open decision; flagging the specific lines so whoever
executes this doesn't have to re-derive the dependency structure. Worth a
visual check after the edit since the entry tier goes from three nodes to
two and auto-centering on two is a different-enough layout from three to be
worth eyeballing.

### `docs/diagrams/user-flow.html` — redraw, lighter touch

The TUI is not a node here, just content inside the existing `refine` stage
node (around line 120-135):

- The node's `tooltip`: "Use the TUI or web UI to accept, reject, and
  customize migration decisions." -> "Use the web UI to accept, reject, and
  customize migration decisions."
- One child bullet removed entirely: `{ text: 'inspectah refine --tui',
  detail: '...' }`.
- Two other child bullets currently say "(both UIs)" and "(both paths
  converge here)" — these need the parenthetical dropped since there's only
  one path now: "Accept / Reject items" -> drop "(both UIs)"; "Export
  Containerfile" -> "Generate refined build definition" (drop "(both paths
  converge here)").

No open decision, no layout recomputation needed (this edits content inside
one existing node rather than removing a node), but it's still a data edit
in the diagram's source, not a find-and-replace on rendered output, which is
why it's listed alongside the architecture diagram rather than with the
plain-markdown deletions.

## 5a. Answered 2026-11-01. All eight settled; nothing here is open.

Mark took every recommendation. The list below is kept as written so the
reasoning behind each is still readable, with his answer against it.

| # | Decision | Answer |
|---|---|---|
| 1 | README: nothing replaces the subsection | **Confirmed.** Raised separately as a real call rather than a formality, since the README is the front door and a visible section leaves it. A pointer to `refine` was considered and rejected: the surrounding material already covers it, so a signpost would point at a road that was never a fork. |
| 2 | Crate count seven to six | Follows. No choice to make. |
| 3 | Delete the dedicated section | Follows. No choice to make. |
| 4 | Cross-reference sentence | **Option B**, drop the claim. It is speculative once there is exactly one frontend. |
| 5 | "Choose your interface" | **Option A**, fold into one `## Launch the refine UI`. The one genuine information-architecture call in the set. |
| 6 | Locked-items comparison | Follows from 5. No choice to make. |
| 7 | Export fidelity note | **Option A**, keep the web-only caveat reworded. The quirk is true independent of the TUI, and dropping it would lose real information while tidying an edit. |
| 8 | Diagrams | Not a decision. Recorded because data-array edits are a different risk profile from regenerating artwork. |

**One consequence worth carrying into the work.** Decision 7's caveat
exists because the web download alone drops `dnf group install`: the
handler passes `None` for the render context while the CLI and TUI pass
a real one. So removing the TUI removes one of the two interfaces that
got that right. It does not cause the bug and it does not block this
work, but it makes the open item slightly more pressing than it was.
Tracked in FKA as `inspectah-downloaded-export-drops-every-dnf-group-install`.

## 5. Decisions for Mark — summary

1. **README.md:** nothing replaces the deleted subsection. *(Recommendation:
   confirmed, no alternative worth considering.)*
2. **architecture.md crate count:** seven -> six throughout. *(Mechanical.)*
3. **architecture.md dedicated section:** delete in full, no replacement.
   *(Mechanical.)*
4. **architecture.md cross-reference sentence:** trim the clause (Option A)
   or drop the sentence's claim entirely (Option B). *(Recommend B.)*
5. **review-and-refine.md "Choose your interface":** fold into a single
   `## Launch the refine UI` section (Option A) or keep a renamed heading
   around the same content (Option B). *(Recommend A — this is the one
   genuine information-architecture call in the set.)*
6. **review-and-refine.md locked-items comparison:** merge into one
   paragraph, no real choice, depends on #5.
7. **review-and-refine.md export fidelity note:** keep the web-only caveat
   reworded (Option A) or drop it (Option B). *(Recommend A — it's true and
   useful independent of the TUI.)*
8. **Diagrams:** both are data-array edits in the existing HTML/JS source,
   not regeneration from an external tool. No decision, confirming for the
   record since it changes how risky this part of the work is.

Everything else in the nine pages (`configuration.md`,
`triage-classification.md`, `cli.md`, `developer-guide.md`'s table and
dependency sentence, `user-flow.html`'s bullet edits) is mechanical: delete
the TUI content, nothing to decide about what replaces it.

## Related

- `workflow/backlog/inspectah/open/inspectah-remove-the-tui.md` (FKA) — the
  scoped removal this pre-spec's decisions feed back into. Step 8 is what
  this document resolves.
- `topics/inspectah-tui.md` (FKA) — what the TUI was, kept as the
  knowledge record once the clean break takes the pages above out of the
  published docs.
