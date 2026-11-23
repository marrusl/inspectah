#!/usr/bin/env bash
# Check that no Rust source under crates/ reads a docs-only path.
#
# The pre-commit hook skips the cargo gates for a commit that changes only
# paths on .githooks/docs-only-paths. That is safe only while no build input
# reads such a path. This script checks, over every tracked crates/**/*.rs
# file:
#
#   - every include_str! and include_bytes! argument, with any of the
#     (), [] or {} delimiters, resolves from the source file's directory to
#     a path outside the list; an argument that is not a single string
#     literal fails as unresolved;
#   - no string literal, normalized lexically (each ../ removes the segment
#     before it, empty and ./ segments are dropped, and any ../ left at the
#     start is stripped), matches the list or names a docs-only directory on
#     its own (docs, process-docs, .github, packaging, .copr, completions,
#     with or without a trailing /), unless its file and literal pair is on
#     the allowlist below. So "../../README.md", "/../../docs/x.md" after
#     CARGO_MANIFEST_DIR, "{}/../../README.md" in a format! string, and
#     "docs/sub/../input.json" are all caught.
#
# Plain literals are decoded first: \x and \u{} escapes, the one-character
# escapes, and a backslash-newline continuation. The scan fails, rather
# than passing, when the list has no pattern, git ls-files fails, or there
# is no tracked crates/**/*.rs file to read.
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
# read from the tempfile::TempDir a scan or refine export renders into
crates/cli/tests/kargs_file_export_test.rs README.md
# read from the tempfile::TempDir a scan or refine export renders into
crates/cli/tests/machine_state_scan_test.rs README.md
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
    raw=$(cat "$1") || return 1
    while IFS= read -r line; do
        case "$line" in '' | '#'*) continue ;; esac
        re="${re:+$re|}$line"
    done <<<"$raw"
    [ -n "$re" ] || return 1
    printf '%s\n' "$re"
}

# check_tree <root> <allowlist>: print one line per finding and return
# non-zero if there is any, or if the scan cannot run. Every step checks
# its own status: callers run this as `check_tree ... || rc=$?`, and set -e
# does nothing inside a function called that way.
check_tree() {
    local root=$1 allowlist=$2 list re files
    list="$root/.githooks/docs-only-paths"
    if ! re=$(load_docs_only "$list"); then
        echo "check_tree: cannot read a pattern from $list"
        return 1
    fi
    if ! files=$(git -C "$root" ls-files -- ':(glob)crates/**/*.rs'); then
        echo "check_tree: git ls-files failed in $root"
        return 1
    fi
    if [ -z "$files" ]; then
        echo "check_tree: no tracked crates/**/*.rs file in $root, so nothing was checked"
        return 1
    fi
    (cd "$root" && DOCS_ONLY_RE=$re ALLOWLIST=$allowlist FILES=$files \
        perl -0777 -e "$CHECK_TREE_PERL")
}

# The tokenizer behind check_tree. It walks each file once, skipping line
# comments, nested block comments and char literals, and reads plain
# ("...", b"...") and raw (r"...", r#"..."#) string literals. Lifetimes
# ('a) are left alone because a char literal needs its closing quote. A
# \u{} escape is decoded to UTF-8 bytes, so every string stays bytes.
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

my %escape = (n => "\n", r => "\r", t => "\t", "0" => "\0",
    "\\" => "\\", "\"" => "\"", "\x27" => "\x27");

# Decode a plain Rust string literal: \xNN, \u{N...}, the one-character
# escapes, and a backslash before a newline, which drops the newline and
# the whitespace after it. An unknown escape keeps its character.
sub unescape {
    my $s = shift;
    $s =~ s{\\(?:\r?\n[ \t\r\n]*|x([0-9a-fA-F]{2})|u\{([0-9a-fA-F_]+)\}|(.))}{
        if (defined $1) { chr hex $1 }
        elsif (defined $2) {
            (my $h = $2) =~ tr/_//d;
            my $c = chr hex $h;
            utf8::encode($c);
            $c;
        }
        elsif (defined $3) { exists $escape{$3} ? $escape{$3} : $3 }
        else { "" }
    }gse;
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
        } elsif ($src =~ m{\G(include_(?:str|bytes))\s*!\s*([(\[\{])\s*}gc) {
            $include = $1;
            my $open = $2;
            my $close = { "(" => ")", "[" => "]", "{" => "}" }->{$open};
            $line += (substr($src, $start, pos($src) - $start) =~ tr/\n//);
            my $arg_start = pos($src);
            if ($src =~ m{\G("((?:[^"\\]|\\.)*)")\s*,?\s*\Q$close\E}gcs
                || $src =~ m{\G(r(\#*)"(.*?)"\2)\s*,?\s*\Q$close\E}gcs) {
                my ($text, $arg) = defined $3 ? ($1, $3) : ($1, unescape($2));
                my $resolved = normalize(substr($arg, 0, 1) eq "/" ? $arg : "$dir/$arg");
                $resolved = "/$resolved" if substr($arg, 0, 1) eq "/";
                report("$file:$line: $include!$open$text$close -> $resolved")
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
            # normalize drops empty and . segments, so a leading / or ./
            # goes with them; what is left above the root is stripped.
            (my $norm = normalize($lit)) =~ s{^(?:\.\.(?:/|$))+}{};
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

# git, as check_tree calls it. With INJECT_LS_FILES_FAILURE set, ls-files
# fails as git does outside a repository.
git() {
    if [ -n "${INJECT_LS_FILES_FAILURE-}" ]; then
        case " $* " in
            *" ls-files "*)
                echo "fatal: injected ls-files failure" >&2
                return 128
                ;;
        esac
    fi
    command git "$@"
}

# new_tree <source of crates/x/src/lib.rs, or empty for no such file>:
# a committed throwaway tree carrying the repository's docs-only list, its
# path in $TREE.
TREE=
new_tree() {
    case_no=$((case_no + 1))
    TREE="$WORK/tree-$case_no"
    mkdir -p "$TREE/.githooks"
    command git init -q "$TREE"
    cp "$REPO_ROOT/.githooks/docs-only-paths" "$TREE/.githooks/docs-only-paths"
    if [ -n "$1" ]; then
        mkdir -p "$TREE/crates/x/src"
        printf '%s\n' "$1" > "$TREE/crates/x/src/lib.rs"
    fi
    command git -C "$TREE" add -A
    command git -C "$TREE" -c user.name=Test -c user.email=test@example.invalid \
        -c commit.gpgsign=false commit -q --no-verify -m "seed"
}

# planted <description> <source of crates/x/src/lib.rs> <allowlist>
#         <want: pass|fail> [expected output substring...]
planted() {
    local desc=$1 source=$2 allowlist=$3 want=$4
    shift 4
    new_tree "$source"
    expect_scan "$desc" "$TREE" "$allowlist" "$want" "$@"
}

# expect_scan <description> <tree> <allowlist> <want: pass|fail>
#             [expected output substring...]
expect_scan() {
    local desc=$1 tree=$2 allowlist=$3 want=$4 out rc=0 text
    shift 4
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
    "" fail "$LIB:1:" 'include_str!("../../../docs/x.md") -> docs/x.md'
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

# Rust string escapes are decoded before a literal is classified.
planted "an include with a \\x escape fails" \
    'pub static X: &str = include_str!("../../../\x64ocs/x.md");' \
    "" fail "$LIB:1:" '-> docs/x.md'
planted "a literal with a \\u{} escape fails" \
    'pub fn f() { let _ = repo_root().join("\u{64}ocs/input.json"); }' \
    "" fail "$LIB:1:" '-> docs/input.json'
planted "a literal with a line continuation fails" \
    'pub fn f() { let _ = std::fs::read("../../\
    docs/x.md"); }' \
    "" fail "$LIB:1:" '-> docs/x.md'

# A macro takes (), [] or {}. The literal rule alone would also report the
# path, so these assert the include rule's own line.
planted "an include with bracket delimiters fails" \
    'pub static X: &str = include_str!["../../../docs/x.md"];' \
    "" fail "$LIB:1:" 'include_str!["../../../docs/x.md"] -> docs/x.md'
planted "an include with brace delimiters fails" \
    'pub static X: &[u8] = include_bytes!{"../../../docs/x.md"};' \
    "" fail "$LIB:1:" 'include_bytes!{"../../../docs/x.md"} -> docs/x.md'
planted "a non-literal include in brackets fails as unresolved" \
    'pub static X: &str = include_str![concat!(
    env!("CARGO_MANIFEST_DIR"), "/x"
)];' \
    "" fail "$LIB:1:" "unresolved include"

# A ../ segment anywhere in a literal is normalized, not only a leading one.
planted "a manifest-dir concat through ../ into docs/ fails" \
    'pub fn f() { let _ = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/x.md")); }' \
    "" fail "$LIB:1:" '"/../../docs/x.md" -> docs/x.md'
planted "a format! path through ../ to README.md fails" \
    'pub fn f() -> String { format!("{}/../../README.md", env!("CARGO_MANIFEST_DIR")) }' \
    "" fail "$LIB:1:" '"{}/../../README.md" -> README.md'

# A ../ segment inside a docs-only directory resolves within it: the prefix
# before it is kept, not stripped along with it.
for dir in docs process-docs .github packaging .copr completions; do
    planted "an internal parent segment stays under $dir/" \
        "pub fn f() { let _ = repo_root().join(\"$dir/sub/../input.json\"); }" \
        "" fail "$LIB:1:" "-> $dir/input.json"
done

# A leading / with no ../ after it is stripped too.
planted "a manifest-dir concat of /docs/ fails" \
    'pub fn f() { let _ = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/x.md")); }' \
    "" fail "$LIB:1:" '"/docs/x.md" -> docs/x.md'

# The scan fails, rather than passing, when it cannot look.
new_tree 'pub fn f() {}'
INJECT_LS_FILES_FAILURE=1
expect_scan "an injected git ls-files failure fails" "$TREE" "" fail \
    "git ls-files failed"
unset INJECT_LS_FILES_FAILURE
new_tree 'pub fn f() {}'
command git -C "$TREE" rm -q .githooks/docs-only-paths
expect_scan "a missing docs-only list fails" "$TREE" "" fail \
    "cannot read a pattern from"
new_tree 'pub fn f() {}'
printf '# only a comment\n\n' > "$TREE/.githooks/docs-only-paths"
expect_scan "a docs-only list with no pattern fails" "$TREE" "" fail \
    "cannot read a pattern from"
new_tree ''
expect_scan "a tree with no Rust source fails" "$TREE" "" fail \
    "no tracked crates/**/*.rs file"

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
