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

## Matching a section to its key

The key's `path` is the expanded path, but every consumer that reads the
repo file back (`parse_repo_sections`, so `RepoSectionDef::local_keys`)
sees the literal `RPM-GPG-KEY-EPEL-$releasever_major`. So the collector
also records that literal in the key's `gpgkey_references` (only when it
differs from `path`), and **every match of a section's key path to a
captured key goes through `RepoFile::is_key_named_by`**, never `k.path ==`.
Comparing paths alone makes a carried key read as missing: the Containerfile
demotes `epel-release` behind a false "not carried" FIXME, the audit says
"no", and refine marks the section Incomplete.

Routed through it today: `render/repo_enabling.rs` `keys_included` (which
`section_survives`, `repo_enabling_export`, the audit Carried column,
`unit_plan` and `repoless::disabled_repo_install` all use), and core's
`repo_ownership`, which `RepoIndex::build`, the ownership pass and the
load check all read, so the owners, the keeper rule and the session's
exclude/include cascade work on real key paths. `is_key_named_by` also
matches by `Destination`, so a reference written `/etc/pki/rpm-gpg//KEY`
names the key at `/etc/pki/rpm-gpg/KEY`.

**One parser.** `parse_repo_sections` is the only reader of `.repo`
content. It continues an indented line after a `key=value` line, keeps
each option's last assignment, drops lines before the first header, and
reads a value with no `://` as a local path (`GpgKeyRef::Local`).
`extract_gpg_keys` reads the keys of each section it returns, so a key a
replaced or headerless `gpgkey=` named is not collected, and
`collect_repo_files` keeps no file that defines no section. The aggregate
merge unions `gpgkey_references` per key path before it keeps one host's
entry. The values themselves are not stored: they differ between an EL9
and an EL10 host in one aggregate, and storing them would move the
expander into every consumer.

## How dnf derives the values

Sources: dnf4 `dnf/cli/cli.py` (`_read_conf_file`), `dnf/rpm/__init__.py`
(`detect_releasevers`), `dnf/const.py.in`, `dnf/conf/substitutions.py`;
libdnf `conf/Const.hpp` and `conf/ConfigParser.cpp`; libdnf5
`conf/vars.cpp` and `include/libdnf5/conf/const.hpp`.

- **Which dnf.** RHEL 9 and RHEL 10 run dnf4 (measured: `4.14.0` on
  RHEL 9.8, `4.20.0` on RHEL 10.2); Fedora runs dnf5. The dnf5 code paths
  are for Fedora. The collector runs `dnf --version`: dnf5 names itself on
  the first line (`dnf5 version 5.2.8.1`, not yet measured on a real
  host), dnf4 prints a bare `4.x` version. Only when `dnf` cannot be run
  does an installed `/usr/bin/dnf5` decide. The binary alone would be
  wrong on a host carrying dnf5 beside the dnf4 that `dnf` runs (dnf5 is
  not installable on RHEL 9.8 from BaseOS, AppStream or EPEL 9).
- **Vars files.** A file in a vars directory defines a variable named after
  the file; its value is the first line. dnf4 reads `/etc/yum/vars` then
  `/etc/dnf/vars`. dnf5 reads `/usr/share/dnf5/vars.d` then
  `/etc/dnf/vars`, and does not read `/etc/yum/vars`. A later directory
  wins.
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
  release package overrides the split. A vars file naming either one
  beats the split in both dnfs. Against the provide, the collector follows
  the host's dnf (`DnfFlavor::release_part`): dnf4, so RHEL 9 and 10, sets
  the detected provide after reading the vars files (`_read_conf_file`),
  so the provide wins; dnf5, so Fedora, gives a vars file a higher
  priority than detection (libdnf5 `Vars`), so the file wins. Current
  dnf5 (`main` after commit `f9bc2076a3`) sets `releasever_major` to an
  empty string when the release package lacks the
  `system-release(releasever_major)` provide, overwriting the split; the
  collector splits instead, because the empty value names
  `RPM-GPG-KEY-EPEL-`, a key no image has, and the split is what dnf4 and
  older dnf5 give. RHEL 9.8 and 10.2 both carry the provide, so the split
  is not used there.
- **`arch` / `basearch`.** The machine type and dnf's base architecture
  for it (`_BASEARCH_MAP` in `dnf/rpm/__init__.py`). The collector reads
  `uname -m` and maps the architectures EL and Fedora build for; an
  unknown one leaves both undefined.
- **Not read:** `DNF_VAR_*` and `DNF0`-`DNF9` environment variables. They
  belong to one dnf invocation, not to the host.

Syntax: `$name`, `${name}`, `${name:-word}` (word when unset or empty) and
`${name:+word}` (word when set and non-empty), with names of ASCII letters,
digits and `_`. dnf leaves an unknown `$name` as written, and a `$` with no
name after it (`$`, `$-x`); the collector leaves the latter too.

Escapes follow the host's dnf. dnf4 (libdnf `ConfigParser`) drops a
backslash before any character; dnf5 (libdnf5 `Vars`) drops it only before
`$` and `\`, and before `}` inside a `${name:-word}` word, and otherwise
keeps it. Both keep a trailing backslash.

## What the collector refuses

A reference that cannot be expanded safely is not read. It becomes a
snapshot warning naming the repo file and the reason, as does a path that
expands but cannot be read:

- an unknown variable (`$name` or `${name}` the host does not define);
- a variable value other than ASCII letters, digits and `._+-~`, or one
  that is `.` or holds `..`: this refuses `/`, whitespace, line breaks,
  control and shell characters, and traversal;
- a `${` that does not close as `${name}`, `${name:-word}` or
  `${name:+word}`: unterminated, with no name, or with an unknown operator.
  dnf leaves those as written, but a key path holding a literal `${` is not
  one a repo means;
- a file that is read but has no line beginning with
  `-----BEGIN PGP PUBLIC KEY BLOCK-----` (spaces or tabs before it are
  allowed; the header inside a line is not). It is still recorded,
  excluded and with its content replaced, so nothing ships it. Text
  before the header is fine: the stock `RPM-GPG-KEY-redhat-release`
  opens with a paragraph of prose, and refusing it gave every RHEL host a
  false warning;
- a file that holds PGP private or secret key armor (`BEGIN` or `END`
  line) anywhere, even above a valid public block. Accepting a header on
  any line would otherwise ingest it verbatim, and the redaction patterns
  do not match PGP armor. It has its own reason,
  `UnreadReason::PrivateKeyMaterial`, so the warning and the placeholder
  say it held private key material rather than that it lacked a public
  block, which would be false for this file.

The warning is conditional ("If the image enables a repo from that file")
rather than gated on an enabled section, since a repo disabled on the host
can be enabled in the image, and it ends with what to do.

Remote references (any `scheme://` other than `file://`) are skipped
without a warning: dnf fetches them at build time.

## One entry per key path, and keys left to the base image

Every section that names a key yields a key entry, and a RHEL host's
`redhat.repo` names the stock key from hundreds of sections (209 on RHEL
10.2, 500 on RHEL 9.8). `repos::record_each_key_once` collapses them per
path and unions `gpgkey_references`. Snapshots from before carried one
entry per section. They predate schema 26, so the loader rejects them, but
consumers still act on **every** entry at a path (the refine cascade
does), never on the first match only, and repeated entries render and
refine as before.

A key whose path a **base-image** package owns (`file_ownership` against
the baseline's package names) and which `rpm -Va` did not report
(`rpm.rpm_va`) is the base image's own file. `leave_out_keys_the_base_ships`
records it excluded with that package in `shipped_by_base`, so it is not
copied over the base image's copy, which for a RHEL 9 host on a RHEL 10
base would replace the EL10 key with the EL9 one. Being owned by *some*
package is not enough: EPEL's key is owned by `epel-release`, which the
base image lacks, and leaving it out breaks the EL10 EPEL build exactly as
on 2026-11-09. No baseline, a host that is not package-based (no `rpm
-Va`), an `rpm -Va` that failed, or an ownership query that failed all
mean carry: a missing key fails the build, a redundant copy does not.
`rpm -Va` exits 1 whenever it reports a modified file, so a non-zero
exit alone is not failure. The collector accepts output only with exit 0
or 1, reads exit 0 with no output as a clean verify, and reads everything
else as failed (`SupplementaryData::rpm_va` is `None`): exit 1 with no
output, and any other exit code even with output, since a signal kill
(exit -1 from the executor) can leave a partial report. A run that stops
partway yet exits 1 looks like a full report and cannot be caught.

Consumers ask `RepoFile::is_in_image` ("copied or shipped by the base"),
not `disposition.is_included()`, when the question is whether dnf in the
image will find the key (`keys_included`, `dropped_owned_path`). The
refine include cascade skips a `shipped_by_base` key, the aggregate keeps
the field only when every host that has the key set it, and the one
wording is `RepoFile::base_image_ships_reason`. A key the scan refused
carries `rejected` and is never offered, counted or re-included. Neither
kind is carriable: no repo op includes one, and the operator sees the
decision in the Containerfile comment, the audit table and the view's
`repo_owned` entry.

## What a repo exclusion drops

The repo section owns every `.repo` file and every key a section names
(Mark Russell, 2026-11-21; spec
`2026-11-21-repo-section-owns-repo-files-and-keys.md`). Their
Configuration rows are inventory shadowed by `Repositories`, so the repo
switch is the one control, and the stager writes the repo inventory's
payload.

A section **keeps** an entry it owns while it is kept (no repo op
excluded it) and live: the host enabled it in the file that defines it,
or the host disabled it and an included, unlocked package attributed to
it installs with `--enablerepo`. `RepoIndex::kept_by` is the one rule.
An exclude drops each carriable entry the repo owns that no other section
keeps; an include restores every carriable entry it owns. Keepers are
read at the op, against the projection at that point. EPEL's
`epel.repo` holds `[epel-debuginfo]` and `[epel-source]` at `enabled=0`,
and excluding `epel` still drops the file and key.

Every repo with a carriable entry has a control. A toggleable repo (not
distro, enabled where dnf reads it) has a full switch; a distro or
host-disabled repo that owns a carriable entry has a carriage-only switch
that never moves members. A repo whose members are all excluded, or that
has none, can still be switched (position 26 reopened). The no-op rule
replays the op and refuses it only when nothing moves.

With no repo op the projection keeps the scan's dispositions, so a file
whose sections are all disabled on the host still ships in the scan
export and in an unchanged refine export, and so does a key only disabled
sections name. A `.repo` file that redaction leaves with no header, or a
key whose only `gpgkey=` reference redaction rewrote, has no owner after
`finish_scan`: it is excluded, never staged, and every export names it.

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
