---
title: Your First Migration
parent: Tutorials
nav_order: 1
---

# Your First Migration

This tutorial walks you through a complete migration analysis from start
to finish. You will scan a running web server, understand what inspectah
found, refine the results in the triage UI, export a curated artifact set,
and build a bootc container image. Total time: about 20 minutes.

## The scenario

You have a CentOS Stream 9 host running Apache (`httpd`) as a web server.
(The same workflow applies to Fedora and RHEL hosts.) Over time the system
accumulated the usual operational reality:

- Apache with a custom virtual host configuration in `/etc/httpd/conf.d/`
- A TLS certificate and key in `/etc/pki/tls/`
- A cron job in `/etc/cron.d/log-cleanup` that rotates application logs
- A handful of packages installed via `dnf` over the months — `mod_ssl`,
  `certbot`, `jq`, `tmux`, `strace`
- A local user `webadmin` that owns the application content

You want to rebuild this host as a bootc image so you can manage it
declaratively, version its configuration in Git, and deploy updates
with `bootc upgrade` instead of `dnf update`.

inspectah will figure out what on this host actually matters — what the
base image already provides, what you intentionally added, and what is
just noise.

## Prerequisites

Before you start, make sure you have:

- **inspectah installed** — see [Getting Started](../getting-started.md)
  for installation options (RPM or from source)
- **Root access** on the host you want to scan
- **Podman installed** (`dnf install podman`) — needed for the build step

**For RHEL users:** inspectah pulls a base image for baseline subtraction.
RHEL base images require registry authentication:

```bash
podman login registry.redhat.io
```

Fedora and CentOS Stream base images are on public registries and do not
require authentication.

## Step 1: Scan the host

SSH into your web server and run inspectah as root:

```bash
sudo inspectah scan
```

inspectah detects the source system, resolves and pulls the target base
image, then runs its inspection pipeline:

```
Detecting source system...
  CentOS Stream 9 (x86_64)
Resolving target image...
  quay.io/centos-bootc/centos-bootc:stream9 (OsRelease)
Pulling quay.io/centos-bootc/centos-bootc:stream9...
Inspecting host webserver01...

  ✓ RPM packages               847 packages, 4 repos
  ✓ Services                    8 units
  ✓ Storage                     done
  ✓ Kernel & boot               done
  ✓ Network                     done
  ✓ Containers                  none found
  ✓ Users & groups              done
  ✓ Scheduled tasks             1 timer
  ✓ Config files                14 modified
  ✓ SELinux                     done
  ✓ Non-RPM packages            none found

  ┄┄┄
  14 modified configs

  Inspected in 28.7s
  Report: webserver01-20260610-093000.tar.gz
  To review: inspectah refine webserver01-20260610-093000.tar.gz
```

The scan typically takes 30--90 seconds. The base image is cached after
the first pull, so subsequent scans are faster.

## Step 2: Examine the tarball

The scan produced a tarball named after your hostname and a timestamp.
List its contents:

```bash
tar tzf webserver01-20260610-093000.tar.gz
```

You should see something like:

```
webserver01-20260610-093000/
webserver01-20260610-093000/Containerfile
webserver01-20260610-093000/audit-report.md
webserver01-20260610-093000/audit-report.html
webserver01-20260610-093000/secrets-review.md
webserver01-20260610-093000/README.md
webserver01-20260610-093000/kickstart-suggestion.ks
webserver01-20260610-093000/inspection-snapshot.json
webserver01-20260610-093000/config/
webserver01-20260610-093000/config/etc/httpd/conf.d/webapp.conf
webserver01-20260610-093000/config/etc/cron.d/log-cleanup
webserver01-20260610-093000/config/etc/logrotate.d/webapp
```

Key files:

| File | What it is |
|------|------------|
| `Containerfile` | A draft image build definition ready for `podman build` |
| `audit-report.md` | Human-readable summary of everything inspectah found |
| `audit-report.html` | HTML audit report — same data, visual format |
| `secrets-review.md` | Any redacted sensitive content flagged for your review |
| `inspection-snapshot.json` | Machine-readable snapshot — the refine UI reads this |
| `config/` | Modified config files, ready to COPY into the image |

For a complete description of every artifact, see
[Output Artifacts](../reference/output-artifacts.md).

## Step 3: Read the audit report

Open `audit-report.md` to understand what inspectah found. The report
groups findings by section. Here is what you would see for our web
server:

**Packages** — 847 RPMs installed. 812 match the CentOS Stream 9 base
image and are classified as **baseline** (already provided, no action
needed).
35 are classified as **site** (you installed them). These include
`httpd`, `mod_ssl`, `certbot`, `jq`, `tmux`, and `strace`.

**Configs** — 14 modified configuration files. 2 are baseline defaults
that ship with the base image. 9 are site-specific (your Apache vhost,
cron jobs, logrotate rules). 3 are marked **investigate** — inspectah
could not confidently classify them. These might be configs that a
package post-install script modified, or files that drifted from their
default state.

**Services** — 8 enabled systemd units. 3 are baseline (sshd, chronyd,
auditd). 5 are site-specific (httpd, certbot-renew.timer, crond, and
two others).

**Users** — 2 non-system users found: `webadmin` (uid 1001) and the
default `cloud-user`.

The audit report tells you the *what*. The refine UI is where you
decide *what to do about it*.

## Step 4: Open the refine UI

Start the refine server, pointing it at your scan tarball:

```bash
inspectah refine webserver01-20260610-093000.tar.gz
```

You will see:

```
Loading snapshot...
Starting refine server on http://127.0.0.1:8642
Press Ctrl-C to stop.
```

Your browser opens automatically. If it does not, navigate to
`http://127.0.0.1:8642`.

If you are working on a remote host over SSH, forward the port first:

```bash
ssh -L 8642:localhost:8642 user@webserver01
```

Then open `http://127.0.0.1:8642` in your local browser.

The refine UI shows every finding organized by section. Each item
displays its triage classification (**baseline**, **site**, or
**investigate**) and an include/exclude toggle. Your changes are
autosaved as you work.

## Step 5: Make triage decisions

Work through each section and decide what belongs in your image. Here
is how to think about the findings on our web server:

### Packages — keep what you need

The 812 baseline packages are already excluded — the base image provides
them. Focus on the 35 site packages:

- **Keep:** `httpd`, `mod_ssl`, `certbot` — these define the workload.
  They should be `dnf install` lines in your Containerfile.
- **Keep:** `jq` — you use it in operational scripts on this host.
- **Exclude:** `tmux`, `strace` — debugging tools you installed during
  troubleshooting. They do not belong in the production image. Toggle
  them to excluded.

### Configs — carry forward what matters

- **Keep:** `config/etc/httpd/conf.d/webapp.conf` — your Apache virtual
  host definition. This gets COPY'd into the image.
- **Keep:** `config/etc/cron.d/log-cleanup` — your application log
  rotation cron job.
- **Investigate:** `config/etc/sysctl.d/99-tuning.conf` — inspectah
  flagged this because it could not determine if you added it or if a
  package post-install script created it. Open it, read the contents. If
  it contains tuning parameters you set intentionally, toggle it to
  included. If it looks like a package default, exclude it.

### Services — enable what the workload needs

- **Keep:** `httpd.service`, `certbot-renew.timer` — the core workload
  services.
- **Exclude:** `cockpit.socket` — you may have enabled Cockpit for
  management, but the bootc image will be managed declaratively. Toggle
  it to excluded unless you want Cockpit in your image.

### Users — include operational accounts

- **Keep:** `webadmin` — this user owns the application content.
  inspectah will generate the appropriate user creation directives.
- **Exclude:** `cloud-user` — this is a cloud-init default. Your bootc
  image will handle user provisioning differently.

The goal is not to migrate everything. The goal is to migrate what
defines this workload and leave behind what was incidental to the
host's lifetime.

## Step 6: Export refined results

After you finish triaging, export the refined artifact set. In the
refine UI, use the export function to save your curated results.

The export produces an updated tarball that reflects your triage
decisions — only the items you marked as included appear in the
generated Containerfile and config tree.

When you are done, stop the refine server with Ctrl-C in the terminal.

## Step 7: Build the image

The fastest way to build is with `inspectah build`:

```bash
inspectah build webserver01-20260610-093000.tar.gz --tag my-webserver:v1
```

This extracts the tarball, handles RHEL subscription cert mounting
automatically (if needed), and runs `podman build` for you.

To preview the generated Containerfile first, extract and inspect it:

```bash
tar xzf webserver01-20260610-093000.tar.gz
cd webserver01-20260610-093000
cat Containerfile
```

You will see something like:

```dockerfile
FROM quay.io/centos-bootc/centos-bootc:stream9

# Site packages
RUN dnf install -y \
    httpd \
    mod_ssl \
    certbot \
    jq \
    && dnf clean all

# Site configs
COPY config/etc/httpd/conf.d/webapp.conf /etc/httpd/conf.d/webapp.conf
COPY config/etc/cron.d/log-cleanup /etc/cron.d/log-cleanup

# Enable services
RUN systemctl enable httpd.service certbot-renew.timer
```

You can also build manually with `podman build -t my-webserver .` if you
prefer. See [How to Build a bootc Image](../how-to/build-bootc-image.md)
for full details on RHEL entitlement handling, cross-architecture builds,
and pushing to a registry.

## What you accomplished

You started with a running host and ended with a bootc container image
that captures only what matters — the packages, configs,
and services that define your workload. Everything the base image
already provides was subtracted automatically. Everything incidental
was excluded by your triage decisions.

## What's next

- **Refine further** — review the
  [How to Review and Refine](../how-to/review-and-refine.md) guide for
  advanced triage techniques.
- **Aggregate a fleet** — if you have multiple hosts to migrate, scan
  them all and use fleet aggregation to find common patterns. See
  [How to Aggregate a Fleet](../how-to/fleet-aggregation.md).
- **Subtract a baseline** — learn how baseline subtraction works and
  how to use custom base images. See
  [How to Subtract a Baseline](../how-to/baseline-subtraction.md).
- **Explore the CLI** — see all available commands and flags in the
  [CLI Reference](../reference/cli.md).
