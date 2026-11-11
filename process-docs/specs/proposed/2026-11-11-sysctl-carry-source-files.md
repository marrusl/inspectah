# Sysctl: Carry the Admin's Files, Report Runtime Drift

## Metadata

| Field | Value |
|---|---|
| Date | 2026-11-11 |
| Status | Proposed. Direction and five design decisions ratified by the project owner on 2026-11-11 (§ 3). Depends on the general defaults work (below). |
| Scope | The sysctl collector, the snapshot types that hold its output, every renderer and refine surface that reads them, and the sysctl half of the aggregate merge. The general config-file path is used as it is. |
| Backlog items | Seven sysctl items; disposition in § 9. |
| Verified | Code read at `ea477950` (HEAD, 2026-11-11). `crates/core/src/aggregate/merge.rs`, `crates/pipeline/src/render/audit.rs`, `crates/pipeline/src/render/containerfile.rs` and `crates/refine/src/session.rs` carry uncommitted edits from unrelated work, so their citations are read from HEAD. Line numbers are a convenience; the symbol name is the identifier. Upstream behavior is cited from systemd `sysctl.d(5)`, systemd `man/standard-conf.xml`, systemd `src/sysctl/sysctl.c`, tuned `tuned-main.conf` and `plugins/plugin_sysctl.py`, as read for the 2026-11-11 analysis. |

**No code, documentation, or CHANGELOG entry was written to produce this file.**

## Dependency

**A modified `/etc/sysctl.conf` is included by default.** This is not a sysctl exception. It follows from two project-wide rules (Mark, 2026-11-11): a user-modified config of an installed RPM is operator intent and is included by default, the same as the package; and every default decision is made before the tarball is written, so refine applies nothing on load. The same holds for any package-shipped `/etc/sysctl.d/*.conf` an admin edited.

Today the collector does not do this. It records every `/etc` file that `rpm -Va` reports with a content change as `ConfigFileKind::RpmOwnedModified` with `disposition: FindingKind::excluded()` (`crates/collect/src/inspectors/config/mod.rs:130-163`), and only refine's session normalization flips it to included. `/etc/sysctl.conf` is RPM-owned on the target family (the systemd spec files for Fedora, CentOS Stream 9 and 10 install it, together with the `/etc/sysctl.d/99-sysctl.conf -> ../sysctl.conf` symlink), so on the scan path an edited one lands in `config.files` excluded.

So this spec **depends on** the defaults work tracked in the backlog item `inspectah-scan-tarball-and-refine-export-disagree-on-defaults`, which moves that default to collect for every section. This spec does not design that work.

If this spec lands first, there are two choices: carry the one-line collect flip for `RpmOwnedModified` itself (`config/mod.rs:162`, `excluded()` to `included()`), or wait for the defaults work. **Recommendation: carry the flip**, because without it removing the drop-in leaves a host whose sysctl configuration lives in `/etc/sysctl.conf` carrying none of it from the scan path, a regression on exactly that host, while the flip is the change already decided for every modified config and no test or snapshot pins the excluded default (whole workspace measured with the line flipped, 2026-11-11 at `45a01a66`). The flip is general, not sysctl-scoped: the plan then also takes its CHANGELOG line and the docs that describe the scan leaving modified configs out, and the defaults work drops that row.

## 1. Problem

inspectah carries sysctl settings through two routes that disagree, and the one it built for sysctl specifically is wrong in what it reads, what it writes, and what it throws away.

**Route one, the general config path.** The `/etc` walker classifies `/etc/sysctl.d/` and `/etc/sysctl.conf` as `ConfigCategory::Sysctl` (`crates/collect/src/inspectors/config/classify.rs:57-60`). An unowned file becomes an included `ConfigFileEntry` (`config/mod.rs:255-262`) and is copied whole by the config tree's bulk `COPY config/<root>/ /<root>/` (`containerfile.rs:1505`).

**Route two, the sysctl collector.** `collect_sysctl_overrides` (`crates/collect/src/inspectors/kernelboot.rs:196-245`) records a `SysctlOverride` only for a key whose file value differs from `sysctl -a` (`:226-239`). The renderer writes the **runtime** value of each included override into a synthesized `/etc/sysctl.d/99-inspectah-migrated.conf` (`crates/pipeline/src/render/configtree.rs:443-468`, `:458`) and the Containerfile copies it (`containerfile.rs:1847-1861`).

The routes are joined by the shadow pass. `shadow_sysctl_sources` (`crates/pipeline/src/collect.rs:272-285`) turns every config entry whose path is some override's `source` into `Inventory` with `shadowed_by: ConfigShadow::Sysctl` (`:324-337`). Its doc comment states the assumption that fails: the file's "content reaches the image through those keys alone" (`:262-265`). A file with one drifted key and six that match is dropped whole, and only the one drifted key reaches the image, at its runtime value. systemd never drops a file's keys because another file overrides one of them; it shadows whole files by name only (`standard-conf.xml` § Configuration Directories and Precedence).

The collector's model of precedence is wrong in three ways:

- **Backwards read order.** `read_sysctl_files` reads `/etc/sysctl.d` and then `/usr/lib/sysctl.d` into one map with a plain `insert` (`kernelboot.rs:274`, `:289`), so a key set in both resolves to the vendor value. systemd sorts surviving files by filename across all directories and applies the last assignment (`sysctl.c`, `parse_line`).
- **No same-name shadowing and no masks.** A `/etc/sysctl.d/50-default.conf` does not hide `/usr/lib/sysctl.d/50-default.conf` in the collector's model, and a `/dev/null` symlink is read as an empty file.
- **Missing directories and syntax.** `/run/sysctl.d` and `/usr/local/lib/sysctl.d` are not read. `parse_sysctl_conf` (`:304-316`) keeps a leading `-` and slash-form keys as written, so they never match `sysctl -a` and are dropped silently (`:227-228`).

Downstream: the audit table prints the file value under "Default Value" and interpolates values raw, so a pipe `kernel.core_pattern` splits the row (`audit.rs:585-604` at HEAD). The aggregate merge keys overrides on the key alone with no variants and excludes any key that drifted on fewer than all hosts (`merge.rs:321-323`, `:701-711` at HEAD), while the config section merges independently, so a file can be shipped and shadowed, or neither, depending on host order. Refine offers a per-key toggle (`crates/refine/src/session.rs:2881-2886` at HEAD) over values the host itself would lose at its next reboot.

## 2. Non-goals

- Expanding glob keys against `sysctl -a`.
- Comparing host vendor files against the target image's `/usr/lib/sysctl.d`. That belongs with any future work that reads the target base image.
- Runtime-only keys that no file names. Finding them needs a baseline `sysctl -a` to diff against, which is the same base-image problem.
- Migrating tuned profiles.
- Per-key exclusion inside a file, or rewriting a copied file (decision 3).

## 3. Decisions

All five are recorded as made. They are not reopened here.

1. **Carry the admin's sysctl files as they were** (Mark, 2026-11-11). `/etc/sysctl.d/*.conf` files the admin owns, a modified `/etc/sysctl.conf`, and `/dev/null` mask symlinks go into the image under their original names, through the general config-file path. Vendor files in `/usr/lib/sysctl.d` are never copied: the target image ships its own. These are uniquely named, admin-owned files, so the `/etc` 3-way merge costs nothing, and anyone running transient `/etc` has had to think about their config on purpose.
2. **Runtime drift is reported, never carried** (Mark, 2026-11-11). `99-inspectah-migrated.conf` and its writer go away. A key whose runtime value differs from its file value becomes a report line: the runtime value, the file value, that the host would go back to the file value at reboot, and that a daemon or a hand-run `sysctl -w` probably set it. No refine toggle, no fleet disposition for drift.
3. **Exclusion is whole-file only** (Mark, 2026-11-11), the same as every other config file. No per-key exclusion and no rewriting of copied files. Changing one key means editing the file.
4. **Vendor advisory: flag shadows and masks only** (Mark, 2026-11-11). When an admin file has the same name as a host vendor file in `/usr/lib/sysctl.d`, or masks one with `/dev/null`, the report says that it replaces the vendor file entirely on the target as well, so keys the target's vendor version adds will not apply, and that the operator should compare against the target's copy. No comparison against the target image.
5. **The collector models systemd's precedence fully, except glob expansion** (Mark, 2026-11-11). All four directories in priority order, same-name whole-file shadowing, `/dev/null` masks, sorting by filename across all directories with the last assignment winning, `/etc/sysctl.conf` through its `99-sysctl.conf` symlink, `-` prefixes, slash-form keys normalized. Glob lines are reported as written. The collector's job is now explanation: which file sets each key, what overrides what, where runtime differs. It no longer decides what gets copied.

Placement in `/etc/sysctl.d` is also what keeps tuned from overriding the carried values: tuned's default `reapply_sysctl = 1` re-applies `/run/sysctl.d`, `/etc/sysctl.d` and `/etc/sysctl.conf` after its profile, and not `/usr/lib/sysctl.d` (`tuned-main.conf`; `plugin_sysctl.py`, `SYSCTL_CONFIG_DIRS`).

## 4. Two verifications

### 4.1 How a modified `/etc/sysctl.conf` reaches the image today

Read, not run.

- It enters `config.files` only through `rpm -Va`. `verification_results()` entries under `/etc` are collected (`config/mod.rs:118-125`); a content change (`S` or `5` flag) classifies as `RpmOwnedModified` (`classify_rpm_va_kind`, `:376-391`), a metadata-only change as `RpmOwnedDefault`, which is dropped (`:148-152`). The modified entry is pushed with `disposition: FindingKind::excluded()` (`:154-163`). An unmodified one never appears.
- The `/etc/sysctl.d/99-sysctl.conf` symlink is RPM-owned, so the walker skips it (`:191`).
- The sysctl collector reads the file's content through that symlink, because it lists `/etc/sysctl.d` and follows it with `read_file` (`kernelboot.rs:274-289`), so every key from `/etc/sysctl.conf` is attributed to `/etc/sysctl.d/99-sysctl.conf`. No config entry has that path, so the shadow pass never touches `/etc/sysctl.conf`; its drifted keys reach the image through the drop-in, and the rest only if the operator includes the file.
- Refine's `is_file_backed` tests `source == "/etc/sysctl.conf"` (`crates/refine/src/classify.rs:735`), a value the collector never produces.

So: by default, not at all except for drifted keys. Hence § Dependency.

### 4.2 Whether `/usr/local/lib/sysctl.d` is carried by the existing `/usr/local` handling

Partly, and only on request.

- `/usr/local` is a Tier 2 unmanaged-file scan root (`crates/collect/src/inspectors/nonrpm.rs:1969`), but that scan runs only under `inspectah scan --include-unmanaged` (`crates/cli/src/commands/scan.rs:157-160`, `:734-737`). Without the flag, nothing under `/usr/local` is collected.
- With the flag, `walk_for_unmanaged` records a non-RPM-owned `/usr/local/lib/sysctl.d/foo.conf` as an `UnmanagedFile` of `FileType::Config`, included by default (`nonrpm.rs:2286-2363`), and `unmanaged_file_lines` copies it to the same path (`crates/pipeline/src/render/unmanaged.rs:99-133`). Nothing there knows it is a sysctl file.
- Where that copy lands depends on the base image. On fedora-bootc, centos-bootc and RHEL bootc, `/opt` and `/usr/local` are plain image directories: carried by the image, replaced on every update, read-only at runtime (bootc docs and lint source; maintainer-confirmed in bootc Discussion #1038, 2025-01-16). So on the default derivation bases the copied file sits in `/usr/local/lib/sysctl.d` as part of the image. CoreOS-lineage bases are the opposite: there `/usr/local` is a link to `/var/usrlocal`, so the same `COPY` lands in `/var`. That affects every `/usr/local` finding, not only sysctl, and is not this spec's to fix.

**Recommendation: leave carriage of `/usr/local/lib/sysctl.d` out of scope.** The collector reads the directory for precedence (decision 5), and the report names any file found there and says it is carried only through the unmanaged-file path, if at all. Moving such a file to `/etc/sysctl.d` is the domain-correct placement (tuned reapplies `/etc/sysctl.d`, not `/usr/local/lib/sysctl.d`), but it would be a second carriage route for one file, which is the defect this spec removes. Accepted 2026-11-11.

## 5. Design

### 5.1 Carriage

Nothing sysctl-specific carries anything. Each file below is a `ConfigFileEntry` of `ConfigCategory::Sysctl` and goes through the config tree under its own disposition, unchanged:

| Host file | Config entry | Default | In the image |
|---|---|---|---|
| unowned `/etc/sysctl.d/<name>.conf` | `Unowned` | included | `/etc/sysctl.d/<name>.conf`, content as it was |
| `/etc/sysctl.d/<name>.conf -> /dev/null` | `Unowned`, empty content | included | an empty regular file of the same name (below) |
| modified `/etc/sysctl.conf` | `RpmOwnedModified` | included (§ Dependency) | `/etc/sysctl.conf`, applied by the target's own `99-sysctl.conf` symlink at the same position |
| modified package-shipped `/etc/sysctl.d/<name>.conf` | `RpmOwnedModified` | as for `/etc/sysctl.conf` | same path |
| `/usr/lib/sysctl.d/*`, `/run/sysctl.d/*` | none | none | not copied |

**Masks arrive as empty files.** `ConfigFileEntry` has no link target, so the config tree writes a `/dev/null` symlink as an empty regular file; the walker reads it through `canonicalize` and `read_file` (`config/mod.rs:212-262`, `crates/collect/src/executor/real.rs:406-408`). systemd replaces a lower-priority file by name, whatever the replacing file holds (`standard-conf.xml`), so an empty same-named file disables the vendor file exactly as the symlink does, and tuned's reapply reads nothing from it. The name is kept, which is what decision 1 requires. Carrying the symlink itself would need a link target on `ConfigFileEntry`, a change for every config file, not only sysctl; the empty file is accepted (2026-11-11).

Removed: the shadow pass `shadow_sysctl_sources` and its call (`collect.rs:258`, `:262-285`); `ConfigShadow::Sysctl` (`crates/core/src/types/config.rs:57-59`); `SYNTHESIZED_SYSCTL_FILE` (`config.rs:48`); the drop-in writer (`configtree.rs:443-468`); the Containerfile `COPY sysctl/...` block (`containerfile.rs:1847-1861`); the `sysctl/` export directory. `ConfigShadow` keeps `Services`; `shadow_service_owned_paths` and `shadow_matching_config_entries` are unchanged.

**`sysctl_refusal` goes too** (`configtree.rs:225-256`). It guarded a line inspectah built from snapshot values. With no drop-in, no sysctl key or value reaches any line inspectah writes; the files are copied verbatim. The 2026-11-11 trust ruling still holds, unchanged in substance: sysctl keys and values are trusted host configuration, applied as the host had them (`process-docs/skills/instruction-line-values.md:131-140`). They now sit in the same boundary as every other verbatim config file, which that skill already names (`:125-129`). `has_line_break_or_non_tab_control` stays; `kickstart.rs` uses it.

### 5.2 Collector

`collect_sysctl_overrides` is replaced by a reader that records what is on disk and what the kernel reports. It resolves nothing; resolution is a pure function in `inspectah-core` (§ 5.3) so the audit, refine and tests all compute it the same way.

**Directories**, highest priority first (`sysctl.d(5)` SYNOPSIS, `standard-conf.xml`): `/etc/sysctl.d`, `/run/sysctl.d`, `/usr/local/lib/sysctl.d`, `/usr/lib/sysctl.d`. Entries ending in `.conf` only. For each: `read_link` first; a target of `/dev/null` is a mask and is not read. Any other symlink is followed and its target recorded (`99-sysctl.conf -> ../sysctl.conf`). A missing directory is normal; a read failure on an existing file degrades the kernel/boot inspector as today (`kernelboot.rs:201-207`), because an explanation missing a file can claim drift that is not there.

**Parsing** (`sysctl.d(5)`, `sysctl.c`): blank lines and lines starting with `#` or `;` are skipped. Split on the first `=`, trim both sides. A key starting with `-` sets `ignore_failure` and is stripped. A key holding `*`, `?` or `[` is a glob and recorded as written. A `-`-prefixed key with no `=` is a glob exclusion and recorded as written. Any other line without `=` is recorded as unparsed so the report can show it.

**Key normalization.** Keys are stored in the dotted form `sysctl -a` prints. `sysctl.d(5)` allows `.` or `/` as the separator and uses the first separator to decide: when it is `/`, the key is already a `/proc/sys` path; when it is `.`, dots and slashes are interchanged to get one. So `net/ipv4/ip_forward` and `net.ipv4.ip_forward` are one key, and `net/ipv4/conf/enp3s0.200/forwarding` and `net.ipv4.conf.enp3s0/200.forwarding` are one key. Normalize to the path, then render the path the way `sysctl -a` does (path `/` to `.`, a literal `.` in a component to `/`). The interface-name rule is from the man page as recalled for this spec: the plan's parser task confirms it against the current `sysctl.d(5)` before writing the newtype.

**Runtime.** Run `sysctl -a` whenever any file holds an assignment (today it is skipped when no file defines a key, `:209-211`). Keep `parse_sysctl_output` and record the runtime value of every key some assignment names. A key absent from the output is recorded as absent.

### 5.3 Snapshot types

`KernelBootSection.sysctl_overrides: Vec<SysctlOverride>` is replaced by `sysctl: Option<SysctlSection>`. `SysctlOverride` is deleted. This is a schema change: bump `SCHEMA_VERSION` (`crates/core/src/snapshot.rs:21`; exact-match loading per `process-docs/skills/snapshot-schema-versioning.md`).

```rust
/// Where systemd-sysctl reads, in priority order: `Ord` is priority,
/// so a same-named file in a lower variant is replaced by a higher one.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SysctlDir { Etc, Run, UsrLocalLib, UsrLib }

/// What the sysctl search path held at scan time. Collected, not resolved.
pub struct SysctlSection {
    /// Every `*.conf` entry in the four directories, sorted by (name, dir).
    pub files: Vec<SysctlFile>,
    /// `sysctl -a` values for every key some assignment names.
    /// A key missing here was not exposed by the kernel at scan time.
    pub runtime: BTreeMap<SysctlKey, String>,
}

pub struct SysctlFile {
    pub dir: SysctlDir,
    /// Basename: the sort key and the shadow key.
    pub name: String,
    pub body: SysctlFileBody,
}

pub enum SysctlFileBody {
    /// A symlink to `/dev/null`.
    Mask,
    /// `link_target` is set when the entry is a symlink
    /// (`99-sysctl.conf` -> `../sysctl.conf`).
    Lines { link_target: Option<String>, lines: Vec<SysctlLine> },
}

pub enum SysctlLine {
    Assign { line_no: u32, key: SysctlKey, value: String, ignore_failure: bool },
    /// Reported as written; not expanded.
    Glob { line_no: u32, pattern: String, value: String, ignore_failure: bool },
    /// `-pattern` with no `=`: excludes keys from globs.
    GlobExclusion { line_no: u32, pattern: String },
    Unparsed { line_no: u32, text: String },
}

/// A key in the dotted form `sysctl -a` prints. Built only by the
/// normalizing parser, so two spellings of one key cannot both exist.
pub struct SysctlKey(String);
```

Resolution, in `inspectah-core`, pure:

```rust
impl SysctlSection {
    /// What systemd-sysctl would apply on this host, one row per key,
    /// sorted by key.
    pub fn effective(&self) -> Vec<SysctlSetting>;
    /// Admin files in /etc that replace or mask a host vendor file.
    pub fn vendor_replacements(&self) -> Vec<VendorReplacement>;
}

pub struct SysctlSetting {
    pub key: SysctlKey,
    pub value: String,
    pub from: SysctlFileRef,               // (dir, name)
    pub overwritten: Vec<SysctlAssignmentRef>, // earlier assignments, apply order
    pub runtime: RuntimeObservation,
}

pub enum RuntimeObservation {
    Matches,
    Differs { runtime: String },
    NotExposed,
}

pub enum VendorReplacement {
    Shadows { name: String }, // /etc/sysctl.d/<name> over /usr/lib/sysctl.d/<name>
    Masks { name: String },
}
```

`effective()`: drop every file whose name appears in a higher-priority directory, drop masks, sort the survivors by name, apply assignments in file then line order with the last one winning, compare each winner with `runtime` through the existing `sysctl_values_equal` (`kernelboot.rs:261-265`, the tab and space rule). Comparing against the winner, not against each file, removes today's false drift from the inverted read order.

Storing the collected facts and computing the effective view means the snapshot cannot hold an effective value that disagrees with its own files.

Fields deliberately absent: no `disposition`, no `locked`, no `aggregate`. The section owns no decision, so it has nothing to toggle, lock or count.

### 5.4 Audit report

The `#### Sysctl Overrides` table (`audit.rs:585-604` at HEAD) becomes `#### Sysctl`, single-host only:

1. One line: "These values are trusted host configuration. The files that set them are carried as they were, not rewritten."
2. A table of `effective()`: `Key | File value | Runtime value | Source`. Runtime value prints `same` when it matches and `not exposed` when the kernel did not list the key. Source is the winning file's path, plus `(link to /etc/sysctl.conf)` for the symlink.
3. **Runtime drift**, one line per `Differs` key: "`<key>` runs at `<runtime>`; the files set `<file value>` (`<source>`). At reboot the host would go back to `<file value>`. A daemon or a hand-run `sysctl -w` probably set the running value. Not carried."
4. **Vendor files replaced**, one line per `VendorReplacement`: "`/etc/sysctl.d/<name>` replaces `/usr/lib/sysctl.d/<name>` entirely, on the target as well, so any keys the target's version of that file adds will not apply. Compare it with the target image's copy." The mask wording says "disables".
5. **Not carried**: files in `/run/sysctl.d` (generated at runtime) and `/usr/local/lib/sysctl.d` (carried only as unmanaged files under `--include-unmanaged`), and any `/etc` sysctl file whose config entry is excluded, found by path in `config.files` at render time. That lookup is display only; the config entry stays the one decision.
6. Glob, glob-exclusion and unparsed lines, as written, with file and line number.

Every cell and every interpolated value passes through one Markdown table-cell escape (`|` as `\|`, and `\` as `\\`) so a pipe `kernel.core_pattern` stays in its row. The HTML report's sysctl table (`crates/pipeline/src/render/report.rs:832-849`, `templates/report/kernel.html:10-22`) takes the same columns; a pipe is harmless in HTML, so it needs no new escaping.

### 5.5 Refine

The sysctl rows stop being decisions. The decision for each `/etc` sysctl file is its Configuration Files row, as for any config file.

- **System Tuning returns** as a section, closing `inspectah-sysctl-section-components-are-orphaned` on its first branch: system tuning comes back (Mark, 2026-11-01). `system_tuning` comes off `RETIRED` (`crates/web/ui/src/App.tsx:183`) and `MainContent` renders `SystemTuningSection`. The tuned rows are unchanged. `SysctlSection.tsx` is rewritten as read-only: the effective table, drift lines, vendor replacement lines and not-carried notes from § 5.4, each `/etc` file linking to its Configuration Files row.
- **Removed:** `ItemId::Sysctl` (`crates/refine/src/types.rs:110-112`) and its arms in `session.rs` (`:350-353`, `:436`, `:2180-2185`, `:2881-2886` at HEAD); `RefinedSysctl` and `classify_sysctls` with its view-only deny list (`classify.rs:707-758`), which hid `kernel.sysrq` from one view while the build still shipped it; `TriageReason::SysctlBaselineMatch`, `SysctlFileBackedOverride` and `SysctlNoBaseline` (`types.rs:354-356`); the bulk include op for sysctls (`crates/web/src/handlers.rs:838-847`); the sysctl decision rows in the Kernel and Boot context list (`crates/web/src/adapter.rs:1026-1042`); `RefSysctlOverride` and the "merged into" pointer (`crates/refine/src/projection/types.rs:248-251`, `:268`); the `sysctl` branch of `shadowOwner.ts`.
- **`SysctlDecisionDto`** becomes a read-only DTO carrying `effective()`, the drift and vendor lines, and each file's config row path. No `decision` field.
- A config row of category `Sysctl` is an ordinary toggleable row: no entry is `Inventory` because of sysctl any more, so `InventoryNotToggleable` no longer applies to it.

### 5.6 Fleet aggregate

No new merge logic. Sysctl files merge as config entries, by path, with the existing per-path content variants (`ConfigFileEntry::content_variant_key`, `merge.rs:102-126` at HEAD). Hosts that disagree on a file's content produce variants the operator picks between, as for any config file.

**The per-key sysctl merge goes away, and the aggregate carries `sysctl: None`.** It is removed, not kept as explanation, for three reasons:

- An effective value, a drift line and a vendor replacement are true of one host's files and one host's kernel. A merged section either picks one host's view and presents it as the fleet's, which is today's whole-struct winner (`merge.rs:554-568` at HEAD) and is what made the reconciliation item possible, or needs per-key value variants, which is new merge logic.
- Nothing downstream needs it: carriage is fully in the config section.
- The aggregate audit and the aggregate refine panel each say one line instead: "Sysctl explanation is per host and is not merged; each host's own scan has it. Sysctl files are carried as Configuration Files, with a variant per differing content."

Cost: the vendor replacement advisory is not shown at fleet level. The operator sees it in each host's scan. Accepted, not open.

`merge_kernelboot_sections` drops its `sysctl_overrides` merge (`merge.rs:1823-1827` at HEAD) and the `AggregateMergeable for SysctlOverride` impl (`:321-333`).

## 6. Docs and skills made stale

The plan updates each.

| File | What changes |
|---|---|
| `process-docs/skills/instruction-line-values.md` | `:17` and the `:43` table row name the sysctl drop-in, which no longer exists. The `:131-140` trust paragraph keeps the ruling and its record, and says sysctl values now reach the image only inside verbatim copied files, so no sysctl value reaches an instruction line. |
| `process-docs/skills/cross-inspector-shadow-passes.md` | `:11-12`, `:47-63`, `:122`: the sysctl pass and the two early-return tests that pair it with the service pass. One pass remains; the rule for adding a second still stands. |
| `process-docs/skills/aggregate-vs-single-host-behavioral-split.md` | `:63-76`: the worked example describes a state that can no longer exist. Replace with one that still can, or drop it. |
| `process-docs/skills/web-disposition-contract.md` | `:66`, `:81-84`, `:151-158`, `:170`: `SysctlDecisionDto` as a decision, and sysctl-shadowed config entries in the advisory bucket. |
| `process-docs/skills/real-host-validation-runs.md` | `:331-341`: the tab and space rule stays right; the note that `SysctlOverride.default` holds the file value goes with the struct. |
| `process-docs/skills/containerfile-value-quoting.md` | `:51`, `:82`: the drop-in as a built line vector. |
| `process-docs/skills/finding-disposition-serde-defaults.md` | `:40`: `SysctlOverride` in the list. |
| `docs/reference/output-artifacts.md` | `:66`, `:182`, `:274-277`, `:356-359`, `:410-413`: the `sysctl/` directory and `99-inspectah-migrated.conf`. |
| `docs/reference/triage-classification.md` | `:13`, `:78`, `:92`, `:223`, `:308`, `:374`: sysctl overrides as triaged decisions, "Merged into sysctl settings". |
| `docs/reference/inspector-coverage.md` | `:341`, `:464`: what the collector reads and what it records. |
| `docs/reference/snapshot-schema.md` | `:64`: `kernel_boot` contents. |
| `docs/how-to/review-and-refine.md` | `:127-129`: "Merged into sysctl settings". |
| `docs/tutorials/first-migration.md` | `:184`, `:259-261`, `:305`, `:472`: sysctl overrides, the merged drop-in, inventory sysctl files. |
| `docs/diagrams/data-flow.html`, `docs/diagrams/software-architecture.html` | Any sysctl drop-in or shadow arrow. |

**CHANGELOG:** user-visible. The plan adds an `[Unreleased]` entry: sysctl files are carried as the host had them, `99-inspectah-migrated.conf` is no longer written, runtime drift is reported and not carried, the per-key sysctl toggle is gone, the collector now follows systemd's precedence, and the audit table shows file and runtime values. Snapshots from earlier releases must be re-scanned (schema bump).

## 7. Out of scope

- Glob expansion against `sysctl -a`.
- Comparing host vendor files with the target image's.
- Runtime-only keys that no file names.
- tuned profile migration.
- Carrying `/usr/local/lib/sysctl.d` by any route other than the existing unmanaged-file path (§ 4.2), and how a `/usr/local` finding lands on a CoreOS-lineage base, where `/usr/local` is a link into `/var`.
- Markdown escaping in other audit tables. This spec adds the helper for the sysctl table only; other tables that interpolate raw values are a separate defect.

## 8. Testing

**Built from collector-produced fields.** Every case below runs the real `KernelbootInspector` and, where carriage is asserted, the real `ConfigInspector`, over a `MockExecutor` holding host files, links and `sysctl -a` output, through the scan path (`collect()`, `validate`, `redact`, then render), as `crates/cli/tests/scan_shadow_test.rs` does today. No test builds a `SysctlSection`, `SysctlFile` or `ConfigFileEntry` by hand to stand for a host layout: a hand-built fixture can encode a layout the collector cannot produce, and the resulting test passes against nothing real. Unit tests of `effective()` may build `SysctlSection` directly only to pin the resolution rule, and each such case has a collector-driven twin in this list.

Required cases:

1. **Precedence across directories.** `/usr/lib/sysctl.d/50-default.conf` sets `k = 1`; `/etc/sysctl.d/90-local.conf` sets `k = 2`; `/run/sysctl.d/95-run.conf` and `/usr/local/lib/sysctl.d/10-local.conf` set other values for `k`. The effective value is `95-run.conf`'s, by name order. Then the same with `/etc` at `40-` and vendor at `50-`: the vendor value wins. Today's reader gets the first case wrong.
2. **Same-name shadow.** `/etc/sysctl.d/50-default.conf` sets `a` only; `/usr/lib/sysctl.d/50-default.conf` sets `a` and `b`. `b` is absent from `effective()`, and a vendor replacement line names `50-default.conf`.
3. **Mask.** `/etc/sysctl.d/50-coredump.conf -> /dev/null`. No key from the vendor file is effective, a mask line is reported, and the export holds an empty `/etc/sysctl.d/50-coredump.conf`.
4. **`/etc/sysctl.conf` through the symlink.** `/etc/sysctl.d/99-sysctl.conf -> ../sysctl.conf` with `rpm -Va` reporting `/etc/sysctl.conf` as `S.5....T.`. Its keys are attributed to the link at position `99-sysctl.conf`; the config entry is included and the export holds `/etc/sysctl.conf`.
5. **Slash-form key.** `net/ipv4/ip_forward = 1` in a file and `net.ipv4.ip_forward = 1` from `sysctl -a`: one key, `Matches`, no drift line.
6. **`-` prefixed key.** `-net.ipv6.conf.all.disable_ipv6 = 1`: the key is `net.ipv6.conf.all.disable_ipv6`, `ignore_failure` is set, and it takes part in precedence and drift like any key.
7. **Drifted key.** A file sets `vm.swappiness = 10`; `sysctl -a` reports `60`. The audit holds the drift line with both values and the reboot wording; the export holds no `sysctl/` directory, no `99-inspectah-migrated.conf`, and no Containerfile line naming either.
8. **Pipe `core_pattern` audit row.** A file sets `kernel.core_pattern = |/usr/lib/systemd/systemd-coredump %P %u %g %s %t %c %h`. The audit row has exactly four cells, and the value appears with the pipe escaped.
9. **One drifted key does not drop the file.** `/etc/sysctl.d/90-tuning.conf` sets seven keys; `sysctl -a` differs on one. The exported `/etc/sysctl.d/90-tuning.conf` is byte-identical to the host file, the config entry is included and not `Inventory`, and exactly one drift line is reported. This is the end-to-end test the shadowed-source item asked for.
10. **Whitespace is not drift.** `net.ipv4.ip_local_port_range = 32768 60999` against a tab-separated runtime value: `Matches`, no drift line.
11. **Glob reported, not expanded.** `net.ipv4.conf.*.rp_filter = 2` appears in the audit as written with file and line, and adds no row to `effective()`.
12. **Aggregate.** Two hosts with different content for one `/etc/sysctl.d` path produce two config variants and `kernel_boot.sysctl == None`, and the aggregate audit prints the per-host line.

Tests that encode the removed behavior are deleted or rewritten, not left skipped: `scan_shadow_test.rs`, the sysctl cases in `configtree_single_source_test.rs`, `instruction_injection_test.rs`'s sysctl row, the paired early-return tests in `collect.rs`, the `SysctlOverride` merge cases in `aggregate_merge_test.rs` and `aggregate_e2e_test.rs`, the sysctl contract snapshots in `crates/web/tests/snapshots/`, and the frontend tests for `shadowOwner`, `SysctlSection` and the shadowed-config rows.

## 9. Backlog items

| Item | Disposition |
|---|---|
| `inspectah-sysctl-render-should-mirror-the-source-files` | **Closes.** Decision 1 is its requirement. |
| `inspectah-sysctl-shadowed-source-drops-non-override-keys` | **Closes.** The shadow pass is deleted; § 8 case 9 is its end-to-end proof. |
| `inspectah-sysctl-whitespace-false-positive-has-no-end-to-end-shadow-test` | **Moot.** No sysctl shadow pass remains to test. The comparison rule survives for drift lines and keeps its test (case 10). |
| `inspectah-aggregate-sysctl-disposition-reconciliation` | **Moot.** No sysctl disposition and no shadow are left to reconcile; files merge by path only. |
| `inspectah-sysctl-override-default-field-holds-the-file-value` | **Closes.** The struct is deleted; the new types name the file value as such. |
| `inspectah-sysctl-section-components-are-orphaned` | **Closes**, on the branch the owner chose: System Tuning returns, with sysctl read-only (§ 5.5). The `shadowOwner.ts` route it worried about is removed rather than retargeted. |
| `inspectah-aggregate-merge-does-not-reconcile-sysctl-shadowing` | Already closed (superseded). No change. |
| `inspectah-sysctl-drift-false-positive-on-tab-versus-space` | Already closed (done). Its rule carries over unchanged. |

## 10. Open questions

None.
