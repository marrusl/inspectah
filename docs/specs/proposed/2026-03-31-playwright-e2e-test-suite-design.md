# Playwright E2E Test Suite for yoinkc

**Date:** 2026-03-31
**Status:** Proposed
**Author:** Kit (via brainstorm with Mark)

## Goal

Broad browser test coverage for both refine and architect interactive UIs, using Node Playwright with its own test directory, programmatically generated fixtures, and schema-version-gated caching.

## Scope

**In scope:**
- All interactive UI features in the refine report (summary dashboard, prevalence slider, variant selection, config editor, section navigation, include/exclude toggles, fleet popovers, theme switching, re-render cycle, keyboard navigation)
- All interactive UI features in the architect report (layer decomposition, package move, Containerfile preview, export, impact tooltips)
- Fixture generation from Pydantic models
- Cached fixtures with schema-version guard

**Out of scope:**
- Inspection pipeline (covered by pytest)
- CLI argument parsing (covered by pytest)
- Schema validation (covered by pytest)
- True e2e with real driftify → real inspect (separate concern)
- Drag-and-drop in architect (not yet implemented)

## Architecture

### Directory Structure

```
tests/e2e/
  package.json
  playwright.config.ts
  globalSetup.ts
  globalTeardown.ts
  generate-fixtures.py

  fixtures/
    fleet-3host.tar.gz          # cached, regenerated on schema change
    single-host.tar.gz          # cached
    architect-topology/         # 3 fleet tarballs for architect
      web-servers.tar.gz
      db-servers.tar.gz
      app-servers.tar.gz
    .schema-version             # triggers regeneration

  tests/
    # Refine specs
    summary-dashboard.spec.ts
    prevalence-slider.spec.ts
    variant-selection.spec.ts
    config-editor.spec.ts
    section-navigation.spec.ts
    include-exclude.spec.ts
    fleet-popovers.spec.ts
    theme-switching.spec.ts
    re-render-cycle.spec.ts
    keyboard-nav.spec.ts

    # Architect specs
    layer-decomposition.spec.ts
    package-move.spec.ts
    containerfile-preview.spec.ts
    export.spec.ts
    impact-tooltips.spec.ts
```

### Test Lifecycle

**globalSetup.ts:**
1. Check `.schema-version` against `yoinkc.schema.SCHEMA_VERSION` (read via `uv run python -c "from yoinkc.schema import SCHEMA_VERSION; print(SCHEMA_VERSION)"`)
2. If stale or missing: run `uv run python tests/e2e/generate-fixtures.py` to regenerate all fixtures
3. Start **three** servers via subprocess:
   - Refine (fleet): `uv run yoinkc refine <fleet-fixture.tar.gz> --no-browser --port <deterministic-port>`
   - Refine (single-host): `uv run yoinkc refine <single-host.tar.gz> --no-browser --port <deterministic-port+1>`
   - Architect: `uv run yoinkc architect <topology-dir> --no-browser --port <deterministic-port+2>`
4. Wait for each server's `/api/health` endpoint to respond before proceeding
5. Store URLs in named environment variables:
   - `REFINE_FLEET_URL` — fleet refine server (e.g., `http://localhost:9100`)
   - `REFINE_SINGLE_URL` — single-host refine server (e.g., `http://localhost:9101`)
   - `ARCHITECT_URL` — architect server (e.g., `http://localhost:9102`)

**Each *.spec.ts:**
- Reads server URL from environment (fleet refine, single-host refine, or architect)
- Opens the live UI
- Interacts with the real rendered report
- Asserts DOM state, embedded snapshot state, and re-render results

**globalTeardown.ts:**
- Kill all three servers
- Clean temp directories

### Isolation Strategy

**Playwright runs with `workers: 1` (serial execution).** Both refine and architect servers mutate shared state:
- Refine re-render rewrites the output directory in place (`shutil.rmtree` + `copytree`)
- Architect mutates in-memory topology when packages are moved

Parallel test execution against shared mutable servers produces flaky tests. Serial execution is the pragmatic choice for this test suite size (~15 spec files). If test count grows significantly, the isolation strategy can be revisited with per-worker server instances.

Within each spec file, tests that mutate state (re-render, package move) should restore state via page reload in `afterEach`, or accept sequential ordering within the file.

### Fixture Generation

`generate-fixtures.py` is a Python script that:
1. Builds `InspectionSnapshot` objects from Pydantic models (same approach as existing pytest fixtures)
2. Renders through the full pipeline (`html_report.render()`)
3. Packages output into tarballs
4. Writes `.schema-version` with current schema version

This ensures fixtures are always schema-current. Cached tarballs are used when `.schema-version` matches, avoiding regeneration overhead on every test run.

**Cache key:** The `.schema-version` file stores `yoinkc.schema.SCHEMA_VERSION` (the module-level constant, not the instance field default). Template or static asset changes that don't bump the schema version won't trigger regeneration — those are caught by the tests themselves failing against the new markup, at which point a manual `uv run python tests/e2e/generate-fixtures.py --force` regenerates.

## Fixture Data

### Fleet Fixture (3 hosts: web-01, web-02, web-03)

- **RPM packages:** Varying prevalence — some at 100% (3/3), some at 66% (2/3), some at 33% (1/3). Enough range that the prevalence slider produces visibly different counts at different thresholds.
- **Config files:**
  - `/etc/app.conf` — 2-way tie (equal prevalence, no auto-selected winner). Tests Compare button behavior.
  - `/etc/httpd/conf/httpd.conf` — 3-way tie (three distinct variants). Tests Display button behavior.
  - `/etc/nginx/nginx.conf` — clear winner (2/3 prevalence). Tests auto-selection and Compare against winner.
- **Services:** At least one service with variants (enabled on 2 hosts, disabled on 1). Tests service variant UI.
- **Triage mix:** Items producing automatic, fixme, and manual triage statuses. Tests summary card counts and section priority ordering.
- **Secrets:** At least one redacted secret. Tests secrets section rendering.
- **Item count:** Enough items per section that the section priority list has meaningful sorting (manual sections sort above review, above automatic).

### Single-Host Fixture

- Basic snapshot with no fleet metadata
- Tests 3-card summary layout (no prevalence slider, no prevalence badges, no variant UI)
- Minimal but complete — packages, configs, services

### Architect Fixture (3 fleet tarballs)

- **web-servers, db-servers, app-servers** — each a fully rendered fleet tarball
- **Package overlap:** ~10 packages shared across all three fleets (→ base layer), ~5 unique per fleet (→ derived layers)
- **Impact data:** At least one package with fan-out dependencies and turbulence score for impact badge tooltip testing
- Generated by the same `generate-fixtures.py` script

## Test Coverage

### Refine Specs (10 files)

| Spec File | Feature Area | Key Assertions |
|-----------|-------------|----------------|
| `summary-dashboard.spec.ts` | Summary tab | 4-card grid (fleet) / 3-card (single), correct counts, card labels, Needs Attention includes ties, "must fix" callout for ties, variant drift callout |
| `prevalence-slider.spec.ts` | Prevalence control | Cards update on drag, preview-state dashed border appears when slider deviates from threshold, disappears when returned, prevalence badges in section headers sync, returning to original value clears dirty state |
| `variant-selection.spec.ts` | Config variant workflow | 2-way tie shows Compare buttons, 3-way tie shows Display buttons, selecting a variant via radio persists through re-render, minority (non-default) selection sticks, selecting resolves the "must fix" count |
| `config-editor.spec.ts` | Config file editor | Open editor on a config file, edit content in CodeMirror, save, re-render reflects the edit in the Containerfile |
| `section-navigation.spec.ts` | Navigation | Priority list row click navigates to correct section tab, sidebar nav links work, active state updates correctly |
| `include-exclude.spec.ts` | Package/item toggles | Toggle package include checkbox, dirty state activates Re-render button, re-render reflects the toggle in the Containerfile |
| `fleet-popovers.spec.ts` | Fleet bar interaction | Click fleet bar opens PF6 popover with host breakdown, click outside closes, fleet bar gets active outline while open |
| `theme-switching.spec.ts` | Dark/light theme | Toggle theme, cards render correctly in both modes, badge text elements have computed color distinct from background (programmatic contrast check via `getComputedStyle`) |
| `re-render-cycle.spec.ts` | Re-render pipeline | Full cycle: make changes → click Re-render → spinner appears → toast on success → changes persist in new report. Error path: first toggle a package include to create dirty state, then corrupt `window.snapshot` via `page.evaluate` (e.g., delete a required section), then click Re-render — verify error toast appears with `pf-m-danger` class |
| `keyboard-nav.spec.ts` | Keyboard accessibility | Prevalence badge focusable via Tab + Enter navigates to summary, priority rows focusable + Enter navigates to section, variant radio buttons keyboard-accessible |

### Architect Specs (5 files)

| Spec File | Feature Area | Key Assertions |
|-----------|-------------|----------------|
| `layer-decomposition.spec.ts` | Layer display | Base and derived layers render with correct names, package counts match fixture expectations, shared packages in base, unique in derived |
| `package-move.spec.ts` | Package manipulation | Move a package between layers via the UI, layer package counts update, operation reflected in the data model |
| `containerfile-preview.spec.ts` | Containerfile output | Preview shows correct `dnf install` lines per layer, lines update after package moves, base Containerfile uses correct base image |
| `export.spec.ts` | Export functionality | Export button triggers download, exported content contains expected Containerfiles for each layer |
| `impact-tooltips.spec.ts` | Impact information | Hover on `.impact-badge` element shows `title` attribute text containing fan-out count and turbulence score. Target the badge span (class `impact-badge`), not the package name span (class `pkg-name`). Layer badges also carry `title` text with layer-level summary. |

## Server Management

Three servers started via `globalSetup.ts` using the actual yoinkc CLI:

```bash
# Refine server (fleet mode)
uv run yoinkc refine <fleet-fixture.tar.gz> --no-browser --port 9100

# Refine server (single-host mode)
uv run yoinkc refine <single-host.tar.gz> --no-browser --port 9101

# Architect server
uv run yoinkc architect <topology-dir> --no-browser --port 9102
```

**Key flags:**
- `--no-browser` is required — without it, refine blocks on `server_thread.join()` after opening a browser, which hangs the test setup
- Deterministic ports (9100-9102) rather than `--port 0` — refine's current port reporting logs the requested port before binding, so `--port 0` doesn't reliably surface the actual bound port. Deterministic ports are simpler and debuggable. If port conflicts become an issue, the implementation plan can add a port-discovery fix to refine as a prerequisite task.
- `uv run` is the invocation convention (consistent with the existing pytest workflow)

**Health check:** `globalSetup.ts` polls each server's `/api/health` endpoint (GET, expects `{"status": "ok"}`) with a timeout before marking setup complete.

**Note on refine statefulness:** Re-render rewrites the output directory in place. The server is NOT stateless across re-renders — a re-render by one test changes the report served to subsequent tests. This is why tests run serially (see Isolation Strategy above).

## Integration with Existing Tests

- Playwright tests are fully independent of the pytest suite
- `uv run --extra dev pytest` continues to run all Python tests
- `npx playwright test` (from `tests/e2e/`) runs all browser tests
- CI can run both in parallel since they don't share state
- A top-level Makefile target or script could run both: `make test` or `./run-all-tests.sh`

## Dependencies and Environment

### Node
- Node.js >= 18 (Playwright requirement)
- `@playwright/test` in `tests/e2e/package.json`
- Browser install: `npx playwright install chromium` (only Chromium needed — not testing cross-browser)

### Python
- Invoked via `uv run` (consistent with existing pytest workflow)
- `generate-fixtures.py` imports from the `yoinkc` package. `uv run` handles the Python path — the package is installed in the virtualenv via `uv sync`. Run from repo root: `uv run python tests/e2e/generate-fixtures.py`
- Server processes started via `uv run yoinkc refine|architect`

### Working Directory
- All commands assume repo root as working directory
- `playwright.config.ts` sets `testDir: './tests'` relative to `tests/e2e/`
- `generate-fixtures.py` outputs to `tests/e2e/fixtures/` relative to repo root

### CI Integration
```bash
# Full test run
uv run --extra dev pytest -q          # Python tests
cd tests/e2e && npm ci && npx playwright install chromium && npx playwright test  # E2E tests
```

## Future Extensions

As features land, new spec files are added:
- `drag-and-drop.spec.ts` — when architect drag-and-drop is implemented
- `blast-radius.spec.ts` — when blast radius scoring ships
- `one-command-inspect.spec.ts` — when remote inspect is implemented
