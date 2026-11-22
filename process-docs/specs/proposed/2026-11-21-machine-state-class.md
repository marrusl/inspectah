# Machine-State Class for Configuration Files

## Metadata

| Field | Value |
|---|---|
| Date | 2026-11-21 |
| Status | Proposed. Design pass for decision 4 of the beta.7 release list (Mark Russell, 2026-11-21). Credentials are never overridable (Mark Russell, 2026-11-21). Choices marked **for the spec review** are the author's and need the review to confirm them. |
| Scope | A named class of configuration files that belong to one machine: how the snapshot carries it, the read gate every collector reads the host through and what each consumer of a read error does with a refusal, where the config inspector records it, the unmanaged walk's withheld list, snapshot validation at load and at every snapshot write, the member list, override semantics in refine and both export paths, the beta.7 fleet merge, provisioning notices, the refine API contract, how refine presents the class (§ 8, Refine presentation), and the redaction pattern's `ENCRYPTED PRIVATE KEY` gap (§ 5.3). |
| Backlog items | `inspectah-config-walk-copies-registration-and-tool-generated-state-into-the-image`, `inspectah-keytabs-ship-as-ordinary-config` (inspectah-team `workflow/backlog/inspectah/open/`) |
| Inputs | A security consult on the class list, and a UX consult on the refine presentation, both 2026-11-21. Real-host measurements in § 13, taken for this spec. |
| Verified | Code citations read at `391354ca` and rechecked at `76bcab50`, `5b041d8a` and `673fe05b` (2026-11-21; no change under `crates/` between them). Line numbers are a convenience; the symbol name is the identifier. |

**No code, documentation, or CHANGELOG entry was written to produce this file.**

## 1. Problem

The config inspector has two ways to treat a file that identifies one machine, and both are wrong.

1. **Silent exclusion.** `UNOWNED_EXCLUDE_EXACT`, `UNOWNED_EXCLUDE_GLOBS` and `UNOWNED_EXCLUDE_PREFIX` (`crates/collect/src/inspectors/config/walk.rs:44`, `:134`, `:198`) drop SSH host keys, `machine-id`, Insights registration state, RHSM consumer and entitlement certificates, NVMe host ids and `/etc/lvm/devices/*` before anything records them. An operator auditing the export cannot tell "not collected" from "never existed", and nothing tells them a provisioning step is owed.
2. **Silent carriage.** Anything the lists do not name is carried. Measured on RHEL 9.8 and 10.2 (§ 13): `/etc/krb5.keytab`, `/etc/iscsi/initiatorname.iscsi` and `/etc/grub.d/00_tuned` are all unowned and appear in no list, so the unowned walk stages them under `config/etc/` and `COPY config/etc/ /etc/` puts them in the image. RHEL 10's `/etc/authselect/switchable-auth` is the same.

The lists are also consulted only by the unowned branch. The rpm -Va branch (`config/mod.rs:121-170`) skips them by design, and `detect_orphaned_configs` (`config/mod.rs:643-710`) never calls `is_excluded_unowned`, so `/etc/rhsm/facts/insights-client.facts` is captured as `Orphaned` on a host where insights-client was removed. A parallel change, the ghost-file change, adds a fourth branch for `%ghost` files that exist on the host; § 13 measures `/etc/machine-id` and `/etc/hostname` as `%config %ghost` files of `systemd`, so that branch would collect both.

Decision 4 (Mark Russell, 2026-11-21): a named machine-state class. Files that identify one machine are shown in refine as excluded, with a visible reason the operator can override. Keytabs are in it.

## 2. Design summary

- **Two kinds of member.** A *carriable* member (host identity or generated file whose content is safe to hold) is an ordinary `ConfigFileEntry` with a new disposition, `FindingKind::MachineState { member, include }`, excluded by default and overridable. A *withheld* member (every host credential, and the identity files that a copy always breaks) is a `WithheldConfigEntry` in a new `ConfigSection::withheld` list. That type has no `content` and no `include` field, so a credential with content, or an included credential, cannot be written down.
- **One read gate.** Inspectors reach the host only through a `ScreenedExecutor`, which `InspectionContext` holds by type. Its `read_file` screens a file before opening it (by its path, the path it resolves to, and its inode against every path-matched member, listed through directory links), then sniffs a two-byte keytab header, then checks what it read for a whole-file private key. A withheld answer is an error that names the member, so no collector that reads through the gate can hold a member's bytes. The gate offers no prefix read, and a lint refuses the direct routes around it (`std::fs` opens, `Command`, a fresh `RealExecutor`); what the lint cannot see is listed in § 4.5. The config inspector's `ConfigCollector::admit`, the only way a file enters the Configuration section, turns that error into a withheld row; every other consumer of a read error is listed in § 4.5 with what it does, and none stores a row for the file. The unmanaged walk screens each regular file by path, alias, inode and content, binary-safe, and records a withheld one in `unmanaged_files.withheld` before anything renders, so no Containerfile names a payload the export does not carry and no bundle copies a credential.
- **One export boundary.** `machine_state::validate` checks the whole snapshot, every row and every variant whatever its include, against two rules: no string anywhere is a credential, and no file row in any section sits at a class path. It runs in `InspectionSnapshot::load`, in `InspectionSnapshot::to_validated_json`, which is the only way a snapshot is written (both render paths, `--inspect-only`, `aggregate --json-only`), and on refine resume. Refine refuses an `EditVariant` whose content is a credential. The stager keeps a guard of its own, across every offer kind, for renders that write no snapshot.
- **Members as data.** A rule table in `inspectah-core` maps path patterns to members; each member answers its reason, override level and provisioning notice. The identity and credential entries leave the silent lists; pure noise stays silent.
- **Override.** Carriable members are overridable at `warn` or `plain`; an override ships the captured file through the existing config tree on both export paths. Withheld members are `never`: the refine API refuses the op with 422, because there are no bytes to ship.
- **Notices.** Owed provisioning steps (`realm join`, `subscription-manager register`, ...) go to the audit report and a short Containerfile comment on both export paths, the README and stderr on the scan path, and `x-export-warnings` on the refine path, each deduplicated by step and none depending on any other section's output.
- **Fleet.** Before any section merges, the beta.7 aggregate computes the union of class paths over every host, and the generic merge drops every observation, in any section, whose destination is in it. Each class path becomes one withheld row with a host count, so no host's identity or credential reaches an aggregate, no other section offers the path, and the aggregate refine refuses the override by the same rule as a withheld row.
- **Contract.** `FindingKind` gains a variant, `ConfigSection` and `UnmanagedFileSection` each gain a withheld list, `SysctlFileBody` gains a variant, the schema version goes up, and the refine view gains a `machine_state` row list. Breaking; re-scan.

## 3. The type

### 3.1 Options

| | Option A: new disposition plus a withheld list (recommended) | Option B: side field on the entry | Option C: content enum on the entry |
|---|---|---|---|
| Carriable member | `ConfigFileEntry` with `FindingKind::MachineState { member, include }` | `ConfigFileEntry` with `machine_state: Option<Member>` and `Actionable { include: false }` | `ConfigFileEntry` with `content: ConfigContent::Captured(String)` |
| Withheld member | `WithheldConfigEntry`, no content field | same entry type, `content` empty by convention | `content: ConfigContent::Withheld { mode, size }` |
| Credential with content | not representable | representable; a test has to catch it | not representable |
| Default excluded vs operator excluded | distinct variants | `Actionable { include: false }` for both; every consumer must read a second field | needs a disposition change as well |
| Cost | one variant, one list, one struct | smallest diff | about 210 production reads of `.content` change |

Option B is rejected: it moves a "should never happen" case from the type system into data. A credential entry with non-empty content, or with `include: true`, deserializes and renders, and stats would count every class row in `excluded`, which the web disposition contract reserves for an operator's decision (`process-docs/skills/web-disposition-contract.md`). Option C is the right end state for content that may or may not exist (it would also absorb `ContentNotCaptured`) and is noted for Phase 2 in § 10; it is too wide for beta.7.

### 3.2 The types (Option A)

New module `crates/core/src/types/machine_state.rs`:

```rust
/// Why a file belongs to one machine rather than to the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineStateReason {
    /// Names this machine: machine-id, hostname, iSCSI IQN, Insights id.
    HostIdentity,
    /// Holds a key or a certificate bound to this machine.
    HostCredential,
    /// Written by a tool from settings carried elsewhere.
    Generated,
}

/// A class member whose content the snapshot may hold. Only these can
/// appear in `FindingKind::MachineState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CarriableMember {
    Hostname,
    Syspurpose,
    NvmeHostId,
    IscsiInitiatorName,
    IpaDefaultConf,
    SaltMinionId,
    AuthselectSwitchable,
    GrubTuned,
}

/// Every class member. A withheld row names one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineStateMember {
    // Withheld by rule: content never held; not opened when matched by
    // path, alias or inode (§ 4.6).
    MachineId,
    InsightsState,
    RhsmConsumer,
    RhsmEntitlement,
    LvmSystemDevices,
    KerberosKeytab,
    SshHostKey,
    SshHostCertificate,
    PrivateKeyFile,
    ConfigAgentKey,
    // Carriable members, withheld only when their read failed (§ 4.3)
    // and on a fleet aggregate (§ 6.1).
    Carriable(CarriableMember),
}

/// How refine offers to carry a member anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OverrideLevel { Never, Warn, Plain }
```

`MachineStateMember::Carriable` gets a hand-written `Serialize`/`Deserialize` that writes the inner member's snake-case name (`"iscsi_initiator_name"`), so a member is a flat string in JSON whichever enum holds it. **For the spec review:** the alternative is two independent enums with duplicated variant names and a `From<CarriableMember> for MachineStateMember`; it is simpler serde and the same type safety. Either satisfies the contract; the recommendation is the nested form because a carriable member then has exactly one spelling in Rust.

Methods, all `const fn` or plain `match` over the enum, so no member can lack an answer:

```rust
impl MachineStateMember {
    pub fn reason(self) -> MachineStateReason;
    pub fn notice(self) -> ProvisioningNotice;
}
impl CarriableMember {
    pub fn override_level(self) -> OverrideLevel; // Warn or Plain, never Never
}
```

The disposition:

```rust
pub enum FindingKind {
    Actionable { include: bool },
    Advisory { advisory_type: AdvisoryType, rationale: String },
    Inventory,
    /// A configuration file that belongs to one machine. Excluded by
    /// default; `include` is true only after an operator override.
    MachineState { member: CarriableMember, include: bool },
}
```

JSON: `{"kind":"machine_state","member":"iscsi_initiator_name","include":false}`.

`FindingKind` changes that follow:

- `is_included()` returns true for `MachineState { include: true, .. }`. Every stager already filters on `is_included()` (`configtree.rs:698`, `:998`), so an override stages with no new writer.
- `with_include(val)` sets `include` on `MachineState` when `member.override_level() != Never`, which today's members always satisfy; it keeps the variant, never converting it to `Actionable`.
- New `is_machine_state()`. `is_advisory()` and `is_inventory()` stay false for it.
- No serde default helper is needed: the field it appears in (`ConfigFileEntry::disposition`) keeps bare `#[serde(default)]`, and an absent key still means a plain included file (`process-docs/skills/finding-disposition-serde-defaults.md`). A class member is always written with the key.

The withheld row:

```rust
/// A configuration file the scan found and whose content the snapshot
/// does not hold. Never staged, never included: the type has nowhere to
/// put either.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WithheldConfigEntry {
    /// Normalized absolute path (§ 4.4).
    pub path: String,
    /// Which branch found it: unowned, orphaned, rpm_owned_modified, or
    /// the ghost branch's kind once that branch exists.
    pub kind: ConfigFileKind,
    pub package: Option<String>,
    pub member: MachineStateMember,
    /// From stat of the file the path resolves to. `None` when the stat
    /// failed.
    pub mode: Option<u32>,
    pub size: Option<u64>,
    /// Hosts that have the path, on a fleet aggregate (§ 6.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<AggregatePrevalence>,
}

pub struct ConfigSection {
    #[serde(default)]
    pub files: Vec<ConfigFileEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub withheld: Vec<WithheldConfigEntry>,
}
```

### 3.3 Provisioning notice

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProvisioningNotice {
    /// The operator runs a step when deploying a machine from the image.
    Owed { step: ProvisioningStep },
    /// Something recreates the file without the operator.
    Automatic { by: RecreatedBy },
    /// Nothing replaces it and nothing needs to.
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvisioningStep {
    RealmJoin,            // "ipa-client-install, realm join or adcli join"
    SubscriptionRegister, // "subscription-manager register or rhc connect"
    InsightsRegister,     // "insights-client --register or rhc connect"
    ResignSshHostCerts,   // "re-sign the SSH host certificates"
    DeliverPrivateKey,    // "re-issue the key, or deliver it at deploy time"
    RebootstrapConfigAgent, // "re-bootstrap the configuration management agent"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecreatedBy {
    FirstBoot,     // a unit creates it when missing
    Provisioning,  // the installer, cloud-init or DHCP sets it
    CarriedSetting, // the tool that wrote it reruns from a setting the image carries
}
```

`ProvisioningStep::command(self) -> &'static str` holds the display text above. Notices are derived from the member, never stored, so the snapshot cannot carry a notice that disagrees with its member.

## 4. Where classification happens

### 4.1 The collector

The inspector stops handing `&mut ConfigSection` to its branches. A private type in `crates/collect/src/inspectors/config/collector.rs` owns the section for the duration of `ConfigInspector::inspect`:

```rust
pub(super) struct ConfigCollector<'e> {
    exec: &'e ScreenedExecutor<'e>, // § 4.5, the read gate
    section: ConfigSection,       // private: no branch can push directly
    seen: HashSet<String>,        // normalized paths in files or withheld
    degraded: Vec<String>,
}

pub(super) struct Candidate<'a> {
    pub path: &'a str,
    pub kind: ConfigFileKind,
    pub package: Option<&'a str>,
    pub rpm_va_flags: Option<&'a str>,
}

impl ConfigCollector<'_> {
    /// The only way a file reaches the Configuration section.
    pub fn admit(&mut self, c: Candidate<'_>);
    /// For entries that carry no content by construction (cross-tree
    /// symlink advisories, modernization advisories for absent files).
    /// Still classifies, through `ScreenedExecutor::screen` and
    /// `classify_path`; a member path never becomes an advisory.
    pub fn admit_advisory(&mut self, c: Candidate<'_>, d: FindingKind);
    pub fn contains(&self, path: &str) -> bool;
    pub fn into_parts(self) -> (ConfigSection, Vec<String>);
}
```

`admit` does, in order:

1. **Read through the gate**, as `capture_config` does today but on the `ScreenedExecutor`. The gate screens, sniffs and checks before handing back any bytes (§ 4.5).
2. **Refused.** `machine_state::refusal(&err)` is `Some(member)`: `exec.file_metadata(path)` for mode and size, push a `WithheldConfigEntry` naming the member, return. For a path, alias or inode match the gate opened nothing; for a keytab header it read two bytes; for a whole-file private key it read the file and dropped it before returning.
3. **Other failure.** A carriable member is withheld (§ 4.3); anything else gets today's `ContentNotCaptured` advisory.
4. **Push.** `classify_path` gives a carriable rule: a `ConfigFileEntry` with `FindingKind::MachineState { member, include: false }`. No rule: the entry exactly as today.

Every branch (rpm -Va, unowned walk, orphan, the ostree path, and the ghost-file change's branch) takes `&mut ConfigCollector`. Each branch keeps its own skips (DHCP, dev artifacts, rpm ownership, the silent lists in the unowned walk); the class is recorded by the one function every surviving path calls. A new branch has no other way to add a file, and reads only through the gate unless it takes a route § 4.5 names.

The modernization pass (`config/mod.rs:299-315`) goes through the collector too: it skips any path in `withheld`, and `with_modernization` returns the existing disposition unchanged when it is `MachineState`. No member path matches a modernization pattern today; the rule keeps it that way.

### 4.2 Executor additions

`Executor` (`crates/core/src/traits/executor.rs`) gains one method, and a new subtrait holds the prefix read, so that only the gate can make one:

```rust
pub trait Executor {
    // ...existing methods...
    /// stat, following symlinks: permission bits, size, device, inode and
    /// link count of the file the path resolves to, without opening it.
    fn file_metadata(&self, path: &Path) -> io::Result<FileMetadata>;
}

/// A host executor the read gate can wrap. `RealExecutor` and
/// `MockExecutor` implement it; `ScreenedExecutor` does not, so no
/// inspector holds a value it can call `read_head` on.
pub trait HostRead: Executor {
    /// At most `n` bytes from the start of the file.
    fn read_head(&self, path: &Path, n: usize) -> io::Result<Vec<u8>>;
}

pub struct FileMetadata { pub mode: u32, pub size: u64, pub dev: u64, pub ino: u64, pub nlink: u64 }
```

`MockExecutor` gains byte-content files (so `read_file` on a non-UTF-8 fixture fails with `InvalidData`, as `std::fs::read_to_string` does), hard links (two paths sharing one inode), and a read log of every `read_file` and `read_head` call with its byte count, which tests use to assert that a withheld path was never opened or that only its header was. The gate (§ 4.5) reads through the wrapped executor, so its own head and full reads appear in the log.

### 4.3 Carriable members that cannot be read

A carriable member whose read fails has nothing to ship, so it is withheld: a `WithheldConfigEntry` with `member: Carriable(m)`, override `never`. The row keeps its reason and notice, which the `ContentNotCaptured` advisory it would get today does not carry. `degraded` records the failure as today.

### 4.4 Snapshot validation and render guard

Path rules match a normalized path. The lexical pass `Destination::new` performs today (drop empty and `.` components, lead with `/`, refuse `..`, NUL and control characters) moves to core as `machine_state::normal_path(&str) -> Option<String>`; `Destination::new` calls it, and `classify_path` normalizes through it before matching. `etc/ssh/ssh_host_ed25519_key`, `/etc/ssh/./ssh_host_ed25519_key` and `/etc//ssh/ssh_host_ed25519_key` are then one path to the classifier and to the stager. A path that does not normalize is refused by the stager today (`PathError`) and never written; its content is still checked below.

**The check.** `machine_state::validate(&InspectionSnapshot) -> Result<(), SnapshotError>` checks the complete snapshot. It never consults an include flag or a variant selection: an excluded row, a selected variant and an alternative are checked alike. It refuses with `SnapshotError::MachineState { path, section, why }`, naming the path and saying to re-scan, when:

1. a `config.files` entry's path classifies to a withheld rule, whatever its disposition or content;
2. a `config.files` entry's path classifies to a carriable rule and its disposition is not `MachineState` with that member, or its disposition is `MachineState` and its path does not classify to that member;
3. **any string in the snapshot is a credential.** `validate` serializes the snapshot to a `serde_json::Value` and passes every string in it, at any depth and in any section, to `credential_content` (§ 4.5): a keytab header or a whole-file private key refuses. The one subtree skipped is `subscription` (§ 4.6). Because the walk is over the serialized value, a field or section added later is checked with no list to update;
4. **a class path sits in a file row.** The class paths are the normalized paths of `config.withheld`, of `unmanaged_files.withheld` and of every `MachineState` entry. A file row is any object in that same `Value` holding a string `path` beside a string `content` or `raw_content`, which at `5b041d8a` is every row a section stores a host file's text in: config entries, unit drop-ins, repo files, GPG keys, firewall zones, quadlet units and compose files, at every variant selection. A withheld class path, or a path that classifies to a withheld rule, is the `path` of no file row in any section. A carriable class path is the `path` of exactly one file row, its own `MachineState` entry. No path is present in both `config.files` and `config.withheld`, and none appears in `config.withheld` twice. An ordinary path appearing several times within `config.files` is valid and not checked: a fleet aggregate keeps one entry per content variant at the same path.
5. **a withheld unmanaged path is also offered.** A path in `unmanaged_files.withheld` (§ 4.5, Unmanaged files) is the path of no `unmanaged_files.items` entry, and appears in `unmanaged_files.withheld` once. Unmanaged items hold no content, so rule 4 does not reach them; this rule does.

**Who calls it.** Three callers, and no snapshot is read from outside or written another way:

| Caller | What passes through it |
|---|---|
| `InspectionSnapshot::load`, after the schema check | `--from-snapshot`, refine's tarball load (through `normalize.rs`), each host snapshot `inspectah aggregate` reads |
| `InspectionSnapshot::to_validated_json(&self) -> Result<String, SnapshotError>`, which validates and then serializes | the only snapshot writer. `render::render_all` calls it first, before anything is written, and writes the string as its `inspection-snapshot.json`: the scan tarball, the aggregate tarball and `--from-snapshot` all render through it. `render_refine_export` calls it first in the same way. `run_scan`'s `--inspect-only` and `inspectah aggregate --json-only` call it in place of `serde_json::to_string_pretty`. |
| `RefineSession::resume_from`, after restoring the timeline | the autosaved session. Restore skips per-op validation (`session.rs`, "Direct restore"), so a session file can carry an op the op check (§ 8) would refuse. Resume validates the projected snapshot and refuses the session file with `RefineError::SnapshotLoad` naming the path, as it refuses a stale tarball hash. |

Since both render paths validate before writing anything, the writers that work outside `staged_writes` (the aggregate variant writer `write_aggregate_variants`, the Compose writer in `render_refine_export`, the quadlet tree) only ever see a snapshot that passed, and need no guard of their own. Every consumer of a read error is listed in § 4.5 with what it does on a refusal, and none stores a file row for a refused file, so a collector-produced snapshot passes (collect cases 1 to 12, 23, 29 and 30 assert it), and validation refuses an edited, externally produced or wrongly projected snapshot. A later consumer that maps a refusal to empty content would produce a snapshot that fails here, loudly, at the scan's own write: rule 4 stays strict rather than accepting an empty row. At a writer a failure is a defect, reported as an error naming the path and section, and no tarball is written. The refusal is never a silent repair.

**At staging.** `staged_writes` (`crates/pipeline/src/render/configtree.rs`) already withholds every offer whose `Destination` is the machine-id, on every offer kind, before any other decision. That check becomes the general one: an offer of any kind (config file, drop-in, repo file, GPG key, firewall zone, generated unit) whose `Destination` classifies to a withheld rule, equals a `config.withheld` path, or whose content `credential_content` refuses, is withheld with `WithheldWhy::MachineState { member }` (replacing `WithheldWhy::MachineId`, in `unmanaged_withheld` too), and the Containerfile names it in the existing "not copied" FIXME line. The guard serves renders that write no snapshot, such as refine's Containerfile preview. It costs one table lookup, one set lookup and the two content sniffs per offer.

### 4.5 The read gate

`ScreenedExecutor` (core, `machine_state.rs`) wraps the host executor, and it is the only executor an inspector sees:

```rust
pub struct ScreenedExecutor<'a> {
    inner: &'a dyn HostRead,
    screen: MachineStateScreen,
}

/// The keytab header the gate reads.
const KEYTAB_HEADER_LEN: usize = 2;
/// How much of an unmanaged payload the content check reads.
pub const PAYLOAD_CHECK_LEN: usize = 1 << 20;

impl<'a> ScreenedExecutor<'a> {
    /// Builds the screen: stats every host file the withheld members'
    /// Exact, Glob and Prefix rules match, listing those rules' trees
    /// through directory links, and keeps each one's (dev, ino) with its
    /// member. Reads no file content. Records a gap where it cannot list
    /// them all.
    pub fn new(inner: &'a dyn HostRead) -> Self;
    /// Where the screen could not list every matching file, if anywhere.
    pub fn gap(&self) -> Option<&ScreenGap>;
    /// Logical path, resolved path, then inode. Opens nothing. Every path
    /// is withheld while there is a gap.
    pub fn screen(&self, path: &str) -> Screened;
    /// `screen`, then the credential detectors over the file's bytes,
    /// binary-safe: the decision for an unmanaged payload.
    pub fn screen_payload(&self, path: &str) -> io::Result<Screened>;
    /// The unscreened executor, for one of the reasons the enum names.
    pub fn bypass(&self, why: ScreenBypass) -> &dyn Executor;
}

// Executor only, not HostRead: an inspector cannot ask the gate for a prefix.
impl Executor for ScreenedExecutor<'_> { /* read_file screened; the rest delegate */ }

pub enum Screened { Withheld(MachineStateMember), Carriable(CarriableMember), Ordinary }

pub enum ScreenBypass {
    /// The subscription inspector's reads of `/etc/pki/entitlement/` and
    /// `/etc/pki/consumer/cert.pem` (§ 4.6).
    SubscriptionSection,
}

/// The member a read was refused for, from an error the gate returned.
pub fn refusal(err: &io::Error) -> Option<MachineStateMember>;

/// The keytab header and whole-file key sniffs, over bytes already read.
pub fn credential_content(content: &[u8]) -> Option<MachineStateMember>;
/// The same, over the whole of a payload or its first PAYLOAD_CHECK_LEN
/// bytes and more.
pub fn payload_credential(head: &[u8], complete: bool) -> Option<MachineStateMember>;
```

`InspectionContext::executor` changes type from `&dyn Executor` to `&ScreenedExecutor`, and `pipeline::collect::collect` takes one. `run_scan` builds a single `ScreenedExecutor` around the real executor before any inspector runs and passes it to `collect` and to the scan-level helpers that take an executor (`scan_unmanaged_files`, `scan_dnf_cache_for_repoless`).

**`read_file(path)`**, for every path:

1. **Screen** (`screen`): the logical path's rule, then the resolved path's (`resolve_final_target`, which follows file and ancestor-directory symlinks), then the file's `(dev, ino)` from `file_metadata` against the screen's set. Resolution and inode add withheld answers only, and a withheld answer from any step wins. A withheld answer returns the refusal. **The file is not opened.**
2. **Header sniff:** `inner.read_head(path, KEYTAB_HEADER_LEN)`. `05 01` or `05 02` returns a `KerberosKeytab` refusal. This runs before the full read and whatever the bytes decode to: a valid keytab can be valid UTF-8 as a whole. A failed head read falls through, and the full read reports its own error.
3. **Read and check:** `inner.read_file(path)`, then `credential_content` on the result. A whole-file private key (§ 5.3) or a keytab header drops the string and returns the refusal.

`read_head` is not on `Executor`, and the gate does not implement `HostRead`, so the gate's own reads are the only prefix reads in the scan, the two-byte header read and the unmanaged payload check (Unmanaged files, below): no collector can fetch an ordinary-path key file through a prefix read that skips steps 2 and 3. `file_metadata`, `read_dir`, `read_dir_strict`, `read_link`, `resolve_final_target`, `file_exists`, `host_root` and the `run` family delegate, since none returns a file's content. A refusal is an `io::Error` of kind `Other` carrying the member; `refusal` recovers it.

The gate screens every read, not only reads under `/etc`. The rules match only `/etc` paths, so elsewhere step 1 refuses only an alias or hard link of a member; steps 2 and 3 refuse a keytab or whole-file private key wherever it is read, so no collector stores one in any section, which is what lets validation rule 3 hold every string to the same test. So `/etc/myapp/id -> /etc/machine-id` is refused as `machine_id`, a file under a symlinked directory that resolves into `/etc/pki/tls/private/` as `private_key_file`, and a hard link elsewhere to `/etc/pki/tls/private/server.key` as well, in whichever order collectors meet the two names. A carriable member is not refused: its content is safe to hold, `classify_path` recognizes it by its own path, and validation keys it to that path (§ 4.4 rule 2). A failed resolution or stat leaves its step silent, and the header sniff still runs. `BasenameSuffix` rules are not enumerated for the inode set, since that would take a second walk of `/etc`; a real keytab linked under another name is caught by its header.

**The inode inventory.** `MachineStateScreen::build` lists each withheld `Glob` and `Prefix` rule's tree from the rule's own directory and stats every regular file whose logical path, the spelling the rule matches, matches the rule. It follows directory links, so `/etc/pki/consumer/linked -> /etc/myapp/consumer` puts `/etc/myapp/consumer/cert.pem` in the set as `rhsm_consumer`, and descends only where the rule can still match, so the `krb5.keytab.*` glob lists `/etc` once and enters only directories named for it. A directory already on the current path is not entered again, which ends a link cycle. The inventory cannot be partial and still keep this section's promise: a hard link or bind mount of a file it never listed would read as ordinary. So where it cannot finish, it records a gap: a tree more than 32 directories deep, more than 65,536 entries examined in one build, an entry whose name is not UTF-8 where a rule could match it (it cannot be named to stat), or a directory that cannot be listed in full. A gate with a gap withholds every path, and `run_scan` checks for one before any inspector runs and fails with an error naming the directory and the cause, and saying to rename or remove the entry or repair the directory or link, then re-run. Withholding only below the gap was rejected: a hard link or bind mount elsewhere of a file below it would still be missed.

**Collectors.** A refusal reaches a collector as a read error. A collector that skips a file it cannot read stores nothing from it. A collector that turns a read error into empty content would store a row with the file's path and an empty `content`, which validation rule 4 refuses at a class path, so the scan's own write would fail. Every production consumer of a `read_file` error at `673fe05b` is therefore listed here, by what it does on a refusal; none stores a row for the refused file. "Degraded reason" means the text `machine-state file left out: <path>` in the inspector's degraded reasons, returned as `InspectorError::Degraded` with the partial section, as the inspectors that already degrade do.

Consumers that hold a file's raw content:

| Collector | On a refusal |
|---|---|
| Config inspector, every branch (`ConfigCollector::admit`), the ostree branch's two reads in `run_ostree_config` included: tier 1 skipped the file on a read error, tier 2 (`config/mod.rs:825`) stored `unwrap_or_default()` content | a withheld row with reason and notice (§ 4.1); `admit` replaces both reads |
| Unit drop-ins (`collect_drop_ins`), repo files and GPG keys (`rpm/repos.rs`), firewall zones (`network.rs`), quadlet units and Compose files in every search root, `/etc` included (`containers.rs`) | no row for the file, and the degraded reason. Each already skips a file it cannot read; the refusal adds the reason. |
| `.env` files (`collect_env_files`, `nonrpm.rs:1666`). Today it reads with `unwrap_or_default()` and pushes a `ConfigFileEntry` into `non_rpm_software.env_files` whatever the read returned, so `--scan-path /etc/myapp` with `.env` linked to a private key would store an empty-content row at a withheld path. | no `env_files` row and no redaction hint for the file, and the degraded reason. The non-RPM inspector, which reports only warnings today, gains a degraded-reasons list for it. Other read errors keep today's behaviour, an empty-content row at an ordinary path, which validation accepts. Collect case 29. |
| `/etc/hosts` (`collect_hosts_additions`) | no `hosts_additions`, and the degraded reason |
| sysctl (`read_sysctl_entry`) | today a read error fails the whole section. A refusal becomes a new `SysctlFileBody::Withheld { member }`: not read, still shadows by name, as `OutsideLink` does, so the section stays complete and that file's assignments are unknown, as for an outside link |

Consumers that keep a value derived from the file, or a row about something else:

| Consumer | Today on a read error | On a refusal |
|---|---|---|
| at jobs (`scan_at_jobs`, `scheduled.rs:1227`) | `unwrap_or_default()`, then an `AtJob` with an empty command | no `at_jobs` row for the file, and the degraded reason. An `AtJob` holds no `content`, so validation would accept the empty job, but a job whose command is unknown is not a finding. Collect case 30. |
| systemd timers (`scan_systemd_timers`, `scan_instance_timers`), the timer file | skipped silently | skipped, and the degraded reason. Collect case 30. |
| the same, the timer's service file (`scheduled.rs:834`, `:914`) | `unwrap_or_default()`: the timer row keeps an empty `exec_start` | the timer row stays, since the timer itself was read, with an empty `exec_start`, and the degraded reason names the service file. `SystemdTimer` holds the timer's path and no `content`. Collect case 30. |
| `/etc/locale.conf` (`kernelboot.rs:108`), `/etc/resolv.conf` provenance (`network.rs:469`), `/etc/selinux/config` policy type (`selinux.rs:151`), SELinux `file_contexts.local` and `.subs` (`selinux.rs:309`), npm `package.json` (`nonrpm.rs:1369`), git `config` remote URL (`nonrpm.rs:1737`), `pyvenv.cfg` (`nonrpm.rs:712`) | `.ok()` or a default: no locale, empty provenance, policy `targeted`, no rules from that file, no package row, an empty remote URL on the repository's row, `system_site_packages: false` | unchanged. Each stores a value parsed from the file or nothing, never the file's text in a row with its path, and a refusal leaves the value as incomplete as a permission error does today. |
| `/etc/hostname` for the subscription section's provenance (`subscription.rs`) | `.ok()` | no `source_hostname`. The read goes through the gate, not the subscription bypass, so an `/etc/hostname` linked to a member is refused like any alias; the carriable file itself reads normally |
| the empty-unit check in the unowned walk (`walk.rs:293`, `is_ok_and`) | false, so the file is left to the config walk | the same; the walk's `admit` then records the withheld row |

Every other `read_file` caller under `crates/collect/src` either skips the file (`if let Ok`, `Err(_) => continue`) or propagates the error to its section, as it does a permission error.

**How a later collector is covered.** By routing and a lint, which together cover the routes named here; they do not make every other route impossible. An inspector gets the host only from `InspectionContext`, whose field is the gate by type, and the gate offers no prefix read. The direct routes around it fail the commit gate: `crates/collect/clippy.toml` lists, under `disallowed-methods`:

- `std::fs::read`, `std::fs::read_to_string`, `std::fs::File::open`, `std::fs::OpenOptions::open` and `std::fs::copy`, the `std` calls that open a file for reading or copy one;
- `std::process::Command::new`, so a command runs only through `Executor::run`;
- `inspectah_collect::executor::real::RealExecutor::new`, so no collector builds an unscreened executor of its own.

`executor/real.rs` allows the file and command calls with its reason. No production code in `crates/collect` builds a `RealExecutor`: `run_scan`, in `crates/cli`, which that `clippy.toml` does not govern, builds the only one and wraps it at once. The constructor's allow is in `tests/executor_boundary_test.rs` and `#[cfg(test)]` code, as is the allow for the fixture helpers that read checked-in files. `disallowed_methods` is in clippy's style group, so the workspace's `-D clippy::all` fails the gate on any listed call. At `673fe05b` no production collector calls `std::fs` to read or `Command` directly, so the list costs no existing code. If clippy does not resolve the crate's own `RealExecutor::new` path, the constructor moves to `crates/cli`, where no collector can name it.

What neither covers, stated plainly: a file-opening route not on the list (a new dependency that opens files, a `libc` call), a command run through `Executor::run` that prints a file (next item), and a new consumer that maps a refusal to empty content (validation refuses its snapshot at the scan's write, § 4.4). Each is a reviewed change, not one the compiler refuses.

**What bypasses the gate**, all named:

- `ScreenBypass::SubscriptionSection`, used by the subscription inspector alone, for the subscription material it stores: the entitlement and CA directories, the consumer certificate's metadata, `rhsm.conf` and `redhat.repo`. Its hostname read goes through the gate. Its section is outside validation (§ 4.6). A new bypass is a new variant, reviewed as such.
- `Executor::run`. Commands are not file reads, and at `673fe05b` no collector runs one that prints an `/etc` file's content: `cat` reads `/proc/mounts`, `grep` prints paths (`-l`) or lines matching a fixed directive (`WorkingDirectory=`), `strings` runs on binaries outside `/etc`, and `sha256sum` in the unmanaged walk prints a hash. Collect case 24 is a regression check on today's command names, not proof against every command that could read a file.
- The scan's payload copies, which use `std::fs` in `crates/pipeline`, outside any collector: `bundle_unmanaged_files` (`render/unmanaged.rs`) copies included unmanaged files from `/opt`, `/srv` and `/usr/local` after asking the gate about each (Unmanaged files, below); `write_cached_rpms` copies package files from the dnf cache, outside the class.

**Unmanaged files.** The unmanaged walk records which files are included, and the Containerfile is rendered from that record before the bundler copies anything (`run_scan`: `render_all`, then `bundle_unmanaged_files`). So the decision to withhold a file is made in the walk and stored in the snapshot, never at copy time: a bundler that skipped a file the snapshot still lists as included would leave a `COPY` with no source, or, when the file shares a directory `COPY` with others, drop it silently while the snapshot says included.

```rust
/// An unmanaged file the read gate withheld: an alias or hard link of a
/// withheld member's file, or a keytab or whole-file private key at any
/// path. No disposition, so it cannot be included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WithheldUnmanagedEntry {
    pub path: String,
    pub member: MachineStateMember,
    pub mode: Option<u32>,
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<AggregatePrevalence>,
}

pub struct UnmanagedFileSection {
    // ...existing fields...
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub withheld: Vec<WithheldUnmanagedEntry>,
}
```

- **In the walk.** `scan_unmanaged_files` and `walk_for_unmanaged` take the `ScreenedExecutor`. For each entry the walk would record as a regular file, it calls `screen_payload` before the stat, the type check and the `sha256sum`: `screen` (path, resolved path, inode), then, for a regular file, one read of at most `PAYLOAD_CHECK_LEN` + 1 bytes judged by `payload_credential`. The bundler copies a payload byte for byte, so the walk is where its content is checked: a path and inode screen alone would let an independent keytab or private key under `/opt` or `/srv`, linked to nothing, be bundled unchanged. `Screened::Withheld(member)` pushes a `WithheldUnmanagedEntry` with mode and size from `file_metadata`, and no `items` entry; the walk continues. `total_size` and `total_count` count `items` only. Symlinks are recorded as today: the bundler recreates them as links, so they carry a target string and no payload. A file the check cannot read stays an item, and the bundler's own check fails the scan on it.
- **The content check.** `payload_credential` uses the shared detectors and is binary-safe. A keytab header (`05 01`, `05 02`) is a keytab whatever follows. A file of at most `PAYLOAD_CHECK_LEN` bytes is judged whole by `credential_content`, so bytes that are not UTF-8 are never a PEM key: an ordinary binary payload is carried, and a failed UTF-8 decode is not a credential classification. A longer file is judged by its start: it is a private key unless its first bytes rule that out, by not being UTF-8 or by holding a line no key file holds, the cut-off last line included. So a large binary or a one-line minified file stays ordinary, and a key file padded past the limit is still withheld. DER and binary PKCS#12 keys are not recognized, as on every other read.
- **Rendering.** `unmanaged_file_lines`, `bundle_unmanaged_files` and the refine export's `included_unmanaged` all read `items`. A withheld path is in none of them, so neither export emits a `COPY` for it, and the scan tarball carries no payload the refine export could extract.
- **The bundler's own check.** `bundle_unmanaged_files` (`crates/pipeline/src/render/unmanaged.rs`, called by `run_scan` with the host root `/`) takes the scan's `ScreenedExecutor` and calls `screen_payload` again on each included regular file before it copies any. A withheld answer there means the file changed after the walk, against the stable-filesystem assumption (§ 4.6), and an error means it can no longer be checked; either fails the scan with an error naming the path, and no tarball is written. It never skips the file. Checking and copying are separate opens, so a file replaced between them is not covered (§ 4.6).
- **Refine.** `SetInclude(UnmanagedFile { path })` on a withheld path returns 422 `not carried:` (`RefineError::NotCarriable`). Each withheld row appears in the view's `machine_state` list (§ 8) with `override: "never"` and no `item_id`, and its owed step (`DeliverPrivateKey` for a key) reaches every provisioning surface (§ 7). The audit report's "Machine state" table lists it (§ 6.3).
- **Validation.** Rule 5 (§ 4.4) refuses a path in both `items` and `withheld`.

### 4.6 What the design guarantees

- No section holds a withheld member's content or a whole-file credential, whichever collector reached it and by whatever name. A member matched by path, alias or inode is not opened. A keytab matched by its header is opened for two bytes. A whole-file private key is read in full by the gate and dropped before any collector sees it.
- The inode inventory lists every file a withheld `Glob` or `Prefix` rule matches, through directory links, or the scan does not run (§ 4.5, The inode inventory).
- No scan tarball carries an unmanaged keytab or whole-file private key, at any path, linked to a member or not: the walk withholds it by content, and the bundler checks again before copying.
- No snapshot is loaded, written or exported that holds a credential in any string outside the subscription section, or a file row at a class path in any section, at any include or variant selection (§ 4.4).
- No export stages a withheld path from any offer kind (§ 4.4).
- No Containerfile, on either export path, names an unmanaged payload the export does not carry: a withheld unmanaged file is recorded in the snapshot before rendering and is in no `items` entry (§ 4.5, Unmanaged files).

Not guaranteed:

- Reads by a route § 4.5 does not name. The gate and the lint cover the routes existing collectors use and the direct `std` routes around them; a new dependency, a `libc` call or a file-printing command is caught by review, not by the compiler.
- A source filesystem that changes during the scan. The design assumes it is stable while inspection runs; the bundler's check turns one change it can see into a failed scan, not a broken export. A file replaced between a check and the read or copy that follows it is not covered: that needs the check and the read on one open handle.

- `--preserve subscription --ack-sensitive` stores the RHSM entitlement and consumer certificates and keys, base64, in the snapshot's subscription section, and `write_config_tree` stages them under `subscription/` (`stage_subscription_files`). That is an acknowledged sensitive feature with its own warnings, and it stays as it is. The `rhsm_consumer` and `rhsm_entitlement` rows describe the configuration entries, not that section.
- A secret inside an ordinary file (a password in `sssd.conf`, a key block inside an application config, a key block among the host lines of `/etc/hosts`) is redaction's (§ 5.3). `hosts_additions` is outside the network redaction pass today; that belongs to `inspectah-secrets-safety-net-spec`.
- Output of a command the gate does not see, beyond what § 4.5 names.

User-facing text follows this: "contents not captured" or "left out of the configuration", never "never read".

## 5. Members as data

### 5.1 The rule table

`crates/core/src/machine_state.rs` holds the classifier, so the collector and the render guard read one table:

```rust
pub enum PathMatch {
    Exact(&'static str),
    Glob(&'static str),          // segment-wise `*`, as match_unowned_glob
    Prefix(&'static str),        // path-boundary prefix, trailing slash
    BasenameSuffix(&'static str),
}

pub enum RuleTarget { Withheld(MachineStateMember), Carriable(CarriableMember) }

pub const RULES: &[(PathMatch, RuleTarget)] = &[ /* first match wins */ ];

pub fn classify_path(path: &str) -> Option<RuleTarget>;
```

`match_unowned_glob` moves to core with it, unchanged.

### 5.2 Members

Rules in table order. "Level" is the override level; `never` means withheld (no content held, override not offered). "Notice" uses § 3.3's kinds.

| Rule | Member | Reason | Level | Notice |
|---|---|---|---|---|
| `Exact /etc/machine-id` | `MachineId` | identity | never | automatic, first boot |
| `Exact /etc/insights-client/machine-id`, `Glob /etc/insights-client/.*`, `Exact /etc/rhsm/facts/insights-client.facts` | `InsightsState` | identity | never | owed, `InsightsRegister` |
| `Glob /etc/pki/consumer/*` | `RhsmConsumer` | credential | never | owed, `SubscriptionRegister` |
| `Glob /etc/pki/entitlement/*` | `RhsmEntitlement` | credential | never | owed, `SubscriptionRegister` |
| `Glob /etc/lvm/devices/*` | `LvmSystemDevices` | identity | never | automatic, provisioning |
| `Exact /etc/krb5.keytab`, `Glob /etc/krb5.keytab.*`, `BasenameSuffix .keytab`; header sniff `05 01`/`05 02` (§ 4.5) | `KerberosKeytab` | credential | never | owed, `RealmJoin` |
| `Glob /etc/ssh/ssh_host_*-cert.pub` | `SshHostCertificate` | credential | never | owed, `ResignSshHostCerts` |
| `Glob /etc/ssh/ssh_host_*` | `SshHostKey` | credential | never | automatic, first boot (`sshd-keygen@.service`) |
| `Prefix /etc/pki/tls/private/`, `Glob /etc/cockpit/ws-certs.d/*.key`, `Prefix /etc/pki/libvirt/private/`; whole-file sniff (§ 5.3) | `PrivateKeyFile` | credential | never | owed, `DeliverPrivateKey` |
| `Prefix /etc/puppetlabs/puppet/ssl/`, `Prefix /etc/salt/pki/minion/`, `Exact /etc/chef/client.pem` | `ConfigAgentKey` | credential | never | owed, `RebootstrapConfigAgent` |
| `Exact /etc/iscsi/initiatorname.iscsi` | `IscsiInitiatorName` | identity | warn | automatic, first boot (`iscsi-init.service`) |
| `Exact /etc/salt/minion_id` | `SaltMinionId` | identity | warn | owed, `RebootstrapConfigAgent` |
| `Exact /etc/ipa/default.conf` | `IpaDefaultConf` | identity | plain | owed, `RealmJoin` |
| `Exact /etc/hostname` | `Hostname` | identity | plain | automatic, provisioning |
| `Exact /etc/rhsm/syspurpose/syspurpose.json` | `Syspurpose` | identity | plain | owed, `SubscriptionRegister` |
| `Exact /etc/nvme/hostnqn`, `Exact /etc/nvme/hostid` | `NvmeHostId` | identity | plain | automatic, first boot |
| `Exact /etc/authselect/switchable-auth` | `AuthselectSwitchable` | generated | plain | automatic, carried setting |
| `Exact /etc/grub.d/00_tuned` | `GrubTuned` | generated | plain | none |

The SSH public keys (`*.pub`) share `SshHostKey`: they are not secret, but they are useless without the private half, and `sshd-keygen` regenerates both together. The certificate rule precedes the key rule so `*-cert.pub` is not swallowed by it.

**Override levels.** Credentials are never overridable (Mark Russell, 2026-11-21): every `HostCredential` member is `never`, on a single host and on a fleet aggregate. A credential holds no content in the snapshot, so an override would have nothing to ship (§ 6.2). The security consult had recommended "extra warning" for keytabs, SSH host keys, whole-file private keys and config-management keys; that was considered and not taken. The consult's `never` list (machine-id, Insights state, RHSM consumer and entitlement, `lvm/devices`) is kept, and those members are withheld too, since content that can never be shipped need not be read. `warn` therefore applies only to carriable identity members: the iSCSI IQN, and the Salt `minion_id` (an identity file next to the agent keys, not a key).

### 5.3 Whole-file private key

`is_whole_file_private_key(content)` is true when the file is a key envelope and nothing else: every line outside a PEM block is blank, a `#` comment, or OpenSSL envelope metadata, every block's label is on the list below, and at least one block is a private key. Any other line, such as an application setting, makes the file ordinary configuration.

- **Key labels:** any label ending in `PRIVATE KEY`, which covers `PRIVATE KEY`, `ENCRYPTED PRIVATE KEY`, `RSA PRIVATE KEY`, `EC PRIVATE KEY`, `DSA PRIVATE KEY` and `OPENSSH PRIVATE KEY`. Legacy encrypted PEM headers (`Proc-Type`, `DEK-Info`) sit inside the block and need no rule.
- **Companion labels:** `CERTIFICATE`, `TRUSTED CERTIFICATE`, `X509 CRL`, and the parameter blocks `EC PARAMETERS`, `DH PARAMETERS` and `DSA PARAMETERS`. `openssl ecparam -genkey` writes `EC PARAMETERS` before `EC PRIVATE KEY` by default.
- **Envelope metadata:** the lines `openssl pkcs12` writes before each block: `Bag Attributes`, `Key Attributes` (with or without `: <No Attributes>`), their indented attribute lines (`    localKeyID: ...`, `    friendlyName: ...`), and `subject=` and `issuer=` lines.

A key embedded in a larger config file is not a member; it stays with redaction. The redaction pattern for it (`redaction/patterns.rs:43`) names only the `RSA`, `EC`, `DSA` and `OPENSSH` labels, so an embedded `ENCRYPTED PRIVATE KEY` block passes through unredacted. This change replaces that label alternation, in both the `BEGIN` and the `END` half, with the key-label rule above, any label ending in `PRIVATE KEY`: `(?:[A-Z0-9]+ )*PRIVATE KEY`. It is one line, and it makes the redaction pattern and `is_whole_file_private_key` agree on what a private key is. Render case 26 covers it.

### 5.4 The silent lists

Leave `UNOWNED_EXCLUDE_*` and become class rules:

- `UNOWNED_EXCLUDE_EXACT`: `/etc/machine-id`, `/etc/hostname`, `/etc/nvme/hostnqn`, `/etc/nvme/hostid`, `/etc/rhsm/syspurpose/syspurpose.json`, `/etc/insights-client/machine-id`, `/etc/rhsm/facts/insights-client.facts`.
- `UNOWNED_EXCLUDE_GLOBS`: `/etc/ssh/ssh_host_*`, `/etc/pki/consumer/*`, `/etc/pki/entitlement/*`, `/etc/insights-client/.*`, `/etc/lvm/devices/*`.

Stay silent (noise, not identity): `/etc/adjtime`, `/etc/localtime` (timezone, which is the locale-and-timezone item's question, not this one), `/etc/machine-info`, `/etc/.updated`, the passwd and shadow backups, `ld.so.cache`, the tuned runtime state files, alternatives, the SELinux policy store, extracted CA trust, `*.wants`/`*.requires`/`*.upholds` links, `/etc/lvm/archive/*`, `/etc/lvm/backup/*`, `/etc/pki/product/` and `/etc/pki/product-default/*.pem`, and every other current entry.

The ostree path's `volatile_names` (`hostname`, `machine-id`) stays as it is: that branch is not reached on package-based hosts (`applicable_to` is `PackageBased` only), and it goes through the collector anyway (§ 4.1).

## 6. Override semantics

### 6.1 Carriable members (`warn`, `plain`)

| Surface | Behaviour |
|---|---|
| Scan tarball, default | Not staged: `is_included()` is false. Listed with reason and notice in the audit report and README (§ 7). |
| Refine API | `SetInclude(Config { path }, include)` is accepted. `with_include` flips `include` on the `MachineState` variant. Undo, redo, autosave and reset treat it like any include; reset restores `include: false`. The server does not require a confirmation field for `warn`: the confirm dialog is the UI's (§ 8), and the refine server trusts its operator (`process-docs/skills/refine-server-trust-boundary.md`). |
| Refine export | The entry is included, so `write_config_tree` stages the captured content under `config/etc/...` and the existing `COPY config/etc/ /etc/` carries it. No new writer, no new `COPY`. A `warn` override adds one `notice` to `x-export-warnings`: `Host identity included in the image: <path>` (path through `sanitize_bounded`). |
| CLI export | `inspectah refine` saves the projected snapshot with `include: true`; rendering that snapshot (`--from-snapshot`) stages the file through the same `is_included()` filter. Both paths are covered by `copy_source_exists_test.rs`. |
| Fleet (beta.7) | Not offered. The aggregate carries every class row as a withheld row (§ 6.4), so the aggregate refine refuses `SetInclude` on it with 422 by § 6.2's rule. A fleet image carrying one host's IQN is the failure the class exists to prevent. |

### 6.2 Withheld members (`never`)

The override is not offered, and for credentials that is decided, not provisional (Mark Russell, 2026-11-21). `SetInclude(Config { path })` for a withheld path returns 422 with `error` starting `not carried:` and naming the reason (new `RefineError::NotCarriable`, mapped in `crates/web/src/error.rs` beside the two `*NotToggleable` arms). The row has no `item_id` in the view (§ 8), so a correct client never posts it.

**Why not offer an include that writes something.** Two alternatives were considered and rejected:

1. *An active `COPY` the operator must supply.* The export would emit a `COPY` with no staged source, which is the build failure `process-docs/skills/export-paths-stage-different-trees.md` exists to prevent, and it would teach operators to bake a key into an image layer anyone with pull access can read.
2. *A commented placeholder or an advisory "provision this".* That is the notice every withheld row already produces whether or not anyone clicks. A button whose only effect is text the export writes anyway is a control that does nothing.

Deploy-time delivery (a secret mount, a provisioning step) is the supported way to give a machine its credential, and every credential member has one in its notice.

### 6.3 Default render

Neither export path stages a class row by default. Both list every row, carriable and withheld, and every `unmanaged_files.withheld` row, in the audit report's Configuration section under a "Machine state" table: path, reason, notice, and for withheld rows `mode`, `size` and "contents not captured".

### 6.4 Fleet aggregate (beta.7)

The class has to win over every section, not only Configuration: a path that is a class row on one host can be an ordinary row on another host in a section that merges on its own (a unit drop-in that is a symlink to a private key on host A and a plain `[Service]` file on host B). So the class paths are settled once, before any section merges.

**The class-path union.** `merge_snapshots` (`crates/core/src/aggregate/mod.rs`) first builds `ClassPaths::from_hosts(&hosts)`: the normalized paths of every host's `config.withheld` rows, `unmanaged_files.withheld` rows and `MachineState` entries. It is built from rows, not from the rule table, so a path withheld on one host by alias or inode (`/etc/systemd/system/app.service.d/key.conf`) is in it.

```rust
/// Normalized paths that are a class row on at least one host.
pub struct ClassPaths(BTreeSet<String>);

pub trait AggregateMergeable: Clone {
    // ...existing methods...
    /// The host path this row stages or exports a file at, or `None` for a
    /// row that writes no file (a package, a module stream, a unit state).
    /// Required, with no default, so every mergeable type answers.
    fn destination(&self) -> Option<&str>;
}

pub fn merge_items<T: AggregateMergeable + Serialize>(
    observed: Vec<(usize, T)>,
    total_hosts: usize,
    hostnames: &[String],
    class: &ClassPaths,
) -> Vec<T>;
```

`merge_items` drops every observation whose `destination()` normalizes into `class` before it groups anything, so no section merged through it, the drop-ins, repo files, GPG keys, firewall zones, quadlet units, Compose files and unmanaged files included, keeps an ordinary row or a variant at a class path. Every `merge_*_sections` function takes `&ClassPaths` to pass it on, so the compiler makes every call site, current and future, supply it, and a new mergeable type cannot be written without saying whether it has a destination. At `5b041d8a` every row type that stages a file at a host path merges through `merge_items`; the rows merged by other helpers (sysctl files, manual links, users) stage none. A later one that does is caught by validation rule 4 at the export boundary and by the stager's withheld-path check (§ 4.4), whichever comes first.

**The class merge.** `merge_config_sections` takes each host's class rows (every `MachineState` entry and every `withheld` entry), plus its ordinary `config.files` observations at a class path, into a class merge; the rest goes to `merge_items`. The generic merge never sees a class path, so its `set_include(true)` on a payload every host shares cannot include one, and its content variants cannot split one into several rows.

1. **One row per path.** Group the class merge's observations by normalized path. The path is a class row in the aggregate and nothing else: no `config.files` entry and no other section's row carries it.
2. **Every row is withheld.** The merged row is a `WithheldConfigEntry`: a captured carriable observation becomes `member: Carriable(m)` with its content dropped, since every value is one host's identity, and an ordinary observation contributes its host and nothing else. Nothing in the aggregate holds content at a class path.
3. **Deterministic representative.** The member is the first member that is not `Carriable(_)` among the class observations in hostname order, else the first `Carriable(_)` one; `kind`, `package`, `mode` and `size` come from the first host in hostname order whose observation has that member.
4. **Prevalence.** `aggregate` holds the hosts that have the path in their Configuration section, class or ordinary, as `AggregatePrevalence` does for other rows.

**Unmanaged withheld rows.** Unmanaged items merge through `merge_items`, so an item at a class path is dropped there. The hosts' `unmanaged_files.withheld` rows merge by path into the aggregate's `unmanaged_files.withheld`, one row per path: member, mode and size by item 3's rule, and `aggregate` holding the hosts that have the path as an unmanaged item or a withheld row. Unmanaged roots are outside `/etc`, so no path is both a configuration and an unmanaged class path.

**Staging across offer kinds.** A merged aggregate has no row at a class path outside `config.withheld`, and validation refuses one that does (§ 4.4 rule 4). The stager also withholds any offer, of any kind, at a `config.withheld` path (§ 4.4, At staging), so a snapshot built in process with such a row stages nothing for it either.

Mixed capture success (host A read its IQN, host B could not) needs no rule of its own: both observations are class rows, and the result is one withheld row with both hosts. The view row is `override: "never"` with no `item_id`, `SetInclude` returns 422 `not carried:`, both exports stage nothing for the path, and the provisioning notice comes from the member as on a single host. The two-host drop-in case above ends the same way: one withheld row with both hosts, no `units.drop_ins` row at the path, so `SetInclude(DropIn { path }, true)` is refused with `UnknownTarget`. Phase 2 (§ 10) may give `Generated` members an ordinary merge and show per-host values; beta.7 does neither.

## 7. Provisioning notices

Built once, from the snapshot, by `render::machine_state::provisioning_steps(snap) -> Vec<(ProvisioningStep, Vec<&str>)>`: owed steps of class rows not overridden and of `unmanaged_files.withheld` rows, deduplicated by step, each with its paths, in `ProvisioningStep` declaration order. Automatic and `None` notices are not steps. An overridden carriable row owes nothing.

Every surface is written whenever a step exists, on its own, whatever else the export holds: a host whose only finding is `/etc/krb5.keytab` stages no config tree and emits no config `COPY`, and still gets every surface below.

| Surface | Export path | What it says |
|---|---|---|
| Audit report | both | New `## Provisioning steps` section in `audit-report.md`, which both exports write (`render::audit::render_audit`): one bullet per step, the command in code, the paths it covers; then one sentence that the remaining machine-state files are recreated without operator action, with a count. Paths go through the Markdown literal helper (`process-docs/skills/markdown-literal-host-text.md`). |
| Containerfile | both | Its own comment-only section, `# === Provisioning ===`, after Configuration Files, through the `section` helper: `# Not in the image: this host's identity. At provisioning:` then one line per step, `#   realm join (or ipa-client-install, adcli join)` and so on, no paths. It depends on no `COPY` line. Paths stay out so the block reads like a note a person would leave, not an inventory; the audit report has the paths. Text is enum-owned, so `comment.rs` escaping has nothing hostile to handle, and it still goes through it. |
| README | scan | The same section as the audit report, after `## Build and deploy`. The refine export does not write a README (its file set is fixed in `render_refine_export`), and this design does not add one; the audit report carries the steps there. |
| `x-export-warnings` | refine | One `notice`, after the not-carried notice: `<N> machine-state files left out. At provisioning: realm join, subscription-manager register.` Only enum text and a count, so no snapshot string enters the header and the 8 KB bound test needs no new case for it. A `warn` override adds the notice in § 6.1. |
| Scan stderr | scan | `run_scan` prints the same notice after the not-carried one, as `Notice: ...` (`process-docs/skills/refine-export-warnings-header.md`, § The scan prints the notice too). |

Config stats are computed in `crates/refine` over ordinary rows only: class rows are partitioned out before `SectionStats::from_dispositions` runs, not filtered in the web adapter, so the section's `total` and `excluded` and the export dialog's "configs excluded" count never include them.

## 8. Refine API contract

`ViewResponse` gains one list. It holds one row per class row (every `MachineState` entry and `config.withheld` row) and per `unmanaged_files.withheld` row. Class rows appear only here, never in `config_files`, and withheld unmanaged rows never in the unmanaged list, so no row is shown twice and the existing stats buckets keep their meaning.

```rust
#[derive(Serialize)]
pub struct MachineStateRowDto {
    pub path: String,
    pub member: MachineStateMember,       // "kerberos_keytab"
    pub reason: MachineStateReason,       // "host_credential"
    #[serde(rename = "override")]
    pub override_level: OverrideLevel,    // "never" | "warn" | "plain"
    pub include: bool,                    // current choice; always false for never
    pub content_captured: bool,           // false for every withheld row
    pub mode: Option<String>,             // "0600", withheld rows only
    pub size: Option<u64>,                // withheld rows only
    pub notice: ProvisioningNoticeDto,
    /// The SetInclude target. Absent exactly when override is never.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<ItemId>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProvisioningNoticeDto {
    Owed { step: ProvisioningStep, command: &'static str },
    Automatic { by: RecreatedBy },
    None,
}
```

Example rows:

```json
{"path":"/etc/krb5.keytab","member":"kerberos_keytab","reason":"host_credential",
 "override":"never","include":false,"content_captured":false,"mode":"0600","size":82,
 "notice":{"kind":"owed","step":"realm_join","command":"ipa-client-install, realm join or adcli join"}}

{"path":"/etc/iscsi/initiatorname.iscsi","member":"iscsi_initiator_name","reason":"host_identity",
 "override":"warn","include":false,"content_captured":true,"mode":null,"size":null,
 "notice":{"kind":"automatic","by":"first_boot"},
 "item_id":{"kind":"Config","key":{"path":"/etc/iscsi/initiatorname.iscsi"}}}
```

Op contract:

- `SetInclude` on a `warn` or `plain` row's `item_id`: 200, one timeline entry; the refetched row has `include` flipped. 422 `nothing to change` when it already holds the value, as for other config rows.
- `SetInclude` on a withheld path, `Config { path }` or `UnmanagedFile { path }`: 422, `error` starting `not carried:`. Nothing recorded.
- `EditVariant` (config, drop-in or quadlet) whose `content` `credential_content` refuses: 422, `error` starting `not carried:` and saying the content is a private key or keytab to deliver at deploy time. `validate_target` checks it before the op is recorded, so the timeline, the autosave and every later projection never hold it; the export boundary (§ 4.4) refuses a projection that holds one anyway, such as one restored from an edited session file. No other op carries file content. The UI sends no `EditVariant` at `5b041d8a`, so this refusal has no UI surface and Refine presentation below does not change for it.
- `SetInclude(Section(Config), ..)` (refine UI fix-up, Phase 2 of that spec) never changes a class row: section bulk ops skip `MachineState` entries the way they skip `shadowed_by` entries.
- The TypeScript `Disposition` union gains `{kind: "machine_state", member, include}`; `isIncluded` mirrors `is_included()`, and `isToggleable` returns false for it, since class rows are decided through their own list.

### Refine presentation

From the UX consult of 2026-11-21, reconciled with the contract above. The consult was written before two things were settled: every credential row is `never`, since no bytes exist to ship and credentials are never overridable (Mark Russell, 2026-11-21), and every row carries `override` and a `notice`. So the consult's warn-and-confirm flow applies only to the two `warn` rows, the iSCSI IQN and Salt `minion_id`, which are identity rows, and its dialog is reworded for identity below. Code read for this section at `e84e6f17`: `MainContent.tsx` (Configuration branch, `handleNotCarriedInclude`), `NotCarriedGroup.tsx`, `ExportDialog.tsx`, `api/exportWarnings.ts`, `GlobalSearch.tsx`, `Sidebar.tsx`, `ServiceSection.tsx` (`configPaths`), and `crates/web/src/web_types.rs` (`not_carried_notice`).

**The UI never derives a member's treatment from its path, member or reason.** The control comes from `override`, the notice line from `notice`, the metadata line from `content_captured`, `mode` and `size`. A member whose level changes on the server changes in the UI with no client release.

#### Placement

A collapsed **Machine state** block at the foot of the Configuration section, after the decision list, built like `NotCarriedGroup`: an `ExpandableSection` in a `<section id="machine-state-group">`, a help line, then the rows. It renders nothing when `machine_state` is empty.

- **Toggle text**, which carries the important facts while collapsed: `Machine state (9) · 2 host credentials · 3 provisioning steps`. The credential part counts rows with `reason: host_credential` and is left out at zero; the step part is `machine_state_steps.length` (API addition 1) and is left out at zero; `· 1 included by you` is added when any row has `include: true`.
- **Help line:** "Files that belong to this host, not to the image. inspectah leaves them out; provisioning or first boot recreates them."
- **Three sub-lists in risk order**, each an `h3` and a one-line explanation, an empty one not rendered:
  - **Host credentials**: "Keys and certificates bound to this host. Their contents are left out of the configuration."
  - **Host identity**: "Values that name this host."
  - **Generated**: "Written by a tool from settings the image already carries."
- **Rows never move.** An override changes a row's control and label in place; it stays in its sub-list and in the block, so the block is also the record of what the operator overrode.
- **The block survives the section's empty states.** Today the Configuration branch returns early for "All configuration files excluded" and for "No items match your search", which would hide the block. Both empty states render above the block instead of replacing the section. "All configuration files excluded" keeps its meaning (operator decisions over `config_files`), and class rows never count toward it.
- **Collapsed by default** in beta.7, as the consult recommends; the expanded state is kept while the section stays mounted, so an op's refetch does not collapse it.

#### Rows

Every row: the path in `code` (`overflow-wrap: anywhere`), the reason label, the notice line, and the action cell. A row with `content_captured: false` adds a muted metadata line: `mode 0600 · 82 bytes · contents not captured`, with `mode unknown` or `size unknown` where the field is null (a failed stat). Rows do not expand in beta.7.

**Reason labels**, PatternFly `Label isCompact`, no "Advisory:" prefix (that belongs to advisory rows and the advisory vocabulary item): **Host credential** (`color="orange"`, `KeyIcon`; not `LockIcon`, which `GroupRow` uses for "locked"), **Host identity** (`grey`), **Generated** (`grey`). Text and sub-list carry the distinction; colour only reinforces it. No new tokens: classes go in an `inspectah-machine-state__*` namespace mirroring `inspectah-not-carried__*`.

**The notice line**, in the `.inspectah-unit-note` style; rows with `content_captured: false` add the `--withheld` left border, which already means "the export does not write this file". Text by `notice.kind`:

| Notice | Line |
|---|---|
| `owed` | `At provisioning: <command>`, `command` verbatim as plain text. Not in `code`: some steps are prose ("re-sign the SSH host certificates"), not commands. |
| `automatic`, `first_boot` | `Recreated at first boot` |
| `automatic`, `provisioning` | `Set at provisioning by the installer, cloud-init or DHCP` |
| `automatic`, `carried_setting` | `Rewritten from a setting the image carries` |
| `none` | `Not needed in the image` |

Every row shows its notice, even when several share a step: a row reached by search or reveal has to explain itself alone. An unknown `kind` or `by` from a newer server shows `See the audit report`, never an empty line.

**States by override level:**

| Row | Action cell | Keyboard and focus | Screen reader |
|---|---|---|---|
| `plain`, `include: false` | secondary sm button **Include** | Tab reaches it; Enter or Space sends `SetInclude` with the row's `item_id` and `include: true` | "Include /etc/hostname, button", `aria-describedby` the reason label and notice line |
| `warn`, `include: false` | secondary sm button **Include…** | Enter or Space opens the confirm dialog; nothing is sent until it is confirmed | "Include /etc/iscsi/initiatorname.iscsi…, button", described by label and notice |
| `plain` or `warn`, `include: true` | green compact Label **Included by you**, then secondary sm button **Exclude**, which sends `include: false` with no dialog | focus stays on the same button while its name changes | button name becomes "Exclude …"; one polite announcement, "/etc/hostname included in the image" or "... left out of the image" |
| `never` | plain text **Not carried**, not a control | not in the Tab order | read as a statement with the row; the notice line says what replaces the file |

**A `never` row shows no disabled button or checkbox.** A disabled control reads as broken and cannot take focus to explain itself. `never` is decided by `override` alone, which covers every credential, the never identity members (machine-id, Insights state, `lvm/devices`), and a carriable member whose read failed (§ 4.3), with no branch per case.

**An included row's notice line** is replaced by its consequence, since an overridden row owes no step (§ 7): `plain` rows read `In the image, copied from this host`; `warn` rows read `In the image: every machine built from it shares this identity`.

**Requests** go through the pattern the not-carried include uses, with `item_id` passed back verbatim, guarded per `process-docs/skills/web-ui-direct-request-guard.md`: one `usePendingGuard` owned by `MainContent` (which stays mounted for the session), keyed `machine-state:<path>` for include and exclude alike, failures held in `useItemFailures` and shown on the row in a live region. While a row's request is in flight its button is disabled. Undo, redo and **Reset to defaults** need nothing new: they refetch the view, and reset puts every class row back to `include: false`. The Configuration bulk **Include all / Exclude all** never reaches class rows (§ 8 op contract), so its wording does not change.

#### The `warn` confirm dialog

PatternFly `Modal variant="small"`, the `StatsBar` reset-confirm pattern; `isDanger` on the primary button is new usage of an existing prop. Only identity rows are `warn`, since credentials are never overridable (Mark Russell, 2026-11-21), so the wording is for identity:

- Title: **Include this host's identity in the image?**
- Body: "Every machine built from this image will share this identity." Then the row's alternative, from its notice: `owed` reads "Leave it out and run this at provisioning instead: <command>."; `automatic` reads "Leave it out and it is <notice line, lower-cased>." (for the IQN, "recreated at first boot").
- Footer: primary **Include in image** with `isDanger`; **Cancel** as a link.
- Initial focus on **Cancel**. Escape or Cancel closes and returns focus to the row's **Include…** button. Confirm closes, sends the op, and returns focus to the same row's button, by then **Exclude**; if the op fails, focus still returns there and the failure shows on the row.
- No "don't ask again". The dialog per row is the point.

The dialog's wording branches on `reason`, so a future `warn` row of the `generated` reason gets its own wording rather than the identity text. A `host_credential` row never reaches the dialog.

#### Search and reveal

- **Section search** (`SectionSearch` in Configuration) matches a class row on its path, its reason label text and its notice line text. While a query is active the block expands, shows only matching rows, and hides sub-lists with none; clearing the query leaves it expanded. The result count is decision-list matches plus block matches, and "No items match your search" appears only when both are zero.
- **GlobalSearch** gets one entry per class row: section `configs`, title the path, item id `machine-state:<path>`. Paths are unique within `machine_state` (the collector's `seen` set covers both configuration lists, validation rule 5 keeps `unmanaged_files.withheld` paths unique, and unmanaged roots are outside `/etc`) and disjoint from `config_files`, so the id cannot collide with a `configs:` id.
- **Reveal** of a `machine-state:<path>` id opens Configuration, expands the block, scrolls the row into view, and focuses the row's button; a `never` row has no button, so the `<li>` takes `tabIndex={-1}` and is focused, and a screen reader reads its path, label, notice and "Not carried". The row is found by its exact test id, `machine-state-row-<path>`, compared as a string, never through a CSS selector (`web-disposition-contract.md`). Focusing the block toggle instead, as the consult suggested for `never` rows, would not say which row was meant.
- **Links from other sections.** `ServiceSection` links a unit's `required_config` path to its Configuration row only when the path is in `configPaths`, built from `config_files`. Class paths join that set and their link reveals `machine-state:<path>`, so a unit that reads `/etc/iscsi/initiatorname.iscsi` points at the row that explains why the file is out.
- **Keyboard row navigation** (the decision list's arrow and shortcut handling) does not enter the block in beta.7. The block is reached by Tab and by reveal.

#### Export dialog

- **Info alert**, `isInline isExpandable`, after the not-carried notice and built the same way, present when `machine_state` is non-empty. Title: `9 machine-state files left out · 3 provisioning steps`, counting rows with `include: false` and `machine_state_steps.length`, the step part left out at zero. Body: one item per entry of `machine_state_steps`, in its order, the command as plain text with its paths in `code` under it; then "The other files are recreated without any step." Action link **Review machine state**, which closes the dialog and reveals the block (the `handleReviewNotCarried` pattern, with its own `revealMachineState` flag in `App`).
- **Warning alert**, present when any `warn` row has `include: true`: `1 host identity file is included in the image`, then the paths. Not an acknowledgement checkbox; the row's dialog already confirmed it.
- **"N configs excluded"** stays the server's `config` stats bucket. Class rows left `config_files`, so it no longer counts them and needs no client change.
- **After download**, the warnings list drops every entry equal, by `kind` and `text`, to one in `machine_state_notices` (API addition 2), as it does for `not_carried_notice`. `withoutShownNotice` takes a list of shown notices instead of one.

#### Required API additions

The rows alone cannot give the UI two things, so the view gains two fields beside `not_carried_notice`. Both are built by the functions the export uses, over the same projected snapshot, so the dialog and the export cannot disagree:

1. **`machine_state_steps: Vec<ProvisioningStepDto>`**, `{ step, command, paths }`, from `render::machine_state::provisioning_steps` (§ 7): owed steps of rows not overridden, deduplicated, in declaration order. The UI could rebuild the deduplication from the rows, but not the order the README and Containerfile use, and the server, not a component, owns the rule for which rows owe a step.
2. **`machine_state_notices: Vec<ExportWarning>`**, the exact class entries the next export's `x-export-warnings` will carry: the left-out notice and one `Host identity included in the image: <path>` per `warn` override, after the same `sanitize_bounded`. Without it the post-download list repeats what the dialog showed, or has to match it by text pattern, which `refine-export-warnings-header.md` forbids.

Two contract clarifications, no new field: rows arrive sorted by path (the UI groups by `reason` and keeps server order within a sub-list); and on a fleet aggregate, where every class path is merged into one withheld row (§ 6.4), every `machine_state` row has `override: "never"` and no `item_id` and paths stay unique, so the same component shows no control and the UI needs no single-host or fleet branch.

**Considered:** a credential variant of the dialog, where the operator supplies the file before building; not taken, since credentials are never overridable (Mark Russell, 2026-11-21).

#### beta.7 and later

**In beta.7:** the block, its three sub-lists and fixed rows; per-row controls from `override`, with `never` rows stating their outcome; the identity confirm dialog with focus on Cancel and focus return; reason label and notice on every row, and the metadata line on rows without content; section search, GlobalSearch, reveal and `required_config` links into the block; the export dialog's info and warning alerts and the two API additions.

**Later:** content preview for captured rows; a stats-bar count for the class, designed with the stats-bucket backlog item rather than added to `excluded`; arrow-key row navigation inside the block; per-host values in the fleet view (§ 10); folding the reason labels into the advisory vocabulary item's single label set; and expanding the block by default when a credential row or an owed step exists, if a walkthrough shows operators miss it collapsed.

#### UI tests

Vitest, from view fixtures that carry the server's real row JSON (`process-docs/skills/e2e-fixtures-are-hand-maintained.md`: `machine_state` and the two additions join every single-host view fixture):

1. One row per level: `plain` shows **Include**, `warn` shows **Include…**, `never` has no button or checkbox and no tab stop.
2. `warn`: the dialog opens with focus on Cancel; Escape sends nothing and returns focus; confirm sends `SetInclude` with the row's `item_id` verbatim and focus lands on **Exclude**.
3. A row with `content_captured: false` and null `mode` shows `mode unknown` and "contents not captured".
4. "All configuration files excluded" and a search with only block matches both leave the block rendered.
5. GlobalSearch reveal of a `never` row expands the block and focuses its `<li>`.
6. The export dialog's step list follows `machine_state_steps` order, and the post-download list omits entries equal to `machine_state_notices`.
7. A row in `PendingGuards.test.tsx`: a second include while one is in flight sends nothing.

## 9. Snapshot and API changes

- `FindingKind::MachineState { member: CarriableMember, include: bool }` (new variant).
- `ConfigSection::withheld: Vec<WithheldConfigEntry>` (new field, `#[serde(default)]`, skipped when empty).
- New core types: `MachineStateReason`, `CarriableMember`, `MachineStateMember`, `OverrideLevel`, `ProvisioningNotice`, `ProvisioningStep`, `RecreatedBy`, `WithheldConfigEntry` and `WithheldUnmanagedEntry` (each with `aggregate` for fleet rows), the rule table, `classify_path`, `normal_path`, `validate`, `MachineStateScreen`, `ScreenGap`, `ScreenedExecutor`, `Screened`, `ScreenBypass`, `refusal`, `credential_content`, `payload_credential` and `ClassPaths`. `FileMetadata::is_directory`.
- `SysctlFileBody::Withheld { member }` (new variant): a sysctl file the gate refused, not read, still shadowing by name (§ 4.5).
- `SnapshotError::MachineState`: `load`, `to_validated_json` and refine resume refuse a snapshot that fails § 4.4's validation. `InspectionSnapshot::to_validated_json` is new and is the only snapshot writer.
- `Executor::file_metadata` (returning `FileMetadata`), and a new `HostRead: Executor` trait holding `read_head`, implemented by `RealExecutor` and `MockExecutor` and not by the gate. `InspectionContext::executor` becomes `&ScreenedExecutor`, and `pipeline::collect::collect` takes one. `crates/collect/clippy.toml` disallows the `std` file opens and copy, `Command::new` and `RealExecutor::new` (§ 4.5).
- Collectors (§ 4.5): `collect_env_files` and `scan_at_jobs` skip a refused file; timers record a refused timer or service file; the non-RPM inspector gains degraded reasons. `UnmanagedFileSection::withheld: Vec<WithheldUnmanagedEntry>` (new field, `#[serde(default)]`, skipped when empty), filled by the unmanaged walk through the gate.
- Aggregate: `AggregateMergeable::destination` (required), `merge_items` and every `merge_*_sections` take `&ClassPaths`, and `merge_config_sections` merges class paths into withheld rows (§ 6.4).
- Render: `WithheldWhy::MachineState { member }` replaces `WithheldWhy::MachineId`, and the stager withholds any offer at a `config.withheld` path; the audit report and Containerfile gain a provisioning section (§ 7); `bundle_unmanaged_files` moves to `crates/pipeline`, checks each payload's path and content again and fails the scan on a withheld answer or a file it cannot check; `run_scan` fails on a screen gap. Redaction: the PEM private-key pattern matches any label ending in `PRIVATE KEY` (§ 5.3).
- `SCHEMA_VERSION` and `MIN_SCHEMA` both raised by one from their values when this lands (28 at `391354ca`). A snapshot from before cannot be re-rendered correctly: its silent exclusions are gone and its carried keytab is an ordinary entry.
- Refine: `RefineError::NotCarriable` (422) for `SetInclude` on a withheld configuration or unmanaged path and for an `EditVariant` holding a credential; `ViewResponse::machine_state: Vec<MachineStateRowDto>`; class rows leave `config_files`; a new `notice` in `x-export-warnings`.
- Refine view additions the UI needs, `machine_state_steps` and `machine_state_notices`: § 8, Refine presentation, Required API additions.

CHANGELOG, under `## [Unreleased]`:

```
### Changed
- **Breaking:** Files that belong to one machine, such as keytabs, SSH host keys, private keys, RHSM and Insights registration, machine-id and the iSCSI initiator name, are no longer copied into the image or silently dropped. Refine lists them with the provisioning step each needs; credential contents are left out. Re-scan the host before refining or rendering.

### Fixed
- Redaction now removes encrypted PEM private keys (`ENCRYPTED PRIVATE KEY`) embedded in configuration files.
```

## 10. What Phase 2 inherits

Phase 2's aggregate convergence work takes `FindingKind` with four variants and the beta.7 class merge of § 6.4, which keeps no member's content in an aggregate. What Phase 2 may change: give `Generated` members an ordinary merge, since their content is derived from settings the fleet shares; show per-host values for identity members as information, never as content variants an operator can select; and keep a fleet override of a `HostIdentity` member at `never`, which makes the override level a function of (member, single host or fleet) rather than of the member alone. If Phase 2 reworks `ConfigFileEntry` content into an enum (§ 3.1 Option C), the withheld list folds into it as a `Withheld` content case and `ContentNotCaptured` stops being a disposition; the rule table, the read gate and the classification point do not change.

## 11. Tests

Built from collector-produced data: each collect case drives `ConfigInspector::inspect` through `MockExecutor` with files and rpm state at the paths, never a hand-built `ConfigFileEntry` (the lesson of 2026-10-31, where fixtures encoding layouts the collector could not produce passed three review rounds). Refine and render cases start from the snapshot those collect cases produce.

**Collect (`crates/collect/tests/config_test.rs`):** every case also asserts that `machine_state::validate` accepts the snapshot it produced.

1. **Real binary keytab, unowned.** `/etc/krb5.keytab` holding bytes `05 02` followed by non-UTF-8 key data including a distinctive marker run. Assert one `withheld` row, `member: kerberos_keytab`, `kind: unowned`, `mode` and `size` from metadata; the mock read log shows no `read_file` or `read_head` on the path (it matched by path); the serialized snapshot does not contain the marker in any encoding (raw, hex, base64).
2. **Keytab by header, either encoding.** `/etc/myapp/service.bin`, a `05 02` header and a body that is not UTF-8; `/etc/myapp/service2.bin`, a version-2 keytab whose every byte is below `0x80` (ASCII principal, synthetic 16-byte key), so the whole file is valid UTF-8. Both withheld `kerberos_keytab`; each read log shows `read_head(2)` and no `read_file`; neither key appears in the serialized snapshot.
3. **Text keytab** (the driftify fixture form, measured on both VMs in § 13): `/etc/krb5.keytab` and `/etc/krb5.keytab.ad` as UTF-8 text: both withheld by path, not opened.
4. **The unowned walk reaches a removed package's state first.** `dnf history` with `insights-client` removed and an unowned `/etc/rhsm/facts/insights-client.facts`: withheld `insights_state` with `kind: unowned`. The unowned walk admits it before orphan detection runs, and `seen` keeps the orphan pass from admitting it again.
5. **Orphan branch.** `/etc/alternatives/example.keytab`, which the retained `/etc/alternatives/*` silent glob keeps from the unowned walk, with `example` removed in `dnf history`: withheld `kerberos_keytab`, `kind: orphaned`, `package: example`; no read on the path; its marker absent from the serialized snapshot.
6. **rpm -Va branch.** `/etc/iscsi/initiatorname.iscsi` reported modified with flags `S.5....T.  c`: `MachineState { iscsi_initiator_name }` with `kind: rpm_owned_modified`.
7. **Ghost branch** (lands with the ghost-file change, or with this change if that one is first): `/etc/machine-id` owned by `systemd` with `%config %ghost` flags and present: withheld `machine_id`, not opened. `/etc/hostname`, same flags: `MachineState { hostname }`.
8. **Unreadable carriable member.** `/etc/iscsi/initiatorname.iscsi` with permission denied: withheld row, `member: iscsi_initiator_name`, override `never` in the view; `degraded` records it.
9. **Whole-file private key**, from files the tools produced, checked in as fixtures: `openssl genpkey` (`PRIVATE KEY`), `openssl pkcs8 -topk8` with a passphrase (`ENCRYPTED PRIVATE KEY`), `openssl ecparam -genkey` (`EC PARAMETERS` then `EC PRIVATE KEY`), `openssl pkcs12 -nodes` output (`Bag Attributes`, a key and certificates), the same with an encrypted key, and a certificate followed by its key. Each at `/etc/myapp/<name>.pem`: withheld `private_key_file`, content absent from the snapshot. Negatives: a certificate-only `.pem` and an `app.conf` with a key block among other settings stay ordinary entries (the second is redaction's case).
10. **Aliases.** `/etc/myapp/tls.key`, a symlink to `/etc/pki/tls/private/server.key`: withheld by resolved path, not opened under either name. `/etc/myapp/keys`, a directory symlink to `/etc/pki/tls/private/`: every file the walk finds under it withheld. `/etc/myapp/server.key`, a hard link to `/etc/pki/tls/private/server.key`: withheld by inode, not opened, with the mock listing the two directories in both orders. `/etc/myapp/id`, a symlink to `/etc/machine-id`: withheld `machine_id`.
11. **A second collector.** `/etc/systemd/system/app.service.d/key.conf`, a symlink to an `ENCRYPTED PRIVATE KEY` file in `/etc/pki/tls/private/`, through the full scan with `UnitsInspector`: no `units.drop_ins` row for it, a degraded reason naming it, a withheld config row; the mock read log shows no read of either name; the snapshot and both exports hold no key bytes.
12. **Modernization does not replace the class.** A test-only modernization pattern on a member path leaves `MachineState` in place.

**Core (`crates/core`):**

13. **Unrepresentable credential with content.** JSON `{"kind":"machine_state","member":"kerberos_keytab","include":true}` fails to deserialize into `FindingKind`. `WithheldConfigEntry` deserialization ignores a stray `content` key and the struct has no field to hold it (assert the re-serialized JSON has no `content`).
14. Round trips for every member; JSON without `withheld` gives an empty list; `is_included` and `with_include` on `MachineState`.
15. **Rule table invariants.** No `UNOWNED_EXCLUDE_EXACT` entry (exported for the test) classifies to a rule, and no `Exact`, `Glob` or `Prefix` rule's path is matched by a retained silent glob or prefix. `BasenameSuffix` rules are exempt by design: a `.keytab` in a silent directory reaches the collector through the orphan branch (case 5). Every `MachineStateMember` and `CarriableMember` variant is the target of at least one rule; the certificate rule precedes the key rule; every carriable member's level is not `Never`; `normal_path` agrees with `Destination::new` on every path in the configtree tests.
16. **Validation at load**, on JSON edited from the snapshots of cases 1, 6 and 9, as a hand-edited snapshot would be: a `config.files` entry at `etc/ssh/ssh_host_ed25519_key`, `/etc/ssh/./ssh_host_ed25519_key` and `/etc//ssh/ssh_host_ed25519_key`, each once included and once excluded; an included and an excluded ordinary entry at `/etc/myapp/server.pem` holding case 9's `EC PARAMETERS` file; a `units.drop_ins` row whose content is a key; `MachineState { hostname }` at `/etc/myapp/x`; one path in both `files` and `withheld`; one path twice in `withheld`. Then, on JSON edited from the snapshots of cases 23, 27 and 28: a sysctl file's `body.content` holding case 9's encrypted key; a Compose file's `raw_content` holding it; an excluded `config.files` alternative and an excluded drop-in holding it; a `units.drop_ins` row, ordinary content, at a `config.withheld` path. `load` refuses each with `SnapshotError::MachineState` naming the path, and `to_validated_json` refuses the same values built in process. Accepted: the unedited snapshots, and an ordinary multi-variant aggregate, two hosts whose `/etc/myapp/app.conf` differ, which repeats the path within `config.files` as a selected entry and an alternative.

**Refine and render:**

17. **Credential override refused** (`crates/refine/tests/session_test.rs`, `crates/web/tests/api_test.rs`): from case 1's snapshot, `SetInclude(Config { "/etc/krb5.keytab" }, true)` returns `Err(NotCarriable)` and `POST /api/op` 422 with `error` starting `not carried:`; `/api/ops` unchanged; the view row has `override: "never"` and no `item_id`; the export has no `config/etc/krb5.keytab` and its `x-export-warnings` carries the provisioning notice naming `realm join`.
18. **Carriable override ships the file on both paths.** From case 6's snapshot: `SetInclude` on the IQN; the refine export stages `config/etc/iscsi/initiatorname.iscsi` with the captured content and carries the `warn` notice; the saved snapshot rendered through the CLI path stages the same bytes; `copy_source_exists_test.rs` passes on both; undo removes it from the next export.
19. **Keytab-only host, both paths.** Case 1's snapshot, whose only finding is the keytab, through `render_all` and through the refine export: no `config/` tree and no config `COPY`; the Containerfile has the `# === Provisioning ===` section naming `realm join` with no paths; `audit-report.md` has `## Provisioning steps`; the scan path's README has it too and the scan prints the notice on stderr; the refine export's `x-export-warnings` carries it. Then cases 1 and 4 together: `realm join` and `insights-client --register` once each on every surface.
20. **Render guard.** A snapshot built in process, never through `load`, with `/etc/ssh/./ssh_host_ed25519_key` as an included `Actionable` entry with content, an included `/etc/myapp/server.pem` holding a key, and an included ordinary drop-in at a path in its `config.withheld`: `staged_writes` withholds all three with `WithheldWhy::MachineState`, and the Containerfile preview names them in its "not copied" lines. `render_all` and `render_refine_export` on the same snapshot refuse with `SnapshotError::MachineState` and write nothing.
21. **Fleet aggregate**, from collector-produced host snapshots through `inspectah aggregate`: two hosts with the same IQN; two hosts with different IQNs; host A's IQN captured and host B's unreadable; host A's `/etc/myapp/server.pem` a key and host B's an ordinary config. Each gives one `withheld` row for the path with both hosts in `aggregate`, the path in no `files` entry, and no member content anywhere in the aggregate snapshot; the aggregate view row is `override: "never"` with no `item_id`; `SetInclude` returns 422; both exports stage nothing for it.
22. **View contract.** `contract_snapshots.rs` gains a `machine_state` list with one row per level, accepted with only that delta and the class rows' departure from `config_files`.

**The read gate (`crates/collect/tests/config_test.rs`, `crates/collect/tests/screen_test.rs`):**

23. **Three more collectors.** Full scans through `collect`, each with a fixture key carrying a marker run, through both exports. `/etc/sysctl.d/90-local.conf`, a symlink to case 9's `ENCRYPTED PRIVATE KEY` file at `/etc/pki/tls/private/server.key`: its `kernel_boot.sysctl.files` entry has body `withheld` with `member: private_key_file`, the sysctl section is present and not failed, and the config walk records a withheld row. `/etc/myapp/compose.yaml`, a symlink to the same file: no `compose_files` row, a degraded reason naming it, a withheld config row. `/etc/hosts`, a symlink to the same file, with `rpm -Va` reporting it changed: no `hosts_additions`, a degraded reason, a withheld config row with `kind: rpm_owned_modified`. For each, the mock read log shows no `read_file` or `read_head` of either name; the serialized snapshot, every file `render_all` writes and every file in the refine export tarball contain the marker in no encoding (raw, base64, hex). Then the same three paths as regular-file copies of the key: each refused by the whole-file check (the read log shows the read), with the same snapshot and export assertions.
24. **Commands print no `/etc` file**, a regression check on today's command names (§ 4.5). A full mock scan's run log holds no `cat`, `head`, `tail`, `base64`, `xxd`, `od`, `dd` or `strings` call with an argument under `/etc`.
25. **The gate itself**, over `MockExecutor`: `refusal` recovers the member from each refusal kind; a withheld path's read log shows no read, and a keytab-header refusal's shows one `read_head` of exactly `KEYTAB_HEADER_LEN` bytes; `/opt/app/creds.pem`, a whole-file key outside `/etc`, is refused by content; `/opt/app/k`, a hard link to `/etc/pki/tls/private/server.key`, is refused by inode and not opened; a carriable member's path reads normally. `bypass(ScreenBypass::SubscriptionSection)` reads `/etc/pki/entitlement/` and the subscription section equals today's. A compile-fail doctest shows `read_head` cannot be called on a `ScreenedExecutor`. The unmanaged export of `/opt/app/k` is case 31.

**Export boundary and fleet (`crates/refine`, `crates/web`, `crates/core`, `crates/pipeline`):**

26. **Embedded encrypted key redacted.** An `app.conf` with settings around an `ENCRYPTED PRIVATE KEY` block (case 9's negative) is redacted by the PEM pattern; the existing `RSA`, `EC`, `DSA`, `OPENSSH` and unlabeled cases still are, and a `CERTIFICATE` block is not.
27. **Edit, then select a variant, from certificate-only inputs.** Two collector-produced hosts whose unowned `/etc/myapp/server.pem` hold different certificates and no key (case 9's negative), aggregated: two variants at the path. `EditVariant(Config { path }, content = case 9's encrypted key)` returns `Err(NotCarriable)` and `POST /api/op` 422 with `error` starting `not carried:`; `/api/ops` and the autosave file are unchanged and hold no key bytes. A session file edited to carry that op makes `resume_from` refuse with `RefineError::SnapshotLoad` naming the path. A projection holding the edit, built in process, in each of three states (the edited variant selected and included; selected and excluded; an alternative after `SelectVariant` back to an original host's content): `render_refine_export` and `render_all` refuse with `SnapshotError::MachineState`, and nothing is written, `aggregate/variants/` included.
28. **Two-host drop-in.** Both hosts scanned with `UnitsInspector`. Host A, a full scan: unowned `/etc/systemd/system/app.service.d/key.conf`, a symlink to `/etc/pki/tls/private/server.key`. Host B, a full scan: the same path is a regular drop-in holding `[Service]` and `Environment=ROLE=worker`. `app.service` at its preset state on both. Host B's snapshot has the drop-in as a `units.drop_ins` row before the merge. Through `inspectah aggregate`: one `config.withheld` row for the path with both hosts in `aggregate`; no `config.files` entry and no `units.drop_ins` row at it, selected or alternative; `ROLE=worker` nowhere in the aggregate snapshot. `SetInclude(DropIn { path }, true)` is refused with `UnknownTarget` and nothing is recorded. Neither export writes a file for the path, and neither Containerfile names it in a `COPY`.

**Read-error consumers and unmanaged withholding (`crates/collect/tests/config_test.rs`, `crates/collect/tests/screen_test.rs`, `crates/cli`, `copy_source_exists_test.rs`):**

29. **`.env` under an explicit scan root.** A complete scan through `collect` with the non-RPM roots of `inspectah scan --scan-path /etc/myapp`: an unowned `/etc/myapp/.env`, a symlink to `/etc/pki/tls/private/server.key`, which holds case 9's `ENCRYPTED PRIVATE KEY` file with a marker run. Assert: exactly one `config.withheld` row for `/etc/myapp/.env`, `member: private_key_file`; no `non_rpm_software.env_files` row and no redaction hint for the path; the non-RPM inspector's degraded reasons name it; the read log shows no read of either name. `machine_state::validate` accepts the snapshot, `to_validated_json` succeeds (the `--inspect-only` write), and `render_all` and the refine export succeed; the marker appears in no encoding in the snapshot or either export. Rule 4 is unchanged: the same snapshot edited to add an empty-content `env_files` row at the path is refused at load.
30. **The other rewritten consumers.** One complete scan: `/var/spool/at/a00001`, a hard link to the key above, gives no `at_jobs` row and a degraded reason; `/etc/systemd/system/backup.timer`, a symlink to the key, gives no `systemd_timers` row and a degraded reason; `/etc/systemd/system/sync.timer`, readable, with `sync.service` a symlink to the key, keeps its timer row with an empty `exec_start` and a degraded reason naming the service file. Validation accepts the snapshot, and the marker is absent from it and from both exports.
31. **Unmanaged withholding through both exports.** Collect half, through `MockExecutor`, `--include-unmanaged`: `/opt/app/k`, a hard link to `/etc/pki/tls/private/server.key`, and beside it `/opt/app/app.conf` and an ordinary binary `/opt/app/run`; an independent binary keytab `/opt/myapp/client.kt` and a whole-file encrypted PKCS#8 key `/opt/myapp/client.pem`, linked to nothing, beside `app.conf` and an ordinary binary `lib.so`; an OpenSSH key `/srv/data/id_ed25519` beside an ordinary file. Each `/opt` directory falls under one directory `COPY`. The snapshot has one `unmanaged_files.withheld` row per credential (`private_key_file`, `kerberos_keytab`), no `items` entry for any, and every ordinary file as an included item; validation accepts it, and the gate never opens the hard link. Render half, in `crates/cli/tests/machine_state_scan_test.rs`: `render_all` on that snapshot, then `bundle_unmanaged_files` copying each remaining item from a host root holding every file, the credentials included; the scan tarball packed from that tree; then the refine export from the tarball. On both: each `/opt` directory `COPY` is emitted, the Containerfile names no credential in any instruction, every `COPY` source exists in the staged tree (`copy_source_exists_test.rs`'s check), each ordinary file, binary included, is staged byte for byte, and the key marker is in no artifact, raw, hex or base64. The refine view's `machine_state` row for `/opt/app/k` has `override: "never"` and no `item_id`, `SetInclude(UnmanagedFile { path }, true)` returns `NotCarriable` and records nothing, and the audit report and `x-export-warnings` carry `DeliverPrivateKey`. Last, the bundler (`crates/pipeline/src/render/unmanaged.rs` tests), screening through a `MockExecutor`-backed gate and given an `items` entry at a hard link to a key, a keytab, a whole-file key or a file it cannot read, fails naming the path and writes nothing.
32. **Inode inventory through links and depth** (`screen_test.rs`, `machine_state_scan_test.rs`). Through a full scan: a certificate-only `/etc/myapp/consumer/cert.pem` behind `/etc/pki/consumer/linked`, registration JSON behind `/etc/insights-client/.state`, and a hard link `/etc/myapp/ent.json` to a file eleven directories below `/etc/pki/entitlement/`: each a withheld row with the rule's member, not opened, its content in no artifact. A link back up the tree ends the walk. Gaps, over the gate: a tree 40 directories deep, a name that is not UTF-8 under `/etc/pki/tls/private/`, and an entitlement directory whose listing fails part way each leave a gap naming the directory, and the gate then withholds and does not open an ordinary path; a name that is not UTF-8 in `/etc` where no rule can match leaves none.
33. **Subscription hostname through the gate.** With the subscription inspector, `/etc/hostname` a symlink to `/etc/ssh/ssh_host_ed25519_key`: `source_hostname` is unset, neither name is opened, and the snapshot holds no key text.
34. **RPM refusal consumers.** `/etc/yum.repos.d/linked.repo`, a symlink to a key, and `app.repo` naming `gpgkey=file:///etc/pki/tls/private/server.key`: the RPM section has `app.repo` and no row for either refused file, is reported degraded, and each path is a `machine-state file left out:` reason; neither name is opened.
35. **The refine API over HTTP** (`crates/web/tests/machine_state_api_test.rs`), from collector-built sessions through the real router: cases 17, 21, 27 and 31's refusals each return 422 `not carried:` and leave `/api/ops` unchanged; `SetInclude(Config { path }, false)` on a default-excluded IQN and a repeated include after a successful one return 422 `nothing to change:` with `/api/ops` unchanged, and the include between them records one op.
36. **Notice parity beyond `warn`.** A collected `plain` member that owes a step (`/etc/rhsm/syspurpose/syspurpose.json`), included in refine: both paths stage the captured bytes, every `COPY` source exists, and the step and any `warn` notice are absent from every surface (both Containerfiles and audit reports, the scan README and notices, the refine export's warnings). A host whose class files are all automatic (machine-id, an SSH host key, the hostname, an IQN): no provisioning section or notice on any of those surfaces, and the audit report still lists the rows.

## 12. Strongest case against

The class is wider than "files that name one machine", and the `never` level for credentials narrows the decision's "the operator can override". Some sites do bake shared SSH host keys behind a VIP, or a shared service keytab, into an image on purpose, and this design gives them no path from refine; they must deliver the file at deploy time. That is decided (Mark Russell, 2026-11-21: credentials are never overridable), but an operator who expects the override will see a row they cannot change.

Second, name-matched rules (`*.keytab`, `/etc/pki/tls/private/`) and the whole-file key sniff will catch files an administrator placed on purpose. The visible withheld row is what makes that acceptable; the silent form would not be.

Third, the protection has costs of its own. `load` and every snapshot write refuse a snapshot edited into a forbidden form rather than repairing it, so a user of another tool, or of a refine session file edited by hand, stops at an error. The gate adds a path resolution, a stat and a two-byte read to every file read in the scan, not only to configuration candidates, and validation serializes the snapshot to a JSON value once per write. Changing `InspectionContext::executor` to the gate's type touches every test that builds a context (about 200 sites, each a mechanical two-line change), and `merge_items` gains a parameter at its 44 call sites. The alias, inode, gate and class-path union are more machinery than the ordinary named-path case needs; they exist because each list the earlier drafts kept, of names, of collectors or of sections, had a route it did not name.

Fourth, the read gate covers the routes it names, not every route. A collector that opens a file through a new dependency or a `libc` call, or runs a command that prints one, is caught by review, and the lint list has to grow with each such route (§ 4.5).

What would change this: evidence that operators bake shared host credentials into images at scale, which would be a reason to revisit the credential decision, and then to look at a deploy-time secret mount stanza rather than a `COPY`; a real binary keytab carried byte for byte by today's code (raises urgency, not design); `ipa-client-install` succeeding cleanly over a carried `/etc/ipa/default.conf` (drops its owed notice to `none`).

## 13. Provenance

**Code** read at `391354ca`: `config/mod.rs` (branches, `capture_config`, `detect_orphaned_configs`, `run_ostree_config`), `config/walk.rs` (the three lists, `is_excluded_unowned`), `core/types/finding.rs`, `core/types/config.rs`, `refine/src/session.rs` (`SetInclude(Config)` validation and projection), `web/src/web_types.rs` (`ConfigFileDto`), `core/snapshot.rs` (`SCHEMA_VERSION` 28), `core/traits/executor.rs` (no metadata or byte read today), and the skills cited inline.

Read at `76bcab50`: `executor/real.rs` (`read_file` reads the whole file before the UTF-8 check fails; `resolve_final_target` is `canonicalize`), `config/walk.rs` (`walk_recursive_inner` follows directory symlinks that stay inside `/etc`), `config/mod.rs` (`detect_orphaned_configs` skips paths already captured and needs the removed package's name in the basename), `units/mod.rs` (`collect_drop_ins` reads each `.conf` itself), `subscription.rs` and `configtree.rs` (`stage_subscription_files`; `Destination`; `staged_writes` withholding the machine-id on every offer), `aggregate/merge.rs` (`merge_items`, `merge_with_variants`, `merge_config_sections`), `core/snapshot.rs` (`load`), `refine/src/session.rs` (`render_refine_export`'s file set, no README), `refine/src/types.rs` (`SectionStats::from_dispositions`), `containerfile.rs` (`section`, config `COPY` per staged root), `redaction/patterns.rs` (the PEM pattern).

Read at `5b041d8a`: `core/traits/executor.rs` and `core/traits/inspector.rs` (`InspectionContext` holds `&dyn Executor`), `pipeline/src/collect.rs` (`collect` builds the only production contexts), `executor/real.rs` (`host_root` is `/`, so collectors' paths are host paths), every `read_file` caller under `crates/collect/src` (no production `std::fs` read; `run` callers: `cat` on `/proc/mounts` only, `grep` printing paths or `WorkingDirectory=` lines, `strings` on binaries), `kernelboot.rs` (`read_sysctl_entry` reads targets under `/etc` and fails the section on a read error), `containers.rs` (`find_compose_files` keeps `raw_content`), `network.rs` (`collect_hosts_additions` keeps raw lines; no redaction pass names `hosts_additions`), `subscription.rs` (reads `/etc/pki/entitlement/` and `/etc/pki/consumer/cert.pem`, stores base64 with `path` and `content`), `cli/src/commands/scan.rs` (`bundle_unmanaged_files` copies with `std::fs`; `--inspect-only` serializes directly), `cli/src/commands/aggregate.rs` (`--json-only` serializes directly), `pipeline/src/render/mod.rs` (`render_all` serializes at step 7), `refine/src/session.rs` (`validate_target` ignores `EditVariant` content; `resume_from` restores the timeline without per-op validation; `render_refine_export` serializes the projection as-is and writes Compose files and `write_aggregate_variants` outside `staged_writes`), `refine/src/autosave.rs` (saves ops, not the snapshot), `aggregate/mod.rs` and `aggregate/merge.rs` (`merge_snapshots`; drop-ins, quadlets, Compose files, repo files and firewall zones each merge through their own `merge_items` call; 22 `AggregateMergeable` impls), `configtree.rs` (`offers`, `StagedFileKind`), and the web UI (no client sends `EditVariant`).

Read at `673fe05b`: every production `read_file` caller under `crates/collect/src` by what it does with an error (§ 4.5's two tables: `unwrap_or_default()` in `collect_env_files`, `run_ostree_config` tier 2, `scan_at_jobs` and the two timer service reads; `.ok()` or a default in the derived-value readers; every other caller skips or propagates), `nonrpm.rs` (`collect_env_files` pushes its row whatever the read returned; `walk_for_unmanaged` admits by metadata, hashes with `sha256sum`, and walks `/opt`, `/srv` and `/usr/local` only; the inspector reports warnings and no degraded reasons), `scheduled.rs` (`AtJob` and `SystemdTimer` hold no `content`), `cli/src/commands/scan.rs` (`run_scan` calls `render_all` before `bundle_unmanaged_files`; the only production `RealExecutor::new`), `pipeline/src/render/unmanaged.rs` (`unmanaged_file_lines` filters by inclusion and `unmanaged_withheld` only), `refine/src/session.rs` (`included_unmanaged` from `items`), `refine/src/types.rs` (`ItemId::UnmanagedFile`), and `crates/collect/src` for `std::fs`, `OpenOptions` and `Command::new` outside `executor/real.rs` (test code only).

**Real hosts**, measured for this spec on 2026-11-21, read-only (`rpm -qf`, `rpm -q --qf '[%{FILENAMES} %{FILEFLAGS:fflags}\n]'`, `stat`, `file`, `systemctl cat`, `rpm -q --scripts`), on the test VMs `rhel9` (RHEL 9.8) and `rhel10` (RHEL 10.2), both aarch64 and carrying an earlier drift-tool run, which is where the keytabs come from. Flags: `c` config, `n` noreplace, `g` ghost.

| Path | RHEL 9.8 | RHEL 10.2 |
|---|---|---|
| `/etc/machine-id` | `systemd`, `cng`, 0444 | `systemd`, `cng`, 0444 |
| `/etc/hostname` | `systemd`, `cng` | `systemd`, `cng` |
| `/etc/ipa/default.conf` | absent; ipa-client not installed; in `ipa-client-common`'s file list (cached repo metadata, flags not visible) | absent; not in cached metadata |
| `/etc/pki/tls/private/localhost.key` | absent, mod_ssl not installed, directory empty | same |
| `/etc/iscsi/initiatorname.iscsi` | **unowned**, 0600; `iscsi-init.service` has `ConditionPathExists=!` this path and runs `iscsi-gen-initiatorname` | same |
| `/etc/krb5.keytab`, `/etc/krb5.keytab.ad` | unowned, 0644, UTF-8 text (drift fixture) | same |
| `/etc/insights-client/machine-id` | unowned | unowned |
| `/etc/grub.d/00_tuned` | unowned, 0755; tuned's scriptlet copies it from `/usr/share/tuned/grub2/` and removes it on uninstall | same |
| `/etc/tuned/{active_profile,profile_mode,bootcmdline,post_loaded_profile}` | `tuned`, `cn` (owned, not ghost) | same |
| `/etc/lvm/devices/system.devices` | unowned | unowned |
| `/etc/nvme/hostnqn`, `hostid` | unowned | unowned |
| `/etc/rhsm/syspurpose/syspurpose.json` | unowned | unowned |
| `/etc/ssh/ssh_host_*` | unowned | unowned |
| `/etc/authselect/switchable-auth` | absent (authselect 1.2.6) | **unowned** (authselect 1.5.2) |
| `/etc/authselect/{authselect.conf,nsswitch.conf,system-auth,password-auth,...}` | `authselect-libs`, `g` (ghost, not config) | `authselect-libs`, `g`; on 10 also `/etc/nsswitch.conf` and `/etc/pam.d/*-auth` |
| `/etc/sssd/sssd.conf` | `sssd-common`, `cng` | `sssd-common`, `cng` |

What this corrects in the consult's recall: the iSCSI IQN is unowned, not package-owned, so today's unowned walk carries it into the image (no list names it), which is a live defect this change closes rather than an rpm -Va corner case. `/etc/machine-id` and `/etc/hostname` are `%config %ghost`, so they are invisible today and reach the inspector only through the ghost-file change's branch, which is why § 4 puts the class in the collector. `ipa/default.conf` and `localhost.key` could not be measured on hosts without their packages; the ghost claims for them remain recalled. No real binary keytab exists on either VM. The design does not depend on how one decodes, since the gate's header check precedes the full read (§ 4.5); tests 1 and 2 cover both encodings.
