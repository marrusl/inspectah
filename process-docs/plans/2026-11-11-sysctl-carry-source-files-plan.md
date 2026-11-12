# Sysctl: carry the admin's files, report runtime drift: implementation plan

> **Status: Proposed (2026-11-11).** Not executed.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking. Every task is test-first: write the tests it names, watch them fail for the stated reason, then implement.

**Goal:** sysctl files reach the image only as ordinary config entries, copied as the host had them; the collector models systemd-sysctl's precedence and reports runtime drift, vendor replacements and unknown inputs, read-only, in the audit, the HTML report and a System Tuning panel in refine.

**Architecture:** pure types, parser and resolution in `inspectah-core` (`types::sysctl`); a collector in the kernel/boot inspector that records files, links and runtime values and resolves nothing; a sysctl pass in `redact()`; one report model in `inspectah-pipeline` (`render::sysctl_report`) that the audit, the HTML report and the web DTO all project from; the per-key decision route (shadow pass, drop-in, `ItemId::Sysctl`, per-key merge) deleted.

**Tech stack:** Rust, serde, insta, MiniJinja, axum, React/TypeScript with Vitest. One new dev-dependency: `comrak` (GFM parser) in `inspectah-pipeline`, for spec case 19.

**Spec:** `process-docs/specs/proposed/2026-11-11-sysctl-carry-source-files.md` (at `207c6178`). The spec is the contract; section numbers below (§) and case numbers (case N) are the spec's.

**Verification convention:** every `cargo` command runs without a pipe, so the exit code is cargo's. The gate for each commit is the pre-commit hook's: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D clippy::all`, `cargo test --workspace`. Frontend tasks add `npx tsc --noEmit` and `npx vitest run` in `crates/web/ui`.

---

## Dependency

This plan builds on branch `fix/include-modified-rpm-configs` (`ef8ec686` and the commits after it): modified RPM configs are included at collection, a failed capture is a `ContentNotCaptured` advisory, and the snapshot schema is 26. It is being merged into `main` in parallel with this plan. **Task 1 verifies it has landed before any code changes.** This plan's schema bump is 26 to 27 (Task 11).

## Questions for Mark

1. **Case 5's "a mask line is reported".** In case 5 the mask is `/etc/sysctl.d/99-sysctl.conf -> /dev/null` and no `/usr/lib/sysctl.d/99-sysctl.conf` exists (systemd ships only the `/etc` link). § 5.3 defines `vendor_replacements()` as admin files that replace or mask a *host vendor file*, and § 5.5 reports masks only through that list, so the audit has no line for this mask. The refine view does show it, as a `files` row with `body.kind == "mask"`. **As written, the plan asserts the refine `files` row and asserts no vendor-replacement line for case 5** (Task 6, Task 9). If the audit should also report a mask that masks no vendor file, which § 5.5 item it belongs in and its wording are a spec decision. Case 15's mask-only host is written with a same-named vendor file under it (Task 4), which keeps that case within the § 5.3 definition.

## Global constraints

- `SCHEMA_VERSION` and `MIN_SCHEMA` both become `27`; only 27 loads (`process-docs/skills/snapshot-schema-versioning.md`).
- No sysctl key or value reaches any Containerfile or other instruction line inspectah writes. Sysctl files are copied verbatim by the config tree.
- The sysctl section owns no decision: no `disposition`, `locked` or `aggregate` field on any sysctl type, no op that targets it, no DTO field an op can target.
- Tests are built from collector-produced fields (§ 8): every case runs the real `KernelbootInspector` (and the real `ConfigInspector` where carriage is asserted) over a `MockExecutor`, through `collect()`, `validate`, `redact`, then render. A hand-built `SysctlSection` is allowed only in unit tests that pin the resolution rule (Task 3), and each has a collector-driven twin named beside it.
- Every interpolated value in the audit's sysctl section goes through the literal helper (§ 5.5, Task 7). The HTML report and refine never mark a sysctl field safe.
- Audit and panel copy is the spec's text, verbatim, held in named constants in `render::sysctl_report`.
- User-facing text (docs, CHANGELOG, audit copy beyond the spec's) follows the voice rules: no em dashes; not "shape"; not "immutable".
- Commits: `type(scope): description`, imperative; trailer `Assisted-by: <tool> (<model>)`; never a `Co-Authored-By:` trailer.

## Review focus

Inputs the spec implies but no § 8 case exercises, most likely first. Each has a test in the owning task.

1. **CRLF files and trailing whitespace.** A file written on Windows or with trailing spaces parses to the same key and value as the LF file; no false drift. Test in Task 2.
2. **Non-`.conf` entries in a sysctl directory** (`50-x.conf.rpmnew`, `50-x.conf~`, `README`): ignored, never read, and an unreadable one does not make the explanation unavailable. Test in Task 4.
3. **Link chains and loops.** A two-hop link ending at `/dev/null` is a mask; a two-hop chain ending under `/etc` is read through and records the final target; a link loop is a read failure (`sysctl == None`). Test in Task 4.
4. **Empty values.** `kernel.foo =` is an assignment of the empty string; against an empty runtime value it matches; the audit prints `(empty)`. Tests in Task 2 and Task 7.
5. **`sysctl -a` that exits 0 with per-key read errors on stderr** (as `net.ipv6.conf.*.stable_secret` produces) **and a stdout line with no `=`**: listed keys are used, an unlisted key is `NotExposed`, and the run is not a failure. Test in Task 4.

---

## File structure

| File | Responsibility |
|---|---|
| `crates/core/src/types/sysctl/mod.rs` (new) | Snapshot types: `SysctlDir`, `SysctlSection`, `SysctlFile`, `SysctlFileBody`, `SysctlKey`, refs; serde and wire checks |
| `crates/core/src/types/sysctl/parse.rs` (new) | The one parser: `SysctlLine`, `SysctlName`, key normalization, `sysctl_values_equal` |
| `crates/core/src/types/sysctl/resolve.rs` (new) | `effective()`, `vendor_replacements()`, `carriage()` and their result types |
| `crates/collect/src/inspectors/kernelboot.rs` | The new reader; the legacy `collect_sysctl_overrides` stays until Task 11 |
| `crates/collect/src/inspectors/config/mod.rs` | RPM-owned `/dev/null` link rule (Task 1); `MAX_CONFIG_FILE_SIZE` made `pub(crate)` (Task 4) |
| `crates/pipeline/src/redaction/engine.rs` | The sysctl pass (Task 5) |
| `crates/pipeline/src/render/sysctl_report.rs` (new) | The report model every surface projects from, and the spec's fixed copy |
| `crates/pipeline/src/render/md_literal.rs` (new) | The literal-text code-span helper |
| `crates/web/src/sysctl_view.rs` (new) | `project_sysctl`: report model to `SysctlViewDto` |
| `crates/cli/tests/support/sysctl_hosts.rs` (new) | Collector-driven host builder and scan-path harness, shared with `crates/web/tests` by `#[path]` |
| `crates/cli/tests/sysctl_scan_test.rs` (new, replaces `scan_shadow_test.rs`) | Spec cases 1 to 19 and 21, end to end |
| `crates/web/tests/system_tuning_fixtures_test.rs` (new) | Case 22's fixture writer and checker |
| `crates/web/ui/src/components/SysctlPanel.tsx` (new, replaces `SysctlSection.tsx`) | Read-only panel |

**Why one report model in the pipeline crate.** The audit (§ 5.5), the HTML report ("the same blocks as the audit") and the refine DTO (§ 5.6, which mirrors § 5.5 items 2 to 6) state the same facts. Deriving drift, the not-carried list, the as-written list and the carriage state once, in `render::sysctl_report`, keeps the three surfaces from disagreeing; each surface only formats. The web crate already depends on `inspectah-pipeline`. The alternative, deriving in each surface from the core resolution, triples the not-carried and as-written rules.

## Lanes

Each task names its lane so the implementer persona can be assigned per task.

- **Lane R (Rust workspace):** Tasks 1, 2, 3, 4, 5, 6, 7, 9, 11, 14.
- **Lane W (web frontend, report templates, Axum handlers):** Tasks 8, 10, 12, 13.

Task 9 deletes code in `handlers.rs` and `aggregate_handlers.rs` because removing `ItemId::Sysctl` does not compile otherwise; those edits are deletions only, and every addition to the handlers is Task 10's.

---

## Tasks

### Task 1: Dependency gate and the RPM-owned `/dev/null` link rule (Lane R)

**Files:**
- Modify: `crates/collect/src/inspectors/config/mod.rs` (the `rpm -Va` loop, after `is_authselect_pam_link`)
- Test: `crates/collect/tests/config_test.rs`

**Interfaces:**
- Produces: `fn is_rpm_owned_dev_null_link(exec: &dyn Executor, path: &str, flags: &str) -> bool` (private); a row it accepts is `ConfigFileKind::RpmOwnedModified`.

- [ ] **Step 0: Verify the dependency.** `git merge-base --is-ancestor ef8ec686 HEAD` exits 0, and `grep -n "pub const SCHEMA_VERSION: u32 = 26;" crates/core/src/snapshot.rs` matches. If either fails, stop: the branch has not landed.
- [ ] **Step 1: Write the failing tests** (table-driven, one `#[test]` iterating rows), each a `ConfigInspector` run over a `MockExecutor` whose `rpm -Va` text goes through the production parser, as `a_config_whose_content_was_not_captured_is_an_advisory` does:
  - `....L....  c /etc/sysctl.d/99-sysctl.conf`, the path a link to `/dev/null`: exactly one `config.files` entry at that path, `RpmOwnedModified`, included, `content == ""`, `rpm_va_flags == Some("....L....")`.
  - The same flags, the link pointing at `/etc/sysctl.conf`: no entry (as today).
  - A two-hop link (`a -> b -> /dev/null`) with `....L....`: one entry, as the first row.
  - A non-sysctl path (`/etc/systemd/logind.conf.d/x.conf -> /dev/null`, `....L....`): one entry, as the first row (the rule is for every `/etc` path).
  - `....L....` under `/etc/pam.d/` pointing into `/etc/authselect/`: no entry (the authselect guard still runs first).
  - `S.5....T.` on a regular file: unchanged `RpmOwnedModified` behavior.
- [ ] **Step 2: Run** `cargo test -p inspectah-collect --test config_test dev_null`. Expected: FAIL, the first, third and fourth rows find no entry.
- [ ] **Step 3: Implement.** In the loop, replace `let kind = classify_rpm_va_kind(flags);` with: `RpmOwnedModified` when `flags` contains `L` and `exec.resolve_final_target(path)` is `/dev/null`, else `classify_rpm_va_kind(flags)`. Content comes from `capture_config` as for any entry. The path stays in `rpm_va_paths`. Doc comment on the helper cites § 5.1 and RPM's `lib/verify.c` (`L` is the only column reported for a non-regular file).
- [ ] **Step 4: Run** the same command, then the full gate. Expected: PASS.
- [ ] **Step 5: Commit** `fix(collect): carry an RPM-owned config link re-pointed to /dev/null`.

Fixture note for this and every later collector-driven test: `MockExecutor::read_file` and `file_exists` do not follow links; `RealExecutor`'s do. A fixture that registers a link also registers, at the link's own path, the content the real executor would read there (`""` for `/dev/null`). Task 4's host builder does this for every link it is given.

### Task 2: Core types, parser and key normalization (Lane R)

**Files:**
- Create: `crates/core/src/types/sysctl/mod.rs`, `crates/core/src/types/sysctl/parse.rs`
- Modify: `crates/core/src/types/mod.rs` (`pub mod sysctl;`), `crates/core/src/types/kernelboot.rs` (add the field)
- Modify: every `KernelBootSection { .. }` literal the compiler names (add `sysctl: None`), and `merge_kernelboot_sections` in `crates/core/src/aggregate/merge.rs` (sets `sysctl: None`, § 5.7)
- Modify: `crates/collect/src/inspectors/kernelboot.rs` (the legacy collector calls the moved `sysctl_values_equal`)
- Test: `crates/core/src/types/sysctl/parse.rs` (`#[cfg(test)]`), `crates/core/tests/snapshot_test.rs`

**Interfaces (Produces):**

```rust
// mod.rs: types exactly as § 5.3 declares them, plus:
impl SysctlDir {
    pub const ALL: [SysctlDir; 4]; // Etc, Run, UsrLocalLib, UsrLib
    /// "/etc/sysctl.d", "/run/sysctl.d", "/usr/local/lib/sysctl.d", "/usr/lib/sysctl.d"
    pub fn path(self) -> &'static str;
}
pub struct SysctlFileRef { pub dir: SysctlDir, pub name: String }   // path(): dir joined with name
pub struct SysctlLineRef { pub dir: SysctlDir, pub name: String, pub line_no: u32 }
impl SysctlFile { pub fn path(&self) -> String; pub fn file_ref(&self) -> SysctlFileRef; pub fn lines(&self) -> Vec<SysctlLine>; }
impl SysctlKey {
    /// Normalizing constructor: the only way to build a key from text.
    pub fn normalize(name: &str) -> SysctlKey;
    pub fn as_str(&self) -> &str;
}
impl TryFrom<String> for SysctlKey { type Error = SysctlKeyError; } // accepts only canonical spellings
// parse.rs
pub fn parse_sysctl_lines(content: &str) -> Vec<SysctlLine>;
pub fn sysctl_values_equal(file_val: &str, runtime_val: &str) -> bool; // moved verbatim from kernelboot.rs, doc comment kept
```

Serde, pinned so every later task and the TypeScript mirror agree: `SysctlDir` snake_case (`etc`, `run`, `usr_local_lib`, `usr_lib`); `SysctlFileBody` internally tagged on `kind` (`mask`, `outside_link`, `read`), `Read.redacted` with `#[serde(default)]`; `SysctlKey` `#[serde(try_from = "String", into = "String")]`; `SysctlSection` deserializes through a private wire struct with `#[serde(try_from = ...)]` that rejects a key present in both `runtime` and `redacted_runtime`, naming it. `KernelBootSection.sysctl: Option<SysctlSection>` with `#[serde(default)]`. `SysctlLine`, `SysctlName` and the resolution types are not serialized.

Key normalization (§ 5.2): find the first `.` or `/` in the name; if it is `/`, swap every `.` and `/` in the name; otherwise keep the name. That is the dotted form `sysctl -a` prints.

- [ ] **Step 1: Write the failing tests.**
  - `normalize_spellings` (table): `net/ipv4/ip_forward` → `net.ipv4.ip_forward`; `net.ipv4.ip_forward` → same; `net/ipv4/conf/enp3s0.200/forwarding` → `net.ipv4.conf.enp3s0/200.forwarding`; `net.ipv4.conf.enp3s0/200.forwarding` → same. A normalizer that maps every `/` to `.` fails the third row.
  - `parse_lines` (table, each row content → `Vec<SysctlLine>`): blank, `#` and `;` lines skipped; `k = v` → `Assign{line_no, name: Key, value: "v", ignore_failure: false}`; `-net.ipv6.conf.all.disable_ipv6 = 1` → key without `-`, `ignore_failure: true` (case 9 unit); `-vm.swappiness` → `Exclude` (case 10 unit); `net.ipv4.conf.*.rp_filter = 2` → `Assign` with `Glob("net.ipv4.conf.*.rp_filter")` (case 14 unit); `-net.*.x` → `Exclude` with a glob; `<img src=x>` → `Unparsed` with the text; `= 1` (empty name) → `Unparsed`; `-` alone → `Unparsed`; line numbers are 1-based physical lines; the split is on the first `=` (`kernel.core_pattern = |/x a=b` keeps `|/x a=b`).
  - `crlf_lines_parse_like_lf` (Review focus 1): `"vm.swappiness = 10\r\n"` and `"vm.swappiness = 10   \n"` give the same line as `"vm.swappiness = 10\n"`.
  - `empty_value_is_an_assignment` (Review focus 4): `kernel.foo =` → `Assign` with `value == ""`; `sysctl_values_equal("", "")` is true.
  - `values_equal_ignores_whitespace_runs` (case 13 unit): `"32768 60999"` equals `"32768\t60999"`.
  - In `crates/core/tests/snapshot_test.rs`, `sysctl_key_on_the_wire` (case 20): a snapshot JSON whose `kernel_boot.sysctl.runtime` holds `net.ipv4.conf.enp3s0/200.forwarding` loads through `InspectionSnapshot::load` and serializes back with that exact key; one holding `net/ipv4/ip_forward`, and one holding `net/ipv4/conf/enp3s0.200/forwarding`, each fails with an error whose text contains that key; one with `vm.swappiness` in both `runtime` and `redacted_runtime` fails naming `vm.swappiness`.
  - `sysctl_file_body_serde`: each `SysctlFileBody` variant round-trips with the `kind` tags above.
- [ ] **Step 2: Run** `cargo test -p inspectah-core sysctl`. Expected: FAIL to compile (types absent).
- [ ] **Step 3: Implement** the types, parser and normalization; add the field; fix the literals the compiler names; move `sysctl_values_equal`.
- [ ] **Step 4: Run** `cargo test -p inspectah-core`, then the full gate. Expected: PASS; no other test changes.
- [ ] **Step 5: Commit** `feat(core): add sysctl section types and the sysctl.d parser`.

### Task 3: Core resolution (Lane R)

**Files:**
- Create: `crates/core/src/types/sysctl/resolve.rs`
- Test: same file, `#[cfg(test)]`

**Interfaces:**
- Consumes: Task 2's types and parser.
- Produces: `SysctlSection::effective`, `vendor_replacements`, `carriage` and `SysctlExplanation`, `UnknownInput`, `SysctlSetting`, `SysctlOutcome`, `RuntimeObservation`, `VendorReplacement`, `SysctlCarriage<'a>` exactly as § 5.3 declares them; plus the constant `pub const SYSCTL_CONF_LINK: &str = "/etc/sysctl.d/99-sysctl.conf";` and `pub const SYSCTL_CONF: &str = "/etc/sysctl.conf";`.

The algorithm is § 5.3's, in its order: same-name shadowing by smallest `SysctlDir` (masks and outside links shadow too); `Unknown` when any survivor is `OutsideLink` or `Read{redacted: true}` or `redacted_runtime` is non-empty, listing every such survivor (in `(name, dir)` order) then every redacted key, and nothing else computed; otherwise survivors sorted by name, each `Read` file's key `Assign`/`Exclude` lines applied in file then line order, last entry wins, earlier entries pushed to `overwritten` in apply order; `Writes` compared with `runtime` through `sysctl_values_equal`; globs take no part. `vendor_replacements()` pairs each `Etc` file with a same-named `UsrLib` file: `Masks` when the `Etc` body is `Mask`, else `Shadows`; sorted by name. `carriage()` covers `Etc` files only, in `(name, dir)` order: the config entry at the file's own path; else, for the file at `SYSCTL_CONF_LINK` whose `link_target` is `SYSCTL_CONF`, the entry at `SYSCTL_CONF`; else `NoConfigEntry`. `config == None` gives `NoConfigEntry` for every file.

- [ ] **Step 1: Write the failing tests.** Each unit test builds a `SysctlSection` directly to pin the rule, and its doc comment names its collector-driven twin in `sysctl_scan_test.rs` (Task 4, 5 or 6):
  - Priority across directories (twin: case 2), same-name shadow per adjacent pair (case 3), repeated assignment (case 8), exclusion both orders (case 10), `Cancelled.runtime` is the runtime value or `None`, mask shadows a vendor file (case 4), outside link survives → `Unknown` and outside link shadowed → `Resolved` (case 17), redacted file survives / shadowed (case 18), redacted runtime key → `Unknown` with no `RedactedFile` (case 18), glob adds no row (case 14), `NotExposed` when the key is absent from `runtime` (case 1).
  - `vendor_replacements`: shadow, mask, no pair for an `Etc` file without a `UsrLib` twin, no pair for a `Run` file over a `UsrLib` file.
  - `carriage`: own-path entry; `99-sysctl.conf` link to `/etc/sysctl.conf` → the `/etc/sysctl.conf` entry; a link to any other `/etc` file → `NoConfigEntry`; non-`Etc` files absent from the result; `config == None`.
- [ ] **Step 2: Run** `cargo test -p inspectah-core resolve`. Expected: FAIL to compile.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same, then the full gate. Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): resolve sysctl precedence the way systemd-sysctl does`.

### Task 4: Collector and the scan-path harness (Lane R)

**Files:**
- Modify: `crates/collect/src/inspectors/kernelboot.rs` (new `collect_sysctl_section`; failures accumulate)
- Modify: `crates/collect/src/inspectors/config/mod.rs` (`MAX_CONFIG_FILE_SIZE` becomes `pub(crate)`)
- Create: `crates/cli/tests/support/sysctl_hosts.rs`, `crates/cli/tests/sysctl_scan_test.rs`
- Test: `crates/collect/tests/kernelboot_test.rs` (Review focus 2, 3, 5)

**Interfaces:**
- Consumes: Task 2 types, `SysctlKey::normalize`, `parse_sysctl_lines`.
- Produces:

```rust
// kernelboot.rs (private)
fn collect_sysctl_section(exec: &dyn Executor) -> Result<SysctlSection, String>;
// support/sysctl_hosts.rs
pub struct SysctlHost { .. }                       // builder over MockExecutor
impl SysctlHost {
    pub fn new() -> Self;                           // rpm prerequisites, cmdline, lsmod, empty /etc
    pub fn file(self, path: &str, content: &str) -> Self;      // registers the dir entry too
    pub fn link(self, path: &str, target: &str) -> Self;       // also registers the content a real read of `path` returns
    pub fn dangling_link(self, path: &str, target: &str) -> Self;
    pub fn rpm_owned(self, path: &str, package: &str) -> Self;
    pub fn rpm_va(self, flags: &str, path: &str) -> Self;
    pub fn sysctl_a(self, stdout: &str) -> Self;
    pub fn sysctl_a_fails(self, exit_code: i32) -> Self;
    pub fn dir_error(self, dir: &str, kind: std::io::ErrorKind) -> Self;
    pub fn build(self) -> MockExecutor;
}
pub fn scan(host: SysctlHost) -> InspectionSnapshot;          // collect, validate, redact; Rpm, Config, Kernelboot inspectors
pub fn reload(snap: &InspectionSnapshot) -> InspectionSnapshot; // to_string then InspectionSnapshot::load path
pub fn render_export(snap: &InspectionSnapshot) -> tempfile::TempDir; // render::render_all
pub fn session(snap: &InspectionSnapshot) -> RefineSession;    // render_all, create_tarball, tarball::from_tarball
pub fn export_session(session: &RefineSession) -> tempfile::TempDir; // export_plan(..).render, unpacked
```

Collector rules (§ 5.2), in order, per directory in `SysctlDir::ALL`:
- `read_dir` `NotFound` → no files from that directory; any other error → `Err("sysctl: cannot list <dir>: <err>")`.
- Only names ending in `.conf`.
- `read_link` ok → `resolve_final_target`: error → `Err("sysctl: cannot read <path>: <err>")`; `/dev/null` → `Mask`; under `/etc/` or under one of the four directory paths → read `<path>` itself, `Read{link_target: Some(final)}`; anything else → `OutsideLink{target}`, not read.
- Regular file → read; over `MAX_CONFIG_FILE_SIZE` → `Err("sysctl: <path> is over <N> bytes")`; read error → `Err("sysctl: cannot read <path>: <err>")`.
- Files sorted by `(name, dir)`. When any `Read` file has a key `Assign` or `Exclude`, run `sysctl -a`; non-zero exit → `Err("sysctl: sysctl -a failed with exit code <n>")`; record the value of every key some key line names, keyed by `SysctlKey::normalize` of the `sysctl -a` name; a key absent from the output is absent from `runtime`. `redacted_runtime` starts empty.
- `inspect` sets `sysctl: Some(..)` on success and `None` on `Err`, and the reasons of every failed primary source are joined with `"; "` into the `Degraded` reason (today the last failure overwrites earlier ones, which can hide the sysctl cause).

The legacy `collect_sysctl_overrides` keeps running, unchanged, until Task 11 deletes it, so every existing test stays green while consumers move.

- [ ] **Step 1: Write the failing tests** in `sysctl_scan_test.rs`, each built with `SysctlHost`, asserting on `reload(&scan(host))` and on `sysctl.effective()`:
  - `case_01_each_directory_is_read`: one file per directory, a key unique to each; four `Writes` rows, each `from` its own file; `runtime` lists all four.
  - `case_02_priority_by_name_across_directories`: both layouts of case 2, with `overwritten` in apply order.
  - `case_03_same_name_shadow_each_adjacent_pair`: three layouts plus `/etc` over `/usr/lib`; `b` absent; `vendor_replacements()` holds `Shadows{name: "50-x.conf"}` only for the `/etc`/`/usr/lib` layout.
  - `case_07_key_spellings`: both layouts; the `Resolved` row's key and the serialized snapshot JSON's `kernel_boot.sysctl.runtime` key are the exact string `net.ipv4.conf.enp3s0/200.forwarding`.
  - `case_08_repeated_assignment_in_one_file`, `case_09_dash_prefixed_assignment`, `case_10_key_exclusion_both_orders` (outcome and runtime only; the audit half is Task 7), `case_13_whitespace_is_not_drift`, `case_14_glob_reported_not_expanded` (no row; the line is a `Glob` in `lines()`).
  - `case_15_mask_only_and_glob_only_snapshot`: host A: `/usr/lib/sysctl.d/50-x.conf` setting a key and `/etc/sysctl.d/50-x.conf -> /dev/null`, nothing else; host B: one `/etc` file holding only a glob line. Both `Resolved` with no settings; A's `vendor_replacements()` holds `Masks`. (The view and `/api/groups` halves are Tasks 9 and 10.)
  - `case_16_collection_failures_snapshot`: three hosts (`sysctl -a` exits 1; `/run/sysctl.d` fails with `PermissionDenied`; `/etc/sysctl.d/90-x.conf` dangling); each has `kernel_boot.sysctl == None`, a `KernelBoot` warning whose message contains the cause (`sysctl -a`, `/run/sysctl.d`, `/etc/sysctl.d/90-x.conf`), and the host's regular `/etc/sysctl.d` file still in `config.files`, included.
  - `case_17_outside_links_snapshot`: the four layouts; *canary* body is `OutsideLink{target: "/var/lib/app/credentials"}` and `canary-outside` is absent from the serialized snapshot; *overlapping assignment* and *overlapping exclusion* give `Unknown([OutsideLink{..}])`; *shadowed link* gives `Resolved` with `vm.swappiness` `Writes 60` `Matches`.
  - In `kernelboot_test.rs`: `non_conf_entries_are_ignored` (Review focus 2: `50-x.conf.rpmnew` and `README` registered with read errors; section is `Some`, neither in `files`); `link_chains_and_loops` (Review focus 3); `sysctl_a_stderr_noise_is_not_a_failure` (Review focus 5: exit 0, stderr `sysctl: reading key "net.ipv6.conf.all.stable_secret"`, a stdout line with no `=`; listed key `Matches`, unlisted key `NotExposed`).
- [ ] **Step 2: Run** `cargo test -p inspectah-cli --test sysctl_scan_test` and `cargo test -p inspectah-collect --test kernelboot_test`. Expected: FAIL (`sysctl` is always `None`).
- [ ] **Step 3: Implement** `collect_sysctl_section`, the failure accumulation, and the harness.
- [ ] **Step 4: Run** both, then the full gate. Expected: PASS; `scan_shadow_test.rs` still passes (the legacy route is untouched).
- [ ] **Step 5: Commit** `feat(collect): record sysctl files, links and runtime values for explanation`.

### Task 5: Redaction pass (Lane R)

**Files:**
- Modify: `crates/pipeline/src/redaction/engine.rs` (the kernel/boot block in `redact`, and the post-redaction content map)
- Modify: `crates/pipeline/src/render/mod.rs` (the two field-classification tests)
- Test: `crates/cli/tests/sysctl_scan_test.rs`

**Interfaces:**
- Consumes: Task 4's collector and harness; `scan_and_redact_field`, `scan_content`, the shared `CounterRegistry`.
- Produces: after `redact`, `Read.redacted` is true exactly when `content` changed; redacted runtime keys moved to `redacted_runtime` (sorted, deduplicated).

Per § 5.4's table: each `Read.content` through `scan_and_redact_field` under `file.path()`, `redacted = content != before`; `link_target` and `OutsideLink.target` through `scan_and_redact_field` under the entry's path, never setting `redacted`; each runtime value scanned as `format!("{key} = {value}")` under `/proc/sys`, and on any finding the value is removed from `runtime`, its key pushed to `redacted_runtime`, and the findings kept; names, dirs and keys are not scanned. All findings join `all_findings`. In the content map, each sysctl file's scrubbed content is inserted under its path with `entry().or_insert`, so a config entry's content keeps the slot.

- [ ] **Step 1: Write the failing tests.**
  - `case_18_redaction_makes_the_explanation_unknown_snapshot`: the six layouts of case 18, asserting on the reloaded snapshot: the exact `UnknownInput` list (or `Resolved` for *shadowed redacted file*), the canary absent from the serialized snapshot, `kernel.core_pattern` in `redacted_runtime` and not in `runtime` for *secret only at runtime*, and each layout's findings present in `snapshot.redactions` with the sysctl file's path or `/proc/sys`. *Secret in a file* also runs a twin host whose `sysctl -a` fails and asserts the `80-app.conf` config entry and the exported `config/etc/sysctl.d/80-app.conf` are byte-identical across the two scans. *Equal values under different keys*: the `vm.swappiness = 60` line is unchanged in the sysctl copy and the config entry, and the password line's token occurs only on that line in each copy. (Audit, HTML and refine halves: Tasks 7, 8, 9.)
  - In `render/mod.rs`: `every_field_of_a_content_bearing_type_is_classified` destructures `SysctlSection`, `SysctlFile` and every `SysctlFileBody` variant, each field commented scrubbed or exempt as § 5.4's table; `exactly_the_exempt_fields_survive_the_export` plants `SENTINEL-SYSCTL-CONTENT`, `SENTINEL-SYSCTL-LINK`, `SENTINEL-SYSCTL-OUTSIDE` and `SENTINEL-SYSCTL-RUNTIME` in the scrubbed list. (This test constructs the section to plant sentinels in fields; it does not stand for a host layout.)
- [ ] **Step 2: Run** `cargo test -p inspectah-cli --test sysctl_scan_test case_18` and `cargo test -p inspectah-pipeline exempt_fields`. Expected: FAIL (canaries survive; `redacted` false).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both, then the full gate. Expected: PASS.
- [ ] **Step 5: Commit** `feat(redaction): scrub the sysctl section and mark what it changed`.

### Task 6: Remove the second carriage route (Lane R)

**Files:**
- Modify: `crates/pipeline/src/collect.rs` (delete `shadow_sysctl_sources`, its call, and the paired early-return tests; keep the service pass and its tests)
- Modify: `crates/core/src/types/config.rs` (delete `ConfigShadow::Sysctl` and `SYNTHESIZED_SYSCTL_FILE`)
- Modify: `crates/pipeline/src/render/configtree.rs` (delete the drop-in writer and `sysctl_refusal`; keep `has_line_break_or_non_tab_control`)
- Modify: `crates/pipeline/src/render/containerfile.rs` (delete the `COPY sysctl/...` block and its tests)
- Modify: `crates/refine/src/session.rs` (`"sysctl"` out of `allowed_top_level`; the `export/sysctl/` assertions go)
- Modify: `crates/refine/src/projection/types.rs` (the `ConfigShadow::Sysctl` "merged into" arm), and every other site the compiler names, including `crates/refine/src/projection/reference.rs` and `crates/web/tests/contract_snapshots.rs` tests that set `ConfigShadow::Sysctl`
- Delete: `crates/cli/tests/scan_shadow_test.rs`; the sysctl cases in `crates/pipeline/tests/configtree_single_source_test.rs`; the sysctl row in `crates/pipeline/tests/instruction_injection_test.rs`
- Test: `crates/cli/tests/sysctl_scan_test.rs`

- [ ] **Step 1: Write the failing tests:**
  - `case_04_unowned_mask`: `/usr/lib/sysctl.d/50-coredump.conf` setting a key, `/etc/sysctl.d/50-coredump.conf -> /dev/null`. No key from the vendor file in `effective()`; `vendor_replacements() == [Masks{"50-coredump.conf"}]`; `render_export` holds a regular, empty `config/etc/sysctl.d/50-coredump.conf` (`symlink_metadata().is_file()`, length 0).
  - `case_05_rpm_owned_mask`: `/etc/sysctl.d/99-sysctl.conf -> /dev/null` RPM-owned with `rpm -Va` `....L....`, `/etc/sysctl.conf` setting `k`. `config.files` has exactly one entry at the link path, `RpmOwnedModified`, included, empty, flags `....L....`; the export holds an empty regular `config/etc/sysctl.d/99-sysctl.conf`; `k` absent from `effective()`; the file's `body` is `Mask`; `vendor_replacements()` is empty (Question 1). Twin: an `L`-only row whose link points at `/etc/sysctl.conf` produces no config entry.
  - `case_06_sysctl_conf_through_the_symlink` (export half): `99-sysctl.conf -> ../sysctl.conf`, `rpm -Va` `S.5....T.  c /etc/sysctl.conf`; keys attributed to `99-sysctl.conf` with `link_target == Some("/etc/sysctl.conf")`; `carriage()` names the `/etc/sysctl.conf` entry; it is included; the export holds `config/etc/sysctl.conf`. Then `SetInclude{Config{"/etc/sysctl.conf"}, false}` on `session(..)`: `export_session` lacks `etc/sysctl.conf` under the config tree. (The panel half is Task 9.)
  - `case_11_drifted_key_export`: export has no `sysctl/` directory, no file named `99-inspectah-migrated.conf` anywhere, and the Containerfile names neither.
  - `case_12_one_drifted_key_does_not_drop_the_file`: seven keys, one differs; the exported `config/etc/sysctl.d/90-tuning.conf` is byte-identical to the host content; the entry is included, `shadowed_by` absent, disposition kind `actionable`; exactly one `Differs` row.
- [ ] **Step 2: Run** `cargo test -p inspectah-cli --test sysctl_scan_test -- case_04 case_05 case_06 case_11 case_12`. Expected: FAIL (the shadow pass makes `90-tuning.conf` inventory; the drop-in exists).
- [ ] **Step 3: Delete** the route and the tests that encode it; fix every site the compiler names.
- [ ] **Step 4: Run** the full gate. Expected: PASS; `grep -rn "99-inspectah-migrated\|SYNTHESIZED_SYSCTL_FILE\|ConfigShadow::Sysctl\|shadow_sysctl_sources\|sysctl_refusal" crates --include='*.rs'` finds nothing.
- [ ] **Step 5: Commit** `refactor(pipeline)!: carry sysctl files only through the config tree`.

### Task 7: Report model, literal helper and the audit section (Lane R)

**Files:**
- Create: `crates/pipeline/src/render/sysctl_report.rs`, `crates/pipeline/src/render/md_literal.rs`
- Modify: `crates/pipeline/src/render/mod.rs` (modules), `crates/pipeline/src/render/audit.rs` (the Kernel & Boot block)
- Modify: `crates/pipeline/Cargo.toml` (`comrak` dev-dependency: the only GFM-conformant parser in Rust; case 19 requires one)
- Test: `md_literal.rs` and `sysctl_report.rs` (`#[cfg(test)]`), `crates/cli/tests/sysctl_scan_test.rs`

**Interfaces (Produces):**

```rust
// md_literal.rs
/// Inline code span per § 5.5 "Literal text"; `(empty)` for "".
pub fn md_code(value: &str) -> String;
/// As `md_code`, with `|` written `\|` for a GFM table cell.
pub fn md_code_cell(value: &str) -> String;

// sysctl_report.rs
pub enum SysctlReport { Aggregate, Unavailable { reason: String }, Host(SysctlHostReport) }
pub struct SysctlHostReport {
    pub explanation: SysctlExplanation,
    pub drift: Vec<SysctlDrift>,                 // Differs rows of Resolved, key order
    pub vendor_replacements: Vec<VendorLine>,
    pub not_carried: Vec<NotCarried>,
    pub as_written: Vec<AsWritten>,
    pub files: Vec<FileRow>,                     // every file, (name, dir) order
    pub tuned_active: Option<String>,            // kernel_boot.tuned_active when non-empty
}
pub struct SysctlDrift { pub key: SysctlKey, pub file_value: String, pub runtime: String, pub from: SysctlLineRef }
pub enum CarriageState { Carried { config_path: String }, Excluded { config_path: String }, Advisory { config_path: String }, NoConfigEntry }
pub struct VendorLine { pub action: VendorReplacement, pub carriage: CarriageState }
pub enum NotCarried { RuntimeDir { path: String }, UsrLocalLib { path: String }, Etc { path: String, carriage: CarriageState } } // Etc never Carried
pub enum AsWritten { GlobAssign { at: SysctlLineRef, pattern: String, value: String }, GlobExclude { at: SysctlLineRef, pattern: String }, Unparsed { at: SysctlLineRef, text: String } }
pub struct FileRow { pub file: SysctlFileRef, pub body: FileRowBody, pub config_path: Option<String> }
pub enum FileRowBody { Read { link_target: Option<String> }, Mask, OutsideLink { target: String } }
pub fn sysctl_report(snap: &InspectionSnapshot) -> SysctlReport;
/// `link_target` of the file a line ref names, for "(link to ..)" notes.
pub fn line_link_target(snap: &InspectionSnapshot, at: &SysctlLineRef) -> Option<String>;
// The spec's fixed copy, verbatim, one `pub const` each: TRUST_LINE, UNKNOWN_INTRO, DRIFT_INTRO, AGGREGATE_LINE, UNAVAILABLE_TAIL.
```

Rules: `Aggregate` when `aggregate_meta.is_some()`; `Unavailable` when `kernel_boot` or `sysctl` is `None`, `reason` = the messages of `snapshot.warnings` whose `inspector == "KernelBoot"`, joined `"; "`; `Host` otherwise. `CarriageState` from `carriage()`: included → `Carried`; advisory → `Advisory`; anything else → `Excluded`. `NotCarried`: every `Run` file, every `UsrLocalLib` file, every `Etc` file whose state is not `Carried`. `AsWritten`: glob lines and unparsed lines of every `Read` file with `redacted == false`, in `(name, dir, line)` order. `config_path` on a `FileRow` is set when `carriage()` names an entry.

Audit (§ 5.5): the Kernel & Boot block renders whenever the snapshot is single-host (and as today for the aggregate's cmdline), with `#### Sysctl` holding, in order: the aggregate line, or the unavailable line, or items 1 to 6. Wording is § 5.5's, each interpolation through `md_code` (table cells through `md_code_cell`). Table header `| Key | File value | Runtime value | Source |`. Source cell: `md_code_cell("<path>:<line>")`, then ` (link to ` + `md_code_cell(<target>)` + `)` when the file was read through a link. Fixed cell texts `not written`, `same`, `not exposed` are plain text. Item 4 uses "disables" in place of "replaces" for `Masks`. Item 5 reasons: `/run/sysctl.d` "generated at runtime"; `/usr/local/lib/sysctl.d` "carried only as unmanaged files under `--include-unmanaged`"; an `/etc` file its carriage state (excluded, advisory, or no Configuration Files row).

- [ ] **Step 1: Write the failing tests.**
  - `md_literal` (table): a value with a newline, U+2028 and a NUL becomes `\u{a}`, `\u{2028}`, `\u{0}` escapes; tab kept; three backticks get a four-backtick fence with one space padding; `|` becomes `\|` only in `md_code_cell`; backslashes not doubled; `""` → `(empty)` (Review focus 4).
  - `sysctl_report` unit tests over scanned snapshots from the harness are in `sysctl_scan_test.rs`, not here: the report reads collector output.
  - In `sysctl_scan_test.rs`, extend the case functions with audit assertions through `render_audit`:
    - case 10: the `Cancelled` row's File value cell is `not written`, no drift line; reversed order: a drift line against `60`.
    - case 11: the drift line `` `vm.swappiness` runs at `60`; the files request `10` (`/etc/sysctl.d/..:1`). ``, `DRIFT_INTRO` once, no sentence containing "after a reboot" other than `DRIFT_INTRO`'s own; with `tuned_active` set, the tuned sentence.
    - case 16: each host's audit holds the unavailable line with its cause, no `#### Sysctl` table, no drift line, and no `not exposed`.
    - case 17 (overlapping assignment and exclusion): `UNKNOWN_INTRO`, the outside-link line naming `/etc/sysctl.d/90-site.conf` and its target, no table, no drift line, and no line containing `vm.swappiness` together with `10` or `60`; canary: `canary-outside` absent.
    - case 18 (all layouts but the last): `UNKNOWN_INTRO` with one line per input; no table or drift; canary absent; *one-line PEM*: "As written" holds no line from `90-app.conf`.
    - `case_19_literal_rendering`: the case 19 host (pipe `kernel.core_pattern`, unparsed `<img src=x>`, `[link](http://example.com)` and a triple-backtick run, a file name holding a newline). Parse the audit with `comrak` (GFM table extension on): the sysctl table's rows each have four cells; each cell's code node literal equals the value exactly; the `#### Sysctl` section has no HTML, link or image node.
- [ ] **Step 2: Run** `cargo test -p inspectah-pipeline md_literal` and `cargo test -p inspectah-cli --test sysctl_scan_test`. Expected: FAIL.
- [ ] **Step 3: Implement** the helper, the model and the audit block; delete the `#### Sysctl Overrides` table.
- [ ] **Step 4: Run** both, then the full gate (accept the audit insta snapshots that change, after reading each diff). Expected: PASS.
- [ ] **Step 5: Commit** `feat(render): explain sysctl in the audit report with literal values`.

### Task 8: HTML report (Lane W)

**Files:**
- Modify: `crates/pipeline/src/render/report.rs` (the kernel/boot context, `section_count` for `InspectorId::KernelBoot`)
- Modify: `crates/pipeline/templates/report/kernel.html`
- Test: `crates/pipeline/src/render/report.rs` tests (delete the `SysctlOverride` ones), `crates/cli/tests/sysctl_scan_test.rs`

**Interfaces:**
- Consumes: `sysctl_report`, its copy constants, `line_link_target` (Task 7).

The template renders the same blocks as the audit from a context built from `SysctlReport`, every field through MiniJinja autoescaping; no `|safe` on any sysctl field. `section_count(KernelBoot)` is the number of `Resolved` settings, `0` for `Unknown`, `Unavailable` or `Aggregate`.

- [ ] **Step 1: Write the failing tests** (extend the cases through `render_report`):
  - case 17 *overlapping assignment* and every case 18 layout but the last: `section_count` shown for Kernel & Boot is `0`. Case 17 *canary* and every case 18 layout: the layout's canary is absent from the HTML.
  - case 19: the HTML holds `&lt;img src=x&gt;`, `[link](http://example.com)` as text, the pipe value, and no `<img` element.
  - case 12: `section_count` equals the number of `Resolved` rows.
  - `kernel.html` grep test: no `|safe` in the sysctl block.
- [ ] **Step 2: Run** `cargo test -p inspectah-cli --test sysctl_scan_test html` and `cargo test -p inspectah-pipeline report`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both, then the full gate. Expected: PASS.
- [ ] **Step 5: Commit** `feat(report): show the sysctl explanation in the HTML report`.

### Task 9: Refine decision removal and the read-only view (Lane R)

**Files:**
- Modify: `crates/refine/src/types.rs` (delete `ItemId::Sysctl`, `RefinedSysctl`, the three `TriageReason::Sysctl*`), `crates/refine/src/classify.rs` (delete `classify_sysctls`, `SYSCTL_DENY_LIST`, `is_file_backed`, their tests), `crates/refine/src/session.rs` (every `ItemId::Sysctl` arm and `sysctl_overrides` loop), `crates/refine/src/projection/{types,decisions,reference}.rs` (delete `DecisionProjection.sysctls`, `RefSysctlOverride`, `RefKernelBoot.sysctl_overrides`)
- Modify: `crates/web/src/web_types.rs` (delete `SysctlDecisionDto` and `ViewResponse.sysctls`; add § 5.6's DTOs and `ViewResponse.sysctl`), `crates/web/src/adapter.rs` (delete the sysctl rows in the Kernel and Boot context list; call `project_sysctl`), `crates/web/src/aggregate_handlers.rs` (delete the sysctl block and `build_sysctl_variants`; add `AggregateViewResponse.sysctl`), `crates/web/src/handlers.rs` (delete the sysctl bulk-op loop)
- Create: `crates/web/src/sysctl_view.rs`
- Test: `crates/cli/tests/sysctl_scan_test.rs`; `crates/web/tests/contract_snapshots.rs` (its fixture sets `sysctl: None`; delete `shadowed_sysctl_keys_decide_and_the_shadowing_file_does_not`)

**Interfaces:**
- Consumes: Task 7's `sysctl_report`.
- Produces: `pub fn project_sysctl(snap: &InspectionSnapshot) -> SysctlViewDto` in `sysctl_view.rs`; the DTOs exactly as § 5.6, `Serialize` only. `build_web_view` calls it with `session.snapshot_projected()`; `aggregate_view` likewise for `AggregateViewResponse.sysctl`. Mapping is one-to-one from the report model: `SysctlExplanationDto::Resolved.drift` from `SysctlHostReport.drift`, `SysctlLineRefDto.link_target` from `line_link_target`, `CarriageDto` from `CarriageState`, `NotCarriedReason::{RuntimeDir, UsrLocalLib}` from the `Run`/`UsrLocalLib` variants and `Excluded`/`Advisory`/`NoConfigEntry` from the `/etc` carriage.

- [ ] **Step 1: Write the failing tests** (extend the cases, serializing the view of `session(..)` with `serde_json::to_value(build_web_view(&session))`):
  - case 6 (panel half): after excluding `/etc/sysctl.conf`, `sysctl.not_carried` holds `{path: "/etc/sysctl.d/99-sysctl.conf", reason: {kind: "excluded", config_path: "/etc/sysctl.conf"}}`; before, `vendor_replacements` is empty and the file row's `config_path` is `/etc/sysctl.conf`.
  - case 5: the `files` row for `/etc/sysctl.d/99-sysctl.conf` has `body.kind == "mask"`.
  - case 15: both hosts' `sysctl.state == "host"`, `explanation.state == "resolved"`, empty `settings`; host A's `vendor_replacements[0].action == "masks"`; host B's `as_written[0].kind == "glob_assign"`.
  - case 16: `sysctl == {state: "unavailable", reason: <contains the cause>}`.
  - case 17 (overlapping): `explanation == {state: "unknown", inputs: [{kind: "outside_link", path, target}]}`, no settings or drift anywhere in `sysctl`; canary absent from the whole view JSON.
  - case 18: the view's `explanation.inputs` match the snapshot's; canary absent from the view JSON.
  - `no_sysctl_item_id`: `ViewResponse` JSON holds no key `sysctls`; `RefinementOp` JSON with `kind: "Sysctl"` fails to deserialize.
  - case 12 (refine half): `SetInclude{Config{"/etc/sysctl.d/90-tuning.conf"}, false}` succeeds (no `InventoryNotToggleable`).
- [ ] **Step 2: Run** `cargo test -p inspectah-cli --test sysctl_scan_test view`. Expected: FAIL to compile (`sysctl` field absent).
- [ ] **Step 3: Implement** the removals and the view. Update the contract insta snapshots in `crates/web/tests/snapshots/` with `cargo insta review`, reading each diff: the only changes are `sysctls` gone and `sysctl` present.
- [ ] **Step 4: Run** the full gate. Expected: PASS; `grep -rn "ItemId::Sysctl\|RefinedSysctl\|classify_sysctls\|SysctlDecisionDto\|RefSysctlOverride" crates --include='*.rs'` finds nothing.
- [ ] **Step 5: Commit** `refactor(refine)!: make sysctl a read-only explanation, not a decision`.

### Task 10: System Tuning section registry and handlers (Lane W)

**Files:**
- Modify: `crates/pipeline/src/section_group.rs` (`system_tuning` off `RETIRED_SECTION_IDS`; `SectionMeta { id: "system_tuning", label: "System Tuning", is_triage: true }` after `kernel_boot`; `for_section("system_tuning")` → `SystemConfig`; flip the retirement test)
- Modify: `crates/web/src/handlers.rs` (batch toggle: the tuned op moves under `section_only` absent or `"system_tuning"`; flip the unknown-slug list)
- Modify: `crates/web/src/aggregate_handlers.rs` (the tuned block always pushes `build_section("system_tuning", "System Tuning", true, &items, ctx)`; tests that look up `"tuned"` look up `"system_tuning"`)
- Test: `crates/web/tests/api_test.rs`, `crates/web/tests/aggregate_api_test.rs`, `crates/cli/tests/sysctl_scan_test.rs`

- [ ] **Step 1: Write the failing tests:**
  - `get_groups` lists `system_tuning` in `system_config`, after `kernel_boot`, `is_triage: true`.
  - Batch toggle with `section_only: "system_tuning"` produces exactly the tuned op; with `"kernel_boot"` it produces none for tuned; with no filter it still reaches the tuned row.
  - Aggregate: a session with no tuned selection has a `system_tuning` section with zero items and no `tuned` or `sysctls` section; with a tuned selection the tuned item is under `system_tuning`.
  - case 15 (groups half): for both hosts, `get_groups` lists `system_tuning`.
- [ ] **Step 2: Run** `cargo test -p inspectah-web` and `cargo test -p inspectah-pipeline section_group`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the full gate. Expected: PASS.
- [ ] **Step 5: Commit** `feat(web): restore System Tuning as a section`.

### Task 11: Aggregate merge, `SysctlOverride` deletion and schema 27 (Lane R)

**Files:**
- Modify: `crates/core/src/aggregate/merge.rs` (delete the `sysctl_overrides` merge and `impl AggregateMergeable for SysctlOverride`)
- Modify: `crates/core/src/types/kernelboot.rs` (delete `SysctlOverride` and `KernelBootSection.sysctl_overrides`, and their tests)
- Modify: `crates/collect/src/inspectors/kernelboot.rs` (delete `collect_sysctl_overrides`, `read_sysctl_files`, `parse_sysctl_conf`; keep `parse_sysctl_output` for the new reader)
- Modify: `crates/core/src/snapshot.rs` (`SCHEMA_VERSION` and `MIN_SCHEMA` to `27`), and the version literals and `testdata/e2e-fixture.tar.gz` per `snapshot-schema-versioning.md` § What a Bump Touches
- Delete: the `SysctlOverride` cases in `crates/core/tests/aggregate_merge_test.rs` and `aggregate_e2e_test.rs`, and any remaining site the compiler names
- Test: `crates/cli/tests/sysctl_scan_test.rs`, `crates/core/tests/snapshot_test.rs`, `crates/refine/tests/tarball_test.rs`, `crates/cli/src/commands/aggregate.rs` tests

- [ ] **Step 1: Write the failing tests:**
  - `case_21_aggregate`: two hosts (`SysctlHost`), different content at `/etc/sysctl.d/90-x.conf`, no tuned selection; `merge_snapshots` of the two reloaded snapshots gives two content variants for that path and `kernel_boot.sysctl == None`; `render_audit` holds `AGGREGATE_LINE`; a session on the merged snapshot gives `AggregateViewResponse.sysctl == {state: "aggregate"}` and `sections` holding `system_tuning` with no items and no `sysctls` or `tuned` section.
  - Schema 26 refused through all three loaders (`InspectionSnapshot::load`, `tarball::from_tarball`, `load_snapshot_from_tarball`), with a payload holding the old `sysctl_overrides` array and no `sysctl`: the reason it is refused is semantic staleness (it predates the precedence model and its files may have been shadowed).
  - Snapshot JSON of a scanned host has no key `sysctl_overrides` under `kernel_boot`.
- [ ] **Step 2: Run** `cargo test -p inspectah-cli --test sysctl_scan_test case_21` and `cargo test -p inspectah-core snapshot`. Expected: FAIL.
- [ ] **Step 3: Implement** and update the skill file's version history paragraph (26 to 27, semantic staleness).
- [ ] **Step 4: Run** the full gate. Expected: PASS; `grep -rn "SysctlOverride\|sysctl_overrides" crates --include='*.rs'` finds nothing.
- [ ] **Step 5: Commit** `feat(core)!: raise the snapshot schema to 27 for the sysctl section`.

### Task 12: Case 22 fixtures from the real handlers (Lane W)

**Files:**
- Create: `crates/web/tests/system_tuning_fixtures_test.rs` (includes `#[path = "../../cli/tests/support/sysctl_hosts.rs"] mod sysctl_hosts;`)
- Modify: `crates/web/Cargo.toml` (dev-dependency `inspectah-collect`, needed to build hosts with `MockExecutor`)
- Create: `crates/web/ui/src/__tests__/fixtures/system-tuning/*.json`

The test builds five sessions (case 15 host A and B, case 16 `sysctl -a` failure, case 17 overlapping assignment, case 21 aggregate, and a host with sysctl files and no tuned selection), calls `get_groups`, `get_view` and `aggregate_view` through the router as `contract_snapshots.rs` does, and compares each body with `<session>.<endpoint>.json`. With `UPDATE_SYSTEM_TUNING_FIXTURES=1` it writes them instead. Health fixtures with and without `aggregate` are written beside them.

- [ ] **Step 1: Write the test** with no fixtures committed. **Run** `cargo test -p inspectah-web --test system_tuning_fixtures_test`. Expected: FAIL, fixture file missing.
- [ ] **Step 2: Run** with `UPDATE_SYSTEM_TUNING_FIXTURES=1`, read every written JSON (states as cases 15, 16, 17, 21 require), then run without the variable. Expected: PASS.
- [ ] **Step 3: Commit** `test(web): record System Tuning fixtures from the real handlers`.

### Task 13: Frontend (Lane W)

**Files:**
- Modify: `crates/web/ui/src/api/types.ts` (mirror § 5.6's DTOs as discriminated unions on `state`/`kind` with the same snake_case strings; `ViewResponse.sysctl`, `AggregateViewResponse.sysctl`; delete `SysctlDecisionDto`, `ViewResponse.sysctls`, `ItemIdSysctl` and its `ItemId` member, the three `sysctl_*` triage reasons, `"sysctl"` from `ConfigShadow`)
- Create: `crates/web/ui/src/components/SysctlPanel.tsx`; Delete: `SysctlSection.tsx`, `shadowOwner.ts`'s sysctl branch (and the file if nothing else remains), their tests
- Modify: `App.tsx` (`system_tuning` off `RETIRED`), `MainContent.tsx` (render `SystemTuningSection` for `system_tuning`; drop the sysctl argument to the shadow owner), `Sidebar.tsx` (`decisionCount` arm for `system_tuning` = tuned row count), `SystemTuningSection.tsx` (takes `sysctl: SysctlViewDto`; tuned rows and `SysctlPanel` independent; the § 5.6 empty-state rule), `components/AggregateApp.tsx` (`<SysctlPanel sysctl={aggregateView.sysctl} />` after `AggregateSectionContent` when `activeSection === "system_tuning"`), `components/aggregate/AggregateItemRow.tsx` (delete the `"Sysctl"` arm), `attentionUtils.ts` (delete the three sysctl labels), every `sysctl:` mutation id
- Test: `crates/web/ui/src/__tests__/SystemTuning.navigation.test.tsx` (new), `SysctlPanel.test.tsx` (new); rewrite `Sidebar.test.tsx:215`'s retirement assertion and `SystemTuningSection.test.tsx`

`SysctlPanel` renders all three view states and both explanation states with the audit's wording (Task 7 constants mirrored as TypeScript constants), every value as React text, every `config_path` as a link to its Configuration Files row; it holds no checkbox, switch, toggle or op-issuing button.

- [ ] **Step 1: Write the failing tests.**
  - `SystemTuning.navigation.test.tsx` (case 22): mount `App` with the API client returning Task 12's fixtures, single-host mode and aggregate mode; click the sidebar entry by accessible name "System Tuning"; assert the content is visible (not a redirect or blank), the panel's state (empty settings table for the zero-settings hosts, the unavailable line, the unknown text naming `/etc/sysctl.d/90-site.conf`, the aggregate line), the badge (`0` single-host, `0 / 0` aggregate), and `within(panel).queryAllByRole("checkbox" | "switch" | "button")` holds nothing that issues an op.
  - `SysctlPanel.test.tsx`: values with `<img src=x>` render as text; a `config_path` renders a link to that row.
  - A source search test: no file under `crates/web/ui/src` holds `"Sysctl"` as an `ItemId` kind (`grep -rn 'kind: "Sysctl"'` empty).
- [ ] **Step 2: Run** `cd crates/web/ui && npx vitest run src/__tests__/SystemTuning.navigation.test.tsx`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `npx tsc --noEmit`, `npx vitest run`, `npm run build`, then the Rust gate. Expected: PASS.
- [ ] **Step 5: Commit** `feat(ui): read-only sysctl panel in System Tuning`.

### Task 14: Docs, skills and CHANGELOG (Lane R)

**Files:** every row of § 6's table, plus `process-docs/skills/codebase-layout.md` (new modules `core::types::sysctl`, `render::sysctl_report`, `render::md_literal`, `web::sysctl_view`, `SysctlPanel`), and `CHANGELOG.md`.

- [ ] **Step 1:** For each § 6 row, make the change it names, at the symbols cited (line numbers are a convenience). `redaction-owns-the-snapshot-object.md` gains the rule: content parsed after redaction is unknown when the pass changed it, and nothing is parsed from scrubbed text. `aggregate-vs-single-host-behavioral-split.md`: replace the worked example with the System Tuning aggregate route (a section that exists in both modes with mode-specific content) or drop it.
- [ ] **Step 2:** Add the `[Unreleased]` entry § 6 specifies, under `### Changed` (and `### Removed` for `99-inspectah-migrated.conf` and the per-key toggle), covering every clause § 6 lists, ending with the re-scan note for schema 27.
- [ ] **Step 3: Verify** `grep -rn "99-inspectah-migrated\|Merged into sysctl\|SysctlOverride\|sysctl_overrides" docs process-docs/skills` finds nothing outside the version-history paragraph of `snapshot-schema-versioning.md`; the new CHANGELOG lines hold no em dash (U+2014); the full gate passes.
- [ ] **Step 4: Commit** `docs: describe sysctl carriage as config files and drift as a report`.

Backlog dispositions (§ 9) are recorded outside this repository after the branch merges.

---

## Coverage

### Spec requirements

| Spec | Requirement | Task |
|---|---|---|
| § Dependency | modified RPM configs included at collect | 1 (verified) |
| § 5.1 | carriage table rows: unowned file, unowned mask, RPM-owned mask, `/etc/sysctl.conf`, modified package file, vendor/run not copied | 1, 6 |
| § 5.1 | `L` + `/dev/null` rule for every `/etc` path; path stays in `rpm_va_paths` | 1 |
| § 5.1 | other re-pointed RPM links not carried; reported "Not carried" | 1 (twin), 7 (`NoConfigEntry`) |
| § 5.1 | removals: shadow pass, `ConfigShadow::Sysctl`, `SYNTHESIZED_SYSCTL_FILE`, writer, `COPY`, `sysctl/` dir, `sysctl_refusal` | 6 |
| § 5.2 | four directories, `.conf` only, link rules, outside-link allowlist, size limit | 4 |
| § 5.2 | failures make `sysctl` `None`, degraded with the cause | 4 |
| § 5.2 | parsing, exclusions, key normalization | 2 |
| § 5.2 | runtime: run when any key line exists, normalized keys, absent recorded as absent | 4 |
| § 5.3 | types, content stored not lines, schema bump | 2, 11 |
| § 5.3 | `SysctlKey` wire check and both-fields check | 2 |
| § 5.3 | `effective`, `vendor_replacements`, `carriage`; `sysctl_values_equal` moves to core | 3, 2 |
| § 5.4 | sysctl pass, table of fields, `redacted`, runtime removal, findings, content map | 5 |
| § 5.4 | field-classification tests | 5 |
| § 5.5 | audit items 1 to 6, unavailable and aggregate lines | 7 |
| § 5.5 | literal helper | 7 |
| § 5.5 | HTML report blocks, autoescape, `section_count` | 8 |
| § 5.6 | registry, badge counts tuned only, batch toggle, retirement tests flip | 10, 13 |
| § 5.6 | read-only DTOs, `project_sysctl`, `ViewResponse.sysctl`, `AggregateViewResponse.sysctl` | 9 |
| § 5.6 | empty-state rule; single-host frontend; aggregate frontend | 13 |
| § 5.6 | removals (Rust) | 9 |
| § 5.6 | removals (TypeScript) | 13 |
| § 5.6 | aggregate `system_tuning` section always pushed | 10 |
| § 5.6 | `Sysctl` config rows toggleable | 6, 9 (case 12) |
| § 5.7 | `sysctl: None` on aggregate | 2 |
| § 5.7 | per-key merge removed; impl removed | 11 |
| § 5.7 | aggregate audit and panel line | 7, 13 |
| § 6 | stale docs and skills | 14 |
| § 6 | CHANGELOG | 14 |
| § 8 | tests encoding removed behavior deleted or rewritten | 6, 9, 10, 11, 13 |

### Spec test cases

| Case | Task(s) | Test |
|---|---|---|
| 1 | 4 | `case_01_each_directory_is_read` |
| 2 | 4 | `case_02_priority_by_name_across_directories` |
| 3 | 4 | `case_03_same_name_shadow_each_adjacent_pair` |
| 4 | 6 | `case_04_unowned_mask` |
| 5 | 1, 6, 9 | config rule; `case_05_rpm_owned_mask`; view `files` row |
| 6 | 6, 9 | `case_06_sysctl_conf_through_the_symlink` (export, panel) |
| 7 | 2, 4 | `normalize_spellings`; `case_07_key_spellings` |
| 8 | 4 | `case_08_repeated_assignment_in_one_file` |
| 9 | 2, 4 | `parse_lines`; `case_09_dash_prefixed_assignment` |
| 10 | 4, 7 | outcome; audit `not written` and reversed drift |
| 11 | 6, 7 | export absence; audit drift line and paragraph |
| 12 | 6, 8, 9 | byte-identical export; `section_count`; toggleable |
| 13 | 2, 4 | `values_equal_ignores_whitespace_runs`; `case_13_whitespace_is_not_drift` |
| 14 | 2, 4, 7 | parser; no row; audit "As written" |
| 15 | 4, 9, 10, 12, 13 | snapshot; view; `/api/groups`; fixtures; navigation |
| 16 | 4, 7, 9, 12, 13 | snapshot; audit; view; fixtures; navigation |
| 17 | 4, 7, 8, 9, 12, 13 | snapshot; audit; HTML; view; fixtures; navigation |
| 18 | 5, 7, 8, 9 | snapshot; audit; HTML; view |
| 19 | 7, 8 | `case_19_literal_rendering` (comrak); HTML escaping |
| 20 | 2 | `sysctl_key_on_the_wire` |
| 21 | 10, 11, 12, 13 | sections; `case_21_aggregate`; fixtures; navigation |
| 22 | 12, 13 | fixture writer; `SystemTuning.navigation.test.tsx` |

**Collector-driven fixtures.** Every case above runs through `SysctlHost` and the scan path; the case 22 fixtures come from sessions built the same way (Task 12). The only hand-built sysctl values are Task 3's rule pins (each naming its twin) and Task 5's sentinel planting.

## Risks

- **Interim commits.** Between Task 9 and Task 13 the running refine UI reads `sysctls` that the backend no longer sends; the branch is not releasable until Task 13 lands. The legacy collector runs beside the new one until Task 11, so a `sysctl -a` failure is reported twice in the degraded reason in those commits.
- **`MockExecutor` link semantics** differ from the real executor's (fixture note under Task 1). A fixture that omits the link-path content tests a read failure, not the layout it names; `SysctlHost::link` exists so no test registers links by hand.
