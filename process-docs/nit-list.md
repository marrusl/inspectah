# inspectah Nit List

Small output quality and polish items. Not worth individual specs — just fix when touching nearby code.

## ~~Build Output Streaming~~ (DONE — 2026-05-30)

## ~~Naming Consistency (Rust)~~ (DONE — 2026-05-30)

- [x] **ContextSection → ReferenceSection:** Renamed `ContextSection` → `ReferenceSection`, `normalize_for_context()` → `normalize_for_reference()`, and `context_section()` helper across Rust backend (handlers.rs, api_test.rs) and TypeScript types (api/types.ts). 18 files touched. *(commit 2d3f22d — 2026-05-30)*

## Repo Tier Model

- [ ] **RPM upload feature (needs spec):** Let users upload a local RPM into the tarball for repo-less packages. Separate folder in the tarball, direct `COPY + rpm -i` in the Containerfile. Turns "this package has no migration path" into "here's the RPM, install it directly." Solves the case for vendor installers, one-off downloads, and manual builds.

## ~~Fleet Aggregate Output~~ (DONE — 2026-05-25)

Moved to Completed section.

## Version Changes Sort

- [ ] **Sort toggle for Version Changes tab:** Add a sort control (alpha vs. status). Current sort is by direction (downgrades first, then upgrades). Add an alphabetical-by-name option. Default to status sort, let user toggle. Applies to both the reference section view and the package detail `VersionChangeEntry` list.

## ~~Port Fallback~~ (DONE — 2026-05-30)

- [x] **Auto-select alternate port when 8642 is in use:** `inspectah refine` auto-retries ports 8643-8652 on AddrInUse error and prints which port it bound to. *(DONE — 2026-05-30, already implemented)*

## Git History Cleanup

- [ ] Scrub `.git-backup/` from git history using `git filter-repo`. The directory was accidentally committed and contains a 68MB packfile. It's removed from the working tree and `.gitignore`'d, but still inflates clone size. Do this before the repo goes more public — requires a force push.

## ~~Scan Progress Follow-ups~~ (DONE — 2026-05-25)

Moved to Completed section.

## ~~Baseline Version Comparison Accuracy~~ (DONE — 2026-05-25)

Moved to Completed section.

## ~~Refine UI Polish~~ (DONE — 2026-05-26)

Moved to Completed section.

## ~~Fleet Refine UI~~ (DONE — 2026-05-26)

Moved to Completed section.

## ~~Containerfile Change Highlights — Review Followups~~ (DONE — 2026-05-30)

- [x] **Scroll test coverage:** Added 5 tests: multi-line scroll targeting (topmost changed line), first-content-while-collapsed baseline (no dot, no announcement), expand-after-collapse baseline diffing, resize-driven auto-collapse (baseline captured, highlights cancelled), pending-change auto-clear. *(commit 0455af6 — 2026-05-30)*

## RepoBar Click-to-Filter (v2 backlog)

- [ ] Repo names in the REPOSITORIES bar should be clickable. Clicking a repo name filters the package list to show only that repo's packages (or scrolls + highlights, lighter option). Render names as `<button>`, `cursor: pointer`, hover color shift to brand color, `aria-label="Jump to baseos packages (61)"`. Fern recommends scroll+highlight using existing `.inspectah-highlight` animation; Ember recommends filter-on-click as more useful for triage. Either way, distro repos gain their first interactive purpose beyond the "always included" label.

## ~~RepoBar Accessibility~~ (DONE — 2026-05-30)

- [x] RepoBar `aria-live` badge announces dismiss/restore changes via dedicated visually-hidden `aria-live="assertive"` span with explicit dismiss/restore messages. 4 tests added. *(commit 450ca18 — 2026-05-30)*

## ~~Type `users_groups_decisions`~~ (DONE — 2026-05-30)

- [x] `users_groups_decisions` on `ViewResponse` is `Vec<serde_json::Value>` — an untyped escape hatch. This means the Playwright fixture-structure validation (insta snapshots) cannot catch drift in the users/groups decision payload. `users.spec.ts` tests rely on structural fixture correctness only. Typing this field as a proper DTO (e.g., `Vec<UserGroupDecision>`) is the prerequisite for full fixture-validation coverage of the users/groups surface. Flagged during Playwright testing expansion spec review (Tang, round 2). *(commit 9121121 — 2026-05-30)*

## Preserve-Subscription Plan Deferrals

- [ ] **Spec/plan provenance alignment:** Spec text says `source_hostname` belongs in fleet metadata; plan stores it in `SubscriptionSection.source_hostname` (per Task 1 contract decision — keeps provenance with the data it describes). Spec needs a text update to match the plan.
- [ ] **`--prefer-host-subscription` override flag:** On RHEL hosts, ambient pass-through wins over tarball-carried certs when the ambient bundle is valid. A user override to force tarball certs is not included in v1. File as enhancement if requested.
- [ ] **Hardlink extraction support:** `TarballExtractor` rejects all hardlinks. inspectah tarballs don't use them today. Add within-root extraction support if a future tarball format needs them.

## Preserve-Subscription Code Review Follow-ups

Items flagged during code review. Reviewers approved at POC bar — these raise it to production bar.

- [ ] **Planner-level ambient fallback test:** Current ambient proof tests cover the `detect_ambient_subscription_in()` helper and `should_use_subscription_mounts()`. Add a deterministic test at the `plan_and_execute()` level in `inspectah-pipeline/src/build/mod.rs` proving that incomplete ambient + complete tarball produces a build command with `-v` subscription mounts, and that complete ambient produces a build command without them.
- [x] **AbsolutePath branch direct proof:** Test renamed from misleading `reject_path_traversal` → `reject_parent_dir_traversal`. Added real `reject_absolute_path` test exercising the literal `/` branch. *(commit b230af9 — 2026-05-30)*
- [ ] **Symlink collector: `canonicalize()` vs lexical normalization:** The real executor uses `std::fs::canonicalize()` for `resolve_final_target()`, but the mock uses lexical chain-following. If intermediate directory symlinks matter in production subscription paths (unlikely but possible with custom subscription-manager configurations), the mock could miss divergence. Consider a filesystem-backed integration test using real symlink chains on a temp directory.

## TUI: Remaining Section Coverage Gaps

- [ ] **Composite section rendering depth:** Services and Containers composite sections render all sub-collections but group headers could be more descriptive (e.g., show sub-collection labels like "Divergent services", "Preset-unknown enabled")
- [ ] **Repo toggle from TUI:** Repo bar is display-only in TUI. Web UI supports toggling entire repos. Wire `SetInclude { ItemId::Repo }` to a TUI keybind (e.g., `R` on the repo bar line)

## Preserve/Redaction Hardening (from spec review)

- [ ] **In-artifact warning for unredacted output:** Consider a report banner or tarball filename suffix (e.g., `-UNREDACTED`) for snapshots produced with `--no-redaction`, to reduce accidental oversharing. Not blocking for pre-1.0 alpha, but should land before GA or broader external sharing.

## ~~Workspace Layout~~ (DONE — 2026-06-09)

## Server Disconnected Overlay

- [ ] **Show "Server disconnected" in the browser when the backend stops:** When the refine web server is killed (Ctrl-C), the page silently stops responding. Add a periodic heartbeat check in the frontend JS (fetch a lightweight endpoint, e.g., `/api/health`). When it fails, show a full-page overlay: "Server disconnected. Restart `inspectah refine` to continue." Prevents confusion when the backend goes away mid-session.

## Scan Output Rethink Spec Polish (from R3 review)

- [ ] **`--quiet` prose vs CLI behavior:** Spec carry-forward text says `--quiet` is a no-op in the new renderers. Current CLI behavior: `--quiet` suppresses the progress checklist (null renderer) but still prints completion + output path. Spec should either preserve the existing contract or explicitly declare a behavior change.
- [ ] **Sensitivity-confirmation ordering:** Section 6a and the subscription example disagree on ordering (timing line → sensitivity → report path vs. timing line → report path → sensitivity). Pick one order and make prose, examples, and implementation match.
- [ ] **Interrupted-path wording:** Spec says "only appears for wave-2 inspectors that never started because SIGINT arrived during wave 1." Actual cancellation boundary is more nuanced — SIGINT can arrive mid-wave, leaving some wave-1 inspectors without a completion result. Soften to "inspectors lacking a completion result by the cancellation boundary."

## TUI Input Handling

- [ ] **Evaluate `tui-input` vs custom input:** TUI currently uses custom input handling via crossterm. The `tui-input` crate was a dependency but unused (removed in dead code scan). Evaluate whether adopting `tui-input` would simplify the input layer vs. maintaining the custom approach.

## TUI: Quality Items (from Thorn CP1/CP2)

- [ ] **Unicode width handling:** `truncate()` in triage_list.rs uses byte length as proxy for display width. CJK/emoji characters may under-truncate. Consider `unicode-width` crate for accurate column measurement.
- [ ] **FlashMessage tests:** No tests for FlashMessage time-based expiry behavior. Add `flash_not_expired_immediately` and `flash_expired_after_zero_duration`.
- [ ] **Baseline group index constant:** `TuiState::new()` hardcodes `2` for baseline group. Define `const BASELINE_GROUP_INDEX: usize = 2;`.
- [ ] **Help mode key mapping test:** No test for Help mode in keys.rs. Add coverage for Esc/q/? → CloseDetail.

## Cloud Provider Detection

- [ ] **DMI strings + cloud-agent detection:** Read `/sys/class/dmi/id/sys_vendor` and `/sys/class/dmi/id/product_name` to identify cloud provider. Lookup table: "Amazon EC2" -> aws, "Google Compute Engine" -> gcp, "Microsoft Corporation" -> azure, etc. Cross-reference with cloud-agent packages already in `packages_added` (`cloud-init`, `WALinuxAgent`, `amazon-ssm-agent`). Store as `Option<String>` on `InspectionSnapshot.meta` (or a dedicated `cloud_provider` field on `OsRelease`). No new inspector needed -- two `exec.read_file()` calls in the existing scan pipeline or a lightweight `system.rs` helper. ~80 lines including tests.

## Multiarch (i686) Warning

- [ ] **Populate `multiarch_packages` during single-host scans:** The `multiarch_packages: Vec<String>` field already exists on `RpmSection` (line 203 of `types/rpm.rs`) and is handled correctly in fleet merge (`merge.rs` line 881-882). It is never populated during single-host collection -- the `RpmSection` construction in `rpm/mod.rs` line 492-508 uses `..Default::default()`. Fix: after building `packages_added`, group by name, collect entries where >1 distinct arch exists, set `multiarch_packages` to those names. Emit a warning with the affected package names. ~40 lines.

## Resource Limits (limits.conf)

- [ ] **Parse limits.conf for operator intent:** `ConfigCategory::Limits` already classifies `/etc/security/limits.*` files (`classify.rs` line 59-62). Content is captured in `ConfigFileEntry.content`. Missing: parse the content for `domain type item value` tuples (one per non-comment, non-blank line) and surface structured entries in the report/audit output so users see "elasticsearch nofile 65535" rather than a raw file reference. Could be a post-processing step in `config/mod.rs` or a presentation enhancement in `pipeline/src/render/report.rs`. No schema changes needed -- `ConfigFileEntry.content` already carries the data, this is about presentation. ~60 lines.

## Chrony/NTP Config

- [ ] **Parse chrony.conf for custom NTP servers:** Config inspector already catches `/etc/chrony.conf` as rpm-owned-modified (confirmed by `config_test.rs` line 52/92). Content is captured. Missing: parse `server`, `pool`, and `peer` directives to surface custom NTP sources vs RHEL defaults. Straightforward line-by-line parse of the already-captured content, no external commands. Consider adding `ConfigCategory::TimeSync` variant for `/etc/chrony.conf` and `/etc/chrony.keys` (currently classified as `Other`). ~50 lines including a category addition and tests.

## Duplicate Packages (same name+arch, multiple versions)

- [ ] **Detect duplicate RPM installs:** Same package name and arch installed at multiple versions (e.g., `kernel` is expected, but `openssl-libs-1.1.1` and `openssl-libs-3.0.7` is a problem). Group `packages_added` by `(name, arch)`, flag entries with >1 version. Emit warning. Reference: convert2rhel `duplicate_packages.py`. Very low effort — group-by logic on existing data.

## Tainted Kernel Modules

- [ ] **Detect tainted kernel modules:** Parse `/proc/modules` flags column for out-of-tree or proprietary modules (taint flags `O`, `E`, `P`). These may not be available in the target image. Emit warning with module names and taint reasons. Reference: convert2rhel `tainted_kmods.py`. Low effort — file parse + flag lookup.

## SSSD/Kerberos/IPA Identity Config

- [ ] **Classify SSSD/Kerberos/IPA configs:** Config inspector already captures modified files under `/etc/sssd/`, `/etc/krb5.conf`, `/etc/ipa/`. Missing: add `ConfigCategory` variant (e.g., `Identity`) so these surface as a classified group rather than generic config. Very low effort — category addition in `classify.rs`.

## firewalld.conf Coverage

- [ ] **Verify firewalld.conf detection:** Likely already caught by rpm -Va as rpm-owned-modified. Verify coverage and confirm it surfaces in the config section. If the `CleanupModulesOnExit` directive changed between RHEL versions, flag it. Very low effort — verification pass.

## System-Wide Crypto Policy

- [ ] **Detect custom crypto policy:** Check `/etc/crypto-policies/config` for non-default policy (e.g., `FUTURE`, `FIPS`, custom). Default is `DEFAULT`. A custom policy is operator intent that must carry into the image. Very low effort — single file read + string compare.

## nsswitch.conf

- [ ] **Detect custom nsswitch.conf:** Parse `/etc/nsswitch.conf` for non-default name service switch entries (e.g., `sss`, `ldap`, `winbind`). These indicate external identity sources that need corresponding packages in the image. Likely already caught as modified config — verify and consider structured parsing. Very low effort.

## kdump.conf Coverage

- [ ] **Verify kdump.conf detection:** Likely already caught by rpm -Va as rpm-owned-modified. Verify coverage and confirm custom crash dump targets surface. Very low effort — verification pass. Reference: leapp kdump actor.

---

## Completed

### Fleet Triage: Non-Universal Variants (DONE — 2026-05-26)

- [x] **Non-universal items with variants don't require review:** Divergent items with prevalence < total demoted from `Investigate` (needs_review) to `Divergent` (informational). Only universal items with variant differences get review-level triage.

### Naming Consistency (DONE — 2026-05-26)

- [x] **DECISION_SECTIONS / CONTEXT_SECTIONS → Review / Reference:** Renamed `DECISION_SECTIONS` → `REVIEW_SECTIONS` and `CONTEXT_SECTIONS` → `REFERENCE_SECTIONS` in `Sidebar.tsx`. No Rust-side constants existed. *(DONE — 2026-05-26)*

### Containerfile Output

- [x] `RUN dnf install -y` lines should use `\` line continuation right after `-y`, one package per line. More readable. Same treatment for the cleanup `dnf` and `rm` commands. *(DONE — 2026-05-25)*
- [x] Don't emit comment headers for empty sections. Structural fix: all 22 section headers route through a `section()` helper that only emits the header if body is non-empty. New sections get the guard automatically. *(DONE — 2026-05-25)*

### Repo Tier Model

- [x] Empty/unknown `section_id` falls through to `RepoTier::ThirdParty` in `repo_index.rs::repo_tier()`. Added explicit `RepoTier::None` variant (renamed from `Unknown` — "none" = no repo identity, not merely unidentified). *(DONE — 2026-05-25)*
- [x] **Config-only `@commandline` packages (auto-exclude):** `@commandline` packages whose ALL owned files are under `/etc/` and in the config tree get auto-excluded with `PackageConfigCaptured` reason. `epel-release` correctly rejected (has `/usr/bin/crb`). `repo_tier("@commandline")` returns `None`. *(DONE — 2026-05-25)*
- [x] **Repo-less packages (visibility):** `@commandline` source_repo now treated like empty — `NeedsReview` / `PackageNoRepoSource`. These surface as highest-risk migration items. *(DONE — 2026-05-25)*

### Fleet Prevalence Bug (configs)

- [x] Config prevalence undercount fixed. `FleetPrevalence` gains `aggregate_count`/`aggregate_hosts` (populated for multi-variant items). `merge_with_variants()` computes union of all variant hosts. Per-variant prevalence still tracks per-variant; aggregate tracks file-level. *(DONE — 2026-05-25)*

### Fleet Aggregate Output (DONE — 2026-05-25)

- [x] Aggregate output surfaces divergence and agreement across the fleet through the zone-based triage system. Fleet view displays items grouped into consensus zones (universal agreement), near-consensus zones (majority agreement), and divergent zones (scattered across hosts). The UI in `FleetSection.tsx` renders these zones with filtering and gives users a clear picture of fleet consistency. Backend triage logic assigns items to zones based on prevalence thresholds. *(DONE — 2026-05-25)*

### Fleet Conflict Count Bug

- [x] `repo_conflict_count` overcount fixed. Root cause: `anaconda` vs `baseos` counted as conflict despite both being distro repos. Conflict detection now normalizes through `repo_tier()` — same-tier differences are skipped. Repo tier constants moved to `inspectah-core::types::repo` to avoid circular dep. *(DONE — 2026-05-25)*

### Fleet API Test Hardening

- [x] Make the `fleet_state_with_packages()` fixture minority-first so the end-to-end API test would independently fail if the row-level `source_repo` majority rewrite in `merge_rpm_sections()` were removed. *(DONE — 2026-05-25)*

### Go Retirement (DONE — 2026-05-24)

- [x] Remove Go source tree, CI workflow, packaging spec rewritten for Rust, schema floor raised, all Go compat code and parity tests removed.

### Scan Progress Follow-ups

- [x] **Export failure double-error:** Prevented duplicate error output on export failure. *(DONE — 2026-05-25)*

### Fleet vs Single-Machine Reference Section Density

- [x] Reference/context sections converged to compact rows matching fleet density. Replaced PF DataList with simple divs, subsection headers in uppercase. *(DONE — 2026-05-25)*

### Host Info Display

- [x] `os_name`/`os_version` duplication ("CentOS Stream 9 9"). Fixed in `handlers.rs` with `deduplicate_version()` — word-boundary matching handles RHEL 9/10, CentOS Stream, Fedora, Bazzite, Aurora, Bluefin. 14 tests. Minor version shown in parens when pretty_name has only major (e.g., "RHEL 10 (10.2)"). *(DONE — 2026-05-25)*

### Baseline Version Comparison Accuracy

- [x] Reframed: the "host newer" count was correct (only shared packages), but the summary didn't surface packages absent from the base image. `version_comparison_summary()` now takes a `not_in_base` count and appends ", N not in base image" when > 0. Both call sites (CLI scan + Containerfile report table) updated. *(DONE — 2026-05-25)*

### Scan Progress Follow-ups

- [x] **`--verbose` / `--quiet` flags:** Added `-v` and `-q` flags to `inspectah scan`. `--quiet` suppresses progress checklist (null renderer), still prints completion + output path. `--verbose` plumbed through to all three renderers (sub-step collapsing not yet implemented — plumbing ready). `conflicts_with` enforced via clap. *(DONE — 2026-05-25)*

### Refine UI Polish (2026-05-26 testing session)

- [x] **User toggle missing:** Added checkbox to UserCard header mapping to skip (unchecked) / useradd (checked), matching DecisionItem pattern. *(DONE — 2026-05-26)*
- [x] **Row click to expand:** Added `onClick={handleExpand}` to DecisionItem and ContextItem row divs with `stopPropagation` on interactive children. `cursor: pointer` styling. *(DONE — 2026-05-26)*
- [x] **@commandline repo toggle:** Made non-toggleable by filtering on `provenance === "unknown"`. Display name changed to "Local / Manual installs". *(DONE — 2026-05-26)*
- [x] **Missing section titles:** Added consistent `<h2>` headings to all content panes via `SECTION_LABELS` map in MainContent. *(DONE — 2026-05-26)*
- [x] **Non-functional chevrons in Storage:** Added `item.detail.trim().length > 0` check to ContextItem. *(DONE — 2026-05-26)*
- [x] **Storage mounts leaking into unrelated sections:** Frontend bug — missing React keys on ContextList caused cross-section state reuse, and GlobalSearch `searchTextMap` had bare-ID key collisions across sections. Fixed with section-scoped keys. *(DONE — 2026-05-26)*

### Fleet Refine UI (2026-05-26 testing session)

- [x] **Warning banners too dark:** Replaced hardcoded backgrounds with `color-mix()` tints that work in both themes. *(DONE — 2026-05-26)*
- [x] **Fleet defaults to intersection:** Applied strict intersection default to all 14 section types — items below full prevalence start excluded but visible. *(DONE — 2026-05-26)*
- [x] **Fleet toggles broken:** Configs, services, drop-ins, and quadlets were recalculating include from raw prevalence instead of reading projected snapshot state. Fixed to use `entry.include`. *(DONE — 2026-05-26)*
- [x] **Tuned fleet data wrong:** Three fixes — merge hardcoded `true`, view hardcoded `true`, prevalence passed as `None`. Added `is_stock_tuned_profile()` recognizing 14 stock profiles; both single-host and fleet paths suppress stock defaults. *(DONE — 2026-05-26)*
- [x] **Prevalence badge contrast:** Replaced yellow-on-white text with tinted pill badges (green/amber/red) all exceeding WCAG AA 4.5:1. *(DONE — 2026-05-26)*

### Build Output Streaming (DONE — 2026-05-30)

- [x] **`inspectah build` swallowed podman output:** Stdout was piped to capture the image digest, which also captured all build step output. Fixed by using `--iidfile` to write the digest to a temp file, then inheriting both stdout and stderr so all podman output streams in real time. *(DONE — 2026-05-30)*

### RPM Performance (DONE — 2026-05-30)

- [x] **Baseline filter ran after DNF queries:** `packages_added` contained all host packages (~491), not just the delta (~120). Both `populate_source_repos` and `classify_leaf_auto` → `classify_deps_dnf` ran on everything. Fixed by filtering `packages_added` against baseline before expensive operations. Regression test guards the ordering. *(DONE — 2026-05-29)*
- [x] **Per-package DNF dep resolution (~6s/query):** `classify_deps_dnf` spawned one `dnf repoquery` per package. With 120 delta packages: 711s. Ported Go's `classifyDepsRpm` approach as primary: `rpm -qR` per package (~8ms) + `rpm --whatprovides` in batches of 50, BFS graph traversal in Rust. DNF is now fallback. *(DONE — 2026-05-30)*

### Code Formatting Cleanup (DONE — 2026-05-30)

- [x] **cargo fmt:** Ran `cargo fmt` across the entire Rust codebase to ensure consistent formatting. 109 files formatted. *(commit 8b31640 — 2026-05-30)*
