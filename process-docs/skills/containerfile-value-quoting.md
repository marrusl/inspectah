---
name: containerfile-value-quoting
description: Three interpolation contexts in the Containerfile renderer need three different treatments (quote, escape, or reject); comment lines are escaped where the file is joined, the quoting has to land before any predicate is narrowed, and a host-sourced value that reaches a command is proved by a newtype rather than by a check at the site.
---

# Containerfile Value Quoting

Every snapshot-derived value that reaches
`crates/pipeline/src/render/language_packages.rs` lands in one of three
contexts. The context decides the treatment, and getting the pairing wrong
has produced both over-rejection of legal paths and live command injection.

## The Three Contexts

| Context | Example site | Treatment | Predicate that matters |
|---|---|---|---|
| Inside single quotes in a command | `RUN python3 -m venv '{abs_path}'` | quote at the site | `'` and the line terminators only: everything else is inert inside the quotes |
| Bare token in a command | none left: the npm global list was the last, now `RUN npm install -g -- '{name}@{version}' ...` | quote it, never blacklist; put `--` before a token list that could hold a word starting with `-` | a blacklist here is structurally incomplete, so this row should stay empty |
| Comment text | `# npm global packages: {prefix}` | escaped for you by `comment::join_lines()`, the join every line-vector renderer goes through (the string-built files in `configtree.rs` escape at the site or carry fixed text); `sanitize_for_comment()` per value where the quotes should double | `\n` closes the comment; every other Unicode mandatory line break (`\r`, VT, FF, U+0085, U+2028, U+2029) is escaped too, since a viewer may break the line there whether or not the build parser does, and so is every other control but tab, since ESC starts a terminal escape sequence |

`contains_unsafe_chars` refuses `'` and every character in
`LINE_TERMINATORS`, one check serving every command site, which is
correct only because every command site single-quotes. A new
interpolation added bare inherits a predicate that admits `;`, `$` and `>`.
The quotes make a carriage return or U+2028 inert to the shell but not to
a terminal, which breaks the line there, so every terminator is refused,
not just the newline (2026-11-10).

## Comment Sites Sanitize, They Never Reject

A value that only labels a comment cannot execute, until a newline closes
the comment and the rest of the value becomes an instruction:

```
# npm global packages: /usr/local/lib/node_modules
RUN echo PWNED (detected via npm list -g)
```

Both the npm global `prefix` and `pinned_package_list()`'s output shipped
this way before 2026-08-15. Rejection is the wrong tool there: it costs
legitimate values a rendered section for no safety gain. Escape instead.

## Comment Lines Are Escaped Where the File Is Joined

**Build a rendered file as a `Vec<String>`, one element per line, and
join it with `render::comment::join_lines()`.** The join runs
`escape_comment_text()` over every element whose first character after
spaces and tabs is `#`, so a snapshot value in a comment line cannot end
it, whichever site built the line and whether or not that site escaped.
The Containerfile, the `/var` tmpfiles.d fragment, the kickstart
suggestion, the users kickstart and blueprint fragments and the sysctl
drop-in all join there. The `#`-comment files built with `push_str` in `configtree.rs`
do not; see the last item below.

Per-site escaping was the rule until 2026-11-10 and it kept failing: a
sweep after the `# Omitted:` fix found raw values in more than twenty
comment sites (package names in `# TODO:`, group names, unmanaged
paths, low-confidence language paths, static routes, `/etc/hosts`
additions, proxy lines, secret paths and remediations, non-RPM binary
paths and notes, five kickstart comments). A few hundred comment lines
come from plain `format!("# ...")`, so only the place every line passes
through can guard a site that does not exist yet.

What the join does not do:

- **It never touches an instruction line.** A value reaching a command
  is refused or proved before it gets there (the sections below, and
  [instruction-line-values](instruction-line-values.md) for every
  other generated file): escaping it would silently change the command.
- **It sees one element as one line.** An element that starts with a
  terminator is not a comment and is left as it is, so never push a
  comment as part of a multi-line element. Debug builds assert this: a
  non-comment element holding a `#` line after a terminator panics in
  `join_lines`, so a test that renders the branch finds it. Do not "fix"
  the join by splitting elements on terminators first: a comment
  carrying `\nRUN x` would split into an uncommented `RUN x` line and
  escape nothing.
- **It does not cover a file built another way.** Two `#`-comment
  files in `configtree.rs` are assembled with `push_str`. The flatpak
  provisioning unit escapes its value-carrying comments (the unknown-URL
  remote and the refusal FIXMEs) at the site. The kernel arguments TOML
  writes fixed comment text only. The sysctl drop-in is a line vector
  joined here since its refusal FIXMEs carry the key. A new file builds a
  line vector and joins it here, or it has no guard.
- **It does nothing for an instruction line.** A value on a command,
  directive or key line is proved before it is interpolated; see
  [instruction-line-values](instruction-line-values.md).

Escaping a value at its site as well is harmless: the escaped text holds
no character the join changes. `sanitize_for_comment()` still has a
job, doubling quotes where a comment wraps a value in them.

A FIXME is a comment site too, and the one most often missed: it is
written *because* the value failed a check, so it is the line most likely
to carry the newline. The Scheduled Tasks section shipped its inventory
comments and its unsafe-name FIXME raw until 2026-11-09.

**Escape shared wording at the emit site, not where it is worded.** A
comment built from a describe-style function that other surfaces also
call, such as `OmissionReason::describe` (the `# Omitted:` line and
refine's Omitted Services subtitle), is escaped on the finished
Containerfile line, never inside the function: refine shows the raw
value on its own surface, and a Containerfile escape baked into the
shared text would show there as a literal `\n`. The join escapes that
line already; the site also calls `escape_comment_text()` on the whole
line because `ServiceRenderPlan.lines` is public. Do not use
`sanitize_for_comment()` there, whose quote doubling would turn the fixed
`package 'nginx'` wording into `package ''nginx''`. The owner in that
line shipped raw until 2026-11-10.

**A renderer that references a staged file asks the stager's question.**
When the export refuses to write a file named by a snapshot value, every
Containerfile line naming that file must make the same decision from the
same function, or it enables or copies something nobody wrote. Timer
units use `configtree::staged_unit_file` on both sides; SSH keys use
`is_single_normal_component` on both sides.

Commented-out `RUN` lines are comment sites, but the line exists to be
uncommented, so the value still has to be plausible as a command argument
after the `#` comes off.

## The Ordering Rule

**A quoted value at every site is a prerequisite for narrowing the
predicate, never a follow-up to it.**

`render_pip_item` derives `venv_name` from the path's final component and
interpolates it into `/tmp/<name>-requirements.txt`, which is a separate
token from the venv path and gets none of its quoting. Narrowing the
path predicate to `'` and `\n` while those sites are bare turns a
visible over-rejection into command substitution in a `RUN`:

```
RUN python3 -m venv '/opt/app/ve$(id)nv' \
    && '/opt/app/ve$(id)nv'/bin/pip install -r /tmp/ve$(id)nv-requirements.txt
```

The `venv_name` sites and the npm global tokens were both quoted before
the predicate narrowed, and the narrowing landed in the same commit as the
npm quoting.

**The trap when narrowing:** two tests go red together, a pip path test
meant to be inverted and an npm test that looks like the same class. The
npm one guarded the bare site, so inverting it to match the narrower
predicate ships `RUN npm install -g pm2;id` with a green suite. Replace a
predicate-behaviour test with an assertion on the rendered output
(`active_lines()` holds only the quoted install), then prove it by
mutation: revert the quoting with the narrow predicate in place and the
test must fail.

## Host-Sourced Values Get a Type, Not a Site Check

Two instances of the same defect have shipped now: an RPM filename and a
`/var` directory path, each interpolated into a shell-form `RUN` that
`/bin/sh -c` runs as root at image build time. Both were closed the same
way, and the next one should be.

| Value | Type | Home |
|---|---|---|
| Staged RPM basename | `StagedName` | `inspectah_core::rpm_filename` |
| `/var` directory path | `ExportPath` | `inspectah_pipeline::render::safety` |
| Mode, account name, group name | `ExportToken` | `inspectah_pipeline::render::safety` |
| Account or group name on a `useradd`, `groupadd` or `chpasswd` line | `AccountName` (`ExportToken` plus no leading `-`, `.`, `..` or all-digit name) | `inspectah_pipeline::render::safety` |
| Password hash in `chpasswd -e` input | `CryptHash` | `inspectah_pipeline::render::safety` |

Each is an ASCII allowlist plus a newtype with no other constructor, and
the emitting function takes the type rather than a `&str`. The guarantee
belongs to the type, so a new emission site cannot forget the check; a
site check closes one site, the type closes the class.

**A value with two emission surfaces is proved once for both, or the fix
is half a fix.** `ExportPath` guards the tmpfiles.d `d` entry as well as
the `RUN`: those two are complementary arms of the same `Provision`
outcome, split only on whether the scan captured a mode. tmpfiles.d fields
are whitespace-separated and `#` starts a comment there, so an unproved
path adds a directive systemd applies at boot. Finding the shell half and
stopping would have left the other arm open.

**And the unit of work is the emitted line, not the value you were sent
to fix.** `d {path} {mode} {owner} {group} -` has four host-sourced
fields. The first pass proved the path, because the path is what the
finding named, and left the other three carrying the same defect; a
review caught it one commit later. Before calling an emission site
closed, enumerate every interpolation on the line and name what proves
each.

The users renderer is the second worked example of that rule. Fixing
the username left `-G`, `-s` and `-d` on the same `useradd` line, and the
password hash in `chpasswd`, unproved: a home of
`/home/alice; id > /tmp/x #` ran `id`. Every field there is now proved
(`AccountName`, `ExportPath`, `CryptHash`) and the paths are quoted too.
A refused name, group, home or shell costs the whole user a FIXME,
because dropping one field creates a user unlike the host's; a refused
hash costs only the password.

**What a refusal costs decides the answer, and the caller decides it, not
the type.** Three answers, all in use on that one line:

- **Degrade** where a safe stand-in means the same thing. A refused
  account name falls back to the numeric uid or gid, which provisions
  identical ownership and is already what the renderer emits when `stat`
  reported no name.
- **Refuse the line** where no stand-in is still the host's value. A
  refused mode does *not* fall back to `0755`. That silently changes the
  artifact, which is the failure mode this project keeps rediscovering.
- **Escape** where the value only labels. The named creator goes through
  `sanitize_for_comment`, because refusing it would cost the operator the
  reason for a FIXME they are being asked to act on.

Rejection stays rejection in the first two: a refused value gets a FIXME
naming the directory or package, never a sanitised rewrite that names
something else.

**Fix the parse as well as the emission when the parse is why the value
is unbounded.** The strongest vector on that line needed no crafted
tarball. `discover_var_directories` split `stat -c "%04a %u %g %U %G"`
with `splitn(5, ' ')`, so the last field took the whole remainder of the
output, newlines included, straight into `group_name`. A renderer
allowlist stops it reaching the build; requiring exactly five
whitespace-separated fields stops it entering the snapshot at all, which
matters because four other surfaces read that field.

## Testing These

Assert on the **rendered output**, not on a predicate's return value. A
guard that returns `false` proves nothing about what a build executes, and
these are injection fixes where the output is the contract. The useful
assertion is that the only active (non-comment, non-blank) lines are the
instructions the renderer is supposed to emit:

```rust
fn active_lines(output: &str) -> Vec<&str> {
    output
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .collect()
}
```

For quoting, the precise negative is the unquoted form with its leading
separator — `" /tmp/{name}"` rather than `"/tmp/{name}"`, since the quoted
rendering contains the latter as a substring.

**One poisoned fixture per field, and prove it by mutation.** The first
version of `var_dir_hostile_fields_add_no_directive` gave `owner_name`
and `group_name` the same bad value, which reads as covering both and
covers one: the refused owner falls through to the numeric branch, that
branch emits a numeric group as well, and the group's own check is never
reached. Deleting group validation left the test green. A fixture that
poisons two fields at once only reaches the first one the code rejects.

So the check on a test written to close an injection finding is not that
it passes. It is: **remove the guard it is supposed to be testing and
confirm this test, specifically, goes red.** Two mutations are worth
running, because they fail in different places and only the second
proves the assertion earns its keep:

- Remove the validation at one call site. A typed guard will often turn
  this into a panic in the constructor rather than a bad line, which
  still fails the test but tells you nothing about the assertion.
- Neuter the allowlist predicate itself. Now the bad value reaches the
  output and the active-line comparison is what has to catch it.
