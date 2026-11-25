# Carried Files Keep Their Owner, Group and Mode

## Metadata

| Field | Value |
|---|---|
| Date | 2026-11-25 |
| Status | Proposed. Goes first in 0.9.1-beta.1, ahead of the aggregate convergence spec, because it is more fundamental (Mark Russell, 2026-11-25). § 6 is the field list the convergence spec takes as input. Every choice not backed by a recorded decision is listed in § 13 for Mark. |
| Release | 0.9.1-beta.1. The schema and the `/etc` part land before convergence code. |
| Scope | Owner, group and mode of every carried `/etc` configuration file, carried unmanaged file and directory, and `/var` directory the export provisions: what the scan records, the snapshot types, one ownership resolver, account creation, the Containerfile lines, both export paths, notices, the fleet field list, tests. |
| Decisions it implements | Ownership is preserved; one resolver for `/etc` and unmanaged content; provisioning-time accounts are handled and the operator is told which accounts the image requires (Mark Russell, 2026-11-23). `/etc` first; mode is not fixed alone (Mark Russell, 2026-11-23). A carried tmpfiles.d line that names an account the image lacks gets the account created (Mark Russell, 2026-11-25). Classification lives in scan through one core defaults module; refine applies only user edits; an unedited export equals the tarball (Mark Russell, 2026-11-24). Variant identity stays byte-identical content (Mark Russell, 2026-11-24). |
| Backlog items | Closes `inspectah-carried-files-lose-owner-and-group`, `inspectah-tmpfiles-line-owned-by-a-skipped-user-is-silently-skipped` and `inspectah-var-parent-dirs-lose-their-ownership` (inspectah-team `workflow/backlog/inspectah/open/`). Leaves `inspectah-storage-ownership-rows-report-var-directories-the-image-already-reproduces` open (§ 5.6). |
| Inputs | A security consult on carried ownership, 2026-11-25, read-only at `4b520879`. Its eight rules are § 2.1. A bootc consult, 2026-11-25, read against ostree `3d72f1d`, bootc `b88f28b` and buildah `4bf2012`. Its answers are § 12 and its rules § 2.2. |
| Verified | Code citations read at `59a068fa` (2026-11-25). Line numbers are a convenience; the symbol name is the identifier. Nothing was built or booted for this spec. Claims marked *inferred* or *recalled* were not checked; § 10.2 names the build or booted host that checks them. |

**No code, documentation, or CHANGELOG entry was written to produce this file.**

## 1. Problem

Every carried file and directory lands in the image owned by `root:root`, and every carried `/etc` file lands `0644`. A host file that was `0600` or `0640` is world-readable in the image, and a file a service reads as its own account is unreadable to it once the mode is right. The evidence, by layer:

**Scan.**

- `ConfigFileEntry` (`crates/core/src/types/config.rs:73-96`) has no owner, group or mode field. The config collector calls `file_metadata` only for a withheld entry (`ConfigCollector::withhold`, `crates/collect/src/inspectors/config/collector.rs:196-206`), and then keeps only `mode` and `size`.
- `FileMetadata` (`crates/core/src/traits/executor.rs:33-40`) holds `mode`, `size`, `dev`, `ino` and `nlink`. No uid or gid, though `RealExecutor::file_metadata` (`crates/collect/src/executor/real.rs:440`) already has the `std::fs::Metadata` that carries both.
- Unmanaged files record numeric `uid`, `gid` and `%a` permissions in `ProvenanceSignals` (`crates/core/src/types/nonrpm.rs:92-125`), from `get_file_metadata` (`crates/collect/src/inspectors/nonrpm.rs:2107-2124`). No names. A failed `stat` returns uid 0, gid 0 and an empty mode, so a file the scan could not stat reads as root-owned with no signal.
- `walk_for_unmanaged` (`nonrpm.rs:2224`) records files and symlinks only. Directory owner and mode are not in the snapshot.
- `/var` directories record mode, ids and names (`VarDirectory`, `crates/core/src/types/storage.rs:329-343`; `stat -c '%04a %u %g %U %G'` in `discover_var_directories`, `crates/collect/src/inspectors/storage.rs:182-260`). The dedup drops every parent of a discovered leaf, so `/var/lib/pgsql 0700 postgres:postgres` is gone before render.
- The users inspector records accounts with uid 1000 to 59999 only (`NON_SYSTEM_UID_MIN`, `NON_SYSTEM_UID_MAX`, `crates/collect/src/inspectors/users.rs:27-36`). A system account that owns a carried file is not in the snapshot by name.

**Render and export.**

- No `chown` or `--chown` appears under `crates/pipeline/src/render`. The only ownership-like line is the SSSD `chmod 0600` (`sssd_mode_lines`, `crates/pipeline/src/render/containerfile.rs:1792`).
- `render/tarball.rs:99-108` packs directories `0755`, unmanaged payloads with `PAYLOAD_MODE_BITS` (`0o777`, so setuid, setgid and sticky are dropped), and everything else `0644`. `build/extract.rs:494-498` keeps a member's mode only for unmanaged payloads.
- The config tree is copied as whole directories (`COPY config/<root>/ /<root>/`, `containerfile.rs:1727`). Whether a directory `COPY` onto an existing image directory such as `/etc/sudoers.d` (`0750` on the base) resets its mode to the member's `0755` is not checked; § 10.2 R3 checks it.
- The Containerfile's order is packages, config (`:205-209`), unit state (`:211-216`), non-RPM (`:218-223`), containers, users (`:238-244`). Any `chown` by name must follow the users block, and an account pinned against package allocation must precede the packages (§ 5.3).

**Interim state.** beta.8 names the loss: `render/unmanaged_owners.rs` lists every carried unmanaged file whose host owner or group was not root or that had a special bit, and the scan prints the count. `render/tmpfiles_accounts.rs` names every carried tmpfiles.d line whose account the image will not have. Both are notices; this spec replaces them with the fix.

**Why mode cannot go first.** Carrying `0640` while ownership stays `root:root` does not leak, but it breaks: `/etc/named.conf 0640 root:named` becomes unreadable to `named`, which then fails to start. Today's `0644` leaks but works. Mode alone trades a confidentiality bug for an availability bug on exactly the files that were configured with care. Mode, owner, group and the fallback land together (rule 6).

## 2. Rules this spec carries

### 2.1 From the security consult

Adopted as requirements:

1. **Never widen a carried mode.** The image mode is the host mode or narrower. No path in the renderer defaults a carried file to `0644`. An unknown mode fails closed.
2. **Record owner and group names at scan time,** not only numbers. The snapshot cannot map a system uid back to a name later.
3. **`chown` by name in a `RUN` after the users block,** not `COPY --chown`. The build fails when a name does not resolve.
4. **An account the image lacks is created at the host's id,** before any `chown` that names it.
5. **setuid and setgid carry only when owner and group also carry;** never on a `root:root` fallback; never on a group- or world-writable file.
6. **Mode, owner, group and the fallback land together.**
7. **A redacted file keeps the host's mode.**
8. **A fleet build never ships a mode wider than any contributing host** for the same content without a flag.

### 2.2 From the bootc consult

Adopted as requirements; the evidence is § 12.

9. **Ownership in the image must be right the first time.** An owner, group or mode change on a deployed host makes that `/etc` file locally modified, and from then on no later image change to it arrives. The export says so wherever it reports ownership (§ 5.8).
10. **Pin every account the image creates for carried content at the host's id:** created before any package install that could allocate it, mirrored in a `sysusers.d` line with the same id, and the build stops if the id is taken (§ 5.3).
11. **`/var` ownership and mode reach the host as tmpfiles.d `d` lines by name, never as image content** (§ 5.6). Image content under `/var` is applied once, at install.
12. **Carry no SELinux labels** (§ 11).

Rule 3 holds with a sharper reason: `COPY --chown=<name>` reads only the image's `/etc/passwd` and `/etc/group`, so it fails the build for any account the RHEL bootc bases keep in `/usr/lib/group`.

Trust assumptions, stated once: the host's owner and mode are taken as intended, not audited, so a host misconfiguration ships (except as § 5.4 clamps it); names resolve in the build image, so the image's account database is trusted for name to id; `stat` runs as root at scan time, and a file changed afterwards is not re-checked.

## 3. Snapshot types

### 3.1 One ownership type

A new module, `crates/core/src/types/ownership.rs`, holds the one type every carried path uses.

```rust
/// Owner, group and permission bits of a path, as the host had them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Ownership {
    /// `stat` answered.
    Recorded {
        owner: AccountRef,
        group: AccountRef,
        mode: Mode,
    },
    /// `stat` failed. The export falls back (§ 4.4); nothing reads this
    /// as root-owned.
    Unknown { reason: String },
}

/// A user or group a host file names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AccountRef {
    /// The host resolved the id to a name that `chown`, `useradd` and
    /// `groupadd` accept.
    Named { name: AccountName, id: u32 },
    /// The host has no name for the id (an orphan), or its name is not
    /// one the export may write into a build instruction. `host_name`
    /// keeps the second kind for the notice.
    Unnamed { id: u32, host_name: Option<String> },
}

/// Permission and special bits, `0o0000..=0o7777`. Serialized as the
/// four-digit octal string `stat %04a` prints ("0640", "4755").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Mode(u16);
```

`Mode` refuses a value above `0o7777` at construction and at load, so a mode with file-type bits cannot be written down. It records the special bits; whether they reach the image is the resolver's call (§ 4.5), not the snapshot's.

`AccountName` moves from `crates/pipeline/src/render/safety.rs:499` to core, with its parse rules unchanged, and gains `serde(try_from = "String")`. The collector decides at scan time whether a host name is `Named` or `Unnamed { host_name: Some(..) }`. An SSSD name such as `jdoe@ad.example.com` that the token set refuses is classified there, once, not refused by the renderer later.

Unknown is an enum variant, not a sentinel. Today's `(0, 0, "")` for a failed `stat` becomes `Unknown`, and no type left in the snapshot can say "root" when it means "not known".

### 3.2 Where it lives

| Record | Field | Replaces |
|---|---|---|
| `ConfigFileEntry` | `ownership: Ownership` | nothing (new) |
| `UnmanagedFile` | `ownership: Ownership` | `provenance.uid`, `provenance.gid`, `provenance.permissions` |
| `ConfigSection` | `directories: Vec<CarriedDirectory>` | nothing (new) |
| `NonRpmSection` (unmanaged files) | `directories: Vec<CarriedDirectory>` | nothing (new) |
| `VarDirectory` | `ownership: Ownership` | `mode`, `owner_name`, `group_name`, `owner_uid`, `owner_gid` |
| `UserGroupSection` | `owner_accounts: Vec<HostAccount>`, `owner_groups: Vec<HostGroup>` | nothing (new) |

```rust
/// A directory above a carried path whose host owner, group or mode is
/// not `0755 root:root`. Carried when anything beneath it is carried;
/// it has no disposition of its own and no refine control.
pub struct CarriedDirectory {
    pub path: String,
    pub ownership: Ownership,
}

/// The host's entry for an account some carried path names, from any
/// uid range: what the export needs to create it (§ 4.3).
pub struct HostAccount {
    pub name: AccountName,
    pub uid: u32,
    pub primary_group: AccountName,
    pub gid: u32,
    pub home: String,
    pub shell: String,
}

pub struct HostGroup {
    pub name: AccountName,
    pub gid: u32,
}
```

A directory is recorded only when it departs from `0755 root:root`. An unrecorded directory is therefore `0755 root:root` on the host, which is what the tarball already writes, so no record is needed to reproduce it. `WithheldConfigEntry` keeps its `mode` and gains nothing: withheld members never ship, whatever their mode.

`SCHEMA_VERSION` (`crates/core/src/snapshot.rs:21`) goes from 31 to 32. inspectah has no compatibility promise before 1.0: the change gets a `**Breaking:**` changelog entry saying a snapshot from an earlier version must be re-scanned, and nothing else.

## 4. Scan

### 4.1 Metadata

`FileMetadata` gains `uid` and `gid`. `RealExecutor::file_metadata` fills them from the `Metadata` it already has; the mock executor and the machine-state gate pass them through. This is one more pair of integers from a `stat` the process already makes.

- **Config.** `ConfigCollector::admit` and `admit_advisory` record `ownership` for every entry from `file_metadata(c.read)`. `c.read` is the path the content came from, so a `%ghost` file under a directory link gets the metadata of the file the image will hold. A failed `file_metadata` is `Ownership::Unknown` with the error.
- **Config ancestors.** For each distinct directory between `/etc` and an admitted file, the collector stats the directory once and records a `CarriedDirectory` when it departs from `0755 root:root`.
- **Unmanaged.** `get_file_metadata`'s `stat -c '%s %Y %u %g %a'` call stays for size and mtime. Owner, group and mode come from the executor's metadata for regular files and from `lstat` for symlinks (whose mode is not carried; § 4.5). The walk records a `CarriedDirectory` for each directory it enters that departs from `0755 root:root`.
- **`/var`.** `discover_var_directories` keeps its `stat` but maps it into `Ownership`; `UNKNOWN` from `%U` or `%G` becomes `Unnamed`. The dedup changes as § 5.6 says.

### 4.2 Names

A new `HostAccounts` index, built once per scan in the collect context and shared by every inspector, maps ids to names:

- It reads `getent passwd` and `getent group` once, through NSS, so a name SSSD or LDAP serves is found where enumeration lists it.
- An id that enumeration did not list (SSSD with `enumerate = false`, the RHEL default, *recalled*) is looked up once with `getent passwd <id>` or `getent group <id>` and cached.
- An id with no answer is `AccountRef::Unnamed { id, host_name: None }`. A name `AccountName` refuses is `Unnamed { id, host_name: Some(name) }`.

After every inspector has run, a pass in `finalize_cross_inspector` collects each `Named` owner and group of every carried record and writes `owner_accounts` and `owner_groups` from the same index. Accounts in the users inspector's range are included too: the resolver needs one table, and the users section's `serde_json::Value` entries do not hold `home` and `shell` in a typed form.

### 4.3 Classification: the defaults the scan writes

Per Mark's rule, the scan decides defaults and refine applies only the operator's edits. Two ownership defaults go into the core defaults module, beside the machine-state table:

1. **A local account the export needs.** A user in the users inspector's range, on the default `skip` strategy and included, that owns or is the group of a carried path, or that a carried tmpfiles.d line names, is set to `useradd` by the scan, with the reason on the row: "carried files need this account: N files, M tmpfiles lines". This is decision 6 (create the account) applied through the users block that already creates local accounts at their host ids, with home handling from the users block spec. Refine shows the reason and lets the operator set the user back to `skip`, which is the provisioning case (§ 8). Decision 13.4.
2. **World-writable regular files under `/etc`.** If Mark picks the clamp (decision 13.2), the scan records the clamp as the default on the entry's ownership outcome with its reason; refine displays it. No other mode change is a default.

Neither default is recomputed in refine. A user edit that changes what the resolver can prove (excluding a user, excluding a package) changes the render's result, which is a consequence of the edit, not a reclassification.

### 4.4 ACLs and file capabilities

Not carried (§ 11). Detected cheaply so the operator is told: one `getfacl -R --skip-base --absolute-names` and one `getcap -r` per carried root (`/etc`, `/opt`, `/srv`, `/usr/local`), parsed into a list of paths on the section. Each is one command per root, not per file. If either tool is absent on the host, the scan records a degraded reason and lists nothing. Decision 13.7.

## 5. Rendering

### 5.1 One resolver

A new `crates/pipeline/src/render/ownership.rs` builds one `OwnershipPlan` per render, from the snapshot and the users block's `ContainerfileUserPlan` (`render/users.rs`). Every ownership line, notice and report row comes from it; nothing else reads `Ownership` to decide output.

```rust
/// How one carried path's owner and group reach the image.
enum Placement {
    /// `root:root` on the host: the staged mode is all it needs.
    Root,
    /// `chown <owner>:<group>`, resolved by name in the build.
    ByName { owner: AccountName, group: AccountName },
    /// The account is provisioned at install (decision 13.5): numeric
    /// `chown`, guarded, listed as required.
    ByNumber { uid: u32, gid: u32, account: AccountName },
    /// Owner or group could not be carried: `root` for that half, mode
    /// narrowed (§ 5.4), listed in the notice.
    Fallback { owner: Option<AccountName>, group: Option<AccountName>, cause: FallbackCause },
}

enum FallbackCause {
    StatFailed,
    OrphanId(u32),
    InvalidName(String),
    /// The operator excluded the user or group the path needs.
    AccountExcluded(AccountName),
}

struct PlannedPath {
    path: ExportPath,
    placement: Placement,
    /// What the image gets. Never wider than the host's mode.
    mode: Mode,
    /// Special bits the `RUN chmod` restores after `chown` (§ 5.5).
    special: SpecialBits,
}
```

Resolution order for each named owner and group:

1. **The users block creates it** (`ContainerfileUserPlan` accepted it): by name.
2. **A local user on `skip` that the operator kept there:** provisioned at install by the export's own kickstart or blueprint, which already write the host `--uid`/`uid =` (`render/users.rs:191-200`, `:308-317`). By number, with a guard (§ 5.3). Decision 13.5.
3. **A local user the operator excluded:** fallback, `AccountExcluded`.
4. **Any other name** (a system account): the accounts block creates it at the host's id before packages unless the base image already has it (§ 5.3), then by name. A carried package that would create it needs no special case: its scriptlet finds the account present and leaves it, so the id is the host's, not whatever the build would allocate. An account the base already holds (`/usr/lib/passwd`, `/usr/lib/group`) is the base vendor's to pin; the guard finds it and does nothing.
5. **`Unnamed`, or `Unknown` ownership:** fallback.

### 5.2 Mode: staged, not commanded

A file's permission bits (`mode & 0o777`) reach the image through the staged tree, as unmanaged payloads already do: `COPY` gives the image file the build-context file's mode. This covers every carried `/etc` file, including the RPM-owned modified ones (`sshd_config` at `0600` stays `0600`), and it adds no Containerfile lines for the common root-owned case.

- **The tarball** (`render/tarball.rs`) writes each staged file's member with the resolver's mode, owner uid 0. Members stay owned by uid 0: a root operator running plain `tar x` would otherwise chown files to host ids that mean other accounts on the workstation (*inferred*).
- **Both export paths stage the tree** (skill `export-paths-stage-different-trees`): the CLI tarball and the refine download each set the mode on the staged file, and `build/extract.rs` keeps every member's mode, not only unmanaged payloads', masked with `0o777`. The guard fixture in that skill gains a `0600` config file.
- **A mode with no owner read bit** (`0000`, `0200`) cannot be read by a rootless `podman build` from the operator's tree. The staged copy adds `u+r`, and the resolver emits an explicit `chmod` to the host mode after the `COPY`. *Rare; named so it is not a surprise.*
- **Directories** stay `0755` in the tarball. A recorded `CarriedDirectory` gets an explicit `chown` and `chmod` in the ownership block, which also covers an existing base directory a `COPY` may have reset (§ 10.2 R3).
- **Redacted files** are written with the host's mode (rule 7). Redaction changes content, never metadata.

### 5.3 The accounts block and the ownership block

Two new blocks. The accounts block runs before packages so no package scriptlet allocates an id first; the ownership block runs after the users block, so every account any block creates exists before a `chown` names it.

**Accounts block.** Immediately before the packages block. For each system account that a `ByName` placement or a carried tmpfiles.d line names and the users block does not create, from `owner_groups` and `owner_accounts`, groups first:

```dockerfile
# Accounts carried files need, pinned at the source host's ids so a
# rebuild cannot renumber them. The build stops if the image already
# uses an id for another account.
COPY config/usr/lib/sysusers.d/inspectah-accounts.conf /usr/lib/sysusers.d/inspectah-accounts.conf
RUN getent group appsvc >/dev/null || groupadd -r -g 985 appsvc \
 && getent passwd appsvc >/dev/null \
    || useradd -r -M -u 985 -g appsvc -d /var/lib/appsvc -s /sbin/nologin appsvc
```

- **Before packages.** A carried package that creates the account (`bind` and `named`) finds it present and leaves it, so the image holds the host's id rather than whatever the build allocates. An id allocated by the build can change on a later rebuild, and nothing on bootc remaps owners when it does (§ 12 Q2).
- **The guard.** `getent` goes through NSS and sees `/usr/lib/passwd` and `/usr/lib/group`, so an account the base already holds is left alone and `chown` by name resolves to the base's id. That account is the base vendor's to pin.
- **A taken id stops the build.** `groupadd -g` and `useradd -u` refuse an id another account holds (exit 4). The users block spec's message wrapper around a refused `groupadd` is reused, so the stop names the account and the id. Decision 13.6.
- **Home and shell** are the host's, validated as `ExportPath`; an invalid one falls back to `/` and `/sbin/nologin`.
- **`sysusers.d` mirror.** `config/usr/lib/sysusers.d/inspectah-accounts.conf` declares every account this block creates, and every local user the scan defaulted to `useradd` for carried content (§ 4.3), with the same ids:

  ```
  g appsvc 985
  u appsvc 985:985 - /var/lib/appsvc /sbin/nologin
  ```

  This keeps `bootc container lint`'s `sysusers` warning quiet for these accounts, and puts an account back at boot on a host whose locally modified `/etc/passwd` lacks it, which a later image adding the account would otherwise never reach. Every field is a proved token. `useradd` in the build, not `systemd-sysusers`, creates the account: `systemd-sysusers` silently takes another id when the requested one is used, which would break rule 10. Decision 13.8.

A **`ByNumber`** account gets a guard in the ownership block instead of a creation:

```dockerfile
RUN ! getent passwd 1001 >/dev/null || [ "$(getent passwd 1001 | cut -d: -f1)" = appuser ] \
 || { echo "uid 1001 (appuser, created at install) is taken in the image" >&2; exit 1; }
```

**Ownership block.** Section 8b, right after the users block (`containerfile.rs:238-244`). One `RUN`, `chown` lines grouped by owner and group, then directory `chmod` lines, then special-bit `chmod` lines:

```dockerfile
# Owner, group and special bits from the source host. Fix ownership
# here and rebuild: a chown on a deployed host freezes that /etc file
# against every later image.
RUN chown root:named /etc/named.conf /etc/rndc.key \
 && chown appsvc:appsvc /etc/appsvc/ /etc/appsvc/secret.conf /opt/appsvc/ \
 && chown 1001:1001 /opt/tools/report.sh \
 && chmod 0750 /etc/appsvc/ /etc/sudoers.d/ \
 && chmod 4750 /opt/appsvc/bin/helper
```

`chown` fails the build when a name does not resolve, which is rule 3's failure with no extra code: the message is `chown`'s own and names the account. No `chown` line exists for a `Root` placement. Paths are `ExportPath`s; a path the token set refuses gets a `# FIXME:` line and a fallback row, as other blocks do.

Every carried path the ownership block names is image content: `/etc` (whose image copy becomes the `/usr/etc` default the merge compares against), and `/opt` and `/usr/local`, which are real image directories on both RHEL bootc bases (`BASE_IMAGE_DIRECTORIES`, `render/selinux.rs`). Unmanaged findings under `/srv`, a link into `/var` on both bases, are routed to the `/var` strategy and never copied (`included_var_srv_paths`, `render/unmanaged.rs`), so the resolver plans nothing for them.

### 5.4 Fallbacks

A path whose owner or group cannot be carried keeps the half that can and falls back to `root` for the other:

| Cause | Image owner and group | Image mode |
|---|---|---|
| `stat` failed | `root:root` | `0600` for a file, `0700` for a directory |
| Owner is an orphan id, refused name, or excluded account | `root` for the owner | host bits, special bits cleared |
| Group is an orphan id, refused name, or excluded account | `root` for the group | host bits with the group bits cleared, special bits cleared |

Owner bits handed to `root` grant nothing root does not have. Group bits handed to `root` are cleared, because the host granted them to a group that is not in the image. Other bits are kept: they were granted to everyone on the host, so keeping them does not widen. Every fallback is a row in the notice (§ 5.8) with its cause.

World-writable regular files: under `/etc`, decision 13.2 (recommended: clamp `o+w`, with a row). Elsewhere they carry, with a row.

### 5.5 Special bits

`chown` by root clears setuid and setgid on Linux (`chown(2)`, *recalled*), so special bits are restored by the `chmod` after the `chown`, never through the staged mode. The staged mode stays `& 0o777` (`PAYLOAD_MODE_BITS` is unchanged), and a special bit reaches the image only on a line the resolver wrote after carrying ownership.

- **setuid** on a regular file carries only when the owner is carried (`ByName` or `ByNumber`), the file is not group- or world-writable, and decision 13.1 allows it.
- **setgid** on a regular file: the same, with the group.
- **Sticky** on a regular file has no effect on Linux and is dropped.
- **setgid and sticky on a directory** carry when the directory's group (for setgid) is carried. They narrow behaviour; dropping sticky from a shared writable directory is the risky direction.
- Every carried setuid or setgid file is listed in the audit report.

### 5.6 `/var` directories

**Parents.** The dedup in `discover_var_directories` (`storage.rs:225-227`) changes from "drop a proper prefix of another path" to "drop a proper prefix of another path whose ownership is `0755 root:root`". A parent that departs is kept as its own `VarDirectory`, gets its own `d` line in `inspectah-var.conf` before its leaf's (the fragment is sorted by path), and so `/var/lib/pgsql 0700 postgres:postgres` survives. This is the cheaper of the two fixes the backlog item names, it uses the same `Ownership` type, and it is a tmpfiles line, not a `chown`. Folded in.

**Lines, not content.** `VarDirectory.ownership` reaches the host only through `d` lines in `/usr/lib/tmpfiles.d/inspectah-var.conf`, which inspectah already writes for provisioned directories. bootc unpacks image content under `/var` only at install and never on upgrade, so ownership carried as content would reach a host once and never be corrected; a `d` line is applied every boot. The one place the renderer still writes `/var` content goes: a provisioned directory with no recorded mode gets `RUN mkdir -p` today (`mkdir_line`, `containerfile.rs:2761`). Under this spec its ownership is `Unknown`, and it gets a `d` line with § 5.4's `stat`-failed fallback, `0700 root root`, and a fallback row. Lines are `d` only: `Z` would recursively chown application data, which a migration tool has no business doing.

**Accounts, by name.** The owner and group a `d` line names go through the same resolver, and every placement writes names. tmpfiles resolves a name at boot to the host's current id, so a `d` line by name also survives a renumbered account. `ByName` accounts exist in the image (§ 5.3) and `ByNumber` accounts exist by first boot because provisioning created them, so both are named. `resolve_tmpfiles_ownership`'s numeric cases (a name available but not guaranteed, or no name) go: the resolver either guarantees the account or falls back, and a fallback writes `root` for that half with the bits § 5.4 gives.

**Not closed here.** The Storage view's rows for carried directories whose ownership departs from a fixed `0755 root:root` are a presentation question waiting on the Storage section's purpose. Keeping departing parents adds rows of the same kind. That item stays open.

### 5.7 tmpfiles.d lines

Decision 6: an account a carried tmpfiles.d line names and the image lacks is created. Under this spec:

- A local account: the scan defaults its user to `useradd` (§ 4.3), so the users block creates it.
- A system account: the line's owner and group enter the account set of § 5.3 like any carried path's.
- An account the operator excluded, or kept on `skip`: excluded keeps today's notice; `skip` is provisioned, so the line resolves at boot and is not named.

`render/tmpfiles_accounts.rs` keeps only the excluded case. The finding that a line whose owner is on `skip` still counts as `/var` backing, so `inspectah-var.conf`'s numeric fallback is dropped while systemd skips the line, no longer bites: the line's account now exists in the image or at install.

### 5.8 Notices

`render/unmanaged_owners.rs` and its scan stderr line are replaced by one ownership notice built from the plan, in the audit report, the HTML report, refine's export warnings (`x-export-warnings`, which today does not carry the beta.8 notice) and `inspectah scan`'s stderr count:

- paths that fell back, with the host owner, group and mode and the cause;
- accounts the image creates (§ 5.3), pinned at the host's ids, and accounts provisioning must create, with uid and gid (`ByNumber`);
- carried setuid and setgid files;
- world-writable files, clamped or carried;
- paths with ACLs or file capabilities that are not carried (§ 4.4).

Each surface that lists ownership also says, once, how it behaves after deployment: the image's ownership of an `/etc` file is the default every later image updates; a host where someone changes that file's owner, group or mode keeps its own copy, content included, and takes no later image change to it, with no warning. So a wrong owner or a fallback is fixed in the Containerfile and rebuilt before deployment, not with `chown` on hosts. This is why fallbacks are listed before the first build rather than left to the operator to find.

The Containerfile carries one comment line with the counts, pointing at the audit report. Refine shows a carried row's host ownership as read-only detail; the user strategy row shows the reason § 4.3 wrote.

## 6. Field list for the convergence spec

The aggregate merge must consider exactly these new or changed fields:

1. `ConfigFileEntry.ownership`
2. `UnmanagedFile.ownership` (and the removal of `provenance.uid`, `provenance.gid`, `provenance.permissions`)
3. `ConfigSection.directories` and the unmanaged section's `directories`, keyed by path, carried when anything beneath them is
4. `VarDirectory.ownership`
5. `UserGroupSection.owner_accounts` and `owner_groups`, keyed by name
6. The users default reason the scan writes (§ 4.3), and the `useradd` strategy it sets
7. The ACL and capability path lists (§ 4.4): union, notice only
8. If decision 13.2 is the clamp: the clamp default on the entry

Questions the convergence spec answers with these fields:

- **Decision for Mark (13.3):** is ownership part of the variant key, or is it resolved separately for the winning variant? Mark already decided variant identity is byte-identical content (2026-11-24), so the recommendation is: content-only key, and ownership resolved across the hosts that hold the winner. Owner and group: the most prevalent pair among them. Mode: the bitwise AND of those hosts' modes, so it is never wider than any of them. When the hosts disagree on owner or group, or when a host outside the winner's pair had a narrower mode, refine flags the row and the audit report lists it (rule 8). The alternative, ownership in the key, shows mode-only differences as variants and still needs the rule-8 flag, because the most prevalent variant can be the wider one.
- **An account with different ids on different hosts** (`chrony` 992 here, 994 there, normal for dynamically allocated system ids): `chown` and the `d` lines by name are unaffected. Creation and the `sysusers.d` mirror (§ 5.3) pin one id: recommended, the most prevalent, with the others listed. The pin matters more than it did: hosts at another id keep their `/var` data at their own number, and nothing on bootc remaps it, so the list is what tells the operator which hosts need a one-time `chown` of their data.
- **Unknown ownership on some hosts:** the hosts with `Recorded` decide; a path `Unknown` on every host falls back.
- **Users:** Mark's leaning keeps users out of convergence. The scan's `useradd` default (§ 4.3) is per host; the merge needs a rule for a user defaulted on some hosts and not others. Recommended: `useradd` when any host's carried files need it.

## 7. What refine and the exports do

- Refine applies no ownership classification. It shows ownership as row detail, shows the § 4.3 reason, and offers the users block's existing strategy control. A strategy or include edit changes the next render's plan, which is a consequence of the edit.
- An unedited export equals the tarball: both build the plan from the same snapshot through the same resolver, and both staging paths set the same modes. The parity test (§ 10.1 T12) holds this.

## 8. Accounts created at provisioning

The security consult named this its biggest size risk. It is in scope, at modest cost, because the export already writes the provisioning side: a local user on `skip` is emitted to the kickstart and blueprint with the host's uid and gid. The design:

- The scan defaults such a user to `useradd` when carried files need it (§ 4.3), so the common case never reaches provisioning.
- An operator who sets it back to `skip` gets `ByNumber` placement: numeric `chown`, a build-time guard that the id is free or already that name (§ 5.3), and a "provisioning must create" row with the uid and gid.
- An account created at install by something the export does not write (a hand-written kickstart, a BIB blueprint with no `uid`) is outside what the export can know. Numeric ownership is right only if that provisioning uses the host's ids, and the required-accounts list says so. Whether BIB's user customization honours a fixed uid and gid is *recalled* as yes, not checked; § 10.2 R11 checks it.

The alternative, falling back to `root:root` with a notice for a kept `skip`, is smaller by one guard line and one list, and breaks every such service. Decision 13.5.

## 9. Changelog and docs

- `**Breaking:**` schema 32; re-scan earlier snapshots.
- Fixed: carried `/etc` files keep their mode, owner and group; carried unmanaged files and directories keep owner, group and special bits; `/var` parent directories keep their ownership; tmpfiles.d lines whose account the image lacks get the account.
- Changed: the beta.8 ownership notice becomes the fallback notice. System accounts that own carried files are created at the source host's ids before packages and declared in `/usr/lib/sysusers.d/inspectah-accounts.conf`. `/var` directory lines name accounts, never numbers, and a `/var` directory with no recorded mode gets a tmpfiles.d line instead of `RUN mkdir`.
- Skills: `export-paths-stage-different-trees` (every staged file's mode now carried, both paths), `bootc-base-groups-and-users` (system account creation before packages, the `sysusers.d` mirror, `COPY --chown` failing on `/usr/lib/group` accounts), `cross-inspector-shadow-passes` is unaffected. A new `carried-file-ownership` skill holds the resolver order, the `chown`-clears-setuid ordering, the fallback table, and the 3-way merge rule that a metadata change on a host freezes the file.
- User docs: the known-limitations entries beta.8 added are removed.

## 10. Testing

### 10.1 Contract tests

Table-driven, over collector-produced snapshots from the mock executor unless stated.

| # | Case | Expected |
|---|---|---|
| T1 | `/etc/named.conf 0640 root:named`, `bind` carried | staged `0640`; `chown root:named`; no account line |
| T2 | `/etc/appsvc/secret.conf 0600 appsvc:appsvc`, `appsvc` uid 985, no package creates it | account lines at 985 before the `chown`; `/etc/appsvc/` recorded and chowned if it departs |
| T3 | Modified RPM config `sshd_config 0600 root:root` | staged and tar member `0600`; no `chown` line |
| T4 | `stat` fails | `Ownership::Unknown`; fallback `root:root 0600`; notice row; never `0644` |
| T5 | Orphan uid 4242 owner, group `appgrp` resolves | `root:appgrp`, special bits cleared, notice row |
| T6 | `/opt/app/bin/helper 4750 appsvc:appgrp` | `chmod 4750` after the `chown`; setuid listed |
| T7 | `4777` file, and a setuid file whose owner falls back | special bits stripped; rows |
| T8 | `/etc/foo 0666` | per decision 13.2 |
| T9 | Redacted `/etc/app.conf 0640 root:app` | staged `0640` |
| T10 | `/var/lib/pgsql 0700 postgres` with leaf `data` | two `d` lines, parent first |
| T11 | tmpfiles line naming `appuser` (uid 1001, on `skip`) | scan defaults `appuser` to `useradd` with the reason; no tmpfiles notice |
| T12 | Parity: the tarball's Containerfile and staged modes equal an untouched refine export's, single host | equal |
| T13 | Snapshot load refuses `Mode` `"10644"` and an `AccountName` with a space | load error |
| T14 | SSSD name `jdoe@ad.example.com` owning a file | `Unnamed { host_name: Some(..) }`, fallback |
| T15 | User set back to `skip` | `ByNumber`, guard line, required-accounts row |
| T16 | `extract.rs` keeps a `0600` config member's mode; the refine download does too | both `0600` |
| T17 | T2's `appsvc`, created by a carried `bind`-style package too | accounts block before the packages block; `sysusers.d` lines `g appsvc 985` and `u appsvc 985:985`; ids equal the `useradd` line's |
| T18 | `/var/lib/app 0750 appsvc:appsvc` and a `/var` directory with `Unknown` ownership | `d /var/lib/app 0750 appsvc appsvc -`, by name; `d <path> 0700 root root -` and a fallback row; no `RUN mkdir` |
| T19 | Audit report, HTML report, export warnings | each carries the after-deployment line (§ 5.8) once |

### 10.2 Real-host checks

Two kinds of host. **Builds:** scan a driftify-prepared rhel9 and rhel10 VM with fixtures for each case, build the generated Containerfile with `podman build` on each release's `rhel-bootc` base, and check ownership with `stat` inside the built image. **Booted:** R13 to R15 run on a booted rhel9 and rhel10 bootc host and settle what the bootc consult read from source or inferred.

| # | Checks |
|---|---|
| R1 | T1: `named` reads its config in the image (`stat` and `runuser -u named -- cat`) |
| R2 | T2: account created at 985; `getent passwd appsvc` |
| R3 | `COPY config/etc/ /etc/` onto the base's `/etc/sudoers.d` (`0750`): does the directory keep `0750` without the `chmod`? Settles whether § 5.2's directory `chmod` is needed for unchanged base directories |
| R4 | T3: `sshd_config` stays `0600` |
| R5 | T6: setuid survives; confirms `chown` clears it when the order is reversed |
| R6 | T4 and T5 fallbacks |
| R7 | T11: `systemd-tmpfiles --create` in the built image exits 0 and the directory is owned by `appuser` |
| R8 | T10 and T18: `/var/lib/pgsql` parent and leaf after `systemd-tmpfiles --create`; then `chown root:root` the parent and re-run: the `d` line restores the owner and mode of an existing directory (*recalled*, Q6) |
| R9 | A local build with `COPY --chown=root:cdrom` on each base (`cdrom` is only in `/usr/lib/group`) fails with "looking up UID/GID", while `RUN chown root:cdrom` succeeds (Q3) |
| R10 | An account id the image already uses: the build stops with the message |
| R11 | T15 with the export's blueprint through BIB on one release: the installed system's account has the host uid |
| R12 | `bootc container lint` on T17's image: no `sysusers` warning for `appsvc`, and no `/var` tmpfiles warning from inspectah's lines |
| R13 | Booted: `chown root:wheel /etc/chrony.conf; ostree admin config-diff \| grep chrony` shows `M chrony.conf` (Q1) |
| R14 | Booted: `grep -c . /etc/passwd /usr/lib/passwd`; the `passwd` and `group` lines in `/etc/nsswitch.conf`; `getent passwd chrony` (Q3: users split like groups, and NSS answers for `/usr/lib` accounts) |
| R15 | Booted: `ls -ld /opt /usr/local /srv /root /home` (Q5: re-confirms the layout `BASE_IMAGE_DIRECTORIES` and `BASE_VAR_SYMLINKS` record) |

## 11. Non-goals

Each left out with its cost:

- **ACLs.** A `setfacl` grant to a service account is lost and the file falls back to its mode bits. Usually narrower, so it breaks rather than leaks, except a default ACL on a directory. Noticed (§ 4.4), not carried.
- **Extended attributes (`user.*`).** No security effect. Not detected.
- **SELinux labels.** Not carried, by rule. ostree's merge ignores extended attributes, labels included, so a label-only change on a host neither persists across an update nor marks the file modified, and image labels come from the image's policy. A label set with `chcon` is lost; `semanage fcontext` rules belong to the SELinux inspector. Low.
- **File capabilities (`security.capability`).** A binary under `/opt` with `cap_net_bind_service` loses it and fails, or an operator re-adds setuid as a workaround. Breakage, not a leak. Noticed (§ 4.4), carried later.
- **Kickstart and blueprint user selection.** Unchanged; the users block spec owns it.
- **Remapping data at a host's old id.** When the base image holds an account at a different id from the source host, or a fleet pins one id across hosts that had several (§ 6), files under `/var` keep the host's number. Nothing on bootc remaps owners. The `d` lines fix the directories themselves at boot; the data inside is listed, not changed.
- **Auditing host modes.** A host misconfiguration ships, except the `/etc` world-writable clamp if decision 13.2 takes it.

## 12. bootc questions

Answered by the bootc consult, read against ostree `3d72f1d`, bootc `b88f28b` and buildah `4bf2012` (2026-11-25). Each answer says whether it was read in source or inferred, and which § 10.2 check confirms it on a host.

- **Q1. Does an owner, group or mode change alone make an `/etc` file locally modified?** Yes, read in source, high confidence. ostree's merge (`merge_configuration_from`, `src/libostree/ostree-sysroot-deploy.c`) diffs the old image default in `/usr/etc` against the running `/etc` with a checksum whose header holds uid, gid and mode; the composefs backend's merge (`crates/etc-merge`) compares the same three. It is not a true 3-way merge: the modified file wins whole, so the new image's content and metadata for that path are dropped. A host that never touches a file follows every later image, ownership fixes included; a host `chown` freezes it with no warning. A rebuilt image that `chown`s by name also heals unmodified files after a renumbering (*inferred*). Drives rule 9 and § 5.8. R13 confirms.
- **Q2. Does renumbering leave files at the old number?** Yes: neither bootc nor ostree remaps owners, and `/var` is never touched on upgrade (read in source, high confidence). An account in `/etc/passwd`, which is almost always locally modified, keeps its old number on the host while new image content carries the new one; an account in `/usr/lib/passwd` moves with the image and strands `/var` data and modified `/etc` files; a `sysusers.d` account with no fixed id can differ per host. The fix, rule 10 and § 5.3, follows bootc's documented pinning pattern (medium confidence that it is the complete fix).
- **Q3. Where do accounts live on the RHEL bootc bases, and how do the build instructions resolve them?** Groups: six in `/etc/group`, the rest in `/usr/lib/group` through nss-altfiles, measured on 9.8 and 10.2 by the users block spec. Users: the same split is *inferred*; R14 measures it. `RUN chown <name>` resolves through NSS and so finds altfiles accounts (*inferred*, consistent with the measured `groupadd` behaviour; R9 and R14). `COPY --chown=<name>` does not: buildah's lookup reads only the image's `/etc/passwd` and `/etc/group`, and the step fails the build with "looking up UID/GID" (read in source, high). On `sysusers.d` at build: `systemd-sysusers` silently takes another id when the requested one is used, so it cannot enforce rule 10; `useradd` creates and a `sysusers.d` line mirrors (§ 5.3).
- **Q4. Does `/var` content in the image reach deployed hosts?** Only at install, like a `VOLUME`; upgrades never apply it (read in bootc's `bootc-filesystem.7.md`, high). `bootc container lint` warns on `/var` content with no tmpfiles.d entry. Drives rule 11 and § 5.6. R8 and R12 check the lines.
- **Q5. Are SELinux labels part of the merge, and where do `/opt` and `/usr/local` live?** ostree's merge ignores labels on purpose (read in source, high), so none are carried (rule 12, § 11). `/opt` and `/usr/local` are real image directories and `/srv`, `/home`, `/root` and `/mnt` are links into `/var` on both RHEL bootc bases: inspectah's own measurement, recorded in `BASE_IMAGE_DIRECTORIES` and `BASE_VAR_SYMLINKS` (`render/selinux.rs`). So setuid files under `/opt` and `/usr/local` are image content, follow every image, and sit in the composefs metadata. R15 re-confirms.

Still open:

- **Q6.** Does a tmpfiles.d `d` line correct the owner and mode of a directory that already exists? *Recalled* as yes, not read today; § 5.6 relies on it to correct a `/var` directory on every boot. R8 checks it.

## 13. Decisions for Mark

1. **setuid and setgid on files:** carry by default when owner and group carry, listed in the audit report, or list and require an include? Recommend: carry by default. An include is a new refine control and adds size.
2. **World-writable files under `/etc`:** clamp `o+w` with a row, or carry and flag? Recommend: clamp under `/etc`, carry and flag elsewhere.
3. **Fleet ownership:** content-only variant key with ownership resolved separately (narrowest mode, flagged when wider than a host), or ownership in the key? Recommend: content-only, as § 6.
4. **A local `skip` user that carried files or tmpfiles lines need:** the scan defaults it to `useradd` with the reason? Recommend: yes; it applies decision 6 through the existing users block.
5. **An operator who keeps such a user on `skip`:** numeric `chown` with a guard and a required-accounts row, or fall back to `root:root`? Recommend: numeric.
6. **An account id already taken in the image by another name:** stop the build, or let `useradd` pick an id? Recommend: stop, as the users block does for groups.
7. **ACLs and file capabilities:** detect and notice now, carry later? Recommend: notice now.
8. **System accounts the image lacks:** create them with `useradd` at the host's id in an accounts block before packages, mirrored in a `sysusers.d` line with the same id (§ 5.3), or create them after the users block and let any carried package allocate its own id first? Recommend: before packages, with the mirror. It is bootc's documented pinning pattern, keeps the lint quiet, and is the only option that stops a rebuild from renumbering an account. The cost is that inspectah, not the package, picks the id of a package-created account; the host's id is the one the carried files already use. `systemd-sysusers` alone is not an option: it silently takes another id when the requested one is used (§ 12 Q3).
9. **`/var` parent directories:** fold the dedup fix in? Recommend: yes, § 5.6.

## 14. Size

About 17 plan tasks across `core` (types, `AccountName` move, schema, defaults), `collect` (executor metadata, `HostAccounts`, config, unmanaged and storage collectors, finalize pass, ACL and capability detection), `pipeline` (resolver, accounts block before packages and the `sysusers.d` fragment, ownership block, `/var` `d` lines by name replacing `resolve_tmpfiles_ownership`'s numeric cases and `mkdir_line`, tarball, extractor, notices, tmpfiles accounts, report), `refine` (export staging, export warnings), `web` (row detail, strategy reason) and `cli` (scan stderr). Up from 15: the bootc answers added the pinned accounts block with its mirror and the `/var` line rewrite. Still L, at its very top; decision 13.1 as an include would make it XL.
