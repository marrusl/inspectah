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
| pip | Virtual environments | `pyvenv.cfg` files under the scan roots |
| pip | System packages | `dist-info` directories under `/usr/lib/python3`, `/usr/lib64/python3`, and `/usr/local/lib/python3`, filtered against RPM-owned packages |
| npm | Lockfile projects | `package-lock.json` files under the scan roots |
| npm | Manifest-only projects | `package.json` when no lockfile is present |
| npm | Global packages | `npm list -g`, merged with a walk of each global prefix, filtered against RPM-owned packages |
| gem | Lockfile projects | `Gemfile.lock` files under the scan roots |
| gem | System gems | `gem list --local`, filtered against RPM-owned gems |

### Scan roots

The scan roots are `/opt`, `/srv`, `/usr/local`, and `/var/www`. Two
flags add to them; neither removes a default root:

- `--scan-path PATH` adds one absolute path. Repeat the flag for more
  than one path. A relative path, or a path that does not exist, is
  skipped with a warning.
- `--scan-home all` adds the home directories of every user with UID
  1000 through 59999. `all` covers users in the regular range only: a
  user whose UID is outside it, including a directory-service user
  (SSSD or IdM) mapped above 59999, is not selected and must be named
  with `--scan-home <user>`. A home directory of `/` is never added.
  `--scan-home alice,bob` adds the home directories of
  the named users instead.

For example, a Python virtual environment under `/apps/reporting` is
outside the default roots. This command adds that directory to the scan:

```bash
sudo inspectah scan --scan-path /apps/reporting
```

### Confidence levels

Each detected environment gets a confidence level that controls how it
renders in the Containerfile:

| Level | Meaning | Containerfile rendering |
|-------|---------|------------------------|
| **High** | Lockfile or `requirements.txt` present, or `npm list -g` reported the packages; full dependency list | Active `RUN` directives |
| **Medium** | Packages detected via `dist-info`, a directory walk, or a runtime command; list may be incomplete | Commented-out directives (`# RUN ...`) |
| **Low** | Manifest-only (e.g., `package.json` without lockfile); no pinned versions | Advisory comment only |

High-confidence items run out of the box. Medium-confidence items need
you to uncomment them after verifying the package list. Low-confidence
items are informational only: you write the install commands yourself.

### Containerfile output

The Language Packages section of a scan of the sample host (two pip
virtual environments, one npm project, and the npm global packages),
captured below. The `venv` name in each temporary path is the final
component of the environment path, and the numbered directory under
`language-packages/` is a hash of the environment. Paths that come from
the host are single-quoted.

```dockerfile
# WARNING: This environment contains packages with C extensions that may need
# native compilation toolchains (gcc, python3-devel).
# pip packages: /opt/myapp/venv (from requirements.txt, RPM-filtered)
COPY language-packages/pip/77fce1b75e7a/requirements.txt '/tmp/venv-requirements.txt'
RUN python3 -m venv '/opt/myapp/venv' \
    && '/opt/myapp/venv'/bin/pip install -r '/tmp/venv-requirements.txt' \
    && rm '/tmp/venv-requirements.txt'

# pip packages: /opt/legacy-app/venv (from requirements.txt, RPM-filtered)
COPY language-packages/pip/3ee13faaf2c5/requirements.txt '/tmp/venv-requirements.txt'
RUN python3 -m venv --system-site-packages '/opt/legacy-app/venv' \
    && '/opt/legacy-app/venv'/bin/pip install -r '/tmp/venv-requirements.txt' \
    && rm '/tmp/venv-requirements.txt'

# npm packages: /opt/webapp (from package-lock.json)
COPY language-packages/npm/23f9725da273/package.json '/opt/webapp'/package.json
COPY language-packages/npm/23f9725da273/package-lock.json '/opt/webapp'/package-lock.json
RUN cd '/opt/webapp' && npm ci --production

# npm global packages: /usr/local/lib/node_modules (detected via npm list -g)
RUN npm install -g @angular/cli @types/node pm2 typescript
```

*Sample output from a RHEL 9.8 test host. Command: `sudo inspectah scan`.
Lines 91 to 111 of the Containerfile.*

The first pip environment is a plain virtual environment. The second was
created with `--system-site-packages`, so its `venv` command carries that
flag. The `# WARNING` line above the first environment appears when the
environment contains packages with C extensions.

For a medium-confidence pip virtual environment (detected from
`dist-info`, with no `requirements.txt`), the directives are commented
out:

```dockerfile
# pip packages: /apps/reporting/venv (detected via dist-info — transitive deps may differ)
# Uncomment after verifying package list is complete:
# RUN python3 -m venv '/apps/reporting/venv' \
#     && '/apps/reporting/venv'/bin/pip install humanize==4.13.0 pip==21.3.1 setuptools==53.0.0 tabulate==0.9.0
```

*Sample output from a RHEL 9.8 test host. Command: `sudo inspectah scan
--scan-path /apps/reporting`. Lines 105 to 108 of the Containerfile.*

Check the package list before you uncomment it. In this sample the list
includes the virtual environment's own tooling (`pip` and `setuptools`)
pinned at the versions that environment carries, which you may not want
in the image.

npm and gem environments follow the same pattern with their respective
install commands (`npm ci`, `bundle install`). For npm global packages,
a high-confidence prefix renders an active `RUN npm install -g ...`
line, as in the capture above. A prefix found only by the directory walk
is medium confidence and renders the same line commented out.

### Runtime prerequisites

The Containerfile emits a warning if a required runtime package
(`python3`, `nodejs`, `rubygems`) is not present in the RPM package
list. If you see this warning, add the runtime package to your
Containerfile before the language package section.

### Reviewing in refine

The refine UI shows a **Language Packages** section with each detected
environment. Each row shows the ecosystem, the path, the package count,
the confidence level, and the manifest the packages came from. You can
toggle individual environments between included and excluded.

Two badges can appear on a row:

- **C extensions**: some package in the environment ships native code,
  so the image build may need a compiler toolchain.
- **system site-packages**: a pip virtual environment that inherits the
  system site-packages directory.

Rows for npm global packages expand to list each package and its
detected version. Expand a row with its arrow button, or focus the row
and press `Enter`. Inside an expanded row, `ArrowUp` and `ArrowDown`
move between packages and `Escape` collapses the row. A search that
matches a package name expands the environment that holds it and moves
focus to the first match.

Each package in an expanded row has a pin checkbox. A pinned package
renders as `name@version` in the `npm install -g` line; an unpinned one
renders as the bare name, so the build takes whatever version npm
resolves. Each expanded row also has a bulk button, labeled **Pin all**
or **Unpin all**, which applies to every package in that environment,
including an environment with a single package. With a package row
focused, `Enter` or `Space` toggles its pin.

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

`--include-unmanaged` catalogs and bundles unmanaged files from `/opt`,
`/srv`, and `/usr/local`: files that are not owned by an RPM package and
not already claimed by a Tier 1 language package environment.

Before bundling, inspectah shows the total size and prompts for
confirmation:

```text
Found 12 unmanaged files in /opt, /usr/local (12.6 MB total)
Include in tarball? [Y/n] y
```

*Sample output from a RHEL 9.8 test host. Command: `sudo inspectah scan
--include-unmanaged`, run in a terminal with the prompt answered `y`.*

The default answer is yes. Suppress the prompt with `-y`/`--yes` for CI
use:

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
| Mutable | Whether the file changed after the system was installed |
| Service working dir | Whether a systemd service uses this path as its working directory |

### Containerfile output

Regular files under `/opt` and `/usr/local` render as `COPY`
directives. When a directory holds more than one regular file, the
renderer emits one `COPY` for the whole directory; when it holds exactly
one, the `COPY` names that file. Symlinks render as `RUN ln -sf`
directives instead of `COPY`, so the image keeps the original link
layout:

```dockerfile
# === Unmanaged files (no package manager provenance) ===
# These files were copied directly from the source host. They have
# no upstream update path and must be manually maintained.
COPY unmanaged/opt/legacy-app/requirements.txt /opt/legacy-app/requirements.txt
COPY unmanaged/opt/myapp/ /opt/myapp/
RUN ln -sf '/usr/lib64/myapp' '/opt/myapp/lib'
COPY unmanaged/usr/local/bin/ /usr/local/bin/
RUN ln -sf '/usr/local/lib/node_modules/pm2/bin/pm2-runtime' '/usr/local/bin/pm2-runtime'
RUN ln -sf '/usr/local/lib/node_modules/typescript/bin/tsc' '/usr/local/bin/tsc'
RUN ln -sf '/usr/local/lib/node_modules/pm2/bin/pm2' '/usr/local/bin/pm2'
RUN ln -sf '/usr/local/lib/node_modules/pm2/bin/pm2-dev' '/usr/local/bin/pm2-dev'
RUN ln -sf '/usr/local/lib/node_modules/pm2/bin/pm2-docker' '/usr/local/bin/pm2-docker'
RUN ln -sf '/usr/local/lib/node_modules/@angular/cli/bin/ng.js' '/usr/local/bin/ng'
```

*Sample output from a RHEL 9.8 test host. Command: `sudo inspectah scan
--include-unmanaged`. Lines 114 to 126 of the Containerfile.*

Findings under `/srv` get no `COPY`. On bootc images `/srv` is a symlink
to `/var/srv`, so that content is machine-local data that belongs to
your `/var` strategy, not image content. The Containerfile carries a
comment block in place of the `COPY`, naming each path:

```dockerfile
#
# Not copied: /srv is a symlink to var/srv on bootc images, so content
# there is machine-local data that belongs to the /var lifecycle, not
# image content. A COPY would land in /var/srv once, on first boot, and
# no later image update would change it. Carry it with your /var
# strategy: restore it into /var/srv after deployment, or give it its
# own mount. The files are in this export under unmanaged/srv/.
#   /srv/www/index.html
```

*Sample output from a RHEL 9.8 test host. Command: `sudo inspectah -y scan
--include-unmanaged`, with one file, `/srv/www/index.html`, added to the
sample host. Lines 127 to 134 of the Containerfile. The text `symlink to
var/srv` is printed without a leading slash.*

The audit report carries the same note under **Unmanaged Files under
/srv**:

```markdown
### Unmanaged Files under /srv

1 included file(s) under `/srv` have no `COPY` line in the Containerfile. On bootc images `/srv` is a symlink to `var/srv`, so this content is machine-local data that belongs to the `/var` lifecycle, not image content: a `COPY` would land in `/var/srv` once, on first boot, and no later image update would change it. Carry it with your `/var` strategy, restoring it into `/var/srv` after deployment or giving it its own mount. The files are in the export under `unmanaged/srv/`.

- `/srv/www/index.html`
```

*Sample output from a RHEL 9.8 test host. Same scan as the block above. Lines 536 to 540 of
`audit-report.md`.*

The files themselves are still bundled in the tarball under
`unmanaged/srv/`. Restore them into `/var/srv` after deployment, or give
that directory its own mount. `/opt` and `/usr/local` findings are not
affected.

### Reviewing in refine

The refine UI shows an **Unmanaged Files** section where you can toggle
individual files. Provenance signals are displayed in the detail pane
to help you decide which files to carry forward.

## Aggregate mode

In aggregate scans, both `language_packages` and `unmanaged_files`
appear as aggregate sections with the same prevalence-zone grouping
(Consensus, Near Consensus, Divergent) used by other sections.
