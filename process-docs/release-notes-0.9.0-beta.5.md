# Release Notes: v0.9.0-beta.5

## What's new in v0.9.0-beta.5

This release makes the unmanaged-file findings on a real host more accurate
and consolidates how the refine export is assembled. Three fixes change what
a generated build contains: the kernel and libgcc no longer read as
unmanaged, `/srv` content is no longer copied into a path that first boot
overwrites, and the `/usr` walk stops reporting paths the unmanaged files
walk already covers. There is no schema change, so a v0.9.0-beta.4 snapshot
still loads.

### Fixed: unmanaged `/usr` walk reports the kernel and libgcc

The rpmdb records some packages under their pre-usrmerge paths (`kernel-core`
under `/lib/modules`, libgcc under `/lib64`), while the walk sees
`/usr/lib/modules` and `/usr/lib64`. Ownership was a plain string match, so
every file of those packages read as unmanaged. On a measured RHEL 10 host
the kernel module tree alone was 108.9 MB of the 121.9 MB the section
reported, and it advised copying the kernel into the image. A path owned
under `/lib`, `/lib64`, `/bin` or `/sbin` now also counts as owned under
`/usr`. A host without usrmerge is unaffected.

### Fixed: unmanaged files under `/srv` are no longer copied into the image

On fedora-bootc, centos-bootc and rhel-bootc, `/srv` is a symlink to
`var/srv`, so the Containerfile's `COPY` wrote that content into `/var/srv`.
It was unpacked once on first boot and never changed by a later image
update. An included `/srv` finding now gets a comment block in the
Containerfile and a note in the audit report instead. Both name the paths and
say to carry them with the `/var` strategy: restore them into `/var/srv`
after deployment, or give them their own mount. The files are still bundled
under `unmanaged/srv/`. `/opt` and `/usr/local` findings are unaffected.

### Fixed: the `/usr` walk honors the same exclusions as the unmanaged files walk

The `/usr` walk filtered only by RPM ownership, so `/usr/local`, which the
unmanaged files walk already covers, was reported a second time. The same
file got two `COPY` lines and two include toggles that did not know about
each other, and a symlink there (an npm global such as `/usr/local/bin/ng`)
could fail the second bundler with `File exists`. Paths given with
`--exclude-path` and language environments already counted elsewhere were
also reported again under `/usr`. The `/usr` walk now skips every unmanaged
files scan root under `/usr`, taken from the same list that walk uses, and
applies both exclusions with the same prefix match.

### Changed: the refine export is assembled in one place

`RefineSession::export_tarball` is removed and replaced by
`RefineSession::export_plan`, which returns an `ExportPlan` the caller
renders with `ExportPlan::render`. `POST /api/tarball` previously built its
own copy of the same argument list, so the route and the session could
disagree about what an export is without any test seeing it. There is now one
assembly. The download is byte-for-byte what it was; this removes a way for
the two paths to drift and does not change what either produces today.
Library callers using `export_tarball` should call
`session.export_plan(generation)?.render(&path)`.

### Carried over from beta.4: an unreadable `/etc` directory marks the scan partial

The v0.9.0-beta.4 release notes do not mention this change, which shipped in
that release (its changelog entry was added afterward). The main `/etc` walk
used to record a permission failure on a subdirectory and then drop it unless
the whole walk came back empty, so the config inspector reported complete
while that subtree's files were missing from the export. The failure now
degrades the config inspector and the scan reports partial. A host with any
unreadable `/etc` subdirectory, for example `/etc/sssd`, gets a partial scan
where it used to get a complete one. If a re-scan after upgrading from
v0.9.0-beta.3 or earlier now reports partial, this is why.

### Schema version

Schema version is **25**, unchanged from v0.9.0-beta.4.

### Binaries

Pre-built binaries for 3 platforms:
- `inspectah-darwin-arm64` -- macOS on Apple Silicon
- `inspectah-linux-arm64-bin` -- Linux on ARM64 (static musl binary)
- `inspectah-linux-amd64` -- Linux on x86_64 (static musl binary)

**Full changelog:** https://github.com/marrusl/inspectah/compare/v0.9.0-beta.4...v0.9.0-beta.5
