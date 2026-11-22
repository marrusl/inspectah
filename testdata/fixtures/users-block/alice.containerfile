# === Users and Groups ===
# Users and groups are written to /etc/passwd and /etc/group at build time.
# A host that changed either file locally keeps its own copy on update, so
# a user, group or membership added or changed in a later image does not
# reach that host.
# Each group keeps the host's gid, so ownership the export writes by number
# still matches. A base image entry with the same name and gid is kept, and
# copied from /usr/lib/group into /etc/group when useradd -G needs it there.
# A group whose name or gid the image already uses differently stops the
# build: set its users to Skip in refine, or renumber it on the host and re-scan.
RUN getent group appgrp | grep -q '^appgrp:[^:]*:1001:' || groupadd -g 1001 appgrp || \
    { echo 'group appgrp (gid 1001): the image already uses the name or the gid differently; see the comment above' >&2; exit 1; }
RUN grep -q '^wheel:[^:]*:10:' /etc/group || getent group wheel | grep '^wheel:[^:]*:10:' >> /etc/group || groupadd -g 10 wheel || \
    { echo 'group wheel (gid 10): the image already uses the name or the gid differently; see the comment above' >&2; exit 1; }
RUN useradd -M -u 1001 -g appgrp -G wheel -s '/bin/bash' -d '/home/alice' alice
# Home directories are created at boot by systemd-tmpfiles, not in the image.
# tmpfiles resets their owner and mode on every boot; to change that on one
# host, put a file named inspectah-users.conf in /etc/tmpfiles.d/.
RUN printf '%s\n' 'd /var/home/alice 0700 alice appgrp -' > /usr/lib/tmpfiles.d/inspectah-users.conf
# SSH keys are not part of this image. Provision them at install time with
# kickstart (sshkey --username=<user>), cloud-init, or a systemd credential.
# alice had 2 keys in /home/alice/.ssh/authorized_keys on the host.
