# Go-Native Port Design

## Summary

Port inspectah from a two-component architecture (Go CLI wrapper + Python container) to a single Go binary. All domain logic moves from `src/inspectah/` (Python) into `cmd/inspectah/internal/` (Go). Templates migrate from Jinja2 to pongo2 (Jinja2-compatible Go engine). Static assets embed into the binary via `go:embed`. The container image is retired.

**Motivation:** The current split creates version coordination overhead (Go CLI at 0.2.2, Python at 0.5.0), requires a container runtime on target hosts, and signals "prototype" to an audience that expects single-binary CLI tools (`podman`, `oc`, `kubectl`). A Go binary eliminates installation friction and removes the container-runtime dependency for the tool itself (podman remains a runtime dependency for `scan` baseline subtraction and `build`).

**Scope:** Full port of all features. Includes all 11 inspectors, all renderers (Containerfile, HTML report, audit, secrets review, kickstart, merge notes, readme), baseline subtraction, refine engine, fleet merge, and the architect web UI. Cross-cutting behaviors (system-type detection, RPM preflight, redaction, subscription bundling, tarball packaging) are explicit port scope.

## Architecture

### Repository structure (post-port)

```
cmd/inspectah/
├── main.go                    # entry point, version stamping via ldflags
└── internal/
    ├── cli/                   # cobra commands (scan, refine, architect, build, fleet, version)
    ├── inspector/             # 11 inspector modules
    ├── schema/                # Go structs replacing Pydantic models
    ├── baseline/              # base image diffing (RPM, services, presets)
    ├── pipeline/              # orchestration: system-type detection, preflight, redaction, tarball packaging
    ├── renderer/              # all renderers
    │   ├── containerfile/     # per-domain Containerfile rendering modules
    │   └── templates/         # pongo2 templates (embedded via go:embed)
    ├── refine/                # interactive tarball refinement
    ├── architect/             # analyzer, loader, export, HTTP server
    │   └── static/            # CodeMirror JS, PatternFly CSS, HTML (embedded)
    ├── fleet/                 # multi-host scan aggregation and merge
    ├── build/                 # existing cross-arch build support
    ├── version/               # existing version management
    └── platform/              # existing platform detection
```

Note: `internal/pipeline/` is new — it owns the cross-cutting orchestration logic that currently lives in `pipeline.py`, `system_type.py`, `rpm_preflight.py`, and scattered across `__main__.py`. This makes these behaviors first-class port targets rather than implicit plumbing.

### Key technology decisions

- **pongo2** for template rendering — Jinja2-compatible Go engine, minimizes template rewrite effort
- **go:embed** for all static assets — templates, PatternFly CSS, CodeMirror JS baked into binary
- **Command-slice cutover** (not module-slice) — each CLI command stays fully container-backed until its entire Go pipeline is ready. No mixed Go/Python data flow within a single command execution. See Port Strategy for details.

### CLI changes post-port

The wrapper-era container management surface is fully removed. No compatibility stubs — these surfaces have no meaning without a container image.

| Surface | Current location | Action |
|---------|-----------------|--------|
| `inspectah image` subcommand (`pull`, `pin`, `info`) | `cli/image.go` | **Remove.** |
| `--image` global flag | `cli/root.go` PersistentFlags | **Remove.** |
| `--pull` global flag (image pull policy) | `cli/root.go` PersistentFlags | **Remove.** |
| `INSPECTAH_IMAGE` env var | `container.ResolveImage()` in `cli/root.go` PersistentPreRun | **Remove.** Stop reading this env var. |
| Pinned-image config file (`~/.config/inspectah/image`) | `container/image.go` (`SavePinnedImage`, `LoadPinnedImage`) | **Remove.** Delete persistence code. |
| `ResolveImage()` resolution chain | `cli/root.go` PersistentPreRun | **Remove.** The entire image resolution chain (flag → env → pinned config → default) is eliminated. |
| Error/help text referencing `inspectah image update` / `inspectah image info` | `errors/translate.go`, help strings | **Remove.** Replace with version-appropriate messaging (e.g., "upgrade inspectah" instead of "run inspectah image update"). |
| `internal/container/` package | All container lifecycle code | **Remove** in merge commit (used during port, deleted at cutover). |

All removals happen in the merge commit (cutover step 6-7). During the port on `go-port`, the container surface stays functional for unported commands.

## Port Strategy

Module-by-module on a `go-port` feature branch. Port in dependency order, validate parity at each step via golden-file testing.

### Guiding principle: reimplement, don't translate

The Go port matches the *behavior* of the Python code, not its *structure*. The golden files are the behavioral contract — they define what each module must produce. The Python source is reference for understanding intent, not a blueprint to transliterate line by line.

Write idiomatic Go that produces the same outputs. If Go's strengths suggest a different internal approach (different data flow, different decomposition, fewer abstractions), take it. The test is: does the output match the golden files? Not: does the code look like the Python?

### Cutover model: command-slice

Each CLI command (`scan`, `refine`, `architect`, `build`, `fleet`) stays **fully container-backed** until its entire Go pipeline is ready — schema, inspectors, renderers, and cross-cutting behaviors for that command are all ported before the command switches to native execution. This avoids mixed Go/Python data flow within a single command and eliminates the need for a cross-language serialization contract during the port.

During the port, unported commands still shell out to the container. The container image is pinned to a specific 0.6.x release tag on the `go-port` branch (not `:latest`) to ensure reproducible parity testing. The `DefaultImageRef` on the feature branch is changed from `:latest` to the pinned 0.6.x tag.

The command unlock order follows the phase table below. A command is "unlocked" (switched from container to native) when all its dependencies are ported and its golden-file and behavioral tests pass.

### Port order

| Phase | Modules | Rationale |
|-------|---------|-----------|
| 0 | Pre-port groundwork | Triage existing specs (move superseded specs to `implemented/` or mark them). Write a behavioral reference doc covering what each command does and what it outputs — pairs with golden files to define the reimplementation contract without relying on Python source as documentation. Pin container image to 0.6.x tag on feature branch. Update CI triggers for `go-port` branch. |
| 1 | Schema (Pydantic → Go structs), RPM spec rewrite | Everything depends on these types. Validate JSON serialization matches first. RPM spec rewrite validates the Go build pipeline end-to-end before porting domain logic. |
| 2 | Inspectors (all 11) + system-type detection + RPM preflight | Leaf nodes beyond schema. Mostly `os/exec` + string parsing. System-type detection and preflight are ported here because they gate inspector behavior (refusal on unknown ostree, base-image mapping, package availability filtering). Port one inspector at a time. |
| 3 | Baseline subtraction | Depends on schema + inspector output. Pulls base image RPM list via podman, diffs. |
| 4 | Renderers (Containerfile, HTML report, audit, secrets review, kickstart, merge notes, readme) + redaction + tarball packaging | Depends on schema. pongo2 templates + go:embed. Port Containerfile renderer first — most critical output. Redaction (before any output is written) and tarball packaging/naming are ported here because they are renderer-adjacent pipeline steps. **Unlocks `scan` command** — once schema, inspectors, baseline, renderers, and cross-cutting pipeline behaviors are all native, `scan` switches from container to Go. |
| 5 | Refine engine | Depends on schema + renderers. Stateful tarball manipulation. `--from-snapshot` and re-render behavior must match. **Unlocks `refine` command.** |
| 6 | Fleet merge | Depends on schema + inspectors. Multi-scan aggregation. **Unlocks `fleet` command.** |
| 7 | Architect (server, analyzer, loader, export) | Last — most complex, depends on everything. HTTP server with embedded assets. Loader enforces snapshot-version contract (see below). **Unlocks `architect` command.** |

`build` command is already Go-native (cross-arch build support in `internal/build/`). It stays native throughout.

### Architect snapshot-version contract

The architect loader and fleet merge logic consume snapshot JSON and tarballs as first-class interfaces. Today, `pipeline.py` hard-fails on schema-version mismatch, and `fleet/loader.py` validates merge compatibility, but `architect/loader.py` projects raw JSON without an explicit version gate.

The Go port enforces **same-major-version refusal**: the snapshot's `schema_version` field must match the binary's expected schema version. Mismatched snapshots produce a clear error: `"snapshot schema version X does not match inspectah version Y — re-scan the host with this version of inspectah."` This applies to architect, fleet, refine, and `--from-snapshot`.

No backward-compatibility window. Old tarballs from 0.6.x (Python era) are not loadable by 0.7.0 — users re-scan. This is acceptable because inspectah scans are cheap (seconds) and the Go port is a clean break.

### Cross-cutting parity checklist

Features that live outside the inspector/renderer split but are load-bearing for inspectah's migration contract:

| Feature | Current location | Disposition | Owning phase | Acceptance test |
|---------|-----------------|-------------|--------------|-----------------|
| System-type detection (package-mode / rpm-ostree / bootc / unknown-ostree refusal) | `system_type.py` | **Retain** | Phase 2 | Golden files for each system type + refusal case |
| Base-image mapping and `--no-baseline` degraded mode | `inspectors/__init__.py` | **Retain** | Phase 2 | Golden file for `--no-baseline` output |
| RPM preflight (package availability, `PreflightResult` propagation to Architect) | `rpm_preflight.py` | **Retain** | Phase 2 | Golden file for preflight-partial case affecting Containerfile + Architect inputs |
| Redaction before output | `pipeline.py` | **Retain** | Phase 4 | Behavioral test: secrets never appear in any output artifact |
| Subscription cert bundling | `subscription.py` | **Retain** | Phase 4 | Unit test |
| Post-render `--validate` | `pipeline.py` | **Retain** | Phase 4 | Behavioral test: validate flag produces expected output |
| Tarball packaging and naming conventions | `pipeline.py` | **Retain** | Phase 4 | Behavioral test: tarball inventory matches expected artifact list |
| Kickstart renderer | `renderers/kickstart.py` | **Retain** | Phase 4 | Golden file for kickstart output |
| GitHub push flow | `git_github.py` | **Retain** | Phase 4 | Unit test |

### Early validation: pongo2 template compatibility

Before porting renderers (Phase 4), run existing Jinja2 templates through pongo2 and diff against Python-rendered output. Surface syntax gaps and filter incompatibilities before committing to renderer port work. This is a risk reduction step — pongo2 covers most of Jinja2 but not all filters/tests.

## Versioning & Release Pipeline

### Version scheme

- **Python/container:** 0.6.x on `main` — available for fixes during the port
- **Go-native:** 0.7.0 — first release after the port merges to main
- **Version source:** `main.go` ldflags (`-X main.version`) + RPM spec `Version:` field
- **Git tags:**
  - `v0.6.x` — last Python/container release on `main` before port merges
  - `v0.6.x-final` — explicit "end of line" container image tag
  - `v0.7.0-rc1` — tagged on `go-port` branch for smoke testing (prerelease tag — must NOT trigger `build-image.yml`)
  - `v0.7.0` — tagged on `main` after merge

### Branch and tag rules during port

- `go-port` branch targets `go-cli.yml` CI. `build-image.yml` is suppressed on this branch (either via path filter or branch filter).
- `main` continues to receive 0.6.x fixes and container image builds normally.
- The `go-port` branch pins `DefaultImageRef` to the specific 0.6.x container tag (e.g., `ghcr.io/marrusl/inspectah:0.6.0`), not `:latest`.

### Prerelease promotion control

Exact rules for how RC and final tags interact with each workflow:

| Workflow | Trigger | RC tags (`v0.7.0-rc*`) | Final tags (`v0.7.0`) |
|----------|---------|------------------------|----------------------|
| `build-image.yml` | `push.tags` | **Must not fire.** Change tag filter from `v*` to `v[0-9]+.[0-9]+.[0-9]+` (no suffix). RC tags never produce container images. | Fires on `main` only (for 0.6.x final). After port merges, this workflow is deleted. |
| `go-cli.yml` | `push` (path-filtered) | Fires on `go-port` branch. Runs Go build + test + golden-file + behavioral tests. | Fires on `main` post-merge. |
| `package-release.yml` | `release.published` | **Must not fire.** Add an explicit final-only job guard: `if: ${{ !github.event.release.prerelease }}` on all jobs. GitHub's `published` event fires for prereleases too, so the workflow trigger alone is not sufficient. | Fires. Guard passes, builds all artifacts per promotion flow. |

**Prerelease gating:** `v0.7.0-rc1` is created as a GitHub prerelease. `v0.7.0` is created as a full published release. The `package-release.yml` workflow adds `if: ${{ !github.event.release.prerelease }}` to every job — this is an enforceable guard, not a convention. GitHub's `release.published` event fires for prereleases, so the trigger alone does not prevent RC artifacts from being built and published.

### CI during transition

Path-gated — both pipelines stay active, triggered by different file paths:

- `go-cli.yml` triggers on `cmd/`, `internal/`, `go.*` changes — primary CI on `go-port` branch. Must be updated in Phase 0 to also run on the `go-port` branch.
- `build-image.yml` triggers on `src/`, `Containerfile` changes — stays active on `main` for 0.6.x. Does not run on `go-port` branch.
- Golden-file parity tests and behavioral regression tests run on `go-port` as part of `go-cli.yml`.

### Cutover sequence

1. All modules ported, all golden-file and behavioral tests passing
2. Tag `v0.7.0-rc1` on `go-port` branch
3. Manual smoke test on a real RHEL system (full pipeline: scan → refine → render → architect)
4. Tag final container image `v0.6.x-final` on `main`, add deprecation notice to GHCR description
5. Merge `go-port` to `main`
6. Delete `src/inspectah/`, `Containerfile`, `build-image.yml`, `pyproject.toml`, `internal/container/` in the merge commit
7. Remove `inspectah image` subcommand and `--image` global flag
8. Tag `v0.7.0` on `main`
9. `package-release.yml` builds and publishes artifacts per the promotion flow below

### Artifact table

| Artifact | OS/Arch | Produced by |
|----------|---------|-------------|
| `inspectah-linux-amd64` | Linux x86_64 | `package-release.yml` |
| `inspectah-linux-arm64` | Linux aarch64 | `package-release.yml` |
| `inspectah-darwin-amd64` | macOS x86_64 | `package-release.yml` |
| `inspectah-darwin-arm64` | macOS Apple Silicon | `package-release.yml` |
| `inspectah-{version}.src.rpm` | Source RPM | `package-release.yml` |
| `inspectah-{version}.{dist}.{arch}.rpm` | Binary RPMs (COPR) | COPR build from SRPM |
| Homebrew formula | macOS (tap PR) | `package-release.yml` |

### Artifact promotion flow

1. **Build** — `go build` with `-trimpath -ldflags` for all OS/arch targets
2. **Checksum** — generate `SHA256SUMS` manifest for all binaries
3. **Sign** — GPG-sign the checksum manifest (`SHA256SUMS.sig`). Cosign signing of binaries is a future enhancement tracked separately.
4. **Verify** — CI step validates signature before publish
5. **Publish** — attach all binaries, checksums, and signature to GitHub Release. Submit SRPM to COPR. Open Homebrew tap PR.
6. **Deprecate legacy** — update GHCR container description with deprecation notice pointing to the binary release

### Runtime dependency contract

| Channel | Required dependencies | Optional dependencies | Notes |
|---------|----------------------|----------------------|-------|
| **RPM (COPR)** | `podman >= 4.4` | — | Podman required for `scan` (baseline subtraction) and `build`. RPM spec `Requires:` stays. |
| **Homebrew** | — | `podman` | Homebrew formula does not enforce podman. Documented as required for `scan`/`build` subcommands. |
| **Direct download** | — | `podman` | Binary runs standalone. `scan` and `build` print clear error if podman is not found. |

**Why podman is still required:** Baseline subtraction pulls the matching bootc base image via podman to diff RPM lists and service presets. The `build` subcommand invokes `podman build`. These are fundamental to inspectah's function — the Go port removes the *tool's* container dependency, not the *workflow's* podman dependency.

**Upgrade from 0.6.x:** Users who installed via COPR RPM get `inspectah` replaced (Go binary `Conflicts: python3-inspectah`). The container image is no longer pulled or used. `inspectah image` commands are removed — users who had pinned images will see a "command removed" message with guidance.

### Post-port distribution

- **COPR RPM** — spec rewrite required (Go `%gobuild` macros replace pip install). Do this during Phase 1.
- **Homebrew formula** — already exists, drops the container dependency
- **Direct download** — `curl -LO` the binary from GitHub releases
- **No container image.** No Python. No pip.

## Testing Strategy

### Four layers

**Unit tests** — per-module, table-driven Go tests with `testdata/` directories co-located with packages. Each inspector gets test fixtures (sample `/etc/` files, mock `rpm -qa` output, mock `systemctl` output). Tests call real Go code, not reimplementations.

**Golden-file integration tests** — parity safety net. Before porting each module, capture Python output on reference inputs. Go implementation must produce identical structured output. Golden files live in `testdata/golden/` and are committed. After Python code is deleted, golden files become the regression baseline.

Parity corpus (expanded beyond package-mode):
- A minimal RHEL 9 package-mode scan
- A layered/drifted system (driftify-generated)
- An `rpm-ostree` source system
- A `bootc` source system
- An unknown-ostree refusal case (system-type detection must hard-fail)
- A `--no-baseline` degraded-mode scan
- A preflight-partial case (missing packages alter Containerfile output and Architect inputs)
- Edge cases specific to each inspector

**Behavioral regression tests** — golden files validate static output; these validate workflows and command contracts. Ported from the Python test suite's behavioral coverage (not line-for-line translation):

| Behavior | What it validates |
|----------|-------------------|
| Snapshot JSON round-trip | `scan` produces snapshot, `--from-snapshot` reproduces identical output |
| Tarball inventory and naming | Output tarball contains exactly the expected artifacts with correct names |
| Refine re-render | `refine` modifies a tarball, re-renders produce correct updated output |
| Architect endpoints | HTTP API serves expected responses, loader handles snapshot schema |
| Kickstart cross-references | Containerfile comments and README point to kickstart artifact correctly |
| Preflight propagation | Preflight results flow through to Containerfile and Architect inputs |
| Redaction completeness | No secrets appear in any output artifact |
| CLI compatibility | Bare invocation defaults, flag parsing, error messages match expected behavior |

**End-to-end smoke test** — full pipeline: scan a real host → refine → render all outputs → architect serves and loads. Run manually before tagging `v0.7.0-rc1`. Automate post-port as a CI job against a test VM.

**Release-channel smoke matrix** — mandatory before publishing `v0.7.0`. Each distribution channel gets a slim verification pass:

| Channel | Smoke test |
|---------|-----------|
| Direct download (linux-amd64) | Download binary from GitHub Release → `chmod +x` → `inspectah version` → `inspectah scan --help` |
| Direct download (linux-arm64) | Same as above on arm64 host or QEMU |
| RPM install (clean) | `dnf install` from COPR → `inspectah version` → `inspectah scan --help` |
| RPM upgrade (0.6.x → 0.7.0) | Install 0.6.x RPM → `dnf update` from COPR → verify `inspectah image` prints "command removed" message → `inspectah scan --help` |
| Homebrew | `brew install` from tap → `inspectah version` → `inspectah scan --help` |
| Checksum verification | Download `SHA256SUMS` + `SHA256SUMS.sig` → `gpg --verify` → `sha256sum --check` against downloaded binary |

This matrix runs manually for `v0.7.0`. Automating it is a post-ship improvement.

### Out of scope

Python test suite (`tests/`) is not migrated line-for-line. It stays on `main` for 0.6.x. Go gets its own test suite, validated against golden files and behavioral regression tests. The behavioral tests are *inspired by* the Python suite's coverage but written idiomatically in Go.

## RPM Spec Changes

The existing `packaging/inspectah.spec` needs a full rewrite for the Go-native binary. Key changes:

- `BuildRequires: golang >= 1.21` (already present)
- Switch to `%gobuild` macros or keep the manual `go build` with ldflags (current approach)
- Drop any Python-related requires/build steps
- `Requires: podman >= 4.4` stays — still a runtime dependency for `scan` and `build`
- `Conflicts: python3-inspectah` stays until 0.6.x is fully EOL
- Shell completions generation stays (cobra's built-in completion support)

Do this during Phase 1 (alongside schema port) — it validates the Go build pipeline end-to-end before you start porting domain logic, and it's more work than it looks.
