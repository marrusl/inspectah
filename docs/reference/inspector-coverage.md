---
title: Inspector Coverage
parent: Reference
nav_order: 5
---

# Inspector Coverage

Inspectors are the data-collection modules that populate snapshot sections.
Each inspector implements the `Inspector` trait and runs during the scan phase.
There are twelve: eleven always run, and the Subscription inspector runs only
when you pass `--preserve subscription` (or `--preserve all`).

**Source:** `crates/collect/src/inspectors/`
{: .text-grey-dk-000 }

## Execution model

Inspectors run in two waves:

1. **Wave 1** runs the RPM inspector and the Subscription inspector (when enabled) in parallel. The RPM inspector's output (installed packages, file ownership, verification results) is extracted into an `RpmState` struct.
2. **Wave 2** runs the other ten inspectors in parallel. They receive the `RpmState` as read-only context to classify files as RPM-owned or unowned.

If the RPM inspector fails entirely, the Wave 2 inspectors that require RPM state return `InspectorError::Failed`: Config, ScheduledTasks, Selinux, and NonRpm. The rest still run, and the scan is marked incomplete.

After both waves, one cross-inspector pass marks a file that two sections both carry. The section that writes the file to the export keeps the decision, and the other section's entry becomes Inventory. See [Unit shadows](#unit-shadows).

The modernization advisory catalog (`inspectors/modernization.rs`) is not an inspector. The Config inspector calls it. See [Advisory catalogs](#advisory-catalogs).

## Inspector reference

### RpmInspector

| | |
|:---|:---|
| **ID** | `Rpm` |
| **Applies to** | `PackageBased`, `RpmOstree` and `Bootc` systems |
| **Snapshot field** | `rpm` (`RpmSection`) |
| **Source** | `inspectors/rpm/` (multi-file module) |

**What it reads:**

| Data source | Purpose |
|:------------|:--------|
| `rpm -qa --queryformat` | Installed package list (NEVRA) |
| `rpm -qa --queryformat '@@%{NAME}\n[%{FILENAMES}\n]'` | Per-package file ownership, grouped by package name (sentinel format) |
| `rpm -Va` | Package verification results (modified files) |
| `dnf repoquery --userinstalled` | Leaf package classification |
| Batched `rpm -q` requires, recommends and supplements, and an `rpm -qa` provides index | Dependency graph for leaf/auto classification. File requirements resolve through the file ownership query above. Falls back to `dnf repoquery --requires --resolve --recursive --installed` when either rpm query fails |
| `dnf repoquery --installed` (`%{from_repo}`) | Source repository of each package; falls back to `rpm -qi` |
| `dnf group list --installed`, `dnf group info` | Installed package groups |
| `/etc/yum.repos.d/` | Repository configuration files |
| `gpgkey=` paths in those repo files | Imported GPG keys (a file is kept only if one of its lines begins with a PGP public key header; a file that also holds a PGP private key block is refused) |
| `/etc/dnf/modules.d/*.module` | Enabled module streams |
| `/etc/yum/pluginconf.d/versionlock.list`, `/etc/dnf/plugins/versionlock.list` | Version lock entries |
| `dnf repolist --enabled` | Enabled repository list (for repo-less detection) |
| `dnf repoquery --available` | Availability check for packages recorded against an installer pseudo-repository |
| `find /var/cache/dnf -name *.rpm` | Cached RPM files for repo-less packages |

**Shared ownership query.** The Storage and NonRpm inspectors need the full set of RPM-owned paths, not the per-package grouping. They run `rpm --query --all --qf '[%{FILENAMES}\n]'`, which prints one path per line and nothing else, so a path containing a space stays whole. A path recorded under `/lib`, `/lib64`, `/bin` or `/sbin` is also recorded in its `/usr` form so ownership matches on a usrmerged host. The RPM inspector itself still uses the sentinel format above for its grouped result.

**Section fields:** `packages_added`, `base_image_only`, `rpm_va`, `repo_files`, `gpg_keys`, `version_changes`, `leaf_packages`, `auto_packages`, `leaf_dep_tree`, `module_streams`, `version_locks`, `installed_groups`, `file_ownership`.

**Repo-less detection** (`inspectors/rpm/repoless.rs`): After the main RPM inventory, packages whose `source_repo` is empty or names a disabled/removed repository are flagged as repo-less. A package that provides repository configuration the export carries, and that an enabled repository serves by name, is repo-enabling instead and leaves the repo-less set. For each remaining repo-less package, the inspector scans `/var/cache/dnf/` for cached `.rpm` files matching the package NEVRA. Matching uses case-insensitive substring comparison between install-time short names (e.g., `AppStream`) and full repo IDs (e.g., `rhel-9-for-aarch64-appstream-rpms`). Cached RPMs are bundled in the tarball under `repoless-packages/`; uncached RPMs are annotated as `MANUAL` for user provision via the refine UI.

---

### ConfigInspector

| | |
|:---|:---|
| **ID** | `Config` |
| **Applies to** | `PackageBased` systems |
| **Snapshot field** | `config` (`ConfigSection`) |
| **Source** | `inspectors/config/` (multi-file module) |

**What it reads:**

| Data source | Purpose |
|:------------|:--------|
| `rpm -Va` results (from RpmState) | RPM-owned files under `/etc` with modified content |
| `/etc/` directory walk | Unowned configuration files |
| `dnf history list --reverse` | Packages removed since install, to find orphaned config files left behind in `/etc` |
| `/etc/crypto-policies/config` | Active crypto policy |
| `/etc/NetworkManager/system-connections/` | Connection files with `method=auto` (DHCP) are left out of the file list |
| Path classification rules | Category assignment (network, security, auth, etc.) |
| Modernization catalog | Advisory entries for legacy patterns (see [Advisory catalogs](#advisory-catalogs)) |

**Which `/etc` files are Configuration Files entries.** This inspector owns every file under `/etc` that the export carries as configuration, including the ones other inspectors report on. PAM files (`/etc/pam.d/`), audit rules (`/etc/audit/rules.d/`), custom tuned profiles (`/etc/tuned/`), kernel module and modprobe files (`/etc/modules-load.d/`, `/etc/modprobe.d/`), dracut configuration (`/etc/dracut.conf.d/`) and sysctl files (`/etc/sysctl.conf`, `/etc/sysctl.d/`) are Configuration Files entries with a category. The SELinux and Kernel/Boot inspectors do not decide them. The Kernel/Boot inspector reads the sysctl files to explain them, but each `/etc` sysctl file is carried, or not, by its Configuration Files row alone. Loaded kernel modules (`lsmod`) are inventory in the Kernel/Boot section, not a decision.

**Skipped files.** Metadata-only changes to an RPM-owned file are skipped, as are system-generated files that are not RPM-owned (machine identity, `/etc/resolv.conf`, host keys, SELinux policy store files, and similar), and dev artifacts (`.git`, `node_modules`, and similar directories). A file whose content cannot be captured, because it is over 256 KiB, cannot be read, or is not text, is kept as a `content_not_captured` advisory with no content, so no export copies it into the image. An oversized or permission-denied file also marks the section degraded.

**Ostree and bootc.** The code has a branch that diffs `/usr/etc` against `/etc`, but the inspector declares itself applicable to `PackageBased` systems only, so a scan does not reach it.

---

### UnitsInspector

| | |
|:---|:---|
| **ID** | `Units` |
| **Applies to** | `PackageBased` systems |
| **Snapshot field** | `units` (`UnitSection`) |
| **Source** | `inspectors/units/` (`mod.rs`; `resolve.rs` for the unit file each name loads; `install.rs` for its `[Install]` section; `links.rs` for the links and what they mean; `also.rs` for `Also=` members) |

**Unit kinds.** Services, sockets, paths, timers, targets and slices carry their state. A mount, automount or swap unit carries its state when a package owns its unit file under `/usr/lib/systemd/system/`; one the administrator wrote is a mount finding (below) and never gets a `systemctl` line. Device and scope units are not recorded, though drop-ins for them are. A template carries only a mask. A template instance enabled by a link under `/etc/systemd/system/`, an instance with a unit file of its own, and a masked instance are recorded with their state and carried like any unit; a masked instance is carried as `systemctl mask`. A disabled instance backed only by its template has no link left, so the scan finds it from the presets (the template's `DefaultInstance=` under a bare `enable` rule, or each instance a rule lists) and from the `Also=` of enabled units, and records it as disabled. An instance at its preset has no state change and no line. An instance's `enable` is carried only when every configuration file it reads under `/etc` is in the export as the host had it (below). An instance of a template inspectah has no table entry for, whose unit file names nothing it reads, is carried on its unit file alone, with a note on its `refine` row.

**What it reads:**

| Data source | Purpose |
|:------------|:--------|
| `systemctl list-unit-files --no-pager` | Installed units of every kind. Its `enabled` and `disabled` are not used: the state is read from the links. Only when no preset file can be read does the degraded unit inventory record them |
| `/usr/lib/systemd/system-preset/`, `/etc/systemd/system-preset/` (`*.preset`) | Preset rules, to tell an operator change from a vendor default |
| `/etc/systemd/system/<unit>` and `/usr/lib/systemd/system/<unit>` | The unit file the name loads: local, a full shadow of a vendor file, the vendor file, or a mask |
| `[Install]` in the unit file and in the `.conf` drop-ins of `<unit>.d/` and, for an instance, its template's `.d/`, under both directories | The links `systemctl enable` would make, and the units its `Also=` names |
| Links in `*.wants/`, `*.requires/` and `*.upholds/`, and aliases, under `/etc/systemd/system/` | The links the source host has, which decide whether a unit is enabled |
| Links under `/usr/lib/systemd/system/` | The links packages ship, which complete an install and never start one |
| `/etc/systemd/system/default.target` | The default target |
| The rest of systemd's unit search path (`/usr/local/lib/systemd/system/`, `/run/systemd/system/`, `/etc/systemd/system.control/` and others) | Only checked: a unit file or `[Install]` drop-in there that changes what loads refuses the unit with a warning |
| `rpm -qf --queryformat` on `/usr/lib/systemd/system/<unit>` | Owning package of each vendor unit file |
| `/etc/systemd/system/<unit>.d/*.conf` | Drop-in overrides, for a unit of any kind |
| `/etc/systemd/system/<unit>` against `/usr/lib/systemd/system/` | Full unit shadows |
| `Where=` and `Options=` in an administrator's mount, automount or swap unit file | The mount point and whether the unit is `nofail`, for its mount finding |
| The paths holding `%i` or `%I` in an enabled template instance's unit file and drop-ins (any key outside `[Install]`, and `StateDirectory=` under `/var/lib`), and inspectah's table for the templates whose unit file does not name them (`wg-quick@.service`, `openvpn-client@.service`, `openvpn-server@.service`, `dirsrv@.service`) | The configuration under `/etc` the instance reads (`required_config`), and the data under `/var` it keeps, recorded when the path exists on the host (`data_paths`) |

**Enablement is read from the links.** `systemctl list-unit-files` says `enabled` whenever it finds a link to a unit, whoever made it. The inspector reads the unit's effective `[Install]` section instead, as `systemctl enable` reads it (an empty `WantedBy=`, `RequiredBy=` or `Alias=` in a drop-in clears the values before it; type-wide `service.d/` and dash-prefix `a-.service.d/` drop-ins are not read for it), and compares the links it names with the links on the host. A unit is enabled when every link its section names is under `/etc/systemd/system/` or shipped by a package, and at least one is under `/etc`; it is disabled when none is under `/etc`. A link a package ships completes an install and never starts one. A unit with some of its links and not others, or whose `[Install]` names no link, has no state of its own and gets no state change.

**What becomes a state change.** The inspector records operator intent only. A masked unit is always recorded, whichever form the mask takes: a link to `/dev/null` or a zero-length file. A vendor unit, a full shadow, or a package-owned storage unit is recorded when its state differs from its preset; with no matching preset rule there is no evidence of a change, and nothing is recorded. A preset line with a verb the inspector does not read stops the lookup for the units it matches and gives a warning naming the line. A local unit, whose file is only under `/etc/systemd/system/`, is recorded when it is enabled, whatever the presets say, because the image does not have it unless the export carries it; a disabled local unit is not recorded. A unit file a package owns under `/etc/systemd/system/` counts as local, with no owner. Timers follow the same rules as every other kind: a local timer that is enabled, or a vendor timer whose state differs from its preset, is a state change, and the unit row is the timer's one state control. Units at their preset state produce no state change, with two exceptions for a packaged unit enabled under an `enable` preset. When its full shadow or `/etc` drop-ins only add links the package's unit does not make, it is an enabled state change, since the image's package install linked it from the package's `[Install]` and the `enable` after the `COPY` adds the rest. When one of its `Also=` members is not enabled on the host, it is an enabled state change, so that the export can say what enabling it would also do (see Links and `[Install]` below). Every unit with an install state, or a mask, is recorded in the section's `inventory` with its state and preset.

Alias and generated units are dropped silently, and so are static and indirect units, unless a link under `/etc/systemd/system/` names one or it has a file or drop-in directory of its own name there. Runtime-only or invalid states produce a warning. `remote-cryptsetup.target` disabled against its `enable` preset is left alone, since a stock RHEL 9 install leaves it that way. A unit whose file or `[Install]` drop-in sits elsewhere on the search path where it changes what loads (under `/usr/local/lib/systemd/system/` or `/run`, for example), an alias, and a link to a file elsewhere each get a warning naming the path and no state change, whatever the unit's state. A unit file that is not valid UTF-8 keeps its unit, and its `[Install]` section is one the scan cannot rely on (below).

**Units the export never switches off.** A `disable` or `mask` of `bootc-*`, `ostree-*` or `systemd-remount-fs.service`, and a `mask` of `tmp.mount`, are not carried: the Containerfile writes a `# FIXME:` saying why, and refine shows the same reason on the row. An `enable` of any of them is carried. The bootc base image mounts `/tmp` from `tmp.mount`, and without it `/tmp` is read-only.

**Units that update packages in place.** `dnf-automatic*.timer` and `packagekit-offline-update.service` are recorded with their state and excluded by default, since an image-based host is updated by a new image. Include one in `refine` to carry it anyway.

#### Links and `[Install]`

The inspector reports what the links and `[Install]` sections say that a `systemctl` line cannot carry. Each finding is an advisory in the unit state section, comment lines only, and a row with no toggle in `refine`'s single-host Services list:

| Finding | What the advisory gives |
|:--------|:------------------------|
| A link under `/etc/systemd/system/` the unit's `[Install]` does not make: one made by hand, one for a unit with no `[Install]` links, one of a unit with only some of its links, an `.upholds` link, or an alias | Why it is not carried, and the `RUN systemctl add-wants` or `add-requires` line that recreates a `.wants` or `.requires` link, or how to add an `.upholds` link or an alias by hand (for an alias: make the link after the configuration `COPY`, or add `Alias=` in a drop-in and enable the unit after that `COPY`) |
| A link to an instance of `systemd-cryptsetup@.service` or `systemd-fsck@.service`, which names a volume or device of the source host | Why it is not carried, with no command |
| A unit whose `[Install]` holds an `UpheldBy=`, a specifier the scan does not expand (any but `%i`, `%p` and `%N`), or a name it does not admit, or whose unit file or `[Install]` drop-in cannot be read as text | Each of its links with the reason; and, when it has an `[Install]` file under `/etc` or assigns `Also=`, one block saying the image's links can differ from the source host's. The unit gets no `enable` or `disable` then |
| A preset-enabled packaged unit whose `/etc` files drop or replace a link the package's unit makes, or change its `Also=` | One advisory per difference: the `add-wants` or `add-requires` line for a link only the source host has, a quoted `RUN test -L '<path>' && rm -- '<path>'` line for a link only the package install makes (or, when the export copies a file to that path, a line saying that file replaces the link), and no command for an `Also=` difference |

While the export leaves out one of a unit's `[Install]` files, that unit's advisories become one `# Omitted:` line naming the files that are not in the export. A link a package ships under `/usr/lib/systemd/system/` is never reported, and a link whose parent or own name is not a unit name gets a warning.

**`Also=` members.** `systemctl enable` and `disable` also act on every unit a unit's `[Install]` names in `Also=`, and on the units those name. Each state change records those members and what the links say about each. An `enable` or `disable` of a unit with members is carried only when every member's links match what the command does, a member with a row of its own is carried with the same verb, and neither the unit nor a member has an `[Install]` file under `/etc/systemd/system/`, whether or not the export copies it. Otherwise the command is an advisory naming the members that keep it from being carried and, where it can, the lines to add by hand. In a fleet aggregate such a command is always an advisory.

#### The default target

When `/etc/systemd/system/default.target` links to a target with a unit file the scan reads, the export sets it with `RUN systemctl set-default`, as the first command of the unit state section. It is not compared with a preset. It is left out with an `# Omitted:` line when the target is local or a replacement whose file the export excludes, or when the target's package is not in the image. In `refine` it has its own toggle. A link to anything else gives a warning and nothing is set. A fleet aggregate keeps the target most hosts set, included only when every host sets it.

#### What the export does with each state change

The Containerfile writes the commands in a `# === Systemd Unit State ===` section after the configuration `COPY`, so each command runs after the files it names are in place. The default target comes first, then each verb is one `RUN` (`enable`, then `disable`, then `mask`), with every unit single-quoted after `--`:

```dockerfile
# === Systemd Unit State ===
RUN systemctl set-default -- 'multi-user.target'
RUN systemctl enable -- \
    'httpd.service' \
    'cockpit.socket'
RUN systemctl mask -- \
    'kdump.service'
```

A command the export cannot reproduce is left out with a `# Omitted:` line naming the unit and the reason, and `refine` shows the same reason on the unit row:

| Condition | Reason given |
|:----------|:-------------|
| A local unit whose unit file is excluded | its unit file is not in the export |
| A shadowed unit whose shadow is excluded, or a vendor unit with a unit file staged at its name | the `[Install]` section in the image would not be the one recorded on the source host |
| An `enable` or `disable` whose `[Install]` drop-in is excluded, or with an `[Install]` drop-in the export would copy that the scan did not record | the `[Install]` section in the image would not be the one recorded on the source host |
| A template instance's `enable` whose configuration file is excluded, or is not in the export | its configuration is excluded from the export, or is not in the export |
| A template instance's `enable` whose configuration file was not captured | its configuration was not captured |
| A template instance's `enable` whose configuration file holds a value the redaction pass replaced | its configuration holds a redacted value; provision the secret on the deployed host and enable the unit there |
| On a fleet aggregate, a template instance's `enable` whose configuration file has more than one content variant across hosts | its configuration differs between hosts in the fleet, so the staged copy may not be the one this instance reads |
| A template instance's `enable` when a setting of its unit file or drop-ins names a path for it in a form inspectah cannot read as systemd does, or a drop-in directory or drop-in it reads could not be read | the setting and the file and line it is on, or the file that could not be read, never the setting's value, and that the `enable` is not carried |
| A package-owned mount, automount or swap unit with an administrator's file staged at its name | the export carries an administrator's unit file for it |
| A `mask` where a unit file is staged at the same name (`systemctl mask` fails on a regular file) | the export also carries a unit file at that path |
| A vendor or package-owned storage unit whose package is excluded, not carried, or not in the target image | the package |

A shadowed unit's command depends on its shadow, not on its package; the rules in the table above, and the rule for units the image needs, still apply to it. A mask follows its package like an `enable`: the mask of a vendor unit whose package is excluded, not carried, or not in the target image is left out with the package reason. The rule for units the image needs and the staged-file rule are checked first. A mask with no unit file, or of a unit no package owns, is carried. On a fleet aggregate, a local unit or shadow whose file has more than one content variant across hosts is left out too: the row may have been recorded from another host's file.

A template instance whose `enable` is carried and that keeps data under `/var` gets one `# Advisory:` line before the `RUN` lines, since the image carries the unit's configuration and not its data. `refine` shows the same sentence on the unit row:

```dockerfile
# Advisory: dirsrv@example.service keeps data under /var/lib/dirsrv/slapd-example. The image does not carry it, and the carried enable may start the unit on first boot. Put the data in place before that boot, or the unit starts without it.
```

A carried instance's `refine` row also notes each configuration file the export stages unredacted, when the scan ran with `--no-redaction`, and each that holds a value the redaction pass flagged and did not change. These notes are not Containerfile lines.

#### Mount findings

A mount, automount or swap unit the administrator wrote is recorded in `mount_findings`: a unit file only under `/etc/systemd/system/`, a full copy over a vendor file, a vendor file no package owns, or a mask with no unit file anywhere. Its unit file stays a Configuration Files entry, carried on that row's own decision. The Containerfile writes an advisory for each finding in the unit state section, comment lines only, never an instruction:

```dockerfile
# Advisory: srv.mount is enabled on the source host. inspectah does not enable, disable or mask a mount, automount or swap unit the administrator wrote.
#   Its unit file is copied into the image. No enable line is written, but the unit still starts if another unit requires it or names its mount point in RequiresMountsFor=, and it replaces a unit of the same name that the image ships.
#   /srv is a symlink into /var in the RHEL bootc base image. The unit must be renamed for the resolved path (for /srv: var-srv.mount with Where=/var/srv).
```

The lines that follow the first depend on the finding: whether its unit file is staged; a mount point under `/usr`, one bootc and ostree own (`/`, `/boot`, `/boot/efi`, `/sysroot`, `/var`), one under `/home`, `/mnt`, `/root` or `/srv` (links into `/var` in the base image), or one under `/opt` (a directory of the read-only image); a staged `tmp.mount`; a mount or swap whose unit file was read and has no `nofail`, and with no drop-in, which could set it (a unit file that is not text or could not be read gets no `nofail` line); and, for an enabled or masked unit, the line to add by hand. A masked finding does not mount, so it gets no mount point or `nofail` line. The line to add by hand is left out where the advisory rules it out: for an enabled unit the advisory says to rename, as in the example, for a masked unit whose unit file is staged at its name (`systemctl mask` fails on a regular file there), and for a masked `tmp.mount`. `Where=` and `Options=` are read by logical line, as systemd reads them, so a line that continues another setting is part of that setting. The mount source (`What=`) is not recorded.

#### Unit shadows

A unit file under `/etc/systemd/system/` can change a unit two ways, and the export treats them differently.

| Kind | Detected by | What it means |
|:-----|:------------|:--------------|
| Drop-in | A `.conf` file under `/etc/systemd/system/<unit>.d/` | An override merged on top of the vendor unit. Base image updates to the vendor unit still apply. |
| Full shadow | A non-empty `<unit>` file directly under `/etc/systemd/system/` whose vendor unit exists in `/usr/lib/systemd/system/` | The administrator's file replaces the vendor unit. Base image updates to that unit are silently ignored. |

Both are Service Drop-ins entries. Drop-ins are collected for a unit of any kind, mount, automount, swap and timer units included. Full shadows are collected for every kind except storage units: a full copy of a mount, automount or swap unit is a mount finding. A timer's full shadow is a Service Drop-ins entry like any other. A full shadow is found whether or not the unit also has a state change, so a unit enabled at its preset (for example `sshd.service`) is still reported. A mask is not a shadow: it is a masked state, carried only as `systemctl mask`.

A full shadow that has no state change gets no unit row, in the scan or in `refine`. Its decision is its Service Drop-ins entry, which in `refine` is a row in the Systemd units section and carries the shadow badge and explanation and the include toggle for the file. Excluding that row leaves the file out of the export; the export writes no `systemctl` line for the unit.

**One decision per file.** The same path can appear in Configuration Files (from the `/etc` walk) and in Service Drop-ins. The Service Drop-ins entry owns the decision; in single-host `refine` it is the row in the Systemd units section. The Configuration Files entry for that path is marked Inventory with the note "controlled in Service Drop-ins", and `refine` refuses to toggle it. This avoids two checkboxes over one file, where one could be on and the other off. Sysctl files need no such handling: the Kernel/Boot inspector records them for its explanation and offers no decision over them.

**Drop-ins with no unit-state row.** A unit with no state change, such as one at its preset state, has no parent unit row. In `refine`, its drop-ins appear in a separate group headed "Other drop-in overrides", with the note "These overrides have no unit-state decision row." Each drop-in row there is the decision for its file.

---

### NetworkInspector

| | |
|:---|:---|
| **ID** | `Network` |
| **Applies to** | `PackageBased` systems |
| **Snapshot field** | `network` (`NetworkSection`) |
| **Source** | `inspectors/network.rs` |

**What it reads:**

| Data source | Purpose |
|:------------|:--------|
| `/etc/NetworkManager/system-connections/` | NetworkManager connection profiles |
| `/etc/sysconfig/network-scripts/` | Legacy ifcfg connection files |
| `/etc/firewalld/zones/`, `/etc/firewalld/direct.xml` | Firewall zones and direct rules |
| `/etc/sysconfig/network-scripts/route-*`, `/etc/iproute2/` | Static route files and iproute2 configuration |
| `ip route`, `ip rule` | Active routes and policy rules |
| `/etc/hosts` | Custom host entries (non-localhost lines) |
| `/etc/environment`, `/etc/profile.d/`, `/etc/dnf/dnf.conf`, `/etc/yum.conf` | Proxy settings |
| `/etc/resolv.conf` | DNS provenance (systemd-resolved, NetworkManager, or hand-edited) |

**ifcfg banner.** When any connection came from `/etc/sysconfig/network-scripts/`, the `refine` Network section shows an info banner at the top: the source host uses ifcfg network scripts, RHEL 9 and later targets use NetworkManager keyfiles by default, ifcfg is deprecated in RHEL 9 and removed in RHEL 10, and network configuration should be planned separately. The banner is informational. ifcfg files are not in the modernization catalog and carry no advisory.

---

### StorageInspector

| | |
|:---|:---|
| **ID** | `Storage` |
| **Applies to** | `PackageBased` systems |
| **Snapshot field** | `storage` (`StorageSection`) |
| **Source** | `inspectors/storage.rs` |

**What it reads:**

| Data source | Purpose |
|:------------|:--------|
| `/etc/fstab` | Persistent mount definitions (required: the inspector fails without it) |
| `findmnt --json` | Active mount points (the section degrades without it) |
| `lvs --reportformat json` | LVM volume configuration (optional) |
| Mount options | Credential reference detection (`credentials=`, inline `password=`) |
| `find /var/lib /var/log /var/cache` | Non-empty directories to depth 2 (see below) |
| `stat -c '%04a %u %g %U %G'` | Mode and ownership of each discovered directory |
| `/etc/tmpfiles.d/`, `/run/tmpfiles.d/`, `/usr/lib/tmpfiles.d/` | Directory-creating tmpfiles entries |
| `*.service`, `*.socket`, `*.mount`, `*.swap` under `/usr/lib/systemd/system/` and `/etc/systemd/system/` | `StateDirectory=`, `CacheDirectory=`, `LogsDirectory=` |
| RPM-owned path set, `rpm --query --file` | Package ownership of a directory |

#### `/var` directory backing

On a package-mode host, `/var` directories exist because something created them. On an image-based host, they exist only if the image or the first boot creates them. The Storage inspector finds the `/var` directories that matter and records what creates each one, so the export knows which to provision.

**Discovery.** It lists directories under `/var/lib`, `/var/log` and `/var/cache` to depth 2 (`-mindepth 1 -maxdepth 2`), skipping empty ones. When a directory and one of its children are both found, only the deepest one is kept, so `/var/lib/pgsql/data` is recorded and `/var/lib/pgsql` is not.

**Backing rules.** For each directory, the first rule that matches decides the backing:

1. **tmpfiles.d.** An entry whose type creates a directory (`d`, `D`, `v`, `q`, `Q`) at exactly this path. Fragments are read in systemd precedence order (`/etc`, then `/run`, then `/usr/lib`; a basename in an earlier directory masks the same basename in a later one) and only `.conf` files count. A parent's entry does not back its children, and entries with a `%` specifier match nothing.
2. **Unit directive.** A unit file that declares the path with `StateDirectory=` (under `/var/lib`), `CacheDirectory=` (under `/var/cache`) or `LogsDirectory=` (under `/var/log`). Every declaring file is recorded.
3. **RPM ownership.** A package owns the directory.
4. **Unbacked.** None of the above. These are grouped into one Unbacked `/var` directory advisory.

**Outcome at export.** The scan records what created each directory on the source host. The export then asks whether that creator reaches the image, and the `/var` summary in `refine` reports one of three outcomes per directory:

| Outcome | Meaning |
|:--------|:--------|
| `backed: creator carried by this export` | The creator (a package, a unit, or a tmpfiles.d fragment the export carries) will make the directory on the image. The export adds nothing. |
| `unbacked: provisioned by this export` | Nothing the image carries creates the directory, so the export does. This includes a directory whose source-host creator the export drops, such as a fragment under `/run`. |
| `backing unconfirmed` | The export can neither place the creator nor rule it out, for example when no baseline is known or no package owns the declaring fragment or unit. The export writes a FIXME and a commented line instead of guessing. |

The summary row reads `all backed by tmpfiles.d, unit directives, or package ownership` when every directory is backed, and otherwise counts `unbacked` and `unconfirmed` directories separately.

**Provisioning.** The export writes `config/usr/lib/tmpfiles.d/inspectah-var.conf` with one `d` entry (path, mode, owner, group) per unbacked directory, and the Containerfile copies it to `/usr/lib/tmpfiles.d/`. A directory whose mode the scan could not read gets a `RUN mkdir -p` line in the Containerfile instead. A backing-unconfirmed directory gets a FIXME comment and a commented-out `d` line in both places.

---

### ScheduledTasksInspector

| | |
|:---|:---|
| **ID** | `ScheduledTasks` |
| **Applies to** | `PackageBased` systems |
| **Snapshot field** | `scheduled_tasks` (`ScheduledTaskSection`) |
| **Source** | `inspectors/scheduled.rs` |

**What it reads:**

| Data source | Purpose |
|:------------|:--------|
| `/etc/cron.d/`, `/etc/crontab` | System cron entries |
| `/etc/cron.hourly/`, `/etc/cron.daily/`, `/etc/cron.weekly/`, `/etc/cron.monthly/` | Periodic cron scripts |
| `/var/spool/cron/` | Per-user crontabs |
| `*.timer` files in `/etc/systemd/system/` and `/usr/lib/systemd/system/` | Existing systemd timers (local and vendor), with the paired `.service` file's `ExecStart` |
| `/var/spool/at/` | Pending at jobs |

**Extra output:** Generates systemd timer unit equivalents for discovered cron entries (stored as `GeneratedTimerUnit`).

**Timers are described here and controlled elsewhere.** An existing timer's row records its schedule (`OnCalendar=`), its command (`ExecStart=`), its description and where its file is, and has no include toggle. Its state (enabled, disabled or masked) is a row in the Systemd units section, and a local timer's file is a Configuration Files entry, or a Service Drop-ins entry when it is a copy of a vendor timer. A generated cron timer is the exception: its row is its one control, and the export copies and enables it in the Scheduled Tasks section. Excluding the row in refine leaves its timer, its service and its enable out, and excluding the cron job it came from does the same for every unit generated from that job. When a unit collected from the host has the name of a generated file, a unit with a row or one listed in the unit inventory, the generated files are not written and the Containerfile says so in a `# FIXME:` naming that unit. At jobs are listed in a `# FIXME:` and never carried, so their rows take no operation.

Scans cron, at and timer command content for secret-like patterns (`password`, `secret`, `token`, `key`, `credential`) and emits redaction hints.

---

### ContainersInspector

| | |
|:---|:---|
| **ID** | `Containers` |
| **Applies to** | `PackageBased` systems |
| **Snapshot field** | `containers` (`ContainerSection`) |
| **Source** | `inspectors/containers.rs` |

**What it reads:**

| Data source | Purpose |
|:------------|:--------|
| `/etc/containers/systemd/`, `/usr/share/containers/systemd/`, `/etc/systemd/system/` | Quadlet unit files (`.container`, `.volume`, `.network`, `.kube`, `.pod`, `.image`, `.build`) |
| `~/.config/containers/systemd/` for each user in `/etc/passwd` with UID 1000 to 59999 | User-level Quadlet files |
| `docker-compose*.yml`, `docker-compose*.yaml`, `compose*.yml`, `compose*.yaml` under `/opt`, `/srv`, `/etc` | Compose file discovery |
| `podman ps --format json`, `podman inspect` | Running container inventory |
| `flatpak list --app --system`, `flatpak remote-list --system` | Installed Flatpak applications and their remotes |

Parses `Image=` directives from `.container` Quadlet files to identify container image references.

**Compose detection:** Discovered compose files are stored in `compose_files` and bundled in the tarball under `compose/`. In the Containerfile, compose stacks are rendered as a reference-only comment block (no COPY or RUN) with a pointer to the bundled files and guidance on Quadlet migration.

---

### NonRpmInspector

| | |
|:---|:---|
| **ID** | `NonRpm` |
| **Applies to** | `PackageBased` systems |
| **Snapshot fields** | `non_rpm_software` (`NonRpmSoftwareSection`), `unmanaged_files` (`UnmanagedFileSection`) |
| **Source** | `inspectors/nonrpm.rs` |

**Scan roots:** `/opt`, `/srv`, `/usr/local`, `/var/www`. Two flags add to them. `--scan-home all|USER,...` adds the home directories of users with UID 1000 through 59999 (`all`) or of the named users, never a home of `/`, and `--scan-path PATH` (repeatable) adds an absolute path that exists. A flag path already under a default root, or under another added root, is not added again. Directories matching `node_modules`, `.git`, `.cache`, `.npm`, `__pycache__`, `.tox`, `.venv`, `.eggs`, `target`, `build`, `dist` and `vendor` are pruned during recursive walks.

**What it reads:**

| Data source | Purpose |
|:------------|:--------|
| `pyvenv.cfg` file walk | Python virtual environment discovery, including whether the venv sets `include-system-site-packages = true` |
| `pip list --path <site-packages> --format=json` | Per-venv pip package inventory |
| `.dist-info/` directory scan | Fallback venv package detection when `pip list` fails, and system-level pip packages under `/usr/lib/python3*/site-packages`, `/usr/lib64/python3*/site-packages` and `/usr/local/lib/python3*/site-packages` |
| `find <site-packages> -name '*.so'` | C extension detection |
| `rpm -qf <path>` | RPM ownership filtering for system pip packages, npm globals and system gems |
| `package-lock.json` file walk | npm project discovery (lockfile version 1, 2 and 3 parsing) |
| `package.json` | npm manifest fallback when no lockfile present |
| `npm root -g`, `npm list -g --json`, `<prefix>/<pkg>/package.json` | npm global packages (see below) |
| `Gemfile.lock` file walk | Ruby gem project discovery (specs section parsing) |
| `Gemfile` | Ruby project manifest collection |
| `gem environment gemdir`, `gem list --local --no-details` | System-installed gem detection |
| `.env`, `.env.local` and similar files under the scan roots | Environment variable files (redaction-sensitive) |
| `.git/config` under the scan roots | Git remote URLs |
| `readelf` and `file` on unpackaged binaries | ELF binary detection |

**Language package detection methods:**

| Method | Ecosystem | Confidence | Description |
|:-------|:----------|:-----------|:------------|
| `python venv` | pip | High or Medium | `pyvenv.cfg` found. High when a `requirements.txt` is collected and RPM state is available, otherwise Medium. Packages come from `pip list`, or from `.dist-info/` directories if that fails. |
| `pip dist-info` | pip | Medium | System-level site-packages. RPM-owned packages are filtered out by `rpm -qf` on the dist-info path. Low when RPM state is unavailable. |
| `npm lockfile` | npm | High | `package-lock.json` found; dependencies parsed from lockfile |
| `npm manifest` | npm | Low | `package.json` found but no lockfile; dependency names extracted, no pinned versions |
| `npm global` | npm | High or Medium | Global packages. High when `npm list -g --json` supplied the list for the prefix `npm root -g` reported, otherwise Medium. |
| `gem lockfile` | gem | High | `Gemfile.lock` found; gems parsed from specs section |
| `gem system` | gem | Medium or Low | System gems via `gem list --local --no-details`. Medium when RPM-owned gems were filtered out, otherwise Low. |

Binaries are reported as `readelf (<language>)` (High) or `file scan` (Low), and git repositories as `git repo`.

**npm global packages.** The inspector asks `npm root -g` for the global prefix and also checks `/usr/lib/node_modules` and `/usr/local/lib/node_modules`. For the prefix `npm` reports, it merges `npm list -g --json` with a directory walk of `<prefix>/<pkg>/package.json` (scoped `@scope/pkg` packages included), preferring the `npm list` versions. Other prefixes use the directory walk only. A package whose install path an RPM owns is dropped. Each prefix with remaining packages becomes one `npm global` item.

**Environment badges.** The `refine` language package list marks an environment with `C extensions` when a `.so` file exists in its site-packages, and with `system site-packages` when the venv's `pyvenv.cfg` enables system site-packages, meaning it can import packages the RPM layer provides.

**Confidence rendering:** High-confidence items render as active Containerfile directives. Medium-confidence items render as commented-out directives. Low-confidence items render as advisory comments only.

**Unmanaged file scanning** (`scan_unmanaged_files()`): `--include-unmanaged` catalogs and bundles unmanaged files from `/opt`, `/srv` and `/usr/local`. Files there that RPM does not own, and that no Tier 1 language package environment claims, are cataloged with provenance signals:

| Signal | Source | Description |
|:-------|:-------|:------------|
| Size, last modified, permissions, UID / GID | `stat -c '%s %Y %u %g %a'` | File size, modification time, permission bits, owning user and group IDs |
| Writable mount | `/proc/mounts` | Whether the file resides on a mount with the `rw` option (longest matching mount point wins) |
| Mutable | `stat -c %Z /etc/machine-id` (fallback: install time of the `filesystem` package) | Whether the file changed after the system was installed |
| Service working dir | `grep -rh WorkingDirectory=` over `/etc/systemd/system` and `/usr/lib/systemd/system` | Whether the path is under a service's `WorkingDirectory` |

Symlinks are detected without following and preserved as tar symlink entries. In the Containerfile, symlinks render as `RUN ln -sf` directives rather than COPY.

---

### KernelbootInspector

| | |
|:---|:---|
| **ID** | `KernelBoot` |
| **Applies to** | `PackageBased` systems |
| **Snapshot field** | `kernel_boot` (`KernelBootSection`) |
| **Source** | `inspectors/kernelboot.rs` |

**What it reads:**

| Data source | Purpose |
|:------------|:--------|
| `/proc/cmdline` | Kernel command line (scanned for secret-like parameters) |
| `lsmod` | Currently loaded kernel modules (inventory) |
| `/etc/sysctl.d/`, `/run/sysctl.d/`, `/usr/local/lib/sysctl.d/`, `/usr/lib/sysctl.d/`, `sysctl -a` | The sysctl search path and the running value of every key the files set or exclude. See [Sysctl files](#sysctl-files). |
| `/etc/locale.conf` | Locale |
| `timedatectl show --property=Timezone` | Timezone configuration |
| `tuned-adm active` | Active tuned profile |

This inspector does not read `/etc/modprobe.d/`, `/etc/modules-load.d/`, `/etc/dracut.conf.d/` or `/etc/tuned/`. Those files are Configuration Files entries (see [ConfigInspector](#configinspector)).

#### Sysctl files

The inspector records what the sysctl search path held and what the kernel reports, and resolves the files the way systemd-sysctl does. It decides nothing: sysctl files reach the image only as Configuration Files entries, copied as the host had them.

- **What it reads.** Every `*.conf` entry in the four directories, highest priority first. A file in a higher-priority directory replaces a file of the same name in a lower one. A link to `/dev/null` is a mask and is not read. A link whose final target is under `/etc/` or one of the four directories is read through, and its target is recorded (the link `/etc/sysctl.d/99-sysctl.conf` records `/etc/sysctl.conf`). A link to anywhere else is recorded and not read.
- **How lines are resolved.** Lines follow `sysctl.d(5)`: a leading `-` on an assignment ignores a failure to set the key, and a line `-<key>` with no `=` excludes the key, so a later exclusion cancels an earlier assignment. `net.ipv4.ip_forward` and `net/ipv4/ip_forward` are one key. A glob is reported as written and not expanded.
- **What it records.** The files and their content, and the running value of every key a file assigns or excludes, from `sysctl -a`. Multi-value settings are compared token by token, so a tab in the kernel's output and a space in a file do not count as a difference.
- **When the explanation is unavailable.** A directory that cannot be listed, an entry that cannot be read (a dangling link included, or content over 256 KiB), or a failed `sysctl -a` leaves no sysctl section and marks the inspector degraded with the reason. The files are still carried as Configuration Files.
- **When values are not stated.** A surviving link to a file outside `/etc/` and the sysctl directories, a file whose content redaction changed, or a running value redaction removed means inspectah does not know every input. The reports then name that file or key and state no values or drift for the host.

The audit report, the HTML report and the System Tuning section in `refine` explain the result: values and their source lines, runtime drift, vendor files an `/etc` file replaces, files that are not carried, and lines reported as written.

---

### SelinuxInspector

| | |
|:---|:---|
| **ID** | `Selinux` |
| **Applies to** | `PackageBased` systems |
| **Snapshot field** | `selinux` (`SelinuxSection`) |
| **Source** | `inspectors/selinux.rs` |

**What it reads:**

| Data source | Purpose |
|:------------|:--------|
| `getenforce`, then `/sys/fs/selinux/enforce` | SELinux mode (enforcing/permissive/disabled) |
| `/etc/selinux/config` | Policy type (`SELINUXTYPE`, default `targeted`) |
| `/etc/selinux/<type>/active/modules/400` | Custom SELinux policy modules (installed locally) |
| `semanage boolean -l`, then `/sys/fs/selinux/booleans/` | Booleans whose current value differs from the default |
| `/etc/selinux/<type>/contexts/files/file_contexts.local` and `file_contexts.subs` | Custom file context rules, in the order the policy resolves them |
| `semanage fcontext -l -C` | File context rules, only when neither file above is readable |
| `semanage port -l -C` | Custom port label assignments |
| `/proc/sys/crypto/fips_enabled` | FIPS mode status |

The `semanage` commands run through `chroot` into the host root.

PAM files and audit rules are not read here. They are Configuration Files entries (see [ConfigInspector](#configinspector)).

#### File context replay

The Containerfile replays the custom file context rules you include as `semanage fcontext` commands, in the order the source host held them, so the image's own policy store carries them.

- **Rule order.** `file_contexts.local` and `file_contexts.subs` list rules in insertion order, which is also the order the policy resolves overlapping local expressions. `semanage fcontext -l -C` prints the same table sorted by expression, which can change which rule wins, so it is the fallback only.
- **Install step.** `semanage` is not in the bootc base, so the block first runs `dnf install -y policycoreutils-python-utils` and cleans the dnf cache in the same layer.
- **Replay.** Each rule removes its own key from the local store if present, then adds it, or modifies it when the target policy defines the key. A rule whose path the target already maps as an equivalence is skipped with a FIXME in the build output, because deleting its key would delete that equivalence. A rule that cannot be rendered safely becomes a `FIXME` comment instead of a command.
- **`/var/srv` equivalence.** `/srv` is a symlink to `/var/srv` on the base image and the policy has no `/var/srv` alias, so when a rule needs it the block adds a `/var/srv` equivalence for `/srv`. The Containerfile comment says it was synthesized.
- **No `restorecon`.** The block emits no `restorecon`. Labels for image content are computed when bootc pulls the image, from the image's own policy, so a rule only needs to be in the policy store. Data you restore into `/var` after deployment (`/var/srv` included) keeps the labels it arrived with, so run `restorecon -R` on it before starting the services that use it.

---

### UsersGroupsInspector

| | |
|:---|:---|
| **ID** | `UsersGroups` |
| **Applies to** | `PackageBased` systems |
| **Snapshot field** | `users_groups` (`UserGroupSection`) |
| **Source** | `inspectors/users.rs` |

**What it reads:**

| Data source | Purpose |
|:------------|:--------|
| `/etc/passwd` | Non-system users (UID 1000 to 59999) |
| `/etc/shadow` | Password aging, hash status |
| `/etc/group` | Non-system groups (GID 1000 to 59999) |
| `/etc/gshadow` | Group shadow entries |
| `/etc/sudoers`, `/etc/sudoers.d/` | Sudoers rules |
| `~/.ssh/authorized_keys` | SSH key references (redacted by default) |
| `/etc/subuid`, `/etc/subgid` | Subordinate UID/GID mappings |

---

### SubscriptionInspector

| | |
|:---|:---|
| **ID** | `Subscription` |
| **Applies to** | `PackageBased` systems |
| **Snapshot field** | `subscription` (`SubscriptionSection`) |
| **Source** | `inspectors/subscription.rs` |
| **Activation** | Only runs when `--preserve subscription` is passed |

**What it reads:**

| Data source | Purpose |
|:------------|:--------|
| `/etc/pki/entitlement/*.pem` | Entitlement certificate and key pairs (serial-number matched) |
| `/etc/rhsm/ca/` | CA certificates (e.g., `redhat-uep.pem`) |
| `/etc/rhsm/rhsm.conf` | Subscription manager configuration |
| `/etc/yum.repos.d/redhat.repo` | Red Hat repository definition |
| `/etc/pki/consumer/cert.pem` | Consumer cert for org metadata (org ID, system UUID, RHSM server) |

**Section fields:** `entitlement_certs`, `ca_certs`, `config_files`, `earliest_expiry`, `incomplete`, `org_id`, `system_uuid`, `rhsm_server`, `source_hostname`.

**Behaviors:**

- Cert/key files are base64-encoded in the snapshot and decoded during tarball staging.
- Certificate expiry is parsed from X.509 PEM data. Certs expiring within 14 days or already expired trigger warnings.
- Bundle completeness is evaluated against a four-component contract (entitlement pair, CA cert, rhsm.conf, redhat.repo). Missing components set `incomplete: true` with a warning naming the missing item.
- Symlinks within subscription paths are followed; symlinks resolving outside approved roots are rejected with a warning.
- Files over 1 MB are rejected.
- Permission-denied errors produce warnings (not silent skips).

---

## Advisory catalogs

Some findings are Advisory: they tell you about a pattern that needs a decision or a different approach in an image, but they do not ask you to include or exclude a file. Three catalogs produce them. Each is folded into Configuration Files by the Config inspector.

### Modernization

The catalog lives in `inspectors/modernization.rs`. It runs when the source OS major version can be read from `os-release`. Each pattern lists legacy paths and a replacement.

| Pattern | Fires on | Replacement |
|:--------|:---------|:------------|
| SysVinit script | Each file in `/etc/init.d/` with no `/usr/lib/systemd/system/<name>.service` of the same name | A systemd unit |
| xinetd configuration | Each file in `/etc/xinetd.d/` | systemd socket activation |
| anacrontab | `/etc/anacrontab`, when it holds entries beyond the stock `cron.daily`, `cron.weekly` and `cron.monthly` lines | A systemd timer |

If the path is already a Configuration Files entry, it becomes an advisory. Otherwise a new advisory entry with no content is added.

### Cross-tree symlinks

For each unowned symlink under `/etc`, the Config inspector resolves the final target. A link that crosses `/etc` to `/var` or `/etc` to `/usr` produces an advisory: a link into `/var` makes the configuration stateful through `/var` persistence, outside the `/etc` three-way merge, and a link into `/usr` points into the read-only `/usr` layer. A link that cannot be resolved produces a "Broken cross-tree symlink" advisory. These standard system links are allowlisted and produce nothing: `/etc/localtime` into `/usr/share/zoneinfo/`, `/etc/alternatives/` into `/usr/`, the CA bundle and `cert.pem` links into `/etc/pki/`, `/etc/crypto-policies/back-ends/` into `/usr/share/crypto-policies/`, and `/etc/resolv.conf` into `/run/`.

An advisory entry is never written to the export. It also keeps its advisory status when a unit drop-in would otherwise shadow the same path.

### ifcfg deprecation banner

The ifcfg banner is not an advisory entry. It is the `refine` Network section note described under [NetworkInspector](#networkinspector).

---

## Redaction engine

The redaction engine runs as a pipeline stage after collection, before
rendering. It is not an inspector but operates on inspector output.

**Source:** `crates/pipeline/src/redaction/`, with the compose scrubber in `crates/core/src/redaction.rs`
{: .text-grey-dk-000 }

### What redaction covers

| Surface | What is scanned |
|:--------|:----------------|
| Configuration Files | File content, including drop-ins, `.env` files and repo files |
| RPM | Repo file content and GPG key content |
| Services | Drop-in and full-shadow unit content |
| Storage | Mount options (inline `password=` values) and credential references |
| Kernel/Boot | The kernel command line, and the sysctl section: file content, link targets and running values. A running value with a match is removed, not scrubbed, and a changed file or a removed value leaves the host's sysctl values and drift unstated. |
| Network | Proxy URLs with embedded credentials (a dedicated masker) |
| Containers | Container environment variables, Quadlet unit content, and compose file content |
| Scheduled tasks | Cron, at and timer commands, and the generated `.service` and `.timer` unit content |
| Non-RPM software | `.env` files, git remote URLs, and language-package manifests (`requirements.txt`, `package.json`, `package-lock.json`, `Gemfile`, `Gemfile.lock`), where registry URLs can carry credentials |
| Users | Sudoers rules and shadow entries |

Compose files get two passes. The compose scrubber rewrites credentials by its own key list, in a URL of any scheme. The generic pattern pass then reports, and catches a connection string whose key name carries no secret-like word.

### Pattern types

| Pattern | What it matches | Example |
|:--------|:---------------|:--------|
| PEM private key | Full private key blocks (`-----BEGIN...-----END`), including RSA, EC, DSA and OpenSSH | Private keys |
| PEM certificate | Full certificate blocks | Certificates |
| Password hash | Modular crypt hashes (`$1$`, `$2b$`, `$5$`, `$6$`, `$y$`, and others) anywhere in content | `$6$rounds=...` |
| Credential assignment | `password`, `secret`, `token`, `api_key` and similar keys followed by `=` or `:` and a value, or as a JSON key followed by a string | `db_password=...`, `"token": "..."` |
| Connection string | PostgreSQL, MongoDB and Redis URIs and JDBC URLs with a password | `mongodb://user:pass@host/db` |
| Cloud and network keys | AWS access key IDs, WireGuard keys, Wi-Fi PSKs | `AKIA...` |

### Behaviors

- **Comment-line filtering**: lines beginning with `#` are excluded from
  pattern matching to avoid false positives in commented-out configuration.
- **False-positive filtering**: known non-sensitive values after
  `password:` or `passwd:` (NSS and PAM tokens such as `files`, `sss`,
  `required`, `pam_unix.so`) are filtered out before redaction. Files
  under `/etc/pam.d/` are exempt from inspector hints.
- **Structure preservation**: a redacted secret becomes a token of the
  form `REDACTED_<KIND>_<N>`, such as `REDACTED_PASSWORD_1`, and the key
  and delimiter before it stay. A PostgreSQL, MongoDB or Redis URI
  keeps its scheme, user and host, and only the password is replaced. A
  JDBC URL that carries a credential keeps only its scheme:
  `jdbc:mysql://REDACTED_JDBCPASSWORD_1`. A credential value is replaced
  up to the next whitespace, and a JSON string with its quotes, so a
  quoted, JSON or XML value can lose its closing quote, brace or tag. See [Known limitations](known-limitations.html).
- **Inspector hints**: inspectors emit `RedactionHint` values to flag
  content they know is sensitive (e.g., sudoers env vars matching secret
  patterns, SSH key material). The redaction engine uses these hints to
  supplement pattern-based scanning. A hint whose flagged content survives
  the pattern pass is reported as unresolved.

### Redaction state

After the pass, the snapshot carries one of these states:

| State | Meaning |
|:------|:--------|
| `FullyRedacted` | Every finding was resolved by redaction, or there were none. |
| `PartiallyRedacted` | At least one finding is unresolved, with a count and the hints. |
| `SensitiveRetained` | You kept sensitive data on purpose (`--preserve` or `--no-redaction`). |

A finding is unresolved when its confidence is below High. An unresolved scrubber finding downgrades the snapshot to `PartiallyRedacted`. For example, when a language-package manifest line matches a credential-bearing key but the scrubber cannot locate the value, the line is reported as a low-confidence finding with its line number rather than passed as clean. Review those lines by hand before sharing the export.
