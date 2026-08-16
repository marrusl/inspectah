# RPM Output: Never Split Paths on Whitespace

`rpm` quotes nothing in its output and paths may contain spaces. Any
parser that recovers a path by splitting a line on whitespace is wrong,
including "take the first field," which looks safe because the path
comes first.

Ask `rpm` for one value per line instead:

```rust
// crates/collect/src/rpm_ownership.rs
const OWNED_PATHS_QUERY_FORMAT: &str = "[%{FILENAMES}\\n]";
exec.run("rpm", &["--query", "--all", "--qf", OWNED_PATHS_QUERY_FORMAT]);
```

The whole line is then the path. Do not `trim()` it either: leading and
trailing spaces are legal in a path.

## Why positional parsing of `--dump` cannot be fixed

`rpm --query --all --dump` emits:

```
PATH SIZE MTIME DIGEST MODE OWNER GROUP ISCONFIG ISDOC RDEV SYMLINK
```

Splitting on whitespace and taking field 0 truncates any owned path
containing a space. Counting ten fields from the right does not rescue
it: the trailing `SYMLINK` field is a link target, which can contain
spaces too. There is no delimiter that separates the ends reliably, so
`--dump` cannot be parsed positionally at all. Only ask for `--dump` if
something actually consumes the size/mode/digest columns.

## This is not a hypothetical

Firmware packages name files after the hardware they drive, spaces
included. `brcmfmac-firmware` on RHEL 10 owns:

```
/usr/lib/firmware/brcm/brcmfmac43241b4-sdio.Intel Corp.-VALLEYVIEW C0 PLATFORM.txt.xz
```

Under first-field parsing this truncated to `...-sdio.Intel`, so the real
path was missing from the owned set and the `/usr` walk reported the file
as unmanaged content for the user to act on. That host had 33 such paths
against 62 total reported `/usr` entries: 53 percent false positives, in
shipped code, because every unit test fixture used space-free paths.

**Test fixtures for anything that parses paths must include a path with
an embedded space.** That single omission is what let this ship.

## Where this applies

- `crates/collect/src/rpm_ownership.rs` — `build_rpm_owned_set`, consumed
  by the `/usr` walk (`inspectors/nonrpm.rs`) and by `/var` directory
  backing analysis (`inspectors/storage.rs`)

## Not affected

Call sites that pass a path *to* `rpm` and read only the exit code are
fine. `RealExecutor::run` builds a fixed argv via `Command::new().args()`
and never a shell string, so a space in an argument is not a delimiter.
`is_rpm_owned_path` and `filter_rpm_owned_gems_with_gemdir` in
`inspectors/nonrpm.rs` both work this way.

## Related

- [mock-executor-key-format](mock-executor-key-format.md) — the mock key
  is `cmd + " " + args.join(" ")`. Build it from the same constant the
  collector uses; a drifted key returns exit 127, which this collector
  turns into an empty owned set, which reports the entire tree as
  unmanaged.
- [rpm-ownership-vs-name-heuristic](rpm-ownership-vs-name-heuristic.md) —
  which ownership signal to trust.
