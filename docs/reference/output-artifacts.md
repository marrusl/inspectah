---
title: Output Artifacts
parent: Reference
nav_order: 4
---

# Output Artifacts

Each inspectah command produces a defined set of files. This reference lists
every artifact by command, distinguishing always-written files from
conditional ones.

Source of truth: `inspectah-pipeline/src/render/mod.rs` (scan),
`inspectah-refine/src/session.rs` (refine export),
`inspectah-cli/src/commands/fleet.rs` (fleet aggregate).

---

## `inspectah scan`

### Tarball naming

```
<hostname>-<YYYYMMDD>-<HHMMSS>.tar.gz
```

The tarball wraps a single prefix directory with the same name as the stamp.
Naming is produced by `get_output_stamp()` in `render/tarball.rs`.

### Always-written artifacts

These files are written by `render_all()` on every scan, regardless of
system state.

| File | Description |
|------|-------------|
| `Containerfile` | Image build definition. COPY lines are derived from the materialized config tree roots. |
| `report.html` | Interactive HTML report (PatternFly). |
| `audit-report.md` | Findings, recommendations, storage plan, version drift. |
| `secrets-review.md` | Redacted sensitive content for operator review. |
| `README.md` | Summary with build commands and FIXME checklist. |
| `kickstart-suggestion.ks` | Deploy-time settings (network, storage, bootloader). |
| `inspection-snapshot.json` | Full structured snapshot (JSON). Can be re-rendered or imported into refine. |
| `config/` | Materialized config files to COPY into the image. Always created as a directory; contents are conditional on what the system has. |

### Conditional artifacts

These files or directories are only written when the corresponding data
exists in the snapshot. If a file is missing from your tarball, the scanned
system did not have the relevant configuration.

| File / Directory | Condition | Description |
|------------------|-----------|-------------|
| `config/etc/` | Config files with `include: true` exist | Modified configs, repo files, GPG keys, firewall zones, audit rules, PAM configs, kernel modules, modprobe, dracut conf. |
| `config/opt/` | Non-RPM software detected under `/opt` | Non-RPM software directories. |
| `config/usr/` | Files under `/usr/local` detected | Files under `/usr/local` (non-RPM). |
| `config/usr/lib/bootc/kargs.d/inspectah-migrated.toml` | Kernel cmdline has safe operator kargs | Kernel argument drop-in for bootc. |
| `env-files/` | `non_rpm_software.env_files` has included entries | `.env` files, separated from `config/` because they are high-probability secret carriers. |
| `quadlet/` | `containers.quadlet_units` has included entries | Quadlet unit files for container workloads. One file per unit. |
| `flatpak/flatpak-install.json` | `containers.flatpak_apps` has included entries | JSON manifest of Flatpak apps to install. |
| `flatpak/flatpak-provision.service` | `containers.flatpak_apps` has included entries | Systemd service to provision Flatpak apps on first boot. |
| `drop-ins/` | `services.drop_ins` has included entries | Systemd service drop-in overrides (e.g., `drop-ins/etc/systemd/system/<unit>.d/override.conf`). |
| `tuned/` | `kernel_boot.tuned_include` is true and custom profiles exist | Custom tuned performance profiles. |
| `sysctl/99-inspectah-migrated.conf` | `kernel_boot.sysctl_overrides` has included entries | Synthesized sysctl configuration from included overrides. |
| `subscription/` | `--preserve-subscription` used during scan | RHEL subscription material for non-RHEL builds. Contains `entitlement/` (cert/key pairs), `rhsm/ca/` (CA certs), `rhsm/rhsm.conf`, and `redhat.repo`. |
| `inspectah-users.ks` | `users_groups` has included users or custom groups | Kickstart fragment for user/group provisioning. |
| `inspectah-users.toml` | `users_groups` has included users or custom groups | Blueprint TOML fragment for bootc-image-builder user/group provisioning. |
| `users/` | Users have `authorized_keys` data | SSH key staging tree (e.g., `users/home/<user>/.ssh/authorized_keys`). |

### `--inspect-only` mode

When `--inspect-only` is passed, the scan writes only
`inspection-snapshot.json` (to `--output` path or stdout). No tarball, no
rendered artifacts.

---

## `inspectah fleet aggregate`

Fleet aggregate merges multiple scan tarballs into a single fleet-level
snapshot and renders a combined tarball.

### Tarball naming

```
fleet-<label>-<YYYYMMDD>-<HHMMSS>.tar.gz
```

### Artifacts

Fleet aggregate calls `render_all()` on the merged snapshot, producing the
same always-written and conditional artifacts as `inspectah scan` (see
above). The following additional artifacts are fleet-specific:

| File / Directory | Condition | Description |
|------------------|-----------|-------------|
| `schema/snapshot.schema.json` | Always written | JSON Schema placeholder for the snapshot format. |

The Containerfile receives a prepended header with fleet metadata (host
count, baseline image, provisionality note).

### `--json-only` mode

When `--json-only` is passed, only the merged `inspection-snapshot.json` is
written (to `--output-file`, `--output-dir`, or stdout). No tarball, no
rendered artifacts.

---

## `inspectah fleet init`

Produces a fleet manifest file, not a tarball.

| File | Description |
|------|-------------|
| `fleet.toml` (default) or `--output` path | TOML manifest listing tarball sources, baseline image, and fleet label. |

---

## `inspectah refine` export

The refine UI export produces a **narrower** artifact set than scan. It
excludes artifacts that are not part of the approved export contract.

### Tarball structure

Refine export tarballs are **flat** (no prefix subdirectory).

### Always-written artifacts

| File | Description |
|------|-------------|
| `Containerfile` | Byte-identical to the refine UI preview. Uses the same materialized roots as the preview to guarantee fidelity. |
| `audit-report.md` | Findings and recommendations. |
| `inspection-snapshot.json` | Projected snapshot reflecting all refinement operations applied. |
| `schema/snapshot.schema.json` | JSON Schema placeholder. |

### Conditional artifacts

Same conditions as scan:

| File / Directory | Condition |
|------------------|-----------|
| `subscription/` | Subscription data preserved in snapshot. |
| `config/` | Included config files exist in projected snapshot. |
| `env-files/` | Included env files exist. |
| `quadlet/` | Included quadlet units exist. |
| `flatpak/` | Included flatpak apps exist. |
| `drop-ins/` | Included service drop-ins exist. |
| `tuned/` | Tuned profiles included. |
| `sysctl/` | Included sysctl overrides exist. |
| `users/` | SSH keys exist for included users. |
| `inspectah-users.ks` | Included users or custom groups exist. |
| `inspectah-users.toml` | Included users or custom groups exist. |

### Excluded from refine export

These scan artifacts are intentionally omitted from the export contract:

- `report.html`
- `README.md`
- `secrets-review.md`
- `kickstart-suggestion.ks`

The export contract is enforced by an allowlist in `render_refine_export()`
and verified by `export_contract_test.rs`.

---

## Tarball structure diagram

### Scan tarball (full)

```
hostname-20260527-143000.tar.gz
└── hostname-20260527-143000/
    ├── Containerfile
    ├── README.md
    ├── audit-report.md
    ├── report.html
    ├── secrets-review.md
    ├── kickstart-suggestion.ks
    ├── inspection-snapshot.json
    ├── config/
    │   ├── etc/
    │   │   ├── httpd/conf/httpd.conf
    │   │   ├── yum.repos.d/*.repo
    │   │   ├── pki/rpm-gpg/*
    │   │   ├── sysctl.d/
    │   │   ├── modules-load.d/
    │   │   ├── modprobe.d/
    │   │   ├── dracut.conf.d/
    │   │   ├── systemd/system/*.timer, *.service
    │   │   ├── firewalld/zones/
    │   │   └── ...
    │   ├── opt/                          (non-RPM software)
    │   └── usr/lib/bootc/kargs.d/        (kernel arg drop-in)
    ├── subscription/                     (conditional)
    │   ├── entitlement/
    │   │   ├── 123.pem
    │   │   └── 123-key.pem
    │   ├── rhsm/
    │   │   ├── ca/redhat-uep.pem
    │   │   └── rhsm.conf
    │   └── redhat.repo
    ├── env-files/                        (conditional)
    │   └── path/to/file.env
    ├── quadlet/                          (conditional)
    │   └── <unit>.container
    ├── flatpak/                          (conditional)
    │   ├── flatpak-install.json
    │   └── flatpak-provision.service
    ├── drop-ins/                         (conditional)
    │   └── etc/systemd/system/<unit>.d/override.conf
    ├── tuned/                            (conditional)
    │   └── etc/tuned/<profile>/tuned.conf
    ├── sysctl/                           (conditional)
    │   └── 99-inspectah-migrated.conf
    ├── inspectah-users.ks                (conditional)
    ├── inspectah-users.toml              (conditional)
    └── users/                            (conditional)
        └── home/<user>/.ssh/authorized_keys
```

### Refine export tarball (flat)

```
export.tar.gz  (no prefix directory)
├── Containerfile
├── audit-report.md
├── inspection-snapshot.json
├── schema/
│   └── snapshot.schema.json
├── config/                               (conditional)
├── env-files/                            (conditional)
├── quadlet/                              (conditional)
├── flatpak/                              (conditional)
├── drop-ins/                             (conditional)
├── tuned/                                (conditional)
├── sysctl/                               (conditional)
├── inspectah-users.ks                    (conditional)
├── inspectah-users.toml                  (conditional)
└── users/                                (conditional)
```

### Fleet aggregate tarball

```
fleet-<label>-20260527-143000.tar.gz
└── fleet-<label>-20260527-143000/
    ├── (same structure as scan tarball)
    └── schema/
        └── snapshot.schema.json
```
