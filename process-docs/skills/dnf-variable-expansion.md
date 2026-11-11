---
name: dnf-variable-expansion
description: How dnf derives $releasever, $releasever_major, $basearch and vars-file variables, and how the collector expands them in gpgkey= paths.
---

# dnf Variable Expansion

Repo files name GPG keys with dnf variables. EL10's `epel.repo` says
`gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-$releasever_major`. Read
literally, that path does not exist, the key is never collected, and the
image build fails at `dnf install` with `Curl error (37)`. EL9's
`epel.repo` names `RPM-GPG-KEY-EPEL-9` literally, which is why the defect
showed only on EL10.

`crates/collect/src/inspectors/rpm/dnf_vars.rs` expands them with the
scanned host's values, and `repos::extract_gpg_keys` records the key at the
expanded path.

## How dnf derives the values

Sources: dnf4 `dnf/cli/cli.py` (`_read_conf_file`), `dnf/rpm/__init__.py`
(`detect_releasevers`), `dnf/const.py.in`, `dnf/conf/substitutions.py`;
libdnf `conf/Const.hpp` and `conf/ConfigParser.cpp`; libdnf5
`conf/vars.cpp` and `include/libdnf5/conf/const.hpp`.

- **Vars files.** A file in a vars directory defines a variable named after
  the file; its value is the first line. dnf4 reads `/etc/yum/vars` then
  `/etc/dnf/vars`. dnf5 reads `/usr/share/dnf5/vars.d` then
  `/etc/dnf/vars`, and does not read `/etc/yum/vars`. A later directory
  wins. The collector picks the dnf5 list when `/usr/bin/dnf5` exists.
- **`releasever`.** A vars file wins. Otherwise dnf asks the rpmdb for the
  first package that provides `system-release(releasever)`,
  `system-release`, `distribution-release(releasever)`,
  `distribution-release`, `redhat-release` or `suse-release`, in that
  order. The value is the version of that package's `=` provide of the
  capability, unless the package is named after the capability, in which
  case it is the package version.
- **`releasever_major` / `releasever_minor`.** Setting `releasever` sets
  both by splitting at the first `.` (`10.2` gives `10` and `2`). A
  `system-release(releasever_major)` or `(releasever_minor)` provide on the
  release package overrides the split. A vars file naming either one wins
  in dnf5. In dnf4 a detected provide beats a vars file for these two; the
  collector follows dnf5, since EL10 is the host where the variable is used.
- **`arch` / `basearch`.** The machine type and dnf's base architecture
  for it (`_BASEARCH_MAP` in `dnf/rpm/__init__.py`). The collector reads
  `uname -m` and maps the architectures EL and Fedora build for; an
  unknown one leaves both undefined.
- **Not read:** `DNF_VAR_*` and `DNF0`-`DNF9` environment variables. They
  belong to one dnf invocation, not to the host.

Syntax: `$name`, `${name}`, `${name:-word}` (word when unset or empty) and
`${name:+word}` (word when set and non-empty), with names of ASCII letters,
digits and `_`. dnf leaves an unknown `$name` as written.

## What the collector refuses

A reference that cannot be expanded safely is not read. It becomes a
snapshot warning naming the repo file and the reason, as does a path that
expands but cannot be read:

- an unknown variable (`$name` or `${name}` the host does not define);
- a variable value other than ASCII letters, digits and `._+-~`, or one
  that is `.` or holds `..`: this refuses `/`, whitespace, line breaks,
  control and shell characters, and traversal;
- a backslash, a `$` with no name, or an unterminated or unknown `${...}`
  form.

Remote references (any `scheme://` other than `file://`) are skipped
without a warning: dnf fetches them at build time.

## Repo file syntax

`extract_gpg_keys` reads `gpgkey=` the way libdnf's `IniParser` does: the
key starts in column 0 and may have spaces around `=`; an indented line
continues the previous value; a comment or empty line ends it. Keys in one
value are separated by whitespace or commas.

## Caveat

The values are the source host's. When the image's base is a different
major from the host, dnf in the image expands the same `epel.repo` with the
image's values, so the key name the collector staged may not be the one
the image asks for.
