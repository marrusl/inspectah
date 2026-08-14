# Preserve Flag Consolidation & --no-redaction

## Summary

Replace the three individual `--preserve-*` flags with a single `--preserve <values>`
flag, and add `--no-redaction` to skip the redaction pipeline phase. Both require the
existing `--ack-sensitive` safety gate.

## Motivation

inspectah currently has three `--preserve-*` flags on `scan`:
- `--preserve-password-hashes`
- `--preserve-ssh-keys`
- `--preserve-subscription`

As more preservable items are added, this pattern doesn't scale. A consolidated
`--preserve` flag with named values is cleaner and extensible.

Separately, there is no way to disable the secrets redaction engine. Users in trusted
environments (internal security audits, debugging, CI pipelines with vault-managed
secrets) sometimes need the raw, unredacted output. `--no-redaction` fills this gap.

These are two independent concerns:
- **`--preserve`** controls what sensitive data the *collector* includes in the snapshot
- **`--no-redaction`** controls whether the *redaction engine* masks secrets in config files

## Design

### 1. `--preserve` flag

**CLI syntax:**
```
--preserve <value>[,<value>...]
```

**Accepted values:** `password-hashes`, `ssh-keys`, `subscription`, `all`

**Behavior:**
- Comma-separated: `--preserve ssh-keys,password-hashes`
- Repeated: `--preserve ssh-keys --preserve password-hashes`
- Mixed: `--preserve ssh-keys,subscription --preserve password-hashes`
- Shorthand: `--preserve all` — expands to all variants, forward-compatible (future
  preserve items are automatically included in `all`)
- Redundancy is silently deduplicated (`--preserve all,ssh-keys` is valid, not an error)
- `--preserve` with no value is a parse error
- Invalid values produce a clap error listing valid options

**Clap implementation:** `Vec<PreserveItem>` field with `value_delimiter = ','`. `PreserveItem`
is an enum deriving `clap::ValueEnum`. The enum provides:
- `expand_all() -> Vec<PreserveItem>` — returns all concrete variants
- `All` is not stored — expansion happens at parse time, replacing `All` with the
  concrete variants before any downstream code runs. This avoids every consumer
  needing an `|| is_all()` guard.

**Migration:** The old flags (`--preserve-password-hashes`, `--preserve-ssh-keys`,
`--preserve-subscription`) are removed entirely. No hidden aliases, no deprecation
period. inspectah is pre-1.0 alpha; clean break is appropriate.

**Scope:** `scan` command only. The `fleet` command does not have preserve flags — it
ingests already-scanned tarballs and checks snapshot metadata.

**Stale references:** `build.rs` currently tells users to re-scan with
`--preserve-subscription`. Update to reference `--preserve subscription`.

### 2. `--no-redaction` flag

**CLI syntax:**
```
--no-redaction
```

**Behavior:**
- Boolean flag, no arguments
- Skips the redaction pipeline phase entirely (collect → validate → ~~redact~~ → render → tarball)
- All secrets, connection strings, API tokens, proxy credentials remain as-is in output
- Requires `--ack-sensitive`
- Independent of `--preserve` — can be used alone, with `--preserve`, or not at all

**Combinations:**
```
inspectah scan --no-redaction --ack-sensitive                    # raw secrets, default collection
inspectah scan --preserve all --ack-sensitive                    # full collection, still redacted
inspectah scan --preserve all --no-redaction --ack-sensitive     # everything, nothing masked
inspectah scan                                                   # default: redacted, minimal collection
```

**Implementation:** A `skip_redaction: bool` field on `ScanArgs`. The `redact()` call
in `scan.rs` is guarded by this flag. Note: the actual `redact()` call site is in
`scan.rs` (not `run_pipeline` in `orchestrate.rs`), so the bypass must be applied there.
The `RedactOptions` struct and redaction engine are untouched — this is a bypass, not a
mode change.

### 3. Snapshot provenance model

The codebase already has a typed provenance model in `RedactionState`:

```rust
pub enum RedactionState {
    FullyRedacted { redacted_by, config_hash },
    PartiallyRedacted { redacted_by, config_hash, unresolved_count, unresolved_hints },
    SensitiveRetained { redacted_by, config_hash },
    Unknown,
    Raw,
}
```

**`--no-redaction` snapshots use `RedactionState::Raw`.** This is the single source of
truth for provenance — no separate `redaction_skipped` boolean. When `--no-redaction` is
set, the snapshot's `redaction_state` is set to `Raw` instead of running the redaction
engine (which would set it to `FullyRedacted` or `PartiallyRedacted`).

**Sensitivity metadata contract:**
```rust
snapshot.sensitive_snapshot =
    any_preserve || skip_redaction;
snapshot.redaction_state = if skip_redaction {
    Some(RedactionState::Raw)
} else {
    // set by redact() as today
};
```

### 4. Refine behavior with unredacted snapshots

The current `validate_provenance()` in `inspectah-refine` rejects snapshots without
a trusted `RedactionState` (only accepts `FullyRedacted`, `PartiallyRedacted`,
`SensitiveRetained`).

**Change:** `Raw` is accepted by `validate_provenance()`. The user explicitly chose
`--no-redaction --ack-sensitive` — the tool should not second-guess that decision.
Refine is not useful without being able to process these snapshots.

Updated acceptance list:
- `FullyRedacted` — accepted (current)
- `PartiallyRedacted` — accepted (current)
- `SensitiveRetained` — accepted (current)
- `Raw` — accepted (new)
- `Unknown` / `None` — rejected (current, unchanged)

### 5. Fleet merge contract for unredacted inputs

The fleet merge currently drops `redaction_state` (per-host, not meaningful when merged)
and ORs the sensitivity booleans. This contract extends to cover unredacted snapshots:

**Propagation rules:**
- `merged.sensitive_snapshot = sorted_snapshots.iter().any(|s| s.sensitive_snapshot)`
  — already covers `--no-redaction` because `sensitive_snapshot` is set for those (§3)
- `merged.redaction_skipped = sorted_snapshots.iter().any(|s| s.redaction_state == Some(Raw))`
  — new boolean on the merged snapshot, true if ANY input was unredacted. Uses
  `#[serde(default, skip_serializing_if = "is_false")]` to match the other sensitivity
  booleans. Consumed by the fleet `--ack-sensitive` error message (to enumerate
  "unredacted secrets" alongside other sensitivity types) and by downstream reporting.
  Gating itself works through `sensitive_snapshot`.
- `merged.redaction_state = None` — unchanged (per-host, not meaningful when merged)

**Fleet `--ack-sensitive` gate:** The existing check already gates on `sensitive_snapshot`,
which now includes unredacted snapshots. The error message detail collection extends to
include "unredacted secrets" when any input has `redaction_state == Raw`.

**Mixed fleet inputs (redacted + unredacted):** Valid. The fleet merge treats data as
opaque — it merges what the scans produced. The `--ack-sensitive` gate on `fleet aggregate`
is the safety checkpoint, and the error message enumerates which sensitivity types are
present across the fleet.

### 6. `--ack-sensitive` gate

**Current behavior (preserved):**
- Required on `scan` when any `--preserve` value is set
- Required on `fleet` when any ingested snapshot has `sensitive_snapshot: true`
- Alias: `--acknowledge-sensitive` (visible alias, already exists)

**Extended behavior:**
- Required on `scan` when `--preserve` is set OR `--no-redaction` is set (or both)
- One `--ack-sensitive` covers everything — no need to pass it multiple times
- `fleet` gate: no changes needed — `sensitive_snapshot` already covers `--no-redaction`
  snapshots per §3

**Error messages (specific per trigger):**

| Trigger | Message |
|---------|---------|
| `--preserve` without gate | `--preserve requires --ack-sensitive to acknowledge sensitive data in the snapshot` |
| `--no-redaction` without gate | `--no-redaction requires --ack-sensitive to acknowledge unredacted secrets in the snapshot` |
| Both without gate | `--preserve and --no-redaction require --ack-sensitive to acknowledge sensitive data in the snapshot` |
| Fleet with sensitive/unredacted snapshots | Existing message format, extended to include "unredacted secrets" when applicable |

### 7. Scan output confirmation

When `--preserve` or `--no-redaction` is used, the scan completion output enumerates
what was preserved and whether redaction was skipped. This serves two purposes:
- Confirms the user's intent (you asked for this, you got it)
- Makes new surface area visible when `--preserve all` expands to include future items

Example output:
```
Scan complete. Snapshot contains sensitive data:
  - Preserved: password-hashes, ssh-keys
  - Redaction: skipped (raw secrets retained)
```

When `--preserve all` is used, the expansion is listed explicitly:
```
Scan complete. Snapshot contains sensitive data:
  - Preserved (all): password-hashes, ssh-keys, subscription
  - Redaction: active
```

## Non-goals

- Granular redaction control (skip specific pattern types) — not needed, nobody has asked
- `--sensitivity` tiers or modes — over-engineered for current needs
- Backwards compatibility with old `--preserve-*` flags — pre-1.0, clean break
- Changes to the redaction engine itself — `--no-redaction` bypasses it, doesn't modify it
- Schema version bump — pre-1.0, no formal versioning contract yet (GA schema naming
  is a separate discussion)

## Testing

**Clap parse tests:**
- All `--preserve` value combinations (single, comma-separated, repeated, mixed)
- `all` expansion: resolves to concrete variants at parse time
- Redundancy: `--preserve all,ssh-keys` silently deduplicates
- Invalid values: clap error with valid option list
- Missing value: `--preserve` alone is a parse error
- Missing `--ack-sensitive`: specific error per trigger (preserve-only, no-redaction-only, both)
- Old flags rejected: `--preserve-ssh-keys` produces "unknown flag" error

**Pipeline integration:**
- `--no-redaction` produces unredacted output (secrets present in rendered artifacts)
- `--preserve` items correctly collected (password hashes, SSH keys, subscription material)
- `--inspect-only` with `--no-redaction`: JSON snapshot has `redaction_state: Raw`

**Provenance and metadata:**
- `redaction_state` set to `Raw` when `--no-redaction` used
- `sensitive_snapshot` set when either `--preserve` or `--no-redaction` used
- Refine accepts `Raw` snapshots without error (replaces existing
  `validate_provenance_rejects_raw` test with an acceptance test for `Raw`)
- Refine still rejects `Unknown` / `None` snapshots

**Fleet:**
- Mixed fleet (redacted + unredacted inputs): merges successfully with `--ack-sensitive`
- Mixed fleet without `--ack-sensitive`: error message lists "unredacted secrets"
- `redaction_skipped` propagated correctly from any unredacted input

**Stale references:**
- `build.rs` updated to reference `--preserve subscription`

**Scan output:**
- Completion message enumerates preserved items
- `--preserve all` expansion shown explicitly
- `--no-redaction` status shown
