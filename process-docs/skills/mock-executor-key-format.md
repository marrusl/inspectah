---
name: mock-executor-key-format
description: MockExecutor command lookup uses a specific key format that silently returns exit 127 on mismatch.
---

# MockExecutor Command Key Format

`MockExecutor` in `crates/collect/src/executor/mock.rs` builds lookup
keys by joining the command and arguments with a single space:

```rust
let key = if args.is_empty() {
    cmd.to_string()
} else {
    format!("{} {}", cmd, args.join(" "))
};
```

If no registered key matches, the mock returns `exit_code: 127` (command
not found) with no stdout. This is the most common source of broken tests
when adding or modifying inspector command calls.

### Common Pitfalls

**1. Whitespace mismatch.** The inspector calls `executor.run("rpm",
&["-qa", "--queryformat", "%{EPOCH}..."])` which produces the key
`rpm -qa --queryformat %{EPOCH}...`. If your `with_command()` string
has extra spaces or different argument order, the lookup silently fails.

**2. Forgetting file/dir registration.** `MockExecutor` has separate
maps for commands (`with_command`), files (`with_file`), directories
(`with_dir`), and symlinks (`with_link`). An inspector that calls
`executor.read_file()` needs `with_file()`, not `with_command()`.
Misusing these returns `io::ErrorKind::NotFound` -- which looks like
a "file doesn't exist on this host" rather than a test setup bug.

**3. Prefix matching is fallback-only.** `with_command_prefix()` only
triggers after exact match fails. If you register both an exact match
and a prefix that covers it, the exact match always wins. Prefix
matching uses `starts_with`, so shorter prefixes are greedier.

**4. Error injection priority.** `with_file_error()` and
`with_dir_error()` take priority over registered content. If you
register both `with_file("/etc/foo", "content")` and
`with_file_error("/etc/foo", PermissionDenied)`, the error wins.

### Debugging Failed Lookups

Use `command_log()` to see every command the inspector actually invoked:

```rust
let mock = MockExecutor::new().with_command("rpm -qa", ...);
// ... run inspector ...
let log = mock.command_log();
// log shows the actual keys the inspector tried to look up
```

Compare the log entries against your registered keys to find the
mismatch.

## Why This Matters

A missing command registration does not panic or error at the mock
level -- it returns a plausible "command not found" result. The
inspector then records a graceful failure or skip, and the test passes
with incomplete data. You end up testing the wrong scenario.

## See Also

- `crates/collect/src/executor/mock.rs` -- full implementation
- `crates/collect/src/executor/real.rs` -- real executor for comparison
- `crates/core/src/traits/executor.rs` -- `Executor` trait definition
