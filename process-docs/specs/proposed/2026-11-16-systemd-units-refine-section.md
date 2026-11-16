# Systemd Units Refine Section

## Metadata

| Field | Value |
|---|---|
| Date | 2026-11-16 |
| Status | Proposed. Scope and seven design choices are decided (§ 2). The rest is the spec author's design; each such call is listed in § 14. No question is open (§ 13). |
| Scope | Single-host refine only: the "Systemd units" section (today "Services"), and five defects in the refine UI around it. Grouping and presentation. No field added to the view response, no snapshot or schema change, no change to what the Containerfile renders. |
| Parent | Phase 6 of `process-docs/specs/implemented/2026-11-13-unified-systemd-units.md`, which recommends a section design in § 11.2 and leaves it to a frontend design pass. This spec is that pass's result. |
| Basis | A design pass with mockups on a real kitchen-sink scan: the view response of a driftify kitchen-sink scan of RHEL 10.2 served by inspectah `e6591669`, unedited, and two stock hosts (RHEL 10.2 and RHEL 9.8) built from the stock-host records in `process-docs/skills/real-host-validation-runs.md`. The pass rendered three options in headless Chromium. It did not check keyboard use, screen readers or widths under 1440 px; § 6.13 states those requirements instead. |
| Backlog items | Closes `inspectah-global-search-services-results-navigate-nowhere`. Touches, does not close: `inspectah-omitted-service-search-text-leaves-out-the-reason`, `inspectah-no-fixture-has-a-non-empty-services-drop-ins`, `inspectah-refine-sections-reorg` (§ 3.2). |
| Verified | Every "today" statement and file reference read at `e6591669`. Line numbers are a convenience; the symbol name is the identifier. |

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
  open.
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
  side by side. No row is nested under its template: the template has no
  row of its own.
- Drop-ins stay nested under their parent row, in server order, full
  shadows included, as today.
- Orphan drop-ins (§ 6.8) sit at the end of the group of their `kind`.
- Every group is open by default and collapsible from its header.
  Collapse state lives in the section component and resets when the
  operator leaves the section. A collapsed group keeps its rows in the
  DOM under `hidden`, so the reveal can find and open it (§ 6.11).

### 6.3 Group header

A group header is a button with `aria-expanded` and `aria-controls`
naming the group's grid. It shows, in order:

1. The disclosure angle (the `AngleRightIcon` / `AngleDownIcon` pair
   `TriageBucketGroup` uses).
2. The group title (§ 7).
3. A count badge (PatternFly `Badge`, read): the number of rows the group
   shows under the current filter and search. Drop-ins are not counted;
   in "Reported, not carried" each reported row counts once. The Default
   target group has no count.
4. **The follow-up flag**, in kind groups 2 to 6 only, and only while the
   filter is "All": when N of the group's unit rows have the outcome
   Needs follow-up and N > 0, a compact orange outline `Label` reading
   `N needs follow-up` (N = 1) or `N need follow-up` (N > 1). N counts
   every such row in the group, whether or not search hides it. The
   Default target group shows the outcome on the row itself (§ 6.5), and
   "Reported, not carried" is all follow-up, so neither has a flag.

The flag's text is part of the button's accessible name, so a screen
reader hears "Services 11, 5 need follow-up".

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
  not carried" is shown under "All" and "Needs follow-up" only. A group
  left with no row is not drawn (§ 6.2).
- **A toggled row stays put.** A row the operator toggles while a filter
  other than "All" is selected stays shown, in its place, until the
  filter changes or the operator leaves the section, even when its
  outcome no longer matches; its outcome label and the counts update.
  Rows whose outcome changes because of another row's toggle, or an Undo
  or Redo, follow the filter as computed.
- **Persistence.** The filter is "All" each time the section mounts; it
  survives toggles, Undo, Redo and view refreshes while the operator stays
  in the section; it is not stored in `localStorage` or the URL. If the
  bar stops being drawn while a value is selected, the filter returns to
  "All".
- **Announcement.** Changing the value updates a polite live region with
  the number of rows now shown (§ 7, strings 19 to 22).

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
    mount finding first, then its links in server order, then its
    undetermined install. So `driftify-upheld.service`'s link row and its
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
| No `unit_states`, no `unit_dropins`, no reported row, and a default target | Heading, the Default target group, then "No other unit differs from its preset, and no drop-in was found." No filter bar. |
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

### 6.11 Reaching a row: Scheduled Tasks, `Also=`, search

Every route that sends the operator to a row in this section lands on it
whatever the section's state:

- **Scheduled Tasks** keeps its link from a timer row to its unit row
  (`ContextItem.tsx`, `onNavigateToUnit`), unchanged. It already does
  what the parent spec's § 11.2 asks.
- **The reveal.** `ServiceSection` takes the `revealItemId` that
  `MainContent` already receives. When it names a row the current filter
  hides, the section sets the filter to "All"; when the row is in a
  collapsed group, it opens the group. `App.tsx`'s reveal effect then
  finds and focuses the row as today; it also opens a collapsed unit
  group the same way it opens an `attention-group-`, by its header
  button.
- **`Also=` links** use the same reveal inside the section.
- **New reveal targets.** `REVEAL_TESTID_PREFIXES` in `App.tsx` gains
  `manual-link-`, `undetermined-install-` and `mount-finding-`, after
  `service-item-`, and the reveal tries the item id as an exact test id
  last, so a search result with item id `default-target` reaches the
  default target row. Matching stays exact-string, never a CSS selector
  (`process-docs/skills/web-disposition-contract.md`).

### 6.12 Search

**Section search.** `MainContent` renders `SectionSearch` for `units`
when `sectionSearchOpen`, as it does for packages, configs, language
packages and unmanaged files, and passes the filter text to
`ServiceSection`. A row matches when the lowercased query is a substring
of any of:

- a unit row: `unit`, `kind`, `owning_package` or fragment word,
  `current_state`, `default_state`, `instance_of`, `declined.text`,
  `attention_reason`, and the paths of its drop-ins (a drop-in match
  shows the parent with all its drop-ins);
- the default target: its `target`;
- an orphan drop-in: `path` and `unit`;
- a reported row: `unit`, `parent`, `relation`, the advisory lines and
  `install_files`.

Search and the outcome filter both apply. `resultCount` is the number of
matching rows (unit rows, the target, orphan drop-ins, reported rows).
ArrowDown from the search box focuses the first shown row of this
section; `handleArrowDown` in `MainContent` gains that case. Escape
closes and clears as elsewhere.

**Global search.** `GlobalSearch` indexes the view response's rows in
place of the `units` reference section:

| Entry | `itemId` | Search text |
|---|---|---|
| Each `unit_states` row | `unit` (resolves to `service-item-<unit>`) | as section search, minus drop-in paths |
| The default target | `default-target` | `target`, `default target` |
| Each `unit_dropins` entry | `path` (as today) | `path`, `unit` |
| Each manual link | `JSON.stringify([relation, parent, unit])` | `unit`, `parent`, `relation`, advisories |
| Each undetermined install | `unit` | `unit`, reason, `install_files`, advisories |
| Each mount finding | `unit` | `unit`, `kind`, `mount_point`, advisories |

It stops indexing the reference section whose `id` is `units` in
single-host mode; every other reference section is indexed as today.
`/api/snapshot/sections` and `adapter.rs` are unchanged, and nothing else
in the single-host UI reads the `units` reference section (it is not in
`MainContent`'s `contextSectionIds`). Because `declined.text` is indexed,
an omitted unit is found by its reason. Entries use the existing
`sectionId:itemId` composite key for the search-text map, so a unit name
cannot collide with another section's bare id.

**Backlog effect.** This closes
`inspectah-global-search-services-results-navigate-nowhere`: today the
divergent-unit entries do resolve (`service-item-` is in the prefix list),
but the `dropin-<unit>`, `omitted-<unit>`, `advisory-<unit>` and
`warning-<unit>` entries resolve to nothing; after this change no `units`
reference entry is indexed and every units entry resolves.
`inspectah-omitted-service-search-text-leaves-out-the-reason` is about the
server's reference projection; the single-host UI stops reading that
text, which makes the item moot for this UI without changing `adapter.rs`,
so the item stays open for whoever triages it.

### 6.13 Keyboard and accessibility

Requirements, each proved by a test in § 10:

- **One tab stop for the rows.** The section keeps a roving `tabIndex`:
  one row is `0`, the rest `-1`. The filter bar's buttons, each group
  header and each link in a row are ordinary tab stops; the checkboxes
  stay focusable.
- **Row movement.** On a focused row, `ArrowDown`/`j` and `ArrowUp`/`k`
  move to the next and previous shown row across groups, skipping
  collapsed groups; `g` and `G` go to the first and last. Rows here are
  unit rows, drop-ins, the default target and reported rows. This
  matches `DecisionList`'s keys (`DecisionList.tsx:545-556`), which the
  units section lacks today.
- **Toggle.** Space or `x` on a focused unit, drop-in or target row
  toggles it, as today. Reported rows have no action.
- **Focus after a toggle** stays on the toggled row, which does not move
  (§ 6.4).
- **Group headers** are `<button>`s with `aria-expanded` and
  `aria-controls`; Enter and Space toggle them. Each group's rows are a
  `role="grid"` labelled by its header (`aria-labelledby`), with
  `role="row"` rows inside a `rowgroup`.
- **The filter** is a `ToggleGroup` with `aria-label` "Show": each item a
  button with `aria-pressed`, count inside its text.
- **Announcements.** A polite live region states the number of rows
  shown when the filter changes (§ 7). The outcome label is text, never
  colour alone; the follow-up flag is text.
- **Labels.** Each checkbox keeps `Toggle <name>` or its locked form
  (§ 9.3); a row's notes stay linked through `aria-describedby` as today.
- **Narrow widths.** At 768 px the bar wraps below the heading, the hint
  wraps, a row's badges wrap under its name, and the section causes no
  horizontal scroll.
- **Themes.** Colours come from PatternFly tokens only, so dark mode
  works as elsewhere.

### 6.14 Heading and sidebar

- The sidebar entry reads **"Systemd units"**. The label is served by
  `/api/groups`, so the change is in `SectionMeta` in
  `crates/pipeline/src/section_group.rs` (`id: "units"`), and in the two
  frontend copies (`MainContent.tsx` `SECTION_LABELS.units`,
  `GlobalSearch.tsx` `SECTION_LABELS.units`). Only the single-host app
  reads `/api/groups`; the audit report prints group labels, not section
  labels, and the fleet view builds its own "Services" section
  (`aggregate_handlers.rs`), which stays. The section id `units`, the
  group slug `services-scheduling`, the batch toggle and its menu text are
  unchanged.
- The section heading is "Systemd units", with no shadow-override count:
  a full shadow has its own orange "Shadow override" label on its row.
- The sidebar's decision count for `units` is unchanged: unit rows,
  drop-ins and the target; reported rows have no toggle and are not
  counted.

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
| 1 | `Systemd units` | Sidebar entry (served by `/api/groups`); section heading; global search section label; the section's accessible name |
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
| 19 | `Showing all {n} rows.` | Live region, filter All (`row` when n = 1) |
| 20 | `Showing {n} rows that need follow-up.` (`1 row that needs follow-up` when n = 1) | Live region |
| 21 | `Showing {n} carried rows.` (`1 carried row`) | Live region |
| 22 | `Showing {n} excluded rows.` (`1 excluded row`) | Live region |
| 23 | `Unit excluded` | Drop-in badge while its parent is excluded (was `Service excluded`) |
| 24 | `Incompatible with image mode` | Badge and checkbox label of a locked row whose `attention_reason` is `service-image-mode-incompatible` (§ 9.3) |
| 25 | `Preview reflects every include and exclude decision you make in refine.` | Containerfile panel footer (§ 9.5) |

Strings 17 and 18 are short names for a wire word, like the triage labels
in `attentionUtils.ts`; the sentence that explains each case stays the
served advisory line under the row.

## 8. Components

**New**

- `components/unitOutcome.ts`: pure functions, no React. A row's outcome
  (§ 6.1); the filter counts (§ 6.4); grouping into the seven groups with
  sort order and orphan placement (§ 6.2, § 6.8); filter and search
  matching (§ 6.4, § 6.12); reported rows grouped per unit (§ 6.6 item
  11); the relation and cause label maps (§ 7). Tested directly.
- `components/UnitGroup.tsx`: a collapsible group, header (title, count,
  flag) and labelled grid. Follows `TriageBucketGroup`'s button pattern;
  `TriageBucketGroup` itself is not reused, since its labels and colours
  are fixed to attention levels.
- `components/UnitOutcomeFilter.tsx`: the `ToggleGroup` bar, hint and
  live region.

**Changed**

- `components/ServiceSection.tsx`: renders through the groups; the row
  changes of § 6.6; filter state, pinned rows, collapse state and the
  `revealItemId` handling; roving focus and keys (§ 6.13); the empty
  states (§ 6.9); the section search text; the locked label (§ 9.3). The
  file and component names and the `service-section` test id stay, so
  the reveal prefixes and existing tests keep working.
- `components/MainContent.tsx`: heading and label (§ 6.14); renders
  `SectionSearch` for `units` and passes the text, `revealItemId` and
  `config_files` paths to the section; `handleArrowDown` gains the units
  case.
- `components/GlobalSearch.tsx`: the index of § 6.12 and the label.
- `App.tsx`: passes `unit_states`, `default_target` and the reported rows
  to `GlobalSearch`; reveal prefixes and the unit-group opening (§ 6.11).
- `components/Sidebar.tsx`: the Scheduled Tasks count (§ 9.4).
- `components/ContainerfilePanel.tsx`: the footer (§ 9.5).
- `components/attentionUtils.ts`: the locked-reason label (§ 9.3).
- `crates/pipeline/src/section_group.rs`: the `units` label, and one test
  assertion for it.

**Untouched**

The view response and everything in `crates/web/src` (`adapter.rs`,
`handlers.rs`, `web_types.rs`); the fleet UI (`AggregateApp` and
`components/aggregate/`) and `aggregate_handlers.rs`; `ContextList` and
`ContextItem`; `DecisionList`, `TriageBucketGroup` and every other
section's badges; the Containerfile render; the sidebar decision count
for `units`; `usePendingGuard` and the op payloads.

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

**Required.** String 25.

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
    whose path has none. Strings copied from source, never invented.
  - `stock10.units.json` (target only) and `stock9.units.json` (target and
    `NetworkManager-wait-online.service`).
- **Playwright fixtures live in `crates/web/ui/e2e/fixtures/`**, served by
  `e2e/helpers/mock-api.ts` and registered in `e2e/fixtures/manifest.json`.
  Add `single-host/view-units-kitchen-sink.json`: `single-host/view.json`
  with the six unit fields replaced by the kitchen-sink ones. Today's
  `view.json` has 2 unit rows and no drop-in. Per
  `process-docs/skills/e2e-fixtures-are-hand-maintained.md`, these files
  are test doubles; run `cargo test -p inspectah-web --test
  fixture_structure_test` after adding one and review the insta snapshot
  diff, and check whether the preset view still crashes the app (that
  skill's item 3) before relying on it.
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
| U4 | Sort by name within a group; `getty@tty2.service` and `wg-quick@wg0.service` sit by name among services; no row nests under a template. |
| U5 | Follow-up flag: Services "5 need follow-up", Other units "1 needs follow-up", no flag on Sockets, Timers, Default target or Reported; no flag under a filter other than All. |
| U6 | Filter bar absent on `stock10` and `stock9`; present on kitchen-sink; "Excluded" disabled at 0; hint shown only while follow-up > 0. |
| U7 | Each filter value shows the right rows; groups with none are not drawn; a shown unit row brings its drop-ins; orphan drop-ins only under All; Reported only under All and Needs follow-up. |
| U8 | Pinned row: under "Needs follow-up", toggling `cups.service` to excluded (mocked `applyOp` returns a view with `declined` cleared and `include: false`) keeps the row in place with focus on it, its outcome label gone, and counts updated; changing the filter drops it. |
| U9 | Filter resets to All on remount and when the bar stops being drawn; it survives a view update in place. |
| U10 | Default target group: explanation string 11 on included, excluded and declined targets; outcome label and `declined.text` when declined. |
| U11 | State against preset: `enabled` then `preset: disable`, no parentheses, no italic or opacity style; state alone when `default_state` is null. |
| U12 | Outcome label by `renders`: FIXME, Advisory, Omitted, on unit rows and the default target. |
| U13 | No "Non-Default State" badge on any unit row; "Unknown Origin" still shown; drop-in badges unchanged. |
| U14 | One reason: `cups.service` shows its `declined.text` and not its `attention_reason`; an undeclined row with an `attention_reason` shows it. |
| U15 | `Also=` member links: `cups.socket` is a link to its row, `cups.path` is plain code; clicking reveals and focuses `service-item-cups.socket`. |
| U16 | `required_config` link to the config row when `config_files` holds the path, plain code otherwise; `data_paths` plain. |
| U17 | Instance: `Instance of wg-quick@.service` on the row's first line. |
| U18 | No package: `(local)` on `driftify-also-main.service`; `(shadowed)` and `(absent)` from `planted`. |
| U19 | Kind chip only in Other units. |
| U20 | Reported rows grouped per unit: `driftify-upheld.service`'s link and undetermined-install rows adjacent; relation phrases and cause labels from § 7 for every `relation` and cause value, including `has alias` and `Only the package unit names it`. |
| U21 | Orphan drop-ins: `sshd.service`'s full shadow in Services under "Other drop-in overrides"; an orphan with no `kind` in Other units. |
| U22 | "Unit excluded" on a drop-in whose parent is excluded. |
| U23 | Empty states: `stock10` shows string 12; an empty view shows string 13 and never "No service state changes detected."; a filter left empty shows string 14 and "Show all" resets it. |
| U24 | Section search: `/` opens it in units; matching on `declined.text`, drop-in path (parent shown with its drop-ins), reported advisory text; result count; no-match `EmptyState`; ArrowDown focuses the first shown row; search and filter combine. |
| U25 | Global search: a `unit_states` row, the default target, a manual link, an undetermined install and a mount finding are each indexed and reach their row; no entry from the `units` reference section is indexed; an omitted unit is found by a word of its reason. |
| U26 | Reveal: navigating to a row hidden by the filter sets the filter to All; to a row in a collapsed group opens it; then the row has focus. |
| U27 | Scheduled Tasks: a timer row with `unit_row: true` links to the unit row, which is revealed and focused with a filter selected and the Timers group collapsed beforehand (extends `ScheduledTimer.unitLink.test.tsx`). |
| U28 | Defect 9.3: with `planted`, the locked row's badge and checkbox label read "Incompatible with image mode"; the slug appears nowhere in the DOM; a null reason shows "LOCKED". |
| U29 | Defect 9.4: the Scheduled Tasks badge counts toggleable generated-timer rows, excludes a row blocked by `cron_job_excluded`, reads `...` while loading, and reads 0 only when there are none. |
| U30 | Defect 9.5: the panel footer reads string 25. |
| U31 | Heading and labels: the heading is "Systemd units" with no shadow count; `GlobalSearch` labels units results "Systemd units". |
| U32 | Keyboard: `j`/`k`/arrows move across groups and skip collapsed ones; `g`/`G`; Space and `x` toggle; one row has `tabIndex` 0; group headers toggle on Enter and Space with `aria-expanded` updated. |
| U33 | Live region text on each filter change, singular and plural (strings 19 to 22). |

No Rust test asserts the `units` section label at `e6591669`; add one
assertion for "Systemd units" beside the existing `SectionMeta` tests in
`crates/pipeline/src/section_group.rs`. The reference section's
`display_name` test in `adapter.rs` (`build_web_sections_display_names`)
keeps "Services", since the reference projection is not changed.

### 10.3 Playwright

Against `view-units-kitchen-sink.json`.

| Id | Proves |
|---|---|
| P1 | The section renders the groups of U3 in order, the bar with counts 23, 9, 14, 0, and the sidebar reads "Systemd units" with count 24. |
| P2 | Clicking "Needs follow-up" leaves 9 rows across Services, Other units and Reported; clicking "All" restores 23. |
| P3 | Keyboard only: Tab reaches the bar, select "Needs follow-up" with Space, Tab into the rows, `j` to `tmp.mount`, toggle with `x`; the row stays in place and keeps focus. |
| P4 | Global search for `wg0`, selecting its Systemd units result, lands on and focuses `service-item-wg-quick@wg0.service`. |
| P5 | Scheduled Tasks to unit row: from the Scheduled Tasks section, the timer link reveals its unit row. |
| P6 | Axe: the units section with the bar, under "All" and under "Needs follow-up", and the stock view, have no critical or serious violations (extends `a11y.spec.ts`). |
| P7 | At 768 x 1024 (as `responsive.spec.ts` sets it) the section has no horizontal scroll and the bar wraps; at 1280 x 800 it does not wrap. |
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
  Containerfile panel footer. One entry each, or one entry naming all
  five; the policy allows either.

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
- How `ServiceSection` is split internally, as long as `unitOutcome.ts`
  holds the pure logic and is tested on its own.
- How the pinned-row set is stored (a `Set` of row keys cleared on filter
  change is enough).
- The exact form of the exact-test-id reveal fallback in `App.tsx`.
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
3. A row toggled under a filter stays shown until the filter changes
   (§ 6.4). Neither the design pass nor the mockups settled this.
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
