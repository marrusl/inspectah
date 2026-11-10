---
title: Getting Started
nav_order: 2
---

# Getting Started

This tutorial walks you through your first inspectah scan. By the end,
you will have scanned a host, reviewed the output artifacts, and opened
the refine UI to triage findings. Total time: about 10 minutes.

## Prerequisites

- A **Fedora, CentOS Stream, or RHEL** host (the system you want to migrate)
- **Podman** installed (`dnf install podman`)
- **Root access** on the target host (inspectah reads system configuration)

**For RHEL users:** inspectah pulls the matching base image to classify
findings. RHEL base images require registry authentication:

```bash
sudo podman login registry.redhat.io
```

Fedora and CentOS Stream base images are available from public registries
and do not require authentication. If you skip registry auth on RHEL,
inspectah will fail to pull the baseline image.

**For disconnected or air-gapped environments:** Pull the base image on a
connected machine, save it as a tarball (`podman save -o baseline.tar <image-ref>`),
transfer the tarball to your target host, and load it (`podman load -i baseline.tar`).
Alternatively, configure a local or mirror registry.

## Install inspectah

### RPM (Fedora / RHEL / CentOS Stream)

```bash
sudo dnf copr enable mrussell/inspectah
sudo dnf install inspectah
```

### macOS (Homebrew)

```bash
brew tap marrusl/inspectah
brew install inspectah
```

Apple Silicon only. For Intel Macs, build from source below.

### Windows (WSL2)

inspectah ships no Windows binary, only Linux and macOS (Apple Silicon)
builds. To run `inspectah refine` on a Windows workstation, install the
Linux binary inside WSL2 and run refine there. WSL2 forwards localhost
ports to Windows by default, so the printed URL should open in your
Windows browser. This should work but has not been tested yet. If the
browser does not open automatically, paste the URL that refine prints
into your Windows browser.

### From source

```bash
git clone https://github.com/marrusl/inspectah.git
cd inspectah
cargo build --release -p inspectah-cli
sudo install target/release/inspectah /usr/local/bin/
```

Requires Rust 1.85 or newer (the workspace uses edition 2024), a C linker,
and Node.js 26 with npm. The `inspectah-web` crate's `build.rs` runs
`npm ci` and `npm run build` to build the refine UI, and the build fails
on a fresh clone if Node.js is missing.

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
          title="Conceptual Pipeline, interactive preview"
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
3. **Inspect** the system: RPMs, configs, services, users, repos,
   containers, storage, and more (11 inspectors by default)
4. **Render** migration artifacts and package them into a tarball

A typical scan takes 30--90 seconds depending on the number of installed
packages and network speed for the base image pull. On subsequent runs,
the base image is cached locally.

When the scan finishes, you will see output like this, captured from a
RHEL 9.8 test host:

```
Detecting source system...
  Red Hat Enterprise Linux 9.8 (Plow)
Resolving target image...
  registry.redhat.io/rhel9/rhel-bootc:9.8 (OsRelease)
Preparing baseline image registry.redhat.io/rhel9/rhel-bootc:9.8...
Pulled registry.redhat.io/rhel9/rhel-bootc:9.8 (sha256:173c055cc9c7)
  Baseline extracted: 457 packages
  Resolved via: os-release (auto-detected)
Inspecting host rhel9...
  ✓ RPM packages                 584 packages, 4 repos
  ✓ Services                     5 units
  ✓ Storage                      done
  ✓ Kernel & boot                done
  ✓ Network                      done
  ✓ Containers                   none found
  ✓ Users & groups               done
  ✓ Scheduled tasks              16 timers
  ✓ Config files                 62 modified
  ✓ SELinux                      done
  ✓ Non-RPM packages             4 ecosystems
      ELF binaries 13 · Python virtualenvs 2 · npm packages 2 · git repos 1

  ┄┄┄
  14 version changes (14 target-newer, 0 host-newer)
  62 modified configs · 2 npm packages · 1 git repo

  Inspected in 27s
  Report: rhel9-20261104-222606.tar.gz
  To review: inspectah refine rhel9-20261104-222606.tar.gz
```

The tarball is written to your current directory. The filename includes
the hostname and a timestamp.

## Understand the output

Extract the tarball to see what inspectah produced:

```bash
tar tzf rhel9-*.tar.gz
```

Key files inside:

| File | Purpose |
|------|---------|
| `Containerfile` | Image build definition: the starting point for `podman build` |
| `audit-report.md` | Findings summary: what was detected, storage plan, version drift |
| `audit-report.html` | HTML audit report of all findings |
| `secrets-review.md` | Details of redacted sensitive content for your review |
| `README.md` | Build commands and a FIXME checklist |
| `kickstart-suggestion.ks` | Deploy-time settings (network, storage, boot) |
| `inspection-snapshot.json` | Structured data: the full snapshot, re-renderable |
| `config/` | Modified config files to COPY into the image |

Conditional files may also appear depending on your system: `quadlet/`
for container workloads, `language-packages/` for pip/npm/gem
environments, `repoless-packages/` for RPMs with no repository source,
`inspectah-users.toml` / `inspectah-users.ks` for user and group
definitions, and `subscription/` for RHEL subscription data.

For a complete description of every artifact, see
[Output Artifacts](reference/output-artifacts.md).

### Beyond RPMs

inspectah detects language packages (pip, npm, gem) automatically.
The Containerfile includes install directives for detected environments,
with confidence levels that control whether directives are active,
commented-out, or advisory. See
[Handle Non-RPM Software](how-to/non-rpm-software.md) for details.

To also capture files under `/opt`, `/srv`, `/usr/local` that are not
managed by any package manager, add `--include-unmanaged`:

```bash
sudo inspectah scan --include-unmanaged
```

inspectah prints the number and total size of the unmanaged files it found
and asks `Include in tarball? [Y/n]`. Add `-y` to skip the prompt.

Packages installed from local `.rpm` files or disabled repositories are
flagged automatically. See
[Handle Repo-less RPMs](how-to/repoless-rpms.md) for the upload and
resolution workflow.

## Open the refine UI

The refine command starts a local web server so you can review and
curate the scan findings in your browser. Run it on your workstation, not
on the scanned host. Refine needs nothing from the scanned host: it works
from the tarball, plus any RPMs you upload in the browser. Copy the
tarball over first. This command is illustrative: replace `user@host`
with your own login and host, and the file name with your tarball's.

```bash
scp user@host:rhel9-20261104-222606.tar.gz .
inspectah refine rhel9-*.tar.gz
```

You will see these three lines, which inspectah prints to stderr:

```
Loading snapshot...
Starting refine server on http://127.0.0.1:8642
Press Ctrl-C to stop.
```

Your browser opens automatically. If it does not, navigate to
`http://127.0.0.1:8642` manually.

The refine UI shows every finding organized by section (packages,
configs, services, language packages, unmanaged files, etc.). Each item
has a triage classification (see below). Actionable items have an
include/exclude toggle; Advisory and Inventory items do not. Your changes
are autosaved: close the browser and stop the server with Ctrl-C when
you are done.

**Running refine on the scanned host.** This is possible but not
recommended. Forward the port from your workstation with
`ssh -L 8642:127.0.0.1:8642 user@host`, then open `http://127.0.0.1:8642`.
Use `127.0.0.1` in the forward, not `localhost`: `localhost` can resolve
to `::1`, which refine does not listen on. Refine has no per-session
authentication yet, so while it runs, other local users on that host can
reach its port and read and change your session. Use this only on a host
where you are the only user.

{% raw %}
<div class="diagram-embed" style="margin: 2em 0;">
  <iframe id="diagram-user-flow"
          src="diagrams/user-flow.html"
          title="User Flow, interactive preview"
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
  <p><em>The end-to-end user workflow: scan, refine, aggregate, and build. Click "Open interactive diagram" for zoom, pan, and click-to-expand detail.</em></p>
</div>
{% endraw %}

## Understand triage classifications

inspectah assigns packages, configuration files, services, and several
other kinds of finding to one of three triage buckets:

| Classification | Meaning | Action |
|---------------|---------|--------|
| **Baseline** | Already provided by the base image: no action needed | Excluded from the Containerfile (subtracted from scope) |
| **Site** | Specific to this host: likely needs to be carried forward | Included in the Containerfile by default |
| **Investigate** | Ambiguous: inspectah could not confidently classify it | Review manually and decide include or exclude |

Separately from its bucket, every finding carries a disposition that says
whether you can decide it at all:

| Disposition | Meaning | Toggle in the refine UI |
|-------------|---------|-------------------------|
| **Actionable** | You decide whether it reaches the image | Yes |
| **Advisory** | Shown with a rationale. No include control. A configuration-file advisory (modernization, cross-tree symlink) is not copied into the image. The unbacked `/var` advisory is the exception: the export provisions those directories through `inspectah-var.conf`, or `RUN mkdir -p` when the mode was not recorded | No |
| **Inventory** | Shown for reference only | No |

The refine server refuses a request to toggle an Advisory or Inventory
finding. See [Finding dispositions](reference/triage-classification.md#finding-dispositions)
for the details.

Baseline subtraction is the core value of running inspectah against a
known base image. Instead of migrating everything, you only carry
forward what the base image does not already provide.

For details on how classification works and the aggregate-level extensions
(Universal, Partial, Divergent), see
[Triage Classification](reference/triage-classification.md).

## Next steps

You have scanned a host, reviewed the output, and seen the triage
model in action. From here:

- **Review and refine findings**: use the refine UI to curate
  include/exclude decisions. See
  [How to Review and Refine](how-to/review-and-refine.md).
- **Build a bootc image**: use `inspectah build` to build the refine
  export (or the scan tarball, if you made no decisions in refine), or
  use `podman build` manually with the generated Containerfile. See
  [How to Build a bootc Image](how-to/build-bootc-image.md).
- **Roll up multiple hosts**: migrating more than one system? Scan them
  all and aggregate the results to find common patterns. See
  [How to Aggregate Multiple Hosts](how-to/aggregation.md).
- **Handle non-RPM software**: pip, npm, and gem packages are
  detected automatically. See
  [Handle Non-RPM Software](how-to/non-rpm-software.md).
- **Resolve repo-less packages**: packages without a repository source
  need manual handling. See
  [Handle Repo-less RPMs](how-to/repoless-rpms.md).
- **Explore the full CLI**: see all available commands and flags in
  the [CLI Reference](reference/cli.md).
