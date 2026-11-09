#!/usr/bin/env bash
#
# Refuse inspectah output tarballs that carry preserved sensitive content.
#
# A scan run with --preserve keeps entitlement certificates, private keys,
# password hashes or SSH keys in its output, by the operator's choice. Such a
# tarball is for local use only. One was once copied onto a tracked fixture
# path on purpose and committed; .gitignore cannot stop a deliberate copy, so
# this check reads the content of every tarball at the commit boundary.
#
# One implementation, two callers:
#   .githooks/pre-commit                 --staged  (staged blobs, not the working tree)
#   .github/workflows/rust-ci.yml        --tracked (every tracked tarball)
#
# USAGE:
#   check-sensitive-tarballs.sh --staged
#   check-sensitive-tarballs.sh --tracked
#   check-sensitive-tarballs.sh FILE...
#
# A tarball is rejected when any of these hold:
#   - a member path contains subscription/entitlement/ (the tree the scan
#     tarball stages entitlement certs and keys into)
#   - its inspection-snapshot.json has redaction_state.state "sensitive_retained"
#   - its inspection-snapshot.json has sensitive_snapshot true (the scan sets
#     it for any --preserve or --no-redaction, and aggregate merges keep it
#     while dropping redaction_state)
#
# Unreadable input fails closed: a tarball tar cannot list, or a snapshot
# member jq cannot parse, is refused. A readable tarball with neither an
# inspection-snapshot.json nor an entitlement member is not inspectah output
# and passes.
#
# EXIT: 0 all pass, 1 sensitive content found, 2 unreadable input, 64 usage.
#
# Requires tar and jq. Written for bash 3.2 (macOS /bin/bash): no mapfile,
# no associative arrays.

set -euo pipefail

SNAPSHOT_MEMBER='inspection-snapshot.json'
ENTITLEMENT_TREE='subscription/entitlement/'
RETAINED_STATE='sensitive_retained'
SENSITIVE_FLAG='sensitive_snapshot'

EXIT_SENSITIVE=1
EXIT_UNREADABLE=2
EXIT_USAGE=64

rejected=0
unreadable=0

usage() {
    echo "usage: $0 --staged | --tracked | FILE..." >&2
    exit "$EXIT_USAGE"
}

# check_file <file on disk> <name to report>
check_file() {
    local file=$1 label=$2 members member verdict

    if ! members=$(tar -tzf "$file" 2>/dev/null); then
        echo "  $label: cannot be read as a gzip tarball" >&2
        unreadable=1
        return
    fi

    if printf '%s\n' "$members" | grep -qF -- "$ENTITLEMENT_TREE"; then
        echo "  $label: contains a $ENTITLEMENT_TREE member" >&2
        rejected=1
        return
    fi

    while IFS= read -r member; do
        case "$member" in
            "$SNAPSHOT_MEMBER" | */"$SNAPSHOT_MEMBER") ;;
            *) continue ;;
        esac
        # -e: an empty member yields no output and exits non-zero, so it
        # fails closed instead of reading as a clean snapshot.
        if ! verdict=$(tar -xzOf "$file" "$member" 2>/dev/null | jq -er \
            --arg state "$RETAINED_STATE" --arg flag "$SENSITIVE_FLAG" '
                if type != "object" then error("snapshot is not a JSON object")
                elif (.redaction_state.state? // null) == $state then "redaction_state.state is \($state)"
                elif (.[$flag]? // false) == true then "\($flag) is true"
                else "" end' 2>/dev/null); then
            echo "  $label: $member cannot be read as snapshot JSON" >&2
            unreadable=1
            return
        fi
        if [ -n "$verdict" ]; then
            echo "  $label: $member $verdict" >&2
            rejected=1
            return
        fi
    done <<EOF
$members
EOF
}

mode=${1:-}
[ -n "$mode" ] || usage

case "$mode" in
    --staged)
        tmp=$(mktemp -d)
        trap 'rm -rf "$tmp"' EXIT
        while IFS= read -r -d '' path; do
            if git cat-file blob ":$path" > "$tmp/blob" 2>/dev/null; then
                check_file "$tmp/blob" "$path"
            else
                echo "  $path: staged blob cannot be read" >&2
                unreadable=1
            fi
        done < <(git diff --cached --name-only -z --diff-filter=ACMR -- '*.tar.gz')
        ;;
    --tracked)
        while IFS= read -r -d '' path; do
            check_file "$path" "$path"
        done < <(git ls-files -z -- '*.tar.gz')
        ;;
    -*)
        usage
        ;;
    *)
        for path in "$@"; do
            check_file "$path" "$path"
        done
        ;;
esac

if [ "$rejected" -eq 0 ] && [ "$unreadable" -eq 0 ]; then
    exit 0
fi

if [ "$rejected" -ne 0 ]; then
    cat >&2 <<'EOF'

    !! BLOCKED: inspectah output with preserved sensitive content !!

    The tarball(s) above were produced with --preserve (or --no-redaction)
    and may hold entitlement certificates, private keys, password hashes or
    SSH keys. Output like this is local-only.

    Fix: re-produce the fixture without --preserve and without
    --no-redaction, then stage that tarball instead.
EOF
else
    cat >&2 <<'EOF'

    !! BLOCKED: tarball content could not be checked !!

    The gate refuses what it cannot read rather than passing it. Replace
    the tarball(s) above with a readable inspectah output, or remove them.
EOF
fi

if [ "$mode" = "--staged" ]; then
    cat >&2 <<'EOF'

    Committing to a private repository on purpose? Bypass this hook with
    git commit --no-verify. CI runs the same check over every tracked
    tarball, so the public repository still refuses it.

EOF
fi

if [ "$rejected" -ne 0 ]; then
    exit "$EXIT_SENSITIVE"
fi
exit "$EXIT_UNREADABLE"
