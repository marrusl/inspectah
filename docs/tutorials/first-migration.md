---
title: Your First Migration
parent: Tutorials
nav_order: 1
---

# Your First Migration

This tutorial walks you through a complete migration analysis from start
to finish. You will scan a running host, understand what inspectah found,
make decisions in the refine UI, export the result, and build a bootc
container image from that export. In the run captured for this page the
scan took under a minute and the image build about three minutes. Your
triage time is the open-ended part.

## The scenario

You have a RHEL 9.8 host that has been running a mixed workload for a
while. (The same workflow applies to CentOS Stream and Fedora hosts.) Over
time it accumulated the usual operational reality:

- Apache (`httpd`) and nginx, both enabled
- A Python application with a virtual environment in `/opt/myapp`, a
  second virtual environment in `/opt/legacy-app`, and an npm application
  in `/opt/webapp`
- Quadlet units for Redis and a web app, plus a Compose file in `/opt/myapp`
- Cron jobs and systemd timers, an SELinux file context rule for
  `/srv/www`, sshd moved to port 2222, and a tuned profile
- The EPEL repository, with a few packages installed from it
- Debugging packages you installed during troubleshooting: `strace`,
  `tcpdump`, and `tmux`

You want to rebuild this host as a bootc image so you can manage it
declaratively, version its configuration in Git, and deploy updates
with `bootc upgrade` instead of `dnf update`.

inspectah will figure out what on this host actually matters: what the
base image already provides, what you intentionally added, and what is
just noise.

The output on this page comes from a real run against a host with
synthetic drift, so a few names, such as `driftify-backup.timer`, belong to
that test host and will not appear on yours. Your counts will differ.

## Prerequisites

Before you start, make sure you have:

- **inspectah installed**: see [Getting Started](../getting-started.md)
  for installation options (RPM or from source)
- **Root access** on the host you want to scan
- **Podman installed** (`dnf install podman`), needed for the build step

**For RHEL users:** inspectah pulls a base image for baseline subtraction.
RHEL base images require registry authentication:

```bash
sudo podman login registry.redhat.io
```

The scan runs as root, so this login is root's. Step 7 runs the build as
your own user, which needs its own `podman login registry.redhat.io`.

Fedora and CentOS Stream base images are on public registries and do not
require authentication.

## Step 1: Scan the host

SSH into the host and run inspectah as root:

```bash
sudo inspectah scan
```

inspectah detects the source system, resolves and pulls the target base
image, then runs its inspection pipeline:

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

The `Containers` line counts running containers only. The Quadlet units
and Compose file on this host are captured anyway, as Step 7 shows.

## Step 2: Examine the tarball

The scan produced a tarball named after your hostname and a timestamp.
List its contents:

```bash
tar tzf rhel9-20261104-222606.tar.gz
```

You should see something like this. The full listing has 136 lines, so
this one is shortened: each `...` stands for omitted lines, and the lines
shown are unchanged.

```
rhel9-20261104-222606/
rhel9-20261104-222606/Containerfile
rhel9-20261104-222606/README.md
rhel9-20261104-222606/audit-report.html
rhel9-20261104-222606/audit-report.md
rhel9-20261104-222606/config/
...
rhel9-20261104-222606/config/etc/nsswitch.conf.bak
...
rhel9-20261104-222606/config/usr/lib/tmpfiles.d/inspectah-var.conf
rhel9-20261104-222606/drop-ins/
...
rhel9-20261104-222606/inspection-snapshot.json
rhel9-20261104-222606/kickstart-suggestion.ks
rhel9-20261104-222606/language-packages/
...
rhel9-20261104-222606/quadlet/
...
rhel9-20261104-222606/secrets-review.md
...
```

Key files:

| File | What it is |
|------|------------|
| `Containerfile` | A draft image build definition ready for `podman build` |
| `audit-report.md` | Human-readable summary of everything inspectah found |
| `audit-report.html` | HTML audit report, same data in a visual format |
| `secrets-review.md` | Any redacted sensitive content flagged for your review |
| `inspection-snapshot.json` | Machine-readable snapshot that the refine UI reads |
| `config/` | Modified config files, ready to COPY into the image |

For a complete description of every artifact, see
[Output Artifacts](../reference/output-artifacts.md).

## Step 3: Read the audit report

Open `audit-report.md` to understand what inspectah found. The report
groups findings by section. Here is what it says for this host:

**Packages:** the base image `registry.redhat.io/rhel9/rhel-bootc:9.8`
ships 457 packages, and the host has 584 installed. 182 packages on the
host are not in the base image; the report lists them under Added
Packages. Most are dependencies of the 25 top-level packages that refine lists
for you to decide on. One of
the 25, `epel-release`, is listed separately as a repo-enabling package.
14 shared packages carry a different version in the base image, and in
every case the base image is newer (the kernel, `grub2`, `tzdata`, and
others).

**Configs:** 47 unowned config files are listed, meaning no RPM ships
them: NetworkManager and firewalld settings, audit rules, `/etc/myapp/`,
sudoers drop-ins, systemd units and timers, and the like. The scan
reported 62 modified configs in total; Step 4 shows how refine sorts all
62.

**Kernel and SELinux:** two sysctl overrides, SELinux in enforcing mode,
and one custom file context rule that labels `/srv/www` as
`httpd_sys_content_t`.

**Services:** `httpd` and `nginx` are enabled, and `bluetooth` and
`kdump` are masked. The report flags three unit files in
`/etc/systemd/system/` (`kdump`, `bluetooth`, `sshd`) as advisories,
because they fully shadow the vendor units and base image updates to
those units would be ignored.

**Users:** three accounts: the one you log in with, `appuser` (both
interactive, with sudo and an SSH key), and `dbuser` (non-interactive).

**Non-RPM software:** 18 items, among them 13 ELF binaries, two Python
virtual environments, npm packages, and a git repository.

**Redactions:** 19 items were redacted; `secrets-review.md` lists them.

The audit report tells you the *what*. The refine UI is where you
decide *what to do about it*.

## Step 4: Open the refine UI

Start the refine server, pointing it at your scan tarball:

```bash
inspectah refine rhel9-20261104-222606.tar.gz
```

You will see these four lines, all printed to standard error:

```
Loading snapshot...
Starting refine server on http://127.0.0.1:8642
If accessing remotely: ssh -L 8642:localhost:8642 <host>
Press Ctrl-C to stop.
```

Your browser opens automatically. If it does not, navigate to
`http://127.0.0.1:8642`.

If you are working on a remote host over SSH, forward the port from your
workstation first. This line is a schematic example: replace `user@host`
with your own login and host.

```bash
ssh -L 8642:localhost:8642 user@host
```

Then open `http://127.0.0.1:8642` in your local browser.

The refine UI groups findings into eight sections: Packages, System
Configuration, Services & Scheduling, Users & Identity, Network, Storage,
Software & Files, and Secrets & Subscription. Network, Storage, and
Secrets & Subscription have nothing to decide.

Each item has a bucket and a disposition:

- The **bucket** (Baseline, Site, or Investigate) says how much attention
  the item needs. Packages and config files get one.
- The **disposition** (Actionable, Advisory, or Inventory) says whether you
  can decide it. Only Actionable items have an include/exclude toggle.
  Advisory and Inventory items are listed with an explanation and no
  toggle; refine refuses a change to one.

For this host, the 25 packages are 24 Site packages and one Investigate
package (`epel-release`, which has no repository source). All 25 are
Actionable. Of the 62 configs, 55 are Actionable and 7 are not: 2
Advisory (`/etc/init.d/legacy-app`, a SysV init script that needs a
`.service` unit in image mode, and `/etc/mydb/config.yaml`, a symlink that
crosses from `/etc` into `/var`) and 5 Inventory (two sysctl drop-ins, two
`httpd.service.d` drop-ins, and the `sshd.service` unit file, which the
export carries through the merged sysctl drop-in and the service
drop-ins). Eight of the Actionable configs
are RPM-owned files you modified; Step 5 covers them.

See [Triage Classification](../reference/triage-classification.md) for
how each bucket is assigned.

## Step 5: Make triage decisions

Work through the sections and decide what belongs in your image. These
are the decisions made on this host. Everything not mentioned stayed at
its default.

### Packages: keep what you need, drop what was incidental

- **Keep:** `httpd` and `nginx`, which define the workload, plus the
  other Site packages such as `git`, `rsync`, and `tuned`. They become
  `dnf install` lines in the Containerfile.
- **Exclude:** `strace`, `tcpdump`, and `tmux`. These are debugging tools
  from past troubleshooting and do not belong in a production image.
  Toggle each to excluded.
- **Exclude the EPEL repository, then take `htop` back.** Four packages
  come from EPEL: `bat`, `fd-find`, `htop`, and `the_silver_searcher`.
  The EPEL switch starts off, because only four of its eight packages are
  included; the other four are dependencies the list hides. Click it once
  to include all eight, then again to exclude all eight. `bat`, `fd-find`,
  `htop`, and `the_silver_searcher` move to "From excluded repos". Then
  include `htop` again, because you do want it on the host.

`epel-release` is the one Investigate item. It is excluded by default and
this tutorial leaves it that way. The EPEL `.repo` files still ship in
the image, which is how `htop` installs.

### Configs: drop what is stale

- **Exclude:** `/etc/nsswitch.conf.bak`, a stale backup file.
- **Check the RPM-modified configs.** refine includes RPM-owned files you
  modified by default, because they are Site items. The scan tarball's
  Containerfile does not carry them. On this host that is eight files:
  `chrony.conf`, `crontab`, `httpd/conf/httpd.conf`, `krb5.conf`,
  `nginx/nginx.conf`, `rhsm/rhsm.conf`, `security/faillock.conf`, and
  `ssh/sshd_config`, which carries the changed `Port 2222`. This
  tutorial keeps them. Exclude any you do not want in the image.
- **Nothing to decide:** `/etc/init.d/legacy-app` (Advisory) and
  `/etc/sysctl.d/thp.conf` (Inventory) have no toggle. The advisory is
  work you do outside refine: write a systemd unit for the init script.

After these decisions the stats bar shows 18 packages included and 7
excluded, and `Configs: 54 included / 1 excluded / 7 advisory`. The 7
counts every config you cannot toggle: the 2 Advisory and the 5 Inventory.

The goal is not to migrate everything. The goal is to migrate what
defines this workload and leave behind what was incidental to the
host's lifetime.

## Step 6: Export refined results

Use the **Export** button in the toolbar, then **Export** again in the
dialog. The browser downloads a tarball that reflects your decisions.
Nothing in the scan tarball from Step 1 changes: your decisions live only
in this export.

The browser saves the export as `inspectah-refine-output.tar.gz` in its
download directory (often `~/Downloads`) on the machine that runs the
browser. Through the port forward above, that is your workstation, not
the host. Copy the file to the host where you will build, for example
with `scp`.

The export is a different tree from the scan tarball. It has no
`README.md`, `audit-report.html`, `secrets-review.md`, or
`kickstart-suggestion.ks`. It adds a `compose/` directory and a
`schema/` directory, and it unpacks into a single directory named
`inspectah-<hostname>-refined/`.

When you are done, stop the refine server with Ctrl-C in the terminal.

## Step 7: Build the image

Build the export, not the scan tarball. The scan tarball from Step 1
still describes the host as it was found, so building it would ignore
every decision from Step 5. From the directory that holds `inspectah-refine-output.tar.gz`:

```bash
inspectah build inspectah-refine-output.tar.gz --tag rhel9-refined:v1
```

This extracts the export and runs `podman build` for you. The build ran
as a regular user. Each Containerfile step prints as it runs; this
excerpt keeps the first step, the last step, and the summary:

```
STEP 1/29: FROM registry.redhat.io/rhel9/rhel-bootc:9.8
...
STEP 29/29: RUN bootc container lint
...
Checks passed: 11
Checks skipped: 1
Warnings: 2
...
✓ Build succeeded: rhel9-refined:v1
  Digest: sha256:847a9fc951113875767df1f2d9f8e040f40755bde869fc6e1a140552b491643d
```

The build finished with 29 of 29 steps. `bootc container lint` passed 11
checks, skipped 1, and raised 2 warnings, which do not fail the build.

### Check the result

Run `rpm -q` inside the image for the packages you excluded and the ones
you kept:

```bash
podman run --rm localhost/rhel9-refined:v1 rpm -q strace tmux tcpdump htop bat httpd
```

```
package strace is not installed
package tmux is not installed
package tcpdump is not installed
htop-3.3.0-1.el9.aarch64
package bat is not installed
httpd-2.4.62-13.el9_8.9.aarch64
```

`rpm -q` exits with a non-zero status when any package is missing, so
expect one here. `strace`, `tmux`, `tcpdump`, and `bat` are gone, `htop`
is back, and `httpd` is installed. The package decisions from Step 5
reached the image.

The config exclusion is different. `/etc/nsswitch.conf.bak` is not in the
export's `config/` tree, so your copy did not travel. But the base image
itself ships a file at that path, so you will still find one in the built
image. The exclusion kept the host's copy out; it cannot remove the
base image's.

### Read the Containerfile

To see what the build used, extract the export and read its
Containerfile:

```bash
tar xzf inspectah-refine-output.tar.gz
cat inspectah-rhel9-refined/Containerfile
```

The Containerfile is long, so this excerpt keeps the base image, the
package list, the service steps, and the config copy. Each `...` stands for
omitted lines; the lines shown are unchanged.

```dockerfile
FROM registry.redhat.io/rhel9/rhel-bootc:9.8
...
# === Packages (18) ===
RUN dnf install -y \
    at \
    git \
    htop \
    httpd \
    info \
    langpacks-en \
    man-pages \
    nginx \
    nmap-ncat \
    nodejs \
    python3-pip \
    rsync \
    sssd \
    tree \
    tuned \
    unzip \
    vim-enhanced \
    wget \
    && dnf clean all \
    && rm -rf \
        /var/cache/dnf \
        /var/lib/dnf/history* \
        /var/log/dnf* \
        /var/log/hawkey.log \
        /var/log/rhsm
...
# === Service Enablement ===
RUN systemctl enable \
    httpd.service \
    NetworkManager-wait-online.service \
    nginx.service \
    sshd.service
RUN systemctl mask \
    bluetooth.service \
    kdump.service
...
# === Configuration Files ===
# 54 config file(s) captured

COPY config/etc/ /etc/
```

Compare it with the Containerfile inside the scan tarball from Step 1:

- Packages: the scan tarball's Containerfile installs 24 packages. The
  export installs 18: the 24, less the six that dropped (`strace`,
  `tcpdump`, `tmux`, `bat`, `fd-find`, and `the_silver_searcher`).
- Configs: the scan tarball's Containerfile captures 47 config files. The
  export captures 54: the 47, less `/etc/nsswitch.conf.bak`, plus the
  eight RPM-modified files described in Step 5.
- Repositories: the scan tarball's Containerfile installs `epel-release`
  as a repo-enabling package. refine excludes it by default, so the export
  does not install it, and the EPEL repo files still ship.
- Services: the export also enables `sshd.service`. refine adds an enable
  for a unit file that fully shadows the vendor unit; you did not decide
  it.

The rest of the Containerfile covers the Python virtual environments, the npm application, the three
Quadlet units, the Compose file as a reference, the sysctl drop-in, the
tuned profile, and the SELinux file context and port rules.

You can also build manually with `podman build`. See
[How to Build a bootc Image](../how-to/build-bootc-image.md) for full
details on RHEL entitlement handling, cross-architecture builds, and
pushing to a registry.

## What you accomplished

You started with a running host and ended with a bootc container image
that captures only what matters: the packages, configs, and services that
define your workload. Everything the base image already provides was
subtracted automatically. The incidental packages you excluded in refine
are not in the image, because you built the export rather than the scan
tarball.

## What's next

- **Refine further**: review the
  [How to Review and Refine](../how-to/review-and-refine.md) guide for
  advanced triage techniques.
- **Roll up multiple hosts**: if you have multiple hosts to migrate, scan
  them all and aggregate the results to find common patterns. See
  [How to Aggregate Multiple Hosts](../how-to/aggregation.md).
- **Subtract a baseline**: learn how baseline subtraction works and
  how to use custom base images. See
  [How to Subtract a Baseline](../how-to/baseline-subtraction.md).
- **Explore the CLI**: see all available commands and flags in the
  [CLI Reference](../reference/cli.md).
