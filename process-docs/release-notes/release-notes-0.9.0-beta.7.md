# Release Notes: v0.9.0-beta.7

## What's new in v0.9.0-beta.7

This release carries more of what makes a host itself and keeps more of it
out of the image. A package that owns an enabled unit is carried, users are
added with `useradd` and build on a bootc base, and files that belong to one
machine (keytabs, SSH host keys, registration state) are excluded by default
and listed under a new Machine state class with the step each one needs. A
repo's switch now decides its `.repo` file and keys, `secrets-review.md` is
grouped per file, and files bundled with `--include-unmanaged` are redacted or
withheld. The snapshot schema is now 31.

### Breaking changes

inspectah has no backwards-compatibility promise before 1.0, so there is no
migration path for these. Re-scan each host and re-aggregate.

- **Snapshot schema is 31.** `refine` and `aggregate` load only schema 31.
  Snapshots from v0.9.0-beta.6 and earlier do not load: re-scan each host and
  re-aggregate. Each added package now records what it needs, each user
  records `primary_group` and `group_gids`, and `selinux.boolean_overrides` is
  a list of `{"name", "value"}`. New fields `selinux.disabled_modules` and
  `selinux.boolean_conflicts` hold the modules disabled with `semodule -d` and,
  in an aggregate, the booleans hosts set differently.
- **A repo's switch decides its `.repo` file and keys.** The Configuration
  Files row for a `.repo` file or a GPG key a repo names is read-only and says
  "decided under Repositories". A repo with no packages left to change, a
  distribution repo or a disabled one can drop its file; a file another kept
  repo needs keeps shipping. A default `redhat.repo` no longer ships.
- **Files that belong to one machine are excluded by default.** Keytabs, SSH
  host keys, private keys, RHSM and Insights registration, `/etc/machine-id`
  and the iSCSI initiator name are no longer copied or silently dropped. Refine
  lists them under Machine state with the step each needs, and you can include
  a host identity file there. Credentials never ship, nor does an
  `--include-unmanaged` file holding a keytab, PEM or OpenPGP private key. DER
  and PKCS#12 keys are not detected.
- **Users' SSH keys are provisioned at install time.** The image no longer
  carries them and the export has no `users/` directory. The Containerfile
  names each user's key file on the host, and `inspectah-users.ks` still has
  kept keys as `sshkey` lines.

### Units, users and machine state

- **Packages that own an enabled unit are carried by default**, such as
  `firewalld` or `cronie`, in the scan tarball and refine exports. One that
  needs a disabled repo, a repo you exclude, a package no enabled repo serves
  or possibly a package the host lacked is listed as Not carried with the
  reason, as is an owner you exclude. A fleet export names owners its package
  merge omitted, and including a package in refine carries what it needs.
- **A user you set to useradd builds on a bootc base.** The Containerfile adds
  it with `useradd -M`, a `tmpfiles.d` line creates its home at first boot, and
  each group gets the host's gid. A group name or gid the image already uses
  differently stops the build with a message. `aggregate` keeps every host's
  supplementary groups for a user.
- **Machine state is its own class.** Exports name the provisioning steps a
  deployed machine owes, such as `realm join` or `subscription-manager
  register`, in the Containerfile, the audit report, the README, the scan's
  output and the refine export's warnings. A `tmpfiles.d` line whose account
  the image lacks is listed with the path and the account.
- **`secrets-review.md` is grouped per file.** Each redacted file is a heading
  listing its redactions and one line on how to supply the real value at
  deploy time. The refine export writes the file too.

### Configuration carried

- **SELinux booleans set with `setsebool -P` are carried** as `RUN setsebool
  -P`. In an aggregate, a boolean hosts set differently is named in a `FIXME`
  and set neither way.
- **SELinux modules are detected again**, from the active policy store, with a
  `semodule` command for each, and modules disabled with `semodule -d` are
  named in a `FIXME`.
- **Kernel arguments go into a `kargs.d` file bootc can read.** The file is a
  top-level `kargs` array, a quoted value is no longer dropped, and an
  argument whose value was redacted is named as a step for `bootc install
  to-disk --karg`. `/etc/default/grub` is never copied and is reported only
  when it sets arguments kargs.d does not carry.
- **Other configuration:** edits to `%ghost` config files under `/etc` are
  collected, files under `/etc/containers/systemd/` that are not quadlets are
  carried, SSSD configuration keeps mode 0600, the authselect profile is
  carried as `authselect select`, `/etc/audit/audit.rules` is carried unless
  the audit units are as packaged, an edited `/etc/anacrontab` is carried, and
  `.rpmsave`, `.rpmnew` and `.rpmorig` files are skipped. A named pipe under
  `/etc` no longer hangs the scan.
- **Version locks and `/var`:** each version lock is added once with its
  package name, and `inspectah-var.conf` names an owner the image has.

### Security and secret redaction

- **`--include-unmanaged` no longer ships credentials in clear.** Text files
  under `unmanaged/` have their credentials replaced with `REDACTED_*`
  placeholders, and text inspectah cannot check is left out. `secrets-review.md`
  names each file, and refine asks for the sensitive-data acknowledgment.
  `--no-redaction` still ships them raw.
- **More secrets are caught.** SSSD `*_authtok` values, encrypted and other
  private key blocks, repo URL credentials, and a quoted kernel argument's
  whole value are redacted. A repo id that reads like a credential is kept.
  Armored OpenPGP private keys are kept out of the scan.
- **Key material is not read under another name.** Paths through a linked
  directory are checked under both names, and a key reached through a link
  never reaches the snapshot or an export. Configuration warnings no longer
  quote file content.

### Repos and packages

- **A package whose repo the export no longer carries is not installed by
  name**; a `FIXME` names the switch that restores it, or the RPM upload.
- **Each architecture of a package keeps its own source repo** on a multilib
  host, and a GPG key named with a doubled slash uses its normalized path.
- **A unit command no longer fails the build when dnf skips its package.**

### Refine and aggregate

- **`refine -y` resumes a saved session**, `--fresh -y` discards it, and a
  closed input without `-y` is an error.
- **`aggregate --strict` passes on scans taken over a few days.** The stale
  scan dates warning fires only when scans are more than 7 days apart.
- **Fixes:** the fleet view keeps the latest decisions and no longer jumps to
  an item you left, keys typed into search boxes stay there after a section
  change, section navigation reads as one list to screen readers, Reset to
  defaults names what it clears, and a package group's optional count counts
  only included packages. `refine --help` names `--open false`, `aggregate`
  takes `-o`, and the scan README no longer cites `--from-snapshot`.
- **The audit's unit table says what the image does**, with Host change and In
  the image columns.
- **Debug builds report `commit dev, built dev`.** Release builds and
  packaged binaries are unchanged.

### Known limitations

These have workarounds and are planned for a later release.

- **Redaction, kernel command lines**: a command line embedded in another file, such as `kernel_cmdline=` in `/etc/dracut.conf.d`, can keep part of a credential after partial replacement, so review those files before sharing an export. A `rd.luks.key=` value that starts with a Unicode space is not redacted; it is a key-file path, not the key.
- **Redaction, shell-quoted values**: a value written with shell escaping or `$'...'` quoting, such as `GRUB_CMDLINE_LINUX="rd.iscsi.password=\"a b\""` in `/etc/default/grub`, can be only partly redacted in the snapshot. Review such files before sharing an export.
- **Redaction, several credentialed URLs on one line**: the URL credential masks (`user:password@` and token-as-username) replace only the first match on a line. When a line holds a second credentialed URL and no generic credential pattern catches it, because its key name does not look like a credential (`MIRRORS=https://a:pw1@h1 https://b:pw2@h2`), the second credential stays in the snapshot, exports and image. Review lines that hold more than one credentialed URL before sharing an export.
- **Redaction, repositories and `--no-redaction`**: redaction can remove URL text or configuration syntax as well as credentials. Inspect the exported `.repo` files and restore non-secret text before building. A package whose usable repository route is lost gets a `FIXME` naming a switch that restores it or an RPM upload. `--no-redaction` keeps credentials in exports.
- **`/etc/default/grub`**: the file is never copied. It gets an advisory when it sets arguments kargs.d does not carry, or when the scan cannot compare it confidently, as with shell constructs the reader does not model. Review the file and put any additional required arguments in a kargs.d file yourself.
- **Unmanaged files under `/opt` and `/srv`**: private keys in DER or PKCS#12 form and binary OpenPGP packets are not detected, though PEM keys are. A PEM key with odd line breaks (`&#10;`, `%0A`, `<br>`), a UTF-16, compressed or double-encoded file, or a key cut off below the minimum length can also be missed. A file can change between the check and the copy.
- **Repositories on other distributions**: on a host that is not RHEL, CentOS or Fedora, such as AlmaLinux or Rocky Linux, inspectah treats the build's dnf version as unknown. It can then require a key the build would not check, and an installable package can get an upload `FIXME`. Check the Containerfile's `FIXME` lines before building.
- **Repositories, repeated signature options**: inspectah uses the last assignment across `gpgcheck` and `pkg_gpgcheck`, but dnf 5 updates each name's value and applies the names in first-occurrence order. On Fedora, a section that mixes the two names with repeats can fail the build for a missing key or leave out an installable package. Use one signature option per section before scanning.
- **When `rpm -Va` fails**: modified RPM-owned configuration files may be left out with no general warning. On Fedora, a stock repo file is then not counted as the base image's, so excluding it can leave its packages with an upload `FIXME`. Check the exported configuration against the host, resolve the failure and re-scan.
- **Packages and units**: an enabled unit's owner whose dependencies need a repo disabled on the host is not carried by default. The notice names it and says why, and you can include it. Refine rows do not show the hold reason; the Containerfile and the audit report do. Guarded unit commands are not shown in refine.
- **Package dependencies**: `Supplements` conditions are judged on the source host, and the dnf fallback ignores weak dependencies. Review the built image's packages and unit commands, and include any required package the export leaves out.
- **SELinux mode**: the recorded running mode has no export command. An unowned or content-modified `/etc/selinux/config` can still be copied as configuration, but a `%ghost` copy is skipped. Check the file and set the image's intended mode yourself.
- **`/etc/crypttab`**: storage does not parse encrypted-volume mappings. An unowned or content-modified copy can appear under Configuration Files, where refine locks it out as merge-hostile; a `%ghost` copy is skipped. Provide any encrypted-volume configuration yourself.
- **Locale and timezone**: the recorded values have no export command, and `/etc/localtime` is not copied. An unowned or content-modified `/etc/locale.conf` can still be copied as configuration; a `%ghost` copy is skipped. Set any missing locale and timezone settings in the image.

### Schema version

Schema version is **31**, up from 28 in v0.9.0-beta.6. Snapshots from earlier
releases must be re-scanned.

### Binaries

Pre-built binaries for 3 platforms:
- `inspectah-darwin-arm64` -- macOS on Apple Silicon
- `inspectah-linux-arm64-bin` -- Linux on ARM64 (static musl binary)
- `inspectah-linux-amd64` -- Linux on x86_64 (static musl binary)

**Full changelog:** https://github.com/marrusl/inspectah/compare/v0.9.0-beta.6...v0.9.0-beta.7
