# Future Inspection Coverage

Gap analysis of system elements not currently captured or fully surfaced by yoinkc inspectors. Organized by priority for bootc migration correctness.

Last audited: 2026-03-17

---

## Priority 1 — High Impact

### NIC Naming Risk (from gap audit)
Physical NICs with kernel-assigned `eth*` names on multi-NIC systems. Post-`bootc switch`, NIC name assignment may change, breaking networking silently. Detection: NM connection profiles (already captured) + `/sys/class/net/*/device` symlink check for physical NICs + `eth*` pattern matching. Schema: `nic_naming_risk: bool` + `affected_nic_names` on `NetworkSection`. Reference: leapp `checkifcfg` actor. **Needs its own brainstorm + spec.**

### PAM Stack Custom Modules (from gap audit)
Non-standard PAM modules loaded in `/etc/pam.d/*` stacks. Currently only catches non-RPM-owned pam.d *files* — does not parse stacks in RPM-owned files for third-party modules (e.g., `pam_radius.so` added to stock sshd PAM config). Detection: parse all pam.d files, extract module names, diff against base image module set. Reference: Augeas `Pam` lens, insights-core `parsers/pam.py`. **Needs its own brainstorm + spec — medium-high effort.**

### sshd_config Structured Parse (from gap audit)
File is captured and COPY'd, but no directive-level parsing. No deprecated-directive detection, no `Match` block awareness. Detection: parse key-value pairs (handle `Match` blocks, `Include` directives), diff against base image defaults, flag deprecated directives. Reference: Augeas `Sshd` lens, insights-core `parsers/ssh.py`. **Needs its own spec.**

### Tainted Kernel Modules (from gap audit)
Third-party/out-of-tree kernel modules flagged in `/proc/modules` with taint flags `(OE)` etc. These won't be in a standard base image kernel. Currently `lsmod` is captured but taint flags are not parsed. Detection: read `/proc/modules` directly, flag tainted entries. Reference: convert2rhel `tainted_kmods.py`. Low effort. **Can fold into a kernel module enhancement spec.**

### Custom Systemd Units (partially detected)
Service state changes are detected by the service inspector, but there is no distinction between admin-created custom units and shipped units. New `.service`/`.timer`/`.socket` files in `/etc/systemd/system/` are caught as config files but not semantically flagged as "this is a custom unit." **Needs enhancement to service inspector.**

### Container Registry Auth
Registry authentication in `~/.config/containers/auth.json`, `/etc/containers/registries.conf`, mirror configurations. Not detected at all. Required for the bootc image to pull the right images from the right registries. **Needs its own spec.**

### RHEL Subscription State
Subscription-manager state, enabled repos, content access mode. Not detected — subscription certs are excluded for privacy. No `subscription-manager identity` or `subscription-manager list` query. Important for understanding the host's entitlement posture. **Needs its own spec.**

### Kerberos Keytabs (partially detected)
`/etc/krb5.conf` is classified as `ConfigCategory.IDENTITY` (quick wins spec). But `/etc/krb5.keytab` and service keytabs are not specifically flagged. Keytabs are host-specific credentials that can't just be copied — they need re-enrollment post-migration. Should be flagged as "requires re-enrollment" rather than "copy this file." **Low effort — add keytab-specific handling.**

### Compliance Profiles (SCAP/OpenSCAP)
If the system was hardened to a DISA STIG or CIS benchmark, that's a holistic configuration that can't be reconstructed from individual file diffs. Knowing "this system was STIG'd" is valuable context. Not detected at all. Eventually should guide the user towards `openscap-im` which runs offline. **Needs its own spec.**

---

## Priority 2 — Medium Impact

### Cloud Provider Detection (from gap audit)
Which cloud (AWS, Azure, GCP, on-prem) the source system runs on. Not currently detected at all. Detection cascade: DMI strings (`/sys/class/dmi/id/product_name`, `sys_vendor`) → cloud-specific packages (already in RPM list) → `/etc/cloud/cloud.cfg`. Schema: `cloud_provider` on `SnapshotMeta`. Improves base image auto-selection. Reference: leapp `CheckRHUI` actor (uses package-based detection), Facter `cloud` fact (uses DMI). **Needs its own spec.**

### Raw iptables/nftables Rules (from gap audit)
Systems bypassing firewalld entirely. yoinkc captures firewalld zones but not raw rules. Detection: check if firewalld is active (already known from service inspector), if not, capture `/etc/sysconfig/iptables`, `/etc/sysconfig/ip6tables`, `nft list ruleset`. Low prevalence on modern RHEL but real when present. Reference: osquery `iptables` table. **Needs its own spec.**

### firewalld.conf Coverage (from gap audit)
Main firewalld config file contains `DefaultZone`, `FirewallBackend` (iptables vs nftables), `CleanupModulesOnExit`. Probably caught by `rpm -Va` if modified, but not verified. If host has `FirewallBackend=iptables` and base image defaults to `nftables`, behavior changes silently. Reference: convert2rhel `check_firewalld_availability.py`. **Very low effort — verify existing coverage.**

### kdump.conf (from gap audit)
Kernel crash dump config. yoinkc captures GRUB cmdline (which includes `crashkernel=`) but not `/etc/kdump.conf` or kdump service state. Production RHEL servers need this for Red Hat support. Probably caught if RPM-modified. Reference: Augeas `kdump.aug`, leapp actors. **Very low effort — verify + surface in report.**

### TLS Certificate Inventory (from gap audit)
CA trust anchors in `/etc/pki/ca-trust/source/anchors/` already handled (Containerfile emits `update-ca-trust`). Gap is application-specific cert stores (`/etc/nginx/ssl/`, `/etc/httpd/ssl/`). Most caught by unowned-file scan if under `/etc/`. Certs outside `/etc/` (e.g., `/opt/app/ssl/`) would be missed. Low priority.

### Ruby Gems System-wide (from gap audit)
Non-RPM inspector scans for `Gemfile.lock` but not `gem list` or system gem paths (`/usr/local/lib/ruby/gems/`). Rare in RHEL server deployments. Reference: osquery `gem_packages` table. Low effort but low priority.

### Flatpak Detection
Flatpak applications installed system-wide or per-user. Not RPMs, not pip/npm — a separate packaging system with its own remotes and runtimes. Primarily a desktop/workstation concern, rare on RHEL servers.

**Detection (low effort):**
- Is `flatpak` installed? (RPM inspector already knows)
- Configured remotes: `/etc/flatpak/remotes.d/*.flatpakrepo` (drop-in INI files with Title, Url, GPGKey)
- System-wide apps: `flatpak list --system --columns=application`
- Per-user apps: `flatpak list --user --columns=application` (less relevant for migration)

**Containerfile output:**
- Ensure `flatpak` is in the dnf install list (renderer prerequisite, same pattern as tuned)
- COPY `/etc/flatpak/remotes.d/` for remote configs
- For system apps: `flatpak install --system -y <app-id>` per app, OR generate a first-boot oneshot systemd service with a declarative app list (Universal Blue/Bazzite pattern — more robust for large app lists)

**No pure drop-in mechanism exists** for declaring flatpak apps without pre-installing flatpak. The remote configs are drop-in style, but app installation requires the flatpak binary. The Bazzite pattern (systemd oneshot + text app list) is the de-facto standard for image-based systems. **Needs its own spec — design decision is inline install vs first-boot service.**

### NetworkManager Connections (partially detected)
Connection profiles in `/etc/NetworkManager/system-connections/` are caught as config files but not semantically labeled. Bonding, VLANs, bridges, static routes, DNS configuration are buried in raw file content. Could benefit from a `ConfigCategory.NETWORK` label and/or structured parsing. **Low effort for category label; medium for structured parse.**

### Polkit Rules (partially detected)
Authorization policies in `/etc/polkit-1/rules.d/` are caught as config files with `ConfigCategory.OTHER`. Could benefit from a dedicated category label. **Very low effort — add `ConfigCategory.POLKIT` and path rule.**

### POSIX ACLs and Extended Attributes
ACLs on config files and directories (`getfacl`). Standard permissions are captured but ACL-based access control is not. Can cause permission issues post-migration. Not detected at all.

### Systemd Resource Controls
`CPUQuota`, `MemoryMax`, `IOWeight` set via `systemctl set-property`. Create drop-in files under `/etc/systemd/system/unit.d/` — these are caught as config files but the semantic "this service is resource-limited" isn't surfaced. Not specifically detected.

### AIDE/File Integrity Monitoring
AIDE database and configuration. If the system runs file integrity monitoring, the config and baseline database are operational state that matters for the migration plan. Not detected at all. Niche.

---

## Priority 3 — Lower Impact

### Swap Configuration
Swap files/partitions, swappiness settings. Host-level concern but affects workload placement.

### Cockpit Plugins
Custom cockpit modules if cockpit is installed. Niche but relevant for managed systems.

### Corosync/Pacemaker
HA cluster configurations. Very niche but critical when present.

### File Capabilities
`getcap` on binaries. If an admin used `setcap` instead of running as root, invisible to RPM queries. Niche.

### IPC Resources
System V shared memory, semaphores, message queues (`ipcs`). Legacy apps sometimes depend on these. Niche.

### Firmware/Microcode
Custom firmware packages or microcode updates. Not relevant to the container image but relevant to the host bootc runs on.

### Mounted Loop Devices
Custom loop mounts, ISO mounts, device-mapper setups outside LVM. Niche.

### Printer/CUPS Configuration
Print queue and driver setup. Niche but operationally important when present.

---

## Already Detected (removed from backlog 2026-03-17)

The following were previously on this list but confirmed fully detected during a coverage audit:

- **Kernel Modules** — `kernel_boot.py`: `lsmod` + diff against expected modules
- **Sysctl Parameters** — `kernel_boot.py`: `/etc/sysctl.d/` parsed, `SysctlOverride` model, defaults diffing
- **Kernel Command Line** — `kernel_boot.py`: `/proc/cmdline` + `/etc/default/grub`
- **Tuned Profiles** — `kernel_boot.py`: `tuned_active` + `tuned_custom_profiles`. Containerfile rendering fixed 2026-03-17.
- **Environment Variables** — `config.py`: `ConfigCategory.ENVIRONMENT`
- **LVM/Storage Layout** — `storage.py`: `LvmVolume` model
- **Alternatives** — `kernel_boot.py`: `AlternativeEntry` model
- **DNF/Yum Repos** — `rpm.py`: `RepoFile` model, rendered in Containerfile. Module streams added by P0 spec.
- **At Jobs** — `scheduled_tasks.py`: `AtJob` model
- **Automount Maps** — `config.py`: `ConfigCategory.AUTOMOUNT`
- **Custom Library Paths** — `config.py`: `ConfigCategory.LIBRARY_PATH`
- **Locale & Timezone** — `kernel_boot.py`: `locale`, `timezone` fields
- **Journal Configuration** — `config.py`: `ConfigCategory.JOURNAL`
- **Log Rotation** — `config.py`: `ConfigCategory.LOGROTATE`
- **Auditd Rules** — `selinux.py`: `audit_rules` field + `ConfigCategory.AUDIT`
- **tmpfiles.d** — `config.py`: `ConfigCategory.TMPFILES` (classified as config files, content captured)
- **Identity/Auth (partial)** — `config.py`: `ConfigCategory.IDENTITY` for `/etc/sssd/`, `/etc/krb5.conf`, `/etc/nsswitch.conf`, `/etc/ipa/` (quick wins spec)

---

## Cleanup Tasks

- **Remove podman prerequisite detection:** bootc depends on podman, so checking for it is redundant. Remove "podman not found" error messages, auto-install logic in `run-yoinkc.sh`, and `YOINKC_EXCLUDE_PREREQS` handling. Keep the `podman` vs `docker` binary selection logic (some environments may have docker instead).

## Naming Standardization

- **"entitlements" → "subscriptions"**: Mostly done. Some historical "entitlement" references remain in the codebase. Audit and standardize remaining instances.
