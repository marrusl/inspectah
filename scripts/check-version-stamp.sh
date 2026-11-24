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
#       the stamp from its bytes. It must embed the line `inspectah <version>`
#       with its newline, as crates/cli/src/commands/version.rs stores it, and
#       no `inspectah <version>+...` form. Of builds from a git checkout,
#       only a build of the release tag satisfies both: a longer version, a
#       `+` suffix or the old `(commit ..., built ...)` line does not end at
#       the newline. A build with no git metadata also prints the bare line.
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
        # The line is preceded by other string data with no separator, so
        # only its end can be checked: the newline byte stored with it.
        # grep cannot check that byte, since its `$` also matches at end of
        # input, so perl searches the raw bytes for the literal string,
        # newline included. The version is passed in the environment, not
        # interpolated into the program.
        STAMP_LINE="inspectah ${version}" perl -0777 -ne \
            '$found ||= index($_, "$ENV{STAMP_LINE}\n") >= 0; END { $? = $found ? 0 : 1 }' "$bin" \
            || error "${bin} does not embed the line 'inspectah ${version}'"
        # Other code embeds `inspectah <version>` too (the redaction engine's
        # `redacted_by`), and a future line printing it would end in a
        # newline as well. The `+` form appears only in an untagged or debug
        # build's version line, so rejecting it keeps such a build failing.
        # Always -q on a binary: grep would print kilobytes of the
        # surrounding bytes per match.
        if LC_ALL=C grep -qaF -- "inspectah ${version}+" "$bin"; then
            error "${bin} embeds 'inspectah ${version}+...', the stamp of an untagged or debug build"
        fi
        echo "${bin}: inspectah ${version}"
        ;;
    *)
        usage
        ;;
esac
