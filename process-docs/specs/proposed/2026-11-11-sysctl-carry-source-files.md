# Sysctl: Carry the Admin's Files, Report Runtime Drift

## Metadata

| Field | Value |
|---|---|
| Date | 2026-11-11 |
| Status | Proposed. Direction and five design decisions ratified by the project owner on 2026-11-11 (§ 3). Depends on the modified-config default (below). |
| Scope | The sysctl collector, the snapshot types that hold its output, the redaction pass over them, every renderer and refine surface that reads them, the System Tuning section's registry entry, and the sysctl half of the aggregate merge. The general config-file path is used as it is, plus one classification rule for RPM-owned `/dev/null` links (§ 5.1). |
| Backlog items | Seven sysctl items; disposition in § 9. |
| Verified | Code read at `33c4518a` (HEAD, 2026-11-11); line numbers are a convenience and the symbol name is the identifier. Upstream behavior as read on 2026-11-11: systemd `src/sysctl/sysctl.c` (`v252` and `main`), `man/sysctl.d.xml` and `man/standard-conf.xml`; tuned `tuned/plugins/plugin_sysctl.py` and `tuned-main.conf` (`master`); RPM `lib/verify.c` (`rpm-4.16.x`); cmark-gfm `extensions/table.c` and `extensions/ext_scanners.re`. |

**No code, documentation, or CHANGELOG entry was written to produce this file.**

## Dependency

**A modified `/etc/sysctl.conf` is included by default.** This is not a sysctl exception. It follows from two project-wide rules (Mark, 2026-11-11): a user-modified config of an installed RPM is operator intent and is included by default, the same as the package; and every default decision is made before the tarball is written, so refine applies nothing on load. The same holds for any package-shipped `/etc/sysctl.d/*.conf` an admin edited.

At `33c4518a` the collector does not do this. It records every `/etc` file that `rpm -Va` reports with a content change as `ConfigFileKind::RpmOwnedModified` with `disposition: FindingKind::excluded()` (`crates/collect/src/inspectors/config/mod.rs:130-163`), and only refine's session normalization flips it to included. `/etc/sysctl.conf` is RPM-owned on the target family (the systemd spec files for Fedora, CentOS Stream 9 and 10 install it, together with the `/etc/sysctl.d/99-sysctl.conf -> ../sysctl.conf` symlink), so on the scan path an edited one lands in `config.files` excluded.

`ef8ec686` (`fix(collect): include user-modified RPM configs by default`) makes that default included at collect. It lands on its own, ahead of this spec and of the wider defaults work in the backlog item `inspectah-scan-tarball-and-refine-export-disagree-on-defaults` (Mark, 2026-11-11). This spec depends on it and does not repeat it: without it, removing the drop-in leaves a host whose sysctl configuration lives in `/etc/sysctl.conf` carrying none of it from the scan path.

## 1. Problem

inspectah carries sysctl settings through two routes that disagree, and the one it built for sysctl specifically is wrong in what it reads, what it writes, and what it throws away.

**Route one, the general config path.** The `/etc` walker classifies `/etc/sysctl.d/` and `/etc/sysctl.conf` as `ConfigCategory::Sysctl` (`crates/collect/src/inspectors/config/classify.rs:57-60`). An unowned file becomes an included `ConfigFileEntry` (`config/mod.rs:255-262`) and is copied whole by the config tree's bulk `COPY config/<root>/ /<root>/` (`containerfile.rs:1505`).

**Route two, the sysctl collector.** `collect_sysctl_overrides` (`crates/collect/src/inspectors/kernelboot.rs:196-245`) records a `SysctlOverride` only for a key whose file value differs from `sysctl -a` (`:226-239`). The renderer writes the **runtime** value of each included override into a synthesized `/etc/sysctl.d/99-inspectah-migrated.conf` (`crates/pipeline/src/render/configtree.rs:443-468`, `:458`) and the Containerfile copies it (`containerfile.rs:1847-1861`).

The routes are joined by the shadow pass. `shadow_sysctl_sources` (`crates/pipeline/src/collect.rs:272-285`) turns every config entry whose path is some override's `source` into `Inventory` with `shadowed_by: ConfigShadow::Sysctl` (`:324-337`). Its doc comment states the assumption that fails: the file's "content reaches the image through those keys alone" (`:262-265`). A file with one drifted key and six that match is dropped whole, and only the one drifted key reaches the image, at its runtime value. systemd never drops a file's keys because another file overrides one of them; it shadows whole files by name only (`standard-conf.xml` § Configuration Directories and Precedence).

The collector's model of precedence is wrong in four ways:

- **Backwards read order.** `read_sysctl_files` reads `/etc/sysctl.d` and then `/usr/lib/sysctl.d` into one map with a plain `insert` (`kernelboot.rs:274`, `:289`), so a key set in both resolves to the vendor value. systemd sorts surviving files by filename across all directories and keeps the last entry for each key (`sysctl.c`, `parse_line`).
- **No same-name shadowing and no masks.** A `/etc/sysctl.d/50-default.conf` does not hide `/usr/lib/sysctl.d/50-default.conf` in the collector's model, and a `/dev/null` symlink is read as an empty file.
- **Missing directories and syntax.** `/run/sysctl.d` and `/usr/local/lib/sysctl.d` are not read. `parse_sysctl_conf` (`:304-316`) keeps a leading `-` and slash-form keys as written, so they never match `sysctl -a` and are dropped silently (`:227-228`). A `-key` line with no `=` is skipped, though systemd treats it as cancelling an earlier assignment of that key.
- **Silent directory failures.** A directory that cannot be listed is skipped as if it were empty (`:276`).

Downstream: the audit table prints the file value under "Default Value" and interpolates values raw, so a pipe `kernel.core_pattern` splits the row (`audit.rs:585-604`). The aggregate merge keys overrides on the key alone with no variants and excludes any key that drifted on fewer than all hosts (`merge.rs:321-323`, `:701-711`), while the config section merges independently, so a file can be shipped and shadowed, or neither, depending on host order. Refine offers a per-key toggle (`crates/refine/src/session.rs:2881-2886`) over running values that no file on the host requests.

## 2. Non-goals

- Expanding glob keys against `sysctl -a`.
- Comparing host vendor files against the target image's `/usr/lib/sysctl.d`. That belongs with any future work that reads the target base image.
- Runtime-only keys that no file names. Finding them needs a baseline `sysctl -a` to diff against, which is the same base-image problem.
- Migrating tuned profiles.
- Per-key exclusion inside a file, or rewriting a copied file (decision 3).

## 3. Decisions

All five are recorded as made. They are not reopened here.

1. **Carry the admin's sysctl files as they were** (Mark, 2026-11-11). `/etc/sysctl.d/*.conf` files the admin owns, a modified `/etc/sysctl.conf`, and `/dev/null` mask symlinks go into the image under their original names, through the general config-file path. Vendor files in `/usr/lib/sysctl.d` are never copied: the target image ships its own. Admin files in `/etc/sysctl.d` normally carry names no package ships, so the image brings no competing version of them. They still take part in the ordinary `/etc` 3-way merge on every update: a file changed on the deployed host keeps the local version over a later image's. Anyone running transient `/etc` has had to think about their config on purpose.
2. **Runtime drift is reported, never carried** (Mark, 2026-11-11). `99-inspectah-migrated.conf` and its writer go away. A key whose running value differs from the value its files request becomes a report line: the running value, the requested value and the file that requests it, and the things that can set a running value after boot (a tuned profile, another service, a hand-run `sysctl -w`, a key created after systemd-sysctl ran). No refine toggle, no fleet disposition for drift.
3. **Exclusion is whole-file only** (Mark, 2026-11-11), the same as every other config file. No per-key exclusion and no rewriting of copied files. Changing one key means editing the file.
4. **Vendor advisory: flag shadows and masks only** (Mark, 2026-11-11). When an admin file has the same name as a host vendor file in `/usr/lib/sysctl.d`, or masks one with `/dev/null`, the report says that it replaces the vendor file entirely, and on the target as well when it is carried, so keys the target's version of that vendor file sets will not apply from that file, and that the operator should compare against the target's copy. No comparison against the target image.
5. **The collector models systemd's precedence fully, except glob expansion** (Mark, 2026-11-11). All four directories in priority order, same-name whole-file shadowing, `/dev/null` masks, sorting by filename across all directories with the last entry for each key winning, `/etc/sysctl.conf` through its `99-sysctl.conf` symlink, `-` prefixes, key exclusions, slash-form keys normalized. Glob lines are reported as written. The collector's job is now explanation: which file sets each key, what overrides what, where runtime differs. It no longer decides what gets copied.

**Placement and tuned.** Carrying the files in `/etc/sysctl.d` keeps them in tuned's reapply set. With tuned's default `reapply_sysctl = 1`, after applying its profile tuned re-applies `/run/sysctl.d`, `/etc/sysctl.d` and `/etc/sysctl.conf`, and never `/usr/lib/sysctl.d` or `/usr/local/lib/sysctl.d` (`plugin_sysctl.py`, `SYSCTL_CONFIG_DIRS` and `_apply_system_sysctl`). That is a default, not a guarantee: with `reapply_sysctl = 0`, or for a key matching `reapply_sysctl_exclude`, the profile's value stands; a key set only in a vendor directory is not re-applied; and any other service can write a key after both. So the report states what the files request and never predicts the value after a reboot.

## 4. Two verifications

### 4.1 How a modified `/etc/sysctl.conf` reaches the image today

Read, not run.

- It enters `config.files` only through `rpm -Va`. `verification_results()` entries under `/etc` are collected (`config/mod.rs:118-125`); a content change (`S` or `5` flag) classifies as `RpmOwnedModified` (`classify_rpm_va_kind`, `:376-389`), anything else as `RpmOwnedDefault`, which is dropped (`:148-152`). The modified entry is pushed with `disposition: FindingKind::excluded()` (`:154-163`). An unmodified one never appears.
- The `/etc/sysctl.d/99-sysctl.conf` symlink is RPM-owned, so the walker skips it (`:191`).
- The sysctl collector reads the file's content through that symlink, because it lists `/etc/sysctl.d` and follows it with `read_file` (`kernelboot.rs:274-289`), so every key from `/etc/sysctl.conf` is attributed to `/etc/sysctl.d/99-sysctl.conf`. No config entry has that path, so the shadow pass never touches `/etc/sysctl.conf`; its drifted keys reach the image through the drop-in, and the rest only if the operator includes the file.
- Refine's `is_file_backed` tests `source == "/etc/sysctl.conf"` (`crates/refine/src/classify.rs:735`), a value the collector never produces.

So: by default, not at all except for drifted keys. Hence § Dependency.

### 4.2 Whether `/usr/local/lib/sysctl.d` is carried by the existing `/usr/local` handling

Partly, and only on request.

- `/usr/local` is a Tier 2 unmanaged-file scan root (`crates/collect/src/inspectors/nonrpm.rs:1969`), but that scan runs only under `inspectah scan --include-unmanaged` (`crates/cli/src/commands/scan.rs:157-160`, `:734-737`). Without the flag, nothing under `/usr/local` is collected.
- With the flag, `walk_for_unmanaged` records a non-RPM-owned `/usr/local/lib/sysctl.d/foo.conf` as an `UnmanagedFile` of `FileType::Config`, included by default (`nonrpm.rs:2286-2363`), and `unmanaged_file_lines` copies it to the same path (`crates/pipeline/src/render/unmanaged.rs:99-133`). Nothing there knows it is a sysctl file.
- Where that copy lands depends on the base image. On fedora-bootc, centos-bootc and RHEL bootc, `/opt` and `/usr/local` are plain image directories: carried by the image, replaced on every update, read-only at runtime (bootc docs and lint source; maintainer-confirmed in bootc Discussion #1038, 2025-01-16). So on the default derivation bases the copied file sits in `/usr/local/lib/sysctl.d` as part of the image. CoreOS-lineage bases are the opposite: there `/usr/local` is a link to `/var/usrlocal`, so the same `COPY` lands in `/var`. That affects every `/usr/local` finding, not only sysctl, and is not this spec's to fix.

**Recommendation: leave carriage of `/usr/local/lib/sysctl.d` out of scope.** The collector reads the directory for precedence (decision 5), and the report names any file found there and says it is carried only through the unmanaged-file path, if at all. Moving such a file to `/etc/sysctl.d` is the domain-correct placement (tuned's default reapply covers `/etc/sysctl.d`, not `/usr/local/lib/sysctl.d`), but it would be a second carriage route for one file, which is the defect this spec removes. Accepted 2026-11-11.

## 5. Design

### 5.1 Carriage

Nothing sysctl-specific carries anything. Each file below is a `ConfigFileEntry` of `ConfigCategory::Sysctl` and goes through the config tree under its own disposition, unchanged:

| Host file | Config entry | Default | In the image |
|---|---|---|---|
| unowned `/etc/sysctl.d/<name>.conf` | `Unowned` | included | `/etc/sysctl.d/<name>.conf`, content as it was |
| unowned `/etc/sysctl.d/<name>.conf -> /dev/null` | `Unowned`, empty content | included | an empty regular file of the same name (below) |
| RPM-owned `/etc/sysctl.d/<name>.conf` re-pointed to `/dev/null` (`rpm -Va` flags `....L....`) | `RpmOwnedModified`, empty content (below) | included (§ Dependency) | an empty regular file of the same name |
| modified `/etc/sysctl.conf` | `RpmOwnedModified` | included (§ Dependency) | `/etc/sysctl.conf`, applied by the target's own `99-sysctl.conf` symlink at the same position |
| modified package-shipped `/etc/sysctl.d/<name>.conf` | `RpmOwnedModified` | as for `/etc/sysctl.conf` | same path |
| `/usr/lib/sysctl.d/*`, `/run/sysctl.d/*` | none | none | not copied |

**Masks arrive as empty files.** `ConfigFileEntry` has no link target, so the config tree writes a `/dev/null` symlink as an empty regular file; the walker reads it through `canonicalize` and `read_file` (`config/mod.rs:212-262`, `crates/collect/src/executor/real.rs:406-408`). systemd replaces a lower-priority file by name, whatever the replacing file holds (`standard-conf.xml`), so an empty same-named file disables the vendor file exactly as the symlink does, and tuned's reapply reads nothing from it. The name is kept, which is what decision 1 requires. Carrying the symlink itself would need a link target on `ConfigFileEntry`, a change for every config file, not only sysctl; the empty file is accepted (2026-11-11).

**RPM-owned masks.** `rpm -Va` reports a re-pointed package symlink only through `L`: RPM skips the size and digest checks for anything that is not a regular file (`lib/verify.c`, `rpmVerifyFile`). `classify_rpm_va_kind` reads only the `S` and `5` columns (`config/mod.rs:376-389`), so the row classifies as `RpmOwnedDefault` and is dropped (`:146-152`), and because the path is in `rpm_va_paths` the unowned walk skips it too (`:182-186`). The package's own `/etc/sysctl.d/99-sysctl.conf` re-pointed to `/dev/null` is therefore lost today, and the target's unmasked link then applies `/etc/sysctl.conf`.

The config collector gains one rule in the `rpm -Va` loop, after the existing `is_authselect_pam_link` guard (`:140`) and in place of the plain `classify_rpm_va_kind` call: a row whose flags include `L` and whose path's final target (`resolve_final_target`) is `/dev/null` is `RpmOwnedModified`, with its flags recorded as reported and its content read as for any entry, which for `/dev/null` is empty. Every other row classifies exactly as today. The path stays in `rpm_va_paths`, so the walk still skips it and the mask has exactly one config entry. The rule applies to every `/etc` path, not only sysctl ones: the unowned walk already carries every unowned `/dev/null` link in `/etc` as an empty file, and this gives an RPM-owned link the same treatment.

**Other re-pointed RPM-owned links are not carried.** The same `L`-only gap drops every RPM-owned config symlink an admin re-points to a real file, sysctl or not. Carrying one needs a decision this spec does not make: a link target on `ConfigFileEntry`, or the target's content under the link's path. It belongs with the defaults work (`inspectah-scan-tarball-and-refine-export-disagree-on-defaults`), which owns how each `rpm -Va` kind is classified and defaulted. For sysctl, such a link has no config entry, and the explanation says so (§ 5.5, "Not carried").

**Removed:** the shadow pass `shadow_sysctl_sources` and its call (`collect.rs:258`, `:262-285`); `ConfigShadow::Sysctl` (`crates/core/src/types/config.rs:57-59`); `SYNTHESIZED_SYSCTL_FILE` (`config.rs:48`); the drop-in writer (`configtree.rs:443-468`); the Containerfile `COPY sysctl/...` block (`containerfile.rs:1847-1861`); the `sysctl/` export directory. `ConfigShadow` keeps `Services`; `shadow_service_owned_paths` and `shadow_matching_config_entries` are unchanged.

**`sysctl_refusal` goes too** (`configtree.rs:225-256`). It guarded a line inspectah built from snapshot values. With no drop-in, no sysctl key or value reaches any Containerfile or other instruction line inspectah writes; the files are copied verbatim. The 2026-11-11 trust ruling still holds, unchanged in substance: sysctl keys and values are trusted host configuration, applied as the host had them (`process-docs/skills/instruction-line-values.md:131-140`). For instruction lines they now sit in the same boundary as every other verbatim config file, which that skill already names (`:125-129`). Sysctl strings still reach the snapshot JSON, the audit and HTML reports, and refine, as data: § 5.4 scrubs them and § 5.5 keeps them literal. `has_line_break_or_non_tab_control` stays; `kickstart.rs` uses it.

### 5.2 Collector

`collect_sysctl_overrides` is replaced by a reader that records what is on disk and what the kernel reports. It resolves nothing; parsing and resolution are pure functions in `inspectah-core` (§ 5.3), so the collector, the audit, refine, deserialization and tests all compute them the same way.

**Directories**, highest priority first (`sysctl.d(5)` SYNOPSIS, `standard-conf.xml`): `/etc/sysctl.d`, `/run/sysctl.d`, `/usr/local/lib/sysctl.d`, `/usr/lib/sysctl.d`. Entries ending in `.conf` only. For each entry:

- A symlink whose final target is `/dev/null` is a mask. It is not read.
- A symlink whose final target is under `/etc/` or under one of the four directories is read through, and its final target recorded (`99-sysctl.conf` records `/etc/sysctl.conf`).
- A symlink whose final target is anywhere else is recorded with that target and **not read**. This is the boundary the config inspector draws when it turns an `/etc` link into `/var` into an advisory with empty content (`config/mod.rs:210-252`), applied here as an allowlist: a section that only explains has no reason to pull content from outside the configuration trees into the snapshot and reports. systemd-sysctl does read such a file, so § 5.5 says its keys are missing from the explanation.
- A regular file is read. Content over the config inspector's `MAX_CONFIG_FILE_SIZE` counts as a read failure.

**Failures.** A directory that does not exist is normal. Any other failure to list a directory, to read an entry (a dangling link included), or to run `sysctl -a` makes the whole explanation unavailable: `KernelBootSection.sysctl` is `None` and the kernel/boot inspector reports the reason as degraded, as today (`kernelboot.rs:86-92`, `:201-207`). No partial section is stored: an explanation missing a file, or missing the runtime values, would claim drift or `not exposed` where neither is true. Config carriage is a separate inspector and is unaffected.

**Parsing** (`sysctl.d(5)`; `sysctl.c`, `parse_line`). One parser in `inspectah-core`. Blank lines and lines starting with `#` or `;` are skipped. A line holding `=`: a leading `-` sets `ignore_failure` and is stripped, the line splits on the first `=`, and both sides are trimmed. A line without `=` that starts with `-` is an exclusion of the name after the `-`. The name on either kind of line is a glob if it holds `*`, `?` or `[`, and otherwise a key, normalized as below. Any other line is unparsed, kept so the report can show it.

**Exclusions.** systemd keeps one entry per key, the last one read: an exclusion of a key after an assignment of it replaces the assignment, and an assignment after an exclusion replaces the exclusion (`parse_line`, the existing-entry replacement). An entry with no value is skipped when settings are applied (`apply_all`). The same holds in `v252`. So `vm.swappiness = 10` followed later by `-vm.swappiness` writes nothing for that key. An exclusion of a glob only removes keys from glob expansion, which this spec does not perform, so it is reported as written.

**Key normalization.** Keys are stored in the dotted form `sysctl -a` prints. `sysctl.d(5)` allows `.` or `/` as the separator and uses the first separator to decide: when it is `/`, the key is already a `/proc/sys` path and remaining dots and slashes stay as they are; when it is `.`, dots and slashes are interchanged to get one. So `net/ipv4/ip_forward` and `net.ipv4.ip_forward` are one key, and `net/ipv4/conf/enp3s0.200/forwarding` and `net.ipv4.conf.enp3s0/200.forwarding` are one key (both examples are the man page's own, `man/sysctl.d.xml`). Normalize to the path, then render the path the way `sysctl -a` does (path `/` to `.`, a literal `.` in a component to `/`).

**Runtime.** Run `sysctl -a` whenever any file holds a key assignment or a key exclusion (today it is skipped when no file defines a key, `:209-211`). Keep `parse_sysctl_output` and record the runtime value of every key some assignment or exclusion names, normalized through `SysctlKey`. A key absent from successful output is recorded as absent.

### 5.3 Snapshot types

`KernelBootSection.sysctl_overrides: Vec<SysctlOverride>` is replaced by `sysctl: Option<SysctlSection>`. `SysctlOverride` is deleted. This is a schema change: bump `SCHEMA_VERSION` and `MIN_SCHEMA` together (`crates/core/src/snapshot.rs:21`, `:119`; exact-match loading per `process-docs/skills/snapshot-schema-versioning.md`).

```rust
/// Where systemd-sysctl reads. Declared highest priority first, so the
/// smallest variant wins a same-name selection: `Etc` replaces a file of
/// the same name in any other directory.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SysctlDir { Etc, Run, UsrLocalLib, UsrLib }

/// What the sysctl search path held at scan time. Collected, not resolved.
/// `KernelBootSection.sysctl` is `None` when collection failed (§ 5.2)
/// and on an aggregate (§ 5.7).
pub struct SysctlSection {
    /// Every `*.conf` entry in the four directories, sorted by (name, dir).
    pub files: Vec<SysctlFile>,
    /// `sysctl -a` values for every key some assignment or exclusion names.
    /// A key missing here was not in the kernel's output at scan time.
    pub runtime: BTreeMap<SysctlKey, String>,
}

pub struct SysctlFile {
    pub dir: SysctlDir,
    /// Basename: the sort key and the shadow key.
    pub name: String,
    pub body: SysctlFileBody,
}

pub enum SysctlFileBody {
    /// A symlink whose final target is `/dev/null`.
    Mask,
    /// A symlink whose final target is outside `/etc` and the four
    /// directories. Not read (§ 5.2); still shadows by name.
    OutsideLink { target: String },
    /// A file that was read. `link_target` is the final target when the
    /// entry is a symlink (`99-sysctl.conf` -> `/etc/sysctl.conf`).
    Read { link_target: Option<String>, content: String },
}

impl SysctlFile {
    /// The file's lines, parsed by the one parser. Derived, never stored.
    pub fn lines(&self) -> Vec<SysctlLine>;
}

pub enum SysctlLine {
    Assign { line_no: u32, name: SysctlName, value: String, ignore_failure: bool },
    /// `-name` with no `=` (§ 5.2, Exclusions).
    Exclude { line_no: u32, name: SysctlName },
    Unparsed { line_no: u32, text: String },
}

pub enum SysctlName {
    Key(SysctlKey),
    /// Reported as written; not expanded.
    Glob(String),
}

/// A key in the dotted form `sysctl -a` prints. Built only by the
/// normalizing parser and by the checked deserializer below, so two
/// spellings of one key cannot both exist.
pub struct SysctlKey(String);
```

**Content, not parsed lines, is stored.** The snapshot keeps each file's text as read and derives its lines on demand. So no parsed key is ever read back from JSON, the redaction pass runs on whole file text with its multi-line context (§ 5.4), and the explanation cannot disagree with the files it explains.

**`SysctlKey` on the wire.** The runtime map's keys are the one place a `SysctlKey` is deserialized. Deserialization goes through `TryFrom<String>`: the string is normalized with the parser's rule and accepted only if normalizing returns it unchanged. Anything else fails the snapshot load with an error that names the key. The dotted form and the `/proc/sys` path correspond one to one, so two distinct accepted strings are two distinct keys and the map cannot hold one key under two spellings.

Resolution, in `inspectah-core`, pure:

```rust
impl SysctlSection {
    /// What systemd-sysctl would do on this host, one row per concrete key
    /// that some surviving file names, sorted by key.
    pub fn effective(&self) -> Vec<SysctlSetting>;
    /// Admin files in /etc that replace or mask a host vendor file.
    pub fn vendor_replacements(&self) -> Vec<VendorReplacement>;
    /// How each /etc file reaches the image, read from the current config
    /// decisions. Display only: the config entry stays the one decision.
    pub fn carriage<'a>(&self, config: Option<&'a ConfigSection>)
        -> Vec<(SysctlFileRef, SysctlCarriage<'a>)>;
}

pub struct SysctlSetting {
    pub key: SysctlKey,
    pub outcome: SysctlOutcome,
    /// Earlier entries for this key, in apply order.
    pub overwritten: Vec<SysctlLineRef>,       // (dir, name, line_no)
}

pub enum SysctlOutcome {
    /// systemd-sysctl writes `value`, from the line `from`.
    Writes { value: String, from: SysctlLineRef, runtime: RuntimeObservation },
    /// The last entry for the key is an exclusion: nothing is written.
    Cancelled { by: SysctlLineRef, runtime: Option<String> },
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

pub enum SysctlCarriage<'a> {
    /// The config entry that carries the file: the entry at its own path,
    /// or, for the package's `99-sysctl.conf` link to `/etc/sysctl.conf`,
    /// the `/etc/sysctl.conf` entry. Carried when the entry is included.
    Config(&'a ConfigFileEntry),
    /// No config entry: true of a package file left as shipped, and of a
    /// link the config walker does not collect.
    NoConfigEntry,
}
```

`effective()`: drop every file whose name appears in a smaller `SysctlDir`; masks and outside links shadow by name like any file and contribute no lines; sort the survivors by name; apply each file's key assignments and key exclusions in file then line order, the last entry for a key winning; compare each `Writes` value with `runtime` through the existing `sysctl_values_equal` (`kernelboot.rs:261-265`, the tab and space rule, which moves to `inspectah-core`). Glob assignments and glob exclusions take no part. Comparing against the winner, not against each file, removes today's false drift from the inverted read order.

`carriage()` covers `/etc` files only; a file in another directory is never carried by the config path. For each, it takes the config entry at the file's own path; failing that, for the file named by the constant `/etc/sysctl.d/99-sysctl.conf` whose link target is `/etc/sysctl.conf`, the entry at `/etc/sysctl.conf`, since the target image's systemd package ships the same link; failing that, `NoConfigEntry`. It follows no other link: a link the walker skipped, or an RPM-owned link an admin re-pointed (§ 5.1), is not carried, and naming its target's entry as the carrier would claim otherwise.

Fields deliberately absent: no `disposition`, no `locked`, no `aggregate`. The section owns no decision, so it has nothing to toggle, lock or count.

### 5.4 Redaction

The sysctl section holds host text no other pass sees: vendor and runtime files, `/etc` files read through links, and kernel values. `redact()` gains a sysctl pass in its kernel/boot block, beside the cmdline pass (`crates/pipeline/src/redaction/engine.rs:920-944`), so every stored fact is scrubbed on the snapshot object before anything serializes or displays it (`process-docs/skills/redaction-owns-the-snapshot-object.md`):

| Field | Treatment |
|---|---|
| `SysctlFileBody::Read.content` | The config-file pass, unchanged, under the file's full path (`/usr/lib/sysctl.d/50-default.conf`): the same patterns, multi-line matching, comment exemption and tokens. Lines are parsed from the scrubbed text, so a redacted value reaches the explanation, the reports and refine only as its token. |
| `Read.link_target`, `OutsideLink.target` | Scanned as one line of content; a match is replaced by its token. |
| `runtime` values | Each scanned as a `<key> = <value>` line; a match replaces the value with its token. |
| `name`, `dir`, `runtime` keys | Exempt: file names and kernel key names, exempt for the reason `ConfigFileEntry.path` is. |

Findings join `all_findings`, so `FullyRedacted` and `PartiallyRedacted` account for them, and each file's scrubbed content goes into the post-redaction content map under its path (`engine.rs:1467-1490`), so a hint against a sysctl file resolves against what survived. Where an `/etc` sysctl file also has a config entry, both come from the same bytes through the same pass and carry the same tokens. The new fields join the two tests in `crates/pipeline/src/render/mod.rs`, `every_field_of_a_content_bearing_type_is_classified` and `exactly_the_exempt_fields_survive_the_export`, scrubbed or exempt as in the table, each with a planted sentinel.

The outside-link rule (§ 5.2) is the other half: a link out of `/etc` and the sysctl directories, such as `/etc/sysctl.d/90-app.conf -> /var/lib/app/credentials`, is never read, so its content is never collected.

### 5.5 Audit report

The `#### Sysctl Overrides` table (`audit.rs:585-604`) becomes `#### Sysctl`. On an aggregate it is the one line in § 5.7. On a single host whose `sysctl` is `None` it is one line: "Sysctl explanation unavailable: `<reason>`. Sysctl files are still carried as Configuration Files.", where the reason is the kernel/boot degradation. Otherwise:

1. One line: "These values are trusted host configuration. The files that set them are carried as they were, not rewritten."
2. A table of `effective()`: `Key | File value | Runtime value | Source`. File value is the written value, or `not written` for a `Cancelled` key. Runtime value prints `same` when it matches, `not exposed` when the kernel did not list the key, and the value otherwise. Source is the deciding line's file path and line number, plus `(link to /etc/sysctl.conf)` when the file was read through a link.
3. **Runtime drift**, one line per `Differs` key: "`<key>` runs at `<runtime>`; the files request `<file value>` (`<source>`)." Above the lines, once: "These keys run at a value other than the one the files request. A tuned profile, another service, a hand-run `sysctl -w`, or a module that created the key after systemd-sysctl ran at early boot can each cause this, and some of them reapply at every boot, so the value after a reboot cannot be read from this scan. The image gets the files, not the running values." When the snapshot records an active tuned profile, the paragraph adds: "tuned profile `<profile>` is active on this host."
4. **Vendor files replaced**, one line per `VendorReplacement`, worded by the file's `carriage()`:
   - Carried (a `Config` entry that is included): "`/etc/sysctl.d/<name>` replaces `/usr/lib/sysctl.d/<name>` entirely, on the target as well, so keys the target's version of that vendor file sets will not apply from it. Compare it with the target image's copy."
   - A `Config` entry that is excluded or advisory: "`/etc/sysctl.d/<name>` replaces `/usr/lib/sysctl.d/<name>` on this host. It is not carried, so on the target the vendor file applies."
   - `NoConfigEntry`: "`/etc/sysctl.d/<name>` replaces `/usr/lib/sysctl.d/<name>` on this host. It has no Configuration Files row, so it reaches the target only if a package there ships it."
   The mask wording says "disables" in place of "replaces".
5. **Not carried**: files in `/run/sysctl.d` (generated at runtime) and `/usr/local/lib/sysctl.d` (carried only as unmanaged files under `--include-unmanaged`); every `/etc` file whose `carriage()` is an excluded or advisory entry, or `NoConfigEntry`, with that reason; and every outside link: "`<path>` links to `<target>`, outside `/etc` and the sysctl directories. inspectah did not read it, so any keys it sets are missing from the values and drift above."
6. **As written**: glob assignments, glob exclusions and unparsed lines, with file and line number.

**Literal text.** Every interpolated value in this section (keys, values, file names, paths, link targets, unparsed text, profile names, the degradation reason) goes through one helper that renders it as an inline code span Markdown cannot reinterpret:

- Control characters other than tab, and U+2028 and U+2029, become `\u{…}` escapes, so no value can end a line, a table row or a list item.
- The fence is one backtick longer than the longest backtick run in the value, with one space of padding inside each end, so embedded backticks cannot close it (CommonMark § Code spans). Inside a code span, HTML, links, images, emphasis and backslash escapes are all literal.
- In a table cell, `|` becomes `\|`. GFM splits a row into cells before inline parsing and honors that escape inside a code span, then removes the backslash (cmark-gfm `table.c`, `unescape_pipes`). No other character changes, and backslashes are not doubled, because inside a code span a backslash is already literal.
- An empty value renders as the fixed text `(empty)`.

The HTML report and refine stay as they are. The HTML report's sysctl block (`crates/pipeline/src/render/report.rs:832-849`, `templates/report/kernel.html:10-22`) takes the same blocks as the audit and renders every field through MiniJinja's HTML autoescaping; refine renders them as React text. Neither marks a sysctl field safe, and the plan adds no such marking. `section_count` for `InspectorId::KernelBoot` (`report.rs:238-240`) counts `effective()` rows, and 0 when `sysctl` is `None`.

### 5.6 Refine

The sysctl rows stop being decisions. The decision for each `/etc` sysctl file is its Configuration Files row, as for any config file.

**System Tuning returns** as a section, closing `inspectah-sysctl-section-components-are-orphaned` on its first branch: system tuning comes back (Mark, 2026-11-01). The section holds the tuned rows, which stay decisions, and the read-only sysctl panel.

Backend registry (`crates/pipeline/src/section_group.rs`):

- `system_tuning` comes off `RETIRED_SECTION_IDS` (`:18`); `version_changes` stays.
- `SectionGroup::SystemConfig::member_sections()` gains `SectionMeta { id: "system_tuning", label: "System Tuning", is_triage: true }` after `kernel_boot` (`:123-140`), and `for_section("system_tuning")` returns `SystemConfig`. `/api/groups` lists it from there (`crates/web/src/handlers.rs:1078-1101`), which is what puts it in the sidebar.
- `is_triage` is true because the tuned rows are decisions. **The badge counts tuned decision rows only.** The sysctl panel holds no decisions and never adds to the count, so a host with sysctl files and no tuned selection shows `0`, cleared.
- Section actions: in the `SystemConfig` arm of the batch toggle (`handlers.rs:811-858`), the tuned op moves from the `kernel_boot` filter to a `system_tuning` filter (`section_only` absent or `"system_tuning"`). The sysctl ops are removed with `ItemId::Sysctl`. A group-level toggle reaches the tuned row as before.
- Tests that pin retirement flip: `section_group.rs:316` and the unknown-slug list in `handlers.rs:1845-1850`.

**Read-only DTO** (`crates/web/src/web_types.rs`). `ViewResponse.sysctls: Vec<SysctlDecisionDto>` (`:314`) is replaced by `sysctl: SysctlViewDto`, always present; `SysctlDecisionDto` (`:246-255`) is deleted.

```rust
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SysctlViewDto {
    /// Single host with a collected section.
    Host(SysctlHostView),
    /// Aggregate: the explanation is per host (§ 5.7).
    Aggregate,
    /// Single host whose `kernel_boot` or `sysctl` is `None`.
    Unavailable { reason: String },
}

pub struct SysctlHostView {
    pub settings: Vec<SysctlSettingDto>,        // effective(), in key order
    pub drift: Vec<SysctlDriftDto>,             // the Differs rows
    pub vendor_replacements: Vec<VendorReplacementDto>, // with carriage wording
    pub not_carried: Vec<NotCarriedDto>,        // § 5.5 item 5
    pub as_written: Vec<AsWrittenLineDto>,      // § 5.5 item 6
    pub files: Vec<SysctlFileDto>,              // every file, with its config row path when it has one
    pub tuned_active: Option<String>,
}
```

One projection builds it: `DecisionProjection.sysctls` (`crates/refine/src/projection/types.rs:27`) and its `classify_sysctls` call (`projection/decisions.rs:28`) are replaced by a `project_sysctl` function that reads the session's materialized snapshot, so `carriage()` sees the current config decisions and a toggled Configuration Files row changes the panel on the next view. The single-host adapter calls it. The aggregate handlers always send `Aggregate`.

Empty-state rule: `Host` renders whenever the section has any file, so a mask-only or glob-only host shows its lines with an empty settings table. "No system tuning detected." appears only when there are no tuned rows and the sysctl view is `Host` with no files. `Aggregate` and `Unavailable` always render their one line.

Frontend (needs review from the web frontend lane in round 2): `system_tuning` comes off `RETIRED` (`crates/web/ui/src/App.tsx:183`); `MainContent` renders `SystemTuningSection` for it; `decisionCount` in `Sidebar.tsx` gains a `system_tuning` arm returning the tuned row count; `SystemTuningSection` takes `sysctl: SysctlViewDto` and renders the panel in all three states, independent of the tuned rows (today it hides sysctl when the array is empty, `SystemTuningSection.tsx:22`, `:40`); `SysctlSection.tsx` is rewritten as read-only, with every `/etc` file that has a config row linking to it; every value renders as React text. The keyboard map (`useKeyboard.ts:18`) and the search label (`GlobalSearch.tsx:46`) already name `system_tuning`.

**Removed:** `ItemId::Sysctl` (`crates/refine/src/types.rs:110-112`) and its arms in `session.rs` (`:350-353`, `:436`, `:2180-2185`, `:2881-2886`); `RefinedSysctl` and `classify_sysctls` with its view-only deny list (`classify.rs:707-758`), which hid `kernel.sysrq` from one view while the build still shipped it; `TriageReason::SysctlBaselineMatch`, `SysctlFileBackedOverride` and `SysctlNoBaseline` (`types.rs:354-356`); the bulk include op for sysctls (`crates/web/src/handlers.rs:838-847`); the sysctl decision rows in the Kernel and Boot context list (`crates/web/src/adapter.rs:1026-1042`); the aggregate sysctl decision items and `build_sysctl_variants` (`crates/web/src/aggregate_handlers.rs:867-929`, `:1941-1983`); `RefSysctlOverride` and the "merged into" pointer (`crates/refine/src/projection/types.rs:248-251`, `:268`); the `sysctl:` mutation ids and the `sysctl` branch of `shadowOwner.ts`.

A config row of category `Sysctl` is an ordinary toggleable row: no entry is `Inventory` because of sysctl any more, so `InventoryNotToggleable` no longer applies to it.

### 5.7 Fleet aggregate

No new merge logic. Sysctl files merge as config entries, by path, with the existing per-path content variants (`ConfigFileEntry::content_variant_key`, `merge.rs:102-126`). Hosts that disagree on a file's content produce variants the operator picks between, as for any config file.

**The per-key sysctl merge goes away, and the aggregate carries `sysctl: None`.** It is removed, not kept as explanation, for three reasons:

- An effective value, a drift line and a vendor replacement are true of one host's files and one host's kernel. A merged section either picks one host's view and presents it as the fleet's, which is today's whole-struct winner (`merge.rs:554-568`) and is what made the reconciliation item possible, or needs per-key value variants, which is new merge logic.
- Nothing downstream needs it: carriage is fully in the config section.
- The aggregate audit and the aggregate refine panel each say one line instead: "Sysctl explanation is per host and is not merged; each host's own scan has it. Sysctl files are carried as Configuration Files, with a variant per differing content."

Cost: the vendor replacement advisory is not shown at fleet level. The operator sees it in each host's scan. Accepted, not open.

`merge_kernelboot_sections` drops its `sysctl_overrides` merge (`merge.rs:1823-1827`) and the `AggregateMergeable for SysctlOverride` impl (`:321-333`).

## 6. Docs and skills made stale

The plan updates each.

| File | What changes |
|---|---|
| `process-docs/skills/instruction-line-values.md` | `:17` and the `:43` table row name the sysctl drop-in, which no longer exists. The `:131-140` trust paragraph keeps the ruling and its record, and says sysctl values now reach the image only inside verbatim copied files, so no sysctl value reaches an instruction line. |
| `process-docs/skills/cross-inspector-shadow-passes.md` | `:11-12`, `:47-63`, `:122`: the sysctl pass and the two early-return tests that pair it with the service pass. One pass remains; the rule for adding a second still stands. |
| `process-docs/skills/aggregate-vs-single-host-behavioral-split.md` | `:63-76`: the worked example describes a state that can no longer exist. Replace with one that still can, or drop it. |
| `process-docs/skills/web-disposition-contract.md` | `:66`, `:81-84`, `:151-158`, `:170`: `SysctlDecisionDto` as a decision, and sysctl-shadowed config entries in the advisory bucket. |
| `process-docs/skills/redaction-owns-the-snapshot-object.md` | The sysctl section's fields join the classified set (§ 5.4). |
| `process-docs/skills/real-host-validation-runs.md` | `:331-341`: the tab and space rule stays right; the note that `SysctlOverride.default` holds the file value goes with the struct. |
| `process-docs/skills/containerfile-value-quoting.md` | `:51`, `:82`: the drop-in as a built line vector. |
| `process-docs/skills/finding-disposition-serde-defaults.md` | `:40`: `SysctlOverride` in the list. |
| `docs/reference/output-artifacts.md` | `:66`, `:182`, `:274-277`, `:356-359`, `:410-413`: the `sysctl/` directory and `99-inspectah-migrated.conf`. |
| `docs/reference/triage-classification.md` | `:13`, `:78`, `:92`, `:223`, `:308`, `:374`: sysctl overrides as triaged decisions, "Merged into sysctl settings". |
| `docs/reference/inspector-coverage.md` | `:341`, `:464`: what the collector reads and what it records. The "What redaction covers" table (`:482-494`): the Kernel/Boot row adds sysctl file content, link targets and runtime values. |
| `docs/reference/snapshot-schema.md` | `:64`: `kernel_boot` contents. |
| `docs/how-to/review-and-refine.md` | `:127-129`: "Merged into sysctl settings"; System Tuning as a section. |
| `docs/tutorials/first-migration.md` | `:184`, `:259-261`, `:305`, `:472`: sysctl overrides, the merged drop-in, inventory sysctl files. |
| `docs/diagrams/data-flow.html`, `docs/diagrams/software-architecture.html` | Any sysctl drop-in or shadow arrow. |

**CHANGELOG:** user-visible. The plan adds an `[Unreleased]` entry: sysctl files are carried as the host had them, `99-inspectah-migrated.conf` is no longer written, runtime drift is reported and not carried, the per-key sysctl toggle is gone, System Tuning is back as a section with a read-only sysctl panel, the collector now follows systemd's precedence including key exclusions, the audit table shows file and runtime values, and an RPM-owned config symlink re-pointed to `/dev/null` is carried as an empty file. Snapshots from earlier releases must be re-scanned (schema bump).

## 7. Out of scope

- Glob expansion against `sysctl -a`.
- Comparing host vendor files with the target image's.
- Runtime-only keys that no file names.
- tuned profile migration.
- Carrying `/usr/local/lib/sysctl.d` by any route other than the existing unmanaged-file path (§ 4.2), and how a `/usr/local` finding lands on a CoreOS-lineage base, where `/usr/local` is a link into `/var`.
- Carrying an RPM-owned config symlink an admin re-pointed to a file other than `/dev/null` (§ 5.1). It belongs with the defaults work.
- Literal-text encoding in other audit tables. This spec adds the helper for the sysctl section only; other sections that interpolate raw values are a separate defect.

## 8. Testing

**Built from collector-produced fields.** Every case below runs the real `KernelbootInspector` and, where carriage is asserted, the real `ConfigInspector`, over a `MockExecutor` holding host files, links, `rpm -Va` rows and `sysctl -a` output, through the scan path (`collect()`, `validate`, `redact`, then render), as `crates/cli/tests/scan_shadow_test.rs` does today. Refine cases load the resulting tarball into a session. No test builds a `SysctlSection`, `SysctlFile` or `ConfigFileEntry` by hand to stand for a host layout: a hand-built fixture can encode a layout the collector cannot produce, and the resulting test passes against nothing real. Unit tests of `effective()` may build `SysctlSection` directly only to pin the resolution rule, and each such case has a collector-driven twin in this list.

Required cases:

1. **Each directory is read.** Each of the four directories holds one file that alone sets a key unique to it. All four keys are effective, each attributed to its own file.
2. **Priority by name across directories.** `/usr/lib/sysctl.d/50-default.conf` sets `k = 1`; `/etc/sysctl.d/90-local.conf` sets `k = 2`; `/run/sysctl.d/95-run.conf` sets `k = 3`; `/usr/local/lib/sysctl.d/97-local.conf` sets `k = 4`. The effective value is `4`, with the other three in `overwritten` in apply order. Then the same with `/etc` at `40-` and vendor at `50-` alone: the vendor value wins. Today's reader gets the first case wrong.
3. **Same-name shadow, each adjacent pair.** For each pair (`/etc` over `/run`, `/run` over `/usr/local/lib`, `/usr/local/lib` over `/usr/lib`), the two directories hold `50-x.conf`; the higher sets `a`, the lower sets `a` and `b`. `b` is absent from `effective()` and `a` comes from the higher file. For the `/etc` over `/usr/lib` case, a vendor replacement line names `50-x.conf`.
4. **Unowned mask.** `/etc/sysctl.d/50-coredump.conf -> /dev/null`. No key from the vendor file is effective, a mask line is reported, and the export holds an empty regular `/etc/sysctl.d/50-coredump.conf`.
5. **RPM-owned mask.** `/etc/sysctl.d/99-sysctl.conf -> /dev/null` with `rpm -Va` reporting it as `....L....`, and `/etc/sysctl.conf` setting `k`. `config.files` holds exactly one entry at that path, `RpmOwnedModified`, included, empty content, flags `....L....`; the export holds an empty regular `/etc/sysctl.d/99-sysctl.conf`; `k` is not effective and a mask line is reported. A twin with an `L`-only row whose link points at another file produces no config entry, as today.
6. **`/etc/sysctl.conf` through the symlink, then excluded.** `/etc/sysctl.d/99-sysctl.conf -> ../sysctl.conf` with `rpm -Va` reporting `/etc/sysctl.conf` as `S.5....T.`. Its keys are attributed to `99-sysctl.conf` with the link note; `carriage()` names the `/etc/sysctl.conf` entry; the config entry is included and the export holds `/etc/sysctl.conf`. Then refine excludes that row: the export lacks `/etc/sysctl.conf`, and the sysctl panel lists `99-sysctl.conf` as not carried because its row is excluded.
7. **Key spellings.** `net/ipv4/ip_forward = 1` in a file and `net.ipv4.ip_forward = 1` from `sysctl -a`: one key, `Matches`, no drift line. And `net/ipv4/conf/enp3s0.200/forwarding = 1` in one file with `net.ipv4.conf.enp3s0/200.forwarding = 0` in a later one, against a `sysctl -a` line for `net.ipv4.conf.enp3s0/200.forwarding`: one key, value `0`, the first in `overwritten`. A normalizer that replaced every `/` with `.` fails this.
8. **Repeated assignment in one file.** One file sets `k = 1` on line 2 and `k = 2` on line 5: value `2` from line 5, line 2 in `overwritten`.
9. **`-` prefixed assignment.** `-net.ipv6.conf.all.disable_ipv6 = 1`: the key is `net.ipv6.conf.all.disable_ipv6`, `ignore_failure` is set, and it takes part in precedence and drift like any key.
10. **Key exclusion, both orders.** `vm.swappiness = 10` then, in a later file, `-vm.swappiness`, against runtime `60`: the outcome is `Cancelled`, the audit row says `not written`, and no drift line is reported. Reversed (exclusion first, assignment later): `Writes` `10`, and drift against `60`.
11. **Drifted key.** A file sets `vm.swappiness = 10`; `sysctl -a` reports `60`. The audit holds the drift line with both values and the source, the shared paragraph, and no sentence predicting the value after a reboot; the export holds no `sysctl/` directory, no `99-inspectah-migrated.conf`, and no Containerfile line naming either.
12. **One drifted key does not drop the file.** `/etc/sysctl.d/90-tuning.conf` sets seven keys; `sysctl -a` differs on one. The exported `/etc/sysctl.d/90-tuning.conf` is byte-identical to the host file, the config entry is included and not `Inventory`, and exactly one drift line is reported. This is the end-to-end test the shadowed-source item asked for.
13. **Whitespace is not drift.** `net.ipv4.ip_local_port_range = 32768 60999` against a tab-separated runtime value: `Matches`, no drift line.
14. **Glob reported, not expanded.** `net.ipv4.conf.*.rp_filter = 2` appears in the audit as written with file and line, and adds no row to `effective()`.
15. **Mask-only and glob-only hosts.** A host whose only sysctl entry is a mask, and one whose only line is a glob: the System Tuning panel renders its lines with an empty settings table, and the sidebar lists System Tuning with a tuned-only badge.
16. **Collection failures.** In turn: `sysctl -a` exits non-zero; `/run/sysctl.d` fails to list with an error other than not-found; an entry is a dangling link. Each gives `sysctl == None`, a kernel/boot degradation naming the cause, the unavailable line in the audit and the `Unavailable` panel, no drift line and no `not exposed` claim, and the `/etc` sysctl files still carried as config entries.
17. **Outside link canary.** `/etc/sysctl.d/90-app.conf -> /var/lib/app/credentials`, which holds `password: canary-outside`. The file is `OutsideLink`, its target is reported, and `canary-outside` is absent from the snapshot JSON, the audit, the HTML report and the refine view.
18. **Redacted content canary.** An unowned `/etc/sysctl.d/80-app.conf` holding a private-key block and `db_password = canary-inside`, and a `/usr/lib/sysctl.d/10-vendor.conf` holding `api_token = canary-vendor`. Under normal redaction neither canary nor the key material appears in the snapshot JSON, the audit, the HTML report or the refine view; the findings are counted in the redaction state.
19. **Literal rendering.** A file sets `kernel.core_pattern = |/usr/lib/systemd/systemd-coredump %P %u %g %s %t %c %h`; another holds unparsed lines with `<img src=x>`, `[link](http://example.com)` and a run of three backticks; a file name holds a newline. The audit is parsed with a GFM-conformant parser added as a dev-dependency: the sysctl table has the expected rows, each with four cells whose code text equals the value exactly, and the section holds no HTML, link or image node. The HTML report holds the same strings escaped.
20. **`SysctlKey` on the wire.** A section with interface-name keys round-trips through JSON unchanged. A snapshot whose runtime map holds `net/ipv4/ip_forward` fails to load, with that key in the error.
21. **Aggregate.** Two hosts with different content for one `/etc/sysctl.d` path produce two config variants and `kernel_boot.sysctl == None`; the aggregate audit prints the per-host line and the aggregate refine view sends `Aggregate`.

Tests that encode the removed behavior are deleted or rewritten, not left skipped: `scan_shadow_test.rs`, the sysctl cases in `configtree_single_source_test.rs`, `instruction_injection_test.rs`'s sysctl row, the paired early-return tests in `collect.rs`, the `SysctlOverride` merge cases in `aggregate_merge_test.rs` and `aggregate_e2e_test.rs`, the sysctl contract snapshots in `crates/web/tests/snapshots/`, the retirement assertions for `system_tuning` (`section_group.rs`, `handlers.rs`, `Sidebar.test.tsx:215`), and the frontend tests for `shadowOwner`, `SysctlSection`, `SystemTuningSection` and the shadowed-config rows.

## 9. Backlog items

| Item | Disposition |
|---|---|
| `inspectah-sysctl-render-should-mirror-the-source-files` | **Closes.** Decision 1 is its requirement. |
| `inspectah-sysctl-shadowed-source-drops-non-override-keys` | **Closes.** The shadow pass is deleted; § 8 case 12 is its end-to-end proof. |
| `inspectah-sysctl-whitespace-false-positive-has-no-end-to-end-shadow-test` | **Moot.** No sysctl shadow pass remains to test. The comparison rule survives for drift lines and keeps its test (case 13). |
| `inspectah-aggregate-sysctl-disposition-reconciliation` | **Moot.** No sysctl disposition and no shadow are left to reconcile; files merge by path only. |
| `inspectah-sysctl-override-default-field-holds-the-file-value` | **Closes.** The struct is deleted; the new types name the file value as such. |
| `inspectah-sysctl-section-components-are-orphaned` | **Closes**, on the branch the owner chose: System Tuning returns, registered in the backend section groups, with sysctl read-only (§ 5.6). The `shadowOwner.ts` route it worried about is removed rather than retargeted. |
| `inspectah-aggregate-merge-does-not-reconcile-sysctl-shadowing` | Already closed (superseded). No change. |
| `inspectah-sysctl-drift-false-positive-on-tab-versus-space` | Already closed (done). Its rule carries over unchanged. |

## 10. Open questions

None.
