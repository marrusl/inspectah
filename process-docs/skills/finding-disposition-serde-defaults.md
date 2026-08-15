---
name: finding-disposition-serde-defaults
description: Every `disposition: FindingKind` field must name its serde default explicitly — field-level `serde(default)` uses `FindingKind::default()` (actionable, included) and ignores the struct's own `Default` impl.
---

# `FindingKind` Disposition Serde Defaults

`FindingKind` is the disposition vocabulary carried by findings across the
snapshot:

```rust
pub enum FindingKind {
    Actionable { include: bool },   // Default::default() → include: true
    Advisory { advisory_type, rationale },
    Inventory,
}
```

`FindingKind::default()` is `Actionable { include: true }`. That is the
right default for a finding the user is expected to act on, and the wrong
default for everything else.

## The Rule

**A `disposition: FindingKind` field that is not actionable-by-default must
name its default function.** Bare `#[serde(default)]` resolves to
`FindingKind::default()`:

```rust
// Inventory-only findings (network items).
#[serde(default = "crate::types::finding::default_finding_inventory")]
pub disposition: FindingKind,

// Findings that must not be baked in without an explicit decision.
#[serde(default = "crate::types::finding::default_finding_excluded")]
pub tuned_disposition: FindingKind,
```

Both helpers live in `crates/core/src/types/finding.rs`. Actionable
findings (`PackageEntry`, `ConfigFileEntry`, `SysctlOverride`,
`ServiceStateChange`, …) correctly keep bare `#[serde(default)]` — for
those, included-by-default is the intended contract.

## Why a Struct `Default` Impl Does Not Cover You

`NMConnection` has a hand-written `impl Default` that sets
`disposition: FindingKind::inventory()`. That impl **does not** participate
in deserialization of a missing field. Serde's field-level `default`
attribute calls `Default::default()` on the *field's* type, never on the
containing struct. A struct `Default` impl only applies when serde is told
to use it at the container level (`#[serde(default)]` on the struct
itself), which none of these types do.

So a struct-level `Default` that looks like it encodes the intended
disposition is decorative as far as JSON is concerned. The field attribute
is the only thing that matters.

## Why It Matters

Live scans are safe: collectors set every disposition explicitly. The
exposure is the **`--from-snapshot` re-render path** and any hand-edited or
externally produced snapshot JSON. An absent `disposition` key silently
became `Actionable { include: true }`, which turns inventory-only network
items and undecided tuned profiles into content that renders into the
Containerfile.

The failure is silent in both directions: nothing errors, and tests that
build snapshot structs in Rust set the disposition directly and never
exercise deserialization. **Regression tests for this class must go through
JSON with the key absent**, ideally through `InspectionSnapshot::load()`.

## When Adding a New Disposition-Bearing Field

1. Decide the disposition an absent key should mean.
2. If it is anything other than actionable-included, point
   `#[serde(default = "…")]` at the matching helper (add one to
   `finding.rs` if a new default class appears).
3. Add a test that deserializes JSON **without** the key and asserts the
   intended variant.

## Evidence

Found during the v0.9.0-beta.2 extended-findings gap audit:
`default_finding_excluded()` and `default_finding_inventory()` had been
written for exactly this purpose and had zero call sites, while
`tuned_disposition` and the three network `disposition` fields all used
the bare attribute.

## See Also

- `crates/core/src/types/finding.rs` — `FindingKind`, both default helpers
- `crates/core/src/types/network.rs` — `NMConnection`, `FirewallZone`,
  `FirewallDirectRule`
- `crates/core/src/types/kernelboot.rs` — `tuned_disposition`
- [serde-include-default-ambiguity](serde-include-default-ambiguity.md) —
  the older `include: bool` variant of the same class of bug
