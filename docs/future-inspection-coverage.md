# Future Inspection Coverage

Gap analysis of system elements not currently captured by yoinkc inspectors. Organized by priority for bootc migration correctness.

## Priority 1 — High Impact

### Kernel Modules
Custom/third-party kernel modules (NVIDIA, ZFS, custom networking). Critical because bootc's kernel comes from the base image. Detection: `lsmod` + diff against modules provided by installed kernel RPMs.

### Kernel Parameters
Sysctl settings (`/etc/sysctl.d/`, `/etc/sysctl.conf`, runtime `sysctl -a`). May be partially caught as config files but not semantically understood. A missing `net.ipv4.ip_forward = 1` breaks routing/container hosts.

### Kernel Command Line
Boot parameters in `/etc/default/grub`, `/etc/kernel/cmdline`, `/proc/cmdline`. Affect system behavior at boot.

### Custom Systemd Units
Entirely new `.service`/`.timer`/`.socket` files created by the admin (not drop-ins of existing units). May partially overlap with config file detection for `/etc/systemd/system/` but not semantically flagged.

### tmpfiles.d
Systemd-tmpfiles configurations for creating directories, files, and symlinks at boot. Custom entries in `/etc/tmpfiles.d/`. Missing these can cause services to fail on first boot.

### Tuned Profiles
Active performance tuning profile (`tuned-adm active`). Custom profiles in `/etc/tuned/`. Performance characteristics would be lost in migration without capturing these.

### Environment Variables
System-wide environment variables in `/etc/environment`, `/etc/profile.d/*.sh`. Can affect application behavior, PATH, proxy settings.

### Container Registry Auth
Registry authentication in `~/.config/containers/auth.json`, `/etc/containers/registries.conf`, mirror configurations. Required for the bootc image to pull the right images from the right registries.

### LVM/Storage Layout
Beyond fstab — the actual LV/VG/PV topology. Operators need to understand the storage layout to map it to PVCs or other bootc-compatible storage.

### NetworkManager Connections
Connection profiles in `/etc/NetworkManager/system-connections/`. Includes bonding, VLANs, bridges, static routes, DNS configuration. Currently undetected.

### Identity/Authentication Integration
SSSD, Kerberos, LDAP, FreeIPA configuration. Config files might be caught but the semantic "this system authenticates against AD" or "this system is an IPA client" isn't surfaced.

### Alternatives
`update-alternatives` selections (e.g., which `java`, which `python`, which `mta`). Symlink-based, easy to miss, can break applications expecting a specific provider.

### RHEL Subscription State
Subscription-manager state, enabled repos, content access mode, subscriptions. Note: codebase currently uses "entitlements" in some places — should be standardized to "subscriptions" everywhere.

### DNF/Yum Repository Configuration
Custom repos in `/etc/yum.repos.d/`, repo priorities, GPG keys, module streams, enabled/disabled repos. Currently caught as config files but not semantically parsed — the migration plan should explicitly handle custom repos.

### At Jobs
One-shot scheduled jobs via `atd`, stored in `/var/spool/at/`. Cron and systemd timers are captured but `at` jobs are ephemeral and easily lost in migration. Worth at least flagging pending jobs.

### Kerberos Keytabs
`/etc/krb5.keytab` and service keytabs. Host-specific credentials that can't just be copied — they need re-enrollment post-migration. Should be flagged as "requires re-enrollment" rather than "copy this file."

### Compliance Profiles (SCAP/OpenSCAP)
If the system was hardened to a DISA STIG or CIS benchmark, that's a holistic configuration that can't be reconstructed from individual file diffs. Knowing "this system was STIG'd" is valuable context. Eventually should guide the user towards `openscap-im` which runs offline.

### Auditd Rules
Security audit rules in `/etc/audit/rules.d/`. Caught as config files but not semantically flagged as "this is your audit policy." Compliance often requires these.

### AIDE/File Integrity Monitoring
AIDE database and configuration. If the system runs file integrity monitoring, the config and baseline database are operational state that matters for the migration plan.

### Automount Maps
Autofs configuration for NFS home directories or shared storage. `/etc/auto.master`, `/etc/auto.*` maps. Missing these means users lose their home directories post-migration.

### Custom Library Paths (`ld.so.conf.d`)
Custom shared library paths in `/etc/ld.so.conf.d/`. If someone added custom library paths, applications fail with "cannot open shared object file" after migration. Subtle and hard to debug.

## Priority 2 — Medium Impact

### PAM Configuration
Authentication module stack customizations in `/etc/pam.d/`. Affects login, sudo, service authentication.

### Flatpaks
Flatpak applications installed system-wide or per-user. Not RPMs, not pip/npm — a separate packaging system with its own remotes and runtimes.

### Polkit Rules
Authorization policies in `/etc/polkit-1/rules.d/`. Control which users/groups can perform privileged operations.

### Locale and Timezone
System locale (`localectl`), timezone (`timedatectl`), keyboard layout. Easy to overlook but affects application behavior.

### Journal Configuration
Journald settings: rate limiting, storage size, forwarding. Custom config in `/etc/systemd/journald.conf.d/`.

### Log Rotation
Custom logrotate configs. Caught as config files but not flagged as operationally important.

### POSIX ACLs and Extended Attributes
ACLs on config files and directories (`getfacl`). Standard permissions are captured but ACL-based access control is not. Can cause permission issues post-migration.

### Systemd Resource Controls
`CPUQuota`, `MemoryMax`, `IOWeight` set via `systemctl set-property`. Create drop-in files but the semantic "this service is resource-limited" isn't surfaced.

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

## Naming Standardization

- **"entitlements" → "subscriptions"**: Standardize terminology across the codebase. Use "subscriptions" everywhere instead of "entitlements" or "entitlement certs."
