---
title: Architecture
parent: Explanation
nav_order: 1
---

# Architecture

Inspectah is structured as a Rust workspace of seven crates. This separation is
deliberate: each crate owns one concern, and the boundaries between them
reflect real architectural decisions about what should depend on what. This
document explains both the structure and the reasoning behind it.

{% raw %}
<div class="diagram-embed" style="margin: 2em 0;">
  <iframe id="diagram-software-architecture"
          src="../diagrams/software-architecture.html"
          title="Software Architecture — interactive preview"
          width="100%" height="450" frameborder="0"
          loading="lazy" tabindex="0"></iframe>
  <div style="margin-top: 0.5em;">
    <button id="btn-diagram-software-architecture"
            onclick="(function(btn){var iframe=document.getElementById('diagram-software-architecture');if(iframe.requestFullscreen){iframe.requestFullscreen();iframe._triggerBtn=btn;document.addEventListener('fullscreenchange',function handler(){if(!document.fullscreenElement){document.removeEventListener('fullscreenchange',handler);if(iframe._triggerBtn){iframe._triggerBtn.focus();iframe._triggerBtn=null;}}});}else{window.open(iframe.src,'_blank');}})(this)"
            aria-label="Open software architecture diagram in fullscreen">
      Open interactive diagram
    </button>
  </div>
  <p><em>The software architecture diagram shows how inspectah's seven crates relate to each other and where the dependency boundaries fall.</em></p>
</div>
{% endraw %}

## The workspace at a glance

The seven crates live under `crates/` with the `inspectah-` prefix dropped from
directory names. Cargo package names remain unchanged. They form a layered
dependency graph:

| Crate | Directory | Purpose | Depends on |
|-------|-----------|---------|------------|
| **inspectah-core** | `crates/core/` | Schema types, traits, snapshot model | Nothing (leaf crate) |
| **inspectah-collect** | `crates/collect/` | Host inspection, data gathering | core |
| **inspectah-pipeline** | `crates/pipeline/` | Orchestration, rendering, output generation | core, collect |
| **inspectah-refine** | `crates/refine/` | Interactive editing and re-rendering engine | core, pipeline |
| **inspectah-web** | `crates/web/` | HTTP server, HTML reports, interactive UIs | core, pipeline, refine |
| **inspectah-tui** | `crates/tui/` | Terminal UI for interactive refinement | core, refine |
| **inspectah-cli** | `crates/cli/` | Binary entry point, argument parsing, subcommands | all of the above |

Dependencies flow in one direction: from the CLI inward toward core. No crate
depends on a crate above it in this table. This layering is what makes the
system testable -- you can unit-test core types without a host, test collectors
with a mock executor, and test renderers against fixture snapshots without
running a real scan.

## inspectah-core: the shared language

Core is the leaf crate. It defines the data types that every other crate
speaks: the snapshot schema, fleet models, baseline definitions, and the trait
interfaces that collectors and renderers implement.

**Why it exists separately.** If type definitions lived alongside the code that
uses them, you would get circular dependencies the moment two crates needed to
share a type. Core breaks that cycle by owning the vocabulary. A collector
produces `types::RpmPackage` values. A renderer consumes them. Neither needs
to know about the other -- they both speak core.

Core contains several important modules:

- **types/** -- One module per domain: `rpm`, `config`, `services`, `network`,
  `containers`, `kernelboot`, `selinux`, `users`, `storage`, `scheduled`,
  `nonrpm`, `fleet`, `subscription`, `repo`, `os`, `system`, and supporting
  types like `warnings`, `redaction`, `completeness`, `preflight`, and
  `progress`. Each defines the serializable structs that flow through the
  entire pipeline. All 25 toggleable item types use a unified include-default
  model: `include` fields default to `true` via a `default_true` serde
  helper, and a `locked` boolean field prevents toggling for non-negotiable
  decisions (e.g., baseline-subtracted items).
- **traits/** -- The `Inspector`, `Renderer`, `Detector`, `Executor`, and
  `Progress` traits that define the contracts between crates. Collectors
  implement `Inspector`. Output generators implement `Renderer`. The executor
  abstraction enables testing without a real host.
- **snapshot.rs** -- The top-level `Snapshot` struct that aggregates all
  inspection results into a single serializable document.
- **baseline.rs** -- Baseline resolution logic: given a target image, determine
  which packages, services, and configs are defaults versus operator-added.
- **fleet/** -- Fleet merge, manifest, and validation logic for combining
  multiple host snapshots into aggregate fleet data.
- **pipeline.rs** -- Pipeline configuration types shared between the
  orchestrator and its consumers.

## inspectah-collect: talking to the host

Collect is the crate that actually touches the system. It runs commands,
reads files, queries package databases, and produces structured data. Every
piece of host interaction lives here and nowhere else.

**Why collection is isolated.** Inspecting a host involves calling `rpm -qa`,
reading `/proc/sys`, parsing firewall rules, walking `/etc` -- operations that
are messy, platform-specific, and sometimes require elevated privileges. By
quarantining all of this in one crate, the rest of the system stays pure: it
operates on well-typed Rust structs rather than raw command output and file
contents.

The key abstraction is the **Executor** trait. Collect defines two
implementations: `RealExecutor`, which runs commands on an actual host (or a
chroot), and `MockExecutor`, which returns canned output for testing. Every
inspector accepts an executor, which means every inspector is testable without
root access or a real system.

Collect organizes inspectors under `inspectors/`, with subdirectories for
complex domains and single files for simpler ones. An `executor/` module
provides the host abstraction.

- **inspectors/rpm/** -- Package inventory, leaf/auto classification, version
  drift detection, repo tracking, GPG key resolution, modified config
  detection via `rpm -Va`, and unowned file scanning.
- **inspectors/config/** -- Configuration file discovery via filesystem walk
  (`walk.rs`), RPM ownership classification (`classify.rs`), and modified
  config detection via `rpm -Va` (`rpmva.rs`).
- **inspectors/services.rs** -- Systemd unit enumeration, preset comparison,
  and enablement state detection.
- **inspectors/network.rs** -- Firewall rules, hostname, DNS configuration,
  network connection profiles, and proxy settings.
- **inspectors/kernelboot.rs** -- Kernel parameters, loaded modules, sysctl
  values with source attribution, locale, and timezone detection.
- **inspectors/containers.rs**, **storage.rs**, **scheduled.rs**,
  **selinux.rs**, **nonrpm.rs**, **users.rs** -- Domain-specific inspectors
  for containers, storage mounts, cron/timers, SELinux policy, non-RPM
  software, and user accounts.
- **inspectors/subscription.rs** -- RHEL subscription material collection:
  entitlement cert/key pairs from `/etc/pki/entitlement`, CA certs from
  `/etc/rhsm/ca`, `rhsm.conf`, and `redhat.repo`. Parses X.509 expiry
  dates, validates bundle completeness, and extracts org metadata from
  consumer certs. Activated by `--preserve subscription`.
- **executor/** -- The `RealExecutor` and `MockExecutor` implementations live
  here, along with the executor selection logic.
- **baseline.rs** -- Baseline image querying: runs `podman run` against the
  target image to extract its package list, service presets, and config
  defaults for subtraction.

## inspectah-pipeline: from data to artifacts

Pipeline is the orchestration layer. It takes the raw data from collect,
applies baseline subtraction, runs redaction, validates the results, and
produces output artifacts. It is the crate that answers "given an inspection,
what should the output look like?"

**Why pipeline sits between collect and the output.** Collection gathers
everything. But not everything belongs in the output -- base image defaults
should be subtracted, secrets should be redacted, and the remaining data needs
to be formatted into specific artifact types. Pipeline owns all of that
transformation logic.

Pipeline's modules:

- **build/** -- Build planning and execution. Given an inspectah tarball,
  extracts it safely, detects RHEL ambient subscription status, plans
  subscription certificate mounts, checks cert expiry, and constructs the
  `podman build` command. Sub-modules: `extract.rs` (archive safety and
  tarball extraction), `rhel.rs` (ambient subscription detection and
  validation).
- **orchestrate.rs** -- The top-level scan orchestrator. Runs inspectors in
  sequence, feeds results through baseline subtraction, and dispatches to
  renderers. This is where the scan workflow is defined.
- **collect.rs** -- Adapter between the orchestrator and the collect crate's
  inspectors.
- **validate.rs** -- Post-collection validation: preflight checks, consistency
  assertions, and completeness verification.
- **redaction/** -- Secret detection and redaction engine. Pattern-based
  scanning with configurable rules ensures credentials, API keys, and other
  sensitive values never appear in output artifacts. The engine matches
  password hashes, PEM certificate blocks, database connection strings
  (including MongoDB URL structure preservation), and NSS/PAM tokens.
  Comment-line filtering and false-positive filtering reduce noise.
  Sub-modules: `engine.rs` (scan orchestration), `patterns.rs` (pattern
  definitions and matching).
- **render/** -- Output renderers, each implementing the `Renderer` trait:
  - `containerfile.rs` -- Generates a Containerfile with correctly ordered
    `RUN`, `COPY`, and `RUN dnf install` directives.
  - `report.rs` -- HTML audit report with PatternFly UI and full section parity.
  - `report_data.rs` -- Report data preparation and serialization for the
    HTML report.
  - `audit.rs` -- Machine-readable audit log of all inspection findings.
  - `kickstart.rs` -- Kickstart file generation for hosts that use that
    provisioning model.
  - `tarball.rs` -- Packages all output artifacts into a downloadable archive.
  - `users.rs` -- User and group materialization with strategy-aware rendering.
  - `secrets.rs` -- Secrets scan summary renderer.
  - `safety.rs` -- Safety net warnings for items that need manual review.
  - Supporting modules: `baseline_fmt.rs`, `configtree.rs`,
    `service_intent.rs`, `readme.rs`.

## inspectah-refine: interactive editing

Refine manages the interactive session state. When a user runs
`inspectah refine`, the tool serves a web UI where they can toggle items in
and out, change user migration strategies, and re-render the Containerfile
live. Refine is the engine behind that interactivity.

**Why refine is a separate crate from pipeline.** Pipeline renders once: data
in, artifacts out. Refine manages a stateful session: the user changes a
toggle, the snapshot is mutated, and the pipeline re-renders with the new
state. This session management, change tracking, and normalization logic does
not belong in the render path -- it sits on top of it.

Refine's modules:

- **session.rs** -- Manages the mutable snapshot state, tracks user edits,
  and coordinates re-rendering through pipeline.
- **classify.rs** -- Triage classification: determines whether each item is
  Baseline (from the base image), Site (operator-added), or Investigate
  (needs attention).
- **normalize.rs** -- Snapshot normalization: ensures consistent ordering and
  deduplication before rendering.
- **autosave.rs** -- Periodic state persistence so edits survive browser
  refreshes.
- **baseline_summary.rs** -- Generates human-readable summaries of what
  baseline subtraction removed.
- **repo_index.rs** -- Repository indexing for the unified repo management
  view.
- **tarball.rs** -- Packages the current refine session state for export.
- **types.rs** -- Refine-specific request/response types for the web API.
- **projection/** -- Snapshot projection for refine: applies user decisions
  (`decisions.rs`) to the snapshot, computes reference data
  (`reference.rs`), and produces the projected types (`types.rs`) consumed
  by renderers and the export path.
- **fleet/** -- Fleet-specific refine logic: triage classification across
  hosts (`classify.rs`), variant diffing (`diff.rs`), and variant operations
  (`variant_ops.rs`) for the fleet refine UI.

## inspectah-web: serving the interface

Web is the HTTP layer. It embeds the HTML/CSS/JavaScript assets, defines the
API routes, and serves the interactive UIs for refine and fleet workflows.

**Why web is separate from refine.** Refine is the engine -- it manages state
and orchestrates re-rendering. Web is the transport -- it maps HTTP requests
to refine operations and serves the resulting HTML. This separation means
the refine engine can be driven by other frontends -- and it is: the TUI
crate provides a terminal-based alternative (see below).

Web's modules:

- **handlers.rs** -- Route handlers for the single-host refine UI: serving
  reports, processing toggle changes, and triggering re-renders.
- **fleet_handlers.rs** -- Route handlers for fleet-specific operations:
  fleet report serving and fleet refine interactions.
- **adapter.rs** -- Bridges refine session state to web response types.
- **web_types.rs** -- Web-specific request and response types.
- **assets.rs** -- Embedded static assets (HTML templates, CSS, JavaScript)
  compiled into the binary at build time.
- **error.rs** -- HTTP error types and response formatting.
- **lib.rs** -- Server construction and route registration using Axum.

## inspectah-tui: the terminal interface

TUI is an alternative frontend for the refine workflow. It provides the same
interactive refinement experience as the web UI -- section navigation, item
toggling, search, export -- but rendered entirely in the terminal using
[ratatui](https://ratatui.rs/). Invoked via `inspectah refine --tui`.

**Why TUI is a separate crate from web.** Web serves an HTTP-based UI with
HTML/JS assets. TUI renders directly to the terminal using crossterm. They
share no transport code, but both drive the same refine engine underneath.
Keeping them separate means each frontend depends only on what it needs: TUI
depends on refine and core, not on axum or web assets.

TUI's modules:

- **app.rs** -- Application state, main loop, and event dispatch.
- **screen/** -- Screen-level layouts: `single_host.rs` for the primary refine
  view.
- **widget/** -- Reusable UI components: `section_nav.rs` (section sidebar),
  `triage_list.rs` (item list with toggle controls), `detail_view.rs` (item
  detail pane), `search.rs` (incremental search), `containerfile.rs`
  (live Containerfile preview), `status_bar.rs`, `info_bar.rs`,
  `help_screen.rs` (keybinding reference), `user_strategy.rs` (user
  provisioning strategy selector), and `command_line.rs`.
- **sections.rs** -- Maps snapshot sections to navigable TUI sections.
- **keys.rs** -- Keybinding definitions.
- **event.rs** -- Terminal event handling (keyboard, mouse, resize).
- **action.rs** -- Action types that flow from events to state mutations.
- **theme.rs** -- Color palette and styling.
- **types.rs** -- TUI-specific type definitions.

## inspectah-cli: the entry point

CLI is the top-level crate that produces the `inspectah` binary. It depends on
every other crate and wires them together. Argument parsing, subcommand
dispatch, and progress display -- the user-facing surface area lives here.

**Why CLI is a thin shell.** The binary itself should do as little as possible.
It parses arguments, selects the right workflow (scan, build, refine, fleet,
or version), and hands off to the appropriate crate. This keeps the logic testable
at the library level rather than requiring end-to-end CLI invocations to
exercise it.

CLI's modules:

- **main.rs** -- Entry point, clap argument definitions, and subcommand
  routing.
- **commands/** -- One module per subcommand: `scan.rs`, `build.rs`,
  `refine.rs`, `fleet.rs`, `version.rs`, and `pull_progress.rs` (image pull
  progress tracking for baseline resolution).
- **progress/** -- Terminal progress display with multiple backends: `pretty.rs`
  (append-only receipt with Unicode symbols), `flat.rs` (numbered sequential
  lines for CI), `receipt.rs` (shared data model), and `display.rs`
  (display trait abstractions).

## Data flow: from scan to artifact

The data flow through inspectah follows a clear pipeline pattern. Understanding
this flow is key to understanding where to make changes when contributing.

{% raw %}
<div class="diagram-embed" style="margin: 2em 0;">
  <iframe id="diagram-data-flow"
          src="../diagrams/data-flow.html"
          title="Data Flow — interactive preview"
          width="100%" height="450" frameborder="0"
          loading="lazy" tabindex="0"></iframe>
  <div style="margin-top: 0.5em;">
    <button id="btn-diagram-data-flow"
            onclick="(function(btn){var iframe=document.getElementById('diagram-data-flow');if(iframe.requestFullscreen){iframe.requestFullscreen();iframe._triggerBtn=btn;document.addEventListener('fullscreenchange',function handler(){if(!document.fullscreenElement){document.removeEventListener('fullscreenchange',handler);if(iframe._triggerBtn){iframe._triggerBtn.focus();iframe._triggerBtn=null;}}});}else{window.open(iframe.src,'_blank');}})(this)"
            aria-label="Open data flow diagram in fullscreen">
      Open interactive diagram
    </button>
  </div>
  <p><em>The data flow diagram traces a scan from host inspection through baseline subtraction, redaction, and rendering to the final output artifacts.</em></p>
</div>
{% endraw %}

### The scan path

1. **CLI parses arguments** and determines the scan configuration: target
   image, output directory, which inspectors to run, and redaction settings.
2. **Pipeline orchestrates collection.** The orchestrator in pipeline calls
   each inspector in collect, passing the configured executor and host root
   path. Inspectors run independently and produce typed results.
3. **Baseline resolution.** If a target image is specified, pipeline runs
   baseline collection against the image (via `podman run`) to get the
   image's package list, service presets, and config defaults.
4. **Baseline subtraction.** The orchestrator subtracts baseline data from
   the host inspection results. Packages present in both host and image are
   removed. Services matching image presets are removed. Config files
   identical to image defaults are removed.
5. **Redaction.** The redaction engine scans all remaining data for secrets
   and sensitive values, replacing them with redaction markers.
6. **Snapshot assembly.** The remaining data is assembled into a `Snapshot`
   -- the canonical intermediate representation that all downstream consumers
   operate on.
7. **Rendering.** Pipeline dispatches the snapshot to each configured
   renderer: Containerfile, HTML report, audit log, kickstart, and tarball.
   Each renderer produces its output independently.
8. **Output.** Artifacts are written to the output directory.

### The refine path

After a scan, `inspectah refine` serves the HTML report through the web
server. The user interacts with the report -- toggling packages, changing
strategies, and excluding config files. Each change flows through the refine
session, which mutates the snapshot and triggers a re-render through
pipeline. The updated HTML replaces the page.

### The fleet path

Fleet analysis aggregates multiple host snapshots. The merge logic in core
combines them into a fleet-aggregate snapshot with prevalence counts. Fleet
classification in refine assigns each item to a prevalence zone (Consensus,
NearConsensus, Divergent). The fleet-specific renderers and handlers in
pipeline and web produce the fleet report and fleet refine UI.

## Design principles

Several principles guided the architecture:

**Baseline subtraction over exhaustive listing.** Rather than dumping
everything on a host, inspectah subtracts what already exists in the target
image. The output shows only what the operator needs to act on. This is the
single most important design decision in the tool.

**Inspectors are independent.** Each inspector runs against the host root
and produces its own typed output without depending on other inspectors'
results. This means inspectors can be added, removed, or modified without
cascading changes.

**The snapshot is the contract.** The `Snapshot` struct in core is the
interface between collection and rendering. Anything upstream of the
snapshot is about gathering data. Anything downstream is about presenting
it. This boundary makes it possible to test renderers with fixture
snapshots and collectors with mock executors.

**Stateless rendering, stateful refine.** Renderers are pure functions:
snapshot in, artifact out. The refine engine adds statefulness on top,
managing user edits as a layer over the snapshot. This keeps the rendering
path simple and predictable.

**Secrets never reach output.** The redaction engine runs before rendering,
ensuring that no renderer ever sees sensitive values. This is a defense-in-depth
measure -- individual renderers do not need to worry about secret handling.
