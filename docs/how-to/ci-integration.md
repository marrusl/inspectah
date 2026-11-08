---
title: CI Integration
parent: How-To Guides
nav_order: 5
---

# CI Integration

inspectah can run unattended in CI/CD pipelines to produce scan snapshots
as build artifacts. This guide covers the flags and patterns for
non-interactive use.

## Non-interactive output

When inspectah detects a non-TTY environment (piped output, CI runner), it
automatically selects flat progress mode. To be explicit:

```bash
sudo inspectah scan --progress flat -o ./scan-output.tar.gz
```

Flat mode prints numbered sequential lines with no ANSI escape codes,
producing clean log output in any CI system.

For minimal logging, combine with quiet mode:

```bash
sudo inspectah scan -q --progress flat -o ./scan-output.tar.gz
```

Quiet mode replaces the progress checklist with a one-line completion
summary. It does not silence the setup lines: the source detection, target
image resolution, image pull and baseline lines still print to stderr. This
is the output of that command on a RHEL 9.8 host, with the pull log
shortened:

```
Detecting source system...
  Red Hat Enterprise Linux 9.8 (Plow)
Resolving target image...
  registry.redhat.io/rhel9/rhel-bootc:9.8 (OsRelease)
Preparing baseline image registry.redhat.io/rhel9/rhel-bootc:9.8...
  pull: Trying to pull registry.redhat.io/rhel9/rhel-bootc:9.8...
  ...
Pulled registry.redhat.io/rhel9/rhel-bootc:9.8 (sha256:173c055cc9c7)
  Baseline extracted: 457 packages
  Resolved via: os-release (auto-detected)
Inspecting host rhel9...
Scan complete (30s)
Report: ./scan-output.tar.gz
To review: inspectah refine ./scan-output.tar.gz
```

## Exit codes

inspectah uses exit codes to communicate scan completeness. Use these in
CI scripts to decide whether a pipeline should continue or fail.

| Code | Meaning | CI action |
|------|---------|-----------|
| 0 | **Clean or Degraded**: Report is trustworthy | Continue pipeline |
| 1 | **Error**: Scan failed to run | Fail the build |
| 2 | **Incomplete**: An inspector failed, report has blind spots | Warn or fail depending on policy |
| 130 | **Interrupted**: User sent SIGINT (Ctrl-C) | Retry or fail |

Exit 0 covers both fully complete scans and degraded scans (where
classification data is less precise but the report is still usable). Exit 2
means at least one inspector could not collect its data, so the report may
be missing sections.

### Check exit codes in a pipeline

```bash
sudo inspectah scan --progress flat -o ./scan-output.tar.gz
exit_code=$?

if [ $exit_code -eq 0 ]; then
  echo "Scan complete, uploading artifact"
  # upload scan-output.tar.gz
elif [ $exit_code -eq 2 ]; then
  echo "Scan incomplete -- some inspectors failed"
  # decide whether to continue or fail
else
  echo "Scan failed with exit code $exit_code"
  exit 1
fi
```

## Machine-readable output

For pipelines that process scan data programmatically, use `--inspect-only`
to get JSON output instead of a tarball:

```bash
sudo inspectah scan --inspect-only -o ./snapshot/inspection-snapshot.json
```

This writes the snapshot JSON to the file you name and prints
`Output: ./snapshot/inspection-snapshot.json` when it finishes. The JSON
contains the full scan data structure and can be parsed by downstream tools.
Give `-o` a file path. A directory path, such as `-o ./snapshot/`, is not
accepted today: the scan completes and then exits with code 1 and
`failed to write output: Is a directory`.

To write JSON to stdout (for piping):

```bash
sudo inspectah scan --inspect-only 2>/dev/null
```

Redirect stderr to suppress progress output and get clean JSON on stdout.

### Extract specific data

Combine `--inspect-only` with `jq` to extract fields in a pipeline. The
snapshot has no top-level `.packages` array, so `jq '.packages | length'`
prints `0` and iterating over `.packages` fails. The package data lives under
`.rpm` and `.baseline`:

```bash
# Every package that is not part of the base image, dependencies included
sudo inspectah scan --inspect-only 2>/dev/null | jq '.rpm.packages_added | length'

# Only the packages that were installed on purpose (leaf packages)
sudo inspectah scan --inspect-only 2>/dev/null | jq '.rpm.leaf_packages | length'

# Packages in the base image
sudo inspectah scan --inspect-only 2>/dev/null | jq '.baseline.packages | length'

# Packages in the base image that the host does not have
sudo inspectah scan --inspect-only 2>/dev/null | jq '.rpm.base_image_only | length'
```

Run against the snapshot of a RHEL 9.8 host, these print:

```
182
25
457
55
```

`.rpm.packages_added` lists every installed package that is not in the base
image. `.rpm.leaf_packages` is the subset that nothing else depends on, and it
is the count the refine stats bar and the generated Containerfile use; it is
`null` when leaf classification was unavailable on the host.
`.baseline.packages` is an object keyed by `name.arch`, so use `length` for a
count and `keys` for the names. The `584 packages` figure on the scan's RPM
line counts every installed package, baseline included.

## Pre-stage baseline image in CI

If your CI environment authenticates to the registry in a setup step, pull
the base image there and run the scan against the same reference. Without
`--base-image`, the scan resolves the image from the host's own release (a
RHEL 9.8 host resolves to `rhel-bootc:9.8`), so an image pinned to another
minor, such as `9.6`, is not the one it pulls. Pass `--base-image` so the
staged image and the scan agree:

```bash
# In a setup step or image build:
sudo podman pull registry.redhat.io/rhel9/rhel-bootc:9.6

# Then run the scan against the same reference:
sudo inspectah scan \
  --base-image registry.redhat.io/rhel9/rhel-bootc:9.6 \
  --progress flat \
  -o ./scan-output.tar.gz
```

The scan uses an image already in local storage instead of pulling it, but
only under the reference it resolves, so the staged image has to be that
reference.

## Specify the base image explicitly

In CI, the auto-detection chain may not resolve correctly (for example,
the runner may not have `bootc` installed). Specify the image explicitly:

```bash
# CentOS Stream example (public registry, no auth needed)
sudo inspectah scan \
  --base-image quay.io/centos-bootc/centos-bootc:stream9 \
  --progress flat \
  -o ./scan-output.tar.gz

# RHEL example (requires registry.redhat.io authentication)
sudo inspectah scan \
  --base-image registry.redhat.io/rhel9/rhel-bootc:9.6 \
  --progress flat \
  -o ./scan-output.tar.gz
```

This ensures consistent baseline comparison regardless of the CI
environment's configuration.

## Example: GitHub Actions

The two pipeline examples below are schematic. They are configuration for
you to adapt: the download URL and the runner are placeholders, and the scan
needs root on the host it inspects.

```yaml
jobs:
  scan:
    runs-on: ubuntu-latest
    steps:
      - name: Install inspectah
        run: |
          curl -LO https://github.com/your-org/inspectah/releases/latest/download/inspectah-x86_64-unknown-linux-gnu.tar.gz
          tar xzf inspectah-x86_64-unknown-linux-gnu.tar.gz
          sudo mv inspectah /usr/local/bin/

      - name: Run scan
        run: |
          sudo inspectah scan \
            --base-image quay.io/centos-bootc/centos-bootc:stream9 \
            --progress flat \
            -o ./scan-output.tar.gz

      - name: Upload scan artifact
        uses: actions/upload-artifact@v4
        with:
          name: migration-scan
          path: ./scan-output.tar.gz
```

## Example: GitLab CI

```yaml
scan:
  stage: analyze
  script:
    - curl -LO https://github.com/your-org/inspectah/releases/latest/download/inspectah-x86_64-unknown-linux-gnu.tar.gz
    - tar xzf inspectah-x86_64-unknown-linux-gnu.tar.gz
    - sudo mv inspectah /usr/local/bin/
    - sudo inspectah scan
        --base-image quay.io/centos-bootc/centos-bootc:stream9
        --progress flat
        -o ./scan-output.tar.gz
  artifacts:
    paths:
      - scan-output.tar.gz
    expire_in: 30 days
```

## Tips

- **Always use `--progress flat`** in CI, even though inspectah auto-detects
  non-TTY environments. Being explicit avoids surprises if the CI runner
  allocates a pseudo-TTY.

- **Pin the base image tag** (e.g., `centos-bootc:stream9` not `centos-bootc:latest`)
  for reproducible comparisons across pipeline runs.

- **Use `--inspect-only`** when you only need the JSON data and want to
  skip tarball generation. This is faster and produces a smaller artifact.

- **Check exit code 2** separately from exit code 1. An incomplete scan may
  still be useful (most sections present, one inspector failed), while a
  failed scan has no usable output.
