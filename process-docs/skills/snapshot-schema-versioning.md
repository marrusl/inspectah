---
name: snapshot-schema-versioning
description: Snapshot JSON schema is version-gated by exact match (MIN_SCHEMA == SCHEMA_VERSION) -- not a rolling acceptance window.
---

# Snapshot Schema Versioning

`InspectionSnapshot` in `crates/core/src/snapshot.rs` carries a
`schema_version` field. The loading contract is an exact match:

```rust
pub const SCHEMA_VERSION: u32 = 29;

// ...

const MIN_SCHEMA: u32 = 29;

if snap.schema_version < Self::MIN_SCHEMA || snap.schema_version > SCHEMA_VERSION {
    return Err(SnapshotError::UnsupportedVersion(snap.schema_version));
}
```

`MIN_SCHEMA` and `SCHEMA_VERSION` are independent constants, not tied
together by the type system -- they just currently hold the same value
(both 29). Only `schema_version == 29` loads; any other value is
rejected. Snapshots on an older schema must be re-scanned -- there is no
migration path, and with the window closed to an exact match there is no
longer even a one-release grace period for a prior version.

**Raise `MIN_SCHEMA` with `SCHEMA_VERSION`, and decide it explicitly.**
Bumping `SCHEMA_VERSION` alone widens the window: 29 with `MIN_SCHEMA`
still 28 accepts both. Ask what the prior version *is* before leaving it
accepted, and derive the answer rather than reusing the last one.

Six bumps, two different arguments, all landing on "reject":

- 23 -> 24 added a cross-inspector annotation written once, at collect
  time, so a schema-23 snapshot is by definition one collected before
  that pass ran. It deserializes cleanly; accepting it would mean
  re-rendering from the state the pass exists to remove, with no field
  that tells the two apart. **Semantic staleness.**
- 24 -> 25 renamed and retyped a field on three `VarDirBacking`
  variants, so a schema-24 payload does not deserialize at all. Beyond
  the mechanics, the new value is a collect-time observation (which
  file on the host carried a directive) that nothing at load time could
  derive from the rest of the snapshot. **Unreadable and
  unupgradable.**
- 25 -> 26 added the `content_not_captured` advisory, so the config
  inspector records a file it could not read instead of an included
  entry with empty content. A schema-25 payload parses, but a failed
  read in it is that included empty entry, and an export copies it over
  the real file. Nothing in the payload tells it from a file that is
  really empty. **Semantic staleness**, with a correctness cost: the
  stale snapshot is not just less informative, it exports a wrong file.
- 26 -> 27 removed `kernel_boot.sysctl_overrides` (and the
  `SysctlOverride` type) in favour of `kernel_boot.sysctl`, which holds
  the files on systemd-sysctl's search path and the running values and
  resolves them by its precedence. A schema-26 payload still parses:
  serde drops the unknown key, so a removed field alone would not have
  forced anything. It is refused because it predates the precedence
  model. Its overrides were read per key with no notion of one file
  shadowing another, so a source it names may be a file the host never
  applied, and it carries none of the files the new section is built
  from. **Semantic staleness**, the removed field being the visible
  part. The refusal tests in all three loaders read one copy of that
  payload, `testdata/fixtures/snapshot/schema-26-sysctl-overrides.json`,
  so the next bump has a template for keeping the closed version's
  payload out of the test bodies.
- 27 -> 28 renamed the `services` section to `units`. A schema-27
  payload parses (serde drops the unknown `services` key), so the
  rename alone would leave an empty section, not an error. It is
  **unreadable**, since its section sits under a key nothing reads, and
  **semantically stale**: it was collected when the section held
  services only, so every socket and timer state the host had is absent
  from it with nothing to say so. Its refusal tests read
  `testdata/fixtures/snapshot/schema-27-services-section.json`, and
  each first loads the same section moved under `units` at the current
  version, so the test proves the version gate refuses it and not a
  broken payload.
- 28 -> 29 added `rpm.dep_closures`, each leaf's and auto package's
  dependency closure, leaves included, and, for an aggregate,
  `units.enabled_on_any_host`. A
  schema-28 payload parses, both fields defaulting to absent, and a
  carried auto package's dependencies then read as not installed: listed
  as not carried, left out of the disabled-repo install that needs them,
  their recorded `systemctl` commands dropped. The closure is a
  collect-time observation nothing at load time can rebuild from leaf
  closures, since sharing a closure does not say which package needs
  which. **Semantic staleness.** Its refusal tests read
  `testdata/fixtures/snapshot/schema-28-leaf-closures-only.json`, and each
  first loads the same payload with the closures at the current version.

The second argument does not follow from the first. A renamed field
would force the bump even if the data were recoverable, and stale
semantics force it even when the payload parses. Check both.

### When to Bump the Version

Bump `SCHEMA_VERSION` whenever you change the `InspectionSnapshot`
struct in a way that changes the JSON shape:

- Adding a new `Option<T>` field with `#[serde(default)]` is safe
  without a bump (old JSON deserializes with `None`/default).
- Removing a field, renaming a field, or changing a field's type
  **requires** a bump.
- Adding a **variant to an existing serialized enum** requires a bump.
  Old readers reject the new string; and when the variant records a
  decision the collector makes, a snapshot without it is not merely
  missing a value, it is a snapshot from before that decision existed.
- Adding a new `SectionData` variant **does not** require a bump
  (new sections are `Option` fields on the snapshot, defaulting to
  `None` for old JSON).

### What a Bump Touches Besides the Constant

`cargo test --workspace` finds most of these, but knowing them up front
saves a round trip. Test files embed `"schema_version": N` as a raw-JSON
or `json!` literal in `crates/cli/src/commands/aggregate.rs`,
`crates/cli/tests/refine_e2e_test.rs` and
`crates/refine/tests/normalize_test.rs`, and
`crates/core/tests/snapshot_test.rs` plus the `snapshot.rs` unit tests
name specific accepted and rejected versions.
`testdata/e2e-fixture.tar.gz` holds a serialized snapshot that
`crates/refine/tests/testdata_tarballs_load.rs` loads, so it has to be
rewritten at the new version: change the one `schema_version` value in
its `inspection-snapshot.json` and re-pack it, leaving out any macOS
`._` AppleDouble member and xattr headers a Finder or `bsdtar` pack adds.

Nothing fails when these go stale: this file,
`docs/reference/snapshot-schema.md` (it states the current version and
quotes the refusal message), and the literals in
`scripts/tests/check-sensitive-tarballs-test.sh`, which that script does
not check but which should not read as an old snapshot.

A bump also wants a test that the version just closed is refused
through every production loader, with a payload that shows why it is
refused: `InspectionSnapshot::load` (`crates/core/tests/snapshot_test.rs`),
refine's `tarball::from_tarball`, which goes through `load_for_refine`
(`crates/refine/tests/tarball_test.rs`), and `inspectah aggregate`'s
`load_snapshot_from_tarball` (unit tests in
`crates/cli/src/commands/aggregate.rs`).

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
