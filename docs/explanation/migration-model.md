---
title: Migration Model
parent: Explanation
nav_order: 2
---

# Migration Model

This page explains why inspectah exists, what problem it solves, and where it
fits in a package-mode to image-mode migration. If you already understand image
mode and just want to run the tool, see the [Getting Started](../getting-started.md) guide.

## Package mode and image mode

Traditional RPM-based Linux systems run in **package mode**. The operating system
is installed once, then maintained over time with `dnf install`, `dnf update`,
and manual configuration changes applied directly to the running host. Each host
accumulates its own history of packages, config edits, enabled services, and
one-off fixes. Over months and years, hosts that started identical drift apart.
Two "identical" web servers may have different package versions, different
firewall rules, and different cron jobs — and nobody can say exactly when they
diverged.

**Image mode** (built on [bootc](https://containers.github.io/bootc/)) replaces
this pattern with container-native OS management. Instead of mutating a running
system, you define the desired state in a Containerfile, build it into a bootc
container image, and deploy that image to hosts. Updates are atomic: you build a
new image version and the host switches to it at next reboot. Every host running
the same image tag is identical by construction, not by hope.

The benefits are significant:

- **Reproducibility** — the container image is the source of truth, not the
  running host
- **Atomic updates** — no partial-upgrade states, no "reboot and pray"
- **Rollback** — the previous image is one reboot away
- **Testability** — you can validate an image in CI before it touches production
- **Drift elimination** — hosts don't accumulate snowflake changes over time

The trade-off is that you need to know what's actually on your hosts *today*
before you can define the target image. That's the gap inspectah fills.

## Where inspectah fits

A package-to-image migration has three broad phases:

1. **Analysis** — understand what's on the current hosts
2. **Build** — create a bootc container image that captures the desired state
3. **Deploy** — switch hosts to the new image

inspectah handles **phases 1 and 2**. It scans a running package-mode host,
classifies everything it finds, generates migration artifacts — a
Containerfile, reports, and a structured snapshot — and can build the
resulting bootc container image via `inspectah build`. It answers the
question: "What is on this host that the base image doesn't already provide?"
and then automates the image build.

Deploying the image (phase 3) is done with `bootc switch` or your existing
provisioning tooling. inspectah does not deploy images and does not manage
anything at runtime.

## The inspection pipeline

When you run `inspectah scan` on a host, the tool walks through a structured
pipeline:

{% raw %}
<div class="diagram-embed" style="margin: 2em 0;">
  <iframe id="diagram-conceptual-pipeline"
          src="../diagrams/conceptual-pipeline.html"
          title="Conceptual Pipeline — interactive preview"
          width="100%" height="450" frameborder="0"
          loading="lazy" tabindex="0"></iframe>
  <div style="margin-top: 0.5em;">
    <button id="btn-diagram-conceptual-pipeline"
            onclick="(function(btn){
      var iframe = document.getElementById('diagram-conceptual-pipeline');
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
            aria-label="Open Conceptual Pipeline in fullscreen">
      Open interactive diagram
    </button>
  </div>
  <p><em>The full inspectah pipeline: inspect the host, subtract the baseline, classify items, and render migration artifacts. Click "Open interactive diagram" for zoom, pan, and click-to-expand detail.</em></p>
</div>
{% endraw %}

### Inspect

A set of inspectors examines different aspects of the host: packages, services,
configuration files, network settings, scheduled tasks, kernel parameters,
SELinux policy, users and groups, storage mounts, container workloads, and
non-RPM software. Each inspector produces structured data about what it finds.

### Baseline subtraction

The raw inventory isn't useful on its own — a typical host has hundreds of
packages, dozens of enabled services, and piles of config files that ship with
the base OS. You don't need to migrate those; they're already in the base image.

inspectah queries the target bootc base image (detected from the host's
`/etc/os-release`) and subtracts its contents from the inspection. Packages
that exist in the base image are removed from the list. Services whose
enable/disable state matches the base image presets are filtered out. The
result is a focused view of what the *operator* added or changed — not what
the base OS ships by default.

This baseline subtraction is fundamental to the tool's value. Without it, the
generated Containerfile would redundantly reinstall hundreds of packages that
are already present, and the reports would be full of noise.

### Classify and render

With the baseline-subtracted data, inspectah classifies each item and renders
output artifacts:

- A **Containerfile** with layer-ordered directives that reproduce the
  operator's additions on top of the base image
- An **HTML report** with interactive tables, search, and filtering for
  reviewing every finding
- A **markdown report** for text-based review
- A **JSON snapshot** capturing the full structured data for downstream
  tooling or re-rendering

The Containerfile is a *starting point*, not a finished product. It captures
the tool's best interpretation of what needs to be in the image, with FIXME
annotations where human judgment is required.

## What inspectah does not do

Understanding the boundaries is as important as understanding the capabilities:

- **Does not deploy images.** Switching a host from package mode to image mode
  is done with `bootc switch`. inspectah has no role at deploy time.
- **Does not manage runtime state.** Once it produces artifacts (or builds
  the image), it's done. There is no daemon, no agent, no ongoing process.
- **Does not make migration decisions for you.** It presents findings and
  classifications. Whether a particular package belongs in your image, whether a
  config file should be baked in or templated at deploy time, whether a cron job
  should become a systemd timer — those are decisions the sysadmin makes.

## The role of the sysadmin

inspectah is designed as an assistant, not an autopilot. The tool does the
tedious work of inventorying a host and generating structured output, but the
migration itself requires human judgment at every step.

The **refine** workflow makes this explicit. After a scan, `inspectah refine`
serves an interactive UI where the sysadmin reviews every finding: toggling
packages in or out of the Containerfile, choosing provisioning strategies for
user accounts, excluding config files that shouldn't be baked into the image.
The Containerfile re-renders live as decisions are made. This is deliberately
a human-in-the-loop process — the tool surfaces the data, the sysadmin makes
the calls.

For fleet migrations (multiple hosts moving to the same image), the **fleet**
workflow aggregates scans from many hosts and shows item prevalence — "this
package is on 47 of 50 hosts" — so the sysadmin can make informed decisions
about what belongs in the shared image versus what's host-specific.

The core scan-refine-build-fleet workflow is available today.

## Putting it together

A typical migration workflow looks like this:

1. Run `inspectah scan` on each package-mode host
2. Use `inspectah refine` to review and adjust findings per host
3. Use `inspectah fleet` to aggregate across hosts and identify the common
   image contents
4. Review the generated Containerfile and customize further as needed
5. Build the image with `inspectah build` (or `podman build` manually)
6. Test the image in a staging environment
7. Deploy with `bootc switch` or your provisioning system

inspectah owns steps 1-5. Steps 6-7 are yours. The tool's job is to make
the scan-refine-build cycle fast, thorough, and auditable — so you can spend
your time on the decisions that matter instead of manually inventorying hosts.
