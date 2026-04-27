# How to Build a bootc Image from inspectah Output

After running `inspectah scan` and `inspectah refine` on a RHEL host, you have a tarball containing everything needed to rebuild that system as a bootc container image. This guide walks through building that image using `inspectah build`.

## Prerequisites

Before you start, you'll need:

1. **inspectah CLI**: Install via COPR (`sudo dnf install inspectah`) or Homebrew (`brew install marrusl/tap/inspectah`). See the [README](../../README.md#installation) for details.

2. **Podman**: Required on your build machine. The RPM package installs it as a dependency. On macOS, install via [Podman Desktop](https://podman-desktop.io/) or `brew install podman`.

3. **inspectah output tarball**: A file named like `hostname-20260312-143000.tar.gz` from the inspect/refine workflow, containing:
   - `Containerfile` — the bootc image definition
   - `config/` — configuration files to layer into the image
   - `entitlement/` and `rhsm/` — RHEL subscription certs (if building a RHEL image)

4. **RHEL subscription certificates** (for RHEL images only):
   - If building on a RHEL host, certificates are auto-detected
   - If building on Mac, Windows, or Fedora, you'll need certs from the source RHEL system (see "RHEL Subscription Cert Handling" below)

## Basic Usage

The simplest build command takes a tarball and an image tag:

```bash
inspectah build hostname-20260312-143000.tar.gz -t my-bootc-image:latest
```

You can also build from an extracted directory:

```bash
tar xzf hostname-20260312-143000.tar.gz
inspectah build hostname-20260312-143000/ -t my-bootc-image:latest
```

## What Happens During the Build

When you run `inspectah build`, it:

1. **Extracts the tarball** (if needed) and validates that a `Containerfile` exists
2. **Checks if the image needs RHEL entitlement** — scans the `Containerfile` for RHEL/UBI base images
3. **Locates subscription certificates** (for RHEL images) — see next section
4. **Runs `podman build`** with entitlement volumes mounted
5. **Reports build results** — shows the image tag on success

The actual build command looks like:

```bash
podman build \
  -f Containerfile \
  -t my-bootc-image:latest \
  -v /path/to/entitlement:/etc/pki/entitlement:ro \
  /path/to/extracted/output
```

Build output streams to your terminal in real time.

## RHEL Subscription Cert Handling

Building a RHEL-based bootc image requires valid subscription certificates so that `dnf install` can access Red Hat's package repositories during the build. `inspectah build` auto-detects and bind-mounts these certificates for you.

### How Certificate Detection Works

The tool searches for certificates in this priority order:

1. **Explicit directory** — Use `--entitlements-dir /path/to/certs` to point directly at your certificate directory.

2. **Bundled in the tarball** — If you ran `inspectah scan` on the RHEL host, certificates are automatically bundled into the tarball at `entitlement/` and `rhsm/`.

3. **RHEL host auto-detection** — If you're building on a RHEL system with `/etc/pki/entitlement/*.pem` present, certs are detected automatically.

4. **Current working directory** — If you've copied certificates to `./entitlement/*.pem` next to where you're running `inspectah build`, they'll be detected.

Use `--no-entitlements` to skip certificate detection entirely (e.g., when building Fedora or CentOS Stream images that don't need RHEL repos).

### What Gets Mounted

When certificates are found, `inspectah build` bind-mounts them read-only into the build container:

- `/etc/pki/entitlement` — subscription certificates (`.pem` files)
- `/etc/rhsm` — subscription manager config (if present alongside entitlement certs)

### Certificate Expiry Checks

`inspectah build` validates certificates before building:

- **Expired certs** trigger a warning and the build is skipped (use `--ignore-expired-certs` to override)
- **Certs expiring within 24 hours** trigger a heads-up warning but don't block the build

### Building RHEL Images on Non-RHEL Hosts

You can build RHEL bootc images on:

- **macOS** — using Podman Desktop
- **Fedora** — using native podman
- **Any Linux with podman** — as long as you have valid RHEL subscription certs

The workflow:

1. Run `inspectah scan` on the RHEL host (certs are bundled automatically)
2. Copy the tarball to your Mac/Fedora workstation
3. Run `inspectah build` — certs are auto-detected from the tarball

If you extracted the tarball and lost the bundled certs, copy them manually:

```bash
scp root@rhel-host:/etc/pki/entitlement/*.pem ./entitlement/
scp -r root@rhel-host:/etc/rhsm ./rhsm/
```

Then use `--entitlements-dir ./entitlement` or place them next to the tarball.

### What If Certificates Aren't Found?

If you're building a RHEL image and no certificates are detected, `inspectah build` will warn you and proceed. If the image actually needs RHEL repos, `dnf install` will fail during the build. Options:

1. Build on the RHEL host directly (entitlement is automatic)
2. Copy certs from the RHEL host (see above)
3. Re-run `inspectah scan` on the RHEL host — it bundles certs automatically

## Build Options

### Force rebuild without cache

```bash
inspectah build hostname-20260312-143000.tar.gz -t my-bootc-image:latest --no-cache
```

### Cross-architecture builds

```bash
inspectah build hostname-20260312-143000.tar.gz -t my-bootc-image:latest --platform linux/arm64
```

### Dry run (preview the podman command)

```bash
inspectah build hostname-20260312-143000.tar.gz -t my-bootc-image:latest --dry-run
```

### Pass extra arguments to podman

Any arguments after `--` are forwarded to `podman build`:

```bash
inspectah build hostname-20260312-143000.tar.gz -t my-bootc-image:latest -- --build-arg FOO=bar
```

## Pushing the Built Image to a Registry

After building, push manually using podman:

```bash
# Log in to your registry
podman login registry.example.com

# Tag the image
podman tag my-bootc-image:latest registry.example.com/my-bootc-image:v1.0

# Push to the registry
podman push registry.example.com/my-bootc-image:v1.0
```

## Troubleshooting

### Build fails with "repository not found" or dnf errors

This usually means RHEL subscription certificates weren't found or are expired. Check:

1. Are you building a RHEL image? Look for `FROM registry.redhat.io` in the `Containerfile`.
2. Did `inspectah build` log certificate discovery? If not, use `--entitlements-dir` to point at your certs.
3. Check cert expiry: `openssl x509 -enddate -noout -in entitlement/*.pem`

Solution: Copy fresh certs from the RHEL host as shown in "RHEL Subscription Cert Handling" above.

### Build fails with "FROM registry.redhat.io: unauthorized"

You need to authenticate to Red Hat's registry:

```bash
podman login registry.redhat.io
```

Use your Red Hat account credentials (same as access.redhat.com).

### Push fails with "not logged in to registry.example.com"

Log in to the registry before pushing:

```bash
podman login registry.example.com
```

## Next Steps

After building your bootc image:

1. **Test the image** — Use bootc-image-builder to create a VM/ISO, or deploy directly to bare metal with `bootc switch`
2. **Review the audit report** — `audit-report.md` in the tarball documents storage requirements, version drift, and configuration
3. **Check the README** — `README.md` in the tarball has build/deploy commands and a FIXME checklist for manual review items

See the [inspectah README](../../README.md) for the full workflow.
