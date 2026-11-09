---
title: Handle Repo-less RPMs
parent: How-To Guides
nav_order: 8
---

# Handle Repo-less RPMs

Some packages on your system may have no repository source: they were
installed from a local `.rpm` file, their original repo was disabled, or
the repo was removed after installation. inspectah detects these during
the scan and gives you a way to include them in your migration.

The examples on this page come from a RHEL 9.8 host that had a cached
`dos2unix` RPM, a `units` package installed from a local file with no
cached copy, and an `openh264` package whose repository had been
disabled.

## How detection works

Detection looks only at packages added on top of the base image. During
the scan, inspectah compares each package's recorded install-time
repository against the repositories `dnf repolist --enabled` reports. A
package is repo-less when:

- Its `source_repo` field is empty.
- Its `source_repo` is `@commandline` (installed from a local RPM path).
  These are always repo-less, unless the package is repo-enabling (see
  [Repo-enabling packages](#repo-enabling-packages)).
- Its `source_repo` names a repository that does not match any enabled
  repository. The match is a case-insensitive substring test: the
  recorded name, lowercased, must appear inside an enabled repo id. That
  handles short names against full ids (for example `AppStream` against
  `rhel-9-for-aarch64-appstream-rpms`). If `dnf repolist --enabled` fails,
  inspectah flags only the packages with an empty `source_repo`.

A package whose recorded repository is an installer pseudo-repo such as
`anaconda` gets one more check: if `dnf repoquery --available` shows an
enabled repository serves it, it is not repo-less.

Detection is a separate step from the cache search. For each repo-less
package, inspectah runs `find /var/cache/dnf -name '*.rpm'` and takes a
file whose path ends in `<name>-<version>-<release>.<arch>.rpm`. That is
an exact filename-suffix match on the package's name, version, release and
architecture. It is unrelated to the substring test above.

The detection code is in
`crates/collect/src/inspectors/rpm/repoless.rs`.

## What you get

Repo-less packages fall into two groups:

| Status | Meaning | Containerfile rendering |
|--------|---------|------------------------|
| **Cached** | A matching `.rpm` was found under `/var/cache/dnf/`, or you uploaded one in refine | A `COPY` of the RPM plus `dnf localinstall`, with the RPM bundled under `repoless-packages/` |
| **Missing** | No cached `.rpm` and no upload | A `MANUAL` comment block, all lines commented out |

Whether the cached rendering is active or commented out depends on which
path produced the Containerfile:

- **The scan tarball** writes a cached RPM as an active `COPY` and
  `dnf localinstall`, even though the package's own annotation says
  "pre-excluded".
- **`inspectah refine`** starts every repo-less package excluded, so the
  preview and an untouched export show the cached block commented out.
  Including the package makes the lines active. The one exception is a
  package from a disabled repository whose `.repo` file the export
  carries (see
  [A package from a disabled repository](#a-package-from-a-disabled-repository)).

### Cached RPM

This is the block the scan tarball wrote for the cached `dos2unix`
package. It is captured output from the scan on the RHEL 9.8 host.

```dockerfile
# === Repo-less RPM packages ===
# Repo-less package: dos2unix (cached RPM, no repository provenance)
# WARNING: This package has no upstream repo and no GPG verification.
# It was found in the local dnf cache. Updates must be managed manually.
COPY repoless-packages/dos2unix-7.4.2-4.el9.aarch64.rpm /tmp/
RUN dnf localinstall -y /tmp/dos2unix-7.4.2-4.el9.aarch64.rpm \
    && rm /tmp/dos2unix-7.4.2-4.el9.aarch64.rpm
```

Refine's first preview of the same package shows the lines commented
out:

```dockerfile
# Repo-less package: dos2unix (cached RPM, no repository provenance)
# WARNING: This package has no upstream repo and no GPG verification.
# Pre-excluded — uncomment after verifying provenance:
# COPY repoless-packages/dos2unix-7.4.2-4.el9.aarch64.rpm /tmp/
# RUN dnf localinstall -y /tmp/dos2unix-7.4.2-4.el9.aarch64.rpm \
#     && rm /tmp/dos2unix-7.4.2-4.el9.aarch64.rpm
```

Cached RPMs are bundled in the tarball under `repoless-packages/`.

### Missing RPM

A package with no cached RPM gets three comment lines and nothing else.
The `units` package (local install) looks like this:

```dockerfile
# MANUAL: units (no repo source, RPM not in cache)
# Provide the RPM via the refine UI upload, add a repo, or uncomment:
# RUN dnf install units
```

The annotation for an `@commandline` package reads as if the repository
were disabled (`repo '@commandline' not in enabled repos`). That is the
same detection path; it does not mean a repository named `@commandline`
exists.

The scan summary, `README.md` and `audit-report.md` have no repo-less
section. Look for the `=== Repo-less RPM packages ===` block in the
Containerfile, and for the `.rpm` files under `repoless-packages/`.

## Repo-enabling packages

A package that exists to enable a repository, such as `epel-release`, is
not handled as repo-less when the export can reproduce the repository it
enables. It installs by name from the repository definition the export
carries.

The scan decides this with two tests. The package was installed from a
local file and is named `*-release`; every path it owns under
`/etc/yum.repos.d/` and `/etc/pki/rpm-gpg/` is captured in the export,
with at least one `.repo` file among them. And an enabled repository on
the host serves the package by name, as `dnf repoquery --available`
reports. The scan records the serving repository ids on the package.

At render time the rule in `crates/pipeline/src/render/repo_enabling.rs`
does not look at the package name. It checks that the recorded proof
still holds in this export:

- An included `.repo` file defines a serving section.
- That section is enabled.
- Every `gpgkey=file://` path the section names is an included key.
- No repo file or key the package itself owns was dropped.

When all of that holds, the package installs by name in its own
`dnf install` step, ahead of the main package list, with a comment saying
why. This is the block from the scan on the RHEL 9.8 host:

```dockerfile
# === Repo-enabling packages (1) ===
# epel-release was installed outside any repo on the source host; the host's
# enabled repos served it, and its repo files and key are copied above, so it
# installs from the copied definition. The build needs those repo endpoints
# reachable with any credentials and CA trust they require. Installing after the
# COPY may leave a cosmetic epel-cisco-openh264.repo.rpmnew. Alternative, when
# dl.fedoraproject.org is reachable:
# RUN dnf install -y https://dl.fedoraproject.org/pub/epel/epel-release-latest-9.noarch.rpm
RUN dnf install -y \
    epel-release \
    && dnf clean all \
    && rm -rf \
        /var/cache/dnf \
        /var/lib/dnf/history* \
        /var/log/dnf* \
        /var/log/hawkey.log \
        /var/log/rhsm
```

When a refine change removes the proof, for example by excluding the
repository file or its key, the package is demoted. The Containerfile has
no install line for it. A `FIXME` comment takes its place, naming the
`.repo` file to include under Configuration Files or the repository to
re-include, and offering the upload path: upload the RPM and it installs
as a repo-less package. If the export captured no file that defines the
serving section, the comment omits the file and names only the
repository. This page does not show the demotion output: the host used
for the examples had no way to reach that state, so the description above
rests on the source and its tests.

If the source host had a CodeReady Builder repository enabled (a section
whose id starts with `codeready-builder-`, other than its debug and
source variants), the Containerfile adds `--enablerepo=<id>` with the id
exactly as the host wrote it. It goes on the package, group,
module-stream and repo-enabling `dnf` transactions, with one comment
saying the repository was enabled on the source host. That also rests on
the source: the example host had no CodeReady Builder repository.

The scan and refine disagree on the default. The scan tarball installs
`epel-release` in the block above. In refine it starts excluded, and an
untouched export has no repo-enabling section; include the package to
get it.

## Providing missing RPMs

### Single upload

In the refine UI, a repo-less package with no RPM shows an upload button
in place of its checkbox and an "RPM needed" label. The button opens the
upload modal, titled "Upload RPM for" followed by the package name. Drag
and drop the `.rpm` file, or choose it with the browse button.

The modal checks the filename before it enables the Upload button. The
name before the first `-<digit>` must equal the package name, and the
file must end in `.<arch>.rpm` for the package's architecture or in
`.noarch.rpm`. A file that fails shows a message such as
`Expected package "units", filename suggests "gnu-units"`. The modal
also shows the expected pattern, `<name>-*-*.<arch>.rpm`.

The server then applies its own checks. These are the three outcomes,
measured with the refine API:

- **Filename outside the allowlist: refused with HTTP 400.** The
  filename must start with a letter or digit, use only the permitted
  bytes, and end in `.rpm` (case-insensitive). The response body carries
  the reason, and the modal shows it. A name with a space or a `../`
  prefix is refused; so is a file that does not end in `.rpm`:

```json
{"error":"filename rejected: staged filename does not end in .rpm"}
```

- **A `name.arch` that matches no repo-less package: accepted, not
  refused.** The server answers 200 with `"status":"unmatched"`, keeps the
  file in the session's uploads directory, and never exports it. The
  modal shows "No matching package". A non-NEVRA filename is accepted as
  long as its name and architecture match a package, for example
  `units.aarch64.rpm`. The response for a file named for the wrong
  architecture:

```json
{"host_version":null,"match_kind":null,"matched":null,"parsed":{"arch":"x86_64","name":"units","version_release":"2.21-4.el9"},"staged_as":"units-2.21-4.el9.x86_64.rpm","status":"unmatched","uploaded":1,"uploaded_version":null}
```

- **A body over 500 MiB: refused with HTTP 413.**

```json
{"error":"Request payload is too large"}
```

Matching is by filename. inspectah does not read the RPM header, so a
file renamed to match another package is treated as that package's RPM.

After a successful upload the package changes from "RPM needed" to
"RPM provided", and the Containerfile preview shows the `COPY` and
`dnf localinstall` lines. An uploaded RPM is rendered like this once the
package is included:

```dockerfile
# Repo-less package: units (uploaded RPM, no repository provenance)
# WARNING: This package has no upstream repo and no GPG verification.
# It was uploaded in refine. Updates must be managed manually.
COPY repoless-packages/units-2.21-4.el9.aarch64.rpm /tmp/
RUN dnf localinstall -y /tmp/units-2.21-4.el9.aarch64.rpm \
    && rm /tmp/units-2.21-4.el9.aarch64.rpm
```

Removing an uploaded RPM (the x on the "RPM provided" label) returns the
package to "RPM needed". A package whose RPM came from the dnf cache has
nothing uploaded to remove; the server answers 409.

### Batch upload

When several repo-less packages need RPMs, the "Upload RPMs" button opens
the batch modal, titled "Upload RPMs (N packages need RPMs)". It has:

- A collapsible "Packages needing RPMs" checklist with a summary such as
  "1 of 3 packages matched". Packages still waiting are grey labels;
  matched packages are green.
- A drop zone, "Drag and drop RPM files here or browse", accepting `.rpm`
  files, several at once.
- A count of matched files and a "Matched RPMs" list. inspectah matches
  each file to its package by `name.arch`, reading both from a NEVRA
  filename.
- A "Conflicting uploads" warning when files do not resolve to one
  package each. The Upload button stays disabled while a conflict is
  listed.

When you press Upload, the modal closes and the files upload one after
another. If the server refuses one, the remaining files are not sent and
the batch view does not report it; check the package list for packages
still marked "RPM needed".

### Including cached RPMs

Cached and uploaded RPMs are excluded by default in refine. To include
one in your Containerfile:

1. Open the refine UI.
2. Go to the Packages view.
3. Find the repo-less package. A cached one carries a "No repo" label; an
   uploaded one carries "RPM provided".
4. Include it.

The Containerfile preview updates to show active (not commented-out)
`COPY` and `dnf localinstall` lines. The preview labels an excluded
uploaded RPM "cached RPM", because the upload marks the package as
cached; only an included upload gets the "uploaded RPM" wording.

## A package from a disabled repository

A package whose repository is disabled on the source host, such as the
`openh264` package in the example, is repo-less: no enabled repository
serves it. The host still has the repository's definition, though, and
the package was installed from it on purpose. When the export carries
that `.repo` file, the disabled section in it defines a `baseurl`,
`metalink` or `mirrorlist`, and the export carries every
`gpgkey=file://` key the section names, the package installs from that
repository in its own step, with the repository enabled for that step
only:

```dockerfile
# === Packages from repos disabled on the source host (1) ===
# 'epel-cisco-openh264' is disabled on the source host (enabled=0); enabled for this install only.
RUN dnf install -y --enablerepo=epel-cisco-openh264 \
    openh264 \
    && dnf clean all \
    ...
```

When CodeReady Builder is carried, its `--enablerepo` flags are on this
step too. The package does not appear in the main `dnf install` list,
the repo-less block or the manual follow-up. As in the main list, a
package that is only a dependency of another one is not named in this
step, and dnf installs it with the package that needs it. The step runs
before the `dnf group install` and the main `dnf install` list, so a
package in either can depend on it. Refine treats it the same
way: it is not pre-excluded, so an untouched export installs it exactly
as the scan tarball does.

When the export does not carry the `.repo` file or a key it names,
because it was never captured or it was excluded, the package is handled like any other
repo-less package: it appears only in the repo-less block, and refine
starts it excluded.

The package still has no row of its own in the refine package list, so
you cannot see or exclude it there. The upload route accepts a file for
it by name and architecture, and an upload the export verifies takes
precedence: the package then installs from the uploaded RPM in the
repo-less block instead of from its repository. An RPM the scan found in
the dnf cache does not take precedence, and while the package installs
from its repository, neither the scan tarball nor the refine export
carries that cached RPM.

## Alternatives to bundling RPMs

Bundling RPMs works for one-off packages, but consider these
alternatives for a more maintainable image:

- **Add the repository** to your Containerfile and install via
  `dnf install`. This gives you update coverage.
- **Rebuild the package** for a supported repository if the source is
  available.
- **Replace the package** with an equivalent from an enabled
  repository if one exists.

The repo-less detection is a safety net: it ensures you know about
every package that cannot be reinstalled from a repo, so nothing falls
through the cracks during migration.
