# Changelog Policy

## Rule

Every change merged to main that affects user-visible behavior gets a
CHANGELOG.md entry under `## [Unreleased]` before the work is considered
done. For feature branches, the entry is written at merge time (not on
every individual commit).

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
The file is for people reading what changed, not a log of commits.

- **Sections**: `[Unreleased]` on top, then one section per release,
  latest first, headed `## [version] - YYYY-MM-DD` (ISO date).
- **Link references**: every version heading has a matching `[version]: <compare URL>`
  line at the bottom of the file, including `[Unreleased]`. Add the new
  release's reference when you cut it.
- **Categories**: only Added, Changed, Deprecated, Removed, Fixed,
  Security, in that order, with empty ones left out. Do not add others
  (`Breaking`, `Known Issues`). A breaking change goes under Changed or
  Removed and its label starts with `Breaking:`. A known issue goes in the
  docs or the issue tracker, not the changelog.
- **Net change**: `[Unreleased]` describes the net difference since the
  last release. A regression introduced and fixed inside `[Unreleased]`
  gets no entry; neither does a feature added and then reworked there,
  which gets one entry describing the end state.

Each entry: `- **Short label**: description of what changed and why it matters.`
Entries written before 2026-11-04 use an em dash after the label; leave
them as they are.

## Length

Lead with what the user sees or can now do, then stop.

- **Target**: one or two sentences, about 40 words.
- **Ceiling**: 80 words. An entry over the ceiling is not done; move
  detail out until it fits. There is no exemption for security or breaking
  entries; they follow the same ceiling, and a breaking entry spends its
  words on what the user must do.
- **First sentence**: the user-facing outcome, in the user's terms (the
  command, flag or output they see), not the code's.

## Where the detail goes

The entry says what changed. Everything about how and why it happened goes
elsewhere:

| Detail | Goes in |
|---|---|
| Mechanism, root cause, how it was found | the commit message |
| File paths, function and field names, test names | the commit message |
| Measurements and the host they came from | the commit message or the linked backlog item or issue |
| Every edge case and its rule | the user-facing docs |
| Migration steps longer than a sentence | the docs, linked from the entry |
| Design history and rejected alternatives | the spec or backlog item |

Name a command, flag or field in an entry only when the reader types it or
sees it. Link to the docs or issue when the detail matters to a reader.

Before and after, from the refine `Host` check entry (135 words down
to 38):

> **Before:** The refine server answers only requests addressed to a
> loopback name: refine checked the `Origin` of changes but nothing on
> reads, so a web page you visited during a refine session could rename
> its own domain to 127.0.0.1 (DNS rebinding) and read every API response:
> the host inventory, configuration contents and, with `reveal=true`, any
> preserved password hashes. Refine now refuses, with 403, any request
> whose `Host` is not `127.0.0.1`, `localhost` or `[::1]`, on any port, so
> the UI and API still load through an SSH forward to a different local
> port. The check covers the page and its static files as well as the API.
> (the entry continues; the quote is cut short)

> **After:** **Refine rejects requests not addressed to a loopback name**:
> a web page open in your browser can no longer read the refine API,
> including preserved password hashes, through DNS rebinding. SSH forwards
> to another local port still work.
