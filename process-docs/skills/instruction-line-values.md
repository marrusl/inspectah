---
name: instruction-line-values
description: A snapshot value that reaches an instruction, directive, key or command line in any file the renderer emits is proved by a type or one validator before it is interpolated, or escaped by a real serializer where the format has one; a refused value gets a FIXME. An operand of a RUN command also may not start with `-` or `#`. Covers the Containerfile, the flatpak systemd unit, both kickstarts, the users blueprint TOML and the sysctl drop-in; sysctl values are trusted host configuration.
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
| Containerfile `COPY` argument (unmanaged path, config root) | path | `CopyToken`: no line break, control, whitespace, `\`, `$`, `*`, `?` or `[` | `render::safety` |
| Staged path under `config/`, `drop-ins/`, `env-files/` | relative path | `validate_path`: no NUL, line break, control, `..` or leading `/` | `render::configtree` |
| Containerfile `RUN ln -sf '<target>' '<path>'` | path, target | `validate_path` on the path, `has_line_break_or_control` on the target, then single-quoted | `render::unmanaged` |
| Containerfile `FROM` | image reference | `BareWord::parse_image_ref` | `render::safety` |
| Containerfile `dnf group install "<name>"` | group name | `double_quoted_word_refusal`, then `operand_refusal` | `render::safety` |
| Containerfile `RUN` operand (`dnf install`, `dnf module enable`, `dnf versionlock add`, `setsebool`, `semanage port`, `systemctl enable`/`disable`/`mask`, `rpm --import`) | package name, module name, stream and profiles, versionlock pattern, boolean name and value, port type, protocol and number, unit name | `shell_operand_refusal`: `sanitize_shell_value`, then `operand_refusal` (no leading `-` or `#`) | `render::safety` |
| systemd `ExecStart=` / `ExecStartPre=` (flatpak unit) | remote, app id, branch, URL | `BareWord::parse_name`, `BareWord::parse_url` | `render::safety` |
| Kickstart `network --device=` | connection name | `BareWord::parse_name` | `render::kickstart` |
| Kickstart `%post` `/etc/hosts` line | hosts line | `has_line_break_or_non_tab_control`, then `single_quote` | `render::kickstart` |
| Users kickstart `group`, `user` | names, groups, home, shell, hash | `AccountName`, `ExportPath`, `CryptHash` through `useradd_refusal`, the Containerfile's own check | `render::users` |
| Users kickstart `sshkey "<key>"` | key | `double_quoted_word_refusal` | `render::users` |
| Users blueprint TOML | group and user names, groups, home, shell, hash | the kickstart's proofs (`AccountName`, `useradd_refusal`, `CryptHash`), then `toml_string` | `render::users` |
| Users blueprint TOML | SSH key | `toml_string` (escaped, not refused) | `render::users` |
| sysctl drop-in `<key> = <value>` | key, value | `sysctl_refusal`: key `[A-Za-z0-9._/-]`, no leading `-` or `/`, no `..`; value no line break or control but tab | `render::configtree` |

Every other Containerfile command site was already guarded against line
terminators: `sanitize_shell_value` (repo ids, GPG key paths, and the
operand sites above), `is_valid_tuned_profile`, the SELinux fcontext
operand checks, `StagedName`, `ExportPath`/`ExportToken`/`AccountName`/
`CryptHash`, and the language-package predicate. `sanitize_shell_value`
refuses every line terminator because its regex's `\s` is Unicode-aware;
a unit test pins that, since every site it guards relies on it.

`sanitize_shell_value` is a line and shell-word guard, not an option
guard: it says nothing about where a word sits. A word in an operand
position that starts with `-` (`--nogpgcheck`) or `#` is refused and gets
a FIXME in place of its line, so a value is never rendered where it could
stand in for an option or a comment. The operand sites also ask
`operand_refusal`, through `shell_operand_refusal`. The `rpm --import`
path of a GPG key is an operand too: a key path that is not absolute is
refused with the GPG FIXME, which keeps it off both the `COPY` and the
import line. It is a
separate check because some values `sanitize_shell_value` guards sit after
`=` (`--enablerepo=<id>`, kernel arguments) or inside another word, where
a leading `-` is legitimate.

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
  is still refused first: the quoting is a shell rule, and a line break
  inside the quotes would still have started a new line in the kickstart
  file.
- **Escape with a serializer** where the format defines escaping and the
  value should survive intact: TOML basic strings. `toml_string` escapes
  `"`, `\` and every control and line terminator as `\uXXXX`, so the
  value round-trips through a TOML parser unchanged. Escaping keeps the
  file whole and says nothing about what its consumer does with the value,
  so a value another output proves (the blueprint's account names, home,
  shell and hash) is proved here too before it is escaped.

A double-quoted shell word (group names, SSH keys) sits between: the
quotes are kept because legitimate values hold spaces, and only what acts
inside double quotes (`"`, `\`, `$`, the backquote, line breaks, controls)
is refused.

## What a Refusal Renders

A refused value gets a `# FIXME` naming it (escaped through the join, or
at the site in the string-built flatpak unit), never a rewritten value.
The cost follows the existing rules in containerfile-value-quoting.md:
a user refused in one field loses the whole `user` line and its keys in
the kickstart and the blueprint as in the Containerfile, so the kickstart
and the blueprint carry the same users; a refused hash costs only the
password; a refused group's members install one by one. A refused
operand costs what its site already cost: the package, module, version
lock, boolean or port label is skipped, and the unit is left as the base
image has it.

**The stager and the renderer ask the same question.** The config tree
stager writes config files, repo files, GPG keys, firewall zones and
systemd drop-ins from one list, `configtree::staged_sources`, and skips a
path `validate_path` refuses. The Containerfile reports from
`configtree::unstaged_files`, the same list through the same check, so a
refused file is never dropped without a FIXME: the config section names
every kind but GPG keys, which the GPG section names itself so that no
`rpm --import` line names a key that was not staged. The config section
also asks `validate_path` when it derives roots from the snapshot, so it
never names a directory nobody staged. `.env` files go to `env-files/`,
which no `COPY` names, and a refused one is skipped without a FIXME.

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

**Sysctl keys and values are trusted host configuration** (Mark,
2026-11-11), the same trust boundary as the verbatim files above. They
are applied as the source host had them. The renderer guarantees only
that a value cannot add a line or a key; it does not judge what the
value does. For example, a `kernel.core_pattern` value that starts with
`|` names a helper the kernel runs as root on a crash, and it is carried
as it is: that is a legitimate host setting (systemd-coredump uses one),
and a snapshot that sets one is trusted to mean it. Do not add a value
refusal here without revisiting that decision.

## Testing

`crates/pipeline/tests/instruction_injection_test.rs` drives each site
through the real render path, one poisoned field per row, for every
character in `LINE_TERMINATORS` plus CRLF, and, where a word or quote
boundary matters, space, quotes, `$(`, backquote, `%` and backslash. The
assertion is on the output: no line that is not a comment carries the
payload, and a FIXME shows the row reached its site. The `/etc/hosts`
line is run through `sh` and must print back unchanged. The blueprint is
parsed with the `toml` crate: a refused field must add no key, leave a
FIXME, and leave the blueprint carrying the same users as the kickstart;
the SSH key must hold exactly the expected keys and carry the hostile
value back in its own field. The operand rows use `--nogpgcheck`,
`-oalice` and `#foo`, and there a comment is a line with `#` at the
margin, because an indented `#foo \` inside a joined `RUN` is what they
look for. Each stager kind has a row with a refused path that must reach
a FIXME. Prove each new row by mutation: neuter the guard and that row,
specifically, must go red.
