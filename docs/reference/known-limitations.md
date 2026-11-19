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

## A redacted quoted or XML value loses its closing quote or tag

**Symptom.** A configuration file in the export no longer parses where
redaction replaced a secret. Tomcat's `<Resource password="p"/>` becomes
`<Resource password=REDACTED_PASSWORD_1`, and
`url="jdbc:mysql://u:pw@db/app" pool=4` becomes
`url="jdbc:mysql://REDACTED_JDBCPASSWORD_1 pool=4`. A JSON string under a
credential key is replaced with its quotes, so `{"password": "p"}` becomes
`{"password": REDACTED_PASSWORD_1`.

**Cause.** Redaction does not yet read a file by its format. A value is
replaced up to the next whitespace, and a JDBC URL that carries a
credential is replaced after its scheme up to the next whitespace after
its last credential, so a closing quote or tag touching the value goes
with it. Ending a value at a quote or tag instead left part of the secret
in the export, because the same characters can be part of a password.

**Who is affected.** Any export whose configuration files hold a secret in
a quoted value, a JSON string, or an XML attribute or element.

**Workaround.** Restore the closing quote or tag by hand in the exported
file. The secret has to be supplied at deploy time in any case.

**Status.** Planned for a later release, which reads each file by its
format.
