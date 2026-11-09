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
# --staged and --tracked check every file named *.tar.gz, *.tgz or *.tar,
# and every other regular file whose content starts with the gzip magic:
# refine loads a gzip archive whatever it is called, so a rename must not
# hide one. FILE arguments are always checked.
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
# member jq cannot parse, is refused. So is a gzip file that is not a tar
# archive. A readable tarball with neither an inspection-snapshot.json nor
# an entitlement member is not inspectah output and passes.
#
# EXIT: 0 all pass, 1 sensitive content found, 2 unreadable input, 64 usage
# (including a missing jq).
#
# Requires tar and jq. Written for bash 3.2 (macOS /bin/bash): no mapfile,
# no associative arrays.
#
# Two traps this script must keep out of:
#   - Member names come from the archive. Every tar call that passes one
#     puts -- before it, or a member named --checkpoint-action=exec=... is
#     a GNU tar option that runs a command.
#   - Under pipefail, `producer | grep -q` fails when grep exits on an early
#     match and the producer dies on SIGPIPE, so a match reads as no match.
#     Match with case or a here-string, never a pipe into grep -q or head.

set -euo pipefail

SNAPSHOT_MEMBER='inspection-snapshot.json'
ENTITLEMENT_TREE='subscription/entitlement/'
RETAINED_STATE='sensitive_retained'
SENSITIVE_FLAG='sensitive_snapshot'
GZIP_MAGIC=$'\x1f\x8b'

EXIT_SENSITIVE=1
EXIT_UNREADABLE=2
EXIT_USAGE=64

rejected=0
unreadable=0

usage() {
    echo "usage: $0 --staged | --tracked | FILE..." >&2
    exit "$EXIT_USAGE"
}

# Without jq every snapshot reads as unparseable, which would blame the
# tarball for a missing tool.
if ! command -v jq >/dev/null 2>&1; then
    echo "$0: requires jq, which is not on PATH" >&2
    exit "$EXIT_USAGE"
fi

has_tarball_name() {
    case "$1" in
        *.tar.gz | *.tgz | *.tar) return 0 ;;
    esac
    return 1
}

# A builtin read, not a process per file: --tracked sniffs the whole tree.
# The C locale makes read count bytes rather than characters.
starts_with_gzip_magic() {
    local LC_ALL=C magic=
    IFS= read -r -n 2 -d '' magic 2>/dev/null < "$1" || true
    [ "$magic" = "$GZIP_MAGIC" ]
}

# check_file <file on disk> <name to report>
check_file() {
    local file=$1 label=$2 members member verdict

    # No -z: tar detects gzip itself, and a bare .tar is checked too.
    if ! members=$(tar -tf "$file" 2>/dev/null); then
        echo "  $label: cannot be read as a tarball" >&2
        unreadable=1
        return
    fi

    case "$members" in
        *"$ENTITLEMENT_TREE"*)
            echo "  $label: contains a $ENTITLEMENT_TREE member" >&2
            rejected=1
            return
            ;;
    esac

    while IFS= read -r member; do
        case "$member" in
            "$SNAPSHOT_MEMBER" | */"$SNAPSHOT_MEMBER") ;;
            *) continue ;;
        esac
        # -e: an empty member yields no output and exits non-zero, so it
        # fails closed instead of reading as a clean snapshot.
        if ! verdict=$(tar -xOf "$file" -- "$member" 2>/dev/null | jq -er \
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
        root=$(git rev-parse --show-toplevel)
        # Staged paths whose working-tree copy differs from the staged blob.
        # Every other staged file holds the staged bytes on disk, so the
        # content sniff reads it there instead of running git once per file.
        unstaged=$'\n'
        while IFS= read -r -d '' path; do
            unstaged="$unstaged$path"$'\n'
        done < <(git diff --name-only -z)
        # --raw gives each entry's new mode and blob id, one NUL-terminated
        # meta field then the path. --no-renames keeps it to one path each.
        while IFS= read -r -d '' meta; do
            IFS= read -r -d '' path
            read -r _ blob_mode _ blob _ <<<"$meta"
            # Only a regular file can hold an archive: a symlink blob is its
            # target path, and a gitlink is a commit id.
            case "$blob_mode" in
                100644 | 100755) ;;
                *) continue ;;
            esac
            if ! has_tarball_name "$path"; then
                case "$unstaged" in
                    *$'\n'"$path"$'\n'*) ;;
                    *)
                        if [ -f "$root/$path" ] && [ ! -L "$root/$path" ] \
                            && ! starts_with_gzip_magic "$root/$path"; then
                            continue
                        fi
                        ;;
                esac
            fi
            if ! git cat-file blob "$blob" > "$tmp/blob" 2>/dev/null; then
                echo "  $path: staged blob cannot be read" >&2
                unreadable=1
                continue
            fi
            if has_tarball_name "$path" || starts_with_gzip_magic "$tmp/blob"; then
                check_file "$tmp/blob" "$path"
            fi
        done < <(git diff --cached --raw -z --no-abbrev --no-renames --diff-filter=ACMT)
        ;;
    --tracked)
        while IFS= read -r -d '' path; do
            if has_tarball_name "$path" \
                || { [ -f "$path" ] && [ ! -L "$path" ] && starts_with_gzip_magic "$path"; }; then
                check_file "$path" "$path"
            fi
        done < <(git ls-files -z)
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
    git commit --no-verify. Never bypass it in a clone that pushes to a
    public remote: CI runs the same check, but only after a push. On main
    the content is already public by then, and on a pull request CI can
    stop the merge, not the upload.

EOF
fi

if [ "$rejected" -ne 0 ]; then
    exit "$EXIT_SENSITIVE"
fi
exit "$EXIT_UNREADABLE"
