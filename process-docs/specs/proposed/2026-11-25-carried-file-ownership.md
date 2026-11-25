# Carried Files Keep Their Owner, Group and Mode

## Metadata

| Field | Value |
|---|---|
| Date | 2026-11-25 |
| Status | Proposed. Goes first in 0.9.1-beta.1, ahead of the aggregate convergence spec, because it is more fundamental (Mark Russell, 2026-11-25). § 0 is a separate small change that lands before the rest (Mark Russell, 2026-11-25). § 6 is the field list the convergence spec takes as input. Decisions are recorded in § 13. |
| Release | 0.9.1-beta.1, in three landings: § 0, then the schema and the scan (§§ 3 and 4), then the render and both exports (§ 5). All before convergence code. |
| Scope | Owner, group and mode of every carried `/etc` file, carried unmanaged file and directory, and `/var` directory the export provisions: what the scan records, the snapshot types, one ownership resolver, account creation, the Containerfile sequence, both export paths, notices, the fleet field list, tests. § 0 replaces the per-user Containerfile strategy with one delivery switch for local users. |
| Decisions it implements | Ownership is preserved; one resolver for `/etc` and unmanaged content; provisioning-time accounts are handled and the operator is told which accounts the image requires (Mark Russell, 2026-11-23). `/etc` first; mode is not fixed alone (Mark Russell, 2026-11-23). A carried tmpfiles.d line that names an account the image lacks gets the account (Mark Russell, 2026-11-25). Classification lives in scan through one core defaults module; refine applies only user edits; an unedited export equals the tarball (Mark Russell, 2026-11-24). Variant identity stays byte-identical content (Mark Russell, 2026-11-24). The ten decisions in § 13 (Mark Russell, 2026-11-25). |
| Backlog items | Closes `inspectah-carried-files-lose-owner-and-group`, `inspectah-tmpfiles-line-owned-by-a-skipped-user-is-silently-skipped` and `inspectah-var-parent-dirs-lose-their-ownership` (inspectah-team `workflow/backlog/inspectah/open/`). Leaves `inspectah-storage-ownership-rows-report-var-directories-the-image-already-reproduces` open (§ 5.6). |
| Inputs | A security consult on carried ownership, 2026-11-25, read-only at `4b520879`: its eight rules are § 2.1. A bootc consult, 2026-11-25, read against ostree `3d72f1d`, bootc `b88f28b` and buildah `4bf2012`: its answers are § 12 and its rules § 2.2. A users consult, 2026-11-25, on the one-switch users model: § 0. The contract, security and image-mode reviews, 2026-11-25; the image-mode reviews read systemd `v252`, `v257` and `main` (`a04701d5eb`) for tmpfiles and sysusers, and ostree `3d72f1d` for directory metadata in the merge. |
| Verified | Code citations read at `59a068fa`, `4e00660d` and `619bcae8` (2026-11-25). Line numbers are a convenience; the symbol name is the identifier. Nothing was built or booted for this spec. Claims marked *inferred* or *recalled* were not checked; § 10.2 names the build or booted host that checks them. |

**No code, documentation, or CHANGELOG entry was written to produce this file.**

## 0. Prerequisite: one delivery switch for local users

A small change of its own, plan task 1, landed before anything else in this spec (Mark Russell, 2026-11-25). Decision 13.4.

**Today.** Each user carries `containerfile_strategy` (`skip` or `useradd`, default `skip`; `UserContainerfileStrategy`, `crates/core/src/types/users.rs:7`), set per user by `RefinementOp::UserStrategy` (`crates/refine/src/types.rs:175`). The kickstart and blueprint render every included user whatever the strategy (`render/users.rs:142-147`, `:259-263`); the strategy decides only whether the Containerfile also creates the user. A per-user `include`, read by `included_users` (`render/users.rs:67`), drops a user from all three. In refine, the user card's header checkbox, labelled "Include <name>", flips the strategy, not the include (`crates/web/ui/src/components/UserCard.tsx:207-216`); a strategy radio fieldset sits beside it (`:333-385`).

**After.**

```rust
/// Where the export creates the host's local users. The kickstart and
/// blueprint always carry every local user; this picks whether the
/// Containerfile also creates them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalUserDelivery {
    /// Kickstart and blueprint only. The production path.
    #[default]
    Provisioning,
    /// Also `useradd` in the Containerfile for every local user whose
    /// line the users block can prove. For testing; not recommended for
    /// production.
    Image,
}
```

- `UserGroupSection` gains `local_user_delivery: LocalUserDelivery`, `#[serde(default)]`. The scan writes `provisioning`. Nothing promotes it, per user or for the section.
- **Every local user goes into the kickstart and blueprint.** A single-host export no longer reads the per-user `include`, and refine offers no per-user include control on a single host. A fleet aggregate (`aggregate_meta` is set) keeps today's include behaviour until the users respec decides the fleet model (§ 11).
- **`Image`** makes the users block (`ContainerfileUserPlan`) create every local user, not a chosen few. A user whose line cannot be proved keeps its `# FIXME:` and is skipped, so "every" means every user that validates; failing the whole switch on one bad name would be stricter and less useful. The block's comment says it is for testing and not recommended for production. With `Image` and a user's password choice `preserve`, that hash is written into an image layer (`chpasswd_lines`, `render/users.rs:613`); the audit report and refine say so once.
- **Goes:** `UserContainerfileStrategy`; the per-user `containerfile_strategy` field, on the user JSON and on `UserGroupDecision`; `RefinementOp::UserStrategy` with its validation and apply (`session.rs:2454`, `:3206`), handler and route (`crates/web/src/handlers.rs:150`, `:504`; `lib.rs:167`) and `setUserStrategy` (`ui/src/api/client.ts:180`); the card's checkbox and radio fieldset; the per-user branch in `ContainerfileUserPlan::from_snapshot` (`render/users.rs:497`).
- **Comes:** `RefinementOp::SetLocalUserDelivery { delivery }`, its handler, route and client call, and one control on the users section (`UsersGroupsSection.tsx`) with the "not recommended for production" warning beside `Image`. `DecisionProjection` carries the section's value.
- **Unchanged:** password choice, `useradd_refusal`, home rules and the home tmpfiles line, the group plan and its collision stop, `chpasswd`, the key comment. The collision message (`render/users.rs:872`) says "deliver users at provisioning" in place of "set its users to Skip".
- `SCHEMA_VERSION` (`crates/core/src/snapshot.rs:21`) goes from 31 to 32, with a `**Breaking:**` changelog entry saying an earlier snapshot must be re-scanned.

Under `Image` the kickstart and blueprint still name users the image already holds. Whether both tolerate that is *recalled* as yes; § 12 Q8 and R11 check it.

The rest of this spec reads the switch: § 5.1 places a local user by name under `Image` and by number under `Provisioning`.

## 1. Problem

Every carried file and directory lands in the image owned by `root:root`, and every carried `/etc` file lands `0644`. A host file that was `0600` or `0640` is world-readable in the image, and a file a service reads as its own account is unreadable to it once the mode is right. The evidence, by layer:

**Scan.**

- `ConfigFileEntry` (`crates/core/src/types/config.rs:73-96`) has no owner, group or mode field. The config collector calls `file_metadata` only for a withheld entry (`ConfigCollector::withhold`, `crates/collect/src/inspectors/config/collector.rs:196-206`), and then keeps only `mode` and `size`.
- Carried `/etc` files come from five kinds of record, not one. `configtree::offers` (`crates/pipeline/src/render/configtree.rs:662`) stages config entries, repository files, GPG keys, firewall zones and unit drop-ins, each at its own path. A custom repository file the config collector leaves out as an unmodified RPM default is still carried by the repository section, and a config entry shadowed by a repository is inventory, never offered (`StagedFileKind::is_inventory_record`). None of these records carries ownership.
- `FileMetadata` (`crates/core/src/traits/executor.rs:33-40`) holds `mode`, `size`, `dev`, `ino` and `nlink`. No uid or gid, though `RealExecutor::file_metadata` (`crates/collect/src/executor/real.rs:440`) already has the `std::fs::Metadata` that carries both. It follows symlinks, and the executor has no call that does not.
- Unmanaged files record numeric `uid`, `gid` and `%a` permissions in `ProvenanceSignals` (`crates/core/src/types/nonrpm.rs:92-125`), from `get_file_metadata` (`crates/collect/src/inspectors/nonrpm.rs:2107-2124`). No names. A failed `stat` returns uid 0, gid 0 and an empty mode, so a file the scan could not stat reads as root-owned with no signal.
- `walk_for_unmanaged` (`nonrpm.rs:2224`) records files and symlinks only. Directory owner and mode are not in the snapshot.
- `/var` directories record mode, ids and names (`VarDirectory`, `crates/core/src/types/storage.rs:329-343`; `stat -c '%04a %u %g %U %G'` in `discover_var_directories`, `crates/collect/src/inspectors/storage.rs:182-260`). The dedup drops every parent of a discovered leaf, so `/var/lib/pgsql 0700 postgres:postgres` is gone before render.
- The users inspector records accounts with uid 1000 to 59999 only (`NON_SYSTEM_UID_MIN`, `NON_SYSTEM_UID_MAX`, `crates/collect/src/inspectors/users.rs:27-36`). A system account that owns a carried file is not in the snapshot by name.

**Render and export.**

- No `chown` or `--chown` appears under `crates/pipeline/src/render`. The only ownership-like line is the SSSD `chmod 0600` (`sssd_mode_lines`, `crates/pipeline/src/render/containerfile.rs:1792`), which writes `0600` whatever the host had, so a `0400` host file is widened.
- `render/tarball.rs:99-108` packs directories `0755`, unmanaged payloads with `PAYLOAD_MODE_BITS` (`0o777`, so setuid, setgid and sticky are dropped), and everything else `0644`. `build/extract.rs:494-498` keeps a member's mode only for unmanaged payloads.
- The config tree is copied as whole directories (`COPY config/<root>/ /<root>/`, `containerfile.rs:1727`). Whether a directory `COPY` onto an existing image directory such as `/etc/sudoers.d` (`0750` on the base) resets its mode to the member's `0755` is not checked; § 10.2 R3 checks it.
- The Containerfile's order is packages, config (`:205-209`), unit state (`:211-216`), non-RPM (`:218-223`), containers, users (`:238-244`). `FROM` is written inside `packages_section_lines` (`:569-588`), ahead of the repository `COPY` and the `dnf install`, so nothing can go between `FROM` and the packages today.

**Interim state.** beta.8 names the loss: `render/unmanaged_owners.rs` lists every carried unmanaged file whose host owner or group was not root or that had a special bit, and the scan prints the count. `render/tmpfiles_accounts.rs` names every carried tmpfiles.d line whose account the image will not have. Both are notices; this spec replaces them with the fix.

**Why mode cannot go first.** Carrying `0640` while ownership stays `root:root` does not leak, but it breaks: `/etc/named.conf 0640 root:named` becomes unreadable to `named`, which then fails to start. Today's `0644` leaks but works. Mode alone trades a confidentiality bug for an availability bug on exactly the files that were configured with care. Mode, owner, group and the fallback land together (rule 6).

## 2. Rules this spec carries

### 2.1 From the security consult

Adopted as requirements:

1. **Never widen a carried mode.** The image mode is the host mode or narrower. No path in the renderer defaults a carried file to `0644`. An unknown mode fails closed.
2. **Record owner and group names at scan time,** not only numbers. The snapshot cannot map a system uid back to a name later.
3. **`chown` by name in a `RUN` after the users block,** not `COPY --chown`. The build fails when a name does not resolve.
4. **An account the image lacks is created at the host's id,** before any `chown` that names it.
5. **setuid and setgid carry only when owner and group also carry,** by root, by name or by number; never on a `root:root` fallback; never on a group- or world-writable file.
6. **Mode, owner, group and the fallback land together.**
7. **A redacted file keeps the host's mode.**
8. **A fleet build never ships a mode wider than any contributing host** for the same content without a flag.

### 2.2 From the bootc consult and the image-mode review

Adopted as requirements; the evidence is § 12.

9. **Ownership in the image must be right the first time.** An owner, group or mode change on a deployed host makes that `/etc` file locally modified, and from then on no later image change to it arrives. A directory under `/etc` goes the same way once a file directly inside it is modified or added on the host: ostree's merge then copies that host directory's owner and mode over the image's. The export says so wherever it reports ownership (§ 5.8).
10. **Pin every system account the image creates for carried content at the host's id:** created before any package install that could allocate it, mirrored in a `sysusers.d` line at the id the build gave it unless a package or the base already declares it, pinned by an override when a package declares it without a fixed id, and the build stops if the id is taken (§ 5.3). Local users are pinned at their host ids by provisioning or by the users block (§ 0), never by the accounts block.
11. **`/var` ownership and mode reach the host as tmpfiles.d `d` lines, never as image content** (§ 5.6). Image content under `/var` is applied once, at install. A `d` line enforces its owner and mode on every boot, so a fallback line is create-only.
12. **Carry no SELinux labels** (§ 11).

Rule 3 holds with a sharper reason: `COPY --chown=<name>` reads only the image's `/etc/passwd` and `/etc/group`, so it fails the build for any account the RHEL bootc bases keep in `/usr/lib/group`.

Trust assumptions, stated once:

- The host's owner and mode are taken as intended, not audited, so a host misconfiguration ships, except as § 5.4 clamps it.
- Names resolve in the build image, so the image's account database is trusted for name to id.
- `stat` runs as root at scan time, and a file changed afterwards is not re-checked.
- A carried symlink is a host-controlled pointer. The export never acts through one (§ 5.1).
- A name NSS resolves may come from a directory service, not from the host's own account files (§ 4.4).
- Provisioning creates local users at the ids the export's kickstart or blueprint name, and only those it names. A user the renderers refuse is not delivered, and the resolver knows which (§ 5.1, § 8). Provisioning the export does not write is outside what it can know.

## 3. Snapshot types

### 3.1 Ownership of one path

A new module, `crates/core/src/types/ownership.rs`, holds the types every carried path uses.

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
    /// `stat` failed. The export falls back (§ 5.4); nothing reads this
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
    Unnamed {
        id: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        host_name: Option<String>,
    },
}

/// Permission and special bits, `0o0000..=0o7777`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Mode(u16);
```

`Unknown` is an enum variant, not a sentinel. Today's `(0, 0, "")` for a failed `stat` becomes `Unknown`, and no type left in the snapshot can say "root" when it means "not known".

`Mode` records the special bits; whether they reach the image is the resolver's call (§ 5.5), not the snapshot's.

`AccountName` moves from `crates/pipeline/src/render/safety.rs:499` to core, with `ExportToken` (`safety.rs:332`), whose rules it wraps; their parse rules are unchanged and the pipeline re-exports both. The collector decides at scan time whether a host name is `Named` or `Unnamed { host_name: Some(..) }`. An SSSD name such as `jdoe@ad.example.com` that the token set refuses is classified there, once, not refused by the renderer later.

### 3.2 The ownership section

Ownership is recorded once per host path, in a section of its own, keyed by where the path lands in the image. Each record kind that stages host bytes (§ 1) then needs no field of its own, and a carried path whose record is shadowed or excluded still has its metadata when an edit brings it back.

```rust
/// Host ownership for every path the export can carry, and the accounts
/// those paths and carried tmpfiles.d lines name.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OwnershipSection {
    /// Every host file a section records for staging, every directory
    /// above one that departs from `0755 root:root` or could not be
    /// read, and every unmanaged symlink, keyed by image path.
    #[serde(default)]
    pub paths: BTreeMap<Destination, PathRecord>,
    /// Users a recorded path or a carried tmpfiles.d line names.
    #[serde(default)]
    pub users: BTreeMap<AccountName, UserLookup>,
    /// Groups a recorded path or a carried tmpfiles.d line names, and
    /// the primary group of every recorded user.
    #[serde(default)]
    pub groups: BTreeMap<AccountName, GroupLookup>,
    /// Paths with a POSIX ACL beyond the mode bits (§ 4.6).
    pub acl: Detection,
    /// Paths with a file capability (§ 4.6).
    pub capabilities: Detection,
}

/// What the host had at one image path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PathRecord {
    File {
        host: Ownership,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        clamp: Option<ModeClamp>,
    },
    Directory {
        host: Ownership,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        clamp: Option<ModeClamp>,
    },
    /// A symbolic link on the host that the image also holds as a link:
    /// an unmanaged symlink, or a directory link the unmanaged walk did
    /// not enter. Its own owner is never applied (§ 5.1), so none is
    /// recorded. A directory link the stager writes through is a
    /// `Directory` (§ 4.1).
    Symlink,
}

/// A mode the scan narrows by default. `host.mode` stays as recorded;
/// the image gets `mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModeClamp {
    pub mode: Mode,
    pub reason: ClampReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClampReason {
    /// `o+w` under `/etc`, file or directory (decision 13.2).
    WorldWritableUnderEtc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UserLookup {
    Found(HostUser),
    /// A carried tmpfiles.d line names it and NSS has no answer.
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum GroupLookup {
    Found(HostGroup),
    NotFound,
}

/// The host's entry for a user: what the export needs to create it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostUser {
    pub uid: u32,
    /// `Unnamed` when the host has no valid name for the primary gid:
    /// the account cannot be created (§ 5.1), and the fact is kept.
    pub primary_group: AccountRef,
    pub home: String,
    pub shell: String,
    pub source: AccountSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_by: Option<SysusersDeclaration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostGroup {
    pub gid: u32,
    pub source: AccountSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_by: Option<SysusersDeclaration>,
}

/// Where the host's answer came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountSource {
    /// `/etc/passwd` or `/etc/group`, or `/usr/lib/passwd` or
    /// `/usr/lib/group` where the host has them.
    LocalFiles,
    /// NSS answered and no local file holds it: SSSD, LDAP and the like.
    Directory,
}

/// A host `/usr/lib/sysusers.d` line that declares the account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SysusersDeclaration {
    pub file: SysusersFile,
    pub id: SysusersId,
}

/// The declaring file's name, which § 5.3's override writes into a
/// `RUN` line as an operand and a redirect target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SysusersFile {
    /// A name the override may write.
    Named { name: SysusersFileName },
    /// A name that fails `SysusersFileName`'s parse, kept for the notice.
    /// No override is written for it.
    Refused { host_name: String },
}

/// An `ExportToken` that ends in `.conf` and starts with neither `.`
/// nor `-`: `named.conf`. The token set has no `/`, so it is never a
/// path. Parsed at scan and again at load.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SysusersFileName(ExportToken);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SysusersId {
    /// The line fixes the id: a plain number in a `g` line, or a plain
    /// number before any `:` in a `u` line.
    Pinned { id: u32 },
    /// `-`, a path, `-:<group>`, or any other form: `systemd-sysusers`
    /// picks the id. Always the value for a group a `u` line implies.
    Unpinned,
}

/// What a detection found, and where it could not run. A partial result
/// keeps both: the paths found on the roots and hosts where it ran, and
/// a gap for each root and host where it did not.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detection {
    /// Image paths found, sorted and deduplicated. Carries no host: the
    /// notice tells the operator which paths to check, wherever found.
    #[serde(default)]
    pub paths: BTreeSet<String>,
    /// Empty when the check ran on every carried root of every host.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gaps: Vec<DetectionGap>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectionGap {
    /// The carried root the command ran on: `/etc`, `/opt` or `/usr/local`.
    pub root: String,
    /// The tool is absent, or its error.
    pub reason: String,
    /// The host the gap came from, as `aggregate_meta` names it. Set by
    /// the aggregate merge; a single-host scan leaves it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}
```

Whether an account is a local user, a system account or a directory account is not stored: the resolver derives it from `source` and the users section (§ 5.1), so the scan records host facts and the classification has one home.

| Record | Change |
|---|---|
| `InspectionSnapshot` | gains `ownership: Option<OwnershipSection>`. `None` makes every host-byte write `Unknown` at render (§ 5.1). |
| `UnmanagedFile.provenance` | loses `uid`, `gid`, `permissions`. Their readers, `ownership_losses` and its uid/gid filter (`render/unmanaged_owners.rs:131`, `:473`), `crates/web/src/adapter.rs:497-499` and `crates/web/src/aggregate_handlers.rs:1518-1520`, read `ownership.paths` instead, in the same landing (§ 14). |
| `VarDirectory` | `ownership: Ownership` replaces `mode`, `owner_name`, `group_name`, `owner_uid`, `owner_gid`. A provisioned directory with no recorded mode is `Unknown`. Their readers, `resolve_tmpfiles_ownership` and `mkdir_line` (`containerfile.rs:2606`, `:2761`), the report's `/var` rows (`render/report.rs:697`), refine's projection (`crates/refine/src/projection/reference.rs:567`) and the web adapter (`adapter.rs:1786`, `:2009`), read `ownership` instead, in the same landing. |
| `WithheldConfigEntry` | unchanged. Withheld members never ship, whatever their mode, and are not recorded in `paths`. |

A directory is recorded only when it departs from `0755 root:root` or its `lstat` failed. An unrecorded directory is therefore `0755 root:root` on the host, which is what the tarball already writes.

### 3.3 Wire format

- **`Mode`** serializes as exactly four octal digits, zero-padded: `"0640"`, `"4755"`, `"0000"`, `"7777"`. Load accepts only `[0-7]{4}`; `"644"`, `"0o644"`, `"10644"` and `""` are load errors. The collector builds it from the numeric `st_mode & 0o7777`, never from text, so one strict form costs nothing.
- **`AccountName`** serializes as its plain string, through `#[serde(try_from = "String", into = "String")]` on `AccountName` itself; `ExportToken` stays without serde. Load runs the same parse as the renderer, so a name with a space or a leading `-` is a load error. As a map key it is the same string.
- **`Destination`** (already in core, `crates/core/src/types/destination.rs:45`) gains the same `try_from`/`into` string pair, so it is the `paths` key. Its normal form is absolute with no trailing `/`.
- **`SysusersFileName`** serializes as its plain string; load runs the same parse as scan, so `"x.conf;curl …|sh;#.conf"`, `".conf"`, `"-x.conf"` and `"named"` are load errors.
- Enum tags are `state` (`Ownership`, `UserLookup`, `GroupLookup`) and `kind` (`AccountRef`, `PathRecord`, `SysusersFile`, `SysusersId`), values `snake_case`. `Detection` is a plain struct.

```json
"ownership": {
  "paths": {
    "/etc/named.conf": { "kind": "file", "host": { "state": "recorded",
      "owner": { "kind": "named", "name": "root", "id": 0 },
      "group": { "kind": "named", "name": "named", "id": 25 }, "mode": "0640" } },
    "/etc/foo.conf": { "kind": "file", "host": { "state": "recorded",
      "owner": { "kind": "named", "name": "root", "id": 0 },
      "group": { "kind": "named", "name": "root", "id": 0 }, "mode": "0666" },
      "clamp": { "mode": "0664", "reason": "world_writable_under_etc" } },
    "/etc/appsvc": { "kind": "directory", "host": { "state": "recorded",
      "owner": { "kind": "named", "name": "appsvc", "id": 985 },
      "group": { "kind": "named", "name": "appsvc", "id": 985 }, "mode": "0750" } },
    "/opt/tools/report.sh": { "kind": "file", "host": { "state": "unknown", "reason": "Permission denied (os error 13)" } },
    "/opt/appsvc/current": { "kind": "symlink" }
  },
  "users": {
    "appsvc": { "state": "found", "uid": 985,
      "primary_group": { "kind": "named", "name": "appsvc", "id": 985 },
      "home": "/var/lib/appsvc", "shell": "/sbin/nologin", "source": "local_files" },
    "ghost": { "state": "not_found" }
  },
  "groups": {
    "appsvc": { "state": "found", "gid": 985, "source": "local_files",
      "declared_by": { "file": { "kind": "named", "name": "appsvc.conf" },
        "id": { "kind": "unpinned" } } }
  },
  "acl": { "paths": ["/etc/appsvc/secret.conf"] },
  "capabilities": { "paths": [], "gaps": [
    { "root": "/etc", "reason": "getcap is not installed" },
    { "root": "/opt", "reason": "getcap is not installed" },
    { "root": "/usr/local", "reason": "getcap is not installed" } ] }
}
```

`SCHEMA_VERSION` goes from 32 (§ 0) to 33. inspectah has no compatibility promise before 1.0: the change gets a `**Breaking:**` changelog entry saying a snapshot from an earlier version must be re-scanned, and nothing else.

## 4. Scan

### 4.1 Metadata

`FileMetadata` gains `uid` and `gid`. The executor gains `symlink_metadata(path)`, the `lstat` counterpart of `file_metadata`, returning the same struct; `RealExecutor` implements it with `std::fs::symlink_metadata`, and the mock executor and the machine-state gate pass it through. `file_metadata` keeps following links for its existing callers.

Which call a record uses follows what the image will hold:

- **A file the stager writes as content** (config entries, repository files, GPG keys, firewall zones, drop-ins): `file_metadata` on the path the bytes were read from, which follows links exactly as the read did. For a config entry that is `c.read`, so a `%ghost` file under a directory link gets the metadata of the file whose bytes the image holds. The image always holds a regular file here.
- **A directory above such a file**: `file_metadata` too. The stager writes the file under real directories, so the image holds a directory at that path even where the host had a link (an `/etc` directory link a `%ghost` file was read through). It is recorded as a `Directory` with the metadata of the directory the host's link resolved to, so it never ships wider than the host's effective access.
- **An unmanaged item and every other directory**: `symlink_metadata`. A symlink is `PathRecord::Symlink`; the export recreates it with `ln -sfn` (`render/unmanaged.rs:437`) and never applies ownership to it or through it. The unmanaged walk never enters a directory link (`walk_for_unmanaged` tests `-L` before descending), so no unmanaged item sits below one; a directory link the walk records is a `Symlink`. Where one directory is above both kinds, the first rule wins, because the stager writes it as a real directory.
- **A failed call** records `Unknown` with the error, for files and for directories alike.

### 4.2 Which paths

An `OwnershipRecorder` in the collect context, shared by every inspector the way the account index is (§ 4.4), takes one call per host path whose bytes a record holds:

| Collector | Records | Read path |
|---|---|---|
| config | every `ConfigFileEntry`, whatever its disposition, shadowed inventory included | `c.read` |
| rpm | every `repo_files` and `gpg_keys` entry | the entry's path |
| network | every `firewall_zones` entry | the entry's path |
| units | every `drop_ins` entry | the entry's path |
| unmanaged | every `UnmanagedFile` item, and each directory the walk enters | the item's path |

Withheld entries (config and unmanaged) are not recorded: they never ship. Content inspectah generates (cron-converted units, the flatpak provisioning service, the kargs file) has no host file and is not recorded; the resolver places it as today (§ 5.1). Two records for one destination (a repository file and its config twin) are the same host file; the first recording stands.

**Ancestors.** After the inspectors run, the recorder walks each recorded file and directory up to its carried root (`/etc`, `/opt`, `/usr/local`; the roots themselves are the base's and not recorded) and reads each directory once, with the call § 4.1 gives it: `file_metadata` above a file the stager writes as content, `symlink_metadata` otherwise. It records a `Directory` that departs from `0755 root:root` or failed, or, under `symlink_metadata`, a `Symlink` for a directory link.

Recording every candidate, not only included ones, keeps scan and refine apart: an include edit in refine never needs metadata the scan did not take.

### 4.3 Scan defaults

Per Mark's rule, the scan decides defaults and refine applies only the operator's edits. Ownership has one default, written into the core defaults module beside the machine-state table:

- **World-writable under `/etc`.** A `File` or `Directory` under `/etc` whose host mode has `o+w`, sticky or not, gets `clamp: Some(ModeClamp { mode: host & !0o002, reason: WorldWritableUnderEtc })`. `host.mode` is not changed, so every notice still shows what the host had. Decision 13.2.

No other ownership default exists. The users section's default is § 0's `provisioning`. Refine displays the clamp and never recomputes it. An operator edit that changes what the resolver can prove (an include, the delivery switch) changes the render's plan, which is a consequence of the edit, not a reclassification.

### 4.4 Accounts

A new `HostAccounts` index, built once per scan in the collect context and shared by every inspector:

- **Names.** It reads `getent passwd` and `getent group` once, through NSS, so a name SSSD or LDAP serves is found where enumeration lists it. An id enumeration did not list (SSSD with `enumerate = false`, the RHEL default, *recalled*) is looked up once with `getent passwd <id>` or `getent group <id>`, through an argument vector, and cached. An id with no answer is `Unnamed { id, host_name: None }`; a name `AccountName` refuses is `Unnamed { id, host_name: Some(name) }`. When several names share an id, the `getent <id>` answer stands.
- **Source.** It reads `/etc/passwd`, `/etc/group`, and `/usr/lib/passwd` and `/usr/lib/group` where present. A name in any of them is `LocalFiles`; a name only NSS answers is `Directory`.
- **Package declarations.** It reads the `u` and `g` lines of `/usr/lib/sysusers.d/*.conf`. A `LocalFiles` account a line names gets `declared_by` with the file's name and whether the line pins the id. A `u` line whose id field has no `:` also declares the same-named group, `Unpinned`: `systemd-sysusers` gives that group the uid only if the number is free (image-mode review, `add_group` at systemd `v252` and `v257`). A `u` line with `:<group>` declares only the user. A file name `SysusersFileName` refuses is `SysusersFile::Refused`.

After the inspectors and the recorder run, a pass in `finalize_cross_inspector` fills `users` and `groups` from the index:

1. Every `Named` owner of a recorded `File` or `Directory` goes into `users`, and every `Named` group into `groups`.
2. Every account name in the user and group fields of a carried tmpfiles.d fragment (config entries under `/etc/tmpfiles.d/`, unmanaged files under `/usr/local/lib/tmpfiles.d/`, any disposition) is looked up by name; no NSS answer is `NotFound`. A numeric field needs no account. A field `AccountName` refuses is not recorded, and the tmpfiles notice names its line, as today.
3. Each `Found` user's primary gid goes into `groups` when it has a valid name; otherwise the user keeps `primary_group: Unnamed` and § 5.1 treats it as uncreatable.

Accounts in the users inspector's range are recorded too: the resolver needs one table, and the users section's `serde_json::Value` entries do not hold the source.

### 4.5 `/var`

`discover_var_directories` keeps its `stat` but maps it into `Ownership`; `UNKNOWN` from `%U` or `%G` becomes `Unnamed`. Names found there go through § 4.4's pass like any recorded path's. The dedup changes as § 5.6 says.

### 4.6 ACLs and file capabilities

Not carried (§ 11). Detected cheaply so the operator is told: one `getfacl -R --skip-base --absolute-names` and one `getcap -r` per carried root (`/etc`, `/opt`, `/usr/local`), parsed into a list of paths. One command per root, not per file. Paths only, never the ACL entries, which can name directory users. A tool that is absent gives a gap for every root; a command that fails on one root gives a gap for that root, and the paths from the roots where it ran are kept. The notice lists the paths found and names each gap. Decision 13.7.

## 5. Rendering

### 5.1 One resolver

A new `crates/pipeline/src/render/ownership.rs` builds one `OwnershipPlan` per render. Every ownership line, account line, tmpfiles `d` line owner, notice and report row comes from it; nothing else reads `ownership` or `VarDirectory.ownership` to decide output.

**Input is the render's include set**, the same set the `COPY` and `ln` lines take:

- every write in `staged_writes(snap).writes` (`configtree.rs:938`) whose kind stages host bytes (`ConfigFile`, `RepoFile`, `GpgKey`, `FirewallZone`, `DropIn`), joined by destination to `ownership.paths`. A host-byte write with no record is `Unknown` with the reason "not recorded": it fails closed.
- every included, not withheld unmanaged item outside `/srv` (findings under `/srv`, a link into `/var` on both bases, go to the `/var` strategy, `included_var_srv_paths`, `render/unmanaged.rs`).
- every `Directory` or `Symlink` record above a path already in the set, and no other.
- accounts from included tmpfiles fragments only.
- the users section and § 0's switch, read through `ProvisioningDelivery` and `ContainerfileUserPlan` below; `storage.var_directories` (§ 5.6).

**What provisioning delivers.** Numeric ownership is right only for an account the kickstart and blueprint actually write at a fixed id; a guard proves the image does not conflict, not that the account will exist. `render/users.rs` gains one walk that both renderers consume:

```rust
/// The local accounts the kickstart and blueprint write, and why any
/// listed account is not written. `render_kickstart` and
/// `render_blueprint_toml` render from this, so what they write and
/// what the resolver assumes cannot diverge.
pub(crate) struct ProvisioningDelivery {
    users: BTreeMap<AccountName, Delivery>,
    groups: BTreeMap<AccountName, Delivery>,
}

pub(crate) enum Delivery {
    /// Both renderers write the account at this id.
    AtId(u32),
    NotDelivered(DeliveryGap),
}

pub(crate) enum DeliveryGap {
    /// `useradd_refusal` refused the name, home, shell or a
    /// supplementary group, and a refusal costs the whole user
    /// (`render/users.rs:184`, `:301`).
    Refused(&'static str),
    /// A fleet aggregate excludes the user or the custom group (§ 0).
    Excluded,
    /// No id is recorded, so the line has no `--uid` or `--gid` and
    /// provisioning picks one.
    NoFixedId,
}
```

A user is `AtId(uid)` when it is included (always on a single host), `useradd_refusal` passes and its uid is non-zero. A custom group is `AtId(gid)` when it is included, its name parses and its gid is non-zero. A user's own same-named group is `AtId(gid)` when the user is `AtId` and its line carries that `--gid` (`user --gid` creating it is *recalled*; R11 checks it). Any other local account is `NotDelivered` with its gap.

Content inspectah generates keeps today's `0644 root:root` and gets no plan entry. An excluded file the base also ships (`/etc/hostname`) is not in the set, so its base copy is never touched.

```rust
/// How one subject of a carried path, its owner or its group, reaches
/// the image. Owner and group are resolved independently.
enum Subject {
    /// Id 0, by id: nothing to do.
    Root,
    /// The image holds the account by this name when the ownership block
    /// runs: the accounts block, the users block or the base made it.
    ByName(AccountName),
    /// Written as a number, behind a guard (§ 5.3).
    ByNumber { id: u32, name: AccountName, via: NumericVia },
    /// Not carried: `root` for this subject.
    Fallback(FallbackCause),
}

enum NumericVia {
    /// A local user or group the export's kickstart and blueprint create.
    Provisioning,
    /// A directory service serves the account at runtime.
    Directory,
}

enum FallbackCause {
    StatFailed,
    NotRecorded,
    OrphanId(u32),
    InvalidName(String),
    /// A local user or group that neither the users block nor
    /// provisioning creates at the recorded id.
    NotDelivered { name: AccountName, why: DeliveryGap },
    /// A system account the accounts block cannot create.
    Uncreatable { name: AccountName, why: Uncreatable },
    /// A carried tmpfiles.d line names it and the host had no answer.
    NotFound(AccountName),
}

/// Why the accounts block cannot create a system user: its primary
/// group cannot exist when `useradd -g` runs.
enum Uncreatable {
    /// The host has no valid name for the primary gid.
    PrimaryGroupUnnamed,
    /// The primary group is a local group. Provisioning creates it at
    /// install and the users block after the accounts block, both too
    /// late, and the accounts block never creates a local account.
    PrimaryGroupLocal(AccountName),
    /// The primary group is a directory group, never created (13.8).
    PrimaryGroupDirectory(AccountName),
}

/// One entry of the accounts block, in the order it runs.
enum SystemAccount {
    Group { name: AccountName, gid: u32 },
    /// Its `primary_group` is a `Group` entry earlier in the list.
    User {
        name: AccountName,
        uid: u32,
        primary_group: AccountName,
        home: ExportPath,
        shell: ExportPath,
    },
}

/// A path the ownership block may act on. There is no symlink kind: a
/// link is never an operand of `chown` or `chmod`.
enum PlannedKind { File, Directory }

struct PlannedPath {
    path: ExportPath,
    kind: PlannedKind,
    owner: Subject,
    group: Subject,
    /// Permission bits for the image. Never wider than the host's.
    mode: Mode,
    /// Special bits restored after `chown` (§ 5.5).
    special: SpecialBits,
    /// The clamp, a carried world-writable mode, a dropped special bit:
    /// what the notice says about this path.
    notes: Vec<PathNote>,
}

/// One account the build creates or checks, keyed by namespace.
enum Namespace { User, Group }

struct OwnershipPlan {
    paths: Vec<PlannedPath>,
    /// The accounts block, groups before users.
    create: Vec<SystemAccount>,
    /// One per distinct `ByNumber` subject.
    guards: Vec<NumericGuard>,
    /// Package-declared accounts with an unpinned id (§ 5.3).
    overrides: Vec<SysusersOverride>,
    /// Owner and group fields for `inspectah-var.conf` (§ 5.6).
    var_lines: Vec<VarLine>,
    rows: Vec<NoticeRow>,
}
```

**Resolution of one subject**, the owner in the user namespace and the group in the group namespace:

1. **Id 0:** `Root`, decided by id, never by name.
2. **`Unknown` ownership:** both subjects `Fallback(StatFailed)` or `Fallback(NotRecorded)`.
3. **`Unnamed`:** `Fallback(OrphanId)` or `Fallback(InvalidName)`.
4. **A local user or group** (the users section lists the user, or lists the group as custom, or the group is a listed user's own same-named primary group), in this order:
   - under `Image`, `ByName` when the users block creates it (`ContainerfileUserPlan::creates_user`, or `guaranteed_gid` for a group);
   - `ByNumber { via: Provisioning }` when `ProvisioningDelivery` has it `AtId` the recorded id, under either delivery value (under `Image` the kickstart and blueprint still write every local user);
   - otherwise `Fallback(NotDelivered)` with the gap. An aggregate-excluded user whose files stay included falls back here, and so does a user the renderers refuse.
   Decision 13.5.
5. **A system account** (`LocalFiles`, not in the users section): the accounts block creates it at the host's id before packages unless the base already holds it (§ 5.3), then `ByName`. The block passes no `-G`, so a user's only prerequisite is its primary group, and the plan closes over it: each planned user's primary group enters `create` as a `Group` entry ahead of the user, whether or not a carried path names the group (the entry's `getent` check makes it a no-op when the base holds it). A user whose primary group cannot be such an entry is `Fallback(Uncreatable)` with the reason: `Unnamed` on the host, a local group (step 4), or a directory group (step 6). Nothing is invented for it, no `useradd` is written, and each subject that names it falls back.
6. **A directory account** (`Directory`): `ByNumber { via: Directory }`, with a "served by the directory" row. It is never created and never mirrored (decision 13.8): a local entry would shadow the directory's under `passwd: files sss`, survive the person's removal from the directory, and pin a mapping the directory may later reuse.
7. **A `NotFound` name** from a tmpfiles line: `Fallback(NotFound)`.

Any pair is valid: `chown -h 1001:named`, `chown -h root:1001`, `chown -h appsvc:root`. Guards and notices come from each subject separately.

### 5.2 Mode: staged, not commanded

A file's permission bits reach the image through the staged tree, as unmanaged payloads already do: `COPY` gives the image file the build-context file's mode. The staged mode is the plan's `mode & 0o777`, which already holds the clamp and § 5.4's fallback narrowing. This covers every carried `/etc` file, including the RPM-owned modified ones (`sshd_config` at `0600` stays `0600`), and it adds no Containerfile lines for the common root-owned case.

- **The tarball** (`render/tarball.rs`) writes each staged member with the plan's mode, owner uid 0. Members stay owned by uid 0: a root operator running plain `tar x` would otherwise chown files to host ids that mean other accounts on the workstation (*inferred*).
- **Both export paths stage the tree** (skill `export-paths-stage-different-trees`): the CLI tarball and the refine download each set the mode on the staged file, and `build/extract.rs` keeps every member's mode, not only unmanaged payloads', masked with `0o777`. The guard fixture in that skill gains a `0600` config file.
- **A mode with no owner read bit** (`0000`, `0040`) cannot be read by a rootless `podman build` from the operator's tree. The staged copy adds `u+r`, and the ownership block writes an explicit `chmod` to the plan's mode. *Rare; named so it is not a surprise.*
- **Directories** stay `0755` in the tarball. A planned `Directory` gets an explicit `chown -h` and `chmod` in the ownership block, which also covers an existing base directory a `COPY` may have reset (§ 10.2 R3) and is the only way the image can state it (rule 9).
- **Redacted files** are written with the host's mode (rule 7). Redaction changes content, never metadata.
- **SSSD.** `sssd_mode_lines` and its constants go. SSSD's files are config entries and carry their host mode like every other; a mode SSSD started with on the host is one it accepts. A `0400` host `sssd.conf` stays `0400`.

### 5.3 The Containerfile sequence

```
FROM <base>                        split out of packages_section_lines
accounts block                     system accounts, groups first
packages block                     repo files, GPG keys, modules, groups, dnf install
sysusers block                     mirror pruning, package overrides
drop-ins, firewall zones, scheduled tasks, config, unit state,
non-RPM, containers                as today
users block                        under Image only (§ 0)
ownership block                    guards, link refusal, chown -h, chmod
kernel and boot, security, network, ...   as today
```

`packages_section_lines` (`containerfile.rs:569`) loses its `FROM` and base comment to a new `from_lines`, so the accounts block can sit between `FROM` and the repository `COPY`. The accounts block never touches a local user, so no account is created twice: local users belong to provisioning or the users block (§ 0), system accounts to the accounts block.

**Accounts block.** For each entry of the plan's `create` list (§ 5.1 step 5), groups before users and every user's primary group among those groups, one `if` command per account, so a refused `groupadd` stops the build before any `useradd` runs (`&&` and `||` have equal precedence in POSIX `sh`, so a bare `getent || groupadd && getent || useradd` chain would continue past a refusal):

```dockerfile
# Accounts carried files need, created at the source host's ids before
# any package can allocate them. Each stops the build if its id is taken.
RUN if ! getent group appsvc >/dev/null; then \
      groupadd -r -g 985 appsvc \
        || { echo "inspectah: group appsvc: gid 985 is taken in the image" >&2; exit 1; }; \
      echo "g appsvc 985" >> /usr/lib/sysusers.d/inspectah-accounts.conf; \
    fi \
 && if ! getent passwd appsvc >/dev/null; then \
      useradd -r -M -u 985 -g appsvc -d /var/lib/appsvc -s /sbin/nologin appsvc \
        || { echo "inspectah: user appsvc: uid 985 is taken in the image" >&2; exit 1; }; \
      echo "u appsvc 985:$(getent group appsvc | cut -d: -f3) - /var/lib/appsvc /sbin/nologin" \
        >> /usr/lib/sysusers.d/inspectah-accounts.conf; \
    fi
```

- **Before packages.** A carried package that creates the account (`bind` and `named`) finds it present and leaves it, so the image holds the host's id rather than whatever the build allocates. An id allocated by the build can change on a later rebuild, and nothing on bootc remaps owners when it does (§ 12 Q2).
- **The `getent` guard** goes through NSS and sees `/usr/lib/passwd` and `/usr/lib/group`, so an account the base already holds is left alone and `chown` by name resolves to the base's id. That account is the base vendor's to pin, and it gets no mirror line.
- **A taken id stops the build** with a message naming the account and the id (decision 13.6). `useradd -g appsvc` takes whichever gid the group has, the base's included.
- **Home and shell** are the host's, validated as `ExportPath`; an invalid one falls back to `/` and `/sbin/nologin`.
- **The mirror is written only for what the block created**, at the id `getent` reports, so a base-held account or a base-held group never gets a line that disagrees with the image.

**Sysusers block.** One `RUN` after the packages block. The mirror is pruned by namespace and name: its `g n` line goes when another file declares `g n`, or `u n` with no `:` in the id field (which implies the same-named group); its `u n` line goes only when another file declares `u n`. A package that declares only `g appsvc` therefore leaves the mirror's `u appsvc` in place.

```dockerfile
# Keep a sysusers.d line only for accounts no package declares: a
# package's own line already covers its account.
RUN f=/usr/lib/sysusers.d/inspectah-accounts.conf; m=/tmp/inspectah-accounts.conf; \
    if [ -e "$f" ]; then \
      mv "$f" "$m"; \
      set -- /usr/lib/sysusers.d/*.conf; [ -e "$1" ] || set --; \
      awk -v m="$m" ' \
        FILENAME != m && $1 == "u" { u[$2] = 1; if ($3 !~ /:/) g[$2] = 1 } \
        FILENAME != m && $1 == "g" { g[$2] = 1 } \
        FILENAME == m && !(($1 == "u" && ($2 in u)) || ($1 == "g" && ($2 in g))) \
      ' "$@" "$m" > "$f" \
        || { echo "inspectah: could not prune $f" >&2; exit 1; }; \
      rm -f "$m"; \
      [ -s "$f" ] || rm -f "$f"; \
    fi
```

The mirror is moved out of the directory first, so the glob lists only the other files and the mirror is read last, after every declaration is collected. No account name is written into this `RUN`; the mirror holds only the accounts block's own lines. What survives keeps `bootc container lint`'s `sysusers` check quiet for those names, and puts an account back on the first boot of a new image on a host whose locally modified `/etc/passwd` lacks it, which a later image adding the account would otherwise never reach. `useradd` in the build, not `systemd-sysusers`, creates the account: `systemd-sysusers` silently takes another id when the requested one is used, which would break rule 10. Decision 13.8.

**Package overrides.** An account the accounts block plans whose `declared_by` is `Unpinned`, user or implied group, would get a different id from the package's own line at boot on a host whose `/etc/passwd` or `/etc/group` lacks it, while the image's files are owned at the image's id. The same `RUN` writes one override per package file, at `/etc/sysusers.d/<the package's file name>`: the package's file, read once, with every planned account it declares pinned in one pass, every other line unchanged.

- **The replacements** are one per planned account whose declaration is `Unpinned`, grouped by the declaring file and keyed by namespace and name, with the ids read from the image after packages. A pinned declaration is left as the package wrote it.
- **A `u` line** for a planned user gets its uid part set to the image's uid: the whole id field when it has no `:` (`-`, a path, or any other form), the part before `:` otherwise, keeping `:<group>`.
- **A `g` line** for a planned group gets its id field set to the image's gid.
- **A planned group the file implies but does not declare** (a `u` line with no `:` and no `g` line for the name) gets a `g <name> <gid>` line appended. `systemd-sysusers` processes groups before users and a `u` line takes a queued same-named group's gid, so the pair pins both (image-mode review, systemd `v252` and `v257`). Writing `u <name> <uid>:<gid>` alone would not do it: `sysusers.d(5)` requires that group to exist or be created explicitly.

```dockerfile
 && if [ -e /usr/lib/sysusers.d/appsvc.conf ]; then \
      u1=$(getent passwd appsvc | cut -d: -f3); u2=$(getent passwd appwork | cut -d: -f3); \
      g1=$(getent group appsvc | cut -d: -f3); \
      [ -n "$u1" ] && [ -n "$u2" ] && [ -n "$g1" ] \
        || { echo "inspectah: appsvc.conf override: an account is missing from the image" >&2; exit 1; }; \
      awk -v users="appsvc=$u1 appwork=$u2" -v groups="appsvc=$g1" ' \
        BEGIN { nu = split(users, a, " "); for (i = 1; i <= nu; i++) { split(a[i], kv, "="); u[kv[1]] = kv[2] }; \
                ng = split(groups, b, " "); for (i = 1; i <= ng; i++) { split(b[i], kv, "="); g[kv[1]] = kv[2]; order[i] = kv[1] } } \
        $1 == "u" && ($2 in u) { c = index($3, ":"); $3 = (c == 0 ? u[$2] : u[$2] substr($3, c)) } \
        $1 == "g" && ($2 in g) { $3 = g[$2]; seen[$2] = 1 } \
        { print } \
        END { for (i = 1; i <= ng; i++) if (!(order[i] in seen)) print "g", order[i], g[order[i]] } \
      ' /usr/lib/sysusers.d/appsvc.conf > /etc/sysusers.d/appsvc.conf \
        || { echo "inspectah: could not write the appsvc.conf override" >&2; exit 1; }; \
    fi
```

File names are `SysusersFileName`s and account names `AccountName`s, so neither can leave its operand. A declaration whose file is `SysusersFile::Refused` gets no override and a row saying the id is not pinned on hosts that lack the account. Assigning `$3` rejoins the fields with single spaces; `systemd-sysusers` splits on whitespace and reads a quoted GECOS as one field, so only runs of spaces inside a quoted GECOS change, which is cosmetic.

**What the override is.** A same-named file in `/etc/sysusers.d` replaces the `/usr/lib/sysusers.d` one whole: `systemd-sysusers` lists `CONF_PATHS` in the order `/etc`, `/run`, `/usr/local/lib`, `/usr/lib` and skips any later file whose name it has seen ("Skipping overridden file" in `conf-files.c`), at systemd `v252` (RHEL 9) and `v257` (RHEL 10), and `sysusers.d(5)` says so at both (image-mode review; § 12 Q7). So the override is a whole-file replacement of the package's file, current as of the build. It is regenerated from the package's current file on every build, so a package update reaches it at the next build, and it lands in the image's `/etc` as a file the host has not modified, so it follows later images. On a host where someone edits it, it stops following the image (rule 9) and from then on masks every later change to the package's own file, not only the id: new accounts, memberships, a changed home. Each override is a notice row that says this. A host-side fix was rejected: editing `/etc` on hosts freezes those files against later images (rule 9). Decision 13.10.

**Guards for numeric owners.** One `RUN` at the top of the ownership block, one check per direction per `ByNumber` subject, for users and groups alike:

```dockerfile
# Accounts created at install, at the source host's ids. Files below are
# owned by number; the build stops if the image uses an id or a name
# differently.
RUN if getent passwd 1001 >/dev/null && [ "$(getent passwd 1001 | cut -d: -f1)" != appuser ]; then \
      echo "inspectah: uid 1001 (appuser, created at install) is taken in the image" >&2; exit 1; fi \
 && if getent passwd appuser >/dev/null && [ "$(getent passwd appuser | cut -d: -f3)" != 1001 ]; then \
      echo "inspectah: the image already has user appuser at another uid" >&2; exit 1; fi \
 && if getent group 1001 >/dev/null && [ "$(getent group 1001 | cut -d: -f1)" != appuser ]; then \
      echo "inspectah: gid 1001 (group appuser, created at install) is taken in the image" >&2; exit 1; fi \
 && if getent group appuser >/dev/null && [ "$(getent group appuser | cut -d: -f3)" != 1001 ]; then \
      echo "inspectah: the image already has group appuser at another gid" >&2; exit 1; fi
```

The name check catches a package that creates an account with a local user's name at its own id; the id check catches an image account at the host's number. `AccountName` refuses an all-digit name, so `getent passwd 1001` cannot mean a user named `1001`.

**Ownership block.** Right after the users block (`containerfile.rs:238-244`). One `RUN`: a refusal over every `chmod` operand that is a link in the image, then `chown -h` grouped by owner and group, then directory `chmod`, then `chmod` for modes the staged tree cannot carry (no owner read bit) and special bits:

```dockerfile
# Owner, group and special bits from the source host. Fix ownership here
# and rebuild: on a deployed host a chown freezes an /etc file against
# every later image, and /opt and /usr/local are read-only.
RUN for p in /etc/appsvc /etc/sudoers.d /opt/appsvc/bin/helper; do \
      [ ! -L "$p" ] || { echo "inspectah: $p is a symlink in the image" >&2; exit 1; }; \
    done \
 && chown -h root:named /etc/named.conf /etc/rndc.key \
 && chown -h appsvc:appsvc /etc/appsvc /etc/appsvc/secret.conf /opt/appsvc \
 && chown -h appsvc:appgrp /opt/appsvc/bin/helper \
 && chown -h 1001:1001 /opt/tools/report.sh \
 && chmod 0750 /etc/appsvc /etc/sudoers.d \
 && chmod 4750 /opt/appsvc/bin/helper
```

- **Links are never acted through.** The plan has no symlink kind (§ 5.1), so no carried link is an operand. Every `chown` is `chown -h`, which acts on a link itself and is identical for files and directories, so a path that is a link in the image but was not on the host cannot redirect it. `chmod` has no `-h`, so the refusal stops the build instead. Paths are written without a trailing `/`, which would resolve a link.
- `chown` fails the build when a name does not resolve, which is rule 3's failure with no extra code: the message is `chown`'s own and names the account. No `chown` line exists for a path whose subjects are both `Root`.
- Paths are `ExportPath`s; a path the token set refuses gets a `# FIXME:` line, through `sanitize_for_comment` (`render/language_packages.rs:168`) as the users block does, and a fallback row.

Every path the ownership block names is image content: `/etc` (whose image copy becomes the `/usr/etc` default the merge compares against), and `/opt` and `/usr/local`, plain image directories on both RHEL bootc bases, replaced on every update and read-only at runtime. `BASE_IMAGE_DIRECTORIES` (`render/selinux.rs:38-42`) lists `/opt`, and its comment covers `/usr/local` under `/usr`; the sysctl spec (`process-docs/specs/implemented/2026-11-11-sysctl-carry-source-files.md`) records the layout with its sources.

### 5.4 Fallbacks and the clamp

A path whose owner or group cannot be carried keeps the subject that can and falls back to `root` for the other:

| Cause | Image owner and group | Image mode |
|---|---|---|
| `stat` failed or not recorded | `root:root` | `0600` for a file, `0700` for a directory |
| Owner falls back | `root` for the owner | host bits, special bits cleared |
| Group falls back | `root` for the group | host bits with the group bits cleared, special bits cleared |

Owner bits handed to `root` grant nothing root does not have. Group bits handed to `root` are cleared, because the host granted them to a group that is not in the image. Other bits are kept: they were granted to everyone on the host, so keeping them does not widen. Every fallback is a row in the notice (§ 5.8) with its cause.

**World-writable.** Under `/etc`, files and directories take the scan's clamp (§ 4.3), `o+w` cleared, with a row. Elsewhere they carry, with a row. Decision 13.2.

### 5.5 Special bits

`chown` by root clears setuid and setgid on Linux (`chown(2)`, *recalled*), so special bits are restored by the `chmod` after the `chown`, never through the staged mode. The staged mode stays `& 0o777` (`PAYLOAD_MODE_BITS` is unchanged), and a special bit reaches the image only on a line the resolver wrote after carrying ownership.

- **setuid and setgid on a regular file** carry only when both subjects are `Root`, `ByName` or `ByNumber`, and the image mode is neither group- nor world-writable (rule 5, decision 13.1). Never on a `Fallback` subject of any cause, `NotDelivered` included, and never on a group- or world-writable mode, whatever the subjects. `Root` counts: a `4755 root:root` helper under `/opt` carries.
- **On a `ByNumber` subject** (decision 13.1), a bit that passes the rule above is kept, a stern warning row is emitted, and the file is listed in the audit report with its path, mode, uid:gid and the account name recorded at scan. The warning depends on how the number is placed:
  - `via: Provisioning`: "This file is setuid or setgid and owned by a number. The bit only does what the host intended if provisioning creates the account at that uid and gid; otherwise it runs as whoever holds that number. Confirm the fixed-id check (R11) on your install path."
  - `via: Directory`: "This file is setuid or setgid and owned by a number a directory service serves. The bit only does what the host intended if the directory serves that account at that uid and gid on every host that boots this image; otherwise it runs as whoever holds that number."
- A dropped bit (a fallback owner or group, or a group- or world-writable mode) is a row.
- **Sticky** on a regular file has no effect on Linux and is dropped.
- **setgid on a directory** carries unless its group falls back; it governs group inheritance, not privilege. **Sticky on a directory** always carries: it narrows, and dropping it from a shared writable directory is the risky direction.
- Every carried setuid or setgid file is listed in the audit report, setuid-root first; numerically owned ones are listed with the account name recorded at scan.

### 5.6 `/var` directories

**Parents.** The dedup in `discover_var_directories` (`storage.rs:225-227`) changes from "drop a proper prefix of another path" to "drop a proper prefix of another path whose ownership is `0755 root:root`". A parent that departs is kept as its own `VarDirectory`, gets its own `d` line in `inspectah-var.conf` before its leaf's (the fragment is sorted by path), and so `/var/lib/pgsql 0700 postgres:postgres` survives. This is the cheaper of the two fixes the backlog item names, and it is a tmpfiles line, not a `chown`. Decision 13.9.

**Lines, not content.** `VarDirectory.ownership` reaches the host only through `d` lines in `/usr/lib/tmpfiles.d/inspectah-var.conf`, which inspectah already writes for provisioned directories. bootc unpacks image content under `/var` only at install and never on upgrade, so ownership carried as content would reach a host once and never be corrected. A `d` line sets owner and mode on an existing directory at every boot (§ 12 Q6). `RUN mkdir -p` for a provisioned directory with no recorded mode (`mkdir_line`, `containerfile.rs:2761`) goes. Lines are `d` only: `Z` would recursively chown application data, which a migration tool has no business doing.

**Recorded and fallback lines.**

```
d /var/lib/pgsql 0700 postgres postgres -
d /var/lib/pgsql/data 0700 postgres postgres -
d /var/lib/app 0750 appsvc appsvc -
d /var/lib/dirsvc 0750 1234567 1234567 -
d /var/lib/unread :0700 :root :root -
```

- A **recorded** line enforces its owner and mode every boot, so a host's change to that directory is reverted at the next boot (§ 5.8).
- An **`Unknown`** directory gets a create-only line: the `:` prefix on mode, user and group makes each apply only when the line creates the directory. A fresh install gets `0700 root:root`, and an existing directory, whose real owner the scan could not read, is left alone. A fallback row names it. The prefix is in systemd `v252`, RHEL 9's (§ 12 Q6).
- **Owner and group fields** come from the plan's subjects. `Root`, `ByName` and `ByNumber { via: Provisioning }` write names: tmpfiles resolves a name at boot to the host's current id, so a name also survives a renumbered account, and provisioning has created local accounts before first boot. `ByNumber { via: Directory }` writes numbers: `systemd-tmpfiles-setup` can run before the directory service answers, and a name it cannot resolve makes it skip the line (*inferred*, § 12 Q9). A fallback subject writes `root` with § 5.4's bits.

`resolve_tmpfiles_ownership`'s numeric cases (`containerfile.rs:2606`; a name available but not guaranteed, or no name) go: the resolver either guarantees the account, writes a directory account's number, or falls back.

**Not closed here.** The Storage view's rows for carried directories whose ownership departs from a fixed `0755 root:root` are a presentation question waiting on the Storage section's purpose. Keeping departing parents adds rows of the same kind. That item stays open.

### 5.7 tmpfiles.d lines

Decision: an account a carried tmpfiles.d line names and the image lacks gets the account (Mark Russell, 2026-11-25). The scan collects every such name (§ 4.4), and the resolver places it like any subject:

- **A local user or group:** created by the users block under `Image`, or at install when provisioning delivers it (§ 5.1 step 4), so the line resolves at boot. No notice either way; the required-accounts list names an account provisioning creates. A local account neither delivers gets today's notice, with the delivery gap as the reason.
- **A system account:** enters the accounts block like any carried path's (§ 5.3), including a fragment whose only reference to the account is inside its content.
- **A directory account:** not created. The line is carried as the host wrote it, by name, and a row says it may be skipped at boot if the directory is not reachable yet (§ 12 Q9).
- **An uncreatable, `NotFound` or refused name:** today's notice, which `render/tmpfiles_accounts.rs` keeps for these cases alone.

### 5.8 Notices

`render/unmanaged_owners.rs` and its scan stderr line are replaced by one ownership notice built from the plan, in the audit report, the HTML report, refine's export warnings (`x-export-warnings`, which today does not carry the beta.8 notice) and `inspectah scan`'s stderr count:

- paths that fell back, with the host owner, group and mode and the cause;
- accounts the accounts block creates, pinned at the host's ids, with each one's shell and primary group;
- accounts provisioning must create, with uid **and** gid (`ByNumber { via: Provisioning }`), and directory accounts placed by number;
- local accounts that carried files name and that neither provisioning nor the users block delivers, with the gap (refused, excluded from the aggregate, no fixed id), and the paths that fell back for them;
- system users the accounts block cannot create, with the primary group that stops them;
- `sysusers.d` overrides the build writes, each saying it replaces the package's whole file as of the build, and that a host edit to it masks every later change to that file; and declarations whose file name could not be written, so no override pins them;
- setuid and setgid files carried (setuid-root first) and dropped, with a stern warning row on each one carried under a numeric owner or group;
- world-writable paths, clamped under `/etc` or carried elsewhere;
- paths with ACLs or file capabilities that are not carried, and each root (and, in an aggregate, host) where the check could not run;
- under `Image`: that local users are in the image, which is not recommended for production, and, with `preserve`, that their hashes are in an image layer.

Host-supplied text in a row (`Unnamed.host_name`, paths, ACL and capability lists) is escaped on every report surface; only counts reach the Containerfile.

Each surface that lists ownership also says, once, how it behaves after deployment:

- **`/etc` files.** The image's ownership of an `/etc` file is the default every later image updates. A host where someone changes that file's owner, group or mode keeps its own copy, content included, and takes no later image change to it, with no warning. Copying the image default back (`cp -a /usr/etc/<file> /etc/<file>`) makes it follow the image again (*inferred*, § 10.2 R13).
- **`/etc` directories.** A directory's owner and mode stop following the image on any host with a modified or added file directly inside it: the merge copies the host's directory metadata over the image's.
- **`/var` directories.** A host's change to the owner or mode of a directory a recorded `inspectah-var.conf` line names is reverted at the next boot. A create-only fallback line leaves an existing directory alone.
- **`/opt` and `/usr/local`.** Read-only on a deployed host, so the image is the only place to change them.

So a wrong owner or a fallback is fixed in the Containerfile and rebuilt before deployment, not with `chown` on hosts. This is why fallbacks are listed before the first build rather than left to the operator to find.

The Containerfile carries one comment line with the counts, pointing at the audit report. Refine shows a carried row's host ownership and clamp as read-only detail.

## 6. Field list for the convergence spec

The aggregate merge must consider exactly these snapshot keys:

1. `ownership.paths`: map keyed by image path; values `PathRecord` (`kind`, `host`, `clamp`).
2. `ownership.users`: map keyed by name; values `UserLookup` (`uid`, `primary_group`, `home`, `shell`, `source`, `declared_by`).
3. `ownership.groups`: map keyed by name; values `GroupLookup` (`gid`, `source`, `declared_by`). `declared_by` is `{ file: SysusersFile, id: SysusersId }` on users and groups alike.
4. `ownership.acl` and `ownership.capabilities`: `Detection` (`paths`, `gaps[]` with `root`, `reason`, `host`). The merge takes the union of `paths` and the concatenation of `gaps`, setting each merged gap's `host` to the host it came from. A host that degraded keeps the paths every other host found, and the notice names both. Notice only.
5. `storage.var_directories[].ownership`: `Ownership`.
6. `users_groups.local_user_delivery` (§ 0): one value per aggregate, `provisioning` unless edited.

Removed, so the merge drops them: `unmanaged_files.items[].provenance.uid`, `.gid` and `.permissions`; `storage.var_directories[].mode`, `owner_name`, `group_name`, `owner_uid`, `owner_gid`; each user's `containerfile_strategy` (§ 0).

Questions the convergence spec answers with these fields:

- **Ownership and the variant key (decision 13.3):** ownership is not part of the variant key, which stays byte-identical content (Mark Russell, 2026-11-24); it is resolved across the hosts that hold the winner. Owner and group: the most prevalent pair among them. Mode: the bitwise AND of those hosts' image modes (each host's clamp applied first), so it is never wider than any of them. When the hosts disagree on owner or group, or when a host outside the winner's pair had a narrower mode, refine flags the row and the audit report lists it (rule 8). The rejected alternative, ownership in the key, shows mode-only differences as variants and still needs the rule-8 flag, because the most prevalent variant can be the wider one.
- **An account with different ids on different hosts** (`chrony` 992 here, 994 there, normal for dynamically allocated system ids): `chown` and the `d` lines by name are unaffected. Creation pins one id: recommended, the most prevalent, with the others listed. Hosts at another id keep their `/var` data at their own number, and nothing on bootc remaps it, so the list is what tells the operator which hosts need a one-time `chown` of their data.
- **`source` disagreeing across hosts** (local on one, directory on another): the convergence spec decides; this spec's resolver takes one value.
- **`Unknown` ownership on some hosts:** the hosts with `Recorded` decide; a path `Unknown` on every host falls back.
- **Users:** the fleet users model (a superset of users across hosts, toggled with prevalence, edited down) belongs to the users respec (§ 11), not to convergence or this spec.

## 7. What refine and the exports do

- Refine applies no ownership classification and never writes `ownership`. It carries the section through the session unchanged.
- The projection adds, for each config, repository, GPG key, firewall zone, drop-in and unmanaged row, a read-only `ownership` detail looked up from `ownership.paths` by the row's path (host owner, group and mode, and the clamp). The users section carries `local_user_delivery` (§ 0). Refine's export warnings carry § 5.8's notice.
- The one ownership-affecting control is § 0's switch, plus the includes every section already has. Each changes the next render's plan as a consequence of the edit.
- An unedited export equals the tarball: both build the plan from the same snapshot through the same resolver, and both staging paths set the same modes. The parity test (§ 10.1 T12) holds this for a single host through a real session import; fleet parity is the convergence spec's.

## 8. Accounts created at provisioning

Under § 0, provisioning is the normal production path for local users, and `ByNumber { via: Provisioning }` is the normal placement for files they own (decision 13.5). The export already writes the provisioning side: the kickstart and blueprint emit every local user with the host's uid and primary gid (`render/users.rs:191-203`, `:308-320`) and custom groups at their gids.

"Every local user" means every local user the renderers can write. A user whose name, home, shell or a supplementary group `useradd_refusal` refuses gets a `# FIXME:` in both and is not created; neither is a user with no recorded id at a fixed id, nor, in a fleet aggregate, an excluded user. `ProvisioningDelivery` (§ 5.1) records each case, and the resolver places a file by number only for an account it records `AtId` the file's id. Every other local owner or group falls back to `root`, with its special bits cleared and a row naming the gap.

- Files owned by a delivered local user get a numeric `chown`, both subjects resolved separately.
- The build guards each number in both directions and in both namespaces (§ 5.3), so an image that already uses the uid, the gid or the name differently stops.
- The required-accounts list names each account with its uid and gid.
- setuid and setgid on these files are kept, with a stern warning row and an audit report listing (§ 5.5, decision 13.1); the bit does what the host intended only if provisioning creates the account at the host's uid and gid, which R11 gates.

**R11 is a gate.** Whether the image builder's blueprint and Anaconda's kickstart create the account at the fixed uid **and** its primary group at the fixed gid is *recalled* as yes, not checked. It runs on its own fixture before the scan landing (§ 14), so its answer is in before any code depends on it. If either does not, implementation stops and decision 13.5 goes back to Mark.

An account created at install by something the export does not write (a hand-written kickstart, a blueprint with no `uid`) is outside what the export can know. Numeric ownership is right only if that provisioning uses the host's ids, and the required-accounts list says so.

## 9. Changelog and docs

**§ 0 change:**

- `**Breaking:**` schema 32; re-scan earlier snapshots.
- Changed: local users are delivered by the kickstart and blueprint, every one; one switch adds them to the Containerfile for testing, with a warning that it is not for production. The per-user Containerfile strategy and the per-user include on a single host are gone.

**This spec:**

- `**Breaking:**` schema 33; re-scan earlier snapshots.
- Fixed: carried `/etc` files, from every section that carries them, keep their mode, owner and group; carried unmanaged files and directories keep owner, group and special bits; `/var` parent directories keep their ownership; tmpfiles.d lines whose account the image lacks get the account; the SSSD configuration keeps its host mode instead of a fixed `0600`.
- Changed: the beta.8 ownership notice becomes the ownership notice. System accounts that own carried files are created at the source host's ids before packages and declared in `/usr/lib/sysusers.d/inspectah-accounts.conf` unless a package declares them; a package that declares one without a fixed id gets an override in `/etc/sysusers.d` that pins it. Files owned by local users the kickstart and blueprint create are owned by number, with a build guard; files owned by a local user they do not create fall back to `root`, with a notice. `/var` directory lines name accounts (numbers for directory accounts), and a `/var` directory whose ownership the scan could not read gets a create-only tmpfiles.d line instead of `RUN mkdir`.
- Skills: `export-paths-stage-different-trees` (every staged file's mode now carried, both paths), `bootc-base-groups-and-users` (system account creation before packages with its primary-group closure, the `sysusers.d` mirror pruned by namespace, the override that pins the implied group too and freezes on a host that edits it, `COPY --chown` failing on `/usr/lib/group` accounts), `cross-inspector-shadow-passes` is unaffected. A new `carried-file-ownership` skill holds the resolver order, the `chown`-clears-setuid ordering, `chown -h` and the link refusal, the fallback table, the merge rules for files and directories, and the create-only `/var` fallback.
- User docs: the known-limitations entries beta.8 added are removed.

## 10. Testing

### 10.1 Contract tests

Table-driven, over collector-produced snapshots from the mock executor unless stated. "Local" means the users section lists the account; delivery is `Provisioning` unless stated.

| # | Case | Expected |
|---|---|---|
| T1 | `/etc/named.conf 0640 root:named`, `bind` carried, `named` not in the base | group `named` at the host gid in the accounts block, before the packages block; `chown -h root:named`; staged `0640` |
| T2 | `/etc/appsvc/secret.conf 0600 appsvc:appsvc` and `/etc/appsvc 0750`, `appsvc` a system account at 985 | accounts block lines at 985 before packages; directory `chown -h` and `chmod 0750` |
| T3 | Modified RPM config `sshd_config 0600 root:root` | staged and tar member `0600`; no `chown` line |
| T4 | `stat` fails on a file and on a directory | `Unknown`; `root:root 0600` and `0700`; rows; never `0644` |
| T5 | Orphan uid 4242 owner, group `appgrp` (system) | `chown -h root:appgrp`, special bits cleared, row |
| T6 | `/opt/app/bin/helper 4750 appsvc:appgrp`, both system | `chmod 4750` after the `chown`; setuid listed |
| T7 | `4777` file; a setuid file whose owner falls back; `4755` owned by a delivered local user; `4775` owned by a delivered local user; `4755` owned by a directory account; `4755` owned by a local user `useradd_refusal` refuses | stripped with rows on the first, second, fourth and sixth (the sixth's row names the gap); the third keeps its bit with the provisioning warning row and an audit listing (path, mode, uid:gid, account name); the fifth keeps it with the directory warning row and the same listing |
| T8 | `/etc/foo.conf 0666`; `/etc/drop 0777` directory; `/etc/spool 1777` directory; `/opt/foo 0666` | snapshot `clamp` `0664`, `0775`, `1775` with `host.mode` unchanged; image modes match; `/opt/foo` carried `0666`; rows |
| T9 | Redacted `/etc/app.conf 0640 root:app` | staged `0640` |
| T10 | `/var/lib/pgsql 0700 postgres` with leaf `data` | two `d` lines, parent first |
| T11 | tmpfiles line naming local `appuser` (1001) | `Provisioning`: no account line, no tmpfiles notice, required-accounts row; `Image`: users block creates it |
| T12 | Parity: import the snapshot into a refine session, export untouched, compare with the tarball | Containerfile, staged modes and the `ownership` section equal; § 0's value unchanged |
| T13 | Load `Mode` `"10644"`, `"644"`, `"0o644"`; an `AccountName` with a space; a `PathRecord` with an unknown `kind`; a `SysusersFileName` `"x.conf;curl x|sh;#.conf"`, `".conf"`, `"-x.conf"`, `"named"`. Scan: a host sysusers file named `x;y.conf` declaring a planned account | load errors; the scan records `SysusersFile::Refused`, the render writes no override and a row |
| T14 | SSSD name `jdoe@ad.example.com` owning a file | `Unnamed { host_name: Some(..) }`, fallback |
| T15 | Local `appuser` 1001:1001 owning `/opt/tools/report.sh` | `Provisioning`: `chown -h 1001:1001`, four guard checks, row with uid and gid; `Image`: `chown -h appuser:appuser`, no guard |
| T16 | `extract.rs` keeps a `0600` config member's mode; the refine download does too | both `0600` |
| T17 | `appsvc` created by the accounts block and declared by a carried package's `appsvc.conf` with `u appsvc -` and no `g` line, host `declared_by` `unpinned` | accounts block before packages; one override `RUN` for `appsvc.conf` that sets the uid and appends `g appsvc <gid>`; override row with the freeze text. Run against sample files in a unit test, the override's output and the pruned mirror are checked line by line |
| T17a | One package file declaring `u appsvc -` and `u appwork -:appgrp`, both planned, and `appgrp` planned with `g appgrp /etc/appgrp-owner` | one override for the file, read once: `u appsvc <uid>`, `u appwork <uid>:appgrp`, `g appgrp <gid>`, `g appsvc <gid>` appended; no pin lost |
| T17b | Mirror holds `g appsvc` and `u appsvc`; a package declares only `g appsvc`. Then a package declares `u appsvc 985:appgrp`. Then `u appsvc -` | first: the mirror keeps `u appsvc`, drops `g appsvc`; second: keeps `g appsvc`, drops `u appsvc`; third: drops both |
| T18 | `/var/lib/app 0750 appsvc:appsvc` and a `/var` directory with `Unknown` ownership | `d /var/lib/app 0750 appsvc appsvc -`; `d <path> :0700 :root :root -` and a row; no `RUN mkdir` |
| T19 | Audit report, HTML report, export warnings | each carries § 5.8's after-deployment text once, all four items |
| T20 | Unmanaged symlinks owned by `appsvc` to `/etc/crontab`, to `../lib/x`, and to a missing target | `PathRecord::Symlink`; no plan entry; no `chown` or `chmod` names them |
| T21 | A config file read through an `/etc` directory link whose target is `0750 root:named` | `PathRecord::Directory` at the link's path with the target's `0750 root:named`; `chown -h root:named` and `chmod 0750` on it; the link refusal names it, so an image holding a link there stops the build |
| T22 | An RPM-owned custom repository file `0640`, no config twin | recorded under its path; staged `0640` |
| T23 | A shadowed drop-in, and a config entry shadowed by a repository, both `0640` | recorded; the staged write carries `0640` |
| T24 | A root-owned tmpfiles fragment whose only reference to `appsvc:appgrp` is a line in its content | both in `users` and `groups`; the accounts block creates both |
| T25 | A system user whose primary gid has no name; a tmpfiles line naming an account NSS does not know | `Fallback(Uncreatable)` and `Fallback(NotFound)`; rows; no `useradd` |
| T26 | A file owned by a directory account (`Directory` source) | `ByNumber { via: Directory }`; guard; row; no accounts block or mirror line; its `/var` `d` line numeric |
| T27 | SSSD: host `sssd.conf 0400`, `conf.d/a.conf 0600` | final modes `0400` and `0600`; no `chmod 0600` line |
| T28 | Mixed subjects: local owner with system group; fallback owner with local group | `chown -h 1001:named`; `chown -h root:1001`; guards only for the numeric subject |
| T29 | An excluded config file the base also ships; a recorded directory with nothing included beneath | no plan entry for either |
| T30 | A file with host mode `0040` | staged `0440`; `chmod 0040` after the `chown` |
| T31 | `getfacl` absent; `getcap` finding a path under `/etc` and failing on `/opt`; an aggregate of a host with ACL findings and a host where `getfacl` is absent | `acl` has three gaps and no paths; `capabilities` keeps the `/etc` path with one `/opt` gap; the aggregate keeps the first host's paths and the second host's gaps with `host` set; notice rows name both |
| T32 | Directory `2775 root:appgrp`; directory `1777` under `/opt`; directory `2770` whose group falls back | `chmod 2775`; `chmod 1777`; setgid dropped with a row |
| T33 | Local `appuser` 1001, and a carried package that creates `appuser` at its own id | the guard's name check is rendered; R10 shows the build stops |
| T34 | Accounts block with a group whose gid is taken | each account its own `if`; R10 shows no `useradd` ran |
| T35 | Serde round-trips from collector-produced snapshots | `Recorded` and `Unknown`; `Named`, `Unnamed` with `host_name` absent and present; `Mode` `0000` and `7777`; all three `PathRecord` kinds; `clamp`; `Found` and `NotFound`; both `SysusersFile` and both `SysusersId` kinds; `Detection` with and without gaps, with and without `host`; map keys |
| T36 | § 0: `Provisioning` and `Image` | kickstart and blueprint hold every local user under both; the users block exists only under `Image`, with its warning; a user's `include: false` is ignored on a single host; `Image` with `preserve` adds the hash notice |
| T37 | Local `app user` home `/home/app user` (refused) owning a file; in an aggregate, local `appuser` excluded with its file included; a local user with no recorded uid | each `Fallback(NotDelivered)` with `Refused`, `Excluded` and `NoFixedId`; `chown` falls back to `root`; no guard; rows; `ProvisioningDelivery` matches the `# FIXME:` and directive lines the kickstart and blueprint render |
| T38 | System user `appsvc` whose primary group `appgrp` (system, gid 990) no carried path names; a system user whose primary group is a custom local group; one whose primary group is a directory group | `appgrp` gets a `groupadd -r -g 990` entry before `useradd -g appgrp`; the second and third are `Fallback(Uncreatable)` with `PrimaryGroupLocal` and `PrimaryGroupDirectory`, no `useradd`, rows |

### 10.2 Real-host checks

Two kinds of host. **Builds:** scan a driftify-prepared rhel9 and rhel10 VM with fixtures for each case, build the generated Containerfile with `podman build` on each release's `rhel-bootc` base, and check ownership with `stat` inside the built image. **Booted:** install the built image on a rhel9 and a rhel10 bootc host and check what only a deployed host shows: the merge, `systemd-sysusers` at boot, and the composefs mount.

| # | Checks |
|---|---|
| R1 | T1: `named` reads its config in the image (`stat` and `runuser -u named -- cat`) |
| R2 | T2: account created at 985; `getent passwd appsvc` |
| R3 | `COPY config/etc/ /etc/` onto the base's `/etc/sudoers.d` (`0750`): does the directory keep `0750` without the `chmod`? |
| R4 | T3: `sshd_config` stays `0600` |
| R5 | T6: setuid survives in the build image; reversing the order shows `chown` clears it. Booted: `stat /opt/app/bin/helper` shows `4750`, and `findmnt -no OPTIONS /` has no `nosuid` |
| R6 | T4 and T5 fallbacks |
| R7 | T11 under `Image`: `systemd-tmpfiles --create` in the built image exits 0 and the directory is owned by `appuser` |
| R8 | Booted, across a reboot: T10 and T18's recorded lines create parent and leaf; `chown root:root` on the parent is reverted at the next boot; the create-only fallback leaves an existing directory's owner and mode alone |
| R9 | A local build with `COPY --chown=root:cdrom` on each base (`cdrom` is only in `/usr/lib/group`) fails with "looking up UID/GID", while `RUN chown root:cdrom` succeeds (Q3) |
| R10 | T33 and T34: an id or name the image already uses stops the build with the message, and no later account line runs |
| R11 | **Gate**, on its own fixture: a minimal snapshot with local user `appuser` at uid 1001, primary group `appuser` at gid 1002 (so a uid-as-gid default would show), and custom group `appgrp` at gid 1050, rendered by the § 0 build, whose kickstart and blueprint already write `--uid`, `--gid` and custom group gids. Installed through the image builder's blueprint and through a kickstart, on at least one release: `getent passwd appuser` shows 1001 with gid 1002, `getent group appuser` shows 1002, `getent group appgrp` shows 1050. Repeated with the § 0 switch at `Image`: install completes and the image's account is unchanged (Q8). Needs no ownership code |
| R12 | `bootc container lint` on T2's and T17's images: no `sysusers` warning for `appsvc`, and no `/var` tmpfiles warning from inspectah's lines |
| R13 | Booted: `chown root:wheel /etc/chrony.conf`; `ostree admin config-diff \| grep chrony` shows `M chrony.conf` (Q1). Then upgrade to an image that changes both `chrony.conf` and an untouched control file: the modified file keeps the host copy, the control follows. `cp -a /usr/etc/chrony.conf /etc/chrony.conf` and upgrade again: it follows |
| R14 | Booted: `grep -c . /etc/passwd /usr/lib/passwd`; the `passwd` and `group` lines in `/etc/nsswitch.conf`; `getent passwd chrony` (Q3) |
| R15 | Booted: `ls -ld /opt /usr/local /srv /root /home` (Q5) |
| R16 | Booted: deploy `/etc/appsvc 0750 appsvc:appsvc`; on the host edit `/etc/appsvc/app.conf` and `chmod 0755 /etc/appsvc`; switch to an image that changes nothing there: `0755` stays. Then the reverse: the image changes the directory while the host has an edited file inside: the host keeps its directory metadata |
| R17 | Booted: deploy an image without `appsvc`, then `bootc switch` to T2's image; after reboot `getent passwd appsvc` shows 985 and `journalctl -b -u systemd-sysusers` has no conflict line. Repeat with an account a carried package also declares |
| R18 | T20 in a build: the link's target keeps its ownership and mode |
| R19 | Booted, with T17's image on a host whose `/etc/passwd` and `/etc/group` lack `appsvc`, on a fixture whose host uid and gid for `appsvc` differ: after reboot `getent passwd appsvc` and `getent group appsvc` show the pinned uid and gid, so the override replaced the package's file and pinned the implied group (Q7 on a host) |

## 11. Non-goals

Each left out with its cost:

- **Fleet users.** The superset of users across hosts, toggled with prevalence and edited down (Mark Russell, 2026-11-24, a leaning), belongs to the separate users respec. § 0 changes single-host users only.
- **A symlink's own owner.** A link is recreated owned by root. Its owner matters only in a sticky world-writable directory under `fs.protected_symlinks`, which this export does not reproduce.
- **ACLs.** A `setfacl` grant to a service account is lost and the file falls back to its mode bits. Usually narrower, so it breaks rather than leaks, except a default ACL on a directory. Noticed (§ 4.6), not carried.
- **Extended attributes (`user.*`).** No security effect. Not detected.
- **SELinux labels.** Not carried, by rule. ostree's merge ignores extended attributes, labels included, so a label-only change on a host neither persists across an update nor marks the file modified, and image labels come from the image's policy. A label set with `chcon` is lost; `semanage fcontext` rules belong to the SELinux inspector. Low.
- **File capabilities (`security.capability`).** A binary under `/opt` with `cap_net_bind_service` loses it and fails, or an operator re-adds setuid as a workaround. Breakage, not a leak. Noticed (§ 4.6), carried later.
- **Remapping data at a host's old id.** When the base image holds an account at a different id from the source host, or a fleet pins one id across hosts that had several (§ 6), files under `/var` keep the host's number. Nothing on bootc remaps owners. The `d` lines fix the directories themselves at boot; the data inside is listed, not changed.
- **Auditing host modes.** A host misconfiguration ships, except the `/etc` world-writable clamp (decision 13.2).

## 12. bootc questions

Answered by the bootc consult (ostree `3d72f1d`, bootc `b88f28b`, buildah `4bf2012`) and the image-mode reviews (systemd `v252`, `v257` and `main` at `a04701d5eb`, ostree `3d72f1d`), 2026-11-25. Each answer says whether it was read in source or inferred, and which § 10.2 check confirms it on a host.

- **Q1. Does an owner, group or mode change alone make an `/etc` file locally modified?** Yes, read in source, high confidence. ostree's merge (`merge_configuration_from`, `src/libostree/ostree-sysroot-deploy.c`) diffs the old image default in `/usr/etc` against the running `/etc` with a checksum whose header holds uid, gid and mode; the composefs backend's merge (`crates/etc-merge`) compares the same three. It is not a true 3-way merge: the modified file wins whole, so the new image's content and metadata for that path are dropped. A host that never touches a file follows every later image; a host `chown` freezes it with no warning. **Directories:** a modified directory alone is skipped, but `copy_modified_config_file` calls `ensure_directory_from_template` on the parent of every modified or added file, which copies the running directory's uid, gid and mode onto the new deployment's (read in source, high; reach on a host medium). Drives rule 9 and § 5.8. R13 and R16 confirm.
- **Q2. Does renumbering leave files at the old number?** Yes: neither bootc nor ostree remaps owners, and `/var` is never touched on upgrade (read in source, high confidence). An account in `/etc/passwd`, which is almost always locally modified, keeps its old number on the host while new image content carries the new one; an account in `/usr/lib/passwd` moves with the image and strands `/var` data and modified `/etc` files; a `sysusers.d` account with no fixed id can differ per host. The fix, rule 10 and § 5.3, follows bootc's documented pinning pattern (medium confidence that it is the complete fix).
- **Q3. Where do accounts live on the RHEL bootc bases, and how do the build instructions resolve them?** Groups: six in `/etc/group`, the rest in `/usr/lib/group` through nss-altfiles, measured on 9.8 and 10.2 by the users block spec. Users: the same split is *inferred*; R14 measures it. `RUN chown <name>` resolves through NSS and so finds altfiles accounts (*inferred*; R9 and R14). `COPY --chown=<name>` does not: buildah's lookup reads only the image's `/etc/passwd` and `/etc/group`, and the step fails the build (read in source, high). `systemd-sysusers` silently takes another id when the requested one is used, so it cannot enforce rule 10; `useradd` creates and a `sysusers.d` line mirrors (§ 5.3). All `sysusers.d` files sort by name across directories, the earliest entry for a name wins, and a non-equivalent duplicate logs a conflict (read in source, high), which is why a package-declared name gets no mirror line.
- **Q4. Does `/var` content in the image reach deployed hosts?** Only at install, like a `VOLUME`; upgrades never apply it (read in bootc's `bootc-filesystem.7.md`, high). `bootc container lint` warns on `/var` content with no tmpfiles.d entry. Drives rule 11 and § 5.6. R8 and R12 check the lines.
- **Q5. Are SELinux labels part of the merge, and where do `/opt` and `/usr/local` live?** ostree's merge ignores labels on purpose (read in source, high), so none are carried. `/opt` and `/usr/local` are plain image directories, read-only at runtime, and `/srv`, `/home`, `/root` and `/mnt` are links into `/var` on both RHEL bootc bases (§ 5.3 gives the sources). So setuid files under `/opt` and `/usr/local` are image content and sit in the composefs metadata. R5 and R15 re-confirm.
- **Q6. Does a tmpfiles.d `d` line correct the owner and mode of a directory that already exists?** Yes, on every `systemd-tmpfiles --create` run, read in source at `v252` and `main` (`create_directory`, `fd_set_perms`) and in `tmpfiles.d(5)`, high. The `:` prefix makes a mode, user or group field apply only at creation, also in `v252`. Drives § 5.6's create-only fallback and the `/var` line in § 5.8. R8 confirms across a reboot.
- **Q7. Does a file in `/etc/sysusers.d` replace a same-named file in `/usr/lib/sysusers.d`, so § 5.3's override pins the id?** Yes, the whole file, on RHEL 9 (systemd `v252`) and RHEL 10 (`v257`), read in source, high confidence. `systemd-sysusers` lists its files with `conf_files_list_with_replacement` over `CONF_PATHS` (`/etc`, `/run`, `/usr/local/lib`, `/usr/lib`, in that order), and `files_add` in `conf-files.c` skips any later file whose name is already listed ("Skipping overridden file"); `sysusers.d(5)` says the same at both tags. So no line of the package's file is read once the override exists, which is why the override carries every other line unchanged and states its freeze (§ 5.3). Also read there: a `u` line with no matching `g` line creates the group at the uid only if that number is free, which is why the override appends the group. R19 confirms on a host.

Still open:

- **Q8.** Do the image builder's blueprint and Anaconda's kickstart create a local user at the fixed uid and its primary group at the fixed gid, and does each tolerate a user the image already holds (§ 0 `Image`)? *Recalled* as yes. R11, a gate.
- **Q9.** Can `systemd-tmpfiles-setup` run before SSSD answers, so a directory account named in a `d` line is skipped at boot? *Inferred*; § 5.6 writes directory accounts as numbers either way. Not checked on a host: the test VMs have no directory service.
- **Q3's inferred parts**: the users split in `/usr/lib/passwd` and `RUN chown` through altfiles. R9 and R14.

## 13. Decisions

All taken by Mark Russell on 2026-11-25.

1. **setuid and setgid on files:** carry by default whenever owner and group carry, whether by root, by name or by number; on a numerically owned file the bits are kept, a stern warning row is emitted, and every such file is listed in the audit report with its path, mode, uid:gid and the account name recorded at scan (Mark Russell, 2026-11-25). Rejected: an include, which would be a new refine control and adds size; dropping the bits on numeric owners, which would make `useradd` the only way to replicate the host exactly.
2. **World-writable under `/etc`:** clamp `o+w`, with a row, for files and directories alike; carry and flag elsewhere (Mark Russell, 2026-11-25). Carry-and-flag everywhere was rejected for `/etc`.
3. **Fleet ownership:** content-only variant key, with ownership resolved separately across the winning hosts (narrowest mode, flagged when wider than a host), as § 6 (Mark Russell, 2026-11-25). Ownership in the key was rejected.
4. **Local users:** every local user always goes into the kickstart and blueprint, and none is removed from them. One section switch, `LocalUserDelivery`, optionally creates all of them in the Containerfile too, for testing and with a "not recommended for production" warning; never per user (Mark Russell, 2026-11-25). Lands first as its own change (§ 0). Rejected: the per-user Containerfile strategy, the per-user include on a single host, and the scan promoting a user that carried files need to `useradd`.
5. **Files owned by local users under `Provisioning`:** numeric `chown`, with a build guard on both uid and gid, each subject resolved independently, and a required-accounts row. This is the normal production path. R11 (the image builder and kickstart honour fixed uid and gid) is a gate (Mark Russell, 2026-11-25). Falling back to `root:root` was rejected.
6. **An account id already taken in the image by another name:** stop the build, as the users block does for groups (Mark Russell, 2026-11-25). Letting `useradd` pick an id was rejected.
7. **ACLs and file capabilities:** detect and notice now, carry later (Mark Russell, 2026-11-25).
8. **Accounts the image lacks:** system accounts are created with `useradd` at the host's id in an accounts block before packages, and mirrored in `sysusers.d` by a `RUN` after packages, at the id `getent` reports, skipping any name a `/usr/lib/sysusers.d/*.conf` already names and any account the base holds. Local users are never mirrored. Directory-service accounts are never created or mirrored and take the numeric path (Mark Russell, 2026-11-25). It is bootc's documented pinning pattern, keeps the lint quiet, and is the only option that stops a rebuild from renumbering an account. The cost is that inspectah, not the package, picks the id of a package-created account; the host's id is the one the carried files already use. Rejected: creating accounts after the users block; `systemd-sysusers` alone, which silently takes another id when the requested one is used (§ 12 Q3); a mirror staged as a fixed file, which collides with package declarations and base-held accounts.
9. **`/var` parent directories:** the dedup fix is folded in, § 5.6 (Mark Russell, 2026-11-25).
10. **A package-declared account with an unpinned id:** accepted that on a deployed host whose `/etc/passwd` lacks the account, the package's own line could allocate a different id. inspectah's fix is in the Containerfile: an override at `/etc/sysusers.d/<the package's file name>` pinning the id the image holds, with a notice row (Mark Russell, 2026-11-25). A fix on hosts was rejected: it would freeze `/etc` files against later images.

## 14. Size

**XL by task count: 22 tasks in three landings and a gate.** The size comes from a destination-indexed table across five record kinds, the provisioning delivery proof, account closure, independent subjects with guards in both namespaces, link handling, a split `FROM`, the sysusers prune and per-file overrides, the reader adaptation that lets the scan land alone, and a booted check matrix. § 0's model removed the per-user promotion and its reason row, which keeps the growth to this rather than more.

Each landing compiles, passes the workspace suite, and stands alone: nothing in it depends on a later one.

**Landing 1, § 0.**

1. **§ 0 switch:** core enum and field, schema 32, users render under `Image` only and every local user in kickstart and blueprint, refine op and route, the section control with its warning, fixture churn from per-user strategy to the section field. Small to medium on its own.

**Gate.**

2. **R11** on its own minimal fixed-id fixture (§ 10.2), rendered by landing 1's kickstart and blueprint. It needs no ownership code, so it runs before the scan landing starts, and a failure stops the spec before any of it is built.

**Landing 2, schema and scan.** After it, a render of the same host produces the same Containerfile, tarball and reports as before it, except the `/var` parents the dedup now keeps, which today's code renders as `d` lines.

3. **Core types:** `ownership.rs`, `AccountName` and `ExportToken` move with serde, `SysusersFileName`, `Destination` serde, `Detection` with gaps, section and record changes, schema 33, T13 and T35.
4. **Executor:** `uid`, `gid`, `symlink_metadata`; real, mock and machine-state gate.
5. **`HostAccounts`:** names, source, sysusers declarations with validated file names and implied groups.
6. **`OwnershipRecorder`:** five collectors and the ancestor walk (`file_metadata` above content writes), the clamp default.
7. **Account dependency pass:** tmpfiles names, primary groups.
8. **ACL and capability detection,** per root, with gaps.
9. **`/var` collector:** `Ownership` mapping and the dedup.
10. **Reader adaptation:** every reader of a removed field reads the new records and keeps today's output: `ownership_losses` and its uid/gid filter (`render/unmanaged_owners.rs`), `resolve_tmpfiles_ownership` and `mkdir_line` (`containerfile.rs`), the report's `/var` rows (`render/report.rs`), refine's projection (`projection/reference.rs`), and both web adapters (`adapter.rs`, `aggregate_handlers.rs`). The web API types do not change; the adapters fill them from `ownership`. Existing tests pass with fixture changes only.

**Landing 3, render and both exports.** It replaces the readers task 10 adapted.

11. **Resolver:** include-set join, `ProvisioningDelivery` (both provisioning renderers consume it), subjects, fallbacks, special bits, plan.
12. **Containerfile sequence:** `FROM` split, accounts block with primary-group closure, sysusers block with the namespace prune and per-file overrides.
13. **Ownership block:** guards, link refusal, `chown -h`, `chmod`; retire `sssd_mode_lines`.
14. **`/var` lines** through the resolver; retire `mkdir_line` and the numeric cases.
15. **Staging:** tarball modes, `extract.rs`, `u+r`.
16. **Refine:** export staging, projection detail, export warnings.
17. **Notices:** replace `unmanaged_owners.rs`, trim `tmpfiles_accounts.rs`, audit and HTML reports, scan stderr.
18. **Web:** row detail.
19. **Contract tests** beyond the per-task ones: T12 parity and the T17 to T38 matrix.
20. **Build checks** R1 to R10, R12 and R18.
21. **Booted checks** R5, R8, R13 to R17, R19.
22. **Changelog, skills, user docs.**
