---
name: package-identity-is-name-dot-arch
description: Package identity is canonical `name.arch` format throughout the codebase. Using bare package names causes multiarch collisions and silent data corruption.
---

# Package Identity Is `name.arch`

Packages in inspectah are identified by canonical `name.arch` strings
(e.g., `httpd.x86_64`, `glibc.i686`). This is enforced across three
crate boundaries:

- **inspectah-collect**: Emits `leaf_packages`, `auto_packages`, and
  `leaf_dep_tree` using `name.arch` keys.
- **inspectah-refine**: `canonical_package_id()` in `session.rs`
  produces the `name.arch` format. `ItemId::Package { name, arch }` is
  the typed identity used for all operations.
- **inspectah-pipeline**: Containerfile rendering uses `name.arch` for
  the `RUN dnf install -y` line when multiarch disambiguation is needed.

```rust
// crates/refine/src/session.rs
fn canonical_package_id(name: &str, arch: &str) -> String {
    format!("{name}.{arch}")
}
```

## Why This Matters

Using bare package names (just `name` without `.arch`) causes two
concrete bugs:

1. **Multiarch collision**: `glibc.x86_64` and `glibc.i686` collapse
   into a single `glibc` entry. One silently disappears from triage,
   counts, and Containerfile output.
2. **Leaf classification leak**: The dependency tree maps leaf packages
   to their dependencies. If keys are bare names, a package that is a
   leaf on one arch can be misclassified as a dependency of the
   same-named package on another arch.

In aggregate/merged snapshots, the same package name can appear with
different arches across hosts. The `name.arch` format keeps these
distinct.

## The Rule

When constructing package identities for:
- HashMap keys
- Set membership checks
- Display in Containerfile `dnf install` lines
- Comparison across collector/refine/pipeline boundaries

Always use `canonical_package_id(name, arch)` or the equivalent
`format!("{name}.{arch}")`. Never use `name` alone.

The `ItemId::Package { name, arch }` enum variant enforces this at the
type level for refine operations. Use it rather than string keys when
possible.

## Evidence

The leaf filter implementation (2026-05-17 comms thread) required a
multi-slice fix across collect, refine, and pipeline specifically
because the original code used bare package names. The fix locked
canonical `name.arch` at the collector contract boundary and propagated
it through all downstream consumers. Reviews explicitly verified that
multiarch packages no longer collapse.

## See Also

- `crates/refine/src/session.rs` -- `canonical_package_id()`
- `crates/collect/src/inspectors/rpm/mod.rs` -- collector leaf output
- `crates/refine/src/normalize.rs` -- `collect_dep_tree_names()`
- `serde-include-default-ambiguity.md` -- related deserialization pitfall
