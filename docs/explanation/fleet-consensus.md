---
title: Fleet Consensus
parent: Explanation
nav_order: 4
---

# Fleet Consensus

Inspecting a single host tells you what that host looks like. Inspecting a
fleet of hosts tells you something more valuable: what your infrastructure
actually *is*. Fleet consensus is the mechanism inspectah uses to transform
individual host data into a unified picture of your environment -- separating
the patterns that define your fleet from the one-off differences that do not.

## Why consensus matters

Most enterprise Linux environments are not snowflakes. Hosts are deployed from
the same base image, configured by the same automation, and expected to run
the same workloads. When you are planning a migration to image mode, the
question is not "what does host-47 look like?" but "what do all my hosts look
like, and where do they disagree?"

Consensus analysis answers that question directly. It identifies which
artifacts are truly universal across your fleet (and therefore belong in your
target image), which appear on most hosts (and probably belong there too), and
which show up on only a handful of machines (and need individual attention).

Without consensus, fleet migration planning devolves into auditing each host
independently and manually correlating the results. With it, inspectah reduces
a fleet of fifty hosts to a single prioritized view where the important
differences float to the top.

## From hosts to fleet: the merge step

Fleet analysis begins with individual host scans. Each host produces an
inspection snapshot -- a complete inventory of packages, configuration files,
services, containers, kernel parameters, and other system artifacts.

The merge step combines these snapshots into a single fleet-aggregate snapshot.
For every item that appears across the fleet, the merge records its
**prevalence**: how many hosts have it, which hosts those are, and (for
configuration files with content variants) what the different versions look
like.

The merge is purely mechanical. It does not interpret or classify -- it counts.
A package that appears on 10 out of 10 hosts gets `count: 10, total: 10`. A
config file with two different versions across the fleet gets separate entries
for each variant, each with its own prevalence count.

## Prevalence zones

Once prevalence data exists, inspectah classifies every item into a
**prevalence zone** based on how widely it appears across the fleet. The
thresholds are simple and deliberate:

| Zone | Condition | Meaning |
|------|-----------|---------|
| **Consensus** | count = total | Item appears on every host in the fleet |
| **NearConsensus** | count >= half of total | Item appears on at least half the hosts |
| **Divergent** | count < half of total | Item appears on fewer than half the hosts |

The half-of-fleet boundary for NearConsensus is a practical threshold. An item
present on 7 of 10 hosts is probably intentional infrastructure -- it belongs
in most places and is just missing from a few. An item present on 2 of 10
hosts is more likely a role-specific addition or a leftover from manual
configuration.

Zone classification activates automatically when a fleet has two or more
hosts. The merge code does not enforce a minimum host count -- any multi-host
fleet gets prevalence zones.

## Fleet buckets: consensus meets triage

Prevalence zones feed into inspectah's triage system through **fleet buckets**,
which extend the single-host triage model (Baseline / Site / Investigate) with
fleet-aware categories:

| Fleet Bucket | Source Zone | Triage Equivalent | What it means |
|--------------|-------------|-------------------|---------------|
| **Universal** | Consensus | Baseline | Present on all hosts. Part of your fleet's shared identity. |
| **Partial** | NearConsensus | Site | Present on most hosts. Likely intentional, possibly inconsistently deployed. |
| **Divergent** | Divergent | Investigate | Present on a minority of hosts. Role-specific, accidental, or leftover. |
| **Investigate** | Divergent (edge case) | Investigate | Divergent-zoned but present on all hosts -- a data anomaly worth examining. |

The mapping from fleet buckets back to single-host triage equivalents means
the same filtering and prioritization logic works in both modes. A Divergent
item routes to the Investigate bucket, just as a suspicious single-host item
would. A Universal item routes to Baseline -- already accounted for, no action
needed.

{% raw %}
<div class="diagram-embed" style="margin: 2em 0;">
  <iframe id="diagram-fleet-topology"
          src="../diagrams/fleet-topology.html"
          title="Fleet Topology — interactive preview"
          width="100%" height="450" frameborder="0"
          loading="lazy" tabindex="0"></iframe>
  <div style="margin-top: 0.5em;">
    <button id="btn-diagram-fleet-topology"
            onclick="(function(btn){var iframe=document.getElementById('diagram-fleet-topology');if(iframe.requestFullscreen){iframe.requestFullscreen();iframe._triggerBtn=btn;document.addEventListener('fullscreenchange',function handler(){if(!document.fullscreenElement){document.removeEventListener('fullscreenchange',handler);if(iframe._triggerBtn){iframe._triggerBtn.focus();iframe._triggerBtn=null;}}});}else{window.open(iframe.src,'_blank');}})(this)"
            aria-label="Open fleet topology diagram in fullscreen">
      Open interactive diagram
    </button>
  </div>
  <p><em>The fleet topology diagram shows how individual host scans combine through aggregation into fleet-level analysis.</em></p>
</div>
{% endraw %}

## Config differences are the real signal

Package consensus is important, but in practice most hosts in a managed fleet
have similar package sets. The packages were installed from the same base image
and the same Ansible playbooks. Where fleets genuinely diverge is in their
**configuration**.

Consider a fleet of 10 database servers. They all have PostgreSQL installed
(Universal, Baseline). They all have a `postgresql.conf` -- but 7 hosts have
one version of the file and 3 hosts have a different version. That config
divergence is the real migration signal. It tells you that your fleet is not
as uniform as it appears at the package level, and you need to decide which
configuration variant goes into your target image.

Inspectah handles this through **variant tracking**. When a config file path
appears with multiple content versions across the fleet, each variant gets its
own prevalence count. The path-level zone is set to the most divergent variant
-- if any variant of `/etc/sshd/sshd_config` is Divergent, the path as a whole
is classified as Divergent. This conservative approach ensures that config
differences surface for review rather than being hidden behind an aggregate
count.

This is why inspectah prioritizes config findings in fleet mode. A fleet where
every host has the same packages but different `/etc/sysctl.conf` values is not
really converged -- and the migration plan needs to account for those
differences.

## Practical implications

### What Universal means for migration

Items classified as Universal across your fleet are strong candidates for your
target image definition. If every host has `httpd` installed and configured the
same way, that belongs in the Containerfile. Universal consensus is inspectah's
way of saying "this is part of your fleet's identity."

### What Partial means for migration

Partial items are present on most hosts but not all. This often indicates
inconsistent deployment -- something that was supposed to be everywhere but
was missed on a few machines, or a package that is being rolled out gradually.
During migration, these items warrant a decision: should they be in the base
image (and the missing hosts were wrong), or should they be role-specific
layering?

### What Divergent means for migration

Divergent items appear on only a minority of hosts. These split into two
categories in practice:

- **Role-specific additions**: a monitoring agent on the two hosts that serve
  as Prometheus endpoints, a debug package left on a staging server. These are
  legitimate and should be handled through image layering or host-specific
  configuration.

- **Drift and accidents**: a package someone installed by hand three years ago,
  a config file that was modified during an incident and never reverted. These
  are candidates for cleanup -- the migration is an opportunity to not carry
  them forward.

The Divergent classification does not judge which category an item falls into.
It surfaces the items and lets the operator decide.

### Fleet of one

When inspectah analyzes a single host, there is no fleet consensus to compute.
Every item is trivially "unanimous" and the single-host triage model
(Baseline / Site / Investigate) applies directly. Fleet consensus activates
with two or more hosts, where prevalence patterns become meaningful.

## How it connects to triage

Fleet consensus and single-host triage are complementary systems, not
alternatives. Single-host triage classifies items based on their relationship
to the target image (is this package in the base image? is this config at its
default value?). Fleet consensus classifies items based on their distribution
across hosts (does everyone have this, or just a few machines?).

In fleet mode, both signals combine. An item might be classified as Site by
single-host triage (it is not in the target image) but Universal by fleet
consensus (every host has it). That combination -- site-specific but universal
-- is a strong signal that this item belongs in the target image definition,
and the current base image selection may need adjustment.

The opposite combination is equally informative. An item classified as Baseline
by single-host triage but Divergent by fleet consensus means the base image
includes something that most of the fleet has removed or replaced. That is
worth investigating.

See [Triage Philosophy](triage-philosophy) for the full single-host
classification model that fleet consensus extends.
