#!/usr/bin/env bash
# Check that crates/web/build.rs reruns when INSPECTAH_SKIP_UI changes.
#
# The pre-commit hook unsets INSPECTAH_SKIP_UI so the gates run the UI
# type check. That only works if Cargo reruns the web build script when the
# variable goes away; otherwise a build-script result cached from a skipped
# build is reused and the UI is never built or type-checked.
#
# The test copies the real crates/web/build.rs into a throwaway crate with
# no dependencies, creates every path the script names in a
# cargo:rerun-if-changed line, and puts a stub npm first on PATH that only
# records its arguments. It then runs real cargo builds and reads the stub's
# record to tell whether the build script ran:
#
#   1. a build with INSPECTAH_SKIP_UI=1 does not call npm;
#   2. the next build, with the variable unset, calls npm ci and npm run
#      build (the case the hook depends on);
#   3. a repeat unset build with nothing changed does not;
#   4. an unset build after a change under ui/src does;
#   5. a build with the variable set again does not call npm.
#
# What it does not prove: that the real UI builds or type-checks, that the
# web crate's own Cargo.toml or dependencies leave the script's tracking
# intact, or anything about profiles other than the default dev build.
#
# Run from anywhere: scripts/tests/web-build-skip-ui-test.sh
# Written for bash 3.2 (macOS /bin/bash): no mapfile, no associative arrays.
# Requires cargo; needs no network and no Node.js.
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BUILD_RS="$REPO_ROOT/crates/web/build.rs"
# Run cargo from the repository, so rustup picks the toolchain pinned in
# rust-toolchain.toml: the one whose fingerprinting the hook relies on.
cd "$REPO_ROOT"

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

# --- throwaway crate ---

CRATE="$WORK/crate"
mkdir -p "$CRATE/src"
cp "$BUILD_RS" "$CRATE/build.rs"
cat > "$CRATE/Cargo.toml" <<'EOF'
[package]
name = "skip-ui-probe"
version = "0.0.0"
edition = "2021"
publish = false

[workspace]
EOF
: > "$CRATE/src/lib.rs"

# Create every rerun-if-changed path, so a missing input does not make the
# script rerun on every build. A path with no extension is a directory.
tracked=$(grep -o 'cargo:rerun-if-changed=[^"]*' "$BUILD_RS" | sed 's/^cargo:rerun-if-changed=//')
if [ -z "$tracked" ]; then
    fail "no cargo:rerun-if-changed line found in $BUILD_RS"
fi
for path in $tracked; do
    case "$(basename "$path")" in
        *.*)
            mkdir -p "$CRATE/$(dirname "$path")"
            echo '{}' > "$CRATE/$path"
            ;;
        *)
            mkdir -p "$CRATE/$path"
            echo 'export {}' > "$CRATE/$path/main.ts"
            ;;
    esac
done

# --- stub npm ---

STUB_BIN="$WORK/bin"
NPM_LOG="$WORK/npm.log"
mkdir -p "$STUB_BIN"
cat > "$STUB_BIN/npm" <<EOF
#!/bin/sh
echo "\$*" >> "$NPM_LOG"
exit 0
EOF
chmod +x "$STUB_BIN/npm"
PATH="$STUB_BIN:$PATH"
export PATH

# The probe's build output stays in the scratch directory, whatever target
# directory the caller's environment names.
CARGO_TARGET_DIR="$WORK/target"
export CARGO_TARGET_DIR

# build_probe <label> [VAR=value]: run one cargo build of the probe with the
# stub log cleared, so the log afterwards holds only this build's npm calls.
build_probe() {
    local label=$1
    shift
    : > "$NPM_LOG"
    if ! env "$@" cargo build --quiet --manifest-path "$CRATE/Cargo.toml" \
        > "$WORK/cargo.out" 2>&1; then
        fail "$label: cargo build failed"
        cat "$WORK/cargo.out" >&2
        return 1
    fi
}

ran_ui_build() {
    grep -qx 'ci' "$NPM_LOG" && grep -qx 'run build' "$NPM_LOG"
}

called_npm() {
    [ -s "$NPM_LOG" ]
}

unset INSPECTAH_SKIP_UI

# 1. skipped build
if build_probe "skipped build" INSPECTAH_SKIP_UI=1; then
    if called_npm; then
        fail "skipped build called npm: $(tr '\n' ' ' < "$NPM_LOG")"
    else
        ok "a build with INSPECTAH_SKIP_UI=1 skips the UI build"
    fi
fi

# 2. unset after skipped: the hook's case
if build_probe "unset after skipped"; then
    if ran_ui_build; then
        ok "unsetting INSPECTAH_SKIP_UI reruns the build script and the UI build"
    else
        fail "unset build after a skipped build reused the cached build-script result (npm not run)"
    fi
fi

# 3. unset, nothing changed
if build_probe "unchanged unset build"; then
    if called_npm; then
        fail "an unset build with nothing changed reran the UI build"
    else
        ok "an unchanged unset build does not rerun the UI build"
    fi
fi

# 4. unset, UI source changed. Cargo compares file times, so wait past the
# filesystem's timestamp resolution before touching the file.
sleep 1
first_src=$(find "$CRATE/ui/src" -type f | head -n 1)
echo '// changed' >> "$first_src"
if build_probe "unset build after a UI change"; then
    if ran_ui_build; then
        ok "a change under ui/src reruns the UI build"
    else
        fail "an unset build after a change under ui/src did not rerun the UI build"
    fi
fi

# 5. set again
if build_probe "skipped build again" INSPECTAH_SKIP_UI=1; then
    if called_npm; then
        fail "setting INSPECTAH_SKIP_UI again called npm: $(tr '\n' ' ' < "$NPM_LOG")"
    else
        ok "setting INSPECTAH_SKIP_UI again skips the UI build"
    fi
fi

echo "$passes passed, $failures failed"
[ "$failures" -eq 0 ]
