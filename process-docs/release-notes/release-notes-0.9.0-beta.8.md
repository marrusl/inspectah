# Release Notes: v0.9.0-beta.8

## What's new in v0.9.0-beta.8

This release closes the remaining gaps in credential redaction and makes
unmanaged files behave better in the image. Every credential on a line is
masked, URL passwords are caught in host text, and passwords with spaces are
masked whole. Unmanaged files keep their host permission bits, the audit report
names the ones that lose their owner, and the Containerfile no longer reports
copied files and links as missing. Refine is fast again on hosts with large
repo files, a failed `rpm -Va` is now a warning, and `inspectah version` no
longer prints the commit or build date. The snapshot schema is still 31.

### Breaking changes

inspectah has no backwards-compatibility promise before 1.0.

- **`inspectah version` no longer prints the commit or build date.** A release
  binary prints `inspectah 0.9.0-beta.8`, without the `(commit <hash>, built
  <date>)` that followed. A release build from a git checkout of any other
  commit adds its short hash, such as `inspectah 0.9.0-beta.8+g5a4389ce`, and a
  debug build adds `+dev`. `inspectah --version` prints the same line. Scripts
  that read the commit or date from this line get neither.

### Security and secret redaction

- **Every URL credential on a line is redacted.** A second `user:password@` or
  token URL on one line, a second `proxy_password=` value, a password after an
  empty user name (`https://:pw@host`) and a token before a password
  (`https://<token>:x-oauth-basic@host`) used to ship raw into the snapshot, the
  exports and the image. Each is now masked and reported as its own finding. A
  URL password holding an `@` is masked whole, in PostgreSQL, MongoDB, Redis and
  compose URLs too.
- **URL passwords in host text are redacted.** A credential in a URL with no
  keyword nearby, such as `HTTPS_PROXY=https://user:secret@proxy`, is now masked
  in config files, drop-ins, quadlets, container environments, timers, sysctl,
  kernel arguments, mounts, Flatpak remotes, proxy lines, manifests, firewall
  rules, `/etc/hosts`, sudoers, SELinux file contexts and unmanaged symlink
  targets. A kernel argument holding one is named as owed. Such a symlink is left
  out, with a `FIXME`.
- **Every credential on a language-package manifest line is redacted.** A
  compact `package-lock.json` or `package.json` with several `resolved` or
  `registry` members, a requirements line with several index options, a Gemfile
  line with several sources, comma-joined URLs, and a JSON value with an escaped
  quote before the credential kept credentials raw in the snapshot and exports.
- **Passwords with spaces are redacted whole.** A quoted Wi-Fi `psk`, a
  `password=` or `proxy_password=` value that runs to the end of its line in an
  INI-style file, a quoted `proxy_password`, and a quoted manifest URL holding an
  escaped quote kept their text after the first space or quote. Each is now
  masked to the end of its value.

### Refine

- **Refine is fast again on hosts with large repo files.** On a subscribed RHEL 9
  host, whose `redhat.repo` holds about 500 sections, each refine operation,
  refine startup and the export download took around 13 s. An operation or
  download now takes under a second, and startup measured about 0.4 s median on
  macOS using a snapshot of that host.
- **Refine says why unmanaged files make an export sensitive.** When redacted or
  left-out unmanaged files were the only sensitive content, an export without
  the acknowledgment was refused with an empty `sensitivity_summary`, and the
  export's warnings left out the line the scan prints about them. The summary now
  lists each count, and the export warnings carry the scan's notice.

### Unmanaged files, ownership and the Containerfile

- **Unmanaged files keep their mode in the image.** With `--include-unmanaged`,
  bundled files shipped as 0644, so scripts under `/opt` lost their exec bits and
  0600 files became world-readable. Both tarballs and `inspectah build` now keep
  the host's permission bits, dropping setuid, setgid and sticky. Re-scan to get
  the host modes.
- **The audit report names unmanaged files that lose their owner or special
  bits.** Unmanaged files are still carried as `root:root`, and setuid, setgid
  and sticky are dropped. The audit report now lists each file whose host owner
  or group was not root, or that had one of those bits, with its host owner,
  group and mode, so you can fix it by hand. The Containerfile and the scan's
  output give the count.
- **The Containerfile no longer says a copied unmanaged file was not copied.**
  When a directory `COPY` such as `COPY unmanaged/opt/app/ /opt/app/` carried a
  nested file or directory whose own path holds a space, the Containerfile still
  printed a `FIXME` saying it was not copied, and an unmanaged symlink it carried
  got a `FIXME` saying it was not recreated. A `FIXME` now appears only for what
  no `COPY` line carries.
- **An unmanaged symlink to a directory no longer gains a stray link inside that
  directory.** When a directory `COPY` had already put a link such as
  `/opt/app/current -> v2` in place, the Containerfile's `ln -sf` for it followed
  the link and created `/opt/app/v2/v2`. A link that a `COPY` carries now gets no
  `ln` line, and every other link is recreated with `ln -sfn`, which replaces a
  link already at that path, such as one the base image ships.
- **The Containerfile no longer asks you to recreate an unmanaged symlink the
  export carries.** When the scan could not read a link's target but bundling
  could, the link shipped correctly and the Containerfile still printed
  `# SYMLINK: ... -> unknown (recreate manually)`. The snapshot now records the
  target bundling read, so a link a `COPY` carries gets no comment and any other
  link gets an `ln -sfn` line.

### Reports and scan output

- **A failed `rpm -Va` is a warning, not a silent loss.** When `rpm -Va` fails,
  is killed or cannot run, modified configuration files used to be left out with
  no warning. The scan now prints a warning on stderr, the audit and HTML reports
  list it under Warnings, and the refine export shows it. Check the exported
  configuration against the host, fix what stopped `rpm -Va` and re-scan.
- **The audit report and refine no longer say first boot recreates the NVMe host
  id.** The RHEL 9 bootc base image ships its own `/etc/nvme/hostnqn` and
  `hostid`, so every machine built from it shares one NVMe host identity. The row
  for those files, in the audit report and in refine's machine-state block, now
  says so and how to give each machine its own.
- **The `tmpfiles.d` account notice covers quoted lines, the HTML report and
  refine.** A carried line with a quoted or escaped path or owner is now named
  when its user or group is missing from the image, and the notice appears in
  `audit-report.html` and refine's Configuration Files section as well as the
  Containerfile and `audit-report.md`.
- **The audit report lists Non-RPM Software in a stable order.** Methods with the
  same count, such as `git repo` and `gem system` with one item each, came out in
  a different order on each run, so two reports of the same snapshot differed.
  Equal counts are now listed by method name.
- **Tarball names are in UTC.** `scan` and `aggregate` name their tarball with
  the UTC time and a `Z` suffix, such as `rhel9-20261123-070502Z.tar.gz`, instead
  of the host's local time. The directory inside the tarball follows the same
  name. Tarballs named the old way still load in `refine` and `aggregate`.

### Known limitations

These have workarounds and are planned for a later release.

- **Redaction, kernel command lines**: a command line embedded in another file, such as `kernel_cmdline=` in `/etc/dracut.conf.d`, can keep part of a credential after partial replacement, so review those files before sharing an export. A `rd.luks.key=` value that starts with a Unicode space is not redacted; it is a key-file path, not the key.
- **Redaction, shell-quoted values**: a value written with shell escaping or `$'...'` quoting, such as `GRUB_CMDLINE_LINUX="rd.iscsi.password=\"a b\""` in `/etc/default/grub`, can be only partly redacted in the snapshot. Review such files before sharing an export.
- **Redaction, repositories and `--no-redaction`**: redaction can remove URL text or configuration syntax as well as credentials. Inspect the exported `.repo` files and restore non-secret text before building. A package whose usable repository route is lost gets a `FIXME` naming a switch that restores it or an RPM upload. `--no-redaction` keeps credentials in exports.
- **`/etc/default/grub`**: the file is never copied. It gets an advisory when it sets arguments kargs.d does not carry, or when the scan cannot compare it confidently, as with shell constructs the reader does not model. Review the file and put any additional required arguments in a kargs.d file yourself.
- **Unmanaged files under `/opt` and `/srv`**: private keys in DER or PKCS#12 form and binary OpenPGP packets are not detected, though PEM keys are. A PEM key with odd line breaks (`&#10;`, `%0A`, `<br>`), a UTF-16, compressed or double-encoded file, or a key cut off below the minimum length can also be missed. A file can change between the check and the copy.
- **Unmanaged file ownership**: every file carried from `/opt`, `/srv` and `/usr/local` lands in the image as `root:root`, whatever owner and group it had on the host. Setuid, setgid and sticky bits are dropped; the other permission bits are kept. The audit report lists each affected file with its host owner, group and mode, and the Containerfile and `inspectah scan` give a count. Carried directories are root-owned too and are not listed, because the scan does not record their owner or mode. Where a file needs its owner or bits back, set them in the Containerfile, for example a `RUN chown` or `chmod` after the `COPY`, once the account exists in the image.
- **Configuration file ownership and mode**: carried configuration files under `/etc` land in the image as `root:root` with mode `0644`, whatever owner, group and mode they had on the host. The SSSD configuration is the one exception (`0600`). A file that was `0600` or `0640` on the host is therefore readable by every user in the image, and a file a service reads through its group works only because it is now world-readable. The scan does not record configuration files' owner or mode, so the reports do not list them. Before sharing or deploying an image, check the configuration files that hold secrets or should not be world-readable, and set their owner and mode in the Containerfile with `RUN chown` or `chmod`. Redaction removes credentials it recognizes from file contents, but it does not change file permissions.
- **Repositories on other distributions**: on a host that is not RHEL, CentOS or Fedora, such as AlmaLinux or Rocky Linux, inspectah treats the build's dnf version as unknown. It can then require a key the build would not check, and an installable package can get an upload `FIXME`. Check the Containerfile's `FIXME` lines before building.
- **Repositories, repeated signature options**: inspectah uses the last assignment across `gpgcheck` and `pkg_gpgcheck`, but dnf 5 updates each name's value and applies the names in first-occurrence order. On Fedora, a section that mixes the two names with repeats can fail the build for a missing key or leave out an installable package. Use one signature option per section before scanning.
- **When `rpm -Va` fails**: the scan now warns, on stderr, in the audit report and in the refine export, but on Fedora a stock repo file is still not counted as the base image's, so excluding it can leave its packages with an upload `FIXME`. Resolve the failure and re-scan.
- **Packages and units**: an enabled unit's owner whose dependencies need a repo disabled on the host is not carried by default. The notice names it and says why, and you can include it. Refine rows do not show the hold reason; the Containerfile and the audit report do. Guarded unit commands are not shown in refine.
- **Package dependencies**: `Supplements` conditions are judged on the source host, and the dnf fallback ignores weak dependencies. Review the built image's packages and unit commands, and include any required package the export leaves out.
- **SELinux mode**: the recorded running mode has no export command. An unowned or content-modified `/etc/selinux/config` can still be copied as configuration, but a `%ghost` copy is skipped. Check the file and set the image's intended mode yourself.
- **`/etc/crypttab`**: storage does not parse encrypted-volume mappings. An unowned or content-modified copy can appear under Configuration Files, where refine locks it out as merge-hostile; a `%ghost` copy is skipped. Provide any encrypted-volume configuration yourself.
- **Locale and timezone**: the recorded values have no export command, and `/etc/localtime` is not copied. An unowned or content-modified `/etc/locale.conf` can still be copied as configuration; a `%ghost` copy is skipped. Set any missing locale and timezone settings in the image.
- **Redaction, URL user names that are tokens**: in general host text, a URL user name is masked only when it looks like a token. A short token, a dotted one such as a JWT, a percent-encoded one, or base64 ending in `=` used as the user name (`https://<token>@host/`) is not masked, and an unmanaged symlink whose target is such a URL is not withheld. Repository URLs and the supported manifest credential fields mask the whole user info whatever it looks like. Review other files that hold URLs with a user name before sharing an export.
- **Redaction, URL passwords and later `@` characters**: a URL password is masked through the last `@` of its authority, which ends at the first `/`, `?`, `#` or whitespace. A later `@` before one of those, as in `https://u:p@host,notify=a@b`, makes the mask consume the host and the non-secret text between. No secret is exposed, but check the exported line and restore the text.
- **NVMe host ids on RHEL 9 bootc images**: the base image bakes in `/etc/nvme/hostnqn` and `hostid` and nothing regenerates them, so every machine built from it shares one NVMe host identity. The audit report and refine now say so, but the machine-state count still lists them as recreated without operator action. Delete both files in the Containerfile or write new ones at provisioning.
- **`tmpfiles.d` lines owned by a skipped account**: a carried line whose user or group the image does not create is still skipped at boot, now with a notice in every report. Its directory may not be created at all, because the line still counts as backing it. Create the account, change the owner in the Containerfile, or create the directory yourself.
- **`/srv` mount units**: a mount unit for `/srv` is carried under its original name with an advisory, not rewritten. `/srv` is a symlink into `/var` on the bootc base images, so rename the unit to `var-srv.mount` and set `Where=/var/srv`.

### Schema version

Schema version is **31**, unchanged from v0.9.0-beta.7. Snapshots from
v0.9.0-beta.7 still load.

### Binaries

Pre-built binaries for 3 platforms:
- `inspectah-darwin-arm64` -- macOS on Apple Silicon
- `inspectah-linux-arm64-bin` -- Linux on ARM64 (static musl binary)
- `inspectah-linux-amd64` -- Linux on x86_64 (static musl binary)

**Full changelog:** https://github.com/marrusl/inspectah/compare/v0.9.0-beta.7...v0.9.0-beta.8
