#!/usr/bin/env bash
# Check that crates/cli/build.rs stamps the right version when a tag appears
# or changes only in `packed-refs`.
#
# The test copies the real crates/cli/build.rs into a throwaway crate whose
# binary prints INSPECTAH_VERSION, inside a throwaway git repository with no
# `packed-refs` file. It then runs real release builds, changing only
# `packed-refs` between them, and checks what the binary prints:
#
#   1. no tag, no packed-refs: `<version>+g<hash>`;
#   2. packed-refs created, holding the tag v<version> on HEAD: the bare
#      version (the file did not exist at the last build);
#   3. the tag in packed-refs renamed to another version: `+g<hash>`;
#   4. the matching tag back in packed-refs: the bare version;
#   5. packed-refs with no tag left: `+g<hash>`.
#
# Each step flips the expected output, so a step passes only if cargo reran
# the build script. packed-refs is written directly, in the form
# `git pack-refs` leaves: pack-refs also deletes the loose ref, which changes
# the watched refs directory and would hide a packed-refs file cargo does
# not watch.
#
# What it does not prove: loose tags, linked worktrees, detached HEAD, or a
# build with no git, which build-rs-version-metadata.md describes.
#
# Run from anywhere: scripts/tests/cli-build-version-test.sh
# Written for bash 3.2 (macOS /bin/bash). Requires cargo and git; needs no
# network.
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BUILD_RS="$REPO_ROOT/crates/cli/build.rs"
# Run cargo from the repository, so rustup picks the toolchain pinned in
# rust-toolchain.toml.
cd "$REPO_ROOT"

# The throwaway repository must not see the caller's repository: a hook or a
# worktree exports GIT_DIR and friends, which would point every git command
# in build.rs at the wrong repository.
# shellcheck disable=SC2046
unset $(git rev-parse --local-env-vars)

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
GIT_CEILING_DIRECTORIES=$WORK
export GIT_CEILING_DIRECTORIES

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

VERSION=1.2.3
CRATE="$WORK/crate"
mkdir -p "$CRATE/src"
cp "$BUILD_RS" "$CRATE/build.rs"
cat > "$CRATE/Cargo.toml" <<EOF
[package]
name = "cli-build-version-probe"
version = "$VERSION"
edition = "2021"
publish = false

[workspace]
EOF
cat > "$CRATE/src/main.rs" <<'EOF'
fn main() {
    println!("{}", env!("INSPECTAH_VERSION"));
}
EOF
printf 'target/\n' > "$CRATE/.gitignore"

git -C "$CRATE" init -q
git -C "$CRATE" add .
git -C "$CRATE" -c user.name=t -c user.email=t@example.invalid commit -qm init
HEAD_ID=$(git -C "$CRATE" rev-parse HEAD)
SHORT=$(git -C "$CRATE" rev-parse --short HEAD)
PACKED=$(git -C "$CRATE" rev-parse --path-format=absolute --git-path packed-refs)
if [ -e "$PACKED" ]; then
    fail "git init created $PACKED; the test needs it absent"
fi

# The probe builds into the crate's own target directory, whatever target
# directory the caller's environment names.
CARGO_TARGET_DIR="$CRATE/target"
export CARGO_TARGET_DIR

# expect_build <description> <expected output>
expect_build() {
    local desc=$1 want=$2 got
    if ! cargo build --quiet --release --manifest-path "$CRATE/Cargo.toml" \
        > "$WORK/cargo.out" 2>&1; then
        fail "$desc: cargo build failed"
        sed 's/^/    /' "$WORK/cargo.out" >&2
        return 0
    fi
    got=$("$CARGO_TARGET_DIR/release/cli-build-version-probe")
    if [ "$got" = "$want" ]; then
        ok "$desc"
    else
        fail "$desc: printed '$got', expected '$want'"
    fi
}

# write_packed [tag]: replace packed-refs with one holding only that tag on
# HEAD, or no ref at all. Cargo compares file times, so wait past the
# filesystem's timestamp resolution first.
write_packed() {
    sleep 1
    {
        echo '# pack-refs with: peeled fully-peeled sorted '
        if [ $# -gt 0 ]; then
            echo "$HEAD_ID refs/tags/$1"
        fi
    } > "$PACKED"
}

expect_build "no tag and no packed-refs gives +g<hash>" "$VERSION+g$SHORT"

write_packed "v$VERSION"
expect_build "a matching tag in a newly created packed-refs gives the bare version" "$VERSION"

write_packed "v9.9.9"
expect_build "a different tag in packed-refs gives +g<hash>" "$VERSION+g$SHORT"

write_packed "v$VERSION"
expect_build "the matching tag back in packed-refs gives the bare version" "$VERSION"

write_packed
expect_build "packed-refs with no tag gives +g<hash>" "$VERSION+g$SHORT"

echo "$passes passed, $failures failed"
[ "$failures" -eq 0 ]
