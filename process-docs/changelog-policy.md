# Changelog Policy

## Rule

Every commit that changes user-facing behavior gets a CHANGELOG.md entry
under `## [Unreleased]` before the work is considered done.

At release time, rename `[Unreleased]` to `[version] - YYYY-MM-DD` and
add a fresh empty `## [Unreleased]` section above it.

## What gets an entry

- New features, CLI flags, output changes
- Bug fixes that affect user-visible behavior
- Breaking changes (flag removals, output format changes, schema bumps)
- Dependency changes that affect supported platforms

## What does NOT get an entry

- Test-only changes (new tests, test refactors)
- CI/CD pipeline changes
- Documentation updates (README, ROADMAP, process docs, skill files)
- Internal refactors with no user-visible effect
- Code style / formatting changes

## Format

Follow [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/).
Categories: Added, Changed, Deprecated, Removed, Fixed, Security.
Each entry: `- **Short label** — description of what changed and why it matters.`
