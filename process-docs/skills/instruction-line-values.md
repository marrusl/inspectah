---
name: instruction-line-values
description: A snapshot value that reaches an instruction, directive, key or command line in any file the renderer emits is proved by a type or one validator before it is interpolated, or escaped by a real serializer where the format has one; a refused value gets a FIXME. Covers the Containerfile, the flatpak systemd unit, both kickstarts, the users blueprint TOML and the sysctl drop-in.
---

# Snapshot Values on Instruction Lines

The comment guard (`render::comment::join_lines`, see
[containerfile-value-quoting](containerfile-value-quoting.md)) keeps a
value inside a comment line. It never touches an instruction line, and it
must not: escaping a value there would silently change the command. So a
value that reaches a non-comment line needs its own answer, and that
answer is decided before the renderer interpolates it.

The threat is the same in every format: a line terminator in the value
ends the line, and what follows is a line the snapshot chose. In the
Containerfile it runs as root at build; in the flatpak unit, the sysctl
drop-in and the kickstarts it runs or applies at boot or install. A
Linux file name can hold a newline, so a host scan alone can deliver one
to a path site; an imported or edited snapshot can deliver one anywhere.

## The Rule

**A renderer interpolates into a non-comment line only a value that was
proved first, by a type with one constructor or by one named validator.**
Which proof depends on how the format reads the value:

| Format and site | Value | Proof | Home |
|---|---|---|---|
| Containerfile `COPY` argument (unmanaged path, config root) | path | `CopyToken`: no line break, control, whitespace, `\` or `$` | `render::safety` |
| Staged path under `config/`, `drop-ins/`, `env-files/` | relative path | `validate_path`: no NUL, line break, control, `..` or leading `/` | `render::configtree` |
| Containerfile `RUN ln -sf '<target>' '<path>'` | path, target | `validate_path` on the path, `has_line_break_or_control` on the target, then single-quoted | `render::unmanaged` |
| Containerfile `FROM` | image reference | `BareWord::parse_image_ref` | `render::safety` |
| Containerfile `dnf group install "<name>"` | group name | `double_quoted_word_refusal` | `render::safety` |
| systemd `ExecStart=` / `ExecStartPre=` (flatpak unit) | remote, app id, branch, URL | `BareWord::parse_name`, `BareWord::parse_url` | `render::safety` |
| Kickstart `network --device=` | connection name | `BareWord::parse_name` | `render::kickstart` |
| Kickstart `%post` `/etc/hosts` line | hosts line | `has_line_break_or_non_tab_control`, then `single_quote` | `render::kickstart` |
| Users kickstart `group`, `user` | names, groups, home, shell, hash | `AccountName`, `ExportPath`, `CryptHash` through `useradd_refusal`, the Containerfile's own check | `render::users` |
| Users kickstart `sshkey "<key>"` | key | `double_quoted_word_refusal` | `render::users` |
| Users blueprint TOML | every string | `toml_string` (escaped, not refused) | `render::users` |
| sysctl drop-in `<key> = <value>` | key, value | `sysctl_refusal` | `render::configtree` |

Every other Containerfile command site was already guarded:
`sanitize_shell_value` (repo ids, package names, module streams,
versionlock patterns, unit names, SELinux booleans and ports, GPG key
paths), `is_valid_tuned_profile`, the SELinux operand checks, `StagedName`,
`ExportPath`/`ExportToken`/`AccountName`/`CryptHash`, and the
language-package predicate. `sanitize_shell_value` refuses every line
terminator because its regex's `\s` is Unicode-aware; a unit test pins
that, since every site it guards relies on it.

## Choosing Refuse, Quote or Escape

- **Refuse** where the format has no quoting the code can prove, or where
  quoting would depend on a parser's behavior the code cannot show:
  `COPY` arguments, `FROM`, systemd command lines, kickstart option
  values, sysctl keys. Prove into an allowlist. This is why the flatpak
  unit refuses rather than escaping `%` as `%%`: refusal needs no claim
  about how systemd unquotes.
- **Quote** where the format has a quoting rule the code fully controls and
  legitimate values need the characters: `/etc/hosts` lines hold spaces
  and tabs, so they are single-quoted for the `%post` shell. A line break
  is still refused first, because inside the quotes the shell keeps it but
  the kickstart parser reads the script line by line, and a `%end` line
  closes the section.
- **Escape with a serializer** where the format defines escaping and the
  value should survive intact: TOML basic strings. `toml_string` escapes
  `"`, `\` and every control and line terminator as `\uXXXX`, so the
  value round-trips through a TOML parser unchanged.

A double-quoted shell word (group names, SSH keys) sits between: the
quotes are kept because legitimate values hold spaces, and only what acts
inside double quotes (`"`, `\`, `$`, the backquote, line breaks, controls)
is refused.

## What a Refusal Renders

A refused value gets a `# FIXME` naming it (escaped through the join, or
at the site in the string-built flatpak unit), never a rewritten value.
The cost follows the existing rules in containerfile-value-quoting.md:
a user refused in one field loses the whole `user` line and its keys in
the kickstart as in the Containerfile; a refused hash costs only the
password; a refused group's members install one by one.

**The stager and the renderer ask the same question.** A config file
`validate_path` refuses is never written, and the Containerfile's config
section asks `validate_path` too, both for its FIXME and when it derives
roots from the snapshot, so it never names a directory nobody staged.

**The unit of work is the emitted line.** A `COPY` of a directory names
the directory, not its files, so it proves the directory; a file inside
it with an odd basename is carried by that `COPY` with no line naming
it. Enumerate every interpolation on a line before calling it closed.

## Out of Scope

Verbatim snapshot file contents are copied, not built: config files,
repo files, drop-ins, quadlet and timer units, manifests, env files and
the `authorized_keys` content. A snapshot that controls them controls
those files outright, so line structure inside them is not this rule's
concern. The Markdown and HTML reports are display only.

## Testing

`crates/pipeline/tests/instruction_injection_test.rs` drives each site
through the real render path, one poisoned field per row, for every
character in `LINE_TERMINATORS` plus CRLF, and, where a word or quote
boundary matters, space, quotes, `$(`, backquote, `%` and backslash. The
assertion is on the output: no line that is not a comment carries the
payload, and a FIXME shows the row reached its site. The `/etc/hosts`
line is run through `sh` and must print back unchanged; the blueprint is
parsed with the `toml` crate and must hold exactly the expected keys,
with the hostile value carried in its own field. Prove each new row by
mutation: neuter the guard and that row, specifically, must go red.
