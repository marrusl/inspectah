---
name: serde-include-default-ambiguity
description: Item types used to carry a bare `include: bool` field, which collapsed "absent" and "explicit false" to the same value and needed pre-deserialization patching. Phase 0 replaced those fields with `disposition: FindingKind`, a tagged union whose absent-key default is named per field, closing the ambiguity for every disposition-bearing field.
---

# Serde `include` Field Default Ambiguity (historical) and the `disposition` Patch

Several item types (`PackageEntry`, `ConfigFileEntry`, `EnabledModuleStream`,
`VersionLockEntry`, and others) used to carry a bare `include: bool` field
with this serde annotation:

```rust
#[serde(default = "crate::default_true")]
pub include: bool,
```

The intent was: absent field means "included by default" (`true`). But
serde's `default` attribute fires for *any* missing field, so there was
no way to distinguish "field was absent" from "field was explicitly
`false`" after deserialization. Both produced `false` when the JSON said
`"include": false`, and `true` when the field was missing.

Phase 0 of the refine UI fixup replaced `include: bool` on every one of
these types with `disposition: FindingKind`, a `#[serde(tag = "kind")]`
enum (`Actionable { include }` / `Advisory { .. }` / `Inventory`,
`crates/core/src/types/finding.rs`). This removes the ambiguity: every
disposition-bearing field names its own absent-key default on the field
itself (plain `#[serde(default)]` resolves to `FindingKind::default()`,
which is `included()`; fields that need a different absent-case default
name it explicitly, e.g. `#[serde(default =
"crate::types::finding::default_finding_inventory")]` on
`KernelModule::disposition`). An absent `disposition` key and an
explicit `{"kind": "actionable", "include": false}` now deserialize to
two distinct, unambiguous values -- there is no longer a bool
collapsing both cases to `false`.

## The Patch That Still Exists

`crates/refine/src/normalize.rs` still contains `load_for_refine()`,
which walks the raw JSON *before* typed deserialization and patches any
entry lacking a `disposition` key by inserting
`{"kind": "actionable", "include": true}`. `patch_missing_includes()`
(`normalize.rs`) calls `patch_array_includes()` on three arrays:
`rpm.packages_added`, `rpm.base_image_only`, and `config.files` --
verified by reading both functions directly.

```rust
// From normalize.rs -- the pre-deserialization patch
fn patch_array_includes(parent: &mut Value, array_key: &str) {
    if let Some(Value::Array(entries)) = parent.get_mut(array_key) {
        for entry in entries {
            if let Value::Object(map) = entry
                && !map.contains_key("disposition")
            {
                map.insert(
                    "disposition".into(),
                    serde_json::json!({"kind": "actionable", "include": true}),
                );
            }
        }
    }
}
```

**Any code that loads snapshots for refine MUST use `load_for_refine()`,
not `InspectionSnapshot::load()` directly**, to stay consistent with
this patch path.

## Why This Matters

If you add a new array of disposition-bearing entries that `refine`
loads, add it to `patch_missing_includes()` too, so a `disposition`-less
legacy entry gets the same treatment as the three arrays above rather
than whatever the field's own serde default produces. For a field whose
own default already matches what the patch inserts (actionable,
included) this is belt-and-suspenders; for a field that defaults to
something else (inventory, excluded), relying on the field default
instead of the patch changes behavior, so match the patch to the field's
intended absent-case default or don't add it to this list.

This bug class was silent -- tests with hand-constructed
`InspectionSnapshot` structs set `disposition` directly and never
exercise the deserialization path.

## Evidence

The `load_for_refine` function was introduced specifically to fix this
class of bug. Review threads for the leaf filter implementation
(2026-05-17) documented the same-name ambiguity pattern at the package
identity level, and the normalize layer was audited for correctness as
part of that fix.

## See Also

- `crates/core/src/types/finding.rs` -- `FindingKind`, `default_finding_excluded()`, `default_finding_inventory()`
- `crates/core/src/types/rpm.rs` -- `PackageEntry` definition
- `crates/refine/src/normalize.rs` -- `load_for_refine()`
