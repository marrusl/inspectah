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
          title="Aggregate Topology — interactive preview"
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
  different hosts
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

## Initialize an aggregate manifest

The manifest is a TOML file that lists which tarballs to aggregate.
Generate one from a directory of tarballs:

```bash
inspectah aggregate init scans/
```

This creates `aggregate.toml` in your current directory. To write it elsewhere:

```bash
inspectah aggregate init --output aggregate-prod.toml scans/
```

The generated manifest looks like this:

```toml
label = "web-servers"
target_image = "quay.io/centos-bootc/centos-bootc:stream9"
sources = [
  "scans/host-a.tar.gz",
  "scans/host-b.tar.gz",
  "scans/host-c.tar.gz",
]
```

The `target_image` is auto-detected from the scan metadata and will reflect
whatever distro your hosts are running (Fedora, CentOS Stream, or RHEL).

| Field | Required | Description |
|-------|----------|-------------|
| `sources` | Yes | List of tarball paths (relative to the manifest file or absolute) |
| `label` | No | A human-readable name for this group |
| `target_image` | No | Target base image reference; auto-detected from scan data when omitted |

When hosts target different images, `aggregate init` selects the most common
image. You can edit the manifest to change this or any other field.

To regenerate a manifest after adding new tarballs:

```bash
inspectah aggregate init --overwrite ./scans/
```

## Aggregate the snapshots

Combine the tarballs into a single aggregate snapshot:

```bash
inspectah aggregate --manifest aggregate.toml
```

This produces an aggregate tarball in the current directory (named with a
timestamp, e.g., `aggregate-20250527-143022.tar.gz`).

### Direct aggregation (no manifest)

You can skip the manifest and pass tarballs directly:

```bash
inspectah aggregate scans/host-a.tar.gz scans/host-b.tar.gz
```

Or point at a directory:

```bash
inspectah aggregate scans/
```

### Output options

Write the aggregate tarball to a specific location:

```bash
inspectah aggregate --manifest aggregate.toml --output-file aggregate-prod.tar.gz
```

Or specify an output directory:

```bash
inspectah aggregate --manifest aggregate.toml --output-dir output/
```

To get JSON output instead of a tarball (useful for scripting):

```bash
inspectah aggregate --manifest aggregate.toml --json-only
```

### Override the baseline

If you want to compare against a different base image than what the hosts
were scanned with:

```bash
# Example with CentOS Stream target image
inspectah aggregate --manifest aggregate.toml --target-image quay.io/centos-bootc/centos-bootc:stream9

# Example with RHEL target image
inspectah aggregate --manifest aggregate.toml --target-image registry.redhat.io/rhel9/rhel-bootc:9.6
```

### Sensitive data acknowledgment

When any contributing scan was run with `--preserve` or `--no-redaction`,
the merged output contains sensitive material. The aggregate command refuses to
produce output unless you acknowledge this with `--ack-sensitive`:

```bash
inspectah aggregate --ack-sensitive --manifest aggregate.toml
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
inspectah aggregate --manifest aggregate.toml --verbose
```

Treat warnings (e.g., mismatched image references across hosts) as errors:

```bash
inspectah aggregate --manifest aggregate.toml --strict
```

## Refine aggregate data

Open the aggregate tarball in the refine UI to get the cross-host view:

```bash
inspectah refine aggregate-20250527-143022.tar.gz
```

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

