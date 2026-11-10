---
title: Testing
parent: Contributing
nav_order: 3
---

# Testing

The testing contract: what must pass before a pull request, when a real
host is required, when a browser pass is required, and what a green
result does not prove. The commands, with their setup, are in the
[Developer Guide § Run the gates](developer-guide.md#run-the-gates).

Two gate sets are named below. The **Rust gates** are `cargo fmt --check`,
`cargo clippy --workspace --all-targets -- -D clippy::all` and
`cargo test --workspace`, from the repo root. The **web gates** are
`npm run test`, `npm run build` and `npm run test:e2e`, from
`crates/web/ui` after `npm ci`.

## Decide what your pull request needs

Use the first row that matches any file in your pull request. If two
rows match different files, do everything both rows require. A pull
request that claims a generated build succeeds requires a real-host run,
whatever row it matches.

| Your PR changes | Local gates | Real-host run | Browser pass |
|---|---|---|---|
| Test code only: every changed line is in a `tests/` directory, a `#[cfg(test)]` module, `crates/web/ui/e2e/`, a `*.test.ts` or `*.test.tsx` file, a fixture under `testdata/` or `crates/web/ui/e2e/fixtures/`, or a `.snap` file | The gate set for the files' area: Rust gates for `.rs`, `.snap` and `testdata/` files, web gates for files under `crates/web/ui/`. The hook fires only on staged added, copied, modified, or renamed `.rs`, `Cargo.toml`, `Cargo.lock` and `build.rs` paths, so run the rest by hand | No | No |
| `Cargo.toml` or `Cargo.lock` only | Rust gates | Only when it changes a dependency of `crates/collect/`, of the Containerfile renderer in `crates/pipeline/`, or of redaction | No |
| Non-test code in a required area: `crates/collect/`, the Containerfile renderer, either export path, redaction, `/var` backing, or SELinux replay (§ Real-host runs lists the files) | Rust gates | Yes | No, unless a web row also matches |
| A web DTO or handler under `crates/web/src/` | Rust gates and web gates | Yes when it touches the refine export download (`crates/web/src/handlers.rs`, the tarball export); otherwise no | Yes, when the UI consumes what changed |
| Other Rust code under `crates/` | Rust gates | No | No |
| `crates/web/ui/src/` | Web gates | No | Yes when it alters what renders, how a control behaves, or any keyboard or accessibility behavior. No for a presentation-only change that alters none of those |
| Docs only: `docs/`, `process-docs/`, `README.md`, `CHANGELOG.md`, `completions/` | None locally. CI still runs the full job on the pull request | No | No |

## Gates before a pull request

| Gate | Command | Catches | Hook runs it? | CI runs it, and does it block? | Required when |
|---|---|---|---|---|---|
| Format | `cargo fmt --check` | Formatting drift | Only on the trigger below | Yes, blocks (as `cargo fmt --all -- --check`) | Every PR that adds, copies, modifies, or renames a `.rs` file (build scripts included), a `Cargo.toml`, or `Cargo.lock`. A manifest-only or lockfile-only PR is in scope |
| Clippy | `cargo clippy --workspace --all-targets -- -D clippy::all` | Lint in every target, tests included, and compile errors. When the web crate's build script reruns, a TypeScript error surfaces here too, as `npm run build failed` from `crates/web/build.rs` | Only on the trigger below | Yes, blocks | Same as Format |
| Rust tests | `cargo test --workspace` | Unit and integration tests, insta snapshots, every tarball under `testdata/` loading at the current schema, the e2e fixture JSON structure, and a refine server driven over HTTP on an ephemeral port | Only on the trigger below | Yes, blocks | Same as Format, plus any change under `testdata/` |
| Vitest | `npm run test` | Web UI unit tests, in jsdom | Never | Yes, blocks | Every PR that touches `crates/web/ui/`, and every PR that changes a web DTO or handler under `crates/web/src/`, because the hook does not run them |
| Type check and bundle | `npm run build`, which is `tsc && vite build` | TypeScript errors under `crates/web/ui/src/` | Indirectly: `crates/web/build.rs` runs it inside the clippy and test gates when the UI source or build files changed since the last build. A mixed Rust and UI commit gets it; a frontend-only commit does not | Indirectly, the same way, inside the clippy, test, and build steps. There is no separate step | Same as Vitest |
| Playwright e2e | `npm run test:e2e` | The UI in a real Chromium against a mocked API, plus one spec against a real refine server when one answers on `127.0.0.1:8642` | Never | Runs, does not block: the Playwright step has `continue-on-error: true`, and the run summary reports pass or fail | Same as Vitest. Report the skipped count separately from the passed count, every time |

**The hook's trigger.** `.githooks/pre-commit` runs
`git diff --cached --name-only --diff-filter=ACMR` and matches the result
against `\.(rs)$|Cargo\.(toml|lock)$|build\.rs$`. When nothing matches it
exits 0 without running any cargo gate. Before that test it always checks
staged tarballs for preserved sensitive content; see the developer guide,
§ Fixture tarballs are local-only. A commit that only deletes such files
matches nothing (`D` is not in the filter), so the hook skips it and you
run the three Rust gates by hand.

**CI's trigger.** Every pull request to `main` runs the full job. Direct
pushes to `main` run it only when a listed path changes; a docs-only
push runs nothing.

## Real-host runs

A real-host run is required for a change to: an inspector or any
non-test file under `crates/collect/`; the lines the Containerfile
renderer emits; either export path (the CLI scan tarball or the refine
web download); redaction; `/var` backing; SELinux replay; and any claim
that a generated build succeeds. It is not required for presentation-only
web UI changes with no DTO change, for docs, or for test-only changes.

Where each area lives in the tree:

| Area | Files |
|---|---|
| Inspectors and collection | Every non-test file under `crates/collect/src/`: the inspectors, `executor/real.rs`, `baseline.rs` |
| Containerfile renderer lines | `crates/pipeline/src/render/containerfile.rs` and the modules it calls for lines, all under `crates/pipeline/src/render/`: `users.rs`, `unmanaged.rs`, `repoless.rs`, `repo_enabling.rs`, `selinux.rs`, `var_dirs.rs`, `service_intent.rs`, `language_packages.rs`, `safety.rs`. The report, advisory, audit, readme, and kickstart renderers in the same directory are not Containerfile lines |
| Export paths | The CLI scan tarball: `crates/cli/src/commands/scan.rs`, `crates/pipeline/src/render/mod.rs`, `crates/pipeline/src/render/configtree.rs`, `crates/pipeline/src/render/tarball.rs`. The refine web download: `crates/web/src/handlers.rs` (the tarball export) and `crates/refine/src/session.rs` (export and render). The two paths stage different trees, so a check of one does not cover the other |
| Redaction | `crates/pipeline/src/redaction/`, `crates/core/src/redaction.rs`, `crates/core/src/types/redaction.rs`, and the export-time scrubbers in `crates/pipeline/src/render/language_packages.rs` and `crates/refine/src/session.rs` |
| `/var` backing | `crates/collect/src/inspectors/storage.rs`, `crates/pipeline/src/render/var_dirs.rs`, the tmpfiles and `/var` sections of `crates/pipeline/src/render/containerfile.rs`, `crates/refine/src/projection/reference.rs` |
| SELinux replay | `crates/collect/src/inspectors/selinux.rs`, `crates/pipeline/src/render/selinux.rs`, the SELinux section of `crates/pipeline/src/render/containerfile.rs` (booleans and port labels included), and `crates/core/src/aggregate/merge.rs` (fcontext merge) |

**Test-only** means every changed line is inside test code: a `tests/`
directory, a `#[cfg(test)]` module, a fixture, or a snapshot file. A
helper in the same file that both test and non-test code use is not test
code.

**Precedence**, so each pull request gets one answer:

1. A change confined to `#[cfg(test)]` modules and other test code needs
   no real-host run, whatever file it is in.
2. A manifest-only change needs no real-host run unless it changes a
   dependency of a crate in a required area (`collect`, the renderer in
   `pipeline`, redaction), which counts as a change to that area.
   Metadata is `description`, `version`, `authors`, `[lints]` and the
   like; a `[dependencies]` entry is not metadata.
3. Otherwise the path rule applies: any non-test change in a required
   area requires the run, whatever the pull request says about its
   effect.

The first two rules override the third. Worked examples:

- A test-only edit inside `crates/collect/src/inspectors/nonrpm.rs`,
  every changed line inside a `#[cfg(test)]` module: no real-host run
  (first rule).
- The same file, a test-only edit plus one changed non-test line:
  real-host run required (third rule; the first rule needs every line to
  be test code).
- A metadata-only edit to `crates/collect/Cargo.toml`: no real-host run
  (second rule).
- Changing a `[dependencies]` entry in `crates/collect/Cargo.toml`, or
  bumping a crate `collect` depends on in `Cargo.lock`: real-host run
  required (second rule, dependency clause).
- A manifest-only PR to the root `Cargo.toml` that changes no dependency
  of a required area: no real-host run (second rule).
- A refactor under `crates/collect/` that claims no output change:
  real-host run required (third rule), because the run is what shows the
  output did not change.

Every case above still runs the local Rust gates. The run is scan,
refine, export, and `podman build`, against a host carrying synthetic
drift from the companion fixture tool, never a clean install: a clean
host produces a near-empty snapshot, which is a test that cannot fail.
The run stops at the build; nothing boots the image. The method and its
traps are in
[`process-docs/skills/real-host-validation-runs.md`](https://github.com/marrusl/inspectah/blob/main/process-docs/skills/real-host-validation-runs.md)
in the repository. This page links it and does not repeat it.

## Browser pass

A browser pass is required for any change under `crates/web/ui/src/`
that alters what renders or how a control behaves, any change to a web
DTO or handler the UI consumes, and any keyboard or accessibility change.
Unit tests and the e2e suite run against mocked APIs and do not
substitute for it.

The pass runs a real refine server on an ephemeral port:

```bash
cargo build -p inspectah-cli
./target/debug/inspectah refine --open false --port 0 testdata/e2e-fixture.tar.gz
```

It prints its origin:

```
Loading snapshot...
warning: snapshot has no redaction metadata. It may contain unredacted sensitive data.
Starting refine server on http://127.0.0.1:58727
Press Ctrl-C to stop.
```

`testdata/e2e-fixture.tar.gz` is enough to start the server; a snapshot
from a drifted host shows more of the UI. Open the origin, wait for the
page to settle, exercise the changed control by mouse and by keyboard,
and confirm the server's response to each action, including a refusal,
rather than the screen alone. Stop the server with Ctrl-C.

## What a green result does not prove

### The hook

- **A passing hook says nothing about a frontend-only, docs-only,
  fixture-only, snapshot-only, or deletion-only commit.** It exits 0
  before any gate unless a staged added, copied, modified, or renamed path
  matches its trigger. An edit to `crates/web/ui/e2e/fixtures/*.json`
  skips the hook, so `crates/web/tests/fixture_structure_test.rs`, which
  snapshots those files, runs only in CI or when you run `cargo test`
  yourself. A `.snap`-only commit after `cargo insta review` skips too.
- **The hook tests your working tree, not your commit.** It runs the
  gates on the files on disk and never stashes unstaged changes. A fix
  you forgot to `git add` makes the hook pass for a commit that fails. CI
  is the first check of what you actually committed.
- **The hook exists only if you installed it** with
  `git config core.hooksPath .githooks`, and `git commit --no-verify`
  bypasses it.

### CI

- **A green CI job can contain a red e2e suite.** The Playwright step has
  `continue-on-error: true` until the mock API covers `/api/groups`. The
  run summary says "Playwright e2e: FAILED, non-blocking"; the job's
  check mark does not. The smoke spec's CI-only failure when no server
  answers is inside that same non-blocking step, so even a refine server
  that never started does not fail the job today. Read the run summary's
  Playwright line, not the job status.
- **CI's "Start refine server" step goes green when the server never
  answers.** It polls `/api/health` for 30 seconds and falls through
  without failing. The line that holds is `cargo test`:
  `crates/refine/tests/testdata_tarballs_load.rs` loads every tarball
  under `testdata/` at the current schema, so a missing or stale fixture
  fails `cargo test`, which the hook runs only when a Rust or Cargo file
  is also staged and which CI's Test step runs on every pull request.
- **A retry pass looks the same as a clean pass.** Playwright retries
  once in CI, the `github` reporter shows no flake counts, and the HTML
  report is uploaded only when the step fails. Locally there are no
  retries, so a test can pass in CI and fail for you, or the reverse.

### The e2e suite

- **A skipped real-server spec is a green result.**
  `e2e/smoke-integration.spec.ts` skips every case when no refine server
  answers on `127.0.0.1:8642`; under CI it fails instead. Run on its own
  (`npx playwright test e2e/smoke-integration.spec.ts` from
  `crates/web/ui`) with nothing on that port, it exits 0 having run
  nothing:

  ```
  Running 6 tests using 1 worker

    -  1 [chromium] › e2e/smoke-integration.spec.ts:61:5 › Smoke integration (real server) › API endpoints › health endpoint returns valid response
  ...
    -  6 [chromium] › e2e/smoke-integration.spec.ts:140:5 › Smoke integration (real server) › UI interactions › viewed persistence across reload

    6 skipped
  ```

  Report the skipped count separately from the passed count, every time.
  Start the server first
  (`./target/debug/inspectah refine --open false testdata/e2e-fixture.tar.gz`)
  and the three API cases pass, but the UI-interaction half currently
  fails, as the next bullet says. Measured 2026-11-08, the spec on its own
  then exited 1 with 3 passed, 1 failed (the exclude and undo case), and 2
  did not run.
- **The full local suite is red today, with or without a server, and
  that is not your change.** Measured 2026-11-08: `npm run test:e2e`
  with nothing on port 8642 ended
  `49 failed, 6 skipped, 9 did not run, 18 passed` and exited 1; with the
  fixture server running it ended `51 failed, 11 did not run, 20 passed`
  and exited 1, and the smoke spec's UI-interaction half was among the
  failures. The exemption in CI is tracked against the `/api/groups` mock
  gap below. Until the suite is green, judge your change by the tests
  that touch it, and say in the pull request which ones you ran.

- **The e2e fixtures under `crates/web/ui/e2e/fixtures/` are hand-written
  JSON, not generated from the server's DTOs.** A DTO change updates them
  by hand. `fixture_structure_test.rs` snapshots them as untyped JSON, so
  it detects an edit to a fixture, not a change to the server type, and a
  passing e2e run does not show the DTO matches what ships.
- **An unmocked route falls through to whatever is listening on port
  8642.** `e2e/helpers/mock-api.ts` registers only its preset route map,
  and `vite.config.ts` proxies every other `/api/*` request to
  `127.0.0.1:8642`. A mocked test's result therefore depends on whether
  nothing, your own refine session, or CI's fixture server is on that
  port. `/api/groups` is not in the map, which is why most of the suite
  fails locally with no server.
- **The real-server smoke spec proves the server starts and responds,
  not that any output is right.** It runs against
  `testdata/e2e-fixture.tar.gz`, a minimal hand-built snapshot: four
  packages, three config entries, no host scan, no baseline. It is written
  to show that the server starts on a current-schema snapshot, that `/api/health`,
  `/api/view`, `/api/ops`, `/api/viewed` and `/api/tarball` answer, and
  that one toggle and its undo round-trip through the UI. It exercises no
  inspector and no other section, and checks the Containerfile preview
  only for being a string.
- **No blocking gate runs UI code in a real browser.** `cargo test` runs
  no browser code and vitest runs in jsdom. Playwright is the only
  browser execution, and it does not block.
- **The type check is narrower than "tsc".** `tsconfig.json` includes
  `src` only, so nothing under `crates/web/ui/e2e/` is ever type-checked:
  Playwright transpiles without checking and vitest excludes `e2e/`.
  `INSPECTAH_SKIP_UI` skips the UI build, and with it `tsc`, entirely.

### The real-host run

- **A successful `podman build` is not a boot.** The run stops at the
  build, and no gate or step boots the image.

## How long the gates take

Measured 2026-11-08 on an Apple M3 Pro (12 cores, 36 GB), macOS 27.2,
rustc 1.99.1: a clean build plus the full Rust suite is two to two and a
half minutes, and the hook on a realistic change is about a minute to a
minute and a half. Earlier figures, from 2026-10-31, are recorded in
[`full-workspace-verification-gates.md`](https://github.com/marrusl/inspectah/blob/main/process-docs/skills/full-workspace-verification-gates.md)
in the repository. A run that has not returned in a few minutes is hung,
not slow. Before waiting
longer, look for a `rustc` child:

```bash
ps -eo pid,etime,command | grep -E "[c]argo|[r]ustc"
```

A `rustc` child with a plausible elapsed time under a `cargo` parent is
compiling. No child and a parent at five minutes is a hang, most often a
command blocked on stdin. Periodic `cargo clean` is cheap maintenance:
about eight seconds, then about a minute to rebuild.

## Harness

This project is developed with AI assistance, and the core team keeps
its harness setup in a private team repository.
