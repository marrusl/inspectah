# Unmanaged /usr Presentation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the already-complete /usr walk an output surface: an "Unmanaged /usr" section in refine (single-host and aggregate), the HTML report, the Containerfile export, and the audit report, with /usr entries treated as ordinary Actionable findings.

**Architecture:** `usr_entries` is collected today and read by nothing. This plan adds an explicit entry-kind field to `UnmanagedUsrEntry`, teaches aggregate merge to preserve the vector with path-keyed prevalence, adds an `ItemId::UnmanagedUsr` refine identity so the standard include/exclude machinery applies, then wires the four renderers and the two web surfaces. No new disposition model, no new decision vocabulary: every surface reuses components, ops, and keyboard bindings that already exist for sibling Actionable sections.

**Tech Stack:** Rust 2024 workspace (`inspectah-core`, `inspectah-collect`, `inspectah-refine`, `inspectah-pipeline`, `inspectah-cli`, `inspectah-web`), serde, minijinja report templates, React + TypeScript + PatternFly web UI, `insta` snapshot tests, `vitest` frontend tests.

**Spec:** `process-docs/specs/proposed/2026-08-15-usr-walk-presentation-design.md`. That design note is authoritative. Where this plan and the note disagree, the note wins and the plan is the bug.

**Target release:** v0.9.0-beta.3.

## Global Constraints

- **Clippy clean:** `cargo clippy --all-targets -- -D clippy::all` with zero warnings. Non-negotiable.
- **Format:** `cargo fmt --check` must pass before every commit.
- **Cargo is not on the subagent PATH.** Prefix Rust commands with the toolchain bin: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"`.
- **Commit format:** `type(scope): description`, imperative mood. Attribution trailer: `Assisted-by: Claude Code (<model>)`. No other identifiers. **No team member names anywhere** (public repo).
- **Never push.** Commit freely; Mark pushes.
- **Section id:** `unmanaged_usr`. **Section label:** `Unmanaged /usr`. **Triage section:** `is_triage: true`. These exact strings appear in `section_group.rs`, the frontend `SECTION_LABELS` map, the report anchor, and the audit heading. Do not vary them.
- **Containerfile block header:** exactly `# === Unmanaged /usr ===`.
- **Section framing copy (verbatim, all surfaces):**
  > In image mode, /usr ships from the container image and stays read-only at runtime. The files below live under /usr on this host but belong to no RPM package, so a rebuilt image will not carry them unless you include them in the export. For content that should be package-managed, building an RPM that owns it is the durable fix; use include only for what genuinely needs to travel with the image as-is.
- **Empty-state copy (verbatim):** `Every file under /usr on this host is owned by an RPM package. This host's /usr is image-clean.`
- **Not-scanned copy (verbatim):** `This snapshot was collected without --include-unmanaged, so /usr was not checked. Re-scan with --include-unmanaged to check it.`
- **Voice rules for all user-visible strings:** no em dashes; do not use the word "immutable" (say "image-based" or "read-only"); do not use "shape" as a noun.
- **Product tenet:** migration assistance, not a best-practices suite. No hygiene-enforcing states, taxonomies, gates, or scoring anywhere in this feature. /usr entries are ordinary Actionable findings with the ordinary toggle.
- **Default sort everywhere:** `total_size_bytes` descending, then `path` ascending. No sort controls in beta.3.
- **Mode-divergence rule:** every behavior added here is checked in both `RefineMode::SingleHost` and `RefineMode::Aggregate` (see `process-docs/skills/aggregate-vs-single-host-behavioral-split.md`).

## Verification Findings (resolved before planning)

The design note listed four items as "needs verification at implementation time." All four are resolved. Implementers should not re-litigate these.

**1. Tier 2 bundling mechanics, and whether /usr can ride the same path.** It can, with one gap. The flow is: `crates/cli/src/commands/scan.rs:828` calls `bundle_unmanaged_files(&unmanaged.items, render_dir.path())` after `render_all` and before `create_tarball`. That function (`scan.rs:1099`) skips entries where `!item.disposition.is_included()`, strips the leading `/`, and `std::fs::copy`s each file to `render_dir/unmanaged/<rel_path>` (symlinks are recreated, not followed). The Containerfile renderer then emits `COPY unmanaged/<rel>` lines. On refine export, `extract_payload_dirs_from_tarball` (`crates/refine/src/session.rs:3161`) re-extracts `unmanaged/*` from the source tarball, filtered against the projected snapshot's included paths. **Gap:** `bundle_unmanaged_files` copies single files only, and the extract filter uses exact-set membership (`included_unmanaged.contains(file_rel)`). /usr entries are collapsed *directories*, so both need subtree handling. Task 3 fixes the filter; Task 5 fixes the bundler and the size prompt.

**2. Whether refine projection passes `usr_entries` through untouched.** Yes. `project_snapshot` (`crates/refine/src/session.rs:1911-1912`) begins `let mut snap = self.original.clone()`, so every field not explicitly rewritten survives. `usr_entries` is never rewritten, so it already passes through. What is missing is the ability to *apply* a decision: there is no `ItemId` variant for a /usr entry, so no `SetInclude` can target one. Task 3 adds `ItemId::UnmanagedUsr { path }` plus the validation, projection, and batch-op arms.

**3. The HTML report's attention-state styling hooks.** The `section()` macro in `crates/pipeline/templates/report/section.html` takes a `state` parameter of `"normal" | "degraded" | "failed"`, sourced from `section_state(id, &snap.completeness)` (`crates/pipeline/src/render/report_data.rs:18`). That state reflects *collection completeness*, not content, so it is the wrong hook. The only content-driven attention class today is the hardcoded `{%- elif id == 'warnings' %} report-section--warning` on line 6 of the macro. The CSS class `.report-section--warning` already exists (`crates/pipeline/assets/report.css:368`). Task 12 generalizes that one-off into an optional `attention=false` macro parameter and reuses the existing class. No new CSS.

**4. Pre-export warning surface for missing content.** Two surfaces, both new for /usr. The Containerfile block emits a `# MISSING FROM BUILD CONTEXT:` comment line per included path whose bytes are not in the archive (Task 6), and the audit report's Unmanaged /usr section repeats the same list (Task 7). `render_software_sections` in `crates/pipeline/src/render/audit.rs:924` currently renders only `non_rpm_software`, so there is no existing unmanaged content in the audit report to extend.

## Schema Version Decision

**A schema bump is required. `SCHEMA_VERSION` goes 22 to 23, and `MIN_SCHEMA` goes 21 to 23.**

Current state (`crates/core/src/snapshot.rs:21,103`): `SCHEMA_VERSION = 22`, `MIN_SCHEMA = 21`. Note that this contradicts `process-docs/skills/snapshot-schema-versioning.md`, which states `MIN_SCHEMA == SCHEMA_VERSION` and describes exact-match gating. The skill is stale; the code currently accepts a two-version window. Task 13 corrects the skill.

Why a bump is required: `UnmanagedUsrEntry` gains a `kind: UsrEntryKind` field whose entire purpose is to record something that cannot be derived from existing data. Today single-file versus collapsed-directory is only inferable from `file_type != Other`, which misclassifies any single file that lands on `FileType::Other`. A `#[serde(default)]` on the new field would therefore silently mislabel rows in every pre-existing snapshot: a collapsed directory defaulting to `File` renders as "File, 214 files," which is nonsense the user cannot detect. Adding aggregate prevalence fields to the same struct compounds it.

Release packaging consequence, and the reason this decision belongs in the plan rather than in implementation: setting `MIN_SCHEMA = 23` means every snapshot and every aggregate on disk stops loading with a clean `UnsupportedVersion` error and must be re-scanned. Aggregate re-aggregation additionally requires re-scanning all constituent hosts. This must appear in the v0.9.0-beta.3 release notes.

**Alternative considered and rejected:** keep `MIN_SCHEMA = 22` and put `#[serde(default)]` on `kind`. This preserves loadability for v22 snapshots at the cost of silently wrong kind badges on exactly the snapshots the window exists to serve. The repo convention is already "no old tarball compatibility, re-scan instead" (`feedback_no_old_tarball_compat`), so the window buys nothing here.

**This is settled: `SCHEMA_VERSION = 23`, `MIN_SCHEMA = 23`.** There is no fallback variant in this plan and no decision left open. Implementers write both constants as 23 and do not add a serde default to `kind`.

**Fixtures that hardcode a schema version.** Closing the window to 23 breaks every test fixture that pins an older number, and those reach past the construction sites Task 1's Step 5 names. Known sites, all of which must move to 23:

- `crates/refine/tests/normalize_test.rs` — schema 21 in the inline JSON of every test (lines 4, 16, 28, 39, 51, 63, 74, 86, 109, 131).
- `crates/cli/src/commands/aggregate.rs:561,567,651,659,709,720` — in-file tarball fixtures, reached through `InspectionSnapshot::load()` by aggregate tarball loading.
- `crates/cli/tests/refine_e2e_test.rs:15` — the end-to-end tarball fixture.

Task 1's Step 6 full-workspace run surfaces these as `UnsupportedVersion` failures. Fix them in that step rather than deferring to Task 13; they are part of the schema bump, not a docs task.

## File Structure

**Created:**
- `crates/pipeline/src/render/unmanaged_usr.rs` — Containerfile block renderer for /usr entries. Kept separate from `unmanaged.rs` because the two sections mean different things and share no grouping logic (`unmanaged.rs` groups by parent directory; /usr entries are already collapsed).
- `crates/pipeline/templates/report/unmanaged-usr.html` — report section template, sibling of `nonrpm.html`.
- `crates/web/ui/src/components/UnmanagedUsrList.tsx` — the decision grid for the section, single-host and aggregate.
- `crates/web/ui/src/components/__tests__/UnmanagedUsrList.test.tsx` — component tests.
- `crates/pipeline/tests/usr_export_test.rs` — end-to-end Containerfile + audit assertions for the section.

**Modified:**
- `crates/core/src/types/nonrpm.rs` — `UsrEntryKind` enum, `UnmanagedUsrEntry` fields, corrected type comment.
- `crates/core/src/snapshot.rs` — schema version constants.
- `crates/collect/src/inspectors/nonrpm.rs` — collector populates `kind`.
- `crates/core/src/aggregate/merge.rs` — `merge_usr_entries` and its call site.
- `crates/refine/src/types.rs` — `ItemId::UnmanagedUsr`.
- `crates/refine/src/session.rs` — op validation, projection arm, tarball extract filter.
- `crates/pipeline/src/section_group.rs` — section registration.
- `crates/pipeline/src/render/mod.rs` — module declaration.
- `crates/pipeline/src/render/containerfile.rs` — block insertion.
- `crates/pipeline/src/render/audit.rs` — audit section.
- `crates/pipeline/src/render/report.rs` — report context data.
- `crates/pipeline/templates/report/section.html`, `base.html` — attention parameter, section include.
- `crates/cli/src/commands/scan.rs` — /usr bundling and the split size prompt.
- `crates/web/src/web_types.rs`, `adapter.rs`, `handlers.rs`, `aggregate_handlers.rs` — DTOs, projection, batch ops, aggregate section.
- `crates/web/ui/src/api/types.ts`, `App.tsx`, `MainContent.tsx`, `Sidebar.tsx` — single-host frontend wiring.
- `crates/web/ui/src/components/aggregate/AggregateItemRow.tsx`, `AggregateSection.tsx`, `ItemDetailPane.tsx` — aggregate section metadata. Aggregate mode renders through these, not through `UnmanagedUsrList`; see Task 11. `ContainerfilePanel.tsx` is deliberately untouched.
- `CHANGELOG.md`, `docs/how-to/review-and-refine.md`, `docs/reference/output-artifacts.md` — user-facing docs.
- `process-docs/skills/snapshot-schema-versioning.md` — correct the stale exact-match claim.

## Task Summary

| # | Lane | Deliverable |
|---|------|-------------|
| 1 | Tang (core, collect) | Entry-kind field, prevalence fields, type-comment fix, schema bump |
| 2 | Tang (core) | Aggregate merge preserves `usr_entries` with prevalence |
| 3 | Tang (refine) | `ItemId::UnmanagedUsr`, projection arm, tarball extract filter |
| 4 | Tang (pipeline) | Section registration in `section_group.rs` |
| 5 | Tang (cli) | Scan-time /usr bundling and the split size prompt |
| 6 | Tang (pipeline) | Containerfile `=== Unmanaged /usr ===` block |
| 7 | Tang (pipeline) | Audit report Unmanaged /usr section |
| 8 | Kit (web backend) | DTOs and single-host adapter projection |
| 9 | Kit (web backend) | Aggregate handler section and batch include/exclude routing |
| 10 | Kit (web UI) | Single-host decision grid, states, keyboard, screen reader |
| 11 | Kit (web UI) | Aggregate row and detail-pane metadata for the /usr section |
| 12 | Kit (report) | HTML report section, template plus renderer context |
| 13 | Tang (docs) | CHANGELOG, user docs, skill correction |

---

### Task 1: Entry kind, prevalence fields, and schema bump

**Lane:** Tang (core, collect)

**Files:**
- Modify: `crates/core/src/types/nonrpm.rs:177-195`
- Modify: `crates/core/src/snapshot.rs:21`, `crates/core/src/snapshot.rs:103`
- Modify: `crates/collect/src/inspectors/nonrpm.rs:2402-2434`
- Test: `crates/core/src/types/nonrpm.rs` (inline `mod tests`), `crates/collect/src/inspectors/nonrpm.rs:4085-4130` (inline `mod tests`)

**Interfaces:**
- Produces: `UsrEntryKind` enum with variants `File` and `Directory`, `#[serde(rename_all = "snake_case")]`, deriving `Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize`. Consumed by Tasks 2, 6, 7, 8, 12.
- Produces: `UnmanagedUsrEntry` gains `kind: UsrEntryKind`, `aggregate: Option<AggregatePrevalence>`, `counts_vary: bool`, `sizes_vary: bool`. Consumed by Tasks 2, 3, 6, 7, 8, 9, 12.
- Produces: `UnmanagedFileSection` gains `usr_bundled: bool` (true when scan copied /usr bytes into the archive). Consumed by Tasks 5, 6, 7.
- Produces: `SCHEMA_VERSION = 23`, `MIN_SCHEMA = 23`.

- [ ] **Step 1: Write the failing serde tests**

Add to the inline `mod tests` in `crates/core/src/types/nonrpm.rs`:

```rust
#[test]
fn usr_entry_kind_roundtrips_as_snake_case() {
    let json = serde_json::to_string(&UsrEntryKind::Directory).unwrap();
    assert_eq!(json, "\"directory\"");
    let parsed: UsrEntryKind = serde_json::from_str("\"file\"").unwrap();
    assert_eq!(parsed, UsrEntryKind::File);
}

#[test]
fn usr_entry_carries_kind_independent_of_file_type() {
    // A single file that classifies as Other must still read as File.
    // Inferring from `file_type != Other` is what this field replaces.
    let entry = UnmanagedUsrEntry {
        path: "/usr/share/vendor-blob".into(),
        file_count: 1,
        total_size_bytes: 4096,
        file_type: FileType::Other,
        kind: UsrEntryKind::File,
        disposition: FindingKind::included(),
        aggregate: None,
        counts_vary: false,
        sizes_vary: false,
    };
    let json = serde_json::to_string(&entry).unwrap();
    let parsed: UnmanagedUsrEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.kind, UsrEntryKind::File);
    assert_eq!(parsed, entry);
}

#[test]
fn usr_entry_varies_flags_default_false_and_are_omitted_when_false() {
    let entry = UnmanagedUsrEntry {
        path: "/usr/lib/custom-agent".into(),
        file_count: 214,
        total_size_bytes: 39_845_888,
        file_type: FileType::Other,
        kind: UsrEntryKind::Directory,
        disposition: FindingKind::included(),
        aggregate: None,
        counts_vary: false,
        sizes_vary: false,
    };
    let json = serde_json::to_string(&entry).unwrap();
    assert!(!json.contains("counts_vary"), "false flags must be skipped: {json}");
    assert!(!json.contains("aggregate"), "None aggregate must be skipped: {json}");
}
```

Add to the inline `mod tests` in `crates/core/src/snapshot.rs`:

```rust
#[test]
fn schema_min_equals_current_so_older_snapshots_are_rejected() {
    assert_eq!(SCHEMA_VERSION, 23);
    let older = format!("{{\"schema_version\": 22}}");
    match InspectionSnapshot::load(&older) {
        Err(SnapshotError::UnsupportedVersion(22)) => {}
        other => panic!("expected UnsupportedVersion(22), got {other:?}"),
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test -p inspectah-core usr_entry`

Expected: FAIL to compile with "cannot find type `UsrEntryKind`" and "struct `UnmanagedUsrEntry` has no field named `kind`".

- [ ] **Step 3: Add the type and fields**

Replace `crates/core/src/types/nonrpm.rs:177-195` with:

```rust
/// Whether an unmanaged /usr entry is a single file or a collapsed
/// directory subtree.
///
/// This is recorded explicitly rather than inferred. Inferring it from
/// `file_type != Other` misclassifies any single file the type sniffer
/// cannot categorize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsrEntryKind {
    /// A single unowned file.
    File,
    /// A collapsed directory subtree; `file_count` and `total_size_bytes`
    /// are the rollup for everything beneath it.
    Directory,
}

/// A collapsed directory (or single file) under /usr that is not owned
/// by any installed RPM package. Produced by the /usr walk with ancestor
/// collapse: the shallowest unowned directory is reported rather than
/// individual files, reducing noise.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnmanagedUsrEntry {
    /// Absolute path of the collapsed directory or individual file.
    pub path: String,
    /// Number of unmanaged files rolled up under this entry. In aggregate
    /// mode this is the maximum across contributing hosts; see
    /// `counts_vary`.
    pub file_count: u32,
    /// Total size in bytes of all rolled-up files. In aggregate mode this
    /// is the maximum across contributing hosts; see `sizes_vary`.
    pub total_size_bytes: u64,
    /// Detected file type. Meaningful for `UsrEntryKind::File`; `Other`
    /// for collapsed directories.
    pub file_type: FileType,
    /// Single file or collapsed directory.
    pub kind: UsrEntryKind,
    /// Standard Actionable finding disposition. /usr entries default to
    /// included on a single host and follow the ordinary include/exclude
    /// toggle; aggregate zone machinery applies in aggregate mode.
    pub disposition: FindingKind,
    /// Aggregate prevalence, populated by aggregate merge only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<AggregatePrevalence>,
    /// True when contributing hosts reported different `file_count`
    /// values for this path. Renders as "up to N files".
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub counts_vary: bool,
    /// True when contributing hosts reported different
    /// `total_size_bytes` values for this path. Renders as "up to N MB".
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub sizes_vary: bool,
}
```

Add `usr_bundled` to `UnmanagedFileSection` immediately after `usr_entries` (`crates/core/src/types/nonrpm.rs:203`):

```rust
    /// True when the scan copied the bytes for included /usr entries into
    /// the archive under `unmanaged/usr/`. False means the export's COPY
    /// lines have no build-context content and must say so per path.
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub usr_bundled: bool,
```

Confirm `AggregatePrevalence` is in scope in this module; if the file does not already import it, add `use crate::types::aggregate::AggregatePrevalence;` at the top. Confirm `crate::is_false` exists (it is used by the existing `skip_serializing_if` boolean pattern documented in `process-docs/skills/snapshot-schema-versioning.md`); if the helper lives elsewhere, use the path that file already uses for boolean skipping.

In `crates/core/src/snapshot.rs`, set line 21 to `pub const SCHEMA_VERSION: u32 = 23;` and line 103 to `const MIN_SCHEMA: u32 = 23;`. Both constants are 23; see § Schema Version Decision. There is no serde default on `kind`.

- [ ] **Step 4: Update the collector to populate `kind`**

`crates/collect/src/inspectors/nonrpm.rs` builds the entry at roughly line 2425 inside `collapse_usr_entries` (the function starting at ~2402). The collapse logic already knows whether it emitted a rolled-up directory or a lone file. Set `kind` from that branch, not from `file_type`. Add `usr_entries` fields to every struct literal the compiler flags. Set `kind: UsrEntryKind::Directory` on the collapsed-directory branch and `kind: UsrEntryKind::File` on the single-file branch, and add `aggregate: None, counts_vary: false, sizes_vary: false` to both. Add `UsrEntryKind` to the `use inspectah_core::types::nonrpm::{...}` list at line 11.

Then add this test to the inline `mod tests`:

```rust
#[test]
fn collapse_marks_single_files_as_file_kind_even_when_type_is_other() {
    // One unowned file directly under an otherwise fully owned directory.
    let rpm_owned: HashSet<String> = ["/usr/share/keep.txt".to_string()].into_iter().collect();
    let found = vec![
        ("/usr/share/keep.txt".to_string(), 10u64),
        ("/usr/share/vendor-blob".to_string(), 4096u64),
    ];
    let entries = collapse_usr_entries(found, &rpm_owned);
    let blob = entries
        .iter()
        .find(|e| e.path == "/usr/share/vendor-blob")
        .expect("single unowned file must be reported");
    assert_eq!(blob.kind, UsrEntryKind::File);
    assert_eq!(blob.file_count, 1);
}
```

Adapt the call signature to whatever `collapse_usr_entries` actually takes at HEAD (read it before writing the test; the arguments are the walk results and the RPM-owned path set). If the collapse function is not directly callable from tests, add `#[cfg(test)]`-visible access rather than restructuring it.

- [ ] **Step 5: Fix every construction site the compiler flags**

`usr_entries: Vec::new()` sites need no change. Struct literals of `UnmanagedUsrEntry` do. Known sites: `crates/collect/src/inspectors/nonrpm.rs:4089`, `crates/collect/src/inspectors/nonrpm.rs:4105`. `UnmanagedFileSection` literals gain `usr_bundled: false`: `crates/core/src/types/nonrpm.rs:311`, `crates/core/src/aggregate/merge.rs:1821` and the eight test sites at 2939-3088, `crates/web/src/adapter.rs:1974`, `crates/web/src/aggregate_handlers.rs` (five sites), `crates/pipeline/src/render/unmanaged.rs:102`, `crates/refine/tests/export_contract_test.rs:1113,1175`, `crates/refine/tests/export_parity_test.rs:135,331`. Use `..Default::default()` only where the surrounding code already does; otherwise name the field.

Separately from the struct literals, the schema floor bump breaks every test fixture that hardcodes an older `schema_version` in JSON. Those are listed in § Schema Version Decision: `crates/refine/tests/normalize_test.rs` (ten sites), `crates/cli/src/commands/aggregate.rs:561,567,651,659,709,720`, and `crates/cli/tests/refine_e2e_test.rs:15`. Change each `21` to `23`. They fail as `UnsupportedVersion(21)`, not as compile errors, so the compiler will not point at them.

- [ ] **Step 6: Run the full workspace test suite**

Run: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test --workspace`

Expected: PASS. Any `insta` snapshot that serializes a snapshot will now show `schema_version: 23`; accept those with `cargo insta accept` after reading each diff to confirm the only change is the version number and the new fields. Any remaining `UnsupportedVersion` failure is a hardcoded fixture Step 5 missed; fix it here rather than widening the floor.

- [ ] **Step 7: Lint and format**

Run: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo fmt && cargo clippy --all-targets -- -D clippy::all`

Expected: zero warnings.

- [ ] **Step 8: Commit**

```bash
git add crates/core/src/types/nonrpm.rs crates/core/src/snapshot.rs \
        crates/collect/src/inspectors/nonrpm.rs
git add -u crates/core crates/web crates/pipeline crates/refine
git commit -m "feat(core): record unmanaged /usr entry kind explicitly

The /usr walk collapses to shallowest unowned ancestors, but nothing
recorded whether an entry was a rolled-up directory or a lone file.
Callers had to infer it from file_type != Other, which misreads any
single file the type sniffer cannot categorize. Add an explicit kind
field plus the aggregate prevalence fields the merge will need.

The kind cannot be derived from existing snapshots, so a serde default
would silently mislabel rows. Bump the schema and close the acceptance
window instead: older snapshots get a clean UnsupportedVersion and a
re-scan.

Assisted-by: Claude Code (Opus 5)"
```

---

### Task 2: Aggregate merge preserves `usr_entries`

**Lane:** Tang (core)

**Files:**
- Modify: `crates/core/src/aggregate/merge.rs:1800-1825` (the `merge_unmanaged_file_sections` body)
- Test: `crates/core/src/aggregate/merge.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: `UnmanagedUsrEntry` with `kind`, `aggregate`, `counts_vary`, `sizes_vary` (Task 1).
- Produces: `fn merge_usr_entries(sections: &[Option<UnmanagedFileSection>], total_hosts: usize, hostnames: &[String]) -> Vec<UnmanagedUsrEntry>`, private to the module, called from `merge_unmanaged_file_sections`.

**Design constraints from the spec (do not vary):**
- Union keyed by `path`. Path is the stable identity for beta.3.
- `AggregatePrevalence { count, total, hosts }` attached exactly as other merged families carry it. `total` is `total_hosts`.
- `file_count` and `total_size_bytes` carry the **maximum** across contributing hosts. `counts_vary` / `sizes_vary` are true when contributing hosts disagreed.
- `file_type` and `kind`: take the value from the first contributing host in sorted host order. Deterministic, and the two only disagree in pathological cases.
- Do not use the generic `merge_items` helper. Its non-variant path picks a representative by serializing every candidate and counting payload frequency, which is the wrong identity model for /usr: entries legitimately differ per host in `file_count` and `total_size_bytes`, and this plan carries the maximum plus a varies flag instead of electing a winner. Variant detection is separately out of scope until subtree digests arrive.
- **Do reuse `narrow_non_universal`.** Skipping `merge_items` must not skip the narrowing pass. `narrow_non_universal` (`crates/core/src/aggregate/merge.rs:701`) is the single place aggregate narrowing happens: it sets `include = false` on any item whose `AggregatePrevalence` has `count < total`. Both existing callers run it (`merge.rs:584` after `merge_items`, `merge.rs:691` after `merge_with_variants`). Without it, a /usr path present on 1 of 3 hosts would stay `include = true` and auto-export fleet-wide, breaking settled product decision 3 (design note lines 45-50: 100 percent prevalence auto-includes, partial prevalence lands in a review zone). `merge_usr_entries` runs the same pass on its own output.
- Output sorted by `path` ascending for deterministic merge output. Presentation sorting by size happens at render time.

- [ ] **Step 1: Write the failing tests**

Add to the inline `mod tests` in `crates/core/src/aggregate/merge.rs`:

```rust
fn usr_entry(path: &str, files: u32, size: u64) -> UnmanagedUsrEntry {
    UnmanagedUsrEntry {
        path: path.into(),
        file_count: files,
        total_size_bytes: size,
        file_type: FileType::Other,
        kind: UsrEntryKind::Directory,
        disposition: FindingKind::included(),
        aggregate: None,
        counts_vary: false,
        sizes_vary: false,
    }
}

fn section_with_usr(entries: Vec<UnmanagedUsrEntry>) -> Option<UnmanagedFileSection> {
    Some(UnmanagedFileSection {
        items: Vec::new(),
        usr_entries: entries,
        usr_bundled: false,
        total_size: 0,
        total_count: 0,
    })
}

#[test]
fn merge_unions_usr_entries_by_path_with_prevalence() {
    let hostnames = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let sections = vec![
        section_with_usr(vec![usr_entry("/usr/lib/agent", 10, 100)]),
        section_with_usr(vec![usr_entry("/usr/lib/agent", 12, 250)]),
        section_with_usr(vec![usr_entry("/usr/local/share/x", 3, 30)]),
    ];
    let merged = merge_unmanaged_file_sections(sections, 3, &hostnames).unwrap();

    assert_eq!(merged.usr_entries.len(), 2, "union by path, not concatenation");

    let agent = merged
        .usr_entries
        .iter()
        .find(|e| e.path == "/usr/lib/agent")
        .unwrap();
    let prev = agent.aggregate.as_ref().expect("prevalence must be attached");
    assert_eq!(prev.count, 2);
    assert_eq!(prev.total, 3);
    assert_eq!(prev.hosts, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn merge_carries_max_counts_and_sizes_with_varies_flags() {
    let hostnames = vec!["a".to_string(), "b".to_string()];
    let sections = vec![
        section_with_usr(vec![usr_entry("/usr/lib/agent", 10, 100)]),
        section_with_usr(vec![usr_entry("/usr/lib/agent", 214, 39_845_888)]),
    ];
    let merged = merge_unmanaged_file_sections(sections, 2, &hostnames).unwrap();
    let agent = &merged.usr_entries[0];

    assert_eq!(agent.file_count, 214, "carry the maximum count");
    assert_eq!(agent.total_size_bytes, 39_845_888, "carry the maximum size");
    assert!(agent.counts_vary, "hosts disagreed on count");
    assert!(agent.sizes_vary, "hosts disagreed on size");
}

#[test]
fn merge_leaves_varies_flags_false_when_hosts_agree() {
    let hostnames = vec!["a".to_string(), "b".to_string()];
    let sections = vec![
        section_with_usr(vec![usr_entry("/usr/lib/agent", 10, 100)]),
        section_with_usr(vec![usr_entry("/usr/lib/agent", 10, 100)]),
    ];
    let merged = merge_unmanaged_file_sections(sections, 2, &hostnames).unwrap();
    assert!(!merged.usr_entries[0].counts_vary);
    assert!(!merged.usr_entries[0].sizes_vary);
}

#[test]
fn merge_output_is_sorted_by_path() {
    let hostnames = vec!["a".to_string()];
    let sections = vec![section_with_usr(vec![
        usr_entry("/usr/share/z", 1, 1),
        usr_entry("/usr/lib/a", 1, 1),
    ])];
    let merged = merge_unmanaged_file_sections(sections, 1, &hostnames).unwrap();
    let paths: Vec<&str> = merged.usr_entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, vec!["/usr/lib/a", "/usr/share/z"]);
}

#[test]
fn merge_narrows_partial_prevalence_entries_to_excluded() {
    // Settled product decision 3: 100 percent prevalence auto-includes,
    // partial prevalence lands in review. That behavior comes from
    // narrow_non_universal, which the /usr merge must run just like the
    // generic merge does. Without it a path on one host of three would
    // auto-export to the whole fleet.
    let hostnames = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let sections = vec![
        section_with_usr(vec![
            usr_entry("/usr/lib/everywhere", 10, 100),
            usr_entry("/usr/lib/only-here", 5, 50),
        ]),
        section_with_usr(vec![usr_entry("/usr/lib/everywhere", 10, 100)]),
        section_with_usr(vec![usr_entry("/usr/lib/everywhere", 10, 100)]),
    ];
    let merged = merge_unmanaged_file_sections(sections, 3, &hostnames).unwrap();

    let universal = merged
        .usr_entries
        .iter()
        .find(|e| e.path == "/usr/lib/everywhere")
        .unwrap();
    assert!(
        universal.disposition.is_included(),
        "3 of 3 hosts auto-includes"
    );

    let partial = merged
        .usr_entries
        .iter()
        .find(|e| e.path == "/usr/lib/only-here")
        .unwrap();
    assert!(
        !partial.disposition.is_included(),
        "1 of 3 hosts must land excluded, in the review zone"
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test -p inspectah-core merge_ -- usr`

Expected: FAIL. `merge_unions_usr_entries_by_path_with_prevalence` fails on `assert_eq!(merged.usr_entries.len(), 2)` with `left: 0`, because the merge zeroes the vector.

- [ ] **Step 3: Implement `merge_usr_entries`**

First, give `UnmanagedUsrEntry` the `AggregateMergeable` impl so the shared narrowing pass applies to it. Put it beside the other impls at the top of `crates/core/src/aggregate/merge.rs` (the trait is at line 12; `PackageEntry`'s impl at line 40 is the model):

```rust
impl AggregateMergeable for UnmanagedUsrEntry {
    fn identity_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.path)
    }

    fn aggregate_mut(&mut self) -> &mut Option<AggregatePrevalence> {
        &mut self.aggregate
    }

    fn set_include(&mut self, val: bool) {
        self.disposition = self.disposition.with_include(val);
    }
}
```

`variant_selection_mut` and `content_variant_key` keep their defaults: /usr entries carry no content hash, so they have no variants.

This impl exists only so `narrow_non_universal` can run on the merged output. It does **not** mean `merge_items` is used; see the design constraints above.

Then add above `merge_unmanaged_file_sections` in `crates/core/src/aggregate/merge.rs`:

```rust
/// Merge `usr_entries` across hosts, keyed by path.
///
/// /usr entries carry no content hash, so there is no variant detection
/// here: one merged entry per path, with prevalence. Per-host `file_count`
/// and `total_size_bytes` can legitimately differ for the same path, so the
/// merged entry carries the maximum and flags that hosts disagreed.
fn merge_usr_entries(
    sections: &[Option<UnmanagedFileSection>],
    total_hosts: usize,
    hostnames: &[String],
) -> Vec<UnmanagedUsrEntry> {
    // Preserve first-seen order per path so the representative file_type
    // and kind come from the lowest-indexed contributing host.
    let mut by_path: BTreeMap<String, (UnmanagedUsrEntry, Vec<String>, bool, bool)> =
        BTreeMap::new();

    for (host_idx, section) in sections.iter().enumerate() {
        let Some(section) = section else { continue };
        let hostname = hostnames
            .get(host_idx)
            .cloned()
            .unwrap_or_else(|| format!("host-{host_idx}"));

        for entry in &section.usr_entries {
            match by_path.get_mut(&entry.path) {
                None => {
                    by_path.insert(
                        entry.path.clone(),
                        (entry.clone(), vec![hostname.clone()], false, false),
                    );
                }
                Some((merged, hosts, counts_vary, sizes_vary)) => {
                    if merged.file_count != entry.file_count {
                        *counts_vary = true;
                        merged.file_count = merged.file_count.max(entry.file_count);
                    }
                    if merged.total_size_bytes != entry.total_size_bytes {
                        *sizes_vary = true;
                        merged.total_size_bytes =
                            merged.total_size_bytes.max(entry.total_size_bytes);
                    }
                    if !hosts.contains(&hostname) {
                        hosts.push(hostname.clone());
                    }
                }
            }
        }
    }

    let mut merged: Vec<UnmanagedUsrEntry> = by_path
        .into_values()
        .map(|(mut entry, mut hosts, counts_vary, sizes_vary)| {
            hosts.sort();
            hosts.dedup();
            entry.aggregate = Some(AggregatePrevalence {
                count: hosts.len() as i32,
                total: total_hosts as i32,
                hosts,
                aggregate_count: None,
                aggregate_hosts: None,
            });
            entry.counts_vary = counts_vary;
            entry.sizes_vary = sizes_vary;
            // The aggregate decision is fresh, not a carry-over of whatever
            // one contributing host's disposition happened to be. Start
            // every merged entry included, exactly as merge_items does for
            // its representative, then let the narrowing pass decide.
            entry.disposition = entry.disposition.with_include(true);
            entry
        })
        .collect();

    // The same narrowing every other merged family gets: partial prevalence
    // means include = false, which is what puts the row in a review zone.
    narrow_non_universal(&mut merged);

    merged
}
```

`BTreeMap` gives the path-ascending sort for free. Add `use std::collections::BTreeMap;` if the module does not already import it. Add `UnmanagedUsrEntry`, `AggregatePrevalence`, and `std::borrow::Cow` to the module's imports if absent.

- [ ] **Step 4: Call it from `merge_unmanaged_file_sections`**

`merge_unmanaged_file_sections` currently takes `sections: Vec<Option<UnmanagedFileSection>>` by value and consumes it in `collect_items`. Call `merge_usr_entries(&sections, total_hosts, hostnames)` **before** the `collect_items` call that moves it, bind the result, and replace `usr_entries: Vec::new()` at line 1821 with `usr_entries`. Carry `usr_bundled` forward with `any()` semantics across contributing sections, matching how the aggregate merge propagates other snapshot booleans (see `process-docs/skills/aggregate-vs-single-host-behavioral-split.md` section 2):

```rust
    let usr_entries = merge_usr_entries(&sections, total_hosts, hostnames);
    let usr_bundled = sections.iter().flatten().any(|s| s.usr_bundled);
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test -p inspectah-core merge_`

Expected: PASS, all five new tests plus the existing merge suite.

- [ ] **Step 6: Lint, format, commit**

```bash
PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo fmt && cargo clippy --all-targets -- -D clippy::all
git add crates/core/src/aggregate/merge.rs
git commit -m "feat(core): preserve unmanaged /usr entries through aggregate merge

Aggregate merge zeroed usr_entries, so fleet views and factor saw
nothing at all from the /usr walk. Union by path with the standard
prevalence record. Counts and sizes legitimately differ per host for
the same path, so carry the maximum and flag the disagreement rather
than picking one host's number silently.

The dedicated merge skips the generic merge_items helper, which elects
a representative payload /usr entries do not have. It still runs the
shared narrowing pass, because that is what makes partial prevalence
land in a review zone instead of auto-exporting to the whole fleet.

Assisted-by: Claude Code (Opus 5)"
```

---

### Task 3: Refine identity, projection, and export payload filter

**Lane:** Tang (refine)

**Files:**
- Modify: `crates/refine/src/types.rs:58-118` (the `ItemId` enum)
- Modify: `crates/refine/src/session.rs:1520-1542` (op validation arm), `crates/refine/src/session.rs:1929+` (projection match), `crates/refine/src/session.rs:3171-3225` (extract filter)
- Modify: `crates/refine/src/aggregate/variant_ops.rs:48,210,282` (exhaustive matches on `ItemId`)
- Modify: `crates/tui/src/app.rs:76` (exhaustive match on `ItemId`)
- Test: `crates/refine/tests/usr_decision_test.rs` (create)

**Interfaces:**
- Consumes: `UnmanagedUsrEntry.disposition`, `UnmanagedUsrEntry.path` (Task 1).
- Produces: `ItemId::UnmanagedUsr { path: String }`. Consumed by Tasks 9 and 10.
- Produces: `extract_payload_dirs_from_tarball` extends its filter to include /usr paths by prefix. Consumed by Task 5's bundling and Task 6's COPY lines.

**Why a prefix match, not set membership:** the existing filter does `included_unmanaged.contains(file_rel)`, an exact match, because Tier 2 entries are individual files. A /usr entry can be a collapsed directory, so every archive member beneath `unmanaged/usr/lib/custom-agent/` must extract when that one entry is included. Exact matching would silently drop the whole subtree and produce an export whose COPY line points at nothing.

- [ ] **Step 1: Write the failing tests**

Create `crates/refine/tests/usr_decision_test.rs`:

```rust
//! /usr entries are ordinary Actionable findings: a SetInclude on one
//! must reach the projected snapshot, and the export payload filter must
//! carry a collapsed directory's whole subtree.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::nonrpm::{
    FileType, UnmanagedFileSection, UnmanagedUsrEntry, UsrEntryKind,
};
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, RefinementOp};

fn snapshot_with_usr() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.unmanaged_files = Some(UnmanagedFileSection {
        items: Vec::new(),
        usr_entries: vec![UnmanagedUsrEntry {
            path: "/usr/lib/custom-agent".into(),
            file_count: 214,
            total_size_bytes: 39_845_888,
            file_type: FileType::Other,
            kind: UsrEntryKind::Directory,
            disposition: FindingKind::included(),
            aggregate: None,
            counts_vary: false,
            sizes_vary: false,
        }],
        usr_bundled: true,
        total_size: 0,
        total_count: 0,
    });
    snap
}

#[test]
fn set_include_false_on_usr_entry_reaches_the_projection() {
    let mut session = RefineSession::new(snapshot_with_usr());
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::UnmanagedUsr {
                path: "/usr/lib/custom-agent".into(),
            },
            include: false,
        })
        .expect("SetInclude on a /usr entry must be accepted");

    let projected = session.snapshot_projected();
    let entry = &projected.unmanaged_files.as_ref().unwrap().usr_entries[0];
    assert!(
        !entry.disposition.is_included(),
        "the export renders from the projection, so the decision must land here"
    );
}

#[test]
fn set_include_on_an_unknown_usr_path_is_rejected() {
    let mut session = RefineSession::new(snapshot_with_usr());
    let result = session.apply(RefinementOp::SetInclude {
        item_id: ItemId::UnmanagedUsr {
            path: "/usr/lib/does-not-exist".into(),
        },
        include: false,
    });
    assert!(result.is_err(), "unknown targets must not apply silently");
}
```

**The extraction test. This one is the point of Step 6 and must be red before Step 6 is written.** The projection tests above pass with the existing exact-match filter; only this one fails, and it is the failure mode that ships a Containerfile whose COPY line points at nothing. `extract_payload_dirs_from_tarball` is private, so drive it through the public export path.

Add to the same file:

```rust
#[test]
fn exporting_an_included_collapsed_directory_carries_its_whole_subtree() {
    // Two sibling paths where one is a strict string prefix of the other.
    // Only /usr/lib/agent is a snapshot entry and only it is included, so
    // its whole subtree must land in the export and the sibling must not
    // appear at all. A bare starts_with() match passes the first assertion
    // and fails the second; exact-set membership fails the first.
    let mut snap = InspectionSnapshot::new();
    snap.unmanaged_files = Some(UnmanagedFileSection {
        items: Vec::new(),
        usr_entries: vec![UnmanagedUsrEntry {
            path: "/usr/lib/agent".into(),
            file_count: 2,
            total_size_bytes: 20,
            file_type: FileType::Other,
            kind: UsrEntryKind::Directory,
            disposition: FindingKind::included(),
            aggregate: None,
            counts_vary: false,
            sizes_vary: false,
        }],
        usr_bundled: true,
        total_size: 0,
        total_count: 0,
    });

    // Archive members, all four under unmanaged/:
    //   usr/lib/agent/bin/run          -> included (subtree member)
    //   usr/lib/agent/lib/data.so      -> included (subtree member)
    //   usr/lib/agent-backup/bin/run   -> excluded (sibling, not a subtree)
    //   opt/other/thing                -> excluded (unrelated Tier 2 path)
    let tarball = tarball_with_unmanaged_members(&[
        "usr/lib/agent/bin/run",
        "usr/lib/agent/lib/data.so",
        "usr/lib/agent-backup/bin/run",
        "opt/other/thing",
    ]);
    let out = tempfile::tempdir().unwrap();

    export_with_payload(&tarball, &snap, out.path()).expect("export must succeed");

    let unmanaged = out.path().join("unmanaged");
    assert!(
        unmanaged.join("usr/lib/agent/bin/run").exists(),
        "an included collapsed directory carries its whole subtree"
    );
    assert!(
        unmanaged.join("usr/lib/agent/lib/data.so").exists(),
        "every member beneath the entry extracts, not just the first"
    );
    assert!(
        !unmanaged.join("usr/lib/agent-backup/bin/run").exists(),
        "a sibling sharing a string prefix is a different entry and must not ride along"
    );
    assert!(
        !unmanaged.join("opt/other/thing").exists(),
        "unrelated Tier 2 content is unaffected"
    );
}

#[test]
fn excluding_a_usr_entry_keeps_its_subtree_out_of_the_export() {
    let mut snap = snapshot_with_usr();
    let mut session = RefineSession::new(snap.clone());
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::UnmanagedUsr {
                path: "/usr/lib/custom-agent".into(),
            },
            include: false,
        })
        .unwrap();
    snap = session.snapshot_projected();

    let tarball = tarball_with_unmanaged_members(&["usr/lib/custom-agent/bin/run"]);
    let out = tempfile::tempdir().unwrap();
    export_with_payload(&tarball, &snap, out.path()).unwrap();

    assert!(
        !out.path().join("unmanaged/usr/lib/custom-agent/bin/run").exists(),
        "the toggle's visible effect: excluded content does not ship"
    );
}
```

Adapt `RefineSession::new` and `apply` to the real constructor and method names at HEAD; read `crates/refine/src/session.rs` before writing. `tarball_with_unmanaged_members` and `export_with_payload` are stand-in names: `crates/refine/tests/export_contract_test.rs` already builds source tarballs and drives export end to end, so lift its helpers rather than writing new ones. If they are not shareable across test binaries, copy the minimum into this file; do not restructure the export API to make it testable.

- [ ] **Step 2: Run to verify failure**

Run: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test -p inspectah-refine --test usr_decision_test`

Expected: FAIL to compile with "no variant named `UnmanagedUsr` found for enum `ItemId`". After Steps 3-5 land the identity and projection, re-run before writing Step 6: `exporting_an_included_collapsed_directory_carries_its_whole_subtree` must still fail on the missing `usr/lib/agent/bin/run`, proving the exact-match filter is the bug Step 6 fixes.

- [ ] **Step 3: Add the `ItemId` variant**

In `crates/refine/src/types.rs`, immediately after the `UnmanagedFile` variant (find it near the Software-section variants; the enum starts at line 58):

```rust
    /// A collapsed directory or single file under /usr that no RPM owns.
    /// Identity is the path, matching the aggregate merge key.
    UnmanagedUsr {
        path: String,
    },
```

- [ ] **Step 4: Add the validation arm**

In `crates/refine/src/session.rs`, the `SetInclude` validation match at line ~1520 lists item kinds that need no extra validation. `UnmanagedUsr` needs target validation, so give it its own arm rather than joining the pass-through list:

```rust
                    ItemId::UnmanagedUsr { path } => {
                        let known = self
                            .original
                            .unmanaged_files
                            .as_ref()
                            .is_some_and(|s| s.usr_entries.iter().any(|e| e.path == *path));
                        if !known {
                            return Err(RefineError::UnknownTarget(path.clone()));
                        }
                    }
```

- [ ] **Step 5: Add the projection arm**

In `project_snapshot` (`crates/refine/src/session.rs:1911`), inside the `RefinementOp::SetInclude` match on `item_id`, add:

```rust
                            ItemId::UnmanagedUsr { path } => {
                                if let Some(ref mut ufs) = snap.unmanaged_files
                                    && let Some(entry) =
                                        ufs.usr_entries.iter_mut().find(|e| e.path == *path)
                                {
                                    entry.disposition =
                                        entry.disposition.with_include(*include);
                                }
                            }
```

Use `with_include`, not `FindingKind::from_bool`. `from_bool` overwrites a non-actionable disposition wholesale, and the export renders from this projection. See `process-docs/skills/web-disposition-contract.md` section 3.

- [ ] **Step 6: Extend the export payload filter**

In `extract_payload_dirs_from_tarball` (`crates/refine/src/session.rs:3161`), after the existing `included_unmanaged` set, add a prefix list and fold it into both the early return and the per-entry decision:

```rust
    // Included /usr entries. A collapsed directory entry owns its whole
    // subtree in the archive, so these match by prefix rather than by
    // exact path the way single-file Tier 2 entries do.
    let included_usr_prefixes: Vec<String> = snap
        .unmanaged_files
        .as_ref()
        .map(|s| {
            s.usr_entries
                .iter()
                .filter(|e| e.disposition.is_included())
                .map(|e| e.path.trim_start_matches('/').to_string())
                .collect()
        })
        .unwrap_or_default();
```

Change the early return to:

```rust
    if included_unmanaged.is_empty()
        && included_repoless.is_empty()
        && included_usr_prefixes.is_empty()
    {
        return Ok(());
    }
```

Change the `should_extract` unmanaged arm to:

```rust
        let should_extract = if let Some(file_rel) = rel.strip_prefix("unmanaged/") {
            !file_rel.is_empty()
                && (included_unmanaged.contains(file_rel)
                    || included_usr_prefixes.iter().any(|p| {
                        file_rel == p
                            || file_rel.strip_prefix(p).is_some_and(|r| r.starts_with('/'))
                    }))
        } else if let Some(filename) = rel.strip_prefix("repoless-packages/") {
```

The `starts_with('/')` guard is load-bearing: a bare `starts_with(p)` would extract `usr/lib/custom-agent-backup/` when only `usr/lib/custom-agent` was included. That is exactly the sibling assertion in `exporting_an_included_collapsed_directory_carries_its_whole_subtree`, so both halves of that test must pass, not just the subtree half.

- [ ] **Step 7: Fix the other exhaustive matches**

The compiler will flag `crates/refine/src/aggregate/variant_ops.rs:48,210,282` and `crates/tui/src/app.rs:76`. For `variant_ops.rs:48` (`identity path extraction`) return `Some(path.as_str())`. For 210 and 282 (variant lookup and application), /usr entries have no content variants, so join whatever arm returns "no variants" for hash-free item kinds. For `crates/tui/src/app.rs:76` return `path.clone()`, matching the `UnmanagedFile` arm directly above it. The TUI does not render the section in beta.3 (design note section 7 defers TUI parity), so this arm exists only to keep the match exhaustive.

- [ ] **Step 8: Run tests, lint, format**

Run: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test --workspace && cargo fmt --check && cargo clippy --all-targets -- -D clippy::all`

Expected: PASS with zero warnings.

- [ ] **Step 9: Commit**

```bash
git add crates/refine/src/types.rs crates/refine/src/session.rs \
        crates/refine/src/aggregate/variant_ops.rs crates/tui/src/app.rs \
        crates/refine/tests/usr_decision_test.rs
git commit -m "feat(refine): give unmanaged /usr entries a decision identity

The projection already carried usr_entries through by cloning the
original snapshot, but no ItemId could target one, so a user had no way
to exclude an entry and the export had nothing to read. Add the variant,
the validation, and the projection arm.

The export payload filter matched Tier 2 paths exactly, which is right
for single files and wrong for a collapsed directory: only the directory
node itself would have extracted, leaving the COPY line pointing at an
empty path. Match /usr entries by path prefix with a separator guard.

Assisted-by: Claude Code (Opus 5)"
```

---

### Task 4: Register the section

**Lane:** Tang (pipeline)

**Files:**
- Modify: `crates/pipeline/src/section_group.rs:172-188` (the `Self::Software` arm)
- Test: `crates/pipeline/src/section_group.rs` (inline `mod tests`)

**Interfaces:**
- Produces: `SectionMeta { id: "unmanaged_usr", label: "Unmanaged /usr", is_triage: true }` as the fourth member of `SectionGroup::Software`, positioned after `unmanaged_files`. Consumed by Tasks 9, 10, 11 (sidebar nav, group batch routing, `GroupMetaDto`).

- [ ] **Step 1: Write the failing test**

Add to the inline `mod tests` in `crates/pipeline/src/section_group.rs`:

```rust
#[test]
fn software_group_lists_unmanaged_usr_after_unmanaged_files() {
    let ids: Vec<&str> = SectionGroup::Software
        .sections()
        .iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(
        ids,
        vec![
            "non_rpm_software",
            "language_packages",
            "unmanaged_files",
            "unmanaged_usr"
        ]
    );
}

#[test]
fn unmanaged_usr_maps_back_to_the_software_group() {
    assert_eq!(
        SectionGroup::for_section("unmanaged_usr"),
        SectionGroup::Software
    );
}

#[test]
fn unmanaged_usr_is_a_triage_section() {
    let meta = SectionGroup::Software
        .sections()
        .iter()
        .find(|s| s.id == "unmanaged_usr")
        .expect("section must be registered");
    assert!(meta.is_triage, "it carries include/exclude decisions");
    assert_eq!(meta.label, "Unmanaged /usr");
}
```

Adapt `.sections()` to whatever the accessor is actually called at HEAD.

- [ ] **Step 2: Run to verify failure**

Run: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test -p inspectah-pipeline unmanaged_usr`

Expected: FAIL with a length mismatch on the id vector.

- [ ] **Step 3: Register the section**

In `crates/pipeline/src/section_group.rs`, append to the `Self::Software` slice, after the `unmanaged_files` entry:

```rust
                SectionMeta {
                    id: "unmanaged_usr",
                    label: "Unmanaged /usr",
                    is_triage: true,
                },
```

If `for_section` is a hand-written match rather than a scan over the group tables, add the `"unmanaged_usr" => SectionGroup::Software` arm too.

- [ ] **Step 4: Run to verify pass, then commit**

Run: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test -p inspectah-pipeline && cargo fmt --check && cargo clippy --all-targets -- -D clippy::all`

Expected: PASS, zero warnings.

```bash
git add crates/pipeline/src/section_group.rs
git commit -m "feat(pipeline): register the Unmanaged /usr section

Fourth sibling in the Software group, after Unmanaged Files. The two
mean different things: Unmanaged Files is bundleable content the user
may choose to copy, while /usr findings say this estate is not
image-clean. Group nav, batch routing, and the sidebar all read this
table, so registration has to land before the web surfaces.

Assisted-by: Claude Code (Opus 5)"
```

---

### Task 5: Scan-time /usr bundling and the split size prompt

**Lane:** Tang (cli)

**Files:**
- Modify: `crates/cli/src/commands/scan.rs:117-171` (`ScanArgs`: new `no_bundle_usr` flag), `crates/cli/src/commands/scan.rs:741-767` (the bundling prompt), `crates/cli/src/commands/scan.rs:826-832` (the bundle call), `crates/cli/src/commands/scan.rs:1099-1141` (`bundle_unmanaged_files`), `crates/cli/src/commands/scan.rs:1242-1256` (`base_args()` test helper)
- Test: `crates/cli/src/commands/scan.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: `UnmanagedFileSection.usr_entries`, `UnmanagedFileSection.usr_bundled`, `UsrEntryKind` (Task 1).
- Produces: `fn bundle_usr_entries(entries: &[UnmanagedUsrEntry], render_dir: &Path) -> Result<u64>` returning the bytes actually copied. Sets `usr_bundled` on the section.
- Produces: `ScanArgs.no_bundle_usr: bool` (`--no-bundle-usr`), the programmatic decline for the /usr bundling question only. See "Programmatic override" below.
- Produces: `fn resolve_usr_bundle(no_bundle_usr: bool, assume_yes: bool) -> Option<bool>`, the pure decision the prompt block consults before it ever touches stdin.

**Decided (Mark, 2026-08-16): /usr content sourcing.** /usr content participates in scan-time Tier 2 bundling, shown as its own separate line in the existing size prompt (e.g. `unmanaged /usr: N files, X MB` alongside the existing unmanaged-files line), separately declinable from the other unmanaged content. Declining keeps the findings and falls back to the per-path `MISSING FROM BUILD CONTEXT` warning behavior already specified for the Containerfile export (Task 6) and the audit report (Task 7). The /usr bundling answer must also be overridable for programmatic (non-interactive) use, not only via the interactive prompt.

**Programmatic override.** Today `-y`/`--yes` is a blanket yes: it suppresses every scan prompt, including the new /usr line, and there is no existing way to force a decline short of typing `n` at an interactive prompt. That is asymmetric, and Mark's requirement needs both directions covered for /usr specifically. `-y`/`--yes` continues to cover the accept direction, unchanged. For the decline direction, add `--no-bundle-usr` (field `no_bundle_usr: bool` on `ScanArgs`, plain `#[arg(long)]`, no short form), extending the repo's existing negative-boolean-flag convention: `no_redaction` (`crates/cli/src/commands/scan.rs:134-136`) is the precedent, a `no_`-prefixed flag that turns off a default-on behavior. This name is coined at binding time and is implementation-adjustable if a better one turns up during Task 5 execution. `--no-bundle-usr` takes precedence over `-y`/`--yes` for the /usr decision: if both are passed, /usr does not bundle. Neither flag touches the Tier 2 prompt's own decision.

**Non-interactive default (no flag, no TTY, no `-y`).** Matches the existing Tier 2 behavior exactly: `prompt_yes_default` reads a closed or empty stdin as yes, so /usr bundles by default when nothing overrides it. This is the existing default carried forward onto the new /usr line, not a new default being introduced.

**Trigger is `usr_entries`, not `items`.** The current gate at `crates/cli/src/commands/scan.rs:741-767` only reaches the prompt block when `!unmanaged.items.is_empty()`, i.e. when Tier 2 findings exist; a host whose only unmanaged content is under /usr would get no prompt at all under that gate alone. Step 4's block below keeps the two questions on independent conditions: the Tier 2 question stays gated on `!unmanaged.items.is_empty()`, the /usr question is gated separately on `usr_included_bytes > 0`. A /usr-only host must still be asked. This independence is load-bearing, not incidental.

**Rejected alternatives:**
- **Bundle unconditionally, like Tier 2, no prompt.** Simplest, and the refine export filter from Task 3 already handles the subtree. Rejected because /usr entries default to included, so every `--include-unmanaged` scan would silently grow the tarball by the host's entire unmanaged-/usr footprint, sometimes multiple gigabytes, with no warning and no chance to decline before the fact.
- **Never bundle; path-only COPY lines with warnings.** Rejected because the exported Containerfile would never build for /usr content, breaking the contract every other Actionable family holds and the design note's own tenet that export proceeds on whatever the toggle states.

**Bug this task must also fix:** the current prompt sets `snapshot.unmanaged_files = None` when the operator declines. That clears the whole section, so declining Tier 2 bundling would erase the /usr *findings* along with the Tier 2 bytes. Detection and bundling are different questions. The decline path must clear `items` and reset `total_size`/`total_count`, not null the section.

- [ ] **Step 1: Write the failing tests**

Add to the inline `mod tests` in `crates/cli/src/commands/scan.rs`:

```rust
#[test]
fn declining_tier2_bundling_keeps_usr_findings() {
    let mut section = UnmanagedFileSection {
        items: vec![UnmanagedFile {
            path: "/opt/app/server".into(),
            size: 100,
            ..Default::default()
        }],
        usr_entries: vec![UnmanagedUsrEntry {
            path: "/usr/lib/custom-agent".into(),
            file_count: 214,
            total_size_bytes: 39_845_888,
            file_type: FileType::Other,
            kind: UsrEntryKind::Directory,
            disposition: FindingKind::included(),
            aggregate: None,
            counts_vary: false,
            sizes_vary: false,
        }],
        usr_bundled: false,
        total_size: 100,
        total_count: 1,
    };
    decline_tier2_bundling(&mut section);
    assert!(section.items.is_empty(), "Tier 2 bytes are dropped");
    assert_eq!(section.total_size, 0);
    assert_eq!(section.total_count, 0);
    assert_eq!(
        section.usr_entries.len(),
        1,
        "declining to carry bytes is not declining to report the finding"
    );
}

#[test]
fn bundle_usr_entries_copies_a_directory_subtree() {
    let src = tempfile::tempdir().unwrap();
    let agent = src.path().join("lib/custom-agent");
    std::fs::create_dir_all(agent.join("nested")).unwrap();
    std::fs::write(agent.join("bin"), b"0123456789").unwrap();
    std::fs::write(agent.join("nested/data"), b"abcde").unwrap();

    let render = tempfile::tempdir().unwrap();
    let entry = UnmanagedUsrEntry {
        path: agent.to_string_lossy().to_string(),
        file_count: 2,
        total_size_bytes: 15,
        file_type: FileType::Other,
        kind: UsrEntryKind::Directory,
        disposition: FindingKind::included(),
        aggregate: None,
        counts_vary: false,
        sizes_vary: false,
    };

    let copied = bundle_usr_entries(&[entry], render.path()).unwrap();
    assert_eq!(copied, 15, "returns the bytes actually written");

    let rel = agent.to_string_lossy().trim_start_matches('/').to_string();
    assert!(render.path().join("unmanaged").join(&rel).join("bin").exists());
    assert!(
        render
            .path()
            .join("unmanaged")
            .join(&rel)
            .join("nested/data")
            .exists(),
        "nested content must travel; a collapsed entry owns its whole subtree"
    );
}

#[test]
fn bundle_usr_entries_skips_excluded_entries() {
    let src = tempfile::tempdir().unwrap();
    let f = src.path().join("blob");
    std::fs::write(&f, b"xxx").unwrap();
    let render = tempfile::tempdir().unwrap();
    let entry = UnmanagedUsrEntry {
        path: f.to_string_lossy().to_string(),
        file_count: 1,
        total_size_bytes: 3,
        file_type: FileType::Other,
        kind: UsrEntryKind::File,
        disposition: FindingKind::excluded(),
        aggregate: None,
        counts_vary: false,
        sizes_vary: false,
    };
    let copied = bundle_usr_entries(&[entry], render.path()).unwrap();
    assert_eq!(copied, 0);
    assert!(!render.path().join("unmanaged").exists());
}

#[test]
fn no_bundle_usr_flag_declines_without_a_prompt() {
    // The programmatic decline direction. Must not depend on stdin at all.
    assert_eq!(resolve_usr_bundle(true, false), Some(false));
}

#[test]
fn no_bundle_usr_flag_overrides_assume_yes() {
    // Both directions of programmatic control exist for /usr specifically;
    // an explicit decline is more specific than a blanket -y and wins.
    assert_eq!(resolve_usr_bundle(true, true), Some(false));
}

#[test]
fn assume_yes_bundles_usr_without_a_prompt() {
    // The programmatic accept direction; unchanged from -y's existing
    // blanket-yes behavior, confirmed here to also cover /usr.
    assert_eq!(resolve_usr_bundle(false, true), Some(true));
}

#[test]
fn neither_flag_defers_to_the_interactive_prompt() {
    // With no override, resolve_usr_bundle hands the decision to
    // prompt_yes_default, which reads a closed or empty stdin as yes --
    // the same non-interactive default Tier 2 bundling already has. No
    // new default is being introduced for /usr.
    assert_eq!(resolve_usr_bundle(false, false), None);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test -p inspectah-cli usr`

Expected: FAIL to compile with "cannot find function `bundle_usr_entries`", "cannot find function `decline_tier2_bundling`", and "cannot find function `resolve_usr_bundle`".

- [ ] **Step 3: Implement the two functions**

Add near `bundle_unmanaged_files` in `crates/cli/src/commands/scan.rs`:

```rust
/// Drop Tier 2 payload bytes while keeping every finding the scan made.
///
/// Declining to carry /opt, /srv, and /usr/local content into the tarball
/// is a size decision. It is not a decision to stop reporting what was
/// found, and it says nothing at all about /usr.
fn decline_tier2_bundling(section: &mut UnmanagedFileSection) {
    section.items.clear();
    section.total_size = 0;
    section.total_count = 0;
}

/// Copy included /usr entries into the render directory for tarball
/// inclusion. Returns the total bytes written.
///
/// A `Directory` entry is a collapse to the shallowest unowned ancestor,
/// so the whole subtree travels. Symlinks are recreated rather than
/// followed, matching `bundle_unmanaged_files` and for the same reason:
/// following them exfiltrates content from outside the scan roots.
fn bundle_usr_entries(entries: &[UnmanagedUsrEntry], render_dir: &Path) -> Result<u64> {
    let mut copied = 0u64;
    for entry in entries {
        if !entry.disposition.is_included() {
            continue;
        }
        let rel_path = entry.path.trim_start_matches('/');
        let dest = render_dir.join("unmanaged").join(rel_path);
        match entry.kind {
            UsrEntryKind::File => {
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent).with_context(|| {
                        format!("failed to create dir for {}", dest.display())
                    })?;
                }
                copied += copy_one(Path::new(&entry.path), &dest)?;
            }
            UsrEntryKind::Directory => {
                copied += copy_subtree(Path::new(&entry.path), &dest)?;
            }
        }
    }
    Ok(copied)
}

/// Copy a single filesystem entry, recreating symlinks rather than
/// dereferencing them. Returns bytes written (0 for a symlink).
fn copy_one(src: &Path, dest: &Path) -> Result<u64> {
    let meta = std::fs::symlink_metadata(src)
        .with_context(|| format!("failed to stat {}", src.display()))?;
    if meta.file_type().is_symlink() {
        let target = std::fs::read_link(src)
            .with_context(|| format!("failed to read symlink {}", src.display()))?;
        std::os::unix::fs::symlink(&target, dest)
            .with_context(|| format!("failed to recreate symlink {}", src.display()))?;
        Ok(0)
    } else if meta.file_type().is_file() {
        std::fs::copy(src, dest)
            .with_context(|| format!("failed to copy {} to tarball", src.display()))
    } else {
        // Sockets, devices, and FIFOs carry nothing useful into an image.
        Ok(0)
    }
}

/// Recursively copy a directory subtree, recreating symlinks in place.
fn copy_subtree(src: &Path, dest: &Path) -> Result<u64> {
    std::fs::create_dir_all(dest)
        .with_context(|| format!("failed to create dir {}", dest.display()))?;
    let mut copied = 0u64;
    for child in std::fs::read_dir(src)
        .with_context(|| format!("failed to read dir {}", src.display()))?
    {
        let child = child.with_context(|| format!("failed to read dir {}", src.display()))?;
        let child_src = child.path();
        let child_dest = dest.join(child.file_name());
        let meta = std::fs::symlink_metadata(&child_src)
            .with_context(|| format!("failed to stat {}", child_src.display()))?;
        if meta.file_type().is_dir() {
            copied += copy_subtree(&child_src, &child_dest)?;
        } else {
            copied += copy_one(&child_src, &child_dest)?;
        }
    }
    Ok(copied)
}
```

`copy_subtree` recurses only into real directories because `symlink_metadata` does not follow links, so a symlinked directory is recreated as a link and never descended. That is what keeps the walk inside /usr.

Add `use inspectah_core::types::nonrpm::{UnmanagedUsrEntry, UsrEntryKind};` to the imports if absent.

- [ ] **Step 4: Add the override flag and rework the prompt**

First, add the flag to `ScanArgs` in `crates/cli/src/commands/scan.rs`, immediately after `include_unmanaged` (line 157), following the `no_redaction` precedent at lines 134-136 -- a plain `#[arg(long)]` negative boolean, no short form:

```rust
    /// Skip bundling included /usr content into the tarball even though
    /// it would otherwise be included. Overrides -y/--yes for this
    /// decision only; the export falls back to a per-path
    /// MISSING FROM BUILD CONTEXT warning for declined /usr content.
    #[arg(long)]
    pub no_bundle_usr: bool,
```

Add `no_bundle_usr: false` to the `base_args()` test helper at `crates/cli/src/commands/scan.rs:1242-1256`. Every other `ScanArgs` test literal in this file is built with `..base_args()`, so that helper is the only construction site the compiler will flag.

Then replace the prompt block at `crates/cli/src/commands/scan.rs:741-767`. The current block is gated on `!unmanaged.items.is_empty()` and nulls the section on decline. The new block asks up to two questions and never nulls:

```rust
    // Prompt for payload bundling if --include-unmanaged was used.
    // Skip when --inspect-only: metadata is kept for the JSON snapshot,
    // but bundling and the size prompt are irrelevant without a tarball.
    if !args.inspect_only
        && args.include_unmanaged
        && let Some(ref mut unmanaged) = snapshot.unmanaged_files
    {
        if !unmanaged.items.is_empty() && !assume_yes {
            let size_display = format_size(unmanaged.total_size);
            let roots = describe_scan_roots(&unmanaged.items);
            eprintln!(
                "Found {} unmanaged files in {} ({} total)",
                unmanaged.total_count, roots, size_display,
            );
            if !prompt_yes_default("Include in tarball?") {
                decline_tier2_bundling(unmanaged);
            }
        }

        let usr_included_bytes: u64 = unmanaged
            .usr_entries
            .iter()
            .filter(|e| e.disposition.is_included())
            .map(|e| e.total_size_bytes)
            .sum();
        if usr_included_bytes > 0 {
            unmanaged.usr_bundled = match resolve_usr_bundle(args.no_bundle_usr, assume_yes) {
                Some(decision) => decision,
                None => {
                    eprintln!(
                        "Found {} unmanaged /usr entries ({} total). /usr ships from \
                         the image and is read-only at runtime, so this content will \
                         not survive a rebuild unless it travels with the image.",
                        unmanaged.usr_entries.len(),
                        format_size(usr_included_bytes),
                    );
                    prompt_yes_default("Include /usr content in tarball?")
                }
            };
        }
    }
```

Extract the shared read-and-answer logic into `fn prompt_yes_default(question: &str) -> bool` next to the other helpers, since the block now asks twice:

```rust
/// Ask a yes-default question on stderr. Any answer beginning with "n"
/// is a no; everything else, including empty input and a closed stdin,
/// is a yes.
fn prompt_yes_default(question: &str) -> bool {
    use std::io::Write;
    eprint!("{question} [Y/n] ");
    std::io::stderr().flush().ok();
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();
    let input = input.trim().to_lowercase();
    input != "n" && input != "no"
}
```

Add `resolve_usr_bundle` beside it. This is the programmatic-override binding: it is a pure function so the flag/no-flag/`-y` matrix is testable without touching stdin, and `None` is the single fall-through case that reaches the interactive prompt:

```rust
/// Decide the /usr bundling answer without any I/O. `--no-bundle-usr` is
/// the programmatic decline and takes precedence over `-y`/`--yes`;
/// `-y`/`--yes` alone is the programmatic accept. `None` means neither
/// flag applies and the interactive prompt must decide -- at which point
/// `prompt_yes_default`'s existing closed-stdin-is-yes default applies,
/// unchanged from Tier 2.
fn resolve_usr_bundle(no_bundle_usr: bool, assume_yes: bool) -> Option<bool> {
    if no_bundle_usr {
        Some(false)
    } else if assume_yes {
        Some(true)
    } else {
        None
    }
}
```

**Acceptance criteria (programmatic override), each covered by a Step 1 test:** `--no-bundle-usr` yields `usr_bundled = false` with no stdin read, including when `-y`/`--yes` is also passed. `-y`/`--yes` without `--no-bundle-usr` yields `usr_bundled = true` with no stdin read. With neither flag, the decision falls through to the interactive prompt, which reads a closed or empty stdin as yes -- the same non-interactive default Tier 2 bundling already has today; no new default is introduced for /usr.

- [ ] **Step 5: Call the bundler**

At `crates/cli/src/commands/scan.rs:826-832`, after the existing `bundle_unmanaged_files` call:

```rust
    if let Some(ref unmanaged) = snapshot.unmanaged_files
        && unmanaged.usr_bundled
    {
        bundle_usr_entries(&unmanaged.usr_entries, render_dir.path())
            .context("failed to bundle unmanaged /usr content")?;
    }
```

The `render_all` call above it already ran, so the Containerfile in the tarball was rendered before `usr_bundled` was consulted. Confirm `usr_bundled` is set during the prompt block (step 4), which runs earlier at line ~738, well before `render_all` at ~825. It is. The renderer in Task 6 reads `usr_bundled` and will see the right value.

- [ ] **Step 6: Run tests, lint, format, commit**

Run: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test -p inspectah-cli && cargo fmt --check && cargo clippy --all-targets -- -D clippy::all`

Expected: PASS, zero warnings.

```bash
git add crates/cli/src/commands/scan.rs
git commit -m "feat(cli): carry included /usr content into the scan tarball

Included /usr entries now bundle under unmanaged/usr/ so the exported
COPY lines have bytes behind them. Collapsed directory entries copy
their whole subtree; symlinks are recreated rather than followed, so
the walk cannot reach outside /usr.

/usr gets its own prompt because the footprint can dwarf Tier 2 and the
operator should see it before the tarball does. Declining sets
usr_bundled = false and the export says so per path.

--no-bundle-usr adds a programmatic decline for /usr specifically: -y
already covered the accept direction, but there was no way to script a
decline short of typing n at the interactive prompt. Neither flag
changes the non-interactive default -- with neither set, the /usr
question falls through to the same stdin read Tier 2 already has, so a
closed or empty stdin still bundles.

Declining Tier 2 bundling used to null the whole unmanaged section,
which would have thrown away the /usr findings along with the /opt
bytes. Declining to carry content is not declining to report it.

Assisted-by: Claude Code (Opus 5)"
```

---

### Task 6: Containerfile `=== Unmanaged /usr ===` block

**Lane:** Tang (pipeline)

**Files:**
- Create: `crates/pipeline/src/render/unmanaged_usr.rs`
- Modify: `crates/pipeline/src/render/mod.rs` (module declaration)
- Modify: `crates/pipeline/src/render/containerfile.rs:1218` (block insertion)
- Test: inline `mod tests` in the new file; `crates/refine/tests/export_contract_test.rs` (export preview pin)

**Interfaces:**
- Consumes: `UnmanagedUsrEntry`, `UsrEntryKind`, `UnmanagedFileSection.usr_bundled` (Task 1).
- Produces: `pub fn unmanaged_usr_lines(snap: &InspectionSnapshot) -> Vec<String>`. Called once from `containerfile.rs`.

**Output contract (this is the spec for the task):**

```
# === Unmanaged /usr ===
# In image mode, /usr ships from the container image and stays read-only
# at runtime. This content belongs to no RPM package, so nothing updates
# it and keeping it current is your responsibility. Building an RPM that
# owns it is the durable fix.
# 6 entries included, 212 MB copied into the image.
COPY unmanaged/usr/lib/custom-agent/ /usr/lib/custom-agent/
COPY unmanaged/usr/share/vendor-blob /usr/share/vendor-blob
```

When `usr_bundled` is false, each COPY line is preceded by a warning and the totals line names the problem:

```
# === Unmanaged /usr ===
# ...warning block as above...
# 6 entries included, 212 MB. Content is NOT in this archive: the scan
# was run without bundling /usr. Stage these paths in the build context
# before building.
# MISSING FROM BUILD CONTEXT: /usr/lib/custom-agent
COPY unmanaged/usr/lib/custom-agent/ /usr/lib/custom-agent/
```

The COPY lines still render either way. Excluding an entry removes its COPY line; that is the toggle's visible effect and the design note requires it.

- [ ] **Step 1: Write the failing tests**

Create `crates/pipeline/src/render/unmanaged_usr.rs` containing only the test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::FindingKind;
    use inspectah_core::types::nonrpm::{
        FileType, UnmanagedFileSection, UnmanagedUsrEntry, UsrEntryKind,
    };

    fn snap_with(entries: Vec<UnmanagedUsrEntry>, bundled: bool) -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::default();
        snap.unmanaged_files = Some(UnmanagedFileSection {
            items: Vec::new(),
            usr_entries: entries,
            usr_bundled: bundled,
            total_size: 0,
            total_count: 0,
        });
        snap
    }

    fn dir_entry(path: &str, size: u64, included: bool) -> UnmanagedUsrEntry {
        UnmanagedUsrEntry {
            path: path.into(),
            file_count: 214,
            total_size_bytes: size,
            file_type: FileType::Other,
            kind: UsrEntryKind::Directory,
            disposition: if included {
                FindingKind::included()
            } else {
                FindingKind::excluded()
            },
            aggregate: None,
            counts_vary: false,
            sizes_vary: false,
        }
    }

    fn file_entry(path: &str, size: u64) -> UnmanagedUsrEntry {
        UnmanagedUsrEntry {
            path: path.into(),
            file_count: 1,
            total_size_bytes: size,
            file_type: FileType::Other,
            kind: UsrEntryKind::File,
            disposition: FindingKind::included(),
            aggregate: None,
            counts_vary: false,
            sizes_vary: false,
        }
    }

    #[test]
    fn renders_header_warning_and_copy_lines() {
        let snap = snap_with(
            vec![
                dir_entry("/usr/lib/custom-agent", 200_000_000, true),
                file_entry("/usr/share/vendor-blob", 12_000_000),
            ],
            true,
        );
        let lines = unmanaged_usr_lines(&snap);
        let out = lines.join("\n");

        assert!(out.contains("# === Unmanaged /usr ==="), "block header: {out}");
        assert!(out.contains("read-only at runtime"), "warning states the contract");
        assert!(out.contains("Building an RPM that owns it"), "names the durable fix");
        assert!(
            out.contains("COPY unmanaged/usr/lib/custom-agent/ /usr/lib/custom-agent/"),
            "directory entry copies the subtree with trailing slashes: {out}"
        );
        assert!(
            out.contains("COPY unmanaged/usr/share/vendor-blob /usr/share/vendor-blob"),
            "single file entry copies the file: {out}"
        );
    }

    #[test]
    fn header_states_the_vendoring_cost() {
        // This line is also the export preview's cost line. The web panel
        // renders `containerfile_preview`, which comes from this same
        // renderer through render_containerfile_with_originals
        // (crates/refine/src/session.rs:2748), so the frontend computes
        // nothing and there is one source of truth for the number.
        let snap = snap_with(vec![dir_entry("/usr/lib/a", 212_000_000, true)], true);
        let out = unmanaged_usr_lines(&snap).join("\n");
        assert!(
            out.contains("1 entry included, 202.2 MB copied into the image."),
            "the user should see the cost before the build does: {out}"
        );
    }

    #[test]
    fn excluded_entries_render_no_copy_line() {
        let snap = snap_with(
            vec![
                dir_entry("/usr/lib/keep", 10, true),
                dir_entry("/usr/lib/drop", 10, false),
            ],
            true,
        );
        let out = unmanaged_usr_lines(&snap).join("\n");
        assert!(out.contains("unmanaged/usr/lib/keep/"));
        assert!(!out.contains("/usr/lib/drop"), "toggling off removes the line: {out}");
    }

    #[test]
    fn unbundled_content_is_flagged_per_path_and_in_the_total() {
        let snap = snap_with(vec![dir_entry("/usr/lib/custom-agent", 10, true)], false);
        let out = unmanaged_usr_lines(&snap).join("\n");
        assert!(
            out.contains("# MISSING FROM BUILD CONTEXT: /usr/lib/custom-agent"),
            "per-path warning: {out}"
        );
        assert!(
            out.contains("Content is NOT in this archive"),
            "the totals line repeats it: {out}"
        );
        assert!(
            out.contains("COPY unmanaged/usr/lib/custom-agent/"),
            "the COPY line still renders; the user staged it or did not"
        );
    }

    #[test]
    fn no_entries_renders_nothing() {
        assert!(unmanaged_usr_lines(&snap_with(Vec::new(), true)).is_empty());
        assert!(
            unmanaged_usr_lines(&InspectionSnapshot::default()).is_empty(),
            "absent section renders nothing"
        );
    }

    #[test]
    fn all_excluded_renders_nothing() {
        let snap = snap_with(vec![dir_entry("/usr/lib/drop", 10, false)], true);
        assert!(unmanaged_usr_lines(&snap).is_empty());
    }

    #[test]
    fn entries_render_largest_first() {
        let snap = snap_with(
            vec![
                dir_entry("/usr/lib/small", 10, true),
                dir_entry("/usr/lib/big", 1_000_000, true),
            ],
            true,
        );
        let out = unmanaged_usr_lines(&snap).join("\n");
        let big = out.find("/usr/lib/big").unwrap();
        let small = out.find("/usr/lib/small").unwrap();
        assert!(big < small, "largest debt on top: {out}");
    }
}
```

Then pin the export preview, so the claim that the block reaches the panel is verified rather than asserted. Add to `crates/refine/tests/export_contract_test.rs`, beside its existing `session.view().containerfile_preview` cases (lines 252, 325, 623):

```rust
#[test]
fn the_export_preview_carries_the_usr_block_and_its_cost_line() {
    // The web panel takes containerfile_preview as opaque text, so the
    // block and its cost line have to be in the rendered Containerfile.
    // If this passes, the frontend needs no /usr props at all.
    let session = session_with_included_usr_entry("/usr/lib/custom-agent", 212_000_000);
    let preview = session.view().containerfile_preview.clone();

    assert!(preview.contains("# === Unmanaged /usr ==="), "block header: {preview}");
    assert!(
        preview.contains("1 entry included, 202.2 MB copied into the image."),
        "cost line reaches the preview: {preview}"
    );
}
```

Build `session_with_included_usr_entry` from the fixtures already in that file.

- [ ] **Step 2: Run to verify failure**

Run: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test -p inspectah-pipeline unmanaged_usr && cargo test -p inspectah-refine --test export_contract_test usr_block`

Expected: FAIL, the module is not declared and `unmanaged_usr_lines` does not exist; the preview test fails on the missing block header.

- [ ] **Step 3: Implement the renderer**

Prepend to `crates/pipeline/src/render/unmanaged_usr.rs`:

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::nonrpm::{UnmanagedUsrEntry, UsrEntryKind};
use inspectah_core::util::format_size;

/// Render Containerfile lines for included unmanaged /usr entries.
///
/// Entries arrive already collapsed to their shallowest unowned ancestor,
/// so there is no grouping to do here: one COPY per entry, largest first.
pub fn unmanaged_usr_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let Some(section) = &snap.unmanaged_files else {
        return Vec::new();
    };

    let mut included: Vec<&UnmanagedUsrEntry> = section
        .usr_entries
        .iter()
        .filter(|e| e.disposition.is_included())
        .collect();
    if included.is_empty() {
        return Vec::new();
    }

    // Largest vendoring debt on top; ties read alphabetically.
    included.sort_by(|a, b| {
        b.total_size_bytes
            .cmp(&a.total_size_bytes)
            .then_with(|| a.path.cmp(&b.path))
    });

    let total_bytes: u64 = included.iter().map(|e| e.total_size_bytes).sum();
    let noun = if included.len() == 1 { "entry" } else { "entries" };

    let mut lines = vec![
        String::new(),
        "# === Unmanaged /usr ===".into(),
        "# In image mode, /usr ships from the container image and stays read-only".into(),
        "# at runtime. This content belongs to no RPM package, so nothing updates".into(),
        "# it and keeping it current is your responsibility. Building an RPM that".into(),
        "# owns it is the durable fix.".into(),
    ];

    if section.usr_bundled {
        lines.push(format!(
            "# {} {noun} included, {} copied into the image.",
            included.len(),
            format_size(total_bytes),
        ));
    } else {
        lines.push(format!(
            "# {} {noun} included, {}. Content is NOT in this archive: the scan",
            included.len(),
            format_size(total_bytes),
        ));
        lines.push("# was run without bundling /usr. Stage these paths in the build".into());
        lines.push("# context before building.".into());
    }

    for entry in included {
        if !section.usr_bundled {
            lines.push(format!("# MISSING FROM BUILD CONTEXT: {}", entry.path));
        }
        let rel = entry.path.trim_start_matches('/');
        match entry.kind {
            UsrEntryKind::Directory => {
                lines.push(format!("COPY unmanaged/{rel}/ {}/", entry.path));
            }
            UsrEntryKind::File => {
                lines.push(format!("COPY unmanaged/{rel} {}", entry.path));
            }
        }
    }

    lines
}
```

Confirm `format_size` lives at `inspectah_core::util::format_size` and produces `"202.2 MB"` for `212_000_000`. If it lives elsewhere or formats differently, adjust the import and the expected string in the Step 1 test to match the real helper. Do not add a second size formatter.

Declare the module in `crates/pipeline/src/render/mod.rs` next to `pub mod unmanaged;`:

```rust
pub mod unmanaged_usr;
```

- [ ] **Step 4: Insert the block into the Containerfile**

In `crates/pipeline/src/render/containerfile.rs`, immediately after line 1218:

```rust
    lines.extend(super::unmanaged_usr::unmanaged_usr_lines(snap));
```

The /usr block follows the Tier 2 unmanaged block, matching the section ordering everywhere else.

- [ ] **Step 5: Run tests, accept snapshot diffs, lint, commit**

Run: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test -p inspectah-pipeline`

Expected: PASS. Any Containerfile `insta` snapshot for a fixture carrying /usr entries gains the block; read each diff before `cargo insta accept`.

```bash
PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo fmt --check && cargo clippy --all-targets -- -D clippy::all
git add crates/pipeline/src/render/unmanaged_usr.rs crates/pipeline/src/render/mod.rs \
        crates/pipeline/src/render/containerfile.rs
git add crates/pipeline/src/render/snapshots
git commit -m "feat(pipeline): render included /usr entries as a Containerfile block

Own block rather than folding into the Tier 2 unmanaged block: the two
answer different questions and the warning text differs. Collapsed
directories copy their subtree, single files copy the file, and the
header states the vendoring cost so a three gigabyte include is visible
before the build discovers it.

When the archive has no bytes for a path the block says so per path and
the totals line repeats it, so an unbuildable export announces itself.

Assisted-by: Claude Code (Opus 5)"
```

---

### Task 7: Audit report Unmanaged /usr section

**Lane:** Tang (pipeline)

**Files:**
- Modify: `crates/pipeline/src/render/audit.rs:924-990` (`render_software_sections`)
- Test: inline `mod tests` in `crates/pipeline/src/render/audit.rs`

**Interfaces:**
- Consumes: `UnmanagedUsrEntry`, `UnmanagedFileSection.usr_bundled` (Task 1).
- Produces: an `### Unmanaged /usr` subsection inside the audit report's Software block.

**Output contract:**

```markdown
### Unmanaged /usr

6 entries included (212 MB), 2 excluded.

Content not present in this archive; stage these paths in the build context:

- /usr/lib/custom-agent
```

The trailing paragraph appears only when `usr_bundled` is false and at least one entry is included. Counts alone when everything is in order.

- [ ] **Step 1: Write the failing tests**

Add to the inline `mod tests` in `crates/pipeline/src/render/audit.rs`:

```rust
#[test]
fn audit_reports_usr_counts_and_included_bytes() {
    let mut snap = InspectionSnapshot::default();
    snap.unmanaged_files = Some(UnmanagedFileSection {
        items: Vec::new(),
        usr_entries: vec![
            UnmanagedUsrEntry {
                path: "/usr/lib/custom-agent".into(),
                file_count: 214,
                total_size_bytes: 212_000_000,
                file_type: FileType::Other,
                kind: UsrEntryKind::Directory,
                disposition: FindingKind::included(),
                aggregate: None,
                counts_vary: false,
                sizes_vary: false,
            },
            UnmanagedUsrEntry {
                path: "/usr/share/skip".into(),
                file_count: 1,
                total_size_bytes: 5,
                file_type: FileType::Other,
                kind: UsrEntryKind::File,
                disposition: FindingKind::excluded(),
                aggregate: None,
                counts_vary: false,
                sizes_vary: false,
            },
        ],
        usr_bundled: true,
        total_size: 0,
        total_count: 0,
    });

    let mut lines = Vec::new();
    render_software_sections(&snap, &mut lines);
    let out = lines.join("\n");

    assert!(out.contains("### Unmanaged /usr"), "section heading: {out}");
    assert!(
        out.contains("1 entry included (202.2 MB), 1 excluded."),
        "counts and included bytes: {out}"
    );
    assert!(
        !out.contains("stage these paths"),
        "bundled content needs no staging note: {out}"
    );
}

#[test]
fn audit_lists_paths_missing_from_the_build_context() {
    let mut snap = InspectionSnapshot::default();
    snap.unmanaged_files = Some(UnmanagedFileSection {
        items: Vec::new(),
        usr_entries: vec![UnmanagedUsrEntry {
            path: "/usr/lib/custom-agent".into(),
            file_count: 214,
            total_size_bytes: 10,
            file_type: FileType::Other,
            kind: UsrEntryKind::Directory,
            disposition: FindingKind::included(),
            aggregate: None,
            counts_vary: false,
            sizes_vary: false,
        }],
        usr_bundled: false,
        total_size: 0,
        total_count: 0,
    });

    let mut lines = Vec::new();
    render_software_sections(&snap, &mut lines);
    let out = lines.join("\n");
    assert!(out.contains("stage these paths in the build context"), "{out}");
    assert!(out.contains("- /usr/lib/custom-agent"), "{out}");
}

#[test]
fn audit_omits_the_usr_section_when_the_walk_found_nothing() {
    let mut snap = InspectionSnapshot::default();
    snap.unmanaged_files = Some(UnmanagedFileSection::default());
    let mut lines = Vec::new();
    render_software_sections(&snap, &mut lines);
    assert!(!lines.join("\n").contains("Unmanaged /usr"));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test -p inspectah-pipeline audit_ -- usr`

Expected: FAIL on the missing heading.

- [ ] **Step 3: Implement**

Append inside `render_software_sections` in `crates/pipeline/src/render/audit.rs`, after the existing `non_rpm_software` block and before the closing brace:

```rust
    if let Some(ufs) = &snap.unmanaged_files
        && !ufs.usr_entries.is_empty()
    {
        let included: Vec<&UnmanagedUsrEntry> = ufs
            .usr_entries
            .iter()
            .filter(|e| e.disposition.is_included())
            .collect();
        let excluded = ufs.usr_entries.len() - included.len();
        let included_bytes: u64 = included.iter().map(|e| e.total_size_bytes).sum();
        let noun = if included.len() == 1 { "entry" } else { "entries" };

        lines.push("### Unmanaged /usr".into());
        lines.push(String::new());
        lines.push(format!(
            "{} {noun} included ({}), {excluded} excluded.",
            included.len(),
            format_size(included_bytes),
        ));
        lines.push(String::new());

        if !ufs.usr_bundled && !included.is_empty() {
            lines.push(
                "Content not present in this archive; stage these paths in the \
                 build context:"
                    .into(),
            );
            lines.push(String::new());
            let mut paths: Vec<&str> = included.iter().map(|e| e.path.as_str()).collect();
            paths.sort_unstable();
            for path in paths {
                lines.push(format!("- {path}"));
            }
            lines.push(String::new());
        }
    }
```

Add `UnmanagedUsrEntry` and `format_size` to the module's imports if absent.

- [ ] **Step 4: Run tests, accept snapshot diffs, lint, commit**

Run: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test -p inspectah-pipeline && cargo fmt --check && cargo clippy --all-targets -- -D clippy::all`

Expected: PASS, zero warnings. Read any audit `insta` diff before accepting.

```bash
git add crates/pipeline/src/render/audit.rs crates/pipeline/src/render/snapshots
git commit -m "docs(audit): report unmanaged /usr counts and included bytes

The audit report ships in the tarball and is the record of what the
export decided. It had nothing to say about /usr. Add counts, the
included-bytes total, and the list of paths whose content is not in the
archive.

Assisted-by: Claude Code (Opus 5)"
```

---

### Task 8: Web DTOs and single-host adapter projection

**Lane:** Kit (web backend)

**Files:**
- Modify: `crates/web/src/web_types.rs:52-67` (alongside the existing unmanaged DTOs)
- Modify: `crates/web/src/adapter.rs:401-430` (the unmanaged-files projection block)
- Test: `crates/web/tests/contract_snapshots.rs`, inline `mod tests` in `crates/web/src/adapter.rs`

**Interfaces:**
- Consumes: `UnmanagedUsrEntry`, `UsrEntryKind` (Task 1).
- Produces: `UnmanagedUsrEntryDto { path: String, kind: String, file_type: String, file_count: u32, size: u64, counts_vary: bool, sizes_vary: bool, prevalence: Option<UsrPrevalenceDto>, include: bool }` and a `unmanaged_usr: Vec<UnmanagedUsrEntryDto>` field plus `has_unmanaged_scan` reuse on the view. Consumed by Tasks 10 and 11.
- Produces: `UsrPrevalenceDto { count: i32, total: i32 }`. Consumed by Task 11.

**Disposition contract:** `UnmanagedUsrEntry.disposition` is always Actionable today, so collapsing it with `.is_included()` into a `bool` is sound and matches every sibling hand-built DTO (`UnmanagedFileItemDto`, `ServiceDecisionDto`, and the rest). If a future change makes /usr entries advisory, convert the DTO to carry the disposition rather than teaching the frontend to guess. See `process-docs/skills/web-disposition-contract.md` section 2.

**Not-scanned signal:** `adapter.rs` already computes `let has_unmanaged_scan = snap.unmanaged_files.is_some();`. Reuse it. Do not add a second flag. `has_unmanaged_scan == false` is the "scanned without `--include-unmanaged`" state for both sections.

- [ ] **Step 1: Write the failing tests**

Add to the inline `mod tests` in `crates/web/src/adapter.rs`:

```rust
#[test]
fn web_view_projects_usr_entries_largest_first() {
    let mut snap = InspectionSnapshot::default();
    snap.unmanaged_files = Some(UnmanagedFileSection {
        items: Vec::new(),
        usr_entries: vec![
            UnmanagedUsrEntry {
                path: "/usr/share/small".into(),
                file_count: 1,
                total_size_bytes: 10,
                file_type: FileType::Other,
                kind: UsrEntryKind::File,
                disposition: FindingKind::included(),
                aggregate: None,
                counts_vary: false,
                sizes_vary: false,
            },
            UnmanagedUsrEntry {
                path: "/usr/lib/custom-agent".into(),
                file_count: 214,
                total_size_bytes: 39_845_888,
                file_type: FileType::Other,
                kind: UsrEntryKind::Directory,
                disposition: FindingKind::excluded(),
                aggregate: None,
                counts_vary: false,
                sizes_vary: false,
            },
        ],
        usr_bundled: true,
        total_size: 0,
        total_count: 0,
    });

    let view = build_web_view(&session_from(snap));
    let rows = &view.data.unmanaged_usr;

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].path, "/usr/lib/custom-agent", "largest first");
    assert_eq!(rows[0].kind, "directory");
    assert_eq!(rows[0].file_count, 214);
    assert_eq!(rows[0].file_type, "other", "directories carry no sniffed type");
    assert!(!rows[0].include, "excluded entries still render, toggled off");
    assert_eq!(rows[1].kind, "file");
    assert_eq!(
        rows[1].file_type, "other",
        "an unclassifiable single file is exactly the case kind exists for"
    );
    assert!(rows[1].include);
}

#[test]
fn web_view_distinguishes_clean_usr_from_an_unscanned_host() {
    let mut clean = InspectionSnapshot::default();
    clean.unmanaged_files = Some(UnmanagedFileSection::default());
    let view = build_web_view(&session_from(clean));
    assert!(view.data.unmanaged_usr.is_empty());
    assert!(
        view.data.has_unmanaged_scan,
        "the walk ran and found nothing; that is the image-clean signal"
    );

    let view = build_web_view(&session_from(InspectionSnapshot::default()));
    assert!(
        !view.data.has_unmanaged_scan,
        "no section at all means --include-unmanaged was not passed"
    );
}
```

Use whatever session-construction helper the surrounding tests already use in place of `session_from`; read the neighbouring tests before writing.

- [ ] **Step 2: Run to verify failure**

Run: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test -p inspectah-web usr`

Expected: FAIL, no field `unmanaged_usr`.

- [ ] **Step 3: Add the DTO**

In `crates/web/src/web_types.rs`, after `UnmanagedFileGroupDto` (line 67):

```rust
/// Aggregate prevalence for a /usr entry, present in aggregate mode only.
#[derive(Serialize, Clone, Debug)]
pub struct UsrPrevalenceDto {
    pub count: i32,
    pub total: i32,
}

/// A collapsed directory or single file under /usr owned by no RPM.
#[derive(Serialize, Clone, Debug)]
pub struct UnmanagedUsrEntryDto {
    pub path: String,
    /// "file" or "directory". Recorded at collection time, not inferred.
    pub kind: String,
    /// Sniffed type, used for the kind badge on single-file entries:
    /// "elf_binary", "jar", "script", "data_file", "config", "symlink",
    /// or "other". Always "other" for directories.
    pub file_type: String,
    pub file_count: u32,
    pub size: u64,
    /// True when contributing hosts disagreed; the UI renders "up to N".
    #[serde(default, skip_serializing_if = "is_false")]
    pub counts_vary: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub sizes_vary: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prevalence: Option<UsrPrevalenceDto>,
    pub include: bool,
}
```

`is_false` already exists at `crates/web/src/web_types.rs:72`.

- [ ] **Step 4: Project it in the adapter**

In `crates/web/src/adapter.rs`, after the `unmanaged_files` grouping block that ends around line 430:

```rust
    // -- Unmanaged /usr ------------------------------------------------------
    //
    // Already collapsed to shallowest unowned ancestors at collection time,
    // so there is no grouping to do. Sorted largest first: the biggest
    // vendoring debt is the one worth a decision.
    let mut unmanaged_usr: Vec<UnmanagedUsrEntryDto> = snap
        .unmanaged_files
        .as_ref()
        .map(|ufs| {
            ufs.usr_entries
                .iter()
                .map(|e| UnmanagedUsrEntryDto {
                    path: e.path.clone(),
                    kind: match e.kind {
                        UsrEntryKind::File => "file".to_string(),
                        UsrEntryKind::Directory => "directory".to_string(),
                    },
                    file_type: file_type_str(&e.file_type).to_string(),
                    file_count: e.file_count,
                    size: e.total_size_bytes,
                    counts_vary: e.counts_vary,
                    sizes_vary: e.sizes_vary,
                    prevalence: e.aggregate.as_ref().map(|a| UsrPrevalenceDto {
                        count: a.count,
                        total: a.total,
                    }),
                    include: e.disposition.is_included(),
                })
                .collect()
        })
        .unwrap_or_default();
    unmanaged_usr.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
```

Add `unmanaged_usr` to the view struct next to `unmanaged_files` and populate it in the struct literal. Add the imports.

- [ ] **Step 5: Run tests, refresh contract snapshots, lint, commit**

Run: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test -p inspectah-web`

Expected: PASS. `crates/web/tests/contract_snapshots.rs` will show the new field; read the diff and accept.

```bash
PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo fmt --check && cargo clippy --all-targets -- -D clippy::all
git add crates/web/src/web_types.rs crates/web/src/adapter.rs crates/web/tests/snapshots
git commit -m "feat(web): project unmanaged /usr entries into the single-host view

Entries arrive pre-collapsed, so the DTO is flat rather than grouped by
parent the way Tier 2 unmanaged files are. Sorted largest first. The
existing has_unmanaged_scan flag already separates a clean /usr from a
host scanned without --include-unmanaged; no second flag.

Assisted-by: Claude Code (Opus 5)"
```

---

### Task 9: Aggregate handler section and batch include/exclude routing

**Lane:** Kit (web backend)

**Files:**
- Modify: `crates/web/src/aggregate_handlers.rs:1395-1460` (section emission, alongside the `unmanaged_files` block)
- Modify: `crates/web/src/handlers.rs:1032-1076` (the `SectionGroup::Software` batch arm)
- Test: inline `mod tests` in both files

**Interfaces:**
- Consumes: `ItemId::UnmanagedUsr` (Task 3), section id `unmanaged_usr` (Task 4), `UnmanagedUsrEntryDto` (Task 8).
- Produces: an `AggregateSection` with `id: "unmanaged_usr"` carrying `AggregateItem`s keyed by `ItemId::UnmanagedUsr`, and batch include/exclude ops for the section.

**Zone behavior:** the existing aggregate zone machinery decides placement from prevalence. 100 percent prevalence auto-includes; partial prevalence lands in a review zone. Emit the section the same way `unmanaged_files` is emitted at line 1452 and let the machinery do its job. Do not add /usr-specific zone logic, badges, or counting.

**No variants:** /usr entries carry no content hash, so pass `variants: None` and no variant payload. Variant detection is out of scope until subtree digests arrive.

- [ ] **Step 1: Write the failing tests**

Add to the inline `mod tests` in `crates/web/src/aggregate_handlers.rs`, modelled on the existing `aggregate_unmanaged_files_*` tests at lines 2493-2650:

```rust
#[test]
fn aggregate_unmanaged_usr_section_emitted() {
    let snap = aggregate_snapshot_with_usr(vec![usr_entry_with_prevalence(
        "/usr/lib/custom-agent",
        39_845_888,
        2,
        3,
    )]);
    let sections = build_aggregate_sections(&snap);
    let section = sections
        .iter()
        .find(|s| s.id == "unmanaged_usr")
        .expect("unmanaged_usr section should be present");
    assert_eq!(section.items.len(), 1);
    assert_eq!(section.label, "Unmanaged /usr");
}

#[test]
fn aggregate_unmanaged_usr_100_pct_includes() {
    let snap = aggregate_snapshot_with_usr(vec![usr_entry_with_prevalence(
        "/usr/lib/everywhere",
        10,
        3,
        3,
    )]);
    let section = build_aggregate_sections(&snap)
        .into_iter()
        .find(|s| s.id == "unmanaged_usr")
        .expect("section should exist");
    assert!(
        section.items[0].include,
        "full prevalence auto-includes, same as every other family"
    );
}

#[test]
fn aggregate_unmanaged_usr_partial_lands_in_review() {
    // Task 2's merge narrows partial prevalence to include = false; this
    // handler serializes that stored bit straight through, exactly as the
    // unmanaged_files block does (aggregate_handlers.rs:1434-1440). Pin
    // the include bit, not just the counts: the counts alone would still
    // pass if the row auto-included fleet-wide.
    let snap = aggregate_snapshot_with_usr(vec![usr_entry_with_prevalence(
        "/usr/lib/somewhere",
        10,
        1,
        3,
    )]);
    let section = build_aggregate_sections(&snap)
        .into_iter()
        .find(|s| s.id == "unmanaged_usr")
        .expect("section should exist");
    let item = &section.items[0];
    assert_eq!(item.prevalence.as_ref().unwrap().count, 1);
    assert_eq!(item.prevalence.as_ref().unwrap().total, 3);
    assert!(
        !item.include,
        "partial prevalence lands in review, not auto-included"
    );
}
```

`usr_entry_with_prevalence` must build the entry the way the merge leaves it: `disposition` excluded when `count < total`, included when `count == total`. Building it always-included would make `aggregate_unmanaged_usr_partial_lands_in_review` test the fixture rather than the handler.

Write `aggregate_snapshot_with_usr` and `usr_entry_with_prevalence` as local helpers mirroring the existing `UnmanagedFileSection` fixtures at lines 2499 and 2553. Adapt `build_aggregate_sections` to the real function name at HEAD.

Add to `crates/web/src/handlers.rs` tests:

```rust
#[test]
fn software_batch_exclude_covers_usr_entries() {
    let snap = snapshot_with_usr_and_tier2();
    let ops = build_software_group_ops(&snap, false);
    assert!(
        ops.iter().any(|op| matches!(
            op,
            inspectah_refine::types::RefinementOp::SetInclude {
                item_id: inspectah_refine::types::ItemId::UnmanagedUsr { path },
                include: false,
            } if path == "/usr/lib/custom-agent"
        )),
        "batch exclude on the Software group must reach the /usr section"
    );
}
```

If the ops construction is inline in the handler rather than a callable function, extract it into `fn build_software_group_ops(snap: &InspectionSnapshot, include: bool) -> Vec<RefinementOp>` as part of this task so it is testable. That extraction is in scope; it is the only way to give this task a real verification.

- [ ] **Step 2: Run to verify failure**

Run: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test -p inspectah-web usr`

Expected: FAIL, no `unmanaged_usr` section found.

- [ ] **Step 3: Emit the aggregate section**

In `crates/web/src/aggregate_handlers.rs`, after the `unmanaged_files` section block that ends around line 1460:

```rust
    if let Some(ref unmanaged) = snap.unmanaged_files
        && !unmanaged.usr_entries.is_empty()
    {
        let mut entries: Vec<&UnmanagedUsrEntry> = unmanaged.usr_entries.iter().collect();
        entries.sort_by(|a, b| {
            b.total_size_bytes
                .cmp(&a.total_size_bytes)
                .then_with(|| a.path.cmp(&b.path))
        });

        let items: Vec<AggregateItem> = entries
            .into_iter()
            .map(|e| AggregateItem {
                item_id: ItemId::UnmanagedUsr {
                    path: e.path.clone(),
                },
                // No content hashes exist for /usr entries, so there is
                // nothing to compare across hosts.
                variants: None,
                variant_payload: None,
                section_metadata: build_unmanaged_usr_metadata(e),
                ..aggregate_item_defaults(e.path.clone(), e.aggregate.as_ref(), &e.disposition)
            })
            .collect();

        push_section(&mut sections, "unmanaged_usr", "Unmanaged /usr", items);
    }
```

**Mirror, do not invent.** The `unmanaged_files` block at `crates/web/src/aggregate_handlers.rs:1402-1455` is the template. Read it first and copy its structure line for line, changing only: the source vector (`usr_entries` rather than `items`), the `ItemId` constructor, the section id and label passed at line 1452, and the metadata builder. `aggregate_item_defaults` and `push_section` above are stand-in names for whatever that block actually calls; use its real calls. The only deliberate divergences from it are `variants: None` and `variant_payload: None`, and the pre-sort by size, both of which the snippet above shows explicitly.

Add `build_unmanaged_usr_metadata` next to `build_unmanaged_file_metadata` (line 1566), carrying `kind`, `file_type`, `file_count`, `size`, `counts_vary`, and `sizes_vary`. `file_type` is there for the single-file case, where the badge shows the sniffed type rather than a count. This metadata is the whole input to Task 11's row branch, `AggregateItemRow.tsx`, and to `ItemDetailPane.tsx`, so the field names here and the `UnmanagedUsrMetadata` interface Task 11 adds to `types.ts` must match exactly.

- [ ] **Step 4: Route the batch ops**

In `crates/web/src/handlers.rs`, inside the `SectionGroup::Software` arm, after the existing unmanaged-files loop:

```rust
            // Unmanaged /usr
            if let Some(ref unmanaged) = snap.unmanaged_files {
                for entry in &unmanaged.usr_entries {
                    if entry.disposition.is_advisory() {
                        continue;
                    }
                    ops.push(inspectah_refine::types::RefinementOp::SetInclude {
                        item_id: inspectah_refine::types::ItemId::UnmanagedUsr {
                            path: entry.path.clone(),
                        },
                        include: payload.include,
                    });
                }
            }
```

`UnmanagedUsrEntry` has no `locked` field, so there is no lock check here. The `is_advisory` guard matches the sibling loops and costs nothing.

- [ ] **Step 5: Run tests, lint, commit**

Run: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test -p inspectah-web && cargo fmt --check && cargo clippy --all-targets -- -D clippy::all`

Expected: PASS, zero warnings.

```bash
git add crates/web/src/aggregate_handlers.rs crates/web/src/handlers.rs crates/web/tests/snapshots
git commit -m "feat(web): emit the aggregate Unmanaged /usr section and batch ops

Emitted the same way unmanaged_files is, so the existing zone machinery
handles it: full prevalence auto-includes, partial lands in review. No
/usr-specific zone logic and no variants, since /usr entries carry no
content hash to compare across hosts.

Software-group batch include and exclude now reach the section.

Assisted-by: Claude Code (Opus 5)"
```

---

### Task 10: Single-host decision grid

**Lane:** Kit (web UI)

**Files:**
- Create: `crates/web/ui/src/components/UnmanagedUsrList.tsx`
- Create: `crates/web/ui/src/components/__tests__/UnmanagedUsrList.test.tsx`
- Modify: `crates/web/ui/src/api/types.ts:553` (view data types)
- Modify: `crates/web/ui/src/components/MainContent.tsx:38-53` (`SECTION_LABELS`), `:661+` (section render)
- Modify: `crates/web/ui/src/components/Sidebar.tsx:94-97` (badge count)
- Modify: `crates/web/ui/src/App.tsx:126,306-370` (section dispatch and batch handlers)

**Interfaces:**
- Consumes: `unmanaged_usr: UnmanagedUsrEntry[]`, `has_unmanaged_scan: boolean` from the view (Task 8).
- Produces: the `unmanaged_usr` section render, reachable inside the Software group.

**Reuse, do not reinvent.** This is a decision surface, so it uses the grid idiom from `DecisionItem.tsx:276-397` (`role="grid"`, `role="row"`, `role="gridcell"`, roving `tabIndex`, `aria-rowindex`) and the flat focus-index model from `DecisionList.tsx`, not the list/checkbox idiom of `UnmanagedFileList.tsx`. The include/exclude mutation goes through the same mutation hook `App.tsx` already uses for `unmanaged_files` (lines 306-370), with `ItemId::UnmanagedUsr` as the target. Do not write a new keyboard handler, a new status region pattern, or a new batch toolbar.

**Row cells, in order:** selection checkbox, path (monospace, middle-truncated on overflow with the full path in the accessible name and `title`), kind badge, total size, include/exclude toggle. No expand affordance: entries carry no child list, so there is nothing to drill into.

**Kind badge text:** `Directory, 214 files` for a directory (`up to 214 files` when `counts_vary`), or the single-file type for a file: `ELF binary`, `Script`, `Symlink`, `File`. Map from `file_type` for the file case and fall back to `File`.

**Keyboard:** the grid is one tab stop. Arrow Up/Down move row focus; Home/End jump to first/last. Space toggles the focused row's selection checkbox. Enter toggles include/exclude. Shift+click on a row checkbox extends selection from the last selected row. A select-all checkbox lives in the grid header. Global shortcuts keep their existing behavior and none are shadowed.

**Screen reader:** grid label `Unmanaged /usr entries`. Row accessible name composed from the cells, for example `/usr/lib/custom-agent, directory, 214 files, 38 megabytes, included`. One `role="status" aria-live="polite"` region per section, matching `UnmanagedFileList.tsx`, announcing a single contextual message such as `/usr/lib/custom-agent excluded. 12 entries excluded, 202 included.` Never announce a bare count. All interactive targets meet a 44 px minimum hit size and focus is always visibly ringed.

**Three states.** These are what the tests pin.

1. `has_unmanaged_scan === false`: render the section heading, the framing copy, and the not-scanned message from Global Constraints. No grid, no toolbar.
2. `has_unmanaged_scan === true` and `unmanaged_usr.length === 0`: render the heading and the empty-state copy from Global Constraints. No grid, no toolbar.
3. Otherwise: heading, framing copy, batch toolbar, grid.

- [ ] **Step 1: Write the failing tests**

Create `crates/web/ui/src/components/__tests__/UnmanagedUsrList.test.tsx`:

```tsx
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { UnmanagedUsrList } from "../UnmanagedUsrList";
import type { UnmanagedUsrEntry } from "../../api/types";

const dirEntry: UnmanagedUsrEntry = {
  path: "/usr/lib/custom-agent",
  kind: "directory",
  file_count: 214,
  size: 39_845_888,
  include: true,
};

const fileEntry: UnmanagedUsrEntry = {
  path: "/usr/share/vendor-blob",
  kind: "file",
  file_count: 1,
  size: 4096,
  include: true,
};

function renderList(props: Partial<React.ComponentProps<typeof UnmanagedUsrList>> = {}) {
  return render(
    <UnmanagedUsrList
      entries={[dirEntry, fileEntry]}
      hasUnmanagedScan={true}
      onToggle={vi.fn()}
      onBatchToggle={vi.fn()}
      {...props}
    />,
  );
}

describe("UnmanagedUsrList", () => {
  it("shows the not-scanned state when the walk did not run", () => {
    renderList({ entries: [], hasUnmanagedScan: false });
    expect(
      screen.getByText(/collected without --include-unmanaged/),
    ).toBeInTheDocument();
    expect(screen.queryByRole("grid")).not.toBeInTheDocument();
  });

  it("shows the image-clean state when the walk found nothing", () => {
    renderList({ entries: [], hasUnmanagedScan: true });
    expect(screen.getByText(/This host's \/usr is image-clean/)).toBeInTheDocument();
    expect(screen.queryByRole("grid")).not.toBeInTheDocument();
  });

  it("renders a grid with one row per entry", () => {
    renderList();
    const grid = screen.getByRole("grid", { name: "Unmanaged /usr entries" });
    expect(within(grid).getAllByRole("row")).toHaveLength(3); // header + 2
  });

  it("labels a directory row with its rolled-up file count", () => {
    renderList();
    expect(screen.getByText("Directory, 214 files")).toBeInTheDocument();
  });

  it("renders 'up to N files' when hosts disagreed", () => {
    renderList({ entries: [{ ...dirEntry, counts_vary: true }] });
    expect(screen.getByText("Directory, up to 214 files")).toBeInTheDocument();
  });

  it("puts the full path in the row's accessible name", () => {
    renderList();
    const row = screen.getByRole("row", { name: /\/usr\/lib\/custom-agent/ });
    expect(row).toHaveAccessibleName(
      expect.stringContaining("/usr/lib/custom-agent"),
    );
  });

  it("moves row focus with ArrowDown and keeps one tab stop", async () => {
    const user = userEvent.setup();
    renderList();
    const rows = screen.getAllByRole("row").slice(1);
    rows[0].focus();
    expect(rows[0]).toHaveAttribute("tabindex", "0");
    expect(rows[1]).toHaveAttribute("tabindex", "-1");
    await user.keyboard("{ArrowDown}");
    expect(rows[1]).toHaveFocus();
  });

  it("toggles include/exclude on Enter", async () => {
    const user = userEvent.setup();
    const onToggle = vi.fn();
    renderList({ onToggle });
    screen.getAllByRole("row")[1].focus();
    await user.keyboard("{Enter}");
    expect(onToggle).toHaveBeenCalledWith("/usr/lib/custom-agent", false);
  });

  it("announces a contextual message, never a bare count", async () => {
    const user = userEvent.setup();
    renderList();
    screen.getAllByRole("row")[1].focus();
    await user.keyboard("{Enter}");
    const status = screen.getByRole("status");
    expect(status).toHaveTextContent(
      "/usr/lib/custom-agent excluded. 1 entry excluded, 1 included.",
    );
  });

  it("toggles the focused row's selection checkbox on Space", async () => {
    const user = userEvent.setup();
    renderList();
    const row = screen.getAllByRole("row")[1];
    row.focus();
    await user.keyboard(" ");
    expect(within(row).getByRole("checkbox")).toBeChecked();
    await user.keyboard(" ");
    expect(within(row).getByRole("checkbox")).not.toBeChecked();
  });

  it("does not toggle include/exclude on Space", async () => {
    // Space selects, Enter decides. Conflating them is the easy bug here.
    const user = userEvent.setup();
    const onToggle = vi.fn();
    renderList({ onToggle });
    screen.getAllByRole("row")[1].focus();
    await user.keyboard(" ");
    expect(onToggle).not.toHaveBeenCalled();
  });

  it("extends selection from the last selected row on Shift+click", async () => {
    const user = userEvent.setup();
    renderList({ entries: [dirEntry, fileEntry, thirdEntry] });
    const boxes = screen.getAllByRole("row").slice(1)
      .map((r) => within(r).getByRole("checkbox"));
    await user.click(boxes[0]);
    await user.click(boxes[2], { shiftKey: true });
    expect(boxes.every((b) => (b as HTMLInputElement).checked)).toBe(true);
  });

  it("selects and clears every row from the header checkbox", async () => {
    const user = userEvent.setup();
    renderList();
    const selectAll = screen.getByRole("checkbox", { name: /select all/i });
    await user.click(selectAll);
    const rowBoxes = screen.getAllByRole("row").slice(1)
      .map((r) => within(r).getByRole("checkbox"));
    expect(rowBoxes.every((b) => (b as HTMLInputElement).checked)).toBe(true);
    await user.click(selectAll);
    expect(rowBoxes.some((b) => (b as HTMLInputElement).checked)).toBe(false);
  });

  it("batch-excludes exactly the selected rows", async () => {
    const user = userEvent.setup();
    const onBatchToggle = vi.fn();
    renderList({ onBatchToggle });
    const rows = screen.getAllByRole("row").slice(1);
    await user.click(within(rows[0]).getByRole("checkbox"));
    await user.click(screen.getByRole("button", { name: /exclude selected/i }));
    expect(onBatchToggle).toHaveBeenCalledWith(["/usr/lib/custom-agent"], false);
  });
});
```

Add a `thirdEntry` fixture beside `dirEntry` and `fileEntry` for the range-select case. Adapt the select-all accessible name and the batch-toolbar button label to whatever the sibling sections already use; read `UnmanagedFileList.tsx`'s toolbar before naming them, and match it rather than inventing new copy.

**Two criteria are staged to manual verification, deliberately.** The 44 px minimum hit size and the visible focus ring come from stylesheet rules, and jsdom does not apply linked stylesheets, so `getComputedStyle` in vitest returns the unstyled defaults and any assertion on them would pass or fail for reasons unrelated to the contract. Rather than write a test that proves nothing, verify both in a browser during Task 10's review:

- Load a snapshot with /usr entries, tab into the section, and confirm every row control and the header checkbox have a visible focus ring.
- With devtools, confirm the checkbox, the toggle, and the row hit target each measure at least 44 px in their smaller dimension.

Record the result in the Task 10 review notes. If the Playwright expansion in the backlog lands before this task, move both checks there instead; they are exactly what that harness is for.

- [ ] **Step 2: Run to verify failure**

Run: `cd crates/web/ui && npm test -- UnmanagedUsrList`

Expected: FAIL, cannot resolve `../UnmanagedUsrList`.

- [ ] **Step 3: Add the TypeScript types**

In `crates/web/ui/src/api/types.ts`, next to `UnmanagedFileGroup` (line 553):

```ts
export interface UnmanagedUsrEntry {
  path: string;
  /** "file" or "directory". Recorded at collection, not inferred. */
  kind: "file" | "directory";
  /** Sniffed type for the badge on single-file entries. */
  file_type: string;
  file_count: number;
  size: number;
  /** Hosts disagreed on the count; render "up to N". */
  counts_vary?: boolean;
  sizes_vary?: boolean;
  prevalence?: { count: number; total: number };
  include: boolean;
}
```

Add `unmanaged_usr?: UnmanagedUsrEntry[];` to the view data interface alongside `unmanaged_files`.

`prevalence` is on the type because Task 8's DTO carries it, but it is always absent in single-host mode and `UnmanagedUsrList` never renders it. Aggregate rows come from `AggregateItemRow`, not this component; see Task 11. This component takes no aggregate mode and no `isAggregate` prop.

- [ ] **Step 4: Build the component**

Create `crates/web/ui/src/components/UnmanagedUsrList.tsx`. Model the grid structure, roving-tabindex hook, selection state, and status-region wiring directly on `DecisionList.tsx` and `DecisionItem.tsx`. Do not copy `UnmanagedFileList.tsx`'s list idiom; copy its `role="status"` region only.

Framing copy, empty copy, and not-scanned copy go in verbatim from Global Constraints.

Kind label helper. `file_type` comes from the DTO (Task 8); the frontend never sniffs or infers it:

```tsx
const FILE_TYPE_LABELS: Record<string, string> = {
  elf_binary: "ELF binary",
  jar: "Jar",
  script: "Script",
  data_file: "Data file",
  config: "Config",
  symlink: "Symlink",
  other: "File",
};

function kindLabel(entry: UnmanagedUsrEntry): string {
  if (entry.kind === "directory") {
    const count = entry.counts_vary
      ? `up to ${entry.file_count}`
      : String(entry.file_count);
    return `Directory, ${count} file${entry.file_count === 1 ? "" : "s"}`;
  }
  return FILE_TYPE_LABELS[entry.file_type] ?? "File";
}
```

Confirm the key strings against `file_type_str` in `crates/web/src/adapter.rs`, which is what Task 8's projection calls. If that helper emits different strings, use its strings as the keys rather than adding a second mapping.

Status message helper:

```tsx
function announce(path: string, nowIncluded: boolean, entries: UnmanagedUsrEntry[]): string {
  const included = entries.filter((e) => e.include).length;
  const excluded = entries.length - included;
  const verb = nowIncluded ? "included" : "excluded";
  return `${path} ${verb}. ${excluded} ${excluded === 1 ? "entry" : "entries"} excluded, ${included} included.`;
}
```

- [ ] **Step 5: Wire the section in**

- `MainContent.tsx:38-53`: add `unmanaged_usr: "Unmanaged /usr",` to `SECTION_LABELS`.
- `MainContent.tsx`: add the `unmanaged_usr` render branch after the `unmanaged_files` branch at line 661, passing `viewData?.unmanaged_usr ?? []` and `viewData?.has_unmanaged_scan ?? false`.
- `Sidebar.tsx:94-97`: add the badge count branch, mirroring the `unmanaged_files` one exactly:

```ts
  if (id === "unmanaged_usr") {
    if (!viewData) return "...";
    return String(viewData.unmanaged_usr?.length ?? 0);
  }
```

- `App.tsx:126`: extend the `activeSection === "unmanaged_files"` branch handling to cover `unmanaged_usr`, or add a sibling branch, whichever the surrounding code makes cleaner.
- `App.tsx:306-370`: add `handleUsrToggle` and `handleUsrBatchToggle` callbacks mirroring the unmanaged-files ones, targeting `{ UnmanagedUsr: { path } }` as the item id.
- **Legacy flat section list:** the sidebar renders a grouped list today, but a flat fallback list still exists for snapshots without group metadata. Grep `crates/web/ui/src` for the array that lists section ids in flat order and insert `unmanaged_usr` immediately after `unmanaged_files` there too. If no flat list survives at HEAD, note that in the task's completion report rather than adding one.
- `useKeyboard.ts` needs no change: number keys 1-8 jump to groups, and the section is reachable inside the Software group. Verify by running the app and pressing the Software group's number key, then arrowing to the section.

- [ ] **Step 6: Run tests, typecheck, commit**

Run: `cd crates/web/ui && npm test && npm run build`

Expected: all vitest suites PASS and `tsc` reports no errors.

```bash
git add crates/web/ui/src/components/UnmanagedUsrList.tsx \
        crates/web/ui/src/components/__tests__/UnmanagedUsrList.test.tsx \
        crates/web/ui/src/api/types.ts crates/web/ui/src/components/MainContent.tsx \
        crates/web/ui/src/components/Sidebar.tsx crates/web/ui/src/App.tsx
git commit -m "feat(web): add the Unmanaged /usr decision grid

Grid idiom rather than the checkbox list Unmanaged Files uses, because
this section carries decisions: roving focus, Enter to toggle, Space to
select, one polite status region per section. Nothing new to learn.

Three states, and the difference matters: a host with no unmanaged /usr
is image-clean and says so, while a host scanned without
--include-unmanaged was never checked and says that instead.

Assisted-by: Claude Code (Opus 5)"
```

---

### Task 11: Aggregate row and detail-pane metadata for the /usr section

**Lane:** Kit (web UI)

**Files:**
- Modify: `crates/web/ui/src/components/aggregate/AggregateItemRow.tsx:204-236` (section-metadata branch)
- Modify: `crates/web/ui/src/components/aggregate/AggregateSection.tsx:54-57` (`itemFilterText` branch)
- Modify: `crates/web/ui/src/components/aggregate/ItemDetailPane.tsx:186` (detail metadata branch)
- Modify: `crates/web/ui/src/api/types.ts` (`UnmanagedUsrMetadata`, beside `UnmanagedFileMetadata`)
- Test: `crates/web/ui/src/components/aggregate/__tests__/AggregateItemRow.test.tsx`, `aggregate/__tests__/ItemDetailPane.test.tsx`

**Interfaces:**
- Consumes: the `unmanaged_usr` aggregate section and its `section_metadata` (Task 9).

**Read this before writing any code: `UnmanagedUsrList` is not on the aggregate path.** Task 10's component is a single-host component reached through `MainContent.tsx`. Aggregate mode never mounts it. `AggregateApp.tsx:608-623` routes every non-package section to `AggregateSectionContent` (`aggregate/AggregateSection.tsx:45-58`), which renders one `AggregateItemRow` per item inside `ZoneGroup`s. Task 10's component therefore gets **no** `isAggregate` prop and no prevalence cell; this task touches the aggregate components only.

**What the generic aggregate row already does, so do not rebuild it:**
- **Prevalence.** `AggregateItemRow.tsx:279` already renders `<PrevalenceBadge count={count} total={total} suffix="hosts" />` from `item.prevalence`, for every section. Task 9 populates `prevalence`, so `14/20 hosts` appears with zero frontend change. The badge sits inside the `role="row"` element, so prevalence is already part of the row's accessible name and the decision's scope is already audible.
- **Include toggle.** `AggregateItemRow.tsx:141,256` already drives `onToggle(item.item_id, !item.include)` from `item.include`. Task 9's `ItemId::UnmanagedUsr` and stored include bit flow through unchanged, one decision per path, fleet-wide.

**What is actually missing** is the `sectionId === "unmanaged_usr"` metadata branch in all three places that switch on section id. That, plus `up to` rendering for the varies flags, is the entire frontend delta.

**Ordering, stated plainly rather than left to discover.** Aggregate sections group items into consensus / near-consensus / divergent zones (`findItemZone` in `AggregateSection.tsx`, rendered through `ZoneGroup`). Size-descending order therefore holds *within a zone*, not across the whole section: a 3 GB entry on 2 of 200 hosts renders in the divergent zone, below smaller universal entries. Two design-note lines pull against each other here — "default sort stays size descending" (line 273) and "aggregates use the existing zone machinery" (lines 45-50, 201-203). The second is a settled product decision, so it governs: zones first, size descending inside each. Task 9 emits items pre-sorted by size, which is what makes the within-zone order right. Do not add a section-level re-sort; it would fight the zone grouping.

**The export preview cost line needs no work in this task.** Task 6 emits `# 6 entries included, 212 MB copied into the image.` as a comment inside the `=== Unmanaged /usr ===` block and asserts it there. `ContainerfilePanel` renders `containerfile_preview`, which comes from `render_containerfile_with_originals` (`crates/refine/src/session.rs:2748`) — the same renderer — so the cost line reaches the preview as part of the text. The panel's interface is `{content, isOpen, onToggle, loading, sessionIsSensitive}` (`ContainerfilePanel.tsx:6-13`), and `AppShell` passes exactly those (`AppShell.tsx:256-261`). Do not add a `usrEntries` prop or recompute the total in the frontend: that would be a second source of truth for a number the renderer already produces. Task 6 carries the end-to-end pin.

- [ ] **Step 1: Write the failing tests**

Add to `crates/web/ui/src/components/aggregate/__tests__/AggregateItemRow.test.tsx`. Reuse that file's existing render helper and `AggregateItem` fixture rather than writing new ones; the `unmanaged_files` cases in it are the direct model.

```tsx
const usrItem: AggregateItem = {
  ...baseItem,
  item_id: { UnmanagedUsr: { path: "/usr/lib/custom-agent" } },
  include: true,
  prevalence: { count: 14, total: 20, hosts: [] },
  section_metadata: {
    kind: "directory",
    file_count: 214,
    size: 39_845_888,
    counts_vary: false,
    sizes_vary: false,
  },
};

it("renders kind and rolled-up file count for a /usr directory row", () => {
  renderRow({ sectionId: "unmanaged_usr", item: usrItem });
  expect(screen.getByText("Directory, 214 files")).toBeInTheDocument();
});

it("renders 'up to' counts and sizes when hosts disagreed", () => {
  renderRow({
    sectionId: "unmanaged_usr",
    item: {
      ...usrItem,
      section_metadata: {
        ...usrItem.section_metadata,
        counts_vary: true,
        sizes_vary: true,
      },
    },
  });
  expect(screen.getByText("Directory, up to 214 files")).toBeInTheDocument();
  expect(screen.getByText(/up to 38(\.0)? MB/)).toBeInTheDocument();
});

it("carries prevalence in the row so the fleet-wide scope is audible", () => {
  // Regression guard, not new behavior: PrevalenceBadge already renders
  // for every section. This pins that the /usr branch did not displace it.
  renderRow({ sectionId: "unmanaged_usr", item: usrItem });
  const row = screen.getByRole("row");
  expect(within(row).getByText(/14.*20/)).toBeInTheDocument();
});

it("toggles the fleet-wide decision through the standard row control", () => {
  const onToggle = vi.fn();
  renderRow({ sectionId: "unmanaged_usr", item: usrItem, onToggle });
  fireEvent.click(screen.getByRole("checkbox"));
  expect(onToggle).toHaveBeenCalledWith(usrItem.item_id, false);
});

it("renders a single-file /usr row with its sniffed type, not a file count", () => {
  renderRow({
    sectionId: "unmanaged_usr",
    item: {
      ...usrItem,
      item_id: { UnmanagedUsr: { path: "/usr/share/vendor-blob" } },
      section_metadata: {
        kind: "file",
        file_type: "other",
        file_count: 1,
        size: 4096,
        counts_vary: false,
        sizes_vary: false,
      },
    },
  });
  expect(screen.queryByText(/1 files/)).not.toBeInTheDocument();
});
```

Add to `crates/web/ui/src/components/aggregate/__tests__/ItemDetailPane.test.tsx`, mirroring its `unmanaged_files` case:

```tsx
it("shows /usr entry metadata in the detail pane", () => {
  render(<ItemDetailPane item={usrItem} sectionId="unmanaged_usr" />);
  expect(screen.getByText(/214/)).toBeInTheDocument();
  expect(screen.getByText(/38(\.0)? MB/)).toBeInTheDocument();
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd crates/web/ui && npm test -- AggregateItemRow ItemDetailPane`

Expected: FAIL. The rows render with a bare display name and no section metadata, because no branch matches `sectionId === "unmanaged_usr"`.

- [ ] **Step 3: Implement the three branches**

Add `UnmanagedUsrMetadata` to `crates/web/ui/src/api/types.ts` beside `UnmanagedFileMetadata`, matching the DTO Task 9 emits: `kind`, `file_type`, `file_count`, `size`, `counts_vary`, `sizes_vary`.

In `AggregateItemRow.tsx`, add a `sectionId === "unmanaged_usr"` branch immediately after the `unmanaged_files` branch at line 204, built the same way (a `<span className="aggregate-item-row__section-meta">` holding compact `Label`s). Contents:

- Directory: `Directory, 214 files`, or `Directory, up to 214 files` when `counts_vary`.
- File: the sniffed type through the existing `formatFileType` helper, the same one the `unmanaged_files` branch uses. No file count on a single file.
- Size through the existing `formatSize` helper, prefixed `up to ` when `sizes_vary`.

Reuse `formatFileType` and `formatSize` from that file. Do not add a second formatter, and do not add a prevalence element: line 279 already renders it for every section.

In `AggregateSection.tsx`, add the matching `itemFilterText` branch after the `unmanaged_files` one at line 54, returning the display name plus kind so in-section filtering matches what the row shows.

In `ItemDetailPane.tsx`, add the `sectionId === "unmanaged_usr"` branch after line 186, mirroring the `unmanaged_files` branch.

- [ ] **Step 4: Run tests, typecheck, commit**

Run: `cd crates/web/ui && npm test && npm run build`

Expected: PASS, no type errors.

```bash
git add crates/web/ui/src/components/aggregate/ \
        crates/web/ui/src/api/types.ts
git commit -m "feat(web): render /usr entry metadata in aggregate rows

Aggregate sections render through AggregateItemRow, which switches on
section id for its metadata cells. Without a /usr branch the rows showed
a bare path with no kind, count, or size. Add the branch beside the
unmanaged_files one, plus the matching filter-text and detail-pane
cases.

Prevalence and the include toggle needed nothing: both are generic to
every aggregate row already, which is the point of putting this section
on the existing zone machinery rather than a bespoke surface.

Counts and sizes read 'up to' when contributing hosts disagreed, so a
merged maximum is never mistaken for a measurement.

Assisted-by: Claude Code (Opus 5)"
```

---

### Task 12: HTML report section

**Lane:** Kit (report template and renderer context)

**Files:**
- Create: `crates/pipeline/templates/report/unmanaged-usr.html`
- Modify: `crates/pipeline/templates/report/section.html:1-8` (attention parameter)
- Modify: `crates/pipeline/templates/report/base.html:68-73` (section include)
- Modify: `crates/pipeline/src/render/report.rs:1008-1030` (context data), `:1196` (group count), `:1328-1330` (template vars)
- Test: inline `mod tests` in `crates/pipeline/src/render/report.rs`

**Interfaces:**
- Consumes: `UnmanagedUsrEntry`, `UsrEntryKind` (Task 1).
- Produces: template variables `usr_entries`, `usr_count`, `usr_attention`, `has_usr_scan`; a `report-section--warning` treatment on the section when entries are present.

**Attention hook (resolved).** The `section()` macro's `state` parameter reflects collection completeness, not content, so it is not the hook. Generalize the macro's hardcoded `id == 'warnings'` case into an optional `attention` parameter and reuse the existing `.report-section--warning` CSS class. No new CSS, no new state vocabulary.

**Read-only surface.** The report renders the framing copy, the entry table (path, kind, files, size, included/excluded state), and the remediation guidance. A raw scan renders every row at the default include state, matching every other Actionable family; refine decisions appear once the report is rendered from a refined snapshot.

**Placement, and why the group gate goes away.** Own section under the existing `Software & Files` TOC group, after the Non-RPM Software content. The group is gated on `has_nonrpm or nonrpm_state == "failed"` at `base.html:68`.

That gate has to be removed, not widened. The /usr section has three states and **all three render something** (design note lines 115-131): not-scanned, image-clean, populated. The not-scanned state is precisely `unmanaged_files: None`, so any gate expressed in terms of /usr data — including `has_usr_scan`, which is `is_some()` — drops the section in exactly the case the spec requires it. And a host with no non-RPM software and no unmanaged scan hits both halves of the current gate, so the group disappears and takes the not-scanned state with it.

A section that always renders needs a group that always renders. Drop the `{% if %}` around the software group entirely, matching the ungated `secrets` group two blocks below it. `group_software_count` already accounts for an empty non-RPM side.

`has_usr_scan` stays, but only as the state selector **inside** the section template, where it correctly separates not-scanned from scanned-and-clean. It is not a gate.

- [ ] **Step 1: Write the failing tests**

Add to the inline `mod tests` in `crates/pipeline/src/render/report.rs`:

```rust
#[test]
fn report_renders_the_unmanaged_usr_section_with_framing_copy() {
    let snap = snapshot_with_usr_entries(vec![
        ("/usr/lib/custom-agent", 214, 39_845_888, UsrEntryKind::Directory, true),
        ("/usr/share/vendor-blob", 1, 4096, UsrEntryKind::File, false),
    ]);
    let html = render_report(&snap).unwrap();

    assert!(html.contains("Unmanaged /usr"), "section heading present");
    assert!(
        html.contains("/usr ships from the container image and stays read-only"),
        "framing copy leads with the blocker"
    );
    assert!(html.contains("/usr/lib/custom-agent"), "entry rows render");
    assert!(html.contains("214"), "file count renders");
}

#[test]
fn report_flags_the_usr_section_for_attention_when_entries_exist() {
    let snap = snapshot_with_usr_entries(vec![(
        "/usr/lib/custom-agent",
        1,
        10,
        UsrEntryKind::Directory,
        true,
    )]);
    let html = render_report(&snap).unwrap();
    let idx = html.find("unmanaged-usr").expect("section anchor");
    let window = &html[idx.saturating_sub(400)..idx];
    assert!(
        window.contains("report-section--warning"),
        "any entries at all get the attention treatment: {window}"
    );
}

#[test]
fn report_shows_the_image_clean_state_when_the_walk_found_nothing() {
    let mut snap = InspectionSnapshot::default();
    snap.unmanaged_files = Some(UnmanagedFileSection::default());
    let html = render_report(&snap).unwrap();
    assert!(html.contains("This host's /usr is image-clean"));

    // A clean section is not an attention state. Check the wrapper div
    // that immediately precedes the section anchor rather than the whole
    // document, since the warnings section legitimately carries the class.
    let idx = html.find("unmanaged-usr").expect("section anchor");
    let window = &html[idx.saturating_sub(400)..idx];
    assert!(
        !window.contains("report-section--warning"),
        "clean /usr must not be styled as a problem: {window}"
    );
}

#[test]
fn report_shows_the_not_scanned_state_when_the_walk_did_not_run() {
    // The bare default snapshot is the hard case: unmanaged_files is None
    // AND there is no non-RPM software, so any gate on either one hides
    // the section. The spec requires the not-scanned state here, so the
    // Software & Files group renders unconditionally.
    let html = render_report(&InspectionSnapshot::default()).unwrap();
    assert!(
        html.contains("Software &amp; Files") || html.contains("Software & Files"),
        "the group renders with no software content at all"
    );
    assert!(
        html.contains("collected without --include-unmanaged"),
        "not-scanned is a rendered state, not an absent section"
    );
    assert!(
        !html.contains("This host's /usr is image-clean"),
        "not scanned is not the same claim as clean"
    );
}

#[test]
fn warnings_section_keeps_its_warning_class() {
    // Regression guard: generalizing the macro's hardcoded id check must
    // not change the one caller that relied on it.
    let html = render_report(&snapshot_with_warnings()).unwrap();
    assert!(html.contains("report-section--warning"), "warnings still styled");
}
```

Write `snapshot_with_usr_entries` as a local helper and reuse whatever `render_report` entry point and warning fixture the surrounding tests already use. `crates/pipeline/src/render/report.rs:2646` has an existing warnings-class assertion; model `snapshot_with_warnings` on its fixture.

- [ ] **Step 2: Run to verify failure**

Run: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test -p inspectah-pipeline report_ -- usr`

Expected: FAIL on the missing heading.

- [ ] **Step 3: Generalize the section macro**

Change the first eight lines of `crates/pipeline/templates/report/section.html` to:

```jinja
{% macro section(id, title, count, state="normal",
                 conflict_count=0, extra_badge="", attention=false) %}
<div class="report-section
  {%- if state == 'failed' %} report-section--failed
  {%- elif state == 'degraded' %} report-section--degraded
  {%- elif id == 'warnings' or attention %} report-section--warning
  {%- endif %}">
```

Nothing else in the macro changes. Existing callers omit `attention` and keep their current behavior exactly.

- [ ] **Step 4: Write the section template**

Create `crates/pipeline/templates/report/unmanaged-usr.html`:

```jinja
{% from "report/section.html" import section %}

{% call section("unmanaged-usr", "Unmanaged /usr", count=usr_count,
                attention=usr_attention) %}

{% if not has_usr_scan %}
<p class="empty-state">This snapshot was collected without
--include-unmanaged, so /usr was not checked. Re-scan with
--include-unmanaged to check it.</p>
{% elif usr_count == 0 %}
<p class="empty-state">Every file under /usr on this host is owned by an
RPM package. This host's /usr is image-clean.</p>
{% else %}
<p>In image mode, /usr ships from the container image and stays read-only
at runtime. The files below live under /usr on this host but belong to no
RPM package, so a rebuilt image will not carry them unless you include
them in the export. For content that should be package-managed, building
an RPM that owns it is the durable fix; use include only for what
genuinely needs to travel with the image as-is.</p>

<table class="report-table">
  <thead>
    <tr>
      <th>Path</th>
      <th>Kind</th>
      <th>Files</th>
      <th>Size</th>
      <th>State</th>
    </tr>
  </thead>
  <tbody>
    {% for entry in usr_entries %}
    <tr>
      <td><code>{{ entry.path }}</code></td>
      <td>{{ entry.kind }}</td>
      <td>{{ entry.file_count }}</td>
      <td>{{ entry.size }}</td>
      <td>{{ entry.state }}</td>
    </tr>
    {% endfor %}
  </tbody>
</table>
{% endif %}

{% endcall %}
```

- [ ] **Step 5: Build the context and include the template**

In `crates/pipeline/src/render/report.rs`, after the `nonrpm_items` block at line 1008-1030:

```rust
    let has_usr_scan = snap.unmanaged_files.is_some();
    let mut usr_rows: Vec<&UnmanagedUsrEntry> = snap
        .unmanaged_files
        .as_ref()
        .map(|ufs| ufs.usr_entries.iter().collect())
        .unwrap_or_default();
    usr_rows.sort_by(|a, b| {
        b.total_size_bytes
            .cmp(&a.total_size_bytes)
            .then_with(|| a.path.cmp(&b.path))
    });
    let usr_count = usr_rows.len();
    // Any entries at all mean this estate is not image-clean, which is the
    // whole point of the section. An empty section gets the clean state.
    let usr_attention = usr_count > 0;
    let usr_entries: Vec<Value> = usr_rows
        .into_iter()
        .map(|e| {
            Value::from_serialize(serde_json::json!({
                "path": e.path,
                "kind": match e.kind {
                    UsrEntryKind::File => "File",
                    UsrEntryKind::Directory => "Directory",
                },
                "file_count": e.file_count,
                "size": format_size(e.total_size_bytes),
                "state": if e.disposition.is_included() { "Included" } else { "Excluded" },
            }))
        })
        .collect();
```

Add to the group count at line 1196: `let group_software_count = nonrpm_count + usr_count;`

Add to the template variable block at lines 1328-1330:

```rust
        usr_entries => Value::from(usr_entries),
        usr_count,
        usr_attention,
        has_usr_scan,
```

In `crates/pipeline/templates/report/base.html`, remove the group gate at line 68 and add the include after line 70. The `{% if %}` and its `{% endif %}` both go:

```jinja
    {% call group("software", "Software & Files", count=group_software_count) %}
      {% include "report/nonrpm.html" %}
      {% include "report/unmanaged-usr.html" %}
    {% endcall %}
```

`nonrpm.html` keeps whatever internal emptiness handling it already has; removing the outer gate must not change what that template renders when there is no non-RPM software. If it renders nothing for an empty section today, it still renders nothing. Check this before moving on, and if it turns out to depend on the outer gate for its own empty state, fix it inside `nonrpm.html` rather than restoring the gate.

Add `("Unmanaged /usr", "unmanaged-usr")` to the report's TOC list. The TOC in `report.rs:305-315` is keyed on `InspectorId`, and /usr has no inspector of its own. Add the entry to the flat TOC vector immediately after the `NonRpmSoftware` push rather than inventing an `InspectorId` variant: /usr is a section of the non-RPM inspector's output, not a new inspector, and adding a variant would ripple into `Completeness` for no gain. Push it with `count = usr_count` and `state = "normal"`, and push it **unconditionally** — the section always renders, so the TOC entry always points at something.

- [ ] **Step 6: Run tests, accept snapshot diffs, lint, commit**

Run: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test -p inspectah-pipeline && cargo fmt --check && cargo clippy --all-targets -- -D clippy::all`

Expected: PASS, zero warnings. Report `insta` snapshots gain the section; read each diff before accepting.

```bash
git add crates/pipeline/templates/report/unmanaged-usr.html \
        crates/pipeline/templates/report/section.html \
        crates/pipeline/templates/report/base.html \
        crates/pipeline/src/render/report.rs crates/pipeline/src/render/snapshots
git commit -m "feat(report): add the Unmanaged /usr section

Own section under Software & Files, after Non-RPM Software. Any entries
at all get the attention treatment, because content under /usr that no
package owns will not survive a rebuild and that is worth seeing. An
empty section gets the image-clean reading instead, and a host scanned
without --include-unmanaged says it was never checked.

Software & Files was gated on there being non-RPM software. The /usr
section has to render in all three of its states, including on a host
with no software content and no unmanaged scan at all, so the gate is
gone and the group renders like the secrets group does.

The section macro had a hardcoded id == 'warnings' check as its only
content-driven attention hook. Generalize it into an optional parameter
and reuse the existing class; the warnings caller is unchanged.

Assisted-by: Claude Code (Opus 5)"
```

---

### Task 13: CHANGELOG, user docs, and the stale schema skill

**Lane:** Tang (docs)

**Files:**
- Modify: `CHANGELOG.md` (`## [Unreleased]`)
- Modify: `docs/how-to/review-and-refine.md:72-79`
- Modify: `docs/reference/output-artifacts.md:70,150,228,256`
- Modify: `process-docs/skills/snapshot-schema-versioning.md`

**Interfaces:** none. Documentation only.

- [ ] **Step 1: Add the CHANGELOG entries**

Under `## [Unreleased]` in `CHANGELOG.md`, add an `### Added` block (create it if absent) following the existing `- **Label** — description` format used by the beta.1 and beta.2 entries:

```markdown
### Added
- **Unmanaged /usr section** — files and directories under `/usr` that no RPM owns now get their own section in refine, the HTML report, the Containerfile export, and the audit report. In image mode `/usr` ships from the image and is read-only at runtime, so this content vanishes on rebuild unless it travels with the image. Entries are ordinary findings with the standard include/exclude toggle, default-included, sorted largest first. Requires `--include-unmanaged` at scan time.
- **Unmanaged /usr in aggregate** — aggregate merge now preserves `/usr` entries with host prevalence instead of discarding them. Counts and sizes carry the maximum across hosts with an "up to" reading when hosts disagree.
- **Separate `/usr` bundling prompt** — scans with `--include-unmanaged` now ask about `/usr` content separately from `/opt`, `/srv`, and `/usr/local`, reporting its size first. Declining leaves the findings in the snapshot and marks the export's COPY lines as needing the content staged in the build context.
```

Add a `### Changed` entry for the schema:

```markdown
### Changed
- **Snapshot schema 23** — `/usr` entries now record whether they are a single file or a collapsed directory. That cannot be derived from older snapshots, so the accepted schema range narrows to 23 only. Existing snapshots and aggregates must be re-scanned; re-aggregating a fleet requires re-scanning every constituent host.
```

Add a `### Fixed` entry for the prompt bug:

```markdown
### Fixed
- **Declining unmanaged-file bundling no longer erases the findings** — answering "n" to the unmanaged-file tarball prompt cleared the whole section from the snapshot, discarding the catalog along with the bytes. It now drops the payload and keeps the findings.
```

- [ ] **Step 2: Update the user-facing docs**

In `docs/how-to/review-and-refine.md`, add a row to the section table at line 72:

```markdown
| Unmanaged /usr | Files and directories under `/usr` owned by no RPM package |
```

And extend the note at lines 78-79 so it covers both sections:

```markdown
only when `--include-unmanaged` was passed during the scan. Unmanaged /usr
follows the same gate: without it, the section reports that /usr was not
checked rather than that it is clean.
```

In `docs/reference/output-artifacts.md`, extend the `unmanaged/` row at line 70 to mention /usr, and update line 150's condition:

```markdown
| `unmanaged/` | `unmanaged_files` has included entries, or included `/usr` entries were bundled | Files from `/opt`, `/srv`, `/usr/local` not owned by RPM or language packages, plus `/usr` content owned by no RPM when bundling was accepted. Symlinks preserved as tar symlink entries. |
```

```markdown
| `unmanaged/` | Unmanaged files with `include: true` exist, or `/usr` bundling was accepted. |
```

Lines 228 and 256 are tree diagrams showing `unmanaged/ (conditional)`. They remain accurate; leave them.

- [ ] **Step 3: Correct the stale schema skill**

`process-docs/skills/snapshot-schema-versioning.md` states `MIN_SCHEMA == SCHEMA_VERSION` and describes exact-match gating. That was false at HEAD before this work (`MIN_SCHEMA = 21`, `SCHEMA_VERSION = 22`) and is true again after Task 1. Replace the code block and the two sentences following it with:

```markdown
`InspectionSnapshot` in `crates/core/src/snapshot.rs` carries a
`schema_version` field (currently 23). The loading contract is a
version range, and the range has been exactly one version wide at some
points and two at others:

```rust
const MIN_SCHEMA: u32 = 23; // equal to SCHEMA_VERSION as of v0.9.0-beta.3

if snap.schema_version < Self::MIN_SCHEMA || snap.schema_version > SCHEMA_VERSION {
    return Err(SnapshotError::UnsupportedVersion(snap.schema_version));
}
```

**Read both constants before assuming the window.** As of v0.9.0-beta.2
`MIN_SCHEMA` was 21 against a `SCHEMA_VERSION` of 22, so two versions
loaded. v0.9.0-beta.3 closed it back to one, because the /usr entry-kind
field cannot be derived from an older snapshot and any serde default
would silently mislabel rows. Widen the window only when every field
added since `MIN_SCHEMA` has a default that is correct rather than merely
present.

There is no migration path. Older snapshots must be re-scanned, and
re-aggregating a fleet requires re-scanning every constituent host.
```

Bump the `(currently 18)` reference to 23 wherever else it appears in that file.

- [ ] **Step 4: Verify and commit**

Run: `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test --workspace && cd crates/web/ui && npm test && npm run build`

Expected: full suite green, frontend green, `tsc` clean. This is the final gate for the feature.

```bash
git add CHANGELOG.md docs/how-to/review-and-refine.md \
        docs/reference/output-artifacts.md \
        process-docs/skills/snapshot-schema-versioning.md
git commit -m "docs: record the unmanaged /usr section and the schema narrowing

Also corrects the schema-versioning skill, which claimed exact-match
gating while the code accepted a two-version window. Future sessions
should read both constants rather than trusting the prose.

Assisted-by: Claude Code (Opus 5)"
```

---

## Open Choices Requiring Mark's Decision

No open choices remain. /usr content sourcing (Task 5) was the one decision recorded with a recommendation rather than settled; Mark bound it 2026-08-16 to the recommended option -- bundle at scan time with /usr as its own separately declinable line in the existing size prompt, plus a `--no-bundle-usr` programmatic override -- with rationale in Task 5's "Decided" and "Programmatic override" sections above.

The schema window was never an open choice. `SCHEMA_VERSION = 23` / `MIN_SCHEMA = 23` is settled; see § Schema Version Decision.

## Out of Scope for beta.3

Named here so nobody adds them mid-task. All are from the design note's future-improvement list.

- Subtree digests, change tracking for collapsed directories, and variant detection across hosts.
- Representative child samples per collapsed directory, and row drill-down.
- Entry notes.
- Per-host variance display beyond the "up to" reading, and sort controls.
- TUI parity. `crates/tui/src/app.rs` gets an exhaustive-match arm in Task 3 and nothing else.
- Revisiting the `--include-unmanaged` scan gate so the /usr walk always runs.
- Factor consumption of these decisions.
