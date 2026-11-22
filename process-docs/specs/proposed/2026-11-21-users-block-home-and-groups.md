# The Users Block Adds Accounts Without a Home in the Image

## Metadata

| Field    | Value |
|----------|-------|
| Date     | 2026-11-21 |
| Status   | Proposed |
| Release  | 0.9.0-beta.7 |
| Decision | The users block emits `useradd -M`, with no home in the image. A `tmpfiles.d` line, `d /var/home/<user> 0700 <user> <group> -`, creates the home with the right owner at first boot. `authorized_keys` and other home content are left to provisioning, with one comment line naming the key file the host had. A primary group the base lacks gets a `groupadd`. Accepted cost: an operator who relied on keys baked into the image must provision them, and the `sysusers` lint warning stays (Mark Russell, 2026-11-21). |
| Verified | Code citations checked against inspectah commit `27cc4531`. Base-image behaviour is from a measured consult on RHEL 9.8 and 10.2 `rhel-bootc` bases (`registry.redhat.io/rhel9/rhel-bootc:9.8` and `rhel10/rhel-bootc:10.2`, bootc 1.16.4), 2026-11-21, which ran `podman build`, `bootc container lint` and `systemd-tmpfiles --create` in each. No new host run for this spec; the gid-checking group lines in § 4.2 are not yet measured, and B1 proves them. Claims marked *recalled* were not checked. |
| Closes | Failures 1 and 3 of the users block build defect; the users block's unannounced `/var` write; the fleet merge dropping users' supplementary groups. |

## 1. Problem

`render_containerfile_users` (`crates/pipeline/src/render/users.rs:380-563`) fails `podman build` on a RHEL bootc base as soon as an operator sets a user to useradd in refine. Two of its failures are in scope:

1. **Home creation.** It emits `useradd -m` with the host's `/home/<user>`, then `install -d` of `<home>/.ssh`, a `COPY` of `users/home/<user>/.ssh/authorized_keys`, and a `chown`. On the base, `/home` is a symlink to `var/home` and `/var/home` does not exist, so `useradd -m` exits 12 and `install -d` exits 1. Measured on both releases. Even where it built, it would have written the key into the image's `/var`, which bootc unpacks only at first install, so a later key rotation would never reach a deployed host.
2. **Missing primary group.** `groupadd` is emitted only for groups the collector marks `custom` (gid 1000 to 59999) and that are included. A primary gid outside that range, or a custom group the operator excluded, leaves `useradd -g <gid>` naming a group that does not exist: exit 6.

The consult found two more facts that change the fix:

- **The base keeps most groups in `/usr/lib/group`.** The base `/etc/group` holds six groups (`root`, `adm`, `wheel`, `systemd-journal`, `tss`, `kvm`). The other 55 (9.8) or 61 (10.2) are in `/usr/lib/group`, read through nss-altfiles. `groupadd` checks through NSS, so it refuses a name or gid that exists only there (exit 9 or 4). Reading the base's `/etc/group` alone would get this wrong.
- **`useradd -G <g>` fails (exit 6) when `<g>` is only in `/usr/lib/group`**, because supplementary membership is written to `/etc/group` and the group is not there. Copying the entry first, `getent group <g> >> /etc/group`, fixes it (measured, and documented upstream in bootc's `bootc-users-and-groups.7.md`, "Modifying or adding members to system groups"). It must not be done for a group already in `/etc/group`: duplicate entries break the tools.

## 2. What this spec settles

1. Whether the pipeline can read the baseline image's groups cheaply, and the choice between a build-time guard and a scan-time read (§ 3).
2. One validated user plan that every users-derived line comes from (§ 4.1).
3. The exact rendered output (§ 4.2, § 4.4).
4. What happens when the image already uses a group's name or gid: collisions and numeric identity (§ 4.3).
5. Which homes get a rule, including homes under `/var` (§ 4.5).
6. What the block stops writing (§ 5).
7. Where the tmpfiles file comes from, and how it coexists with the `/var` provisioning fragment (§ 6).
8. Refine, both export paths, and the fleet merge fix (§ 7).
9. Snapshot and API changes, docs, and the changelog (§ 8).
10. Tests (§ 9).

Out of scope: § 10.

## 3. Reading the base's groups: build time, not scan time

### 3.1 How the baseline image is read today

`extract_baseline` (`crates/collect/src/baseline.rs:146-260`) runs at scan time: `podman image exists` or `podman pull`, `podman create --network none` of the image with `sleep infinity` as entrypoint, `podman start`, one `podman exec <ctr> rpm -qa --queryformat ...`, `podman rm -f`, then `podman inspect` for the digest. The result is `BaselineData { image_digest, packages, extracted_at }` (`crates/core/src/baseline.rs:89-93`), stored in `snapshot.baseline`. Only the package set is read. No file from the image is read today.

So a scan-time read **is** cheap: one more `podman exec <ctr> getent group` and one `podman exec <ctr> cat /etc/group` in a container that already exists. It would need a new `BaselineData` field and a schema bump.

### 3.2 Why the build-time guard wins anyway

The users block runs after the package section (`containerfile.rs:242`, section 8, after section 1's `dnf install`). A package the image installs can create the very group a user needs (a `docker` group, a service group), or take the gid a user's group had on the host, and a scan-time read of the bare base cannot see either. A build-time guard reads the image as it stands at that line, after every package, on the base the build actually uses, including one the operator changed in `FROM` after the scan. It needs no snapshot field for the base and no second code path for a snapshot without a baseline.

The cost is where a conflict surfaces. A scan-time read could refuse a user at render time with a `FIXME`, the way `useradd_refusal` refuses a bad name. Under the build-time guard, a conflict with the image is known only when the build reaches that group's line, so the refusal is a stopped build with a message naming the group and the gid (§ 4.3). A render-time refusal that misses package-created groups would still need the build-time stop behind it, so it would add a second path without removing the first.

Does it still read as hand-written? Close to it. `getent group <g> >/dev/null || groupadd -g <gid> <g>` is the idiom Fedora's packaging guidelines long used for creating a group in a scriptlet (*recalled*). The lines in § 4.2 are that idiom with the gid added to the match, because a name alone does not show that the image's group has the host's gid, plus a message when `groupadd` refuses. They are longer than a person would write for one machine, and one comment above the group lines explains them once. The scan-time read would give plainer lines (`RUN groupadd -g 1001 appgrp`, `RUN getent group users >> /etc/group`) but would be wrong for package-created groups, so plainness loses to correctness here.

### 3.3 What the snapshot must carry for the guard

The guard needs each group's name and the host's gid, to check the image's entry and to create it when neither the base nor a package did. Today the user entry has the primary `gid` but no group name, and `groups` holds only gids 1000 to 59999 (`parse_group`, `crates/collect/src/inspectors/users.rs:407-452`). A primary gid of 100, 65534 or above 60000 has no name anywhere in the snapshot, which is why the 2026-11-19 attempt, which inferred names from gaps, failed.

The collector already re-reads the whole host `/etc/group` for supplementary membership (`enrich_supplementary_groups`, `users.rs:654-700`). In the same pass it adds two fields to each user entry:

- `primary_group`: the name of the host group whose gid is the user's `gid`, from any gid range. Absent when the host's `/etc/group` has no entry for that gid.
- `group_gids`: an object mapping the name of the primary group and of each supplementary group to its host gid, for example `{"appgrp": 1001, "wheel": 10}`.

These are collector-produced. The renderer never infers a name from a gid range.

## 4. Rendered output

### 4.1 One validated user plan

Today three places decide independently which users exist: the group selection, the `useradd` loop with its refusals, and `collect_materialized_usernames` (`crates/pipeline/src/render/containerfile.rs:2235`), which checks only strategy and inclusion. A user the block refuses still counts as materialized there, so `inspectah-var.conf` can name an account the image never creates. This spec replaces the three with one plan, built once per render in `crates/pipeline/src/render/users.rs`:

```rust
/// The users the Containerfile creates, each proved once. Every line the
/// users block writes, and every account name `inspectah-var.conf` may use
/// for a user, comes from `accepted`.
pub(crate) struct ContainerfileUserPlan {
    accepted: Vec<PlannedUser>,
    /// In user order, each rendered where its `useradd` line would be.
    refused: Vec<RefusedUser>,
    /// Deduplicated, first appearance wins, accepted users only.
    groups: Vec<PlannedGroup>,
}

struct PlannedGroup {
    name: AccountName,
    gid: u32,
    /// Named in some accepted user's `-G`: the line also copies the
    /// entry into `/etc/group`.
    in_supplementary: bool,
}

enum HomeRule {
    /// `d <path> 0700 <user> <primary_group> -` in inspectah-users.conf.
    Tmpfiles(ExportPath),
    /// A `/var` home the export already creates (§ 4.5).
    VarFragment,
    /// The home is `/`.
    Exists,
    /// The "not created by this image" comment.
    NotCreated,
}
```

`PlannedUser` holds the proved fields of one `useradd` line (`AccountName` name, uid, `AccountName` primary group, `AccountName` supplementary groups, `ExportPath` shell and home), its `HomeRule`, its proved password hash when it has one, and its key count.

A user enters the plan when `containerfile_strategy == "useradd"` and `include != false`, and is accepted when it passes, in order:

1. `useradd_refusal`, unchanged. It is shared with the kickstart and blueprint renderers, which this spec leaves alone, so none of the checks below go into it and its wording does not change.
2. The plan's own checks, each with its own reason in `# FIXME: user <name> not created: <reason>`:
   - `primary_group` present; else "its primary group (gid <gid>) has no entry in the host's /etc/group".
   - `primary_group` and every `group_gids` key an `AccountName`; else "a group name is not valid".
   - every group the user names, primary and supplementary, has an entry in `group_gids`; else "a group it names has no recorded gid".
   - every group it names has the same gid as for any user accepted before it; else "group <g> has gid <a> here and <b> for an earlier user". Within one host this cannot happen, since every user's `group_gids` comes from the same `/etc/group`; in a fleet aggregate it can (§ 7).

Only an accepted user's groups enter `groups`. Group lines, `useradd` lines, `chpasswd` entries, home rules and the keys comment are all derived from `accepted`; nothing re-reads the snapshot's user entries after the plan is built. `collect_materialized_usernames` is removed. Its two callers, `render_tmpfiles_conf` and `var_dir_section_lines`, build the plan with `ContainerfileUserPlan::from_snapshot(snap)` and pass it to `resolve_tmpfiles_ownership`, which asks it two questions:

```rust
/// Whether the Containerfile creates the account `name`, so a
/// tmpfiles.d line may name it as an owner.
pub(crate) fn creates_user(&self, name: &str) -> bool;

/// The gid group `name` is guaranteed to have in the image: a group
/// line gives the image that name with that gid or stops the build.
/// `None` for any other group, which the image may lack, so a
/// tmpfiles.d line must not name it.
pub(crate) fn guaranteed_gid(&self, name: &str) -> Option<u32>;
```

`creates_user` is true for accepted users only. `guaranteed_gid` answers for the planned groups only, never for a base image group by its name. A directory's group is guaranteed when `guaranteed_gid` returns the host's gid for it; how each branch of `resolve_tmpfiles_ownership` uses that is in § 4.3. The plan is a pure function of the snapshot, so the users block and the two `/var` callers each building it get the same answers.

A refusal costs the whole user, as today: no group lines on its behalf, no `useradd`, no password, no home rule, no key line, and a `/var` directory it owns falls to the numeric ownership `resolve_tmpfiles_ownership` (`containerfile.rs:2344`) already uses for an account it cannot vouch for.

### 4.2 Rules

The block is one section, `# === Users and Groups ===`, rendered when the plan has at least one accepted or refused user, in this order:

1. **The update comment**, four lines, once:
   ```
   # Users and groups are written to /etc/passwd and /etc/group at build time.
   # A host that changed either file locally keeps its own copy on update, so
   # a user, group or membership added or changed in a later image does not
   # reach that host.
   ```
   This is the case the `sysusers` lint warns about at the end of the file. When such a host's own files lack the user or group a tmpfiles line names, that line is skipped with one journal line (`systemd-tmpfiles` exits 65, which the unit's `SuccessExitStatus=DATAERR CANTCREAT` accepts, so the host is not degraded); when they still have both, the line applies as usual. The comment says it where an operator edits users; the README does not repeat it.
2. **Group lines**, one per planned group, after a five-line comment:
   ```
   # Each group keeps the host's gid, so ownership the export writes by number
   # still matches. A base image entry with the same name and gid is kept, and
   # copied from /usr/lib/group into /etc/group when useradd -G needs it there.
   # A group whose name or gid the image already uses differently stops the
   # build: set its users to Skip in refine, or renumber it on the host and re-scan.
   ```
   - A group no accepted user names in `-G`:
     ```
     RUN getent group <g> | grep -q '^<g>:[^:]*:<gid>:' || groupadd -g <gid> <g> || \
         { echo 'group <g> (gid <gid>): the image already uses the name or the gid differently; see the comment above' >&2; exit 1; }
     ```
   - A group some accepted user names in `-G`:
     ```
     RUN grep -q '^<g>:[^:]*:<gid>:' /etc/group || getent group <g> | grep '^<g>:[^:]*:<gid>:' >> /etc/group || groupadd -g <gid> <g> || \
         { echo 'group <g> (gid <gid>): the image already uses the name or the gid differently; see the comment above' >&2; exit 1; }
     ```
     A pipeline binds tighter than `||`, so the copy is `getent ... | grep ... >> /etc/group` as one step, and it succeeds only when the image's entry has the host's gid.
   - In every pattern, each `.` in the name is written `\.`; it is the only regular-expression character `AccountName` admits. The name in the message is written as is.
   - `<gid>` is from `group_gids`. The group's `include` flag is not consulted: refine has no group exclusion action, and a user's line needs the group whatever its flag says. This is what fixes failure 3 for an excluded custom group.
   - The comment is emitted only when at least one group line is.
3. **`useradd` lines**, one per accepted user, with each refused user's `FIXME` in its place in user order:
   `RUN useradd -M -u <uid> -g <primary_group> [-G <g1>,<g2>] -s '<shell>' -d '<home>' <name>`
   `-M` is explicit: the bases set `CREATE_HOME yes`, so a plain `useradd` fails like `-m`. `-g` takes the group name; after the group lines, the name and the host's gid name the same group, and the name reads better. `-d` keeps the host's path, which for `/home/<user>` resolves through the `/home` symlink.
4. **`chpasswd`**, unchanged, for accepted users only.
5. **The home file**, when at least one accepted user has `HomeRule::Tmpfiles` (§ 4.5), after a three-line comment:
   ```
   # Home directories are created at boot by systemd-tmpfiles, not in the image.
   # tmpfiles resets their owner and mode on every boot; to change that on one
   # host, put a file named inspectah-users.conf in /etc/tmpfiles.d/.
   ```
   One user: `RUN printf '%s\n' '<line>' > /usr/lib/tmpfiles.d/inspectah-users.conf`. More than one: the same `printf` with one quoted line per user, each on its own continuation line (example in § 4.4). Then one `# <name>'s home, <home>, is not created by this image.` per user with `HomeRule::NotCreated`.
6. **The keys comment**, when at least one accepted user has `ssh_key_count > 0`:
   ```
   # SSH keys are not part of this image. Provision them at install time with
   # kickstart (sshkey --username=<user>), cloud-init, or a systemd credential.
   ```
   When at least one such user also has `ssh_keys` (the scan ran with key preservation), a third line follows: `# inspectah-users.ks has the sshkey lines for the keys this scan kept.` Then one line per user with keys:
   `# <name> had <n> key(s) in <home>/.ssh/authorized_keys on the host.` (`1 key` / `<n> keys`)

**Root.** Root never reaches this block: the collector records users with uids 1000 to 59999 only (`NON_SYSTEM_UID_MIN`/`MAX`, `users.rs:28-35`), and `collect_ssh_keys` reads keys only for those users. The keys comment therefore never points at `bootc install --root-ssh-authorized-keys`, which provisions root only, and the renderer has no root branch.

### 4.3 Group identity: collisions and numeric ownership

The export writes group ownership two ways: by name, or as the host's numeric gid. `systemd-tmpfiles` rejects a line whose group it cannot resolve, and the directory is then never made, so a tmpfiles.d line names a group only when the image is sure to have it. Every other group is written by number.

- **The users tmpfiles lines** name the user's primary group, which a group line always guarantees, since every accepted user's primary group is planned.
- **`resolve_tmpfiles_ownership`, owner the plan creates** (`creates_user`): the owner by name; the group by name when `guaranteed_gid` returns the host's gid for it, else the host's numeric gid. A directory `d /var/lib/svc` owned by `svc:homeowners` renders `svc homeowners` only when some accepted user is in `homeowners`, and `svc 45001` otherwise.
- **`resolve_tmpfiles_ownership`, owner a carried package creates** (`KNOWN_PACKAGE_ACCOUNTS`, or a replicated package whose name is the account's): the owner by name, with a `# created by package: <pkg>` comment. The package is the evidence for the account and for the group it creates with it, which has the account's name, so that group is written by name; both then take the ids the image's package gives them. Any other group is named only when `guaranteed_gid` returns the host's gid for it, else written as the host's gid. A directory owned by `apache:homeowners`, where `httpd` is carried and no accepted user is in `homeowners`, renders `apache 45001`. A directory whose group the scan recorded no name for is written by gid.
- **Any other owner**: the host's numeric uid and gid. A collected `/var/lib/shared` owned by `legacy:appgrp` (uid 500, gid 1001, mode 0770), where `legacy` is not created by this export, renders `d /var/lib/shared 0770 500 1001 -`.

A numeric gid is only as good as the group behind it: if the image's `appgrp` had gid 2001, `alice`'s membership in it would grant nothing on that directory, and nothing in the build would say so.

The policy is the one that needs no further decision: **every group the block touches ends up with the host's gid, or the build stops at that group's line.** The line succeeds only in these cases:

| Image at that line | Outcome |
|---|---|
| `<g>` with the host's gid, in `/etc/group` | kept, nothing written |
| `<g>` with the host's gid, only in `/usr/lib/group` | kept; copied into `/etc/group` when named in `-G` |
| neither the name nor the gid present | `groupadd -g <gid> <g>` |
| `<g>` with another gid, anywhere (base or package) | `groupadd` exits 9; the message; the build stops |
| `<g>` absent, the gid used by another name, anywhere | `groupadd` exits 4; the message; the build stops |

So a numeric gid anywhere in the export that came from the host still names the group it named there, for every group a rendered user belongs to, and no ownership reference needs rewriting. The stop is the build-time counterpart of a render-time refusal (§ 3.2): the message names the group and the gid, and the comment above the group lines says what to do.

Neither alternative is taken. Duplicate gids (`groupadd -o`) would make two distinct groups share permissions. Creating the group with whatever gid is free would change numeric ownership the export writes elsewhere, and reconciling every such reference is a larger design than this block; it is not clearly better, so it is not proposed here.

### 4.4 Examples

The host for these examples, as the collector would record it:

| user | uid | primary group (gid) | supplementary | keys |
|---|---|---|---|---|
| `alice` | 1001 | `appgrp` (1001), not in the base | `wheel` | 2, not preserved |
| `carol` | 1002 | `users` (100), in the base's `/usr/lib/group` | none | none |
| `dave` | 1003 | `dave` (1003) | `users`, only in the base's `/usr/lib/group` | none |
| `erin` | 1004 | `erin` (1004) | `wheel`, in the base's `/etc/group` | none |

All have shell `/bin/bash` and home `/home/<name>`. Every gid in the table is the base's gid for that name, or free in the base.

**A primary group the base lacks** (`alice` alone, the L5 case):

```
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

```

At build time: `getent group appgrp` finds nothing, so `grep` fails and `groupadd` creates it in `/etc/group`; `wheel:x:10:` is already in the base `/etc/group`, so the first `grep` succeeds and nothing is written; `useradd -M` exits 0 and writes nothing under `/var` (measured, with the mailbox message below).

`/usr/lib/tmpfiles.d/inspectah-users.conf` in the image:

```
d /var/home/alice 0700 alice appgrp -
```

**All four users:**

```
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
RUN grep -q '^users:[^:]*:100:' /etc/group || getent group users | grep '^users:[^:]*:100:' >> /etc/group || groupadd -g 100 users || \
    { echo 'group users (gid 100): the image already uses the name or the gid differently; see the comment above' >&2; exit 1; }
RUN getent group dave | grep -q '^dave:[^:]*:1003:' || groupadd -g 1003 dave || \
    { echo 'group dave (gid 1003): the image already uses the name or the gid differently; see the comment above' >&2; exit 1; }
RUN getent group erin | grep -q '^erin:[^:]*:1004:' || groupadd -g 1004 erin || \
    { echo 'group erin (gid 1004): the image already uses the name or the gid differently; see the comment above' >&2; exit 1; }
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

- **Primary group the base has** (`carol`, `users`): `users` is also named in `dave`'s `-G`, so it gets the long form, and the copy writes `users:x:100:` from `/usr/lib/group` into `/etc/group`. Had no user named it in `-G`, it would get the short form, which finds `users` with gid 100 and writes nothing; `useradd -g users` works against the `/usr/lib/group` entry (measured with `-g 100`).
- **Primary group the base lacks** (`appgrp`, `dave`, `erin`): created with the host's gid.
- **Supplementary group only in `/usr/lib/group`** (`users` for `dave`): copied into `/etc/group` before `useradd -G users` (measured: exit 6 without the copy, 0 with it).
- **Supplementary group in `/etc/group`** (`wheel` for `alice` and `erin`): the first `grep` matches and nothing is written, so no duplicate entry.
- **A supplementary group the image does not have at all**, such as a package-created group whose package the image does not install: `groupadd` creates it with the host's gid, so the membership is kept and the build goes on, unless the gid is taken (§ 4.3).
- **Keys**: only `alice` has keys. With key preservation on, the extra `inspectah-users.ks` line appears after the two pointer lines.

**Expected build-log noise.** `useradd` prints `Creating mailbox file: No such file or directory` on these bases (no `/var/spool/mail`) and still exits 0. Measured; not a failure, and the export does not try to silence it.

### 4.5 Which homes get a rule

| host home | `HomeRule` | why |
|---|---|---|
| `/home/<rest>` | `Tmpfiles`: `d /var/home/<rest> 0700 <user> <primary_group> -` | `/home` is a symlink to `var/home` on the base; the real path is written, matching the passwd entry's resolved home. A nested `<rest>` gets its missing parents from tmpfiles as root 0755 (*recalled*); `/var/home` itself comes from the base's `rpm-ostree-0-integration.conf` (measured). |
| `/var/home/<rest>` | `Tmpfiles`: `d /var/home/<rest> 0700 <user> <primary_group> -` | The same directory, recorded by its real path, as on a host that was already image-based. |
| any other path under `/var/` | `VarFragment` when `storage.var_directories` has an entry for exactly that path whose effective backing (`effective_var_backing`, `crates/pipeline/src/render/var_dirs.rs:95`) is `Carried` or `Provision`; otherwise `NotCreated` | The export then has a creator or an active rule for it, and the fragment names the owner because the plan creates the user (§ 4.1). `Unresolved` gets only a commented line from the fragment, so it is not evidence. |
| `/` | `Exists` | It exists. |
| anything else | `NotCreated` | Under `/usr` it would be image content; `/opt`, `/srv` and others are outside what the decision covers. |

The other `/var` case is needed because the `/var` walk is narrow: `discover_var_directories` (`crates/collect/src/inspectors/storage.rs:165-205`) runs `find` on `/var/lib`, `/var/log` and `/var/cache` only, to depth 2, skipping empty directories and keeping only the deepest of nested ones. A home at `/var/opt/app` or `/var/lib/a/b/c` is never in `var_directories`, however full it is, and neither is `/var/lib/svc` when it has a non-empty subdirectory; each gets the comment.

Mode is always `0700`, the bases' `HOME_MODE`, per the decision. The group field is the primary group's name, which the group line guarantees exists by that name with the host's gid.

### 4.6 The `d` type

`d` re-applies owner and mode on every boot: the consult chowned a home to `root:root 0755` and the next run set it back. That is acceptable here. The invariant it enforces, a home owned by its user and closed to others, is the one an image should hold, and the escape is the standard one, a same-named file in `/etc/tmpfiles.d/`, which the comment names. The alternative, a `:` prefix on the mode and owner fields so they apply only at creation, needs a newer systemd than RHEL 9's 252 (*recalled*), so it cannot be written for both releases.

## 5. What the block stops writing

- `useradd -m` (failure 1).
- `RUN install -d -m 700 -o <user> -g <gid> '<home>/.ssh'`.
- `COPY users/home/<user>/.ssh/authorized_keys <home>/.ssh/authorized_keys`.
- `RUN chown ... '<home>/.ssh/authorized_keys' && chmod 600 ...`.
- `users::stage_ssh_keys` (`users.rs:571-605`) and both of its call sites: `render_all` step 6d (`crates/pipeline/src/render/mod.rs:139`) and `render_refine_export` (`crates/refine/src/session.rs:4504`). No `users/` tree is written on either export path.
- `"users"` in the refine export's top-level allowlist (`session.rs:4607`).
- The old group selection (`custom_groups` filtered by primary gid or supplementary name) in the Containerfile block. `custom_groups` stays for the kickstart and blueprint fragments.
- `collect_materialized_usernames`' own filter (§ 4.1).

Nothing the block writes lands under `/var` at build time: `useradd -M`, `groupadd` and the `/etc/group` copy write `/etc`, and the tmpfiles file is in `/usr/lib`. The consult measured `useradd -M` adding and modifying nothing under `/var` (a sorted `find /var -xdev` before and after).

## 6. The tmpfiles file

### 6.1 Written by a `RUN`, not staged

The file is written by the `printf` in § 4.2 step 5, at `/usr/lib/tmpfiles.d/inspectah-users.conf`. `/usr/lib/tmpfiles.d/` rather than `/etc/tmpfiles.d/`: `/usr` is image content replaced on every update, while `/etc` goes through the 3-way merge, where a host's copy would mask a later image's. `render::var_dirs` and the `/var` fragment already use `/usr/lib/tmpfiles.d/` for the same reason.

A staged file with a `COPY` was the main alternative. It reads slightly more like a hand-built project, but every `COPY` source has to be staged by two writers that nothing forces to agree, the CLI scan tarball and the refine download, and that gap has produced five build-breakers (`process-docs/skills/export-paths-stage-different-trees.md`). The `RUN` has no source to stage, so it cannot fail that way. A `RUN` heredoc was not taken: heredoc support in `podman build` arrived in a later podman than the documented 4.4 floor (*recalled*). Every value on a line is already proved (`ExportPath`, `AccountName`, a fixed mode), so none contains a `'`.

### 6.2 Coexistence with `inspectah-var.conf`

The `/var` fragment, `config/usr/lib/tmpfiles.d/inspectah-var.conf` (`render_tmpfiles_conf`, `containerfile.rs:2592`), covers `/var` directories the scan walked. The users file covers homes under `/home` and `/var/home`. The walk never enters `/var/home` (§ 4.5), so the two files never name the same path, and no rule between them is needed. A home elsewhere under `/var/` is the fragment's only when the fragment has it (§ 4.5); it then resolves the owner by name because the plan materializes the user, and uses the host's mode, which is better than a fixed 0700 for a service-style home. Every other home gets exactly one of: a users line, the fragment's rule, or the "not created" comment.

## 7. Refine, the exports, and the fleet merge

- **Refine.** No API or UI change. Users still default to Skip; only the useradd path renders differently. The expanded user card still lists preserved SSH keys: they are what `inspectah-users.ks` carries, and the Containerfile's keys comment points there.
- **CLI scan tarball and refine download.** Both lose the `users/` tree (§ 5). Both keep writing `inspectah-users.ks` and `inspectah-users.toml` unchanged. Neither needs a new staged file.
- **Fleet aggregate.** `merge_usersgroups_sections` (`crates/core/src/aggregate/merge.rs:2243`) keeps the first-seen user entry by name and unions the array fields it lists, `groups` and `secondary_groups`. Neither is a user field: the collector writes `supplementary_groups`. So an aggregated user's supplementary groups today come from the first host only. The fix lists `supplementary_groups` in place of both names, and merges `group_gids` as an object: the union of the keys, keeping the first-seen gid for a name, as the merge already keeps the first-seen `uid` and `gid`. Every supplementary group after the merge therefore has a recorded gid. A user whose group has another gid on another host is not reconciled here; when two aggregated users disagree on a group's gid, the plan refuses the later one (§ 4.1).

## 8. Snapshot and API changes

**Snapshot.** Each `users_groups.users` entry gains `primary_group` (string, absent when the host has no group for the gid) and `group_gids` (object, name to gid). `SCHEMA_VERSION` and `MIN_SCHEMA` rise together to the next value, unless another beta.7 change has already raised them in this release, in which case this change rides that bump. A schema-28 snapshot is refused: it parses, but without `primary_group` every useradd user would render as a refusal, and only a re-scan can record the names. Semantic staleness, as in the `snapshot-schema-versioning` skill.

**Refine API.** Unchanged. `UserGroupDecision` (`crates/core/src/types/users.rs`) does not gain the fields; refine does not show them.

**Docs.** The implementation updates:

- `docs/reference/known-limitations.md`: the two users-block build entries are removed; one entry states that a later image's user and group changes do not reach a host whose `/etc/passwd` or `/etc/group` changed locally, that a home rule on such a host is skipped only when the user or group it names is absent from the host's files, and that keys are provisioned, not built in.
- `docs/reference/output-artifacts.md`: all three `users/` mentions, the scan-tarball table row (line 69), the refine-export table row (line 265), and `users/` in the example's list of absent conditional directories (line 329).
- `docs/diagrams/data-flow.html`: the `users/home/*/ssh/` output entry.
- `process-docs/skills/archive-extraction-confinement.md`: the `users/home/{name}/.ssh` example of a snapshot name joined onto an export path, which no longer exists.

**Changelog** (`## [Unreleased]`):

```markdown
### Changed
- **Breaking: snapshot schema is now <N>**: each user records its primary group's name and the gid of every group it belongs to, so the Containerfile can create a group the base image lacks. `refine` and `aggregate` load only schema <N>; re-scan each host and re-aggregate.

### Removed
- **Breaking: provision users' SSH keys at install time**: the image no longer carries them, and the export has no `users/` directory. The Containerfile names each user's key file on the host, and `inspectah-users.ks` still has kept keys as `sshkey` lines.

### Fixed
- **A user you set to useradd builds on a bootc base**: the Containerfile adds it with `useradd -M`, a `tmpfiles.d` line creates its home at first boot, and each group gets the host's gid. A group name or gid the image already uses differently stops the build with a message.
- **`aggregate` keeps every host's supplementary groups for a user**: they came from the first host only.
```

## 9. Tests

Fixtures come from collector output: `UsersGroupsInspector`, and `StorageInspector` where a case needs `/var` directories, over one `MockExecutor` holding `/etc/passwd`, `/etc/shadow`, `/etc/group`, the `authorized_keys` files, and the `find` and `stat` output of the `/var` walk, as the 2026-10-31 lesson requires. No hand-written user or `/var` directory `Value` stands in for a scan in U1 to U10 or F1. The § 4.4 host is the base fixture.

**What a unit test can prove:** the exact lines, their order, the refusals, that no `/home` or `/var` write and no `COPY users/` is emitted, which file owns each home, and that both export paths stage nothing for the block. **What only a real build proves:** that `getent`, `grep` and `groupadd` behave as § 4.3 says against the real base and against installed packages, that `useradd -M` exits 0, that `bootc container lint` passes, and that the tmpfiles lines resolve. The boot-time behaviour (ordering, SELinux label, a provisioned key logging in) is the release check's.

**C1. Collector** (`crates/collect/src/inspectors/users.rs` tests). `primary_group` and `group_gids` for primary gids 1001 (custom), 100, 65534 and 60001; `group_gids` covers every supplementary group, including system ones; no `primary_group` when `/etc/group` has no entry for the gid.

**U1. Primary group the base lacks** (`crates/pipeline/tests/users_render_test.rs`). `alice` alone renders § 4.4's first block exactly, an insta snapshot of the section and of the tmpfiles line.

**U2. All four users.** § 4.4's second block exactly, as a snapshot; group lines deduplicated in first-appearance order; `users` in its long form because `dave` names it in `-G`.

**U3. The group line forms.** A table: a group named only as primary gets the short form; named in any `-G` gets the long form; every form carries the host's gid in its pattern, its `groupadd` and its message; a name with a `.` is escaped in every `grep` pattern and nowhere else.

**U4. Nothing under `/home` or `/var` at build time.** Over U2's output: no `useradd -m`, every `useradd` has `-M`; no `install`, `mkdir`, `chown` or `COPY` in the section; the only `/var` string is inside the `printf` argument.

**U5. Homes, across the whole export.** One collector-produced snapshot rendered through `render_all`; the test reads the Containerfile and `config/usr/lib/tmpfiles.d/inspectah-var.conf` and asserts, per home, exactly one of a users line, a fragment rule, or the "not created" comment:
- `/home/staff/bob`: users line `d /var/home/staff/bob 0700 bob bob -`.
- `/var/home/vic`: users line `d /var/home/vic 0700 vic vic -`.
- `/var/lib/svc`, which the walk finds (non-empty; `stat` gives `0750 1006 1006 svc svc`): no users line, one `d /var/lib/svc 0750 svc svc -` in `inspectah-var.conf`, no comment.
- `/var/opt/app`, which the walk does not cover: the "not created" comment and no rule in either file.
- `/opt/app`: the comment. `/`: nothing.
- With no `/home` or `/var/home` home at all: no `printf` and no home comment.

**U6. Keys comment.** Two users with keys and one without: the pointer lines once, one line per user with the right count and path; the `inspectah-users.ks` line only when `ssh_keys` is present; no keys comment when no user has keys.

**U7. Refusals and the plan.** A user `zed` whose gid has no host group, owning a walked `/var/lib/zed` (`stat` gives `0700 1009 1009 zed UNKNOWN`): the FIXME, and no group line, `useradd`, `chpasswd` entry, tmpfiles line or key line for it, while the next user renders; in `inspectah-var.conf`, `/var/lib/zed` is owned `1009 1009` with the "not guaranteed in image" comment, and `zed` appears in no owner field anywhere in the export. The existing `hostile_user_fields_add_no_command` gains a hostile `primary_group` and a hostile `group_gids` key. The kickstart and blueprint output for the same snapshot is byte-identical to before this change (`useradd_refusal` unchanged).

**U8. Excluded custom group.** A useradd user whose primary group's entry has `include: false` still gets its group line.

**U9. Numeric ownership.** `alice` in `appgrp` (1001), and a walked `/var/lib/shared` with `stat` `0770 500 1001 legacy appgrp`, where `legacy` is not a collected user: `inspectah-var.conf` has `d /var/lib/shared 0770 500 1001 -`, and the users block's `appgrp` line requires gid 1001 in its pattern and passes `-g 1001` to `groupadd`.

**U10. Conflicting gids across users.** Two users whose `group_gids` give `devs` different gids (as a fleet aggregate can): the first renders, the second gets the "group devs has gid ..." FIXME.

**F1. Fleet merge** (`crates/core/tests/aggregate_merge_test.rs`). Two collector-produced host sections: `alice` in `wheel` on one host and in `docker` on the other. The merged `alice` has `supplementary_groups` `["docker", "wheel"]` and `group_gids` with both. The existing `test_merge_usersgroups_sections_dedup_by_name`, which passes only because its hand-written users carry a `groups` field the collector never writes, is rewritten on collector output.

**E1. Both export paths** (`crates/cli/tests/copy_source_exists_test.rs`). The fixture gains a useradd user with preserved keys, so the guard is not vacuous for this block: no `users/` directory in either tree, no `COPY users/` in either Containerfile, and the existing invariant holds. The `users` entry in the test's list of top-level paths that are not `COPY` sources (line 180) is removed, since nothing stages it.

**Existing tests that expect the removed artifacts** are rewritten in the same change:
- `crates/refine/tests/users_integration_test.rs:152-155` (the staged key file in the tarball) and `:176-183` (`COPY users/...` and `install -d -m 700`): assert that neither appears, and that the Containerfile has `useradd -M`. The checks that `inspectah-users.ks` and `inspectah-users.toml` are in the tarball stay.
- `crates/refine/tests/export_contract_test.rs:696`: asserts that no path starts with `users/`.
- `crates/pipeline/tests/users_render_test.rs:144-171` and `crates/pipeline/tests/smoke_render_2b.rs:379`: the `useradd -m`, `install -d`, `COPY users/` and `chown` expectations become the new forms.
- The kickstart `sshkey` and blueprint `key` assertions (`users.rs` tests at lines 676 and 746, and their `users_render_test.rs` counterparts) are kept unchanged: those fragments still carry the keys.

**B1. The build test** (the L5 done bar), `crates/pipeline/tests/users_build_test.rs`, registered as `mod users_build_test;` in `crates/pipeline/tests/main.rs` (the crate sets `autotests = false`, so an unregistered file never compiles). Each case is `#[ignore]`d and run with `INSPECTAH_BUILD_TEST_BASE=<image>` on a host that has podman and the base. A case renders a collector-produced snapshot into a Containerfile `FROM` that base with the users section (and, where named, the `/var` section and an earlier `RUN`) and `RUN bootc container lint`. Where a case needs a base gid, the harness reads it from the base first (`podman run --rm <base> getent group <name>`) rather than hard-coding it.

- **Positive, one user** (U1's snapshot): `podman build` exits 0; the lint output has no error; in the built image `getent group appgrp` is `appgrp:x:1001:`, `id alice` lists `wheel`, `/etc/group` holds one `wheel` line, and `systemd-tmpfiles --create /usr/lib/tmpfiles.d/inspectah-users.conf` exits 0 and leaves `/var/home/alice` as `drwx------ alice appgrp`.
- **Positive, four users**: U2's snapshot, the same checks for each user.
- **Numeric ownership** (U9's snapshot, with the `/var` section): the build exits 0, and after `systemd-tmpfiles --create` on both files, `stat -c %g /var/lib/shared` equals the gid of `getent group appgrp`, 1001.
- **Missing name, occupied gid**, each expecting a failed build whose log has the group line's message: a host group `virt` with the gid of the base's `kvm` (in `/etc/group`); a host group `staff` with gid 100, `users`' gid in `/usr/lib/group`; and a host group `web` with the gid of `apache` after an earlier `RUN dnf -y install httpd`, a group a package creates.
- **Same name, different gid**, each expecting the same failure: host `kvm` with the base's gid plus 1000; host `users` with gid 1100; host `apache` with its package gid plus 1000, after the same `dnf install`.

The two package cases need the base's repositories reachable from the build host, and fail rather than skip when `dnf` cannot reach them. The release check runs B1 on `rhel-bootc` 9 and 10 on a subscribed host, besides its own end-to-end useradd build.

**S1. Schema.** A schema-28 snapshot is refused by the core loader (`InspectionSnapshot::load`, `crates/core/src/snapshot.rs:116`), by `refine` and by `aggregate`.

**Skills.** The implementation records the `/usr/lib/group` fact (groups in two files, `groupadd` sees both, `useradd -G` needs `/etc/group`) and the group-identity rule of § 4.3 in a skill under `process-docs/skills/`, since the next renderer that touches accounts will need both, and adds it to the index.

## 10. Out of scope

- **Failure 2** of the users block item (`chpasswd` with several hashes).
- **`sysusers.d`.** Considered and not chosen. Upstream recommends it for the `/etc/passwd` drift above, and it would clear the `sysusers` lint warning, but it cannot set a password hash without credentials, will not force a uid or gid already taken, and reads less like a hand-written Containerfile. The decision accepts the warning knowingly.
- **Keys in the image through tmpfiles** (`f~ /home/<u>/.ssh/authorized_keys ... <base64>`, upstream's documented route). It bakes host keys into the image, which the decision chose not to do.
- **The SELinux label of `/var/home/<user>` on a booted host.** Policy gives `user_home_dir_t` (measured with `matchpathcon` in both bases); the label on a real boot is the release check's to measure.
- **The kickstart and blueprint fragments**, which have the same group gap and re-create excluded groups. Unchanged here.
- **A uid the image already uses.** `useradd -u` stops the build with its own "UID not unique" message, as today; this spec adds no message for it.
- **Widening the `/var` walk** to directories outside `/var/lib`, `/var/log` and `/var/cache`, or to empty ones (`inspectah-var-walk-skips-empty-directories`). Homes it misses get the "not created" comment (§ 4.5).
- **Reconciling a group's gid across fleet hosts.** The merge keeps the first-seen gid, and the plan refuses a later user that disagrees (§ 7); a fleet-wide policy waits for the fleet redesign.
