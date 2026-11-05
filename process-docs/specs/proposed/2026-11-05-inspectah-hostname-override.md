# `INSPECTAH_HOSTNAME` Override for `inspectah scan`

## Metadata

| Field | Value |
|---|---|
| Date | 2026-11-05 |
| Status | Proposed. Of fifteen decisions, ten were ratified by the project owner on 2026-11-05 and five remain the spec author's call. See § 14. |
| Scope | One environment variable, `INSPECTAH_HOSTNAME`, honored by `inspectah scan`. Its documentation and a CHANGELOG entry. Nothing else. |
| Backlog item | `inspectah-hostname-override` (inspectah-team `workflow/backlog/inspectah/open/`) |
| Verified | Code citations read at `04ec336f` (HEAD, 2026-11-05). Consumer claims read against the driftify working tree on 2026-11-05. Line numbers are a convenience; the symbol name is the identifier. |

**No code, documentation, or CHANGELOG entry was written to produce this file.**

## 1. Problem

`inspectah scan` has no way to tell it what host it is scanning. The snapshot's
host identity comes from two independent detections of the real machine, and
neither takes input:

- `crates/pipeline/src/collect.rs:216-225` reads `/etc/hostname` through the
  executor, trims it, and inserts `meta["hostname"]` only when the result is
  non-empty.
- `crates/cli/src/commands/scan.rs:208-217`, `get_hostname()`, runs the
  `hostname` command and falls back to `"unknown"`. Its value binds at
  `scan.rs:559`, prints as `Inspecting host {hostname}...` at `:560`, and
  becomes the tarball stamp through `get_output_stamp()` (`:839`,
  `crates/pipeline/src/render/tarball.rs:28-32`).

These two can already disagree, and on a DHCP-named host they do: the snapshot
carries no hostname at all while the tarball is named for the machine.

Anything that wants more than one host identity out of one machine therefore has
to change the machine. That is what driftify's fleet test does:
`sudo hostnamectl set-hostname` per profile with restores after the loop and in
an `EXIT` trap (`run-fleet-test.sh:55`, `:67`, `:77-87`). It needs distinct
identities because `aggregate` refuses a set with repeats, raising
`DuplicateHostname` as a hard error over `extract_hostname()`, which reads
`meta["hostname"]` (`crates/core/src/aggregate/validate.rs:86-102`, `:223-225`).
Snapshots with no hostname all extract to `<unknown>`, so three of them collide
on that string just as surely as three real duplicates would.

Renaming a machine to produce fixtures needs more privilege than the scan
itself, leaves the host wrong if the restore does not fire, and cannot work at
all where the hostname is not the caller's to change: a container, a CI runner,
a shared box. An environment variable costs almost nothing and removes all of
that. Beyond the fleet test it is useful anywhere a synthetic host identity is
wanted.

The consumer-side work is not automatic and is out of scope here:
`run-fleet-test.sh` prefers `./inspectah` over `$PATH` (`:40-49`) and reads no
version, so nothing in driftify flips when a release carries this. The script
edits, with line numbers, are in `driftify-adopt-inspectah-hostname-override`.
driftify's `design.md:583` describes the `hostnamectl` workaround accurately
today and will need an edit when the script changes.

## 2. Non-goals

- A `--hostname` flag. Additive later if wanted.
- Honoring the variable in `aggregate`, `refine`, or `build`.
- Changing how the hostname is detected when the variable is unset. The host
  with an empty `/etc/hostname` is a separate problem; see § 8.
- Carrying hostname provenance through `aggregate`.
- Any change to `crates/pipeline/src/orchestrate.rs::run_pipeline`, which
  already takes a `hostname` parameter and is not on the scan path.
- Any change to driftify.

## 3. Anti-goals

The outcomes that make this feature a failure even if the variable technically
works.

- **A snapshot and its tarball disagree about which host this is.** A tarball
  named `rhel9-20261105-120000.tar.gz` wrapping a snapshot that says `web-01`
  is worse than no override, because the operator now has two answers and no
  way to tell which one the next tool will believe.
- **A variable that is present in the inspectah process is silently ignored and
  the scan looks like it worked.** A fleet loop that silently produces three
  snapshots all claiming the real machine's name hits the duplicate-hostname
  gate much later, with an error naming a symptom and not the cause. The one
  route to it is a value that is set but not valid UTF-8, and the resolution
  rule below is typed so it cannot happen. A variable dropped before the
  process starts is a different problem and not this one: sudo's `env_reset`
  makes it the most likely field complaint, and nothing inside inspectah can
  tell it from a variable that was never set. It is an operational limitation,
  answered by the invocation forms in § 9 and the documentation § 10 requires,
  not by the implementation.
- **A reader of a single-host snapshot cannot tell a declared name from a
  detected one.** inspectah's output justifies a migration. A synthetic identity
  presented as a measured fact is a provenance defect, not a convenience.
- **The override quietly rewrites the value between the snapshot and a
  filename.** If the accepted set is wider than what `sanitize_hostname`
  preserves, the name in the snapshot and the name in the tarball differ with
  nothing saying so.

## 4. What else the code does today

**A third hostname read.** `collect/inspectors/subscription.rs:63-70` reads
`etc/hostname` joined onto `exec.host_root()`, rather than the bare path
`collect.rs` uses, into `SubscriptionSection::source_hostname`. Populated only
under `--preserve subscription`.

**`sanitize_hostname`** (`render/tarball.rs:18-25`) strips everything outside
`[^\w.-]` and falls back to `"unknown"`. The `regex` crate's `\w` is
Unicode-aware, so it preserves more than ASCII alphanumerics.

**`meta` is free-form.** It is `HashMap<String, serde_json::Value>` with
`#[serde(default)]` (`core/snapshot.rs:26-27`), there is no
`deny_unknown_fields`, every reader uses `.get()`, and the load gate matches
`schema_version` exactly, so adding a key costs no `SCHEMA_VERSION` bump.
`validate` (`pipeline/validate.rs:21-33`) compares `schema_version` and nothing
else, so it is not an ordering boundary here, and the redaction engine never
reads `meta` at all.

**The aggregate collapses `meta`.** `aggregate/mod.rs:190-201` unions every
input's `meta` first-writer-wins into one map for the whole fleet, and
`AggregateSnapshotMeta` (`core/types/aggregate.rs:39-48`) carries `hostnames`
and `host_count` and nothing about where a name came from, so any per-host meta
key survives the merge exactly once, attached to nothing in particular.

**Readers of `meta["hostname"]`**, found by searching for every read rather than
by trusting an enumeration. None parses the value, resolves it against DNS, or
compares it to the machine, which is why a straight substitution is safe:

| Site | Use |
|---|---|
| `render/report.rs:147-151` | HTML report header |
| `render/readme.rs:64-72` | `Hostname:` line, omitted when empty |
| `render/kickstart.rs:52-61` | a FIXME comment naming the source host |
| `aggregate/validate.rs::extract_hostname` (`:223-225`) | the accessor, falls back to `"<unknown>"` |
| `aggregate/validate.rs:86-102` | the duplicate-hostname gate, the point of this feature |
| `aggregate/validate.rs:134-140` | the `EmptySnapshot` error's label |
| `aggregate/mod.rs:53-64` | canonical host ordering, sorts by hostname |
| `aggregate/mod.rs:147-155` | subscription merge tiebreak, prefers `source_hostname` |
| `web/handlers.rs:165-170` and `:214` | refine `/api/health` |
| `web/handlers.rs:403-411` | refine export download filename, via `sanitize_hostname` |
| `render/audit.rs:137-143` | `aggregate_meta.hostnames`, a transitive reader |

The refine web UI displays the `/api/health` value and the aggregate hostname
list without parsing them, in `Sidebar.tsx` and `StatsBar.tsx`, both
display-only.

## 5. The resolution rule

A pure function over the raw value. It takes the value as a parameter, not from
the environment, so every case is unit-testable with no environment mutation.

```rust
/// Resolve the `INSPECTAH_HOSTNAME` override from its raw environment value.
///
/// `Ok(None)` means the variable is **not present**, and the detected hostname
/// applies. A value that is present but not valid UTF-8 is `Err(NotUtf8)`,
/// never `Ok(None)`.
fn resolve_hostname_override(
    raw: Option<&std::ffi::OsStr>,
) -> Result<Option<String>, HostnameOverrideError>;

enum HostnameOverrideError {
    /// Present, but not valid UTF-8.
    NotUtf8,
    /// Set, but empty or whitespace only.
    Empty,
    /// First character outside `[A-Za-z0-9_]`.
    InvalidFirstCharacter { ch: char },
    /// A later character outside `[A-Za-z0-9_.-]`.
    InvalidCharacter { ch: char },
    /// Longer than `MAX_HOSTNAME_BYTES` after trimming.
    TooLong { bytes: usize },
}
```

The single call site is `std::env::var_os("INSPECTAH_HOSTNAME")`, passed in as
`.as_deref()`. The parameter is `OsStr` because `std::env::var(..).ok()` maps
`VarError::NotUnicode` to `None`, which this function defines as "not present",
and that is anti-goal 2 reached through the type. There are five variants rather
than one `InvalidSyntax` because § 7 requires the message to name the broken
rule.

**The rule, applied in this order.** Reject a non-UTF-8 value first. Then trim
leading and trailing whitespace, then:

1. Reject an empty result.
2. Reject unless the first character is in `[A-Za-z0-9_]`.
3. Reject unless every remaining character is in `[A-Za-z0-9_.-]`.
4. Reject unless the byte length is at most `MAX_HOSTNAME_BYTES`, 253.

The order is load-bearing and testable. Syntax before length means the length
error only ever appears for an otherwise valid value, which makes it actionable.
First character before the rest means `-web/01` reports the leading hyphen,
which is the rule the operator has to fix first.

The accepted set is a strict subset of what `sanitize_hostname` preserves, so an
accepted value passes through the filename path byte for byte. It is looser than
RFC 1123, which forbids underscores and a leading digit, because `web_01` is a
reasonable synthetic identity and inspectah never resolves the name. The
first-character constraint exists for one reason: a value beginning with `-`
produces a tarball filename beginning with `-`, which any later command reads as
a flag. Interior dots are unlimited, so `a..b` is accepted and harmless: the
first-character rule means the value cannot be `.` or `..`, an embedded `..` is
one ordinary path component, and `validate_tarball_entry`
(`render/configtree.rs:46-58`) rejects `..` components for every archive member
regardless.

### 5.1 Input and output table

This table is the unit test suite. One case per row. Every row that resolves to
`Ok(Some(v))` additionally asserts the pass-through property; see § 11.

| Raw value | Result |
|---|---|
| `None` | `Ok(None)` |
| `Some("web-01")` | `Ok(Some("web-01"))` |
| `Some("  web-01\n")` | `Ok(Some("web-01"))` |
| `Some("web.01.example.com")` | `Ok(Some("web.01.example.com"))` |
| `Some("web_01")` | `Ok(Some("web_01"))` |
| `Some("_web01")` | `Ok(Some("_web01"))`, leading underscore accepted |
| `Some("01web")` | `Ok(Some("01web"))` |
| `Some("WEB-01")` | `Ok(Some("WEB-01"))`, no case folding |
| `Some("a")` | `Ok(Some("a"))`, single character |
| `Some("_")` | `Ok(Some("_"))`, single character, first-class only |
| `Some("")` | `Err(Empty)` |
| `Some("   ")` | `Err(Empty)` |
| `Some("\t\n")` | `Err(Empty)` |
| `Some("web 01")` | `Err(InvalidCharacter { ch: ' ' })`, trimming strips only the ends |
| `Some("web\n01")` | `Err(InvalidCharacter { ch: '\n' })`, interior newline |
| `Some("web/01")` | `Err(InvalidCharacter { ch: '/' })` |
| `Some("web:01")` | `Err(InvalidCharacter { ch: ':' })` |
| `Some("wéb-01")` | `Err(InvalidCharacter { ch: 'é' })`, non-ASCII but valid UTF-8 |
| `Some("-web01")` | `Err(InvalidFirstCharacter { ch: '-' })` |
| `Some(".web01")` | `Err(InvalidFirstCharacter { ch: '.' })` |
| `Some("-")` | `Err(InvalidFirstCharacter { ch: '-' })`, single character |
| `Some(".")` | `Err(InvalidFirstCharacter { ch: '.' })`, single character |
| `Some("-web/01")` | `Err(InvalidFirstCharacter { ch: '-' })`, **not** `InvalidCharacter { ch: '/' }` |
| `Some(OsStr::from_bytes(b"web\xff01"))` | `Err(NotUtf8)`, behind `#[cfg(unix)]` |
| 253 bytes of `a` | `Ok(Some(..))` |
| 254 bytes of `a` | `Err(TooLong { bytes: 254 })` |
| 253 bytes of `a` plus two trailing spaces | `Ok(Some(..))`, trim precedes the measurement |
| 254 bytes of `a` plus two trailing spaces | `Err(TooLong { bytes: 254 })`, the reported count is the trimmed length |
| 300 bytes of `a` plus a trailing `/` | `Err(InvalidCharacter { ch: '/' })`, **not** `TooLong` |

Nine of these rows exist to falsify a specific wrong implementation rather than
to cover an interesting value, and removing them removes the only evidence the
rule is implemented as stated. The last row is the only thing that catches a
length-first implementation. The `-web/01` row is the only thing that catches
the other stated sub-ordering, a first character checked after the remainder
scan: every other rejected value is invalid in exactly one of the two positions,
so an implementation that scans the remainder first passes all of them. `_web01`
is the only thing that catches a first-character check narrowed to
`is_ascii_alphanumeric`. The four single-character rows catch a first-character
check written as `chars().next()` plus a separate loop that is off by one. The
254-plus-spaces row catches a `TooLong` reporting the untrimmed count. The
`b"web\xff01"` row proves the resolver rejects malformed bytes rather than
reading them as absent. It cannot prove the call site hands them over: it builds
the `OsStr` directly, so a `run_scan` that read the variable as a `String` and
mapped it back would pass the variable as absent and leave this row green. The
`OsStr` parameter makes that regression hard rather than impossible, and case 4
in § 11 is what covers the boundary.

## 6. Where the resolved value is applied

### 6.1 Placements rejected

Two placements were considered and both are rejected. They are recorded because
the first is the obvious one-line change and nothing else here would stop an
implementer reaching for it.

- **Rejected: read the variable inside `collect()`.** A one-line change at
  `collect.rs:216` and the smallest possible diff, but it buries a
  process-global input in a library function whose other inputs all arrive as
  parameters, and it makes the rule untestable without mutating the environment,
  which in edition 2024 is `unsafe` and races the test harness anyway.
- **Rejected: add a seventh parameter to `collect()`.** Correct in principle,
  and where the value would live if the function were being written today, but
  `collect()` has **54 call sites** at HEAD and the argument would be `None` at
  53 of them. Not proportionate to a one-value feature. *Method:* every
  `collect(` call expression in the workspace's `.rs` files, excluding comments
  and the definition. 52 are in test code (23 in `collect.rs`'s own
  `#[cfg(test)]` module from `:538`, 29 across `crates/pipeline/tests/` and
  `crates/cli/tests/`); the production sites are `scan.rs:608` and
  `orchestrate.rs:45`, and the second is reached only from an e2e test.

### 6.2 Chosen placement

**Resolve first.** `resolve_hostname_override` is called at the very top of
`crates/cli/src/commands/scan.rs::run_scan`, **before the `geteuid()` root
check** at `scan.rs:362-369`, which is currently the first statement in the body
and calls `std::process::exit(1)`. Argument validation before a privilege check
is the normal ordering, and it has a concrete payoff: the rejection path becomes
reproducible without root, which is what makes the integration tests in § 11
runnable in CI. Nothing with a side effect has happened at that point: no
executor, no source detection, no baseline image pull, no inspector.

**Apply twice, from one resolved value.** At `scan.rs:559`, where `hostname` is
bound from `get_hostname(&executor)`, prefer the resolved override when present;
`get_hostname` need not run at all then, and both the progress line and
`get_output_stamp` at `:839` read that one binding. And into the snapshot by
**insertion**, not conditional replacement, so the key is present even when
`collect()` left it out on a host with an empty `/etc/hostname`. The insertion
goes in the existing CLI meta block at `scan.rs:672-684`, beside `scan_roots`
and the other CLI-sourced meta keys.

**The insertion is a named function, not an inline pair of `insert` calls.** A
requirement, not an implementer's choice: the two unit tests in § 11 need
something to call, and the second of them is the only test anywhere of the
ordering constraint above. Note what they do and do not prove. They prove that
insertion rather than conditional replacement is what makes acceptance
criterion 3 hold. They do not prove that `run_scan` calls the function, and
nothing in CI can, because an unprivileged `run_scan` exits at the root check
before a snapshot exists. The wiring is proved by the acceptance run.

```rust
fn apply_hostname_override(
    meta: &mut std::collections::HashMap<String, serde_json::Value>,
    resolved: &str,
) {
    meta.insert("hostname".into(), serde_json::Value::String(resolved.into()));
    meta.insert("hostname_source".into(), serde_json::json!("INSPECTAH_HOSTNAME"));
}
```

**The ordering constraint, stated honestly.** The insertion must happen after
`collect()` returns, which writes the detected value and would overwrite an
earlier insert, and before `redact()` and any renderer or tarball writer. The
backlog item says "after `collect()` and before `validate()`", which is stricter
than the code requires. Both placements are correct; the existing block is
preferred because it keeps CLI-sourced meta in one place.

### 6.3 Recording the override in the snapshot

When and only when the override is in force, `apply_hostname_override` also
inserts `meta["hostname_source"]` with the value `"INSPECTAH_HOSTNAME"`.

**Why a key at all, when the scan already announces the override on its own
output: stdout does not travel with the tarball.** The snapshot is the artifact
that leaves the machine and justifies a migration. Without the key it carries a
synthetic hostname with no record that it is synthetic, and the only evidence
that the name was declared rather than measured is a progress line in a terminal
that is gone. The cost is near zero (§ 4).

**The contract, which is a property of the override only:**

> In a snapshot that carries no `aggregate_meta`, `meta.hostname_source` is
> present if and only if `INSPECTAH_HOSTNAME` was in force for this scan. Its
> absence says nothing about how `meta.hostname` was obtained, or whether
> `meta.hostname` is present at all. In a snapshot that carries
> `aggregate_meta`, the key is an artifact of the meta merge and carries no
> per-host meaning; see below.

The weaker form is the only true one. "Absence means the hostname was detected"
is false on a host whose `/etc/hostname` is empty and whose scan set no
override: nothing was detected, so the snapshot would assert a detection that
did not happen. That host is the `rhel9` test VM and acceptance criterion 3.

Two limits follow from this and are tracked rather than left in prose. Absence
is two-valued, because a pre-feature snapshot and a non-overridden post-feature
scan look identical (`inspectah-hostname-source-absence-is-two-valued`). And the
key loses its per-host meaning under `aggregate` rather than being dropped by
it: the meta merge at `aggregate/mod.rs:190-196` unions every input's `meta`
first-writer-wins, so `hostname_source` is present in the merged map whenever
any member carried it, under every input ordering, while `meta.hostname` is
taken from the lexicographically first member. A fleet of one detected host `a`
and one overridden host `b` therefore merges to `meta.hostname` of `a` beside
`meta.hostname_source` of `INSPECTAH_HOSTNAME`, which belongs to `b` and says so
nowhere. Interpreting the key is correct only on a snapshot with no
`aggregate_meta` (`inspectah-aggregate-carries-no-hostname-provenance`). The
first limit is a consequence of the ratified decision to add the key, not a cost
this spec chose; the second is a deferral the project owner ratified on
2026-11-05, and ratified as priority work rather than an indefinite park.
Decisions 13 and 14.

**This is a rename, not anonymization.** `INSPECTAH_HOSTNAME` changes the name
the snapshot records. It does not remove the host's real identity from the
snapshot: `subscription.source_hostname` under `--preserve subscription`, and
non-loopback `/etc/hosts` lines captured into `network.hosts_additions`
(`crates/collect/src/inspectors/network.rs:494-516`), can both still name the
real machine. The documentation must not present the variable as a way to
sanitize a snapshot before sending it to a vendor.

### 6.4 Surfaces the override drives, and surfaces it does not

| Surface | Source today | Overridden |
|---|---|---|
| `meta["hostname"]` | `collect()`, `/etc/hostname` | Yes, by insertion |
| `Inspecting host X...` (`scan.rs:560`) | `get_hostname()` | Yes, annotated `(INSPECTAH_HOSTNAME)` |
| Default tarball filename and tar entry prefix (`scan.rs:839`) | the same `get_hostname()` value | Yes |
| `meta["hostname_source"]` | does not exist | Written only when overridden |
| Report, README, kickstart, refine `/api/health`, refine export filename | `meta["hostname"]` | Yes, transitively |
| Aggregate ordering, duplicate gate, `aggregate_meta.hostnames`, audit report | each input's `meta["hostname"]` | Yes, transitively |
| Per-host provenance in the aggregate | does not exist | **No**, `meta` collapses first-writer-wins |
| `subscription.source_hostname` | the subscription inspector's own read | **No** |
| `network.hosts_additions` | `/etc/hosts` | **No**, can still name the real machine |
| `aggregate`'s own tarball stamp (`aggregate.rs:216`) | the literal `aggregate-{label}` | No, never read a hostname |
| `run_pipeline()`'s `hostname` parameter | its caller | No, test-only |

**Why all three of snapshot, progress line, and tarball name.** The fleet loop
writes several tarballs into one directory; named from the real machine they
differ only by a timestamp to the second and can collide, silently clobbering a
scan. And a tarball named for the real machine wrapping a snapshot claiming
another name is the first anti-goal, which the strict reading of "everything
else unchanged" in the backlog item would produce exactly.

**Why not `subscription.source_hostname`.** It answers which registered machine
held this entitlement, which stays true whatever the scan calls itself. The
divergence must be documented, because it otherwise reads as a bug: under
`--preserve subscription` a snapshot has `meta.hostname` of `web-01` and
`subscription.source_hostname` of the real machine. The merge tiebreak at
`aggregate/mod.rs:147-155` prefers `source_hostname`, so several overridden scans
of one machine tiebreak on the real name, between sections identical anyway.

**Why `scan` only.** The other commands read a hostname out of snapshots that
already recorded one; honoring an override there would rewrite recorded
provenance after the fact, and in `aggregate`'s case collapse every input to one
name and trip the duplicate gate. They ignore it **silently**: a fleet script
exports the variable once, so a warning would fire in the normal case, and a
warning that fires in the normal case teaches people to ignore warnings.

## 7. Error behavior

A rejected value returns `Err` from `run_scan`. `crates/cli/src/main.rs:73-76`
already prints `error: {e:#}` to stderr and exits **1**, the documented code for
a fatal argument error. No new exit code. Because resolution runs before the
root check, a rejected value fails before `geteuid()`, before source detection,
before the baseline image pull, and before any inspector.

The message must name `INSPECTAH_HOSTNAME` and state which rule was broken; a
message that only says "invalid hostname" fails the project's actionable error
standard. Each variant carries what its message needs, so the message is
derivable from the error rather than recomputed at the call site.

The character carried by `InvalidFirstCharacter` and `InvalidCharacter` is
**rendered escaped**, with `{ch:?}` or equivalent, never `{ch}`. The accepted set
excludes every control character, so the only characters the payload can hold
are ones that should not reach a terminal raw:
`INSPECTAH_HOSTNAME=$'web\x1b[2J01'` is rejected, and under `{ch}` the rejection
message writes a raw escape sequence to the stderr of a root process. `{ch:?}`
writes `'\u{1b}'` instead. Echoing the value itself is optional and the default
is not to; if the implementer echoes it, the echo is length-capped and the
`NotUtf8` case must not echo raw bytes.

Worked strings, in the repo's style of naming the input and the fix:

```
INSPECTAH_HOSTNAME is set to an empty value. Unset it to use the detected hostname, or set it to a hostname.
INSPECTAH_HOSTNAME starts with a character that is not allowed: '-'. Start with a letter, digit, or underscore.
INSPECTAH_HOSTNAME contains a character that is not allowed: '/'. Use letters, digits, dots, hyphens, and underscores.
INSPECTAH_HOSTNAME is longer than 253 bytes after trimming: 254.
INSPECTAH_HOSTNAME is set to a value that is not valid UTF-8.
```

## 8. Interaction with the empty-hostname item

Open backlog item `inspectah-no-hostname-when-etc-hostname-is-empty`
(2026-10-31) reports that on a DHCP-named RHEL 9.8 host `/etc/hostname` is
empty, so the `!hostname.is_empty()` guard at `collect.rs:221` never inserts the
key and the snapshot carries no hostname at all. Every reader in § 4 falls back.

The two items are independent and the ordering does not matter. The override is
applied after `collect()` returns, by insertion rather than modification, so
whether `collect()` inserted a key, an empty-looking one, or nothing, the result
is the same. Detection still runs when the override is set:
`collect.rs:216-225` reads `/etc/hostname` on every scan and nothing in this
feature suppresses that read. What the override removes is detection's effect
on the snapshot. On a host with an empty `/etc/hostname` the guard at `:221`
still inserts no key and the empty-hostname condition is still reached; it
simply cannot determine the final value, because the CLI insertion runs after
it and is unconditional. When the override is unset this spec changes nothing
at all.

The override is therefore a workaround for that item in the cases where someone
sets it, and no part of a fix for it. The two detections in § 1 stay separate and
stay inconsistent on a DHCP host: `/etc/hostname` is empty while the `hostname`
command returns the transient name. Reconciling them belongs to that item. One
deliberate overlap: both items want `meta["hostname_source"]`, and this spec
writes the first value into it while writing no key on the detected path, so it
does not pre-empt that item's taxonomy call.

## 9. The sudo gotcha

The primary consumer runs `sudo inspectah scan`, and `scan` requires root. sudo
defaults to `env_reset`, which drops the variable. Three forms work, and they
are not equivalent.

```bash
# Recommended. Passes exactly this one variable into the root process.
sudo INSPECTAH_HOSTNAME=web-01 inspectah scan

# For a loop that exports the variable once. Equally narrow.
export INSPECTAH_HOSTNAME=web-01
sudo --preserve-env=INSPECTAH_HOSTNAME inspectah scan

# Works, and is the widest of the three. Prefer one of the above.
INSPECTAH_HOSTNAME=web-01 sudo -E inspectah scan
```

All three rely on the sudoers rule implying `SETENV`, which a plain
`ALL=(ALL) ALL` does. `-E` is listed last because it disables `env_reset`
wholesale, so the root process also inherits `PATH` and the proxy and
trust-store variables. `inspectah scan` pulls a container image on the baseline
path, so a stale `HTTPS_PROXY` in the caller's environment fails the scan for
reasons that look nothing like a proxy problem. sudo still scrubs `LD_PRELOAD`,
so this is a wider blast radius, not a privilege escalation.

Without one of these forms the variable simply is not there and the scan uses
the detected hostname with no complaint, which looks exactly like the feature
not working. **This belongs in the documentation, not only in a release note.**

## 10. Documentation and CHANGELOG

The pages are written in a separate pass. What they must say:

**`docs/reference/configuration.md` is the normative home.** The
`## Environment variables` table (`:14-18`) gains a row in the existing
four-column form: the accepted set in words, a default of unset meaning the
detected hostname is used, and a description saying it is honored by `scan`
only, sets both the snapshot hostname and the default tarball name, and that an
empty or invalid value is an error rather than a fallback. Below the table, a
subsection carrying what the row cannot: the three sudo forms with
`sudo VAR=… cmd` recommended; that `aggregate`, `refine`, and `build` ignore the
variable; that `subscription.source_hostname` stays as detected and this is not
an anonymization feature; and that `meta.hostname_source` is meaningful on a
single-host snapshot only: `aggregate` merges every member's `meta` into one
map, so on a fleet snapshot the key appears whenever any member was overridden
and does not say which one, which is why a reader should check for
`aggregate_meta` before reading it.

**`docs/reference/cli.md`, under `## inspectah scan`.** The Options table is
flags only, so add a short `### Environment` subsection immediately before
`### Examples` (today at `:81`, after `### Progress Modes` at `:74`) linking to
the configuration page, then two examples: the recommended sudo form with one
sentence on why plain `INSPECTAH_HOSTNAME=web-01 sudo inspectah scan` does not
work, and a worked fleet loop of three scans of one machine into one directory
followed by the `inspectah aggregate <directory>` call that consumes them. That
loop is the feature's reason for existing and should read end to end.

**`docs/reference/snapshot-schema.md:44`.** The `meta` row says `hostname` is
"omitted when `/etc/hostname` is empty", which becomes conditionally false. The
row must say so, and must document `hostname_source` using the if-and-only-if
contract from § 6.3, not the weaker "absence means the hostname was detected".

**CHANGELOG.** One entry under `## [Unreleased]`, category `### Added`, in the
current `- **Short label**: description` form from
`process-docs/changelog-policy.md`. Three things, roughly a sentence each: that
`inspectah scan` honors `INSPECTAH_HOSTNAME`; that it sets both the snapshot
hostname and the default tarball name; and the recommended sudo form, because an
entry that omits it generates support traffic. Link to the configuration
reference rather than restating the validation rule.

## 11. Test strategy

**Unit, table-driven, no environment mutation.** One data array over § 5.1,
iterated in a single `#[test]`, in the `#[cfg(test)]` module beside
`resolve_hostname_override`. The non-UTF-8 row is built with
`std::os::unix::ffi::OsStrExt::from_bytes` behind `#[cfg(unix)]`.

**The same test asserts the pass-through property.** For every row resolving to
`Ok(Some(v))`:

```rust
// The accepted set must survive sanitize_hostname unchanged, or the snapshot
// and the tarball name silently disagree (anti-goal 4).
assert_eq!(sanitize_hostname(&v), v, "accepted value rewritten by sanitize: {v}");
```

Two lines, no new harness. The subset relationship between the accepted set and
what `sanitize_hostname` preserves is the widest call in this document and is
otherwise argued only in prose; this assertion fails loudly the first time
anyone widens the accepted class past what `[^\w.-]` preserves.
`inspectah_pipeline::render::tarball` is reachable from `crates/cli`.

**The environment is off limits in tests.** In edition 2024 (`Cargo.toml:15`),
`std::env::set_var` and `remove_var` are `unsafe` and race the test harness.
`crates/cli/src/progress/mod.rs:210-240` already does this for
`INSPECTAH_PROGRESS`; do not copy that pattern.

**Integration, subprocess, in `crates/cli/tests/`.** `assert_cmd = "2"` and
`predicates = "3"` are confirmed present in `crates/cli/Cargo.toml`
`[dev-dependencies]`. Each test sets the variable on the child with `.env(..)`.
Five cases, all unprivileged because resolution precedes the root check:

1. An invalid value exits non-zero and stderr names `INSPECTAH_HOSTNAME`.
2. An empty value does the same. It gets its own case because it is the one most
   likely to be "corrected" into a silent fallback by a later change, and the
   unit test would not catch that at the binary boundary.
3. A valid value is **not** rejected: the process gets past resolution and fails
   at the root check instead. Asserting the next failure is what proves the
   valid path is open without needing root.
4. A non-UTF-8 value, behind `#[cfg(unix)]`, set with
   `.env("INSPECTAH_HOSTNAME", OsStr::from_bytes(b"web\xff01"))`, is rejected
   the same way as case 1. `Command::env` takes `AsRef<OsStr>`, so the bytes
   reach the child unchanged. This is the only case that exercises the
   environment read itself, and the only thing that catches a call site that
   reads the variable as a `String`.
5. A value carrying an escape character, `web\x1b[2J01`, is rejected and its
   stderr contains no raw `0x1b` byte. § 7 requires the offending character to
   be rendered escaped; the unit rows assert the error variant and its payload,
   so nothing else observes the rendering.

**Both failure paths exit 1**, the rejection through `main.rs:73-76` and the
root check through `scan.rs:368`, so `.failure()` and `.code(1)` prove nothing
on their own. Each case asserts a positive and a negative on stderr:

```rust
// cases 1, 2, 4 and 5: rejected at resolution, never reached the root check
.failure()
.stderr(predicate::str::contains("INSPECTAH_HOSTNAME"))
.stderr(predicate::str::contains("requires root privileges").not());

// case 3: valid value, got past resolution, stopped at the root check
.failure()
.stderr(predicate::str::contains("requires root privileges"))
.stderr(predicate::str::contains("INSPECTAH_HOSTNAME").not());
```

**Case 3 needs a privilege guard.** It would be the first test in this repo that
enters `run_scan`'s body; the only existing `assert_cmd` test is clap-rejected at
exit code 2 and never gets there. Run as root, case 3 does not merely fail:
execution falls through the `euid != 0` branch and performs a real scan,
including a baseline container image pull (`scan.rs:430`), every inspector, and
a tarball written into the test's working directory. GitHub CI is safe today,
`rust-ci.yml:91` runs `cargo test --workspace` on `runs-on: ubuntu-latest` with
no `container:` key, so the exposure is local: a devcontainer, a rootful
container shell, or anyone who reaches for `sudo cargo test` after a permissions
error. `libc` is already a direct dependency of `crates/cli`:

```rust
// This case asserts on the root-check rejection. Run as root it is both
// meaningless and destructive: it would perform a real scan.
// SAFETY: geteuid() is a simple syscall with no preconditions.
if unsafe { libc::geteuid() } == 0 {
    eprintln!("skipping: requires an unprivileged test runner");
    return;
}
```

Cases 1, 2, 4 and 5 need no guard: resolution precedes the root check, so they
behave identically either way.

**Any case that must see the variable absent calls
`.env_remove("INSPECTAH_HOSTNAME")`**, because `Command::cargo_bin(..)` passes
the caller's environment to the child. No case above needs it, since all five
set the variable; this is a standing requirement for any case that later does.

**Two unit tests for the insertion**, over `apply_hostname_override`: a `meta`
with no `hostname` key gains both keys, which is acceptance criterion 3 and holds
only because the write is an insertion; and a `meta` whose `hostname` is already
`"rhel9"` has it replaced, which is the ordering constraint in § 6.2 and nothing
else tests it.

**No snapshot-test churn.** Existing `insta` snapshots are taken with the
variable unset and must not move. A diff in any of them means the unset path
changed, which is a bug in the change.

**End to end is a real-host step, not a CI test.** A snapshot whose hostname is
actually `web-01` requires root and a real system; it belongs in the acceptance
run, consistent with `process-docs/skills/real-host-validation-runs.md`. That
run is the only evidence that the resolved value reaches the snapshot. Every
unit and integration case above passes on a binary that resolves the override
correctly, rejects bad values, and uses the value for the progress line and the
archive name while leaving `meta.hostname` detected. The acceptance run must
therefore read back the emitted snapshot JSON and the archive name and entry
prefix from the same scan, per criteria 1 and 2, and record what it read.
Confirming that the scan exited zero does not cover it.

## 12. Acceptance criteria

1. `sudo INSPECTAH_HOSTNAME=web-01 inspectah scan` produces a snapshot whose
   `meta.hostname` is `web-01` and whose `meta.hostname_source` is
   `INSPECTAH_HOSTNAME`, with no other `meta` key changed.
2. That run's default tarball is named `web-01-YYYYmmdd-HHMMSS.tar.gz` and its
   tar entries sit under the matching prefix.
3. The same holds on a host whose `/etc/hostname` is empty; the `rhel9` test VM
   is one.
4. With the variable unset, output is unchanged from today, modulo timestamps,
   and **no existing `insta` snapshot moves**.
5. Three scans of one machine under three different overrides aggregate without
   a duplicate-hostname error, and `aggregate_meta.hostnames` lists the three in
   sorted order.
6. An empty, whitespace-only, over-length, non-UTF-8, or syntactically invalid
   value exits 1 before any inspector runs, with a message on stderr naming
   `INSPECTAH_HOSTNAME` and the broken rule.
7. `aggregate`, `refine`, and `build` run with the variable set behave exactly
   as with it unset, and say nothing about it.
8. `cargo clippy --workspace --all-targets -- -D clippy::all` and
   `cargo fmt --check` pass, and no test mutates the process environment.
9. The three documentation pages carry what § 10 requires, including the
   recommended sudo form and the `--preserve-env` form.
10. A CHANGELOG entry exists under `## [Unreleased]`.

## 13. Left to the implementer

- Which file the pure function lives in. A new small module under
  `crates/cli/src/` or a private function in `scan.rs` are both fine; it is
  CLI-layer, not library, because the variable is a process-global input.
- Exact error message wording, subject to § 7. The strings there are worked
  examples, not normative text.
- Whether the syntax check is a `LazyLock<Regex>` matching the existing
  `tarball.rs` pattern or a hand-rolled character loop. Both work and `regex` is
  already in the tree, but the choice is not level now that the error variants
  carry the offending character: a loop has that character in hand and
  `Regex::is_match` cannot produce it at all. The loop is the simpler fit.
- Whether the error type is `thiserror`-derived or converted to `anyhow` at the
  call site.
- Whether the progress-line annotation reads `(INSPECTAH_HOSTNAME)` or
  something else.

One consequence to know about and **not** try to fix: a value over roughly 232
bytes produces a tarball filename past the 255-byte component limit and fails
late, at tarball creation. See decision 10, which is ratified: do not add a
second, tighter cap.

## 14. Decisions

Seven questions in the backlog item carried provisional recommendations that
had never been reviewed, and eight more arose from reading the code. All
fifteen are now settled one of two ways, and the **Settled by** column says
which: **Owner, 2026-11-05** for the ten the project owner ratified, and
**Spec** for the five that remain the spec author's call. The "reopen if"
clause applies to both kinds, because a ratified decision can still be
revisited on new evidence.

| # | Decision | Rules out | Reopen if | Settled by |
|---|---|---|---|---|
| 1 | An empty or whitespace-only value is an error, and the scan exits before any inspector runs | The `NO_COLOR` convention of treating presence as the signal. A fallback produces several snapshots claiming one identity and fails much later in `aggregate`, naming a symptom | A real caller needs `INSPECTAH_HOSTNAME="$maybe_unset"` to degrade rather than fail | Owner, 2026-11-05 |
| 2 | Accept `[A-Za-z0-9_.-]` after trimming, first character in `[A-Za-z0-9_]`, at most 253 bytes | Full RFC 1123 explicitly, because `web_01` and `3node` are reasonable synthetic identities and inspectah never resolves the name. Also rules out no validation, which would let the snapshot and the tarball name diverge through `sanitize_hostname` | A user needs a non-ASCII host identity | Owner, 2026-11-05, with note (a) |
| 3 | The override drives all three surfaces: the snapshot, the tarball filename, and the progress line, from one resolved value | The strict reading of "everything else unchanged", which leaves a tarball named for the real machine wrapping a snapshot claiming another name | A consumer wants the real machine name on the archive for audit | Owner, 2026-11-05, with note (b) |
| 4 | `subscription.source_hostname` stays as detected | One consistent name everywhere in the snapshot. The mitigation instead is to document the divergence as the reason this is a rename and not anonymization | The divergence confuses a real operator | Owner, 2026-11-05 |
| 5 | Add `meta["hostname_source"] = "INSPECTAH_HOSTNAME"`, written on the override path only | The backlog item's "no new meta key, make it visible in the scan output instead", and a boolean `hostname_overridden` flag | A consumer is found that chokes on an unknown meta key, which would be a defect in that consumer | Owner, 2026-11-05, reversing the item's recommendation; note (c) |
| 6 | `scan` only. `aggregate`, `refine`, and `build` ignore the variable silently, with no warning | Honoring it in those commands; and warning when it is set on one that ignores it, which would fire in the normal case because a fleet script exports it once | Users report confusion | Owner, 2026-11-05 |
| 7 | No `--hostname` flag. The variable only | Shipping both with the flag taking precedence, as `--progress` relates to `INSPECTAH_PROGRESS`. A flag also costs hand-edits to `completions/inspectah.{bash,fish,zsh}`, checked-in `clap_complete` output that nothing regenerates | Someone asks for the flag by name. Adding it later is strictly additive | Owner, 2026-11-05 |
| 8 | Resolve before the `geteuid()` root check, not after | Validation alongside `validate_sensitivity_flags`, after which the integration tests pass vacuously on the root error | Some future check genuinely has to precede argument validation | Spec |
| 9 | Insert into the existing CLI meta block after `validate()` | The item's "between `collect()` and `validate()`". `validate()` compares only `schema_version`, so the real constraints are after `collect()` and before `redact()` | `validate()` grows a check that reads `meta` | Spec |
| 10 | No second, tighter byte cap. The 253-byte cap stands and the filename overflow is a documented edge | A cap derived from the 255-byte component limit. `-o` does not avoid the overflow: it renames the outer file only and the prefix is still derived from the hostname (`scan.rs:838-853`, `tarball.rs:68-72`), so the archive will not extract, which is the worse outcome | Anyone hits it. A single cap of 232 would then cover both | Owner, 2026-11-05 |
| 11 | `run_pipeline()` is left alone | Plumbing the override through `orchestrate.rs`, whose only caller is an e2e test and which already takes a `hostname` parameter | The production scan starts calling it | Spec |
| 12 | `docs/reference/snapshot-schema.md` joins the documentation set | Nothing. Its `meta` row says `hostname` is "omitted when `/etc/hostname` is empty", which this feature makes conditionally false | Not reversible as a judgment call: the sentence is simply wrong once this ships | Spec |
| 13 | `hostname_source` is written on the override path only, and the two-valued absence that follows is accepted | Writing `"detected"` on the unset path: false on a host where nothing was detected, pre-empts the taxonomy `inspectah-no-hostname-when-etc-hostname-is-empty` owns, and changes unset-path output against acceptance criterion 4 | That item is picked up. Tracked as `inspectah-hostname-source-absence-is-two-valued` | Spec, as the mechanics of decision 5; note (e) |
| 14 | `hostname_source` is a single-host marker; aggregate-level provenance is deferred, as priority work and not as an indefinite park | An `AggregateSnapshotMeta.hostname_sources` field parallel to `hostnames`, a struct change outside this spec's scope that would decide the sibling item's question from the wrong side | Either item is picked up. Tracked as `inspectah-aggregate-carries-no-hostname-provenance`, which carries the priority designation | Owner, 2026-11-05, deferred but flagged priority |
| 15 | Accepted trust change: an aggregate's `host_count` and `hostnames` are declared, not measured. Already true wherever an operator could rename the machine, as the fleet test does; this feature makes it true everywhere and at no cost | Nothing, see note (d) | Stronger assurance about host identity is wanted later, which is a new spec rather than an amendment to this one. See note (d) | Owner, 2026-11-05 |

**(a) On the character set.** The backlog item says this set is "exactly" what
`sanitize_hostname` preserves, which is false because the `regex` crate's `\w`
is Unicode-aware and preserves more; the property that matters, that accepted
values pass through unchanged, still holds because the accepted set is a subset.
The first-character rule is ratified specifically as a defect fix: without it
`-web01` is accepted and produces a tarball filename beginning with a hyphen,
which any later command reads as a flag. The set is strictly tighter than what
already flows into every one of these slots from the unvalidated detected path,
so the override narrows the character space reaching downstream readers rather
than widening it.

**(b) On the mechanics.** The progress line and the tarball name do not come
from `meta["hostname"]` at all, as the backlog item assumed. They come from
`get_hostname()`, so this is one extra binding at `scan.rs:559`, not a
downstream consequence of the snapshot change.

**(c) On the reversal.** The backlog item recommended no new key, with the
override made visible in the scan's own output instead. The reason that does
not hold is in § 6.3: stdout does not travel with the tarball, so without the
key the artifact that leaves the machine carries a synthetic name with no record
that it is synthetic. The item's cost argument also does not hold, because
`meta` is a free-form map and the key costs no `SCHEMA_VERSION` bump. The
scan-output annotation the item proposed is adopted as well, not instead.

**(d) On the trust change.** The duplicate-hostname gate
(`aggregate/validate.rs:86-102`) asserts only that a set of snapshots carries
distinct hostname labels. It has never asserted distinct machines: nothing
cross-checks machine-id, serial, or MAC, and `host_count` is `snapshots.len()`
(`aggregate/mod.rs:52`, `:77`), a count of input files. The project's own fleet
test already satisfies the gate from a single box by renaming it to `web-01`,
`web-02`, and `web-03` between scans (`run-fleet-test.sh:51-52`, `:82`). This
feature does not weaken the gate. It makes declaring a label cheap and
unprivileged, and removes the last incidental reason a fleet set would happen
to correspond to distinct machines. The related
assumption, which this spec does not try to solve and should not: the operator
setting `INSPECTAH_HOSTNAME` is authorized to decide what the snapshot claims to
be, and in the Ansible and CI cases that authorization is whatever protects the
inventory file or the job parameter. This is **not a choice between
alternatives**, so it rules nothing out: it is the recorded consequence of the
feature existing at all, and of decision 3 in particular, where the alternatives
were weighed. The project owner accepted it on 2026-11-05 as fine for now and
set the remedy with it: if stronger validation of host identity is wanted later,
that is a new spec, not an amendment to this one. It stops being the whole story
only if some later control cross-checks machine identity against something the
operator does not supply, which nothing asks for today.

**(e) On what decision 13 is and is not.** The key itself is the owner's call
(decision 5). What remains the spec's is only the mechanics: writing nothing on
the unset path, and therefore accepting that an absent `hostname_source` cannot
distinguish a pre-feature snapshot from a non-overridden scan. That ambiguity is
a consequence of the ratified decision rather than a cost the spec chose, and it
bites first when `inspectah-no-hostname-when-etc-hostname-is-empty` is picked up.


## 15. Related

- `inspectah-hostname-override`: the source backlog item.
- `inspectah-no-hostname-when-etc-hostname-is-empty`: why a DHCP-named host has
  no hostname to detect. Independent in both directions; see § 8.
- `inspectah-hostname-source-absence-is-two-valued`: the cost decision 13
  accepts.
- `inspectah-aggregate-carries-no-hostname-provenance`: the gap decision 14
  defers.
- `inspectah-snapshot-schema-meta-row-omits-three-keys`: a pre-existing defect in
  the same `meta` row § 10 corrects. Out of scope here, but whoever writes the
  documentation pass is standing in the right place to fix both.
- `inspectah-aggregate-rejects-unnamed-snapshots-as-duplicates`: the failure this
  feature lets an operator work around by naming each host at scan time.
- `driftify-adopt-inspectah-hostname-override`: the consumer-side work, which
  carries the `run-fleet-test.sh` drop list with line numbers and does not happen
  automatically when a release carries this.
- `process-docs/skills/snapshot-schema-versioning.md`: not engaged; `meta` is a
  free-form map.
