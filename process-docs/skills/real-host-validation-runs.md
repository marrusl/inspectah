# Real-host validation runs

Running the scan → refine → build loop against a real RHEL host rather
than against fixtures. Everything here was measured on RHEL 9.8 aarch64,
2026-10-31, and each item cost time to discover.

## A clean host is not a test

A fresh RHEL install has almost nothing to detect. Scanning one produces
a near-empty snapshot and exercises refine with no data in it — a test
that cannot fail. Apply synthetic drift first with the companion
fixture tool, then scan. The drift tool's own summary is the oracle: the
gap between what it planted and what the scan found is worth more than
anything the UI shows, because it is measured against known ground truth
rather than against our own fixtures.

Profiles are **cumulative**. A partially-applied larger profile cannot
host a clean run of a smaller one; revert the VM to a clean snapshot
instead of trying to undo.

## Fixtures and real hosts disagree about where `/etc` unit files live

The single most valuable finding of the first real-host run.

`config.files` carries **zero** paths whose parent is
`/etc/systemd/system`. Real `/etc` unit files land in
`services.drop_ins`, classified with `shadow_type: full_shadow`. The
hand-built fixtures put them in `config.files`.

This matters because `Evidence::from_snapshot` in
`crates/pipeline/src/render/var_dirs.rs` builds `config_units` from
`snap.config.files`. On real data that map is empty, so the
`/etc`-versus-vendor unit precedence rule cannot fire. Three rounds of
review argued over that predicate without anyone noticing the path was
unreachable, because every test fed it fixture-shaped data.

**Before trusting a test that feeds `config.files` a unit path, check
whether a real scan would ever put one there.**

## Stock files reach findings, and fixtures never contain them

Curated fixtures hold only interesting files. Real hosts hold the
distro's furniture, and several inspectors pick it up:

- `/etc/iproute2/README` is the only file in that directory on stock
  RHEL 9 (iproute2 moved to stateless config), and it is reported as a
  static route on **every** stock RHEL 9 scan.
- `/var/spool/at/spool` is a stock *directory* of the `at` package,
  collected as an at job with every field empty.
- The `/var` walk returns ~65 directories on a lightly-used host, of
  which the great majority are `man` locale caches and dnf cache
  directories.

When adding an inspector, test it against a stock host before a drifted
one. The stock run is where false positives live.

## A zero-length unit file is masked

Worth knowing before filing it as a bug. A unit masked on the host is a
symlink to `/dev/null`; the export writes it as a **0-byte regular
file**, and the Containerfile's `COPY` lands *after* its `systemctl
mask`, overwriting the symlink. This looks like it silently unmasks the
unit. It does not — systemd treats a zero-length unit file as masked, and
`systemctl is-enabled` inside the built image confirms `masked`.

## `/etc/hostname` is often empty

A DHCP-named host has an empty `/etc/hostname` and only a transient
hostname; `hostnamectl --static` is empty too. `collect.rs` reads only
`/etc/hostname` and inserts `meta["hostname"]` only when non-empty, so
the snapshot carries no hostname at all. Consequences reach the refine
header, the export filename (`inspectah-inspectah-refined.tar.gz`, the
fallback doubled against the prefix), the README, the kickstart, and
`aggregate/validate.rs`, which keys the host as `<unknown>`.

Fixtures all set a hostname, so this only appears on real hardware.

## Persistent SELinux booleans read as non-customised

`semanage boolean -l` prints *state* and *default*. A persistent change
(`setsebool -P`) updates the stored default, so both columns read the
same. Only a runtime-only change leaves them different. Any filter of the
form `current != default` therefore keeps transient state and discards
durable admin customisation — the exact inverse of what a migration
wants. Use `semanage boolean -l -C`, which lists customised booleans
directly, as the `fcontext` and `port` collectors already do.

## sysctl values are not string-comparable

The kernel returns multi-value sysctls **tab**-separated; config files
write them **space**-separated. Comparing raw strings reports
`net.ipv4.ip_local_port_range` as drift on any host that sets it to its
own configured value. Normalise whitespace before comparing. Note also
that `SysctlOverride.default` holds the *file* value, not the kernel
default — the field name invites a wrong reading of the whole section.

## Driving refine without a browser

The server allows requests with **no `Origin` header** (`origin_guard`
treats absent-Origin as a non-browser client), so `curl` drives the full
API directly:

```
inspectah refine <tarball> --port 8642 --open false --fresh
curl -s localhost:8642/api/snapshot/sections
curl -s -X POST localhost:8642/api/op -H 'content-type: application/json' \
  -d '{"kind":"Op","op":"SetInclude","target":{"item_id":{"kind":"Package","key":{"name":"nginx","arch":"aarch64"}},"include":false}}'
curl -s -X POST localhost:8642/api/tarball -H 'content-type: application/json' -d '{"generation":N}' -o export.tar.gz
```

`TimelineEntry` is internally tagged on `kind`; `RefinementOp` uses
`tag = "op", content = "target"`; `ItemId` uses `tag = "kind", content = "key"`.
`/api/tarball` requires the current `generation` from `/api/view` and
returns `409` on a stale one. Export warnings ride in the
`x-export-warnings` response header.

## Verify every COPY source before blaming the build

Two export-contract defects were found this way, and the second was
invisible in the build log because an earlier step failed first:

```
grep "^COPY" Containerfile | awk '{print $2}' | tr -d "'" | \
  while read s; do [ -e "$s" ] || echo "MISSING $s"; done
```

Run this against an extracted export before `podman build`. The refine
web download and the CLI scan tarball write **different** trees — the
CLI writes `quadlet/`, the web export strips it while still emitting
`COPY quadlet/`; `language-packages/` exists only in the web export and
is therefore untouched by any CLI-side test.

## Cross-compiling and shipping

```
cargo zigbuild --target aarch64-unknown-linux-musl --release
```

Produces a statically linked, stripped ARM64 ELF in ~15 s on an M-series
Mac. Confirm the shipped binary is the commit under test with
`inspectah version`, which prints the short commit — the cheapest guard
against validating a stale binary.

`/usr/bin/time` is **not installed** on a minimal RHEL 9. Use
`start=$(date +%s)` / `$((end-start))` for timings on the guest. An
agent cannot time its own run; time the command.

## Test VM hygiene

- Killing a local `ssh` does **not** kill the remote process. Output
  redirected to a file on the guest survives the disconnect; `pkill` on
  the guest is what stops it.
- The drift tool's standard profile moves sshd to port **2222** and sets
  `PermitRootLogin no`, effective on the next sshd restart. It issues no
  restart, so a running session keeps port 22. Do not reboot mid-run.
- Shut a guest down with `poweroff` inside it, not by killing the VM. A
  guest killed mid-write can leave the rpmdb or journal inconsistent,
  which later reads as an inspectah bug.
