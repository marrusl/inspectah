---
title: Triage Classification
parent: Reference
nav_order: 2
---

# Triage Classification

inspectah sorts findings along two independent axes.

- A **triage bucket** (Baseline, Site, or Investigate) signals how much human
  attention an item needs. Packages, configuration files, services and service
  drop-ins, Quadlet units, Flatpaks, sysctl overrides, and the tuned profile
  get a bucket. For packages and configuration files the bucket also sets the
  default inclusion in the generated Containerfile.
- A **disposition** (Actionable, Advisory, or Inventory) says whether the user
  can decide the finding at all. Every finding carries one. See
  [Finding dispositions](#finding-dispositions).

Bucket classification works differently depending on whether you scanned a
single host or merged multiple hosts into an aggregate snapshot.

{% raw %}
<div class="diagram-embed" style="margin: 2em 0;">
  <iframe id="diagram-triage-decision-tree"
          src="../diagrams/triage-decision-tree.html"
          title="Triage Decision Tree, interactive preview"
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
  <p><em>How inspectah classifies each item into Baseline, Site, or Investigate, and how aggregate prevalence layers on top. Click "Open interactive diagram" for zoom, pan, and click-to-expand detail.</em></p>
</div>
{% endraw %}

---

## Single-host classifications

When inspectah scans one host, every item receives one of three buckets:

| Bucket | Meaning | Default action |
|---|---|---|
| **Baseline** | Already present in the base image. No migration action needed. | Excluded from Containerfile |
| **Site** | User-installed, user-configured, or otherwise intentionally changed. This is your customization. | Included in Containerfile |
| **Investigate** | Unclear origin or ambiguous state. Needs human review before deciding. | Varies: packages excluded by default, config files included (flagged) |

### How each bucket is assigned

**Baseline** items matched one of these conditions:

- Package name appears in the base image manifest (`PackageBaselineMatch`)
- Package installed by Anaconda that is platform plumbing, which is also locked (`PackagePlatformPlumbing`), or installer default with no active service or modified configuration (`PackageInstallerDefault`)
- Config file content is identical to the RPM-shipped default (`ConfigDefault`, `ConfigBaselineMatch`)

Services, sysctl overrides, and tuned profiles are never classified Baseline on
a single host, because no baseline data exists for them.

**Site** items show intentional customization:

- Package was added by the user and has a known repository source (`PackageUserAdded`)
- Package version was upgraded from the base image version (`PackageVersionChanged` with upgrade direction)
- Config file was modified from the RPM default (`ConfigModified`)
- Config file is unowned: not shipped by any RPM (`ConfigUnowned`)
- Config file is orphaned: its owning RPM was removed (`ConfigOrphaned`)
- Service was changed to a non-default state (`ServiceNonDefaultState`)
- Service has a drop-in override (`ServiceDropInPresent`)
- Quadlet container unit deployed by the user (`QuadletUserDeployed`)
- Flatpak with a remote and branch, provisioned at first boot (`FlatpakProvisionedOnFirstBoot`)
- Sysctl override: every override is Site. A file-backed override (source under `/etc/sysctl.d/` or `/etc/sysctl.conf`) is `SysctlFileBackedOverride`; any other is `SysctlNoBaseline` and carries the `RuntimeOnlyObservation` annotation
- Tuned profile is non-default (`TunedNonDefaultProfile`) or custom (`TunedCustomProfile`)
- Anaconda-installed package promoted by an enabled service plus a modified config, or by a modified config in a short list of packages (`PackageInstallerPromotedService`, `PackageInstallerPromotedConfig`)
- Package from the command line whose files are all captured as config files (`PackageConfigCaptured`), which is Site but excluded by default

**Investigate** items need human judgment:

- Package has no known repository source (`PackageNoRepoSource`)
- Package was locally installed from an RPM file (`PackageLocalInstall`)
- Package version was downgraded (`PackageVersionChanged` with downgrade direction)
- Package has an empty or `@commandline` source repository (`PackageNoRepoSource`), or no baseline was available to compare against (`PackageProvenanceUnavailable`)
- Anaconda-installed package that may reflect user intent (`PackageInstallerAmbiguous`), or whose evidence was degraded (`PackageInstallerEvidenceUnavailable`)
- Service has no owning package (`ServiceUnknownOrigin`)
- Flatpak has incomplete provenance data (`FlatpakIncompleteProvenance`)
- Tuned is active but the `tuned` package is not in the package list (`TunedUnusualState`)
- Config file with an unresolved redaction hint

### Annotations

Items may carry additional annotations alongside their bucket:

| Annotation | Meaning |
|---|---|
| `SensitivePath` | Item lives in a security-sensitive directory (e.g., `/etc/ssh/`, `/etc/pki/`) |
| `FirstBootProvisioned` | Item was provisioned during first boot (e.g., Flatpak from a manifest) |
| `RequiresProjectedPackage` | Config file belongs to a package that must also be included |
| `RuntimeOnlyObservation` | Item reflects runtime state, not a persistent configuration |

Annotations do not change the bucket. They add context for review.

---

## Aggregate classifications

When you merge multiple host snapshots into an aggregate, items receive an
aggregate bucket instead. Aggregate classification layers **prevalence** (how many
hosts have this item) on top of the single-host classification. The prevalence
gate is applied to packages and configuration files: one that is not present on
every host defaults to excluded.

| Bucket | Meaning | Prevalence | Default action |
|---|---|---|---|
| **Universal** | Present on all hosts. Consensus across the aggregate. | count = total | Keeps its single-host default (the prevalence gate does not apply) |
| **Partial** | Present on some hosts, absent on others. Role-specific. | count >= half of total | Excluded from Containerfile (prevalence gate) |
| **Divergent** | Present on fewer than half the hosts. Unusual or role-specific. | count < half of total | Excluded from Containerfile (prevalence gate) |
| **Investigate** | Divergent-zone item that appears on all hosts but with unclear provenance. Needs review. | count = total, but zone is Divergent | Keeps its single-host default (the prevalence gate does not apply) |

### Prevalence zones

Aggregate classification is built on **prevalence zones**, which are computed
from the ratio of hosts that have an item to the total host count:

| Zone | Condition | Maps to aggregate bucket |
|---|---|---|
| **Consensus** | count = total (present on every host) | Universal |
| **NearConsensus** | count * 2 >= total (present on at least half) | Partial |
| **Divergent** | count * 2 < total (present on fewer than half) | Divergent |

A special case: if an item is in the Divergent zone but its prevalence count
equals the total host count, it is promoted to **Investigate** instead of
Divergent. This catches items that exist everywhere but were flagged as
divergent due to configuration differences.

### Aggregate-to-single-host mapping

For filtering and counting, aggregate buckets map back to single-host equivalents:

| Aggregate bucket | Single-host equivalent |
|---|---|
| Universal | Baseline |
| Partial | Site |
| Divergent | Investigate |
| Investigate | Investigate |

This mapping is used when the UI needs to show aggregate counts across both
single-host and aggregate views.

---

## The Partial vs. Divergent distinction

This is the most important distinction in aggregate classification. Both describe
items that vary across hosts, but they mean different things:

| | Partial | Divergent |
|---|---|---|
| **What varies** | **Presence**: the item exists on some hosts but not others | **Presence**: the item exists on fewer than half the hosts |
| **Prevalence** | At least half the aggregate has it | Fewer than half the aggregate has it |
| **Typical cause** | Role-based deployment (web servers vs. database servers) | One-off installation, test host, or legacy outlier |
| **Default action** | Excluded from Containerfile | Excluded from Containerfile |

### Examples

**Partial: httpd on web servers.**
You have 10 hosts. 6 are web servers with `httpd` installed, 4 are database
servers without it. httpd appears on 6/10 hosts (NearConsensus zone), so it
gets the **Partial** bucket. It is excluded from the Containerfile by default
(not universal), but you can manually include it if this represents a
legitimate role-based package that should be in the target image.

**Divergent: debugging tool on one host.**
You have 10 hosts. 1 host has `strace` installed for a debugging session that
was never cleaned up. strace appears on 1/10 hosts (Divergent zone), so it
gets the **Divergent** bucket. It is excluded from the Containerfile because
it is not part of the standard deployment.

**Partial: sshd_config with different AllowUsers.**
You have 10 hosts. 7 have a modified `/etc/ssh/sshd_config` with
site-specific `AllowUsers` lines. The config file appears on 7/10 hosts
(NearConsensus zone), so it gets **Partial**. In aggregate mode, the refine UI
lets you pick which variant of the file to include.

**Divergent: custom repo on a test host.**
You have 10 hosts. 2 have an EPEL repository configured. The repo config
appears on 2/10 hosts (Divergent zone), so it gets **Divergent**. It is
excluded by default but you can manually include it if you want EPEL in your
target image.

---

## Section types

Triage classification applies to these inspected sections. Item types that are
not listed have no triage bucket.

| Section | Item type | Common Baseline reasons | Common Site reasons |
|---|---|---|---|
| Packages | RPM packages | In base image manifest | User-added, version upgraded |
| Config Files | Files under `/etc/` | RPM default, baseline match | Modified, unowned, orphaned |
| Services | systemd units and drop-ins | (none, no baseline data) | Non-default state, drop-in present |
| Containers | Quadlet units, Flatpaks | (none) | User-deployed, provisioned at first boot |
| Kernel/Boot | sysctl overrides, tuned profile | (none, no baseline data) | Every sysctl override; non-default or custom tuned profile |

Loaded kernel modules, network findings, and the other reference sections
carry a disposition but no bucket. Scheduled tasks and non-RPM software have no
bucket either; their default comes from the item type (a language environment
below high confidence defaults to excluded).

---

## Finding dispositions

Every finding in the snapshot carries a disposition, a `FindingKind` in the
code. It replaced the `include: bool` flag that item types used to carry. The
disposition answers a different question from the bucket: not "how much
attention does this need" but "can the user decide it, and if not, why not".

| Disposition | In the snapshot | In the refine UI | In the export |
|---|---|---|---|
| **Actionable** | `"kind": "actionable"` with an `include` flag | The row has an include control. The user decides. | Included findings reach the Containerfile and the config tree. Excluded ones do not. |
| **Advisory** | `"kind": "advisory"` with an `advisory_type` and a `rationale` | The row shows an "Advisory" badge and the rationale beneath it. No control. The stats bar counts it as "N advisory", omitted at zero. | Never rendered into the Containerfile and its file is not copied into the image. Listed under "Advisories" in the audit report. |
| **Inventory** | `"kind": "inventory"` | The row shows an "Inventory" badge, or a badge naming the row that owns the decision. No control. | Shown for reference only. Never produces a Containerfile line. |

The field is named `disposition` on 27 types, and `tuned_disposition` on
the kernel and boot section. No type carries a bare `include` flag any
more: the flag lives inside the Actionable variant. An Advisory or Inventory finding is not toggleable. The refine session
refuses a request to change one and the server answers HTTP 422 with a JSON
body whose `error` field names the reason:

- `advisory finding is not toggleable: <path>`
- `inventory finding is not toggleable: <target>`

The same status code carries the refusal of a group or repository operation
that would change nothing (`nothing to change: <target>`).

Aggregate merging leaves Advisory and Inventory findings as they are. Only
Actionable findings have their include flag changed.

### What is an advisory

An advisory is a finding the user cannot act on through include and exclude.
It carries a rationale and nothing else. Three advisory types exist, in the
`advisory_type` field:

| Type | What triggers it |
|---|---|
| `unbacked_var_dir` | One advisory for the storage section, listing the `/var` directories that no tmpfiles.d entry, `StateDirectory=`, `CacheDirectory=`, `LogsDirectory=`, or package declares. |
| `cross_tree_symlink` | An unowned symlink under `/etc` that is broken, or that resolves under `/var` or `/usr`, unless it is a known system link (for example `/etc/localtime`, `/etc/alternatives/`, the crypto policy back-ends, `/etc/resolv.conf`). Appears as a Configuration Files row. |
| `modernization` | A legacy artifact with a modern replacement: a SysVinit script in `/etc/init.d` with no systemd unit of the same name, any file in `/etc/xinetd.d`, or custom entries in `/etc/anacrontab`. Appears as a Configuration Files row. |

Two related reports are not advisory dispositions:

- **Full unit shadows.** A unit file in `/etc/systemd/system/` that replaces the
  vendor unit in `/usr/lib/systemd/system/` is collected as a service drop-in
  with a shadow rationale: base image updates to that unit are silently
  ignored. It is detected even when the unit's state matches its preset. When a shadowed unit
  has no state divergence entry of its own and the scan recorded whether it is
  enabled or disabled, refine adds a service entry for it so the unit can be
  decided. In refine the full shadow is an Actionable row with the rationale
  attached. The audit report lists it under Advisories.
- **The ifcfg note.** When a network connection was collected from
  `/etc/sysconfig/network-scripts/`, the Network section shows a deprecation
  banner: RHEL 9 and later use NetworkManager keyfiles, and ifcfg support is
  removed in RHEL 10. It is a note on the section. The connection rows
  themselves are Inventory.

### What is inventory

Inventory findings are displayed for review and never decided. These item types
default to Inventory:

- NetworkManager connections
- Firewall zones and firewall direct rules
- Loaded kernel modules

Nothing renders a firewall zone or a loaded module into the image. The files
that configure module loading (`/etc/modules-load.d/`, `/etc/modprobe.d/`) are
ordinary Configuration Files entries.

A **Configuration Files** row can also become Inventory when another section
carries the same file. This keeps one decision per file. The row shows the
owner instead of a control:

| Badge | When | Where the decision lives |
|---|---|---|
| Controlled in Service Drop-ins | The file's path is also a Service Drop-ins entry (a `*.service.d/*.conf` drop-in, or a full unit shadow). | The Service Drop-ins row for the same path. |
| Merged into sysctl settings | The file is the `source` of one or more sysctl overrides. The included keys are written to one synthesized `99-inspectah-migrated.conf` drop-in. | The sysctl rows, key by key, under Kernel and Boot. |

The badge links to the owning row. An advisory entry is never converted this
way, so the warning is not lost.

---

## Include/exclude and the Containerfile

### Include defaults by bucket

Only Actionable findings have an include default. Advisory and Inventory
findings pass through classification unchanged. For Actionable packages and
configuration files, the bucket sets the starting state:

| Bucket | Effective include | Rationale |
|---|---|---|
| Baseline | No | Excluded by baseline subtraction |
| Site | Yes | User customization to reproduce. A package is included only if it is a leaf package, when leaf data is available. An orphaned config file is excluded. |
| Investigate | Varies by section | Packages: excluded by default. Config files: included. |
| Universal | Per single-host default | Present on every host, so the prevalence gate does not narrow it |
| Partial | No | Excluded by prevalence narrowing |
| Divergent | No | Excluded by prevalence narrowing |

Two sources of exclusion apply on top of the bucket:

1. **Baseline subtraction**: items matching the base image are excluded during
   the scan pipeline.
2. **Aggregate prevalence narrowing**: in aggregate mode, packages and config
   files not present on every host are excluded during the aggregate merge
   pass.

Snapshots written without a `disposition` key read back with a named default:
Actionable and included for most item types, Inventory for network findings and
loaded modules, and excluded for `tuned_disposition`.

### Locked items

Some items carry a `locked: true` flag that prevents toggling in the refine
UI. Locked items display their include/exclude state but the toggle is
disabled. A reason string explains why the item is locked. Reasons in use
include "host state, not image-portable" (every fstab entry), "merge-hostile"
(`/etc/fstab` and `/etc/crypttab` as config files), and services and drop-ins
that are incompatible with image mode. Platform-plumbing packages installed by
Anaconda are locked too.

Locked items are set during classification and normalization, before refine
opens the snapshot. They represent non-negotiable decisions: items where
toggling would produce an invalid or broken Containerfile.

Users can override any **unlocked** Actionable item's include/exclude state in
the refine UI. The Containerfile is regenerated from the current include
states when exported.

### What "included" means in the Containerfile

The Containerfile action depends on the item type:

| Item type | Containerfile action |
|---|---|
| RPM package | `RUN dnf install -y <names>`, followed by `dnf clean all` |
| Config file | `COPY config/<root>/ /<root>/`, one per top-level directory of the included files (for example `COPY config/etc/ /etc/`) |
| Service enable/disable/mask | `RUN systemctl enable`, `disable`, or `mask` with the unit names |
| Service drop-in or full unit shadow | `COPY drop-ins/etc/systemd/system/ /etc/systemd/system/` |
| Quadlet unit | `COPY quadlet/ /etc/containers/systemd/` |
| Sysctl override | The included keys are merged into one file: `COPY sysctl/etc/sysctl.d/99-inspectah-migrated.conf /etc/sysctl.d/` |
| Scheduled task (timer) | `COPY config/etc/systemd/system/ /etc/systemd/system/` and `RUN systemctl enable <name>.timer` |

Advisory and Inventory findings produce no Containerfile line. That includes
firewall zones, NetworkManager connections, and loaded kernel modules: the
Containerfile never runs `firewall-offline-cmd`.

---

## Section promotion

Section promotion is the mechanism by which items that would normally be
invisible (classified as Baseline, or excluded by aggregate intersection) can be
surfaced for review or inclusion.

In the refine UI, users can:
- **Include** a Baseline item to add it to the Containerfile (e.g., pinning
  a specific package version even though it matches the base image)
- **Exclude** a Site or Investigate item to remove it from the Containerfile
  (e.g., dropping a debugging package you do not want in the target image)
- **Select a variant** in aggregate mode when multiple hosts have different
  versions of the same config file

These user decisions override the automatic classification. The triage bucket
itself does not change, only the include/exclude flag is toggled. This
preserves the classification rationale while giving users full control over
the migration output.

When a section contains items that the user promoted (changed from excluded to
included), the export tarball includes the corresponding file roots so the
Containerfile's `COPY` directives have matching source files.
