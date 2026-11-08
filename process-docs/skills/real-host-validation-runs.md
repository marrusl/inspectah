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

## A unit file directly in `/etc/systemd/system` never reaches `config.files`

The single most valuable finding of the first real-host run, and the
mechanism is not where you would look for it.

**Fixed 2026-11-01.** The account below is the behaviour through
`4af6582f` and is kept because the sequence that hid it is the lesson.
What the predicate does now, and what the fix did and did not move, is
in [§ Measured, 2026-11-01: the walk fix](#measured-2026-11-01-the-walk-fix-and-what-it-did-not-move).

`is_systemd_unit_alias` (`crates/collect/src/inspectors/config/walk.rs`)
runs **after** the three documented exclusion lists, so reading
`UNOWNED_EXCLUDE_EXACT`, `_GLOBS` and `_PREFIX` does not explain the
behaviour. It is:

```rust
const PREFIX: &str = "/etc/systemd/system/";
if let Some(rest) = path.strip_prefix(PREFIX) {
    !rest.is_empty() && !rest.contains('/')
} else { false }
```

**Named for alias symlinks, implemented as a nesting-depth test.** It
never checks whether the path is a symlink. An admin-written full unit
shadow — a regular file, owned by no package, exactly the thing a
migration must carry — is discarded for having no `/` left after the
prefix. Measured: `/etc/systemd/system/sshd.service` (regular file,
unowned) absent from `config.files`; `httpd.service.d/*.conf` present.

This matters beyond the walk, because that match set is the **exact
complement** of `unit_filename`'s `/etc`-side accept set in
`crates/pipeline/src/render/var_dirs.rs`. Both mean "a bare filename
directly under `/etc/systemd/system`". One excludes from `config.files`
exactly what the other looks for there, so `Evidence::from_snapshot`
builds an empty `/etc` half of `config_units` **by construction** and the
`/etc`-versus-vendor precedence rule cannot fire on an admin-written
shadow. Three review rounds argued over that predicate without anyone
noticing, because the fixtures put unit files in `config.files` where the
real collector routes them to `services.drop_ins` as
`shadow_type: full_shadow`.

**Correction, 2026-11-01.** An earlier revision of this file said one
route survives and called the rule "near-dead rather than dead", citing
`a_packaged_etc_override_does_not_mask_its_own_vendor_unit` in
`crates/pipeline/tests/var_backing_export_test.rs` as the test that
encodes it. **That citation was wrong and the status is `unproven`, not
`near-dead`.** The test's `config_file()` helper hardcodes
`kind: ConfigFileKind::Unowned`, so the entry feeding `config_units` is
unowned — exactly the class the real walk filters. Listing the path in
`rpm.file_ownership` makes the case "packaged" in name only. The test
exercises the classifier's path and include bits; it is not an
RPM-modified collector snapshot.

What still holds: `is_excluded_unowned` is consulted only for *unowned*
files (`config/mod.rs:198`), so an RPM-**owned** unit at
`/etc/systemd/system/<name>` that `rpm -Va` reports modified short-circuits before the alias check and
would reach `config.files`. That
control flow was re-read and is sound. What does **not** hold is any
evidence that it happens: no test and no measurement shows a real
collector producing such an entry. The route may exist. Nothing we have
demonstrates it does.

**The transferable lesson is the sequence, not the finding.** This is
the same error three layers deep. A fixture encoded a layout the
collector cannot produce. A review believed the fixture. A correction to
that review cited a second fixture carrying the same defect. Each layer
reasoned correctly from a premise nobody measured. A hand-built snapshot
is not evidence about collector behaviour at any depth, including inside
a correction that is otherwise right.

**Before trusting a test that feeds `config.files` a unit path, check
whether a real scan would ever put one there — and check the helper that
builds the entry, not just the path string.**

## Three things that hide a `/var` backing defect from a real host

All three cost time on 2026-11-01 while measuring the `crond.service`
full-shadow case below. Each one makes the host answer look right.

**The `/var` walk skips empty directories.** `discover_var_directories`
runs `find <base> -mindepth 1 -maxdepth 2 -type d ! -empty`. A
`StateDirectory=` systemd has just created has nothing in it, so it
reaches no snapshot at all and the export says nothing about it. Plant a
file inside the directory before scanning or the case does not exist.
Worth knowing beyond the test setup: a state directory that is empty on
the source host is invisible to the migration.

**Excluding a full shadow in refine also excludes the owning package.**
Unchecking `/etc/systemd/system/crond.service` left `cronie` excluded in
the exported snapshot. That masks a backing defect precisely, because
the thing being tested is whether a *carried* vendor package is wrongly
read as proof: with the package gone, the wrong rule and the right rule
agree. Re-include the package in the same session
(`SetInclude` on `ItemId::Package`) before downloading the export.

**`inspectah refine --fresh` prompts.** With saved operations on disk it
asks `Discard 1 saved operations? [y/N]` and quits on EOF, so a script
that backgrounds it with no stdin dies with curl exit 7 and an empty
log. Pipe `y` into it.

### Measured, 2026-11-01: the renderer never reads the field the walk routes to

Measured on `rhel9` with a planted `/etc/systemd/system/crond.service`
adding `StateDirectory=crondstate` over the stock `cronie` unit, which
declares no directory of its own. The scan returned `/var/lib/crondstate`
as `state_directory` naming `crond.service`, the shadow in
`services.drop_ins` as `full_shadow`, nothing in `config.files`, and
`cronie` as an included package owning
`/usr/lib/systemd/system/crond.service`.

Exclude the shadow and the export still calls the directory `Carried`,
and `inspectah-var.conf` comes out **empty** — the export neither makes
the directory nor flags it. `Evidence::from_snapshot` builds
`config_units` from `config.files` alone, so an operator decision taken
on a `services.drop_ins` entry is invisible to `/var` classification, and
a package shipping a same-named vendor unit is read as proof the
directory gets created. The two halves fit: the walk sends `/etc` unit
files to `services.drop_ins`, and the evidence builder does not look
there.

Pinned by `an_excluded_full_shadow_is_invisible_to_var_backing_evidence`
in `crates/pipeline/tests/var_backing_export_test.rs`. That test is also
the worked example of building a pipeline fixture from collector output
rather than from a guess at it.

**Fixed on 2026-11-01 and the test is live again.** The backing records
the full path of every unit file carrying the directive instead of one
filename, so "a unit of that name is carried" stopped standing in for "a
unit that declares this directory is carried". Re-measured on the same
host with the same planted shadow, with `cronie` put back in the install
set so the package cascade above could not decide it:

| | before | after |
|---|---|---|
| snapshot backing | `{"unit": "crond.service"}` | `{"declared_by": ["/etc/systemd/system/crond.service"]}` |
| verdict with the shadow excluded | `Carried` | `Provision` |
| `inspectah-var.conf` | no mention | comment naming the lost creator, then `d /var/lib/crondstate 0755 root root -` |
| audit report | no row | `created on host by crond.service \| not carried, provisioned` |

The control still holds: include the shadow and the directory is
`Carried` with nothing written, so the verdict follows the operator's
decision in both directions.

### Measured, 2026-11-01: the walk fix, and what it did not move

The predicate now tests the file type instead of the path depth. Same
host, same driftified state, privileged scan, control snapshot from the
merged `main` at `4af6582f`:

| path | before | after |
|---|---|---|
| `/etc/systemd/system/sshd.service` (regular file) | absent | **present** |
| `/etc/systemd/system/bluetooth.service` (link to `/dev/null`) | absent | absent |
| `/etc/systemd/system/kdump.service` (link to `/dev/null`) | absent | absent |
| `/etc/systemd/system/httpd.service.d/limits.conf` | present | present |

`config.files` went from 57 to 62 entries. The five arrivals are the
full shadow plus four admin-written units driftify plants
(`driftify-backup.service`/`.timer`, `myapp-report.service`/`.timer`).
Nothing was removed and nothing outside `/etc/systemd/system` moved.

**Two things the before-picture would have led you to expect, that did
not happen.** Both were measured rather than reasoned, and both matter
more than the row that flipped.

- **The export tree did not change at all.** Byte-identical file sets
  between the two binaries; the only Containerfile difference is the
  captured-file count. `configtree::service_owned_paths` subtracts every
  `services.drop_ins` path from the generic config tree, so a full
  shadow is still written from `drop-ins/` alone and is not duplicated.
- **The `/var` verdict did not move either.** `an_excluded_full_shadow_is_invisible_to_var_backing_evidence`
  fails identically before and after, on the same assertion, and adding
  the new `config.files` entry to that fixture at either polarity leaves
  it `Carried`. The walk fix makes the `/etc`-versus-vendor precedence
  rule **reachable** — the `/etc` half of `config_units` is no longer
  empty by construction — but `classify`'s excluded-entry arm then falls
  through to `unit_owners` and reads a same-named vendor unit's
  installation as proof the directory is created, without checking
  whether that unit declares it. Reachable is not fixed.

**Note the mask links.** `bluetooth.service` and `kdump.service` are
symlinks to `/dev/null`, which is masking, not aliasing. A predicate
written to the strict definition — a symlink whose target is another
unit file — would have pulled both into `config.files`. They are
excluded because the test is "is it a symlink", and the services
inspector records masked units separately. Worth knowing before anyone
tightens the rule: the services inspector labels both of them
`shadow_type: full_shadow`, which is wrong for a mask link, and that
mislabel predates the walk fix.

### Measured, 2026-11-01: one file, two dispositions, written nowhere

Found while checking what the walk fix widened. **It is not caused by
the walk fix, and it is reachable on `main` today.**

A `*.service.d/*.conf` drop-in appears in **both** `config.files` and
`services.drop_ins`, so refine offers two independent include switches
for one file. `configtree::service_owned_paths` subtracts the path from
the generic config tree with **no disposition check**, while the
drop-ins writer skips excluded entries. Set the config side included and
the services side excluded and the file is written to neither tree.

Measured end to end through the refine API with the `main` binary at
`4af6582f`: exclude the drop-in `/etc/systemd/system/httpd.service.d/limits.conf`,
leave its `config.files` twin included, download the tarball, and the
file is in neither `config/` nor `drop-ins/`. `x-export-warnings` is
`[]`. The reverse polarity carries the file from `drop-ins/`, so the
config-side switch is inert for these paths in both directions.

The walk fix extends the affected set from drop-in `.conf` files to bare
unit files: the same probe with the fixed binary and
`/etc/systemd/system/sshd.service` loses the file the same way.

### Measured, 2026-11-01: a drop-in over a preset-state unit builds correctly

The question behind the drop-in include guard: what does the export do
with a drop-in whose parent unit carries no `systemctl enable` line?
Answer: the right thing, and it is the ordinary case rather than a
corner.

Planted `/etc/systemd/system/chronyd.service.d/inspectah-probe.conf` on
`rhel9`. `chronyd.service` is `enabled` against an `enabled` preset, so
the services inspector put it in `preset_matched_units` and wrote no
`state_changes` row; the scan tarball from `f8839078` staged the file
under `drop-ins/` and emitted `COPY drop-ins/etc/systemd/system/
/etc/systemd/system/` with nothing naming `chronyd.service` in the
Service Enablement block.

`podman build` of that Containerfile, unmodified, against
`registry.redhat.io/rhel9/rhel-bootc:9.8`, inspected in the layer after
`COPY config/etc/ /etc/`:

```
/etc/systemd/system/chronyd.service.d/inspectah-probe.conf   present
/etc/systemd/system/multi-user.target.wants/chronyd.service  Jan  1 1970 -> /usr/lib/...
systemctl is-enabled chronyd.service                         enabled
```

The `Jan 1 1970` timestamp is the tell: that wants-link came from the
base image's own preset, not from anything inspectah wrote. The host was
enabled, the image is enabled, and the drop-in is in place. Emitting no
enable line is the correct output for a unit at its preset, not an
omission.

Two notes for whoever runs this next.

- **The build needs two workarounds, neither of them a bug in the
  export.** The driftify standard profile sets `HTTPS_PROXY` to an
  unresolvable `proxy.internal` and leaves `access.redhat.com` out of
  `NO_PROXY`, so the signature fetch fails; run podman under
  `env -u HTTP_PROXY -u HTTPS_PROXY -u NO_PROXY`. And the CLI scan
  tarball emits `COPY language-packages/...` lines for a directory only
  the web export writes, so the build dies there. Build with
  `--layers=true` and inspect the last good layer; everything the
  services sections do is finished by then.
- **A full shadow's wants-link still points at the vendor path.**
  `/etc/systemd/system/multi-user.target.wants/sshd.service ->
  /usr/lib/systemd/system/sshd.service` while the shadow sits at
  `/etc/systemd/system/sshd.service`. Unmeasured which fragment systemd
  loads at boot from that pair. Out of scope for the drop-in guard;
  worth a measurement before anyone relies on full-shadow enablement.

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
own configured value. Compare through `sysctl_values_equal` in
`crates/collect/src/inspectors/kernelboot.rs`, which compares
whitespace-split token sequences; keep the stored `runtime` and `default`
strings raw, since they are the snapshot contract. Note also
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

### Backgrounding the server over ssh leaks the session

This one is silent and costs nothing until you notice it eight hours
later. Measured 2026-11-01.

```
ssh host 'nohup inspectah refine … --port 8755 & sleep 5; curl …'
```

**does not return.** Redirecting the server's stdout and stderr is not
enough: the backgrounded process still inherits the ssh session's
descriptors, so sshd holds the connection open waiting for them to
close. The local `ssh` sits there indefinitely and the refine server
stays up on the guest holding its port.

What it looks like from outside: an agent that reported success hours
ago but whose harness entry never goes idle, because it still has a
live child. The run itself was fine — only the teardown leaked.

Use `ssh -n`, or `setsid`, or close the descriptors explicitly:

```
ssh -n host 'setsid nohup inspectah refine … > /tmp/refine.log 2>&1 < /dev/null & disown; sleep 5; curl …'
```

And check before you finish: `pgrep -af "inspectah.*refine"` on the
guest, `ps -eo pid,etime,command | grep "[s]sh <host>"` on the Mac.
`--fresh` is a second reason to look — it prompts
`Discard N saved operations? [y/N]` and quits on EOF, so a scripted run
can die with `curl` exit 7 and an empty log.

## Verify every COPY source before blaming the build

Two export-contract defects were found this way, and the second was
invisible in the build log because an earlier step failed first:

```
grep '^COPY ' Containerfile | sed "s/^COPY //" | while read -r line; do
  src=$(printf '%s' "$line" | awk '{print $1}' | sed "s/^'//; s/'$//")
  [ -e "$src" ] || echo "MISSING $src"
done
```

Run this against an extracted export before `podman build`. It names
every missing source at once, where the build names only the first.

The quoting matters: an earlier revision of this file used
`tr -d "'"`, which under a layer of shell quoting deletes literal
characters out of the hashes in `language-packages/<eco>/<hash>/` and
reports four missing files that are all present. Check a "MISSING" path
against `ls` before believing it.

**Both defects are fixed** — `quadlet/` is in the refine export and the
language-package manifests are written by both paths — and
`crates/cli/tests/copy_source_exists_test.rs` now holds the invariant.
Why the two paths could disagree at all, and what the guard does not
cover, is in
[export-paths-stage-different-trees](export-paths-stage-different-trees.md).

## A redaction test that checks for the secret proves nothing about the file

The auth scrubber doubled the quotes around every URL it rewrote, so a
redacted export's `package-lock.json` was unparseable on every
`"resolved"` line. `npm ci` reports that as a missing or too-old
lockfile, which points nowhere near the export, and the build dies at
that step after fifteen successful ones.

It shipped past a test over exactly that path. The test asserted the
credential was gone and `REDACTED@host` survived, and both were true of
`""https://REDACTED@host/…""`. **Assert that the staged file still
parses**, in the format the package manager will read it in, and only
then assert what was removed. The same reasoning covers any scrubber
that rewrites structured text in place.

The first unmodified export to build end to end on a real host,
`29/29` with `bootc container lint` passing, came after that fix. Two
warnings from the lint, `nonempty-run-tmp` and `var-tmpfiles`, are
about content the base image and the build's own `dnf` leave behind,
not about anything the export wrote.

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

## An unprivileged scan over-reports `/var` directories

The `/var` directory population differs by privilege in both directions.
On RHEL 9.8 aarch64 (measured 2026-10-31), unprivileged
`find /var/{lib,log,cache} -mindepth 1 -maxdepth 2 -type d ! -empty`
reported 43 non-empty directories where root reported 32. Root sees three
the unprivileged user cannot confirm. The unprivileged run reports eight
or more that root does not: `find` cannot read them, so `-empty` is false
and they survive `! -empty`. Run the control scan as root, or the count is
not comparable.

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
