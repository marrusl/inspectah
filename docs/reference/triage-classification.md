---
title: Triage Classification
parent: Reference
nav_order: 2
---

# Triage Classification

inspectah automatically classifies every discovered item (package, config file,
service, container workload, kernel parameter, etc.) into a **triage bucket**.
The bucket determines the item's default inclusion in the generated
Containerfile and signals how much human attention it needs.

Classification works differently depending on whether you scanned a single
host or merged multiple hosts into a fleet snapshot.

---

## Single-host classifications

When inspectah scans one host, every item receives one of three buckets:

| Bucket | Meaning | Default action |
|---|---|---|
| **Baseline** | Already present in the base image. No migration action needed. | Excluded from Containerfile |
| **Site** | User-installed, user-configured, or otherwise intentionally changed. This is your customization. | Included in Containerfile |
| **Investigate** | Unclear origin or ambiguous state. Needs human review before deciding. | Included in Containerfile (flagged) |

### How each bucket is assigned

**Baseline** items matched one of these conditions:

- Package name appears in the base image manifest (`PackageBaselineMatch`)
- Config file content is identical to the RPM-shipped default (`ConfigDefault`, `ConfigBaselineMatch`)
- Service state matches the base image (`ServiceBaselineMatch`)
- Kernel parameter matches the base image (`SysctlBaselineMatch`)
- Tuned profile matches the base image (`TunedBaselineMatch`)

**Site** items show intentional customization:

- Package was added by the user and has a known repository source (`PackageUserAdded`)
- Package version was upgraded from the base image version (`PackageVersionChanged` with upgrade direction)
- Config file was modified from the RPM default (`ConfigModified`)
- Config file is unowned -- not shipped by any RPM (`ConfigUnowned`)
- Config file is orphaned -- its owning RPM was removed (`ConfigOrphaned`)
- Service was changed to a non-default state (`ServiceNonDefaultState`)
- Service has a drop-in override (`ServiceDropInPresent`)
- Quadlet container unit deployed by the user (`QuadletUserDeployed`)
- Kernel parameter has a file-backed override (`SysctlFileBackedOverride`)
- Tuned profile is non-default (`TunedNonDefaultProfile`) or custom (`TunedCustomProfile`)

**Investigate** items need human judgment:

- Package has no known repository source (`PackageNoRepoSource`)
- Package was locally installed from an RPM file (`PackageLocalInstall`)
- Package provenance is unavailable -- no baseline to compare against (`PackageProvenanceUnavailable`)
- Package version was downgraded (`PackageVersionChanged` with downgrade direction)
- Service has no owning package (`ServiceUnknownOrigin`)
- Flatpak has incomplete provenance data (`FlatpakIncompleteProvenance`)
- Kernel parameter has no baseline for comparison (`SysctlNoBaseline`)
- Tuned subsystem is in an unusual state (`TunedUnusualState`)

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

## Fleet classifications

When you merge multiple host snapshots into a fleet, items receive a
**fleet bucket** instead. Fleet classification layers **prevalence** (how many
hosts have this item) on top of the single-host classification.

| Bucket | Meaning | Prevalence | Default action |
|---|---|---|---|
| **Universal** | Present on all hosts. Part of the common baseline. | count = total | Excluded from Containerfile |
| **Partial** | Present on some hosts, absent on others. Role-specific. | count >= half of total | Included in Containerfile |
| **Divergent** | Present on fewer than half the hosts. Unusual or role-specific. | count < half of total | Excluded from Containerfile |
| **Investigate** | Divergent-zone item that appears on all hosts but with unclear provenance. Needs review. | count = total, but zone is Divergent | Included in Containerfile (flagged) |

### Prevalence zones

Fleet classification is built on **prevalence zones**, which are computed
from the ratio of hosts that have an item to the total host count:

| Zone | Condition | Maps to fleet bucket |
|---|---|---|
| **Consensus** | count = total (present on every host) | Universal |
| **NearConsensus** | count * 2 >= total (present on at least half) | Partial |
| **Divergent** | count * 2 < total (present on fewer than half) | Divergent |

A special case: if an item is in the Divergent zone but its prevalence count
equals the total host count, it is promoted to **Investigate** instead of
Divergent. This catches items that exist everywhere but were flagged as
divergent due to configuration differences.

### Fleet-to-single-host mapping

For filtering and counting, fleet buckets map back to single-host equivalents:

| Fleet bucket | Single-host equivalent |
|---|---|
| Universal | Baseline |
| Partial | Site |
| Divergent | Investigate |
| Investigate | Investigate |

This mapping is used when the UI needs to show aggregate counts across both
single-host and fleet views.

---

## The Partial vs. Divergent distinction

This is the most important distinction in fleet classification. Both describe
items that vary across hosts, but they mean different things:

| | Partial | Divergent |
|---|---|---|
| **What varies** | **Presence** -- the item exists on some hosts but not others | **Presence** -- the item exists on fewer than half the hosts |
| **Prevalence** | At least half the fleet has it | Fewer than half the fleet has it |
| **Typical cause** | Role-based deployment (web servers vs. database servers) | One-off installation, test host, or legacy outlier |
| **Default action** | Included in Containerfile | Excluded from Containerfile |

### Examples

**Partial -- httpd on web servers:**
You have 10 hosts. 6 are web servers with `httpd` installed, 4 are database
servers without it. httpd appears on 6/10 hosts (NearConsensus zone), so it
gets the **Partial** bucket. It is included in the Containerfile because it
represents a legitimate role-based package.

**Divergent -- debugging tool on one host:**
You have 10 hosts. 1 host has `strace` installed for a debugging session that
was never cleaned up. strace appears on 1/10 hosts (Divergent zone), so it
gets the **Divergent** bucket. It is excluded from the Containerfile because
it is not part of the standard deployment.

**Partial -- sshd_config with different AllowUsers:**
You have 10 hosts. 7 have a modified `/etc/ssh/sshd_config` with
site-specific `AllowUsers` lines. The config file appears on 7/10 hosts
(NearConsensus zone), so it gets **Partial**. In fleet mode, the refine UI
lets you pick which variant of the file to include.

**Divergent -- custom repo on a test host:**
You have 10 hosts. 2 have an EPEL repository configured. The repo config
appears on 2/10 hosts (Divergent zone), so it gets **Divergent**. It is
excluded by default but you can manually include it if you want EPEL in your
target image.

---

## Section types

Triage classification applies across all inspected sections:

| Section | Item type | Common Baseline reasons | Common Site reasons |
|---|---|---|---|
| Packages | RPM packages | In base image manifest | User-added, version upgraded |
| Config Files | Files under `/etc/` | RPM default, baseline match | Modified, unowned, orphaned |
| Services | systemd units | Default state | Non-default state, drop-in present |
| Containers | Quadlet units, Flatpaks | (rare) Present in base image | User-deployed |
| Kernel/Boot | sysctl, loaded modules | Baseline match | File-backed override |
| Scheduled Tasks | cron jobs, systemd timers | (rare) | User-created |
| Non-RPM Software | Binaries outside RPM | (none) | Always Site |

---

## Include/exclude and the Containerfile

Every classified item has an `include` flag that determines whether it
appears in the generated Containerfile. The default value depends on the
triage bucket:

| Bucket | Default include | Rationale |
|---|---|---|
| Baseline | No | Already in the base image |
| Site | Yes | User customization to reproduce |
| Investigate | Yes | Included to be safe; user reviews and removes if unneeded |
| Universal | No | Fleet consensus -- already in the common baseline |
| Partial | Yes | Role-specific customization |
| Divergent | No | Below fleet consensus threshold |

In fleet mode, items that are not present on all hosts (`count < total`) are
excluded by default regardless of their single-host classification. This
implements **strict intersection** semantics: only items common to all hosts
are included automatically.

Users can override any item's include/exclude state in the refine UI. The
Containerfile is regenerated from the current include states when exported.

### What "included" means in the Containerfile

The Containerfile action depends on the item type:

| Item type | Containerfile action |
|---|---|
| RPM package | `RUN dnf install -y <name>` |
| Config file | `COPY <path> <path>` (from materialized config tree) |
| Service enable/disable | `RUN systemctl enable/disable <unit>` |
| Service drop-in | `COPY <drop-in-path> <drop-in-path>` |
| Quadlet unit | `COPY <unit-path> <unit-path>` |
| Sysctl override | `COPY <sysctl-conf> <sysctl-conf>` |
| Kernel module | `COPY <modules-load.d-conf> <modules-load.d-conf>` |
| Firewall zone | `RUN firewall-offline-cmd` directives |
| Scheduled task (timer) | `COPY <timer-unit> <timer-unit>` |

---

## Section promotion

Section promotion is the mechanism by which items that would normally be
invisible (classified as Baseline, or excluded by fleet intersection) can be
surfaced for review or inclusion.

In the refine UI, users can:
- **Include** a Baseline item to add it to the Containerfile (e.g., pinning
  a specific package version even though it matches the base image)
- **Exclude** a Site or Investigate item to remove it from the Containerfile
  (e.g., dropping a debugging package you do not want in the target image)
- **Select a variant** in fleet mode when multiple hosts have different
  versions of the same config file

These user decisions override the automatic classification. The triage bucket
itself does not change -- only the include/exclude flag is toggled. This
preserves the classification rationale while giving users full control over
the migration output.

When a section contains items that the user promoted (changed from excluded to
included), the export tarball includes the corresponding file roots so the
Containerfile's `COPY` directives have matching source files.
