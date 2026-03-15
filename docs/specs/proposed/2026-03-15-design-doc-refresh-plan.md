# Design Doc Refresh Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development
> (if subagents available) or superpowers:executing-plans to implement this plan.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restructure `yoinkc/design.md` into an architecture-first living
system document, update `driftify/design.md` with recent additions, and
capture the prioritized backlog and tech debt assessment in the refreshed
Future Work sections.

**Architecture:** The yoinkc design doc is restructured from 7 sections into
10 sections with a new ordering that leads with overview/architecture/CLI
before diving into internals. Existing prose is preserved and relocated where
possible; new sections are written from source code. The driftify doc keeps
its existing structure with targeted additions.

**Tech Stack:** Markdown documentation. Source of truth is the Python
codebase and shell scripts.

**Spec:** `docs/specs/proposed/2026-03-15-design-doc-refresh-design.md`

---

## File Map

- Modify: `yoinkc/design.md` — full restructure (tasks 1–8)
- Modify: `driftify/design.md` — targeted updates (task 9)

No new files. No test files. No code changes.

---

## Chunk 1: yoinkc design.md

### Task 1: Scaffold new structure, write Overview & Architecture

**Context:** The current `design.md` starts with Runtime Model (container
flags, progress output) and buries architecture under the inspector catalog.
This task creates the new document structure and writes the first two
sections, relocating existing prose where it fits.

**Files:**
- Modify: `design.md`
- Read for reference:
  - `src/yoinkc/pipeline.py` (pipeline orchestrator)
  - `src/yoinkc/inspectors/__init__.py` (inspector orchestration, `run_all`)
  - `src/yoinkc/renderers/__init__.py` (renderer orchestration, `run_all`)
  - `src/yoinkc/executor.py` (executor abstraction)
  - `src/yoinkc/baseline.py` (baseline generation)
  - `README.md` (Architecture section has baseline subtraction description)

- [ ] **Step 1: Read all reference files listed above**

Understand the current pipeline flow: how `__main__.py` calls `pipeline.py`,
which calls `inspectors.run_all()`, then `renderers.run_all()`. Note how the
executor abstraction works (host vs container mode). Note how baseline
generation uses `nsenter` to query the base image.

- [ ] **Step 2: Write section 1 — Overview & Principles**

Replace the current opening of `design.md` with:

```markdown
# Tool Design: yoinkc

## Overview & Principles

[One-paragraph purpose: yoinkc inspects package-based RHEL/CentOS/Fedora
hosts and produces bootc migration artifacts — a Containerfile, config tree,
reports, and a structured JSON snapshot.]

[Baseline subtraction principle: relocate from README. Wherever possible,
the tool subtracts base-image defaults so only operator-added or
operator-modified items appear. Packages diffed against base image, services
against presets, timers against RPM ownership, kernel/SELinux against shipped
defaults.]

[Pipeline concept: inspect → schema → render. Inspectors produce structured
data, the schema normalizes it, renderers consume it to produce artifacts.]

[Companion tools — one sentence each:
- yoinkc-refine: interactive UI for editing findings and re-rendering
- yoinkc-build: builds a bootc container image with entitlement handling
- yoinkc-fleet: aggregates multi-host inspections into a single fleet image]
```

- [ ] **Step 3: Write section 2 — Architecture**

```markdown
## Architecture

[Module map: inspectors/ → schema.py → renderers/, orchestrated by
pipeline.py. Describe the data flow.]

### Runtime Model

[Relocate existing Runtime Model content. Container with --pid=host
--privileged --security-opt label=disable -v /:/host:ro. Progress output.
Tarball as default output. The container is ghcr.io/marrusl/yoinkc:latest.]

### Baseline Generation

[Relocate existing Baseline Generation content from its current deep
position. nsenter into host PID namespace to run podman, query base image
for package list and systemd presets. Fallback modes: --baseline-packages
file, --no-baseline. The fail-fast guidance for missing baseline.]

### Executor

[New subsection. Describe the executor abstraction from executor.py:
how inspectors use it to run commands on the host via nsenter, how it
handles container vs. direct-run modes.]
```

Note: any content in the current `design.md` that belongs in later sections
(3-10) should be left in place for now — later tasks will relocate or remove
it. Only move content INTO sections 1-2, don't delete content that belongs
elsewhere.

- [ ] **Step 4: Verify sections 1-2 against source**

Cross-check:
- Pipeline flow matches `pipeline.py:run_pipeline()`
- Executor description matches `executor.py`
- Baseline description matches `baseline.py`
- Runtime model flags match `run-yoinkc.sh` and `__main__.py`

- [ ] **Step 5: Commit**

```
git add design.md
git commit -m "docs(design): restructure with Overview & Architecture sections

Reorganize design.md to lead with purpose, principles, and architecture
before diving into component details. Relocate Runtime Model and Baseline
Generation into Architecture section. Add Executor subsection.

Assisted-by: Claude"
```

---

### Task 2: CLI Reference

**Context:** The existing CLI Flags Summary is near the bottom of the doc.
This task moves it to section 3, refreshes it against current `cli.py`, and
adds all entry points.

**Files:**
- Modify: `design.md`
- Read for reference:
  - `src/yoinkc/cli.py` (argument parser for `yoinkc`)
  - `src/yoinkc/fleet/cli.py` (argument parser for `yoinkc-fleet`)
  - `src/yoinkc/__main__.py` (main entry point)
  - `src/yoinkc/fleet/__main__.py` (fleet entry point)
  - `run-yoinkc.sh` (wrapper script, env vars)
  - `run-yoinkc-fleet.sh` (fleet wrapper script, env vars)
  - `pyproject.toml` (`[project.scripts]` section)

- [ ] **Step 1: Read cli.py and fleet/cli.py argument parsers**

Catalog every flag, its type, default, and help text. Note any flags in the
current design doc that no longer exist, and any new flags not documented.

- [ ] **Step 2: Write section 3 — CLI Reference**

Place after Architecture section. Include:

- `yoinkc` entry point: all flags from `cli.py`, grouped logically (core,
  target image, inspection, output). Include `--validate` and
  `--push-to-github` subsections (relocate existing content).
- `yoinkc-fleet` entry point: flags from `fleet/cli.py` (`aggregate`
  subcommand, `-p`, `-o`, `--output-dir`, `--json-only`, `--no-hosts`).
- `yoinkc-refine` and `yoinkc-build`: brief description of each (these are
  separate tools, just note their existence and purpose).
- Environment variables table: `YOINKC_HOSTNAME`, `YOINKC_IMAGE`,
  `YOINKC_DEBUG` and their effects.
- Wrapper scripts: `run-yoinkc.sh` and `run-yoinkc-fleet.sh` — what they do,
  how to use them.
- Note: future entry points like `yoinkc-render` are documented in Future
  Work (section 10), not here.

- [ ] **Step 3: Remove old CLI Flags Summary section**

Delete the old CLI Flags Summary from its former position (near end of doc)
since it's now in section 3. Also remove the Build Validation subsection
from its old location — it's now part of the CLI Reference.

- [ ] **Step 4: Verify against source**

Cross-check every flag in the written CLI Reference against `cli.py` and
`fleet/cli.py`. Ensure no flag is missing or has a wrong default value.
Verify env vars match what `__main__.py` and the shell scripts actually read.

- [ ] **Step 5: Commit**

```
git add design.md
git commit -m "docs(design): add CLI Reference as section 3

Refresh from current cli.py and fleet/cli.py argument parsers. Add all
entry points, environment variables, and wrapper script documentation.
Remove old CLI Flags Summary from former position.

Assisted-by: Claude"
```

---

### Task 3: Schema

**Context:** The schema is central to yoinkc (50+ Pydantic models, schema
versioning, fleet metadata) but completely undocumented in `design.md`.

**Files:**
- Modify: `design.md`
- Read for reference:
  - `src/yoinkc/schema.py` (all models)
  - `src/yoinkc/pipeline.py:load_snapshot()` (schema version check)

- [ ] **Step 1: Read schema.py**

Understand the model hierarchy:
- `InspectionSnapshot` as root (what top-level fields does it have?)
- Section models (`RpmSection`, `ConfigSection`, `ServiceSection`, etc.)
- Item models within sections (`PackageEntry`, `ConfigFileEntry`, etc.)
- Enums (`PackageState`, `ConfigFileKind`)
- Fleet models (`FleetPrevalence`, `FleetMeta`)
- Metadata models (`OsRelease`)
- `SCHEMA_VERSION` constant

- [ ] **Step 2: Write section 4 — Schema**

Place after CLI Reference. Include:

- Purpose: the schema is the contract between inspectors and renderers.
  Inspectors produce it, renderers consume it, fleet merge operates on it.
- Schema versioning: `SCHEMA_VERSION` integer, checked on snapshot load in
  `pipeline.py:load_snapshot()`. Version mismatch → clear error telling user
  to re-run inspection.
- Model hierarchy: `InspectionSnapshot` contains optional section models
  (each inspector populates its section). Each section contains lists of
  item models.
- Key design choices: Pydantic v2 `BaseModel` for validation and
  serialization, `model_dump_json()` / `model_validate()` round-trip,
  optional sections (inspector didn't run → field is `None`).
- Fleet metadata: `FleetPrevalence` (count, total, percentage, hosts list)
  and `FleetMeta` (prevalence + optional content_variants). The `fleet`
  field is `Optional[FleetMeta]` on 10 item models — present only in
  fleet-aggregated snapshots.
- Do NOT list every model — just the hierarchy and key patterns. The source
  code is the reference for individual fields.

- [ ] **Step 3: Verify against source**

Check that `SCHEMA_VERSION` value, the version check logic, the fleet field
count (10 item models), and the Pydantic version (v2) are all accurate.

- [ ] **Step 4: Commit**

```
git add design.md
git commit -m "docs(design): add Schema section

Document the Pydantic v2 model hierarchy, schema versioning, fleet
metadata models, and the contract between inspectors and renderers.

Assisted-by: Claude"
```

---

### Task 4: Inspectors refresh

**Context:** The inspector catalog is the strongest part of the existing doc
but may have gaps from recent additions. This task verifies each inspector's
description against current code and adds missing capabilities.

**Files:**
- Modify: `design.md`
- Read for reference:
  - `src/yoinkc/inspectors/rpm.py`
  - `src/yoinkc/inspectors/service.py`
  - `src/yoinkc/inspectors/config.py`
  - `src/yoinkc/inspectors/network.py`
  - `src/yoinkc/inspectors/storage.py`
  - `src/yoinkc/inspectors/scheduled_tasks.py`
  - `src/yoinkc/inspectors/container.py`
  - `src/yoinkc/inspectors/non_rpm_software.py`
  - `src/yoinkc/inspectors/kernel_boot.py`
  - `src/yoinkc/inspectors/selinux.py`
  - `src/yoinkc/inspectors/users_groups.py`

- [ ] **Step 1: Read each inspector module**

For each inspector, note:
- What it detects (compare with design doc description)
- Any new detection capabilities not in the doc
- Any capabilities described in the doc that no longer exist
- How it uses the executor

Pay special attention to:
- Service inspector: drop-in override detection (recently added)
- Kernel/boot inspector: tuned profile detection (recently added)
- RPM inspector: any changes to baseline subtraction logic
- Config inspector: any changes to file classification

- [ ] **Step 2: Update section 5 — Inspectors**

Keep the existing section heading `## Inspectors` (or equivalent). For each
inspector:
- If the description is accurate, leave it unchanged
- If capabilities were added, add them to the existing description
- If capabilities were removed or changed, update accordingly
- Do NOT rewrite descriptions that are already accurate — preserve existing
  prose

- [ ] **Step 3: Remove Implementation Language section**

Delete the "Implementation Language" section entirely. This was a
pre-implementation decision record (choosing Python over Rust/Go) that has
no ongoing design value.

- [ ] **Step 4: Verify**

Spot-check 3-4 inspectors: read the source code for each and confirm the
design doc description matches what the code actually does.

- [ ] **Step 5: Commit**

```
git add design.md
git commit -m "docs(design): refresh inspector catalog

Verify all 11 inspector descriptions against current code. Add drop-in
override detection, tuned profile support, and other recent capabilities.
Remove Implementation Language section.

Assisted-by: Claude"
```

---

### Task 5: Renderers

**Context:** Renderers currently get brief mentions in "Output Artifacts" but
have no architectural treatment. This task creates a new Renderers section
and relocates relevant content from Output Artifacts.

**Files:**
- Modify: `design.md`
- Read for reference:
  - `src/yoinkc/renderers/__init__.py` (orchestration)
  - `src/yoinkc/renderers/containerfile.py` (Containerfile renderer)
  - `src/yoinkc/renderers/audit_report.py` (markdown audit report)
  - `src/yoinkc/renderers/html_report.py` (HTML report renderer)
  - `src/yoinkc/renderers/readme.py` (README renderer)
  - `src/yoinkc/renderers/kickstart.py` (kickstart suggestion)
  - `src/yoinkc/renderers/secrets_review.py` (secrets review)
  - `src/yoinkc/renderers/_triage.py` (shared triage computation)
  - `src/yoinkc/templates/report.html.j2` (HTML template — read the
    structure, not all 2,556 lines)

- [ ] **Step 1: Read renderer modules and template structure**

Understand:
- How `renderers/__init__.py:run_all()` orchestrates the 6 renderers
- How each renderer receives `(snapshot, env, output_dir)` and what it writes
- How `_triage.py` computes triage counts shared by audit and HTML renderers
- The HTML template structure: PF6 CSS, sidebar navigation, section cards,
  fleet-aware conditionals, embedded JS for interactivity

- [ ] **Step 2: Write section 6 — Renderers**

Place after Inspectors section. Include:

- Overview: 6 renderers, each produces one output artifact. All receive the
  snapshot and a Jinja2 environment. Orchestrated by `renderers/__init__.py`.
- Jinja2 setup: `FileSystemLoader` pointing to `templates/`, `autoescape=True`.
- Brief description of each renderer:
  - **Containerfile** (`containerfile.py`, 1,293 lines): structured in
    deliberate layer order for cache efficiency. Relocate existing
    Containerfile layer ordering content from Output Artifacts.
  - **Audit Report** (`audit_report.py`): markdown report with per-section
    findings, storage migration plan, triage summary.
  - **HTML Report** (`html_report.py` + `report.html.j2`): self-contained
    single-file report. PF6 CSS (unminified, v6.4.0) embedded. Sidebar
    navigation, section cards with toggle visibility. Fleet-aware: if
    snapshot has fleet metadata, renders prevalence color bars, host
    popovers, fraction/percentage toggle, content variant grouping.
  - **README** (`readme.py`): generated README with usage instructions,
    user provisioning strategy guide.
  - **Kickstart** (`kickstart.py`): kickstart suggestion file for
    deploy-time provisioning (human users, static routes, proxy vars).
  - **Secrets Review** (`secrets_review.py`): standalone redacted secrets
    report for security review.
- Shared triage: `_triage.py` computes sidebar counts (auto-included,
  needs-review, FIXME) used by both audit report and HTML report. Counts
  respect `.include` flags for item types that support fleet filtering.
- Template architecture note: the HTML template is a single 2,556-line
  Jinja2 file with embedded CSS and JS. This is a known complexity hotspot
  (see Future Work / Tech Debt).

- [ ] **Step 3: Remove old Output Artifacts section**

Delete the old Output Artifacts section. Its content has been distributed:
- Containerfile details → Renderers section
- Git repo layout → Pipeline & Packaging (task 7)
- Interactive refinement → keep as-is or relocate to Renderers
- Other output artifact descriptions → Renderers section

Careful: some Output Artifacts content (like the git repo layout, interactive
refinement, and building on non-RHEL hosts) may belong in Pipeline &
Packaging (task 7) rather than Renderers. Leave those for task 7 — just
remove what's been relocated to Renderers.

- [ ] **Step 4: Verify**

Check that the 6 renderer names match `renderers/__init__.py` imports. Check
that `_triage.py` is accurately described. Confirm PF6 version by checking
the CSS file header in `templates/patternfly.css`.

- [ ] **Step 5: Commit**

```
git add design.md
git commit -m "docs(design): add Renderers section

Document all 6 renderers, Jinja2 template system, shared triage
computation, and HTML report architecture. Relocate Containerfile
layer ordering from old Output Artifacts section.

Assisted-by: Claude"
```

---

### Task 6: Fleet Analysis

**Context:** Fleet analysis is a major subsystem (loader, merge engine, CLI,
prevalence UI) with no design doc coverage. The implemented specs in
`docs/specs/implemented/` have the detailed design history, but `design.md`
needs a self-contained description.

**Files:**
- Modify: `design.md`
- Read for reference:
  - `src/yoinkc/fleet/loader.py` (snapshot discovery and validation)
  - `src/yoinkc/fleet/merge.py` (merge engine)
  - `src/yoinkc/fleet/cli.py` (argument parser)
  - `src/yoinkc/fleet/__main__.py` (entry point)
  - `docs/specs/implemented/2026-03-13-fleet-analysis-design.md` (design)
  - `docs/specs/implemented/2026-03-14-fleet-prevalence-ui-design.md` (UI)
  - `docs/specs/implemented/2026-03-14-fleet-tarball-output-design.md` (output)
  - `run-yoinkc-fleet.sh` (container wrapper)

- [ ] **Step 1: Read fleet source files and implemented specs**

Understand:
- Loader: how `discover_snapshots()` finds tarballs/JSON files, validates
  schema version, extracts hostname
- Merge engine: identity keys per item type, content merge vs. identity
  merge, prevalence computation, content variant grouping, `--no-hosts`
  option, leaf/auto package preservation
- CLI: `aggregate` subcommand, `-p` threshold, output modes (tarball,
  directory, JSON-only)
- UI: how `html_report.py` detects fleet data, what fleet-specific elements
  it renders

- [ ] **Step 2: Write section 7 — Fleet Analysis**

Place after Renderers section. Include:

- Purpose: one image per host doesn't scale. Fleet analysis finds common
  ground across hosts serving the same role.
- Architecture: `loader.py` → `merge.py` → standard renderers. The fleet
  subsystem produces a merged `InspectionSnapshot` that the existing
  renderers consume — no fleet-specific renderers needed.
- Loader: `discover_snapshots()` accepts a directory, finds `.tar.gz` and
  `.json` files, extracts and validates each, returns list of
  `(hostname, snapshot)` pairs.
- Merge engine:
  - Identity merge: items matched by identity key (e.g., package name for
    RPM, path for config files). Items appearing on multiple hosts get
    `FleetMeta` with prevalence count/percentage and host list.
  - Content merge: items with same identity but different content (e.g.,
    config file with different contents across hosts) get grouped as
    content variants.
  - Prevalence threshold: `-p N` means include items on >= N% of hosts.
    Items below threshold are preserved in the snapshot (visible in report)
    but marked as excluded from the Containerfile.
  - Special handling: leaf/auto package classification is unioned across
    hosts; warnings are collected from all hosts.
- Fleet UI: the HTML renderer detects fleet metadata on the snapshot and
  conditionally renders: summary banner, prevalence color bars (blue/gold/red
  by threshold), fraction/percentage toggle, host list popovers, content
  variant grouping for files with per-host differences.
- Container wrapper: `run-yoinkc-fleet.sh` runs the fleet tool inside the
  yoinkc container for zero-install workstation use.

- [ ] **Step 3: Verify**

Check identity key logic in `merge.py` for 2-3 item types. Verify the
prevalence threshold logic. Confirm the fleet UI elements match what
`html_report.py` and `report.html.j2` actually render.

- [ ] **Step 4: Commit**

```
git add design.md
git commit -m "docs(design): add Fleet Analysis section

Document the fleet subsystem: loader, merge engine with identity/content
merge, prevalence threshold, fleet UI elements, and container wrapper.

Assisted-by: Claude"
```

---

### Task 7: Pipeline & Packaging + Secret Handling

**Context:** Pipeline & Packaging reorganizes existing content about output
modes, entitlement bundling, and validation. Secret Handling is mostly
unchanged but needs verification.

**Files:**
- Modify: `design.md`
- Read for reference:
  - `src/yoinkc/pipeline.py` (orchestrator)
  - `src/yoinkc/packaging.py` (tarball creation, output stamping)
  - `src/yoinkc/entitlement.py` (cert bundling)
  - `src/yoinkc/redact.py` (secret redaction)
  - `src/yoinkc/git_github.py` (GitHub push guardrails)
  - `Containerfile` (image build)

- [ ] **Step 1: Read pipeline, packaging, entitlement, redact, git_github**

Understand:
- `run_pipeline()` flow: inspect-or-load → redact → render → package
- Tarball creation: how `create_tarball()` works, what goes in the tarball
- `get_output_stamp()`: hostname-timestamp naming
- Entitlement bundling: when and how certs are bundled
- Redaction: what patterns are redacted, redaction layers
- GitHub push: pre-push secret scanning

- [ ] **Step 2: Write section 8 — Pipeline & Packaging**

Place after Fleet Analysis. Include:

- Pipeline orchestrator: `run_pipeline()` in `pipeline.py`. Modes:
  from-inspectors or from-snapshot. Flow: build/load snapshot → redact →
  render into temp dir → package as tarball or copy to output dir.
- `--inspect-only` mode: save snapshot and exit early.
- Tarball-first output: default produces
  `HOSTNAME-YYYYMMDD-HHMMSS.tar.gz` in CWD. Contains Containerfile,
  `config/` tree, reports, `inspection-snapshot.json`, and entitlement
  certs if present. `--output-dir` for unpacked directory output.
- Entitlement cert bundling: if RHEL host has certs in
  `/etc/pki/entitlement/`, they're copied into the tarball for use during
  `podman build` on non-RHEL build hosts.
- `--validate`: runs `podman build` against the generated Containerfile
  (relocate existing content about what it catches).
- `--push-to-github`: pushes output to a GitHub repo (relocate existing
  content).
- Wrapper scripts: `run-yoinkc.sh` (installs podman if needed, pulls image,
  runs inspection), `run-yoinkc-fleet.sh` (runs fleet aggregation in
  container).
- Image build: `Containerfile` at repo root, Fedora-based, multi-arch.
- Relocate any remaining Output Artifacts content that wasn't moved in
  task 5 (git repo layout, building on non-RHEL hosts, interactive
  refinement description).

- [ ] **Step 3: Verify section 9 — Secret Handling**

Read the existing Secret Handling section. Compare against current
`redact.py` (redaction patterns and layers) and `git_github.py` (push
guardrails). Update any descriptions that no longer match. If accurate,
leave unchanged.

- [ ] **Step 4: Commit**

```
git add design.md
git commit -m "docs(design): add Pipeline & Packaging, verify Secret Handling

Document the pipeline orchestrator, tarball-first output, entitlement
bundling, validation, and wrapper scripts. Verify Secret Handling section
against current redact.py and git_github.py.

Assisted-by: Claude"
```

---

### Task 8: Future Work & Backlog

**Context:** The current Future Work section has stale items. This task
replaces it with the prioritized backlog and tech debt from the spec.

**Files:**
- Modify: `design.md`
- Read for reference:
  - `docs/specs/proposed/2026-03-15-design-doc-refresh-design.md`
    (backlog and tech debt sections — this is the source material)

- [ ] **Step 1: Read current Future Work section**

Identify which items are done, which are still relevant, and which have been
superseded.

- [ ] **Step 2: Write section 10 — Future Work & Backlog**

Replace the existing Future Work section with content from the spec's
Prioritized Backlog and Tech Debt sections. Structure as:

```markdown
## Future Work

### High Priority

**Cross-stream targeting.** [from spec item 1]

**CI integration.** [from spec item 2]

**Containerless re-rendering.** [from spec item 3 — note yoinkc-render
as the proposed entry point name]

### Medium Priority

**Fleet drift variations.** [from spec item 4]

**Enhanced cron-to-timer conversion.** [from spec item 5]

**`/var` size estimation.** [from spec item 6]

### Lower Priority

**In-place migration.** [from spec item 7]

**Snapshot diffing.** [from spec item 8]

**Cross-family migration.** [from spec item 9]

### Tech Debt

**containerfile.py size.** [from tech debt item 1]

**report.html.j2 size.** [from tech debt item 2]

**Large test files.** [from tech debt item 3]

[Items 4-6 from tech debt "Monitor" section can be mentioned briefly
or omitted — they're not actionable yet.]
```

- [ ] **Step 3: Remove `/var` handling section if redundant**

Check if the existing "The `/var` Handling" section's content is covered by
the Storage Inspector description. If so, remove it to avoid duplication.
If it contains unique information, leave it in the Inspectors section or
relocate to Pipeline & Packaging.

- [ ] **Step 4: Final structure review**

Read through the entire restructured `design.md` from top to bottom. Check:
- Section numbering/ordering matches the spec (1-10)
- No orphaned sections from the old structure remain
- No duplicate content across sections
- Document flows logically from high-level to detailed

- [ ] **Step 5: Commit**

```
git add design.md
git commit -m "docs(design): refresh Future Work with prioritized backlog

Replace stale future work with prioritized backlog (cross-stream targeting,
CI, containerless rendering) and tech debt assessment. Remove redundant
sections. Final structure review.

Assisted-by: Claude"
```

---

## Chunk 2: driftify design.md

### Task 9: Driftify design.md updates

**Context:** Targeted updates to an existing doc that's mostly accurate.
Six specific changes identified in the spec.

**Files:**
- Modify: `design.md` at the **driftify** repo root
  (`/Users/mrussell/Work/bootc-migration/driftify/design.md`)
- Read for reference:
  - `driftify.py` (main script — check coverage map tables against
    actual code constants and drift functions)
  - `run-fleet-test.sh` (fleet testing script)

All paths in this task are relative to the driftify repo root
(`/Users/mrussell/Work/bootc-migration/driftify/`).

- [ ] **Step 1: Read driftify.py for recent additions**

Check the following against the design doc:
- `drift_services()`: does it create drop-in overrides for httpd and nginx?
  At which profile levels?
- `drift_users()`: does it create a shared `developers` group with
  supplementary membership?
- `RPMFUSION_PACKAGES` dict: do the package assignments match the coverage
  map table?
- CLI argument parser: do the flags match the CLI Interface section?

- [ ] **Step 2: Verify/update Coverage Map — Service Inspector**

Check if the Service Inspector table already has rows for drop-in overrides.
If present, verify they're accurate. If missing, add rows for:
- httpd drop-in override (`/etc/systemd/system/httpd.service.d/override.conf`
  with `TimeoutStartSec=600`, `LimitNOFILE=65535`) at standard profile
- nginx drop-in override (`/etc/systemd/system/nginx.service.d/override.conf`
  with `LimitNOFILE=131072`, `ExecStartPost` hook) at kitchen-sink profile

Verify exact profile levels and override contents against `drift_services()`
in `driftify.py`.

- [ ] **Step 3: Update Coverage Map — User/Group Inspector**

Check if the User/Group Inspector table already has a row for the shared
`developers` group. If missing, add it with supplementary membership detail.
Verify profile level and implementation against `drift_users()`. Also check
the "System state changes" subsection of File Inventory — if `developers`
group is not listed there, add it.

- [ ] **Step 4: Update Coverage Map — RPM Inspector**

Verify `RPMFUSION_PACKAGES` dict matches the coverage map table. The packages
were reshuffled across tiers recently. Update any mismatches.

- [ ] **Step 5: Update Non-Goals section**

Change the "driftify does not handle multi-host scenarios" statement. New
text should say something like: driftify itself is a single-host tool, but
`run-fleet-test.sh` orchestrates multi-host fleet testing by running driftify
with all three profiles on separate hostnames and aggregating the results.

- [ ] **Step 6: Add Fleet Testing section**

Add a new section (after CLI Interface or as subsection of Coverage
Verification) documenting `run-fleet-test.sh`:
- What it does: curls `driftify.py` and `run-yoinkc.sh`, runs all 3 profiles
  with unique `YOINKC_HOSTNAME` values, aggregates results with
  `yoinkc-fleet`
- How to use it: `curl ... | sudo sh` or download and run
- What it produces: 3 host tarballs + 1 fleet tarball
- Read `run-fleet-test.sh` for exact details

- [ ] **Step 7: Verify/update CLI Interface**

Compare the CLI Interface section against the current `argparse` block in
`driftify.py`. Check that all flags are documented and defaults are correct.
Verify `--run-yoinkc` behavior description matches implementation. Update
any discrepancies.

- [ ] **Step 8: Update File Inventory**

Check the "Files created" list against actual file creation in `driftify.py`.
Drop-in override files and the `developers` group may already be listed —
verify rather than blindly adding. Search for all `Path(...).write_text()`,
`Path(...).mkdir()`, and file creation calls to ensure completeness. Also
check the "System state changes" subsection.

- [ ] **Step 9: Update Future Work**

The existing Future Work section has three items (CI integration,
parameterized scenarios, drift variations). Update the "Drift variations"
item to note that `run-fleet-test.sh` partially addresses this (3 profiles =
3 different host configurations), but true within-profile variation (slightly
different configs per host within the same profile) is still future work.
Keep CI integration and parameterized scenarios as-is.

- [ ] **Step 10: Verify and commit**

Read through the updated sections. Verify each change against `driftify.py`
source code.

```
cd /Users/mrussell/Work/bootc-migration/driftify
git add design.md
git commit -m "docs(design): update for drop-ins, fleet testing, user groups

Add drop-in override coverage, shared developers group, fleet testing
section. Update Non-Goals for run-fleet-test.sh multi-host orchestration.
Refresh RPM Fusion package distribution and file inventory.

Assisted-by: Claude"
```
