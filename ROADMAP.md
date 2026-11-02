# inspectah Roadmap

**Current version:** v0.9.0-beta.3 (pure Rust, snapshot schema 25)

## How to read this file

This roadmap is a **view over the project backlog**, not a second list.
It carries one entry per backlog item marked roadmap-worthy, with its
title and a line of summary.

**The backlog is authoritative.** Where this file and the backlog
disagree, the backlog is right. State, priority, evidence, and the
reasoning behind each item live in the item, which is where they stay
current. This file is written by hand for now, because the backlog and
this repository are not yet the same repository, so the two can still
disagree between refreshes. That is understood and accepted: a
disagreement here is findable, which is more than the arrangement this
replaced could say.

Completed release inventories live in `CHANGELOG.md`. Recent shipped
context is in `process-docs/release-notes-0.9.0-beta.1.md` and
`process-docs/release-notes-0.9.0-beta.2.md`.

Grouping below reflects the backlog's own split between live work and
work that has been deliberately parked. It is not a priority ranking.

## In the program

### NIC Naming Risk Detection

Detect kernel-assigned `eth*` NIC names on multi-NIC systems, where
predictable naming after `bootc switch` can silently change interface
assignment and break networking.

### PAM Module Parsing

Parse `pam.d` module load lists, diff against the base image's module
set, and flag a missing non-base module as high severity. The
difference between "your PAM config changed" and "your authentication
will break".

### /usr Walk Presentation

`usr_entries` are collected and read by nothing, so unmanaged `/usr`
content has no user-facing surface at all. The design note is settled
and landed; the implementation plan is parked pending reanalysis
against a tree that has moved past it.

### Docs Overhaul

User-facing documentation refresh ahead of 1.0. Scope is in
`process-docs/backlog/documentation-backlog.md`.

### Sysctl Source File Preservation

Rendering collapses every sysctl override into one generated file
instead of mirroring the admin's source layout, which can move a key's
position in `sysctl.d` lexical ordering. The ordering consequence is
reasoned, not yet measured, and measuring it is the first step.

### Secrets Safety Net

Guardrails against accidental credential exposure in inspectah output.
Gates the full-capture work. Needs its own spec.

### Release Binary Size Optimization

Add `[profile.release]` settings (`lto = "thin"`, `strip = true`,
`codegen-units = 1`). Expected 30 to 50 percent size reduction.

### Aggregate Contract Convergence

Make aggregate speak `FindingKind` natively, retire the boolean include
field and the untyped per-item metadata, and publish the result as a
versioned refined-aggregate export contract in core. Inside 1.0's
stability promise, and the prerequisite for factor.

### Factor Propose-Only Precursor

CLI-only first increment of factor for late 0.9.x: read a directory of
refined aggregates, compute the base as a true intersection, emit one
role per input plus a falsifiable proposal report and a re-runnable
adjustments artifact. No canvas, no TUI.

### Factor Build

The 1.x flagship. A `crates/factor/` depending on core only, consuming
the published refined-aggregate contract, with the canvas UI as its own
bundle. Role archives and the editor UI live here. Factor output
formats sit outside the 1.0 stability promise.

## Parked

Each of these has a backlog item recording what it waits on. Parked
means deliberately not scheduled, not forgotten.

### Secrets v2

The structural successor to today's pattern-based redaction. Split from
the 1.0 program deliberately: the safety net above is the minimum that
gates full capture, and v2 is the larger architecture rework.

### Internals Documentation and Process Debt

Plain-English documentation of inspectah's internal decision logic
across five subsystems, plus internationalization and the `.git-backup/`
history scrub. Post-1.0, and the documentation half may fold into the
Docs Overhaul.

### Post-1.0 Detection Coverage

A coverage-themed release bundling eleven small detectors (cloud
provider, multiarch, `limits.conf`, chrony, duplicate packages, tainted
kernel modules, SSSD and Kerberos, `firewalld.conf`, crypto policy,
`nsswitch.conf`, `kdump.conf`) with the demand-driven ecosystem work:
additional language package managers, and locale and timezone
rendering.

### Driftify Kitchen-Sink End-to-End Run

Re-run the real-host end-to-end test under driftify's kitchen-sink
profile, which reaches the inspectors the standard profile does not.
Widens the ground-truth comparison rather than repeating it.

### Playwright CI, Visual Regression, Multi-Browser

The existing Playwright suite needs a shared refine-server startup path
for local and CI runs, screenshot assertions on key views, and a
Firefox project. Best scheduled right after the aggregate UI
convergence lands, before 1.0 freezes the presentation.
