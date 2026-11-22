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
   until the suite is actually run.** An unmocked `/api/*` fetch falls
   through to `vite.config.ts`'s proxy to 8642, and with nothing there
   the empty response makes `App.tsx` show "Failed to load" instead of
   the UI under test. `/api/groups` was missing this way until
   2026-11-21 and failed about half the suite. `applyMockApi` now serves
   it on every preset from the recording
   `src/__tests__/fixtures/system-tuning/files-no-tuned.groups.json`,
   which `system_tuning_fixtures_test` holds to the real handler; the
   handler's answer does not depend on the session. A spec that wants a
   narrower sidebar still registers its own `page.route` after
   `applyMockApi`, and that one wins.
3. **A hand-written view row can crash the app.** Until 2026-11-21 the
   package and config rows in the single-host view fixtures carried the
   retired `include` bool instead of `disposition`, so `isIncluded` in
   `src/api/disposition.ts` threw inside `MainContent` and the page went
   blank a moment after the stats bar rendered. A `beforeEach` that
   waits for `.inspectah-statsbar` still passes; the next locator then
   times out. When a mocked spec times out on an element that should be
   there, capture `page.on("pageerror")` before anything else. The rows
   now carry `{ "kind": "actionable", "include": ... }`, the form
   `contract_snapshots__contract_view.snap` shows; the group fixtures
   carry the full `GroupInfo` field set from `grouped_view.snap`.

**A fixture edit changes a Rust snapshot.**
`crates/web/tests/fixture_structure_test.rs` snapshots several of these
files with insta, so adding a field to a view fixture breaks it. A
fixture-only commit is gated like any code commit, so the pre-commit
hook's `cargo test` refuses it while the snapshot is stale. After any
fixture edit, run the test yourself first and review the snapshot diff:

```bash
cargo test -p inspectah-web --test integration fixture_structure_test::
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
integration system_tuning_fixtures_test::` and read the diff. A key skipped when empty
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

## Running the Suite the Way CI Does

CI runs the mocked specs and `smoke-integration.spec.ts` in one step,
with `CI` set and a refine server on 8642 over
`testdata/e2e-fixture.tar.gz`. `CI` means one worker and one retry, and
`failOnFlakyTests`: the retry only records a trace, and a test that
passes on it still fails the run, so a summary line reading `1 flaky`
is a red run, not a green one. To reproduce it, from the worktree root:

```bash
cargo build -p inspectah-cli
./target/debug/inspectah refine --open=false testdata/e2e-fixture.tar.gz < /dev/null &
cd crates/web/ui && CI=1 npx playwright test
```

Two things bite a second run:

- **refine autosaves beside the tarball.** The run leaves
  `testdata/.inspectah-session-e2e-fixture.json` (gitignored), and the
  next `refine` over that tarball asks Resume, Start fresh or Quit. With
  stdin at `/dev/null` it reads that as Quit and exits, so nothing
  answers on 8642. Move the file aside before restarting the server. CI
  starts from a clean checkout and never sees it.
- **The smoke spec mutates the server.** Its UI half toggles a package
  and undoes it, so a second run against the same server starts from a
  different timeline. Restart the server between runs.

The suite takes about a minute in CI mode on a laptop. A run that takes
much longer is timing out on something, not running slowly.

## The Recorded Alternative

`crates/web/ui/src/__tests__/fixtures/system-tuning/` is the opposite
kind: `crates/web/tests/system_tuning_fixtures_test.rs` builds each
session with the collector-driven sysctl hosts, serves it through the
real router, and fails when a committed `<session>.<endpoint>.json`
differs from the body the handler returns. After a deliberate DTO
change, re-record and review the diff:

```bash
UPDATE_SYSTEM_TUNING_FIXTURES=1 cargo test -p inspectah-web --test integration system_tuning_fixtures_test::
```

Anything run-specific in a body has to be pinned at the input, not
scrubbed from the output: the fleet merge writes the wall clock into
`aggregate_meta.merged_at`, which `/api/health` reports, so the
aggregate session sets it to a constant before the session opens.
Prefer this pattern for any new fixture a frontend test loads. A
browser test that needs the packages view with real rows can serve one
of these recordings as `/api/view` and override only the fields it
varies, as `e2e/not-carried.spec.ts` does with `files-no-tuned`; its
`.groups.json` also gives the sidebar real section links to click. A
recording can also be taken after ops: `machine_state_host_with_overrides`
applies `SetInclude` to the session before recording, which gives
`e2e/machine-state.spec.ts` and the Vitest tests a real "after the op"
view to answer `/api/op` with.

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
  units section: the view has no unit rows and the sections lack
  `scheduled_tasks`.

## See Also

- `process-docs/skills/full-workspace-verification-gates.md` — the
  adjacent gotchas in running `cargo clippy`/`cargo test` at full
  strictness, and telling a slow run from a hung one.
