# inspectah

Migration analysis tool: package-mode RHEL/CentOS/Fedora → image-mode (bootc).

## Orientation

Read `process-docs/skills/codebase-layout.md` first — it maps the full workspace structure (crates, commands, inspectors, renderers, tests, docs).

Read `process-docs/skills/index.md` for non-obvious patterns and correctness requirements discovered during development.

## Key Conventions

- **Clippy clean:** `cargo clippy -- -W clippy::all` with zero warnings. Non-negotiable.
- **Format:** `cargo fmt --check` must pass.
- **Commit format:** `type(scope): description` in imperative mood. Attribution: `Assisted-by: Claude Code (<model>)`.
- **No names:** This is a public repo. Never reference AI team member names in commits, code, or docs.
- **Schema versioning:** Snapshot JSON has a `schema_version` field. Bump it when types change. See `process-docs/skills/snapshot-schema-versioning.md`.
- **Package identity:** Always `name.arch`, never bare names. See `process-docs/skills/package-identity-is-name-dot-arch.md`.
- **Specs and plans** go in `process-docs/specs/` and `process-docs/plans/`, not `docs/` (which is for GitHub Pages user-facing docs).
