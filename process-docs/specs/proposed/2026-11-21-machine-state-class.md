# Machine-State Class for Configuration Files

## Metadata

| Field | Value |
|---|---|
| Date | 2026-11-21 |
| Status | Proposed. Design pass for decision 4 of the beta.7 release list (Mark Russell, 2026-11-21). Choices marked **for the spec review** are the author's and need the review to confirm them. |
| Scope | A named class of configuration files that belong to one machine: how the snapshot carries it, where the config inspector classifies it, the member list, override semantics in refine and both export paths, provisioning notices, and the refine API contract. The refine UI is out of scope beyond § 8's API. |
| Backlog items | `inspectah-config-walk-copies-registration-and-tool-generated-state-into-the-image`, `inspectah-keytabs-ship-as-ordinary-config` (inspectah-team `workflow/backlog/inspectah/open/`) |
| Inputs | A security consult on the class list, and a UX consult on the refine presentation, both 2026-11-21. Real-host measurements in § 13, taken for this spec. |
| Verified | Code citations read at `391354ca` (2026-11-21). Line numbers are a convenience; the symbol name is the identifier. |

**No code, documentation, or CHANGELOG entry was written to produce this file.**

## 1. Problem

The config inspector has two ways to treat a file that identifies one machine, and both are wrong.

1. **Silent exclusion.** `UNOWNED_EXCLUDE_EXACT`, `UNOWNED_EXCLUDE_GLOBS` and `UNOWNED_EXCLUDE_PREFIX` (`crates/collect/src/inspectors/config/walk.rs:44`, `:134`, `:198`) drop SSH host keys, `machine-id`, Insights registration state, RHSM consumer and entitlement certificates, NVMe host ids and `/etc/lvm/devices/*` before anything records them. An operator auditing the export cannot tell "not collected" from "never existed", and nothing tells them a provisioning step is owed.
2. **Silent carriage.** Anything the lists do not name is carried. Measured on RHEL 9.8 and 10.2 (§ 13): `/etc/krb5.keytab`, `/etc/iscsi/initiatorname.iscsi` and `/etc/grub.d/00_tuned` are all unowned and appear in no list, so the unowned walk stages them under `config/etc/` and `COPY config/etc/ /etc/` puts them in the image. RHEL 10's `/etc/authselect/switchable-auth` is the same.

The lists are also consulted only by the unowned branch. The rpm -Va branch (`config/mod.rs:121-170`) skips them by design, and `detect_orphaned_configs` (`config/mod.rs:643-710`) never calls `is_excluded_unowned`, so `/etc/rhsm/facts/insights-client.facts` is captured as `Orphaned` on a host where insights-client was removed. A parallel change, the ghost-file change, adds a fourth branch for `%ghost` files that exist on the host; § 13 measures `/etc/machine-id` and `/etc/hostname` as `%config %ghost` files of `systemd`, so that branch would collect both.

Decision 4 (Mark Russell, 2026-11-21): a named machine-state class. Files that identify one machine are shown in refine as excluded, with a visible reason the operator can override. Keytabs are in it.

## 2. Design summary

- **Two kinds of member.** A *carriable* member (host identity or generated file whose content is safe to hold) is an ordinary `ConfigFileEntry` with a new disposition, `FindingKind::MachineState { member, include }`, excluded by default and overridable. A *withheld* member (every host credential, and the identity files that a copy always breaks) is a `WithheldConfigEntry` in a new `ConfigSection::withheld` list. That type has no `content` and no `include` field, so a credential with content, or an included credential, cannot be written down.
- **One classification point.** A `ConfigCollector` owns the section during inspection, and its `admit` method is the only way any branch adds a file. `admit` classifies by path before reading, so a withheld member is never read.
- **Members as data.** A rule table in `inspectah-core` maps path patterns to members; each member answers its reason, override level and provisioning notice. The identity and credential entries leave the silent lists; pure noise stays silent.
- **Override.** Carriable members are overridable at `warn` or `plain`; an override ships the captured file through the existing config tree on both export paths. Withheld members are `never`: the refine API refuses the op with 422, because there are no bytes to ship.
- **Notices.** Owed provisioning steps (`realm join`, `subscription-manager register`, ...) go to the README, a short Containerfile comment, `x-export-warnings`, and the scan's stderr, each deduplicated by step.
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
    // Withheld by rule: never carried, never read.
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
    // Carriable members, withheld only when their read failed (§ 4.3).
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
/// A configuration file the scan found and deliberately did not read.
/// Never staged, never included: the type has nowhere to put either.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WithheldConfigEntry {
    pub path: String,
    /// Which branch found it: unowned, orphaned, rpm_owned_modified, or
    /// the ghost branch's kind once that branch exists.
    pub kind: ConfigFileKind,
    pub package: Option<String>,
    pub member: MachineStateMember,
    /// From lstat. `None` when the stat failed.
    pub mode: Option<u32>,
    pub size: Option<u64>,
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
    section: ConfigSection,       // private: no branch can push directly
    seen: HashSet<String>,        // paths in files or withheld
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

1. `machine_state::classify_path(path)` (core, § 5). A withheld rule: `exec.file_metadata(path)` for mode and size, push a `WithheldConfigEntry`, return. **The file is not opened.**
2. A carriable rule: capture content as `capture_config` does today. Success: push a `ConfigFileEntry` with `FindingKind::MachineState { member, include: false }`. Failure: push a withheld row with `MachineStateMember::Carriable(member)` (§ 4.3).
3. No rule: capture content. On `InvalidData` (not UTF-8), read the first two bytes with `exec.read_head(path, 2)`; `05 01` or `05 02` is a keytab header, so push a withheld `KerberosKeytab` row. On a successful read, `is_whole_file_private_key(&content)` (§ 5.3) pushes a withheld `PrivateKeyFile` row and drops the string. Otherwise push the entry exactly as today.

Every branch (rpm -Va, unowned walk, orphan, the ostree path, and the ghost-file change's branch) takes `&mut ConfigCollector`. Each branch keeps its own skips (DHCP, dev artifacts, rpm ownership, the silent lists in the unowned walk); the class is applied by the one function every surviving path calls. A new branch has no other way to add a file, which is the "by construction" the class needs: forgetting to classify would mean bypassing a private field.

The modernization pass (`config/mod.rs:299-315`) goes through the collector too: it skips any path in `withheld`, and `with_modernization` returns the existing disposition unchanged when it is `MachineState`. No member path matches a modernization pattern today; the rule keeps it that way.

### 4.2 Executor additions

`Executor` (`crates/core/src/traits/executor.rs`) gains:

```rust
/// lstat: permission bits and size, without opening the file.
fn file_metadata(&self, path: &Path) -> io::Result<FileMetadata>;
/// At most `n` bytes from the start of the file.
fn read_head(&self, path: &Path, n: usize) -> io::Result<Vec<u8>>;
```

`MockExecutor` gains byte-content files (so `read_file` on a non-UTF-8 fixture fails with `InvalidData`, as `std::fs::read_to_string` does) and a read log that tests use to assert a withheld path was never read.

### 4.3 Carriable members that cannot be read

A carriable member whose read fails has nothing to ship, so it is withheld: a `WithheldConfigEntry` with `member: Carriable(m)`, override `never`. The row keeps its reason and notice, which the `ContentNotCaptured` advisory it would get today does not carry. `degraded` records the failure as today.

### 4.4 Render guard

`staged_writes` and `write_config_tree` (`crates/pipeline/src/render/configtree.rs`) re-run `classify_path` on every entry they would stage and refuse a path that classifies as a withheld rule, returning a render error naming the path. A collector-produced snapshot never trips it; a hand-edited or externally produced snapshot that lists `/etc/krb5.keytab` as an included `Actionable` entry with content cannot put it in the image. The guard costs one table lookup per staged file.

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

`is_whole_file_private_key(content)` is true when the trimmed content is one or more PEM blocks and nothing else, every block's label is either a certificate or ends in `PRIVATE KEY` (covering `ENCRYPTED PRIVATE KEY`, `RSA PRIVATE KEY`, `EC PRIVATE KEY`, `OPENSSH PRIVATE KEY`), and at least one is a key. A key embedded in a larger config file is not a member; it stays with redaction (`redaction/patterns.rs:43`), and the `ENCRYPTED PRIVATE KEY` gap in that pattern belongs to `inspectah-secrets-safety-net-spec`.

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
| Fleet (beta.7) | The aggregate merge passes class rows through excluded and the aggregate refine refuses `SetInclude` on them with 422 until Phase 2 (§ 10). A fleet image carrying one host's IQN is the failure the class exists to prevent. |

### 6.2 Withheld members (`never`)

The override is not offered. `SetInclude(Config { path })` for a withheld path returns 422 with `error` starting `not carried:` and naming the reason (new `RefineError::NotCarriable`, mapped in `crates/web/src/error.rs` beside the two `*NotToggleable` arms). The row has no `item_id` in the view (§ 8), so a correct client never posts it.

**Why not offer an include that writes something.** Two alternatives were considered and rejected:

1. *An active `COPY` the operator must supply.* The export would emit a `COPY` with no staged source, which is the build failure `process-docs/skills/export-paths-stage-different-trees.md` exists to prevent, and it would teach operators to bake a key into an image layer anyone with pull access can read.
2. *A commented placeholder or an advisory "provision this".* That is the notice every withheld row already produces whether or not anyone clicks. A button whose only effect is text the export writes anyway is a control that does nothing.

Deploy-time delivery (a secret mount, a provisioning step) is the supported way to give a machine its credential, and every credential member has one in its notice.

### 6.3 Default render

Neither export path stages a class row by default. Both list every row, carriable and withheld, in the audit report's Configuration section under a "Machine state" table: path, reason, notice, and for withheld rows `mode`, `size` and "contents not captured".

## 7. Provisioning notices

Built once, from the snapshot, by `render::machine_state::provisioning_steps(snap) -> Vec<(ProvisioningStep, Vec<&str>)>`: owed steps of class rows not overridden, deduplicated by step, each with its paths, in `ProvisioningStep` declaration order. Automatic and `None` notices are not steps. An overridden carriable row owes nothing.

| Surface | What it says |
|---|---|
| README | New `## Provisioning steps` section after `## Build and deploy`, present when any step exists: one bullet per step, the command in code, the paths it covers; then one sentence that the remaining machine-state files are recreated without operator action, with a count. Paths go through the Markdown literal helper (`process-docs/skills/markdown-literal-host-text.md`). |
| Containerfile | A comment block directly above `COPY config/etc/ /etc/`, only when a step exists, one line per step and no paths: `# Not in the image: this host's identity. At provisioning:` then `#   realm join (or ipa-client-install, adcli join)` and so on. Paths stay out so the block reads like a note a person would leave, not an inventory; the README has the paths. Text is enum-owned, so `comment.rs` escaping has nothing hostile to handle, and it still goes through it. |
| `x-export-warnings` | One `notice`, after the not-carried notice: `<N> machine-state files left out. At provisioning: realm join, subscription-manager register.` Only enum text and a count, so no snapshot string enters the header and the 8 KB bound test needs no new case for it. A `warn` override adds the notice in § 6.1. |
| Scan stderr | `run_scan` prints the same notice after the not-carried one, as `Notice: ...` (`process-docs/skills/refine-export-warnings-header.md`, § The scan prints the notice too). |

The export dialog's "configs excluded" count must not count class rows; that is a UI rule recorded in § 8 for the refine owner.

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

*Left for the refine owner, from the UX consult of 2026-11-21 and the contract above. The API settles two things that consult asked for: a credential row is `never` (its warn-and-confirm flow applies only to `warn` identity rows, today the iSCSI IQN and Salt `minion_id`), and every row carries `override` and a `notice` with an `owed` or `automatic` kind.*

## 9. Snapshot and API changes

- `FindingKind::MachineState { member: CarriableMember, include: bool }` (new variant).
- `ConfigSection::withheld: Vec<WithheldConfigEntry>` (new field, `#[serde(default)]`, skipped when empty).
- New core types: `MachineStateReason`, `CarriableMember`, `MachineStateMember`, `OverrideLevel`, `ProvisioningNotice`, `ProvisioningStep`, `RecreatedBy`, `WithheldConfigEntry`, the rule table and `classify_path`.
- `Executor::file_metadata` and `Executor::read_head`.
- `SCHEMA_VERSION` and `MIN_SCHEMA` both raised by one from their values when this lands (28 at `391354ca`). A snapshot from before cannot be re-rendered correctly: its silent exclusions are gone and its carried keytab is an ordinary entry.
- Refine: `RefineError::NotCarriable` (422); `ViewResponse::machine_state: Vec<MachineStateRowDto>`; class rows leave `config_files`; a new `notice` in `x-export-warnings`.

CHANGELOG, under `## [Unreleased]`:

```
### Changed
- **Breaking:** Files that belong to one machine are now a named class. Kerberos keytabs, SSH host keys, private key files, RHSM and Insights registration state, machine-id, the iSCSI initiator name and similar files are no longer copied into the image or silently dropped: refine lists them as excluded with a reason and the provisioning step each one needs, and the README and Containerfile name those steps. Host credentials are never read or carried; host identity and generated files can be included from refine. The snapshot format changed, so re-scan the host before refining or rendering.
```

## 10. What Phase 2 inherits

Phase 2's aggregate convergence work takes `FindingKind` with four variants, and `MachineState` is a per-host fact, not a variant choice. The fleet merge must carry withheld rows as a union by path with a host count (the content is absent, so there is nothing to compare), and must never offer a carriable identity member as a content variant, since every value is one host's identity; `Generated` members may merge like ordinary config. A fleet override of a `HostIdentity` member should be `never`, which makes the override level a function of (member, single host or fleet) rather than of the member alone. If Phase 2 reworks `ConfigFileEntry` content into an enum (§ 3.1 Option C), the withheld list folds into it as a `Withheld` content case and `ContentNotCaptured` stops being a disposition; the rule table and classification point do not change.

## 11. Tests

Built from collector-produced data: each collect case drives `ConfigInspector::inspect` through `MockExecutor` with files and rpm state at the paths, never a hand-built `ConfigFileEntry` (the lesson of 2026-10-31, where fixtures encoding layouts the collector could not produce passed three review rounds). Refine and render cases start from the snapshot those collect cases produce.

**Collect (`crates/collect/tests/config_test.rs`):**

1. **Real binary keytab, unowned.** `/etc/krb5.keytab` holding bytes `05 02` followed by non-UTF-8 key data including a distinctive marker run. Assert one `withheld` row, `member: kerberos_keytab`, `kind: unowned`, `mode` and `size` from metadata; the mock read log shows no `read_file` or `read_head` on the path (it matched by path); the serialized snapshot does not contain the marker in any encoding (raw, hex, base64).
2. **Binary keytab by header.** `/etc/myapp/service.bin`, `05 02` header, no matching name: withheld `kerberos_keytab` through the `InvalidData` then `read_head` path; the read log shows `read_head(2)` and nothing larger.
3. **Text keytab** (the driftify fixture form, measured on both VMs in § 13): `/etc/krb5.keytab` and `/etc/krb5.keytab.ad` as UTF-8 text: both withheld by path, never read.
4. **Orphan branch.** `dnf history` with `insights-client` removed and an unowned `/etc/rhsm/facts/insights-client.facts`: withheld `insights_state` with `kind: orphaned`, `package: insights-client`. Second case: `salt-minion` removed, `/etc/salt/minion_id` present: `MachineState { member: salt_minion_id, include: false }`, `kind: orphaned`, content captured.
5. **rpm -Va branch.** `/etc/iscsi/initiatorname.iscsi` reported modified with flags `S.5....T.  c`: `MachineState { iscsi_initiator_name }` with `kind: rpm_owned_modified`.
6. **Ghost branch** (lands with the ghost-file change, or with this change if that one is first): `/etc/machine-id` owned by `systemd` with `%config %ghost` flags and present: withheld `machine_id`, never read. `/etc/hostname`, same flags: `MachineState { hostname }`.
7. **Unreadable carriable member.** `/etc/iscsi/initiatorname.iscsi` with permission denied: withheld row, `member: iscsi_initiator_name`, override `never` in the view; `degraded` records it.
8. **Whole-file private key.** `/etc/myapp/server.pem` holding only an `ENCRYPTED PRIVATE KEY` block: withheld `private_key_file`, content absent from the snapshot. `/etc/myapp/bundle.pem` with a certificate and a key: withheld. `/etc/myapp/app.conf` with a key block inside other text: an ordinary entry (redaction's case).
9. **Modernization does not replace the class.** A test-only modernization pattern on a member path leaves `MachineState` in place.

**Core (`crates/core`):**

10. **Unrepresentable credential with content.** JSON `{"kind":"machine_state","member":"kerberos_keytab","include":true}` fails to deserialize into `FindingKind`. `WithheldConfigEntry` deserialization ignores a stray `content` key and the struct has no field to hold it (assert the re-serialized JSON has no `content`).
11. Round trips for every member; JSON without `withheld` gives an empty list; `is_included` and `with_include` on `MachineState`.
12. **Rule table invariants.** No `UNOWNED_EXCLUDE_*` entry (exported for the test) matches any rule; every `MachineStateMember` and `CarriableMember` variant is the target of at least one rule; the certificate rule precedes the key rule; every carriable member's level is not `Never`.

**Refine and render:**

13. **Credential override refused** (`crates/refine/tests/session_test.rs`, `crates/web/tests/api_test.rs`): from case 1's snapshot, `SetInclude(Config { "/etc/krb5.keytab" }, true)` returns `Err(NotCarriable)` and `POST /api/op` 422 with `error` starting `not carried:`; `/api/ops` unchanged; the view row has `override: "never"` and no `item_id`; the export has no `config/etc/krb5.keytab` and its `x-export-warnings` carries the provisioning notice naming `realm join`.
14. **Carriable override ships the file on both paths.** From case 5's snapshot: `SetInclude` on the IQN; the refine export stages `config/etc/iscsi/initiatorname.iscsi` with the captured content and carries the `warn` notice; the saved snapshot rendered through the CLI path stages the same bytes; `copy_source_exists_test.rs` passes on both; undo removes it from the next export.
15. **Default render.** Case 1 plus 4's snapshot rendered by `render_all`: no class file under `config/`; README has `## Provisioning steps` with `realm join` and `insights-client --register` once each; the Containerfile carries the comment block above `COPY config/etc/ /etc/` with no paths; the scan prints the notice on stderr.
16. **Render guard.** A snapshot JSON with `/etc/krb5.keytab` as an included `actionable` entry with content: `render_all` and the refine export both refuse with an error naming the path.
17. **View contract.** `contract_snapshots.rs` gains a `machine_state` list with one row per level, accepted with only that delta and the class rows' departure from `config_files`.

## 12. Strongest case against

The class is wider than "files that name one machine", and the `never` level for credentials narrows the decision's "the operator can override". Some sites do bake shared SSH host keys behind a VIP, or a shared service keytab, into an image on purpose, and this design gives them no path from refine; they must deliver the file at deploy time. That is deliberate, but an operator who expects the override will see a row they cannot change.

Second, name-matched rules (`*.keytab`, `/etc/pki/tls/private/`) and the whole-file key sniff will catch files an administrator placed on purpose. The visible withheld row is what makes that acceptable; the silent form would not be.

What would change this: evidence that operators bake shared host credentials into images at scale (then add a `CredentialMember` override that emits a deploy-time secret mount stanza, not a `COPY`); a real binary keytab carried byte for byte by today's code (raises urgency, not design); `ipa-client-install` succeeding cleanly over a carried `/etc/ipa/default.conf` (drops its owed notice to `none`).

## 13. Provenance

**Code** read at `391354ca`: `config/mod.rs` (branches, `capture_config`, `detect_orphaned_configs`, `run_ostree_config`), `config/walk.rs` (the three lists, `is_excluded_unowned`), `core/types/finding.rs`, `core/types/config.rs`, `refine/src/session.rs` (`SetInclude(Config)` validation and projection), `web/src/web_types.rs` (`ConfigFileDto`), `core/snapshot.rs` (`SCHEMA_VERSION` 28), `core/traits/executor.rs` (no metadata or byte read today), and the skills cited inline.

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

What this corrects in the consult's recall: the iSCSI IQN is unowned, not package-owned, so today's unowned walk carries it into the image (no list names it), which is a live defect this change closes rather than an rpm -Va corner case. `/etc/machine-id` and `/etc/hostname` are `%config %ghost`, so they are invisible today and reach the inspector only through the ghost-file change's branch, which is why § 4 puts the class in the collector. `ipa/default.conf` and `localhost.key` could not be measured on hosts without their packages; the ghost claims for them remain recalled. No real binary keytab exists on either VM, so the `read_to_string` failure for one remains inferred from source (`executor/real.rs:379`) and is covered by test 1 rather than by measurement.
