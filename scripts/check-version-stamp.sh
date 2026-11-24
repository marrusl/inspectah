#!/usr/bin/env bash
# Checks the version line a release binary carries. Called by
# .github/workflows/build-binary.yml and release.yml; tested by
# scripts/tests/check-version-stamp-test.sh.
#
#   check-version-stamp.sh run <binary> tagged|untagged
#       Runs `<binary> version` and `<binary> --version`. Both must print
#       exactly `inspectah <version>` for a build of the release tag, or
#       `inspectah <version>+g<short HEAD>` for any other release build.
#       Run it from the checkout the binary was built from: the version comes
#       from its Cargo.toml and the hash from its git HEAD.
#
#   check-version-stamp.sh bytes <binary> <version>
#       For a binary built for another platform, which cannot run here: reads
#       the stamp from its bytes. It must embed `inspectah <version>` and no
#       `inspectah <version>+...` form, which only a build of the release tag
#       satisfies.
#
# Failures print an `::error::` line naming the binary and exit 1.
# Written for bash 3.2 (macOS /bin/bash).
set -euo pipefail

usage() {
    echo "usage: $0 run <binary> tagged|untagged" >&2
    echo "       $0 bytes <binary> <version>" >&2
    exit 2
}

error() {
    echo "::error::$*" >&2
    exit 1
}

[ $# -eq 3 ] || usage
mode=$1 bin=$2

[ -s "$bin" ] || error "${bin} is missing or empty"

case "$mode" in
    run)
        version=$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\(.*\)"/\1/p' Cargo.toml | head -1)
        [ -n "$version" ] || error "could not read the version from Cargo.toml"
        case "$3" in
            tagged) expected="inspectah ${version}" ;;
            untagged) expected="inspectah ${version}+g$(git rev-parse --short HEAD)" ;;
            *) usage ;;
        esac
        for args in version --version; do
            out=$("$bin" "$args")
            echo "${bin} ${args}: ${out}"
            [ "$out" = "$expected" ] || error "${bin} ${args} printed '${out}', expected '${expected}'"
        done
        ;;
    bytes)
        version=$3
        # Always -q or -o on a binary: an unanchored grep prints kilobytes
        # of the surrounding bytes per match.
        LC_ALL=C grep -qaF -- "inspectah ${version}" "$bin" \
            || error "${bin} does not embed 'inspectah ${version}'"
        # The stamp is followed by other string data with no separator, so
        # only the `+` that starts build metadata is matched, not what follows.
        if LC_ALL=C grep -qaF -- "inspectah ${version}+" "$bin"; then
            error "${bin} embeds 'inspectah ${version}+...', the stamp of an untagged or debug build"
        fi
        echo "${bin}: inspectah ${version}"
        ;;
    *)
        usage
        ;;
esac
