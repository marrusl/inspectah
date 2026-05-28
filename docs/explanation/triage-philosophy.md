---
title: Triage Philosophy
parent: Explanation
nav_order: 3
---

# Triage Philosophy

A typical RHEL host has hundreds of installed packages, thousands of
configuration files, dozens of running services, kernel parameters, container
workloads, and more. Migrating that host to image mode means deciding what
belongs in the target container image, what stays as runtime configuration, and
what can be left behind entirely.

Without a systematic classification, migration becomes a manual audit of every
artifact on the system. Inspectah's triage system exists to do that audit
automatically and surface only the items that need human attention.

## Baseline subtraction: the foundation

The core idea is simple: if something already exists in the base image you are
migrating to, it does not require action. The sysadmin did not put it there; it
ships by default.

When inspectah has a baseline available (a target image to compare against), it
compares every package on the host against that image's package manifest. A
package like `glibc` that appears in both the host and the target image is
classified as **Baseline** -- already accounted for, no work needed.

The same logic applies to other artifact types. A config file that matches its
RPM-shipped default is Baseline. A service running in the same state as the
base image is Baseline. The baseline is always the starting point: subtract
what is already handled, then focus on the remainder.

{% raw %}
<div class="diagram-embed" style="margin: 2em 0;">
  <iframe id="diagram-triage-decision-tree"
          src="../diagrams/triage-decision-tree.html"
          title="Triage Decision Tree — interactive preview"
          width="100%" height="450" frameborder="0"
          loading="lazy" tabindex="0"></iframe>
  <div style="margin-top: 0.5em;">
    <button id="btn-diagram-triage-decision-tree"
            onclick="(function(btn){
      var iframe = document.getElementById('diagram-triage-decision-tree');
      if (iframe.requestFullscreen) {
        iframe.requestFullscreen();
        iframe._triggerBtn = btn;
        document.addEventListener('fullscreenchange', function handler() {
          if (!document.fullscreenElement) {
            document.removeEventListener('fullscreenchange', handler);
            if (iframe._triggerBtn) {
              iframe._triggerBtn.focus();
              iframe._triggerBtn = null;
            }
          }
        });
      } else {
        window.open(iframe.src, '_blank');
      }
    })(this)"
            aria-label="Open Triage Decision Tree in fullscreen">
      Open interactive diagram
    </button>
  </div>
  <p><em>The classification decision tree: how inspectah routes each item to Baseline, Site, or Investigate based on available evidence. Click "Open interactive diagram" for zoom, pan, and click-to-expand detail.</em></p>
</div>
{% endraw %}

## The three buckets

Every item inspectah classifies lands in one of three buckets:

**Baseline** -- present in the target image or at its default state. No
migration action required. This is the vast majority of items on a well-managed
host: the operating system packages, their default configs, standard service
states.

**Site** -- something the sysadmin explicitly added or changed. A package
installed from EPEL, a modified `sshd_config`, a non-default service state, a
user-deployed container workload. These are the items that define what makes
this host (or fleet) different from a stock image. They are the migration work.

**Investigate** -- inspectah cannot determine the classification with
confidence. Maybe no baseline is available for comparison. Maybe a package was
installed locally from an RPM file with no repository source. Maybe a config
file is not owned by any installed package and its origin is unclear.

### Why three categories, not two

A binary system -- "needs action" vs. "no action" -- seems simpler. But it
forces a bad choice: when inspectah is uncertain, should it say "no action" and
risk missing something, or "needs action" and bury real findings under noise?

Investigate is an explicit uncertainty marker. It says: *I don't have enough
information to classify this -- human, please look.* This is a deliberate
design principle. The tool should not guess when it lacks evidence. An
incorrect Baseline classification (false negative) could cause a production
system to lose a critical package during migration. An incorrect Site
classification (false positive) adds noise but does not cause harm.

Investigate biases toward safety. When in doubt, flag it.

In practice, Investigate items often shrink dramatically once a baseline is
available. A host scanned without a target image comparison will show many
Investigate results. The same host scanned with a baseline typically shows very
few, because the comparison resolves most ambiguity.

## Fleet consensus: the second axis

Single-host triage answers: *what matters on this host?* But many
organizations run fleets -- 10, 50, or 200 hosts in the same role. When you
manage a fleet of web servers, the question changes from "what is on this host"
to "what is consistent across all my hosts, and where do they diverge?"

Fleet analysis adds a second classification dimension: how prevalent is each
item across the fleet?

**Universal** -- present on every host in the fleet. When all 50 web servers
have `httpd` installed, that is a universal package. It almost certainly
belongs in the target image.

**Partial** -- present on some hosts but not all. Maybe 30 of 50 hosts have a
monitoring agent. This is likely intentional but might reflect an incomplete
rollout, or a subset of hosts serving a slightly different function.

**Divergent** -- present on only a few hosts. When 3 of 50 hosts have a
debugging tool installed, that is probably ad-hoc -- someone installed it to
troubleshoot and never removed it.

These fleet buckets map back to the single-host triage model for consistency:
Universal items map to Baseline (they are the fleet's effective "base image"),
Partial items map to Site (they are meaningful additions worth preserving),
and Divergent items map to Investigate (low prevalence suggests anomaly or
one-off activity that needs a human decision).

### How prevalence zones work

The prevalence calculation is straightforward:

- **Consensus** -- the item appears on every host (count equals total)
- **NearConsensus** -- the item appears on at least half the hosts
- **Divergent** -- the item appears on fewer than half the hosts

The threshold at 50% is a pragmatic choice. Below half, an item is more likely
noise than intent. Above half, it is more likely deliberate -- even if a few
hosts missed a rollout.

## Design choices: what counts as "the same"

Fleet analysis has to decide what "the same item" means when hosts are not
perfectly identical. Two choices in particular shape the results:

### Version differences are universal, not divergent

If host A has `httpd-2.4.57` and host B has `httpd-2.4.58`, inspectah treats
this as the same package at universal prevalence. The package *name and
architecture* determine identity, not the version.

This is intentional. Version differences between hosts usually reflect patching
timing, not operational divergence. Host B got the update on Tuesday; host A
gets it Thursday. The sysadmin intent -- "all web servers run httpd" -- is
identical. Treating version skew as divergence would flood the results with
noise that obscures real differences.

### Config differences are the real signal

Configuration is different. If host A and host B both have
`/etc/ssh/sshd_config` but with different contents, that is genuine
operational divergence. Maybe one host permits root login and the other does
not. Maybe one has a custom port. These differences matter for migration
because they represent different operational decisions that cannot be collapsed
into a single image configuration.

When a config path has multiple variants across a fleet (3 hosts have version
A of the file, 2 hosts have version B), the path's overall classification
reflects the most divergent variant. The system does not hide the divergence
by averaging -- it surfaces it, because config divergence is exactly the kind
of detail a migration engineer needs to resolve.

## What triage does not do

Triage classifies. It does not prescribe.

Inspectah does not tell you *how* to handle a Site package or an Investigate
config file. It does not generate a Containerfile. It does not decide whether a
package should be layered into the image at build time or installed at runtime.
Those are migration decisions that depend on your architecture, your compliance
requirements, and your operational model.

The classification system gives you a prioritized, organized view of what
exists on your hosts. The migration decisions remain yours.

For exact definitions of every triage reason and the complete classification
rules, see the [Triage Classification Reference](../reference/triage-classification.md).
