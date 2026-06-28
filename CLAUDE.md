# inspectah

Migration analysis tool: package-mode RHEL/CentOS/Fedora → image-mode (bootc).

## Orientation

Read `process-docs/skills/codebase-layout.md` first — it maps the full workspace structure (crates, commands, inspectors, renderers, tests, docs).

Read `process-docs/skills/index.md` for non-obvious patterns and correctness requirements discovered during development.

## Key Conventions

- **Clippy clean:** `cargo clippy -- -W clippy::all` with zero warnings. Non-negotiable.
- **Format:** `cargo fmt --check` must pass.
- **Commit format:** `type(scope): description` in imperative mood. Attribution: `Assisted-by: Claude Code (<model>)`.
- **Attribution:** LLM-assisted commits include `Assisted-by: <tool> (<model>)` (e.g., `Assisted-by: Claude Code (Opus 4.6)`). No other identifiers.
- **Schema versioning:** Snapshot JSON has a `schema_version` field. Bump it when types change. See `process-docs/skills/snapshot-schema-versioning.md`.
- **Package identity:** Always `name.arch`, never bare names. See `process-docs/skills/package-identity-is-name-dot-arch.md`.
- **Specs and plans** go in `process-docs/specs/` and `process-docs/plans/`, not `docs/` (which is for GitHub Pages user-facing docs).
- **Pre-commit hook:** `.githooks/pre-commit` gates on `cargo fmt --check`, `cargo clippy`, and `cargo test`. New clones need `git config core.hooksPath .githooks`. Bypass with `--no-verify` as a last resort.
- **Skill file maintenance:** If you add, remove, or rename crates, CLI commands, inspector modules, or major directories, update `process-docs/skills/codebase-layout.md`. If your work reveals a non-obvious pattern or correctness requirement, capture it in a skill file (new or existing) and update `process-docs/skills/index.md`.
