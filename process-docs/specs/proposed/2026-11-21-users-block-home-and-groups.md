# The Users Block Adds Accounts Without a Home in the Image

## Metadata

| Field    | Value |
|----------|-------|
| Date     | 2026-11-21 |
| Status   | Proposed |
| Release  | 0.9.0-beta.7 |
| Decision | The users block emits `useradd -M`, with no home in the image. A `tmpfiles.d` line, `d /var/home/<user> 0700 <user> <group> -`, creates the home with the right owner at first boot. `authorized_keys` and other home content are left to provisioning, with one comment line naming the key file the host had. A primary group the base lacks gets a `groupadd`. Accepted cost: an operator who relied on keys baked into the image must provision them, and the `sysusers` lint warning stays (Mark Russell, 2026-11-21). |
| Verified | Code citations checked against inspectah commit `102a9f77`. Base-image behaviour is from a measured consult on RHEL 9.8 and 10.2 `rhel-bootc` bases (`registry.redhat.io/rhel9/rhel-bootc:9.8` and `rhel10/rhel-bootc:10.2`, bootc 1.16.4), 2026-11-21, which ran `podman build`, `bootc container lint` and `systemd-tmpfiles --create` in each. No new host run for this spec. Claims marked *recalled* were not checked. |
| Closes | Failures 1 and 3 of the users block build defect; the users block's unannounced `/var` write. |

## 1. Problem

`render_containerfile_users` (`crates/pipeline/src/render/users.rs:380-562`) fails `podman build` on a RHEL bootc base as soon as an operator sets a user to useradd in refine. Two of its failures are in scope:

1. **Home creation.** It emits `useradd -m` with the host's `/home/<user>`, then `install -d` of `<home>/.ssh`, a `COPY` of `users/home/<user>/.ssh/authorized_keys`, and a `chown`. On the base, `/home` is a symlink to `var/home` and `/var/home` does not exist, so `useradd -m` exits 12 and `install -d` exits 1. Measured on both releases. Even where it built, it would have written the key into the image's `/var`, which bootc unpacks only at first install, so a later key rotation would never reach a deployed host.
2. **Missing primary group.** `groupadd` is emitted only for groups the collector marks `custom` (gid 1000 to 59999) and that are included. A primary gid outside that range, or a custom group the operator excluded, leaves `useradd -g <gid>` naming a group that does not exist: exit 6.

The consult found two more facts that change the fix:

- **The base keeps most groups in `/usr/lib/group`.** The base `/etc/group` holds six groups (`root`, `adm`, `wheel`, `systemd-journal`, `tss`, `kvm`). The other 55 (9.8) or 61 (10.2) are in `/usr/lib/group`, read through nss-altfiles. `groupadd` checks through NSS, so it refuses a name or gid that exists only there (exit 9 or 4). Reading the base's `/etc/group` alone would get this wrong.
- **`useradd -G <g>` fails (exit 6) when `<g>` is only in `/usr/lib/group`**, because supplementary membership is written to `/etc/group` and the group is not there. Copying the entry first, `getent group <g> >> /etc/group`, fixes it (measured, and documented upstream in bootc's `bootc-users-and-groups.7.md`, "Modifying or adding members to system groups"). It must not be done for a group already in `/etc/group`: duplicate entries break the tools.

## 2. What this spec settles

1. Whether the pipeline can read the baseline image's groups cheaply, and the choice between a build-time guard and a scan-time read (§ 3).
2. The exact rendered output (§ 4).
3. What the block stops writing (§ 5).
4. Where the tmpfiles file comes from, and how it coexists with the `/var` provisioning fragment (§ 6).
5. Refine and both export paths (§ 7).
6. Snapshot and API changes, and the changelog (§ 8).
7. Tests (§ 9).

Out of scope: § 10.

## 3. Reading the base's groups: build time, not scan time

### 3.1 How the baseline image is read today

`extract_baseline` (`crates/collect/src/baseline.rs:146-260`) runs at scan time: `podman image exists` or `podman pull`, `podman create --network none` of the image with `sleep infinity` as entrypoint, `podman start`, one `podman exec <ctr> rpm -qa --queryformat ...`, `podman rm -f`, then `podman inspect` for the digest. The result is `BaselineData { image_digest, packages, extracted_at }` (`crates/core/src/baseline.rs:89-93`), stored in `snapshot.baseline`. Only the package set is read. No file from the image is read today.

So a scan-time read **is** cheap: one more `podman exec <ctr> getent group` and one `podman exec <ctr> cat /etc/group` in a container that already exists. It would need a new `BaselineData` field and a schema bump.

### 3.2 Why the build-time guard wins anyway

The users block runs after the package section (`containerfile.rs:242`, section 8, after section 1's `dnf install`). A package the image installs can create the very group a user needs (a `docker` group, a service group), and a scan-time read of the bare base cannot see it: the export would emit a `groupadd` the build then refuses with exit 9. A build-time guard reads the image as it stands at that line, after every package, on the base the build actually uses, including one the operator changed in `FROM` after the scan. It needs no snapshot field for the base and no second code path for a snapshot without a baseline.

Does it still read as hand-written? For a primary group, yes: `getent group <g> >/dev/null || groupadd -g <gid> <g>` is the idiom Fedora's packaging guidelines long used for creating a group in a scriptlet (*recalled*), and a person writing this Containerfile would reach for it. A group that `useradd -G` names needs one more clause to copy a `/usr/lib/group` entry into `/etc/group`; that line is longer, and one comment above the group lines explains it once (§ 4.1). The scan-time read would give plainer lines (`RUN groupadd -g 1001 appgrp`, `RUN getent group users >> /etc/group`) but would be wrong for package-created groups, so plainness loses to correctness here.

### 3.3 What the snapshot must carry for the guard

The guard needs each group's name and the host's gid, to create it when neither the base nor a package did. Today the user entry has the primary `gid` but no group name, and `groups` holds only gids 1000 to 59999 (`parse_group`, `crates/collect/src/inspectors/users.rs:403-452`). A primary gid of 100, 65534 or above 60000 has no name anywhere in the snapshot, which is why the 2026-11-19 attempt, which inferred names from gaps, failed.

The collector already re-reads the whole host `/etc/group` for supplementary membership (`enrich_supplementary_groups`, `users.rs:654-700`). In the same pass it adds two fields to each user entry:

- `primary_group`: the name of the host group whose gid is the user's `gid`, from any gid range. Absent when the host's `/etc/group` has no entry for that gid.
- `group_gids`: an object mapping the name of the primary group and of each supplementary group to its host gid, for example `{"appgrp": 1001, "wheel": 10}`.

These are collector-produced. The renderer never infers a name from a gid range.

## 4. Rendered output

### 4.1 Rules

The block is one section, `# === Users and Groups ===`, rendered for users with `containerfile_strategy == "useradd"` and `include != false`, in this order:

1. **The update comment**, three lines, once:
   ```
   # Users are written to /etc/passwd at build time. A host whose /etc/passwd
   # was changed locally keeps its own copy on update, so a user added or
   # changed in a later image does not reach that host.
   ```
   This is the case the `sysusers` lint warns about at the end of the file. On such a host the user's tmpfiles line is then skipped with one journal line (`systemd-tmpfiles` exits 65, which the unit's `SuccessExitStatus=DATAERR CANTCREAT` accepts, so the host is not degraded). The comment says it where an operator edits users; the README does not repeat it.
2. **Group lines**, one per distinct group name the rendered users need (each user's primary group, then its supplementary groups, in user order, first appearance wins), after a two-line comment:
   ```
   # Each group keeps the base image's entry when it has one. useradd -G needs
   # the entry in /etc/group itself, so one kept in /usr/lib/group is copied there.
   ```
   - A group no rendered user names in `-G`:
     `RUN getent group <g> >/dev/null || groupadd -g <gid> <g>`
   - A group some rendered user names in `-G`:
     `RUN grep -q '^<g>:' /etc/group || getent group <g> >> /etc/group || groupadd -g <gid> <g>`
     In the pattern, each `.` in the name is written `\.`; it is the only regular-expression character `AccountName` admits.
   - `<gid>` is from `group_gids`. The group's `include` flag is not consulted: refine has no group exclusion action, and a user's line needs the group whatever its flag says. This is what fixes failure 3 for an excluded custom group.
   - The comment line is emitted only when at least one group line is.
3. **`useradd` lines**, one per user:
   `RUN useradd -M -u <uid> -g <primary_group> [-G <g1>,<g2>] -s '<shell>' -d '<home>' <name>`
   `-M` is explicit: the bases set `CREATE_HOME yes`, so a plain `useradd` fails like `-m`. `-g` takes the group **name**, so that a base entry with the same name and a different gid is used as-is rather than failing on a gid that does not exist. `-d` keeps the host's path (`/home/<user>`), which resolves through the `/home` symlink. Fields are proved as today (`useradd_refusal`), plus `primary_group` and every `group_gids` key as `AccountName`.
4. **`chpasswd`**, unchanged.
5. **The home file**, when at least one user's home maps to a tmpfiles line (§ 4.3), after a three-line comment:
   ```
   # Home directories are created at boot by systemd-tmpfiles, not in the image.
   # tmpfiles resets their owner and mode on every boot; to change that on one
   # host, put a file named inspectah-users.conf in /etc/tmpfiles.d/.
   ```
   One user: `RUN printf '%s\n' '<line>' > /usr/lib/tmpfiles.d/inspectah-users.conf`. More than one: the same `printf` with one quoted line per user, each on its own continuation line (example in § 4.2).
6. **The keys comment**, when at least one rendered user has `ssh_key_count > 0`:
   ```
   # SSH keys are not part of this image. Provision them at install time with
   # kickstart (sshkey --username=<user>), cloud-init, or a systemd credential.
   ```
   When at least one such user also has `ssh_keys` (the scan ran with key preservation), a third line follows: `# inspectah-users.ks has the sshkey lines for the keys this scan kept.` Then one line per user with keys:
   `# <name> had <n> key(s) in <home>/.ssh/authorized_keys on the host.` (`1 key` / `<n> keys`)

**Refusals.** A user whose `gid` has no `primary_group` gets `# FIXME: user <name> not created: its primary group (gid <gid>) has no entry in the host's /etc/group` and nothing else: no group lines on its behalf, no `useradd`, no password, no home line, no key line. A user whose `primary_group` or any `group_gids` key fails `AccountName` gets the existing "a supplementary group is not a valid group name" refusal, reworded to "a group name is not valid". A refusal costs the whole user, as today.

**Root.** Root never reaches this block: the collector records users with uids 1000 to 59999 only (`NON_SYSTEM_UID_MIN`/`MAX`, `users.rs:28-35`), and `collect_ssh_keys` reads keys only for those users. The keys comment therefore never points at `bootc install --root-ssh-authorized-keys`, which provisions root only, and the renderer has no root branch.

### 4.2 Examples

The host for these examples, as the collector would record it:

| user | uid | primary group (gid) | supplementary | keys |
|---|---|---|---|---|
| `alice` | 1001 | `appgrp` (1001), not in the base | `wheel` | 2, not preserved |
| `carol` | 1002 | `users` (100), in the base's `/usr/lib/group` | none | none |
| `dave` | 1003 | `dave` (1003) | `users`, only in the base's `/usr/lib/group` | none |
| `erin` | 1004 | `erin` (1004) | `wheel`, in the base's `/etc/group` | none |

All have shell `/bin/bash` and home `/home/<name>`.

**A primary group the base lacks** (`alice` alone, the L5 case):

```
# === Users and Groups ===
# Users are written to /etc/passwd at build time. A host whose /etc/passwd
# was changed locally keeps its own copy on update, so a user added or
# changed in a later image does not reach that host.
# Each group keeps the base image's entry when it has one. useradd -G needs
# the entry in /etc/group itself, so one kept in /usr/lib/group is copied there.
RUN getent group appgrp >/dev/null || groupadd -g 1001 appgrp
RUN grep -q '^wheel:' /etc/group || getent group wheel >> /etc/group || groupadd -g 10 wheel
RUN useradd -M -u 1001 -g appgrp -G wheel -s '/bin/bash' -d '/home/alice' alice
# Home directories are created at boot by systemd-tmpfiles, not in the image.
# tmpfiles resets their owner and mode on every boot; to change that on one
# host, put a file named inspectah-users.conf in /etc/tmpfiles.d/.
RUN printf '%s\n' 'd /var/home/alice 0700 alice appgrp -' > /usr/lib/tmpfiles.d/inspectah-users.conf
# SSH keys are not part of this image. Provision them at install time with
# kickstart (sshkey --username=<user>), cloud-init, or a systemd credential.
# alice had 2 keys in /home/alice/.ssh/authorized_keys on the host.

```

At build time: `getent group appgrp` finds nothing, so `groupadd` creates it in `/etc/group`; `wheel` is already in the base `/etc/group`, so `grep` succeeds and nothing is written; `useradd -M` exits 0 and writes nothing under `/var` (measured, with the mailbox message below).

`/usr/lib/tmpfiles.d/inspectah-users.conf` in the image:

```
d /var/home/alice 0700 alice appgrp -
```

**All four users:**

```
# === Users and Groups ===
# Users are written to /etc/passwd at build time. A host whose /etc/passwd
# was changed locally keeps its own copy on update, so a user added or
# changed in a later image does not reach that host.
# Each group keeps the base image's entry when it has one. useradd -G needs
# the entry in /etc/group itself, so one kept in /usr/lib/group is copied there.
RUN getent group appgrp >/dev/null || groupadd -g 1001 appgrp
RUN grep -q '^wheel:' /etc/group || getent group wheel >> /etc/group || groupadd -g 10 wheel
RUN grep -q '^users:' /etc/group || getent group users >> /etc/group || groupadd -g 100 users
RUN getent group dave >/dev/null || groupadd -g 1003 dave
RUN getent group erin >/dev/null || groupadd -g 1004 erin
RUN useradd -M -u 1001 -g appgrp -G wheel -s '/bin/bash' -d '/home/alice' alice
RUN useradd -M -u 1002 -g users -s '/bin/bash' -d '/home/carol' carol
RUN useradd -M -u 1003 -g dave -G users -s '/bin/bash' -d '/home/dave' dave
RUN useradd -M -u 1004 -g erin -G wheel -s '/bin/bash' -d '/home/erin' erin
# Home directories are created at boot by systemd-tmpfiles, not in the image.
# tmpfiles resets their owner and mode on every boot; to change that on one
# host, put a file named inspectah-users.conf in /etc/tmpfiles.d/.
RUN printf '%s\n' \
    'd /var/home/alice 0700 alice appgrp -' \
    'd /var/home/carol 0700 carol users -' \
    'd /var/home/dave 0700 dave dave -' \
    'd /var/home/erin 0700 erin erin -' \
    > /usr/lib/tmpfiles.d/inspectah-users.conf
# SSH keys are not part of this image. Provision them at install time with
# kickstart (sshkey --username=<user>), cloud-init, or a systemd credential.
# alice had 2 keys in /home/alice/.ssh/authorized_keys on the host.

```

Per case:

- **Primary group the base has** (`carol`, `users`): `users` is also named in `dave`'s `-G`, so it gets the long form, and `getent` copies `users:x:100:` from `/usr/lib/group` into `/etc/group`. Had no user named it in `-G`, it would get `RUN getent group users >/dev/null || groupadd -g 100 users`, which finds it and writes nothing; `useradd -g users` works against the `/usr/lib/group` entry (measured with `-g 100`).
- **Primary group the base lacks** (`appgrp`, `dave`, `erin`): created with the host's gid.
- **Supplementary group only in `/usr/lib/group`** (`users` for `dave`): copied into `/etc/group` before `useradd -G users` (measured: exit 6 without the copy, 0 with it).
- **Supplementary group in `/etc/group`** (`wheel` for `alice` and `erin`): `grep` matches and nothing is written, so no duplicate entry.
- **A supplementary group the image does not have at all**, such as a package-created group whose package the image does not install: the last clause creates it with the host's gid, so the membership is kept and the build goes on.
- **Keys**: only `alice` has keys. With key preservation on, the extra `inspectah-users.ks` line appears after the two pointer lines.

**Expected build-log noise.** `useradd` prints `Creating mailbox file: No such file or directory` on these bases (no `/var/spool/mail`) and still exits 0. Measured; not a failure, and the export does not try to silence it.

### 4.3 Which homes get a tmpfiles line

| host home | line | why |
|---|---|---|
| `/home/<rest>` | `d /var/home/<rest> 0700 <user> <primary_group> -` | `/home` is a symlink to `var/home` on the base; the real path is written, matching the passwd entry's resolved home. A nested `<rest>` gets its missing parents from tmpfiles as root 0755 (*recalled*); `/var/home` itself comes from the base's `rpm-ostree-0-integration.conf` (measured). |
| under `/var/` | none from this block | The `/var` provisioning fragment owns every `/var` directory the scan walked (§ 6.2). |
| `/` | none | It exists. |
| anything else | none, plus `# <name>'s home, <home>, is not created by this image.` after the tmpfiles lines | Under `/usr` it would be image content; `/opt`, `/srv` and others are outside what the decision covers. |

Mode is always `0700`, the bases' `HOME_MODE`, per the decision. The group field is the primary group's name, which the group line guarantees exists by that name.

### 4.4 The `d` type

`d` re-applies owner and mode on every boot: the consult chowned a home to `root:root 0755` and the next run set it back. That is acceptable here. The invariant it enforces, a home owned by its user and closed to others, is the one an image should hold, and the escape is the standard one, a same-named file in `/etc/tmpfiles.d/`, which the comment names. The alternative, a `:` prefix on the mode and owner fields so they apply only at creation, needs a newer systemd than RHEL 9's 252 (*recalled*), so it cannot be written for both releases.

## 5. What the block stops writing

- `useradd -m` (failure 1).
- `RUN install -d -m 700 -o <user> -g <gid> '<home>/.ssh'`.
- `COPY users/home/<user>/.ssh/authorized_keys <home>/.ssh/authorized_keys`.
- `RUN chown ... '<home>/.ssh/authorized_keys' && chmod 600 ...`.
- `users::stage_ssh_keys` (`users.rs:571-605`) and both of its call sites: `render_all` step 6d (`crates/pipeline/src/render/mod.rs:139`) and `render_refine_export` (`crates/refine/src/session.rs:4504`). No `users/` tree is written on either export path.
- `"users"` in the refine export's top-level allowlist (`session.rs:4607`).
- The old group selection (`custom_groups` filtered by primary gid or supplementary name) in the Containerfile block. `custom_groups` stays for the kickstart and blueprint fragments.

Nothing the block writes lands under `/var` at build time: `useradd -M`, `groupadd` and the `/etc/group` copy write `/etc`, and the tmpfiles file is in `/usr/lib`. The consult measured `useradd -M` adding and modifying nothing under `/var` (a sorted `find /var -xdev` before and after).

## 6. The tmpfiles file

### 6.1 Written by a `RUN`, not staged

The file is written by the `printf` in § 4.1 step 5, at `/usr/lib/tmpfiles.d/inspectah-users.conf`. `/usr/lib/tmpfiles.d/` rather than `/etc/tmpfiles.d/`: `/usr` is image content replaced on every update, while `/etc` goes through the 3-way merge, where a host's copy would mask a later image's. `render::var_dirs` and the `/var` fragment already use `/usr/lib/tmpfiles.d/` for the same reason.

A staged file with a `COPY` was the main alternative. It reads slightly more like a hand-built project, but every `COPY` source has to be staged by two writers that nothing forces to agree, the CLI scan tarball and the refine download, and that gap has produced five build-breakers (`process-docs/skills/export-paths-stage-different-trees.md`). The `RUN` has no source to stage, so it cannot fail that way. A `RUN` heredoc was not taken: heredoc support in `podman build` arrived in a later podman than the documented 4.4 floor (*recalled*). Every value on a line is already proved (`ExportPath`, `AccountName`, a fixed mode), so none contains a `'`.

### 6.2 Coexistence with `inspectah-var.conf`

The `/var` fragment, `config/usr/lib/tmpfiles.d/inspectah-var.conf` (`render_tmpfiles_conf`, `containerfile.rs:2592`), covers `/var` directories the scan walked. The users file covers only homes under `/home`. On a package-mode host `/home` is not under `/var`, so the two files never name the same path, and no rule between them is needed. A home under `/var/` is the fragment's: it already resolves its owner by name when the user is materialized by useradd (`collect_materialized_usernames`, `containerfile.rs:2233`), and uses the host's mode, which is better than a fixed 0700 for a service-style home. The known gap, that the `/var` walk skips empty directories, is `inspectah-var-walk-skips-empty-directories`'s and is not widened here.

## 7. Refine and the exports

- **Refine.** No API or UI change. Users still default to Skip; only the useradd path renders differently. The expanded user card still lists preserved SSH keys: they are what `inspectah-users.ks` carries, and the Containerfile's keys comment points there.
- **CLI scan tarball and refine download.** Both lose the `users/` tree (§ 5). Both keep writing `inspectah-users.ks` and `inspectah-users.toml` unchanged. Neither needs a new staged file.
- **Fleet aggregate.** `merge_usersgroups_sections` keeps the first-seen user entry by name, so `primary_group` and `group_gids` come from that host. Fleet user handling is otherwise unchanged in this release.

## 8. Snapshot and API changes

**Snapshot.** Each `users_groups.users` entry gains `primary_group` (string, absent when the host has no group for the gid) and `group_gids` (object, name to gid). `SCHEMA_VERSION` and `MIN_SCHEMA` rise together to the next value, unless another beta.7 change has already raised them in this release, in which case this change rides that bump. A schema-28 snapshot is refused: it parses, but without `primary_group` every useradd user would render as a refusal, and only a re-scan can record the names. Semantic staleness, as in the `snapshot-schema-versioning` skill.

**Refine API.** Unchanged. `UserGroupDecision` (`crates/core/src/types/users.rs`) does not gain the fields; refine does not show them.

**Docs.** The implementation updates `docs/reference/known-limitations.md` (the two users-block build entries are removed; one entry states that a later image's user changes do not reach a host whose `/etc/passwd` changed locally, and that keys are provisioned, not built in), `docs/reference/output-artifacts.md` (the `users/` row is removed), and the `users/home/*/ssh/` row in `docs/diagrams/data-flow.html`.

**Changelog** (`## [Unreleased]`):

```markdown
### Changed
- **Breaking: snapshot schema is now <N>**: each user records its primary group's name and the gid of every group it belongs to, so the Containerfile can create a group the base image lacks. `refine` and `aggregate` load only schema <N>; re-scan each host and re-aggregate.

### Removed
- **SSH keys are no longer copied into the image**: the export has no `users/` directory, and the Containerfile names each user's key file on the host and says to provision the keys at install time with kickstart, cloud-init or a systemd credential. A key baked into the image would have been frozen at first install. `inspectah-users.ks` still carries kept keys as `sshkey` lines.

### Fixed
- **A user you set to useradd builds on a bootc base**: the Containerfile adds the user with `useradd -M`, and a `tmpfiles.d` line creates the home under `/var/home` at first boot. A group the base image lacks is created with the host's gid, and one the base keeps only in `/usr/lib/group` is copied into `/etc/group` before `useradd -G` uses it. Before, the build stopped at the `useradd` line.
```

## 9. Tests

Fixtures come from collector output: `UsersGroupsInspector` over a `MockExecutor` holding `/etc/passwd`, `/etc/shadow`, `/etc/group` and the `authorized_keys` files, as the 2026-10-31 lesson requires. No hand-written user `Value` stands in for a scan in U1 to U8. The § 4.2 host is the base fixture.

**What a unit test can prove:** the exact lines, their order, the refusals, that no `/home` or `/var` write and no `COPY users/` is emitted, and that both export paths stage nothing for the block. **What only a real build proves:** that `getent`, `grep` and `groupadd` behave as § 4.2 says against the real base, that `useradd -M` exits 0, that `bootc container lint` passes, and that the tmpfiles line resolves. The boot-time behaviour (ordering, SELinux label, a provisioned key logging in) is the release check's.

**C1. Collector** (`crates/collect/src/inspectors/users.rs` tests). `primary_group` and `group_gids` for primary gids 1001 (custom), 100, 65534 and 60001; `group_gids` covers every supplementary group, including system ones; no `primary_group` when `/etc/group` has no entry for the gid.

**U1. Primary group the base lacks** (`crates/pipeline/tests/users_render_test.rs`). `alice` alone renders § 4.2's first block exactly, an insta snapshot of the section and of the tmpfiles line.

**U2. All four users.** § 4.2's second block exactly, as a snapshot; group lines deduplicated in first-appearance order; `users` in its long form because `dave` names it in `-G`.

**U3. The `-G` rule.** A table: a group named only as primary gets the short form; named in any `-G` gets the long form; a name with a `.` is escaped in the `grep` pattern and nowhere else.

**U4. Nothing under `/home` or `/var` at build time.** Over U2's output: no `useradd -m`, every `useradd` has `-M`; no `install`, `mkdir`, `chown` or `COPY` in the section; the only `/var` string is inside the `printf` argument.

**U5. Homes.** `/home/staff/bob` gives `d /var/home/staff/bob ...`; `/var/lib/svc` gives no users line; `/opt/app` gives the "not created by this image" comment; `/` gives nothing; with no `/home` home at all, no `printf` and no home comment.

**U6. Keys comment.** Two users with keys and one without: the pointer lines once, one line per user with the right count and path; the `inspectah-users.ks` line only when `ssh_keys` is present; no keys comment when no user has keys.

**U7. Refusals.** A user whose gid has no host group: the FIXME, and no group line, `useradd`, `chpasswd` entry, tmpfiles line or key line for it, while the next user renders. The existing `hostile_user_fields_add_no_command` gains a hostile `primary_group` and a hostile `group_gids` key.

**U8. Excluded custom group.** A useradd user whose primary group's entry has `include: false` still gets its group line.

**E1. Both export paths** (`crates/cli/tests/copy_source_exists_test.rs`). The fixture gains a useradd user with preserved keys, so the guard is not vacuous for this block: no `users/` directory in either tree, no `COPY users/` in either Containerfile, and the existing invariant holds.

**B1. The build test** (the L5 done bar), `crates/pipeline/tests/users_build_test.rs`, `#[ignore]`d, run with `INSPECTAH_BUILD_TEST_BASE=<image>` on a host that has podman and the base. It renders U1's snapshot (one useradd user, a primary group the base lacks) into a Containerfile `FROM` that base with the users section and `RUN bootc container lint`, and asserts: `podman build` exits 0; the lint output has no error; in the built image `getent group appgrp` is `appgrp:x:1001:`, `id alice` lists `wheel`, `/etc/group` holds one `wheel` line, and `systemd-tmpfiles --create /usr/lib/tmpfiles.d/inspectah-users.conf` exits 0 and leaves `/var/home/alice` as `drwx------ alice appgrp`. A second case uses U2's four users. The release check runs B1 on `rhel-bootc` 9 and 10, besides its own end-to-end useradd build.

**S1. Schema.** A schema-28 snapshot is refused by `refine` and `aggregate`.

**Skills.** The implementation records the `/usr/lib/group` fact (groups in two files, `groupadd` sees both, `useradd -G` needs `/etc/group`) in a skill under `process-docs/skills/`, since the next renderer that touches accounts will need it, and adds it to the index.

## 10. Out of scope

- **Failure 2** of the users block item (`chpasswd` with several hashes).
- **`sysusers.d`.** Considered and not chosen. Upstream recommends it for the `/etc/passwd` drift above, and it would clear the `sysusers` lint warning, but it cannot set a password hash without credentials, will not force a uid or gid already taken, and reads less like a hand-written Containerfile. The decision accepts the warning knowingly.
- **Keys in the image through tmpfiles** (`f~ /home/<u>/.ssh/authorized_keys ... <base64>`, upstream's documented route). It bakes host keys into the image, which the decision chose not to do.
- **The SELinux label of `/var/home/<user>` on a booted host.** Policy gives `user_home_dir_t` (measured with `matchpathcon` in both bases); the label on a real boot is the release check's to measure.
- **The kickstart and blueprint fragments**, which have the same group gap and re-create excluded groups. Unchanged here.
- **Homes under `/var/` that the `/var` walk missed** (§ 6.2).
