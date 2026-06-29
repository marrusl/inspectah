---
title: Handle Repo-less RPMs
parent: How-To Guides
nav_order: 8
---

# Handle Repo-less RPMs

Some packages on your system may have no repository source -- they were
installed from a local `.rpm` file, their original repo was disabled, or
the repo was removed after installation. inspectah detects these
automatically and provides a path to include them in your migration.

## How detection works

During the scan, inspectah compares each package's install-time
repository against the list of currently enabled repos (`dnf repolist
--enabled`). A package is flagged as repo-less when:

- Its `source_repo` field is empty (locally installed RPM)
- Its `source_repo` names a repository not in the enabled list

For each repo-less package, inspectah searches `/var/cache/dnf/` for a
cached `.rpm` file matching the package NEVRA
(name-version-release.arch). Matching uses case-insensitive substring
comparison to handle short name vs. full repo ID differences (e.g.,
`AppStream` vs. `rhel-9-for-aarch64-appstream-rpms`).

## What you get

After the scan, repo-less packages fall into two categories:

| Status | Meaning | Containerfile rendering |
|--------|---------|------------------------|
| **Cached** | `.rpm` found in `/var/cache/dnf/` | `COPY` + `dnf localinstall` (commented-out by default) |
| **Missing** | No cached `.rpm` available | `MANUAL` comment with the package NEVRA |

### Cached RPM example

```dockerfile
# === Repo-less RPM packages ===
# Repo-less package: custom-tool (cached RPM, no repository provenance)
# WARNING: This package has no upstream repo and no GPG verification.
# It was found in the dnf cache but cannot be reinstalled from a repository.
# Uncomment to install from the bundled RPM.
# COPY repoless-packages/custom-tool-1.2.3-1.el9.x86_64.rpm /tmp/
# RUN dnf localinstall -y /tmp/custom-tool-1.2.3-1.el9.x86_64.rpm && rm /tmp/custom-tool-1.2.3-1.el9.x86_64.rpm
```

Cached RPMs are bundled in the tarball under `repoless-packages/`.

### Missing RPM example

```dockerfile
# Repo-less package: legacy-app (MANUAL — no cached RPM found)
# NEVRA: legacy-app-3.0-1.el9.x86_64
# ACTION REQUIRED: Obtain the RPM and upload it in the refine UI,
# or add a repository that provides this package.
```

## Providing missing RPMs

### Single upload

In the refine UI, repo-less packages with missing RPMs show an upload
button. Click it to open the upload modal, then drag and drop (or
browse for) the `.rpm` file.

The upload endpoint validates:

- File has a `.rpm` extension
- File matches the expected package by `name.arch` (non-NEVRA filenames
  are accepted as long as the name and architecture match)
- File size is under 500 MiB

After upload, the package status changes from `MANUAL` to cached, and
the Containerfile preview updates to show the `COPY` + `dnf
localinstall` directives.

### Batch upload

When multiple repo-less packages need RPMs, use the batch upload modal.
It shows a checklist of all packages needing RPMs with live match
progress. Drop multiple `.rpm` files at once -- inspectah auto-matches
each file to its package by `name.arch`.

The batch view shows:

- Packages still needing RPMs (grey)
- Packages matched to an uploaded file (green)
- Conflicts when an uploaded file does not match any expected package

### Including cached RPMs

Cached RPMs (whether from the dnf cache or uploaded) are pre-excluded
by default. To include them in your Containerfile:

1. Open the refine UI
2. Navigate to the Packages section
3. Find the repo-less package (it has a "repo-less" annotation)
4. Toggle it to included

The Containerfile preview updates immediately to show active (not
commented-out) `COPY` and `dnf localinstall` directives.

## Alternatives to bundling RPMs

Bundling RPMs works for one-off packages, but consider these
alternatives for a more maintainable image:

- **Add the repository** to your Containerfile and install via
  `dnf install`. This gives you update coverage.
- **Rebuild the package** for a supported repository if the source is
  available.
- **Replace the package** with an equivalent from an enabled
  repository if one exists.

The repo-less detection is a safety net -- it ensures you know about
every package that cannot be reinstalled from a repo, so nothing falls
through the cracks during migration.
