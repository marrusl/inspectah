---
title: Build a bootc Image from inspectah Output
parent: How-To Guides
nav_order: 6
---

# Build a bootc Image from inspectah Output

After running `inspectah scan` on a host, you have a tarball containing
everything needed to rebuild that system as a bootc container image. If you
triaged the findings in `inspectah refine`, the file you downloaded from it
(`export.tar.gz` in the examples below) builds the same way. The recommended
workflow uses `inspectah build`, which handles extraction, subscription cert
mounting, and `podman build` invocation automatically. A
[manual workflow](#manual-workflow-with-podman-build) is also available.

The command output on this page comes from a RHEL 9.8 aarch64 host with
synthetic drift, building the refine export of its scan.

## Prerequisites

- An `inspectah scan` tarball (for example `hostname-20260312-143000.tar.gz`) or a refine export
- [Podman](https://podman.io/) installed on your build machine
- **RHEL only:** valid subscription certificates (see [RHEL Subscription Cert Handling](#rhel-subscription-cert-handling)). Not needed for Fedora or CentOS Stream.

## Build with `inspectah build` (recommended)

`inspectah build` extracts the tarball, validates its contents, detects
subscription requirements, and runs `podman build` with the correct flags:

```bash
inspectah build export.tar.gz --tag rhel9-refined:v1
```

This handles everything: tarball extraction, archive safety validation,
RHEL subscription cert mounting (when needed), and podman invocation. The
build context is cleaned up automatically on completion. The tag must
include a version (`rhel9-refined:v1`, not `rhel9-refined`); `inspectah build`
refuses a tag without one before it starts podman.

The podman output streams to your terminal. The end of a successful build
looks like this:

```
Lint warning: nonempty-run-tmp: Found content in runtime-only directories (/run, /tmp):
  /run/httpd
  /run/httpd/htcacheclean
  /run/rhsm
  /run/tuned

Lint warning: var-tmpfiles: Found content in /var missing systemd tmpfiles.d entries:
  d /var/lib/dnf 0755 root root - -
  d /var/lib/dnf/repos/epel-21ce1e0cf71f4502 0755 root root - -
  d /var/lib/tuned 0755 root root - -
  d /var/log/tuned 0755 root root - -
  d /var/roothome/.cache 0755 root root - -
  ...and 1359 more
Found non-directory/non-symlink files in /var:
  var/lib/dnf/repos/epel-21ce1e0cf71f4502/countme
  var/lib/rhsm/cache/productid_repo_mapping.json
  var/lib/rhsm/productid.js
  var/lib/rhsm/repo_server_val/redhat.repo
  var/cache/ldconfig/aux-cache
  ...and 826 more

Checks passed: 11
Checks skipped: 1
Warnings: 2
COMMIT rhel9-refined:v1
--> 847a9fc95111
Successfully tagged localhost/rhel9-refined:v1
847a9fc951113875767df1f2d9f8e040f40755bde869fc6e1a140552b491643d
✓ Build succeeded: rhel9-refined:v1
  Digest: sha256:847a9fc951113875767df1f2d9f8e040f40755bde869fc6e1a140552b491643d
```

The `bootc container lint` step at the end of the Containerfile reports a
pass, skip, or warning count. Warnings do not fail the build. Here the lint
passed 11 checks, skipped 1, and printed 2 warnings.

### RHEL subscription handling

`inspectah build` automatically detects whether subscription certificates
are needed:

- **Building on a subscribed RHEL host:** uses the host's ambient
  subscription via RHEL pass-through. No extra flags needed.
- **Building on macOS, Fedora, or CI:** mounts subscription material from
  the tarball's `subscription/` directory. This requires a scan tarball made
  with `--preserve subscription`. A refine export has no `subscription/`
  directory, so build an export on a subscribed RHEL host, or build the
  scan tarball instead:

```bash
# On the RHEL host: preserve subscription material
sudo inspectah scan --preserve subscription --ack-sensitive

# On your workstation: build with the bundled certs
inspectah build hostname-*.tar.gz --tag my-bootc-image:v1
```

### Preview before building

Use `--dry-run` to see the exact podman command without executing it:

```bash
inspectah build export.tar.gz --tag rhel9-refined:v1 --dry-run
```

```
Dry run — would execute:
cd /tmp/.tmpLGDd53
/usr/bin/podman build -t \
  rhel9-refined:v1 --iidfile /tmp/.tmpLGDd53/.inspectah-iidfile -f Containerfile .
```

### Keep the build context

By default, the extracted build context is cleaned up after the build. To
keep it for inspection:

```bash
inspectah build export.tar.gz --tag rhel9-refined:v1 --keep-context
```

The extracted directory is written to `~/.cache/inspectah/builds/`, in a
subdirectory named `build-` plus the process ID. `inspectah build` does not
print that path, so list the directory to find it.

### Pass flags to podman

Pass additional arguments to `podman build` after `--`:

```bash
inspectah build export.tar.gz --tag rhel9-refined:v1 -- --no-cache --platform linux/arm64
```

## Manual workflow with `podman build`

If you prefer manual control or need to edit the Containerfile before
building, use the manual workflow.

### Extract the tarball

```bash
tar xzf export.tar.gz
cd inspectah-rhel9-refined/
```

A refine export extracts to `inspectah-<hostname>-refined/`. A scan tarball
extracts to `<hostname>-<timestamp>/`, for example
`hostname-20260312-143000/`.

The extracted directory contains:

| Path | Description |
|------|-------------|
| `Containerfile` | The bootc image definition generated by inspectah |
| `config/` | Configuration files to layer into the image |
| `subscription/` | RHEL subscription certificates and config (scan tarball only, when scanned with `--preserve subscription`) |
| `audit-report.md` | Storage, drift, and configuration audit |
| `README.md` | Build/deploy commands and FIXME checklist (scan tarball only) |

### Review the Containerfile

Before building, review the generated Containerfile for accuracy:

```bash
cat Containerfile
```

Look for:

- **Base image:** confirm the `FROM` line references the correct bootc base image for your distro.
- **Package list:** check that `dnf install` lines include the packages you expect. Remove anything unnecessary.
- **FIXME comments:** inspectah flags items that need manual review. Search for `FIXME` and resolve each one before building.
- **Configuration files:** `COPY` lines should reference files present in the `config/` directory.
- **Repo-enabling packages:** a package that enables a repository, such as `epel-release`, installs by name only when the repository that serves it is carried in the export. Otherwise the Containerfile has no install line for it, and a `FIXME` names the repository to re-include or the RPM to upload (see [Handle Repo-less RPMs](repoless-rpms.md)). When the source host had CodeReady Builder enabled, the package, group, module-stream and repo-enabling `dnf` transactions carry `--enablerepo=<id>` for it, with one comment that says why.
- **SELinux block:** the Containerfile installs `policycoreutils-python-utils`, replays each file context rule from the source host as a `semanage fcontext` command, and adds a `/var/srv` equivalence when `/srv` is a symlink to `/var/srv`. It emits no `restorecon`: the rules are applied against the target base image's policy at build time, and labels are computed from that policy when bootc pulls the image. Here is the start of that block:

```
# === Security & Access Control ===
# SELinux file contexts (1 custom rule). Labels for image content are computed
# when bootc pulls the image, from the image's own policy under /etc/selinux, so
# a rule only needs to be in the policy store: no restorecon at build time. Each
# rule below is either a command or a FIXME saying why it could not be one. Data
# you restore into /var (/var/srv included) keeps the labels it arrived with;
# run restorecon -R on it before starting the services that use it.
# The file context and port rules below need semanage, which this base does not
# carry; policycoreutils-python-utils provides it.
RUN dnf install -y policycoreutils-python-utils \
    && dnf clean all \
    && rm -rf \
        /var/cache/dnf \
        /var/lib/dnf/history* \
        /var/log/dnf* \
        /var/log/hawkey.log \
        /var/log/rhsm
```

The Containerfile is longer than the block above; this listing is shortened to
the start of the SELinux section and shows its lines unchanged. After the
build, `semanage fcontext -l -C` inside the image lists the replayed rule and
the equivalence:

```bash
podman run --rm localhost/rhel9-refined:v1 semanage fcontext -l -C
```

```
SELinux fcontext                                   type               Context

/srv/www(/.*)?                                     all files          system_u:object_r:httpd_sys_content_t:s0 

SELinux Local fcontext Equivalence 

/var/srv = /srv
```

**`/var` directories.** The scan records directories under `/var/lib`,
`/var/log` and `/var/cache` and checks whether something in the image will
create each one: a package, a `StateDirectory=`, `CacheDirectory=` or
`LogsDirectory=` unit directive, or a tmpfiles.d fragment the export carries.
A directory is `backed: creator carried by this export` when one does, and the
export adds nothing for it. An `unbacked: provisioned by this export`
directory has no creator, so the export provisions it: a `d` line in
`config/usr/lib/tmpfiles.d/inspectah-var.conf`, which the Containerfile copies
to `/usr/lib/tmpfiles.d/`, or a `RUN mkdir -p` line when the scan did not
read the directory's mode. When the scan can neither place the creator nor
rule it out, the directory is `backing unconfirmed`, and the Containerfile
carries a `FIXME` with the `d` line commented out for you to decide:

```
# === /var directory provisioning ===
# /var directories with known ownership/mode provisioned via tmpfiles.d
COPY config/usr/lib/tmpfiles.d/inspectah-var.conf /usr/lib/tmpfiles.d/inspectah-var.conf
# FIXME: confirm the base image creates /var/lib/myapp; on the host myapp.service created it and no package owns that unit; if the base does not, uncomment the next line
#d /var/lib/myapp 0755 root root -
```

The refine Storage view shows the same outcome for each directory before you
export. See [Review and Refine Findings](review-and-refine.md).

### Build the image

Run `podman build` from the extracted output directory:

```bash
podman build -f Containerfile -t my-bootc-image:latest .
```

This works directly for Fedora and CentOS Stream images, whose packages are
available from public repositories.

**For RHEL images**, mount subscription certificates into the build so `dnf`
can access Red Hat's package repositories. `podman build` requires an
absolute host path for each `-v` mount, so use `$(pwd)` or a full path:

```bash
podman build \
  -f Containerfile \
  -t my-bootc-image:latest \
  -v "$(pwd)/subscription/entitlement:/etc/pki/entitlement:ro" \
  -v "$(pwd)/subscription/rhsm:/etc/rhsm:ro" \
  .
```

A relative path such as `./subscription/entitlement` fails before the build
starts:

```
Error: validating volumes: invalid host path, must be an absolute path "./subscription/entitlement"
```

Build output streams to your terminal. A successful build ends with the
`Successfully tagged` line and the image ID.

### Force rebuild without cache

To rebuild from scratch, ignoring cached layers:

```bash
podman build --no-cache -f Containerfile -t my-bootc-image:latest .
```

## RHEL Subscription Cert Handling

Building a RHEL-based bootc image requires valid subscription certificates so
that `dnf install` can access Red Hat's package repositories during the build.

### Where certificates come from

When you scan a RHEL host with `--preserve subscription`, inspectah collects
subscription material and stores it in the tarball under `subscription/`.
The layout is illustrative: the entitlement file names carry a serial number
that differs on every host, and `123` stands in for it.

```
subscription/
├── entitlement/          # Cert/key pairs (e.g., 123.pem, 123-key.pem)
├── rhsm/
│   ├── ca/               # CA certificates (redhat-uep.pem, redhat-entitlement-authority.pem)
│   └── rhsm.conf         # RHSM configuration
└── redhat.repo           # Red Hat repo definition
```

These are the same certificates found on the host at:

- `/etc/pki/entitlement/*.pem`: entitlement certificate and key pairs
- `/etc/rhsm/ca/`: CA certificates
- `/etc/rhsm/rhsm.conf`: subscription manager configuration
- `/etc/yum.repos.d/redhat.repo`: Red Hat repository definition

Without `--preserve subscription`, subscription material is not collected.

### Using `inspectah build` (recommended)

`inspectah build` handles subscription cert mounting automatically. It detects
whether the build host has an ambient RHEL subscription and either uses
pass-through or mounts the tarball's bundled certs:

```bash
inspectah build export.tar.gz --tag rhel9-refined:v1
```

No manual `-v` flags needed.

### Manual cert mounting

If using the manual `podman build` workflow, mount the subscription
directories as read-only bind mounts, with absolute host paths:

```bash
podman build \
  -f Containerfile \
  -t my-bootc-image:latest \
  -v "$(pwd)/subscription/entitlement:/etc/pki/entitlement:ro" \
  -v "$(pwd)/subscription/rhsm:/etc/rhsm:ro" \
  .
```

### Building on a RHEL host

If you're building directly on a subscribed RHEL system, podman can use the
host's live certificates via RHEL pass-through. `inspectah build` detects
this automatically. For manual builds, mount the host's certificates:

```bash
podman build \
  -f Containerfile \
  -t my-bootc-image:latest \
  -v /etc/pki/entitlement:/etc/pki/entitlement:ro \
  -v /etc/rhsm:/etc/rhsm:ro \
  .
```

### Building RHEL images on non-RHEL hosts

You can build RHEL bootc images on macOS (via Podman Desktop), Fedora, or any
Linux with podman, as long as you have valid RHEL subscription certs.

The workflow:

1. Run `inspectah scan --preserve subscription --ack-sensitive` on the RHEL host
2. Copy the tarball to your workstation
3. Build with `inspectah build` or extract and mount the bundled certs manually

If you extracted the tarball and lost the bundled certs, copy them from the
source RHEL host. This block is illustrative: `rhel-host` is a placeholder for
your own host.

```bash
scp root@rhel-host:/etc/pki/entitlement/*.pem ./subscription/entitlement/
scp -r root@rhel-host:/etc/rhsm ./subscription/rhsm/
```

### Checking certificate expiry

RHEL subscription certificates expire. `inspectah build` checks expiry
automatically and warns when certificates are expired or expiring within
14 days.

For manual checks, name the certificate file, not the `*.pem` glob. The glob
also matches the `-key.pem` file next to each certificate, and `openssl x509`
fails with `x509: Extra option` when it gets two files. The file name is the
entitlement serial from your host; `123.pem` is a placeholder, so this block is
illustrative:

```bash
openssl x509 -enddate -noout -in subscription/entitlement/123.pem
```

If certificates are expired, re-register the source host with
`subscription-manager` and re-run `inspectah scan --preserve subscription`
to get fresh certs.

### Fedora and CentOS Stream

For Fedora and CentOS Stream images, no subscription certificates are
needed. Run `inspectah build` or `podman build` without subscription flags:

```bash
inspectah build export.tar.gz --tag rhel9-refined:v1
```

## Cross-Architecture Builds

To build for a different architecture than your host, use `--platform`. This
example builds for x86_64 from an aarch64 host. It was not run for this page,
because the build host was aarch64 and the emulation package was not installed.

```bash
podman build \
  --platform linux/amd64 \
  -f Containerfile \
  -t my-bootc-image:amd64 \
  .
```

Common platform values:

| Platform | Use case |
|----------|----------|
| `linux/amd64` | Standard x86_64 servers and cloud instances |
| `linux/arm64` | ARM servers, AWS Graviton, Apple Silicon VMs |

Cross-architecture builds require QEMU user-mode emulation. On Fedora/RHEL:

```bash
sudo dnf install qemu-user-static
```

On macOS with Podman Desktop, QEMU emulation is included automatically.

## Push to a Registry

After building, tag and push the image to your container registry. The
registry name is a placeholder, and pushing publishes an image, so this block
is illustrative:

```bash
# Log in to your registry
podman login registry.example.com

# Tag the image for your registry
podman tag my-bootc-image:latest registry.example.com/my-bootc-image:v1.0

# Push to the registry
podman push registry.example.com/my-bootc-image:v1.0
```

**For RHEL users:** authenticate with your Red Hat account to push to
Red Hat's registry:

```bash
podman login registry.redhat.io
```

## Troubleshooting

### dnf errors during build ("repository not found")

If building a RHEL-based image, subscription certificates may be missing or
expired. Check:

1. Is the Containerfile using a RHEL base image (`FROM registry.redhat.io/...`)?
   If so, you need subscription certs mounted.
2. Did you pass the `-v` flags to mount `entitlement/` and `rhsm/`, with absolute host paths?
3. Are the certificates still valid? Run `openssl x509 -enddate -noout -in subscription/entitlement/123.pem`, naming your certificate file.

Fix: Mount valid certificates as shown in [RHEL Subscription Cert Handling](#rhel-subscription-cert-handling).

For Fedora and CentOS Stream images, this error typically means the base
image's repository configuration is incorrect or network access is blocked.

### "unauthorized" pulling the base image

RHEL base images require registry authentication:

```bash
podman login registry.redhat.io
```

Use your Red Hat account credentials (the same as [access.redhat.com](https://access.redhat.com)).

Fedora and CentOS Stream base images are on public registries and do not
require authentication.

### Registry push fails ("not logged in")

Log in to the target registry before pushing:

```bash
podman login registry.example.com
```

### Missing packages in the built image

Review the generated Containerfile. inspectah captures the package list from
the source system, but some packages may not be available in the target base
image's repositories. Check `FIXME` comments in the Containerfile for flagged
items.

### Build context too large or slow

If the extracted output directory is large, podman sends the full context to
the build daemon. Use a `.containerignore` file to exclude unnecessary files.
This file is one you create, so the block is illustrative:

```
audit-report.md
README.md
*.tar.gz
```

## Next Steps

After building your bootc image:

1. **Test the image**: use [bootc-image-builder](https://github.com/containers/bootc-image-builder) to create a VM or ISO, or deploy directly to bare metal with `bootc switch`.
2. **Review the audit report**: `audit-report.md` in the tarball documents storage requirements, version drift, and configuration decisions.
3. **Check the README**: `README.md` in a scan tarball includes a FIXME checklist for manual review items. A refine export has no `README.md`.
