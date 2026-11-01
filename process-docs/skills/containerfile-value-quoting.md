---
name: containerfile-value-quoting
description: Three interpolation contexts in the Containerfile renderer need three different treatments — quote, sanitize, or reject — the quoting has to land before any predicate is narrowed, and a host-sourced value that reaches a command is proved by a newtype rather than by a check at the site.
---

# Containerfile Value Quoting

Every snapshot-derived value that reaches
`crates/pipeline/src/render/language_packages.rs` lands in one of three
contexts. The context decides the treatment, and getting the pairing wrong
has produced both over-rejection of legal paths and live command injection.

## The Three Contexts

| Context | Example site | Treatment | Predicate that matters |
|---|---|---|---|
| Inside single quotes in a command | `RUN python3 -m venv '{abs_path}'` | quote at the site | `'` and `\n` only — everything else is inert inside the quotes |
| Bare token in a command | `RUN npm install -g {pkg_list}` | reject unsafe tokens, or quote them | the full metacharacter set, and a blacklist here is structurally incomplete |
| Comment text | `# npm global packages: {prefix}` | `sanitize_for_comment()` | `\n` only — it closes the comment |

`UNSAFE_SHELL_CHARS` is currently one widened set serving all three, which
is why it over-rejects legal quoted paths: `/opt/app$canary/venv` is
perfectly safe inside single quotes and is rejected anyway.

## Comment Sites Sanitize, They Never Reject

A value that only labels a comment cannot execute — until a newline closes
the comment and the rest of the value becomes an instruction:

```
# npm global packages: /usr/local/lib/node_modules
RUN echo PWNED (detected via npm list -g)
```

Both the npm global `prefix` and `pinned_package_list()`'s output shipped
this way before 2026-08-15. Route comment interpolations through
`sanitize_for_comment()`. Rejection is the wrong tool there: it costs
legitimate values a rendered section for no safety gain.

Commented-out `RUN` lines are comment sites, but the line exists to be
uncommented, so the value still has to be plausible as a command argument
after the `#` comes off.

## The Ordering Rule

**A quoted value at every site is a prerequisite for narrowing the
predicate, never a follow-up to it.**

`render_pip_item` derives `venv_name` from the path's final component and
interpolates it into `/tmp/<name>-requirements.txt`, which is a separate
token from the venv path and gets none of its quoting. Narrowing
`UNSAFE_SHELL_CHARS` to `'` and `\n` while those sites are bare turns a
visible over-rejection into command substitution in a `RUN`:

```
RUN python3 -m venv '/opt/app/ve$(id)nv' \
    && '/opt/app/ve$(id)nv'/bin/pip install -r /tmp/ve$(id)nv-requirements.txt
```

The widened predicate is the only thing holding that back today. The
`venv_name` sites were quoted first (2026-08-15) so the narrowing can
follow safely.

The same rule covers the npm global package list, the one genuinely
unquoted command site. Quote the tokens there before narrowing, or the
blacklist stays load-bearing — and it already under-rejects `>`, `<`, `(`,
`*`, `{`, and a bare space.

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
