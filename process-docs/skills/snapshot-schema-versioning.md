---
name: snapshot-schema-versioning
description: Snapshot JSON schema is version-gated across a rolling two-version acceptance window (MIN_SCHEMA through SCHEMA_VERSION) -- not an exact match.
---

# Snapshot Schema Versioning

`InspectionSnapshot` in `crates/core/src/snapshot.rs` carries a
`schema_version` field. The loading contract accepts a two-version window:

```rust
pub const SCHEMA_VERSION: u32 = 22;

// ...

const MIN_SCHEMA: u32 = 21;

if snap.schema_version < Self::MIN_SCHEMA || snap.schema_version > SCHEMA_VERSION {
    return Err(SnapshotError::UnsupportedVersion(snap.schema_version));
}
```

`MIN_SCHEMA` and `SCHEMA_VERSION` are independent constants, not tied
together. Currently `SCHEMA_VERSION` is 22 and `MIN_SCHEMA` is 21, so
snapshots with `schema_version` 21 or 22 both load; only versions outside
that window are rejected. Snapshots older than `MIN_SCHEMA` must still be
re-scanned -- there is no unbounded migration path, just whatever gap the
two constants currently leave between them.

The planned v0.9.0-beta.3 work
(`process-docs/plans/2026-08-15-usr-walk-presentation-plan.md`, Task 1)
bumps `SCHEMA_VERSION` to 23 and closes `MIN_SCHEMA` to 23 as well,
narrowing the window back to an exact match; that plan's Task 13 updates
this skill again when it lands.

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
of the clean `UnsupportedVersion` message. The two-version window only
covers one prior release -- get the serde annotations right on the
first commit.

## See Also

- `crates/core/src/snapshot.rs` -- schema version, `load()`, all fields
- `crates/core/src/types/` -- section types referenced by snapshot
- `crates/core/src/types/aggregate.rs` -- `AggregateSnapshotMeta`
