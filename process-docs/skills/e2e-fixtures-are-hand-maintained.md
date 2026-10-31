---
name: e2e-fixtures-are-hand-maintained
description: The Playwright e2e fixtures under crates/web/ui/e2e/fixtures/ are hand-written JSON, not generated from the server's real DTOs, so they cannot drift-detect a server change — and `npm run test:e2e` can report green while its real-server coverage silently skips.
---

# The e2e Fixtures Are Not Contract Tests

`crates/web/ui/e2e/fixtures/{single-host,aggregate,empty}/*.json` are
hand-maintained JSON files that `e2e/helpers/mock-api.ts` serves back
through `page.route()` in place of a real refine server. Nothing
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
   `copyMap` depends on. Full write-up:
   `workflow/backlog/inspectah-e2e-fixtures-describe-an-api-the-server-does-not-serve.md`
   (FKA repo).
2. **A whole route can be missing from the mock map and nothing notices
   until the suite is actually run.** `/api/groups` has never been in
   `PRESET_ROUTE_MAP`, on this branch or on `main`. Nothing fetched
   through it fails loudly in review — it fails at runtime, when
   `useGroups`'s fetch falls through to the real network, `vite.config.ts`
   proxies it to a refine server that usually isn't running, and the
   resulting empty response makes `App.tsx` show "Failed to load" instead
   of the UI under test. On this repo that one gap was enough to fail 50
   of 76 Playwright tests. Full write-up:
   `workflow/backlog/inspectah-e2e-mock-api-missing-groups-route.md`
   (FKA repo).

**The compensating control named in `fixture_structure_test.rs`'s header
is nominal.** That file says Rust-type compatibility is "proven by
[the] real-server smoke tests" — `e2e/smoke-integration.spec.ts`. Those
tests `test.skip` every case in `beforeAll` when nothing answers
`http://127.0.0.1:8642/api/health`, which is the normal state for a
local run or a CI runner with no live refine server. **A skipped suite
reports green.** `npm run test:e2e`'s summary line does not distinguish
"6 passed, 0 skipped" from "0 passed, 6 skipped" at a glance — read the
`skipped` count in the output explicitly, every time, and report it
separately from `passed`. Treat a 100%-skipped file as a different fact
than a 100%-passed one, not as the same green.

## See Also

- `process-docs/skills/full-workspace-verification-gates.md` — the
  adjacent gotchas in running `cargo clippy`/`cargo test` at full
  strictness, and telling a slow run from a hung one.
