# End-to-End Tests

Playwright e2e tests for the inspectah refine web UI.

## Prerequisites

1. **Playwright browsers** must be installed:

   ```bash
   cd inspectah-web/ui
   npx playwright install chromium
   ```

2. **A running `inspectah refine` server** with a valid scan tarball:

   ```bash
   # Build the binary first
   cargo build -p inspectah-cli

   # Start the server (uses port 8642 by default)
   ./target/debug/inspectah refine --open=false <path-to-tarball.tar.gz>
   ```

   The tarball must be a valid inspectah scan output (`.tar.gz`) containing
   an `inspection-snapshot.json` with package and config data.

## Test Data

For CI, place a minimal test tarball at `testdata/e2e-fixture.tar.gz`. This
tarball should contain a valid `inspection-snapshot.json` with at least:

- A few packages (some included, some excluded)
- A few config file entries
- Enough data to exercise toggle/undo/redo workflows

To create a fixture tarball from a real scan:

```bash
# Run a scan on a RHEL system
inspectah scan -o /tmp/scan-output.tar.gz

# Copy it to testdata/
cp /tmp/scan-output.tar.gz testdata/e2e-fixture.tar.gz
```

Alternatively, construct a minimal `inspection-snapshot.json` by hand and
package it:

```bash
mkdir -p /tmp/e2e-fixture
# Create a minimal snapshot JSON (see testdata/golden/ for schema examples)
tar czf testdata/e2e-fixture.tar.gz -C /tmp/e2e-fixture .
```

## Running Tests

Tests assume the server is already running on `http://127.0.0.1:8642`.

```bash
# Run all e2e tests (headless)
npm run test:e2e

# Run with browser visible
npm run test:e2e:headed

# Run a specific test file
npx playwright test e2e/smoke.spec.ts

# Run with Playwright UI mode
npx playwright test --ui
```

## Test Files

| File               | Coverage                                         |
| ------------------ | ------------------------------------------------ |
| `smoke.spec.ts`    | Page load, sidebar sections, stats bar rendering |
| `triage.spec.ts`   | Toggle, undo, redo, export tarball download      |
| `keyboard.spec.ts` | j/k nav, Space toggle, /, Ctrl+K, ?, Escape     |
| `responsive.spec.ts` | Hamburger at 1024px, sidebar at 1280px, panel toggle |
| `a11y.spec.ts`     | axe-core WCAG audit, keyboard accessibility      |

## CI

The GitHub Actions workflow installs Chromium, builds the binary, starts the
server in the background with a test tarball, waits for `/api/health` to
respond, runs the tests, and kills the server. See
`.github/workflows/rust-ci.yml` for details.
