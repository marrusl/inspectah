---
name: serde-include-default-ambiguity
description: The `include` field uses `serde(default)` with `default_true`, creating an ambiguity between absent and explicit-false that requires pre-deserialization patching.
---

# Serde `include` Field Default Ambiguity

Several item types (`PackageEntry`, `ConfigEntry`, `EnabledModuleStream`,
`VersionLockEntry`) carry an `include: bool` field with this serde
annotation:

```rust
#[serde(default = "crate::default_true")]
pub include: bool,
```

The intent is: absent field means "included by default" (`true`). But
serde's `default` attribute fires for *any* missing field, so there is
no way to distinguish "field was absent" from "field was explicitly
`false`" after deserialization. Both produce `false` when the JSON says
`"include": false`, and `true` when the field is missing -- but the
refine layer needs to know *which* case it is.

## The Fix That Exists

`crates/refine/src/normalize.rs` contains `load_for_refine()`, which
walks the raw JSON *before* typed deserialization and patches any entry
lacking an `include` key by inserting `"include": true`. This preserves
an existing `"include": false` while ensuring absent fields get the
correct default.

```rust
// From normalize.rs -- the pre-deserialization patch
fn patch_array_includes(parent: &mut Value, array_key: &str) {
    if let Some(Value::Array(entries)) = parent.get_mut(array_key) {
        for entry in entries {
            if let Value::Object(map) = entry
                && !map.contains_key("include")
            {
                map.insert("include".into(), Value::Bool(true));
            }
        }
    }
}
```

**Any code that loads snapshots for refine MUST use `load_for_refine()`,
not `InspectionSnapshot::load()` directly.** Using `load()` bypasses
the patch and silently collapses absent-include to `false`.

## Why This Matters

If you add a new item type with an `include` field, or add `include` to
an existing type, you must also update `patch_missing_includes()` in
`normalize.rs` to cover the new array. Otherwise the refine layer will
treat all items without an explicit `include` in their JSON as excluded.

This bug is silent -- tests with hand-constructed `InspectionSnapshot`
structs set `include: true` directly and never exercise the
deserialization path.

## Evidence

The `load_for_refine` function was introduced specifically to fix this
class of bug. Review threads for the leaf filter implementation
(2026-05-17) documented the same-name ambiguity pattern at the package
identity level, and the normalize layer was audited for correctness as
part of that fix.

## See Also

- `crates/core/src/lib.rs` -- `default_true()` and `is_false()`
- `crates/core/src/types/rpm.rs` -- `PackageEntry` definition
- `crates/refine/src/normalize.rs` -- `load_for_refine()`
