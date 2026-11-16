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
  `/etc/systemd/system/sshd.service`. Measured on 2026-11-13: systemd
  loads the `/etc` file and the link target does not matter. See
  [§ Measured, 2026-11-13](#measured-2026-11-13-systemd-unit-state-links-masks-and-build-time-systemctl).

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
`crates/core/src/types/sysctl/parse.rs`, which compares
whitespace-split token sequences; keep the stored runtime values and the
file content raw, since they are the snapshot contract.

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
A refine server run on the Mac cannot be checked with `pgrep -af`: on
macOS, `pgrep -a` means "include ancestors", and it matched an unrelated
process. Use `lsof -iTCP:<port> -sTCP:LISTEN` there.
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

### Measured, 2026-11-08: disabled-repo and repo-less exports build

This is the first `podman build` of the disabled-repo and repo-less
render paths, at `f85f4297`, on RHEL 9.8 aarch64 against
`rhel-bootc:9.8`, after a standard drift run. Five contexts were built:
the scan tarball, an untouched refine export, one with `nginx` excluded,
one with an uploaded RPM, and a scan with a cached repo-less RPM. All
five passed with no `COPY` source missing. Each ended in
`bootc container lint` with the two known warnings. `--layers=true` was
not needed this time, but the proxy workaround still was.

**Planting a disabled-repo package.** On EL9, `epel-release` ships
`epel-cisco-openh264.repo` with its main section enabled, so a standard
drift run leaves the repo already enabled:

```
sudo dnf install -y openh264
sudo sed -i '0,/^enabled=1/s//enabled=0/' /etc/yum.repos.d/epel-cisco-openh264.repo
```

The scan then renders `openh264` in its own block, with
`--enablerepo=epel-cisco-openh264`, and the build log shows it resolved
from that repo. The edit makes the `.repo` file an RPM-modified config,
and the image keeps it disabled. Installing `epel-release` after the
repo `COPY` leaves the `.rpmnew` that the Containerfile comment
predicts.

**Planting a cached repo-less RPM.** The collector looks for cached RPMs
with `find /var/cache/dnf -name '*.rpm'`, so a planted copy anywhere
under that tree counts:

```
dnf download dos2unix
sudo dnf install -y ./dos2unix-*.rpm            # from_repo @commandline
sudo mkdir -p /var/cache/dnf/local/packages && sudo cp dos2unix-*.rpm /var/cache/dnf/local/packages/
```

With both plants in place, the scan tarball's single combined step is
`dnf install -y --enablerepo=epel-cisco-openh264 openh264 /tmp/dos2unix-*.rpm`.
The build resolved both packages in one transaction (`Install 2
Packages`, one from `@commandline` and one from the disabled repo).

**Uploading over the API.** The multipart field must be named `file`.
The response says whether the RPM matched a package:

```
curl -X POST localhost:8642/api/upload-rpm -F "file=@openh264-2.3.1-1.el9.aarch64.rpm"
# {"match_kind":"exact","matched":"openh264.aarch64","status":"matched",...}
```

A matched upload bumps the generation. Read it again from `/api/view`
before calling `/api/tarball`. An RPM for a package with no repo-less
annotation comes back `"status":"unmatched"` and changes nothing.
`epel-release` installed from a URL is such a package, although it is
`@commandline`. A matched upload of a disabled-repo package replaces its
`--enablerepo` install with a `COPY` and a `/tmp` path.

**The drift proxy reaches plain ssh commands too.**
`/etc/profile.d/proxy.sh` is sourced for non-interactive ssh shells, so
`dnf download` as the login user fails with
`Could not resolve proxy: proxy.internal`. Run it under the same
`env -u HTTP_PROXY -u HTTPS_PROXY -u NO_PROXY` as the build.

**A remote `pkill -f` matches its own shell.** `pkill -f "inspectah refine"`
inside `ssh host '...'` also kills the `bash -c` that carries the
pattern, so nothing after it runs. Use `pgrep -a inspectah`, which
matches the process name, to check, and run the kill on its own.

Timings for a full build of the drifted export, measured on the guest
with `date +%s`: 337 s to 476 s per context, run one at a time.

### Measured, 2026-11-08: the dependency tree after the provides-index fix

Rescanned at `c397f274` on RHEL 9.8 aarch64 (rpm 4.16.1.3) after a
standard drift run.

**The provides-index query works on EL9.** On a clean guest,
`rpm -qa --queryformat '[%{=NAME}.%{=ARCH}\t%{PROVIDENAME}\n]'` exits 0
and prints 35,877 lines, two tab-separated fields each, in 69 ms. Every
installed package appears in it. Piped into `head`, rpm reports 141
(SIGPIPE), which is not a failure.

**The dnf fallback is silent, so check it with exec auditing.**
`classify_leaf_auto` falls back from rpm to dnf without a log line, and
the scan log looks the same either way. Wrap the scan in an audit rule
and read the execs afterwards:

```
sudo auditctl -a always,exit -F arch=b64 -S execve -F auid=1000 -k inspscan
sudo ./inspectah scan -y --progress flat -o ~/scan.tar.gz
sudo auditctl -D
sudo sh -c 'cat /var/log/audit/audit.log.* /var/log/audit/audit.log'
```

The rpm path shows one `rpm -q --queryformat @@%{NAME}...` call with
every added package as arguments, then one `rpm -qa` provides index. The
fallback shows one `dnf repoquery --requires --resolve --recursive
--installed` per added package. Three traps:

- auditd hex-encodes any argument that contains a tab, a newline or a
  brace, so `grep PROVIDENAME` finds nothing. Decode the `aN=` fields
  before you search them.
- A scan runs about 1,500 `readelf` and `file` execs, which rotated an
  8 MB `audit.log` during the scan. Read the rotated files too.
- `ausearch -k <key>` returned `<no matches>` here while the raw log
  held the records. Read the raw log.

**Planting a non-leaf package that owns an enabled unit:**
`sudo dnf install -y pcp-zeroconf`. Its `%post` enables `pmcd`, `pmie`
and `pmlogger`, which are owned by `pcp`, a hard requirement of the
leaf. A refine export with `pcp-zeroconf` included keeps all three
units. Excluding it as well omits them. Both exports build. `cockpit`
does not work for this: its enable-able unit is `cockpit.socket`, and
the services inspector collects only `--type=service`, so a socket
never reaches the snapshot.

**What the tree still leaves out.** It records hard requirements only.
File requirements (`/usr/bin/perl`) and rich dependencies
(`(cockpit-ws-selinux = ... if selinux-policy-base)`) are dropped when
the requires are parsed, and weak dependencies were never in it. dnf
installs all of these in the image anyway. On a cockpit host, the
export printed `# Omitted: iscsi-starter.service` while the image held
`iscsi-initiator-utils` through `cockpit-storaged`'s Recommends. Check
the built image before reading an `# Omitted:` comment as proof that a
package is absent.

### Measured, 2026-11-09: weak, file and rich edges, on EL9 and EL10

Rescanned at `1be5bd44` on RHEL 9.8 (rpm 4.16.1.3) and RHEL 10.2
(rpm 4.19.1.1), both aarch64, after a standard drift run plus `cockpit`
and `pcp-zeroconf`. Both rpm versions accept the tagged
`[R\t…][W\t…][S\t…]` requires query. The exec audit shows it once, then
the provides index once, with no `dnf repoquery --requires` after it.
On each host, all 28 leaf closures matched the libsolv check below.

**Planting on EL10.** The driftify standard profile runs on RHEL 10.
The `openh264` disabled-repo plant does not: EPEL 10 has no
`epel-cisco-openh264.repo`, and dnf finds no `openh264`. `pcp-zeroconf`
behaves differently on EL10. It hard-requires `pcp-pmda-bpftrace`, and
`bcc` Recommends `kernel-devel`, so the install pulls 94 packages,
including `gcc`, `gcc-toolset-15-*`, `clang-libs` and Perl. Its leaf
closure is 110 packages, against 10 on EL9. That is correct, not a
collapsed tree. Expect it before reading a large closure as a defect.

**Weak dependencies reach the image.** `rhel-bootc:9.8` and `:10.2`
both report `install_weak_deps` as `True`
(`podman run <image> python3 -c 'import dnf; print(dnf.Base().conf.install_weak_deps)'`).
So a Recommends edge in the tree is a package the build installs.

**An independent closure check, with no rpm parsing of our own.** Run
it on the guest under `sudo python3 -I`. The dnf API resolves
capabilities and file paths through libsolv:

```python
import dnf
b = dnf.Base(); b.fill_sack(load_system_repo=True, load_available_repos=False)
inst = b.sack.query().installed()
providers = {f"{p.name}.{p.arch}" for p in inst.filter(provides=cap)}
# per package: p.requires, p.recommends, p.supplements are reldep lists
```

Build edges from those lists with the inspector's rule, walk from each
leaf, and filter the result to the auto set. Rich expressions come back
as strings starting with `(`, and you evaluate them yourself. That part
of the check is therefore not independent. `p.reason` (`group`,
`dependency`, `weak-dependency`, `user`) tells you which of the
"needed by no leaf" packages are installer comps installs, and which
are their dependencies.

**A self-conditional rich dependency is a version floor, not an edge.**
`(foo >= 1.2 if foo)` never makes dnf install `foo`. At `1be5bd44` the
tree evaluated the condition, found `foo` installed and credited it. The
tree now holds back a provider only when the condition holds through it
alone (see [rpm-output-field-parsing](rpm-output-field-parsing.md)).
Neither host hit the old behaviour: every such dependency on an added
package targeted a baseline or uninstalled package (`selinux-policy`,
`fedora-release-common`). A rescan that wants to exercise it needs an
added package whose condition names another added package. The next
subsection confirms the reworked four-field provides index on rpm 4.16
and 4.19.

### Measured, 2026-11-09: the reworked tree, and the versioned provides index

Rescanned at `e55a45f7` on the same two hosts, with the same drift and
plants. **The four-field provides index works on rpm 4.16.1.3 and
4.19.1.1.** On a clean guest it exits 0 and prints exactly as many
lines as the old two-field query (35,877 on EL9, 38,080 on EL10), each
with four fields, in 77 ms and 146 ms. Under `head` it reports 141. The
flags print as decimal integers, and an unversioned provide prints an
empty version. Expect bits other than the comparison ones:

| flags | meaning | version |
|---|---|---|
| `8` | `EQUAL`, the package's own `name = evr` | set |
| `32768` | `FIND_PROVIDES`, an auto-generated soname such as `libc.so.6()(64bit)` | empty |
| `32776` | `FIND_PROVIDES` and `EQUAL`, a generated provide with a version | set |
| `268435464` | `CONFIG` and `EQUAL`, the `config(<pkg>)` provides | set |
| `0` | explicit unversioned provide, such as bash's `/bin/sh` | empty |

No line on either host had a version without a comparison bit. The exec
audit showed the index once per scan, with no `dnf repoquery --requires`
after it.

Both leaf trees came out identical to the `1be5bd44` run, closure for
closure, and still matched the libsolv check on 28 of 28 leaves. The
check now mirrors the rework: libsolv resolves each versioned atom, and
the script subtracts an `if` condition's witnesses and builds
Supplements trigger clauses as `DepExpr` did at that commit. The tree
has since stopped subtracting every witness (it holds back a provider
only when the condition fails without it), so a later run's script has
to re-evaluate the condition without each provider instead. On these hosts none of
the new rules changed an edge. No Supplements became a joint rule,
because `glibc` is in the base image and drops out of the langpack's
`and`. No self-conditional dependency targeted an added package. Every
versioned rich dependency picked the same added packages with and
without its version. To exercise those rules, a host needs an added
package in each role.

### Measured, 2026-11-09: the not-carried class, scan to build

Run at `af96710c` on the same two hosts. Each was reverted to its clean
snapshot, given the drift and plants of the rescan above, and scanned as
root. Package counts reproduce that rescan:

- RHEL 9: 235 added, 28 leaf, 207 auto;
- RHEL 10: 262 added, 28 leaf, 234 auto.

The expected set is the rescan's list of auto packages that no leaf
needs.

**The scan's not-carried list equals the expected set**, by `name.arch`:
38 packages on RHEL 9 and 24 on RHEL 10. The Containerfile block and the
audit table carry the same count. Against the `e55a45f7` scan, every
disposition is the same except those packages, which are now excluded.
Owners of an enabled unit lead the list, then the rest, each part by
name: `cronie`, `firewalld`, `insights-client` (RHEL 9 only) and
`rsyslog`.

**`audit` is not in the class here.** The planted `cockpit` pulls it in,
through `setroubleshoot-server` in the built image, so it stays included
and every image has it. To see `audit` in the list you need a host
without `cockpit`.

**No unit was omitted as not carried.** Every unit the class owns was
enabled at its preset, so it has no `state_changes` row, and no
`systemctl` line exists to drop. Neither path wrote a `# Omitted:` line
on either host. To exercise the `NotCarried` reason, a not-carried
package needs a unit enabled against its preset, such as `rhcd` on
RHEL 9.

**What moved from the `e55a45f7` scan tarball.** The Containerfile gains
only the block. The install lines were already leaf names only, so they
did not change, and neither did the export's file tree.
`inspectah-var.conf` gains `/var/lib/rsyslog` on both hosts and
`/var/log/insights-client` on RHEL 9, each with a comment naming the
creator the export does not carry.

**Scan tarball against an unchanged refine export.** These are the
permitted differences seen:

- `epel-release` is installed by the RHEL 9 scan and left out of the
  RHEL 9 refine export. On RHEL 10 it has no repo source, so both paths
  render it as a manual step.
- `sshd.service` is enabled in the refine export on both hosts.
- `inspectah-var.conf` is identical on both paths.

**One difference is outside the permitted list, on both hosts.**
Refine's list has one more entry, `dnf-plugins-core.noarch` (39 and 25).
Its only puller in `leaf_dep_tree` is `epel-release`. Refine excludes
that package as repo-less pending an upload, so nothing included pulls
`dnf-plugins-core` in. The refine images do not have it, so the refine
list is right.

The RHEL 10 scan is the side that is off. There, `epel-release` routes
repo-less and stays included as the manual step, so `dnf-plugins-core`
counts as pulled in. The image installs neither package, so the scan
neither installs `dnf-plugins-core` nor lists it. On RHEL 9 the scan
installs `epel-release` from the copied repo, and the image has both.

When you compare the two snapshots, two more things look like
differences and are not part of this change:

- Refine's snapshot excludes every pulled-in auto package (169 and 210),
  while the scan includes them. The install lines still agree, because
  both paths render leaf names only.
- The refine export carries nine more `rpm_owned_modified` config files,
  which the scan excludes by default. The scan's file tree is unchanged
  from `e55a45f7`. One of the nine is `/etc/crontab`, from the
  not-carried `crontabs`, so the refine image ships a crontab with no
  cron.

**COPY sources and builds.** The COPY-source check was clean on all four
exports.

- **RHEL 9:** both builds passed, in 498 s and 416 s, and ended in
  `bootc container lint` with the two known warnings.
- **RHEL 10:** both builds failed at the package install, because
  `RPM-GPG-KEY-EPEL-10` is missing.

The RHEL 10 cause predates this change. EL10's `epel.repo` writes
`gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-$releasever_major`.
`extract_gpg_keys` in `crates/collect/src/inspectors/rpm/repos.rs` does
not expand dnf variables, so the read fails and the key is skipped
without a warning. The snapshot then has no `gpg_keys`, and the export
has no `COPY config/etc/pki/rpm-gpg/`. EL9's file names the key
literally, so EL9 is unaffected. With the host's key added by hand to a
copy of each context, under that one `COPY` line, both RHEL 10 exports
built, in 324 s and 242 s. Both now build unmodified: see
[§ Measured, 2026-11-11: gpgkey= dnf variables, scan to build](#measured-2026-11-11-gpgkey-dnf-variables-scan-to-build).

**The images.**

- `rpm -q firewalld audit cronie` reports `firewalld` and `cronie` not
  installed in all four images, and `audit` installed, as above.
- The units of the class are not found.
- `sshd`, `httpd`, `nginx`, `pmcd`, `pmie`, `pmlogger` and `tuned` are
  enabled, as are `iscsi-starter` and `NetworkManager-wait-online` on
  RHEL 9. `bluetooth` and `kdump` are masked.
- Every `systemctl` step succeeded.

**Checking the call-site order needs an auto repo-less package.** With
the standard plants, every repo-less or disabled-repo package
(`epel-release`, `openh264`) is a leaf, and leaves are never in the
class, so the main scans cannot tell the order. After the builds, on
RHEL 10:

```
dnf download dos2unix                    # under the proxy workaround
sudo dnf install -y ./dos2unix-*.rpm
sudo mkdir -p /var/cache/dnf/local/packages && sudo cp dos2unix-*.rpm /var/cache/dnf/local/packages/
sudo dnf mark remove dos2unix            # now auto, from_repo @commandline
```

The rescan has `dos2unix.aarch64` in `auto_packages`, needed by no
leaf, annotated as a cached repo-less RPM, and included. The scan
installs it with `COPY repoless-packages/...`, and the not-carried
block is byte-identical to the main scan's, with 24 entries. Classified
before the repo-less pass, it would have been in the class.

### Measured, 2026-11-11: gpgkey= dnf variables, scan to build

Run at `403843fd` on RHEL 9.8 and RHEL 10.2, both aarch64, each reverted
to its clean snapshot, given a standard drift run, and scanned as root.
No other plants.

**Both RHEL 10 exports build unmodified.** The scan collects
`RPM-GPG-KEY-EPEL-10` once per section that names it (six: three in
`epel.repo`, three in `epel-testing.repo`), each with
`gpgkey_references` holding the literal `...-EPEL-$releasever_major`.
The scan Containerfile copies the key and installs `epel-release` from
the Repo-enabling block with no "does not carry" FIXME, the audit says
`yes` under Carried by Export, and refine reports `epel` as `verified`
and accepts excluding it. The scan build and an unchanged refine export
both passed, in 471 s and 400 s, with `Importing GPG key 0xE37ED158 ...
From: /etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-10` in each log. Both ended in
`bootc container lint` with three warnings, `nonempty-boot`,
`nonempty-run-tmp` and `var-tmpfiles`: `rhel-bootc:10.2` adds the first.

The scan image has `epel-release` and now `dnf-plugins-core` with it, as
a weak dependency. That closes the 2026-11-09 gap where the RHEL 10 scan
counted `dnf-plugins-core` as pulled in but installed neither. The
refine export excludes `epel-release` by default on both majors, so its
image has the key and the EPEL packages (`htop`, `bat`, `fd-find`) but
not `epel-release`.

**RHEL 9 is unchanged.** EPEL 9 names its key literally, so its nine
entries carry no reference. Both builds passed, in 668 s and 476 s, with
the two known lint warnings.

**RHEL 10.2 runs dnf4, not dnf5.** `dnf --version` prints `4.20.0` on
its first line (RHEL 9.8: `4.14.0`), `/usr/bin/dnf` links to `dnf-3`,
and no `/usr/share/dnf5/vars.d` exists. So the dnf5 empty-`releasever_major`
question does not arise on this release. On both hosts `redhat-release`
provides `system-release(releasever)`, `(releasever_major)` and
`(releasever_minor)` with flag `8` (`EQUAL`), as `10`, `10`, `2` and `9`,
`9`, `8`, and dnf's own values match the expansion. `dnf config-manager
--dump-variables` is the dnf4 dump. On RHEL 9.8 it leaves out
vars-file variables; read them through the dnf API instead:

```
sudo python3 -I -c 'import dnf; b=dnf.Base(); b.conf.substitutions.update_from_etc("/"); b.read_all_repos(); print({r.id: list(r.gpgkey) for r in b.repos.values()})'
```

`update_from_etc` is the step that reads the vars directories. Without
it the API leaves `$name` unexpanded, which looks like dnf ignoring the
file.

**Probing the vars directories.** A disabled repo file with one section
per case, `gpgkey=` naming `RPM-GPG-KEY-$gkprobe-$releasever_major`
(`/etc/yum/vars/gkprobe` holding `EPEL`), the same through
`${gkprobe2}` in `/etc/dnf/vars`, `$nosuchvar`, and `/etc/os-release`.
On both hosts the scan resolved the first two to the EPEL key, as dnf
did, which confirms the dnf4 directory list. It warned for the other
two with the documented sentences, in README.md and the audit report.
`dnf5` is not installable on RHEL 9.8 from BaseOS, AppStream or EPEL 9
(`No match for argument: dnf5`), so the dnf5-beside-dnf4 case was not
run.

**Every stock RHEL host carried a false GPG warning in this run.**
`RPM-GPG-KEY-redhat-release` opens with several lines of prose before
its first `-----BEGIN PGP PUBLIC KEY BLOCK-----` (line 10 on RHEL 9.8,
line 12 on RHEL 10.2), and `validate_gpg_content` tested `starts_with`
after trimming whitespace. So the collector had always excluded that
key as not PGP, and printed one stderr line per `redhat.repo` section
(500 on RHEL 9.8, 209 on RHEL 10.2). Since `403843fd` it also wrote a
snapshot warning telling the operator to COPY the key in. The file is a
real key file holding several key blocks, and the base image ships a
copy of the same size, so the builds were unaffected: each image lists
the Red Hat release keys as imported `gpg-pubkey` packages. The
collector now accepts a key file when any line begins with the header,
so the stock key is collected.

**Rescan at `9974f5f9`: one entry per key, the stock key left to the
base image.** Same hosts, reverted, standard drift, scanned as root. Each
snapshot now holds two `gpg_keys` entries where the `403843fd` run held
215 on RHEL 10.2 and 509 on RHEL 9.8. The stock key is recorded once,
excluded, with `shipped_by_base: redhat-release` (`rpm -V redhat-release`
exit 0 on both, `rpm -qf` names `redhat-release`), with its full content
(20700 bytes on 10.2, 3682 on 9.8). EPEL's key is recorded once,
included, owned by `epel-release`, which the base lacks; on 10.2 its
`gpgkey_references` holds the `$releasever_major` literal. No GPG warning
in the snapshot and no `RPM-GPG-KEY` line on stderr. Snapshot size went
from 5,205,871 to 5,139,765 bytes on 10.2 and from 5,336,264 to
5,138,878 on 9.8; the version between the two, which collected the stock
key once per section with its content, was never run here. Both
Containerfiles read `GPG Keys (1)`, the directory `COPY`, and
`# Not copied: /etc/pki/rpm-gpg/RPM-GPG-KEY-redhat-release (the base image
ships it (redhat-release, unmodified on the source host))`; the audit has
a `GPG Keys Not Copied (1)` table with the same reason, and the staged
`config/etc/pki/rpm-gpg/` holds only the EPEL key. The unchanged refine
export carries the same section (HTTP 200, `x-export-warnings: []`).

All four builds passed with the COPY-source check clean: RHEL 10.2 scan
488 s and refine 373 s (lint `nonempty-boot`, `nonempty-run-tmp`,
`var-tmpfiles`), RHEL 9.8 scan 644 s and refine 458 s (the last two).
Every image imports the EPEL key (`0xE37ED158` on 10, `0x3228467C` on 9)
and the Red Hat keys from the base image's file, and the image's
`RPM-GPG-KEY-redhat-release` has the base image's sha256 (`d5ddf4a0...` on
10.2, `0db3dc1b...` on 9.8, each also the host's). The scan images have
`epel-release`; the refine images do not, as before.

On RHEL 9.8 the drift run dropped the ssh session that started it
(`Read from remote host ...: Operation timed out`) while driftify carried
on. Start a drift-then-scan run on the guest with `ssh -n ... setsid
nohup`, as for refine, and wait on a marker file, not on the session.

### Measured, 2026-11-12: sysctl carriage, scan to build

Run at `97b4da13` on RHEL 9.8 and RHEL 10.2, both aarch64, each reverted
to its clean snapshot and given a standard drift run. tuned was active
with `throughput-performance` on both. Every criterion passed on both
hosts.

**What the drift run gives you, and what it does not.** It writes
`/etc/sysctl.d/99-driftify.conf`, `hugepages.conf` and `thp.conf`. Only
`vm.compaction_proactiveness` drifts (`thp.conf` asks for `0`, the
kernel runs `20`). `vm.nr_hugepages` came up at `128` on both guests, so
it did not drift. Four cases had to be planted by hand, in three passes
on the same guest, each scanned:

```
# pass A
echo 'kernel.sysrq = 1' | sudo tee -a /etc/sysctl.conf    # not applied: drifts against 50-default.conf's 16
sudo ln -s /dev/null /etc/sysctl.d/50-coredump.conf       # mask over a vendor file
echo 'kernel.pid_max = 4194304' | sudo tee /etc/sysctl.d/50-pid-max.conf   # same-name shadow
# pass B
sudo ln -sfn /dev/null /etc/sysctl.d/99-sysctl.conf       # rpm -Va: ....L....
# pass C
echo 'db_password = canary-realhost' | sudo tee /etc/sysctl.d/80-app.conf
```

They go in separate passes because each later plant changes the whole
explanation: the mask on `99-sysctl.conf` turns the `/etc/sysctl.conf`
edit off, and a redacted file makes values and drift unknown. Both hosts
ship `50-coredump.conf` and `50-pid-max.conf` in `/usr/lib/sysctl.d`, and
`rpm -qf` names `systemd-udev` for both `/etc/sysctl.conf` and the
`99-sysctl.conf` link. RHEL 10.2 adds `10-map-count.conf`.

**What passed.**

- Pass A: two drift lines (`kernel.sysrq`, sourced through the
  `99-sysctl.conf` link with the link note, and `vm.compaction_proactiveness`)
  and two vendor-replacement lines (`disables` and `replaces`). No table
  row is sourced from a masked or shadowed vendor file. The mask is staged
  as a 0-byte regular file, and the other files with the host's sha256.
  An `item_id` of kind `Sysctl` gets HTTP 400 and the generation does not
  move. Excluding `/etc/sysctl.conf` in refine lists `99-sysctl.conf` as
  not carried, `excluded` via `/etc/sysctl.conf`, and the next export
  drops the file.
- Pass B: one `config.files` entry for `99-sysctl.conf`, RPM-owned
  modified with flags `....L....` and empty content. It is staged as a
  0-byte file. Nothing in the sysctl block names it, and `kernel.sysrq`
  falls back to `50-default.conf`.
- Builds: the scan tarball and an unchanged refine export built on both
  hosts with the COPY-source check clean. RHEL 9.8 took 590 s and 409 s,
  with the two known lint warnings. RHEL 10.2 took 427 s and 323 s, with
  the three. In every image, `systemd-sysctl --cat-config` reads
  `/etc/sysctl.d/50-coredump.conf` and `50-pid-max.conf` in place of their
  `/usr/lib/sysctl.d` namesakes, which stay in the image from the base.
  `kernel.core_pattern` appears nowhere in its output, and the 0-byte
  `99-sysctl.conf` is read as empty.
- Pass C: the canary is in no file of the tarball, the HTML report or
  `/api/view`. The explanation is unknown, with one `redacted_file` input.
- An ancestor link: with `/usr/local/lib/sysctl.d` a link to a directory
  under `/var`, its file is recorded as `outside_link` with the `/var`
  target and its content is not read. Values and drift become unknown, and
  the file is listed as not carried (`usr_local_lib`). The planted
  directory also shows up as an unbacked `/var` directory, which is the
  plant, not a defect.

**For the next run.**

- RHEL 10.2 build logs print
  `%transfiletriggerpostun(info-...) scriptlet failed` and
  `Error in <unknown> scriptlet in rpm package libipa_hbac` during the
  package install, and the build still succeeds. This is not caused by
  the export.
- A redacted sysctl file is staged with its placeholder line
  (`REDACTED_PASSWORD_6`), which systemd-sysctl will not read as an
  assignment. The pass C export was not built.
- `secrets-review.md` lists the redacted sysctl file twice, with identical
  rows.
- Two shell traps. zsh reads `$h:t15/...` as a history modifier, so write
  `"${h}:t15/..."`. podman refuses an upper-case image tag.

### Measured, 2026-11-12: the scan's not-carried notice on stock hosts

Run at `a4d93647` on RHEL 9.8 and RHEL 10.2, both aarch64, each reverted
to its clean snapshot and scanned as root with **no drift**. Every check
passed on both hosts: the `Notice:` line is the last stderr line in flat,
pretty and `--quiet` modes, stdout is empty, `--inspect-only` prints no
notice and valid JSON, and the exit status is 0.

**A stock host lists five owners, not the three a drifted host lists.**

| host | not carried | owners of an enabled unit |
|---|---|---|
| RHEL 9.8 | 51 | `audit`, `cronie`, `firewalld`, `insights-client`, `rsyslog` |
| RHEL 10.2 | 35 | `audit`, `audit-rules`, `cronie`, `firewalld`, `rsyslog` |

So the line names three and ends `and 2 more` on both. Nothing a stock
host installs pulls `audit` in; on a drifted host the planted `cockpit`
does (see the 2026-11-09 run). The owners lead the Containerfile block
and the audit table in the same order, both lists are identical, and the
count matches the rows with an enabled unit. An unchanged refine export
of the RHEL 10.2 tarball sent the same text as its only
`x-export-warnings` entry.

**Capturing pretty mode without a local TTY.** An agent's shell has no
terminal, so `ssh -t` allocates none. Run util-linux `script` on the
guest instead, which gives the scan a pty there:

```
script -q -e -c 'sudo ./inspectah scan -y -o ~/tty.tar.gz' tty.typescript
```

Read the typescript with `cat -v`; the spinner frames come before the
summary.

### Measured, 2026-11-13: systemd unit state, links, masks and build-time systemctl

No inspectah build was involved. Measured on RHEL 9.8 (systemd 252) and
RHEL 10.2 (systemd 257), both aarch64, stock and after a kitchen-sink
drift run, with planted units, and in `podman build` from
`rhel9/rhel-bootc:9.8` and `rhel10/rhel-bootc:10.2`. Each fact holds on
both releases unless it names one.

**What `list-unit-files` and `is-enabled` report.**

- Both releases print the preset column.
- Template instances get no row. With `wg-quick@wg0` enabled, the
  template reads `wg-quick@.service indirect disabled` and `is-enabled`
  reads `enabled` for the instance. A template whose `DefaultInstance=`
  is the enabled instance reads `enabled` (`getty@.service` with
  `getty@tty1`). A concrete instance file, such as
  `/etc/systemd/system/probe@site.service`, gets its own row.
- `indirect` also covers a unit whose `[Install]` holds only `Also=`
  (`sssd-kcm.service` and its siblings, stock on both), and a unit whose
  only link is an alias its `[Install]` does not declare. The services
  inspector drops `indirect` without a warning, so an enabled instance
  never reaches a snapshot through this column.
- A hand-made link into another target's `.wants` makes an installable
  unit `enabled` in both commands. The same link to a unit with no
  `[Install]` makes it `enabled` with preset `disabled`, where it read
  `static` before. A declared `Alias=` link left alone after its wants
  link is removed still reads `enabled`.
- A `/dev/null` link and a zero-length regular file both read `masked`
  (`LoadState=masked`). A file holding only a comment line, or only
  blank lines, is **not** a mask. It reads `enabled` with
  `LoadState=bad-setting`, and it replaces the vendor unit with a broken
  one. Test for size zero, not for "no content after trimming".
- A concrete instance file under `/etc` wins over a template under
  `/usr/lib`: `FragmentPath` and `ExecStart` both come from it, and
  `enable` links the instance to its own file.
- `enable` reads `[Install]` from a drop-in, in
  `/etc/systemd/system/<unit>.d/` and in `/usr/lib/systemd/system/<unit>.d/`.
- `UpheldBy=` in `[Install]` works on systemd 257 only. RHEL 10 makes
  `<target>.upholds/<unit>` on `enable`. RHEL 9 prints the "no
  installation config" text, exits 0 and makes no link.
- `disable` removes only links under `/etc`. A unit a package pulls in
  through `/usr/lib/systemd/system/<target>.wants/` reads `disabled`
  afterwards and is still in that target's `WantedBy=`. `disabled` does
  not mean the unit will not start.

**Stock hosts against the base images.**

- `tmp.mount` is `disabled` against a `disabled` preset on both hosts,
  nothing pulls it in, and `/tmp` is not a separate mount. Both base
  images ship `/usr/lib/systemd/system/local-fs.target.wants/tmp.mount`,
  so on the target `/tmp` is mounted from that unit while `is-enabled`
  reads `disabled`. In an image only `mask` changes that. What `/tmp` is
  on a booted host with the mask was not measured.
- One unit both carry has a different preset on the host and in the
  base image, on each release: `dnf-makecache.timer` (`enabled` on the
  hosts, `disabled` in both images). Three more (`multi-user.target`,
  `systemd-remount-fs.service`, and `sshd@.service` on RHEL 10) read
  `static` in the image and `indirect` or `enabled-runtime` on the host.
  The RHEL 9 host also has `remote-cryptsetup.target`
  `disabled` against an `enable` preset, where the image has it
  `enabled`. That is a target that differs from preset on every stock
  RHEL 9 host, with no administrator involved.
- Both stock hosts report `systemd-remount-fs.service` as
  `enabled-runtime`, which the services inspector warns about.
- On a stock and a drifted host of each release, no socket, path, slice
  or storage unit differed from preset, one target did with no
  administrator involved (`remote-cryptsetup.target`, above), and the
  only enabled instance was `getty@tty1.service`. The only differing
  units that were not services were one local timer and
  `remote-cryptsetup.target`. Two clean installs and one drift tool are
  not a fleet.
- A package install enables some units through another unit's `Also=`.
  On RHEL 9, `NetworkManager-wait-online.service` and, once
  `device-mapper-multipath` is installed, `multipathd.socket` are
  enabled against a `disable` preset with no administrator involved.
- The packaged units that use `Also=` in the measured package set are
  vendor units with nothing under `/etc` but links.
- A package install in the image applies the preset and links the unit
  by the vendor `[Install]` (measured for `cups` and `libvirt-daemon`).
- An upheld unit is started as soon as its `.upholds` link exists and
  restarted whenever it goes inactive, so a `Type=oneshot` unit without
  `RemainAfterExit=yes` loops until it hits its start limit and leaves
  the host degraded on every boot. Measured on RHEL 10 (RHEL 9 makes no
  `.upholds` link, above).
- On a package-mode host with `tmp.mount` masked, after a reboot `/tmp`
  is a plain directory on the writable root filesystem.
- Layout of both base images: `/home`, `/mnt`, `/root` (to
  `var/roothome`) and `/srv` are links into `/var`. `/opt` and
  `/usr/local` are directories. `/tmp` and `/var/tmp` are mode 1777
  directories.

**`systemctl` in a `RUN` step, with no manager running.**

- `enable`, `disable`, `mask`, `set-default`, `add-wants` and
  `add-requires` work. `systemctl enable -- 'unit'` works, and an
  instance named with `\x2d` links as `probe@a\x2db.service`.
- `mask` of a unit absent from the image succeeds. `mask` fails with
  `already exists`, exit 1, when a regular file is at the `/etc` path,
  zero length included.
- `enable` of an absent unit exits 1. `disable` of an absent unit exits
  1 on RHEL 9, and on RHEL 10 prints the same error and **exits 0**.
- `add-wants` and `add-requires` of an absent unit exit 1. For a unit
  with an `/etc` full shadow they link to the `/etc` file.
- `enable` and `disable` follow `Also=` through a chain (a names b, b
  names c: all three change). `mask` does not follow it: only the named
  unit is masked. An `Also=` member absent from the image is skipped
  with `Failed to enable auxiliary unit ..., ignoring.` and exit 0. A
  masked member is skipped with `Unit ... is masked, ignoring.`
- `is-enabled` and `list-unit-files` work in a `RUN` step. `systemctl
  cat` and `show` fail with `System has not been booted with systemd`,
  and `systemd-analyze verify` prints nothing and exits 0. To ask the
  manager, run the built image with systemd as PID 1:
  `sudo podman run -d --systemd=always <image> /sbin/init`, then
  `podman exec`. That is a container, not a booted host, and podman
  mounts its own tmpfs on `/tmp` in that mode.

**A full shadow loads from `/etc` whatever its wants-link names.** A link
made by `enable` before the `/etc` copy lands points at `/usr/lib/...`
and still does after the `COPY`. Run with systemd as PID 1, the image
loads the `/etc` file (`FragmentPath=/etc/systemd/system/vshadow2.service`,
the administrator's `Description=`), and the unit is active, on both
releases. systemd resolves the name, so the link target does not matter.
`enable` after the `COPY` links to the `/etc` file.

**`Also=` on packaged daemons, and rendering links without `enable`.**
Measured the same day on clean hosts after installing a broad AppStream
daemon list, and in the base images after `dnf install cups libvirt-daemon`.

- With the daemons installed, 60 unit files on each release carry
  `Also=`. Every member is installable: none is static, absent or a
  timer naming its service. Most are the libvirt socket triples
  (`virtXd.socket`, `-ro`, `-admin`), which name each other, and each
  `virtXd.service` names its three sockets. `libvirtd.service` and
  `virtqemud.service` also name `virtlogd.socket` and
  `virtlockd.socket`, which belong to other packages.
- `disable` of a member follows that member's own `Also=`: disabling
  `virtqemud.socket` or `virtqemud-ro.socket` removes all three socket
  links.
- `disable` removes every link to the unit and to its `Also=` members
  under `/etc`, including hand-made links. A link a person made to a
  static member, or to an installable member in another target's
  `.wants`, is removed too. `disable libvirtd.service` removes the
  `virtlogd` and `virtlockd` socket links that `virtqemud.service`
  also names.
- An `enable` then `disable` pair, the unit's own command and then the
  opposite verb for each `Also=` member recorded on the other side,
  reproduced the host exactly for `cups.service` and `cups.socket`.
- The four template units of the instance tables, read on the installed
  hosts: `wg-quick@.service` runs `wg-quick up %i` and names no path
  under `/etc`. The OpenVPN templates (`openvpn-client@`,
  `openvpn-server@`) pass `--config %i.conf` with a
  `WorkingDirectory=`. `dirsrv@.service` names its
  `/etc/dirsrv/slapd-%i` directory and never names its data directory.
- `add-wants` and `add-requires` do not read `[Install]`. They create
  no `Also=` member link and no alias link. No verb creates an alias
  link without `enable`: `systemctl link` does nothing for a unit
  already in the search path, so the alias has to be an `ln -s`.
- `preset-all` puts every unit back to preset, whichever way it was
  rendered. `--preset-mode=enable-only` re-enables anything rendered off
  against an `enable` preset. Neither runs at first boot (next
  item).
- **First boot does not re-apply presets.** Both base images ship a
  zero-length `/etc/machine-id`. Installed with `bootc install
  to-existing-root` and booted, RHEL 10 logged `Initializing machine ID
  from random generator` and `first-boot-complete.target ... skipped
  because of an unmet condition check (ConditionFirstBoot=yes)`. A
  `systemctl disable`, an `rm` of a unit's links, a shipped preset file
  with `systemctl preset`, `mask` and `add-wants` all kept their
  build-time state through two boots. RHEL 9 showed the same unit
  states on its console. Only this install route was tried.
- **A masked `tmp.mount` makes `/tmp` read-only on a booted bootc
  host.** The base images pull `tmp.mount` in through a vendor link.
  With it masked, `/tmp` is the read-only composefs root's directory
  (`touch: ... Read-only file system`), `bootc status` fails with
  `Ensuring selinux install_t type: ... Read-only file system` (it works
  with `TMPDIR=/var/tmp`), and `bootc-publish-rhsm-facts.service` fails
  at boot. Measured on RHEL 10.2.
- **Booting a probe image on a test guest.** Run `bootc install
  to-existing-root` from the image with `podman run --privileged
  --pid=host -v /var/lib/containers:/var/lib/containers -v /dev:/dev -v
  /:/target --security-opt label=type:unconfined_t`, passing
  `--root-ssh-authorized-keys` with a copy of the key under `/root` (the
  guest's `/home` is a separate volume). Then log in as root with
  `-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null`, since
  the host keys are new. The installed system has no Parallels tools,
  so `prlctl list -f` shows no address, and `prlctl restart` is
  cancelled. On RHEL 9.8 the installed system never got an address,
  with or without an NM keyfile, and the cause was not found (probably
  the probe's `tmp.mount` mask: see
  [§ Measured, 2026-11-14: unit state at first boot](#measured-2026-11-14-unit-state-at-first-boot)). Bake a
  oneshot unit into the probe image that writes its measurements to
  `/var` and a summary to `/dev/tty1`, then read the summary with
  `prlctl capture <vm> --file x.png`. Revert the guest afterwards.
- A podman container booted with `--systemd=always` cannot stand in for
  a first boot: podman writes its own machine-id into it even when none
  is supplied, and mounts its own tmpfs on `/tmp`.
- Installing `podman-docker` on RHEL 10.2 upgraded podman to 6.1.1, and
  the next `podman build` failed at its first `RUN` with
  `netavark: failed to load network options: IO error: invalid type:
  sequence, expected a map`. `podman build --network=host` worked.
- zsh reads `$ip:table.tsv` as `$ip` with the `:t` modifier, so
  `scp $ip:file` copies a mangled path. Write `"${ip}:file"`.

**Two traps from this run.**

- Give each probe a name no earlier step touches. A `mask` of an absent
  unit leaves a `/dev/null` link, and a later `enable` that names the
  same unit through `Also=` reports it as masked. Wrap each probe as
  `RUN ...; ( probe ); echo "rc=$?"` so one failure does not stop the
  build.
- `ssh host 'a && b && setsid nohup c ... & disown'` backgrounds the
  whole `&&` list, so the session stays open until `c` ends. Put the
  `setsid nohup` command in an `ssh -n` call of its own. Killing that
  local `ssh` lets the rest of the local script run, which here started
  a second copy of a planting script on the same guest; revert before
  trusting what it measured.

### Measured, 2026-11-14: cockpit.socket end to end

Run R1 of the units spec, at `6220362e` on the units branch (`inspectah
version` on each guest printed `commit 6220362e`). RHEL 9.8 and RHEL
10.2, both aarch64, each reverted to its clean snapshot, given a
standard drift run, scanned as root, and the scan tarball built
unmodified with `podman build --layers=true` under the proxy workaround.
**The built image reads the host's state on both releases.**

**Neither the clean snapshot nor the base image has cockpit.** On the
clean guests `rpm -q cockpit-ws` reports it not installed and
`systemctl list-unit-files cockpit.socket` lists 0 unit files, so there
is no preset column to read before drift. Install it first with
`sudo dnf install -y cockpit`, which is the package the standard profile
installs anyway (driftify then prints `already installed`). In the base
images, `podman run --rm <base> systemctl is-enabled cockpit.socket`
answers `Failed to get unit file state ... No such file or directory`
(exit 1) on `rhel-bootc:9.8` and `not-found` (exit 4) on
`rhel-bootc:10.2`. So the comparison that can fail is against the layer
after `COPY config/etc/ /etc/`, the last one before the unit state
section: the build's package install has applied the preset there.

| | RHEL 9.8 | RHEL 10.2 |
|---|---|---|
| preset rule | none for cockpit; `99-default-disable.preset`: `disable *` | `90-default.preset:475`: `enable cockpit.socket` |
| after `dnf install cockpit`: `list-unit-files` state, preset | `disabled disabled` | `enabled enabled` |
| driftify's line | `cockpit.socket: preset disabled, setting enabled` | `cockpit.socket: preset enabled, setting disabled` |
| host after drift: `list-unit-files` | `enabled disabled` | `disabled enabled` |
| unmodified base image: `is-enabled` | no such file (not installed) | `not-found` |
| layer before the unit state section: `is-enabled` | `disabled` | `enabled` |
| built image: `list-unit-files` | `enabled disabled` | `disabled enabled` |
| built image: `is-enabled` | `enabled` | `disabled` |

In each case the image moved off the preset in the direction the host
did, so the run discriminates on both releases and no hand plant was
needed. The snapshot row is `current_state: enabled, default_state:
disable` on RHEL 9 and `disabled`, `enable` on RHEL 10, both `fragment:
vendor`, `owning_package: cockpit-ws`, included.

**The Containerfile lines.** On both, `# === Systemd Unit State ===`
follows `COPY config/etc/ /etc/`, opens with the `tmp.mount` FIXME, and
puts the socket after the services in its verb's list. RHEL 9:

```
RUN systemctl enable -- \
    ...
    'nginx.service' \
    'cockpit.socket' \
    'multipathd.socket'
```

The build printed `Created symlink
/etc/systemd/system/sockets.target.wants/cockpit.socket →
/usr/lib/systemd/system/cockpit.socket.` for that step. RHEL 10:

```
RUN systemctl disable -- \
    'cockpit.socket'
```

The build printed `Removed
'/etc/systemd/system/sockets.target.wants/cockpit.socket'.`, the link the
package install had made earlier in the same build.

**What else the scans showed.**

- Warnings: the snapshot holds one on RHEL 9 and two on RHEL 10. Both
  hosts have `unit systemd-remount-fs.service has state
  'enabled-runtime'`, and RHEL 10 adds the skipped ELF scan (`readelf`
  absent). Nothing on stderr is a warning. None names a `.scope` or
  `.device` unit, `NotADirectory`, or a `*.d` path. The only `.scope` and
  `.device` strings in the snapshot are man page paths and one unit
  file's `Conflicts=` line.
- Owner lookups: every `vendor` fragment in `units.state_changes` has an
  `owning_package` (9 of 9 on RHEL 9, 6 of 6 on RHEL 10), so no lookup
  failed. The scan prints no count of failed lookups: read from the
  collector, not measured, an unknown owner shows only as a null there.
- Unit state lines, by verb: RHEL 9 `enable` 9 and `mask` 2; RHEL 10
  `enable` 5, `disable` 1 and `mask` 2. `state_changes` holds 12 and 9
  rows; the third masked row in each is `tmp.mount`, which the FIXME
  covers.
- Both builds passed with the COPY-source check clean, in 346 s and
  288 s, with the known lint warnings (two and three).

**A hand-made link is not carried yet.** `driftify-static.service` is
`enabled` on both hosts through a hand-made `multi-user.target.wants/`
link to a unit with no `[Install]`. It is in the `enable` list, the
build printed no link for it, and the image reads `static` with no
link. Inferred, not measured: this is the expected phase 2 output, since
the plan gives hand-made links to phase 4.

### Measured, 2026-11-14: unit state at first boot

Run R3 of the units spec, on RHEL 9.8 (systemd 252) and RHEL 10.2
(systemd 257), both aarch64. Each case started from a guest reverted to
its clean snapshot, built a probe image on the guest as root, installed
it with `bootc install to-existing-root` as described in the 2026-11-13
section (with `--root-ssh-authorized-keys`), and rebooted with
`sudo systemctl reboot`. The probe is a `Type=simple` unit,
`After=multi-user.target`, that waits 25 s, writes `systemctl is-enabled`,
`is-active`, `FragmentPath`, `findmnt`, `systemctl --failed` and filtered
`journalctl -b` lines to `/var`, pages them on `/dev/tty1`, and powers the
guest off seven minutes later. Results were read over root ssh where the
installed system allowed it and from `prlctl capture` otherwise. Before
each reboot, the deployed `/etc/machine-id` was read on the old root at
`/ostree/deploy/default/deploy/*.0/etc/machine-id`.

**An image whose `/etc/machine-id` reads `uninitialized` gets a preset
pass at first boot.** Built from each base with
`systemctl disable chronyd.service` (preset `enable`), `systemctl enable
dnf-makecache.timer` (preset `disable`), `systemctl mask
irqbalance.service`, then `printf 'uninitialized\n' > /etc/machine-id`.
`to-existing-root` deployed that file unchanged (14 bytes). Both releases
logged `Detected first boot.`, `Populated /etc with preset unit
settings.` and `Reached target First Boot Complete`.

| after first boot | RHEL 9.8 | RHEL 10.2 |
|---|---|---|
| `chronyd.service`, disabled in the image | `enabled`, `active` | `enabled`, `active` |
| `dnf-makecache.timer`, enabled in the image | `enabled` | `disabled`, link gone |
| the probe, enabled in the image, no preset rule (`disable *`) | ran | `disabled`, never started |
| `irqbalance.service`, masked | `masked` | `masked` |

So RHEL 9 undoes a rendered `disable` that departs from preset and keeps
a rendered `enable`, and RHEL 10 undoes both. `mask` survives on both.
Inferred, not measured: the difference is each systemd's first-boot
preset mode.

**A zero-length `/etc/machine-id` gets no preset pass.** Every other
image here kept the base's zero-length file (deployed size 0), and every
boot logged `First Boot Complete was skipped because of an unmet
condition check (ConditionFirstBoot=yes)` and no preset line.
`bootc install to-disk --via-loopback --generic-image --filesystem xfs`
from `rhel10/rhel-bootc:10.2` to a 12 GB file wrote a 0-byte
`/etc/machine-id` into its deployment too (loop-mounted read-only). That
disk was not booted: the guests have one disk, so `to-disk` as a booted
route was not run. Anaconda was not run either: it needs the guest booted
from installer media with a kickstart and a registry the installer can
pull the probe from, and neither was set up.

**Rendered `enable`, `disable` and `mask` on RHEL 9 hold at first boot.**
Branch build at `6220362e`, standard drift run after `dnf install -y
cockpit`, plus `sudo systemctl disable irqbalance.service` by hand
because the standard profile renders no `disable` on RHEL 9 (it disables
`kdump` at minimal and masks it at standard). The scan rendered
`enable` (nine units, `cockpit.socket` among them), `disable --
'irqbalance.service'` and `mask -- 'bluetooth.service' 'kdump.service'`.
The scan tarball built unmodified (310 s, 348 s and 330 s on three runs)
and was installed. Read from the console on two runs and over root ssh on
the third, with the same result each time:

| unit | `is-enabled` | `is-active` |
|---|---|---|
| `cockpit.socket`, `httpd.service`, `nginx.service` | `enabled` | `active` |
| `irqbalance.service` | `disabled` | `inactive` |
| `kdump.service`, `bluetooth.service` | `masked` | `inactive` |

`systemctl --failed` was empty and `is-system-running` read `running`.
The full `journalctl -b` of the third run has `Listening on Cockpit Web
Service Socket.`, `Started The Apache HTTP Server.` and `Started The
nginx HTTP and reverse proxy server.`, no line naming `irqbalance`,
`kdump` or `bluetooth`, and no preset line. `mask` leaves the base image's own `multi-user.target.wants/kdump.service`
and `bluetooth.target.wants/bluetooth.service` links in place; the units
stay inactive.

**`tmp.mount` on a booted host.**

- Unmasked, on both releases: `/tmp` is a writable tmpfs mounted by
  `tmp.mount` through the base's `local-fs.target.wants/tmp.mount` link,
  while `is-enabled` reads `disabled`. `bootc status` works.
- Masked, on RHEL 9.8: `/tmp` is not a mount, `stat -f` reads
  `overlayfs`, and `touch /tmp/x` fails. `bootc status` fails with
  `Ensuring selinux install_t type: ... Read-only file system`, and
  `systemctl --failed` lists `bootc-publish-rhsm-facts.service`,
  `dbus-broker.service` and `dbus.socket`. The host got no network
  address. RHEL 10.2 was measured masked on 2026-11-13.

**A `disable` of a unit the base pulls in through a vendor `.wants` link
does nothing.** The units each base pulls in that way and that have an
`[Install]` section: `tmp.mount`, `rpm-ostree-fix-shadow-mode.service`,
and `ostree-readonly-sysroot-migration.service` on RHEL 9.8 or
`systemd-sysext.socket`, `systemd-pcrextend.socket` and
`systemd-pcrlock.socket` on RHEL 10.2. Each reads `disabled` in the base
already. `systemctl disable` of three of them in a `RUN` step exited 0,
printed nothing and removed nothing. After boot every one was started
(`active`, or on RHEL 9 `ostree-readonly-sysroot-migration.service`
queued and then skipped by its own `ConditionPathIsReadWrite=`).

**Both bases already default to `graphical.target`.** `systemctl
get-default` printed `graphical.target` in `rhel-bootc:9.8` and `:10.2`
before anything set it. With no display manager installed
(`display-manager.service` not found, though `graphical.target` wants
it), both booted to `graphical.target` with no failed unit and no jobs
left.

**Mounts under `/opt` and `/usr`, and a copied `tmp.mount`.** Same result
on both releases. Each mount unit was a tmpfs with
`WantedBy=multi-user.target`, copied to `/etc/systemd/system` and enabled.

- `/opt/<dir>`, directory in the image: mounted, with `Directory ... to
  mount over is not empty, mounting anyway.` The image's file under it is
  hidden until `umount`.
- `/opt/<dir>`, no directory in the image: `mount point does not exist`,
  the unit `failed`, and the host `degraded`. Boot still completed.
  `/opt` is read-only on the booted host.
- `/usr/share/<dir>`, directory in the image: mounted over the read-only
  image, hiding its file.
- `/etc/systemd/system/tmp.mount`, a copy of the vendor unit with
  `size=77m`, no enable line: `FragmentPath` is the `/etc` file, `/tmp`
  is mounted at 77 MiB, and `is-enabled` reads `disabled`. The vendor
  link starts the copy by name.

**Traps from this run.**

- **A drifted image refuses root ssh.** The standard profile writes
  `/etc/ssh/sshd_config.d/00-driftify.conf` with `PermitRootLogin no`.
  sshd keeps the first value it reads, so a probe drop-in must sort before
  `00-driftify.conf`: `00-r3-probe.conf` did not work and
  `00-a-r3-probe.conf` did. On RHEL 9.8 a booted
  image without a `tmp.mount` mask had an address every time.
- **RHEL 9's systemd logs units by description.** `Started The Apache
  HTTP Server`, not `httpd.service`, so grepping `journalctl -b` for unit
  names finds almost nothing on RHEL 9. Use `journalctl -b -u <unit>`.
- **A preset pass also undoes the probe.** On RHEL 10 the first-boot
  preset pass disabled the probe unit, so nothing reached the console. A
  probe for this case has to be read over ssh, or carried by a preset
  file that enables it.

### Measured, 2026-11-14: the small systemd facts phase 4 relies on

Run R5 of the units spec. No inspectah build: every case is a unit planted
by hand. RHEL 9.8 (systemd 252-67.el9_8.6) and RHEL 10.2 (systemd
257-23.el10_2.2), both aarch64, guests reverted to their clean snapshots.
Each base image carries the same systemd build as its guest. Unless a line
names one release, both gave the same result. Where each case ran:

- the search path and `systemd-fsck@`: the package-mode guest, with systemd
  as PID 1;
- the `[Install]` reads, `disable`, `preset` and `tuned`: `podman run --rm`
  of `rhel-bootc:9.8` and `:10.2`, with no manager, as in a `RUN` step;
- the additive sequence: `podman build`;
- the timer: an image built, installed with `bootc install
  to-existing-root` and booted, read over root ssh.

**An empty `[Install]` key in a drop-in resets it, except `Also=`.** With
`WantedBy=multi-user.target` in the unit (under `/etc` or `/usr/lib`) and
an `/etc` drop-in holding only `WantedBy=`, `enable` prints the "no
installation config" text, exits 0 and makes no link, and `is-enabled`
reads `static`. `WantedBy=` followed by `WantedBy=graphical.target` links
`graphical.target.wants` only. An empty `Alias=` drops the alias link. An
empty `Also=` resets nothing: the member was still enabled. `RequiredBy=`
was not tried.

**`enable` reads `[Install]` only from the unit's own `.d` and, for an
instance, its template's `.d`.** A vendor drop-in holding
`WantedBy=multi-user.target` in `/usr/lib/systemd/system/service.d/`
(type-wide) or in `r5px-.service.d/` for `r5px-a.service` (dash prefix)
made no link: `static`, "no installation config". The same file in
`<unit>.service.d/` or `<template>@.service.d/` linked the unit. In those
two directories, an `/etc` drop-in with the same file name and no
`[Install]` hides the vendor one, and `enable` makes no link. An `/etc`
file of that name in the type-wide directory does not hide a vendor
drop-in in the unit's own directory.

**The search path is the documented order.** `systemd-analyze
unit-paths` prints `/etc/systemd/system.control`,
`/run/systemd/system.control`, `/run/systemd/transient`,
`/run/systemd/generator.early`, `/etc/systemd/system`,
`/etc/systemd/system.attached`, `/run/systemd/system`,
`/run/systemd/system.attached`, `/run/systemd/generator`,
`/usr/local/lib/systemd/system`, `/usr/lib/systemd/system`,
`/run/systemd/generator.late`. Planted pairs agreed with it:
`FragmentPath` was `/run` over `/usr/lib`, `/usr/local/lib` over
`/usr/lib`, `/etc` over `/run` and over `/usr/local/lib`, and `/run` over
`/usr/local/lib`.

**A concrete instance file beats its template wherever each one sits.**
`r5ci@site.service` under `/usr/lib` loaded over a template
`r5ci@.service` under `/etc`, `/run/systemd/system` or
`/usr/local/lib/systemd/system`. `enable` linked the instance to the
`/usr/lib` file, and the unit ran that file's `ExecStart`. The 2026-11-13
section has the reverse placement.

**`disable` exits 0 when an `Also=` member is absent from the image, on
both releases.** `enable` of a unit naming an absent member prints `Failed
to enable auxiliary unit ..., ignoring.` and exits 0. `disable` of it
removes its own link, prints nothing about the member and exits 0, with or
without a prior `enable`. A present member beside the absent one is linked
and removed as usual. Only an absent unit named on the command line fails,
and only on RHEL 9 (exit 1; RHEL 10 prints the error and exits 0).

**`systemctl preset` and template instances.** Each template had
`DefaultInstance=one`.

| Rule | `preset` on | Link |
|---|---|---|
| `enable t@.service` | `t@.service` | `t@one` |
| `enable t@.service` | `t@one.service` or `t@two.service` | none |
| `enable t@two.service` | `t@two.service` | `t@two` |
| `enable t@two.service` | `t@.service` | none |
| `enable t@.service two three` | `t@.service` | `t@two` and `t@three`, not `t@one` |
| `enable t@.service two three` | `t@two.service` | `t@two` |
| `enable t@.service two three` | `t@one.service` | none |
| `disable t@.service` | `t@one.service`, an enabled instance | link removed |

A bare template rule reaches the default instance only through a preset of
the template, which is what a package scriptlet and `preset-all` do. Preset
on the instance name does nothing, even when it is the default instance.
`list-unit-files` shows the template's preset as `enabled` for the bare rule
and the list form, and `disabled` for an instance rule. Stock rules: both
bases have `90-systemd.preset: enable getty@.service`, and RHEL 9.8 also
has `90-default.preset: enable getty@tty1.service`.

**`systemd-fsck@` has no link; `systemd-cryptsetup@` was not measured.**
Both guests have `/boot/efi` as vfat with fsck pass `2`, and `/`, `/boot`
and `/home` as xfs with pass `0`. The instance for the EFI partition is
`loaded active exited`, `static`, from the template under `/usr/lib`, and
is pulled in only by `Requires=` in the generated
`/run/systemd/generator/boot-efi.mount`. No link names it under
`/etc/systemd`, `/run/systemd` or `/usr/lib/systemd/system`, and the
template has no `[Install]`. Neither guest has an encrypted volume
(`/etc/crypttab` is empty), so the cryptsetup case was not run.

**The additive sequence gives one new link and leaves the existing one
alone.** From each base, `dnf install -y rsyslog` linked
`multi-user.target.wants/rsyslog.service` by its preset (`enable
rsyslog.*`). Then, in two separate images:

- a `COPY` of `/etc/systemd/system/rsyslog.service.d/50-extra.conf`
  holding `WantedBy=extra.target`, then `systemctl enable rsyslog.service`:
  exit 0, one new link `extra.target.wants/rsyslog.service` to the
  `/usr/lib` file;
- a `COPY` of a full shadow (the vendor file with `WantedBy=extra.target`
  added), then `enable`: exit 0, one new link to the `/etc` file.

In both images the package's link kept its target (`/usr/lib/...`) and its
mtime. `enable` also printed `added as a dependency to a non-existent unit
extra.target`, which is a notice, not an error. The unit has no alias
(`Alias=syslog.service` ships commented out), so an existing alias link was
not part of the test.

**systemd refuses to start a timer whose service is not in the image, and
nothing records it as a failure.** The image carried
`/etc/systemd/system/r5t.timer` (`OnBootSec=15s`,
`WantedBy=timers.target`) and no `r5t.service`. At build time `systemctl
enable r5t.timer` exited 0 with no warning and made one link,
`timers.target.wants/r5t.timer`. After the boot on each release:

- `r5t.service` `LoadState=not-found`;
- `r5t.timer` `loaded`, `inactive (dead)`, `Result=success`, `Trigger:
  n/a`, `is-enabled` `enabled`;
- the journal for the timer: `Refusing to start, unit r5t.service to
  trigger not loaded.`, then `Failed to start ...`;
- `systemctl --failed` empty, `is-system-running` `running`.

A control timer with its service present in the same image fired and ran
the service. So the start job is rejected and the host is not degraded. The
two journal lines are the only trace.

**`tuned` is enabled by preset in both bases.** Neither base has `tuned`.
After `dnf install -y tuned` (2.28.0-1 on both), `is-enabled tuned.service`
reads `enabled` and `list-unit-files` reads `enabled enabled`, with the
link `multi-user.target.wants/tuned.service`. The rule that names it is
`90-default.preset: enable tuned.service` (line 122 on 9.8, 213 on 10.2),
ahead of `99-default-disable.preset: disable *`.

**Traps from this run.**

- A probe `RUN` whose last command fails fails the build, even after
  `( probe ); echo "rc=$?"` wrappers on the earlier commands. End the step
  with `; true` when its last line is an `ls` expected to miss.
- A `podman pull` of a base image can die mid-blob with `unexpected EOF`
  from the CDN. `podman pull --retry 5` completed it on the second try.

### Measured, 2026-11-16: stock hosts at the units branch tip

Part of run R4 of the units spec. Branch build `f9ddc7de` and, for
comparison, `main` at `f6fe8333` (`inspectah version` printed each
commit on the guest). RHEL 9.8 and RHEL 10.2, both aarch64, each reverted
to its clean snapshot and scanned as root with no drift, branch first.
Both builds' snapshots are `complete` on both hosts.

| | RHEL 9.8 | RHEL 10.2 |
|---|---|---|
| state changes | `NetworkManager-wait-online.service` `enabled`, preset `disable` | none |
| inventory | 91 units; two away from preset: that one and `remote-cryptsetup.target` (`disabled`, preset `enable`) | 107 units, none away from preset |
| `remote-cryptsetup.target` | inventory only, no row | `enabled` at its preset |
| default target | `multi-user.target` | `multi-user.target` |
| manual links, undetermined installs, mount findings | none | none |
| link warnings (all seven causes) | none | none |
| `Also=` owner advisories | none | none |

Every row is explained. `remote-cryptsetup.target` is held back by the
stock-state constant. `NetworkManager-wait-online.service` is the known
`Also=` class (a package install enables it in the image too).
`getty@tty1.service` is the only enabled instance, at its `enable`
preset, so it has no row.

**The default target is a stock line on both releases.** Both clean
guests have `/etc/systemd/system/default.target ->
/usr/lib/systemd/system/multi-user.target`, owned by no package and
dated the install, and both base images default to `graphical.target`
(2026-11-14). So every stock scan now renders
`RUN systemctl set-default -- 'multi-user.target'`, which moves the image
to the host's target. That is what § 10.5 asks for. `main` renders
nothing for it.

**Degraded and warning lists, `main` against the branch.** No section is
degraded on either build. RHEL 9.8: one warning on each, `unit
systemd-remount-fs.service has state 'enabled-runtime'`, from inspector
`services` on `main` and `units` on the branch. RHEL 10.2: the same
warning plus the skipped ELF scan (`readelf` absent), identical on both.
The widened `Also=` walk added no warning and no degraded reason on a
stock host. The `main` Containerfile's `Service Enablement` section
enables `NetworkManager-wait-online.service` on RHEL 9 and holds
nothing on RHEL 10.

Scan time on the clean guests: 41 s to 90 s for the branch build, 17 s
to 55 s for `main`, the first scan on each guest including the base
image pull.

### Measured, 2026-11-16: a link to a template name starts an instance

Throwaway units on the clean guests of the section above, then
`daemon-reload`, a branch scan, and a reboot. `tl-bar@.service` is a
`Type=oneshot` unit with `RemainAfterExit=yes` and `ExecStart=/bin/echo
tl-bar-started %i`. `tl-fooi@.target` and `tl-foot@.target` are template
targets, and links in `multi-user.target.wants` pull in `tl-fooi@a.target`
and `tl-foot@b.target`. Three links name the template:

```
/etc/systemd/system/multi-user.target.wants/tl-bar@.service   plain parent
/etc/systemd/system/tl-fooi@a.target.wants/tl-bar@.service    instance parent
/etc/systemd/system/tl-foot@.target.wants/tl-bar@.service     template parent
```

Same result on systemd 252 and 257, after `daemon-reload` and after a
reboot:

- **Plain parent: systemd fills in the parent's name as the instance.**
  `systemctl show -p Wants multi-user.target` lists
  `tl-bar@multi-user.service`, and after the reboot it ran
  (`tl-bar-started multi-user`, `active (exited)`). The journal's only
  line about the link, at each load, is `multi-user.target: Wants
  dependency dropin /etc/systemd/system/multi-user.target.wants/tl-bar@.service
  target /etc/systemd/system/tl-bar@.service has different name`.
  `is-enabled tl-bar@.service` reads `enabled`.
- **Instance parent:** `tl-fooi@a.target` wants `tl-bar@a.service`.
- **Template parent:** every instance of the parent wants the matching
  instance: `tl-foot@b.target` wants `tl-bar@b.service`, and
  `tl-foot@c.target`, never started, wants `tl-bar@c.service`.
- After the reboot `tl-bar@a.service` and `tl-bar@b.service` ran too.

So a link to a template name is never inert under a `.wants` directory.
The branch scan at `f9ddc7de` named the plain-parent link with cause
`template` and the text "systemd starts no unit from a link to a template
name without an instance, so the link has no effect", which this
measurement contradicts. Build behavior, re-measured at `49027f6e` on both
releases: the warning names the instance systemd starts
(`starts tl-bar@multi-user.service`, and a `starts` field on the link), and
that instance ran after a reboot. See
[§ the instance fixes](#measured-2026-11-16-the-instance-fixes-at-49027f6e).
The two other links get cause `instance_filled`, whose text claims
nothing about the effect, and that holds. The scan made `enable` rows for
`tl-fooi@a.target` and `tl-foot@b.target` with the attention reason for an
instance on no list.

### Measured, 2026-11-16: cockpit.socket at the branch tip, both directions

Run R1 of the units spec at `f9ddc7de`, on guests reverted to their clean
snapshots: `dnf install -y cockpit`, a standard drift run, a scan as
root, and `podman build --layers=true --network=host` of the unmodified
scan tarball under the proxy workaround, with the COPY-source check
clean. On RHEL 10, where the profile disables the socket, `systemctl
enable podman.socket` (preset `disable` on the host and `disabled` in
the base image) was added by hand before the scan, so that release has
a socket moved off its preset in each direction.

| | RHEL 9.8 | RHEL 10.2 |
|---|---|---|
| `cockpit.socket` before drift: state, preset | `disabled disabled` | `enabled enabled` |
| host after drift and plant | `cockpit.socket enabled`; `podman.socket disabled` | `cockpit.socket disabled`; `podman.socket enabled` |
| base image `is-enabled` | `cockpit.socket`: no such file | `cockpit.socket`: `not-found`; `podman.socket`: `disabled` |
| rendered | `enable -- ... 'cockpit.socket' 'multipathd.socket' ...` | `enable -- ... 'podman.socket' ...` and `disable -- 'cockpit.socket'` |
| build output for those lines | `Created symlink .../sockets.target.wants/cockpit.socket` | `Created symlink .../podman.socket`, then `Removed '.../cockpit.socket'` (the link the package install made in the same build) |
| built image `list-unit-files` | `cockpit.socket enabled disabled` | `cockpit.socket disabled enabled`, `podman.socket enabled disabled` |

**The built image reads the host's state on both releases, enabled
against preset and disabled against preset.** Builds took 511 s (RHEL 9)
and 394 s (RHEL 10).

The rest of the unit section, the same on both apart from the RHEL 9
rows below: `set-default -- 'multi-user.target'`; `enable` for
`driftify-also-member`, `getty@tty2`, `httpd`, `nginx` and
`myapp-report.timer`; `mask` for `bluetooth` and `kdump`; the
`tmp.mount` FIXME; an `Also=` advisory for the local
`driftify-also-main.service` (its own unit file under `/etc` is an
install file, so rule 7 declines it, as § 17 says it will); and an
`add-wants` advisory for the hand-made `driftify-static.service` link.
RHEL 9 adds `NetworkManager-wait-online.service`, `iscsi-starter.service`
and `multipathd.socket`, each enabled against a `disable` preset with no
driftify `systemctl` step naming it: the known `Also=` class
(`iscsi-starter.service` is the only `Also=` of `iscsi.service`, which
has an `enable` preset; read on the kitchen-sink host below). One
link warning on each host, cause `masked`:
`dbus-org.bluez.service` and `bluetooth.target.wants/bluetooth.service`,
links of the masked `bluetooth.service`. Expected.

### Measured, 2026-11-16: kitchen-sink at the branch tip, scan to build

Runs R2 and R4 of the units spec at `f9ddc7de`, with `main` at
`f6fe8333` for comparison. RHEL 9.8 and RHEL 10.2, each reverted to its
clean snapshot and given a kitchen-sink drift run (driftify `294d8b1`,
545 s on RHEL 9 and 382 s on RHEL 10). One case was planted by hand
before the scans, since the profile has no instance whose configuration
is clean:
`/etc/systemd/system/ci@.service` (`ExecStart=/bin/cat /etc/ci/%i.conf`,
`Type=oneshot`, `RemainAfterExit=yes`, `WantedBy=multi-user.target`),
`/etc/ci/site.conf`, and `systemctl enable ci@site.service`. Both builds
then scanned as root, branch first.

**WireGuard (R2 observation 3).** On both hosts `config.files` holds
`/etc/wireguard/wg0.conf` (`unowned`, mode 0600 on the host), its
`PrivateKey =` line replaced by `REDACTED_WIREGUARDKEY_1`, and
`redactions` holds an `inline` finding for it (`wireguard_key`, line 3).
The row has `required_config: ["/etc/wireguard/wg0.conf"]`, and the
Containerfile has `# Omitted: wg-quick@wg0.service (its configuration
/etc/wireguard/wg0.conf holds a redacted value; provision the secret on
the deployed host and enable the unit there)` and no `enable` for it.
So the config walk does record the file, which § 10.8 had left open.

**A clean instance (observation 4, scan side).** `ci@site.service` has
`required_config: ["/etc/ci/site.conf"]`, the file is in `config.files`
with its content, and the unit is in the `enable` list.

**The package that would own `/etc/systemd/system/service.d/*.conf`
(observation 6).** None exists in the enabled repositories, so the
snapshot question does not arise. Searched with `dnf repoquery --file
'/etc/systemd/system/service.d/*'`: on the clean guests (BaseOS and
AppStream of each release) and again after the drift run with EPEL
enabled (EPEL 9 and `epel-cisco-openh264` on RHEL 9, EPEL 10 on RHEL 10).
Every search printed nothing. Neither clean guest has the directory
under `/etc` or `/usr/lib`. For scale, the wider pattern
`/etc/systemd/system/*.d/*` does match per-unit drop-ins in `cloud-init`
and `ipa-server-common` (RHEL 9) and `ipa-server-common`, `php-fpm` and
`php8.4-fpm` (RHEL 10), which are not type-wide.

**A local service with no paired timer (observation 1).** Both scan
tarballs were built unmodified, with `--layers=true --network=host` under
the proxy workaround and the COPY-source check clean.

- `main`: the `Service Enablement` block comes before every `COPY` of
  `/etc` and names `driftify-also-main.service`, whose file is only in
  the config tree. The build stops there on both releases: `Failed to
  enable unit, unit driftify-also-main.service does not exist.` (RHEL 9)
  and `Failed to enable unit: Unit driftify-also-main.service does not
  exist` (RHEL 10), exit status 1.
- Branch: `# === Systemd Unit State ===` follows `COPY config/etc/ /etc/`,
  and both builds passed (761 s on RHEL 9, 539 s on RHEL 10). The local
  units it enables (`driftify-also-member.service`, `ci@site.service`)
  are in the image by then. `driftify-also-main.service` itself is an
  `Also=` advisory, as in the standard run.

**The libvirt advisory against the host (observation 5).** On both hosts
`virtnetworkd.service` is `enabled` with
`multi-user.target.wants/virtnetworkd.service`, and the three sockets are
`disabled`, with no `virtnetworkd*` link in `sockets.target.wants`. The
scan carries a `disable` row for each socket (each against an `enable`
preset) and makes the service an advisory naming all three sockets as
`not enabled there`, with the lines `enable -- 'virtnetworkd.service'`
then `disable --` for each socket. The `cups.service` and
`iscsid.service` advisories have the same form for one socket each. Built
from the branch image as it is, `virtnetworkd.service` and
`iscsid.service` read `disabled`. With the three advisories' eight lines
appended, in their order, every unit reads as on the host, and the
`.wants` directories hold the same `virtnetworkd`, `cups` and `iscsid`
links as the host's. The first `disable` of a socket removes all three
socket links, through the sockets' own `Also=`, and the two after it
print nothing. On both releases `cups.service` was already `enabled`, and
`cups.socket` `disabled`, without the lines: `cups.service` has an
`enable` preset, the package install enabled it, and the socket's own
`disable` row is carried. That advisory is the owner check of § 7.4; its
lines are redundant here and harmless.

**The booted image (observations 2 and 4).** The branch image, with one
layer added for the probe (`/etc/ssh/sshd_config.d/00-a-probe.conf`
allowing root key login, and a probe unit), installed with `bootc
install to-existing-root` (deployed `/etc/machine-id` size 0 on both)
and booted. Same on both releases, read over root ssh:

- The full shadow: `sshd.service` `FragmentPath=/etc/systemd/system/sshd.service`,
  `ExecStart` with the shadow's `-o LogLevel=VERBOSE`, `active`.
- `ci@site.service`: `enabled`, `active`, `FragmentPath` the `/etc`
  template, and its journal shows `greeting = hello` read from
  `/etc/ci/site.conf`. `getty@tty2.service` is `active` too.
- `wg-quick@wg0.service`: `disabled`, `inactive`.
- `virtnetworkd.service` and `iscsid.service` `disabled`, their sockets
  `disabled`, as in the build above without the advisory lines.
- First boot logged `First Boot Complete was skipped`, as in R3.
- `is-system-running` read `degraded`. The failed units are drift content
  the export does not supply, not unit state: `nginx.service` (`Unable to
  locate executable '/usr/local/bin/notify-deploy.sh'`, which the
  kitchen-sink drop-in names in `ExecStartPost=`), `sssd.service`, the
  quadlet units `webapp.service` and `dev-tools.service` (image pulls
  from registries the guest cannot reach), and on RHEL 10
  `audit-rules.service`.

**Every row, classified (R4, kitchen-sink).** No row is unexplained.

| Row | RHEL 9 | RHEL 10 | Reason |
|---|---|---|---|
| `tmp.mount` masked | FIXME, not carried | same | driftify minimal; § 10.3 |
| `bluetooth`, `kdump` masked | `mask` | same | driftify |
| `httpd`, `nginx`, `driftify-also-member`, `myapp-report.timer`, `getty@tty2` enabled | `enable` | same | driftify |
| `ci@site` enabled | `enable` | same | planted, configuration staged |
| `wg-quick@wg0` enabled | `# Omitted:` redacted | same | driftify; § 10.8 |
| `driftify-also-main` enabled (local, `Also=`) | advisory, install file | same | § 17: a local unit with `Also=` is never carried |
| `virtnetworkd`, `iscsid` enabled, `Also=` sockets off | advisory | same | driftify pairs; rule 7 |
| `cups.service` enabled at its `enable` preset, `cups.socket` off | advisory (owner check) | same | driftify pair; § 7.4 |
| `cups.socket`, `iscsid.socket`, three `virtnetworkd` sockets disabled | `disable` | same | driftify pairs, against `enable` presets |
| `cockpit.socket` | `enable` | `disable` | driftify, against preset |
| `NetworkManager-wait-online`, `multipathd.socket`, `iscsi-starter` enabled against `disable` | `enable` | no row: `NetworkManager-wait-online` and `iscsi-starter` at an `enable` preset, `multipathd.socket` `static` | the known `Also=` class: `iscsi-starter` is the only `Also=` of `iscsi.service` (preset `enable`, `indirect` on the host) |
| `driftify-static` hand-made link | `add-wants` advisory | same | driftify; `StaticUnit` |
| `driftify-upheld` | n/a (RHEL 9 makes no `.upholds` link) | link advisory and `UndeterminedInstall` | driftify; § 7.3 |
| default target | `set-default -- 'graphical.target'` | same | driftify |
| `remote-cryptsetup.target` disabled against `enable` | inventory, no row | at preset | stock-state constant |

**How often the link warnings fire.** Every warning with a link `cause`,
per host and run. Both hosts gave the same counts in every run.

| Run | `masked` | `template` | `instance_filled` | `not_read`, `refused`, `alias`, `dangling` |
|---|---|---|---|---|
| stock | 0 | 0 | 0 | 0 |
| stock plus the template-link plants | 0 | 1 (planted) | 2 (planted) | 0 |
| standard (R1) | 2 | 0 | 0 | 0 |
| kitchen-sink | 2 | 0 | 0 | 0 |
| kitchen-sink plus the hand plants below | 2 | 0 | 0 | 0 |

The two `masked` links are `/etc/systemd/system/dbus-org.bluez.service`
(the alias) and `bluetooth.target.wants/bluetooth.service`, both of the
masked `bluetooth.service`: expected, and true, since a masked unit
starts from neither. The planted ones are in
[§ a link to a template name starts an instance](#measured-2026-11-16-a-link-to-a-template-name-starts-an-instance):
the `instance_filled` text holds, and the `template` text does not. No
run produced a `not_read`, `refused`, `alias` or `dangling` warning.

### Measured, 2026-11-16: what an instance reads, systemd against the scan

On each kitchen-sink guest of the section above, after its scans and
builds, more cases were planted by hand, `daemon-reload` was run, each
instance was enabled, `systemctl show` was read for every one, and the
branch build scanned again. Every result below is the same on RHEL 9.8
and RHEL 10.2.

**Shipped templates.** `wireguard-tools`, `389-ds-base` (AppStream) and
`openvpn` (EPEL) installed on both: `wireguard-tools-1.0.20210914-3.el9`,
`389-ds-base-2.9.0-5.el9_9`, `openvpn-2.5.11-2.el9`;
`wireguard-tools-1.0.20250521-1.el10`, `389-ds-base-3.3.0-5.el10_3`,
`openvpn-2.7.9-1.el10_3`. Instances `openvpn-client@c1`,
`openvpn-server@s1` and `dirsrv@example`, each with a small unowned file
where the unit reads it (`/etc/openvpn/client/c1.conf`,
`/etc/openvpn/server/s1.conf`, `/etc/dirsrv/slapd-example/dse.ldif`) and
`/var/lib/dirsrv/slapd-example/marker`.

| Instance | What the unit reads (`systemctl show`) | `required_config` | `data_paths` | Rendered |
|---|---|---|---|---|
| `wg-quick@wg0` | `wg-quick up wg0` | `/etc/wireguard/wg0.conf` | none | `# Omitted:` redacted value |
| `openvpn-client@c1` | `--config c1.conf` in `WorkingDirectory=/etc/openvpn/client` | `/etc/openvpn/client/c1.conf` | none | `enable` |
| `openvpn-server@s1` | `--status /run/openvpn-server/status-s1.log ... --config s1.conf` in `WorkingDirectory=/etc/openvpn/server` | **no row** | **no row** | **nothing** |
| `dirsrv@example` | `ns-slapd -D /etc/dirsrv/slapd-example`, `ExecStartPre=` on `.../dse.ldif`, `EnvironmentFile=-/etc/sysconfig/dirsrv-example` | `/etc/dirsrv/slapd-example`, `/etc/dirsrv/slapd-example/dse.ldif` | `/var/lib/dirsrv/slapd-example` | `enable`, and the data advisory |

(Each `required_config` also held the type-wide plant below, as systemd
did.)

**`openvpn-server@` is never carried.** The shipped unit on both
releases has `ExecStart=/usr/sbin/openvpn --status
%t/openvpn-server/status-%i.log ...`. The reader expands only `%i`, `%I`,
`%p`, `%N` and `%%` (`specifiers.rs`), so the operand holding `%t` and
`%i` is uninterpretable, and the instance gets no state change. The scan
says so only as a snapshot warning: `unit openvpn-server@s1.service has
state 'enabled' - its ExecStart= setting in
/usr/lib/systemd/system/openvpn-server@.service (line 13) names a path
for the instance that inspectah cannot read as systemd does, so what
the instance reads is unknown and its enable is not carried`. The
Containerfile names the unit nowhere. systemd expands `%t` to `/run`.
Build behavior, re-measured at `49027f6e` on both releases: the reader
expands `%t` and the system manager's other fixed directory specifiers,
so the instance's `required_config` is the fixed table's
`/etc/openvpn/server/<i>.conf`, its enable is carried when that file is
staged, and it is omitted when the file is excluded. See
[§ the instance fixes](#measured-2026-11-16-the-instance-fixes-at-49027f6e).

**`dirsrv@` names its directory.** The derivation finds both the
directory and `dse.ldif` from the unit's own lines, and the fixed table
adds the data path. Rule 6 passed with the directory holding one staged
unowned file. The `-` prefixed `EnvironmentFile=` is left out, as
systemd treats it as optional. A real instance directory holds more
(certificate databases, a PIN file), which this run did not plant.

**Reader cases.** Each template is a `Type=oneshot` unit under `/etc` with
`WantedBy=multi-user.target`.

| Case | systemd (`systemctl show`, journal) | Scan | Rendered |
|---|---|---|---|
| `ExecStart=/usr/bin/true --config "/etc/w x/%i.conf"` | argv `--config /etc/w x/a.conf` | `/etc/w x/a.conf` | `enable` (file planted) |
| `/etc/systemd/system/service.d/50-rq-env.conf`: `EnvironmentFile=/etc/rqenv/%i.env` | every service gets it; `rt@b`: `/etc/rqenv/b.env` | `/etc/rqenv/b.env`, and on every enabled instance | `enable` (files planted) |
| `w-.service.d/50-wq.conf` for `w-q@a`: `EnvironmentFile=/etc/wq/%i.env` | `/etc/wq/a.env` | `/etc/wq/a.env` | `enable` |
| `rr@`: `EnvironmentFile=/etc/rr/%i.env`, then `rr@.service.d/10-reset.conf`: `EnvironmentFile=` | `/etc/rr/a.env` gone | gone; `10-reset.conf` in `install_drop_ins` | `enable` |
| `rs@`: the same value, then `[Unit] EnvironmentFile=` in a drop-in | `Unknown key ... in section [Unit], ignoring`; `/etc/rs/a.env` kept | kept | `# Omitted:` not in the export (file absent) |
| `rw@`: `[Unit] EnvironmentFile=/etc/rw/%i.env` | `Unknown key ...`, ignoring; nothing added | `/etc/rw/a.env` added | `# Omitted:` not in the export |
| `rl@`: `WorkingDirectory=/etc/rl/%i`, then `WorkingDirectory=-/` + 2048 `./` + `srv` | `Failed to resolve unit specifiers in working directory path ...`; stays `/etc/rl/site` | `/etc/rl/site` kept | `# Omitted:` not in the export |
| `rlc@`: the same, then `WorkingDirectory=-/srv/t` | `!/srv/t` | cleared; drop-in in `install_drop_ins` | `enable` |

Only `rw@` differs from systemd: an assignment systemd ignores still
adds its path, as built on purpose, which omits an enable systemd would
start without the file. The journal line wording differs by release (RHEL
9: `Unknown key name 'EnvironmentFile' in section 'Unit', ignoring.`).

**Instance timers.** `/etc/systemd/system/tt@.timer` with
`OnCalendar=*-*-* 04:00:00`, `tt@.service` and `tu@.service`, and
`tt@.timer.d/.hidden.conf` holding `[Timer]` `Unit=tu@%i.service`.
`tt@one.timer` and `tt@two.timer` enabled. systemd ignores the hidden
file: `Triggers=tt@one.service`, `DropInPaths=` empty. The scan records
the same triggers, `tt@one.service` and `tt@two.service`, and both
instances get `enable` rows. The hidden file is in `units.drop_ins`, and
in `config.files` as inventory. Refine's scheduled section has three rows
with distinct ids: `timer:etc/systemd/system/tt@.timer` (the template,
titled `tt@`), `timer-instance:["etc/systemd/system/tt@.timer","tt@one"]`
and `timer-instance:["etc/systemd/system/tt@.timer","tt@two"]`. Each
instance's `searchable_text` starts with its own name, so a search for
`tt@two` finds one scheduled row; `tt@` finds all three. The description
and command are not expanded per instance (`instance timer %i`,
`/usr/bin/echo tt %i`).

### Measured, 2026-11-16: the instance fixes at `49027f6e`

Part B of the branch-tip runs. Branch build `49027f6e` (`inspectah
version` printed `commit 49027f6e` on each guest), RHEL 9.8 (systemd
252-67.el9_8.6) and RHEL 10.2 (systemd 257-23.el10_2.2), both aarch64,
each reverted to its clean snapshot. No drift run. The plants, as root:
EPEL from `epel-release-latest-<major>`, `dnf install openvpn`
(`openvpn-2.5.11-2.el9`, `openvpn-2.7.9-1.el10_3`), an unowned
`/etc/openvpn/server/s1.conf`, `systemctl enable openvpn-server@s1.service`;
the template links below; `systemctl enable podman.socket`, `disable
irqbalance.service` and `mask kdump.service` for the R3 image; and the
timers of the next section. Scanned as root in 62 s and 18 s.

**`openvpn-server@s1` is carried, and omitted when its file is
excluded.** The shipped `/usr/lib/systemd/system/openvpn-server@.service`
is byte-identical to the test fixtures `OPENVPN_SERVER_EL9` (908 bytes)
and `OPENVPN_SERVER_EL10` (847 bytes) in `crates/cli/tests/units_scan_test.rs`,
which were built from upstream sources. On both hosts:

- the row has `required_config: ["/etc/openvpn/server/s1.conf"]` and no
  `data_paths`, and no warning names the unit;
- the scan Containerfile lists `'openvpn-server@s1.service'` in its
  `enable` line, the build printed `Created symlink
  .../multi-user.target.wants/openvpn-server@s1.service →
  /usr/lib/systemd/system/openvpn-server@.service.`, and the image reads
  `enabled`;
- in refine, `SetInclude` with `{"kind":"Config","key":{"path":"/etc/openvpn/server/s1.conf"}}`
  set to false, then an export (HTTP 200): the Containerfile has `#
  Omitted: openvpn-server@s1.service (its configuration
  /etc/openvpn/server/s1.conf is excluded from the export)`, no
  `systemctl` line names the unit, and `config/etc/openvpn/server` is not
  in the export. `x-export-warnings` held only the not-carried notice.

**A template link under a plain parent names the instance systemd
starts.** The scan's warning, cause `template`, with `starts` on the link:

```
link /etc/systemd/system/multi-user.target.wants/tl-bar@.service (tl-bar@.service, starts tl-bar@multi-user.service) is not reported or carried: its unit is a template, and systemd fills in the parent's name without its type as the instance and starts that instance with the parent
```

After a reboot, `tl-bar@multi-user.service` was `active (exited)` and the
journal had `tl-bar-started multi-user`, on both releases.

**Template links under an alias, a masked and a missing parent.** The same
`tl-bar@.service`, linked from three more `.wants` directories.
`tlw.target` (enabled) has `Wants=tlm.target tln.target`, so the boot asks
for all three parents. Same on both releases:

| Parent | `systemctl show -p Wants` after `daemon-reload` | After a reboot | Scan |
|---|---|---|---|
| `tla.target`, the `Alias=` link of an enabled `tlr.target` | `tlr.target` (and `tla.target`, the same unit) wants `tl-bar@tlr.service` | `tl-bar@tlr.service` ran | `instance_filled` |
| `tlm.target -> /dev/null` | `tlm.target` (`masked`) wants `tl-bar@tlm.service` | `tl-bar@tlm.service` loaded, never started | `instance_filled`; `mask -- 'tlm.target'` rendered |
| `tln.target`, no unit file | `not-found`, no wants | nothing started | `instance_filled` |

So an alias parent fills in the unit's own name, not the alias's. A
masked parent names an instance and never starts it. A parent with no
unit file reads no `.wants` link at all. The journal's `has different
name` line appears for the plain, alias and masked parents, not for the
missing one. The `instance_filled` text claims nothing about the effect,
so it holds for all three.

### Measured, 2026-11-16: R5's open items

Same guests and day as the section above. Each case ran in one of two
places:

- `podman run --rm` of `rhel-bootc:9.8` and `:10.2`, with no manager, as
  in a `RUN` step: the drop-in order, the four directories above `/etc`,
  `%I` in `[Install]`, the preset cases, the resets, the additive
  sequence, the preset file search and the scriptlets;
- the package-mode guest, with systemd as PID 1: the four directories
  above `/etc` again, with `systemctl set-property`, the timer, and
  `systemd-cryptsetup@`.

Every result is the same on both releases unless a line names one.

**The instance's `.d` wins over its template's, in both placements.**
`enable g2@x.service`, the template under `/usr/lib` with no `[Install]`,
one file of the same name, `50-g2.conf`, in each directory:

| `g2@x.service.d/` | `g2@.service.d/` | Link |
|---|---|---|
| `/etc`, no `[Install]` | vendor, `WantedBy=multi-user.target` | none, "no installation config" |
| vendor, no `[Install]` | `/etc`, `WantedBy=multi-user.target` | none |
| `/etc`, `WantedBy=graphical.target` | vendor, `WantedBy=multi-user.target` | `graphical.target.wants` only |
| vendor, `WantedBy=graphical.target` | `/etc`, `WantedBy=multi-user.target` | `graphical.target.wants` only |
| none | vendor or `/etc`, `WantedBy=multi-user.target` | `multi-user.target.wants` |

**A drop-in above `/etc` hides a same-named one below it.** The hiding
file is `50-CPUWeight.conf` holding `[Service]` `CPUWeight=200`, the name
`systemctl set-property ... CPUWeight=` gives its file. The hidden file,
of the same name, holds `WantedBy=multi-user.target`.

| Hidden file | Hiding file | In the image | On the host |
|---|---|---|---|
| the unit's `.d` under `/etc` | the unit's `.d` | no link, each of the four directories | no link, each of the four |
| the unit's `.d` under `/usr/lib` | the unit's `.d` | no link, each of the four | no link, each of the four |
| the template's `.d` under `/etc` | the instance's `.d` | no link, each of the four | no link, each of the four |
| the template's `.d` under `/usr/lib` | the instance's `.d` | no link, each of the four | not run |
| the instance's `.d` under `/etc` | the template's `.d` | **linked**, each of the four | not run |
| any of these | removed (the image without the hiding file) | linked | linked |

On the host, `set-property` wrote the hiding file to
`/etc/systemd/system.control/<unit>.d/` and, with `--runtime`, to
`/run/systemd/system.control/<unit>.d/`, for an instance under its own
name (`mc1@x.service.d`). The other two directories were planted by hand.
The manager hides the same way: `DropInPaths` listed only the control
file. The last table row is the order § 7.2 reads from source: every
directory of the instance's name before any of its template's, then the
root. `systemctl revert` removed the control files for the controls.

**`/run/systemd/generator.early` does not keep a hand-placed file.** An
online `systemctl enable` reloads the manager (one `Reloading.` journal
line per call), and a reload re-runs the generators, which replaces that
directory: it was gone after the re-test. In the first host run, the
vendor case and the template case in `generator.early` linked. Re-run with
each file written just before its own `enable`, online and with
`SYSTEMD_OFFLINE=1`, nothing linked on either release. Inferred, not
measured: the first run's links came from the reload of the previous
case's `enable` removing the next case's file.

**`%I` in `[Install]` fails `enable`, after the values before it.** On
`pi@a\x2db.service`:

| `[Install]` | `enable` | Links |
|---|---|---|
| `WantedBy=pi-%I.target` | `Failed to enable unit, invalid specifier in "pi-%I.target".` (RHEL 10: `Failed to enable unit: Invalid specifier in unit pi-%I.target`), exit 1 | none |
| `WantedBy=multi-user.target pk-%I.target` | the same error, exit 1 | `multi-user.target.wants` |
| `Alias=pl-%I@.service`, `WantedBy=multi-user.target` | the same error for the alias, exit 1 | `multi-user.target.wants` |
| `WantedBy=pj-%i.target` (control) | exit 0 | `pj-a\x2db.target.wants/pj@a\x2db.service` |

So `enable` does not skip `%I`: it fails the `RUN` step, with any link it
made before the value. § 7.2 declines the value, so no `enable` is
rendered for such a unit.

**A preset line starting with `;` is a comment, and a drop-in whose name
starts with `.` is not read.** A preset file holding `;enable psemi.service`,
`xenable pxv.service`, `#enable phash.service` and `enable ppos.service`:
`systemctl preset` printed `Couldn't parse line 'xenable pxv.service'.
Ignoring.` and nothing for the other lines, linked `ppos` only, and
`list-unit-files` read `psemi.service disabled disabled`. A
`<unit>.d/.hidden.conf` holding `WantedBy=multi-user.target`, under `/etc`
or under `/usr/lib`: "no installation config", no link. The same file
named `hidden.conf` linked the unit.

**For an instance with its own unit file, a template rule does not count
and later rules decide.** Units `t@three.service` (and `t@.service`, but
for the last row) under `/usr/lib`, each with
`WantedBy=multi-user.target`, a preset file ending in `disable *`, then
`systemctl preset t@three.service`:

| Rules before `disable *` | Link |
|---|---|
| `enable t@.service two`, `enable t@three.service` | linked |
| `enable t@.service`, `enable t@three.service` | linked |
| `disable t@.service`, `enable t@three.service` | linked |
| `enable t@three.service` (control) | linked |
| `enable t@.service two`, `enable t@three.service`, no `t@.service` file | linked |

**An empty `RequiredBy=` resets, as `WantedBy=` does. An empty `UpheldBy=`
resets on RHEL 10.** Unit under `/usr/lib`, drop-in under `/etc`:

| Unit `[Install]` | `/etc` drop-in | `enable` | `is-enabled` |
|---|---|---|---|
| `RequiredBy=multi-user.target` | `RequiredBy=` | "no installation config", exit 0, no link | `static` |
| the same | `RequiredBy=` then `RequiredBy=graphical.target` | `graphical.target.requires` only | `enabled` |
| `RequiredBy=multi-user.target` then `RequiredBy=`, both in the unit | none | no link | `static` |
| `RequiredBy=multi-user.target` | none (control) | `multi-user.target.requires` | `enabled` |
| `UpheldBy=multi-user.target`, RHEL 10 | `UpheldBy=` | no link | `static` |
| the same, RHEL 10 | `UpheldBy=` then `UpheldBy=graphical.target` | `graphical.target.upholds` only | `enabled` |

RHEL 9 prints `Unknown key name 'UpheldBy' in section 'Install',
ignoring.` for every such line, links nothing and reads `static`. § 7.2
declines the `RequiredBy=` reset as `Undetermined(RequiredByReset)`; § 17
names this measurement as one that moves the reader to follow it.

**The additive sequence leaves an existing alias link alone.** In a fresh
container of each base, for `bluetooth.service` (`WantedBy=bluetooth.target`,
`Alias=dbus-org.bluez.service`), `NetworkManager-dispatcher.service`
(`Alias=` only) and `NetworkManager.service` (whose alias link belongs to
its `Also=` member): an `/etc` drop-in holding `WantedBy=extra.target`, or
a full shadow with that line added, then `systemctl enable <unit>`. Every
case exited 0 and made exactly one new link,
`extra.target.wants/<unit>`, to the `/usr/lib` file for the drop-in and to
the `/etc` file for the shadow. Every existing link, the alias links
included (`dbus-org.bluez.service -> /usr/lib/systemd/system/bluetooth.service`),
kept its target and its `1970-01-01` mtime from the base, under the
shadow too.

**No stock preset file has a list-form line.** Searched in both bases and
on both guests with `^\s*(enable|disable)\s+\S+@\S*\s+\S`: no match. The
only lines naming an `@` unit are `90-systemd.preset:18: enable
getty@.service` (all four) and `90-default.preset:9: enable
getty@tty1.service` (RHEL 9 base and guest only).

**What package scriptlets pass to `systemctl preset`.** Every package
owning a file named `*@*` under `/usr/lib/systemd/system` or
`/usr/lib/systemd/user` in each base (30 paths on RHEL 9, 42 on RHEL 10;
every one a template or a drop-in directory, no concrete instance file),
read with `rpm -q --scripts`. `systemd-update-helper install-system-units`
runs `systemctl --no-reload preset "$@"`.

| Package | Names passed to `install-system-units` |
|---|---|
| `mdadm` | `mdmonitor.service`, `raid-check.timer` |
| `nvme-cli` | RHEL 9: none (the scriptlet runs `systemctl enable nvmefc-boot-connections`). RHEL 10: `nvmefc-boot-connections.service`, `nvmf-autoconnect.service`, **`nvmf-connect@.service`**, `nvmf-connect-nbft.service` |
| `openssh-server` | `sshd.service`, `sshd.socket` |
| `ostree` | `ostree-remount.service` |
| `podman` | none (RHEL 9 has `systemctl disable` of `podman.service` and `podman.socket`) |
| `systemd` | none; on first install its `%post` runs `systemctl preset-all` and `systemctl --global preset-all` |
| `systemd-udev` | the udev units, `systemd-boot-update.service`, `systemd-pstore.service`, and on RHEL 10 `remote-cryptsetup.target` |
| `zram-generator` | none |
| `yggdrasil` (RHEL 10) | `yggdrasil.service`, system and user |

So one scriptlet presets a template by its template name, none presets an
instance by name, and `systemd`'s own first install presets everything.

**`systemd-cryptsetup@` lives in the generator directory only.** Neither
guest has an installer-encrypted volume, so one was planted: a 64 MiB
LUKS2 file under `/var/tmp`, a key file, and a line in `/etc/crypttab`,
then `daemon-reload`. The generator wrote
`/run/systemd/generator/systemd-cryptsetup@r5c.service` and two links,
`cryptsetup.target.requires/` and `dev-mapper-r5c.device.requires/`, both
in `/run/systemd/generator`. Nothing under `/etc/systemd` or
`/usr/lib/systemd/system` names it. `UnitFileState=generated`,
`is-enabled` and `list-unit-files` read `generated`, and a start opened
`/dev/mapper/r5c`. Like `systemd-fsck@`, the instance has no link a scan
reads. The plant was removed afterwards.

**A timer's `Unit=` holding `%I` is dropped by systemd, and the scan
expands it.** On the host, `/etc/systemd/system/ti@.timer` with
`Unit=tiu@%I.service`, `tj@.timer` with `Unit=tju@%i.service`, both
`tiu@.service` and `tju@.service` present, each instance enabled and
started:

| Instance | systemd | Scan `trigger.unit` |
|---|---|---|
| `ti@a\x2db.timer` | `Failed to resolve unit specifiers in tiu@%I.service, ignoring: Invalid slot`; `Triggers=ti@a\x2db.service`; `Refusing to start, unit ti@a\x2db.service to trigger not loaded.` | `tiu@a-b.service` |
| `ti@plain.timer` | the same error; `Triggers=ti@plain.service`; refused | `tiu@plain.service` |
| `tj@a\x2db.timer` | `Triggers=tju@a\x2db.service`, `active` | `tju@a\x2db.service` |

So systemd ignores the assignment, with or without an escape in the
instance, and the timer falls back to its same-named service. Started by
hand, `tiu@a-b.service` ran, so the unit exists and no timer reaches it.
The scan renders `enable` for both `ti@` instances, which matches the
host, where they are enabled and never fire.

### Measured, 2026-11-16: an image-builder disk, read before boot

Part of run R3. The scan tarball of the section above, built unmodified on
each guest (`podman build --layers=true --network=host`, COPY-source
check clean, 264 s on RHEL 9 and 163 s on RHEL 10), renders `enable` for
`podman.socket`, `disable -- 'irqbalance.service'` and `mask --
'kdump.service'`. The image's `/etc/machine-id` is 0 bytes. The lint
warnings were `nonempty-run-tmp`, `var-tmpfiles`, `sysusers` (new here,
not investigated) and on RHEL 10 `nonempty-boot`.

**The image-builder deploys a zero-length machine-id.** On each guest:

```
sudo podman run --rm --privileged --pull=never --security-opt label=type:unconfined_t \
  -v /var/lib/containers/storage:/var/lib/containers/storage -v ./bib-out:/output \
  registry.redhat.io/rhel<major>/bootc-image-builder:latest --type raw --rootfs xfs localhost/r3probe:latest
```

The builder images carried `version` labels `9.9` and `10.3`. Each run
took about a minute and wrote a 10 GiB sparse `disk.raw` (2.1 and 2.9 GiB
used) with three partitions: `EFI-SYSTEM` (vfat), `boot` and `root` (xfs).
Loop-mounted read-only (`losetup -P -r`, `mount -o ro,norecovery`):

- `ostree/deploy/default/deploy/<checksum>.0/etc/machine-id`: **0 bytes**,
  on both releases;
- the deployment's `/etc`: `sockets.target.wants/podman.socket`, no
  `multi-user.target.wants/irqbalance.service`, `kdump.service ->
  /dev/null` with the base's own `multi-user.target.wants/kdump.service`
  still there, and `default.target -> multi-user.target`;
- the boot entry's options: `root=UUID=... rw boot=UUID=... console=tty0
  console=ttyS0 ostree=...`, no `systemd.firstboot` or other first-boot
  argument.

**Not booted.** Booting the disk needs a VM that has it as a disk: a new
VM, or a second disk on `rhel9` or `rhel10`, which are VM configuration
changes beyond the start, stop and revert these guests allow, or nested
virtualization, which is off on both (`prlctl list -i`: `Nested
virtualization: off`; no `/dev/kvm` in either guest). So this route's
first-boot journal line and booted unit states are unmeasured.

**The installer routes were not run.**

- The installer ISO route needs the guest to boot that ISO with a
  separate disk to install to. Each guest has one disk, and attaching an
  ISO is a VM configuration change. The image-builder ISO was not built.
- A plain `ostreecontainer` kickstart needs boot media (`rhel9`'s
  `cdrom0` holds `rhel-9.8-aarch64-boot.iso`, connected; `rhel10`'s holds
  the 10.2 boot ISO, disconnected), a way to hand the installer the
  kickstart (a kernel argument typed at the console, or an `OEMDRV`
  volume), and a registry the installer can pull the probe image from.
  No registry runs on the test network.

**A layer that reinstalls `systemd` writes a machine-id.** From each
base, `RUN dnf -y reinstall systemd` (`252-67.el9_8.6`,
`257-23.el10_2.2`) in `podman build`: `/etc/machine-id` was 0 bytes
before the step and 33 bytes after it, holding a fresh ID, on both
releases. RHEL 10's `systemd` `%post` runs `systemd-machine-id-setup`
before its first-install check (read from `rpm -q --scripts`). Inferred,
not measured: an upgrade of `systemd` in a layer does the same, and the
image then ships one ID to every host deployed from it, with no first
boot.

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

**Two builds at once break the UI step.** `crates/web/build.rs` runs
`npm ci` and `npm run build` in `crates/web/ui` for every build, whatever
the target directory. A second `cargo build` of the same checkout,
running at the same time with another `--target-dir`, makes `npm ci` fail
with `ENOTEMPTY` or `npm run build` fail partway. If `ui/dist` is already
built from the commit under test, `INSPECTAH_SKIP_UI=1` skips the step
and embeds the existing assets. Otherwise wait for the other build.

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
- The drift tool's standard profile sets `PermitRootLogin no`, effective
  on the next sshd restart. It no longer moves sshd off port 22 (a
  standard run on 2026-11-08 left it on 22), but drift still stacks
  across runs, so revert the guest before each one.
- A scan pulls the bootc base image (about 2 GB, here
  `quay.io/fedora/fedora-bootc:44`) into root's podman storage. On a
  guest you will not revert, remove it with `sudo podman rmi`.
- Shut a guest down with `poweroff` inside it, not by killing the VM. A
  guest killed mid-write can leave the rpmdb or journal inconsistent,
  which later reads as an inspectah bug.

## Stock Fedora Server has no `readelf`

binutils is not installed on stock Fedora Server, so inspectah skips the
non-RPM ELF walk and misses Go binaries such as the drift tool's
`/usr/local/bin/driftify-probe`. For a validation run on a test guest,
install binutils before scanning if the run needs those findings. That is
a note for test runs only. The scan's warning says the walk was skipped
and that hand-installed compiled programs were not detected, and it
deliberately gives no install advice while the walk's design is under
review. Do not add advice to the warning to match this note.
