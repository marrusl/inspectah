# Sysctl: carry the admin's files, report runtime drift: implementation plan

> **Status: Proposed (2026-11-11).** Not executed.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking. Every task is test-first: write the tests it names, watch them fail for the stated reason, then implement.

**Goal:** sysctl files reach the image only as ordinary config entries, copied as the host had them; the collector models systemd-sysctl's precedence and reports runtime drift, vendor replacements and unknown inputs, read-only, in the audit, the HTML report and a System Tuning panel in refine.

**Architecture:** pure types, parser and resolution in `inspectah-core` (`types::sysctl`); a collector in the kernel/boot inspector that records files, links and runtime values and resolves nothing; a sysctl pass in `redact()`; one report model in `inspectah-pipeline` (`render::sysctl_report`) that the audit, the HTML report and the web DTO all project from; the per-key decision route (shadow pass, drop-in, `ItemId::Sysctl`, per-key merge) deleted.

**Tech stack:** Rust, serde, insta, MiniJinja, axum, React/TypeScript with Vitest. One new dev-dependency: `comrak` (GFM parser) in `inspectah-cli`, whose `sysctl_scan_test.rs` parses the audit for spec case 19.

**Spec:** `process-docs/specs/proposed/2026-11-11-sysctl-carry-source-files.md` (at `22956315`). The spec is the contract; section numbers below (§) and case numbers (case N) are the spec's.

**Verification convention:** every `cargo` command runs without a pipe, so the exit code is cargo's. The gate for each commit is the pre-commit hook's: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D clippy::all`, `cargo test --workspace`. Frontend tasks add `npx tsc --noEmit` and `npx vitest run` in `crates/web/ui`.

---

## Dependency

This plan builds on branch `fix/include-modified-rpm-configs` (`ef8ec686` and the commits after it): modified RPM configs are included at collection, a failed capture is a `ContentNotCaptured` advisory, and the snapshot schema is 26. It is being merged into `main` in parallel with this plan. **Task 1 verifies it has landed before any code changes.** This plan's schema bump is 26 to 27 (Task 11).

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
| `crates/collect/src/inspectors/kernelboot.rs` | The new reader (Task 4); the legacy `collect_sysctl_overrides` is deleted in Task 5 |
| `crates/collect/src/inspectors/config/mod.rs` | RPM-owned `/dev/null` link rule (Task 1); `MAX_CONFIG_FILE_SIZE` made `pub(crate)` (Task 4) |
| `crates/pipeline/src/redaction/engine.rs` | The sysctl pass (Task 6) |
| `crates/pipeline/src/render/sysctl_report.rs` (new) | The report model every surface projects from, and the spec's fixed copy |
| `crates/pipeline/src/render/md_literal.rs` (new) | The literal-text code-span helper |
| `crates/web/src/sysctl_view.rs` (new) | `project_sysctl`: report model to `SysctlViewDto` |
| `crates/cli/tests/support/sysctl_hosts.rs` (new) | Collector-driven host builder and scan-path harness, shared with `crates/web/tests` by `#[path]` |
| `crates/cli/tests/sysctl_scan_test.rs` (new, replaces `scan_shadow_test.rs`) | Spec cases 1 to 21, end to end. Each case has one function per surface, named `case_NN_<layout>` for the snapshot and export, then `_audit`, `_html`, `_view` and `_groups` for the surfaces, so each task's red run filters to its own functions |
| `crates/web/tests/system_tuning_fixtures_test.rs` (new) | Case 22's fixture writer and checker |
| `crates/web/ui/src/components/SysctlPanel.tsx` (new, replaces `SysctlSection.tsx`) | Read-only panel |

**Why one report model in the pipeline crate.** The audit (§ 5.5), the HTML report ("the same blocks as the audit") and the refine DTO (§ 5.6, which mirrors § 5.5 items 2 to 6) state the same facts. Deriving drift, the not-carried list, the as-written list and the carriage state once, in `render::sysctl_report`, keeps the three surfaces from disagreeing; each surface only formats. The web crate already depends on `inspectah-pipeline`. The alternative, deriving in each surface from the core resolution, triples the not-carried and as-written rules.

## Lanes

Each task names its lane so the implementer persona can be assigned per task.

Fifteen tasks.

- **Lane R (Rust workspace):** Tasks 1, 2, 3, 4, 5, 6, 7, 9, 11, 14, 15. Task 15 runs on real hosts and needs the VM operation skill supplied at dispatch time.
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
  - In `crates/core/tests/snapshot_test.rs`, `sysctl_key_on_the_wire` (case 20 wire unit; the case's collector-driven run is Task 4's `case_20_key_on_the_wire`): a snapshot JSON whose `kernel_boot.sysctl.runtime` holds `net.ipv4.conf.enp3s0/200.forwarding` loads through `InspectionSnapshot::load` and serializes back with that exact key; one holding `net/ipv4/ip_forward`, and one holding `net/ipv4/conf/enp3s0.200/forwarding`, each fails with an error whose text contains that key; one with `vm.swappiness` in both `runtime` and `redacted_runtime` fails naming `vm.swappiness`.
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

- [ ] **Step 1: Write the failing tests.** Each unit test builds a `SysctlSection` directly to pin the rule, and its doc comment names its collector-driven twin in `sysctl_scan_test.rs` (written in Task 4, 5 or 6), as listed here:
  - Priority across directories (twin: case 2), same-name shadow per adjacent pair (case 3), repeated assignment (case 8), exclusion both orders (case 10), `Cancelled.runtime` is the runtime value (case 10) or `None` (case 10's unlisted-key layout), mask shadows a vendor file (case 4), outside link survives → `Unknown` and outside link shadowed → `Resolved` (case 17), redacted file survives / shadowed (case 18), redacted runtime key → `Unknown` with no `RedactedFile` (case 18), glob adds no row (case 14), `NotExposed` when the key is absent from `runtime` (case 1's unlisted-key layout).
  - `vendor_replacements`: shadow (case 3's `/etc` over `/usr/lib` layout), mask (case 4), no pair for an `Etc` file without a `UsrLib` twin (case 5), no pair for a `Run` file over a `UsrLib` file (case 3's `/run` over `/usr/lib` layout).
  - `carriage`: own-path entry (case 12); `99-sysctl.conf` link to `/etc/sysctl.conf` → the `/etc/sysctl.conf` entry (case 6); a link to any other `/etc` file → `NoConfigEntry` (case 5's twin layout); non-`Etc` files absent from the result (case 1); `config == None` (`config_section_absent_from_a_scan`).
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
pub fn scan_without_config(host: SysctlHost) -> InspectionSnapshot; // the same path with Rpm and Kernelboot only, so `config == None`
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

The legacy `collect_sysctl_overrides` keeps running, unchanged, until Task 5 retires it with the carriage route it feeds, so every existing test stays green in this task.

- [ ] **Step 1: Write the failing tests** in `sysctl_scan_test.rs`, each built with `SysctlHost`, asserting on `reload(&scan(host))` and on `sysctl.effective()`:
  - `case_01_each_directory_is_read`: one file per directory, a key unique to each; four `Writes` rows, each `from` its own file; `runtime` lists all four; no `Run`, `UsrLocalLib` or `UsrLib` file appears in `carriage()`. Unlisted-key layout: the same four files with `sysctl -a` omitting the `/run` file's key; that row's outcome is `NotExposed`, the other three `Matches`, and the key is absent from the serialized `kernel_boot.sysctl.runtime`.
  - `case_02_priority_by_name_across_directories`: both layouts of case 2, with `overwritten` in apply order.
  - `case_03_same_name_shadow_each_adjacent_pair`: three layouts plus `/etc` over `/usr/lib` and `/run` over `/usr/lib`; `b` absent; `vendor_replacements()` holds `Shadows{name: "50-x.conf"}` for the `/etc`/`/usr/lib` layout and is empty for every other layout.
  - `case_07_key_spellings`: both layouts; the `Resolved` row's key and the serialized snapshot JSON's `kernel_boot.sysctl.runtime` key are the exact string `net.ipv4.conf.enp3s0/200.forwarding`.
  - `case_08_repeated_assignment_in_one_file`, `case_09_dash_prefixed_assignment`, `case_13_whitespace_is_not_drift`, `case_14_glob_reported_not_expanded` (no row; the line is a `Glob` in `lines()`).
  - `case_10_key_exclusion_both_orders` (outcome and runtime): both orders against runtime `60`, `Cancelled{runtime: Some("60")}` and `Writes 10` `Differs`; unlisted-key layout, the exclusion order with `sysctl -a` not listing `vm.swappiness`: `Cancelled{runtime: None}`.
  - `case_20_key_on_the_wire`: the case 7 host with `net/ipv4/conf/enp3s0.200/forwarding = 1` in its file. Serialize the scanned snapshot: `kernel_boot.sysctl.runtime` holds the key `net.ipv4.conf.enp3s0/200.forwarding`, and `InspectionSnapshot::load` of that text serializes back with the exact key. Then edit the serialized JSON: renaming that runtime key to `net/ipv4/ip_forward`, and in a second copy to `net/ipv4/conf/enp3s0.200/forwarding`, each fails `InspectionSnapshot::load` with an error whose text contains the renamed key; adding the key to `redacted_runtime` while it stays in `runtime` fails naming it.
  - `config_section_absent_from_a_scan`: `scan_without_config` of a host with one `/etc/sysctl.d` file; `config` is `None` and `carriage()` gives `NoConfigEntry` for that file.
  - `case_15_mask_only_and_glob_only_snapshot`: host A: `/usr/lib/sysctl.d/50-x.conf` setting a key and `/etc/sysctl.d/50-x.conf -> /dev/null`, nothing else; host B: one `/etc` file holding only a glob line. Both `Resolved` with no settings; A's `vendor_replacements()` holds `Masks`. (The view and `/api/groups` halves are Tasks 9 and 10.)
  - `case_16_collection_failures_snapshot`: three hosts (`sysctl -a` exits 1; `/run/sysctl.d` fails with `PermissionDenied`; `/etc/sysctl.d/90-x.conf` dangling); each has `kernel_boot.sysctl == None`, a `KernelBoot` warning whose message contains the cause (`sysctl -a`, `/run/sysctl.d`, `/etc/sysctl.d/90-x.conf`), and the host's regular `/etc/sysctl.d` file still in `config.files`, included.
  - `case_17_outside_links_snapshot`: the four layouts; *canary* body is `OutsideLink{target: "/var/lib/app/credentials"}` and `canary-outside` is absent from the serialized snapshot; *overlapping assignment* and *overlapping exclusion* give `Unknown([OutsideLink{..}])`; *shadowed link* gives `Resolved` with `vm.swappiness` `Writes 60` `Matches`.
  - In `kernelboot_test.rs`: `non_conf_entries_are_ignored` (Review focus 2: `50-x.conf.rpmnew` and `README` registered with read errors; section is `Some`, neither in `files`); `link_chains_and_loops` (Review focus 3); `sysctl_a_stderr_noise_is_not_a_failure` (Review focus 5: exit 0, stderr `sysctl: reading key "net.ipv6.conf.all.stable_secret"`, a stdout line with no `=`; listed key `Matches`, unlisted key `NotExposed`).
- [ ] **Step 2: Run** `cargo test -p inspectah-cli --test sysctl_scan_test` and `cargo test -p inspectah-collect --test kernelboot_test`. Expected: FAIL (`sysctl` is always `None`).
- [ ] **Step 3: Implement** `collect_sysctl_section`, the failure accumulation, and the harness.
- [ ] **Step 4: Run** both, then the full gate. Expected: PASS; `scan_shadow_test.rs` still passes (the legacy route is untouched).
- [ ] **Step 5: Commit** `feat(collect): record sysctl files, links and runtime values for explanation`.

### Task 5: Remove the second carriage route and retire the legacy capture (Lane R)

The legacy capture (`collect_sysctl_overrides`) retires in this task, not with its type in Task 11. Its only carriage consumers are the shadow pass, the drop-in writer and the `COPY sysctl/` block this task deletes, so once they go it carries nothing. It also has to stop before Task 6's redaction pass: that pass scrubs the new section only, and while the capture runs, the same host values sit unscrubbed in `kernel_boot.sysctl_overrides` (`runtime` and `default`), so Task 6's whole-snapshot canary assertions could not pass; and the shadow pass that capture drives turns a drifting secret-bearing file into inventory, so Task 6's carriage comparison against a failed-runtime twin could not pass either. The `SysctlOverride` type and the `KernelBootSection.sysctl_overrides` field stay, always empty from a scan, until Task 11: removing the field changes the wire format, which is the schema bump, and the surfaces that read it (the audit table, the HTML block, the refine decisions, the aggregate merge) leave in Tasks 7, 8, 9 and 11 beside their replacements.

**Files:**
- Modify: `crates/collect/src/inspectors/kernelboot.rs` (delete `collect_sysctl_overrides`, `read_sysctl_files`, `parse_sysctl_conf` and the unit test that calls `collect_sysctl_overrides`; `inspect` sets `sysctl_overrides: Vec::new()`; keep `parse_sysctl_output` for the new reader)
- Modify: `crates/collect/tests/kernelboot_test.rs` (delete `sysctl_three_way_diff`, whose behavior cases 11 to 13 now own; the fixture test asserts `sysctl.is_some()` where it asserted a non-empty override list)
- Modify: `crates/pipeline/src/collect.rs` (delete `shadow_sysctl_sources`, its call, and the paired early-return tests; keep the service pass and its tests)
- Modify: `crates/core/src/types/config.rs` (delete `ConfigShadow::Sysctl` and `SYNTHESIZED_SYSCTL_FILE`)
- Modify: `crates/pipeline/src/render/configtree.rs` (delete the drop-in writer and `sysctl_refusal`; keep `has_line_break_or_non_tab_control`)
- Modify: `crates/pipeline/src/render/containerfile.rs` (delete the `COPY sysctl/...` block and its tests)
- Modify: `crates/refine/src/session.rs` (`"sysctl"` out of `allowed_top_level`; the `export/sysctl/` and `99-inspectah-migrated.conf` assertions in its tests go)
- Modify: `crates/refine/src/projection/types.rs` (the `ConfigShadow::Sysctl` "merged into" arm), and every other site the compiler names, including `crates/refine/src/projection/reference.rs` and `crates/web/tests/contract_snapshots.rs`'s `shadowed_sysctl_keys_decide_and_the_shadowing_file_does_not`, which sets `ConfigShadow::Sysctl` and is deleted
- Modify: the tests that name the drop-in by a string path, which the compiler does not flag: `crates/refine/tests/session_test.rs` (delete `shadowed_sysctl_snapshot` and `the_sysctl_key_behind_a_shadowed_file_stays_operable`) and `crates/web/tests/decision_surface_test.rs` (delete `SYNTHESIZED_SYSCTL_PATH` and the `ItemId::Sysctl` row whose export checks read it)
- Modify: `crates/web/tests/snapshots/contract_snapshots__contract_view.snap` through `cargo insta review`, reading the diff: the only change is `containerfile_preview` losing the sysctl override comment and the `COPY sysctl/...` line
- Delete: `crates/cli/tests/scan_shadow_test.rs`; the sysctl cases in `crates/pipeline/tests/configtree_single_source_test.rs`; the sysctl row in `crates/pipeline/tests/instruction_injection_test.rs`
- Test: `crates/cli/tests/sysctl_scan_test.rs`

- [ ] **Step 1: Write the tests:**
  - `legacy_sysctl_overrides_not_captured`: the case 11 host (`vm.swappiness = 10` in a file, runtime `60`); the serialized snapshot's `kernel_boot.sysctl_overrides` is `[]` while `kernel_boot.sysctl.runtime` holds `vm.swappiness`. Task 11 rewrites this test to assert the key is absent.
  - `case_04_unowned_mask`: `/usr/lib/sysctl.d/50-coredump.conf` setting a key, `/etc/sysctl.d/50-coredump.conf -> /dev/null`. No key from the vendor file in `effective()`; `vendor_replacements() == [Masks{"50-coredump.conf"}]`; `render_export` holds a regular, empty `config/etc/sysctl.d/50-coredump.conf` (`symlink_metadata().is_file()`, length 0).
  - `case_05_rpm_owned_mask`: `/etc/sysctl.d/99-sysctl.conf -> /dev/null` RPM-owned with `rpm -Va` `....L....`, `/etc/sysctl.conf` setting `k`. `config.files` has exactly one entry at the link path, `RpmOwnedModified`, included, empty, flags `....L....`; the export holds an empty regular `config/etc/sysctl.d/99-sysctl.conf`; `k` absent from `effective()`; the file's `body` is `Mask`; `vendor_replacements()` is empty: no `/usr/lib/sysctl.d/99-sysctl.conf` exists, so the mask appears in refine's file list only and the audit adds no vendor-replacement line for it (spec case 5, Mark, 2026-11-11). Twin: an `L`-only row for `/etc/sysctl.d/99-sysctl.conf` whose link points at another `/etc` file (`/etc/sysctl-site.conf`) produces no config entry at the link path, and `carriage()` gives `NoConfigEntry` for that file.
  - `case_06_sysctl_conf_through_the_symlink` (export half): `99-sysctl.conf -> ../sysctl.conf`, `rpm -Va` `S.5....T.  c /etc/sysctl.conf`; keys attributed to `99-sysctl.conf` with `link_target == Some("/etc/sysctl.conf")`; `carriage()` names the `/etc/sysctl.conf` entry; it is included; the export holds `config/etc/sysctl.conf`. Then `SetInclude{Config{"/etc/sysctl.conf"}, false}` on `session(..)`: `export_session` lacks `etc/sysctl.conf` under the config tree. (The panel half is Task 9.)
  - `case_11_drifted_key_export`: export has no `sysctl/` directory, no file named `99-inspectah-migrated.conf` anywhere, and the Containerfile names neither.
  - `case_12_one_drifted_key_does_not_drop_the_file`: `/etc/sysctl.d/90-tuning.conf` sets seven keys, `sysctl -a` differs on one; the exported `config/etc/sysctl.d/90-tuning.conf` is byte-identical to the host content; the entry is included, `shadowed_by` absent, disposition kind `actionable`; `carriage()` names the file's own entry; exactly one `Differs` row.
- [ ] **Step 2: Run** `cargo test -p inspectah-cli --test sysctl_scan_test -- legacy_sysctl_overrides case_04 case_05 case_06 case_11 case_12`. Expected: `legacy_sysctl_overrides_not_captured` FAILS (the capture still fills the field), `case_11` FAILS (the drop-in and `sysctl/` exist) and `case_12` FAILS (the shadow pass makes `90-tuning.conf` inventory). `case_04`, `case_05` and `case_06` already pass on Tasks 1, 3 and 4 (the unowned walk carries a `/dev/null` link as an empty file, Task 1 carries the RPM-owned one, and neither route shadows these layouts); they are written here as the end-to-end owners of their cases and must stay green through the deletion.
- [ ] **Step 3: Delete** the capture, the route and the tests that encode them; fix every site the compiler names and the string-path sites listed above.
- [ ] **Step 4: Run** the full gate. Expected: PASS. Removal checks: `grep -rn "SYNTHESIZED_SYSCTL_FILE\|SYNTHESIZED_SYSCTL_PATH\|ConfigShadow::Sysctl\|shadow_sysctl_sources\|sysctl_refusal\|collect_sysctl_overrides\|read_sysctl_files\|parse_sysctl_conf" crates --include='*.rs'` finds nothing; `grep -rln "99-inspectah-migrated" crates --include='*.rs' --include='*.snap'` prints only `crates/cli/tests/sysctl_scan_test.rs`, and every match there is one of `case_11_drifted_key_export`'s absence assertions. (The frontend's mentions go in Task 13.)
- [ ] **Step 5: Commit** `refactor(pipeline)!: carry sysctl files only through the config tree`.

### Task 6: Redaction pass (Lane R)

**Files:**
- Modify: `crates/pipeline/src/redaction/engine.rs` (the kernel/boot block in `redact`, and the post-redaction content map)
- Modify: `crates/pipeline/src/render/mod.rs` (the two field-classification tests)
- Test: `crates/cli/tests/sysctl_scan_test.rs`

**Interfaces:**
- Consumes: Task 4's collector and harness; Task 5's removal of the shadow pass and the legacy capture, which leaves the new section the only place a scan stores sysctl values; `scan_and_redact_field`, `scan_content`, the shared `CounterRegistry`.
- Produces: after `redact`, `Read.redacted` is true exactly when `content` changed; redacted runtime keys moved to `redacted_runtime` (sorted, deduplicated).

Per § 5.4's table: each `Read.content` through `scan_and_redact_field` under `file.path()`, `redacted = content != before`; `link_target` and `OutsideLink.target` through `scan_and_redact_field` under the entry's path, never setting `redacted`; each runtime value scanned as `format!("{key} = {value}")` under `/proc/sys`, and on any finding the value is removed from `runtime`, its key pushed to `redacted_runtime`, and the findings kept; names, dirs and keys are not scanned. All findings join `all_findings`. In the content map, each sysctl file's scrubbed content is inserted under its path with `entry().or_insert`, so a config entry's content keeps the slot.

- [ ] **Step 1: Write the failing tests.**
  - `case_18_redaction_makes_the_explanation_unknown_snapshot`: the six layouts of case 18, asserting on the reloaded snapshot: the exact `UnknownInput` list for the first five; for *shadowed redacted file*, `Resolved` with one row, `vm.swappiness` `Writes 60` from `/etc/sysctl.d/50-x.conf`, `Matches`; the canary absent from the whole serialized snapshot; `kernel.core_pattern` in `redacted_runtime` and not in `runtime` for *secret only at runtime*; and each layout's findings present in `snapshot.redactions` with the sysctl file's path or `/proc/sys`. *Secret in a file* also runs a twin host whose `sysctl -a` fails and asserts the `80-app.conf` config entry and the exported `config/etc/sysctl.d/80-app.conf` are byte-identical across the two scans. *Equal values under different keys*: the `vm.swappiness = 60` line is unchanged in the sysctl copy and the config entry, and the password line's token occurs only on that line in each copy. (Audit, HTML and refine halves: Tasks 7, 8, 9.)
  - In `render/mod.rs`: `every_field_of_a_content_bearing_type_is_classified` destructures `SysctlSection`, `SysctlFile` and every `SysctlFileBody` variant, each field commented scrubbed or exempt as § 5.4's table; `exactly_the_exempt_fields_survive_the_export` plants `SENTINEL-SYSCTL-CONTENT`, `SENTINEL-SYSCTL-LINK`, `SENTINEL-SYSCTL-OUTSIDE` and `SENTINEL-SYSCTL-RUNTIME` in the scrubbed list. (This test constructs the section to plant sentinels in fields; it does not stand for a host layout.)
- [ ] **Step 2: Run** `cargo test -p inspectah-cli --test sysctl_scan_test case_18` and `cargo test -p inspectah-pipeline exempt_fields`. Expected: FAIL (canaries survive; `redacted` false). The classification test is a compile-time exhaustiveness guard, not a red test: it compiles and passes once its comments are written.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both, then the full gate. Expected: PASS.
- [ ] **Step 5: Commit** `feat(redaction): scrub the sysctl section and mark what it changed`.

### Task 7: Report model, literal helper and the audit section (Lane R)

**Files:**
- Create: `crates/pipeline/src/render/sysctl_report.rs`, `crates/pipeline/src/render/md_literal.rs`
- Modify: `crates/pipeline/src/render/mod.rs` (modules), `crates/pipeline/src/render/audit.rs` (the Kernel & Boot block)
- Modify: `crates/cli/Cargo.toml` (`comrak` under `[dev-dependencies]`, because `case_19_literal_rendering_audit` in the CLI's `sysctl_scan_test.rs` parses with it; a dev-dependency of `inspectah-pipeline` is not visible to another crate's tests. `comrak` is the only GFM-conformant parser in Rust; case 19 requires one)
- Test: `md_literal.rs` (`#[cfg(test)]`), `crates/cli/tests/sysctl_scan_test.rs`

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
  - In `sysctl_scan_test.rs`, one `_audit` function per case, each scanning the case's host or hosts and asserting on `render_audit`. A *drift line* below is a line starting with a code span and containing ` runs at `.
    - `case_01_each_directory_is_read_audit`: four table rows, each Source cell its own file; unlisted-key layout: the `/run` key's Runtime value cell is `not exposed`.
    - `case_10_key_exclusion_both_orders_audit`: the `Cancelled` row's File value cell is `not written`, no drift line; reversed order: one drift line for `vm.swappiness` against `60`.
    - `case_11_drifted_key_audit`: the drift line `` `vm.swappiness` runs at `60`; the files request `10` (`/etc/sysctl.d/..:1`). ``, `DRIFT_INTRO` once, no sentence containing "after a reboot" other than `DRIFT_INTRO`'s own; with `tuned_active` set, the tuned sentence.
    - `case_12_one_drifted_key_does_not_drop_the_file_audit`: seven table rows; exactly one drift line, naming the differing key with both values and source `/etc/sysctl.d/90-tuning.conf:<line>`; `DRIFT_INTRO` once.
    - `case_13_whitespace_is_not_drift_audit`: the `net.ipv4.ip_local_port_range` row's Runtime value cell is `same`; no drift line and no `DRIFT_INTRO`.
    - `case_14_glob_reported_not_expanded_audit`: the "As written" item holds the glob assignment with `net.ipv4.conf.*.rp_filter`, its value `2`, and `<path>:<line>` of the line that holds it; no table row has a key starting `net.ipv4.conf.`.
    - `case_16_collection_failures_audit`: each host's audit holds the unavailable line with its cause, no `#### Sysctl` table, no drift line, and no `not exposed`.
    - `case_17_outside_links_audit`: *overlapping assignment* and *overlapping exclusion* each give `UNKNOWN_INTRO`, the outside-link line naming `/etc/sysctl.d/90-site.conf` and its target, no table, no drift line, and no line containing `vm.swappiness` together with `10` or `60`; *canary*: `canary-outside` absent from the whole audit; *shadowed link*: no `UNKNOWN_INTRO`, one table row, `vm.swappiness` with File value `60`, Runtime value `same` and Source `/etc/sysctl.d/90-site.conf:1`, no drift line.
    - `case_18_redaction_makes_the_explanation_unknown_audit`: every layout but the last gives `UNKNOWN_INTRO` with exactly one line per input of that layout (the redacted-file line naming the file, or the runtime line naming `kernel.core_pattern`), no table and no drift line, and the layout's canary absent from the whole audit; *one-line PEM*: "As written" holds no line from `90-app.conf`; *equal values under different keys*: no line contains `vm.swappiness` together with `60`, and the password line's token is absent from the whole audit. *Shadowed redacted file*: no `UNKNOWN_INTRO`, one table row, `vm.swappiness` with File value `60`, Runtime value `same` and Source `/etc/sysctl.d/50-x.conf:1`, no drift line, and `canary-shadowed` absent from the whole audit.
    - `case_19_literal_rendering_audit`: the case 19 host (pipe `kernel.core_pattern`, unparsed `<img src=x>`, `[link](http://example.com)` and a triple-backtick run, a file name holding a newline). Parse the audit with `comrak` (GFM table extension on): the sysctl table's rows each have four cells; each cell's code node literal equals the value exactly; the `#### Sysctl` section has no HTML, link or image node.
- [ ] **Step 2: Run** `cargo test -p inspectah-pipeline md_literal` and `cargo test -p inspectah-cli --test sysctl_scan_test audit`. Expected: FAIL (`render::md_literal` and `render::sysctl_report` do not exist; the audit still holds `#### Sysctl Overrides`).
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

- [ ] **Step 1: Write the failing tests**, in `sysctl_scan_test.rs` one `_html` function per case, each scanning the case's host or hosts and asserting on `render_report` and its Kernel & Boot `section_count`:
  - `case_12_one_drifted_key_does_not_drop_the_file_html`: `section_count` is `7`, the number of `Resolved` rows.
  - `case_17_outside_links_html`: *overlapping assignment* and *overlapping exclusion* each give `section_count` `0`; *canary*: `canary-outside` absent from the HTML; *shadowed link*: `section_count` `1`.
  - `case_18_redaction_makes_the_explanation_unknown_html`: every layout but the last gives `section_count` `0`; *equal values under different keys*: the password line's token is absent from the HTML; *shadowed redacted file*: `section_count` `1`; every layout: its canary is absent from the HTML.
  - `case_19_literal_rendering_html`: the HTML holds `&lt;img src=x&gt;`, `[link](http://example.com)` as text, the pipe value, and no `<img` element.
  - In `report.rs` tests, `sysctl_block_has_no_safe_filter`: the sysctl block of `kernel.html` holds no `|safe`.
- [ ] **Step 2: Run** `cargo test -p inspectah-cli --test sysctl_scan_test html` and `cargo test -p inspectah-pipeline report`. Expected: FAIL (`section_count` counts the legacy overrides, always empty since Task 5, so the case 12 and shadowed-layout counts are `0`; no case 19 string reaches the HTML).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both, then the full gate. Expected: PASS.
- [ ] **Step 5: Commit** `feat(report): show the sysctl explanation in the HTML report`.

### Task 9: Refine decision removal and the read-only view (Lane R)

**Files:**
- Modify: `crates/refine/src/types.rs` (delete `ItemId::Sysctl`, `RefinedSysctl`, the three `TriageReason::Sysctl*`), `crates/refine/src/classify.rs` (delete `classify_sysctls`, `SYSCTL_DENY_LIST`, `is_file_backed`, their tests), `crates/refine/src/session.rs` (every `ItemId::Sysctl` arm and `sysctl_overrides` loop), `crates/refine/src/projection/{types,decisions,reference}.rs` (delete `DecisionProjection.sysctls`, `RefSysctlOverride`, `RefKernelBoot.sysctl_overrides`)
- Modify: `crates/web/src/web_types.rs` (delete `SysctlDecisionDto` and `ViewResponse.sysctls`; add § 5.6's DTOs and `ViewResponse.sysctl`), `crates/web/src/adapter.rs` (delete the sysctl rows in the Kernel and Boot context list; call `project_sysctl`), `crates/web/src/aggregate_handlers.rs` (delete the sysctl block and `build_sysctl_variants`; add `AggregateViewResponse.sysctl`), `crates/web/src/handlers.rs` (delete the sysctl bulk-op loop)
- Create: `crates/web/src/sysctl_view.rs`
- Test: `crates/cli/tests/sysctl_scan_test.rs`; `crates/web/tests/contract_snapshots.rs` (its fixture sets `sysctl: None`)

**Interfaces:**
- Consumes: Task 7's `sysctl_report`.
- Produces: `pub fn project_sysctl(snap: &InspectionSnapshot) -> SysctlViewDto` in `sysctl_view.rs`; the DTOs exactly as § 5.6, `Serialize` only. `build_web_view` calls it with `session.snapshot_projected()`; `aggregate_view` likewise for `AggregateViewResponse.sysctl`. Mapping is one-to-one from the report model: `SysctlExplanationDto::Resolved.drift` from `SysctlHostReport.drift`, `SysctlLineRefDto.link_target` from `line_link_target`, `CarriageDto` from `CarriageState`, `NotCarriedReason::{RuntimeDir, UsrLocalLib}` from the `Run`/`UsrLocalLib` variants and `Excluded`/`Advisory`/`NoConfigEntry` from the `/etc` carriage.

- [ ] **Step 1: Write the failing tests**, in `sysctl_scan_test.rs` one `_view` function per case, each scanning the case's host or hosts, loading `session(..)`, and serializing its view with `serde_json::to_value(build_web_view(&session))`:
  - `case_05_rpm_owned_mask_view`: the `files` row for `/etc/sysctl.d/99-sysctl.conf` has `body.kind == "mask"` and `config_path == "/etc/sysctl.d/99-sysctl.conf"`, and `vendor_replacements` holds no entry naming `99-sysctl.conf` (spec case 5, Mark, 2026-11-11).
  - `case_06_sysctl_conf_through_the_symlink_view` (panel half): before any op, `vendor_replacements` is empty and the file row's `config_path` is `/etc/sysctl.conf`; after `SetInclude{Config{"/etc/sysctl.conf"}, false}`, `sysctl.not_carried` holds `{path: "/etc/sysctl.d/99-sysctl.conf", reason: {kind: "excluded", config_path: "/etc/sysctl.conf"}}`.
  - `case_12_one_drifted_key_does_not_drop_the_file_view` (refine half): `explanation.settings` has seven rows and `explanation.drift` one; `SetInclude{Config{"/etc/sysctl.d/90-tuning.conf"}, false}` succeeds (no `InventoryNotToggleable`).
  - `case_15_mask_only_and_glob_only_view`: both hosts' `sysctl.state == "host"`, `explanation.state == "resolved"`, empty `settings`; host A's `vendor_replacements[0].action == "masks"`; host B's `as_written[0].kind == "glob_assign"`.
  - `case_16_collection_failures_view`: each host's `sysctl == {state: "unavailable", reason: <contains the cause>}`.
  - `case_17_outside_links_view`: *overlapping assignment* and *overlapping exclusion* each give `explanation == {state: "unknown", inputs: [{kind: "outside_link", path: "/etc/sysctl.d/90-site.conf", target: "/usr/share/site/tuning.conf"}]}`, with no settings or drift anywhere in `sysctl`; *canary*: `canary-outside` absent from the whole view JSON; *shadowed link*: `explanation.state == "resolved"`, one setting, key `vm.swappiness` with `outcome == {kind: "writes", value: "60", from: {path: "/etc/sysctl.d/90-site.conf", line: 1, link_target: null}, runtime: {kind: "matches"}}`, and `drift` empty.
  - `case_18_redaction_makes_the_explanation_unknown_view`: every layout but the last gives `explanation.state == "unknown"` with `inputs` exactly the layout's list (`redacted_file` naming the file, or `redacted_runtime` naming `kernel.core_pattern`) and no settings or drift anywhere in `sysctl`; *equal values under different keys*: the password line's token is absent from the view's `sysctl` object; *shadowed redacted file*: `explanation.state == "resolved"`, one setting, key `vm.swappiness` with `outcome == {kind: "writes", value: "60", from: {path: "/etc/sysctl.d/50-x.conf", line: 1, link_target: null}, runtime: {kind: "matches"}}`, and `drift` empty; every layout: its canary is absent from the whole view JSON.
  - `no_sysctl_item_id_view`: `ViewResponse` JSON holds no key `sysctls`; `RefinementOp` JSON with `kind: "Sysctl"` fails to deserialize.
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
  - `case_15_mask_only_and_glob_only_groups` (in `sysctl_scan_test.rs`): for both hosts, `get_groups` lists `system_tuning` in `system_config`.
  - `case_21_aggregate` (in `sysctl_scan_test.rs`): two hosts (`SysctlHost`), different content at `/etc/sysctl.d/90-x.conf`, no tuned selection; `merge_snapshots` of the two reloaded snapshots gives two content variants for that path and `kernel_boot.sysctl == None`; `render_audit` holds `AGGREGATE_LINE`; a session on the merged snapshot gives `AggregateViewResponse.sysctl == {state: "aggregate"}` and `sections` holding `system_tuning` with no items and no `sysctls` or `tuned` section. This task owns case 21 because its last surface, the `system_tuning` aggregate section, lands here; the merge (Task 2), audit line (Task 7) and view state (Task 9) are already in place.
- [ ] **Step 2: Run** `cargo test -p inspectah-web`, `cargo test -p inspectah-pipeline section_group` and `cargo test -p inspectah-cli --test sysctl_scan_test -- case_15_mask_only_and_glob_only_groups case_21`. Expected: FAIL (`system_tuning` is retired; the aggregate pushes `tuned` only when a tuned selection exists).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the full gate. Expected: PASS.
- [ ] **Step 5: Commit** `feat(web): restore System Tuning as a section`.

### Task 11: Aggregate merge, `SysctlOverride` deletion and schema 27 (Lane R)

**Files:**
- Modify: `crates/core/src/aggregate/merge.rs` (delete the `sysctl_overrides` merge and `impl AggregateMergeable for SysctlOverride`)
- Modify: `crates/core/src/types/kernelboot.rs` (delete `SysctlOverride` and `KernelBootSection.sysctl_overrides`, and their tests)
- Modify: `crates/collect/src/inspectors/kernelboot.rs` (drop the `sysctl_overrides: Vec::new()` that Task 5 left in `inspect`)
- Modify: `crates/core/src/snapshot.rs` (`SCHEMA_VERSION` and `MIN_SCHEMA` to `27`), and the version literals and `testdata/e2e-fixture.tar.gz` per `snapshot-schema-versioning.md` § What a Bump Touches
- Delete: the `SysctlOverride` cases in `crates/core/tests/aggregate_merge_test.rs` and `aggregate_e2e_test.rs`, and any remaining site the compiler names
- Create: `testdata/fixtures/snapshot/schema-26-sysctl-overrides.json`, a schema 26 snapshot holding a non-empty `kernel_boot.sysctl_overrides` array and no `sysctl`, the one copy of the old payload that every loader test reads with `include_str!`
- Test: `crates/cli/tests/sysctl_scan_test.rs`, `crates/core/tests/snapshot_test.rs`, `crates/refine/tests/tarball_test.rs`, `crates/cli/src/commands/aggregate.rs` tests

- [ ] **Step 1: Write the failing tests:**
  - `schema_26_snapshot_is_refused`, one in each loader's tests (`InspectionSnapshot::load` in `snapshot_test.rs`, `tarball::from_tarball` in `tarball_test.rs`, `load_snapshot_from_tarball` in `aggregate.rs`), each over the fixture payload: the load fails with the unsupported-schema error naming `26`, as the existing schema 25 refusal tests assert. The reason for the bump, which Step 3 records in the skill file, is semantic staleness: a schema 26 snapshot predates the precedence model, and its files may have been shadowed.
  - Rewrite Task 5's `legacy_sysctl_overrides_not_captured` as `legacy_sysctl_overrides_absent_from_a_scan`: the snapshot JSON of the case 11 host has no key `sysctl_overrides` under `kernel_boot`.
- [ ] **Step 2: Run** `cargo test -p inspectah-core --test snapshot_test schema_26`, `cargo test -p inspectah-refine --test tarball_test schema_26`, `cargo test -p inspectah-cli --bin inspectah schema_26` and `cargo test -p inspectah-cli --test sysctl_scan_test legacy_sysctl_overrides`. Expected: FAIL (schema 26 still loads everywhere; the field still serializes as `[]`).
- [ ] **Step 3: Implement** and update the skill file's version history paragraph (26 to 27, semantic staleness).
- [ ] **Step 4: Run** the full gate. Expected: PASS. Removal checks: `grep -rnw "SysctlOverride" crates --include='*.rs'` finds nothing; `grep -rn "sysctl_overrides" crates --include='*.rs'` finds only the string literal in `legacy_sysctl_overrides_absent_from_a_scan` (the old payload lives in the fixture under `testdata/`, outside `crates`).
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

### Task 15: Real-host validation run (Lane R)

Runs after Tasks 1 to 14 have landed, against the branch tip. It changes no code. It follows `process-docs/skills/real-host-validation-runs.md`; read it in full first. Dispatch needs the VM operation skill, which this repository does not hold.

**Hosts:** the RHEL 9.8 and RHEL 10.2 aarch64 guests that file's recent runs used, each reverted to its clean snapshot before the run (profiles are cumulative). Do not reboot a guest between the drift run and a scan: tuned reapplies its profile at boot, which changes the expected drift set.

**Binary:** `cargo zigbuild --target aarch64-unknown-linux-musl --release`; on each guest, `inspectah version` prints the branch tip's short commit. Record it.

**What driftify plants and what it does not.** The standard profile writes `/etc/sysctl.d/99-driftify.conf` (applied live), `hugepages.conf` (applied live) and `thp.conf` (written, not applied, so `vm.compaction_proactiveness` drifts). It does not edit `/etc/sysctl.conf`, mask a sysctl file, shadow a vendor sysctl file, or put a secret in a sysctl file. The guide plants by hand where the drift tool falls short (the disabled-repo and repo-less plants), so this task plants those four by hand, with the commands below, and the report lists each as a driftify follow-up. A mask on `99-sysctl.conf` turns off `/etc/sysctl.conf`, and a secret makes the whole explanation unknown, so the plants go in three passes on the same guest, each scanned.

**Step 1: Pass A plants**, after `driftify --profile standard`:

```
ls -l /usr/lib/sysctl.d/ /etc/sysctl.d/                  # record both listings
rpm -qf /etc/sysctl.conf /etc/sysctl.d/99-sysctl.conf    # record the owners
echo 'kernel.sysrq = 1' | sudo tee -a /etc/sysctl.conf   # not applied live
sudo ln -s /dev/null /etc/sysctl.d/50-coredump.conf      # mask over a vendor file
echo 'kernel.pid_max = 4194304' | sudo tee /etc/sysctl.d/50-pid-max.conf   # same-name shadow
sudo rpm -Va | grep sysctl                               # record
```

If `/usr/lib/sysctl.d/` lacks `50-coredump.conf` or `50-pid-max.conf` on a host, use another vendor name it lists, and record the substitution. If `/etc/sysctl.d/99-sysctl.conf` is not a package-owned link to `../sysctl.conf` on a host, record that and skip the criteria below that name it on that host.

**Step 2: Pass A scan and checks.** Scan as root, extract the tarball, then run refine on it over `ssh -n` with `setsid` as the guide shows, and drive it with `curl`.

- [ ] A1 snapshot: `schema_version` is 27; `kernel_boot` holds `sysctl` and no `sysctl_overrides`; `sysctl.files` lists `99-driftify.conf`, `hugepages.conf`, `thp.conf` and `50-pid-max.conf` under `etc` with `body.kind == "read"`, `50-coredump.conf` under `etc` with `body.kind == "mask"`, `99-sysctl.conf` under `etc` read with `link_target == "/etc/sysctl.conf"`, and every file `ls /usr/lib/sysctl.d/` printed under `usr_lib`.
- [ ] A2 audit, `#### Sysctl`: the settings table is present (a `Resolved` explanation); drift lines exist for `vm.compaction_proactiveness` (files request `0`) and `kernel.sysrq` (source `/etc/sysctl.d/99-sysctl.conf:<line>` with the `/etc/sysctl.conf` link note); every other drift line is listed in the report with an explanation (`vm.nr_hugepages` drifts when the guest cannot allocate 128 pages); no row's source names `/usr/lib/sysctl.d/50-coredump.conf` or `/usr/lib/sysctl.d/50-pid-max.conf`.
- [ ] A3 audit, vendor files replaced: exactly two lines, `/etc/sysctl.d/50-coredump.conf` *disables* and `/etc/sysctl.d/50-pid-max.conf` *replaces* its `/usr/lib/sysctl.d` namesake, both in the carried wording.
- [ ] A4 config tree and Containerfile: `config/etc/sysctl.d/` holds `99-driftify.conf`, `hugepages.conf`, `thp.conf` and `50-pid-max.conf` with the host file's `sha256sum`, and `50-coredump.conf` as a regular 0-byte file; `config/etc/sysctl.conf` has the host file's `sha256sum`; `find . -name 99-inspectah-migrated.conf` and `find . -type d -name sysctl` print nothing; no Containerfile line names `99-inspectah-migrated` or `COPY sysctl/`.
- [ ] A5 refine: `/api/groups` lists `system_tuning`, labelled System Tuning, in `system_config`; `/api/view` has no `sysctls` key, `sysctl.state == "host"`, `sysctl.explanation.state == "resolved"`, and a `files` row for `50-coredump.conf` with `body.kind == "mask"`; a `POST /api/op` whose `item_id` kind is `Sysctl` is refused (4xx) and the generation in `/api/view` does not move.
- [ ] A6 refine, case 6 on a real host: download an export at the current generation and keep it; then `SetInclude` on `{"kind":"Config","key":{"path":"/etc/sysctl.conf"}}` with `include: false`; `/api/view` lists `/etc/sysctl.d/99-sysctl.conf` in `sysctl.not_carried` with reason `excluded` and `config_path` `/etc/sysctl.conf`; a second export lacks `config/etc/sysctl.conf`.

**Step 3: Pass B plant, scan and checks.** On the same guest: `sudo ln -sfn /dev/null /etc/sysctl.d/99-sysctl.conf`, then record `sudo rpm -Va | grep 99-sysctl`. Rescan and rerun refine.

- [ ] B1 snapshot: `config.files` holds exactly one entry at `/etc/sysctl.d/99-sysctl.conf`, RPM-owned modified, included, empty content, with the flags `rpm -Va` printed (an `L` among them); `sysctl.files` shows it with `body.kind == "mask"`.
- [ ] B2 audit: no row or drift line for `kernel.sysrq` sourced from `99-sysctl.conf`; no vendor-replacement line naming `99-sysctl.conf` (spec case 5, Mark, 2026-11-11); the two A3 lines are unchanged.
- [ ] B3 refine: the `files` row for `99-sysctl.conf` has `body.kind == "mask"` and `config_path == "/etc/sysctl.d/99-sysctl.conf"`; `vendor_replacements` holds nothing naming it.
- [ ] B4 export: the scan tarball and an untouched refine export each hold `config/etc/sysctl.d/99-sysctl.conf` as a regular 0-byte file and `config/etc/sysctl.conf` with the host's `sha256sum`.

**Step 4: Pass B builds.** For the pass B scan tarball and the untouched pass B refine export: run the guide's COPY-source check, then `podman build` under `env -u HTTP_PROXY -u HTTPS_PROXY -u NO_PROXY`, then `bootc container lint`. Time each build with `date +%s`.

- [ ] C1: the COPY-source check prints no `MISSING` line; both builds pass; lint shows only the known warnings (two on RHEL 9.8, `nonempty-run-tmp` and `var-tmpfiles`; three on RHEL 10.2, adding `nonempty-boot`).
- [ ] C2: in each image, `ls -l /etc/sysctl.d/` shows `50-coredump.conf` and `99-sysctl.conf` as regular 0-byte files and the four admin files present; `/usr/lib/sysctl.d/50-coredump.conf` is present from the base image; `systemd-sysctl --cat-config` reads `/etc/sysctl.d/50-coredump.conf` and `/etc/sysctl.d/50-pid-max.conf` in place of their `/usr/lib/sysctl.d` namesakes. Record its output.

**Step 5: Pass C plant, scan and checks.** On the same guest: `echo 'db_password = canary-realhost' | sudo tee /etc/sysctl.d/80-app.conf`. Rescan and rerun refine.

- [ ] D1: `grep -rl canary-realhost` over the extracted tarball, the audit, the HTML report and the `/api/view` JSON prints nothing.
- [ ] D2 audit: the unknown text, with one line naming `/etc/sysctl.d/80-app.conf` as text that redaction replaced; no settings table and no drift line.
- [ ] D3 refine: `sysctl.explanation.state == "unknown"` with one input, `redacted_file` naming `/etc/sysctl.d/80-app.conf`; no settings or drift anywhere under `sysctl`.

**Step 6: Teardown.** Check for a leftover refine server (`pgrep -a inspectah` on the guest), remove the base image with `sudo podman rmi` if the guest will not be reverted, and `poweroff` inside the guest.

**Step 7: Report.** Every criterion above, per host, as pass or fail with the command and its output for each failure. **A failure is reported, not fixed**: no code change in this task, and later criteria that do not depend on the failed one still run. The report lists the driftify follow-ups: an `/etc/sysctl.conf` edit, a vendor sysctl mask, a same-name vendor shadow, and a secret-bearing sysctl file, at a profile level where the mask on `99-sysctl.conf` and the `/etc/sysctl.conf` edit do not collide.

**Step 8: Skill update.** Add a `### Measured, <date>: sysctl carriage, scan to build` subsection to `process-docs/skills/real-host-validation-runs.md` with the commit, hosts, the plants, what passed and what did not, and any finding the next run needs. Commit `docs(skills): record the sysctl real-host run`.

---

## Coverage

### Spec requirements

| Spec | Requirement | Task |
|---|---|---|
| § Dependency | modified RPM configs included at collect | 1 (verified) |
| § 5.1 | carriage table rows: unowned file, unowned mask, RPM-owned mask, `/etc/sysctl.conf`, modified package file, vendor/run not copied | 1, 5 |
| § 5.1 | `L` + `/dev/null` rule for every `/etc` path; path stays in `rpm_va_paths` | 1 |
| § 5.1 | other re-pointed RPM links not carried; reported "Not carried" | 1 (twin), 5 (case 5 twin), 7 (`NoConfigEntry`) |
| § 5.1 | removals: shadow pass, `ConfigShadow::Sysctl`, `SYNTHESIZED_SYSCTL_FILE`, writer, `COPY`, `sysctl/` dir, `sysctl_refusal` | 5 |
| (plan) | legacy `sysctl_overrides` capture retired before the redaction pass | 5 (capture), 11 (field and type) |
| § 5.2 | four directories, `.conf` only, link rules, outside-link allowlist, size limit | 4 |
| § 5.2 | failures make `sysctl` `None`, degraded with the cause | 4 |
| § 5.2 | parsing, exclusions, key normalization | 2, 4 |
| § 5.2 | runtime: run when any key line exists, normalized keys, absent recorded as absent | 4 |
| § 5.3 | types, content stored not lines, schema bump | 2, 11 |
| § 5.3 | `SysctlKey` wire check and both-fields check | 2, 4 (case 20 from a scan) |
| § 5.3 | `effective`, `vendor_replacements`, `carriage`; `sysctl_values_equal` moves to core | 3, 2 |
| § 5.4 | sysctl pass, table of fields, `redacted`, runtime removal, findings, content map | 6 |
| § 5.4 | field-classification tests | 6 |
| § 5.5 | audit items 1 to 6, unavailable and aggregate lines | 7 |
| § 5.5 | literal helper | 7 |
| § 5.5 | HTML report blocks, autoescape, `section_count` | 8 |
| § 5.6 | registry, badge counts tuned only, batch toggle, retirement tests flip | 10, 13 |
| § 5.6 | read-only DTOs, `project_sysctl`, `ViewResponse.sysctl`, `AggregateViewResponse.sysctl` | 9 |
| § 5.6 | empty-state rule; single-host frontend; aggregate frontend | 13 |
| § 5.6 | removals (Rust) | 9 |
| § 5.6 | removals (TypeScript) | 13 |
| § 5.6 | aggregate `system_tuning` section always pushed | 10 (with case 21) |
| § 5.6 | `Sysctl` config rows toggleable | 5, 9 (case 12) |
| § 5.7 | `sysctl: None` on aggregate | 2 |
| § 5.7 | per-key merge removed; impl removed | 11 |
| § 5.7 | aggregate audit and panel line | 7, 9, 10 (case 21), 13 |
| § 6 | stale docs and skills | 14 |
| § 6 | CHANGELOG | 14 |
| § 8 | tests encoding removed behavior deleted or rewritten | 5, 9, 10, 11, 13 |
| (plan) | real-host run of scan, audit, refine and build on RHEL 9 and 10 | 15 |

### Spec test cases

| Case | Task(s) | Test |
|---|---|---|
| 1 | 3, 4, 7 | rule pins; `case_01_each_directory_is_read` (with the unlisted-key `NotExposed` layout); `_audit` (`not exposed` cell) |
| 2 | 3, 4 | rule pin; `case_02_priority_by_name_across_directories` |
| 3 | 3, 4, 15 | rule pins; `case_03_same_name_shadow_each_adjacent_pair` (with `/run` over `/usr/lib`); real host pass A |
| 4 | 3, 5, 15 | rule pin; `case_04_unowned_mask`; real host pass A |
| 5 | 1, 5, 9, 15 | config rule; `case_05_rpm_owned_mask` (with the `NoConfigEntry` twin); `_view`; real host pass B |
| 6 | 3, 5, 9, 15 | rule pin; `case_06_sysctl_conf_through_the_symlink` (export); `_view` (panel); real host A6 |
| 7 | 2, 4 | `normalize_spellings`; `case_07_key_spellings` |
| 8 | 3, 4 | rule pin; `case_08_repeated_assignment_in_one_file` |
| 9 | 2, 4 | `parse_lines`; `case_09_dash_prefixed_assignment` |
| 10 | 3, 4, 7 | rule pins; `case_10_key_exclusion_both_orders` (with the unlisted-key layout); `_audit` |
| 11 | 5, 7, 15 | `case_11_drifted_key_export`; `case_11_drifted_key_audit`; real host A2, A4 |
| 12 | 3, 5, 7, 8, 9 | rule pin (own-path carriage); `case_12_one_drifted_key_does_not_drop_the_file` (export, one `Differs`); `_audit` (exactly one drift line); `_html` (`section_count`); `_view` (toggleable) |
| 13 | 2, 4, 7 | `values_equal_ignores_whitespace_runs`; `case_13_whitespace_is_not_drift`; `_audit` (no drift line) |
| 14 | 2, 3, 4, 7 | parser; rule pin; `case_14_glob_reported_not_expanded`; `_audit` ("As written" with file and line) |
| 15 | 4, 9, 10, 12, 13 | `case_15_mask_only_and_glob_only_snapshot`; `_view`; `_groups`; fixtures; navigation |
| 16 | 4, 7, 9, 12, 13 | `case_16_collection_failures_snapshot`; `_audit`; `_view`; fixtures; navigation |
| 17 | 3, 4, 7, 8, 9, 12, 13 | rule pins; `case_17_outside_links_snapshot`; `_audit`, `_html` and `_view`, each covering both overlapping layouts, the canary and the shadowed link; fixtures; navigation |
| 18 | 3, 6, 7, 8, 9, 15 | rule pins; `case_18_redaction_makes_the_explanation_unknown_snapshot`; `_audit`, `_html` and `_view`, each with the shadowed-redacted-file `Resolved` branch; real host pass C |
| 19 | 7, 8 | `case_19_literal_rendering_audit` (comrak, CLI dev-dependency); `case_19_literal_rendering_html` |
| 20 | 2, 4 | `sysctl_key_on_the_wire` (wire unit); `case_20_key_on_the_wire` (from a scan) |
| 21 | 2, 7, 9, 10, 12, 13 | merge sets `None`; `AGGREGATE_LINE`; aggregate view state; `case_21_aggregate`; fixtures; navigation |
| 22 | 12, 13 | fixture writer; `SystemTuning.navigation.test.tsx` |

**Collector-driven fixtures.** Every case above runs through `SysctlHost` and the scan path; the case 22 fixtures come from sessions built the same way (Task 12). The only hand-built sysctl values are Task 3's rule pins (each naming its twin), Task 2's wire-check payloads (twin: Task 4's `case_20_key_on_the_wire`), Task 6's sentinel planting, and Task 11's schema 26 payload, which stands for an old snapshot, not a host layout.

## Risks

- **Interim commits.** Between Task 4 and Task 5 the legacy collector runs beside the new one, so a `sysctl -a` failure is reported twice in the degraded reason. From Task 5 until Tasks 7 and 9 replace them, the audit's `#### Sysctl Overrides` table and refine's per-key sysctl rows are empty for every new scan, because the capture that filled them is gone. Between Task 9 and Task 13 the running refine UI reads `sysctls` that the backend no longer sends. The branch is not releasable until Task 13 lands.
- **`MockExecutor` link semantics** differ from the real executor's (fixture note under Task 1). A fixture that omits the link-path content tests a read failure, not the layout it names; `SysctlHost::link` exists so no test registers links by hand.
