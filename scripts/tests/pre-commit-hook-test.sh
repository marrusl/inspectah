#!/usr/bin/env bash
# Tests for .githooks/pre-commit.
#
# Each case builds a fresh throwaway repository seeded with the real hook,
# the docs-only list and the sensitive-tarball check, then commits into it
# with a stub cargo first on PATH and a stub in place of
# scripts/tests/docs-only-paths-test.sh. The stubs record every call, so a
# case can assert which gates ran, in which order, and with what environment.
# Run from anywhere: scripts/tests/pre-commit-hook-test.sh
# Written for bash 3.2 (macOS /bin/bash): no mapfile, no associative arrays.
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "$0")/../.." && pwd)
HOOK="$REPO_ROOT/.githooks/pre-commit"
DOCS_ONLY="$REPO_ROOT/.githooks/docs-only-paths"
TARBALL_CHECK="$REPO_ROOT/scripts/check-sensitive-tarballs.sh"
DOCS_CHECK="$REPO_ROOT/scripts/tests/docs-only-paths-test.sh"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

failures=0
passes=0

fail() {
    echo "FAIL: $*" >&2
    failures=$((failures + 1))
}

ok() {
    echo "ok: $*"
    passes=$((passes + 1))
}

REFUSED='pre-commit: commit refused:'
DOCS_ONLY_LINE='pre-commit: docs-only, cargo gates skipped'
# A passing commit prints four gate lines and the closing line; the bound
# leaves room for git's own output without letting gate output through.
QUIET_OUTPUT_MAX_LINES=10
STUB_NOISE_LINES=500

# --- stub cargo ---
#
# Appends "<args> INSPECTAH_SKIP_UI=<value or unset>" to $STUB_LOG, then:
#   STUB_FAIL=<subcommand>          exit 1 for that subcommand after printing
#                                   "stub <subcommand> failed"
#   STUB_NOISE=1                    print many lines
#   STUB_DURING_TEST=<mode>:<path>  during `test`: untracked creates the
#                                   file, add creates and stages it, edit
#                                   appends a line to a tracked file
STUB_BIN="$WORK/stub-bin"
mkdir -p "$STUB_BIN"
cat > "$STUB_BIN/cargo" <<EOF
#!/usr/bin/env bash
set -eu
echo "\$* INSPECTAH_SKIP_UI=\${INSPECTAH_SKIP_UI-unset}" >> "\$STUB_LOG"
if [ "\${STUB_NOISE-}" = 1 ]; then
    i=0
    while [ "\$i" -lt $STUB_NOISE_LINES ]; do
        echo "stub noise line \$i"
        i=\$((i + 1))
    done
fi
if [ "\$1" = test ] && [ -n "\${STUB_DURING_TEST-}" ]; then
    mode=\${STUB_DURING_TEST%%:*}
    path=\${STUB_DURING_TEST#*:}
    case "\$mode" in
        untracked) mkdir -p "\$(dirname "\$path")"; echo "// late" > "\$path" ;;
        add) mkdir -p "\$(dirname "\$path")"; echo "// late" > "\$path"; git add -- "\$path" ;;
        edit) echo "// edited during test" >> "\$path" ;;
        *) echo "stub: unknown STUB_DURING_TEST mode \$mode" >&2; exit 99 ;;
    esac
fi
if [ "\${STUB_FAIL-}" = "\$1" ]; then
    echo "stub \$1 failed"
    exit 1
fi
exit 0
EOF
chmod +x "$STUB_BIN/cargo"

# --- stub docs-only read check ---
#
# Seeded at scripts/tests/docs-only-paths-test.sh in each repository.
# Appends "docs-only-paths-test INSPECTAH_SKIP_UI=<value or unset>" to
# $STUB_LOG; with STUB_FAIL=docs-only-paths it prints
# "stub docs-only-paths failed" and exits 1.
STUB_DOCS_CHECK="$WORK/docs-only-paths-test.sh"
cat > "$STUB_DOCS_CHECK" <<'STUB'
#!/usr/bin/env bash
set -eu
echo "docs-only-paths-test INSPECTAH_SKIP_UI=${INSPECTAH_SKIP_UI-unset}" >> "$STUB_LOG"
if [ "${STUB_FAIL-}" = docs-only-paths ]; then
    echo "stub docs-only-paths failed"
    exit 1
fi
exit 0
STUB
chmod +x "$STUB_DOCS_CHECK"
PATH="$STUB_BIN:$PATH"
export PATH

# --- throwaway repositories ---

case_no=0
R=
STUB_LOG=
export STUB_LOG

# new_repo: a fresh repository in $R, seeded and committed past the hook,
# with an empty stub log in $STUB_LOG.
new_repo() {
    case_no=$((case_no + 1))
    R="$WORK/repo-$case_no"
    STUB_LOG="$WORK/stub-$case_no.log"
    : > "$STUB_LOG"
    git init -q "$R"
    git -C "$R" config user.name "Test"
    git -C "$R" config user.email "test@example.invalid"
    git -C "$R" config commit.gpgsign false
    git -C "$R" config core.hooksPath .githooks
    mkdir -p "$R/.githooks" "$R/scripts/tests" "$R/crates/a/src" "$R/crates/a/tests" "$R/docs"
    cp "$HOOK" "$R/.githooks/pre-commit"
    cp "$DOCS_ONLY" "$R/.githooks/docs-only-paths"
    cp "$TARBALL_CHECK" "$R/scripts/check-sensitive-tarballs.sh"
    cp "$STUB_DOCS_CHECK" "$R/scripts/tests/docs-only-paths-test.sh"
    echo "pub fn a() {}" > "$R/crates/a/src/lib.rs"
    echo "#[test] fn t() {}" > "$R/crates/a/tests/t.rs"
    echo "# Guide" > "$R/docs/guide.md"
    echo "# Readme" > "$R/README.md"
    git -C "$R" add -A
    git -C "$R" commit -q --no-verify -m "chore: seed"
}

# edit <path>: append a line to a file in $R, creating it if needed.
edit() {
    mkdir -p "$(dirname "$R/$1")"
    echo "// change $RANDOM" >> "$R/$1"
}

stage() {
    git -C "$R" add -- "$@"
}

OUT=
RC=0
# run <command...>: run in $R, capturing combined output in $OUT and the
# exit status in $RC.
run() {
    RC=0
    OUT=$(cd "$R" && "$@" 2>&1) || RC=$?
}

commit() {
    run git commit -q -m "test: commit" "$@"
}

# commit_env <VAR=value...>: commit with extra environment.
commit_env() {
    run env "$@" git commit -q -m "test: commit"
}

# Assertions. Each returns non-zero on the first failed check, after
# recording the failure with the case's output.
CASE=
report() {
    fail "$CASE: $*"
    echo "--- output (exit $RC) ---" >&2
    printf '%s\n' "$OUT" >&2
    echo "--- stub log ---" >&2
    cat "$STUB_LOG" >&2
    return 1
}
want_accepted() { [ "$RC" -eq 0 ] || report "refused, want accepted"; }
want_refused() {
    [ "$RC" -ne 0 ] || { report "accepted, want refused"; return 1; }
    has "$REFUSED"
}
has() {
    case "$OUT" in
        *"$1"*) ;;
        *) report "output lacks '$1'" ;;
    esac
}
lacks() {
    case "$OUT" in
        *"$1"*) report "output has '$1'" ;;
    esac
}
stub_empty() { [ ! -s "$STUB_LOG" ] || report "stub log is not empty"; }
stub_has() {
    grep -qE -- "^$1( |\$)" "$STUB_LOG" || report "stub log has no '$1' line"
}
stub_lacks() {
    if grep -qE -- "^$1( |\$)" "$STUB_LOG"; then report "stub log has a '$1' line"; fi
}
# want_gated: accepted, and the stub log is exactly the four gates, run
# once each in order. A hook that stops after fmt, or refuses after the
# gates, fails it.
want_gated() {
    want_accepted || return 1
    local lines
    lines=$(wc -l < "$STUB_LOG" | tr -d ' ')
    [ "$lines" -eq 4 ] || { report "stub log is $lines lines, want 4"; return 1; }
    case "$(sed -n 1p "$STUB_LOG")" in "docs-only-paths-test "*) ;; *) report "line 1 is not the docs-only read check"; return 1 ;; esac
    case "$(sed -n 2p "$STUB_LOG")" in "fmt --check "*) ;; *) report "line 2 is not fmt --check"; return 1 ;; esac
    case "$(sed -n 3p "$STUB_LOG")" in "clippy --workspace --all-targets -- -D clippy::all "*) ;; *) report "line 3 is not the clippy gate"; return 1 ;; esac
    case "$(sed -n 4p "$STUB_LOG")" in "test --workspace "*) ;; *) report "line 4 is not test --workspace"; return 1 ;; esac
}
# want_exit <n>: the hook exited with status n.
want_exit() { [ "$RC" -eq "$1" ] || report "exit $RC, want $1"; }

# commit_no_verify <message>: commit in $R without running the hook.
commit_no_verify() {
    git -C "$R" commit -q --no-verify -m "$1"
}

# check <name> <function>: run one case; ok if the function returns 0.
check() {
    CASE=$1
    if "$2"; then ok "$1"; fi
}

# --- cases ---

case_docs_only() {
    new_repo; edit docs/guide.md; stage docs/guide.md; commit
    want_accepted && stub_empty && has "$DOCS_ONLY_LINE"
}
check "docs-only commit skips the cargo gates" case_docs_only

case_docs_dirty_code() {
    new_repo; edit docs/guide.md; stage docs/guide.md
    edit crates/a/src/lib.rs; edit crates/a/src/new.rs
    commit
    want_accepted && has "$DOCS_ONLY_LINE" && stub_empty
}
check "a docs-only commit is not blocked by dirty code" case_docs_dirty_code

case_empty() {
    new_repo; commit --allow-empty
    want_accepted && has "$DOCS_ONLY_LINE" && stub_empty
}
check "an empty commit is docs-only" case_empty

case_order() {
    new_repo; edit crates/a/src/lib.rs; stage crates/a/src/lib.rs; commit
    want_gated
}
check "a code commit runs the docs-only read check, fmt, clippy and test in order" case_order

case_quiet() {
    new_repo; edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    commit_env STUB_NOISE=1
    want_accepted && has "pre-commit: docs-only-paths ok (" \
        && has "pre-commit: fmt ok (" && has "pre-commit: clippy ok (" \
        && has "pre-commit: test ok (" && lacks "stub noise" || return 1
    local lines
    lines=$(printf '%s\n' "$OUT" | wc -l | tr -d ' ')
    [ "$lines" -lt "$QUIET_OUTPUT_MAX_LINES" ] || report "output is $lines lines"
}
check "a passing commit prints one line per gate" case_quiet

case_gate_fails() {
    new_repo; edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    commit_env STUB_FAIL=clippy
    [ "$RC" -eq 1 ] || { report "exit $RC, want 1"; return 1; }
    has "pre-commit: clippy FAILED (" && has "stub clippy failed" \
        && has "clippy warnings found" && stub_lacks test
}
check "a failing gate prints its output" case_gate_fails

case_docs_check_fails() {
    new_repo; edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    commit_env STUB_FAIL=docs-only-paths
    [ "$RC" -eq 1 ] || { report "exit $RC, want 1"; return 1; }
    has "pre-commit: docs-only-paths FAILED (" && has "stub docs-only-paths failed" \
        && has "a build input reads a docs-only path" && stub_lacks fmt
}
check "a failing docs-only read check prints its output and stops before fmt" case_docs_check_fails

# In a linked worktree git runs the hook with GIT_DIR set. The real check
# makes throwaway repositories of its own, and once committed its seed
# commits onto the worktree's branch and its git init set core.bare in the
# shared config. Here it fails (its allowlist names files this repository
# lacks), but must leave the repository as it found it.
case_docs_check_leaves_worktree_alone() {
    new_repo
    local wt="$WORK/wt-$case_no" before after
    git -C "$R" worktree add -q -b wt "$wt"
    cp "$DOCS_CHECK" "$wt/scripts/tests/docs-only-paths-test.sh"
    git -C "$wt" add scripts/tests/docs-only-paths-test.sh
    git -C "$wt" commit -q --no-verify -m "test: real docs-only read check"
    before=$(git -C "$R" rev-parse wt)
    echo "// change $RANDOM" >> "$wt/crates/a/src/lib.rs"
    git -C "$wt" add crates/a/src/lib.rs
    RC=0
    OUT=$(cd "$wt" && git commit -q -m "test: commit" 2>&1) || RC=$?
    has "pre-commit: docs-only-paths FAILED (" || return 1
    after=$(git -C "$R" rev-parse wt)
    [ "$after" = "$before" ] || { report "the worktree's branch moved"; return 1; }
    [ "$(git -C "$R" config --bool core.bare)" = false ] || report "core.bare changed"
}
check "the real docs-only read check leaves a worktree's repository alone" case_docs_check_leaves_worktree_alone

case_skip_ui() {
    new_repo; edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    commit_env INSPECTAH_SKIP_UI=1
    want_accepted || return 1
    [ -s "$STUB_LOG" ] || { report "stub log is empty"; return 1; }
    if grep -vq ' INSPECTAH_SKIP_UI=unset$' "$STUB_LOG"; then
        report "a stub call saw INSPECTAH_SKIP_UI set"
    fi
}
check "the hook unsets INSPECTAH_SKIP_UI" case_skip_ui

case_snapshot() {
    new_repo; edit crates/a/tests/snapshots/x.snap; stage crates/a/tests/snapshots/x.snap
    commit; want_gated
}
check "a snapshot-only commit is gated" case_snapshot

case_ui() {
    new_repo; edit crates/web/ui/src/App.tsx; stage crates/web/ui/src/App.tsx
    commit; want_gated
}
check "a UI-only commit is gated" case_ui

case_deletion() {
    new_repo; run git rm -q crates/a/tests/t.rs; commit; want_gated
}
check "a deletion-only code commit is gated" case_deletion

case_rename() {
    new_repo; run git mv crates/a/src/lib.rs docs/lib.rs; commit; want_gated
}
check "a rename out of a code path is gated" case_rename

case_unstaged_code() {
    new_repo; edit docs/guide.md; edit crates/a/src/lib.rs
    stage docs/guide.md crates/a/src/lib.rs; edit crates/a/src/lib.rs
    commit
    want_refused && has crates/a/src/lib.rs && stub_empty
}
check "an unstaged code edit is refused before fmt" case_unstaged_code

case_untracked_code() {
    new_repo; edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    edit crates/a/tests/new.rs
    commit
    want_refused && has crates/a/tests/new.rs && stub_empty
}
check "an untracked code path is refused before fmt" case_untracked_code

case_docs_dirt_ok() {
    new_repo; edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    edit README.md; edit docs/new.md
    commit
    want_accepted
}
check "an unstaged docs edit and an untracked docs file do not block" case_docs_dirt_ok

case_late_untracked() {
    new_repo; edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    commit_env STUB_DURING_TEST=untracked:crates/a/late.rs
    want_refused && has crates/a/late.rs && stub_has test
}
check "an untracked file created during the test gate is refused after it" case_late_untracked

case_late_edit() {
    new_repo; edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    commit_env STUB_DURING_TEST=edit:crates/a/tests/t.rs
    want_refused && has crates/a/tests/t.rs && stub_has test
}
check "a tracked code file edited during the test gate is refused after it" case_late_edit

case_late_add() {
    new_repo; edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    commit_env STUB_DURING_TEST=add:crates/a/late.rs
    want_refused && has "the index changed while the gate ran"
}
check "the index changing during the gate is refused" case_late_add

# The --only pair proves the hook reads the index being committed
# (GIT_INDEX_FILE), not the real one. A hook reading the real index runs
# the gates in (a) and, in (b), names the committed file instead.
case_only_docs() {
    new_repo; edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    edit docs/guide.md
    commit --only docs/guide.md
    want_accepted && has "$DOCS_ONLY_LINE" && stub_empty || return 1
    local staged
    staged=$(git -C "$R" diff --cached --name-only)
    case "$staged" in
        *crates/a/src/lib.rs*) ;;
        *) report "crates/a/src/lib.rs is no longer staged" ;;
    esac
}
case_only_code() {
    new_repo; edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    edit crates/a/tests/t.rs
    commit --only crates/a/tests/t.rs
    want_refused && has crates/a/src/lib.rs && lacks crates/a/tests/t.rs && stub_empty
}
check "--only refuses a code path staged in the real index but left out of the commit (a: docs-only)" case_only_docs
check "--only refuses a code path staged in the real index but left out of the commit (b: code)" case_only_code

case_all() {
    new_repo; edit crates/a/src/new.rs; edit crates/a/src/lib.rs
    commit -a
    want_refused && has crates/a/src/new.rs
}
check "-a refuses an untracked code path" case_all

case_missing_list() {
    new_repo; run git rm -q --cached .githooks/docs-only-paths
    edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    commit
    want_refused && has "cannot read .githooks/docs-only-paths from the index" && stub_empty
}
check "a missing docs-only list refuses" case_missing_list

case_unstaged_list() {
    new_repo; echo '^crates/' >> "$R/.githooks/docs-only-paths"
    edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    commit
    lacks "$DOCS_ONLY_LINE" && want_refused && has .githooks/docs-only-paths && stub_empty
}
check "an unstaged edit to the docs-only list is not applied" case_unstaged_list

case_bad_tarball() {
    new_repo; mkdir -p "$R/testdata"
    echo "this is not an archive" > "$R/testdata/bad.tar.gz"
    run git add -f testdata/bad.tar.gz
    run .githooks/pre-commit
    [ "$RC" -eq 1 ] || { report "exit $RC, want 1"; return 1; }
    has "cannot be read" && has "commit refused: sensitive-tarball check failed (exit 2)"
}
check "an unreadable staged tarball refuses with exit 1" case_bad_tarball

case_head_code() {
    new_repo; edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    commit_no_verify "test: code"
    run .githooks/pre-commit --head
    want_gated
}
check "head mode gates a code commit" case_head_code

case_head_docs() {
    new_repo; edit docs/guide.md; stage docs/guide.md
    run git commit -q --no-verify -m "test: docs"
    run .githooks/pre-commit --head
    [ "$RC" -eq 0 ] || { report "exit $RC, want 0"; return 1; }
    has "$DOCS_ONLY_LINE" && stub_empty
}
check "head mode skips a docs commit" case_head_docs

# head mode tests HEAD, so the index must not differ from it outside the
# docs-only list: a staged repair would be tested in HEAD's place.
case_head_staged() {
    new_repo; edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    commit_no_verify "test: code"
    edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    run .githooks/pre-commit --head
    want_refused && has "the index differs from HEAD" && has crates/a/src/lib.rs && stub_empty
}
check "head mode refuses a staged code change on top of HEAD before fmt" case_head_staged

case_head_staged_docs() {
    new_repo; edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    commit_no_verify "test: code"
    edit docs/guide.md; stage docs/guide.md
    run .githooks/pre-commit --head
    want_gated
}
check "head mode is not refused for a staged docs change" case_head_staged_docs

case_head_untracked() {
    new_repo; edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    commit_no_verify "test: code"
    edit crates/a/tests/new.rs
    run .githooks/pre-commit --head
    want_refused && has crates/a/tests/new.rs && stub_empty
}
check "head mode refuses an untracked code path before fmt" case_head_untracked

# merge_with <path on side> <path on main> <hand-resolved path or empty>:
# a merge commit in $R, made without the hook, whose two parents changed
# the first two paths and whose resolution also edits the third.
merge_with() {
    git -C "$R" checkout -q -b side
    edit "$1"; stage "$1"; commit_no_verify "test: side"
    git -C "$R" checkout -q -
    edit "$2"; stage "$2"; commit_no_verify "test: main"
    git -C "$R" merge -q --no-ff --no-commit side >/dev/null
    if [ -n "$3" ]; then edit "$3"; stage "$3"; fi
    commit_no_verify "test: merge"
}

case_head_merge() {
    new_repo; merge_with docs/side.md docs/guide.md crates/a/src/lib.rs
    run .githooks/pre-commit --head
    want_gated
}
check "head mode gates a merge whose resolution changes code" case_head_merge

case_head_merge_docs() {
    new_repo; merge_with docs/side.md docs/guide.md ""
    run .githooks/pre-commit --head
    want_accepted && has "$DOCS_ONLY_LINE" && stub_empty
}
check "head mode skips a docs-only merge" case_head_merge_docs

# Gate 0 runs as --tracked in head mode. Under --staged the index equals
# HEAD, so nothing would be checked and this tarball would pass.
case_head_tarball() {
    new_repo; mkdir -p "$R/testdata"
    echo "this is not an archive" > "$R/testdata/bad.tar.gz"
    run git add -f testdata/bad.tar.gz
    commit_no_verify "test: tarball"
    run .githooks/pre-commit --head
    want_exit 1 && has "commit refused: sensitive-tarball check failed (exit 2)" && stub_empty
}
check "head mode checks every tracked tarball" case_head_tarball

case_usage() {
    new_repo
    run .githooks/pre-commit --bogus
    want_exit 1 && has "pre-commit: usage: pre-commit [--head]" && stub_empty
}
check "an unknown argument is a usage error" case_usage

# A helper status other than 2 is still mapped to 1 and reported as is.
# A PATH holding only bash and git hides jq, so the helper exits 64.
case_gate0_usage() {
    new_repo; edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    local bin="$WORK/no-jq-$case_no"
    mkdir -p "$bin"
    ln -s "$(command -v bash)" "$bin/bash"
    ln -s "$(command -v git)" "$bin/git"
    run env PATH="$bin" .githooks/pre-commit
    want_exit 1 && has "requires jq" \
        && has "commit refused: sensitive-tarball check failed (exit 64)" && stub_empty
}
check "gate 0 exiting 64 refuses with exit 1" case_gate0_usage

# A git failure that set -e turns into an exit still exits 1: an unmerged
# docs path passes the refuse check, then git write-tree exits 128.
case_set_e_exit() {
    new_repo; merge_conflict_on docs/guide.md
    edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    run .githooks/pre-commit
    want_exit 1 && has "unmerged" && stub_empty
}
# merge_conflict_on <path>: leave $R mid-merge with <path> unmerged.
merge_conflict_on() {
    git -C "$R" checkout -q -b side
    echo "side" > "$R/$1"; stage "$1"; commit_no_verify "test: side"
    git -C "$R" checkout -q -
    echo "main" > "$R/$1"; stage "$1"; commit_no_verify "test: main"
    if git -C "$R" merge -q side >/dev/null 2>&1; then
        echo "merge_conflict_on: the merge did not conflict" >&2
        return 1
    fi
}
check "a status set -e raises is mapped to exit 1" case_set_e_exit

# --- ignored build inputs ---

# An ignored Rust source under crates/ is compiled by the gates but never
# committed. The repository's unanchored build/ rule makes this real.
case_ignored_source() {
    new_repo; printf 'build/\n' > "$R/.gitignore"; stage .gitignore
    echo "mod build;" >> "$R/crates/a/src/lib.rs"; stage crates/a/src/lib.rs
    mkdir -p "$R/crates/a/src/build"; echo "pub fn b() {}" > "$R/crates/a/src/build/mod.rs"
    commit
    want_refused && has crates/a/src/build/mod.rs && stub_empty
}
check "an ignored Rust source under crates/ is refused before fmt" case_ignored_source

case_ignored_manifest() {
    new_repo; printf 'build/\n' > "$R/.gitignore"; stage .gitignore
    edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    mkdir -p "$R/crates/build/b"; echo "[package]" > "$R/crates/build/b/Cargo.toml"
    commit
    want_refused && has crates/build/b/Cargo.toml && stub_empty
}
check "an ignored Cargo.toml under crates/ is refused before fmt" case_ignored_manifest

case_ignored_other() {
    new_repo; printf 'build/\n*.log\n' > "$R/.gitignore"; stage .gitignore
    edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    mkdir -p "$R/crates/a/build"; echo "out" > "$R/crates/a/build/out.txt"
    echo "log" > "$R/crates/a/run.log"
    commit
    want_gated
}
check "an ignored file outside the refused set does not block" case_ignored_other

# The second pass lists ignored inputs too: one written while the gates ran
# was compiled by them but is not in the commit.
case_late_ignored() {
    new_repo; printf 'build/\n' > "$R/.gitignore"; stage .gitignore
    edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    commit_env STUB_DURING_TEST=untracked:crates/a/src/build/late.rs
    want_refused && has "after the gates ran" && has crates/a/src/build/late.rs \
        && stub_has test
}
check "an ignored Rust source created during the test gate is refused after it" case_late_ignored

# --- path names git quotes ---

# git quotes a non-ASCII name unless core.quotePath is off, and a quoted
# docs path would fail the list.
case_unicode_docs_only() {
    new_repo; edit "docs/café.md"; stage "docs/café.md"; commit
    want_accepted && has "$DOCS_ONLY_LINE" && stub_empty
}
check "a docs-only commit of a non-ASCII docs path skips the cargo gates" case_unicode_docs_only

case_unicode_docs_dirt() {
    new_repo; edit "docs/naïve.md"; stage "docs/naïve.md"; commit_no_verify "test: seed"
    edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    edit "docs/naïve.md"; edit "docs/café.md"
    commit
    want_gated
}
check "an unstaged and an untracked non-ASCII docs path do not block" case_unicode_docs_dirt

case_unicode_code() {
    new_repo; edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    edit "crates/a/src/café.rs"
    commit
    want_refused && has "crates/a/src/café.rs" && stub_empty
}
check "an untracked non-ASCII code path is refused, named as is" case_unicode_code

# A name git must still quote (here a double quote) keeps its quotes and
# fails the list: gated, never exempt.
case_quoted_docs() {
    new_repo; edit 'docs/a"b.md'; stage 'docs/a"b.md'; commit
    want_gated
}
check "a docs path git must still quote is gated" case_quoted_docs

case_head_unicode_docs() {
    new_repo; edit "docs/café.md"; stage "docs/café.md"; commit_no_verify "test: docs"
    run .githooks/pre-commit --head
    want_accepted && has "$DOCS_ONLY_LINE" && stub_empty
}
check "head mode skips a commit of a non-ASCII docs path" case_head_unicode_docs

case_head_staged_unicode_docs() {
    new_repo; edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    commit_no_verify "test: code"
    edit "docs/café.md"; stage "docs/café.md"
    run .githooks/pre-commit --head
    want_gated
}
check "head mode is not refused for a staged non-ASCII docs path" case_head_staged_unicode_docs

echo
echo "$passes passed, $failures failed"
[ "$failures" -eq 0 ]
