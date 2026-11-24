#!/usr/bin/env bash
# Tests for scripts/check-version-stamp.sh.
#
# Drives the script against stand-in binaries in a throwaway git repository:
# small shell scripts for `run`, and byte files for `bytes`. The real release
# binaries are checked by the same script in build-binary.yml and release.yml.
# Run from anywhere: scripts/tests/check-version-stamp-test.sh
# Written for bash 3.2 (macOS /bin/bash).
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "$0")/../.." && pwd)
SCRIPT="$REPO_ROOT/scripts/check-version-stamp.sh"

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

# expect <0|1> <description> <script args...>
# Runs the script in the fake checkout and checks its exit status.
expect() {
    local want=$1 desc=$2
    shift 2
    local got=0
    (cd "$WORK/repo" && "$SCRIPT" "$@") > "$WORK/out" 2>&1 || got=$?
    if [ "$want" -eq 0 ] && [ "$got" -eq 0 ]; then
        ok "$desc"
    elif [ "$want" -ne 0 ] && [ "$got" -eq 1 ] && grep -q '^::error::' "$WORK/out"; then
        ok "$desc"
    else
        fail "$desc (exit ${got})"
        sed 's/^/    /' "$WORK/out" >&2
    fi
}

# fake_bin <name> <version line> [--version line]
# A stand-in binary that prints the first line for `version` and the second
# (default: the first) for `--version`.
fake_bin() {
    local path="$WORK/$1" line=$2 dash=${3:-$2}
    cat > "$path" <<EOF
#!/bin/sh
case "\$1" in
    version) echo '$line' ;;
    --version) echo '$dash' ;;
esac
EOF
    chmod +x "$path"
}

mkdir -p "$WORK/repo"
(
    cd "$WORK/repo"
    git init -q
    printf '[workspace.package]\nversion = "1.2.3-beta.4"\n' > Cargo.toml
    git add Cargo.toml
    git -c user.name=t -c user.email=t@example.invalid commit -qm init
)
HASH=$(git -C "$WORK/repo" rev-parse --short HEAD)

fake_bin tagged "inspectah 1.2.3-beta.4"
fake_bin untagged "inspectah 1.2.3-beta.4+g${HASH}"
fake_bin mismatch "inspectah 1.2.3-beta.4" "inspectah 1.2.3-beta.4+g${HASH}"
fake_bin stale "inspectah 1.2.3-beta.4+g0000000"
fake_bin old "inspectah 1.2.3-beta.4 (commit ${HASH}, built 2026-11-23)"

expect 0 "run tagged accepts the bare version" run "$WORK/tagged" tagged
expect 1 "run tagged rejects a +g suffix" run "$WORK/untagged" tagged
expect 1 "run tagged rejects the old commit and date line" run "$WORK/old" tagged
expect 0 "run untagged accepts +g<HEAD>" run "$WORK/untagged" untagged
expect 1 "run untagged rejects the bare version" run "$WORK/tagged" untagged
expect 1 "run untagged rejects another commit's hash" run "$WORK/stale" untagged
expect 1 "run rejects --version differing from version" run "$WORK/mismatch" tagged
expect 1 "run rejects a missing binary" run "$WORK/absent" tagged

# Byte files: the version line sits among other bytes with no separator, as
# string literals do in a binary's read-only data. The binary stores it with
# its trailing newline, which is what delimits the version.
printf 'xx\0inspectah 1.2.3-beta.4\nUsage: inspectah\0yy' > "$WORK/bytes-tagged"
printf 'xx\0inspectah 1.2.3-beta.4+g%s\nUsage\0yy' "$HASH" > "$WORK/bytes-untagged"
printf 'xx\0inspectah 1.2.3-beta.4+dev\nUsage\0yy' > "$WORK/bytes-dev"
printf 'xx\0inspectah 1.2.3-beta.3\nUsage\0yy' > "$WORK/bytes-other"
printf 'xx\0inspectah 1.2.3-beta.40\nUsage\0yy' > "$WORK/bytes-longer"
printf 'xx\0inspectah 1.2.3-beta.4 (commit %s, built 2026-11-23)Usage\0yy' "$HASH" > "$WORK/bytes-old"
printf 'xx\0inspectah 1.2.3-beta.4Usage\0yy' > "$WORK/bytes-unterminated"
printf 'xx\0inspectah 1.2.3-beta.4' > "$WORK/bytes-eof"

expect 0 "bytes accepts the bare version" bytes "$WORK/bytes-tagged" 1.2.3-beta.4
expect 1 "bytes rejects a +g stamp" bytes "$WORK/bytes-untagged" 1.2.3-beta.4
expect 1 "bytes rejects a debug +dev stamp" bytes "$WORK/bytes-dev" 1.2.3-beta.4
expect 1 "bytes rejects another version" bytes "$WORK/bytes-other" 1.2.3-beta.4
expect 1 "bytes rejects a longer version the expected one prefixes" bytes "$WORK/bytes-longer" 1.2.3-beta.4
expect 1 "bytes rejects the old commit and date line" bytes "$WORK/bytes-old" 1.2.3-beta.4
expect 1 "bytes rejects a version line with no newline after it" bytes "$WORK/bytes-unterminated" 1.2.3-beta.4
expect 1 "bytes rejects EOF without a newline" bytes "$WORK/bytes-eof" 1.2.3-beta.4
expect 1 "bytes rejects a missing binary" bytes "$WORK/absent" 1.2.3-beta.4

echo "${passes} passed, ${failures} failed"
[ "$failures" -eq 0 ]
