# End-to-End Tests

Playwright e2e tests for the inspectah refine web UI. Run every command
below from `crates/web/ui`.

## How the Suite Runs

Playwright starts the Vite dev server on port 5174 itself
(`playwright.config.ts`), so no server needs to be running for most specs.

- **Mocked specs** (every spec but one) answer `/api/*` from the JSON
  fixtures under `e2e/fixtures/`, served by `e2e/helpers/mock-api.ts`
  through `page.route()`. The fixtures are written by hand; see
  `process-docs/skills/e2e-fixtures-are-hand-maintained.md` before
  changing one.
- **`smoke-integration.spec.ts`** talks to a real `inspectah refine`
  server on `http://127.0.0.1:8642`, through the Vite proxy. Without one
  its tests skip on a local run and fail with `CI` set.

## Running Tests

```bash
npx playwright install chromium   # once

npm run test:e2e                  # whole suite, headless
npm run test:e2e:headed           # with a browser window
npx playwright test e2e/groups.spec.ts
npx playwright test --ui
```

To include the real-server spec, start refine over the committed fixture
from the repository root first:

```bash
cargo build -p inspectah-cli
./target/debug/inspectah refine --open=false testdata/e2e-fixture.tar.gz < /dev/null &
```

Restart it between runs: the smoke spec changes its timeline, and refine
saves a session file next to the tarball that the next start reads. The
skill file above has the details.

## CI

`.github/workflows/rust-ci.yml` installs Chromium, builds the binary,
starts refine over `testdata/e2e-fixture.tar.gz`, waits for `/api/health`
and runs the whole suite as a blocking step. With `CI` set the suite runs
on one worker and retries a failed test once to record a trace; a test
that passes only on its retry still fails the run (`failOnFlakyTests`).
Run it the same way locally with `CI=1 npx playwright test`.
