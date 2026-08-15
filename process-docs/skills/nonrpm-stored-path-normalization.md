---
name: nonrpm-stored-path-normalization
description: Non-RPM software paths are stored relative with `trim_start_matches('/')` and rendered back with exactly one leading slash — `strip_prefix('/')` is wrong on both sides.
---

# Non-RPM Stored Path Normalization

## The Rule

`NonRpmItem.path` and its siblings are stored **relative** — every leading
slash removed:

```rust
let path = prefix.trim_start_matches('/').to_string();
```

Renderers restore exactly one, through the single helper that owns the
convention (`absolute_path()` in
`crates/pipeline/src/render/language_packages.rs`):

```rust
format!("/{}", stored_path.trim_start_matches('/'))
```

Both halves trim. Never `strip_prefix('/')`.

## Why `strip_prefix` Is Wrong

`strip_prefix` removes **one** slash. `trim_start_matches` removes **all**.
Mixing them diverges on any doubled root, which `npm root -g` and
`gem environment gemdir` will happily report because they echo whatever the
prefix config holds:

| Input | `strip_prefix` stores | `trim_start_matches` stores |
|---|---|---|
| `/usr/lib/node_modules` | `usr/lib/node_modules` | `usr/lib/node_modules` |
| `//usr/lib/node_modules` | `/usr/lib/node_modules` | `usr/lib/node_modules` |

A `strip_prefix`-stored doubled root renders as `//usr/lib/node_modules`,
which no `COPY` line resolves, while the same path from a trimming
collector renders correctly.

## Where the Sites Are

- Collectors: ten sites in `crates/collect/src/inspectors/nonrpm.rs` (npm
  global, system gems, venvs, site-packages, and the directory walks).
- Renderer: `absolute_path()` in
  `crates/pipeline/src/render/language_packages.rs`, called at every site
  that needs the absolute form.

`configtree.rs` also trims leading slashes, but on a different data path
(config file tree materialization). It is not part of this contract.

## History

The nit list recorded this backwards — it named `strip_prefix` as the
convention and asked for the two trimming outliers to be converted. The
code says the opposite: eight of ten collector sites and all six renderer
sites trimmed, and the comment above the gem site describes trimming
behaviour. Corrected 2026-08-15; if you find a `strip_prefix('/')` on a
non-RPM path, it is a bug, not a convention.
