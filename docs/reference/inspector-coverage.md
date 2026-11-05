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

After both waves, one cross-inspector pass marks a file that two sections both carry. The section that writes the file to the export keeps the decision, and the other section's entry becomes Inventory. See [Service shadows](#service-shadows).

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
| `dnf repoquery --requires --resolve --recursive --installed` | Dependency graph for leaf/auto classification; falls back to batched `rpm -q` requires and `--whatprovides` queries |
| `dnf repoquery --installed` (`%{from_repo}`) | Source repository of each package; falls back to `rpm -qi` |
| `dnf group list --installed`, `dnf group info` | Installed package groups |
| `/etc/yum.repos.d/` | Repository configuration files |
| `gpgkey=` paths in those repo files | Imported GPG keys (a file is kept only if it starts with a PGP public key header) |
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

**Which `/etc` files are Configuration Files entries.** This inspector owns every file under `/etc` that the export carries as configuration, including the ones other inspectors report on. PAM files (`/etc/pam.d/`), audit rules (`/etc/audit/rules.d/`), custom tuned profiles (`/etc/tuned/`), kernel module and modprobe files (`/etc/modules-load.d/`, `/etc/modprobe.d/`), dracut configuration (`/etc/dracut.conf.d/`) and sysctl files are Configuration Files entries with a category. The SELinux and Kernel/Boot inspectors do not read them. Loaded kernel modules (`lsmod`) are inventory in the Kernel/Boot section, not a decision.

**Skipped files.** Metadata-only changes to an RPM-owned file are skipped, as are system-generated files that are not RPM-owned (machine identity, `/etc/resolv.conf`, host keys, SELinux policy store files, and similar), and dev artifacts (`.git`, `node_modules`, and similar directories). A file over 256 KiB is kept with empty content and marks the section degraded.

**Ostree and bootc.** The code has a branch that diffs `/usr/etc` against `/etc`, but the inspector declares itself applicable to `PackageBased` systems only, so a scan does not reach it.

---

### ServicesInspector

| | |
|:---|:---|
| **ID** | `Services` |
| **Applies to** | `PackageBased` systems |
| **Snapshot field** | `services` (`ServiceSection`) |
| **Source** | `inspectors/services.rs` |

**What it reads:**

| Data source | Purpose |
|:------------|:--------|
| `systemctl list-unit-files --type=service` | Installed service units and their states |
| `/usr/lib/systemd/system-preset/`, `/etc/systemd/system-preset/` (`*.preset`) | Preset rules, to tell an operator change from a vendor default |
| `rpm -qf --queryformat` on `/usr/lib/systemd/system/<unit>` | Owning package of each unit that has a state change |
| `/etc/systemd/system/*.service.d/*.conf` | Drop-in overrides |
| `/etc/systemd/system/*.service` against `/usr/lib/systemd/system/` | Full unit shadows |

**What becomes a state change.** The inspector records operator intent only. A masked service is always recorded. An enabled or disabled service is recorded when it differs from its preset. Static, alias, indirect and generated units are dropped silently, and runtime-only or invalid states produce a warning. Template units (names containing `@`) are skipped. Units at their preset state are listed as preset-matched and produce no state change.

#### Service shadows

A unit file under `/etc/systemd/system/` can change a service two ways, and the export treats them differently.

| Kind | Detected by | What it means |
|:-----|:------------|:--------------|
| Drop-in | A `.conf` file under `/etc/systemd/system/<unit>.service.d/` | An override merged on top of the vendor unit. Base image updates to the vendor unit still apply. |
| Full shadow | A `<unit>.service` file directly under `/etc/systemd/system/` whose vendor unit exists in `/usr/lib/systemd/system/` | The administrator's file replaces the vendor unit. Base image updates to that unit are silently ignored. |

Both are Service Drop-ins entries. A full shadow is found whether or not the unit also has a state change, so a unit enabled at its preset (for example `sshd.service`) is still reported. A masked unit is a link to `/dev/null` and matches the full-shadow test as well; it carries no replacement content.

Full shadows that have no state change do not get a service row from the scan. When you open the snapshot in `refine`, the session adds a synthetic state change for each one whose enabled or disabled state is known, so the unit is a toggle target through classify, export and reload. A full shadow with no known state is not synthesized.

**One decision per file.** The same path can appear in Configuration Files (from the `/etc` walk) and in Service Drop-ins. The Service Drop-ins row owns the decision. The Configuration Files entry for that path is marked Inventory with the note "controlled in Service Drop-ins", and `refine` refuses to toggle it. This avoids two checkboxes over one file, where one could be on and the other off. A sysctl file that the Kernel/Boot inspector merges into its own output is handled the same way.

**Drop-ins for units at their preset state.** A unit at its preset state has no state change, so it has no parent service row. In `refine`, its drop-ins appear in a separate group headed "Other drop-in overrides", with the note that the unit is at its preset state, so the drop-in is the whole decision.

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

**Scan roots:** `/opt`, `/srv`, `/usr/local`, `/var/www`. Two flags add to them. `--scan-home all|USER,...` adds the home directories of users with UID 1000 or higher (`all`) or of the named users, and `--scan-path PATH` (repeatable) adds an absolute path that exists. A flag path already under a default root, or under another added root, is not added again. Directories matching `node_modules`, `.git`, `.cache`, `.npm`, `__pycache__`, `.tox`, `.venv`, `.eggs`, `target`, `build`, `dist` and `vendor` are pruned during recursive walks.

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
| `/etc/sysctl.d/`, `/usr/lib/sysctl.d/`, `sysctl -a` | Sysctl overrides: a key is reported only where the file value differs from the runtime value |
| `/etc/locale.conf` | Locale |
| `timedatectl show --property=Timezone` | Timezone configuration |
| `tuned-adm active` | Active tuned profile |

This inspector does not read `/etc/modprobe.d/`, `/etc/modules-load.d/`, `/etc/dracut.conf.d/` or `/etc/tuned/`. Those files are Configuration Files entries (see [ConfigInspector](#configinspector)).

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

An advisory entry is never written to the export. It also keeps its advisory status when a service drop-in or sysctl merge would otherwise shadow the same path.

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
| Kernel/Boot | The kernel command line |
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
| Credential assignment | `password`, `secret`, `token`, `api_key` and similar keys followed by `=` or `:` and a value | `db_password=...` |
| Connection string | PostgreSQL, MongoDB and Redis URIs and JDBC URLs with a password | `mongodb://user:pass@host/db` |
| Cloud and network keys | AWS access key IDs, WireGuard keys, Wi-Fi PSKs | `AKIA...` |

### Behaviors

- **Comment-line filtering**: lines beginning with `#` are excluded from
  pattern matching to avoid false positives in commented-out configuration.
- **False-positive filtering**: known non-sensitive values after
  `password:` or `passwd:` (NSS and PAM tokens such as `files`, `sss`,
  `required`, `pam_unix.so`) are filtered out before redaction. Files
  under `/etc/pam.d/` are exempt from inspector hints.
- **Structure preservation**: connection string redaction preserves the URL
  structure (scheme, host, path) while replacing credentials with a
  `REDACTED_TOKEN` placeholder. MongoDB connection strings preserve the
  `mongodb://` prefix and database path.
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
