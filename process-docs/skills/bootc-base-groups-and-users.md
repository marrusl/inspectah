# Groups on a bootc base live in two files

Anything that renders account commands for a RHEL bootc image (the
Containerfile's users block in `crates/pipeline/src/render/users.rs`, and
whatever renderer comes next) needs two facts about the base and one rule
about gids. All three were measured on `rhel9/rhel-bootc:9.8` and
`rhel10/rhel-bootc:10.2` (bootc 1.16.4) on 2026-11-21.

## The base keeps most groups in `/usr/lib/group`

The base `/etc/group` holds six groups (`root`, `adm`, `wheel`,
`systemd-journal`, `tss`, `kvm`). The other 55 (9.8) or 61 (10.2) are in
`/usr/lib/group`, read through nss-altfiles. So:

- **`getent group` and `groupadd` see both files.** `groupadd` refuses a
  name (exit 9) or a gid (exit 4) that exists only in `/usr/lib/group`.
  Reading the base's `/etc/group` alone gives the wrong answer.
- **`useradd -G <g>` fails (exit 6) when `<g>` is only in
  `/usr/lib/group`**, because supplementary membership is written to
  `/etc/group`. Copy the entry first,
  `getent group <g> | grep '^<g>:[^:]*:<gid>:' >> /etc/group`, and only when
  it is not already in `/etc/group`: a duplicate entry breaks the tools.
  Upstream documents the copy in bootc's `bootc-users-and-groups.7.md`.
- `useradd -g <g>` works against a `/usr/lib/group` entry; only `-G` needs
  the copy.

## `/home` is a symlink to a `/var/home` that does not exist at build time

`useradd -m` exits 12 and `install -d /home/<u>/...` exits 1. The bases set
`CREATE_HOME yes`, so even a plain `useradd` tries; pass `-M`. Create the
home at boot with a `tmpfiles.d` line, `d /var/home/<u> 0700 <u> <g> -`,
written into `/usr/lib/tmpfiles.d/` so updates replace it. Anything written
under `/var` at build time reaches a host only at first install.

`useradd -M` still prints `Creating mailbox file: No such file or
directory` (no `/var/spool/mail`) and exits 0. That is noise, not a failure.

## Every group keeps the host's gid, or the build stops

The export writes some ownership by number (`resolve_tmpfiles_ownership`
falls back to the host's numeric gid for an account it cannot vouch for).
If the image gave a group another gid, membership in it would grant nothing
on that directory and nothing in the build would say so. So each group line
succeeds only when the image has the name with the host's gid, or has
neither the name nor the gid (then `groupadd -g <gid>`). A name with another
gid, or the gid under another name, stops the build with a message naming
both. Do not use `groupadd -o` (two groups sharing permissions) or pick a
free gid (it silently changes numeric ownership elsewhere in the export).

The check runs at build time, not scan time, because a package installed
earlier in the Containerfile can create the group or take the gid, and the
operator can change `FROM` after the scan. The collector records the names
and gids the check needs on each user (`primary_group`, `group_gids`), from
any gid range; never infer a group name from a gid range.

## A tmpfiles.d line names only groups the image is sure to have

`systemd-tmpfiles` rejects a line whose owner or group it cannot resolve,
so the directory is never made. Proving the owner does not prove the group the
host recorded on the directory, whichever way the owner is proved:

- **A user the users block creates** (`ContainerfileUserPlan::creates_user`)
  is safe to name, but its directory's group may be one no group line
  creates (`d /var/lib/svc 0750 svc homeowners -`, where no accepted user
  is in `homeowners`).
- **An account a carried package creates** (`KNOWN_PACKAGE_ACCOUNTS`, or a
  package named for the account) proves the account and the group the
  package creates with it, which has the account's name. It proves no
  other group: `apache:homeowners` with only `httpd` carried.

A `KNOWN_PACKAGE_ACCOUNTS` row must be an account the package creates on
every install. An account its scriptlet creates only under some condition
proves nothing: `nfs-utils` creates the legacy `nfsnobody` only when uid
65534 is free, and every bootc base has `nobody` there, so a carried
`nfs-utils` never gives the image `nfsnobody`. Read the package's spec
before adding a row; leave out any account created inside an `if`.

Name a group in a tmpfiles.d line only when it is that same-named package
group or `ContainerfileUserPlan::guaranteed_gid` returns the host's gid for
it; otherwise write the host's numeric gid, which the group-identity rule
above keeps meaningful. A group the scan recorded no name for is written by
gid too, never as the owner's name. Ask the plan; do not re-derive which
groups exist from the snapshot, and do not assume a base group from its
name.
