---
title: Output Artifacts
parent: Reference
nav_order: 4
---

# Output Artifacts

Each inspectah command produces a defined set of files. This reference lists
every artifact by command, distinguishing always-written files from
conditional ones.

Source of truth: `crates/pipeline/src/render/mod.rs` (scan),
`crates/refine/src/session.rs` (refine export),
`crates/cli/src/commands/aggregate.rs` aggregate.

---

## `inspectah scan`

### Tarball naming

```
<hostname>-<YYYYMMDD>-<HHMMSS>.tar.gz
```

This pattern is schematic: it has placeholders, so there is nothing to
capture. A real name from the sample host is `rhel9-20261104-222606.tar.gz`.

The tarball wraps a single prefix directory with the same name as the stamp.
Naming is produced by `get_output_stamp()` in `render/tarball.rs`.

### Always-written artifacts

These files are written by `render_all()` on every scan, regardless of
system state.

| File | In the refine export | Description |
|------|----------------------|-------------|
| `Containerfile` | Yes | Image build definition. COPY lines are derived from the materialized config tree roots. |
| `audit-report.html` | No | HTML audit report (PatternFly). |
| `audit-report.md` | Yes | Findings, recommendations, storage plan, version drift. |
| `secrets-review.md` | No | Redacted sensitive content for operator review. |
| `README.md` | No | Summary with build commands and FIXME checklist. |
| `kickstart-suggestion.ks` | No | Deploy-time settings (network, storage, bootloader). |
| `inspection-snapshot.json` | Yes | Full structured snapshot (JSON). Can be re-rendered or imported into refine. |
| `config/` | Yes | Materialized config files to COPY into the image. Always created as a directory; contents are conditional on what the system has. |

### Conditional artifacts

These files or directories are only written when the corresponding data
exists in the snapshot. If a file is missing from your tarball, the scanned
system did not have the relevant configuration.

| File / Directory | Condition | Description |
|------------------|-----------|-------------|
| `config/etc/` | Config files with `include: true` exist | Modified configs, repo files, GPG keys, firewall zones, audit rules, PAM configs, kernel modules, modprobe, dracut conf, sysctl files (`etc/sysctl.conf` and `etc/sysctl.d/*.conf`, copied as the host had them). |
| `config/usr/lib/bootc/kargs.d/inspectah-migrated.toml` | Kernel cmdline has safe operator kargs | Kernel argument drop-in for bootc. |
| `config/usr/lib/tmpfiles.d/inspectah-var.conf` | The storage inspector found `/var` directories the image must provision, and they carry an owner and mode | tmpfiles.d fragment that creates those directories. See [`/var` provisioning](#var-provisioning). |
| `env-files/` | `non_rpm_software.env_files` has included entries | `.env` files, separated from `config/` because they are high-probability secret carriers. |
| `quadlet/` | `containers.quadlet_units` has included entries | Quadlet unit files for container workloads. One file per unit. |
| `flatpak/flatpak-install.json` | `containers.flatpak_apps` has included entries | JSON manifest of Flatpak apps to install. |
| `flatpak/flatpak-provision.service` | `containers.flatpak_apps` has included entries | Systemd service to provision Flatpak apps on first boot. |
| `drop-ins/` | `units.drop_ins` has included entries | Systemd unit drop-in overrides for units of any kind, and full unit shadows (e.g., `drop-ins/etc/systemd/system/<unit>.d/override.conf`). |
| `tuned/` | `kernel_boot.tuned_include` is true and custom profiles exist | Custom tuned performance profiles. |
| `subscription/` | `--preserve subscription` used during scan | RHEL subscription material for non-RHEL builds. Contains `entitlement/` (cert/key pairs), `rhsm/ca/` (CA certs), `rhsm/rhsm.conf`, and `redhat.repo`. |
| `inspectah-users.ks` | `users_groups` has included users or custom groups | Kickstart fragment for user/group provisioning. |
| `inspectah-users.toml` | `users_groups` has included users or custom groups | Blueprint TOML fragment for bootc-image-builder user/group provisioning. |
| `users/` | Users have `authorized_keys` data | SSH key staging tree (e.g., `users/home/<user>/.ssh/authorized_keys`). |
| `language-packages/` | `non_rpm_software.language_packages` has included entries | Manifest files for pip/npm/gem language package environments. |
| `unmanaged/` | `--include-unmanaged` was used and `unmanaged_files` has included entries | Files from `/opt`, `/srv`, `/usr/local` not owned by RPM or language packages. Symlinks preserved as tar symlink entries. Findings under `/srv` are bundled here too, but the Containerfile emits no `COPY` for them: it carries a comment block, and `audit-report.md` a note, saying to carry them with your `/var` strategy (restore them into `/var/srv` after deployment, or give them their own mount). |
| `repoless-packages/` | Repo-less RPMs detected with cached or uploaded `.rpm` files | RPM files for packages with no repository source. Used by `dnf localinstall` in the Containerfile. |

The scan tarball does not contain `compose/`. Compose files found on the
host are recorded in `inspection-snapshot.json`, and the refine export
writes them out (see [`inspectah refine` export](#inspectah-refine-export)).

### `/var` provisioning

The storage inspector discovers directories under `/var/lib`, `/var/log`,
and `/var/cache` to a depth of two, and works out what creates each one
on the image. A directory with an owner and mode that nothing in the
image creates is listed in `config/usr/lib/tmpfiles.d/inspectah-var.conf`,
and the Containerfile copies that file to `/usr/lib/tmpfiles.d/`. A
directory without an owner and mode gets a `RUN mkdir -p` line instead.
The first lines of a fragment look like this:

```
d /var/lib/dnf/repos 0755 root root -
d /var/lib/systemd/timers 0755 root root -
d /var/lib/containers/storage 0700 root root -
d /var/lib/containers/cache 0700 root root -
```

*Sample output from a RHEL 9.8 test host: the first four lines of
`config/usr/lib/tmpfiles.d/inspectah-var.conf` in the extracted refine
export (`tar xzf export.tar.gz`).* Later lines in the same file carry
comments naming the creator on the source host and whether the export
carries it, and a `FIXME` entry (commented out) where the export cannot
tell whether the base image creates the directory.

### Systemd unit state

The Containerfile writes the state changes the scan found under
`# === Systemd Unit State ===`, after the configuration and drop-in `COPY`
lines: a mask, an enabled or disabled unit whose state differs from its preset,
and an enabled local unit whatever its preset. A unit at its preset is recorded
in the snapshot's inventory and gets no command. Each verb (`enable`,
`disable`, `mask`) is one `RUN systemctl <verb> --` with one unit per line,
ordered by unit kind and then name. Every `systemctl` line in the Containerfile
comes from this one plan. The rules below decide what a line says. Refine shows
the same reason on the unit's row.

**Commands left out.** An `# Omitted: <unit> (<reason>)` line replaces a
command that would fail or do something the source host did not:

- an `enable` or `disable` of a local or shadowed unit whose unit file is
  excluded, or of a vendor unit with another unit file copied in at its name;
- a `mask` where a unit file is copied in at the same name, since
  `systemctl mask` fails on a regular file;
- any command for a vendor unit, or a package-owned mount, automount or swap
  unit, whose owning package is excluded or not carried. A unit stays when the
  base image ships its package or a package you kept depends on it. A local
  unit has no owning package, and a shadowed unit follows its staged file, as
  below.

**Units the image needs.** A `disable` or `mask` of `bootc-*`, `ostree-*` or
`systemd-remount-fs.service`, and a `mask` of `tmp.mount`, are never carried;
a `# FIXME:` says the image needs the unit. An `enable` of any of them is
carried.

**Package updaters.** `dnf-automatic*.timer` and
`packagekit-offline-update.service` are excluded by default, because an
image-based host is updated by a new image. Include one in refine to carry it.

**Local units.** A service, socket, path, timer, target or slice whose file is
only in `/etc/systemd/system/` is a state change when it is enabled, whatever
the presets say, and gets its `enable` when that file is copied, since the
image has the unit only if the export carries it. A disabled local unit gets
no `disable`. A mount, automount or swap unit you wrote gets advisory comments
and never a command. A fleet aggregate leaves the command out when hosts hold
different versions of the file.

**Shadowed units.** A unit whose vendor file you replaced in
`/etc/systemd/system/` is a state change only when its state differs from its
preset, as for any vendor unit. Its `enable` or `disable` is carried when the
shadow file is copied, whether or not its package is in the image, since the
image loads the `/etc` file. A shadow of a unit otherwise at its preset has no
state row and shows on its drop-in row only.

**How state is read.** The scan does not take `systemctl list-unit-files` at
its word, because it reports `enabled` for any link to a unit. It reads the
links the unit's effective `[Install]` section names, against the links under
`/etc/systemd/system/` and the links packages ship. A unit is enabled when
every named link is present and at least one is under `/etc`, and disabled
when none is under `/etc`. A unit whose `[Install]` section holds an
`UpheldBy=`, a specifier the scan does not expand or a name it does not admit
gets no `enable`, and its links are reported with that reason. A unit with
`Also=` members gets its command only when every member's links on the source
host match what the command does. Otherwise the Containerfile names each member
and the lines to add by hand.

**Template instances.** An instance such as `getty@tty2.service` gets its
`enable` only when the `/etc` configuration the scan identified for it is in
the export as the host had it. The scan identifies the paths that the unit
file and the drop-ins systemd applies to the instance (its own and its
template's) name with `%i` or `%I`, and the file read by `wg-quick@`,
`openvpn-client@` and `openvpn-server@` instances. It does not discover every
file the application reads: an `EnvironmentFile=` or other path with no
specifier, such as `/etc/sysconfig/x`, is not checked.

When an identified file is excluded, was not captured or is not in the
export, an `# Omitted:` line names the file and reason. Include an excluded
file in refine to carry the `enable`; for the other two, put the file in the
image yourself. A file a package owns and you did not change is not in the
export, but the image's package supplies it: add the `enable` by hand. A file
with a redacted secret is omitted too: provision the secret on the deployed
host and enable the unit there. An instance that keeps data under `/var` also
gets an `# Advisory:` line, since the image does not carry the data.

### `--inspect-only` mode

When `--inspect-only` is passed, the scan writes only
`inspection-snapshot.json` (to `--output` path or stdout). No tarball, no
rendered artifacts.

---

## `inspectah aggregate`

`inspectah aggregate` merges multiple scan tarballs into a single aggregate-level
snapshot and renders a combined tarball.

### Tarball naming

```
aggregate-<label>-<YYYYMMDD>-<HHMMSS>.tar.gz
```

This pattern is schematic: it has placeholders, so there is nothing to
capture. A real name from a directory input is
`aggregate-scans-20261104-234031.tar.gz`.

### Artifacts

`inspectah aggregate` calls `render_all()` on the merged snapshot, producing the
same always-written and conditional artifacts as `inspectah scan` (see
above). The following additional artifacts are aggregate-specific:

| File / Directory | Condition | Description |
|------------------|-----------|-------------|
| `schema/snapshot.schema.json` | Always written | JSON Schema placeholder for the snapshot format. |

The Containerfile receives a prepended header with aggregate metadata (host
count, baseline image, provisionality note).

### `--json-only` mode

When `--json-only` is passed, only the merged `inspection-snapshot.json` is
written (to `--output-file`, `--output-dir`, or stdout). No tarball, no
rendered artifacts.

---

## `inspectah refine` export

The refine UI export produces a **narrower** artifact set than scan, plus
two directories scan does not write. It excludes artifacts that are not
part of the approved export contract.

### Tarball structure

The Export button saves `inspectah-refine-output.tar.gz`. The archive
wraps a single prefix directory named `inspectah-<hostname>-refined/`, for
example `inspectah-rhel9-refined/`. A direct `POST /api/tarball` names the
file `inspectah-<hostname>-refined.tar.gz` instead.

### Always-written artifacts

| File | Description |
|------|-------------|
| `Containerfile` | Reflects the session's include/exclude decisions. Package groups render as individual members, not a single `dnf group install` line, unlike the preview pane. |
| `audit-report.md` | Findings and recommendations. |
| `inspection-snapshot.json` | Projected snapshot reflecting all refinement operations applied. |
| `schema/snapshot.schema.json` | JSON Schema placeholder. Scan does not write it. |

### Conditional artifacts

Same conditions as scan, except for `compose/`, which only the export writes:

| File / Directory | Condition |
|------------------|-----------|
| `subscription/` | Subscription data preserved in snapshot. |
| `config/` | Included config files exist in projected snapshot. Includes `config/usr/lib/tmpfiles.d/inspectah-var.conf` under the same conditions as scan. |
| `env-files/` | Included env files exist. |
| `quadlet/` | Included quadlet units exist. |
| `flatpak/` | Included flatpak apps exist. |
| `drop-ins/` | Included unit drop-ins exist. |
| `tuned/` | Tuned profiles included. |
| `users/` | SSH keys exist for included users. |
| `inspectah-users.ks` | Included users or custom groups exist. |
| `inspectah-users.toml` | Included users or custom groups exist. |
| `language-packages/` | Included language package manifests exist. |
| `unmanaged/` | Unmanaged files with `include: true` exist. Carried over from the scan tarball, so it is present only if the scan used `--include-unmanaged`. `/srv` findings are bundled but get no `COPY`, as in scan. |
| `repoless-packages/` | Repo-less RPMs detected (cached or uploaded). |
| `compose/` | Included compose files exist. Written by the export only. |

### RPM-modified config files

An RPM-owned config file that `rpm -Va` reports as changed in content is
included by default on both paths. The change is treated as yours, so the
file is copied into `config/` and the image, the same as the package that
owns it. A config file you did not change is left to the base image and
dnf. The scan tarball's audit report lists the copied files under
"Modified RPM-Owned Files", and an export with no decisions copies the
same files as the scan, among them `ssh/sshd_config` on the sample host.

An RPM-owned config symlink that was re-pointed to `/dev/null`, such as
a package's `/etc/sysctl.d/99-sysctl.conf` link masked by an
administrator, is treated the same way: it is copied as an empty file of
the same name, which masks the file it names just as the link did. A
package symlink re-pointed to any other file is not carried.

`rpm -Va` reports that a file changed, not who changed it. Tools such as
subscription-manager, authselect, or a realm join also rewrite RPM-owned
configs. A file copied into the image also hides later package updates to
it. Review the Configuration Files section in refine and exclude any you
do not want in the image.

An export with no decisions still differs from the scan in other ways: it
drops `epel-release`, which refine excludes by default, and enables
`sshd.service`.

### Excluded from refine export

These scan artifacts are intentionally omitted from the export contract:

- `audit-report.html`
- `README.md`
- `secrets-review.md`
- `kickstart-suggestion.ks`

The export contract is enforced by an allowlist in `render_refine_export()`
and verified by `export_contract_test.rs`.

---

## Tarball structure diagram

Each listing below is the output of `tar tzf` on a tarball from the
sample host. Each is shortened: the entries under `config/etc/` are
omitted (93 in the scan listing, 104 in the export, 106 in the aggregate).
The entries that remain are printed unchanged, except that a `sysctl/`
directory is removed from each: these tarballs come from an earlier
release, which wrote one, and inspectah no longer does. Sysctl files now
arrive under `config/etc/`, among the omitted entries, so on a current
release those counts include them.

### Scan tarball

This listing is from a scan with `--include-unmanaged`, so `unmanaged/`
appears. A default scan omits that directory. Other conditional
directories from the tables above, such as `env-files/`, `subscription/`,
and `users/`, do not appear because this scan had nothing for them or
was not asked for them.

```
rhel9-20261104-223931/
rhel9-20261104-223931/Containerfile
rhel9-20261104-223931/README.md
rhel9-20261104-223931/audit-report.html
rhel9-20261104-223931/audit-report.md
rhel9-20261104-223931/config/
rhel9-20261104-223931/config/usr/
rhel9-20261104-223931/config/usr/lib/
rhel9-20261104-223931/config/usr/lib/tmpfiles.d/
rhel9-20261104-223931/config/usr/lib/tmpfiles.d/inspectah-var.conf
rhel9-20261104-223931/drop-ins/
rhel9-20261104-223931/drop-ins/etc/
rhel9-20261104-223931/drop-ins/etc/systemd/
rhel9-20261104-223931/drop-ins/etc/systemd/system/
rhel9-20261104-223931/drop-ins/etc/systemd/system/bluetooth.service
rhel9-20261104-223931/drop-ins/etc/systemd/system/httpd.service.d/
rhel9-20261104-223931/drop-ins/etc/systemd/system/httpd.service.d/limits.conf
rhel9-20261104-223931/drop-ins/etc/systemd/system/httpd.service.d/override.conf
rhel9-20261104-223931/drop-ins/etc/systemd/system/kdump.service
rhel9-20261104-223931/drop-ins/etc/systemd/system/sshd.service
rhel9-20261104-223931/inspectah-users.ks
rhel9-20261104-223931/inspectah-users.toml
rhel9-20261104-223931/inspection-snapshot.json
rhel9-20261104-223931/kickstart-suggestion.ks
rhel9-20261104-223931/language-packages/
rhel9-20261104-223931/language-packages/npm/
rhel9-20261104-223931/language-packages/npm/23f9725da273/
rhel9-20261104-223931/language-packages/npm/23f9725da273/package-lock.json
rhel9-20261104-223931/language-packages/npm/23f9725da273/package.json
rhel9-20261104-223931/language-packages/pip/
rhel9-20261104-223931/language-packages/pip/3ee13faaf2c5/
rhel9-20261104-223931/language-packages/pip/3ee13faaf2c5/requirements.txt
rhel9-20261104-223931/language-packages/pip/77fce1b75e7a/
rhel9-20261104-223931/language-packages/pip/77fce1b75e7a/requirements.txt
rhel9-20261104-223931/quadlet/
rhel9-20261104-223931/quadlet/myapp.network
rhel9-20261104-223931/quadlet/redis.container
rhel9-20261104-223931/quadlet/webapp.container
rhel9-20261104-223931/secrets-review.md
rhel9-20261104-223931/unmanaged/
rhel9-20261104-223931/unmanaged/opt/
rhel9-20261104-223931/unmanaged/opt/legacy-app/
rhel9-20261104-223931/unmanaged/opt/legacy-app/requirements.txt
rhel9-20261104-223931/unmanaged/opt/myapp/
rhel9-20261104-223931/unmanaged/opt/myapp/docker-compose.yml
rhel9-20261104-223931/unmanaged/opt/myapp/lib
rhel9-20261104-223931/unmanaged/opt/myapp/requirements.txt
rhel9-20261104-223931/unmanaged/srv/
rhel9-20261104-223931/unmanaged/srv/www/
rhel9-20261104-223931/unmanaged/srv/www/index.html
rhel9-20261104-223931/unmanaged/usr/
rhel9-20261104-223931/unmanaged/usr/local/
rhel9-20261104-223931/unmanaged/usr/local/bin/
rhel9-20261104-223931/unmanaged/usr/local/bin/deploy.sh
rhel9-20261104-223931/unmanaged/usr/local/bin/driftify-probe
rhel9-20261104-223931/unmanaged/usr/local/bin/ng
rhel9-20261104-223931/unmanaged/usr/local/bin/pm2
rhel9-20261104-223931/unmanaged/usr/local/bin/pm2-dev
rhel9-20261104-223931/unmanaged/usr/local/bin/pm2-docker
rhel9-20261104-223931/unmanaged/usr/local/bin/pm2-runtime
rhel9-20261104-223931/unmanaged/usr/local/bin/tsc
```

*Sample output from a RHEL 9.8 test host. Command: `tar tzf
rhel9-20261104-223931.tar.gz`, from `sudo inspectah -y scan
--include-unmanaged`, with one file added under `/srv`.*

### Refine export tarball

The export has a prefix directory. It has no `README.md`,
`audit-report.html`, `secrets-review.md`, or `kickstart-suggestion.ks`,
and it adds `compose/` and `schema/snapshot.schema.json`. This export
has no `unmanaged/` because the scan it came from did not use
`--include-unmanaged`.

```
inspectah-rhel9-refined/
inspectah-rhel9-refined/Containerfile
inspectah-rhel9-refined/audit-report.md
inspectah-rhel9-refined/compose/
inspectah-rhel9-refined/compose/opt/
inspectah-rhel9-refined/compose/opt/myapp/
inspectah-rhel9-refined/compose/opt/myapp/docker-compose.yml
inspectah-rhel9-refined/config/
inspectah-rhel9-refined/config/usr/
inspectah-rhel9-refined/config/usr/lib/
inspectah-rhel9-refined/config/usr/lib/tmpfiles.d/
inspectah-rhel9-refined/config/usr/lib/tmpfiles.d/inspectah-var.conf
inspectah-rhel9-refined/drop-ins/
inspectah-rhel9-refined/drop-ins/etc/
inspectah-rhel9-refined/drop-ins/etc/systemd/
inspectah-rhel9-refined/drop-ins/etc/systemd/system/
inspectah-rhel9-refined/drop-ins/etc/systemd/system/bluetooth.service
inspectah-rhel9-refined/drop-ins/etc/systemd/system/httpd.service.d/
inspectah-rhel9-refined/drop-ins/etc/systemd/system/httpd.service.d/limits.conf
inspectah-rhel9-refined/drop-ins/etc/systemd/system/httpd.service.d/override.conf
inspectah-rhel9-refined/drop-ins/etc/systemd/system/kdump.service
inspectah-rhel9-refined/drop-ins/etc/systemd/system/sshd.service
inspectah-rhel9-refined/inspectah-users.ks
inspectah-rhel9-refined/inspectah-users.toml
inspectah-rhel9-refined/inspection-snapshot.json
inspectah-rhel9-refined/language-packages/
inspectah-rhel9-refined/language-packages/npm/
inspectah-rhel9-refined/language-packages/npm/23f9725da273/
inspectah-rhel9-refined/language-packages/npm/23f9725da273/package-lock.json
inspectah-rhel9-refined/language-packages/npm/23f9725da273/package.json
inspectah-rhel9-refined/language-packages/pip/
inspectah-rhel9-refined/language-packages/pip/3ee13faaf2c5/
inspectah-rhel9-refined/language-packages/pip/3ee13faaf2c5/requirements.txt
inspectah-rhel9-refined/language-packages/pip/77fce1b75e7a/
inspectah-rhel9-refined/language-packages/pip/77fce1b75e7a/requirements.txt
inspectah-rhel9-refined/quadlet/
inspectah-rhel9-refined/quadlet/myapp.network
inspectah-rhel9-refined/quadlet/redis.container
inspectah-rhel9-refined/quadlet/webapp.container
inspectah-rhel9-refined/schema/
inspectah-rhel9-refined/schema/snapshot.schema.json
```

*Sample output from a RHEL 9.8 test host. Command: `tar tzf
export.tar.gz`, where `export.tar.gz` is the refine download for the scan
`rhel9-20261104-222606.tar.gz`.*

### Aggregate tarball

```
aggregate-scans-20261104-234031/
aggregate-scans-20261104-234031/Containerfile
aggregate-scans-20261104-234031/README.md
aggregate-scans-20261104-234031/audit-report.html
aggregate-scans-20261104-234031/audit-report.md
aggregate-scans-20261104-234031/config/
aggregate-scans-20261104-234031/config/usr/
aggregate-scans-20261104-234031/config/usr/lib/
aggregate-scans-20261104-234031/config/usr/lib/tmpfiles.d/
aggregate-scans-20261104-234031/config/usr/lib/tmpfiles.d/inspectah-var.conf
aggregate-scans-20261104-234031/drop-ins/
aggregate-scans-20261104-234031/drop-ins/etc/
aggregate-scans-20261104-234031/drop-ins/etc/systemd/
aggregate-scans-20261104-234031/drop-ins/etc/systemd/system/
aggregate-scans-20261104-234031/drop-ins/etc/systemd/system/bluetooth.service
aggregate-scans-20261104-234031/drop-ins/etc/systemd/system/httpd.service.d/
aggregate-scans-20261104-234031/drop-ins/etc/systemd/system/httpd.service.d/limits.conf
aggregate-scans-20261104-234031/drop-ins/etc/systemd/system/httpd.service.d/override.conf
aggregate-scans-20261104-234031/drop-ins/etc/systemd/system/kdump.service
aggregate-scans-20261104-234031/drop-ins/etc/systemd/system/sshd.service
aggregate-scans-20261104-234031/inspectah-users.ks
aggregate-scans-20261104-234031/inspectah-users.toml
aggregate-scans-20261104-234031/inspection-snapshot.json
aggregate-scans-20261104-234031/kickstart-suggestion.ks
aggregate-scans-20261104-234031/language-packages/
aggregate-scans-20261104-234031/language-packages/npm/
aggregate-scans-20261104-234031/language-packages/npm/23f9725da273/
aggregate-scans-20261104-234031/language-packages/npm/23f9725da273/package-lock.json
aggregate-scans-20261104-234031/language-packages/npm/23f9725da273/package.json
aggregate-scans-20261104-234031/language-packages/pip/
aggregate-scans-20261104-234031/language-packages/pip/3ee13faaf2c5/
aggregate-scans-20261104-234031/language-packages/pip/3ee13faaf2c5/requirements.txt
aggregate-scans-20261104-234031/language-packages/pip/77fce1b75e7a/
aggregate-scans-20261104-234031/language-packages/pip/77fce1b75e7a/requirements.txt
aggregate-scans-20261104-234031/quadlet/
aggregate-scans-20261104-234031/quadlet/myapp.network
aggregate-scans-20261104-234031/quadlet/redis.container
aggregate-scans-20261104-234031/quadlet/webapp.container
aggregate-scans-20261104-234031/schema/
aggregate-scans-20261104-234031/schema/snapshot.schema.json
aggregate-scans-20261104-234031/secrets-review.md
```

*Sample output from a RHEL 9.8 test host. Command: `tar tzf
aggregate-scans-20261104-234031.tar.gz`, from `inspectah aggregate scans/`
over two scans of the sample host.*

Apart from the added `schema/snapshot.schema.json`, an aggregate tarball
has the same structure as a scan tarball.
