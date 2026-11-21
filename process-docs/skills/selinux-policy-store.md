---
name: selinux-policy-store
description: Where an SELinux host keeps its local modules, disabled modules and persistent booleans, how semanage.conf's store-root is lexed, why warnings never name the store's path, and why the collector reads the policy store rather than /etc/selinux/<type> or semanage boolean -l.
---

# The SELinux policy store

`/etc/selinux/<type>/` is the **installed** policy: what the kernel loads,
plus `contexts/files/file_contexts.local` and `.subs`, which libsemanage
writes out on every commit. The **policy store** that `semodule` and
`semanage` modify is somewhere else:

| Host | Store root | Where it comes from |
|---|---|---|
| Package-mode RHEL 9 and 10 | `/var/lib/selinux` | libsemanage's default; `semanage.conf` sets no `store-root` |
| bootc images (`rhel-bootc:10.3`) | `/etc/selinux` | `store-root=/etc/selinux` in `/etc/selinux/semanage.conf` |

The active store is `<store-root>/<type>/active`. `PolicyStore::active()`
in `crates/collect/src/inspectors/selinux.rs` reads `store-root` from
`semanage.conf` and falls back to `/var/lib/selinux`, so read anything
else in the store through it (`PolicyStore::join()`).

**A diagnostic never names the store's path.** Both parts of it are
configuration (`store-root`, and `SELINUXTYPE` from `/etc/selinux/config`),
each value runs to the end of its line, and warnings are not redacted, so
`store-root = /var/lib/selinux # password=...` would carry the note into
both exports' snapshot and audit report, the README and the HTML report.
`PolicyStore` has no `Display`; a warning names the store by
`PolicyStore::LABEL`, a place in it by its relative name
(`modules/disabled/ in ...`, `the priority 400 modules in ...`), and an
error by `e.kind()` only, since an I/O error's text can hold the path
(the size-cap error in `RealExecutor::read_file` does, and so does every
`MockExecutor` error). A new warning about the store follows the same
rule; `a_configured_store_root_reaches_no_export` checks every file of
both exports and refine's view for a sentinel in `store-root`.

`store_root_from_conf()` follows libsemanage's own lexer and parser
(`libsemanage/src/conf-scan.l` and `conf-parse.y`, SELinux 3.6), not a
generic `key=value` reading, because the two differ where it matters:

- `"..."` around the value is stripped, so `store-root = "/etc/selinux"`
  is `/etc/selinux`. Flex takes the longer of the quoted and unquoted
  rules, so a quoted value followed by anything, even a trailing space,
  is read unquoted and keeps its quotes.
- An unquoted value runs to the end of the line. `#` there is literal:
  the lexer has no inline comments, so do not add shell-style stripping.
- Every assignment replaces the last (`parse_store_root_path` frees the
  old path). Taking the first one read a stale store silently. Until 2026-11-21 the module collector read
`/etc/selinux/<type>/active/modules/400`, which does not exist on a
package-mode host, and its unit test mocked that same path, so the suite
passed while every module on RHEL 9.8 and 10.2 went unreported.

## Modules

`active/modules/<priority>/<name>/` holds `cil`, `hll` and `lang_ext`
(all bzip2-compressed except `lang_ext`). Priorities are directories
named by number, beside `disabled/`.

- **100**: the base policy, from `selinux-policy-<type>`. Not local.
- **200**: modules an RPM installs with `%selinux_modules_install`
  (`container`, `insights_core`, ...). The package carries them. Not local.
- **400**: `semodule -i`'s default. Local, except `extra_varrun` and
  `extra_binsbin`, which selinux-policy's own `varrun-convert.sh` and
  `binsbin-convert.sh` generate there and its RPM macros rebuild in every
  transaction; the bootc base image already carries `extra_varrun` at 400.
- **Any other number**: local, put there with `semodule -X <n> -i`.

The generated-module exception holds at 400 only (the CentOS Stream
`selinux-policy.spec` generates them there); a module with either name at
any other priority is the administrator's and is reported.

`disabled/` holds one empty file per module turned off with `semodule
-d`, by name and whatever its priority; the module stays installed and
the policy is built without it. The collector puts every name there in
`disabled_modules`, with origin `local` when the module is at a local
priority and `packaged` otherwise, and removes a disabled local module
from `custom_modules`: offering it for installation as active would
switch on policy the host turned off. A store with no `disabled/` has
nothing disabled; one that cannot be read is a scan warning. The
Containerfile names disabled modules in a FIXME with a commented
`semodule -d` line rather than running it: a module the target policy
lacks fails the build, disabling a base module changes confinement
broadly, and an aggregate does not record which hosts disabled it.

Every module directory is read with `read_dir_strict`: `read_dir` drops
entries it fails to read, and a partial listing looks complete.

A name at two local priorities is listed once. The snapshot holds names,
not module content, so the Containerfile cannot carry a module: it names
each one and comments out the lines that would. `semodule -c -E <name>`
on the host writes `<name>.cil` from the highest priority, and `semodule
-i /tmp/selinux/*.cil` installs it in a build (both checked in a
`rhel-bootc:10.3` container, 2026-11-21). An RPM that runs plain
`semodule -i` in its scriptlet lands at 400 and is reported as local.

## Booleans

`setsebool -P` and `semanage boolean -m` write `active/booleans.local`:
a libsemanage comment header, then `name=0|1` lines (it also reads
`true`/`false`). That file is the set of booleans set persistently, a
persistent `off` included. The collector reads it (a file read, so the
host needs no `semanage`) and falls back to `chroot <root> semanage
boolean -l -C` only when the store cannot be read; that command prints
`(running, persistent)` and the second column is the one kept. A readable
store with no `booleans.local` means none are set. A line the parser does
not read (no `=`, a name outside the boolean grammar, a value other than
0, 1, true or false) is reported in a scan warning by **line number and
reason only**. Warnings are not redacted, and a line that is not what
libsemanage writes can hold anything; the end-to-end test puts a sentinel
credential in one and checks both exports and refine for it.

Not collected:

- **`semanage boolean -l` without `-C`**: a persistent change rewrites
  the default it prints, so "current differs from default" keeps exactly
  the runtime-only changes and drops the persistent ones. The collector
  did this until 2026-11-21.
- **Runtime-only changes** (`setsebool` without `-P`, or the sysfs files
  under `/sys/fs/selinux/booleans/`, which hold running and pending values
  only): the host loses them at its next boot, so the image matches the
  host as it boots.

The boolean name grammar is `is_boolean_name()` in
`crates/core/src/types/selinux.rs`: libsepol's CIL name rule (a letter,
then letters, digits, `_` and `-`, with `.` for names qualified by a
block). The renderer refuses any other name with a FIXME, which also
keeps shell globs (`*`, `?`, `[...]`) and empty names out of `RUN`.

In a fleet aggregate, `merge_selinux_booleans()` moves a boolean that one
host sets on and another off into `boolean_conflicts`, with the hosts on
each side, and the renderer names it in a FIXME instead of emitting both
commands (host order would pick the value). Agreed booleans stay in
`boolean_overrides`. A host that names one twice holds the last value.

The renderer emits `RUN setsebool -P <name> on|off` per boolean;
`setsebool -P` succeeds in a `rhel-bootc:10.3` container (2026-11-21).
The bootc base image itself carries `virt_use_nfs=1` and
`virt_sandbox_use_all_caps=1` in its `booleans.local`, so a RHEL 10 host
usually lists those too; setting them again is harmless.

## Testing

Mock the store at its real path through the collector (see
`crates/cli/tests/selinux_scan_test.rs`, which also mocks a runtime-only
boolean in sysfs to prove it is not carried), and render both exports
from what the collector produced. A hand-built `SelinuxSection` cannot catch a
collector that reads the wrong path.
