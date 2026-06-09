# Preserve Flag Consolidation & --no-redaction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace three individual `--preserve-*` flags with a consolidated `--preserve <values>` flag, add `--no-redaction` to skip the redaction pipeline phase, and wire both through the snapshot/fleet/refine provenance model.

**Architecture:** The `PreserveItem` enum lives in `inspectah-cli` (CLI parsing concern). It maps to the existing per-item booleans on `InspectionSnapshot` at scan time. `--no-redaction` bypasses the `redact()` call in `scan.rs` and sets `RedactionState::Raw` on the snapshot — this is the single source of truth for provenance on individual snapshots. Fleet merge derives `redaction_skipped` from `redaction_state == Some(Raw)` across inputs (because the merged snapshot drops `redaction_state`). Both flags require the existing `--ack-sensitive` gate. Refine accepts `Raw` snapshots.

**Tech Stack:** Rust, clap (derive API), serde

**Spec:** `process-docs/specs/proposed/2026-06-08-preserve-consolidation-and-no-redaction.md`

---

### Task 1: Define PreserveItem enum, update ScanArgs, and wire all call sites

This task replaces the old flags AND fixes all call sites in a single buildable commit.
Every intermediate state must compile.

**Files:**
- Modify: `inspectah-cli/src/commands/scan.rs`

- [ ] **Step 1: Write the PreserveItem enum**

Add the enum above the `ScanArgs` struct in `scan.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum PreserveItem {
    #[value(name = "password-hashes")]
    PasswordHashes,
    #[value(name = "ssh-keys")]
    SshKeys,
    #[value(name = "subscription")]
    Subscription,
    #[value(name = "all")]
    All,
}

impl PreserveItem {
    /// Expand `All` into concrete variants. `All` itself is consumed — it never
    /// appears in the returned vec.
    pub fn expand(items: &[PreserveItem]) -> Vec<PreserveItem> {
        let mut result = Vec::new();
        let has_all = items.iter().any(|i| matches!(i, PreserveItem::All));
        if has_all {
            result.push(PreserveItem::PasswordHashes);
            result.push(PreserveItem::SshKeys);
            result.push(PreserveItem::Subscription);
        } else {
            for item in items {
                if !result.contains(item) {
                    result.push(*item);
                }
            }
        }
        result
    }
}
```

- [ ] **Step 2: Replace the three preserve booleans in ScanArgs**

Remove the three `--preserve-*` fields and add the new ones:

```rust
// REMOVE these three fields:
//   pub preserve_password_hashes: bool,
//   pub preserve_ssh_keys: bool,
//   pub preserve_subscription: bool,

// ADD these two fields (after no_baseline, before ack_sensitive):

    /// Preserve sensitive data in the snapshot
    #[arg(long, value_delimiter = ',', value_name = "ITEM")]
    pub preserve: Vec<PreserveItem>,

    /// Skip the redaction phase — secrets remain unmasked in output
    #[arg(long)]
    pub no_redaction: bool,
```

- [ ] **Step 3: Add a validate_sensitivity_flags function**

Add above `run_scan`:

```rust
fn validate_sensitivity_flags(args: &ScanArgs) -> Result<()> {
    let has_preserve = !args.preserve.is_empty();
    let has_no_redaction = args.no_redaction;

    if (has_preserve || has_no_redaction) && !args.ack_sensitive {
        let msg = match (has_preserve, has_no_redaction) {
            (true, true) => {
                "--preserve and --no-redaction require --ack-sensitive to acknowledge sensitive data in the snapshot"
            }
            (true, false) => {
                "--preserve requires --ack-sensitive to acknowledge sensitive data in the snapshot"
            }
            (false, true) => {
                "--no-redaction requires --ack-sensitive to acknowledge unredacted secrets in the snapshot"
            }
            (false, false) => unreachable!(),
        };
        anyhow::bail!(msg);
    }
    Ok(())
}
```

- [ ] **Step 4: Update run_scan — replace old validation with new function**

Replace the old inline validation block (the `if (args.preserve_password_hashes || ...)`
block around line 189) with:

```rust
validate_sensitivity_flags(args)?;
```

- [ ] **Step 5: Add preserve expansion at top of run_scan**

After `validate_sensitivity_flags(args)?;`, add:

```rust
let preserved = PreserveItem::expand(&args.preserve);
let has_password_hashes = preserved.contains(&PreserveItem::PasswordHashes);
let has_ssh_keys = preserved.contains(&PreserveItem::SshKeys);
let has_subscription = preserved.contains(&PreserveItem::Subscription);
```

- [ ] **Step 6: Update UserGroupOptions**

Replace the `UserGroupOptions` construction (around line 340):

```rust
let user_group_options = UserGroupOptions {
    strategy_override: None,
    preserve_password_hashes: has_password_hashes,
    preserve_ssh_keys: has_ssh_keys,
};
```

- [ ] **Step 7: Update SubscriptionInspector conditional**

Replace `if args.preserve_subscription` (around line 361):

```rust
if has_subscription {
    inspectors.push(Box::new(SubscriptionInspector::new()));
}
```

- [ ] **Step 8: Update sensitivity metadata block**

Replace the sensitivity metadata block (around line 418-423):

```rust
snapshot.sensitive_snapshot =
    has_password_hashes || has_ssh_keys || has_subscription || args.no_redaction;
snapshot.preserved_credentials = has_password_hashes;
snapshot.preserved_ssh_keys = has_ssh_keys;
snapshot.preserved_subscription = has_subscription;
```

- [ ] **Step 9: Guard the redact() call with --no-redaction**

Replace the bare `redact()` call (line 449):

```rust
if args.no_redaction {
    snapshot.redaction_state = Some(RedactionState::Raw);
} else {
    redact(&mut snapshot, &RedactOptions::default());
}
```

Add the `RedactionState` import at the top of the file if not already present:

```rust
use inspectah_core::types::redaction::RedactionState;
```

- [ ] **Step 10: Build and verify**

Run: `cargo build -p inspectah-cli 2>&1 | tail -10`
Expected: successful build with no errors.

- [ ] **Step 11: Commit**

```bash
git add inspectah-cli/src/commands/scan.rs
git commit -m "feat(cli): consolidate --preserve flags and add --no-redaction

Replace --preserve-password-hashes, --preserve-ssh-keys, and
--preserve-subscription with --preserve <values> (comma-separated,
repeatable). Add --no-redaction to skip the redaction pipeline phase.
Both require --ack-sensitive with specific error messages.

RedactionState::Raw is the single source of truth for unredacted
snapshots.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 2: Add validation and expansion tests

**Files:**
- Modify: `inspectah-cli/src/commands/scan.rs` (test module)

- [ ] **Step 1: Write the test module**

Add at the bottom of `scan.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn base_args() -> ScanArgs {
        ScanArgs {
            inspect_only: false,
            output: None,
            base_image: None,
            no_baseline: false,
            preserve: vec![],
            no_redaction: false,
            ack_sensitive: false,
            progress: None,
            verbose: false,
            quiet: false,
        }
    }

    // --- ack-sensitive validation ---

    #[test]
    fn preserve_without_ack_is_error() {
        let args = ScanArgs {
            preserve: vec![PreserveItem::SshKeys],
            ..base_args()
        };
        let result = validate_sensitivity_flags(&args);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("--preserve requires --ack-sensitive"));
    }

    #[test]
    fn no_redaction_without_ack_is_error() {
        let args = ScanArgs {
            no_redaction: true,
            ..base_args()
        };
        let result = validate_sensitivity_flags(&args);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("--no-redaction requires --ack-sensitive"));
    }

    #[test]
    fn both_without_ack_is_error() {
        let args = ScanArgs {
            preserve: vec![PreserveItem::All],
            no_redaction: true,
            ..base_args()
        };
        let result = validate_sensitivity_flags(&args);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("--preserve and --no-redaction require --ack-sensitive"));
    }

    #[test]
    fn preserve_with_ack_is_ok() {
        let args = ScanArgs {
            preserve: vec![PreserveItem::SshKeys],
            ack_sensitive: true,
            ..base_args()
        };
        assert!(validate_sensitivity_flags(&args).is_ok());
    }

    #[test]
    fn no_redaction_with_ack_is_ok() {
        let args = ScanArgs {
            no_redaction: true,
            ack_sensitive: true,
            ..base_args()
        };
        assert!(validate_sensitivity_flags(&args).is_ok());
    }

    #[test]
    fn no_sensitive_flags_is_ok() {
        let args = base_args();
        assert!(validate_sensitivity_flags(&args).is_ok());
    }

    // --- PreserveItem expansion ---

    #[test]
    fn expand_all_returns_concrete_variants() {
        let items = vec![PreserveItem::All];
        let expanded = PreserveItem::expand(&items);
        assert_eq!(expanded.len(), 3);
        assert!(expanded.contains(&PreserveItem::PasswordHashes));
        assert!(expanded.contains(&PreserveItem::SshKeys));
        assert!(expanded.contains(&PreserveItem::Subscription));
        assert!(!expanded.contains(&PreserveItem::All));
    }

    #[test]
    fn expand_deduplicates_redundant_with_all() {
        let items = vec![PreserveItem::All, PreserveItem::SshKeys];
        let expanded = PreserveItem::expand(&items);
        assert_eq!(expanded.len(), 3);
    }

    #[test]
    fn expand_deduplicates_repeated_items() {
        let items = vec![PreserveItem::SshKeys, PreserveItem::SshKeys];
        let expanded = PreserveItem::expand(&items);
        assert_eq!(expanded.len(), 1);
        assert_eq!(expanded[0], PreserveItem::SshKeys);
    }

    #[test]
    fn expand_empty_returns_empty() {
        let items: Vec<PreserveItem> = vec![];
        let expanded = PreserveItem::expand(&items);
        assert!(expanded.is_empty());
    }

    #[test]
    fn expand_single_item() {
        let items = vec![PreserveItem::Subscription];
        let expanded = PreserveItem::expand(&items);
        assert_eq!(expanded.len(), 1);
        assert_eq!(expanded[0], PreserveItem::Subscription);
    }
}
```

- [ ] **Step 2: Run all tests**

Run: `cargo test -p inspectah-cli -- tests:: -v 2>&1 | tail -25`
Expected: all 11 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add inspectah-cli/src/commands/scan.rs
git commit -m "test(cli): add validation and expansion tests for --preserve

Covers ack-sensitive validation (6 tests) and PreserveItem expansion
(5 tests): all expansion, redundancy dedup, repeated dedup, empty
input, single item.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 3: Add redaction_skipped to InspectionSnapshot (fleet-only field)

This field exists for fleet merge propagation only. On individual snapshots,
`RedactionState::Raw` is the single source of truth — `redaction_skipped` is
NOT set in `scan.rs`.

**Files:**
- Modify: `inspectah-core/src/snapshot.rs:70-82`

- [ ] **Step 1: Write round-trip serialization test**

Add to the existing `#[cfg(test)] mod tests` in `snapshot.rs`:

```rust
#[test]
fn redaction_skipped_round_trip() {
    let mut snap = InspectionSnapshot::new();
    snap.redaction_skipped = true;
    snap.sensitive_snapshot = true;
    snap.redaction_state = Some(RedactionState::Raw);

    let json = serde_json::to_string(&snap).unwrap();
    let parsed: InspectionSnapshot = serde_json::from_str(&json).unwrap();
    assert!(parsed.redaction_skipped);
    assert!(parsed.sensitive_snapshot);
    assert_eq!(parsed.redaction_state, Some(RedactionState::Raw));
}

#[test]
fn redaction_skipped_defaults_false() {
    let snap = InspectionSnapshot::new();
    assert!(!snap.redaction_skipped);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-core -- redaction_skipped 2>&1 | tail -5`
Expected: FAIL — field doesn't exist.

- [ ] **Step 3: Add the field to InspectionSnapshot**

Check how the existing `preserved_subscription` field handles `skip_serializing_if`
for booleans. The codebase uses a helper — look for `is_false` or similar:

Run: `grep -rn 'skip_serializing_if.*false\|fn is_false' inspectah-core/src/ --include='*.rs' | head -5`

Use the same pattern. If the codebase uses a `crate::is_false` helper:

```rust
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub redaction_skipped: bool,
```

If no helper exists and booleans use a different pattern, match that pattern exactly.
Add the field after `preserved_subscription` (around line 82).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p inspectah-core -- redaction_skipped -v 2>&1 | tail -10`
Expected: both tests PASS.

- [ ] **Step 5: Build the full workspace**

Run: `cargo build 2>&1 | tail -10`
Expected: successful build across all crates.

- [ ] **Step 6: Commit**

```bash
git add inspectah-core/src/snapshot.rs
git commit -m "feat(core): add redaction_skipped field to InspectionSnapshot

Fleet-only boolean derived from RedactionState::Raw during merge.
NOT set on individual snapshots — Raw is the single source of truth
there. Uses the same serde skip pattern as other sensitivity bools.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 4: Update validate_provenance to accept Raw

**Files:**
- Modify: `inspectah-refine/src/tarball.rs:140-210`

- [ ] **Step 1: Update the existing rejection test to an acceptance test**

In `inspectah-refine/src/tarball.rs`, find the test `validate_provenance_rejects_raw`
(around line 196) and replace it:

```rust
#[test]
fn validate_provenance_accepts_raw() {
    let snap = InspectionSnapshot {
        redaction_state: Some(RedactionState::Raw),
        ..Default::default()
    };
    assert!(validate_provenance(&snap).is_ok());
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p inspectah-refine -- validate_provenance_accepts_raw -v 2>&1 | tail -10`
Expected: FAIL — `validate_provenance` still rejects `Raw`.

- [ ] **Step 3: Update validate_provenance to accept Raw**

Change the match in `validate_provenance` (line 140):

```rust
fn validate_provenance(snap: &InspectionSnapshot) -> Result<(), RefineError> {
    match &snap.redaction_state {
        Some(RedactionState::FullyRedacted { .. })
        | Some(RedactionState::PartiallyRedacted { .. })
        | Some(RedactionState::SensitiveRetained { .. })
        | Some(RedactionState::Raw) => Ok(()),
        _ => Err(RefineError::UntrustedSnapshot(
            "Snapshot has not been redacted. Run inspectah scan to produce a redacted snapshot before refining.".into(),
        )),
    }
}
```

- [ ] **Step 4: Run all provenance tests**

Run: `cargo test -p inspectah-refine -- validate_provenance -v 2>&1 | tail -15`
Expected: all acceptance tests PASS (fully_redacted, partially_redacted,
sensitive_retained, raw). The catch-all `_` arm still rejects `Unknown` and `None`.
Check which rejection tests exist and verify they still pass — don't assume test
names that may not exist.

- [ ] **Step 5: Commit**

```bash
git add inspectah-refine/src/tarball.rs
git commit -m "feat(refine): accept Raw snapshots in validate_provenance

Users who explicitly chose --no-redaction --ack-sensitive should be
able to refine their snapshots. Replaces the former rejection test
with an acceptance test.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 5: Update fleet merge to derive redaction_skipped from RedactionState

Per the spec, `redaction_skipped` is derived from `redaction_state == Some(Raw)`,
NOT from a per-snapshot boolean. The merged snapshot drops `redaction_state` (per-host,
not meaningful when merged), so this derived boolean is how the fleet remembers that
some inputs were unredacted.

**Files:**
- Modify: `inspectah-core/src/fleet/mod.rs:123-131`

- [ ] **Step 1: Write test for redaction_skipped derivation**

Add to the existing fleet tests in `inspectah-core/src/fleet/mod.rs`:

```rust
#[test]
fn merge_derives_redaction_skipped_from_raw_state() {
    let mut snap1 = valid_snap("host-a");
    snap1.sensitive_snapshot = true;
    snap1.redaction_state = Some(RedactionState::Raw);

    let snap2 = valid_snap("host-b");

    let (merged, _) = merge_snapshots(vec![snap1, snap2], None).unwrap();
    assert!(merged.redaction_skipped);
    assert!(merged.sensitive_snapshot);
    // redaction_state is dropped for merged snapshots
    assert!(merged.redaction_state.is_none());
}

#[test]
fn merge_no_redaction_skipped_when_all_redacted() {
    let snap1 = valid_snap("host-a");
    let snap2 = valid_snap("host-b");

    let (merged, _) = merge_snapshots(vec![snap1, snap2], None).unwrap();
    assert!(!merged.redaction_skipped);
}
```

Add the `RedactionState` import if not already present in the test module:

```rust
use crate::types::redaction::RedactionState;
```

- [ ] **Step 2: Run tests to verify the first fails**

Run: `cargo test -p inspectah-core -- merge_derives_redaction_skipped -v 2>&1 | tail -10`
Expected: FAIL — `redaction_skipped` defaults to false.

- [ ] **Step 3: Add derivation line to merge_snapshots**

After the existing `merged.preserved_subscription` line (around line 131), add:

```rust
    merged.redaction_skipped = sorted_snapshots
        .iter()
        .any(|s| s.redaction_state == Some(RedactionState::Raw));
```

Add the `RedactionState` import at the top of the file if not already present:

```rust
use crate::types::redaction::RedactionState;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p inspectah-core -- merge_ -v 2>&1 | grep -E 'redaction_skipped|test result'`
Expected: both tests PASS.

- [ ] **Step 5: Commit**

```bash
git add inspectah-core/src/fleet/mod.rs
git commit -m "feat(fleet): derive redaction_skipped from RedactionState::Raw

Derived from redaction_state == Some(Raw) across input snapshots,
not from a per-snapshot boolean. The merged snapshot drops
redaction_state (per-host), so this derived field is how fleet
remembers that some inputs were unredacted.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 6: Update fleet aggregate ack-sensitive gate

**Files:**
- Modify: `inspectah-cli/src/commands/fleet.rs:150-176`

- [ ] **Step 1: Add "unredacted secrets" to the sensitive types enumeration**

In `run_aggregate`, find the `sensitive_types` collection loop (around line 155).
Add after the SSH keys check:

```rust
            if snapshot.redaction_state == Some(RedactionState::Raw) {
                sensitive_types.insert("unredacted secrets");
            }
```

Add the `RedactionState` import at the top of `fleet.rs` if not already present:

```rust
use inspectah_core::types::redaction::RedactionState;
```

- [ ] **Step 2: Build and verify**

Run: `cargo build -p inspectah-cli 2>&1 | tail -5`
Expected: successful build.

- [ ] **Step 3: Commit**

```bash
git add inspectah-cli/src/commands/fleet.rs
git commit -m "feat(fleet): include unredacted secrets in ack-sensitive error

Fleet aggregate error message now enumerates 'unredacted secrets'
when any input snapshot was produced with --no-redaction.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 7: Add scan completion sensitivity confirmation

**Files:**
- Modify: `inspectah-cli/src/commands/scan.rs` (print_completion function, around line 589)

- [ ] **Step 1: Add sensitivity summary to print_completion**

The `print_completion` function currently prints outcome + counts. After the
existing match block, add a sensitivity summary that fires when any sensitive
flags are active. The function receives `&InspectionSnapshot`, so check
`redaction_state` (the single source of truth) rather than a boolean:

```rust
    // Sensitivity confirmation
    if snapshot.sensitive_snapshot {
        let mut preserved_items = Vec::new();
        if snapshot.preserved_credentials {
            preserved_items.push("password-hashes");
        }
        if snapshot.preserved_ssh_keys {
            preserved_items.push("ssh-keys");
        }
        if snapshot.preserved_subscription {
            preserved_items.push("subscription");
        }

        eprintln!("  Snapshot contains sensitive data:");
        if !preserved_items.is_empty() {
            eprintln!("    Preserved: {}", preserved_items.join(", "));
        }
        let is_raw = matches!(
            snapshot.redaction_state,
            Some(RedactionState::Raw)
        );
        if is_raw {
            eprintln!("    Redaction: skipped (raw secrets retained)");
        } else {
            eprintln!("    Redaction: active");
        }
    }
```

- [ ] **Step 2: Build and verify**

Run: `cargo build -p inspectah-cli 2>&1 | tail -5`
Expected: successful build.

- [ ] **Step 3: Commit**

```bash
git add inspectah-cli/src/commands/scan.rs
git commit -m "feat(cli): add sensitivity confirmation to scan completion output

Enumerates preserved items and redaction status after scan completes.
Checks RedactionState::Raw directly as the source of truth.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 8: Update stale references

**Files:**
- Modify: `inspectah-cli/src/commands/build.rs:86`

- [ ] **Step 1: Search for all old flag references across the codebase**

Run: `grep -rn 'preserve-password-hashes\|preserve-ssh-keys\|preserve-subscription\|preserve_password_hashes\|preserve_ssh_keys\|preserve_subscription' inspectah-cli/src/ inspectah-core/src/ inspectah-pipeline/src/ inspectah-refine/src/ inspectah-web/src/ inspectah-tui/src/ --include='*.rs' | grep -v target/ | grep -v '#\[test\]'`

Fix every match. The known one is `build.rs` line 86:

```rust
// OLD:
eprintln!("Re-scan the source host with --preserve-subscription.");

// NEW:
eprintln!("Re-scan the source host with --preserve subscription --ack-sensitive.");
```

Fix any others found by the grep.

- [ ] **Step 2: Build and verify**

Run: `cargo build -p inspectah-cli 2>&1 | tail -5`
Expected: successful build.

- [ ] **Step 3: Commit**

```bash
git add inspectah-cli/
git commit -m "fix(cli): update stale --preserve-* references

Old flag names removed in the preserve consolidation.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 9: Regenerate shell completions

**Files:**
- Modify: `completions/inspectah.bash`, `completions/inspectah.fish`, `completions/inspectah.zsh`

- [ ] **Step 1: Check how completions are generated**

Look for a generation script or build.rs that produces completions:

Run: `grep -rn 'completions\|clap_complete\|generate_to' inspectah-cli/src/ inspectah-cli/build.rs 2>/dev/null | head -10`

If completions are auto-generated by a build script, just rebuild. If they're
manually generated, run the generation command (likely something like
`cargo run -- completions bash > completions/inspectah.bash`).

- [ ] **Step 2: Regenerate all three completion files**

Run the appropriate command for each shell. The new `--preserve` flag with its
enum values and `--no-redaction` should appear in the completions.

- [ ] **Step 3: Verify the old flags are gone**

Run: `grep 'preserve-password-hashes\|preserve-ssh-keys\|preserve-subscription' completions/*`
Expected: no matches.

Run: `grep 'preserve' completions/inspectah.bash | head -5`
Expected: references to `--preserve` with value completions.

- [ ] **Step 4: Commit**

```bash
git add completions/
git commit -m "chore(cli): regenerate shell completions for new --preserve syntax

Old --preserve-* flags removed, new --preserve <values> and
--no-redaction added.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 10: Full workspace build and test pass

- [ ] **Step 1: Run full workspace build**

Run: `cargo build 2>&1 | tail -10`
Expected: successful build, no warnings related to preserve/redaction.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings 2>&1 | tail -20`
Expected: no warnings.

- [ ] **Step 3: Run full test suite**

Run: `cargo test 2>&1 | tail -30`
Expected: all tests pass. Watch specifically for:
- `scan::tests::` — all 11 new tests
- `validate_provenance_accepts_raw` — the replacement test
- `merge_derives_redaction_skipped_from_raw_state` — fleet derivation
- `redaction_skipped_round_trip` — serialization

- [ ] **Step 4: Verify --help output**

Run: `cargo run -- scan --help 2>&1`
Expected:
- `--preserve <ITEM>` with value list
- `--no-redaction` flag
- `--ack-sensitive` unchanged
- NO `--preserve-password-hashes`, `--preserve-ssh-keys`, `--preserve-subscription`

- [ ] **Step 5: Commit if any fixups were needed**

```bash
git add -A
git commit -m "chore: fixups from full workspace verification

Assisted-by: Claude Code (Opus 4.6)"
```
