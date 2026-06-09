---
name: snapshot-schema-versioning
description: Snapshot JSON schema is version-gated with no forward/backward compatibility -- version must match exactly.
---

# Snapshot Schema Versioning

`InspectionSnapshot` in `inspectah-core/src/snapshot.rs` carries a
`schema_version` field (currently 18). The loading contract is strict:

```rust
const MIN_SCHEMA: u32 = SCHEMA_VERSION; // same as current

if snap.schema_version < Self::MIN_SCHEMA || snap.schema_version > SCHEMA_VERSION {
    return Err(SnapshotError::UnsupportedVersion(snap.schema_version));
}
```

This means `MIN_SCHEMA == SCHEMA_VERSION` -- only the current version
loads. There is no migration path and no backward compatibility window.
Older snapshots must be re-scanned.

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

### Fleet Snapshots Share the Schema

Fleet aggregation (`inspectah fleet`) reads individual host snapshots
and produces a merged output. The fleet metadata
(`FleetSnapshotMeta`) is stored on the same `InspectionSnapshot`
struct. If you bump the schema version, fleet re-aggregation also
requires re-scanning all constituent hosts.

## Why This Matters

If you add a required field without `serde(default)`, all existing
snapshots on disk become unloadable with an opaque serde error instead
of the clean `UnsupportedVersion` message. The `MIN_SCHEMA == current`
policy means there is no grace period -- get the serde annotations
right on the first commit.

## See Also

- `inspectah-core/src/snapshot.rs` -- schema version, `load()`, all fields
- `inspectah-core/src/types/` -- section types referenced by snapshot
- `inspectah-core/src/types/fleet.rs` -- `FleetSnapshotMeta`
