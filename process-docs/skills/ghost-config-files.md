# `%ghost` files under `/etc`, and authselect

## Why a branch of their own

A package lists a `%ghost` file without shipping its content. rpm has
nothing to compare it with, so `rpm -V` never reports it, and the
unowned walk skips it because a package owns it. Without the ghost
branch, an admin's edit to one reaches no collector. Measured on RHEL
9.8 and 10.2 (and Fedora 44 for grub): `/etc/sssd/sssd.conf` (`cng`),
`/etc/authselect/authselect.conf` and `nsswitch.conf` (`g`, no config
flag), `/etc/default/grub` (`cng`), `/etc/machine-id` and
`/etc/hostname` (`cng`).

## The rule

`crates/collect/src/inspectors/config/ghost.rs` and
`collect_ghost_configs` in `config/mod.rs`:

- **Found by one query:** `rpm -qa --queryformat
  '[%{=NAME} %{FILEFLAGS:fflags} %{FILENAMES}\n]'`. `%{=NAME}` repeats
  the scalar on each array iteration. The path is last and the line is
  split at its first two spaces, so a path with a space survives; an
  empty `fflags` leaves two spaces. A flags field with a byte outside
  `dcsmnglra` is not trusted.
- **Every ghost file under `/etc`, not only `%config`:** authselect's
  files carry `g` alone. Requiring `c` loses them.
- **Collected** as `RpmOwnedModified`, `rpm_va_flags: None`, with its
  package and content (redaction applies), when it exists as a regular
  file: not a symlink, not a directory (`%ghost %dir`).
- **Skipped:** paths `rpm -Va` reported; every skip the unowned walk
  applies (`is_excluded_unowned`, dev artifacts, DHCP, SELinux fcontext
  store); machine-state members (`is_machine_state_member`, the § 5.2
  list of the machine-state class spec); `is_ghost_skip` (locale and
  keymap files, which an open locale item owns; the `sscg` certificate;
  `crypto-policies/state/`; authselect's generated files).
- **Not run** when `owned_paths` is empty; a failed query degrades the
  section. Mocks that run the config inspector with owned paths must
  register the query (an empty answer is fine), or the section reads
  degraded.

When the machine-state class lands, the member skip goes and the push
becomes `ConfigCollector::admit` with the same fields.

## authselect

- `/etc/authselect/authselect.conf` (profile id, then one feature per
  line) is collected, never staged (`owned_elsewhere`), and rendered by
  `render/authselect.rs` as `RUN authselect select <profile> <features>
  --force` after the Configuration Files `COPY`, so a custom profile is
  in place. Every word is a `BareWord`; a refusal renders a FIXME.
- The generated files (`nsswitch.conf`, the PAM stacks, `dconf-*`) are
  not collected: `authselect select` writes them again.
  `user-nsswitch.conf` is admin input and is carried as a file.
- `authselect check` runs when the selection is collected. Each file it
  names in brackets becomes a `Modernization` advisory (an existing
  entry at that path is turned into one, so it is not copied over what
  `select` writes). A failure naming no file is a scan warning.

## Other files the branch reaches

- `sssd.conf` and `conf.d/*.conf`: carried; the Containerfile adds
  `RUN chmod 0600` after the `COPY`, because SSSD refuses any other mode
  and `COPY` gives the build context's. `*_authtok` is a credential key
  in the redaction patterns.
- `/etc/default/grub`: the `grub_defaults` modernization rule makes it an
  advisory on any branch; it is never copied. Translating its
  `GRUB_CMDLINE_LINUX` into kargs.d is not done.
