#!/usr/bin/env bash
# Tests for scripts/check-sensitive-tarballs.sh.
#
# Builds small tarballs in a temp directory, drives the script against them,
# and runs the real pre-commit hook end to end in a throwaway repository.
# Run from anywhere: scripts/tests/check-sensitive-tarballs-test.sh
# Written for bash 3.2 (macOS /bin/bash): no mapfile, no associative arrays.
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "$0")/../.." && pwd)
SCRIPT="$REPO_ROOT/scripts/check-sensitive-tarballs.sh"
HOOK="$REPO_ROOT/.githooks/pre-commit"

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

# make_tarball <name> <top-level dir> <snapshot json or empty> [extra member...]
# Builds $WORK/<name>.tar.gz with the members under a top-level directory,
# the way the scan tarball writer lays them out.
make_tarball() {
    local name=$1 top=$2 snapshot=$3
    shift 3
    local src="$WORK/src-$name"
    mkdir -p "$src/$top"
    if [ -n "$snapshot" ]; then
        printf '%s\n' "$snapshot" > "$src/$top/inspection-snapshot.json"
    fi
    echo "FROM scratch" > "$src/$top/Containerfile"
    local member
    for member in "$@"; do
        mkdir -p "$(dirname "$src/$top/$member")"
        echo "content" > "$src/$top/$member"
    done
    tar -czf "$WORK/$name.tar.gz" -C "$src" "$top"
}

# expect <description> <expected exit> <expected output substring> <args...>
expect() {
    local desc=$1 want_rc=$2 want_text=$3
    shift 3
    local out rc=0
    out=$("$SCRIPT" "$@" 2>&1) || rc=$?
    if [ "$rc" -ne "$want_rc" ]; then
        fail "$desc: exit $rc, want $want_rc. Output:"
        printf '%s\n' "$out" >&2
        return
    fi
    if [ -n "$want_text" ] && ! printf '%s' "$out" | grep -qF -- "$want_text"; then
        fail "$desc: output lacks '$want_text'. Output:"
        printf '%s\n' "$out" >&2
        return
    fi
    ok "$desc"
}

CLEAN_SNAPSHOT='{"schema_version":25,"redaction_state":{"state":"fully_redacted","redacted_by":"t","config_hash":"h"}}'
RETAINED_SNAPSHOT='{"schema_version":25,"redaction_state":{"state":"sensitive_retained","redacted_by":"t","config_hash":"h","unresolved_count":0}}'
# An aggregate snapshot: the merge drops redaction_state and keeps the flag.
FLAGGED_SNAPSHOT='{"schema_version":25,"sensitive_snapshot":true,"preserved_subscription":true}'

make_tarball retained host-a "$RETAINED_SNAPSHOT"
make_tarball flagged fleet "$FLAGGED_SNAPSHOT"
make_tarball entitlement host-b "$CLEAN_SNAPSHOT" subscription/entitlement/123-key.pem
make_tarball entitlement-no-snapshot stray "" subscription/entitlement/123.pem
make_tarball clean host-c "$CLEAN_SNAPSHOT" subscription/rhsm/rhsm.conf
make_tarball unrelated not-inspectah "" docs/readme.txt
make_tarball badjson host-d '{"schema_version": 25,'
printf 'this is not a gzip stream\n' > "$WORK/corrupt.tar.gz"

# --- file mode ---

expect "sensitive_retained snapshot is rejected" 1 "sensitive_retained" "$WORK/retained.tar.gz"
expect "sensitive_snapshot flag is rejected" 1 "sensitive_snapshot" "$WORK/flagged.tar.gz"
expect "entitlement member is rejected" 1 "subscription/entitlement/" "$WORK/entitlement.tar.gz"
expect "entitlement member without a snapshot is rejected" 1 "subscription/entitlement/" \
    "$WORK/entitlement-no-snapshot.tar.gz"
expect "rejection names the --preserve remedy" 1 "without --preserve" "$WORK/retained.tar.gz"
expect "clean inspectah tarball passes" 0 "" "$WORK/clean.tar.gz"
expect "unrelated tarball passes" 0 "" "$WORK/unrelated.tar.gz"
expect "corrupt tarball fails closed" 2 "cannot be read" "$WORK/corrupt.tar.gz"
expect "unparseable snapshot fails closed" 2 "cannot be read" "$WORK/badjson.tar.gz"
expect "missing file fails closed" 2 "cannot be read" "$WORK/does-not-exist.tar.gz"
expect "a rejection among clean files still fails" 1 "retained.tar.gz" \
    "$WORK/clean.tar.gz" "$WORK/retained.tar.gz" "$WORK/unrelated.tar.gz"
expect "no arguments is a usage error" 64 "usage"

# --- hook end to end, in a throwaway repository ---

REPO="$WORK/repo"
git init -q "$REPO"
mkdir -p "$REPO/.githooks" "$REPO/scripts"
cp "$HOOK" "$REPO/.githooks/pre-commit"
cp "$SCRIPT" "$REPO/scripts/check-sensitive-tarballs.sh"
git -C "$REPO" config core.hooksPath .githooks
git -C "$REPO" config user.name "Test"
git -C "$REPO" config user.email "test@example.invalid"
git -C "$REPO" config commit.gpgsign false
git -C "$REPO" add .githooks scripts
git -C "$REPO" commit -q --no-verify -m "chore: seed"

GATE_BANNER="BLOCKED: inspectah output with preserved sensitive content"

# commit_expect <description> <want: refused|accepted> <output substring>
commit_expect() {
    local desc=$1 want=$2 want_text=$3 out rc=0
    out=$(git -C "$REPO" commit -q -m "test: $desc" 2>&1) || rc=$?
    if [ "$want" = refused ] && [ "$rc" -eq 0 ]; then
        fail "hook: $desc: commit was accepted"
        return
    fi
    # A refusal for any other reason (nothing staged, another gate) must not
    # count: require the gate's own banner.
    if [ "$want" = refused ] && ! printf '%s' "$out" | grep -qF -- "$GATE_BANNER"; then
        fail "hook: $desc: refused, but not by this gate. Output:"
        printf '%s\n' "$out" >&2
        return
    fi
    if [ "$want" = accepted ] && [ "$rc" -ne 0 ]; then
        fail "hook: $desc: commit was refused. Output:"
        printf '%s\n' "$out" >&2
        return
    fi
    if [ -n "$want_text" ] && ! printf '%s' "$out" | grep -qF -- "$want_text"; then
        fail "hook: $desc: output lacks '$want_text'. Output:"
        printf '%s\n' "$out" >&2
        return
    fi
    ok "hook: $desc"
}

mkdir -p "$REPO/testdata"
cp "$WORK/retained.tar.gz" "$REPO/testdata/e2e-fixture.tar.gz"
git -C "$REPO" add testdata/e2e-fixture.tar.gz
commit_expect "sensitive tarball with no Rust change is refused" refused "--no-verify"

# The hook must read the staged blob: a clean file left in the working tree
# over a staged sensitive one does not get the commit through.
cp "$WORK/clean.tar.gz" "$REPO/testdata/e2e-fixture.tar.gz"
commit_expect "staged blob is checked, not the working tree" refused "e2e-fixture.tar.gz"

git -C "$REPO" add testdata/e2e-fixture.tar.gz
commit_expect "clean tarball is accepted" accepted ""

echo
echo "$passes passed, $failures failed"
[ "$failures" -eq 0 ]
