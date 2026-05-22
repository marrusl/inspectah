# Go Compatibility Removal — Pre-Spec

**Date:** 2026-05-21
**Status:** Pre-spec (awaiting Mark's additional input)
**Scope:** `rust` branch only. `main` branch Go code untouched until cutover.

## Goal

Clean break from Go heritage on the `rust` branch. Remove all Go compatibility code, Go source files, and Go-related build/CI artifacts. The Rust binary is the product — the branch should reflect that.

## Tier 1: Go Compat Code in Rust (remove)

- `patch_legacy_tie_fields()` in `snapshot.rs` (~60 lines) — pre-patches `tie`/`tie_winner` bools to `VariantSelection` enum for Go-era snapshots (schema v12-v14)
- Parity gate tests in `parity_gate.rs` — tests verifying Rust can deserialize Go v13 golden output
- `normalize.rs` strip logic (~6 lines) — strips `tie`/`tie_winner` fields for Go/Rust diff normalization
- Raise `MIN_SCHEMA` to current Rust floor (16 or 17), delete migration steps for versions below that floor

## Tier 2: Keep (not compat code)

- `Warning.extra: HashMap<String, Value>` with `#[serde(flatten)]` — defensive deserialization, forward-compatible
- `serde(default)` annotations — handles missing fields, useful for forward compat too
- `serde(rename)` on `SystemType` variants — canonical domain values, not Go-specific
- `selinux` JSON key name — renaming would break Rust-to-Rust compat for no benefit

## Tier 3: Go Source Tree & Related Files (delete from rust branch)

- Go source code (`cmd/`, `internal/`, `go.mod`, `go.sum`, etc.)
- Go build scripts, Makefiles, or CI config targeting Go builds
- Go test fixtures that aren't also used by Rust tests
- Go-specific documentation (if any)
- Any Go binary packaging files (spec files for Go binary, COPR config)

## Risk

- **Bounded:** Anyone with Go-generated tarballs (schema v12-v14) would need to re-scan with the Rust binary. Go binary is retired from active use.
- **No user impact on main:** `main` branch is untouched. Go CLI wrapper continues to work until cutover.

## Open

- Mark has additional prespec info to add
- Full spec + brainstorm after prespec is complete

## Assessment

Tang: "Ready. Overdue, not premature." (consult 2026-05-21)
