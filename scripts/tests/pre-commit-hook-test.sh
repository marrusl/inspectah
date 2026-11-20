#!/usr/bin/env bash
# Tests for .githooks/pre-commit.
#
# Each case builds a fresh throwaway repository seeded with the real hook,
# the docs-only list and the sensitive-tarball check, then commits into it
# with a stub cargo first on PATH. The stub records every call, so a case
# can assert which gates ran, in which order, and with what environment.
# Run from anywhere: scripts/tests/pre-commit-hook-test.sh
# Written for bash 3.2 (macOS /bin/bash): no mapfile, no associative arrays.
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "$0")/../.." && pwd)
HOOK="$REPO_ROOT/.githooks/pre-commit"
DOCS_ONLY="$REPO_ROOT/.githooks/docs-only-paths"
TARBALL_CHECK="$REPO_ROOT/scripts/check-sensitive-tarballs.sh"

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
# A passing commit prints three gate lines and the closing line; the bound
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
    mkdir -p "$R/.githooks" "$R/scripts" "$R/crates/a/src" "$R/crates/a/tests" "$R/docs"
    cp "$HOOK" "$R/.githooks/pre-commit"
    cp "$DOCS_ONLY" "$R/.githooks/docs-only-paths"
    cp "$TARBALL_CHECK" "$R/scripts/check-sensitive-tarballs.sh"
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
    want_accepted || return 1
    [ "$(wc -l < "$STUB_LOG" | tr -d ' ')" -eq 3 ] || { report "stub log is not three lines"; return 1; }
    case "$(sed -n 1p "$STUB_LOG")" in "fmt --check "*) ;; *) report "line 1 is not fmt --check"; return 1 ;; esac
    case "$(sed -n 2p "$STUB_LOG")" in "clippy --workspace --all-targets -- -D clippy::all "*) ;; *) report "line 2 is not the clippy gate"; return 1 ;; esac
    case "$(sed -n 3p "$STUB_LOG")" in "test --workspace "*) ;; *) report "line 3 is not test --workspace"; return 1 ;; esac
}
check "a code commit runs fmt, clippy and test in order" case_order

case_quiet() {
    new_repo; edit crates/a/src/lib.rs; stage crates/a/src/lib.rs
    commit_env STUB_NOISE=1
    want_accepted && has "pre-commit: fmt ok (" && has "pre-commit: clippy ok (" \
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
    has "pre-commit: clippy FAILED (" && has "stub clippy failed" && stub_lacks test
}
check "a failing gate prints its output" case_gate_fails

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
    commit; stub_has fmt
}
check "a snapshot-only commit is gated" case_snapshot

case_ui() {
    new_repo; edit crates/web/ui/src/App.tsx; stage crates/web/ui/src/App.tsx
    commit; stub_has fmt
}
check "a UI-only commit is gated" case_ui

case_deletion() {
    new_repo; run git rm -q crates/a/tests/t.rs; commit; stub_has fmt
}
check "a deletion-only code commit is gated" case_deletion

case_rename() {
    new_repo; run git mv crates/a/src/lib.rs docs/lib.rs; commit; stub_has fmt
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
    run git commit -q --no-verify -m "test: code"
    run .githooks/pre-commit --head
    [ "$RC" -eq 0 ] || { report "exit $RC, want 0"; return 1; }
    stub_has fmt
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

echo
echo "$passes passed, $failures failed"
[ "$failures" -eq 0 ]
