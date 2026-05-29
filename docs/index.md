---
title: Home
layout: home
nav_order: 1
---

# inspectah

inspectah scans package-mode Fedora, CentOS Stream, and RHEL hosts and generates the artifacts you need to migrate them to bootc image mode -- Containerfiles, package lists, config trees, and systemd service maps. It analyzes what is installed beyond the base image so you know exactly what to carry forward.

## Quick start

```bash
# Scan a host
sudo inspectah scan

# Refine the output interactively
inspectah refine <snapshot.tar.gz>
```

See [Getting Started](getting-started) for a full walkthrough.

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
  <p><em>The end-to-end inspectah workflow: scan hosts, refine findings, aggregate fleets, and build images. Click "Open interactive diagram" for zoom, pan, and click-to-expand detail.</em></p>
</div>
{% endraw %}

## CLI reference

The current CLI surface:

| Command | Description |
|---------|-------------|
| `inspectah scan` | Scan the current system and produce a migration snapshot |
| `inspectah refine` | Interactively refine scan output and re-render artifacts |
| `inspectah fleet init` | Initialize a fleet directory from host snapshots |
| `inspectah fleet aggregate` | Aggregate snapshots into a fleet-wide specification |
| `inspectah version` | Print version, commit, and build date |

See [CLI Reference](reference/) for the full flag tables.

## Documentation

{: .fs-5 }

**[Tutorials](tutorials/)** -- Step-by-step guides to get started with inspectah, from first scan to fleet aggregation.

**[How-To Guides](how-to/)** -- Task-oriented recipes for specific workflows like building a bootc image from inspectah output.

**[Reference](reference/)** -- Complete CLI flags, output formats, and schema documentation.

**[Explanation](explanation/)** -- Architecture, design decisions, and how baseline subtraction works under the hood.

**[Contributing](contributing-index/)** -- How to build from source, run tests, and submit changes.
