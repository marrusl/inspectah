# The Repo Section Owns Its `.repo` Files and GPG Keys

## Metadata

| Field    | Value |
|----------|-------|
| Date     | 2026-11-21 |
| Status   | Proposed |
| Release  | 0.9.0-beta.7 |
| Decision | The repo section owns both the `/etc/yum.repos.d/*.repo` file and the keys its `gpgkey=` names. The repo toggle decides whether its `.repo` file and those keys ship, and their Configuration rows become read-only inventory, "decided under Repositories". Position 26 of the refine-ui-fixup spec is reopened so a repo whose packages have all been excluded can still be toggled (Mark Russell, 2026-11-21). |
| Verified | Code citations checked against inspectah commit `102a9f77`. Source reading only; no new host run. |
| Supersedes | Position 26 of `2026-10-26-refine-ui-fixup.md`, rewritten in place to point here. |

## 1. Problem

A `.repo` file and a GPG key can each sit in two inventories with two live dispositions, and the stager ships the file when **either** is included.

- **Repo files.** The RPM collector reads every `.repo` file in `/etc/yum.repos.d/` into `rpm.repo_files` (`crates/collect/src/inspectors/rpm/repos.rs:40-87`). The config walk also records the same file in `config.files` when it is unowned or a modified RPM-owned file. `staged_writes` offers both (`crates/pipeline/src/render/configtree.rs:691-713`), and its "file two sections record" rule (`configtree.rs:1018-1030`) only makes the inventory copy yield to an **included** config entry. Config excluded plus repo included therefore ships the repo copy, and config included plus repo excluded ships the config copy. Measured on one polarity on 2026-11-01 with `x-export-warnings: []`.
- **GPG keys.** `extract_gpg_keys` reads every key a `gpgkey=file://` names into `rpm.gpg_keys` (`repos.rs:256-345`). An unowned key under `/etc/pki/rpm-gpg/` is also a config row, and the same OR rule applies. There is no `ItemId` for a key, so the config row is the only control in view, and it does nothing.
- **A repo with no member to move cannot be toggled.** `check_repo_noop` (`crates/refine/src/session.rs:2726-2768`) refuses a repo op whose effective member set is empty or already at the requested state. A repo whose packages are all excluded, all locked, or that has no package attributed to it keeps shipping its `.repo` file and keys, and refine offers no control that drops them. A repo with no attributed package is not listed at all: `build_repo_groups` (`crates/refine/src/projection/decisions.rs:513-570`) lists only sections that some package names.

The repo toggle already moves the file and key dispositions (`session.rs:2840-2937`, through `RepoIndex::repo_file_carried` and `RepoIndex::gpg_key_carried`). What is missing is that it is not the **only** thing that does, and that it refuses the case where it is the only thing left to decide.

## 2. What this spec settles

1. One owner: a `.repo` file in `rpm.repo_files` and a key in `rpm.gpg_keys` are decided by the repo toggle and by nothing else (§ 3).
2. What the toggle does to each export path (§ 4).
3. A repo with no effective member keeps a working toggle; `check_repo_noop` and the switch state change to match (§ 5).
4. A rule for each kind of shared file (§ 6).
5. The refine half, separated for the frontend implementer (§ 7).
6. The snapshot and API changes, and the changelog line (§ 8).
7. The tests (§ 9).

Out of scope: § 10.

## 3. Ownership model

### 3.1 What a repo owns

A repo is a `[section]` by its exact spelling, as `RepoIndex::repo_identity` names it. It owns:

- every entry of `rpm.repo_files` whose content defines that section (`parse_repo_sections`), and
- every entry of `rpm.gpg_keys` that a `gpgkey=` path of that section names (`RepoFile::is_key_named_by`).

**Ownership is membership in the repo inventory.** Two collector facts make that exact:

- `rpm.repo_files` holds only `.repo` files in `/etc/yum.repos.d/`, read by `collect_repo_files`.
- `rpm.gpg_keys` has one source, `extract_gpg_keys`, run over the content of each entry of `rpm.repo_files` (`crates/collect/src/inspectors/rpm/mod.rs:200-215`). Every key in it was named by some section's `gpgkey=`. A key under `/etc/pki/rpm-gpg/` that no repo names never enters `rpm.gpg_keys`, and stays an ordinary config row with its own control.

So "is this path decided by a repo" is "is this path an entry of `rpm.repo_files` or `rpm.gpg_keys`", and "which repos decide it" is the `RepoIndex` maps that already exist (`repo_file_by_section`, `gpg_keys_by_section`, built once per session at `crates/refine/src/repo_index.rs:66-160`). § 9 T1 pins the collector invariant with a test, since the whole model rests on it.

### 3.2 The link: options

Three valid patterns. The spec review picks; the recommendation is option A.

**A. Inventory membership, plus a typed shadow on the config row (recommended).** Nothing new is stored on `RepoFile`. The repo-to-file link stays where it is today, derived from the file's content and the key's recorded `gpgkey_references`, through the one matcher `RepoFile::is_key_named_by`. The config-to-repo link is stored, once, at collect time: the config row is marked `FindingKind::Inventory` with `shadowed_by: Some(ConfigShadow::Repositories)` (§ 3.3). The render reads dispositions and never matches a config path against the repo inventory.

```rust
// crates/core/src/types/config.rs
pub enum ConfigShadow {
    Units,
    /// The file's path is an entry of `rpm.repo_files` or `rpm.gpg_keys`:
    /// the repo toggle decides whether it ships, and the repo inventory's
    /// entry is what the stager writes.
    Repositories,
}
```

**B. Stored back-links.** `RepoFile` gains `defines: Vec<RepoId>` for a repo file and `named_by: BTreeSet<(String, RepoId)>` for a key, written by the collector. Rejected: both are facts the content already holds, so they would be a second writer for one fact, and the aggregate merge would need a union rule for each, as it already needs one for `gpgkey_references` (`crates/core/src/aggregate/merge.rs:1163-1172`).

**C. Derived carriage.** Drop `disposition` from repo files and keys and compute whether each ships from a per-repo decision. This makes "a file included while every repo that defines it is excluded" unrepresentable, which is the right end state. Rejected for this release: the scan tarball has no per-repo decision to derive from, so the snapshot would need a new per-repo record that the CLI path, the aggregate merge and refine all write, and about ten readers of `RepoFile::disposition` change with it (`repo_enabling.rs`, `repoless.rs`, `containerfile.rs`, `configtree.rs`, `redaction/engine.rs`, the aggregate merge). Option A reaches one owner with the existing state, because after it the repo op is the only writer of those dispositions after the scan (§ 3.5).

### 3.3 The collect-time pass

`finalize_cross_inspector` (`crates/pipeline/src/collect.rs:338-340`) gains a second pass, in its own function, called unconditionally beside the first, per the `cross-inspector-shadow-passes` skill:

```rust
pub fn finalize_cross_inspector(snapshot: &mut InspectionSnapshot) {
    shadow_unit_owned_paths(snapshot);
    shadow_repo_owned_paths(snapshot);
}

/// A config entry at the path of a `.repo` file or GPG key the repo
/// inventory records is decided by the repo toggle (§ 3.1 of the repo
/// ownership spec). The inventory entry is what the stager writes.
fn shadow_repo_owned_paths(snapshot: &mut InspectionSnapshot) {
    let Some(rpm) = snapshot.rpm.as_ref() else {
        return;
    };
    let owned: HashSet<Destination> = rpm
        .repo_files
        .iter()
        .chain(&rpm.gpg_keys)
        .filter_map(|f| Destination::new(&f.path).ok())
        .collect();
    if owned.is_empty() {
        return;
    }
    shadow_matching_config_entries(snapshot.config.as_mut(), &owned, ConfigShadow::Repositories);
}
```

`shadow_matching_config_entries` (`collect.rs:383-399`) changes to compare through `Destination::new` for both passes, rather than raw strings. A key path comes from a `gpgkey=` value, which can be written `/etc/pki/rpm-gpg//KEY` or with a `./` component, while the config walk records the canonical path. The advisory exception stays: an advisory config entry is never materialized, and shadowing it would erase its warning.

The pass runs per host, before any aggregate merge, so every config variant of a repo-owned path in an aggregate carries the shadow.

**The config walk is unchanged.** It keeps recording repo files and keys it would record today. The row carries `diff_against_rpm` for a modified RPM-owned `.repo` file, which is inventory an operator reads; dropping the walk's record would lose it for no gain.

### 3.4 The render: one writer by construction

- **`offers`** already skips a config entry that is not included (`configtree.rs:691-701`), so an `Inventory` config twin is never offered. No path test is added to the render.
- **`StagedFileKind::is_inventory_record`** (`configtree.rs:621-623`) drops `RepoFile` and `GpgKey`, leaving `FirewallZone`, and its doc says so. The yield rule at `configtree.rs:1018-1030` then cannot resurrect an included config twin over a repo decision. A snapshot that carries an included config entry and an included repo entry at one path anyway (hand-edited) gets the existing rule 6: one write when the contents match, a `Conflict` withholding both and a FIXME when they differ. That is an honest outcome, not a silent OR.
- **`repo_enabling.rs`.** `serving_loss`'s doc (`repo_enabling.rs:109-116`) says refine has no control for a repo file or key on its own and that a config row does not change the decision. Reword it to state the rule: the repo toggle is the one control for both. `toggleable` (`repo_enabling.rs:175-186`) already ignores member counts, so after § 5 the advice it gives always names a toggle the server accepts. Today it can name a zero-member repo the server refuses.
- **`repoless.rs`.** `InstallRepos::from_staged` (`repoless.rs:237-270`) reads the stager's decision and needs no change. Its `files` list picks up any staged `.repo` destination; after this change that is always a `RepoFile` write.

### 3.5 Who writes the dispositions

After the scan, exactly one thing writes a `rpm.repo_files` or `rpm.gpg_keys` disposition: the `ItemId::Repo` arm of `project_snapshot` (`session.rs:2840-2937`). The config row cannot (it is `Inventory`, and `validate_target` refuses `SetInclude` on it with `InventoryNotToggleable`, `session.rs:2140-2155`), and no `ItemId` names a repo file or key. § 9 T3 asserts it.

The scan defaults do not change: a non-default repo file is included, a default one (`redhat.repo` and the like) excluded, and a key included unless the base image ships it (`repos.rs:71-77`, `repos.rs:367-398`).

## 4. The toggle's effect on each export path

The stager is the one decision for all three paths (`staged_writes`), and the Containerfile counts from it (`containerfile.rs:600-614`). The repo file and keys land under `config/`, which the Configuration section's directory `COPY` also covers, so "staged" and "ships" are the same thing.

| | Repo carried (scan default, or included in refine) | Repo excluded in refine |
|---|---|---|
| Config tree | `config/etc/yum.repos.d/<file>.repo` and each key the export copies, at `config/<key path>`; one writer each | Neither, unless § 6 keeps a shared file or key for another carried repo |
| Custom Repositories section | Counts the file; `COPY config/etc/yum.repos.d/ /etc/yum.repos.d/` | Does not count it; the section is absent when the count is zero |
| GPG Keys section | The key's `COPY` (directory `COPY` for `/etc/pki/rpm-gpg/`, per-key `COPY` and `rpm --import` elsewhere) | No line for it; the section is absent when no key is left |
| Packages | The repo's included members in the install line | Members excluded by the cascade (unchanged); a repo-enabling package that stays included and loses its serving repo gets the existing demotion FIXME (`demote_unserved_repo_enabling`) |
| Configuration section | Nothing for these paths | Nothing for these paths |
| A key the base image ships | Never copied; the existing "left to the base image" comment | Same |

Per path:

- **Refine export** (`POST /api/tarball`, `render_refine_export`): the table as written. The repo row's state, and the config row's read-only state, are what the export does.
- **CLI scan tarball** (`inspectah scan`, `render::render_all`): no toggle exists, so it renders the scan defaults, the left column. What changes is that a config twin no longer has a vote. A modified `epel.repo`'s config row was included by default and shipped the file through `ConfigFile`; now the same bytes ship through `RepoFile`, so for a non-default repo file the tree is unchanged. **One default changes:** a default repo file (`is_default_repo`: `redhat.repo`, `rhel.repo`, `centos.repo`, `rocky.repo`, `alma.repo`) is excluded in the repo inventory, and the config walk does not exclude `/etc/yum.repos.d/`, so where the walk recorded one as an included config row it shipped through `ConfigFile`. It now follows its repo inventory entry and does not ship. That is the intended default: the subscription flow mounts `redhat.repo` at build time and never bakes it in. Whether a real RHEL scan records `redhat.repo` as an included config row is read from source and not measured; T6 asserts the rule.
- **Aggregate** (`inspectah aggregate`): the left column, with one difference, § 6.4.

## 5. Repos with no effective member

### 5.1 The rule

The repo toggle decides two things: the repo's members, and whether its files and keys are carried. An op is a no-op only when it moves neither.

`check_repo_noop` becomes:

```rust
/// Refuses a repo `SetInclude` that would move neither a member (§ 3.2 of
/// the refine-ui-fixup spec) nor the repo's carriage (§ 5 of the repo
/// ownership spec). A repo with no effective member is still decided: its
/// files and keys follow the op.
fn check_repo_noop(&self, section_id: &str, include: bool) -> Result<(), RefineError> {
    let id = RepoId::new(section_id);
    let effective = self.repo_effective(&id);
    let carried = self.repo_carried(&id);
    let moves_member = if include {
        effective.included < effective.member_count
    } else {
        effective.included > 0
    };
    if moves_member || carried != include {
        return Ok(());
    }
    Err(RefineError::NoopTarget(if include {
        format!("repo '{section_id}' is already carried, with every effective member included")
    } else {
        format!("repo '{section_id}' is already excluded, with no effective member included")
    }))
}
```

`repo_carried` reads the same set `build_repo_groups` reads for `enabled` today: the repo is carried unless a repo op excluded it and no later op included it (`excluded_sections_at`, `session.rs:3286-3310`). Cache it in `recompute_view` beside `repo_effective`, so the no-op rule and the view read one value.

**Why history is the right source here, when position 29 of the refine-ui-fixup spec rejected it for members.** Position 29's objection is that per-package ops move members without touching the repo-exclusion history, so history cannot say whether an op moves a member. Carriage has no such second writer (§ 3.5): only the repo op moves a repo's files and keys, so the history of repo ops is exactly the carriage state. Members keep the count-based test.

The `nothing to decide in '<id>': all <n> members are locked` and `no package is attributed to it` refusals go away. Locked members are still outside the counts and the cascade; a repo whose members are all locked toggles its files and keys only.

### 5.2 Which repos have a row

`build_repo_groups`, in both `crates/refine/src/projection/decisions.rs:513` and `crates/web/src/handlers.rs:256`, adds a row for every section `RepoIndex` knows that has no attributed package and that the toggle accepts: not distro, provenance `Verified` or `Incomplete`. Its `package_count` and effective counts are zero. A host-disabled section with no package (`[epel-debuginfo]`) gets no row: it decides nothing.

This is what makes a never-attributed repo reachable at all, such as `epel-cisco-openh264` on an EL9 host with EPEL, or a bootstrap repo that serves a vendor `-release` package installed from `@commandline`.

### 5.3 The switch

The switch shows for a non-distro repo with provenance `Verified` or `Incomplete`, whatever its member count. Its state and direction:

```ts
const isChecked =
  repo.carried &&
  (repo.effective_member_count === 0 ||
    repo.effective_included === repo.effective_member_count);
// onChange posts !isChecked, as today
```

The server never refuses the direction the switch posts. Checked posts exclude, and `carried` is true, so the op moves carriage. Unchecked posts include, and either `carried` is false or a member is excluded, so the op moves something. This keeps position 29's guarantee and extends it to the zero-member case, where the count term alone would read checked forever.

The count term stays for repos with members. Replacing it with carriage alone is the open item `inspectah-repo-switch-reads-off-on-load-when-dependency-members-are-excluded`, out of scope here (§ 10).

## 6. Shared files

### 6.1 A key named by two repos, one excluded

**The key ships while any carried, host-enabled section names it** (`RepoIndex::gpg_key_carried`, unchanged). The included repo cannot verify its packages without it, so dropping it would break a decision the operator kept to honor one they made elsewhere. The excluded repo's row lists the key as "kept by" the other repo (§ 7.2), so the operator sees why it still ships.

### 6.2 One `.repo` file defining several repos, with mixed toggles

**The file ships whole while any host-enabled section it defines is carried, and is dropped once none is** (`RepoIndex::repo_file_carried`, unchanged). A host-disabled section does not keep the file. inspectah copies host files byte for byte and never edits a `.repo` file. Rewriting `enabled=` for one section would put a definition in the image that no one on the host wrote, and the alternative, `dnf config-manager --set-disabled`, needs `dnf-plugins-core` in the base, for the reason position 17 of the refine-ui-fixup spec gives.

What changes is that the operator is told. The excluded section stays defined, and enabled if the host enabled it, in a file that ships, so:

- the Custom Repositories section gets one comment line per such file, built through `comment::join_lines`, with each value through the existing comment sanitizer:
  `# <path> also defines repo <excluded ids>, excluded in refine; the file is copied whole for repo <carried ids>, so <excluded ids> stays defined as the host had it`
- the refine export adds an `ExportWarning::notice` with the same text.

The CLI tarball never has an excluded repo, so the line never appears there.

### 6.3 A key also used by something other than a repo

**Not detected, and not needed for the rule to be sound.** A key enters `rpm.gpg_keys` only because a `gpgkey=` named it (§ 3.1). A key that only something else uses stays an ordinary config row with its own control. What is not detected is a key a repo names that something else also reads, such as a `keyPath` in `/etc/containers/policy.json`. Detecting it would mean a path search through every config file's content, which is the render-time string matching this design avoids, and in practice the key such a policy names is the vendor release key, which the base image ships (`shipped_by_base`) and the repo toggle never copies or drops. A custom key used that way follows its repo, the steer on record being that a key that only exists for the source host's repos does not belong in the image.

### 6.4 Content variants in an aggregate

A fleet aggregate keeps content variants on a config entry but one representative payload on a repo file (`merge_items`, `crates/core/src/aggregate/merge.rs:542-600`). Since `9bf5fb28` an included config variant decided a repo file's content (`crates/cli/tests/fleet_repo_precedence_test.rs`). With the config row read-only, the stager writes the repo inventory's representative payload. When a repo-owned path's config variants differ from that payload, the Containerfile's Custom Repositories or GPG Keys section says so in one comment line:

`# <path>: <n> of <total> hosts have a different version of this file; the export copies the one on <hosts>`

This keeps "nothing carried without the operator being told" for the fleet case without designing variant selection for repo files, which belongs with the fleet work that follows single-host correctness. See "For Mark".

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
    /// Whether the repo's files and keys are carried: no repo op excluded
    /// it, or the last one included it (§ 5.1). Renamed from `enabled`,
    /// which collided with the host's `enabled=` and its `Disabled`
    /// provenance.
    pub carried: bool,
    /// The `.repo` files that define this repo and the keys its sections
    /// name, sorted by path.
    pub decides: Vec<RepoOwnedFile>,
}

#[derive(Serialize, Clone, Debug)]
pub struct RepoOwnedFile {
    pub path: String,
    pub kind: RepoOwnedKind,
    /// Whether this export ships it: the projected disposition is included
    /// and the stager writes it (`is_staged`, as `InstallRepos` reads it).
    pub ships: bool,
    /// Other carried repos that keep this shared file or key shipping (§ 6).
    /// Empty when only this repo decides it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub kept_by: Vec<String>,
    /// For a key the base image ships: the package that ships it. The
    /// export never copies such a key, whatever the repo's state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_image_ships: Option<String>,
}

#[derive(Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "snake_case")]
pub enum RepoOwnedKind {
    RepoFile,
    GpgKey,
}
```

`decides` is filled from `RepoIndex::repo_file_by_section` and `gpg_keys_by_section`, both by the repo's exact spelling. A config row's `shadowed_by` gains the value `"repositories"` (`ConfigShadow` in `types.ts`). No new field goes on config rows: the join key is the path, `repo_groups[].decides[].path`, as `unit_dropins[].path` is for the Units shadow.

The 422 `NoopTarget` texts change to § 5.1's.

### 7.2 The repo row

- **Switch:** shown and driven by § 5.3, in both `RepoBar.tsx` and `RepoGroupHeader.tsx`. `canToggleRepo` and `showToggle` drop the `effective_member_count > 0` term. The "nothing to decide: all members are locked" state text becomes unreachable and goes.
- **Count text:** with members, as today, followed by `, repo files excluded` when `carried` is false. With no members: `no packages, repo files carried` or `no packages, repo files excluded`. The `aria-label` keeps the section id first, as `7cc98658` requires.
- **What it decides:** a compact line under the switch label listing each `decides` entry by basename, with the full path as its accessible name. Each entry carries one state word: `not shipped` when `ships` is false, `kept by <ids>` when `kept_by` is not empty, `base image` when `base_image_ships` is set, and nothing when it simply ships. `RepoGroupHeader`'s line-through styling reads `carried` where it read `enabled`.
- **A reveal target:** the repo row gets an item id, `repo:<section_id>`, in the existing reveal plumbing (`revealItemId`), so a config row can route to it.

### 7.3 The config row

`resolveShadowOwner` gains a `"repositories"` branch beside `"units"`. It takes the view's `repo_groups` and finds every repo whose `decides` lists the entry's path:

- **label:** `Decided under Repositories` (§ 8: the server's `DECIDED_UNDER_REPOSITORIES` summary uses the same words)
- **state:** `Ships` or `Not shipped`, from that `decides` entry's `ships`
- **explanation:** `This file belongs to repo <ids>. Its switch under Repositories, in Packages, decides whether the file ships.` When every owner is a distro repo (a default repo file such as `redhat.repo`), which has no switch: `This file belongs to distribution repo <ids>, which the export leaves to the base image.`
- **target:** `{ sectionId: "packages", itemId: "repo:<first owner by section id>" }`, or null when no owner row is in the view

The row has no include checkbox: the server already refuses `SetInclude` on an `Inventory` entry. The single-host view gets this; the aggregate view renders shadowed config rows as it does for the Units shadow today, and does not change in this release.

### 7.4 Frontend tests

In § 9, T8.

## 8. Snapshot and API changes

**Snapshot:**

- `ConfigShadow::Repositories`, serialized `"repositories"`. A config entry at a repo-owned path is `FindingKind::Inventory` with `shadowed_by: Some(ConfigShadow::Repositories)`.
- `SCHEMA_VERSION` and `MIN_SCHEMA` rise together to the next value, unless another beta.7 change has already raised them in this release, in which case this change rides that bump. A schema-28 snapshot is refused: it parses, but its config twins are live, so rendering it re-creates the OR this spec removes. That is semantic staleness, the same argument as the 23 to 24 bump in the `snapshot-schema-versioning` skill.
- `config_ref_summary` (`crates/refine/src/projection/types.rs:541-546`) gains `Some(ConfigShadow::Repositories) => DECIDED_UNDER_REPOSITORIES`, with `pub const DECIDED_UNDER_REPOSITORIES: &str = "decided under Repositories";`.

**Refine API:**

- `RepoGroupInfo.enabled` is renamed `carried`; `decides` is added (§ 7.1).
- `repo_groups` lists zero-package toggleable repos (§ 5.2).
- `SetInclude` on a `Repo` is accepted for a repo with no effective member, and the `NoopTarget` texts change (§ 5.1).
- `SetInclude` on a repo-owned `Config` row is refused with `InventoryNotToggleable`, as for any inventory row.
- Saved sessions are unaffected: the ops are the same.

**Changelog** (`## [Unreleased]`):

```markdown
### Changed
- **Breaking: snapshot schema is now <N>**: a `.repo` file or GPG key that a repo names is decided by that repo, so its Configuration Files row is read-only and says "decided under Repositories". `refine` and `aggregate` load only schema <N>; re-scan each host and re-aggregate. The refine API's repo groups rename `enabled` to `carried` and list the files and keys each repo decides in `decides`.

### Fixed
- **Excluding a repo drops its `.repo` file and keys every time**: a repo file or key that also had a Configuration Files row shipped whenever either control was on, so excluding the repo or the row left the file in the image. The repo switch is now the one control, and a repo whose packages are all excluded or locked, or that has none, can still be switched off. A file that defines another repo you kept still ships, and the Containerfile and the export warnings say so.
```

## 9. Tests

Every host-level test builds its snapshot from collector output: `MockExecutor` with the real inspectors through `collect()`, over the `UnitHost` builder in `crates/cli/tests/support/unit_hosts.rs`, as `units_scan_test.rs` and `fleet_repo_precedence_test.rs` do. No hand-written `RepoFile` or `ConfigFileEntry` stands in for a scan in T2 to T7: a hand-built fixture can encode a layout the collectors cannot produce, and that has passed review three times on this codebase. `UnitHost` gains `package_from(name, repo)`, a package whose `%{from_repo}` is `repo` (today `package` always answers `appstream`), so a test can attribute members to a custom repo.

The host used throughout, unless a test says otherwise:

- `/etc/yum.repos.d/custom.repo`, unowned, `[custom]` enabled, `gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-custom`
- `/etc/pki/rpm-gpg/RPM-GPG-KEY-custom`, unowned, a public key block
- `/etc/yum.repos.d/epel.repo`, owned by `epel-release` and reported modified by `rpm -Va`, with `[epel]` enabled and `[epel-debuginfo]` disabled, both naming `RPM-GPG-KEY-EPEL-$releasever_major`, and the key at its expanded path, owned by `epel-release`
- `/etc/pki/rpm-gpg/RPM-GPG-KEY-orphan`, unowned, named by no repo
- `/etc/yum.repos.d/custom.repo.rpmnew`, unowned
- members: `tool-a` and `tool-b` from `custom`, `bat` from `epel`

**T1. The collector invariant** (`crates/collect`). On that host: every `rpm.gpg_keys` entry is named, through `is_key_named_by`, by a section of some `rpm.repo_files` entry, including the variable-expanded EPEL key. `RPM-GPG-KEY-orphan` is not in `rpm.gpg_keys`.

**T2. The shadow pass** (`pipeline::collect::finalize_tests`). The config rows for `custom.repo`, `epel.repo` and `RPM-GPG-KEY-custom` are `Inventory` with `ConfigShadow::Repositories`; `RPM-GPG-KEY-orphan` and `custom.repo.rpmnew` stay actionable and unshadowed. One case writes the key reference as `file:///etc/pki/rpm-gpg//RPM-GPG-KEY-custom` and is still shadowed. Two direction tests, as the skill requires: the repo pass runs on a host where the units pass returns early (no units section), and the units pass runs where the repo pass does (no RPM section).

**T3. Refine four-state contract** (`crates/cli/tests`, new `repo_ownership_test.rs`). For each of `custom.repo`, `epel.repo` and `RPM-GPG-KEY-custom`, drive the repo switch and the config row through `RefineSession`, both directions each. Assert the contract, not a table: `SetInclude` on the config row is refused with `InventoryNotToggleable` every time; every repo op the session accepts is obeyed by the staged tree **and** the Containerfile; no path is written twice; `x-export-warnings` holds no `failure`. After every op, the projected `repo_files` and `gpg_keys` dispositions differ from the scan's only where a repo op moved them (the one-writer rule, § 3.5).

**T4. Zero-member repos, refine export** (same file):
- every member excluded by per-package ops, then the repo excluded: accepted; `custom.repo` and its key leave the tree and the Containerfile; included again: both return, members stay excluded;
- every member locked: the same;
- never attributed: a host with `vendor-release` installed from `@commandline` that owns `/etc/yum.repos.d/vendor.repo` defining `[bootstrap]`, with no package from `bootstrap`. `/api/view`'s `repo_groups` lists `bootstrap` with `carried: true` and zero counts; excluding it is accepted, drops `vendor.repo` and its key, and demotes `vendor-release` with the existing FIXME;
- refusals: excluding an excluded zero-member repo, and including a carried zero-member repo, are 422 `NoopTarget` with § 5.1's texts; the switch state of § 5.3 never posts either.

**T5. Shared files** (same file):
- two files, `a.repo` with `[a]` and `b.repo` with `[b]`, naming one key: excluding `a` keeps the key, and `a`'s `decides` entry for it has `kept_by: ["b"]`; excluding `b` too drops it;
- one file `ab.repo` with `[a]` and `[b]` enabled and `[a-source]` disabled, members in both: excluding `a` keeps the file whole, the Containerfile carries § 6.2's line and the export warnings its notice; excluding `b` too drops the file, and the disabled section does not keep it.

**T6. CLI export** (same file):
- `render::render_all` over the scanned host: each repo file and key is staged once, at `config/<path>`, from its `RepoFile` entry, with the Custom Repositories and GPG Keys `COPY` lines; `custom.repo.rpmnew` and `RPM-GPG-KEY-orphan` are staged as config files; `epel.repo`'s staged bytes equal the modified file on the host; a second host adds an unowned `/etc/yum.repos.d/redhat.repo` with a distro section, whose config row is shadowed and which is not staged;
- `render::render_all` over a refine session's `snapshot_projected()` after excluding `custom`: `custom.repo` and its key are absent, and the tree under `config/etc/yum.repos.d/` and `config/etc/pki/rpm-gpg/` equals the refine export's for the same session (the parity `staged_writes_parity_test.rs` keeps for the rest of the tree).

**T7. Aggregate.** `fleet_repo_precedence_test.rs` is rewritten to the new contract: the two config variants are `Inventory` and their `SetInclude` is refused; the export stages the repo row's representative payload in both host orders; the Containerfile carries § 6.4's line naming the hosts.

**T8. Frontend** (`crates/web/ui`):
- `RepoBar` and `RepoGroupHeader`: a table over `carried`, `effective_member_count` and `effective_included` asserting the switch is shown, its checked state and the direction it posts, including zero members carried and not carried, and members all included with `carried: false`;
- a stateful fake of the server's § 5.1 rule, extending the one `§ 8.12` of the refine-ui-fixup spec uses, under which a click sequence over every state never receives a 422;
- the `decides` line: state words and accessible names, queried by accessible name, not by role;
- a config row with `shadowed_by: "repositories"`: the `Decided under Repositories` badge, the ship state, no checkbox, and the link revealing `repo:<id>`;
- the hand-maintained e2e fixtures under `crates/web/ui/e2e/fixtures/` gain `carried` and `decides` and lose `enabled` (the `e2e-fixtures-are-hand-maintained` skill).

**T9. Schema.** A schema-28 snapshot is refused by `refine` and `aggregate`.

**Skills.** The implementation updates `cross-inspector-shadow-passes.md` (a second pass now exists; repo files and keys leave its "known and unmeasured" list), `dnf-variable-expansion.md` (the gap it states is closed), and the `ConfigShadow` note in `snapshot-schema-versioning.md` if the bump is this change's.

## 10. Out of scope

- **The repo cascade pair:** `inspectah-repo-cascade-flattens-advisory-dispositions` and `inspectah-repo-switch-reads-off-on-load-when-dependency-members-are-excluded`. This spec does not change what the cascade writes to members. Interaction: a zero-member op moves no member, so it cannot flatten an advisory; and § 5.3 keeps the count term the switch item will replace, adding only the `carried` term that item also needs.
- **The `/var` items** of the beta.7 list.
- **Renaming `ItemId::Repo`'s `path` field** (`inspectah-itemid-repo-path-field-carries-a-section-id`). The op's wire format is unchanged here.
- **File-level controls.** No `ItemId` for a repo file or key; the decision rules it out.
- **Variant selection for repo files in an aggregate** (§ 6.4), and the aggregate view's config-row owner label (§ 7.3).
- **Host-disabled repos.** Their toggle stays refused (`Disabled` provenance); a package from one installs with `--enablerepo` and needs its key, which is why a host-disabled section's key is not dropped by any rule here.

Related, and partly closed by § 5: `inspectah-serving-repo-cannot-be-dropped-through-the-repo-control`. A serving repo with no attributed package now has a row and a switch that drops its file and keys (T4, never attributed). The package-membership question that item raises, whether the serving repo's switch should also decide the `-release` package, is unchanged.

## 11. For Mark

1. **Aggregate repo file content (§ 6.4).** Making the config row read-only reverses `9bf5fb28` for an aggregate: an operator can no longer pick which host's version of a repo file the export carries. The default here is the most prevalent version, with a Containerfile line naming the hosts that differ, and variant selection for repo files deferred to the fleet work. The alternative is to keep `SelectVariant` live on a shadowed config row (content from the config row, ship-or-not from the repo), which this spec does not design. Confirm the default, or ask for the alternative.
