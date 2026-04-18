# How to Build a bootc Image from inspectah Output

After running `inspectah inspect` and `inspectah refine` on a RHEL host, you have a tarball containing everything needed to rebuild that system as a bootc container image. This guide walks through building that image using the `inspectah-build` tool.

## Prerequisites

Before you start, you'll need:

1. **A container runtime**: Either Podman or Docker installed on your build machine
   - Install Podman: https://podman.io/docs/installation
   - Install Docker: https://docs.docker.com/engine/install/

2. **inspectah output tarball**: A file named like `hostname-20260312-143000.tar.gz` from the inspect/refine workflow, containing:
   - `Containerfile` — the bootc image definition
   - `config/` — configuration files to layer into the image
   - `entitlement/` and `rhsm/` — RHEL subscription certs (if building a RHEL image)

3. **Python 3.9 or later**: The `inspectah-build` script is pure Python with no external dependencies

4. **RHEL subscription certificates** (for RHEL images only):
   - If building on a RHEL host, certificates are auto-detected
   - If building on Mac, Windows, or Fedora, you'll need certs from the source RHEL system (see "RHEL Subscription Cert Handling" below)

## Getting inspectah-build

Download the build script:

```bash
curl -fsSL -o inspectah-build https://raw.githubusercontent.com/marrusl/inspectah/main/inspectah-build
chmod +x inspectah-build
```

The script stays on disk for future builds.

## Basic Usage

The simplest build command takes a tarball and an image name:

```bash
./inspectah-build hostname-20260312-143000.tar.gz my-bootc-image:latest
```

You can also build from an extracted directory:

```bash
tar xzf hostname-20260312-143000.tar.gz
./inspectah-build hostname-20260312-143000/ my-bootc-image:latest
```

If you don't provide an image name, `inspectah-build` will prompt for one interactively (when running in a terminal).

## What Happens During the Build

When you run `inspectah-build`, it:

1. **Extracts the tarball** (if needed) and validates that a `Containerfile` exists
2. **Detects the container runtime** — looks for `podman` first, falls back to `docker`
3. **Checks if the image needs RHEL entitlement** — scans the `Containerfile` for `FROM registry.redhat.io`
4. **Locates subscription certificates** (for RHEL images) — see next section
5. **Runs the container build** — executes `podman build` or `docker build` with entitlement volumes mounted
6. **Reports build results** — shows image ID and size

The actual build command looks like:

```bash
podman build \
  -f Containerfile \
  -t my-bootc-image:latest \
  -v /path/to/entitlement:/etc/pki/entitlement:ro,Z \
  -v /path/to/rhsm:/etc/rhsm:ro,Z \
  /path/to/extracted/output
```

Build output streams to your terminal in real time, just like a manual `podman build`.

## RHEL Subscription Cert Handling

Building a RHEL-based bootc image requires valid subscription certificates so that `dnf install` can access Red Hat's package repositories during the build. `inspectah-build` auto-detects and bind-mounts these certificates for you.

### How Certificate Detection Works

The script searches for certificates in this priority order:

1. **RHEL host auto-detection** — If you're building on a RHEL system with `/etc/pki/entitlement/*.pem` present, podman handles entitlement automatically. No explicit mount needed.

2. **Bundled in the tarball** — If you ran `inspectah inspect` on the RHEL host using `run-inspectah.sh`, certificates are automatically bundled into the tarball at `entitlement/` and `rhsm/`.

3. **Current working directory** — If you've copied certificates to `./entitlement/*.pem` and optionally `./rhsm/` next to where you're running `inspectah-build`, they'll be detected.

4. **INSPECTAH_ENTITLEMENT environment variable** — Set this to a directory containing `*.pem` files:
   ```bash
   export INSPECTAH_ENTITLEMENT=/path/to/entitlement
   ./inspectah-build hostname-20260312-143000.tar.gz my-bootc-image:latest
   ```

### What Gets Mounted

When certificates are found, `inspectah-build` bind-mounts them read-only into the build container:

- `/etc/pki/entitlement` — subscription certificates (`.pem` files)
- `/etc/rhsm` — subscription manager config (if present alongside entitlement certs)

The `:ro,Z` flags mean:
- `:ro` — read-only mount (build can't modify your host certs)
- `:Z` — SELinux relabeling for rootless podman compatibility

This makes the certificates available inside the container during `RUN dnf install` commands, exactly as if the build were running on the original RHEL host.

### Certificate Expiry Checks

If `openssl` is available, `inspectah-build` validates certificates before building:

- **Expired certs** trigger a warning with instructions to copy fresh certs from the RHEL host
- **Certs expiring within 24 hours** trigger a heads-up warning but don't block the build
- Expiry date is logged for transparency

### Building RHEL Images on Non-RHEL Hosts

This is the primary reason `inspectah-build` exists. You can build RHEL bootc images on:

- **macOS** — using Podman Desktop or Docker Desktop
- **Windows** — using Podman Desktop, Docker Desktop, or WSL2
- **Fedora** — using native podman
- **Any Linux with podman/docker** — as long as you have valid RHEL subscription certs

The workflow:

1. Run `inspectah inspect` on the RHEL host (certs are bundled automatically)
2. Copy the tarball to your Mac/Windows/Fedora workstation
3. Run `inspectah-build` — certs are auto-detected from the tarball

If you extracted the tarball and lost the bundled certs, or if you're working from an older inspectah output that didn't bundle them, copy them manually:

```bash
scp root@rhel-host:/etc/pki/entitlement/*.pem ./entitlement/
scp -r root@rhel-host:/etc/rhsm ./rhsm/
```

Then `inspectah-build` will find them in your current directory.

### What If Certificates Aren't Found?

If you're building a RHEL image and no certificates are detected, you'll see:

```
⚠ RHEL entitlement certificates not found.
  If the build fails, options:
    1. Build on the RHEL host directly (entitlement is automatic)
    2. Copy certs from the RHEL host:
         scp root@rhel-host:/etc/pki/entitlement/*.pem ./entitlement/
         scp -r root@rhel-host:/etc/rhsm ./rhsm/
    3. Re-run run-inspectah.sh — it bundles certs automatically.
```

The build will proceed anyway (in case you're building a Fedora-based image), but if it's a RHEL image, `dnf install` will fail when it can't access Red Hat's repos.

## Build Options

### Force rebuild without cache

By default, container builds reuse cached layers. To force a clean rebuild:

```bash
./inspectah-build hostname-20260312-143000.tar.gz my-bootc-image:latest --no-cache
```

This passes `--no-cache` to the underlying `podman build` or `docker build` command.

### Build and push in one command

To push the built image to a registry immediately after building:

```bash
./inspectah-build hostname-20260312-143000.tar.gz my-bootc-image \
  --push registry.example.com/my-bootc-image:v1.0
```

The script:
1. Builds the image with the tag `my-bootc-image:latest` (or whatever you specified)
2. Checks if you're logged in to `registry.example.com`
3. Prompts for login if needed (in an interactive terminal)
4. Tags the image as `registry.example.com/my-bootc-image:v1.0`
5. Pushes to the registry

## Pushing the Built Image to a Registry

If you didn't use `--push` during the build, you can push manually afterward using your container runtime directly:

```bash
# Log in to your registry
podman login registry.example.com

# Tag the image
podman tag my-bootc-image:latest registry.example.com/my-bootc-image:v1.0

# Push to the registry
podman push registry.example.com/my-bootc-image:v1.0
```

Or use `inspectah-build` to do it for you after the fact:

```bash
./inspectah-build hostname-20260312-143000.tar.gz my-bootc-image \
  --push registry.example.com/my-bootc-image:v1.0
```

This will detect that the image already exists locally and skip the build, going straight to tag-and-push.

## Troubleshooting

### Build fails with "repository not found" or dnf errors

This usually means RHEL subscription certificates weren't found or are expired. Check:

1. Are you building a RHEL image? Look for `FROM registry.redhat.io` in the `Containerfile`.
2. Did `inspectah-build` log "using bundled/cwd/INSPECTAH_ENTITLEMENT entitlement certs"? If not, certs weren't detected.
3. Run `openssl x509 -enddate -noout -in entitlement/*.pem` to verify cert expiry.

Solution: Copy fresh certs from the RHEL host as shown in "RHEL Subscription Cert Handling" above.

### Build fails with "FROM registry.redhat.io: unauthorized"

You need to authenticate to Red Hat's registry:

```bash
podman login registry.redhat.io
```

Use your Red Hat account credentials (same as access.redhat.com).

### "neither podman nor docker found"

Install a container runtime:
- Podman: https://podman.io/docs/installation
- Docker: https://docs.docker.com/engine/install/

### Push fails with "not logged in to registry.example.com"

If running non-interactively (e.g., in CI), log in before running `inspectah-build`:

```bash
podman login registry.example.com
./inspectah-build ... --push registry.example.com/...
```

## Next Steps

After building your bootc image:

1. **Test the image** — Use bootc-image-builder to create a VM/ISO, or deploy directly to bare metal with `bootc switch`
2. **Review the audit report** — `audit-report.md` in the tarball documents storage requirements, version drift, and configuration
3. **Check the README** — `README.md` in the tarball has build/deploy commands and a FIXME checklist for manual review items

See the [inspectah README](../../README.md) for the full workflow.
