---
title: Snapshot Schema
parent: Reference
nav_order: 3
---

# Snapshot Schema

The `InspectionSnapshot` is the core data structure produced by `inspectah scan`.
It captures the full state of a RHEL system relevant to image-mode migration.

**Schema version:** 17
{: .label .label-blue }

**Source:** `inspectah-core/src/snapshot.rs`
{: .text-grey-dk-000 }

<div id="diagram-data-flow-snapshot">
{% include_relative ../diagrams/data-flow.html %}
</div>

## Top-level fields

| Field | Type | Description |
|:------|:-----|:------------|
| `schema_version` | `u32` | Schema version number. Current: **17**. Used for forward/backward compatibility checks. |
| `meta` | `Map<String, Value>` | Free-form metadata (hostname, scan timestamp, tool version). |
| `os_release` | `OsRelease?` | Parsed `/etc/os-release` fields (name, version, ID, variant). |
| `system_type` | `SystemType` | Detected system kind: `PackageBased` (traditional) or `OstreeBased` (image mode). Default: `PackageBased`. |

## Inspector sections

Each section is populated by a dedicated inspector during the scan phase.
All are `Option` types -- absent when the inspector did not run or was not applicable.

| Field | Type | Inspector | Description |
|:------|:-----|:----------|:------------|
| `rpm` | `RpmSection?` | `RpmInspector` | Installed packages, version changes, repo files, GPG keys, module streams, leaf/auto classification, dependency tree. |
| `config` | `ConfigSection?` | `ConfigInspector` | RPM-owned modified files (`rpm -Va`), unowned `/etc` files, orphaned configs. Ostree systems use `/usr/etc` overlay diffing instead. |
| `services` | `ServiceSection?` | `ServicesInspector` | Systemd unit files, enabled/disabled state, preset deviations, drop-in overrides. |
| `network` | `NetworkSection?` | `NetworkInspector` | NetworkManager connections, firewall zones/direct rules, static routes, `/etc/hosts` additions, proxy settings, DNS provenance. |
| `storage` | `StorageSection?` | `StorageInspector` | fstab entries, active mount points, LVM volumes, credential references in mount options. |
| `scheduled_tasks` | `ScheduledTaskSection?` | `ScheduledTasksInspector` | Cron jobs (system + user), systemd timers, at jobs, generated timer unit conversions. |
| `containers` | `ContainerSection?` | `ContainersInspector` | Quadlet unit files, compose YAML files, running containers (via Podman), installed Flatpak applications. |
| `non_rpm_software` | `NonRpmSoftwareSection?` | `NonRpmInspector` | Python virtualenvs, pip packages, Node.js modules, `.env` files, git repositories, unpackaged binaries. |
| `kernel_boot` | `KernelBootSection?` | `KernelbootInspector` | Loaded kernel modules (`lsmod`), sysctl overrides, timezone, tuned profile, boot config snippets. |
| `selinux` | `SelinuxSection?` | `SelinuxInspector` | SELinux mode, custom modules, boolean overrides, fcontext rules, port labels, audit rules, FIPS mode, PAM configs. |
| `users_groups` | `UserGroupSection?` | `UsersGroupsInspector` | Non-system users/groups, shadow entries, sudoers rules, SSH key references, subuid/subgid mappings. |

## Quality and trust fields

| Field | Type | Description |
|:------|:-----|:------------|
| `preflight` | `PreflightResult` | Pre-scan checks (root access, RPM DB availability). Always present (defaults to empty). |
| `warnings` | `Vec<Warning>` | Inspector-emitted warnings (non-fatal issues encountered during scan). |
| `completeness` | `Completeness` | Artifact completeness based on inspector failure state. Tracks which inspectors ran, degraded, or failed. |

## Redaction fields

Sensitive data handling is built into the snapshot lifecycle.

| Field | Type | Description |
|:------|:-----|:------------|
| `redactions` | `Vec<RedactionFinding>` | Records of redacted content (what was removed and where). |
| `redaction_hints` | `Vec<RedactionHint>` | Inspector-emitted hints about content that may need redaction. Consumed by the redaction engine to supplement pattern-based scanning. |
| `redaction_state` | `RedactionState?` | Trust state for snapshot re-rendering. Only `FullyRedacted` skips redaction on import. |

## Baseline fields

Baseline data connects the host snapshot to a target container image.

| Field | Type | Description |
|:------|:-----|:------------|
| `target_image` | `TargetImageIdentity?` | Canonical reference and resolution strategy for the target base image. |
| `baseline_data` | `BaselineData?` | Package data resolved from the target image's RPM database. Used for added/removed classification. |
| `no_baseline` | `bool` | `true` if baseline resolution was attempted but failed or is unavailable. Distinguishes "no baseline" from "baseline not yet attempted." |

## Sensitivity flags

| Field | Type | Description |
|:------|:-----|:------------|
| `sensitive_snapshot` | `bool` | `true` if this snapshot intentionally retains credential material. |
| `preserved_credentials` | `bool` | `true` if password hashes were preserved by operator choice. |
| `preserved_ssh_keys` | `bool` | `true` if SSH authorized keys were preserved by operator choice. |

## Fleet fields

Present only in fleet (multi-host) snapshots produced by `inspectah fleet aggregate`.

| Field | Type | Description |
|:------|:-----|:------------|
| `fleet_meta` | `FleetSnapshotMeta?` | Fleet-level metadata (hostnames, host count, label). `None` for single-host snapshots. |
| `rpm_repo_conflicts` | `Map<String, Vec<RepoSourceEntry>>` | Repo-source conflicts detected during fleet merge. Maps `name.arch` identity keys to distinct repos with host counts. Empty for single-host snapshots. |

## Serialization

Snapshots serialize to JSON via `serde`. Key behaviors:

- Fields with `skip_serializing_if` annotations are omitted when empty/default to keep output compact.
- `serde(default)` on deserialization ensures forward compatibility -- new fields added in future schema versions parse as defaults when reading older snapshots.
- Schema version is checked on load: versions newer than the current tool version are rejected with `SnapshotError::UnsupportedVersion`.
