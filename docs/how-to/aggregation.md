---
title: Aggregation
parent: How-To Guides
nav_order: 2
---

# Aggregation

When migrating multiple hosts, aggregation combines individual scan
snapshots into a unified cross-host view. This lets you see which packages,
configs, and services are common across your infrastructure versus unique to specific
hosts.

{% raw %}
<div class="diagram-embed" style="margin: 2em 0;">
  <iframe id="diagram-aggregate-topology"
          src="../diagrams/aggregate-topology.html"
          title="Aggregate Topology: interactive preview"
          width="100%" height="450" frameborder="0"
          loading="lazy" tabindex="0"></iframe>
  <div style="margin-top: 0.5em;">
    <button id="btn-diagram-aggregate-topology"
            onclick="(function(btn){
      var iframe = document.getElementById('diagram-aggregate-topology');
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
            aria-label="Open Aggregate Topology in fullscreen">
      Open interactive diagram
    </button>
  </div>
  <p><em>How aggregation combines individual host scans into a unified cross-host view with prevalence zones. Click "Open interactive diagram" for zoom, pan, and click-to-expand detail.</em></p>
</div>
{% endraw %}

## Prerequisites

- Two or more scan output tarballs (`.tar.gz`) from `inspectah scan` runs on
  different hosts. `aggregate` needs at least two valid inputs.
- Every tarball from the same inspectah release. A tarball from a different
  release does not load: `aggregate` warns, skips it, and merges the rest if
  at least two valid inputs remain. Re-scan the host with the current release.
- A static hostname on each host. A scan from a host with no static hostname
  records none, and `aggregate` refuses two such scans with
  `duplicate hostname: <unknown>`. Set one with `hostnamectl set-hostname`
  before you scan.
- The same OS major version on every host. `aggregate` refuses a mix, for
  example RHEL 9 and RHEL 10, with `OS major version mismatch`.
- All hosts should target the same base image for meaningful comparison

## Scan multiple hosts

Run `inspectah scan` on each host you want to include in the aggregate. Collect
the resulting tarballs in a single directory on your workstation.

```bash
# On each host
sudo inspectah scan -o /tmp/scan-output.tar.gz

# Copy tarballs to your workstation
scp host-a:/tmp/scan-output.tar.gz scans/host-a.tar.gz
scp host-b:/tmp/scan-output.tar.gz scans/host-b.tar.gz
scp host-c:/tmp/scan-output.tar.gz scans/host-c.tar.gz
```

Tarball filenames do not affect analysis. Use whatever naming convention
helps you identify hosts.

## Aggregate the snapshots

Combine the tarballs into a single aggregate snapshot. Pass tarballs directly:

```bash
inspectah aggregate scans/host-a.tar.gz scans/host-b.tar.gz
```

Or point at a directory:

```bash
inspectah aggregate scans/
```

Both forms print the same kind of summary and write the result to the
current directory:

```
$ inspectah aggregate scans/rhel9-20261104-222606.tar.gz scans/rhel9-b-20261104-223850.tar.gz
warning: stale scan dates: Scan dates range from 2026-11-05T04:26:03.552382773+00:00 to 2026-11-05T04:38:47.338375489+00:00
Aggregate: aggregate (2 hosts)
Baseline (unanimous): registry.redhat.io/rhel9/rhel-bootc:9.8
Merged: 25 packages, 62 config files, 5 services
Output: aggregate-aggregate-20261104-233953.tar.gz
```

```
$ inspectah aggregate scans/
warning: stale scan dates: Scan dates range from 2026-11-05T04:26:03.552382773+00:00 to 2026-11-05T04:38:47.338375489+00:00
Aggregate: scans (2 hosts)
Baseline (unanimous): registry.redhat.io/rhel9/rhel-bootc:9.8
Merged: 25 packages, 62 config files, 5 services
Output: aggregate-scans-20261104-234031.tar.gz
```

The file name is `aggregate-`, then a label, then a timestamp. With a
directory input the label is the directory name. With tarball arguments it is
a generic default, which is why the first name above repeats a word. The
warning line is covered under [Verbose and strict modes](#verbose-and-strict-modes).

### Output options

Write the aggregate tarball to a specific location:

```bash
inspectah aggregate --output-file aggregate-prod.tar.gz scans/
```

```
warning: stale scan dates: Scan dates range from 2026-11-05T04:26:03.552382773+00:00 to 2026-11-05T04:38:47.338375489+00:00
Aggregate: scans (2 hosts)
Baseline (unanimous): registry.redhat.io/rhel9/rhel-bootc:9.8
Merged: 25 packages, 62 config files, 5 services
Output: aggregate-prod.tar.gz
```

Or specify an output directory:

```bash
inspectah aggregate --output-dir output/ scans/
```

```
warning: stale scan dates: Scan dates range from 2026-11-05T04:26:03.552382773+00:00 to 2026-11-05T04:38:47.338375489+00:00
Aggregate: scans (2 hosts)
Baseline (unanimous): registry.redhat.io/rhel9/rhel-bootc:9.8
Merged: 25 packages, 62 config files, 5 services
Output: output/aggregate-scans-20261104-234031.tar.gz
```

To get JSON output instead of a tarball (useful for scripting), use
`--json-only`. The merged snapshot goes to stdout and the warnings stay on
stderr:

```bash
inspectah aggregate --json-only scans/
```

### Override the baseline

`--target-image` changes the baseline that the aggregate records. It does not
change what was compared:

```bash
# Example with CentOS Stream target image
inspectah aggregate --target-image quay.io/centos-bootc/centos-bootc:stream9 scans/

# Example with RHEL target image
inspectah aggregate --target-image registry.redhat.io/rhel9/rhel-bootc:9.6 scans/
```

```
warning: stale scan dates: Scan dates range from 2026-11-05T04:26:03.552382773+00:00 to 2026-11-05T04:38:47.338375489+00:00
Aggregate: scans (2 hosts)
Baseline (unanimous): quay.io/centos-bootc/centos-bootc:stream9
Merged: 25 packages, 62 config files, 5 services
Output: aggregate-scans-20261104-234032.tar.gz
```

```
warning: stale scan dates: Scan dates range from 2026-11-05T04:26:03.552382773+00:00 to 2026-11-05T04:38:47.338375489+00:00
Aggregate: scans (2 hosts)
Baseline (unanimous): registry.redhat.io/rhel9/rhel-bootc:9.6
Merged: 25 packages, 62 config files, 5 services
Output: aggregate-scans-20261104-234032.tar.gz
```

The command pulls no image and compares no packages again. The package,
config and service counts are the same as in a run without the flag, because
each scan classified its packages against the image it was scanned with. The
override replaces the recorded image reference and marks it as a command-line
override. The baseline package list that a scan carries is kept only when a
scan was run against the image that ends up selected, so an override that
none of your scans used leaves the aggregate without that list. To compare the
hosts against a different image, re-scan each one with `--base-image` (see
[Baseline Subtraction](baseline-subtraction.md)) and aggregate the new
tarballs.

### Sensitive data acknowledgment

When any contributing scan was run with `--preserve` or `--no-redaction`,
the merged output contains sensitive material. The aggregate command refuses to
produce output unless you acknowledge this with `--ack-sensitive`:

```bash
inspectah aggregate --ack-sensitive scans/
```

Without the flag, the aggregate command exits with an error listing which
sensitive data types are present and instructing you to re-run with
`--ack-sensitive`.

### Subscription merging

When multiple hosts have subscription data, the aggregate command selects the
bundle with the latest certificate expiry date. If expiry dates are
identical, the snapshot with the lexicographically first hostname wins.
Incomplete bundles (missing required components) are excluded from
selection. The winning bundle's `source_hostname` field records where it
came from.

### Verbose and strict modes

Show per-host detail during aggregation:

```bash
inspectah aggregate --verbose scans/
```

```
warning: stale scan dates: Scan dates range from 2026-11-05T04:26:03.552382773+00:00 to 2026-11-05T04:38:47.338375489+00:00
Aggregate: scans (2 hosts)
Baseline (unanimous): registry.redhat.io/rhel9/rhel-bootc:9.8
Merged: 25 packages, 62 config files, 5 services
Hosts:
  - rhel9
  - rhel9-b
Section coverage:
  config: 2/2 hosts
  containers: 2/2 hosts
  kernel_boot: 2/2 hosts
  network: 2/2 hosts
  non_rpm_software: 2/2 hosts
  rpm: 2/2 hosts
  scheduled_tasks: 2/2 hosts
  selinux: 2/2 hosts
  services: 2/2 hosts
  storage: 2/2 hosts
  users_groups: 2/2 hosts
Output: aggregate-scans-20261104-234033.tar.gz
```

Treat warnings (e.g., mismatched image references across hosts) as errors:

```bash
inspectah aggregate --strict scans/
```

```
warning: stale scan dates: Scan dates range from 2026-11-05T04:26:03.552382773+00:00 to 2026-11-05T04:38:47.338375489+00:00
error: aborting due to --strict: 1 warning(s)
```

The warning in every output above, `stale scan dates`, is raised whenever the
earliest and the latest scan timestamp differ at all. There is no tolerance,
so two scans taken at different moments, even minutes apart, trigger it. With
`--strict` that one warning is enough to exit with code 1. The flag passes
only when no warning of any kind is raised, which for the stale-date check
means every scan carries the same timestamp. The other warnings are a
baseline conflict (hosts scanned against different images), a spread of minor
versions, and a mix of system types.

## Refine aggregate data

Open the aggregate tarball in the refine UI to get the cross-host view:

```bash
inspectah refine aggregate-scans-20261104-234031.tar.gz
```

The aggregate tarball loads only in the inspectah release that wrote it, like
any other snapshot.

The aggregate view adds consensus information to each finding, showing how many
hosts share a given package, config change, or service. This helps you
prioritize what to include in your target image by focusing on items common
across the infrastructure.

For details on using the refine UI, see the
[Review and Refine Findings](review-and-refine.md) guide.

## Understand consensus

In the aggregate view, each item shows a host count indicating how many hosts
share that finding. Items present on every host represent
strong consensus candidates for your target image. Items unique to one or
two hosts may be host-specific customizations.

For details on how items are classified during triage, see the
[Triage Classification](../reference/triage-classification.md) reference.

