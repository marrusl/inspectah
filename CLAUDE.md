# inspectah

Migration analysis tool: package-mode RHEL/CentOS/Fedora → image-mode (bootc).

## Orientation

Read `process-docs/skills/codebase-layout.md` first — it maps the full workspace structure (crates, commands, inspectors, renderers, tests, docs).

Read `process-docs/skills/index.md` for non-obvious patterns and correctness requirements discovered during development.

## Key Conventions

- **Clippy clean:** `cargo clippy --workspace --all-targets -- -D clippy::all` with zero warnings. Non-negotiable. `--all-targets` is part of the policy, not an optimisation: without it clippy checks lib and bin only, and test targets are where warnings accumulate unseen.
- **Format:** `cargo fmt --check` must pass.
- **Commit format:** `type(scope): description` in imperative mood.
- **Attribution:** LLM-assisted commits include `Assisted-by: <tool> (<model>)` (e.g., `Assisted-by: Claude Code (Opus 4.6)`). No other identifiers.
- **Schema versioning:** Snapshot JSON has a `schema_version` field. Bump it when types change. See `process-docs/skills/snapshot-schema-versioning.md`.
- **Package identity:** Always `name.arch`, never bare names. See `process-docs/skills/package-identity-is-name-dot-arch.md`.
- **Specs and plans** go in `process-docs/specs/` and `process-docs/plans/`, not `docs/` (which is for GitHub Pages user-facing docs).
- **Pre-commit hook:** `.githooks/pre-commit` runs `cargo fmt --check`, `cargo clippy` and `cargo test`, one output line per gate, on every commit that changes a path outside the docs-only list in `.githooks/docs-only-paths`; deletions, renames, snapshots, fixtures and UI files are all gated. Before fmt and again after test it refuses while any non-docs path differs from the index being committed (unstaged or untracked). A docs-only commit skips the gates and is never refused for other files. The hook unsets `INSPECTAH_SKIP_UI`. New clones need `git config core.hooksPath .githooks`. Bypass with `--no-verify` as a last resort.
- **Skill file maintenance:** If you add, remove, or rename crates, CLI commands, inspector modules, or major directories, update `process-docs/skills/codebase-layout.md`. If your work reveals a non-obvious pattern or correctness requirement, capture it in a skill file (new or existing) and update `process-docs/skills/index.md`.
