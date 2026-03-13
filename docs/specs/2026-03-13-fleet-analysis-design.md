# Fleet-Level Migration Analysis

**Date:** 2026-03-13
**Status:** Brainstorming (early — first questions only)

## Problem

Migrating hosts one-by-one with a 1:1 host:image ratio is an
anti-pattern with bootc. For a fleet of 100 similar web servers, the
user needs a single golden image, not 100 Containerfiles. yoinkc
currently produces per-host output with no way to find commonality
across hosts.

## Decomposition

This breaks into two independent sub-projects:

### Sub-project 1: Fleet Collection

Run yoinkc across N hosts and gather tarballs. Essentially an
orchestration problem — push yoinkc to hosts via SSH/Ansible, collect
results. Could be a wrapper script, an Ansible playbook, or a
dedicated `yoinkc-collect` tool that takes a list of hosts/IPs/subnet.

### Sub-project 2: Fleet Aggregation (the novel problem)

Analyze N inspection snapshots to produce a fleet-level view:
- **Intersection**: what's common across ALL hosts (the golden image
  candidate)
- **Union with prevalence**: every item across all hosts, annotated
  with how many hosts have it (e.g., "httpd: 98/100 hosts")
- **Outlier detection**: items present on only 1-2 hosts (likely
  snowflake drift, not fleet intent)

The output could be a merged super-snapshot, a fleet report, or an
interactive refinement UI similar to yoinkc-refine.

## Decisions So Far

- **Persona**: single admin managing a fleet (not a team workflow)
- **Collection should be automated** — eventually push-button across a
  subnet or host list
- **Collection and aggregation are separate tools/specs**

## Open Questions (for next session)

1. What is the primary output of aggregation?
   a) A merged snapshot that feeds into the existing yoinkc pipeline
      (renderers produce a single Containerfile from the merged data)
   b) A fleet-level report that shows commonality and differences
      (informational, user decides what to include)
   c) Both — merged snapshot + fleet diff report

2. How does the user indicate which hosts are "the same role"? Do they
   pre-group (all web servers in one directory), or does the tool
   cluster automatically based on package similarity?

3. Config file handling — if 98/100 hosts have the same httpd.conf but
   2 have different values, how is that surfaced? Majority wins with
   outlier warnings?

4. Where does the interactive refinement happen — in the existing
   yoinkc-refine (extended for fleet view), or a new fleet-specific
   UI?

5. Should the tool be part of yoinkc (a new CLI mode like
   `yoinkc fleet`), or a separate companion tool?

6. Scale considerations — does this need to work with 10 hosts? 100?
   1000? (affects data structures and UI approach)
