# Systemd Units Refine Section

## Metadata

| Field | Value |
|---|---|
| Date | 2026-11-16 |
| Status | Approved (Mark Russell, 2026-11-16). Amended 2026-11-16 after the mid-way implementation gate (§ 2 items 9 to 13). Scope and thirteen design choices are decided (§ 2). The rest is the spec author's design; each such call is listed in § 14. No question is open (§ 13). |
| Plan | `process-docs/plans/2026-11-16-systemd-units-refine-section-plan.md` |
| Scope | Single-host refine only: the "Systemd units" section (today "Services"), and five defects in the refine UI around it. Grouping and presentation. No field added to the view response, no snapshot or schema change, no change to what the Containerfile renders. |
| Parent | Phase 6 of `process-docs/specs/implemented/2026-11-13-unified-systemd-units.md`, which recommends a section design in § 11.2 and leaves it to a frontend design pass. This spec is that pass's result. |
| Basis | A design pass with mockups on a real kitchen-sink scan: the view response of a driftify kitchen-sink scan of RHEL 10.2 served by inspectah `e6591669`, unedited, and two stock hosts (RHEL 10.2 and RHEL 9.8) built from the stock-host records in `process-docs/skills/real-host-validation-runs.md`. The pass rendered three options in headless Chromium. It did not check keyboard use, screen readers or widths under 1440 px; § 6.13 states those requirements instead. |
| Backlog items | Closes `inspectah-global-search-services-results-navigate-nowhere`. Touches, does not close: `inspectah-omitted-service-search-text-leaves-out-the-reason`, `inspectah-no-fixture-has-a-non-empty-services-drop-ins`, `inspectah-refine-sections-reorg` (§ 3.2). |
| Verified | Every "today" statement and file reference read at `e6591669`. Every statement of what the feature branch builds read on `feat/systemd-units-refine-section` at `8fe3f24f`. Line numbers are a convenience; the symbol name is the identifier. |

**No code, test, fixture, documentation or CHANGELOG entry was written to
produce this file.**

## 1. Problem

The units section's main control is a checkbox, and for a third of the
rows on a drifted host the checkbox does not mean what it shows.

On the kitchen-sink scan the section lists 19 unit rows, 4 drop-ins, a
default target, 2 hand-made link rows and 1 undetermined-install row.
Of the 20 rows with a toggle (the units and the target), **6 are checked
and write no command**: `tmp.mount` renders as a `# FIXME:`,
`wg-quick@wg0.service` as an `# Omitted:` line, and four services as
`Also=` advisories. With the 3 reported rows, **9 of 23 rows need the
operator to do something by hand.** Today all six declined rows show the
same checked box as a carried row, and the reason is a sentence of helper
text under the row in the same weight as every other note.

The most hazardous row in the scan is the `tmp.mount` FIXME: carrying the
mask would leave `/tmp` read-only on the image, so the export refuses it
and the operator has to decide what to do instead. It is a mount unit.

Other things get in the way:

- **Every unit row carries the same blue "Non-Default State" badge**,
  except three "Unknown Origin" rows. The section lists only units away
  from their preset, so the badge tells the operator nothing.
- **The row's main fact is its hardest text to read.** The state and the
  preset (`enabled (preset: disable)`) are grey italic at 55% opacity.
- **Some rows say the same thing twice.** `cups.service` shows its
  `attention_reason` ("A unit it enables through `Also=` is not enabled
  alongside it.") and its `declined` text, which says the same in more
  words.
- **Kind is lopsided.** The busy host has 11 services, 6 sockets, 1
  timer and 1 mount unit; a stock host has one row or two. A flat list
  in server order mixes them.
- **Five defects** sit in and around the section (§ 9): a stale empty
  state, no section search, a slug in a locked row's badge, a Scheduled
  Tasks badge that always reads 0, and a Containerfile panel footer that
  leaves unit decisions out.

## 2. Decided scope

1. **The section uses option C: units grouped by kind, an outcome filter
   bar with counts, a "needs follow-up" flag on each group header, and
   the advisory rows in their own open group.** Option A (kind groups
   with one collapsed group for the rest, the parent spec's § 11.2 as
   written) and option B (grouped by outcome) were considered and not
   taken (§ 5). (Mark Russell, 2026-11-16)
2. **Single-host refine only.** The fleet view is unchanged. (Mark
   Russell, 2026-11-16)
3. **Grouping and presentation only:** no new field in the view response,
   no snapshot or schema change, no change to what the Containerfile
   renders. (Mark Russell, 2026-11-16)
4. **The default target row carries a one-sentence explanation, as a
   frontend label, not a served string.** (Mark Russell, 2026-11-16)
5. **The "Non-Default State" badge is dropped in this section.** Other
   sections keep theirs. (Mark Russell, 2026-11-16)
6. **The outcome for rows that are checked but write no command, or that
   only report work to do by hand, is named "Needs follow-up".** (Mark
   Russell, 2026-11-16)
7. **Phase 6 also fixes five defects in the current refine UI** (§ 9).
   (Mark Russell, 2026-11-16)
8. **The Containerfile panel footer fix (§ 9.5) applies in single-host
   mode only.** The fleet view's footer stays as it is. (Mark Russell,
   2026-11-16)
9. **A kind group whose shown rows are only orphan drop-ins has no count
   badge** (§ 6.3). (Mark Russell, 2026-11-16)
10. **Section search matches the words a row shows**: a link's relation
    phrase, an undetermined install's reason words, and a mount
    finding's kind and mount point, so section search finds what global
    search finds (§ 6.12). (Mark Russell, 2026-11-16)
11. **After a filter change the live region announces every row now
    shown, orphan drop-ins included**, so under "All" it can read one
    more than the bar's All count (§ 6.4). (Mark Russell, 2026-11-16)
12. **The section's state lives in a hook file, `useUnitsSectionState.ts`,
    apart from `ServiceSection.tsx`'s rendering** (§ 8). Behaviour is
    unchanged. (Mark Russell, 2026-11-16)
13. **The owner badge on a configuration row that a unit drop-in shadows
    reads "Controlled in Systemd units"** in single-host mode (string
    26). (Mark Russell, 2026-11-16)

## 3. Goals, non-goals, anti-goals

### 3.1 Goals

- An operator can answer "does anything here need me?" in one look, and
  reach every such row in one click.
- An operator can find a unit by what it is (service, socket, timer)
  without knowing what the export does with it.
- A toggle never moves the row the operator just toggled.
- Each row says its state against its preset legibly, says what the
  export does with it in the Containerfile's own word, and gives one
  reason, once.
- The section is fully usable from the keyboard and a screen reader, and
  at 768 px wide.

### 3.2 Non-goals

- **The fleet view.** `AggregateApp` and the aggregate handlers keep
  their "Services" section as it is. Extending this design to the fleet
  needs `declined` in the fleet view, which the parent spec's § 17 leaves
  open. Three components this spec changes are shared with the fleet
  view through `AppShell`: `GlobalSearch`, `ContainerfilePanel` and
  `AppShell` itself. Each change to them is scoped to single-host mode by
  the mechanism of § 6.15, and a regression test holds the fleet view
  unchanged (U34).
- **The fleet view's Containerfile panel footer.** It keeps a sentence
  that is wrong there too (§ 9.5, § 2 item 8). **Known limit.**
- **Showing `Also=` repair commands on a row.** The Containerfile's
  `Also=` advisory gives the lines to add (`RUN systemctl enable --
  'cups.socket'` and so on); the view response carries only
  `declined.text`, the advisory's first sentence. The row can say what is
  wrong and not what to type. Showing the lines needs a field, which § 2
  rules out. **This is a known limit:** the operator reads the lines in
  the Containerfile panel.
- **Finding a withheld drop-in through the outcome filter.** Outcomes are
  computed for unit rows, the default target and reported rows (§ 6.1).
  A drop-in the export withholds keeps its `withheld` note under its
  parent, as today, and the "Needs follow-up" filter finds it only when
  its parent is a follow-up row. **Known limit**, named so a reviewer does
  not take it for an oversight.
- **Anything in `inspectah-refine-sections-reorg`.** That item is about
  which sections exist, their order and grouping, and what the sidebar
  badge means once triage versus reference is computed. This section
  stays one sidebar entry, `units`, in the same group at the same place,
  and its design does not depend on the triage versus reference split. It
  does add one more section that mixes decision rows and reported rows,
  which is the case the reorg is about; the reorg can move or split it
  later without undoing anything here.
- **Server-side search text.** `crates/web/src/adapter.rs` and the
  `units` reference section it builds are not changed (§ 6.12).

### 3.3 Anti-goals

- **No reordering on toggle.** Rows are placed by kind and name, never
  by outcome. Option B was not taken for this reason.
- **No new served strings.** Every new word on screen is a frontend label
  (§ 7). Reason and advisory sentences stay the server's, shown as given:
  never composed, rephrased or parsed.
- **No second home for a row.** A unit row appears once. A filter hides
  rows; it never copies them into another group.

## 4. What the section does today

Read at `e6591669`.

- `MainContent.tsx` renders the section when `activeSection === "units"`
  (`:758`). Its heading is `Services`, plus `(N shadow override[s])` when
  any `unit_dropins` entry is a full shadow (`:766-773`), which no other
  section does.
- `ServiceSection.tsx` renders one flat `role="grid"` (`aria-label
  "Services"`, `:743-748`): the default target row first, then every
  `unit_states` row in server order with its drop-ins nested under it,
  then mount findings, manual links and undetermined installs, then
  orphan drop-ins under "Other drop-in overrides" (`:974-1002`).
- A unit row shows its name, a kind chip, `(owning_package)`, and
  `current_state (preset: default_state)` in grey italic at 55% opacity
  (`:847-858`); a triage badge (`badgeTextForTriage`, `:139-143`), which
  is "Non-Default State" for almost every row; `attention_reason`
  unless it equals `declined.text` (`:770-773`); `declined.text`;
  `Also=` members as plain `<code>`; and `Instance of`, `Needs
  configuration:` and `Keeps data in:` as separate lines.
- Rows have `role="row"`; only the first has `tabIndex={0}` and no row
  handles arrow keys or `j`/`k`. Space or `x` on a focused row toggles it.
  The native checkboxes are tab stops.
- Link and undetermined-install rows show wire words: the relation
  (`wants`) and the cause only as `data-cause`.
- Scheduled Tasks already links a timer to its unit row when `unit_row`
  is true (`ContextItem.tsx:89-99`), through
  `onNavigateToItem("units", unit)` and the reveal effect in `App.tsx`,
  which finds `service-item-<unit>`.
- The sidebar label "Services" is served by `/api/groups` from
  `SectionMeta` in `crates/pipeline/src/section_group.rs:148-152`, not by
  the view response. `MainContent.tsx:53` and `GlobalSearch.tsx:36` each
  hold their own copy. The sidebar's decision count for `units` is
  `unit_states + unit_dropins + (default_target ? 1 : 0)`
  (`Sidebar.tsx:106-112`).
- `App.tsx` mounts `SingleHostApp` or, for an aggregate session,
  `AggregateApp` (`App`, `:44-61`). Both render `AppShell`, which renders
  `GlobalSearch` and `ContainerfilePanel` (`AppShell.tsx:233-278`);
  `AggregateApp` passes `isAggregateMode` (`AggregateApp.tsx:523`).
  `MainContent`, `Sidebar` and `useGroups` (the only reader of
  `/api/groups`) are used by `SingleHostApp` only; `AggregateApp` renders
  `AggregateSidebar` from its own view. The fleet view has a `units`
  section too (`aggregate_handlers.rs:773`, display name "Services"), and
  `GlobalSearch` labels its results from its own `SECTION_LABELS` map,
  not from the section's `display_name` (`GlobalSearch.tsx:168`).

## 5. Approaches

All three options share the row changes of § 6.6. They differ in
grouping.

**A. By kind, § 11.2 as written.** One collapsible group each for
services, sockets, timers and paths; targets, slices, mount units and
every reported row in one collapsed group. Easy: finding a unit you can
name; rows never move; smallest build. Hard: seeing what needs work. The
six declined rows are spread over two groups, and the FIXME and all three
reported rows start hidden, so the operator has to open a group named for
targets and slices to learn that `/tmp` would be read-only.

**B. By outcome.** Three groups named for what the export does: "Needs
follow-up" (declined rows, then reported rows), "Carried" (subdivided by
`current_state`, which mirrors the Containerfile's `enable`, `disable`
and `mask` blocks) and "Excluded". Kind is a label and a filter. Easy:
the five-second answer, and the Carried group reads line for line
against the Containerfile panel. Hard: rows move. `declined` is
recomputed from the toggles on every view response, so excluding
`cups.service` clears its `declined` and the row leaves "Needs follow-up"
on the operator's own keystroke; excluding a drop-in can move its parent.
Finding a unit by name means looking in three groups.

**C. By kind, outcome as a filter (taken, § 2).** Rows grouped by kind
and never moved, as in A. Above the list, a filter shows the outcome
counts and narrows to one outcome. Each kind group's header says how
many of its rows need follow-up. The reported rows get their own open
group. C answers both questions: the bar says whether anything needs the
operator, with a number and one click, and the groups say where a unit
is. It also fixes what is wrong with A, which is not grouping by kind but
collapsing the advisories: they are few, so collapsing them saves a few
lines and hides the rows that ask for manual work.

### 5.1 The case against C, and what would change it

B is the stronger information design, and the row-movement objection may
be overstated. An operator makes a handful of toggles in this section,
not hundreds. The Packages section already moves an excluded repo's
packages into a zone of their own. A row that jumps into "Needs
follow-up" because of a toggle elsewhere is arguably the most useful
signal the section could give, and C shows it only as a count that ticks
up. And C's filter is a control people may not click: if the operator
never presses "Needs follow-up", C is A with a number on top. C also has
two navigation devices where A and B each have one.

What would move the design to B: watching someone triage a real drifted
host with the keyboard and not lose their place when a row moves; or
`declined` becoming sticky for an excluded row (a server change, outside
this scope), which would stop a row leaving its group on its own toggle.
What would move it to A: real hosts turning out to have almost no
declined rows, so the bar is usually absent anyway. The kitchen-sink
profile is built to trip every rule and a customer host may trip none.
Two stock records and one drift profile are the whole evidence base, and
none is a customer host.

## 6. Design

### 6.1 A row's outcome

Every row with a toggle, unit rows and the default target, has one
outcome, computed in the browser from fields § 11.1 of the parent spec
already carries, in this order:

| Outcome | Rule |
|---|---|
| Needs follow-up | `declined` is present |
| Excluded | `include` is false |
| Carried | otherwise |

Every reported row (each `mount_findings`, `manual_links` and
`undetermined_installs` entry) has the outcome **Needs follow-up**: it
has no toggle and the Containerfile writes comments for it and no
command.

Drop-ins have no outcome of their own: they are shown with their parent
row and follow it through the filter (§ 6.4). A locked row follows the
same rules (a locked, excluded row is Excluded).

The outcome is recomputed from each view response. A row's place in the
list never depends on it.

### 6.2 Groups and their order

| Order | Group | Contains | Drawn when |
|---|---|---|---|
| 1 | Default target | The `default_target` row | `default_target` is present |
| 2 | Services | `unit_states` rows with `kind` `service` | it has a row to show |
| 3 | Sockets | `kind` `socket` | as above |
| 4 | Timers | `kind` `timer` | as above |
| 5 | Paths | `kind` `path` | as above |
| 6 | Other units | Every other `kind`: targets, slices, and package-owned mount, automount and swap units | as above |
| 7 | Reported, not carried | Every `mount_findings`, `manual_links` and `undetermined_installs` entry | it has a row to show |

- A group with no row to show, under the current filter and search, is
  not drawn: no header, no "(0)".
- Within a kind group, unit rows sort ascending by `unit`, compared as
  plain strings (code-point order), so the order is deterministic and a
  template's instances (`getty@tty2.service`, `getty@tty3.service`) sit
  side by side. Rows with the same `unit` (a true duplicate, § 6.12)
  keep their response order.
- **Templates and instances are siblings, never parent and child.** A
  template has a row only when `unit_states` holds one: a masked template
  (`getty@.service`), which the parent spec's § 11.1 includes. That row
  is an ordinary unit row in the group of its kind, sorted by name like
  any other, so it usually sits next to its instances. No row is nested
  under a template's row, and an instance's row is the same whether or
  not its template has one; the instance names its template through
  `Instance of` (§ 6.6 item 8).
- Drop-ins stay nested under their parent row, in server order, full
  shadows included, as today. When two unit rows share a `unit`, the
  drop-ins nest under the first of them only (§ 6.12).
- Orphan drop-ins (§ 6.8) sit at the end of the group of their `kind`.
- Every group is open by default and collapsible from its header.
  Collapse state lives in the section's state hook (§ 8) and resets
  when the operator leaves the section. A collapsed group keeps its rows in the
  DOM under `hidden`; a reveal opens it (§ 6.11).

### 6.3 Group header

A group header is a button with `aria-expanded` and `aria-controls`
naming the group's grid. It shows, in order:

1. The disclosure angle (the `AngleRightIcon` / `AngleDownIcon` pair
   `TriageBucketGroup` uses).
2. The group title (§ 7).
3. A count badge (PatternFly `Badge`, read): the number of rows the group
   shows under the current filter and search. Drop-ins are not counted,
   orphan drop-ins included; in "Reported, not carried" each reported
   row counts once. The badge is drawn when that number is above 0. A
   group is drawn with the number at 0 only when it is a kind group
   whose shown rows are all orphan drop-ins (§ 6.8), and such a group
   has no badge (§ 2 item 9). The Default target group has no count.
4. **The follow-up flag**, in kind groups 2 to 6 only, and only while the
   filter is "All": when N of the group's unit rows have the outcome
   Needs follow-up and N > 0, a compact orange outline `Label` reading
   `N needs follow-up` (N = 1) or `N need follow-up` (N > 1). N counts
   every such row in the group, whether or not search hides it. The
   Default target group shows the outcome on the row itself (§ 6.5), and
   "Reported, not carried" is all follow-up, so neither has a flag.

The button's accessible name is the title, then the count after a space
when the badge is drawn, then a comma and the flag's text when the flag
is drawn: a screen reader hears "Services 11, 5 need follow-up". A group
with no badge is named by its title alone ("Services"), or by its title,
a comma and the flag ("Services, 5 need follow-up", when a section query
leaves only an orphan drop-in shown under "All").

### 6.4 The outcome filter

A bar between the heading (and section search, when open) and the first
group, built on PatternFly `ToggleGroup` (already in
`@patternfly/react-core`; no new dependency):

```
Show  [All 23] [Needs follow-up 9] [Carried 14] [Excluded 0]
Needs follow-up: the Containerfile writes a comment for the row and no command.
```

- **Values and counts.** `All` counts every row with an outcome: unit
  rows, the default target and reported rows. `Needs follow-up` counts
  declined unit rows, a declined default target and every reported row.
  `Carried` and `Excluded` count unit rows and the default target with
  that outcome. Drop-ins are not counted. Counts are recomputed from each
  view response and are not affected by section search.
- **When it is drawn.** Only while Needs follow-up + Excluded > 0. On a
  host where everything is carried, the bar is absent (both stock hosts).
- **A value with count 0 is disabled** (`isDisabled`), except the
  selected one.
- **The hint line** under the bar is shown while Needs follow-up > 0.
- **What a value does.** "All" shows every row. Any other value shows the
  rows with that outcome. A unit row shown under a filter brings its
  drop-ins with it. Orphan drop-ins are shown only under "All". "Reported,
  not carried" is shown under "All" and "Needs follow-up" only. A pinned
  branch is shown under every value (below). A group left with no row is
  not drawn (§ 6.2).
- **A toggle never hides what the operator acted on (pinning).** A
  *branch* is a unit row with the drop-ins nested under it, the default
  target row, or one orphan drop-in. When the operator toggles a row,
  the branch that holds it is pinned: the row's own branch for a unit
  row, the default target or an orphan drop-in, and the parent's branch
  for a nested drop-in. A pin is held by the occurrence key (§ 6.11) of
  the row that heads the branch, so of two branches with one row key
  only the toggled one is pinned. A pinned branch stays shown, in its
  place, with all its rows, whatever the outcome filter and the section
  search would now say, and its group is drawn even when nothing else in
  it is shown. Focus stays on the toggled row, the occurrence the
  operator toggled. Pins accumulate while the
  operator works. All pins are cleared when the operator changes the
  filter value or the section query, when a reveal changes either
  (§ 6.11), and when the section unmounts; the filter returning to "All"
  because the bar stopped being drawn clears none, since that follows a
  toggle. A pin whose occurrence key is not in the new view response is
  dropped. A pinned row's outcome label, its group's count and the
  filter counts update from each view response as for any row. Only the
  operator's toggles pin: a row whose outcome changes because of another
  row's toggle, or an Undo or Redo, follows the filter and search as
  computed, unless it is in a pinned branch. This covers the case where
  restoring an excluded `[Install]` drop-in clears its parent's
  `declined` (`rule_5_drop_ins` in
  `crates/pipeline/src/render/unit_plan.rs`): the parent's branch is
  pinned, so neither the parent nor the drop-in leaves "Needs follow-up"
  on that keystroke.
- **Persistence.** The filter is "All" each time the section mounts; it
  survives toggles, Undo, Redo and view refreshes while the operator stays
  in the section; it is not stored in `localStorage` or the URL. If the
  bar stops being drawn while a value is selected, the filter returns to
  "All".
- **Announcement.** Changing the value, by the operator or by a reveal
  that sets it to "All" (§ 6.11), updates a polite live region with the
  number of rows now shown (§ 7, strings 19 to 22), every shown row
  counted, orphan drop-ins included (§ 2 item 11). The number is
  `resultCount`'s (§ 6.12): unit rows, the default target, orphan
  drop-ins and reported rows shown under the new value and the current
  section query, nested drop-ins not counted; a filter change clears
  every pin first, so no pin adds to it. It is not the bar's count for
  the value, which counts only rows with an outcome and ignores the
  query: on the kitchen-sink view, choosing "All" announces "Showing all
  24 rows." while the bar reads "All 23", since `sshd.service`'s orphan
  full shadow is shown and has no outcome; under a section query the
  number is the rows shown under that query. The region is in the DOM,
  empty, from the bar's first render, so the first change is announced.

### 6.5 The default target

The default target has its own group, first, titled "Default target". Its
row keeps today's toggle, `ItemId::DefaultTarget`, guard key and test id
`default-target`, and shows:

- the target name;
- the explanation, a frontend label, on every host and whatever the
  toggle's state: **"The target the source host boots to. Included, the
  image boots to it too."**;
- when `declined` is present, the outcome label (§ 6.6 item 2) and
  `declined.text`, as for a unit row.

No kind chip: the group title says what it is. On a stock host this is
the only row, so the explanation is what keeps a lone checked box from
reading as a mystery.

### 6.6 The row

Each change below applies to unit rows, and where it says so to the
default target, drop-ins or reported rows.

1. **State against preset.** `current_state` in body text, then
   `preset: <default_state>` in subtle text
   (`--pf-t--global--text--color--subtle`), no italic, no opacity. No
   parentheses. When `default_state` is null, the state alone.
2. **The outcome label.** A row with `declined` shows a compact outline
   `Label` in the badge cell chosen by `declined.renders`: `fixme` →
   `FIXME` (orange), `advisory` → `Advisory` (blue), `omitted` →
   `Omitted` (grey). These are the Containerfile's own comment prefixes,
   so the row and the file use one word. Also on the default target.
3. **No "Non-Default State" badge** (§ 2, item 5). A unit row whose
   `triage.primary_reason` is `service_non_default_state` shows no triage
   badge. Any other reason renders as today (`badgeTextForTriage`, same
   colour), so "Unknown Origin" stays. Drop-in badges ("Drop-in
   Override", "[Install]", "Shadow override") are unchanged.
4. **One reason per row.** When a row has `declined`, `declined.text` is
   shown and `attention_reason` is not. Without `declined`, an unlocked
   row's `attention_reason` is shown as today.
5. **`Also=` members link to their own row.** On the `Also=` line, a
   member whose name matches a `unit_states` row's `unit` is a link (an
   `<a>` with `href="#units:<member>"`) that reveals that row (§ 6.11); a
   member with no row stays `<code>`. The `links` word beside it is
   unchanged. No new field: the match is against `unit_states`.
6. **Drop-ins and full shadows under their parent**, as today. Their
   "Service excluded" badge, shown while the parent is excluded, reads
   **"Unit excluded"**, since the parent is no longer always a service.
7. **`required_config` paths link to their config row** when
   `config_files` holds an entry at that path, through the same
   `onNavigateToConfig` route a mount finding's unit file uses
   (`#configs:<path>`); a path with no config row stays `<code>`.
   `data_paths` stay plain: nothing in the export holds them.
8. **Instances.** An instance sits in the group of its kind and sorts by
   name (§ 6.2). `Instance of <template>` moves from its own line onto
   the row's first line, after the package word.
9. **A unit with no package** shows its `fragment` word in parentheses,
   `(local)`, `(shadowed)` or `(absent)`, where the package name would
   be. A row with a package shows `(<package>)` as today.
10. **Kind chip.** Shown only in "Other units", where the group mixes
    kinds. Rows in a kind-named group drop it.
11. **Reported rows kept together per unit.** In "Reported, not carried",
    rows are grouped by `unit`, units sorted by name; within a unit, its
    mount finding first, then its links, then its undetermined install,
    each kind in server order (a true duplicate, § 6.12, is two rows of
    one kind). So `driftify-upheld.service`'s link row and its
    undetermined-install row sit side by side.
12. **A link's relation and cause get words** (§ 7). The relation is
    shown as a phrase before the parent: `wants` → "wanted by",
    `requires` → "required by", `upholds` → "upheld by", `alias` → "has
    alias", `also` → "named in Also= by". The cause is a compact grey
    outline label in the badge cell. An undetermined-install row shows the
    `undetermined` cause label too; its reason keeps today's words
    (`upheld by`, the wire word with `_` as a space). Advisory lines and
    `Not in the export:` are unchanged.
13. **A locked row's badge never prints a slug** (§ 9.3).

Unchanged: the triage left border, the toggle and its guard keys, Space
or `x` on a focused row, the test ids `service-item-<unit>`,
`service-group-<unit>`, `dropin-item-<path>`, `manual-link-<identity>`,
`undetermined-install-<unit>`, `mount-finding-<unit>` and
`default-target`, the `declined` note's classes, and every served string.
Added: each row of this section carries its row key in `data-unit-key`
(§ 6.11), on the element that carries its row test id, and the reveal
reads the key in place of the test id. The two rows of a true duplicate
(§ 6.12) carry the same test id and the same row key; only their
occurrence keys differ.

**DOM ids.** A row's first record with its row key (ordinal 0, § 6.11)
keeps today's DOM ids for the row's elements and notes. A later
occurrence of a true duplicate gives each such id a suffix of its own,
generated per row (React `useId`), appended to today's id, so no two
elements of the section share an id. Each occurrence's
`aria-describedby` names its own notes, never its twin's.

### 6.7 Reported, not carried

The last group, open, with a one-line description under its header:
"inspectah reports these and does not carry them. No toggle: the lines
under each row are the Containerfile's comments for it." It holds the
rows of § 6.6 item 11. It is drawn under "All" and "Needs follow-up" and
counts toward "Needs follow-up". A mount finding keeps its link to its
unit file's Configuration Files row.

### 6.8 Orphan drop-ins

A drop-in whose `unit` has no `unit_states` row goes at the end of the
group named for its `kind` (the parent's kind, § 11.1); a drop-in with no
`kind` goes in "Other units". The block keeps today's heading "Other
drop-in overrides", note "These overrides have no unit-state decision
row.", per-unit subheading and test ids (`dropin-orphans`,
`dropin-orphan-group-<unit>`), now inside the group. More than one kind
group can hold such a block. Orphan drop-ins are shown under "All" only
and do not count toward the group's count.

### 6.9 Empty states

| Case | What shows |
|---|---|
| No `unit_states`, no `unit_dropins`, no reported row, and a default target | Heading, then the filter bar by the rule of § 6.4, then the Default target group, then "No other unit differs from its preset, and no drop-in was found." The target is the only row with an outcome, so a carried target draws no bar (both stock hosts); an excluded target draws it with All 1 and Excluded 1, a declined one with All 1 and Needs follow-up 1 and the hint. Under a value that hides the target, the filter's empty state below applies and the sentence still shows. |
| The same with no default target | Heading, then "No unit differs from its preset, and no drop-in was found." This replaces "No service state changes detected." (§ 9.1). |
| A kind with no row | No group (§ 6.2). |
| A filter value that shows no row (reachable after Undo, or when only pinned rows held it) | Under the bar: "No units match this filter." and a link button "Show all", which sets the filter to "All". |
| Section search with no match | The existing `EmptyState` "No items match your search" with its "Clear filter" link button, as in Language Packages and Unmanaged Files. It takes precedence over the filter's empty state. |

### 6.10 Sketches

Stock RHEL 10.2:

```
Systemd units
v Default target
  [x] multi-user.target
      The target the source host boots to. Included, the image boots to it too.
No other unit differs from its preset, and no drop-in was found.
```

Stock RHEL 9.8 (no bar: nothing needs follow-up, nothing is excluded):

```
Systemd units
v Default target
  [x] multi-user.target
      The target the source host boots to. Included, the image boots to it too.
v Services 1
  [x] NetworkManager-wait-online.service (NetworkManager)  enabled  preset: disable
```

Kitchen-sink RHEL 10.2, filter "All":

```
Systemd units
Show  [All 23] [Needs follow-up 9] [Carried 14] [Excluded 0]
Needs follow-up: the Containerfile writes a comment for the row and no command.
v Default target
  [x] graphical.target
      The target the source host boots to. Included, the image boots to it too.
v Services 11  (5 need follow-up)
  [x] bluetooth.service (bluez)  masked  preset: enable
  [x] cups.service (cups)  enabled  preset: enable                        [Advisory]
      cups.service is enabled on the source host. Not carried: ...
      Also=  cups.socket installed   cups.path ...
  [x] driftify-also-main.service (local)  enabled  preset: disable  [Unknown Origin] [Advisory]
  ...
  [x] httpd.service (httpd)  enabled  preset: disable
      |- [x] /etc/systemd/system/httpd.service.d/override.conf  [Drop-in Override]
      |- [x] /etc/systemd/system/httpd.service.d/limits.conf    [Drop-in Override]
  ...
  [x] wg-quick@wg0.service (wireguard-tools) Instance of wg-quick@.service  enabled  preset: disable  [Omitted]
      wg-quick@wg0.service (its configuration /etc/wireguard/wg0.conf holds a redacted value; ...
  Other drop-in overrides
  These overrides have no unit-state decision row.
  sshd.service
      |- [x] /etc/systemd/system/sshd.service  [Shadow override]
v Sockets 6
  [x] cockpit.socket (cockpit-ws)  disabled  preset: enable
  ...
v Timers 1
  [x] myapp-report.timer (local)  enabled  preset: disable  [Unknown Origin]
v Other units 1  (1 needs follow-up)
  [x] tmp.mount mount (systemd)  masked  preset: disable                  [FIXME]
      tmp.mount is masked on the source host, where /tmp is then a directory ...
v Reported, not carried 3
  inspectah reports these and does not carry them. No toggle: ...
  driftify-static.service  wanted by multi-user.target   [Link its [Install] does not make]
      # Advisory: multi-user.target.wants/driftify-static.service is a link ...
  driftify-upheld.service  upheld by multi-user.target   [[Install] cannot be relied on]
      # Advisory: multi-user.target.upholds/driftify-upheld.service is not carried: ...
  driftify-upheld.service  upheld by  /etc/systemd/system/driftify-upheld.service  [[Install] cannot be relied on]
      # Advisory: ...
```

With "Needs follow-up" selected, the same host shows Services with its 5
declined rows, Other units with `tmp.mount`, and Reported, not carried;
Default target, Sockets and Timers are not drawn.

### 6.11 Reaching a row: Scheduled Tasks, `Also=`, search, "Go to owner"

Every route that sends the operator to a row in this section lands on
it, shown and focused, whatever the section's filter, collapse state and
section search.

**Routes.** All four end in `App.tsx`'s `handleNavigateFromSearch`,
which sets `revealItemId`, the pending focus item and the navigation
counter:

- **Scheduled Tasks** keeps its link from a timer row to its unit row
  (`ContextItem.tsx`, `onNavigateToUnit`, through `MainContent`'s
  `onNavigateToItem("units", unit)`), unchanged. It already does what the
  parent spec's § 11.2 asks. It changes section, so the units section
  mounts fresh: filter "All", every group open, no query (§ 6.2, § 6.4,
  and `MainContent` clearing its filter text on a section change).
- **Global search** through `AppShell`'s `handleSearchNavigate`, which
  also clears the section query, as today.
- **`Also=` links** (§ 6.6 item 5). `ServiceSection` calls
  `onNavigateToItem("units", member)`, passed down by `MainContent`.
  The section does not change, so the section stays mounted with its
  filter, collapse state and query as they were; the reveal below
  undoes whichever of them hides the destination.
- **"Go to owner"** on a config row a drop-in shadows (`shadowOwner.ts`,
  through `MainContent`'s `onNavigateToItem`). It changes section, so
  the units section mounts fresh, as for Scheduled Tasks. It sends the
  drop-in's row key (below), not its bare `path`.

**Row keys.** Every row of this section has a row key: its kind and its
complete identity, JSON-encoded as an array of strings, kind first. The
key is `JSON.stringify` of that array with no further arguments (no
replacer, no indent), so every producer yields the same bytes. One
pure function in `unitOutcome.ts` builds it from a row, and one parses
it back; nothing else spells a key. The row key is the row's logical
identity: two records can share one (a true duplicate, § 6.12), so it
names the row for search and navigation but is not what renders a row
(occurrence keys, below).

| Row kind | Row key | Identity, from source |
|---|---|---|
| Unit state | `["unit_state", unit]` | `UnitStateView.unit`; the op's `ItemId` and guard key (`serviceKey`) name the unit alone |
| Drop-in or full shadow | `["dropin", path]` | `path`; the op's `ItemId` and `dropinKey` name the path alone |
| Default target | `["default_target"]` | none: a snapshot records at most one (`ItemId::DefaultTarget` has no key) |
| Manual link | `["manual_link", relation, parent, unit]` | `relation`, `parent` and `unit`, as `linkIdentity` in `ServiceSection.tsx` encodes them; `cause` is not part of it |
| Undetermined install | `["undetermined_install", unit]` | `unit`, the row's React key today (`["undetermined", unit]`) |
| Mount finding | `["mount_finding", unit]` | `unit`, the row's React key today |

`relation` is the wire word (`wants`, `requires`, `upholds`, `alias`,
`also`). Each row carries its row key in a `data-unit-key` attribute.

**Occurrence keys.** Every record the section renders also has an
occurrence key, unique within one view response: its row key, then `#`,
then the record's zero-based ordinal among the records of its own
source list (`unit_states`, `unit_dropins`, `manual_links`,
`undetermined_installs`, `mount_findings`) that have the same row key,
counted in response order. A record with no duplicate is `#0`; the
default target is always `["default_target"]#0`. `unitOutcome.ts`
assigns occurrence keys once per view response, from the six unit
fields as served, before grouping, the outcome filter or the section
search sees any record, so changing the filter, the query or a group's
collapse state never changes a record's occurrence key. Every kind gets
one, duplicated or not, so no code asks whether a record has a twin.
The projection makes one view per record in snapshot order, so a record
keeps its occurrence key from one view response to the next unless a
record with the same row key before it in its list is added or removed.
Two occurrence keys are equal only when row key and ordinal are both
equal: a row key ends with `]` and an ordinal holds only digits, so the
last `#` is always the boundary. Nothing parses an occurrence key.

Each identity has its own uses, and none borrows the other's:

| Identity | Used for |
|---|---|
| Row key | `data-unit-key`; a global search entry's `itemId`; "Go to owner"; the reveal lookup, which resolves to the first row in section order carrying the key |
| Occurrence key | The React `key` of each row and of the element that wraps a unit row's branch; the roving tab stop (§ 6.13); pins (§ 6.4); focus after a toggle; any per-row UI state the section keeps |
| The op's `ItemId` | The toggle's op and its guard key (`serviceKey`, `dropinKey`), unchanged |

An occurrence key never appears in a search entry, an item id, a URL,
`data-unit-key` or a test id.

**Which row an item id names.** An item id that reaches this section is
one of two forms, told apart by its first character:

| Item id | Sent by | Row |
|---|---|---|
| Starts with `[`: a row key | Global search (every units entry, § 6.12); "Go to owner" (`["dropin", path]`) | The first row, in section order, whose own key equals it, compared as a string |
| Anything else: a bare unit name | The Scheduled Tasks link and `Also=` links only | Read as `["unit_state", <name>]`: the `unit_states` row with that `unit`, never a reported row of the same name. Both links link only when such a row exists (`unit_row`; § 6.6 item 5). |

The two forms cannot be confused. A bare id is a unit name, and
`UnitName` admits only ASCII letters and digits and `:_.\@-`
(`UNIT_NAME_PUNCTUATION` in `crates/core/src/types/units.rs`, enforced
by `TryFrom<String> for UnitName`); every unit field of the rows above
is a `UnitName` in the projection (`UnitStateView`, `ManualLinkView`,
`UndeterminedInstallView`, `MountFindingView`, and `AlsoView` for an
`Also=` member). So a unit name never holds `[` or `"`, and every row
key starts with `["`. Two row keys are equal only when their kinds and
every identity part are equal, since a JSON array of strings decodes to
exactly the strings it was built from; a separator inside a unit name
(`-`, `:`, `.`) or a backslash (`backup\x2djob.timer`) is a character of
a JSON string, never a boundary. A unit named
`undetermined-install-demo.service` has the key
`["unit_state","undetermined-install-demo.service"]`, and an undetermined
install for `demo.service` has `["undetermined_install","demo.service"]`.
An id that starts with `[` names a row only when it parses to an array
of strings whose first is one of the kinds above, with that kind's
number of identity parts, and is byte for byte what `JSON.stringify`
writes for that array (no added whitespace, no other escaping).
Anything else, the same array spelled another way included, names no
row and is treated as not found.

**The reveal.** When `revealItemId` names a row of this section, the
destination is made visible before `App.tsx`'s reveal effect looks it
up, in this order:

1. **Section query.** When the query hides the destination (its branch
   fails the match of § 6.12, computed by `unitOutcome.ts`, and is not
   pinned), `MainContent` clears the query text, as a global search
   navigation does; the search box stays open. A query that leaves the
   destination shown is kept.
2. **Outcome filter.** When the filter hides the destination, the
   section sets it to "All".
3. **Collapse.** When the destination's group is collapsed, the section
   opens it.

`unitOutcome.ts` decides the three steps in one function; the section's
state hook (§ 8) applies them. Steps 1 and 2 clear pins (§ 6.4).
`App.tsx`'s reveal effect then finds the row and focuses it as today.
`App.tsx` needs no code for unit groups: the section owns collapse state
and has opened the group before the lookup. A reveal that finds no row after these steps is a
defect, and U26 proves it does not happen.

**Reveal lookup.** When the navigation's section is `units`, `App.tsx`'s
reveal effect turns the item id into a row key as the table above says
(a bare name becomes `["unit_state", <name>]`) and finds the first
element, in document order, whose `data-unit-key` equals that key,
compared as a string, as
`elementByTestId` compares test ids; it never builds a CSS selector from
the key (`process-docs/skills/web-disposition-contract.md`). It does not
try `REVEAL_TESTID_PREFIXES` for `units`. Every other section resolves
through the prefixes as today, and the list itself is unchanged. The test
ids are no key for this section: `mount-finding-kind-<unit>`, the kind
cell of one mount finding's row, is also the row test id of a mount
finding for a unit named `kind-<unit>`, and `kind-tmp.mount` is a valid
name.

### 6.12 Search

**Section search.** `MainContent` renders `SectionSearch` for `units`
when `sectionSearchOpen`, as it does for packages, configs, language
packages and unmanaged files, and passes the filter text to
`ServiceSection`. A row matches when the lowercased query is a substring
of its search text. A row's search text holds the fields that identify
it and the words the row shows (§ 2 item 10), lowercased:

- a unit row: `unit`, `kind`, `owning_package` or fragment word,
  `current_state`, `default_state`, `instance_of`, `declined.text`,
  `attention_reason`, and the paths of its drop-ins (a drop-in match
  shows the parent with all its drop-ins);
- the default target: its `target`;
- an orphan drop-in: `path` and `unit`;
- a manual link: `unit`, `parent`, `relation` (the wire word), its
  relation phrase (string 17, such as "required by") and its advisory
  lines;
- an undetermined install: `unit`, its reason words as the row shows
  them (the wire word with `_` as a space, such as `upheld by`),
  `install_files` and its advisory lines;
- a mount finding: `unit`, `kind`, `mount_point` and its advisory lines.

Global search indexes the same text for each row (table below, whose
only differences are a unit row's drop-in paths and the default
target's words `default target`), so section search finds a row by
every field and shown word global search indexes for it.
`unitOutcome.ts` builds both texts from one field list per row kind.

Search and the outcome filter both apply; a pinned branch is shown
whatever either says (§ 6.4). `resultCount` is the number of shown rows
(unit rows, the target, orphan drop-ins, reported rows).
ArrowDown from the search box focuses the first shown row of this
section; `handleArrowDown` in `MainContent` gains that case. Escape
closes and clears as elsewhere.

**Global search, single-host mode only.** In single-host mode
(§ 6.15), `GlobalSearch` indexes the view response's rows in place of
the `units` reference section. Every units entry is a
`GlobalSearchResult` with `sectionId` `units`, `itemId` the row's key
(§ 6.11), so the entry carries its kind, and its own search text:

| Entry | Kind in `itemId` | Search text |
|---|---|---|
| Each `unit_states` row | `unit_state` | the unit row's section search text without its drop-ins' paths (each drop-in is its own entry) |
| The default target | `default_target` | `target`, `default target` |
| Each `unit_dropins` entry | `dropin` | `path`, `unit`, as an orphan drop-in's |
| Each manual link | `manual_link` | as section search |
| Each undetermined install | `undetermined_install` | as section search |
| Each mount finding | `mount_finding` | as section search |

**Search text travels on the entry.** `GlobalSearchResult` gains an
optional `searchText`, lowercased, which only units entries set, and the
filter reads `r.searchText` before its two map lookups. Units entries put
nothing in `searchTextMap`, so no map key, bare or `sectionId:itemId`,
can hand one entry another's text. Every other section's entries keep
the map as today. The drop-in entries already built from
`serviceDropins` take the `dropin` key and carry their text in single-host
mode; only `SingleHostApp` sets that prop (`App.tsx`,
`searchServiceDropins`), so the fleet view has no drop-in entries either
way.

**Duplicates are by complete identity.** The identity of each kind is
the one in § 6.11's row-key table. A unit name can appear in more than
one kind: the snapshot keeps `state_changes`, `drop_ins`,
`mount_findings`, `manual_links` and `undetermined_installs` as separate
lists (`UnitSection` in `crates/core/src/types/units.rs`), and loading
does not require them to be disjoint. Within one kind, rows that differ
in any part of the identity are different rows: two manual links for one
unit with a different `relation` or `parent` are both valid, since the
collector emits one record per link (`link_findings` and
`apply_install_difference` in
`crates/collect/src/inspectors/units/links.rs`), and all of them are
rendered, indexed and found, each focusing its own row.

**True duplicates.** A true duplicate is two records of one kind with
the same complete identity, so the same row key. The response can carry
one in every list kind: `unit_states`, `unit_dropins`, `manual_links`,
`undetermined_installs` and `mount_findings`. `UnitSection` is a plain
serde struct whose lists have no uniqueness check, `classify_services`
(`crates/refine/src/classify.rs`) maps `state_changes` and `drop_ins`
one to one, and the projection makes one view per record
(`unit_state_views`, `drop_in_views`, `link_finding_views` and
`mount_finding_views` in `crates/refine/src/projection/decisions.rs`)
and merges none, so a snapshot that holds a duplicate passes it to the
browser. The default target cannot repeat: it is an `Option`.

The section keeps every record. Each is rendered once, as its own row
under its own occurrence key (§ 6.11), counted once in the filter counts
(§ 6.4), the group header count (§ 6.3) and `resultCount`, and indexed
as its own global search entry with its own search text. Rendering by
occurrence key is what holds the row count to the record count: React
reconciles repeated sibling keys wrongly, and with the row key as the
React key a list holding a duplicate gains rows each time a filter or
query narrows it and is restored. The rest follows from the two
identities:

- **Search.** Both entries carry the same row key as `itemId`, so
  selecting either reveals and focuses the first row in section order
  that carries it; the operator reaches the second from there with `j`.
  This is accepted.
- **A duplicated unit row.** Both occurrences are unit rows with an
  outcome, sorted next to each other in response order (§ 6.2). The
  unit's drop-ins nest under the first occurrence only, so each drop-in
  record renders once; the second occurrence's branch has no drop-ins.
  Both checkboxes send the same op (`ItemId::Unit` names the unit alone)
  under one guard key, `serviceKey(unit)`, so a toggle in flight disables
  both, which is right for one op. Which record the session changes is
  the server's (`ItemId::Unit` and `ItemId::DropIn` in
  `crates/refine/src/session.rs` change the first matching record), and
  this spec does not change it.
- **A duplicated drop-in.** Two drop-in rows under the parent, each
  under its own occurrence key, sharing `dropinKey(path)` as above.
- **Per-row state.** `ServiceSection` keeps no per-row state keyed by
  identity today: no expanded state, and its pending set is keyed by the
  op's guard key, as above. The per-row state this spec adds, the tab
  stop and the pins, is keyed by occurrence key, so a duplicate cannot
  share it.
- **DOM ids.** The second occurrence's ids carry a suffix of their own
  and its `aria-describedby` names its own notes (§ 6.6, DOM ids), so a
  duplicate never shares an id with its twin.

In single-host mode `GlobalSearch` stops indexing the reference section
whose `id` is `units`; every other reference section is indexed as today.
`/api/snapshot/sections` and `adapter.rs` are unchanged, and nothing else
in the single-host UI reads the `units` reference section (it is not in
`MainContent`'s `contextSectionIds`). Because `declined.text` is indexed,
an omitted unit is found by its reason. Units results are labelled
"Systemd units" (string 1).

In aggregate mode `GlobalSearch` is unchanged: it indexes the fleet
view's `units` section through `buildAggregateSearchSections`, labels
its results "Services", and navigates through `AggregateApp`'s
`handleSearchNavigate`, as today.

**Backlog effect.** This closes
`inspectah-global-search-services-results-navigate-nowhere`: today the
divergent-unit entries do resolve (`service-item-` is in the prefix list),
but the `dropin-<unit>`, `omitted-<unit>`, `advisory-<unit>` and
`warning-<unit>` entries resolve to nothing; after this change no `units`
reference entry is indexed in single-host mode and every units entry
resolves. The fleet view navigates search results through its own
handler and is not part of that item.
`inspectah-omitted-service-search-text-leaves-out-the-reason` is about the
server's reference projection; the single-host UI stops reading that
text, which makes the item moot for this UI without changing `adapter.rs`,
so the item stays open for whoever triages it.

### 6.13 Keyboard and accessibility

Requirements, each proved by a test in § 10:

- **One tab stop for the rows.** The section keeps a roving `tabIndex`:
  one row is `0`, the rest `-1`. The `0` row is always a shown row: when
  the row holding it stops being shown (its group collapses, or the
  filter or the section search hides it), the tab stop moves to the
  first shown row in section order; when no row is shown, no row holds
  it. The section holds the tab stop by occurrence key (§ 6.11), never by
  row key, so of two rows with one row key exactly one can hold it. The
  filter bar's buttons, each group header and each link in a row are
  ordinary tab stops; the checkboxes stay focusable.
- **Row movement.** On a focused row, `ArrowDown`/`j` and `ArrowUp`/`k`
  move to the next and previous shown row across groups, skipping
  collapsed groups; `g` and `G` go to the first and last. Rows here are
  unit rows, drop-ins, the default target and reported rows, one per
  record: the two occurrences of a true duplicate are two stops. This
  matches `DecisionList`'s keys (`DecisionList.tsx:545-556`), which the
  units section lacks today.
- **Toggle.** Space or `x` on a focused unit, drop-in or target row
  toggles it, as today. Reported rows have no action.
- **Focus after a toggle** stays on the toggled row, the occurrence the
  operator toggled, which does not move and is not hidden (pinning,
  § 6.4).
- **Group headers** are `<button>`s with `aria-expanded` and
  `aria-controls` naming the `id` of the group's grid; Enter and Space
  toggle them. Each group's rows are a `role="grid"` whose
  `aria-labelledby` names the header's title element, so the grid's
  name is the group title alone, with every `role="row"` row inside a
  `role="rowgroup"` within that grid. Rows carry no `aria-rowindex`.
  `TriageBucketGroup`, whose button pattern the header follows, has no
  `aria-controls`; `UnitGroup` adds it.
- **The section** is a container with `role="region"` named "Systemd
  units" (string 1, `aria-label`), holding the filter bar, the empty
  states and the groups, with the test id `service-section`.
- **The filter** is a `ToggleGroup` with `aria-label` "Show": each item a
  button with `aria-pressed`, count inside its text.
- **Announcements.** A live region with `aria-live="polite"`, present
  and empty from the bar's first render, states the number of rows
  shown, orphan drop-ins included, when the filter changes (§ 6.4
  Announcement, § 7). The outcome label is text, never colour alone; the
  follow-up flag is text.
- **Labels.** Each checkbox keeps `Toggle <name>` or its locked form
  (§ 9.3); a row's notes stay linked through `aria-describedby` as today,
  each occurrence of a true duplicate to its own notes (§ 6.6, DOM ids).
- **Narrow widths.** At 768 px the bar wraps below the heading, the hint
  wraps, a row's badges wrap under its name, and the section causes no
  horizontal scroll.
- **Themes.** Colours come from PatternFly tokens only, so dark mode
  works as elsewhere.

### 6.14 Heading and sidebar

- The sidebar entry reads **"Systemd units"**. The label is served by
  `/api/groups`, so the change is in `SectionMeta` in
  `crates/pipeline/src/section_group.rs` (`id: "units"`). That label
  reaches the single-host sidebar only, so it needs no mode boundary.
  Every reader of `SectionMeta.label`, read from source: `get_groups` in
  `crates/web/src/handlers.rs`, which serves `/api/groups`, whose only
  client caller is `useGroups`, used by `SingleHostApp` alone
  (`AggregateApp` never fetches it and renders `AggregateSidebar` from
  the fleet view's own section list). The batch-toggle route in
  `handlers.rs` reads member section ids and `is_triage`, not labels;
  `crates/pipeline/src/render/audit.rs` prints group labels, not section
  labels. The fleet view's "Services" section is built by
  `aggregate_handlers.rs` and stays.
- The frontend copies: `MainContent.tsx` `SECTION_LABELS.units` becomes
  "Systemd units"; `MainContent` is rendered by `SingleHostApp` only.
  `GlobalSearch.tsx` `SECTION_LABELS.units` stays "Services", the fleet
  view's label; single-host units results take "Systemd units" through
  the mode boundary of § 6.15.
- The owner badge on a configuration row that a unit drop-in shadows
  (`resolveShadowOwner` in `shadowOwner.ts`, test id
  `shadow-owner-badge`) reads "Controlled in Systemd units" (string 26),
  in place of "Controlled in Service Drop-ins" (§ 2 item 13). The
  badge's explanation sentence and its "Go to owner" button are
  unchanged.
- The section id `units`, the group slug `services-scheduling`, the
  batch toggle and its menu text are unchanged.
- The section heading is "Systemd units", with no shadow-override count:
  a full shadow has its own orange "Shadow override" label on its row.
- The sidebar's decision count for `units` is unchanged: unit rows,
  drop-ins and the target; reported rows have no toggle and are not
  counted.

### 6.15 Single-host scoping of shared components

`AppShell` is shared with the fleet view and renders `GlobalSearch` and
`ContainerfilePanel` for both apps (§ 4). Every change this spec makes
to those three components is scoped to single-host mode by one
mechanism: `AppShell`'s existing `isAggregateMode` prop, which
`AggregateApp` already sets and `SingleHostApp` leaves false.

- `AppShell` passes `isAggregateMode` to `GlobalSearch` and to
  `ContainerfilePanel`, and passes through the new unit-row search props
  from `SingleHostApp` (`unit_states`, `default_target`,
  `mount_findings`, `manual_links`, `undetermined_installs`), which
  `AggregateApp` does not set.
- `GlobalSearch` with `isAggregateMode` false applies § 6.12: the unit
  rows indexed under their row keys with their own search text, the
  `units` reference section skipped, units results labelled "Systemd
  units". With it true, indexing, labels and navigation are as today.
- `ContainerfilePanel` with `isAggregateMode` false shows string 25 as
  its footer (§ 9.5). With it true, the footer is today's text (§ 2
  item 8).
- `isAggregateMode` keeps its current effect on the panel's default
  open state; its doc comment in `AppShell.tsx` names both uses.

Everything else this spec changes is rendered by `SingleHostApp` only
(`MainContent`, `ServiceSection`, `Sidebar`, `App.tsx`'s reveal), or is
the `SectionMeta` label of § 6.14, which only `SingleHostApp` reads. The
owner badge of string 26 is among them: `buildShadowOwners` has one
caller, `MainContent`, which hands the owners to `DecisionList` and
`DecisionItem`; the fleet view builds no shadow owner and draws no such
badge, so string 26 is single-host by where it is rendered and needs no
mode boundary. U34 holds the fleet view unchanged.

## 7. Strings

Every string this spec adds or changes. All are frontend labels except
1, which `/api/groups` serves. None is a view response field, and none is
a reason or advisory sentence: those stay single-sourced in the pipeline
crate and are referenced, not restated (`declined.text`,
`attention_reason`, `withheld.text`, `shadow_rationale`, `advisories[]`,
`not_applied`, and the existing labels "Other drop-in overrides", "These
overrides have no unit-state decision row.", "Also=", "Needs
configuration:", "Keeps data in:", "Not in the export:", "Instance of",
"Shadow override", "[Install]", "Drop-in Override", "Unknown Origin").

| # | String | Where it shows |
|---|---|---|
| 1 | `Systemd units` | Sidebar entry (served by `/api/groups`); section heading; global search section label in single-host mode (§ 6.15); the section's accessible name |
| 2 | `Default target` | Group title |
| 3 | `Services` / `Sockets` / `Timers` / `Paths` | Kind group titles |
| 4 | `Other units` | Group title |
| 5 | `Reported, not carried` | Group title |
| 6 | `inspectah reports these and does not carry them. No toggle: the lines under each row are the Containerfile's comments for it.` | Description under the Reported, not carried header |
| 7 | `Show` | Filter bar label and the `ToggleGroup`'s `aria-label` |
| 8 | `All` / `Needs follow-up` / `Carried` / `Excluded` | Filter values, each followed by its count badge |
| 9 | `Needs follow-up: the Containerfile writes a comment for the row and no command.` | Hint under the filter bar |
| 10 | `{n} needs follow-up` (n = 1), `{n} need follow-up` (n > 1) | Follow-up flag on a kind group header |
| 11 | `The target the source host boots to. Included, the image boots to it too.` | Explanation under the default target row |
| 12 | `No other unit differs from its preset, and no drop-in was found.` | Quiet host with a default target |
| 13 | `No unit differs from its preset, and no drop-in was found.` | Empty section, no default target; replaces `No service state changes detected.` |
| 14 | `No units match this filter.` and the link button `Show all` | Filter that shows no row |
| 15 | `FIXME` / `Advisory` / `Omitted` | Outcome label, by `declined.renders` |
| 16 | `preset: {default_state}` | State against preset (was `(preset: {default_state})`) |
| 17 | `wanted by` / `required by` / `upheld by` / `has alias` / `named in Also= by` | A link's relation, by `relation` |
| 18 | `Link its [Install] does not make` (`static_unit`), `Link made by hand` (`not_in_install`), `Only some of its links` (`partial_install`), `Tied to a host device` (`host_device`), `Only the /etc files name it` (`host_install_only`), `Only the package unit names it` (`vendor_install_only`), `[Install] cannot be relied on` (`undetermined`, and every undetermined-install row) | A link's cause label |
| 19 | `Showing all {n} rows.` | Live region, filter All (`row` when n = 1). In strings 19 to 22, n is the number of rows shown, orphan drop-ins included (§ 6.4 Announcement), so under All it can exceed the bar's All count |
| 20 | `Showing {n} rows that need follow-up.` (`1 row that needs follow-up` when n = 1) | Live region |
| 21 | `Showing {n} carried rows.` (`1 carried row`) | Live region |
| 22 | `Showing {n} excluded rows.` (`1 excluded row`) | Live region |
| 23 | `Unit excluded` | Drop-in badge while its parent is excluded (was `Service excluded`) |
| 24 | `Incompatible with image mode` | Badge and checkbox label of a locked row whose `attention_reason` is `service-image-mode-incompatible` (§ 9.3) |
| 25 | `Preview reflects every include and exclude decision you make in refine.` | Containerfile panel footer in single-host mode (§ 9.5, § 6.15) |
| 26 | `Controlled in Systemd units` | Owner badge on a configuration row a unit drop-in shadows, single-host mode (§ 6.14, § 6.15); was `Controlled in Service Drop-ins` |

Strings 17 and 18 are short names for a wire word, like the triage labels
in `attentionUtils.ts`; the sentence that explains each case stays the
served advisory line under the row.

## 8. Components

**New**

- `components/unitOutcome.ts`: pure functions, no React, holding every
  rule the section applies. A row's outcome (§ 6.1); the filter counts
  (§ 6.4); grouping into the seven groups with sort order and orphan
  placement (§ 6.2, § 6.8); filter and search matching and both search
  texts (§ 6.4, § 6.12); reported rows grouped per unit (§ 6.6 item
  11); the relation and cause label maps (§ 7); building a row's key
  and parsing an item id into one, and assigning every record its
  occurrence key (§ 6.11); whether the bar is drawn, which filter
  values are disabled and whether the hint shows (§ 6.4); which empty
  state applies, the quiet-host sentences and the filter's and search's
  empty states (§ 6.9); the live region's text and its count (§ 6.4);
  the reveal's three steps (§ 6.11); the shown row order, the roving
  tab stop and row movement (§ 6.13). Tested directly.
- `hooks/useUnitsSectionState.ts`: the section's state, apart from its
  rendering (§ 2 item 12): the filter value, the live region text, the
  pins, collapse state, the reveal's steps as applied, the roving tab
  stop and focus after a toggle. It holds React state and calls
  `unitOutcome.ts` for every rule; it decides none itself.
  `ServiceSection` calls it and renders what it returns. Behaviour is
  the same as with the state inside `ServiceSection`, so its tests are
  the section's tests.
- `components/UnitGroup.tsx`: a collapsible group, header (title, count,
  flag) and labelled grid. Follows `TriageBucketGroup`'s button pattern;
  `TriageBucketGroup` itself is not reused, since its labels and colours
  are fixed to attention levels.
- `components/UnitOutcomeFilter.tsx`: the `ToggleGroup` bar, hint and
  live region, drawn as `unitOutcome.ts` decides them.

**Changed**

- `components/ServiceSection.tsx`: renders through the groups, with its
  state from `useUnitsSectionState.ts`; the row changes of § 6.6;
  `Also=` links through `onNavigateToItem`; the row elements for roving
  focus and keys (§ 6.13); the empty states as `unitOutcome.ts` decides
  them (§ 6.9); the section region (§ 6.13); the locked label (§ 9.3);
  a true duplicate's own DOM ids (§ 6.6);
  `data-unit-key` on every row, and resolving `revealItemId` to a row
  through `unitOutcome.ts` (§ 6.11); every row's React `key`, and the
  key of the element wrapping a unit row's branch, is its occurrence key
  in place of today's identity keys (`svc.unit`, `di.path`, `f.unit`,
  `linkIdentity(link)`, `["undetermined", unit]`), and a duplicated unit
  row's drop-ins nest under its first occurrence (§ 6.12). The file and
  component names, the `service-section` test id and every row test id
  stay, so existing tests keep working.
- `components/MainContent.tsx`: heading and label (§ 6.14); renders
  `SectionSearch` for `units` and passes the text, `revealItemId`,
  `onNavigateToItem` and `config_files` paths to the section; clears the
  query when a reveal's destination fails it (§ 6.11 step 1);
  `handleArrowDown` gains the units case.
- `components/AppShell.tsx`: passes `isAggregateMode` to `GlobalSearch`
  and `ContainerfilePanel`, and passes the unit-row search props through
  (§ 6.15).
- `components/GlobalSearch.tsx`: the single-host index of § 6.12, with
  row keys as item ids and the optional `searchText` on
  `GlobalSearchResult`, read first by the filter, and the "Systemd
  units" label, all only while `isAggregateMode` is false (§ 6.15).
- `App.tsx`: `SingleHostApp` passes `unit_states`, `default_target` and
  the reported rows to `AppShell` for `GlobalSearch`; for the `units`
  section the reveal resolves the item id to a row key and finds the
  element by `data-unit-key` instead of the prefixes (§ 6.11).
- `components/shadowOwner.ts`: "Go to owner" sends the drop-in's row key
  (§ 6.11), and the owner badge reads string 26 (§ 6.14);
  `shadowOwner.test.ts` expects that key as the target and that label,
  and `MainContent.shadowed-configs.test.tsx` expects the label.
- `components/Sidebar.tsx`: the Scheduled Tasks count (§ 9.4).
- `components/ContainerfilePanel.tsx`: the single-host footer (§ 9.5,
  § 6.15).
- `components/attentionUtils.ts`: the locked-reason label (§ 9.3).
- `crates/pipeline/src/section_group.rs`: the `units` label, and one test
  assertion for it.

**Untouched**

The view response and everything in `crates/web/src` (`adapter.rs`,
`handlers.rs`, `web_types.rs`); the fleet UI (`AggregateApp`,
`AggregateSidebar` and the rest of `components/aggregate/`) and
`aggregate_handlers.rs`; `ContextList` and `ContextItem`;
`DecisionList`, `TriageBucketGroup` and every other section's badges;
the Containerfile render; the sidebar decision count for `units`;
`usePendingGuard` and the op payloads; `REVEAL_TESTID_PREFIXES`.

## 9. Defect fixes

Each was checked against source at `e6591669`. "Read from source" means
traced, not run.

### 9.1 The empty state still says "services"

**Today.** `ServiceSection.tsx:725-740`: with no unit row, drop-in,
reported row or default target, the section shows "No service state
changes detected." The section covers every unit kind, links and mount
units, so the sentence is wrong on two counts. Confirmed by reading the
render branch; the string is the only one in the tree.

**Required.** String 13 (§ 7), and string 12 when only the default target
exists (§ 6.9).

### 9.2 Section search is not wired into the units section

**Today.** `useKeyboard.ts:158-162` opens section search on `/` in every
section, but `MainContent.tsx` renders `SectionSearch` only for packages
(`:602`), configs (`:706`), language packages (`:852`) and unmanaged
files (`:896`). In the units section `/` sets `sectionSearchOpen` and
nothing appears. Confirmed by reading every `SectionSearch` call site.

**Required.** § 6.12, section search.

### 9.3 A locked unit row's badge prints a slug

**Today.** `normalize_incompatible_services`
(`crates/refine/src/normalize.rs:244-259`) locks `dnf-makecache.service`,
`dnf-makecache.timer` and `packagekit.service` and sets their
`attention_reason` to the slug `service-image-mode-incompatible`. The
adapter passes it through unchanged (`crates/web/src/adapter.rs:145`),
and `ServiceSection.tsx:868-879` prints `svc.attention_reason ?? "LOCKED"`
in the locked badge; the checkbox's label is `<unit> (locked:
service-image-mode-incompatible)` (`:812-816`). Read from source and
confirmed by `crates/refine/tests/normalize_test.rs:519`, which asserts
the slug; not reproduced in a browser, since the kitchen-sink scan has no
locked row.

**Required.** The section maps the slug to string 24 for the badge and
the checkbox label, through a lookup in `attentionUtils.ts` beside
`formatTriageReason`, which already maps triage reason ids to labels. Any
other `attention_reason` on a locked row is shown as given; a null one
shows "LOCKED" as today. The served value is not changed (§ 2, item 3). A
locked drop-in's reason is already prose and is unchanged.

### 9.4 The Scheduled Tasks badge reads 0

**Today.** `scheduled_tasks` is a triage section (`section_group.rs:153-
157`, `is_triage: true`), so `Sidebar.tsx` takes its badge from
`decisionCount`, which has no case for it and falls through to the
`stats.sections` lookup, which matches only `packages` and `config`, and
returns `"0"` (`Sidebar.tsx:95-136`). The section has toggleable
generated-timer rows (`MainContent.tsx`, `controlFor`), so the badge
reads 0 and the sidebar's cleared-state announcement treats the section
as cleared. Confirmed by reading `decisionCount`; the design pass saw the
badge read 0 on the kitchen-sink host, which has five generated-timer
rows with a toggle.

**Required.** `decisionCount` gains a `scheduled_tasks` case: the number
of rows in the `scheduled_tasks` reference section (items and subsection
items) whose `generated_timer` names a `generated_timers` entry, less
those whose entry is `include: false` with `cron_job_excluded` (they show
a reason in place of a toggle). It reads `"..."` until both the sections
and the view have loaded. This matches the rule the `units` count states:
every toggle the section renders.

`kernel_boot` and `selinux` are also triage sections that fall through
to `"0"` the same way. Whether they render toggles is outside this
section; this spec does not change them and names it here so it is not
lost.

### 9.5 The Containerfile panel footer leaves out unit decisions

**Today.** `ContainerfilePanel.tsx:448-451`: "Preview reflects package
and config decisions. Context sections are included as-is." The preview
is the server's render of the session's current snapshot
(`containerfile_preview` in the view response), so it reflects unit,
drop-in, default target, generated-timer and every other toggle.
Confirmed by reading the component and where its content comes from.

**Required.** String 25 in single-host mode (§ 6.15). The fleet view
renders the same component, and its preview comes from the same
projection (`aggregate_handlers.rs` passes the view's
`containerfile_preview` through), so today's footer is wrong there too.
The fix is single-host only (§ 2 item 8): the fleet footer keeps today's
text, a known limit (§ 3.2).

## 10. Verification

### 10.1 Fixtures

- **Vitest fixtures live in `crates/web/ui/src/__tests__/fixtures/`**,
  one directory per area (`system-tuning/` is the precedent). Add
  `systemd-units/`:
  - `kitchen-sink.units.json`: the six unit fields of the view response
    (`unit_states`, `unit_dropins`, `default_target`, `mount_findings`,
    `manual_links`, `undetermined_installs`) copied verbatim from a
    driftify kitchen-sink scan of RHEL 10.2 served by refine at the
    implementing commit. The test file's header records the inspectah
    commit and how it was taken. Check it for secrets and host-specific
    names before committing; the scan's redaction applies, and the
    sensitive-tarball hook checks only tarballs.
  - `planted.units.json`: the kitchen-sink rows plus hand-made rows the
    scan lacks: a locked row with the slug, an excluded runtime updater, a
    path unit, a slice, an administrator's mount finding, a withheld
    drop-in, an orphan drop-in with no `kind`, a link with relation
    `alias` and one with cause `vendor_install_only`, a declined default
    target, a row whose `required_config` path has a config row and one
    whose path has none, a masked template (`getty@.service`) beside its
    instance `getty@tty2.service`, and one unit name held by a
    `unit_states` row, a mount finding, an undetermined install and a
    manual link's `unit` at once. It also has the pair U8 needs: a unit
    row declined because its recorded `[Install]` drop-in is excluded,
    and the same row after that drop-in is included again, with
    `declined` cleared, as the response the mocked `applyOp` returns.
    For the row keys of § 6.11 it also has a `unit_states` row for a unit
    literally named `undetermined-install-demo.service` beside an
    undetermined install for `demo.service`, and three manual links for
    one unit: two with relation `wants` under different parents
    (`multi-user.target`, `graphical.target`), and one with relation
    `requires` under `multi-user.target`.
    For true duplicates (§ 6.12) it has a manual link for
    `dup-link-demo.service` with relation `wants` under
    `multi-user.target` twice, the two records identical, beside one
    `wants` link under `multi-user.target` for
    `dup-link-demo-other.service`, which sorts before it; and a carried
    `unit_states` row for `dup-state-demo.service` twice, with its one
    drop-in, `/etc/systemd/system/dup-state-demo.service.d/override.conf`,
    twice in `unit_dropins`. No other row holds either name, so a section
    query on it selects these rows alone.
    Strings copied from source, never invented.
  - `stock10.units.json` (target only) and `stock9.units.json` (target and
    `NetworkManager-wait-online.service`). U23's excluded and declined
    target-only cases are `stock10` with `include` set to false, and with
    `planted`'s declined target, built in the test.
- **Playwright bodies are recorded, not hand-written.** The preset
  `single-host/view.json` crashes the app (its package and config rows
  have no `disposition`; item 3 of
  `process-docs/skills/e2e-fixtures-are-hand-maintained.md`), the preset
  route map serves no `/api/groups`, without which `Sidebar` renders
  skeletons, and `single-host/sections.json` has no `scheduled_tasks`
  section, without which `MainContent` shows "Section data not
  available." for Scheduled Tasks. So the P tests do not build on the
  preset bodies. From one refine session serving the driftify
  kitchen-sink scan of RHEL 10.2 at the implementing commit, record and
  add under `crates/web/ui/e2e/fixtures/`:
  - `single-host/view-units-kitchen-sink.json`: the `GET /api/view` body,
    with every item list outside the six unit fields, `generated_timers`
    and `declined_generated` emptied (`[]`), so it is small and holds no
    package or config row the units tests do not need.
  - `single-host/view-units-stock10.json`: the same body with its six
    unit fields replaced by `stock10.units.json`'s, for P6.
  - `single-host/sections-units-kitchen-sink.json`: the
    `GET /api/snapshot/sections` body with only the `units` and
    `scheduled_tasks` sections kept, less any row that names an account
    or a host (below). The `units` reference section stays
    so the tests prove global search skips it. The `scheduled_tasks`
    section must hold a timer row with `unit_row: true`; if the scan has
    none, P5's setup fails rather than passing on nothing.
  - `sequences/units-exclude-tmp-mount/01-after-exclude.json`: the body
    `POST /api/op` returns after excluding `tmp.mount` in that session,
    trimmed as the view is. `App` refetches `/api/view` after an op
    (`view.invalidate()` in `onMutationSuccess`), so P3 serves it with
    `mockSequence(page, "/api/view", [the view, this body], { triggerOn:
    "/api/op" })`, called after `applyMockApi`.
  - `/api/groups` is served from
    `crates/web/ui/src/__tests__/fixtures/system-tuning/files-no-tuned.groups.json`
    by a `page.route` handler, as `e2e/system-tuning.spec.ts` does. That
    body is held to the real `get_groups` by `system_tuning_fixtures_test`
    and carries "Systemd units" once re-recorded (§ 10.2); `/api/groups`
    does not depend on the host.

  Every P test calls one setup function: `applyMockApi(page,
  "single-host", ...)` with absolute override paths for `/api/view` and
  `/api/snapshot/sections` (as `system-tuning.spec.ts` passes them), then
  the groups route. Check the recorded bodies for secrets and
  host-specific names before committing, as for the Vitest fixtures.
  **A recorded body leaves out any row that names an account or a host
  of the scanned machine**, and no asserted count may depend on a row
  left out. The kitchen-sink recording loses one row this way: an `at`
  job in the `scheduled_tasks` section, which names an account. No count
  reads it: the six unit fields are verbatim, so U2's and P1's numbers
  are unchanged, and the Scheduled Tasks badge (U29) counts
  generated-timer rows, which an `at` job is not.
  Register each new file in `e2e/fixtures/manifest.json` (category
  `body`, schema `ViewResponse` or `ContextSections`), and add one
  `#[test]` per file to `crates/web/tests/fixture_structure_test.rs`,
  which snapshots only the files it names: a manifest entry alone
  snapshots nothing. Run `cargo test -p inspectah-web --test
  fixture_structure_test` and review the new snapshots and the manifest
  snapshot diff.
- This adds the first frontend fixtures with non-empty drop-ins. It does
  not close `inspectah-no-fixture-has-a-non-empty-services-drop-ins`,
  which is about the snapshot corpus.

### 10.2 Vitest

`unitOutcome.ts` tests are table-driven over the fixtures.

| Id | Proves |
|---|---|
| U1 | Outcome rule order: declined beats excluded beats carried, for unit rows and the default target; every reported row is Needs follow-up; a locked excluded row is Excluded. |
| U2 | Kitchen-sink counts: All 23, Needs follow-up 9, Carried 14, Excluded 0. Drop-ins are not counted. |
| U3 | Groups and order: Default target, Services (11), Sockets (6), Timers (1), Other units (1: `tmp.mount`), Reported, not carried (3); no Paths group. With `planted`, a Paths group and the slice in Other units. |
| U4 | Sort by name within a group; `getty@tty2.service` and `wg-quick@wg0.service` sit by name among services. With `planted`, the masked template `getty@.service` has its own row in Services, sorted by name, and `getty@tty2.service` is its sibling: not inside the template's `service-group-getty@.service`, and rendered the same as with no template row. |
| U5 | Follow-up flag: Services "5 need follow-up", Other units "1 needs follow-up", no flag on Sockets, Timers, Default target or Reported; no flag under a filter other than All. |
| U6 | Filter bar absent on `stock10` and `stock9`; present on kitchen-sink; "Excluded" disabled at 0; hint shown only while follow-up > 0. |
| U7 | Each filter value shows the right rows; groups with none are not drawn; a shown unit row brings its drop-ins; orphan drop-ins only under All; Reported only under All and Needs follow-up. |
| U8 | Pinning (§ 6.4). (a) Under "Needs follow-up", toggling `cups.service` to excluded (mocked `applyOp` returns a view with `declined` cleared and `include: false`) keeps the row in place with focus on it, its outcome label gone, and counts updated. (b) With `planted`, under "Needs follow-up", including the parent's excluded `[Install]` drop-in again (mocked `applyOp` returns the pair's second view) keeps the parent's branch shown in place, the drop-in included and focused, the parent's outcome label gone, the counts updated. (c) With a section query that matches `cups.service` only through its `declined.text`, toggling it as in (a) keeps it shown. (d) A row whose outcome changes through an Undo, with no toggle on its branch, follows the filter. (e) Changing the filter value, changing the query, or a reveal that changes either drops every pin; the bar ceasing to be drawn does not. |
| U9 | Filter resets to All on remount and when the bar stops being drawn; it survives a view update in place. |
| U10 | Default target group: explanation string 11 on included, excluded and declined targets; outcome label and `declined.text` when declined. |
| U11 | State against preset: `enabled` then `preset: disable`, no parentheses, no italic or opacity style; state alone when `default_state` is null. |
| U12 | Outcome label by `renders`: FIXME, Advisory, Omitted, on unit rows and the default target. |
| U13 | No "Non-Default State" badge on any unit row; "Unknown Origin" still shown; drop-in badges unchanged. |
| U14 | One reason: `cups.service` shows its `declined.text` and not its `attention_reason`; an undeclined row with an `attention_reason` shows it. |
| U15 | `Also=` member links: `cups.socket` is a link to its row, `cups.path` is plain code; clicking reveals and focuses `service-item-cups.socket`. |
| U16 | `required_config` link to the config row when `config_files` holds the path, plain code otherwise; `data_paths` plain. |
| U17 | Instance: `Instance of wg-quick@.service` on the row's first line. |
| U18 | No package: `(local)` on `driftify-also-main.service`; `(shadowed)` and `(absent)` on two `planted` rows the test edits, each given `owning_package: null` and that `fragment` (`planted` holds only `local` and `vendor` fragments). |
| U19 | Kind chip only in Other units. |
| U20 | Reported rows grouped per unit: `driftify-upheld.service`'s link and undetermined-install rows adjacent; relation phrases and cause labels from § 7 for every `relation` and cause value, including `has alias` and `Only the package unit names it`. |
| U21 | Orphan drop-ins: `sshd.service`'s full shadow in Services under "Other drop-in overrides"; an orphan with no `kind` in Other units. |
| U22 | "Unit excluded" on a drop-in whose parent is excluded. |
| U23 | Empty states: `stock10` shows string 12 and no filter bar; `stock10` with the target excluded shows the bar with All 1 and Excluded 1, and with the target declined, the bar with All 1, Needs follow-up 1 and the hint, string 12 in both; an empty view shows string 13 and never "No service state changes detected."; a filter left empty shows string 14 and "Show all" resets it. |
| U24 | Section search: `/` opens it in units; matching on `declined.text`, drop-in path (parent shown with its drop-ins), reported advisory text; with `planted`, matching on the words a reported row shows (§ 6.12): `required by` shows the `requires` link and no `wants` link, `foreign name` (the reason `foreign_name` as the row shows it) shows `demo.service`'s undetermined-install row, and the administrator's mount finding is found by its `mount_point` (`/data/demo`), and its search text holds its `kind`; each search text is the one global search indexes for that row (§ 6.12 table); result count; no-match `EmptyState`; ArrowDown focuses the first shown row; search and filter combine. |
| U25 | Global search, single-host: a `unit_states` row, a drop-in, the default target, a manual link, an undetermined install and a mount finding are each indexed under their row key (§ 6.11) and reach their row; no entry from the `units` reference section is indexed; an omitted unit is found by a word of its reason. With `planted`'s unit name shared by four row kinds: four results, each with its own search text (a word only in the undetermined install's reason finds that result and not the unit row's), and each result focuses its own row; a bare-name navigation to that unit focuses `service-item-<unit>`. Prefixed name: the unit row for `undetermined-install-demo.service` and the undetermined install for `demo.service` give two results with different item ids and different search text, the first focusing `service-item-undetermined-install-demo.service` and the second the undetermined-install row of `demo.service`; a bare-name navigation to `undetermined-install-demo.service` focuses the unit row. Several links for one unit: `planted`'s three manual links give three results; of those three, a search for `graphical` lists only the link under `graphical.target`, and `requires` and `required by` each list only the `requires` link, and each result focuses its own link row. True duplicates: a search for `dup-link-demo.service` gives two results with one item id, and selecting either focuses the first of its two link rows in section order; the same holds for `dup-state-demo.service`'s two unit rows. "Go to owner" on a shadowed config focuses its drop-in. |
| U26 | Reveal into retained state, mounting `App` with the units section open (§ 6.11). Each case starts with the destination hidden and ends with it shown and focused: an `Also=` link whose member is hidden by the filter (filter set to All), in a collapsed group (group opened), and failing the section query (query cleared, search box still open); a query the member matches is kept; a global search result selected from within the section, with a filter selected and the destination's group collapsed. |
| U27 | Scheduled Tasks, reset on remount: in the units section, select a filter and collapse Timers; go to Scheduled Tasks, which unmounts the section; follow the timer row's link (`unit_row: true`). The section mounts with filter All and every group open, and the unit row is focused (extends `ScheduledTimer.unitLink.test.tsx`). |
| U28 | Defect 9.3: with `planted`, the locked row's badge and checkbox label read "Incompatible with image mode"; the slug appears nowhere in the DOM; a null reason shows "LOCKED". |
| U29 | Defect 9.4: the Scheduled Tasks badge counts toggleable generated-timer rows, excludes a row blocked by `cron_job_excluded`, reads `...` while loading, and reads 0 only when there are none. |
| U30 | Defect 9.5: in single-host mode the panel footer reads string 25. |
| U31 | Heading and labels: the heading is "Systemd units" with no shadow count; in single-host mode `GlobalSearch` labels units results "Systemd units"; a configuration row a unit drop-in shadows shows the owner badge "Controlled in Systemd units" (string 26), `resolveShadowOwner` returns that label, and "Controlled in Service Drop-ins" appears nowhere. |
| U32 | Keyboard and grid structure. Movement: `j`/`k`/arrows move across groups and skip collapsed ones; `g`/`G`; Space and `x` toggle. Structure: each header's `aria-controls` equals its grid's `id`; each grid's `aria-labelledby` names an element whose text is the group title; every `role="row"` element has a `role="rowgroup"` ancestor inside its `role="grid"`. Roving tab stop: exactly one row in the section has `tabIndex` 0 and it is shown (no `[hidden]` ancestor); this holds after collapsing the group that holds it, after a filter change and after a section query hides it, each time on the first shown row. Headers toggle on Enter and Space with `aria-expanded` updated. |
| U33 | Live region: an element with `aria-live="polite"` is in the DOM, empty, before the first filter change; after each change its text is strings 19 to 22 for the number of rows shown (§ 6.4 Announcement), singular and plural. On kitchen-sink, returning to "All" reads "Showing all 24 rows." while the bar reads "All 23"; "Needs follow-up" reads "Showing 9 rows that need follow-up."; with a section query, the number is the rows shown under the query; a reveal that sets the filter to "All" announces the same way. |
| U34 | Fleet view unchanged (§ 6.15), mounting `AggregateApp` with a fleet view holding a `units` section (extends `AggregateApp.integration.test.tsx`): a global search for one of its items lists a result labelled "Services", selecting it navigates as today, and no "Systemd units" text appears; the Containerfile panel footer reads today's text (§ 2 item 8). |
| U35 | True duplicates render one row per record across narrowing (§ 6.11, § 6.12), with `planted`. Each step counts the section's shown `role="row"` elements, not its records, and checks the Reported group header count and `resultCount` against that row count. (a) Unfiltered: exactly two rows carry `dup-link-demo.service`'s link key and one carries `dup-link-demo-other.service`'s; exactly two unit rows carry `["unit_state","dup-state-demo.service"]`, and exactly two drop-in rows carry its drop-in's key, both inside the first unit row's branch and none in the second's; the filter's All and Needs follow-up counts each count both link records and All counts both unit records. (b) Section query `dup-link-demo`: 3 link rows; then `dup-link-demo.service`: 2; then `dup-link-demo` again: 3; then cleared: the rows of (a). (c) Filter "Carried": both `dup-state-demo.service` unit rows and their two drop-in rows shown, Services header count equal to its shown unit rows; filter "All": the rows of (a). A build keyed by row key fails (b): React reconciles the repeated key wrongly and leaves 3 rows at the second step and 5 at the third. |
| U36 | Keyboard over true duplicates (§ 6.13), with `planted`. From `dup-link-demo-other.service`'s link row, `j` focuses the first `dup-link-demo.service` link row, `j` again the second (a different element with the same `data-unit-key`), `k` the first; from the first `dup-state-demo.service` unit row, `j` goes through its two drop-in rows to the second unit row. After every move exactly one row in the section has `tabIndex` 0, it is the focused element, and it is shown. Toggling the second `dup-state-demo.service` occurrence with `x` (mocked `applyOp` returns the `planted` view as it was) leaves focus and the tab stop on the second occurrence, not the first. |
| U37 | Group header of an orphan-only kind group (§ 6.3). (a) A view built in the test from kitchen-sink's default target and `sshd.service`'s orphan full shadow alone: the Services header has no count badge, its button's accessible name is "Services", and the orphan row is shown under it. (b) Kitchen-sink under "All" with a section query that only the orphan's path matches (asserted first): Services shows the orphan alone, has no count badge, and its button's accessible name is "Services, 5 need follow-up"; clearing the query brings back the badge "11". |

No Rust test asserts the `units` section label at `e6591669`; add one
assertion for "Systemd units" beside the existing `SectionMeta` tests in
`crates/pipeline/src/section_group.rs`. The reference section's
`display_name` test in `adapter.rs` (`build_web_sections_display_names`)
keeps "Services", since the reference projection is not changed.

The label change alters the `/api/groups` body, so the recorded
`*.groups.json` files under
`crates/web/ui/src/__tests__/fixtures/system-tuning/` fail
`system_tuning_fixtures_test` until re-recorded with
`UPDATE_SYSTEM_TUNING_FIXTURES=1 cargo test -p inspectah-web --test
system_tuning_fixtures_test`; the diff is that one label in each.
`aggregate.groups.json` is among them: an aggregate session serves
`/api/groups` too, though no fleet code reads it (§ 6.14). The
Playwright specs that assert the single-host sidebar reads "Services"
(`e2e/sections.spec.ts`, `e2e/smoke.spec.ts`) change to "Systemd units".
Tests that feed their own groups body or fleet sections with "Services"
in them are inputs, not assertions about this label, and stay.

### 10.3 Playwright

Every P test uses the setup of § 10.1: the recorded kitchen-sink view
and sections bodies and the recorded groups body.

| Id | Proves |
|---|---|
| P1 | The section renders the groups of U3 in order, the bar with counts 23, 9, 14, 0, and the sidebar reads "Systemd units" with count 24. |
| P2 | Clicking "Needs follow-up" leaves 9 rows across Services, Other units and Reported; clicking "All" restores every row: 24 shown, the orphan drop-in included, beside the bar's "All 23" (§ 6.4 Announcement). |
| P3 | Keyboard only, with `/api/view` sequenced to the recorded post-exclude body (§ 10.1): Tab reaches the bar, select "Needs follow-up" with Space, Tab into the rows, `j` to `tmp.mount`, toggle with `x`; after the op, the row stays in place and keeps focus, and the counts are the post-exclude body's. |
| P4 | Global search for `wg0`, selecting its Systemd units result, lands on and focuses `service-item-wg-quick@wg0.service`; no result for the units section is labelled "Services". |
| P5 | Scheduled Tasks to unit row: from the sidebar's Scheduled Tasks entry, the link on the recorded timer row with `unit_row: true` reveals and focuses its unit row. |
| P6 | Axe: the units section with the bar, under "All" and under "Needs follow-up", and the section against `view-units-stock10.json`, have no critical or serious violations. |
| P7 | At 768 x 1024 (the size `responsive.spec.ts` uses) the section has no horizontal scroll and the bar wraps; at 1280 x 800 it does not wrap. |
| P8 | Dark theme: axe colour contrast passes for the outcome labels, the flag and the subtle preset text. |

## 11. Changelog and docs

**CHANGELOG.** Due under `process-docs/changelog-policy.md`: the section
changes visibly and four of the fixes are user-visible bugs. Under
`## [Unreleased]`:

- `### Changed`: one entry for the "Systemd units" section: its name,
  groups by kind, the outcome filter and follow-up flag, the reported
  rows' group, the default target's explanation, the row changes
  (preset, outcome label, one reason, `Also=` and config links, no
  "Non-Default State" badge), and that the fleet view is unchanged.
- `### Fixed`: the empty-state text, section search in the units
  section, the locked row's badge, the Scheduled Tasks badge, and the
  Containerfile panel footer in single-host refine. One entry each, or
  one entry naming all five; the policy allows either.

**Docs under `docs/`.**

- `docs/how-to/review-and-refine.md`: the section table row for
  "Services & Scheduling" (`:51`) names the section "Systemd units"; the
  paragraph on "Other drop-in overrides" (`:127-131`) says where orphan
  drop-ins now sit; the paragraph on units rows (`:133-146`) describes
  the groups, the filter, "Needs follow-up" and the reported group.
- `docs/reference/triage-classification.md`: the "Services" row of the
  section table (`:220`) is renamed, and notes that the units section
  shows no badge for the non-default-state reason.
- `docs/reference/inspector-coverage.md:323` says "the units section";
  name it "the Systemd units section".
- Not changed: `docs/tutorials/first-migration.md:195` and the
  getting-started scan output describe the HTML report and the scan's
  CLI summary, not refine.

## 12. Left to the builder

- CSS: spacing, the subtle-text token for the preset, the flag's place in
  the header, and how the bar wraps, within § 6.13.
- How `ServiceSection`'s rendering is split further, as long as
  `unitOutcome.ts` holds every rule, including the bar's disabled values
  and hint and the empty states, and is tested on its own, and the
  section's state stays in `useUnitsSectionState.ts` (§ 8).
- How the pinned set is stored (a `Set` of the occurrence keys of the
  rows heading the pinned branches, cleared by the events of § 6.4, is
  enough). It never holds row keys (§ 6.11).
- How occurrence keys travel from `unitOutcome.ts` to the rows (a field
  on each grouped row, or a map from record to key), as long as they are
  assigned once per view response before grouping, filter and search
  (§ 6.11).
- The exact form of the `data-unit-key` lookup in `App.tsx`, as long as
  it compares strings and builds no selector from the key (§ 6.11).
- How the reveal steps of § 6.11 are sequenced against `App.tsx`'s
  reveal frame (a layout effect keyed on the reveal request, or deriving
  the state during render), as long as the destination is shown when the
  lookup runs.
- Where P6 to P8 live (`e2e/systemd-units.spec.ts`, or `a11y.spec.ts`
  and `responsive.spec.ts`), as long as each uses the setup of § 10.1.
- Whether the five fixes land as separate commits; each is independent of
  the section redesign except 9.2, which needs § 6.12.

## 13. Open questions

None.

## 14. The spec author's calls

Not decided by Mark; each is this spec's design and can be changed in
review.

1. Outcomes cover unit rows, the default target and reported rows, not
   drop-ins; a withheld drop-in is not found by the filter (§ 3.2, § 6.1).
2. Filter counts ignore search; a value with count 0 is disabled; the
   filter resets on leaving the section and is not persisted (§ 6.4).
3. Pinning (§ 6.4): a toggle pins the branch that holds the toggled row,
   the parent's branch for a nested drop-in, against both the filter and
   the section search, until the operator changes the filter or the
   query, a reveal changes either, or the section unmounts. The filter
   returning to "All" because the bar stops being drawn clears no pin.
   Neither the design pass nor the mockups settled this.
4. Orphan drop-ins show under "All" only (the mockups also showed them
   under "Carried") and a drop-in with no `kind` goes in Other units
   (§ 6.8).
5. Groups are open by default and collapsible, collapse state not
   persisted (§ 6.2).
6. The kind chip is dropped from rows in kind-named groups (§ 6.6 item
   10).
7. The relation word for `alias` is "has alias", not the mockups' "alias
   of", which reads backwards: the link row's `parent` is the alias name
   (§ 6.6 item 12).
8. The Reported group's description (string 6) replaces the mockups'
   "No toggle: each row gives the line to add by hand", which is not true
   of every row (a `host_device` link and some mount findings carry no
   command).
9. "Service excluded" becomes "Unit excluded" (string 23).
10. The locked slug maps to "Incompatible with image mode" in the
    frontend rather than changing the served value (§ 9.3).
11. The Scheduled Tasks count rule (§ 9.4) and the footer text (string
    25).
12. Global search drops the `units` reference section in single-host mode
    instead of changing the server (§ 6.12).
13. Row movement keys (`j`/`k`, arrows, `g`/`G`) are added to the units
    section, which lacks them today (§ 6.13).
14. The heading drops the shadow-override count (§ 6.14).
15. The two empty-state sentences and the filter's empty state (strings
    12 to 14) and the live region strings (19 to 22).
16. Shared components are scoped through `AppShell`'s existing
    `isAggregateMode` prop, passed to `GlobalSearch` and
    `ContainerfilePanel`, rather than a new prop or a second component;
    `GlobalSearch`'s shared label map keeps "Services", the fleet label
    (§ 6.15).
17. A reveal clears the section query only when the query hides the
    destination; global search keeps clearing it always, as today
    (§ 6.11).
18. Every row of the section has a row key, its kind and complete
    identity JSON-encoded as an array of strings, carried in
    `data-unit-key`; global search and "Go to owner" send row keys, and
    the reveal for `units` finds the row by that attribute instead of
    by test-id prefixes, so no lookup order decides which row an id
    names. A bare unit name, from the Scheduled Tasks and `Also=` links
    only, always means the `unit_states` row (§ 6.11, § 6.12). New
    reveal prefixes or a separator-joined id were not taken: a unit name
    can hold `-`, `:` and `.`, so one prefix and name can spell another
    prefix and name.
19. A masked template's row sits in its kind group sorted by name, and
    instances never nest under it (§ 6.2).
20. The roving tab stop moves to the first shown row when its row stops
    being shown, and each grid is labelled by its group title alone, not
    the whole header with its count and flag (§ 6.13).
21. The Playwright bodies are recorded from a refine session and
    trimmed, not hand-written; `/api/groups` is served from the
    system-tuning recording, which a test already holds to the real
    handler (§ 10.1).
22. Units search entries carry their search text on the result
    (`searchText` on `GlobalSearchResult`) rather than in
    `searchTextMap`, so no map key can be shared (§ 6.12).
23. A manual link's identity is `relation`, `parent` and `unit`, as
    `linkIdentity` has it, without `cause`. A true duplicate (one kind,
    one complete identity, twice) is kept, not merged or dropped in the
    browser, since the records can differ outside the identity and both
    are the server's. A row has two identities: its row key, for search,
    navigation and `data-unit-key`, and an occurrence key (the row key,
    `#`, and the record's ordinal among records with that row key in its
    source list), assigned once per view response before filter and
    search, for the React key, the tab stop, pins and focus. Each record
    renders once and counts once; both search results reveal the first
    row with the key; a duplicated unit row's drop-ins nest under its
    first occurrence. Occurrence keys go on every kind, not only those
    that can repeat, so no code path depends on knowing which can
    (§ 6.11, § 6.12).
24. A true duplicate's later occurrence gives its DOM ids a generated
    suffix while the first keeps today's ids, so existing ids and the
    tests that read them do not change (§ 6.6).
25. An item id in the `[` form names a row only in the exact bytes
    `JSON.stringify` writes; any other spelling of the same array is
    not found, so one key has one spelling (§ 6.11).
26. A link's search text keeps the wire word beside its relation
    phrase, so a search for `requires` still finds it (§ 6.12).
27. Rows carry no `aria-rowindex`, and the section's container is a
    region named "Systemd units" (§ 6.13).
28. Recorded bodies leave out rows that name an account or a host, and
    no asserted count may depend on such a row (§ 10.1).
29. The state hook lives in `hooks/`, beside the UI's other hooks
    (§ 8).

## 15. Related

- `process-docs/specs/implemented/2026-11-13-unified-systemd-units.md`:
  § 11.1 is the data contract this section reads; § 11.2 is the
  recommendation this spec replaces; § 10.2 and § 10.9 are where
  `declined` and the advisory lines come from.
- `process-docs/skills/web-disposition-contract.md`: link row identity,
  `ManualLinkView.cause`, exact test-id reveal.
- `process-docs/skills/e2e-fixtures-are-hand-maintained.md`: what the new
  e2e fixture can and cannot prove.
- `process-docs/skills/web-ui-direct-request-guard.md`: unchanged toggle
  guarding.
