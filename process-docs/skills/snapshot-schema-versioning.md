---
name: snapshot-schema-versioning
description: Snapshot JSON schema is version-gated by exact match (MIN_SCHEMA == SCHEMA_VERSION) -- not a rolling acceptance window.
---

# Snapshot Schema Versioning

`InspectionSnapshot` in `crates/core/src/snapshot.rs` carries a
`schema_version` field. The loading contract is an exact match:

```rust
pub const SCHEMA_VERSION: u32 = 23;

// ...

const MIN_SCHEMA: u32 = 23;

if snap.schema_version < Self::MIN_SCHEMA || snap.schema_version > SCHEMA_VERSION {
    return Err(SnapshotError::UnsupportedVersion(snap.schema_version));
}
```

`MIN_SCHEMA` and `SCHEMA_VERSION` are independent constants, not tied
together by the type system -- they just currently hold the same value
(both 23). Only `schema_version == 23` loads; any other value is
rejected. Snapshots on an older schema must be re-scanned -- there is no
migration path, and with the window closed to an exact match there is no
longer even a one-release grace period for a prior version.

### When to Bump the Version

Bump `SCHEMA_VERSION` whenever you change the `InspectionSnapshot`
struct in a way that changes the JSON shape:

- Adding a new `Option<T>` field with `#[serde(default)]` is safe
  without a bump (old JSON deserializes with `None`/default).
- Removing a field, renaming a field, or changing a field's type
  **requires** a bump.
- Adding a new `SectionData` variant **does not** require a bump
  (new sections are `Option` fields on the snapshot, defaulting to
  `None` for old JSON).

### The serde(default) / skip_serializing_if Pattern

New `Option` fields on `InspectionSnapshot` follow a consistent
pattern:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub new_field: Option<NewType>,
```

For boolean flags:

```rust
#[serde(default, skip_serializing_if = "crate::is_false")]
pub new_flag: bool,
```

This keeps serialized JSON minimal (omitting `null` and `false` values)
while ensuring older JSON without these fields deserializes cleanly.
Missing either annotation breaks one direction of the roundtrip.

### Aggregate Snapshots Share the Schema

Aggregate aggregation (`inspectah aggregate`) reads individual host snapshots
and produces a merged output. The aggregate metadata
(`AggregateSnapshotMeta`) is stored on the same `InspectionSnapshot`
struct. If you bump the schema version, aggregate re-aggregation also
requires re-scanning all constituent hosts.

## Why This Matters

If you add a required field without `serde(default)`, all existing
snapshots on disk become unloadable with an opaque serde error instead
of the clean `UnsupportedVersion` message. With the window closed to an
exact match, a snapshot from the prior release no longer loads at all --
get the serde annotations right on the first commit, and re-scan rather
than rely on a grace period that no longer exists.

## See Also

- `crates/core/src/snapshot.rs` -- schema version, `load()`, all fields
- `crates/core/src/types/` -- section types referenced by snapshot
- `crates/core/src/types/aggregate.rs` -- `AggregateSnapshotMeta`
