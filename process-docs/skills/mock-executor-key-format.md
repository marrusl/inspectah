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

**5. Links are not followed on reads.** `RealExecutor::read_file` and
`file_exists` follow symlinks; `MockExecutor`'s do not, and
`resolve_final_target` succeeds on a link to an unregistered path. A
fixture that registers a link must also register, at the link's own
path, the content a real read returns there (`""` for `/dev/null`), or
the inspector sees a missing file where a real host has content. Use
`with_dangling_link()` for a link whose target is missing: it makes
`resolve_final_target` fail with `NotFound`, as `canonicalize` does. The
sysctl harness (`crates/cli/tests/support/sysctl_hosts.rs`,
`SysctlHost::link`) applies this rule for every link it is given,
and for a link to a directory it registers the target's listing and
files under the link's path too.
`file_exists` is true for a registered file or a registered directory
(`with_dir`), as `Path::exists` is for both on a real host.

**6. Link checks behave like the real syscalls.** `resolve_final_target`
follows a link on any component, so `/etc/sysctl.d/a.conf` under a
linked `/etc/sysctl.d` resolves into the link's target, as
`canonicalize` does. `read_link` resolves every component but the
last, as `readlink(2)` does, and fails with `InvalidInput` (`EINVAL`)
on a registered path that is not a link and `NotFound` on an
unregistered one. A collector that must not read an entry before it
knows where the entry resolves (the sysctl reader) treats only
`InvalidInput` as "not a link"; use `with_link_error()` to inject any
other failure. A resolution through a `with_link_error()` path fails
with the same kind, as `canonicalize` does, so it also injects a
failure partway along a chain. A loop fails with `ELOOP`
(`raw_os_error()`), as `canonicalize` does. Callers that only ask
`is_ok()` are unaffected.

**7. `read_dir` returns a partial listing.** Both executors' `read_dir`
drop an entry the directory iterator fails on and return the rest,
which inspectors that tolerate a missing file rely on. A caller whose
answer is wrong when one entry is missing (the sysctl reader: the
missing file could set or cancel any key) uses `read_dir_strict`, which
fails instead. `with_dir_iteration_error()` makes a mock directory list
its entries and then fail: `read_dir` hides it, `read_dir_strict`
returns it. A `with_dir_error()` test does not cover this case: it
fails the open, which both methods already report.

**8. The RPM ownership query key.** The RPM inspector lists owned paths
with `rpm -qa --queryformat @@%{NAME}\n[%{FILENAMES}\n]` (the `\n` is
the two characters backslash and `n`). A mock registering an older
format gets exit 127, and the only symptom is the warning "rpm file
ownership query returned no data", so every path looks unowned.

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
