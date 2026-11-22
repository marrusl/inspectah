---
title: Known Limitations
parent: Reference
nav_order: 8
---

# Known limitations

Problems in the current release that have a workaround and a planned fix.

## A later image's users and groups do not reach a host that changed them locally

**Symptom.** A user, group or group membership you add or change in a later
image is missing on a deployed host after `bootc upgrade`, while other hosts
get it.

**Cause.** The Containerfile writes carried users and groups to `/etc/passwd`
and `/etc/group` at build time. Both are in `/etc`, so a host that changed
either file locally, for example with `useradd` on the running system, keeps
its own copy on update. `bootc container lint` warns about this as its
`sysusers` lint.

**Who is affected.** Hosts built from an export that carries a user
(Containerfile strategy `useradd`) and whose `/etc/passwd` or `/etc/group`
was changed after deployment. On such a host, the `tmpfiles.d` line that
creates a user's home is skipped, with one journal line, only when the user
or group it names is missing from the host's files.

**Workarounds.** Make account changes on the deployed host the same way the
image would, or reconcile `/etc/passwd` and `/etc/group` there after the
update. SSH keys are not in the image: provision them at install time with
kickstart (`sshkey`), cloud-init or a systemd credential.

**Status.** By design for this release.

## A redacted quoted or XML value loses its closing quote or tag

**Symptom.** A configuration file in the export no longer parses where
redaction replaced a secret. Tomcat's `<Resource password="p"/>` becomes
`<Resource password=REDACTED_PASSWORD_1`, and
`url="jdbc:mysql://u:pw@db/app" pool=4` becomes
`url="jdbc:mysql://REDACTED_JDBCPASSWORD_1 pool=4`. A JSON string under a
credential key is replaced with its quotes, so `{"password": "p"}` becomes
`{"password": REDACTED_PASSWORD_1`, and so is a quoted value under any
other credential key: `password: "p q"` becomes
`password: REDACTED_PASSWORD_1`.

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
