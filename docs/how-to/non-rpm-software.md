---
title: Handle Non-RPM Software
parent: How-To Guides
nav_order: 7
---

# Handle Non-RPM Software

inspectah detects pip, npm, and gem packages alongside unmanaged files
that live outside the RPM package manager. This guide covers how the
detection works, what ends up in the Containerfile, and how to adjust
the results.

## Language packages (Tier 1)

Language packages are detected automatically during every scan. No
extra flags required.

### What gets detected

| Ecosystem | Detection method | What inspectah looks for |
|-----------|-----------------|--------------------------|
| pip | Virtual environments | `pyvenv.cfg` files under `/opt`, `/srv`, `/usr/local` |
| pip | System packages | `pip list --format=json`, filtered against RPM-owned packages |
| npm | Lockfile projects | `package-lock.json` files under scan roots |
| npm | Manifest-only projects | `package.json` when no lockfile is present |
| gem | Lockfile projects | `Gemfile.lock` files under scan roots |
| gem | System gems | `gem list --local`, filtered against RPM-owned gems |

### Confidence levels

Each detected environment gets a confidence level that controls how it
renders in the Containerfile:

| Level | Meaning | Containerfile rendering |
|-------|---------|------------------------|
| **High** | Lockfile or `requirements.txt` present; full dependency list | Active `RUN` directives |
| **Medium** | Packages detected via `dist-info` or runtime command; list may be incomplete | Commented-out directives (`# RUN ...`) |
| **Low** | Manifest-only (e.g., `package.json` without lockfile); no pinned versions | Advisory comment only |

High-confidence items run out of the box. Medium-confidence items need
you to uncomment them after verifying the package list. Low-confidence
items are informational only -- you write the install commands yourself.

### Containerfile output

For a high-confidence pip venv at `/opt/myapp/venv`:

```dockerfile
# --- pip packages: /opt/myapp/venv (from requirements.txt) ---
COPY language-packages/pip/opt-myapp-venv/requirements.txt /tmp/myapp-venv-requirements.txt
RUN python3 -m venv /opt/myapp/venv \
    && /opt/myapp/venv/bin/pip install -r /tmp/myapp-venv-requirements.txt \
    && rm /tmp/myapp-venv-requirements.txt
```

For a medium-confidence pip venv (no `requirements.txt`):

```dockerfile
# pip packages: /opt/myapp/venv (detected via dist-info — transitive deps may differ)
# Uncomment after verifying package list is complete:
# RUN python3 -m venv /opt/myapp/venv \
#     && /opt/myapp/venv/bin/pip install flask==2.3.3 requests==2.31.0
```

npm and gem environments follow the same pattern with their respective
install commands (`npm ci`, `bundle install`).

### Runtime prerequisites

The Containerfile emits a warning if a required runtime package
(`python3`, `nodejs`, `rubygems`) is not present in the RPM package
list. If you see this warning, add the runtime package to your
Containerfile before the language package section.

### Reviewing in refine

The refine UI shows a **Language Packages** section with each detected
environment. You can:

- Toggle individual environments between included and excluded
- View the detected packages and their versions
- See the confidence level and detection method

Manifest files (`requirements.txt`, `package-lock.json`, `Gemfile.lock`)
are bundled in the tarball under `language-packages/` and subject to
redaction (auth-bearing URLs are scrubbed).

## Unmanaged files (Tier 2)

Unmanaged file scanning is opt-in. Pass `--include-unmanaged` to enable
it:

```bash
sudo inspectah scan --include-unmanaged
```

### What gets scanned

inspectah catalogs files under `/opt`, `/srv`, and `/usr/local` that are
not owned by an RPM package and not already claimed by a Tier 1 language
package environment.

Before bundling, inspectah shows the total size and prompts for
confirmation. Suppress the prompt with `-y`/`--yes` for CI use:

```bash
sudo inspectah -y scan --include-unmanaged
```

### Excluding paths

Use `--exclude-path` (repeatable) to skip directories you do not want
bundled:

```bash
sudo inspectah scan --include-unmanaged \
    --exclude-path /opt/backups \
    --exclude-path /srv/archive
```

### Provenance signals

Each unmanaged file carries provenance signals to help you decide
whether to include it:

| Signal | What it tells you |
|--------|-------------------|
| Last modified | When the file was last changed |
| Permissions | File permission bits |
| UID / GID | Owning user and group |
| Writable mount | Whether the file sits on a writable filesystem |
| Mutable | Whether the path is on a mutable filesystem layer |
| Service working dir | Whether a systemd service uses this path as its working directory |

### Containerfile output

Regular files render as COPY directives:

```dockerfile
# === Unmanaged files (no package manager provenance) ===
# /opt/myapp/
COPY unmanaged/opt/myapp/config.yml /opt/myapp/config.yml
COPY unmanaged/opt/myapp/app.jar /opt/myapp/app.jar
```

Symlinks render as `RUN ln -sf` directives instead of COPY:

```dockerfile
RUN ln -sf '/opt/myapp/current' '/opt/myapp/latest'
```

### Reviewing in refine

The refine UI shows an **Unmanaged Files** section where you can toggle
individual files. Provenance signals are displayed in the detail pane
to help you decide which files to carry forward.

## Aggregate mode

In aggregate scans, both `language_packages` and `unmanaged_files`
appear as aggregate sections with the same prevalence-zone grouping
(Consensus, Near Consensus, Divergent) used by other sections.
