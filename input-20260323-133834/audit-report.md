# Audit Report

**OS:** Red Hat Enterprise Linux 9.7 (Plow)

## Executive Summary

**301** items handled automatically &nbsp;|&nbsp; **15** items with FIXME (need review) &nbsp;|&nbsp; **4** items need manual intervention

- Packages added (beyond base image): 225
- Packages in target image only: 78
- Config files captured: 47
- Containers/quadlet found: 5
- Secrets redacted: 1

## RPM / Packages

Baseline: 446 packages from `registry.redhat.io/rhel9/rhel-bootc:9.7`.

### Explicitly installed (40)

These packages appear in the Containerfile `dnf install` line.

#### @commandline (1)

- epel-release 9-10.el9.noarch

#### epel (7)

- htop 3.3.0-1.el9.aarch64
- bat 0.24.0-12.el9.aarch64
- fd-find 10.3.0-1.el9.aarch64
- the_silver_searcher 2.2.0^2020704.5a1c8d8-3.el9.aarch64
- [EXCLUDED] ripgrep 14.1.1-1.el9.aarch64
- [EXCLUDED] hyperfine 1.20.0-1.el9.aarch64
- [EXCLUDED] fzf 0.58.0-2.el9.aarch64

#### rhel-9-for-aarch64-appstream-rpms (20)

- langpacks-en 3.0-16.el9.noarch
- httpd 2.4.62-7.el9_7.3.aarch64
- git 2.47.3-1.el9_6.aarch64
- vim-enhanced 8.2.2637-23.el9_7.aarch64
- nginx 1.20.1-22.el9_6.3.aarch64
- python3-pip 21.3.1-1.el9.noarch
- wget 1.21.1-8.el9_4.aarch64
- nodejs 16.20.2-8.el9_4.aarch64
- policycoreutils-python-utils 3.6-3.el9.noarch
- tcpdump 4.99.0-9.el9.aarch64
- nmap-ncat 7.92-3.el9.aarch64
- [EXCLUDED] autoconf 2.69-41.el9.noarch
- [EXCLUDED] automake 1.16.2-8.el9.noarch
- [EXCLUDED] cmake 3.26.5-3.el9_7.aarch64
- [EXCLUDED] gdb 16.3-2.el9.aarch64
- [EXCLUDED] valgrind 3.25.1-3.el9.aarch64
- [EXCLUDED] gcc 11.5.0-11.el9.aarch64
- [EXCLUDED] kernel-devel 5.14.0-611.42.1.el9_7.aarch64
- [EXCLUDED] libtool 2.4.6-46.el9.aarch64
- [EXCLUDED] ruby 3.0.7-165.el9_5.aarch64

#### rhel-9-for-aarch64-baseos-rpms (12)

- tuned 2.26.0-1.el9.noarch
- tmux 3.2a-5.el9.aarch64
- man-pages 6.04-9.el9_7.noarch
- at 3.1.23-13.el9_7.aarch64
- unzip 6.0-59.el9.aarch64
- strace 6.12-1.el9.aarch64
- rsync 3.2.5-3.el9.aarch64
- tree 1.8.0-10.el9.aarch64
- info 6.7-15.el9.aarch64
- sssd 2.9.7-4.el9_7.1.aarch64
- [EXCLUDED] make 4.3-8.el9.aarch64
- [EXCLUDED] python3-dnf-plugin-versionlock 4.3.0-24.el9_7.noarch

### In target image only (not on inspected host)
- NetworkManager-cloud-setup
- WALinuxAgent-udev
- adcli
- avahi-libs
- bash-completion
- binutils
- binutils-gold
- bootc
- bootupd
- bubblewrap
- bzip2
- cloud-utils-growpart
- composefs
- composefs-libs
- console-login-helper-messages
- console-login-helper-messages-issuegen
- console-login-helper-messages-profile
- cryptsetup
- cyrus-sasl-gssapi
- dnf-bootc
- dracut-network
- dracut-squash
- elfutils-debuginfod-client
- flatpak-session-helper
- fuse
- glibc-minimal-langpack
- gssproxy
- iptables-nft-services
- kexec-tools
- libev
- libicu
- libipa_hbac
- libnfsidmap
- libpkgconf
- libsmbclient
- libtirpc
- libverto-libev
- libwbclient
- lsof
- lzo
- nano
- net-tools
- nfs-utils
- nss-altfiles
- nvme-cli
- ostree
- ostree-libs
- p11-kit-server
- pkgconf
- pkgconf-m4
- pkgconf-pkg-config
- python3-pexpect
- python3-ptyprocess
- quota
- quota-nls
- rpcbind
- rpm-ostree
- rpm-ostree-libs
- samba-client-libs
- samba-common
- samba-common-libs
- skopeo
- snappy
- socat
- sos
- squashfs-tools
- sssd-ad
- sssd-common-pac
- sssd-ipa
- sssd-krb5
- sssd-krb5-common
- sssd-ldap
- sssd-nfs-idmap
- stalld
- toolbox
- tpm2-tools
- zram-generator
- zstd

- Module Streams: 1 enabled (1 need enable in image)

- Version Locks: 1 packages pinned

## Services

| Unit | Current | Default | Action |
|------|---------|---------|--------|
| httpd.service | enabled | disabled | enable |
| insights-client-boot.service | enabled | disabled | enable |
| kdump.service | masked | enabled | mask |
| NetworkManager-wait-online.service | enabled | disabled | enable |
| nginx.service | enabled | disabled | enable |
| [EXCLUDED] sssd.service | disabled | enabled | disable |
| dnf-makecache.timer | enabled | disabled | enable |
| myapp-report.timer | enabled | disabled | enable |

### Systemd drop-in overrides

**httpd.service** — `etc/systemd/system/httpd.service.d/override.conf`
```ini
[Service]
TimeoutStartSec=600
LimitNOFILE=65535
```

**nginx.service** — `etc/systemd/system/nginx.service.d/override.conf`
```ini
[Service]
LimitNOFILE=131072
ExecStartPost=/usr/local/bin/notify-deploy.sh
```

[EXCLUDED] **httpd.service** — `etc/systemd/system/httpd.service.d/limits.conf`

[EXCLUDED] **httpd.service** — `etc/systemd/system/httpd.service.d/limits.conf`

## Configuration Files

- RPM-owned modified: 19
- Unowned: 47
- `/etc/security/limits.conf` (rpm_owned_modified — rpm -Va flags: `S.5....T.`)
- [EXCLUDED] `/etc/crontab` (rpm_owned_modified — rpm -Va flags: `.......T.`)
- `/etc/selinux/targeted/contexts/customizable_types` (rpm_owned_modified — rpm -Va flags: `.......T.`)
- [EXCLUDED] `/etc/selinux/targeted/contexts/files/file_contexts.local` (rpm_owned_modified — rpm -Va flags: `S.5....T.`)
- `/etc/rhsm/rhsm.conf` (rpm_owned_modified — rpm -Va flags: `S.5....T.`)
- `/etc/audit/auditd.conf` (rpm_owned_modified — rpm -Va flags: `SM5....T.`)
- [EXCLUDED] `/etc/chrony.conf` (rpm_owned_modified — rpm -Va flags: `S.5....T.`)
- `/etc/ssh/sshd_config` (rpm_owned_modified — rpm -Va flags: `SM5....T.`)
- `/etc/nginx/nginx.conf` (rpm_owned_modified — rpm -Va flags: `S.5....T.`)
- `/etc/httpd/conf/httpd.conf` (rpm_owned_modified — rpm -Va flags: `S.5....T.`)
- `/etc/nsswitch.conf.bak` (unowned)
- [EXCLUDED] `/etc/driftify.stamp` (unowned)
- `/etc/multipath.conf` (unowned)
- `/etc/sysctl.d/99-driftify.conf` (unowned)
- `/etc/grub.d/00_tuned` (unowned)
- `/etc/cron.daily/cleanup.sh` (unowned)
- `/etc/cron.d/backup-daily` (unowned)
- `/etc/nginx/nginx.conf.rpmsave` (unowned)
- [EXCLUDED] `/etc/myapp/app.conf` (unowned)
- `/etc/myapp/database.yml` (unowned)
- `/etc/myapp/server.key` (unowned)
- `/etc/httpd/conf/httpd.conf.rpmsave` (unowned)
- `/etc/ssh/sshd_config.d/01-permitrootlogin.conf` (unowned)
- `/etc/NetworkManager/system-connections/enp0s5.nmconnection` (unowned)
- [EXCLUDED] `/etc/firewalld/zones/public.xml` (unowned)
- `/etc/dnf/modules.d/postgresql.module` (unowned)
- `/etc/crontab` (rpm_owned_modified — rpm -Va flags: `S.5....T.`)
- `/etc/krb5.conf` (rpm_owned_modified — rpm -Va flags: `S.5....T.`)
- `/etc/selinux/targeted/contexts/files/file_contexts.local` (rpm_owned_modified — rpm -Va flags: `S.5....T.`)
- [EXCLUDED] `/etc/chrony.conf` (rpm_owned_modified — rpm -Va flags: `S.5....T.`)
- [EXCLUDED] `/etc/ssh/sshd_config` (rpm_owned_modified — rpm -Va flags: `SM5....T.`)
- [EXCLUDED] `/etc/driftify.stamp` (unowned)
- `/etc/words.conf` (unowned)
- `/etc/logrotate.d/myapp` (unowned)
- `/etc/dracut.conf.d/driftify.conf` (unowned)
- `/etc/modprobe.d/driftify.conf` (unowned)
- `/etc/profile.d/custom-env.sh` (unowned)
- `/etc/profile.d/proxy.sh` (unowned)
- `/etc/modules-load.d/driftify.conf` (unowned)
- `/etc/myapp/app.conf` (unowned)
- [EXCLUDED] `/etc/myapp/database.conf` (unowned)
- `/etc/myapp/cifs.creds` (unowned)
- `/etc/audit/rules.d/driftify.rules` (unowned)
- `/etc/audit/rules.d/driftify-file-watch.rules` (unowned)
- `/etc/NetworkManager/system-connections/mgmt.nmconnection` (unowned)
- [EXCLUDED] `/etc/firewalld/zones/public.xml` (unowned)
- `/etc/firewalld/zones/myapp.xml` (unowned)
- `/etc/systemd/system/driftify-backup.timer` (unowned)
- `/etc/systemd/system/driftify-backup.service` (unowned)
- `/etc/systemd/system/myapp-report.service` (unowned)
- `/etc/systemd/system/myapp-report.timer` (unowned)
- [EXCLUDED] `/etc/security/limits.conf` (rpm_owned_modified — rpm -Va flags: `S.5....T.`)
- [EXCLUDED] `/etc/chrony.conf` (rpm_owned_modified — rpm -Va flags: `S.5....T.`)
- [EXCLUDED] `/etc/httpd/conf/httpd.conf` (rpm_owned_modified — rpm -Va flags: `S.5....T.`)
- `/etc/dnf/plugins/versionlock.list` (rpm_owned_modified — rpm -Va flags: `S.5....T.`)
- [EXCLUDED] `/etc/driftify.stamp` (unowned)
- [EXCLUDED] `/etc/multipath.conf` (unowned)
- `/etc/auto.app` (unowned)
- `/etc/firewalld/direct.xml` (unowned)
- `/etc/cron.d/complex-job` (unowned)
- `/etc/auto.master.d/app.autofs` (unowned)
- [EXCLUDED] `/etc/myapp/database.conf` (unowned)
- [EXCLUDED] `/etc/firewalld/zones/public.xml` (unowned)
- `/etc/sysconfig/network-scripts/route-eth0` (unowned)
- [EXCLUDED] `/etc/dnf/modules.d/postgresql.module` (unowned)
- `/etc/lvm/profile/driftify-thin.profile` (unowned)

## Network

### Firewall zones (bake into image)

**public:** services=ssh, dhcpv6-client, cockpit, http, https | ports=8080/tcp | rich rules=0

#### Alternative: firewall-offline-cmd (instead of COPY)

```dockerfile
RUN firewall-offline-cmd --zone=public --add-service=ssh
RUN firewall-offline-cmd --zone=public --add-service=dhcpv6-client
RUN firewall-offline-cmd --zone=public --add-service=cockpit
RUN firewall-offline-cmd --zone=public --add-service=http
RUN firewall-offline-cmd --zone=public --add-service=https
RUN firewall-offline-cmd --zone=public --add-port=8080/tcp
```

**myapp:** services=http, https | ports=8080/tcp | rich rules=0

#### Alternative: firewall-offline-cmd (instead of COPY)

```dockerfile
RUN firewall-offline-cmd --zone=myapp --add-service=http
RUN firewall-offline-cmd --zone=myapp --add-service=https
RUN firewall-offline-cmd --zone=myapp --add-port=8080/tcp
```

## Scheduled tasks

### Existing systemd timers (local)

| Timer | Schedule | ExecStart | Path |
|-------|----------|-----------|------|
| driftify-backup | *-*-* 03:00:00 | `/usr/local/bin/backup.sh` | `etc/systemd/system/driftify-backup.timer` |
| myapp-report | daily | `/usr/local/bin/generate-report.sh` | `etc/systemd/system/myapp-report.timer` |

- 14 vendor timer(s) from the base image are present and will carry over automatically.

### Cron-converted timers

- **cron-backup-daily** — converted from `etc/cron.d/backup-daily` (cron: `0 2 * * *`)
- **cron-daily-cleanup-sh** — converted from `etc/cron.daily/cleanup.sh` (cron: `@daily`)
- **cron-appuser** — converted from `var/spool/cron/appuser` (cron: `*/15 * * * *`)
- [EXCLUDED] **cron-complex-job** — converted from `etc/cron.d/complex-job` (cron: `30 6 * * 1-5`)

### Cron jobs

- 3 package-owned cron job(s) not listed (handled by package install).

| Path | Source | Action |
|------|--------|--------|
| `etc/cron.d/backup-daily` | cron.d | Convert to systemd timer |
| `etc/cron.daily/cleanup.sh` | cron.daily | Convert to systemd timer |
| `var/spool/cron/appuser` | spool/cron (appuser) | Convert to systemd timer |
| [EXCLUDED] `etc/cron.d/complex-job` | cron.d | Convert to systemd timer |

## Container workloads

### Quadlet units

| Unit | Image | Path |
|------|-------|------|
| webapp.container | `registry.example.com/myorg/webapp:v2.1.3` | `etc/containers/systemd/webapp.container` |
| redis.container | `docker.io/library/redis:7-alpine` | `etc/containers/systemd/redis.container` |
| myapp.network | `*none*` | `etc/containers/systemd/myapp.network` |
| dev-tools.container | `quay.io/toolbox/toolbox:latest` | `home/appuser/.config/containers/systemd/dev-tools.container` |

### Compose files

**`opt/myapp/docker-compose.yml`**

| Service | Image |
|---------|-------|
| app | `registry.example.com/myorg/webapp:v2.1.3` |
| db | `docker.io/library/postgres:16` |

## Non-RPM software

### Compiled binaries

| Path | Language | Linking | Shared Libraries |
|------|----------|---------|------------------|
| `opt/myapp` | c/c++ | dynamic | libpython3.14.so.1.0, libc.so.6 |
| `usr/local/bin/driftify-probe` | go | static | — |
| [EXCLUDED] `usr/local/bin/mystery-tool` | c/c++ | dynamic | libc.so.6, ld-linux-aarch64.so.1 |

### Python virtual environments

#### `opt/myapp/venv` (isolated)

| Package | Version |
|---------|---------|
| blinker | 1.9.0 |
| certifi | 2026.2.25 |
| charset-normalizer | 3.4.6 |
| click | 8.1.8 |
| Flask | 3.1.3 |
| gunicorn | 23.0.0 |
| idna | 3.11 |
| importlib_metadata | 8.7.1 |
| itsdangerous | 2.2.0 |
| Jinja2 | 3.1.6 |
| MarkupSafe | 3.0.3 |
| packaging | 26.0 |
| pip | 21.3.1 |
| requests | 2.32.5 |
| setuptools | 53.0.0 |
| urllib3 | 2.6.3 |
| Werkzeug | 3.1.6 |
| zipp | 3.23.0 |

### Other non-RPM items

| Path / Name | Version | Confidence | Method |
|-------------|---------|------------|--------|
| `opt/tools` | — | low | directory scan |
| `usr/local/bin/bundle` | — | low | file scan |
| `usr/local/bin/bundler` | — | low | file scan |
| `usr/local/bin/tilt` | — | low | file scan |
| `usr/local/share` | 1.7.0 | medium | strings (first 4KB) |
| `opt/webapp` | — | high | npm package-lock.json |
| `usr/local/bin/deploy.sh` | — | low | file scan |

## Kernel and boot

- cmdline: `BOOT_IMAGE=(hd0,gpt2)/vmlinuz-5.14.0-611.36.1.el9_7.aarch64 root=/dev/mapper/rhel_rhel9-root ro crashkernel=1G-4G:256M,4G-64G:320M,64G-:576M rd.lvm.lv=rhel_rhel9/root rd.lvm.lv=rhel_rhel9/swap`
- GRUB defaults present
- Tuned profile: **virtual-guest**

- 33 kernel module(s) loaded at inspection time (hardware-specific, not included in the image). See modules-load.d entries below for explicitly configured modules.

### Non-default sysctl values (6)

| Key | Runtime | Default | Source |
|-----|---------|---------|--------|
| `fs.file-max` | **2097152** | — | `etc/sysctl.d/99-driftify.conf` |
| `net.core.somaxconn` | **4096** | — | `etc/sysctl.d/99-driftify.conf` |
| `net.ipv4.ip_local_port_range` | **32768	60999** | — | `etc/sysctl.d/99-driftify.conf` |
| `net.ipv4.tcp_keepalive_time` | **7200** | — | `etc/sysctl.d/99-driftify.conf` |
| `net.ipv4.tcp_max_syn_backlog` | **128** | — | `etc/sysctl.d/99-driftify.conf` |
| `vm.swappiness` | **10** | — | `etc/sysctl.d/99-driftify.conf` |
- modprobe.d: `etc/modprobe.d/firewalld-sysctls.conf`
- modprobe.d: `etc/modprobe.d/tuned.conf`

## SELinux / Security

- SELinux mode: enforcing
- **Custom fcontext rules** (1):
  - `/srv/www(/.*)?    system_u:object_r:httpd_sys_content_t:s0`
- Audit rule files: 2
  - `etc/audit/rules.d/driftify-file-watch.rules`
  - `etc/audit/rules.d/driftify.rules`

## Users and groups

- User: **mrussell** (uid 1000, home `/home/mrussell`, shell `/bin/bash`)
- User: **appuser** (uid 1001, home `/home/appuser`, shell `/bin/bash`)
- User: **dbuser** (uid 1002, home `/home/dbuser`, shell `/sbin/nologin`)
- Group: **mrussell** (gid 1000)
- Group: **appgroup** (gid 1001)
- Group: **dbuser** (gid 1002)
- Group: **developers** (gid 1050, members: dbuser)

### Sudoers rules
- `root	ALL=(ALL) 	ALL`
- `%wheel	ALL=(ALL)	ALL`

### User Migration Strategy

| User | UID | Type | Strategy | Notes |
|------|-----|------|----------|-------|
| mrussell | 1000 | human | kickstart |  |
| appuser | 1001 | human | kickstart |  |
| dbuser | 1002 | service | sysusers | shell: /sbin/nologin |

**Strategies:** sysusers = systemd-sysusers drop-in (boot-time), useradd = explicit RUN in Containerfile, kickstart = deferred to deploy-time provisioning, blueprint = bootc-image-builder TOML

## Data Migration Plan (/var)

Content under `/var` is seeded at initial bootstrap and **not updated** by subsequent bootc deployments.
`tmpfiles.d` snippets ensure expected directories exist on every boot.
Review application data under `/var/lib`, `/var/log`, `/var/data` for separate migration strategies.

*No significant application data directories found under `/var`.*

## Environment-specific considerations

### Identity provider integration

This system is integrated with an identity provider (SSSD/Kerberos). Config files are included, but Kerberos keytabs are machine-specific and excluded. After deployment: re-enroll the machine in the Kerberos realm, regenerate keytabs, and verify SSSD connectivity.

### NTP/Chrony configuration

Custom NTP servers are configured in `chrony.conf`. If deploying across multiple sites with different time sources, consider making the NTP server address a deploy-time parameter.

**Note:** bootc uses a 3-way merge strategy for `/etc` during image updates. Local changes to `/etc` persist across updates, but the merge behavior has nuances — see the [bootc filesystem documentation](https://containers.github.io/bootc/filesystem.html) for details.

## Items requiring manual intervention

- 1 package(s) will be downgraded by the base image — review the Version Changes section.
- ip route failed — static route information unavailable.
- ip rule failed — policy routing rule information unavailable.

## Package Details

### Dependencies (185)

These packages are pulled in automatically by dnf. If the target image produces a different dependency set, promote packages from this list to the `dnf install` line.

- hwdata 0.348-9.20.el9.noarch
- glibc-langpack-en 2.34-231.el9_7.10.aarch64
- libnl3-cli 3.11.0-1.el9.aarch64
- libteam 1.31-16.el9_1.aarch64
- ipset-libs 7.11-11.el9_5.aarch64
- ipset 7.11-11.el9_5.aarch64
- libestr 0.1.11-4.el9.aarch64
- libfastjson 0.99.9-5.el9.aarch64
- libdaemon 0.14-23.el9.aarch64
- kernel-tools-libs 5.14.0-611.36.1.el9_7.aarch64
- firewalld-filesystem 1.3.4-15.el9_6.noarch
- pciutils 3.7.0-7.el9.aarch64
- python3-libselinux 3.6-3.el9.aarch64
- libcap-ng-python3 0.8.2-7.el9.aarch64
- teamd 1.31-16.el9_1.aarch64
- python3-nftables 1.0.9-6.el9_7.aarch64
- python3-firewall 1.3.4-15.el9_6.noarch
- crontabs 1.11-27.20190603git.el9_0.noarch
- cronie-anacron 1.5.7-15.el9.aarch64
- cronie 1.5.7-15.el9.aarch64
- fonts-filesystem 2.0.5-7.el9.1.noarch
- dejavu-sans-fonts 2.37-18.el9.noarch
- langpacks-core-font-en 3.0-16.el9.noarch
- langpacks-core-en 3.0-16.el9.noarch
- rsyslog-logrotate 8.2506.0-2.el9.aarch64
- rsyslog 8.2506.0-2.el9.aarch64
- grubby 8.40-68.el9.aarch64
- insights-client 3.2.8-1.el9.noarch
- initscripts-service 10.11.8-4.el9.noarch
- audit 3.1.5-7.el9.aarch64
- rhc 0.2.7-1.el9_6.aarch64
- dnf-plugins-core 4.3.0-24.el9_7.noarch
- rpm-plugin-audit 4.16.1.3-39.el9.aarch64
- NetworkManager-team 1.54.0-3.el9_7.aarch64
- dracut-config-rescue 057-104.git20250919.el9_7.aarch64
- sssd-kcm 2.9.7-4.el9_7.1.aarch64
- initscripts-rename-device 10.11.8-4.el9.aarch64
- firewalld 1.3.4-15.el9_6.noarch
- kernel-tools 5.14.0-611.36.1.el9_7.aarch64
- prefixdevname 0.1.0-8.el9.aarch64
- lshw B.02.20-2.el9.aarch64
- lsscsi 0.32-6.el9.aarch64
- libsysfs 2.1.1-11.el9.aarch64
- rootfiles 8.1-35.el9.noarch
- perl-Digest 1.19-4.el9.noarch
- perl-Digest-MD5 2.58-4.el9.aarch64
- perl-B 1.80-481.1.el9_6.aarch64
- perl-FileHandle 2.03-481.1.el9_6.noarch
- perl-Data-Dumper 2.174-462.el9.aarch64
- perl-libnet 3.13-4.el9.noarch
- perl-AutoLoader 5.74-481.1.el9_6.noarch
- perl-base 2.27-481.1.el9_6.noarch
- perl-URI 5.09-3.el9.noarch
- perl-Time-Local 1.300-7.el9.noarch
- perl-Mozilla-CA 20200520-6.el9.noarch
- perl-if 0.60.800-481.1.el9_6.noarch
- perl-IO-Socket-IP 0.41-5.el9.noarch
- perl-File-Path 2.18-4.el9.noarch
- perl-Pod-Escapes 1.07-460.el9.noarch
- perl-Text-Tabs+Wrap 2013.0523-460.el9.noarch
- perl-IO-Socket-SSL 2.073-2.el9.noarch
- perl-Net-SSLeay 1.94-3.el9.aarch64
- perl-Term-ANSIColor 5.01-461.el9.noarch
- perl-Class-Struct 0.66-481.1.el9_6.noarch
- perl-POSIX 1.94-481.1.el9_6.aarch64
- perl-IPC-Open3 1.21-481.1.el9_6.noarch
- perl-subs 1.03-481.1.el9_6.noarch
- perl-Term-Cap 1.17-460.el9.noarch
- perl-File-Temp 0.231.100-4.el9.noarch
- perl-Pod-Simple 3.42-4.el9.noarch
- perl-HTTP-Tiny 0.076-462.el9.noarch
- perl-Socket 2.031-4.el9.aarch64
- perl-SelectSaver 1.02-481.1.el9_6.noarch
- perl-Symbol 1.08-481.1.el9_6.noarch
- perl-File-stat 1.09-481.1.el9_6.noarch
- perl-podlators 4.14-460.el9.noarch
- perl-Pod-Perldoc 3.28.01-461.el9.noarch
- perl-Text-ParseWords 3.30-460.el9.noarch
- perl-Fcntl 1.13-481.1.el9_6.aarch64
- perl-mro 1.23-481.1.el9_6.aarch64
- perl-IO 1.43-481.1.el9_6.aarch64
- perl-overloading 0.02-481.1.el9_6.noarch
- perl-Pod-Usage 2.01-4.el9.noarch
- perl-parent 0.238-460.el9.noarch
- perl-MIME-Base64 3.16-4.el9.aarch64
- perl-constant 1.33-461.el9.noarch
- perl-Scalar-List-Utils 1.56-462.el9.aarch64
- perl-Errno 1.30-481.1.el9_6.aarch64
- perl-File-Basename 2.85-481.1.el9_6.noarch
- perl-Getopt-Std 1.12-481.1.el9_6.noarch
- perl-Storable 3.21-460.el9.aarch64
- perl-overload 1.31-481.1.el9_6.noarch
- perl-vars 1.05-481.1.el9_6.noarch
- perl-Getopt-Long 2.52-4.el9.noarch
- perl-Carp 1.50-460.el9.noarch
- perl-PathTools 3.78-461.el9.aarch64
- perl-NDBM_File 1.15-481.1.el9_6.aarch64
- perl-Encode 3.08-462.el9.aarch64
- perl-Exporter 5.74-461.el9.noarch
- perl-libs 5.32.1-481.1.el9_6.aarch64
- perl-interpreter 5.32.1-481.1.el9_6.aarch64
- libtraceevent 1.8.4-2.el9.aarch64
- python3-linux-procfs 0.7.3-1.el9.noarch
- python3-pyudev 0.22.0-6.el9.noarch
- hdparm 9.62-2.el9.aarch64
- opencsd 1.2.1-1.el9.aarch64
- libbabeltrace 1.5.8-10.el9.aarch64
- python3-perf 5.14.0-611.42.1.el9_7.aarch64
- apr 1.7.0-12.el9_3.aarch64
- apr-util-openssl 1.6.1-23.el9.aarch64
- apr-util 1.6.1-23.el9.aarch64
- apr-util-bdb 1.6.1-23.el9.aarch64
- git-core 2.47.3-1.el9_6.aarch64
- redhat-logos-httpd 90.5-1.el9_6.1.noarch
- nginx-filesystem 1.20.1-22.el9_6.3.noarch
- nginx-core 1.20.1-22.el9_6.3.aarch64
- git-core-doc 2.47.3-1.el9_6.noarch
- httpd-tools 2.4.62-7.el9_7.3.aarch64
- vim-filesystem 8.2.2637-23.el9_7.noarch
- vim-common 8.2.2637-23.el9_7.aarch64
- mailcap 2.1.49-5.el9.noarch
- httpd-filesystem 2.4.62-7.el9_7.3.noarch
- httpd-core 2.4.62-7.el9_7.3.aarch64
- mod_lua 2.4.62-7.el9_7.3.aarch64
- mod_http2 2.0.26-5.el9.aarch64
- emacs-filesystem 27.2-18.el9.noarch
- perl-lib 0.65-481.1.el9_6.aarch64
- perl-File-Find 1.37-481.1.el9_6.noarch
- perl-DynaLoader 1.47-481.1.el9_6.aarch64
- perl-TermReadKey 2.38-11.el9.aarch64
- gpm-libs 1.20.7-29.el9.aarch64
- perl-Error 0.17029-7.el9.noarch
- perl-Git 2.47.3-1.el9_6.noarch
- hwloc-libs 2.4.1-6.el9_7.aarch64
- libibverbs 57.0-2.el9.aarch64
- libpcap 1.10.0-4.el9.aarch64
- python3-setools 4.4.4-1.el9.aarch64
- python3-audit 3.1.5-7.el9.aarch64
- python3-libsemanage 3.6-5.el9_6.aarch64
- nodejs-libs 16.20.2-8.el9_4.aarch64
- nodejs-docs 16.20.2-8.el9_4.noarch
- nodejs-full-i18n 16.20.2-8.el9_4.aarch64
- npm 8.19.4-1.16.20.2.8.el9_4.aarch64
- checkpolicy 3.6-1.el9.aarch64
- man-pages-overrides 9.0.0.0-1.el9.noarch
- python3-distro 1.5.0-7.el9.noarch
- python3-policycoreutils 3.6-3.el9.noarch
- jemalloc 5.2.1-2.el9.aarch64
- sssd-proxy 2.9.7-4.el9_7.1.aarch64
- [EXCLUDED] m4 1.4.19-1.el9.aarch64
- [EXCLUDED] cmake-filesystem 3.26.5-3.el9_7.aarch64
- [EXCLUDED] perl-File-Copy 2.34-481.1.el9_6.noarch
- [EXCLUDED] perl-File-Compare 1.100.600-481.1.el9_6.noarch
- [EXCLUDED] perl-threads 2.25-460.el9.aarch64
- [EXCLUDED] libmpc 1.2.1-4.el9.aarch64
- [EXCLUDED] cpp 11.5.0-11.el9.aarch64
- [EXCLUDED] perl-threads-shared 1.61-460.el9.aarch64
- [EXCLUDED] perl-Thread-Queue 3.14-460.el9.noarch
- [EXCLUDED] cmake-data 3.26.5-3.el9_7.noarch
- [EXCLUDED] flex 2.6.4-9.el9.aarch64
- [EXCLUDED] bison 3.7.4-5.el9.aarch64
- [EXCLUDED] kernel-headers 5.14.0-611.42.1.el9_7.aarch64
- [EXCLUDED] glibc-devel 2.34-231.el9_7.10.aarch64
- [EXCLUDED] libxcrypt-devel 4.4.18-3.el9.aarch64
- [EXCLUDED] openssl-devel 3.5.1-7.el9_7.aarch64
- [EXCLUDED] boost-regex 1.75.0-13.el9_7.aarch64
- [EXCLUDED] source-highlight 3.1.9-12.el9.aarch64
- [EXCLUDED] gdb-headless 16.3-2.el9.aarch64
- [EXCLUDED] libasan 11.5.0-11.el9.aarch64
- [EXCLUDED] valgrind-docs 3.25.1-3.el9.aarch64
- [EXCLUDED] valgrind-gdb 3.25.1-3.el9.aarch64
- [EXCLUDED] valgrind-scripts 3.25.1-3.el9.aarch64
- [EXCLUDED] libubsan 11.5.0-11.el9.aarch64
- [EXCLUDED] libzstd-devel 1.5.5-1.el9.aarch64
- [EXCLUDED] zlib-devel 1.2.11-40.el9.aarch64
- [EXCLUDED] elfutils-libelf-devel 0.193-1.el9.aarch64
- [EXCLUDED] ruby-libs 3.0.7-165.el9_5.aarch64
- [EXCLUDED] rubygem-bigdecimal 3.0.0-165.el9_5.aarch64
- [EXCLUDED] ruby-default-gems 3.0.7-165.el9_5.noarch
- [EXCLUDED] rubygem-bundler 2.2.33-165.el9_5.noarch
- [EXCLUDED] rubygem-io-console 0.5.7-165.el9_5.aarch64
- [EXCLUDED] rubygem-json 2.5.1-165.el9_5.aarch64
- [EXCLUDED] rubygem-psych 3.3.2-165.el9_5.aarch64
- [EXCLUDED] rubygem-rdoc 6.3.4.1-165.el9_5.noarch
- [EXCLUDED] rubygems 3.2.33-165.el9_5.noarch

### Package Dependency Tree (225 packages beyond base image)

**40 leaf packages** → 185 dependencies

**libtool** (71 deps)
  ├── cpp
  ├── emacs-filesystem
  ├── glibc-devel
  ├── kernel-headers
  ├── libasan
  ├── libmpc
  ├── libubsan
  ├── libxcrypt-devel
  ├── m4
  └── perl-AutoLoader
  └── ... and 61 more

**kernel-devel** (70 deps)
  ├── bison
  ├── cpp
  ├── elfutils-libelf-devel
  ├── flex
  ├── glibc-devel
  ├── kernel-headers
  ├── libasan
  ├── libmpc
  ├── libubsan
  └── libxcrypt-devel
  └── ... and 60 more

**git** (64 deps)
  ├── emacs-filesystem
  ├── git-core
  ├── git-core-doc
  ├── perl-AutoLoader
  ├── perl-B
  ├── perl-Carp
  ├── perl-Class-Struct
  ├── perl-Data-Dumper
  ├── perl-Digest
  └── perl-Digest-MD5
  └── ... and 54 more

**automake** (64 deps)
  ├── emacs-filesystem
  ├── m4
  ├── perl-AutoLoader
  ├── perl-B
  ├── perl-Carp
  ├── perl-Class-Struct
  ├── perl-Data-Dumper
  ├── perl-Digest
  ├── perl-Digest-MD5
  └── perl-DynaLoader
  └── ... and 54 more

**autoconf** (61 deps)
  ├── emacs-filesystem
  ├── m4
  ├── perl-AutoLoader
  ├── perl-B
  ├── perl-Carp
  ├── perl-Class-Struct
  ├── perl-Data-Dumper
  ├── perl-Digest
  ├── perl-Digest-MD5
  └── perl-DynaLoader
  └── ... and 51 more

**httpd** (7 deps)
  ├── apr
  ├── apr-util
  ├── apr-util-bdb
  ├── httpd-core
  ├── httpd-filesystem
  ├── httpd-tools
  └── redhat-logos-httpd

**policycoreutils-python-utils** (7 deps)
  ├── checkpolicy
  ├── python3-audit
  ├── python3-distro
  ├── python3-libselinux
  ├── python3-libsemanage
  ├── python3-policycoreutils
  └── python3-setools

**gcc** (7 deps)
  ├── cpp
  ├── glibc-devel
  ├── kernel-headers
  ├── libasan
  ├── libmpc
  ├── libubsan
  └── libxcrypt-devel

**langpacks-en** (4 deps)
  ├── dejavu-sans-fonts
  ├── fonts-filesystem
  ├── langpacks-core-en
  └── langpacks-core-font-en

**gdb** (4 deps)
  ├── boost-regex
  ├── gdb-headless
  ├── libbabeltrace
  └── source-highlight

**nginx** (3 deps)
  ├── nginx-core
  ├── nginx-filesystem
  └── redhat-logos-httpd

**tuned** (3 deps)
  ├── hdparm
  ├── python3-linux-procfs
  └── python3-pyudev

**vim-enhanced** (3 deps)
  ├── gpm-libs
  ├── vim-common
  └── vim-filesystem

**nmap-ncat** (2 deps)
  ├── libibverbs
  └── libpcap

**tcpdump** (2 deps)
  ├── libibverbs
  └── libpcap

**htop** (1 deps)
  └── hwloc-libs

**fd-find** (1 deps)
  └── jemalloc

**nodejs** (1 deps)
  └── nodejs-libs

**sssd** (1 deps)
  └── sssd-proxy

**ruby** (1 deps)
  └── ruby-libs

**bat** (0 deps — installed independently)

**epel-release** (0 deps — installed independently)

**python3-pip** (0 deps — installed independently)

**tmux** (0 deps — installed independently)

**wget** (0 deps — installed independently)

**at** (0 deps — installed independently)

**info** (0 deps — installed independently)

**man-pages** (0 deps — installed independently)

**rsync** (0 deps — installed independently)

**strace** (0 deps — installed independently)

**the_silver_searcher** (0 deps — installed independently)

**tree** (0 deps — installed independently)

**unzip** (0 deps — installed independently)

**cmake** (0 deps — installed independently)

**fzf** (0 deps — installed independently)

**hyperfine** (0 deps — installed independently)

**make** (0 deps — installed independently)

**python3-dnf-plugin-versionlock** (0 deps — installed independently)

**ripgrep** (0 deps — installed independently)

**valgrind** (0 deps — installed independently)


## Redactions (secrets)

- **/etc/rhsm/rhsm.conf**: PASSWORD — Use a secret store or inject at deploy time.
