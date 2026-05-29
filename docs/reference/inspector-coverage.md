---
title: Inspector Coverage
parent: Reference
nav_order: 5
---

# Inspector Coverage

Inspectors are the data-collection modules that populate snapshot sections.
Each inspector implements the `Inspector` trait and runs during the scan phase.

**Source:** `inspectah-collect/src/inspectors/`
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

**Section fields:** `packages_added`, `base_image_only`, `rpm_va`, `repo_files`, `gpg_keys`, `version_changes`, `leaf_packages`, `auto_packages`, `leaf_dep_tree`, `module_streams`, `version_locks`, `multiarch_packages`, `duplicate_packages`.

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

---

### NonRpmInspector

| | |
|:---|:---|
| **ID** | `NonRpm` |
| **Applies to** | `PackageBased` systems |
| **Snapshot field** | `non_rpm_software` (`NonRpmSoftwareSection`) |
| **Source** | `inspectors/nonrpm.rs` |

**What it reads:**

| Data source | Purpose |
|:------------|:--------|
| Filesystem walk | Python virtualenvs (`venv`, `.venv`, `virtualenv`) |
| `pip list --format=json` | Pip-installed packages (system + per-venv) |
| `package-lock.json` / `node_modules/` | Node.js dependencies |
| `.env` files in common locations | Environment variable files (redaction-sensitive) |
| `.git/` directories | Git repository locations |
| `readelf` on unpackaged binaries | ELF binary detection |

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
| **Activation** | Only runs when `--preserve-subscription` is passed |

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
