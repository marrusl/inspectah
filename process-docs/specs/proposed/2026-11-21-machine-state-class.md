# Machine-State Class for Configuration Files

## Metadata

| Field | Value |
|---|---|
| Date | 2026-11-21 |
| Status | Proposed. Design pass for decision 4 of the beta.7 release list (Mark Russell, 2026-11-21). Choices marked **for the spec review** are the author's and need the review to confirm them. |
| Scope | A named class of configuration files that belong to one machine: how the snapshot carries it, where the config inspector classifies it and how other collectors defer to it, snapshot validation, the member list, override semantics in refine and both export paths, the beta.7 fleet merge, provisioning notices, the refine API contract, and how refine presents the class (§ 8, Refine presentation). |
| Backlog items | `inspectah-config-walk-copies-registration-and-tool-generated-state-into-the-image`, `inspectah-keytabs-ship-as-ordinary-config` (inspectah-team `workflow/backlog/inspectah/open/`) |
| Inputs | A security consult on the class list, and a UX consult on the refine presentation, both 2026-11-21. Real-host measurements in § 13, taken for this spec. |
| Verified | Code citations read at `391354ca` and rechecked at `76bcab50` (2026-11-21; no change under `crates/` between them). Line numbers are a convenience; the symbol name is the identifier. |

**No code, documentation, or CHANGELOG entry was written to produce this file.**

## 1. Problem

The config inspector has two ways to treat a file that identifies one machine, and both are wrong.

1. **Silent exclusion.** `UNOWNED_EXCLUDE_EXACT`, `UNOWNED_EXCLUDE_GLOBS` and `UNOWNED_EXCLUDE_PREFIX` (`crates/collect/src/inspectors/config/walk.rs:44`, `:134`, `:198`) drop SSH host keys, `machine-id`, Insights registration state, RHSM consumer and entitlement certificates, NVMe host ids and `/etc/lvm/devices/*` before anything records them. An operator auditing the export cannot tell "not collected" from "never existed", and nothing tells them a provisioning step is owed.
2. **Silent carriage.** Anything the lists do not name is carried. Measured on RHEL 9.8 and 10.2 (§ 13): `/etc/krb5.keytab`, `/etc/iscsi/initiatorname.iscsi` and `/etc/grub.d/00_tuned` are all unowned and appear in no list, so the unowned walk stages them under `config/etc/` and `COPY config/etc/ /etc/` puts them in the image. RHEL 10's `/etc/authselect/switchable-auth` is the same.

The lists are also consulted only by the unowned branch. The rpm -Va branch (`config/mod.rs:121-170`) skips them by design, and `detect_orphaned_configs` (`config/mod.rs:643-710`) never calls `is_excluded_unowned`, so `/etc/rhsm/facts/insights-client.facts` is captured as `Orphaned` on a host where insights-client was removed. A parallel change, the ghost-file change, adds a fourth branch for `%ghost` files that exist on the host; § 13 measures `/etc/machine-id` and `/etc/hostname` as `%config %ghost` files of `systemd`, so that branch would collect both.

Decision 4 (Mark Russell, 2026-11-21): a named machine-state class. Files that identify one machine are shown in refine as excluded, with a visible reason the operator can override. Keytabs are in it.

## 2. Design summary

- **Two kinds of member.** A *carriable* member (host identity or generated file whose content is safe to hold) is an ordinary `ConfigFileEntry` with a new disposition, `FindingKind::MachineState { member, include }`, excluded by default and overridable. A *withheld* member (every host credential, and the identity files that a copy always breaks) is a `WithheldConfigEntry` in a new `ConfigSection::withheld` list. That type has no `content` and no `include` field, so a credential with content, or an included credential, cannot be written down.
- **One classification point.** A `ConfigCollector` owns the section during inspection, and its `admit` method is the only way any branch adds a file. `admit` screens each file before a full read: by its path, by the path it resolves to, and by inode against every path-matched member, then by a two-byte keytab header; only then does it read, and a whole-file private key is dropped after the read. Collectors outside the config inspector that store a file's raw content run the same screen and leave a member's content out.
- **Validation.** `InspectionSnapshot::load` refuses a snapshot whose configuration, or any other raw-content field, holds a member's path (in any spelling) or a credential's content, so an edited or externally produced snapshot cannot reintroduce what the collector withholds. The stager keeps a guard of its own for snapshots built in process.
- **Members as data.** A rule table in `inspectah-core` maps path patterns to members; each member answers its reason, override level and provisioning notice. The identity and credential entries leave the silent lists; pure noise stays silent.
- **Override.** Carriable members are overridable at `warn` or `plain`; an override ships the captured file through the existing config tree on both export paths. Withheld members are `never`: the refine API refuses the op with 422, because there are no bytes to ship.
- **Notices.** Owed provisioning steps (`realm join`, `subscription-manager register`, ...) go to the audit report and a short Containerfile comment on both export paths, the README and stderr on the scan path, and `x-export-warnings` on the refine path, each deduplicated by step and none depending on any other section's output.
- **Fleet.** The beta.7 aggregate merges every class row into one withheld row per path with a host count, so no host's identity or credential reaches an aggregate and the aggregate refine refuses the override by the same rule as a withheld row.
- **Contract.** `FindingKind` gains a variant, `ConfigSection` gains a list, the schema version goes up, and the refine view gains a `machine_state` row list. Breaking; re-scan.

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
    exec: &'e dyn Executor,
    screen: &'e MachineStateScreen, // § 4.5, built once per scan
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
    /// Still classifies; a member path never becomes an advisory.
    pub fn admit_advisory(&mut self, c: Candidate<'_>, d: FindingKind);
    pub fn contains(&self, path: &str) -> bool;
    pub fn into_parts(self) -> (ConfigSection, Vec<String>);
}
```

`admit` does, in order:

1. **Screen before reading** (`MachineStateScreen::before_read`, § 4.5): the normalized path, then the path it resolves to (`exec.resolve_final_target`, which follows file and ancestor-directory symlinks), then the file's device and inode against every path-matched withheld member on the host. A withheld match: `exec.file_metadata(path)` for mode and size, push a `WithheldConfigEntry` naming the matched member, return. **The file is not opened.**
2. **Header sniff**, for every candidate step 1 did not withhold, carriable ones included: `exec.read_head(path, 2)`. `05 01` or `05 02` is a keytab header, so push a withheld `KerberosKeytab` row and return. This runs before the full read and whatever the bytes decode to: a valid keytab can be valid UTF-8 as a whole.
3. **Read**, as `capture_config` does today. On failure, a carriable member is withheld (§ 4.3) and anything else gets today's `ContentNotCaptured` advisory. On success, `is_whole_file_private_key(&content)` (§ 5.3) pushes a withheld `PrivateKeyFile` row and drops the string.
4. **Push.** A carriable rule from step 1: a `ConfigFileEntry` with `FindingKind::MachineState { member, include: false }`. No rule: the entry exactly as today.

Every branch (rpm -Va, unowned walk, orphan, the ostree path, and the ghost-file change's branch) takes `&mut ConfigCollector`. Each branch keeps its own skips (DHCP, dev artifacts, rpm ownership, the silent lists in the unowned walk); the class is applied by the one function every surviving path calls. A new branch has no other way to add a file, which is the "by construction" the class needs: forgetting to classify would mean bypassing a private field.

The modernization pass (`config/mod.rs:299-315`) goes through the collector too: it skips any path in `withheld`, and `with_modernization` returns the existing disposition unchanged when it is `MachineState`. No member path matches a modernization pattern today; the rule keeps it that way.

### 4.2 Executor additions

`Executor` (`crates/core/src/traits/executor.rs`) gains:

```rust
/// stat, following symlinks: permission bits, size, device, inode and
/// link count of the file the path resolves to, without opening it.
fn file_metadata(&self, path: &Path) -> io::Result<FileMetadata>;
/// At most `n` bytes from the start of the file.
fn read_head(&self, path: &Path, n: usize) -> io::Result<Vec<u8>>;

pub struct FileMetadata { pub mode: u32, pub size: u64, pub dev: u64, pub ino: u64, pub nlink: u64 }
```

`MockExecutor` gains byte-content files (so `read_file` on a non-UTF-8 fixture fails with `InvalidData`, as `std::fs::read_to_string` does), hard links (two paths sharing one inode), and a read log of every `read_file` and `read_head` call with its byte count, which tests use to assert that a withheld path was never opened or that only its header was.

### 4.3 Carriable members that cannot be read

A carriable member whose read fails has nothing to ship, so it is withheld: a `WithheldConfigEntry` with `member: Carriable(m)`, override `never`. The row keeps its reason and notice, which the `ContentNotCaptured` advisory it would get today does not carry. `degraded` records the failure as today.

### 4.4 Snapshot validation and render guard

Path rules match a normalized path. The lexical pass `Destination::new` performs today (drop empty and `.` components, lead with `/`, refuse `..`, NUL and control characters) moves to core as `machine_state::normal_path(&str) -> Option<String>`; `Destination::new` calls it, and `classify_path` normalizes through it before matching. `etc/ssh/ssh_host_ed25519_key`, `/etc/ssh/./ssh_host_ed25519_key` and `/etc//ssh/ssh_host_ed25519_key` are then one path to the classifier and to the stager. A path that does not normalize is refused by the stager today (`PathError`) and never written; its content is still checked below.

**At load.** `InspectionSnapshot::load` is the one way a snapshot enters from outside: `--from-snapshot`, refine's tarball load (through `normalize.rs`), and each host snapshot `inspectah aggregate` reads. After the schema check it calls `machine_state::validate(&snap)`, which refuses the snapshot with `SnapshotError::MachineState { path, why }`, naming the path and saying to re-scan, when:

1. a `config.files` entry's path classifies to a withheld rule, whatever its disposition or content;
2. a `config.files` entry's path classifies to a carriable rule and its disposition is not `MachineState` with that member, or its disposition is `MachineState` and its path does not classify to that member;
3. a raw-content field holds a credential, included or not: the content starts with a keytab header or passes `is_whole_file_private_key` (§ 5.3). The fields are `config.files` and those § 4.5 screens (`units.drop_ins`, `rpm.repo_files`, `rpm.gpg_keys`, `network.firewall_zones`, the quadlet files in `containers`), and rule 1's path check applies to each of them too;
4. one normalized path appears twice across `config.files` and `config.withheld`.

The collector never produces any of these, so validation refuses only an edited or externally produced snapshot, and the refusal is an error, never a silent repair. `run_scan` runs the same `validate` over the snapshot it is about to write: a failure there is a collector that skipped the screen, reported as an error naming the section, never a written tarball. The subscription section is outside validation by design (§ 4.6).

**At staging.** `staged_writes` (`crates/pipeline/src/render/configtree.rs`) already withholds every offer whose `Destination` is the machine-id, on every offer kind, before any other decision. That check becomes the general one: an offer whose `Destination` classifies to a withheld rule, or whose content is a credential by rule 3, is withheld with `WithheldWhy::MachineState { member }` (replacing `WithheldWhy::MachineId`, in `unmanaged_withheld` too), and the Containerfile names it in the existing "not copied" FIXME line. The guard covers snapshots built in process, which never pass through `load`. It costs one table lookup and the two content sniffs per staged file.

### 4.5 The screen, and collectors outside the config inspector

`MachineStateScreen` (core) is built once per scan, before any inspector runs, and passed in the inspection context:

```rust
pub enum Screened { Withheld(MachineStateMember), Carriable(CarriableMember), Ordinary }

impl MachineStateScreen {
    /// Stats every host path the withheld members' Exact, Glob and Prefix
    /// rules match, listing only those rules' own directories, and keeps
    /// each one's (dev, ino) with its member. Reads no file content.
    pub fn build(exec: &dyn Executor) -> Self;
    /// Logical path, resolved path, then inode. Opens nothing.
    pub fn before_read(&self, exec: &dyn Executor, path: &str) -> Screened;
}

/// The keytab header and whole-file key sniffs, over bytes already read.
pub fn credential_content(content: &[u8]) -> Option<MachineStateMember>;
```

`before_read` takes the logical path's rule, then the resolved path's (`exec.resolve_final_target`), then looks up the file's `(dev, ino)` from `file_metadata`. Resolution and inode add withheld answers only, and a withheld answer from any step wins. So `/etc/myapp/id -> /etc/machine-id` is withheld as `machine_id`, a file under a symlinked directory that resolves into `/etc/pki/tls/private/` is withheld as `private_key_file`, and a hard link elsewhere in `/etc` to `/etc/pki/tls/private/server.key` is withheld too, in whichever order the branches meet the two names. A carriable member is recognized by its own path only: its content is safe to hold, and validation keys it to that path (§ 4.4 rule 2). A failed resolution or stat leaves its step silent, and the header sniff still runs. `BasenameSuffix` rules are not enumerated for the inode set, since that would take a second walk of `/etc`; a real keytab linked under another name is caught by its header.

**Other collectors.** Every collector that stores a file's raw content from under `/etc` calls `before_read` before reading and `credential_content` after. At `76bcab50` those are the unit drop-ins (`collect_drop_ins`, `units/mod.rs`), repo files and GPG keys (`rpm/repos.rs`), firewall zones (`network.rs`) and quadlet files (`containers.rs`). On a withheld answer the collector stores no row for the file, as the drop-in reader already does for a file it cannot read as text, and pushes `machine-state file left out: <path>` to its degraded reasons. The config inspector records the withheld row, with reason and notice, whenever one of its branches reaches the same path. The nonrpm inspector (`/opt`, `/srv`, `/usr/local`) and compose files outside `/etc` are out of scope: no member path is there, and secrets in them are redaction's.

### 4.6 What the design guarantees

- The configuration section never holds a withheld member's content. A member matched by path, alias or inode is not opened. A keytab matched by its header is opened for two bytes. A whole-file private key is read in full, classified, and dropped before anything is stored.
- The raw-content fields § 4.5 lists never hold a member's content, by the same rules.
- No export stages a withheld member's path, or a credential's content, from any of those sections (§ 4.4).

Not guaranteed:

- `--preserve subscription --ack-sensitive` stores the RHSM entitlement and consumer certificates and keys, base64, in the snapshot's subscription section, and `write_config_tree` stages them under `subscription/` (`stage_subscription_files`). That is an acknowledged sensitive feature with its own warnings, and it stays as it is. The `rhsm_consumer` and `rhsm_entitlement` rows describe the configuration entries, not that section.
- A secret inside an ordinary configuration file (a password in `sssd.conf`, a key block inside an application config) is redaction's (§ 5.3).

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
| `Exact /etc/krb5.keytab`, `Glob /etc/krb5.keytab.*`, `BasenameSuffix .keytab`; header sniff `05 01`/`05 02` (§ 4.1) | `KerberosKeytab` | credential | never | owed, `RealmJoin` |
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

**For the spec review: override levels.** The security consult recommended "extra warning" for keytabs, SSH host keys, whole-file private keys and config-management keys. In this design they are `never`, because a credential holds no content in the snapshot (the consult's own constraint) and so an override has nothing to ship (§ 6.2). The consult's `never` list (machine-id, Insights state, RHSM consumer and entitlement, `lvm/devices`) is kept, and those members are withheld too, since content that can never be shipped need not be read. `warn` therefore applies only to carriable identity members: the iSCSI IQN, and the Salt `minion_id` (an identity file next to the agent keys, not a key).

### 5.3 Whole-file private key

`is_whole_file_private_key(content)` is true when the file is a key envelope and nothing else: every line outside a PEM block is blank, a `#` comment, or OpenSSL envelope metadata, every block's label is on the list below, and at least one block is a private key. Any other line, such as an application setting, makes the file ordinary configuration.

- **Key labels:** any label ending in `PRIVATE KEY`, which covers `PRIVATE KEY`, `ENCRYPTED PRIVATE KEY`, `RSA PRIVATE KEY`, `EC PRIVATE KEY`, `DSA PRIVATE KEY` and `OPENSSH PRIVATE KEY`. Legacy encrypted PEM headers (`Proc-Type`, `DEK-Info`) sit inside the block and need no rule.
- **Companion labels:** `CERTIFICATE`, `TRUSTED CERTIFICATE`, `X509 CRL`, and the parameter blocks `EC PARAMETERS`, `DH PARAMETERS` and `DSA PARAMETERS`. `openssl ecparam -genkey` writes `EC PARAMETERS` before `EC PRIVATE KEY` by default.
- **Envelope metadata:** the lines `openssl pkcs12` writes before each block: `Bag Attributes`, `Key Attributes` (with or without `: <No Attributes>`), their indented attribute lines (`    localKeyID: ...`, `    friendlyName: ...`), and `subject=` and `issuer=` lines.

A key embedded in a larger config file is not a member; it stays with redaction (`redaction/patterns.rs:43`), and the `ENCRYPTED PRIVATE KEY` gap in that pattern belongs to `inspectah-secrets-safety-net-spec`. A whole-file encrypted key is a member here, so the gap no longer reaches one.

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

The override is not offered. `SetInclude(Config { path })` for a withheld path returns 422 with `error` starting `not carried:` and naming the reason (new `RefineError::NotCarriable`, mapped in `crates/web/src/error.rs` beside the two `*NotToggleable` arms). The row has no `item_id` in the view (§ 8), so a correct client never posts it.

**Why not offer an include that writes something.** Two alternatives were considered and rejected:

1. *An active `COPY` the operator must supply.* The export would emit a `COPY` with no staged source, which is the build failure `process-docs/skills/export-paths-stage-different-trees.md` exists to prevent, and it would teach operators to bake a key into an image layer anyone with pull access can read.
2. *A commented placeholder or an advisory "provision this".* That is the notice every withheld row already produces whether or not anyone clicks. A button whose only effect is text the export writes anyway is a control that does nothing.

Deploy-time delivery (a secret mount, a provisioning step) is the supported way to give a machine its credential, and every credential member has one in its notice.

### 6.3 Default render

Neither export path stages a class row by default. Both list every row, carriable and withheld, in the audit report's Configuration section under a "Machine state" table: path, reason, notice, and for withheld rows `mode`, `size` and "contents not captured".

### 6.4 Fleet aggregate (beta.7)

`merge_config_sections` (`crates/core/src/aggregate/merge.rs`) partitions each host's section before merging: ordinary rows go to `merge_items` as today, and class rows (every `MachineState` entry and every `withheld` entry) go to a class merge. The generic merge never sees a class row, so its `set_include(true)` on a payload every host shares cannot include one, and its content variants cannot split one path into several rows.

The class merge:

1. **One row per path.** Group class observations by normalized path. A path that is a class row on any host is a class row in the aggregate: an ordinary observation of the same path on another host (a content-matched key on one host, plain configuration on another) is taken out of the ordinary partition and counted in the class row's hosts, so the path never appears in both lists.
2. **Every row is withheld.** The merged row is a `WithheldConfigEntry`: a captured carriable observation becomes `member: Carriable(m)` with its content dropped, since every value is one host's identity. Nothing in the aggregate holds a class member's content.
3. **Deterministic representative.** The member is the first member that is not `Carriable(_)` among the observations in hostname order, else the first `Carriable(_)` one; `kind`, `package`, `mode` and `size` come from the first host in hostname order whose observation has that member.
4. **Prevalence.** `aggregate` holds the hosts that have the path, as `AggregatePrevalence` does for other rows.

Mixed capture success (host A read its IQN, host B could not) needs no rule of its own: both observations are class rows, and the result is one withheld row with both hosts. The view row is `override: "never"` with no `item_id`, `SetInclude` returns 422 `not carried:`, both exports stage nothing for the path, and the provisioning notice comes from the member as on a single host. Phase 2 (§ 10) may give `Generated` members an ordinary merge and show per-host values; beta.7 does neither.

## 7. Provisioning notices

Built once, from the snapshot, by `render::machine_state::provisioning_steps(snap) -> Vec<(ProvisioningStep, Vec<&str>)>`: owed steps of class rows not overridden, deduplicated by step, each with its paths, in `ProvisioningStep` declaration order. Automatic and `None` notices are not steps. An overridden carriable row owes nothing.

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

`ViewResponse` gains one list. Class rows appear only here, never in `config_files`, so no row is shown twice and the existing `config_files` stats buckets keep their meaning.

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
- `SetInclude` on a withheld path: 422, `error` starting `not carried:`. Nothing recorded.
- `SetInclude(Section(Config), ..)` (refine UI fix-up, Phase 2 of that spec) never changes a class row: section bulk ops skip `MachineState` entries the way they skip `shadowed_by` entries.
- The TypeScript `Disposition` union gains `{kind: "machine_state", member, include}`; `isIncluded` mirrors `is_included()`, and `isToggleable` returns false for it, since class rows are decided through their own list.

### Refine presentation

From the UX consult of 2026-11-21, reconciled with the contract above. The consult was written before the Rust half settled two things: every credential row is `never` (no bytes exist to ship), and every row carries `override` and a `notice`. So the consult's warn-and-confirm flow applies only to the two `warn` rows, the iSCSI IQN and Salt `minion_id`, which are identity rows, and its dialog is reworded for identity below. Code read for this section at `e84e6f17`: `MainContent.tsx` (Configuration branch, `handleNotCarriedInclude`), `NotCarriedGroup.tsx`, `ExportDialog.tsx`, `api/exportWarnings.ts`, `GlobalSearch.tsx`, `Sidebar.tsx`, `ServiceSection.tsx` (`configPaths`), and `crates/web/src/web_types.rs` (`not_carried_notice`).

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

PatternFly `Modal variant="small"`, the `StatsBar` reset-confirm pattern; `isDanger` on the primary button is new usage of an existing prop. Today only identity rows are `warn`, so the wording is for identity:

- Title: **Include this host's identity in the image?**
- Body: "Every machine built from this image will share this identity." Then the row's alternative, from its notice: `owed` reads "Leave it out and run this at provisioning instead: <command>."; `automatic` reads "Leave it out and it is <notice line, lower-cased>." (for the IQN, "recreated at first boot").
- Footer: primary **Include in image** with `isDanger`; **Cancel** as a link.
- Initial focus on **Cancel**. Escape or Cancel closes and returns focus to the row's **Include…** button. Confirm closes, sends the op, and returns focus to the same row's button, by then **Exclude**; if the op fails, focus still returns there and the failure shows on the row.
- No "don't ask again". The dialog per row is the point.

The dialog's wording branches on `reason`, so a future `warn` row of another reason gets its own wording rather than the identity text: a `host_credential` row would use the credential wording in the alternative below.

#### Search and reveal

- **Section search** (`SectionSearch` in Configuration) matches a class row on its path, its reason label text and its notice line text. While a query is active the block expands, shows only matching rows, and hides sub-lists with none; clearing the query leaves it expanded. The result count is decision-list matches plus block matches, and "No items match your search" appears only when both are zero.
- **GlobalSearch** gets one entry per class row: section `configs`, title the path, item id `machine-state:<path>`. Paths are unique within `machine_state` (the collector's `seen` set covers both lists) and disjoint from `config_files`, so the id cannot collide with a `configs:` id.
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

#### If credentials become overridable

If credentials are made overridable instead of `never`, the UI change is small: those rows arrive as `warn` with `content_captured: false` and an `item_id`, and the dialog takes a credential variant, branched on `reason`: title **Include a host credential in the image?**, body "Every machine built from this image will share this credential. Anyone who can pull the image can read it. The scan did not capture it, so you supply the file before building." then the notice's alternative, primary **Include key in image**; an included credential row reads `In the image: you supply this file`, and the export dialog's warning alert names it. The view's row fields already carry the override level, the choice and the missing content. What the API would need beyond them: one row field, the export-relative path the operator must place the file at (for example `config/etc/krb5.keytab`), since the UI must not work out the export layout itself; a `failure` entry in `x-export-warnings` while that file is missing; and, outside the view, a snapshot type that can hold an include on a withheld row (`WithheldConfigEntry` has no `include` by design) plus an export that stages an operator-supplied file without emitting a `COPY` with no source (§ 6.2's first rejected alternative).

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
- New core types: `MachineStateReason`, `CarriableMember`, `MachineStateMember`, `OverrideLevel`, `ProvisioningNotice`, `ProvisioningStep`, `RecreatedBy`, `WithheldConfigEntry` (with `aggregate` for fleet rows), the rule table, `classify_path`, `normal_path`, `validate`, `MachineStateScreen` and `credential_content`.
- `SnapshotError::MachineState`: `load` refuses a snapshot that fails § 4.4's validation.
- `Executor::file_metadata` (returning `FileMetadata`) and `Executor::read_head`.
- Render: `WithheldWhy::MachineState { member }` replaces `WithheldWhy::MachineId`; the audit report and Containerfile gain a provisioning section (§ 7); `merge_config_sections` partitions class rows (§ 6.4).
- `SCHEMA_VERSION` and `MIN_SCHEMA` both raised by one from their values when this lands (28 at `391354ca`). A snapshot from before cannot be re-rendered correctly: its silent exclusions are gone and its carried keytab is an ordinary entry.
- Refine: `RefineError::NotCarriable` (422); `ViewResponse::machine_state: Vec<MachineStateRowDto>`; class rows leave `config_files`; a new `notice` in `x-export-warnings`.
- Refine view additions the UI needs, `machine_state_steps` and `machine_state_notices`: § 8, Refine presentation, Required API additions.

CHANGELOG, under `## [Unreleased]`:

```
### Changed
- **Breaking:** Files that belong to one machine, such as keytabs, SSH host keys, private keys, RHSM and Insights registration, machine-id and the iSCSI initiator name, are no longer copied into the image or silently dropped. Refine lists them with the provisioning step each needs; credential contents are left out. Re-scan the host before refining or rendering.
```

## 10. What Phase 2 inherits

Phase 2's aggregate convergence work takes `FindingKind` with four variants and the beta.7 class merge of § 6.4, which keeps no member's content in an aggregate. What Phase 2 may change: give `Generated` members an ordinary merge, since their content is derived from settings the fleet shares; show per-host values for identity members as information, never as content variants an operator can select; and keep a fleet override of a `HostIdentity` member at `never`, which makes the override level a function of (member, single host or fleet) rather than of the member alone. If Phase 2 reworks `ConfigFileEntry` content into an enum (§ 3.1 Option C), the withheld list folds into it as a `Withheld` content case and `ContentNotCaptured` stops being a disposition; the rule table, the screen and the classification point do not change.

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
11. **A second collector.** `/etc/systemd/system/app.service.d/key.conf`, a symlink to an `ENCRYPTED PRIVATE KEY` file in `/etc/pki/tls/private/`, through the full scan: no `units.drop_ins` row for it, a degraded reason naming it, a withheld config row; the snapshot and both exports hold no key bytes.
12. **Modernization does not replace the class.** A test-only modernization pattern on a member path leaves `MachineState` in place.

**Core (`crates/core`):**

13. **Unrepresentable credential with content.** JSON `{"kind":"machine_state","member":"kerberos_keytab","include":true}` fails to deserialize into `FindingKind`. `WithheldConfigEntry` deserialization ignores a stray `content` key and the struct has no field to hold it (assert the re-serialized JSON has no `content`).
14. Round trips for every member; JSON without `withheld` gives an empty list; `is_included` and `with_include` on `MachineState`.
15. **Rule table invariants.** No `UNOWNED_EXCLUDE_EXACT` entry (exported for the test) classifies to a rule, and no `Exact`, `Glob` or `Prefix` rule's path is matched by a retained silent glob or prefix. `BasenameSuffix` rules are exempt by design: a `.keytab` in a silent directory reaches the collector through the orphan branch (case 5). Every `MachineStateMember` and `CarriableMember` variant is the target of at least one rule; the certificate rule precedes the key rule; every carriable member's level is not `Never`; `normal_path` agrees with `Destination::new` on every path in the configtree tests.
16. **Validation at load**, on JSON edited from the snapshots of cases 1, 6 and 9, as a hand-edited snapshot would be: a `config.files` entry at `etc/ssh/ssh_host_ed25519_key`, `/etc/ssh/./ssh_host_ed25519_key` and `/etc//ssh/ssh_host_ed25519_key`, each once included and once excluded; an included and an excluded ordinary entry at `/etc/myapp/server.pem` holding case 9's `EC PARAMETERS` file; a `units.drop_ins` row whose content is a key; `MachineState { hostname }` at `/etc/myapp/x`; one path in both `files` and `withheld`. `load` refuses each with `SnapshotError::MachineState` naming the path, and accepts the unedited snapshots.

**Refine and render:**

17. **Credential override refused** (`crates/refine/tests/session_test.rs`, `crates/web/tests/api_test.rs`): from case 1's snapshot, `SetInclude(Config { "/etc/krb5.keytab" }, true)` returns `Err(NotCarriable)` and `POST /api/op` 422 with `error` starting `not carried:`; `/api/ops` unchanged; the view row has `override: "never"` and no `item_id`; the export has no `config/etc/krb5.keytab` and its `x-export-warnings` carries the provisioning notice naming `realm join`.
18. **Carriable override ships the file on both paths.** From case 6's snapshot: `SetInclude` on the IQN; the refine export stages `config/etc/iscsi/initiatorname.iscsi` with the captured content and carries the `warn` notice; the saved snapshot rendered through the CLI path stages the same bytes; `copy_source_exists_test.rs` passes on both; undo removes it from the next export.
19. **Keytab-only host, both paths.** Case 1's snapshot, whose only finding is the keytab, through `render_all` and through the refine export: no `config/` tree and no config `COPY`; the Containerfile has the `# === Provisioning ===` section naming `realm join` with no paths; `audit-report.md` has `## Provisioning steps`; the scan path's README has it too and the scan prints the notice on stderr; the refine export's `x-export-warnings` carries it. Then cases 1 and 4 together: `realm join` and `insights-client --register` once each on every surface.
20. **Render guard.** A snapshot built in process, never through `load`, with `/etc/ssh/./ssh_host_ed25519_key` as an included `Actionable` entry with content and an included `/etc/myapp/server.pem` holding a key: `staged_writes` withholds both with `WithheldWhy::MachineState`, nothing is written under `config/` for them, and the Containerfile names both in its "not copied" lines.
21. **Fleet aggregate**, from collector-produced host snapshots through `inspectah aggregate`: two hosts with the same IQN; two hosts with different IQNs; host A's IQN captured and host B's unreadable; host A's `/etc/myapp/server.pem` a key and host B's an ordinary config. Each gives one `withheld` row for the path with both hosts in `aggregate`, the path in no `files` entry, and no member content anywhere in the aggregate snapshot; the aggregate view row is `override: "never"` with no `item_id`; `SetInclude` returns 422; both exports stage nothing for it.
22. **View contract.** `contract_snapshots.rs` gains a `machine_state` list with one row per level, accepted with only that delta and the class rows' departure from `config_files`.

## 12. Strongest case against

The class is wider than "files that name one machine", and the `never` level for credentials narrows the decision's "the operator can override". Some sites do bake shared SSH host keys behind a VIP, or a shared service keytab, into an image on purpose, and this design gives them no path from refine; they must deliver the file at deploy time. That is deliberate, but an operator who expects the override will see a row they cannot change.

Second, name-matched rules (`*.keytab`, `/etc/pki/tls/private/`) and the whole-file key sniff will catch files an administrator placed on purpose. The visible withheld row is what makes that acceptable; the silent form would not be.

Third, the protection has costs of its own. `load` refuses a snapshot another tool edited into a forbidden form rather than repairing it, so that tool's user stops at an error. The screen adds a stat per candidate and a two-byte read per unmatched one to every scan, and the alias, inode and second-collector rules are more machinery than the ordinary named-path case needs; they exist because the named path alone was a bypass.

What would change this: evidence that operators bake shared host credentials into images at scale (then add a `CredentialMember` override that emits a deploy-time secret mount stanza, not a `COPY`); a real binary keytab carried byte for byte by today's code (raises urgency, not design); `ipa-client-install` succeeding cleanly over a carried `/etc/ipa/default.conf` (drops its owed notice to `none`).

## 13. Provenance

**Code** read at `391354ca`: `config/mod.rs` (branches, `capture_config`, `detect_orphaned_configs`, `run_ostree_config`), `config/walk.rs` (the three lists, `is_excluded_unowned`), `core/types/finding.rs`, `core/types/config.rs`, `refine/src/session.rs` (`SetInclude(Config)` validation and projection), `web/src/web_types.rs` (`ConfigFileDto`), `core/snapshot.rs` (`SCHEMA_VERSION` 28), `core/traits/executor.rs` (no metadata or byte read today), and the skills cited inline.

Read at `76bcab50`: `executor/real.rs` (`read_file` reads the whole file before the UTF-8 check fails; `resolve_final_target` is `canonicalize`), `config/walk.rs` (`walk_recursive_inner` follows directory symlinks that stay inside `/etc`), `config/mod.rs` (`detect_orphaned_configs` skips paths already captured and needs the removed package's name in the basename), `units/mod.rs` (`collect_drop_ins` reads each `.conf` itself), `subscription.rs` and `configtree.rs` (`stage_subscription_files`; `Destination`; `staged_writes` withholding the machine-id on every offer), `aggregate/merge.rs` (`merge_items`, `merge_with_variants`, `merge_config_sections`), `core/snapshot.rs` (`load`), `refine/src/session.rs` (`render_refine_export`'s file set, no README), `refine/src/types.rs` (`SectionStats::from_dispositions`), `containerfile.rs` (`section`, config `COPY` per staged root), `redaction/patterns.rs` (the PEM pattern).

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

What this corrects in the consult's recall: the iSCSI IQN is unowned, not package-owned, so today's unowned walk carries it into the image (no list names it), which is a live defect this change closes rather than an rpm -Va corner case. `/etc/machine-id` and `/etc/hostname` are `%config %ghost`, so they are invisible today and reach the inspector only through the ghost-file change's branch, which is why § 4 puts the class in the collector. `ipa/default.conf` and `localhost.key` could not be measured on hosts without their packages; the ghost claims for them remain recalled. No real binary keytab exists on either VM. The design does not depend on how one decodes, since the header check precedes the full read (§ 4.1); tests 1 and 2 cover both encodings.
