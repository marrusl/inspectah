# Crates Reorganization Plan

Move all workspace crate directories under `crates/` and drop the
`inspectah-` prefix from directory names. Package names in Cargo.toml
stay unchanged (`inspectah-core`, `inspectah-cli`, etc.). Clean up dead
Go and Python artifacts.

## Before / After

```
BEFORE                          AFTER
inspectah-cli/                  crates/cli/
inspectah-collect/              crates/collect/
inspectah-core/                 crates/core/
inspectah-pipeline/             crates/pipeline/
inspectah-refine/               crates/refine/
inspectah-tui/                  crates/tui/
inspectah-web/                  crates/web/
cmd/                            (deleted)
.venv/                          (deleted)
.pytest_cache/                  (deleted)
```

## Tasks

### 1. Move crate directories

```
mkdir crates
git mv inspectah-core     crates/core
git mv inspectah-collect  crates/collect
git mv inspectah-pipeline crates/pipeline
git mv inspectah-cli      crates/cli
git mv inspectah-web      crates/web
git mv inspectah-refine   crates/refine
git mv inspectah-tui      crates/tui
```

### 2. Update workspace root Cargo.toml

Change `[workspace] members` from:

```toml
members = [
    "inspectah-core",
    "inspectah-collect",
    "inspectah-pipeline",
    "inspectah-cli",
    "inspectah-web",
    "inspectah-refine",
    "inspectah-tui",
]
```

to:

```toml
members = [
    "crates/core",
    "crates/collect",
    "crates/pipeline",
    "crates/cli",
    "crates/web",
    "crates/refine",
    "crates/tui",
]
```

### 3. Update inter-crate path dependencies in all crate Cargo.toml files

Every `path = "../inspectah-X"` becomes `path = "../X"` (relative paths
within `crates/` stay one level up). Affected files:

| File | Dependencies to update |
|---|---|
| `crates/cli/Cargo.toml` | inspectah-core, inspectah-collect, inspectah-pipeline, inspectah-web, inspectah-refine, inspectah-tui (deps + dev-deps) |
| `crates/collect/Cargo.toml` | inspectah-core |
| `crates/pipeline/Cargo.toml` | inspectah-core, inspectah-collect |
| `crates/refine/Cargo.toml` | inspectah-core, inspectah-pipeline |
| `crates/web/Cargo.toml` | inspectah-core, inspectah-pipeline, inspectah-refine |
| `crates/tui/Cargo.toml` | inspectah-core, inspectah-refine (check for inspectah-pipeline too) |
| `crates/core/Cargo.toml` | No inter-crate deps (leaf crate) |

### 4. Update .gitignore

Replace the crate allowlist block:

```gitignore
!/inspectah-core/
!/inspectah-collect/
!/inspectah-pipeline/
!/inspectah-cli/
!/inspectah-web/
!/inspectah-refine/
!/inspectah-tui/
```

with:

```gitignore
!/crates/
```

### 5. Update GitHub Actions workflows

**`.github/workflows/rust-ci.yml`** -- update path triggers:

```yaml
# Old
- 'inspectah-*/src/**'
- 'inspectah-*/tests/**'
- 'inspectah-*/Cargo.toml'
- 'inspectah-web/ui/src/**'
- 'inspectah-web/ui/package.json'
- 'inspectah-web/ui/package-lock.json'
- 'inspectah-web/ui/vite.config.ts'
- 'inspectah-web/ui/tsconfig.json'
- 'inspectah-web/ui/index.html'
- 'inspectah-web/ui/e2e/**'
- 'inspectah-web/ui/playwright.config.ts'
- 'inspectah-web/build.rs'

# New
- 'crates/*/src/**'
- 'crates/*/tests/**'
- 'crates/*/Cargo.toml'
- 'crates/web/ui/src/**'
- 'crates/web/ui/package.json'
- 'crates/web/ui/package-lock.json'
- 'crates/web/ui/vite.config.ts'
- 'crates/web/ui/tsconfig.json'
- 'crates/web/ui/index.html'
- 'crates/web/ui/e2e/**'
- 'crates/web/ui/playwright.config.ts'
- 'crates/web/build.rs'
```

Also update `cache-dependency-path` and `working-directory` references:

- `inspectah-web/ui/package-lock.json` -> `crates/web/ui/package-lock.json`
- `working-directory: inspectah-web/ui` -> `working-directory: crates/web/ui`

**`.github/workflows/build-binary.yml`** -- update path triggers:

```yaml
# Old
- 'inspectah-*/src/**'
- 'inspectah-*/Cargo.toml'
- 'inspectah-web/ui/src/**'
- 'inspectah-web/ui/package.json'

# New
- 'crates/*/src/**'
- 'crates/*/Cargo.toml'
- 'crates/web/ui/src/**'
- 'crates/web/ui/package.json'
```

Also update `cache-dependency-path`:

- `inspectah-web/ui/package-lock.json` -> `crates/web/ui/package-lock.json`

**`.github/workflows/package-release.yml`** -- no crate-path references
(uses `cargo build -p inspectah-cli` which resolves by package name,
not directory). No changes needed.

### 6. Update COPR Makefile

In `.copr/Makefile`, update the web UI pre-build path:

```makefile
# Old
cd $(STAGING)/$(NAME)-$(VERSION)/inspectah-web/ui && \

# New
cd $(STAGING)/$(NAME)-$(VERSION)/crates/web/ui && \
```

### 7. Update developer docs

**`docs/contributing/developer-guide.md`** -- update the workspace
structure table. The crate names in the table stay as-is (they are
Cargo package names), but add a note about the `crates/` directory
layout. Also update the dependency description paragraph to mention
`crates/` as the containing directory.

**`docs/explanation/architecture.md`** -- update the workspace table
and section headers. Same approach: package names stay, but clarify
that directories live under `crates/`. The section headers
(`## inspectah-core: the shared language`, etc.) use package names
and can stay unchanged.

**`docs/contributing/adding-an-inspector.md`** -- update file path
references:

- `inspectah-core/src/types/` -> `crates/core/src/types/`
- `inspectah-core/src/types/mod.rs` -> `crates/core/src/types/mod.rs`
- `inspectah-core/src/types/completeness.rs` -> `crates/core/src/types/completeness.rs`
- `inspectah-collect/src/inspectors/` -> `crates/collect/src/inspectors/`
- `inspectah-collect/src/inspectors/mod.rs` -> `crates/collect/src/inspectors/mod.rs`

Also update code comment paths in examples (e.g.,
`// inspectah-core/src/types/firewall.rs` ->
`// crates/core/src/types/firewall.rs`).

### 8. Update process-docs skill files

These files reference crate directory paths and should be updated so
future agents get correct paths:

- `process-docs/skills/package-identity-is-name-dot-arch.md`
- `process-docs/skills/snapshot-schema-versioning.md`
- `process-docs/skills/two-wave-collection.md`
- `process-docs/skills/serde-include-default-ambiguity.md`
- `process-docs/skills/fleet-vs-single-host-behavioral-split.md`

Pattern: `inspectah-X/src/` -> `crates/X/src/`

### 9. Do NOT update process-docs/plans/

Existing plans (e.g., `fleet-leaf-intersection.md`,
`refine-projection-consolidation.md`, etc.) are historical documents
that were written against the old layout. Updating them would rewrite
history. Leave them as-is -- they describe what was true when they were
written.

### 10. Remove dead Go artifacts

```
rm -rf cmd/
```

Contains compiled Go binaries (`inspectah`, `inspectah-mac`,
`inspectah-linux-arm64`, `inspectah-linux-amd64`, `inspector.test`) --
all obsolete since the Rust rewrite.

### 11. Remove dead Python artifacts

```
rm -rf .venv/
rm -rf .pytest_cache/
```

Both are remnants of the original Python implementation.

### 12. Verify

Run the following from the repo root. All must pass:

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo doc --workspace --no-deps
cargo fmt --all -- --check
```

Also confirm the binary runs:

```bash
cargo run -p inspectah-cli -- --version
```

## Scope notes

- **Package names are unchanged.** `inspectah-core`, `inspectah-cli`,
  etc. remain the Cargo package names in each crate's `[package] name`.
  Only directory paths change.
- **The software-architecture diagram (`docs/diagrams/software-architecture.html`)
  does not need updates.** It uses short IDs (`core`, `cli`, etc.)
  internally and displays package names (`inspectah-core`, etc.) as
  labels. Neither references directory paths.
- **Completions files (`completions/`) are unaffected.** They reference
  the binary name `inspectah`, not crate directories.
- **The RPM spec (`packaging/inspectah.spec`) is unaffected.** It uses
  `cargo build -p inspectah-cli` (package name, not directory path) and
  `target/release/inspectah` (binary output path). Both are unchanged.
- **`scripts/build-codemirror.sh` is unaffected.** It references
  `src/inspectah/static/codemirror` (a Python-era path), not crate
  directories.
- **`scripts/host-validation.sh` and `scripts/verify/` are unaffected.**
  They reference binary paths and JSON output, not crate directories.
