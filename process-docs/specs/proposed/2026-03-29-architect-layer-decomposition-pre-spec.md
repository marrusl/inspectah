# inspectah architect: Enterprise Image Layer Decomposition

**Date:** 2026-03-29
**Status:** Pre-spec (early thinking, not ready for formal brainstorm)

## Summary

A new tool (working names: `inspectah architect`, `inspectah factor`) that takes
refined fleet outputs and helps enterprise architects decompose them into a
layered bootc image hierarchy: a base image plus derived role/hardware-specific
images.

## Motivation

The current inspectah flow is bottom-up aggregation:

    hosts -> inspect -> fleet -> refine -> Containerfile

This produces one refined Containerfile per fleet. But enterprises running
multiple fleets need to go further: factor the commonality across fleets into
a shared base image, then express each fleet's unique packages and config as
a derived `FROM base` layer.

This is how internal "host as a service" teams already operate with golden
image pipelines. The tool helps them recreate (and potentially improve) their
existing layer strategy using data from what's actually running, rather than
what someone documented three years ago.

## Key Insight: Decomposition, Not Aggregation

Refine aggregates upward: "what do these hosts have in common?" The question
is "include or exclude?" and prevalence data answers it.

Architect decomposes downward: "where does this belong?" The question is
"base layer or derived layer?" and that's an architectural judgment, not a
statistical one. A package at 100% prevalence might still belong in a derived
layer if it's role-specific. A package at 40% might belong in base because
it's a security requirement the other 60% should have had.

The decisions are architectural, not statistical. This demands a different UI.

## Output

A **Containerfile tree**, not a single file:

- Base image: `FROM <upstream-stream-image>`
- Derived images: `FROM <base>` with role/hardware-specific deltas

The depth is arbitrary in principle. In practice, expect 2-4 levels:
base -> middleware/hardware tier -> application role.

## UI Direction

Standalone web application (not a Podman Desktop extension). Rationale:

- **Screen freedom**: the decomposition workbench needs drag-and-drop layer
  assignment, impact visualization, what-if exploration. Podman Desktop's
  extension framework may constrain this.
- **Iteration speed**: the interaction model needs to be discovered through
  use. Fighting an extension API's layout assumptions slows that down.
- **Podman Desktop as future wrapper**: once the UI stabilizes, a thin
  extension shell hosting the app in a webview remains an option for
  catalog distribution.

Not containerized. Runs from an admin's connected workstation (needs internet
for advisory feeds). This is a planning tool, not an execution tool.

## Blast Radius as a Core Metric

In bootc, the cost of a layer change isn't rebuild time -- it's **reboot
surface**: how many hosts across how many fleets must stage an update and
cycle when a layer's contents get patched.

### What to measure

- **Fan-out**: how many derived images rebuild when this layer changes
- **Reboot surface**: how many hosts are running images derived from this
  layer (requires fleet collection data for host counts)
- **Change frequency**: how often the packages in this layer receive upstream
  advisories (errata, CVEs). A high-fan-out layer with stable packages is
  less painful than a low-fan-out layer with weekly patches.
- **Expected churn**: fan-out x change frequency = organizational pain per
  unit time

### Where change frequency data comes from

Upstream advisory feeds (Red Hat OVAL, CentOS Stream advisories, EPEL).
Fetched live at planning time since the tool runs on a connected workstation.
Cached with staleness indicator ("advisory data as of ...").

This is OS/ecosystem update frequency, not the customer's own patching
cadence. The tool shows how often upstream *offers* updates; how often the
org *applies* them is their own policy.

### How to convey it

- **Tree visualization with color/weight**: hot paths (high fan-out,
  frequent changes) are visually prominent. Stable foundations are cool.
- **What-if on move**: dragging a package between layers shows impact
  tooltip: "Moving openssl-devel to base exposes 14 downstream images.
  Patched 6 times in last 12 months."
- **Turbulence score per layer**: dependents x change frequency. Higher =
  more reboot coordination pain.
- **Diff from current state**: "Your current pipeline rebuilds 40 images
  when base changes. This layout would rebuild 28."

## Input Format

### Minimum viable

The clean render from refine: winning variants and selected items per fleet.
This is sufficient to drive the decomposition UI.

### Preserved context

Full fleet/refine data (variant history, prevalence scores, per-host
breakdown) should be available but not required. The architect may recognize
a problem in the decomposition and want to revisit a variant choice without
re-running fleet collection against live hosts.

## Hardware as a Decomposition Axis

Hardware identity (GPU, NIC firmware, specific bare-metal drivers) is
orthogonal to application role. A "bare metal with Mellanox" tier might sit
between base and application role. The tree may branch by hardware at one
level and by role at the next.

## Single vs. Multiple Base Images

Working assumption: single base image. The enterprise SOE pattern almost
always converges on one base (core packages, security tooling, observability,
auth config are universal enough).

Multiple bases remain architecturally possible but we don't design for it
until a real use case surfaces. The tool might *reveal* that two bases with
less forced commonality produce a cleaner tree -- that's a finding, not a
starting assumption.

## Data Freshness

Two freshness concerns:

1. **Fleet data**: how recently were hosts inspected? Displayed as collection
   timestamp. Stale fleet data means stale host counts for reboot surface.
2. **Advisory frequency**: how current is the upstream errata data? Displayed
   with "last refreshed" indicator and manual refresh. A package that was
   quiet for two years may suddenly become hot (new CVE surface).

## Relationship to Existing Tools

| Tool    | Direction  | Question answered              |
|---------|------------|--------------------------------|
| inspect | Collect    | What's on this host?           |
| fleet   | Aggregate  | What do these hosts share?     |
| refine  | Curate     | Which variants win?            |
| architect | Decompose | How should the layers split? |

Architect consumes refine outputs. It does not replace or subsume refine.

## Open Questions

- **Naming**: `architect` vs `factor` vs something else. "Architect" implies
  the human role; "factor" implies the mathematical operation.
- **Collaboration**: is this single-seat or does it need sharing/review
  workflows? Enterprise layer decisions involve multiple stakeholders.
- **Subcommand or standalone?** It may not belong in the inspectah CLI at all
  given that it doesn't need containerization and has different runtime
  requirements (connected workstation, web UI).
- **What makes a "good" decomposition?** Architects will likely want to match
  their existing golden image structure. The tool should accommodate that
  while providing impact data that might challenge their assumptions.

## Upstream Testing: Driftify

Testing architect requires multi-fleet topology fixtures that don't exist
yet. Driftify currently generates drift for hosts within a fleet; it needs
to generate entire fleet topologies with controlled inter-fleet variance.

Candidate scenarios:
- Three fleets with 90% package overlap (clean base extraction)
- Two fleets that look different but could share a single image
- Hardware axis splits (GPU nodes mixed into an otherwise homogeneous fleet)
- Packages with high advisory frequency landing in different proposed layers

**TDD consideration**: building the driftify fixtures first would define the
input contract and force concrete decisions about data structures before
the architect tool is designed. This is TDD in spirit (input-first) even if
not in letter (no assertion cycle yet).
