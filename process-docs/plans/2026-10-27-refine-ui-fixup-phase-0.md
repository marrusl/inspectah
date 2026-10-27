# Refine UI Fix-up, Phase 0 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship phase 0 of the refine single-host UI fix-up: the Rust correctness change set, `SCHEMA_VERSION` 23, and the three small frontend edits that keep the existing web UI compatible with the new server contract.

**Architecture:** One shippable change set with one schema bump. The schema constants move first so every later type change is coherent; then the typed domain models replace the stringly-typed ones (`RepoId`/`RepoAttribution`, `FcontextRule`, `VarDirBacking`), then the session and renderer rules that read them, then the web DTOs, then the frontend compatibility edits. The two unrun host checks (B1, B2) are merge gates and run first, because a failure re-opens § 3.1 or § 3.4 before any of the code is written.

**Tech Stack:** Rust 2024 workspace (`crates/core`, `crates/collect`, `crates/pipeline`, `crates/cli`, `crates/web`, `crates/refine`, `crates/tui`), serde, `thiserror`, `tempfile`, `insta` snapshot tests, axum 0.8, React + TypeScript + PatternFly web UI under `crates/web/ui/`, Vitest + Testing Library, Playwright.

**Spec:** `process-docs/specs/proposed/2026-10-26-refine-ui-fixup.md`, committed at `87498e4b`. The spec is authoritative. Where this plan and the spec disagree, the spec wins and the plan is the bug. Code citations in the spec are at `6e8a52b1`; `git diff --stat 6e8a52b1..HEAD` touches only the spec file, so every citation holds at HEAD.

**Scope:** The § 7 phasing table's row "0. Correctness and the bump (Rust, plus the frontend repo compatibility edits)" and nothing else. Phases 1, 2, and 3 are separate web releases with their own plans. Two adjacent items are explicitly **not** in this plan and must not be absorbed into any task: the pre-existing unmanaged-extraction path-traversal issue (its own backlog item, its own fix) and the deferred sidebar sections reorganization. Everything in spec § 9 "Out of scope" stays out.

**Target release:** v0.9.0-beta.4.

---

## Global Constraints

Exact values copied from the spec. Every task's requirements implicitly include this section.

- **Clippy clean:** zero warnings. Non-negotiable (repo CLAUDE.md). Read the clippy constraint below for which invocation a task's "clippy clean" acceptance means.
- **Format:** `cargo fmt --check` must pass before every commit.
- **Pre-commit hook:** `.githooks/pre-commit` gates on `cargo fmt --check`, `cargo clippy --workspace -- -W clippy::all`, and `cargo test`, and fast-paths out when no `.rs`/`Cargo.*`/`build.rs` file is staged. New clones need `git config core.hooksPath .githooks` (already set in this clone).
- **Toolchain, and the PATH requirement.** Verified on this machine at `815e65fb` on 2026-10-27: `rustc`/`cargo` 1.99.1 stable (the workspace is edition 2024, which clears), `node` v26.12.1, `npm` 11.20.0. **`cargo` is not on a default shell's PATH here.** `brew install rustup` puts only `rustup` in `/home/linuxbrew/.linuxbrew/bin`; the `cargo`, `rustc`, `cargo-clippy`, and `rustfmt` shims live in `/home/linuxbrew/.linuxbrew/opt/rustup/bin`, which is not on PATH, and `~/.cargo/bin` does not exist. Every step below writes `cargo` bare. Before running any of them, either prepend the shim directory once per shell:

  ```bash
  export PATH="/home/linuxbrew/.linuxbrew/opt/rustup/bin:$PATH"
  ```

  or read each bare `cargo ...` as `rustup run stable cargo ...`. `node` and `npm` are already on PATH and need nothing.
- **Clippy coverage gap, and the baseline at `815e65fb`.** Phase 0 adds a large amount of new test code, and neither gate sees it. The hook and CI (`.github/workflows/rust-ci.yml:44`) both run `cargo clippy --workspace -- -W clippy::all`, with **no `--all-targets`**, so neither lints a test target; and `-W` warns rather than denies, so the command exits 0 on warnings and the hook's `if ! cargo clippy` blocks only on hard errors. A task's **"clippy clean" acceptance means `cargo clippy --workspace --all-targets -- -D clippy::all` reports nothing beyond the baseline below**, which is stricter than either gate and is what catches lint in the new tests.

  Measured baseline at `815e65fb`, all pre-existing and caused by no phase 0 work. `cargo clippy --workspace -- -W clippy::all` is **clean, 0 warnings**. `cargo clippy --workspace --all-targets` reports **9 warnings, every one in test code**: `bool_assert_comparison` x5, `field_reassign_with_default` x2, `needless_range_loop` x1, `op_ref` x1. Per target: `inspectah-web` test `contract_snapshots` 4, `inspectah-web` lib test 1, `inspectah-pipeline` lib test 2, `inspectah-cli` bin test 2. By location, four are in `crates/web/tests/contract_snapshots.rs` (`:334`, `:340`, `:348`, `:354`) and five are in `#[cfg(test)]` modules inside `src/`: `crates/cli/src/commands/scan.rs:1516`, `crates/cli/src/progress/pretty.rs:1150`, `crates/pipeline/src/render/repoless.rs:87`, `crates/pipeline/src/render/unmanaged.rs:100`, `crates/web/src/handlers.rs:2083`. Task 23 edits `contract_snapshots.rs` and will see those four.

  Do not mistake any of the nine for your own regressions, and **do not fix them**: they are outside this spec, as are the hook and the CI workflow, which this plan does not change. `cargo fmt --check` and `cargo test --workspace` are both green at `815e65fb`.
- **Commit format:** `type(scope): description`, imperative mood. Attribution trailer `Assisted-by: Claude Code (<model>)`. **No team member names anywhere in commits or code** (public repo).
- **Never push.** Commit freely; the maintainer pushes.
- **Schema constants:** `SCHEMA_VERSION = 23`, `MIN_SCHEMA = 23` (`crates/core/src/snapshot.rs:21`, `:103`). Exact match, no migration, re-scan. Aggregates re-aggregate from re-scanned hosts.
- **Named constants with exact values:** `MAX_UPLOAD_FILENAME_LEN: usize = 200`; `UPLOAD_LIMIT_BYTES` = 500 MiB (`524_288_000`); `VAR_SYMLINKED_TOPLEVELS: &[(&str, &str)] = &[("/srv", "/var/srv")]`; `RELEASE_PERMALINKS: &[(&str, &str)] = &[("epel-release", "https://dl.fedoraproject.org/pub/epel/epel-release-latest-{major}.noarch.rpm")]`.
- **Upload filename allowlist (§ 3.3):** every byte in `[A-Za-z0-9._+~^-]`, checked with `u8::is_ascii_alphanumeric` and never `char::is_alphanumeric`; first byte `[A-Za-z0-9]`; ends in `.rpm` case-insensitively; length 5 to `MAX_UPLOAD_FILENAME_LEN` bytes. Rejection is always rejection, never stripping.
- **fcontext operand validation (§ 3.1 rule 4):** every host-supplied operand single-quoted. Equivalence operands must begin with `/`, contain no whitespace, and contain none of `* ? [ ] ( ) | \`. A label `spec` must begin with `/`. `type_`, `user`, and `level` must match `^[A-Za-z0-9_.:-]+$`. A rule failing any check renders the two-line FIXME, never a command.
- **Diagnostics are data (§ 3.1 rule 4):** a message the emitted commands print at build time is a static `printf` format written by the renderer with no host bytes in it; each operand is its own argument, single-quoted when it is a validated literal and `"$var"` when it is a shell variable. Never `echo` with an interpolated double-quoted string.
- **FIXME wording rule (§ 5.6):** every `# FIXME:` and `# MANUAL:` block starts with an imperative instruction and puts the cause after a semicolon or on the next comment line; a block never ends on the failure statement. Rewrite existing violations only where this plan already touches them (fcontext, tuned, the GPG key path at `containerfile.rs:357-368`).
- **Voice rules for user-visible strings:** no em dashes; do not use "immutable" (say "image-based" or "read-only"); do not use "shape" as a noun. `CHANGELOG.md` keeps the repo's own `- **Label** — description` separator per `process-docs/changelog-policy.md`; the prose after it follows the voice rules.
- **Package identity is always `name.arch`**, never a bare name (`process-docs/skills/package-identity-is-name-dot-arch.md`).
- **Mode-divergence rule:** behaviour added here is checked in both `RefineMode::SingleHost` and `RefineMode::Aggregate` where the surface exists in both (`process-docs/skills/aggregate-vs-single-host-behavioral-split.md`).
- **Release-compatibility contract (§ 7):** no control is offered for an op the server refuses, and no offered control posts one. The three frontend tasks exist only to hold that line.

## Review Focus

Input classes the spec implies but whose behaviour no § 8 entry pins. Each line's test is added to the task named, in that task's own step style.

1. **`RepoId` case folding must be ASCII-only and consistent across `Eq`, `Hash`, and `Ord`.** § 3.2 says equality is `eq_ignore_ascii_case` and "Hash and Ord over the lowercased form". If `Hash` uses Unicode `to_lowercase()` while `Eq` uses ASCII folding, two ids that compare unequal can hash equal and a `HashMap` silently loses an entry. Test in Task 4.
2. **`parse_repo_sections` on dnf's full `enabled=` vocabulary and on malformed INI.** It replaces two existing parsers and nothing in § 8 feeds it `enabled=yes`, `enabled = True`, a trailing comment, or a line outside any section. Test in Task 4.
3. **The `Repo` and `Group` no-op arms when the projected snapshot has no `rpm` section.** § 3.7 names `unwrap_or(true)` for `Group` and § 3.2 says nothing for `Repo`; a repo op on a snapshot with no rpm section must refuse, not cascade over an empty set as if it had changed something. Test in Task 7.
4. **`effective_var_backing` on a snapshot with no `config` section and no `rpm` section.** The § 3.4 table's first row reads "the `config.files` entry for `conf`"; with no config section there is no entry, and the function must yield `Provision`, not panic or index. Test in Task 19.
5. **`validate_staged_name` at both length boundaries.** § 8.2 tests a 201-byte name but neither the 200-byte acceptance nor the 5-byte minimum, and `a.rpm` is exactly 5. An off-by-one here rejects legal vendor filenames. Test in Task 12.

---

## § 4 Consumer Checklist, Task Map

Spec § 4's consumer table says what phase 0 does at each site that names a removed field or changed type. The compiler finds them; this map says which task owns each, so the checklist lands coherently instead of scattered. Task 26 walks this table as its acceptance.

| § 4 consumer row | Owning task |
|---|---|
| Aggregate merge, fcontext (`merge.rs:1608-1609`) | Task 9 |
| Aggregate merge, kernel (`merge.rs:1685-1695`, `:1731-1755`) | Task 21 |
| Aggregate merge, PAM and audit (`merge.rs:1619-1660`) | Task 20 |
| Aggregate merge, storage (`merge.rs:1512`) | Task 17 |
| Classification (`classify.rs:782-785`) | Task 20 |
| Redaction (`engine.rs:790-805`, `:990-1028`) | Task 21 |
| HTML report (`report.rs:910-917`) | Task 10 |
| Audit report (`audit.rs:593-625`) | Tasks 10 (fcontext), 18 (storage), 20 (PAM/audit), 16 (repo-enabling) |
| TUI (`sections.rs:254-263`, `screen/single_host.rs:1030`) | Tasks 17 (storage row), 21 (kernel counts) |
| Web projections (`reference.rs:549-550`, `:863-897`; `adapter.rs:949-968`, `:1293-1310`) | Tasks 18 (storage), 20 (PAM pointers), 21 (snippet items) |
| `load_for_refine` (`normalize.rs:54-72`) | Unchanged; asserted in Task 3 |

---

## File Structure

**Created (Rust):**
- `crates/refine/src/rpm_filename.rs` — `RpmFilename::parse`, `validate_staged_name`, `NameError`, `MAX_UPLOAD_FILENAME_LEN`. Moved out of `session.rs:3554-3600` because four call sites outside the session now need it (the upload handler, the rescan, the export pre-pass, the dnf-cache NEVRA path).
- `crates/pipeline/src/render/selinux.rs` — `selinux_equivalences()`, the one helper the Containerfile, audit report, and HTML report all read so a synthesized or conflicting equivalence is reported the same way everywhere.
- `crates/pipeline/src/render/var_dirs.rs` — `effective_var_backing()`, the render-time reconciliation of the source-host backing verdict against what the refined export carries.

**Created (tests):**
- `crates/pipeline/tests/configtree_single_source_test.rs`
- `crates/refine/tests/upload_export_test.rs`
- `crates/refine/tests/repo_index_test.rs`
- `crates/pipeline/tests/var_backing_export_test.rs`
- `crates/pipeline/tests/selinux_block_shell_test.rs`
- `crates/cli/tests/scan_shadow_test.rs`
- `crates/web/tests/decision_surface_test.rs`

**Created (frontend tests):**
- `crates/web/ui/src/components/__tests__/MainContent.repo.test.tsx`
- `crates/web/ui/src/components/aggregate/__tests__/AggregateApp.repo.test.tsx`
- `crates/web/ui/e2e/fixtures/sequences/repo-disagree/` (8 JSON fixtures)

**Modified, by area:** core types (`selinux.rs`, `repo.rs`, `storage.rs`, `rpm.rs`, `config.rs`, `kernelboot.rs`, `snapshot.rs`, `aggregate/merge.rs`); collect inspectors (`selinux.rs`, `storage.rs`, `kernelboot.rs`, `rpm/source_repos.rs`, `rpm/repoless.rs`, `config/{walk,mod,classify}.rs`); pipeline (`collect.rs`, `render/{configtree,containerfile,repoless,service_intent,report,audit,readme}.rs`, `redact/engine.rs`); refine (`session.rs`, `repo_index.rs`, `types.rs`, `normalize.rs`, `projection/{reference,types}.rs`, `group_state.rs`); web (`handlers.rs`, `adapter.rs`, `upload.rs`, `web_types.rs`, `lib.rs`, `error.rs`, `aggregate_handlers.rs`); tui (`sections.rs`, `screen/single_host.rs`, `app.rs`); frontend (`RepoBar.tsx`, `RepoGroupHeader.tsx`, `MainContent.tsx`, `AggregateApp.tsx`, `api/types.ts`).

---

## Merge Gates

Tasks 1 and 2 are **merge gates, not implementation tasks**. Neither has ever been run. Spec § 8.14: "the spec is not verified on these points until they are." Both must pass before phase 0 merges. They are placed first so that a failure re-opens § 3.1 or § 3.4 before the code that depends on them is written. **Do not start Task 3 until both have run and their results are recorded in this document.**

### Task 1: MERGE GATE B1 — SELinux replay against the target base

**Implements § 3.1 (positions 23, 24, 30). Satisfies § 8.14, the B1 half.**

Runs the forms § 5.1 emits against the real `semanage` on the real target base. Nothing in the Rust change set is correct if these fail.

**Files:**
- Modify (results only): this plan document, the "Result" line under each sub-case.

**Interfaces:**
- Consumes: nothing.
- Produces: a go/no-go for § 3.1. A differing result on any sub-case re-opens § 3.1 before merge; Task 9 and Task 10 do not start until this reads PASS.

**Mechanics note.** The spec writes the gate as `podman run --rm registry.redhat.io/rhel9/rhel-bootc:9.8 sh -c '...'`. The emitted commands contain single quotes (`spec='...'`, `awk -v s='...'`), which cannot survive nesting inside `sh -c '...'`. Each sub-case below therefore pipes the identical script through `sh -s` on stdin, which keeps the emitted bytes byte-identical to what the renderer writes. The image, the commands, and the expectations are the spec's.

- [ ] **Step 1: Confirm the tooling the emitted commands need exists in the base**

```bash
podman run --rm registry.redhat.io/rhel9/rhel-bootc:9.8 sh -c 'command -v semanage matchpathcon awk grep printf; echo "exit=$?"'
```

Expected: paths for `semanage`, `matchpathcon`, `awk`, and `grep` (`printf` is a shell builtin and may print nothing), exit 0.

**Re-open condition:** if `semanage` is absent from the base, the `RUN semanage ...` lines § 5.1 emits cannot run as specified and **§ 3.1 is re-opened before merge** — the rendered block would need a package-install precondition the spec does not describe. If only `matchpathcon` is missing, install `libselinux-utils` inside the sub-case (iii) and (iv) containers and note it; the oracle still holds.

Result: _not yet run_

- [ ] **Step 2: Sub-case (i) — stock expression, delete-then-add-or-modify**

```bash
podman run --rm -i registry.redhat.io/rhel9/rhel-bootc:9.8 sh -s <<'B1I'
set -u
spec='/var/www(/.*)?'; \
    eqs="$(semanage fcontext -l -C)" || { printf 'FIXME: could not list the target file contexts before replaying the rule for %s; fix semanage in the base, or exclude this rule, then rebuild\n' "$spec" >&2; exit 1; }; \
    if printf '%s\n' "$eqs" | awk '$2 == "=" { print $1 }' | grep -qxF -- "$spec"; then \
      printf 'FIXME: skipped the file context rule for %s; the target maps that path as an equivalence, which semanage fcontext -d would delete instead of a label; delete it with semanage fcontext -d -e, or exclude this rule, then rebuild\n' "$spec" >&2; \
    else semanage fcontext -d "$spec" 2>/dev/null || true; \
      semanage fcontext -a -t public_content_t "$spec" \
      || semanage fcontext -m -t public_content_t "$spec"; fi
echo "exit=$?"
semanage fcontext -l -C
B1I
```

Expected: exit 0. `-d` fails on a key defined in policy and is swallowed by `2>/dev/null || true`; `-a` fails because `/var/www(/.*)?` is a stock expression; `-m` succeeds. The final listing shows `/var/www(/.*)?` with `public_content_t`.

**Re-open condition:** the spec allows exactly one surprise here — if a RHEL downstream `semanage` accepts the duplicate `-a`, the `|| -m` fallback stays (harmless) and § 3.1 rule 1 gains a note on the downstream behaviour. Any other failure re-opens § 3.1. "The duplicate-add allowance is not permission to ignore any other failure" (§ 8.14).

Result: _not yet run_

- [ ] **Step 3: Sub-case (ii) — equivalence added, then identical**

```bash
podman run --rm -i registry.redhat.io/rhel9/rhel-bootc:9.8 sh -s <<'B1II'
set -u
for pass in 1 2; do
  eq="$(semanage fcontext -l -C | awk -v s='/var/srv' '$2 == "=" && $1 == s { print $3 }')"; \
      if [ -z "$eq" ]; then semanage fcontext -a -e '/srv' '/var/srv'; \
      elif [ "$eq" != '/srv' ]; then printf 'FIXME: %s is already equivalent to %s on the target, not %s; delete that equivalence with semanage fcontext -d -e, or exclude this rule, then rebuild\n' '/var/srv' "$eq" '/srv' >&2; exit 1; fi
  echo "pass${pass} exit=$?"
done
semanage fcontext -l -C | awk '$2 == "=" && $1 == "/var/srv"'
B1II
```

Expected: `pass1 exit=0`, `pass2 exit=0`, and exactly one `/var/srv = /srv` line in the final listing. The second pass finds it identical and runs no `semanage` mutation.

**Re-open condition:** a second entry, a nonzero exit, or a mutation on pass 2 re-opens § 3.1 rule 2.

Result: _not yet run_

- [ ] **Step 4: Sub-case (iii) — two overlapping rules in reversed specificity**

```bash
podman run --rm -i registry.redhat.io/rhel9/rhel-bootc:9.8 sh -s <<'B1III'
set -u
replay() {
  spec="$1"; type="$2"
  eqs="$(semanage fcontext -l -C)" || { printf 'FIXME: could not list the target file contexts before replaying the rule for %s; fix semanage in the base, or exclude this rule, then rebuild\n' "$spec" >&2; exit 1; }
  if printf '%s\n' "$eqs" | awk '$2 == "=" { print $1 }' | grep -qxF -- "$spec"; then
    printf 'FIXME: skipped the file context rule for %s; the target maps that path as an equivalence, which semanage fcontext -d would delete instead of a label; delete it with semanage fcontext -d -e, or exclude this rule, then rebuild\n' "$spec" >&2
  else
    semanage fcontext -d "$spec" 2>/dev/null || true
    semanage fcontext -a -t "$type" "$spec" || semanage fcontext -m -t "$type" "$spec"
  fi
}
semanage fcontext -a -e '/srv' '/var/srv'
replay '/srv/www(/.*)?' httpd_sys_content_t; echo "A exit=$?"
replay '/srv/.*' public_content_t; echo "B exit=$?"
matchpathcon /var/srv/www/index.html
matchpathcon /srv/www/index.html
B1III
```

Expected: both replays exit 0, and both `matchpathcon` paths report the **later** rule's type, `public_content_t`, and agree with each other. (`replay` is a shell function wrapping the identical emitted body; the renderer writes the body inline per rule. The equivalence is added first so the two paths can agree.)

**Re-open condition:** if the two paths disagree, or the winner is `httpd_sys_content_t`, § 3.1 rule 1's recency claim is wrong and § 3.1 is re-opened.

Result: _not yet run_

- [ ] **Step 5: Sub-case (iv) — preseeded overlapping local key, `matchpathcon` oracle**

```bash
podman run --rm -i registry.redhat.io/rhel9/rhel-bootc:9.8 sh -s <<'B1IV'
set -u
replay() {
  spec="$1"; type="$2"
  eqs="$(semanage fcontext -l -C)" || { printf 'FIXME: could not list the target file contexts before replaying the rule for %s; fix semanage in the base, or exclude this rule, then rebuild\n' "$spec" >&2; exit 1; }
  if printf '%s\n' "$eqs" | awk '$2 == "=" { print $1 }' | grep -qxF -- "$spec"; then
    printf 'FIXME: skipped the file context rule for %s; the target maps that path as an equivalence, which semanage fcontext -d would delete instead of a label; delete it with semanage fcontext -d -e, or exclude this rule, then rebuild\n' "$spec" >&2
  else
    semanage fcontext -d "$spec" 2>/dev/null || true
    semanage fcontext -a -t "$type" "$spec" || semanage fcontext -m -t "$type" "$spec"
  fi
}
semanage fcontext -a -t var_t '/srv/.*'
replay '/srv/www(/.*)?' httpd_sys_content_t; echo "A exit=$?"
replay '/srv/.*' public_content_t; echo "B exit=$?"
echo '--- listing ---'; semanage fcontext -l -C
echo '--- store order ---'; cat /etc/selinux/targeted/contexts/files/file_contexts.local
echo '--- oracle ---'; matchpathcon /srv/www/index.html
B1IV
```

Expected: both replays exit 0; the listing shows `/srv/.*` with the source's `public_content_t`, not the preseeded `var_t`; `file_contexts.local` lists A's line before B's; and the oracle, `matchpathcon /srv/www/index.html`, gives **`public_content_t`** — the source winner, not `httpd_sys_content_t`. The listing is sorted by expression and cannot show replay order, which is why `matchpathcon` is the oracle and `file_contexts.local` only the secondary check.

**Re-open condition:** if `matchpathcon` names `httpd_sys_content_t`, or `file_contexts.local` lists B before A, § 3.1 is wrong and is re-opened before merge.

Result: _not yet run_

- [ ] **Step 6: Sub-case (v) — conflicting target equivalence stops the build**

```bash
podman run --rm -i registry.redhat.io/rhel9/rhel-bootc:9.8 sh -s <<'B1V'
set -u
semanage fcontext -a -e '/opt/srv' '/var/srv'
eq="$(semanage fcontext -l -C | awk -v s='/var/srv' '$2 == "=" && $1 == s { print $3 }')"; \
    if [ -z "$eq" ]; then semanage fcontext -a -e '/srv' '/var/srv'; \
    elif [ "$eq" != '/srv' ]; then printf 'FIXME: %s is already equivalent to %s on the target, not %s; delete that equivalence with semanage fcontext -d -e, or exclude this rule, then rebuild\n' '/var/srv' "$eq" '/srv' >&2; exit 1; fi
echo "exit=$?"
semanage fcontext -l -C | awk '$2 == "="'
B1V
```

Expected: nonzero exit; stderr carries exactly

```
FIXME: /var/srv is already equivalent to /opt/srv on the target, not /srv; delete that equivalence with semanage fcontext -d -e, or exclude this rule, then rebuild
```

and the final listing still shows `/var/srv = /opt/srv` with no `/srv` mapping.

**Re-open condition:** if the mapping was replaced, § 3.1 rule 2 is wrong and is re-opened.

Result: _not yet run_

- [ ] **Step 7: Sub-case (vi) — a target equivalence subject under a label spec is refused without mutation**

```bash
podman run --rm -i registry.redhat.io/rhel9/rhel-bootc:9.8 sh -s <<'B1VI'
set -u
replay() {
  spec="$1"; shift
  eqs="$(semanage fcontext -l -C)" || { printf 'FIXME: could not list the target file contexts before replaying the rule for %s; fix semanage in the base, or exclude this rule, then rebuild\n' "$spec" >&2; exit 1; }
  if printf '%s\n' "$eqs" | awk '$2 == "=" { print $1 }' | grep -qxF -- "$spec"; then
    printf 'FIXME: skipped the file context rule for %s; the target maps that path as an equivalence, which semanage fcontext -d would delete instead of a label; delete it with semanage fcontext -d -e, or exclude this rule, then rebuild\n' "$spec" >&2
  else
    semanage fcontext -d "$@" "$spec" 2>/dev/null || true
    semanage fcontext -a "$@" "$spec" || semanage fcontext -m "$@" "$spec"
  fi
}
semanage fcontext -a -e '/srv' '/var/srv'
replay '/var/srv' -t var_t;            echo "label exit=$?"
replay '/var/srv' -t '<<none>>';       echo "unlabel exit=$?"
replay '/var/srv' -f d -t var_t;       echo "ftype exit=$?"
echo '--- listing ---'; semanage fcontext -l -C
replay '/var/srv/www(/.*)?' -t httpd_sys_content_t; echo "descendant exit=$?"
echo '--- listing ---'; semanage fcontext -l -C
B1VI
```

Expected: the three `/var/srv` replays each exit 0 with the rule 1 skip FIXME on stderr naming `/var/srv`, and the listing still shows `/var/srv = /srv` with **no** `/var/srv` label row. The descendant spec `/var/srv/www(/.*)?` does **not** fire the guard; `semanage` refuses `-a` and `-m` with its equivalency-conflict message, that replay exits nonzero, and the equivalence survives.

**Re-open condition:** if the equivalence is gone or a `/var/srv` label row appears, § 3.1 is wrong and is re-opened before merge.

Result: _not yet run_

- [ ] **Step 8: Record the verdict**

Write PASS or the failing sub-case and its observed output into each Result line above, and commit this document alone.

```bash
git add process-docs/plans/2026-10-27-refine-ui-fixup-phase-0.md
git commit -m "docs(plans): record the B1 SELinux replay results"
```

---

### Task 2: MERGE GATE B2 — tmpfiles creation in a clean root

**Implements § 3.4 (the exact-creator backing rule). Satisfies § 8.14, the B2 half.**

**Files:**
- Modify (results only): this plan document, the "Result" line below.

**Interfaces:**
- Consumes: nothing.
- Produces: a go/no-go for § 3.4. Task 17 and Task 18 do not start until this reads PASS.

Run inside the target base rather than on the workstation, so the systemd under test is the 252 line § 3.4 cites rather than whatever the host ships.

- [ ] **Step 1: Run the check with the ownership identity prepared**

```bash
podman run --rm -i registry.redhat.io/rhel9/rhel-bootc:9.8 sh -s <<'B2'
set -eu
ROOT=$(mktemp -d)
mkdir -p "$ROOT/etc"
# Ownership-identity preparation (§ 8.14): systemd-tmpfiles --root resolves
# user and group names against the root's own passwd/group, so a missing
# appone account would turn the path assertion into an ownership failure.
printf 'root:x:0:0:root:/root:/sbin/nologin\nappone:x:4242:4242::/var/lib/appone:/sbin/nologin\n' > "$ROOT/etc/passwd"
printf 'root:x:0:\nappone:x:4242:\n' > "$ROOT/etc/group"
mkdir -p "$ROOT/etc/tmpfiles.d"
printf 'd /var/lib/appone/cache 0750 appone appone -\n' > "$ROOT/etc/tmpfiles.d/appone.conf"
systemd-tmpfiles --create --root="$ROOT"
echo "exit=$?"
echo "cache:"; test -d "$ROOT/var/lib/appone/cache" && echo PRESENT || echo ABSENT
echo "sub:";   test -e "$ROOT/var/lib/appone/cache/sub" && echo PRESENT || echo ABSENT
ls -ld "$ROOT/var/lib/appone/cache"
B2
```

Expected: `cache: PRESENT` and `sub: ABSENT`. That is the whole claim: a `d` line creates exactly its own path and nothing below it, which is why § 3.4 deletes the ancestor walk outright rather than narrowing it.

**Re-open condition:** anything else, and **§ 3.4 is wrong and is re-opened before merge**.

Result: _not yet run_

- [ ] **Step 2: Record the verdict and commit**

```bash
git add process-docs/plans/2026-10-27-refine-ui-fixup-phase-0.md
git commit -m "docs(plans): record the B2 tmpfiles creation result"
```

---

## Rust Change Set

### Task 3: The schema bump

**Implements § 4 (the single schema bump). Satisfies § 8.10, the `crates/core/tests/snapshot_test.rs` version half: "a snapshot JSON declaring `schema_version: 22` fails `load` with `UnsupportedVersion(22)`."**

The bump lands first so every later type change in this plan is coherent on disk. Removed fields and changed types make 22 unloadable by definition, so the window closes to an exact match.

**Files:**
- Modify: `crates/core/src/snapshot.rs:21` (`SCHEMA_VERSION`), `:103` (`MIN_SCHEMA`)
- Modify (fixtures pinning an old version, all move to 23): `crates/cli/src/commands/aggregate.rs` (6 sites), `crates/cli/tests/refine_e2e_test.rs:15`, `crates/refine/tests/normalize_test.rs` (10 sites at 21), `crates/refine/tests/cross_crate_integration_test.rs` (the `16` site), `crates/refine/tests/tarball_test.rs` (the `12` site)
- Test: `crates/core/tests/snapshot_test.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `inspectah_core::snapshot::SCHEMA_VERSION == 23`; `load()` rejects anything but 23 with `SnapshotError::UnsupportedVersion(u32)`.

Leave alone the fixtures that pin a deliberately-invalid version to exercise the rejection path (`99`, `999`, the `16` and `17` cases inside `snapshot.rs` itself, and `normalize_test.rs`'s "schema_version 16 must be rejected" case): they test the gate, not the format.

- [ ] **Step 1: Write the failing tests** in `crates/core/tests/snapshot_test.rs`

```rust
#[test]
fn schema_22_is_rejected() {
    let json = r#"{"schema_version": 22, "meta": {}, "system_type": "package-mode",
        "preflight": {"status": "ok"}, "warnings": [], "redactions": []}"#;
    match InspectionSnapshot::load(json) {
        Err(SnapshotError::UnsupportedVersion(v)) => assert_eq!(v, 22),
        other => panic!("expected UnsupportedVersion(22), got {other:?}"),
    }
}

#[test]
fn schema_23_round_trips() {
    let snap = InspectionSnapshot { schema_version: SCHEMA_VERSION, ..Default::default() };
    let json = serde_json::to_string(&snap).unwrap();
    assert_eq!(InspectionSnapshot::load(&json).unwrap().schema_version, 23);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p inspectah-core --test snapshot_test`
Expected: FAIL — `schema_22_is_rejected` gets `Ok(_)` because `MIN_SCHEMA` is 21.

- [ ] **Step 3: Set `SCHEMA_VERSION = 23` and `MIN_SCHEMA = 23` in `crates/core/src/snapshot.rs`**

- [ ] **Step 4: Move every fixture in the Files list to `"schema_version": 23`**

- [ ] **Step 5: Run the full workspace suite**

Run: `cargo test --workspace`
Expected: PASS. Any remaining `UnsupportedVersion` failure is a fixture this plan did not list; fix it here rather than deferring, because it is part of the bump.

- [ ] **Step 6: Commit**

```bash
git add crates/
git commit -m "feat(core)!: bump the snapshot schema to 23 with exact-match loading"
```

---

### Task 4: `RepoId`, `RepoAttribution`, and one repo-section parser

**Implements § 3.2 (types and the normalization rule), open positions 4 and 2. Satisfies § 8.4 in full, and the `parse_repo_sections` input classes of Review Focus items 1 and 2.**

Replaces the order-dependent first-substring-hit relabeling that produced `EPEL_10_Everything_x86_64`, and collapses the two INI parsers (`extract_section_ids` in `source_repos.rs`, `parse_repo_sections` in `repo_index.rs`) into one.

**Files:**
- Modify: `crates/core/src/types/repo.rs` (add `RepoId`, `PseudoRepo`, `RepoAttribution`, `RepoSectionDef`, `parse_repo_sections`)
- Modify: `crates/core/src/types/rpm.rs` (`PackageEntry` gains `repo_attribution`)
- Modify: `crates/collect/src/inspectors/rpm/source_repos.rs:184-207` (`normalize_source_repos`; delete `extract_section_ids`)
- Test: `crates/collect/src/inspectors/rpm/source_repos.rs` `#[cfg(test)]`, `crates/core/src/types/repo.rs` `#[cfg(test)]`

**Interfaces:**
- Consumes: Task 3's `SCHEMA_VERSION`.
- Produces:
  - `pub struct RepoId(String)` — `#[serde(transparent)]`; `PartialEq`/`Eq` via `eq_ignore_ascii_case`; `Hash` and `Ord` over `to_ascii_lowercase()`; `Display` keeps the id as the host wrote it. Constructor `RepoId::new(impl Into<String>) -> RepoId`, accessor `RepoId::as_str(&self) -> &str`.
  - `pub enum PseudoRepo { Commandline, Anaconda, System }`
  - `pub enum RepoAttribution { Exact { id: RepoId }, Resolved { recorded: String, id: RepoId }, Ambiguous { recorded: String, candidates: Vec<RepoId> }, Stale { recorded: String }, Pseudo { which: PseudoRepo, recorded: String }, Unrecorded }` — `#[serde(tag = "kind", rename_all = "snake_case")]`, with `pub fn recorded(&self) -> &str`.
  - `pub struct RepoSectionDef { pub id: RepoId, pub enabled: bool, pub has_source: bool, pub gpg_key_paths: Vec<String> }`
  - `pub fn parse_repo_sections(content: &str) -> Vec<RepoSectionDef>`
  - `PackageEntry.repo_attribution: RepoAttribution`, `#[serde(default)]`, default `Unrecorded`. `source_repo: String` keeps its meaning: the resolved id for `Exact` and `Resolved`, the recorded value otherwise.

- [ ] **Step 1: Write the failing tests**

In `crates/core/src/types/repo.rs`:

```rust
#[test]
fn repo_id_folds_ascii_case_only() {
    assert_eq!(RepoId::new("EPEL_10_Everything_x86_64"), RepoId::new("epel_10_everything_x86_64"));
    // Review Focus 1: Eq, Hash and Ord must agree, and must not fold non-ASCII.
    assert_ne!(RepoId::new("repo-Ü"), RepoId::new("repo-ü"));
    let mut set = std::collections::HashSet::new();
    set.insert(RepoId::new("EPEL"));
    assert!(set.contains(&RepoId::new("epel")));
    assert!(!set.contains(&RepoId::new("repo-Ü")) || set.len() == 1);
    assert_eq!(RepoId::new("EPEL").to_string(), "EPEL"); // Display keeps the host's spelling
}

#[test]
fn parse_repo_sections_reads_dnf_enabled_vocabulary() {
    // Review Focus 2.
    let content = "\
stray = line outside any section
[a]
baseurl=https://example.invalid/a
[b]
enabled=0
metalink=https://example.invalid/b
[c]
enabled = True   # trailing comment
mirrorlist=https://example.invalid/c
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-C
[d]
enabled=yes
";
    let got = parse_repo_sections(content);
    let by = |n: &str| got.iter().find(|s| s.id == RepoId::new(n)).unwrap();
    assert_eq!(got.len(), 4);
    assert!(by("a").enabled && by("a").has_source);      // absent enabled= is true
    assert!(!by("b").enabled && by("b").has_source);
    assert!(by("c").enabled);
    assert_eq!(by("c").gpg_key_paths, vec!["/etc/pki/rpm-gpg/RPM-GPG-KEY-C"]);
    assert!(by("d").enabled);
}
```

In `crates/collect/src/inspectors/rpm/source_repos.rs`:

```rust
#[test]
fn normalization_is_order_independent_and_never_relabels() {
    // § 8.4: the host's four repo files in both orders yield the same answer.
    let files = host_repo_files();              // redhat.repo, epel-cisco-openh264.repo, epel-testing.repo, epel.repo
    let mut reversed = files.clone(); reversed.reverse();
    for order in [files, reversed] {
        let mut pkgs = vec![pkg("htop", "aarch64", "epel")];
        normalize_source_repos(&mut pkgs, &order);
        assert_eq!(pkgs[0].source_repo, "epel");
        assert!(matches!(&pkgs[0].repo_attribution, RepoAttribution::Exact { id } if id == &RepoId::new("epel")));
    }
}

#[test]
fn attribution_table() {
    // § 8.4, the remaining rows.
    // exact match on a disabled section -> Exact on that section
    // short name whose only substring match is disabled -> Stale
    // short name substring-matching two enabled sections -> Ambiguous, source_repo unchanged
    // "anaconda" -> Pseudo(Anaconda); "" -> Unrecorded
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p inspectah-core types::repo && cargo test -p inspectah-collect source_repos`
Expected: FAIL — `RepoId` and `RepoAttribution` do not exist.

- [ ] **Step 3: Implement the types and `parse_repo_sections` in `crates/core/src/types/repo.rs`**

Hand-write `PartialEq`, `Eq`, `Hash`, and `Ord` for `RepoId` rather than deriving, so all four agree on `to_ascii_lowercase()`. `parse_repo_sections` reads `[section]` headers and `key=value` lines, trims a trailing `#` or `;` comment from a value, treats `1`, `true`, `yes`, `on` (case-insensitive) as enabled and everything else as disabled, treats an absent `enabled=` as enabled, sets `has_source` when any of `baseurl`, `metalink`, `mirrorlist` is present, and collects the path part of each `gpgkey=file://` value. Lines outside any section are dropped.

- [ ] **Step 4: Rewrite `normalize_source_repos` against the § 3.2 rule in `crates/collect/src/inspectors/rpm/source_repos.rs`**

Signature stays `pub fn normalize_source_repos(packages: &mut [PackageEntry], repo_files: &[RepoFile])`. Order of resolution: exact case-insensitive match against any section, enabled or not, wins first (`Exact`); otherwise substring match against `enabled = true` sections only, exactly one being `Resolved` and more than one `Ambiguous` with `source_repo` left as recorded; then `@commandline`/`anaconda`/`@System` as `Pseudo`, empty as `Unrecorded`, anything else as `Stale`. It must never iterate sections looking for a first hit. Delete `extract_section_ids` and route its callers through `parse_repo_sections`.

- [ ] **Step 5: Run the tests to verify they pass, then the workspace**

Run: `cargo test -p inspectah-core -p inspectah-collect && cargo test --workspace`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/core crates/collect
git commit -m "fix(collect): attribute packages to repos without relabeling them"
```

---

### Task 5: `RepoIndex` keyed by `RepoId`, `Disabled` and `Stale` provenance

**Implements § 3.2 (`RepoIndex` and `RepoProvenance`), open position 2. Satisfies § 8.5 in full.**

Closes the other half of item 1.5: the index keys `repo_file_by_section` by the header as written and `packages_by_repo` by `source_repo.to_lowercase()`, so a mixed-case section id can never be `Verified`.

**Files:**
- Modify: `crates/refine/src/repo_index.rs:58-66`, `:77-80`, `:85-93`, `:97-108` (delete its local `parse_repo_sections`, key everything by `RepoId`)
- Modify: `crates/refine/src/types.rs` (`RepoProvenance` gains `Disabled` and `Stale`; `Incomplete` is redefined)
- Modify: `crates/refine/src/session.rs:1401-1413` (`validate_target`'s `ItemId::Repo` arm)
- Modify: `crates/web/src/handlers.rs:209` (the `to_lowercase()` goes)
- Test: `crates/refine/tests/repo_index_test.rs` (new)

**Interfaces:**
- Consumes: Task 4's `RepoId`, `parse_repo_sections`, `RepoSectionDef`.
- Produces:
  - `pub enum RepoProvenance { Verified, Incomplete, Disabled, Stale, Unknown }` — `Verified`: section present, enabled, `has_source`, keys captured. `Incomplete`: section present and enabled, not fetchable as captured (no `baseurl`/`metalink`/`mirrorlist`, or a `gpgkey=file://` path not in `gpg_keys`). `Disabled`: section present, `enabled = 0`. `Stale`: referenced by packages, no section on the host. `Unknown`: pseudo repo or empty.
  - `RepoIndex::provenance(&RepoId) -> RepoProvenance`, `packages_by_repo: BTreeMap<RepoId, Vec<String>>`, `repo_file_by_section: BTreeMap<RepoId, ..>`, `gpg_keys_by_section: BTreeMap<RepoId, ..>`.
  - `validate_target` accepts `Verified` and `Incomplete` for `ItemId::Repo` and refuses `Disabled`, `Stale`, `Unknown` with `RefineError::BadRequest` naming the provenance, as today.

- [ ] **Step 1: Write the failing test** in `crates/refine/tests/repo_index_test.rs`

```rust
#[test]
fn case_split_collapses_to_one_verified_key() {
    // § 8.5: [EPEL_10_Everything_x86_64] in the file, "epel_10_everything_x86_64" on the package.
    let idx = index_with(/* repo file content */, /* packages */);
    assert_eq!(idx.provenance(&RepoId::new("EPEL_10_Everything_x86_64")), RepoProvenance::Verified);
    assert_eq!(idx.provenance(&RepoId::new("epel_10_everything_x86_64")), RepoProvenance::Verified);
    assert_eq!(idx.packages_by_repo().len(), 1);
}

#[test]
fn disabled_section_reports_disabled() { /* same fixture with enabled = 0 -> RepoProvenance::Disabled */ }

#[test]
fn validate_target_accepts_incomplete_and_refuses_the_rest() {
    // Incomplete -> Ok. Disabled, Stale, Unknown -> Err(BadRequest) whose message names the provenance.
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p inspectah-refine --test repo_index_test`
Expected: FAIL — two keys, and `Incomplete` is refused.

- [ ] **Step 3: Key the index on `RepoId` and widen the provenance enum**

Delete `repo_index.rs`'s own INI parser in favour of Task 4's `parse_repo_sections`, and the stale comment at `:77-80` with it. `ItemId::Repo { path }` comparisons go through `RepoId`, which also closes the hole where the server accepted an id the view never emitted.

- [ ] **Step 4: Run the test to verify it passes, then the workspace**

Run: `cargo test -p inspectah-refine --test repo_index_test && cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/refine crates/web
git commit -m "fix(refine): key the repo index case-insensitively and widen provenance"
```

---

### Task 6: `NoopTarget`, group effective members, and `Inert`

**Implements § 3.7 (groups), open positions 1 and 5. Satisfies § 8.8's group half: the three `crates/refine/tests/session_test.rs` group cases and the `GroupInfo` assertions in `crates/web/src/adapter.rs`, plus `crates/web/src/error.rs` mapping `NoopTarget` to 422. Satisfies the § 8.12 `api_test.rs` line "`SetInclude(Group, false)` on the Container Management fixture returns 422 with `error` starting `nothing to change`".**

Closes item 1.9: group exclude returns 200 and records nothing.

**Files:**
- Modify: `crates/refine/src/types.rs` (`NoopTarget`)
- Modify: `crates/refine/src/session.rs:1816-1836` (the `ItemId::Group` no-op arm), `:2479-2502`, `:2553-2557`, `:2652-2729` (`GroupEvalContext.effective_packages`), `:2670-2690` (the `'group_scan` loop)
- Modify: `crates/refine/src/group_state.rs:47-101` (`GroupRenderState::Inert`, `is_renderable`)
- Modify: `crates/web/src/web_types.rs:231-241` (`GroupInfo`), `crates/web/src/error.rs:29-36`
- Test: `crates/refine/tests/session_test.rs`, `crates/web/src/adapter.rs` `#[cfg(test)]`

**Interfaces:**
- Consumes: Task 5's `RepoProvenance` (no direct dependency, but the same release).
- Produces:
  - `RefineError::NoopTarget(String)` with `#[error("nothing to change: {0}")]`, mapped in `crates/web/src/error.rs` to 422 beside the two `*NotToggleable` arms.
  - `GroupRenderState::Inert`, reported as `render_state: "inert"`, treated by `is_renderable()` as not renderable.
  - `GroupInfo` gains `effective_member_count: usize` and `effective_included: usize`. `GroupMemberInfo.in_base_image` is computed from membership in `baseline_suppressed` or `base_image_only` rather than from absence.
  - The effective member set, used three times (count, no-op rule, render evaluation): `packages_added` members that are actionable and not locked, taken before any display filter.

- [ ] **Step 1: Write the failing tests** in `crates/refine/tests/session_test.rs`

```rust
#[test]
fn group_exclude_on_an_already_excluded_set_is_refused() {
    // § 8.8: Container Management, two non-leaf present members, one baseline-suppressed.
    let mut s = container_management_session();
    assert!(matches!(s.apply(set_include_group("Container Management", false)),
                     Err(RefineError::NoopTarget(_))));
    assert_eq!(s.timeline().len(), 0);
}

#[test]
fn group_include_then_exclude_moves_the_render_state_and_the_export() {
    let mut s = container_management_session();
    s.apply(set_include_group("Container Management", true)).unwrap();
    assert_eq!(s.timeline().len(), 1);
    let cf = containerfile_of(&s);
    assert!(cf.contains("buildah") && cf.contains("containernetworking-plugins"));
    s.apply(set_include_group("Container Management", false)).unwrap();
    assert_eq!(group_info(&s, "Container Management").render_state, "excluded");
    let cf = containerfile_of(&s);
    assert!(!cf.contains("dnf group install"));
    assert!(!active_install_list(&cf).contains(&"buildah".to_string()));
    s.undo().unwrap(); assert!(containerfile_of(&s).contains("dnf group install"));
    s.redo().unwrap(); assert!(!containerfile_of(&s).contains("dnf group install"));
}

#[test]
fn a_group_whose_present_members_are_all_locked_is_inert() {
    let mut s = all_locked_group_session();
    let gi = group_info(&s, "Locked Group");
    assert_eq!(gi.render_state, "inert");
    assert_eq!(gi.effective_member_count, 0);
    assert!(!containerfile_of(&s).contains("dnf group install"));
    for include in [true, false] {
        assert!(matches!(s.apply(set_include_group("Locked Group", include)),
                         Err(RefineError::NoopTarget(_))));
    }
}
```

In `crates/web/src/adapter.rs`:

```rust
#[test]
fn group_info_reports_declared_and_effective_counts() {
    let gi = group_info_for_host_fixture("Container Management");
    assert_eq!((gi.member_count, gi.effective_member_count, gi.effective_included), (3, 2, 0));
    assert!(gi.members.iter().find(|m| m.name == "podman").unwrap().in_base_image);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p inspectah-refine --test session_test group_ && cargo test -p inspectah-web adapter::`
Expected: FAIL — the exclude returns `Ok(())` and records nothing; `effective_member_count` does not exist.

- [ ] **Step 3: Add `NoopTarget` and its 422 mapping, and the `GroupInfo` counts**

The no-op check for `ItemId::Group` runs **before** `apply`'s single-item idempotency shortcut at `session.rs:789-791`. Message forms, exactly: `"all <M> effective members of '<name>' are already included"` / `"... already excluded"`, and for an empty set `"nothing to decide in '<name>': <reason>"` where `<reason>` is `all <n> members are locked` or `no package is attributed to it`. `is_op_noop`'s `.unwrap_or(true)` for a missing rpm section is the one case where "no members" must also refuse rather than succeed.

- [ ] **Step 4: Build `GroupEvalContext.effective_packages` from the projected `packages_added` before any display filter, and add `Inert`**

One definition used three times. The baseline rules stand: a member absent from `packages_added` is in the base and is not evaluated, and the leaf filter still governs the individual install list, not group evaluation. `dnf group install` is emitted only for `Renderable`, which now requires at least one effective member and every one of them included; the multilib, divergent-override, `Excluded`, and `MemberExcluded` priorities are unchanged.

**Deliberately not in phase 0:** § 3.7 also gives the `'group_scan` loop at `session.rs:2670-2690` an `ItemId::Section { section: Packages }` arm. `ItemId::Section` and `SectionId` are phase 2 work (§ 7, row 2), so there is no variant for that arm to match and it is not written here. The § 8.8 cases that exercise it are the ones the spec labels "Phase 2, same file", and they are out of this plan.

- [ ] **Step 5: Run the tests to verify they pass, then the workspace**

Run: `cargo test -p inspectah-refine -p inspectah-web && cargo test --workspace`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/refine crates/web
git commit -m "fix(refine): decide groups over their effective members"
```

---

### Task 7: Repo effective members, counts, and the member-based no-op rule

**Implements § 3.2 ("The effective member set of a repo", "Repo header counts and direction", "Repo no-op and refusal"), open positions 26 and 29. Satisfies § 8.8's repo half: the "Repo regression", "History and members disagreeing, both directions", and "Hidden member" blocks of `crates/refine/tests/session_test.rs`, `crates/web/src/adapter.rs`, and `crates/web/tests/api_test.rs`. Satisfies Review Focus item 3.**

This is the rule the two frontend tasks pair with. The atomic boundary of position 29 means Tasks 7, 8, and 9 ship together or not at all.

**Files:**
- Modify: `crates/refine/src/session.rs:1806-1814` (the `ItemId::Repo` no-op arm), `:1959-2032` (the cascade), `:2526-2535` (where `classified_state` is built)
- Modify: `crates/web/src/web_types.rs:130-137` (`RepoGroupInfo`), `crates/web/src/handlers.rs:197-252` (`build_repo_groups`)
- Test: `crates/refine/tests/session_test.rs`, `crates/web/src/adapter.rs` `#[cfg(test)]`, `crates/web/tests/api_test.rs`

**Interfaces:**
- Consumes: Task 5's `RepoId`-keyed `RepoIndex` and `RepoProvenance`; Task 6's `RefineError::NoopTarget`.
- Produces:
  - `pub struct RepoEffective { pub member_count: usize, pub included: usize, pub unlisted: usize }`, computed per `RepoId` by `recompute_view` in the same pass that builds `classified_state`, stored on the cached view.
  - `RefineSession::repo_effective(&RepoId) -> RepoEffective` — the one accessor both consumers go through.
  - `RepoGroupInfo` gains `effective_member_count: usize`, `effective_included: usize`, `effective_unlisted: usize`, filled in `build_repo_groups` from `repo_effective`. `package_count` stays the visible count over `view.packages` and is documented as such; it is never the denominator of a control. `enabled` stays `!repos_excluded().contains(section)`, the repo-exclusion history.

**The effective member set, exactly (§ 3.2):** the projected `packages_added` entries whose `source_repo` resolves to the section's `RepoId` (`Exact` or `Resolved` attribution), that are actionable, and that are not locked, taken from the classified package list `recompute_view` builds **before** its display filters (the list `classified_state` reads from at `session.rs:2526-2535`), not from `view.packages`. `unlisted` is the number of effective members absent from the filtered `view.packages`.

- [ ] **Step 1: Write the failing tests** in `crates/refine/tests/session_test.rs`

```rust
#[test]
fn repo_include_all_records_when_one_member_was_individually_excluded() {
    // § 8.8 "Repo regression".
    let mut s = verified_repo_session();          // two actionable unlocked members, no repo op yet
    s.apply(set_include_pkg("b.x86_64", false)).unwrap();
    let before = s.timeline().len();
    s.apply(set_include_repo("vendor", true)).unwrap();
    assert_eq!(s.timeline().len(), before + 1);
    assert!(install_list(&s).contains(&"b.x86_64".to_string()));
    assert!(!s.repos_excluded().contains(&RepoId::new("vendor")));
    s.undo().unwrap(); assert!(!install_list(&s).contains(&"b.x86_64".to_string()));
    s.redo().unwrap(); assert!(install_list(&s).contains(&"b.x86_64".to_string()));
    assert!(matches!(s.apply(set_include_repo("vendor", true)), Err(RefineError::NoopTarget(_))));
}

#[test]
fn history_and_members_disagree_in_both_directions() {
    // § 8.8, position 29. State (a): enabled history, both members individually excluded.
    let mut a = verified_repo_session();
    a.apply(set_include_pkg("a.x86_64", false)).unwrap();
    a.apply(set_include_pkg("b.x86_64", false)).unwrap();
    assert!(matches!(a.apply(set_include_repo("vendor", false)), Err(RefineError::NoopTarget(_))));
    let n = a.timeline().len();
    a.apply(set_include_repo("vendor", true)).unwrap();
    assert_eq!(a.timeline().len(), n + 1);
    assert_eq!(install_list(&a).len(), 2);
    a.undo().unwrap(); assert_eq!(install_list(&a).len(), 0);
    a.redo().unwrap(); assert_eq!(install_list(&a).len(), 2);

    // State (b): excluded history, both members individually included.
    let mut b = verified_repo_session();
    b.apply(set_include_repo("vendor", false)).unwrap();
    b.apply(set_include_pkg("a.x86_64", true)).unwrap();
    b.apply(set_include_pkg("b.x86_64", true)).unwrap();
    assert!(matches!(b.apply(set_include_repo("vendor", true)), Err(RefineError::NoopTarget(_))));
    b.apply(set_include_repo("vendor", false)).unwrap();
    assert_eq!(install_list(&b).len(), 0);
    assert!(!export_carries_repo_file(&b, "vendor.repo"));
}

#[test]
fn a_repo_with_no_effective_member_is_refused_in_both_directions() {
    // § 8.8 + position 26 + Review Focus 3.
    for mut s in [all_members_locked_repo_session(), repo_known_to_index_with_no_packages(),
                  session_with_no_rpm_section()] {
        for include in [true, false] {
            match s.apply(set_include_repo("vendor", include)) {
                Err(RefineError::NoopTarget(m)) => assert!(m.starts_with("nothing to decide")),
                other => panic!("expected NoopTarget, got {other:?}"),
            }
        }
        assert_eq!(s.timeline().len(), 0);
    }
}
```

In `crates/web/src/adapter.rs`:

```rust
#[test]
fn repo_group_info_counts_the_effective_set_not_the_visible_one() {
    // § 8.8 "Hidden member": one included visible leaf, one actionable unlocked
    // non-leaf dependency the display filter hides.
    let rg = repo_group_info_for("vendor");
    assert_eq!((rg.effective_member_count, rg.effective_included, rg.effective_unlisted, rg.package_count),
               (2, 1, 1, 1));
}

#[test]
fn build_repo_groups_reports_the_two_disagreement_states() {
    // § 8.8: the exact inputs the § 6.3 Switch reads, which § 8.12's frontend fixtures copy verbatim.
    assert_eq!(state_a(), (true,  2usize, 0usize, 0usize));  // enabled, M=2, N=0, K=0
    assert_eq!(state_b(), (false, 2usize, 2usize, 0usize));  // excluded history, M=2, N=2, K=0
}
```

In `crates/web/tests/api_test.rs`: post the accepted direction of each state through `POST /api/op`, assert 2xx with exactly one new `/api/ops` entry; post the refused direction, assert 422 with `error` starting `nothing to change`; post either direction at a zero-effective repo and assert 422 with `error` starting `nothing to decide`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p inspectah-refine --test session_test repo_ && cargo test -p inspectah-web`
Expected: FAIL — the history-based arm accepts the refused directions and refuses the real ones.

- [ ] **Step 3: Compute `RepoEffective` in `recompute_view` and expose `repo_effective`**

- [ ] **Step 4: Replace the `ItemId::Repo` no-op arm with the member-based rule**

`excluded_sections_at` is no longer read by this arm. Messages, exactly: `"all <M> effective members of '<id>' are already included"` / `"... already excluded"`, and `"nothing to decide in '<id>': <reason>"` with `<reason>` of `all <n> members are locked` or `no package is attributed to it`. The cascade is otherwise unchanged from `session.rs:1959-2032`: `include: false` excludes every effective member, the section's `.repo` file when every section in that file is excluded, and the GPG keys only that file references; `include: true` reverses it. Both directions skip locked members, which are also outside the counts. The server acts on the payload's `include` and never infers it from `enabled`.

- [ ] **Step 5: Fill the three counts in `build_repo_groups` from `repo_effective`**

`build_repo_groups` is also what `aggregate_handlers.rs:468` calls to fill `AggregateViewResponse.repo_groups`, so the aggregate view carries the counts with no aggregate-specific field or builder. Keep it that way.

- [ ] **Step 6: Run the tests to verify they pass, then the workspace**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/refine crates/web
git commit -m "fix(refine): decide repos by their members, not by exclusion history"
```

---

## Frontend Lane

Tasks 8 and 9 are the only frontend work in phase 0. They are small, and they are the release-compatibility contract of § 7: *no control is offered for an op the server refuses, and no offered control posts one.* They belong to the web frontend lane and ship in the same release as Task 5's new provenance values and Task 7's member-based no-op rule (position 29). Nothing else in the frontend moves until phase 1.

### Task 8: FRONTEND — repo provenance predicate and state text

**Implements § 3.2 ("Provenance consumers ship with the enum and the no-op rule") and § 6.3's no-control state table. Satisfies § 8.12's phase 0 release-boundary block: the `__tests__/RepoGroup.test.tsx` and `__tests__/RepoBar.test.tsx` `disabled`, `stale`, and zero-effective fixtures.**

Without this, phase 0's server hands a Verified-looking switch to a section it now refuses.

**Files:**
- Modify: `crates/web/ui/src/api/types.ts:376` (`RepoProvenance`), `:396` (`RepoGroupInfo`)
- Modify: `crates/web/ui/src/components/RepoGroupHeader.tsx:26-27` (`showToggle`)
- Modify: `crates/web/ui/src/components/RepoBar.tsx:32-34` (`toggleableRepos`)
- Test: `crates/web/ui/src/components/__tests__/RepoGroup.test.tsx`, `crates/web/ui/src/components/__tests__/RepoBar.test.tsx`

**Interfaces:**
- Consumes: Task 5's `disabled` and `stale` provenance values; Task 7's three counts on `RepoGroupInfo`.
- Produces: `RepoProvenance = "verified" | "incomplete" | "disabled" | "stale" | "unknown"`; `RepoGroupInfo` gains `effective_member_count: number`, `effective_included: number`, `effective_unlisted: number`. The shared predicate, in both components: `!isDistro && (provenance === "verified" || provenance === "incomplete") && effective_member_count > 0`.

**Exact state text (§ 6.3 table), rendered in the badge cell with `aria-describedby` pointing at it, left-edge cell empty, never a disabled checkbox:**

| Condition | Badge cell text |
|---|---|
| `is_distro` | `always included` |
| `provenance: disabled` | `disabled on host` |
| `provenance: stale` | `repo id not found in any .repo file` |
| `provenance: unknown` (`@commandline`) | title `Local / Manual installs`; `installed outside any repo` |
| verified or incomplete, `effective_member_count === 0`, every listed member locked | `nothing to decide: all members are locked` |
| verified or incomplete, `effective_member_count === 0`, base members listed too | `nothing to decide: all members are locked or from base` |
| `provenance: incomplete`, control shown | count text plus `repo definition incomplete` |

- [ ] **Step 1: Write the failing tests**

In `__tests__/RepoBar.test.tsx` and `__tests__/RepoGroup.test.tsx`, one block each:

```tsx
it.each(["disabled", "stale"] as const)("renders no switch and the state text for %s", (provenance) => {
  render(<RepoBar repos={[repo({ provenance, effective_member_count: 2, effective_included: 1 })]} onToggle={vi.fn()} />);
  expect(screen.queryByRole("switch")).toBeNull();
  expect(screen.getByText(provenance === "disabled"
    ? "disabled on host"
    : "repo id not found in any .repo file")).toBeInTheDocument();
});

it("renders no switch for a verified repo with no effective member", () => {
  render(<RepoBar repos={[repo({ provenance: "verified", effective_member_count: 0, effective_included: 0 })]} onToggle={vi.fn()} />);
  expect(screen.queryByRole("switch")).toBeNull();
  expect(screen.getByText("nothing to decide: all members are locked")).toBeInTheDocument();
});

it("keeps the switch for verified and incomplete with members", () => {
  render(<RepoBar repos={[repo({ provenance: "incomplete", effective_member_count: 2, effective_included: 2 })]} onToggle={vi.fn()} />);
  expect(screen.getByRole("switch")).toBeInTheDocument();
});
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cd crates/web/ui && npm test -- RepoBar RepoGroup`
Expected: FAIL — `disabled` and `stale` are not assignable to `RepoProvenance`, and the current predicate renders a Switch for all four.

- [ ] **Step 3: Widen `RepoProvenance` and `RepoGroupInfo` in `api/types.ts`**

- [ ] **Step 4: Apply the predicate and the state text in both components**

`RepoGroupHeader.tsx`'s `showToggle` and `RepoBar.tsx`'s `toggleableRepos` filter take the same expression. Render the table's text in the badge cell for every other value.

- [ ] **Step 5: Run the tests to verify they pass, then the whole frontend suite**

Run: `cd crates/web/ui && npm test && npx tsc --noEmit`
Expected: PASS. `DecisionSections.test.tsx`, `ResponsiveLayout.test.tsx`, and `FocusAndNavigation.test.tsx` carry `RepoBar`/`RepoGroupHeader` fixtures; update their fixtures with the three new fields rather than deleting the cases.

- [ ] **Step 6: Commit**

```bash
git add crates/web/ui/src
git commit -m "fix(web): offer no repo control the server will refuse"
```

---

### Task 9: FRONTEND — `RepoBar`'s Switch state and posted direction, both mounts

**Implements § 3.2 ("The atomic boundary", position 29) and § 6.3's "Phase 0 `RepoBar` rule, both mounts". Satisfies § 8.12's phase 0 activation block: `__tests__/RepoBar.test.tsx`, `__tests__/MainContent.repo.test.tsx` (new), `aggregate/__tests__/AggregateApp.repo.test.tsx` (new), and the `e2e/repos.spec.ts` / `e2e/aggregate.spec.ts` sequence extensions.**

Task 8's predicate is not enough on its own: the control that survives it still reads `enabled`, the repo-exclusion history, and both callers post `!repo.enabled`. Under Task 7's member-based rule that posts the refused direction whenever history and members disagree, and both disagreements are reachable through ordinary per-package decisions.

**Files:**
- Modify: `crates/web/ui/src/components/RepoBar.tsx:5-7` (`onToggle` signature), `:99-101` (the visible count leaves the row), `:103-109` (the Switch)
- Modify: `crates/web/ui/src/components/MainContent.tsx:281-300` (`handleRepoToggle`), `:436-439` (the mount)
- Modify: `crates/web/ui/src/components/AggregateApp.tsx:350-364` (`handleAggregateRepoToggle`), `:588-603` (the mount)
- Modify: `crates/web/ui/src/components/PackageList.tsx:42` (the `onRepoToggle` prop type takes the second argument; `PackageList` still ignores it at `:96`)
- Test: `crates/web/ui/src/components/__tests__/RepoBar.test.tsx`; `crates/web/ui/src/components/__tests__/MainContent.repo.test.tsx` (new); `crates/web/ui/src/components/aggregate/__tests__/AggregateApp.repo.test.tsx` (new); `crates/web/ui/e2e/repos.spec.ts`, `e2e/aggregate.spec.ts`, `e2e/fixtures/sequences/repo-disagree/` (new)

**Interfaces:**
- Consumes: Task 8's widened `RepoGroupInfo`; Task 7's `build_repo_groups` numbers, which the fixtures copy verbatim.
- Produces:
  - `RepoBarProps.onToggle: (sectionId: string, include: boolean) => void`
  - `MainContent.handleRepoToggle(sectionId: string, include: boolean)` and `AggregateApp.handleAggregateRepoToggle(sectionId: string, include: boolean)`, both posting the `include` they are given. `PackageList`'s `onRepoToggle` prop takes the same signature, which phase 1 keeps.

**The rule, exactly (§ 6.3):** `isChecked` is `effective_included === effective_member_count` and never `enabled`; `enabled` is read by nothing in the bar. Activation posts `include = !isChecked`. The label beside the Switch is the count text, `<N> of <M> packages included`, or `<M> packages excluded` when N is 0, each followed by `, <K> not listed` when `effective_unlisted` is non-zero. The visible `package_count` leaves the row.

**The two fixture states, verbatim from Task 7's `build_repo_groups` assertions:**
- state (a): `enabled: true, effective_member_count: 2, effective_included: 0, effective_unlisted: 0`
- state (b): `enabled: false, effective_member_count: 2, effective_included: 2, effective_unlisted: 0`

- [ ] **Step 1: Write the failing `RepoBar` unit tests**

```tsx
it("reads the counts, not the history, and posts the derived direction", () => {
  const onToggle = vi.fn();
  const { rerender } = render(<RepoBar repos={[repo(stateA)]} onToggle={onToggle} />);
  expect(screen.getByRole("switch")).not.toBeChecked();
  expect(screen.getByText("2 packages excluded")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("switch"));
  expect(onToggle).toHaveBeenCalledWith("vendor", true);

  rerender(<RepoBar repos={[repo(stateB)]} onToggle={onToggle} />);
  expect(screen.getByRole("switch")).toBeChecked();
  expect(screen.getByText("2 of 2 packages included")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("switch"));
  expect(onToggle).toHaveBeenLastCalledWith("vendor", false);
});

it("renders the not-listed clause and never the visible count", () => {
  render(<RepoBar repos={[repo({ ...stateA, effective_included: 1, effective_unlisted: 1, package_count: 1 })]}
                  onToggle={vi.fn()} />);
  expect(screen.getByText("1 of 2 packages included, 1 not listed")).toBeInTheDocument();
  expect(screen.getByRole("switch")).not.toBeChecked();
});

it.each([stateA, stateB])("ignores a contradicting enabled bit", (state) => {
  const onToggle = vi.fn();
  const { container } = render(<RepoBar repos={[repo(state)]} onToggle={onToggle} />);
  const before = container.innerHTML;
  render(<RepoBar repos={[repo({ ...state, enabled: !state.enabled })]} onToggle={onToggle} />);
  // same rendered markup and, on activation, the same posted direction
});
```

Rewrite the existing `enabled`-driven cases into these rather than keeping both.

- [ ] **Step 2: Write the failing caller tests**

`__tests__/MainContent.repo.test.tsx` (new) mounts `MainContent` with the live `RepoBar` over a view holding one `verified` repo and two actionable member rows. Mock `api/client` the way `MainContent.ungroup.test.tsx:8-11` does (`vi.mock("../../api/client", () => ({ applyOp: vi.fn(), ungroupGroup: vi.fn() }))`) but give `applyOp` a **stateful fake**, not `vi.fn()`. The fake holds the view and applies § 3.2's Repo arm in one line: reject with a `nothing to change` error carrying status 422 when every effective member already holds `target.include`; otherwise append to an ops log, move every effective member and `effective_included`, and set the repo's `enabled` to the posted `include`. An accepted `SetInclude(Package)` moves its member and the counts and leaves `enabled` alone. Undo restores the prior `enabled` with the members and counts; redo reapplies. It resolves the refetched view, which the test feeds back through `onViewUpdate`.

```tsx
it("state (a): reaches enabled history with both members excluded, then includes all", async () => {
  // Reach the state through the two mounted member rows, not fixture surgery.
  await excludeMemberRow("a.x86_64");
  await excludeMemberRow("b.x86_64");
  expect(fake.repo("vendor")).toMatchObject({ enabled: true, effective_member_count: 2, effective_included: 0 });
  expect(screen.getByRole("switch")).not.toBeChecked();
  expect(screen.getByText("2 packages excluded")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("switch"));
  await waitFor(() => expect(fake.lastPayload()).toEqual({
    op: "SetInclude",
    target: { item_id: { kind: "Repo", key: { path: "vendor" } }, include: true },
  }));
  expect(onMutationError).not.toHaveBeenCalled();
  expect(fake.opsLog()).toHaveLength(3);                       // two package ops, one repo op
  await waitFor(() => expect(screen.getByRole("switch")).toBeChecked());
  expect(screen.getByText("2 of 2 packages included")).toBeInTheDocument();
});

it("state (b): repo exclude then two individual includes, then excludes all", async () => {
  // ... asserts checked, activation posting include: false, one new log entry,
  // and the rerendered Switch unchecked beside "2 packages excluded" with both rows excluded.
});
```

A case asserting only that `onToggle` was called does not satisfy this entry.

`aggregate/__tests__/AggregateApp.repo.test.tsx` (new) mounts `AggregateApp` with the live `RepoBar` at `AggregateApp.tsx:588-603` over an `AggregateViewResponse` with one `verified` repo and two member rows. Mock `../../../api/client` with the same stateful fake plus `undo` and `redo` (the file at `aggregate/__tests__/AggregateApp.test.tsx:22-26` is the mocking pattern). Reach state (a) through two individual member toggles on the mounted aggregate rows; assert unchecked beside `2 packages excluded`, activation posting `include: true` through `handleAggregateRepoToggle`, acceptance with one log entry, the refetched Switch checked beside `2 of 2 packages included`, Ctrl+Z restoring unchecked with the log popped (the `undo/redo flow` block at `AggregateApp.integration.test.tsx:371` is the pattern), and Ctrl+Shift+Z re-applying. Then state (b) in the mirror direction.

- [ ] **Step 3: Write the failing browser sequences**

Single-host undo and redo are wired in `App.tsx` (`handleUndo` at `:453`, `handleRedo` at `:468`), not in `MainContent`, so they are covered in the browser. Add `e2e/fixtures/sequences/repo-disagree/` with `00-enabled-all-excluded.json`, `01-after-include-all.json`, `02-after-undo.json`, `03-after-redo.json`, and the mirror set from excluded history. Replay them through `mockSequence` (`e2e/helpers/mock-api.ts`) on `/api/op`, `/api/undo`, `/api/redo` in `e2e/repos.spec.ts`, asserting at each step the `repo-toggle-<id>` Switch's checked state and count text against the fixture's counts, and on the captured `/api/op` request that `target.include` equals the direction the counts derive. Replay the same sequences on the `aggregate-3` fixture in `e2e/aggregate.spec.ts`.

- [ ] **Step 4: Run the new tests to verify they fail**

Run: `cd crates/web/ui && npm test -- RepoBar MainContent.repo AggregateApp.repo`
Expected: FAIL — `onToggle` takes one argument, the Switch reads `enabled`, and both callers post `!repo.enabled`.

- [ ] **Step 5: Change the `RepoBar` Switch and the two callers**

`isChecked={effective_included === effective_member_count}`; `onChange={() => onToggle(repo.section_id, !isChecked)}`; the `aria-label` and label text become the count text; `package_count` leaves the row. Replace `include: !repo.enabled` at `MainContent.tsx:291` and `AggregateApp.tsx:360` with the `include` the handler is given. Widen `PackageList.tsx:42`'s prop type with them.

- [ ] **Step 6: Run the tests to verify they pass, then the whole frontend suite**

Run: `cd crates/web/ui && npm test && npx tsc --noEmit && npm run test:e2e -- repos aggregate`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/web/ui
git commit -m "fix(web): read the repo switch from its members in both mounts"
```

---

## Rust Change Set, continued

### Task 10: `FcontextRule` types and the ordered collector

**Implements § 3.1 (types and Collector), open positions 11, 12, 19. Satisfies § 8.9's collector half: the table-driven `crates/collect/tests/selinux_test.rs` parser cases and the source-selection cases.**

Closes the collection half of item 1.4. Order is the first thing the collector has to preserve: `semanage fcontext -l -C` prints the local table sorted by expression, while the policy resolves overlapping local expressions by recency.

**Files:**
- Modify: `crates/core/src/types/selinux.rs` (the four types, `SelinuxSection.fcontext_rules`, `fcontext_source`)
- Modify: `crates/collect/src/inspectors/selinux.rs:318-345` (`collect_fcontext_rules`)
- Modify: `crates/collect/src/inspectors/config/walk.rs:128-130` (exclusion globs), `crates/collect/src/inspectors/config/mod.rs:150` (the `rpm_va` path)
- Modify: `crates/core/src/aggregate/merge.rs:1608-1609` (§ 4 consumer row: fcontext dedup)
- Test: `crates/collect/tests/selinux_test.rs`

**Interfaces:**
- Consumes: Task 3's schema 23.
- Produces, in `crates/core/src/types/selinux.rs`:
  - `pub enum FcontextFileType { All, RegularFile, Directory, CharDevice, BlockDevice, Socket, Symlink, NamedPipe }` with `pub fn semanage_flag(self) -> Option<char>` (`f d c b s l p`, `None` for `All`) and `pub fn parse_column(text: &str) -> Option<Self>`.
  - `pub struct SelinuxContext { pub user: String, pub role: String, pub type_: String, pub level: Option<String> }`
  - `pub enum FcontextRule { Label { spec, file_type, context, #[serde(default)] disposition }, Unlabel { spec, file_type, #[serde(default)] disposition }, Equivalence { subject, labeled_like, #[serde(default)] disposition }, Unparsed { raw } }` — `#[serde(tag = "kind", rename_all = "snake_case")]`. The three decision-bearing variants use a **bare** `#[serde(default)]`: a collected rule is a customization the user made and included-by-default is the intended contract, the same as `SelinuxPortLabel`. `Unparsed` has no disposition.
  - `pub enum FcontextSource { PolicyStore, SemanageList }`, `#[serde(default)]` to `SemanageList`, on `SelinuxSection.fcontext_source`.
  - `SelinuxSection.fcontext_rules: Vec<FcontextRule>` in collected order.

**Collector order of preference, exactly (§ 3.1):** read `/etc/selinux/<policy>/contexts/files/file_contexts.local` and `file_contexts.subs` first, in file order, and set `PolicyStore`; `file_contexts.subs_dist` is the distro's and is not collected. Fall back to `chroot <root> semanage fcontext -l -C` only when neither store file is readable, and then set `SemanageList`.

**Formats, exactly:**
- `file_contexts.local` line: `regex [--|-d|-c|-b|-s|-l|-p] user:role:type[:level]`, whitespace-separated, `<<none>>` as the context for unlabel, the flag mapping onto `FcontextFileType` (`--` is `RegularFile`, no flag is `All`).
- `file_contexts.subs` line: `subject labeled_like`.
- `-l -C` output, two parts: lines before `SELinux Local fcontext Equivalence` are table rows split on a run of **two or more** spaces (column gaps are never a single space; path regexes can contain one), three columns giving `Label`, or `Unlabel` when the third is `<<None>>` case-insensitively, the context split on `:` into user, role, type, and an optional level that may itself contain `:`; lines after the header match `^(\S+) = (\S+)$`. Anything else non-blank and not a header becomes `Unparsed`.

- [ ] **Step 1: Write the failing table-driven test** in `crates/collect/tests/selinux_test.rs`

```rust
#[test]
fn fcontext_parser_table() {
    let cases: &[(&str, FcontextRule)] = &[
        // the host's padded Label row
        ("/srv/www(/.*)?                                     all files          system_u:object_r:httpd_sys_content_t:s0",
         label("/srv/www(/.*)?", FcontextFileType::All, "system_u", "object_r", "httpd_sys_content_t", Some("s0"))),
        // <<None>> -> Unlabel; "regular file" -> RegularFile
        // a user_u:object_r:foo_t:s0-s0:c0 context with the level kept intact
        // "/home1 = /home" -> Equivalence { subject: "/home1", labeled_like: "/home" }
        // a stray line -> Unparsed
    ];
    for (line, want) in cases { assert_eq!(parse_semanage_line(line), Some(want.clone()), "{line}"); }
}

#[test]
fn policy_store_forms_and_order() {
    // "/srv/www(/.*)?  -d  system_u:object_r:httpd_sys_content_t:s0" -> Label { file_type: Directory, .. }
    // "--" -> RegularFile; a file_contexts.subs line "/home1 /home" -> Equivalence
    // With file_contexts.local readable the collector runs no `semanage` and sets PolicyStore,
    // and two rules written in the order "/srv/www(/.*)?", "/srv/.*" come out in that order.
    // With neither store file readable it runs `semanage fcontext -l -C` and sets SemanageList.
}
```

Use `MockExecutor` per `process-docs/skills/mock-executor-key-format.md`.

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p inspectah-collect --test selinux_test`
Expected: FAIL — `fcontext_rules` is `Vec<String>`.

- [ ] **Step 3: Add the four types and `FcontextSource` to `crates/core/src/types/selinux.rs`**

- [ ] **Step 4: Rewrite `collect_fcontext_rules` against the order of preference and the three formats**

- [ ] **Step 5: Take `file_contexts.local` and `file_contexts.subs` out of the config walk**

Add `/etc/selinux/*/contexts/files/file_contexts.local` and `file_contexts.subs` to the walker's exclusion globs at `walk.rs:128-130`, which already exclude the `contexts/files/*` family for unowned files, and give the `rpm_va` path at `config/mod.rs:150` the same skip — the file is `rpm_owned_modified` on this host. It is derived policy-store state, not config.

- [ ] **Step 6: Update the aggregate fcontext merge (§ 4 consumer row)**

`merge.rs:1608-1609` stops calling `dedup_strings` and dedups by semantic key: `Label`/`Unlabel` on `(spec, file_type)`, `Equivalence` on `subject`, `Unparsed` on `raw`. The surviving rule's `disposition` is included when included on every host carrying the key and excluded otherwise, the `merge_items` convention. Hosts that disagree on the context for one key keep the first in hostname order, and the aggregate audit report names the disagreement. Order: hostname order, then each host's collected order. `fcontext_source` is `PolicyStore` only when every host's is.

- [ ] **Step 7: Run the tests, then the workspace**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/core crates/collect
git commit -m "feat(collect): collect file context rules as typed, ordered records"
```

---

### Task 11: The SELinux block, `selinux_equivalences`, and the executed-shell test

**Implements § 3.1 rules 1 to 4 and § 5.1, open positions 18, 23, 24, 30. Satisfies § 8.9's render half: the `crates/pipeline/tests/smoke_render.rs` `insta` assertions and the whole of `crates/pipeline/tests/selinux_block_shell_test.rs` (new). Also satisfies the § 4 consumer rows for the HTML report (`report.rs:910-917`) and the audit report's fcontext section (`audit.rs:593-625`).**

Closes the render half of item 1.4. The renderer has no host access, so every decision that depends on target state is made by the emitted commands at build time.

**Files:**
- Create: `crates/pipeline/src/render/selinux.rs`
- Modify: `crates/pipeline/src/render/containerfile.rs:1445-1462` (the SELinux block), `:357-368` (the GPG key path FIXME, rewritten to the § 5.6 wording rule because this task touches it)
- Modify: `crates/pipeline/src/render/report.rs:910-917`, `crates/pipeline/src/render/audit.rs:593-625`, `crates/pipeline/src/render/readme.rs:228`
- Test: `crates/pipeline/tests/smoke_render.rs`, `crates/pipeline/tests/selinux_block_shell_test.rs` (new)

**Interfaces:**
- Consumes: Task 10's `FcontextRule`, `FcontextSource`, `SelinuxContext`.
- Produces:
  - `pub fn selinux_equivalences(snap: &InspectionSnapshot) -> Vec<(FcontextRule, EquivalenceOrigin)>` in `crates/pipeline/src/render/selinux.rs`, applying § 3.1 rule 2's dedup and conflict detection.
  - `pub enum EquivalenceOrigin { Collected, Synthesized { reason: String }, Conflict { rows: Vec<FcontextRule> } }`
  - `const VAR_SYMLINKED_TOPLEVELS: &[(&str, &str)] = &[("/srv", "/var/srv")];`
  - The Containerfile, the audit report, and the HTML report all read the helper, so "synthesized by inspectah" and a conflict are reported the same way everywhere.

**The two emitted forms, verbatim (§ 5.1).** Rule 1, with `<spec>` the single-quoted validated spec, `[-f <flag>]` present only when `file_type != All`, and `<args>` identical on the `-a` and the `-m`:

```
RUN spec='<spec>'; \
    eqs="$(semanage fcontext -l -C)" || { printf 'FIXME: could not list the target file contexts before replaying the rule for %s; fix semanage in the base, or exclude this rule, then rebuild\n' "$spec" >&2; exit 1; }; \
    if printf '%s\n' "$eqs" | awk '$2 == "=" { print $1 }' | grep -qxF -- "$spec"; then \
      printf 'FIXME: skipped the file context rule for %s; the target maps that path as an equivalence, which semanage fcontext -d would delete instead of a label; delete it with semanage fcontext -d -e, or exclude this rule, then rebuild\n' "$spec" >&2; \
    else semanage fcontext -d [-f <flag>] "$spec" 2>/dev/null || true; \
      semanage fcontext -a <args> "$spec" \
      || semanage fcontext -m <args> "$spec"; fi
```

Rule 2, with `'<subject>'` and `'<labeled_like>'` in the places `/var/srv` and `/srv` take:

```
RUN eq="$(semanage fcontext -l -C | awk -v s='/var/srv' '$2 == "=" && $1 == s { print $3 }')"; \
    if [ -z "$eq" ]; then semanage fcontext -a -e '/srv' '/var/srv'; \
    elif [ "$eq" != '/srv' ]; then printf 'FIXME: %s is already equivalent to %s on the target, not %s; delete that equivalence with semanage fcontext -d -e, or exclude this rule, then rebuild\n' '/var/srv' "$eq" '/srv' >&2; exit 1; fi
```

The unrenderable form, two lines, instruction first:

```
# FIXME: add this file context by hand with: RUN semanage fcontext -a -t <type> '<spec>' || semanage fcontext -m -t <type> '<spec>' (labels apply at deploy; no restorecon needed)
#   the collected rule could not be rendered: <raw line or the offending operand, quoted>
```

**Flag rules:** `-f <flag>` only when `file_type != All`; `-s <user>` when `user != "system_u"`; `-r <level>` when `level` is `Some` and not `s0`; `Unlabel` renders `-t '<<none>>'` inside the same guard. The `-d` carries only `-f` and the spec. `-m -e` is never emitted. When `fcontext_source` is `SemanageList`, the block starts with `# NOTE: rule order comes from a sorted listing, not insertion order; if two expressions overlap, reorder so the intended winner is last.` No `restorecon` line at all; the only `restorecon` text in the block is the `#` sentence about data restored into `/var`, which `readme.rs:228` repeats.

**Where the synthesized equivalence goes:** the renderer walks included `Label`/`Unlabel` rules and, before the first whose `spec` begins with a var-symlinked top level, emits the equivalence once. A `spec` begins with a top level when it equals it, or continues with `/` or `(`.

- [ ] **Step 1: Write the failing `insta` render assertions** in `crates/pipeline/tests/smoke_render.rs`

The § 5.1 after-text for the host rule, exactly, plus: two rules render two guarded forms in collected order; a `Directory` rule carries `-f d` on all three commands; an `Unlabel` carries `-t '<<none>>'` inside the same guard; no `RUN` line contains `echo`; a spec containing `'` renders the two-line FIXME with the imperative first, and so does a spec of `--help`, with no `RUN` for either; no line contains `restorecon` outside the `#` sentence; the equivalence `RUN` appears once for two `/srv` rules and not at all for a `/opt` rule; a collected `/var/srv = /srv` plus the synthesis emits one equivalence `RUN`; a collected `/var/srv = /opt/srv` plus the synthesis emits no equivalence `RUN` and a FIXME naming both; no line contains `-m -e`; `/tmp/$(id) = /tmp` and `/srv;rm -rf / = /srv` each appear only inside a `#` line and no `RUN` line contains `;rm`; a `SemanageList` snapshot renders the `# NOTE:` caveat first and a `PolicyStore` one does not.

**Rendered bodies, not a lexical ban (§ 8.9).** Compare every `RUN` of the block whole against its expected body, the rule 1 or rule 2 template above with its placeholders filled. The only command substitutions it may hold are the two fixed queries, each verbatim, and every operand appears only as a single-quoted validated literal or as `"$eq"` or `"$spec"`. A `Label` with spec `/x/$(id)` renders `spec='/x/$(id)'`, its `$(` inside the single quotes, and passes. One negative case substitutes the round 2 double-quoted diagnostic, `echo "FIXME: <subject> is already equivalent to $eq on the target, not <labeled_like>; ..." >&2`, for the `printf` line of the expected equivalence body and asserts the comparison fails.

- [ ] **Step 2: Write the failing executed-shell test** in `crates/pipeline/tests/selinux_block_shell_test.rs` (new)

Take each rendered `RUN` of the block, strip the instruction keyword and the `\` continuations, and run it with `/bin/sh -c` under a `PATH` whose first entry holds a stub `semanage` script that prints a configured `-l -C` listing for `fcontext -l -C`, appends its argv to a log one argument per line, and exits 0 or, when the test asks, 1 for `-a`. `awk`, `grep`, and `printf` are the host's. Every case asserts the exit status, the stub's whole argv log (the listing call is logged like any other), the exact stdout and stderr, and the absence of the execution signal.

**Execution signal.** Whether an operand ran is measured by a side effect, not by its text: the stub directory also holds a `c6_exec` script that creates a `C6_EXECUTED` file in the case's temporary directory, and every case asserts that file absent. `printf` is not the signal, because it is a `/bin/sh` builtin a `PATH` stub does not intercept and the diagnostic itself calls it.

Equivalence form (rule 2) for `/var/srv = /srv`, three branches: *absent* (empty listing) gives empty stdout and stderr, a log of the listing call then exactly one five-entry `fcontext -a -e /srv /var/srv`, exit 0; *identical* gives empty output, no `-a`/`-m`/`-d`, exit 0; *conflict* (`/var/srv = /opt/srv` listed) gives empty stdout, stderr exactly the one § 5.1 FIXME line with the three operands filled in and nothing else, no mutating call, exit 1. Repeat all three with the allowed shell-looking operands, subject `` /literal/`printf${IFS}C6_EXECUTED` `` and `labeled_like` `` /l{a,b}$HOME`c6_exec` ``, `HOME` set to a fixed value, asserting each operand byte for byte as one argv entry on the *absent* branch and, on *conflict*, stderr exactly:

```
FIXME: /literal/`printf${IFS}C6_EXECUTED` is already equivalent to /opt/srv on the target, not /l{a,b}$HOME`c6_exec`; delete that equivalence with semanage fcontext -d -e, or exclude this rule, then rebuild
```

with the backticks, `${IFS}`, braces, and `$HOME` unexpanded. `C6_EXECUTED` is expected there as data inside the quoted subject. The render test's negative case runs here too: with the round 2 `echo` substituted, the conflict branch writes `/literal/C6_EXECUTED` and the fixed `HOME` value into stderr and creates the file, and the case fails on both.

Label guard (rule 1), three branches: *no same-subject equivalence* with spec `/srv/www(/.*)?` gives empty output and a log of the listing, then `fcontext -d <spec>`, then `fcontext -a -t httpd_sys_content_t <spec>`, in that order with the spec as one argv entry, exit 0; repeat with `/etc/foo\.conf` (backslash intact), `` /x/`id` ``, and `/x/$(id)`, and with a stub that fails `-a`, where the log continues with `fcontext -m -t httpd_sys_content_t <spec>` and the exit is 0; a `-f d` rule logs `-f d` on all three. *Same-subject equivalence* (`/var/srv = /srv` listed, spec exactly `/var/srv`), for `Label`, `Unlabel`, and `-f d`: empty stdout, stderr exactly the rule 1 skip FIXME naming `/var/srv` and nothing else, no `-d`/`-a`/`-m`, exit 0; with the same listing and spec `/var/srv/www(/.*)?` the guard does not fire and the triple runs. *Listing failure* (a stub whose `fcontext -l -C` exits 1), for `/x/$(id)` and `/etc/foo\.conf`: empty stdout, stderr exactly the rule 1 listing-failure FIXME naming the spec byte for byte, no `-d`/`-a`/`-m`, exit 1; the rule 2 form in the same condition runs `-a -e` once and exits nonzero.

- [ ] **Step 3: Run both to verify they fail**

Run: `cargo test -p inspectah-pipeline selinux`
Expected: FAIL — the renderer still emits the `sanitize_shell_value` FIXME and the `restorecon` line.

- [ ] **Step 4: Implement `selinux_equivalences` in `crates/pipeline/src/render/selinux.rs`**

Key the list on `subject` over the collected `Equivalence` rows plus the synthesized `/var/srv = /srv`. A collected row equal to the synthesized one is emitted once as the collected row, and the audit report notes the synthesis was redundant. Two rows with one subject and different `labeled_like`, or a collected row mapping the other direction (`/srv = /var/srv`), are a `Conflict`: neither is emitted, and the two-line FIXME lists both and asks the operator to keep one.

- [ ] **Step 5: Rewrite the Containerfile SELinux block at `containerfile.rs:1445-1462`**

Operand validation per the Global Constraints; a rule failing any check renders the two-line FIXME, never a command. Diagnostics are static `printf` formats with operands as arguments. `sanitize_shell_value` is not used on fcontext operands.

- [ ] **Step 6: Point the HTML report, audit report, and README at the same data (§ 4 consumer rows)**

`report.rs:910-917` renders each rule as the command the Containerfile emits, or its FIXME text, with its disposition; equivalences come from `selinux_equivalences`, synthesized and conflict rows marked. `audit.rs:593-625` lists commands, dispositions, `fcontext_source`, and the helper's equivalence origins; its "File context rules: N" count line (`audit.rs:617-621`) counts decision-bearing rules only. `readme.rs:228` carries the restored-data `restorecon` sentence.

- [ ] **Step 7: Run both tests to verify they pass, then the workspace**

Run: `cargo insta test -p inspectah-pipeline --review` then `cargo test --workspace`
Expected: PASS, with the SELinux snapshot deltas reviewed and accepted.

- [ ] **Step 8: Commit**

```bash
git add crates/pipeline
git commit -m "fix(pipeline): render file context rules as commands valid against the target"
```

---

### Task 12: `rpm_filename.rs` — one filename parser and one allowlist

**Implements § 3.3 ("The staged filename rule"), open positions 21 and 27. Satisfies § 8.6's `crates/refine/src/rpm_filename.rs` table and § 8.2's "`crates/refine/src/rpm_filename.rs` carries the same table against `validate_staged_name` directly". Satisfies Review Focus item 5.**

Four filename matchers disagree today (item 1.10); the server becomes the only matcher, and the one place a name is proved safe is this module.

**Files:**
- Create: `crates/refine/src/rpm_filename.rs` (moved from `crates/refine/src/session.rs:3554-3600`)
- Modify: `crates/refine/src/lib.rs` (module declaration)
- Test: `crates/refine/src/rpm_filename.rs` `#[cfg(test)]`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub struct RpmFilename { pub name: String, pub arch: String, pub version_release: Option<String> }`, `#[derive(Debug, Clone, PartialEq, Eq, Serialize)]`, with `pub fn parse(filename: &str) -> Option<Self>` — `.rpm` suffix matched case-insensitively, arch from `KNOWN_ARCHES` after `.` or `_`, name is everything before the first `-<digit>`. Parsing is matching, not a safety check.
  - `pub const MAX_UPLOAD_FILENAME_LEN: usize = 200;`
  - `pub enum NameError { Empty, TooLong, BadFirstByte, BadByte { at: usize }, NotRpm }`
  - `pub fn validate_staged_name(name: &str) -> Result<(), NameError>` — the allowlist in the Global Constraints. Rejection is always rejection, never stripping.

- [ ] **Step 1: Write the failing table-driven tests**

```rust
#[test]
fn rpm_filename_parse_table() {
    // § 8.6, the executed matcher table.
    let cases: &[(&str, Option<(&str, &str, Option<&str>)>)] = &[
        ("epel-release-latest-9.noarch.rpm",      Some(("epel-release-latest", "noarch", Some("9")))),
        ("epel-release-9-11.el9.noarch.rpm",      Some(("epel-release", "noarch", Some("9-11.el9")))),
        ("epel-release-9-10.el9.noarch.rpm",      Some(("epel-release", "noarch", Some("9-10.el9")))),
        ("epel-release-latest-10.noarch.rpm",     Some(("epel-release-latest", "noarch", Some("10")))),
        ("epel-release.noarch.rpm",               Some(("epel-release", "noarch", None))),
        ("epel-release-latest-9.noarch.RPM",      Some(("epel-release-latest", "noarch", Some("9")))),
        ("epel-release-latest-9.noarch (1).rpm",  None),
        ("epel-release-latest-9.noarch-1.rpm",    None),
        ("epel-release-latest-9.rpm",             None),
    ];
    for (input, want) in cases { /* assert RpmFilename::parse(input) matches */ }
}

#[test]
fn validate_staged_name_table() {
    // § 8.2, the same table the handler test posts.
    let accepted = ["epel-release-9-12.el9.noarch.rpm", "epel-release-latest-9.noarch.rpm",
                    "google-chrome-stable_current_x86_64.rpm", "libstdc++-devel-11-1.el9.x86_64.rpm",
                    "foo-1.0~rc1-1.fc40.x86_64.rpm", "foo-1.2^20240101git1-1.fc40.x86_64.rpm",
                    "X-1-1.noarch.RPM",
                    "a.rpm",                                   // Review Focus 5: the 5-byte minimum
                    &format!("{}.rpm", "a".repeat(196))];      // Review Focus 5: exactly 200 bytes
    for n in accepted { assert_eq!(validate_staged_name(n), Ok(()), "{n}"); }

    let rejected = ["epel-release-9$(id)-1.noarch.rpm", "epel-release-9-1\nRUN id.noarch.rpm",
                    "epel-release-9-1\r.noarch.rpm", "epel-release 9-1.noarch.rpm",
                    "epel-release-9-1.noarch (1).rpm", "../x.rpm", "..\\x.rpm", "/etc/x.rpm",
                    ".epel-release-9-1.noarch.rpm", "-rf.rpm", "foo.txt", "", ".rpm",
                    &format!("{}.rpm", "a".repeat(197))];      // 201 bytes
    for n in rejected { assert!(validate_staged_name(n).is_err(), "{n}"); }
    // "../x.rpm" is rejected, never stripped to "x.rpm".
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p inspectah-refine rpm_filename`
Expected: FAIL — the module does not exist.

- [ ] **Step 3: Move `RpmFilename` out of `session.rs:3554-3600` and add `validate_staged_name`**

Check bytes with `u8::is_ascii_alphanumeric`, never `char::is_alphanumeric`, which admits Unicode. The alphabet is rpm's: Name is alnum plus `-._+`, Version and Release alnum plus `._+~^` (`^` for snapshot versions such as `1.2^20240101git1`), arch `[a-z0-9_]`, and vendor names use `_`. No allowed byte is significant to the Dockerfile parser, to `COPY` globbing, to shell-form `RUN` and the `&& rm` operand, or to a comment.

- [ ] **Step 4: Run the tests to verify they pass, then the workspace**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/refine
git commit -m "refactor(refine): move rpm filename parsing into one module with an allowlist"
```

---

### Task 13: The upload handler — one field, chunked staging, one lock hold

**Implements § 3.3 ("Upload staging", the four-step lifecycle) and § 6.9's `/api/upload-rpm` bullet. Satisfies § 8.2's `crates/web/tests/api_test.rs` filename-rule block, the 413, disconnect, and startup-sweep cases, the overlap cases through `stage`/`commit`, and § 8.6's and § 8.13's `api_test.rs` 200 cases.**

**Files:**
- Modify: `crates/web/src/upload.rs:17-78` (the handler, split into `stage` and `commit`), `:24-28`, `:29-37`, `:39`, `:45-54`, `:62`, `:73-77`
- Modify: `crates/web/src/lib.rs:110` (the route limit becomes `UPLOAD_LIMIT_BYTES`)
- Modify: `crates/refine/src/session.rs:585-604` (`ensure_upload_dir`), `:626-690` (`mark_uploaded_rpm` becomes `match_uploaded_rpm`), `:691-697` (`invalidate_caches` is called by `set_uploaded`)
- Test: `crates/web/tests/api_test.rs`

**Interfaces:**
- Consumes: Task 12's `validate_staged_name`, `RpmFilename`, `MAX_UPLOAD_FILENAME_LEN`.
- Produces:
  - `pub const UPLOAD_LIMIT_BYTES: usize = 524_288_000;` (500 MiB) in `crates/web/src/lib.rs`
  - `RefineSession::match_uploaded_rpm(&self, filename: &str) -> Option<UploadMatch>` — today's matcher with every mutation removed (exact NEVRA first, then `RpmFilename` name and arch with progressively shorter name prefixes).
  - `pub struct UploadMatch { pub canonical: String, pub match_kind: UploadMatchKind, pub host_version: String, pub uploaded_version: Option<String> }`; `pub enum UploadMatchKind { Exact, VersionDiffers }` serializing as `exact` | `version_differs`. `host_version` is `format!("{version}-{release}")` of the entry; `uploaded_version` is `Some(parsed version-release)` when it differs.
  - `RefineSession::set_uploaded(&mut self, canonical: &str, staged_path: PathBuf, uploaded_version: Option<String>)` — records the entry's current `(repoless_cached, cache_path, uploaded_version)` in `prior_cache: HashMap<String, PriorCache>` when the entry was `repoless_cached` from the host's dnf cache and not yet `uploaded`, then sets `uploaded = true`, `repoless_cached = true`, `cache_path = Some(staged_path)`, `uploaded_version`, and calls `invalidate_caches()`.
  - Handler split: `stage(field) -> StagedPart` and `commit(&mut session, StagedPart, &filename) -> UploadOutcome`, so the § 8.2 tests can interleave two uploads deterministically.
  - Response body: `{ "status": "matched" | "unmatched", "matched": "<name.arch>" | null, "match_kind": "exact" | "version_differs" | null, "host_version": "<v-r>" | null, "uploaded_version": "<v-r>" | null, "uploaded": <count>, "parsed": RpmFilename | null, "staged_as": "<filename>" }`.

**Request contract, exactly (§ 3.3):** exactly one field, named `file`, carrying a filename; a request with no such field, a second field, or a field without a filename is 400 before anything is staged. The filename passes `validate_staged_name` or the request is 400 with the rule named. `upload.rs:29-32`'s `unknown.rpm` default and `Path::file_name`/`to_string_lossy` stripping go; stripping turns `../x.rpm` into `x.rpm` and admits `$(id)`.

**Staging, exactly:** stream with `field.chunk()` into `tempfile::Builder::new().prefix(".upload-").suffix(".part").rand_bytes(16).tempfile_in(upload_dir)` (`tempfile` is already a dependency of `crates/web`, `Cargo.toml:19`) — `O_EXCL`, mode 0600, unique per request, deleted on `Drop` including when axum drops the handler future on a client disconnect. Resident memory per upload is one multipart chunk. Map every `MultipartError` through its own `status()` instead of the blanket 400 at `:24-28` and `:45-49`: in axum 0.8.9 a `DefaultBodyLimit` overrun arrives as `StreamReadFailed` wrapping `LengthLimitError`, which `status()` maps to 413. `sync_all()` before the session mutex is taken, so the critical section holds no I/O wait.

**Startup sweep:** when a process first opens the upload directory, before the server listens, remove regular files (by `symlink_metadata`) matching `.upload-*.part`. A running server never removes a part file by name pattern; each request removes only its own, through `Drop`.

**The upload directory is owned and private:** `ensure_upload_dir` creates `<stem>-uploads` beside the tarball with mode 0700 and, when it already exists, requires `symlink_metadata` to show a directory (not a symlink) owned by the effective uid with no group or other write bit; otherwise the session refuses to stage with `RefineError::TarballError` naming the path and the upload returns 500. The canonical path is computed once here and kept on the session.

**The critical section, in order,** inside one hold of `state.session.lock()` with no `.await` inside it, the step-2 match running under the same hold so it cannot go stale: the 409 check, then `NamedTempFile::persist(upload_dir.join(filename))` (an atomic `rename(2)`), then `set_uploaded`, then deletion of a superseded file, then release. The 409 check applies to unmatched placements too: if another package already has `cache_path == upload_dir.join(filename)`, the upload is refused with 409 `<filename> is already staged for <other name.arch>` and the part file dropped. No rename ever replaces a file that is another package's `cache_path`. If the matched package already has `uploaded == true` with a different `cache_path`, the previous file is deleted after the record step. Two requests for one filename or one package each stage their own part file; the one whose critical section runs last wins, both receive 200, and the generation bump tells the client which state holds. On no match, the part file is persisted to `upload_dir.join(filename)` so it is staged for the modal's "remove it or try another", nothing on any package changes, and the response is `status: "unmatched"`.

- [ ] **Step 1: Write the failing handler tests** in `crates/web/tests/api_test.rs`

```rust
#[tokio::test]
async fn upload_filename_rule_at_the_handler() {
    // § 8.2: each 400, with the upload directory and the view generation unchanged.
    let rejected = ["epel-release-9$(id)-1.noarch.rpm",
                    // a newline sent as filename*=UTF-8''epel-release-9-1%0ARUN%20id.noarch.rpm, and a \r variant
                    "epel-release 9-1.noarch.rpm", "epel-release-9-1.noarch (1).rpm",
                    "../x.rpm", "..\\x.rpm", "/etc/x.rpm", ".epel-release-9-1.noarch.rpm"];
    // plus: a 201-byte name; a name with invalid UTF-8;
    // a request with two fields; a field not named "file"; a "file" field with no filename.
    // Accepted (200): the seven names of § 8.2.
}

#[tokio::test]
async fn upload_limits_and_lifecycle() {
    // a body one byte over UPLOAD_LIMIT_BYTES returns 413 (the route layer's limit,
    // which multipart framing counts against) and leaves no part file;
    // a client that disconnects mid-body leaves no part file;
    // a process started over a directory holding `.upload-dead.part` removes it before serving.
}

#[tokio::test]
async fn upload_response_carries_the_server_verdict() {
    // § 8.6 + § 8.13: POST with `epel-release-latest-9.noarch.RPM` returns 200,
    // status "matched", match_kind "version_differs", host_version "9-11.el9",
    // uploaded_version "9", parsed.name "epel-release-latest",
    // staged_as equal to the uploaded filename, and the staged file exists under that name.
}
```

Peak resident memory is not asserted: there is no portable measurement. The chunk size is a named constant and the bounded-memory claim rests on the `field.chunk()` loop being the only read, which review of the implementation confirms.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p inspectah-web --test api_test upload`
Expected: FAIL — the handler buffers with `field.bytes()`, defaults a missing filename, and answers 400 for an oversize body.

- [ ] **Step 3: Split the handler into `stage` and `commit` and implement the contract above**

- [ ] **Step 4: Turn `mark_uploaded_rpm` into `match_uploaded_rpm` plus `set_uploaded`, and harden `ensure_upload_dir`**

- [ ] **Step 5: Run the tests to verify they pass, then the workspace**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/web crates/refine
git commit -m "fix(web): stage uploads in bounded chunks under one lock hold"
```

---

### Task 14: Export ownership and verification of staged uploads

**Implements § 3.3 ("Export", "One writer per output path"), open positions 28 and 31. Satisfies § 8.2's "render safety", "export confinement", and "output ownership against the source tarball" blocks, and the dnf-cache path case.**

Closes item 1.3's export half: a relaxed-matched upload exports nothing today because `copy_uploaded_rpms` allowlists by snapshot-NEVRA filename equality and never reads `cache_path`.

**Files:**
- Modify: `crates/refine/src/session.rs:3185-3195` (the step 2e allowlist), `:3219-3245` (the extraction), `:3259-3300` (`copy_uploaded_rpms`, replaced), `:3350-3362` (step 2f)
- Modify: `crates/pipeline/src/render/repoless.rs:28-48` (`repoless_rpm_lines`)
- Modify: `crates/web/src/handlers.rs:334` (`POST /api/tarball` gains `warnings`)
- Test: `crates/refine/tests/upload_export_test.rs` (new)

**Interfaces:**
- Consumes: Task 12's `validate_staged_name`, `RpmFilename`; Task 13's `set_uploaded` and the session's canonical `upload_dir`.
- Produces:
  - `fn verify_staged_upload(upload_dir_canon: &Path, pkg: &PackageEntry) -> Result<VerifiedUpload, Refusal>`
  - `pub struct VerifiedUpload { pub canonical: String, pub basename: String, pub uploaded_version: Option<String> }`
  - `VerifiedUploads`, a map from `name.arch` to `VerifiedUpload`, handed to the renderer through `render_ctx`. The Containerfile preview runs the same verification without the copy and passes the same map, so preview and export cannot disagree.
  - `POST /api/tarball` response gains `warnings: [..]`.

**The verification order, exactly (§ 3.3):** (1) `pkg.uploaded` is true, the package is included, `cache_path` is `Some`; (2) `basename = Path::new(cache_path).file_name()` is valid UTF-8 and passes `validate_staged_name`; (3) `canonicalize(parent(cache_path)) == upload_dir_canon` — the parent only, since canonicalizing the full path would follow a final symlink; (4) `p = upload_dir_canon.join(basename)` and `symlink_metadata(p).file_type().is_file()`, false for a symlink; (5) the file is opened, the handle is `fstat`ed, and the handle must be a regular file with the same `(dev, ino)` as step 4 (`std::os::unix::fs::MetadataExt`, no new dependency), closing the check-to-copy race on the final component. The copy then reads from that handle with `io::copy`.

**One writer per output path (position 31):** step 2e's allowlist becomes the included `repoless_cached` packages with `uploaded == false`; nothing under `repoless-packages/` is extracted for an uploaded package, whatever the entry type. A formatted allowlist name is allowlisted only when it passes `validate_staged_name` (which admits no `/`), and the remainder of every `repoless-packages/` entry after the prefix must be one `Normal` path component (`Path::components()` yields exactly one), else the entry is refused with `TarballError` naming it, allowlisted or not — `tar::Entry::path()` does not clean `..` (only `unpack_in` does; `tar` 0.4.45 `entry.rs:415`). For packages that stay on the allowlist, a `Symlink` or `Link` entry at an allowlisted name is rejected with `RefineError::TarballError` naming the entry; the `unmanaged/` branch keeps its link handling, since unmanaged files can be links. Step 2f requires `symlink_metadata(dest_dir)` to report a directory, then creates the destination with `OpenOptions::new().write(true).create_new(true)`; an `AlreadyExists` error is an ownership violation, not a demotion, and fails the export with `TarballError` naming the path.

**Demotion (position 28):** on verification failure, no member, no active `COPY`, the comment `# FIXME: upload the RPM for <name.arch> again in refine; the staged file could not be verified for export` (the name sanitized, `cache_path` never echoed), and `warnings: ["<name.arch>: staged RPM not verified, not exported"]` in the export response. A missing file is the same demotion.

**Render:** `repoless_rpm_lines` names only what the map holds — `COPY repoless-packages/<basename> /tmp/`, `RUN dnf localinstall -y /tmp/<basename>`, `&& rm /tmp/<basename>`, and when `uploaded_version` is `Some` the comment `# uploaded <name>-<uploaded_version>.<arch>; the host had <host_version>` with the three snapshot-derived operands through `sanitize_for_comment()`. `uploaded_version` is re-derived from the verified basename through `RpmFilename::parse`, never read from the snapshot field. The pre-existing dnf-cache path at `repoless.rs:29-30` and `:46-48` builds its `COPY` source from snapshot NEVRA fields bare; from phase 0 a NEVRA filename that fails `validate_staged_name` renders the non-active FIXME instead of a `COPY`.

- [ ] **Step 1: Write the failing tests** in `crates/refine/tests/upload_export_test.rs` (new)

One session on an `epel-release` entry (`9-11.el9`) with `set_upload_dir`, each case through `match_uploaded_rpm`, the place step, `set_uploaded`, `SetInclude`, and `render_refine_export(.., Some(&dir))`. The § 8.2 cases, each as its own `#[test]`:

- **exact** — `epel-release-9-11.el9.noarch.rpm`: `match_kind == Exact`, `uploaded_version == None`, the active `COPY repoless-packages/epel-release-9-11.el9.noarch.rpm /tmp/`, that member in the tarball, `uploaded == true` in the view.
- **differing version** — `epel-release-9-12.el9.noarch.rpm`: `match_kind == VersionDiffers`, `host_version == "9-11.el9"`, `uploaded_version == Some("9-12.el9")`, the active COPY and the `# uploaded ...; the host had 9-11.el9` comment, that member in the tarball, and **no** line naming `9-11.el9.noarch.rpm`.
- **Fedora filename** — `epel-release-latest-9.noarch.rpm`: matched, `uploaded_version == Some("9")`, the active COPY and the tarball member both naming `epel-release-latest-9.noarch.rpm`.
- **placement failure after staging** — with the part file fully staged, make `upload_dir.join(filename)` a non-empty directory so the rename fails (a read-only directory would fail the staging step instead and test the wrong thing): the commit returns the error, `uploaded` stays false, `cache_path` and the previous upload are unchanged, the part file is gone, and the export emits the MANUAL block instead of an active COPY.
- **replace** — `9-11` then `9-12` for one package: only the second file is in the directory, `cache_path` names it, the export copies only it. The same filename twice: one file, one COPY.
- **same filename for two packages** — the second upload returns 409 and the first package's state is untouched.
- **overlap, both orders** — through `stage` and `commit`: stage A (`AAAA`) and B (`BBBB`) for one filename, commit A then B, and separately B then A; after each, the directory holds one file whose bytes are the last committer's, `cache_path` names it, the exported member's bytes equal it, and no `.part` remains. Stage `9-12` and `9-13` for one package and commit in both orders: only the last file remains and the export copies only it. Stage A and B, drop A's `StagedPart`: A's part is gone, B's still exists, and committing B succeeds.
- **render safety** — a snapshot with `uploaded: true`, an included package, a real file at `upload_dir/epel-release-9$(id)-1.noarch.rpm`, and `uploaded_version: "9$(id)-1"`: `active_lines()` contain no `$(` and no `COPY repoless-packages/`, the § 3.3 FIXME is present, and the tarball has no `repoless-packages/` member. A clean filename with `uploaded_version: "9\nRUN id"` in the snapshot: the active lines equal the expected set exactly and the comment carries the version re-derived from the filename.
- **export confinement** — each yielding no tarball member with those bytes (compared by content), no active `COPY repoless-packages/`, the FIXME, and the warning in the export response, with the export succeeding in every case: `cache_path` outside `upload_dir` with `uploaded: true`; an in-directory symlink with a valid name pointing outside; `cache_path` in a subdirectory of `upload_dir`; `uploaded: true` with a `/var/cache/dnf/...` path; a missing file.
- **output ownership against the source tarball** — each asserting on the exported tarball's members, entry type, and bytes, not only on the Containerfile: a source tarball carrying `repoless-packages/epel-release-9-11.el9.noarch.rpm` as a `Symlink` entry whose target is a writable sentinel outside the export directory, with a verified exact-NEVRA upload staged (the sentinel's bytes unchanged, the member a regular-file entry whose bytes equal the staged file, the active `COPY` naming it); the same with a `Link`; that name as a regular cached RPM with different bytes plus an unverifiable staged upload, first an in-directory symlink then a missing file (demotion with the FIXME and the response warning, no active `COPY`, no member from either source); a regular cached RPM in the tarball for an uploaded package with a verified upload of different bytes (the member's bytes are the upload's); a `repoless_cached` package with `uploaded == false` whose source entry at its NEVRA is a `Symlink` or `Link` pointing at the sentinel (`Err(TarballError(_))` naming the entry, no tarball written, sentinel unchanged); a destination planted in the export directory before step 2f as a regular file and as a symlink to the sentinel (`create_new` fails, `Err(TarballError(_))` naming the path, sentinel unchanged); a snapshot whose `repoless_cached` entry has `name: "../repoless-packages/epel-release"` with the matching regular-file entry `repoless-packages/../repoless-packages/epel-release-9-11.el9.noarch.rpm`, with and without a verified upload of that basename (`Err(TarballError(_))` naming the entry, no `repoless-packages/` member, and a listing of the directory above the export directory unchanged); a `name` of `../../sentinel` with its matching entry (the same error, sentinel directory unchanged).
- **dnf-cache path** — a `repoless_cached` entry whose NEVRA fields form `evil-1$(id)-1.x86_64.rpm` renders the FIXME and no `COPY`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p inspectah-refine --test upload_export_test`
Expected: FAIL — the NEVRA allowlist exports nothing for a relaxed match, and `fs::copy` follows a planted symlink.

- [ ] **Step 3: Implement `verify_staged_upload` and replace `copy_uploaded_rpms`**

- [ ] **Step 4: Give step 2e the ownership rule and the one-component check, and step 2f `create_new`**

- [ ] **Step 5: Rewrite `repoless_rpm_lines` to read only the `VerifiedUploads` map, and add `warnings` to `POST /api/tarball`**

- [ ] **Step 6: Run the tests to verify they pass, then the workspace**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/refine crates/pipeline crates/web
git commit -m "fix(refine): give an uploaded package's export path one proven writer"
```

---

### Task 15: Confined removal and the upload-directory rescan

**Implements § 3.3 ("Removal", "Rescan"), open position 14. Satisfies § 8.2's "removal", "removal confinement", and "rescan" blocks.**

Today any `repoless_cached` package qualifies for removal and its `cache_path` is deleted, which for a cache RPM is a file under the host's `/var/cache/dnf`.

**Files:**
- Modify: `crates/refine/src/session.rs:585` (`ensure_upload_dir` gains the rescan), `:699-737` (`unmark_uploaded_rpm`)
- Modify: `crates/web/src/upload.rs:80-107` (`DELETE /api/upload-rpm/{name.arch}`)
- Test: `crates/refine/tests/upload_export_test.rs`

**Interfaces:**
- Consumes: Tasks 12, 13, 14.
- Produces: `unmark_uploaded_rpm` with the precondition `uploaded == true`; `ensure_upload_dir` returning the rescan's warnings.

**Removal, exactly:** the precondition becomes `uploaded == true`; a cached-only package returns 409 `only uploads can be removed; <name.arch> is cached from the host's dnf cache` and nothing changes. Deletion is confined by the same proof export uses: the basename of `cache_path` passes `validate_staged_name`, its canonicalized parent equals the session's canonical `upload_dir`, and `symlink_metadata` of `upload_dir_canon.join(basename)` is a regular file; the unlink is of that joined path after the check, never of the raw `cache_path`; otherwise 500 naming the basename and no deletion. After deleting, `uploaded` becomes false and `(repoless_cached, cache_path, uploaded_version)` are restored from `prior_cache` when the upload had replaced a dnf-cache RPM, else cleared.

**Rescan, exactly:** `ensure_upload_dir` lists the directory, keeps regular files (by `symlink_metadata`) whose name passes `validate_staged_name`, skips everything else with a warning naming the skipped entry (part files, symlinks, and names outside the rule included), and runs steps 2 and 4 of the lifecycle on each in filename order — through the upload handler's matcher (`RpmFilename`, then name and arch with prefix shortening), **never** NEVRA filename equality, because staged files keep their real names. An unmatched file stays staged and unmarked; two files matching one package leave the later in filename order marked and the earlier staged, with a warning. Unmatched files are never copied.

- [ ] **Step 1: Write the failing tests** in `crates/refine/tests/upload_export_test.rs`

```rust
#[test]
fn removal_requires_an_upload_and_restores_the_prior_cache() {
    // § 8.2 removal: on an uploaded package, the staged file is deleted,
    // uploaded == false, repoless_cached cleared;
    // on a repoless_cached package with uploaded == false, the 409 variant,
    // the cache_path file still exists, repoless_cached still true;
    // on a package whose upload replaced a dnf-cache RPM, repoless_cached == true
    // and the original cache_path are restored.
}

#[test]
fn removal_is_confined_to_the_staging_directory() {
    // a cache_path outside upload_dir (set directly on the fixture),
    // an in-directory symlink, and a basename failing the rule are each
    // refused and nothing is deleted.
}

#[test]
fn rescan_remarks_through_the_matcher_and_warns_on_the_rest() {
    // a fresh session over the same directory re-marks
    // `epel-release-latest-9.noarch.rpm` with the same uploaded_version;
    // `.upload-1234.part`, a symlink named `x.rpm`, and `bad name.rpm` are each
    // skipped with a warning; an unmatched `epel-next-release-latest-9.noarch.rpm`
    // stays and marks nothing.
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p inspectah-refine --test upload_export_test removal rescan`
Expected: FAIL — removal accepts a cached-only package and the rescan does not exist.

- [ ] **Step 3: Add the rescan to `ensure_upload_dir` and the precondition and confinement to `unmark_uploaded_rpm`**

- [ ] **Step 4: Run the tests to verify they pass, then the workspace**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/refine crates/web
git commit -m "fix(refine): confine upload removal and re-match staged files on resume"
```

---

### Task 16: Repo-enabling classification

**Implements § 3.3 ("Classification"), open positions 3, 10, 16. Satisfies § 8.3's `crates/collect/tests/repoless_rpm_test.rs` half.**

Closes item 1.3's classification half: `epel-release` renders twice today, once as an active install and once as a MANUAL block.

**Files:**
- Modify: `crates/core/src/types/rpm.rs` (`PackageEntry` gains `repo_enabling_from` and `uploaded`; `repo_providing_packages` at `:232` is deleted)
- Modify: `crates/collect/src/inspectors/rpm/repoless.rs:58-79` (`get_available_packages`), `:99-192` (`scan_dnf_cache_for_repoless`), `:129-143` (the anaconda check, unchanged in behaviour)
- Modify: `crates/core/src/aggregate/merge.rs:949` (the `repo_providing_packages` merge is deleted)
- Test: `crates/collect/tests/repoless_rpm_test.rs`

**Interfaces:**
- Consumes: Task 4's `RepoId`.
- Produces:
  - `PackageEntry.repo_enabling_from: Vec<RepoId>`, `#[serde(default, skip_serializing_if = "Vec::is_empty")]`. Non-empty means repo-enabling, never repo-less.
  - `PackageEntry.uploaded: bool`, `#[serde(default, skip_serializing_if = "crate::is_false")]`. Distinct from `repoless_cached`, which also covers RPMs found in the dnf cache.
  - `get_available_packages` returns `Option<HashMap<String, Vec<RepoId>>>` (name to serving section ids) instead of `Option<HashSet<String>>`, with the query format widened from `%{name}\n` to `%{name} %{reponame}\n`.

**The two tests a candidate must both pass (§ 3.3), run in `scan_dnf_cache_for_repoless` before the cache scan:**
1. *Provides repository configuration:* `source_repo == "@commandline"`, a name ending in `-release`, and an `rpm.file_ownership` entry whose paths under `/etc/yum.repos.d/` and `/etc/pki/rpm-gpg/` are a subset of `repo_files[].path ∪ gpg_keys[].path`, with at least one repo file among them.
2. *Installable by name from a repository the export can carry:* the package name is a key of the map `get_available_packages` returns.

A package passing both gets `repo_enabling_from = <the serving ids>`, no `repoless_annotation`, and leaves the repo-less set. A package passing only the first keeps `repoless_annotation` (`No repo source: owns <n> captured repo file(s) but is not available from them`), keeps the upload control, and still has its repo files and keys copied. A line that does not split into exactly two tokens is dropped; a map with no entry for the name is the same as absence; `None` (dnf failed, or the host's dnf does not print the format) keeps the package repo-less — the recovery path is suppressed only on proof.

- [ ] **Step 1: Write the failing tests** in `crates/collect/tests/repoless_rpm_test.rs`

With a `MockExecutor` answering `dnf repoquery --available` with `%{name} %{reponame}` lines:

```rust
#[test]
fn repo_enabling_classification_table() {
    // § 8.3, each case its own assertion on repo_enabling_from and repoless_annotation:
    // epel-release @commandline, file_ownership paths all captured, output "epel-release epel"
    //   -> repo_enabling_from == [RepoId::new("epel")], repoless_annotation empty
    // the same with epel.repo missing from repo_files -> stays repo-less
    // vendor-release owning a fully captured vendor.repo and key, absent from the output
    //   -> repo-less with the "owns <n> captured repo file(s) but is not available" annotation
    // the same with vendor.repo's only section enabled=0 -> the same result
    // the same with the output listing "vendor-release vendor" -> repo-enabling from "vendor"
    // two repositories: vendor-release owns vendor.repo (workloads) while the output lists
    //   "vendor-release bootstrap" from a captured bootstrap.repo
    //   -> repo_enabling_from == [RepoId::new("bootstrap")], not ["vendor"]
    // repoquery failing (None) -> every candidate stays repo-less
    // output whose lines carry one token -> every candidate stays repo-less
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p inspectah-collect --test repoless_rpm_test`
Expected: FAIL — `repo_enabling_from` does not exist.

- [ ] **Step 3: Widen `get_available_packages` to `%{name} %{reponame}` and `HashMap<String, Vec<RepoId>>`**

The anaconda check at `repoless.rs:129-143` keeps using membership and is otherwise unchanged.

- [ ] **Step 4: Add the two-test classification to `scan_dnf_cache_for_repoless`, and delete `repo_providing_packages`**

`rpm.repo_providing_packages` (`crates/core/src/types/rpm.rs:232`) is dead — its only writer is the aggregate merge of empty lists at `merge.rs:949`. Delete the field and the merge.

- [ ] **Step 5: Run the tests to verify they pass, then the workspace**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/core crates/collect
git commit -m "feat(collect): classify repo-enabling packages by proven availability"
```

---

### Task 17: The repo-enabling Containerfile block, CRB, and export-time demotion

**Implements § 3.3 ("Render", "CRB") and § 5.2, open position 17. Satisfies § 8.3's `crates/pipeline/src/render/containerfile.rs` unit tests, the two-repository export case, and the CRB cases. Satisfies the § 4 audit-report row for repo-enabling packages.**

**Files:**
- Modify: `crates/pipeline/src/render/containerfile.rs:635-640` (the install partition), the Repo-enabling block, every `RUN dnf ... install -y` site, `:357-368` is already covered by Task 11
- Modify: `crates/pipeline/src/render/service_intent.rs:70` (`is_package_installable` no longer admits a `@commandline` `-release` package by `source_repo` alone)
- Modify: `crates/pipeline/src/render/audit.rs` (the Packages section lists each repo-enabling package with its serving repo ids and whether the export still carries one)
- Test: `crates/pipeline/src/render/containerfile.rs` `#[cfg(test)]`

**Interfaces:**
- Consumes: Task 16's `repo_enabling_from`; Task 4's `parse_repo_sections`.
- Produces:
  - The install partition at `containerfile.rs:635-640` becomes `!pkg.repo_enabling_from.is_empty() || base.ends_with("-release")`.
  - `const RELEASE_PERMALINKS: &[(&str, &str)] = &[("epel-release", "https://dl.fedoraproject.org/pub/epel/epel-release-latest-{major}.noarch.rpm")];` — `{major}` is the host's OS major version from `os.version_id`.
  - `crb_ids: Vec<RepoId>`, collected by `parse_repo_sections` over the included `repo_files`.

**The export-time serving check (§ 3.3):** against the projected snapshot, at least one id in `repo_enabling_from` must name a section that is *carried* (its `.repo` file's entry in `rpm.repo_files` is included), *enabled* (`parse_repo_sections` over that file's content reports `enabled = true` for the id), and *keyed* (every `gpgkey=file://` path the section names is an included `gpg_keys` entry). The files and keys the package itself owns are checked too, as before. When no serving id survives, the package is demoted to the repo-less rendering, `repoless_annotation` is set on the projection (`No repo source: the repo that served <name> (<id>) is not carried by this export`), the upload control returns in the view, and the Containerfile prints, in place of any active install:

```
# FIXME: include <file> under Configuration Files, or re-include repo <id>, or upload the RPM; <name> installs from that repo definition, which this export does not carry
```

**Conservative narrowing, as the spec states it:** a demotion is the only outcome when proof is lost. The renderer never keeps an active `dnf install -y <name>` on the strength of a repo the export does not carry, and never substitutes another enabled repo that the host's query did not name as a provider.

**CRB (position 17):** `crb_ids` is every section with `enabled = true` whose id, lowercased, starts with `codeready-builder-` and contains neither `-debug-` nor `-source-`, kept as the host wrote it. Every `RUN dnf ... install -y` the Containerfile emits — repo-enabling packages, `dnf group install`, the package list, module streams — carries `--enablerepo=<id>` for each id in `crb_ids`, and the first such transaction carries `# CRB was enabled on the source host; EPEL packages may depend on it.` No id is reconstructed from a prefix and no `$(arch)` literal is written.

**Block text, verbatim (§ 5.2),** with the permalink on a `#` line only:

```
# === Repo-enabling packages (1) ===
# epel-release was installed outside any repo on the source host; the host's
# enabled repos served it, and its repo files and key are copied above, so it
# installs from the copied definition. The build needs those repo endpoints
# reachable with any credentials and CA trust they require. Installing after the
# COPY may leave a cosmetic epel.repo.rpmnew. Alternative, when
# dl.fedoraproject.org is reachable:
# RUN dnf install -y https://dl.fedoraproject.org/pub/epel/epel-release-latest-9.noarch.rpm
```

The "Repo-less RPM packages" section is not emitted when no package carries `repoless_annotation`.

- [ ] **Step 1: Write the failing render tests** in `crates/pipeline/src/render/containerfile.rs` `#[cfg(test)]`

```rust
#[test]
fn repo_enabling_block_replaces_the_duplicate_manual_block() {
    // § 8.3: active lines are COPY config/etc/yum.repos.d/, COPY config/etc/pki/rpm-gpg/,
    // then `RUN dnf install -y \` with epel-release; the permalink appears only on a `#` line;
    // no MANUAL line.
}

#[test]
fn losing_the_serving_repo_demotes_the_package() {
    // excluding epel.repo in the projected snapshot demotes epel-release to the § 3.3 FIXME
    // and leaves no active `dnf install -y epel-release`.
    // Two-repository case through export: repo_enabling_from == ["bootstrap"] with
    // bootstrap.repo excluded while vendor.repo and every file vendor-release owns stay
    // included -> no active `dnf install -y vendor-release`, the FIXME naming `bootstrap`,
    // the repo-less MANUAL block for vendor-release, and the upload control in the view;
    // re-including bootstrap.repo restores the active install.
    // The same with [bootstrap] carried but enabled=0 in the included file, or with its
    // gpgkey=file:// path excluded under GPG Keys, is the demotion.
}

#[test]
fn crb_flags_every_dnf_transaction() {
    // [codeready-builder-for-rhel-9-aarch64-rpms] enabled=1 with an EPEL workload:
    // every RUN dnf install line (bootstrap, `dnf group install`, package list) carries
    // --enablerepo=codeready-builder-for-rhel-9-aarch64-rpms and the comment appears once;
    // [codeready-builder-rhel9-internal] enabled is passed through verbatim;
    // [crb-mirror-rhel9] is not matched; the -debug- and -source- sections alone emit nothing;
    // no line contains `$(arch)`.
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p inspectah-pipeline containerfile::tests::repo`
Expected: FAIL — `epel-release` renders twice and no CRB flag is emitted.

- [ ] **Step 3: Rewrite the install partition and the Repo-enabling block, with the export-time serving check**

- [ ] **Step 4: Collect `crb_ids` and carry `--enablerepo` on every `dnf ... install -y`**

- [ ] **Step 5: Add the repo-enabling rows to the audit report's Packages section**

- [ ] **Step 6: Run the tests to verify they pass, then the workspace**

Run: `cargo insta test -p inspectah-pipeline --review && cargo test --workspace`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/pipeline
git commit -m "fix(pipeline): install a repo-enabling package from the repo that served it"
```

---

### Task 18: The tmpfiles.d parser and `VarDirBacking` payloads

**Implements § 3.4 ("Parser", the type block). Satisfies § 8.7's `crates/collect/src/inspectors/storage.rs` unit tests, with host check A3 behind them. Satisfies the § 4 consumer rows for the storage merge (`merge.rs:1512`) and the TUI storage row (`screen/single_host.rs:1030`). Blocked on Task 2 reading PASS.**

Closes items 1.6 and the dead-field half of 1.12: the classifier's unanchored `grep -r` plus ancestor walk makes 66 of 66 directories `tmpfiles`, so the one storage advisory that matters never fires.

**Files:**
- Modify: `crates/core/src/types/storage.rs:81-83` (`VarDirectory`, `VarDirBacking`)
- Modify: `crates/collect/src/inspectors/storage.rs:368-398` (the classifier, replaced), `:417-456` (the unit and RPM checks, provenance recorded)
- Modify: `crates/refine/src/projection/types.rs:334-335`, `crates/web/src/adapter.rs:1293-1310`, `crates/tui/src/screen/single_host.rs:1030`, `crates/pipeline/src/render/report.rs:654-660`, `crates/pipeline/src/render/readme.rs:264`, `crates/refine/src/projection/reference.rs:549-550` (`size_estimate` and `recommendation` removed)
- Modify: `crates/core/src/aggregate/merge.rs:1512` (§ 4 row: unchanged in structure; `backing` payloads compare by value, and hosts that disagree keep the first in hostname order)
- Test: `crates/collect/src/inspectors/storage.rs` `#[cfg(test)]`

**Interfaces:**
- Consumes: Task 3's schema 23.
- Produces:
  - `pub enum VarDirBacking { Tmpfiles { conf: String, line: String }, StateDirectory { unit: String }, CacheDirectory { unit: String }, LogsDirectory { unit: String }, RpmOwned { package: String }, Unbacked }` — `#[serde(tag = "kind", rename_all = "snake_case")]`.
  - `VarDirectory { path, backing: Option<VarDirBacking>, mode, owner_name, group_name, owner_uid, owner_gid }`. `size_estimate` and `recommendation` are removed everywhere.

**The parser, exactly (§ 3.4):** `exec.read_dir` over `/etc/tmpfiles.d`, `/run/tmpfiles.d`, `/usr/lib/tmpfiles.d`; a basename in an earlier directory masks the same basename in a later one. Each non-blank, non-comment line splits on whitespace into a type token and a path; the type's first character selects the entry kind and the `+ ! - = ~ ^` modifiers after it are ignored. Entries whose path contains a `%` specifier never match. **A directory is backed by tmpfiles when, and only when, an entry whose type creates a directory at exactly that path exists: `d`, `D`, `v`, `q`, or `Q` with `path == directory`.** Nothing else counts: `z`, `Z`, `e`, `a`/`A`, `h`/`H`, `t`/`T` adjust a path that already exists; `f`, `F`, `w`, `p`, `c`, `b`, `L`, `C` create something other than a directory; `r`, `R`, `x`, `X` remove or exempt. A parent's `d` line does not back its descendants. **The old ancestor walk at `storage.rs:374-398` is deleted outright, not narrowed to a root.** The `StateDirectory=`/`CacheDirectory=`/`LogsDirectory=` and RPM-ownership checks at `:417-456` are unchanged except that the unit filename the grep returns is recorded as `unit` and the owning package as `package`.

- [ ] **Step 1: Write the failing tests** in `crates/collect/src/inspectors/storage.rs` `#[cfg(test)]`

With a `MockExecutor` serving systemd's `var.conf` (`d /var/lib`, `d /var/log`, `d /var/cache`), the host's `appone.conf` (`d /var/lib/appone/cache 0750 appone appone -`), a `labels.conf` with `z /var/lib/zonly 0755 root root -`, `Z /var/lib/zrec - - - -`, `e /var/lib/eonly - - - 30d`, `f /var/lib/afile 0644 root root -`, and (as § 8.14 permits) the real `cleanup.conf` entry `D /run/myapp 0755 root root -` as a non-matching line:

```rust
#[test]
fn only_an_exact_directory_creator_backs_a_directory() {
    // /var/lib/pgsql is Unbacked and unbacked_var_advisory lists it
    // /var/lib/appone/cache is Tmpfiles { conf: "/etc/tmpfiles.d/appone.conf",
    //     line: "d /var/lib/appone/cache 0750 appone appone -" }
    // /var/lib/appone/cache/sub is Unbacked            <- a parent's `d` creates nothing below it
    // /var/lib/zonly, /var/lib/zrec, /var/lib/eonly, /var/lib/afile are all Unbacked
    // /var/lib/foo with only var.conf present is Unbacked
    // a /etc file masks its /usr/lib namesake
    // %-specifier entries match nothing
    // /run/tmpfiles.d/runtime.conf with `d /var/lib/rt` -> Tmpfiles { conf: "/run/tmpfiles.d/runtime.conf", .. }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p inspectah-collect storage::`
Expected: FAIL — the ancestor walk reports `Tmpfiles` for `/var/lib/pgsql`.

- [ ] **Step 3: Replace the classifier with the parser and widen `VarDirBacking` to carry provenance**

- [ ] **Step 4: Remove `size_estimate` and `recommendation` from every consumer in the Files list**

The adapter renders them as `~` and ` (0755 root:root)` today (`adapter.rs:1293-1310`), as do the TUI and the projection. Combined with the classifier fix, 66 rows that say nothing become three that say something.

- [ ] **Step 5: Run the tests to verify they pass, then the workspace**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/core crates/collect crates/refine crates/pipeline crates/web crates/tui
git commit -m "fix(collect): back a /var directory only on an exact tmpfiles creator"
```

---

### Task 19: `effective_var_backing` — reconcile the verdict against what the export carries

**Implements § 3.4 ("Effective backing at export") and § 5.4, open positions 20 and 25. Satisfies § 8.7's `crates/pipeline/tests/var_backing_export_test.rs` (new) in full: the `Provision`, `Unresolved`, `Carried`, and "`/etc` fragments follow the config decision" blocks. Satisfies Review Focus item 4, and the § 4 audit-report Storage row.**

The snapshot verdict says what created the directory on the source host; the export has to say whether that creator reaches the image.

**Files:**
- Create: `crates/pipeline/src/render/var_dirs.rs`
- Modify: `crates/pipeline/src/render/containerfile.rs:1743-1810` (`render_tmpfiles_conf`, `var_dir_section_lines`)
- Modify: `crates/web/src/adapter.rs` (Storage rows derive from the same function)
- Modify: `crates/pipeline/src/render/audit.rs` (source verdict and export outcome side by side)
- Test: `crates/pipeline/tests/var_backing_export_test.rs` (new)

**Interfaces:**
- Consumes: Task 18's `VarDirBacking`; Task 16's `repo_enabling_from` indirectly through the install set.
- Produces:
  - `pub fn effective_var_backing(snap: &InspectionSnapshot) -> Vec<VarDirOutcome>`
  - `pub struct VarDirOutcome { pub path: String, pub source: Option<VarDirBacking>, pub outcome: EffectiveBacking }`
  - `pub enum EffectiveBacking { Carried, Provision { lost: Option<String> }, Unresolved { creator: String, reason: UnresolvedReason } }`
  - `pub enum UnresolvedReason { UnownedFragment, UnownedUnit, BaseUnknown }`

**The two sets the proof is made from (§ 3.4):**
- *Known target base:* `rpm.baseline_package_names` (`rpm.rs:240`) when it is `Some`. When it is `None` there is **no** base set — `service_intent.rs:316` already treats that as unavailable rather than as a universal set, and without a baseline `packages_added` holds every host package, so membership there is a fact about the host, not the target.
- *Exported install set:* the included `packages_added` entries the Containerfile actively installs (`is_package_installable` at `service_intent.rs:70-72`, the filter `containerfile.rs:575-576` applies), or a repo-less entry whose RPM the export carries (a `VerifiedUpload` of § 3.3, or a dnf-cache RPM the payload extraction carries).

A provider package is **carried** when it is in the exported install set or in the known target base; **lost** when a baseline is known and it is in neither; **unknown** when no baseline is known and it is not in the install set.

| Source backing | `Carried` when | `Provision` when | `Unresolved` when |
|---|---|---|---|
| `Tmpfiles { conf }`, `conf` under `/etc/tmpfiles.d/` | the `config.files` entry for `conf` is included | otherwise | never |
| `Tmpfiles`, `conf` under `/usr/lib/tmpfiles.d/` | `rpm.file_ownership` names an owner and it is carried | the owner is known and lost | no `file_ownership` entry names `conf` (`UnownedFragment`), or the owner is unknown (`BaseUnknown`) |
| `Tmpfiles`, `conf` under `/run/tmpfiles.d/` | never | always | never |
| `StateDirectory`, `CacheDirectory`, `LogsDirectory { unit }` | the unit's `config.files` entry is included, or the unit file is RPM-owned by a carried package | the entry exists and is excluded, or the unit's owner is known and lost | the unit is neither a `config.files` entry nor RPM-owned (`UnownedUnit`), or its owner is unknown (`BaseUnknown`) |
| `RpmOwned { package }` | `package` is carried | `package` is lost | `package` is unknown (`BaseUnknown`) |
| `Unbacked` | never | always | never |

**Rendering (§ 5.4):** `Provision` puts the directory into the set `render_tmpfiles_conf` and `var_dir_section_lines` render — a `d` line in `inspectah-var.conf` with the captured mode and ownership, or `RUN mkdir -p` without them, preceded when `lost` is `Some` by `# /var/lib/appone/cache: created on the host by /etc/tmpfiles.d/appone.conf, which this export does not carry`. `Unresolved` renders **no active line**: `inspectah-var.conf` carries

```
# FIXME: confirm the base image creates /var/lib/vendor; on the host /usr/lib/tmpfiles.d/vendor.conf created it and no package owns that file; if the base does not, uncomment the next line
#d /var/lib/vendor 0755 root root -
```

and the Containerfile's Storage block carries the same two-line FIXME in the § 5.6 form so the attention strip counts it. The snapshot verdict is never rewritten.

**View (§ 3.4):** the adapter emits one summary `ContextItem` for the `Carried` set, one `ContextItem` with `advisory` per `Provision` outcome with `rationale` naming the lost creator when there was one, and one `ContextItem` with `advisory` per `Unresolved` outcome, the same `advisory_type: unbacked_var_dir` with `rationale` `created on the host by <creator>; this export cannot tell whether the base image carries it (<reason text>)`, counted in the summary row's `<U> unbacked`. A carried directory with a non-root owner or a mode other than `0755` also gets an inventory row. Nothing else reaches the view as a row.

- [ ] **Step 1: Write the failing tests** in `crates/pipeline/tests/var_backing_export_test.rs` (new), through `render_refine_export`

```rust
#[test]
fn provisions_what_the_export_does_not_carry() {
    // with nothing excluded, inspectah-var.conf contains
    //   `d /var/lib/pgsql <mode> <owner> <group> -` and no appone line;
    // with /etc/tmpfiles.d/appone.conf excluded under Configuration Files it gains
    //   `d /var/lib/appone/cache 0750 appone appone -` preceded by the lost-creator comment,
    //   the tarball lacks config/etc/tmpfiles.d/appone.conf, and /api/snapshot/sections
    //   carries an advisory row whose rationale names appone.conf;
    // including it again removes all three;
    // a /run/tmpfiles.d directory is provisioned with its comment unconditionally;
    // a StateDirectory { unit: "myapp.service" } whose unit file is excluded is provisioned,
    //   and carried when the unit is included;
    // an RpmOwned { package } absent from both the known baseline and the included install
    //   set is provisioned.
}

#[test]
fn unresolved_renders_a_fixme_and_no_active_line() {
    // Each case asserts: an advisory row in /api/snapshot/sections whose rationale names the
    // creator and the reason; a two-line FIXME in the Containerfile's Storage block counted by
    // the attention strip; a commented `#d` line and no active `d` line in inspectah-var.conf;
    // and never a Carried summary membership.
    //  (a) /var/lib/vendor backed by /usr/lib/tmpfiles.d/vendor.conf that no file_ownership
    //      entry names, with a known baseline                         -> UnownedFragment
    //  (b) /var/lib/appone/cache backed by Tmpfiles { conf: "/usr/lib/tmpfiles.d/appone.conf" }
    //      where file_ownership names `appone` as the owner, `appone` is an excluded
    //      packages_added entry, and baseline_package_names is None   -> BaseUnknown
    //  (c) RpmOwned { package: "appone" } with appone excluded and no baseline -> BaseUnknown
}

#[test]
fn provisioned_not_unresolved_and_carried_cases() {
    // the same /usr/lib fragment with appone excluded and a known baseline that lacks it
    //   yields the active `d` line and the lost-creator comment naming
    //   /usr/lib/tmpfiles.d/appone.conf;
    // appone included and installable yields no line, with and without a baseline.
}

#[test]
fn etc_fragments_follow_the_config_decision_never_the_provider() {
    // /var/lib/appone/cache backed by /etc/tmpfiles.d/appone.conf is Carried when that
    // config.files entry is included and Provision with the lost-creator comment when it is
    // excluded, in both cases whatever appone's disposition and whether baseline_package_names
    // is Some or None, and the outcome is never Unresolved (§ 3.4 table, first row).
}

#[test]
fn absent_sections_provision_rather_than_panic() {
    // Review Focus 4: a snapshot with storage directories but no `config` section and no
    // `rpm` section yields Provision for every directory and no panic.
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p inspectah-pipeline --test var_backing_export_test`
Expected: FAIL — the function does not exist and the renderer reads the snapshot verdict directly.

- [ ] **Step 3: Implement `effective_var_backing` in `crates/pipeline/src/render/var_dirs.rs`**

- [ ] **Step 4: Point `render_tmpfiles_conf`, `var_dir_section_lines`, the adapter, and the audit report at it**

The adapter derives the Storage advisory rows from the same function, so excluding `appone.conf` under Configuration Files makes `/var/lib/appone/cache` an advisory row with a `d` line in the export, and including it again removes both: the view reflects the export.

- [ ] **Step 5: Run the tests to verify they pass, then the workspace**

Run: `cargo insta test -p inspectah-pipeline --review && cargo test --workspace`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/pipeline crates/web
git commit -m "feat(pipeline): reconcile /var backing against what the export carries"
```

---

### Task 20: The config walker owns PAM, audit, tuned profiles, and the kernel directories

**Implements § 3.6 in full and § 3.5's "Config walker changes" and "Tuned", open position 9. Satisfies § 8.11 in full. Satisfies the § 4 consumer rows for the PAM/audit aggregate merge (`merge.rs:1619-1660`), classification (`classify.rs:782-785`), and the web PAM pointer list (`reference.rs:863-897`).**

Closes items 1.2 and 1.8: PAM and audit rules ship with no disposition, and RPM-owned unmodified `modprobe.d` files ship in every image.

**Files:**
- Modify: `crates/collect/src/inspectors/config/walk.rs:183`, `:209-213` (`CROSS_INSPECTOR_EXCLUDE_PREFIXES` and its loop, deleted), `:155` (`/etc/tuned/*/tuned.conf` leaves `UNOWNED_EXCLUDE_GLOBS`)
- Modify: `crates/collect/src/inspectors/config/mod.rs:376` (the authselect rule, next to `is_rpm_owned_via_symlink`)
- Modify: `crates/core/src/types/config.rs` (`ConfigCategory` gains `Pam` and `Tuned`)
- Modify: `crates/collect/src/inspectors/selinux.rs:21-30`, `:396-485` (`collect_pam_configs`, `collect_audit_rules`, `pam_configs`, `audit_rules`, `CarryForwardFile`, deleted)
- Modify: `crates/pipeline/src/render/configtree.rs:525-547` (the carry-forward block, deleted), `:150`, `:231` (`TUNED_PREFIX` and the `tuned/` root, deleted)
- Modify: `crates/pipeline/src/render/containerfile.rs:1464-1476` (the two comment lines, deleted), `:1365` (`COPY tuned/etc/tuned/ /etc/tuned/`, deleted)
- Modify: `crates/core/src/aggregate/merge.rs:1619-1660`, `crates/collect/src/inspectors/config/classify.rs:782-785`, `crates/refine/src/projection/reference.rs:863-897`
- Test: `crates/collect/tests/config_test.rs`, `crates/pipeline/tests/smoke_render_2c.rs`

**Interfaces:**
- Consumes: Task 10's SELinux type changes (the same file).
- Produces: `ConfigCategory::Pam` for `/etc/pam.d/` and `ConfigCategory::Tuned` for `/etc/tuned/`. `ConfigCategory::Audit` already classifies exactly `/etc/audit/rules.d/` (`classify.rs:20-23`) and is **not** renamed (position 9). `project_ref_selinux` keeps one `GenericRefItem` per `config.files` entry with category `Pam` or `Audit`: key is the path under `/etc`, summary `decided under Configuration Files`, no content.

**The authselect rule (§ 3.6), exactly:** an `rpm_va` entry under `/etc/pam.d/` whose flags are exactly `L` and whose symlink target resolves under `/etc/authselect/` is skipped. The customization lives in `/etc/authselect/`, which the walker already collects as unowned files. Without it, the walker's `rpm_va` path classifies `system-auth`, `password-auth`, `fingerprint-auth`, `smartcard-auth`, and `postlogin` as `rpm_owned_modified` and copies the target's content.

The eight `EXCLUDED_PAM_CONFIGS` entries at `selinux.rs:21-30` already appear verbatim in the walker's exact-exclusion list at `walk.rs:102-110`, so nothing moves. The three kernel directories are already walked. RPM-owned unmodified files are already skipped by the walker's ownership check, which is the filter item 1.2 needs and the reason no `SnippetOrigin` is introduced. `active_profile`, `profile_mode`, and `bootcmdline` stay excluded at `walk.rs:112-115` because `tuned_disposition` renders them.

- [ ] **Step 1: Write the failing tests**

In `crates/collect/tests/config_test.rs`:

```rust
#[test]
fn pam_audit_and_tuned_are_config_entries() {
    // § 8.11 + § 8.10: /etc/pam.d/custom-sshd has category Pam;
    // /etc/audit/rules.d/driftify.rules has category Audit;
    // /etc/tuned/myapp/tuned.conf has category Tuned.
}

#[test]
fn an_authselect_symlink_is_not_a_local_pam_file() {
    // an rpm_va entry `....L....` for /etc/pam.d/system-auth pointing into /etc/authselect/
    // produces no entry.
}
```

In `crates/pipeline/tests/smoke_render_2c.rs`:

```rust
#[test]
fn pam_and_audit_travel_through_the_config_tree() {
    // excluding the PAM entry leaves config/etc/pam.d/custom-sshd out of the tarball and the
    // audit report lists it as excluded; render_audit output contains the path under
    // Configuration Files.
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p inspectah-collect --test config_test && cargo test -p inspectah-pipeline --test smoke_render_2c`
Expected: FAIL — the two prefixes are excluded from the walk and the files arrive as `CarryForwardFile`.

- [ ] **Step 3: Delete `CROSS_INSPECTOR_EXCLUDE_PREFIXES`, add the two categories, and add the authselect rule**

- [ ] **Step 4: Delete the SELinux collector's PAM and audit paths, the configtree carry-forward block, the `tuned/` root, and the two Containerfile comment lines**

- [ ] **Step 5: Update the three § 4 consumers: the PAM/audit merge, classification, and the SELinux reference pointer list**

`merge.rs:1619-1660` is deleted; the files merge as `config.files` entries through the existing config merge. `classify.rs:782-785` reads `config.files` entries with category `Tuned` instead of `tuned_custom_profiles` paths. The audit report covers both through its existing `config.files` loops (`audit.rs:161`, `:472`, `:492`).

- [ ] **Step 6: Run the tests to verify they pass, then the workspace**

Run: `cargo insta test -p inspectah-pipeline --review && cargo test --workspace`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/core crates/collect crates/pipeline crates/refine
git commit -m "fix(collect): give PAM, audit, and tuned files one owner and one decision"
```

---

### Task 21: Kernel & Boot — one decision per entity

**Implements § 3.5 (the field removals, `shadowed_by`, `finalize_cross_inspector`, Validation, Projection), open positions 6 and 22. Satisfies § 8.10 in full apart from the schema-version half Task 3 owns: `crates/collect/tests/kernelboot_test.rs`, `crates/cli/tests/scan_shadow_test.rs` (new), `crates/refine/tests/session_test.rs`'s `InventoryNotToggleable` cases, and the tuned FIXME case in `containerfile.rs`. Satisfies the § 4 consumer rows for the kernel aggregate merge (`merge.rs:1685-1695`, `:1731-1755`), redaction (`engine.rs:790-805`, `:990-1028`), the TUI counts (`sections.rs:254-263`), and the web snippet items (`adapter.rs:949-968`).**

Closes item 1.7: Kernel & Boot ships two decisions nothing mounts and holds 66 phantom ones.

**Files:**
- Modify: `crates/core/src/types/kernelboot.rs:39-40` (`loaded_modules[].disposition` default), and the removal of `modules_load_d`, `modprobe_d`, `dracut_conf`, `tuned_custom_profiles`, `non_default_modules`, `ConfigSnippet`
- Modify: `crates/core/src/types/config.rs` (`ConfigFileEntry.shadowed_by`, `ConfigShadow`)
- Modify: `crates/pipeline/src/collect.rs:233-235` (`finalize_cross_inspector` called from `collect()`)
- Modify: `crates/refine/src/types.rs` (`ItemId::KernelModule` removed), `crates/refine/src/session.rs:1383-1399` (the `ItemId::Config` validate arm), `:1521` (the `KernelModule` validate arm, removed)
- Modify: `crates/tui/src/app.rs:59`, `crates/web/src/handlers.rs:826-852`, `crates/web/src/aggregate_handlers.rs:1278-1290`
- Modify: `crates/refine/src/projection/types.rs` (`RefKernelBoot.customizations_from_config`), `crates/web/src/adapter.rs:949-968`
- Modify: `crates/core/src/aggregate/merge.rs:1685-1695`, `:1731-1755`; `crates/pipeline/src/redact/engine.rs:790-805`, `:990-1028`; `crates/tui/src/sections.rs:254-263`
- Modify: `crates/pipeline/src/render/containerfile.rs` (the tuned activation FIXME)
- Test: `crates/collect/tests/kernelboot_test.rs`, `crates/cli/tests/scan_shadow_test.rs` (new), `crates/refine/tests/session_test.rs`

**Interfaces:**
- Consumes: Task 20's `ConfigCategory::Tuned` and the walked kernel directories.
- Produces:
  - `ConfigFileEntry.shadowed_by: Option<ConfigShadow>`, `#[serde(default, skip_serializing_if = "Option::is_none")]`; `pub enum ConfigShadow { Sysctl }`. The entry's disposition is `Inventory` whenever this is `Some`.
  - `pub fn finalize_cross_inspector(snapshot: &mut InspectionSnapshot)` in `crates/pipeline/src/collect.rs`.
  - `loaded_modules[].disposition` defaults through `#[serde(default = "crate::types::finding::default_finding_inventory")]`, and the collector sets `FindingKind::inventory()`.
  - `RefKernelBoot` loses the snippet lists and gains `customizations_from_config: Vec<RefConfigRef { path, category, disposition, shadowed_by }>`.

**Where the pass runs, and why (§ 3.5):** `finalize_cross_inspector` is called by `collect()` after wave 2 has been joined and before `Pipeline { state: Collected { snapshot } }` is returned (`collect.rs:233-235`). That boundary is the one every collection path crosses: the CLI scan calls `collect()` directly and then `validate` and `redact` (`scan.rs:608-615`, `:653-660`, `:689-692`) and never `run_pipeline()`; `run_pipeline()` (`orchestrate.rs:37-59`) calls the same `collect()`. Placing it in `orchestrate.rs` would let the production scan bypass it.

**The pass:** when `config` and `kernel_boot` are both `Some`, every `config.files` entry whose path appears in `kernel_boot.sysctl_overrides[].source` gets `shadowed_by: Some(Sysctl)` and `disposition: FindingKind::inventory()`.

**The criterion is the override list, not the inspector's degraded flag (position 22).** A failed sysctl collection returns `Err` and the section gets `sysctl_overrides: Vec::new()` (`kernelboot.rs:91-98`), so a source path can appear in the list only when that collection succeeded. The outcome per file is therefore always one of two: the file produced an override, it is `Inventory` with `shadowed_by`, and its content reaches the image only through the sysctl decisions; or it produced none — whether because the collection failed, the file was not read, or its keys match the runtime — and it is an ordinary actionable entry copied as plain config. An inspector degraded for an unrelated reason changes nothing about which of the two applies.

**Validation:** `validate_target`'s `ItemId::Config` arm gains `if entry.disposition.is_inventory() { return Err(RefineError::InventoryNotToggleable(path.clone())) }`. The variant exists (`types.rs:720`, 422 at `error.rs:33-36`) and `validate_target` already runs before `is_op_noop`, so both directions on a shadowed Config target return 422 without extending the timeline, while `ItemId::Sysctl` for the same key stays operable. Shadowed entries are outside every effective bulk set.

**Tuned:** `/etc/tuned/myapp/tuned.conf` is a `ConfigCategory::Tuned` entry with its own `ItemId::Config`, copied through `config/etc/` like any file. `tuned_disposition` remains the activation decision and gates `active_profile`, `profile_mode`, and `systemctl enable tuned.service`. When activation is included and `tuned_active` names a profile whose `config.files` entry `/etc/tuned/<profile>/tuned.conf` exists but is excluded, the renderer emits, in place of the activation lines:

```
# FIXME: include /etc/tuned/<profile>/tuned.conf under Configuration Files or activate a stock profile; the active profile is custom and its file is excluded
```

**Projection:** `customizations_from_config` is built from `config.files` entries under `/etc/modules-load.d/`, `/etc/modprobe.d/`, `/etc/dracut.conf.d/`, `/etc/sysctl.d/`, `/etc/tuned/`. The adapter renders them as `ContextItem`s titled by **path under `/etc`**, not basename (item 1.14), with `decision` filled from the config entry so the row is a live mirror — Space posts `ItemId::Config`, the same op the Configuration Files row posts — or, for shadowed entries, no `decision` and the subtitle `merged into 99-inspectah-migrated.conf`. (The `decision` field itself is Task 23's; wire the projection here and fill the DTO there.)

- [ ] **Step 1: Write the failing tests**

In `crates/collect/tests/kernelboot_test.rs`: `loaded_modules[].disposition` deserializes to `Inventory` when the key is absent.

In `crates/core/tests/snapshot_test.rs`: a schema 23 snapshot round-trips and its serialized `kernel_boot` has no `modules_load_d`, `modprobe_d`, `dracut_conf`, `tuned_custom_profiles`, or `non_default_modules` key, and its `selinux` has no `pam_configs` or `audit_rules` key. Serde ignores unknown fields, so a 23 payload carrying an old key loads and drops it; that is accepted and not tested as a failure.

In `crates/cli/tests/scan_shadow_test.rs` (new), through the `scan` command's own code path with a `MockExecutor` and **not** `run_pipeline()`, each case against the final phase 0 renderer:

```rust
#[test]
fn a_sysctl_source_file_is_inventory_and_its_keys_are_the_decision() {
    // /etc/sysctl.d/99-driftify.conf is Inventory with shadowed_by: Some(Sysctl) in the
    // written snapshot while hugepages.conf stays actionable.
}

#[test]
fn degraded_for_an_unrelated_reason_still_shadows() {
    // /proc/cmdline unreadable while the sysctl files and `sysctl -a` answer normally:
    // the section is in completeness.degraded with a non-empty sysctl_overrides, and
    //  - the written snapshot still shadows 99-driftify.conf
    //  - /api/view `sysctls` and the Kernel & Boot rows of /api/snapshot/sections carry
    //    ItemId::Sysctl for its keys and no `decision` for the file
    //  - the Containerfile copies sysctl/etc/sysctl.d/99-inspectah-migrated.conf and nothing
    //    names config/etc/sysctl.d/99-driftify.conf
    //  - the tarball has the synthesized file and not the source file
    //  - SetInclude(Sysctl(key), false) removes the key from the synthesized file and leaves
    //    the source file absent; undo restores the key; redo removes it again
    //  - SetInclude(Config("/etc/sysctl.d/99-driftify.conf"), ..) is 422 in both directions
    //    throughout
}

#[test]
fn a_failed_sysctl_collection_leaves_both_files_actionable() {
    // `sysctl -a` fails while /proc/cmdline reads, so the section is degraded with an empty
    // sysctl_overrides: neither entry is shadowed, both stay actionable, the Containerfile
    // copies config/etc/sysctl.d/99-driftify.conf and no 99-inspectah-migrated.conf exists,
    // /api/view offers no sysctl decision, and SetInclude(Config(..), false) on the file
    // removes it from the tarball. With the kernel inspector absent, the same.
}
```

In `crates/refine/tests/session_test.rs`: `apply(SetInclude(Config("/etc/sysctl.d/99-driftify.conf"), true))` and `.., false)` both return `Err(InventoryNotToggleable(_))` with `timeline.len()` unchanged, while `SetInclude(Sysctl("vm.swappiness"), false)` is `Ok` and the export loses the key.

In `crates/pipeline/src/render/containerfile.rs` `#[cfg(test)]`: an excluded custom profile with activation included renders the § 3.5 FIXME and no `active_profile` line; `COPY tuned/` never appears.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p inspectah-cli --test scan_shadow_test && cargo test --workspace`
Expected: FAIL — nothing sets `shadowed_by` and `ItemId::KernelModule` still exists.

- [ ] **Step 3: Add `shadowed_by`, `ConfigShadow`, and `finalize_cross_inspector`, called from `collect()`**

- [ ] **Step 4: Remove the five `KernelBootSection` fields, `ConfigSnippet`, and `ItemId::KernelModule`**

The `ItemId::KernelModule` removal touches `session.rs:1521`, the projection arm, `crates/tui/src/app.rs:59`, `handlers.rs:826-852`, and `crates/web/src/aggregate_handlers.rs:1278-1290` — the aggregate Kernel & Boot item list drops loaded modules, which are inventory now. That is the one aggregate touch the enum removal forces (§ 9).

- [ ] **Step 5: Add the `Inventory` refusal to the `ItemId::Config` validate arm**

- [ ] **Step 6: Build `customizations_from_config` in the projection and retitle the rows by path under `/etc`**

- [ ] **Step 7: Update the four § 4 consumers: kernel merge, redaction, TUI counts, web snippet items**

`merge.rs` deletes the five fields and `dedup_config_snippets`, and `KernelBootSection` construction loses them. `engine.rs` deletes both loops; the same files are redacted as `config.files` content by the existing config pass, and fcontext rules carry no free text and are not scanned. `sections.rs:254-263` counts `sysctl_overrides`, `alternatives`, and `customizations_from_config`.

- [ ] **Step 8: Run the tests to verify they pass, then the workspace**

Run: `cargo insta test --workspace --review && cargo test --workspace`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/
git commit -m "fix(core): put each kernel and boot decision on exactly one entity"
```

---

### Task 22: configtree writes only from `config.files`

**Implements § 5.3 and the last of § 3.5 ("The renderer's `sysctl_source_paths` skip is deleted in phase 0 with the pass"). Satisfies § 8.1 in full. Closes item 1.1.**

`write_config_tree` copies `config.files` entries gated on `disposition.is_included()`, then rewrites three kernel snippet lists with no gate at all, so a Configuration Files exclusion is silently overridden at export.

**Files:**
- Modify: `crates/pipeline/src/render/configtree.rs:212` (the one gate that stays), `:285-311` (the ungated kernel snippet block, deleted), `:133-140`, `:228` (the `sysctl_source_paths` skip, deleted)
- Test: `crates/pipeline/tests/configtree_single_source_test.rs` (new)

**Interfaces:**
- Consumes: Task 21's `shadowed_by` and the removed snippet lists; Task 20's deleted carry-forward block and `tuned/` root.
- Produces: `write_config_tree` with one source for `/etc` content, `config.files`, gated on `is_included()`. The sysctl synthesis at `configtree.rs:327-345` and the kargs file are unchanged.

**Why the skip is deleted rather than kept as a guard (§ 3.5):** `sysctl_source_paths` skipped an included, actionable entry, which under the shadowing rule is a file that owns itself. The `is_included()` gate at `:212` is the only gate. The § 8.10 degraded-scan test of Task 21 is the regression test for the deletion.

- [ ] **Step 1: Write the failing test** in `crates/pipeline/tests/configtree_single_source_test.rs` (new)

```rust
#[test]
fn a_config_exclusion_is_not_overridden_by_a_kernel_list() {
    // § 8.1, replacing the executed throwaway.
    let snap = snapshot_with_excluded(/* /etc/modules-load.d/driftify.conf */);
    let out = write_config_tree(&snap, dir.path()).unwrap();
    assert!(!dir.path().join("config/etc/modules-load.d/driftify.conf").exists());
    assert_eq!(out.roots, Vec::<String>::new());
}

#[test]
fn an_included_config_entry_is_written() {
    let snap = snapshot_with_included(/* the same path */);
    let out = write_config_tree(&snap, dir.path()).unwrap();
    assert!(dir.path().join("config/etc/modules-load.d/driftify.conf").exists());
    assert_eq!(out.roots, vec!["etc".to_string()]);
}
```

The spec's first case populates `kernel_boot` with sysctls and tuned so the snapshot is realistic; after Task 21 the snippet lists no longer exist, so the second source the test guarded against is gone by construction and the assertion is that the gate is the only path.

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p inspectah-pipeline --test configtree_single_source_test`
Expected: FAIL until the ungated block and the skip are gone.

- [ ] **Step 3: Delete the ungated kernel snippet block and the `sysctl_source_paths` skip**

- [ ] **Step 4: Run the test to verify it passes, then the workspace**

Run: `cargo insta test -p inspectah-pipeline --review && cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/pipeline
git commit -m "fix(pipeline): write /etc content from config files alone"
```

---

### Task 23: `ContextItem` gains `decision` and `advisory`, and `empty_reason` everywhere

**Implements § 3.8 and the `/api/snapshot/sections` and `/api/view` bullets of § 6.9, open positions 7 and 8. Satisfies § 8.12's Rust half: the `crates/web/tests/contract_snapshots.rs` fixture move, the reviewed snapshot deltas, `plain_context_item_is_byte_identical`, the `grouped_view` additions, and the `advisory.advisory_type == "unbacked_var_dir"` assertion.**

The server side only. Phase 1 renders these fields; phase 0 ships them so phase 1 is a web-only release.

**Files:**
- Modify: `crates/web/src/web_types.rs:102-108` (`ContextItem`), `:130-140` (already widened by Task 7), `:231-253`
- Modify: `crates/web/src/adapter.rs:70`, `:191-218` (the sysctl and tuned DTOs stay until phase 3)
- Test: `crates/web/tests/contract_snapshots.rs`

**Interfaces:**
- Consumes: Task 7's `RepoGroupInfo` counts; Task 6's `GroupInfo` counts and `render_state: "inert"`; Task 19's `effective_var_backing`; Task 21's `customizations_from_config` and `shadowed_by`; Task 16's `repo_attribution`, `repo_enabling_from`, `uploaded`.
- Produces:
  - `ContextItem` gains `#[serde(skip_serializing_if = "Option::is_none")] pub decision: Option<ContextDecision>` and `pub advisory: Option<ContextAdvisory>`.
  - `pub struct ContextDecision { pub item_id: ItemId, pub include: bool, pub locked: bool }`
  - `pub struct ContextAdvisory { pub advisory_type: AdvisoryType, pub rationale: String }`
  - `item_id` serializes as `ItemId` does in `/api/op` payloads (`#[serde(tag = "kind", content = "key")]`, `types.rs:57`), so a sysctl row carries `"item_id": {"kind":"Sysctl","key":{"key":"vm.swappiness"}}` and a config-backed row `{"kind":"Config","key":{"path":"/etc/modprobe.d/driftify.conf"}}`.

**Where `decision` is filled:** `tuned_active` (`ItemId::TunedSelection`, from `tuned_disposition`), each sysctl row (`ItemId::Sysctl`), and each config-backed Kernel & Boot row (`ItemId::Config`). `advisory` is filled for unbacked `/var` rows from `UnbackedVarAdvisory` and for Task 19's `Provision` and `Unresolved` outcomes. `empty_reason` is set for every empty reference section: `data_unavailable` when the snapshot section is `None`, `zero_drift` when it is present and projects to no items.

**The only byte-identity this spec promises:** both fields are skipped when `None`, so a plain reference item with neither serializes byte-identically. The endpoint snapshots change for reasons the spec intends, and § 8.12 names each delta.

- [ ] **Step 1: Write the failing tests** in `crates/web/tests/contract_snapshots.rs`

```rust
#[test]
fn plain_context_item_is_byte_identical() {
    // Serialize one ContextItem with neither decision nor advisory against an inline
    // snapshot taken BEFORE the change. Write this test and its snapshot first.
}
```

Then move the fixture (`contract_snapshots.rs:57-67`, `:144-201`): its `audit_rules` and `pam_configs` files become `config.files` entries with category `Audit` and `Pam` (actionable, included), and it drops `modules_load_d`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p inspectah-web --test contract_snapshots`
Expected: FAIL — `decision` and `advisory` do not exist.

- [ ] **Step 3: Add the two optional fields and fill them in the adapter**

- [ ] **Step 4: Set `empty_reason` on every empty reference section**

- [ ] **Step 5: Re-accept the two endpoint snapshots with exactly these reviewed deltas and no others**

In `contract_sections`: the SELinux section loses its two carry-forward rows and gains the two pointer items (`decided under Configuration Files`); the Kernel & Boot `modules_load_d` row leaves; the sysctl row gains `decision` with `item_id == {"kind":"Sysctl","key":{"key":"net.ipv4.ip_forward"}}`; every empty section gains `empty_reason`.

In `contract_view`: `config_files` gains the two entries with their `decision` and `output_path` under `config/etc/pam.d/` and `config/etc/audit/rules.d/`; `packages[0].entry` gains `repo_attribution: {"kind":"unrecorded"}`; `repo_groups[]` gains the three counts; the Containerfile preview loses the two PAM/audit comment lines and the `restorecon` line.

In `grouped_view`: `effective_member_count`, `effective_included`, and `render_state` values appear; a new assertion checks an unbacked `/var` item carries `advisory.advisory_type == "unbacked_var_dir"`.

Run: `cargo insta test -p inspectah-web --review`
Any delta outside this list is a bug in an earlier task, not a snapshot to accept.

- [ ] **Step 6: Run the workspace**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/web
git commit -m "feat(web): carry a row's decision and advisory on the context item"
```

---

### Task 24: One state behind every decision surface

**Implements § 3.9, open position 15. Satisfies § 8.15 in full: `crates/web/tests/decision_surface_test.rs` (new).**

Two surfaces cache a projection of `self.original` for the server's lifetime. Once sysctl, tuned, and config-backed rows carry `decision` (Task 23), a cached reference keeps answering with the original include state: a Config exclusion removes the file from the export while its Kernel & Boot mirror stays checked, and the next Space posts the same exclusion again.

**Files:**
- Modify: `crates/web/src/handlers.rs:64-68` (`AppState.sections_cache`, deleted), `:1121-1126` (`get_sections`)
- Modify: `crates/refine/src/session.rs:2803-2808` (`cached_reference`, deleted; `reference()` recomputed)
- Test: `crates/web/tests/decision_surface_test.rs` (new)

**Interfaces:**
- Consumes: Task 23's `decision` and `advisory`; Task 19's `effective_var_backing`.
- Produces: `RefineSession::reference()` computed on call as `project_reference(&self.snapshot_projected())`; `get_sections` builds `build_web_sections(session.reference())` under the lock on each request.

**No cache or cache layer is added anywhere to satisfy this section.** The reference projection is a few hundred rows of strings and the GET runs once per op, so nothing measurable is lost; the one mutation-aware cache that exists, `cached_view` with `generation`, is the model and is already invalidated on every path.

- [ ] **Step 1: Write the failing test** in `crates/web/tests/decision_surface_test.rs` (new)

One table-driven test over the surfaces of § 3.9. Each case: `GET` the surface, `POST /api/op`, `GET` again, `POST /api/undo`, `GET`, `POST /api/redo`, `GET`, and at each step `render_refine_export` on the same session; assert the include state the surface reports equals what the Containerfile and tarball render.

- `/api/snapshot/sections`, `SetInclude(Config("/etc/modprobe.d/driftify.conf"), false)`: the Kernel & Boot row's `decision.include` is `false` after the op and `config/etc/modprobe.d/driftify.conf` is absent from the tarball; both revert on undo, both re-apply on redo. The same for `SetInclude(Sysctl, false)` (row and `99-inspectah-migrated.conf` content) and `SetInclude(TunedSelection, false)` (row and the `active_profile` line).
- `/api/snapshot/sections`, Storage: `SetInclude(Config("/etc/tmpfiles.d/appone.conf"), false)` produces the advisory row and the `d` line; undo removes both.
- `/api/view`, `SetInclude(Repo, false)` then `SetInclude(Package, true)` on one of its members: `repo_groups[]` counts and `packages[]` dispositions match the install list at each step.
- `/api/view`, the group sequence of § 8.8 (`render_state` versus `dnf group install`).
- `POST /api/reset` and a `resume_from` of the autosaved session both return surfaces equal to a fresh export of the same timeline.

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p inspectah-web --test decision_surface_test`
Expected: FAIL on the first case — the cached sections report `true` after the op. This is the regression test for position 15.

- [ ] **Step 3: Delete `AppState.sections_cache` and `RefineSession::cached_reference`**

- [ ] **Step 4: Run the test to verify it passes, then the workspace**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/web crates/refine
git commit -m "fix(web): project the reference sections from the live session"
```

---

## Documentation and Release

### Task 25: Skill file updates

**Implements § 9's last bullet, the five skill files phase 0's implementation must update. Satisfies no § 8 entry: § 8 contains no test for a skill file, and the repo's own `.githooks/pre-commit` fast-paths over a docs-only commit. The acceptance is the content named below.**

**Files:**
- Modify: `process-docs/skills/serde-include-default-ambiguity.md`
- Modify: `process-docs/skills/snapshot-schema-versioning.md`
- Modify: `process-docs/skills/web-disposition-contract.md`
- Modify: `process-docs/skills/rpm-repo-name-mismatch.md`
- Modify: `process-docs/skills/codebase-layout.md`
- Modify: `process-docs/skills/index.md` (only if a cross-reference changes; no new skill file is created)

**Interfaces:**
- Consumes: every preceding task.
- Produces: skill text that matches the shipped code.

- [ ] **Step 1: `serde-include-default-ambiguity.md` — the patch inserts a `disposition` key, not an `include` key**

The file describes `patch_missing_includes` as patching `include`. It patches `disposition` (`crates/refine/src/normalize.rs:64`), and § 4 states the text is stale on exactly that point. Correct the description and note that every disposition-bearing field phase 0 adds is a tagged `FindingKind` whose absent-key default is named on the field, which carries no absent-versus-false ambiguity.

- [ ] **Step 2: `snapshot-schema-versioning.md` — the constants**

Set `SCHEMA_VERSION` to 23 and `MIN_SCHEMA` to 23 wherever the file quotes them, and keep the exact-match stance the file already describes, which the code now matches again.

- [ ] **Step 3: `web-disposition-contract.md` — the `ContextItem` decision path and the `Inventory` bucket**

Add: a `ContextItem` may carry `decision` (`ItemId`, `include`, `locked`) and `advisory` (`advisory_type`, `rationale`), both skipped when `None` so a plain reference item is byte-identical; the `sysctls` and `tuned` DTOs on `ViewResponse` are retired in phase 3, not phase 0; and the `Inventory` bucket now holds `loaded_modules` and sysctl-shadowed config entries, which `validate_target` refuses in both directions with `InventoryNotToggleable`.

- [ ] **Step 4: `rpm-repo-name-mismatch.md` — the substring rule is replaced**

Replace the first-substring-hit description with § 3.2's rule: exact case-insensitive match against any section wins first (`Exact`); otherwise substring against enabled sections only, one match `Resolved`, several `Ambiguous`; then `Pseudo`, `Unrecorded`, `Stale`. Record that the rule is order-independent by construction and that `RepoId` folds ASCII case for `Eq`, `Hash`, and `Ord` while `Display` keeps the host's spelling.

- [ ] **Step 5: `codebase-layout.md` — the three new modules**

Add `crates/refine/src/rpm_filename.rs`, `crates/pipeline/src/render/selinux.rs`, and `crates/pipeline/src/render/var_dirs.rs` with one line each on their role.

- [ ] **Step 6: Commit**

```bash
git add process-docs/skills
git commit -m "docs(skills): bring the five skills phase 0 changed back in line with the code"
```

---

### Task 26: CHANGELOG entries and the § 4 consumer checklist walk

**Implements § 9's last bullet ("adds `[Unreleased]` entries for the Containerfile changes") and § 4's consumer checklist. Satisfies no § 8 entry directly; the acceptance is the entry text below and a clean full-workspace run.**

**Files:**
- Modify: `CHANGELOG.md` (under `## [Unreleased]`)
- Modify: this plan document (the checklist walk's result)

**Interfaces:**
- Consumes: every preceding task.
- Produces: a release-ready `[Unreleased]` section.

- [ ] **Step 1: Add the `[Unreleased]` entries**

Keep a Changelog 1.1.0 categories, the repo's `- **Label** — description` form (`process-docs/changelog-policy.md`). Write, under `## [Unreleased]`:

```markdown
### Changed
- **File context rules are replayed as real commands** — custom `semanage fcontext` rules were collected as raw table rows and every one of them failed the renderer's safety check, so the generated build carried a FIXME and a `restorecon -Rv /` line instead of the rules. They are now collected as typed records in policy-store order and emitted as `semanage fcontext` commands that are valid against the target's policy: each rule removes its own key from the target's local store, then adds it, or modifies it when the target policy already defines the key. There is no build-time `restorecon`, because a bootc target labels the deployed tree from the image's own policy. On bases where `/srv` is a symlink into `/var`, a `/srv` equivalence is added once so the rules reach the real path.
- **A repo-enabling package installs from the repo that served it** — a `-release` package installed outside any repo rendered twice, once as an active install and once as a manual block, and the build could resolve neither reliably. Such a package is now recognised only when the host's enabled repos actually served it, the serving repo ids travel in the snapshot, and the export keeps the active install only while one of those repo definitions is still carried, enabled, and keyed. When none is, the package is demoted to the upload path with an instruction naming the repo. A CodeReady Builder section enabled on the source host is carried as `--enablerepo` on every `dnf` transaction the build emits.
- **`/etc` content is written from the Configuration Files decisions alone** — the export rewrote the kernel snippet directories after honouring those decisions, with no include check, so excluding a file under Configuration Files did not keep it out of the image, and RPM-owned unmodified `modprobe.d` files shipped in every build. There is now one source for `/etc` content. PAM files, audit rules, and custom tuned profiles become ordinary Configuration Files entries with their own include decision and their own rows in the reports.
- **A `/var` directory is backed only by a tmpfiles entry that creates it** — backing was decided by an unanchored recursive grep that walked up to `/var`, so a stock `d /var/lib` line marked every directory under it as backed and the unbacked-directory advisory never fired. Only a `d`, `D`, `v`, `q`, or `Q` entry at exactly that path, a unit `StateDirectory=`/`CacheDirectory=`/`LogsDirectory=`, or RPM ownership counts. The export then checks whether that creator reaches the image and provisions the directory when it does not, or prints an instruction when it cannot tell.
- **Repo and group controls act on their members** — a repo toggle was decided by repo-exclusion history and a group toggle by a check that could never fail, so a group exclude returned success and recorded nothing, and a repo switch could post the direction the server would refuse. Both now read the set of packages the operation actually changes, and an operation that would change nothing is refused with a message saying so.
- **Uploaded RPMs keep their real filename and are proven before export** — an RPM whose filename did not match the host's exact version was matched, marked, and then silently left out of the build, so the build failed on a missing `COPY` source. Uploads are now staged under their own name through one allowlist, streamed to disk in bounded chunks rather than buffered whole, placed and recorded under one lock, and verified as a regular file inside the staging directory before the export copies them. A staged file that cannot be verified demotes its package with an instruction instead of failing the export.

### Removed
- **Dead `/var` fields and phantom kernel decisions** — `size_estimate` and `recommendation` were written by nothing and rendered as placeholder text on every `/var` row. Loaded kernel modules carried 66 include decisions no surface read and no renderer honoured; they are inventory now. `non_default_modules` and `repo_providing_packages` were never populated.

### Security
- **Uploaded and cached RPM filenames are allowlisted at every boundary** — a filename reaches `COPY`, a shell-form `RUN`, an `rm` operand, and a comment in the generated build, and the upload handler previously stripped a bad name rather than rejecting it. Names are now validated once against one ASCII allowlist at the handler, at session resume, at export, and on the pre-existing dnf-cache path, and a name outside it is rejected. The export writes each uploaded package's file itself, never through a path a crafted source tarball could have planted.

### Breaking
- **Snapshot schema 23** — removed fields and changed types make schema 22 snapshots unloadable. Re-scan; aggregates re-aggregate from re-scanned hosts.
```

- [ ] **Step 2: Walk the § 4 consumer checklist**

Open § 4's consumer table and the task map at the top of this plan, and confirm each row's "Phase 0" column is what the code does. Record any row the implementation diverged from, with the reason, under a new "Divergences" heading in this document.

- [ ] **Step 3: Run the full suite and the gates one last time**

Run:
```bash
cargo fmt --check && cargo test --workspace
cargo clippy --workspace -- -W clippy::all          # what the hook and CI run
cargo clippy --workspace --all-targets -- -D clippy::all
cd crates/web/ui && npm test && npx tsc --noEmit && npm run test:e2e
```
Expected: `fmt`, `test`, and the three frontend commands PASS. The hook/CI clippy reports **0 warnings**. The `--all-targets` run reports **nothing beyond the 9-warning pre-existing test-code baseline** recorded in Global Constraints; anything else is a phase 0 regression and is fixed before merge. Prepend the shim directory to `PATH` first, per the toolchain constraint.

- [ ] **Step 4: Confirm both merge gates read PASS**

Tasks 1 and 2. If either does not, phase 0 does not merge.

- [ ] **Step 5: Commit**

```bash
git add CHANGELOG.md process-docs/plans/2026-10-27-refine-ui-fixup-phase-0.md
git commit -m "docs(changelog): record the phase 0 refine fix-up changes"
```

---

## Coverage Map

Checkable against § 7's phase 0 row and § 8. Every task cites its spec section and its § 8 test in its own heading; this is the same information inverted.

### Spec section to task

| Spec section | Tasks |
|---|---|
| § 3.1 fcontext rules | 10 (types, collector), 11 (renderer, helper, reports) |
| § 3.2 repo attribution and `RepoIndex` | 4 (types, normalization), 5 (`RepoIndex`, provenance, `validate_target`), 7 (effective set, counts, no-op), 8 and 9 (the frontend half) |
| § 3.3 repo-enabling packages and the upload path | 12 (filename), 13 (handler), 14 (export), 15 (removal, rescan), 16 (classification), 17 (render, CRB) |
| § 3.4 tmpfiles.d parser and `/var` backing | 2 (gate), 18 (parser, payloads), 19 (effective backing) |
| § 3.5 Kernel & Boot and the one-decision rule | 20 (walker, tuned), 21 (shadowing, field removals, validation, projection), 22 (configtree) |
| § 3.6 PAM and audit rules | 20 |
| § 3.7 groups and sections (phase 0 half) | 6. The `SectionId` / `ItemId::Section` half is phase 2. |
| § 3.8 `ContextItem` DTO | 23 |
| § 3.9 one state behind every surface | 24 |
| § 4 the single schema bump | 3 (constants, fixtures); the consumer checklist is distributed by the task map above and walked in 26 |
| § 5.1 SELinux block | 11 |
| § 5.2 repo-enabling block | 17 |
| § 5.3 configtree | 22 |
| § 5.4 tmpfiles.d emission | 19 |
| § 5.5 audit report | 11 (fcontext), 17 (packages), 19 (storage), 20 (PAM/audit) |
| § 5.6 FIXME wording rule | 11 (fcontext and the GPG key path), 17, 19, 21 — applied where this plan already touches a line, left alone elsewhere |
| § 6.3 phase 0 rows (predicate, state text, `RepoBar` rule) | 8, 9 |
| § 6.9 handler changes (all phase 0 unless named otherwise) | 7 (`/api/view` counts, `/api/op` repo), 13 and 15 (`/api/upload-rpm`), 14 (`/api/tarball` warnings), 21 (`/api/op` Config inventory, `KernelModule` gone, the batch-toggle kernel arm), 23 and 24 (`/api/snapshot/sections`) |
| § 9 skill and CHANGELOG updates | 25, 26 |

### § 8 test to task

| § 8 entry | Task | Which part, where the entry mixes phases |
|---|---|---|
| 8.1 configtree single source | 22 | all of it |
| 8.2 upload lifecycle through the export | 12 (the `validate_staged_name` table), 13 (the `api_test.rs` handler block, 413, disconnect, startup sweep, overlap through `stage`/`commit`), 14 (exact, differing version, Fedora filename, placement failure, replace, 409, render safety, export confinement, output ownership, dnf-cache path), 15 (removal, removal confinement, rescan) | all phase 0 |
| 8.3 repo-enabling classification and block | 16 (`repoless_rpm_test.rs`), 17 (`containerfile.rs` units, two-repository export, CRB) | all phase 0 |
| 8.4 `normalize_source_repos` | 4 | all of it |
| 8.5 `RepoIndex` collapses case | 5 | all of it |
| 8.6 `RpmFilename` table | 12 (the module table), 13 (the `api_test.rs` 200 case) | all phase 0 |
| 8.7 tmpfiles classifier and effective backing | 18 (the `storage.rs` unit cases), 19 (`var_backing_export_test.rs`) | all phase 0 |
| 8.8 group and repo no-op | 6 (the group cases and `GroupInfo`), 7 (the repo regression, both disagreement states, hidden member, `RepoGroupInfo`, `api_test.rs`, `error.rs`) | **phase 0 only**; the entry's final "Phase 2, same file" block (`SetInclude(Section(..))`) is not planned here |
| 8.9 fcontext collector, parser table, SELinux block | 10 (the collector and parser table, source selection), 11 (`smoke_render.rs` and `selinux_block_shell_test.rs`) | all phase 0 |
| 8.10 Kernel & Boot after the move | 3 (the schema-22 rejection), 21 (everything else) | all phase 0 |
| 8.11 PAM and audit through the config tree | 20 | all of it |
| 8.12 decision rows, containers, and the invariant | 23 (the Rust `contract_snapshots.rs` block and `plain_context_item_is_byte_identical`), 7 (the `api_test.rs` group 422 and incomplete-repo 200), 8 (the `disabled`/`stale`/zero-effective frontend fixtures), 9 (the phase 0 activation block through both live `RepoBar` callers, including the stateful fake and the `repo-disagree` sequences) | **phase 0 only**; everything the entry labels "Frontend, phase 1" (`ListSemantics.test.tsx`, `ContextItem`/`ContextList`, `GroupRow`, `RepoGroup` phase 1 cases, `PackageList`, `UnmanagedFileList`, `DecisionSections`, `Sidebar`, `e2e/triage.spec.ts`, `e2e/keyboard.spec.ts`) and "Phase 2" is not planned here |
| 8.13 error toast and upload modal | 13 (the one Rust line: `POST /api/upload-rpm` with `epel-release-latest-9.noarch.RPM` returns 200) | **phase 0 only**; the whole frontend body of the entry is phase 1 |
| 8.14 host checks | 1 (B1), 2 (B2) | all of it; A1 to A3 already passed on 2026-10-26 and are cited inside Tasks 4, 11, and 18 |
| 8.15 every decision surface reflects the export | 24 | all of it |
| 8.16 attention strip targets | — | phase 1, not planned here |
| 8.17 narrow layout | — | phase 1, not planned here |

---

## Declared Narrowings and Deferrals

Each of these is a decision, not an omission. The plan review treats a silent narrowing as a finding and a declared one as a decision, so they are listed here rather than left in a task.

1. **`ItemId::Section` and `SectionId` are not added.** § 3.7 defines them and § 6.4 phases them, and § 7 puts them in phase 2 ("one Rust op"). Task 6 therefore does not add the `ItemId::Section { section: Packages }` arm to the group evaluator's `'group_scan` loop that § 3.7 describes, and the § 8.8 cases the spec labels "Phase 2, same file" are not planned. The group evaluator's effective-set change, which those cases also depend on, does land in Task 6.
2. **The `tuned/` promoted root and `TUNED_PREFIX` are deleted in Task 20, not Task 21.** § 3.5 groups them with the tuned move and § 7 lists them in phase 0's deleted column. Task 20 owns the walker and the configtree blocks; splitting them across two tasks would leave the workspace uncompilable between them.
3. **The § 4 consumer checklist is distributed across the tasks that force each site, not held in one task.** Every row names a field removal or type change, and the compiler will not let the owning task commit without it. The checklist lands coherently through the task map at the top of this plan and Task 26's walk, which is the acceptance.
4. **B1's sub-cases run through `sh -s` on stdin rather than `sh -c '...'`.** The emitted commands contain single quotes, which cannot nest inside `sh -c '...'`. The image, the commands, and the expectations are the spec's; only the way the script reaches the shell differs, and it differs so the bytes under test are the bytes the renderer writes.
5. **B1 gains a step 1 the spec does not list:** confirming `semanage` and `matchpathcon` exist in `registry.redhat.io/rhel9/rhel-bootc:9.8`. § 3.1 assumes `semanage` is present in the target base and the spec never checks it. The step carries its own re-open condition rather than a workaround, because inventing an install precondition would be a redesign.
6. **Peak resident memory during an upload is not asserted.** § 8.2 says so: there is no portable measurement. The chunk size is a named constant and the bounded-memory claim rests on the `field.chunk()` loop being the only read, which review of the implementation confirms.
7. **Three adjacent items are out of scope and no task touches them:** the pre-existing unmanaged-extraction path-traversal issue, the deferred sidebar sections reorganization, and everything in § 9. In particular, `rpm.gpg_keys` is still nine copies of one path on the reference host (§ 5.2 notices it and § 9 excludes it), and the "GPG Keys (9)" count stays as it is.

---

## Self-Review

Run against the spec with fresh eyes after the plan was written.

**1. Spec coverage.** Every § 3 subsection, every § 5 subsection, § 4, the phase 0 rows of § 6.3 and § 6.9, and § 9's last bullet map to a task in the Coverage Map above. The only § 3 content without a task is § 3.7's `SectionId` / `ItemId::Section` half, which § 7 places in phase 2; it is declared in Deferrals item 1.

**2. Step scan.** Each step names one action with a checkable result. Code blocks appear only where the spec fixes exact bytes (the two emitted `RUN` forms, the FIXME texts, the CHANGELOG entries) or where a test's assertions are the decision. Function bodies are not written; signatures, files, and the values the spec pins are.

**3. Type consistency.** `RepoId` (Task 4) is the key type in Tasks 5, 7, 16, 17. `RefineError::NoopTarget` (Task 6) is the error Task 7 returns. `validate_staged_name` and `RpmFilename` (Task 12) are consumed under those exact names by Tasks 13, 14, 15, 17. `VarDirBacking` (Task 18) is the input to `effective_var_backing` (Task 19). `ContextDecision` and `ContextAdvisory` (Task 23) are what Task 24's surfaces must keep in step with the export. `UploadMatch`/`UploadMatchKind` (Task 13) feed `VerifiedUpload` (Task 14). No name is introduced in a later task that an earlier one did not define.

**4. Review Focus.** Five input classes the spec implies and § 8 does not pin, each with a test in the owning task: `RepoId` case folding (Task 4), `parse_repo_sections` on dnf's `enabled=` vocabulary (Task 4), the no-op arms with no `rpm` section (Task 7), `effective_var_backing` with absent sections (Task 19), and the `validate_staged_name` length boundaries (Task 12).

**5. Proportion.** The plan is shorter than the spec it implements. Most of its bulk is the two merge gates' runnable shell and the two emitted `RUN` forms, both of which are exact bytes the implementer cannot derive.
