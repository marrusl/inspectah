---
title: Inspector Coverage
parent: Reference
nav_order: 5
---

# Inspector Coverage

Inspectors are the data-collection modules that populate snapshot sections.
Each inspector implements the `Inspector` trait and runs during the scan phase.

**Source:** `crates/collect/src/inspectors/`
{: .text-grey-dk-000 }

## Execution model

Inspectors run in two waves:

1. **Wave 1 -- RPM inspector** runs first. Its output (installed packages, file ownership, verification results) is extracted into an `RpmState` struct.
2. **Wave 2 -- all other inspectors** run in parallel. They receive the `RpmState` as read-only context to classify files as RPM-owned vs. unowned.

If the RPM inspector fails entirely, Wave 2 inspectors that depend on RPM state return `InspectorError::Failed`.

## Inspector reference

### RpmInspector

| | |
|:---|:---|
| **ID** | `Rpm` |
| **Applies to** | `PackageBased` systems |
| **Snapshot field** | `rpm` (`RpmSection`) |
| **Source** | `inspectors/rpm/` (multi-file module) |

**What it reads:**

| Data source | Purpose |
|:------------|:--------|
| `rpm -qa --queryformat` | Installed package list (NEVRA) |
| `rpm -qa` (sentinel format) | File ownership mapping (`/etc` paths to owning packages) |
| `rpm -Va` | Package verification results (modified files) |
| `dnf repoquery --userinstalled` | Leaf package classification |
| `dnf repoquery --requires --resolve` | Dependency graph for leaf/auto classification |
| `/etc/yum.repos.d/` | Repository configuration files |
| RPM GPG keyring | Imported GPG keys |
| `dnf module list --enabled` | Enabled module streams |
| `dnf versionlock list` | Version lock entries |
| `dnf repolist --enabled` | Enabled repository list (for repo-less detection) |
| `find /var/cache/dnf -name *.rpm` | Cached RPM files for repo-less packages |

**Section fields:** `packages_added`, `base_image_only`, `rpm_va`, `repo_files`, `gpg_keys`, `version_changes`, `leaf_packages`, `auto_packages`, `leaf_dep_tree`, `module_streams`, `version_locks`, `multiarch_packages`, `duplicate_packages`.

**Repo-less detection** (`inspectors/rpm/repoless.rs`): After the main RPM inventory, packages whose `source_repo` is empty or names a disabled/removed repository are flagged as repo-less. For each repo-less package, the inspector scans `/var/cache/dnf/` for cached `.rpm` files matching the package NEVRA. Matching uses case-insensitive substring comparison between install-time short names (e.g., `AppStream`) and full repo IDs (e.g., `rhel-9-for-aarch64-appstream-rpms`). Cached RPMs are bundled in the tarball under `repoless-packages/`; uncached RPMs are annotated as `MANUAL` for user provision via the refine UI.

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
| `rpm -Va` results (from RpmState) | RPM-owned files with modifications |
| `/etc/` directory walk | Unowned configuration files |
| `/usr/etc` vs `/etc` overlay diff | Ostree/bootc system config changes |
| Path classification rules | Category assignment (network, security, auth, etc.) |

**Branches:** Traditional package-based systems use `rpm -Va` + `/etc` walk. Ostree/bootc systems use `/usr/etc` overlay diffing instead.

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
| `/etc/systemd/system/`, `/usr/lib/systemd/system/` | Unit file locations |
| `.preset` files | Systemd preset rules for deviation detection |
| Drop-in directories (`*.d/`) | Service override snippets |

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
| `firewall-cmd` / firewalld config | Firewall zones and direct rules |
| `/etc/sysconfig/network-scripts/route-*` | Static route files |
| `/etc/hosts` | Custom host entries |
| Proxy environment / config | HTTP/HTTPS proxy settings |
| DNS configuration | DNS provenance detection |

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
| `/etc/fstab` | Persistent mount definitions |
| `findmnt --json` | Active mount points |
| `lvs` / LVM metadata | LVM volume configuration |
| Mount options | Credential reference detection |

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
| `/etc/cron.d/`, `/etc/cron.daily/`, etc. | System cron directories |
| `/var/spool/cron/` | Per-user crontabs |
| `systemctl list-timers` | Active systemd timers |
| `/var/spool/at/` | Pending at jobs |

**Extra output:** Generates systemd timer unit equivalents for discovered cron entries (stored as `GeneratedTimerUnit`).

Scans cron/at command content for secret-like patterns (`password`, `secret`, `token`, `key`, `credential`) and emits redaction hints.

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
| `/etc/containers/systemd/`, `/usr/share/containers/systemd/` | Quadlet unit files (`.container`, `.volume`, etc.) |
| `~/.config/containers/systemd/` | User-level Quadlet files |
| `docker-compose*.yml` / `docker-compose*.yaml` | Compose file discovery |
| `podman ps --format json` | Running container inventory |
| `flatpak list` | Installed Flatpak applications |

Parses `Image=` directives from `.container` Quadlet files to identify container image references.

**Compose detection:** Discovered compose files (`docker-compose*.yml` / `docker-compose*.yaml`) are stored in `compose_files` and bundled in the tarball under `compose/`. In the Containerfile, compose stacks are rendered as a reference-only comment block (no COPY or RUN) with a pointer to the bundled files and guidance on Quadlet migration.

---

### NonRpmInspector

| | |
|:---|:---|
| **ID** | `NonRpm` |
| **Applies to** | `PackageBased` systems |
| **Snapshot fields** | `non_rpm_software` (`NonRpmSoftwareSection`), `unmanaged_files` (`UnmanagedFileSection`) |
| **Source** | `inspectors/nonrpm.rs` |

**Scan roots:** `/opt`, `/srv`, `/usr/local`. Directories matching `node_modules`, `.git`, `.cache`, `.npm`, `.bundle`, `__pycache__`, `target`, `build`, `dist` are pruned during recursive walks.

**What it reads:**

| Data source | Purpose |
|:------------|:--------|
| `pyvenv.cfg` file walk | Python virtual environment discovery |
| `pip list --path <site-packages> --format=json` | Per-venv pip package inventory |
| `.dist-info/` directory scan | Fallback venv package detection when `pip list` fails |
| `pip list --format=json` (system) | System-level pip packages |
| `rpm -qf <path>` | RPM ownership filtering for system pip packages |
| `package-lock.json` file walk | npm project discovery (lockfile version 2/3 parsing) |
| `package.json` | npm manifest fallback when no lockfile present |
| `Gemfile.lock` file walk | Ruby gem project discovery (specs section parsing) |
| `Gemfile` | Ruby project manifest collection |
| `gem list --local` | System-installed gem detection |
| `.env` files in common locations | Environment variable files (redaction-sensitive) |
| `.git/` directories | Git repository locations |
| `readelf` on unpackaged binaries | ELF binary detection |

**Language package detection methods:**

| Method | Ecosystem | Confidence | Description |
|:-------|:----------|:-----------|:------------|
| `python venv` | pip | High | `pyvenv.cfg` found + `requirements.txt` present in venv root |
| `pip dist-info` | pip | Medium | `pyvenv.cfg` found but no `requirements.txt`; packages read from `.dist-info/` directories |
| `pip list` | pip | Medium | System-level pip packages via `pip list --format=json`, RPM-owned packages filtered out via `rpm -qf` |
| `npm lockfile` | npm | High | `package-lock.json` found; dependencies parsed from lockfile |
| `npm manifest` | npm | Low | `package.json` found but no lockfile; dependency names extracted, no pinned versions |
| `gem lockfile` | gem | High | `Gemfile.lock` found; gems parsed from specs section |
| `gem system` | gem | Medium/Low | System gems via `gem list --local`, RPM-owned gems filtered out |

**Confidence rendering:** High-confidence items render as active Containerfile directives. Medium-confidence items render as commented-out directives. Low-confidence items render as advisory comments only.

**Unmanaged file scanning** (`scan_unmanaged_files()`): When `--include-unmanaged` is passed, files under `/opt`, `/srv`, `/usr/local` that are not owned by RPM or claimed by a Tier 1 language package environment are cataloged with provenance signals:

| Signal | Source | Description |
|:-------|:-------|:------------|
| Last modified | `stat` | File modification timestamp |
| Permissions | `stat` | File permission bits |
| UID / GID | `stat` | Owning user and group IDs |
| Writable mount | `findmnt` | Whether the file resides on a writable mount |
| Mutable | Filesystem check | Whether the file path is on a mutable filesystem layer |
| Service working dir | `systemctl show` | Whether the path is a service's `WorkingDirectory` |

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
| `lsmod` | Currently loaded kernel modules |
| `/etc/sysctl.d/`, `/etc/sysctl.conf` | Sysctl parameter overrides |
| `timedatectl` | Timezone configuration |
| `tuned-adm active` | Active tuned profile |
| `/etc/dracut.conf.d/`, `/etc/modprobe.d/` | Boot configuration snippets |

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
| `getenforce` / `/etc/selinux/config` | SELinux mode (enforcing/permissive/disabled) |
| `semodule -l` | Custom SELinux policy modules |
| `semanage boolean -l` | Boolean overrides from defaults |
| `semanage fcontext -l -C` | Custom file context rules |
| `semanage port -l -C` | Custom port label assignments |
| Audit rule files | Audit subsystem rules |
| `fips-mode-setup --check` | FIPS mode status |
| `/etc/pam.d/` | PAM configuration files |

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
| `/etc/passwd` | Non-system users (UID 1000--59999) |
| `/etc/shadow` | Password aging, hash status |
| `/etc/group` | Non-system groups (GID 1000--59999) |
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

## Redaction engine

The redaction engine runs as a pipeline stage after collection, before
rendering. It is not an inspector but operates on inspector output.

**Source:** `crates/pipeline/src/redaction/`
{: .text-grey-dk-000 }

### Pattern types

| Pattern | What it matches | Example |
|:--------|:---------------|:--------|
| PasswordHash | Shadow-file password hash strings | `$6$rounds=...` |
| PEM block | Full PEM certificate/key blocks (`-----BEGIN...-----END`) | Private keys, certificates |
| Connection string | Database connection URLs (PostgreSQL, MySQL, MongoDB) | `mongodb://user:pass@host/db` |
| NSS/PAM token | Sensitive tokens in NSS/PAM configuration files | `ldap_default_authtok` values |

### Behaviors

- **Comment-line filtering** -- lines beginning with `#` are excluded from
  pattern matching to avoid false positives in commented-out configuration.
- **False-positive filtering** -- known non-sensitive values (e.g., example
  hostnames, placeholder strings) are filtered out before redaction.
- **Structure preservation** -- connection string redaction preserves the URL
  structure (scheme, host, path) while replacing credentials with
  `[REDACTED]`. MongoDB connection strings preserve the
  `mongodb://` prefix and database path.
- **Inspector hints** -- inspectors emit `RedactionHint` values to flag
  content they know is sensitive (e.g., sudoers env vars matching secret
  patterns, SSH key material). The redaction engine uses these hints to
  supplement pattern-based scanning.
