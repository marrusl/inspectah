---
title: Getting Started
nav_order: 2
---

# Getting Started

This tutorial walks you through your first inspectah scan. By the end,
you will have scanned a host, reviewed the output artifacts, and opened
the refine UI to triage findings. Total time: about 10 minutes.

## Prerequisites

- A **RHEL, CentOS Stream, or Fedora** host (the system you want to migrate)
- **Podman** installed (`dnf install podman`)
- **Root access** on the target host (inspectah reads system configuration)
- **Registry authentication** for RHEL hosts — inspectah pulls the matching
  base image to classify findings:

```bash
podman login registry.redhat.io
```

If you skip registry auth, inspectah will fail to pull the baseline image.
You can work around this with `--no-baseline`, but classification quality
degrades significantly.

## Install inspectah

### RPM (Fedora / RHEL / CentOS Stream)

```bash
sudo dnf copr enable marrusl/inspectah
sudo dnf install inspectah
```

### From source

```bash
git clone https://github.com/marrusl/inspectah.git
cd inspectah
cargo build --release -p inspectah-cli
sudo install target/release/inspectah /usr/local/bin/
```

Requires Rust 1.80+ and a C linker.

Verify the install:

```bash
inspectah version
```

You should see the version number, git commit, and build date.

## How inspectah works

Before diving into the commands, here is a visual overview of what
inspectah does when you scan a host:

{% raw %}
<div class="diagram-embed" style="margin: 2em 0;">
  <iframe id="diagram-conceptual-pipeline"
          src="diagrams/conceptual-pipeline.html"
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
  <p><em>The inspectah pipeline: from host inspection through baseline subtraction to migration artifacts. Click "Open interactive diagram" for zoom, pan, and click-to-expand detail.</em></p>
</div>
{% endraw %}

## Scan your first host

Run inspectah as root on the system you want to migrate:

```bash
sudo inspectah scan
```

inspectah runs a four-stage pipeline:

1. **Detect** the OS and resolve a matching base image
2. **Pull** the base image (you will see download progress)
3. **Inspect** the system — RPMs, configs, services, users, repos,
   containers, storage, and more (11 inspectors total)
4. **Render** migration artifacts and package them into a tarball

A typical scan takes 30--90 seconds depending on the number of installed
packages and network speed for the base image pull. On subsequent runs,
the base image is cached locally.

When the scan finishes, you will see output like:

```
Scan complete (42.3s) — 847 packages, 12 configs, 4 services, 2 containers
Report: myhost-20260527-143000.tar.gz
To review: inspectah refine myhost-20260527-143000.tar.gz
```

The tarball is written to your current directory. The filename includes
the hostname and a timestamp.

## Understand the output

Extract the tarball to see what inspectah produced:

```bash
tar tzf myhost-*.tar.gz
```

Key files inside:

| File | Purpose |
|------|---------|
| `Containerfile` | Image build definition — the starting point for `podman build` |
| `audit-report.md` | Findings summary: what was detected, storage plan, version drift |
| `report.html` | Interactive HTML dashboard of all findings |
| `secrets-review.md` | Details of redacted sensitive content for your review |
| `README.md` | Build commands and a FIXME checklist |
| `kickstart-suggestion.ks` | Deploy-time settings (network, storage, boot) |
| `inspection-snapshot.json` | Structured data — the full snapshot, re-renderable |
| `config/` | Modified config files to COPY into the image |

Conditional files may also appear depending on your system: `quadlet/`
for container workloads, `inspectah-users.toml` / `inspectah-users.ks`
for user and group definitions, and `entitlement/` / `rhsm/` for
RHEL subscription data.

For a complete description of every artifact, see
[Output Artifacts](reference/output-artifacts.md).

## Open the refine UI

The refine command starts a local web server so you can review and
curate the scan findings in your browser:

```bash
inspectah refine myhost-*.tar.gz
```

You will see:

```
Loading snapshot...
Starting refine server on http://127.0.0.1:8642
Press Ctrl-C to stop.
```

Your browser opens automatically. If it does not, navigate to
`http://127.0.0.1:8642` manually.

The refine UI shows every finding organized by section (packages,
configs, services, etc.). Each item has a triage classification and
an include/exclude toggle. Your changes are autosaved — close the
browser and stop the server with Ctrl-C when you are done.

If you are accessing a remote host over SSH, forward the port:

```bash
ssh -L 8642:localhost:8642 user@remote-host
```

Then open `http://127.0.0.1:8642` in your local browser.

{% raw %}
<div class="diagram-embed" style="margin: 2em 0;">
  <iframe id="diagram-user-flow"
          src="diagrams/user-flow.html"
          title="User Flow — interactive preview"
          width="100%" height="450" frameborder="0"
          loading="lazy" tabindex="0"></iframe>
  <div style="margin-top: 0.5em;">
    <button id="btn-diagram-user-flow"
            onclick="(function(btn){
      var iframe = document.getElementById('diagram-user-flow');
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
            aria-label="Open User Flow in fullscreen">
      Open interactive diagram
    </button>
  </div>
  <p><em>The end-to-end user workflow: scan, refine, fleet aggregate, and build. Click "Open interactive diagram" for zoom, pan, and click-to-expand detail.</em></p>
</div>
{% endraw %}

## Understand triage classifications

inspectah classifies every finding into one of three buckets:

| Classification | Meaning | Action |
|---------------|---------|--------|
| **Baseline** | Already provided by the base image — no action needed | Excluded from the Containerfile (subtracted from scope) |
| **Site** | Specific to this host — likely needs to be carried forward | Included in the Containerfile by default |
| **Investigate** | Ambiguous — inspectah could not confidently classify it | Review manually and decide include or exclude |

Baseline subtraction is the core value of running inspectah against a
known base image. Instead of migrating everything, you only carry
forward what the base image does not already provide.

For details on how classification works and the fleet-level extensions
(Universal, Partial, Divergent), see
[Triage Classification](reference/triage-classification.md).

## Next steps

You have scanned a host, reviewed the output, and seen the triage
model in action. From here:

- **Review and refine findings** — use the refine UI to curate
  include/exclude decisions. See
  [How to Review and Refine](how-to/review-and-refine.md).
- **Build a bootc image** — take the Containerfile and config tree
  from your scan output and build with `podman build`. See
  [How to Build a bootc Image](how-to/build-bootc-image.md).
- **Aggregate a fleet** — scan multiple hosts and merge the results
  to find common patterns across your environment. See
  [How to Aggregate a Fleet](how-to/fleet-aggregation.md).
- **Explore the full CLI** — see all available commands and flags in
  the [CLI Reference](reference/cli.md).
