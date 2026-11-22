# `%ghost` files under `/etc`, and authselect

## Why a branch of their own

A package lists a `%ghost` file without shipping its content. rpm has
nothing to compare it with: `rpm -V` checks a ghost file's mode, owner
and group, never its digest or size, and the unowned walk skips it
because a package owns it. Without the ghost branch, an admin's edit to
one reaches no collector. Measured on RHEL 9.8 and 10.2 (and Fedora 44
for grub): `/etc/sssd/sssd.conf` (`cng`),
`/etc/authselect/authselect.conf` and `nsswitch.conf` (`g`, no config
flag), `/etc/default/grub` (`cng`), `/etc/machine-id` and
`/etc/hostname` (`cng`).

A `chmod` of a ghost file does reach `rpm -Va`, as `.M.......  g
<path>` (measured on both releases, 2026-11-21). The `rpm -Va` branch
drops that row as metadata-only, so the ghost branch must not skip a
path just because `rpm -Va` named it.

## The rule

`crates/collect/src/inspectors/config/ghost.rs` and
`collect_ghost_configs` in `config/mod.rs`:

- **Found by one query:** `rpm -qa --queryformat
  '[%{=NAME} %{FILEFLAGS:fflags} %{FILENAMES:shescape}\n]'`. `%{=NAME}`
  repeats the scalar on each array iteration. A path can hold a newline,
  and rpm's queryformat cannot print a NUL (`\0` prints a literal `0` on
  rpm 4.16.1.3 and 4.19.1.1), so the path is shell-quoted and the
  output is read as a stream of `<package> <flags> '<path>'\n` records:
  a quote inside the path is `'\''`, everything else is literal. Every
  record must parse or the section is degraded. A decoded path holding a
  control character is a scan warning and is never read. The full query
  output of both test hosts (118,684 and 124,732 records) parses with no
  error and no refused path.
- **Every ghost file under `/etc`, not only `%config`:** authselect's
  files carry `g` alone. Requiring `c` loses them.
- **Read only a regular file**, by `Executor::file_kind` (`lstat`): not
  a symlink, not a directory (`%ghost %dir`), and never a FIFO, socket
  or device, since opening a FIFO with no writer blocks and the
  subprocess timeout does not cover file reads. `MockExecutor`'s
  `with_special_file` registers such a path and panics if it is read.
- **Never through a directory link**: `admit::locate` refuses a path
  with a symlink in any directory above it, with a scan warning, and
  hands back the normalized spelling the class is decided on (see
  [Admission](#admission-one-gate-before-every-read)).
- **Carries only general configuration no other section owns.**
  `classify_ghost` decides every path before anything reads it, in this
  order:
  - `Credential`: key material by path (`GHOST_CREDENTIAL_EXACT`:
    `chrony.keys`, bind's `rndc.key` and `rndc.conf`, unbound's control
    and server keys). Never opened; a scan warning names the path and
    says the content was withheld.
  - `MachineState`: the § 5.2 members of the machine-state class spec by
    path (`is_machine_state_member`). Skipped silently until the class
    lands.
  - `OwnedBy(inspector)`: a file another section records. The tables
    (`OWNED_ELSEWHERE`, `OWNED_BY_RULE_ELSEWHERE`) hold each owner's own
    constant or predicate, the one it reads the file by: storage's
    `FSTAB_PATH` and `CRYPTTAB_PATH`, the users section's account
    databases, the SELinux section's `SELINUX_CONFIG_PATH`, kernelboot's
    `LOCALE_CONF_PATH`, and the repos section's `is_repo_file_path`
    (a `*.repo` file directly in `/etc/yum.repos.d`, the one test
    `collect_repo_files` reads by). A nested file or another suffix there
    is general configuration: a claim wider than its reader drops a file
    from both. Add a claim there, in the owner's module, rather than a
    second list here. Storage claims `crypttab`
    but does not parse it yet, so an entry in it is recorded nowhere.
  - `Generated`: written on the host from input the image has or writes
    again (`GHOST_GENERATED_EXACT`, `GHOST_GENERATED_PREFIX`, authselect's
    generated files), or on the unowned walk's path lists
    (`is_excluded_unowned_path`), a dev artifact, or the SELinux
    fcontext store.
  - `Deferred`: the keymap files, which the open locale item decides.
  - `AuditRules`: `/etc/audit/audit.rules`, which the host decides (see
    [audit.rules](#etcauditauditrules)).
  - `Config`: everything else, collected as `RpmOwnedModified`,
    `rpm_va_flags: None`, with its package and content (redaction
    applies).
- **Skipped before the class:** a path an earlier branch already
  collected (an `rpm -Va` row with a content flag). **After it:** the
  unowned walk's checks that need the host (the unit-link test, DHCP
  connections); and, after the read, a file whose content is a keytab
  (first bytes `05 01` or `05 02`) or a whole-file private key (§ 5.3),
  dropped before its content is kept, with the same warning as a
  credential by path. A key embedded in a larger configuration file is
  configuration and stays with redaction.
- **Not run** when `owned_paths` is empty; a failed query degrades the
  section. Mocks that run the config inspector with owned paths must
  register the query (an empty answer is fine), or the section reads
  degraded.

When the machine-state class lands, the member skips go and the push
becomes `ConfigCollector::admit` with the same fields.

## Every ghost file on the test hosts, classified

Measured 2026-11-21 on `rhel9` (RHEL 9.8) and `rhel10` (RHEL 10.2),
both after a driftify kitchen-sink run, with the query above,
`FILEMODES` and `stat`. The lists are in
`testdata/fixtures/config/ghost/`, and
`measured_ghost_files_land_in_known_classes` (`ghost.rs`) pins every
entry: a ghost path a later measurement adds fails it until it is placed
in the table there.

What exists under `/etc` as a ghost: on RHEL 9.8, 157 regular files, 2
empty ones, 258 symlinks and 9 directories; on RHEL 10.2, 155, 2, 497
and 10. The symlinks (CA trust links under `/etc/pki/ca-trust` on both
and `/etc/pki/tls` on 10.2, the crypto-policies back-ends, the
authselect links in `/etc/pam.d` and `nsswitch.conf` on 10.2,
`localtime`, `mtab`, `motd.d`, `issue.d`) and the directories (the
`*.target.wants` directories) are never read: lstat says they are not
regular files.

| Ghost file | Package, flags | 9.8 | 10.2 | Class | Why |
|---|---|---|---|---|---|
| `/etc/sssd/sssd.conf` | sssd-common `cng` | yes | yes | Config | SSSD's configuration |
| `/etc/audit/rules.d/audit.rules` | audit(-rules) `cng` | yes | yes | Config | the rules input `augenrules` reads |
| `/etc/authselect/authselect.conf` | authselect-libs `g` | yes | yes | Config | rendered as `authselect select` |
| `/etc/authselect/user-nsswitch.conf` | authselect-libs `g` | yes | unowned | Config | admin input to `nsswitch.conf` |
| `/etc/cockpit/disallowed-users` | cockpit-ws `g` | yes | yes | Config | `%post` writes `root`; admins edit it |
| `/etc/crypto-policies/config` | crypto-policies `cmng` | yes | yes | Config | rendered as `update-crypto-policies --set` |
| `/etc/default/grub` | grub2-tools `cng` | yes | yes | Config | collected as an advisory, never copied |
| `/etc/systemd/{journald,logind,system,user,coredump,pstore,sleep,resolved}.conf` | systemd, systemd-udev, systemd-resolved `cng` | not ghosts | absent | Config | RHEL 10 moved the defaults to `/usr/lib/systemd/`; a file here is the admin's |
| `/etc/udev/udev.conf`, `iocost.conf` | systemd-udev `cng` | not ghosts | absent | Config | the same move to `/usr/lib/udev/` |
| `/etc/containers/containers.conf` | containers-common `g` | absent | absent | Config | admin overrides of the `/usr/share/containers` defaults |
| `/etc/containers/storage.conf` | containers-common `g` | not a ghost | absent | Config | the same |
| `/etc/gai.conf`, `/etc/gnupg/gpgconf.conf`, `/etc/npmignore`, `/etc/NetworkManager/VPN` | glibc, gnupg2, npm, NetworkManager | absent | absent | Config | admin-written when present |
| `/etc/fstab` | setup `cmng` | yes | yes | OwnedBy(Storage) | the storage section reads it |
| `/etc/crypttab` | systemd-udev `cng` | empty | empty | OwnedBy(Storage) | host block devices; storage does not parse it yet |
| `/etc/selinux/config` | selinux-policy `cng` | yes | yes | OwnedBy(Selinux) | the SELinux section reads it; no renderer carries the mode |
| `/etc/locale.conf` | systemd `cng` | yes | yes | OwnedBy(KernelBoot) | kernelboot records the locale; the locale item decides carriage |
| `/etc/yum.repos.d/redhat.repo` | subscription-manager `g` | 272,787 bytes | 119,378 bytes | OwnedBy(Rpm) | the repos section's; on 9.8 over the 256 KiB read limit, which degraded the config section |
| `/etc/chrony.keys` | chrony `cg` | `cn`, not a ghost | absent | Credential | NTP authentication keys, `<id> <hash> HEX:<key>` |
| `/etc/machine-id`, `/etc/hostname` | systemd `cng` | yes | yes | MachineState | § 5.2 members |
| `/etc/authselect/{nsswitch.conf,system-auth,password-auth,fingerprint-auth,smartcard-auth,postlogin,dconf-db,dconf-locks}` | authselect-libs `g` | yes | yes | Generated | `authselect select` writes them again |
| `/etc/audit/audit.rules` | audit(-rules) `cng` | yes | yes | AuditRules | `augenrules --load` writes it from `rules.d` with the packaged units; the admin's when the host loads it directly |
| `/etc/avahi/etc/localtime` | avahi `g` | yes | yes | Generated | avahi's `%post` copies `/etc/localtime` into its chroot |
| `/etc/crypto-policies/state/*` | crypto-policies `g` | yes | yes | Generated | written from `config` |
| `/etc/crypto-policies/back-ends/nss.config` | crypto-policies `cmng` | yes | yes | Generated | the walk's back-ends glob |
| `/etc/foomatic/hashes.d/hashes.new` | cups-filters `g` | empty | empty | Generated | rewritten while the host runs (mtime after install) |
| `/etc/ld.so.cache` | glibc `cmng` | yes | yes | Generated | ldconfig |
| `/etc/machine-info` | systemd `cng` | absent | absent | Generated | the walk's exact list |
| `/etc/pki/ca-trust/extracted/*` | ca-certificates `g` | 124 files | 124 files | Generated | `update-ca-trust` |
| `/etc/selinux/targeted/contexts/files/*.bin` | selinux-policy-targeted `g` | yes | yes | Generated | compiled policy |
| `/etc/udev/hwdb.bin` | systemd-udev `g` | yes | yes | Generated | `systemd-hwdb update` |
| `/etc/lvm/cache/.cache`, `/etc/modprobe.d/kvm.rt.tuned.conf` | lvm2, tuned `g` | absent | absent | Generated | LVM's device cache; written by tuned's profile functions (`/usr/lib/tuned/functions`) |
| `/etc/cni/net.d/87-podman-bridge.conflist` | podman `g` | absent | not a ghost | Generated | the default network podman's old CNI backend wrote |
| `/etc/vconsole.conf`, `/etc/X11/xorg.conf.d/00-keyboard.conf` | systemd(-udev) `cng` | yes | yes | Deferred | the locale and keymap item |

Packages not installed on either host, read from their RPMs with `rpm
-qp` on 2026-11-21, give the rest of the credential list: `bind` lists
`/etc/rndc.key` and `/etc/rndc.conf` (`cng`), `unbound` lists
`/etc/unbound/unbound_{control,server}.{key,pem}` (`g`) on both
releases; the `.pem` certificates are `Generated` beside their keys, as
`sscg`'s `localhost.crt` is. `libreswan` has no ghost under `/etc`.

**Not covered by this branch:** on RHEL 9.8 `/etc/chrony.keys` is a
plain `%config(noreplace)` file (`cn`, 540 bytes stock). An admin who
adds a key makes it an `rpm -Va` content change, and that branch
collects it with its content. The credential list is applied to the
ghost branch only.

## Admission: one gate before every read

`crates/collect/src/inspectors/config/admit.rs`. Every exclusion the
config inspector has matches a path by its spelling, and the kernel
follows links in every component of a path, `lstat` included. Two
checks close that, and every branch (`rpm -Va`, ghost, the unowned
walk, orphans, authselect's edited files) runs both:

- **`locate`, before any rule is asked.** The path is normalized (`.`
  and repeated separators removed; a `..` is refused), and its directory
  is resolved with `resolve_final_target`. If that is not the directory
  as spelled, a directory above the file is a link and the path is
  refused. With `/etc/app/state -> /etc`, `/etc/app/state/chrony.keys`
  read `/etc/chrony.keys` as `Config` before this; the same held for
  `machine-id`, `shadow` and a `/etc/yum.repos.d` alias. A packaged path
  refused this way (ghost or `rpm -Va`) is a scan warning; a walked one
  is skipped silently, since the walk reaches the file under its own
  name. Refusing outright was chosen over resolving the path and applying
  every exclusion twice: measured on `rhel9` and `rhel10`, 2026-11-21,
  after drift runs, no packaged file lies under a directory link in
  `/etc` (`find /etc -type l -xtype d` against `rpm -qa` file lists),
  and every directory link there points out of `/etc` (`/etc/httpd/logs`,
  `/etc/swid/swidtags.d/redhat.com`, which the walk already skips) or at
  a directory the walk reaches by its own name (`/etc/ssl/certs`,
  `/etc/yum/vars`, `/etc/xdg/systemd/user`). The scans of those hosts
  held no config entry under any of them.
- **`readable`, inside `capture_config`.** A regular file is read. A
  link is followed to its end: `/dev/null` is a mask, kept empty and
  never opened; a FIFO, socket or device is never opened; a regular file
  under `/etc` is read only when `classify_ghost` calls its path
  `Config`; anything else is not read. One exception leaves `/etc`: a
  custom authselect profile's link into the shipped profiles. A modified
  packaged file re-pointed at a withheld file is a scan warning; a walked
  link leaving `/etc` (other than the `/var` and `/usr` cross-tree cases
  and the allowlist) is a `CrossTreeSymlink` advisory with no content.
- **The reads that only decide** (DHCP detection over
  `system-connections`, the crypto policy, the anacrontab modernization
  rule, the audit units) check `opens_without_blocking` first.
- **`rpm -Va` decides the kind before it reads**: a metadata-only row is
  dropped before content capture, so a ghost FIFO with a `.M.......` row
  is never opened.

**Known limitation:** the checks and the read are separate calls, so a
file replaced between them (a regular file swapped for a FIFO, or a
directory for a link) is read as what it became. Closing that needs the
executor to open with `O_NONBLOCK` and check type and path through the
descriptor. Not covered by the gate: the ostree branch of the config
inspector (not reached, since the inspector applies to package-based
hosts only), and readers outside it, such as the repos section's
`collect_repo_files`, which opens each `*.repo` entry without a type
check.

## `/etc/audit/audit.rules`

`config/audit_rules.rs`. With the packaged units, `augenrules --load`
builds the file from `/etc/audit/rules.d/*.rules` (which the walk and
`rpm -Va` carry) and loads it: `ExecStartPost=-/sbin/augenrules --load`
in `auditd.service` on RHEL 9.8 (audit 3.1.5), `ExecStart=/usr/sbin/augenrules --load`
in `audit-rules.service` on RHEL 10.2 (audit-rules 4.0.3), whose
`auditd.service` has no loader. Both units are in
`testdata/fixtures/config/audit/`, copied from the hosts on 2026-11-21
(`rpm -V` clean). RHEL also supports loading the file directly: copy
the unit to `/etc/systemd/system/`, comment out the `augenrules` line
and enable the `auditctl -R /etc/audit/audit.rules` line the RHEL 9 unit
ships commented (its own comment and the RHEL 9 hardening guide,
"Disabling augenrules").

`augenrules` (read on both hosts) always writes its output when
`rules.d` exists, since the output starts with a header line, and loads
the old file unchanged when `rules.d` is missing.

`source` reads both loader units the way systemd would: the `/etc`
unit file or the packaged one, then drop-ins from `<unit>.d/` and
`service.d/` in `/etc` and `/usr/lib` in file-name order, an `/etc` one
hiding a packaged one of the same name, an empty assignment clearing the
setting, a `/dev/null` unit masked. Then:

| Host | Class | Both exports |
|---|---|---|
| a start command runs `augenrules` (not `--check`) and `rules.d` is a directory | `Augenrules`: not collected | not staged; `rules.d` carries the rules |
| no start command runs `augenrules`, one runs `auditctl -R /etc/audit/audit.rules` | `LoadedDirectly`: collected as `RpmOwnedModified` | staged under `config/etc/audit/`, copied by `COPY config/etc/ /etc/`, as are the `/etc` override or drop-in, which the walk collects |
| anything else (no `rules.d`, a masked or missing loader, an unreadable unit or drop-in) | `Unknown`: collected | staged, and a scan warning names the file and the reason |

Carrying is the safe side: a carried file `augenrules` regenerates is
only rewritten at the first audit start, while a dropped file the host
loads directly leaves the image with no audit rules. Nothing sets the
host's 0640 on the staged file, as nothing does for the `rules.d` files.

**Not measured:** no host here was switched to direct loading, so the
`LoadedDirectly` row rests on the packaged unit's own instructions and
the unit-file reading above, not on a booted image.

## authselect

- `/etc/authselect/authselect.conf` (profile id, then one feature per
  line) is collected, never staged (`owned_elsewhere`), and rendered by
  `render/authselect.rs` as `RUN authselect select <profile> <features>
  --force` after the Configuration Files `COPY`, so a custom profile is
  in place. Every word is a `BareWord`; a refusal renders a FIXME.
- The generated files (`nsswitch.conf`, the PAM stacks, `dconf-*`) are
  not collected: `authselect select` writes them again.
  `user-nsswitch.conf` is admin input and is carried as a file (a ghost
  of `authselect-libs` on RHEL 9, unowned on RHEL 10).
- **Hand edits to the generated files** become `Modernization`
  advisories (an existing entry at that path is turned into one, so it
  is not copied over what `select` writes). Two sources, because
  `authselect check` is not enough on RHEL 10:
  - `authselect check`. It names a changed mode or a replaced link on
    both releases, and a content edit only on 1.2.6. A check that exits
    non-zero without naming a file or saying "not valid" is
    `AuthselectCheck::Failed`, never a clean result.
  - Each generated file under `/etc/authselect/` that is a regular file
    against `authselect test --<file> <profile> <features>`. `test`
    prints `File <path>:` and then the content; the file on disk has a
    header (`# Generated by authselect on <date>` and one more line on
    1.2.6, four lines on 1.5.2) up to the first blank line. With the
    header and trailing line breaks set aside the two are byte-equal on
    an untouched configuration from both releases.
  - A check or comparison that could not run is one scan warning naming
    why. A check that reports a change but no file is found is another.
- **Custom profiles.** `authselect create-profile <name> -b <base>
  --symlink-meta --symlink-pam` links `README`, `REQUIREMENTS` and the
  PAM templates into `/usr/share/authselect/vendor/<base>/` (RHEL 10
  adds `switchable-auth`). The walk collects such a link
  (`is_authselect_profile_link`) as a file with what it resolves to,
  instead of a cross-tree advisory. The renderer selects `custom/<name>`
  only when every config entry under `/etc/authselect/custom/<name>/`
  and its `README` are staged; otherwise the section is a FIXME naming
  the missing files and the command to run.

### Measured in rhel-bootc 9.8 and 10.2, 2026-11-21

On `rhel9` and `rhel10`, in throwaway containers and `podman build`
from `registry.redhat.io/rhel9/rhel-bootc:9.8` and
`registry.redhat.io/rhel10/rhel-bootc:10.2`, no host files changed.
The bytes the tests use are in `testdata/fixtures/config/authselect/`.

| | RHEL 9.8, authselect 1.2.6 | RHEL 10.2, authselect 1.5.2 |
|---|---|---|
| base image before any select | `No existing configuration detected.`, exit 2; `/etc/pam.d/system-auth` and `/etc/nsswitch.conf` regular files | profile `local`, no features; files are links into `/etc/authselect/` |
| `COPY` of `user-nsswitch.conf`, then `RUN authselect select sssd with-faillock with-mkhomedir --force` | exit 0 | exit 0 |
| what `--force` overwrote (its backup under `/var/lib/authselect/backups/`) | the base's own `fingerprint-auth`, `nsswitch.conf`, `password-auth`, `postlogin`, `smartcard-auth`, `system-auth`, replaced by links into `/etc/authselect/` | the `local` profile's output: `authselect.conf`, `dconf-db`, `dconf-locks`, the PAM files, `nsswitch.conf`, `switchable-auth` |
| carried `user-nsswitch.conf` line `hosts: files dns myhostname` | in the generated `nsswitch.conf` | in it, as `hosts:      files  dns myhostname` |
| `authselect check` afterwards | `Current configuration is valid.`, exit 0 | the same |
| `bootc container lint` | 12 passed, 1 skipped, 1 warning: `var-tmpfiles` for the select backup under `/var/lib/authselect/backups` | the same |
| `--nobackup` added | fails: `Directory [/var/lib/authselect] does not exist`, exit 5 | builds, check valid; lint still warns `var-tmpfiles`, for other files in `/var` |
| a line appended to `/etc/authselect/system-auth`, mode kept: `authselect check` | exit 3, `[error] [/etc/authselect/system-auth] has unexpected content!` | exit 0, `Current configuration is valid.` |
| `chmod 0600 password-auth`: `authselect check` | names it, `has wrong mode [0600], expected [0644]!` | the same |
| `sed -i` through `/etc/nsswitch.conf` (replaces the link) | `is not a symbolic link!`, `was not created by authselect!` | the same |
| `rpm -V authselect-libs` after the content edit | nothing for the file | the same |
| `rpm -V` after `chmod` of a ghost file | `.M.......  g /etc/authselect/password-auth` | the same |
| `authselect create-profile corp -b sssd --symlink-meta --symlink-pam` | 7 links into `/usr/share/authselect/vendor/sssd/`, 3 copied files | 8 links (adds `switchable-auth`) |
| select of that profile with `README` removed | `Unable to find profile [custom/noreadme]`, exit 1; the build fails the same way | the same |
| select with the `system-auth` template removed | exit 0, check valid, `system-auth` holds only its header (91 bytes): an empty PAM stack | the same (255 bytes) |
| custom profile staged as the export did before: links dropped | build fails at the select line | the same |
| links staged as files holding what they resolve to (what the export does now) | builds; `custom/corp with-faillock` selected; check valid; lint as above | the same |
| links staged as links | builds, same results | the same |

So `--force` is needed on RHEL 9 (the base has files authselect did not
write) and harmless on RHEL 10; `--nobackup` is not usable on RHEL 9.
The select adds its backup directory to a `var-tmpfiles` warning the
image raises without it, and adds no warning of its own.

## Other files the branch reaches

- `sssd.conf` and `conf.d/*.conf`: carried; the Containerfile adds
  `RUN chmod 0600` after the `COPY`, because SSSD refuses any other mode
  and `COPY` gives the build context's. A `*_authtok` value is redacted
  whole: SSSD's INI value is the rest of the line, spaces, tabs and
  quotes included, and a password spelled like an NSS word (`files`) is
  still a password. The SSSD pattern sets `filter_nss_pam_tokens:
  false`, which the generic `password=` patterns set to true.
- `/etc/default/grub`: the `grub_defaults` modernization rule makes it an
  advisory on any branch; it is never copied. Translating its
  `GRUB_CMDLINE_LINUX` into kargs.d is not done.
