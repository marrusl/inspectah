# Release Notes: v0.9.0-beta.3

## What's new in v0.9.0-beta.3

A single-fix follow-up to v0.9.0-beta.2. inspectah builds a set of every path owned by an installed RPM and uses it to decide what on a host is stock and what is not. Paths containing a space were dropped from that set, so files RPM already provides were read as unmanaged content.

### RPM-owned paths containing spaces are read correctly

The owned-path set was built from the first whitespace-delimited field of each `rpm --query --all --dump` line. `rpm` puts the path first but quotes nothing, so an owned path containing a space is indistinguishable from a path followed by the trailing fields `--dump` appends, and every such path was cut at the first space and never entered the set. Stock packages ship these routinely, firmware blobs named after the hardware they drive being the common case, so an unmodified host runs into it. Ownership is now queried with `--qf '[%{FILENAMES}\n]'`, one path per line. That removes the ambiguity and gives up nothing, since nothing consumed the size, mode, or digest columns `--dump` adds.

Two things read that set.

**`/var` directory backing analysis**, which reaches the generated build. Backing detection checks `tmpfiles.d`, then systemd's `StateDirectory=`, `CacheDirectory=`, and `LogsDirectory=`, then RPM ownership, and calls a directory unbacked when none of them match. A dropped path fell through to unbacked, so a directory RPM already provides picked up provisioning it did not need: a `tmpfiles.d` entry in `config/usr/lib/tmpfiles.d/inspectah-var.conf`, or a `RUN mkdir -p` line in the Containerfile when mode data was missing. The same directory was listed as unbacked in the HTML audit report and in refine, and could turn on the tmpfiles.d row in the generated README. This is the consumer that made the fix worth releasing on its own.

**The `/usr` walk**, which diffs the filesystem against the same set and records what is left over as unmanaged `/usr` entries in the snapshot. Those entries were wrong by the same mechanism. On the RHEL 10 host this was confirmed against, 33 owned paths contained spaces, and the reported `/usr` entry count fell from 62 to 29 once ownership was read correctly. No report, web view, or TUI surface renders these entries yet, so this half of the fix corrects collected data rather than anything on screen.

Both consumers date from the features that introduced them and shipped in v0.9.0-beta.1 and v0.9.0-beta.2. Neither is a beta.2 regression.

### Schema version

No schema change in this release. Schema version remains 22, as set in v0.9.0-beta.1.

### Binaries

Pre-built binaries for 3 platforms:
- `inspectah-darwin-arm64` -- macOS on Apple Silicon
- `inspectah-linux-arm64-bin` -- Linux on ARM64 (static musl binary)
- `inspectah-linux-amd64` -- Linux on x86_64 (static musl binary)

**Full changelog:** https://github.com/marrusl/inspectah/compare/v0.9.0-beta.2...v0.9.0-beta.3
