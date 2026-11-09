---
name: e2e-fixtures-are-hand-maintained
description: The Playwright e2e fixtures under crates/web/ui/e2e/fixtures/ are hand-written JSON, not generated from the server's real DTOs, so they cannot drift-detect a server change — and `npm run test:e2e` can report green while its real-server coverage silently skips.
---

# The e2e Fixtures Are Not Contract Tests

The JSON files under `crates/web/ui/e2e/fixtures/` (`single-host/`,
`aggregate/`, `errors/`, `post-responses/`, `sequences/` and
`manifest.json`) are hand-maintained JSON that `e2e/helpers/mock-api.ts`
serves back through `page.route()` in place of a real refine server. Nothing
generates them from `build_web_sections`, `build_web_view`, or any other
real adapter output, and nothing deserializes them into the Rust
response types before trusting them.

**Treat them as test doubles you must update by hand whenever a DTO
changes, not as proof the DTO matches what ships.** They have already
drifted in two independent ways:

1. **Section ids disagree with the server.** `single-host/sections.json`
   lists nine section ids; `build_web_sections` emits eight, and only
   two names are shared between the two lists. `empty_reason` in the
   fixture is free-form prose; the server emits fixed tokens
   (`zero_drift`, `data_unavailable`) that `VersionChangesTable`'s
   `copyMap` depends on. The fixtures describe an API the server does
   not serve, so a green fixture-backed test says nothing about the
   real section list.
2. **A whole route can be missing from the mock map and nothing notices
   until the suite is actually run.** `/api/groups` has never been in
   `PRESET_ROUTE_MAP`, on this branch or on `main`. Nothing fetched
   through it fails loudly in review — it fails at runtime, when
   `useGroups`'s fetch falls through to the real network, `vite.config.ts`
   proxies it to a refine server that usually isn't running, and the
   resulting empty response makes `App.tsx` show "Failed to load" instead
   of the UI under test. With the suite at 76 tests, that one gap failed
   50 of them; at 82 tests on 2026-11-08 the local run fails 49 to 51
   with or without a fixture server on 8642, so the groups route is not
   the only cause (see `docs/contributing/testing.md`).

**The compensating control named in `fixture_structure_test.rs`'s header
is nominal.** That file says Rust-type compatibility is "proven by
[the] real-server smoke tests" — `e2e/smoke-integration.spec.ts`. Those
tests `test.skip` every case in a per-test `beforeEach` (the API
endpoints block and the UI interactions block) when nothing answers
`http://127.0.0.1:8642/api/health`, which is the normal state for a
local run. (With `CI` set the spec throws instead of skipping, so a dead
backend fails there; a local run stays quiet.) **A skipped suite
exits 0.** Playwright prints `N skipped` on its own line, but a run
with zero tests passed still exits 0 and looks green to anything that
checks only the exit code. Read the `skipped` line in the output
explicitly, every time, and report it separately from `passed`. Treat a 100%-skipped file as a different fact
than a 100%-passed one, not as the same green.

## See Also

- `process-docs/skills/full-workspace-verification-gates.md` — the
  adjacent gotchas in running `cargo clippy`/`cargo test` at full
  strictness, and telling a slow run from a hung one.
