---
title: Known Limitations
parent: Reference
nav_order: 8
---

# Known limitations

Problems in the current release that have a workaround and a planned fix.

## `podman build` fails on a carried user whose home is under `/home`

**Symptom.** The build stops at the `RUN useradd -m ... -d /home/<user>` line
in the Containerfile's "Users and Groups" section. `useradd` exits 12 with
`cannot create directory /home`.

**Cause.** On the `rhel-bootc` base image, `/home` is a symlink to `var/home`,
and `var/home` does not exist at build time, so `useradd -m` cannot create the
home directory.

**Who is affected.** Any export that carries a user (Containerfile strategy
`useradd`) whose home directory is under `/home`. This was measured on
`registry.redhat.io/rhel9/rhel-bootc:9.8`. Other base images are not verified.
Users whose home is elsewhere, such as `/var/lib/myapp`, are not affected by
this failure.

**Workarounds.** Pick one.

1. Exclude the user before export. In `inspectah refine`, open Users & Groups
   and clear the user's include checkbox (or expand the user and set
   Containerfile strategy to Skip). The user is then left out of the
   Containerfile's Users and Groups section. Create the account another way,
   for example at provisioning time.

2. Edit the generated Containerfile.
   - Change `useradd -m` to `useradd -M` on the user's line, so `useradd` does
     not try to create the home directory.
   - Remove or move the lines that follow for that user if the user has SSH
     keys: `RUN install -d -m 700 ... '/home/<user>/.ssh'`,
     `COPY users/home/<user>/.ssh/authorized_keys ...`, and the `RUN chown ...
     authorized_keys && chmod 600 ...` line. All three write under `/home`
     and fail the same way.
   - Create the home directory on the running system instead. For example, a
     `systemd-tmpfiles` rule such as
     `d /var/home/<user> 0700 <user> <group> -` creates it at boot. This rule
     has not been tested against the image. Install the user's
     `authorized_keys` at provisioning time.

**Status.** Planned for a later release.
