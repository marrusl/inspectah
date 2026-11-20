# Release Notes: v0.9.0-beta.6

## What's new in v0.9.0-beta.6

This release changes what an export from an ordinary registered RHEL host
contains. Redaction replaces only the secret and leaves the stock
`rhsm.conf` parseable, the host's Insights and subscription identity no longer
reaches the image, repo and duplicate-definition handling is corrected, and
systemd units are reworked from services into a full unit model (sockets,
timers, links, the default target). Packages nothing in the image needs are
now listed as not carried, and refine can include one with a click. The
snapshot schema is now 28.

### Breaking changes

inspectah has no backwards-compatibility promise before 1.0, so there is no
migration path for these.

- **Snapshot schema is 28.** `refine` and `aggregate` load only schema 28.
  Snapshots from v0.9.0-beta.5 and earlier do not load: re-scan each host and
  re-aggregate. The `services` section is now `units`.
- **A saved refine session that cannot load is kept.** `inspectah refine` used
  to delete an unreadable saved session and start fresh. It now stops with an
  error naming the file and why, and leaves it in place. `--fresh` discards it.
- **The refine API uses `units`.** The API and unit operations take unit names
  instead of service names. Scheduled Tasks rows have new ids in
  `/api/snapshot/sections`, because rows that share a name no longer share an
  id.
- **Export warnings carry a severity.** The `x-export-warnings` header from
  `POST /api/tarball` is now a JSON array of `{"kind": ..., "text": ...}`
  objects, with `kind` being `failure` (something you asked for is missing) or
  `notice`. A script that reads the header must expect objects.
- **`99-inspectah-migrated.conf` is no longer written.** The scan tarball and
  the refine export have no `sysctl/` directory and no `COPY sysctl/` line.
  Sysctl files are carried as the host had them, under their source paths in
  `config/`, such as `config/etc/sysctl.d/` and `config/etc/sysctl.conf`.
  Runtime sysctl drift is reported, not carried.
- **The scheduled timer toggle in refine is gone.** A converted cron timer and
  a local or shadowed timer have their own include controls in the unit rows.

### Security and secret redaction

- **Redaction replaces only the secret, not its key.** `proxy_password =
  hunter2` becomes `proxy_password = REDACTED_PASSWORD_1`, so the stock
  `rhsm.conf` still parses. An empty value is left alone.
- **Lines starting with `//` or `;` are no longer skipped where neither is
  comment syntax.** In `fstab`, `sshd_config`, PAM files and most `.conf` files
  only `#` starts a comment, so `//srv/share/password=hunter2` used to keep its
  secret with no finding in `secrets-review.md`. `;` now marks a comment only
  in `.ini`, `.cnf`, `.repo`, `smb.conf` and systemd files, and `//` only in
  `named.conf`.
- **Quoted credential values are redacted whole**, and an AWS access key id
  after a comment line, or right after another key id, is redacted. Credentials
  in podman output shown during a scan are redacted too.
- **`secrets-review.md` gives the line each secret is on** and names the
  tokens redaction writes.
- **Snapshot values can no longer inject into generated output.** Values taken
  from a snapshot are now checked or quoted before they reach a Containerfile
  instruction, a `COPY`, `FROM` or `dnf group install` line, a `RUN` line, the
  flatpak provisioning unit, the kickstart suggestion or the users blueprint.
  A refused value gets a `# FIXME`.
- **Paths and archives are bounded.** Names from the snapshot cannot write
  outside the export, and refine keeps every file it extracts inside the export
  directory. It enforces its archive limits on the size actually extracted and
  bounds decompression. The scan tarball's cached repo-less RPMs pass the RPM
  filename allowlist before they are written, and npm global package names are
  quoted and cannot pass npm an option.
- **Refine rejects requests not addressed to a loopback name.**

### Registration state stays out of the image

- **The source host's machine-id is never copied into the image.** Unit states
  rely on the image keeping the base image's empty `/etc/machine-id`. Any offer
  of the file, as a config file, drop-in, repo file or unmanaged file, is left
  out, the Containerfile names it in a `# FIXME:`, and refine shows why under
  the row.
- **Insights and subscription identity are left out.** The scan no longer
  collects the files insights-client and subscription-manager write at
  registration (`/etc/insights-client/machine-id`, `.registered`,
  `/etc/rhsm/facts/insights-client.facts`, and `/etc/pki/product/`
  certificates), so deployed machines do not claim the source host's
  registration. Admin-written configuration such as Insights `tags.yaml` is
  still carried.

### Repos, GPG keys and packages

- **Excluding a repo in refine drops its `.repo` file and GPG key.** A `.repo`
  file is copied while one of its enabled sections is included, and a key while
  an included, enabled section names it. Disabled sections no longer count as
  in use, and a case variant of the repo id is no longer excluded with it.
- **EL10 exports that use EPEL build.** The scan expands dnf variables such as
  `$releasever_major` in `gpgkey=` paths as dnf does on that host and copies
  the key. A key it still cannot collect is a warning naming the repo file.
  Snapshots record the path as written in the new `gpgkey_references` field,
  and a key several repo sections name is recorded once. A GPG key the base
  image already ships is not copied over it, and the stock Red Hat release key
  is read as a key.
- **A package from a repo disabled on the source host installs from that
  repo**, with `--enablerepo=<repo id>`, once, when the export carries the
  `.repo` file and its keys.
- **A repo defined in two files is judged by the copy dnf reads first.**
- **Repo-less RPMs install before the packages and services that need them**,
  in the same `dnf install` as packages from disabled repos.
- **Packages nothing in the image needs are listed as Not carried.** A package
  not user-installed on the source host that no installed package depends on is
  left out by default and listed in the Containerfile (`# Not carried:`) and
  the audit report. Dependencies of kept packages are still carried. Refine has
  a Not carried group in the Packages view, with a one-click Include that
  installs the package, enables its units again and stops provisioning its
  `/var` directories. `inspectah scan` and the refine export name packages
  they leave out that owned an enabled unit.
- **Excluding a package no longer leaves its service in `systemctl enable`.**
- **Config files:** RPM-owned config files you modified are copied into the
  scan tarball, and a config file the scan could not read is reported instead
  of copied empty. `inspectah aggregate` no longer treats an unreadable file as
  matching an empty copy.

### Systemd units rework

- **Units, not services.** The scan records state for sockets, timers and other
  unit kinds, reads enablement from a unit's links instead of systemctl's
  word, and carries the source host's default target and enabled template
  instances whose configuration is in the export. Hand-made unit links are
  reported with the line that recreates them.
- **Safer unit commands.** Unit commands run after the files they name, in one
  section. A command the export cannot reproduce is left out with the reason,
  units the scan cannot place get a warning, and unit names are checked against
  systemd's naming rules. A unit at its preset whose `/etc` files change
  `[Install]` is carried or advised, and an `[Install]` section the scan cannot
  rely on is advised, not carried.
- **Units the image needs are never switched off**, and a disabled local timer
  is no longer enabled in the image. A timer whose trigger the export does not
  carry is not enabled.
- **Package updaters that run in place are excluded by default**
  (`dnf-automatic*.timer`, `packagekit-offline-update.service`), because an
  image-based host is updated by a new image. Include one in refine to carry it.
- **An unmanaged file is never written under `/etc/systemd/system/`**, and
  refine says why. A unit inspectah generates never replaces one collected from
  the host.
- **The export README explains carried unit states**, and advisories cover your
  own mount, automount and swap units.

### Hostnames

- **`inspectah aggregate` accepts hosts that have no hostname.** Scans of
  unnamed hosts, such as DHCP-named cloud instances, were refused as
  `duplicate hostname: <unknown>`. Each now aggregates as its own host, listed
  as `<unnamed N, scanned <time>>`. The same snapshot given twice is still
  refused.
- **A host whose `/etc/hostname` is empty is named by the scan.** It falls
  back to the host's transient name and records its source, so the refine
  header is no longer blank and the export is no longer named
  `inspectah-inspectah-refined.tar.gz`.

### Refine

- **The Export dialog shows the export's warnings.** When an export has
  warnings the dialog stays open as "Downloaded, with warnings" and lists each
  one. A collapsed notice before download names not-carried packages that own
  an enabled unit.
- **Services is now Systemd units, grouped by kind**, with each unit's kind,
  unit links, `Also=` members and why a command is left out. Section search
  works there.
- **Refine is documented as a workstation tool.**
- **Fixes:** an SELinux port label excluded in refine stays out of the export;
  the refine download installs package groups as the preview shows; a failed
  user change shows on the user card; closing the RPM upload dialog no longer
  abandons the upload; removing an uploaded RPM says why it was refused; an
  export from a stale tab says so; the Version Changes panel opens; typing in
  Packages search keeps the whole word; row shortcuts work on package and
  container rows; modifier keys no longer act on Configuration Files rows; and
  the web UI's development server no longer has its changes refused.

### Other fixes

- A pre-loaded base image works on a disconnected host: the scan uses a stored
  image without pulling and prints `Using local image`. A stored image is not
  refreshed.
- `--scan-home all` selects UIDs 1000 through 59999 and never a home of `/`.
  A home or `--scan-path` with a `..` segment is refused.
- Path exclusions and provenance no longer hide sibling paths, package
  dependency lists are accurate, and a multi-value sysctl set to its configured
  value no longer reports as drift.
- Host text in `README.md` and `audit-report.md` renders literally, and
  language package paths with shell characters render again.
- A file the export cannot write now fails the export, and an npm project
  missing a manifest no longer breaks the build.

### Known limitations

Three problems in this release have workarounds and are planned for a later
release. Details: [docs/reference/known-limitations.md](../../docs/reference/known-limitations.md).

- A carried user whose home is under `/home` fails `podman build` on a bootc
  base (`useradd -m` cannot create the home): exclude the user in refine, or
  change `useradd -m` to `-M` in the Containerfile and create the home on the
  running system.
- A carried user whose group is missing from the image fails `podman build`
  with `useradd` exit 6: exclude the user in refine, or add
  `RUN groupadd -g <gid> <name>` before that user's `useradd` line.
- A redacted quoted or XML value loses its closing quote or tag: restore the
  closing quote or tag by hand in the exported file.

### Schema version

Schema version is **28**, up from 25 in v0.9.0-beta.5. Snapshots from earlier
releases must be re-scanned.

### Binaries

Pre-built binaries for 3 platforms:
- `inspectah-darwin-arm64` -- macOS on Apple Silicon
- `inspectah-linux-arm64-bin` -- Linux on ARM64 (static musl binary)
- `inspectah-linux-amd64` -- Linux on x86_64 (static musl binary)

**Full changelog:** https://github.com/marrusl/inspectah/compare/v0.9.0-beta.5...v0.9.0-beta.6
