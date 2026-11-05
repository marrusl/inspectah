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
| `config/etc/` | Config files with `include: true` exist | Modified configs, repo files, GPG keys, firewall zones, audit rules, PAM configs, kernel modules, modprobe, dracut conf. |
| `config/usr/lib/bootc/kargs.d/inspectah-migrated.toml` | Kernel cmdline has safe operator kargs | Kernel argument drop-in for bootc. |
| `config/usr/lib/tmpfiles.d/inspectah-var.conf` | The storage inspector found `/var` directories the image must provision, and they carry an owner and mode | tmpfiles.d fragment that creates those directories. See [`/var` provisioning](#var-provisioning). |
| `env-files/` | `non_rpm_software.env_files` has included entries | `.env` files, separated from `config/` because they are high-probability secret carriers. |
| `quadlet/` | `containers.quadlet_units` has included entries | Quadlet unit files for container workloads. One file per unit. |
| `flatpak/flatpak-install.json` | `containers.flatpak_apps` has included entries | JSON manifest of Flatpak apps to install. |
| `flatpak/flatpak-provision.service` | `containers.flatpak_apps` has included entries | Systemd service to provision Flatpak apps on first boot. |
| `drop-ins/` | `services.drop_ins` has included entries | Systemd service drop-in overrides (e.g., `drop-ins/etc/systemd/system/<unit>.d/override.conf`). |
| `tuned/` | `kernel_boot.tuned_include` is true and custom profiles exist | Custom tuned performance profiles. |
| `sysctl/99-inspectah-migrated.conf` | `kernel_boot.sysctl_overrides` has included entries | Synthesized sysctl configuration from included overrides. |
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
| `drop-ins/` | Included service drop-ins exist. |
| `tuned/` | Tuned profiles included. |
| `sysctl/` | Included sysctl overrides exist. |
| `users/` | SSH keys exist for included users. |
| `inspectah-users.ks` | Included users or custom groups exist. |
| `inspectah-users.toml` | Included users or custom groups exist. |
| `language-packages/` | Included language package manifests exist. |
| `unmanaged/` | Unmanaged files with `include: true` exist. Carried over from the scan tarball, so it is present only if the scan used `--include-unmanaged`. `/srv` findings are bundled but get no `COPY`, as in scan. |
| `repoless-packages/` | Repo-less RPMs detected (cached or uploaded). |
| `compose/` | Included compose files exist. Written by the export only. |

### RPM-modified config files

The two paths render different sets of RPM-modified config files from the
same snapshot. The scan tarball leaves RPM-modified configs out of
`config/`: the snapshot records them, but they are not copied into the
image. The refine export includes them by default, because refine treats site-specific
configs as included until you exclude them. On the sample host the scan
tarball's Containerfile header reads `47 config file(s) captured`. An
export with no decisions reads `55`: the scan's 47 plus the eight
RPM-modified configs, among them `ssh/sshd_config`. Such an export also
drops `epel-release`, which refine excludes by default, and enables
`sshd.service`. Review the Configuration Files
section in refine before you build an image from the export.

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
The entries that remain are printed unchanged.

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
rhel9-20261104-223931/sysctl/
rhel9-20261104-223931/sysctl/etc/
rhel9-20261104-223931/sysctl/etc/sysctl.d/
rhel9-20261104-223931/sysctl/etc/sysctl.d/99-inspectah-migrated.conf
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
inspectah-rhel9-refined/sysctl/
inspectah-rhel9-refined/sysctl/etc/
inspectah-rhel9-refined/sysctl/etc/sysctl.d/
inspectah-rhel9-refined/sysctl/etc/sysctl.d/99-inspectah-migrated.conf
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
aggregate-scans-20261104-234031/sysctl/
aggregate-scans-20261104-234031/sysctl/etc/
aggregate-scans-20261104-234031/sysctl/etc/sysctl.d/
aggregate-scans-20261104-234031/sysctl/etc/sysctl.d/99-inspectah-migrated.conf
```

*Sample output from a RHEL 9.8 test host. Command: `tar tzf
aggregate-scans-20261104-234031.tar.gz`, from `inspectah aggregate scans/`
over two scans of the sample host.*

Apart from the added `schema/snapshot.schema.json`, an aggregate tarball
has the same structure as a scan tarball.
