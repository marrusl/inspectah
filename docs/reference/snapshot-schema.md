---
title: Snapshot Schema
parent: Reference
nav_order: 3
---

# Snapshot Schema

The `InspectionSnapshot` is the core data structure produced by `inspectah scan`.
It captures the full state of an RPM-based Linux system relevant to bootc
image-mode migration. The structure has 32 top-level fields, listed in the
tables below. A snapshot loads only in the inspectah release that wrote it:
see [Loading and schema version](#loading-and-schema-version).

**Schema version:** 29
{: .label .label-blue }

**Source:** `crates/core/src/snapshot.rs`
{: .text-grey-dk-000 }

{% raw %}
<div class="diagram-embed" style="margin: 2em 0;">
  <iframe id="diagram-data-flow-snapshot"
          src="../diagrams/data-flow.html"
          title="Data Flow, interactive preview"
          width="100%" height="450" frameborder="0"
          loading="lazy" tabindex="0"></iframe>
  <div style="margin-top: 0.5em;">
    <button id="btn-diagram-data-flow-snapshot"
            onclick="(function(btn){var iframe=document.getElementById('diagram-data-flow-snapshot');if(iframe.requestFullscreen){iframe.requestFullscreen();iframe._triggerBtn=btn;document.addEventListener('fullscreenchange',function handler(){if(!document.fullscreenElement){document.removeEventListener('fullscreenchange',handler);if(iframe._triggerBtn){iframe._triggerBtn.focus();iframe._triggerBtn=null;}}});}else{window.open(iframe.src,'_blank');}})(this)"
            aria-label="Open data flow diagram in fullscreen">
      Open interactive diagram
    </button>
  </div>
  <p><em>The data flow diagram traces snapshot data from host filesystems through inspectors, redaction, and rendering.</em></p>
</div>
{% endraw %}

## Top-level fields

| Field | Type | Description |
|:------|:-----|:------------|
| `schema_version` | `u32` | Schema version number, currently **29**. Checked on load: any other value is rejected. |
| `meta` | `Map<String, Value>` | Free-form metadata written by `scan`: `hostname`, `hostname_source`, `timestamp` (RFC 3339), and `inspectah_version`. `hostname` is the static name from `/etc/hostname`, or when that is empty the transient name from `/proc/sys/kernel/hostname`, and is omitted when neither names the host. `hostname_source` is the file that supplied it, or `none` when no name was found. |
| `os_release` | `OsRelease?` | Parsed `/etc/os-release` fields (name, version, version ID, ID, ID-like, pretty name, variant ID). |
| `system_type` | `SystemType` | Detected system kind, serialized as `package-mode`, `rpm-ostree`, or `bootc`. Default: `unknown`. |

## Inspector sections

Each section is populated by a dedicated inspector during the scan phase.
All are `Option` types: absent when the inspector did not run or was not applicable.

| Field | Type | Inspector | Description |
|:------|:-----|:----------|:------------|
| `rpm` | `RpmSection?` | `RpmInspector` | Installed packages, version changes, repo files, GPG keys, module streams, leaf/auto classification, and dependency trees: `dep_closures` maps each leaf and each dependency-installed package to the added packages installing it certainly brings, those it requires that nothing else provides, `dep_alternatives` maps it to the added packages it may bring as one of several providers of something it requires, and `leaf_dep_tree` maps each leaf to the dependency-installed ones among both. On an aggregate, both cover every package the merge kept: `dep_closures` what it certainly installs on every host that has it, `dep_alternatives` the rest of what it may install on any. |
| `config` | `ConfigSection?` | `ConfigInspector` | RPM-owned modified files (`rpm -Va`), unowned `/etc` files, orphaned configs, including PAM files, audit rules, and custom tuned profiles. Ostree systems use `/usr/etc` overlay diffing instead. |
| `units` | `UnitSection?` | `UnitsInspector` | Systemd unit state for every unit kind. `state_changes`: enabled, disabled and masked units whose state differs from what the image would have, each with the unit file it loads and its owning package, and where they apply the units its `Also=` reaches (`also`), its `[Install]` drop-ins (`install_drop_ins`, `vendor_drop_ins`) and, for an enabled template instance, the configuration under `/etc` it reads (`required_config`) and the data under `/var` it keeps (`data_paths`), or, when what it reads could not be read, why (`needs_unknown`: the setting, file and line, or the file that could not be read, never the value). `inventory`: every unit with an install state or a mask, once each, with its preset. `drop_ins`: drop-in overrides and full unit shadows. `mount_findings`: the administrator's mount, automount and swap units. `manual_links` and `undetermined_installs`: links and `[Install]` sections inspectah reports and does not carry. `default_target`: the default target, when the host sets one. `enabled_on_any_host`: on an aggregate, every unit enabled on at least one host, whatever state the merged `inventory` keeps. |
| `network` | `NetworkSection?` | `NetworkInspector` | NetworkManager connections, firewall zones/direct rules, static routes, `/etc/hosts` additions, proxy settings, DNS provenance. |
| `storage` | `StorageSection?` | `StorageInspector` | fstab entries, active mount points, LVM volumes, `/var` directories with their backing, credential references in mount options. |
| `scheduled_tasks` | `ScheduledTaskSection?` | `ScheduledTasksInspector` | Cron jobs (system + user), systemd timers, at jobs, generated timer unit conversions. |
| `containers` | `ContainerSection?` | `ContainersInspector` | Quadlet unit files, compose YAML files, running containers (via Podman), installed Flatpak applications. |
| `non_rpm_software` | `NonRpmSoftwareSection?` | `NonRpmInspector` | Python virtualenvs, pip packages, Node.js modules, `.env` files, git repositories, unpackaged binaries. |
| `unmanaged_files` | `UnmanagedFileSection?` | `scan` | Files under `/opt`, `/srv`, and `/usr/local` that no RPM owns, with size, type, and provenance signals. Present only when the scan ran with `--include-unmanaged`; omitted from the JSON otherwise. |
| `kernel_boot` | `KernelBootSection?` | `KernelbootInspector` | Kernel command line, GRUB defaults, the sysctl section (`sysctl`: the files on the sysctl search path and the running value of each key they name, recorded for explanation only), loaded kernel modules (`lsmod`), active tuned profile, locale, timezone, alternatives. Loaded modules carry their own dispositions and the sysctl section carries none; module-loading configuration and sysctl files are in `config`. `sysctl` is absent when its collection failed and on an aggregate. |
| `selinux` | `SelinuxSection?` | `SelinuxInspector` | SELinux mode, custom modules, boolean overrides, fcontext rules and where they were read from, port labels, FIPS mode. PAM files, audit rules, and custom tuned profiles are not here: they are `config` entries. |
| `users_groups` | `UserGroupSection?` | `UsersGroupsInspector` | Non-system users/groups, shadow entries, sudoers rules, SSH key references, subuid/subgid mappings. |
| `subscription` | `SubscriptionSection?` | `SubscriptionInspector` | RHEL subscription material: entitlement cert/key pairs, CA certs, rhsm.conf, redhat.repo. Includes cert expiry dates, bundle completeness, and org metadata. Only populated when `--preserve subscription` is used. |

## Quality and trust fields

| Field | Type | Description |
|:------|:-----|:------------|
| `preflight` | `PreflightResult` | Package availability check against the target image's repositories: which packages are available, unavailable, or unverifiable, and which repositories were unreachable. Always present (defaults to empty). |
| `warnings` | `Vec<Warning>` | Inspector-emitted warnings (non-fatal issues encountered during scan), each with an inspector name, a message, and an optional severity. |
| `completeness` | `Completeness` | Artifact completeness based on inspector failure state: `complete`, `partial` (lists the degraded inspectors), or `incomplete` (lists the failed and degraded inspectors). |

## Redaction fields

Sensitive data handling is built into the snapshot lifecycle.

| Field | Type | Description |
|:------|:-----|:------------|
| `redactions` | `Vec<RedactionFinding>` | Records of redacted content (what was removed and where). |
| `redaction_hints` | `Vec<RedactionHint>` | Inspector-emitted hints about content that may need redaction. Consumed by the redaction engine to supplement pattern-based scanning. |
| `redaction_state` | `RedactionState?` | Trust state for snapshot re-rendering: `fully_redacted`, `partially_redacted`, `sensitive_retained`, `unknown`, or `raw`. Only `fully_redacted` skips redaction on import. |
| `redaction_skipped` | `bool` | `true` when redaction was skipped during a scan. Set by `aggregate` from a `raw` input state; omitted when `false`. |

## Baseline fields

Baseline data connects the host snapshot to a target container image.

| Field | Type | Description |
|:------|:-----|:------------|
| `target_image` | `TargetImageIdentity?` | Canonical reference and resolution strategy for the target base image. |
| `baseline` | `BaselineData?` | Package data resolved from the target image's RPM database. Used for added/removed classification. Omitted when no baseline was resolved. |

## Sensitivity flags

| Field | Type | Description |
|:------|:-----|:------------|
| `sensitive_snapshot` | `bool` | `true` if this snapshot intentionally retains credential material. Set when `--preserve` or `--no-redaction` is used. |
| `preserved_credentials` | `bool` | `true` if password hashes were preserved by operator choice (`--preserve password-hashes`). |
| `preserved_ssh_keys` | `bool` | `true` if SSH authorized keys were preserved by operator choice (`--preserve ssh-keys`). |
| `preserved_subscription` | `bool` | `true` if RHEL subscription material was preserved by operator choice (`--preserve subscription`). |

## Aggregate fields

Present only in aggregate (multi-host) snapshots produced by `inspectah aggregate`.

| Field | Type | Description |
|:------|:-----|:------------|
| `aggregate_meta` | `AggregateSnapshotMeta?` | Aggregate-level metadata (hostnames, host count, label). `None` for single-host snapshots. |
| `rpm_repo_conflicts` | `Map<String, Vec<RepoSourceEntry>>` | Repo-source conflicts detected during aggregate merge. Maps `name.arch` identity keys to distinct repos with host counts. Empty for single-host snapshots. |

## Serialization

Snapshots serialize to JSON via `serde`. Key behaviors:

- Fields with `skip_serializing_if` annotations are omitted when empty, `None`, or `false` to keep output compact. Examples: `unmanaged_files`, `redaction_state`, `baseline`, and the four sensitivity flags.
- `serde(default)` on a field means a key missing from a snapshot of the current schema parses as that field's default. It is not a compatibility mechanism across schema versions. Findings are the one place the default is not uniform: a missing `disposition` key reads back as Actionable and included for most types, as Inventory for network findings and loaded kernel modules, and as excluded for `tuned_disposition`. See [Triage Classification](triage-classification.md#finding-dispositions).
- Schema version is checked on load, after the JSON parses. See below.

## Loading and schema version

`SCHEMA_VERSION` and `MIN_SCHEMA` in `crates/core/src/snapshot.rs` are both 29. The loading contract is an exact match, not a rolling window: a snapshot whose `schema_version` is anything other than 29 does not load, whether it is older or newer. There is no migration. A schema bump renames or retypes fields, and some of the data a new field holds is observed on the host at scan time, so it cannot be derived from an older snapshot afterward. Schema 29 adds `rpm.dep_closures`, what each added package needs. A schema 28 snapshot still parses, but without it the packages that a carried package such as `firewalld` needs read as not installed, and nothing in it says so.

In practice:

- A tarball from a different inspectah release does not load in `refine` or `aggregate` when the two releases use different schema versions, which is the usual case between releases. Re-scan the host with the release you are running.
- `refine` stops with an error. For a tarball whose snapshot is schema 28, the output is:

  ```text
  Loading snapshot...
  error: snapshot load error: unsupported schema version: 28 (accepted: 29)
  ```

- `aggregate` prints a warning for each tarball it cannot load and skips it, then refuses to merge fewer than two snapshots.
- The file is parsed before its version is compared. A snapshot from an older schema whose fields no longer parse reports a `json error` describing what failed to parse, in place of the version message. It does not load either way.
