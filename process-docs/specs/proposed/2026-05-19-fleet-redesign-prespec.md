# Fleet Redesign Pre-Spec

> Starting point for brainstorming the Rust fleet experience.
> Nothing from the Go implementation is sacred apart from the basic
> concept: analyze multiple hosts together to find patterns.

## Open Questions

### Flow model
- The Go flow is batch-merge-then-render. Should the Rust version be
  interactive from the start (refine-style), or is there still value
  in a batch aggregate step?
- Should fleet produce a single merged Containerfile, or per-host
  Containerfiles with a fleet-level summary showing what's common
  vs. what's host-specific?

### Primary axis
- Fleet prevalence is the Go core concept: "N of M hosts have this
  package." Is that still the right primary axis?
- Should it be organized differently — by drift from baseline, by
  repo, by host cluster/role?
- How does baseline subtraction interact with fleet? One shared
  baseline, or per-host baselines?

### Naming
- `fleet` implies a managed fleet. Is that the right framing for the
  target users, or is this more like "multi-host analysis" /
  "environment scan" / "cohort" / something else?
- The subcommand name (`inspectah fleet`) — keep or rename?

### Refine integration
- Can the same `RefineSession` trait serve fleet, or does fleet need
  its own session type?
- What operations make sense at fleet scope? Exclude-package across
  all hosts? Per-host overrides?
- How does the refine UI change for fleet — host selector? Prevalence
  columns? Diff view between hosts?

### Input model
- Go takes a directory of tarballs. Is that still the right input,
  or should fleet support a manifest file, remote hosts, or a
  registry of scans?
- Should fleet support incremental updates (add a new host scan to
  an existing aggregate)?

### Output model
- What does the fleet export look like? One tarball? Per-host
  tarballs? A fleet summary report?
- How does the Containerfile render change for fleet prevalence data?

## Context

- Go fleet code: `cmd/inspectah/internal/fleet/` and
  `cmd/inspectah/internal/cli/fleet.go`
- Rust is already fleet-aware: `FleetPrevalence` type exists,
  renderers skip leaf filtering on merged snapshots, refine handles
  `pkg.fleet.is_some()`
- The `inspectah-refine` session trait was designed to support both
  single-host and fleet sessions (Phase 3 design decision)

## Brainstorm Team

- Ember (product strategy — who is this for, what's the pitch)
- Fern (interaction design — fleet UX in refine)
- Collins (domain — how does fleet relate to image mode migration at scale)
- Tang (architecture — session trait, merge engine, data model)
