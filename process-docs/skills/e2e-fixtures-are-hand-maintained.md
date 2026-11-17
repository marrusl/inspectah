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
3. **The preset view crashes the app.** As of 2026-11-12 the package
   and config rows in `single-host/view.json` have no `disposition`, so
   `isIncluded` in `src/api/disposition.ts` throws inside `MainContent`
   and the page goes blank a moment after the stats bar renders. A
   `beforeEach` that waits for `.inspectah-statsbar` still passes; the
   next locator then times out. Capture `page.on("pageerror")` to see
   it. `e2e/export-warnings.spec.ts` works around it by serving the
   preset view with its item lists emptied, which is enough for a test
   that only needs the stats bar and the Export dialog.

**A fixture edit changes a Rust snapshot.**
`crates/web/tests/fixture_structure_test.rs` snapshots several of these
files with insta, so adding a field to a view fixture breaks it. The
pre-commit hook skips every cargo gate when no Rust or Cargo file is
staged, so a fixture-only commit lands with that test red, as one did
when `declined_generated` and `generated_timers` were added. After any
fixture edit, run it yourself and review the snapshot diff:

```bash
cargo test -p inspectah-web --test fixture_structure_test
```

**A new always-present view key goes into every view fixture.** The
single-host view fixtures (`single-host/view*.json`,
`post-responses/*/success.json` and `sequences/**`) carry every key
`ViewResponse` always serializes, `manual_links` and
`undetermined_installs` among them, and so do the system-tuning
recordings that `system_tuning_fixtures_test` compares against. A key
added to the response and not to both goes red in
`fixture_structure_test` or `system_tuning_fixtures_test`. Add it to the
hand-written fixtures by script, then re-record the system-tuning ones
with `UPDATE_SYSTEM_TUNING_FIXTURES=1 cargo test -p inspectah-web --test
system_tuning_fixtures_test` and read the diff. A key skipped when empty
or absent (`default_target`, a unit row's `also`) needs neither.

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

## The Recorded Alternative

`crates/web/ui/src/__tests__/fixtures/system-tuning/` is the opposite
kind: `crates/web/tests/system_tuning_fixtures_test.rs` builds each
session with the collector-driven sysctl hosts, serves it through the
real router, and fails when a committed `<session>.<endpoint>.json`
differs from the body the handler returns. After a deliberate DTO
change, re-record and review the diff:

```bash
UPDATE_SYSTEM_TUNING_FIXTURES=1 cargo test -p inspectah-web --test system_tuning_fixtures_test
```

Anything run-specific in a body has to be pinned at the input, not
scrubbed from the output: the fleet merge writes the wall clock into
`aggregate_meta.merged_at`, which `/api/health` reports, so the
aggregate session sets it to a constant before the session opens.
Prefer this pattern for any new fixture a frontend test loads. A
browser test that needs the packages view with real rows can serve one
of these recordings as `/api/view` and override only the fields it
varies, as `e2e/not-carried.spec.ts` does with `files-no-tuned`; its
`.groups.json` also gives the sidebar real section links to click.

**The recorded units bodies are the other kind of recording.**
`e2e/systemd-units.spec.ts` and the units Vitest tests load four bodies
taken from one refine session over a driftify kitchen-sink scan, not
written by hand: `single-host/view-units-kitchen-sink.json` (the view
with every list outside the six unit fields emptied),
`single-host/sections-units-kitchen-sink.json` (the sections body cut to
`units` and `scheduled_tasks`),
`sequences/units-exclude-tmp-mount/01-after-exclude.json` (the response
to excluding `tmp.mount`) and `single-host/view-units-stock10.json` (the
view with a stock host's unit fields). Rules for taking or changing one:

- Check the body before committing, and leave out every row that names
  an account of the scanned machine. In a sections body these are an `at`
  job (its owner and home directory), a per-user crontab (its spool path
  `var/spool/cron/<account>`) and the timer generated from that crontab;
  the account is in the `id`, `title`, `subtitle`, `detail` or
  `searchable_text`. The sensitive-tarball hook checks tarballs only, so
  nothing else catches it.
- `/api/health` returns an empty hostname at schema 28, so a test that
  searches for the host name needs the scan's output directory name.
- `view-units-stock10.json` is coherent for the units section only: it
  keeps another view's Containerfile preview and stats, so a test of
  anything else must not read it.
- The preset `single-host/view.json` and `sections.json` cannot serve the
  units section: the view's rows crash the app and the sections lack
  `scheduled_tasks`.

## See Also

- `process-docs/skills/full-workspace-verification-gates.md` — the
  adjacent gotchas in running `cargo clippy`/`cargo test` at full
  strictness, and telling a slow run from a hung one.
