---
title: Customize Output
parent: How-To Guides
nav_order: 4
---

# Customize Output

inspectah offers several flags to control how scan progress is displayed,
how much detail is shown, and how sensitive data is handled. This guide
covers all the output customization options.

## Progress display modes

The `--progress` flag controls how scan progress appears in your terminal.
inspectah auto-detects the best mode based on your environment, but you can
override it.

### Rich mode (default for terminals)

```bash
sudo inspectah scan --progress rich
```

Rich mode renders a block-redraw checklist with spinners. Each inspector
gets a live status line that updates in place. This is the default when
your terminal supports it.

Rich mode requires a sufficiently wide terminal. On narrow terminals,
inspectah falls back to plain mode automatically.

### Plain mode

```bash
sudo inspectah scan --progress plain
```

Plain mode prints append-only lines with Unicode symbols. Each step writes
a new line rather than redrawing in place. This preserves full scrollback
history, which is useful when you want to review the complete scan log
after it finishes.

### Flat mode (CI and pipes)

```bash
sudo inspectah scan --progress flat
```

Flat mode prints numbered sequential lines with no ANSI escape codes. This
is the right choice for CI pipelines, log files, or any context where the
output is piped rather than displayed in a terminal. inspectah selects this
automatically when it detects a non-TTY environment.

## Verbosity

### Verbose output

```bash
sudo inspectah scan -v
```

Verbose mode (`-v` or `--verbose`) shows sub-step detail for all inspectors,
including fast ones that normally complete silently. Use this when you want
to see exactly what inspectah is doing at each stage.

### Quiet output

```bash
sudo inspectah scan -q
```

Quiet mode (`-q` or `--quiet`) suppresses the scan progress checklist
entirely. The completion summary still prints so you know the scan finished
and where the output was written. Combine with `--progress flat` for minimal
CI output:

```bash
sudo inspectah scan -q --progress flat -o /tmp/scan.tar.gz
```

## Output path

By default, inspectah writes the scan tarball to the current directory. Use
`-o` or `--output` to specify a different path:

```bash
sudo inspectah scan -o /tmp/migration-scan.tar.gz
```

When used with `--inspect-only`, the output path is treated as a directory
for the JSON snapshot rather than a tarball filename.

## JSON-only output

To get the raw scan data as JSON without producing a tarball:

```bash
sudo inspectah scan --inspect-only
```

This writes a JSON snapshot and exits without generating rendered artifacts
or a tarball. The JSON is pretty-printed for readability.

Write to a specific directory:

```bash
sudo inspectah scan --inspect-only -o ./snapshot-dir/
```

Or let it write to stdout by omitting `-o` (useful for piping to other tools):

```bash
sudo inspectah scan --inspect-only | jq '.packages | length'
```

## Sensitive data handling

By default, inspectah redacts sensitive data from scan output. Password
hashes are replaced with placeholders and SSH key contents are summarized
rather than included verbatim. Three flags control this behavior.

### Preserve password hashes

```bash
sudo inspectah scan --preserve-password-hashes
```

Retains the actual password hash strings for users whose password status is
`password_set`. Without this flag, hashes are replaced with redaction
placeholders.

### Preserve SSH keys

```bash
sudo inspectah scan --preserve-ssh-keys
```

Includes the full content of each user's `authorized_keys` file. Without
this flag, SSH keys are summarized (key count and types) but the actual
key material is omitted.

### Acknowledge sensitive data

```bash
sudo inspectah scan --preserve-password-hashes --preserve-ssh-keys --ack-sensitive
```

When either `--preserve-password-hashes` or `--preserve-ssh-keys` is used,
inspectah requires `--ack-sensitive` as an explicit confirmation that the
resulting snapshot contains sensitive data. This prevents accidental export
of credentials. The long form `--acknowledge-sensitive` is accepted as an
alias.

The acknowledge flag is only needed when a preserve flag is active. A
standard scan (with default redaction) does not require it.

### Combining sensitive data flags

A common pattern for environments where you need full-fidelity user data:

```bash
sudo inspectah scan \
  --preserve-password-hashes \
  --preserve-ssh-keys \
  --ack-sensitive \
  -o /secure/path/scan-output.tar.gz
```

Handle the resulting tarball with the same care as any file containing
credentials. The refine UI will show the sensitive data status so
reviewers know what level of redaction was applied.
