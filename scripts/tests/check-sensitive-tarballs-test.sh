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
tar -cf "$WORK/retained-bare.tar" -C "$WORK/src-retained" host-a
tar -cf "$WORK/clean-bare.tar" -C "$WORK/src-clean" host-c

# A listing larger than the pipe buffer: a pipe into `grep -q` under
# pipefail reads an early match as no match. The operands are listed so the
# entitlement member comes first; directory order differs between file
# systems and would put it last on some.
BULK_MEMBERS=6000
make_tarball big big "$CLEAN_SNAPSHOT" subscription/entitlement/123-key.pem
mkdir -p "$WORK/src-big/big/bulk"
i=0
while [ "$i" -lt "$BULK_MEMBERS" ]; do
    : > "$WORK/src-big/big/bulk/member-with-a-reasonably-long-name-$i"
    i=$((i + 1))
done
tar -czf "$WORK/big.tar.gz" -C "$WORK/src-big" \
    big/subscription big/inspection-snapshot.json big/Containerfile big/bulk

# A member whose path starts with -- must reach tar as a member operand,
# never as an option. GNU tar runs a --checkpoint-action command while it
# reads the archive, so the marker file is the proof there. bsdtar refuses
# the option outright, so on macOS only the exit status below can fail;
# CI runs GNU tar. The padding carries GNU tar past its first checkpoint
# (every 10 records, 100 KiB, by default). The Containerfile after it makes
# "every member" unparseable, so only a literal read of the one member
# returns the flagged snapshot; it comes last so that jq, failing on it,
# does not stop tar before the checkpoint.
CHECKPOINT_PADDING_BYTES=204800
INJECT_MARKER="$WORK/injected/inspection-snapshot.json"
mkdir -p "$WORK/injected"
inject_member="--checkpoint-action=exec=touch $INJECT_MARKER"
mkdir -p "$WORK/src-inject/${inject_member%/*}"
printf '%s%*s' "$FLAGGED_SNAPSHOT" "$CHECKPOINT_PADDING_BYTES" '' \
    > "$WORK/src-inject/$inject_member"
echo "FROM scratch" > "$WORK/src-inject/Containerfile"
tar -czf "$WORK/inject.tar.gz" -C "$WORK/src-inject" -- "${inject_member%%/*}" Containerfile

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
expect "entitlement member in a large tarball is rejected" 1 "subscription/entitlement/" \
    "$WORK/big.tar.gz"
expect "uncompressed sensitive tarball is rejected" 1 "sensitive_retained" "$WORK/retained-bare.tar"
expect "uncompressed clean tarball passes" 0 "" "$WORK/clean-bare.tar"
expect "a member named like a tar option is read as a member" 1 "sensitive_snapshot is true" \
    "$WORK/inject.tar.gz"
if [ -e "$INJECT_MARKER" ]; then
    fail "a member named like a tar option ran its checkpoint command"
else
    ok "a member named like a tar option runs no command"
fi

# Without jq every snapshot would read as unparseable, which blames the
# tarball for a missing tool. A PATH holding only bash hides jq.
NO_JQ_BIN="$WORK/no-jq-bin"
mkdir -p "$NO_JQ_BIN"
ln -s "$(command -v bash)" "$NO_JQ_BIN/bash"
rc=0
out=$(PATH="$NO_JQ_BIN" "$SCRIPT" "$WORK/clean.tar.gz" 2>&1) || rc=$?
case "$rc:$out" in
    64:*"requires jq"*) ok "missing jq is a usage error naming jq" ;;
    *)
        fail "missing jq: exit $rc, want 64 naming jq. Output:"
        printf '%s\n' "$out" >&2
        ;;
esac

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

# refine loads a gzip archive whatever it is called, so a rename must not
# hide one. Each is staged alone, then unstaged.
for renamed in sensitive.fixture sensitive.tgz sensitive.tar; do
    case "$renamed" in
        *.tar) cp "$WORK/retained-bare.tar" "$REPO/testdata/$renamed" ;;
        *) cp "$WORK/retained.tar.gz" "$REPO/testdata/$renamed" ;;
    esac
    git -C "$REPO" add "testdata/$renamed"
    commit_expect "staged $renamed is checked" refused "testdata/$renamed"
    git -C "$REPO" reset -q -- "testdata/$renamed"
done

# The content check reads the staged blob too, not the working tree: a
# clean copy on disk over a staged renamed archive does not mask it.
git -C "$REPO" add testdata/sensitive.fixture
cp "$WORK/clean.tar.gz" "$REPO/testdata/sensitive.fixture"
commit_expect "staged blob of a renamed archive is checked, not the working tree" \
    refused "testdata/sensitive.fixture"
git -C "$REPO" reset -q -- testdata/sensitive.fixture

# Non-archives pass, and a symlink is its target path, not the file it
# points at.
echo "plain text" > "$REPO/testdata/notes.txt"
ln -s ../testdata/sensitive.tgz "$REPO/testdata/link-to-sensitive"
git -C "$REPO" add testdata/notes.txt testdata/link-to-sensitive
commit_expect "a text file and a symlink are accepted" accepted ""

# --tracked: the same renamed archives, committed past the hook.
cp "$WORK/retained.tar.gz" "$REPO/testdata/sensitive.fixture"
git -C "$REPO" add testdata/sensitive.fixture testdata/sensitive.tgz testdata/sensitive.tar
git -C "$REPO" commit -q --no-verify -m "test: renamed archives past the hook"
rc=0
out=$(cd "$REPO" && "$SCRIPT" --tracked 2>&1) || rc=$?
for renamed in sensitive.fixture sensitive.tgz sensitive.tar; do
    case "$rc:$out" in
        1:*"testdata/$renamed:"*) ok "--tracked checks $renamed" ;;
        *)
            fail "--tracked: $renamed: exit $rc, want 1 naming it. Output:"
            printf '%s\n' "$out" >&2
            ;;
    esac
done

echo
echo "$passes passed, $failures failed"
[ "$failures" -eq 0 ]
