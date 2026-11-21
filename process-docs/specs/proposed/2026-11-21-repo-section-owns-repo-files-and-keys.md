# The Repo Section Owns Its `.repo` Files and GPG Keys

## Metadata

| Field    | Value |
|----------|-------|
| Date     | 2026-11-21 |
| Status   | Proposed |
| Release  | 0.9.0-beta.7 |
| Decision | The repo section owns both the `/etc/yum.repos.d/*.repo` file and the keys its `gpgkey=` names. The repo toggle decides whether its `.repo` file and those keys ship, and their Configuration rows become read-only inventory, "decided under Repositories". Position 26 of the refine-ui-fixup spec is reopened so a repo whose packages have all been excluded can still be toggled (Mark Russell, 2026-11-21). |
| Verified | Code citations checked against inspectah commit `102a9f77`; the crates are unchanged at `b3d2622e` and at `5b041d8a`, where the aggregate merge citations of § 3.4 were re-read. Source reading only; no new host run. |
| Supersedes | Position 26 of `2026-10-26-refine-ui-fixup.md`, rewritten in place to point here. Position 29 and the § 3.2 repo no-op paragraph there are rewritten to § 5's rule. |

## 1. Problem

A `.repo` file and a GPG key can each sit in two inventories with two live dispositions, and the stager ships the file when **either** is included.

- **Repo files.** The RPM collector reads every `.repo` file in `/etc/yum.repos.d/` into `rpm.repo_files` (`crates/collect/src/inspectors/rpm/repos.rs:40-87`). The config walk also records the same file in `config.files` when it is unowned or a modified RPM-owned file. `staged_writes` offers both (`crates/pipeline/src/render/configtree.rs:691-713`), and its "file two sections record" rule (`configtree.rs:1018-1030`) only makes the inventory copy yield to an **included** config entry. Config excluded plus repo included therefore ships the repo copy, and config included plus repo excluded ships the config copy. Measured on one polarity on 2026-11-01 with `x-export-warnings: []`.
- **GPG keys.** `extract_gpg_keys` reads every key a `gpgkey=` names into `rpm.gpg_keys` (`repos.rs:256-345`). An unowned key under `/etc/pki/rpm-gpg/` is also a config row, and the same OR rule applies. There is no `ItemId` for a key, so the config row is the only control in view, and it does nothing.
- **A repo with no member to move cannot be toggled.** `check_repo_noop` (`crates/refine/src/session.rs:2726-2768`) refuses a repo op whose effective member set is empty or already at the requested state. A repo whose packages are all excluded, all locked, or that has no package attributed to it keeps shipping its `.repo` file and keys, and refine offers no control that drops them. A repo with no attributed package is not listed at all: `build_repo_groups` (`crates/refine/src/projection/decisions.rs:513-570`) lists only sections that some package names.
- **Two parsers say what a repo names.** `extract_gpg_keys` reads `gpgkey=` through `gpgkey_values` (`repos.rs:141-179`), which joins indented continuation lines, keeps every assignment, ignores section headers, and takes a bare path as a local key. `RepoIndex` reads the same file through `parse_repo_sections` (`crates/core/src/types/repo.rs:298-339`), which drops continuation lines, keeps the last assignment in each section, drops lines before the first header, and keeps only `file://` values. A key the first collects and the second never names has no repo that owns it, so once its config row is read-only nothing can drop it. `collect_repo_files` also keeps a `.repo` file that defines no section.
- **A rejected key comes back.** `extract_gpg_keys` records a file that is not a PGP public key, or that holds private key armor, at its path with placeholder content and excluded (`repos.rs:317-342`). Nothing marks it as different from a usable key. The repo include arm sets every key the repo names that the base image does not ship to included (`session.rs:2922-2935`), placeholder and all, and `toggle_restores` does the same (`crates/pipeline/src/render/repo_enabling.rs:221-226`). The aggregate merge sets every universal item included (`crates/core/src/aggregate/merge.rs:591-602`) and re-excludes only keys the base image ships (`merge.rs:1171-1175`), so a universal placeholder key and a universal `redhat.repo` are both included in an aggregate. Read from source, not measured.

The repo toggle already moves the file and key dispositions (`session.rs:2840-2937`, through `RepoIndex::repo_file_carried` and `RepoIndex::gpg_key_carried`). What is missing is that it is not the **only** thing that does, that it refuses the case where it is the only thing left to decide, and that "the files and keys a repo names" has two answers.

## 2. What this spec settles

1. One parser, and one owner: a `.repo` file in `rpm.repo_files` and a key in `rpm.gpg_keys` are decided by the repo toggle and by nothing else, and every such entry the export can carry has a toggle that can drop it (§ 3).
2. What the toggle does to each export path (§ 4).
3. The repo's decision kept apart from what its files actually do, and a no-op rule over both (§ 5).
4. A rule for each kind of shared file (§ 6).
5. The refine half, separated for the frontend implementer (§ 7).
6. The snapshot and API changes, and the changelog lines (§ 8).
7. The tests (§ 9).

Out of scope: § 10. Open decisions: § 11.

## 3. Ownership model

### 3.1 One parser

`parse_repo_sections` becomes the only reader of `.repo` content. `gpgkey_values` is deleted, and `extract_gpg_keys` reads the keys of each section the parser returns. Changes to the parser:

- **Continuation.** An indented line that follows a `key=value` line continues that value. A comment line or an empty line ends the value; a line of only spaces or tabs stays inside it. These are `gpgkey_values`'s rules, which its doc states as dnf's ini reading.
- **Replacement, headers, stray lines:** unchanged. Each option keeps its last assignment across every block of its section, a repeated header reopens its section, and lines before the first header are dropped. A `gpgkey=` before the first header, or one a later assignment replaced, names no key.
- **Trailing comments:** unchanged, `strip_trailing_comment` as today. Whether that matches dnf is a parser-fidelity question this spec does not reopen; with one parser, any answer applies to collection and ownership alike.
- **Key references.** `RepoSectionDef::gpg_key_paths: Vec<String>` becomes `gpg_keys: Vec<GpgKeyRef>`, every reference in the final `gpgkey=` value, split on whitespace and commas:

```rust
// crates/core/src/types/repo.rs
/// One reference in a section's final `gpgkey=` value.
pub enum GpgKeyRef {
    /// `file:///path`, or a value with no `://`, which the scan reads as a
    /// path. The path as written, before dnf variables expand.
    Local(String),
    /// Any other scheme. dnf fetches it; the scan does not.
    Remote(String),
}
```

Every consumer that asks which keys a section names reads `GpgKeyRef::Local` from this struct: `extract_gpg_keys`, `RepoIndex::build`, `repo_enabling::serves` and `toggle_restores`, `repoless::disabled_repo_install`, and `RepoIndex`'s provenance check. Collection and ownership then cannot disagree on continuation, replacement, bare paths or stray lines: they read one parse.

**A `.repo` file that defines no section is not a repo definition.** `collect_repo_files` keeps only content for which `parse_repo_sections` returns at least one section. A comment-only or sectionless file is then not in `rpm.repo_files`, no repo owns it, nothing shadows its config row, and that row, when the config walk records one, is its own control. Included, it ships through the Configuration section like any other config file.

### 3.2 What a repo owns

A repo is a `[section]` by its exact spelling, as `RepoIndex::repo_identity` names it. It owns:

- every entry of `rpm.repo_files` that defines that section, and
- every entry of `rpm.gpg_keys` that a `GpgKeyRef::Local` of that section's final `gpgkey=` names (`RepoFile::is_key_named_by`).

**Ownership is membership in the repo inventory, and owners are read from the repo files the snapshot holds.** In a scan, two collector facts give every entry an owner:

- `rpm.repo_files` holds only `.repo` files in `/etc/yum.repos.d/` that define at least one section (§ 3.1).
- `rpm.gpg_keys` has one source, `extract_gpg_keys`, run over the sections `parse_repo_sections` returns for each entry of `rpm.repo_files` (`crates/collect/src/inspectors/rpm/mod.rs:200-215`). Every key in it was named by some section's `Local` reference. A key under `/etc/pki/rpm-gpg/` that no section names never enters `rpm.gpg_keys`, and stays an ordinary config row with its own control.

An aggregate breaks the second fact. Repo files and keys merge independently, each to one representative payload (`merge_rpm_sections`, `crates/core/src/aggregate/merge.rs:1158-1168`), so a key every host collected can be named by no representative: three hosts whose `x.repo`, `y.repo` and `z.repo` each name key A on two hosts and key B on the third, a different third host for each file, merge to three representatives naming only A, while B, which every host collected, is universal. Ownership is therefore never read from how the snapshot was collected. It is read, by one function, from the repo files the snapshot holds: `repo_ownership` (§ 3.4) parses each entry of `rpm.repo_files` with § 3.1's parser and maps every destination in `rpm.repo_files` and `rpm.gpg_keys` to the sections that own it. A repo file always has one, since every representative defines a section. A key may have none in an aggregate; it is never carriable, and § 3.4's pass excludes it. `RepoIndex::build` (`crates/refine/src/repo_index.rs:66-160`) takes its `repo_file_by_section` and `gpg_keys_by_section` maps from `repo_ownership`, so refine, the pass and the load check cannot disagree about who owns what. § 9 T1 pins the collector invariant over the inputs where the two parsers disagree today, and T7 the aggregate case.

**Identity is the destination.** `RepoIndex` keys every owned entry by `Destination::new(path)`, the stager's normalized path, and the repo arm moves every inventory entry at that destination. A key named once as `/etc/pki/rpm-gpg//KEY` and once as `/etc/pki/rpm-gpg/KEY` is recorded twice by `record_each_key_once`, which merges by raw path; to every decision it is one entry.

**Carriable entries.** An owned entry is carriable when a repo op can carry it:

- a repo file that is not a default repo file (`is_default_repo`) and not locked;
- a key that has an owner, is not rejected (§ 3.6), is not shipped by the base image (`shipped_by_base`), and is not locked.

A default repo file, a base-shipped key and a rejected key are still owned, shown, and explained (§ 7), but no op ever includes one. A key with no owner is shown and explained too, and no op can reach it. A default repo file is excluded by the scan because the subscription flow mounts `redhat.repo` at build time, and the scan treats the other default names the same way; the repo include arm stops overriding that, as it already stops for a base-shipped key. `locked` is never set on a repo file or key by `normalize`; it can only arrive in an imported snapshot, where `clamp_locked_items` already holds a locked repo file excluded (`session.rs:415-419`).

### 3.3 The link: options

Three valid patterns. The spec review picks; the recommendation is option A.

**A. Inventory membership, plus a typed shadow on the config row (recommended).** Nothing new is stored to link a file to its repo. The repo-to-file link stays derived from the file's content and the key's recorded `gpgkey_references`, through the one parser (§ 3.1) and the one matcher `RepoFile::is_key_named_by`. The config-to-repo link is stored by § 3.4's pass, which every producer of a snapshot runs last: the config row is marked `FindingKind::Inventory` with `shadowed_by: Some(ConfigShadow::Repositories)`. The render reads dispositions and never matches a config path against the repo inventory.

```rust
// crates/core/src/types/config.rs
pub enum ConfigShadow {
    Units,
    /// The file's destination is that of an entry of `rpm.repo_files` or
    /// `rpm.gpg_keys`: the repo toggle decides whether it ships, and the
    /// repo inventory's entry is what the stager writes.
    Repositories,
}
```

**B. Stored back-links.** `RepoFile` gains `defines: Vec<RepoId>` for a repo file and `named_by: BTreeSet<(String, RepoId)>` for a key, written by the collector. Rejected: both are facts the content already holds, so they would be a second writer for one fact, and the aggregate merge would need a union rule for each, as it already needs one for `gpgkey_references` (`crates/core/src/aggregate/merge.rs:1163-1172`).

**C. Derived carriage.** Drop `disposition` from repo files and keys and compute whether each ships from a per-repo decision. This makes "a file included while every repo that keeps it is excluded" unrepresentable, which is the right end state. Rejected for this release: the scan tarball has no per-repo decision to derive from, so the snapshot would need a new per-repo record that the CLI path, the aggregate merge and refine all write, and about ten readers of `RepoFile::disposition` change with it (`repo_enabling.rs`, `repoless.rs`, `containerfile.rs`, `configtree.rs`, `redaction/engine.rs`, the aggregate merge). Option A reaches one owner with the existing state, because after it the repo op is the only writer of those dispositions after the scan (§ 3.7).

### 3.4 The ownership pass

The ownership and shadow invariants are established in one place, over a whole snapshot, after the last change to its `rpm` and `config` sections, by whichever producer made it. A scan establishes them per host. An aggregate establishes them again over the merged sections, after representative selection, because neither survives the merge on its own: repo files and keys pick their representatives independently (§ 3.2), and the config merge clones the first host's entry for each content variant (`merge_with_variants`, `crates/core/src/aggregate/merge.rs:667-678`) without reconciling its shadow, so a key that is repo-owned on one host and an ordinary config row on another can merge to an actionable, included config row.

```rust
// crates/core/src/repo_ownership.rs (new)

/// Who owns each destination of `rpm.repo_files` and `rpm.gpg_keys`.
pub struct RepoOwnership {
    /// Every destination either list records, with its owning sections by
    /// exact spelling. A key that no section of any `rpm.repo_files`
    /// entry names maps to an empty set.
    owners: BTreeMap<Destination, BTreeSet<RepoId>>,
}

/// Ownership read from the repo files `rpm` holds, through
/// `parse_repo_sections` and `RepoFile::is_key_named_by` (§ 3.1, § 3.2).
/// `RepoIndex::build` takes its owner maps from this.
pub fn repo_ownership(rpm: &RpmSection) -> RepoOwnership;

/// Establishes § 3.2 over `snapshot` as it stands. Idempotent.
///
/// 1. Excludes every repo file or key no repo op can carry: a default
///    repo file, a rejected key, a base-shipped key, and a key with no
///    owner.
/// 2. Marks every config entry at a destination `repo_ownership` records,
///    every content variant and every path spelling of it, as
///    `FindingKind::Inventory` with
///    `shadowed_by: Some(ConfigShadow::Repositories)`, an advisory entry
///    excepted.
pub fn settle_repo_ownership(snapshot: &mut InspectionSnapshot);

/// The invariants `settle_repo_ownership` establishes, checked without
/// changing anything. Returns the first violation by destination.
pub fn check_repo_ownership(snapshot: &InspectionSnapshot) -> Result<(), RepoOwnershipViolation>;
```

**Where it runs.**

- **Scan.** `finalize_cross_inspector` (`crates/pipeline/src/collect.rs:338-340`) calls `settle_repo_ownership` beside `shadow_unit_owned_paths`, each its own unconditional call, per the `cross-inspector-shadow-passes` skill. On a scan, step 1 changes nothing, since the collector already excludes those entries (§ 3.7), and every key has an owner (§ 3.2).
- **Aggregate.** `merge_snapshots` (`crates/core/src/aggregate/mod.rs:42`) calls it once, after every section is merged and before it returns (`mod.rs:259`). It therefore reads the selected representatives, and the serialized aggregate carries the result before either export path or refine reads it. The base-shipped re-exclusion in `merge_rpm_sections` (`merge.rs:1172-1175`) moves into step 1, as do the default repo file and rejected key exclusions of § 3.7, so no merge function writes an ownership fact.
- **Refine.** Not run. No op changes the content of `rpm.repo_files` or `rpm.gpg_keys` or a config row's shadow, and the repo arm includes only carriable entries (§ 6.1), so every projection keeps both invariants.

**At load.** `InspectionSnapshot::load` (`crates/core/src/snapshot.rs:116-122`) runs `check_repo_ownership` after the schema check and refuses a snapshot that fails it with a new `SnapshotError::RepoOwnership(RepoOwnershipViolation)`:

```rust
pub enum RepoOwnershipViolation {
    /// A config entry, not advisory, at a repo-owned destination that is
    /// not `Inventory` shadowed by `Repositories`.
    UnshadowedConfig { path: String },
    /// A config entry shadowed by `Repositories` at a destination the repo
    /// inventory does not record.
    ShadowWithoutOwner { path: String },
    /// An included default repo file, rejected key, base-shipped key, or
    /// key with no owner.
    UncarriableIncluded { path: String },
}
```

The message is `<path>: <rule>. inspectah always writes snapshots that hold this; this one was edited or written by another version. Re-scan the host, or re-aggregate the scans.` The load refuses rather than repairs: a repair cannot recover the disposition a shadow overwrote, so it would guess a decision, and only an edited file reaches a violation. Every reader that loads through `InspectionSnapshot::load`, `refine`, `aggregate` and a re-render from a snapshot, gets the check.

**Destinations.** Both steps and the check compare through `Destination::new`. A key path comes from a `gpgkey=` value, which can be written `/etc/pki/rpm-gpg//KEY` or with a `./` component, and the ostree branch of the config walk records paths without the leading `/` (`etc/yum.repos.d/custom.repo`, `crates/collect/src/inspectors/config/mod.rs:786`), while the RPM inventory records absolute paths. The config merge keys on the raw path, so an aggregate of an ostree host and a package-mode host holds two config entries for one destination, and step 2 shadows both. `Destination` and `PathError` move from `crates/pipeline/src/render/configtree.rs:59` to `crates/core/src/types/`, re-exported by the pipeline, because the pass and the check live in core; the stager's own methods on it (`relative`, `parent`, `is_unit_file_path`) stay in the pipeline. `shadow_matching_config_entries` (`collect.rs:383-399`) moves with it and compares by destination for the Units pass and this one alike. The API join uses the same normalization, done once on the server (§ 7.1). The advisory exception stays: an advisory config entry is never materialized, and shadowing it would erase its warning. An entry whose path `Destination::new` refuses has no destination: nothing shadows by it, and the stager already refuses to write it.

**A key with no owner.** Step 1 excludes it, and no repo op can include it, since no section owns it, so it never ships. That is the right default: no repo file the export copies names it, so nothing in the image needs it, which is § 6.4's steer for a key that exists only for the source host's repos. Its config row stays shadowed: its destination is still in the repo inventory, and making the row live again would need the disposition the per-host shadow overwrote. § 6.5 says how the export reports it, and § 7 how refine shows it.

**Shadows across hosts.** Step 2 reads the merged repo inventory, narrowed entries included, so a destination that any host's repo owns shadows every config variant at it, whatever the other hosts recorded. In the two-host case, `host-a` has an unowned key K under `/etc/pki/rpm-gpg/` that no repo names, so its config row is actionable, and `host-b` has the same K bytes and an unowned `custom.repo` naming K, so its row is inventory. The merged K repo entry has one host of two and is narrowed out; the merged config entry is one universal variant, cloned from whichever host sorts first. Step 2 makes that config entry inventory in both host orders, and K is decided by `custom`'s switch alone: excluded by default, as `custom.repo` is, included with it, and excluded with it again. Without step 2, `host-a` sorting first gave an actionable, included config row, and excluding `custom` dropped the repo copy while the config copy still staged K.

**The config walk is unchanged.** It keeps recording repo files and keys it would record today. The row carries `diff_against_rpm` for a modified RPM-owned `.repo` file, which is inventory an operator reads; dropping the walk's record would lose it for no gain.

### 3.5 The render: one writer by construction

- **`offers`** already skips a config entry that is not included (`configtree.rs:691-701`), so an `Inventory` config twin is never offered. It also stops offering a rejected key, whatever its disposition (§ 3.6). No path test is added to the render.
- **`StagedFileKind::is_inventory_record`** (`configtree.rs:621-623`) drops `RepoFile` and `GpgKey`, leaving `FirewallZone`, and its doc says so. The yield rule at `configtree.rs:1018-1030` then cannot resurrect an included config twin over a repo decision. A snapshot that carries an included config entry at a repo-owned destination is refused at load (§ 3.4). The existing rule 6 stays as the stager's own backstop for one built in memory: one write when the contents match, a `Conflict` withholding both and a FIXME when they differ, an honest outcome rather than a silent OR.
- **`repo_enabling.rs`.** `serving_loss`'s doc (`repo_enabling.rs:109-116`) says refine has no control for a repo file or key on its own and that a config row does not change the decision. Reword it to state the rule: the repo toggle is the one control for both. `toggle_restores` (`repo_enabling.rs:203-232`) re-stages the export as the include arm leaves it, so it follows § 6's include rule, carriable entries only. `toggleable` (`repo_enabling.rs:175-186`) moves to core beside `effective_definition`, because § 5.2 uses it to choose a row's control; the advice it gives names only a repo whose op the server accepts.
- **`repoless.rs`.** `InstallRepos::from_staged` (`repoless.rs:237-270`) reads the stager's decision and needs no change. Its `files` list picks up any staged `.repo` destination; after this change that is always a `RepoFile` write.

### 3.6 Usable and rejected keys

```rust
// crates/core/src/types/rpm.rs, a new field on RepoFile
/// Set on a key the scan refused to store. Its `content` is a placeholder,
/// and nothing stages it or counts it as a key in the image.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub rejected: Option<KeyRejection>,

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyRejection {
    /// No line begins with a PGP public key header.
    NotPgp,
    /// PGP private or secret key armor appears in it.
    PrivateKeyMaterial,
}
```

- `extract_gpg_keys` sets `rejected` on its two placeholder branches (`repos.rs:317-342`), which already record the key excluded. `UnreadReason` keeps its path and its warning text.
- A rejected key is never carriable (§ 3.2): the include arm and `toggle_restores` skip it, as they skip a base-shipped key, so excluding and including its repo leaves it excluded.
- `offers` never offers it, so it is never staged, `InstallRepos::copies_key` is never true for it, and `keys_included` cannot count it as the key a section needs. `RepoFile::is_in_image` returns false for it. A rejected key never carries `shipped_by_base`: `leave_out_keys_the_base_ships` already considers only included keys.
- `RepoIndex` provenance counts only a usable or base-shipped key as captured, so a section whose key was rejected is `Incomplete`, as for a key the scan could not read.

### 3.7 Initial state and who writes it

Once a snapshot is produced, by a scan or an aggregate merge, each ending with § 3.4's pass, exactly one thing writes a `rpm.repo_files` or `rpm.gpg_keys` disposition: the `ItemId::Repo` arm of `project_snapshot` (`session.rs:2840-2937`). The config row cannot: it is `Inventory`, so `validate_target` refuses `SetInclude` on it with `InventoryNotToggleable` (`session.rs:2140-2155`), and the same refusal is added for `SelectVariant`, `EditVariant` and `DiscardVariant` on a `Config` path any of whose entries is shadowed by `Repositories`, which `validate_select` (`crates/refine/src/aggregate/variant_ops.rs:171-218`) and the `EditVariant` arm (`session.rs:2363-2380`) accept today. A variant op there would be accepted and change nothing, since an `Inventory` entry is never offered; a user variant cloned from an `Inventory` template is `Inventory` too. No `ItemId` names a repo file or key. § 9 T3 and T7 assert it.

What the dispositions start as:

- **Scan.** A repo file is included unless it is a default repo file; a key is included unless the base image ships it or it is rejected (`repos.rs:71-77`, `repos.rs:317-342`, `repos.rs:367-398`). Only the `rejected` field is new.
- **Aggregate.** `merge_items` includes every universal item and narrows the rest (`merge.rs:591-602`). Step 1 of § 3.4's pass then excludes every entry no op can carry, a default repo file, a rejected key, a base-shipped key and a key with no owner, since none of those exclusions is about prevalence; the base-shipped re-exclusion at `merge.rs:1172-1175` moves into it. A rejected payload takes part in choosing a key's representative only when no host has a usable key at that path. A custom repo file that only some hosts have stays excluded by narrowing; that is a default like any other, and § 5 reads it.
- **Refine decisions.** Every repo starts kept (§ 5.1). A decision is never seeded from dispositions: the scan or aggregate default lives in the dispositions, and § 5's rule reads the decision and the dispositions separately.

## 4. The toggle's effect on each export path

The stager is the one decision for all three paths (`staged_writes`), and the Containerfile counts from it (`containerfile.rs:600-614`). The repo file and keys land under `config/`, which the Configuration section's directory `COPY` also covers, so "staged" and "ships" are the same thing.

| | Entry included (scan or aggregate default, or an include op) | Entry excluded (an exclude op, or the default) |
|---|---|---|
| Config tree | `config/etc/yum.repos.d/<file>.repo` and each key the export copies, at `config/<key path>`; one writer each | Neither |
| Custom Repositories section | Counts the file; `COPY config/etc/yum.repos.d/ /etc/yum.repos.d/` | Does not count it; the section is absent when the count is zero |
| GPG Keys section | The key's `COPY` (directory `COPY` for `/etc/pki/rpm-gpg/`, per-key `COPY` and `rpm --import` elsewhere) | No line for it; the section is absent when no key is left |
| Packages | The repo's included members in the install line | Members excluded by a full-control repo's cascade (unchanged); a repo-enabling or `--enablerepo` package that stays included and loses its route gets the existing demotion FIXME (`demote_unserved_repo_enabling`) |
| Configuration section | Nothing for these paths | Nothing for these paths |
| Default repo file, base-shipped key, rejected key | Never copied, whatever the repo's state; § 7 says why | Same |

Per path:

- **Refine export** (`POST /api/tarball`, `render_refine_export`): the table as written. The repo row's state, and the config row's read-only state, are what the export does.
- **CLI scan tarball** (`inspectah scan`, `render::render_all`): no toggle exists, so it renders the scan defaults. What changes is that a config twin no longer has a vote. A modified `epel.repo`'s config row was included by default and shipped the file through `ConfigFile`; now the same bytes ship through `RepoFile`, so for a non-default repo file the tree is unchanged. **One default changes:** a default repo file (`is_default_repo`: `redhat.repo`, `rhel.repo`, `centos.repo`, `rocky.repo`, `alma.repo`) is excluded in the repo inventory, and the config walk does not exclude `/etc/yum.repos.d/`, so where the walk recorded one as an included config row it shipped through `ConfigFile`. It now follows its repo inventory entry and does not ship. That is the intended default: the subscription flow mounts `redhat.repo` at build time and never bakes it in. Whether a real RHEL scan records `redhat.repo` as an included config row is read from source and not measured; T6 asserts the rule.
- **Aggregate** (`inspectah aggregate`): the scan column for each entry's merged default (§ 3.7), with one addition, § 6.4.

## 5. The repo decision and the no-op rule

### 5.1 Decision, carriage and members

A repo row reads three things, and the no-op rule reads the same three.

- **Decision**, `kept`: no repo op excluded the repo, or the last one included it (`excluded_sections_at`, `session.rs:3286-3310`, unchanged). It is what the operator asked for. Every repo starts kept, in a scan session and an aggregate session alike.
- **Carriage**: what the projected snapshot does with the repo's carriable entries (§ 3.2), as two counts, `carriage_count` and `carriage_included`. It can disagree with the decision both ways: a file the aggregate narrowed out is not carried while its repo is kept, and a file shared with a kept repo is carried while this repo is excluded.
- **Members**: the effective set, as `repo_effective` counts it today.

`recompute_view` computes all three once per view into a cached `RepoState`, beside `repo_effective`, so the no-op rule, the row and the switch read one value.

### 5.2 Which control a repo has

```rust
// crates/refine/src/repo_index.rs
/// What a repo's switch decides.
pub enum RepoControl {
    /// Members and carriage: the switch cascades to its effective members
    /// (§ 3.2 of the refine-ui-fixup spec) and moves its carriable entries.
    Full,
    /// Carriage only: its carriable entries, never its members.
    CarriageOnly,
    /// No switch: nothing the op could move.
    None,
}
```

- **`Full`** for a repo that is `toggleable` (not distro, and the definition dnf uses on the host is enabled) and has an effective member or a carriable entry.
- **`CarriageOnly`** for a repo that is not `toggleable` and owns a carriable entry, when it can keep one (§ 6.1: it is enabled in a file that defines it, or it is host-disabled with an attributed package), or when it owns a carriable entry that no repo with a `Full` control or a control from the first clause owns.
- **`None`** otherwise.

The carriage-only control exists because the decision gives the repo section every repo file and key, and a carriable entry that only a distro or host-disabled section owns would otherwise ship with no control at all once its config row is read-only. A non-default `company.repo` defining `[baseos]` for an internal mirror, a custom `.repo` file whose every section is disabled, and `epel-cisco-openh264.repo` are such files. The switch never cascades to members: a distro repo's members are the bulk of the install list, and a host-disabled repo's members already follow their route, losing it with the existing demotion FIXME when the file goes. The two clauses give a control to every section that can keep an entry (§ 6.1) and to the owners of an entry nothing can keep, such as a file whose every section is disabled or a key only a disabled section names. So every carriable entry has a control that can drop it, which is what § 9 T3 asserts. `[epel-debuginfo]` beside `[epel]` gets no control: `[epel]` reaches `epel.repo` and the key both name. This control is open decision 2 (§ 11).

### 5.3 The rule

```rust
/// Refuses a repo `SetInclude` that changes nothing. The op is replayed:
/// the timeline up to the cursor with the op appended is projected, as
/// `project_snapshot` projects the timeline, and compared with the current
/// projection. The op is accepted when it moves an effective member, the
/// disposition of any `rpm.repo_files` or `rpm.gpg_keys` entry, or the
/// decision of a repo whose control is not `None`.
fn check_repo_noop(&self, section_id: &str, include: bool) -> Result<(), RefineError>
```

Refusal texts, 422 `NoopTarget`:

- control `None`: `nothing to decide in '<id>': no effective member and no file or key the export can carry`
- include: `repo '<id>' is already kept, with its files and keys carried and every effective member included`
- exclude: `repo '<id>' is already excluded, with no effective member included and nothing more to drop`

**Why replay rather than a second model.** What an op moves is decided by the cascade and by § 6's keeper rules, against initial states that differ by path (an aggregate-narrowed file, a default repo file, a rejected key, a file another repo keeps). A predicate that restates those rules is a second model that can drift from them; replaying the op is the definition. `check_repo_noop` already projects once for a repo with no effective member (`session.rs:2731`). Undo and redo need nothing new: the timeline is the decision.

**Members still never read history.** Position 29 of the refine-ui-fixup spec stays true for members: whether an op moves a member is read from the projection, and the repo-exclusion history plays no part in it. History is read only as the decision, which only the repo op writes.

### 5.4 Which repos have a row

`build_repo_groups`, in both `crates/refine/src/projection/decisions.rs:513` and `crates/web/src/handlers.rs:256`, lists every section a package is attributed to, as today, and adds every section whose control is not `None`. A section with neither has no row; its owned entries still appear in `repo_owned` (§ 7.1), so a config row can always say what decides it.

This is what makes a never-attributed repo reachable, such as `epel-cisco-openh264` on an EL9 host with EPEL, or a bootstrap repo that serves a vendor `-release` package installed from `@commandline`.

### 5.5 The switch

The switch shows when `control` is not `none`. Its state and direction:

```ts
const membersAll = repo.effective_included === repo.effective_member_count;
const carriageAll = repo.carriage_included === repo.carriage_count;
const isChecked =
  repo.kept && carriageAll && (repo.control === "carriage_only" || membersAll);
// onChange posts !isChecked, as today
```

The server never refuses the direction the switch posts. Checked posts exclude, and `kept` is true and the control is not `none`, so the op moves the decision. Unchecked posts include, and either `kept` is false (the op moves the decision), or a carriable entry is excluded (the include arm includes it), or, for a full control, an effective member is excluded (the cascade includes it). The count terms are both zero-safe, so a zero-member repo reads by carriage alone and a repo that owns nothing carriable reads by members alone.

The member term stays for full controls. Replacing it with the decision alone is the open item `inspectah-repo-switch-reads-off-on-load-when-dependency-members-are-excluded`, out of scope here (§ 10).

## 6. Shared files

### 6.1 Keepers

A section **keeps** an entry it owns while it is kept (§ 5.1) and live. Live means one of:

- the host enabled it, in the file that defines it, or
- the host disabled it and an included, unlocked package is attributed to it: that package installs with `--enablerepo=<id>` and needs the file and the keys (`repoless::disabled_repo_install`).

The repo op applies it:

- **Exclude** excludes each carriable entry the repo owns that no other section keeps.
- **Include** includes every carriable entry the repo owns.

Keepers are read at the op, against the snapshot as projected at that point, as every cascade rule is. `RepoIndex::repo_file_carried` and `gpg_key_carried` become one `kept_by(destination, excluded, snapshot)`; `enabled_sections_by_file` and `enabled_sections_by_gpg_key` become one map from destination to owning sections, each with its file and whether that file enables it.

### 6.2 A key named by two repos, one excluded

**The key ships while another section keeps it.** The other repo cannot verify its packages without it, so dropping it would break a decision the operator kept to honor one they made elsewhere. That includes a host-disabled repo whose package installs with `--enablerepo`: excluding the enabled repo that shares its key no longer breaks that install. The excluded repo's view lists the keeper and its route (§ 7.1), so the operator sees why the key still ships.

### 6.3 One `.repo` file defining several repos, with mixed toggles

**The file ships whole while another section keeps it, and is dropped once none does.** A host-disabled section with no included package does not keep it: excluding `[epel]` drops `epel.repo` beside `[epel-debuginfo]`, as today. inspectah copies host files byte for byte and never edits a `.repo` file. Rewriting `enabled=` for one section would put a definition in the image that no one on the host wrote, and the alternative, `dnf config-manager --set-disabled`, needs `dnf-plugins-core` in the base, for the reason position 17 of the refine-ui-fixup spec gives.

What changes is that the operator is told. The excluded section stays defined, and enabled if the host enabled it, in a file that ships, so:

- the Custom Repositories section gets one comment line per such file, built through `comment::join_lines`, with each value through the existing comment sanitizer:
  `# <path> also defines repo <excluded ids>, excluded in refine; the file is copied whole for repo <keeping ids>, so <excluded ids> stays defined as the host had it`
- the refine export adds an `ExportWarning::notice` with the same text.

The CLI tarball never has an excluded repo, so the line never appears there.

### 6.4 A key also used by something other than a repo

**Not detected, and not needed for the rule to be sound.** A key enters `rpm.gpg_keys` only because a section named it (§ 3.2). A key that only something else uses stays an ordinary config row with its own control. What is not detected is a key a repo names that something else also reads, such as a `keyPath` in `/etc/containers/policy.json`. Detecting it would mean a path search through every config file's content, which is the render-time string matching this design avoids, and in practice the key such a policy names is the vendor release key, which the base image ships (`shipped_by_base`) and the repo toggle never copies or drops. A custom key used that way follows its repo, the steer on record being that a key that only exists for the source host's repos does not belong in the image.

### 6.5 Content variants in an aggregate

A fleet aggregate keeps content variants on a config entry but one representative payload on a repo file or key (`merge_items`, `crates/core/src/aggregate/merge.rs:542-600`): the most prevalent payload, ties to the first host by name. Since `9bf5fb28` an included config variant decided a repo file's content (`crates/cli/tests/fleet_repo_precedence_test.rs`). With the config row read-only, the stager writes the representative.

The merge keeps no record of which hosts had the representative's bytes: `AggregatePrevalence::hosts` lists every host with the path. So the merge records it, without selecting anything new:

```rust
// crates/core/src/types/aggregate.rs, a new field on AggregatePrevalence
/// For an item merged to one representative payload, the hosts whose
/// payload is the representative's, sorted. Absent when every host in
/// `hosts` had the same payload.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub payload_hosts: Option<Vec<String>>,
```

`merge_items` fills it in its non-variant branch from the `payload_counts` it already builds. When a repo-owned entry that ships has `payload_hosts`, the Containerfile's Custom Repositories or GPG Keys section says so in one comment line, built as § 6.3's is:

`# <path>: <k> of <count> hosts with this file have a different version; the export copies the version on <hosts>`

`<hosts>` names up to three hosts, then `and <m> more`. The line reads the merged entry, so it covers an unmodified RPM-owned file that two `-release` versions put on different hosts, which no config walk records.

**A key no copied version names.** Representative selection can leave a key that every host collected named by no representative (§ 3.2); § 3.4's pass excludes it and no op reaches it. Every export of such an aggregate adds one `ExportWarning::notice` per such key, which the refine export puts in `x-export-warnings` and the CLI prints:

`<path> is not copied: no version of a repo file that the export copies names it. <hosts> have it because a different version of a repo file names it there.`

`<hosts>` reads the key's `AggregatePrevalence::hosts` and is shortened as above. The aggregate view does not render owner text on config rows in this release (§ 7.3), so this notice is what tells the operator; `RepoOwnedExport::NoOwner` (§ 7.1) still exists so that `repo_owned` has a state for every entry a snapshot can hold.

This keeps "nothing carried without the operator being told" for the fleet case without designing variant selection for repo files, which belongs with the fleet work that follows single-host correctness. The default is open decision 1 (§ 11).

## 7. The refine half (frontend)

This section is the frontend implementer's. It stays within the existing patterns: the config row uses the shadow-owner pattern the Units shadow introduced (`crates/web/ui/src/components/shadowOwner.ts`), and the repo row keeps its layout in both `RepoBar.tsx` and `RepoGroupHeader.tsx`. It is not a redesign of the Repositories section.

### 7.1 What the API returns

`RepoGroupInfo` (`crates/web/src/web_types.rs:191-210`, `crates/web/ui/src/api/types.ts`) changes:

```rust
pub struct RepoGroupInfo {
    pub section_id: String,
    pub provenance: RepoProvenance,
    pub is_distro: bool,
    pub tier: RepoTier,
    pub package_count: usize,
    pub effective_member_count: usize,
    pub effective_included: usize,
    pub effective_unlisted: usize,
    /// What the switch decides (§ 5.2). `none` renders no switch.
    pub control: RepoControl,
    /// The repo's decision: no repo op excluded it, or the last one
    /// included it (§ 5.1). Renamed from `enabled`, which collided with the
    /// host's `enabled=` and read as if it said what the files do.
    pub kept: bool,
    /// The carriable entries this repo owns (§ 3.2), and how many of them
    /// the projected snapshot includes.
    pub carriage_count: usize,
    pub carriage_included: usize,
    /// The destination of every entry this repo owns, carriable or not;
    /// each is the `destination` of one `repo_owned` entry. Sorted.
    pub owns: Vec<String>,
}
```

The view gains `repo_owned: Vec<RepoOwnedEntry>`, one entry per destination of `rpm.repo_files` and `rpm.gpg_keys`, whether or not any owner has a row, and whether or not it has an owner at all:

```rust
#[derive(Serialize, Clone, Debug)]
pub struct RepoOwnedEntry {
    /// `Destination::new` of the path: the identity every join uses.
    pub destination: String,
    /// Every spelling the snapshot records at this destination: the
    /// inventory entries' paths and the paths of the config rows shadowed
    /// by it (an ostree scan's `etc/yum.repos.d/x.repo`, a `gpgkey=`
    /// written with `//`). A config row joins on its own `path` being in
    /// this list, so the client never normalizes a path.
    pub recorded_paths: Vec<String>,
    pub kind: RepoOwnedKind,
    /// Every section that owns it, by exact spelling, sorted, from
    /// `repo_ownership` (§ 3.4). Empty only for `export: no_owner`.
    pub owners: Vec<RepoOwner>,
    /// What this export does with it, and why.
    pub export: RepoOwnedExport,
}

#[derive(Serialize, Clone, Debug)]
pub struct RepoOwner {
    pub section_id: String,
    /// The owner's row control; `none` when it has no row or no switch.
    pub control: RepoControl,
    pub kept: bool,
}

#[derive(Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "snake_case")]
pub enum RepoOwnedKind {
    RepoFile,
    GpgKey,
}

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RepoOwnedExport {
    /// Included and staged. `kept_by` names the sections keeping it for an
    /// excluded owner (§ 6.1); empty when no owner is excluded.
    Ships { kept_by: Vec<Keeper> },
    /// Carriable and excluded: by a repo op, or by the scan or aggregate
    /// default (§ 3.7).
    NotShipped,
    /// A key the base image ships, with the package that ships it.
    BaseImage { package: String },
    /// A default repo file: never copied (§ 3.2).
    DefaultRepoFile,
    /// A key the scan refused to store (§ 3.6).
    Rejected { reason: KeyRejection },
    /// A key no repo file in the snapshot names: only an aggregate has
    /// one (§ 3.4). `hosts` are the hosts that have it.
    NoOwner { hosts: Vec<String> },
    /// Included, and withheld by the stager, with its reason.
    Withheld { reason: String },
}

#[derive(Serialize, Clone, Debug)]
pub struct Keeper {
    pub section_id: String,
    /// Set when the keeper is host-disabled and keeps the entry for this
    /// included package's `--enablerepo` install (§ 6.1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enablerepo_package: Option<String>,
}
```

`export` is read from the projected snapshot's dispositions and from the stager's plan for that snapshot (`staged_writes`, as `InstallRepos::from_staged` reads it), never from the repo's tier. `recompute_view` computes the plan once per view if it does not already.

**The Units join follows the same rule.** Once `shadow_matching_config_entries` compares through `Destination` (§ 3.4), an ostree scan's relative drop-in path is shadowed by an absolute `unit_dropins[].path`, and today's join on that path string (`shadowOwner.ts:37-47`) cannot find it. `DropInDecisionDto` gains `config_paths: Vec<String>`, the paths of the config rows its destination shadows, and `resolveShadowOwner` joins a `"units"` row on `config_paths.includes(entry.path)`.

A config row's `shadowed_by` gains the value `"repositories"` (`ConfigShadow` in `types.ts`).

The 422 `NoopTarget` texts change to § 5.3's.

### 7.2 The repo row

- **Switch:** shown and driven by § 5.5, in both `RepoBar.tsx` and `RepoGroupHeader.tsx`. `canToggleRepo` and `showToggle` become `control !== "none"`. A distro or host-disabled row with `control: "carriage_only"` keeps its badge text (`always included`, `disabled on host`) and gains the switch for its files.
- **Count text:** for a full control with members, as today, followed by `, repo files <i> of <n> carried` when `carriage_count` is not zero and `carriage_included` is less. With no members, or for a carriage-only control: `repo files <i> of <n> carried`. When `kept` is false, `excluded in refine` is appended. The `aria-label` keeps the section id first, as `7cc98658` requires. A `none` row keeps the `nothing to decide` text of the refine-ui-fixup spec, now `nothing to decide: no package to change and no file or key the export can carry`.
- **What it owns:** a compact line under the switch label listing each `owns` entry by basename, with the full destination as its accessible name, and one state word from that entry's `export`: `not shipped`, `kept by <ids>`, `base image`, `default repo file`, `rejected`, `withheld`, or nothing when it simply ships. `RepoGroupHeader` gets no line-through: § 6.3 of the refine-ui-fixup spec removes the `--disabled` strike-through, and `excluded in refine` in the count text carries the decision.
- **A reveal target:** the repo row gets an item id, `repo:<section_id>`, in the existing reveal plumbing (`revealItemId`), so a config row can route to it.

### 7.3 The config row

`resolveShadowOwner` gains a `"repositories"` branch beside `"units"`. It takes the view's `repo_owned` and finds the entry whose `recorded_paths` holds the row's `path`. There is always one: the pass shadows only destinations of the repo inventory, and `repo_owned` lists every one.

- **label:** `Decided under Repositories` (§ 8: the server's `DECIDED_UNDER_REPOSITORIES` summary uses the same words)
- **state:** `Ships` for `ships`, `Not shipped` for every other export state
- **explanation**, by `export.state`, with `<ids>` the owners:
  - `ships`: `This file belongs to repo <ids>. Its switch under Repositories, in Packages, decides whether the file ships.` When `kept_by` is not empty: `Repo <excluded ids> is excluded, but repo <keeper ids> still needs this file, so it ships.`, with `, whose package <name> installs with --enablerepo` after a keeper that has `enablerepo_package`.
  - `not_shipped`: `This file belongs to repo <ids>, and the export leaves it out. Its switch under Repositories decides whether it ships.`
  - `base_image`: `The base image ships this key (<package>), so the export never copies it.`
  - `default_repo_file`: `This is a default repo file, which the export never copies. The build is expected to get it from the base image or the subscription mount.`
  - `rejected`: `The scan did not store this file because it <is not a PGP public key | holds private key material>, so the export never copies it.`
  - `withheld`: `The export withholds this file: <reason>.`
  - `no_owner`: `Every host that has this key had it because a repo file named it, but no version of a repo file that the export copies names it, so the export never copies it. Hosts with it: <hosts>.`
- **target:** `{ sectionId: "packages", itemId: "repo:<first owner by section id whose control is not none>" }`, or null when no owner has a switch or there is no owner (a key with `no_owner`; a default repo file whose sections are distro, for instance)

The row has no include checkbox: the server already refuses `SetInclude` on an `Inventory` entry. The single-host view gets this; the aggregate view renders shadowed config rows as it does for the Units shadow today, and does not change in this release.

### 7.4 Frontend tests

In § 9, T8.

## 8. Snapshot and API changes

**Snapshot:**

- `ConfigShadow::Repositories`, serialized `"repositories"`. Every config entry at a repo-owned destination, in every content variant and path spelling, is `FindingKind::Inventory` with `shadowed_by: Some(ConfigShadow::Repositories)`, in a scan and in an aggregate alike (§ 3.4).
- An aggregate's key that no repo file in it names is excluded (§ 3.4). No field records it: `repo_ownership` derives it from the repo files, as it derives every owner.
- `InspectionSnapshot::load` refuses a snapshot that breaks § 3.4's invariants, with `SnapshotError::RepoOwnership`.
- `RepoFile::rejected: Option<KeyRejection>` (§ 3.6) and `AggregatePrevalence::payload_hosts: Option<Vec<String>>` (§ 6.5), both omitted when absent.
- `rpm.repo_files` holds no file that defines no section (§ 3.1), and `rpm.gpg_keys` no key that only a replaced or headerless `gpgkey=` named.
- `SCHEMA_VERSION` and `MIN_SCHEMA` rise together to the next value, unless another beta.7 change has already raised them in this release, in which case this change rides that bump. A schema-28 snapshot is refused, whichever bump carries this change: it parses, but its config twins are live and its rejected keys unmarked, so rendering it re-creates the OR this spec removes. That is semantic staleness, the same argument as the 23 to 24 bump in the `snapshot-schema-versioning` skill.
- `config_ref_summary` (`crates/refine/src/projection/types.rs:541-546`) gains `Some(ConfigShadow::Repositories) => DECIDED_UNDER_REPOSITORIES`, with `pub const DECIDED_UNDER_REPOSITORIES: &str = "decided under Repositories";`.

**Refine API:**

- `RepoGroupInfo.enabled` is renamed `kept`; `control`, `carriage_count`, `carriage_included` and `owns` are added (§ 7.1).
- The view gains `repo_owned`; `DropInDecisionDto` gains `config_paths`.
- `repo_groups` lists every section whose control is not `none` (§ 5.4).
- `SetInclude` on a `Repo` follows § 5.3: accepted for a repo with no effective member when it moves the decision or an entry, and for a distro or host-disabled repo with a carriage-only control; the `NoopTarget` texts change.
- `SetInclude` on a repo-owned `Config` row is refused with `InventoryNotToggleable`, as for any inventory row, and so are `SelectVariant`, `EditVariant` and `DiscardVariant` on it (§ 3.7).
- Saved sessions keep their ops. A saved aggregate session whose snapshot predates the schema bump is refused with its snapshot, so no saved session over a schema-<N> snapshot holds a variant op on a repo-owned row.

**Changelog** (`## [Unreleased]`). The released beta.6 entry about excluding a repo stays as it is; these describe the net change since then.

```markdown
### Changed
- **Breaking: snapshot schema is now <N>**: a `.repo` file or GPG key that a repo names is decided by that repo's switch, so its Configuration Files row is read-only and says "decided under Repositories". A `redhat.repo` or other default repo file that the scan recorded as a configuration file no longer ships. `refine` and `aggregate` load only schema <N>; re-scan each host and re-aggregate.

### Fixed
- **A repo's switch is the one control for its `.repo` file and keys**: a file or key that also had a Configuration Files row shipped when either control was on. A repo with no packages left to change can now still be switched off, including a distribution or disabled repo that owns a file. A file or key another repo you kept still needs keeps shipping, and the export says why.
- **Aggregate exports follow each repo's switch for its files and keys**: a `redhat.repo` every host had, a key file the scan refused to store, or a key no copied `.repo` file names was included in an aggregate, and a key one host's repo named could ship as a plain configuration file after its repo was excluded. Each now follows its repo, and the export names any key it leaves out that way.
```

## 9. Tests

Every host-level test builds its snapshot from collector output: `MockExecutor` with the real inspectors through `collect()`, over the `UnitHost` builder in `crates/cli/tests/support/unit_hosts.rs`, as `units_scan_test.rs` and `fleet_repo_precedence_test.rs` do. No hand-written `RepoFile` or `ConfigFileEntry` stands in for a scan in T2 to T7: a hand-built fixture can encode a layout the collectors cannot produce, and that has passed review three times on this codebase. `UnitHost` gains:

- `package_from(name, repo)`: a package whose `%{from_repo}` is `repo` (today `package` always answers `appstream`);
- `enabled_repos(ids)`: the answer to `dnf repolist --enabled -q` (`get_enabled_repos`, `crates/collect/src/inspectors/rpm/repoless.rs:36`);
- `available(name, repo)`: a `%{name} %{reponame}` line in the answer to `get_available_packages`'s `dnf repoquery --available` (`repoless.rs:87`).

The host used throughout, unless a test says otherwise:

- `/etc/yum.repos.d/custom.repo`, unowned, `[custom]` enabled, `gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-custom`
- `/etc/pki/rpm-gpg/RPM-GPG-KEY-custom`, unowned, a public key block
- `/etc/yum.repos.d/epel.repo`, owned by `epel-release` and reported modified by `rpm -Va`, with `[epel]` enabled and `[epel-debuginfo]` disabled, both naming `RPM-GPG-KEY-EPEL-$releasever_major`, and the key at its expanded path, owned by `epel-release`
- `/etc/pki/rpm-gpg/RPM-GPG-KEY-orphan`, unowned, named by no repo
- `/etc/yum.repos.d/custom.repo.rpmnew`, unowned
- members: `tool-a` and `tool-b` from `custom`, `bat` from `epel`

The parser inputs, all unowned, added for T1 to T3:

- `cont.repo`: `[cont]` with `gpgkey=file:///etc/pki/rpm-gpg/A` and an indented continuation line `file:///etc/pki/rpm-gpg/B`
- `super.repo`: `[sup]` with `gpgkey=file:///etc/pki/rpm-gpg/OLD`, then `gpgkey=file:///etc/pki/rpm-gpg/NEW`
- `bare.repo`: `[bare]` with `gpgkey=/etc/pki/rpm-gpg/BARE`
- `pre.repo`: `gpgkey=file:///etc/pki/rpm-gpg/PRE` before the first header, then `[pre]` naming no key
- `empty.repo`: comments only
- `reject.repo`: `[rej]` naming `RPM-GPG-KEY-notpgp` (plain text) and `RPM-GPG-KEY-priv` (a public block and private key armor)
- each named key file present and unowned, and `OLD` and `PRE` present too

**T1. One parser** (`crates/collect`). On that host: `A`, `B`, `NEW`, `BARE`, the EPEL key at its expanded path, `RPM-GPG-KEY-custom` and the two `reject.repo` keys are in `rpm.gpg_keys`; `OLD`, `PRE` and `RPM-GPG-KEY-orphan` are not; `empty.repo` is not in `rpm.repo_files`. The invariant over the whole scan: every `rpm.repo_files` entry defines at least one section, and every `rpm.gpg_keys` entry is named, through `is_key_named_by`, by a `GpgKeyRef::Local` of a section `parse_repo_sections` returns for some `rpm.repo_files` entry. `RPM-GPG-KEY-notpgp` and `RPM-GPG-KEY-priv` are excluded with `rejected` set to `not_pgp` and `private_key_material` and placeholder content. `[rej]`'s provenance is `Incomplete`.

**T2. The ownership pass on a scan** (`pipeline::collect::finalize_tests`). The config rows for `custom.repo`, `epel.repo`, `RPM-GPG-KEY-custom`, `A`, `B` and `BARE` are `Inventory` with `ConfigShadow::Repositories`; the rows for `RPM-GPG-KEY-orphan`, `custom.repo.rpmnew`, `empty.repo`, `OLD` and `PRE` stay actionable and unshadowed. One case writes the key reference as `file:///etc/pki/rpm-gpg//RPM-GPG-KEY-custom` and is still shadowed. One case runs the config walk's ostree branch, which records `etc/yum.repos.d/custom.repo`, and that row is shadowed. Two direction tests, as the skill requires: the repo pass runs on a host where the units pass returns early (no units section), and the units pass runs where the repo pass does (no RPM section).

**T3. Refine four-state contract and reach** (`crates/cli/tests`, new `repo_ownership_test.rs`). For each of `custom.repo`, `epel.repo` and `RPM-GPG-KEY-custom`, drive the repo switch and the config row through `RefineSession`, both directions each. Assert the contract, not a table: `SetInclude` on the config row is refused with `InventoryNotToggleable` every time; every repo op the session accepts is obeyed by the staged tree **and** the Containerfile; no path is written twice; `x-export-warnings` holds no `failure`. After every op, the projected `repo_files` and `gpg_keys` dispositions differ from the scan's only where a repo op moved them (the one-writer rule, § 3.7). **Reach:** for every carriable entry whose config row is shadowed, excluding every owner whose control is not `none`, in sorted order and in reverse, leaves it out of the staged tree; the rejected keys, a base-shipped key and a default repo file (T6's second host) are never staged in any state, and excluding then including `[rej]` leaves both rejected keys excluded, unstaged, and failing `keys_included`.

**T4. Zero-member repos and initial exclusions, refine export** (same file):

- every member of `custom` excluded by per-package ops: the row reads unchecked (members), `kept: true`, `carriage 2 of 2`; excluding the repo is accepted, `custom.repo` and its key leave the tree and the Containerfile, and the row reads `kept: false`; including it again is accepted, both return, and the unchanged cascade includes `tool-a` and `tool-b` again; undo and redo each move the whole step;
- every member locked: the same, except the locked members stay excluded after the include;
- never attributed: a host with `vendor-release` installed from `@commandline` that owns `/etc/yum.repos.d/vendor.repo` defining `[bootstrap]` and its key, `enabled_repos(["bootstrap"])`, `available("vendor-release", "bootstrap")`, and no package from `bootstrap`. The scanned snapshot has `repo_enabling_from == ["bootstrap"]` for `vendor-release` before any op, and the export installs it from that repo. `/api/view`'s `repo_groups` lists `bootstrap` with `control: "full"`, zero member counts, `kept: true` and `carriage 2 of 2`; excluding it is accepted, drops `vendor.repo` and its key, and demotes `vendor-release` with the existing FIXME;
- aggregate narrowing: two hosts, `custom.repo` and its key on one only, no attributed package. The aggregate row reads `kept: true`, `carriage 0 of 2`, switch unchecked; including is accepted and stages both; excluding is then accepted (the decision) and drops both; undo and redo restore each state;
- a default repo file: a host whose `redhat.repo` defines distro sections and an enabled `[codeready-builder-for-rhel-9-x86_64-rpms]` with one member. That row's `carriage_count` counts no file; its include and exclude move the member only; `redhat.repo` is never staged;
- refusals: a repo with control `none` is 422 with `nothing to decide` in both directions; including a kept repo that is fully carried with every effective member included, and excluding an excluded repo with nothing left to drop, are 422 with § 5.3's texts; the switch state of § 5.5 never posts any of them.

**T5. Shared files** (same file):

- two files, `a.repo` with `[a]` and `b.repo` with `[b]`, naming one key, no members in either: excluding `a` is accepted though no staged byte changes, `a` reads `kept: false`, and the key's `repo_owned` entry is `ships` with `kept_by: [{ section_id: "b" }]`; excluding `b` too drops it; undo and redo move each step;
- one file `ab.repo` with `[a]` and `[b]` enabled and `[a-source]` disabled, members in both: excluding `a` keeps the file whole, the Containerfile carries § 6.3's line and the export warnings its notice; excluding `b` too drops the file, and the disabled section does not keep it;
- the `--enablerepo` keeper: `a.repo` with `[a]` enabled and `d.repo` with `[d]` disabled, naming one key, and `pd` attributed to `d`, included. Before any op the export installs `pd` with `--enablerepo=d`. Excluding `a` keeps the key, with `kept_by: [{ section_id: "d", enablerepo_package: "pd" }]`, and `pd` still installs with `--enablerepo=d`. With `pd` excluded first, excluding `a` drops the key and `d`'s row reads carriage incomplete. `d`'s row has `control: "carriage_only"`; excluding it drops `d.repo` and leaves `pd`'s disposition alone.

**T6. CLI export** (same file):

- `render::render_all` over the scanned host: each repo file and key is staged once, at `config/<path>`, from its `RepoFile` entry, with the Custom Repositories and GPG Keys `COPY` lines; `custom.repo.rpmnew`, `empty.repo` and `RPM-GPG-KEY-orphan` are staged as config files; the rejected keys are staged nowhere; `epel.repo`'s staged bytes equal the modified file on the host; a second host adds an unowned `/etc/yum.repos.d/redhat.repo` with a distro section, whose config row is shadowed and which is not staged;
- `render::render_all` over a refine session's `snapshot_projected()` after excluding `custom`: `custom.repo` and its key are absent, and the tree under `config/etc/yum.repos.d/` and `config/etc/pki/rpm-gpg/` equals the refine export's for the same session (the parity `staged_writes_parity_test.rs` keeps for the rest of the tree);
- an aggregate of two hosts that both have the unowned `redhat.repo` and `reject.repo`'s keys: neither is staged, and both entries are excluded in the merged snapshot.

**T7. Aggregate.** `fleet_repo_precedence_test.rs` is rewritten to the new contract: the two config variants are `Inventory` and their `SetInclude` is refused; the export stages the repo row's representative payload in both host orders; the Containerfile carries § 6.5's line naming the host whose version it copies. Added cases:

- three hosts, two with one `custom.repo` and one with another: the representative is the two-host version, `payload_hosts` names those two, and the line reads `1 of 3 hosts with this file have a different version; the export copies the version on <h1>, <h2>`;
- an unmodified, RPM-owned `epel.repo` from two `epel-release` versions, which no config walk records: the line still prints;
- a key whose content differs between hosts: the GPG Keys section carries the line.

The ownership cases below build each host with `UnitHost` through `collect()` and merge with `merge_snapshots`, as `fleet_repo_precedence_test.rs` does. **After every fleet case in T7**, on the merged snapshot and on it serialized and reloaded through `InspectionSnapshot::load`: `check_repo_ownership` passes; every config entry at a destination of `rpm.repo_files` or `rpm.gpg_keys` is `Inventory` with `ConfigShadow::Repositories`; and every included, carriable entry is reachable, which means it has an owner whose control is not `none`, and excluding every such owner, in sorted order and in reverse, leaves it out of the staged tree.

- **Correlated keys over three hosts.** `h1`, `h2` and `h3` each have unowned `x.repo`, `y.repo` and `z.repo` defining enabled `[x]`, `[y]` and `[z]` with a `baseurl`, each naming one key: on `h1`, `x` names B and `y` and `z` name A; on `h2`, `y` names B; on `h3`, `z` names B. A and B are unowned public keys under `/etc/pki/rpm-gpg/`, with the same bytes on every host. The three representatives each name A. A is included and owned by `x`, `y` and `z`. B is excluded, its `repo_owned` entry has no owner and `export: no_owner` with all three hosts, its config row is `Inventory` shadowed by `Repositories`, and the export carries § 6.5's notice for it. B is staged in no state. Excluding `x`, `y` and `z` drops the three files and A, leaving nothing under `config/etc/yum.repos.d/` or `config/etc/pki/rpm-gpg/`; including them again restores the three files and A, never B. Each repo file carries § 6.5's version line. Run under two host orders, the names rotated, and the merged `rpm` and `config` sections are identical.
- **A key one host's repo owns.** `host-a` has an unowned key K under `/etc/pki/rpm-gpg/` that no repo names; `host-b` has the same K bytes and an unowned `custom.repo` with `[custom]` enabled, naming K. Run twice, once with hostnames that sort `host-a` first and once `host-b` first. In both, K's merged config entry is one variant, `Inventory` shadowed by `Repositories`, identical between the runs; K's repo entry is narrowed and owned by `custom`; `custom`'s row has `control: "full"`, `kept: true` and `carriage 0 of 2`; and neither `custom.repo` nor K is staged. **Direct config-op refusal:** `SetInclude` in both directions, `SelectVariant` to its one hash, `EditVariant` with new content and `DiscardVariant` on K's config row are each 422 `InventoryNotToggleable`, and `/api/ops` is unchanged. **Staged tree after repo exclusion:** including `custom` stages `custom.repo` and K once each, from their `RepoFile` entries; excluding it then leaves neither under `config/etc/yum.repos.d/` or `config/etc/pki/rpm-gpg/`, and the Containerfile has no Custom Repositories or GPG Keys line for them; undo and redo restore and re-apply each state.

**T8. Frontend** (`crates/web/ui`):

- `RepoBar` and `RepoGroupHeader`: a table over `control`, `kept`, the carriage counts and the member counts, asserting whether the switch shows, its checked state and the direction it posts, including zero members kept and not kept, members all included with `kept: false`, a kept repo with carriage incomplete, and a carriage-only distro row whose members are mixed;
- a stateful fake of the server's § 5.3 rule, extending the one in test 12 of the refine-ui-fixup spec, under which a click sequence over every state never receives a 422;
- the `owns` line: one state word per `export` state and accessible names, queried by accessible name, not by role;
- a config row with `shadowed_by: "repositories"`: the `Decided under Repositories` badge, the ship state, no checkbox, the explanation for each `export` state, and the link revealing `repo:<id>`; the join through `recorded_paths` for a relative ostree path and a `//` key path; a default `redhat.repo` with distro owners and no switch, which explains itself and has no link; a non-default `company.repo` defining `[baseos]`, which ships and links to its carriage-only row; a file whose every section is disabled, with no attributed package;
- a `"units"` config row with a relative ostree path, joined through `config_paths`;
- the hand-maintained e2e fixtures under `crates/web/ui/e2e/fixtures/` gain `control`, `kept`, the carriage counts, `owns`, `repo_owned` and `config_paths`, and lose `enabled` (the `e2e-fixtures-are-hand-maintained` skill).

**T9. Schema and load.** A schema-28 snapshot is refused by `refine` and `aggregate`. In `crates/core`, over the JSON of T7's K aggregate: three edited copies, one per `RepoOwnershipViolation` (K's config row made actionable and included; a `Repositories` shadow on a config path the repo inventory does not record; T7's B made included in the correlated aggregate), are each refused by `InspectionSnapshot::load` with that violation and path, and the unedited JSON loads. `settle_repo_ownership` run a second time on each merged snapshot of T7 changes nothing, and `RepoIndex::build`'s owner maps equal `repo_ownership`'s over T1's host and T7's correlated aggregate.

**Skills.** The implementation updates `cross-inspector-shadow-passes.md` (a second pass now exists, in core, and runs again after an aggregate merge because a per-host shadow does not survive the config merge; both passes compare by destination; repo files and keys leave its "known and unmeasured" list), `dnf-variable-expansion.md` (the gap it states is closed), and the `ConfigShadow` note in `snapshot-schema-versioning.md` if the bump is this change's.

## 10. Out of scope

- **The repo cascade pair:** `inspectah-repo-cascade-flattens-advisory-dispositions` and `inspectah-repo-switch-reads-off-on-load-when-dependency-members-are-excluded`. This spec does not change what the cascade writes to members. Interaction: an op that moves no member cannot flatten an advisory, and a carriage-only control never cascades; § 5.5 keeps the member term the switch item will replace, adding only the `kept` and carriage terms that item also needs.
- **The `/var` items** of the beta.7 list.
- **Renaming `ItemId::Repo`'s `path` field** (`inspectah-itemid-repo-path-field-carries-a-section-id`). The op's wire format is unchanged here.
- **File-level controls.** No `ItemId` for a repo file or key; the decision rules it out.
- **Variant selection for repo files in an aggregate** (§ 6.5), and the aggregate view's config-row owner label (§ 7.3).
- **The member half of host-disabled and distro repos.** A carriage-only control (§ 5.2) never moves members. A host-disabled repo's keys and file stay kept while one of its packages installs with `--enablerepo` (§ 6.1); whether such a repo's switch should also decide those packages is not settled here.
- **Whether the trailing-comment rule matches dnf** (§ 3.1).

Related, and partly closed by § 5: `inspectah-serving-repo-cannot-be-dropped-through-the-repo-control`. A serving repo with no attributed package now has a row and a switch that drops its file and keys (T4, never attributed). The package-membership question that item raises, whether the serving repo's switch should also decide the `-release` package, is unchanged.

## 11. Open decisions

1. **Aggregate repo file content (§ 6.5). Open: asked 2026-11-21, not yet answered.** Making the config row read-only reverses `9bf5fb28` for an aggregate: an operator can no longer pick which host's version of a repo file the export carries. The proposed default is the most prevalent version, which `merge_items` already picks (ties to the first host by name), with a Containerfile line naming the hosts whose version is copied, and variant selection for repo files deferred to the fleet work. Under that default, `SelectVariant`, `EditVariant` and `DiscardVariant` on the row are refused (§ 3.7). The alternative is to keep `SelectVariant` live on a shadowed config row (content from the config row, ship-or-not from the repo), which this spec does not design and the review advised against as a second content authority. Confirm the default, or ask for the alternative.
2. **A switch on distro and host-disabled rows that own a file (§ 5.2). Open.** Under the decision, every carriable repo file and key needs a repo control that can drop it, and a file only a distro or host-disabled section owns has none today. Options: (a) a carriage-only switch on those rows, moving files and keys and never members (proposed); (b) leave such files unshadowed, so their Configuration Files row stays their control and the repo inventory copy yields to it in both directions, which gives two kinds of repo-owned path and needs a per-host test of switchability before the aggregate merge; (c) ship them at their default with no control and say so on the row, which leaves a carried file the operator cannot drop. Recommendation (a): it is the only one that keeps one owner per path and a control for every path. It is visible on common EPEL hosts: `epel-cisco-openh264`'s row gains a switch for its file.
