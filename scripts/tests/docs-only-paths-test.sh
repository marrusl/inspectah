#!/usr/bin/env bash
# Check that no Rust source under crates/ reads a docs-only path.
#
# The pre-commit hook skips the cargo gates for a commit that changes only
# paths on .githooks/docs-only-paths. That is safe only while no build input
# reads such a path. This script checks, over every tracked crates/**/*.rs
# file:
#
#   - every include_str! and include_bytes! argument resolves, from the
#     source file's directory, to a path outside the list; an argument that
#     is not a plain string literal fails as unresolved;
#   - no string literal, normalized lexically with its leading ./ and ../
#     segments stripped, matches the list or names a docs-only directory on
#     its own (docs, process-docs, .github, packaging, .copr, completions,
#     with or without a trailing /), unless its file and literal pair is on
#     the allowlist below.
#
# What it does not cover: a path assembled from fragments none of which
# names a docs-only path or directory on its own (format!("{}s/x.md", "doc"),
# or a directory walk from the repo root), a path read from the environment
# or from a file, and a read by a non-Rust build input (crates/web/ui sources
# and config, crates/pipeline/templates).
#
# Run from anywhere: scripts/tests/docs-only-paths-test.sh
# Written for bash 3.2 (macOS /bin/bash): no mapfile, no associative arrays.
# Requires git and perl.
set -euo pipefail

# Allowlist: one "<file> <literal>" pair per line, each under a comment
# giving the reason it is not a read from the repository. A pair whose
# literal no longer occurs in its file fails as a stale allowlist entry.
REPO_ALLOWLIST='
# expected member of a rendered output tarball
crates/cli/tests/copy_source_exists_test.rs README.md
# read from a tempfile::TempDir the test renders into
crates/cli/tests/units_scan_test.rs README.md
# name of the rendered output file under test
crates/cli/tests/warning_markdown_test.rs README.md
# fixture file name under a fake /etc/pki/entitlement
crates/collect/src/inspectors/subscription.rs notes.md
# the renderer writes README.md into the output directory
crates/pipeline/src/render/mod.rs README.md
# test writes it into a temporary source tree
crates/pipeline/src/render/tarball.rs README.md
# expected member list of the rendered output
crates/pipeline/tests/e2e.rs README.md
'

REPO_ROOT=$(cd "$(dirname "$0")/../.." && pwd)

# load_docs_only <file>: print the list as one ERE. Blank lines and lines
# starting with # are dropped and the rest joined with |, the same way the
# pre-commit hook loads it.
load_docs_only() {
    local raw line re=
    raw=$(cat "$1")
    while IFS= read -r line; do
        case "$line" in '' | '#'*) continue ;; esac
        re="${re:+$re|}$line"
    done <<<"$raw"
    [ -n "$re" ] || return 1
    printf '%s\n' "$re"
}

# check_tree <root> <allowlist>: print one line per finding and return
# non-zero if there is any.
check_tree() {
    local root=$1 allowlist=$2 re files
    re=$(load_docs_only "$root/.githooks/docs-only-paths")
    files=$(git -C "$root" ls-files -- ':(glob)crates/**/*.rs')
    [ -n "$files" ] || return 0
    (cd "$root" && DOCS_ONLY_RE=$re ALLOWLIST=$allowlist FILES=$files \
        perl -0777 -e "$CHECK_TREE_PERL")
}

# The tokenizer behind check_tree. It walks each file once, skipping line
# comments, nested block comments and char literals, and reads plain
# ("...", b"...") and raw (r"...", r#"..."#) string literals. Lifetimes
# ('a) are left alone because a char literal needs its closing quote.
# shellcheck disable=SC2016
CHECK_TREE_PERL='
use strict;
use warnings;
use File::Spec;

my $re = qr/$ENV{DOCS_ONLY_RE}/;
# A pattern of the form ^dir/ also names the bare directory dir.
my @dirs;
for my $p (split /\|/, $ENV{DOCS_ONLY_RE}) {
    if ($p =~ m{^\^(.+)/$}) { (my $d = $1) =~ s/\\(.)/$1/g; push @dirs, $d; }
}
my %allow;
for my $line (split /\n/, $ENV{ALLOWLIST}) {
    next if $line =~ /^\s*(#|$)/;
    my ($file, $lit) = $line =~ /^\s*(\S+)\s+(\S+)\s*$/
        or die "bad allowlist line: $line\n";
    $allow{"$file\0$lit"} = 0;
}

my $failures = 0;
sub report { print "$_[0]\n"; $failures++; }

# Resolve "a/../b" lexically; a path above the root keeps its leading "..".
sub normalize {
    my @out;
    for my $seg (split m{/}, $_[0]) {
        next if $seg eq "" || $seg eq ".";
        if ($seg eq ".." && @out && $out[-1] ne "..") { pop @out; next; }
        push @out, $seg;
    }
    return join "/", @out;
}

sub docs_only {
    my $path = shift;
    return 1 if $path =~ $re;
    (my $bare = $path) =~ s{/$}{};
    return scalar grep { $_ eq $bare } @dirs;
}

sub unescape {
    my $s = shift;
    $s =~ s/\\(["\\])/$1/g;
    return $s;
}

for my $file (split /\n/, $ENV{FILES}) {
    open my $fh, "<", $file or die "$file: $!\n";
    my $src = <$fh>;
    close $fh;
    $src = "" unless defined $src;
    (my $dir = $file) =~ s{/[^/]*$}{};
    my $line = 1;
    my $include;    # set when an include macro opened just before
    pos($src) = 0;
    while (pos($src) < length $src) {
        my $start = pos($src);
        my $lit;
        if ($src =~ m{\G//[^\n]*}gc) {
        } elsif ($src =~ m{\G/\*}gc) {
            my $depth = 1;
            while ($depth && $src =~ m{\G(?:[^/*]+|/\*|\*/|.)}gcs) {
                $depth++ if $& eq "/*";
                $depth-- if $& eq "*/";
            }
        } elsif ($src =~ m{\Gb?r(\#*)"(.*?)"\1}gcs) {
            $lit = $2;
        } elsif ($src =~ m{\Gb?"((?:[^"\\]|\\.)*)"}gcs) {
            $lit = unescape($1);
        } elsif ($src =~ m{\Gb?\x27(?:[^\x27\\\n]|\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]+\}|.))\x27}gc) {
        } elsif ($src =~ m{\G(include_(?:str|bytes))\s*!\s*\(\s*}gc) {
            $include = $1;
            $line += (substr($src, $start, pos($src) - $start) =~ tr/\n//);
            my $arg_start = pos($src);
            if ($src =~ m{\G"((?:[^"\\]|\\.)*)"\s*,?\s*\)}gcs
                || $src =~ m{\Gr(\#*)"(.*?)"\1\s*,?\s*\)}gcs) {
                my $arg = defined $2 ? $2 : unescape($1);
                my $resolved = normalize(substr($arg, 0, 1) eq "/" ? $arg : "$dir/$arg");
                $resolved = "/$resolved" if substr($arg, 0, 1) eq "/";
                report("$file:$line: $include!(\"$arg\") -> $resolved")
                    if docs_only($resolved);
            } else {
                report("$file:$line: $include! argument is not a string literal: unresolved include");
                pos($src) = $arg_start;
            }
            $start = $arg_start;
        } elsif ($src =~ m{\G[A-Za-z_][A-Za-z0-9_]*}gc) {
        } else {
            $src =~ m{\G.}gcs;
        }
        if (defined $lit) {
            (my $norm = $lit) =~ s{^(?:\.\.?/)+}{};
            if (docs_only($norm)) {
                if (exists $allow{"$file\0$lit"}) {
                    $allow{"$file\0$lit"}++;
                } else {
                    report("$file:$line: \"$lit\" -> $norm");
                }
            }
        }
        $line += (substr($src, $start, pos($src) - $start) =~ tr/\n//);
    }
}

for my $key (sort keys %allow) {
    next if $allow{$key};
    my ($file, $lit) = split /\0/, $key;
    report("$file: \"$lit\": stale allowlist entry");
}
exit($failures ? 1 : 0);
'

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

case_no=0

# planted <description> <source of crates/x/src/lib.rs> <allowlist>
#         <want: pass|fail> [expected output substring...]
planted() {
    local desc=$1 source=$2 allowlist=$3 want=$4 tree out rc=0 text
    shift 4
    case_no=$((case_no + 1))
    tree="$WORK/tree-$case_no"
    mkdir -p "$tree/.githooks" "$tree/crates/x/src"
    git init -q "$tree"
    cp "$REPO_ROOT/.githooks/docs-only-paths" "$tree/.githooks/docs-only-paths"
    printf '%s\n' "$source" > "$tree/crates/x/src/lib.rs"
    git -C "$tree" add -A
    git -C "$tree" -c user.name=Test -c user.email=test@example.invalid \
        -c commit.gpgsign=false commit -q --no-verify -m "seed"
    out=$(check_tree "$tree" "$allowlist" 2>&1) || rc=$?
    if [ "$want" = pass ] && [ "$rc" -ne 0 ]; then
        fail "$desc: failed, want pass. Output:"
        printf '%s\n' "$out" >&2
        return
    fi
    if [ "$want" = fail ] && [ "$rc" -eq 0 ]; then
        fail "$desc: passed, want fail. Output:"
        printf '%s\n' "$out" >&2
        return
    fi
    for text in "$@"; do
        case "$out" in
            *"$text"*) ;;
            *)
                fail "$desc: output lacks '$text'. Output:"
                printf '%s\n' "$out" >&2
                return
                ;;
        esac
    done
    ok "$desc"
}

LIB=crates/x/src/lib.rs

planted "a planted include under docs/ fails" \
    'pub static X: &str = include_str!("../../../docs/x.md");' \
    "" fail "$LIB:1:" "-> docs/x.md"
planted "a planted literal joined to the repo root fails" \
    'pub fn f() { let _ = repo_root().join("docs/input.json"); }' \
    "" fail "$LIB:1:" "-> docs/input.json"
planted "a planted directory-name literal fails" \
    'pub fn f(p: &Path) { let _ = p.join("..").join("..").join("process-docs"); }' \
    "" fail "$LIB:1:" "-> process-docs"
planted "a planted runtime path to a top-level markdown file fails" \
    'pub fn f() { let _ = std::fs::read("../../README.md"); }' \
    "" fail "$LIB:1:" "-> README.md"
planted "a literal in a comment is ignored" \
    '// see "docs/x.md"
pub fn f() {}' \
    "" pass
planted "an allowlisted literal passes" \
    'pub const NAME: &str = "README.md";' \
    "$LIB README.md" pass
planted "a stale allowlist entry fails" \
    'pub fn f() {}' \
    "$LIB README.md" fail "$LIB: \"README.md\": stale allowlist entry"
planted "a non-literal include argument fails as unresolved" \
    'pub static X: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/x"));' \
    "" fail "$LIB:1:" "unresolved include"

rc=0
out=$(check_tree "$REPO_ROOT" "$REPO_ALLOWLIST" 2>&1) || rc=$?
if [ "$rc" -eq 0 ]; then
    ok "the repository passes"
else
    fail "the repository passes: exit $rc. Output:"
    printf '%s\n' "$out" >&2
fi

echo
echo "$passes passed, $failures failed"
[ "$failures" -eq 0 ]
