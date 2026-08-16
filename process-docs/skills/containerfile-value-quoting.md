---
name: containerfile-value-quoting
description: Three interpolation contexts in the Containerfile renderer need three different treatments — quote, sanitize, or reject — and the quoting has to land before any predicate is narrowed.
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
