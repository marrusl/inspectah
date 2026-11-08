# Release Notes: v0.9.0-beta.4

## What's new in v0.9.0-beta.4

This release ships what accumulated since v0.9.0-beta.3: SELinux file context
rules that now replay as real `semanage` commands, package and repo state
that reflects what a scan can actually prove, a single source of truth for
`/etc` content, and more accurate `/var` directory backing. It also carries a
breaking snapshot schema change, removes the experimental terminal UI, and
includes a security advisory that requires action from anyone who has
scanned a host with a recent inspectah. Read that advisory first.

### Security advisory: rotate credentials from affected exports

`inspectah scan` wrote every collected language-package manifest
(`requirements.txt`, `package.json`, `package-lock.json`, `Gemfile`,
`Gemfile.lock`) into `inspection-snapshot.json` with no credential
redaction. The redaction pass scrubbed `.env` file content and git remote
URLs but never reached manifest files, so a credential embedded in one of
those manifests, a compose file, or a quadlet unit reached the exported
snapshot unredacted. A snapshot could still be marked `FullyRedacted` despite
carrying the exposed credential, and `FullyRedacted` is the one state the
refine import path trusts to skip re-redaction, so the exposure could
survive a round trip through refine.

**If you've taken an export with any version from v0.8.7-beta.1 through
v0.9.0-beta.3: rotate any credential a scanned host carried in a
language-package manifest, a compose file, or a quadlet unit, then re-scan.**
Upgrading inspectah does not retroactively close this. A credential already
sitting in a shared tarball, a fleet aggregate, or a pushed image layer stays
exposed until it's rotated.

The scrubbing defect itself is fixed in this release (see Security, below),
so the fix protects exports taken from here on, not exports already made.

### Breaking change: snapshot schema 25

Snapshot schema moves to 25, up from 22 in v0.9.0-beta.3. Removed fields,
changed types, a new `shadowed_by` value, and a `/var` directory backing
that now records declaring file paths instead of a unit filename make
schema 22, 23, and 24 snapshots unloadable. `MIN_SCHEMA` rises with
`SCHEMA_VERSION`. There is no migration path for existing snapshots or
tarballs at those schema versions: re-scan affected hosts, and re-aggregate
fleet views from the re-scanned results.

### The experimental terminal UI is removed

`inspectah refine --tui` is gone, along with the `inspectah-tui` crate.
`--tui` is no longer a recognized flag. The web UI is the only refine
interface. The TUI shipped marked experimental from the start, so there is
no deprecation cycle and no migration path.

### SELinux file context rules now replay as real commands

Custom `semanage fcontext` rules used to be collected as raw table rows, and
every one of them failed the renderer's shell safety check. The generated
build carried a FIXME in place of each of the first ten rules, nothing at
all for any beyond that, and a commented-out `restorecon -Rv /` suggestion
at the end. Rules are now collected as typed records and emitted as valid
`semanage fcontext` commands, ordered to match the source host's policy
store when it's readable and falling back to sorted `semanage fcontext -l
-C` output otherwise, with a note in the build saying which. Label, unlabel,
and equivalence rules are each resolved against what the target image's
policy already defines, rather than applied blind. There is no build-time
`restorecon`, because a bootc target labels the deployed tree from the
image's own policy.

### Package, repo, and `/etc`/`/var` state reflects what's provable

Several places where inspectah previously guessed or inferred state now
check it directly:

- **Repo-enabling packages** (`*-release` packages) are recognized only when
  the host's enabled repos actually served them. The export keeps the active
  install only while one of those repo definitions is still carried,
  enabled, and keyed; when no proof survives, the package is demoted to the
  upload path with an instruction naming the repo.
- **Repo and group toggles** now read the effective member set and the
  packages an operation would actually change, rather than history or an
  empty-member check. An operation that would change nothing is refused with
  a message saying so.
- **`/etc` content now has one source.** The export previously rewrote
  kernel snippet directories independent of the Configuration Files include
  decisions, so excluding a file there didn't keep it out of the image.
  PAM files, audit rules, and custom tuned profiles are now ordinary
  Configuration Files entries with their own include decision.
- **`/var` directory backing is an exact-path check**, not a recursive grep
  that could match any directory under the real one. Backing now requires a
  `tmpfiles.d` entry, a `StateDirectory=`/`CacheDirectory=`/`LogsDirectory=`
  declaration, or RPM ownership at the exact path. On the unit side, the
  export also now asks whether the file declaring a directory actually
  reaches the image, so an excluded shadow unit no longer counts as backing
  it still provides.
- **The `/var` summary's wording is consistent** between the outcome column
  and the counts above it, instead of describing the same row two different
  ways.

### Uploaded RPMs are staged and verified before export

An RPM whose filename didn't match the host's exact version used to be
matched, marked, and then silently left out of the build, failing on a
missing `COPY` source. Uploads are now staged under their own filename
through one allowlist, streamed to disk in bounded chunks, and verified as a
regular file in the staging directory before the export copies them. A
staged file that can't be verified demotes its package with an instruction
instead of failing the export.

### Security: inputs that reach a shell or Containerfile are allowlisted

Two injection-class defects are fixed, both from host-sourced strings
reaching a `RUN` line, a `COPY` source, or a tmpfiles.d entry with no
validation:

- **RPM filenames** reach four places in the generated build: a `COPY`
  source, a shell-form `RUN dnf localinstall`, the `rm` operand after it, and
  a commented copy of all three for pre-excluded packages. A scanned host
  reporting a package name like `evil$(id)` could run arbitrary commands as
  root at build time. There's now one ASCII allowlist and one type that can
  only be built by passing it, enforced at upload, export, cached-RPM
  rendering, and session-resume rescan alike.
- **`/var` provisioning fields** (path, mode, owner, group, creator name)
  reach a `RUN mkdir -p` or a tmpfiles.d `d` entry with the same lack of
  validation. A path containing a shell metacharacter, or a newline in any
  of the five whitespace-separated `stat` fields the collector parsed, could
  inject a second tmpfiles.d directive or a build-time command. Each field
  is now proven by an ASCII allowlist, with a refused field either degrading
  to a safe stand-in (a numeric id for an account name) or falling back to a
  manual instruction.

### Fixed: config-file and drop-in ownership decisions

A cluster of fixes around who owns the decision to include or exclude a
systemd drop-in or an admin-written unit file:

- A `*.service.d/*.conf` drop-in and the admin-written unit it modifies are
  tracked in two places, and only one of them governed the actual export.
  The Configuration Files row for a service-owned path is now inventory,
  naming Service Drop-ins as the one place that decision is made, and the
  server refuses a toggle on the inventory copy.
- A Configuration Files row that became inventory this way now says which
  section controls it instead of showing a bare "Inventory" badge, and links
  to the owning row.
- A service drop-in could be excluded but never re-included, because the
  check for "can this be turned back on" relied on a parent-unit record that
  doesn't exist for a unit sitting at its preset state, which is the
  ordinary case. The refusal now tests for an actual exclusion of the parent
  rather than the absence of a decision about it.
- An admin-written unit file placed directly in `/etc/systemd/system/` was
  dropped from the configuration inventory by a nesting-depth check that was
  meant to exclude systemd's own alias symlinks. The check now tests file
  type directly, so a symlink is excluded and a regular file is kept.

### Fixed: exports produce what they claim to produce

- A redacted export's `package-lock.json` and `package.json` came out with
  every scrubbed value double-quoted (`""value""`), which is invalid JSON
  and broke `npm ci` on the generated image with no error pointing back at
  the cause.
- A failed RPM upload used to report success in the refine web UI: the
  client checked only the HTTP status and discarded the server's error body,
  so a rejected upload still showed a green checkmark. Both upload routes
  and the delete route now surface the server's actual reason.
- Two refine web surfaces (the Kernel and Boot mirror of Configuration
  Files, and the Storage view) served a cached projection of the session's
  original snapshot instead of the live, decision-replayed state, so a
  resumed session could show an exclusion as still included.
- Both export paths now carry every directory their own Containerfile
  copies: the refine web download was dropping `quadlet/` from the archive
  while still emitting a `COPY quadlet/` line, and the CLI scan tarball was
  emitting a `COPY language-packages/` line it never wrote a payload for.
- The refine web download's Containerfile could reference a tmpfiles.d
  fragment that wasn't written into the archive, failing `podman build` on
  a missing `COPY` source with no warning beforehand.

### Removed: dead `/var` fields and phantom kernel decisions

`size_estimate` and `recommendation` were written by nothing and rendered as
placeholder text on every `/var` row. Every loaded kernel module carried an
include decision no surface read and no renderer honored; they're inventory
now. `non_default_modules` and `repo_providing_packages` were never
populated.

### Schema version

Schema version is **25**, up from 22 in v0.9.0-beta.3. See "Breaking
change" above.

### Binaries

Pre-built binaries for 3 platforms:
- `inspectah-darwin-arm64` -- macOS on Apple Silicon
- `inspectah-linux-arm64-bin` -- Linux on ARM64 (static musl binary)
- `inspectah-linux-amd64` -- Linux on x86_64 (static musl binary)

**Full changelog:** https://github.com/marrusl/inspectah/compare/v0.9.0-beta.3...v0.9.0-beta.4
