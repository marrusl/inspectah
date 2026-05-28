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

This suppresses the progress checklist and prints only the completion
summary.

## Exit codes

inspectah uses exit codes to communicate scan completeness. Use these in
CI scripts to decide whether a pipeline should continue or fail.

| Code | Meaning | CI action |
|------|---------|-----------|
| 0 | **Clean or Degraded** -- Report is trustworthy | Continue pipeline |
| 1 | **Error** -- Scan failed to run | Fail the build |
| 2 | **Incomplete** -- An inspector failed, report has blind spots | Warn or fail depending on policy |
| 130 | **Interrupted** -- User sent SIGINT (Ctrl-C) | Retry or fail |

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
sudo inspectah scan --inspect-only -o ./snapshot/
```

This writes a `inspection-snapshot.json` file to the specified directory.
The JSON contains the full scan data structure and can be parsed by
downstream tools.

To write JSON to stdout (for piping):

```bash
sudo inspectah scan --inspect-only 2>/dev/null
```

Redirect stderr to suppress progress output and get clean JSON on stdout.

### Extract specific data

Combine `--inspect-only` with `jq` to extract fields in a pipeline:

```bash
# Count packages found on the host
sudo inspectah scan --inspect-only 2>/dev/null | jq '.packages | length'

# List non-baseline packages
sudo inspectah scan --inspect-only 2>/dev/null | \
  jq '[.packages[] | select(.baseline_match == false)] | length'
```

## Skip baseline in CI

If your pipeline only needs a host inventory without package classification,
skip the container image pull:

```bash
sudo inspectah scan --no-baseline --progress flat -o ./scan-output.tar.gz
```

This avoids the network dependency on a container registry, which can
speed up CI runs and avoid authentication issues in restricted
environments. The trade-off is degraded classification -- all packages
get provisional triage data.

## Specify the base image explicitly

In CI, the auto-detection chain may not resolve correctly (for example,
the runner may not have `bootc` installed). Specify the image explicitly:

```bash
sudo inspectah scan \
  --base-image registry.redhat.io/rhel9/rhel-bootc:9.6 \
  --progress flat \
  -o ./scan-output.tar.gz
```

This ensures consistent baseline comparison regardless of the CI
environment's configuration.

## Example: GitHub Actions

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
            --base-image registry.redhat.io/rhel9/rhel-bootc:9.6 \
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
        --base-image registry.redhat.io/rhel9/rhel-bootc:9.6
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

- **Pin the base image tag** (e.g., `rhel-bootc:9.6` not `rhel-bootc:latest`)
  for reproducible comparisons across pipeline runs.

- **Use `--inspect-only`** when you only need the JSON data and want to
  skip tarball generation. This is faster and produces a smaller artifact.

- **Check exit code 2** separately from exit code 1. An incomplete scan may
  still be useful (most sections present, one inspector failed), while a
  failed scan has no usable output.
