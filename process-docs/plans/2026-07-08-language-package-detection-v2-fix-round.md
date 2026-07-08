# Language Package Detection v2 — Fix Round

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** Fix all must-fix and should-fix findings from the team panel review (Thorn, Slate, Fern, Tang — all request-changes).

**Architecture:** Six logical fixes ordered by unblock priority. Task 1 first (corrupted snapshot data), then 2-5 can proceed, Task 6 last.

**Reviews:** `marks-inbox/reviews/2026-07-08-language-package-detection-v2-{thorn,slate,fern,tang}-review.md`

## Global Constraints

- `cargo clippy -- -W clippy::all` must pass with zero warnings on every commit
- `cargo fmt --check` must pass on every commit
- Leave the already-accepted Enter-on-listitem nit deferred unless it falls out for free
- Pre-merge matrix: `cargo test -p inspectah-collect`, `cargo test -p inspectah-cli`, `cargo test -p inspectah-refine package_pin`, `cargo test -p inspectah-web`, `cargo test -p inspectah-pipeline language_packages`, targeted UI tests, `make test` in driftify, then `cargo clippy --workspace -- -W clippy::all` and `cargo fmt --check`

---

### Task 1: npm-global prefix binding

**Must-fix.** npm-global collection is wrong on multi-prefix hosts.

**Files:**
- Modify: `crates/collect/src/inspectors/nonrpm.rs`

**Problem:** `npm list -g` is parsed once, then merged into every discovered prefix. A host with both `/usr/lib/node_modules` and `/usr/local/lib/node_modules` gets packages attributed to the wrong environment.

**Fix:** Bind `npm list -g` results only to the prefix returned by `npm root -g`; any additional discovered prefixes stay directory-walk-only and medium-confidence.

**Test:** Add a regression test where `/usr/lib/node_modules` is the npm root and `/usr/local/lib/node_modules` also exists — verify packages from `npm list` appear only in the npm-root prefix item, not the other.

**Verify:** `cargo test -p inspectah-collect -- npm_global`

---

### Task 2: Ship the live pin screen + UI contract gaps

**Must-fix (wiring) + should-fix (contract gaps folded in).**

**Files:**
- Modify: `crates/web/ui/src/App.tsx`
- Modify: `crates/web/ui/src/components/MainContent.tsx`
- Modify: `crates/web/ui/src/components/LanguagePackageList.tsx`
- Modify: `crates/web/ui/src/components/GlobalSearch.tsx`
- Modify: related test files

**Problem:** MainContent never passes `onSetPackagePin` or `onSetBulkPackagePin` to the component, so the entire pin interaction is dark in the live screen.

**Fix:**
1. Thread `setPackagePin()` and `setBulkPackagePin()` from the real screen through MainContent → LanguagePackageList
2. Pass section search text into LanguagePackageList
3. Keep the bulk button visible for one-package environments
4. Add version/path-specific accessible labels on checkboxes and bulk buttons
5. Fix row/expanded semantics per spec
6. Make package search hits land on the package row, not only the parent env

**Verify:** `npm test -- src/components/__tests__/LanguagePackageList.test.tsx src/components/__tests__/SectionPlumbing.test.tsx src/components/__tests__/GlobalSearch.test.tsx`

---

### Task 3: Fix scan-root assembly semantics + scan.rs should-fixes

**Must-fix (dedupe) + should-fix (folded in).**

**Files:**
- Modify: `crates/cli/src/commands/scan.rs`

**Problem:** Duplicate suppression uses raw `starts_with` string checks — `/var/www2` treated as covered by `/var/www`, `/opt-app/appuser` treated as covered by `/opt`.

**Fix:**
1. Replace string `starts_with` dedupe with component-aware path checks (ensure the character after the prefix match is `/` or end-of-string)
2. Add `METHOD_NPM_GLOBAL` to Tier-1 language_env_paths so `--include-unmanaged` doesn't double-count npm globals
3. Preserve `scan_home_users=["all"]` sentinel when `--scan-home all` was used
4. Return a `Result` from `build_scan_roots()` instead of calling `process::exit(1)`

**Test:** Add sibling-prefix tests (`/var/www` vs `/var/www2`, `/opt` vs `/opt-app`).

**Verify:** `cargo test -p inspectah-cli`

---

### Task 4: Contain recursive walkers to requested roots

**Must-fix.** Symlink following without containment or cycle checks.

**Files:**
- Modify: `crates/collect/src/inspectors/nonrpm.rs`
- Reference: `crates/collect/src/inspectors/config/walk.rs` (existing symlink containment logic)

**Problem:** The new `--scan-home` / `--scan-path` surface follows directory symlinks without containment or cycle checks. On user-controlled trees, this can overscan outside intended roots or hang in loops.

**Fix:** Before recursing into a directory, detect directory symlinks, resolve final targets, require them to stay under the active root, and keep a visited-inode set so self-loops fail closed. Reuse or extract the symlink-containment logic from `config/walk.rs`.

**Test:** Add targeted collector tests for self-loop symlinks and outside-root symlinks.

**Verify:** `cargo test -p inspectah-collect`

---

### Task 5: Make driftify fixtures portable + missing coverage

**Must-fix (portability) + should-fix (coverage gap).**

**Files:**
- Modify: `driftify/driftify.py`
- Modify: `driftify/tests/test_driftify.py`

**Problem:** C-extension fixtures hardcode `python3.9` paths. Also missing `/var/www` Python/venv fixture.

**Fix:**
1. Resolve the active venv's real `site-packages` path before writing `.so` stubs (use `sysconfig.get_paths()` or similar)
2. Add `/var/www` Django venv fixture so default-root coverage includes both Node and pip paths

**Verify:** `make test` in driftify

---

### Task 6: Renderer hardening sweep

**Should-fix.** Shell injection surface in rendered commands.

**Files:**
- Modify: `crates/pipeline/src/render/language_packages.rs`

**Problem:** Expanded scan roots can contain characters that produce misparsed `RUN` lines in the rendered Containerfile. Path and package strings are interpolated raw into shell commands.

**Fix:** Quote or reject unsafe path/package tokens at the snapshot-to-shell boundary. At minimum, single-quote paths in `RUN` lines. Reject paths containing `'` (single quote) or newlines.

**Verify:** `cargo test -p inspectah-pipeline -- language_packages`

---

## Task Dependency Graph

```
T1 (npm-global prefix) → first
T2 (live pin screen) — after T1 or parallel
T3 (scan-root assembly) — after T1 or parallel
T4 (symlink containment) — after T1 or parallel
T5 (driftify portable) — after T1 or parallel
T6 (renderer hardening) — last
```
