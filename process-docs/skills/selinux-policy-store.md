---
name: selinux-policy-store
description: Where an SELinux host keeps its local modules and persistent booleans, and why the collector reads the policy store rather than /etc/selinux/<type> or semanage boolean -l.
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

The active store is `<store-root>/<type>/active`. `active_store()` in
`crates/collect/src/inspectors/selinux.rs` reads `store-root` from
`semanage.conf` and falls back to `/var/lib/selinux`, so read anything
else in the store through it. Until 2026-11-21 the module collector read
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

A name at two local priorities is listed once. The snapshot holds names,
not module content, so the Containerfile cannot carry a module: it names
each one and comments out the lines that would. `semodule -c -E <name>`
on the host writes `<name>.cil` from the highest priority, and `semodule
-i /tmp/selinux/*.cil` installs it in a build (both checked in a
`rhel-bootc:10.3` container, 2026-11-21). An RPM that runs plain
`semodule -i` in its scriptlet lands at 400 and is reported as local.

Not collected: `disabled/`, which records modules turned off with
`semodule -d`, including base modules.

## Booleans

`setsebool -P` and `semanage boolean -m` write `active/booleans.local`:
a libsemanage comment header, then `name=0|1` lines (it also reads
`true`/`false`). That file is the set of booleans set persistently, a
persistent `off` included. The collector reads it (a file read, so the
host needs no `semanage`) and falls back to `chroot <root> semanage
boolean -l -C` only when the store cannot be read; that command prints
`(running, persistent)` and the second column is the one kept. A readable
store with no `booleans.local` means none are set. A line the parser does
not understand is named in a scan warning.

Not collected:

- **`semanage boolean -l` without `-C`**: a persistent change rewrites
  the default it prints, so "current differs from default" keeps exactly
  the runtime-only changes and drops the persistent ones. The collector
  did this until 2026-11-21.
- **Runtime-only changes** (`setsebool` without `-P`, or the sysfs files
  under `/sys/fs/selinux/booleans/`, which hold running and pending values
  only): the host loses them at its next boot, so the image matches the
  host as it boots.

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
